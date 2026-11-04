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
    // Raccourcis globaux (fichier, onglets) seulement : les raccourcis d'édition (Ctrl+X/V/Z/A…)
    // modifieraient le document caché sans qu'on le voie.
    if k.mods.alt {
        return crate::editor::is_comment_key(k).then(Vec::new);
    }
    if k.mods.ctrl {
        return match k.key {
            Key::Char('q' | 'Q' | 's' | 'S' | 'w' | 'W')
            | Key::Tab
            | Key::PageUp
            | Key::PageDown => None,
            Key::Char('f' | 'F' | 'h' | 'H') => {
                state.focus = Focus::Editor;
                None
            }
            _ => Some(vec![]),
        };
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
        // Droite sur un dossier en cours de listage : rien (Entrée, elle, l'annule).
        Key::Right
            if node
                .as_ref()
                .is_some_and(|n| n.entry.is_dir && !n.expanded && !n.loading) =>
        {
            return toggle(state, sel).into_iter().collect();
        }
        Key::Right => {
            let next = state.explorer.nodes.get(sel + 1);
            if node.is_some_and(|n| n.expanded && next.is_some_and(|c| c.depth > n.depth)) {
                select(state, sel + 1);
            }
        }
        Key::Enter => match node {
            Some(n) if n.entry.is_dir => return toggle(state, sel).into_iter().collect(),
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

/// Collage avec le focus explorateur : dans l'invite de saisie (première ligne), jamais dans le document.
pub(crate) fn paste(state: &mut State, text: &str) {
    if let Some(p) = &mut state.explorer.prompt
        && p.kind != PromptKind::ConfirmTrash
    {
        p.input.push_str(text.lines().next().unwrap_or_default());
    }
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
    let cmd = target_command(ex, p.kind, p.input.trim()).and_then(|cmd| match &cmd {
        // Le disque ne connaît pas un onglet pas encore écrit : deux onglets sur un chemin s'écraseraient.
        Command::Rename { to, .. }
            if std::iter::once(&state.doc)
                .chain(state.tabs.iter().map(|t| &t.doc))
                .any(|d| d.path.as_deref().is_some_and(|p| p.starts_with(to))) =>
        {
            Err(format!("{} est ouvert dans un onglet", to.display()))
        }
        _ => Ok(cmd),
    });
    match cmd {
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
                || input.ends_with('/')
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
    let w = explorer_width(state);
    let ex = &mut state.explorer;
    let mut cmds = vec![];
    match m.kind {
        MouseKind::ScrollUp => ex.scroll = ex.scroll.saturating_sub(WHEEL_LINES),
        MouseKind::ScrollDown => {
            ex.scroll = (ex.scroll + WHEEL_LINES).min(ex.nodes.len().saturating_sub(h))
        }
        // Invite ouverte : un clic dans l'arbre l'annule, sinon la cible affichée et la ligne sélectionnée divergent.
        MouseKind::Down if ex.prompt.is_some() => {
            ex.prompt = None;
            state.focus = Focus::Explorer;
            ensure_visible(state);
        }
        MouseKind::Down => {
            state.focus = Focus::Explorer;
            state.dragging = false;
            let row = usize::from(m.row);
            let i = (ex.scroll + row).wrapping_sub(1);
            let inside = m.column > 0 && m.column + 1 < w;
            if inside && (1..=h).contains(&row) && i < ex.nodes.len() {
                ex.selected = i;
                if ex.nodes[i].entry.is_dir {
                    cmds.extend(toggle(state, i));
                } else {
                    cmds.push(Command::OpenFile(ex.nodes[i].entry.path.clone()));
                    state.focus = Focus::Editor;
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
    let selected = ex.nodes.get(ex.selected).map(|n| n.entry.path.clone());
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
    let find = |p: &PathBuf| ex.nodes.iter().position(|n| &n.entry.path == p);
    // Chemin à révéler s'il est apparu, sinon la ligne qui était sélectionnée.
    if let Some(i) = ex.reveal.as_ref().and_then(find) {
        ex.selected = i;
        ex.reveal = None;
    } else if let Some(i) = selected.as_ref().and_then(find) {
        ex.selected = i;
    }
    ensure_visible(state);
}

/// `Event::DiskChanged` : re-liste (une fois) chaque dossier parent qui est la racine ou un dossier déplié ;
/// `listed` garde sélection et dépliage.
pub(crate) fn disk_changed(state: &State, paths: &[PathBuf]) -> Vec<Command> {
    let ex = &state.explorer;
    let mut cmds = vec![];
    for dir in paths.iter().filter_map(|p| p.parent()) {
        let known = ex.root.as_deref() == Some(dir)
            || ex.nodes.iter().any(|n| n.expanded && n.entry.path == dir);
        let cmd = Command::ListDir {
            dir: dir.to_path_buf(),
            show_hidden: ex.show_hidden,
        };
        if known && !cmds.contains(&cmd) {
            cmds.push(cmd);
        }
    }
    cmds
}

/// `Event::FsDone` : résultat d'une opération disque ; rafraîchit les dossiers concernés.
pub(crate) fn fs_done(state: &mut State, result: Result<FsChange, String>) -> Vec<Command> {
    let change = match result {
        Ok(c) => c,
        Err(e) => {
            state.explorer.pending_open = None;
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
            // Les chemins des onglets sont suivis par `tabs::on_fs_change`, appelée avant.
            let (a, b) = (from.parent(), to.parent());
            a.into_iter().chain(b.filter(|b| Some(*b) != a)).collect()
        }
        FsChange::Trashed(p) => {
            state.status = format!("Mis à la corbeille : {}", rel(&root, p));
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
    // Rien ne re-listera le dossier du chemin à révéler : l'oublier, sinon un dépliage futur y sauterait.
    let relisted = |r: &PathBuf| {
        cmds.iter()
            .any(|c| matches!(c, Command::ListDir { dir, .. } if Some(dir.as_path()) == r.parent()))
    };
    if !ex.reveal.as_ref().is_some_and(relisted) {
        ex.reveal = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, Modifiers, update};

    const ROOT: &str = "/r";

    fn p(s: &str) -> PathBuf {
        Path::new(ROOT).join(s)
    }

    /// `"a/"` = dossier, sinon fichier ; chemin relatif à la racine.
    fn ent(rel: &str) -> FsEntry {
        let is_dir = rel.ends_with('/');
        let path = p(rel.trim_end_matches('/'));
        let name = path.file_name().expect("nom").to_string_lossy().into();
        FsEntry { name, path, is_dir }
    }

    fn list(s: &mut State, dir: PathBuf, entries: &[&str]) -> Vec<Command> {
        let result = Ok(entries.iter().map(|e| ent(e)).collect());
        update(s, Event::DirListed { dir, result })
    }

    fn kin(key: Key, ctrl: bool, alt: bool, shift: bool) -> KeyInput {
        KeyInput {
            key,
            mods: Modifiers { ctrl, alt, shift },
        }
    }

    fn press(s: &mut State, key: Key) -> Vec<Command> {
        update(s, Event::Key(kin(key, false, false, false)))
    }

    fn typ(s: &mut State, text: &str) {
        for c in text.chars() {
            press(s, Key::Char(c));
        }
    }

    fn input(s: &mut State, text: &str) {
        s.explorer.prompt.as_mut().expect("invite").input = text.into();
    }

    fn ls(dir: PathBuf) -> Command {
        Command::ListDir {
            dir,
            show_hidden: false,
        }
    }

    fn mi(kind: MouseKind, column: u16, row: u16) -> MouseInput {
        MouseInput {
            kind,
            column,
            row,
            mods: Modifiers::default(),
        }
    }

    fn click(s: &mut State, kind: MouseKind, column: u16, row: u16) -> Vec<Command> {
        update(s, Event::Mouse(mi(kind, column, row)))
    }

    /// Hauteur 10 → 7 lignes d'arbre. Racine : a/, b/, f.txt ; focus explorateur.
    fn setup() -> State {
        let mut s = State {
            width: 100,
            height: 10,
            ..State::default()
        };
        let cmds = update(&mut s, Event::OpenRoot(ROOT.into()));
        assert_eq!(cmds, vec![ls(ROOT.into())]);
        list(&mut s, ROOT.into(), &["a/", "b/", "f.txt"]);
        s.focus = Focus::Explorer;
        s
    }

    #[test]
    fn focus_explorateur_protege_le_document() {
        let mut s = setup();
        s.doc = crate::Document::from_text(Some(p("f.txt")), "texte");
        // Raccourcis d'édition ignorés, collage hors invite ignoré.
        for c in ['x', 'v', 'z', 'a'] {
            assert_eq!(
                update(&mut s, Event::Key(kin(Key::Char(c), true, false, false))),
                vec![]
            );
        }
        update(&mut s, Event::Paste("collé".into()));
        assert_eq!(s.doc.rope().to_string(), "texte");
        // Collage dans l'invite : première ligne seulement.
        press(&mut s, Key::Char('n'));
        update(&mut s, Event::Paste("nom.rs\nreste".into()));
        assert_eq!(
            s.explorer.prompt.as_ref().map(|p| p.input.as_str()),
            Some("nom.rs")
        );
        assert_eq!(s.doc.rope().to_string(), "texte");
        // Ctrl+F rend le focus à l'éditeur pour que la barre de recherche reçoive la saisie.
        press(&mut s, Key::Esc);
        update(&mut s, Event::Key(kin(Key::Char('f'), true, false, false)));
        assert_eq!(s.focus, Focus::Editor);
        assert!(s.search.is_some());
    }

    #[test]
    fn renommer_vers_un_onglet_ouvert_refuse() {
        let mut s = setup();
        // Onglet sur un fichier pas encore écrit : le disque ne le connaît pas.
        s.doc = crate::Document::from_text(Some(p("neuf.txt")), "");
        let i = s
            .explorer
            .nodes
            .iter()
            .position(|n| n.entry.name == "f.txt");
        s.explorer.selected = i.unwrap_or_default();
        press(&mut s, Key::Char('r'));
        input(&mut s, "neuf.txt");
        assert!(press(&mut s, Key::Enter).is_empty());
        assert!(s.status.contains("ouvert dans un onglet"), "{}", s.status);
        assert!(s.explorer.prompt.is_some());
    }

    fn names(s: &State) -> Vec<(String, usize)> {
        let nodes = &s.explorer.nodes;
        nodes
            .iter()
            .map(|n| (n.entry.name.clone(), n.depth))
            .collect()
    }

    fn n(name: &str, depth: usize) -> (String, usize) {
        (name.into(), depth)
    }

    fn sel(s: &State) -> &str {
        &s.explorer.nodes[s.explorer.selected].entry.name
    }

    fn prompt(s: &State) -> (PromptKind, &str) {
        let p = s.explorer.prompt.as_ref().expect("invite");
        (p.kind, &p.input)
    }

    /// Déplie `a` (sélectionné) : a/x/, a/y.rs.
    fn expand_a(s: &mut State) {
        assert_eq!(press(s, Key::Right), vec![ls(p("a"))]);
        assert!(s.explorer.nodes[0].loading);
        list(s, p("a"), &["a/x/", "a/y.rs"]);
    }

    #[test]
    fn ouverture_de_la_racine() {
        let s = setup();
        assert_eq!(s.explorer.root, Some(ROOT.into()));
        assert_eq!(names(&s), vec![n("a", 0), n("b", 0), n("f.txt", 0)]);
        assert_eq!(s.explorer.selected, 0);
    }

    #[test]
    fn depliage_puis_reponse() {
        let mut s = setup();
        expand_a(&mut s);
        let a = &s.explorer.nodes[0];
        assert!(a.expanded && !a.loading);
        assert_eq!(names(&s)[1..3], [n("x", 1), n("y.rs", 1)]);
        // Entrée sur un dossier replié : même effet.
        press(&mut s, Key::Down);
        assert_eq!(press(&mut s, Key::Enter), vec![ls(p("a/x"))]);
        list(&mut s, p("a/x"), &["a/x/z"]);
        assert_eq!(names(&s)[2], n("z", 2));
    }

    #[test]
    fn droite_pendant_le_listage_ne_relance_rien() {
        let mut s = setup();
        press(&mut s, Key::Right);
        assert!(press(&mut s, Key::Right).is_empty());
        assert!(s.explorer.nodes[0].loading);
        // Entrée annule ; la réponse tardive est ignorée.
        assert!(press(&mut s, Key::Enter).is_empty());
        assert!(!s.explorer.nodes[0].loading);
        list(&mut s, p("a"), &["a/y.rs"]);
        assert_eq!(s.explorer.nodes.len(), 3);
    }

    #[test]
    fn droite_sur_dossier_deplie_va_au_premier_enfant() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::Right);
        assert_eq!(sel(&s), "x");
    }

    #[test]
    fn repliage_retire_les_descendants() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::Down);
        press(&mut s, Key::Right);
        list(&mut s, p("a/x"), &["a/x/z"]);
        press(&mut s, Key::Up);
        press(&mut s, Key::Left);
        assert_eq!(names(&s), vec![n("a", 0), n("b", 0), n("f.txt", 0)]);
        assert!(!s.explorer.nodes[0].expanded);
        assert_eq!(sel(&s), "a");
    }

    #[test]
    fn repliage_avec_selection_dessous_la_garde() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::End);
        assert_eq!(sel(&s), "f.txt");
        // Repli d'un dossier au-dessus de la sélection : elle suit sa ligne.
        collapse(&mut s.explorer, 0);
        assert_eq!(sel(&s), "f.txt");
    }

    #[test]
    fn gauche_sur_enfant_remonte_au_parent() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::Down);
        press(&mut s, Key::Down);
        assert_eq!(sel(&s), "y.rs");
        press(&mut s, Key::Left);
        assert_eq!(sel(&s), "a");
        // Gauche sur un nœud de premier niveau replié : rien.
        press(&mut s, Key::Left);
        press(&mut s, Key::Left);
        assert_eq!(sel(&s), "a");
    }

    #[test]
    fn navigation_bornee_et_defilement() {
        let mut s = setup();
        let many: Vec<String> = (0..20).map(|i| format!("f{i:02}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        list(&mut s, ROOT.into(), &refs);
        let at = |s: &State| (s.explorer.selected, s.explorer.scroll);
        press(&mut s, Key::Up);
        assert_eq!(at(&s), (0, 0));
        press(&mut s, Key::PageDown);
        assert_eq!(at(&s), (7, 1));
        press(&mut s, Key::End);
        assert_eq!(at(&s), (19, 13));
        press(&mut s, Key::Down);
        press(&mut s, Key::PageDown);
        assert_eq!(at(&s), (19, 13));
        press(&mut s, Key::PageUp);
        assert_eq!(at(&s), (12, 12));
        press(&mut s, Key::Up);
        assert_eq!(at(&s), (11, 11));
        press(&mut s, Key::Home);
        assert_eq!(at(&s), (0, 0));
    }

    #[test]
    fn invite_reduit_la_hauteur_visible() {
        let mut s = setup();
        let many: Vec<String> = (0..20).map(|i| format!("f{i:02}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        list(&mut s, ROOT.into(), &refs);
        for _ in 0..6 {
            press(&mut s, Key::Down);
        }
        assert_eq!((s.explorer.selected, s.explorer.scroll), (6, 0));
        // L'invite prend la dernière ligne : la sélection reste visible.
        press(&mut s, Key::Char('n'));
        assert_eq!(s.explorer.scroll, 1);
    }

    #[test]
    fn arbre_vide_sans_panique() {
        let mut s = setup();
        list(&mut s, ROOT.into(), &[]);
        for k in [
            Key::Up,
            Key::Down,
            Key::End,
            Key::PageDown,
            Key::Left,
            Key::Right,
            Key::Enter,
        ] {
            assert!(press(&mut s, k).is_empty());
        }
        assert_eq!((s.explorer.selected, s.explorer.scroll), (0, 0));
        // Rien à renommer ; création à la racine.
        press(&mut s, Key::F(2));
        assert!(s.explorer.prompt.is_none());
        press(&mut s, Key::Char('n'));
        typ(&mut s, "x");
        assert_eq!(press(&mut s, Key::Enter), vec![Command::CreateFile(p("x"))]);
    }

    #[test]
    fn sans_racine_pas_d_invite() {
        let mut s = State {
            focus: Focus::Explorer,
            ..State::default()
        };
        press(&mut s, Key::Char('n'));
        assert!(s.explorer.prompt.is_none());
        assert_eq!(s.status, "Aucun dossier ouvert");
    }

    #[test]
    fn entree_sur_fichier_ouvre_et_rend_le_focus() {
        let mut s = setup();
        press(&mut s, Key::End);
        assert_eq!(
            press(&mut s, Key::Enter),
            vec![Command::OpenFile(p("f.txt"))]
        );
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn creation_dans_le_dossier_le_parent_ou_la_racine() {
        let mut s = setup();
        expand_a(&mut s);
        // Dossier sélectionné : dedans.
        press(&mut s, Key::Char('n'));
        assert_eq!(prompt(&s), (PromptKind::NewFile, ""));
        typ(&mut s, "new.rs");
        let cmds = press(&mut s, Key::Enter);
        assert_eq!(cmds, vec![Command::CreateFile(p("a/new.rs"))]);
        assert!(s.explorer.prompt.is_none());
        // Fichier sélectionné : dans son dossier.
        press(&mut s, Key::Down);
        press(&mut s, Key::Down);
        update(&mut s, Event::Key(kin(Key::Char('N'), false, false, true)));
        assert_eq!(prompt(&s), (PromptKind::NewDir, ""));
        typ(&mut s, "d");
        assert_eq!(
            press(&mut s, Key::Enter),
            vec![Command::CreateDir(p("a/d"))]
        );
        // Fichier de premier niveau : à la racine.
        press(&mut s, Key::End);
        press(&mut s, Key::Char('n'));
        typ(&mut s, "g");
        assert_eq!(press(&mut s, Key::Enter), vec![Command::CreateFile(p("g"))]);
    }

    #[test]
    fn noms_invalides_refuses() {
        let mut s = setup();
        press(&mut s, Key::Char('n'));
        for bad in ["", "  ", "x/y", ".", ".."] {
            input(&mut s, bad);
            assert!(press(&mut s, Key::Enter).is_empty(), "{bad:?}");
            assert!(s.explorer.prompt.is_some());
            assert!(!s.status.is_empty());
        }
        // Retour arrière, espaces retirés, puis nom valide.
        input(&mut s, " ok!");
        press(&mut s, Key::Backspace);
        assert_eq!(
            press(&mut s, Key::Enter),
            vec![Command::CreateFile(p("a/ok"))]
        );
    }

    #[test]
    fn renommer_pre_rempli() {
        let mut s = setup();
        press(&mut s, Key::End);
        press(&mut s, Key::F(2));
        assert_eq!(prompt(&s), (PromptKind::Rename, "f.txt"));
        // Nom inchangé : refusé.
        assert!(press(&mut s, Key::Enter).is_empty());
        assert!(s.explorer.prompt.is_some());
        for _ in 0..3 {
            press(&mut s, Key::Backspace);
        }
        typ(&mut s, "md");
        let (from, to) = (p("f.txt"), p("f.md"));
        assert_eq!(
            press(&mut s, Key::Enter),
            vec![Command::Rename { from, to }]
        );
        // `/` interdit.
        press(&mut s, Key::Char('r'));
        input(&mut s, "a/f.txt");
        assert!(press(&mut s, Key::Enter).is_empty());
    }

    #[test]
    fn renommer_un_dossier_vers_lui_meme_refuse() {
        let mut s = setup();
        press(&mut s, Key::Char('r'));
        assert_eq!(prompt(&s), (PromptKind::Rename, "a"));
        assert!(press(&mut s, Key::Enter).is_empty());
        assert!(s.explorer.prompt.is_some());
    }

    #[test]
    fn deplacer_chemin_relatif() {
        let mut s = setup();
        press(&mut s, Key::End);
        press(&mut s, Key::Char('m'));
        assert_eq!(prompt(&s), (PromptKind::Move, "f.txt"));
        let from = p("f.txt");
        let mv = |s: &mut State, to: &str| {
            input(s, to);
            press(s, Key::Enter)
        };
        let rn = |to| {
            vec![Command::Rename {
                from: p("f.txt"),
                to,
            }]
        };
        // Dossier connu : dedans.
        assert_eq!(mv(&mut s, "a"), rn(p("a/f.txt")));
        // Inconnu avec `/` final : dedans ; sans : chemin final.
        press(&mut s, Key::Char('m'));
        assert_eq!(mv(&mut s, "new/"), rn(p("new/f.txt")));
        press(&mut s, Key::Char('m'));
        assert_eq!(mv(&mut s, "./b/../b/g.txt"), rn(p("b/g.txt")));
        // Refusés : hors racine, absolu, identique, racine (déjà là), vide.
        press(&mut s, Key::Char('m'));
        for bad in ["../x", "a/../../x", "/tmp/x", "f.txt", ".", ""] {
            assert!(mv(&mut s, bad).is_empty(), "{bad:?}");
            assert!(s.explorer.prompt.is_some());
        }
        assert_eq!(s.explorer.prompt_target, from);
    }

    #[test]
    fn deplacer_un_dossier_dans_lui_meme_refuse() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::Char('m'));
        input(&mut s, "a/x");
        assert!(press(&mut s, Key::Enter).is_empty());
        assert!(s.status.contains("lui-même"));
    }

    #[test]
    fn corbeille_confirmee_ou_annulee() {
        let mut s = setup();
        press(&mut s, Key::End);
        press(&mut s, Key::Delete);
        assert_eq!(prompt(&s).0, PromptKind::ConfirmTrash);
        // Autre touche : ignorée, l'invite reste.
        assert!(press(&mut s, Key::Char('x')).is_empty());
        assert_eq!(prompt(&s), (PromptKind::ConfirmTrash, ""));
        assert_eq!(
            press(&mut s, Key::Char('o')),
            vec![Command::Trash(p("f.txt"))]
        );
        for (cancel, confirm) in [(Key::Char('n'), Key::Char('y')), (Key::Esc, Key::Enter)] {
            press(&mut s, Key::Delete);
            assert!(press(&mut s, cancel).is_empty());
            assert!(s.explorer.prompt.is_none());
            assert_eq!(s.focus, Focus::Explorer);
            press(&mut s, Key::Delete);
            assert_eq!(press(&mut s, confirm), vec![Command::Trash(p("f.txt"))]);
        }
    }

    #[test]
    fn echap_ferme_l_invite_puis_rend_le_focus() {
        let mut s = setup();
        press(&mut s, Key::Char('n'));
        typ(&mut s, "abc");
        press(&mut s, Key::Esc);
        assert!(s.explorer.prompt.is_none());
        assert_eq!(s.focus, Focus::Explorer);
        press(&mut s, Key::Esc);
        assert_eq!(s.focus, Focus::Editor);
        assert_eq!(s.doc.rope().to_string(), "");
    }

    #[test]
    fn fs_done_creation_reliste_et_ouvre() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::Char('n'));
        typ(&mut s, "new.rs");
        press(&mut s, Key::Enter);
        let done = update(&mut s, Event::FsDone(Ok(FsChange::Created(p("a/new.rs")))));
        assert_eq!(done, vec![ls(p("a")), Command::OpenFile(p("a/new.rs"))]);
        assert_eq!(s.focus, Focus::Editor);
        // Le fichier créé est sélectionné dès que le listage le montre.
        list(&mut s, p("a"), &["a/x/", "a/new.rs", "a/y.rs"]);
        assert_eq!(sel(&s), "new.rs");
        assert!(s.explorer.reveal.is_none());
    }

    #[test]
    fn fs_done_dossier_cree_dans_un_dossier_replie_le_deplie() {
        let mut s = setup();
        update(&mut s, Event::Key(kin(Key::Char('N'), false, false, true)));
        typ(&mut s, "d");
        press(&mut s, Key::Enter);
        let done = update(&mut s, Event::FsDone(Ok(FsChange::Created(p("a/d")))));
        // Dossier : pas d'ouverture.
        assert_eq!(done, vec![ls(p("a"))]);
        assert_eq!(s.focus, Focus::Explorer);
        assert!(s.explorer.nodes[0].loading);
        list(&mut s, p("a"), &["a/d/"]);
        assert!(s.explorer.nodes[0].expanded);
        assert_eq!(sel(&s), "d");
    }

    #[test]
    fn reveal_absent_du_listage_garde_la_selection() {
        let mut s = setup();
        press(&mut s, Key::End);
        s.explorer.reveal = Some(p("b/new"));
        // La racine est listée avant `b` : f.txt reste sélectionné, reveal attend.
        list(&mut s, ROOT.into(), &["a/", "aa", "b/", "f.txt"]);
        assert_eq!(sel(&s), "f.txt");
        assert_eq!(s.explorer.reveal, Some(p("b/new")));
    }

    #[test]
    fn fs_done_renommage_met_a_jour_les_onglets() {
        let mut s = setup();
        expand_a(&mut s);
        s.doc.path = Some(p("a/y.rs"));
        s.tabs.push(crate::Tab {
            doc: crate::Document::from_text(Some(p("a/x/z")), ""),
            scroll_line: 0,
            scroll_col: 0,
        });
        let (from, to) = (p("a"), p("b/a"));
        let done = update(&mut s, Event::FsDone(Ok(FsChange::Renamed { from, to })));
        assert_eq!(done, vec![ls(ROOT.into()), ls(p("b"))]);
        assert!(s.explorer.nodes[3].loading, "b se dépliera");
        assert_eq!(s.doc.path, Some(p("b/a/y.rs")));
        assert_eq!(s.tabs[0].doc.path, Some(p("b/a/x/z")));
        assert_eq!(s.explorer.reveal, Some(p("b/a")));
        // Fichier renommé lui-même : pas de `/` final ; parent commun listé une fois.
        let (from, to) = (p("b/a/y.rs"), p("b/a/w.rs"));
        update(&mut s, Event::FsDone(Ok(FsChange::Renamed { from, to })));
        assert_eq!(s.doc.path, Some(p("b/a/w.rs")));
        // Préfixe de nom, pas de chemin : intact.
        let (from, to) = (p("b/a/w"), p("b/a/v"));
        update(&mut s, Event::FsDone(Ok(FsChange::Renamed { from, to })));
        assert_eq!(s.doc.path, Some(p("b/a/w.rs")));
    }

    #[test]
    fn fs_done_parent_inconnu_pas_de_listage() {
        let mut s = setup();
        let done = update(&mut s, Event::FsDone(Ok(FsChange::Created(p("a/x/z")))));
        assert!(done.is_empty());
        assert_eq!(s.status, "Créé : a/x/z");
    }

    #[test]
    fn fs_done_corbeille_et_erreur() {
        let mut s = setup();
        s.doc.path = Some(p("a/y.rs"));
        let done = update(&mut s, Event::FsDone(Ok(FsChange::Trashed(p("a")))));
        assert_eq!(done, vec![ls(ROOT.into())]);
        assert!(s.status.contains("Ctrl+S le recréera"), "{}", s.status);
        // Erreur : statut, pas d'ouverture différée.
        press(&mut s, Key::Char('n'));
        typ(&mut s, "z");
        press(&mut s, Key::Enter);
        assert!(s.explorer.pending_open.is_some());
        assert!(update(&mut s, Event::FsDone(Err("refusé".into()))).is_empty());
        assert_eq!(s.status, "Échec : refusé");
        assert!(s.explorer.pending_open.is_none());
    }

    #[test]
    fn erreur_de_listage() {
        let mut s = setup();
        let err = |dir| Event::DirListed {
            dir,
            result: Err("permission".into()),
        };
        // Sous-dossier : statut, l'arbre ne bouge pas.
        press(&mut s, Key::Right);
        update(&mut s, err(p("a")));
        assert!(s.status.contains("permission"));
        assert!(!s.explorer.nodes[0].loading && !s.explorer.nodes[0].expanded);
        assert_eq!(s.explorer.nodes.len(), 3);
        // Racine : erreur dans le panneau, arbre vidé ; une réussite l'efface.
        update(&mut s, err(ROOT.into()));
        assert_eq!(s.explorer.error.as_deref(), Some("permission"));
        assert!(s.explorer.nodes.is_empty());
        list(&mut s, ROOT.into(), &["a/"]);
        assert!(s.explorer.error.is_none());
        assert_eq!(s.explorer.nodes.len(), 1);
        // Nouvelle racine : tout repart de zéro.
        update(&mut s, err(ROOT.into()));
        update(&mut s, Event::OpenRoot("/autre".into()));
        assert!(s.explorer.error.is_none() && s.explorer.nodes.is_empty());
    }

    #[test]
    fn relistage_de_la_racine_garde_les_dossiers_deplies() {
        let mut s = setup();
        expand_a(&mut s);
        press(&mut s, Key::End);
        // `b` disparu, `c` ajouté : `a` reste déplié avec son contenu, la sélection suit f.txt.
        list(&mut s, ROOT.into(), &["a/", "c", "f.txt"]);
        let all = vec![n("a", 0), n("x", 1), n("y.rs", 1), n("c", 0), n("f.txt", 0)];
        assert_eq!(names(&s), all);
        assert_eq!(sel(&s), "f.txt");
        // Sélection disparue : l'index est borné.
        list(&mut s, ROOT.into(), &["a/"]);
        assert_eq!(s.explorer.selected, 2);
        // `a` devenu fichier : plus d'enfants.
        list(&mut s, ROOT.into(), &["a"]);
        assert_eq!(names(&s), vec![n("a", 0)]);
        assert_eq!(s.explorer.selected, 0);
    }

    #[test]
    fn disque_change_reliste_les_dossiers_connus() {
        let mut s = setup();
        expand_a(&mut s);
        let paths = [
            "f.txt",
            "nouveau",
            "a/y.rs",
            "a/z",
            "b/w",
            "a/x/q",
            "ailleurs/k",
        ];
        let cmds = update(&mut s, Event::DiskChanged(paths.map(p).to_vec()));
        // Racine et `a` (déplié) une fois chacun ; `b` et `a/x` repliés, `ailleurs` inconnu : rien.
        assert_eq!(cmds, vec![ls(ROOT.into()), ls(p("a"))]);
    }

    #[test]
    fn fichiers_caches_relistent() {
        let mut s = setup();
        expand_a(&mut s);
        let cmds = press(&mut s, Key::Char('.'));
        let hidden = |dir| Command::ListDir {
            dir,
            show_hidden: true,
        };
        assert_eq!(cmds, vec![hidden(ROOT.into()), hidden(p("a"))]);
        assert!(s.explorer.show_hidden);
        // Les dépliages suivants suivent le réglage.
        press(&mut s, Key::Down);
        assert_eq!(press(&mut s, Key::Right), vec![hidden(p("a/x"))]);
    }

    #[test]
    fn ctrl_b_masque_et_ctrl_e_bascule() {
        let mut s = setup();
        press(&mut s, Key::Char('n'));
        let ctrl = |c| Event::Key(kin(Key::Char(c), true, false, false));
        assert!(update(&mut s, ctrl('b')).is_empty());
        assert!(!s.explorer.visible && s.explorer.prompt.is_none());
        assert_eq!(s.focus, Focus::Editor);
        // Ctrl+E réaffiche et donne le focus, puis le rend.
        update(&mut s, ctrl('e'));
        assert!(s.explorer.visible);
        assert_eq!(s.focus, Focus::Explorer);
        update(&mut s, ctrl('E'));
        assert_eq!(s.focus, Focus::Editor);
        update(&mut s, ctrl('b'));
        update(&mut s, ctrl('B'));
        assert!(s.explorer.visible);
        assert_eq!(s.focus, Focus::Editor);
    }

    #[test]
    fn touches_consommees_ou_laissees_a_l_editeur() {
        let mut s = setup();
        let plain = |c| kin(Key::Char(c), false, false, false);
        for c in ['z', 'q', ' '] {
            assert_eq!(key(&mut s, plain(c)), Some(vec![]));
        }
        assert_eq!(
            key(&mut s, kin(Key::Backspace, false, false, false)),
            Some(vec![])
        );
        assert_eq!(
            key(&mut s, kin(Key::Tab, false, false, false)),
            Some(vec![])
        );
        assert_eq!(s.doc.rope().to_string(), "");
        let ctrl = |c| kin(Key::Char(c), true, false, false);
        assert_eq!(key(&mut s, ctrl('q')), None);
        assert_eq!(key(&mut s, ctrl('s')), None);
        let alt1 = kin(Key::Char('1'), false, true, false);
        assert_eq!(key(&mut s, alt1), None);
        // Pendant une invite aussi.
        press(&mut s, Key::Char('n'));
        assert_eq!(key(&mut s, alt1), None);
        assert_eq!(key(&mut s, ctrl('s')), None);
        assert_eq!(prompt(&s).1, "");
        assert_eq!(update(&mut s, Event::Key(ctrl('q'))), vec![Command::Quit]);
        // Focus éditeur : rien n'est pris, sauf Ctrl+B / Ctrl+E.
        s.focus = Focus::Editor;
        assert_eq!(key(&mut s, kin(Key::Down, false, false, false)), None);
        assert_eq!(key(&mut s, plain('n')), None);
    }

    #[test]
    fn largeur_bornee() {
        let mut s = setup();
        for _ in 0..50 {
            press(&mut s, Key::Char('>'));
        }
        assert_eq!(s.explorer.width, 80);
        for _ in 0..50 {
            update(&mut s, Event::Key(kin(Key::Left, true, true, false)));
        }
        assert_eq!(s.explorer.width, 12);
        update(&mut s, Event::Key(kin(Key::Right, true, true, false)));
        assert_eq!(s.explorer.width, 14);
        press(&mut s, Key::Char('<'));
        press(&mut s, Key::Char('<'));
        assert_eq!(s.explorer.width, 12);
        // Terminal étroit : l'éditeur garde 20 colonnes.
        s.width = 40;
        for _ in 0..50 {
            press(&mut s, Key::Char('>'));
        }
        assert_eq!(s.explorer.width, 20);
        // Très étroit : minimum 12.
        s.width = 10;
        press(&mut s, Key::Char('>'));
        assert_eq!(s.explorer.width, 12);
    }

    #[test]
    fn clic_pendant_une_invite_l_annule() {
        let mut s = setup();
        press(&mut s, Key::Delete);
        assert!(s.explorer.prompt.is_some());
        // Clic sur une autre ligne : l'invite se ferme au lieu de changer de cible en silence.
        click(&mut s, MouseKind::Down, 5, 3);
        assert!(s.explorer.prompt.is_none());
        assert!(press(&mut s, Key::Char('o')).is_empty());
    }

    #[test]
    fn souris_clics() {
        let mut s = setup();
        s.focus = Focus::Editor;
        // Clic sur `a` (ligne 1 = premier nœud) : déplie, focus explorateur.
        assert_eq!(click(&mut s, MouseKind::Down, 3, 1), vec![ls(p("a"))]);
        assert_eq!(s.focus, Focus::Explorer);
        list(&mut s, p("a"), &["a/y.rs"]);
        // Fichier : ouvert, sélectionné.
        let open = vec![Command::OpenFile(p("a/y.rs"))];
        assert_eq!(click(&mut s, MouseKind::Down, 3, 2), open);
        assert_eq!(sel(&s), "y.rs");
        // Ligne vide, bordures (haut, gauche, droite, bas) : rien.
        for (col, row) in [(3, 6), (3, 0), (0, 2), (29, 2), (3, 8)] {
            assert!(click(&mut s, MouseKind::Down, col, row).is_empty());
            assert_eq!(sel(&s), "y.rs");
        }
        // Glisser : laissé à l'éditeur.
        assert_eq!(mouse(&mut s, mi(MouseKind::Drag, 3, 2)), None);
        // Hors du panneau ou sur le statut : laissé à l'éditeur, focus éditeur.
        assert_eq!(mouse(&mut s, mi(MouseKind::Down, 30, 2)), None);
        assert_eq!(s.focus, Focus::Editor);
        assert_eq!(mouse(&mut s, mi(MouseKind::Down, 3, 9)), None);
        assert_eq!(names(&s).len(), 4);
        // Panneau masqué : tout va à l'éditeur.
        s.explorer.visible = false;
        assert_eq!(mouse(&mut s, mi(MouseKind::Down, 3, 1)), None);
    }

    #[test]
    fn souris_molette() {
        let mut s = setup();
        let many: Vec<String> = (0..10).map(|i| format!("f{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        list(&mut s, ROOT.into(), &refs);
        click(&mut s, MouseKind::ScrollDown, 3, 3);
        assert_eq!(s.explorer.scroll, 3);
        click(&mut s, MouseKind::ScrollDown, 3, 3);
        assert_eq!(s.explorer.scroll, 3, "borné à len − hauteur");
        click(&mut s, MouseKind::ScrollUp, 3, 3);
        click(&mut s, MouseKind::ScrollUp, 3, 3);
        assert_eq!(s.explorer.scroll, 0);
        // Clic après défilement : ligne 1 = nœud `scroll`.
        click(&mut s, MouseKind::ScrollDown, 3, 3);
        click(&mut s, MouseKind::Down, 3, 1);
        assert_eq!(sel(&s), "f3");
    }
}
