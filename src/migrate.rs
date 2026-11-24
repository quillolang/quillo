//! `db.migrate("migrations")`: brings a database's schema up to date from a
//! folder of SQL files, applied in name order (`001_todos.sql`,
//! `002_tags.sql`, ...). Each file runs once, in a transaction of its own,
//! and is recorded in `_quillo_migrations` with its SHA-256; a recorded file
//! whose text has since changed gets a warning, not a second run. Servers
//! starting together take turns: Postgres through an advisory lock, SQLite
//! through an immediate transaction.

use crate::db::Backend;
use crate::pg;
use crate::value::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

/// What judges each file a migration runs, by its path as listed, where it really is and its text,
/// as it is read (`run`): db.rs's, which holds a taken file's migration as it holds its statements.
pub type Judge<'a> = &'a (dyn Fn(&Path, &Path, &str) -> Result<(), String> + Sync);

/// The files to apply, in order: name and text, each judged (`judge`) by the text read, which is the
/// text that runs: judged by a first reading of its own, a file could be another by the time it ran
/// (FEEDBACK83.md, Q-1300, found by the coverage tests of 0038a97; branch feedback82-85).
fn files(dir: &Path, judge: Judge<'_>) -> Result<Vec<(String, String)>, String> {
    // A folder that is not there may be one git did not keep, as it keeps no empty folder: said,
    // with what to do (FEEDBACK34.md, Q-423; branch feedback27-35-fixes).
    let entries = std::fs::read_dir(dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!(
            "cannot read {}: there is no such folder (git keeps no empty folder: put the first migration in it, or a .gitkeep, which is not read)",
            dir.display()
        ),
        _ => format!("cannot read {}: {}", dir.display(), e),
    })?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().map_or(false, |x| x == "sql") && path.is_file() {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let text = read_judged(&path, judge)?;
            // A file with no statement yet (an editor's new file, made empty before it is
            // written) waits: applied empty, it was recorded, and the SQL then written in it
            // never ran, under --watch at the first save (FEEDBACK62.md, Q-919, found in review).
            if crate::sql::scan(crate::sql::Dialect::Sqlite, &text).stmts.is_empty() {
                eprintln!("quillo: warning: migration {} has no statement yet: it is applied once it has one", name);
                continue;
            }
            out.push((name, text));
        }
    }
    out.sort();
    Ok(out)
}

/// The text of the migration at `path`, read where it really is, and judged there with it (`judge`):
/// its last part is read as no link (`O_NOFOLLOW`), and the file opened is the one judged, by the
/// path the system says it opened (`opened_path`): a folder of the path swapped for a link between
/// the two had the file read be another than the one judged (FEEDBACK83.md, Q-1300, found by review
/// 6 of aebe60e; branch feedback82-85).
fn read_judged(path: &Path, judge: Judge<'_>) -> Result<String, String> {
    let cannot = |e: std::io::Error| format!("cannot read {}: {}", path.display(), e);
    let real = std::fs::canonicalize(path).map_err(cannot)?;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = o.open(&real).map_err(cannot)?;
    if let Some(opened) = opened_path(&f) {
        if opened != real {
            return Err(format!("cannot read {}: the file opened, {}, is not the one judged, {} (a folder of its path changed as it was read)", path.display(), opened.display(), real.display()));
        }
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut f, &mut text).map_err(cannot)?;
    judge(path, &real, &text)?;
    Ok(text)
}

/// Where the system says the file `f` is, as it opened it: macOS's `F_GETPATH`, Linux's
/// `/proc/self/fd`. None where it cannot say (another system).
#[cfg(target_os = "macos")]
fn opened_path(f: &std::fs::File) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];
    // SAFETY: the buffer holds PATH_MAX bytes and its end, as F_GETPATH asks; the descriptor is
    // the open file's.
    if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } == -1 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0)?;
    Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..len])))
}

#[cfg(target_os = "linux")]
fn opened_path(f: &std::fs::File) -> Option<std::path::PathBuf> {
    use std::os::unix::io::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd())).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn opened_path(_: &std::fs::File) -> Option<std::path::PathBuf> {
    None
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect()
}

const TABLE: &str = "create table if not exists _quillo_migrations (name text primary key, sha256 text not null, applied_at text not null)";

/// The folders `db.migrate` has read in this process, for `--watch` to watch: the default
/// database's, a tenant's, a shard's, and one a load that failed read (FEEDBACK62.md, Q-919,
/// found in review). At most 64.
static FOLDERS: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn folders() -> Vec<std::path::PathBuf> {
    FOLDERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// Applies what `dir` has that the database has not; returns the names
/// applied. Stops at the first file that fails, which is rolled back. Each file is judged as it is
/// read (`judge`), all of them before any runs.
pub fn run(db: &Backend, dir: &Path, judge: Judge<'_>) -> Result<Vec<String>, String> {
    {
        let mut f = FOLDERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if f.len() < 64 && !f.iter().any(|d| d == dir) {
            f.push(dir.to_path_buf());
        }
    }
    let todo = files(dir, judge)?;
    match db {
        Backend::Sqlite(m) => {
            let c = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // Quillo's own table, written as its own where a taken file's statements may not write
            // one (db.rs `OwnSql`; FEEDBACK83.md, Q-1300, found by the review of bc460fd; branch
            // feedback82-85).
            {
                let _own = crate::db::OwnSql::enter();
                c.execute_batch(TABLE).map_err(|e| e.to_string())?;
            }
            let mut applied = Vec::new();
            for (name, text) in &todo {
                // SQLite takes `pragma foreign_keys` only outside a transaction, and each file
                // runs in one: a file that turns them off to rebuild a table, as SQLite's own
                // steps do, had them on, and dropping the old table ran its children's `on
                // delete` actions, deleting or blanking their rows while `db.migrate` said it
                // worked. Such a file has them off for its run, its foreign keys checked before
                // it commits, and on again after (FEEDBACK62.md, Q-915; branch cache-and-migrate).
                let says = sqlite_says(text);
                let off = says.keys_off && c.query_row("pragma foreign_keys", [], |r| r.get::<_, i64>(0)).map_or(false, |v| v == 1);
                if off {
                    c.execute_batch("pragma foreign_keys = off").map_err(|e| e.to_string())?;
                }
                let r = sqlite_one(&c, name, text, &says, off);
                if off {
                    let _ = c.execute_batch("pragma foreign_keys = on");
                }
                if r? {
                    applied.push(name.clone());
                }
            }
            Ok(applied)
        }
        Backend::Postgres(pool) => {
            let mut c = pool.checkout()?;
            let r = pg_run(&mut c, &todo);
            // What a migration set in the session (`set search_path`) is not the next request's
            // (FEEDBACK55.md, Q-708; branch feedback55).
            if c.batch(pg::RESET).is_ok() {
                pool.give_back(c);
            } else {
                pool.discard(c);
            }
            r
        }
        #[cfg(feature = "mysql")]
        Backend::Mysql(pool) => {
            let mut c = pool.checkout()?;
            let r = my_run(&mut c, &todo);
            pool.give_back(c);
            r
        }
    }
}

/// One file on SQLite, in a transaction of its own: whether it was applied (false when it
/// was already). `off` is that foreign keys were turned off for it.
fn sqlite_one(c: &rusqlite::Connection, name: &str, text: &str, says: &SqliteSays, off: bool) -> Result<bool, String> {
    let hash = digest(text);
    c.execute_batch("begin immediate").map_err(|e| e.to_string())?;
    // Quillo's own table, read as its own: the file's code may not read it (db.rs `authorize`), and
    // a taken file's second `c.migrate()` had found nothing and applied its files again
    // (FEEDBACK83.md, Q-1300, found by tests/cover8285_confirm.rs; branch feedback82-85).
    let had: Option<String> = {
        let _own = crate::db::OwnSql::enter();
        c.query_row("select sha256 from _quillo_migrations where name = ?1", [name], |r| r.get(0)).ok()
    };
    if let Some(had) = had {
        let _ = c.execute_batch("commit");
        if had != hash {
            eprintln!("quillo: warning: migration {} changed after it was applied; it is not run again", name);
        }
        return Ok(false);
    }
    // A migration is where tables are made (FEEDBACK50.md, Q-628; found in review).
    crate::db::warn_lossy_columns(text);
    if !off {
        warn_dropped_parents(c, name, &says.drops);
    }
    // Rows already pointing at nothing (made with foreign keys off, by the `sqlite3` shell say)
    // are not this file's doing: only those it leaves are refused (found in review).
    let before = if off { violations(c).map_err(|e| format!("migration {} failed: {}", name, e)) } else { Ok(Vec::new()) };
    let before = match before {
        Ok(b) => b,
        Err(e) => {
            let _ = c.execute_batch("rollback");
            return Err(e);
        }
    };
    let done = c
        .execute_batch(text)
        .and_then(|_| {
            let _own = crate::db::OwnSql::enter();
            c.execute("insert into _quillo_migrations (name, sha256, applied_at) values (?1, ?2, datetime('now'))", [name, &hash])
        })
        .map_err(|e| e.to_string())
        .and_then(|_| if off { foreign_key_check(c, name, &before) } else { Ok(()) });
    match done {
        // A deferred constraint fails at the commit, which leaves the file open
        // (FEEDBACK16.md, Q-143; branch feedback20-fixes).
        Ok(()) => {
            if let Err(e) = c.execute_batch("commit") {
                let _ = c.execute_batch("rollback");
                return Err(format!("migration {} failed: {}", name, e));
            }
            Ok(true)
        }
        Err(e) => {
            let _ = c.execute_batch("rollback");
            Err(format!("migration {} failed: {}", name, e))
        }
    }
}

/// What a SQLite migration's statements say, read by the tokenizer, so that a comment or a
/// string saying it is not taken (FEEDBACK62.md, Q-915; branch cache-and-migrate).
#[derive(Default)]
struct SqliteSays {
    /// A `pragma foreign_keys = off` (or `0`, `false`, `no`, and the `(off)` form).
    keys_off: bool,
    /// The tables it drops, as named (without quotes).
    drops: Vec<String>,
}

fn sqlite_says(text: &str) -> SqliteSays {
    use crate::sql::Kind;
    let (toks, _) = crate::sql::lex(crate::sql::Dialect::Sqlite, text);
    let word = |i: usize| toks.get(i).filter(|t| t.kind == Kind::Word).map(|t| text[t.start..t.end].to_ascii_lowercase());
    let unquoted = |i: usize| {
        toks.get(i).filter(|t| matches!(t.kind, Kind::Word | Kind::Quoted | Kind::Str)).map(|t| {
            let w = &text[t.start..t.end];
            if t.kind == Kind::Word { w.to_string() } else { w[1..w.len().saturating_sub(1).max(1)].to_string() }
        })
    };
    let mut says = SqliteSays::default();
    // Each statement starts after a `;`, or at the text's start.
    let mut starts = vec![0];
    starts.extend(toks.iter().enumerate().filter(|(_, t)| t.kind == Kind::Semi).map(|(i, _)| i + 1));
    for i in starts {
        match word(i).as_deref() {
            Some("pragma") => {
                // `pragma [schema.]foreign_keys = off`, or `pragma foreign_keys(off)`.
                let mut j = i + 1;
                if toks.get(j + 1).is_some_and(|t| t.kind == Kind::Dot) {
                    j += 2;
                }
                if !unquoted(j).is_some_and(|n| n.eq_ignore_ascii_case("foreign_keys")) {
                    continue;
                }
                let v = match toks.get(j + 1).map(|t| (t.kind, &text[t.start..t.end])) {
                    Some((Kind::Op, "=")) | Some((Kind::Open, _)) => toks.get(j + 2),
                    _ => None,
                };
                if let Some(t) = v {
                    let v = text[t.start..t.end].trim_matches(|c| c == '\'' || c == '"').to_ascii_lowercase();
                    if matches!(v.as_str(), "off" | "0" | "false" | "no") {
                        says.keys_off = true;
                    }
                }
            }
            Some("drop") if word(i + 1).as_deref() == Some("table") => {
                let mut j = i + 2;
                if word(j).as_deref() == Some("if") && word(j + 1).as_deref() == Some("exists") {
                    j += 2;
                }
                if toks.get(j + 1).is_some_and(|t| t.kind == Kind::Dot) {
                    j += 2;
                }
                if let Some(t) = unquoted(j) {
                    says.drops.push(t);
                }
            }
            _ => {}
        }
    }
    says
}

/// Dropping a table that others point at with an `on delete` action runs it for each row,
/// foreign keys being on: said before the file runs, with what to write instead (FEEDBACK62.md,
/// Q-915; branch cache-and-migrate).
fn warn_dropped_parents(c: &rusqlite::Connection, name: &str, drops: &[String]) {
    for table in drops {
        let Ok(mut q) = c.prepare(
            "select m.name, f.on_delete from sqlite_schema m, pragma_foreign_key_list(m.name) f \
             where m.type = 'table' and f.\"table\" = ?1 collate nocase and m.name <> ?1 collate nocase \
             and f.on_delete in ('CASCADE', 'SET NULL', 'SET DEFAULT')",
        ) else {
            continue;
        };
        let children: Vec<(String, String)> = q
            .query_map([table], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        if let Some((child, action)) = children.first() {
            let more = if children.len() > 1 { format!(" (and {} more)", children.len() - 1) } else { String::new() };
            let does = if action == "CASCADE" { "deletes their rows" } else { "blanks their references" };
            eprintln!(
                "quillo: warning: migration {} drops table {}, which {} points at with on delete {}{}: with foreign keys on, the drop {}. To rebuild a table, write `pragma foreign_keys = off` in the file: Quillo turns them off for that file and checks them before it commits",
                name, table, child, action.to_ascii_lowercase(), more, does
            );
        }
    }
}

/// The rows whose foreign key finds no row: table, row, parent and which key.
fn violations(c: &rusqlite::Connection) -> Result<Vec<(String, Option<i64>, String, i64)>, String> {
    let mut q = c.prepare("pragma foreign_key_check").map_err(|e| e.to_string())?;
    let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

/// After a file run with foreign keys off: every row's foreign key still finds its row, but as
/// many as found none before it, or the file fails, naming a row of the first table and parent
/// that has more (FEEDBACK62.md, Q-915; branch cache-and-migrate). Counted by table and parent,
/// not by row, as a rebuilt table numbers its rows again, and one `without rowid` has none
/// (found in review).
fn foreign_key_check(c: &rusqlite::Connection, name: &str, before: &[(String, Option<i64>, String, i64)]) -> Result<(), String> {
    let all = violations(c)?;
    let count = |vs: &[(String, Option<i64>, String, i64)], t: &str, p: &str| vs.iter().filter(|v| v.0 == t && v.2 == p).count();
    let mut grown: Vec<(&str, &str)> = Vec::new();
    for v in &all {
        let key = (v.0.as_str(), v.2.as_str());
        if !grown.contains(&key) && count(&all, key.0, key.1) > count(before, key.0, key.1) {
            grown.push(key);
        }
    }
    let bad: Vec<_> = all.iter().filter(|v| grown.contains(&(v.0.as_str(), v.2.as_str()))).collect();
    if !before.is_empty() {
        let (t, row, p, _) = &before[0];
        let more = if before.len() > 1 { format!(" (and {} more)", before.len() - 1) } else { String::new() };
        eprintln!(
            "quillo: warning: before migration {} ran, {}{} already pointed at no row of {}{}: left as they are, as the file did not make them",
            name,
            t,
            row.map_or(String::new(), |id| format!(" row {}", id)),
            p,
            more
        );
    }
    // One that was not there before, when rows tell them apart; how many more there are now.
    let new_rows: usize = grown.iter().map(|(t, p)| count(&all, t, p) - count(before, t, p)).sum();
    match bad.iter().find(|v| !before.contains(v)).or(bad.first()) {
        None => Ok(()),
        Some((table, rowid, parent, _)) => {
            let row = rowid.map_or(String::new(), |id| format!(" row {}", id));
            let more = if new_rows > 1 { format!(" (and {} more)", new_rows - 1) } else { String::new() };
            Err(format!(
                "it turns foreign keys off, and leaves {}{} pointing at no row of {}{}: FOREIGN KEY constraint failed, so it was rolled back",
                table, row, parent, more
            ))
        }
    }
}

/// MySQL: one server at a time through a named lock. A statement that
/// changes the schema commits at once there (MySQL's rule), so a migration
/// that fails part way may leave what came before it done.
#[cfg(feature = "mysql")]
fn my_run(c: &mut crate::my::Conn, todo: &[(String, String)]) -> Result<Vec<String>, String> {
    c.query("select get_lock('quillo_migrations', 600) as got", &[], Some(1))?;
    let r = (|| {
        c.batch("create table if not exists _quillo_migrations (name varchar(255) primary key, sha256 char(64) not null, applied_at varchar(32) not null)")?;
        let mut applied = Vec::new();
        for (name, text) in todo {
            let hash = digest(text);
            let had = c.query("select sha256 from _quillo_migrations where name = ?", &[Value::str(name)], Some(1))?;
            if let Some(Value::Map(row)) = had.first() {
                if !matches!(row.get("sha256"), Some(Value::Str(s) | Value::Css(s)) if **s == *hash) {
                    eprintln!("quillo: warning: migration {} changed after it was applied; it is not run again", name);
                }
                continue;
            }
            c.batch("start transaction")?;
            let done = c.batch(text).and_then(|_| {
                c.exec(
                    "insert into _quillo_migrations (name, sha256, applied_at) values (?, ?, cast(now() as char))",
                    &[Value::str(name), Value::str(&hash)],
                )
            });
            match done {
                Ok(_) => {
                    if let Err(e) = c.batch("commit") {
                        let _ = c.batch("rollback");
                        return Err(format!("migration {} failed: {}", name, e));
                    }
                    applied.push(name.clone());
                }
                Err(e) => {
                    let _ = c.batch("rollback");
                    return Err(format!("migration {} failed: {} (on MySQL, its schema changes before the failure stay: they commit as they run)", name, e));
                }
            }
        }
        Ok(applied)
    })();
    let _ = c.query("select release_lock('quillo_migrations') as released", &[], Some(1));
    r
}

fn pg_run(c: &mut pg::Conn, todo: &[(String, String)]) -> Result<Vec<String>, String> {
    // One server at a time, across every server sharing the database.
    c.batch("select pg_advisory_lock(7171717171)")?;
    let r = (|| {
        c.batch(TABLE)?;
        let mut applied = Vec::new();
        for (name, text) in todo {
            let hash = digest(text);
            let had = c.query("select sha256 from _quillo_migrations where name = $1", &[Value::str(name)], Some(1))?;
            if let Some(Value::Map(row)) = had.first() {
                if !matches!(row.get("sha256"), Some(Value::Str(s) | Value::Css(s)) if **s == *hash) {
                    eprintln!("quillo: warning: migration {} changed after it was applied; it is not run again", name);
                }
                continue;
            }
            c.batch("begin")?;
            let done = c.batch(text).and_then(|_| {
                c.exec(
                    "insert into _quillo_migrations (name, sha256, applied_at) values ($1, $2, now()::text)",
                    &[Value::str(name), Value::str(&hash)],
                )
            });
            match done {
                Ok(_) => {
                    if let Err(e) = c.batch("commit") {
                        let _ = c.batch("rollback");
                        return Err(format!("migration {} failed: {}", name, e));
                    }
                    applied.push(name.clone());
                }
                Err(e) => {
                    let _ = c.batch("rollback");
                    return Err(format!("migration {} failed: {}", name, e));
                }
            }
        }
        Ok(applied)
    })();
    let _ = c.batch("select pg_advisory_unlock(7171717171)");
    r
}
