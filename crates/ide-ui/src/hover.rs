//! Infobulle : le markdown des serveurs (clangd, rust-analyzer, pyright…) mis en forme pour le terminal,
//! libellés connus traduits en français. La documentation elle-même reste telle que le serveur l'envoie.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::theme::Theme;

/// Nature du symbole dans le titre `### function `nom`` (clangd).
const KINDS: &[(&str, &str)] = &[
    ("function", "fonction"),
    ("instance-method", "méthode"),
    ("static-method", "méthode statique"),
    ("method", "méthode"),
    ("constructor", "constructeur"),
    ("destructor", "destructeur"),
    ("variable", "variable"),
    ("param", "paramètre"),
    ("parameter", "paramètre"),
    ("field", "champ"),
    ("property", "propriété"),
    ("macro", "macro"),
    ("struct", "structure"),
    ("union", "union"),
    ("class", "classe"),
    ("enum", "énumération"),
    ("enumerator", "valeur d'énumération"),
    ("typedef", "alias de type"),
    ("type-alias", "alias de type"),
    ("type", "type"),
    ("namespace", "espace de noms"),
    ("module", "module"),
];

/// Débuts de ligne connus (clangd, Doxygen).
const LABELS: &[(&str, &str)] = &[
    ("→ ", "Renvoie : "),
    ("Parameters:", "Paramètres :"),
    ("Type: ", "Type : "),
    ("Value = ", "Valeur = "),
    ("Size: ", "Taille : "),
    ("Offset: ", "Décalage : "),
    ("Passed by value", "Passé par valeur"),
    ("Passed by const reference", "Passé par référence constante"),
    ("Passed by reference", "Passé par référence"),
    ("Passed as ", "Passé comme "),
    ("// In ", "// Dans "),
    ("@brief ", ""),
    ("@param ", "Paramètre "),
    ("@return ", "Renvoie : "),
    ("@returns ", "Renvoie : "),
];

/// Ligne logique : morceaux (texte, style), avant découpe à la largeur.
type Styled = Vec<(String, Style)>;

/// `text` (markdown) → lignes stylées d'au plus `max` colonnes.
pub(crate) fn lines(text: &str, theme: &Theme, max: usize) -> Vec<Line<'static>> {
    let plain = Style::new().fg(theme.foreground);
    let code = plain.patch(theme.syntax("type"));
    let muted = Style::new().fg(theme.muted);
    let mut out: Vec<Option<Styled>> = Vec::new(); // `None` = séparateur
    let mut in_code = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(Some(vec![(translate(line).replace('\t', "    "), code)]));
            continue;
        }
        let line = line.replace("\\_", "_").replace("\\*", "*");
        if matches!(line.trim(), "---" | "***" | "___") {
            out.push(None);
        } else if let Some(title) = line.trim_start().strip_prefix('#') {
            let title = title.trim_start_matches('#').trim();
            let (kind, rest) = title.split_once(' ').unwrap_or(("", title));
            match KINDS.iter().find(|(en, _)| *en == kind) {
                Some((_, fr)) => {
                    let mut l = vec![(format!("{fr} "), muted)];
                    l.extend(inline(rest, plain.add_modifier(Modifier::BOLD), code));
                    out.push(Some(l));
                }
                None => out.push(Some(inline(
                    title,
                    plain.add_modifier(Modifier::BOLD),
                    code,
                ))),
            }
        } else {
            let line = match line.strip_prefix("- ") {
                Some(item) => format!("• {item}"),
                None => line,
            };
            out.push(Some(inline(&translate(&line), plain, code)));
        }
    }
    // Pas de séparateur ni de ligne vide en tête, en fin, ou en double.
    let blank = |l: &Option<Styled>| {
        l.as_ref()
            .is_none_or(|s| s.iter().all(|(t, _)| t.trim().is_empty()))
    };
    let mut tidy: Vec<Option<Styled>> = Vec::new();
    for l in out {
        let sep = l.is_none();
        if blank(&l) && tidy.last().is_none_or(blank) {
            // Un séparateur remplace une ligne vide qui le précède.
            if sep && let Some(last) = tidy.last_mut().filter(|x| x.is_some()) {
                *last = None;
            }
            continue;
        }
        tidy.push(l);
    }
    while tidy.last().is_some_and(blank) {
        tidy.pop();
    }
    let rule = Style::new().fg(theme.border);
    let width = tidy
        .iter()
        .flatten()
        .map(|s| s.iter().map(|(t, _)| text_width(t)).sum::<usize>())
        .max()
        .unwrap_or(0)
        .min(max);
    tidy.into_iter()
        .flat_map(|l| match l {
            None => vec![Line::from(Span::styled("─".repeat(width), rule))],
            Some(s) => wrap(s, max),
        })
        .collect()
}

fn translate(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    match LABELS.iter().find(|(en, _)| trimmed.starts_with(en)) {
        Some((en, fr)) => format!("{indent}{fr}{}", &trimmed[en.len()..]),
        None => line.to_owned(),
    }
}

/// `code`, **gras**, liens `[texte](url)` → morceaux stylés sans la syntaxe markdown.
fn inline(s: &str, base: Style, code: Style) -> Styled {
    let mut out = Vec::new();
    for (i, part) in s.split('`').enumerate() {
        if part.is_empty() {
            continue;
        }
        if i % 2 == 1 {
            out.push((part.to_owned(), code));
        } else {
            out.push((strip_markup(part), base));
        }
    }
    out
}

fn strip_markup(s: &str) -> String {
    let mut s = s.replace("**", "");
    // [texte](url) → texte
    while let Some(open) = s.find('[') {
        let Some(mid) = s[open..].find("](").map(|m| open + m) else {
            break;
        };
        let Some(close) = s[mid..].find(')').map(|c| mid + c) else {
            break;
        };
        s = format!("{}{}{}", &s[..open], &s[open + 1..mid], &s[close + 1..]);
    }
    s
}

fn text_width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Découpe une ligne stylée en lignes d'au plus `max` colonnes (une ligne vide reste une ligne).
fn wrap(line: Styled, max: usize) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut w = 0;
    for (text, style) in line {
        let mut cur = String::new();
        for c in text.chars() {
            let cw = c.width().unwrap_or(0);
            if w + cw > max && w > 0 {
                if !cur.is_empty() {
                    row.push(Span::styled(std::mem::take(&mut cur), style));
                }
                rows.push(Line::from(std::mem::take(&mut row)));
                w = 0;
            }
            cur.push(c);
            w += cw;
        }
        if !cur.is_empty() {
            row.push(Span::styled(cur, style));
        }
    }
    rows.push(Line::from(row));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texte(text: &str, max: usize) -> Vec<String> {
        lines(text, &Theme::dark(), max)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn clangd_traduit_et_mis_en_forme() {
        let md = "### function `ajoute`  \n\n---\n→ `int`  \nParameters:  \n- `int a`\n\nAjoute deux nombres.  \n@param a premier  \n\n---\n```cpp\n// In main\nstatic int ajoute(int a)\n```";
        assert_eq!(
            texte(md, 80),
            [
                "fonction ajoute",
                "────────────────────────",
                "Renvoie : int",
                "Paramètres :",
                "• int a",
                "",
                "Ajoute deux nombres.",
                "Paramètre a premier",
                "────────────────────────",
                "// Dans main",
                "static int ajoute(int a)",
            ]
        );
    }

    #[test]
    fn liens_echappements_et_largeur() {
        assert_eq!(
            texte("voir [Vec](https://x) et a\\_b", 80),
            ["voir Vec et a_b"]
        );
        assert_eq!(texte("abcdef", 4), ["abcd", "ef"]);
        assert!(texte("\n---\n\n", 10).is_empty());
    }
}
