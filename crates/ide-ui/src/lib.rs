//! Crate ide-ui : widgets ratatui.

use ide_core::State;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::widgets::{Block, Paragraph};

/// Dessine toute l'interface à partir de l'état. Pure : aucune I/O.
pub fn render(state: &State, frame: &mut Frame) {
    let [main, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [explorer, editor] =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(main);

    frame.render_widget(Block::bordered().title(" EXPLORATEUR "), explorer);

    let [_, welcome, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(editor);
    frame.render_widget(
        Paragraph::new("Ctrl+Q pour quitter").alignment(Alignment::Center),
        welcome,
    );

    let text = format!(" {} | {}x{}", state.status, state.width, state.height);
    frame.render_widget(Paragraph::new(text), status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn disposition_de_base_80x24() {
        let state = State {
            width: 80,
            height: 24,
            status: "Prêt".into(),
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| render(&state, f)).unwrap();
        insta::assert_snapshot!(terminal.backend());
    }
}
