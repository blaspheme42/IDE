//! Palettes (F-50, F-51) : ouvrir un fichier par recherche floue (Ctrl+P) et lancer une commande
//! (Ctrl+Shift+P). Une commande de la palette rejoue simplement son raccourci clavier.

use crate::{Command, KeyInput, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    Files,
    Commands,
}

/// Palette ouverte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub kind: PaletteKind,
    pub query: String,
    /// Index dans `palette_items(state)`.
    pub selected: usize,
}

/// Ligne affichée : libellé (nom de fichier ou commande) et détail (dossier relatif ou raccourci).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub label: String,
    pub detail: String,
}

/// Éléments filtrés et triés par pertinence pour la requête, au plus 50.
pub fn palette_items(_state: &State) -> Vec<PaletteItem> {
    vec![]
}

/// Touches : Ctrl+P / Ctrl+Shift+P ouvrent ; palette ouverte, toutes les touches lui reviennent
/// (saisie, Haut/Bas, Entrée, Échap). `None` = touche laissée à la suite.
pub(crate) fn key(_state: &mut State, _k: KeyInput) -> Option<Vec<Command>> {
    None
}
