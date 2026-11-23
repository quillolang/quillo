//! `.env` files (FEEDBACK9.md, Q-071; branch feedback9-fixes).
//!
//! `quillo run` reads the `.env` beside the program (or `--env-file path`)
//! into its environment before anything reads it: `env("API_KEY")`, `PORT`,
//! `QUILLO_SECRET`. A variable the environment already has wins, so a stray
//! file cannot override what production sets.
//!
//! A service reads the `.env` beside its own file too, in one process and
//! under `--split` alike: its file's values come before the app's file's,
//! and after the real environment's. The keys a file set are passed on to
//! the processes `--split` starts (`QUILLO_ENV_FROM_FILE`), so each of them
//! knows which of its variables a file gave and which the environment did.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// Where the keys a file set are passed on to the processes started from
/// this one: Quillo's, the services `--split` starts. Not given to `--with`'s commands, which are
/// no Quillo service (`with::start` in src/main.rs; FEEDBACK82.md, Q-1281, found by the review of
/// bf40063; branch feedback82-85).
pub const FROM_FILE_VAR: &str = "QUILLO_ENV_FROM_FILE";

/// The names a file set in this process, and those its app's file set, as `take_marker` takes them
/// from the app. None is taken from the environment unasked: `QUILLO_ENV_FROM_FILE` was believed in
/// any process, so a start whose environment named `QUILLO_POLICY_PATH` there ran without the
/// provider's layer it names, and a `.env` replaced what the host's `[env] set` forces
/// (FEEDBACK82.md, Q-1281 and Q-1280, found by the coverage tests; branch feedback82-85).
fn from_file() -> &'static Mutex<HashSet<String>> {
    static KEYS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    KEYS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The names the app's `.env` gave, passed on in `QUILLO_ENV_FROM_FILE`, believed only where
/// `from_app`: the process was given `--as-service` and its environment had
/// `QUILLO_STOP_WITH_APP` (`stop_with_app` in src/main.rs), as `--split` starts a service; there
/// the service's own `.env` comes before them, which is what the variable is for. That is what is
/// checked, not who started the process: any starter that gives both is believed. It is enough,
/// as believing the marker gives that starter nothing it lacks: the marker only lets the
/// service's `.env` replace a variable the environment set, and whoever sets the environment can
/// leave that variable out, or set it to the file's value, directly. What a starter may not do
/// is held apart, whatever the marker says: a name that chooses the policy is never taken from
/// it, and one the host's `[env] set` forces no file replaces (`set_by_file`). Elsewhere the
/// marker is no one's word but whoever set the environment's, and is taken out, so that it
/// reaches no process started from this one (FEEDBACK82.md, Q-1281, found by the coverage tests,
/// and the review of bf40063, which found "a service its app started" said more than is checked;
/// branch feedback82-85). Called once, at startup, before the program's `.env` is read.
pub fn take_marker(from_app: bool) {
    let inherited = std::env::var(FROM_FILE_VAR).unwrap_or_default();
    let mut keys = from_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if from_app {
        keys.extend(inherited.split(',').filter(|k| !k.is_empty() && !chooses_policy(k)).map(str::to_string));
    }
    if keys.is_empty() {
        std::env::remove_var(FROM_FILE_VAR);
    } else {
        let mut list: Vec<&str> = keys.iter().map(String::as_str).collect();
        list.sort();
        std::env::set_var(FROM_FILE_VAR, list.join(","));
    }
}

/// Whether `name` chooses the policy (`QUILLO_POLICY_PATH` and the like), however it is written.
fn chooses_policy(name: &str) -> bool {
    name.to_ascii_uppercase().starts_with("QUILLO_POLICY")
}

/// Whether `name` came from a `.env` file rather than the environment. Never one the policy's
/// `[env] set` forces, which is the host's whatever a file or the marker says (FEEDBACK82.md,
/// Q-1280, found by the coverage tests; branch feedback82-85). The policy is set before any of
/// this is asked, and `policy::get` only reads it.
pub(crate) fn set_by_file(name: &str) -> bool {
    !crate::policy::get().forces_env(name) && from_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(name)
}

/// Reads `path` into the process's environment, leaving the variables the
/// environment set alone. Called at startup, before any other thread runs.
/// A file that is not there is nothing, unless it was named (`required`).
pub fn load_into_process(path: &Path, required: bool) -> Result<(), String> {
    let Some(text) = read_file(path, required, true)? else { return Ok(()) };
    let mut keys = from_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (pairs, skipped) = parse_noting(&text);
    for (n, why) in skipped {
        eprintln!("quillo: {}:{}: {}, skipped", path.display(), n, why);
    }
    let policy = crate::policy::get();
    let host = policy.has_host_layer();
    for (k, v) in pairs {
        if std::env::var_os(&k).is_some() && !keys.contains(&k) {
            continue;
        }
        // What the host's `[env] set` forces is not replaced, whatever the names a file set say:
        // with `QUILLO_ENV_FROM_FILE=APP_ENV` passed in, `.env`'s `APP_ENV=dev` beat the host's
        // "production" (FEEDBACK82.md, Q-1280, found by the coverage tests; branch feedback82-85).
        if policy.forces_env(&k) {
            continue;
        }
        // A name that steers Quillo or the processes it starts (`--split`'s services, cargo,
        // `--with`'s commands) is not the program's file's to set: `QUILLO_POLICY_PATH` in `.env`
        // made a service read the developer's file as a host's layer, and `LD_PRELOAD` or `CARGO`
        // reached every process started from this one. Under a host's policy the start stops,
        // naming it; without one, only the policy's names are left out, said (FEEDBACK82.md,
        // Q-1281, found in review; branch feedback82-85).
        if steers(&k) {
            if host {
                return Err(format!(
                    "{} sets {}, which steers Quillo or the processes it starts and is not a program's setting under the host's policy: set it where the program is started, or take it out of {}",
                    path.display(),
                    k,
                    path.display()
                ));
            }
            if chooses_policy(&k) {
                eprintln!("quillo: {} sets {}, which only whoever starts Quillo may set: not read", path.display(), k);
                continue;
            }
        }
        std::env::set_var(&k, v);
        keys.insert(k);
    }
    let mut list: Vec<&str> = keys.iter().map(String::as_str).collect();
    list.sort();
    std::env::set_var(FROM_FILE_VAR, list.join(","));
    Ok(())
}

/// The text of a `.env`, or none when there is no such file and it was not named. Only a file that
/// is not there (or a folder of that name, as a Python virtual environment called `.env` is) is
/// nothing: one that is not UTF-8 text, or cannot be read, is said and stops the start, as with
/// `--env-file`, rather than every `env()` being null without a word (FEEDBACK55.md, Q-835; branch
/// feedback55).
/// One that is there and cannot be read says the ways out: as a program's, `--env-file` too
/// (`program`); a link that leads nowhere says where it leads.
fn read_file(path: &Path, required: bool, program: bool) -> Result<Option<String>, String> {
    if !required && (path.is_dir() || std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)) {
        return Ok(None);
    }
    let way_out = |e: std::io::ErrorKind| match (required, program) {
        (true, _) => String::new(),
        (false, true) if e == std::io::ErrorKind::PermissionDenied => format!("; make it readable, or, to start without it, remove it or give `--env-file {}`", NO_FILE),
        (false, true) => format!("; to start without it, remove it or give `--env-file {}`", NO_FILE),
        (false, false) if e == std::io::ErrorKind::PermissionDenied => "; make it readable, or remove it to start without it".into(),
        (false, false) => "; remove it to start without it".into(),
    };
    if let (Ok(to), Err(e)) = (std::fs::read_link(path), std::fs::metadata(path)) {
        let why = if e.kind() == std::io::ErrorKind::NotFound { "which is not there".to_string() } else { format!("which cannot be read ({})", e) };
        return Err(format!("cannot read {}: it is a link to {}, {}{}", path.display(), to.display(), why, way_out(e.kind())));
    }
    crate::error::read_source_whole(path).map(Some).map_err(|e| format!("cannot read {}: {}{}", path.display(), e, way_out(e.kind())))
}

/// Whether `.env` may not set `name` under a host's policy: what chooses the policy, the internal
/// markers one Quillo gives the processes it starts, what the system's loader reads, and what
/// steers cargo, rustc and where plugins are built (FEEDBACK82.md, Q-1281, found in review; branch
/// feedback82-85).
pub fn steers(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["QUILLO_POLICY", "QUILLO_HOST", "LD_", "DYLD_", "CARGO", "RUSTC", "RUSTUP_"].iter().any(|p| n.starts_with(p))
        || ["RUSTFLAGS", "RUSTDOCFLAGS", "QUILLO_NATIVE_CACHE", "XDG_CACHE_HOME", "QUILLO_STOP_WITH_APP", "QUILLO_PROGRAM_ROOT", FROM_FILE_VAR, "QUILLO_APP_URL", "HOME", "TMPDIR", "TMP", "TEMP", "PATH", "USER"].contains(&n.as_str())
}

/// A file that is always there and empty, for `--env-file`: no settings.
const NO_FILE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// A service's own `.env` in this process: its values, by name; the lines skipped said as the
/// program's are, and one that cannot be read an error (FEEDBACK55.md, Q-835; branch feedback55).
pub fn read(path: &Path) -> Result<HashMap<String, String>, String> {
    let Some(text) = read_file(path, false, false)? else { return Ok(HashMap::new()) };
    let (pairs, skipped) = parse_noting(&text);
    for (n, why) in skipped {
        eprintln!("quillo: {}:{}: {}, skipped", path.display(), n, why);
    }
    Ok(pairs.into_iter().collect())
}

/// `env(name)` for a program whose own file holds `own`: the environment's
/// value, unless a file gave it and `own` has one. What the host's `[env] set` forces is its value
/// whatever a service's own `.env` says (FEEDBACK82.md, Q-1280, found by the coverage tests; branch
/// feedback82-85).
pub fn var(own: &HashMap<String, String>, name: &str) -> Option<String> {
    if let Some(v) = crate::policy::get().forced_env(name) {
        return Some(v.to_string());
    }
    let real = std::env::var(name).ok().filter(|_| name != FROM_FILE_VAR);
    match (real, own.get(name)) {
        (Some(v), Some(mine)) => Some(if set_by_file(name) { mine.clone() } else { v }),
        (Some(v), None) => Some(v),
        (None, mine) => mine.cloned(),
    }
}

/// Why most skipped lines are.
const NOT_A_PAIR: &str = "not KEY=value";

/// `KEY=value` lines: blank lines and `#` comments skipped, `export `
/// allowed, values bare (to a ` #`), in double quotes (with `\n`, `\"` and
/// `\\`) or in single quotes (as written).
pub fn parse(text: &str) -> Vec<(String, String)> {
    parse_noting(text).0
}

/// `parse`, and the lines it skipped, by number, with why. A byte-order mark is no part of the
/// first key, and a quoted value goes on to its closing quote, over several lines, as a PEM key is
/// kept in Node's `dotenv` and Docker Compose (FEEDBACK31.md, Q-380; branch feedback27-35-fixes).
pub fn parse_noting(text: &str) -> (Vec<(String, String)>, Vec<(usize, &'static str)>) {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let at = i + 1;
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            skipped.push((at, NOT_A_PAIR));
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            skipped.push((at, NOT_A_PAIR));
            continue;
        }
        let spaced = v.starts_with(char::is_whitespace);
        let v = v.trim_start();
        let mut unclosed = false;
        let value = if let Some(first) = v.strip_prefix('"') {
            let mut s = String::new();
            let mut rest = first.to_string();
            loop {
                let mut chars = rest.chars();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => {}
                        },
                        c => s.push(c),
                    }
                }
                if closed || i >= lines.len() {
                    if !closed {
                        unclosed = true;
                    }
                    break;
                }
                s.push('\n');
                rest = lines[i].trim_end_matches('\r').to_string();
                i += 1;
            }
            s
        } else if let Some(first) = v.strip_prefix('\'') {
            let mut s = String::new();
            let mut rest = first;
            loop {
                match rest.split_once('\'') {
                    Some((before, _)) => {
                        s.push_str(before);
                        break;
                    }
                    None if i < lines.len() => {
                        s.push_str(rest);
                        s.push('\n');
                        rest = lines[i];
                        i += 1;
                    }
                    None => {
                        unclosed = true;
                        break;
                    }
                }
            }
            s
        } else if spaced && v.starts_with('#') {
            // `KEY= # note`: a note after the spaces is no value, as ` #` ends a bare one;
            // `KEY=#fff` is still "#fff" (FEEDBACK55.md, Q-838; branch feedback55).
            String::new()
        } else {
            v.split(" #").next().unwrap_or("").trim_end().to_string()
        };
        // A quote never closed is skipped as said, and the lines after it read as their own, not
        // taken into its value (FEEDBACK55.md, Q-839; branch feedback55).
        if unclosed {
            skipped.push((at, "its quote is not closed"));
            i = at;
            continue;
        }
        // The environment cannot hold a NUL, and setting one panicked (FEEDBACK55.md, Q-840;
        // branch feedback55).
        if value.contains('\0') {
            skipped.push((at, "a value cannot hold a NUL byte"));
            continue;
        }
        out.push((k.to_string(), value));
    }
    (out, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_of_a_dotenv_file() {
        let got = parse("# settings\nAPI_KEY=abc123\nexport GREETING=\"hello there\\n\"\nRAW='a $b'\nPLAIN=x y # note\nbad line\n=nokey\n");
        assert_eq!(
            got,
            vec![
                ("API_KEY".into(), "abc123".into()),
                ("GREETING".into(), "hello there\n".into()),
                ("RAW".into(), "a $b".into()),
                ("PLAIN".into(), "x y".into()),
            ]
        );
    }
}
