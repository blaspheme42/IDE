//! Rendu LSP (J4) : soulignement des diagnostics, popup de complétion, infobulle, panneau Problèmes.

use ide_core::{Severity, State};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::ops::Range;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;

pub(crate) use ide_core::{PROBLEMS_HEIGHT, diagnostic_counts};
use ide_core::{problems, visible_completions};

/// Symbole d'une sévérité, ASCII sans Nerd Font.
pub(crate) fn icon(s: Severity, nerd: bool) -> &'static str {
    match (s, nerd) {
        (Severity::Error, true) => "✖",
        (Severity::Warning, true) => "⚠",
        (_, true) => "●",
        (Severity::Error, false) => "E",
        (Severity::Warning, false) => "W",
        (_, false) => "i",
    }
}

pub(crate) fn color(s: Severity, theme: &Theme) -> Color {
    match s {
        Severity::Error => theme.error,
        Severity::Warning => theme.warning,
        _ => theme.muted,
    }
}

/// Diagnostics du document actif touchant les lignes `lines`, en index de `char` absolus,
/// triés du plus grave au moins grave. Plage vide élargie à un caractère pour rester visible.
pub(crate) fn visible_diagnostics(
    state: &State,
    lines: &Range<usize>,
) -> Vec<(Range<usize>, Severity)> {
    let Some(ds) = state
        .doc
        .path
        .as_ref()
        .and_then(|p| state.diagnostics.get(p))
    else {
        return vec![];
    };
    let mut v: Vec<_> = ds
        .iter()
        .filter(|d| d.end.line >= lines.start && d.start.line < lines.end)
        .map(|d| {
            let s = state.doc.char_at_lsp(d.start);
            let e = state.doc.char_at_lsp(d.end).max(s + 1);
            (s..e, d.severity)
        })
        .collect();
    v.sort_by_key(|(_, s)| *s);
    v
}

/// Position écran (x, y) de l'index de `char` `pos` du document actif, si visible dans `text`.
fn screen_pos(state: &State, text: Rect, pos: usize) -> Option<(u16, u16)> {
    let rope = state.doc.rope();
    let line = rope.char_to_line(pos.min(rope.len_chars()));
    let col = state.doc.display_col(line, pos - rope.line_to_char(line));
    let dy = state.doc.rows_between(state.scroll_line, line)?;
    let dx = col.checked_sub(state.scroll_col)?;
    (dy < text.height as usize && dx < text.width as usize)
        .then(|| (text.x + dx as u16, text.y + dy as u16))
}

/// Cadre de `h` lignes intérieures sous la ligne `y` (au-dessus si `above` ou pas la place
/// dessous), dans `bounds` ; hauteur réduite si aucun côté ne suffit.
fn place(x: u16, y: u16, w: u16, h: u16, above: bool, bounds: Rect) -> Rect {
    let below_room = bounds.bottom().saturating_sub(y + 1);
    let above_room = y.saturating_sub(bounds.y);
    let fits = |room: u16| room >= h + 2;
    let up = if above {
        fits(above_room) || !fits(below_room) && above_room > below_room
    } else {
        !fits(below_room) && above_room > below_room
    };
    let h = (h + 2).min(if up { above_room } else { below_room });
    let w = w.min(bounds.width);
    Rect {
        x: x.min(bounds.right().saturating_sub(w)),
        y: if up { y - h } else { y + 1 },
        width: w,
        height: h,
    }
}

/// Popup de complétion sous le curseur, alignée sur le début du mot (F-41).
pub(crate) fn completion(state: &State, frame: &mut Frame, text: Rect, theme: &Theme) {
    let Some(c) = &state.completion else { return };
    let items = visible_completions(state);
    let (_, cy) = match screen_pos(state, text, state.doc.cursor()) {
        Some(p) if !items.is_empty() => p,
        _ => return,
    };
    let x = screen_pos(state, text, c.anchor).map_or(text.x, |(x, _)| x);
    let kind_w = items
        .iter()
        .map(|i| i.kind.as_deref().map_or(0, str::width))
        .max()
        .unwrap_or(0);
    let label_w = items.iter().map(|i| i.label.width()).max().unwrap_or(0);
    let w = (kind_w + 1 + label_w + 2).min(50) as u16;
    let mut bounds = frame.area();
    bounds.height = text.bottom().saturating_sub(bounds.y);
    let area = place(
        x.saturating_sub(1),
        cy,
        w,
        items.len().min(10) as u16,
        false,
        bounds,
    );
    let rows = area.height.saturating_sub(2) as usize;
    // Défilement minimal qui garde la sélection visible.
    let first = (c.selected + 1).saturating_sub(rows);
    let inner = area.width.saturating_sub(2) as usize;
    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .map(|(i, it)| {
            let kind = format!("{:<kind_w$} ", it.kind.as_deref().unwrap_or(""));
            let room = inner.saturating_sub(kind.width() + it.label.width());
            let detail = it
                .detail
                .as_deref()
                .filter(|_| room > 2)
                .map_or_else(String::new, |d| {
                    let d = cut(d, room - 1);
                    format!("{}{d}", " ".repeat(room - d.width()))
                });
            let line = Line::from(vec![
                Span::styled(kind, Style::new().fg(theme.muted)),
                Span::raw(it.label.clone()),
                Span::styled(detail, Style::new().fg(theme.muted)),
            ]);
            if i == c.selected {
                line.style(theme.explorer_selected)
            } else {
                line
            }
        })
        .collect();
    popup(frame, area, lines, theme);
}

/// Infobulle de survol au-dessus de la ligne du curseur (F-43).
pub(crate) fn hover(state: &State, frame: &mut Frame, text: Rect, theme: &Theme) {
    let Some(h) = &state.hover else { return };
    let Some((_, cy)) = screen_pos(state, text, state.doc.cursor()) else {
        return;
    };
    let x = screen_pos(state, text, h.at).map_or(text.x, |(x, _)| x);
    let max = (frame.area().width.saturating_sub(2) as usize).min(80);
    let lines: Vec<String> = h
        .text
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .flat_map(|l| wrap(l, max))
        .take(12)
        .collect();
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(0);
    if w == 0 {
        return;
    }
    let mut bounds = frame.area();
    bounds.height = text.bottom().saturating_sub(bounds.y);
    let area = place(
        x.saturating_sub(1),
        cy,
        w as u16 + 2,
        lines.len() as u16,
        true,
        bounds,
    );
    popup(
        frame,
        area,
        lines.into_iter().map(Line::from).collect(),
        theme,
    );
}

fn popup(frame: &mut Frame, area: Rect, lines: Vec<Line>, theme: &Theme) {
    let block = Block::bordered()
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().fg(theme.foreground).bg(theme.background));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Plus long préfixe de `s` tenant dans `max` colonnes.
fn cut(s: &str, max: usize) -> &str {
    let mut w = 0;
    for (i, c) in s.char_indices() {
        w += c.width().unwrap_or(0);
        if w > max {
            return &s[..i];
        }
    }
    s
}

/// `s` coupé en morceaux d'au plus `max` colonnes (ligne vide gardée).
fn wrap(s: &str, max: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > max && w > 0 {
            out.push(String::new());
            w = 0;
        }
        if let Some(l) = out.last_mut() {
            l.push(c);
        }
        w += cw;
    }
    out
}

/// Panneau « PROBLÈMES (n) » : `✖ fichier:ligne:col  message`, chemins relatifs à la racine.
pub(crate) fn problems_panel(state: &State, frame: &mut Frame, area: Rect, theme: &Theme) {
    let all = problems(state);
    let nerd = state.settings.nerd_icons;
    let root = state.explorer.root.as_deref();
    let mut lines: Vec<Line> = all
        .iter()
        .take(area.height.saturating_sub(1) as usize)
        .map(|(p, d)| {
            let p = root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p);
            Line::from(vec![
                Span::styled(
                    format!(" {} ", icon(d.severity, nerd)),
                    Style::new().fg(color(d.severity, theme)),
                ),
                Span::raw(format!(
                    "{}:{}:{}  ",
                    p.display(),
                    d.start.line + 1,
                    d.start.character + 1
                )),
                Span::raw(d.message.lines().next().unwrap_or("").to_owned()),
            ])
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::styled(
            " Aucun problème",
            Style::new().fg(theme.muted),
        ));
    }
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(theme.border))
        .title(Span::styled(
            format!(" PROBLÈMES ({}) ", all.len()),
            Style::new().add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
