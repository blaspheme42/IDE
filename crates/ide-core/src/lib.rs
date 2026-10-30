//! Cœur de l'IDE : état et logique, sans aucune I/O.

use std::path::PathBuf;

pub mod document;
mod editor;
mod history;

pub use document::{Document, LineEnding};
pub use editor::GUTTER_WIDTH;

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
    OpenFailed {
        path: PathBuf,
        error: String,
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
}

#[derive(Debug, Default)]
pub struct State {
    pub width: u16,
    pub height: u16,
    pub status: String,
    pub doc: Document,
    /// Première ligne et première colonne d'affichage visibles dans l'éditeur.
    pub scroll_line: usize,
    pub scroll_col: usize,
    /// Ctrl+Q déjà pressé une fois avec des modifications non sauvegardées.
    pub quit_armed: bool,
}

/// Largeur de l'explorateur, partagée par l'UI et le calcul du défilement.
pub const EXPLORER_WIDTH: u16 = 30;

/// Taille (colonnes, lignes) de la zone de texte : tout sauf l'explorateur et la barre de statut.
pub fn editor_size(state: &State) -> (u16, u16) {
    (
        state.width.saturating_sub(EXPLORER_WIDTH),
        state.height.saturating_sub(1),
    )
}

/// Seule fonction qui modifie l'état. Synchrone, rapide, sans I/O.
pub fn update(state: &mut State, event: Event) -> Vec<Command> {
    editor::update(state, event)
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
            (key(Key::Char('b'), true, false, false), "Ctrl+B"),
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
