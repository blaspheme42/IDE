//! Types LSP côté cœur (J4) : positions UTF-16, diagnostics, complétion, survol. Zéro I/O :
//! le client LSP vit dans l'app, qui convertit vers et depuis ces types.

use std::path::PathBuf;

/// Position LSP : ligne 0-based et colonne en unités UTF-16 (encodage par défaut du protocole).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub start: Position,
    pub end: Position,
    pub severity: Severity,
    pub message: String,
    /// Émetteur (`rustc`, `clippy`, `clangd`…).
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    /// Nature courte affichée dans la liste (`fn`, `struct`, `var`…).
    pub kind: Option<String>,
    pub detail: Option<String>,
    /// Texte inséré à la place du mot en cours (sans snippets : `$1`… déjà retirés par l'app).
    pub insert_text: String,
}

/// Liste de complétion ouverte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Éléments reçus du serveur, dans son ordre.
    pub items: Vec<CompletionItem>,
    /// Index dans les éléments filtrés (`visible`).
    pub selected: usize,
    /// Index de `char` du début du mot complété : le texte de `anchor` au curseur sert de filtre.
    pub anchor: usize,
    /// Document visé : la liste se ferme quand l'onglet actif change.
    pub path: Option<PathBuf>,
}

/// Infobulle de survol ouverte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    /// Texte brut (Markdown laissé tel quel).
    pub text: String,
    /// Index de `char` du symbole survolé.
    pub at: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub start: Position,
    pub end: Position,
}

/// Serveur à lancer pour un langage (`[lsp.<langage>]` de config.toml).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServer {
    /// Nom en minuscules, comme `ide_syntax::Language::name()` en minuscules (`rust`, `python`, `c`…).
    pub language: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Serveurs par défaut (F-40) : rust-analyzer, pyright, clangd.
pub fn default_servers() -> Vec<LspServer> {
    let s = |language: &str, command: &str, args: &[&str]| LspServer {
        language: language.into(),
        command: command.into(),
        args: args.iter().map(|a| (*a).into()).collect(),
    };
    vec![
        s("rust", "rust-analyzer", &[]),
        s("python", "pyright-langserver", &["--stdio"]),
        s("c", "clangd", &[]),
    ]
}

use crate::{Command, Document, Focus, Key, KeyInput, State};

/// Lignes du panneau « Problèmes », sous l'éditeur (Ctrl+Shift+M).
/// shortcut: pas de navigation clavier dans le panneau (F8 / Shift+F8 suffisent), à ajouter avec le focus du panneau.
pub const PROBLEMS_HEIGHT: u16 = 8;

/// Éléments affichés au plus dans la liste de complétion.
const MAX_VISIBLE: usize = 50;

/// Suivi interne des requêtes en vol (pas destiné à l'UI).
#[derive(Debug, Default)]
pub struct LspPending {
    /// La touche en cours est un caractère tapé dans l'éditeur.
    typed: bool,
    /// Début du mot visé par la requête de complétion en attente.
    completion_anchor: Option<usize>,
    /// Document et curseur de la requête de survol en attente.
    hover_at: Option<(PathBuf, usize)>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Début du mot identifiant qui finit en `pos`.
fn word_start(doc: &Document, pos: usize) -> usize {
    let rope = doc.rope();
    let mut i = pos.min(rope.len_chars());
    while i > 0 && is_ident(rope.char(i - 1)) {
        i -= 1;
    }
    i
}

/// Éléments filtrés par le mot en cours (sous-suite insensible à la casse), préfixes d'abord ; 50 au plus.
pub fn visible_completions(state: &State) -> Vec<&CompletionItem> {
    let Some(c) = &state.completion else {
        return vec![];
    };
    let cursor = state.doc.cursor();
    let word = state
        .doc
        .rope()
        .slice(c.anchor.min(cursor)..cursor)
        .to_string()
        .to_lowercase();
    let fuzzy = |s: &str| {
        let mut it = s.chars().flat_map(char::to_lowercase);
        word.chars().all(|w| it.any(|c| c == w))
    };
    let prefix = |s: &str| s.to_lowercase().starts_with(&word);
    let mut v: Vec<&CompletionItem> = c
        .items
        .iter()
        .filter(|i| fuzzy(&i.label) || fuzzy(&i.insert_text))
        .collect();
    v.sort_by_key(|i| !(prefix(&i.label) || prefix(&i.insert_text)));
    v.truncate(MAX_VISIBLE);
    v
}

/// Diagnostics de tous les fichiers, triés par chemin, sévérité puis ligne.
pub fn problems(state: &State) -> Vec<(PathBuf, Diagnostic)> {
    let mut v: Vec<_> = state
        .diagnostics
        .iter()
        .flat_map(|(p, ds)| ds.iter().map(|d| (p.clone(), d.clone())))
        .collect();
    v.sort_by(|a, b| (&a.0, a.1.severity, a.1.start).cmp(&(&b.0, b.1.severity, b.1.start)));
    v
}

/// (erreurs, avertissements), tous fichiers confondus.
pub fn diagnostic_counts(state: &State) -> (usize, usize) {
    let n = |s| {
        state
            .diagnostics
            .values()
            .flatten()
            .filter(|d| d.severity == s)
            .count()
    };
    (n(Severity::Error), n(Severity::Warning))
}

/// Requête de complétion au curseur ; `None` sans chemin.
fn request_completion(state: &mut State) -> Option<Command> {
    let path = state.doc.path.clone()?;
    let cursor = state.doc.cursor();
    state.lsp_pending.completion_anchor = Some(word_start(&state.doc, cursor));
    Some(Command::RequestCompletion {
        path,
        version: state.doc.version(),
        pos: state.doc.lsp_position(cursor),
    })
}

/// Remplace le mot en cours par l'élément choisi (une seule entrée d'historique) et ferme la liste.
fn accept(state: &mut State) {
    let selected = state.completion.as_ref().map_or(0, |c| c.selected);
    let text = visible_completions(state)
        .get(selected)
        .map(|i| i.insert_text.clone());
    let Some(c) = state.completion.take() else {
        return;
    };
    if let Some(text) = text {
        let cursor = state.doc.cursor();
        state.doc.seal_history();
        state.doc.edit(c.anchor.min(cursor)..cursor, &text);
    }
}

/// F8 / Shift+F8 : diagnostic suivant ou précédent du document actif, en boucle.
fn step_diagnostic(state: &mut State, forward: bool) {
    let mut starts: Vec<(usize, &str)> = state
        .doc
        .path
        .as_ref()
        .and_then(|p| state.diagnostics.get(p))
        .into_iter()
        .flatten()
        .map(|d| (state.doc.char_at_lsp(d.start), d.message.as_str()))
        .collect();
    starts.sort_by_key(|d| d.0);
    let cursor = state.doc.cursor();
    let found = if forward {
        starts.iter().find(|d| d.0 > cursor).or(starts.first())
    } else {
        starts.iter().rev().find(|d| d.0 < cursor).or(starts.last())
    };
    match found.map(|&(pos, msg)| (pos, msg.to_string())) {
        Some((pos, msg)) => {
            state.status = msg;
            state.doc.set_cursor(pos, false);
        }
        None => state.status = "Aucun problème".into(),
    }
}

/// Touches de la complétion (liste ouverte : Haut/Bas/Entrée/Tab/Échap) et déclencheurs
/// (Ctrl+Espace, F12, survol au clavier). `None` = touche laissée à la suite.
pub(crate) fn key(state: &mut State, k: KeyInput) -> Option<Vec<Command>> {
    let (ctrl, alt, shift) = (k.mods.ctrl, k.mods.alt, k.mods.shift);
    let plain = !ctrl && !alt && !shift;
    // Toute touche ferme l'infobulle ; Échap ne sert alors qu'à ça.
    if state.hover.take().is_some() && k.key == Key::Esc && plain {
        return Some(vec![]);
    }
    state.lsp_pending.typed =
        matches!(k.key, Key::Char(c) if !c.is_control()) && !ctrl && !alt && state.search.is_none();
    if state.completion.is_some() {
        let n = visible_completions(state).len().max(1);
        let sel = state.completion.as_ref().map_or(0, |c| c.selected);
        let sel = match k.key {
            Key::Up if plain => (sel + n - 1) % n,
            Key::Down if plain => (sel + 1) % n,
            Key::Enter | Key::Tab if plain => {
                accept(state);
                return Some(vec![]);
            }
            Key::Esc if plain => {
                state.completion = None;
                return Some(vec![]);
            }
            _ => return trigger(state, k),
        };
        if let Some(c) = &mut state.completion {
            c.selected = sel;
        }
        return Some(vec![]);
    }
    trigger(state, k)
}

/// Ctrl+Espace, Ctrl+K, F12, Ctrl+Shift+M, F8 / Shift+F8.
fn trigger(state: &mut State, k: KeyInput) -> Option<Vec<Command>> {
    let (ctrl, alt, shift) = (k.mods.ctrl, k.mods.alt, k.mods.shift);
    let cursor = state.doc.cursor();
    let pos = state.doc.lsp_position(cursor);
    let cmd = match k.key {
        // crossterm envoie Ctrl+Espace comme `Char(' ')` + Ctrl.
        Key::Char(' ') if ctrl && !alt => request_completion(state)?,
        Key::Char('k' | 'K') if ctrl && !alt && !shift => {
            let path = state.doc.path.clone()?;
            state.lsp_pending.hover_at = Some((path.clone(), cursor));
            Command::RequestHover { path, pos }
        }
        Key::F(12) if !ctrl && !alt && !shift => Command::RequestDefinition {
            path: state.doc.path.clone()?,
            pos,
        },
        // Selon le terminal : `M` (avec ou sans Shift) ou `m` + Shift.
        Key::Char(c @ ('m' | 'M')) if ctrl && !alt && (shift || c == 'M') => {
            state.problems_open = !state.problems_open;
            return Some(vec![]);
        }
        Key::F(8) if !ctrl && !alt => {
            step_diagnostic(state, !shift);
            return Some(vec![]);
        }
        _ => return None,
    };
    Some(vec![cmd])
}

/// Après chaque événement : déclenchement à la frappe, filtrage ou fermeture de la liste.
/// `version_before` = version du document actif avant l'événement.
pub(crate) fn after_update(state: &mut State, version_before: u64) -> Vec<Command> {
    let typed = std::mem::take(&mut state.lsp_pending.typed);
    let changed = state.doc.version() != version_before;
    let cursor = state.doc.cursor();
    let editor = state.focus == Focus::Editor;
    if state
        .hover
        .as_ref()
        .is_some_and(|h| changed || !editor || h.at != cursor)
    {
        state.hover = None;
    }
    if let Some(c) = &mut state.completion {
        if changed {
            c.selected = 0;
        }
        // Onglet changé, curseur sorti du mot (autre ligne, avant `anchor`), sélection, perte de focus.
        let stale = !editor
            || c.path != state.doc.path
            || word_start(&state.doc, cursor) != c.anchor
            || state.doc.selection().is_some();
        if stale || visible_completions(state).is_empty() {
            state.completion = None;
        }
    }
    // Liste ouverte : la frappe filtre localement, sans nouvelle requête.
    if !(typed && changed && editor && state.completion.is_none() && cursor > 0) {
        return vec![];
    }
    let rope = state.doc.rope();
    let c = rope.char(cursor - 1);
    let colons = c == ':' && cursor > 1 && rope.char(cursor - 2) == ':';
    if is_ident(c) || c == '.' || colons {
        request_completion(state).into_iter().collect()
    } else {
        vec![]
    }
}

pub(crate) fn completion_result(
    state: &mut State,
    path: PathBuf,
    version: u64,
    items: Vec<CompletionItem>,
) {
    let doc = &state.doc;
    let Some(anchor) = state.lsp_pending.completion_anchor.take().filter(|&a| {
        doc.path.as_ref() == Some(&path)
            && doc.version() == version
            && word_start(doc, doc.cursor()) == a
    }) else {
        return;
    };
    state.completion = (!items.is_empty()).then_some(Completion {
        items,
        selected: 0,
        anchor,
        path: Some(path),
    });
    if visible_completions(state).is_empty() {
        state.completion = None;
    }
}

pub(crate) fn hover_result(state: &mut State, path: PathBuf, text: Option<String>) {
    let at = state.doc.cursor();
    let current = state.doc.path.as_ref().is_some_and(|p| *p == path);
    if state.lsp_pending.hover_at.take() != Some((path, at)) || !current {
        return;
    }
    state.hover = text
        .filter(|t| !t.trim().is_empty())
        .map(|text| Hover { text, at });
}

/// Aller à la définition : ouvre le fichier si besoin et place le curseur.
pub(crate) fn definition_result(state: &mut State, locations: Vec<Location>) -> Vec<Command> {
    let Some(loc) = locations.into_iter().next() else {
        state.status = "Aucune définition".into();
        return vec![];
    };
    if state.doc.path.as_ref() == Some(&loc.path) {
        let pos = state.doc.char_at_lsp(loc.start);
        state.doc.set_cursor(pos, false);
        return vec![];
    }
    let path = loc.path.clone();
    state.pending_jump = Some(loc);
    vec![Command::OpenFile(path)]
}

/// Après `FileOpened` : applique le saut de définition en attente s'il visait ce fichier (sinon l'oublie).
pub(crate) fn apply_pending_jump(state: &mut State) {
    if let Some(loc) = state.pending_jump.take()
        && state.doc.path.as_ref() == Some(&loc.path)
    {
        let pos = state.doc.char_at_lsp(loc.start);
        state.doc.set_cursor(pos, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, Modifiers, editor_size, update};

    fn touche(key: Key, ctrl: bool, shift: bool) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers {
                ctrl,
                alt: false,
                shift,
            },
        })
    }
    fn k(key: Key) -> Event {
        touche(key, false, false)
    }
    fn ouvrir(s: &mut State, path: &str, text: &str) -> Vec<Command> {
        update(
            s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        )
    }
    fn ouvert(path: &str, text: &str) -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 20,
            },
        );
        ouvrir(&mut s, path, text);
        s
    }
    fn tape(s: &mut State, text: &str) -> Vec<Command> {
        text.chars()
            .flat_map(|c| update(s, k(Key::Char(c))))
            .collect()
    }
    fn item(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.into(),
            kind: None,
            detail: None,
            insert_text: label.into(),
        }
    }
    fn reponse(s: &mut State, version: u64, labels: &[&str]) {
        let path = s.doc.path.clone().unwrap_or_default();
        update(
            s,
            Event::CompletionResult {
                path,
                version,
                items: labels.iter().map(|l| item(l)).collect(),
            },
        );
    }
    fn visibles(s: &State) -> Vec<&str> {
        visible_completions(s)
            .iter()
            .map(|i| i.label.as_str())
            .collect()
    }
    fn texte(s: &State) -> String {
        s.doc.rope().to_string()
    }
    /// Liste ouverte sur « pr » tapé en fin de `avant`.
    fn liste(avant: &str) -> State {
        let mut s = ouvert("/p/a.rs", avant);
        update(&mut s, k(Key::End));
        tape(&mut s, "pr");
        let v = s.doc.version();
        reponse(&mut s, v, &["print", "println!", "sprintf", "push"]);
        s
    }
    fn pos(line: usize, character: usize) -> Position {
        Position { line, character }
    }
    fn diag(line: usize, character: usize, severity: Severity, message: &str) -> Diagnostic {
        Diagnostic {
            start: pos(line, character),
            end: pos(line, character + 1),
            severity,
            message: message.into(),
            source: None,
        }
    }

    #[test]
    fn ctrl_espace_demande_a_la_position_utf16() {
        let mut s = ouvert("/p/a.rs", "x\nlet 😀 = ab");
        update(&mut s, k(Key::Down));
        update(&mut s, k(Key::End));
        let cmds = update(&mut s, touche(Key::Char(' '), true, false));
        assert_eq!(
            cmds,
            [Command::RequestCompletion {
                path: "/p/a.rs".into(),
                version: s.doc.version(),
                // "let " 4 + 😀 2 + " = " 3 + "ab" 2.
                pos: pos(1, 11),
            }]
        );
        // Sans chemin : rien.
        let mut s = State::default();
        assert!(update(&mut s, touche(Key::Char(' '), true, false)).is_empty());
    }

    #[test]
    fn frappe_demande_puis_filtre_localement() {
        let mut s = ouvert("/p/a.rs", "");
        let cmds = tape(&mut s, "p");
        let v = s.doc.version();
        assert_eq!(
            cmds,
            [Command::RequestCompletion {
                path: "/p/a.rs".into(),
                version: v,
                pos: pos(0, 1),
            }]
        );
        reponse(&mut s, v, &["format", "sprintf", "print", "push"]);
        assert_eq!(s.completion.as_ref().map(|c| c.anchor), Some(0));
        assert_eq!(visibles(&s), ["print", "push", "sprintf"]);
        // Liste ouverte : pas de nouvelle requête, filtrage (préfixes d'abord, insensible à la casse).
        assert!(tape(&mut s, "R").is_empty());
        assert_eq!(visibles(&s), ["print", "sprintf"]);
        // Plus rien de visible → fermée.
        tape(&mut s, "zz");
        assert!(s.completion.is_none());
    }

    #[test]
    fn reponse_perimee_ignoree() {
        let mut s = ouvert("/p/a.rs", "");
        tape(&mut s, "p");
        let v = s.doc.version();
        tape(&mut s, "r");
        reponse(&mut s, v, &["print"]);
        assert!(s.completion.is_none());
        // Liste vide → fermée.
        let v = s.doc.version();
        reponse(&mut s, v, &[]);
        assert!(s.completion.is_none());
        // Curseur sorti du mot entre la requête et la réponse.
        tape(&mut s, " i");
        let v = s.doc.version();
        update(&mut s, k(Key::Home));
        reponse(&mut s, v, &["print"]);
        assert!(s.completion.is_none());
    }

    #[test]
    fn haut_bas_echap_entree_tab() {
        let mut s = liste("");
        assert_eq!(visibles(&s), ["print", "println!", "sprintf"]);
        let sel = |s: &State| s.completion.as_ref().map(|c| c.selected);
        update(&mut s, k(Key::Up));
        assert_eq!(sel(&s), Some(2));
        update(&mut s, k(Key::Down));
        update(&mut s, k(Key::Down));
        assert_eq!(sel(&s), Some(1));
        update(&mut s, k(Key::Tab));
        assert_eq!(texte(&s), "println!");
        assert!(s.completion.is_none());

        let mut s = liste("x = ");
        update(&mut s, k(Key::Down));
        update(&mut s, k(Key::Down));
        update(&mut s, k(Key::Enter));
        assert_eq!(texte(&s), "x = sprintf");
        assert_eq!(s.doc.cursor(), 11);
        // Un seul Ctrl+Z revient au mot tapé.
        update(&mut s, touche(Key::Char('z'), true, false));
        assert_eq!(texte(&s), "x = pr");

        let mut s = liste("");
        update(&mut s, k(Key::Esc));
        assert!(s.completion.is_none());
        assert_eq!(texte(&s), "pr");
        // Échap a fermé la liste : Entrée revient à l'éditeur.
        update(&mut s, k(Key::Enter));
        assert_eq!(texte(&s), "pr\n");
    }

    #[test]
    fn fermee_quand_le_curseur_sort_du_mot_ou_l_onglet_change() {
        let mut s = liste("ab ");
        update(&mut s, k(Key::Left));
        update(&mut s, k(Key::Left));
        assert!(
            s.completion.is_some(),
            "curseur sur l'ancre : encore dans le mot"
        );
        update(&mut s, k(Key::Left));
        assert!(s.completion.is_none());

        let mut s = liste("ab ");
        update(&mut s, k(Key::Backspace));
        update(&mut s, k(Key::Backspace));
        assert!(s.completion.is_some());
        update(&mut s, k(Key::Backspace));
        assert!(s.completion.is_none());

        // Haut/Bas sont pris par la liste ; Page suiv. change de ligne.
        let mut s = liste("\n");
        update(&mut s, k(Key::PageDown));
        assert!(s.completion.is_none());

        let mut s = ouvert("/p/b.rs", "");
        ouvrir(&mut s, "/p/a.rs", "");
        tape(&mut s, "pr");
        let v = s.doc.version();
        reponse(&mut s, v, &["print"]);
        assert!(s.completion.is_some());
        update(&mut s, touche(Key::PageUp, true, false));
        assert_eq!(s.doc.display_name(), "b.rs");
        assert!(s.completion.is_none());
    }

    #[test]
    fn point_et_deux_points_declenchent() {
        let mut s = ouvert("/p/a.rs", "");
        tape(&mut s, "a");
        let cmds = tape(&mut s, ".");
        assert!(
            matches!(&cmds[..], [Command::RequestCompletion { pos: p, .. }] if *p == pos(0, 2))
        );
        assert_eq!(s.lsp_pending.completion_anchor, Some(2));
        let v = s.doc.version();
        reponse(&mut s, v, &["len"]);
        assert_eq!(visibles(&s), ["len"]);
        // Espace : sort du mot, pas de requête.
        assert!(tape(&mut s, " ").is_empty());
        assert!(s.completion.is_none());
        assert!(tape(&mut s, ":").is_empty());
        assert_eq!(tape(&mut s, ":").len(), 1);
    }

    #[test]
    fn survol_affiche_puis_ferme() {
        let mut s = ouvert("/p/a.rs", "abc");
        update(&mut s, k(Key::Right));
        let cmds = update(&mut s, touche(Key::Char('k'), true, false));
        assert_eq!(
            cmds,
            [Command::RequestHover {
                path: "/p/a.rs".into(),
                pos: pos(0, 1),
            }]
        );
        let hover = |s: &mut State| {
            update(
                s,
                Event::HoverResult {
                    path: "/p/a.rs".into(),
                    text: Some("fn abc()".into()),
                },
            )
        };
        hover(&mut s);
        assert_eq!(
            s.hover,
            Some(Hover {
                text: "fn abc()".into(),
                at: 1
            })
        );
        update(&mut s, k(Key::Right));
        assert!(s.hover.is_none());

        // Échap ferme sans rien faire d'autre.
        update(&mut s, touche(Key::Char('k'), true, false));
        hover(&mut s);
        assert!(s.hover.is_some());
        update(&mut s, k(Key::Esc));
        assert!(s.hover.is_none());

        // Curseur déplacé avant la réponse : ignorée.
        update(&mut s, touche(Key::Char('k'), true, false));
        update(&mut s, k(Key::Left));
        hover(&mut s);
        assert!(s.hover.is_none());
    }

    #[test]
    fn f12_dans_le_meme_fichier() {
        let mut s = ouvert("/p/a.rs", "fn a() {}\na();");
        update(&mut s, k(Key::Down));
        assert_eq!(
            update(&mut s, k(Key::F(12))),
            [Command::RequestDefinition {
                path: "/p/a.rs".into(),
                pos: pos(1, 0),
            }]
        );
        let loc = Location {
            path: "/p/a.rs".into(),
            start: pos(0, 3),
            end: pos(0, 4),
        };
        assert!(update(&mut s, Event::DefinitionResult(vec![loc])).is_empty());
        assert_eq!(s.doc.cursor(), 3);
    }

    #[test]
    fn f12_vers_un_autre_fichier() {
        let mut s = ouvert("/p/a.rs", "a");
        let loc = Location {
            path: "/p/b.rs".into(),
            start: pos(1, 2),
            end: pos(1, 3),
        };
        assert_eq!(
            update(&mut s, Event::DefinitionResult(vec![loc.clone()])),
            [Command::OpenFile("/p/b.rs".into())]
        );
        ouvrir(&mut s, "/p/b.rs", "x\nabcd");
        assert_eq!(s.doc.display_name(), "b.rs");
        assert_eq!(s.doc.cursor(), 4);
        assert!(s.pending_jump.is_none());

        // Onglet déjà ouvert : FileOpened l'active et le saut s'applique aussi.
        update(&mut s, touche(Key::PageUp, true, false));
        assert_eq!(s.doc.display_name(), "a.rs");
        update(&mut s, Event::DefinitionResult(vec![loc]));
        ouvrir(&mut s, "/p/b.rs", "x\nabcd");
        assert_eq!((s.doc.display_name().as_str(), s.doc.cursor()), ("b.rs", 4));
    }

    #[test]
    fn aucune_definition() {
        let mut s = ouvert("/p/a.rs", "a");
        assert!(update(&mut s, Event::DefinitionResult(vec![])).is_empty());
        assert_eq!(s.status, "Aucune définition");
    }

    #[test]
    fn ctrl_shift_m_reduit_l_editeur() {
        let mut s = ouvert("/p/a.rs", "a");
        let h = editor_size(&s).1;
        update(&mut s, touche(Key::Char('M'), true, true));
        assert!(s.problems_open);
        assert_eq!(editor_size(&s).1, h - PROBLEMS_HEIGHT);
        update(&mut s, touche(Key::Char('m'), true, true));
        assert!(!s.problems_open);
        assert_eq!(editor_size(&s).1, h);
    }

    #[test]
    fn f8_boucle_sur_les_diagnostics() {
        let mut s = ouvert("/p/a.rs", "abc\ndef");
        update(
            &mut s,
            Event::Diagnostics {
                path: "/p/a.rs".into(),
                items: vec![
                    diag(1, 1, Severity::Warning, "deux"),
                    diag(0, 1, Severity::Error, "un"),
                ],
            },
        );
        let f8 = |s: &mut State, shift| {
            update(s, touche(Key::F(8), false, shift));
            (s.doc.cursor(), s.status.clone())
        };
        assert_eq!(f8(&mut s, false), (1, "un".into()));
        assert_eq!(f8(&mut s, false), (5, "deux".into()));
        assert_eq!(f8(&mut s, false), (1, "un".into()));
        assert_eq!(f8(&mut s, true), (5, "deux".into()));
        assert_eq!(f8(&mut s, true), (1, "un".into()));

        let mut s = ouvert("/p/b.rs", "");
        f8(&mut s, false);
        assert_eq!(s.status, "Aucun problème");
    }

    #[test]
    fn compteurs_et_problemes_tries() {
        let mut s = State::default();
        for (path, items) in [
            (
                "/p/b.rs",
                vec![
                    diag(3, 0, Severity::Warning, "b3"),
                    diag(9, 0, Severity::Error, "b9"),
                    diag(1, 0, Severity::Error, "b1"),
                ],
            ),
            (
                "/p/a.rs",
                vec![
                    diag(0, 0, Severity::Hint, "a0"),
                    diag(5, 0, Severity::Warning, "a5"),
                ],
            ),
        ] {
            update(
                &mut s,
                Event::Diagnostics {
                    path: path.into(),
                    items,
                },
            );
        }
        assert_eq!(diagnostic_counts(&s), (2, 2));
        let ordre: Vec<_> = problems(&s).into_iter().map(|(_, d)| d.message).collect();
        assert_eq!(ordre, ["a5", "a0", "b1", "b9", "b3"]);
    }
}
