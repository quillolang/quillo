//! `db`: an embedded SQLite database, or PostgreSQL.
//!
//! ```text
//! db.open("app.db")                          // SQLite, next to the program
//! db.open("postgres://user:pass@host/app")   // PostgreSQL
//! db.exec("create table if not exists todos (id integer primary key, text text)")
//! db.exec("insert into todos (text) values (?)", text)      // { changes, last_id }
//! db.query("select * from todos where id > ?", 10)           // list of maps
//! db.one("select * from todos where id = :id", { id })       // a map or null
//! db.transaction(() => { ... })                              // all or nothing
//! ```
//!
//! SQL problems are returned as error values, so `?` and `??` work on them.
//! Until `db.open` is called the database is `$QUILLO_DB` (a path or a
//! postgres:// URL), or else SQLite in memory.
//!
//! SQLite queries run under the interpreter lock; they are fast and local.
//! Postgres queries release it while they wait on the network, and use a
//! pool of connections. A transaction keeps its connection to itself until
//! it ends, so other requests never run inside it.

use crate::interp::{fail, Ctrl, Interp};
use crate::pg;
use crate::value::*;
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

type R = Result<Value, Ctrl>;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("db.open", open),
        ("db.exec", exec),
        ("db.query", query),
        ("db.one", one),
        ("db.transaction", transaction),
        ("db.migrate", migrate),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("db.").into(), Value::Builtin(name, *f));
    }
}

/// The database a program uses.
pub enum Backend {
    Sqlite(Mutex<Connection>),
    Postgres(pg::Pool),
}

/// A Postgres connection kept by one request for its `db.transaction`.
pub struct PgTx {
    backend: Arc<Backend>,
    conn: Option<pg::Conn>,
    depth: usize,
}

fn db_err(e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("db: {}", e)))
}

fn connect(path: &str) -> Result<Connection, rusqlite::Error> {
    let c = if path == ":memory:" { Connection::open_in_memory()? } else { Connection::open(path)? };
    c.execute_batch("pragma foreign_keys = on; pragma journal_mode = wal;")?;
    Ok(c)
}

/// Opens a path (SQLite) or a postgres:// URL. Connecting to Postgres
/// waits on the network, so the interpreter lock is released meanwhile.
fn open_backend(it: &mut Interp, target: &str) -> Result<Backend, String> {
    if pg::is_url(target) {
        let url = target.to_string();
        return it.unlocked(move || pg::Pool::open(&url)).map(Backend::Postgres);
    }
    let full = if target == ":memory:" || Path::new(target).is_absolute() {
        target.to_string()
    } else {
        it.base_dir.join(target).display().to_string()
    };
    connect(&full).map(|c| Backend::Sqlite(Mutex::new(c))).map_err(|e| e.to_string())
}

/// The program's database, opening the default one if needed.
fn backend(it: &mut Interp) -> Result<Arc<Backend>, String> {
    if let Some(b) = it.db.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(b);
    }
    let target = std::env::var("QUILLO_DB").unwrap_or_else(|_| ":memory:".into());
    let b = Arc::new(open_backend(it, &target)?);
    let mut slot = it.db.lock().unwrap_or_else(|e| e.into_inner());
    Ok(slot.get_or_insert(b).clone())
}

/// Runs `f` on a Postgres connection without holding the interpreter
/// lock: the request's transaction connection if it has one, else one
/// from the pool.
fn pg_run<T: Send>(
    it: &mut Interp,
    b: &Arc<Backend>,
    f: impl FnOnce(&mut pg::Conn) -> Result<T, String> + Send,
) -> Result<T, String> {
    let Backend::Postgres(pool) = &**b else { unreachable!("pg_run on SQLite") };
    let pinned = match &mut *it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(tx) if Arc::ptr_eq(&tx.backend, b) => tx.conn.take(),
        _ => None,
    };
    match pinned {
        Some(mut c) => {
            let (c, r) = it.unlocked(move || {
                let r = f(&mut c);
                (c, r)
            });
            if let Some(tx) = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                tx.conn = Some(c);
            }
            r
        }
        None => it.unlocked(|| {
            let mut c = pool.checkout()?;
            let r = f(&mut c);
            pool.give_back(c);
            r
        }),
    }
}

/// In the server, outside a transaction, queries use the connections all
/// requests share (pipelined; see `pg::Lanes`). `None`: use `pg_run`.
fn shared_lanes(it: &Interp, b: &Arc<Backend>, pool: &pg::Pool) -> Option<Arc<pg::Lanes>> {
    let pinned = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(false, |t| Arc::ptr_eq(&t.backend, b));
    (!pinned && crate::httpd::in_task()).then(|| pool.lanes())
}

fn awaited<T>(r: Option<Result<Result<T, String>, String>>) -> Result<T, String> {
    match r {
        Some(Ok(v)) => v,
        Some(Err(e)) => Err(e),
        None => Err("not in a request".into()),
    }
}

fn to_sql(v: &Value) -> Sql {
    match v {
        Value::Nil => Sql::Null,
        Value::Bool(b) => Sql::Integer(*b as i64),
        Value::Int(n) => Sql::Integer(*n),
        Value::Float(f) => Sql::Real(*f),
        Value::Str(s) => Sql::Text(s.to_string()),
        Value::Html(h) => Sql::Text(h.to_string()),
        other => Sql::Text(other.to_json().to_string()),
    }
}

/// SQLite has no booleans: `true` is stored as 1. In a column declared
/// `boolean` (or `bool`) 0 and 1 come back as `false` and `true`, since 0
/// would be true in Quillo; elsewhere, such as `select x = 1`, they are ints.
fn from_sql(v: ValueRef, is_bool: bool) -> Value {
    match v {
        ValueRef::Null => Value::Nil,
        ValueRef::Integer(n @ (0 | 1)) if is_bool => Value::Bool(n == 1),
        ValueRef::Integer(n) => Value::Int(n),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::str(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => Value::str(String::from_utf8_lossy(b)),
    }
}

/// Binds `?` parameters from the arguments, or `:name` parameters from a
/// single map argument. As with Postgres, every name needs a value and
/// every key of the map must be a name in the query.
fn bind(stmt: &mut rusqlite::Statement, params: &[Value]) -> Result<(), String> {
    if let [Value::Map(m)] = params {
        if stmt.parameter_count() > 0 && stmt.parameter_name(1).is_some() {
            let mut bound = vec![false; stmt.parameter_count()];
            for (k, v) in m.iter() {
                let name = if k.starts_with([':', '@', '$']) { k.to_string() } else { format!(":{}", k) };
                match stmt.parameter_index(&name).map_err(|e| e.to_string())? {
                    Some(i) => {
                        stmt.raw_bind_parameter(i, to_sql(v)).map_err(|e| e.to_string())?;
                        bound[i - 1] = true;
                    }
                    None => return Err(format!("the query has no parameter {}", name)),
                }
            }
            if let Some(i) = bound.iter().position(|b| !b) {
                let name = stmt.parameter_name(i + 1).unwrap_or("?").to_string();
                return Err(format!("no value for {}", name));
            }
            return Ok(());
        }
    }
    let want = stmt.parameter_count();
    if params.len() != want {
        return Err(format!("the query has {} parameter{}, got {}", want, if want == 1 { "" } else { "s" }, params.len()));
    }
    for (i, v) in params.iter().enumerate() {
        stmt.raw_bind_parameter(i + 1, to_sql(v)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rows(conn: &Connection, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    bind(&mut stmt, params)?;
    let cols: Vec<(String, bool)> = stmt
        .columns()
        .iter()
        .map(|c| (c.name().to_string(), c.decl_type().map_or(false, |t| t.to_ascii_lowercase().starts_with("bool"))))
        .collect();
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut m = Map::new();
        for (i, (c, is_bool)) in cols.iter().enumerate() {
            m.insert(c.into(), from_sql(row.get_ref(i).map_err(|e| e.to_string())?, *is_bool));
        }
        out.push(Value::map(m));
        if limit.map_or(false, |l| out.len() >= l) {
            break;
        }
    }
    Ok(out)
}

/// Live updates: a statement's reads become the running loader's topics,
/// and its writes are announced (after the transaction, if any, commits).
fn announce(it: &mut Interp, sql: &str) {
    let t = crate::sqlinfo::tables_of(sql);
    if it.reads.is_some() {
        for r in &t.reads {
            it.watch(format!("db:{}", r));
        }
    }
    if !t.writes.is_empty() {
        it.publish(t.writes.iter().map(|w| format!("db:{}", w)).collect());
    }
}

/// Starts a transaction level; returns where its announcements begin.
fn tx_begin(it: &mut Interp) -> usize {
    it.tx_depth += 1;
    it.pending.len()
}

/// Ends a level: drops its announcements on rollback, and makes them
/// once the outermost transaction commits.
fn tx_end(it: &mut Interp, mark: usize, committed: bool) {
    if !committed {
        it.pending.truncate(mark);
    }
    it.tx_depth -= 1;
    if it.tx_depth == 0 {
        let topics = std::mem::take(&mut it.pending);
        it.shared.publish(&topics);
    }
}

fn sql_arg(args: &[Value], name: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.to_string()),
        other => fail(line, format!("`{}` expects SQL text as its first argument, got {}", name, other.map_or("nothing", |v| v.type_name()))),
    }
}

/// `db.open(target)`: a SQLite file next to the program, ":memory:", or a
/// postgres:// URL.
fn open(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let target = sql_arg(&args, "db.open", line)?;
    match open_backend(it, &target) {
        Ok(b) => {
            *it.db.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(b));
            Ok(Value::Nil)
        }
        Err(e) => Ok(db_err(e)),
    }
}

/// `db.migrate(dir = "migrations")`: applies the folder's SQL files the
/// database has not had yet, in name order (see `migrate`); returns their
/// names. A file that fails is rolled back and stops the program loading.
fn migrate(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let dir = match args.first() {
        None => "migrations".to_string(),
        Some(Value::Str(s)) => s.to_string(),
        Some(other) => return fail(line, format!("`db.migrate` expects a folder name, got {}", other.type_name())),
    };
    let full = it.base_dir.join(&dir);
    let b = match backend(it) {
        Ok(b) => b,
        Err(e) => return fail(line, format!("db.migrate: {}", e)),
    };
    let to = full.clone();
    match it.unlocked(move || crate::migrate::run(&b, &to)) {
        Ok(applied) => {
            std::sync::Arc::make_mut(&mut it.site).migrations = Some(full);
            for name in &applied {
                eprintln!("quillo: applied migration {}", name);
            }
            Ok(Value::list(applied.into_iter().map(Value::str).collect()))
        }
        Err(e) => fail(line, format!("db.migrate: {}", e)),
    }
}

/// `db.exec(sql, ...params)`: `{ changes, last_id }`. Without parameters the
/// text may hold several statements (a schema, for example).
fn exec(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sql = sql_arg(&args, "db.exec", line)?;
    let params = args[1..].to_vec();
    let b = match backend(it) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    let res = match &*b {
        Backend::Sqlite(m) => {
            let c = m.lock().unwrap_or_else(|e| e.into_inner());
            (|| {
                if params.is_empty() {
                    c.execute_batch(&sql).map_err(|e| e.to_string())?;
                } else {
                    let mut stmt = c.prepare(&sql).map_err(|e| e.to_string())?;
                    bind(&mut stmt, &params)?;
                    stmt.raw_execute().map_err(|e| e.to_string())?;
                }
                let mut m = Map::new();
                m.insert("changes".into(), Value::Int(c.changes() as i64));
                m.insert("last_id".into(), Value::Int(c.last_insert_rowid()));
                Ok(Value::map(m))
            })()
        }
        Backend::Postgres(pool) => match shared_lanes(it, &b, pool) {
            Some(lanes) => {
                let sql = sql.clone();
                // Boxed here, so the (large) future is not moved again.
                awaited(crate::httpd::await_elsewhere(Box::pin(lanes.exec(crate::httpd::request_number(), sql, params))))
            }
            None => {
                let sql = sql.clone();
                pg_run(it, &b, move |c| c.exec(&sql, &params))
            }
        },
    };
    if res.is_ok() {
        announce(it, &sql);
    }
    Ok(res.unwrap_or_else(db_err))
}

fn select(it: &mut Interp, sql: String, params: Vec<Value>, limit: Option<usize>) -> Result<Vec<Value>, String> {
    let b = backend(it)?;
    let res = match &*b {
        Backend::Sqlite(m) => rows(&m.lock().unwrap_or_else(|e| e.into_inner()), &sql, &params, limit),
        Backend::Postgres(pool) => match shared_lanes(it, &b, pool) {
            Some(lanes) => awaited(crate::httpd::await_elsewhere(Box::pin(lanes.query(crate::httpd::request_number(), sql.clone(), params, limit)))),
            None => {
                let sql = sql.clone();
                pg_run(it, &b, move |c| c.query(&sql, &params, limit))
            }
        },
    };
    if res.is_ok() {
        announce(it, &sql);
    }
    res
}

/// `db.query(sql, ...params)`: every row, as maps from column to value.
fn query(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sql = sql_arg(&args, "db.query", line)?;
    Ok(select(it, sql, args[1..].to_vec(), None).map(Value::list).unwrap_or_else(db_err))
}

/// `db.one(sql, ...params)`: the first row, or null.
fn one(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sql = sql_arg(&args, "db.one", line)?;
    Ok(select(it, sql, args[1..].to_vec(), Some(1))
        .map(|r| r.into_iter().next().unwrap_or(Value::Nil))
        .unwrap_or_else(db_err))
}

/// `db.transaction(() => { ... })`: commits if the function returns normally,
/// rolls back if it fails, returns an error value, or replies early.
/// Transactions nest (as savepoints).
fn transaction(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = match args.first() {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => {
            return fail(line, format!("`db.transaction` expects a function, got {}", other.map_or("nothing", |v| v.type_name())))
        }
    };
    let b = match backend(it) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    let ok = |r: &R| matches!(r, Ok(v) if !matches!(v, Value::Err(_)));
    match &*b {
        Backend::Sqlite(m) => {
            let run = |sql: &str| m.lock().unwrap_or_else(|e| e.into_inner()).execute_batch(sql).map_err(|e| e.to_string());
            if let Err(e) = run("savepoint quillo_tx") {
                return Ok(db_err(e));
            }
            let mark = tx_begin(it);
            it.sqlite_tx += 1;
            let r = it.call(&f, Vec::new(), line);
            it.sqlite_tx -= 1;
            let commit = ok(&r);
            let end = if commit { "release quillo_tx" } else { "rollback to quillo_tx; release quillo_tx" };
            let ended = run(end);
            tx_end(it, mark, commit && ended.is_ok());
            match ended {
                Err(e) if commit => Ok(db_err(e)),
                _ => r,
            }
        }
        Backend::Postgres(pool) => {
            // Keep one connection for this request until the outermost
            // transaction ends.
            let pinned = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(false, |t| Arc::ptr_eq(&t.backend, &b));
            if !pinned {
                let c = match it.unlocked(|| pool.checkout()) {
                    Ok(c) => c,
                    Err(e) => return Ok(db_err(e)),
                };
                *it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(PgTx { backend: b.clone(), conn: Some(c), depth: 0 });
            }
            let depth = {
                let mut g = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner());
                let tx = g.as_mut().expect("pinned");
                tx.depth += 1;
                tx.depth
            };
            let begin = if depth == 1 { "begin".to_string() } else { format!("savepoint quillo_tx_{}", depth) };
            let started = pg_run(it, &b, move |c| c.batch(&begin));
            let mark = tx_begin(it);
            let r = match started {
                Ok(()) => it.call(&f, Vec::new(), line),
                Err(e) => Ok(db_err(e)),
            };
            let commit = ok(&r);
            let end = match (commit, depth == 1) {
                (true, true) => "commit".to_string(),
                (false, true) => "rollback".to_string(),
                (true, false) => format!("release savepoint quillo_tx_{}", depth),
                (false, false) => format!("rollback to savepoint quillo_tx_{d}; release savepoint quillo_tx_{d}", d = depth),
            };
            let ended = pg_run(it, &b, move |c| c.batch(&end));
            tx_end(it, mark, commit && ended.is_ok());
            let done = {
                let mut g = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner());
                let tx = g.as_mut().expect("pinned");
                tx.depth -= 1;
                if tx.depth == 0 {
                    g.take()
                } else {
                    None
                }
            };
            if let Some(PgTx { conn: Some(c), .. }) = done {
                pool.give_back(c);
            }
            match ended {
                Err(e) if commit => Ok(db_err(e)),
                _ => r,
            }
        }
    }
}
