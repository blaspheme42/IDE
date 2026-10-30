//! Traduction des événements en actions sur l'état (clavier, fichiers, défilement).

use crate::{
    Command, Document, EXPLORER_WIDTH, Event, Key, KeyInput, MouseInput, MouseKind, State,
    editor_size, search,
};

/// Colonnes réservées par l'UI aux numéros de ligne, à gauche du texte.
pub const GUTTER_WIDTH: u16 = 6;

/// Lignes défilées par cran de molette.
const WHEEL_LINES: usize = 3;

pub(crate) fn update(state: &mut State, event: Event) -> Vec<Command> {
    // La molette déplace la vue sans le curseur : ne pas la ramener sur lui ensuite.
    let wheel = matches!(&event, Event::Mouse(m) if matches!(m.kind, MouseKind::ScrollUp | MouseKind::ScrollDown));
    let version = state.doc.version();
    let cmds = match event {
        Event::Key(k) => key(state, k),
        Event::Mouse(m) => {
            mouse(state, m);
            vec![]
        }
        Event::Paste(text) if state.search.is_some() => {
            search::input(state, text.lines().next().unwrap_or_default());
            vec![]
        }
        Event::Paste(text) => {
            state.doc.insert_str(&text);
            vec![]
        }
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
    // Annuler, couper, remplacer… : les occurrences ont bougé.
    if state.search.is_some() && state.doc.version() != version {
        search::refresh(state, false);
    }
    if !wheel {
        scroll_to_cursor(state);
    }
    cmds
}

fn mouse(state: &mut State, m: MouseInput) {
    let last = state.doc.rope().len_lines() - 1;
    let left = EXPLORER_WIDTH + GUTTER_WIDTH;
    match m.kind {
        MouseKind::ScrollUp => state.scroll_line = state.scroll_line.saturating_sub(WHEEL_LINES),
        MouseKind::ScrollDown => state.scroll_line = (state.scroll_line + WHEEL_LINES).min(last),
        MouseKind::Up => {}
        // Un clic hors de la zone de texte est ignoré ; un glisser peut en sortir.
        MouseKind::Down if m.column < left || m.row >= editor_size(state).1 => {}
        MouseKind::Down | MouseKind::Drag => {
            let line = state.scroll_line + usize::from(m.row);
            let col = state.scroll_col + usize::from(m.column.saturating_sub(left));
            let pos = state.doc.pos_at(line, col);
            let extend = m.kind == MouseKind::Drag || m.mods.shift;
            state.doc.set_cursor(pos, extend);
        }
    }
}

/// Texte copié : la sélection, sinon la ligne courante avec son `\n` (comme VSCode).
fn copied(doc: &Document) -> (std::ops::Range<usize>, String) {
    if let Some(r) = doc.selection() {
        return (r.clone(), doc.rope().slice(r).to_string());
    }
    let rope = doc.rope();
    let line = rope.char_to_line(doc.cursor());
    let end = if line + 1 < rope.len_lines() {
        rope.line_to_char(line + 1)
    } else {
        rope.len_chars()
    };
    let r = rope.line_to_char(line)..end;
    let mut text = rope.slice(r.clone()).to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    (r, text)
}

fn key(state: &mut State, k: KeyInput) -> Vec<Command> {
    let ctrl_q = k.mods.ctrl && !k.mods.alt && matches!(k.key, Key::Char('q' | 'Q'));
    if !ctrl_q && state.quit_armed {
        state.quit_armed = false;
        state.status.clear();
    }
    if search::key(state, k) {
        return vec![];
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
            Key::Char('z' | 'Z') if !k.mods.shift => {
                if !doc.undo() {
                    state.status = "Rien à annuler".into();
                }
            }
            Key::Char('z' | 'Z' | 'y' | 'Y') => {
                if !doc.redo() {
                    state.status = "Rien à rétablir".into();
                }
            }
            Key::Char('a' | 'A') => doc.select_all(),
            Key::Char(c @ ('c' | 'C' | 'x' | 'X')) => {
                let (r, text) = copied(doc);
                if c.eq_ignore_ascii_case(&'x') {
                    doc.edit(r, "");
                }
                return vec![Command::CopyToClipboard(text)];
            }
            Key::Char('v' | 'V') => return vec![Command::RequestPaste],
            Key::Char('f' | 'F') => search::open(state, false),
            Key::Char('h' | 'H') => search::open(state, true),
            _ => state.status = k.to_string(),
        }
        return vec![];
    }
    if k.mods.alt {
        state.status = k.to_string();
        return vec![];
    }
    let ext = k.mods.shift;
    match k.key {
        Key::Char(c) if !c.is_control() => doc.insert_char(c),
        Key::Enter => doc.insert_newline(),
        Key::Tab if ext => doc.untab(),
        Key::Tab => doc.tab(),
        Key::Backspace => doc.backspace(),
        Key::Delete => doc.delete(),
        Key::Left => doc.move_left(ext),
        Key::Right => doc.move_right(ext),
        Key::Up => doc.move_up(1, ext),
        Key::Down => doc.move_down(1, ext),
        Key::Home => doc.move_line_start(ext),
        Key::End => doc.move_line_end(ext),
        Key::PageUp => doc.move_up(page, ext),
        Key::PageDown => doc.move_down(page, ext),
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
    use crate::{Modifiers, MouseInput, MouseKind, update};
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
                contents: "abc \r\n".into(),
                version: 2,
            }]
        );
        update(&mut s, k(Key::Char('d')));
        update(
            &mut s,
            Event::Saved {
                path: "/x/a.txt".into(),
                version: 2,
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

    fn avec(key: Key, ctrl: bool, shift: bool) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers {
                ctrl,
                shift,
                ..Modifiers::default()
            },
        })
    }
    fn souris(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseInput {
            kind,
            column,
            row,
            mods: Modifiers::default(),
        })
    }
    fn texte(s: &State) -> String {
        s.doc.rope().to_string()
    }

    #[test]
    fn ctrl_z_y_et_ctrl_shift_z() {
        let mut s = opened("a");
        update(&mut s, k(Key::Char('b')));
        update(&mut s, ctrl('z'));
        assert_eq!(texte(&s), "a");
        assert!(!s.doc.is_dirty());
        update(&mut s, ctrl('y'));
        assert_eq!(texte(&s), "ba");
        update(&mut s, ctrl('z'));
        update(&mut s, avec(Key::Char('Z'), true, true));
        assert_eq!(texte(&s), "ba");
        update(&mut s, avec(Key::Char('Z'), true, true));
        assert_eq!(s.status, "Rien à rétablir");
    }

    #[test]
    fn shift_fleches_et_ctrl_a() {
        let mut s = opened("abc\ndef");
        update(&mut s, avec(Key::End, false, true));
        update(&mut s, avec(Key::Left, false, true));
        assert_eq!(s.doc.selection(), Some(0..2));
        update(&mut s, k(Key::Char('x')));
        assert_eq!(texte(&s), "xc\ndef");
        update(&mut s, ctrl('a'));
        assert_eq!(s.doc.selection(), Some(0..6));
        update(&mut s, k(Key::Tab));
        assert_eq!(texte(&s), "    xc\n    def");
        assert_eq!(s.doc.selection(), Some(0..14));
        update(&mut s, avec(Key::Tab, false, true));
        assert_eq!(texte(&s), "xc\ndef");
        update(&mut s, k(Key::Char(' ')));
        assert_eq!(texte(&s), " ");
        update(&mut s, avec(Key::Home, false, true));
        update(&mut s, k(Key::Delete));
        assert_eq!(texte(&s), "");
    }

    #[test]
    fn copier_couper_coller() {
        let mut s = opened("un\ndeux");
        // Sans sélection : la ligne entière, avec un `\n` même en fin de fichier.
        assert_eq!(
            update(&mut s, ctrl('c')),
            vec![Command::CopyToClipboard("un\n".into())]
        );
        update(&mut s, k(Key::Down));
        assert_eq!(
            update(&mut s, ctrl('x')),
            vec![Command::CopyToClipboard("deux\n".into())]
        );
        assert_eq!(texte(&s), "un\n");
        update(&mut s, k(Key::Up));
        update(&mut s, avec(Key::Right, false, true));
        assert_eq!(
            update(&mut s, ctrl('x')),
            vec![Command::CopyToClipboard("u".into())]
        );
        assert_eq!(texte(&s), "n\n");
        assert_eq!(update(&mut s, ctrl('v')), vec![Command::RequestPaste]);
        update(&mut s, Event::Paste("a\r\nb".into()));
        assert_eq!(texte(&s), "a\nbn\n");
        update(&mut s, ctrl('z'));
        assert_eq!(texte(&s), "n\n");
    }

    #[test]
    fn souris_clic_glisser_defilement() {
        // Texte à partir de la colonne 30 + 6 = 36 ; 5 lignes visibles.
        let mut s = opened("abc\nd中e\n");
        update(&mut s, souris(MouseKind::Down, 37, 0));
        assert_eq!(s.doc.cursor(), 1);
        update(&mut s, souris(MouseKind::Drag, 38, 1)); // colonne 2 = milieu de 中 : avant
        assert_eq!(s.doc.selection(), Some(1..5));
        update(&mut s, souris(MouseKind::Up, 38, 1));
        assert_eq!(s.doc.selection(), Some(1..5));
        // Au-delà de la fin de ligne, puis au-delà du document.
        update(&mut s, souris(MouseKind::Down, 39, 0));
        assert_eq!((s.doc.cursor(), s.doc.selection()), (3, None));
        update(&mut s, souris(MouseKind::Down, 50, 4));
        assert_eq!(s.doc.cursor(), 8);
        // Clic dans l'explorateur ou la barre de statut : ignoré.
        update(&mut s, souris(MouseKind::Down, 5, 0));
        update(&mut s, souris(MouseKind::Down, 37, 5));
        assert_eq!(s.doc.cursor(), 8);
        let mut s = opened(&"l\n".repeat(20));
        update(&mut s, souris(MouseKind::ScrollDown, 37, 0));
        update(&mut s, souris(MouseKind::ScrollDown, 37, 0));
        assert_eq!((s.scroll_line, s.doc.cursor()), (6, 0));
        for _ in 0..10 {
            update(&mut s, souris(MouseKind::ScrollDown, 37, 0));
        }
        assert_eq!(s.scroll_line, 20);
        update(&mut s, souris(MouseKind::ScrollUp, 37, 0));
        assert_eq!(s.scroll_line, 17);
        update(&mut s, souris(MouseKind::Down, 36, 1));
        assert_eq!(s.doc.cursor_line_col(), (18, 0));
    }
}
