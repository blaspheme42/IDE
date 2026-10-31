//! Types LSP côté cœur (J4) : positions UTF-16, diagnostics, complétion, survol. Zéro I/O :
//! le client LSP vit dans l'app, qui convertit vers et depuis ces types.

use std::path::PathBuf;

/// Position LSP : ligne 0-based et colonne en unités UTF-16 (encodage par défaut du protocole).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub start: Position,
    pub end: Position,
    pub severity: Severity,
    pub message: String,
    /// Émetteur (`rustc`, `clippy`, `clangd`…).
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    /// Nature courte affichée dans la liste (`fn`, `struct`, `var`…).
    pub kind: Option<String>,
    pub detail: Option<String>,
    /// Texte inséré à la place du mot en cours (sans snippets : `$1`… déjà retirés par l'app).
    pub insert_text: String,
}

/// Liste de complétion ouverte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Éléments reçus du serveur, dans son ordre.
    pub items: Vec<CompletionItem>,
    /// Index dans les éléments filtrés (`visible`).
    pub selected: usize,
    /// Index de `char` du début du mot complété : le texte de `anchor` au curseur sert de filtre.
    pub anchor: usize,
}

/// Infobulle de survol ouverte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    /// Texte brut (Markdown laissé tel quel).
    pub text: String,
    /// Index de `char` du symbole survolé.
    pub at: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub start: Position,
    pub end: Position,
}

/// Serveur à lancer pour un langage (`[lsp.<langage>]` de config.toml).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServer {
    /// Nom en minuscules, comme `ide_syntax::Language::name()` en minuscules (`rust`, `python`, `c`…).
    pub language: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Serveurs par défaut (F-40) : rust-analyzer, pyright, clangd.
pub fn default_servers() -> Vec<LspServer> {
    let s = |language: &str, command: &str, args: &[&str]| LspServer {
        language: language.into(),
        command: command.into(),
        args: args.iter().map(|a| (*a).into()).collect(),
    };
    vec![
        s("rust", "rust-analyzer", &[]),
        s("python", "pyright-langserver", &["--stdio"]),
        s("c", "clangd", &[]),
    ]
}

use crate::{Command, KeyInput, State};

/// Touches de la complétion (liste ouverte : Haut/Bas/Entrée/Tab/Échap) et déclencheurs
/// (Ctrl+Espace, F12, survol au clavier). `None` = touche laissée à la suite.
pub(crate) fn key(_state: &mut State, _k: KeyInput) -> Option<Vec<Command>> {
    None
}

/// Après chaque événement : déclenchement à la frappe, filtrage ou fermeture de la liste.
/// `version_before` = version du document actif avant l'événement.
pub(crate) fn after_update(_state: &mut State, _version_before: u64) -> Vec<Command> {
    vec![]
}

pub(crate) fn completion_result(
    _state: &mut State,
    _path: PathBuf,
    _version: u64,
    _items: Vec<CompletionItem>,
) {
}

pub(crate) fn hover_result(_state: &mut State, _path: PathBuf, _text: Option<String>) {}

/// Aller à la définition : ouvre le fichier si besoin et place le curseur.
pub(crate) fn definition_result(_state: &mut State, _locations: Vec<Location>) -> Vec<Command> {
    vec![]
}
