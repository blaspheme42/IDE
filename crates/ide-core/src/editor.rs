//! Traduction des événements en actions sur l'état (clavier, fichiers, défilement).

use crate::{Command, Document, Event, Key, KeyInput, State, editor_size};

/// Colonnes réservées par l'UI aux numéros de ligne, à gauche du texte.
pub const GUTTER_WIDTH: u16 = 6;

pub(crate) fn update(state: &mut State, event: Event) -> Vec<Command> {
    let cmds = match event {
        Event::Key(k) => key(state, k),
        Event::Resize { width, height } => {
            state.width = width;
            state.height = height;
            vec![]
        }
        Event::Tick => vec![],
        Event::FileOpened { path, text } => {
            state.doc = Document::from_text(Some(path), &text);
            state.scroll_line = 0;
            state.scroll_col = 0;
            state.status = format!("Ouvert : {}", state.doc.display_name());
            vec![]
        }
        Event::OpenFailed { path, error } => {
            // Le chemin est gardé : Ctrl+S créera le fichier s'il était introuvable.
            state.doc.path = Some(path);
            state.status = format!("Impossible d'ouvrir {} : {error}", state.doc.display_name());
            vec![]
        }
        Event::Saved { version, .. } => {
            state.doc.mark_saved(version);
            state.status = format!("Sauvegardé : {}", state.doc.display_name());
            vec![]
        }
        Event::SaveFailed { error, .. } => {
            state.status = format!("Échec de la sauvegarde : {error}");
            vec![]
        }
    };
    scroll_to_cursor(state);
    cmds
}

fn key(state: &mut State, k: KeyInput) -> Vec<Command> {
    let ctrl_q = k.mods.ctrl && !k.mods.alt && matches!(k.key, Key::Char('q' | 'Q'));
    if !ctrl_q && state.quit_armed {
        state.quit_armed = false;
        state.status.clear();
    }
    let page = usize::from(editor_size(state).1).max(1);
    let doc = &mut state.doc;
    if k.mods.ctrl && !k.mods.alt {
        match k.key {
            Key::Char('q' | 'Q') => {
                if !doc.is_dirty() || state.quit_armed {
                    return vec![Command::Quit];
                }
                state.quit_armed = true;
                state.status =
                    "Modifications non sauvegardées : Ctrl+Q à nouveau pour quitter sans sauvegarder"
                        .into();
            }
            Key::Char('s' | 'S') => match doc.path.clone() {
                Some(path) => {
                    state.status = "Sauvegarde…".into();
                    return vec![Command::Save {
                        path,
                        contents: doc.to_saved_text(),
                        version: doc.version(),
                    }];
                }
                None => state.status = "Aucun fichier : lancez `ide <fichier>`".into(),
            },
            _ => state.status = k.to_string(),
        }
        return vec![];
    }
    if k.mods.alt {
        state.status = k.to_string();
        return vec![];
    }
    match k.key {
        Key::Char(c) if !c.is_control() => doc.insert_char(c),
        Key::Enter => doc.insert_newline(),
        Key::Tab => (0..4).for_each(|_| doc.insert_char(' ')),
        Key::Backspace => doc.backspace(),
        Key::Delete => doc.delete(),
        Key::Left => doc.move_left(),
        Key::Right => doc.move_right(),
        Key::Up => doc.move_up(1),
        Key::Down => doc.move_down(1),
        Key::Home => doc.move_line_start(),
        Key::End => doc.move_line_end(),
        Key::PageUp => doc.move_up(page),
        Key::PageDown => doc.move_down(page),
        _ => state.status = k.to_string(),
    }
    vec![]
}

/// Fait défiler juste assez pour que le curseur reste dans la zone de texte.
fn scroll_to_cursor(state: &mut State) {
    let (line, col) = state.doc.cursor_line_col();
    let (w, h) = editor_size(state);
    let h = usize::from(h).max(1);
    let w = usize::from(w.saturating_sub(GUTTER_WIDTH)).max(1);
    state.scroll_line = state.scroll_line.clamp(line.saturating_sub(h - 1), line);
    state.scroll_col = state.scroll_col.clamp(col.saturating_sub(w - 1), col);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Modifiers, update};
    use std::path::PathBuf;

    fn k(key: Key) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers::default(),
        })
    }
    fn ctrl(c: char) -> Event {
        Event::Key(KeyInput {
            key: Key::Char(c),
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        })
    }
    fn opened(text: &str) -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 40,
                height: 6,
            },
        );
        update(
            &mut s,
            Event::FileOpened {
                path: "/x/a.txt".into(),
                text: text.into(),
            },
        );
        s
    }

    #[test]
    fn ouverture_edition_sauvegarde() {
        let mut s = opened("ab\r\n");
        assert_eq!(s.status, "Ouvert : a.txt");
        update(&mut s, k(Key::End));
        update(&mut s, k(Key::Char('c')));
        update(&mut s, k(Key::Tab));
        assert!(s.doc.is_dirty());
        let cmds = update(&mut s, ctrl('s'));
        assert_eq!(s.status, "Sauvegarde…");
        assert_eq!(
            cmds,
            vec![Command::Save {
                path: PathBuf::from("/x/a.txt"),
                contents: "abc    \r\n".into(),
                version: 5,
            }]
        );
        update(&mut s, k(Key::Char('d')));
        update(
            &mut s,
            Event::Saved {
                path: "/x/a.txt".into(),
                version: 5,
            },
        );
        assert_eq!(s.status, "Sauvegardé : a.txt");
        assert!(s.doc.is_dirty());
        update(
            &mut s,
            Event::SaveFailed {
                path: "/x/a.txt".into(),
                error: "disque plein".into(),
            },
        );
        assert_eq!(s.status, "Échec de la sauvegarde : disque plein");
    }

    #[test]
    fn ctrl_s_sans_chemin() {
        let mut s = State::default();
        assert!(update(&mut s, ctrl('s')).is_empty());
        assert_eq!(s.status, "Aucun fichier : lancez `ide <fichier>`");
    }

    #[test]
    fn open_failed_garde_le_chemin() {
        let mut s = State::default();
        update(
            &mut s,
            Event::OpenFailed {
                path: "/x/neuf.rs".into(),
                error: "introuvable".into(),
            },
        );
        assert_eq!(s.status, "Impossible d'ouvrir neuf.rs : introuvable");
        update(&mut s, k(Key::Char('a')));
        assert!(matches!(
            update(&mut s, ctrl('s')).as_slice(),
            [Command::Save { .. }]
        ));
    }

    #[test]
    fn double_ctrl_q() {
        let mut s = opened("a");
        assert_eq!(update(&mut s, ctrl('q')), vec![Command::Quit]);
        update(&mut s, k(Key::Char('b')));
        assert!(update(&mut s, ctrl('q')).is_empty());
        assert!(s.quit_armed);
        assert!(s.status.starts_with("Modifications non sauvegardées"));
        // Une autre touche désarme.
        update(&mut s, k(Key::Left));
        assert!(!s.quit_armed);
        assert!(update(&mut s, ctrl('q')).is_empty());
        assert_eq!(update(&mut s, ctrl('Q')), vec![Command::Quit]);
    }

    #[test]
    fn scroll_suit_le_curseur() {
        // Zone : 40-30 = 10 colonnes, moins 6 de gouttière = 4 ; 6-1 = 5 lignes.
        let mut s = opened(&"l\n".repeat(20));
        for _ in 0..7 {
            update(&mut s, k(Key::Down));
        }
        assert_eq!(s.scroll_line, 3);
        update(&mut s, k(Key::PageDown));
        assert_eq!(s.doc.cursor_line_col().0, 12);
        assert_eq!(s.scroll_line, 8);
        update(&mut s, k(Key::PageUp));
        update(&mut s, k(Key::PageUp));
        assert_eq!((s.doc.cursor_line_col().0, s.scroll_line), (2, 2));
        for c in "中文ab".chars() {
            update(&mut s, k(Key::Char(c)));
        }
        // Idéogrammes = 2 colonnes : curseur en colonne 6, 4 colonnes visibles.
        assert_eq!(s.doc.cursor_line_col().1, 6);
        assert_eq!(s.scroll_col, 3);
        update(&mut s, k(Key::Home));
        assert_eq!(s.scroll_col, 0);
    }
}
