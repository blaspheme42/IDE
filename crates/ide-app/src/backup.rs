//! Copies de secours des fichiers non sauvegardés (NF-07) : `<état>/ide/backup/<empreinte>.bak`,
//! première ligne = chemin du fichier, puis son texte `to_saved_text()`.
//! shortcut: documents sans chemin ignorés (rien pour les retrouver au redémarrage) ; à couvrir si on ajoute « nouveau fichier ».

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use ide_core::{Document, Event, State};
use tokio::sync::mpsc;

use crate::files;

/// Période des copies.
pub const PERIOD: std::time::Duration = std::time::Duration::from_secs(30);

/// FNV-1a 64 bits : empreinte stable entre versions de Rust (`DefaultHasher` ne l'est pas).
pub fn fingerprint(path: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_os_str().as_encoded_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Travail disque, exécuté dans l'ordre par `worker`.
#[derive(Debug, PartialEq, Eq)]
pub enum Job {
    Write { file: PathBuf, contents: String },
    Remove(PathBuf),
}

pub fn run(job: Job) {
    let res = match &job {
        Job::Write { file, contents } => files::save_atomic(file, contents),
        Job::Remove(file) => match std::fs::remove_file(file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    };
    if let Err(e) = res {
        tracing::warn!("copie de secours {job:?} : {e}");
    }
}

/// Une tâche, dans l'ordre : une suppression ne peut pas passer avant l'écriture qu'elle annule.
pub async fn worker(mut jobs: mpsc::UnboundedReceiver<Job>) {
    while let Some(job) = jobs.recv().await {
        if let Err(e) = tokio::task::spawn_blocking(move || run(job)).await {
            tracing::error!("copie de secours : {e}");
        }
    }
}

/// Copies trouvées au démarrage dont le texte diffère du disque (fichier disparu compris), triées
/// par chemin ; celles identiques au disque sont supprimées.
pub fn scan(dir: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut found = vec![];
    for file in entries.flatten().map(|e| e.path()) {
        if file.extension().is_none_or(|e| e != "bak") {
            continue;
        }
        let Some((path, text)) = std::fs::read_to_string(&file).ok().and_then(|s| {
            s.split_once('\n')
                .map(|(p, t)| (PathBuf::from(p), t.to_string()))
        }) else {
            tracing::warn!("copie de secours illisible : {}", file.display());
            continue;
        };
        if files::read_file(&path).is_ok_and(|disk| disk == text) {
            run(Job::Remove(file));
        } else {
            found.push((path, text));
        }
    }
    found.sort();
    found
}

pub struct Backups {
    dir: PathBuf,
    /// Copies dont cette session a la charge (écrites, ou proposées puis restaurées), et la version
    /// du document copiée (`u64::MAX` : pas encore réécrite).
    written: HashMap<PathBuf, u64>,
    /// Copies trouvées au démarrage pas encore proposées : le cœur n'en garde qu'une à la fois.
    offers: VecDeque<(PathBuf, String)>,
}

fn docs(state: &State) -> impl Iterator<Item = &Document> {
    std::iter::once(&state.doc).chain(state.tabs.iter().map(|t| &t.doc))
}

impl Backups {
    pub fn new(dir: PathBuf, offers: Vec<(PathBuf, String)>) -> Self {
        Self {
            dir,
            written: HashMap::new(),
            offers: offers.into(),
        }
    }

    fn file(&self, path: &Path) -> PathBuf {
        self.dir.join(format!("{}.bak", fingerprint(path)))
    }

    /// Copie proposée ou en attente : on ne l'écrase pas avant le choix de l'utilisateur.
    fn pending(&self, state: &State, path: &Path) -> bool {
        state.backup_offer.as_ref().is_some_and(|(p, _)| p == path)
            || self.offers.iter().any(|(p, _)| p == path)
    }

    /// Toutes les 30 s : copie de chaque document modifié dont la version a changé.
    pub fn tick(&mut self, state: &State) -> Vec<Job> {
        let mut jobs = vec![];
        for doc in docs(state).filter(|d| d.is_dirty()) {
            let Some(path) = &doc.path else { continue };
            if self.written.get(path) == Some(&doc.version()) || self.pending(state, path) {
                continue;
            }
            self.written.insert(path.clone(), doc.version());
            let contents = format!("{}\n{}", path.to_string_lossy(), doc.to_saved_text());
            let file = self.file(path);
            jobs.push(Job::Write { file, contents });
        }
        jobs
    }

    /// Après chaque événement : supprime les copies des documents redevenus propres ou fermés
    /// (même modifiés : Ctrl+W confirmé est un choix explicite).
    pub fn cleanup(&mut self, state: &State) -> Vec<Job> {
        let dirty: Vec<&Path> = docs(state)
            .filter(|d| d.is_dirty())
            .filter_map(|d| d.path.as_deref())
            .collect();
        let gone: Vec<PathBuf> = self
            .written
            .keys()
            .filter(|p| !dirty.contains(&p.as_path()) && !self.pending(state, p))
            .cloned()
            .collect();
        gone.into_iter()
            .map(|p| {
                self.written.remove(&p);
                Job::Remove(self.file(&p))
            })
            .collect()
    }

    /// Alt+N.
    pub fn discard(&mut self, path: &Path) -> Job {
        self.written.remove(path);
        Job::Remove(self.file(path))
    }

    /// Sortie normale (Ctrl+Q confirmé) : l'utilisateur a renoncé aux modifications. Les copies
    /// trouvées au démarrage sans réponse restent, elles seront reproposées.
    pub fn quit(&mut self, state: &State) -> Vec<Job> {
        let paths: Vec<PathBuf> = (self.written.keys())
            .filter(|p| !self.pending(state, p))
            .cloned()
            .collect();
        paths
            .into_iter()
            .map(|p| {
                self.written.remove(&p);
                Job::Remove(self.file(&p))
            })
            .collect()
    }

    /// Prochaine copie à proposer, une fois la précédente réglée et son onglet ouvert.
    pub fn next_offer(&mut self, state: &State) -> Option<Event> {
        if state.backup_offer.is_some() {
            return None;
        }
        let open = |p: &Path| docs(state).any(|d| d.path.as_deref() == Some(p));
        let i = self.offers.iter().position(|(p, _)| open(p))?;
        let (path, text) = self.offers.remove(i)?;
        // Restaurée (Alt+B), elle est à nous : supprimée à la sortie ou quand le document redevient propre.
        self.written.insert(path.clone(), u64::MAX);
        Some(Event::BackupFound { path, text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Key, KeyInput, Modifiers, update};

    fn ouvert(path: &Path, text: &str) -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        );
        s
    }
    fn tape(s: &mut State, c: char) {
        update(
            s,
            Event::Key(KeyInput {
                key: Key::Char(c),
                mods: Modifiers::default(),
            }),
        );
    }
    fn applique(jobs: Vec<Job>) -> usize {
        let n = jobs.len();
        jobs.into_iter().for_each(run);
        n
    }

    #[test]
    fn empreinte_stable() {
        // Valeurs de référence FNV-1a 64 : ne doivent jamais changer (copies d'une version précédente).
        assert_eq!(fingerprint(Path::new("")), "cbf29ce484222325");
        assert_eq!(fingerprint(Path::new("a")), "af63dc4c8601ec8c");
    }

    #[test]
    fn copie_si_modifie_une_fois_par_version_supprimee_apres_sauvegarde() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("backup");
        let p = tmp.path().join("a.txt");
        std::fs::write(&p, "disque")?;
        let mut b = Backups::new(dir.clone(), vec![]);
        let mut s = ouvert(&p, "disque");
        assert_eq!(applique(b.tick(&s)), 0, "document propre : pas de copie");
        tape(&mut s, 'x');
        assert_eq!(applique(b.tick(&s)), 1);
        let file = dir.join(format!("{}.bak", fingerprint(&p)));
        let attendu = format!("{}\nxdisque", p.display());
        assert_eq!(std::fs::read_to_string(&file)?, attendu);
        assert_eq!(applique(b.tick(&s)), 0, "même version : pas de réécriture");
        assert_eq!(applique(b.cleanup(&s)), 0);
        // Au redémarrage, la copie diffère du disque : proposée.
        assert_eq!(scan(&dir), vec![(p.clone(), "xdisque".into())]);
        // Sauvegardé : document propre, copie supprimée.
        let v = s.doc.version();
        update(
            &mut s,
            Event::Saved {
                path: p,
                version: v,
            },
        );
        assert_eq!(applique(b.cleanup(&s)), 1);
        assert!(!file.exists());
        Ok(())
    }

    #[test]
    fn fermeture_et_sortie_suppriment_les_copies() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("backup");
        let p = tmp.path().join("a.txt");
        let mut b = Backups::new(dir.clone(), vec![]);
        let mut s = ouvert(&p, "");
        tape(&mut s, 'x');
        applique(b.tick(&s));
        assert_eq!(std::fs::read_dir(&dir)?.count(), 1);
        applique(b.quit(&s));
        assert_eq!(std::fs::read_dir(&dir)?.count(), 0);
        // Ctrl+W confirmé sur un document modifié.
        tape(&mut s, 'y');
        applique(b.tick(&s));
        assert_eq!(std::fs::read_dir(&dir)?.count(), 1);
        // Onglet fermé (seul document restant : vierge).
        applique(b.cleanup(&State::default()));
        assert_eq!(std::fs::read_dir(&dir)?.count(), 0);
        Ok(())
    }

    #[test]
    fn lecture_au_demarrage_et_offres_une_a_une() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("backup");
        std::fs::create_dir(&dir)?;
        let (a, b, c) = (
            tmp.path().join("a.txt"),
            tmp.path().join("b.txt"),
            tmp.path().join("disparu.txt"),
        );
        std::fs::write(&a, "pareil")?;
        std::fs::write(&b, "disque")?;
        for (p, t) in [(&a, "pareil"), (&b, "copie\r\n"), (&c, "orphelin")] {
            let file = dir.join(format!("{}.bak", fingerprint(p)));
            std::fs::write(file, format!("{}\n{t}", p.display()))?;
        }
        std::fs::write(dir.join("autre.tmp"), "ignoré")?;
        let found = scan(&dir);
        assert_eq!(
            found,
            vec![
                (b.clone(), "copie\r\n".into()),
                (c.clone(), "orphelin".into())
            ]
        );
        // Identique au disque : supprimée.
        assert!(!dir.join(format!("{}.bak", fingerprint(&a))).exists());
        let mut backups = Backups::new(dir, found);
        let mut s = ouvert(&b, "disque");
        // c.txt pas encore ouvert : seule b.txt est proposée ; la suivante attend la réponse.
        let ev = backups.next_offer(&s);
        assert!(matches!(&ev, Some(Event::BackupFound { path, .. }) if *path == b));
        update(&mut s, ev.expect("offre"));
        assert_eq!(backups.next_offer(&s), None);
        // Pendant que la copie attend : ni supprimée (document propre), ni écrasée (modifié), ni à la sortie.
        assert!(backups.cleanup(&s).is_empty());
        tape(&mut s, 'x');
        assert!(backups.tick(&s).is_empty());
        assert!(backups.quit(&s).is_empty());
        // Alt+B : la copie est réécrite au prochain passage, puis supprimée à la sortie.
        update(
            &mut s,
            Event::Key(KeyInput {
                key: Key::Char('b'),
                mods: Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
            }),
        );
        assert_eq!(s.doc.rope().to_string(), "copie\n");
        assert_eq!(backups.tick(&s).len(), 1);
        assert_eq!(backups.quit(&s).len(), 1);
        Ok(())
    }
}
