//! Vue d'ensemble du projet : tous ses fichiers (palette Ctrl+P) et la branche git (F-70).

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// shortcut: liste plafonnée à ce nombre de fichiers ; passer à un parcours incrémental (nucleo injecteur) si besoin.
const MAX_FILES: usize = 100_000;

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
