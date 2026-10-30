//! Binaire de l'IDE : boucle d'événements façon Elm (événement → update → rendu).

mod files;

use std::path::PathBuf;

use anyhow::Context;
use crossterm::event::{self as ct, EventStream, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ide_core::{Command, Event, Key, KeyInput, Modifiers, State};
use tokio::sync::mpsc;
use tracing_appender::non_blocking::WorkerGuard;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _log_guard = init_logging()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let panic_test = args.iter().any(|a| a == "--panic-test");
    // `ide [FICHIER]` : premier argument qui n'est pas une option.
    let file = args
        .into_iter()
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from);
    // Trace les panics dans le log ; ratatui::init chaîne ce hook après la restauration.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic : {info}");
        previous_hook(info);
    }));
    // ratatui::init installe un hook de panic qui restaure le terminal (NF-08).
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, panic_test, file).await;
    ratatui::restore();
    if let Err(e) = &result {
        tracing::error!("sortie sur erreur : {e:#}");
    }
    result
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    panic_test: bool,
    file: Option<PathBuf>,
) -> anyhow::Result<()> {
    let size = terminal.size()?;
    let mut state = State {
        width: size.width,
        height: size.height,
        ..State::default()
    };

    let (tx, mut rx) = mpsc::channel::<Event>(256);
    // Fichier lu avant de lancer le lecteur : FileOpened passe avant toute touche.
    if let Some(path) = file {
        let p = path.clone();
        let ev = match tokio::task::spawn_blocking(move || files::read_file(&p)).await? {
            Ok(text) => Event::FileOpened { path, text },
            Err(error) => Event::OpenFailed { path, error },
        };
        tx.send(ev).await?;
    }
    let mut reader = tokio::spawn(read_terminal_events(tx.clone()));

    terminal.draw(|f| ide_ui::render(&state, f))?;
    if panic_test {
        panic!("--panic-test : panic volontaire après le premier rendu");
    }

    // Une seule tâche écrit, dans l'ordre des Ctrl+S : une vieille version ne peut pas écraser une récente.
    let (save_tx, save_rx) = mpsc::unbounded_channel();
    let saver = tokio::spawn(save_worker(save_rx, tx.clone()));
    let result = async {
        loop {
            // On garde un `tx` pour les sauvegardes, donc `recv` ne finit jamais : on surveille la fin du lecteur.
            let ev = tokio::select! {
                biased;
                Some(ev) = rx.recv() => ev,
                _ = &mut reader => break,
            };
            let mut quit = false;
            for cmd in ide_core::update(&mut state, ev) {
                match cmd {
                    Command::Quit => quit = true,
                    Command::Save {
                        path,
                        contents,
                        version,
                    } => {
                        let _ = save_tx.send((path, contents, version));
                    }
                }
            }
            if quit {
                break;
            }
            terminal.draw(|f| ide_ui::render(&state, f))?;
        }
        anyhow::Ok(())
    }
    .await;
    // Avant de quitter, on laisse finir les sauvegardes en attente ; sans `rx`, leurs événements sont ignorés.
    drop(rx);
    drop(save_tx);
    if let Err(e) = saver.await {
        tracing::error!("tâche de sauvegarde : {e}");
    }
    result
}

/// Sauvegardes hors de la boucle (NF-05), une à la fois ; chaque résultat revient comme un événement.
async fn save_worker(
    mut jobs: mpsc::UnboundedReceiver<(PathBuf, String, u64)>,
    tx: mpsc::Sender<Event>,
) {
    while let Some((path, contents, version)) = jobs.recv().await {
        let p = path.clone();
        let res = tokio::task::spawn_blocking(move || files::save_atomic(&p, &contents))
            .await
            .map_err(std::io::Error::other)
            .and_then(|r| r);
        let ev = match res {
            Ok(()) => Event::Saved { path, version },
            Err(e) => Event::SaveFailed {
                path,
                error: format!("échec de la sauvegarde : {e}"),
            },
        };
        let _ = tx.send(ev).await;
    }
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
            let mods = Modifiers {
                ctrl: k.modifiers.contains(KeyModifiers::CONTROL),
                alt: k.modifiers.contains(KeyModifiers::ALT),
                shift: k.modifiers.contains(KeyModifiers::SHIFT) || k.code == ct::KeyCode::BackTab,
            };
            Some(Event::Key(KeyInput { key, mods }))
        }
        ct::Event::Resize(width, height) => Some(Event::Resize { width, height }),
        _ => None,
    }
}

/// Logs vers `$XDG_STATE_HOME/ide/ide.log` (sinon `~/.local/state/ide/ide.log`), jamais stdout/stderr.
fn init_logging() -> anyhow::Result<WorkerGuard> {
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME non défini")?).join(".local/state")
        }
    };
    let dir = base.join("ide");
    std::fs::create_dir_all(&dir).with_context(|| format!("création de {}", dir.display()))?;
    // Le builder renvoie une erreur là où `rolling::never` paniquerait.
    let appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix("ide.log")
        .build(&dir)
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
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState};

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
        save_worker(jobs_rx, tx).await;
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
}
