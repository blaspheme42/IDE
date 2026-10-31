//! Coloration : un arbre tree-sitter par document ouvert, tenu à jour avant chaque rendu (F-30, J3).

use std::collections::HashMap;

use ide_core::{Document, State, editor_size};
use ide_syntax::{Language, Syntax};

/// Arbres indexés par `Document::syntax_id` ; `None` : pas de langage ou fichier trop gros (NF-03).
#[derive(Default)]
pub struct Highlighter {
    trees: HashMap<u64, (Option<Language>, Option<Syntax>)>,
    next_id: u64,
}

impl Highlighter {
    /// Met à jour les arbres de tous les onglets puis `state.highlights` (lignes visibles de l'actif).
    pub fn sync(&mut self, state: &mut State) {
        let docs = std::iter::once(&mut state.doc).chain(state.tabs.iter_mut().map(|t| &mut t.doc));
        let live: Vec<u64> = docs.map(|d| self.sync_doc(d)).collect();
        self.trees.retain(|id, _| live.contains(id));
        state.highlights = self.visible(state);
    }

    /// Crée l'arbre, le recrée si le langage a changé (renommage), sinon applique les éditions en un reparse.
    fn sync_doc(&mut self, doc: &mut Document) -> u64 {
        let edits = doc.take_edits();
        let lang = doc.path.as_deref().and_then(Language::from_path);
        if let Some(id) = doc.syntax_id
            && let Some((l, syntax)) = self.trees.get_mut(&id)
            && *l == lang
        {
            if let Some(s) = syntax
                && !edits.is_empty()
            {
                for e in &edits {
                    s.apply_edit(
                        e.start_byte,
                        e.old_end_byte,
                        e.new_end_byte,
                        e.start,
                        e.old_end,
                        e.new_end,
                    );
                }
                // shortcut: copie O(n) de la rope à chaque reparse (~0,1 ms pour 10 000 lignes), passer par `parse_with` + chunks si ça se voit.
                s.reparse(&doc.rope().to_string());
            }
            return id;
        }
        let id = doc.syntax_id.unwrap_or_else(|| {
            self.next_id += 1;
            self.next_id
        });
        doc.syntax_id = Some(id);
        let syntax = lang.and_then(|l| Syntax::new(l, &doc.rope().to_string()));
        self.trees.insert(id, (lang, syntax));
        id
    }

    fn visible(&self, state: &State) -> Vec<(std::ops::Range<usize>, &'static str)> {
        let Some((_, Some(syntax))) = state.doc.syntax_id.and_then(|id| self.trees.get(&id)) else {
            return Vec::new();
        };
        let rope = state.doc.rope();
        let first = state.scroll_line.min(rope.len_lines());
        let last = (state.scroll_line + usize::from(editor_size(state).1)).min(rope.len_lines());
        let bytes = rope.line_to_byte(first)..rope.line_to_byte(last);
        // shortcut: `highlights` veut un `&str` entier, copie O(n) par rendu ; TextProvider sur la rope si ça se voit.
        syntax
            .highlights(&rope.to_string(), bytes)
            .into_iter()
            .map(|(r, h)| (r, h.name()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Event, Key, KeyInput, Modifiers, update};

    fn etat(path: &str, text: &str) -> State {
        let mut s = State {
            width: 80,
            height: 30,
            ..State::default()
        };
        ouvrir(&mut s, path, text);
        s
    }

    fn ouvrir(s: &mut State, path: &str, text: &str) {
        update(
            s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        );
    }

    fn ctrl(s: &mut State, key: Key) {
        let mods = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        update(s, Event::Key(KeyInput { key, mods }));
    }

    /// Catégorie de l'octet `at` du document actif.
    fn categorie(s: &State, at: usize) -> Option<&'static str> {
        s.highlights
            .iter()
            .find(|(r, _)| r.contains(&at))
            .map(|(_, h)| *h)
    }

    /// Coloration calculée de zéro sur le même texte, pour comparer à l'incrémental.
    fn complet(s: &State) -> Vec<(std::ops::Range<usize>, &'static str)> {
        let mut neuf = etat(
            &s.doc.path.as_ref().expect("chemin").to_string_lossy(),
            &s.doc.rope().to_string(),
        );
        neuf.scroll_line = s.scroll_line;
        Highlighter::default().sync(&mut neuf);
        neuf.highlights
    }

    #[test]
    fn mot_cle_au_bon_octet() {
        let mut s = etat("/x/a.rs", "let x = 1;\nfn f() {}\n");
        Highlighter::default().sync(&mut s);
        assert_eq!(categorie(&s, 0), Some("keyword"));
        assert_eq!(categorie(&s, 11), Some("keyword"));
    }

    #[test]
    fn edition_incrementale_egale_complet() {
        let mut s = etat("/x/a.rs", "fn a() {}\n");
        let mut h = Highlighter::default();
        h.sync(&mut s);
        s.doc.insert_str("let y = \"t\";\n");
        s.doc.insert_str("// c\n");
        h.sync(&mut s);
        assert_eq!(categorie(&s, 0), Some("keyword"));
        assert_eq!(s.highlights, complet(&s));
        assert!(s.doc.take_edits().is_empty());
    }

    #[test]
    fn annuler_retablir() {
        let mut s = etat("/x/a.rs", "fn a() {}\n");
        let mut h = Highlighter::default();
        h.sync(&mut s);
        s.doc.insert_str("/* c */ ");
        h.sync(&mut s);
        assert_eq!(categorie(&s, 0), Some("comment"));
        assert!(s.doc.undo());
        h.sync(&mut s);
        assert_eq!(categorie(&s, 0), Some("keyword"));
        assert_eq!(s.highlights, complet(&s));
        assert!(s.doc.redo());
        h.sync(&mut s);
        assert_eq!(categorie(&s, 0), Some("comment"));
        assert_eq!(s.highlights, complet(&s));
    }

    #[test]
    fn onglets_et_fermeture() {
        let mut s = etat("/x/a.rs", "fn a() {}\n");
        ouvrir(&mut s, "/x/b.txt", "fn b() {}\n");
        let mut h = Highlighter::default();
        h.sync(&mut s);
        assert_eq!(h.trees.len(), 2);
        assert!(s.highlights.is_empty(), "b.txt sans langage");
        // Édition d'un onglet inactif : appliquée aussi, pas accumulée.
        s.tabs[0].doc.insert_str("// c\n");
        h.sync(&mut s);
        assert!(s.tabs[0].doc.take_edits().is_empty());
        ctrl(&mut s, Key::Tab);
        h.sync(&mut s);
        assert_eq!(s.doc.display_name(), "a.rs");
        assert_eq!(categorie(&s, 0), Some("comment"));
        assert_eq!(s.highlights, complet(&s));
        // a.rs modifié : Ctrl+W deux fois.
        ctrl(&mut s, Key::Char('w'));
        ctrl(&mut s, Key::Char('w'));
        h.sync(&mut s);
        assert_eq!(h.trees.len(), 1);
        assert!(h.trees.contains_key(&s.doc.syntax_id.expect("id")));
    }

    #[test]
    fn renommage_active_la_coloration() {
        let mut s = etat("/x/a.txt", "fn a() {}\n");
        let mut h = Highlighter::default();
        h.sync(&mut s);
        assert!(s.highlights.is_empty());
        s.doc.path = Some("/x/a.rs".into());
        h.sync(&mut s);
        assert_eq!(categorie(&s, 0), Some("keyword"));
        s.doc.path = Some("/x/a.md".into());
        h.sync(&mut s);
        assert_ne!(categorie(&s, 0), Some("keyword"));
    }

    #[test]
    fn lignes_visibles_seulement() {
        let text = "fn a() {}\n".repeat(100);
        let mut s = etat("/x/a.rs", &text);
        s.scroll_line = 50;
        Highlighter::default().sync(&mut s);
        let (debut, fin) = (50 * 10, (50 + usize::from(editor_size(&s).1)) * 10);
        assert!(!s.highlights.is_empty());
        assert!(
            s.highlights
                .iter()
                .all(|(r, _)| r.start >= debut && r.end <= fin)
        );
        // Défilement au-delà de la fin : vide, sans panic.
        s.scroll_line = 1_000;
        Highlighter::default().sync(&mut s);
        assert!(s.highlights.is_empty());
    }

    /// `cargo test -p ide-app --release -- --ignored --nocapture perf`
    #[test]
    #[ignore]
    fn perf_frappe_10_000_lignes() {
        let mut text = String::new();
        for i in 0..2_000 {
            text.push_str(&format!(
                "fn f{i}(a: u32) -> u32 {{\n    let x = compute(\"texte\", 42); // c\n    x + a\n}}\n\n"
            ));
        }
        let mut s = etat("/x/a.rs", &text);
        let mut h = Highlighter::default();
        h.sync(&mut s);
        s.scroll_line = 5_000;
        let at = s.doc.pos_at(5_001, 9);
        s.doc.set_cursor(at, false);
        let t = std::time::Instant::now();
        s.doc.insert_char('y');
        h.sync(&mut s);
        let dt = t.elapsed();
        println!("frappe + sync sur 10 000 lignes : {dt:?}");
        assert!(!s.highlights.is_empty());
        assert!(dt.as_millis() < 16, "{dt:?}");
    }
}
