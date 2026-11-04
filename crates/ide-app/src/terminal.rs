//! Terminal intégré (F-60) : un shell dans un PTY, émulé par vt100. Un thread lit, un autre écrit :
//! la boucle ne bloque jamais sur le shell ; elle recopie la grille dans l'état avant chaque rendu.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ide_core::{Event, Key, KeyInput, State, TermColor, TermSpan, terminal_size};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::sync::Notify;
use vt100::{Cell, Color, Parser, Screen};

/// Lignes d'historique gardées par vt100.
const SCROLLBACK: usize = 1000;

pub struct Term {
    command: CommandBuilder,
    /// Unique pour toute la vie de `Term` : la boucle attend dessus, quel que soit le shell.
    output: Arc<Notify>,
    session: Option<Session>,
}

/// Un shell lancé. Parser et `exited` lui sont propres : le lecteur d'un ancien shell, qui peut
/// survivre à ce dernier, ne touche pas au suivant.
struct Session {
    parser: Arc<Mutex<Parser<Replies>>>,
    exited: Arc<AtomicBool>,
    writer: Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
    /// Offset d'historique donné à vt100 au dernier `sync` : vt100 le décale ensuite à chaque ligne
    /// reçue pour figer la vue, et l'écart est à garder.
    view: usize,
}

/// vt100 ne répond pas aux requêtes du programme. L'envoi ne bloque pas et l'écran est passé en
/// paramètre : pas de verrou du parser à reprendre ici.
struct Replies(Sender<Vec<u8>>);

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let reply = match (i1, c, params) {
            (None, 'n', [[6]]) => {
                let (row, col) = cursor(screen);
                format!("\x1b[{};{}R", row + 1, col + 1)
            }
            (None, 'c', [] | [[0]]) => "\x1b[?1;2c".to_string(),
            _ => return,
        };
        let _ = self.0.send(reply.into_bytes());
    }
}

impl Session {
    fn parser(&self) -> MutexGuard<'_, Parser<Replies>> {
        self.parser.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `try_wait` en plus : un job de fond qui garde le PTY ouvert retarde EOF après la fin du shell.
    /// `exited` arrête alors le lecteur, dont la copie du maître empêcherait le raccroché.
    fn finished(&mut self) -> bool {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.exited.store(true, Ordering::Release);
        }
        self.exited.load(Ordering::Acquire)
    }
}

impl Term {
    pub fn new(root: PathBuf) -> Self {
        let mut command = CommandBuilder::new_default_prog();
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        Self {
            command,
            output: Arc::new(Notify::new()),
            session: None,
        }
    }

    pub fn output(&self) -> Arc<Notify> {
        self.output.clone()
    }

    /// À appeler avant chaque rendu : fin du shell, lancement, taille, copie de la grille.
    pub fn sync(&mut self, state: &mut State) {
        if let Some(mut child) = self.session.take_if(|s| s.finished()).map(|s| s.child) {
            // Après EOF, le shell peut vivre encore un instant : pas d'attente dans la boucle.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            ide_core::update(state, Event::TerminalExited { error: None });
            return;
        }
        if !state.terminal.open {
            return;
        }
        let (cols, rows) = terminal_size(state);
        let (Some(c), Some(r)) = (NonZeroU16::new(cols), NonZeroU16::new(rows)) else {
            return;
        };
        if self.session.is_none() {
            match self.spawn(r, c) {
                Ok(session) => self.session = Some(session),
                Err(e) => {
                    tracing::warn!("lancement du shell : {e:#}");
                    let error = Some(format!("{e:#}"));
                    ide_core::update(state, Event::TerminalExited { error });
                    return;
                }
            }
        }
        let Some(session) = &mut self.session else {
            return;
        };
        if session.size != (cols, rows) {
            session.size = (cols, rows);
            session.parser().screen_mut().set_size(r, c);
            if let Err(e) = session.master.resize(pty_size(r, c)) {
                tracing::warn!("redimensionnement du PTY : {e:#}");
            }
        }
        let mut parser = session
            .parser
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let screen = parser.screen_mut();
        let wanted = match state.terminal.scroll {
            0 => 0,
            scroll => (screen.scrollback() + scroll).saturating_sub(session.view),
        };
        screen.set_scrollback(wanted);
        session.view = screen.scrollback();
        state.terminal.scroll = session.view;
        state.terminal.lines = grid(screen);
        state.terminal.cursor =
            (!screen.hide_cursor() && state.terminal.scroll == 0).then(|| cursor(screen));
    }

    pub fn key(&mut self, k: KeyInput) {
        if let Some(s) = &self.session {
            let bytes = encode(k, s.parser().screen().application_cursor());
            let _ = s.writer.send(bytes);
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(s) = &self.session {
            let bytes = paste_bytes(text, s.parser().screen().bracketed_paste());
            let _ = s.writer.send(bytes);
        }
    }

    /// SIGHUP, puis SIGKILL après 200 ms au pire si le shell résiste.
    pub fn shutdown(&mut self) {
        if let Some(mut s) = self.session.take()
            && let Err(e) = s.child.kill()
        {
            tracing::warn!("arrêt du shell : {e}");
        }
    }

    fn spawn(&self, rows: NonZeroU16, cols: NonZeroU16) -> anyhow::Result<Session> {
        let pair = native_pty_system().openpty(pty_size(rows, cols))?;
        // Avant le lancement : une fois le shell parti, plus rien ne doit pouvoir échouer.
        let mut reader = pair.master.try_clone_reader()?;
        let mut pty_writer = pair.master.take_writer()?;
        let child = pair.slave.spawn_command(self.command.clone())?;
        // Sinon notre copie de l'esclave garde le PTY ouvert et EOF n'arrive jamais.
        drop(pair.slave);

        let (writer, queue) = mpsc::channel::<Vec<u8>>();
        // Threads détachés : une écriture bloquée (programme qui ne lit pas) n'arrête ni la boucle
        // ni la sortie. Pas d'`unwrap` ici : le hook de panic restaurerait l'écran en pleine session.
        std::thread::spawn(move || {
            for bytes in queue {
                if pty_writer
                    .write_all(&bytes)
                    .and_then(|()| pty_writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let parser = Parser::new_with_callbacks(rows, cols, SCROLLBACK, Replies(writer.clone()));
        let parser = Arc::new(Mutex::new(parser));
        let exited = Arc::new(AtomicBool::new(false));
        let (shared, done, output) = (parser.clone(), exited.clone(), self.output.clone());
        std::thread::spawn(move || {
            let mut buf = [0; 8192];
            // Linux répond EIO, pas 0, quand le dernier esclave se ferme.
            // shortcut: un job de fond muet bloque ce `read`, donc le raccroché, jusqu'à sa fin ; poll
            // sur le descripteur s'il faut raccrocher tout de suite.
            while !done.load(Ordering::Acquire)
                && let Ok(n @ 1..) = reader.read(&mut buf)
            {
                shared
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .process(&buf[..n]);
                output.notify_one();
            }
            done.store(true, Ordering::Release);
            output.notify_one();
        });
        Ok(Session {
            parser,
            exited,
            writer,
            master: pair.master,
            child,
            size: (cols.get(), rows.get()),
            view: 0,
        })
    }
}

fn pty_size(rows: NonZeroU16, cols: NonZeroU16) -> PtySize {
    PtySize {
        rows: rows.get(),
        cols: cols.get(),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// vt100 laisse la colonne à `cols`, hors grille, tant qu'un retour à la ligne est en attente.
fn cursor(screen: &Screen) -> (u16, u16) {
    let (row, col) = screen.cursor_position();
    (row, col.min(screen.size().1.get() - 1))
}

fn color(c: Color) -> TermColor {
    match c {
        Color::Default => TermColor::Default,
        Color::Idx(i) => TermColor::Idx(i),
        Color::Rgb(r, g, b) => TermColor::Rgb(r, g, b),
    }
}

/// Même style que `span` ? Le texte de la cellule peut alors s'y ajouter.
fn same_style(span: &TermSpan, cell: &Cell) -> bool {
    span.fg == color(cell.fgcolor())
        && span.bg == color(cell.bgcolor())
        && span.bold == cell.bold()
        && span.italic == cell.italic()
        && span.underline == cell.underline()
        && span.inverse == cell.inverse()
}

/// Écran visible, une `TermSpan` par suite de style.
fn grid(screen: &Screen) -> Vec<Vec<TermSpan>> {
    let (rows, cols) = screen.size();
    (0..rows.get())
        .map(|row| {
            let mut spans: Vec<TermSpan> = vec![];
            for cell in (0..cols.get()).filter_map(|col| screen.cell(row, col)) {
                if cell.is_wide_continuation() {
                    continue;
                }
                let text = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                match spans.last_mut() {
                    Some(span) if same_style(span, cell) => span.text.push_str(text),
                    _ => spans.push(TermSpan {
                        text: text.to_string(),
                        fg: color(cell.fgcolor()),
                        bg: color(cell.bgcolor()),
                        bold: cell.bold(),
                        italic: cell.italic(),
                        underline: cell.underline(),
                        inverse: cell.inverse(),
                    }),
                }
            }
            spans
        })
        .collect()
}

/// Séquence xterm d'une touche. `Key::Other` (dont Inser, que `Key` ne distingue pas) : rien.
fn encode(k: KeyInput, app_cursor: bool) -> Vec<u8> {
    let m = k.mods;
    // Paramètre de modificateurs xterm.
    let code = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl);
    let letter = |c: char, ss3: bool| match (code > 1, ss3) {
        (true, _) => format!("\x1b[1;{code}{c}"),
        (false, true) => format!("\x1bO{c}"),
        (false, false) => format!("\x1b[{c}"),
    };
    let tilde = |n: u8| match code {
        1 => format!("\x1b[{n}~"),
        _ => format!("\x1b[{n};{code}~"),
    };
    let seq = match k.key {
        Key::Up => letter('A', app_cursor),
        Key::Down => letter('B', app_cursor),
        Key::Right => letter('C', app_cursor),
        Key::Left => letter('D', app_cursor),
        Key::Home => letter('H', app_cursor),
        Key::End => letter('F', app_cursor),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => letter(char::from(b'P' + n - 1), true),
        Key::F(n @ 5) => tilde(n + 10),
        Key::F(n @ 6..=10) => tilde(n + 11),
        Key::F(n @ 11..=12) => tilde(n + 12),
        Key::F(_) | Key::Other => String::new(),
        Key::Tab if m.shift => "\x1b[Z".to_string(),
        Key::Tab => with_alt(m.alt, "\t"),
        Key::Enter => with_alt(m.alt, "\r"),
        Key::Backspace => with_alt(m.alt, "\x7f"),
        Key::Esc => with_alt(m.alt, "\x1b"),
        Key::Char(c) if m.ctrl => with_alt(m.alt, ctrl_char(c).encode_utf8(&mut [0; 4])),
        Key::Char(c) => with_alt(m.alt, c.encode_utf8(&mut [0; 4])),
    };
    seq.into_bytes()
}

fn with_alt(alt: bool, s: &str) -> String {
    if alt { format!("\x1b{s}") } else { s.into() }
}

/// Ctrl+touche selon xterm. Hors protocole étendu, crossterm rend 0x1c..0x1f comme Ctrl+4..7 : sans
/// les chiffres, Ctrl+\ (SIGQUIT) n'arriverait pas au shell.
fn ctrl_char(c: char) -> char {
    match c {
        '@'..='_' | 'a'..='z' => char::from(c as u8 & 0x1f),
        ' ' | '2' => '\0',
        '3'..='7' => char::from(c as u8 - b'3' + 0x1b),
        '/' => '\x1f',
        '?' | '8' => '\x7f',
        _ => c,
    }
}

/// Tous les ESC retirés : le texte collé ne peut pas clore le collage encadré et se faire exécuter.
fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace('\x1b', "");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Modifiers;

    fn touche(key: Key, ctrl: bool, alt: bool, shift: bool) -> KeyInput {
        KeyInput {
            key,
            mods: Modifiers { ctrl, alt, shift },
        }
    }

    fn nu(key: Key) -> KeyInput {
        touche(key, false, false, false)
    }

    #[test]
    fn encode_touches_simples() {
        for (k, attendu) in [
            (Key::Char('a'), "a"),
            (Key::Char('é'), "é"),
            (Key::Enter, "\r"),
            (Key::Backspace, "\x7f"),
            (Key::Tab, "\t"),
            (Key::Esc, "\x1b"),
            (Key::Home, "\x1b[H"),
            (Key::End, "\x1b[F"),
            (Key::PageUp, "\x1b[5~"),
            (Key::PageDown, "\x1b[6~"),
            (Key::Delete, "\x1b[3~"),
            (Key::Other, ""),
        ] {
            assert_eq!(encode(nu(k), false), attendu.as_bytes(), "{k:?}");
        }
        assert_eq!(
            encode(touche(Key::Tab, false, false, true), false),
            b"\x1b[Z"
        );
    }

    #[test]
    fn encode_ctrl_et_alt() {
        for (c, attendu) in [
            ('a', 1),
            ('C', 3),
            ('z', 26),
            ('[', 0x1b),
            ('\\', 0x1c),
            ('4', 0x1c),
            ('7', 0x1f),
            ('_', 0x1f),
            ('/', 0x1f),
            (' ', 0),
            ('?', 0x7f),
        ] {
            let k = touche(Key::Char(c), true, false, false);
            assert_eq!(encode(k, false), [attendu], "Ctrl+{c}");
        }
        let alt = |key| touche(key, false, true, false);
        assert_eq!(encode(alt(Key::Char('b')), false), b"\x1bb");
        assert_eq!(encode(alt(Key::Backspace), false), b"\x1b\x7f");
        let ctrl_alt = touche(Key::Char('c'), true, true, false);
        assert_eq!(encode(ctrl_alt, false), b"\x1b\x03");
    }

    #[test]
    fn encode_fleches() {
        for (k, l) in [
            (Key::Up, 'A'),
            (Key::Down, 'B'),
            (Key::Right, 'C'),
            (Key::Left, 'D'),
        ] {
            assert_eq!(encode(nu(k), false), format!("\x1b[{l}").as_bytes());
            assert_eq!(encode(nu(k), true), format!("\x1bO{l}").as_bytes());
            // Modificateur : la forme CSI, même en mode applicatif.
            let ctrl = touche(k, true, false, false);
            assert_eq!(encode(ctrl, true), format!("\x1b[1;5{l}").as_bytes());
            let shift_alt = touche(k, false, true, true);
            assert_eq!(encode(shift_alt, false), format!("\x1b[1;4{l}").as_bytes());
        }
    }

    #[test]
    fn encode_fonctions() {
        for (n, attendu) in [
            (1, "\x1bOP"),
            (2, "\x1bOQ"),
            (3, "\x1bOR"),
            (4, "\x1bOS"),
            (5, "\x1b[15~"),
            (6, "\x1b[17~"),
            (7, "\x1b[18~"),
            (8, "\x1b[19~"),
            (9, "\x1b[20~"),
            (10, "\x1b[21~"),
            (11, "\x1b[23~"),
            (12, "\x1b[24~"),
            (13, ""),
        ] {
            assert_eq!(encode(nu(Key::F(n)), false), attendu.as_bytes(), "F{n}");
        }
        let ctrl = |n| touche(Key::F(n), true, false, false);
        assert_eq!(encode(ctrl(1), false), b"\x1b[1;5P");
        assert_eq!(encode(ctrl(5), false), b"\x1b[15;5~");
    }

    #[test]
    fn collage_sans_esc() {
        // Imbriqué : retirer seulement `ESC[201~` laisserait reformer une fin de collage.
        let piege = "a\x1b[20\x1b[201~1~\x1b[201~rm -rf /\r";
        assert_eq!(
            paste_bytes(piege, true),
            b"\x1b[200~a[20[201~1~[201~rm -rf /\r\x1b[201~"
        );
        assert_eq!(paste_bytes("a\x1bb", false), b"ab");
    }

    #[test]
    fn collage_fins_de_ligne() {
        assert_eq!(paste_bytes("a\r\nb\nc", false), b"a\rb\rc");
        assert_eq!(
            paste_bytes("a\r\nb\nc", true),
            b"\x1b[200~a\r\nb\nc\x1b[201~"
        );
    }

    fn taille(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
    }

    fn ecran(bytes: &[u8]) -> Parser {
        let mut p = Parser::new(taille(2), taille(6), 0);
        p.process(bytes);
        p
    }

    #[test]
    fn grille_couleurs_et_styles() {
        let p = ecran(b"a\x1b[31mb\x1b[1;48;5;20mc\x1b[0m");
        let g = grid(p.screen());
        assert_eq!(g.len(), 2);
        let (a, b, c, reste) = (&g[0][0], &g[0][1], &g[0][2], &g[0][3]);
        assert_eq!((a.text.as_str(), a.fg), ("a", TermColor::Default));
        assert_eq!((b.text.as_str(), b.fg), ("b", TermColor::Idx(1)));
        assert_eq!(c.bg, TermColor::Idx(20));
        assert!(c.bold && !b.bold);
        assert_eq!(reste.text, "   ");
        let p = ecran(b"\x1b[38;2;1;2;3mx");
        assert_eq!(grid(p.screen())[0][0].fg, TermColor::Rgb(1, 2, 3));
    }

    #[test]
    fn grille_cellule_large_et_vide() {
        let p = ecran("\u{4e16}a".as_bytes());
        let g = grid(p.screen());
        assert_eq!(g[0].len(), 1);
        // 6 colonnes : le caractère large en occupe 2 mais ne compte qu'une fois.
        assert_eq!(g[0][0].text, "\u{4e16}a   ");
        assert_eq!(g[1][0].text, "      ");
    }

    #[test]
    fn curseur_dans_la_grille_en_fin_de_ligne() {
        assert_eq!(cursor(ecran(b"abcdef").screen()), (0, 5));
    }

    #[test]
    fn repond_a_dsr_et_da() {
        let (tx, rx) = mpsc::channel();
        let mut p = Parser::new_with_callbacks(taille(24), taille(80), 0, Replies(tx));
        p.process(b"\x1b[2;3H\x1b[6n");
        assert_eq!(rx.try_recv().ok(), Some(b"\x1b[2;3R".to_vec()));
        p.process(b"\x1b[c");
        assert_eq!(rx.try_recv().ok(), Some(b"\x1b[?1;2c".to_vec()));
        p.process(b"\x1b[0c");
        assert_eq!(rx.try_recv().ok(), Some(b"\x1b[?1;2c".to_vec()));
        p.process(b"\x1b[>c\x1b[5n");
        assert!(rx.try_recv().is_err());
    }

    #[cfg(unix)]
    mod pty {
        use super::*;
        use std::time::{Duration, Instant};

        fn sh() -> Term {
            let mut term = Term::new(std::env::temp_dir());
            term.command = CommandBuilder::new("sh");
            term
        }

        fn ouvert() -> State {
            let mut s = State {
                width: 80,
                height: 40,
                ..State::default()
            };
            s.explorer.visible = false;
            s.terminal.open = true;
            s
        }

        fn tape(term: &mut Term, texte: &str) {
            for c in texte.chars() {
                term.key(nu(Key::Char(c)));
            }
            term.key(nu(Key::Enter));
        }

        fn texte(s: &State) -> String {
            let lignes = s.terminal.lines.iter().flatten();
            lignes.map(|span| span.text.as_str()).collect()
        }

        fn attend(term: &mut Term, s: &mut State, fini: impl Fn(&State) -> bool) {
            let limite = Instant::now() + Duration::from_secs(10);
            loop {
                term.sync(s);
                if fini(s) {
                    return;
                }
                assert!(Instant::now() < limite, "délai dépassé : {}", texte(s));
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn echo_puis_eof() {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            tape(&mut term, "echo un$((1+1))trois");
            attend(&mut term, &mut s, |s| texte(s).contains("un2trois"));
            tape(&mut term, "exit");
            attend(&mut term, &mut s, |s| !s.terminal.open);
            assert!(term.session.is_none());
            assert!(s.terminal.lines.is_empty());
        }

        #[test]
        fn eof_sans_fin_visible_du_fils() {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            // Le lecteur seul signale la fin : le fils n'est pas repris par `try_wait` avant.
            let exited = term.session.as_ref().map(|t| t.exited.clone());
            tape(&mut term, "exit");
            let limite = Instant::now() + Duration::from_secs(10);
            while !exited.as_ref().is_some_and(|e| e.load(Ordering::Acquire)) {
                assert!(Instant::now() < limite, "pas d'EOF");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn relance_sans_interference_de_l_ancien_lecteur() {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            // Le job de fond garde le PTY ouvert : l'ancien lecteur survit au shell et finit après
            // le lancement du nouveau.
            tape(&mut term, "sleep 1 & exit");
            attend(&mut term, &mut s, |s| !s.terminal.open);
            s.terminal.open = true;
            term.sync(&mut s);
            std::thread::sleep(Duration::from_millis(1500));
            term.sync(&mut s);
            assert!(s.terminal.open, "nouveau shell marqué comme terminé");
            tape(&mut term, "echo en$((1+1))vie");
            attend(&mut term, &mut s, |s| texte(s).contains("en2vie"));
            term.shutdown();
        }

        #[test]
        fn job_de_fond_raccroche_a_la_fin_du_shell() {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            let exited = term.session.as_ref().map(|t| t.exited.clone());
            // Borné à 10 s : pas de processus laissé si le test échoue. `echo` échoue dès le raccroché.
            tape(
                &mut term,
                "(i=0; while [ $i -lt 100 ] && echo x; do sleep 0.1; i=$((i+1)); done) & exit",
            );
            attend(&mut term, &mut s, |s| !s.terminal.open);
            let limite = Instant::now() + Duration::from_secs(3);
            // Le lecteur rend sa copie d'`exited` en sortant, avec celle du maître.
            while exited.as_ref().is_some_and(|e| Arc::strong_count(e) > 1) {
                assert!(
                    Instant::now() < limite,
                    "PTY toujours ouvert 3 s après exit"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn vue_remontee_figee_pendant_la_sortie() {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            tape(&mut term, "seq 1 100; echo FI''NI");
            attend(&mut term, &mut s, |s| texte(s).contains("FINI"));
            s.terminal.scroll = 5;
            term.sync(&mut s);
            let vue = s.terminal.lines.clone();
            tape(&mut term, "seq 200 230");
            attend(&mut term, &mut s, |s| s.terminal.scroll > 20);
            assert_eq!(s.terminal.lines, vue);
            s.terminal.scroll = 0;
            attend(&mut term, &mut s, |s| texte(s).contains("230"));
            assert_eq!(s.terminal.scroll, 0);
            term.shutdown();
        }

        #[test]
        fn taille_suivie_et_vue_bornee() {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            let (cols, rows) = terminal_size(&s);
            assert_eq!(s.terminal.lines.len(), usize::from(rows));
            tape(&mut term, "stty size");
            let attendu = format!("{rows} {cols}");
            attend(&mut term, &mut s, |s| texte(s).contains(&attendu));
            s.width = 60;
            term.sync(&mut s);
            tape(&mut term, "stty size");
            let attendu = format!("{rows} 60");
            attend(&mut term, &mut s, |s| texte(s).contains(&attendu));
            assert!(s.terminal.cursor.is_some());
            s.terminal.scroll = 5000;
            term.sync(&mut s);
            assert!(s.terminal.scroll < 5000);
            term.shutdown();
        }

        #[test]
        fn fenetre_trop_petite_ne_lance_rien() {
            let (mut term, mut s) = (sh(), ouvert());
            s.height = 0;
            term.sync(&mut s);
            assert!(term.session.is_none());
        }

        #[test]
        fn echec_du_lancement_ferme_le_panneau() {
            let (mut term, mut s) = (sh(), ouvert());
            term.command = CommandBuilder::new("/inexistant/shell");
            term.sync(&mut s);
            assert!(!s.terminal.open);
            assert!(s.status.starts_with("Terminal : "), "{}", s.status);
        }

        #[test]
        fn shutdown_ne_laisse_pas_de_shell() -> anyhow::Result<()> {
            let (mut term, mut s) = (sh(), ouvert());
            term.sync(&mut s);
            let pid = term.session.as_ref().and_then(|t| t.child.process_id());
            let pid = pid.map(|p| p.to_string()).unwrap_or_default();
            assert!(!pid.is_empty());
            term.shutdown();
            let vivant = std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(std::process::Stdio::null())
                .status()?;
            assert!(!vivant.success());
            Ok(())
        }
    }
}
