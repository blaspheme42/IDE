//! Écran d'aide des raccourcis (F1) : fenêtre centrée, groupes de `SHORTCUTS`, défilée par `state.help`.

use ide_core::{SHORTCUTS, State, help_len, help_rows};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::theme::Theme;

/// Colonne des touches (la plus longue, « Ctrl+Page suiv. / Ctrl+Page préc. », fait 33 colonnes).
const KEYS: usize = 34;

pub(crate) fn render(state: &State, frame: &mut Frame, theme: &Theme) {
    let Some(top) = state.help else { return };
    let screen = frame.area();
    let w = 90.min(screen.width);
    let h = (help_rows(state).min(help_len()) as u16 + 2).min(screen.height);
    let area = Rect {
        x: screen.x + (screen.width - w) / 2,
        y: screen.y + (screen.height - h) / 2,
        width: w,
        height: h,
    };
    let block = Block::bordered()
        .title(" Raccourcis clavier ")
        .title_bottom(" Échap / F1 fermer · ↑↓ défiler ")
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().fg(theme.foreground).bg(theme.background));
    frame.render_widget(Clear, area);
    frame.render_widget(&block, area);
    // Lignes construites comme les compte `help_len` : titre, raccourcis, ligne vide entre groupes.
    let title = theme.border_focus;
    let keys = Style::new()
        .fg(theme.foreground)
        .add_modifier(Modifier::BOLD);
    let mut lines = Vec::with_capacity(help_len());
    for (i, (group, items)) in SHORTCUTS.iter().enumerate() {
        if i > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(*group, title));
        for (k, action) in *items {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<KEYS$}"), keys),
                Span::raw(*action),
            ]));
        }
    }
    let scroll = u16::try_from(top).unwrap_or(u16::MAX);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), block.inner(area));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_with;
    use ratatui::{Terminal, backend::TestBackend};

    fn rendu(w: u16, h: u16, top: usize) -> TestBackend {
        let s = State {
            width: w,
            height: h,
            help: Some(top),
            ..State::default()
        };
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| render_with(&s, f, &Theme::dark())).unwrap();
        t.backend().clone()
    }

    #[test]
    fn ecran_d_aide() {
        insta::assert_snapshot!(rendu(100, 30, 0));
    }

    #[test]
    fn defile_et_terminal_minuscule() {
        let b = rendu(100, 30, 2);
        let l: String = (0..100).map(|x| b.buffer()[(x, 2)].symbol()).collect();
        assert!(l.contains("Ctrl+Shift+P"), "{l}");
        // Pas de panique sur un écran minuscule.
        rendu(3, 2, 0);
        rendu(1, 1, 0);
    }
}
