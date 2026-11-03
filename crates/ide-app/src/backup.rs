//! Copies de secours des fichiers non sauvegardés (NF-07).
//!
//! Chaque instance écrit dans son dossier `<état>/ide/backup/<pid>/` : `lock` (identité du
//! processus) et `<empreinte>.bak` (première ligne = chemin du fichier, puis son texte
//! `to_saved_text()`). Au démarrage, seuls les dossiers d'instances mortes sont lus, plus les copies
//! posées directement dans `backup/` par une version précédente. Une copie proposée est adoptée :
//! déplacée dans notre dossier en `<empreinte>.offer<i>.bak`, supprimée une fois l'offre réglée
//! (Alt+B / Alt+N) et la copie normale du document écrite. Pendant l'attente, les nouvelles
//! modifications vont dans cette copie normale `<empreinte>.bak`, distincte : au redémarrage, la
//! plus récente des copies d'un même fichier est proposée, les autres supprimées après réponse.
//! shortcut: documents sans chemin ignorés (rien pour les retrouver au redémarrage) ; à couvrir si on ajoute « nouveau fichier ».

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ide_core::{Document, Event, LineEnding, State};
use tokio::sync::mpsc;

use crate::files;

/// Période des copies.
pub const PERIOD: Duration = Duration::from_secs(30);

/// Hors Linux, verrou plus vieux que ça = instance morte (il est touché à chaque `tick`).
#[cfg(not(target_os = "linux"))]
const STALE: Duration = Duration::from_secs(120);

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
pub enum Job {
    /// Texte produit dans le worker (une rope se clone sans copie : pas de `to_saved_text()` d'un
    /// fichier de 60 Mo dans la boucle) ; `then_remove` n'est supprimé qu'après une écriture réussie.
    Write {
        file: PathBuf,
        path: PathBuf,
        version: u64,
        rope: ropey::Rope,
        ending: LineEnding,
        then_remove: Vec<PathBuf>,
    },
    Remove(PathBuf),
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    /// Verrou touché : signe de vie (utile hors Linux, voir `alive`).
    Touch(PathBuf),
    /// Dossier d'instance supprimé s'il ne contient plus de copie.
    Release(PathBuf),
}

/// Écriture échouée : (chemin du document, version), retentée au prochain tick.
pub type Failure = (PathBuf, u64);

pub fn run(job: Job) -> Option<Failure> {
    match job {
        Job::Write {
            file,
            path,
            version,
            rope,
            ending,
            then_remove,
        } => {
            let header = path.to_string_lossy();
            let mut contents = String::with_capacity(header.len() + 1 + rope.len_bytes());
            contents.push_str(&header);
            contents.push('\n');
            for chunk in rope.chunks() {
                match ending {
                    LineEnding::Lf => contents.push_str(chunk),
                    LineEnding::CrLf => contents.push_str(&chunk.replace('\n', "\r\n")),
                }
            }
            if let Err(e) = files::save_atomic(&file, &contents) {
                tracing::warn!("copie de secours de {} : {e}", path.display());
                return Some((path, version));
            }
            then_remove.iter().for_each(|f| remove(f));
        }
        Job::Remove(file) => remove(&file),
        Job::Rename { from, to } => {
            // Notre dossier a pu disparaître (supprimé à la main) : recréé.
            let parent = to.parent().map_or(Ok(()), std::fs::create_dir_all);
            if let Err(e) = parent.and_then(|()| std::fs::rename(&from, &to)) {
                tracing::warn!("adoption de la copie {} : {e}", from.display());
            }
        }
        Job::Touch(lock) => {
            let touched = std::fs::File::options()
                .create(true)
                .append(true)
                .open(&lock)
                .and_then(|f| f.set_modified(SystemTime::now()));
            if let Err(e) = touched {
                tracing::warn!("verrou des copies {} : {e}", lock.display());
            }
        }
        Job::Release(dir) => release(&dir),
    }
    None
}

fn remove(file: &Path) {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!("suppression de la copie {} : {e}", file.display());
        }
        _ => {}
    }
}

/// Une tâche, dans l'ordre : une suppression ne peut pas passer avant l'écriture qu'elle annule.
/// Les échecs d'écriture reviennent sur `failures`.
pub async fn worker(
    mut jobs: mpsc::UnboundedReceiver<Job>,
    failures: mpsc::UnboundedSender<Failure>,
) {
    while let Some(job) = jobs.recv().await {
        match tokio::task::spawn_blocking(move || run(job)).await {
            Ok(Some(failure)) => {
                let _ = failures.send(failure);
            }
            Ok(None) => {}
            Err(e) => tracing::error!("copie de secours : {e}"),
        }
    }
}

/// Date de démarrage du processus `pid` (champ 22 de `/proc/<pid>/stat`), qui distingue un pid réutilisé.
#[cfg(target_os = "linux")]
fn start_time(pid: &str) -> Option<String> {
    let stat = std::fs::read_to_string(Path::new("/proc").join(pid).join("stat")).ok()?;
    // Le nom du processus, entre parenthèses, peut contenir des espaces : on compte après la dernière.
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(19).map(String::from)
}

/// Contenu de notre verrou.
fn identity() -> String {
    #[cfg(target_os = "linux")]
    {
        start_time("self").unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        String::new()
    }
}

/// L'instance propriétaire de `dir` (nommé d'après son pid) est-elle vivante ? Nom qui n'est pas un
/// pid (version précédente, pid réutilisé) : morte.
/// Linux : `/proc/<pid>` existe et sa date de démarrage est celle du verrou (verrou absent ou vide :
/// en cours de création, vivante).
/// Ailleurs : le processus existe (`kill -0`) et son verrou a été touché il y a moins de 2 min
/// (tick de 30 s), ce qui écarte un pid réutilisé.
/// shortcut: une machine en veille plus de 2 min laisse une autre instance proposer ses copies ;
/// passer à un vrai verrou de fichier (`File::lock`, Rust 1.89) quand la version minimale le permettra.
fn alive(dir: &Path) -> bool {
    let name = dir.file_name().and_then(|n| n.to_str());
    let Some(pid) = name.filter(|n| n.parse::<u32>().is_ok()) else {
        return false;
    };
    let lock = dir.join("lock");
    #[cfg(target_os = "linux")]
    {
        start_time(pid).is_some_and(|t| {
            std::fs::read_to_string(&lock)
                .ok()
                .is_none_or(|l| l.is_empty() || l == t)
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Pid hors des entiers positifs : `kill -0 -1` viserait tous les processus.
        let exists = pid.parse::<i32>().is_ok_and(|p| p > 0)
            && std::process::Command::new("kill")
                .args(["-0", pid])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                // Sans commande `kill` (Windows) : on s'en tient au verrou.
                .map_or(true, |s| s.success());
        exists
            && (std::fs::metadata(&lock).and_then(|m| m.modified()).ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|d| d < STALE)
    }
}

/// Crée notre dossier `<root>/<pid>/` et son verrou. Un dossier existant à notre pid vient
/// forcément d'une instance morte : renommé, ses copies restent proposables.
pub fn claim(root: &Path) -> PathBuf {
    let pid = std::process::id();
    let dir = root.join(pid.to_string());
    if dir.exists() {
        let nanos =
            (SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)).map_or(0, |d| d.as_nanos());
        if let Err(e) = std::fs::rename(&dir, root.join(format!("{pid}-{nanos}"))) {
            tracing::warn!("copies de secours d'un ancien pid {pid} : {e}");
        }
    }
    if let Err(e) = files::save_atomic(&dir.join("lock"), &identity()) {
        tracing::warn!("verrou des copies de secours dans {} : {e}", dir.display());
    }
    dir
}

/// Copies d'un fichier trouvées au démarrage : texte de la plus récente, et tous leurs fichiers.
#[derive(Debug, PartialEq, Eq)]
pub struct Offer {
    pub path: PathBuf,
    pub text: String,
    pub files: Vec<PathBuf>,
}

fn baks(dir: &Path) -> Vec<PathBuf> {
    (std::fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|f| f.extension().is_some_and(|e| e == "bak") && f.is_file())
        .collect()
}

fn release(dir: &Path) {
    if baks(dir).is_empty() {
        remove(&dir.join("lock"));
        let _ = std::fs::remove_dir(dir);
    }
}

/// Copies des instances mortes (hors `own`) qui diffèrent du disque (fichier disparu compris), triées
/// par chemin ; celles dont la plus récente est identique au disque sont supprimées. Lent sur de
/// grosses copies : à lancer hors de la boucle (NF-01).
pub fn scan(root: &Path, own: &Path) -> Vec<Offer> {
    let mut dirs = vec![root.to_path_buf()];
    dirs.extend(
        (std::fs::read_dir(root).into_iter().flatten().flatten())
            .map(|e| e.path())
            .filter(|d| d.is_dir() && d != own && !alive(d)),
    );
    let mut copies: BTreeMap<PathBuf, Vec<(SystemTime, PathBuf, String)>> = BTreeMap::new();
    for file in dirs.iter().flat_map(|d| baks(d)) {
        let read = std::fs::read_to_string(&file).ok().and_then(|s| {
            let (p, t) = s.split_once('\n')?;
            Some((PathBuf::from(p), t.to_string()))
        });
        let Some((path, text)) = read else {
            tracing::warn!("copie de secours illisible : {}", file.display());
            continue;
        };
        let time =
            (std::fs::metadata(&file).and_then(|m| m.modified())).unwrap_or(SystemTime::UNIX_EPOCH);
        copies.entry(path).or_default().push((time, file, text));
    }
    let mut found = vec![];
    for (path, mut list) in copies {
        list.sort_by_key(|c| std::cmp::Reverse(c.0));
        let all: Vec<PathBuf> = list.iter().map(|c| c.1.clone()).collect();
        let Some((_, _, text)) = list.into_iter().next() else {
            continue;
        };
        if files::read_file(&path).is_ok_and(|disk| disk == text) {
            all.iter().for_each(|f| remove(f));
        } else {
            found.push(Offer {
                path,
                text,
                files: all,
            });
        }
    }
    dirs[1..].iter().for_each(|d| release(d));
    found
}

fn docs(state: &State) -> impl Iterator<Item = &Document> {
    std::iter::once(&state.doc).chain(state.tabs.iter().map(|t| &t.doc))
}

/// Ouvre en fond les fichiers des copies trouvées qui ne le sont pas encore, puis rend la main à
/// l'onglet actif : les offres suivent par `next_offer`.
pub fn open_missing(state: &State, offers: &[Offer], tx: mpsc::Sender<Event>) {
    let paths: Vec<PathBuf> = (offers.iter().map(|o| o.path.clone()))
        .filter(|p| !docs(state).any(|d| d.path.as_ref() == Some(p)))
        .collect();
    if paths.is_empty() {
        return;
    }
    let active = state.doc.path.clone();
    tokio::spawn(async move {
        let opened = tokio::task::spawn_blocking(move || {
            (paths.into_iter())
                .flat_map(crate::workspace::open_file)
                .collect::<Vec<_>>()
        });
        let Ok(mut events) = opened.await else {
            return;
        };
        events.extend(active.map(Event::ActivateTab));
        for ev in events {
            if tx.send(ev).await.is_err() {
                break;
            }
        }
    });
}

pub struct Backups {
    dir: PathBuf,
    /// Copies de notre dossier, par chemin de document : version demandée au worker (`u64::MAX` :
    /// écriture échouée, retentée au prochain tick).
    sent: HashMap<PathBuf, u64>,
    /// Copies trouvées au démarrage pas encore proposées : le cœur n'en garde qu'une à la fois.
    offers: VecDeque<Offer>,
    /// Offre proposée pas encore réglée, et ses fichiers adoptés.
    offered: Option<(PathBuf, Vec<PathBuf>)>,
    /// Chemins sans copie possible, déjà signalés dans le log.
    unsupported: HashSet<PathBuf>,
}

impl Backups {
    /// `dir` : notre dossier, créé par `claim`.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            sent: HashMap::new(),
            offers: VecDeque::new(),
            offered: None,
            unsupported: HashSet::new(),
        }
    }

    /// Résultat du scan du démarrage, arrivé après le premier rendu.
    pub fn add_offers(&mut self, offers: Vec<Offer>) {
        self.offers.extend(offers);
    }

    fn file(&self, path: &Path) -> PathBuf {
        self.dir.join(format!("{}.bak", fingerprint(path)))
    }

    /// Le chemin peut-il servir d'en-tête de copie, relue jusqu'au premier `\n` ?
    /// shortcut: chemin non UTF-8 ou contenant `\n` : ce document n'a pas de copie (signalé une
    /// fois dans le log) ; encoder l'en-tête si un utilisateur en a besoin.
    fn copyable(&mut self, path: &Path) -> bool {
        let ok = path.to_str().is_some_and(|p| !p.contains('\n'));
        if !ok && self.unsupported.insert(path.to_path_buf()) {
            tracing::warn!(
                "pas de copie de secours pour {} : chemin non UTF-8 ou contenant un saut de ligne",
                path.display()
            );
        }
        ok
    }

    fn write(&mut self, doc: &Document, path: &Path, then_remove: Vec<PathBuf>) -> Job {
        self.sent.insert(path.to_path_buf(), doc.version());
        Job::Write {
            file: self.file(path),
            path: path.to_path_buf(),
            version: doc.version(),
            rope: doc.rope().clone(),
            ending: doc.line_ending,
            then_remove,
        }
    }

    /// Écriture échouée remontée par le worker : retentée au prochain tick.
    pub fn failed(&mut self, (path, version): Failure) {
        if let Some(v) = self.sent.get_mut(&path).filter(|v| **v == version) {
            *v = u64::MAX;
        }
    }

    /// Copie des documents modifiés retenus par `keep` dont cette version n'est pas déjà demandée.
    fn flush_where(&mut self, state: &State, keep: impl Fn(&Self, &Path) -> bool) -> Vec<Job> {
        let mut jobs = vec![];
        for doc in docs(state).filter(|d| d.is_dirty()) {
            let Some(path) = doc.path.as_deref().filter(|p| keep(self, p)) else {
                continue;
            };
            if self.sent.get(path) != Some(&doc.version()) && self.copyable(path) {
                jobs.push(self.write(doc, path, vec![]));
            }
        }
        jobs
    }

    /// Sortie anormale (signal, terminal fermé) : dernière copie de tout ce qui a changé ; les
    /// copies restent pour le prochain démarrage.
    pub fn flush(&mut self, state: &State) -> Vec<Job> {
        self.flush_where(state, |_, _| true)
    }

    /// Toutes les 30 s : copie de chaque document modifié dont la version a changé, signe de vie.
    pub fn tick(&mut self, state: &State) -> Vec<Job> {
        let mut jobs = self.flush(state);
        jobs.push(Job::Touch(self.dir.join("lock")));
        jobs
    }

    /// Après chaque événement : offre réglée, copie immédiate des documents modifiés qui n'en ont
    /// pas (premier changement, renommage), suppression des copies des documents redevenus propres
    /// ou fermés (même modifiés : Ctrl+W confirmé est un choix explicite). Écritures d'abord.
    pub fn cleanup(&mut self, state: &State) -> Vec<Job> {
        let mut jobs = vec![];
        let dirty = |p: &Path| docs(state).find(|d| d.is_dirty() && d.path.as_deref() == Some(p));
        if state.backup_offer.is_none()
            && let Some((path, adopted)) = self.offered.take()
        {
            // Réglée (Alt+B ou Alt+N) : la copie normale, à jour, remplace les copies adoptées.
            match dirty(&path) {
                Some(doc) if self.copyable(&path) => jobs.push(self.write(doc, &path, adopted)),
                _ => jobs.extend(adopted.into_iter().map(Job::Remove)),
            }
        }
        jobs.extend(self.flush_where(state, |b, p| !b.sent.contains_key(p)));
        let gone: Vec<PathBuf> = (self.sent.keys())
            .filter(|p| dirty(p).is_none())
            .cloned()
            .collect();
        for p in gone {
            self.sent.remove(&p);
            jobs.push(Job::Remove(self.file(&p)));
        }
        jobs
    }

    /// Sortie normale (Ctrl+Q confirmé), une fois les sauvegardes finies : l'utilisateur a renoncé
    /// aux modifications, sauf pour `failed` (dernière sauvegarde en échec) dont la copie est gardée
    /// et mise à jour. Les copies adoptées d'une offre sans réponse restent : reproposées.
    pub fn quit(&mut self, state: &State, failed: &[PathBuf]) -> Vec<Job> {
        let keep = |p: &Path| failed.iter().any(|f| f == p);
        let mut jobs = self.flush_where(state, |_, p| keep(p));
        let gone: Vec<PathBuf> = (self.sent.keys()).filter(|p| !keep(p)).cloned().collect();
        for p in gone {
            self.sent.remove(&p);
            jobs.push(Job::Remove(self.file(&p)));
        }
        jobs.push(Job::Release(self.dir.clone()));
        jobs
    }

    /// Prochaine copie à proposer, une fois la précédente réglée et son onglet ouvert. Elle est
    /// adoptée (déplacée dans notre dossier) : une autre instance ne la proposera pas.
    pub fn next_offer(&mut self, state: &State) -> Option<(Event, Vec<Job>)> {
        if state.backup_offer.is_some() || self.offered.is_some() {
            return None;
        }
        let open = |p: &Path| docs(state).any(|d| d.path.as_deref() == Some(p));
        let i = self.offers.iter().position(|o| open(&o.path))?;
        let Offer { path, text, files } = self.offers.remove(i)?;
        let fp = fingerprint(&path);
        let (jobs, adopted) = (files.into_iter().enumerate())
            .map(|(i, from)| {
                let to = self.dir.join(format!("{fp}.offer{i}.bak"));
                (
                    Job::Rename {
                        from,
                        to: to.clone(),
                    },
                    to,
                )
            })
            .unzip();
        self.offered = Some((path.clone(), adopted));
        Some((Event::BackupFound { path, text }, jobs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{FsChange, Key, KeyInput, Modifiers, update};

    fn ouvert(path: &Path, text: &str) -> State {
        let mut s = State::default();
        ouvre(&mut s, path, text);
        s
    }
    fn ouvre(s: &mut State, path: &Path, text: &str) {
        update(
            s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        );
    }
    fn touche(s: &mut State, c: char, alt: bool) {
        update(
            s,
            Event::Key(KeyInput {
                key: Key::Char(c),
                mods: Modifiers {
                    alt,
                    ..Modifiers::default()
                },
            }),
        );
    }
    fn tape(s: &mut State, c: char) {
        touche(s, c, false);
    }
    /// Exécute les tâches comme le worker ; renvoie le nombre d'écritures.
    fn applique(jobs: Vec<Job>) -> usize {
        let n = jobs
            .iter()
            .filter(|j| matches!(j, Job::Write { .. }))
            .count();
        jobs.into_iter().for_each(|j| {
            run(j);
        });
        n
    }
    fn bak(dir: &Path, p: &Path) -> PathBuf {
        dir.join(format!("{}.bak", fingerprint(p)))
    }
    /// Copie au format du disque, écrite par une autre instance.
    fn copie(dir: &Path, p: &Path, text: &str) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let file = bak(dir, p);
        std::fs::write(&file, format!("{}\n{text}", p.display()))?;
        Ok(file)
    }
    fn date(file: &Path, secs: u64) -> anyhow::Result<()> {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs);
        std::fs::File::options()
            .write(true)
            .open(file)?
            .set_modified(t)?;
        Ok(())
    }

    #[test]
    fn empreinte_stable() {
        // Valeurs de référence FNV-1a 64 : ne doivent jamais changer (copies d'une version précédente).
        assert_eq!(fingerprint(Path::new("")), "cbf29ce484222325");
        assert_eq!(fingerprint(Path::new("a")), "af63dc4c8601ec8c");
    }

    #[test]
    fn copie_immediate_puis_une_fois_par_version_supprimee_apres_sauvegarde() -> anyhow::Result<()>
    {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("backup/moi");
        let p = tmp.path().join("a.txt");
        std::fs::write(&p, "disque\r\n")?;
        let mut b = Backups::new(dir.clone());
        let mut s = ouvert(&p, "disque\r\n");
        assert_eq!(applique(b.cleanup(&s)) + applique(b.tick(&s)), 0, "propre");
        tape(&mut s, 'x');
        // Premier changement : copie tout de suite, fins de ligne d'origine.
        assert_eq!(applique(b.cleanup(&s)), 1);
        let file = bak(&dir, &p);
        let attendu = format!("{}\nxdisque\r\n", p.display());
        assert_eq!(std::fs::read_to_string(&file)?, attendu);
        assert_eq!(applique(b.cleanup(&s)) + applique(b.tick(&s)), 0);
        // Changements suivants : au tick.
        tape(&mut s, 'y');
        assert_eq!(applique(b.cleanup(&s)), 0);
        assert_eq!(applique(b.tick(&s)), 1);
        assert!(std::fs::read_to_string(&file)?.ends_with("\nxydisque\r\n"));
        // Au redémarrage (notre dossier est alors celui d'une instance morte), proposée.
        let other = tmp.path().join("backup/autre");
        let found = scan(&tmp.path().join("backup"), &other);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "xydisque\r\n");
        // Sauvegardé : document propre, copie supprimée.
        let v = s.doc.version();
        update(
            &mut s,
            Event::Saved {
                path: p,
                version: v,
            },
        );
        assert_eq!(applique(b.cleanup(&s)), 0);
        assert!(!file.exists());
        Ok(())
    }

    #[test]
    fn fermeture_et_sortie_normale_suppriment_les_copies() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = claim(tmp.path());
        let p = tmp.path().join("a.txt");
        let mut b = Backups::new(dir.clone());
        let mut s = ouvert(&p, "");
        tape(&mut s, 'x');
        applique(b.cleanup(&s));
        assert!(bak(&dir, &p).exists());
        // Ctrl+W confirmé : seul document restant vierge.
        applique(b.cleanup(&State::default()));
        assert!(!bak(&dir, &p).exists());
        tape(&mut s, 'y');
        applique(b.cleanup(&s));
        // Sortie normale, sauvegardes réussies : copies et dossier supprimés.
        applique(b.quit(&s, &[]));
        assert!(!dir.exists());
        Ok(())
    }

    #[test]
    fn sauvegarde_en_echec_a_la_sortie_copie_gardee() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = claim(tmp.path());
        let (p, q) = (tmp.path().join("a.txt"), tmp.path().join("b.txt"));
        let mut b = Backups::new(dir.clone());
        let mut s = ouvert(&q, "");
        tape(&mut s, 'q');
        applique(b.cleanup(&s));
        ouvre(&mut s, &p, "");
        // Pas encore de copie de a.txt (Ctrl+S puis Ctrl+Q aussitôt) : écrite à la sortie.
        tape(&mut s, 'p');
        applique(b.quit(&s, std::slice::from_ref(&p)));
        assert_eq!(
            std::fs::read_to_string(bak(&dir, &p))?,
            format!("{}\np", p.display())
        );
        assert!(!bak(&dir, &q).exists());
        // Proposée au redémarrage, une fois ce processus « mort ».
        std::fs::rename(&dir, tmp.path().join("4294967295"))?;
        let found = scan(tmp.path(), &tmp.path().join("autre"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, p);
        Ok(())
    }

    #[test]
    fn renommage_copie_immediate_sous_le_nouveau_chemin() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("moi");
        let (p, q) = (tmp.path().join("a.txt"), tmp.path().join("b.txt"));
        let mut b = Backups::new(dir.clone());
        let mut s = ouvert(&p, "");
        tape(&mut s, 'x');
        applique(b.cleanup(&s));
        update(
            &mut s,
            Event::FsDone(Ok(FsChange::Renamed {
                from: p.clone(),
                to: q.clone(),
            })),
        );
        assert_eq!(s.doc.path.as_ref(), Some(&q));
        let jobs = b.cleanup(&s);
        // Nouvelle copie écrite avant la suppression de l'ancienne.
        assert!(
            matches!(&jobs[..], [Job::Write { path, .. }, Job::Remove(f)] if *path == q && *f == bak(&dir, &p))
        );
        applique(jobs);
        assert!(bak(&dir, &q).exists() && !bak(&dir, &p).exists());
        Ok(())
    }

    #[test]
    fn ecriture_en_echec_retentee_au_tick() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        // Un fichier à la place du dossier parent : l'écriture échoue.
        let blocker = tmp.path().join("bloque");
        std::fs::write(&blocker, "")?;
        let dir = blocker.join("moi");
        let p = tmp.path().join("a.txt");
        let mut b = Backups::new(dir.clone());
        let mut s = ouvert(&p, "");
        tape(&mut s, 'x');
        let mut jobs = b.cleanup(&s);
        assert_eq!(jobs.len(), 1);
        let failure = run(jobs.remove(0));
        assert_eq!(failure, Some((p.clone(), s.doc.version())));
        b.failed(failure.expect("échec"));
        // Pas de rafale à chaque événement : retentée au tick.
        assert_eq!(applique(b.cleanup(&s)), 0);
        std::fs::remove_file(&blocker)?;
        let jobs = b.tick(&s);
        assert_eq!(applique(jobs), 1);
        assert!(bak(&dir, &p).exists());
        assert_eq!(applique(b.tick(&s)), 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn chemin_non_utf8_ou_avec_saut_de_ligne_sans_copie() -> anyhow::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("moi");
        let mut b = Backups::new(dir.clone());
        for p in [
            tmp.path()
                .join(std::ffi::OsStr::from_bytes(b"\xff\xfe.txt")),
            tmp.path().join("a\nb.txt"),
        ] {
            let mut s = ouvert(&p, "");
            tape(&mut s, 'x');
            assert_eq!(applique(b.cleanup(&s)) + applique(b.tick(&s)), 0);
            assert_eq!(applique(b.flush(&s)), 0);
        }
        assert_eq!(baks(&dir).len(), 0);
        Ok(())
    }

    #[test]
    fn scan_ignore_les_instances_vivantes_et_propose_les_mortes() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("backup");
        let own = root.join("moi");
        let (a, b, c, d, e) = (
            tmp.path().join("a.txt"),
            tmp.path().join("b.txt"),
            tmp.path().join("c.txt"),
            tmp.path().join("d.txt"),
            tmp.path().join("e.txt"),
        );
        std::fs::write(&e, "pareil")?;
        // Instance vivante (ce processus, verrou à son identité) : ignorée et gardée.
        let vivant = root.join(std::process::id().to_string());
        let fa = copie(&vivant, &a, "vivante")?;
        std::fs::write(vivant.join("lock"), identity())?;
        // Instance morte (pid impossible), copie d'une version précédente, notre propre dossier.
        let mort = root.join("4294967295");
        copie(&mort, &b, "morte")?;
        std::fs::write(mort.join("lock"), "1")?;
        copie(&root, &c, "ancienne")?;
        copie(&own, &d, "nôtre")?;
        // Morte et identique au disque : supprimée, dossier libéré.
        let mort2 = root.join("4294967294");
        copie(&mort2, &e, "pareil")?;
        let found = scan(&root, &own);
        let paths: Vec<&Path> = found.iter().map(|o| o.path.as_path()).collect();
        assert_eq!(paths, [b.as_path(), c.as_path()]);
        assert!(fa.exists() && vivant.join("lock").exists());
        assert!(bak(&mort, &b).exists(), "supprimée seulement après réponse");
        assert!(!mort2.exists());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pid_reutilise_compte_comme_mort() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let a = tmp.path().join("a.txt");
        // Notre pid, mais une autre date de démarrage : instance morte d'avant un redémarrage.
        let dir = tmp.path().join(std::process::id().to_string());
        copie(&dir, &a, "copie")?;
        std::fs::write(dir.join("lock"), "0")?;
        assert_eq!(scan(tmp.path(), &tmp.path().join("moi")).len(), 1);
        // `claim` le met de côté au lieu de le reprendre : toujours proposé.
        let mine = claim(tmp.path());
        assert_eq!(mine, dir);
        assert!(!bak(&dir, &a).exists());
        assert_eq!(scan(tmp.path(), &mine).len(), 1);
        Ok(())
    }

    #[test]
    fn offre_adoptee_modifications_pendant_l_attente_puis_alt_b() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("backup");
        let own = root.join("moi");
        let a = tmp.path().join("a.txt");
        std::fs::write(&a, "disque")?;
        let morte = copie(&root.join("4294967295"), &a, "copie")?;
        let mut b = Backups::new(own.clone());
        b.add_offers(scan(&root, &own));
        let mut s = ouvert(&a, "disque");
        let (ev, jobs) = b.next_offer(&s).expect("offre");
        applique(jobs);
        let adoptee = own.join(format!("{}.offer0.bak", fingerprint(&a)));
        assert!(adoptee.exists() && !morte.exists(), "adoptée");
        update(&mut s, ev);
        assert!(b.next_offer(&s).is_none());
        // Modifiée pendant l'attente : copiée dans un second fichier, l'offre reste.
        tape(&mut s, 'x');
        assert_eq!(applique(b.cleanup(&s)), 1);
        let normale = bak(&own, &a);
        assert_eq!(
            std::fs::read_to_string(&normale)?,
            format!("{}\nxdisque", a.display())
        );
        assert!(adoptee.exists());
        // Crash simulé : la plus récente des deux est proposée, aucune n'est supprimée.
        date(&adoptee, 0)?;
        date(&normale, 10)?;
        let found = scan(&root, &root.join("autre"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "xdisque");
        assert_eq!(found[0].files, [normale.clone(), adoptee.clone()]);
        // Alt+B : la copie normale reprend le texte restauré, l'adoptée disparaît ensuite.
        touche(&mut s, 'b', true);
        assert_eq!(s.doc.rope().to_string(), "copie");
        assert_eq!(applique(b.cleanup(&s)), 1);
        assert!(!adoptee.exists());
        assert_eq!(
            std::fs::read_to_string(&normale)?,
            format!("{}\ncopie", a.display())
        );
        Ok(())
    }

    #[test]
    fn alt_n_document_propre_tout_supprime() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("backup");
        let own = claim(&root);
        let a = tmp.path().join("a.txt");
        copie(&root.join("4294967295"), &a, "copie")?;
        let mut b = Backups::new(own.clone());
        b.add_offers(scan(&root, &own));
        let mut s = ouvert(&a, "disque");
        let (ev, jobs) = b.next_offer(&s).expect("offre");
        applique(jobs);
        update(&mut s, ev);
        // Sortie pendant l'attente : l'adoptée reste, reproposée.
        let mut keep = Backups::new(own.clone());
        applique(keep.quit(&s, &[]));
        assert_eq!(baks(&own).len(), 1);
        touche(&mut s, 'n', true);
        assert!(s.backup_offer.is_none());
        applique(b.cleanup(&s));
        applique(b.quit(&s, &[]));
        assert!(baks(&root.join("4294967295")).is_empty() && !own.exists());
        Ok(())
    }

    #[test]
    fn offres_arrivees_apres_le_premier_rendu() {
        let a = PathBuf::from("/x/a.txt");
        let mut b = Backups::new("/x/moi".into());
        let s = ouvert(&a, "disque");
        assert!(b.next_offer(&s).is_none());
        b.add_offers(vec![Offer {
            path: a.clone(),
            text: "copie".into(),
            files: vec![],
        }]);
        let ev = b.next_offer(&s).map(|(ev, _)| ev);
        assert_eq!(
            ev,
            Some(Event::BackupFound {
                path: a,
                text: "copie".into()
            })
        );
    }

    #[tokio::test]
    async fn fichiers_des_copies_ouverts_en_fond_puis_onglet_actif_rendu() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let base = tmp.path().canonicalize()?;
        let (a, b) = (base.join("a.txt"), base.join("b.txt"));
        std::fs::write(&a, "")?;
        std::fs::write(&b, "bbb")?;
        let s = ouvert(&a, "");
        let offre = |p: &Path| Offer {
            path: p.into(),
            text: String::new(),
            files: vec![],
        };
        let (tx, mut rx) = mpsc::channel(8);
        open_missing(&s, &[offre(&a), offre(&b)], tx);
        let mut events = vec![];
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        assert!(matches!(&events[..], [
            Event::FileOpened { path, .. },
            Event::IndentDetected { .. },
            Event::ActivateTab(active),
        ] if *path == b && *active == a));
        Ok(())
    }
}
