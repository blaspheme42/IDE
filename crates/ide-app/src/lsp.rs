//! Serveurs de langage (J4) : démarrage par langage, synchronisation des documents, requêtes.
//! Tout passe par des tâches de fond : un serveur lent ou planté ne bloque jamais la boucle (NF-05).

use std::path::PathBuf;

use ide_core::{Command, Event, LspServer, State};
use tokio::sync::mpsc;

pub struct LspManager {
    tx: mpsc::Sender<Event>,
    root: PathBuf,
    servers: Vec<LspServer>,
}

impl LspManager {
    pub fn new(tx: mpsc::Sender<Event>, root: PathBuf, servers: Vec<LspServer>) -> Self {
        Self { tx, root, servers }
    }

    /// Avant chaque rendu : démarre les serveurs utiles, envoie didOpen / didChange / didClose
    /// pour les documents ouverts, modifiés ou fermés depuis le dernier appel. Ne bloque jamais.
    pub fn sync(&mut self, state: &State) {
        let _ = (state, &self.tx, &self.root, &self.servers);
    }

    /// `RequestCompletion` / `RequestHover` / `RequestDefinition` → réponse en `Event` sur `tx`.
    pub fn request(&mut self, cmd: Command) {
        let _ = cmd;
    }
}
