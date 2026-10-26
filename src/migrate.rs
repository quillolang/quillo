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

/// The files to apply, in order: name and text.
fn files(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().map_or(false, |x| x == "sql") && path.is_file() {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
            out.push((name, text));
        }
    }
    out.sort();
    Ok(out)
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect()
}

const TABLE: &str = "create table if not exists _quillo_migrations (name text primary key, sha256 text not null, applied_at text not null)";

/// Applies what `dir` has that the database has not; returns the names
/// applied. Stops at the first file that fails, which is rolled back.
pub fn run(db: &Backend, dir: &Path) -> Result<Vec<String>, String> {
    let todo = files(dir)?;
    match db {
        Backend::Sqlite(m) => {
            let c = m.lock().unwrap_or_else(|e| e.into_inner());
            c.execute_batch(TABLE).map_err(|e| e.to_string())?;
            let mut applied = Vec::new();
            for (name, text) in &todo {
                let hash = digest(text);
                c.execute_batch("begin immediate").map_err(|e| e.to_string())?;
                let had: Option<String> = c
                    .query_row("select sha256 from _quillo_migrations where name = ?1", [name], |r| r.get(0))
                    .ok();
                if let Some(had) = had {
                    let _ = c.execute_batch("commit");
                    if had != hash {
                        eprintln!("quillo: warning: migration {} changed after it was applied; it is not run again", name);
                    }
                    continue;
                }
                let done = c.execute_batch(text).and_then(|_| {
                    c.execute(
                        "insert into _quillo_migrations (name, sha256, applied_at) values (?1, ?2, datetime('now'))",
                        [name, &hash],
                    )
                });
                match done {
                    Ok(_) => {
                        c.execute_batch("commit").map_err(|e| e.to_string())?;
                        applied.push(name.clone());
                    }
                    Err(e) => {
                        let _ = c.execute_batch("rollback");
                        return Err(format!("migration {} failed: {}", name, e));
                    }
                }
            }
            Ok(applied)
        }
        Backend::Postgres(pool) => {
            let mut c = pool.checkout()?;
            let r = pg_run(&mut c, &todo);
            pool.give_back(c);
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
                if !matches!(row.get("sha256"), Some(Value::Str(s)) if **s == *hash) {
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
                    c.batch("commit")?;
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
                if !matches!(row.get("sha256"), Some(Value::Str(s)) if **s == *hash) {
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
                    c.batch("commit")?;
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
