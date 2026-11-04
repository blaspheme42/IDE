//! Vue d'ensemble du projet : tous ses fichiers (palette Ctrl+P), recherche texte (F-52) et branche git (F-70).

use std::io::Read;
use std::path::{Path, PathBuf};

use ide_core::{Location, Position, TextHit};
use ignore::WalkBuilder;
use regex::Regex;

/// shortcut: liste plafonnée à ce nombre de fichiers ; passer à un parcours incrémental (nucleo injecteur) si besoin.
const MAX_FILES: usize = 100_000;
/// Résultats gardés au plus par `search_text`.
const MAX_HITS: usize = 1000;
/// Au-delà, un fichier n'est pas lu par `search_text`.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// shortcut: seul le début d'une ligne géante (bundle minifié) est cherché, une regex coûteuse y
/// prendrait des minutes ; rendre la recherche interruptible pour tout parcourir.
const MAX_LINE_BYTES: usize = 100 * 1024;
/// Caractères gardés avant l'occurrence dans un extrait, et dans tout l'extrait.
const EXCERPT_CONTEXT: usize = 20;
const EXCERPT_CHARS: usize = 200;

/// Fichiers du projet (chemins absolus), triés, .gitignore respecté comme dans l'explorateur, `.git` exclu.
pub fn list_files(root: &Path, show_hidden: bool) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WalkBuilder::new(root)
        .require_git(false)
        .hidden(!show_hidden)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .take(MAX_FILES)
        .map(ignore::DirEntry::into_path)
        .collect();
    files.sort();
    files
}

/// Lignes des fichiers de `list_files` où `re` trouve une occurrence non vide (la première de la ligne),
/// et `true` si un plafond coupe la liste. Sautés : fichiers trop gros, illisibles ou non UTF-8.
/// `None` : `stop` a répondu vrai, testé entre deux fichiers.
pub fn search_text(
    root: &Path,
    show_hidden: bool,
    re: &Regex,
    stop: impl Fn() -> bool,
) -> Option<(Vec<TextHit>, bool)> {
    let mut hits = Vec::new();
    let files = list_files(root, show_hidden);
    let capped = files.len() >= MAX_FILES;
    for path in files {
        if stop() {
            return None;
        }
        // `take` et non `metadata` : un pseudo-fichier annonce 0 octet, un autre peut grossir.
        let mut text = String::new();
        let read = std::fs::File::open(&path)
            .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_string(&mut text));
        if !read.is_ok_and(|n| n as u64 <= MAX_FILE_BYTES) {
            continue;
        }
        for (line_no, line) in text.lines().enumerate() {
            let mut end = line.len().min(MAX_LINE_BYTES);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            let line = &line[..end];
            let Some(m) = re.find_iter(line).find(|m| !m.is_empty()) else {
                continue;
            };
            if hits.len() == MAX_HITS {
                return Some((hits, true));
            }
            let character = line[..m.start()].encode_utf16().count();
            let at = |character| Position {
                line: line_no,
                character,
            };
            hits.push(TextHit {
                location: Location {
                    path: path.clone(),
                    start: at(character),
                    end: at(character + m.as_str().encode_utf16().count()),
                },
                excerpt: excerpt(line, m.start()),
            });
        }
    }
    Some((hits, capped))
}

/// Extrait de `line` qui montre l'occurrence à l'octet `start`, même au fond d'une ligne minifiée :
/// sans l'indentation, au plus `EXCERPT_CONTEXT` caractères avant elle, borné à `EXCERPT_CHARS`.
fn excerpt(line: &str, start: usize) -> String {
    let trimmed = line.trim_start();
    let before = &trimmed[..start.saturating_sub(line.len() - trimmed.len())];
    let from = before
        .char_indices()
        .rev()
        .nth(EXCERPT_CONTEXT - 1)
        .map_or(0, |(i, _)| i);
    let shown = trimmed[from..].chars().take(EXCERPT_CHARS);
    // Les marques bidi réordonneraient l'affichage de la ligne.
    let hidden =
        |c: char| c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let shown = shown.map(|c| if hidden(c) { ' ' } else { c });
    (if from > 0 { "…" } else { "" })
        .chars()
        .chain(shown)
        .collect()
}

/// Branche git de `root` (dépôt trouvé en remontant) ; HEAD détachée → 7 premiers caractères du commit.
/// `None` hors dépôt. Lecture directe de `.git/HEAD` : pas besoin de `gix` pour ça.
pub fn git_branch(root: &Path) -> Option<String> {
    let git = root
        .ancestors()
        .map(|d| d.join(".git"))
        .find(|g| g.exists())?;
    // Worktree ou sous-module : `.git` est un fichier « gitdir: <chemin> ».
    let dir = if git.is_file() {
        let text = std::fs::read_to_string(&git).ok()?;
        let target = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
        if target.is_absolute() {
            target
        } else {
            git.parent()?.join(target)
        }
    } else {
        git
    };
    let head = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(r) => Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string()),
        None => head.get(..7).map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Result;

    #[test]
    fn fichiers_du_projet() -> Result<()> {
        let d = tempfile::tempdir()?;
        fs::create_dir_all(d.path().join("src/sous"))?;
        fs::create_dir_all(d.path().join(".git"))?;
        for f in [
            "src/a.rs",
            "src/sous/b.rs",
            "ignoré.log",
            ".caché",
            ".git/HEAD",
        ] {
            fs::write(d.path().join(f), "")?;
        }
        fs::write(d.path().join(".gitignore"), "*.log\n")?;
        let rel = |v: Vec<PathBuf>| -> Vec<String> {
            v.iter()
                .filter_map(|p| p.strip_prefix(d.path()).ok())
                .map(|p| p.display().to_string())
                .collect()
        };
        assert_eq!(
            rel(list_files(d.path(), false)),
            ["src/a.rs", "src/sous/b.rs"]
        );
        assert_eq!(
            rel(list_files(d.path(), true)),
            [".caché", ".gitignore", "src/a.rs", "src/sous/b.rs"]
        );
        Ok(())
    }

    fn regex(query: &str, regex: bool, case_sensitive: bool) -> Regex {
        let s = ide_core::Search {
            query: query.into(),
            regex,
            case_sensitive,
            ..ide_core::Search::default()
        };
        s.compile().expect("regex valide")
    }

    /// (fichier, ligne, colonne) des résultats et plafond atteint.
    fn cherche(root: &Path, re: &Regex) -> (Vec<(String, usize, usize)>, bool) {
        let (hits, truncated) = search_text(root, false, re, || false).expect("non interrompue");
        let hits = hits.iter().map(|h| {
            let path = h.location.path.strip_prefix(root).expect("sous la racine");
            let start = h.location.start;
            (path.display().to_string(), start.line, start.character)
        });
        (hits.collect(), truncated)
    }

    #[test]
    fn recherche_respecte_gitignore_et_saute_binaire_et_gros_fichier() -> Result<()> {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join(".gitignore"), "*.log\n")?;
        fs::write(d.path().join("a.txt"), "rien\ncible\n")?;
        fs::write(d.path().join("b.log"), "cible\n")?;
        fs::write(d.path().join("c.bin"), b"cible\xff\xfe\n")?;
        let limite = "cible\n".to_owned() + &"x".repeat(MAX_FILE_BYTES as usize - 6);
        fs::write(d.path().join("d.txt"), &limite)?;
        fs::write(d.path().join("e.txt"), limite + "x")?;
        assert_eq!(
            cherche(d.path(), &regex("cible", false, false)),
            (vec![("a.txt".into(), 1, 0), ("d.txt".into(), 0, 0)], false)
        );
        Ok(())
    }

    #[test]
    fn recherche_plafonnee() -> Result<()> {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join("a.txt"), "x\n".repeat(MAX_HITS))?;
        let re = regex("x", false, false);
        let (hits, truncated) = cherche(d.path(), &re);
        assert_eq!((hits.len(), truncated), (MAX_HITS, false));
        fs::write(d.path().join("b.txt"), "x\n")?;
        let (hits, truncated) = cherche(d.path(), &re);
        assert_eq!((hits.len(), truncated), (MAX_HITS, true));
        Ok(())
    }

    #[test]
    fn recherche_colonne_utf16_et_premiere_occurrence() -> Result<()> {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join("a.txt"), "é😀 cible cible\r\ncible\n")?;
        let (hits, _) =
            search_text(d.path(), false, &regex("cible", false, false), || false).expect("finie");
        // é = 1 unité, 😀 = 2, l'espace = 1 ; une seule ligne de résultat pour deux occurrences.
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].location.start.character, 4);
        assert_eq!(hits[0].location.end.character, 9);
        assert_eq!(hits[1].location.start.line, 1);
        Ok(())
    }

    #[test]
    fn recherche_regex_et_casse() -> Result<()> {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join("a.txt"), "Cible\ncible\na.b\naxb\n")?;
        let lignes = |re| -> Vec<usize> { cherche(d.path(), &re).0.iter().map(|h| h.1).collect() };
        assert_eq!(lignes(regex("cible", false, false)), [0, 1]);
        assert_eq!(lignes(regex("cible", false, true)), [1]);
        assert_eq!(lignes(regex("a.b", false, false)), [2]);
        assert_eq!(lignes(regex("a.b", true, false)), [2, 3]);
        assert_eq!(lignes(regex("^cible$", true, true)), [1]);
        // Occurrence vide : pas un résultat, même si la ligne en a une vraie plus loin.
        assert_eq!(lignes(regex("x*", true, false)), [3]);
        Ok(())
    }

    #[test]
    fn recherche_interrompue_entre_deux_fichiers() -> Result<()> {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join("a.txt"), "x\n")?;
        fs::write(d.path().join("b.txt"), "x\n")?;
        let tests = std::cell::Cell::new(0);
        let stop = || {
            tests.set(tests.get() + 1);
            tests.get() == 2
        };
        assert!(search_text(d.path(), false, &regex("x", false, false), stop).is_none());
        Ok(())
    }

    #[test]
    fn ligne_geante_cherchee_jusqu_au_plafond_sans_couper_un_caractere() -> Result<()> {
        let d = tempfile::tempdir()?;
        let line = format!("{}début{}fin", "é".repeat(100), "é".repeat(MAX_LINE_BYTES));
        fs::write(d.path().join("a.js"), line)?;
        let re = |p| Regex::new(p).expect("regex");
        assert_eq!(cherche(d.path(), &re("début")).0.len(), 1);
        assert!(cherche(d.path(), &re("fin")).0.is_empty());
        Ok(())
    }

    #[test]
    fn marques_bidi_neutralisees_dans_l_extrait() {
        assert_eq!(excerpt("a\u{202e}b\u{2066}c", 0), "a b c");
    }

    #[test]
    fn extrait_borne_autour_de_l_occurrence() {
        // Indentation et tabulation.
        assert_eq!(excerpt("\t\tun\tdeux", 2), "un deux");
        // Occurrence commençant dans l'indentation.
        assert_eq!(excerpt("    x", 1), "x");
        // Ligne minifiée multioctet : ne coupe aucun caractère et montre l'occurrence.
        let ligne = "é".repeat(100_000) + "cible" + &"è".repeat(100_000);
        let e = excerpt(&ligne, ligne.find("cible").expect("présente"));
        let attendu = "…".to_owned() + &"é".repeat(20) + "cible" + &"è".repeat(175);
        assert_eq!(e, attendu);
    }

    #[test]
    fn branche_git() -> Result<()> {
        let d = tempfile::tempdir()?;
        assert_eq!(git_branch(d.path()), None);
        fs::create_dir(d.path().join(".git"))?;
        fs::write(d.path().join(".git/HEAD"), "ref: refs/heads/main\n")?;
        let sous = d.path().join("a/b");
        fs::create_dir_all(&sous)?;
        assert_eq!(git_branch(&sous), Some("main".into()));
        fs::write(
            d.path().join(".git/HEAD"),
            "4c6d0e0bb402b1f64f7ab533c2ead342f2f8d42d\n",
        )?;
        assert_eq!(git_branch(d.path()), Some("4c6d0e0".into()));
        // Worktree : `.git` est un fichier qui pointe ailleurs.
        let wt = tempfile::tempdir()?;
        fs::write(
            wt.path().join(".git"),
            format!("gitdir: {}\n", d.path().join(".git").display()),
        )?;
        assert_eq!(git_branch(wt.path()), Some("4c6d0e0".into()));
        Ok(())
    }
}
