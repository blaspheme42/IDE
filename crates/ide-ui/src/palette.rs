//! Rendu des palettes (F-50 Ctrl+P, F-51 Ctrl+Shift+P, F-52 Ctrl+Shift+F) : en haut de l'écran, sous la barre
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
    let search = &state.project_search;
    let title = match p.kind {
        PaletteKind::Files => Line::raw(" Ouvrir un fichier "),
        PaletteKind::Commands => Line::raw(" Commandes "),
        PaletteKind::Text => {
            let count = match (search.searching, search.truncated, search.hits.len()) {
                (true, ..) => "…".to_owned(),
                (_, _, 0) => String::new(),
                (_, true, n) => format!("{n}+ "),
                (.., n) => format!("{n} "),
            };
            let flag = |on| crate::search_flag(on, theme);
            Line::from(vec![
                Span::raw(" Rechercher dans le projet "),
                Span::styled("[.*]", flag(search.regex)),
                Span::raw(" "),
                Span::styled("[Aa]", flag(search.case_sensitive)),
                Span::raw(format!(" {count}")),
            ])
        }
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
    let text = p.kind == PaletteKind::Text;
    let lines: Vec<Line> = if p.kind == PaletteKind::Files && state.project_files.is_empty() {
        vec![Line::styled("Chargement…", muted)]
    } else if let (true, Some(e)) = (text, &search.error) {
        vec![Line::styled(fit(e, width), Style::new().fg(theme.error))]
    } else if text && search.searching && items.is_empty() {
        vec![Line::styled("Recherche…", muted)]
    } else if text && p.query.is_empty() {
        vec![]
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
                // Texte : place gardée pour le dossier, seul à distinguer deux `mod.rs:12`.
                let keep = match (text, it.detail.is_empty()) {
                    (true, false) => it.detail.width().min(width / 3) + 1,
                    _ => 0,
                };
                let label = fit(&it.label, width.saturating_sub(keep));
                let room = width.saturating_sub(label.width() + 1);
                let d = fit(&it.detail, room);
                let pad = width.saturating_sub(label.width() + d.width());
                let line = Line::from(vec![
                    Span::raw(label),
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
    use ide_core::{Document, Location, Palette, Search, TextHit};
    use ratatui::{Terminal, backend::TestBackend};

    fn state(kind: PaletteKind, query: &str, selected: usize) -> State {
        State {
            width: 80,
            height: 24,
            status: "Prêt".into(),
            doc: Document::from_text(Some("a.rs".into()), "fn main() {}\n"),
            palette: Some(Palette {
                ranked: vec![],
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

    fn hit() -> TextHit {
        let start = ide_core::Position {
            line: 0,
            character: 0,
        };
        TextHit {
            location: Location {
                path: "a.rs".into(),
                start,
                end: start,
            },
            excerpt: "cible".into(),
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
            ranked: vec![],
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
    fn palette_recherche_dans_le_projet() {
        let items = [
            item("src/main.rs:12: fn cible() {", ""),
            item("src/lib.rs:3: // cible", ""),
        ];
        let mut s = state(PaletteKind::Text, "cible", 0);
        s.project_search.hits = vec![hit(), hit()];
        s.project_search.case_sensitive = true;
        let b = rendu(&s, &items);
        insta::assert_snapshot!(b);
        let t = Theme::dark();
        let titre = ligne(&b, 1);
        let at = |motif: &str| {
            X + titre
                .chars()
                .skip(X as usize)
                .collect::<String>()
                .find(motif)
                .expect("indicateur") as u16
        };
        assert!(titre.contains("[.*] [Aa] 2 "), "{titre}");
        assert_eq!(b.buffer()[(at("[Aa]"), 1)].bg, t.foreground);
        assert_eq!(b.buffer()[(at("[.*]"), 1)].fg, t.muted);
    }

    #[test]
    fn recherche_etats_de_la_liste() {
        let mut s = state(PaletteKind::Text, "cible", 0);
        // Terminée sans rien.
        assert!(ligne(&rendu(&s, &[]), 3).contains("│Aucun résultat "));
        // En cours, sans résultat à garder à l'écran.
        s.project_search.searching = true;
        assert!(ligne(&rendu(&s, &[]), 3).contains("│Recherche… "));
        // En cours, résultats précédents gardés.
        s.project_search.hits = vec![hit()];
        let b = rendu(&s, &[item("a.rs:1: cible", "")]);
        assert!(ligne(&b, 1).contains("[Aa] …"), "{}", ligne(&b, 1));
        assert!(ligne(&b, 3).contains("│a.rs:1: cible "));
        // Plafond atteint.
        s.project_search.searching = false;
        s.project_search.truncated = true;
        let b = rendu(&s, &[item("a.rs:1: cible", "")]);
        assert!(ligne(&b, 1).contains("[Aa] 1+ "), "{}", ligne(&b, 1));
        // Regex invalide, message long tronqué.
        s.project_search.error = Some(format!("Regex invalide : {}", "x".repeat(80)));
        let b = rendu(&s, &[]);
        assert!(ligne(&b, 3).contains("│Regex invalide : xxx"));
        assert!(ligne(&b, 3).contains("x…│"));
        assert_eq!(b.buffer()[(X + 1, 3)].fg, Theme::dark().error);
        // Requête vide : ni attente ni « Aucun résultat ».
        let s = state(PaletteKind::Text, "", 0);
        let b = rendu(&s, &[]);
        assert!(!ligne(&b, 3).contains("Aucun"), "{}", ligne(&b, 3));
    }

    #[test]
    fn resultat_tronque_a_la_largeur() {
        let long = format!("src/a.rs:7: {}", "日本語 ".repeat(40));
        let b = rendu(
            &state(PaletteKind::Text, "x", 0),
            &[item(&long, ""), item("court", "")],
        );
        // 46 colonnes : l'étiquette ne déborde pas du cadre et n'avale pas la ligne suivante.
        let l = ligne(&b, 3);
        assert!(l.contains("│src/a.rs:7: 日"), "{l}");
        assert!(l.contains("…"), "{l}");
        assert_eq!(b.buffer()[(X + 47, 3)].symbol(), "│", "{l}");
        assert!(ligne(&b, 4).contains("│court "));
    }

    #[test]
    fn resultats_au_dela_de_cinquante_defilent() {
        let items: Vec<_> = (0..300)
            .map(|i| item(&format!("a.rs:{i}: x"), ""))
            .collect();
        let b = rendu(&state(PaletteKind::Text, "x", 299), &items);
        assert!(ligne(&b, 14).contains("│a.rs:299: x "));
        assert_eq!(
            b.buffer()[(X + 1, 14)].bg,
            Theme::dark().explorer_selected.bg.expect("fond")
        );
    }

    #[test]
    fn dossier_garde_sur_un_resultat_long() {
        let mut s = state(PaletteKind::Text, "x", 0);
        s.project_search.hits = vec![TextHit {
            location: Location {
                path: "src/a/mod.rs".into(),
                start: Default::default(),
                end: Default::default(),
            },
            excerpt: "x".repeat(200),
        }];
        if let Some(p) = &mut s.palette {
            p.ranked = vec![0];
        }
        let mut t = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        t.draw(|f| render_with(&s, f, &Theme::dark()))
            .expect("rendu");
        let ecran: String = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            ecran.contains("mod.rs:1: x") && ecran.contains("src/a"),
            "{ecran}"
        );
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
