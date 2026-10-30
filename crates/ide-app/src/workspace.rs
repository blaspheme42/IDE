//! Commandes de l'explorateur exécutées hors de la boucle : listage, ouverture, opérations disque.

use ide_core::{Command, Event};
use tokio::sync::mpsc;

/// Exécute `cmd` (ListDir, OpenFile, CreateFile, CreateDir, Rename, Trash) en tâche de fond ;
/// le résultat revient comme un événement sur `tx`.
pub fn spawn(cmd: Command, tx: mpsc::Sender<Event>) {
    let _ = (cmd, tx);
}
