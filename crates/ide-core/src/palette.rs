//! Palettes (F-50, F-51) : ouvrir un fichier par recherche floue (Ctrl+P) et lancer une commande
//! (Ctrl+Shift+P). Une commande de la palette rejoue simplement son raccourci clavier.

use std::path::Path;

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::{Command, Event, Key, KeyInput, Modifiers, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    Files,
    Commands,
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
    cmd("trigger_completion", "Code : autocomplétion", ctrl(' ')),
    cmd("hover", "Code : afficher l'infobulle", ctrl('k')),
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
            ("Alt+N", "Terminal, avec le focus : nouvel onglet"),
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

/// Index triés dans `project_files` ou `commands()`, au plus `MAX_ITEMS`.
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
    };
    v.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    v.into_iter().take(MAX_ITEMS).map(|(.., i)| i).collect()
}

/// Éléments filtrés et triés par pertinence pour la requête, au plus 50.
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
    }
}

/// Ouvre (ou bascule vers) une palette. La liste des fichiers est redemandée à chaque ouverture (fichiers créés
/// ou supprimés entre-temps) ; l'ancienne reste affichée en attendant, sauf si elle vient d'une autre racine.
fn open(state: &mut State, kind: PaletteKind) -> Vec<Command> {
    let mut cmds = vec![];
    if kind == PaletteKind::Files {
        let Some(root) = state.explorer.root.clone() else {
            state.status = "Aucun dossier ouvert".into();
            return cmds;
        };
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
    refresh(state);
    cmds
}

/// Entrée : ferme la palette, puis ouvre le fichier ou rejoue le raccourci de la commande.
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
    }
}

/// Touches : Ctrl+P / Ctrl+Shift+P ouvrent ; palette ouverte, toutes les touches lui reviennent
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
            refresh(state);
        }
        // `>` en tête (VSCode) : commandes, joignables même quand le terminal confond Ctrl+Shift+P et Ctrl+P.
        Key::Char('>') if p.kind == PaletteKind::Files && p.query.is_empty() && !ctrl && !alt => {
            p.kind = PaletteKind::Commands;
            refresh(state);
        }
        Key::Char(c) if !ctrl && !alt && !c.is_control() => {
            p.query.push(c);
            p.selected = 0;
            refresh(state);
        }
        Key::Char('q' | 'Q') if ctrl && !alt => return None,
        _ => {}
    }
    Some(vec![])
}

/// Collage dans la requête : première ligne seulement.
pub(crate) fn paste(state: &mut State, text: &str) {
    if let Some(p) = &mut state.palette {
        p.query.push_str(text.lines().next().unwrap_or_default());
        p.selected = 0;
    }
    refresh(state);
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
