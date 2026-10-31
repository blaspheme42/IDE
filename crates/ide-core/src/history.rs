//! Historique annuler/rétablir illimité, avec regroupement des frappes d'un même mot.

/// (curseur, ancre de sélection) à restaurer.
pub(crate) type Sel = (usize, Option<usize>);

/// Remplacement élémentaire : `removed` remplacé par `inserted` à partir de `pos` (index de `char`).
#[derive(Debug)]
pub(crate) struct Change {
    pub pos: usize,
    pub removed: String,
    pub inserted: String,
}

/// Type d'édition : seules les frappes successives d'un même type se regroupent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Insert,
    Backspace,
    Delete,
    Other,
}

/// Une étape d'annulation : un groupe de changements appliqués dans l'ordre.
#[derive(Debug)]
pub(crate) struct Entry {
    pub changes: Vec<Change>,
    pub kind: Kind,
    pub before: Sel,
    pub after: Sel,
    pub version_before: u64,
    pub version_after: u64,
}

#[derive(Debug, Default)]
pub(crate) struct History {
    pub undo: Vec<Entry>,
    pub redo: Vec<Entry>,
    /// Interdit de fusionner la prochaine édition avec le groupe précédent.
    sealed: bool,
}

impl History {
    /// Identifiant d'état unique sur TOUS les documents : une version désigne un seul document et un seul
    /// contenu, si bien qu'une confirmation de sauvegarde ne peut pas marquer propre un autre onglet.
    pub fn next_id(&mut self) -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Coupe le regroupement (déplacement, annulation, sauvegarde).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Enregistre une édition en la fusionnant si possible avec la précédente ; vide la pile redo.
    pub fn record(&mut self, mut e: Entry) {
        self.redo.clear();
        let word_break =
            e.kind == Kind::Insert && e.changes.iter().all(|c| c.inserted.trim().is_empty());
        match self.undo.last_mut() {
            Some(last)
                if !self.sealed
                    && !word_break
                    && e.kind != Kind::Other
                    && last.kind == e.kind
                    && last.after == e.before =>
            {
                last.changes.append(&mut e.changes);
                last.after = e.after;
                last.version_after = e.version_after;
            }
            _ => self.undo.push(e),
        }
        self.sealed = false;
    }
}
