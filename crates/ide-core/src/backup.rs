//! Copies de secours (NF-07) et session (F-80), côté cœur : l'app trouve et écrit les fichiers,
//! le cœur propose la restauration et replace la vue.

use std::path::{Path, PathBuf};

use crate::{Command, Document, Key, KeyInput, State, tabs};

/// Index d'affichage de l'onglet ouvert sur `path`.
fn index(state: &State, path: &Path) -> Option<usize> {
    if state.doc.path.as_deref() == Some(path) {
        return Some(state.active);
    }
    let i = state
        .tabs
        .iter()
        .position(|t| t.doc.path.as_deref() == Some(path))?;
    Some(if i < state.active { i } else { i + 1 })
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

/// `Event::BackupFound` : la copie attend Alt+B / Alt+N.
pub(crate) fn found(state: &mut State, path: PathBuf, text: String) {
    state.status = format!(
        "Copie de secours de {} plus récente que le disque : Alt+B restaurer, Alt+N ignorer",
        name(&path)
    );
    state.backup_offer = Some((path, text));
}

/// Alt+B / Alt+N quand une copie est proposée ; `None` = touche laissée à la suite.
pub(crate) fn key(state: &mut State, k: KeyInput) -> Option<Vec<Command>> {
    let restore = match k.key {
        Key::Char('b' | 'B') => true,
        Key::Char('n' | 'N') => false,
        _ => return None,
    };
    if !k.mods.alt || k.mods.ctrl {
        return None;
    }
    let (path, text) = state.backup_offer.take()?;
    let n = name(&path);
    if !restore {
        state.status = format!("Copie de secours de {n} supprimée");
        return Some(vec![Command::DiscardBackup(path)]);
    }
    let Some(i) = index(state, &path) else {
        // Onglet fermé entre-temps : la copie reste proposée, rien n'est perdu.
        state.status = format!("{n} n'est plus ouvert : rouvrez-le pour restaurer (Alt+N ignore)");
        state.backup_offer = Some((path, text));
        return Some(vec![]);
    };
    tabs::activate(state, i);
    // Une seule édition annulable, comme un rechargement disque ; le document devient modifié.
    let fresh = Document::from_text(None, &text);
    let doc = &mut state.doc;
    let cursor = doc.cursor();
    doc.line_ending = fresh.line_ending;
    doc.edit(0..doc.rope().len_chars(), &fresh.rope().to_string());
    doc.set_cursor(cursor, false);
    state.status = format!("{n} restauré depuis la copie de secours (Ctrl+Z annule)");
    Some(vec![])
}

/// `Event::RestoreView` : curseur (ligne, colonne en chars) et défilement, bornés au texte.
pub(crate) fn restore_view(
    state: &mut State,
    path: &Path,
    (line, col): (usize, usize),
    scroll_line: usize,
) {
    let (doc, scroll) = if state.doc.path.as_deref() == Some(path) {
        (&mut state.doc, &mut state.scroll_line)
    } else {
        match state
            .tabs
            .iter_mut()
            .find(|t| t.doc.path.as_deref() == Some(path))
        {
            Some(t) => (&mut t.doc, &mut t.scroll_line),
            None => return,
        }
    };
    let last = doc.rope().len_lines() - 1;
    let line = line.min(last);
    let col = col.min(doc.line_text(line).chars().count());
    doc.set_cursor(doc.rope().line_to_char(line) + col, false);
    *scroll = scroll_line.min(last);
}

/// `Event::ActivateTab` : active l'onglet de `path` s'il est ouvert.
pub(crate) fn activate_tab(state: &mut State, path: &Path) {
    if let Some(i) = index(state, path) {
        tabs::activate(state, i);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Command, Event, Key, KeyInput, Modifiers, State, update};
    use std::path::PathBuf;

    fn touche(c: char, ctrl: bool, alt: bool) -> Event {
        Event::Key(KeyInput {
            key: Key::Char(c),
            mods: Modifiers {
                ctrl,
                alt,
                shift: false,
            },
        })
    }
    fn deux() -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 10,
            },
        );
        for (p, t) in [("/x/a.txt", "disque\r\n"), ("/x/b.txt", "bbb")] {
            update(
                &mut s,
                Event::FileOpened {
                    path: p.into(),
                    text: t.into(),
                },
            );
        }
        s
    }
    fn offre(s: &mut State) {
        update(
            s,
            Event::BackupFound {
                path: "/x/a.txt".into(),
                text: "copie\r\nsuite\r\n".into(),
            },
        );
    }

    #[test]
    fn bandeau_retire_une_ligne_et_decale_la_souris() {
        use crate::{MouseInput, MouseKind, banner_height, editor_size};
        let mut s = deux();
        update(
            &mut s,
            Event::FileOpened {
                path: "/x/c.txt".into(),
                text: "l0\nl1\nl2".into(),
            },
        );
        let h = editor_size(&s).1;
        assert_eq!(banner_height(&s), 0);
        offre(&mut s);
        assert_eq!(banner_height(&s), 1);
        assert_eq!(editor_size(&s).1, h - 1);
        let col = crate::explorer_width(&s) + crate::gutter_width(&s) + 1;
        let clic = |s: &mut State, row| {
            update(
                s,
                Event::Mouse(MouseInput {
                    kind: MouseKind::Down,
                    column: col,
                    row,
                    mods: Modifiers::default(),
                }),
            );
            s.doc.cursor_line_col()
        };
        // Ligne 0 : onglets ; ligne 1 : bandeau (ignorée) ; ligne 2 : première ligne du texte.
        assert_eq!(clic(&mut s, 3), (1, 1));
        assert_eq!(clic(&mut s, 1), (1, 1));
        assert_eq!(clic(&mut s, 2), (0, 1));
        // Offre réglée : le texte remonte d'une ligne.
        update(&mut s, touche('n', false, true));
        assert_eq!(editor_size(&s).1, h);
        assert_eq!(clic(&mut s, 1), (0, 1));
        assert_eq!(clic(&mut s, 2), (1, 1));
    }

    #[test]
    fn copie_trouvee_proposee() {
        let mut s = deux();
        offre(&mut s);
        assert_eq!(
            s.status,
            "Copie de secours de a.txt plus récente que le disque : Alt+B restaurer, Alt+N ignorer"
        );
        assert_eq!(
            s.backup_offer,
            Some((PathBuf::from("/x/a.txt"), "copie\r\nsuite\r\n".into()))
        );
    }

    #[test]
    fn alt_b_restaure_annulable_et_modifie() {
        let mut s = deux();
        offre(&mut s);
        assert!(update(&mut s, touche('b', false, true)).is_empty());
        assert!(s.backup_offer.is_none());
        // L'onglet inactif a.txt est activé et restauré, fins de ligne comprises.
        assert_eq!(s.doc.display_name(), "a.txt");
        assert_eq!(s.doc.rope().to_string(), "copie\nsuite\n");
        assert_eq!(s.doc.to_saved_text(), "copie\r\nsuite\r\n");
        assert!(s.doc.is_dirty());
        assert!(s.status.contains("restauré"), "{}", s.status);
        update(&mut s, touche('z', true, false));
        assert_eq!(s.doc.rope().to_string(), "disque\n");
        assert!(!s.doc.is_dirty());
        // Plus d'offre : Alt+B redevient une touche ordinaire.
        update(&mut s, touche('b', false, true));
        assert_eq!(s.status, "Alt+B");
    }

    #[test]
    fn alt_n_supprime_la_copie() {
        let mut s = deux();
        offre(&mut s);
        assert_eq!(
            update(&mut s, touche('N', false, true)),
            vec![Command::DiscardBackup("/x/a.txt".into())]
        );
        assert!(s.backup_offer.is_none());
        assert_eq!(s.doc.rope().to_string(), "bbb");
    }

    #[test]
    fn recherche_ouverte_prioritaire_et_onglet_ferme() {
        let mut s = deux();
        offre(&mut s);
        update(&mut s, touche('f', true, false));
        update(&mut s, touche('n', false, true));
        // Alt+N n'est pas une touche de la recherche : l'offre est traitée même barre ouverte.
        assert!(s.backup_offer.is_none());
        let mut s = deux();
        offre(&mut s);
        update(&mut s, touche('1', false, true));
        update(&mut s, touche('w', true, false));
        assert!(update(&mut s, touche('b', false, true)).is_empty());
        assert!(
            s.backup_offer.is_some(),
            "copie gardée si l'onglet est fermé"
        );
    }

    #[test]
    fn restore_view_borne_et_activate_tab() {
        let mut s = deux();
        let text = "l\n".repeat(30) + "fin";
        update(
            &mut s,
            Event::FileOpened {
                path: "/x/long.txt".into(),
                text,
            },
        );
        let vue = |s: &mut State, path: &str, cursor, scroll_line| {
            update(
                s,
                Event::RestoreView {
                    path: path.into(),
                    cursor,
                    scroll_line,
                },
            )
        };
        // Onglet inactif : appliqué à lui, visible à son activation.
        vue(&mut s, "/x/a.txt", (0, 99), 50);
        vue(&mut s, "/x/long.txt", (20, 0), 18);
        assert_eq!((s.doc.cursor_line_col(), s.scroll_line), ((20, 0), 18));
        vue(&mut s, "/x/long.txt", (99, 99), 99);
        assert_eq!(s.doc.cursor_line_col(), (30, 3));
        vue(&mut s, "/x/inconnu.txt", (1, 1), 1);
        update(&mut s, Event::ActivateTab("/x/a.txt".into()));
        assert_eq!(s.doc.display_name(), "a.txt");
        assert_eq!((s.doc.cursor(), s.scroll_line), (6, 0));
        update(&mut s, Event::ActivateTab("/x/inconnu.txt".into()));
        assert_eq!(s.doc.display_name(), "a.txt");
    }
}
