//! Panneau de l'explorateur : arbre, icônes, sélection, invite.
//!
//! Géométrie (partagée avec le cœur pour les clics et le défilement) : contenu à l'intérieur de la
//! bordure ; ligne intérieure `i` = nœud `scroll + i` ; une invite prend la dernière ligne intérieure.
//! Une ligne : `2 * depth` espaces, icône (1 colonne), espace, nom. Pas de chevron séparé : l'icône
//! d'un dossier marque déjà l'état ouvert/fermé (glyphe Nerd, ou `-`/`+` en ASCII).

use std::borrow::Cow;

use ide_core::{Focus, PromptKind, State};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::icons::{IconSet, icon};
use crate::theme::Theme;

const LOADING: &str = " …";

/// Dessine l'explorateur dans `area` (bordures comprises).
pub(crate) fn render(state: &State, frame: &mut Frame, area: Rect, theme: &Theme) {
    let ex = &state.explorer;
    let focus = state.focus == Focus::Explorer;
    let dim = Style::new().fg(theme.muted);
    let icons = if state.settings.nerd_icons {
        IconSet::Nerd
    } else {
        IconSet::Ascii
    };
    let title = ex.root.as_ref().map_or(" EXPLORATEUR ".into(), |r| {
        let name = r.file_name().unwrap_or(r.as_os_str()).to_string_lossy();
        format!(" {} ", name.to_uppercase())
    });
    let block = Block::bordered().title(title).border_style(if focus {
        theme.border_focus
    } else {
        Style::new().fg(theme.border)
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }

    let rows = inner.height - u16::from(ex.prompt.is_some());
    let width = inner.width as usize;
    if ex.nodes.is_empty() {
        let msg = match (&ex.error, &ex.root) {
            (Some(e), _) => Span::styled(e.as_str(), Style::new().fg(theme.error)),
            (None, Some(_)) => Span::styled("(dossier vide)", dim),
            (None, None) => Span::raw(""),
        };
        frame.render_widget(
            Paragraph::new(msg),
            Rect {
                height: rows,
                ..inner
            },
        );
    }
    for (i, node) in ex
        .nodes
        .iter()
        .enumerate()
        .skip(ex.scroll)
        .take(rows as usize)
    {
        let row = Rect {
            y: inner.y + (i - ex.scroll) as u16,
            height: 1,
            ..inner
        };
        let e = &node.entry;
        let (glyph, color) = icon(&e.name, e.is_dir, node.expanded, icons);
        let indent = 2 * node.depth;
        let marker = if node.loading { LOADING } else { "" };
        let avail = width.saturating_sub(indent + 2 + marker.width());
        let name_style = if state.doc.path.as_deref() == Some(e.path.as_path()) {
            Style::new().bold()
        } else {
            Style::new()
        };
        let line = Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(glyph, Style::new().fg(color)),
            Span::raw(" "),
            Span::styled(fit(&e.name, avail), name_style),
            Span::styled(marker, dim),
        ]);
        // Sélection posée avant la ligne : l'icône garde sa couleur, le nom prend celle du thème.
        if i == ex.selected {
            let style = if focus {
                theme.explorer_selected
            } else {
                theme.explorer_selected_unfocused
            };
            frame.buffer_mut().set_style(row, style);
        }
        frame.render_widget(Paragraph::new(line), row);
    }

    if let Some(p) = &ex.prompt {
        let label: Cow<str> = match p.kind {
            PromptKind::NewFile => "Nouveau fichier : ".into(),
            PromptKind::NewDir => "Nouveau dossier : ".into(),
            PromptKind::Rename => "Renommer : ".into(),
            PromptKind::Move => "Déplacer vers : ".into(),
            PromptKind::ConfirmTrash => {
                // La cible de la commande, pas la ligne sélectionnée (qui a pu bouger depuis).
                let name = ex
                    .prompt_target
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                format!("Mettre « {name} » à la corbeille ? (o/n)").into()
            }
        };
        let row = Rect {
            y: inner.bottom() - 1,
            height: 1,
            ..inner
        };
        // Une cellule gardée pour le curseur ; saisie trop longue → on en montre la fin.
        let shown = crate::tail(&p.input, width.saturating_sub(label.width() + 1));
        let x = row.x + (label.width() + shown.width()) as u16;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(label, Style::new().bold()),
                Span::raw(shown),
            ])),
            row,
        );
        if focus {
            frame.set_cursor_position(Position::new(x.min(row.right() - 1), row.y));
        }
    }
}

/// `s` tenant dans `max` colonnes, terminé par `…` s'il a fallu couper.
fn fit(s: &str, max: usize) -> Cow<'_, str> {
    if s.width() <= max {
        return s.into();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        w += c.width().unwrap_or(0);
        if w + 1 > max {
            break;
        }
        out.push(c);
    }
    if max > 0 {
        out.push('…');
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Document, Explorer, FsEntry, Node, Prompt};
    use ratatui::style::{Color, Modifier};
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;

    fn node(rel: &str, is_dir: bool, depth: usize, expanded: bool) -> Node {
        Node {
            entry: FsEntry {
                name: rel.rsplit('/').next().unwrap_or(rel).into(),
                path: PathBuf::from("/proj").join(rel),
                is_dir,
            },
            depth,
            expanded,
            loading: false,
        }
    }

    /// Racine `/ide`, dossier `src` déplié, fichiers à plusieurs profondeurs ; `main.rs` ouvert.
    fn state() -> State {
        let mut nodes = vec![
            node("src", true, 0, true),
            node("src/ui", true, 1, true),
            node("src/ui/thème_🦀.rs", false, 2, false),
            node("src/main.rs", false, 1, false),
            node("target", true, 0, false),
            node(
                "un_nom_de_fichier_vraiment_beaucoup_trop_long.txt",
                false,
                0,
                false,
            ),
            node("Cargo.toml", false, 0, false),
            node("README.md", false, 0, false),
        ];
        nodes[4].loading = true;
        State {
            width: 60,
            height: 12,
            doc: Document::from_text(Some("/proj/src/main.rs".into()), ""),
            explorer: Explorer {
                root: Some("/home/moi/ide".into()),
                nodes,
                selected: 3,
                ..Explorer::default()
            },
            ..State::default()
        }
    }

    fn draw(state: &State) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        terminal
            .draw(|f| render(state, f, Rect::new(0, 0, 30, 10), &Theme::dark()))
            .unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn arbre() {
        let b = draw(&state());
        insta::assert_snapshot!(b);
        // Nom long : 2 (icône + espace) + 25 + `…` = largeur intérieure 28.
        assert_eq!(b.buffer()[(28, 6)].symbol(), "…");
    }

    #[test]
    fn invite_avec_saisie_longue() {
        let mut s = state();
        s.focus = Focus::Explorer;
        s.explorer.prompt = Some(Prompt {
            kind: PromptKind::NewFile,
            input: "dossier/sous/encore/fichier.rs".into(),
        });
        let b = draw(&s);
        insta::assert_snapshot!(b);
        // Fin de la saisie visible, curseur juste après, dernière ligne intérieure.
        assert_eq!(b.buffer()[(27, 8)].symbol(), "s");
        assert_eq!(b.cursor_position(), Position::new(28, 8));
    }

    #[test]
    fn invite_corbeille_nomme_la_cible() {
        let mut s = state();
        s.explorer.prompt = Some(Prompt {
            kind: PromptKind::ConfirmTrash,
            input: String::new(),
        });
        s.explorer.prompt_target = "/p/Cargo.toml".into();
        s.explorer.selected = 0;
        let b = draw(&s);
        let line: String = (1..29).map(|x| b.buffer()[(x, 8)].symbol()).collect();
        assert!(line.starts_with("Mettre « Cargo.toml » à la"), "{line}");
    }

    #[test]
    fn curseur_invite_seulement_si_focus_explorateur() {
        // Via le rendu complet : l'éditeur ne doit pas reprendre le curseur.
        let mut s = state();
        s.width = 80;
        s.height = 24;
        s.explorer.prompt = Some(Prompt {
            kind: PromptKind::Rename,
            input: "ab".into(),
        });
        let full = |s: &State| {
            let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
            t.draw(|f| crate::render(s, f)).unwrap();
            t.backend().cursor_position()
        };
        s.focus = Focus::Explorer;
        // Invite sur la ligne 21 (dernière intérieure au-dessus de la barre d'état), après « Renommer : ab ».
        assert_eq!(full(&s), Position::new(1 + 11 + 2, 21));
        s.focus = Focus::Editor;
        assert_eq!(
            full(&s).y,
            ide_core::TAB_BAR_HEIGHT,
            "curseur dans l'éditeur, sous la barre d'onglets"
        );
    }

    #[test]
    fn selection_sur_toute_la_largeur() {
        let mut s = state();
        let bg = |b: &TestBackend, x| b.buffer()[(x, 4)].bg;
        let b = draw(&s);
        assert!((1..29).all(|x| Some(bg(&b, x)) == Theme::dark().explorer_selected_unfocused.bg));
        assert_eq!(bg(&b, 0), Color::Reset, "bordure non surlignée");
        assert_eq!(b.buffer()[(1, 3)].bg, Color::Reset);
        s.focus = Focus::Explorer;
        let b = draw(&s);
        assert!((1..29).all(|x| Some(bg(&b, x)) == Theme::dark().explorer_selected.bg));
        assert!(b.buffer()[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn icone_coloree_et_fichier_ouvert_en_gras() {
        let b = draw(&state());
        let cell = |x, y| b.buffer()[(x, y)].clone();
        // `main.rs` (ligne 4, profondeur 1) : icône Rust en x = 3, nom en x = 5.
        assert_eq!(
            cell(3, 4).fg,
            icon("main.rs", false, false, IconSet::Nerd).1
        );
        assert_ne!(cell(3, 4).fg, Color::Reset);
        assert!(cell(5, 4).modifier.contains(Modifier::BOLD));
        assert!(!cell(3, 7).modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn icones_ascii() {
        let mut s = state();
        s.settings.nerd_icons = false;
        let b = draw(&s);
        insta::assert_snapshot!(b);
        // `src` déplié, `main.rs`, `target` replié.
        let sym = |x, y| b.buffer()[(x, y)].symbol().to_string();
        assert_eq!([sym(1, 1), sym(3, 4), sym(1, 5)], ["-", "*", "+"]);
    }

    #[test]
    fn defilement() {
        let mut s = state();
        s.explorer.scroll = 3;
        let b = draw(&s);
        let line = |y| -> String { (1..29).map(|x| b.buffer()[(x, y)].symbol()).collect() };
        assert!(line(1).contains("main.rs"), "{}", line(1));
        assert!(line(5).contains("README.md"));
        assert!(line(6).trim().is_empty());
        // La sélection (nœud 3) suit le défilement : première ligne intérieure.
        assert_eq!(
            Some(b.buffer()[(1, 1)].bg),
            Theme::dark().explorer_selected_unfocused.bg
        );
    }

    #[test]
    fn erreur_et_dossier_vide() {
        let mut s = state();
        s.explorer.nodes.clear();
        let line =
            |b: &TestBackend| -> String { (1..29).map(|x| b.buffer()[(x, 1)].symbol()).collect() };
        assert!(line(&draw(&s)).starts_with("(dossier vide)"));
        s.explorer.error = Some("Permission refusée".into());
        let b = draw(&s);
        assert!(line(&b).starts_with("Permission refusée"));
        assert_eq!(b.buffer()[(1, 1)].fg, Theme::dark().error);
        s.explorer = Explorer::default();
        assert!(line(&draw(&s)).trim().is_empty());
    }

    #[test]
    fn troncature_unicode() {
        assert_eq!(fit("abc", 3), "abc");
        assert_eq!(fit("abcd", 3), "ab…");
        assert_eq!(fit("日本語", 4), "日…");
        assert_eq!(fit("abc", 0), "");
    }
}
