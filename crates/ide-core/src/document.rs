//! Document texte : rope, curseur, fins de ligne. Positions en index de `char` dans la rope.

use std::path::PathBuf;

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Largeur d'une tabulation (tab stops tous les 4).
const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

/// Le texte est stocké en interne avec des `\n` seuls ; `line_ending` est réappliqué à la sauvegarde.
#[derive(Debug, Default)]
pub struct Document {
    rope: Rope,
    pub path: Option<PathBuf>,
    pub line_ending: LineEnding,
    /// Index de `char` du curseur dans la rope.
    cursor: usize,
    /// Colonne d'affichage voulue pour les déplacements verticaux.
    preferred_col: Option<usize>,
    /// Incrémentée à chaque modification ; comparée à `saved_version` pour savoir si c'est modifié.
    version: u64,
    saved_version: u64,
}

/// Bornes de graphèmes de `s`, en index de `char`, de 0 à `s.chars().count()` inclus.
fn grapheme_bounds(s: &str) -> Vec<usize> {
    let mut bounds = vec![0];
    let mut chars = 0;
    for g in s.graphemes(true) {
        chars += g.chars().count();
        bounds.push(chars);
    }
    bounds
}

/// Colonne d'affichage atteinte après le graphème `g` placé à la colonne `col`.
fn advance(col: usize, g: &str) -> usize {
    if g == "\t" {
        (col / TAB_WIDTH + 1) * TAB_WIDTH
    } else {
        col + g.width()
    }
}

impl Document {
    /// Détecte LF/CRLF (majoritaire sur le texte), normalise en `\n`, curseur au début.
    pub fn from_text(path: Option<PathBuf>, text: &str) -> Self {
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        let (line_ending, rope) = if crlf > lf {
            (
                LineEnding::CrLf,
                Rope::from_str(&text.replace("\r\n", "\n")),
            )
        } else {
            // Fichier LF : on garde tel quel, d'éventuels `\r\n` isolés restent intacts.
            (LineEnding::Lf, Rope::from_str(text))
        };
        Self {
            rope,
            path,
            line_ending,
            ..Self::default()
        }
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// (ligne, colonne d'affichage) du curseur, 0-based ; la colonne compte la largeur Unicode.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let line = self.rope.char_to_line(self.cursor);
        let col = self.cursor - self.rope.line_to_char(line);
        (line, self.display_col(line, col))
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }

    /// Appelé sur `Event::Saved` : seule la version effectivement écrite est marquée propre.
    pub fn mark_saved(&mut self, version: u64) {
        self.saved_version = version;
    }

    /// Texte à écrire sur disque, avec les fins de ligne d'origine.
    pub fn to_saved_text(&self) -> String {
        let text = self.rope.to_string();
        match self.line_ending {
            LineEnding::Lf => text,
            LineEnding::CrLf => text.replace('\n', "\r\n"),
        }
    }

    /// Nom affiché : nom du fichier, ou "[sans nom]".
    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "[sans nom]".into(), |n| n.to_string_lossy().into_owned())
    }

    /// Texte de la ligne sans sa fin de ligne.
    pub fn line_text(&self, line: usize) -> String {
        self.rope
            .line(line)
            .slice(..self.line_len(line))
            .to_string()
    }

    /// Colonne d'affichage de l'index de `char` `char_col` dans la ligne `line`.
    pub fn display_col(&self, line: usize, char_col: usize) -> usize {
        let text = self.line_text(line);
        let mut col = 0;
        let mut chars = 0;
        for g in text.graphemes(true) {
            chars += g.chars().count();
            if chars > char_col {
                break;
            }
            col = advance(col, g);
        }
        col
    }

    /// Nombre de `char` de la ligne hors fin de ligne (selon le découpage de ropey).
    fn line_len(&self, line: usize) -> usize {
        let full = self.rope.line(line).len_chars();
        let next = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            return full;
        };
        let end = next - 1;
        // `\r\n` compte pour une seule fin de ligne de 2 caractères.
        let brk = if end > 0 && self.rope.char(end) == '\n' && self.rope.char(end - 1) == '\r' {
            2
        } else {
            1
        };
        full - brk
    }

    /// Index de `char` (relatif à la ligne) le plus à droite dont la colonne d'affichage ≤ `target`.
    fn char_col_for_display(&self, line: usize, target: usize) -> usize {
        let mut col = 0;
        let mut chars = 0;
        for g in self.line_text(line).graphemes(true) {
            let next = advance(col, g);
            if next > target {
                break;
            }
            col = next;
            chars += g.chars().count();
        }
        chars
    }

    fn prev_boundary(&self, pos: usize) -> usize {
        let line = self.rope.char_to_line(pos);
        let start = self.rope.line_to_char(line);
        if pos == start {
            return if line == 0 {
                0
            } else {
                start - (self.rope.line(line - 1).len_chars() - self.line_len(line - 1))
            };
        }
        let rel = pos - start;
        let b = grapheme_bounds(&self.line_text(line));
        start + b.iter().rev().find(|&&x| x < rel).copied().unwrap_or(0)
    }

    fn next_boundary(&self, pos: usize) -> usize {
        let line = self.rope.char_to_line(pos);
        let start = self.rope.line_to_char(line);
        let rel = pos - start;
        if rel >= self.line_len(line) {
            return if line + 1 < self.rope.len_lines() {
                self.rope.line_to_char(line + 1)
            } else {
                pos
            };
        }
        let b = grapheme_bounds(&self.line_text(line));
        start + b.iter().find(|&&x| x > rel).copied().unwrap_or(rel)
    }

    fn edited(&mut self) {
        self.version += 1;
        self.preferred_col = None;
    }

    pub fn insert_char(&mut self, c: char) {
        self.rope.insert_char(self.cursor, c);
        self.cursor += 1;
        self.edited();
    }

    /// Insère `\n` (pas d'auto-indentation en J1a).
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Supprime le graphème avant le curseur.
    pub fn backspace(&mut self) {
        let p = self.prev_boundary(self.cursor);
        if p < self.cursor {
            self.rope.remove(p..self.cursor);
            self.cursor = p;
            self.edited();
        }
    }

    /// Supprime le graphème après le curseur.
    pub fn delete(&mut self) {
        let n = self.next_boundary(self.cursor);
        if n > self.cursor {
            self.rope.remove(self.cursor..n);
            self.edited();
        }
    }

    /// Déplacements par graphème (gauche/droite) et par ligne avec colonne d'affichage préférée (haut/bas).
    pub fn move_left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
        self.preferred_col = None;
    }
    pub fn move_right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
        self.preferred_col = None;
    }
    pub fn move_up(&mut self, lines: usize) {
        let line = self.rope.char_to_line(self.cursor);
        self.move_to_line(line.saturating_sub(lines));
    }
    pub fn move_down(&mut self, lines: usize) {
        let line = self.rope.char_to_line(self.cursor);
        let last = self.rope.len_lines() - 1;
        self.move_to_line(line.saturating_add(lines).min(last));
    }
    fn move_to_line(&mut self, line: usize) {
        let target = self
            .preferred_col
            .unwrap_or_else(|| self.cursor_line_col().1);
        self.cursor = self.rope.line_to_char(line) + self.char_col_for_display(line, target);
        self.preferred_col = Some(target);
    }
    pub fn move_line_start(&mut self) {
        self.cursor = self.rope.line_to_char(self.rope.char_to_line(self.cursor));
        self.preferred_col = None;
    }
    pub fn move_line_end(&mut self) {
        let line = self.rope.char_to_line(self.cursor);
        self.cursor = self.rope.line_to_char(line) + self.line_len(line);
        self.preferred_col = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn doc(text: &str) -> Document {
        Document::from_text(Some("/tmp/f.txt".into()), text)
    }

    #[test]
    fn crlf_aller_retour_octet_pour_octet() {
        for text in [
            "a\r\nb\r\n",
            "a\r\nb",
            "a\nb\n",
            "a\nb",
            "",
            "\r\n",
            "x\r\r\n",
        ] {
            let d = doc(text);
            assert_eq!(d.to_saved_text(), text, "{text:?}");
        }
        assert_eq!(doc("a\r\nb\r\n").rope().to_string(), "a\nb\n");
        assert_eq!(doc("a\r\nb\r\nc\n").line_ending, LineEnding::CrLf);
        assert_eq!(doc("a\r\nb\nc\n").line_ending, LineEnding::Lf);
    }

    #[test]
    fn crlf_edition_garde_crlf() {
        let mut d = doc("ab\r\ncd");
        d.move_line_end();
        d.insert_newline();
        d.insert_char('x');
        assert_eq!(d.to_saved_text(), "ab\r\nx\r\ncd");
        // Backspace en début de ligne supprime toute la fin de ligne.
        d.move_line_start();
        d.backspace();
        assert_eq!(d.to_saved_text(), "abx\r\ncd");
    }

    #[test]
    fn graphemes_un_seul_pas() {
        // e + accent combinant, emoji famille (ZWJ), drapeau.
        let text = "e\u{301}👨\u{200d}👩\u{200d}👧🇫🇷";
        let mut d = doc(text);
        d.move_right();
        assert_eq!(d.cursor(), 2);
        d.move_right();
        assert_eq!(d.cursor(), 7);
        d.move_right();
        assert_eq!(d.cursor(), 9);
        d.move_right();
        assert_eq!(d.cursor(), 9);
        d.backspace();
        assert_eq!(d.rope().to_string(), "e\u{301}👨\u{200d}👩\u{200d}👧");
        d.move_line_start();
        d.delete();
        assert_eq!(d.rope().to_string(), "👨\u{200d}👩\u{200d}👧");
        d.move_left();
        assert_eq!(d.cursor(), 0);
    }

    #[test]
    fn largeur_affichage() {
        let d = doc("中a\tb👍");
        assert_eq!(d.display_col(0, 1), 2);
        assert_eq!(d.display_col(0, 2), 3);
        assert_eq!(d.display_col(0, 3), 4); // tab de la col 3 à 4
        assert_eq!(d.display_col(0, 5), 7);
        let d = doc("\tx");
        assert_eq!(d.display_col(0, 1), 4);
    }

    #[test]
    fn haut_bas_colonne_preferee() {
        let mut d = doc("abcdef\nab\n中文字\nabcdef");
        d.move_line_end(); // col 6
        d.move_down(1);
        assert_eq!(d.cursor_line_col(), (1, 2));
        d.move_down(1);
        assert_eq!(d.cursor_line_col(), (2, 6));
        d.move_down(1);
        assert_eq!(d.cursor_line_col(), (3, 6));
        d.move_up(1);
        d.move_left();
        d.move_up(1); // col 4 : préférée oubliée après move_left
        assert_eq!(d.cursor_line_col(), (1, 2));
        d.move_down(1);
        assert_eq!(d.cursor_line_col(), (2, 4));
        // Colonne au milieu d'un idéogramme : on reste avant.
        let mut d = doc("abc\n中文");
        d.move_right();
        d.move_down(1);
        assert_eq!(d.cursor_line_col(), (1, 0));
        d.move_down(5);
        d.move_up(9);
        assert_eq!(d.cursor_line_col(), (0, 1));
    }

    #[test]
    fn dirty_et_mark_saved() {
        let mut d = doc("a");
        assert!(!d.is_dirty());
        d.insert_char('b');
        let v = d.version();
        d.insert_char('c');
        d.mark_saved(v);
        assert!(d.is_dirty());
        d.mark_saved(d.version());
        assert!(!d.is_dirty());
        d.move_right();
        d.move_down(1);
        assert!(!d.is_dirty());
    }

    #[test]
    fn line_text_et_nom() {
        let d = doc("ab\r\ncd\n");
        assert_eq!(d.line_text(0), "ab");
        assert_eq!(d.line_text(2), "");
        assert_eq!(d.display_name(), "f.txt");
        assert_eq!(Document::default().display_name(), "[sans nom]");
    }

    #[derive(Debug, Clone)]
    enum Op {
        Ins(char),
        Newline,
        Back,
        Del,
        Left,
        Right,
        Up(usize),
        Down(usize),
        Home,
        End,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            prop::sample::select(vec!['a', 'é', '\u{301}', '👍', '\u{200d}', '中', '\t', ' '])
                .prop_map(Op::Ins),
            Just(Op::Newline),
            Just(Op::Back),
            Just(Op::Del),
            Just(Op::Left),
            Just(Op::Right),
            (0usize..5).prop_map(Op::Up),
            (0usize..5).prop_map(Op::Down),
            Just(Op::Home),
            Just(Op::End),
        ]
    }

    proptest! {
        #[test]
        fn suite_aleatoire(text in "[a-zé中\u{301}\n]{0,30}", crlf: bool, ops in prop::collection::vec(op(), 0..60)) {
            let text = if crlf { text.replace('\n', "\r\n") } else { text };
            let mut d = doc(&text);
            for o in ops {
                match o {
                    Op::Ins(c) => d.insert_char(c),
                    Op::Newline => d.insert_newline(),
                    Op::Back => d.backspace(),
                    Op::Del => d.delete(),
                    Op::Left => d.move_left(),
                    Op::Right => d.move_right(),
                    Op::Up(n) => d.move_up(n),
                    Op::Down(n) => d.move_down(n),
                    Op::Home => d.move_line_start(),
                    Op::End => d.move_line_end(),
                }
                prop_assert!(d.cursor() <= d.rope().len_chars());
                let _ = d.cursor_line_col();
            }
            let again = Document::from_text(None, &d.to_saved_text());
            prop_assert_eq!(again.rope().to_string(), d.rope().to_string());
        }
    }
}
