//! Crate ide-ui : widgets ratatui.

use ide_core::{EXPLORER_WIDTH, GUTTER_WIDTH, LineEnding, State};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

const TAB_WIDTH: usize = 4;
/// Styles sobres, lisibles en 16 couleurs : sélection inversée, ligne courante sur fond gris foncé.
const SELECTION: Style = Style::new().add_modifier(Modifier::REVERSED);
const CURRENT_LINE: Style = Style::new().bg(Color::DarkGray);

/// Dessine toute l'interface à partir de l'état. Pure : aucune I/O.
pub fn render(state: &State, frame: &mut Frame) {
    let [main, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [explorer, editor] =
        Layout::horizontal([Constraint::Length(EXPLORER_WIDTH), Constraint::Min(0)]).areas(main);
    let [gutter, text] =
        Layout::horizontal([Constraint::Length(GUTTER_WIDTH), Constraint::Min(0)]).areas(editor);

    frame.render_widget(Block::bordered().title(" EXPLORATEUR "), explorer);

    let doc = &state.doc;
    let rope = doc.rope();
    let (cur_line, cur_col) = doc.cursor_line_col();
    let selection = doc.selection();
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
        // Gouttière : numéro sur 4 colonnes (jusqu'à 9999), espace, colonne de diagnostics (vide avant le LSP).
        numbers.push(Line::from(vec![
            Span::styled(format!("{:>4}", i + 1), style),
            Span::raw(" "),
            Span::raw(" "),
        ]));
        // Sélection ramenée en index de char relatifs à la ligne (vide si hors ligne).
        let start = rope.line_to_char(i);
        let sel = selection.as_ref().map_or(0..0, |r| {
            r.start.saturating_sub(start)..r.end.saturating_sub(start)
        });
        lines.push(Line::from(visible_slice(
            rope.line(i).chars(),
            state.scroll_col,
            text.width as usize,
            sel,
        )));
    }
    frame.render_widget(Paragraph::new(numbers), gutter);
    frame.render_widget(Paragraph::new(lines), text);
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

    if (state.scroll_line..state.scroll_line + text.height as usize).contains(&cur_line)
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

/// Partie d'une ligne entre les colonnes d'affichage `start` et `start + width`, en spans stylés.
/// Tabulations développées en espaces ; caractère large coupé à gauche → espaces.
/// `sel` : index de char de la ligne sélectionnés ; un saut de ligne sélectionné
/// marque une cellule après la fin de ligne, comme VSCode.
fn visible_slice(
    chars: impl Iterator<Item = char>,
    start: usize,
    width: usize,
    sel: Range<usize>,
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
        let style = if sel.contains(&k) {
            SELECTION
        } else {
            Style::new()
        };
        if c == '\n' {
            if style == SELECTION && col >= start {
                push(" ", style);
            }
            break;
        }
        let w = match c {
            '\t' => TAB_WIDTH - col % TAB_WIDTH,
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
            visible_slice(t.chars(), start, width, 0..0)
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
}
