//! Panneau de l'explorateur : arbre, icônes, sélection, invite.

use ide_core::State;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Block;

/// Dessine l'explorateur dans `area` (bordures comprises).
pub(crate) fn render(_state: &State, frame: &mut Frame, area: Rect) {
    frame.render_widget(Block::bordered().title(" EXPLORATEUR "), area);
}
