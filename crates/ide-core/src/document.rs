//! Document texte : rope, curseur, fins de ligne. Positions en index de `char` dans la rope.

use std::ops::Range;
use std::path::PathBuf;

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::history::{Change, Entry, History, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

/// Style d'indentation (F-14), fourni par l'app depuis `.editorconfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub use_tabs: bool,
    /// Largeur d'un niveau d'indentation.
    pub width: usize,
    /// Largeur d'affichage d'une tabulation.
    pub tab_width: usize,
}

impl Default for Indent {
    fn default() -> Self {
        Self {
            use_tabs: false,
            width: 4,
            tab_width: 4,
        }
    }
}

impl Indent {
    /// Un niveau d'indentation.
    fn unit(self) -> String {
        if self.use_tabs {
            "\t".into()
        } else {
            " ".repeat(self.width.max(1))
        }
    }
}

/// Position après les éditions triées `edits` ; `right` : une insertion pile sur `pos` le pousse.
fn map_pos(pos: usize, edits: &[(Range<usize>, String)], right: bool) -> usize {
    let mut delta = 0isize;
    for (r, s) in edits {
        if r.end < pos || (r.end == pos && (r.start < r.end || right)) {
            delta += s.chars().count() as isize - r.len() as isize;
        } else if r.start < pos {
            return r.start.saturating_add_signed(delta);
        } else {
            break;
        }
    }
    pos.saturating_add_signed(delta)
}

/// Le texte est stocké en interne avec des `\n` seuls ; `line_ending` est réappliqué à la sauvegarde.
#[derive(Debug, Default)]
pub struct Document {
    rope: Rope,
    pub path: Option<PathBuf>,
    pub line_ending: LineEnding,
    pub indent: Indent,
    /// Index de `char` du curseur dans la rope.
    cursor: usize,
    /// Colonne d'affichage voulue pour les déplacements verticaux.
    preferred_col: Option<usize>,
    /// Autre extrémité de la sélection ; sélection vide si absente ou égale au curseur.
    anchor: Option<usize>,
    /// Identifie le contenu : nouvel id à chaque édition, id restauré par annuler/rétablir.
    version: u64,
    saved_version: u64,
    history: History,
    /// Éditions pas encore transmises à la coloration (`take_edits`).
    edits: Vec<TextEdit>,
    /// Identifiant attribué par l'app pour retrouver l'arbre syntaxique de ce document.
    pub syntax_id: Option<u64>,
    /// Empreintes des derniers contenus disque connus (lu, sauvegardé, refusé) : les retrouver sur le
    /// disque n'est pas un changement venu d'ailleurs (notre propre sauvegarde, même si l'on a retapé depuis).
    pub disk_hashes: Vec<u64>,
}

/// Une édition en octets et en (ligne, colonne-octet), avant/après, au format de `tree_sitter::InputEdit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start: (usize, usize),
    pub old_end: (usize, usize),
    pub new_end: (usize, usize),
}

fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
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

/// Colonne d'affichage atteinte après le graphème `g` placé à la colonne `col` (taquets tous les `tab`).
fn advance(col: usize, g: &str, tab: usize) -> usize {
    if g == "\t" {
        (col / tab + 1) * tab
    } else {
        col + g.width()
    }
}

impl Document {
    /// Détecte CRLF (toutes les fins de ligne) et normalise en `\n` ; sinon LF, texte gardé tel quel. Curseur au début.
    pub fn from_text(path: Option<PathBuf>, text: &str) -> Self {
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        // CRLF seulement si toutes les fins de ligne le sont : un fichier mixte reste octet pour octet.
        let (line_ending, rope) = if crlf > 0 && lf == 0 {
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
            disk_hashes: vec![text_hash(text)],
            ..Self::default()
        }
    }

    /// Retient `text` comme contenu disque connu (les 8 derniers : des sauvegardes en file d'attente).
    pub fn remember_disk(&mut self, text: &str) {
        let h = text_hash(text);
        self.disk_hashes.retain(|&x| x != h);
        self.disk_hashes.push(h);
        let extra = self.disk_hashes.len().saturating_sub(8);
        self.disk_hashes.drain(..extra);
    }

    pub fn knows_disk(&self, text: &str) -> bool {
        self.disk_hashes.contains(&text_hash(text))
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

    /// Ce document est passé par l'état `version` (courant, à annuler ou à rétablir).
    pub fn knows_version(&self, version: u64) -> bool {
        self.version == version
            || (self.history.undo.iter().chain(&self.history.redo))
                .any(|e| e.version_before == version || e.version_after == version)
    }

    /// Appelé sur `Event::Saved` : seule la version effectivement écrite est marquée propre.
    /// Ctrl+S : la version envoyée reste un état distinct de l'historique, même si l'on continue à taper
    /// avant la confirmation (sinon la frappe fusionnerait et `Saved` ne retrouverait plus ce document).
    pub fn seal_history(&mut self) {
        self.history.seal();
    }

    pub fn mark_saved(&mut self, version: u64) {
        self.saved_version = version;
        // La frappe suivante ouvre un nouveau groupe : on peut annuler jusqu'à l'état sauvegardé.
        self.history.seal();
    }

    /// Fichier disparu du disque : modifié jusqu'à la prochaine sauvegarde (aucune version n'atteint `u64::MAX`).
    pub fn mark_unsaved(&mut self) {
        self.saved_version = u64::MAX;
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

    /// Largeur d'affichage d'une tabulation : `width` (tab_width de .editorconfig) si indentation par tabs, sinon 4.
    pub fn tab_width(&self) -> usize {
        self.indent.tab_width.max(1)
    }

    /// Colonne d'affichage de l'index de `char` `char_col` dans la ligne `line`.
    pub fn display_col(&self, line: usize, char_col: usize) -> usize {
        let text = self.line_text(line);
        let tab = self.tab_width();
        let mut col = 0;
        let mut chars = 0;
        for g in text.graphemes(true) {
            chars += g.chars().count();
            if chars > char_col {
                break;
            }
            col = advance(col, g, tab);
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
        let tab = self.tab_width();
        let mut col = 0;
        let mut chars = 0;
        for g in self.line_text(line).graphemes(true) {
            let next = advance(col, g, tab);
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

    /// Remplace `range` (index de `char`, bornés au texte) par `text` ; curseur après le texte inséré.
    pub fn edit(&mut self, range: Range<usize>, text: &str) {
        self.apply(range, text, Kind::Other);
    }

    fn apply(&mut self, range: Range<usize>, text: &str, kind: Kind) {
        self.apply_many(vec![(range, text.into())], kind);
    }

    /// Plusieurs remplacements disjoints (positions d'avant édition) en une seule entrée d'historique,
    /// appliqués de la fin vers le début ; curseur après le premier texte inséré.
    pub fn edit_many(&mut self, edits: Vec<(Range<usize>, String)>) {
        self.apply_many(edits, Kind::Other);
    }

    /// `false` si rien n'a changé (aucune entrée d'historique).
    fn apply_many(&mut self, mut edits: Vec<(Range<usize>, String)>, kind: Kind) -> bool {
        let len = self.rope.len_chars();
        for (r, _) in &mut edits {
            r.start = r.start.min(len);
            r.end = r.end.clamp(r.start, len);
        }
        edits.retain(|(r, s)| !r.is_empty() || !s.is_empty());
        if edits.is_empty() {
            return false;
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
        let before = (self.cursor, self.anchor);
        let mut changes = Vec::with_capacity(edits.len());
        for (r, text) in edits {
            let removed = self.rope.slice(r.clone()).to_string();
            self.replace(r.clone(), &text);
            self.cursor = r.start + text.chars().count();
            changes.push(Change {
                pos: r.start,
                removed,
                inserted: text,
            });
        }
        self.anchor = None;
        self.preferred_col = None;
        let version_before = self.version;
        self.version = self.history.next_id();
        self.history.record(Entry {
            changes,
            kind,
            before,
            after: (self.cursor, None),
            version_before,
            version_after: self.version,
        });
        true
    }

    /// Place curseur et ancre après une édition ; c'est aussi l'état restauré par « rétablir ».
    fn place_after_edit(&mut self, cursor: usize, anchor: Option<usize>) {
        (self.cursor, self.anchor) = (cursor, anchor);
        if let Some(e) = self.history.undo.last_mut() {
            e.after = (cursor, anchor);
        }
    }

    /// Début de ligne et texte de la ligne avant `pos`.
    fn line_before(&self, pos: usize) -> (usize, String) {
        let start = self.rope.line_to_char(self.rope.char_to_line(pos));
        (start, self.rope.slice(start..pos).to_string())
    }

    /// Lignes touchées par la sélection (ou la ligne du curseur) ; une fin de sélection
    /// en colonne 0 n'inclut pas sa ligne.
    fn selected_lines(&self) -> Range<usize> {
        let r = self.selection().unwrap_or(self.cursor..self.cursor);
        let first = self.rope.char_to_line(r.start);
        let mut last = self.rope.char_to_line(r.end);
        if last > first && self.rope.line_to_char(last) == r.end {
            last -= 1;
        }
        first..last + 1
    }

    /// Tab : indente les lignes d'une sélection multi-ligne, sinon insère une tabulation
    /// ou des espaces jusqu'au prochain taquet.
    pub fn tab(&mut self) {
        let multi = self
            .selection()
            .is_some_and(|r| self.rope.char_to_line(r.start) != self.rope.char_to_line(r.end));
        if multi {
            let unit = self.indent.unit();
            let edits = self
                .selected_lines()
                .filter(|&l| self.line_len(l) > 0)
                .map(|l| {
                    let p = self.rope.line_to_char(l);
                    (p..p, unit.clone())
                })
                .collect();
            return self.reindent(edits);
        }
        let (r, kind) = self.target();
        let text = if self.indent.use_tabs {
            "\t".into()
        } else {
            // Taquet calculé depuis le début de la plage remplacée, pas depuis le curseur.
            let line = self.rope.char_to_line(r.start);
            let col = self.display_col(line, r.start - self.rope.line_to_char(line));
            let w = self.indent.width.max(1);
            " ".repeat(w - col % w)
        };
        self.apply(r, &text, kind);
    }

    /// Shift+Tab : retire une tabulation ou jusqu'à `width` espaces en tête de chaque ligne touchée.
    pub fn untab(&mut self) {
        let w = self.indent.width.max(1);
        let edits = self
            .selected_lines()
            .filter_map(|l| {
                let p = self.rope.line_to_char(l);
                let line = self.rope.line(l);
                let n = if line.get_char(0) == Some('\t') {
                    1
                } else {
                    line.chars().take(w).take_while(|&c| c == ' ').count()
                };
                (n > 0).then(|| (p..p + n, String::new()))
            })
            .collect();
        self.reindent(edits);
    }

    /// Applique `edits` (triées) en une entrée et reporte curseur et ancre.
    fn reindent(&mut self, edits: Vec<(Range<usize>, String)>) {
        let (c, a) = (self.cursor, self.anchor);
        let hi = c.max(a.unwrap_or(c));
        let map = |p: usize| map_pos(p, &edits, p == hi && a.is_some_and(|a| a != c));
        let (c, a) = (map(c), a.map(map));
        if self.apply_many(edits, Kind::Other) {
            self.place_after_edit(c, a);
        }
    }

    /// Plage remplacée par une frappe : la sélection, sinon le curseur.
    fn target(&self) -> (Range<usize>, Kind) {
        match self.selection() {
            Some(r) => (r, Kind::Other),
            None => (self.cursor..self.cursor, Kind::Insert),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let (r, kind) = self.target();
        self.apply(r, c.encode_utf8(&mut [0; 4]), kind);
    }

    /// Entrée : reprend l'indentation de tête, un niveau de plus après `{ [ ( :`,
    /// et entre `{}` `[]` `()` renvoie la fermante sur sa propre ligne.
    pub fn insert_newline(&mut self) {
        let (r, _) = self.target();
        let (start, before) = self.line_before(r.start);
        let indent: String = before
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let opener = before.trim_end().chars().last();
        let mut text = format!("\n{indent}");
        let mut cursor = None;
        if matches!(opener, Some('{' | '[' | '(' | ':')) {
            text += &self.indent.unit();
            if matches!(
                (opener, self.rope.get_char(r.end)),
                (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
            ) {
                cursor = Some(text.chars().count());
                text += &format!("\n{indent}");
            }
        }
        // Ligne quittée faite seulement d'indentation : on la vide.
        let from = if before.trim().is_empty() {
            start
        } else {
            r.start
        };
        self.apply(from..r.end, &text, Kind::Other);
        if let Some(n) = cursor {
            self.place_after_edit(from + n, None);
        }
    }

    /// Insère du texte (collage) en une seule entrée d'historique ; `\r\n` et `\r` deviennent `\n`.
    pub fn insert_str(&mut self, s: &str) {
        let text = s.replace("\r\n", "\n").replace('\r', "\n");
        let (r, _) = self.target();
        self.apply(r, &text, Kind::Other);
    }

    /// Supprime la sélection, sinon le graphème avant le curseur.
    pub fn backspace(&mut self) {
        match self.selection() {
            Some(r) => self.apply(r, "", Kind::Other),
            None => {
                let (start, before) = self.line_before(self.cursor);
                let n = before.chars().count();
                // Dans une indentation d'espaces : jusqu'au taquet précédent.
                let from = if n > 0 && before.chars().all(|c| c == ' ') {
                    let w = self.indent.width.max(1);
                    start + (n - 1) / w * w
                } else {
                    self.prev_boundary(self.cursor)
                };
                self.apply(from..self.cursor, "", Kind::Backspace);
            }
        }
    }

    /// Supprime la sélection, sinon le graphème après le curseur.
    pub fn delete(&mut self) {
        match self.selection() {
            Some(r) => self.apply(r, "", Kind::Other),
            None => self.apply(
                self.cursor..self.next_boundary(self.cursor),
                "",
                Kind::Delete,
            ),
        }
    }

    /// Seule mutation de la rope : remplace `range` (chars) par `text` et journalise l'édition en octets.
    fn replace(&mut self, range: Range<usize>, text: &str) {
        let point = |rope: &Rope, byte: usize| {
            let line = rope.byte_to_line(byte);
            (line, byte - rope.line_to_byte(line))
        };
        let start_byte = self.rope.char_to_byte(range.start);
        let old_end_byte = self.rope.char_to_byte(range.end);
        let (start, old_end) = (
            point(&self.rope, start_byte),
            point(&self.rope, old_end_byte),
        );
        self.rope.remove(range.clone());
        self.rope.insert(range.start, text);
        let new_end_byte = start_byte + text.len();
        self.edits.push(TextEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start,
            old_end,
            new_end: point(&self.rope, new_end_byte),
        });
    }

    /// Position LSP (ligne, colonne en unités UTF-16) de l'index de `char` `pos` (borné au texte).
    pub fn lsp_position(&self, pos: usize) -> crate::Position {
        let pos = pos.min(self.rope.len_chars());
        let line = self.rope.char_to_line(pos);
        let start = self.rope.line_to_char(line);
        let character = self
            .rope
            .slice(start..pos)
            .chars()
            .map(char::len_utf16)
            .sum();
        crate::Position { line, character }
    }

    /// Index de `char` d'une position LSP ; ligne ou colonne hors bornes → ramenée au texte
    /// (fin de ligne, fin de document), une colonne au milieu d'une paire UTF-16 → avant le caractère.
    pub fn char_at_lsp(&self, p: crate::Position) -> usize {
        if p.line >= self.rope.len_lines() {
            return self.rope.len_chars();
        }
        let start = self.rope.line_to_char(p.line);
        let mut units = 0;
        let mut chars = 0;
        for c in self.line_text(p.line).chars() {
            units += c.len_utf16();
            if units > p.character {
                break;
            }
            chars += 1;
        }
        start + chars
    }

    /// Éditions depuis le dernier appel, dans l'ordre, à appliquer à l'arbre syntaxique.
    pub fn take_edits(&mut self) -> Vec<TextEdit> {
        std::mem::take(&mut self.edits)
    }

    /// Annule le dernier groupe d'éditions ; `false` s'il n'y en a pas.
    pub fn undo(&mut self) -> bool {
        let Some(e) = self.history.undo.pop() else {
            return false;
        };
        for c in e.changes.iter().rev() {
            self.replace(c.pos..c.pos + c.inserted.chars().count(), &c.removed);
        }
        (self.cursor, self.anchor) = e.before;
        self.version = e.version_before;
        self.preferred_col = None;
        self.history.redo.push(e);
        self.history.seal();
        true
    }

    /// Rétablit le dernier groupe annulé ; `false` s'il n'y en a pas.
    pub fn redo(&mut self) -> bool {
        let Some(e) = self.history.redo.pop() else {
            return false;
        };
        for c in &e.changes {
            self.replace(c.pos..c.pos + c.removed.chars().count(), &c.inserted);
        }
        (self.cursor, self.anchor) = e.after;
        self.version = e.version_after;
        self.preferred_col = None;
        self.history.undo.push(e);
        self.history.seal();
        true
    }

    /// Plage sélectionnée triée, `None` si vide.
    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor.filter(|&a| a != self.cursor)?;
        Some(a.min(self.cursor)..a.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|r| self.rope.slice(r).to_string())
    }

    pub fn select_all(&mut self) {
        self.set_cursor(0, false);
        self.set_cursor(self.rope.len_chars(), true);
    }

    /// Place le curseur (borné au texte) ; `extend` garde ou pose l'ancre de sélection.
    pub fn set_cursor(&mut self, pos: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = pos.min(self.rope.len_chars());
        self.preferred_col = None;
        self.history.seal();
    }

    /// Index de `char` à la colonne d'affichage `col` de `line` ; ligne bornée au document, fin de ligne au-delà.
    pub fn pos_at(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.rope.len_lines() - 1);
        self.rope.line_to_char(line) + self.char_col_for_display(line, col)
    }

    /// Déplacements par graphème (gauche/droite) et par ligne avec colonne d'affichage préférée (haut/bas).
    /// `extend` (Shift) étend la sélection ; sinon gauche/droite sur une sélection vont à son bord.
    pub fn move_left(&mut self, extend: bool) {
        let pos = match self.selection() {
            Some(r) if !extend => r.start,
            _ => self.prev_boundary(self.cursor),
        };
        self.set_cursor(pos, extend);
    }
    pub fn move_right(&mut self, extend: bool) {
        let pos = match self.selection() {
            Some(r) if !extend => r.end,
            _ => self.next_boundary(self.cursor),
        };
        self.set_cursor(pos, extend);
    }
    pub fn move_up(&mut self, lines: usize, extend: bool) {
        let line = self.rope.char_to_line(self.cursor);
        self.move_to_line(line.saturating_sub(lines), extend);
    }
    pub fn move_down(&mut self, lines: usize, extend: bool) {
        let line = self.rope.char_to_line(self.cursor);
        let last = self.rope.len_lines() - 1;
        self.move_to_line(line.saturating_add(lines).min(last), extend);
    }
    fn move_to_line(&mut self, line: usize, extend: bool) {
        let target = self
            .preferred_col
            .unwrap_or_else(|| self.cursor_line_col().1);
        self.set_cursor(self.pos_at(line, target), extend);
        self.preferred_col = Some(target);
    }
    pub fn move_line_start(&mut self, extend: bool) {
        let pos = self.rope.line_to_char(self.rope.char_to_line(self.cursor));
        self.set_cursor(pos, extend);
    }
    pub fn move_line_end(&mut self, extend: bool) {
        let line = self.rope.char_to_line(self.cursor);
        self.set_cursor(self.rope.line_to_char(line) + self.line_len(line), extend);
    }

    /// Début du mot suivant / précédent (Ctrl+→ / Ctrl+←, comme VSCode) : saute les blancs, puis un groupe
    /// de caractères de même nature (lettres/chiffres/_ ou ponctuation). Un saut de ligne compte comme un blanc.
    fn word_boundary(&self, forward: bool) -> usize {
        let class = |c: char| {
            if c.is_whitespace() {
                0
            } else if c.is_alphanumeric() || c == '_' {
                1
            } else {
                2
            }
        };
        let len = self.rope.len_chars();
        let mut p = self.cursor;
        let at = |p: usize| self.rope.char(p);
        if forward {
            while p < len && class(at(p)) == 0 {
                p += 1;
            }
            let k = if p < len { class(at(p)) } else { 0 };
            while p < len && class(at(p)) == k && k != 0 {
                p += 1;
            }
        } else {
            while p > 0 && class(at(p - 1)) == 0 {
                p -= 1;
            }
            let k = if p > 0 { class(at(p - 1)) } else { 0 };
            while p > 0 && class(at(p - 1)) == k && k != 0 {
                p -= 1;
            }
        }
        p
    }

    pub fn move_word(&mut self, forward: bool, extend: bool) {
        let p = self.word_boundary(forward);
        self.set_cursor(p, extend);
    }

    /// Ctrl+Retour arrière : supprime la sélection, sinon jusqu'au début du mot précédent.
    pub fn delete_word_before(&mut self) {
        if self.selection().is_some() {
            return self.backspace();
        }
        let p = self.word_boundary(false);
        if p < self.cursor {
            self.edit(p..self.cursor, "");
        }
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
            "a\r\nb\r\nc\n",
        ] {
            let d = doc(text);
            assert_eq!(d.to_saved_text(), text, "{text:?}");
        }
        assert_eq!(doc("a\r\nb\r\n").rope().to_string(), "a\nb\n");
        // Mixte : gardé tel quel, les nouvelles lignes seront en LF.
        assert_eq!(doc("a\r\nb\r\nc\n").line_ending, LineEnding::Lf);
        assert_eq!(doc("a\r\nb\nc\n").line_ending, LineEnding::Lf);
    }

    #[test]
    fn crlf_edition_garde_crlf() {
        let mut d = doc("ab\r\ncd");
        d.move_line_end(false);
        d.insert_newline();
        d.insert_char('x');
        assert_eq!(d.to_saved_text(), "ab\r\nx\r\ncd");
        // Backspace en début de ligne supprime toute la fin de ligne.
        d.move_line_start(false);
        d.backspace();
        assert_eq!(d.to_saved_text(), "abx\r\ncd");
    }

    #[test]
    fn graphemes_un_seul_pas() {
        // e + accent combinant, emoji famille (ZWJ), drapeau.
        let text = "e\u{301}👨\u{200d}👩\u{200d}👧🇫🇷";
        let mut d = doc(text);
        d.move_right(false);
        assert_eq!(d.cursor(), 2);
        d.move_right(false);
        assert_eq!(d.cursor(), 7);
        d.move_right(false);
        assert_eq!(d.cursor(), 9);
        d.move_right(false);
        assert_eq!(d.cursor(), 9);
        d.backspace();
        assert_eq!(d.rope().to_string(), "e\u{301}👨\u{200d}👩\u{200d}👧");
        d.move_line_start(false);
        d.delete();
        assert_eq!(d.rope().to_string(), "👨\u{200d}👩\u{200d}👧");
        d.move_left(false);
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
    fn positions_lsp_en_utf16() {
        use crate::Position;
        // 🦀 = 2 unités UTF-16, é = 1 ; CRLF compte comme une seule fin de ligne.
        let d = doc("é🦀x\r\nab");
        let pos = |line, character| Position { line, character };
        assert_eq!(d.lsp_position(0), pos(0, 0));
        assert_eq!(d.lsp_position(2), pos(0, 3));
        assert_eq!(d.lsp_position(3), pos(0, 4));
        assert_eq!(d.lsp_position(5), pos(1, 1));
        assert_eq!(d.lsp_position(99), pos(1, 2));
        for i in 0..=d.rope().len_chars() {
            assert_eq!(d.char_at_lsp(d.lsp_position(i)), i, "aller-retour {i}");
        }
        // Milieu de la paire de 🦀 → avant ; colonne et ligne trop grandes → bornées.
        assert_eq!(d.char_at_lsp(pos(0, 2)), 1);
        assert_eq!(d.char_at_lsp(pos(0, 50)), 3);
        assert_eq!(d.char_at_lsp(pos(9, 0)), d.rope().len_chars());
    }

    #[test]
    fn journal_d_editions_rejouable_en_octets() {
        // Rejoue chaque édition sur une copie : texte et points (ligne, colonne-octet) doivent suivre la rope.
        let mut d = doc("é🦀\nab\n");
        let mut copie = d.rope().to_string();
        let mut check = |d: &mut Document| {
            let edits = d.take_edits();
            let texte = d.rope().to_string();
            let point = |s: &str, b: usize| {
                let ligne = s[..b].matches('\n').count();
                (ligne, b - s[..b].rfind('\n').map_or(0, |i| i + 1))
            };
            if let [e] = edits.as_slice() {
                // Une seule édition : on peut la rejouer avec le texte inséré lu dans la rope.
                assert_eq!(e.start, point(&copie, e.start_byte));
                assert_eq!(e.old_end, point(&copie, e.old_end_byte));
                copie.replace_range(
                    e.start_byte..e.old_end_byte,
                    &texte[e.start_byte..e.new_end_byte],
                );
                assert_eq!(e.new_end, point(&copie, e.new_end_byte));
            } else {
                // Lot : au moins la somme des longueurs doit correspondre.
                let delta: isize = edits
                    .iter()
                    .map(|e| e.new_end_byte as isize - e.old_end_byte as isize)
                    .sum();
                assert_eq!(copie.len() as isize + delta, texte.len() as isize);
                copie = texte.clone();
            }
            assert_eq!(copie, texte);
        };
        d.move_right(false);
        d.insert_str("x\ny");
        check(&mut d);
        d.move_down(1, false);
        d.backspace();
        check(&mut d);
        d.select_all();
        d.tab();
        check(&mut d);
        d.edit_many(vec![(0..1, "Z".into()), (3..5, "👍".into())]);
        check(&mut d);
        while d.undo() {}
        check(&mut d);
        while d.redo() {}
        check(&mut d);
    }

    #[test]
    fn deplacement_par_mot() {
        let mut d = doc("let x_1 = foo.bar();\n  é2");
        let mut sauts = vec![];
        for _ in 0..9 {
            d.move_word(true, false);
            sauts.push(d.cursor());
        }
        // let | x_1 | = | foo | . | bar | (); | é2 | fin
        assert_eq!(sauts, [3, 7, 9, 13, 14, 17, 20, 25, 25]);
        d.move_word(false, false);
        assert_eq!(d.cursor(), 23);
        d.move_word(false, true);
        assert_eq!(d.selection(), Some(17..23));
        d.set_cursor(13, false);
        d.delete_word_before();
        assert_eq!(d.rope().to_string(), "let x_1 = .bar();\n  é2");
    }

    #[test]
    fn seul_lf_coupe_les_lignes() {
        // Saut de page, U+2028 ou `\r` isolé ne sont pas des fins de ligne (comme rustc et le LSP).
        assert_eq!(doc("a\x0Cb\nc\n").rope().len_lines(), 3);
        assert_eq!(doc("a\u{2028}b\rc").rope().len_lines(), 1);
        assert_eq!(doc("a\r\nb").rope().len_lines(), 2);
    }

    #[test]
    fn tab_sur_selection_d_une_ligne_depuis_son_debut() {
        let mut d = doc("abc");
        d.set_cursor(0, false);
        d.set_cursor(3, true);
        d.tab();
        assert_eq!(d.rope().to_string(), "    ");
    }

    #[test]
    fn largeur_tabulation_editorconfig() {
        let mut d = doc("a\tb\tc");
        d.indent = Indent {
            use_tabs: true,
            width: 4,
            tab_width: 8,
        };
        assert_eq!(d.display_col(0, 2), 8);
        assert_eq!(d.display_col(0, 4), 16);
        assert_eq!(d.pos_at(0, 9), 3); // clic juste après « b » → après b
        // Indentation par espaces : `tab_width` s'applique toujours à l'affichage.
        d.indent.use_tabs = false;
        assert_eq!(d.display_col(0, 2), 8);
    }

    #[test]
    fn haut_bas_colonne_preferee() {
        let mut d = doc("abcdef\nab\n中文字\nabcdef");
        d.move_line_end(false); // col 6
        d.move_down(1, false);
        assert_eq!(d.cursor_line_col(), (1, 2));
        d.move_down(1, false);
        assert_eq!(d.cursor_line_col(), (2, 6));
        d.move_down(1, false);
        assert_eq!(d.cursor_line_col(), (3, 6));
        d.move_up(1, false);
        d.move_left(false);
        d.move_up(1, false); // col 4 : préférée oubliée après move_left
        assert_eq!(d.cursor_line_col(), (1, 2));
        d.move_down(1, false);
        assert_eq!(d.cursor_line_col(), (2, 4));
        // Colonne au milieu d'un idéogramme : on reste avant.
        let mut d = doc("abc\n中文");
        d.move_right(false);
        d.move_down(1, false);
        assert_eq!(d.cursor_line_col(), (1, 0));
        d.move_down(5, false);
        d.move_up(9, false);
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
        d.move_right(false);
        d.move_down(1, false);
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
                    Op::Left => d.move_left(false),
                    Op::Right => d.move_right(false),
                    Op::Up(n) => d.move_up(n, false),
                    Op::Down(n) => d.move_down(n, false),
                    Op::Home => d.move_line_start(false),
                    Op::End => d.move_line_end(false),
                }
                prop_assert!(d.cursor() <= d.rope().len_chars());
                let _ = d.cursor_line_col();
            }
            let again = Document::from_text(None, &d.to_saved_text());
            prop_assert_eq!(again.rope().to_string(), d.rope().to_string());
        }
    }

    fn texte(d: &Document) -> String {
        d.rope().to_string()
    }

    fn taper(d: &mut Document, s: &str) {
        s.chars().for_each(|c| d.insert_char(c));
    }

    #[test]
    fn undo_redo_regroupe_par_mot() {
        let mut d = doc("");
        taper(&mut d, "ab cd\nef");
        assert!(d.undo());
        assert_eq!(texte(&d), "ab cd");
        assert!(d.undo());
        assert_eq!(texte(&d), "ab");
        assert_eq!(d.cursor(), 2);
        assert!(d.redo());
        assert_eq!((texte(&d).as_str(), d.cursor()), ("ab cd", 5));
        // Un déplacement coupe le groupe, un changement de type aussi.
        d.move_left(false);
        d.move_right(false);
        taper(&mut d, "x");
        d.backspace();
        d.backspace();
        assert_eq!(texte(&d), "ab c");
        assert!(d.undo());
        assert_eq!(texte(&d), "ab cdx");
        assert!(d.undo());
        assert_eq!(texte(&d), "ab cd");
        assert!(d.undo() && d.undo());
        assert!(!d.undo());
        assert_eq!(texte(&d), "");
    }

    #[test]
    fn nouvelle_edition_vide_redo() {
        let mut d = doc("a");
        d.move_line_end(false);
        taper(&mut d, "b");
        assert!(d.undo());
        taper(&mut d, "c");
        assert!(!d.redo());
        assert_eq!(texte(&d), "ac");
    }

    #[test]
    fn dirty_suit_undo_redo() {
        let mut d = doc("a");
        taper(&mut d, "b");
        d.mark_saved(d.version());
        taper(&mut d, "c");
        assert!(d.is_dirty());
        assert!(d.undo());
        assert!(!d.is_dirty());
        assert!(d.undo());
        assert!(d.is_dirty());
        assert!(d.redo());
        assert!(!d.is_dirty());
        assert!(d.redo());
        assert!(d.is_dirty());
        // L'état d'origine et un contenu retapé identique ont des versions différentes : prudent.
        let mut d = doc("a");
        taper(&mut d, "b");
        d.backspace();
        assert!(d.is_dirty());
    }

    #[test]
    fn undo_restaure_la_selection() {
        let mut d = doc("abc");
        d.move_right(false);
        d.move_line_end(true);
        taper(&mut d, "x");
        assert_eq!(texte(&d), "ax");
        assert!(d.undo());
        assert_eq!(texte(&d), "abc");
        assert_eq!(d.selection(), Some(1..3));
    }

    #[test]
    fn selection_shift_et_remplacement() {
        let mut d = doc("héllo\nmonde");
        d.move_right(true);
        d.move_right(true);
        assert_eq!(d.selection(), Some(0..2));
        assert_eq!(d.selected_text().as_deref(), Some("hé"));
        // Sans Shift : gauche/droite vont au bord de la sélection.
        d.move_left(false);
        assert_eq!((d.cursor(), d.selection()), (0, None));
        d.move_line_end(true);
        d.move_left(true);
        assert_eq!(d.selection(), Some(0..4));
        d.move_right(false);
        assert_eq!((d.cursor(), d.selection()), (4, None));
        d.move_down(1, true);
        assert_eq!(d.selected_text().as_deref(), Some("o\nmond"));
        d.insert_char('!');
        assert_eq!(texte(&d), "héll!e");
        d.select_all();
        d.backspace();
        assert_eq!(texte(&d), "");
        let mut d = doc("abc");
        d.set_cursor(99, false);
        assert_eq!(d.cursor(), 3);
        d.set_cursor(1, true);
        d.delete();
        assert_eq!(texte(&d), "a");
    }

    #[test]
    fn insert_str_normalise_et_une_seule_entree() {
        let mut d = doc("xy");
        d.move_right(false);
        d.insert_str("a\r\nb\rc");
        assert_eq!((texte(&d).as_str(), d.cursor()), ("xa\nb\ncy", 6));
        assert!(d.undo());
        assert_eq!(texte(&d), "xy");
        d.edit(0..9, "z");
        assert_eq!((texte(&d).as_str(), d.cursor()), ("z", 1));
    }

    #[derive(Debug, Clone)]
    enum Ed {
        Str(String),
        Back,
        Del,
        Left(bool),
        Right(bool),
        Up(usize, bool),
        Down(usize, bool),
        Home(bool),
        End(bool),
        Edit(usize, usize, String),
        Tab(bool),
        Newline,
    }

    fn ed() -> impl Strategy<Value = Ed> {
        let s = "[aé😀👍\u{301}\u{200d}中 \n\r]{0,4}";
        prop_oneof![
            s.prop_map(Ed::Str),
            Just(Ed::Back),
            Just(Ed::Del),
            any::<bool>().prop_map(Ed::Left),
            any::<bool>().prop_map(Ed::Right),
            (0usize..4, any::<bool>()).prop_map(|(n, e)| Ed::Up(n, e)),
            (0usize..4, any::<bool>()).prop_map(|(n, e)| Ed::Down(n, e)),
            any::<bool>().prop_map(Ed::Home),
            any::<bool>().prop_map(Ed::End),
            (0usize..100, 0usize..100, s).prop_map(|(a, b, t)| Ed::Edit(a, b, t)),
            any::<bool>().prop_map(Ed::Tab),
            Just(Ed::Newline),
        ]
    }

    proptest! {
        #[test]
        fn undo_complet_puis_redo_complet(text in "[a-zé中\u{301}\n]{0,30}", ops in prop::collection::vec(ed(), 0..60)) {
            let mut d = doc(&text);
            for o in ops {
                let n = d.rope().len_chars() + 1;
                match o {
                    Ed::Str(s) => d.insert_str(&s),
                    Ed::Back => d.backspace(),
                    Ed::Del => d.delete(),
                    Ed::Left(e) => d.move_left(e),
                    Ed::Right(e) => d.move_right(e),
                    Ed::Up(k, e) => d.move_up(k, e),
                    Ed::Down(k, e) => d.move_down(k, e),
                    Ed::Home(e) => d.move_line_start(e),
                    Ed::End(e) => d.move_line_end(e),
                    Ed::Edit(a, b, s) => d.edit((a % n).min(b % n)..(a % n).max(b % n), &s),
                    Ed::Tab(true) => d.untab(),
                    Ed::Tab(false) => d.tab(),
                    Ed::Newline => d.insert_newline(),
                }
                prop_assert!(d.cursor() <= d.rope().len_chars());
            }
            let fin = texte(&d);
            let version = d.version();
            while d.undo() {}
            prop_assert_eq!(texte(&d), text);
            prop_assert!(!d.is_dirty());
            while d.redo() {}
            prop_assert_eq!(texte(&d), fin);
            prop_assert_eq!(d.version(), version);
        }
    }

    fn avec_indent(text: &str, use_tabs: bool, width: usize) -> Document {
        let mut d = doc(text);
        d.indent = Indent {
            use_tabs,
            width,
            tab_width: width,
        };
        d
    }

    #[test]
    fn tab_espaces_aux_taquets() {
        let mut d = doc("a");
        d.move_line_end(false);
        d.tab();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("a   ", 4));
        d.tab();
        assert_eq!(texte(&d), format!("a{}", " ".repeat(7)));
        let mut d = avec_indent("abc", false, 2);
        d.move_line_end(false);
        d.tab();
        assert_eq!(texte(&d), "abc ");
        let mut d = avec_indent("ab", true, 4);
        d.move_right(false);
        d.tab();
        assert_eq!(texte(&d), "a\tb");
    }

    #[test]
    fn tab_multiligne_et_undo_unique() {
        let mut d = doc("a\n\nb\nc");
        d.move_right(false);
        d.move_down(2, true); // de "a|" à "b|" : lignes 0 à 2
        d.tab();
        assert_eq!(texte(&d), "    a\n\n    b\nc");
        assert_eq!(d.selection(), Some(5..12));
        assert!(d.undo());
        assert_eq!(
            (texte(&d).as_str(), d.selection()),
            ("a\n\nb\nc", Some(1..4))
        );
        assert!(!d.undo());
        assert!(d.redo());
        assert_eq!(d.selection(), Some(5..12));
        // Fin de sélection en colonne 0 : sa ligne n'est pas indentée.
        let mut d = avec_indent("a\nb\nc", true, 4);
        d.move_down(1, true);
        d.tab();
        assert_eq!(texte(&d), "\ta\nb\nc");
        assert_eq!(d.selection(), Some(0..3));
    }

    #[test]
    fn shift_tab_desindente() {
        let mut d = doc("  a\n\tb\n      c");
        d.select_all();
        d.untab();
        assert_eq!(texte(&d), "a\nb\n  c");
        assert_eq!(d.selection(), Some(0..7));
        assert!(d.undo());
        assert_eq!(texte(&d), "  a\n\tb\n      c");
        assert!(!d.undo());
        // Sans sélection : ligne du curseur, curseur suivi.
        let mut d = avec_indent("    x", false, 2);
        d.move_line_end(false);
        d.untab();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("  x", 3));
        d.move_line_start(false);
        d.untab();
        d.untab();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("x", 0));
    }

    #[test]
    fn entree_indentation() {
        // Indentation copiée, seulement la portion avant le curseur.
        let mut d = doc("  ab");
        d.set_cursor(3, false);
        d.insert_newline();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("  a\n  b", 6));
        // Après `{` en fin de ligne : un niveau de plus.
        let mut d = doc("\tif x {");
        d.move_line_end(false);
        d.insert_newline();
        assert_eq!(texte(&d), "\tif x {\n\t    ");
        // Entre `{}` : fermante sur sa ligne, une seule entrée d'historique.
        let mut d = avec_indent("  f(){}", false, 2);
        d.set_cursor(6, false);
        d.insert_newline();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("  f(){\n    \n  }", 11));
        assert!(d.undo());
        assert_eq!((texte(&d).as_str(), d.cursor()), ("  f(){}", 6));
        assert!(d.redo());
        assert_eq!(d.cursor(), 11);
        // `:` (Python), et ligne d'espaces quittée : vidée.
        let mut d = doc("def f():");
        d.move_line_end(false);
        d.insert_newline();
        d.insert_newline();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("def f():\n\n    ", 14));
    }

    #[test]
    fn backspace_au_taquet() {
        let mut d = doc("      x");
        d.set_cursor(6, false);
        d.backspace();
        assert_eq!((texte(&d).as_str(), d.cursor()), ("    x", 4));
        d.backspace();
        assert_eq!(texte(&d), "x");
        let mut d = avec_indent("   ", false, 2);
        d.move_line_end(false);
        d.backspace();
        assert_eq!(texte(&d), "  ");
        // Hors indentation : un graphème.
        let mut d = doc("a   ");
        d.move_line_end(false);
        d.backspace();
        assert_eq!(texte(&d), "a  ");
    }

    #[test]
    fn edit_many_une_entree() {
        let mut d = doc("abc");
        d.edit_many(vec![(2..3, "Z".into()), (0..1, "XY".into())]);
        assert_eq!((texte(&d).as_str(), d.cursor()), ("XYbZ", 2));
        assert!(d.undo());
        assert_eq!(texte(&d), "abc");
        assert!(!d.undo());
    }
}
