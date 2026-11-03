//! Marques git de la gouttière : HEAD de chaque fichier ouvert lu en fond (`Event::GitBase`),
//! diff avec le texte de l'onglet actif recalculé avant le rendu quand l'un des deux a changé.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ide_core::{Event, State};
use tokio::sync::mpsc;

#[derive(Default)]
pub struct GitMarks {
    /// Fichiers dont HEAD a été demandé (réponse arrivée ou en route).
    requested: HashSet<PathBuf>,
    /// Document et version des marques actuelles ; `None` = à recalculer.
    computed: Option<(PathBuf, u64)>,
    /// Dernière relecture générale : un outil qui écrit en rafale ne doit pas lancer des dizaines de `git`.
    cleared: Option<Instant>,
}

impl GitMarks {
    /// À appeler avant `update` : relit HEAD après une sauvegarde ou un changement sous la racine
    /// (branche, commit…), et recalcule à l'arrivée d'une base.
    pub fn on_event(&mut self, ev: &Event) {
        match ev {
            Event::Saved { path, .. } => {
                self.requested.remove(path);
            }
            Event::GitBranch(_)
                if self
                    .cleared
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(2)) =>
            {
                self.cleared = Some(Instant::now());
                self.requested.clear();
            }
            Event::GitBase { .. } => self.computed = None,
            _ => {}
        }
    }

    /// Demande HEAD des fichiers ouverts pas encore demandés, puis met à jour `state.git_marks`.
    pub fn sync(&mut self, state: &mut State, tx: &mpsc::Sender<Event>) {
        let docs = std::iter::once(&state.doc).chain(state.tabs.iter().map(|t| &t.doc));
        for path in docs.filter_map(|d| d.path.clone()) {
            if self.requested.insert(path.clone()) {
                let tx = tx.clone();
                tokio::task::spawn_blocking(move || {
                    let base = ide_workspace::git_base(&path);
                    let _ = tx.blocking_send(Event::GitBase { path, base });
                });
            }
        }
        let key = state.doc.path.clone().map(|p| (p, state.doc.version()));
        if key == self.computed {
            return;
        }
        state.git_marks = key
            .as_ref()
            .and_then(|(p, _)| state.git_bases.get(p))
            // Texte au format du disque (fins de ligne d'origine), comme HEAD.
            .map(|base| ide_workspace::git_marks(base, &state.doc.to_saved_text()))
            .unwrap_or_default();
        self.computed = key;
    }
}
