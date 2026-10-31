//! Thèmes (F-31) : couleurs de l'interface et de la coloration syntaxique, sombre (« Dark+ »)
//! et clair (« Light+ ») embarqués dans le binaire.
//!
//! Les catégories de coloration sont désignées par le nom en minuscules de la variante
//! `ide_syntax::Highlight` correspondante (`Highlight::Keyword` → `"keyword"`,
//! `Highlight::Punctuation` → `"punctuation"`…), pour ne pas dépendre de `ide-syntax`.

use ratatui::style::{Color, Modifier, Style};

const fn hex(c: u32) -> Color {
    Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

const fn fg(c: u32) -> Style {
    Style::new().fg(hex(c))
}

const fn bg(c: u32) -> Style {
    Style::new().bg(hex(c))
}

/// Couleurs de l'interface et de la coloration.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub background: Color,
    pub foreground: Color,
    pub current_line: Color,
    pub selection: Style,
    pub search_match: Style,
    pub search_current: Style,
    pub gutter: Color,
    pub gutter_current: Style,
    pub border: Color,
    pub border_focus: Style,
    pub explorer_selected: Style,
    pub explorer_selected_unfocused: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
    pub status_bar: Style,
    pub error: Color,
    pub warning: Color,
    pub muted: Color,
    /// Couleur par catégorie de coloration, indexée par le nom de catégorie en minuscules (`keyword`, `function`…).
    syntax: Vec<(&'static str, Style)>,
}

impl Theme {
    /// Thème sombre, d'après « Dark+ » de VSCode.
    pub fn dark() -> Self {
        Self {
            name: "dark",
            background: hex(0x1e1e1e),
            foreground: hex(0xd4d4d4),
            current_line: hex(0x2a2d2e),
            selection: bg(0x264f78),
            search_match: bg(0x613315).add_modifier(Modifier::UNDERLINED),
            search_current: fg(0x1e1e1e).bg(hex(0xe2c08d)),
            gutter: hex(0x858585),
            gutter_current: fg(0xc6c6c6).add_modifier(Modifier::BOLD),
            border: hex(0x454545),
            border_focus: fg(0x007fd4).add_modifier(Modifier::BOLD),
            explorer_selected: fg(0xffffff).bg(hex(0x04395e)),
            explorer_selected_unfocused: bg(0x37373d),
            tab_active: fg(0xffffff).bg(hex(0x1e1e1e)).add_modifier(Modifier::BOLD),
            tab_inactive: fg(0x969696).bg(hex(0x2d2d2d)),
            status_bar: fg(0xffffff).bg(hex(0x007acc)),
            error: hex(0xf48771),
            warning: hex(0xcca700),
            muted: hex(0x808080),
            syntax: vec![
                ("keyword", fg(0x569cd6)),
                ("function", fg(0xdcdcaa)),
                ("type", fg(0x4ec9b0)),
                ("string", fg(0xce9178)),
                ("number", fg(0xb5cea8)),
                ("comment", fg(0x6a9955).add_modifier(Modifier::ITALIC)),
                ("constant", fg(0x4fc1ff)),
                ("variable", fg(0x9cdcfe)),
                ("property", fg(0x9cdcfe)),
                ("operator", fg(0xd4d4d4)),
                ("punctuation", fg(0x9a9a9a)),
                ("attribute", fg(0xc586c0)),
                ("tag", fg(0x569cd6)),
                ("label", fg(0xc8c8c8)),
                ("namespace", fg(0x4ec9b0)),
                ("macro", fg(0xc586c0)),
                ("escape", fg(0xd7ba7d)),
                ("heading", fg(0x569cd6).add_modifier(Modifier::BOLD)),
                ("link", fg(0x4fc1ff).add_modifier(Modifier::UNDERLINED)),
                ("emphasis", Style::new().add_modifier(Modifier::ITALIC)),
            ],
        }
    }

    /// Thème clair, d'après « Light+ » de VSCode.
    pub fn light() -> Self {
        Self {
            name: "light",
            background: hex(0xffffff),
            foreground: hex(0x000000),
            current_line: hex(0xeeeeee),
            selection: bg(0xadd6ff),
            search_match: bg(0xf8c9ab).add_modifier(Modifier::UNDERLINED),
            search_current: fg(0x000000).bg(hex(0xf3c500)),
            gutter: hex(0x237893),
            gutter_current: fg(0x0b216f).add_modifier(Modifier::BOLD),
            border: hex(0xc8c8c8),
            border_focus: fg(0x0090f1).add_modifier(Modifier::BOLD),
            explorer_selected: fg(0xffffff).bg(hex(0x0060c0)),
            explorer_selected_unfocused: bg(0xe4e6f1),
            tab_active: fg(0x333333).bg(hex(0xffffff)).add_modifier(Modifier::BOLD),
            tab_inactive: fg(0x616161).bg(hex(0xececec)),
            status_bar: fg(0xffffff).bg(hex(0x007acc)),
            error: hex(0xe51400),
            warning: hex(0x9a6700),
            muted: hex(0x6e6e6e),
            syntax: vec![
                ("keyword", fg(0x0000ff)),
                ("function", fg(0x795e26)),
                ("type", fg(0x267f99)),
                ("string", fg(0xa31515)),
                ("number", fg(0x098658)),
                ("comment", fg(0x008000).add_modifier(Modifier::ITALIC)),
                ("constant", fg(0x0070c1)),
                ("variable", fg(0x001080)),
                ("property", fg(0x001080)),
                ("operator", fg(0x000000)),
                ("punctuation", fg(0x5a5a5a)),
                ("attribute", fg(0xe50000)),
                ("tag", fg(0x800000)),
                ("label", fg(0x811f3f)),
                ("namespace", fg(0x267f99)),
                ("macro", fg(0xaf00db)),
                ("escape", fg(0xee0000)),
                ("heading", fg(0x800000).add_modifier(Modifier::BOLD)),
                ("link", fg(0x0070c1).add_modifier(Modifier::UNDERLINED)),
                ("emphasis", Style::new().add_modifier(Modifier::ITALIC)),
            ],
        }
    }

    /// Thème embarqué par son nom : `"dark"` ou `"light"`.
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }

    /// Style d'une catégorie (`"keyword"`, `"string"`…) ; `Style::default()` si inconnue.
    pub fn syntax(&self, category: &str) -> Style {
        self.syntax
            .iter()
            .find(|(c, _)| *c == category)
            .map_or_else(Style::default, |&(_, s)| s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Miroir de `ide_syntax::Highlight`, en minuscules.
    const CATEGORIES: [&str; 20] = [
        "keyword",
        "function",
        "type",
        "string",
        "number",
        "comment",
        "constant",
        "variable",
        "property",
        "operator",
        "punctuation",
        "attribute",
        "tag",
        "label",
        "namespace",
        "macro",
        "escape",
        "heading",
        "link",
        "emphasis",
    ];

    fn themes() -> [Theme; 2] {
        [Theme::dark(), Theme::light()]
    }

    /// Ratio de contraste WCAG 2 entre deux couleurs RGB.
    fn contrast(a: Color, b: Color) -> f64 {
        fn lum(c: Color) -> f64 {
            let Color::Rgb(r, g, b) = c else {
                panic!("couleur non RGB : {c:?}")
            };
            let lin = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        }
        let (a, b) = (lum(a), lum(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn contraste_wcag_connu() {
        let r = contrast(hex(0x000000), hex(0xffffff));
        assert!((r - 21.0).abs() < 1e-9, "{r}");
        assert!((contrast(hex(0x777777), hex(0x777777)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn toutes_les_categories_definies() {
        for t in themes() {
            assert_eq!(t.syntax.len(), CATEGORIES.len(), "{}", t.name);
            for c in CATEGORIES {
                assert_ne!(t.syntax(c), Style::default(), "{} : {c}", t.name);
            }
        }
    }

    #[test]
    fn par_nom() {
        assert_eq!(Theme::by_name("dark"), Some(Theme::dark()));
        assert_eq!(Theme::by_name("light"), Some(Theme::light()));
        assert_eq!(Theme::by_name("Dark"), None);
        assert_eq!(Theme::by_name(""), None);
    }

    #[test]
    fn categorie_inconnue_style_par_defaut() {
        assert_eq!(Theme::dark().syntax("inconnue"), Style::default());
        assert_eq!(Theme::light().syntax("Keyword"), Style::default());
    }

    #[test]
    fn contrastes_suffisants() {
        for t in themes() {
            let n = t.name;
            let text = |s: Style| s.fg.unwrap_or(t.foreground);
            let back = |s: Style| s.bg.unwrap_or(t.background);
            // Texte normal ≥ 4,5:1.
            for b in [t.background, t.current_line, back(t.selection)] {
                let r = contrast(t.foreground, b);
                assert!(r >= 4.5, "{n} : texte sur {b:?} = {r:.2}");
            }
            for s in [
                t.search_current,
                t.explorer_selected,
                t.explorer_selected_unfocused,
                t.tab_active,
                t.tab_inactive,
                t.status_bar,
            ] {
                let r = contrast(text(s), back(s));
                assert!(r >= 4.5, "{n} : {s:?} = {r:.2}");
            }
            // Syntaxe et éléments secondaires ≥ 3:1, sur le fond et sur la ligne courante.
            let secondary = CATEGORIES.iter().map(|c| (*c, text(t.syntax(c)))).chain([
                ("gutter", t.gutter),
                ("gutter_current", text(t.gutter_current)),
                ("error", t.error),
                ("warning", t.warning),
                ("muted", t.muted),
            ]);
            for (c, f) in secondary {
                for b in [t.background, t.current_line] {
                    let r = contrast(f, b);
                    assert!(r >= 3.0, "{n} : {c} sur {b:?} = {r:.2}");
                }
            }
        }
    }
}
