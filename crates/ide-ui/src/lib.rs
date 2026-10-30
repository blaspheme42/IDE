//! Crate ide-ui : widgets ratatui.

use ide_core::{EXPLORER_WIDTH, GUTTER_WIDTH, LineEnding, State};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthChar;

const TAB_WIDTH: usize = 4;

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
        numbers.push(Line::from(vec![
            Span::styled(format!("{:>4}", i + 1), style),
            Span::raw("  "),
        ]));
        lines.push(Line::raw(visible_slice(
            rope.line(i).chars(),
            state.scroll_col,
            text.width as usize,
        )));
    }
    frame.render_widget(Paragraph::new(numbers), gutter);
    frame.render_widget(Paragraph::new(lines), text);

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

/// Partie d'une ligne entre les colonnes d'affichage `start` et `start + width`.
/// Tabulations développées en espaces ; caractère large coupé à gauche → espaces.
fn visible_slice(chars: impl Iterator<Item = char>, start: usize, width: usize) -> String {
    let end = start + width;
    let mut out = String::new();
    let mut col = 0;
    for c in chars {
        if c == '\n' || col >= end {
            break;
        }
        let w = match c {
            '\t' => TAB_WIDTH - col % TAB_WIDTH,
            _ => c.width().unwrap_or(0),
        };
        if c == '\t' || (col < start && col + w > start) {
            // Seule la partie visible de la cellule est rendue, en espaces.
            let from = col.max(start);
            out.extend(std::iter::repeat_n(
                ' ',
                (col + w).min(end).saturating_sub(from),
            ));
        } else if col >= start && col + w <= end && !c.is_control() {
            out.push(c);
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
        let s = |t: &str, start, width| visible_slice(t.chars(), start, width);
        assert_eq!(s("a\tb", 0, 10), "a   b");
        assert_eq!(s("日本", 1, 10), " 本");
        assert_eq!(s("日本", 0, 3), "日");
        assert_eq!(s("\tx", 2, 10), "  x");
        assert_eq!(s("abc\n", 1, 10), "bc");
    }
}
