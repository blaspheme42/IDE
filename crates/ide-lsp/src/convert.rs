//! Conversions JSON LSP ↔ types de `ide_core`, URI `file://` ↔ chemin.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use ide_core::{CompletionItem, Diagnostic, Location, Position, Severity};
use serde_json::{Value, json};

/// `file://` + octets du chemin, encodés en `%XX` hors caractères non réservés et `/`.
// shortcut: chemins Unix seulement (pas de `C:\`), à revoir pour un portage Windows.
pub(crate) fn path_to_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            let _ = write!(uri, "%{b:02X}");
        }
    }
    uri
}

/// Inverse de [`path_to_uri`] ; accepte `file://localhost/…`. `None` si pas un `file://`.
pub(crate) fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out).into_owned()))
}

pub(crate) fn position_json(p: Position) -> Value {
    json!({ "line": p.line, "character": p.character })
}

fn position(v: &Value) -> Option<Position> {
    Some(Position {
        line: v["line"].as_u64()? as usize,
        character: v["character"].as_u64()? as usize,
    })
}

fn range(v: &Value) -> Option<(Position, Position)> {
    Some((position(&v["start"])?, position(&v["end"])?))
}

/// Paramètres de `textDocument/publishDiagnostics`. Sévérité absente → erreur.
pub(crate) fn diagnostics(params: &Value) -> Option<(PathBuf, Vec<Diagnostic>)> {
    let path = uri_to_path(params["uri"].as_str()?)?;
    let items = params["diagnostics"].as_array().into_iter().flatten();
    let items = items.filter_map(|d| {
        let (start, end) = range(&d["range"])?;
        let severity = match d["severity"].as_u64() {
            Some(2) => Severity::Warning,
            Some(3) => Severity::Info,
            Some(4) => Severity::Hint,
            _ => Severity::Error,
        };
        Some(Diagnostic {
            start,
            end,
            severity,
            message: d["message"].as_str()?.to_string(),
            source: d["source"].as_str().map(Into::into),
        })
    });
    Some((path, items.collect()))
}

/// Libellé court d'un `CompletionItemKind`.
fn kind_label(kind: u64) -> Option<&'static str> {
    const LABELS: [&str; 25] = [
        "text", "method", "fn", "ctor", "field", "var", "class", "iface", "mod", "prop", "unit",
        "value", "enum", "kw", "snippet", "color", "file", "ref", "folder", "variant", "const",
        "struct", "event", "op", "type",
    ];
    LABELS
        .get(usize::try_from(kind).ok()?.checked_sub(1)?)
        .copied()
}

/// Retire les marqueurs de snippet : `$0`, `$1`, `${1}` disparaissent, `${1:texte}` → `texte`.
// shortcut: choix `${1|a,b|}` et variables `$TM_…` laissés tels quels, rares en complétion.
pub(crate) fn strip_snippet(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut open = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('$' | '}' | '\\')) => {
                out.extend(chars.next());
            }
            '$' if chars.peek().is_some_and(char::is_ascii_digit) => {
                while chars.next_if(char::is_ascii_digit).is_some() {}
            }
            '$' if chars.peek() == Some(&'{') => {
                chars.next();
                while chars.next_if(char::is_ascii_digit).is_some() {}
                if chars.next_if_eq(&':').is_some() {
                    open += 1;
                } else {
                    chars.next_if_eq(&'}');
                }
            }
            '}' if open > 0 => open -= 1,
            c => out.push(c),
        }
    }
    out
}

/// Réponse de `textDocument/completion` : `CompletionList`, tableau ou `null`.
pub(crate) fn completion(v: &Value) -> Vec<CompletionItem> {
    let items = v.as_array().or_else(|| v["items"].as_array());
    items
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let label = it["label"].as_str()?.to_string();
            let text = it["insertText"]
                .as_str()
                .or_else(|| it["textEdit"]["newText"].as_str())
                .unwrap_or(&label);
            let insert_text = if it["insertTextFormat"].as_u64() == Some(2) {
                strip_snippet(text)
            } else {
                text.to_string()
            };
            // `TextEdit` (range) ou `InsertReplaceEdit` (insert).
            let edit = &it["textEdit"];
            let range = if edit["range"].is_object() {
                &edit["range"]
            } else {
                &edit["insert"]
            };
            let replace_start = range["start"]["line"]
                .as_u64()
                .zip(range["start"]["character"].as_u64())
                .map(|(line, character)| ide_core::Position {
                    line: line as usize,
                    character: character as usize,
                });
            Some(CompletionItem {
                replace_start,
                kind: it["kind"].as_u64().and_then(kind_label).map(Into::into),
                detail: it["detail"].as_str().map(Into::into),
                insert_text,
                label,
            })
        })
        .collect()
}

/// Réponse de `textDocument/hover` : `MarkupContent`, `MarkedString` ou tableau → texte brut.
pub(crate) fn hover(v: &Value) -> Option<String> {
    let marked = |m: &Value| match m {
        Value::String(s) => Some(s.clone()),
        _ => m["value"].as_str().map(Into::into),
    };
    let text = match &v["contents"] {
        Value::Array(a) => a.iter().filter_map(marked).collect::<Vec<_>>().join("\n\n"),
        c => marked(c)?,
    };
    (!text.trim().is_empty()).then_some(text)
}

/// Réponse de `textDocument/definition` : `Location`, `Location[]` ou `LocationLink[]`.
pub(crate) fn locations(v: &Value) -> Vec<Location> {
    let one = |l: &Value| {
        let (uri, r) = match l["targetUri"].as_str() {
            Some(u) => (u, &l["targetSelectionRange"]),
            None => (l["uri"].as_str()?, &l["range"]),
        };
        let (start, end) = range(r)?;
        Some(Location {
            path: uri_to_path(uri)?,
            start,
            end,
        })
    };
    match v {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        Value::Object(_) => one(v).into_iter().collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_aller_retour() {
        let p = Path::new("/tmp/mon dossier/été #1%/a.rs");
        let uri = path_to_uri(p);
        assert_eq!(
            uri,
            "file:///tmp/mon%20dossier/%C3%A9t%C3%A9%20%231%25/a.rs"
        );
        assert_eq!(uri_to_path(&uri).as_deref(), Some(p));
        assert_eq!(uri_to_path("file://localhost/a%2"), Some("/a%2".into()));
        assert_eq!(uri_to_path("https://x"), None);
    }

    #[test]
    fn snippets() {
        assert_eq!(strip_snippet("foo($1)$0"), "foo()");
        assert_eq!(strip_snippet("foo(${1:a}, ${2:b})"), "foo(a, b)");
        assert_eq!(strip_snippet("if ${1:x == ${2:y}} {$0}"), "if x == y {}");
        assert_eq!(strip_snippet(r"a\$b${3}"), "a$b");
    }

    #[test]
    fn libelles() {
        assert_eq!(kind_label(3), Some("fn"));
        assert_eq!(kind_label(22), Some("struct"));
        assert_eq!(kind_label(0), None);
        assert_eq!(kind_label(99), None);
    }
}
