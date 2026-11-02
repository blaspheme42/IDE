//! Serveurs de langage (J4) : démarrage par langage, synchronisation des documents, requêtes.
//! Tout passe par des tâches de fond : un serveur lent ou planté ne bloque jamais la boucle (NF-05).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ide_core::{Command, Document, Event, LspServer, State};
use ide_lsp::{Client, Notification};
use ide_syntax::Language;
use tokio::sync::mpsc;

/// Redémarrages automatiques par langage et par session, après un arrêt du serveur.
const MAX_RESTARTS: u32 = 3;

enum Server {
    Starting,
    Ready(Client),
    /// Lancement raté (commande introuvable…) ou trop de plantages : plus de tentative de la session.
    Failed,
}

/// Ce qu'on a déjà dit au serveur d'un document (clé : chemin).
#[derive(Debug)]
struct Tracked {
    lang: String,
    version: u64,
    /// Version LSP : compteur local croissant, la version du cœur (u64 globale) ne tient pas dans un i32.
    lsp_version: i32,
    dirty: bool,
}

#[derive(Debug, PartialEq)]
enum Sync {
    Open {
        path: PathBuf,
        lang: String,
        version: i32,
        text: String,
    },
    Change {
        path: PathBuf,
        lang: String,
        version: i32,
        text: String,
    },
    Save {
        path: PathBuf,
        lang: String,
    },
    Close {
        path: PathBuf,
        lang: String,
    },
}

pub struct LspManager {
    tx: mpsc::Sender<Event>,
    root: PathBuf,
    servers: Vec<LspServer>,
    /// État par langage ; absent = jamais démarré (ou à redémarrer).
    clients: HashMap<String, Server>,
    restarts: HashMap<String, u32>,
    tracked: HashMap<PathBuf, Tracked>,
    /// Résultats des démarrages en fond, lus au début de `sync`.
    started_tx: mpsc::UnboundedSender<(String, Option<Client>)>,
    started_rx: mpsc::UnboundedReceiver<(String, Option<Client>)>,
    /// Serveur d'appoint (Emmet) : ni statut ni diagnostics, complétion seulement.
    quiet: bool,
    companion: Option<Box<LspManager>>,
}

/// Serveurs d'appoint, pour la complétion seulement : leurs propositions s'ajoutent à celles du serveur principal.
// shortcut: un processus Emmet par langage (html, css, php), à partager si la mémoire compte.
fn companions() -> Vec<LspServer> {
    ["html", "css", "php"]
        .map(|l| LspServer {
            language: l.into(),
            command: "emmet-language-server".into(),
            args: vec!["--stdio".into()],
        })
        .into()
}

/// `languageId` du protocole : diffère de notre nom de langage pour quelques-uns.
/// Emmet ne connaît pas PHP : on lui présente le fichier comme du HTML.
fn language_id(lang: &str, emmet: bool) -> &str {
    match lang {
        "tsx" => "typescriptreact",
        "php" if emmet => "html",
        other => other,
    }
}

fn lang_of(path: &Path) -> Option<String> {
    Language::from_path(path).map(|l| l.name().to_lowercase())
}

/// Au-delà, un document n'est pas confié au serveur (comme la coloration, NF-03) : la synchronisation complète
/// recopierait et sérialiserait tout le texte à chaque frappe (≈200 ms sur 60 Mo).
const MAX_SYNC_BYTES: usize = 5 << 20;

/// Documents ouverts confiés aux serveurs de langage.
fn docs(state: &State) -> impl Iterator<Item = &Document> {
    std::iter::once(&state.doc)
        .chain(state.tabs.iter().map(|t| &t.doc))
        .filter(|d| d.rope().len_bytes() <= MAX_SYNC_BYTES)
}

/// Diff pur entre ce que le serveur sait (`tracked`) et les documents ouverts dont le langage est prêt.
fn plan<'a>(
    tracked: &mut HashMap<PathBuf, Tracked>,
    docs: impl Iterator<Item = &'a Document>,
    ready: impl Fn(&str) -> bool,
) -> Vec<Sync> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for doc in docs {
        let Some(path) = &doc.path else { continue };
        let Some(lang) = lang_of(path).filter(|l| ready(l)) else {
            continue;
        };
        seen.insert(path.clone());
        // shortcut: synchronisation complète (texte entier à chaque version), passer en incrémental si les gros fichiers rament.
        match tracked.get_mut(path) {
            None => {
                tracked.insert(
                    path.clone(),
                    Tracked {
                        lang: lang.clone(),
                        version: doc.version(),
                        lsp_version: 1,
                        dirty: doc.is_dirty(),
                    },
                );
                out.push(Sync::Open {
                    path: path.clone(),
                    lang,
                    version: 1,
                    text: doc.rope().to_string(),
                });
            }
            Some(t) => {
                if t.version != doc.version() {
                    t.version = doc.version();
                    t.lsp_version += 1;
                    out.push(Sync::Change {
                        path: path.clone(),
                        lang: lang.clone(),
                        version: t.lsp_version,
                        text: doc.rope().to_string(),
                    });
                }
                if t.dirty && !doc.is_dirty() {
                    out.push(Sync::Save {
                        path: path.clone(),
                        lang,
                    });
                }
                t.dirty = doc.is_dirty();
            }
        }
    }
    tracked.retain(|path, t| {
        let keep = seen.contains(path);
        if !keep {
            out.push(Sync::Close {
                path: path.clone(),
                lang: t.lang.clone(),
            });
        }
        keep
    });
    out
}

/// Erreur ou délai dépassé : résultat vide, tracé dans le log.
fn or_empty<T: Default>(what: &str, r: Result<T, String>) -> T {
    r.unwrap_or_else(|e| {
        tracing::warn!("LSP {what} : {e}");
        T::default()
    })
}

impl LspManager {
    pub fn new(tx: mpsc::Sender<Event>, root: PathBuf, servers: Vec<LspServer>) -> Self {
        let companion = Self::build(tx.clone(), root.clone(), companions(), true);
        Self {
            companion: Some(Box::new(companion)),
            ..Self::build(tx, root, servers, false)
        }
    }

    fn build(tx: mpsc::Sender<Event>, root: PathBuf, servers: Vec<LspServer>, quiet: bool) -> Self {
        let (started_tx, started_rx) = mpsc::unbounded_channel();
        Self {
            tx,
            root,
            servers,
            clients: HashMap::new(),
            restarts: HashMap::new(),
            tracked: HashMap::new(),
            started_tx,
            started_rx,
            quiet,
            companion: None,
        }
    }

    /// Avant chaque rendu : démarre les serveurs utiles, envoie didOpen / didChange / didClose
    /// pour les documents ouverts, modifiés ou fermés depuis le dernier appel. Ne bloque jamais.
    /// Sauvegarde confirmée alors que le document a déjà été retouché (frappe pendant l'écriture) :
    /// pas de passage modifié → propre, donc `sync` n'enverrait pas de didSave ; on l'envoie ici.
    pub fn saved_while_dirty(&mut self, path: &Path) {
        let lang = self.tracked.get(path).map(|t| t.lang.clone());
        if let Some(Server::Ready(c)) = lang.and_then(|l| self.clients.get(&l))
            && c.is_alive()
        {
            c.did_save(path);
        }
        if let Some(c) = &mut self.companion {
            c.saved_while_dirty(path);
        }
    }

    pub fn sync(&mut self, state: &State) {
        while let Ok((lang, client)) = self.started_rx.try_recv() {
            let s = client.map_or(Server::Failed, Server::Ready);
            self.clients.insert(lang, s);
        }
        // Serveurs morts : oubliés (documents ré-ouverts au redémarrage), au plus MAX_RESTARTS fois.
        let dead: Vec<String> = self
            .clients
            .iter()
            .filter(|(_, s)| matches!(s, Server::Ready(c) if !c.is_alive()))
            .map(|(l, _)| l.clone())
            .collect();
        for lang in dead {
            // Ses diagnostics deviendraient faux au fil de l'édition : on les efface (sans bloquer : try_send).
            let mine = |t: &Tracked| t.lang == lang && !self.quiet;
            for (path, _) in self.tracked.iter().filter(|(_, t)| mine(t)) {
                let _ = self.tx.try_send(Event::Diagnostics {
                    path: path.clone(),
                    items: vec![],
                });
            }
            self.tracked.retain(|_, t| t.lang != lang);
            let n = self.restarts.entry(lang.clone()).or_default();
            *n += 1;
            if *n > MAX_RESTARTS {
                self.clients.insert(lang, Server::Failed);
            } else {
                self.clients.remove(&lang);
            }
        }
        let wanted: HashSet<String> = docs(state)
            .filter_map(|d| d.path.as_deref().and_then(lang_of))
            .collect();
        for lang in wanted {
            if !self.clients.contains_key(&lang) {
                self.start(&lang);
            }
        }
        let clients = &self.clients;
        let ready = |l: &str| matches!(clients.get(l), Some(Server::Ready(_)));
        for s in plan(&mut self.tracked, docs(state), ready) {
            let lang = match &s {
                Sync::Open { lang, .. }
                | Sync::Change { lang, .. }
                | Sync::Save { lang, .. }
                | Sync::Close { lang, .. } => lang,
            };
            let Some(Server::Ready(c)) = self.clients.get(lang) else {
                continue;
            };
            match &s {
                Sync::Open {
                    path,
                    lang,
                    version,
                    text,
                } => c.did_open(path, language_id(lang, self.quiet), *version, text),
                Sync::Change {
                    path,
                    version,
                    text,
                    ..
                } => c.did_change(path, *version, text),
                Sync::Save { path, .. } => c.did_save(path),
                Sync::Close { path, .. } => c.did_close(path),
            }
        }
        if let Some(c) = &mut self.companion {
            c.sync(state);
        }
    }

    /// Lance le serveur du langage en fond s'il est configuré ; sinon rien.
    fn start(&mut self, lang: &str) {
        let Some(server) = self.servers.iter().find(|s| s.language == lang) else {
            return;
        };
        self.clients.insert(lang.to_string(), Server::Starting);
        let (cmd, args, root) = (
            server.command.clone(),
            server.args.clone(),
            self.root.clone(),
        );
        let (lang, tx, started) = (lang.to_string(), self.tx.clone(), self.started_tx.clone());
        let quiet = self.quiet;
        let status = move |tx: mpsc::Sender<Event>, text: String| async move {
            if !quiet {
                let _ = tx.send(Event::LspStatus(text)).await;
            }
        };
        tokio::spawn(async move {
            status(tx.clone(), format!("{cmd}…")).await;
            match Client::start(&cmd, &args, &root).await {
                Ok((client, notifs)) => {
                    // Client remis avant le statut : le `sync` réveillé par le statut ouvre les documents.
                    let _ = started.send((lang, Some(client)));
                    status(tx.clone(), format!("{cmd} ✓")).await;
                    forward(cmd, notifs, tx, quiet).await;
                }
                Err(e) => {
                    tracing::warn!("{e}");
                    let _ = started.send((lang, None));
                    let why = if e.contains("introuvable") {
                        "introuvable"
                    } else {
                        "échec du démarrage"
                    };
                    status(tx, format!("{cmd} : {why}")).await;
                }
            }
        });
    }

    fn client(&self, path: &Path) -> Option<Client> {
        match self.clients.get(&lang_of(path)?) {
            Some(Server::Ready(c)) if c.is_alive() => Some(c.clone()),
            _ => None,
        }
    }

    /// `RequestCompletion` / `RequestHover` / `RequestDefinition` / `RequestFormat` → réponse en `Event` sur `tx`.
    /// Sans serveur vivant : réponse vide tout de suite, le cœur n'attend jamais.
    pub fn request(&mut self, cmd: Command) {
        let path = match &cmd {
            Command::RequestCompletion { path, .. }
            | Command::RequestHover { path, .. }
            | Command::RequestDefinition { path, .. }
            | Command::RequestFormat { path, .. } => path,
            _ => return,
        };
        let client = self.client(path);
        let extra = self.companion.as_ref().and_then(|c| c.client(path));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let ev = match cmd {
                Command::RequestCompletion { path, version, pos } => {
                    let ask = |c: Option<Client>| {
                        let path = path.clone();
                        async move {
                            match c {
                                Some(c) => or_empty("completion", c.completion(&path, pos).await),
                                None => (Vec::new(), false),
                            }
                        }
                    };
                    // Emmet d'abord : une abréviation valide n'y donne qu'une ou deux propositions.
                    let ((mut items, a), (main, b)) = tokio::join!(ask(extra), ask(client));
                    items.extend(main);
                    let incomplete = a || b;
                    Event::CompletionResult {
                        path,
                        version,
                        items,
                        incomplete,
                    }
                }
                Command::RequestHover { path, pos } => {
                    let text = match &client {
                        Some(c) => or_empty("hover", c.hover(&path, pos).await),
                        None => None,
                    };
                    Event::HoverResult { path, text }
                }
                Command::RequestDefinition { path, pos } => {
                    Event::DefinitionResult(match &client {
                        Some(c) => or_empty("definition", c.definition(&path, pos).await),
                        None => Vec::new(),
                    })
                }
                Command::RequestFormat {
                    path,
                    version,
                    indent,
                } => {
                    let edits = match &client {
                        Some(c) => or_empty("formatting", c.formatting(&path, indent).await),
                        None => Vec::new(),
                    };
                    Event::FormatResult {
                        path,
                        version,
                        edits,
                    }
                }
                _ => return,
            };
            let _ = tx.send(ev).await;
        });
    }

    /// À la sortie : `shutdown` de tous les serveurs en parallèle, 1 s au plus en tout.
    pub async fn shutdown(self) {
        let extra = self.companion.map(|c| c.clients).unwrap_or_default();
        let all = self
            .clients
            .into_values()
            .chain(extra.into_values())
            .filter_map(|s| match s {
                Server::Ready(c) => Some(async move { c.shutdown().await }),
                _ => None,
            });
        let all = futures::future::join_all(all);
        if tokio::time::timeout(Duration::from_secs(1), all)
            .await
            .is_err()
        {
            tracing::warn!("LSP : arrêt des serveurs au-delà d'1 s, abandonné");
        }
    }
}

/// Notifications d'un serveur → événements du cœur, jusqu'à son arrêt.
async fn forward(
    name: String,
    mut notifs: mpsc::UnboundedReceiver<Notification>,
    tx: mpsc::Sender<Event>,
    quiet: bool,
) {
    while let Some(n) = notifs.recv().await {
        let ev = match n {
            // Serveur d'appoint : ses diagnostics écraseraient ceux du serveur principal.
            Notification::Diagnostics { .. } if quiet => continue,
            Notification::Exited(msg) if quiet => {
                tracing::warn!("{msg}");
                continue;
            }
            Notification::Diagnostics { path, items } => Event::Diagnostics { path, items },
            Notification::Message(m) => {
                tracing::info!("{name} : {m}");
                continue;
            }
            Notification::Exited(msg) => {
                tracing::warn!("{msg}");
                Event::LspStatus(format!("{name} : arrêté (l'édition continue)"))
            }
        };
        if tx.send(ev).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Key, KeyInput, Modifiers, Position, update};

    fn etat(path: &str, text: &str) -> State {
        let mut s = State {
            width: 80,
            height: 30,
            ..State::default()
        };
        ouvrir(&mut s, path, text);
        s
    }

    fn ouvrir(s: &mut State, path: &str, text: &str) {
        update(
            s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        );
    }

    fn rust(l: &str) -> bool {
        l == "rust"
    }

    fn p(tracked: &mut HashMap<PathBuf, Tracked>, s: &State) -> Vec<Sync> {
        plan(tracked, docs(s), rust)
    }

    #[test]
    fn ouverture_edition_sauvegarde_fermeture() {
        let mut s = etat("/x/a.rs", "fn a() {}\n");
        let mut t = HashMap::new();
        let open = Sync::Open {
            path: "/x/a.rs".into(),
            lang: "rust".into(),
            version: 1,
            text: "fn a() {}\n".into(),
        };
        assert_eq!(p(&mut t, &s), vec![open]);
        assert_eq!(p(&mut t, &s), vec![], "rien de neuf");
        s.doc.insert_str("// c\n");
        let change = |v| Sync::Change {
            path: "/x/a.rs".into(),
            lang: "rust".into(),
            version: v,
            text: "// c\nfn a() {}\n".into(),
        };
        assert_eq!(p(&mut t, &s), vec![change(2)]);
        let version = s.doc.version();
        update(
            &mut s,
            Event::Saved {
                path: "/x/a.rs".into(),
                version,
            },
        );
        assert!(!s.doc.is_dirty());
        let save = Sync::Save {
            path: "/x/a.rs".into(),
            lang: "rust".into(),
        };
        assert_eq!(p(&mut t, &s), vec![save]);
        // Onglet fermé (document propre) : Ctrl+W.
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        update(
            &mut s,
            Event::Key(KeyInput {
                key: Key::Char('w'),
                mods: ctrl,
            }),
        );
        let close = Sync::Close {
            path: "/x/a.rs".into(),
            lang: "rust".into(),
        };
        assert_eq!(p(&mut t, &s), vec![close]);
        assert!(t.is_empty());
    }

    #[test]
    fn renommage_ferme_puis_ouvre() {
        let mut s = etat("/x/a.rs", "x");
        let mut t = HashMap::new();
        p(&mut t, &s);
        s.doc.path = Some("/x/b.rs".into());
        let out = p(&mut t, &s);
        assert_eq!(out.len(), 2);
        assert!(out.contains(&Sync::Close {
            path: "/x/a.rs".into(),
            lang: "rust".into()
        }));
        assert!(
            matches!(&out[0], Sync::Open { path, version: 1, .. } if path == Path::new("/x/b.rs"))
        );
    }

    #[test]
    fn sans_serveur_ni_chemin_rien() {
        let mut s = etat("/x/a.py", "x = 1\n");
        ouvrir(&mut s, "/x/notes.txt", "x");
        s.tabs[0].doc.path = None;
        let mut t = HashMap::new();
        assert_eq!(p(&mut t, &s), vec![]);
        // Document sans chemin, même en Rust.
        let mut s = State::default();
        s.doc.insert_str("fn a() {}");
        assert_eq!(p(&mut t, &s), vec![]);
        assert!(t.is_empty());
    }

    #[test]
    fn onglet_inactif_suivi() {
        let mut s = etat("/x/a.rs", "a");
        ouvrir(&mut s, "/x/b.rs", "b");
        let mut t = HashMap::new();
        assert_eq!(p(&mut t, &s).len(), 2);
        s.tabs[0].doc.insert_str("z");
        assert!(
            matches!(&p(&mut t, &s)[..], [Sync::Change { path, version: 2, .. }] if path == Path::new("/x/a.rs"))
        );
    }

    #[tokio::test]
    async fn requete_sans_client_reponse_vide() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut m = LspManager::new(tx, "/x".into(), Vec::new());
        let pos = Position {
            line: 0,
            character: 0,
        };
        m.request(Command::RequestCompletion {
            path: "/x/a.rs".into(),
            version: 7,
            pos,
        });
        m.request(Command::RequestHover {
            path: "/x/a.rs".into(),
            pos,
        });
        m.request(Command::RequestDefinition {
            path: "/x/a.rs".into(),
            pos,
        });
        let mut got = Vec::new();
        for _ in 0..3 {
            let ev = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
            got.push(ev.expect("réponse immédiate").expect("événement"));
        }
        assert!(got.contains(&Event::CompletionResult {
            path: "/x/a.rs".into(),
            version: 7,
            items: Vec::new(),
            incomplete: false,
        }));
        assert!(got.contains(&Event::HoverResult {
            path: "/x/a.rs".into(),
            text: None
        }));
        assert!(got.contains(&Event::DefinitionResult(Vec::new())));
    }

    #[tokio::test]
    async fn serveur_introuvable_une_seule_tentative() {
        let (tx, mut rx) = mpsc::channel(8);
        let servers = vec![LspServer {
            language: "rust".into(),
            command: "ide-serveur-qui-n-existe-pas".into(),
            args: Vec::new(),
        }];
        let mut m = LspManager::new(tx, "/".into(), servers);
        let s = etat("/x/a.rs", "x");
        m.sync(&s);
        let mut status = Vec::new();
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            m.sync(&s);
            status.push(ev);
        }
        assert_eq!(
            status,
            vec![
                Event::LspStatus("ide-serveur-qui-n-existe-pas…".into()),
                Event::LspStatus("ide-serveur-qui-n-existe-pas : introuvable".into()),
            ]
        );
        assert!(matches!(m.clients.get("rust"), Some(Server::Failed)));
    }

    /// Attend un événement qui vérifie `f`, en resynchronisant à chaque événement reçu.
    async fn attendre(
        m: &mut LspManager,
        s: &State,
        rx: &mut mpsc::Receiver<Event>,
        secs: u64,
        f: impl Fn(&Event) -> bool,
    ) -> Event {
        let fin = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let ev = tokio::time::timeout_at(fin, rx.recv())
                .await
                .expect("délai dépassé")
                .expect("canal fermé");
            m.sync(s);
            if f(&ev) {
                return ev;
            }
        }
    }

    #[tokio::test]
    #[ignore = "lance un vrai rust-analyzer"]
    async fn rust_analyzer_reel() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("racine");
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"essai\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("Cargo.toml");
        std::fs::create_dir(root.join("src")).expect("src");
        let main = root.join("src/main.rs");
        let text = "fn main() {\n    let v: Vec<u8> = Vec::new();\n    v.\n    let x: u32 = \"pas un nombre\";\n}\n";
        std::fs::write(&main, text).expect("main.rs");
        let (tx, mut rx) = mpsc::channel(64);
        let mut m = LspManager::new(tx, root.clone(), ide_core::default_servers());
        let s = etat(&main.to_string_lossy(), text);
        m.sync(&s);
        attendre(
            &mut m,
            &s,
            &mut rx,
            30,
            |e| matches!(e, Event::LspStatus(t) if t == "rust-analyzer ✓"),
        )
        .await;
        let diag = attendre(&mut m, &s, &mut rx, 30, |e| {
            matches!(e, Event::Diagnostics { path, items } if *path == main && !items.is_empty())
        })
        .await;
        eprintln!("diagnostics : {diag:?}");
        // Complétion après `v.` (ligne 2, colonne 6) ; l'indexation peut la retarder.
        let mut items = Vec::new();
        for _ in 0..20 {
            m.request(Command::RequestCompletion {
                path: main.clone(),
                version: s.doc.version(),
                pos: Position {
                    line: 2,
                    character: 6,
                },
            });
            if let Event::CompletionResult { items: i, .. } =
                attendre(&mut m, &s, &mut rx, 30, |e| {
                    matches!(e, Event::CompletionResult { .. })
                })
                .await
            {
                items = i;
            }
            if !items.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        eprintln!("complétion : {} éléments", items.len());
        assert!(!items.is_empty());
        // Arrêt du serveur en cours de session (pas d'accès au pid : shutdown du client).
        let Some(Server::Ready(c)) = m.clients.get("rust") else {
            panic!("client absent");
        };
        c.clone().shutdown().await;
        let fin = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let ev = tokio::time::timeout_at(fin, rx.recv())
                .await
                .expect("pas de statut « arrêté »")
                .expect("canal");
            if matches!(&ev, Event::LspStatus(t) if t.contains("arrêté")) {
                eprintln!("statut : {ev:?}");
                break;
            }
        }
        m.request(Command::RequestHover {
            path: main.clone(),
            pos: Position {
                line: 0,
                character: 3,
            },
        });
        let ev = tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .expect("réponse immédiate");
        assert_eq!(
            ev,
            Some(Event::HoverResult {
                path: main,
                text: None
            })
        );
    }
}
