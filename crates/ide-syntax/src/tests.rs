use super::*;

const ALL: [Language; 11] = [
    Language::Rust,
    Language::Python,
    Language::C,
    Language::JavaScript,
    Language::TypeScript,
    Language::Tsx,
    Language::Html,
    Language::Css,
    Language::Json,
    Language::Toml,
    Language::Markdown,
];

/// Catégorie de la première occurrence de `word` dans `text`.
fn category(lang: Language, text: &str, word: &str) -> Option<Highlight> {
    let syntax = Syntax::new(lang, text).expect("grammaire");
    let at = text.find(word).expect("mot présent");
    syntax
        .highlights(text, 0..text.len())
        .into_iter()
        .find(|(r, _)| r.contains(&at))
        .map(|(_, h)| h)
}

fn assert_sorted_disjoint(hl: &[(Range<usize>, Highlight)]) {
    for w in hl.windows(2) {
        assert!(w[0].0.end <= w[1].0.start, "chevauchement : {w:?}");
    }
    assert!(hl.iter().all(|(r, _)| r.start < r.end));
}

#[test]
fn from_path() {
    let f = |p: &str| Language::from_path(Path::new(p));
    assert_eq!(f("src/main.rs"), Some(Language::Rust));
    assert_eq!(f("a.py"), Some(Language::Python));
    assert_eq!(f("a.h"), Some(Language::C));
    assert_eq!(f("a.mjs"), Some(Language::JavaScript));
    assert_eq!(f("a.ts"), Some(Language::TypeScript));
    assert_eq!(f("a.tsx"), Some(Language::Tsx));
    assert_eq!(f("index.HTML"), Some(Language::Html));
    assert_eq!(f("a.css"), Some(Language::Css));
    assert_eq!(f("package.json"), Some(Language::Json));
    assert_eq!(f("Cargo.toml"), Some(Language::Toml));
    assert_eq!(f("Cargo.lock"), Some(Language::Toml));
    assert_eq!(f("README.md"), Some(Language::Markdown));
    assert_eq!(f("a.xyz"), None);
    assert_eq!(f("Makefile"), None);
    assert_eq!(Language::Tsx.name(), "TSX");
}

#[test]
fn chaque_requete_compile() {
    for lang in ALL {
        assert!(Syntax::new(lang, "").is_some(), "{lang:?}");
    }
}

#[test]
fn neuf_langages_colores() {
    use Highlight as H;
    let cases: [(Language, &str, &str, H); 16] = [
        (
            Language::Rust,
            "fn main() { let x = \"a\"; } // c",
            "let",
            H::Keyword,
        ),
        (
            Language::Rust,
            "fn main() { let x = \"a\"; } // c",
            "\"a\"",
            H::String,
        ),
        (
            Language::Rust,
            "fn main() { println!(\"a\"); }",
            "println",
            H::Macro,
        ),
        (
            Language::Python,
            "def f():\n    return 'a'  # c\n",
            "def",
            H::Keyword,
        ),
        (
            Language::Python,
            "def f():\n    return 'a'  # c\n",
            "# c",
            H::Comment,
        ),
        (
            Language::C,
            "int main() { return 0; } /* c */",
            "return",
            H::Keyword,
        ),
        (
            Language::C,
            "int main() { return 0; } /* c */",
            "/* c */",
            H::Comment,
        ),
        (
            Language::JavaScript,
            "const a = 'x'; // c",
            "const",
            H::Keyword,
        ),
        (
            Language::JavaScript,
            "const a = <div>x</div>;",
            "div",
            H::Tag,
        ),
        (
            Language::TypeScript,
            "let a: string = 'x';",
            "let",
            H::Keyword,
        ),
        (Language::Tsx, "const a = <div>x</div>;", "div", H::Tag),
        (
            Language::Html,
            "<p class=\"a\">x</p><!-- c -->",
            "class",
            H::Attribute,
        ),
        (
            Language::Css,
            "a { color: red; } /* c */",
            "/* c */",
            H::Comment,
        ),
        (
            Language::Json,
            "{\"k\": \"v\", \"n\": 1}",
            "\"v\"",
            H::String,
        ),
        (
            Language::Toml,
            "[p]\nname = \"x\" # c\n",
            "\"x\"",
            H::String,
        ),
        (
            Language::Markdown,
            "# Titre\n\ntexte\n",
            "Titre",
            H::Heading,
        ),
    ];
    for (lang, text, word, want) in cases {
        assert_eq!(category(lang, text, word), Some(want), "{lang:?} {word}");
    }
}

#[test]
fn borne_a_la_plage() {
    let text = "fn a() {}\nfn b() {}\nfn c() {}\n";
    let syntax = Syntax::new(Language::Rust, text).expect("rust");
    let hl = syntax.highlights(text, 10..20);
    assert!(!hl.is_empty());
    assert!(
        hl.iter().all(|(r, _)| r.start >= 10 && r.end <= 20),
        "{hl:?}"
    );
    // Commentaire bloc commencé avant la plage : rogné.
    let text = "/* long\ncommentaire */ fn a() {}";
    let syntax = Syntax::new(Language::Rust, text).expect("rust");
    assert_eq!(
        syntax.highlights(text, 3..8),
        vec![(3..8, Highlight::Comment)]
    );
    assert!(syntax.highlights(text, 50..60).is_empty());
}

#[test]
fn triees_sans_chevauchement() {
    let text = "#[derive(Debug)]\npub struct S<'a> { x: &'a str }\nimpl S<'_> { fn f(&self) -> u8 { 0x1 + 2 } }\n";
    let syntax = Syntax::new(Language::Rust, text).expect("rust");
    let hl = syntax.highlights(text, 0..text.len());
    assert_sorted_disjoint(&hl);
    let md = "# T\n\n- a\n\n```rs\nx\n```\n[l]: http://x\n";
    let syntax = Syntax::new(Language::Markdown, md).expect("md");
    assert_sorted_disjoint(&syntax.highlights(md, 0..md.len()));
}

#[test]
fn edition_incrementale() {
    let before = "fn a() {}\n";
    let mut syntax = Syntax::new(Language::Rust, before).expect("rust");
    // Insertion d'une ligne « let s = "é"; » dans le corps.
    let ins = " let s = \"é\"; ";
    let at = before.find('{').expect("accolade") + 1;
    let after = format!("{}{ins}{}", &before[..at], &before[at..]);
    let end = at + ins.len();
    syntax.edit(at, at, end, (0, at), (0, at), (0, end), &after);
    let full = Syntax::new(Language::Rust, &after).expect("rust");
    assert_eq!(
        syntax.highlights(&after, 0..after.len()),
        full.highlights(&after, 0..after.len())
    );
    assert_eq!(
        category(Language::Rust, &after, "let"),
        Some(Highlight::Keyword)
    );
}

#[test]
fn au_dela_de_5_mo() {
    let big = "a".repeat(MAX_LEN + 1);
    assert!(Syntax::new(Language::Rust, &big).is_none());
    let mut syntax = Syntax::new(Language::Rust, "fn a() {}").expect("rust");
    syntax.edit(0, 9, big.len(), (0, 0), (0, 9), (0, big.len()), &big);
    assert!(syntax.highlights(&big, 0..100).is_empty());
}

#[test]
fn octets_utf8() {
    let text = "// é🦀\nlet s = \"àé🦀\";";
    let syntax = Syntax::new(Language::Rust, text).expect("rust");
    let hl = syntax.highlights(text, 0..text.len());
    let at = text.find('"').expect("guillemet");
    let string = hl
        .iter()
        .find(|(_, h)| *h == Highlight::String)
        .expect("chaîne");
    assert_eq!(&text[string.0.clone()], "\"àé🦀\"");
    assert_eq!(string.0.start, at);
    assert_eq!(&text[hl[0].0.clone()], "// é🦀");
}

/// Mesure une frappe au milieu de `text` puis la coloration de 50 lignes autour.
fn frappe(text: &mut String) -> std::time::Duration {
    let mut syntax = Syntax::new(Language::Rust, text).expect("rust");
    let mid = text.len() / 2;
    let at = mid + text[mid..].find("let x").expect("let x") + 5;
    let row = text[..at].matches('\n').count();
    let col = at - text[..at].rfind('\n').map_or(0, |i| i + 1);
    let t = std::time::Instant::now();
    text.insert(at, 'y');
    syntax.edit(at, at, at + 1, (row, col), (row, col), (row, col + 1), text);
    let start = text[..at]
        .match_indices('\n')
        .nth_back(25)
        .map_or(0, |(i, _)| i + 1);
    let end = text[at..]
        .match_indices('\n')
        .nth(24)
        .map_or(text.len(), |(i, _)| at + i);
    let hl = syntax.highlights(text, start..end);
    let dt = t.elapsed();
    assert!(!hl.is_empty());
    dt
}

/// `cargo test -p ide-syntax --release -- --ignored --nocapture perf`
#[test]
#[ignore]
fn perf_10_000_lignes() {
    // Fichier réaliste : 2 000 fonctions de 5 lignes.
    let mut text = String::new();
    for i in 0..2_000 {
        text.push_str(&format!(
            "fn f{i}(a: u32) -> u32 {{\n    let x = compute(\"texte\", 42); // c\n    x + a\n}}\n\n"
        ));
    }
    let dt = frappe(&mut text);
    println!("fonctions : frappe + 50 lignes en {dt:?}");
    // Cas défavorable : une seule fonction de 10 000 instructions (mesuré, non borné).
    let mut flat = String::from("fn main() {\n");
    for _ in 0..10_000 {
        flat.push_str("    let x: u32 = compute(\"texte\", 42); // commentaire\n");
    }
    flat.push_str("}\n");
    println!("bloc plat : frappe + 50 lignes en {:?}", frappe(&mut flat));
    assert!(dt.as_millis() < 16, "{dt:?}");
}
