//! Rendu des palettes (F-50 Ctrl+P, F-51 Ctrl+Shift+P) : en haut de l'écran, sous la barre
//! d'onglets, centrée comme VSCode. Dessinée en dernier : elle recouvre tout et prend le curseur.

use ide_core::{PaletteItem, PaletteKind, State, TAB_BAR_HEIGHT, palette_items};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;

/// Éléments visibles au plus.
const ROWS: usize = 12;

pub(crate) fn render(state: &State, frame: &mut Frame, theme: &Theme) {
    draw(state, frame, theme, &palette_items(state));
}

/// Palette ouverte de `state` avec la liste `items` (déjà filtrée).
fn draw(state: &State, frame: &mut Frame, theme: &Theme, items: &[PaletteItem]) {
    let Some(p) = &state.palette else { return };
    let screen = frame.area();
    let w = 80.min(screen.width * 3 / 5).min(screen.width);
    let rows = items.len().clamp(1, ROWS) as u16;
    let y = screen.y + TAB_BAR_HEIGHT.min(screen.height);
    let area = Rect {
        x: screen.x + (screen.width - w) / 2,
        y,
        width: w,
        height: (rows + 3).min(screen.bottom() - y),
    };
    let title = match p.kind {
        PaletteKind::Files => " Ouvrir un fichier ",
        PaletteKind::Commands => " Commandes ",
    };
    let block = Block::bordered()
        .title(title)
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().fg(theme.foreground).bg(theme.background));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let width = inner.width as usize;

    // Saisie : une cellule gardée pour le curseur ; requête trop longue → on en montre la fin.
    let shown = crate::tail(&p.query, width.saturating_sub(3));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::new().fg(theme.muted)),
            Span::raw(shown),
        ])),
        Rect { height: 1, ..inner },
    );
    let x = inner.x + (2 + shown.width()) as u16;
    frame.set_cursor_position(Position::new(x.min(inner.right() - 1), inner.y));

    let list = Rect {
        y: inner.y + 1,
        height: inner.height - 1,
        ..inner
    };
    let muted = Style::new().fg(theme.muted);
    let lines: Vec<Line> = if p.kind == PaletteKind::Files && state.project_files.is_empty() {
        vec![Line::styled("Chargement…", muted)]
    } else if items.is_empty() {
        vec![Line::styled("Aucun résultat", muted)]
    } else {
        let shown_rows = list.height as usize;
        // Défilement minimal qui garde la sélection visible.
        let first = (p.selected + 1).saturating_sub(shown_rows);
        items
            .iter()
            .enumerate()
            .skip(first)
            .take(shown_rows)
            .map(|(i, it)| {
                // Détail calé à droite après au moins une espace, tronqué par « … ».
                let room = width.saturating_sub(it.label.width() + 1);
                let d = fit(&it.detail, room);
                let pad = width.saturating_sub(it.label.width() + d.width());
                let line = Line::from(vec![
                    Span::raw(it.label.clone()),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(d, muted),
                ]);
                if i == p.selected {
                    line.style(theme.explorer_selected)
                } else {
                    line
                }
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), list);
}

/// `s` s'il tient dans `max` colonnes, sinon son plus long préfixe suivi de « … » ; vide si `max` < 2.
fn fit(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_owned();
    }
    if max < 2 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        w += c.width().unwrap_or(0);
        if w > max - 1 {
            break;
        }
        out.push(c);
    }
    out + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_with;
    use ide_core::{Document, Palette, Search};
    use ratatui::{Terminal, backend::TestBackend};

    fn state(kind: PaletteKind, query: &str, selected: usize) -> State {
        State {
            width: 80,
            height: 24,
            status: "Prêt".into(),
            doc: Document::from_text(Some("a.rs".into()), "fn main() {}\n"),
            palette: Some(Palette {
                kind,
                query: query.into(),
                selected,
            }),
            project_files: vec!["src/main.rs".into()],
            ..State::default()
        }
    }

    /// Interface complète, puis la palette redessinée avec `items`.
    fn rendu(s: &State, items: &[PaletteItem]) -> TestBackend {
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        t.draw(|f| {
            render_with(s, f, &Theme::dark());
            draw(s, f, &Theme::dark(), items);
        })
        .unwrap();
        t.backend().clone()
    }

    fn item(label: &str, detail: &str) -> PaletteItem {
        PaletteItem {
            label: label.into(),
            detail: detail.into(),
        }
    }

    fn ligne(b: &TestBackend, y: u16) -> String {
        (0..80).map(|x| b.buffer()[(x, y)].symbol()).collect()
    }

    /// Palette de 48 colonnes (60 % de 80) en x = 16, cadre en y = 1.
    const X: u16 = 16;

    #[test]
    fn palette_fichiers() {
        let items = [
            item("main.rs", "src"),
            item("lib.rs", "crates/ide-ui/src"),
            item("Cargo.toml", ""),
        ];
        let b = rendu(&state(PaletteKind::Files, "rs", 1), &items);
        insta::assert_snapshot!(b);
        assert_eq!(b.cursor_position(), Position::new(X + 1 + 4, 2));
        let t = Theme::dark();
        assert!(ligne(&b, 4).contains(&format!("│lib.rs{}crates/ide-ui/src│", " ".repeat(23))));
        assert_eq!(
            b.buffer()[(X + 1, 4)].bg,
            t.explorer_selected.bg.expect("fond")
        );
        assert_ne!(b.buffer()[(X + 1, 3)].bg, b.buffer()[(X + 1, 4)].bg);
        assert_eq!(b.buffer()[(X + 46, 4)].fg, t.muted);
    }

    #[test]
    fn palette_commandes() {
        let items = [
            item("Sauvegarder", "Ctrl+S"),
            item("Fermer l'onglet", "Ctrl+W"),
        ];
        let b = rendu(&state(PaletteKind::Commands, "", 0), &items);
        assert!(ligne(&b, 1).contains("┌ Commandes ─"), "{}", ligne(&b, 1));
        assert!(ligne(&b, 2).contains("│> "));
        assert!(ligne(&b, 3).contains(&format!("│Sauvegarder{}Ctrl+S│", " ".repeat(29))));
        assert!(ligne(&b, 5).contains("└──"));
    }

    #[test]
    fn selection_defile() {
        let items: Vec<_> = (0..30).map(|i| item(&format!("f{i:02}"), "")).collect();
        let b = rendu(&state(PaletteKind::Files, "", 20), &items);
        // 12 lignes : f09..=f20, la sélection en dernier, puis le bord du bas.
        assert!(ligne(&b, 3).contains("│f09 "));
        assert!(ligne(&b, 14).contains("│f20 "));
        assert!(ligne(&b, 15).contains("└──"));
        let sel = Theme::dark().explorer_selected.bg.expect("fond");
        assert_eq!(b.buffer()[(X + 1, 14)].bg, sel);
        assert_ne!(b.buffer()[(X + 1, 13)].bg, sel);
    }

    #[test]
    fn aucun_resultat_et_chargement() {
        let mut s = state(PaletteKind::Files, "zzz", 0);
        assert!(ligne(&rendu(&s, &[]), 3).contains("│Aucun résultat "));
        s.project_files.clear();
        let b = rendu(&s, &[]);
        assert!(ligne(&b, 3).contains("│Chargement… "));
        assert!(ligne(&b, 1).contains("┌ Ouvrir un fichier ─"));
        // La palette commandes ne dépend pas de la liste des fichiers.
        s.palette = Some(Palette {
            kind: PaletteKind::Commands,
            query: String::new(),
            selected: 0,
        });
        assert!(ligne(&rendu(&s, &[]), 3).contains("│Aucun résultat "));
    }

    #[test]
    fn curseur_dans_la_palette_meme_recherche_ouverte() {
        let mut s = state(PaletteKind::Files, "main", 0);
        s.search = Some(Search {
            query: "fn".into(),
            ..Search::default()
        });
        // Rendu public : la palette passe en dernier et garde le curseur.
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        t.draw(|f| render_with(&s, f, &Theme::dark())).unwrap();
        assert_eq!(t.backend().cursor_position(), Position::new(X + 1 + 6, 2));
    }

    #[test]
    fn detail_tronque() {
        let long = "crates/ide-ui/src/très/profond/日本語/dossier";
        let b = rendu(
            &state(PaletteKind::Files, "", 0),
            &[item("nom_assez_long.rs", long)],
        );
        // 46 colonnes : label 17, espace, détail sur 28 dont « … ».
        let l = ligne(&b, 3);
        assert!(
            l.contains("│nom_assez_long.rs crates/ide-ui/src/très/prof…│"),
            "{l}"
        );
        assert_eq!(fit("日本語", 4), "日…");
        assert_eq!(fit("abc", 1), "");
        assert_eq!(fit("abc", 3), "abc");
    }
}
