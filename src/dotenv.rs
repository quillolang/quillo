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
/// this one.
const FROM_FILE_VAR: &str = "QUILLO_ENV_FROM_FILE";

fn from_file() -> &'static Mutex<HashSet<String>> {
    static KEYS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let inherited = std::env::var(FROM_FILE_VAR).unwrap_or_default();
        Mutex::new(inherited.split(',').filter(|k| !k.is_empty()).map(str::to_string).collect())
    })
}

/// Whether `name` came from a `.env` file rather than the environment.
fn set_by_file(name: &str) -> bool {
    from_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(name)
}

/// Reads `path` into the process's environment, leaving the variables the
/// environment set alone. Called at startup, before any other thread runs.
/// A file that is not there is nothing, unless it was named (`required`).
pub fn load_into_process(path: &Path, required: bool) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) if !required => return Ok(()),
        Err(e) => return Err(format!("cannot read {}: {}", path.display(), e)),
    };
    let mut keys = from_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (pairs, skipped) = parse_noting(&text);
    for n in skipped {
        eprintln!("quillo: {}:{}: not KEY=value, skipped", path.display(), n);
    }
    for (k, v) in pairs {
        if std::env::var_os(&k).is_some() && !keys.contains(&k) {
            continue;
        }
        std::env::set_var(&k, v);
        keys.insert(k);
    }
    let mut list: Vec<&str> = keys.iter().map(String::as_str).collect();
    list.sort();
    std::env::set_var(FROM_FILE_VAR, list.join(","));
    Ok(())
}

/// A service's own `.env` in this process: its values, by name.
pub fn read(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path).map(|t| parse(&t).into_iter().collect()).unwrap_or_default()
}

/// `env(name)` for a program whose own file holds `own`: the environment's
/// value, unless a file gave it and `own` has one.
pub fn var(own: &HashMap<String, String>, name: &str) -> Option<String> {
    let real = std::env::var(name).ok().filter(|_| name != FROM_FILE_VAR);
    match (real, own.get(name)) {
        (Some(v), Some(mine)) => Some(if set_by_file(name) { mine.clone() } else { v }),
        (Some(v), None) => Some(v),
        (None, mine) => mine.cloned(),
    }
}

/// `KEY=value` lines: blank lines and `#` comments skipped, `export `
/// allowed, values bare (to a ` #`), in double quotes (with `\n`, `\"` and
/// `\\`) or in single quotes (as written).
pub fn parse(text: &str) -> Vec<(String, String)> {
    parse_noting(text).0
}

/// `parse`, and the lines it skipped, by number. A byte-order mark is no part of the first key,
/// and a quoted value goes on to its closing quote, over several lines, as a PEM key is kept in
/// Node's `dotenv` and Docker Compose (FEEDBACK31.md, Q-380; branch feedback27-35-fixes).
pub fn parse_noting(text: &str) -> (Vec<(String, String)>, Vec<usize>) {
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
            skipped.push(at);
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            skipped.push(at);
            continue;
        }
        let v = v.trim_start();
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
                        skipped.push(at);
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
                        s.push_str(rest);
                        skipped.push(at);
                        break;
                    }
                }
            }
            s
        } else {
            v.split(" #").next().unwrap_or("").trim_end().to_string()
        };
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
