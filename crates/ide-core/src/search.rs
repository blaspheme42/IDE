//! Rechercher / remplacer dans le fichier (F-15), en littéral ou en regex.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

use crate::{Key, KeyInput, State};

/// shortcut: recalcul complet sur tout le texte à chaque frappe ou édition, surlignage borné à ce
/// nombre d'occurrences (pas « tout remplacer ») ; passer à une recherche par fenêtre si les gros fichiers rament.
const MAX_MATCHES: usize = 10_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    /// `Some` en mode remplacer (Ctrl+H).
    pub replace: Option<String>,
    /// Le champ en cours de saisie est celui du remplacement.
    pub focus_replace: bool,
    pub regex: bool,
    pub case_sensitive: bool,
    /// Occurrences en plages d'index de char dans la rope, triées.
    pub matches: Vec<Range<usize>>,
    /// Index dans `matches` de l'occurrence courante.
    pub current: Option<usize>,
    /// Regex invalide : message affichable.
    pub error: Option<String>,
}

impl Search {
    pub fn compile(&self) -> Result<Regex, String> {
        let pattern = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| {
                // Le message de `regex` tient sur plusieurs lignes : la dernière dit l'essentiel.
                let msg = e.to_string();
                let last = msg.lines().last().unwrap_or_default();
                format!("Regex invalide : {}", last.trim_start_matches("error: "))
            })
    }

    fn field(&mut self) -> &mut String {
        match &mut self.replace {
            Some(r) if self.focus_replace => r,
            _ => &mut self.query,
        }
    }
}

/// Occurrences non vides de `re` en plages de char, avec leur remplacement (`$1`, `${nom}`) si `rep` est donné.
fn scan(text: &str, re: &Regex, rep: Option<&str>, limit: usize) -> Vec<(Range<usize>, String)> {
    let (mut byte, mut chars) = (0, 0);
    re.captures_iter(text)
        .filter_map(|c| Some((c.get(0)?, c)))
        .filter(|(m, _)| !m.is_empty())
        .take(limit)
        .map(|(m, c)| {
            chars += text[byte..m.start()].chars().count();
            let start = chars;
            chars += m.as_str().chars().count();
            byte = m.end();
            let mut out = String::new();
            if let Some(rep) = rep {
                c.expand(rep, &mut out);
            }
            (start..chars, out)
        })
        .collect()
}

/// Ctrl+F / Ctrl+H : ouvre (ou change de mode), pré-rempli par la sélection si elle tient sur une ligne.
pub(crate) fn open(state: &mut State, replace: bool) {
    let mut s = state.search.take().unwrap_or_else(|| Search {
        query: state
            .doc
            .selected_text()
            .filter(|t| !t.contains('\n'))
            .unwrap_or_default(),
        ..Search::default()
    });
    s.replace = replace.then(|| s.replace.take().unwrap_or_default());
    s.focus_replace = false;
    state.search = Some(s);
    refresh(state, true);
}

/// Recalcule les occurrences ; courante = première à partir de la sélection (ou du curseur).
/// `select` place la sélection dessus pour que le défilement suive.
pub(crate) fn refresh(state: &mut State, select: bool) {
    let Some(s) = state.search.as_mut() else {
        return;
    };
    s.error = None;
    s.matches = match s.compile() {
        _ if s.query.is_empty() => vec![],
        Ok(re) => scan(&state.doc.rope().to_string(), &re, None, MAX_MATCHES)
            .into_iter()
            .map(|(r, _)| r)
            .collect(),
        Err(e) => {
            s.error = Some(e);
            vec![]
        }
    };
    let from = state
        .doc
        .selection()
        .map_or(state.doc.cursor(), |r| r.start);
    s.current = (!s.matches.is_empty())
        .then(|| s.matches.iter().position(|r| r.start >= from).unwrap_or(0));
    show(state, select);
}

/// Met le statut à jour (« 3/12 », « Aucun résultat », erreur) et sélectionne l'occurrence courante si `select`.
fn show(state: &mut State, select: bool) {
    let Some(s) = &state.search else {
        return;
    };
    state.status = match (&s.error, s.current) {
        (Some(e), _) => e.clone(),
        _ if s.query.is_empty() => String::new(),
        (None, Some(i)) if s.matches.len() >= MAX_MATCHES => format!("{}/{MAX_MATCHES}+", i + 1),
        (None, Some(i)) => format!("{}/{}", i + 1, s.matches.len()),
        (None, None) => "Aucun résultat".into(),
    };
    let r = s.current.and_then(|i| s.matches.get(i)).cloned();
    if let (true, Some(r)) = (select, r) {
        state.doc.set_cursor(r.start, false);
        state.doc.set_cursor(r.end, true);
    }
}

/// Texte tapé ou collé dans le champ actif.
pub(crate) fn input(state: &mut State, text: &str) {
    if let Some(s) = state.search.as_mut() {
        s.field().push_str(text);
        refresh(state, true);
    }
}

/// Occurrence suivante ou précédente, en boucle.
fn step(state: &mut State, forward: bool) {
    let Some(s) = state.search.as_mut() else {
        return;
    };
    let n = s.matches.len();
    if n == 0 {
        return;
    }
    s.current = Some(match s.current {
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
        None => 0,
    });
    show(state, true);
}

/// Occurrences et leurs remplacements, ou `None` hors mode remplacer / regex invalide.
fn replacements(state: &State) -> Option<Vec<(Range<usize>, String)>> {
    let s = state.search.as_ref()?;
    let rep = s.replace.as_ref()?;
    let re = s.compile().ok().filter(|_| !s.query.is_empty())?;
    // En littéral, `$` n'a rien de spécial.
    let rep = if s.regex {
        rep.clone()
    } else {
        rep.replace('$', "$$")
    };
    Some(scan(
        &state.doc.rope().to_string(),
        &re,
        Some(&rep),
        usize::MAX,
    ))
}

/// Remplace l'occurrence sélectionnée puis passe à la suivante. Si le curseur a bougé
/// (sélection ≠ une occurrence), va d'abord à l'occurrence suivante sans rien remplacer, comme VSCode.
fn replace_one(state: &mut State) {
    let sel = state.doc.selection();
    let current = state
        .search
        .as_ref()
        .and_then(|s| s.matches.iter().position(|r| Some(r) == sel.as_ref()));
    if current.is_none() {
        refresh(state, true);
        return;
    }
    let one = current.and_then(|i| replacements(state)?.into_iter().nth(i));
    if let Some((r, text)) = one {
        state.doc.edit(r, &text);
    }
    refresh(state, true);
}

/// Tout remplacer : une seule entrée d'historique.
fn replace_all(state: &mut State) {
    if let Some(all) = replacements(state) {
        state.doc.edit_many(all);
    }
}

/// Touches de la barre de recherche ; `false` = touche laissée à l'éditeur.
pub(crate) fn key(state: &mut State, k: KeyInput) -> bool {
    let Some(s) = state.search.as_mut() else {
        return false;
    };
    let (ctrl, alt, shift) = (k.mods.ctrl, k.mods.alt, k.mods.shift);
    match k.key {
        Key::Esc => {
            state.search = None;
            state.status.clear();
        }
        // Sans protocole clavier étendu, le terminal envoie Ctrl+Alt+Entrée comme Alt+Entrée.
        Key::Enter if alt => replace_all(state),
        Key::Enter if !ctrl && !alt && !shift && s.focus_replace => replace_one(state),
        Key::Enter | Key::F(3) if !ctrl && !alt => step(state, !shift),
        Key::Tab => s.focus_replace = s.replace.is_some() && !s.focus_replace,
        Key::Backspace => {
            s.field().pop();
            refresh(state, true);
        }
        // Pas de curseur dans le champ : Suppr ne doit pas toucher au document.
        Key::Delete => {}
        Key::Char(c) if !ctrl && !alt && !c.is_control() => {
            input(state, c.encode_utf8(&mut [0; 4]));
        }
        Key::Char('r' | 'R') if alt && !ctrl => {
            s.regex = !s.regex;
            refresh(state, true);
        }
        Key::Char('c' | 'C') if alt && !ctrl => {
            s.case_sensitive = !s.case_sensitive;
            refresh(state, true);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use crate::{Event, Key, KeyInput, Modifiers, State, editor_size, update};

    fn touche(key: Key, ctrl: bool, alt: bool, shift: bool) -> Event {
        Event::Key(KeyInput {
            key,
            mods: Modifiers { ctrl, alt, shift },
        })
    }
    fn k(key: Key) -> Event {
        touche(key, false, false, false)
    }
    fn ctrl(c: char) -> Event {
        touche(Key::Char(c), true, false, false)
    }
    fn alt(c: char) -> Event {
        touche(Key::Char(c), false, true, false)
    }
    fn opened(text: &str) -> State {
        let mut s = State::default();
        update(
            &mut s,
            Event::Resize {
                width: 40,
                height: 10,
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
    fn taper(s: &mut State, t: &str) {
        for c in t.chars() {
            update(s, k(Key::Char(c)));
        }
    }
    fn chercher(text: &str, query: &str) -> State {
        let mut s = opened(text);
        update(&mut s, ctrl('f'));
        taper(&mut s, query);
        s
    }
    fn matches(s: &State) -> Vec<std::ops::Range<usize>> {
        s.search
            .as_ref()
            .map(|x| x.matches.clone())
            .unwrap_or_default()
    }
    fn texte(s: &State) -> String {
        s.doc.rope().to_string()
    }

    #[test]
    fn litteral_insensible_puis_sensible_a_la_casse() {
        let mut s = chercher("Ab ab AB a.b", "ab");
        assert_eq!(matches(&s), vec![0..2, 3..5, 6..8]);
        assert_eq!(s.status, "1/3");
        assert_eq!(s.doc.selection(), Some(0..2));
        update(&mut s, alt('c'));
        assert_eq!(matches(&s), vec![3..5]);
        // Littéral : le `.` n'est pas un joker.
        update(&mut s, alt('C'));
        update(&mut s, k(Key::Backspace));
        taper(&mut s, ".b");
        assert_eq!(matches(&s), vec![9..12]);
    }

    #[test]
    fn regex_groupes_et_remplacement_dollar() {
        let mut s = opened("x=1, y=22");
        update(&mut s, ctrl('h'));
        update(&mut s, alt('r'));
        taper(&mut s, r"(?<nom>\w)=(\d+)");
        assert_eq!(matches(&s), vec![0..3, 5..9]);
        update(&mut s, k(Key::Tab));
        taper(&mut s, "$2:${nom}");
        update(&mut s, touche(Key::Enter, false, true, false));
        assert_eq!(texte(&s), "1:x, 22:y");
        assert_eq!(s.status, "Aucun résultat");
    }

    #[test]
    fn regex_invalide() {
        let mut s = opened("abc");
        update(&mut s, ctrl('f'));
        update(&mut s, alt('r'));
        taper(&mut s, "(a");
        let err = s.search.as_ref().and_then(|x| x.error.clone());
        assert_eq!(err.as_deref(), Some("Regex invalide : unclosed group"));
        assert_eq!(s.status, "Regex invalide : unclosed group");
        assert!(matches(&s).is_empty());
        // En littéral, la même saisie est valide.
        update(&mut s, alt('r'));
        assert_eq!(s.search.as_ref().and_then(|x| x.error.clone()), None);
        assert_eq!(s.status, "Aucun résultat");
    }

    #[test]
    fn accents_et_emoji_en_index_de_char() {
        let s = chercher("é👨‍👩‍👧 Été é", "é");
        // 👨‍👩‍👧 = 5 char ; É et é se confondent sans la casse.
        assert_eq!(matches(&s), vec![0..1, 7..8, 9..10, 11..12]);
        let s = chercher("中文 abc 中文", "中文");
        assert_eq!(matches(&s), vec![0..2, 7..9]);
    }

    #[test]
    fn occurrences_vides_ignorees() {
        let mut s = opened("baab\nb");
        update(&mut s, ctrl('f'));
        update(&mut s, alt('r'));
        taper(&mut s, "a*");
        assert_eq!(matches(&s), vec![1..3]);
        update(&mut s, k(Key::Backspace));
        update(&mut s, k(Key::Backspace));
        taper(&mut s, "^");
        assert!(matches(&s).is_empty());
        assert_eq!(s.status, "Aucun résultat");
    }

    #[test]
    fn suivante_precedente_en_boucle() {
        let mut s = opened("a a a");
        update(&mut s, k(Key::Right));
        update(&mut s, ctrl('f'));
        taper(&mut s, "a");
        // Première à partir du curseur.
        assert_eq!((s.status.as_str(), s.doc.selection()), ("2/3", Some(2..3)));
        update(&mut s, k(Key::Enter));
        assert_eq!(s.status, "3/3");
        update(&mut s, k(Key::F(3)));
        assert_eq!((s.status.as_str(), s.doc.selection()), ("1/3", Some(0..1)));
        update(&mut s, touche(Key::Enter, false, false, true));
        assert_eq!(s.status, "3/3");
        update(&mut s, touche(Key::F(3), false, false, true));
        assert_eq!((s.status.as_str(), s.doc.selection()), ("2/3", Some(2..3)));
        assert_eq!(texte(&s), "a a a");
    }

    #[test]
    fn remplacer_un_puis_suivante() {
        let mut s = opened("a a a");
        update(&mut s, ctrl('h'));
        taper(&mut s, "a");
        update(&mut s, k(Key::Tab));
        taper(&mut s, "aa");
        assert_eq!(s.search.as_ref().map(|x| x.query.as_str()), Some("a"));
        update(&mut s, k(Key::Enter));
        // Le remplacement contient la recherche : on ne le re-remplace pas.
        assert_eq!(texte(&s), "aa a a");
        assert_eq!((s.status.as_str(), s.doc.selection()), ("3/4", Some(3..4)));
        update(&mut s, k(Key::Enter));
        assert_eq!(texte(&s), "aa aa a");
        assert_eq!(s.doc.selection(), Some(6..7));
    }

    #[test]
    fn plus_de_10000_occurrences() {
        use super::MAX_MATCHES;
        let mut s = opened(&"a".repeat(MAX_MATCHES + 5));
        update(&mut s, ctrl('h'));
        taper(&mut s, "a");
        assert_eq!(s.status, format!("1/{MAX_MATCHES}+"));
        update(&mut s, k(Key::Tab));
        taper(&mut s, "b");
        update(&mut s, touche(Key::Enter, false, true, false));
        assert_eq!(texte(&s), "b".repeat(MAX_MATCHES + 5));
    }

    #[test]
    fn remplacer_apres_deplacement_du_curseur() {
        let mut s = opened("ab ab ab");
        update(&mut s, ctrl('h'));
        taper(&mut s, "ab");
        update(&mut s, k(Key::Tab));
        taper(&mut s, "X");
        // Le curseur part vers la fin : Entrée sélectionne d'abord la 3e occurrence, sans remplacer.
        s.doc.set_cursor(5, false);
        update(&mut s, k(Key::Enter));
        assert_eq!(
            (texte(&s).as_str(), s.doc.selection()),
            ("ab ab ab", Some(6..8))
        );
        update(&mut s, k(Key::Enter));
        assert_eq!(texte(&s), "ab ab X");
    }

    #[test]
    fn tout_remplacer_un_seul_undo() {
        let mut s = opened("chat, Chat, chien");
        update(&mut s, ctrl('h'));
        taper(&mut s, "chat");
        update(&mut s, k(Key::Tab));
        taper(&mut s, "$1 é");
        update(&mut s, touche(Key::Enter, true, true, false));
        // Littéral : `$1` est gardé tel quel.
        assert_eq!(texte(&s), "$1 é, $1 é, chien");
        update(&mut s, ctrl('z'));
        assert_eq!(texte(&s), "chat, Chat, chien");
        assert!(!s.doc.is_dirty());
        // Les occurrences suivent l'annulation.
        assert_eq!(matches(&s), vec![0..4, 6..10]);
        update(&mut s, ctrl('y'));
        assert_eq!(texte(&s), "$1 é, $1 é, chien");
        assert!(matches(&s).is_empty());
    }

    #[test]
    fn ctrl_f_prerempli_par_la_selection_et_echap() {
        let mut s = opened("un deux\ndeux");
        update(&mut s, k(Key::End));
        for _ in 0..4 {
            update(&mut s, touche(Key::Left, false, false, true));
        }
        update(&mut s, ctrl('f'));
        let q = s.search.as_ref().map(|x| x.query.clone());
        assert_eq!(q.as_deref(), Some("deux"));
        assert_eq!((s.status.as_str(), s.doc.selection()), ("1/2", Some(3..7)));
        update(&mut s, k(Key::Enter));
        update(&mut s, k(Key::Esc));
        assert!(s.search.is_none());
        assert_eq!((s.status.as_str(), s.doc.selection()), ("", Some(8..12)));
        // Sélection multi-ligne : pas de pré-remplissage.
        update(&mut s, ctrl('a'));
        update(&mut s, ctrl('h'));
        let q = s.search.as_ref().map(|x| x.query.clone());
        assert_eq!(q.as_deref(), Some(""));
    }

    #[test]
    fn editor_size_reduit_par_la_barre() {
        let mut s = opened("a");
        // 10 lignes moins la barre d'onglets et le statut.
        assert_eq!(editor_size(&s), (10, 8));
        update(&mut s, ctrl('f'));
        assert_eq!(editor_size(&s), (10, 7));
        update(&mut s, ctrl('h'));
        assert_eq!(editor_size(&s), (10, 6));
        update(&mut s, k(Key::Esc));
        assert_eq!(editor_size(&s), (10, 8));
    }

    #[test]
    fn coller_va_dans_le_champ() {
        let mut s = opened("abc");
        update(&mut s, ctrl('f'));
        update(&mut s, Event::Paste("bc\nzz".into()));
        assert_eq!(texte(&s), "abc");
        assert_eq!(matches(&s), vec![1..3]);
    }
}
