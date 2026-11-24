//! What whoever runs Quillo lets a program do (FEEDBACK41.md, Q-478; branch feedback40-43).
//!
//! ```toml
//! # /etc/quillo/policy.toml: the host's, for every program on the server
//! [plugins]
//! build = false                  # never run cargo when loading: prebuilt libraries only
//! allow = ["csv@1", "qr", "jwt@2.1.0"]
//!
//! [effects]
//! allow = ["db", "http", "fs", "time", "random"]
//!
//! [http]
//! private = false                # no loopback, private or link-local addresses
//! hosts = ["*"]
//!
//! [env]
//! names = ["QUILLO_SECRET", "DATABASE_URL"]
//!
//! [limits]
//! workers = 8
//! max_compute = "10s"
//! max_body = "10MB"
//! max_tool_calls = 60            # a minute, for each caller of /_quillo/mcp
//! max_memory = "512MB"           # no one value a request makes may be bigger
//! ```
//!
//! The host's file comes first, then any `--policy FILE`, then `quillo.policy` beside the
//! program. Each is a layer: a later one can only narrow, never widen, as each list is a
//! condition every layer must meet. What one refuses says which file refused it.
//!
//! The layers, in the order read (FEEDBACK82.md, Q-1281 and Q-1282, as the user decided them
//! 2026-11-22; branch feedback82-85): the anchor (`/etc/quillo/policy.toml`; on Windows
//! `%ProgramData%\quillo\policy.toml`, found through the system's known folder, never the current
//! drive; or the path a provider's own build compiles in as `QUILLO_HOST_POLICY`), the files the
//! anchor `include`s, `policy.d/*.toml` beside the anchor in name order, each file
//! `QUILLO_POLICY_PATH` names (a list, as `PATH` is), each `--policy FILE`, then `quillo.policy`.
//! Only the anchor may say `include = [...]` and `require_path = true` (then a start without the
//! variable stops). A file any of these names must be there, and a layer that is there but is no
//! readable file (a broken link, a folder) stops the program, naming it (Q-1283).
//!
//! What `quillo run`'s options may be (Q-1280), each a forced value, a ceiling or an off switch:
//!
//! ```toml
//! [run]
//! trust_proxy = ["10.0.0.0/8"]   # forced: `--trust-proxy` may only name networks inside it
//! with = false                   # off: no `--with`
//! watch = false                  # off: no `--watch` or `--reload`
//! describe = false               # off: no `--describe`
//! plain_http = false             # off: no `--plain-http`
//! split = false                  # off: no `--split` or `--scale`
//! join = false                   # off: no `--join` or `--register`
//! check = true                   # forced: no `--no-check`
//! instances = 2                  # ceilings: the least of the layers' and the option's
//! max_connections = 200
//! connections_per_client = 32
//! live_per_client = 16
//! drain = "30s"
//! socket_mode = "660"            # a ceiling of permission bits
//! listen = "unix"                # forced: a Unix socket only (`--listen unix:PATH`, `quillo fcgi`)
//! base = "/shop"                 # forced: the path it is served under
//! log = "json"                   # forced: the request log's format
//!
//! [fs]
//! paths = ["/srv/shared"]        # a ceiling for `--allow-path`; [] allows none
//!
//! [env]
//! set = { APP_ENV = "production" }   # what env() gives, whatever .env or the shell says
//! ```

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The host's policy, always read when it is there (the anchor on Unix: see `anchor`).
pub const HOST_FILE: &str = "/etc/quillo/policy.toml";
/// The program's own, beside its main file.
pub const PROGRAM_FILE: &str = "quillo.policy";
/// The variable that names further layers, a list as `PATH` is (FEEDBACK82.md, Q-1281; branch
/// feedback82-85).
pub const PATH_VAR: &str = "QUILLO_POLICY_PATH";

/// What a layer is, by where it was found (FEEDBACK82.md, Q-1286; branch feedback82-85).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The anchor: the host's file.
    Host,
    /// A file the anchor names with `include`.
    Included,
    /// A file of `policy.d/` beside the anchor.
    Dropin,
    /// A file `QUILLO_POLICY_PATH` names.
    PathVar,
    /// A `--policy FILE`.
    Named,
    /// `quillo.policy` beside the program.
    Program,
}

impl Kind {
    pub fn said(self) -> &'static str {
        match self {
            Kind::Host => "the host's file",
            Kind::Included => "included by the host's file",
            Kind::Dropin => "policy.d",
            Kind::PathVar => PATH_VAR,
            Kind::Named => "--policy",
            Kind::Program => "the program's",
        }
    }

    /// A provider's layer: the host's file and what it brings in (`include`, `policy.d`), and
    /// `QUILLO_POLICY_PATH`, set by whoever starts Quillo. A `--policy FILE` is the developer's,
    /// like the program's own `quillo.policy`: in the repository's hands (FEEDBACK83.md, question
    /// 5 as the user revised it on 2026-11-23; branch feedback82-85).
    pub fn is_providers(self) -> bool {
        matches!(self, Kind::Host | Kind::Included | Kind::Dropin | Kind::PathVar)
    }
}

/// A network as `--trust-proxy` takes one: an address and how many of its bits.
pub type Net = (IpAddr, u32);

/// What a policy says of `quillo run`'s options (FEEDBACK82.md, Q-1280, Part D; branch
/// feedback82-85): each a forced value, a ceiling or an off switch, which a later layer only
/// narrows.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// The proxies whose `X-Forwarded-For` is believed: every layer's networks at once.
    pub trust_proxy: Option<Vec<Net>>,
    /// The switches turned off (`with`, `watch`, ...), with the first file that did.
    pub off: Vec<(&'static str, PathBuf)>,
    /// `check = true`: `--no-check` is refused.
    pub check: bool,
    /// `instances`, `max_connections`, `connections_per_client`, `live_per_client`, and `drain`
    /// in ms: the least of the layers'.
    pub ceilings: Vec<(&'static str, u64)>,
    /// `socket_mode`: the permission bits a socket may have, every layer's at once.
    pub socket_mode: Option<u32>,
    /// `log`, `base` and `listen`: a value no later layer may change.
    pub forced: Vec<(&'static str, String)>,
}

#[derive(Debug, Clone)]
pub struct Policy {
    /// The files read, in order.
    pub files: Vec<PathBuf>,
    /// What each of `files` is (FEEDBACK82.md, Q-1286; branch feedback82-85).
    pub kinds: Vec<Kind>,
    /// The places looked at that had no layer, for `quillo policy`.
    pub absent: Vec<String>,
    /// The anchor's `include` and `require_path` (FEEDBACK82.md, Q-1281; branch feedback82-85).
    pub include: Vec<PathBuf>,
    pub require_path: bool,
    /// `[run]` (FEEDBACK82.md, Q-1280; branch feedback82-85).
    pub run: Run,
    /// Each layer's `[fs] paths`: where `--allow-path` may lead.
    pub fs_paths: Vec<(PathBuf, Vec<PathBuf>)>,
    /// `[env] set`: what env() gives for each name, whatever `.env` or the environment says.
    pub env_set: Vec<(String, String)>,
    /// Each value that took effect and the file it came from, by its `section.key`, for
    /// `quillo policy` (FEEDBACK82.md, Q-1286; branch feedback82-85).
    pub origin: Vec<(String, PathBuf)>,
    /// Every `section.key` each layer said, as written, and its file.
    pub said: Vec<(String, String, PathBuf)>,
    /// May cargo run when a plugin is used (false in any layer: never).
    pub build: bool,
    /// Where a build was forbidden.
    pub build_from: Option<PathBuf>,
    /// Each layer's plugins: `name` or `name@version`, the version a prefix of the plugin's.
    pub plugins: Vec<(PathBuf, Vec<String>)>,
    /// The effects every layer allows.
    pub effects: crate::ast::Effects,
    /// The first layer that took effects away.
    pub effects_from: Option<PathBuf>,
    /// May `http` reach loopback, private and link-local addresses.
    pub private: bool,
    pub private_from: Option<PathBuf>,
    /// Each layer's hosts: `api.stripe.com`, `*.mapbox.com`, `*`.
    pub hosts: Vec<(PathBuf, Vec<String>)>,
    /// Each layer's environment variables `env()` may read.
    pub env: Vec<(PathBuf, Vec<String>)>,
    pub workers: Option<usize>,
    pub max_compute: Option<Duration>,
    pub max_body: Option<usize>,
    /// Tool calls a minute each caller may make at the MCP door, as `--max-tool-calls` says it
    /// (FEEDBACK56.md, C.4, Q-860; branch feedback56).
    pub max_tool_calls: Option<usize>,
    /// The file each limit came from, for the messages that hold a program to it (FEEDBACK55.md,
    /// Q-849; branch feedback55).
    pub max_compute_from: Option<PathBuf>,
    /// What no one value a request makes may be bigger than, as `--max-memory` says it, and the file it came
    /// from (FEEDBACK71.md, Q-977; branch security).
    pub max_memory: Option<usize>,
    pub max_memory_from: Option<PathBuf>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            files: Vec::new(),
            kinds: Vec::new(),
            absent: Vec::new(),
            include: Vec::new(),
            require_path: false,
            run: Run::default(),
            fs_paths: Vec::new(),
            env_set: Vec::new(),
            origin: Vec::new(),
            said: Vec::new(),
            build: true,
            build_from: None,
            plugins: Vec::new(),
            effects: crate::ast::ALL_EFFECTS,
            effects_from: None,
            private: true,
            private_from: None,
            hosts: Vec::new(),
            env: Vec::new(),
            workers: None,
            max_compute: None,
            max_body: None,
            max_tool_calls: None,
            max_compute_from: None,
            max_memory: None,
            max_memory_from: None,
        }
    }
}

static POLICY: OnceLock<Policy> = OnceLock::new();

/// The policy this process runs under: none until `set`.
pub fn get() -> &'static Policy {
    static NONE: OnceLock<Policy> = OnceLock::new();
    POLICY.get().unwrap_or_else(|| NONE.get_or_init(Policy::default))
}

/// Sets the policy, once, before the program loads.
pub fn set(p: Policy) {
    let _ = POLICY.set(p);
}

/// The effects nothing in the program may go past.
pub fn ceiling() -> crate::ast::Effects {
    get().effects
}

/// Why the policy could not be read: `missing` when a layer that was asked for is not there (a
/// file `include` or `QUILLO_POLICY_PATH` names, or the variable `require_path` asks for), which
/// `quillo policy` tells from a broken one by its exit code (FEEDBACK82.md, Q-1286; branch
/// feedback82-85).
#[derive(Debug)]
pub struct LoadError {
    pub message: String,
    pub missing: bool,
}

impl From<String> for LoadError {
    fn from(message: String) -> Self {
        LoadError { message, missing: false }
    }
}

/// The anchor: the host's file, which every `quillo` reads and a developer cannot switch off. A
/// provider's own build may compile in another (`QUILLO_HOST_POLICY=/opt/host/policy.toml cargo
/// build`); on Windows it is `%ProgramData%\quillo\policy.toml`, which only administrators may
/// write, where `/etc/quillo/policy.toml` was `\etc` on whichever drive the program ran from and
/// any user could make it (FEEDBACK82.md, Q-1281 and Q-1282; branch feedback82-85).
pub fn anchor() -> Result<PathBuf, String> {
    if let Some(built) = option_env!("QUILLO_HOST_POLICY").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(built));
    }
    #[cfg(windows)]
    {
        program_data().map(|d| d.join("quillo").join("policy.toml")).ok_or_else(|| "cannot find the ProgramData folder, where the host's policy is".to_string())
    }
    #[cfg(not(windows))]
    Ok(PathBuf::from(HOST_FILE))
}

/// `FOLDERID_ProgramData`, from the system (`SHGetKnownFolderPath`), never from the environment,
/// which the process's starter sets.
#[cfg(windows)]
fn program_data() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);
    // {62AB5D82-FDC1-4DC3-A9DD-070D1D495D97}
    const PROGRAM_DATA: Guid = Guid(0x62AB_5D82, 0xFDC1, 0x4DC3, [0xA9, 0xDD, 0x07, 0x0D, 0x1D, 0x49, 0x5D, 0x97]);
    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: *mut core::ffi::c_void, path: *mut *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(p: *mut core::ffi::c_void);
    }
    let mut p: *mut u16 = std::ptr::null_mut();
    // SAFETY: the GUID and the out pointer are valid; the string it gives is freed below, as its
    // documentation says, whether or not the call succeeded.
    let hr = unsafe { SHGetKnownFolderPath(&PROGRAM_DATA, 0, std::ptr::null_mut(), &mut p) };
    let out = (hr >= 0 && !p.is_null()).then(|| {
        let mut n = 0;
        // SAFETY: a NUL-ended string of UTF-16 the system wrote.
        while unsafe { *p.add(n) } != 0 {
            n += 1;
        }
        PathBuf::from(std::ffi::OsString::from_wide(unsafe { std::slice::from_raw_parts(p, n) }))
    });
    if !p.is_null() {
        // SAFETY: allocated by the call above.
        unsafe { CoTaskMemFree(p.cast()) };
    }
    out
}

/// Whether a layer is at `f`: not there at all is none, while anything else at that name must be
/// a file, or the program stops naming it, as the host's file did already. A `quillo.policy` that
/// was a broken link or a folder was skipped without a word, and the program ran with no policy
/// (FEEDBACK82.md, Q-1283, as the user decided it 2026-11-22; branch feedback82-85).
fn there(f: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(f) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot read the policy {}: {}", f.display(), e)),
        Ok(m) if m.file_type().is_symlink() && std::fs::metadata(f).is_err() => {
            let to = std::fs::read_link(f).map_or_else(|_| "somewhere".to_string(), |t| t.display().to_string());
            Err(format!("cannot read the policy {}: it is a link to {}, which is not there", f.display(), to))
        }
        Ok(_) if f.is_dir() => Err(format!("cannot read the policy {}: it is a folder, not a policy file", f.display())),
        Ok(_) => Ok(true),
    }
}

/// The host's file, then `extra` (`--policy`), then the program's: each read that is there.
pub fn load(main: &Path, extra: &[PathBuf]) -> Result<Policy, String> {
    load_layers(main, extra).map_err(|e| e.message)
}

/// `load`, saying whether what stopped it was a layer asked for and not there.
pub fn load_layers(main: &Path, extra: &[PathBuf]) -> Result<Policy, LoadError> {
    // Always the environment's: the policy is read before the program's `.env`, which never sets
    // it (`dotenv::load_into_process`), nor is it ever taken from the names an app's file gave.
    // It was left out when `QUILLO_ENV_FROM_FILE` named it, which anyone starting Quillo could
    // set, and the program ran without the provider's layer (FEEDBACK82.md, Q-1281, found by the
    // coverage tests; branch feedback82-85).
    load_from(&anchor()?, main, extra, std::env::var_os(PATH_VAR))
}

/// The layers from `anchor`, `path_var` (`QUILLO_POLICY_PATH`'s value), `extra` and the program's
/// file, in that order (FEEDBACK82.md, Q-1281; branch feedback82-85).
pub fn load_from(anchor: &Path, main: &Path, extra: &[PathBuf], path_var: Option<std::ffi::OsString>) -> Result<Policy, LoadError> {
    let mut p = Policy::default();
    let missing = |message: String| LoadError { message, missing: true };
    // Only a host's file that is not there is no policy: one that cannot be read is an error, not
    // a policy skipped without a word.
    if there(anchor)? {
        p.read(anchor, Kind::Host)?;
    } else {
        p.absent.push(format!("{} ({})", anchor.display(), Kind::Host.said()));
    }
    for f in p.include.clone() {
        if !there(&f)? {
            return Err(missing(format!("the policy {} includes {}, which is not there", anchor.display(), f.display())));
        }
        p.read(&f, Kind::Included)?;
    }
    // Plans and drop-ins, in name order, without editing one file.
    let dropins = anchor.parent().unwrap_or(Path::new("/")).join("policy.d");
    match std::fs::symlink_metadata(&dropins) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => p.absent.push(format!("{}/ ({})", dropins.display(), Kind::Dropin.said())),
        Err(e) => return Err(format!("cannot read the policies in {}: {}", dropins.display(), e).into()),
        Ok(_) if !dropins.is_dir() => return Err(format!("cannot read the policies in {}: it is not a folder", dropins.display()).into()),
        Ok(_) => {
            let mut names: Vec<PathBuf> = std::fs::read_dir(&dropins)
                .map_err(|e| format!("cannot read the policies in {}: {}", dropins.display(), e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                // `.TOML` too: a provider's layer written in capitals was skipped without a word,
                // and the program ran without its limits. A layer is never left out unsaid
                // (`there`), and Quillo takes `.QWE` as `.qwe` (FEEDBACK82.md, Q-1281, found by
                // the coverage tests; branch feedback82-85).
                .filter(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
                .collect();
            names.sort();
            for f in names {
                there(&f)?;
                p.read(&f, Kind::Dropin)?;
            }
        }
    }
    // The variable adds layers where the provider starts the process; each file it names must be
    // there, and the anchor's `require_path` makes a start without it stop rather than run without
    // the provider's limits.
    let named: Vec<PathBuf> = path_var.as_ref().map(|v| std::env::split_paths(v).filter(|f| !f.as_os_str().is_empty()).collect()).unwrap_or_default();
    if named.is_empty() {
        if p.require_path {
            let why = if path_var.is_none() { "is not set" } else { "names no file" };
            return Err(missing(format!("the policy {} says require_path = true, and {} {}", anchor.display(), PATH_VAR, why)));
        }
        p.absent.push(format!("{} ({})", PATH_VAR, if path_var.is_none() { "not set" } else { "names no file" }));
    }
    for f in named {
        if !there(&f)? {
            return Err(missing(format!("{} names the policy {}, which is not there", PATH_VAR, f.display())));
        }
        p.read(&f, Kind::PathVar)?;
    }
    // A `--policy` is the person's to name: `--policy <(cat p.toml)` is read (FEEDBACK71.md, Q-982,
    // found in review; branch security).
    for f in extra {
        crate::error::name_file(f);
        p.read(f, Kind::Named)?;
    }
    p.program(main)?;
    Ok(p)
}

/// The program's own layer alone, for a phone app, which has no host's file nor environment of
/// the provider's (FEEDBACK82.md, Q-1284; branch feedback82-85).
pub fn load_program(main: &Path) -> Result<Policy, String> {
    let mut p = Policy::default();
    p.program(main)?;
    Ok(p)
}

impl Policy {
    /// One layer, read from a regular file of a bounded size: `quillo check` reads a cloned
    /// repository's (FEEDBACK71.md, Q-982; branch security).
    fn read(&mut self, f: &Path, kind: Kind) -> Result<(), String> {
        let text = crate::error::read_text(f).map_err(|e| format!("cannot read the policy {}: {}", f.display(), e))?;
        self.add_layer(f, &text, kind)
    }

    /// `quillo.policy` beside `main`, when there is one.
    fn program(&mut self, main: &Path) -> Result<(), String> {
        let program = main.parent().unwrap_or(Path::new(".")).join(PROGRAM_FILE);
        if there(&program)? {
            self.read(&program, Kind::Program)
        } else {
            self.absent.push(format!("{} ({})", program.display(), Kind::Program.said()));
            Ok(())
        }
    }

    /// Whether a layer of the host's is in force: its file, what it includes, policy.d's, or
    /// `QUILLO_POLICY_PATH`'s.
    pub fn has_host_layer(&self) -> bool {
        self.kinds.iter().any(|k| matches!(k, Kind::Host | Kind::Included | Kind::Dropin | Kind::PathVar))
    }

    /// The layers a `--split` service is given as `--policy`: the host's it reads itself.
    pub fn passed_on(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter().zip(&self.kinds).filter(|(_, k)| matches!(k, Kind::Named | Kind::Program)).map(|(f, _)| f)
    }
}

/// A value of the file's small TOML.
#[derive(Debug, Clone, PartialEq)]
enum V {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
    /// `{ APP_ENV = "production", REGION = "eu" }`, for `[env] set` (FEEDBACK82.md, Q-1280; branch
    /// feedback82-85).
    Table(Vec<(String, String)>),
}

/// A value as a layer wrote it, for `quillo policy`.
fn shown(v: &V) -> String {
    match v {
        V::Bool(b) => b.to_string(),
        V::Int(n) => n.to_string(),
        V::Str(s) => format!("\"{}\"", s),
        V::List(l) => format!("[{}]", l.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ")),
        V::Table(t) => format!("{{ {} }}", t.iter().map(|(k, v)| format!("{} = \"{}\"", k, v)).collect::<Vec<_>>().join(", ")),
    }
}

/// `{ A = "x", B = "y" }`: bare names, each a text; a `,` or `=` inside the text is the text's.
fn table(inner: &str) -> Option<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut rest = inner.trim();
    while !rest.is_empty() {
        let (k, after) = rest.split_once('=')?;
        let k = k.trim();
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') || out.iter().any(|(o, _)| o == k) {
            return None;
        }
        let after = after.trim_start().strip_prefix('"')?;
        let end = after.find('"')?;
        out.push((k.to_string(), after[..end].to_string()));
        rest = after[end + 1..].trim_start();
        rest = match rest.strip_prefix(',') {
            Some(r) => r.trim_start(),
            None if rest.is_empty() => rest,
            None => return None,
        };
    }
    Some(out)
}

/// `[section]` and `key = value` lines: booleans, integers, strings, and lists of strings
/// (which may span lines); `#` begins a comment.
fn parse(text: &str) -> Result<Vec<(String, String, V, usize)>, String> {
    // A byte-order mark is no part of the first line, as in `.qwe` and `.env` files (FEEDBACK55.md,
    // Q-852; branch feedback55).
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut out = Vec::new();
    let mut section = String::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((n, raw)) = lines.next() {
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.trim().to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { return Err(format!("line {}: expected `key = value` or `[section]`", n + 1)) };
        let mut v = v.trim().to_string();
        // A list may go on over lines.
        while v.starts_with('[') && !v.ends_with(']') {
            match lines.next() {
                Some((_, more)) => {
                    v.push(' ');
                    v.push_str(strip_comment(more).trim());
                }
                None => return Err(format!("line {}: a list with no `]`", n + 1)),
            }
        }
        let value = value(&v).ok_or_else(|| format!("line {}: `{}` is not true, false, a number, \"text\" or a list of \"text\"", n + 1, v))?;
        out.push((section.clone(), k.trim().to_string(), value, n + 1));
    }
    Ok(out)
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn value(v: &str) -> Option<V> {
    match v {
        "true" => return Some(V::Bool(true)),
        "false" => return Some(V::Bool(false)),
        _ => {}
    }
    if let Some(s) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return (!s.contains('"')).then(|| V::Str(s.to_string()));
    }
    if let Some(items) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let mut out = Vec::new();
        for item in items.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match value(item)? {
                V::Str(s) => out.push(s),
                _ => return None,
            }
        }
        return Some(V::List(out));
    }
    if let Some(inner) = v.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        return table(inner).map(V::Table);
    }
    v.replace('_', "").parse().ok().map(V::Int)
}

/// `"10s"`, `"500ms"`, `"2m"`.
fn duration(s: &str) -> Option<Duration> {
    let (n, unit) = s.find(|c: char| !c.is_ascii_digit()).map_or((s, "s"), |i| (&s[..i], &s[i..]));
    let n: u64 = n.parse().ok()?;
    // In milliseconds that fit, as `--max-compute` reads a time (FEEDBACK55.md, Q-849; branch
    // feedback55).
    n.checked_mul(match unit.trim() {
        "ms" => 1,
        "s" => 1000,
        "m" => 60_000,
        _ => return None,
    })
    .map(Duration::from_millis)
}

/// `"10MB"`, `"512KB"`, `1048576`.
fn bytes(v: &V) -> Option<usize> {
    match v {
        V::Int(n) if *n >= 0 => Some(*n as usize),
        V::Str(s) => size(s),
        _ => None,
    }
}

/// A size as people write one: `10MB`, `512KB`, `2GB`, `100B` or plain bytes, in any case
/// (KB, MB and GB are 1024, 1024² and 1024³ bytes). `--max-body` takes the same (branch
/// feedback40-43).
pub fn size(s: &str) -> Option<usize> {
    let s = s.trim().to_ascii_uppercase();
    let (n, mul) = if let Some(n) = s.strip_suffix("GB") {
        (n, 1usize << 30)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1 << 20)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1 << 10)
    } else {
        (s.strip_suffix('B').unwrap_or(&s), 1)
    };
    n.trim().parse::<usize>().ok().and_then(|n| n.checked_mul(mul))
}

/// `[run]`'s off switches, and the options each refuses (FEEDBACK82.md, Q-1280; branch
/// feedback82-85).
pub const SWITCHES: [&str; 6] = ["with", "watch", "describe", "plain_http", "split", "join"];

/// The options of `quillo run` a switch of `SWITCHES` turns off.
pub fn switch_options(switch: &str) -> &'static [&'static str] {
    match switch {
        "with" => &["--with"],
        "watch" => &["--watch", "-w", "--watch=reload", "--reload"],
        "describe" => &["--describe"],
        "plain_http" => &["--plain-http"],
        // And what serves as a service or lets copies join (found in review): `--as-service` serves
        // server fns to other processes; `--allow-join` takes copies, `--join-as` and `--service`
        // go with `--join` and `--register` (FEEDBACK82.md, Q-1280; branch feedback82-85).
        "split" => &["--split", "--scale", "--as-service"],
        "join" => &["--join", "--register", "--allow-join", "--join-as", "--service"],
        _ => &[],
    }
}

/// `--base`'s path: `/shop`, without a `/` at its end.
fn base_path(s: &str) -> Option<String> {
    let b = s.trim_end_matches('/');
    (b.starts_with('/') && !b.contains("//") && !b.contains('?')).then(|| b.to_string())
}

/// `10.0.0.0/8,127.0.0.1`, as `--trust-proxy` takes it; None if a part is neither an address nor
/// a network, or there is none. An IPv4 address written as IPv6 is IPv4.
pub fn nets(list: &str) -> Option<Vec<Net>> {
    let mut out = Vec::new();
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (ip, bits) = part.split_once('/').map_or((part, None), |(i, b)| (i, Some(b)));
        let ip: IpAddr = ip.parse().ok()?;
        let most = if ip.is_ipv4() { 32 } else { 128 };
        let bits = match bits {
            None => most,
            Some(b) => b.parse::<u32>().ok().filter(|b| *b <= most)?,
        };
        out.push(match ip {
            IpAddr::V6(a) => match a.to_ipv4_mapped() {
                Some(v4) if bits >= 96 => (IpAddr::V4(v4), bits - 96),
                _ => (ip, bits),
            },
            v4 => (v4, bits),
        });
    }
    (!out.is_empty()).then_some(out)
}

/// The first `bits` of `ip`, the rest zero, as a number of its family's width.
fn prefix(ip: IpAddr, bits: u32) -> u128 {
    match ip {
        IpAddr::V4(a) => (u32::from(a) as u128).checked_shr(32 - bits).unwrap_or(0),
        IpAddr::V6(a) => u128::from(a).checked_shr(128 - bits).unwrap_or(0),
    }
}

/// Whether network `inner` lies inside network `outer`.
pub fn within(inner: &Net, outer: &Net) -> bool {
    inner.0.is_ipv4() == outer.0.is_ipv4() && outer.1 <= inner.1 && prefix(inner.0, outer.1) == prefix(outer.0, outer.1)
}

/// The networks both lists hold: of each pair, the one inside the other.
fn intersect(a: &[Net], b: &[Net]) -> Vec<Net> {
    let mut out: Vec<Net> = Vec::new();
    for x in a {
        for y in b {
            let both = if within(x, y) {
                *x
            } else if within(y, x) {
                *y
            } else {
                continue;
            };
            if !out.contains(&both) {
                out.push(both);
            }
        }
    }
    out
}

/// A network as `--trust-proxy` writes it.
pub fn net_shown(n: &Net) -> String {
    let most = if n.0.is_ipv4() { 32 } else { 128 };
    if n.1 == most {
        n.0.to_string()
    } else {
        format!("{}/{}", n.0, n.1)
    }
}

impl Policy {
    /// One more layer, read from `file`: it narrows what is allowed.
    pub fn add(&mut self, file: &Path, text: &str) -> Result<(), String> {
        self.add_layer(file, text, Kind::Named)
    }

    /// The value of `key` took effect from `file` (FEEDBACK82.md, Q-1286; branch feedback82-85).
    fn took(&mut self, key: &str, file: &Path) {
        match self.origin.iter_mut().find(|(k, _)| k == key) {
            Some((_, f)) => *f = file.to_path_buf(),
            None => self.origin.push((key.to_string(), file.to_path_buf())),
        }
    }

    /// The file `key`'s value in force came from (`limits.workers`, `run.trust_proxy`, ...).
    pub fn origin_of(&self, key: &str) -> Option<&Path> {
        self.origin.iter().find(|(k, _)| k == key).map(|(_, f)| f.as_path())
    }

    /// `[run]`'s ceiling `key`, the least of the layers'.
    pub fn run_ceiling(&self, key: &str) -> Option<u64> {
        self.run.ceilings.iter().find(|(k, _)| *k == key).map(|(_, n)| *n)
    }

    /// The file that turned `[run]`'s switch `key` off, if one did.
    pub fn switched_off(&self, key: &str) -> Option<&Path> {
        self.run.off.iter().find(|(k, _)| *k == key).map(|(_, f)| f.as_path())
    }

    /// `[run]`'s forced `key` (`log`, `base`, `listen`).
    pub fn run_forced(&self, key: &str) -> Option<&str> {
        self.run.forced.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    fn ceiling(&mut self, key: &'static str, n: u64, file: &Path) {
        match self.run.ceilings.iter_mut().find(|(k, _)| *k == key) {
            Some((_, m)) if *m <= n => return,
            Some((_, m)) => *m = n,
            None => self.run.ceilings.push((key, n)),
        }
        self.took(&format!("run.{}", key), file);
    }

    /// One more layer, of `kind`: only the host's file may say `include` and `require_path`.
    pub fn add_layer(&mut self, file: &Path, text: &str, kind: Kind) -> Result<(), String> {
        let at = |line: usize, why: String| format!("{}:{}: {}", file.display(), line, why);
        self.files.push(file.to_path_buf());
        self.kinds.push(kind);
        let names_before = self.env.len();
        let mut set_here: Vec<String> = Vec::new();
        for (section, key, v, line) in parse(text).map_err(|e| format!("{}: {}", file.display(), e))? {
            let list = |v: &V| match v {
                V::List(l) => Ok(l.clone()),
                _ => Err(at(line, format!("`{}` is a list: [\"...\", ...]", key))),
            };
            let name = if section.is_empty() { key.clone() } else { format!("{}.{}", section, key) };
            self.said.push((name.clone(), shown(&v), file.to_path_buf()));
            match (section.as_str(), key.as_str(), &v) {
                // The anchor ties the other layers to it (FEEDBACK82.md, Q-1281; branch
                // feedback82-85).
                ("", "include" | "require_path", _) if kind != Kind::Host => {
                    return Err(at(line, format!("`{}` is said only by the host's file, {}", key, anchor().map_or_else(|e| e, |a| a.display().to_string()))));
                }
                ("", "include", _) => {
                    for f in list(&v)? {
                        if !Path::new(&f).is_absolute() {
                            return Err(at(line, format!("`include` names files by their whole path, not \"{}\"", f)));
                        }
                        self.include.push(PathBuf::from(f));
                    }
                }
                ("", "require_path", V::Bool(b)) => self.require_path |= *b,
                ("plugins", "build", V::Bool(b)) => {
                    if !b && self.build {
                        self.build = false;
                        self.build_from = Some(file.to_path_buf());
                    }
                }
                ("plugins", "allow", _) => {
                    let allow = list(&v)?;
                    // A plugin held to its hash (FEEDBACK82.md, Q-1285; branch feedback82-85).
                    for a in &allow {
                        if let Some(h) = a.split_once('@').and_then(|(_, v)| v.strip_prefix("sha256:")) {
                            if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                                return Err(at(line, format!("`{}`: a plugin's SHA-256 is 64 hexadecimal digits, as `qr@sha256:9f86d0...`", a)));
                            }
                        }
                    }
                    self.plugins_of_layer(file, allow);
                }
                // Only the catalog's files, each as the catalog has it: by the SHA-256 a copy of
                // its index gives, which the provider keeps (`true`: `catalog.json` beside the
                // file that says it). A plugin is then held as one pinned by its source's hash, so
                // it is built here from the bytes hashed (FEEDBACK82.md, Q-1285, and FEEDBACK83.md,
                // Q-1307; branch feedback82-85).
                ("plugins", "catalog", V::Bool(false)) => {}
                ("plugins", "catalog", V::Bool(true) | V::Str(_)) => {
                    let named = match &v {
                        V::Str(s) => PathBuf::from(s),
                        _ => PathBuf::from("catalog.json"),
                    };
                    let index = if named.is_absolute() { named } else { file.parent().unwrap_or(Path::new("/")).join(named) };
                    let pins = catalog_pins(&index).map_err(|e| at(line, e))?;
                    self.plugins_of_layer(file, pins);
                }
                ("effects", "allow", _) => {
                    let mut mask = 0;
                    for e in list(&v)? {
                        match crate::ast::effect_bit(&e) {
                            0 => return Err(at(line, format!("`{}` is not an effect (they are {})", e, crate::ast::EFFECTS.join(", ")))),
                            b => mask |= b,
                        }
                    }
                    if self.effects & !mask != 0 && self.effects_from.is_none() {
                        self.effects_from = Some(file.to_path_buf());
                    }
                    self.effects &= mask;
                }
                ("http", "private", V::Bool(b)) => {
                    if !b && self.private {
                        self.private = false;
                        self.private_from = Some(file.to_path_buf());
                    }
                }
                ("http", "hosts", _) => self.hosts.push((file.to_path_buf(), list(&v)?)),
                ("env", "names", _) => self.env.push((file.to_path_buf(), list(&v)?)),
                // The command line's ranges: 0 is no limit to nginx and refused by `--max-body 0`,
                // and was taken as one that refused every body, or ended every request at once
                // (FEEDBACK55.md, Q-849; branch feedback55).
                ("limits", "workers", V::Int(n)) if *n > 0 => {
                    if self.workers.map_or(true, |w| (*n as usize) < w) {
                        self.workers = Some(*n as usize);
                        self.took("limits.workers", file);
                    }
                }
                ("limits", "workers", V::Int(_)) => return Err(at(line, "`workers` is a positive number, as `--workers` takes".into())),
                ("limits", "max_compute", V::Str(s)) => {
                    let d = duration(s).ok_or_else(|| at(line, format!("`{}` is not a time such as \"10s\" or \"500ms\"", s)))?;
                    if d.is_zero() {
                        return Err(at(line, format!("`max_compute` is a time longer than 0, as `--max-compute` takes, not \"{}\"", s)));
                    }
                    if self.max_compute.map_or(true, |m| d < m) {
                        self.max_compute = Some(d);
                        self.max_compute_from = Some(file.to_path_buf());
                    }
                }
                ("limits", "max_body", _) => {
                    let b = bytes(&v).ok_or_else(|| at(line, "`max_body` is a size such as \"10MB\"".into()))?;
                    if b == 0 {
                        return Err(at(line, "`max_body` is a size larger than 0, as `--max-body` takes, such as \"10MB\"".into()));
                    }
                    if self.max_body.map_or(true, |m| b < m) {
                        self.max_body = Some(b);
                        self.took("limits.max_body", file);
                    }
                }
                // As `--max-memory` takes it, the least of the layers' (FEEDBACK71.md, Q-977; branch
                // security).
                ("limits", "max_memory", _) => {
                    let b = bytes(&v).ok_or_else(|| at(line, "`max_memory` is a size such as \"512MB\"".into()))?;
                    if b < crate::httpd::LARGE {
                        return Err(at(line, "`max_memory` is a size of 64KB or more, as `--max-memory` takes, such as \"512MB\"".into()));
                    }
                    if self.max_memory.map_or(true, |m| b < m) {
                        self.max_memory = Some(b);
                        self.max_memory_from = Some(file.to_path_buf());
                    }
                }
                ("limits", "max_tool_calls", V::Int(n)) if *n > 0 => {
                    if self.max_tool_calls.map_or(true, |m| (*n as usize) < m) {
                        self.max_tool_calls = Some(*n as usize);
                        self.took("limits.max_tool_calls", file);
                    }
                }
                ("limits", "max_tool_calls", V::Int(_)) => return Err(at(line, "`max_tool_calls` is a positive number, the calls a minute, as `--max-tool-calls` takes".into())),
                // `[run]`, `[fs] paths` and `[env] set` (FEEDBACK82.md, Q-1280, Part D, as the user
                // decided it 2026-11-22; branch feedback82-85).
                ("run", "trust_proxy", _) => {
                    let mut given = Vec::new();
                    for n in list(&v)? {
                        match nets(&n) {
                            Some(mut one) if one.len() == 1 => given.append(&mut one),
                            _ => return Err(at(line, format!("`{}` is not an address or a network, as 127.0.0.1 or 10.0.0.0/8", n))),
                        }
                    }
                    let now = match self.run.trust_proxy.take() {
                        None => given,
                        Some(before) => intersect(&before, &given),
                    };
                    self.run.trust_proxy = Some(now);
                    self.took("run.trust_proxy", file);
                }
                ("run", k @ ("with" | "watch" | "describe" | "plain_http" | "split" | "join"), V::Bool(b)) => {
                    let k: &'static str = SWITCHES.iter().find(|s| **s == k).copied().unwrap_or("with");
                    if !b && self.switched_off(k).is_none() {
                        self.run.off.push((k, file.to_path_buf()));
                        self.took(&format!("run.{}", k), file);
                    }
                }
                ("run", "check", V::Bool(b)) => {
                    if *b && !self.run.check {
                        self.run.check = true;
                        self.took("run.check", file);
                    }
                }
                ("run", k @ ("instances" | "max_connections" | "connections_per_client" | "live_per_client"), V::Int(n)) if *n > 0 => {
                    let k: &'static str = ["instances", "max_connections", "connections_per_client", "live_per_client"].into_iter().find(|s| *s == k).unwrap_or("instances");
                    self.ceiling(k, *n as u64, file);
                }
                ("run", k @ ("instances" | "max_connections" | "connections_per_client" | "live_per_client"), V::Int(_)) => {
                    return Err(at(line, format!("`{}` is a positive number, as `--{}` takes", k, k.replace('_', "-"))));
                }
                ("run", "drain", V::Str(s)) => match duration(s) {
                    Some(d) if !d.is_zero() => self.ceiling("drain", d.as_millis() as u64, file),
                    _ => return Err(at(line, format!("`drain` is a time longer than 0, as `--drain` takes, such as \"30s\", not \"{}\"", s))),
                },
                ("run", "socket_mode", V::Str(s)) => match u32::from_str_radix(s, 8) {
                    Ok(m) if m <= 0o777 && m & 0o600 == 0o600 => {
                        let now = self.run.socket_mode.map_or(m, |before| before & m);
                        if self.run.socket_mode != Some(now) {
                            self.run.socket_mode = Some(now);
                            self.took("run.socket_mode", file);
                        }
                    }
                    _ => return Err(at(line, format!("`socket_mode` is octal permissions the owner can read and write, as `--socket-mode` takes, such as \"660\", not \"{}\"", s))),
                },
                ("run", k @ ("log" | "base" | "listen"), V::Str(s)) => {
                    let ok = match k {
                        "log" => matches!(s.as_str(), "text" | "json" | "off"),
                        "listen" => s == "unix",
                        _ => base_path(s).is_some(),
                    };
                    if !ok {
                        let what = match k {
                            "log" => "text, json or off, as `--log` takes",
                            "listen" => "\"unix\": a Unix socket only",
                            _ => "a path such as \"/shop\", as `--base` takes",
                        };
                        return Err(at(line, format!("`{}` is {}, not \"{}\"", k, what, s)));
                    }
                    let s = if k == "base" { base_path(s).unwrap_or_default() } else { s.clone() };
                    let k: &'static str = ["log", "base", "listen"].into_iter().find(|f| *f == k).unwrap_or("log");
                    match self.run_forced(k) {
                        Some(before) if before == s => {}
                        Some(before) => {
                            let from = self.origin_of(&format!("run.{}", k)).map_or_else(String::new, |f| f.display().to_string());
                            return Err(at(line, format!("`run.{}` is \"{}\" in {}, which a later layer cannot change", k, before, from)));
                        }
                        None => {
                            self.run.forced.push((k, s));
                            self.took(&format!("run.{}", k), file);
                        }
                    }
                }
                ("fs", "paths", _) => {
                    let mut paths = Vec::new();
                    for f in list(&v)? {
                        if !Path::new(&f).is_absolute() {
                            return Err(at(line, format!("`paths` names folders by their whole path, not \"{}\"", f)));
                        }
                        // As `--allow-path` takes a folder, its links followed.
                        paths.push(Path::new(&f).canonicalize().unwrap_or_else(|_| PathBuf::from(&f)));
                    }
                    self.fs_paths.push((file.to_path_buf(), paths));
                }
                ("env", "set", V::Table(t)) => {
                    for (k, val) in t {
                        self.force_env(k, val, file, kind).map_err(|e| at(line, e))?;
                        set_here.push(k.clone());
                    }
                }
                ("env.set", k, V::Str(val)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => {
                    self.force_env(k, val, file, kind).map_err(|e| at(line, e))?;
                    set_here.push(k.to_string());
                }
                (s, k, _) => {
                    let known = [
                        "plugins.build", "plugins.allow", "plugins.catalog", "effects.allow", "http.private", "http.hosts", "env.names", "env.set", "limits.workers", "limits.max_compute", "limits.max_body", "limits.max_tool_calls", "limits.max_memory",
                        "run.trust_proxy", "run.with", "run.watch", "run.describe", "run.plain_http", "run.split", "run.join", "run.check", "run.instances", "run.max_connections", "run.connections_per_client", "run.live_per_client",
                        "run.drain", "run.socket_mode", "run.listen", "run.base", "run.log", "fs.paths",
                    ];
                    let name = format!("{}.{}", s, k);
                    let known_here = known.contains(&name.as_str()) || s == "env.set" || (s.is_empty() && matches!(k, "include" | "require_path"));
                    return Err(at(line, if known_here { format!("`{}` has the wrong kind of value", name) } else { format!("`{}` is not something a policy says (it says {}, and the host's file include and require_path)", name, known.join(", ")) }));
                }
            }
        }
        // What a layer sets, it lets env() read, beside the names it gives.
        if self.env.len() > names_before {
            if let Some((_, names)) = self.env.last_mut() {
                for k in set_here {
                    if !names.contains(&k) {
                        names.push(k);
                    }
                }
            }
        }
        Ok(())
    }

    /// A layer's plugins: its `allow` and its `catalog`'s are one list, either letting a plugin in,
    /// as both are the one provider's; the layers' lists each must let it in (FEEDBACK82.md,
    /// Q-1285, found in review 2 of FEEDBACK83.md; branch feedback82-85).
    fn plugins_of_layer(&mut self, file: &Path, list: Vec<String>) {
        match self.plugins.iter_mut().find(|(f, _)| f == file) {
            Some((_, have)) => have.extend(list),
            None => self.plugins.push((file.to_path_buf(), list)),
        }
    }

    /// `[env] set`'s `name` = `value`: a name an earlier layer set keeps its value.
    /// Only the host's layers may say it: a value the program's own layer or a `--policy` put in
    /// the environment narrows nothing, and would reach every process started beside the program
    /// (`--with`'s commands, cargo, a `--split` service, which reads `QUILLO_POLICY_PATH` again).
    /// Nor may any layer set the names that choose the policy or what a process loads, so that a
    /// host's own slip cannot drop its layers (FEEDBACK82.md, Q-1280, found in review; branch
    /// feedback82-85).
    fn force_env(&mut self, name: &str, value: &str, file: &Path, kind: Kind) -> Result<(), String> {
        if !matches!(kind, Kind::Host | Kind::Included | Kind::Dropin | Kind::PathVar) {
            // Named as a layer of its kind is: `said()` is "the program's" already, which read
            // "not the program's's" (FEEDBACK82.md, Q-1280, found by the coverage tests; branch
            // feedback82-85).
            let whose = if kind == Kind::Program { "the program's own (quillo.policy)" } else { "a --policy file's" };
            return Err(format!("`env.set` is the host's to say (its file, what it includes, policy.d, {}), not {}", PATH_VAR, whose));
        }
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("QUILLO_POLICY") || upper.starts_with("LD_") || upper.starts_with("DYLD_") {
            return Err(format!("`env.set` may not set {}: it chooses the policy, or what a process loads", name));
        }
        match self.env_set.iter().find(|(k, _)| k == name) {
            Some((_, before)) if before == value => Ok(()),
            // Its name only: a value may be a secret (found in review).
            Some(_) => {
                let from = self.origin_of(&format!("env.set.{}", name)).map_or_else(String::new, |f| f.display().to_string());
                Err(format!("`env.set.{}` is set in {}, which a later layer cannot change", name, from))
            }
            None => {
                self.env_set.push((name.to_string(), value.to_string()));
                self.took(&format!("env.set.{}", name), file);
                Ok(())
            }
        }
    }

    /// Why `--allow-path` may not name `real` (a folder, its links followed), if it may not
    /// (FEEDBACK82.md, Q-1280; branch feedback82-85).
    pub fn allows_path(&self, real: &Path) -> Result<(), String> {
        for (file, paths) in &self.fs_paths {
            if paths.is_empty() {
                return Err(format!("--allow-path {}: {} allows no folder outside the program's ([fs] paths = [])", real.display(), file.display()));
            }
            // By whichever name each was given: `[fs] paths = ["/System/Volumes/Data/Users/x"]`
            // held no `--allow-path /Users/x/shared` on macOS, the same folder by its other name
            // (FEEDBACK82.md, Q-1285, found by the coverage tests; branch feedback82-85).
            if !paths.iter().any(|p| crate::statics::within(real, p)) {
                let list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
                return Err(format!("--allow-path {} is not inside the folders {} allows: {}", real.display(), file.display(), list.join(", ")));
            }
        }
        Ok(())
    }

    /// Whether `--trust-proxy`'s networks lie inside the policy's: the first that does not.
    pub fn wider_proxy<'a>(&self, given: &'a [Net]) -> Option<&'a Net> {
        let allowed = self.run.trust_proxy.as_ref()?;
        given.iter().find(|n| !allowed.iter().any(|a| within(n, a)))
    }

    fn where_(&self, f: &Option<PathBuf>) -> String {
        f.as_ref().map_or_else(|| "the policy".to_string(), |p| p.display().to_string())
    }

    /// Why plugin `name` (at `version`, its `//! version:`) may not be used, if it may not.
    /// `hashes` are what it is by SHA-256, in lower-case hex: its source's (what the catalog
    /// gives for it) and its built library's, which `name@sha256:...` must match, so that a file
    /// renamed to an allowed name does not pass (FEEDBACK82.md, Q-1285; branch feedback82-85).
    pub fn plugin(&self, name: &str, version: Option<&str>, hashes: &[String]) -> Result<(), String> {
        for (file, allow) in &self.plugins {
            let ok = allow.iter().any(|a| match a.split_once('@') {
                None => a == name,
                Some((n, v)) => match v.strip_prefix("sha256:") {
                    Some(h) => n == name && hashes.iter().any(|have| have.eq_ignore_ascii_case(h)),
                    None => n == name && version.is_some_and(|have| have == v || have.starts_with(&format!("{}.", v))),
                },
            });
            if !ok {
                let said = version.map_or(String::new(), |v| format!(" {}", v));
                // Its hash, when a layer pins one by it: what to put in the list, or why it differs.
                let pinned = allow.iter().any(|a| a.split_once('@').is_some_and(|(n, v)| n == name && v.starts_with("sha256:")));
                let its = match hashes.first() {
                    Some(h) if pinned => format!(" (its SHA-256 is {})", h),
                    _ => String::new(),
                };
                // A catalog's index names hundreds: said by their number (FEEDBACK82.md, Q-1285).
                let list = if allow.len() > 12 { format!("{} plugins, each by its SHA-256", allow.len()) } else { allow.join(", ") };
                return Err(format!("the plugin `{}`{} is not in the plugins {} allows: {}{}", name, said, file.display(), list, its));
            }
        }
        Ok(())
    }

    /// Whether a layer holds a plugin to its SHA-256, so that its hashes are worth working out.
    pub fn pins_by_hash(&self) -> bool {
        self.plugins.iter().any(|(_, allow)| allow.iter().any(|a| a.contains("@sha256:")))
    }

    /// Why `env()` may not read `name`, if it may not.
    pub fn env_name(&self, name: &str) -> Result<(), String> {
        for (file, names) in &self.env {
            if !names.iter().any(|n| n == name) {
                return Err(format!("`{}` is not among the variables {} lets env() read", name, file.display()));
            }
        }
        Ok(())
    }

    /// Whether `http` is held to hosts or kept from private addresses.
    pub fn limits_http(&self) -> bool {
        !self.hosts.is_empty() || !self.private
    }

    /// Why `http` may not reach `host`, as `quillo check` can say before a deploy: a host no layer
    /// allows (FEEDBACK83.md, Q-1302; branch feedback82-85). `private = false` is judged as the
    /// call runs, by where the name leads then.
    pub fn refuses_host(&self, host: &str) -> Option<String> {
        let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        for (file, hosts) in &self.hosts {
            if !hosts.iter().any(|h| host_matches(h, &host)) {
                return Some(format!("{} is not among the hosts {} allows: {}", host, file.display(), hosts.join(", ")));
            }
        }
        None
    }

    /// Why `http` may not reach `url`, if it may not: a host no layer allows, or (with `private =
    /// false`) an address on this machine or its network, as the name resolves now.
    pub fn http(&self, url: &str) -> Result<(), String> {
        if self.hosts.is_empty() && self.private {
            return Ok(());
        }
        let Ok(u) = url::Url::parse(url) else { return Ok(()) };
        let host = u.host_str().unwrap_or("").trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        for (file, hosts) in &self.hosts {
            if !hosts.iter().any(|h| host_matches(h, &host)) {
                return Err(format!("{} is not among the hosts {} allows: {}", host, file.display(), hosts.join(", ")));
            }
        }
        if !self.private {
            let port = u.port_or_known_default().unwrap_or(80);
            let addrs: Vec<IpAddr> = match host.parse::<IpAddr>() {
                Ok(ip) => vec![ip],
                Err(_) => std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port)).map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default(),
            };
            if let Some(ip) = addrs.into_iter().find(|ip| private_ip(*ip)) {
                let what = if host == ip.to_string() { host.clone() } else { format!("{} ({})", host, ip) };
                return Err(format!("{} is an address of this machine or its network, which {} does not let a program reach", what, self.where_(&self.private_from)));
            }
        }
        Ok(())
    }

    /// What `quillo check --policy` says of it, beside the program's own findings.
    pub fn summary(&self) -> String {
        let mut out = Vec::new();
        if self.effects != crate::ast::ALL_EFFECTS {
            out.push(format!("effects: {}", crate::ast::effect_names(self.effects).join(", ")));
        }
        if !self.build {
            out.push("no builds".into());
        }
        if !self.private {
            out.push("no private addresses".into());
        }
        // Its limits, so a request held to one is seen to be (FEEDBACK55.md, Q-849; branch
        // feedback55).
        if let Some(w) = self.workers {
            out.push(format!("workers {}", w));
        }
        if let Some(c) = self.max_compute {
            out.push(format!("max_compute {}ms", c.as_millis()));
        }
        if let Some(b) = self.max_body {
            out.push(format!("max_body {} byte{}", b, if b == 1 { "" } else { "s" }));
        }
        if let Some(n) = self.max_tool_calls {
            out.push(format!("max_tool_calls {} a minute", n));
        }
        if let Some(m) = self.max_memory {
            out.push(format!("max_memory {} bytes", m));
        }
        // And what it says of the options, so a start held to it is seen to be (FEEDBACK82.md,
        // Q-1280; branch feedback82-85).
        if let Some(t) = &self.run.trust_proxy {
            out.push(if t.is_empty() { "no proxy trusted".to_string() } else { format!("trust_proxy {}", t.iter().map(net_shown).collect::<Vec<_>>().join(",")) });
        }
        for (k, _) in &self.run.off {
            out.push(format!("no {}", switch_options(k)[0]));
        }
        if self.run.check {
            out.push("checked".into());
        }
        for (k, n) in &self.run.ceilings {
            out.push(if *k == "drain" { format!("drain {}ms", n) } else { format!("{} {}", k, n) });
        }
        if let Some(m) = self.run.socket_mode {
            out.push(format!("socket_mode {:o}", m));
        }
        for (k, v) in &self.run.forced {
            out.push(format!("{} {}", k, v));
        }
        if !self.fs_paths.is_empty() {
            out.push("allow-path held".into());
        }
        if !self.env_set.is_empty() {
            out.push(format!("env set: {}", self.env_set.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")));
        }
        format!("policy {}{}", self.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(" + "), if out.is_empty() { String::new() } else { format!(" ({})", out.join("; ")) })
    }

    /// Where effects were taken away, for a message.
    pub fn effects_source(&self) -> String {
        self.where_(&self.effects_from)
    }

    /// What `quillo policy` prints: the layers, the places that had none, and each setting in
    /// force with the file it came from (FEEDBACK82.md, Q-1286; branch feedback82-85).
    pub fn report(&self) -> String {
        let mut out = Vec::new();
        if self.files.is_empty() {
            out.push("no policy: nothing narrows what the program may do".to_string());
        } else {
            out.push("layers, in the order read (each only narrows the ones before it):".to_string());
            for (f, k) in self.files.iter().zip(&self.kinds) {
                out.push(format!("  {} ({})", f.display(), k.said()));
            }
        }
        if !self.absent.is_empty() {
            out.push("not there:".to_string());
            for a in &self.absent {
                out.push(format!("  {}", a));
            }
        }
        let from = |f: Option<&Path>| f.map_or_else(String::new, |f| format!(" ({})", f.display()));
        let mut said = Vec::new();
        if !self.build {
            said.push(format!("plugins.build = false{}", from(self.build_from.as_deref())));
        }
        // A list is a condition each layer's meets: each is shown with its file.
        let each = |key: &str, layers: &[(PathBuf, Vec<String>)]| -> Option<String> {
            (!layers.is_empty()).then(|| {
                let l: Vec<String> = layers.iter().map(|(f, v)| format!("[{}] ({})", v.join(", "), f.display())).collect();
                format!("{}: in every one of {}", key, l.join(", "))
            })
        };
        said.extend(each("plugins.allow", &self.plugins));
        if self.effects != crate::ast::ALL_EFFECTS {
            said.push(format!("effects.allow = {}{}", crate::ast::effect_names(self.effects).join(", "), from(self.effects_from.as_deref())));
        }
        if !self.private {
            said.push(format!("http.private = false{}", from(self.private_from.as_deref())));
        }
        said.extend(each("http.hosts", &self.hosts));
        said.extend(each("env.names", &self.env));
        for (k, v) in &self.env_set {
            // Its name only: a value may be a secret (found in review).
            let _ = v;
            said.push(format!("env.set.{} (set){}", k, from(self.origin_of(&format!("env.set.{}", k)))));
        }
        let paths: Vec<(PathBuf, Vec<String>)> = self.fs_paths.iter().map(|(f, p)| (f.clone(), p.iter().map(|p| p.display().to_string()).collect())).collect();
        said.extend(each("fs.paths", &paths));
        if let Some(w) = self.workers {
            said.push(format!("limits.workers = {}{}", w, from(self.origin_of("limits.workers"))));
        }
        if let Some(c) = self.max_compute {
            said.push(format!("limits.max_compute = {}ms{}", c.as_millis(), from(self.max_compute_from.as_deref())));
        }
        if let Some(b) = self.max_body {
            said.push(format!("limits.max_body = {} bytes{}", b, from(self.origin_of("limits.max_body"))));
        }
        if let Some(m) = self.max_memory {
            said.push(format!("limits.max_memory = {} bytes{}", m, from(self.max_memory_from.as_deref())));
        }
        if let Some(n) = self.max_tool_calls {
            said.push(format!("limits.max_tool_calls = {} a minute{}", n, from(self.origin_of("limits.max_tool_calls"))));
        }
        if let Some(t) = &self.run.trust_proxy {
            let nets: Vec<String> = t.iter().map(net_shown).collect();
            said.push(format!("run.trust_proxy = [{}]{}", nets.join(", "), from(self.origin_of("run.trust_proxy"))));
        }
        for (k, f) in &self.run.off {
            said.push(format!("run.{} = false ({}){}", k, switch_options(k).join(", "), from(Some(f))));
        }
        if self.run.check {
            said.push(format!("run.check = true{}", from(self.origin_of("run.check"))));
        }
        for (k, n) in &self.run.ceilings {
            let n = if *k == "drain" { format!("{}ms", n) } else { n.to_string() };
            said.push(format!("run.{} = {}{}", k, n, from(self.origin_of(&format!("run.{}", k)))));
        }
        if let Some(m) = self.run.socket_mode {
            said.push(format!("run.socket_mode = {:o}{}", m, from(self.origin_of("run.socket_mode"))));
        }
        for (k, v) in &self.run.forced {
            said.push(format!("run.{} = \"{}\"{}", k, v, from(self.origin_of(&format!("run.{}", k)))));
        }
        if self.require_path {
            said.push(format!("require_path = true{}", from(self.files.first().map(PathBuf::as_path))));
        }
        if !self.files.is_empty() {
            out.push(if said.is_empty() { "in force: nothing (the layers say nothing that narrows)".to_string() } else { "in force:".to_string() });
            out.extend(said.into_iter().map(|l| format!("  {}", l)));
        }
        out.join("\n")
    }

    /// The value `[env] set` forces for `name`, if it forces one: by the name as the system reads
    /// it, on Windows whatever its case, where `app_env` is `APP_ENV` (FEEDBACK82.md, Q-1280, found
    /// by the coverage tests; branch feedback82-85).
    pub fn forced_env(&self, name: &str) -> Option<&str> {
        self.env_set.iter().find(|(k, _)| if cfg!(windows) { k.eq_ignore_ascii_case(name) } else { k == name }).map(|(_, v)| v.as_str())
    }

    /// Whether `[env] set` forces `name`: no `.env` replaces it.
    pub fn forces_env(&self, name: &str) -> bool {
        self.forced_env(name).is_some()
    }

    /// `[env] set`'s values, put in the environment before the program's `.env` is read, which
    /// does not change what is there already: what env() gives, whatever `.env` or the shell said
    /// (FEEDBACK82.md, Q-1280; branch feedback82-85).
    pub fn put_env(&self) {
        for (k, v) in &self.env_set {
            std::env::set_var(k, v);
        }
    }

    /// The limits a request is held to, in force where they are less than the options' (or those
    /// are not given): `max_compute`, `max_memory` and `max_tool_calls`, as `quillo run` holds
    /// them, for a phone app too (FEEDBACK82.md, Q-1284; branch feedback82-85).
    pub fn hold_limits(&self) {
        if let Some(n) = self.max_tool_calls {
            crate::mcp::MAX_TOOL_CALLS.fetch_min(n, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(c) = self.max_compute {
            let (now, c) = (crate::httpd::compute_limit_ms(), c.as_millis().max(1) as u64);
            if now == 0 || now > c {
                crate::httpd::limit_compute(c);
                // Its message names the file (FEEDBACK55.md, Q-849; branch feedback55).
                if let Some(f) = &self.max_compute_from {
                    crate::httpd::compute_limit_from(f);
                }
            }
        }
        // It narrows, as the other limits do (FEEDBACK71.md, Q-977; branch security).
        if let Some(m) = self.max_memory {
            let now = crate::httpd::memory_limit();
            if now == 0 || now > m as u64 {
                crate::httpd::limit_memory(m as u64);
                if let Some(f) = &self.max_memory_from {
                    crate::httpd::memory_limit_from(f);
                }
            }
        }
    }
}

/// Each plugin of a catalog's index (`plugins/index.json`, format 1, as `quillo install` reads it)
/// as `name@sha256:hex`; why not, naming the file.
fn catalog_pins(index: &Path) -> Result<Vec<String>, String> {
    let text = crate::error::read_text(index).map_err(|e| format!("the catalog's index {}: {}", index.display(), e))?;
    let j: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("the catalog's index {} is not JSON: {}", index.display(), e))?;
    if j["format"].as_u64() != Some(1) {
        return Err(format!("the catalog's index {} is not one this quillo reads (format 1)", index.display()));
    }
    let mut pins = Vec::new();
    for p in j["plugins"].as_array().into_iter().flatten() {
        let (Some(name), Some(h)) = (p["name"].as_str(), p["sha256"].as_str()) else { continue };
        if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty() {
            pins.push(format!("{}@sha256:{}", name, h.to_ascii_lowercase()));
        }
    }
    Ok(pins)
}

/// `*` any host, `*.example.com` its subdomains (not itself), else the host itself.
fn host_matches(pattern: &str, host: &str) -> bool {
    let p = pattern.trim().to_ascii_lowercase();
    match p.strip_prefix("*.") {
        _ if p == "*" => true,
        Some(base) => host.ends_with(&format!(".{}", base)),
        None => p == host,
    }
}

/// Loopback, private, link-local (the cloud's metadata at 169.254.169.254), unspecified,
/// carrier-grade NAT, and IPv6's unique-local and link-local, with IPv4 in IPv6 judged as IPv4.
pub fn private_ip(ip: IpAddr) -> bool {
    // What `http`'s `public_only` refuses (NAT64, 6to4 and IPv4-compatible addresses too, and
    // IPv4-translated and ISATAP ones and the rest of 0::/8: FEEDBACK75.md, Q-1093; branch
    // feedback75).
    if !crate::fetch::public_ip(ip) {
        return true;
    }
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || (o[0] == 100 && (64..128).contains(&o[1])) || o[0] == 0
        }
        IpAddr::V6(v) => match v.to_ipv4_mapped() {
            Some(v4) => private_ip(IpAddr::V4(v4)),
            None => {
                let s = v.segments();
                v.is_loopback() || v.is_unspecified() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(layers: &[&str]) -> Result<Policy, String> {
        let mut p = Policy::default();
        for (i, t) in layers.iter().enumerate() {
            p.add(Path::new(&format!("layer{}", i)), t)?;
        }
        Ok(p)
    }

    #[test]
    fn a_layer_only_narrows() {
        let p = policy(&[
            "[effects]\nallow = [\"db\", \"http\", \"fs\"]\n[plugins]\nallow = [\"csv@1\", \"qr\"]\n[http]\nhosts = [\"*\"]\n",
            "[effects]\nallow = [\"db\", \"env\"] # env: not given by the first\n[plugins]\nallow = [\"csv@1.4\", \"pdf\"]\nbuild = false\n[http]\nhosts = [\"api.stripe.com\", \"*.mapbox.com\"]\n",
        ])
        .unwrap();
        assert_eq!(p.effects, crate::ast::effect_bit("db"));
        assert_eq!(p.effects_source(), "layer0");
        assert!(!p.build);
        assert!(p.plugin("csv", Some("1.4.2"), &[]).is_ok());
        assert!(p.plugin("csv", Some("1.5.0"), &[]).unwrap_err().contains("layer1"));
        assert!(p.plugin("csv", Some("10.4"), &[]).is_err(), "1 is not 10");
        assert!(p.plugin("pdf", None, &[]).unwrap_err().contains("not in the plugins layer0 allows: csv@1, qr"));
        assert!(p.plugin("qr", None, &[]).unwrap_err().contains("layer1"));
        assert!(p.http("https://api.stripe.com/v1").is_ok());
        assert!(p.http("https://tiles.mapbox.com/x").is_ok());
        assert!(p.http("https://mapbox.com/x").unwrap_err().contains("not among the hosts layer1 allows"));
        assert!(p.http("not a url").is_ok());
    }

    #[test]
    fn private_addresses_and_names() {
        let p = policy(&["[http]\nprivate = false\n[env]\nnames = [\"A\", \"B\"]\n", "[env]\nnames = [\"B\"]\n"]).unwrap();
        for u in ["http://127.0.0.1:8080/", "http://169.254.169.254/latest/meta-data", "http://10.0.0.5/", "http://[::1]/", "http://[::ffff:192.168.1.1]/", "http://localhost/", "http://100.64.0.1/", "http://[fd00::1]/", "http://0.0.0.0/"] {
            assert!(p.http(u).unwrap_err().contains("an address of this machine or its network"), "{}", u);
        }
        assert!(p.http("http://93.184.216.34/").is_ok());
        assert!(p.env_name("B").is_ok());
        assert!(p.env_name("A").unwrap_err().contains("layer1"));
        assert!(p.env_name("PATH").unwrap_err().contains("layer0"));
        assert!(Policy::default().env_name("PATH").is_ok());
    }

    #[test]
    fn limits_take_the_least() {
        let p = policy(&["[limits]\nworkers = 8\nmax_compute = \"10s\"\nmax_body = \"10MB\"\n", "[limits]\nworkers = 16\nmax_compute = \"500ms\"\nmax_body = 1_000\n"]).unwrap();
        assert_eq!((p.workers, p.max_compute, p.max_body), (Some(8), Some(Duration::from_millis(500)), Some(1000)));
        assert_eq!(bytes(&V::Str("512kb".into())), Some(512 << 10));
        assert_eq!(bytes(&V::Str("2GB".into())), Some(2 << 30));
        assert_eq!(duration("2m"), Some(Duration::from_secs(120)));
        assert_eq!(duration("5"), Some(Duration::from_secs(5)));
        assert_eq!(duration("5h"), None);
        assert_eq!(size("50MB"), Some(50 << 20));
        assert_eq!(size("50mb"), Some(50 << 20));
        assert_eq!(size(" 2 GB "), Some(2 << 30));
        assert_eq!(size("512KB"), Some(512 << 10));
        assert_eq!(size("100B"), Some(100));
        assert_eq!(size("52428800"), Some(52428800));
        assert_eq!(size("1.5GB"), None);
        assert_eq!(size("lots"), None);
        assert_eq!(size(&format!("{}GB", usize::MAX)), None, "too large is no size, not a wrapped one");
    }

    #[test]
    fn what_a_policy_does_not_say_is_refused() {
        assert!(policy(&["[effects]\nallow = [\"disk\"]\n"]).unwrap_err().contains("layer0:2: `disk` is not an effect"));
        assert!(policy(&["[plugins]\nsigned = true\n"]).unwrap_err().contains("`plugins.signed` is not something a policy says"));
        assert!(policy(&["[plugins]\nbuild = \"no\"\n"]).unwrap_err().contains("wrong kind of value"));
        assert!(policy(&["[plugins]\nallow = \"csv\"\n"]).unwrap_err().contains("is a list"));
        assert!(policy(&["just words\n"]).unwrap_err().contains("expected `key = value`"));
        assert!(policy(&["[env]\nnames = [\"A\",\n"]).unwrap_err().contains("a list with no `]`"));
        assert!(policy(&["[limits]\nmax_compute = \"soon\"\n"]).unwrap_err().contains("not a time"));
        // A list over lines, comments, and a `#` inside text.
        let p = policy(&["# the host's\n[env]\nnames = [\n  \"A#1\", # first\n  \"B\",\n]\n"]).unwrap();
        assert_eq!(p.env[0].1, ["A#1", "B"]);
        assert!(policy(&["[limits]\nworkers = 0\n"]).is_err());
    }
}
