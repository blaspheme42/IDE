//! Terminal intégré (F-60) : panneau bas, un shell par onglet. Le PTY et l'émulation vivent dans
//! l'app ; le cœur ne garde que l'ouverture, les onglets, le focus, la taille et la grille à afficher.

use crate::editor::WHEEL_LINES;
use crate::{
    Command, Focus, Key, KeyInput, MouseInput, MouseKind, PROBLEMS_HEIGHT, State, TAB_BAR_HEIGHT,
    explorer_width,
};

/// Hauteur du panneau, bordure haute comprise.
pub const TERMINAL_HEIGHT: u16 = 12;

/// Début de la bordure haute, suivi des onglets.
pub const TERMINAL_TITLE: &str = " TERMINAL ";

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
    /// Identifiants stables des shells, dans l'ordre affiché ; vide tant que rien n'est ouvert.
    pub tabs: Vec<u32>,
    /// Position de l'onglet actif dans `tabs`.
    pub active: usize,
    next_id: u32,
    /// Grille de l'onglet actif, remplie par l'app avant chaque rendu.
    pub lines: Vec<Vec<TermSpan>>,
    /// (ligne, colonne) dans la grille ; `None` = masqué.
    pub cursor: Option<(u16, u16)>,
    /// Lignes d'historique remontées, 0 = direct ; borné par l'app.
    pub scroll: usize,
}

impl Terminal {
    pub fn active_id(&self) -> Option<u32> {
        self.tabs.get(self.active).copied()
    }

    fn new_tab(&mut self) {
        self.tabs.push(self.next_id);
        self.next_id += 1;
        self.select(self.tabs.len() - 1);
    }

    fn select(&mut self, i: usize) {
        if i != self.active {
            self.active = i;
            self.scroll = 0;
        }
    }
}

/// Libellé de l'onglet à la position `i`, celui du rendu comme du clic.
pub fn terminal_tab_label(i: usize) -> String {
    format!(" {} ", i + 1)
}

/// Onglet sous `column`, comptée depuis le bord gauche du panneau.
pub fn terminal_tab_at(t: &Terminal, column: u16) -> Option<usize> {
    let column = usize::from(column);
    let mut end = TERMINAL_TITLE.len();
    if column < end {
        return None;
    }
    (0..t.tabs.len()).find(|&i| {
        end += terminal_tab_label(i).len();
        column < end
    })
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
        if state.terminal.tabs.is_empty() {
            state.terminal.new_tab();
        }
    }
}

/// Alt+N, Alt+Page suiv. et Alt+Page préc. ; `false` = pas une touche d'onglet. `N` : verrouillage
/// majuscule, que le terminal ne distingue pas de Shift. Alt+Shift+Page reste au shell.
fn tab_key(t: &mut Terminal, k: KeyInput) -> bool {
    if !k.mods.alt || k.mods.ctrl {
        return false;
    }
    let n = t.tabs.len().max(1);
    match k.key {
        Key::Char('n' | 'N') => t.new_tab(),
        Key::PageDown if !k.mods.shift => t.select((t.active + 1) % n),
        Key::PageUp if !k.mods.shift => t.select((t.active + n - 1) % n),
        _ => return false,
    }
    true
}

/// Panneau invisible (écran trop petit) : touches et collages partiraient à l'aveugle, Ctrl+Q compris.
pub(crate) fn release_hidden_focus(state: &mut State) {
    if state.focus == Focus::Terminal && matches!(terminal_size(state), (0, _) | (_, 0)) {
        state.focus = Focus::Editor;
    }
}

/// `k` : touche après `[keys]`, qui décide de la bascule ; `pressed` : celle que le shell reçoit.
/// Les onglets passent avant la bascule : `[keys]` ne doit pas pouvoir retirer leurs touches.
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
    if state.focus == Focus::Terminal
        && !replaying
        && (tab_key(&mut state.terminal, pressed) || tab_key(&mut state.terminal, k))
    {
        return Some(vec![]);
    }
    // `pressed` aussi : la bascule annoncée dans la barre sort du terminal même si `[keys]` la prend.
    if is_toggle(k) || (state.focus == Focus::Terminal && is_toggle(pressed)) {
        toggle(state);
        return Some(vec![]);
    }
    if state.focus == Focus::Terminal && !replaying {
        state.terminal.scroll = 0;
        let id = state.terminal.active_id();
        return Some(
            id.map(|id| Command::TerminalKey(id, pressed))
                .into_iter()
                .collect(),
        );
    }
    None
}

/// Clic ou molette dans le panneau. Un glisser ou un relâchement n'en est pas : celui venu de
/// l'éditeur continue.
pub(crate) fn owns_mouse(state: &State, m: MouseInput) -> bool {
    let bottom = state.height.saturating_sub(1);
    matches!(
        m.kind,
        MouseKind::Down | MouseKind::ScrollUp | MouseKind::ScrollDown
    ) && (panel_top(state)..bottom).contains(&m.row)
        && m.column >= explorer_width(state)
}

/// Ligne de la bordure haute.
fn panel_top(state: &State) -> u16 {
    state
        .height
        .saturating_sub(1)
        .saturating_sub(terminal_height(state))
}

pub(crate) fn mouse(state: &mut State, m: MouseInput) {
    let on_border = m.row == panel_top(state);
    let column = m.column.saturating_sub(explorer_width(state));
    let t = &mut state.terminal;
    match m.kind {
        MouseKind::ScrollUp => t.scroll = t.scroll.saturating_add(WHEEL_LINES),
        MouseKind::ScrollDown => t.scroll = t.scroll.saturating_sub(WHEEL_LINES),
        _ => {
            if let Some(i) = terminal_tab_at(t, column).filter(|_| on_border) {
                t.select(i);
            }
            state.focus = Focus::Terminal;
            state.dragging = false;
        }
    }
}

/// Le shell de l'onglet `id` s'est terminé : l'onglet part, l'actif devient son voisin. Le dernier
/// parti, panneau fermé et vidé ; `next_id` reste : un identifiant ne resert jamais.
pub(crate) fn exited(state: &mut State, id: u32, error: Option<String>) {
    let t = &mut state.terminal;
    let Some(i) = t.tabs.iter().position(|&tab| tab == id) else {
        return;
    };
    t.tabs.remove(i);
    if t.tabs.is_empty() {
        *t = Terminal {
            next_id: t.next_id,
            ..Terminal::default()
        };
        if state.focus == Focus::Terminal {
            state.focus = Focus::Editor;
        }
    } else if i < t.active {
        t.active -= 1;
    } else if i == t.active {
        t.active = i.min(t.tabs.len() - 1);
        t.scroll = 0;
    }
    if let Some(error) = error {
        state.status = format!("Terminal : {error}");
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Command, Event, Focus, Key, KeyInput, Modifiers, MouseInput, MouseKind, PROBLEMS_HEIGHT,
        State, TERMINAL_HEIGHT, TERMINAL_TITLE, TermSpan, editor_size, terminal_height,
        terminal_size, terminal_tab_at, terminal_tab_label, update,
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

    fn nouvel_onglet() -> Event {
        Event::Key(touche(Key::Char('n'), false, true))
    }

    fn sorti(id: u32) -> Event {
        Event::TerminalExited { id, error: None }
    }

    /// Terminal ouvert avec le focus, `n` onglets d'identifiants 0..n, le dernier actif.
    fn onglets(n: usize) -> State {
        let mut s = etat();
        update(&mut s, bascule());
        for _ in 1..n {
            update(&mut s, nouvel_onglet());
        }
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
                vec![Command::TerminalKey(0, k)],
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
            vec![Command::TerminalKey(
                0,
                touche(Key::Char('{'), false, false)
            )]
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
            vec![Command::TerminalKey(0, f5)]
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
        update(&mut s, bascule());
        let cmds = palette(&mut s, "sauvegarder");
        assert!(
            matches!(cmds.as_slice(), [Command::RequestFormat { save: true, .. }]),
            "{cmds:?}"
        );
        assert_eq!(s.focus, Focus::Terminal);
        // Le rejeu ne vaut que pour une touche : la suivante repart au shell.
        let a = touche(Key::Char('a'), false, false);
        assert_eq!(
            update(&mut s, Event::Key(a)),
            vec![Command::TerminalKey(0, a)]
        );
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
            vec![Command::TerminalPaste(0, "ls\n".into())]
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
        assert!(!matches!(cmds.as_slice(), [Command::TerminalPaste(..)]));
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
        assert!(!matches!(cmds.as_slice(), [Command::TerminalKey(..)]));
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
        assert!(update(&mut s, sorti(0)).is_empty());
        assert!(!s.terminal.open && s.terminal.lines.is_empty());
        assert_eq!((s.terminal.cursor, s.terminal.scroll), (None, 0));
        assert_eq!(s.focus, Focus::Editor);
        assert_eq!(s.doc.version(), v);
        assert!(s.status.is_empty());
    }

    #[test]
    fn fin_du_shell_avec_erreur_et_focus_ailleurs() {
        let mut s = etat();
        update(&mut s, bascule());
        s.focus = Focus::Explorer;
        let error = Some("shell introuvable".into());
        assert!(update(&mut s, Event::TerminalExited { id: 0, error }).is_empty());
        assert_eq!(s.status, "Terminal : shell introuvable");
        assert_eq!(s.focus, Focus::Explorer);
    }

    #[test]
    fn ouvrir_cree_le_premier_onglet_et_fermer_les_garde() {
        let mut s = etat();
        update(&mut s, bascule());
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[0][..], 0)
        );
        update(&mut s, nouvel_onglet());
        update(&mut s, bascule());
        assert!(!s.terminal.open);
        update(&mut s, bascule());
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[0, 1][..], 1)
        );
    }

    #[test]
    fn alt_n_cree_et_bascule_sans_partir_au_shell() {
        let mut s = onglets(2);
        s.terminal.scroll = 7;
        assert!(update(&mut s, nouvel_onglet()).is_empty());
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[0, 1, 2][..], 2)
        );
        assert_eq!(s.terminal.scroll, 0);
        let a = touche(Key::Char('a'), false, false);
        assert_eq!(
            update(&mut s, Event::Key(a)),
            vec![Command::TerminalKey(2, a)]
        );
        // Verrouillage majuscule : le terminal annonce Alt+Shift+N.
        let mut k = touche(Key::Char('N'), false, true);
        k.mods.shift = true;
        assert!(update(&mut s, Event::Key(k)).is_empty());
        assert_eq!(s.terminal.tabs.len(), 4);
    }

    #[test]
    fn navigation_en_boucle() {
        let mut s = onglets(3);
        let (suivant, precedent) = (
            Event::Key(touche(Key::PageDown, false, true)),
            Event::Key(touche(Key::PageUp, false, true)),
        );
        for (touche, active) in [
            (&suivant, 0),
            (&suivant, 1),
            (&precedent, 0),
            (&precedent, 2),
        ] {
            s.terminal.scroll = 7;
            assert!(update(&mut s, touche.clone()).is_empty());
            assert_eq!((s.terminal.active, s.terminal.scroll), (active, 0));
        }
        // Un seul onglet : rien ne change, la vue reste.
        let mut s = onglets(1);
        s.terminal.scroll = 7;
        update(&mut s, suivant);
        assert_eq!((s.terminal.active, s.terminal.scroll), (0, 7));
    }

    #[test]
    fn alt_shift_page_et_ctrl_alt_restent_au_shell() {
        let mut s = onglets(2);
        for mut k in [
            touche(Key::PageDown, false, true),
            touche(Key::PageUp, false, true),
        ] {
            k.mods.shift = true;
            assert_eq!(
                update(&mut s, Event::Key(k)),
                vec![Command::TerminalKey(1, k)]
            );
        }
        let k = touche(Key::Char('n'), true, true);
        assert_eq!(
            update(&mut s, Event::Key(k)),
            vec![Command::TerminalKey(1, k)]
        );
        assert_eq!((s.terminal.tabs.len(), s.terminal.active), (2, 1));
    }

    #[test]
    fn touches_d_onglet_laissees_a_l_editeur_hors_focus() {
        let mut s = onglets(2);
        s.focus = Focus::Editor;
        for k in [
            touche(Key::Char('n'), false, true),
            touche(Key::PageUp, false, true),
            touche(Key::PageDown, false, true),
        ] {
            assert!(update(&mut s, Event::Key(k)).is_empty());
            assert_eq!(s.status, k.to_string());
            assert_eq!((s.terminal.tabs.len(), s.terminal.active), (2, 1));
        }
        // Copie de secours proposée : Alt+N reste son « ignorer ».
        s.backup_offer = Some(("/p/a.rs".into(), String::new()));
        let cmds = update(&mut s, Event::Key(touche(Key::Char('n'), false, true)));
        assert!(matches!(cmds.as_slice(), [Command::DiscardBackup(_)]));
        assert_eq!(s.terminal.tabs.len(), 2);
    }

    #[test]
    fn aide_et_palette_gardent_les_touches_d_onglet() {
        let mut s = onglets(2);
        s.help = Some(0);
        update(&mut s, nouvel_onglet());
        s.help = None;
        s.focus = Focus::Editor;
        update(&mut s, Event::Key(touche(Key::Char('P'), true, false)));
        s.focus = Focus::Terminal;
        update(&mut s, nouvel_onglet());
        assert!(s.palette.is_some());
        assert_eq!(s.terminal.tabs.len(), 2);
    }

    #[test]
    fn keys_ne_retire_pas_les_touches_d_onglet() {
        for commande in ["save", "toggle_terminal"] {
            let mut s = onglets(2);
            let alt_n = touche(Key::Char('n'), false, true);
            let alt_pgup = touche(Key::PageUp, false, true);
            s.settings.keys = vec![(alt_n, commande.into()), (alt_pgup, commande.into())];
            assert!(update(&mut s, Event::Key(alt_n)).is_empty());
            assert_eq!((s.terminal.tabs.len(), s.terminal.active), (3, 2));
            assert!(update(&mut s, Event::Key(alt_pgup)).is_empty());
            assert_eq!(s.terminal.active, 1);
            assert_eq!((s.terminal.open, s.focus), (true, Focus::Terminal));
        }
    }

    #[test]
    fn exit_de_l_onglet_actif_active_le_voisin() {
        let mut s = onglets(3);
        s.terminal.active = 1;
        s.terminal.scroll = 7;
        update(&mut s, sorti(1));
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[0, 2][..], 1)
        );
        assert_eq!((s.terminal.scroll, s.focus), (0, Focus::Terminal));
        // Le plus à droite : le voisin est à gauche.
        update(&mut s, sorti(2));
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[0][..], 0)
        );
        assert!(s.terminal.open);
    }

    #[test]
    fn exit_en_arriere_plan_garde_l_actif() {
        let mut s = onglets(3);
        s.terminal.scroll = 7;
        s.scroll_line = 20;
        s.focus = Focus::Editor;
        update(&mut s, sorti(0));
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[1, 2][..], 1)
        );
        assert_eq!(s.terminal.active_id(), Some(2));
        assert_eq!(s.terminal.scroll, 7);
        // Après l'actif : sa position ne bouge pas. Un inconnu : rien.
        s.terminal.active = 0;
        update(&mut s, sorti(2));
        update(&mut s, sorti(9));
        assert_eq!(
            (s.terminal.tabs.as_slice(), s.terminal.active),
            (&[1][..], 0)
        );
        assert!(s.terminal.open);
        assert_eq!(s.focus, Focus::Editor);
        // La vue déplacée à la molette ne saute pas pour un onglet qu'on ne regarde pas.
        assert_eq!(s.scroll_line, 20);
    }

    #[test]
    fn exit_du_dernier_ferme_et_un_id_ne_resert_pas() {
        let mut s = onglets(2);
        update(&mut s, sorti(1));
        assert!(s.terminal.open);
        update(&mut s, sorti(0));
        assert!(!s.terminal.open && s.terminal.tabs.is_empty());
        assert_eq!(s.focus, Focus::Editor);
        update(&mut s, bascule());
        assert_eq!(s.terminal.tabs, [2]);
    }

    #[test]
    fn panneau_ferme_exit_d_un_onglet_garde_les_autres() {
        let mut s = onglets(2);
        update(&mut s, bascule());
        update(&mut s, sorti(1));
        assert_eq!(
            (s.terminal.open, s.terminal.tabs.as_slice()),
            (false, &[0][..])
        );
        update(&mut s, bascule());
        assert_eq!(s.terminal.active_id(), Some(0));
    }

    #[test]
    fn echec_d_un_onglet_ferme_lui_seul() {
        let mut s = onglets(2);
        let error = Some("shell introuvable".into());
        update(&mut s, Event::TerminalExited { id: 1, error });
        assert_eq!(
            (s.terminal.open, s.terminal.tabs.as_slice()),
            (true, &[0][..])
        );
        assert_eq!(s.focus, Focus::Terminal);
        assert_eq!(s.status, "Terminal : shell introuvable");
    }

    #[test]
    fn colonne_vers_onglet() {
        let s = onglets(11);
        let debut = u16::try_from(TERMINAL_TITLE.len()).unwrap_or_default();
        assert_eq!(terminal_tab_at(&s.terminal, 0), None);
        assert_eq!(terminal_tab_at(&s.terminal, debut - 1), None);
        assert_eq!(terminal_tab_at(&s.terminal, debut), Some(0));
        assert_eq!(terminal_tab_at(&s.terminal, debut + 2), Some(0));
        assert_eq!(terminal_tab_at(&s.terminal, debut + 3), Some(1));
        // 9 libellés de 3 colonnes, puis ceux de 4 : le format, pas une largeur fixe.
        assert_eq!(terminal_tab_label(9), " 10 ");
        assert_eq!(terminal_tab_at(&s.terminal, debut + 27 + 3), Some(9));
        assert_eq!(terminal_tab_at(&s.terminal, debut + 27 + 4), Some(10));
        assert_eq!(terminal_tab_at(&s.terminal, debut + 27 + 8), None);
    }

    #[test]
    fn clic_sur_un_onglet() {
        let mut s = onglets(3);
        s.explorer.visible = true;
        s.explorer.width = 30;
        s.focus = Focus::Editor;
        s.terminal.scroll = 7;
        let haut = 40 - 1 - terminal_height(&s);
        let col = 30 + u16::try_from(TERMINAL_TITLE.len()).unwrap_or_default();
        update(&mut s, souris(MouseKind::Down, col + 4, haut));
        assert_eq!((s.terminal.active, s.terminal.scroll), (1, 0));
        assert_eq!(s.focus, Focus::Terminal);
        // Le titre et la grille donnent le focus sans changer d'onglet.
        s.focus = Focus::Editor;
        update(&mut s, souris(MouseKind::Down, 30, haut));
        assert_eq!((s.terminal.active, s.focus), (1, Focus::Terminal));
        update(&mut s, souris(MouseKind::Down, col, haut + 1));
        assert_eq!(s.terminal.active, 1);
        // Pas de changement d'onglet : la vue reste.
        s.terminal.scroll = 7;
        update(&mut s, souris(MouseKind::Down, col + 4, haut));
        assert_eq!(s.terminal.scroll, 7);
        // La molette sur un onglet fait défiler sans l'activer.
        update(&mut s, souris(MouseKind::ScrollUp, col, haut));
        assert_eq!(s.terminal.active, 1);
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
