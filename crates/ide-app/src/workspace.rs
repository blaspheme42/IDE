//! Commandes de l'explorateur exécutées hors de la boucle : listage, ouverture, opérations disque.

use std::path::{Path, PathBuf};

use ide_core::{Command, Event, FsChange, FsEntry, Indent};
use tokio::sync::mpsc;

use crate::files;

/// Exécute `cmd` (ListDir, OpenFile, CreateFile, CreateDir, Rename, Trash) en tâche de fond ;
/// le résultat revient comme un événement sur `tx`.
pub fn spawn(cmd: Command, tx: mpsc::Sender<Event>) {
    let copy = cmd.clone();
    tokio::spawn(async move {
        // Une panique dans la tâche devient une erreur affichée, pas une sortie de l'app.
        let events = tokio::task::spawn_blocking(move || execute(cmd))
            .await
            .unwrap_or_else(|e| vec![failure(copy, format!("tâche interrompue : {e}"))]);
        // Envoyés dans l'ordre depuis la même tâche : FileOpened précède IndentDetected.
        for ev in events {
            if tx.send(ev).await.is_err() {
                break;
            }
        }
    });
}

/// Partie synchrone de `spawn` : les événements à renvoyer, dans l'ordre.
fn execute(cmd: Command) -> Vec<Event> {
    let done = |r: Result<(), String>, change: FsChange| vec![Event::FsDone(r.map(|()| change))];
    match cmd {
        Command::ListDir { dir, show_hidden } => {
            let result = ide_workspace::list_dir(&dir, show_hidden).map(|v| {
                v.into_iter()
                    .map(|e| FsEntry {
                        name: e.name,
                        path: e.path,
                        is_dir: e.is_dir,
                    })
                    .collect()
            });
            vec![Event::DirListed { dir, result }]
        }
        Command::OpenFile(path) => open_file(path),
        Command::CreateFile(p) => done(ide_workspace::create_file(&p), FsChange::Created(p)),
        Command::CreateDir(p) => done(ide_workspace::create_dir(&p), FsChange::Created(p)),
        Command::Rename { from, to } => done(
            ide_workspace::rename(&from, &to),
            FsChange::Renamed { from, to },
        ),
        Command::Trash(p) => done(ide_workspace::trash(&p), FsChange::Trashed(p)),
        Command::Quit
        | Command::Save { .. }
        | Command::CopyToClipboard(_)
        | Command::RequestPaste => {
            vec![]
        }
    }
}

/// Événement d'échec de `cmd`, quand sa tâche a paniqué.
fn failure(cmd: Command, error: String) -> Event {
    match cmd {
        Command::ListDir { dir, .. } => Event::DirListed {
            dir,
            result: Err(error),
        },
        Command::OpenFile(path) => Event::OpenFailed {
            path,
            error,
            create: false,
        },
        _ => Event::FsDone(Err(error)),
    }
}

/// Lit `path` et son style `.editorconfig` : FileOpened (ou OpenFailed) puis IndentDetected,
/// qui doit suivre l'ouverture puisque celle-ci remet le document à zéro.
pub fn open_file(path: PathBuf) -> Vec<Event> {
    let style = ide_workspace::indent_style(&path);
    let opened = match files::read_file(&path) {
        Ok(text) => Event::FileOpened { path, text },
        Err(error) => Event::OpenFailed {
            create: error == files::NEW_FILE,
            path,
            error,
        },
    };
    let indent = Indent {
        use_tabs: style.use_tabs,
        width: style.width,
        tab_width: style.tab_width,
    };
    vec![opened, Event::IndentDetected(indent)]
}

/// Racine de l'explorateur et fichier à ouvrir pour `ide [dossier|fichier]` ;
/// `cwd` est le dossier courant canonique.
pub fn choose_root(arg: Option<&Path>, cwd: &Path) -> (PathBuf, Option<PathBuf>) {
    let Some(arg) = arg else {
        return (cwd.to_path_buf(), None);
    };
    let abs = cwd.join(arg);
    if abs.is_dir() {
        return (abs.canonicalize().unwrap_or(abs), None);
    }
    // Fichier inexistant : on canonicalise son parent pour le comparer à `cwd`.
    let file = abs.canonicalize().ok().or_else(|| {
        let parent = abs.parent()?.canonicalize().ok()?;
        Some(parent.join(abs.file_name()?))
    });
    match file {
        Some(f) if !f.starts_with(cwd) => {
            let parent = f
                .parent()
                .map_or_else(|| cwd.to_path_buf(), Path::to_path_buf);
            (parent, Some(f))
        }
        Some(f) => (cwd.to_path_buf(), Some(f)),
        None => (cwd.to_path_buf(), Some(abs)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Passe par `spawn` et le canal, comme dans l'app.
    async fn run(cmd: Command, n: usize) -> Vec<Event> {
        let (tx, mut rx) = mpsc::channel(8);
        spawn(cmd, tx);
        let mut out = Vec::new();
        while let Some(ev) = rx.recv().await {
            out.push(ev);
        }
        assert_eq!(out.len(), n);
        out
    }

    #[tokio::test]
    async fn listage_converti_en_fs_entry() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("a.rs"), "")?;
        std::fs::create_dir(dir.path().join("src"))?;
        let d = dir.path().to_path_buf();
        let evs = run(
            Command::ListDir {
                dir: d.clone(),
                show_hidden: false,
            },
            1,
        )
        .await;
        let Event::DirListed { dir: got, result } = &evs[0] else {
            panic!("{evs:?}");
        };
        assert_eq!(got, &d);
        let mut entries = result.clone().map_err(anyhow::Error::msg)?;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            entries,
            vec![
                FsEntry {
                    name: "a.rs".into(),
                    path: d.join("a.rs"),
                    is_dir: false
                },
                FsEntry {
                    name: "src".into(),
                    path: d.join("src"),
                    is_dir: true
                },
            ]
        );
        let evs = run(
            Command::ListDir {
                dir: d.join("absent"),
                show_hidden: false,
            },
            1,
        )
        .await;
        assert!(matches!(&evs[0], Event::DirListed { result: Err(_), .. }));
        Ok(())
    }

    #[tokio::test]
    async fn ouverture_puis_indentation() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("a.txt");
        std::fs::write(&p, "salut")?;
        let evs = run(Command::OpenFile(p.clone()), 2).await;
        assert_eq!(
            evs[0],
            Event::FileOpened {
                path: p,
                text: "salut".into()
            }
        );
        assert!(matches!(evs[1], Event::IndentDetected(_)));
        Ok(())
    }

    #[tokio::test]
    async fn echec_d_ouverture_create_ou_non() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let absent = dir.path().join("neuf.txt");
        let evs = run(Command::OpenFile(absent.clone()), 2).await;
        assert!(matches!(&evs[0], Event::OpenFailed { path, create: true, .. } if *path == absent));
        assert!(matches!(evs[1], Event::IndentDetected(_)));
        let bin = dir.path().join("bin");
        std::fs::write(&bin, [0xff, 0xfe])?;
        let evs = run(Command::OpenFile(bin), 2).await;
        assert!(matches!(&evs[0], Event::OpenFailed { create: false, .. }));
        Ok(())
    }

    #[tokio::test]
    async fn operations_disque() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let f = dir.path().join("f.txt");
        let d = dir.path().join("d");
        let g = dir.path().join("g.txt");
        let evs = run(Command::CreateFile(f.clone()), 1).await;
        assert_eq!(evs[0], Event::FsDone(Ok(FsChange::Created(f.clone()))));
        assert!(f.is_file());
        let evs = run(Command::CreateFile(f.clone()), 1).await;
        assert!(matches!(evs[0], Event::FsDone(Err(_))));
        let evs = run(Command::CreateDir(d.clone()), 1).await;
        assert_eq!(evs[0], Event::FsDone(Ok(FsChange::Created(d.clone()))));
        assert!(d.is_dir());
        let evs = run(
            Command::Rename {
                from: f.clone(),
                to: g.clone(),
            },
            1,
        )
        .await;
        assert_eq!(
            evs[0],
            Event::FsDone(Ok(FsChange::Renamed {
                from: f.clone(),
                to: g.clone()
            }))
        );
        assert!(g.is_file() && !f.exists());
        // Pas de vraie corbeille en test : seulement l'échec.
        let evs = run(Command::Trash(f), 1).await;
        assert!(matches!(evs[0], Event::FsDone(Err(_))));
        Ok(())
    }

    #[test]
    fn panique_convertie_en_erreur() {
        let e = || "boum".to_string();
        assert!(matches!(
            failure(Command::OpenFile("x".into()), e()),
            Event::OpenFailed { create: false, .. }
        ));
        assert!(matches!(
            failure(Command::Trash("x".into()), e()),
            Event::FsDone(Err(_))
        ));
        assert!(matches!(
            failure(
                Command::ListDir {
                    dir: "x".into(),
                    show_hidden: true
                },
                e()
            ),
            Event::DirListed { result: Err(_), .. }
        ));
    }

    #[test]
    fn choix_de_la_racine() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let cwd = base.join("cwd");
        let ailleurs = base.join("ailleurs");
        std::fs::create_dir_all(cwd.join("sous"))?;
        std::fs::create_dir(&ailleurs)?;
        std::fs::write(cwd.join("sous/a.rs"), "")?;
        std::fs::write(ailleurs.join("b.rs"), "")?;
        let r = |a: &str| choose_root(Some(Path::new(a)), &cwd);

        assert_eq!(choose_root(None, &cwd), (cwd.clone(), None));
        assert_eq!(r("sous"), (cwd.join("sous"), None));
        assert_eq!(r("../ailleurs"), (ailleurs.clone(), None));
        assert_eq!(r("sous/a.rs"), (cwd.clone(), Some(cwd.join("sous/a.rs"))));
        assert_eq!(
            r("../ailleurs/b.rs"),
            (ailleurs.clone(), Some(ailleurs.join("b.rs")))
        );
        // Inexistants : parent existant hors cwd, dans cwd, ou parent absent.
        assert_eq!(
            r("../ailleurs/neuf.rs"),
            (ailleurs.clone(), Some(ailleurs.join("neuf.rs")))
        );
        assert_eq!(r("neuf.rs"), (cwd.clone(), Some(cwd.join("neuf.rs"))));
        assert_eq!(
            r("/nulle/part/x.rs"),
            (cwd.clone(), Some(PathBuf::from("/nulle/part/x.rs")))
        );
        Ok(())
    }
}
