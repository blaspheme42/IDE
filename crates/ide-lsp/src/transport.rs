//! Cadrage des messages : `Content-Length: N\r\n\r\n<json>`.

use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

/// Message prêt à écrire.
pub(crate) fn frame(msg: &Value) -> Vec<u8> {
    let body = msg.to_string();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Prochain message JSON valide ; `None` en fin de flux (ou message tronqué par la fin).
/// En-têtes inconnus (`Content-Type`…) ignorés ; JSON invalide journalisé puis sauté.
pub(crate) async fn read_message<R: AsyncBufRead + Unpin>(r: &mut R) -> Option<Value> {
    let mut line = Vec::new();
    loop {
        let mut len: Option<usize> = None;
        loop {
            line.clear();
            if r.read_until(b'\n', &mut line).await.ok()? == 0 {
                return None;
            }
            let text = String::from_utf8_lossy(&line);
            let text = text.trim();
            if text.is_empty() {
                // Ligne vide sans Content-Length : bruit avant les en-têtes, on continue.
                if len.is_some() {
                    break;
                }
                continue;
            }
            if let Some((k, v)) = text.split_once(':')
                && k.trim().eq_ignore_ascii_case("content-length")
            {
                len = v.trim().parse().ok();
            }
        }
        let mut body = vec![0; len.unwrap_or(0)];
        if let Err(e) = r.read_exact(&mut body).await {
            tracing::warn!("LSP : message tronqué ({e})");
            return None;
        }
        match serde_json::from_slice(&body) {
            Ok(v) => return Some(v),
            Err(e) => tracing::warn!("LSP : JSON invalide ignoré ({e})"),
        }
    }
}
