//! Lecture de `config.toml` : utilisateur puis projet, clé par clé.

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use ide_core::Settings;
use toml::{Table, Value};

/// Lit la config utilisateur puis celle du projet (`project_root/.ide/config.toml`), la seconde surchargeant la première clé par clé.
/// Fichier absent = rien ; fichier illisible, TOML invalide, clé inconnue ou valeur hors bornes = message dans `errors`, la clé garde sa valeur précédente.
pub fn load_config(project_root: &Path) -> (Settings, Vec<String>) {
    let user = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .map(|d| d.join("ide/config.toml"));
    let project = project_root.join(".ide/config.toml");
    load_config_from(user.as_deref(), Some(&project))
}

/// Variante testable : chemins explicites (None = pas de fichier).
pub fn load_config_from(user: Option<&Path>, project: Option<&Path>) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let mut errors = Vec::new();
    for path in [user, project].into_iter().flatten() {
        apply_file(&mut s, path, &mut errors);
    }
    (s, errors)
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Chemin affiché, `~` à la place du dossier personnel.
fn shown(path: &Path) -> String {
    match home().and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rel) => format!("~/{}", rel.display()),
        None => path.display().to_string(),
    }
}

fn apply_file(s: &mut Settings, path: &Path, errors: &mut Vec<String>) {
    let file = shown(path);
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return,
        Err(e) => return errors.push(format!("{file} : illisible ({e})")),
    };
    let table = match text.parse::<Table>() {
        Ok(t) => t,
        Err(e) => {
            let msg = e.message().trim_end();
            return errors.push(match e.span() {
                Some(span) => {
                    let before = text.get(..span.start).unwrap_or(&text);
                    let line = before.matches('\n').count() + 1;
                    format!("{file} : TOML invalide, ligne {line} : {msg}")
                }
                None => format!("{file} : TOML invalide : {msg}"),
            });
        }
    };
    let mut err = |key: &str, msg: &str| errors.push(format!("{file} : {key} : {msg}"));
    for (section, value) in &table {
        let handler: fn(&mut Settings, &str, &Value) -> Result<(), String> = match section.as_str()
        {
            "editor" => editor_key,
            "explorer" => explorer_key,
            // Sections du cahier pas encore gérées : acceptées, ignorées.
            "keys" | "lsp" | "icons" => continue,
            _ => {
                err(section, "section inconnue");
                continue;
            }
        };
        let Value::Table(keys) = value else {
            err(section, "doit être une section [..]");
            continue;
        };
        for (key, v) in keys {
            if let Err(msg) = handler(s, key, v) {
                err(&format!("{section}.{key}"), &msg);
            }
        }
    }
}

fn editor_key(s: &mut Settings, key: &str, v: &Value) -> Result<(), String> {
    match key {
        "theme" => match str_of(v)? {
            t @ ("dark" | "light") => s.theme = t.into(),
            t if t.ends_with(".toml") || t.contains('/') => {
                return Err("thème personnalisé pas encore pris en charge".into());
            }
            _ => return Err("attendu \"dark\", \"light\" ou un chemin de thème".into()),
        },
        "tab_size" => {
            let n = int_in(v, 1, 16)? as usize;
            s.indent.width = n;
            s.indent.tab_width = n;
        }
        "insert_spaces" => s.indent.use_tabs = !bool_of(v)?,
        "format_on_save" => {
            bool_of(v)?;
        }
        "icons" => match str_of(v)? {
            "nerd" => s.nerd_icons = true,
            "ascii" => s.nerd_icons = false,
            _ => return Err("attendu \"nerd\" ou \"ascii\"".into()),
        },
        _ => return Err("clé inconnue".into()),
    }
    Ok(())
}

fn explorer_key(s: &mut Settings, key: &str, v: &Value) -> Result<(), String> {
    match key {
        "width" => s.explorer_width = int_in(v, 12, 80)? as u16,
        "show_hidden" => s.show_hidden = bool_of(v)?,
        _ => return Err("clé inconnue".into()),
    }
    Ok(())
}

fn str_of(v: &Value) -> Result<&str, String> {
    v.as_str().ok_or_else(|| "chaîne attendue".into())
}

fn bool_of(v: &Value) -> Result<bool, String> {
    v.as_bool().ok_or_else(|| "true ou false attendu".into())
}

fn int_in(v: &Value, min: i64, max: i64) -> Result<i64, String> {
    match v.as_integer() {
        Some(n) if (min..=max).contains(&n) => Ok(n),
        _ => Err(format!("entier de {min} à {max} attendu")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Result;

    /// Écrit `user` et `project` (None = pas de fichier) puis charge.
    fn load(user: Option<&str>, project: Option<&str>) -> Result<(Settings, Vec<String>)> {
        let dir = tempfile::tempdir()?;
        let (u, p) = (dir.path().join("user.toml"), dir.path().join("projet.toml"));
        if let Some(t) = user {
            fs::write(&u, t)?;
        }
        if let Some(t) = project {
            fs::write(&p, t)?;
        }
        Ok(load_config_from(Some(&u), Some(&p)))
    }

    #[test]
    fn aucun_fichier() -> Result<()> {
        assert_eq!(load(None, None)?, (Settings::default(), vec![]));
        let dir = tempfile::tempdir()?;
        // Pas de .ide/ : seule la config utilisateur (éventuelle) compte, sans erreur pour le projet.
        let (_, errors) = load_config(dir.path());
        assert!(errors.iter().all(|e| !e.contains(".ide")), "{errors:?}");
        Ok(())
    }

    #[test]
    fn utilisateur_seul() -> Result<()> {
        let cfg = "[editor]\ntheme = \"light\"\ntab_size = 2\ninsert_spaces = false\n\
                   icons = \"ascii\"\n[explorer]\nwidth = 40\nshow_hidden = true\n";
        let (s, errors) = load(Some(cfg), None)?;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(s.theme, "light");
        assert_eq!(
            (s.indent.use_tabs, s.indent.width, s.indent.tab_width),
            (true, 2, 2)
        );
        assert!(!s.nerd_icons);
        assert_eq!((s.explorer_width, s.show_hidden), (40, true));
        Ok(())
    }

    #[test]
    fn projet_surcharge_une_cle() -> Result<()> {
        let user = "[editor]\ntheme = \"light\"\n[explorer]\nwidth = 40\n";
        let (s, errors) = load(Some(user), Some("[explorer]\nwidth = 20\n"))?;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!((s.theme.as_str(), s.explorer_width), ("light", 20));
        Ok(())
    }

    #[test]
    fn toml_invalide() -> Result<()> {
        let (s, errors) = load(Some("[editor]\ntheme = \n"), None)?;
        assert_eq!(s, Settings::default());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("TOML invalide, ligne 2"), "{errors:?}");
        Ok(())
    }

    #[test]
    fn cles_inconnues() -> Result<()> {
        let (_, errors) = load(Some("[editor]\nthem = \"light\"\n[divers]\nx = 1\n"), None)?;
        assert_eq!(errors.len(), 2, "{errors:?}");
        // Table triée : messages dans l'ordre alphabétique des sections et clés.
        assert!(
            errors[0].ends_with("divers : section inconnue"),
            "{errors:?}"
        );
        assert!(
            errors[1].ends_with("editor.them : clé inconnue"),
            "{errors:?}"
        );
        Ok(())
    }

    #[test]
    fn hors_bornes_ou_mauvais_type_garde_la_valeur_precedente() -> Result<()> {
        let user = "[editor]\ntab_size = 2\n[explorer]\nwidth = 40\n";
        let project = "[editor]\ntab_size = 17\ninsert_spaces = \"oui\"\n[explorer]\nwidth = 5\n";
        let (s, errors) = load(Some(user), Some(project))?;
        assert_eq!(errors.len(), 3, "{errors:?}");
        let has = |m: &str| errors.iter().any(|e| e.contains(m));
        assert!(has("editor.tab_size : entier de 1 à 16"), "{errors:?}");
        assert!(has("editor.insert_spaces : true ou false"), "{errors:?}");
        assert!(has("explorer.width : entier de 12 à 80"), "{errors:?}");
        assert_eq!(
            (s.indent.width, s.indent.use_tabs, s.explorer_width),
            (2, false, 40)
        );
        Ok(())
    }

    #[test]
    fn sections_futures_ignorees() -> Result<()> {
        let cfg = "[editor]\nformat_on_save = true\n[keys]\n\"ctrl+p\" = \"open_file_picker\"\n\
                   [lsp.rust]\ncommand = \"rust-analyzer\"\n\
                   [icons.extensions]\nrs = { glyph = \"x\", color = \"#dea584\" }\n";
        assert_eq!(load(Some(cfg), None)?, (Settings::default(), vec![]));
        Ok(())
    }

    #[test]
    fn theme_chemin_non_pris_en_charge() -> Result<()> {
        let (s, errors) = load(Some("[editor]\ntheme = \"~/themes/nuit.toml\"\n"), None)?;
        assert_eq!(s.theme, "dark");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].ends_with("editor.theme : thème personnalisé pas encore pris en charge"));
        Ok(())
    }
}
