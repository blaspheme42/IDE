//! Repli de blocs par indentation : une ligne ouvre un bloc si la ligne non vide suivante est
//! plus indentée ; le bloc va jusqu'à la dernière ligne plus indentée (lignes vides de fin exclues).
// shortcut: indentation seule, pas de `foldingRange` LSP ; une accolade fermante reste visible sous l'en-tête.

use std::ops::Range;

use super::Document;

impl Document {
    /// Largeur d'indentation de `line`, `None` si elle est vide ou blanche.
    fn indent_of(&self, line: usize) -> Option<usize> {
        let tab = self.tab_width().max(1);
        let mut w = 0;
        for c in self.rope.line(line).chars() {
            match c {
                ' ' => w += 1,
                '\t' => w += tab - w % tab,
                '\n' | '\r' => return None,
                _ => return Some(w),
            }
        }
        None
    }

    /// Lignes masquées quand `line` est repliée, `None` si elle n'ouvre pas de bloc.
    pub fn fold_range(&self, line: usize) -> Option<Range<usize>> {
        let base = self.indent_of(line)?;
        let mut end = line + 1;
        for i in line + 1..self.rope.len_lines() {
            match self.indent_of(i) {
                Some(n) if n <= base => break,
                Some(_) => end = i + 1,
                None => {}
            }
        }
        (end > line + 1).then(|| line + 1..end)
    }

    /// `line` est un en-tête replié.
    pub fn is_folded(&self, line: usize) -> bool {
        self.folds.contains(&line) && self.fold_range(line).is_some()
    }

    /// Plages de lignes masquées, triées et disjointes.
    pub fn hidden(&self) -> Vec<Range<usize>> {
        let mut v: Vec<_> = self
            .folds
            .iter()
            .filter_map(|&l| self.fold_range(l))
            .collect();
        v.sort_by_key(|r| r.start);
        let mut out: Vec<Range<usize>> = Vec::new();
        for r in v {
            match out.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => out.push(r),
            }
        }
        out
    }

    /// `line` si visible, sinon la ligne visible qui suit (`forward`) ou l'en-tête du repli.
    pub(super) fn skip_hidden(&self, hidden: &[Range<usize>], line: usize, forward: bool) -> usize {
        match hidden.iter().find(|r| r.contains(&line)) {
            Some(r) if forward && r.end < self.rope.len_lines() => r.end,
            Some(r) => r.start - 1,
            None => line,
        }
    }

    /// Ligne visible atteinte en avançant (ou reculant) de `n` lignes visibles depuis `line`.
    pub fn step_visible(&self, line: usize, n: usize, forward: bool) -> usize {
        let hidden = self.hidden();
        let last = self.rope.len_lines() - 1;
        let mut line = self.skip_hidden(&hidden, line.min(last), false);
        for _ in 0..n {
            let next = match forward {
                true if line < last => self.skip_hidden(&hidden, line + 1, true),
                false if line > 0 => self.skip_hidden(&hidden, line - 1, false),
                _ => break,
            };
            if next == line {
                break;
            }
            line = next;
        }
        line
    }

    /// Jusqu'à `n` lignes visibles à partir de `from` (ramenée à son en-tête si masquée).
    pub fn visible_lines(&self, from: usize, n: usize) -> Vec<usize> {
        let hidden = self.hidden();
        let last = self.rope.len_lines() - 1;
        let mut line = self.skip_hidden(&hidden, from.min(last), false);
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            out.push(line);
            let next = self.skip_hidden(&hidden, line + 1, true);
            if line == last || next <= line {
                break;
            }
            line = next;
        }
        out
    }

    /// Replie ou déplie l'en-tête `line` ; `false` si elle n'ouvre pas de bloc.
    pub fn toggle_fold(&mut self, line: usize) -> bool {
        if let Some(i) = self.folds.iter().position(|&h| h == line) {
            self.folds.swap_remove(i);
            return true;
        }
        if self.fold_range(line).is_none() {
            return false;
        }
        self.folds.push(line);
        self.cursor_out_of_folds();
        true
    }

    /// Replie le bloc le plus intérieur non replié qui contient le curseur (ou qu'il ouvre).
    pub fn fold_at_cursor(&mut self) -> bool {
        let line = self.cursor_line_col().0;
        if !self.is_folded(line) && self.fold_range(line).is_some() {
            return self.toggle_fold(line);
        }
        // En-têtes englobants : en remontant, chaque ligne moins indentée que la précédente.
        let mut target = (line..self.rope.len_lines())
            .find_map(|i| self.indent_of(i))
            .unwrap_or(0);
        for i in (0..line).rev() {
            match self.indent_of(i) {
                Some(n) if n < target => {
                    if !self.is_folded(i) {
                        return self.toggle_fold(i);
                    }
                    target = n;
                }
                _ => {}
            }
        }
        false
    }

    /// Déplie le repli dont la ligne du curseur est l'en-tête.
    pub fn unfold_at_cursor(&mut self) -> bool {
        let line = self.cursor_line_col().0;
        self.is_folded(line) && self.toggle_fold(line)
    }

    pub fn fold_all(&mut self) {
        self.folds = (0..self.rope.len_lines())
            .filter(|&l| self.opens_block(l))
            .collect();
        self.cursor_out_of_folds();
    }

    pub fn unfold_all(&mut self) {
        self.folds.clear();
    }

    /// Comme `fold_range(line).is_some()`, sans parcourir tout le bloc.
    pub fn opens_block(&self, line: usize) -> bool {
        let Some(base) = self.indent_of(line) else {
            return false;
        };
        (line + 1..self.rope.len_lines())
            .find_map(|i| self.indent_of(i))
            .is_some_and(|n| n > base)
    }

    /// Déplie ce qui masque `line` (curseur amené là par une recherche, F12, annuler…).
    pub fn reveal(&mut self, line: usize) {
        let folds = std::mem::take(&mut self.folds);
        self.folds = folds
            .into_iter()
            .filter(|&h| !self.fold_range(h).is_some_and(|r| r.contains(&line)))
            .collect();
    }

    /// `pos` dans un bloc replié : début de la ligne visible suivante (`forward`), sinon fin de l'en-tête.
    pub(super) fn past_folds(&self, pos: usize, forward: bool) -> usize {
        let line = self.rope.char_to_line(pos);
        let to = self.step_visible(line, 0, false);
        if to == line {
            return pos;
        }
        let after = self.step_visible(to, 1, true);
        if forward && after != to {
            self.rope.line_to_char(after)
        } else {
            self.rope.line_to_char(to) + self.line_len(to)
        }
    }

    /// Curseur masqué par un repli : ramené en fin de son en-tête, sélection abandonnée.
    fn cursor_out_of_folds(&mut self) {
        let line = self.cursor_line_col().0;
        let header = self.step_visible(line, 0, false);
        if header != line {
            let end = self.rope.line_to_char(header) + self.line_len(header);
            self.set_cursor(end, false);
        }
    }

    /// Après le remplacement des lignes `start..=old_end` par `start..=new_end` : en-têtes décalés,
    /// ceux des lignes remplacées (sauf la première) oubliés.
    pub(super) fn shift_folds(&mut self, start: usize, old_end: usize, new_end: usize) {
        self.folds.retain(|&h| h <= start || h > old_end);
        for h in &mut self.folds {
            if *h > old_end {
                *h = (*h + new_end) - old_end;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Document;

    const SRC: &str = "fn a() {\n    if x {\n        y();\n    }\n\n    z();\n}\n\nfn b() {}\n";

    fn doc() -> Document {
        Document::from_text(None, SRC)
    }

    #[test]
    fn blocs_par_indentation() {
        let d = doc();
        assert_eq!(d.fold_range(0), Some(1..6));
        assert_eq!(d.fold_range(1), Some(2..3));
        assert_eq!(d.fold_range(2), None);
        assert_eq!(d.fold_range(4), None);
        assert_eq!(d.fold_range(8), None);
    }

    #[test]
    fn lignes_visibles_et_deplacements() {
        let mut d = doc();
        assert!(d.toggle_fold(1));
        assert_eq!(d.visible_lines(0, 4), vec![0, 1, 3, 4]);
        d.set_cursor(d.pos_at(1, 0), false);
        d.move_down(1, false);
        assert_eq!(d.cursor_line_col().0, 3);
        d.move_up(1, false);
        assert_eq!(d.cursor_line_col().0, 1);
        // Gauche/droite sautent le bloc replié.
        d.set_cursor(d.pos_at(3, 0), false);
        d.move_left(false);
        assert_eq!(d.cursor_line_col(), (1, 10));
        d.move_right(false);
        assert_eq!(d.cursor_line_col(), (3, 0));
        // Repli englobant : le curseur masqué remonte sur l'en-tête.
        d.set_cursor(d.pos_at(5, 4), false);
        assert!(d.fold_at_cursor());
        assert_eq!(d.cursor_line_col(), (0, 8));
        assert_eq!(d.visible_lines(0, 3), vec![0, 6, 7]);
        assert_eq!(d.visible_lines(3, 1), vec![0]);
        assert_eq!(d.step_visible(0, 2, true), 7);
        d.reveal(2);
        assert!(d.hidden().is_empty());
    }

    #[test]
    fn edition_decale_les_replis() {
        let mut d = doc();
        d.toggle_fold(1);
        d.edit(0..0, "// a\n// b\n");
        assert!(d.is_folded(3));
        assert!(d.undo());
        assert!(d.is_folded(1));
        // Ligne d'en-tête supprimée avec la précédente : repli oublié.
        let r = d.rope().line_to_char(0)..d.rope().line_to_char(2);
        d.edit(r, "");
        assert!(d.hidden().is_empty());
        let mut d = doc();
        d.fold_all();
        assert_eq!(d.visible_lines(0, 9), vec![0, 6, 7, 8, 9]);
        d.unfold_all();
        assert_eq!(d.visible_lines(0, 3), vec![0, 1, 2]);
    }
}
