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
    match event {
        Event::Key(KeyInput {
            key: Key::Char('q' | 'Q'),
            mods: Modifiers { ctrl: true, .. },
        }) => vec![Command::Quit],
        Event::Key(input) => {
            state.status = input.to_string();
            Vec::new()
        }
        Event::Resize { width, height } => {
            state.width = width;
            state.height = height;
            Vec::new()
        }
        Event::Tick => Vec::new(),
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
    fn q_sans_ctrl_ne_quitte_pas() {
        let mut s = State::default();
        assert!(update(&mut s, key(Key::Char('q'), false, false, false)).is_empty());
        assert_eq!(s.status, "q");
        assert!(update(&mut s, key(Key::Char('q'), false, true, false)).is_empty());
        assert_eq!(s.status, "Alt+Q");
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
            (key(Key::Char('s'), true, false, false), "Ctrl+S"),
            (key(Key::Char('a'), false, false, false), "a"),
            (key(Key::F(12), false, false, false), "F12"),
            (key(Key::Enter, false, false, false), "Entrée"),
            (key(Key::Esc, false, false, false), "Échap"),
            (key(Key::Char(' '), false, false, false), "Espace"),
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
