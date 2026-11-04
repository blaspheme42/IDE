//! Terminal intégré (F-60) : panneau bas, un seul shell. Le PTY et l'émulation vivent dans l'app ;
//! le cœur ne garde que l'ouverture, le focus, la taille et la grille à afficher.

use crate::editor::WHEEL_LINES;
use crate::{
    Command, Focus, Key, KeyInput, MouseInput, MouseKind, PROBLEMS_HEIGHT, State, TAB_BAR_HEIGHT,
    explorer_width,
};

/// Hauteur du panneau, bordure haute comprise.
pub const TERMINAL_HEIGHT: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    #[default]
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

/// Suite de cellules de même style sur une ligne de la grille.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TermSpan {
    pub text: String,
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Default)]
pub struct Terminal {
    pub open: bool,
    /// Grille affichée, remplie par l'app avant chaque rendu.
    pub lines: Vec<Vec<TermSpan>>,
    /// (ligne, colonne) dans la grille ; `None` = masqué.
    pub cursor: Option<(u16, u16)>,
    /// Lignes d'historique remontées, 0 = direct ; borné par l'app.
    pub scroll: usize,
}

/// Hauteur du panneau, 0 si fermé. Ne dépend ni de la recherche ni du bandeau : les ouvrir ne doit
/// pas envoyer de SIGWINCH au shell.
pub fn terminal_height(s: &State) -> u16 {
    if !s.terminal.open {
        return 0;
    }
    let problems = if s.problems_open { PROBLEMS_HEIGHT } else { 0 };
    TERMINAL_HEIGHT.min(s.height.saturating_sub(TAB_BAR_HEIGHT + 1 + problems) / 2)
}

/// (colonnes, lignes) de la grille : largeur moins l'explorateur, hauteur moins la bordure.
pub fn terminal_size(s: &State) -> (u16, u16) {
    (
        s.width.saturating_sub(explorer_width(s)),
        terminal_height(s).saturating_sub(1),
    )
}

fn is_toggle(k: KeyInput) -> bool {
    match k.key {
        Key::Char('ù' | 'Ù') => k.mods.ctrl && !k.mods.alt,
        Key::Char('j' | 'J') => k.mods.alt && !k.mods.ctrl,
        _ => false,
    }
}

fn toggle(state: &mut State) {
    if state.terminal.open && state.focus == Focus::Terminal {
        state.terminal.open = false;
        state.focus = Focus::Editor;
    } else {
        state.terminal.open = true;
        state.focus = Focus::Terminal;
    }
}

/// Panneau invisible (écran trop petit) : touches et collages partiraient à l'aveugle, Ctrl+Q compris.
pub(crate) fn release_hidden_focus(state: &mut State) {
    if state.focus == Focus::Terminal && matches!(terminal_size(state), (0, _) | (_, 0)) {
        state.focus = Focus::Editor;
    }
}

/// `k` : touche après `[keys]`, qui décide de la bascule ; `pressed` : celle que le shell reçoit.
/// `None` = touche laissée à la suite.
pub(crate) fn key(
    state: &mut State,
    k: KeyInput,
    pressed: KeyInput,
    replaying: bool,
) -> Option<Vec<Command>> {
    if state.help.is_some() || state.palette.is_some() {
        return None;
    }
    // `pressed` aussi : la bascule annoncée dans la barre sort du terminal même si `[keys]` la prend.
    if is_toggle(k) || (state.focus == Focus::Terminal && is_toggle(pressed)) {
        toggle(state);
        return Some(vec![]);
    }
    if state.focus == Focus::Terminal && !replaying {
        state.terminal.scroll = 0;
        return Some(vec![Command::TerminalKey(pressed)]);
    }
    None
}

/// Clic ou molette dans le panneau. Un glisser ou un relâchement n'en est pas : celui venu de
/// l'éditeur continue.
pub(crate) fn owns_mouse(state: &State, m: MouseInput) -> bool {
    let bottom = state.height.saturating_sub(1);
    let top = bottom.saturating_sub(terminal_height(state));
    matches!(
        m.kind,
        MouseKind::Down | MouseKind::ScrollUp | MouseKind::ScrollDown
    ) && (top..bottom).contains(&m.row)
        && m.column >= explorer_width(state)
}

pub(crate) fn mouse(state: &mut State, m: MouseInput) {
    let t = &mut state.terminal;
    match m.kind {
        MouseKind::ScrollUp => t.scroll = t.scroll.saturating_add(WHEEL_LINES),
        MouseKind::ScrollDown => t.scroll = t.scroll.saturating_sub(WHEEL_LINES),
        _ => {
            state.focus = Focus::Terminal;
            state.dragging = false;
        }
    }
}

/// Le shell s'est terminé : panneau fermé et vidé.
pub(crate) fn exited(state: &mut State, error: Option<String>) {
    state.terminal = Terminal::default();
    if state.focus == Focus::Terminal {
        state.focus = Focus::Editor;
    }
    if let Some(error) = error {
        state.status = format!("Terminal : {error}");
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Command, Event, Focus, Key, KeyInput, Modifiers, MouseInput, MouseKind, PROBLEMS_HEIGHT,
        State, TERMINAL_HEIGHT, TermSpan, editor_size, terminal_height, terminal_size, update,
    };

    fn touche(key: Key, ctrl: bool, alt: bool) -> KeyInput {
        KeyInput {
            key,
            mods: Modifiers {
                ctrl,
                alt,
                shift: false,
            },
        }
    }

    fn bascule() -> Event {
        Event::Key(touche(Key::Char('ù'), true, false))
    }

    /// Écran 80×40, document de 100 lignes, explorateur masqué.
    fn etat() -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::FileOpened {
                path: "/p/a.rs".into(),
                text: "ligne\n".repeat(100),
            },
        );
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 40,
            },
        );
        s.explorer.visible = false;
        s.status.clear();
        s
    }

    fn souris(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseInput {
            kind,
            column,
            row,
            mods: Modifiers::default(),
        })
    }

    /// Ouvre la palette de commandes, tape `requete` puis valide (focus d'origine rendu avant la saisie).
    fn palette(s: &mut State, requete: &str) -> Vec<Command> {
        let focus = std::mem::take(&mut s.focus);
        update(s, Event::Key(touche(Key::Char('P'), true, false)));
        s.focus = focus;
        for c in requete.chars() {
            update(s, Event::Key(touche(Key::Char(c), false, false)));
        }
        update(s, Event::Key(touche(Key::Enter, false, false)))
    }

    #[test]
    fn bascule_trois_etats() {
        let mut s = etat();
        assert!(update(&mut s, bascule()).is_empty());
        assert!(s.terminal.open);
        assert_eq!(s.focus, Focus::Terminal);
        // Ouvert sans focus : la bascule le reprend sans fermer.
        s.focus = Focus::Explorer;
        update(&mut s, bascule());
        assert!(s.terminal.open);
        assert_eq!(s.focus, Focus::Terminal);
        update(&mut s, bascule());
        assert!(!s.terminal.open);
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn toutes_les_touches_de_bascule() {
        for k in [
            touche(Key::Char('ù'), true, false),
            touche(Key::Char('Ù'), true, false),
            touche(Key::Char('j'), false, true),
            touche(Key::Char('J'), false, true),
        ] {
            let mut s = etat();
            update(&mut s, Event::Key(k));
            assert!(s.terminal.open, "{k}");
            update(&mut s, Event::Key(k));
            assert!(!s.terminal.open, "{k}");
        }
    }

    #[test]
    fn focus_terminal_tout_part_au_shell() {
        let mut s = etat();
        update(&mut s, bascule());
        s.terminal.scroll = 7;
        let v = s.doc.version();
        for k in [
            touche(Key::Char('q'), true, false),
            touche(Key::Char('p'), true, false),
            touche(Key::F(1), false, false),
            touche(Key::Char('s'), true, false),
            touche(Key::Char('a'), false, false),
            touche(Key::Esc, false, false),
        ] {
            assert_eq!(
                update(&mut s, Event::Key(k)),
                vec![Command::TerminalKey(k)],
                "{k}"
            );
        }
        assert_eq!(s.terminal.scroll, 0);
        assert_eq!((s.help, s.palette.is_none()), (None, true));
        assert_eq!(s.doc.version(), v);
    }

    #[test]
    fn altgr_arrive_au_shell_comme_caractere() {
        let mut s = etat();
        update(&mut s, bascule());
        assert_eq!(
            update(&mut s, Event::Key(touche(Key::Char('{'), true, true))),
            vec![Command::TerminalKey(touche(Key::Char('{'), false, false))]
        );
    }

    #[test]
    fn keys_remappe_la_bascule() {
        let mut s = etat();
        let f5 = touche(Key::F(5), false, false);
        s.settings.keys = vec![(f5, "toggle_terminal".into())];
        update(&mut s, Event::Key(f5));
        assert_eq!((s.terminal.open, s.focus), (true, Focus::Terminal));
        assert!(update(&mut s, Event::Key(f5)).is_empty());
        assert_eq!((s.terminal.open, s.focus), (false, Focus::Editor));
    }

    #[test]
    fn keys_autre_commande_laisse_la_touche_brute_au_shell() {
        let mut s = etat();
        let f5 = touche(Key::F(5), false, false);
        s.settings.keys = vec![(f5, "save".into())];
        update(&mut s, bascule());
        assert_eq!(
            update(&mut s, Event::Key(f5)),
            vec![Command::TerminalKey(f5)]
        );
    }

    #[test]
    fn aide_ouverte_garde_la_bascule() {
        let mut s = etat();
        update(&mut s, Event::Key(touche(Key::F(1), false, false)));
        update(&mut s, bascule());
        assert!(!s.terminal.open);
        assert!(s.help.is_some());
    }

    #[test]
    fn palette_bascule() {
        let mut s = etat();
        assert!(palette(&mut s, "Terminal").is_empty());
        assert!(s.palette.is_none());
        assert_eq!((s.terminal.open, s.focus), (true, Focus::Terminal));
        assert!(palette(&mut s, "Terminal").is_empty());
        assert_eq!((s.terminal.open, s.focus), (false, Focus::Editor));
    }

    #[test]
    fn palette_rejoue_sur_l_ide_avec_focus_terminal() {
        let mut s = etat();
        s.terminal.open = true;
        s.focus = Focus::Terminal;
        let cmds = palette(&mut s, "sauvegarder");
        assert!(
            matches!(cmds.as_slice(), [Command::RequestFormat { save: true, .. }]),
            "{cmds:?}"
        );
        assert_eq!(s.focus, Focus::Terminal);
        // Le rejeu ne vaut que pour une touche : la suivante repart au shell.
        let a = touche(Key::Char('a'), false, false);
        assert_eq!(update(&mut s, Event::Key(a)), vec![Command::TerminalKey(a)]);
    }

    #[test]
    fn clic_et_molette_dans_le_panneau() {
        let mut s = etat();
        s.terminal.open = true;
        s.scroll_line = 20;
        s.dragging = true;
        let (haut, bas) = (40 - 1 - terminal_height(&s), 40 - 2);
        update(&mut s, souris(MouseKind::Down, 5, haut));
        assert_eq!((s.focus, s.dragging), (Focus::Terminal, false));
        assert_eq!(s.scroll_line, 20);
        update(&mut s, souris(MouseKind::ScrollUp, 5, bas));
        update(&mut s, souris(MouseKind::ScrollUp, 5, bas));
        assert_eq!(s.terminal.scroll, 6);
        update(&mut s, souris(MouseKind::ScrollDown, 5, bas));
        assert_eq!(s.terminal.scroll, 3);
        update(&mut s, souris(MouseKind::ScrollDown, 5, bas));
        update(&mut s, souris(MouseKind::ScrollDown, 5, bas));
        assert_eq!(s.terminal.scroll, 0);
        assert_eq!(s.scroll_line, 20);
    }

    #[test]
    fn hors_du_panneau_la_souris_reste_a_l_ide() {
        let mut s = etat();
        s.terminal.open = true;
        s.explorer.visible = true;
        s.explorer.width = 30;
        let h = terminal_height(&s);
        // Colonne de l'explorateur, ligne juste au-dessus, barre de statut.
        for (col, row) in [(29, 40 - 2), (50, 40 - 2 - h), (50, 40 - 1)] {
            update(&mut s, souris(MouseKind::ScrollUp, col, row));
            assert_eq!(s.terminal.scroll, 0, "{col},{row}");
            update(&mut s, souris(MouseKind::Down, col, row));
            assert_ne!(s.focus, Focus::Terminal, "{col},{row}");
        }
        update(&mut s, souris(MouseKind::Down, 30, 40 - 2));
        assert_eq!(s.focus, Focus::Terminal);
        // Fermé, ses lignes sont à l'éditeur.
        let mut s = etat();
        update(&mut s, souris(MouseKind::Down, 5, 40 - 2));
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn glisser_de_l_editeur_continue_sous_le_panneau() {
        let mut s = etat();
        s.terminal.open = true;
        update(&mut s, souris(MouseKind::Down, 10, 3));
        assert!(s.dragging);
        update(&mut s, souris(MouseKind::Drag, 10, 40 - 3));
        assert!(s.dragging);
        assert_eq!(s.focus, Focus::Editor);
        assert!(s.doc.selection().is_some());
    }

    #[test]
    fn la_vue_ne_suit_le_curseur_que_si_le_document_bouge() {
        let mut s = etat();
        update(&mut s, bascule());
        s.scroll_line = 50;
        update(&mut s, Event::Key(touche(Key::Char('a'), false, false)));
        assert_eq!(s.scroll_line, 50);
        // Fichier raccourci par une commande du shell : la vue ne reste pas après la fin.
        update(
            &mut s,
            Event::FileChanged {
                path: "/p/a.rs".into(),
                text: Some("court\n".into()),
            },
        );
        assert_eq!(s.scroll_line, 0);
    }

    #[test]
    fn ouvrir_garde_le_curseur_visible() {
        let mut s = etat();
        let bas = usize::from(editor_size(&s).1) - 1;
        s.doc.set_cursor(s.doc.rope().line_to_char(bas), false);
        update(&mut s, bascule());
        assert_eq!(s.scroll_line, usize::from(TERMINAL_HEIGHT));
    }

    #[test]
    fn collage_part_au_shell() {
        let mut s = etat();
        update(&mut s, bascule());
        let v = s.doc.version();
        assert_eq!(
            update(&mut s, Event::Paste("ls\n".into())),
            vec![Command::TerminalPaste("ls\n".into())]
        );
        assert_eq!(s.doc.version(), v);
        // Aide ouverte par-dessus : le collage est perdu, comme ailleurs.
        s.help = Some(0);
        assert!(update(&mut s, Event::Paste("x".into())).is_empty());
        assert_eq!(s.doc.version(), v);
        // Focus éditeur : comportement inchangé.
        s.help = None;
        s.focus = Focus::Editor;
        assert!(update(&mut s, Event::Paste("x".into())).is_empty());
        assert_ne!(s.doc.version(), v);
    }

    #[test]
    fn collage_ramene_la_vue_en_bas() {
        let mut s = etat();
        update(&mut s, bascule());
        s.terminal.scroll = 7;
        update(&mut s, Event::Paste("ls".into()));
        assert_eq!(s.terminal.scroll, 0);
    }

    #[test]
    fn alt_j_sort_du_terminal_meme_pris_par_keys() {
        let mut s = etat();
        update(&mut s, bascule());
        s.settings.keys = vec![(touche(Key::Char('j'), false, true), "save".into())];
        assert!(update(&mut s, Event::Key(touche(Key::Char('j'), false, true))).is_empty());
        assert!(!s.terminal.open);
    }

    #[test]
    fn collage_sur_panneau_invisible_ne_part_pas_au_shell() {
        let mut s = etat();
        update(&mut s, bascule());
        s.height = 2;
        let cmds = update(&mut s, Event::Paste("rm x\n".into()));
        assert!(!matches!(cmds.as_slice(), [Command::TerminalPaste(_)]));
    }

    #[test]
    fn resize_recadre_l_editeur_meme_focus_terminal() {
        let mut s = etat();
        update(&mut s, Event::Key(touche(Key::End, true, false)));
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 40,
            },
        );
        update(&mut s, bascule());
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 30,
            },
        );
        let (_, rows) = crate::editor_size(&s);
        let line = s.doc.cursor_line_col().0;
        assert!(line < s.scroll_line + usize::from(rows), "{line}");
    }

    #[test]
    fn panneau_invisible_rend_les_touches_a_l_editeur() {
        let mut s = etat();
        update(&mut s, bascule());
        s.height = 2;
        let cmds = update(&mut s, Event::Key(touche(Key::Char('q'), true, false)));
        assert!(!matches!(cmds.as_slice(), [Command::TerminalKey(_)]));
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn fin_du_shell_ferme_et_vide() {
        let mut s = etat();
        update(&mut s, bascule());
        s.terminal.lines = vec![vec![TermSpan::default()]];
        s.terminal.cursor = Some((1, 2));
        s.terminal.scroll = 3;
        let v = s.doc.version();
        assert!(update(&mut s, Event::TerminalExited { error: None }).is_empty());
        assert!(!s.terminal.open && s.terminal.lines.is_empty());
        assert_eq!((s.terminal.cursor, s.terminal.scroll), (None, 0));
        assert_eq!(s.focus, Focus::Editor);
        assert_eq!(s.doc.version(), v);
        assert!(s.status.is_empty());
    }

    #[test]
    fn fin_du_shell_avec_erreur_et_focus_ailleurs() {
        let mut s = etat();
        s.terminal.open = true;
        s.focus = Focus::Explorer;
        let error = Some("shell introuvable".into());
        assert!(update(&mut s, Event::TerminalExited { error }).is_empty());
        assert_eq!(s.status, "Terminal : shell introuvable");
        assert_eq!(s.focus, Focus::Explorer);
    }

    #[test]
    fn editor_size_retire_le_panneau() {
        let mut s = etat();
        let h = editor_size(&s).1;
        update(&mut s, bascule());
        assert_eq!(terminal_height(&s), TERMINAL_HEIGHT);
        assert_eq!(editor_size(&s).1, h - TERMINAL_HEIGHT);
        assert_eq!(terminal_size(&s), (80, TERMINAL_HEIGHT - 1));
        update(&mut s, bascule());
        assert_eq!(editor_size(&s).1, h);
        assert_eq!(terminal_size(&s), (80, 0));
    }

    #[test]
    fn hauteur_bornee_et_stable() {
        let mut s = etat();
        s.terminal.open = true;
        s.height = 10;
        assert_eq!(terminal_height(&s), (10 - 2) / 2);
        s.height = 40;
        let h = terminal_height(&s);
        s.backup_offer = Some(("/p/a.rs".into(), String::new()));
        assert_eq!(terminal_height(&s), h);
        s.problems_open = true;
        s.height = 20;
        assert_eq!(terminal_height(&s), (20 - 2 - PROBLEMS_HEIGHT) / 2);
    }
}
