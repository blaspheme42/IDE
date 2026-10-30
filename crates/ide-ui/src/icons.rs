//! Icônes de l'explorateur (F-02) : glyphe Nerd Font v3 et couleur par nom puis par extension,
//! repli ASCII sans Nerd Font (NF-12). Recherche linéaire dans des tables statiques.

use ratatui::style::Color;

/// Jeu d'icônes : Nerd Font ou ASCII (NF-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconSet {
    #[default]
    Nerd,
    Ascii,
}

type Entry = (&'static str, &'static str, Color);

const fn hex(c: u32) -> Color {
    Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

const RUST: Color = hex(0xdea584);
const GREY: Color = hex(0x6d8086);
const FOLDER: Color = hex(0x7ebae4);

const FILE: Entry = ("", "\u{f15b}", GREY);
const DIR_CLOSED: &str = "\u{f07b}";
const DIR_OPEN: &str = "\u{f07c}";

/// Noms exacts, en minuscules (prioritaires sur l'extension).
const NAMES: &[Entry] = &[
    ("cargo.toml", "\u{e7a8}", RUST),
    ("cargo.lock", "\u{e7a8}", RUST),
    ("rust-toolchain.toml", "\u{e7a8}", RUST),
    ("rustfmt.toml", "\u{e7a8}", RUST),
    ("clippy.toml", "\u{e7a8}", RUST),
    ("makefile", "\u{e779}", GREY),
    ("justfile", "\u{f0ae}", GREY),
    ("cmakelists.txt", "\u{e794}", GREY),
    ("dockerfile", "\u{f308}", hex(0x458ee6)),
    ("docker-compose.yml", "\u{f308}", hex(0x458ee6)),
    (".dockerignore", "\u{f308}", hex(0x458ee6)),
    (".gitignore", "\u{e702}", hex(0xf1502f)),
    (".gitattributes", "\u{e702}", hex(0xf1502f)),
    (".gitmodules", "\u{e702}", hex(0xf1502f)),
    (".gitlab-ci.yml", "\u{f296}", hex(0xe24329)),
    (".editorconfig", "\u{e652}", hex(0xfff2f2)),
    ("readme.md", "\u{f00ba}", hex(0xededed)),
    ("readme", "\u{f00ba}", hex(0xededed)),
    ("changelog.md", "\u{f48a}", hex(0xdddddd)),
    ("license", "\u{e60a}", hex(0xd0bf41)),
    ("license.md", "\u{e60a}", hex(0xd0bf41)),
    ("package.json", "\u{e71e}", hex(0xe8274b)),
    ("package-lock.json", "\u{e71e}", hex(0x7a0d21)),
    (".npmrc", "\u{e71e}", hex(0xe8274b)),
    ("tsconfig.json", "\u{e69d}", hex(0x519aba)),
    (".prettierrc", "\u{e6b4}", hex(0x4285f4)),
    (".eslintrc", "\u{e655}", hex(0x4b32c3)),
    (".env", "\u{f462}", hex(0xfaf743)),
    ("go.mod", "\u{e627}", hex(0x519aba)),
    ("go.sum", "\u{e627}", hex(0x519aba)),
    ("gemfile", "\u{e791}", hex(0x701516)),
    ("rakefile", "\u{e791}", hex(0x701516)),
    ("requirements.txt", "\u{e606}", hex(0xffbc03)),
    ("pyproject.toml", "\u{e606}", hex(0xffbc03)),
    ("flake.nix", "\u{f313}", FOLDER),
    (".bashrc", "\u{e795}", hex(0x89e051)),
    (".zshrc", "\u{e795}", hex(0x89e051)),
];

/// Dernière extension, en minuscules.
const EXTS: &[Entry] = &[
    ("rs", "\u{e7a8}", RUST),
    ("py", "\u{e606}", hex(0xffbc03)),
    ("c", "\u{e61e}", hex(0x599eff)),
    ("h", "\u{f0fd}", hex(0xa074c4)),
    ("cpp", "\u{e61d}", hex(0x519aba)),
    ("cc", "\u{e61d}", hex(0x519aba)),
    ("hpp", "\u{f0fd}", hex(0xa074c4)),
    ("cs", "\u{f031b}", hex(0x596706)),
    ("js", "\u{e74e}", hex(0xcbcb41)),
    ("mjs", "\u{e74e}", hex(0xcbcb41)),
    ("ts", "\u{e628}", hex(0x519aba)),
    ("tsx", "\u{e7ba}", hex(0x1354bf)),
    ("jsx", "\u{e7ba}", hex(0x20c2e3)),
    ("vue", "\u{e6a0}", hex(0x8dc149)),
    ("svelte", "\u{e697}", hex(0xff3e00)),
    ("html", "\u{e736}", hex(0xe44d26)),
    ("htm", "\u{e736}", hex(0xe44d26)),
    ("css", "\u{e749}", hex(0x42a5f5)),
    ("scss", "\u{e603}", hex(0xf55385)),
    ("json", "\u{e60b}", hex(0xcbcb41)),
    ("toml", "\u{e6b2}", hex(0x9c4221)),
    ("yaml", "\u{e6a8}", GREY),
    ("yml", "\u{e6a8}", GREY),
    ("xml", "\u{f05c0}", hex(0xe37933)),
    ("ini", "\u{e615}", GREY),
    ("conf", "\u{e615}", GREY),
    ("csv", "\u{e64a}", hex(0x89e051)),
    ("md", "\u{f48a}", hex(0xdddddd)),
    ("tex", "\u{e69b}", hex(0x3d6117)),
    ("sh", "\u{e795}", hex(0x4d5a5e)),
    ("bash", "\u{e795}", hex(0x89e051)),
    ("zsh", "\u{e795}", hex(0x89e051)),
    ("fish", "\u{e795}", hex(0x4d5a5e)),
    ("ps1", "\u{f0a0a}", hex(0x4273ca)),
    ("bat", "\u{e629}", hex(0xc1f12e)),
    ("go", "\u{e627}", hex(0x00add8)),
    ("java", "\u{e738}", hex(0xcc3e44)),
    ("kt", "\u{e634}", hex(0x7f52ff)),
    ("scala", "\u{e737}", hex(0xcc3e44)),
    ("swift", "\u{e755}", hex(0xe37933)),
    ("dart", "\u{e798}", hex(0x03589c)),
    ("lua", "\u{e620}", hex(0x51a0cf)),
    ("rb", "\u{e791}", hex(0x701516)),
    ("php", "\u{e608}", hex(0xa074c4)),
    ("pl", "\u{e769}", hex(0x519aba)),
    ("hs", "\u{e61f}", hex(0xa074c4)),
    ("ml", "\u{e67a}", hex(0xe37933)),
    ("ex", "\u{e62d}", hex(0xa074c4)),
    ("exs", "\u{e62d}", hex(0xa074c4)),
    ("erl", "\u{e7b1}", hex(0xb83998)),
    ("clj", "\u{e768}", hex(0x8dc149)),
    ("el", "\u{e632}", hex(0x8172be)),
    ("jl", "\u{e624}", hex(0xa270ba)),
    ("r", "\u{f07d4}", hex(0x2266ba)),
    ("zig", "\u{e6a9}", hex(0xf69a1b)),
    ("nim", "\u{e677}", hex(0xf3d400)),
    ("nix", "\u{f313}", FOLDER),
    ("vim", "\u{e62b}", hex(0x019833)),
    ("sql", "\u{e706}", hex(0xdad8d8)),
    ("graphql", "\u{f0877}", hex(0xe535ab)),
    ("tf", "\u{e69a}", hex(0x5f43e9)),
    ("diff", "\u{f440}", hex(0x41535b)),
    ("patch", "\u{f440}", hex(0x41535b)),
    ("png", "\u{e60d}", hex(0xa074c4)),
    ("jpg", "\u{e60d}", hex(0xa074c4)),
    ("jpeg", "\u{e60d}", hex(0xa074c4)),
    ("gif", "\u{e60d}", hex(0xa074c4)),
    ("webp", "\u{e60d}", hex(0xa074c4)),
    ("ico", "\u{e60d}", hex(0xcbcb41)),
    ("svg", "\u{f0721}", hex(0xffb13b)),
    ("pdf", "\u{e67d}", hex(0xb30b00)),
    ("mp3", "\u{f001}", hex(0x66d8ef)),
    ("wav", "\u{f001}", hex(0x66d8ef)),
    ("mp4", "\u{f03d}", hex(0xfd971f)),
    ("ttf", "\u{f031}", hex(0xececec)),
    ("zip", "\u{f410}", hex(0xeca517)),
    ("tar", "\u{f410}", hex(0xeca517)),
    ("gz", "\u{f410}", hex(0xeca517)),
    ("lock", "\u{e672}", hex(0xbbbbbb)),
    ("txt", "\u{f15c}", hex(0x89e051)),
    ("log", "\u{f0331}", hex(0xdddddd)),
    ("exe", "\u{eae8}", hex(0x9f0500)),
    ("so", "\u{eae8}", hex(0xdcddd6)),
];

/// Dossiers spéciaux (même glyphe ouvert ou fermé).
const DIRS: &[Entry] = &[
    (".git", "\u{e5fb}", hex(0xf1502f)),
    (".github", "\u{e5fd}", FOLDER),
    ("node_modules", "\u{e5fa}", hex(0xe8274b)),
    ("target", "\u{f07b}", GREY),
    ("src", "\u{f07b}", RUST),
    ("tests", "\u{f07b}", hex(0x89e051)),
    ("docs", "\u{f07b}", hex(0xdddddd)),
];

fn find(table: &'static [Entry], key: &str) -> Option<&'static Entry> {
    table.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(key))
}

/// Glyphe et couleur pour une entrée de l'explorateur. `expanded` ne sert qu'aux dossiers.
pub fn icon(name: &str, is_dir: bool, expanded: bool, set: IconSet) -> (&'static str, Color) {
    let (glyph, color) = if is_dir {
        let default = if expanded { DIR_OPEN } else { DIR_CLOSED };
        find(DIRS, name).map_or((default, FOLDER), |&(_, g, c)| (g, c))
    } else {
        // `.bashrc` n'a pas d'extension : il faut un préfixe non vide avant le point.
        let ext = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty());
        let &(_, g, c) = find(NAMES, name)
            .or_else(|| ext.and_then(|(_, e)| find(EXTS, e)))
            .unwrap_or(&FILE);
        (g, c)
    };
    match set {
        IconSet::Nerd => (glyph, color),
        IconSet::Ascii if !is_dir => ("*", color),
        IconSet::Ascii if expanded => ("-", color),
        IconSet::Ascii => ("+", color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    const N: IconSet = IconSet::Nerd;

    #[test]
    fn nom_exact_prioritaire_sur_extension() {
        assert_eq!(icon("Cargo.toml", false, false, N), ("\u{e7a8}", RUST));
        assert_eq!(icon("x.toml", false, false, N).0, "\u{e6b2}");
        assert_eq!(icon("MAKEFILE", false, false, N).0, "\u{e779}");
    }

    #[test]
    fn extension_insensible_a_la_casse_et_derniere() {
        assert_eq!(icon("MAIN.RS", false, false, N), ("\u{e7a8}", RUST));
        assert_eq!(icon("a.tar.gz", false, false, N).0, "\u{f410}");
    }

    #[test]
    fn sans_extension_ou_dotfile_inconnu_donne_le_defaut() {
        assert_eq!(icon("notes", false, false, N), (FILE.1, FILE.2));
        assert_eq!(icon(".inconnu", false, false, N), (FILE.1, FILE.2));
        assert_eq!(icon("fin.", false, false, N), (FILE.1, FILE.2));
        assert_eq!(icon(".gitignore", false, false, N).0, "\u{e702}");
    }

    #[test]
    fn dossiers_ouvert_ferme_et_speciaux() {
        assert_eq!(icon("lib", true, false, N), (DIR_CLOSED, FOLDER));
        assert_eq!(icon("lib", true, true, N), (DIR_OPEN, FOLDER));
        assert_eq!(icon(".git", true, false, N).0, "\u{e5fb}");
        // Un dossier nommé comme un fichier connu reste un dossier.
        assert_eq!(icon("Cargo.toml", true, false, N).0, DIR_CLOSED);
    }

    #[test]
    fn mode_ascii_garde_la_couleur() {
        let a = IconSet::Ascii;
        assert_eq!(icon("main.rs", false, false, a), ("*", RUST));
        assert_eq!(icon("src", true, false, a), ("+", RUST));
        assert_eq!(icon("lib", true, true, a), ("-", FOLDER));
        assert_eq!(IconSet::default(), N);
    }

    #[test]
    fn table_couvre_80_entrees_sans_doublon() {
        assert!(
            NAMES.len() + EXTS.len() >= 80,
            "{}",
            NAMES.len() + EXTS.len()
        );
        for table in [NAMES, EXTS, DIRS] {
            for (i, (k, _, _)) in table.iter().enumerate() {
                assert_eq!(*k, k.to_ascii_lowercase(), "clé non minuscule : {k}");
                assert!(table[..i].iter().all(|(o, _, _)| o != k), "doublon : {k}");
            }
        }
    }

    #[test]
    fn chaque_glyphe_est_un_char_de_largeur_1() {
        let all = NAMES.iter().chain(EXTS).chain(DIRS).map(|e| e.1);
        let all = all.chain([FILE.1, DIR_CLOSED, DIR_OPEN, "*", "+", "-"]);
        for g in all {
            assert_eq!(g.chars().count(), 1, "{g:?}");
            assert_eq!(g.width(), 1, "{g:?}");
        }
    }
}
