use std::fmt;
use std::sync::Arc;

/// A parse or runtime error, with the line it happened on when known.
#[derive(Debug, Clone)]
pub struct QuilloError {
    pub msg: String,
    pub line: u32,
    /// Not a String: an error goes back through every call as a `Result`, and its size is
    /// what the deepest code has to fit on the stack, so `calls` takes these eight bytes.
    pub file: Option<Box<str>>,
    /// A change to the text that would mend it, offered by `quillo lsp`
    /// as a quick fix (branch editor-end-tags).
    pub fix: Option<Box<Fix>>,
    /// The calls it has come out of so far (FEEDBACK21.md, Q-174; branch feedback20-fixes).
    calls: Option<Box<Calls>>,
    /// The request's stop at `--max-memory`, told by this and never by its words, which a
    /// program, a map's key or another server's header can hold too (FEEDBACK71.md, Q-977,
    /// found in review; branch security).
    pub memory_stop: bool,
    /// The place in the source its message names, by line and column, which a template's
    /// `${…}` part, parsed on its own text, moves to where the part starts (FEEDBACK87.md,
    /// Q-1371, round 10, found in review; branch feedback86-89).
    pub named: Option<Box<Place>>,
}

/// A message naming a place by line and column: the words before and after the column, so that
/// the column can change without its words being searched (FEEDBACK87.md, Q-1371, round 10;
/// branch feedback86-89).
#[derive(Debug, Clone)]
pub struct Place {
    pub line: u32,
    pub col: u32,
    pub before: String,
    pub after: String,
}

/// The calls an error came out of, innermost first, kept as it passes up them
/// so that a call that succeeds pays nothing. Those of a deep recursion: the
/// first and the last `KEPT`.
#[derive(Debug, Clone, Default)]
struct Calls {
    kept: Vec<Call>,
    left_out: usize,
}

#[derive(Debug, Clone)]
struct Call {
    name: String,
    file: Option<Arc<str>>,
    line: u32,
}

const KEPT: usize = 5;

/// Replace `len` characters at `line` (from 1) and `col` (in characters,
/// from 0) with `text`.
#[derive(Debug, Clone)]
pub struct Fix {
    pub title: String,
    pub line: u32,
    pub col: u32,
    pub len: u32,
    pub text: String,
}

impl QuilloError {
    pub fn new(msg: impl Into<String>, line: u32) -> Self {
        // A private name as it was written (src/privacy.rs; branch public-private).
        QuilloError { msg: crate::privacy::demangle(&msg.into()), line, file: None, fix: None, calls: None, memory_stop: false, named: None }
    }

    /// This error's message as `named` says it: the words before its column, the column, and
    /// those after (FEEDBACK87.md, Q-1371, round 10; branch feedback86-89).
    pub fn with_named(mut self, n: Place) -> Self {
        self.msg = format!("{}{}{}", n.before, n.col, n.after);
        self.named = Some(Box::new(n));
        self
    }

    /// The place its message names moved `by` columns where it is on `line`: the first line of a
    /// template's part, which starts `by` columns into its line (round 10).
    pub fn shift_named(&mut self, line: u32, by: u32) {
        if let Some(n) = self.named.as_mut().filter(|n| n.line == line) {
            n.col += by;
            self.msg = format!("{}{}{}", n.before, n.col, n.after);
        }
    }

    /// The stop of a request whose value would take the server's memory past `--max-memory`.
    pub fn memory_stop(msg: impl Into<String>, line: u32) -> Self {
        QuilloError { memory_stop: true, ..QuilloError::new(msg, line) }
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(Box::new(fix));
        self
    }

    pub fn in_file(mut self, file: &str) -> Self {
        if self.file.is_none() {
            self.file = Some(file.into());
        }
        // Calls made at a file's top level, which is no function's.
        if let Some(c) = &mut self.calls {
            for call in c.kept.iter_mut().filter(|c| c.file.is_none()) {
                call.file = Some(Arc::from(file));
            }
        }
        self
    }

    /// Whether `in_file` has anything to tell it.
    pub fn wants_file(&self) -> bool {
        self.file.is_none() || self.calls.as_ref().is_some_and(|c| c.kept.iter().any(|c| c.file.is_none()))
    }

    /// The error leaves function `name`, which was called at `line` of `file`.
    #[cold]
    pub fn called(mut self, name: &str, file: Option<&Arc<str>>, line: u32) -> Self {
        let c = self.calls.get_or_insert_with(Default::default);
        if c.kept.len() == 2 * KEPT {
            c.kept.remove(KEPT);
            c.left_out += 1;
        }
        c.kept.push(Call { name: crate::privacy::demangle(name), file: file.cloned(), line });
        self
    }
}

impl fmt::Display for Call {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = if self.name == "<fn>" { "a function" } else { &self.name };
        match (&self.file, self.line) {
            (Some(file), l) if l > 0 => write!(f, "in {}, called at {}:{}", name, shown_path(file), l),
            (Some(file), _) => write!(f, "in {}, called in {}", name, shown_path(file)),
            (None, l) if l > 0 => write!(f, "in {}, called at line {}", name, l),
            _ => write!(f, "in {}", name),
        }
    }
}

/// `{}` is the one line the error has always been: what a visitor under
/// `--watch`, a test or another message goes on. `{:#}` adds the calls it came
/// through, a line each: for the log, and the page under `--watch`.
impl fmt::Display for QuilloError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = &self.msg;
        match (&self.file, self.line) {
            (Some(file), l) if l > 0 => write!(f, "{}:{}: {}", file, l, msg)?,
            (Some(file), _) => write!(f, "{}: {}", file, msg)?,
            (None, l) if l > 0 => write!(f, "line {}: {}", l, msg)?,
            _ => write!(f, "{}", msg)?,
        }
        if let (true, Some(calls)) = (f.alternate(), &self.calls) {
            for (i, c) in calls.kept.iter().enumerate() {
                if i == KEPT && calls.left_out > 0 {
                    write!(f, "\n  ... {} more call{}", calls.left_out, if calls.left_out == 1 { "" } else { "s" })?;
                }
                write!(f, "\n  {}", c)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for QuilloError {}

/// A program's file as messages name it: relative to the folder Quillo
/// runs in, when it is inside it (`ui/board.qwe`), else as it is.
pub fn shown_path(file: &str) -> String {
    let p = std::path::Path::new(file);
    std::env::current_dir()
        .ok()
        .and_then(|d| p.strip_prefix(&d).ok().map(|r| r.display().to_string()))
        .unwrap_or_else(|| file.to_string())
}

/// When this process's binary was built, as it was when the process
/// started: a long-running one (the language server) keeps running the old
/// code after a rebuild replaces the file.
static BUILT: std::sync::OnceLock<Option<std::time::SystemTime>> = std::sync::OnceLock::new();

fn exe_modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(std::env::current_exe().ok()?).ok()?.modified().ok()
}

/// Builtins Quillo had and removed in 0.5.0 (205d7cd, "for programs to do
/// as they need"), with what a program does instead: a program written
/// before then is told so, not that the name is unknown (FEEDBACK.md, Q-037;
/// branch feedback6-fixes).
const REMOVED: &[(&[&str], &str)] = &[
    (&["rate_limit", "rate_limit_reset"], "count attempts in a table of the program's own, as app/bank's customers service counts wrong PINs"),
    (&["audit", "audit.actor", "audit.query", "audit.verify"], "write what happened to a table of the program's own"),
    (&["idempotent"], "keep each request's id in a table with a unique key, and answer the first answer again"),
    (&["db.backup"], "back up with the database's own tools (sqlite3's .backup, pg_dump)"),
    (&["crypto.encrypt", "crypto.decrypt", "crypto.reencrypt", "crypto.totp_secret", "crypto.totp_uri", "crypto.totp_verify", "crypto.recovery_codes", "crypto.verify_recovery_code"],
        "write it in Rust, in a .qwer file the program uses"),
    (&["crypto.verify_webhook"], "compare crypto.hmac(secret, body) with the signature using crypto.equal"),
    (&["csp", "csp_nonce", "security_headers"], "set the headers on the response (respond(status, body, headers)) or at the proxy in front"),
    (&["redact"], "leave what must not be logged out of what is printed"),
    (&["sessions", "end_sessions", "user_sessions", "renew_session"], "sessions are signed cookies; keep a table of the program's own to end them early"),
];

/// ` — removed from Quillo in 0.5.0 (205d7cd): <what to do instead>`, for a
/// name that was a builtin, or what Quillo has for a word it never had
/// (FEEDBACK19.md, Q-152; branch feedback19-fixes), or nothing.
pub fn removed_note(name: &str) -> String {
    match name {
        "break" => return ": Quillo has no `break`: `return` leaves a loop and the function around it, and `find` stops at the first match".into(),
        "continue" => return ": Quillo has no `continue`: `filter` the list first, or put the rest of the loop in an `if`".into(),
        _ => {}
    }
    REMOVED
        .iter()
        .find(|(names, _)| names.contains(&name))
        .map(|(_, instead)| format!(": Quillo removed it in 0.5.0 (205d7cd); {}", instead))
        .unwrap_or_default()
}

/// Notes when this binary was built, for `which_quillo` later on.
pub fn note_start() {
    BUILT.get_or_init(exe_modified);
}

/// Whether a parse error could be code for a newer Quillo: something this
/// one does not know (an unknown character or escape, a keyword where it
/// does not expect one, a word where it expects something else: a keyword
/// it does not have yet), not a rule it names (`reply` is a keyword) nor a
/// typo (a bracket left open, the end of the file, a stray comma), which a
/// newer Quillo would refuse as well (FEEDBACK.md §5, Q-008; branch
/// feedback6-fixes).
pub fn may_be_newer_syntax(msg: &str) -> bool {
    // A character that cannot be seen is no newer syntax (FEEDBACK31.md, Q-386; branch
    // feedback27-35-fixes).
    if msg.starts_with("unexpected character U+") {
        return false;
    }
    // Nor is one outside ASCII, which no Quillo takes in code (letters are names'): `♥`, as in
    // a tag's text read as code (FEEDBACK87.md, Q-1371 and Q-008; branch feedback86-89).
    if msg.strip_prefix("unexpected character `").and_then(|r| r.chars().next()).is_some_and(|c| !c.is_ascii()) {
        return false;
    }
    // One that says what to write instead (`@`, a curly quote, `===`) names a rule, not newer
    // syntax (FEEDBACK45.md, Q-008, Q-520; branch feedback40-43).
    if msg.starts_with("unexpected character") {
        return !msg.contains(": ");
    }
    // A regular expression's escape is told what to write (FEEDBACK53.md, Q-008).
    if msg.contains("a regular expression's") {
        return false;
    }
    // An attribute's `{` named before what the parser wanted for it: as that alone would be
    // (FEEDBACK87.md, Q-1371, found in review; branch feedback86-89).
    if let Some((_, found)) = msg.split_once(": expected `}` to close it, ") {
        return may_be_newer_syntax(&format!("expected `}}`, {}", found));
    }
    if msg.starts_with("unexpected keyword") || msg.starts_with("unknown escape") || msg.starts_with("bad escape") {
        return true;
    }
    if !msg.starts_with("expected ") || msg.contains("which is a keyword") {
        return false;
    }
    // `expected X, found `word``: a word this Quillo does not know as syntax.
    match msg.rsplit_once("found `") {
        Some((_, rest)) => {
            let word = rest.split('`').next().unwrap_or("");
            !word.is_empty() && word.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && word.chars().all(|c| c.is_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// After a syntax error: which quillo this is, as code written for a newer
/// one fails to parse here (an old build left on the PATH, say).
pub fn which_quillo() -> String {
    let exe = std::env::current_exe().ok();
    let now = exe_modified();
    let then = *BUILT.get_or_init(|| now);
    let built = then
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            // Days since 1970 to a date (Howard Hinnant's civil_from_days).
            let z = (d.as_secs() / 86_400) as i64 + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let day = doy - (153 * mp + 2) / 5 + 1;
            let month = if mp < 10 { mp + 3 } else { mp - 9 };
            let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
            format!(", built {:04}-{:02}-{:02}", year, month, day)
        })
        .unwrap_or_default();
    let at = exe.map(|p| format!(" at {}", p.display())).unwrap_or_default();
    if then.is_some() && now != then {
        return format!(
            "this is quillo {}{}{}, which has been built again since this process started; restart it to use the new build",
            crate::version(),
            built,
            at
        );
    }
    format!("this is quillo {}{}{}; if the code was written for a newer Quillo, that would explain it", crate::version(), built, at)
}

/// A program's file as text. One that is not UTF-8 is refused with the line and column of the
/// first byte that is not (FEEDBACK31.md, Q-386; branch feedback27-35-fixes).
/// A program that is no file: `quillo serve`'s, at the path it is given (FEEDBACK36.md, Q-450;
/// branch feedback36-39).
static VIRTUAL: std::sync::OnceLock<(std::path::PathBuf, String)> = std::sync::OnceLock::new();

thread_local! {
    /// The files read, or tried, on this thread while `noting_reads` runs.
    static TRIED: std::cell::RefCell<Option<Vec<std::path::PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// `f`, and every file it read or tried to read as a program's: what a load that failed named,
/// for `--watch` to watch until it loads, a file not there yet included (FEEDBACK55.md, Q-836;
/// branch feedback55).
pub fn noting_reads<T>(f: impl FnOnce() -> T) -> (T, Vec<std::path::PathBuf>) {
    let outer = TRIED.with(|t| t.borrow_mut().replace(Vec::new()));
    let r = f();
    let mut files = TRIED.with(|t| std::mem::replace(&mut *t.borrow_mut(), outer)).unwrap_or_default();
    files.dedup();
    TRIED.with(|t| {
        if let Some(o) = t.borrow_mut().as_mut() {
            o.extend(files.iter().cloned());
        }
    });
    (r, files)
}

/// The most of one file Quillo reads whole for a program: a file it uses, its policy, its
/// app.toml, a style sheet or script of its own.
pub const MOST_READ: u64 = 64 << 20;

/// The most of one program source (`.qwe`, `.qwer`, the program named on the command line):
/// parsing takes about seventy times the text, and the largest real one, quillo-ui.qwe, is 10 MB
/// (FEEDBACK71.md, Q-982, found in review; branch security).
pub const MOST_SOURCE: u64 = 16 << 20;

/// The most of all the sources of one program that `quillo check`, the language server and
/// `quillo docs` read for it together (`with_source_budget`).
pub const MOST_PROGRAM: u64 = 32 << 20;

/// A file a program supplies, read whole: a regular file of at most `MOST_READ` bytes
/// (`MOST_SOURCE` for a `.qwe` or `.qwer`), asked of the file opened, not of its name. A link to
/// `/dev/zero` in a cloned repository was read without end by `quillo check` and the language
/// server, which took the machine's memory at about 4 GB a second; a FIFO held the read forever.
/// Each is refused as what it is (FEEDBACK71.md, Q-982; branch security). Opened without waiting
/// and without becoming the process's terminal, so a FIFO with no writer does not hold the open
/// either; a regular file reads the same. What the person names on the command line under
/// `/dev/` or `/proc/` may be anything (`name_file`), read to the same limit.
pub fn read_file(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    let source = path.extension().is_some_and(|e| e == "qwe" || e == "qwer");
    read_bounded(path, if source { MOST_SOURCE } else { MOST_READ })
}

fn read_bounded(path: &std::path::Path, most: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let named = named_index(path);
    if let Some(i) = named {
        if let Some(b) = lock_named()[i].held.clone() {
            return Ok(b);
        }
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, if named.is_some() { libc::O_NOCTTY } else { libc::O_NONBLOCK | libc::O_NOCTTY });
    let file = opts.open(path)?;
    let meta = file.metadata()?;
    // A folder is said as reading one said it before.
    #[cfg(unix)]
    if meta.is_dir() {
        return Err(std::io::Error::from_raw_os_error(libc::EISDIR));
    }
    if !meta.is_file() && named.is_none() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "it is not a regular file"));
    }
    let larger = || std::io::Error::new(std::io::ErrorKind::InvalidData, format!("it is larger than {} MiB", most >> 20));
    if meta.is_file() && meta.len() > most {
        return Err(larger());
    }
    let mut bytes = Vec::with_capacity(if meta.is_file() { meta.len() as usize } else { 0 });
    file.take(most + 1).read_to_end(&mut bytes)?;
    // It may have grown since it was asked.
    if bytes.len() as u64 > most {
        return Err(larger());
    }
    if let Some(i) = named.filter(|_| !meta.is_file()) {
        lock_named()[i].held = Some(bytes.clone());
    }
    Ok(bytes)
}

/// A file the person named on the command line (the program, a `--policy`) where it is a pipe or
/// a device of theirs: `quillo run <(...)`, `quillo run /dev/stdin`, `/proc/self/fd/3`. Only
/// such a path, under `/dev/` or `/proc/`, may be no regular file: a committed `main.qwe` that
/// links to a FIFO is refused, not waited on. What a pipe held is kept, as a pipe gives its text
/// once and a program is read more than once (checked, then run), which read nothing the second
/// time (FEEDBACK71.md, Q-982, found in review; branch security). Every file Quillo finds by
/// itself (an import, a `use`d file, the language server's neighbours, `public/`, `app.toml`, the
/// policy beside the program) stays held to a regular file.
struct Named {
    given: std::path::PathBuf,
    real: Option<std::path::PathBuf>,
    held: Option<Vec<u8>>,
}

static NAMED: std::sync::Mutex<Vec<Named>> = std::sync::Mutex::new(Vec::new());

fn lock_named() -> std::sync::MutexGuard<'static, Vec<Named>> {
    NAMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Notes `path`, named on the command line, as the person's own (see `Named`): only a path under
/// `/dev/` or `/proc/` is noted.
pub fn name_file(path: &std::path::Path) {
    let s = path.to_string_lossy();
    if !(s.starts_with("/dev/") || s.starts_with("/proc/")) {
        return;
    }
    let mut all = lock_named();
    if !all.iter().any(|n| n.given == path) {
        all.push(Named { given: path.to_path_buf(), real: path.canonicalize().ok(), held: None });
    }
}

fn named_index(path: &std::path::Path) -> Option<usize> {
    let all = lock_named();
    if all.is_empty() {
        return None;
    }
    all.iter().position(|n| n.given == path).or_else(|| {
        // Another spelling of the same name is asked of the system, only then.
        let real = path.canonicalize().ok()?;
        all.iter().position(|n| n.real.as_ref() == Some(&real))
    })
}

thread_local! {
    /// What is left of `MOST_PROGRAM` for the sources being read, while `with_source_budget` runs.
    static SOURCE_LEFT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// The sources counted so far, each once however often it is read.
    static SOURCE_COUNTED: std::cell::RefCell<std::collections::HashSet<std::path::PathBuf>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

/// `f`, with every program source it reads (`read_source`) counted against `MOST_PROGRAM`: past
/// it, a read is refused. Sixteen imports of 16 MiB each would otherwise each be parsed, at about
/// seventy times their size (FEEDBACK71.md, Q-982, found in review; branch security). A call
/// inside another shares its budget.
pub fn with_source_budget<T>(f: impl FnOnce() -> T) -> T {
    if SOURCE_LEFT.with(|l| l.get()).is_some() {
        return f();
    }
    SOURCE_LEFT.with(|l| l.set(Some(MOST_PROGRAM)));
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            SOURCE_LEFT.with(|l| l.set(None));
            SOURCE_COUNTED.with(|c| c.borrow_mut().clear());
        }
    }
    let _reset = Reset;
    f()
}

/// `read_file`, as text: `std::fs::read_to_string`'s answer for a file a program supplies
/// (FEEDBACK71.md, Q-982; branch security).
pub fn read_text(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    String::from_utf8(read_file(path.as_ref())?).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "stream did not contain valid UTF-8"))
}

/// Sets the program read for `path` from now on.
pub fn set_virtual(path: std::path::PathBuf, source: String) {
    let _ = VIRTUAL.set((path, source));
}

pub fn read_source(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    source_text(path.as_ref(), true)
}

/// `read_source`, of a file the person running the program names rather than one a repository
/// supplies, read whole as it comes: `.env` and `--env-file`, which may be `/dev/null` or a pipe a
/// secrets manager writes (FEEDBACK71.md, Q-982; branch security). Only `quillo run` reads them.
pub fn read_source_whole(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    source_text(path.as_ref(), false)
}

fn source_text(path: &std::path::Path, regular: bool) -> std::io::Result<String> {
    if let Some((p, s)) = VIRTUAL.get().filter(|(p, _)| p == path) {
        let _ = p;
        return Ok(s.clone());
    }
    TRIED.with(|t| {
        if let Some(files) = t.borrow_mut().as_mut() {
            files.push(path.to_path_buf());
        }
    });
    let bytes = if regular { read_bounded(path, MOST_SOURCE)? } else { std::fs::read(path)? };
    if regular {
        if let Some(left) = SOURCE_LEFT.with(|l| l.get()) {
            let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if !SOURCE_COUNTED.with(|c| c.borrow().contains(&key)) {
                if bytes.len() as u64 > left {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("the program's files come to more than {} MiB together", MOST_PROGRAM >> 20)));
                }
                SOURCE_LEFT.with(|l| l.set(Some(left - bytes.len() as u64)));
                SOURCE_COUNTED.with(|c| c.borrow_mut().insert(key));
            }
        }
    }
    String::from_utf8(bytes).map_err(|e| {
        let ok = &e.as_bytes()[..e.utf8_error().valid_up_to()];
        let line = ok.iter().filter(|&&b| b == b'\n').count() + 1;
        let col = std::str::from_utf8(ok).map_or(0, |s| s.rsplit('\n').next().map_or(0, |l| l.chars().count())) + 1;
        let bad = e.as_bytes()[ok.len()];
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("it is not UTF-8 text: byte 0x{:02X} at line {}, column {} (save it as UTF-8)", bad, line, col))
    })
}
