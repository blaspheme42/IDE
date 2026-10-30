//! Crate ide-ui : widgets ratatui.

pub mod icons;

use ide_core::{EXPLORER_WIDTH, LineEnding, Search, State, gutter_width, search_bar_height};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use std::ops::Range;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Styles sobres, lisibles en 16 couleurs : sélection inversée, ligne courante sur fond gris foncé.
const SELECTION: Style = Style::new().add_modifier(Modifier::REVERSED);
const CURRENT_LINE: Style = Style::new().bg(Color::DarkGray);
/// Occurrences de la recherche : la courante sur fond jaune, les autres soulignées en jaune.
const MATCH: Style = Style::new()
    .fg(Color::Yellow)
    .add_modifier(Modifier::UNDERLINED);
const MATCH_CURRENT: Style = Style::new().fg(Color::Black).bg(Color::Yellow);
/// Indicateurs `[.*]` `[Aa]` de la barre de recherche.
const FLAG_ON: Style = Style::new().fg(Color::Black).bg(Color::Gray);
const FLAG_OFF: Style = Style::new().fg(Color::DarkGray);

/// Dessine toute l'interface à partir de l'état. Pure : aucune I/O.
pub fn render(state: &State, frame: &mut Frame) {
    let [main, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [explorer, editor] =
        Layout::horizontal([Constraint::Length(EXPLORER_WIDTH), Constraint::Min(0)]).areas(main);
    // Même hauteur que `editor_size` : la barre de recherche est prise sur la zone éditeur.
    let [editor, bar] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(search_bar_height(state)),
    ])
    .areas(editor);
    let [gutter, text] = Layout::horizontal([
        Constraint::Length(gutter_width(&state.doc)),
        Constraint::Min(0),
    ])
    .areas(editor);

    frame.render_widget(Block::bordered().title(" EXPLORATEUR "), explorer);

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
            Style::new().bold()
        } else {
            Style::new()
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
        let end = start + rope.line(i).len_chars();
        let first = matches.partition_point(|m| m.end <= start);
        let hits: Vec<_> = matches[first..]
            .iter()
            .take_while(|m| m.start < end)
            .map(|m| {
                let style = if Some(m) == current {
                    MATCH_CURRENT
                } else {
                    MATCH
                };
                (rel(m), style)
            })
            .collect();
        lines.push(Line::from(visible_slice(
            rope.line(i).chars(),
            state.scroll_col,
            text.width as usize,
            doc.tab_width(),
            |k| {
                if sel.contains(&k) {
                    return SELECTION;
                }
                hits.iter()
                    .find(|(r, _)| r.contains(&k))
                    .map_or(Style::new(), |(_, s)| *s)
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
            CURRENT_LINE,
        );
    }
    frame.render_widget(Paragraph::new(lines), text);

    if let Some(s) = &state.search {
        search_bar(s, bar, frame);
    } else if (state.scroll_line..state.scroll_line + text.height as usize).contains(&cur_line)
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
    frame.render_widget(Paragraph::new(left), status);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), status);
}

/// Barre de recherche : `Rechercher : <query>` + indicateurs et compteur à droite,
/// puis `Remplacer  : <replace>` en mode remplacer. Curseur du terminal à la fin du champ actif.
fn search_bar(s: &Search, area: Rect, frame: &mut Frame) {
    let flag = |on, t| Span::styled(t, if on { FLAG_ON } else { FLAG_OFF });
    let count = match (&s.error, s.current) {
        (Some(e), _) => Span::styled(e.clone(), Style::new().fg(Color::Red)),
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
        if i == active {
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
/// `style_of(k)` : style du char d'index `k` de la ligne ; un saut de ligne sélectionné
/// marque une cellule après la fin de ligne, comme VSCode.
fn visible_slice(
    chars: impl Iterator<Item = char>,
    start: usize,
    width: usize,
    tab: usize,
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
        let style = style_of(k);
        if c == '\n' {
            if style == SELECTION && col >= start {
                push(" ", style);
            }
            break;
        }
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
    use ratatui::{Terminal, backend::TestBackend};

    fn draw(state: &State) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| render(state, f)).unwrap();
        terminal.backend().clone()
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
        assert_eq!(backend.cursor_position(), Position::new(36, 0));
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
        assert_eq!(backend.cursor_position(), Position::new(36 + 9, 2));
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
    fn tranche_visible() {
        let s = |t: &str, start, width| -> String {
            visible_slice(t.chars(), start, width, 4, |_| Style::new())
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

    /// Zone texte : x = 30 (explorateur) + 6 (gouttière).
    const X: u16 = 36;

    fn selectionne(doc: Document, sel: Range<usize>, scroll_col: usize) -> TestBackend {
        let mut doc = doc;
        doc.set_cursor(sel.start, false);
        doc.set_cursor(sel.end, true);
        let mut s = state(doc);
        s.scroll_col = scroll_col;
        draw(&s)
    }

    /// Colonnes de la ligne `y` (relatives à la zone texte) en style inversé, parmi 0..n.
    fn inverses(b: &TestBackend, y: u16, n: u16) -> Vec<u16> {
        (0..n)
            .filter(|&x| b.buffer()[(X + x, y)].modifier.contains(Modifier::REVERSED))
            .collect()
    }

    #[test]
    fn selection_une_ligne() {
        let b = selectionne(Document::from_text(None, "abcdef"), 1..4, 0);
        assert_eq!(inverses(&b, 0, 10), [1, 2, 3]);
        // Pas de ligne courante surlignée pendant une sélection.
        assert_eq!(b.buffer()[(X + 8, 0)].bg, Color::Reset);
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
        assert_eq!(b.buffer()[(X + 1, 0)].symbol(), "x");
    }

    #[test]
    fn ligne_courante_surlignee() {
        let mut doc = Document::from_text(None, "a\nb");
        doc.move_down(1, false);
        let b = draw(&state(doc));
        let bg = |x, y| b.buffer()[(x, y)].bg;
        assert_eq!(bg(X, 1), Color::DarkGray);
        assert_eq!(bg(79, 1), Color::DarkGray);
        assert_eq!(bg(X - 1, 1), Color::Reset, "gouttière non surlignée");
        assert_eq!(bg(X, 0), Color::Reset);
        // Numéro de ligne courant en gras.
        assert!(b.buffer()[(33, 1)].modifier.contains(Modifier::BOLD));
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
        assert_eq!(b.buffer()[(X + 8, 0)].symbol(), "x");
        assert_eq!(b.cursor_position(), Position::new(X + 8, 0));
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
        assert_eq!(fg(at("[Aa]")), Color::Gray);
        assert_eq!(fg(at("[.*]")), Color::Reset);
        assert!(line.contains("5/120"));
        // La zone de texte s'arrête au-dessus : 22 lignes visibles.
        assert_eq!(b.buffer()[(0, 21)].symbol(), "│");
        assert_eq!(b.buffer()[(33, 21)].symbol(), "2");
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
        let cell = |x, y| b.buffer()[(X + x, y)].clone();
        assert_eq!(cell(8, 1).bg, Color::Yellow);
        assert_eq!(cell(10, 1).fg, Color::Black);
        assert_eq!(cell(11, 1).bg, Color::Reset);
        assert!(cell(0, 1).modifier.contains(Modifier::UNDERLINED));
        assert_eq!(cell(0, 1).fg, Color::Yellow);
        assert!(!cell(4, 1).modifier.contains(Modifier::UNDERLINED));
        // Sur la ligne courante (0), le fond gris reste sous les occurrences ordinaires.
        assert_eq!(cell(0, 0).bg, Color::DarkGray);
        assert!(cell(0, 0).modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn occurrence_courante_hors_ecran_et_selection_prioritaire() {
        // Courante en ligne 39, hors écran : rien en fond jaune dans la zone de texte.
        let mut s = recherche(None, 3 * 39);
        s.doc.set_cursor(8, false);
        s.doc.set_cursor(11, true);
        let b = draw(&s);
        for y in 0..22 {
            for x in 0..44 {
                assert_ne!(b.buffer()[(X + x, y)].bg, Color::Yellow, "({x}, {y})");
            }
        }
        // La sélection l'emporte sur le surlignage.
        assert!(b.buffer()[(X + 8, 0)].modifier.contains(Modifier::REVERSED));
        assert!(
            !b.buffer()[(X + 8, 0)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }
}
