//! Marques git de la gouttière : version HEAD d'un fichier, et diff par ligne avec le texte courant.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use ide_core::GitMark;
use similar::{DiffOp, TextDiff};

/// Au-delà (base + texte, en octets), pas de marques : le diff coûterait trop cher à chaque édition.
const MAX_BYTES: usize = 4 << 20;

/// Texte de `path` dans HEAD. `None` = pas de marques (hors dépôt, git absent, fichier ignoré,
/// contenu non UTF-8) ; `Some("")` = fichier non suivi d'un dépôt (tout est ajouté).
pub fn git_base(path: &Path) -> Option<String> {
    let (dir, name) = (path.parent()?, path.file_name()?);
    let git = |args: &[&OsStr]| -> Option<Output> {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
    };
    // `HEAD:./nom` : chemin relatif au dossier de `-C`, pas à la racine du dépôt.
    let mut spec = OsString::from("HEAD:./");
    spec.push(name);
    let show = git(&["show".as_ref(), &spec])?;
    if show.status.success() {
        return String::from_utf8(show.stdout).ok();
    }
    let inside = git(&["rev-parse".as_ref(), "--is-inside-work-tree".as_ref()])?;
    if !inside.status.success() || !inside.stdout.starts_with(b"true") {
        return None;
    }
    let ignored = git(&["check-ignore".as_ref(), "-q".as_ref(), name])?;
    (!ignored.status.success()).then(String::new)
}

/// Marque de chaque ligne de `current` (autant que de lignes ropey : la ligne vide après le dernier
/// `\n` compte) par rapport à `base`. Vide si les textes sont trop gros.
pub fn git_marks(base: &str, current: &str) -> Vec<Option<GitMark>> {
    if base.len() + current.len() > MAX_BYTES {
        return Vec::new();
    }
    // Sans `\n` final, la dernière ligne différerait de la même ligne suivie d'autres.
    let terminated = |s: &str| {
        if s.is_empty() || s.ends_with('\n') {
            s.to_owned()
        } else {
            format!("{s}\n")
        }
    };
    let (old, new) = (terminated(base), terminated(current));
    let diff = TextDiff::configure()
        .deadline(Instant::now() + Duration::from_millis(50))
        .diff_lines(&old, &new);
    let mut marks = vec![None; current.matches('\n').count() + 1];
    for op in diff.ops() {
        let (mark, at, len) = match *op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Insert {
                new_index, new_len, ..
            } => (GitMark::Added, new_index, new_len),
            DiffOp::Replace {
                new_index, new_len, ..
            } => (GitMark::Modified, new_index, new_len),
            DiffOp::Delete { new_index, .. } => {
                let (line, mark) = match new_index.checked_sub(1) {
                    Some(prev) => (prev, GitMark::DeletedBelow),
                    None => (0, GitMark::DeletedAbove),
                };
                if let Some(m @ None) = marks.get_mut(line) {
                    *m = Some(mark);
                }
                continue;
            }
        };
        for m in marks.iter_mut().skip(at).take(len) {
            *m = Some(mark);
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use GitMark::*;

    #[test]
    fn ajout_modification_suppression() {
        let base = "a\nb\nc\nd\n";
        assert_eq!(git_marks(base, base), [None; 5]);
        assert_eq!(
            git_marks(base, "a\nX\nb\nc\nd\n"),
            [None, Some(Added), None, None, None, None]
        );
        assert_eq!(
            git_marks(base, "a\nB\nc\nd\n"),
            [None, Some(Modified), None, None, None]
        );
        assert_eq!(git_marks(base, "a\nd\n"), [Some(DeletedBelow), None, None]);
        assert_eq!(
            git_marks(base, "b\nc\nd\n"),
            [Some(DeletedAbove), None, None, None]
        );
        // Dernière ligne sans `\n` : pas une modification.
        assert_eq!(git_marks(base, "a\nb\nc\nd"), [None; 4]);
    }

    #[test]
    fn fichier_non_suivi_tout_ajoute() {
        assert_eq!(git_marks("", "x\ny\n"), [Some(Added), Some(Added), None]);
        assert_eq!(git_marks("", ""), [None]);
        assert_eq!(git_marks("a\n", ""), [Some(DeletedAbove)]);
    }

    #[test]
    fn trop_gros_aucune_marque() {
        assert!(git_marks(&"x".repeat(MAX_BYTES), "y").is_empty());
    }

    /// git lancé dans `dir`, identité fixée pour le commit ; `false` si git est absent.
    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn base_depuis_head() -> Result<(), Box<dyn std::error::Error>> {
        let d = tempfile::tempdir()?;
        let (suivi, neuf) = (d.path().join("sous/a.txt"), d.path().join("sous/b.txt"));
        std::fs::create_dir(d.path().join("sous"))?;
        std::fs::write(&suivi, "v1\n")?;
        // Hors dépôt : pas de marques.
        assert_eq!(git_base(&suivi), None);
        if !git(d.path(), &["init", "-q"]) {
            return Ok(()); // git absent : rien d'autre à vérifier.
        }
        std::fs::write(d.path().join(".gitignore"), "*.log\n")?;
        assert!(git(d.path(), &["add", "."]));
        assert!(git(d.path(), &["commit", "-qm", "init"]));
        std::fs::write(&suivi, "v2\n")?;
        std::fs::write(&neuf, "x\n")?;
        std::fs::write(d.path().join("sous/c.log"), "x\n")?;
        assert_eq!(git_base(&suivi).as_deref(), Some("v1\n"));
        assert_eq!(git_base(&neuf).as_deref(), Some(""));
        assert_eq!(git_base(&d.path().join("sous/c.log")), None);
        Ok(())
    }
}
