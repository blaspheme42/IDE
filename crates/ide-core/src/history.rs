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
    /// Dernier identifiant d'état distribué : jamais réutilisé, donc une version = un contenu.
    last_id: u64,
    /// Interdit de fusionner la prochaine édition avec le groupe précédent.
    sealed: bool,
}

impl History {
    pub fn next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    /// Coupe le regroupement (déplacement, annulation, sauvegarde).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Enregistre une édition (une seule `Change`) en la fusionnant si possible ; vide la pile redo.
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
