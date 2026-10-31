//! Session (F-80) : onglets, curseurs et onglet actif, écrits à la sortie normale dans
//! `<état>/ide/session/<empreinte de la racine>.json`, relus au démarrage sans fichier en argument.

use std::path::{Path, PathBuf};

use ide_core::State;
use serde_json::{Value, json};

/// Onglet mémorisé : chemin, curseur (ligne, colonne en chars), première ligne visible.
pub type TabView = (PathBuf, (usize, usize), usize);

pub fn file(dir: &Path, root: &Path) -> PathBuf {
    dir.join(format!("{}.json", crate::backup::fingerprint(root)))
}

/// Onglets avec chemin, dans l'ordre d'affichage.
/// shortcut: chemins non UTF-8 non mémorisés (JSON) ; à revoir si un utilisateur en a.
pub fn save(file: &Path, state: &State) -> std::io::Result<()> {
    let mut views: Vec<(&ide_core::Document, usize)> =
        state.tabs.iter().map(|t| (&t.doc, t.scroll_line)).collect();
    views.insert(
        state.active.min(views.len()),
        (&state.doc, state.scroll_line),
    );
    let mut active = Value::Null;
    let mut tabs = vec![];
    for (i, (doc, scroll)) in views.into_iter().enumerate() {
        let Some(path) = doc.path.as_deref().and_then(Path::to_str) else {
            continue;
        };
        let rope = doc.rope();
        let line = rope.char_to_line(doc.cursor());
        let col = doc.cursor() - rope.line_to_char(line);
        if i == state.active {
            active = json!(path);
        }
        tabs.push(json!({ "path": path, "line": line, "col": col, "scroll_line": scroll }));
    }
    let text = json!({ "tabs": tabs, "active": active }).to_string();
    crate::files::save_atomic(file, &text)
}

/// Onglets encore présents sur le disque, et chemin de l'actif. Session absente ou illisible : vide.
pub fn load(file: &Path) -> (Vec<TabView>, Option<PathBuf>) {
    let Some(v) = std::fs::read_to_string(file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    else {
        return (vec![], None);
    };
    let num = |t: &Value, k: &str| {
        t[k].as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0)
    };
    let tabs = v["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let path = PathBuf::from(t["path"].as_str()?);
            path.is_file()
                .then(|| (path, (num(t, "line"), num(t, "col")), num(t, "scroll_line")))
        })
        .collect();
    (tabs, v["active"].as_str().map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Event, Key, KeyInput, Modifiers, update};

    #[test]
    fn aller_retour_et_onglet_disparu_ignore() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let (a, b, c) = (
            tmp.path().join("a.txt"),
            tmp.path().join("b.txt"),
            tmp.path().join("c.txt"),
        );
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 20,
            },
        );
        for p in [&a, &b, &c] {
            std::fs::write(p, "un\ndeux\ntrois")?;
            update(
                &mut s,
                Event::FileOpened {
                    path: p.clone(),
                    text: "un\ndeux\ntrois".into(),
                },
            );
        }
        // b.txt actif, curseur ligne 1 colonne 2.
        update(&mut s, Event::ActivateTab(b.clone()));
        for key in [Key::Down, Key::Right, Key::Right] {
            update(
                &mut s,
                Event::Key(KeyInput {
                    key,
                    mods: Modifiers::default(),
                }),
            );
        }
        let f = file(&tmp.path().join("session"), tmp.path());
        save(&f, &s)?;
        std::fs::remove_file(&a)?;
        let (tabs, active) = load(&f);
        assert_eq!(tabs, vec![(b.clone(), (1, 2), 0), (c, (0, 0), 0)]);
        assert_eq!(active, Some(b));
        assert_eq!(load(&tmp.path().join("absent.json")), (vec![], None));
        Ok(())
    }
}
