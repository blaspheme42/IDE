//! Crate ide-ui : widgets ratatui.

mod explorer;
pub mod icons;
pub mod theme;

use ide_core::{
    LineEnding, Search, State, TAB_BAR_HEIGHT, explorer_width, gutter_width, search_bar_height,
    tab_label, tab_list, tab_scroll,
};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::ops::Range;
use theme::Theme;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Dessine toute l'interface avec le thème de la configuration (sombre si nom inconnu).
pub fn render(state: &State, frame: &mut Frame) {
    let theme = Theme::by_name(&state.settings.theme).unwrap_or_else(Theme::dark);
    render_with(state, frame, &theme);
}

/// Dessine toute l'interface à partir de l'état, avec `theme`. Pure : aucune I/O.
///
/// Fond et texte du thème posés sur tout l'écran, comme VSCode : coloration et ligne courante
/// sont choisies pour ce fond, celui du terminal (clair, transparent…) pourrait les rendre
/// illisibles. Contrepartie assumée : un terminal transparent devient opaque.
pub fn render_with(state: &State, frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().fg(theme.foreground).bg(theme.background));
    let [main, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [explorer, editor] = Layout::horizontal([
        Constraint::Length(explorer_width(state)),
        Constraint::Min(0),
    ])
    .areas(main);
    // Même hauteur que `editor_size` : barres d'onglets et de recherche prises sur la zone éditeur.
    let [tabs, editor, bar] = Layout::vertical([
        Constraint::Length(TAB_BAR_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(search_bar_height(state)),
    ])
    .areas(editor);
    let [gutter, text] = Layout::horizontal([
        Constraint::Length(gutter_width(&state.doc)),
        Constraint::Min(0),
    ])
    .areas(editor);

    if explorer.width > 0 {
        explorer::render(state, frame, explorer, theme);
    }
    tab_bar(state, tabs, frame, theme);

    let doc = &state.doc;
    let rope = doc.rope();
    let (cur_line, cur_col) = doc.cursor_line_col();
    let selection = doc.selection();
    let matches = state.search.as_ref().map_or(&[][..], |s| &s.matches[..]);
    let current = state
        .search
        .as_ref()
        .and_then(|s| s.current)
        .and_then(|i| matches.get(i));
    // NF-03 : seules les lignes visibles sont parcourues.
    let visible = state.scroll_line
        ..rope
            .len_lines()
            .min(state.scroll_line + text.height as usize);
    let mut numbers = Vec::new();
    let mut lines = Vec::new();
    for i in visible {
        let style = if i == cur_line {
            theme.gutter_current
        } else {
            Style::new().fg(theme.gutter)
        };
        // Gouttière : numéro aligné à droite, espace, colonne de diagnostics (vide avant le LSP).
        numbers.push(Line::from(vec![
            Span::styled(
                format!(
                    "{:>w$}",
                    i + 1,
                    w = usize::from(gutter.width).saturating_sub(2)
                ),
                style,
            ),
            Span::raw(" "),
            Span::raw(" "),
        ]));
        // Sélection ramenée en index de char relatifs à la ligne (vide si hors ligne).
        let start = rope.line_to_char(i);
        let rel = |r: &Range<usize>| r.start.saturating_sub(start)..r.end.saturating_sub(start);
        let sel = selection.as_ref().map_or(0..0, rel);
        // Occurrences touchant la ligne : triées, la première trouvée par recherche binaire.
        let line = rope.line(i);
        let end = start + line.len_chars();
        let first = matches.partition_point(|m| m.end <= start);
        let hits: Vec<_> = matches[first..]
            .iter()
            .take_while(|m| m.start < end)
            .map(|m| {
                let style = if Some(m) == current {
                    theme.search_current
                } else {
                    theme.search_match
                };
                (rel(m), style)
            })
            .collect();
        // Coloration touchant la ligne : octets ramenés en chars de la ligne, bornés à celle-ci
        // (une plage périmée ne doit pas faire paniquer la rope).
        let b0 = rope.line_to_byte(i);
        let b1 = b0 + line.len_bytes();
        let first = state.highlights.partition_point(|(r, _)| r.end <= b0);
        let colors: Vec<_> = state.highlights[first..]
            .iter()
            .take_while(|(r, _)| r.start < b1)
            .map(|(r, cat)| {
                let c = |b: usize| rope.byte_to_char(b.clamp(b0, b1)) - start;
                (c(r.start)..c(r.end), theme.syntax(cat))
            })
            .collect();
        let find = |v: &[(Range<usize>, Style)], k| {
            v.iter().find(|(r, _)| r.contains(&k)).map(|(_, s)| *s)
        };
        let eol = line
            .len_chars()
            .checked_sub(1)
            .filter(|k| sel.contains(k))
            .map(|_| theme.selection);
        lines.push(Line::from(visible_slice(
            line.chars(),
            state.scroll_col,
            text.width as usize,
            doc.tab_width(),
            eol,
            |k| {
                // Priorité : sélection > occurrence (courante ou non) > coloration seule.
                let base = find(&colors, k).unwrap_or_default();
                if sel.contains(&k) {
                    base.patch(theme.selection)
                } else {
                    find(&hits, k).map_or(base, |s| base.patch(s))
                }
            },
        )));
    }
    frame.render_widget(Paragraph::new(numbers), gutter);
    // Ligne courante posée avant le texte pour que le fond des occurrences reste visible.
    if selection.is_none()
        && (state.scroll_line..state.scroll_line + text.height as usize).contains(&cur_line)
    {
        let y = text.y + (cur_line - state.scroll_line) as u16;
        frame.buffer_mut().set_style(
            Rect {
                y,
                height: 1,
                ..text
            },
            Style::new().bg(theme.current_line),
        );
    }
    frame.render_widget(Paragraph::new(lines), text);

    // Focus explorateur : le curseur est celui de son invite, ou masqué.
    let editor_focus = state.focus == ide_core::Focus::Editor;
    if let Some(s) = &state.search {
        search_bar(s, bar, frame, editor_focus, theme);
    } else if editor_focus
        && (state.scroll_line..state.scroll_line + text.height as usize).contains(&cur_line)
        && (state.scroll_col..state.scroll_col + text.width as usize).contains(&cur_col)
    {
        frame.set_cursor_position(Position::new(
            text.x + (cur_col - state.scroll_col) as u16,
            text.y + (cur_line - state.scroll_line) as u16,
        ));
    }

    let dirty = if doc.is_dirty() { " ●" } else { "" };
    let left = format!(" {}{dirty} | {} ", doc.display_name(), state.status);
    let ending = match doc.line_ending {
        LineEnding::Lf => "LF",
        LineEnding::CrLf => "CRLF",
    };
    let right = format!(
        "Ln {}, Col {} | {ending} | UTF-8 ",
        cur_line + 1,
        cur_col + 1
    );
    frame.render_widget(Paragraph::new(left).style(theme.status_bar), status);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), status);
}

/// Barre d'onglets ` nom ● │ nom2 │ …`, défilée comme `tab_spans` pour garder l'actif visible.
fn tab_bar(state: &State, area: Rect, frame: &mut Frame, theme: &Theme) {
    let mut spans = Vec::new();
    for t in tab_list(state) {
        if !spans.is_empty() {
            spans.push(Span::styled("│", Style::new().fg(theme.border)));
        }
        let style = if t.active {
            theme.tab_active
        } else {
            theme.tab_inactive
        };
        spans.push(Span::styled(tab_label(&t), style));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).scroll((0, tab_scroll(state))),
        area,
    );
}

/// Barre de recherche : `Rechercher : <query>` + indicateurs et compteur à droite,
/// puis `Remplacer  : <replace>` en mode remplacer. Curseur du terminal à la fin du champ actif.
fn search_bar(s: &Search, area: Rect, frame: &mut Frame, cursor: bool, theme: &Theme) {
    // Indicateurs `[.*]` `[Aa]` : actif en vidéo inverse du thème, inactif estompé.
    let on = Style::new().fg(theme.background).bg(theme.foreground);
    let off = Style::new().fg(theme.muted);
    let flag = |is_on, t| Span::styled(t, if is_on { on } else { off });
    let count = match (&s.error, s.current) {
        (Some(e), _) => Span::styled(e.clone(), Style::new().fg(theme.error)),
        _ if s.query.is_empty() => Span::raw(""),
        (None, Some(i)) => Span::raw(format!("{}/{}", i + 1, s.matches.len())),
        (None, None) => Span::raw("Aucun résultat"),
    };
    let right = Line::from(vec![
        flag(s.regex, "[.*]"),
        Span::raw(" "),
        flag(s.case_sensitive, "[Aa]"),
        Span::raw(" "),
        count,
        Span::raw(" "),
    ]);
    let mut fields = vec![(
        "Rechercher : ",
        &s.query,
        area.width.saturating_sub(right.width() as u16),
    )];
    if let Some(r) = &s.replace {
        fields.push(("Remplacer  : ", r, area.width));
    }
    let active = usize::from(s.focus_replace && s.replace.is_some());
    for (i, (label, field, width)) in fields.into_iter().enumerate() {
        let row = Rect {
            y: area.y + i as u16,
            height: 1,
            width,
            ..area
        };
        // Une cellule gardée pour le curseur ; champ trop long → on en montre la fin.
        let shown = tail(field, (width as usize).saturating_sub(label.width() + 1));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(label, Style::new().bold()),
                Span::raw(shown),
            ])),
            row,
        );
        if cursor && i == active {
            let x = row.x + (label.width() + shown.width()) as u16;
            frame.set_cursor_position(Position::new(x.min(row.right().saturating_sub(1)), row.y));
        }
    }
    frame.render_widget(
        Paragraph::new(right).alignment(Alignment::Right),
        Rect { height: 1, ..area },
    );
}

/// Plus long suffixe de `s` tenant dans `max` colonnes.
fn tail(s: &str, max: usize) -> &str {
    let mut w = 0;
    for (i, c) in s.char_indices().rev() {
        w += c.width().unwrap_or(0);
        if w > max {
            return &s[i + c.len_utf8()..];
        }
    }
    s
}

/// Partie d'une ligne entre les colonnes d'affichage `start` et `start + width`, en spans stylés.
/// Tabulations développées en espaces (taquets tous les `tab`) ; caractère large coupé à gauche → espaces.
/// `style_of(k)` : style du char d'index `k` de la ligne ; `eol` : style d'une cellule marquée
/// après la fin de ligne (saut de ligne sélectionné, comme VSCode).
fn visible_slice(
    chars: impl Iterator<Item = char>,
    start: usize,
    width: usize,
    tab: usize,
    eol: Option<Style>,
    style_of: impl Fn(usize) -> Style,
) -> Vec<Span<'static>> {
    let end = start + width;
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut push = |s: &str, style: Style| match out.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push_str(s),
        _ => out.push(Span::styled(s.to_owned(), style)),
    };
    let mut col = 0;
    for (k, c) in chars.enumerate() {
        if col >= end {
            break;
        }
        if c == '\n' {
            if let Some(style) = eol.filter(|_| col >= start) {
                push(" ", style);
            }
            break;
        }
        let style = style_of(k);
        let w = match c {
            '\t' => tab - col % tab,
            _ => c.width().unwrap_or(0),
        };
        if c == '\t' || (col < start && col + w > start) {
            // Seule la partie visible de la cellule est rendue, en espaces.
            let from = col.max(start);
            push(&" ".repeat((col + w).min(end).saturating_sub(from)), style);
        } else if col >= start && col + w <= end && !c.is_control() {
            push(c.encode_utf8(&mut [0; 4]), style);
        }
        col += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Document;
    use ratatui::buffer::Cell;
    use ratatui::style::{Color, Modifier};
    use ratatui::{Terminal, backend::TestBackend};

    fn draw(state: &State) -> TestBackend {
        draw_with(state, &Theme::dark())
    }

    fn draw_with(state: &State, theme: &Theme) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| render_with(state, f, theme)).unwrap();
        terminal.backend().clone()
    }

    /// Fond de sélection du thème sombre.
    fn sel_bg() -> Color {
        Theme::dark().selection.bg.expect("fond de sélection")
    }

    /// La cellule porte-t-elle le style `s` (couleurs et modificateurs qu'il définit) ?
    fn a_style(c: &Cell, s: Style) -> bool {
        s.fg.is_none_or(|f| f == c.fg)
            && s.bg.is_none_or(|b| b == c.bg)
            && c.modifier.contains(s.add_modifier)
    }

    fn state(doc: Document) -> State {
        State {
            width: 80,
            height: 24,
            status: "Prêt".into(),
            doc,
            ..State::default()
        }
    }

    #[test]
    fn document_vide() {
        let backend = draw(&state(Document::from_text(None, "")));
        insta::assert_snapshot!(backend);
        assert_eq!(backend.cursor_position(), Position::new(36, 1));
    }

    #[test]
    fn accents_emoji_tabulation() {
        let mut doc = Document::from_text(
            Some("notes.txt".into()),
            "fn main() {\n\tlet é = \"café\";\n\t// 🦀 crabe 日本\n}\n",
        );
        doc.move_down(2, false);
        doc.move_right(false);
        doc.move_right(false);
        doc.move_right(false);
        doc.move_right(false);
        doc.move_right(false);
        let backend = draw(&state(doc));
        insta::assert_snapshot!(backend);
        // "\t// 🦀" : tab (4) + "// " (3) + 🦀 (2) = colonne 9.
        assert_eq!(backend.cursor_position(), Position::new(36 + 9, 3));
    }

    #[test]
    fn defilement() {
        let text: String = (1..=40)
            .map(|i| {
                format!(
                    "ligne {i:02} abcdefghijklmnopqrstuvwxyz0123456789 {}\n",
                    "x".repeat(i)
                )
            })
            .collect();
        let mut doc = Document::from_text(Some("long.txt".into()), &text);
        doc.move_down(7, false);
        let mut s = state(doc);
        s.scroll_line = 5;
        s.scroll_col = 3;
        let backend = draw(&s);
        insta::assert_snapshot!(backend);
        // Curseur en colonne 0 < scroll_col : masqué, reste à sa position par défaut.
        assert_eq!(backend.cursor_position(), Position::new(0, 0));
    }

    #[test]
    fn barre_d_onglets() {
        let mut s = state(Document::from_text(Some("b.rs".into()), "x"));
        s.tabs.push(ide_core::Tab {
            doc: Document::from_text(Some("a.rs".into()), ""),
            ..ide_core::Tab::default()
        });
        s.active = 1;
        s.doc.insert_char('y');
        let b = draw(&s);
        let line: String = (30..80).map(|x| b.buffer()[(x, 0)].symbol()).collect();
        assert_eq!(line.trim_end(), " a.rs │ b.rs ●");
        // Actif et inactifs aux styles du thème, séparateur couleur de bordure.
        let t = Theme::dark();
        let cell = |x: u16| &b.buffer()[(x, 0)];
        assert!(a_style(cell(38), t.tab_active));
        assert!(a_style(cell(31), t.tab_inactive));
        assert!(!cell(31).modifier.contains(Modifier::BOLD));
        assert_eq!(cell(36).fg, t.border);
        // Trop étroit : la barre défile pour montrer l'actif en entier.
        s.width = 40;
        let mut t = Terminal::new(TestBackend::new(40, 24)).unwrap();
        t.draw(|f| render(&s, f)).unwrap();
        let line: String = (30..40)
            .map(|x| t.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert_eq!(line, " │ b.rs ● ");
    }

    #[test]
    fn tranche_visible() {
        let s = |t: &str, start, width| -> String {
            visible_slice(t.chars(), start, width, 4, None, |_| Style::new())
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        };
        assert_eq!(s("a\tb", 0, 10), "a   b");
        assert_eq!(s("日本", 1, 10), " 本");
        assert_eq!(s("日本", 0, 3), "日");
        assert_eq!(s("\tx", 2, 10), "  x");
        assert_eq!(s("abc\n", 1, 10), "bc");
    }

    /// Zone texte : x = 30 (explorateur) + 6 (gouttière), y = 1 (sous la barre d'onglets).
    const X: u16 = 36;
    const Y: u16 = TAB_BAR_HEIGHT;

    fn selectionne(doc: Document, sel: Range<usize>, scroll_col: usize) -> TestBackend {
        let mut doc = doc;
        doc.set_cursor(sel.start, false);
        doc.set_cursor(sel.end, true);
        let mut s = state(doc);
        s.scroll_col = scroll_col;
        draw(&s)
    }

    /// Colonnes de la ligne `y` (relatives à la zone texte) sur fond de sélection, parmi 0..n.
    fn inverses(b: &TestBackend, y: u16, n: u16) -> Vec<u16> {
        (0..n)
            .filter(|&x| b.buffer()[(X + x, Y + y)].bg == sel_bg())
            .collect()
    }

    #[test]
    fn selection_une_ligne() {
        let b = selectionne(Document::from_text(None, "abcdef"), 1..4, 0);
        assert_eq!(inverses(&b, 0, 10), [1, 2, 3]);
        // Pas de ligne courante surlignée pendant une sélection : fond du thème.
        assert_eq!(b.buffer()[(X + 8, Y)].bg, Theme::dark().background);
    }

    #[test]
    fn selection_plusieurs_lignes() {
        // b, \n, c, d, \n, e : le saut de ligne sélectionné marque une cellule après la fin.
        let b = selectionne(Document::from_text(None, "ab\ncd\nef"), 1..7, 0);
        assert_eq!(inverses(&b, 0, 10), [1, 2]);
        assert_eq!(inverses(&b, 1, 10), [0, 1, 2]);
        assert_eq!(inverses(&b, 2, 10), [0]);
    }

    #[test]
    fn selection_tab_emoji_defilement() {
        // Colonnes : tab 0..4, 🦀 4..6, x 6, 日 7..9, y 9.
        let doc = || Document::from_text(None, "\t🦀x日y");
        // scroll 2 : fin de tab en 0..2, 🦀 en 2 (cellule de tête), x en 4.
        let b = selectionne(doc(), 1..3, 2);
        assert_eq!(inverses(&b, 0, 10), [2, 4]);
        let b = selectionne(doc(), 0..1, 2);
        assert_eq!(inverses(&b, 0, 10), [0, 1]);
        // scroll 5 : 🦀 coupé à gauche → espace sélectionné en 0, x en 1 non sélectionné.
        let b = selectionne(doc(), 1..2, 5);
        assert_eq!(inverses(&b, 0, 10), [0]);
        assert_eq!(b.buffer()[(X + 1, Y)].symbol(), "x");
    }

    #[test]
    fn ligne_courante_surlignee() {
        let mut doc = Document::from_text(None, "a\nb");
        doc.move_down(1, false);
        let b = draw(&state(doc));
        let t = Theme::dark();
        let bg = |x, y| b.buffer()[(x, Y + y)].bg;
        assert_eq!(bg(X, 1), t.current_line);
        assert_eq!(bg(79, 1), t.current_line);
        assert_eq!(bg(X - 1, 1), t.background, "gouttière non surlignée");
        assert_eq!(bg(X, 0), t.background);
        assert_eq!(b.buffer()[(X, Y)].fg, t.foreground);
        // Numéros de ligne : courant au style dédié, les autres en couleur de gouttière.
        assert!(a_style(&b.buffer()[(33, Y + 1)], t.gutter_current));
        assert_eq!(b.buffer()[(33, Y)].fg, t.gutter);
    }

    #[test]
    fn tabulation_largeur_editorconfig() {
        let mut doc = Document::from_text(None, "\tx\ny");
        doc.indent = ide_core::Indent {
            use_tabs: true,
            width: 8,
            tab_width: 8,
        };
        doc.move_right(false);
        let b = draw(&state(doc));
        assert_eq!(b.buffer()[(X + 8, Y)].symbol(), "x");
        assert_eq!(b.cursor_position(), Position::new(X + 8, Y));
    }

    /// Document « foo bar foo baz foo » répété sur 40 lignes, occurrences de « foo » ; courante = `current`.
    fn recherche(replace: Option<&str>, current: usize) -> State {
        let text = "foo bar foo baz foo\n".repeat(40);
        let matches = (0..40)
            .flat_map(|l| [0, 8, 16].map(|c| l * 20 + c..l * 20 + c + 3))
            .collect();
        let mut s = state(Document::from_text(Some("a.txt".into()), &text));
        s.search = Some(Search {
            query: "foo".into(),
            replace: replace.map(Into::into),
            focus_replace: replace.is_some(),
            case_sensitive: true,
            matches,
            current: Some(current),
            ..Search::default()
        });
        s
    }

    #[test]
    fn barre_rechercher() {
        let s = recherche(None, 4);
        let b = draw(&s);
        insta::assert_snapshot!(b);
        // Barre sur la dernière ligne de la zone éditeur, curseur après « Rechercher : foo ».
        assert_eq!(b.cursor_position(), Position::new(30 + 13 + 3, 22));
        assert_eq!(b.buffer()[(30, 22)].symbol(), "R");
        // Indicateurs : casse active, regex inactive.
        let fg = |x| b.buffer()[(x, 22)].bg;
        let line: String = (30..80).map(|x| b.buffer()[(x, 22)].symbol()).collect();
        let at = |t: &str| 30 + line.find(t).map_or(0, |i| line[..i].chars().count()) as u16;
        let t = Theme::dark();
        assert_eq!(fg(at("[Aa]")), t.foreground);
        assert_eq!(fg(at("[.*]")), t.background);
        assert_eq!(b.buffer()[(at("[.*]"), 22)].fg, t.muted);
        // Barre d'état sur toute la largeur.
        assert!((0..80).all(|x| a_style(&b.buffer()[(x, 23)], t.status_bar)));
        assert!(line.contains("5/120"));
        // La zone de texte s'arrête au-dessus : 21 lignes visibles, sous la barre d'onglets.
        assert_eq!(b.buffer()[(0, 21)].symbol(), "│");
        assert_eq!(b.buffer()[(33, 21)].symbol(), "1");
    }

    #[test]
    fn barre_remplacer() {
        let mut s = recherche(Some("quux"), 0);
        if let Some(r) = s.search.as_mut() {
            r.error = Some("Regex invalide : x".into());
        }
        let b = draw(&s);
        insta::assert_snapshot!(b);
        assert_eq!(b.cursor_position(), Position::new(30 + 13 + 4, 22));
    }

    #[test]
    fn focus_explorateur_curseur_hors_de_la_barre() {
        let mut s = recherche(None, 0);
        s.focus = ide_core::Focus::Explorer;
        let b = draw(&s);
        // Ni la barre (ligne 22) ni le texte : rien n'est placé, le curseur reste à l'origine.
        assert_eq!(b.cursor_position(), Position::new(0, 0));
    }

    #[test]
    fn champ_trop_long_montre_la_fin() {
        let mut s = recherche(Some(""), 0);
        if let Some(r) = s.search.as_mut() {
            r.focus_replace = false;
            r.query = format!("{}日本", "x".repeat(60));
        }
        let b = draw(&s);
        let line: String = (30..80).map(|x| b.buffer()[(x, 21)].symbol()).collect();
        assert!(
            line.starts_with("Rechercher : xxx") && line.contains("x日"),
            "{line}"
        );
        let x = b.cursor_position().x;
        // Le curseur suit le dernier caractère affiché (日本 occupe 4 cellules).
        assert_eq!(b.buffer()[(x - 2, 21)].symbol(), "本");
    }

    #[test]
    fn occurrences_surlignees() {
        let s = recherche(None, 4); // ligne 1, colonne 8
        let b = draw(&s);
        let t = Theme::dark();
        let cell = |x, y| b.buffer()[(X + x, Y + y)].clone();
        assert!(a_style(&cell(8, 1), t.search_current));
        assert!(a_style(&cell(10, 1), t.search_current));
        assert_eq!(cell(11, 1).bg, t.background);
        assert!(a_style(&cell(0, 1), t.search_match));
        assert!(!cell(4, 1).modifier.contains(Modifier::UNDERLINED));
        // Ligne courante (0) : son fond reste hors des occurrences.
        assert!(a_style(&cell(0, 0), t.search_match));
        assert_eq!(cell(4, 0).bg, t.current_line);
    }

    #[test]
    fn occurrence_courante_hors_ecran_et_selection_prioritaire() {
        // Courante en ligne 39, hors écran : aucune cellule au style de la courante.
        let mut s = recherche(None, 3 * 39);
        s.doc.set_cursor(8, false);
        s.doc.set_cursor(11, true);
        let b = draw(&s);
        let courante = Theme::dark()
            .search_current
            .bg
            .expect("fond de la courante");
        for y in Y..22 {
            for x in 0..44 {
                assert_ne!(b.buffer()[(X + x, y)].bg, courante, "({x}, {y})");
            }
        }
        // La sélection l'emporte sur le surlignage.
        assert_eq!(b.buffer()[(X + 8, Y)].bg, sel_bg());
        assert!(
            !b.buffer()[(X + 8, Y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    /// État sur `text` avec la coloration `hl` (plages en octets).
    fn colore(text: &str, hl: &[(Range<usize>, &'static str)]) -> State {
        let mut s = state(Document::from_text(Some("a.rs".into()), text));
        s.highlights = hl.to_vec();
        s
    }

    /// Couleur de texte des cellules `xs` de la ligne `y` de la zone texte.
    fn fgs(b: &TestBackend, y: u16, xs: Range<u16>) -> Vec<Color> {
        xs.map(|x| b.buffer()[(X + x, Y + y)].fg).collect()
    }

    fn syn(cat: &str) -> Color {
        Theme::dark().syntax(cat).fg.expect("couleur de catégorie")
    }

    #[test]
    fn mot_cle_colore() {
        let s = colore("fn main() {}", &[(0..2, "keyword"), (3..7, "function")]);
        let b = draw(&s);
        let (k, f, t) = (syn("keyword"), syn("function"), Theme::dark().foreground);
        assert_eq!(fgs(&b, 0, 0..8), [k, k, t, f, f, f, f, t]);
        // La ligne courante garde son fond sous la coloration.
        assert_eq!(b.buffer()[(X, Y)].bg, Theme::dark().current_line);
    }

    #[test]
    fn coloration_accents_emoji() {
        // Octets : é 0..2, 🦀 2..6, "café" 6..13 (é sur 2 octets), espace 13, fn 14..16.
        // Colonnes : é 0, 🦀 1..3, "café" 3..9, espace 9, fn 10..12.
        let s = colore("é🦀\"café\" fn", &[(6..13, "string"), (14..16, "keyword")]);
        let b = draw(&s);
        let (st, k, t) = (syn("string"), syn("keyword"), Theme::dark().foreground);
        // Cellule de queue de 🦀 (2) remise à zéro par ratatui : seules les cellules de tête comptent.
        assert_eq!(fgs(&b, 0, 0..2), [t, t]);
        assert_eq!(fgs(&b, 0, 3..9), [st; 6]);
        assert_eq!(fgs(&b, 0, 9..12), [t, k, k]);
    }

    #[test]
    fn tabulation_avant_mot_cle() {
        let b = draw(&colore("\tfn x", &[(1..3, "keyword")]));
        let (k, t) = (syn("keyword"), Theme::dark().foreground);
        assert_eq!(fgs(&b, 0, 0..7), [t, t, t, t, k, k, t]);
        assert_eq!(b.buffer()[(X + 4, Y)].symbol(), "f");
    }

    #[test]
    fn defilement_coupe_un_mot_colore() {
        let mut s = colore("fn hello 日本", &[(3..8, "function"), (9..15, "string")]);
        s.scroll_col = 5;
        let b = draw(&s);
        let (f, st, t) = (syn("function"), syn("string"), Theme::dark().foreground);
        // « llo », espace, 日 coupé à gauche → espace coloré, 本.
        assert_eq!(fgs(&b, 0, 0..4), [f, f, f, t]);
        assert_eq!(b.buffer()[(X, Y)].symbol(), "l");
        s.scroll_col = 10;
        let b = draw(&s);
        assert_eq!(b.buffer()[(X, Y)].symbol(), " ");
        assert_eq!(fgs(&b, 0, 0..2), [st, st]);
    }

    #[test]
    fn selection_prioritaire_sur_coloration() {
        let mut s = colore("fn x", &[(0..2, "keyword")]);
        s.doc.set_cursor(0, false);
        s.doc.set_cursor(1, true);
        let b = draw(&s);
        let c = |x| b.buffer()[(X + x, Y)].clone();
        // Fond de sélection, couleur du mot-clé conservée dessous (comme VSCode).
        assert_eq!((c(0).bg, c(0).fg), (sel_bg(), syn("keyword")));
        assert_eq!(
            (c(1).bg, c(1).fg),
            (Theme::dark().background, syn("keyword"))
        );
    }

    #[test]
    fn plage_hors_ecran_ignoree() {
        // 40 lignes « fn x » (5 octets) ; défilement à la ligne 10, plage périmée au-delà du texte.
        let mut s = colore(
            &"fn x\n".repeat(40),
            &[
                (0..2, "keyword"),
                (50..52, "keyword"),
                (9_000..9_010, "string"),
            ],
        );
        s.scroll_line = 10;
        let b = draw(&s);
        let (k, t) = (syn("keyword"), Theme::dark().foreground);
        assert_eq!(fgs(&b, 0, 0..3), [k, k, t]);
        assert_eq!(fgs(&b, 1, 0..3), [t, t, t]);
    }

    #[test]
    fn theme_clair() {
        let t = Theme::light();
        let b = draw_with(&colore("fn x", &[(0..2, "keyword")]), &t);
        let c = |x| b.buffer()[(X + x, Y + 1)].clone();
        assert_eq!(
            b.buffer()[(X, Y)].fg,
            t.syntax("keyword").fg.expect("couleur du mot-clé")
        );
        assert_eq!(b.buffer()[(X + 3, Y)].fg, t.foreground);
        // Ligne vide sous le texte : fond et texte du thème clair.
        assert_eq!((c(0).fg, c(0).bg), (t.foreground, t.background));
    }

    fn draw_settings(theme: &str) -> TestBackend {
        let mut s = State::default();
        s.settings.theme = theme.into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| render(&s, f)).unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn render_suit_le_theme_configure() {
        let (l, d) = (Theme::light(), Theme::dark());
        let c = draw_settings("light").buffer()[(X, Y + 1)].clone();
        assert_eq!((c.fg, c.bg), (l.foreground, l.background));
        // Nom inconnu : repli sur le sombre.
        let c = draw_settings("solarized").buffer()[(X, Y + 1)].clone();
        assert_eq!((c.fg, c.bg), (d.foreground, d.background));
    }
}
