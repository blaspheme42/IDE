//! Crate ide-ui : widgets ratatui.

mod explorer;
mod help;
mod hover;
pub mod icons;
mod lsp;
mod palette;
mod terminal;
pub mod theme;

use ide_core::{
    Focus, GitMark, LineEnding, Search, State, TAB_BAR_HEIGHT, banner_height, explorer_width,
    gutter_width, search_bar_height, tab_label, tab_list, tab_scroll, terminal_height,
};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::ops::Range;
use theme::Theme;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Dessine toute l'interface avec le thème de la configuration (sombre si nom inconnu).
pub fn render(state: &State, frame: &mut Frame) {
    let theme = Theme::by_name(&state.settings.theme).unwrap_or_else(Theme::dark);
    render_with(state, frame, &theme);
}

/// Dessine toute l'interface à partir de l'état, avec `theme`. Pure : aucune I/O.
///
/// Fond et texte du thème posés sur tout l'écran, comme VSCode : coloration et ligne courante
/// sont choisies pour ce fond, celui du terminal (clair, transparent…) pourrait les rendre
/// illisibles. Contrepartie assumée : un terminal transparent devient opaque.
pub fn render_with(state: &State, frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().fg(theme.foreground).bg(theme.background));
    let [main, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [explorer, editor] = Layout::horizontal([
        Constraint::Length(explorer_width(state)),
        Constraint::Min(0),
    ])
    .areas(main);
    // Même hauteur que `editor_size` : barres d'onglets, bandeau, recherche, panneaux Problèmes et
    // terminal pris sur la zone éditeur.
    let problems_h = if state.problems_open {
        lsp::PROBLEMS_HEIGHT
    } else {
        0
    };
    let [tabs, banner, editor, bar, problems, terminal] = Layout::vertical([
        Constraint::Length(TAB_BAR_HEIGHT),
        Constraint::Length(banner_height(state)),
        Constraint::Min(0),
        Constraint::Length(search_bar_height(state)),
        Constraint::Length(problems_h),
        Constraint::Length(terminal_height(state)),
    ])
    .areas(editor);
    let [gutter, text] =
        Layout::horizontal([Constraint::Length(gutter_width(state)), Constraint::Min(0)])
            .areas(editor);

    if explorer.width > 0 {
        explorer::render(state, frame, explorer, theme);
    }
    tab_bar(state, tabs, frame, theme);
    // Tant que l'offre attend : le statut, lui, est écrasé par le message suivant.
    if let Some((path, _)) = &state.backup_offer {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        let text = format!(
            " Copie de secours de {name} plus récente que le disque : Alt+B restaurer · Alt+N ignorer"
        );
        let style = Style::new().fg(theme.warning).add_modifier(Modifier::BOLD);
        frame.render_widget(Paragraph::new(text).style(style), banner);
    }

    let doc = &state.doc;
    let rope = doc.rope();
    let (cur_line, cur_col) = doc.cursor_line_col();
    let selection = doc.selection();
    let brackets = doc.bracket_pair();
    let matches = state.search.as_ref().map_or(&[][..], |s| &s.matches[..]);
    let current = state
        .search
        .as_ref()
        .and_then(|s| s.current)
        .and_then(|i| matches.get(i));
    // NF-03 : seules les lignes visibles sont parcourues ; un bloc replié compte pour une.
    let shown = doc.visible_lines(state.scroll_line, text.height as usize);
    let visible = shown
        .first()
        .map_or(0..0, |&a| a..shown[shown.len() - 1] + 1);
    let diags = lsp::visible_diagnostics(state, &visible);
    let cur_row = shown.iter().position(|&l| l == cur_line);
    let nerd = state.settings.nerd_icons;
    let mut numbers = Vec::new();
    let mut lines = Vec::new();
    for &i in &shown {
        let folded = doc.is_folded(i);
        let style = if i == cur_line {
            theme.gutter_current
        } else {
            Style::new().fg(theme.gutter)
        };
        // Diagnostics touchant la ligne, du plus grave au moins grave, en chars de la ligne.
        let start = rope.line_to_char(i);
        let line = rope.line(i);
        let end = start + line.len_chars();
        let rel = |r: &Range<usize>| r.start.saturating_sub(start)..r.end.saturating_sub(start);
        let marks: Vec<_> = diags
            .iter()
            .filter(|(r, _)| r.start < end && r.end > start)
            .map(|(r, sev)| (rel(r), *sev))
            .collect();
        let marker = marks.first().map_or(Span::raw(" "), |(_, sev)| {
            Span::styled(
                lsp::icon(*sev, nerd),
                Style::new().fg(lsp::color(*sev, theme)),
            )
        });
        // Gouttière : marque git, numéro aligné à droite et espace (s'ils sont affichés), marque de
        // repli, colonne de diagnostics.
        let (bar, color) = match state.git_marks.get(i).copied().flatten() {
            None => (" ", theme.gutter),
            Some(GitMark::Added) => ("▎", theme.git_added),
            Some(GitMark::Modified) => ("▎", theme.git_modified),
            Some(GitMark::DeletedBelow) => ("▁", theme.git_deleted),
            Some(GitMark::DeletedAbove) => ("▔", theme.git_deleted),
        };
        let mut row = vec![Span::styled(bar, Style::new().fg(color))];
        if state.settings.line_numbers {
            let w = usize::from(gutter.width).saturating_sub(4);
            row.push(Span::styled(format!("{:>w$} ", i + 1), style));
        }
        row.extend([
            Span::styled(
                if folded {
                    "+"
                } else if doc.opens_block(i) {
                    "-"
                } else {
                    " "
                },
                style,
            ),
            marker,
        ]);
        numbers.push(Line::from(row));
        // Sélection ramenée en index de char relatifs à la ligne (vide si hors ligne).
        let sel = selection.as_ref().map_or(0..0, rel);
        // Occurrences touchant la ligne : triées, la première trouvée par recherche binaire.
        let first = matches.partition_point(|m| m.end <= start);
        let hits: Vec<_> = matches[first..]
            .iter()
            .take_while(|m| m.start < end)
            .map(|m| {
                let style = if Some(m) == current {
                    theme.search_current
                } else {
                    theme.search_match
                };
                (rel(m), style)
            })
            .collect();
        // Coloration touchant la ligne : octets ramenés en chars de la ligne, bornés à celle-ci
        // (une plage périmée ne doit pas faire paniquer la rope).
        let b0 = rope.line_to_byte(i);
        let b1 = b0 + line.len_bytes();
        let first = state.highlights.partition_point(|(r, _)| r.end <= b0);
        let colors: Vec<_> = state.highlights[first..]
            .iter()
            .take_while(|(r, _)| r.start < b1)
            .map(|(r, cat)| {
                let c = |b: usize| rope.byte_to_char(b.clamp(b0, b1)) - start;
                (c(r.start)..c(r.end), theme.syntax(cat))
            })
            .collect();
        let find = |v: &[(Range<usize>, Style)], k| {
            v.iter().find(|(r, _)| r.contains(&k)).map(|(_, s)| *s)
        };
        let eol = line
            .len_chars()
            .checked_sub(1)
            .filter(|k| sel.contains(k))
            .map(|_| theme.selection);
        let mut spans = visible_slice(
            line.chars(),
            state.scroll_col,
            text.width as usize,
            doc.tab_width(),
            eol,
            |k| {
                // Priorité : sélection > occurrence (courante ou non) > coloration seule.
                // Diagnostic : souligné de sa couleur, la couleur du texte reste celle de la coloration.
                let mut base = find(&colors, k).unwrap_or_default();
                if let Some((_, sev)) = marks.iter().find(|(r, _)| r.contains(&k)) {
                    base = base
                        .add_modifier(Modifier::UNDERLINED)
                        .underline_color(lsp::color(*sev, theme));
                }
                if sel.contains(&k) {
                    base.patch(theme.selection)
                } else if brackets.is_some_and(|(a, b)| start + k == a || start + k == b) {
                    base.patch(theme.bracket_match)
                } else {
                    find(&hits, k).map_or(base, |s| base.patch(s))
                }
            },
        );
        if folded {
            spans.push(Span::styled(" ⋯", Style::new().fg(theme.gutter)));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(numbers), gutter);
    // Ligne courante posée avant le texte pour que le fond des occurrences reste visible.
    if let Some(row) = cur_row.filter(|_| selection.is_none()) {
        let y = text.y + row as u16;
        frame.buffer_mut().set_style(
            Rect {
                y,
                height: 1,
                ..text
            },
            Style::new().bg(theme.current_line),
        );
    }
    frame.render_widget(Paragraph::new(lines), text);
    if state.problems_open {
        lsp::problems_panel(state, frame, problems, theme);
    }
    if state.terminal.open {
        terminal::panel(state, frame, terminal, theme);
    }
    lsp::hover(state, frame, text, theme);
    lsp::completion(state, frame, text, theme);

    // Focus ailleurs : le curseur est celui de l'invite de l'explorateur ou du shell, ou masqué.
    let editor_focus = state.focus == Focus::Editor;
    if let Some(s) = &state.search {
        search_bar(s, bar, frame, editor_focus, theme);
    } else if let Some(row) = cur_row.filter(|_| {
        editor_focus
            && (state.scroll_col..state.scroll_col + text.width as usize).contains(&cur_col)
    }) && state.help.is_none()
    {
        frame.set_cursor_position(Position::new(
            text.x + (cur_col - state.scroll_col) as u16,
            text.y + row as u16,
        ));
    }

    let dirty = if doc.is_dirty() { " ●" } else { "" };
    let counts = match lsp::diagnostic_counts(state) {
        (0, 0) => String::new(),
        (e, w) if nerd => format!("✖ {e} ⚠ {w} | "),
        (e, w) => format!("E {e} W {w} | "),
    };
    // F-70 : branche en tête, icône Nerd seulement si disponible.
    let branch = match &state.git_branch {
        Some(b) if nerd => format!("\u{e725} {b} | "),
        Some(b) => format!("{b} | "),
        None => String::new(),
    };
    // Dossier du projet avant la branche ; rappel de sortie quand l'éditeur n'a pas le focus.
    let project = state
        .explorer
        .root
        .as_deref()
        .and_then(|r| r.file_name())
        .map_or_else(String::new, |n| format!("{} | ", n.to_string_lossy()));
    let hint = match state.focus {
        Focus::Editor => "",
        Focus::Explorer => "Échap/Ctrl+E : retour au code | ",
        Focus::Terminal => "Ctrl+ù/Alt+J : fermer le terminal | ",
    };
    let left = format!(
        " {hint}{project}{branch}{counts}{}{dirty} | {} ",
        doc.display_name(),
        state.status
    );
    let lsp_status = if state.lsp_status.is_empty() {
        String::new()
    } else {
        format!("{} | ", state.lsp_status)
    };
    let ending = match doc.line_ending {
        LineEnding::Lf => "LF",
        LineEnding::CrLf => "CRLF",
    };
    let right = format!(
        "{lsp_status}Ln {}, Col {} | {ending} | UTF-8 ",
        cur_line + 1,
        cur_col + 1
    );
    // « F1 Raccourcis » toujours visible à gauche : le reste ne s'écrit qu'à sa droite.
    const F1: &str = " F1 Raccourcis ";
    let [f1, status] =
        Layout::horizontal([Constraint::Length(F1.len() as u16), Constraint::Min(0)]).areas(status);
    frame.render_widget(
        Paragraph::new(F1).style(theme.search_current.add_modifier(Modifier::BOLD)),
        f1,
    );
    // La partie droite garde sa place ; la gauche est tronquée avant elle au lieu de s'y coller.
    let right = Line::from(format!(" {right}"));
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right.width() as u16)])
            .areas(status);
    frame.render_widget(Paragraph::new(left).style(theme.status_bar), left_area);
    frame.render_widget(
        Paragraph::new(right)
            .alignment(Alignment::Right)
            .style(theme.status_bar),
        right_area,
    );
    // En dernier : recouvrent tout et prennent le curseur placé plus haut (éditeur, recherche, explorateur).
    palette::render(state, frame, theme);
    help::render(state, frame, theme);
}

/// Barre d'onglets ` nom ● │ nom2 × │ …`, défilée comme `tab_spans` pour garder l'actif visible.
fn tab_bar(state: &State, area: Rect, frame: &mut Frame, theme: &Theme) {
    let mut spans = Vec::new();
    for t in tab_list(state) {
        if !spans.is_empty() {
            spans.push(Span::styled("│", Style::new().fg(theme.border)));
        }
        let style = if t.active {
            theme.tab_active
        } else {
            theme.tab_inactive
        };
        spans.push(Span::styled(tab_label(&t), style));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).scroll((0, tab_scroll(state))),
        area,
    );
}

/// Barre de recherche : `Rechercher : <query>` + indicateurs et compteur à droite,
/// puis `Remplacer  : <replace>` en mode remplacer. Curseur du terminal à la fin du champ actif.
fn search_bar(s: &Search, area: Rect, frame: &mut Frame, cursor: bool, theme: &Theme) {
    // Indicateurs `[.*]` `[Aa]` : actif en vidéo inverse du thème, inactif estompé.
    let on = Style::new().fg(theme.background).bg(theme.foreground);
    let off = Style::new().fg(theme.muted);
    let flag = |is_on, t| Span::styled(t, if is_on { on } else { off });
    let count = match (&s.error, s.current) {
        (Some(e), _) => Span::styled(e.clone(), Style::new().fg(theme.error)),
        _ if s.query.is_empty() => Span::raw(""),
        (None, Some(i)) => Span::raw(format!("{}/{}", i + 1, s.matches.len())),
        (None, None) => Span::raw("Aucun résultat"),
    };
    let right = Line::from(vec![
        flag(s.regex, "[.*]"),
        Span::raw(" "),
        flag(s.case_sensitive, "[Aa]"),
        Span::raw(" "),
        count,
        Span::raw(" "),
    ]);
    let mut fields = vec![(
        "Rechercher : ",
        &s.query,
        area.width.saturating_sub(right.width() as u16),
    )];
    if let Some(r) = &s.replace {
        fields.push(("Remplacer  : ", r, area.width));
    }
    let active = usize::from(s.focus_replace && s.replace.is_some());
    for (i, (label, field, width)) in fields.into_iter().enumerate() {
        let row = Rect {
            y: area.y + i as u16,
            height: 1,
            width,
            ..area
        };
        // Une cellule gardée pour le curseur ; champ trop long → on en montre la fin.
        let shown = tail(field, (width as usize).saturating_sub(label.width() + 1));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(label, Style::new().bold()),
                Span::raw(shown),
            ])),
            row,
        );
        if cursor && i == active {
            let x = row.x + (label.width() + shown.width()) as u16;
            frame.set_cursor_position(Position::new(x.min(row.right().saturating_sub(1)), row.y));
        }
    }
    frame.render_widget(
        Paragraph::new(right).alignment(Alignment::Right),
        Rect { height: 1, ..area },
    );
}

/// Plus long suffixe de `s` tenant dans `max` colonnes.
fn tail(s: &str, max: usize) -> &str {
    let mut w = 0;
    for (i, c) in s.char_indices().rev() {
        w += c.width().unwrap_or(0);
        if w > max {
            return &s[i + c.len_utf8()..];
        }
    }
    s
}

/// Partie d'une ligne entre les colonnes d'affichage `start` et `start + width`, en spans stylés.
/// Tabulations développées en espaces (taquets tous les `tab`) ; caractère large coupé à gauche → espaces.
/// `style_of(k)` : style du char d'index `k` de la ligne ; `eol` : style d'une cellule marquée
/// après la fin de ligne (saut de ligne sélectionné, comme VSCode).
fn visible_slice(
    chars: impl Iterator<Item = char>,
    start: usize,
    width: usize,
    tab: usize,
    eol: Option<Style>,
    style_of: impl Fn(usize) -> Style,
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
        if c == '\n' {
            if let Some(style) = eol.filter(|_| col >= start) {
                push(" ", style);
            }
            break;
        }
        let style = style_of(k);
        let w = match c {
            '\t' => tab - col % tab,
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
    use ide_core::{CompletionItem, Diagnostic, Document, Severity, TermColor, TermSpan};
    use ratatui::buffer::Cell;
    use ratatui::style::{Color, Modifier};
    use ratatui::{Terminal, backend::TestBackend};

    fn draw(state: &State) -> TestBackend {
        draw_with(state, &Theme::dark())
    }

    fn draw_with(state: &State, theme: &Theme) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| render_with(state, f, theme)).unwrap();
        terminal.backend().clone()
    }

    /// Fond de sélection du thème sombre.
    fn sel_bg() -> Color {
        Theme::dark().selection.bg.expect("fond de sélection")
    }

    /// La cellule porte-t-elle le style `s` (couleurs et modificateurs qu'il définit) ?
    fn a_style(c: &Cell, s: Style) -> bool {
        s.fg.is_none_or(|f| f == c.fg)
            && s.bg.is_none_or(|b| b == c.bg)
            && c.modifier.contains(s.add_modifier)
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
    fn bandeau_de_copie_de_secours_tant_que_l_offre_attend() {
        let mut s = state(Document::from_text(Some("/x/a.txt".into()), "un\ndeux"));
        s.backup_offer = Some(("/x/a.txt".into(), "copie".into()));
        s.status = "Ouvert : b.txt".into();
        let b = draw(&s);
        let row = |b: &TestBackend, y: u16| -> String {
            (30..80).map(|x| b.buffer()[(x, y)].symbol()).collect()
        };
        assert!(
            row(&b, 1).starts_with(" Copie de secours de a.txt plus récente que le"),
            "{}",
            row(&b, 1)
        );
        assert_eq!(b.buffer()[(31, 1)].fg, Theme::dark().warning);
        // Le texte commence une ligne plus bas, le curseur aussi.
        assert!(row(&b, 2).contains("1   un"), "{}", row(&b, 2));
        assert_eq!(b.cursor_position(), Position::new(38, 2));
        s.backup_offer = None;
        let b = draw(&s);
        assert!(row(&b, 1).contains("1   un"), "{}", row(&b, 1));
    }

    #[test]
    fn document_vide() {
        let backend = draw(&state(Document::from_text(None, "")));
        insta::assert_snapshot!(backend);
        assert_eq!(backend.cursor_position(), Position::new(38, 1));
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
        assert_eq!(backend.cursor_position(), Position::new(38 + 9, 3));
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
    fn barre_d_onglets() {
        let mut s = state(Document::from_text(Some("b.rs".into()), "x"));
        s.tabs.push(ide_core::Tab {
            doc: Document::from_text(Some("a.rs".into()), ""),
            ..ide_core::Tab::default()
        });
        s.active = 1;
        s.doc.insert_char('y');
        let b = draw(&s);
        let line: String = (30..80).map(|x| b.buffer()[(x, 0)].symbol()).collect();
        assert_eq!(line.trim_end(), " a.rs × │ b.rs ●");
        // Actif et inactifs aux styles du thème, séparateur couleur de bordure.
        let t = Theme::dark();
        let cell = |x: u16| &b.buffer()[(x, 0)];
        assert!(a_style(cell(40), t.tab_active));
        assert!(a_style(cell(31), t.tab_inactive));
        assert!(!cell(31).modifier.contains(Modifier::BOLD));
        assert_eq!(cell(38).fg, t.border);
        // Trop étroit : la barre défile pour montrer l'actif en entier.
        s.width = 40;
        let mut t = Terminal::new(TestBackend::new(40, 24)).unwrap();
        t.draw(|f| render(&s, f)).unwrap();
        let line: String = (30..40)
            .map(|x| t.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert_eq!(line, " │ b.rs ● ");
    }

    #[test]
    fn tranche_visible() {
        let s = |t: &str, start, width| -> String {
            visible_slice(t.chars(), start, width, 4, None, |_| Style::new())
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

    /// Zone texte : x = 30 (explorateur) + 8 (gouttière), y = 1 (sous la barre d'onglets).
    const X: u16 = 38;
    const Y: u16 = TAB_BAR_HEIGHT;

    fn selectionne(doc: Document, sel: Range<usize>, scroll_col: usize) -> TestBackend {
        let mut doc = doc;
        doc.set_cursor(sel.start, false);
        doc.set_cursor(sel.end, true);
        let mut s = state(doc);
        s.scroll_col = scroll_col;
        draw(&s)
    }

    /// Colonnes de la ligne `y` (relatives à la zone texte) sur fond de sélection, parmi 0..n.
    fn inverses(b: &TestBackend, y: u16, n: u16) -> Vec<u16> {
        (0..n)
            .filter(|&x| b.buffer()[(X + x, Y + y)].bg == sel_bg())
            .collect()
    }

    #[test]
    fn selection_une_ligne() {
        let b = selectionne(Document::from_text(None, "abcdef"), 1..4, 0);
        assert_eq!(inverses(&b, 0, 10), [1, 2, 3]);
        // Pas de ligne courante surlignée pendant une sélection : fond du thème.
        assert_eq!(b.buffer()[(X + 8, Y)].bg, Theme::dark().background);
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
        assert_eq!(b.buffer()[(X + 1, Y)].symbol(), "x");
    }

    #[test]
    fn ligne_courante_surlignee() {
        let mut doc = Document::from_text(None, "a\nb");
        doc.move_down(1, false);
        let b = draw(&state(doc));
        let t = Theme::dark();
        let bg = |x, y| b.buffer()[(x, Y + y)].bg;
        assert_eq!(bg(X, 1), t.current_line);
        assert_eq!(bg(79, 1), t.current_line);
        assert_eq!(bg(X - 1, 1), t.background, "gouttière non surlignée");
        assert_eq!(bg(X, 0), t.background);
        assert_eq!(b.buffer()[(X, Y)].fg, t.foreground);
        // Numéros de ligne : courant au style dédié, les autres en couleur de gouttière.
        assert!(a_style(&b.buffer()[(34, Y + 1)], t.gutter_current));
        assert_eq!(b.buffer()[(34, Y)].fg, t.gutter);
    }

    #[test]
    fn tabulation_largeur_editorconfig() {
        let mut doc = Document::from_text(None, "\tx\ny");
        doc.indent = ide_core::Indent {
            use_tabs: true,
            width: 8,
            tab_width: 8,
        };
        doc.move_right(false);
        let b = draw(&state(doc));
        assert_eq!(b.buffer()[(X + 8, Y)].symbol(), "x");
        assert_eq!(b.cursor_position(), Position::new(X + 8, Y));
    }

    /// Document « foo bar foo baz foo » répété sur 40 lignes, occurrences de « foo » ; courante = `current`.
    fn recherche(replace: Option<&str>, current: usize) -> State {
        let text = "foo bar foo baz foo\n".repeat(40);
        let matches = (0..40)
            .flat_map(|l| [0, 8, 16].map(|c| l * 20 + c..l * 20 + c + 3))
            .collect();
        let mut s = state(Document::from_text(Some("a.txt".into()), &text));
        s.search = Some(Search {
            query: "foo".into(),
            replace: replace.map(Into::into),
            focus_replace: replace.is_some(),
            case_sensitive: true,
            matches,
            current: Some(current),
            ..Search::default()
        });
        s
    }

    #[test]
    fn barre_rechercher() {
        let s = recherche(None, 4);
        let b = draw(&s);
        insta::assert_snapshot!(b);
        // Barre sur la dernière ligne de la zone éditeur, curseur après « Rechercher : foo ».
        assert_eq!(b.cursor_position(), Position::new(30 + 13 + 3, 22));
        assert_eq!(b.buffer()[(30, 22)].symbol(), "R");
        // Indicateurs : casse active, regex inactive.
        let fg = |x| b.buffer()[(x, 22)].bg;
        let line: String = (30..80).map(|x| b.buffer()[(x, 22)].symbol()).collect();
        let at = |t: &str| 30 + line.find(t).map_or(0, |i| line[..i].chars().count()) as u16;
        let t = Theme::dark();
        assert_eq!(fg(at("[Aa]")), t.foreground);
        assert_eq!(fg(at("[.*]")), t.background);
        assert_eq!(b.buffer()[(at("[.*]"), 22)].fg, t.muted);
        // Barre d'état sur toute la largeur, après le badge « F1 Raccourcis ».
        assert!((15..80).all(|x| a_style(&b.buffer()[(x, 23)], t.status_bar)));
        assert!(line.contains("5/120"));
        // La zone de texte s'arrête au-dessus : 21 lignes visibles, sous la barre d'onglets.
        assert_eq!(b.buffer()[(0, 21)].symbol(), "│");
        assert_eq!(b.buffer()[(34, 21)].symbol(), "1");
    }

    #[test]
    fn barre_remplacer() {
        let mut s = recherche(Some("quux"), 0);
        if let Some(r) = s.search.as_mut() {
            r.error = Some("Regex invalide : x".into());
        }
        let b = draw(&s);
        insta::assert_snapshot!(b);
        assert_eq!(b.cursor_position(), Position::new(30 + 13 + 4, 22));
    }

    #[test]
    fn focus_explorateur_curseur_hors_de_la_barre() {
        let mut s = recherche(None, 0);
        s.focus = ide_core::Focus::Explorer;
        let b = draw(&s);
        // Ni la barre (ligne 22) ni le texte : rien n'est placé, le curseur reste à l'origine.
        assert_eq!(b.cursor_position(), Position::new(0, 0));
    }

    #[test]
    fn champ_trop_long_montre_la_fin() {
        let mut s = recherche(Some(""), 0);
        if let Some(r) = s.search.as_mut() {
            r.focus_replace = false;
            r.query = format!("{}日本", "x".repeat(60));
        }
        let b = draw(&s);
        let line: String = (30..80).map(|x| b.buffer()[(x, 21)].symbol()).collect();
        assert!(
            line.starts_with("Rechercher : xxx") && line.contains("x日"),
            "{line}"
        );
        let x = b.cursor_position().x;
        // Le curseur suit le dernier caractère affiché (日本 occupe 4 cellules).
        assert_eq!(b.buffer()[(x - 2, 21)].symbol(), "本");
    }

    #[test]
    fn occurrences_surlignees() {
        let s = recherche(None, 4); // ligne 1, colonne 8
        let b = draw(&s);
        let t = Theme::dark();
        let cell = |x, y| b.buffer()[(X + x, Y + y)].clone();
        assert!(a_style(&cell(8, 1), t.search_current));
        assert!(a_style(&cell(10, 1), t.search_current));
        assert_eq!(cell(11, 1).bg, t.background);
        assert!(a_style(&cell(0, 1), t.search_match));
        assert!(!cell(4, 1).modifier.contains(Modifier::UNDERLINED));
        // Ligne courante (0) : son fond reste hors des occurrences.
        assert!(a_style(&cell(0, 0), t.search_match));
        assert_eq!(cell(4, 0).bg, t.current_line);
    }

    #[test]
    fn occurrence_courante_hors_ecran_et_selection_prioritaire() {
        // Courante en ligne 39, hors écran : aucune cellule au style de la courante.
        let mut s = recherche(None, 3 * 39);
        s.doc.set_cursor(8, false);
        s.doc.set_cursor(11, true);
        let b = draw(&s);
        let courante = Theme::dark()
            .search_current
            .bg
            .expect("fond de la courante");
        for y in Y..22 {
            for x in 0..42 {
                assert_ne!(b.buffer()[(X + x, y)].bg, courante, "({x}, {y})");
            }
        }
        // La sélection l'emporte sur le surlignage.
        assert_eq!(b.buffer()[(X + 8, Y)].bg, sel_bg());
        assert!(
            !b.buffer()[(X + 8, Y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    /// État sur `text` avec la coloration `hl` (plages en octets).
    fn colore(text: &str, hl: &[(Range<usize>, &'static str)]) -> State {
        let mut s = state(Document::from_text(Some("a.rs".into()), text));
        s.highlights = hl.to_vec();
        s
    }

    /// Couleur de texte des cellules `xs` de la ligne `y` de la zone texte.
    fn fgs(b: &TestBackend, y: u16, xs: Range<u16>) -> Vec<Color> {
        xs.map(|x| b.buffer()[(X + x, Y + y)].fg).collect()
    }

    fn syn(cat: &str) -> Color {
        Theme::dark().syntax(cat).fg.expect("couleur de catégorie")
    }

    #[test]
    fn mot_cle_colore() {
        let s = colore("fn main() {}", &[(0..2, "keyword"), (3..7, "function")]);
        let b = draw(&s);
        let (k, f, t) = (syn("keyword"), syn("function"), Theme::dark().foreground);
        assert_eq!(fgs(&b, 0, 0..8), [k, k, t, f, f, f, f, t]);
        // La ligne courante garde son fond sous la coloration.
        assert_eq!(b.buffer()[(X, Y)].bg, Theme::dark().current_line);
    }

    #[test]
    fn coloration_accents_emoji() {
        // Octets : é 0..2, 🦀 2..6, "café" 6..13 (é sur 2 octets), espace 13, fn 14..16.
        // Colonnes : é 0, 🦀 1..3, "café" 3..9, espace 9, fn 10..12.
        let s = colore("é🦀\"café\" fn", &[(6..13, "string"), (14..16, "keyword")]);
        let b = draw(&s);
        let (st, k, t) = (syn("string"), syn("keyword"), Theme::dark().foreground);
        // Cellule de queue de 🦀 (2) remise à zéro par ratatui : seules les cellules de tête comptent.
        assert_eq!(fgs(&b, 0, 0..2), [t, t]);
        assert_eq!(fgs(&b, 0, 3..9), [st; 6]);
        assert_eq!(fgs(&b, 0, 9..12), [t, k, k]);
    }

    #[test]
    fn tabulation_avant_mot_cle() {
        let b = draw(&colore("\tfn x", &[(1..3, "keyword")]));
        let (k, t) = (syn("keyword"), Theme::dark().foreground);
        assert_eq!(fgs(&b, 0, 0..7), [t, t, t, t, k, k, t]);
        assert_eq!(b.buffer()[(X + 4, Y)].symbol(), "f");
    }

    #[test]
    fn defilement_coupe_un_mot_colore() {
        let mut s = colore("fn hello 日本", &[(3..8, "function"), (9..15, "string")]);
        s.scroll_col = 5;
        let b = draw(&s);
        let (f, st, t) = (syn("function"), syn("string"), Theme::dark().foreground);
        // « llo », espace, 日 coupé à gauche → espace coloré, 本.
        assert_eq!(fgs(&b, 0, 0..4), [f, f, f, t]);
        assert_eq!(b.buffer()[(X, Y)].symbol(), "l");
        s.scroll_col = 10;
        let b = draw(&s);
        assert_eq!(b.buffer()[(X, Y)].symbol(), " ");
        assert_eq!(fgs(&b, 0, 0..2), [st, st]);
    }

    #[test]
    fn selection_prioritaire_sur_coloration() {
        let mut s = colore("fn x", &[(0..2, "keyword")]);
        s.doc.set_cursor(0, false);
        s.doc.set_cursor(1, true);
        let b = draw(&s);
        let c = |x| b.buffer()[(X + x, Y)].clone();
        // Fond de sélection, couleur du mot-clé conservée dessous (comme VSCode).
        assert_eq!((c(0).bg, c(0).fg), (sel_bg(), syn("keyword")));
        assert_eq!(
            (c(1).bg, c(1).fg),
            (Theme::dark().background, syn("keyword"))
        );
    }

    #[test]
    fn plage_hors_ecran_ignoree() {
        // 40 lignes « fn x » (5 octets) ; défilement à la ligne 10, plage périmée au-delà du texte.
        let mut s = colore(
            &"fn x\n".repeat(40),
            &[
                (0..2, "keyword"),
                (50..52, "keyword"),
                (9_000..9_010, "string"),
            ],
        );
        s.scroll_line = 10;
        let b = draw(&s);
        let (k, t) = (syn("keyword"), Theme::dark().foreground);
        assert_eq!(fgs(&b, 0, 0..3), [k, k, t]);
        assert_eq!(fgs(&b, 1, 0..3), [t, t, t]);
    }

    #[test]
    fn theme_clair() {
        let t = Theme::light();
        let b = draw_with(&colore("fn x", &[(0..2, "keyword")]), &t);
        let c = |x| b.buffer()[(X + x, Y + 1)].clone();
        assert_eq!(
            b.buffer()[(X, Y)].fg,
            t.syntax("keyword").fg.expect("couleur du mot-clé")
        );
        assert_eq!(b.buffer()[(X + 3, Y)].fg, t.foreground);
        // Ligne vide sous le texte : fond et texte du thème clair.
        assert_eq!((c(0).fg, c(0).bg), (t.foreground, t.background));
    }

    fn draw_settings(theme: &str) -> TestBackend {
        let mut s = State::default();
        s.settings.theme = theme.into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| render(&s, f)).unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn render_suit_le_theme_configure() {
        let (l, d) = (Theme::light(), Theme::dark());
        let c = draw_settings("light").buffer()[(X, Y + 1)].clone();
        assert_eq!((c.fg, c.bg), (l.foreground, l.background));
        // Nom inconnu : repli sur le sombre.
        let c = draw_settings("solarized").buffer()[(X, Y + 1)].clone();
        assert_eq!((c.fg, c.bg), (d.foreground, d.background));
    }

    fn diag(l0: usize, c0: usize, l1: usize, c1: usize, severity: Severity) -> Diagnostic {
        let pos = |line, character| ide_core::Position { line, character };
        Diagnostic {
            start: pos(l0, c0),
            end: pos(l1, c1),
            severity,
            message: format!("problème {l0}:{c0}"),
            source: None,
        }
    }

    /// État sur `text` (fichier `/p/src/a.rs`) avec les diagnostics `ds`.
    fn diagnostique(text: &str, ds: Vec<Diagnostic>) -> State {
        let mut s = state(Document::from_text(Some("/p/src/a.rs".into()), text));
        s.diagnostics.insert("/p/src/a.rs".into(), ds);
        s
    }

    fn souligne(b: &TestBackend, x: u16, y: u16) -> Option<Color> {
        let c = &b.buffer()[(X + x, Y + y)];
        c.modifier
            .contains(Modifier::UNDERLINED)
            .then_some(c.underline_color)
    }

    fn ligne(b: &TestBackend, y: u16) -> String {
        (0..80).map(|x| b.buffer()[(x, y)].symbol()).collect()
    }

    #[test]
    fn diagnostic_souligne_apres_emoji() {
        // 🦀 = 2 unités UTF-16 et 2 colonnes : `x` (UTF-16 2..3) est en colonne 2.
        let mut s = diagnostique("🦀x = y", vec![diag(0, 2, 0, 3, Severity::Error)]);
        s.highlights = vec![(4..5, "variable")];
        let b = draw(&s);
        let err = Theme::dark().error;
        assert_eq!(souligne(&b, 2, 0), Some(err));
        assert_eq!(souligne(&b, 0, 0), None);
        assert_eq!(souligne(&b, 3, 0), None);
        // La couleur du texte reste celle de la coloration.
        assert_eq!(b.buffer()[(X + 2, Y)].fg, syn("variable"));
    }

    #[test]
    fn diagnostic_multi_ligne() {
        let b = draw(&diagnostique(
            "abc\ndef\nghi",
            vec![diag(0, 1, 1, 2, Severity::Warning)],
        ));
        let w = Some(Theme::dark().warning);
        let l = |y| (0..3).map(|x| souligne(&b, x, y)).collect::<Vec<_>>();
        assert_eq!(l(0), [None, w, w]);
        assert_eq!(l(1), [w, w, None]);
        assert_eq!(l(2), [None; 3]);
        // Marqueur sur les deux lignes touchées.
        assert_eq!(b.buffer()[(X - 1, Y + 1)].symbol(), "⚠");
        assert_eq!(b.buffer()[(X - 1, Y + 2)].symbol(), " ");
    }

    #[test]
    fn gouttiere_le_plus_grave_gagne() {
        let ds = vec![
            diag(0, 0, 0, 1, Severity::Warning),
            diag(0, 2, 0, 3, Severity::Error),
            diag(1, 0, 1, 1, Severity::Info),
        ];
        let mut s = diagnostique("abcd\nef", ds);
        let b = draw(&s);
        let t = Theme::dark();
        let g = |b: &TestBackend, y| b.buffer()[(X - 1, Y + y)].clone();
        assert_eq!((g(&b, 0).symbol(), g(&b, 0).fg), ("✖", t.error));
        assert_eq!((g(&b, 1).symbol(), g(&b, 1).fg), ("●", t.muted));
        s.settings.nerd_icons = false;
        let b = draw(&s);
        assert_eq!(g(&b, 0).symbol(), "E");
        assert_eq!(g(&b, 1).symbol(), "i");
    }

    #[test]
    fn marques_git_suivent_le_repli_et_sans_numeros() {
        let mut s = state(Document::from_text(
            Some("a.rs".into()),
            "fn a() {\n    x();\n}\nb\nc\n",
        ));
        s.git_marks = vec![
            Some(GitMark::Modified),
            Some(GitMark::Added),
            None,
            Some(GitMark::DeletedBelow),
            Some(GitMark::DeletedAbove),
        ];
        let t = Theme::dark();
        let g = |b: &TestBackend, y| b.buffer()[(30, Y + y)].clone();
        let b = draw(&s);
        assert_eq!((g(&b, 0).symbol(), g(&b, 0).fg), ("▎", t.git_modified));
        assert_eq!((g(&b, 1).symbol(), g(&b, 1).fg), ("▎", t.git_added));
        assert_eq!(g(&b, 2).symbol(), " ");
        assert_eq!((g(&b, 3).symbol(), g(&b, 3).fg), ("▁", t.git_deleted));
        assert_eq!(g(&b, 4).symbol(), "▔");
        // Corps du bloc replié (ligne 1, ajoutée) masqué : `}` puis `b` et sa marque remontent d'une rangée.
        s.doc.toggle_fold(0);
        s.settings.line_numbers = false;
        let b = draw(&s);
        assert_eq!(g(&b, 1).symbol(), " ");
        assert_eq!(g(&b, 2).symbol(), "▁");
        // Sans numéros : marque git, repli, diagnostic, puis le texte.
        let row: String = (30..36).map(|x| b.buffer()[(x, Y)].symbol()).collect();
        assert_eq!(row, "▎+ fn ");
    }

    fn item(label: &str, kind: &str, detail: Option<&str>) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            kind: Some(kind.into()),
            detail: detail.map(Into::into),
            insert_text: label.into(),
            cursor: None,
            replace_start: None,
        }
    }

    /// 40 lignes « let x = io::st », curseur en fin de ligne `line`, complétion sur « st ».
    fn complete(line: usize, items: Vec<CompletionItem>, selected: usize) -> State {
        let mut s = state(Document::from_text(None, &"let x = io::st\n".repeat(40)));
        s.doc.move_down(line, false);
        s.doc.move_line_end(false);
        let anchor = s.doc.cursor() - 2;
        s.completion = Some(ide_core::Completion {
            items,
            selected,
            anchor,
            path: None,
            incomplete: false,
        });
        s
    }

    /// Symboles des cellules `xs` de la ligne écran `y`.
    fn cells(b: &TestBackend, y: u16, xs: Range<u16>) -> String {
        xs.map(|x| b.buffer()[(x, y)].symbol()).collect()
    }

    #[test]
    fn popup_sous_le_curseur() {
        let items = vec![
            item("stdin", "fn", Some("fn() -> Stdin")),
            item("stdout_long", "fn", None),
            // Filtrée par « st » (le mot tapé) : « xst » reste visible, en dernier.
            item("xst", "struct", None),
        ];
        let b = draw(&complete(0, items, 1));
        // Bord gauche une cellule avant `st` (colonne 12) ; largeur 6 + 1 + 11 + 2 = 20.
        let x = X + 12 - 1;
        assert_eq!(cells(&b, Y + 1, x..x + 20), format!("┌{}┐", "─".repeat(18)));
        // Détail tronqué, calé à droite dans la place restante.
        assert_eq!(cells(&b, Y + 2, x..x + 20), "│fn     stdin fn() │");
        assert_eq!(cells(&b, Y + 3, x..x + 20), "│fn     stdout_long│");
        assert_eq!(cells(&b, Y + 4, x..x + 20), "│struct xst        │");
        assert_eq!(cells(&b, Y + 5, x..x + 20), format!("└{}┘", "─".repeat(18)));
        // Sélection surlignée, kind estompé.
        let t = Theme::dark();
        assert!(a_style(&b.buffer()[(x + 8, Y + 3)], t.explorer_selected));
        assert_eq!(b.buffer()[(x + 1, Y + 2)].fg, t.muted);
        assert_ne!(
            b.buffer()[(x + 8, Y + 2)].bg,
            t.explorer_selected.bg.expect("fond")
        );
    }

    #[test]
    fn popup_au_dessus_en_bas_d_ecran() {
        // Ligne 20 = ligne écran 21, zone texte jusqu'à 22 : pas la place dessous.
        let b = draw(&complete(20, vec![item("stdin", "fn", None)], 0));
        let x = X + 11;
        assert_eq!(cells(&b, 18, x..x + 3), "┌──");
        assert_eq!(cells(&b, 19, x..x + 9), "│fn stdin");
        assert_eq!(cells(&b, 20, x..x + 3), "└──");
        assert_eq!(cells(&b, 21, x..x + 3), ":st");
    }

    #[test]
    fn popup_defile_vers_la_selection() {
        let items = (0..15)
            .map(|i| item(&format!("st{i:02}"), "fn", None))
            .collect();
        let b = draw(&complete(0, items, 12));
        let x = X + 11;
        // 10 lignes visibles : 03..=12, la sélection en dernier.
        assert_eq!(cells(&b, Y + 2, x..x + 8), "│fn st03");
        assert_eq!(cells(&b, Y + 11, x..x + 8), "│fn st12");
        assert_eq!(cells(&b, Y + 12, x..x + 3), "└──");
        assert!(a_style(
            &b.buffer()[(x + 4, Y + 11)],
            Theme::dark().explorer_selected
        ));
    }

    #[test]
    fn infobulle_au_dessus_sans_clotures() {
        let mut s = state(Document::from_text(None, &"abc\n".repeat(10)));
        s.doc.move_down(5, false);
        let at = s.doc.cursor();
        s.hover = Some(ide_core::Hover {
            text: "```rust\nfn main()\n```\nDoc".into(),
            at,
        });
        let b = draw(&s);
        // Deux lignes de texte + bords, juste au-dessus de la ligne 5 (écran 6) ; `Clear` masque le texte.
        let x = X - 1;
        assert_eq!(cells(&b, Y + 1, x..x + 11), "┌─────────┐");
        assert_eq!(cells(&b, Y + 2, x..x + 11), "│fn main()│");
        assert_eq!(cells(&b, Y + 3, x..x + 11), "│Doc      │");
        assert_eq!(cells(&b, Y + 4, x..x + 11), "└─────────┘");
        // En haut d'écran : dessous.
        s.doc.set_cursor(0, false);
        s.hover = Some(ide_core::Hover {
            text: "Doc".into(),
            at: 0,
        });
        let b = draw(&s);
        assert_eq!(cells(&b, Y + 2, x..x + 5), "│Doc│");
    }

    #[test]
    fn panneau_problemes() {
        let mut s = diagnostique(
            &"let x = io::st;\n".repeat(30),
            vec![
                diag(3, 12, 3, 14, Severity::Error),
                diag(0, 4, 0, 5, Severity::Warning),
            ],
        );
        s.diagnostics.insert(
            "/ailleurs/b.rs".into(),
            vec![diag(9, 0, 9, 1, Severity::Info)],
        );
        s.explorer.root = Some("/p".into());
        s.problems_open = true;
        s.lsp_status = "rust-analyzer ✓".into();
        let b = draw(&s);
        insta::assert_snapshot!(b);
        // La zone texte perd la hauteur du panneau : 14 lignes, la dernière en écran 14.
        assert_eq!(b.buffer()[(X - 4, 14)].symbol(), "4");
        assert!(ligne(&b, 15)[..].contains("│ PROBLÈMES (3) ─"));
        let vide = draw(&State {
            problems_open: true,
            ..state(Document::from_text(None, ""))
        });
        assert!(ligne(&vide, 16).contains("Aucun problème"));
    }

    #[test]
    fn barre_de_statut_compteurs_et_serveur() {
        let mut s = diagnostique(
            "abc",
            vec![
                diag(0, 0, 0, 1, Severity::Error),
                diag(0, 1, 0, 2, Severity::Error),
                diag(0, 2, 0, 3, Severity::Hint),
            ],
        );
        s.lsp_status = "rust-analyzer ✓".into();
        let b = draw(&s);
        let l = ligne(&b, 23);
        assert!(
            l.starts_with(" F1 Raccourcis  ✖ 2 ⚠ 0 | a.rs | Prê rust-analyzer"),
            "{l}"
        );
        assert!(l.contains("rust-analyzer ✓ | Ln 1, Col 1 | LF"), "{l}");
        s.settings.nerd_icons = false;
        assert!(ligne(&draw(&s), 23).starts_with(" F1 Raccourcis  E 2 W 0 | a.rs"));
        // Sans diagnostic ni serveur : barre inchangée.
        let l = ligne(&draw(&state(Document::from_text(None, ""))), 23);
        assert!(l.starts_with(" F1 Raccourcis  [sans nom] | Prêt"), "{l}");
        assert!(l.contains("Ln 1, Col 1 | LF") && !l.contains("| Ln"), "{l}");
    }

    #[test]
    fn barre_de_statut_branche() {
        let mut s = diagnostique("abc", vec![diag(0, 0, 0, 1, Severity::Error)]);
        s.git_branch = Some("main".into());
        let l = ligne(&draw(&s), 23);
        assert!(
            l.starts_with(" F1 Raccourcis  \u{e725} main | ✖ 1 ⚠ 0 | a.rs | Prêt"),
            "{l}"
        );
        s.settings.nerd_icons = false;
        let l = ligne(&draw(&s), 23);
        assert!(
            l.starts_with(" F1 Raccourcis  main | E 1 W 0 | a.rs"),
            "{l}"
        );
        // Dossier du projet avant la branche ; rappel quand l'explorateur a le focus.
        s.explorer.root = Some("/home/moi/IDE".into());
        s.focus = ide_core::Focus::Explorer;
        let b = draw(&s);
        let l = ligne(&b, 23);
        assert!(
            l.starts_with(" F1 Raccourcis  Échap/Ctrl+E : retour au code | IDE | "),
            "{l}"
        );
        let accent = Theme::dark().search_current.bg.expect("fond");
        assert_eq!(b.buffer()[(1, 23)].bg, accent);
        assert_ne!(b.buffer()[(16, 23)].bg, accent);
    }

    #[test]
    fn barre_de_statut_etroite() {
        // F1 reste visible, rien ne panique, même plus étroit que le badge.
        for w in [1, 10, 20, 40] {
            let s = State {
                width: w,
                ..state(Document::from_text(Some("a.rs".into()), "x"))
            };
            let mut t = Terminal::new(TestBackend::new(w, 24)).unwrap();
            t.draw(|f| render_with(&s, f, &Theme::dark())).unwrap();
            let l: String = (0..w)
                .map(|x| t.backend().buffer()[(x, 23)].symbol())
                .collect();
            assert!(" F1 Raccourcis ".starts_with(&l[..l.len().min(15)]), "{l}");
        }
    }

    fn cellules(text: &str) -> TermSpan {
        TermSpan {
            text: text.into(),
            ..TermSpan::default()
        }
    }

    /// Terminal ouvert avec le focus, curseur en fin de seconde ligne de la grille.
    fn terminal_ouvert() -> State {
        let mut s = state(Document::from_text(
            Some("a.rs".into()),
            &"foo bar foo\n".repeat(40),
        ));
        s.focus = Focus::Terminal;
        s.terminal.open = true;
        s.terminal.lines = vec![vec![cellules("$ ls")], vec![cellules("$ ")]];
        s.terminal.cursor = Some((1, 2));
        s
    }

    #[test]
    fn panneau_terminal() {
        let s = terminal_ouvert();
        let b = draw(&s);
        insta::assert_snapshot!(b);
        // 24 lignes : 11 pour le panneau, juste au-dessus de la barre de statut, large comme l'éditeur.
        assert!(ligne(&b, 12)[..].contains("│ TERMINAL ─"));
        assert!(ligne(&b, 13)[..].contains("│$ ls "));
        assert_eq!(b.cursor_position(), Position::new(30 + 2, 12 + 1 + 1));
        // Fermé : rien n'est dessiné, même avec une grille restante.
        let mut ferme = terminal_ouvert();
        ferme.terminal.open = false;
        ferme.focus = Focus::Editor;
        assert!(!(0..24).any(|y| ligne(&draw(&ferme), y).contains("TERMINAL")));
    }

    #[test]
    fn terminal_bordure_selon_le_focus() {
        let t = Theme::dark();
        let mut s = terminal_ouvert();
        let b = draw(&s);
        assert!(a_style(&b.buffer()[(30, 12)], t.border_focus));
        assert!(a_style(&b.buffer()[(35, 12)], t.border_focus), "titre");
        assert!(a_style(&b.buffer()[(79, 12)], t.border_focus));
        s.focus = Focus::Editor;
        let b = draw(&s);
        assert_eq!(b.buffer()[(30, 12)].fg, t.border);
        assert_eq!(b.buffer()[(79, 12)].fg, t.border);
    }

    #[test]
    fn hauteur_du_texte_egale_a_editor_size() {
        let recherche_ouverte = recherche(None, 0).search;
        for (problemes, barre, bandeau) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            let mut s = terminal_ouvert();
            s.problems_open = problemes;
            if barre {
                s.search = recherche_ouverte.clone();
            }
            if bandeau {
                s.backup_offer = Some(("/x/a.rs".into(), "copie".into()));
            }
            let b = draw(&s);
            let rendues = (0..24).filter(|&y| ligne(&b, y).contains("foo bar foo"));
            assert_eq!(
                rendues.count(),
                ide_core::editor_size(&s).1 as usize,
                "{problemes} {barre} {bandeau}"
            );
            // La barre de recherche reste sur la zone éditeur, le panneau colle à la barre de statut.
            let titre = 23 - terminal_height(&s);
            assert!(ligne(&b, titre).contains(" TERMINAL "), "{titre}");
        }
    }

    #[test]
    fn problemes_et_terminal_ouverts_ensemble() {
        let mut s = terminal_ouvert();
        s.problems_open = true;
        let b = draw(&s);
        // 7 lignes de terminal (lignes 16 à 22) sous 8 de Problèmes (8 à 15).
        assert_eq!(terminal_height(&s), 7);
        assert!(ligne(&b, 8).contains(" PROBLÈMES (0) "));
        assert!(ligne(&b, 9).contains("Aucun problème"));
        assert!(ligne(&b, 16).contains(" TERMINAL "));
        assert!(ligne(&b, 17).contains("$ ls"));
        assert_eq!(b.cursor_position(), Position::new(32, 18));
    }

    #[test]
    fn curseur_du_terminal() {
        let mut s = terminal_ouvert();
        s.help = Some(0);
        assert_eq!(
            draw(&s).cursor_position(),
            Position::new(0, 0),
            "sous l'aide"
        );
        s.help = None;
        // Grille en retard sur la zone : ligne ou colonne hors du panneau.
        for hors_zone in [(10, 0), (0, 50), (u16::MAX, u16::MAX)] {
            s.terminal.cursor = Some(hors_zone);
            assert_eq!(draw(&s).cursor_position(), Position::new(0, 0));
        }
        // Dernière cellule de la grille.
        s.terminal.cursor = Some((9, 49));
        assert_eq!(draw(&s).cursor_position(), Position::new(79, 22));
        s.terminal.cursor = None;
        assert_eq!(draw(&s).cursor_position(), Position::new(0, 0));
        // Panneau ouvert sans le focus : le curseur reste celui de l'éditeur.
        s.terminal.cursor = Some((1, 2));
        s.focus = Focus::Editor;
        assert_eq!(draw(&s).cursor_position(), Position::new(X, Y));
    }

    #[test]
    fn barre_de_statut_focus_terminal() {
        let mut s = terminal_ouvert();
        let l = ligne(&draw(&s), 23);
        assert!(
            l.starts_with(" F1 Raccourcis  Ctrl+ù/Alt+J : fermer le terminal | a."),
            "{l}"
        );
        s.focus = Focus::Editor;
        let l = ligne(&draw(&s), 23);
        assert!(l.starts_with(" F1 Raccourcis  a.rs"), "{l}");
    }

    #[test]
    fn terminal_couleurs_et_inverse() {
        let mut s = terminal_ouvert();
        s.terminal.lines = vec![vec![
            cellules("a"),
            TermSpan {
                fg: TermColor::Idx(3),
                bg: TermColor::Rgb(1, 2, 3),
                ..cellules("b")
            },
            TermSpan {
                inverse: true,
                ..cellules("c")
            },
            TermSpan {
                bold: true,
                italic: true,
                underline: true,
                ..cellules("d")
            },
        ]];
        let b = draw(&s);
        let t = Theme::dark();
        let cell = |x: u16| &b.buffer()[(30 + x, 13)];
        assert_eq!((cell(0).fg, cell(0).bg), (t.foreground, t.background));
        assert!(cell(0).modifier.is_empty());
        assert_eq!(
            (cell(1).fg, cell(1).bg),
            (Color::Indexed(3), Color::Rgb(1, 2, 3))
        );
        assert_eq!(cell(2).modifier, Modifier::REVERSED);
        assert_eq!((cell(2).fg, cell(2).bg), (t.foreground, t.background));
        assert_eq!(
            cell(3).modifier,
            Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED
        );
        // Reste du panneau : fond de l'éditeur.
        assert_eq!(cell(4).bg, t.background);
        assert_eq!(b.buffer()[(30, 22)].bg, t.background);
    }

    #[test]
    fn ligne_de_grille_trop_longue_coupee_sans_retour() {
        let mut s = terminal_ouvert();
        s.terminal.lines = vec![vec![cellules(&"日".repeat(100))], vec![cellules("suite")]];
        let b = draw(&s);
        // 25 caractères doubles dans les 50 colonnes, le reste n'arrive pas sur la ligne suivante.
        assert_eq!(ligne(&b, 13).matches('日').count(), 25);
        assert!(ligne(&b, 14).contains("│suite "));
    }

    #[test]
    fn terminal_sur_ecran_minuscule() {
        for (w, h) in [(1, 1), (5, 2), (5, 3), (30, 4), (80, 5), (80, 9), (80, 12)] {
            for problemes in [false, true] {
                let mut s = terminal_ouvert();
                s.width = w;
                s.height = h;
                s.problems_open = problemes;
                s.terminal.lines[0] = vec![cellules(&"x".repeat(300))];
                s.terminal.cursor = Some((0, 0));
                let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
                t.draw(|f| render_with(&s, f, &Theme::dark())).unwrap();
                if terminal_height(&s) == 0 {
                    // Panneau sans hauteur : pas de curseur fantôme.
                    assert_eq!(t.backend().cursor_position(), Position::new(0, 0));
                }
            }
        }
    }
}
