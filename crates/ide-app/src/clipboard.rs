//! Presse-papiers système (arboard) avec repli sur un presse-papiers interne.

use ide_core::Event;
use tokio::sync::mpsc;

pub enum Request {
    Copy(String),
    Paste,
}

/// Presse-papiers système si disponible, sinon le dernier texte copié dans l'app.
struct Clip {
    system: Option<arboard::Clipboard>,
    internal: String,
    warned: bool,
}

impl Clip {
    fn new(system: Option<arboard::Clipboard>) -> Self {
        Self {
            system,
            internal: String::new(),
            warned: false,
        }
    }

    fn warn_once(&mut self, e: impl std::fmt::Display) {
        if !self.warned {
            self.warned = true;
            tracing::warn!("presse-papiers système indisponible, repli interne : {e}");
        }
    }

    fn copy(&mut self, text: String) {
        if let Some(Err(e)) = self.system.as_mut().map(|c| c.set_text(text.as_str())) {
            self.warn_once(e);
        }
        self.internal = text;
    }

    fn paste(&mut self) -> String {
        match self.system.as_mut().map(|c| c.get_text()) {
            Some(Ok(text)) => text,
            Some(Err(e)) => {
                self.warn_once(e);
                self.internal.clone()
            }
            None => self.internal.clone(),
        }
    }
}

/// Lance le thread qui possède le `Clipboard` : sous X11, le contenu copié disparaît avec lui.
pub fn spawn(tx: mpsc::Sender<Event>) -> mpsc::UnboundedSender<Request> {
    let (req_tx, mut req_rx) = mpsc::unbounded_channel();
    // Thread détaché plutôt que spawn_blocking : il ne retarde pas l'arrêt du runtime.
    std::thread::spawn(move || {
        let mut clip = Clip::new(None);
        match arboard::Clipboard::new() {
            Ok(c) => clip.system = Some(c),
            Err(e) => clip.warn_once(e),
        }
        while let Some(req) = req_rx.blocking_recv() {
            match req {
                Request::Copy(text) => clip.copy(text),
                Request::Paste => {
                    if tx.blocking_send(Event::Paste(clip.paste())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    req_tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repli_interne_sans_systeme() {
        let mut clip = Clip::new(None);
        assert_eq!(clip.paste(), "");
        clip.copy("abc".into());
        clip.copy("déf\n".into());
        assert_eq!(clip.paste(), "déf\n");
    }
}
