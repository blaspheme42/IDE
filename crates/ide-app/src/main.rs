//! Binaire de l'IDE : boucle d'événements façon Elm (événement → update → rendu).

mod backup;
mod clipboard;
mod files;
mod git;
mod highlight;
mod lsp;
mod session;
mod terminal;
mod workspace;

use std::path::PathBuf;

use anyhow::Context;
use crossterm::event::{
    self as ct, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
    EnableMouseCapture, EventStream, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use futures::StreamExt;
use ide_core::{Command, Event, Key, KeyInput, Modifiers, MouseInput, MouseKind, State};
use tokio::sync::mpsc;
use tracing_appender::non_blocking::WorkerGuard;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let state_dir = state_dir()?;
    let _log_guard = init_logging(&state_dir)?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let panic_test = args.iter().any(|a| a == "--panic-test");
    // `ide [dossier|fichier]` : premier argument qui n'est pas une option.
    let arg = args
        .into_iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from);
    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .context("dossier courant inaccessible")?;
    let (root, file) = workspace::choose_root(arg.as_deref(), &cwd);
    // Trace les panics dans le log ; ratatui::init chaîne ce hook après la restauration.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic : {info}");
        disable_mouse_and_paste();
        previous_hook(info);
    }));
    // ratatui::init installe un hook de panic qui restaure le terminal (NF-08).
    let mut terminal = ratatui::init();
    if let Err(e) = crossterm::execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste)
    {
        tracing::warn!("activation souris / collage : {e}");
    }
    // Protocole clavier étendu (kitty, WezTerm, foot…) : sans lui, Ctrl+Shift+P arrive comme Ctrl+P et
    // Ctrl+Shift+M comme Entrée. Ailleurs, Ctrl+P puis `>` ouvre la palette de commandes.
    // shortcut: détection par variables d'environnement plutôt que par requête au terminal, qui attend
    // jusqu'à 2 s sa réponse (NF-01) ; élargir la liste si un terminal compatible manque.
    if kitty_protocol_terminal() {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        );
    }
    let result = run(&mut terminal, panic_test, root, file, started, state_dir).await;
    disable_mouse_and_paste();
    ratatui::restore();
    if let Err(e) = &result {
        tracing::error!("sortie sur erreur : {e:#}");
    }
    result
}

/// Ratatui ne connaît pas la souris : à appeler à la sortie et en cas de panic.
/// Terminal connu pour gérer le protocole clavier de kitty (désambiguïsation des Ctrl+Shift+…).
fn kitty_protocol_terminal() -> bool {
    let var = |k: &str| std::env::var(k).unwrap_or_default().to_lowercase();
    let (term, program) = (var("TERM"), var("TERM_PROGRAM"));
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var_os("WEZTERM_PANE").is_some()
        || ["kitty", "foot", "ghostty", "alacritty"]
            .iter()
            .any(|t| term.contains(t))
        || ["wezterm", "ghostty"].iter().any(|t| program.contains(t))
}

fn disable_mouse_and_paste() {
    let _ = crossterm::execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        // Sans effet si le protocole n'a pas été activé.
        crossterm::event::PopKeyboardEnhancementFlags
    );
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    panic_test: bool,
    root: PathBuf,
    file: Option<PathBuf>,
    started: std::time::Instant,
    state_dir: PathBuf,
) -> anyhow::Result<()> {
    let size = terminal.size()?;
    let mut state = State {
        width: size.width,
        height: size.height,
        ..State::default()
    };

    let (tx, mut rx) = mpsc::channel::<Event>(256);
    // Configuration d'abord (cachés, largeur du panneau, indentation), puis la racine :
    // l'arbre se charge pendant la lecture du fichier.
    let r = root.clone();
    let (mut settings, errors) =
        tokio::task::spawn_blocking(move || ide_workspace::load_config(&r)).await?;
    // Le dernier thème choisi passe avant la configuration : sinon `theme =` annulerait Alt+T.
    let theme_file = state_dir.join("theme");
    if let Some(theme) = session::load_theme(&theme_file) {
        settings.theme = theme;
    }
    let first_theme = settings.theme.clone();
    for e in &errors {
        tracing::warn!("configuration : {e}");
    }
    workspace::set_default_indent(settings.indent);
    // Appliquée tout de suite : le premier rendu a déjà le bon thème (pas de flash sombre).
    let config = Event::ConfigLoaded { settings, errors };
    ide_core::update(&mut state, config.clone());
    // Surveillance du disque (F-05, NF-09), lancée en fond : inscrire récursivement un gros arbre
    // (target/…) prend des centaines de ms, le premier rendu ne doit pas l'attendre (NF-01).
    let (watch_tx, watch_root) = (tx.clone(), root.clone());
    tokio::spawn(async move {
        let err_tx = watch_tx.clone();
        let started = tokio::task::spawn_blocking(move || {
            // Depuis le thread du watcher, `blocking_send` n'attend que lui, jamais la boucle.
            let _ =
                watch_tx.blocking_send(Event::GitBranch(ide_workspace::git_branch(&watch_root)));
            let root = watch_root.clone();
            ide_workspace::watch(&watch_root, move |paths| {
                let _ = watch_tx.blocking_send(Event::DiskChanged(paths));
                // shortcut: le watcher ignore .git/ ; la branche est relue à chaque changement de fichiers
                // (un checkout en modifie presque toujours), pas sur un simple changement de HEAD.
                let _ = watch_tx.blocking_send(Event::GitBranch(ide_workspace::git_branch(&root)));
            })
        })
        .await;
        match started {
            // Le watcher vit tant que cette tâche vit, c'est-à-dire jusqu'à l'arrêt du runtime.
            Ok(Ok(_watcher)) => std::future::pending::<()>().await,
            Ok(Err(e)) => {
                tracing::warn!("{e}");
                let msg = format!("rafraîchissement automatique désactivé : {e}");
                let _ = err_tx.send(Event::FsDone(Err(msg))).await;
            }
            Err(e) => tracing::warn!("watcher : {e}"),
        }
    });
    tx.send(Event::OpenRoot(root.clone())).await?;
    // Fichiers lus avant de lancer le lecteur : FileOpened passe avant toute touche.
    // Sans fichier en argument, la session (F-80) ; le fichier en argument en dernier, actif.
    let session_file = session::file(&state_dir.join("session"), &root);
    let sf = session_file.clone();
    let opened =
        tokio::task::spawn_blocking(move || {
            let (views, active) = match file {
                None => session::load(&sf),
                Some(_) => (vec![], None),
            };
            let mut paths: Vec<PathBuf> = views.iter().map(|v| v.0.clone()).collect();
            paths.extend(file);
            let mut events: Vec<Event> = paths.into_iter().flat_map(workspace::open_file).collect();
            events.extend(views.into_iter().map(|(path, cursor, scroll_line)| {
                Event::RestoreView {
                    path,
                    cursor,
                    scroll_line,
                }
            }));
            events.extend(active.map(Event::ActivateTab));
            events
        })
        .await?;
    // Copies de secours (NF-07) : notre dossier tout de suite, la lecture des copies des instances
    // mortes en fond (elle lit tous les .bak, NF-01) ; offres et fichiers arrivent après le premier rendu.
    let backup_root = state_dir.join("backup");
    let backup_dir = backup::claim(&backup_root);
    let own = backup_dir.clone();
    let mut scan = tokio::task::spawn_blocking(move || backup::scan(&backup_root, &own));
    let mut scanning = true;
    for ev in opened {
        tx.send(ev).await?;
    }
    // Erreurs de config renvoyées en dernier : « Ouvert : … » ne doit pas les masquer.
    if matches!(&config, Event::ConfigLoaded { errors, .. } if !errors.is_empty()) {
        tx.send(config).await?;
    }
    let mut reader = tokio::spawn(read_terminal_events(tx.clone()));

    let mut highlighter = highlight::Highlighter::default();
    highlighter.sync(&mut state);
    let mut git = git::GitMarks::default();
    git.sync(&mut state, &tx);
    let mut lsp = lsp::LspManager::new(tx.clone(), root.clone(), state.settings.lsp.clone());
    lsp.sync(&state);
    terminal.draw(|f| ide_ui::render(&state, f))?;
    tracing::info!(
        "premier rendu en {:?} (NF-01 : < 100 ms)",
        started.elapsed()
    );
    if panic_test {
        panic!("--panic-test : panic volontaire après le premier rendu");
    }

    // Une seule tâche écrit, dans l'ordre des Ctrl+S : une vieille version ne peut pas écraser une récente.
    let (save_tx, save_rx) = mpsc::unbounded_channel();
    let saver = tokio::spawn(save_worker(save_rx, tx.clone()));
    // Fenêtre du terminal fermée (SIGHUP), arrêt de session (SIGTERM) : dernière copie avant de sortir.
    let mut terminated = terminate_signal()?;
    let clip = clipboard::spawn(tx.clone());
    let (backup_tx, backup_rx) = mpsc::unbounded_channel();
    let (failures_tx, mut failures) = mpsc::unbounded_channel();
    let backup_worker = tokio::spawn(backup::worker(backup_rx, failures_tx));
    let send_backup = |jobs: Vec<backup::Job>| {
        for job in jobs {
            let _ = backup_tx.send(job);
        }
    };
    let mut backups = backup::Backups::new(backup_dir);
    let mut backup_tick = tokio::time::interval(backup::PERIOD);
    backup_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut term = terminal::Term::new(root.clone());
    let output = term.output();
    // Sortie du shell : au plus un rendu par trame, même sous un flot continu.
    let mut frame = tokio::time::interval(std::time::Duration::from_millis(16));
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut quitting = false;
    let result = async {
        loop {
            // Copies trouvées au démarrage : proposées une à une, avant les événements suivants.
            let ev = match backups.next_offer(&state) {
                Some((ev, adopt)) => {
                    send_backup(adopt);
                    ev
                }
                // On garde un `tx` pour les sauvegardes, donc `recv` ne finit jamais : on surveille la fin du lecteur.
                None => tokio::select! {
                    biased;
                    _ = &mut terminated => {
                        tracing::warn!("signal de fin : dernière copie de secours, puis sortie");
                        break;
                    }
                    _ = backup_tick.tick() => {
                        while let Ok(failure) = failures.try_recv() {
                            backups.failed(failure);
                        }
                        send_backup(backups.tick(&state));
                        continue;
                    }
                    Some(ev) = rx.recv() => ev,
                    // `tick` d'abord : un futur annulé ne consomme pas le permis de `notified`.
                    _ = async { frame.tick().await; output.notified().await } => {
                        // Pas d'`update` hors fin du shell : `scroll_to_cursor` annulerait une vue
                        // déplacée à la molette.
                        let open = state.terminal.open;
                        term.sync(&mut state);
                        if open != state.terminal.open {
                            // Panneau refermé : la zone visible de l'éditeur a grandi.
                            highlighter.sync(&mut state);
                        }
                        terminal.draw(|f| ide_ui::render(&state, f))?;
                        continue;
                    }
                    // Après `rx` : la session et le fichier en argument sont ouverts avant, l'onglet
                    // actif rendu par `open_missing` est le bon.
                    found = &mut scan, if scanning => {
                        scanning = false;
                        match found {
                            Ok(offers) => {
                                backup::open_missing(&state, &offers, tx.clone());
                                backups.add_offers(offers);
                            }
                            Err(e) => tracing::error!("lecture des copies de secours : {e}"),
                        }
                        continue;
                    }
                    _ = &mut reader => break,
                },
            };
            let mut quit = false;
            let saved = match &ev {
                Event::Saved { path, .. } => Some(path.clone()),
                _ => None,
            };
            git.on_event(&ev);
            let cmds = ide_core::update(&mut state, ev);
            // didChange de la version éditée avant toute requête LSP qui la vise.
            lsp.sync(&state);
            if let Some(path) = saved {
                let docs = std::iter::once(&state.doc).chain(state.tabs.iter().map(|t| &t.doc));
                if docs
                    .filter(|d| d.path.as_ref() == Some(&path))
                    .any(|d| d.is_dirty())
                {
                    lsp.saved_while_dirty(&path);
                }
            }
            for cmd in cmds {
                match cmd {
                    Command::Quit => quit = true,
                    Command::CopyToClipboard(text) => {
                        let _ = clip.send(clipboard::Request::Copy(text));
                    }
                    Command::RequestPaste => {
                        let _ = clip.send(clipboard::Request::Paste);
                    }
                    Command::Save {
                        path,
                        contents,
                        version,
                    } => {
                        let _ = save_tx.send((path, contents, version));
                    }
                    cmd @ (Command::ListDir { .. }
                    | Command::OpenFile(_)
                    | Command::CreateFile(_)
                    | Command::CreateDir(_)
                    | Command::Rename { .. }
                    | Command::Trash(_)
                    | Command::Reload(_)
                    | Command::ListFiles { .. }
                    | Command::SearchText { .. }) => workspace::spawn(cmd, tx.clone()),
                    cmd @ (Command::RequestCompletion { .. }
                    | Command::RequestHover { .. }
                    | Command::RequestDefinition { .. }
                    | Command::RequestFormat { .. }) => lsp.request(cmd),
                    // Alt+N comme Alt+B : l'offre réglée est traitée par `cleanup`.
                    Command::DiscardBackup(_) => {}
                    Command::TerminalKey(id, k) => term.key(id, k),
                    Command::TerminalPaste(id, text) => term.paste(id, &text),
                }
            }
            send_backup(backups.cleanup(&state));
            if quit {
                quitting = true;
                if let Err(e) = session::save(&session_file, &state) {
                    tracing::warn!("écriture de la session : {e}");
                }
                break;
            }
            // Avant le coloriage : un échec du lancement referme le panneau.
            term.sync(&mut state);
            highlighter.sync(&mut state);
            git.sync(&mut state, &tx);
            terminal.draw(|f| ide_ui::render(&state, f))?;
        }
        anyhow::Ok(())
    }
    .await;
    // Seulement s'il a changé : une autre instance fermée après ne remet pas l'ancien.
    if state.settings.theme != first_theme
        && let Err(e) = files::save_atomic(&theme_file, &state.settings.theme)
    {
        tracing::warn!("écriture du thème : {e}");
    }
    // Sortie anormale (signal, terminal fermé, erreur) : dernière copie de tout ce qui est modifié,
    // et les copies restent pour le prochain démarrage. Un crash ou un kill -9 ne passe pas ici.
    if !quitting {
        send_backup(backups.flush(&state));
    }
    // Avant de quitter, on laisse finir les sauvegardes en attente ; sans `rx`, leurs événements sont ignorés.
    drop(rx);
    drop(save_tx);
    match saver.await {
        // Sortie normale : l'utilisateur a renoncé aux modifications non sauvegardées ; les copies
        // ne sont supprimées qu'une fois les sauvegardes finies, sauf celles des sauvegardes en échec.
        Ok(failed) if quitting => send_backup(backups.quit(&state, &failed)),
        Ok(_) => {}
        Err(e) => tracing::error!("tâche de sauvegarde : {e} (copies de secours gardées)"),
    }
    drop(backup_tx);
    if let Err(e) = backup_worker.await {
        tracing::error!("tâche des copies de secours : {e}");
    }
    term.shutdown();
    lsp.shutdown().await;
    result
}

/// SIGHUP ou SIGTERM reçu (rien hors Unix).
fn terminate_signal() -> std::io::Result<futures::future::BoxFuture<'static, ()>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (mut hup, mut term) = (
            signal(SignalKind::hangup())?,
            signal(SignalKind::terminate())?,
        );
        Ok(Box::pin(async move {
            tokio::select! {
                _ = hup.recv() => {}
                _ = term.recv() => {}
            }
        }))
    }
    #[cfg(not(unix))]
    {
        Ok(Box::pin(std::future::pending()))
    }
}

/// Sauvegardes hors de la boucle (NF-05), une à la fois ; chaque résultat revient comme un événement.
/// Renvoie, à la fin, les chemins dont la dernière sauvegarde a échoué.
async fn save_worker(
    mut jobs: mpsc::UnboundedReceiver<(PathBuf, String, u64)>,
    tx: mpsc::Sender<Event>,
) -> Vec<PathBuf> {
    let mut failed: Vec<PathBuf> = vec![];
    while let Some((path, contents, version)) = jobs.recv().await {
        let p = path.clone();
        let res = tokio::task::spawn_blocking(move || files::save_atomic(&p, &contents))
            .await
            .map_err(std::io::Error::other)
            .and_then(|r| r);
        failed.retain(|p| *p != path);
        let ev = match res {
            Ok(()) => Event::Saved { path, version },
            Err(e) => {
                failed.push(path.clone());
                Event::SaveFailed {
                    path,
                    error: format!("échec de la sauvegarde : {e}"),
                }
            }
        };
        let _ = tx.send(ev).await;
    }
    failed
}

async fn read_terminal_events(tx: mpsc::Sender<Event>) {
    let mut stream = EventStream::new();
    while let Some(res) = stream.next().await {
        match res {
            Ok(ev) => {
                if let Some(ev) = convert(ev)
                    && tx.send(ev).await.is_err()
                {
                    break;
                }
            }
            Err(e) => {
                tracing::error!("lecture des événements terminal : {e}");
                break;
            }
        }
    }
}

/// Conversion pure crossterm → ide_core ; `None` pour ce qu'on ignore.
fn convert(ev: ct::Event) -> Option<Event> {
    match ev {
        ct::Event::Key(k) if k.kind == KeyEventKind::Press => {
            let key = match k.code {
                ct::KeyCode::Char(c) => Key::Char(c),
                ct::KeyCode::Enter => Key::Enter,
                ct::KeyCode::Esc => Key::Esc,
                ct::KeyCode::Backspace => Key::Backspace,
                ct::KeyCode::Tab | ct::KeyCode::BackTab => Key::Tab,
                ct::KeyCode::Up => Key::Up,
                ct::KeyCode::Down => Key::Down,
                ct::KeyCode::Left => Key::Left,
                ct::KeyCode::Right => Key::Right,
                ct::KeyCode::Delete => Key::Delete,
                ct::KeyCode::Home => Key::Home,
                ct::KeyCode::End => Key::End,
                ct::KeyCode::PageUp => Key::PageUp,
                ct::KeyCode::PageDown => Key::PageDown,
                ct::KeyCode::F(n) => Key::F(n),
                _ => Key::Other,
            };
            let mut mods = convert_mods(k.modifiers);
            mods.shift |= k.code == ct::KeyCode::BackTab;
            Some(Event::Key(KeyInput { key, mods }))
        }
        ct::Event::Mouse(m) => {
            let kind = match m.kind {
                MouseEventKind::Down(MouseButton::Left) => MouseKind::Down,
                MouseEventKind::Drag(MouseButton::Left) => MouseKind::Drag,
                MouseEventKind::Up(MouseButton::Left) => MouseKind::Up,
                MouseEventKind::ScrollUp => MouseKind::ScrollUp,
                MouseEventKind::ScrollDown => MouseKind::ScrollDown,
                _ => return None,
            };
            Some(Event::Mouse(MouseInput {
                kind,
                column: m.column,
                row: m.row,
                mods: convert_mods(m.modifiers),
            }))
        }
        ct::Event::Paste(s) => Some(Event::Paste(s)),
        ct::Event::Resize(width, height) => Some(Event::Resize { width, height }),
        _ => None,
    }
}

fn convert_mods(m: KeyModifiers) -> Modifiers {
    Modifiers {
        ctrl: m.contains(KeyModifiers::CONTROL),
        alt: m.contains(KeyModifiers::ALT),
        shift: m.contains(KeyModifiers::SHIFT),
    }
}

/// Dossier d'état : `$XDG_STATE_HOME/ide` (sinon `~/.local/state/ide`) ; logs, copies, sessions.
fn state_dir() -> anyhow::Result<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME non défini")?).join(".local/state")
        }
    };
    Ok(base.join("ide"))
}

/// Logs vers `<état>/ide.log`, jamais stdout/stderr.
fn init_logging(dir: &std::path::Path) -> anyhow::Result<WorkerGuard> {
    std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    // Le builder renvoie une erreur là où `rolling::never` paniquerait.
    let appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix("ide.log")
        .build(dir)
        .with_context(|| format!("ouverture du log dans {}", dir.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, MouseEvent};

    fn key(code: KeyCode, mods: KeyModifiers) -> ct::Event {
        ct::Event::Key(KeyEvent::new(code, mods))
    }

    fn input(key: Key, mods: Modifiers) -> Option<Event> {
        Some(Event::Key(KeyInput { key, mods }))
    }

    #[tokio::test]
    async fn sauvegardes_ecrites_dans_l_ordre() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("a.txt");
        let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();
        let (tx, mut rx) = mpsc::channel(32);
        for v in 1..=20u64 {
            jobs_tx.send((p.clone(), "x".repeat(20_000 / v as usize), v))?;
        }
        drop(jobs_tx);
        assert!(save_worker(jobs_rx, tx).await.is_empty());
        for v in 1..=20u64 {
            assert_eq!(
                rx.recv().await,
                Some(Event::Saved {
                    path: p.clone(),
                    version: v
                })
            );
        }
        // La dernière demande (la plus courte) gagne, même si les premières sont plus longues à écrire.
        assert_eq!(std::fs::read_to_string(&p)?, "x".repeat(1_000));
        Ok(())
    }

    #[tokio::test]
    async fn sauvegarde_en_echec_remontee_a_la_fin() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        // Parent qui est un fichier : l'écriture échoue.
        std::fs::write(dir.path().join("f"), "")?;
        let (bad, good) = (dir.path().join("f/a.txt"), dir.path().join("b.txt"));
        let (jobs_tx, jobs_rx) = mpsc::unbounded_channel();
        let (tx, _rx) = mpsc::channel(32);
        jobs_tx.send((good.clone(), "x".into(), 1))?;
        jobs_tx.send((bad.clone(), "x".into(), 1))?;
        drop(jobs_tx);
        assert_eq!(save_worker(jobs_rx, tx).await, vec![bad]);
        Ok(())
    }

    #[test]
    fn ctrl_q() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            convert(key(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            input(Key::Char('q'), ctrl)
        );
    }

    #[test]
    fn f12_et_entree() {
        assert_eq!(
            convert(key(KeyCode::F(12), KeyModifiers::NONE)),
            input(Key::F(12), Modifiers::default())
        );
        assert_eq!(
            convert(key(KeyCode::Enter, KeyModifiers::NONE)),
            input(Key::Enter, Modifiers::default())
        );
    }

    #[test]
    fn touches_de_navigation() {
        for (code, k) in [
            (KeyCode::Delete, Key::Delete),
            (KeyCode::Home, Key::Home),
            (KeyCode::End, Key::End),
            (KeyCode::PageUp, Key::PageUp),
            (KeyCode::PageDown, Key::PageDown),
        ] {
            assert_eq!(
                convert(key(code, KeyModifiers::NONE)),
                input(k, Modifiers::default())
            );
        }
    }

    #[test]
    fn caractere_avec_shift() {
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            convert(key(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            input(Key::Char('A'), shift)
        );
    }

    #[test]
    fn shift_tab() {
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            convert(key(KeyCode::BackTab, KeyModifiers::NONE)),
            input(Key::Tab, shift)
        );
    }

    #[test]
    fn ignore_relachement_et_convertit_resize() {
        let release = KeyEvent::new_with_kind_and_state(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
            KeyEventState::NONE,
        );
        assert_eq!(convert(ct::Event::Key(release)), None);
        assert_eq!(
            convert(ct::Event::Resize(100, 40)),
            Some(Event::Resize {
                width: 100,
                height: 40
            })
        );
    }

    fn souris(kind: MouseEventKind, mods: KeyModifiers) -> ct::Event {
        ct::Event::Mouse(MouseEvent {
            kind,
            column: 7,
            row: 3,
            modifiers: mods,
        })
    }

    fn mouse(kind: MouseKind, mods: Modifiers) -> Option<Event> {
        Some(Event::Mouse(MouseInput {
            kind,
            column: 7,
            row: 3,
            mods,
        }))
    }

    #[test]
    fn collage_terminal() {
        assert_eq!(
            convert(ct::Event::Paste("a\nb".into())),
            Some(Event::Paste("a\nb".into()))
        );
    }

    #[test]
    fn clic_glisser_relacher_molette() {
        let none = Modifiers::default();
        for (k, attendu) in [
            (MouseEventKind::Down(MouseButton::Left), MouseKind::Down),
            (MouseEventKind::Drag(MouseButton::Left), MouseKind::Drag),
            (MouseEventKind::Up(MouseButton::Left), MouseKind::Up),
            (MouseEventKind::ScrollUp, MouseKind::ScrollUp),
            (MouseEventKind::ScrollDown, MouseKind::ScrollDown),
        ] {
            assert_eq!(convert(souris(k, KeyModifiers::NONE)), mouse(attendu, none));
        }
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            convert(souris(
                MouseEventKind::Down(MouseButton::Left),
                KeyModifiers::SHIFT
            )),
            mouse(MouseKind::Down, shift)
        );
    }

    #[test]
    fn autres_boutons_et_mouvement_ignores() {
        for k in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Drag(MouseButton::Middle),
            MouseEventKind::Up(MouseButton::Right),
            MouseEventKind::Moved,
            MouseEventKind::ScrollLeft,
        ] {
            assert_eq!(convert(souris(k, KeyModifiers::NONE)), None);
        }
    }
}
