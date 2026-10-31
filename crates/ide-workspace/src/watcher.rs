//! Surveillance du disque (F-05, NF-09) : changements sous la racine, regroupés et filtrés.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{Debouncer, new_debouncer};

/// Garde la surveillance active ; la lâcher l'arrête.
pub struct Watcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

/// Délai de regroupement : une sauvegarde ou un `git checkout` arrive en un seul appel.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Surveille `root` récursivement ; `on_change` reçoit, depuis un thread du watcher, les chemins changés
/// (hors `.git/`, `target/` et fichiers temporaires de notre sauvegarde atomique). Jamais de liste vide.
pub fn watch(
    root: &Path,
    on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
) -> Result<Watcher, String> {
    let root_owned = root.to_path_buf();
    let mut debouncer = new_debouncer(
        DEBOUNCE,
        move |res: notify_debouncer_mini::DebounceEventResult| {
            // Une erreur de surveillance isolée (dossier disparu…) n'arrête rien : rien à signaler ici.
            let Ok(events) = res else { return };
            let paths: Vec<PathBuf> = events
                .into_iter()
                .map(|e| e.path)
                .filter(|p| !ignored(&root_owned, p))
                .collect();
            if !paths.is_empty() {
                on_change(paths);
            }
        },
    )
    .map_err(|e| format!("surveillance impossible : {e}"))?;
    debouncer
        .watcher()
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| format!("surveillance de {} impossible : {e}", root.display()))?;
    Ok(Watcher {
        _debouncer: debouncer,
    })
}

/// `.git/`, `target/` sous la racine, ou `.tmpXXXXXX` (préfixe par défaut de `tempfile::NamedTempFile`,
/// utilisé par `save_atomic` de l'app).
fn ignored(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let in_skipped_dir = rel
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n == ".git" || n == "target"));
    let temp = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(".tmp"));
    in_skipped_dir || temp
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn filtre() {
        let r = Path::new("/r");
        assert!(ignored(r, Path::new("/r/.git/index")));
        assert!(ignored(r, Path::new("/r/sous/target/debug/x")));
        assert!(ignored(r, Path::new("/r/src/.tmpAb12Cd")));
        assert!(!ignored(r, Path::new("/r/src/main.rs")));
        assert!(!ignored(r, Path::new("/r/targets.txt")));
    }

    /// Attend un lot contenant `want`, en ≤ 2 s ; renvoie tous les chemins reçus.
    fn attendre(rx: &mpsc::Receiver<Vec<PathBuf>>, want: &Path) -> Vec<PathBuf> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut got = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(v) => {
                    got.extend(v);
                    if got.iter().any(|p| p == want) {
                        return got;
                    }
                }
                Err(_) => break,
            }
        }
        panic!("{} non signalé en 2 s : {got:?}", want.display());
    }

    #[test]
    fn creation_signalee_et_git_ignore() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().canonicalize()?;
        std::fs::create_dir(root.join(".git"))?;
        let (tx, rx) = mpsc::channel();
        let _w = watch(&root, move |paths| {
            let _ = tx.send(paths);
        })?;
        std::fs::write(root.join(".git/index"), "x")?;
        std::fs::write(root.join("a.txt"), "x")?;
        let got = attendre(&rx, &root.join("a.txt"));
        assert!(
            got.iter().all(|p| !p.starts_with(root.join(".git"))),
            "{got:?}"
        );
        Ok(())
    }
}
