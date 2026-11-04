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
    /// Parenthèse sous le curseur et son vis-à-vis.
    pub bracket_match: Style,
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
    /// Marques git de la gouttière : lignes ajoutées, modifiées, supprimées.
    pub git_added: Color,
    pub git_modified: Color,
    pub git_deleted: Color,
    /// Couleur par catégorie de coloration, indexée par le nom de catégorie en minuscules (`keyword`, `function`…).
    syntax: Vec<(&'static str, Style)>,
}

/// Couleurs de base d'un thème, voir `Theme::from_palette`.
struct Palette {
    name: &'static str,
    bg: u32,
    panel: u32,
    selection: u32,
    fg: u32,
    comment: u32,
    border: u32,
    accent: u32,
    on_accent: u32,
    red: u32,
    yellow: u32,
    green: u32,
    blue: u32,
    keyword: u32,
    function: u32,
    kind: u32,
    string: u32,
    number: u32,
    constant: u32,
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
            bracket_match: bg(0x515c6a).add_modifier(Modifier::BOLD),
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
            git_added: hex(0x81b88b),
            git_modified: hex(0x4fa7e6),
            git_deleted: hex(0xf14c4c),
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
            bracket_match: bg(0xb9d1c4).add_modifier(Modifier::BOLD),
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
            git_added: hex(0x48985d),
            git_modified: hex(0x2090d3),
            git_deleted: hex(0xe51400),
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

    /// Thème sombre « Gruvbox » (palette de Pavel Pertsev, licence MIT).
    pub fn gruvbox() -> Self {
        Self {
            name: "gruvbox",
            background: hex(0x282828),
            foreground: hex(0xebdbb2),
            current_line: hex(0x32302f),
            selection: bg(0x504945),
            search_match: bg(0x665c54).add_modifier(Modifier::UNDERLINED),
            search_current: fg(0x282828).bg(hex(0xfabd2f)),
            bracket_match: bg(0x665c54).add_modifier(Modifier::BOLD),
            gutter: hex(0x928374),
            gutter_current: fg(0xfabd2f).add_modifier(Modifier::BOLD),
            border: hex(0x504945),
            border_focus: fg(0x83a598).add_modifier(Modifier::BOLD),
            explorer_selected: fg(0xfbf1c7).bg(hex(0x504945)),
            explorer_selected_unfocused: bg(0x3c3836),
            tab_active: fg(0xfbf1c7).bg(hex(0x282828)).add_modifier(Modifier::BOLD),
            tab_inactive: fg(0xa89984).bg(hex(0x1d2021)),
            status_bar: fg(0x282828).bg(hex(0xa89984)),
            error: hex(0xfb4934),
            warning: hex(0xfabd2f),
            muted: hex(0x928374),
            git_added: hex(0xb8bb26),
            git_modified: hex(0x83a598),
            git_deleted: hex(0xfb4934),
            syntax: vec![
                ("keyword", fg(0xfb4934)),
                ("function", fg(0xb8bb26)),
                ("type", fg(0xfabd2f)),
                ("string", fg(0xb8bb26)),
                ("number", fg(0xd3869b)),
                ("comment", fg(0x928374).add_modifier(Modifier::ITALIC)),
                ("constant", fg(0xd3869b)),
                ("variable", fg(0xebdbb2)),
                ("property", fg(0x83a598)),
                ("operator", fg(0xfe8019)),
                ("punctuation", fg(0xa89984)),
                ("attribute", fg(0x8ec07c)),
                ("tag", fg(0x8ec07c)),
                ("label", fg(0x83a598)),
                ("namespace", fg(0x8ec07c)),
                ("macro", fg(0x8ec07c)),
                ("escape", fg(0xfe8019)),
                ("heading", fg(0xb8bb26).add_modifier(Modifier::BOLD)),
                ("link", fg(0x83a598).add_modifier(Modifier::UNDERLINED)),
                ("emphasis", Style::new().add_modifier(Modifier::ITALIC)),
            ],
        }
    }

    /// Thème clair « Solarized Light » (palette d'Ethan Schoonover, licence MIT), accents un peu
    /// assombris pour un contraste ≥ 3:1 sur le fond.
    pub fn solarized_light() -> Self {
        Self {
            name: "solarized-light",
            background: hex(0xfdf6e3),
            foreground: hex(0x073642),
            current_line: hex(0xeee8d5),
            selection: bg(0xe0dccb),
            search_match: bg(0xf2e6b6).add_modifier(Modifier::UNDERLINED),
            search_current: fg(0xfdf6e3).bg(hex(0x8a6100)),
            bracket_match: bg(0xd6d0bc).add_modifier(Modifier::BOLD),
            gutter: hex(0x758587),
            gutter_current: fg(0x586e75).add_modifier(Modifier::BOLD),
            border: hex(0xd6d0bc),
            border_focus: fg(0x2588cd).add_modifier(Modifier::BOLD),
            explorer_selected: fg(0xfdf6e3).bg(hex(0x1b65a6)),
            explorer_selected_unfocused: bg(0xeee8d5),
            tab_active: fg(0x073642).bg(hex(0xfdf6e3)).add_modifier(Modifier::BOLD),
            tab_inactive: fg(0x3f5157).bg(hex(0xeee8d5)),
            status_bar: fg(0xfdf6e3).bg(hex(0x1b65a6)),
            error: hex(0xdc322f),
            warning: hex(0xa66d00),
            muted: hex(0x768789),
            git_added: hex(0x7a8c00),
            git_modified: hex(0x2588cd),
            git_deleted: hex(0xdc322f),
            syntax: vec![
                ("keyword", fg(0x7a8c00)),
                ("function", fg(0x2588cd)),
                ("type", fg(0xa67e00)),
                ("string", fg(0x259088)),
                ("number", fg(0xd33682)),
                ("comment", fg(0x758587).add_modifier(Modifier::ITALIC)),
                ("constant", fg(0xcb4b16)),
                ("variable", fg(0x586e75)),
                ("property", fg(0x2588cd)),
                ("operator", fg(0x657b83)),
                ("punctuation", fg(0x657b83)),
                ("attribute", fg(0x6c71c4)),
                ("tag", fg(0x2588cd)),
                ("label", fg(0x6c71c4)),
                ("namespace", fg(0xa67e00)),
                ("macro", fg(0xcb4b16)),
                ("escape", fg(0xdc322f)),
                ("heading", fg(0xcb4b16).add_modifier(Modifier::BOLD)),
                ("link", fg(0x2588cd).add_modifier(Modifier::UNDERLINED)),
                ("emphasis", Style::new().add_modifier(Modifier::ITALIC)),
            ],
        }
    }

    /// Thème sombre « Solarized Dark » (palette d'Ethan Schoonover, licence MIT), texte et accents éclaircis pour le contraste.
    pub fn solarized_dark() -> Self {
        Self::from_palette(Palette {
            name: "solarized-dark",
            bg: 0x002b36,
            panel: 0x073642,
            selection: 0x17505f,
            fg: 0xd5dcd8,
            comment: 0x7f9399,
            border: 0x2d5560,
            accent: 0x1b65a6,
            on_accent: 0xfdf6e3,
            red: 0xef5350,
            yellow: 0xc99a00,
            green: 0x9aae00,
            blue: 0x4aa3e0,
            keyword: 0x9aae00,
            function: 0x4aa3e0,
            kind: 0xc99a00,
            string: 0x2fb5aa,
            number: 0xe0609d,
            constant: 0xe8703a,
        })
    }

    /// Thème sombre « Nord » (palette d'Arctic Ice Studio, licence MIT), commentaires et rouge éclaircis pour le contraste.
    pub fn nord() -> Self {
        Self::from_palette(Palette {
            name: "nord",
            bg: 0x2e3440,
            panel: 0x3b4252,
            selection: 0x434c5e,
            fg: 0xeceff4,
            comment: 0x98a4b8,
            border: 0x4c566a,
            accent: 0x88c0d0,
            on_accent: 0x2e3440,
            red: 0xd8848c,
            yellow: 0xebcb8b,
            green: 0xa3be8c,
            blue: 0x81a1c1,
            keyword: 0x81a1c1,
            function: 0x88c0d0,
            kind: 0x8fbcbb,
            string: 0xa3be8c,
            number: 0xb48ead,
            constant: 0xd08770,
        })
    }

    /// Thème sombre « Dracula » (palette de Zeno Rocha, licence MIT), commentaires éclaircis pour le contraste.
    pub fn dracula() -> Self {
        Self::from_palette(Palette {
            name: "dracula",
            bg: 0x282a36,
            panel: 0x343746,
            selection: 0x44475a,
            fg: 0xf8f8f2,
            comment: 0x8290bd,
            border: 0x44475a,
            accent: 0xbd93f9,
            on_accent: 0x282a36,
            red: 0xff5555,
            yellow: 0xf1fa8c,
            green: 0x50fa7b,
            blue: 0x8be9fd,
            keyword: 0xff79c6,
            function: 0x50fa7b,
            kind: 0x8be9fd,
            string: 0xf1fa8c,
            number: 0xbd93f9,
            constant: 0xffb86c,
        })
    }

    /// Thème sombre d'après la palette « Monokai » de Wimer Hazenberg, rouge et commentaires éclaircis pour le contraste.
    pub fn monokai() -> Self {
        Self::from_palette(Palette {
            name: "monokai",
            bg: 0x272822,
            panel: 0x3e3d32,
            selection: 0x49483e,
            fg: 0xf8f8f2,
            comment: 0xa5a28c,
            border: 0x49483e,
            accent: 0xa6e22e,
            on_accent: 0x272822,
            red: 0xff5c8a,
            yellow: 0xe6db74,
            green: 0xa6e22e,
            blue: 0x66d9ef,
            keyword: 0xff5c8a,
            function: 0xa6e22e,
            kind: 0x66d9ef,
            string: 0xe6db74,
            number: 0xae81ff,
            constant: 0xfd971f,
        })
    }

    /// Thème sombre « Tokyo Night » (palette d'enkia, licence MIT), commentaires éclaircis pour le contraste.
    pub fn tokyo_night() -> Self {
        Self::from_palette(Palette {
            name: "tokyo-night",
            bg: 0x1a1b26,
            panel: 0x24283b,
            selection: 0x33467c,
            fg: 0xc0caf5,
            comment: 0x7f88b0,
            border: 0x3b4261,
            accent: 0x7aa2f7,
            on_accent: 0x1a1b26,
            red: 0xf7768e,
            yellow: 0xe0af68,
            green: 0x9ece6a,
            blue: 0x7aa2f7,
            keyword: 0xbb9af7,
            function: 0x7aa2f7,
            kind: 0x2ac3de,
            string: 0x9ece6a,
            number: 0xff9e64,
            constant: 0xff9e64,
        })
    }

    /// Thème clair « Catppuccin Latte » (palette de Catppuccin, licence MIT), accents assombris pour le contraste.
    pub fn catppuccin_latte() -> Self {
        Self::from_palette(Palette {
            name: "catppuccin-latte",
            bg: 0xeff1f5,
            panel: 0xe6e9ef,
            selection: 0xccd0da,
            fg: 0x4c4f69,
            comment: 0x70738a,
            border: 0xbcc0cc,
            accent: 0x1a5cdc,
            on_accent: 0xeff1f5,
            red: 0xd20f39,
            yellow: 0x8f6200,
            green: 0x2f7d1a,
            blue: 0x1e66f5,
            keyword: 0x8839ef,
            function: 0x1e66f5,
            kind: 0x8f6200,
            string: 0x2f7d1a,
            number: 0xc2410c,
            constant: 0xc2410c,
        })
    }

    /// Thème complet à partir d'une palette ; les thèmes écrits à la main ci-dessus règlent chaque
    /// couleur, ceux-ci partagent une correspondance.
    fn from_palette(p: Palette) -> Self {
        Self {
            name: p.name,
            background: hex(p.bg),
            foreground: hex(p.fg),
            current_line: hex(p.panel),
            selection: bg(p.selection),
            search_match: bg(p.selection).add_modifier(Modifier::UNDERLINED),
            search_current: fg(p.bg).bg(hex(p.yellow)),
            bracket_match: bg(p.selection).add_modifier(Modifier::BOLD),
            gutter: hex(p.comment),
            gutter_current: fg(p.fg).add_modifier(Modifier::BOLD),
            border: hex(p.border),
            border_focus: fg(p.accent).add_modifier(Modifier::BOLD),
            explorer_selected: fg(p.on_accent).bg(hex(p.accent)),
            explorer_selected_unfocused: bg(p.panel),
            tab_active: fg(p.fg).bg(hex(p.bg)).add_modifier(Modifier::BOLD),
            tab_inactive: fg(p.fg).bg(hex(p.panel)),
            status_bar: fg(p.on_accent).bg(hex(p.accent)),
            error: hex(p.red),
            warning: hex(p.yellow),
            muted: hex(p.comment),
            git_added: hex(p.green),
            git_modified: hex(p.blue),
            git_deleted: hex(p.red),
            syntax: vec![
                ("keyword", fg(p.keyword)),
                ("function", fg(p.function)),
                ("type", fg(p.kind)),
                ("string", fg(p.string)),
                ("number", fg(p.number)),
                ("comment", fg(p.comment).add_modifier(Modifier::ITALIC)),
                ("constant", fg(p.constant)),
                ("variable", fg(p.fg)),
                ("property", fg(p.function)),
                ("operator", fg(p.fg)),
                ("punctuation", fg(p.comment)),
                ("attribute", fg(p.keyword)),
                ("tag", fg(p.keyword)),
                ("label", fg(p.number)),
                ("namespace", fg(p.kind)),
                ("macro", fg(p.constant)),
                ("escape", fg(p.red)),
                ("heading", fg(p.keyword).add_modifier(Modifier::BOLD)),
                ("link", fg(p.blue).add_modifier(Modifier::UNDERLINED)),
                ("emphasis", Style::new().add_modifier(Modifier::ITALIC)),
            ],
        }
    }

    /// Thème embarqué par son nom, un de `ide_core::THEMES`.
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            "gruvbox" => Some(Self::gruvbox()),
            "solarized-light" => Some(Self::solarized_light()),
            "solarized-dark" => Some(Self::solarized_dark()),
            "nord" => Some(Self::nord()),
            "dracula" => Some(Self::dracula()),
            "monokai" => Some(Self::monokai()),
            "tokyo-night" => Some(Self::tokyo_night()),
            "catppuccin-latte" => Some(Self::catppuccin_latte()),
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

    fn themes() -> Vec<Theme> {
        ide_core::THEMES
            .iter()
            .map(|n| Theme::by_name(n).expect("thème de THEMES"))
            .collect()
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
        // Chaque nom de `THEMES` a son thème, sous ce nom.
        for (n, t) in ide_core::THEMES.iter().zip(themes()) {
            assert_eq!(*n, t.name);
        }
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
                ("git_added", t.git_added),
                ("git_modified", t.git_modified),
                ("git_deleted", t.git_deleted),
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
