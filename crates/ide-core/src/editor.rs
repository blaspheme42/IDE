//! Traduction des événements en actions sur l'état (clavier, fichiers, défilement).

use crate::{
    Command, Document, Event, Key, KeyInput, MouseInput, MouseKind, State, TAB_BAR_HEIGHT, backup,
    editor_size, explorer, lsp, palette, search, tabs,
};

/// Colonnes réservées par l'UI à gauche du texte : marque git, numéro (au moins 4 chiffres) et espace
/// si `line_numbers`, marque de repli (`-` / `+`), colonne de diagnostics.
pub fn gutter_width(state: &State) -> u16 {
    let numbers = if state.settings.line_numbers {
        state.doc.rope().len_lines().to_string().len().max(4) + 1
    } else {
        0
    };
    u16::try_from(numbers + 3).unwrap_or(u16::MAX)
}

/// Lignes défilées par cran de molette.
const WHEEL_LINES: usize = 3;

pub(crate) fn update(state: &mut State, event: Event) -> Vec<Command> {
    // La molette déplace la vue sans le curseur : ne pas la ramener sur lui ensuite.
    let wheel = matches!(&event, Event::Mouse(m) if matches!(m.kind, MouseKind::ScrollUp | MouseKind::ScrollDown));
    let version = state.doc.version();
    let mut cmds = match event {
        Event::Key(k) => key(state, k),
        Event::Mouse(m) if state.help.is_some() => {
            match m.kind {
                MouseKind::ScrollUp => scroll_help(state, -(WHEEL_LINES as isize)),
                MouseKind::ScrollDown => scroll_help(state, WHEEL_LINES as isize),
                _ => {}
            }
            vec![]
        }
        Event::Mouse(m) => explorer::mouse(state, m).unwrap_or_else(|| {
            mouse(state, m);
            vec![]
        }),
        Event::OpenRoot(root) => explorer::open_root(state, root),
        Event::DirListed { dir, result } => {
            explorer::listed(state, dir, result);
            vec![]
        }
        Event::DiskChanged(paths) => {
            let mut cmds = explorer::disk_changed(state, &paths);
            cmds.extend(tabs::disk_changed(state, &paths));
            cmds
        }
        Event::FileChanged { path, text } => {
            tabs::file_changed(state, path, text);
            vec![]
        }
        Event::FileList(files) => {
            state.project_files = files;
            palette::refresh(state);
            vec![]
        }
        Event::GitBranch(branch) => {
            state.git_branch = branch;
            vec![]
        }
        Event::GitBase { path, base } => {
            match base {
                Some(b) => state.git_bases.insert(path, b),
                None => state.git_bases.remove(&path),
            };
            vec![]
        }
        Event::Diagnostics { path, items } => {
            state.diagnostics.insert(path, items);
            vec![]
        }
        Event::CompletionResult {
            path,
            version,
            items,
            incomplete,
        } => {
            lsp::completion_result(state, path, version, items, incomplete);
            vec![]
        }
        Event::HoverResult { path, text } => {
            lsp::hover_result(state, path, text);
            vec![]
        }
        Event::DefinitionResult(locations) => lsp::definition_result(state, locations),
        Event::FormatResult {
            path,
            version,
            edits,
            save,
        } => lsp::format_result(state, path, version, edits, save),
        Event::LspStatus(s) => {
            state.lsp_status = s;
            vec![]
        }
        Event::ConfigLoaded { settings, errors } => {
            state.explorer.width = settings.explorer_width;
            state.explorer.show_hidden = settings.show_hidden;
            if !errors.is_empty() {
                state.status = format!("Configuration : {}", errors.join(" ; "));
            }
            state.settings = settings;
            vec![]
        }
        Event::FsDone(result) => {
            // `fs_done` écrase le statut : la note des onglets est ajoutée après.
            state.status.clear();
            if let Ok(change) = &result {
                tabs::on_fs_change(state, change);
            }
            let note = std::mem::take(&mut state.status);
            let cmds = explorer::fs_done(state, result);
            if !note.is_empty() {
                state.status = format!("{} — {note}", state.status);
            }
            cmds
        }
        Event::Paste(text) if state.palette.is_some() => {
            palette::paste(state, &text);
            vec![]
        }
        Event::Paste(text) if state.focus == crate::Focus::Explorer => {
            explorer::paste(state, &text);
            vec![]
        }
        Event::Paste(text) if state.search.is_some() => {
            search::input(state, text.lines().next().unwrap_or_default());
            vec![]
        }
        // Presse-papiers vide : rien à coller, la sélection reste.
        Event::Paste(text) if text.is_empty() => vec![],
        // Ligne entière copiée sans sélection : collée au-dessus de la ligne courante, comme VSCode.
        Event::Paste(text)
            if state.doc.selection().is_none() && state.line_clip.as_ref() == Some(&text) =>
        {
            let doc = &mut state.doc;
            let cursor = doc.cursor();
            let start = doc.rope().line_to_char(doc.rope().char_to_line(cursor));
            doc.edit(start..start, &text);
            doc.set_cursor(cursor + text.chars().count(), false);
            vec![]
        }
        Event::Paste(text) => {
            state.doc.insert_str(&text);
            vec![]
        }
        Event::Resize { width, height } => {
            state.width = width;
            state.height = height;
            vec![]
        }
        Event::Tick => vec![],
        Event::BackupFound { path, text } => {
            backup::found(state, path, text);
            vec![]
        }
        Event::RestoreView {
            path,
            cursor,
            scroll_line,
        } => {
            backup::restore_view(state, &path, cursor, scroll_line);
            vec![]
        }
        Event::ActivateTab(path) => {
            backup::activate_tab(state, &path);
            vec![]
        }
        Event::FileOpened { path, text } => {
            if tabs::open(state, path, |p| Document::from_text(Some(p), &text)) {
                state.status = format!("Ouvert : {}", state.doc.display_name());
            }
            lsp::apply_pending_jump(state);
            vec![]
        }
        Event::IndentDetected { path, indent } => {
            if let Some(doc) = tabs::doc_mut(state, &path) {
                doc.indent = indent;
            }
            vec![]
        }
        Event::OpenFailed {
            path,
            error,
            create,
        } => {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            if create {
                tabs::open(state, path, |p| Document::from_text(Some(p), ""));
            }
            state.status = format!("Impossible d'ouvrir {name} : {error}");
            vec![]
        }
        Event::Saved { path, version } => {
            if let Some(doc) = tabs::saved_doc(state, &path, version) {
                doc.mark_saved(version);
                state.status = format!("Sauvegardé : {}", doc.display_name());
            }
            vec![]
        }
        Event::SaveFailed { error, .. } => {
            state.status = format!("Échec de la sauvegarde : {error}");
            vec![]
        }
    };
    // Annuler, couper, remplacer… : les occurrences ont bougé.
    if state.search.is_some() && state.doc.version() != version {
        search::refresh(state, false);
    }
    cmds.extend(lsp::after_update(state, version));
    // Curseur amené dans un bloc replié (recherche, F12, annuler…) : on le déplie.
    let line = state.doc.cursor_line_col().0;
    state.doc.reveal(line);
    if !wheel {
        scroll_to_cursor(state);
    }
    cmds
}

/// Écriture du texte courant de `doc` dans `path`.
pub(crate) fn save_command(doc: &mut Document, path: std::path::PathBuf) -> Command {
    let contents = doc.to_saved_text();
    // Le watcher verra cette écriture : ce n'est pas un changement venu d'ailleurs.
    doc.remember_disk(&contents);
    Command::Save {
        path,
        contents,
        version: doc.version(),
    }
}

fn mouse(state: &mut State, m: MouseInput) {
    let left = crate::explorer_width(state) + gutter_width(state);
    // Ligne 0 : onglets, puis le bandeau de copie de secours s'il est affiché, puis le texte.
    let top = TAB_BAR_HEIGHT + crate::banner_height(state);
    let bottom = top + editor_size(state).1;
    match m.kind {
        MouseKind::Down if m.row < TAB_BAR_HEIGHT => {
            state.dragging = false;
            let tab = tabs::tab_spans(state)
                .into_iter()
                .find(|(r, _)| r.contains(&m.column));
            if let Some(i) = tabs::close_mark_at(state, m.column) {
                // Activé d'abord : la fermeture vise toujours l'onglet actif.
                tabs::activate(state, i);
                tabs::request_close(state);
            } else if let Some((_, i)) = tab {
                tabs::activate(state, i);
            }
        }
        MouseKind::ScrollUp => {
            state.scroll_line = state
                .doc
                .step_visible(state.scroll_line, WHEEL_LINES, false);
        }
        MouseKind::ScrollDown => {
            state.scroll_line = state.doc.step_visible(state.scroll_line, WHEEL_LINES, true);
        }
        MouseKind::Up => state.dragging = false,
        // Clic dans la gouttière : replie ou déplie le bloc de la ligne.
        MouseKind::Down
            if m.column < left
                && m.column >= crate::explorer_width(state)
                && (top..bottom).contains(&m.row) =>
        {
            state.dragging = false;
            let rows = usize::from(m.row - top) + 1;
            if let Some(&line) = state
                .doc
                .visible_lines(state.scroll_line, rows)
                .get(rows - 1)
            {
                state.doc.toggle_fold(line);
            }
        }
        // Un clic hors de la zone de texte est ignoré ; un glisser peut en sortir, s'il y a commencé.
        MouseKind::Down if m.column < left || m.row < top || m.row >= bottom => {
            state.dragging = false;
        }
        MouseKind::Drag if !state.dragging => {}
        MouseKind::Down | MouseKind::Drag => {
            state.dragging = true;
            let row = usize::from(m.row.saturating_sub(top));
            let line = state.doc.step_visible(state.scroll_line, row, true);
            let col = state.scroll_col + usize::from(m.column.saturating_sub(left));
            let pos = state.doc.pos_at(line, col);
            let extend = m.kind == MouseKind::Drag || m.mods.shift;
            state.doc.set_cursor(pos, extend);
        }
    }
}

/// Texte copié et plage à couper : la sélection, sinon la ligne courante avec son `\n` (comme VSCode).
/// `true` = ligne entière.
fn copied(doc: &Document) -> (std::ops::Range<usize>, String, bool) {
    if let Some(r) = doc.selection() {
        return (r.clone(), doc.rope().slice(r).to_string(), false);
    }
    let rope = doc.rope();
    let line = rope.char_to_line(doc.cursor());
    let start = rope.line_to_char(line);
    let r = if line + 1 < rope.len_lines() {
        start..rope.line_to_char(line + 1)
    } else {
        // Dernière ligne sans `\n` : on coupe le saut de ligne qui la précède, pas de ligne vide qui reste.
        start.saturating_sub(1)..rope.len_chars()
    };
    let mut text = rope.slice(start..r.end).to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    (r, text, true)
}

/// Défile l'aide de `delta` lignes, sans dépasser sa dernière page.
fn scroll_help(state: &mut State, delta: isize) {
    let max = crate::help_len().saturating_sub(crate::help_rows(state));
    if let Some(top) = &mut state.help {
        *top = top.saturating_add_signed(delta).min(max);
    }
}

/// F1 ouvre / ferme l'aide ; ouverte, elle prend toutes les touches sauf Ctrl+Q. `None` = touche laissée à la suite.
fn help_key(state: &mut State, k: KeyInput) -> Option<Vec<Command>> {
    let plain = k.mods == crate::Modifiers::default();
    if k.key == Key::F(1) && plain {
        state.help = state.help.is_none().then_some(0);
        return Some(vec![]);
    }
    state.help?;
    let page = crate::help_rows(state) as isize;
    match k.key {
        Key::Esc => state.help = None,
        Key::Up => scroll_help(state, -1),
        Key::Down => scroll_help(state, 1),
        Key::PageUp => scroll_help(state, -page),
        Key::PageDown => scroll_help(state, page),
        Key::Home => state.help = Some(0),
        Key::End => scroll_help(state, isize::MAX),
        Key::Char('q' | 'Q') if k.mods.ctrl && !k.mods.alt => return None,
        _ => {}
    }
    Some(vec![])
}

fn key(state: &mut State, mut k: KeyInput) -> Vec<Command> {
    // AltGr arrive comme Ctrl+Alt sur certains terminaux (AZERTY : AltGr+4 = `{`) : c'est un caractère tapé.
    if let Key::Char(c) = k.key
        && k.mods.ctrl
        && k.mods.alt
        && !c.is_ascii_alphanumeric()
        && !c.is_control()
    {
        k.mods = crate::Modifiers::default();
    }
    // `[keys]` : la touche personnalisée rejoue le raccourci d'origine de sa commande.
    let replaying = std::mem::take(&mut state.replaying);
    let custom = (!replaying)
        .then(|| state.settings.keys.iter().find(|(key, _)| *key == k))
        .flatten();
    if let Some(info) = custom.and_then(|(_, name)| crate::command_by_name(name)) {
        k = info.key;
    }
    let ctrl = |c: char| k.mods.ctrl && !k.mods.alt && k.key == Key::Char(c);
    if !(ctrl('q') || ctrl('Q')) && state.quit_armed {
        state.quit_armed = false;
        state.status.clear();
    }
    if !(ctrl('w') || ctrl('W')) && state.close_armed {
        state.close_armed = false;
        state.status.clear();
    }
    // Aide (F1) : modale, au-dessus de tout, même de la palette ; Ctrl+Q passe.
    if let Some(cmds) = help_key(state, k) {
        return cmds;
    }
    // Palette ouverte : modale, elle passe avant tout le reste.
    if let Some(cmds) = palette::key(state, k) {
        return cmds;
    }
    if let Some(cmds) = explorer::key(state, k) {
        return cmds;
    }
    if let Some(cmds) = lsp::key(state, k) {
        return cmds;
    }
    // Avant la recherche, qui prend Tab pour changer de champ.
    if tabs::key(state, k) {
        return vec![];
    }
    if search::key(state, k) {
        return vec![];
    }
    // Après la recherche : ouverte, elle garde Alt+R (regex).
    if tabs::conflict_key(state, k) {
        return vec![];
    }
    if let Some(cmds) = backup::key(state, k) {
        return cmds;
    }
    let page = usize::from(editor_size(state).1).max(1);
    let doc = &mut state.doc;
    if k.mods.ctrl && !k.mods.alt {
        match k.key {
            Key::Char('q' | 'Q') => {
                let n = tabs::dirty_count(state);
                if n == 0 || state.quit_armed {
                    return vec![Command::Quit];
                }
                state.quit_armed = true;
                let s = if n > 1 { "s" } else { "" };
                state.status = format!(
                    "Modifications non sauvegardées ({n} fichier{s}) : Ctrl+Q à nouveau pour quitter sans sauvegarder"
                );
            }
            Key::Char('s' | 'S') => match doc.path.clone() {
                Some(path) => {
                    state.status = "Sauvegarde…".into();
                    doc.seal_history();
                    if state.settings.format_on_save {
                        return vec![Command::RequestFormat {
                            path,
                            version: doc.version(),
                            indent: doc.indent,
                            save: true,
                        }];
                    }
                    return vec![save_command(doc, path)];
                }
                None => state.status = "Aucun fichier : lancez `ide <fichier>`".into(),
            },
            Key::Char('z' | 'Z') if !k.mods.shift => {
                if !doc.undo() {
                    state.status = "Rien à annuler".into();
                }
            }
            Key::Char('z' | 'Z' | 'y' | 'Y') => {
                if !doc.redo() {
                    state.status = "Rien à rétablir".into();
                }
            }
            Key::Char('a' | 'A') => doc.select_all(),
            Key::Char(c @ ('c' | 'C' | 'x' | 'X')) => {
                let (r, text, whole_line) = copied(doc);
                if c.eq_ignore_ascii_case(&'x') {
                    doc.edit(r, "");
                }
                state.line_clip = whole_line.then(|| text.clone());
                return vec![Command::CopyToClipboard(text)];
            }
            Key::Char('v' | 'V') => return vec![Command::RequestPaste],
            Key::Char('f' | 'F') => search::open(state, false),
            Key::Char('h' | 'H') => search::open(state, true),
            // Shift étend la sélection, comme pour les déplacements simples.
            Key::Home => doc.set_cursor(0, k.mods.shift),
            Key::End => doc.set_cursor(doc.rope().len_chars(), k.mods.shift),
            Key::Left => doc.move_word(false, k.mods.shift),
            Key::Right => doc.move_word(true, k.mods.shift),
            Key::Backspace => doc.delete_word_before(),
            _ => state.status = k.to_string(),
        }
        return vec![];
    }
    if k.mods.alt {
        // Alt+Retour arrière : la plupart des terminaux envoient Ctrl+Retour arrière comme Ctrl+H.
        match k.key {
            Key::Backspace if !k.mods.ctrl => doc.delete_word_before(),
            // Repli : `Alt+-` / `Alt+=` au curseur, avec Shift (`_` / `+`) pour tout le fichier.
            Key::Char('-') if !k.mods.shift => {
                if !doc.fold_at_cursor() {
                    state.status = "Aucun bloc à replier ici".into();
                }
            }
            Key::Char('=') if !k.mods.shift => {
                if !doc.unfold_at_cursor() {
                    state.status = "Aucun bloc replié sur cette ligne".into();
                }
            }
            Key::Char('_' | '-') => doc.fold_all(),
            Key::Char('+' | '=') => doc.unfold_all(),
            Key::Char('l' | 'L') => state.settings.line_numbers ^= true,
            Key::Char('t' | 'T') => {
                // Thème inconnu (rendu en sombre) : comme le premier.
                let i = crate::THEMES
                    .iter()
                    .position(|t| *t == state.settings.theme);
                let next = crate::THEMES[(i.unwrap_or(0) + 1) % crate::THEMES.len()];
                state.settings.theme = next.into();
                state.status = format!("Thème : {next}");
            }
            _ => state.status = k.to_string(),
        }
        return vec![];
    }
    let ext = k.mods.shift;
    match k.key {
        Key::Char(c) if !c.is_control() => {
            if !auto_pair(doc, c) {
                doc.insert_char(c);
            }
            if c == '>' {
                close_tag(doc);
            }
        }
        Key::Enter => doc.insert_newline(),
        Key::Tab if ext => doc.untab(),
        Key::Tab => {
            if !expand_abbreviation(doc) {
                doc.tab();
            }
        }
        Key::Backspace => {
            // Entre une paire vide `(|)` : retire les deux.
            let cur = doc.cursor();
            let rope = doc.rope();
            if doc.selection().is_none()
                && cur > 0
                && PAIRS.contains(&(rope.char(cur - 1), rope.get_char(cur).unwrap_or('\0')))
            {
                doc.edit(cur - 1..cur + 1, "");
            } else {
                doc.backspace();
            }
        }
        Key::Delete => doc.delete(),
        Key::Left => doc.move_left(ext),
        Key::Right => doc.move_right(ext),
        Key::Up => doc.move_up(1, ext),
        Key::Down => doc.move_down(1, ext),
        Key::Home => doc.move_line_start(ext),
        Key::End => doc.move_line_end(ext),
        Key::PageUp => doc.move_up(page, ext),
        Key::PageDown => doc.move_down(page, ext),
        _ => state.status = k.to_string(),
    }
    vec![]
}

/// Fait défiler juste assez pour que le curseur reste dans la zone de texte.
fn scroll_to_cursor(state: &mut State) {
    let (line, col) = state.doc.cursor_line_col();
    let (w, h) = editor_size(state);
    let h = usize::from(h).max(1);
    let w = usize::from(w.saturating_sub(gutter_width(state))).max(1);
    // En lignes visibles : un bloc replié n'occupe qu'une rangée.
    let doc = &state.doc;
    let top = doc.step_visible(state.scroll_line, 0, false);
    state.scroll_line = match doc.rows_between(top, line) {
        None => line.min(top),
        Some(rows) if rows >= h => doc.step_visible(line, h - 1, false),
        Some(_) => top,
    };
    state.scroll_col = state.scroll_col.clamp(col.saturating_sub(w - 1), col);
}

const PAIRS: [(char, char); 5] = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];

/// Paires automatiques : une ouvrante insère sa fermante derrière le curseur, et taper
/// une fermante déjà présente juste après le curseur la saute. `false` si frappe normale.
/// Guillemets seulement hors d'un mot (`l'arbre`), et pas `'` en Rust (durées de vie `'a`).
fn auto_pair(doc: &mut Document, c: char) -> bool {
    if doc.selection().is_some() {
        return false;
    }
    let cur = doc.cursor();
    let rope = doc.rope();
    let next = rope.get_char(cur);
    let quote = matches!(c, '"' | '\'');
    if PAIRS.iter().any(|&(_, close)| close == c) && next == Some(c) {
        doc.set_cursor(cur + 1, false);
        return true;
    }
    let Some(&(_, close)) = PAIRS.iter().find(|&&(open, _)| open == c) else {
        return false;
    };
    let word = |ch: Option<char>| ch.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    let prev = cur.checked_sub(1).map(|i| rope.char(i));
    let rust = doc
        .path
        .as_ref()
        .is_some_and(|p| p.extension().is_some_and(|e| e == "rs"));
    if word(next) || (quote && (word(prev) || (c == '\'' && rust))) {
        return false;
    }
    doc.edit(cur..cur, &format!("{c}{close}"));
    doc.set_cursor(cur + 1, false);
    true
}

fn is_html(doc: &Document) -> bool {
    doc.path
        .as_ref()
        .and_then(|p| p.extension()?.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "html" | "htm" | "php" | "phtml"
            )
        })
}

/// Après le `>` d'une balise ouvrante (`<div class="a">`), insère `</div>` derrière le curseur.
/// Rien pour les éléments vides (`<br>`), `/>`, `<!…>`, `<?php`, ni si la fermante suit déjà.
// shortcut: un `>` dans la valeur d'un attribut coupe la balise ; suffisant pour la frappe courante.
fn close_tag(doc: &mut Document) {
    const VOID: [&str; 14] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    if !is_html(doc) {
        return;
    }
    let rope = doc.rope();
    let gt = doc.cursor() - 1;
    let from = gt.saturating_sub(2000);
    let Some(lt) = (from..gt)
        .rev()
        .find(|&i| matches!(rope.char(i), '<' | '>'))
    else {
        return;
    };
    let inner = rope.slice(lt + 1..gt).to_string();
    if rope.char(lt) != '<' || inner.ends_with('/') {
        return;
    }
    let name: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.'))
        .collect();
    let starts_alpha = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest = &inner[name.len()..];
    if !starts_alpha
        || !(rest.is_empty() || rest.starts_with(char::is_whitespace))
        || VOID.contains(&name.to_ascii_lowercase().as_str())
    {
        return;
    }
    let closing = format!("</{name}>");
    let cursor = doc.cursor();
    let after = rope.slice(cursor..(cursor + closing.chars().count()).min(rope.len_chars()));
    if after == closing.as_str() {
        return;
    }
    doc.edit(cursor..cursor, &closing);
    doc.set_cursor(cursor, false);
}

/// Abréviations Emmet de base, seules avant le curseur dans un fichier HTML ou PHP :
/// `!` → page HTML5 (curseur dans `<body>`), `!!!` → doctype. `false` si rien à développer.
// shortcut: seulement `!` et `!!!` ; le reste d'Emmet (`div.a>li*3`) demanderait emmet-language-server en second serveur.
fn expand_abbreviation(doc: &mut Document) -> bool {
    if !is_html(doc) || doc.selection().is_some() {
        return false;
    }
    let cursor = doc.cursor();
    let start = doc.rope().line_to_char(doc.rope().char_to_line(cursor));
    let before = doc.rope().slice(start..cursor).to_string();
    let abbr = before.trim_start();
    let pad = &before[..before.len() - abbr.len()];
    let unit = doc.indent.unit();
    let (text, caret) = match abbr {
        "!!!" => ("<!DOCTYPE html>".to_string(), None),
        "!" => {
            let head = [
                "<!DOCTYPE html>".to_string(),
                "<html lang=\"fr\">".into(),
                "<head>".into(),
                format!("{unit}<meta charset=\"UTF-8\">"),
                format!(
                    "{unit}<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">"
                ),
                format!("{unit}<title>Document</title>"),
                "</head>".into(),
                "<body>".into(),
                unit.clone(),
            ]
            .join(&format!("\n{pad}"));
            let tail = format!("\n{pad}</body>\n{pad}</html>");
            (format!("{head}{tail}"), Some(head.chars().count()))
        }
        _ => return false,
    };
    let at = cursor - abbr.chars().count();
    doc.seal_history();
    doc.edit(at..cursor, &text);
    if let Some(caret) = caret {
        doc.set_cursor(at + caret, false);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Modifiers, MouseInput, MouseKind, update};
    use std::path::PathBuf;

    fn k(key: Key) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers::default(),
        })
    }
    fn ctrl(c: char) -> Event {
        Event::Key(KeyInput {
            key: Key::Char(c),
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        })
    }
    fn opened(text: &str) -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 40,
                height: 7,
            },
        );
        update(
            &mut s,
            Event::FileOpened {
                path: "/x/a.txt".into(),
                text: text.into(),
            },
        );
        s
    }

    #[test]
    fn ouverture_edition_sauvegarde() {
        let mut s = opened("ab\r\n");
        s.settings.format_on_save = false;
        assert_eq!(s.status, "Ouvert : a.txt");
        update(&mut s, k(Key::End));
        update(&mut s, k(Key::Char('c')));
        update(&mut s, k(Key::Tab));
        assert!(s.doc.is_dirty());
        let version = s.doc.version();
        let cmds = update(&mut s, ctrl('s'));
        assert_eq!(s.status, "Sauvegarde…");
        assert_eq!(
            cmds,
            vec![Command::Save {
                path: PathBuf::from("/x/a.txt"),
                contents: "abc \r\n".into(),
                version,
            }]
        );
        update(&mut s, k(Key::Char('d')));
        update(
            &mut s,
            Event::Saved {
                path: "/x/a.txt".into(),
                version,
            },
        );
        assert_eq!(s.status, "Sauvegardé : a.txt");
        assert!(s.doc.is_dirty());
        update(
            &mut s,
            Event::SaveFailed {
                path: "/x/a.txt".into(),
                error: "disque plein".into(),
            },
        );
        assert_eq!(s.status, "Échec de la sauvegarde : disque plein");
    }

    #[test]
    fn ctrl_s_sans_chemin() {
        let mut s = State::default();
        assert!(update(&mut s, ctrl('s')).is_empty());
        assert_eq!(s.status, "Aucun fichier : lancez `ide <fichier>`");
    }

    #[test]
    fn open_failed_garde_le_chemin() {
        let mut s = State::default();
        s.settings.format_on_save = false;
        update(
            &mut s,
            Event::OpenFailed {
                path: "/x/neuf.rs".into(),
                error: "introuvable".into(),
                create: true,
            },
        );
        assert_eq!(s.status, "Impossible d'ouvrir neuf.rs : introuvable");
        update(&mut s, k(Key::Char('a')));
        assert!(matches!(
            update(&mut s, ctrl('s')).as_slice(),
            [Command::Save { .. }]
        ));
        // Fichier illisible : Ctrl+S ne doit pas l'écraser avec un buffer vide.
        let mut s = State::default();
        update(
            &mut s,
            Event::OpenFailed {
                path: "/x/latin1.txt".into(),
                error: "non UTF-8".into(),
                create: false,
            },
        );
        assert_eq!(s.status, "Impossible d'ouvrir latin1.txt : non UTF-8");
        assert!(update(&mut s, ctrl('s')).is_empty());
    }

    #[test]
    fn double_ctrl_q() {
        let mut s = opened("a");
        assert_eq!(update(&mut s, ctrl('q')), vec![Command::Quit]);
        update(&mut s, k(Key::Char('b')));
        assert!(update(&mut s, ctrl('q')).is_empty());
        assert!(s.quit_armed);
        assert!(s.status.starts_with("Modifications non sauvegardées"));
        // Une autre touche désarme.
        update(&mut s, k(Key::Left));
        assert!(!s.quit_armed);
        assert!(update(&mut s, ctrl('q')).is_empty());
        assert_eq!(update(&mut s, ctrl('Q')), vec![Command::Quit]);
    }

    #[test]
    fn scroll_suit_le_curseur() {
        // Zone : 40-30 = 10 colonnes, moins 8 de gouttière = 2 ; 7-1-1 = 5 lignes (statut, onglets).
        let mut s = opened(&"l\n".repeat(20));
        for _ in 0..7 {
            update(&mut s, k(Key::Down));
        }
        assert_eq!(s.scroll_line, 3);
        update(&mut s, k(Key::PageDown));
        assert_eq!(s.doc.cursor_line_col().0, 12);
        assert_eq!(s.scroll_line, 8);
        update(&mut s, k(Key::PageUp));
        update(&mut s, k(Key::PageUp));
        assert_eq!((s.doc.cursor_line_col().0, s.scroll_line), (2, 2));
        for c in "中文ab".chars() {
            update(&mut s, k(Key::Char(c)));
        }
        // Idéogrammes = 2 colonnes : curseur en colonne 6, 2 colonnes visibles.
        assert_eq!(s.doc.cursor_line_col().1, 6);
        assert_eq!(s.scroll_col, 5);
        update(&mut s, k(Key::Home));
        assert_eq!(s.scroll_col, 0);
    }

    fn avec(key: Key, ctrl: bool, shift: bool) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers {
                ctrl,
                shift,
                ..Modifiers::default()
            },
        })
    }
    fn souris(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseInput {
            kind,
            column,
            row,
            mods: Modifiers::default(),
        })
    }
    fn texte(s: &State) -> String {
        s.doc.rope().to_string()
    }

    #[test]
    fn ctrl_z_y_et_ctrl_shift_z() {
        let mut s = opened("a");
        update(&mut s, k(Key::Char('b')));
        update(&mut s, ctrl('z'));
        assert_eq!(texte(&s), "a");
        assert!(!s.doc.is_dirty());
        update(&mut s, ctrl('y'));
        assert_eq!(texte(&s), "ba");
        update(&mut s, ctrl('z'));
        update(&mut s, avec(Key::Char('Z'), true, true));
        assert_eq!(texte(&s), "ba");
        update(&mut s, avec(Key::Char('Z'), true, true));
        assert_eq!(s.status, "Rien à rétablir");
    }

    #[test]
    fn shift_fleches_et_ctrl_a() {
        let mut s = opened("abc\ndef");
        update(&mut s, avec(Key::End, false, true));
        update(&mut s, avec(Key::Left, false, true));
        assert_eq!(s.doc.selection(), Some(0..2));
        update(&mut s, k(Key::Char('x')));
        assert_eq!(texte(&s), "xc\ndef");
        update(&mut s, ctrl('a'));
        assert_eq!(s.doc.selection(), Some(0..6));
        update(&mut s, k(Key::Tab));
        assert_eq!(texte(&s), "    xc\n    def");
        assert_eq!(s.doc.selection(), Some(0..14));
        update(&mut s, avec(Key::Tab, false, true));
        assert_eq!(texte(&s), "xc\ndef");
        update(&mut s, k(Key::Char(' ')));
        assert_eq!(texte(&s), " ");
        update(&mut s, avec(Key::Home, false, true));
        update(&mut s, k(Key::Delete));
        assert_eq!(texte(&s), "");
    }

    #[test]
    fn copier_couper_coller() {
        let mut s = opened("un\ndeux");
        // Sans sélection : la ligne entière, avec un `\n` même en fin de fichier.
        assert_eq!(
            update(&mut s, ctrl('c')),
            vec![Command::CopyToClipboard("un\n".into())]
        );
        update(&mut s, k(Key::Down));
        assert_eq!(
            update(&mut s, ctrl('x')),
            vec![Command::CopyToClipboard("deux\n".into())]
        );
        // Dernière ligne sans `\n` : le saut de ligne précédent part avec elle.
        assert_eq!(texte(&s), "un");
        // Collée au-dessus de la ligne courante, curseur sur la même colonne.
        update(&mut s, k(Key::End));
        update(&mut s, Event::Paste("deux\n".into()));
        assert_eq!(texte(&s), "deux\nun");
        assert_eq!(s.doc.cursor_line_col(), (1, 2));
        let mut s = opened("un\n");
        update(&mut s, avec(Key::Right, false, true));
        assert_eq!(
            update(&mut s, ctrl('x')),
            vec![Command::CopyToClipboard("u".into())]
        );
        assert_eq!(texte(&s), "n\n");
        assert_eq!(update(&mut s, ctrl('v')), vec![Command::RequestPaste]);
        update(&mut s, Event::Paste("a\r\nb".into()));
        assert_eq!(texte(&s), "a\nbn\n");
        update(&mut s, ctrl('z'));
        assert_eq!(texte(&s), "n\n");
    }

    #[test]
    fn gouttiere_s_elargit_au_dela_de_9999_lignes() {
        let w = |text: &str| gutter_width(&opened(text));
        assert_eq!(w(""), 8);
        assert_eq!(w(&"\n".repeat(9_998)), 8);
        assert_eq!(w(&"\n".repeat(9_999)), 9);
    }

    #[test]
    fn numeros_de_ligne_masques_alt_l() {
        let alt_l = Event::Key(KeyInput {
            key: Key::Char('l'),
            mods: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
        });
        let mut s = opened("abc\ndef\n");
        update(&mut s, alt_l.clone());
        assert!(!s.settings.line_numbers);
        // Plus que marque git, repli et diagnostics : le texte commence en 30 + 3.
        assert_eq!(gutter_width(&s), 3);
        update(&mut s, souris(MouseKind::Down, 34, 2));
        assert_eq!(s.doc.cursor_line_col(), (1, 1));
        // Clic dans la gouttière : repli, pas de déplacement du curseur.
        update(&mut s, souris(MouseKind::Down, 32, 1));
        assert_eq!(s.doc.cursor_line_col(), (1, 1));
        update(&mut s, alt_l.clone());
        assert_eq!(gutter_width(&s), 8);
    }

    #[test]
    fn alt_t_passe_au_theme_suivant() {
        let alt_t = Event::Key(KeyInput {
            key: Key::Char('t'),
            mods: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
        });
        let mut s = opened("");
        let mut vus = vec![];
        for _ in 0..crate::THEMES.len() {
            update(&mut s, alt_t.clone());
            vus.push(s.settings.theme.clone());
        }
        assert_eq!(vus, ["light", "gruvbox", "solarized-light", "dark"]);
        assert_eq!(s.status, "Thème : dark");
        s.settings.theme = "inconnu".into();
        update(&mut s, alt_t.clone());
        assert_eq!(s.settings.theme, "light");
    }

    #[test]
    fn raccourci_personnalise_rejoue_la_commande() {
        let mut s = opened("abc");
        let f5 = crate::KeyInput::parse("f5").expect("f5");
        s.settings.keys.push((f5, "select_all".into()));
        update(&mut s, Event::Key(f5));
        assert_eq!(s.doc.selection(), Some(0..3));
        for (texte, attendu) in [
            ("ctrl+shift+k", "Ctrl+Shift+K"),
            ("Alt+1", "Alt+1"),
            ("ctrl+space", "Ctrl+Espace"),
            ("ctrl+pagedown", "Ctrl+Page suiv."),
            ("f12", "F12"),
        ] {
            let k = crate::KeyInput::parse(texte).map(|k| k.to_string());
            assert_eq!(k.as_deref(), Some(attendu), "{texte}");
        }
        for invalide in ["", "ctrl+", "hyper+x", "f99", "ctrl+ab"] {
            assert_eq!(crate::KeyInput::parse(invalide), None, "{invalide}");
        }
    }

    #[test]
    fn alt_retour_arriere_efface_le_mot() {
        let mut s = opened("un deux");
        update(&mut s, k(Key::End));
        update(
            &mut s,
            Event::Key(KeyInput {
                key: Key::Backspace,
                mods: Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
            }),
        );
        assert_eq!(s.doc.rope().to_string(), "un ");
    }

    #[test]
    fn altgr_en_ctrl_alt_insere_le_caractere() {
        let mut s = opened("");
        let altgr = |c| {
            Event::Key(KeyInput {
                key: Key::Char(c),
                mods: Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: false,
                },
            })
        };
        update(&mut s, altgr('{'));
        update(&mut s, altgr('€'));
        assert_eq!(s.doc.rope().to_string(), "{€}");
        // Une vraie combinaison Ctrl+Alt+lettre reste un raccourci.
        update(&mut s, altgr('x'));
        assert_eq!(s.doc.rope().to_string(), "{€}");
    }

    #[test]
    fn glisser_commence_hors_du_texte_ignore() {
        let mut s = opened("abcdef");
        update(&mut s, souris(MouseKind::Down, 5, 1));
        update(&mut s, souris(MouseKind::Drag, 40, 1));
        assert_eq!((s.doc.cursor(), s.doc.selection()), (0, None));
        update(&mut s, souris(MouseKind::Down, 38, 1));
        update(&mut s, souris(MouseKind::Up, 38, 1));
        update(&mut s, souris(MouseKind::Drag, 40, 1));
        assert_eq!(s.doc.selection(), None);
    }

    #[test]
    fn collage_vide_et_indentation_detectee() {
        let mut s = opened("abc");
        s.doc.select_all();
        update(&mut s, Event::Paste(String::new()));
        assert_eq!(s.doc.selection(), Some(0..3));
        update(&mut s, Event::Paste("x".into()));
        assert_eq!(s.doc.rope().to_string(), "x");
        let indent = crate::Indent {
            use_tabs: true,
            width: 8,
            tab_width: 8,
        };
        update(
            &mut s,
            Event::IndentDetected {
                path: "/x/autre.txt".into(),
                indent,
            },
        );
        assert_eq!(
            s.doc.indent,
            crate::Indent::default(),
            "autre fichier : onglet actif intact"
        );
        update(
            &mut s,
            Event::IndentDetected {
                path: "/x/a.txt".into(),
                indent,
            },
        );
        assert_eq!(s.doc.indent, indent);
    }

    #[test]
    fn souris_clic_glisser_defilement() {
        // Texte à partir de la colonne 30 + 8 = 38 ; 5 lignes visibles, à partir de la ligne 1 (onglets en 0).
        let mut s = opened("abc\nd中e\n");
        update(&mut s, souris(MouseKind::Down, 39, 1));
        assert_eq!(s.doc.cursor(), 1);
        update(&mut s, souris(MouseKind::Drag, 40, 2)); // colonne 2 = milieu de 中 : avant
        assert_eq!(s.doc.selection(), Some(1..5));
        update(&mut s, souris(MouseKind::Up, 40, 2));
        assert_eq!(s.doc.selection(), Some(1..5));
        // Au-delà de la fin de ligne, puis au-delà du document.
        update(&mut s, souris(MouseKind::Down, 41, 1));
        assert_eq!((s.doc.cursor(), s.doc.selection()), (3, None));
        update(&mut s, souris(MouseKind::Down, 52, 5));
        assert_eq!(s.doc.cursor(), 8);
        // Clic dans l'explorateur, la barre d'onglets hors onglet ou la barre de statut : ignoré.
        update(&mut s, souris(MouseKind::Down, 71, 0));
        update(&mut s, souris(MouseKind::Down, 5, 1));
        update(&mut s, souris(MouseKind::Down, 39, 6));
        assert_eq!(s.doc.cursor(), 8);
        let mut s = opened(&"l\n".repeat(20));
        update(&mut s, souris(MouseKind::ScrollDown, 39, 1));
        update(&mut s, souris(MouseKind::ScrollDown, 39, 1));
        assert_eq!((s.scroll_line, s.doc.cursor()), (6, 0));
        for _ in 0..10 {
            update(&mut s, souris(MouseKind::ScrollDown, 39, 1));
        }
        assert_eq!(s.scroll_line, 20);
        update(&mut s, souris(MouseKind::ScrollUp, 39, 1));
        assert_eq!(s.scroll_line, 17);
        update(&mut s, souris(MouseKind::Down, 38, 2));
        assert_eq!(s.doc.cursor_line_col(), (18, 0));
    }

    #[test]
    fn repli_des_blocs() {
        let alt = |c| {
            Event::Key(KeyInput {
                key: Key::Char(c),
                mods: Modifiers {
                    ctrl: false,
                    alt: true,
                    shift: false,
                },
            })
        };
        let mut s = opened("fn a() {\n    x();\n    y();\n}\nz\n");
        update(&mut s, alt('-'));
        assert!(s.doc.is_folded(0));
        update(&mut s, k(Key::Down));
        assert_eq!(s.doc.cursor_line_col().0, 3);
        // Ctrl+Z ou recherche qui amène le curseur dans le bloc : il se déplie.
        let pos = s.doc.pos_at(2, 0);
        s.doc.set_cursor(pos, false);
        update(&mut s, Event::GitBranch(None));
        assert!(!s.doc.is_folded(0));
        update(&mut s, alt('_'));
        assert_eq!(s.doc.cursor_line_col(), (0, 8));
        update(&mut s, alt('='));
        assert!(!s.doc.is_folded(0));
        // Clic dans la gouttière (colonnes 30 à 36, texte à partir de la ligne 1) : bascule.
        update(&mut s, souris(MouseKind::Down, 31, 1));
        assert!(s.doc.is_folded(0));
        update(&mut s, souris(MouseKind::Down, 38, 2));
        assert_eq!(s.doc.cursor_line_col().0, 3);
        update(&mut s, souris(MouseKind::Down, 31, 1));
        assert!(!s.doc.is_folded(0));
    }

    #[test]
    fn paires_automatiques() {
        let tape = |path: &str, text: &str| {
            let mut s = State::default();
            update(
                &mut s,
                Event::FileOpened {
                    path: path.into(),
                    text: String::new(),
                },
            );
            for c in text.chars() {
                update(
                    &mut s,
                    k(if c == '\x08' {
                        Key::Backspace
                    } else {
                        Key::Char(c)
                    }),
                );
            }
            let mut t = s.doc.rope().to_string();
            t.insert(s.doc.rope().char_to_byte(s.doc.cursor()), '|');
            t
        };
        assert_eq!(tape("/x/a.c", "f("), "f(|)");
        assert_eq!(tape("/x/a.c", "f(x)"), "f(x)|");
        assert_eq!(tape("/x/a.c", "{[\""), "{[\"|\"]}");
        assert_eq!(tape("/x/a.c", "a = \"b\";"), "a = \"b\";|");
        assert_eq!(tape("/x/a.c", "('"), "('|')");
        assert_eq!(tape("/x/a.md", "l'arbre"), "l'arbre|");
        assert_eq!(tape("/x/a.rs", "<'a"), "<'a|");
        assert_eq!(tape("/x/a.c", "(\x08"), "|");
        assert_eq!(tape("/x/a.c", "a("), "a(|)");
        // Devant un mot : pas de fermante.
        let mut s = State::default();
        update(
            &mut s,
            Event::FileOpened {
                path: "/x/a.c".into(),
                text: "x".into(),
            },
        );
        update(&mut s, k(Key::Char('(')));
        assert_eq!(s.doc.rope().to_string(), "(x");
    }

    #[test]
    fn balise_fermante_automatique() {
        let tape = |path: &str, text: &str| {
            let mut s = State::default();
            update(
                &mut s,
                Event::FileOpened {
                    path: path.into(),
                    text: String::new(),
                },
            );
            for c in text.chars() {
                update(&mut s, k(Key::Char(c)));
            }
            let mut t = s.doc.rope().to_string();
            t.insert(s.doc.rope().char_to_byte(s.doc.cursor()), '|');
            t
        };
        assert_eq!(tape("/x/a.html", "<div>"), "<div>|</div>");
        assert_eq!(
            tape("/x/a.php", "<p class=\"a b\">x"),
            "<p class=\"a b\">x|</p>"
        );
        assert_eq!(tape("/x/a.html", "<ul><li>"), "<ul><li>|</li></ul>");
        assert_eq!(tape("/x/a.html", "<my-el>"), "<my-el>|</my-el>");
        for rien in [
            "<br>",
            "<img src=\"a\">",
            "<a/>",
            "<!DOCTYPE html>",
            "</p>",
            "a > b",
            "< b>",
        ] {
            assert_eq!(tape("/x/a.html", rien), format!("{rien}|"));
        }
        // Pas dans les autres langages (génériques Rust, comparaisons…).
        assert_eq!(tape("/x/a.rs", "Vec<u8>"), "Vec<u8>|");
        // Ctrl+Z retire la fermante seule.
        let mut s = State::default();
        update(
            &mut s,
            Event::FileOpened {
                path: "/x/a.html".into(),
                text: String::new(),
            },
        );
        for c in "<b>".chars() {
            update(&mut s, k(Key::Char(c)));
        }
        update(&mut s, ctrl('z'));
        assert_eq!(s.doc.rope().to_string(), "<b>");
    }

    #[test]
    fn abreviations_html() {
        let ouvert = |path: &str, text: &str| {
            let mut s = State::default();
            let path = PathBuf::from(path);
            update(
                &mut s,
                Event::FileOpened {
                    path,
                    text: text.into(),
                },
            );
            update(&mut s, k(Key::End));
            update(&mut s, k(Key::Tab));
            s
        };
        let s = ouvert("/x/a.html", "!!!");
        assert_eq!(s.doc.rope().to_string(), "<!DOCTYPE html>");
        let s = ouvert("/x/a.php", "  !");
        let text = s.doc.rope().to_string();
        assert!(
            text.starts_with(
                "  <!DOCTYPE html>\n  <html lang=\"fr\">\n  <head>\n      <meta charset"
            ),
            "{text}"
        );
        assert!(
            text.ends_with("  <body>\n      \n  </body>\n  </html>"),
            "{text}"
        );
        let line = s.doc.rope().char_to_line(s.doc.cursor());
        assert_eq!(s.doc.line_text(line), "      ");
        assert_eq!(s.doc.cursor(), s.doc.rope().line_to_char(line) + 6);
        // Ailleurs : tabulation ordinaire.
        assert_eq!(ouvert("/x/a.txt", "!").doc.rope().to_string(), "!   ");
        assert_eq!(ouvert("/x/a.html", "a!").doc.rope().to_string(), "a!  ");
    }

    #[test]
    fn f1_ouvre_et_ferme_l_aide() {
        let mut s = opened("ab");
        s.height = 20;
        s.focus = crate::Focus::Explorer;
        update(&mut s, k(Key::F(1)));
        assert_eq!(s.help, Some(0));
        // Modale : les touches ne vont ni au document ni à l'explorateur.
        update(&mut s, k(Key::Char('x')));
        update(&mut s, k(Key::Down));
        assert_eq!(s.help, Some(1));
        assert_eq!(s.doc.rope().to_string(), "ab");
        // Défilement borné à la dernière page (16 lignes visibles).
        update(&mut s, k(Key::End));
        let max = crate::help_len() - 16;
        assert_eq!(s.help, Some(max));
        update(&mut s, k(Key::PageDown));
        assert_eq!(s.help, Some(max));
        let molette = |kind| {
            Event::Mouse(MouseInput {
                kind,
                column: 0,
                row: 0,
                mods: Modifiers::default(),
            })
        };
        update(&mut s, molette(MouseKind::ScrollUp));
        assert_eq!(s.help, Some(max - WHEEL_LINES));
        update(&mut s, k(Key::PageUp));
        assert_eq!(s.help, Some(max - WHEEL_LINES - 16));
        update(&mut s, k(Key::Home));
        assert_eq!(s.help, Some(0));
        update(&mut s, k(Key::F(1)));
        assert_eq!(s.help, None);
        update(&mut s, k(Key::F(1)));
        assert_eq!(update(&mut s, ctrl('q')), vec![Command::Quit]);
        update(&mut s, k(Key::Esc));
        assert_eq!(s.help, None);
        assert_eq!(s.focus, crate::Focus::Explorer);
    }

    #[test]
    fn commande_raccourcis_clavier() {
        let mut s = State::default();
        update(&mut s, avec(Key::Char('P'), true, true));
        for c in "raccourcis".chars() {
            update(&mut s, k(Key::Char(c)));
        }
        update(&mut s, k(Key::Enter));
        assert!(s.palette.is_none());
        assert_eq!(s.help, Some(0));
    }
}
