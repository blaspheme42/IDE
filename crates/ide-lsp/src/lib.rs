//! Crate ide-lsp : client LSP (J4), JSON-RPC maison sur stdin/stdout du serveur.
//!
//! JSON lu à la main (`serde_json::Value`) plutôt que `lsp-types` : une quinzaine de champs
//! seulement, et des réponses tolérées sous toutes leurs formes.
//! Un serveur lent ou mort ne bloque jamais : délai par requête, erreurs plutôt que paniques.

mod convert;
#[cfg(test)]
mod tests;
mod transport;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot};

use convert::{path_to_uri, position_json};

/// Délai par défaut d'une requête.
const TIMEOUT: Duration = Duration::from_secs(5);
/// `initialize` : un serveur froid peut indexer avant de répondre.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Événements spontanés du serveur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    Diagnostics {
        path: PathBuf,
        items: Vec<ide_core::Diagnostic>,
    },
    /// `window/showMessage`.
    Message(String),
    /// Serveur arrêté ou planté ; émise une seule fois.
    Exited(String),
}

type Reply = oneshot::Sender<Result<Value, String>>;

/// État partagé avec la tâche de lecture (qui ne garde pas le processus en vie).
struct Shared {
    pending: Mutex<HashMap<i64, Reply>>,
    alive: AtomicBool,
}

impl Shared {
    fn pending(&self) -> MutexGuard<'_, HashMap<i64, Reply>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct Inner {
    shared: Arc<Shared>,
    out: mpsc::UnboundedSender<Vec<u8>>,
    next_id: AtomicI64,
    timeout: Duration,
    /// Processus tué quand le dernier `Client` disparaît (`kill_on_drop`).
    child: Mutex<Option<Child>>,
}

/// Client d'un serveur de langage ; clonable, toutes les copies parlent au même serveur.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    /// Lance le serveur, fait initialize/initialized (capabilities minimales : synchronisation
    /// complète, completion, hover, definition, publishDiagnostics ; positionEncoding utf-16).
    /// Échec de lancement (commande introuvable) → Err lisible en français.
    pub async fn start(
        command: &str,
        args: &[String],
        root: &Path,
    ) -> Result<(Client, mpsc::UnboundedReceiver<Notification>), String> {
        let mut child = tokio::process::Command::new(command)
            .args(args)
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => {
                    format!("serveur LSP « {command} » introuvable : installez-le ou corrigez [lsp] dans config.toml")
                }
                _ => format!("lancement du serveur LSP « {command} » impossible : {e}"),
            })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(format!(
                "serveur LSP « {command} » : flux standard indisponibles"
            ));
        };
        if let Some(stderr) = child.stderr.take() {
            // Vidé en continu, sinon un serveur bavard se bloque sur un tube plein.
            tokio::spawn(async move {
                let mut r = BufReader::new(stderr);
                let mut line = Vec::new();
                while r.read_until(b'\n', &mut line).await.is_ok_and(|n| n > 0) {
                    tracing::trace!("LSP stderr : {}", String::from_utf8_lossy(&line).trim_end());
                    line.clear();
                }
            });
        }
        let (client, rx) = Client::with_io(stdout, stdin, command, TIMEOUT);
        *client.inner.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        client.initialize(root).await?;
        Ok((client, rx))
    }

    /// Client sur des flux quelconques (tests : `tokio::io::duplex`), sans `initialize`.
    #[doc(hidden)]
    pub fn with_io<R, W>(
        reader: R,
        writer: W,
        name: &str,
        timeout: Duration,
    ) -> (Client, mpsc::UnboundedReceiver<Notification>)
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (out, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (notif, notif_rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            alive: AtomicBool::new(true),
        });
        tokio::spawn(async move {
            let mut w = writer;
            while let Some(bytes) = out_rx.recv().await {
                if w.write_all(&bytes).await.is_err() || w.flush().await.is_err() {
                    break; // la lecture verra la fin du flux et signalera l'arrêt
                }
            }
        });
        tokio::spawn(read_loop(
            BufReader::new(reader),
            shared.clone(),
            out.clone(),
            notif,
            name.to_string(),
        ));
        let inner = Inner {
            shared,
            out,
            next_id: AtomicI64::new(1),
            timeout,
            child: Mutex::new(None),
        };
        (
            Client {
                inner: Arc::new(inner),
            },
            notif_rx,
        )
    }

    /// `initialize` puis `initialized`.
    #[doc(hidden)]
    pub async fn initialize(&self, root: &Path) -> Result<(), String> {
        let uri = path_to_uri(root);
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned());
        let params = json!({
            "processId": std::process::id(),
            "rootUri": uri,
            "workspaceFolders": [{ "uri": uri, "name": name.unwrap_or_default() }],
            "clientInfo": { "name": "ide" },
            "capabilities": {
                "general": { "positionEncodings": ["utf-16"] },
                "textDocument": {
                    "synchronization": { "didSave": true },
                    "completion": { "completionItem": { "snippetSupport": true } },
                    "hover": { "contentFormat": ["plaintext", "markdown"] },
                    "definition": { "linkSupport": true },
                    "publishDiagnostics": {},
                },
                "window": { "workDoneProgress": true },
            },
        });
        self.request_with("initialize", params, INIT_TIMEOUT)
            .await?;
        self.notify("initialized", json!({}));
        Ok(())
    }

    pub fn did_open(&self, path: &Path, language_id: &str, version: i32, text: &str) {
        let doc = json!({ "uri": path_to_uri(path), "languageId": language_id, "version": version, "text": text });
        self.notify("textDocument/didOpen", json!({ "textDocument": doc }));
    }

    /// Texte complet (`TextDocumentSyncKind::Full`).
    pub fn did_change(&self, path: &Path, version: i32, text: &str) {
        let doc = json!({ "uri": path_to_uri(path), "version": version });
        let params = json!({ "textDocument": doc, "contentChanges": [{ "text": text }] });
        self.notify("textDocument/didChange", params);
    }

    pub fn did_save(&self, path: &Path) {
        self.notify("textDocument/didSave", doc_params(path));
    }

    pub fn did_close(&self, path: &Path) {
        self.notify("textDocument/didClose", doc_params(path));
    }

    pub async fn completion(
        &self,
        path: &Path,
        pos: ide_core::Position,
    ) -> Result<Vec<ide_core::CompletionItem>, String> {
        let v = self.request("textDocument/completion", pos_params(path, pos));
        Ok(convert::completion(&v.await?))
    }

    pub async fn hover(
        &self,
        path: &Path,
        pos: ide_core::Position,
    ) -> Result<Option<String>, String> {
        let v = self.request("textDocument/hover", pos_params(path, pos));
        Ok(convert::hover(&v.await?))
    }

    pub async fn definition(
        &self,
        path: &Path,
        pos: ide_core::Position,
    ) -> Result<Vec<ide_core::Location>, String> {
        let v = self.request("textDocument/definition", pos_params(path, pos));
        Ok(convert::locations(&v.await?))
    }

    /// `shutdown` + `exit`, puis kill si le processus traîne (≤ 1 s).
    pub async fn shutdown(&self) {
        if self.is_alive() {
            let _ = self
                .request_with("shutdown", Value::Null, Duration::from_millis(500))
                .await;
            self.notify("exit", Value::Null);
        }
        let child = self
            .inner
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(mut child) = child
            && tokio::time::timeout(Duration::from_millis(500), child.wait())
                .await
                .is_err()
        {
            let _ = child.kill().await;
        }
    }

    pub fn is_alive(&self) -> bool {
        self.inner.shared.alive.load(Ordering::SeqCst)
    }

    fn send(&self, msg: Value) {
        let _ = self.inner.out.send(transport::frame(&msg));
    }

    fn notify(&self, method: &str, params: Value) {
        if self.is_alive() {
            self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
        }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request_with(method, params, self.inner.timeout).await
    }

    /// Requête avec délai ; délai dépassé ou futur abandonné → `$/cancelRequest`.
    async fn request_with(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        if !self.is_alive() {
            return Err("serveur LSP arrêté".into());
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.inner.shared.pending().insert(id, tx);
        let guard = CancelOnDrop { client: self, id };
        // Mort entre-temps : la table a peut-être déjà été soldée sans nous.
        if !self.is_alive() {
            return Err("serveur LSP arrêté".into());
        }
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let res = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err("serveur LSP arrêté".into()),
            Err(_) => Err(format!(
                "{method} : pas de réponse du serveur LSP en {timeout:?}"
            )),
        };
        drop(guard);
        res
    }
}

/// Retire la requête de la table si elle y est encore, et prévient le serveur.
struct CancelOnDrop<'a> {
    client: &'a Client,
    id: i64,
}

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        if self
            .client
            .inner
            .shared
            .pending()
            .remove(&self.id)
            .is_some()
        {
            self.client
                .notify("$/cancelRequest", json!({ "id": self.id }));
        }
    }
}

fn doc_params(path: &Path) -> Value {
    json!({ "textDocument": { "uri": path_to_uri(path) } })
}

fn pos_params(path: &Path, pos: ide_core::Position) -> Value {
    json!({ "textDocument": { "uri": path_to_uri(path) }, "position": position_json(pos) })
}

/// Lit les messages du serveur jusqu'à la fin du flux, puis solde les requêtes en attente.
async fn read_loop<R: AsyncRead + Unpin>(
    mut r: BufReader<R>,
    shared: Arc<Shared>,
    out: mpsc::UnboundedSender<Vec<u8>>,
    notif: mpsc::UnboundedSender<Notification>,
    name: String,
) {
    while let Some(msg) = transport::read_message(&mut r).await {
        let method = msg["method"].as_str();
        match (method, msg.get("id")) {
            // Requête serveur → client : réponse neutre pour ne pas le bloquer.
            (Some(method), Some(id)) => {
                let reply = match method {
                    "workspace/configuration" => {
                        let n = msg["params"]["items"].as_array().map_or(0, Vec::len);
                        json!({ "jsonrpc": "2.0", "id": id, "result": vec![Value::Null; n] })
                    }
                    "client/registerCapability"
                    | "client/unregisterCapability"
                    | "window/workDoneProgress/create" => {
                        json!({ "jsonrpc": "2.0", "id": id, "result": null })
                    }
                    _ => json!({ "jsonrpc": "2.0", "id": id,
                        "error": { "code": -32601, "message": format!("non pris en charge : {method}") } }),
                };
                let _ = out.send(transport::frame(&reply));
            }
            (Some("textDocument/publishDiagnostics"), None) => {
                match convert::diagnostics(&msg["params"]) {
                    Some((path, items)) => {
                        let _ = notif.send(Notification::Diagnostics { path, items });
                    }
                    None => tracing::warn!("LSP : diagnostics illisibles ignorés"),
                }
            }
            (Some("window/showMessage"), None) => {
                if let Some(text) = msg["params"]["message"].as_str() {
                    let _ = notif.send(Notification::Message(text.to_string()));
                }
            }
            (Some(_), None) => {} // logMessage, $/progress…
            (None, id) => {
                let tx = id
                    .and_then(Value::as_i64)
                    .and_then(|id| shared.pending().remove(&id));
                let Some(tx) = tx else { continue };
                let res = match msg.get("error") {
                    Some(e) => Err(format!(
                        "erreur LSP : {}",
                        e["message"].as_str().unwrap_or("inconnue")
                    )),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(res);
            }
        }
    }
    shared.alive.store(false, Ordering::SeqCst);
    // Vider la table renvoie Err à chaque appelant (oneshot fermé).
    shared.pending().clear();
    let _ = notif.send(Notification::Exited(format!(
        "serveur LSP « {name} » arrêté ; l'édition continue sans lui"
    )));
}
