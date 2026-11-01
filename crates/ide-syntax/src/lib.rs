//! Crate ide-syntax : coloration tree-sitter (F-30, J3).
//! Produit des catégories (`Highlight`) ; les couleurs sont choisies par les thèmes de l'UI.

use std::ops::Range;
use std::path::Path;

use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

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
    Php,
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
            "php" | "phtml" => Self::Php,
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
            Self::Php => "PHP",
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
            // Le HTML hors des balises <?php ?> est coloré par un second arbre (`Syntax::html`).
            Self::Php => (
                tree_sitter_php::LANGUAGE_PHP.into(),
                tree_sitter_php::HIGHLIGHTS_QUERY.into(),
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
    /// Nom de la variante en minuscules (`"keyword"`…), clé des thèmes de l'UI.
    pub fn name(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Function => "function",
            Self::Type => "type",
            Self::String => "string",
            Self::Number => "number",
            Self::Comment => "comment",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::Property => "property",
            Self::Operator => "operator",
            Self::Punctuation => "punctuation",
            Self::Attribute => "attribute",
            Self::Tag => "tag",
            Self::Label => "label",
            Self::Namespace => "namespace",
            Self::Macro => "macro",
            Self::Escape => "escape",
            Self::Heading => "heading",
            Self::Link => "link",
            Self::Emphasis => "emphasis",
        }
    }

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
    /// PHP : HTML hors des balises `<?php ?>`, analysé sur ces seules plages.
    html: Option<Box<Syntax>>,
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
        let html = match lang {
            Language::Php => Some(Box::new(Syntax::new(Language::Html, "")?)),
            _ => None,
        };
        let mut syntax = Self {
            parser,
            tree,
            language: lang,
            query,
            captures,
            html,
        };
        syntax.reparse_html(text);
        Some(syntax)
    }

    /// Reparse le HTML d'un fichier PHP sur les nœuds `text` (hors `<?php ?>`).
    // shortcut: HTML reparsé en entier à chaque édition (ses plages bougent), à rendre incrémental si de gros PHP rament.
    fn reparse_html(&mut self, text: &str) {
        let Some(html) = &mut self.html else {
            return;
        };
        let mut ranges = Vec::new();
        if let Some(tree) = &self.tree {
            collect_text(tree.root_node(), &mut ranges);
        }
        html.tree = if ranges.is_empty() {
            None
        } else {
            html.parser
                .set_included_ranges(&ranges)
                .ok()
                .and_then(|()| html.parser.parse(text, None))
        };
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
        self.apply_edit(
            start_byte,
            old_end_byte,
            new_end_byte,
            start,
            old_end,
            new_end,
        );
        self.reparse(new_text);
    }

    /// Reporte une édition sur l'arbre sans reparser : pour un lot, plusieurs `apply_edit` puis un `reparse`.
    pub fn apply_edit(
        &mut self,
        start_byte: usize,
        old_end_byte: usize,
        new_end_byte: usize,
        start: (usize, usize),
        old_end: (usize, usize),
        new_end: (usize, usize),
    ) {
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
    }

    /// Reparse incrémental après `apply_edit` ; au-delà de 5 Mo, plus d'arbre (NF-03).
    pub fn reparse(&mut self, new_text: &str) {
        if new_text.len() > MAX_LEN {
            self.tree = None;
            self.reparse_html(new_text);
            return;
        }
        self.tree = self.parser.parse(new_text, self.tree.as_ref());
        self.reparse_html(new_text);
    }

    pub fn highlights(&self, text: &str, range: Range<usize>) -> Vec<(Range<usize>, Highlight)> {
        let range = range.start.min(text.len())..range.end.min(text.len());
        if range.is_empty() {
            return Vec::new();
        }
        let mut paint: Vec<Option<Highlight>> = vec![None; range.len()];
        // HTML d'abord : le PHP repeint par-dessus (ex. `<?= $x ?>` dans un attribut).
        if let Some(html) = &self.html {
            html.paint(text, &range, &mut paint);
        }
        self.paint(text, &range, &mut paint);
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

    /// Peint les captures de `range` dans `paint` (un octet par case).
    fn paint(&self, text: &str, range: &Range<usize>, paint: &mut [Option<Highlight>]) {
        let Some(tree) = &self.tree else {
            return;
        };
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
        for (s, e, _, _, h) in found {
            paint[s - range.start..e - range.start].fill(Some(h));
        }
    }
}

/// Plages des nœuds `text` (HTML hors `<?php ?>`), dans l'ordre du document.
fn collect_text(node: Node, out: &mut Vec<tree_sitter::Range>) {
    if node.kind() == "text" {
        out.push(node.range());
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_text(child, out);
    }
}

#[cfg(test)]
mod tests;
