//! Retour à la ligne visuel : une ligne logique se découpe en rangées d'écran. `wrap: None` = 1 ligne, 1 rangée.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use super::{Document, advance, char_of_col, col_of_char, grapheme_bounds};

/// Rangée d'écran : `sub`-ième rangée de `line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualRow {
    pub line: usize,
    pub sub: usize,
    /// Plage relative à la ligne ; la dernière rangée inclut le `\n`.
    pub chars: Range<usize>,
}

impl Document {
    /// Débuts de rangée de `line` en index de `char`, `[0]` au minimum.
    // shortcut: la ligne entière est rebalayée à chaque appel, coûteux sur une ligne très longue, cache `(line, version, width)` si ça se voit.
    fn wrap_starts(&self, line: usize, width: usize) -> Vec<usize> {
        let tab = self.tab_width();
        let text = self.line_text(line);
        let bounds = grapheme_bounds(&text);
        let graphemes: Vec<&str> = text.graphemes(true).collect();
        let mut starts = vec![0];
        let mut first = 0;
        loop {
            let (mut col, mut i, mut inked, mut after_blank) = (0, first, false, None);
            while i < graphemes.len() {
                let next = advance(col, graphemes[i], tab);
                // `i > first` : au moins un graphème par rangée, même plus large qu'elle.
                if next > width && i > first {
                    break;
                }
                if !matches!(graphemes[i], " " | "\t") {
                    inked = true;
                } else if inked {
                    after_blank = Some(i + 1);
                }
                col = next;
                i += 1;
            }
            if i == graphemes.len() {
                return starts;
            }
            first = after_blank.unwrap_or(i);
            starts.push(bounds[first]);
        }
    }

    fn row_starts(&self, line: usize, wrap: Option<usize>) -> Vec<usize> {
        wrap.map_or_else(|| vec![0], |w| self.wrap_starts(line, w))
    }

    /// Texte de `line` de `start` à `end` exclu, ou à la fin de ligne hors `\n`.
    fn row_text(&self, line: usize, start: usize, end: Option<usize>) -> String {
        let end = end.unwrap_or_else(|| self.line_len(line));
        self.rope.line(line).slice(start..end).to_string()
    }

    /// Jusqu'à `n` rangées visibles à partir de `top` = (ligne, rangée), `sub` borné à la ligne.
    pub fn visual_rows(
        &self,
        top: (usize, usize),
        n: usize,
        wrap: Option<usize>,
    ) -> Vec<VisualRow> {
        let mut out = Vec::with_capacity(n);
        let lines = self.visible_lines(top.0, n);
        // `sub` visait une ligne masquée : son en-tête s'affiche depuis sa première rangée.
        let mut skip = if lines.first() == Some(&top.0) {
            top.1
        } else {
            0
        };
        for line in lines {
            let starts = self.row_starts(line, wrap);
            let len = self.rope.line(line).len_chars();
            for sub in std::mem::take(&mut skip).min(starts.len() - 1)..starts.len() {
                if out.len() == n {
                    return out;
                }
                let end = starts.get(sub + 1).copied().unwrap_or(len);
                let chars = starts[sub]..end;
                out.push(VisualRow { line, sub, chars });
            }
        }
        out
    }

    /// (ligne, rangée, colonne d'affichage dans la rangée) de `pos`, bornée au texte.
    pub fn row_col(&self, pos: usize, wrap: Option<usize>) -> (usize, usize, usize) {
        let pos = pos.min(self.rope.len_chars());
        let line = self.rope.char_to_line(pos);
        let rel = pos - self.rope.line_to_char(line);
        let starts = self.row_starts(line, wrap);
        let sub = starts.partition_point(|&s| s <= rel) - 1;
        let text = self.row_text(line, starts[sub], None);
        (
            line,
            sub,
            col_of_char(&text, rel - starts[sub], self.tab_width()),
        )
    }

    /// Rangée visible atteinte en avançant (ou reculant) de `n` rangées depuis `from`.
    pub fn step_rows(
        &self,
        from: (usize, usize),
        n: usize,
        forward: bool,
        wrap: Option<usize>,
    ) -> (usize, usize) {
        if forward {
            let rows = self.visual_rows(from, n + 1, wrap);
            return rows.last().map_or(from, |r| (r.line, r.sub));
        }
        let hidden = self.hidden();
        let mut line = self.skip_hidden(&hidden, from.0.min(self.rope.len_lines() - 1), false);
        let from_sub = if line == from.0 { from.1 } else { 0 };
        let mut sub = from_sub.min(self.row_starts(line, wrap).len() - 1);
        let mut left = n;
        while left > sub && line > 0 {
            left -= sub + 1;
            line = self.skip_hidden(&hidden, line - 1, false);
            sub = self.row_starts(line, wrap).len() - 1;
        }
        (line, sub.saturating_sub(left))
    }

    /// Index de `char` à la colonne `col` de la rangée, sans jamais en sortir : la fin d'une rangée non
    /// finale est le début de la suivante, on s'arrête donc avant son dernier graphème.
    pub fn pos_in_row(&self, line: usize, sub: usize, col: usize, wrap: Option<usize>) -> usize {
        let line = line.min(self.rope.len_lines() - 1);
        let starts = self.row_starts(line, wrap);
        let sub = sub.min(starts.len() - 1);
        let begin = self.rope.line_to_char(line);
        let end = starts
            .get(sub + 1)
            .map(|&next| self.prev_boundary(begin + next) - begin);
        let text = self.row_text(line, starts[sub], end);
        begin + starts[sub] + char_of_col(&text, col, self.tab_width())
    }

    /// Haut/bas par rangées visibles : un bloc replié compte pour une ligne, `preferred_col` est relative à la rangée.
    pub fn move_rows(&mut self, n: usize, forward: bool, wrap: Option<usize>, extend: bool) {
        let (line, sub, col) = self.row_col(self.cursor, wrap);
        let target = self.preferred_col.unwrap_or(col);
        let (line, sub) = self.step_rows((line, sub), n, forward, wrap);
        self.set_cursor(self.pos_in_row(line, sub, target, wrap), extend);
        self.preferred_col = Some(target);
    }

    /// Début de la rangée ; déjà dessus, début de la ligne logique.
    pub fn move_row_start(&mut self, wrap: Option<usize>, extend: bool) {
        let (line, sub, _) = self.row_col(self.cursor, wrap);
        let begin = self.rope.line_to_char(line);
        let start = begin + self.row_starts(line, wrap)[sub];
        let pos = if self.cursor == start { begin } else { start };
        self.set_cursor(pos, extend);
    }

    /// Fin de la rangée (voir `pos_in_row`) ; déjà dessus, fin de la ligne logique.
    pub fn move_row_end(&mut self, wrap: Option<usize>, extend: bool) {
        let (line, sub, _) = self.row_col(self.cursor, wrap);
        let end = self.pos_in_row(line, sub, usize::MAX, wrap);
        if self.cursor == end {
            self.move_line_end(extend);
        } else {
            self.set_cursor(end, extend);
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn doc(text: &str) -> Document {
        Document::from_text(None, text)
    }

    /// Texte de chaque rangée de la première ligne, `\n` compris.
    fn rangees(text: &str, width: usize) -> Vec<String> {
        let d = doc(text);
        let rows = d.visual_rows((0, 0), usize::from(u16::MAX), Some(width));
        rows.iter()
            .filter(|r| r.line == 0)
            .map(|r| d.rope.line(0).slice(r.chars.clone()).to_string())
            .collect()
    }

    #[test]
    fn coupe_apres_le_dernier_blanc() {
        assert_eq!(rangees("foo bar baz", 8), ["foo bar ", "baz"]);
        assert_eq!(rangees("aa   bb", 6), ["aa   ", "bb"]);
        assert_eq!(rangees("foo bar", 7), ["foo bar"]);
    }

    #[test]
    fn coupe_a_la_colonne_sans_blanc() {
        assert_eq!(rangees("abcdefgh", 3), ["abc", "def", "gh"]);
        // Le blanc qui déborde ouvre la rangée suivante.
        assert_eq!(rangees("abc d", 3), ["abc", " d"]);
    }

    #[test]
    fn indentation_seule_ne_coupe_pas() {
        assert_eq!(rangees("    abcdef", 6), ["    ab", "cdef"]);
        assert_eq!(rangees("        ", 3), ["   ", "   ", "  "]);
    }

    #[test]
    fn ligne_vide_une_rangee() {
        assert_eq!(rangees("", 4), [""]);
        assert_eq!(rangees("\nb", 4), ["\n"]);
    }

    #[test]
    fn la_derniere_rangee_porte_le_saut_de_ligne() {
        assert_eq!(rangees("abcd\nz", 3), ["abc", "d\n"]);
    }

    #[test]
    fn graphemes_larges_entiers() {
        assert_eq!(rangees("a中中", 4), ["a中", "中"]);
        let zwj = "👩\u{200d}💻";
        assert_eq!(rangees(&format!("a{zwj}b"), 2), ["a", zwj, "b"]);
        // Plus large que la rangée : il passe seul.
        assert_eq!(rangees("中a", 1), ["中", "a"]);
    }

    #[test]
    fn tabulation_relative_a_la_rangee() {
        let d = doc("abcde\tx");
        assert_eq!(d.tab_width(), 4);
        assert_eq!(d.wrap_starts(0, 5), [0, 5]);
        assert_eq!(d.row_col(6, Some(5)), (0, 1, 4));
        assert_eq!(d.pos_in_row(0, 1, 4, Some(5)), 6);
        assert_eq!(d.row_col(6, None), (0, 0, 8));
    }

    #[test]
    fn largeur_1_progresse() {
        assert_eq!(rangees("a b\t中", 1), ["a", " ", "b", "\t", "中"]);
    }

    #[test]
    fn sub_hors_bornes_borne() {
        let d = doc("abcdef\nz");
        let w = Some(3);
        assert_eq!(d.visual_rows((0, 99), 1, w)[0].sub, 1);
        assert_eq!(d.step_rows((0, 99), 0, true, w), (0, 1));
        assert_eq!(d.step_rows((0, 99), 0, false, w), (0, 1));
        assert_eq!(d.step_rows((0, 99), 1, false, w), (0, 0));
        assert_eq!(d.pos_in_row(0, 99, 1, w), 4);
        assert_eq!(d.pos_in_row(99, 99, 0, w), 7);
        assert_eq!(d.visual_rows((99, 99), 2, w).len(), 1);
    }

    #[test]
    fn sub_d_une_ligne_masquee_repart_de_l_en_tete() {
        let mut d = doc("if x {\n  abcdefgh\n}\n");
        d.toggle_fold(0);
        let rows = d.visual_rows((1, 2), 1, Some(3));
        assert_eq!((rows[0].line, rows[0].sub), (0, 0));
        assert_eq!(d.step_rows((1, 2), 0, false, Some(3)), (0, 0));
    }

    #[test]
    fn rangees_autour_d_un_repli() {
        let mut d = doc("abcdef\n  x\n  y\nabcdef");
        d.toggle_fold(0);
        let w = Some(3);
        let vue: Vec<_> = d
            .visual_rows((0, 1), 3, w)
            .iter()
            .map(|r| (r.line, r.sub))
            .collect();
        assert_eq!(vue, [(0, 1), (3, 0), (3, 1)]);
        assert_eq!(d.step_rows((3, 0), 1, false, w), (0, 1));
        assert_eq!(d.step_rows((3, 1), 9, false, w), (0, 0));
        assert_eq!(d.step_rows((0, 1), 9, true, w), (3, 1));
        // Ligne masquée : ramenée à son en-tête.
        assert_eq!(d.step_rows((1, 0), 0, false, w), (0, 0));
    }

    #[test]
    fn move_rows_garde_la_colonne_de_rangee() {
        let mut d = doc("abcdefgh\nxy");
        let w = Some(4);
        d.set_cursor(2, false);
        d.move_rows(1, true, w, false);
        assert_eq!(d.cursor(), 6);
        d.move_rows(1, true, w, false);
        assert_eq!(d.cursor(), 11); // `xy` trop courte : fin de ligne, colonne 2 tout juste
        d.move_rows(2, false, w, false);
        assert_eq!(d.cursor(), 2);
        d.move_rows(9, true, w, true);
        assert_eq!(d.selection(), Some(2..11));
    }

    #[test]
    fn move_rows_ne_deborde_pas_de_la_rangee_non_finale() {
        let mut d = doc("abcdefgh\nabcdefgh");
        let w = Some(4);
        d.set_cursor(9 + 8, false); // fin de la ligne 1, colonne 4 de sa dernière rangée
        d.move_rows(1, false, w, false);
        assert_eq!(d.row_col(d.cursor(), w), (1, 0, 3));
        d.move_rows(1, false, w, false);
        assert_eq!(d.row_col(d.cursor(), w), (0, 1, 4));
    }

    #[test]
    fn debut_de_rangee_puis_de_ligne() {
        let mut d = doc("abcdefgh\nz");
        let w = Some(4);
        d.set_cursor(6, false);
        d.move_row_start(w, false);
        assert_eq!(d.cursor(), 4);
        d.move_row_start(w, false);
        assert_eq!(d.cursor(), 0);
        d.move_row_start(w, false);
        assert_eq!(d.cursor(), 0);
        d.set_cursor(6, false);
        d.move_row_start(None, true);
        assert_eq!(d.selection(), Some(0..6));
    }

    #[test]
    fn fin_de_rangee_puis_de_ligne() {
        let mut d = doc("abcdefgh\nz");
        let w = Some(4);
        d.set_cursor(1, false);
        d.move_row_end(w, false);
        assert_eq!(d.cursor(), 3, "avant le dernier graphème de la rangée");
        d.move_row_end(w, false);
        assert_eq!(d.cursor(), 8);
        d.move_row_end(w, false);
        assert_eq!(d.cursor(), 8);
        d.set_cursor(1, false);
        d.move_row_end(None, true);
        assert_eq!(d.selection(), Some(1..8));
    }

    fn texte() -> impl Strategy<Value = String> {
        "[a-c é中\t\n]{0,40}"
    }

    proptest! {
        #[test]
        fn rangees_contigues_couvrant_la_ligne(text in texte(), width in 1usize..12) {
            let d = doc(&text);
            let rows = d.visual_rows((0, 0), text.chars().count() + 2, Some(width));
            for line in 0..d.rope.len_lines() {
                let mut at = 0;
                for (i, r) in rows.iter().filter(|r| r.line == line).enumerate() {
                    prop_assert_eq!(r.sub, i);
                    prop_assert_eq!(r.chars.start, at);
                    prop_assert!(r.chars.start < r.chars.end || r.sub == 0);
                    at = r.chars.end;
                }
                prop_assert_eq!(at, d.rope.line(line).len_chars());
            }
        }

        #[test]
        fn pos_in_row_inverse_row_col(text in texte(), width in 1usize..12, wrap: bool) {
            let d = doc(&text);
            let w = wrap.then_some(width);
            for p in 0..=d.rope.len_chars() {
                let (line, sub, col) = d.row_col(p, w);
                prop_assert_eq!(d.pos_in_row(line, sub, col, w), p);
            }
        }

        #[test]
        fn sans_wrap_equivaut_a_step_visible(
            text in "[a \n]{0,40}",
            fold in 0usize..8,
            from in 0usize..12,
            n in 0usize..12,
            forward: bool,
        ) {
            let mut d = doc(&text);
            d.toggle_fold(fold.min(d.rope.len_lines() - 1));
            prop_assert_eq!(
                d.step_rows((from, 0), n, forward, None),
                (d.step_visible(from, n, forward), 0)
            );
        }

        #[test]
        fn aller_retour_par_rangees(text in texte(), width in 1usize..12, n in 0usize..20) {
            let d = doc(&text);
            let w = Some(width);
            let bas = d.step_rows((0, 0), n, true, w);
            let rows = d.visual_rows((0, 0), n + 1, w);
            prop_assert_eq!(d.step_rows(bas, rows.len() - 1, false, w), (0, 0));
        }
    }
}
