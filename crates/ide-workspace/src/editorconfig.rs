//! Style d'indentation lu depuis les `.editorconfig` (F-14).

use std::path::Path;

use ec4rs::property::{IndentSize, IndentStyle as Style, TabWidth};

/// Style d'indentation d'un fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentStyle {
    pub use_tabs: bool,
    /// Largeur d'un niveau d'indentation (`indent_size`).
    pub width: usize,
    /// Largeur d'affichage d'une tabulation (`tab_width`, sinon `indent_size`).
    pub tab_width: usize,
}

impl Default for IndentStyle {
    /// 4 espaces.
    fn default() -> Self {
        Self {
            use_tabs: false,
            width: 4,
            tab_width: 4,
        }
    }
}

/// Lit les `.editorconfig` applicables à `path` (en remontant jusqu'à `root = true`) ;
/// défaut si absent, illisible ou invalide. `path` n'a pas besoin d'exister.
pub fn indent_style(path: &Path) -> IndentStyle {
    indent_style_or(path, IndentStyle::default())
}

/// Comme [`indent_style`], mais `default` quand aucun `.editorconfig` ne fixe
/// `indent_style`, `indent_size` ni `tab_width`.
pub fn indent_style_or(path: &Path, default: IndentStyle) -> IndentStyle {
    // Un .editorconfig illisible ou mal formé : on garde le défaut.
    let Ok(props) = ec4rs::properties_of(path) else {
        return default;
    };
    if ["indent_style", "indent_size", "tab_width"]
        .iter()
        .all(|k| props.get_raw_for_key(k).is_unset())
    {
        return default;
    }
    let use_tabs = match props.get::<Style>() {
        Ok(Style::Tabs) => true,
        Ok(Style::Spaces) => false,
        Err(_) => default.use_tabs,
    };
    let tab_width = match props.get::<TabWidth>() {
        Ok(TabWidth::Value(n)) => Some(n),
        _ => None,
    };
    let width = match props.get::<IndentSize>() {
        Ok(IndentSize::Value(n)) => n,
        Ok(IndentSize::UseTabWidth) => tab_width.unwrap_or(default.width),
        Err(_) if use_tabs => tab_width.unwrap_or(default.width),
        Err(_) => default.width,
    };
    IndentStyle {
        use_tabs,
        width: width.clamp(1, 16),
        // Spécification editorconfig : sans `tab_width`, on prend `indent_size`.
        tab_width: tab_width.unwrap_or(width).clamp(1, 16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Result;

    const fn st(use_tabs: bool, width: usize) -> IndentStyle {
        IndentStyle {
            use_tabs,
            width,
            tab_width: width,
        }
    }

    /// Écrit `config` comme `.editorconfig` de `dir` et lit le style de `dir/file`.
    fn style(dir: &Path, config: &str, file: &str) -> Result<IndentStyle> {
        fs::write(dir.join(".editorconfig"), config)?;
        Ok(indent_style(&dir.join(file)))
    }

    #[test]
    fn sans_regle_defaut() -> Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(style(dir.path(), "root = true\n", "a.rs")?, st(false, 4));
        assert_eq!(IndentStyle::default(), st(false, 4));
        Ok(())
    }

    #[test]
    fn tab_width_distinct_de_indent_size() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cfg = "root = true\n[*]\nindent_style = tab\nindent_size = 2\ntab_width = 8\n";
        let s = style(dir.path(), cfg, "a.c")?;
        assert_eq!((s.use_tabs, s.width, s.tab_width), (true, 2, 8));
        Ok(())
    }

    #[test]
    fn tabulations() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let d = dir.path();
        let tab = "root = true\n[*]\nindent_style = tab\n";
        assert_eq!(style(d, tab, "a.rs")?, st(true, 4));
        let tab8 = "root = true\n[*]\nindent_style = tab\ntab_width = 8\n";
        assert_eq!(style(d, tab8, "a.rs")?, st(true, 8));
        let via_tab = "root = true\n[*]\nindent_size = tab\ntab_width = 3\n";
        assert_eq!(style(d, via_tab, "a.rs")?, st(false, 3));
        Ok(())
    }

    #[test]
    fn section_par_extension() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cfg = "root = true\n[*.rs]\nindent_size = 2\n";
        assert_eq!(style(dir.path(), cfg, "a.rs")?, st(false, 2));
        assert_eq!(style(dir.path(), cfg, "a.md")?, st(false, 4));
        Ok(())
    }

    #[test]
    fn enfant_gagne_sur_parent() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let sub = dir.path().join("sous");
        fs::create_dir(&sub)?;
        let parent = "root = true\n[*]\nindent_style = tab\nindent_size = 8\n";
        fs::write(dir.path().join(".editorconfig"), parent)?;
        // Fichier inexistant : le style s'applique quand même.
        let s = style(&sub, "[*]\nindent_size = 2\n", "nouveau.rs")?;
        assert_eq!(s, st(true, 2));
        Ok(())
    }

    #[test]
    fn root_arrete_la_remontee() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let sub = dir.path().join("sous");
        fs::create_dir(&sub)?;
        let parent = "root = true\n[*]\nindent_style = tab\n";
        fs::write(dir.path().join(".editorconfig"), parent)?;
        let s = style(&sub, "root = true\n[*]\nindent_size = 2\n", "a.rs")?;
        assert_eq!(s, st(false, 2));
        Ok(())
    }

    #[test]
    fn defaut_fourni_sans_indentation_fixee() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let d = dir.path();
        let tab2 = IndentStyle {
            use_tabs: true,
            width: 2,
            tab_width: 2,
        };
        let sans = "root = true\n[*]\ncharset = utf-8\n";
        fs::write(d.join(".editorconfig"), sans)?;
        assert_eq!(indent_style_or(&d.join("a.rs"), tab2), tab2);
        assert_eq!(indent_style(&d.join("a.rs")), st(false, 4));
        fs::write(
            d.join(".editorconfig"),
            "root = true\n[*]\nindent_size = 3\n",
        )?;
        assert_eq!(indent_style_or(&d.join("a.rs"), tab2), st(true, 3));
        Ok(())
    }

    #[test]
    fn valeurs_invalides_ou_hors_bornes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let d = dir.path();
        let invalide = "root = true\n[*]\nindent_style = zigzag\nindent_size = beaucoup\n";
        assert_eq!(style(d, invalide, "a.rs")?, st(false, 4));
        let zero = "root = true\n[*]\nindent_size = 0\n";
        assert_eq!(style(d, zero, "a.rs")?, st(false, 1));
        let enorme = "root = true\n[*]\nindent_size = 99\n";
        assert_eq!(style(d, enorme, "a.rs")?, st(false, 16));
        // Fichier mal formé : défaut, sans panique.
        let casse = "root = true\n[*\nindent_size = 2\n";
        assert_eq!(style(d, casse, "a.rs")?, st(false, 4));
        Ok(())
    }
}
