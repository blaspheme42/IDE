//! Rendu du terminal intégré (F-60) : la grille remplie par l'app, sous une bordure haute.

use ide_core::{Focus, State, TermColor, TermSpan};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::theme::Theme;

/// `None` pour `Default` : la cellule garde la couleur du thème, posée sur tout l'écran.
fn color(c: TermColor) -> Option<Color> {
    match c {
        TermColor::Default => None,
        TermColor::Idx(n) => Some(Color::Indexed(n)),
        TermColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

fn style(s: &TermSpan) -> Style {
    let mut add_modifier = Modifier::empty();
    add_modifier.set(Modifier::BOLD, s.bold);
    add_modifier.set(Modifier::ITALIC, s.italic);
    add_modifier.set(Modifier::UNDERLINED, s.underline);
    add_modifier.set(Modifier::REVERSED, s.inverse);
    Style {
        fg: color(s.fg),
        bg: color(s.bg),
        add_modifier,
        ..Style::new()
    }
}

/// Panneau « TERMINAL » ; le curseur est celui du shell, tant que rien ne recouvre le panneau.
pub(crate) fn panel(state: &State, frame: &mut Frame, area: Rect, theme: &Theme) {
    let focus = state.focus == Focus::Terminal;
    let lines: Vec<Line> = state
        .terminal
        .lines
        .iter()
        .map(|l| {
            l.iter()
                .map(|s| Span::styled(s.text.as_str(), style(s)))
                .collect()
        })
        .collect();
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(if focus {
            theme.border_focus
        } else {
            Style::new().fg(theme.border)
        })
        .title(Span::styled(
            " TERMINAL ",
            Style::new().add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(Paragraph::new(lines).block(block), area);
    if let Some((row, col)) = state.terminal.cursor
        && focus
        && state.help.is_none()
        && state.palette.is_none()
    {
        // La grille peut retarder d'un rendu sur la taille du panneau.
        let at = Position::new(
            area.x.saturating_add(col),
            area.y.saturating_add(row).saturating_add(1),
        );
        if area.contains(at) {
            frame.set_cursor_position(at);
        }
    }
}
