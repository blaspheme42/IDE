//! Arborescence du dossier ouvert (F-01, F-03, F-04) : listage paresseux, dossier par dossier (NF-01).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// Entrée d'un dossier listé. `ide_core` aura son propre type miroir : garde des champs simples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Enfants directs de `dir`, dossiers d'abord puis fichiers, triés sans tenir compte de la casse.
///
/// Respecte `.gitignore`, `.ignore`, `.git/info/exclude` et l'exclusion git globale, y compris
/// ceux des dossiers parents quand `dir` est un sous-dossier. `.gitignore` s'applique aussi hors
/// dépôt git (`require_git(false)`) : c'est ce qu'attend l'utilisateur d'un IDE.
/// Cachés (nom commençant par `.`) masqués sauf si `show_hidden` ; `.git` toujours masqué.
/// Les liens symboliques ne sont pas suivis ; un lien vers un dossier est affiché comme dossier.
pub fn list_dir(dir: &Path, show_hidden: bool) -> Result<Vec<DirEntry>, String> {
    let walker = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .parents(true)
        .require_git(false)
        .hidden(!show_hidden)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    let mut entries = Vec::new();
    for item in walker {
        let entry = match item {
            Ok(e) => e,
            // Motif invalide dans un .gitignore : on l'ignore plutôt que de bloquer l'arbre.
            Err(e) => match e.io_error() {
                Some(io) => return Err(io_message("lecture du dossier impossible", io)),
                None => continue,
            },
        };
        if entry.depth() == 0 {
            continue;
        }
        let Some(ft) = entry.file_type() else {
            continue;
        };
        let path = entry.into_path();
        let is_dir = ft.is_dir() || (ft.is_symlink() && path.is_dir());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        entries.push(DirEntry { name, path, is_dir });
    }
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase(), e.name.clone()));
    Ok(entries)
}

/// Crée un fichier vide ; échoue s'il existe déjà. Crée les dossiers parents manquants.
pub fn create_file(path: &Path) -> Result<(), String> {
    create_parents(path)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(drop)
        .map_err(|e| io_message("création impossible", &e))
}

/// Crée un dossier ; échoue s'il existe déjà. Crée les dossiers parents manquants.
pub fn create_dir(path: &Path) -> Result<(), String> {
    create_parents(path)?;
    std::fs::create_dir(path).map_err(|e| io_message("création impossible", &e))
}

/// Renomme ou déplace `from` vers `to` ; refuse d'écraser une cible existante
/// et de déplacer un dossier dans lui-même.
pub fn rename(from: &Path, to: &Path) -> Result<(), String> {
    // shortcut: vérification puis renommage, pas atomique (renameat2 NOREPLACE), suffisant pour un IDE mono-utilisateur.
    if to.symlink_metadata().is_ok() {
        return Err("renommage refusé : la cible existe déjà".to_string());
    }
    // Le lien lui-même, pas sa cible : seul son dossier parent est résolu.
    let name = from
        .file_name()
        .ok_or("renommage impossible : chemin sans nom")?;
    let from_parent = match from.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let src = std::fs::canonicalize(from_parent)
        .map_err(|e| io_message("renommage impossible", &e))?
        .join(name);
    from.symlink_metadata()
        .map_err(|e| io_message("renommage impossible", &e))?;
    let dst_parent = match to.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dst_parent = std::fs::canonicalize(dst_parent)
        .map_err(|e| io_message("dossier de destination inaccessible", &e))?;
    if dst_parent.starts_with(&src) {
        return Err("déplacement refusé : un dossier ne peut pas aller dans lui-même".to_string());
    }
    std::fs::rename(from, to).map_err(|e| match e.kind() {
        ErrorKind::CrossesDevices => {
            "déplacement impossible : destination sur un autre disque".to_string()
        }
        _ => io_message("renommage impossible", &e),
    })
}

/// Envoie `path` à la corbeille du système ; jamais de suppression définitive.
pub fn trash(path: &Path) -> Result<(), String> {
    if path.symlink_metadata().is_err() {
        return Err("suppression impossible : introuvable".to_string());
    }
    trash::delete(path).map_err(|e| format!("corbeille indisponible : {e}"))
}

fn create_parents(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => {
            std::fs::create_dir_all(p).map_err(|e| io_message("création impossible", &e))
        }
        _ => Ok(()),
    }
}

fn io_message(action: &str, e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::NotFound => format!("{action} : introuvable"),
        ErrorKind::AlreadyExists => format!("{action} : existe déjà"),
        ErrorKind::PermissionDenied => format!("{action} : permission insuffisante"),
        _ => format!("{action} : {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Les erreurs `String` des fonctions testées passent aussi par `?`.
    type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// Message d'erreur ; vide si l'appel a réussi (l'assertion qui suit échoue alors).
    fn err<T>(r: std::result::Result<T, String>) -> String {
        r.err().unwrap_or_default()
    }

    fn names(dir: &Path, show_hidden: bool) -> Result<Vec<String>> {
        Ok(list_dir(dir, show_hidden)?
            .into_iter()
            .map(|e| e.name)
            .collect())
    }

    #[test]
    fn dossiers_d_abord_puis_tri_sans_casse() -> Result {
        let d = tempfile::tempdir()?;
        for f in ["b.txt", "A.txt", "c.txt"] {
            fs::write(d.path().join(f), "")?;
        }
        for sub in ["zeta", "Beta", "alpha"] {
            fs::create_dir(d.path().join(sub))?;
        }
        assert_eq!(
            names(d.path(), false)?,
            ["alpha", "Beta", "zeta", "A.txt", "b.txt", "c.txt"]
        );
        let first = &list_dir(d.path(), false)?[0];
        assert!(first.is_dir);
        assert_eq!(first.path, d.path().join("alpha"));
        Ok(())
    }

    #[test]
    fn gitignore_racine_et_parent_hors_depot() -> Result {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join(".gitignore"), "*.log\ncible/\n")?;
        fs::create_dir_all(d.path().join("src/cible"))?;
        fs::write(d.path().join("a.log"), "")?;
        fs::write(d.path().join("a.rs"), "")?;
        fs::write(d.path().join("src/b.log"), "")?;
        fs::write(d.path().join("src/b.rs"), "")?;
        assert_eq!(names(d.path(), false)?, ["src", "a.rs"]);
        // Le .gitignore du parent s'applique quand on liste le sous-dossier.
        assert_eq!(names(&d.path().join("src"), false)?, ["b.rs"]);
        Ok(())
    }

    #[test]
    fn gitignore_parent_dans_un_depot() -> Result {
        let d = tempfile::tempdir()?;
        fs::create_dir_all(d.path().join(".git"))?;
        fs::create_dir_all(d.path().join("src"))?;
        fs::write(d.path().join(".gitignore"), "*.tmp\n")?;
        fs::write(d.path().join("src/x.tmp"), "")?;
        fs::write(d.path().join("src/x.rs"), "")?;
        assert_eq!(names(&d.path().join("src"), false)?, ["x.rs"]);
        Ok(())
    }

    #[test]
    fn caches_masques_ou_affiches_git_toujours_masque() -> Result {
        let d = tempfile::tempdir()?;
        fs::create_dir(d.path().join(".git"))?;
        fs::create_dir(d.path().join(".config"))?;
        fs::write(d.path().join(".env"), "")?;
        fs::write(d.path().join("main.rs"), "")?;
        assert_eq!(names(d.path(), false)?, ["main.rs"]);
        assert_eq!(names(d.path(), true)?, [".config", ".env", "main.rs"]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lien_vers_dossier_affiche_comme_dossier() -> Result {
        let d = tempfile::tempdir()?;
        fs::create_dir(d.path().join("vrai"))?;
        std::os::unix::fs::symlink(d.path().join("vrai"), d.path().join("lien"))?;
        std::os::unix::fs::symlink(d.path().join("absent"), d.path().join("casse"))?;
        let got: Vec<_> = list_dir(d.path(), false)?
            .into_iter()
            .map(|e| (e.name, e.is_dir))
            .collect();
        assert_eq!(
            got,
            [
                ("lien".to_string(), true),
                ("vrai".to_string(), true),
                ("casse".to_string(), false)
            ]
        );
        Ok(())
    }

    #[test]
    fn dossier_introuvable_erreur() {
        let e = err(list_dir(Path::new("/nexiste/pas/du/tout"), false));
        assert!(e.contains("introuvable"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn dossier_illisible_erreur() -> Result {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir()?;
        let sub = d.path().join("fermé");
        fs::create_dir(&sub)?;
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o000))?;
        let got = list_dir(&sub, false);
        // root lit tout : rien à vérifier dans ce cas.
        let lisible = fs::read_dir(&sub).is_ok();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755))?;
        if !lisible {
            assert!(err(got).contains("permission"));
        }
        Ok(())
    }

    #[test]
    fn create_file_et_dir() -> Result {
        let d = tempfile::tempdir()?;
        let f = d.path().join("a/b/c.txt");
        create_file(&f)?;
        assert!(f.is_file());
        assert!(err(create_file(&f)).contains("existe déjà"));
        let sub = d.path().join("x/y/z");
        create_dir(&sub)?;
        assert!(sub.is_dir());
        assert!(err(create_dir(&sub)).contains("existe déjà"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn lien_vers_dossier_deplace_dans_sa_cible() -> Result {
        // Le lien est déplacé, pas sa cible : aller dans `vrai/` n'est pas « dans lui-même ».
        let d = tempfile::tempdir()?;
        let vrai = d.path().join("vrai");
        fs::create_dir(&vrai)?;
        let lien = d.path().join("lien");
        std::os::unix::fs::symlink(&vrai, &lien)?;
        rename(&lien, &vrai.join("lien2"))?;
        assert!(
            vrai.join("lien2")
                .symlink_metadata()?
                .file_type()
                .is_symlink()
        );
        assert!(vrai.is_dir());
        Ok(())
    }

    #[test]
    fn rename_et_deplacement() -> Result {
        let d = tempfile::tempdir()?;
        let a = d.path().join("a.txt");
        let b = d.path().join("b.txt");
        fs::write(&a, "a")?;
        fs::write(&b, "b")?;
        assert!(err(rename(&a, &b)).contains("existe déjà"));
        assert_eq!(fs::read_to_string(&b)?, "b");
        let dir = d.path().join("dir");
        fs::create_dir(&dir)?;
        rename(&a, &dir.join("a.txt"))?;
        assert!(!a.exists());
        assert_eq!(fs::read_to_string(dir.join("a.txt"))?, "a");
        Ok(())
    }

    #[test]
    fn dossier_dans_lui_meme_refuse() -> Result {
        let d = tempfile::tempdir()?;
        let dir = d.path().join("dir");
        fs::create_dir_all(dir.join("sous"))?;
        assert!(err(rename(&dir, &dir.join("dir"))).contains("lui-même"));
        assert!(err(rename(&dir, &dir.join("sous/dir"))).contains("lui-même"));
        // Un voisin au nom préfixé n'est pas « dans » le dossier.
        rename(&dir, &d.path().join("dir2"))?;
        Ok(())
    }

    #[test]
    fn trash_introuvable_erreur() {
        let e = err(trash(Path::new("/nexiste/pas/du/tout")));
        assert!(e.contains("introuvable"), "{e}");
    }

    /// Remplit la corbeille de la machine : à lancer à la main.
    #[test]
    #[ignore]
    fn trash_nominal() -> Result {
        let d = tempfile::tempdir()?;
        let f = d.path().join("ide-test-corbeille.txt");
        fs::write(&f, "")?;
        trash(&f)?;
        assert!(!f.exists());
        Ok(())
    }

    /// NF-01 : `cargo test --release -p ide-workspace -- --ignored --nocapture liste_5000`.
    #[test]
    #[ignore]
    fn liste_5000_entrees_rapide() -> Result {
        let d = tempfile::tempdir()?;
        fs::write(d.path().join(".gitignore"), "*.log\n")?;
        for i in 0..5000 {
            fs::write(d.path().join(format!("f{i}.rs")), "")?;
        }
        let t = std::time::Instant::now();
        let n = list_dir(d.path(), false)?.len();
        let dt = t.elapsed();
        println!("list_dir 5000 entrées : {dt:?}");
        assert_eq!(n, 5000);
        assert!(dt.as_millis() < 100, "{dt:?}");
        Ok(())
    }
}
