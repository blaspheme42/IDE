//! I/O fichiers : lecture stricte UTF-8 et écriture atomique (NF-06).

use std::io::{ErrorKind, Write};
use std::path::Path;

/// Message renvoyé quand le fichier n'existe pas : le cœur garde le chemin pour le créer.
pub const NEW_FILE: &str = "nouveau fichier";

/// Lit un fichier texte ; refuse le non-UTF-8 plutôt que de le corrompre à la sauvegarde.
pub fn read_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => NEW_FILE.to_string(),
        ErrorKind::PermissionDenied => "lecture refusée : permission insuffisante".to_string(),
        ErrorKind::IsADirectory => "c'est un dossier, pas un fichier".to_string(),
        _ => format!("lecture impossible : {e}"),
    })?;
    String::from_utf8(bytes)
        .map_err(|_| "fichier non UTF-8 : ouverture refusée pour ne pas le corrompre".to_string())
}

/// Écrit dans un fichier temporaire du même dossier puis le renomme sur `path` :
/// en cas d'échec, l'original reste intact.
pub fn save_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    // Suit un lien symbolique pour remplacer sa cible et non le lien lui-même.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    if let Ok(meta) = std::fs::metadata(&path) {
        tmp.as_file().set_permissions(meta.permissions())?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(&path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecrit_le_contenu_et_cree_le_fichier() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("a.txt");
        save_atomic(&p, "bonjour\n")?;
        assert_eq!(std::fs::read_to_string(&p)?, "bonjour\n");
        save_atomic(&p, "remplacé")?;
        assert_eq!(std::fs::read_to_string(&p)?, "remplacé");
        // Aucun fichier temporaire ne traîne.
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn crlf_ecrit_tel_quel() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("w.txt");
        save_atomic(&p, "a\r\nb\r\n")?;
        assert_eq!(std::fs::read(&p)?, b"a\r\nb\r\n");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn preserve_les_permissions() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("x.sh");
        std::fs::write(&p, "old")?;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o750))?;
        save_atomic(&p, "new")?;
        let mode = std::fs::metadata(&p)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o750);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn dossier_non_inscriptible_laisse_l_original_intact() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "original")?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))?;
        // Sous root le dossier reste inscriptible : le test n'a alors rien à prouver.
        let root = std::fs::write(dir.path().join("sonde"), "").is_ok();
        let res = save_atomic(&p, "nouveau");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))?;
        if root {
            return Ok(());
        }
        assert!(res.is_err());
        assert_eq!(std::fs::read_to_string(&p)?, "original");
        Ok(())
    }

    #[test]
    fn lecture_inexistant_et_non_utf8() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(
            read_file(&dir.path().join("absent")),
            Err(NEW_FILE.to_string())
        );
        let p = dir.path().join("bin");
        std::fs::write(&p, [0xff, 0xfe, 0x00])?;
        assert!(read_file(&p).err().unwrap_or_default().contains("UTF-8"));
        assert!(read_file(dir.path()).is_err());
        std::fs::write(&p, "é\r\n")?;
        assert_eq!(read_file(&p), Ok("é\r\n".to_string()));
        Ok(())
    }
}
