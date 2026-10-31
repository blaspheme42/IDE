//! Explorateur de fichiers (F-01, F-03, F-04) : arbre affiché, sélection, invites. Zéro I/O :
//! le listage et les opérations disque passent par des `Command` exécutées par l'app.

use std::path::{Component, Path, PathBuf};

use crate::{Command, Focus, Key, KeyInput, MouseInput, MouseKind, State, explorer_width};

/// Lignes défilées par cran de molette.
const WHEEL_LINES: usize = 3;

/// Entrée d'un dossier listé par l'app (miroir de `ide_workspace::DirEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Une ligne visible de l'arbre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub entry: FsEntry,
    /// Profondeur sous la racine (0 = enfant direct).
    pub depth: usize,
    /// Dossier déplié (ses enfants suivent dans `nodes`).
    pub expanded: bool,
    /// Listage demandé, réponse pas encore arrivée.
    pub loading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    NewFile,
    NewDir,
    Rename,
    Move,
    /// Confirmer la mise à la corbeille : `o`/`y` ou Entrée confirme, Échap ou `n` annule.
    ConfirmTrash,
}

/// Invite en bas du panneau (nom à saisir, ou confirmation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    /// Dossier ouvert ; `None` tant que l'app ne l'a pas fourni (`Event::OpenRoot`).
    pub root: Option<PathBuf>,
    /// Lignes visibles, dans l'ordre d'affichage.
    pub nodes: Vec<Node>,
    /// Index dans `nodes` de la ligne sélectionnée.
    pub selected: usize,
    /// Première ligne visible.
    pub scroll: usize,
    /// Panneau affiché (Ctrl+B).
    pub visible: bool,
    /// Largeur du panneau, bordures comprises (redimensionnable).
    pub width: u16,
    pub show_hidden: bool,
    pub prompt: Option<Prompt>,
    /// Dernière erreur de listage de la racine, affichée dans le panneau.
    pub error: Option<String>,
    /// Cible de l'invite ouverte : dossier où créer, ou chemin à renommer / déplacer / jeter.
    pub prompt_target: PathBuf,
    /// Fichier créé par l'invite, à ouvrir quand `FsDone` confirmera sa création.
    pub pending_open: Option<PathBuf>,
    /// Chemin à sélectionner dès qu'un listage le fera apparaître (fichier créé, renommé).
    pub reveal: Option<PathBuf>,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            root: None,
            nodes: Vec::new(),
            selected: 0,
            scroll: 0,
            visible: true,
            width: 30,
            show_hidden: false,
            prompt: None,
            error: None,
            prompt_target: PathBuf::new(),
            pending_open: None,
            reveal: None,
        }
    }
}

/// Touches : globales (Ctrl+B, Ctrl+E) et, quand l'explorateur a le focus, navigation et invites.
/// `None` = touche laissée à l'éditeur.
pub(crate) fn key(state: &mut State, k: KeyInput) -> Option<Vec<Command>> {
    let ctrl = k.mods.ctrl && !k.mods.alt;
    let ex = &mut state.explorer;
    match k.key {
        Key::Char('b' | 'B') if ctrl => {
            ex.visible = !ex.visible;
            if !ex.visible {
                ex.prompt = None;
                state.focus = Focus::Editor;
            }
            return Some(vec![]);
        }
        Key::Char('e' | 'E') if ctrl => {
            state.focus = if state.focus == Focus::Explorer {
                Focus::Editor
            } else {
                ex.visible = true;
                Focus::Explorer
            };
            ensure_visible(state);
            return Some(vec![]);
        }
        _ => {}
    }
    if state.focus != Focus::Explorer {
        return None;
    }
    if k.mods.ctrl && k.mods.alt && matches!(k.key, Key::Left | Key::Right) {
        resize(state, if k.key == Key::Left { -2 } else { 2 });
        return Some(vec![]);
    }
    // Ctrl+Q, Ctrl+S, Ctrl+W… restent globaux, même pendant une invite.
    if k.mods.ctrl {
        return None;
    }
    if state.explorer.prompt.is_some() {
        return Some(prompt_key(state, k));
    }
    Some(nav_key(state, k))
}

fn nav_key(state: &mut State, k: KeyInput) -> Vec<Command> {
    let sel = state.explorer.selected;
    let page = rows(state);
    let node = state.explorer.nodes.get(sel).cloned();
    match k.key {
        Key::Up => select(state, sel.saturating_sub(1)),
        Key::Down => select(state, sel + 1),
        Key::PageUp => select(state, sel.saturating_sub(page)),
        Key::PageDown => select(state, sel + page),
        Key::Home => select(state, 0),
        Key::End => select(state, usize::MAX),
        Key::Right | Key::Enter if node.as_ref().is_some_and(|n| n.entry.is_dir && !n.expanded) => {
            return toggle(state, sel).into_iter().collect();
        }
        Key::Right => {
            let next = state.explorer.nodes.get(sel + 1);
            if node.is_some_and(|n| n.expanded && next.is_some_and(|c| c.depth > n.depth)) {
                select(state, sel + 1);
            }
        }
        Key::Enter => match node {
            Some(n) if n.entry.is_dir => {
                toggle(state, sel);
            }
            Some(n) => {
                state.focus = Focus::Editor;
                return vec![Command::OpenFile(n.entry.path)];
            }
            None => {}
        },
        Key::Left => match node {
            Some(n) if n.expanded || n.loading => {
                collapse(&mut state.explorer, sel);
                ensure_visible(state);
            }
            Some(n) => {
                let parent = state.explorer.nodes[..sel]
                    .iter()
                    .rposition(|p| p.depth < n.depth);
                if let Some(p) = parent {
                    select(state, p);
                }
            }
            None => {}
        },
        Key::Char('n') if !k.mods.shift => open_prompt(state, PromptKind::NewFile),
        Key::Char('n' | 'N') => open_prompt(state, PromptKind::NewDir),
        Key::F(2) | Key::Char('r') => open_prompt(state, PromptKind::Rename),
        Key::Char('m') => open_prompt(state, PromptKind::Move),
        Key::Delete => open_prompt(state, PromptKind::ConfirmTrash),
        Key::Char('.') => return toggle_hidden(state),
        Key::Char('<') => resize(state, -2),
        Key::Char('>') => resize(state, 2),
        Key::Esc => state.focus = Focus::Editor,
        _ => {}
    }
    vec![]
}

fn prompt_key(state: &mut State, k: KeyInput) -> Vec<Command> {
    let ex = &mut state.explorer;
    let Some(p) = &mut ex.prompt else {
        return vec![];
    };
    match (p.kind, k.key) {
        (PromptKind::ConfirmTrash, Key::Enter | Key::Char('o' | 'O' | 'y' | 'Y'))
        | (_, Key::Enter) => return submit(state),
        (_, Key::Esc) | (PromptKind::ConfirmTrash, Key::Char('n' | 'N')) => {
            ex.prompt = None;
            ensure_visible(state);
        }
        (PromptKind::ConfirmTrash, _) => {}
        (_, Key::Char(c)) if !c.is_control() => p.input.push(c),
        (_, Key::Backspace) => {
            p.input.pop();
        }
        _ => {}
    }
    vec![]
}

/// Ouvre l'invite : création dans le dossier sélectionné (ou celui du fichier sélectionné, ou la racine),
/// sinon opération sur l'entrée sélectionnée.
fn open_prompt(state: &mut State, kind: PromptKind) {
    let ex = &mut state.explorer;
    let Some(root) = &ex.root else {
        state.status = "Aucun dossier ouvert".into();
        return;
    };
    let creating = matches!(kind, PromptKind::NewFile | PromptKind::NewDir);
    let (target, input) = match ex.nodes.get(ex.selected).map(|n| &n.entry) {
        None if creating => (root.clone(), String::new()),
        None => return,
        Some(e) if creating && e.is_dir => (e.path.clone(), String::new()),
        Some(e) if creating => (
            e.path
                .parent()
                .map_or_else(|| root.clone(), Path::to_path_buf),
            String::new(),
        ),
        Some(e) => {
            let input = match kind {
                PromptKind::Rename => e.name.clone(),
                PromptKind::Move => rel(root, &e.path),
                _ => String::new(),
            };
            (e.path.clone(), input)
        }
    };
    ex.prompt_target = target;
    ex.prompt = Some(Prompt { kind, input });
    ensure_visible(state);
}

/// Entrée dans l'invite : commande à exécuter, ou statut d'erreur et l'invite reste ouverte.
fn submit(state: &mut State) -> Vec<Command> {
    let ex = &mut state.explorer;
    let Some(p) = &ex.prompt else {
        return vec![];
    };
    match target_command(ex, p.kind, p.input.trim()) {
        Ok(cmd) => {
            ex.prompt = None;
            if let Command::CreateFile(path) = &cmd {
                ex.pending_open = Some(path.clone());
            }
            ensure_visible(state);
            vec![cmd]
        }
        Err(msg) => {
            state.status = msg;
            vec![]
        }
    }
}

fn target_command(ex: &Explorer, kind: PromptKind, input: &str) -> Result<Command, String> {
    let root = ex.root.as_deref().ok_or("Aucun dossier ouvert")?;
    let target = ex.prompt_target.clone();
    if kind == PromptKind::ConfirmTrash {
        return Ok(Command::Trash(target));
    }
    if input.is_empty() {
        return Err("Nom vide".into());
    }
    if kind != PromptKind::Move && (input.contains('/') || input == "." || input == "..") {
        return Err(format!("Nom invalide : {input}"));
    }
    match kind {
        PromptKind::NewFile => Ok(Command::CreateFile(target.join(input))),
        PromptKind::NewDir => Ok(Command::CreateDir(target.join(input))),
        PromptKind::Rename => {
            let to = target.with_file_name(input);
            if to == target {
                return Err("Nom inchangé".into());
            }
            Ok(Command::Rename { from: target, to })
        }
        _ => {
            let mut to = root.to_path_buf();
            for c in Path::new(input).components() {
                match c {
                    Component::Normal(s) => to.push(s),
                    Component::CurDir => {}
                    Component::ParentDir if to != root => {
                        to.pop();
                    }
                    _ => return Err(format!("Chemin hors de la racine : {input}")),
                }
            }
            // Dossier connu (ou la racine) : on déplace dedans, sinon `to` est le chemin final.
            let into_dir = to == root
                || ex
                    .nodes
                    .iter()
                    .any(|n| n.entry.is_dir && n.entry.path == to);
            if into_dir && let Some(name) = target.file_name() {
                to.push(name);
            }
            if to == target {
                return Err("Destination identique".into());
            }
            if to.starts_with(&target) {
                return Err("Impossible de déplacer un dossier dans lui-même".into());
            }
            Ok(Command::Rename { from: target, to })
        }
    }
}

/// `.` : bascule les fichiers cachés et re-liste la racine et tous les dossiers dépliés.
fn toggle_hidden(state: &mut State) -> Vec<Command> {
    let ex = &mut state.explorer;
    ex.show_hidden = !ex.show_hidden;
    state.status = if ex.show_hidden {
        "Fichiers cachés affichés"
    } else {
        "Fichiers cachés masqués"
    }
    .into();
    let show_hidden = ex.show_hidden;
    let expanded = ex.nodes.iter().filter(|n| n.expanded);
    ex.root
        .iter()
        .chain(expanded.map(|n| &n.entry.path))
        .map(|d| Command::ListDir {
            dir: d.clone(),
            show_hidden,
        })
        .collect()
}

/// Largeur ±, bornée à 12..=80 et à la largeur du terminal − 20 (l'éditeur garde de la place).
fn resize(state: &mut State, delta: i32) {
    let max = 80.min(i32::from(state.width) - 20).max(12);
    let w = (i32::from(state.explorer.width) + delta).clamp(12, max);
    state.explorer.width = u16::try_from(w).unwrap_or(30);
}

/// Hauteur des lignes de l'arbre : écran − statut − 2 bordures − ligne d'invite.
fn rows(state: &State) -> usize {
    let prompt = usize::from(state.explorer.prompt.is_some());
    usize::from(state.height).saturating_sub(3 + prompt).max(1)
}

fn select(state: &mut State, i: usize) {
    state.explorer.selected = i;
    ensure_visible(state);
}

/// Borne la sélection et fait défiler juste assez pour la garder visible.
fn ensure_visible(state: &mut State) {
    let h = rows(state);
    let ex = &mut state.explorer;
    let len = ex.nodes.len();
    ex.selected = ex.selected.min(len.saturating_sub(1));
    ex.scroll = ex.scroll.min(len.saturating_sub(h));
    if ex.selected < ex.scroll {
        ex.scroll = ex.selected;
    } else if ex.selected >= ex.scroll + h {
        ex.scroll = ex.selected + 1 - h;
    }
}

/// Fin (exclue) du sous-arbre affiché sous `nodes[i]`.
fn subtree_end(nodes: &[Node], i: usize) -> usize {
    let depth = nodes[i].depth;
    nodes[i + 1..]
        .iter()
        .position(|n| n.depth <= depth)
        .map_or(nodes.len(), |p| i + 1 + p)
}

fn collapse(ex: &mut Explorer, i: usize) {
    let end = subtree_end(&ex.nodes, i);
    ex.nodes.drain(i + 1..end);
    ex.nodes[i].expanded = false;
    ex.nodes[i].loading = false;
    if ex.selected >= end {
        ex.selected -= end - i - 1;
    } else if ex.selected > i {
        ex.selected = i;
    }
}

/// Replie un dossier déplié, ou demande le listage d'un dossier replié.
fn toggle(state: &mut State, i: usize) -> Option<Command> {
    let ex = &mut state.explorer;
    let n = &mut ex.nodes[i];
    if n.expanded || n.loading {
        collapse(ex, i);
        ensure_visible(state);
        return None;
    }
    n.loading = true;
    Some(Command::ListDir {
        dir: n.entry.path.clone(),
        show_hidden: ex.show_hidden,
    })
}

/// Souris dans le panneau ; `None` = hors du panneau.
pub(crate) fn mouse(state: &mut State, m: MouseInput) -> Option<Vec<Command>> {
    let status_row = usize::from(state.height).saturating_sub(1);
    if m.column >= explorer_width(state) || usize::from(m.row) >= status_row {
        if m.kind == MouseKind::Down {
            state.focus = Focus::Editor;
        }
        return None;
    }
    let h = rows(state);
    let ex = &mut state.explorer;
    let mut cmds = vec![];
    match m.kind {
        MouseKind::ScrollUp => ex.scroll = ex.scroll.saturating_sub(WHEEL_LINES),
        MouseKind::ScrollDown => {
            ex.scroll = (ex.scroll + WHEEL_LINES).min(ex.nodes.len().saturating_sub(h))
        }
        MouseKind::Down => {
            state.focus = Focus::Explorer;
            state.dragging = false;
            let row = usize::from(m.row);
            let i = (ex.scroll + row).wrapping_sub(1);
            if (1..=h).contains(&row) && i < ex.nodes.len() {
                ex.selected = i;
                if ex.nodes[i].entry.is_dir {
                    cmds.extend(toggle(state, i));
                } else {
                    cmds.push(Command::OpenFile(ex.nodes[i].entry.path.clone()));
                }
            }
        }
        // L'éditeur ignore déjà un glisser qui n'a pas commencé dans le texte, et prolonge celui qui y a commencé.
        MouseKind::Drag | MouseKind::Up => return None,
    }
    Some(cmds)
}

/// `Event::OpenRoot` : nouvelle racine, à lister.
pub(crate) fn open_root(state: &mut State, root: PathBuf) -> Vec<Command> {
    let ex = &mut state.explorer;
    ex.nodes.clear();
    ex.selected = 0;
    ex.scroll = 0;
    ex.prompt = None;
    ex.error = None;
    ex.root = Some(root.clone());
    vec![Command::ListDir {
        dir: root,
        show_hidden: ex.show_hidden,
    }]
}

/// `Event::DirListed` : insère (ou remplace) les enfants de `dir`.
/// Un re-listage (rafraîchissement, `.`) garde dépliés les sous-dossiers qui l'étaient, avec leur
/// contenu déjà connu : il n'est pas re-listé ici (`listed` ne renvoie pas de commande).
pub(crate) fn listed(state: &mut State, dir: PathBuf, result: Result<Vec<FsEntry>, String>) {
    let ex = &mut state.explorer;
    let is_root = ex.root.as_ref() == Some(&dir);
    let (start, end, depth) = if is_root {
        (0, ex.nodes.len(), 0)
    } else {
        // Dossier replié entre-temps, ou disparu : réponse ignorée.
        let Some(i) = ex
            .nodes
            .iter()
            .position(|n| n.entry.path == dir && (n.expanded || n.loading))
        else {
            return;
        };
        ex.nodes[i].loading = false;
        (i + 1, subtree_end(&ex.nodes, i), ex.nodes[i].depth + 1)
    };
    let entries = match result {
        Ok(entries) => entries,
        Err(e) if is_root => {
            ex.nodes.clear();
            ex.error = Some(e);
            ensure_visible(state);
            return;
        }
        Err(e) => {
            state.status = format!("Impossible de lister {} : {e}", dir.display());
            return;
        }
    };
    if is_root {
        ex.error = None;
    } else {
        ex.nodes[start - 1].expanded = true;
    }
    let keep = ex
        .reveal
        .clone()
        .or_else(|| ex.nodes.get(ex.selected).map(|n| n.entry.path.clone()));
    let old = &ex.nodes[start..end];
    let mut fresh = Vec::new();
    for entry in entries {
        // Dossier déjà affiché : il garde son état et, s'il est déplié, son contenu connu.
        let prev = old.iter().position(|n| {
            n.depth == depth && n.entry.path == entry.path && n.entry.is_dir == entry.is_dir
        });
        let (expanded, loading) =
            prev.map_or((false, false), |p| (old[p].expanded, old[p].loading));
        fresh.push(Node {
            entry,
            depth,
            expanded,
            loading,
        });
        if let Some(p) = prev.filter(|_| expanded) {
            fresh.extend_from_slice(&old[p + 1..subtree_end(old, p)]);
        }
    }
    ex.nodes.splice(start..end, fresh);
    if let Some(i) = keep.and_then(|k| ex.nodes.iter().position(|n| n.entry.path == k)) {
        ex.selected = i;
        if ex.reveal.as_ref() == Some(&ex.nodes[i].entry.path) {
            ex.reveal = None;
        }
    }
    ensure_visible(state);
}

/// `Event::FsDone` : résultat d'une opération disque ; rafraîchit les dossiers concernés.
pub(crate) fn fs_done(state: &mut State, result: Result<FsChange, String>) -> Vec<Command> {
    let change = match result {
        Ok(c) => c,
        Err(e) => {
            state.status = format!("Échec : {e}");
            return vec![];
        }
    };
    let root = state.explorer.root.clone().unwrap_or_default();
    let mut open = None;
    let parents: Vec<&Path> = match &change {
        FsChange::Created(p) => {
            state.status = format!("Créé : {}", rel(&root, p));
            state.explorer.reveal = Some(p.clone());
            if state.explorer.pending_open.as_ref() == Some(p) {
                state.explorer.pending_open = None;
                state.focus = Focus::Editor;
                open = Some(Command::OpenFile(p.clone()));
            }
            p.parent().into_iter().collect()
        }
        FsChange::Renamed { from, to } => {
            state.status = format!("Renommé : {} → {}", rel(&root, from), rel(&root, to));
            state.explorer.reveal = Some(to.clone());
            if let Some(dp) = &state.doc.path
                && let Ok(rest) = dp.strip_prefix(from)
            {
                // `join("")` ajouterait un `/` final.
                let moved = if rest.as_os_str().is_empty() {
                    to.clone()
                } else {
                    to.join(rest)
                };
                state.doc.path = Some(moved);
            }
            let (a, b) = (from.parent(), to.parent());
            a.into_iter().chain(b.filter(|b| Some(*b) != a)).collect()
        }
        FsChange::Trashed(p) => {
            state.status = format!("Mis à la corbeille : {}", rel(&root, p));
            if state.doc.path.as_ref().is_some_and(|d| d.starts_with(p)) {
                state.status.push_str(" (encore ouvert dans l'éditeur)");
            }
            p.parent().into_iter().collect()
        }
    };
    let ex = &mut state.explorer;
    let mut cmds = vec![];
    for dir in parents {
        let known = ex.root.as_deref() == Some(dir)
            || ex.nodes.iter_mut().any(|n| {
                let hit = n.entry.is_dir && n.entry.path == dir;
                // Dossier replié : le listage le dépliera, pour montrer le résultat.
                n.loading |= hit;
                hit
            });
        if known {
            cmds.push(Command::ListDir {
                dir: dir.to_path_buf(),
                show_hidden: ex.show_hidden,
            });
        }
    }
    cmds.extend(open);
    cmds
}

/// Chemin affiché relativement à la racine.
fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

/// Ce qu'une opération disque réussie a changé, pour rafraîchir l'arbre et les onglets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    Created(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    Trashed(PathBuf),
}
