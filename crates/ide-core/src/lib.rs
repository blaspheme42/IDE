//! Cœur de l'IDE : état et logique, sans aucune I/O.

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

/// Tout ce qui arrive dans la boucle d'événements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key(KeyInput),
    Resize { width: u16, height: u16 },
    Tick,
}

/// Effets demandés par `update`, exécutés par l'app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit,
}

#[derive(Debug, Default)]
pub struct State {
    pub width: u16,
    pub height: u16,
    pub status: String,
}

/// Seule fonction qui modifie l'état. Synchrone, rapide, sans I/O.
pub fn update(state: &mut State, event: Event) -> Vec<Command> {
    let _ = (state, event);
    Vec::new()
}
