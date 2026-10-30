//! Explorateur de fichiers (F-01, F-03, F-04) : arbre affiché, sélection, invites. Zéro I/O :
//! le listage et les opérations disque passent par des `Command` exécutées par l'app.

use std::path::PathBuf;

use crate::{Command, KeyInput, MouseInput, State};

/// Entrée d'un dossier listé par l'app (miroir de `ide_workspace::DirEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Une ligne visible de l'arbre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub entry: FsEntry,
    /// Profondeur sous la racine (0 = enfant direct).
    pub depth: usize,
    /// Dossier déplié (ses enfants suivent dans `nodes`).
    pub expanded: bool,
    /// Listage demandé, réponse pas encore arrivée.
    pub loading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    NewFile,
    NewDir,
    Rename,
    Move,
    /// Confirmer la mise à la corbeille : `o`/`y` ou Entrée confirme, Échap ou `n` annule.
    ConfirmTrash,
}

/// Invite en bas du panneau (nom à saisir, ou confirmation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    /// Dossier ouvert ; `None` tant que l'app ne l'a pas fourni (`Event::OpenRoot`).
    pub root: Option<PathBuf>,
    /// Lignes visibles, dans l'ordre d'affichage.
    pub nodes: Vec<Node>,
    /// Index dans `nodes` de la ligne sélectionnée.
    pub selected: usize,
    /// Première ligne visible.
    pub scroll: usize,
    /// Panneau affiché (Ctrl+B).
    pub visible: bool,
    /// Largeur du panneau, bordures comprises (redimensionnable).
    pub width: u16,
    pub show_hidden: bool,
    pub prompt: Option<Prompt>,
    /// Dernière erreur de listage de la racine, affichée dans le panneau.
    pub error: Option<String>,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            root: None,
            nodes: Vec::new(),
            selected: 0,
            scroll: 0,
            visible: true,
            width: 30,
            show_hidden: false,
            prompt: None,
            error: None,
        }
    }
}

/// Touches : globales (Ctrl+B, Ctrl+E) et, quand l'explorateur a le focus, navigation et invites.
/// `None` = touche laissée à l'éditeur.
pub(crate) fn key(_state: &mut State, _k: KeyInput) -> Option<Vec<Command>> {
    None
}

/// Souris dans le panneau ; `None` = hors du panneau.
pub(crate) fn mouse(_state: &mut State, _m: MouseInput) -> Option<Vec<Command>> {
    None
}

/// `Event::OpenRoot` : nouvelle racine, à lister.
pub(crate) fn open_root(_state: &mut State, _root: PathBuf) -> Vec<Command> {
    vec![]
}

/// `Event::DirListed` : insère (ou remplace) les enfants de `dir`.
pub(crate) fn listed(_state: &mut State, _dir: PathBuf, _result: Result<Vec<FsEntry>, String>) {}

/// `Event::FsDone` : résultat d'une opération disque ; rafraîchit les dossiers concernés.
pub(crate) fn fs_done(_state: &mut State, _result: Result<FsChange, String>) -> Vec<Command> {
    vec![]
}

/// Ce qu'une opération disque réussie a changé, pour rafraîchir l'arbre et les onglets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    Created(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    Trashed(PathBuf),
}
