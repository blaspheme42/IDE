//! Cœur de l'IDE : état et logique, sans aucune I/O.

use std::path::PathBuf;

mod backup;
mod document;
mod editor;
mod explorer;
mod history;
mod lsp;
mod palette;
mod search;
mod tabs;

pub use document::{Document, Indent, LineEnding, TextEdit};
pub use editor::gutter_width;
pub use explorer::{Explorer, FsChange, FsEntry, Node, Prompt, PromptKind};
pub use lsp::{
    Completion, CompletionItem, Diagnostic, Hover, Location, LspPending, LspServer,
    PROBLEMS_HEIGHT, Position, Severity, default_servers, diagnostic_counts, problems,
    visible_completions,
};
pub use palette::{
    CommandInfo, Palette, PaletteItem, PaletteKind, SHORTCUTS, command_by_name, commands, help_len,
    palette_items,
};
pub use search::Search;
pub use tabs::{TAB_BAR_HEIGHT, Tab, TabInfo, tab_label, tab_list, tab_scroll, tab_spans};

/// Touche indépendante du terminal (l'app convertit depuis crossterm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    pub key: Key,
    pub mods: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Down,
    Drag,
    Up,
    ScrollUp,
    ScrollDown,
}

/// Événement souris ; coordonnées écran 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseInput {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
    pub mods: Modifiers,
}

/// Tout ce qui arrive dans la boucle d'événements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key(KeyInput),
    Mouse(MouseInput),
    /// Texte du presse-papiers, en réponse à `Command::RequestPaste` ou collé par le terminal.
    Paste(String),
    Resize {
        width: u16,
        height: u16,
    },
    Tick,
    /// L'app a lu le fichier passé en argument.
    FileOpened {
        path: PathBuf,
        text: String,
    },
    /// Style d'indentation (.editorconfig) du fichier `path`, lu par l'app : appliqué à l'onglet de ce
    /// chemin, pas à l'actif (ouverture ratée, ou deux ouvertures entrelacées).
    IndentDetected {
        path: PathBuf,
        indent: Indent,
    },
    /// `create` : le fichier n'existe pas encore, Ctrl+S le créera. Sinon le chemin est oublié,
    /// pour qu'un Ctrl+S n'écrase pas un fichier qu'on n'a pas pu lire (non UTF-8, permission…).
    OpenFailed {
        path: PathBuf,
        error: String,
        create: bool,
    },
    /// `version` = celle transmise dans `Command::Save`.
    Saved {
        path: PathBuf,
        version: u64,
    },
    SaveFailed {
        path: PathBuf,
        error: String,
    },
    /// Dossier racine de l'explorateur (argument de `ide`, ou dossier du fichier, ou dossier courant).
    OpenRoot(PathBuf),
    /// Réponse à `Command::ListDir`.
    DirListed {
        dir: PathBuf,
        result: Result<Vec<FsEntry>, String>,
    },
    /// Réponse à `CreateFile` / `CreateDir` / `Rename` / `Trash`.
    FsDone(Result<FsChange, String>),
    /// Chemins changés sur le disque (watcher de l'app), regroupés.
    DiskChanged(Vec<PathBuf>),
    /// Réponse à `Command::Reload` ; `None` = fichier disparu du disque.
    FileChanged {
        path: PathBuf,
        text: Option<String>,
    },
    /// Diagnostics publiés par le serveur pour `path` (remplacent les précédents).
    Diagnostics {
        path: PathBuf,
        items: Vec<Diagnostic>,
    },
    /// Réponse à `RequestCompletion` ; ignorée si le document a changé de version entre-temps.
    CompletionResult {
        path: PathBuf,
        version: u64,
        items: Vec<CompletionItem>,
        /// Liste tronquée par le serveur (`isIncomplete`) : chaque frappe la redemande.
        incomplete: bool,
    },
    /// Réponse à `RequestHover` (`None` : rien à afficher).
    HoverResult {
        path: PathBuf,
        text: Option<String>,
    },
    /// Réponse à `RequestDefinition`.
    DefinitionResult(Vec<Location>),
    /// Réponse à `RequestFormat` : remplacements (début, fin, texte) sur le texte de `version`.
    FormatResult {
        path: PathBuf,
        version: u64,
        edits: Vec<(Position, Position, String)>,
        save: bool,
    },
    /// État du serveur pour la barre de statut (« rust-analyzer ✓ », « clangd : arrêté »…), vide = aucun.
    LspStatus(String),
    /// Fichiers du projet (chemins absolus, .gitignore respecté), réponse à `Command::ListFiles`.
    FileList(Vec<PathBuf>),
    /// Branche git courante (`None` : pas de dépôt, ou HEAD détachée → l'app envoie le commit court).
    GitBranch(Option<String>),
    /// Configuration chargée au démarrage ; `errors` : problèmes à signaler sans bloquer (clé inconnue, TOML invalide…).
    ConfigLoaded {
        settings: Settings,
        errors: Vec<String>,
    },
    /// Copie de secours de `path` (NF-07) dont le texte diffère du disque, envoyée après l'ouverture
    /// de son onglet : proposée par Alt+B (restaurer) / Alt+N (ignorer).
    BackupFound {
        path: PathBuf,
        text: String,
    },
    /// Session (F-80) : curseur (ligne, colonne en chars) et première ligne visible de l'onglet de `path`.
    RestoreView {
        path: PathBuf,
        cursor: (usize, usize),
        scroll_line: usize,
    },
    /// Active l'onglet ouvert sur ce chemin, s'il existe.
    ActivateTab(PathBuf),
}

/// Effets demandés par `update`, exécutés par l'app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit,
    /// Écriture atomique par l'app (NF-06) ; `contents` a déjà ses fins de ligne d'origine.
    Save {
        path: PathBuf,
        contents: String,
        version: u64,
    },
    CopyToClipboard(String),
    /// L'app lit le presse-papiers et renvoie `Event::Paste`.
    RequestPaste,
    /// Lister les enfants directs de `dir` → `Event::DirListed`.
    ListDir {
        dir: PathBuf,
        show_hidden: bool,
    },
    /// Lire un fichier → `Event::FileOpened` (+ `IndentDetected`) ou `Event::OpenFailed`.
    OpenFile(PathBuf),
    /// Opérations disque → `Event::FsDone`.
    CreateFile(PathBuf),
    CreateDir(PathBuf),
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    /// Vers la corbeille, jamais de suppression définitive.
    Trash(PathBuf),
    /// Lister tous les fichiers du projet (palette Ctrl+P) → `Event::FileList`.
    ListFiles {
        root: PathBuf,
        show_hidden: bool,
    },
    /// Requêtes LSP sur le document `path` à la `version` courante (l'app aura déjà synchronisé ce texte).
    RequestCompletion {
        path: PathBuf,
        version: u64,
        pos: Position,
    },
    RequestHover {
        path: PathBuf,
        pos: Position,
    },
    RequestDefinition {
        path: PathBuf,
        pos: Position,
    },
    /// Mise en page du document entier (`textDocument/formatting`) → `Event::FormatResult`.
    RequestFormat {
        path: PathBuf,
        version: u64,
        indent: Indent,
        /// Ctrl+S : sauvegarder après la mise en page (même sans serveur ou si elle échoue).
        save: bool,
    },
    /// Relire un fichier ouvert, changé sur le disque → `Event::FileChanged`.
    Reload(PathBuf),
    /// Supprimer la copie de secours de ce fichier (Alt+N).
    DiscardBackup(PathBuf),
}

/// Réglages lus dans `config.toml` (utilisateur puis projet) par l'app ; chaque clé a un défaut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `"dark"` ou `"light"`.
    pub theme: String,
    /// Icônes Nerd Font (`false` = repli ASCII, NF-12).
    pub nerd_icons: bool,
    /// Indentation par défaut, quand aucun `.editorconfig` ne s'applique.
    pub indent: Indent,
    pub explorer_width: u16,
    pub show_hidden: bool,
    /// Mise en page par le serveur de langage avant chaque sauvegarde (F-45).
    pub format_on_save: bool,
    /// Serveurs de langage par langage (`[lsp.*]`), défauts : `default_servers()`.
    pub lsp: Vec<LspServer>,
    /// Raccourcis personnalisés (`[keys]`) : touche → nom de commande de la palette.
    pub keys: Vec<(KeyInput, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            nerd_icons: true,
            indent: Indent::default(),
            explorer_width: 30,
            show_hidden: false,
            format_on_save: true,
            lsp: default_servers(),
            keys: Vec::new(),
        }
    }
}

/// Partie de l'écran qui reçoit le clavier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Editor,
    Explorer,
}

#[derive(Debug, Default)]
pub struct State {
    pub width: u16,
    pub height: u16,
    pub status: String,
    /// Document de l'onglet actif.
    pub doc: Document,
    /// Autres onglets, dans l'ordre d'affichage ; l'actif s'intercale à l'index `active`.
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Première ligne et première colonne d'affichage visibles dans l'éditeur.
    pub scroll_line: usize,
    pub scroll_col: usize,
    /// Ctrl+Q déjà pressé une fois avec des modifications non sauvegardées.
    pub quit_armed: bool,
    /// Ctrl+W déjà pressé une fois sur un onglet modifié.
    pub close_armed: bool,
    /// Bouton gauche enfoncé dans la zone de texte : un glisser étend la sélection.
    pub dragging: bool,
    /// Dernière copie faite sans sélection (ligne entière) : elle se colle au-dessus de la ligne courante.
    pub line_clip: Option<String>,
    /// Barre de recherche ouverte (Ctrl+F / Ctrl+H).
    pub search: Option<Search>,
    pub explorer: Explorer,
    pub settings: Settings,
    /// Touche rejouée par la palette de commandes : `[keys]` ne doit pas la détourner.
    #[doc(hidden)]
    pub replaying: bool,
    /// Palette ouverte (Ctrl+P, Ctrl+Shift+P).
    pub palette: Option<Palette>,
    /// Fichiers du projet pour Ctrl+P (vide tant que la liste n'est pas arrivée).
    pub project_files: Vec<PathBuf>,
    /// Branche git courante, pour la barre de statut (F-70).
    pub git_branch: Option<String>,
    /// Diagnostics LSP par fichier (F-42).
    pub diagnostics: std::collections::HashMap<PathBuf, Vec<Diagnostic>>,
    /// Liste de complétion ouverte (F-41).
    pub completion: Option<Completion>,
    /// Infobulle de survol ouverte (F-43).
    pub hover: Option<Hover>,
    /// Panneau « Problèmes » affiché (Ctrl+Shift+M).
    pub problems_open: bool,
    /// État du serveur de langage, pour la barre de statut.
    pub lsp_status: String,
    /// Aller à la définition dans un autre fichier : cible appliquée à l'arrivée de son `FileOpened`.
    pub pending_jump: Option<Location>,
    /// Requêtes LSP en vol (interne au cœur).
    pub lsp_pending: LspPending,
    /// Coloration des lignes visibles du document actif, calculée par l'app avant chaque rendu :
    /// plages en OCTETS de la rope, triées, sans chevauchement, et catégorie (`"keyword"`, `"string"`…).
    pub highlights: Vec<(std::ops::Range<usize>, &'static str)>,
    pub focus: Focus,
    /// Fichier modifié à la fois dans l'onglet et sur le disque : chemin et texte disque proposé
    /// (Alt+R recharge, Alt+K garde la version de l'éditeur).
    pub disk_conflict: Option<(PathBuf, String)>,
    /// Copie de secours proposée au démarrage : chemin et texte (Alt+B restaure, Alt+N ignore).
    pub backup_offer: Option<(PathBuf, String)>,
    /// Écran d'aide des raccourcis (F1) ouvert : index de sa première ligne affichée.
    pub help: Option<usize>,
}

/// Lignes de l'aide visibles à la fois : écran moins le cadre et une marge en haut et en bas.
pub fn help_rows(state: &State) -> usize {
    usize::from(state.height.saturating_sub(4)).max(1)
}

/// Largeur occupée par l'explorateur (0 s'il est masqué), partagée par l'UI et le calcul du défilement.
pub fn explorer_width(state: &State) -> u16 {
    if state.explorer.visible {
        state.explorer.width.min(state.width)
    } else {
        0
    }
}

/// Lignes de la barre de recherche, en bas de la zone éditeur : 0, 1 (rechercher) ou 2 (remplacer).
pub fn search_bar_height(state: &State) -> u16 {
    state
        .search
        .as_ref()
        .map_or(0, |s| if s.replace.is_some() { 2 } else { 1 })
}

/// Bandeau sous la barre d'onglets, tant qu'une copie de secours attend Alt+B / Alt+N : 0 ou 1 ligne.
pub fn banner_height(state: &State) -> u16 {
    u16::from(state.backup_offer.is_some())
}

/// Taille (colonnes, lignes) de la zone de texte : tout sauf l'explorateur, les barres d'onglets,
/// de recherche et de statut, le bandeau et le panneau « Problèmes » s'il est ouvert.
pub fn editor_size(state: &State) -> (u16, u16) {
    let problems = if state.problems_open {
        PROBLEMS_HEIGHT
    } else {
        0
    };
    let bars = TAB_BAR_HEIGHT + banner_height(state) + 1 + search_bar_height(state);
    (
        state.width.saturating_sub(explorer_width(state)),
        state.height.saturating_sub(bars + problems),
    )
}

/// Seule fonction qui modifie l'état. Synchrone, rapide, sans I/O.
pub fn update(state: &mut State, event: Event) -> Vec<Command> {
    editor::update(state, event)
}

impl KeyInput {
    /// Lit un raccourci de config.toml : `"ctrl+shift+k"`, `"alt+1"`, `"f12"`, `"ctrl+space"`, `"ctrl+pagedown"`…
    /// (insensible à la casse). `None` si la touche est inconnue.
    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = Modifiers::default();
        let parts: Vec<String> = s.split('+').map(|p| p.trim().to_lowercase()).collect();
        let (name, prefix) = parts.split_last()?;
        for m in prefix {
            match m.as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" => mods.alt = true,
                "shift" => mods.shift = true,
                _ => return None,
            }
        }
        let key = match name.as_str() {
            "space" => Key::Char(' '),
            "enter" | "return" => Key::Enter,
            "esc" | "escape" => Key::Esc,
            "backspace" => Key::Backspace,
            "tab" => Key::Tab,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "delete" | "del" => Key::Delete,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            f if f.len() > 1 && f.starts_with('f') => {
                Key::F(f[1..].parse().ok().filter(|n| (1..=24).contains(n))?)
            }
            c => {
                let mut chars = c.chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Key::Char(ch)
            }
        };
        Some(Self { key, mods })
    }
}

/// Description lisible, ex. "Ctrl+Alt+Shift+Entrée".
impl std::fmt::Display for KeyInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (on, name) in [
            (self.mods.ctrl, "Ctrl+"),
            (self.mods.alt, "Alt+"),
            (self.mods.shift, "Shift+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        match self.key {
            Key::Char(' ') => f.write_str("Espace"),
            // Lettres en majuscule après un modificateur : "Ctrl+S", mais "a" seul.
            Key::Char(c) if self.mods != Modifiers::default() => {
                write!(f, "{}", c.to_uppercase())
            }
            Key::Char(c) => write!(f, "{c}"),
            Key::Enter => f.write_str("Entrée"),
            Key::Esc => f.write_str("Échap"),
            Key::Backspace => f.write_str("Retour arrière"),
            Key::Tab => f.write_str("Tab"),
            Key::Up => f.write_str("Haut"),
            Key::Down => f.write_str("Bas"),
            Key::Left => f.write_str("Gauche"),
            Key::Right => f.write_str("Droite"),
            Key::Delete => f.write_str("Suppr"),
            Key::Home => f.write_str("Début"),
            Key::End => f.write_str("Fin"),
            Key::PageUp => f.write_str("Page préc."),
            Key::PageDown => f.write_str("Page suiv."),
            Key::F(n) => write!(f, "F{n}"),
            Key::Other => f.write_str("Touche inconnue"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, ctrl: bool, alt: bool, shift: bool) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers { ctrl, alt, shift },
        })
    }

    #[test]
    fn ctrl_q_quitte() {
        let mut s = State::default();
        assert_eq!(
            update(&mut s, key(Key::Char('q'), true, false, false)),
            vec![Command::Quit]
        );
        assert_eq!(
            update(&mut s, key(Key::Char('Q'), true, false, true)),
            vec![Command::Quit]
        );
    }

    #[test]
    fn q_sans_ctrl_insere() {
        let mut s = State::default();
        assert!(update(&mut s, key(Key::Char('q'), false, false, false)).is_empty());
        assert_eq!(s.doc.rope().to_string(), "q");
        assert!(update(&mut s, key(Key::Char('q'), false, true, false)).is_empty());
        assert_eq!(s.status, "Alt+Q");
        assert_eq!(s.doc.rope().to_string(), "q");
    }

    #[test]
    fn resize_met_a_jour_les_dimensions() {
        let mut s = State::default();
        assert!(
            update(
                &mut s,
                Event::Resize {
                    width: 120,
                    height: 40
                }
            )
            .is_empty()
        );
        assert_eq!((s.width, s.height), (120, 40));
        assert!(s.status.is_empty());
    }

    #[test]
    fn tick_ne_fait_rien() {
        let mut s = State::default();
        assert!(update(&mut s, Event::Tick).is_empty());
        assert_eq!((s.width, s.height, s.status.as_str()), (0, 0, ""));
    }

    #[test]
    fn autres_touches_dans_le_statut() {
        let cas = [
            (key(Key::Char('k'), true, false, false), "Ctrl+K"),
            (key(Key::F(12), false, false, false), "F12"),
            (key(Key::Enter, true, false, false), "Ctrl+Entrée"),
            (key(Key::Esc, false, false, false), "Échap"),
            (key(Key::Char(' '), false, true, false), "Alt+Espace"),
            (key(Key::Other, false, false, false), "Touche inconnue"),
            (key(Key::Left, true, true, true), "Ctrl+Alt+Shift+Gauche"),
            (key(Key::Char('x'), true, true, true), "Ctrl+Alt+Shift+X"),
        ];
        for (event, attendu) in cas {
            let mut s = State::default();
            assert!(update(&mut s, event).is_empty());
            assert_eq!(s.status, attendu);
        }
    }
}
