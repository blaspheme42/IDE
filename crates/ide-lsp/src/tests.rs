//! Faux serveur en processus (`tokio::io::duplex`) : le test joue le rôle du serveur.
#![allow(clippy::unwrap_used)]

use super::*;
use ide_core::{Position, Severity};
use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

struct Fake {
    r: BufReader<ReadHalf<DuplexStream>>,
    w: WriteHalf<DuplexStream>,
}

impl Fake {
    async fn recv(&mut self) -> Value {
        transport::read_message(&mut self.r)
            .await
            .expect("message du client")
    }

    async fn raw(&mut self, bytes: &[u8]) {
        self.w.write_all(bytes).await.unwrap();
    }

    async fn send(&mut self, v: Value) {
        self.raw(&transport::frame(&v)).await;
    }

    /// Lit la requête suivante (doit être `method`) et y répond `result`.
    async fn answer(&mut self, method: &str, result: Value) -> Value {
        let req = self.recv().await;
        assert_eq!(req["method"], method);
        self.send(json!({ "jsonrpc": "2.0", "id": req["id"], "result": result }))
            .await;
        req
    }
}

fn setup(timeout: Duration) -> (Client, mpsc::UnboundedReceiver<Notification>, Fake) {
    let (a, b) = tokio::io::duplex(1 << 20);
    let (ar, aw) = tokio::io::split(a);
    let (br, bw) = tokio::io::split(b);
    let (c, rx) = Client::with_io(ar, aw, "faux", timeout);
    (
        c,
        rx,
        Fake {
            r: BufReader::new(br),
            w: bw,
        },
    )
}

const P: Position = Position {
    line: 1,
    character: 2,
};

fn path() -> &'static Path {
    Path::new("/tmp/un dossier/é.rs")
}

#[tokio::test]
async fn initialize_puis_initialized() {
    let (c, _rx, mut s) = setup(TIMEOUT);
    let (res, ()) = tokio::join!(
        c.initialize(
            Path::new("/tmp/projet"),
            json!({ "provideFormatter": true })
        ),
        async {
            let req = s.answer("initialize", json!({ "capabilities": {} })).await;
            let p = &req["params"];
            assert_eq!(p["rootUri"], "file:///tmp/projet");
            assert_eq!(p["initializationOptions"]["provideFormatter"], true);
            assert_eq!(
                p["capabilities"]["general"]["positionEncodings"],
                json!(["utf-16"])
            );
            assert_eq!(s.recv().await["method"], "initialized");
        }
    );
    res.unwrap();
}

#[tokio::test]
async fn notifications_du_serveur() {
    let (_c, mut rx, mut s) = setup(TIMEOUT);
    let range =
        json!({ "start": { "line": 0, "character": 1 }, "end": { "line": 0, "character": 3 } });
    s.send(json!({ "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
        "uri": path_to_uri(path()),
        "diagnostics": [{ "range": range, "severity": 2, "message": "oups", "source": "rustc" }],
    }}))
    .await;
    s.send(json!({ "method": "window/logMessage", "params": { "message": "bruit" } }))
        .await;
    s.send(json!({ "method": "window/showMessage", "params": { "message": "salut" } }))
        .await;
    let Some(Notification::Diagnostics { path: p, items }) = rx.recv().await else {
        panic!("diagnostics attendus")
    };
    assert_eq!(p, path());
    assert_eq!(items[0].severity, Severity::Warning);
    assert_eq!(
        items[0].end,
        Position {
            line: 0,
            character: 3
        }
    );
    assert_eq!(items[0].source.as_deref(), Some("rustc"));
    assert_eq!(rx.recv().await, Some(Notification::Message("salut".into())));
}

#[tokio::test]
async fn completion_liste_tableau_snippets() {
    let (c, _rx, mut s) = setup(TIMEOUT);
    let list = json!({ "isIncomplete": false, "items": [
        { "label": "a", "insertText": "aa", "kind": 3, "detail": "fn a()" },
        { "label": "b", "insertTextFormat": 2, "kind": 2,
          "textEdit": { "newText": "b(${1:x})$0", "range": {} } },
        { "label": "S", "kind": 22 },
    ]});
    let (res, req) = tokio::join!(
        c.completion(path(), P),
        s.answer("textDocument/completion", list)
    );
    assert_eq!(
        req["params"]["position"],
        json!({ "line": 1, "character": 2 })
    );
    let (items, incomplete) = res.unwrap();
    assert!(!incomplete);
    let got: Vec<_> = items
        .iter()
        .map(|i| (i.insert_text.as_str(), i.kind.as_deref()))
        .collect();
    assert_eq!(
        got,
        [
            ("aa", Some("fn")),
            ("b(x)", Some("method")),
            ("S", Some("struct"))
        ]
    );
    assert_eq!(items[0].detail.as_deref(), Some("fn a()"));

    let arr = json!([{ "label": "k" }]);
    let (res, _) = tokio::join!(
        c.completion(path(), P),
        s.answer("textDocument/completion", arr)
    );
    let (items, incomplete) = res.unwrap();
    assert_eq!((items[0].insert_text.as_str(), incomplete), ("k", false));
    let (res, _) = tokio::join!(
        c.completion(path(), P),
        s.answer("textDocument/completion", Value::Null)
    );
    assert_eq!(res.unwrap(), (vec![], false));
    // `isIncomplete` vrai, puis absent d'une `CompletionList`.
    let (res, _) = tokio::join!(
        c.completion(path(), P),
        s.answer(
            "textDocument/completion",
            json!({ "isIncomplete": true, "items": [{ "label": "x" }] })
        )
    );
    assert!(res.unwrap().1);
    let (res, _) = tokio::join!(
        c.completion(path(), P),
        s.answer("textDocument/completion", json!({ "items": [] }))
    );
    assert!(!res.unwrap().1);
}

#[tokio::test]
async fn hover_trois_formats() {
    let (c, _rx, mut s) = setup(TIMEOUT);
    let cases = [
        (
            json!({ "contents": { "kind": "markdown", "value": "**x**" } }),
            Some("**x**"),
        ),
        (
            json!({ "contents": { "language": "rust", "value": "fn f()" } }),
            Some("fn f()"),
        ),
        (
            json!({ "contents": ["a", { "language": "rust", "value": "b" }] }),
            Some("a\n\nb"),
        ),
        (json!({ "contents": "" }), None),
        (Value::Null, None),
    ];
    for (reply, want) in cases {
        let (res, _) = tokio::join!(c.hover(path(), P), s.answer("textDocument/hover", reply));
        assert_eq!(res.unwrap().as_deref(), want);
    }
}

#[tokio::test]
async fn definition_trois_formats() {
    let (c, _rx, mut s) = setup(TIMEOUT);
    let uri = path_to_uri(path());
    let r = json!({ "start": { "line": 4, "character": 0 }, "end": { "line": 4, "character": 5 } });
    let loc = json!({ "uri": uri, "range": r });
    let link = json!({ "targetUri": uri, "targetRange": {}, "targetSelectionRange": r });
    for reply in [loc.clone(), json!([loc]), json!([link])] {
        let (res, _) = tokio::join!(
            c.definition(path(), P),
            s.answer("textDocument/definition", reply)
        );
        let l = res.unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].path.clone(), l[0].start.line, l[0].end.character),
            (path().to_path_buf(), 4, 5)
        );
    }
    let (res, _) = tokio::join!(
        c.definition(path(), P),
        s.answer("textDocument/definition", Value::Null)
    );
    assert!(res.unwrap().is_empty());
}

#[tokio::test]
async fn requetes_du_serveur_repondues() {
    let (_c, _rx, mut s) = setup(TIMEOUT);
    let params = json!({ "items": [{ "section": "a" }, { "section": "b" }] });
    s.send(
        json!({ "jsonrpc": "2.0", "id": 7, "method": "workspace/configuration", "params": params }),
    )
    .await;
    assert_eq!(
        s.recv().await,
        json!({ "jsonrpc": "2.0", "id": 7, "result": [null, null] })
    );
    s.send(json!({ "id": "x", "method": "window/workDoneProgress/create", "params": {} }))
        .await;
    assert_eq!(s.recv().await["result"], Value::Null);
    s.send(json!({ "id": 8, "method": "inconnue" })).await;
    assert_eq!(s.recv().await["error"]["code"], -32601);
}

#[tokio::test]
async fn erreur_du_serveur() {
    let (c, _rx, mut s) = setup(TIMEOUT);
    let (res, ()) = tokio::join!(c.hover(path(), P), async {
        let req = s.recv().await;
        s.send(json!({ "id": req["id"], "error": { "code": -32603, "message": "boum" } }))
            .await;
    });
    assert_eq!(res.unwrap_err(), "erreur LSP : boum");
}

#[tokio::test]
async fn delai_depasse_puis_annulation() {
    let (c, _rx, mut s) = setup(Duration::from_millis(100));
    let (res, ()) = tokio::join!(c.hover(path(), P), async {
        let req = s.recv().await;
        let cancel = s.recv().await;
        assert_eq!(cancel["method"], "$/cancelRequest");
        assert_eq!(cancel["params"]["id"], req["id"]);
    });
    assert!(res.unwrap_err().contains("pas de réponse"));
    // Réponse tardive : ignorée sans dommage, le client reste utilisable.
    s.send(json!({ "id": 1, "result": null })).await;
    let (res, _) = tokio::join!(
        c.hover(path(), P),
        s.answer("textDocument/hover", json!({ "contents": "ok" }))
    );
    assert_eq!(res.unwrap().as_deref(), Some("ok"));
}

#[tokio::test]
async fn futur_abandonne_annule() {
    let (c, _rx, mut s) = setup(TIMEOUT);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), c.hover(path(), P))
            .await
            .is_err()
    );
    let req = s.recv().await;
    let cancel = s.recv().await;
    assert_eq!(cancel["method"], "$/cancelRequest");
    assert_eq!(cancel["params"]["id"], req["id"]);
    assert!(c.inner.shared.pending().is_empty());
}

#[tokio::test]
async fn serveur_qui_ferme_le_flux() {
    let (c, mut rx, mut s) = setup(TIMEOUT);
    let c2 = c.clone();
    let pending = tokio::spawn(async move { c2.hover(path(), P).await });
    s.recv().await;
    // Message tronqué puis fin du flux.
    s.raw(b"Content-Length: 100\r\n\r\n{\"id\"").await;
    drop(s);
    assert_eq!(pending.await.unwrap().unwrap_err(), "serveur LSP arrêté");
    assert!(matches!(rx.recv().await, Some(Notification::Exited(_))));
    assert!(!c.is_alive());
    assert!(c.completion(path(), P).await.is_err());
    c.did_open(path(), "rust", 1, "x");
    c.shutdown().await;
    assert_eq!(rx.recv().await, None, "Exited émise une seule fois");
}

#[tokio::test]
async fn messages_invalides_ignores() {
    let (_c, mut rx, mut s) = setup(TIMEOUT);
    s.raw(b"Content-Length: 3\r\n\r\n{{{").await;
    s.raw(b"X-Inconnu: 1\r\n\r\n").await;
    let body = br#"{"method":"window/showMessage","params":{"message":"ok"}}"#;
    let head = format!(
        "content-length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n",
        body.len()
    );
    s.raw(head.as_bytes()).await;
    s.raw(body).await;
    assert_eq!(rx.recv().await, Some(Notification::Message("ok".into())));
}

#[tokio::test]
async fn commande_introuvable() {
    let Err(e) = Client::start("serveur-qui-n-existe-pas", &[], Path::new("/")).await else {
        panic!("échec attendu")
    };
    assert!(e.contains("introuvable"), "{e}");
}

/// Vrai rust-analyzer sur un petit projet : `cargo test -p ide-lsp -- --ignored`.
#[tokio::test]
#[ignore]
async fn rust_analyzer_reel() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"essai\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    let main = root.join("src/main.rs");
    let text = "fn main() {\n    let x: i32 = \"a\";\n    \n}\n";
    std::fs::write(&main, text).unwrap();

    let t0 = std::time::Instant::now();
    let (c, mut rx) = Client::start("rust-analyzer", &[], root).await.unwrap();
    c.did_open(&main, "rust", 1, text);
    c.did_save(&main);
    let diags = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match rx.recv().await {
                Some(Notification::Diagnostics { path, items })
                    if path == main && !items.is_empty() =>
                {
                    return items;
                }
                Some(Notification::Exited(m)) | Some(Notification::Message(m)) => eprintln!("{m}"),
                None => panic!("canal fermé"),
                _ => {}
            }
        }
    })
    .await
    .expect("diagnostics en moins de 30 s");
    eprintln!("diagnostics en {:?} : {:?}", t0.elapsed(), diags[0].message);

    let pos = Position {
        line: 2,
        character: 4,
    };
    let mut items = vec![];
    while items.is_empty() && t0.elapsed() < Duration::from_secs(30) {
        items = c.completion(&main, pos).await.unwrap_or_default().0;
    }
    eprintln!("{} complétions en {:?}", items.len(), t0.elapsed());
    assert!(!items.is_empty());
    c.shutdown().await;
    let exited = tokio::time::timeout(Duration::from_secs(2), async {
        while !matches!(rx.recv().await, Some(Notification::Exited(_)) | None) {}
    });
    assert!(exited.await.is_ok());
    assert!(!c.is_alive());
}

#[test]
fn dossiers_d_en_tetes_pour_clangd() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for d in ["include", "src", "lib/sub", ".cache", "node_modules/x"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    for f in [
        "include/media.h",
        "src/main.c",
        "lib/sub/a.hpp",
        ".cache/b.h",
        "node_modules/x/c.h",
    ] {
        std::fs::write(root.join(f), "").unwrap();
    }
    let mut dirs = header_dirs(root);
    dirs.sort();
    assert_eq!(dirs, [root.join("include"), root.join("lib/sub")]);
}
