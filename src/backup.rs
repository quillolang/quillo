//! `db.backup(path)`: a copy of a SQLite database, made while it is in use.
//!
//! ```text
//! db.backup("backups/app.db")                              // { path, bytes, ms }
//! db.backup("backups/app-{time}.db", { keep: 7 })          // the seven newest kept
//! schedule("@daily", () => db.backup("backups/app-{time}.db", { keep: 14 }))
//! ```
//!
//! It uses SQLite's online backup API, from a connection of its own: in
//! WAL mode that is a read, which sees the database as it was when the
//! copy began and holds up no one's writes. The copy is written next to
//! where it goes and renamed there once whole, so a backup that is there
//! is complete. `{time}` in the path is the UTC time (`20261027T030000Z`),
//! `{tenant}` the tenant's name; with `keep`, older files made by the same
//! pattern are removed, keeping that many. A relative path is in the
//! program's folder.
//!
//! PostgreSQL and MySQL have tools of their own for this (`pg_dump`,
//! `mysqldump`, or the hosting service's snapshots): see the user guide.

use crate::db::Backend;
use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::path::{Path, PathBuf};

type R = Result<Value, Ctrl>;

fn err(e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("db.backup: {}", e)))
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, m, d) = crate::httpd::civil((secs / 86400) as i64);
    let s = secs % 86400;
    format!("{}{:02}{:02}T{:02}{:02}{:02}Z", y, m, d, s / 3600, s % 3600 / 60, s % 60)
}

/// Copies the database `src` (a file, or `None` for one in memory, copied
/// from `conn`) to `to`.
fn copy(src: Option<&Path>, conn: Option<&rusqlite::Connection>, to: &Path) -> Result<(), String> {
    let part = to.with_extension("part");
    let _ = std::fs::remove_file(&part);
    let mut dst = rusqlite::Connection::open(&part).map_err(|e| e.to_string())?;
    let run = |from: &rusqlite::Connection, dst: &mut rusqlite::Connection| -> Result<(), String> {
        use rusqlite::backup::StepResult;
        let b = rusqlite::backup::Backup::new(from, dst).map_err(|e| e.to_string())?;
        // All pages in one step: one read, so a consistent copy, which
        // writers in WAL mode do not wait for.
        for _ in 0..100 {
            match b.step(-1).map_err(|e| e.to_string())? {
                StepResult::Done => return Ok(()),
                StepResult::More => {}
                _ => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        Err("the database stayed busy".into())
    };
    let r = match (src, conn) {
        (Some(p), _) => {
            let from = rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
                .map_err(|e| e.to_string())?;
            run(&from, &mut dst)
        }
        (None, Some(c)) => run(c, &mut dst),
        (None, None) => Err("nothing to copy".into()),
    };
    // A copy that is one file: no WAL beside it.
    let r = r.and_then(|_| dst.execute_batch("pragma journal_mode = delete").map_err(|e| e.to_string()));
    drop(dst);
    match r {
        Ok(()) => std::fs::rename(&part, to).map_err(|e| e.to_string()),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Removes the oldest files `pattern` (with `{time}` in it) made, keeping
/// the `keep` newest.
fn prune(pattern: &Path, keep: usize) -> Result<usize, String> {
    let (Some(dir), Some(name)) = (pattern.parent(), pattern.file_name().and_then(|n| n.to_str())) else { return Ok(0) };
    let Some((before, after)) = name.split_once("{time}") else { return Ok(0) };
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| {
            n.len() == before.len() + 16 + after.len()
                && n.starts_with(before)
                && n.ends_with(after)
                && n[before.len()..before.len() + 16].bytes().enumerate().all(|(i, b)| match i {
                    8 => b == b'T',
                    15 => b == b'Z',
                    _ => b.is_ascii_digit(),
                })
        })
        .collect();
    found.sort();
    let gone = found.len().saturating_sub(keep);
    for n in &found[..gone] {
        std::fs::remove_file(dir.join(n)).map_err(|e| e.to_string())?;
    }
    Ok(gone)
}

/// `db.backup(path, { keep })`: `{ path, bytes, ms, removed }`, or an
/// error value.
pub fn backup(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let pattern = match args.first() {
        Some(Value::Str(s)) if !s.is_empty() => s.to_string(),
        other => return fail(line, format!("`db.backup` expects the path of the copy, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let keep = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(Value::Map(m)) => match m.get("keep") {
            None | Some(Value::Nil) => None,
            Some(Value::Int(n)) if *n >= 1 => Some(*n as usize),
            Some(v) => return fail(line, format!("`db.backup`'s keep is how many copies to keep (1 or more), got {}", v.repr())),
        },
        Some(v) => return fail(line, format!("`db.backup` expects options as a map, got {}", v.type_name())),
    };
    if keep.is_some() && !pattern.contains("{time}") {
        return fail(line, "`db.backup` keeps copies by the time in their names: put {time} in the path");
    }
    let tenant = it.tenant();
    let filled = pattern.replace("{tenant}", tenant.as_deref().unwrap_or(""));
    let full = |p: &str| if Path::new(p).is_absolute() { PathBuf::from(p) } else { it.base_dir.join(p) };
    let to = full(&filled.replace("{time}", &stamp()));
    let pattern_path = full(&filled);
    let b = match crate::db::backend_on(it, on) {
        Ok(b) => b,
        Err(e) => return Ok(err(e)),
    };
    let Backend::Sqlite(m) = &*b else {
        return Ok(err("only SQLite databases are copied this way; use pg_dump or mysqldump (see the user guide)"));
    };
    if let Some(dir) = to.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return Ok(err(format!("{}: {}", dir.display(), e)));
        }
    }
    let started = std::time::Instant::now();
    // The database's file, read from a connection of its own off the
    // interpreter lock; one in memory is copied from its connection.
    let file = {
        let c = m.lock().unwrap_or_else(|e| e.into_inner());
        c.path().filter(|p| !p.is_empty()).map(PathBuf::from)
    };
    let r = match file {
        Some(src) => {
            let dst = to.clone();
            it.unlocked(move || copy(Some(&src), None, &dst))
        }
        None => copy(None, Some(&m.lock().unwrap_or_else(|e| e.into_inner())), &to),
    };
    if let Err(e) = r {
        return Ok(err(e));
    }
    let removed = match keep {
        Some(k) => match prune(&pattern_path, k) {
            Ok(n) => n,
            Err(e) => return Ok(err(format!("made {}, but could not remove older copies: {}", to.display(), e))),
        },
        None => 0,
    };
    let mut out = Map::new();
    out.insert("path".into(), Value::str(to.display().to_string()));
    out.insert("bytes".into(), Value::Int(std::fs::metadata(&to).map_or(0, |m| m.len() as i64)));
    out.insert("ms".into(), Value::Float((started.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0));
    out.insert("removed".into(), Value::Int(removed as i64));
    Ok(Value::map(out))
}
