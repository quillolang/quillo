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
        QuilloError { msg: crate::privacy::demangle(&msg.into()), line, file: None, fix: None, calls: None }
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
    if msg.starts_with("unexpected character") || msg.starts_with("unexpected keyword") || msg.starts_with("unknown escape") || msg.starts_with("bad escape") {
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
