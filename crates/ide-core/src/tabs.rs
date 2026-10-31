//! Onglets (F-20, F-21). Le document actif reste dans `state.doc` (et sa vue dans `state.scroll_*`) ;
//! les autres attendent dans `state.tabs`, dans l'ordre d'affichage, l'actif s'insérant à `state.active`.

use std::ops::Range;
use std::path::{Path, PathBuf};

use unicode_width::UnicodeWidthStr;

use crate::{Document, FsChange, Key, KeyInput, State, explorer_width, search};

/// Ligne de la barre d'onglets, en haut de la zone éditeur.
pub const TAB_BAR_HEIGHT: u16 = 1;

/// Onglet inactif : son document et sa vue.
#[derive(Debug, Default)]
pub struct Tab {
    pub doc: Document,
    pub scroll_line: usize,
    pub scroll_col: usize,
}

/// Ce que l'UI affiche d'un onglet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    pub name: String,
    pub dirty: bool,
    pub active: bool,
}

/// Onglets dans l'ordre d'affichage.
pub fn tab_list(state: &State) -> Vec<TabInfo> {
    let info = |d: &Document, active| TabInfo {
        name: d.display_name(),
        dirty: d.is_dirty(),
        active,
    };
    let mut v: Vec<_> = state.tabs.iter().map(|t| info(&t.doc, false)).collect();
    v.insert(state.active.min(v.len()), info(&state.doc, true));
    v
}

/// Texte d'un onglet dans la barre ; les onglets sont séparés par `│`.
pub fn tab_label(t: &TabInfo) -> String {
    format!(" {}{} ", t.name, if t.dirty { " ●" } else { "" })
}

/// Colonnes (relatives à la barre) de chaque onglet, et décalage pour que l'actif soit visible.
fn layout(state: &State) -> (Vec<Range<usize>>, usize) {
    let mut x = 0;
    let mut active_end = 0;
    let ranges = tab_list(state)
        .iter()
        .map(|t| {
            let r = x..x + tab_label(t).width();
            x = r.end + 1;
            if t.active {
                active_end = r.end;
            }
            r
        })
        .collect();
    let width = usize::from(state.width.saturating_sub(explorer_width(state)));
    (ranges, active_end.saturating_sub(width))
}

/// Colonnes masquées à gauche de la barre d'onglets.
pub fn tab_scroll(state: &State) -> u16 {
    u16::try_from(layout(state).1).unwrap_or(u16::MAX)
}

/// Zones de clic (colonnes écran) des onglets visibles, avec leur index d'affichage.
pub fn tab_spans(state: &State) -> Vec<(Range<u16>, usize)> {
    let (ranges, offset) = layout(state);
    let left = usize::from(explorer_width(state));
    let right = usize::from(state.width);
    let col =
        |x: usize| u16::try_from((left + x.saturating_sub(offset)).min(right)).unwrap_or(u16::MAX);
    ranges
        .into_iter()
        .enumerate()
        .map(|(i, r)| (col(r.start)..col(r.end), i))
        .filter(|(r, _)| !r.is_empty())
        .collect()
}

/// Active l'onglet d'index d'affichage `i` ; la recherche ouverte se recalcule sur lui.
pub(crate) fn activate(state: &mut State, i: usize) {
    if i == state.active || i > state.tabs.len() {
        return;
    }
    let current = take_active(state);
    state.tabs.insert(state.active, current);
    let t = state.tabs.remove(i);
    put_active(state, t, i);
}

fn take_active(state: &mut State) -> Tab {
    Tab {
        doc: std::mem::take(&mut state.doc),
        scroll_line: state.scroll_line,
        scroll_col: state.scroll_col,
    }
}

fn put_active(state: &mut State, t: Tab, i: usize) {
    state.doc = t.doc;
    state.scroll_line = t.scroll_line;
    state.scroll_col = t.scroll_col;
    state.active = i;
    state.dragging = false;
    search::refresh(state, false);
}

/// Index d'affichage de l'onglet ouvert sur `path`.
fn find(state: &State, path: &Path) -> Option<usize> {
    if state.doc.path.as_deref() == Some(path) {
        return Some(state.active);
    }
    let i = state
        .tabs
        .iter()
        .position(|t| t.doc.path.as_deref() == Some(path))?;
    Some(if i < state.active { i } else { i + 1 })
}

/// Document de l'onglet ouvert sur `path`, actif ou non.
pub(crate) fn doc_mut<'a>(state: &'a mut State, path: &Path) -> Option<&'a mut Document> {
    if state.doc.path.as_deref() == Some(path) {
        return Some(&mut state.doc);
    }
    state
        .tabs
        .iter_mut()
        .find(|t| t.doc.path.as_deref() == Some(path))
        .map(|t| &mut t.doc)
}

/// Ouvre `doc` : onglet existant sur ce chemin activé (sans recharger), sinon remplace le document
/// actif s'il est vierge, sinon nouvel onglet à droite de l'actif. `false` = déjà ouvert.
pub(crate) fn open(
    state: &mut State,
    path: PathBuf,
    doc: impl FnOnce(PathBuf) -> Document,
) -> bool {
    if let Some(i) = find(state, &path) {
        activate(state, i);
        return false;
    }
    let blank =
        state.doc.path.is_none() && !state.doc.is_dirty() && state.doc.rope().len_chars() == 0;
    let at = if blank {
        state.active
    } else {
        let current = take_active(state);
        state.tabs.insert(state.active, current);
        state.active + 1
    };
    put_active(
        state,
        Tab {
            doc: doc(path),
            ..Tab::default()
        },
        at,
    );
    true
}

/// Ferme l'onglet actif ; le voisin de droite (sinon de gauche) devient actif. Dernier onglet → document vierge.
fn close(state: &mut State) {
    let t = if state.active < state.tabs.len() {
        state.tabs.remove(state.active)
    } else if let Some(t) = state.tabs.pop() {
        state.active -= 1;
        t
    } else {
        Tab::default()
    };
    put_active(state, t, state.active);
}

/// Nombre de documents modifiés, tous onglets confondus.
pub(crate) fn dirty_count(state: &State) -> usize {
    tab_list(state).iter().filter(|t| t.dirty).count()
}

/// Suit une opération disque dans tous les onglets. Idempotente : un chemin déjà déplacé ne correspond plus.
pub(crate) fn on_fs_change(state: &mut State, change: &FsChange) {
    let docs = std::iter::once(&mut state.doc).chain(state.tabs.iter_mut().map(|t| &mut t.doc));
    let mut trashed = false;
    for doc in docs {
        let Some(path) = &doc.path else { continue };
        match change {
            FsChange::Renamed { from, to } => {
                if let Ok(rest) = path.strip_prefix(from) {
                    // `join("")` ajouterait un `/` final.
                    doc.path = Some(if rest.as_os_str().is_empty() {
                        to.clone()
                    } else {
                        to.join(rest)
                    });
                }
            }
            // Contenu et chemin gardés, document marqué modifié : Ctrl+Q avertit et Ctrl+S recrée le fichier.
            FsChange::Trashed(p) if path.starts_with(p) => {
                doc.mark_unsaved();
                trashed = true;
            }
            _ => {}
        }
    }
    if trashed {
        state.status = "fichier supprimé du disque : Ctrl+S le recréera".into();
    }
}

/// Raccourcis d'onglets ; `false` = touche laissée à la suite.
pub(crate) fn key(state: &mut State, k: KeyInput) -> bool {
    let n = state.tabs.len() + 1;
    let (ctrl, alt, shift) = (k.mods.ctrl, k.mods.alt, k.mods.shift);
    match k.key {
        Key::Tab if ctrl && !alt => {
            let step = if shift { n - 1 } else { 1 };
            activate(state, (state.active + step) % n);
        }
        Key::PageDown if ctrl && !alt && !shift => activate(state, (state.active + 1) % n),
        Key::PageUp if ctrl && !alt && !shift => activate(state, (state.active + n - 1) % n),
        Key::Char(c @ '1'..='9') if alt && !ctrl => {
            let i = if c == '9' {
                n - 1
            } else {
                usize::from(c as u8 - b'1')
            };
            activate(state, i);
        }
        Key::Char('w' | 'W') if ctrl && !alt => {
            if state.doc.is_dirty() && !state.close_armed {
                state.close_armed = true;
                state.status = format!(
                    "{} modifié : Ctrl+W à nouveau pour fermer sans sauvegarder",
                    state.doc.display_name()
                );
            } else {
                state.close_armed = false;
                state.status.clear();
                close(state);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Event, Modifiers, MouseInput, MouseKind, Search, editor_size, update};

    fn touche(key: Key, ctrl: bool, alt: bool, shift: bool) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers { ctrl, alt, shift },
        })
    }
    fn ctrl(c: char) -> Event {
        touche(Key::Char(c), true, false, false)
    }
    fn ouvrir(s: &mut State, path: &str, text: &str) {
        update(
            s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        );
    }
    /// 80×10, explorateur de 30 colonnes, trois fichiers ouverts : c.txt actif.
    fn trois() -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 10,
            },
        );
        ouvrir(&mut s, "/x/a.txt", "aaa");
        ouvrir(&mut s, "/x/b.txt", "bbb");
        ouvrir(&mut s, "/x/c.txt", "ccc");
        s
    }
    fn noms(s: &State) -> Vec<(String, bool, bool)> {
        tab_list(s)
            .into_iter()
            .map(|t| (t.name, t.dirty, t.active))
            .collect()
    }
    fn actif(s: &State) -> String {
        s.doc.display_name()
    }

    #[test]
    fn ouverture_de_trois_fichiers_et_reouverture() {
        let mut s = trois();
        // Le document vierge du départ a été remplacé par a.txt.
        assert_eq!(
            noms(&s),
            [
                ("a.txt".into(), false, false),
                ("b.txt".into(), false, false),
                ("c.txt".into(), false, true)
            ]
        );
        update(&mut s, touche(Key::Char('1'), false, true, false));
        update(&mut s, touche(Key::Char('z'), false, false, false));
        // Réouverture : pas de doublon, onglet activé, modifications gardées.
        ouvrir(&mut s, "/x/c.txt", "autre");
        assert_eq!(actif(&s), "c.txt");
        assert_eq!(s.doc.rope().to_string(), "ccc");
        ouvrir(&mut s, "/x/a.txt", "autre");
        assert_eq!(s.doc.rope().to_string(), "zaaa");
        // Nouvel onglet à droite de l'actif.
        ouvrir(&mut s, "/x/d.txt", "");
        let n: Vec<_> = noms(&s).into_iter().map(|t| t.0).collect();
        assert_eq!(n, ["a.txt", "d.txt", "b.txt", "c.txt"]);
        assert_eq!(s.active, 1);
    }

    #[test]
    fn open_failed_create_ouvre_un_onglet() {
        let mut s = trois();
        update(
            &mut s,
            Event::OpenFailed {
                path: "/x/neuf.rs".into(),
                error: "introuvable".into(),
                create: true,
            },
        );
        assert_eq!(
            (actif(&s), s.active, s.tabs.len()),
            ("neuf.rs".into(), 3, 3)
        );
    }

    #[test]
    fn saved_sur_un_onglet_inactif() {
        let mut s = trois();
        update(&mut s, touche(Key::Char('x'), false, false, false));
        let cmds = update(&mut s, ctrl('s'));
        let [Command::Save { version, .. }] = cmds[..] else {
            panic!("{cmds:?}")
        };
        update(&mut s, touche(Key::Char('1'), false, true, false));
        assert!(tab_list(&s)[2].dirty);
        update(
            &mut s,
            Event::Saved {
                path: "/x/c.txt".into(),
                version,
            },
        );
        assert!(!tab_list(&s)[2].dirty);
        assert_eq!(actif(&s), "a.txt");
    }

    #[test]
    fn ctrl_w_double_si_modifie_et_dernier_onglet() {
        let mut s = trois();
        update(&mut s, touche(Key::Char('x'), false, false, false));
        update(&mut s, ctrl('w'));
        assert!(s.close_armed && s.status.contains("Ctrl+W à nouveau"));
        assert_eq!(tab_list(&s).len(), 3);
        // Une autre touche désarme.
        update(&mut s, touche(Key::Left, false, false, false));
        assert!(!s.close_armed);
        update(&mut s, ctrl('w'));
        update(&mut s, ctrl('w'));
        // c.txt était le dernier : b.txt, à sa gauche, devient actif.
        assert_eq!((actif(&s), tab_list(&s).len()), ("b.txt".into(), 2));
        update(&mut s, ctrl('w'));
        update(&mut s, ctrl('W'));
        assert_eq!(tab_list(&s).len(), 1);
        assert_eq!(actif(&s), "[sans nom]");
        assert_eq!(s.doc.rope().len_chars(), 0);
    }

    #[test]
    fn ctrl_w_active_le_voisin_de_droite() {
        let mut s = trois();
        update(&mut s, touche(Key::Char('1'), false, true, false));
        update(&mut s, ctrl('w'));
        assert_eq!((actif(&s), s.active), ("b.txt".into(), 0));
    }

    #[test]
    fn ctrl_q_compte_tous_les_onglets_modifies() {
        let mut s = trois();
        update(&mut s, touche(Key::Char('x'), false, false, false));
        update(&mut s, touche(Key::Char('1'), false, true, false));
        update(&mut s, touche(Key::Char('x'), false, false, false));
        update(&mut s, touche(Key::Char('2'), false, true, false));
        // L'onglet actif est propre, mais deux autres ne le sont pas.
        assert!(update(&mut s, ctrl('q')).is_empty());
        assert!(s.status.contains("2 fichiers"), "{}", s.status);
        assert_eq!(update(&mut s, ctrl('q')), vec![Command::Quit]);
    }

    #[test]
    fn navigation_en_boucle_et_alt_n() {
        let mut s = trois();
        let nav = |s: &mut State, e| {
            update(s, e);
            s.active
        };
        assert_eq!(nav(&mut s, touche(Key::Tab, true, false, false)), 0);
        assert_eq!(nav(&mut s, touche(Key::PageDown, true, false, false)), 1);
        assert_eq!(nav(&mut s, touche(Key::Tab, true, false, true)), 0);
        assert_eq!(nav(&mut s, touche(Key::PageUp, true, false, false)), 2);
        assert_eq!(nav(&mut s, touche(Key::Char('2'), false, true, false)), 1);
        assert_eq!(nav(&mut s, touche(Key::Char('9'), false, true, false)), 2);
        // Onglet inexistant : rien.
        assert_eq!(nav(&mut s, touche(Key::Char('5'), false, true, false)), 2);
        assert_eq!(actif(&s), "c.txt");
        // Le texte reste intact.
        assert_eq!(s.doc.rope().to_string(), "ccc");
    }

    fn fs(s: &mut State, change: FsChange) {
        update(s, Event::FsDone(Ok(change)));
    }
    fn chemins(s: &State) -> Vec<Option<PathBuf>> {
        let mut v: Vec<_> = s.tabs.iter().map(|t| t.doc.path.clone()).collect();
        v.insert(s.active, s.doc.path.clone());
        v
    }

    #[test]
    fn renommage_d_un_fichier_ouvert_actif_et_inactif() {
        let mut s = trois();
        let renomme = |s: &mut State, from: &str, to: &str| {
            fs(
                s,
                FsChange::Renamed {
                    from: from.into(),
                    to: to.into(),
                },
            )
        };
        renomme(&mut s, "/x/c.txt", "/x/c2.txt");
        renomme(&mut s, "/x/a.txt", "/x/a2.txt");
        // `/x/b` n'est pas un dossier parent de `/x/b.txt`.
        renomme(&mut s, "/x/b", "/x/z");
        let attendu = ["/x/a2.txt", "/x/b.txt", "/x/c2.txt"].map(|p| Some(PathBuf::from(p)));
        assert_eq!(chemins(&s), attendu);
        assert_eq!(actif(&s), "c2.txt");
        assert!(tab_list(&s).iter().all(|t| !t.dirty));
    }

    #[test]
    fn renommage_d_un_dossier_contenant_des_fichiers_ouverts() {
        let mut s = trois();
        ouvrir(&mut s, "/y/d.txt", "");
        fs(
            &mut s,
            FsChange::Renamed {
                from: "/x".into(),
                to: "/w/x".into(),
            },
        );
        let attendu = ["/w/x/a.txt", "/w/x/b.txt", "/w/x/c.txt", "/y/d.txt"];
        assert_eq!(chemins(&s), attendu.map(|p| Some(PathBuf::from(p))));
    }

    #[test]
    fn corbeille_d_un_fichier_ouvert() {
        let mut s = trois();
        fs(&mut s, FsChange::Trashed("/x/a.txt".into()));
        assert!(s.status.contains("Ctrl+S le recréera"), "{}", s.status);
        assert_eq!(chemins(&s)[0], Some(PathBuf::from("/x/a.txt")));
        assert_eq!(
            noms(&s).into_iter().map(|t| t.1).collect::<Vec<_>>(),
            [true, false, false]
        );
        update(&mut s, touche(Key::Char('1'), false, true, false));
        assert_eq!(s.doc.rope().to_string(), "aaa");
        assert!(update(&mut s, ctrl('q')).is_empty());
        assert!(s.status.contains("1 fichier"), "{}", s.status);
        // Ctrl+S recrée le fichier et le document redevient propre.
        let cmds = update(&mut s, ctrl('s'));
        let [Command::Save { version, .. }] = cmds[..] else {
            panic!("{cmds:?}")
        };
        update(
            &mut s,
            Event::Saved {
                path: "/x/a.txt".into(),
                version,
            },
        );
        assert!(!s.doc.is_dirty());
    }

    #[test]
    fn corbeille_d_un_dossier_et_creation_sans_effet() {
        let mut s = trois();
        fs(&mut s, FsChange::Created("/x/a.txt".into()));
        assert!(tab_list(&s).iter().all(|t| !t.dirty));
        fs(&mut s, FsChange::Trashed("/x".into()));
        assert!(tab_list(&s).iter().all(|t| t.dirty));
    }

    #[test]
    fn raccourcis_d_onglets_avec_le_focus_explorateur() {
        let mut s = trois();
        s.focus = crate::Focus::Explorer;
        update(&mut s, touche(Key::Char('1'), false, true, false));
        assert_eq!(actif(&s), "a.txt");
        update(&mut s, touche(Key::PageDown, true, false, false));
        assert_eq!(actif(&s), "b.txt");
        update(&mut s, touche(Key::PageUp, true, false, false));
        assert_eq!(actif(&s), "a.txt");
        assert_eq!(s.focus, crate::Focus::Explorer);
    }

    #[test]
    fn vue_conservee_par_onglet() {
        let mut s = trois();
        ouvrir(&mut s, "/x/long.txt", &"l\n".repeat(50));
        for _ in 0..20 {
            update(&mut s, touche(Key::Down, false, false, false));
        }
        update(&mut s, touche(Key::Right, false, false, true));
        let vue = (s.scroll_line, s.doc.cursor(), s.doc.selection());
        assert!(vue.0 > 0);
        update(&mut s, touche(Key::Char('1'), false, true, false));
        assert_eq!((s.scroll_line, s.doc.cursor()), (0, 0));
        update(&mut s, touche(Key::Char('9'), false, true, false));
        assert_eq!((s.scroll_line, s.doc.cursor(), s.doc.selection()), vue);
    }

    #[test]
    fn clic_sur_un_onglet() {
        let mut s = trois();
        // « a.txt » commence en colonne 30, après l'explorateur ; chaque onglet fait 7 colonnes + `│`.
        let spans = tab_spans(&s);
        assert_eq!(spans, [(30..37, 0), (38..45, 1), (46..53, 2)]);
        let clic = |s: &mut State, column| {
            update(
                s,
                Event::Mouse(MouseInput {
                    kind: MouseKind::Down,
                    column,
                    row: 0,
                    mods: Modifiers::default(),
                }),
            );
        };
        clic(&mut s, 40);
        assert_eq!(actif(&s), "b.txt");
        // Sur un séparateur : rien.
        clic(&mut s, 37);
        assert_eq!(actif(&s), "b.txt");
        assert!(!s.dragging);
    }

    #[test]
    fn barre_defile_pour_montrer_l_actif() {
        let mut s = trois();
        s.width = 45; // 15 colonnes de barre
        assert_eq!(tab_scroll(&s), 53 - 30 - 15);
        assert_eq!(tab_spans(&s), [(30..37, 1), (38..45, 2)]);
        update(&mut s, touche(Key::Char('1'), false, true, false));
        assert_eq!(tab_scroll(&s), 0);
        assert_eq!(tab_spans(&s), [(30..37, 0), (38..45, 1)]);
    }

    #[test]
    fn recherche_ouverte_pendant_un_changement_d_onglet() {
        let mut s = trois();
        ouvrir(&mut s, "/x/d.txt", "c c c");
        update(&mut s, ctrl('f'));
        update(&mut s, touche(Key::Char('c'), false, false, false));
        assert_eq!(s.status, "1/3");
        update(&mut s, touche(Key::PageUp, true, false, false));
        assert_eq!(actif(&s), "c.txt");
        let m = s.search.as_ref().map(|x| x.matches.clone());
        assert_eq!(m, Some(vec![0..1, 1..2, 2..3]));
        assert_eq!(s.status, "1/3");
        // La recherche ne déplace pas la sélection de l'onglet activé.
        assert_eq!(s.doc.selection(), None);
        assert!(matches!(s.search, Some(Search { .. })));
        // La barre de recherche et la barre d'onglets sont retirées de la hauteur.
        assert_eq!(editor_size(&s).1, 10 - 1 - 1 - TAB_BAR_HEIGHT);
    }
}
