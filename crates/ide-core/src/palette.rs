//! Palettes (F-50, F-51, F-52) : ouvrir un fichier par recherche floue (Ctrl+P), lancer une commande
//! (Ctrl+Shift+P) et chercher du texte dans le projet (Ctrl+Shift+F, Alt+F). Une commande de la palette
//! rejoue simplement son raccourci clavier.

use std::path::Path;

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::{Command, Event, Focus, Key, KeyInput, Location, Modifiers, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    Files,
    Commands,
    Text,
}

/// Palette ouverte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub kind: PaletteKind,
    pub query: String,
    /// Index dans `palette_items(state)`.
    pub selected: usize,
    /// Classement mis en cache (index dans `project_files` ou `commands()`), recalculé seulement quand la
    /// requête, le type ou la liste des fichiers changent : pas de reclassement complet à chaque rendu.
    pub ranked: Vec<usize>,
}

/// Ligne affichée : libellé (nom de fichier ou commande) et détail (dossier relatif ou raccourci).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub label: String,
    pub detail: String,
}

/// Ligne du projet où le texte cherché apparaît (sa première occurrence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextHit {
    pub location: Location,
    /// Extrait de la ligne, déjà borné et sans caractère de contrôle.
    pub excerpt: String,
}

/// Recherche dans le projet (F-52).
/// shortcut: elle lit le disque, pas les onglets modifiés ; dans l'un d'eux, un résultat peut viser une ligne
/// qui a bougé (`char_at_lsp` borne sans erreur). Passer le texte des onglets à la commande si ça gêne.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSearch {
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    /// Numéro de la dernière recherche lancée : les réponses d'un autre numéro sont périmées.
    pub generation: u64,
    /// Réponse attendue ; les résultats précédents restent affichés en attendant.
    pub searching: bool,
    pub hits: Vec<TextHit>,
    /// Plafond atteint : d'autres résultats existent.
    pub truncated: bool,
    /// Regex invalide : message affichable.
    pub error: Option<String>,
}

/// Action de la palette de commandes ; `name` est aussi celui de `[keys]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    pub name: &'static str,
    pub label: &'static str,
    /// Raccourci rejoué quand on choisit la commande.
    pub key: KeyInput,
}

const MAX_ITEMS: usize = 50;
const PAGE: usize = 10;

const fn with(key: Key, ctrl: bool, shift: bool) -> KeyInput {
    KeyInput {
        key,
        mods: Modifiers {
            ctrl,
            alt: false,
            shift,
        },
    }
}

const fn ctrl(c: char) -> KeyInput {
    with(Key::Char(c), true, false)
}

const fn alt(c: char) -> KeyInput {
    KeyInput {
        key: Key::Char(c),
        mods: Modifiers {
            ctrl: false,
            alt: true,
            shift: false,
        },
    }
}

const fn cmd(name: &'static str, label: &'static str, key: KeyInput) -> CommandInfo {
    CommandInfo { name, label, key }
}

static COMMANDS: &[CommandInfo] = &[
    cmd(
        "command_palette",
        "Palette de commandes",
        with(Key::Char('P'), true, true),
    ),
    cmd(
        "keyboard_shortcuts",
        "Aide : raccourcis clavier",
        with(Key::F(1), false, false),
    ),
    cmd(
        "open_file_picker",
        "Fichier : ouvrir un fichier du projet",
        ctrl('p'),
    ),
    cmd("save", "Fichier : sauvegarder", ctrl('s')),
    cmd("close_tab", "Fichier : fermer l'onglet", ctrl('w')),
    cmd("quit", "Fichier : quitter", ctrl('q')),
    cmd("next_tab", "Onglets : suivant", with(Key::Tab, true, false)),
    cmd(
        "previous_tab",
        "Onglets : précédent",
        with(Key::Tab, true, true),
    ),
    cmd(
        "toggle_explorer",
        "Explorateur : afficher / masquer",
        ctrl('b'),
    ),
    cmd(
        "focus_explorer",
        "Explorateur : basculer le focus",
        ctrl('e'),
    ),
    cmd("undo", "Édition : annuler", ctrl('z')),
    cmd("redo", "Édition : rétablir", ctrl('y')),
    cmd("select_all", "Édition : tout sélectionner", ctrl('a')),
    cmd("copy", "Édition : copier", ctrl('c')),
    cmd("cut", "Édition : couper", ctrl('x')),
    cmd("paste", "Édition : coller", ctrl('v')),
    cmd("find", "Recherche : rechercher", ctrl('f')),
    cmd("replace", "Recherche : remplacer", ctrl('h')),
    cmd(
        "project_search",
        "Recherche : rechercher dans le projet",
        alt('f'),
    ),
    cmd("trigger_completion", "Code : autocomplétion", ctrl(' ')),
    cmd("hover", "Code : afficher l'infobulle", ctrl('k')),
    cmd(
        "toggle_comment",
        "Code : commenter / décommenter",
        ctrl(':'),
    ),
    cmd(
        "goto_definition",
        "Code : aller à la définition",
        with(Key::F(12), false, false),
    ),
    cmd(
        "format_document",
        "Code : mettre en page",
        KeyInput {
            key: Key::Char('F'),
            mods: Modifiers {
                ctrl: false,
                alt: true,
                shift: true,
            },
        },
    ),
    cmd(
        "toggle_line_numbers",
        "Affichage : afficher / masquer les numéros de ligne",
        alt('l'),
    ),
    cmd("next_theme", "Affichage : changer de thème", alt('t')),
    cmd("fold", "Code : replier le bloc", alt('-')),
    cmd("unfold", "Code : déplier le bloc", alt('=')),
    cmd("fold_all", "Code : tout replier", alt('_')),
    cmd("unfold_all", "Code : tout déplier", alt('+')),
    cmd(
        "next_diagnostic",
        "Problèmes : suivant",
        with(Key::F(8), false, false),
    ),
    cmd(
        "previous_diagnostic",
        "Problèmes : précédent",
        with(Key::F(8), false, true),
    ),
    cmd(
        "toggle_problems",
        "Problèmes : afficher / masquer le panneau",
        with(Key::Char('m'), true, true),
    ),
    cmd(
        "toggle_terminal",
        "Terminal : ouvrir / aller / fermer",
        ctrl('ù'),
    ),
];

/// Écran d'aide (F1) : groupes de (touches, action). Touches séparées par « / » (un test vérifie que
/// chaque commande de `COMMANDS` y figure).
pub static SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Fichiers",
        &[
            ("Ctrl+P", "Ouvrir un fichier du projet"),
            ("Ctrl+Shift+P", "Palette de commandes (ou Ctrl+P puis >)"),
            ("Ctrl+S", "Sauvegarder"),
            ("Ctrl+Q", "Quitter (deux fois : sans sauvegarder)"),
            (
                "Alt+R / Alt+K",
                "Fichier changé sur le disque : recharger / garder",
            ),
            ("Alt+B / Alt+N", "Copie de secours : restaurer / ignorer"),
        ],
    ),
    (
        "Édition",
        &[
            ("Ctrl+Z / Ctrl+Y", "Annuler / rétablir"),
            ("Ctrl+: / Alt+:", "Commenter / décommenter les lignes"),
            (
                "Ctrl+C / Ctrl+X / Ctrl+V",
                "Copier / couper / coller (sans sélection : la ligne)",
            ),
            ("Ctrl+A", "Tout sélectionner"),
            (
                "Ctrl+Gauche / Ctrl+Droite",
                "Mot précédent / suivant (Shift : sélectionner)",
            ),
            ("Ctrl+Début / Ctrl+Fin", "Début / fin du fichier"),
            ("Alt+Retour arrière", "Effacer le mot précédent"),
            ("Tab / Shift+Tab", "Indenter / désindenter"),
        ],
    ),
    (
        "Recherche",
        &[
            ("Ctrl+F / Ctrl+H", "Rechercher / remplacer"),
            ("Ctrl+Shift+F / Alt+F", "Rechercher dans le projet"),
            (
                "F3 / Shift+F3",
                "Occurrence suivante / précédente (aussi Entrée)",
            ),
            ("Alt+Entrée", "Tout remplacer"),
            ("Alt+R / Alt+C", "Expression régulière / respecter la casse"),
            ("Échap", "Fermer la barre"),
        ],
    ),
    (
        "Repliage",
        &[
            ("Alt+- / Alt+=", "Replier / déplier le bloc du curseur"),
            ("Alt+_ / Alt++", "Tout replier / tout déplier"),
        ],
    ),
    (
        "Affichage",
        &[
            ("Alt+L", "Afficher / masquer les numéros de ligne"),
            ("Alt+T", "Thème suivant"),
        ],
    ),
    (
        "Code / LSP",
        &[
            ("Ctrl+Espace", "Complétion"),
            ("Ctrl+K", "Infobulle"),
            ("F12", "Aller à la définition"),
            (
                "Alt+Shift+F",
                "Mettre en page (aussi Ctrl+0 selon le terminal)",
            ),
            ("F8 / Shift+F8", "Problème suivant / précédent"),
        ],
    ),
    (
        "Onglets",
        &[
            ("Ctrl+W", "Fermer l'onglet (deux fois s'il est modifié)"),
            (
                "Ctrl+Page suiv. / Ctrl+Page préc.",
                "Onglet suivant / précédent",
            ),
            ("Ctrl+Tab / Ctrl+Shift+Tab", "Onglet suivant / précédent"),
            ("Alt+1 / … / Alt+9", "Aller à l'onglet (Alt+9 : le dernier)"),
        ],
    ),
    (
        "Explorateur",
        &[
            ("Ctrl+B", "Afficher / masquer l'explorateur"),
            ("Ctrl+E", "Basculer le focus explorateur / code"),
            ("Échap", "Revenir au code"),
            ("Entrée", "Ouvrir le fichier, déplier le dossier"),
            ("n / Shift+N", "Nouveau fichier / dossier"),
            ("F2 / r", "Renommer"),
            ("m", "Déplacer"),
            ("Suppr", "Mettre à la corbeille"),
            (".", "Afficher / masquer les fichiers cachés"),
            ("< / >", "Réduire / élargir"),
        ],
    ),
    (
        "Panneaux",
        &[
            ("Ctrl+Shift+M", "Panneau Problèmes"),
            ("Ctrl+Ù / Alt+J", "Terminal intégré"),
            ("Alt+T", "Terminal, avec le focus : nouvel onglet"),
            (
                "Alt+Page suiv. / Alt+Page préc.",
                "Terminal, avec le focus : onglet suivant / précédent",
            ),
            ("F1", "Cet écran d'aide"),
        ],
    ),
];

/// Lignes de l'écran d'aide : par groupe, son titre puis ses raccourcis ; une ligne vide entre groupes.
pub fn help_len() -> usize {
    SHORTCUTS.iter().map(|(_, v)| v.len() + 2).sum::<usize>() - 1
}

/// Toutes les commandes, dans l'ordre affiché pour une requête vide.
pub fn commands() -> &'static [CommandInfo] {
    COMMANDS
}

/// Commande par son identifiant (`[keys]`).
pub fn command_by_name(name: &str) -> Option<&'static CommandInfo> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// Score flou nucleo (réglé pour les chemins) ; `None` = pas de correspondance.
struct Fuzzy {
    pattern: Pattern,
    matcher: Matcher,
    buf: Vec<char>,
}

impl Fuzzy {
    fn new(query: &str) -> Self {
        Self {
            pattern: Pattern::new(
                query,
                CaseMatching::Smart,
                Normalization::Smart,
                AtomKind::Fuzzy,
            ),
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            buf: Vec::new(),
        }
    }

    fn score(&mut self, text: &str) -> Option<u32> {
        self.pattern
            .score(Utf32Str::new(text, &mut self.buf), &mut self.matcher)
    }
}

/// Chemin relatif à la racine de l'explorateur (tel quel s'il est ailleurs).
fn relative<'a>(state: &State, path: &'a Path) -> &'a Path {
    state
        .explorer
        .root
        .as_deref()
        .and_then(|r| path.strip_prefix(r).ok())
        .unwrap_or(path)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Index triés dans `project_files` ou `commands()`, au plus `MAX_ITEMS` ; pour `Text`, tous les résultats,
/// déjà triés et plafonnés par la recherche.
fn ranked(state: &State, p: &Palette) -> Vec<usize> {
    let mut fz = Fuzzy::new(&p.query);
    // (score, clé de départage, index)
    let mut v: Vec<(u32, String, usize)> = match p.kind {
        PaletteKind::Files if p.query.is_empty() => {
            // Onglets ouverts d'abord, dans l'ordre de la barre, puis ordre alphabétique.
            let mut open: Vec<_> = state.tabs.iter().map(|t| t.doc.path.as_deref()).collect();
            open.insert(state.active.min(open.len()), state.doc.path.as_deref());
            state
                .project_files
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let rank = open.iter().position(|o| *o == Some(f.as_path()));
                    let rank = open.len() - rank.unwrap_or(open.len());
                    (rank as u32, relative(state, f).to_string_lossy().into(), i)
                })
                .collect()
        }
        PaletteKind::Files => state
            .project_files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let rel = relative(state, f).to_string_lossy().into_owned();
                // Bonus au nom de fichier : « main » préfère `src/main.rs` à `main/a.rs`.
                let s = fz.score(&rel)? + fz.score(&file_name(f)).unwrap_or(0);
                Some((s, rel, i))
            })
            .collect(),
        PaletteKind::Commands => COMMANDS
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((fz.score(c.label).max(fz.score(c.name))?, String::new(), i)))
            .collect(),
        PaletteKind::Text => return (0..state.project_search.hits.len()).collect(),
    };
    v.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    v.into_iter().take(MAX_ITEMS).map(|(.., i)| i).collect()
}

/// Éléments filtrés et triés par pertinence pour la requête, au plus 50 (hors résultats de `Text`).
pub fn palette_items(state: &State) -> Vec<PaletteItem> {
    let Some(p) = &state.palette else {
        return vec![];
    };
    let ids = p.ranked.iter().copied();
    match p.kind {
        PaletteKind::Files => ids
            .map(|i| {
                let f = &state.project_files[i];
                PaletteItem {
                    label: file_name(f),
                    detail: relative(state, f)
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default(),
                }
            })
            .collect(),
        PaletteKind::Commands => ids
            .map(|i| PaletteItem {
                label: COMMANDS[i].label.into(),
                detail: COMMANDS[i].key.to_string(),
            })
            .collect(),
        PaletteKind::Text => ids
            .map(|i| {
                let hit = &state.project_search.hits[i];
                let path = relative(state, &hit.location.path).display();
                let line = hit.location.start.line + 1;
                PaletteItem {
                    label: format!("{path}:{line}: {}", hit.excerpt),
                    detail: String::new(),
                }
            })
            .collect(),
    }
}

/// Ouvre (ou bascule vers) une palette. La liste des fichiers est redemandée à chaque ouverture (fichiers créés
/// ou supprimés entre-temps) ; l'ancienne reste affichée en attendant, sauf si elle vient d'une autre racine.
/// La recherche dans le projet reprend sa requête et ses résultats, relancée pour la même raison.
fn open(state: &mut State, kind: PaletteKind) -> Vec<Command> {
    let mut cmds = vec![];
    if kind != PaletteKind::Commands && state.explorer.root.is_none() {
        state.status = "Aucun dossier ouvert".into();
        return cmds;
    }
    if let (PaletteKind::Files, Some(root)) = (kind, state.explorer.root.clone()) {
        if !state
            .project_files
            .first()
            .is_some_and(|f| f.starts_with(&root))
        {
            state.project_files.clear();
        }
        cmds.push(Command::ListFiles {
            root,
            show_hidden: state.settings.show_hidden,
        });
    }
    let p = state.palette.get_or_insert_with(|| Palette {
        kind,
        query: String::new(),
        selected: 0,
        ranked: vec![],
    });
    p.kind = kind;
    p.selected = 0;
    if kind == PaletteKind::Text {
        p.query.clone_from(&state.project_search.query);
        return query_changed(state);
    }
    refresh(state);
    cmds
}

/// Requête ou option changée : nouveau classement et, en palette `Text`, nouvelle recherche.
fn query_changed(state: &mut State) -> Vec<Command> {
    let cmds = search_text(state);
    refresh(state);
    cmds
}

/// Lance la recherche dans le projet, ce qui rend périmée toute réponse en vol. Requête vide : liste vide.
fn search_text(state: &mut State) -> Vec<Command> {
    let (Some(p), Some(root)) = (&state.palette, &state.explorer.root) else {
        return vec![];
    };
    if p.kind != PaletteKind::Text {
        return vec![];
    }
    let s = &mut state.project_search;
    s.query.clone_from(&p.query);
    s.generation += 1;
    s.error = None;
    s.searching = !s.query.is_empty();
    if !s.searching {
        s.hits.clear();
        s.truncated = false;
        return vec![];
    }
    vec![Command::SearchText {
        root: root.clone(),
        show_hidden: state.settings.show_hidden,
        query: s.query.clone(),
        regex: s.regex,
        case_sensitive: s.case_sensitive,
        generation: s.generation,
    }]
}

/// Résultats d'une recherche, ignorés si une autre a été lancée depuis.
pub(crate) fn text_results(
    state: &mut State,
    generation: u64,
    result: Result<(Vec<TextHit>, bool), String>,
) {
    let s = &mut state.project_search;
    if generation != s.generation {
        return;
    }
    s.searching = false;
    (s.hits, s.truncated, s.error) = match result {
        Ok((hits, truncated)) => (hits, truncated, None),
        Err(e) => (vec![], false, Some(e)),
    };
    refresh(state);
}

/// Entrée : ferme la palette, puis ouvre le fichier, rejoue le raccourci de la commande ou va au résultat.
fn accept(state: &mut State) -> Vec<Command> {
    let Some(p) = state.palette.take() else {
        return vec![];
    };
    let Some(&i) = p.ranked.get(p.selected) else {
        return vec![];
    };
    match p.kind {
        PaletteKind::Files => vec![Command::OpenFile(state.project_files[i].clone())],
        // Palette déjà fermée : la touche rejouée ne revient pas ici.
        PaletteKind::Commands => {
            state.replaying = true;
            crate::update(state, Event::Key(COMMANDS[i].key))
        }
        PaletteKind::Text => {
            // Lancée depuis l'explorateur, la saisie doit reprendre sur le résultat.
            state.focus = Focus::Editor;
            let location = state.project_search.hits[i].location.clone();
            crate::lsp::jump(state, location)
        }
    }
}

/// Touches : Ctrl+P / Ctrl+Shift+P / Ctrl+Shift+F / Alt+F ouvrent ; palette ouverte, toutes les touches lui reviennent
/// (saisie, Haut/Bas, Entrée, Échap), sauf Ctrl+Q. `None` = touche laissée à la suite.
pub(crate) fn key(state: &mut State, k: KeyInput) -> Option<Vec<Command>> {
    let (ctrl, alt, shift) = (k.mods.ctrl, k.mods.alt, k.mods.shift);
    // Selon le terminal : `P` (avec ou sans Shift) ou `p` + Shift.
    if let Key::Char(c @ ('p' | 'P')) = k.key
        && ctrl
        && !alt
    {
        let kind = if shift || c == 'P' {
            PaletteKind::Commands
        } else {
            PaletteKind::Files
        };
        return Some(open(state, kind));
    }
    // Avant l'éditeur, qui prendrait Ctrl+Shift+F pour Ctrl+F. Alt+F : repli hors protocole Kitty, où Shift
    // n'arrive pas avec Ctrl. Alt avec `F` ou Shift reste la mise en page (`lsp::key`).
    if let Key::Char(c @ ('f' | 'F')) = k.key
        && ((ctrl && !alt && (shift || c == 'F')) || (alt && !ctrl && !shift && c == 'f'))
    {
        return Some(open(state, PaletteKind::Text));
    }
    let n = state.palette.as_ref()?.ranked.len().max(1);
    let p = state.palette.as_mut()?;
    match k.key {
        Key::Esc => state.palette = None,
        Key::Enter => return Some(accept(state)),
        Key::Up => p.selected = (p.selected + n - 1) % n,
        Key::Down => p.selected = (p.selected + 1) % n,
        Key::PageUp => p.selected = p.selected.saturating_sub(PAGE),
        Key::PageDown => p.selected = (p.selected + PAGE).min(n - 1),
        Key::Backspace => {
            p.query.pop();
            p.selected = 0;
            return Some(query_changed(state));
        }
        // `>` en tête (VSCode) : commandes, joignables même quand le terminal confond Ctrl+Shift+P et Ctrl+P.
        Key::Char('>') if p.kind == PaletteKind::Files && p.query.is_empty() && !ctrl && !alt => {
            p.kind = PaletteKind::Commands;
            refresh(state);
        }
        Key::Char(c) if !ctrl && !alt && !c.is_control() => {
            p.query.push(c);
            p.selected = 0;
            return Some(query_changed(state));
        }
        Key::Char('r' | 'R') if alt && !ctrl && p.kind == PaletteKind::Text => {
            state.project_search.regex = !state.project_search.regex;
            return Some(query_changed(state));
        }
        Key::Char('c' | 'C') if alt && !ctrl && p.kind == PaletteKind::Text => {
            state.project_search.case_sensitive = !state.project_search.case_sensitive;
            return Some(query_changed(state));
        }
        Key::Char('q' | 'Q') if ctrl && !alt => return None,
        _ => {}
    }
    Some(vec![])
}

/// Collage dans la requête : première ligne seulement.
pub(crate) fn paste(state: &mut State, text: &str) -> Vec<Command> {
    if let Some(p) = &mut state.palette {
        p.query.push_str(text.lines().next().unwrap_or_default());
        p.selected = 0;
    }
    query_changed(state)
}

/// Recalcule le classement de la palette ouverte (requête, type ou liste des fichiers changés).
pub(crate) fn refresh(state: &mut State) {
    if let Some(p) = &state.palette {
        let r = ranked(state, p);
        if let Some(p) = &mut state.palette {
            p.ranked = r;
            p.selected = p.selected.min(p.ranked.len().saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn touche(key: Key, ctrl: bool, shift: bool) -> Event {
        Event::Key(with(key, ctrl, shift))
    }
    fn tape(s: &mut State, texte: &str) {
        for c in texte.chars() {
            update(s, touche(Key::Char(c), false, false));
        }
    }
    fn liste(files: &[&str]) -> Event {
        Event::FileList(files.iter().map(|f| PathBuf::from("/p").join(f)).collect())
    }
    /// Racine `/p` et ses fichiers, palette fermée.
    fn projet(files: &[&str]) -> State {
        let mut s = State::default();
        s.explorer.root = Some("/p".into());
        update(&mut s, liste(files));
        s
    }
    fn labels(s: &State) -> Vec<String> {
        palette_items(s).into_iter().map(|i| i.label).collect()
    }
    fn ouvrir_fichier(s: &mut State, path: &str, text: &str) {
        update(
            s,
            Event::FileOpened {
                path: path.into(),
                text: text.into(),
            },
        );
    }

    #[test]
    fn ctrl_p_demande_la_liste_si_absente_ou_d_une_autre_racine() {
        let mut s = projet(&[]);
        let attendu = vec![Command::ListFiles {
            root: "/p".into(),
            show_hidden: false,
        }];
        assert_eq!(update(&mut s, touche(Key::Char('p'), true, false)), attendu);
        assert_eq!(s.palette.as_ref().map(|p| p.kind), Some(PaletteKind::Files));
        // Liste arrivée après l'ouverture : l'affichage suit.
        assert!(labels(&s).is_empty());
        update(&mut s, liste(&["a.rs"]));
        assert_eq!(labels(&s), ["a.rs"]);
        // Réouverture : liste redemandée (fichiers créés entre-temps), l'ancienne reste affichée en attendant.
        update(&mut s, touche(Key::Esc, false, false));
        assert_eq!(update(&mut s, touche(Key::Char('p'), true, false)), attendu);
        assert_eq!(labels(&s), ["a.rs"]);
        // Racine changée : liste périmée.
        s.explorer.root = Some("/q".into());
        assert_eq!(
            update(&mut s, touche(Key::Char('p'), true, false)),
            vec![Command::ListFiles {
                root: "/q".into(),
                show_hidden: false,
            }]
        );
    }

    #[test]
    fn commande_rejouee_sans_detour_par_keys() {
        // `"ctrl+s" = "quit"` : la commande « sauvegarder » de la palette sauvegarde quand même.
        let mut s = State::default();
        s.settings.format_on_save = false;
        s.doc.path = Some("/p/a.rs".into());
        s.settings
            .keys
            .push((KeyInput::parse("ctrl+s").expect("ctrl+s"), "quit".into()));
        update(&mut s, touche(Key::Char('P'), true, true));
        for c in "sauvegarder".chars() {
            update(&mut s, touche(Key::Char(c), false, false));
        }
        let cmds = update(&mut s, touche(Key::Enter, false, false));
        assert!(
            matches!(cmds.as_slice(), [Command::Save { .. }]),
            "{cmds:?}"
        );
        // La touche tapée, elle, reste détournée.
        assert_eq!(
            update(&mut s, touche(Key::Char('s'), true, false)),
            vec![Command::Quit]
        );
    }

    #[test]
    fn sans_dossier_ouvert() {
        let mut s = State::default();
        assert!(update(&mut s, touche(Key::Char('p'), true, false)).is_empty());
        assert_eq!(s.status, "Aucun dossier ouvert");
        assert!(s.palette.is_none());
    }

    #[test]
    fn recherche_floue_sur_le_chemin() {
        let mut s = projet(&[
            "README.md",
            "crates/ide-core/src/lib.rs",
            "crates/ide-ui/src/main.rs",
        ]);
        update(&mut s, touche(Key::Char('p'), true, false));
        tape(&mut s, "corelib");
        assert_eq!(
            palette_items(&s),
            [PaletteItem {
                label: "lib.rs".into(),
                detail: "crates/ide-core/src".into(),
            }]
        );
        // Effacer élargit de nouveau.
        for _ in 0..4 {
            update(&mut s, touche(Key::Backspace, false, false));
        }
        assert_eq!(s.palette.as_ref().map(|p| p.query.as_str()), Some("cor"));
        assert_eq!(labels(&s)[0], "lib.rs");
    }

    #[test]
    fn nom_de_fichier_prioritaire() {
        let mut s = projet(&["main/a.rs", "src/main.rs"]);
        update(&mut s, touche(Key::Char('p'), true, false));
        tape(&mut s, "main");
        assert_eq!(labels(&s), ["main.rs", "a.rs"]);
    }

    #[test]
    fn requete_vide_onglets_ouverts_d_abord() {
        let mut s = projet(&["a.rs", "b.rs", "c.rs", "d.rs"]);
        ouvrir_fichier(&mut s, "/p/d.rs", "");
        ouvrir_fichier(&mut s, "/p/b.rs", "");
        update(&mut s, touche(Key::Char('p'), true, false));
        assert_eq!(labels(&s), ["d.rs", "b.rs", "a.rs", "c.rs"]);
        let files: Vec<String> = (0..80).map(|i| format!("f{i}.rs")).collect();
        update(
            &mut s,
            liste(&files.iter().map(String::as_str).collect::<Vec<_>>()),
        );
        assert_eq!(palette_items(&s).len(), MAX_ITEMS);
    }

    #[test]
    fn entree_ouvre_le_fichier_et_ferme() {
        let mut s = projet(&["a.rs", "b.rs", "c.rs"]);
        update(&mut s, touche(Key::Char('p'), true, false));
        update(&mut s, touche(Key::Down, false, false));
        update(&mut s, touche(Key::Up, false, false));
        update(&mut s, touche(Key::Up, false, false)); // en boucle : dernier
        assert_eq!(
            update(&mut s, touche(Key::Enter, false, false)),
            vec![Command::OpenFile("/p/c.rs".into())]
        );
        assert!(s.palette.is_none());
    }

    #[test]
    fn echap_ferme() {
        let mut s = projet(&["a.rs"]);
        update(&mut s, touche(Key::Char('p'), true, false));
        tape(&mut s, "a");
        assert!(update(&mut s, touche(Key::Esc, false, false)).is_empty());
        assert!(s.palette.is_none());
        assert!(palette_items(&s).is_empty());
    }

    #[test]
    fn palette_de_commandes_rejoue_le_raccourci() {
        // Les deux formes envoyées par les terminaux.
        for c in ['P', 'p'] {
            let mut s = State::default();
            s.settings.format_on_save = false;
            ouvrir_fichier(&mut s, "/p/a.rs", "x");
            assert!(update(&mut s, touche(Key::Char(c), true, true)).is_empty());
            assert_eq!(
                s.palette.as_ref().map(|p| p.kind),
                Some(PaletteKind::Commands)
            );
            tape(&mut s, "sauv");
            assert_eq!(
                palette_items(&s)[0],
                PaletteItem {
                    label: "Fichier : sauvegarder".into(),
                    detail: "Ctrl+S".into(),
                }
            );
            let cmds = update(&mut s, touche(Key::Enter, false, false));
            assert!(
                matches!(&cmds[..], [Command::Save { path, .. }] if path == Path::new("/p/a.rs"))
            );
            assert!(s.palette.is_none());
        }
    }

    #[test]
    fn commande_par_son_nom() {
        let mut s = State::default();
        update(&mut s, touche(Key::Char('P'), true, true));
        tape(&mut s, "toggle_expl");
        assert_eq!(labels(&s)[0], "Explorateur : afficher / masquer");
        let visible = s.explorer.visible;
        update(&mut s, touche(Key::Enter, false, false));
        assert_ne!(s.explorer.visible, visible);
    }

    #[test]
    fn bascule_entre_palettes() {
        let mut s = projet(&["a.rs"]);
        update(&mut s, touche(Key::Char('p'), true, false));
        tape(&mut s, "a");
        update(&mut s, touche(Key::Char('P'), true, true));
        let p = s.palette.clone();
        assert_eq!(
            p.map(|p| (p.kind, p.query)),
            Some((PaletteKind::Commands, "a".into()))
        );
        update(&mut s, touche(Key::Char('p'), true, false));
        assert_eq!(s.palette.map(|p| p.kind), Some(PaletteKind::Files));
    }

    #[test]
    fn registre_coherent() {
        let noms: HashSet<_> = commands().iter().map(|c| c.name).collect();
        assert_eq!(noms.len(), commands().len());
        for c in commands() {
            assert!(!c.label.is_empty() && !c.name.is_empty());
            assert!(
                c.name
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch == '_')
            );
        }
        assert_eq!(command_by_name("save").map(|c| c.key), Some(ctrl('s')));
        assert_eq!(
            command_by_name("toggle_problems").map(|c| c.key.to_string()),
            Some("Ctrl+Shift+M".into())
        );
        assert!(command_by_name("inexistante").is_none());
    }

    #[test]
    fn touches_consommees() {
        let mut s = projet(&["a.rs"]);
        ouvrir_fichier(&mut s, "/p/a.rs", "ab");
        update(&mut s, touche(Key::Char('p'), true, false));
        for k in [
            touche(Key::Char('a'), true, false),
            touche(Key::Char('x'), true, false),
            touche(Key::Char('v'), true, false),
            touche(Key::Delete, false, false),
            touche(Key::Tab, false, false),
            touche(Key::Left, false, false),
            touche(Key::F(12), false, false),
            touche(Key::Char('w'), true, false),
        ] {
            assert!(update(&mut s, k).is_empty());
        }
        assert_eq!(s.doc.rope().to_string(), "ab");
        assert_eq!(s.doc.cursor(), 0);
        assert!(s.palette.is_some());
        assert_eq!(
            update(&mut s, touche(Key::Char('q'), true, false)),
            vec![Command::Quit]
        );
    }

    #[test]
    fn chevron_bascule_vers_les_commandes() {
        let mut s = projet(&["a.rs"]);
        update(&mut s, touche(Key::Char('p'), true, false));
        tape(&mut s, ">");
        let p = s.palette.clone().expect("palette");
        assert_eq!((p.kind, p.query.as_str()), (PaletteKind::Commands, ""));
        // Ailleurs qu'en tête, `>` se tape.
        update(&mut s, touche(Key::Char('p'), true, false));
        tape(&mut s, "a>");
        assert_eq!(s.palette.map(|p| p.query), Some("a>".into()));
    }

    #[test]
    fn aide_couvre_chaque_commande() {
        let touches: HashSet<&str> = SHORTCUTS
            .iter()
            .flat_map(|(_, v)| v.iter().flat_map(|(k, _)| k.split(" / ")))
            .collect();
        for c in commands() {
            assert!(touches.contains(c.key.to_string().as_str()), "{}", c.key);
        }
    }

    fn hit(path: &str, line: usize, character: usize, excerpt: &str) -> TextHit {
        let at = |character| crate::Position { line, character };
        TextHit {
            location: Location {
                path: path.into(),
                start: at(character),
                end: at(character + 1),
            },
            excerpt: excerpt.into(),
        }
    }
    /// Palette de recherche ouverte sur `query`, sans réponse encore.
    fn recherche(s: &mut State, query: &str) {
        update(s, Event::Key(alt('f')));
        tape(s, query);
    }
    fn reponse(s: &mut State, hits: Vec<TextHit>) {
        let generation = s.project_search.generation;
        update(
            s,
            Event::TextResults {
                generation,
                result: Ok((hits, false)),
            },
        );
    }

    #[test]
    fn deux_touches_ouvrent_la_recherche_dans_le_projet() {
        for k in [
            with(Key::Char('F'), true, true),
            with(Key::Char('f'), true, true),
            // Certains terminaux envoient la majuscule sans Shift.
            with(Key::Char('F'), true, false),
            alt('f'),
        ] {
            let mut s = projet(&[]);
            assert!(update(&mut s, Event::Key(k)).is_empty());
            assert_eq!(s.palette.map(|p| p.kind), Some(PaletteKind::Text), "{k}");
            assert!(s.search.is_none(), "{k}");
        }
        // Ctrl+F reste la barre du fichier.
        let mut s = projet(&[]);
        update(&mut s, touche(Key::Char('f'), true, false));
        assert!(s.palette.is_none() && s.search.is_some());
        // Sans dossier ouvert, comme Ctrl+P.
        let mut s = State::default();
        update(&mut s, Event::Key(alt('f')));
        assert_eq!(s.status, "Aucun dossier ouvert");
        assert!(s.palette.is_none());
    }

    #[test]
    fn focus_terminal_la_touche_va_au_shell() {
        let mut s = projet(&[]);
        update(
            &mut s,
            Event::Resize {
                width: 80,
                height: 40,
            },
        );
        update(&mut s, Event::Key(alt('j')));
        assert_eq!(s.focus, Focus::Terminal);
        let cmds = update(&mut s, Event::Key(alt('f')));
        assert!(matches!(cmds[..], [Command::TerminalKey(..)]), "{cmds:?}");
        assert!(s.palette.is_none());
    }

    #[test]
    fn alt_shift_f_met_toujours_en_page() {
        let alt_shift = |c| KeyInput {
            key: Key::Char(c),
            mods: Modifiers {
                ctrl: false,
                alt: true,
                shift: true,
            },
        };
        let alt_majuscule = KeyInput {
            key: Key::Char('F'),
            mods: Modifiers {
                ctrl: false,
                alt: true,
                shift: false,
            },
        };
        for k in [alt_shift('F'), alt_shift('f'), alt_majuscule] {
            let mut s = projet(&[]);
            ouvrir_fichier(&mut s, "/p/a.rs", "x");
            let cmds = update(&mut s, Event::Key(k));
            assert!(
                matches!(cmds[..], [Command::RequestFormat { .. }]),
                "{cmds:?}"
            );
            assert!(s.palette.is_none());
        }
    }

    #[test]
    fn frappe_lance_la_recherche_et_requete_vide_non() {
        let mut s = projet(&[]);
        assert!(update(&mut s, Event::Key(alt('f'))).is_empty());
        let cmds = update(&mut s, touche(Key::Char('x'), false, false));
        assert_eq!(
            cmds,
            vec![Command::SearchText {
                root: "/p".into(),
                show_hidden: false,
                query: "x".into(),
                regex: false,
                case_sensitive: false,
                generation: 2,
            }]
        );
        assert!(s.project_search.searching);
        // Bascules : même requête, nouvelle génération.
        let cmds = update(&mut s, Event::Key(alt('r')));
        assert!(matches!(
            cmds[..],
            [Command::SearchText {
                regex: true,
                case_sensitive: false,
                generation: 3,
                ..
            }]
        ));
        let cmds = update(&mut s, Event::Key(alt('c')));
        assert!(matches!(
            cmds[..],
            [Command::SearchText {
                regex: true,
                case_sensitive: true,
                generation: 4,
                ..
            }]
        ));
        let cmds = update(&mut s, Event::Paste("yz\nautre".into()));
        assert!(matches!(&cmds[..], [Command::SearchText { query, .. }] if query == "xyz"));
        reponse(&mut s, vec![hit("/p/a.rs", 0, 0, "xyz")]);
        update(&mut s, touche(Key::Backspace, false, false));
        update(&mut s, touche(Key::Backspace, false, false));
        // Requête vidée : la réponse encore en vol ne doit pas remplir la liste.
        let en_vol = s.project_search.generation;
        assert!(update(&mut s, touche(Key::Backspace, false, false)).is_empty());
        assert!(labels(&s).is_empty());
        assert!(!s.project_search.searching);
        update(
            &mut s,
            Event::TextResults {
                generation: en_vol,
                result: Ok((vec![hit("/p/a.rs", 0, 0, "x")], false)),
            },
        );
        assert!(labels(&s).is_empty());
    }

    #[test]
    fn alt_r_et_alt_c_ne_visent_que_la_recherche_dans_le_projet() {
        let mut s = projet(&["a.rs"]);
        update(&mut s, touche(Key::Char('p'), true, false));
        update(&mut s, Event::Key(alt('r')));
        update(&mut s, Event::Key(alt('c')));
        assert_eq!(s.project_search, ProjectSearch::default());
    }

    #[test]
    fn generation_perimee_ignoree() {
        let mut s = projet(&[]);
        recherche(&mut s, "ab");
        let courante = s.project_search.generation;
        update(
            &mut s,
            Event::TextResults {
                generation: courante - 1,
                result: Ok((vec![hit("/p/vieux.rs", 0, 0, "a")], false)),
            },
        );
        assert!(labels(&s).is_empty());
        assert!(s.project_search.searching);
        reponse(&mut s, vec![hit("/p/src/neuf.rs", 6, 2, "ab")]);
        assert_eq!(labels(&s), ["src/neuf.rs:7: ab"]);
        assert_eq!(palette_items(&s)[0].detail, "");
        assert!(!s.project_search.searching);
    }

    #[test]
    fn regex_invalide_liste_vide_et_message() {
        let mut s = projet(&[]);
        recherche(&mut s, "(");
        reponse(&mut s, vec![hit("/p/a.rs", 0, 0, "(")]);
        let generation = s.project_search.generation;
        update(
            &mut s,
            Event::TextResults {
                generation,
                result: Err("Regex invalide : x".into()),
            },
        );
        assert!(labels(&s).is_empty());
        assert_eq!(
            s.project_search.error.as_deref(),
            Some("Regex invalide : x")
        );
        // Une nouvelle frappe efface le message.
        tape(&mut s, "a");
        assert_eq!(s.project_search.error, None);
    }

    #[test]
    fn plus_de_cinquante_resultats_tous_atteignables() {
        let mut s = projet(&[]);
        recherche(&mut s, "x");
        reponse(
            &mut s,
            (0..120).map(|i| hit("/p/a.rs", i, 0, "x")).collect(),
        );
        assert_eq!(labels(&s).len(), 120);
        update(&mut s, touche(Key::Up, false, false));
        assert_eq!(s.palette.as_ref().map(|p| p.selected), Some(119));
        ouvrir_fichier(&mut s, "/p/a.rs", &"x\n".repeat(120));
        update(&mut s, touche(Key::Enter, false, false));
        assert_eq!(s.doc.rope().char_to_line(s.doc.cursor()), 119);
    }

    #[test]
    fn entree_va_au_resultat_dans_le_meme_fichier() {
        let mut s = projet(&[]);
        ouvrir_fichier(&mut s, "/p/a.rs", "un\n😀 cible\n");
        s.focus = Focus::Explorer;
        recherche(&mut s, "cible");
        // Colonne UTF-16 : l'émoji compte pour deux.
        reponse(&mut s, vec![hit("/p/a.rs", 1, 3, "😀 cible")]);
        assert!(update(&mut s, touche(Key::Enter, false, false)).is_empty());
        assert!(s.palette.is_none());
        assert_eq!(s.doc.cursor(), 5);
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn entree_va_au_resultat_d_un_autre_fichier() {
        let mut s = projet(&[]);
        ouvrir_fichier(&mut s, "/p/a.rs", "ici\n");
        s.focus = Focus::Explorer;
        recherche(&mut s, "cible");
        reponse(&mut s, vec![hit("/p/b.rs", 1, 2, "cible")]);
        assert_eq!(
            update(&mut s, touche(Key::Enter, false, false)),
            vec![Command::OpenFile("/p/b.rs".into())]
        );
        assert_eq!(s.focus, Focus::Editor);
        ouvrir_fichier(&mut s, "/p/b.rs", "\n  cible\n");
        assert_eq!(s.doc.path.as_deref(), Some(Path::new("/p/b.rs")));
        assert_eq!(s.doc.cursor(), 3);
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn reouverture_avec_requete_et_resultats() {
        let mut s = projet(&[]);
        ouvrir_fichier(&mut s, "/p/a.rs", "ab\n");
        recherche(&mut s, "ab");
        update(&mut s, Event::Key(alt('c')));
        reponse(&mut s, vec![hit("/p/a.rs", 0, 0, "ab")]);
        update(&mut s, touche(Key::Enter, false, false));
        assert!(s.palette.is_none());
        // Résultats réaffichés tout de suite, relus sur le disque en attendant.
        let cmds = update(&mut s, Event::Key(alt('f')));
        assert!(matches!(
            &cmds[..],
            [Command::SearchText { query, case_sensitive: true, .. }] if query == "ab"
        ));
        assert_eq!(s.palette.as_ref().map(|p| p.query.as_str()), Some("ab"));
        assert_eq!(labels(&s), ["a.rs:1: ab"]);
    }

    #[test]
    fn commande_de_la_palette_ouvre_la_recherche() {
        let mut s = projet(&[]);
        update(&mut s, touche(Key::Char('P'), true, true));
        tape(&mut s, "project_search");
        assert_eq!(labels(&s)[0], "Recherche : rechercher dans le projet");
        update(&mut s, touche(Key::Enter, false, false));
        assert_eq!(s.palette.map(|p| p.kind), Some(PaletteKind::Text));
    }

    #[test]
    fn collage_dans_la_requete() {
        let mut s = projet(&["a.rs"]);
        ouvrir_fichier(&mut s, "/p/a.rs", "ab");
        update(&mut s, touche(Key::Char('p'), true, false));
        update(&mut s, Event::Paste("lib\nautre".into()));
        assert_eq!(s.palette.map(|p| p.query), Some("lib".into()));
        assert_eq!(s.doc.rope().to_string(), "ab");
    }
}
