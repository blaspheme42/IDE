//! Crate ide-syntax : coloration tree-sitter (F-30, J3).
//! Produit des catégories (`Highlight`) ; les couleurs sont choisies par les thèmes de l'UI.

use std::ops::Range;
use std::path::Path;

use tree_sitter::{InputEdit, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

/// Au-delà, pas de coloration (NF-03).
const MAX_LEN: usize = 5 * 1024 * 1024;

/// Langage reconnu par extension ou nom de fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    C,
    JavaScript,
    TypeScript,
    Tsx,
    Html,
    Css,
    Json,
    Toml,
    Markdown,
}

impl Language {
    /// Langage d'après le nom de fichier puis l'extension (insensible à la casse).
    pub fn from_path(path: &Path) -> Option<Self> {
        if path.file_name()? == "Cargo.lock" {
            return Some(Self::Toml);
        }
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => Self::Rust,
            "py" | "pyi" => Self::Python,
            "c" | "h" => Self::C,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "html" | "htm" => Self::Html,
            "css" => Self::Css,
            "json" => Self::Json,
            "toml" => Self::Toml,
            "md" | "markdown" => Self::Markdown,
            _ => return None,
        })
    }

    /// Nom affichable.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::C => "C",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Tsx => "TSX",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Markdown => "Markdown",
        }
    }

    /// Grammaire et requête de coloration fournies par la crate de la grammaire.
    fn grammar(self) -> (tree_sitter::Language, String) {
        // Le premier motif gagne à nœud égal : JSX avant les identifiants génériques de JS.
        let js = || {
            format!(
                "{}\n{}",
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_javascript::HIGHLIGHT_QUERY
            )
        };
        match self {
            Self::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY.into(),
            ),
            Self::Python => (
                tree_sitter_python::LANGUAGE.into(),
                tree_sitter_python::HIGHLIGHTS_QUERY.into(),
            ),
            Self::C => (
                tree_sitter_c::LANGUAGE.into(),
                tree_sitter_c::HIGHLIGHT_QUERY.into(),
            ),
            Self::JavaScript => (tree_sitter_javascript::LANGUAGE.into(), js()),
            // La requête TypeScript complète celle de JavaScript (placée avant : elle prime).
            Self::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                format!(
                    "{}\n{}",
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY
                ),
            ),
            Self::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                format!("{}\n{}", tree_sitter_typescript::HIGHLIGHTS_QUERY, js()),
            ),
            Self::Html => (
                tree_sitter_html::LANGUAGE.into(),
                tree_sitter_html::HIGHLIGHTS_QUERY.into(),
            ),
            Self::Css => (
                tree_sitter_css::LANGUAGE.into(),
                tree_sitter_css::HIGHLIGHTS_QUERY.into(),
            ),
            Self::Json => (
                tree_sitter_json::LANGUAGE.into(),
                tree_sitter_json::HIGHLIGHTS_QUERY.into(),
            ),
            Self::Toml => (
                tree_sitter_toml_ng::LANGUAGE.into(),
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(),
            ),
            // shortcut: grammaire bloc seule (pas d'emphase ni de liens en ligne) ; ajouter INLINE_LANGUAGE si besoin.
            Self::Markdown => (
                tree_sitter_md::LANGUAGE.into(),
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.into(),
            ),
        }
    }
}

/// Catégories de coloration, partagées avec les thèmes de l'UI (une couleur par variante).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Highlight {
    Keyword,
    Function,
    Type,
    String,
    Number,
    Comment,
    Constant,
    Variable,
    Property,
    Operator,
    Punctuation,
    Attribute,
    Tag,
    Label,
    Namespace,
    Macro,
    Escape,
    Heading,
    Link,
    Emphasis,
}

impl Highlight {
    /// Catégorie d'un nom de capture (`function.method`, `text.title`…) ; `None` si inconnu.
    fn from_capture(name: &str) -> Option<Self> {
        use Highlight as H;
        // Exceptions d'abord (nom complet ou préfixe), puis le premier segment.
        let exact = match name {
            "function.macro" => Some(H::Macro),
            "string.escape" => Some(H::Escape),
            "string.special.key" => Some(H::Property),
            "constructor" => Some(H::Type),
            "boolean" => Some(H::Constant),
            "text.title" | "markup.heading" => Some(H::Heading),
            "text.uri" | "text.reference" | "markup.link" => Some(H::Link),
            "text.emphasis" | "text.strong" | "markup.italic" | "markup.bold" => Some(H::Emphasis),
            "text.literal" | "markup.raw" => Some(H::String),
            _ => None,
        };
        if exact.is_some() {
            return exact;
        }
        if name.starts_with("markup.heading") {
            return Some(H::Heading);
        }
        Some(match name.split('.').next()? {
            "keyword" | "import" | "charset" | "media" | "keyframes" | "supports" => H::Keyword,
            "function" | "method" => H::Function,
            "type" => H::Type,
            "string" => H::String,
            "number" | "float" => H::Number,
            "comment" => H::Comment,
            "constant" => H::Constant,
            "variable" | "parameter" => H::Variable,
            "property" => H::Property,
            "operator" => H::Operator,
            "punctuation" | "delimiter" => H::Punctuation,
            "attribute" => H::Attribute,
            "tag" => H::Tag,
            "label" => H::Label,
            "namespace" | "module" => H::Namespace,
            "macro" => H::Macro,
            "escape" => H::Escape,
            _ => return None,
        })
    }
}

/// Arbre syntaxique d'un document, réutilisé entre deux éditions (incrémental).
pub struct Syntax {
    parser: Parser,
    /// `None` si le texte dépasse 5 Mo après une édition, ou si l'analyse échoue.
    tree: Option<Tree>,
    language: Language,
    query: Query,
    /// Catégorie de chaque capture de `query`, par indice.
    captures: Vec<Option<Highlight>>,
}

impl Syntax {
    /// `None` si le texte dépasse 5 Mo (NF-03) ou si la grammaire ou sa requête échoue.
    pub fn new(lang: Language, text: &str) -> Option<Self> {
        if text.len() > MAX_LEN {
            return None;
        }
        let (grammar, source) = lang.grammar();
        let query = Query::new(&grammar, &source).ok()?;
        let captures = query
            .capture_names()
            .iter()
            .map(|n| Highlight::from_capture(n))
            .collect();
        let mut parser = Parser::new();
        parser.set_language(&grammar).ok()?;
        let tree = Some(parser.parse(text, None)?);
        Some(Self {
            parser,
            tree,
            language: lang,
            query,
            captures,
        })
    }

    /// Langage du document.
    pub fn language(&self) -> Language {
        self.language
    }

    /// Édition incrémentale : positions en OCTETS et en (ligne, colonne-octet) avant/après,
    /// comme `tree_sitter::InputEdit` ; puis reparse avec le nouveau texte.
    #[allow(clippy::too_many_arguments)]
    pub fn edit(
        &mut self,
        start_byte: usize,
        old_end_byte: usize,
        new_end_byte: usize,
        start: (usize, usize),
        old_end: (usize, usize),
        new_end: (usize, usize),
        new_text: &str,
    ) {
        if new_text.len() > MAX_LEN {
            self.tree = None;
            return;
        }
        let point = |(row, column)| Point { row, column };
        if let Some(tree) = &mut self.tree {
            tree.edit(&InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position: point(start),
                old_end_position: point(old_end),
                new_end_position: point(new_end),
            });
        }
        // Sans arbre précédent (texte redescendu sous 5 Mo) : analyse complète.
        self.tree = self.parser.parse(new_text, self.tree.as_ref());
    }

    /// Plages colorées (octets) qui touchent `range` (octets), triées, sans chevauchement
    /// (la capture la plus spécifique gagne). Les plages sont rognées à `range`.
    pub fn highlights(&self, text: &str, range: Range<usize>) -> Vec<(Range<usize>, Highlight)> {
        let range = range.start.min(text.len())..range.end.min(text.len());
        let Some(tree) = &self.tree else {
            return Vec::new();
        };
        if range.is_empty() {
            return Vec::new();
        }
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        // (début, fin, indice du motif, catégorie)
        let mut found = Vec::new();
        let mut matches = cursor.matches(&self.query, tree.root_node(), text.as_bytes());
        while let Some(m) = matches.next() {
            for c in m.captures {
                if let Some(Some(h)) = self.captures.get(c.index as usize) {
                    let r = c.node.byte_range();
                    let (s, e) = (r.start.max(range.start), r.end.min(range.end));
                    if s < e {
                        found.push((s, e, r.len(), m.pattern_index, *h));
                    }
                }
            }
        }
        // Peinture : les grands nœuds d'abord, les petits (plus spécifiques) par-dessus ;
        // à nœud égal, le premier motif de la requête gagne (convention tree-sitter).
        found.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)));
        let mut paint: Vec<Option<Highlight>> = vec![None; range.len()];
        for (s, e, _, _, h) in found {
            paint[s - range.start..e - range.start].fill(Some(h));
        }
        let mut out: Vec<(Range<usize>, Highlight)> = Vec::new();
        for (i, h) in paint.into_iter().enumerate() {
            let Some(h) = h else { continue };
            let pos = range.start + i;
            match out.last_mut() {
                Some((r, last)) if r.end == pos && *last == h => r.end += 1,
                _ => out.push((pos..pos + 1, h)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
