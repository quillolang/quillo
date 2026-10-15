//! `db`: an embedded SQLite database.
//!
//! ```text
//! db.open("app.db")
//! db.exec("create table if not exists todos (id integer primary key, text text)")
//! db.exec("insert into todos (text) values (?)", text)      // { changes, last_id }
//! db.query("select * from todos where id > ?", 10)           // list of maps
//! db.one("select * from todos where id = :id", { id })       // a map or nil
//! db.transaction(fn() { ... })                               // all or nothing
//! ```
//!
//! SQL problems are returned as error values, so `?` and `??` work on them.
//! Until `db.open` is called the database is `$LITE_DB`, or else in memory.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::Connection;
use std::path::Path;

type R = Result<Value, Ctrl>;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("db.open", open),
        ("db.exec", exec),
        ("db.query", query),
        ("db.one", one),
        ("db.transaction", transaction),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("db.").into(), Value::Builtin(name, *f));
    }
}

fn db_err(e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("db: {}", e)))
}

fn connect(path: &str) -> Result<Connection, rusqlite::Error> {
    let c = if path == ":memory:" { Connection::open_in_memory()? } else { Connection::open(path)? };
    c.execute_batch("pragma foreign_keys = on; pragma journal_mode = wal;")?;
    Ok(c)
}

/// Runs `f` with the program's connection, opening the default one if needed.
fn with_conn<T>(it: &Interp, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
    let mut slot = it.shared.db.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        let path = std::env::var("LITE_DB").unwrap_or_else(|_| ":memory:".into());
        *slot = Some(connect(&path).map_err(|e| e.to_string())?);
    }
    f(slot.as_ref().unwrap())
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

fn from_sql(v: ValueRef) -> Value {
    match v {
        ValueRef::Null => Value::Nil,
        ValueRef::Integer(n) => Value::Int(n),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::str(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => Value::str(String::from_utf8_lossy(b)),
    }
}

/// Binds `?` parameters from the arguments, or `:name` parameters from a
/// single map argument.
fn bind(stmt: &mut rusqlite::Statement, params: &[Value]) -> Result<(), String> {
    if let [Value::Map(m)] = params {
        if stmt.parameter_count() > 0 && stmt.parameter_name(1).is_some() {
            for (k, v) in m.iter() {
                let name = if k.starts_with([':', '@', '$']) { k.clone() } else { format!(":{}", k) };
                match stmt.parameter_index(&name).map_err(|e| e.to_string())? {
                    Some(i) => stmt.raw_bind_parameter(i, to_sql(v)).map_err(|e| e.to_string())?,
                    None => return Err(format!("the query has no parameter {}", name)),
                }
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
    let cols: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut m = Map::new();
        for (i, c) in cols.iter().enumerate() {
            m.insert(c.clone(), from_sql(row.get_ref(i).map_err(|e| e.to_string())?));
        }
        out.push(Value::map(m));
        if limit.map_or(false, |l| out.len() >= l) {
            break;
        }
    }
    Ok(out)
}

fn sql_arg(args: &[Value], name: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.to_string()),
        other => fail(line, format!("`{}` expects SQL text as its first argument, got {}", name, other.map_or("nothing", |v| v.type_name()))),
    }
}

/// `db.open(path)`: a file next to the program, or ":memory:".
fn open(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let path = sql_arg(&args, "db.open", line)?;
    let full = if path == ":memory:" || Path::new(&path).is_absolute() {
        path
    } else {
        it.base_dir.join(&path).display().to_string()
    };
    match connect(&full) {
        Ok(c) => {
            *it.shared.db.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
            Ok(Value::Nil)
        }
        Err(e) => Ok(db_err(e)),
    }
}

/// `db.exec(sql, ...params)`: `{ changes, last_id }`. Without parameters the
/// text may hold several statements (a schema, for example).
fn exec(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sql = sql_arg(&args, "db.exec", line)?;
    let params = &args[1..];
    let res = with_conn(it, |c| {
        if params.is_empty() {
            c.execute_batch(&sql).map_err(|e| e.to_string())?;
        } else {
            let mut stmt = c.prepare(&sql).map_err(|e| e.to_string())?;
            bind(&mut stmt, params)?;
            stmt.raw_execute().map_err(|e| e.to_string())?;
        }
        let mut m = Map::new();
        m.insert("changes".into(), Value::Int(c.changes() as i64));
        m.insert("last_id".into(), Value::Int(c.last_insert_rowid()));
        Ok(Value::map(m))
    });
    Ok(res.unwrap_or_else(db_err))
}

/// `db.query(sql, ...params)`: every row, as maps from column to value.
fn query(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sql = sql_arg(&args, "db.query", line)?;
    Ok(with_conn(it, |c| rows(c, &sql, &args[1..], None)).map(Value::list).unwrap_or_else(db_err))
}

/// `db.one(sql, ...params)`: the first row, or nil.
fn one(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sql = sql_arg(&args, "db.one", line)?;
    Ok(with_conn(it, |c| rows(c, &sql, &args[1..], Some(1)))
        .map(|r| r.into_iter().next().unwrap_or(Value::Nil))
        .unwrap_or_else(db_err))
}

/// `db.transaction(fn() { ... })`: commits if the function returns normally,
/// rolls back if it fails, returns an error value, or replies early.
fn transaction(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = match args.first() {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => {
            return fail(line, format!("`db.transaction` expects a function, got {}", other.map_or("nothing", |v| v.type_name())))
        }
    };
    if let Err(e) = with_conn(it, |c| c.execute_batch("savepoint lite_tx").map_err(|e| e.to_string())) {
        return Ok(db_err(e));
    }
    let r = it.call(&f, Vec::new(), line);
    let commit = matches!(&r, Ok(v) if !matches!(v, Value::Err(_)));
    let end = if commit { "release lite_tx" } else { "rollback to lite_tx; release lite_tx" };
    if let Err(e) = with_conn(it, |c| c.execute_batch(end).map_err(|e| e.to_string())) {
        if commit {
            return Ok(db_err(e));
        }
    }
    r
}
