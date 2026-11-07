//! `db`: an embedded SQLite database, PostgreSQL, or MySQL/MariaDB; and
//! as many of them at once as a program connects to.
//!
//! ```text
//! db.open("app.db")                          // SQLite, next to the program
//! db.open("postgres://user:pass@host/app")   // PostgreSQL
//! db.exec("create table if not exists todos (id integer primary key, text text)")
//! db.exec("insert into todos (text) values (?)", text)      // { changes, last_id }
//! db.query("select * from todos where id > ?", 10)           // list of maps
//! db.one("select * from todos where id = :id", { id })       // a map or null
//! db.transaction(() => { ... })                              // all or nothing
//! let analytics = db.connect("postgres://.../analytics")     // another database
//! analytics.query("select ...")                              // the same functions
//! db.open((tenant) => registry.one("select url from tenants where name = ?", tenant).url)
//! ```
//!
//! `db` is the program's database; `db.connect` opens others, each with the
//! same functions, its own transactions, migrations and live updates. A
//! target with `{tenant}` in it, or a function of the tenant's name, gives
//! each tenant a database of its own (with a function, of any kind).
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
        ("db.exec_many", exec_many),
        ("db.query", query),
        ("db.one", one),
        ("db.transaction", transaction),
        ("db.migrate", migrate),
        ("db.tenants", tenants),
        ("db.tenant_exists", tenant_exists),
        ("db.connect", db_connect),
        ("db.system", system),
        ("db.shards", shards),
        ("db.atomic", atomic),
        ("db.error_kind", error_kind),
        ("db.sql_name", sql_name_on),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("db.").into(), Value::Builtin(name, *f));
    }
}

/// The database a program uses.
pub enum Backend {
    Sqlite(Mutex<Connection>),
    Postgres(pg::Pool),
    #[cfg(feature = "mysql")]
    Mysql(crate::my::Pool),
}

/// A MySQL or MariaDB URL (whether or not this Quillo was built with MySQL).
pub fn is_mysql_url(s: &str) -> bool {
    s.starts_with("mysql://") || s.starts_with("mariadb://")
}

/// Whether the program's database is MySQL (or will be, once opened):
/// `sql_name` quotes names as it reads them.
pub(crate) fn program_db_is_mysql(it: &Interp) -> bool {
    is_mysql_on(it, None)
}

/// Whether the database `on` names (the program's for `None`) is MySQL.
fn is_mysql_on(it: &Interp, on: Option<usize>) -> bool {
    match slot_on(it, on) {
        Some(DbSlot::One(b)) => b.is_mysql(),
        Some(DbSlot::PerTenant(t)) => match &t.target {
            Target::Text(s) => is_mysql_url(s),
            Target::Fn(_) => t.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner).values().next().map_or(false, |b| b.is_mysql()),
        },
        Some(DbSlot::Shards(_)) => false,
        None if on.is_some() => false,
        None => std::env::var("QUILLO_DB").map_or(false, |t| is_mysql_url(&t)),
    }
}

/// `analytics.sql_name(col)`: a name quoted for the database it is called on, not the
/// program's: MySQL reads "x" as text (FEEDBACK43.md, Q-492; branch feedback40-43).
fn sql_name_on(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let mysql = is_mysql_on(it, on);
    crate::builtins::sql_idents(&args, "sql_name", line, |s| if mysql { format!("`{}`", s) } else { format!("\"{}\"", s) })
}

/// A database on a server (not a SQLite file).
fn is_server_url(s: &str) -> bool {
    pg::is_url(s) || is_mysql_url(s)
}

/// A connection taken from a server database's pool.
pub enum Pinned {
    Pg(pg::Conn),
    #[cfg(feature = "mysql")]
    My(crate::my::Conn),
}

/// What a statement gets in a transaction that already failed because of
/// another (see `crate::txn`).
const CONFLICTED: &str = "the transaction conflicted with another and was rolled back";

impl Pinned {
    pub(crate) fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        if self.conflicted() {
            return Err(CONFLICTED.into());
        }
        match self {
            Pinned::Pg(c) => c.query(sql, params, limit),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.query(sql, params, limit),
        }
    }

    pub(crate) fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        if self.conflicted() {
            return Err(CONFLICTED.into());
        }
        match self {
            Pinned::Pg(c) => c.exec(sql, params),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.exec(sql, params),
        }
    }

    pub(crate) fn batch(&mut self, sql: &str) -> Result<(), String> {
        match self {
            Pinned::Pg(c) => c.batch(sql),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.batch(sql),
        }
    }

    fn watch_conflicts(&mut self, on: bool) {
        match self {
            Pinned::Pg(c) => c.watch_conflicts(on),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.watch_conflicts(on),
        }
    }

    fn conflicted(&self) -> bool {
        match self {
            Pinned::Pg(c) => c.conflicted(),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.conflicted(),
        }
    }
}

impl Backend {
    /// A connection from the pool, for a server database.
    pub(crate) fn checkout(&self) -> Result<Pinned, String> {
        match self {
            Backend::Postgres(p) => p.checkout().map(Pinned::Pg),
            #[cfg(feature = "mysql")]
            Backend::Mysql(p) => p.checkout().map(Pinned::My),
            Backend::Sqlite(_) => Err("SQLite has no pool".into()),
        }
    }

    /// A connection for work done while the request may hold one already
    /// (Quillo's own tables): it never waits for one of the pool's (see
    /// `pg::Pool::checkout_nested`). MySQL's pool is the driver's, larger.
    pub(crate) fn checkout_nested(&self) -> Result<Pinned, String> {
        match self {
            Backend::Postgres(p) => p.checkout_nested().map(Pinned::Pg),
            #[cfg(feature = "mysql")]
            Backend::Mysql(p) => p.checkout().map(Pinned::My),
            Backend::Sqlite(_) => Err("SQLite has no pool".into()),
        }
    }

    pub(crate) fn discard(&self, c: Pinned) {
        match (self, c) {
            (Backend::Postgres(p), Pinned::Pg(c)) => p.discard(c),
            #[cfg(feature = "mysql")]
            (Backend::Mysql(p), Pinned::My(c)) => p.give_back(c),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    pub(crate) fn give_back(&self, c: Pinned) {
        match (self, c) {
            (Backend::Postgres(p), Pinned::Pg(c)) => p.give_back(c),
            #[cfg(feature = "mysql")]
            (Backend::Mysql(p), Pinned::My(c)) => p.give_back(c),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    pub(crate) fn is_mysql(&self) -> bool {
        #[cfg(feature = "mysql")]
        if let Backend::Mysql(_) = self {
            return true;
        }
        false
    }

    /// How the database reads SQL (src/sql.rs).
    pub(crate) fn dialect(&self) -> crate::sql::Dialect {
        match self {
            Backend::Sqlite(_) => crate::sql::Dialect::Sqlite,
            Backend::Postgres(_) => crate::sql::Dialect::Postgres,
            #[cfg(feature = "mysql")]
            Backend::Mysql(_) => crate::sql::Dialect::Mysql,
        }
    }
}

/// The program's database: one, or one per tenant when `db.open`'s target
/// has `{tenant}` in it (`data/{tenant}.db`, `postgres://host/app_{tenant}`).
#[derive(Clone)]
pub enum DbSlot {
    One(Arc<Backend>),
    PerTenant(Arc<TenantDbs>),
    /// `db.shards([...])`: the connections (by number) of its shards.
    Shards(Arc<Vec<usize>>),
}

// ---- replicas -------------------------------------------------------------------------------

/// A database's read replicas (`db.open(primary, { replicas: [...] })`):
/// reads go to them in turn; one that fails is left out for a while.
struct Replicas {
    list: Vec<Arc<Backend>>,
    next: std::sync::atomic::AtomicUsize,
    down: Mutex<Vec<Option<std::time::Instant>>>,
}

/// Each primary's replicas. (Held weakly: a primary dropped is no longer found.)
fn replica_sets() -> &'static Mutex<Vec<(std::sync::Weak<Backend>, Arc<Replicas>)>> {
    static R: std::sync::OnceLock<Mutex<Vec<(std::sync::Weak<Backend>, Arc<Replicas>)>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

fn replicas_of(b: &Arc<Backend>) -> Option<Arc<Replicas>> {
    let mut sets = replica_sets().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    sets.retain(|(w, _)| w.strong_count() > 0);
    sets.iter().find(|(w, _)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(b))).map(|(_, r)| r.clone())
}

/// Whether a statement only reads, as its database reads it: see `sql::Scan::reads_only`
/// (FEEDBACK55.md, Q-750; branch feedback55).
fn only_reads(scan: &crate::sql::Scan) -> bool {
    scan.reads_only()
}

/// Where a read goes: a replica of `b`, unless it is in a transaction on
/// `b`, or this request wrote (it reads what it wrote, on the primary). A statement that
/// writes where its words do not say so (`select nextval('s')`, a function that writes) goes
/// to the primary: the databases' own such functions by name, the program's own when looked up
/// (`isolated`, which the caller asks first; FEEDBACK55.md, Q-750; branch feedback55).
fn reader(it: &Interp, b: &Arc<Backend>, scan: &crate::sql::Scan) -> Option<(Arc<Replicas>, usize)> {
    let reps = replicas_of(b)?;
    if it.wrote_req == Some(crate::httpd::request_number()) || !only_reads(scan) {
        return None;
    }
    if it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|t| Arc::ptr_eq(&t.backend, b)) {
        return None;
    }
    let n = reps.list.len();
    let start = reps.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = std::time::Instant::now();
    let down = reps.down.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let i = (0..n).map(|k| (start + k) % n).find(|i| down[*i].map_or(true, |t| now >= t))?;
    drop(down);
    Some((reps, i))
}

/// Tenants' databases, opened when first used (and migrated, after
/// `db.migrate`); the least recently used are closed past `OPEN_TENANTS`.
pub struct TenantDbs {
    target: Target,
    open: Mutex<indexmap::IndexMap<String, Arc<Backend>>>,
    /// The migrations each is brought up to date with when opened.
    migrations: Mutex<Option<std::path::PathBuf>>,
    /// `?open_tenants=N` in a server's URL: how many are kept open, in place of
    /// `OPEN_PG_TENANTS`, as a SaaS passes 16 customers early and each past it connects
    /// again. Connections are not shared between tenants: they keep statements prepared
    /// against one schema (FEEDBACK20.md, Q-168; branch feedback20-fixes).
    most_open: Option<usize>,
}

/// Where a tenant's database is: `{tenant}` in a path or URL, or what a
/// function of the tenant's name gives (a database of any kind each).
enum Target {
    Text(String),
    Fn(Value),
}

impl TenantDbs {
    fn new(target: Target) -> TenantDbs {
        let most_open = match &target {
            Target::Text(t) => t.split_once('?').and_then(|(_, q)| q.split('&').find_map(|p| p.strip_prefix("open_tenants=")?.parse().ok())),
            Target::Fn(_) => None,
        };
        TenantDbs { target, open: Mutex::new(Default::default()), migrations: Mutex::new(None), most_open }
    }

    fn template(&self) -> Option<&str> {
        match &self.target {
            Target::Text(t) => Some(t),
            Target::Fn(_) => None,
        }
    }
}

/// The key of a connection's number in the map `db.connect` gives.
pub const CONN_KEY: &str = "$db";

/// The connection `recv.name(...)` is a `db` function of, if it is one.
pub(crate) fn conn_of(recv: &Value, name: &str) -> Option<usize> {
    let Value::Map(m) = recv else { return None };
    match (m.get(name), m.get(CONN_KEY)) {
        (Some(Value::Builtin(b, _)), Some(Value::Int(id))) if b.starts_with("db.") => Some(*id as usize),
        _ => None,
    }
}

/// Tenant databases kept open at once: SQLite files, and Postgres
/// databases or schemas, whose pools keep up to 4 connections each.
const OPEN_TENANTS: usize = 1024;
const OPEN_PG_TENANTS: usize = 16;

/// A server database's connection kept by one request for its
/// `db.transaction`, one for each database it has one open on.
pub struct TxConn {
    backend: Arc<Backend>,
    conn: Option<Pinned>,
    depth: usize,
}

/// What a session changed is set back, its prepared statements kept (Quillo's own among them):
/// whether that worked. One that did not is closed, not given back (found in review).
fn reset_session(c: &mut Pinned) -> bool {
    match c {
        Pinned::Pg(_) => c.batch(pg::RESET).is_ok(),
        #[allow(unreachable_patterns)]
        _ => true,
    }
}

/// A statement that changes the session, on PostgreSQL: refused outside a transaction, where
/// the connection is shared by every request (Q-581); inside one it is allowed, and the
/// connection reset as every transaction ends (see `attempt`). Read by the tokenizer, in any
/// statement and with the values given (`set_config(?, ?, ?)` with false; FEEDBACK55.md,
/// Q-708; branch feedback55).
fn session_change(it: &Interp, b: &Arc<Backend>, scan: &crate::sql::Scan, params: &[Value]) -> Result<(), String> {
    if !matches!(&**b, Backend::Postgres(_)) {
        return Ok(());
    }
    // A statement prepared by name outlives everything, and cannot be let go without Quillo's
    // own (found in review).
    if scan.prepares() {
        return Err("`prepare` makes a statement for the whole session: pass the values as parameters (`?`) instead, which Quillo prepares itself".to_string());
    }
    if !scan.changes_session(params) {
        return Ok(());
    }
    if in_tx_on(it, b) {
        return Ok(());
    }
    Err("outside `db.transaction` this connection is shared by every request, so what this changes would reach others: use `set local` (or `set_config(..., true)`) inside `db.transaction`".to_string())
}

/// Whether the request has a transaction open on `b`.
fn in_tx_on(it: &Interp, b: &Arc<Backend>) -> bool {
    it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|t| Arc::ptr_eq(&t.backend, b))
}

/// Whether a statement outside a transaction, on PostgreSQL, may change the session where its
/// reading cannot see: a `do` block, whose SQL may be made as it runs (`execute`), or a call
/// of a function of the database's own, which may call `set_config` (looked up once a name,
/// in `pg_proc`). It then runs on a connection of its own, reset before it goes back
/// (`isolated_run`), not on one other requests share. Inside a transaction every connection is
/// reset as it ends (FEEDBACK55.md, Q-708; branch feedback55).
fn isolated(it: &mut Interp, b: &Arc<Backend>, scan: &crate::sql::Scan) -> bool {
    let Backend::Postgres(pool) = &**b else { return false };
    if (!scan.unseen && scan.calls.is_empty()) || in_tx_on(it, b) {
        return false;
    }
    if scan.unseen {
        return true;
    }
    if let Some(own) = pool.calls_own(&scan.calls) {
        return own;
    }
    let (names, b) = (scan.calls.clone(), b.clone());
    // One that cannot be looked up is taken for the program's.
    it.unlocked(move || match &*b {
        Backend::Postgres(p) => p.look_up_fns(&names),
        _ => Ok(false),
    })
    .unwrap_or(true)
}

/// `f` on a connection of the pool's own, reset before it goes back, or closed when that
/// fails (see `isolated`).
fn isolated_run<T: Send>(it: &mut Interp, b: &Arc<Backend>, f: impl FnOnce(&mut Pinned) -> Result<T, String> + Send) -> Result<T, String> {
    let mut c = checkout_for_tx(it, b)?;
    let b = b.clone();
    it.unlocked(move || {
        let r = f(&mut c);
        if reset_session(&mut c) {
            b.give_back(c);
        } else {
            b.discard(c);
        }
        r
    })
}

/// On MySQL, a statement that commits the transaction it is in by itself (`truncate`, `create
/// table`, ...), inside `db.transaction`: refused before it runs, so that what ran before it can
/// still be rolled back, as PostgreSQL and SQLite roll back theirs; the server would have
/// committed it, and the rest after it outside any transaction (FEEDBACK55.md, Q-728; branch
/// feedback55).
fn no_commit_inside(it: &Interp, b: &Arc<Backend>, scan: &crate::sql::Scan, f: &str, line: u32) -> Result<(), Ctrl> {
    if !b.is_mysql() || !it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|t| Arc::ptr_eq(&t.backend, b)) {
        return Ok(());
    }
    match scan.commits_by_itself() {
        Some(stmt) => fail(line, commits_by_itself(f, stmt)),
        None => Ok(()),
    }
}

/// What MySQL's driver says when a statement ended the transaction it was in (the server's
/// flag after it: a procedure's `create table`, `execute immediate`, a block's).
pub(crate) const COMMITTED: &str = "\u{1}committed by itself";

/// A statement after which MySQL was no longer in the transaction: it committed what ran before
/// it, which can no longer be rolled back; the function ends here, as at a refusal
/// (FEEDBACK55.md, Q-728; branch feedback55).
fn committed_inside<T>(res: &Result<T, String>, scan: &crate::sql::Scan, f: &str, line: u32) -> Result<(), Ctrl> {
    match res {
        Err(e) if e == COMMITTED => fail(line, format!(
            "`{}(\"{}\")` inside a transaction: MySQL and MariaDB committed the transaction around this statement by itself, with what ran before it, which can no longer be rolled back: run it before or after the transaction",
            f,
            scan.sql.trim()
        )),
        _ => Ok(()),
    }
}

fn commits_by_itself(f: &str, stmt: &str) -> String {
    format!(
        "`{}(\"{}\")` inside a transaction: MySQL and MariaDB commit the transaction around this statement by itself, and what ran before it could no longer be rolled back: run it before or after the transaction",
        f, stmt
    )
}

/// The values a statement is given, as every database takes them: an error is given back, and
/// a function refused, where it was stored as its text (FEEDBACK55.md, Q-727); one map where
/// the SQL has no placeholder is no value (Q-747); and placeholders with no values are refused
/// where SQLite stored nulls (Q-729; branch feedback55).
fn given_values(scan: &crate::sql::Scan, params: Vec<Value>) -> Result<Vec<Value>, String> {
    // `@name` with a map that has `name`, on MySQL: a variable of the session, null unless set,
    // where SQLite takes the map's value; the map was silently left out (FEEDBACK55.md, Q-752;
    // branch feedback55).
    if let Some(v) = scan.var_given(&params) {
        return Err(format!("`@{}` is a variable of MySQL's session (null unless set), not one of the values: write `:{}` for the value", v, v));
    }
    // Not a value: not looked at (found in review).
    if matches!(params.as_slice(), [Value::Map(_)]) && !scan.named && scan.count == 0 {
        return Ok(Vec::new());
    }
    match scan.by_names(&params) {
        Some(m) => {
            // By the name as written (`:id`, SQLite's `@id`), as the value is looked up.
            for name in scan.signed_names() {
                if let Some(v) = crate::sql::Scan::by_name(m, name) {
                    storable(v, false, false).map_err(|why| format!("value {}{}", name, why))?;
                }
            }
        }
        None => {
            for (i, v) in params.iter().enumerate() {
                storable(v, true, false).map_err(|why| format!("value {}{}", i + 1, why))?;
            }
        }
    }
    if params.is_empty() && scan.count > 0 {
        return Err(crate::sql::wrong_count(scan.count, 0));
    }
    Ok(params)
}

/// Whether a value can be stored: what it holds, if a list or a map, too (`inside`). An error
/// the call was given itself (`given`) is told by the interpreter ("it failed before this
/// call"); one in a map of named values, or inside another value, is told here.
///
/// NaN and the infinities are refused, naming them, on every database: SQLite stored NaN as
/// null, and MySQL and MariaDB refuse both as "out of range", while PostgreSQL keeps them
/// (FEEDBACK55.md, Q-735; branch feedback55).
fn storable(v: &Value, given: bool, inside: bool) -> Result<(), String> {
    match v {
        Value::Float(f) if !f.is_finite() => Err(format!(
            "{} {}, which cannot be stored: SQLite keeps NaN as null and MySQL refuses NaN and the infinities, so no database is given one",
            if inside { " holds" } else { " is" },
            v.repr()
        )),
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Dec(_) | Value::Str(_) | Value::Html(_) => Ok(()),
        Value::List(l) => l.iter().try_for_each(|x| storable(x, false, true)),
        Value::Map(m) => m.values().try_for_each(|x| storable(x, false, true)),
        Value::Err(..) if given => Err(" cannot be stored: got error".to_string()),
        Value::Err(e, _, at) => Err(format!(
            "{}: got error (it failed before this call, {})",
            if inside { " holds an error, which cannot be stored" } else { " cannot be stored" },
            at.before(&crate::value::told(|| e.display()))
        )),
        other if inside => Err(format!(" holds a {t}, and a {t} cannot be stored in the database", t = other.type_name())),
        other => Err(format!(": a {} cannot be stored in the database", other.type_name())),
    }
}

/// A list or a map as JSON for a database, as `Value::to_json` writes it but a whole `dec` as a
/// JSON number: written as a string, a number past an int read from a JSON column (a `dec`)
/// went back as a string, and no longer matched its row (FEEDBACK55.md, Q-732; branch
/// feedback55). A `dec` with a fraction stays a string, as a JSON number with a fraction reads
/// as a float.
pub(crate) fn json_text(v: &Value) -> String {
    fn write(v: &Value, depth: usize, out: &mut String) {
        match v {
            // Deeper than this, as `to_json` (which stops at its own depth).
            _ if depth > 512 => out.push_str(&v.to_json().to_string()),
            Value::List(l) => {
                out.push('[');
                for (i, x) in l.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(x, depth + 1, out);
                }
                out.push(']');
            }
            Value::Map(m) => {
                out.push('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String(k.to_string()).to_string());
                    out.push(':');
                    write(x, depth + 1, out);
                }
                out.push('}');
            }
            Value::Dec(d) if d.scale() == 0 => out.push_str(&d.to_string()),
            other => out.push_str(&other.to_json().to_string()),
        }
    }
    let mut out = String::new();
    write(v, 0, &mut out);
    out
}

fn db_err(e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("db: {}", e)))
}

/// `db.error_kind(e)`: what kind of failure a database's error value is,
/// the same for SQLite, PostgreSQL and MySQL, whose words differ: "unique"
/// (a key or unique column already has the value), "foreign_key",
/// "not_null", "check", "conflict" (another transaction: busy, a deadlock,
/// a serialization failure), or null for any other failure and for what is
/// not a database's error. A program could only match each database's
/// words itself (FEEDBACK5.md, Q-053; branch stays-fixes).
fn error_kind(_it: &mut Interp, args: Vec<Value>, _line: u32) -> R {
    let kind = match args.first() {
        Some(Value::Err(e, _, _)) => match &**e {
            Value::Str(s) => s.strip_prefix("db: ").and_then(kind_of),
            _ => None,
        },
        _ => None,
    };
    Ok(kind.map_or(Value::Nil, Value::str))
}

/// The kind of a database's message (see `error_kind`).
fn kind_of(msg: &str) -> Option<&'static str> {
    let m = msg.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    // PostgreSQL's SQLSTATE, in whatever language its words are (Q-493).
    let sqlstate = m.rsplit_once(" [").and_then(|(_, c)| c.strip_suffix(']')).filter(|c| c.len() == 5);
    if let Some(code) = sqlstate {
        match code {
            "23505" => return Some("unique"),
            "23503" => return Some("foreign_key"),
            "23502" => return Some("not_null"),
            "23514" => return Some("check"),
            "40001" | "40p01" | "55p03" => return Some("conflict"),
            // The server ended the connection (a restart, a failover, an administrator), or
            // it could not be made (FEEDBACK49.md, Q-601; branch main).
            "57p01" | "57p02" | "57p03" => return Some("connection"),
            c if c.starts_with("08") => return Some("connection"),
            _ => {}
        }
    }
    if has(&["terminating connection", "server closed the connection", "connection closed", "connection reset", "broken pipe", "connection refused"]) {
        return Some("connection");
    }
    // MariaDB's own words, for a failed check (4025) and for a row changed since the
    // snapshot (1020), which a transaction retries on (FEEDBACK16.md, Q-122; branch
    // feedback20-fixes).
    if has(&["record has changed since last read", "(1020)"]) {
        return Some("conflict");
    }
    if m.contains("constraint `") && m.contains(" failed for ") || m.contains("(4025)") {
        return Some("check");
    }
    if has(&["unique constraint failed", "violates unique constraint", "duplicate entry", "(1062)"]) {
        Some("unique")
    } else if has(&["foreign key constraint failed", "violates foreign key constraint", "a foreign key constraint fails", "(1451)", "(1452)"]) {
        Some("foreign_key")
    } else if has(&["not null constraint failed", "violates not-null constraint", "cannot be null", "(1048)"]) {
        Some("not_null")
    } else if has(&["check constraint failed", "violates check constraint", "check constraint", "(3819)"]) {
        Some("check")
    } else if has(&["database is locked", "database is busy", "could not serialize access", "deadlock detected", "deadlock found", "lock wait timeout", "(1213)", "(1205)"]) {
        Some("conflict")
    } else {
        None
    }
}

fn connect(path: &str) -> Result<Connection, rusqlite::Error> {
    // A new file is its owner's alone, and so its `-wal` and `-shm`, which SQLite makes with
    // the database's mode: sessions and password hashes were readable by every user of the
    // machine (FEEDBACK51.md, Q-675; branch feedback51-fixes). One there already keeps its own.
    #[cfg(unix)]
    if path != ":memory:" && !path.starts_with("file:") && !Path::new(path).exists() {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path);
    }
    let c = if path == ":memory:" { Connection::open_in_memory()? } else { Connection::open(path)? };
    c.execute_batch("pragma foreign_keys = on")?;
    wal(&c)?;
    // The statements a program makes, compiled once each (see `rows`).
    c.set_prepared_statement_cache_capacity(64);
    // Kept from being written over while it is open (FEEDBACK55.md, Q-702; branch feedback55).
    if path != ":memory:" {
        opened(Path::new(path));
    }
    Ok(c)
}

/// Every SQLite file the program has open (`db.open`, `db.connect`, a tenant's, `$QUILLO_DB`),
/// as it really is, by identity and by its path folded, with how many connections have it: the
/// program writes over none of them and deletes no folder holding one, whichever was opened last
/// and however it is named now, in another case or through a link; one whose connections have
/// all closed (a database `db.open` replaced, a tenant's let go) is a file again. `OPENED` was the
/// last one only. And the targets with `{tenant}` in them, by which a visitor's upload makes no
/// tenant's database, opened or not (FEEDBACK55.md, Q-702 and Q-705; branch feedback55).
#[derive(Default)]
struct Opened {
    files: std::collections::HashMap<std::path::PathBuf, (Option<crate::statics::FileId>, String, usize)>,
    ids: std::collections::HashMap<crate::statics::FileId, usize>,
    folded: std::collections::BTreeMap<String, usize>,
    templates: Vec<(String, String)>,
}

/// The program's (FEEDBACK55.md, Q-702; branch feedback55).
static SQLITE_FILES: std::sync::LazyLock<std::sync::RwLock<Opened>> = std::sync::LazyLock::new(Default::default);

fn opened_files() -> std::sync::RwLockReadGuard<'static, Opened> {
    SQLITE_FILES.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) const OPEN: &str = "it is the database the program has open";

/// One more connection to the SQLite file `p` (FEEDBACK55.md, Q-702; branch feedback55).
fn opened(p: &Path) {
    // Looked at before the lock is taken: no call to the system under it.
    let Ok(real) = std::fs::canonicalize(p) else { return };
    let id = std::fs::metadata(&real).ok().and_then(|m| crate::statics::id_of(&m));
    let folded = crate::builtins::fold_name(&real.to_string_lossy());
    let mut o = SQLITE_FILES.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(e) = o.files.get_mut(&real) {
        e.2 += 1;
        return;
    }
    if let Some(id) = id {
        *o.ids.entry(id).or_default() += 1;
    }
    *o.folded.entry(folded.clone()).or_default() += 1;
    o.files.insert(real, (id, folded, 1));
}

/// One connection fewer: the last closed, the file is no longer the program's to keep
/// (FEEDBACK55.md, Q-702; branch feedback55).
fn closed(p: &Path) {
    let real = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut o = SQLITE_FILES.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(e) = o.files.get_mut(&real) else { return };
    e.2 -= 1;
    if e.2 > 0 {
        return;
    }
    let Some((id, folded, _)) = o.files.remove(&real) else { return };
    if let Some(id) = id {
        if let Some(n) = o.ids.get_mut(&id) {
            *n -= 1;
            if *n == 0 {
                o.ids.remove(&id);
            }
        }
    }
    if let Some(n) = o.folded.get_mut(&folded) {
        *n -= 1;
        if *n == 0 {
            o.folded.remove(&folded);
        }
    }
}

// A database's connection closed, the last of it, lets the file go (FEEDBACK55.md, Q-702;
// branch feedback55).
impl Drop for Backend {
    fn drop(&mut self) {
        if let Backend::Sqlite(m) = self {
            let c = m.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(p) = c.path().filter(|p| !p.is_empty()) {
                closed(Path::new(p));
            }
        }
    }
}

/// Notes a target with `{tenant}` in it, as the folded path before the name and after it
/// (`/app/t/` and `.db`), wherever its folder is made later (FEEDBACK55.md, Q-702; branch
/// feedback55).
fn tenant_template(base: &Path, t: &str) {
    if is_server_url(t) {
        return;
    }
    let full = if Path::new(t).is_absolute() { t.to_string() } else { base.join(t).to_string_lossy().into_owned() };
    let Some((before, after)) = full.split_once("{tenant}") else { return };
    let (dir, lead) = before.rsplit_once(std::path::MAIN_SEPARATOR).or_else(|| before.rsplit_once('/')).unwrap_or((".", before));
    let dir = if dir.is_empty() { std::path::MAIN_SEPARATOR_STR } else { dir };
    let before = crate::builtins::fold_name(&crate::builtins::real_as_made(Path::new(dir)).join(lead).to_string_lossy());
    let t = (before, crate::builtins::fold_name(after));
    let mut o = SQLITE_FILES.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !o.templates.contains(&t) {
        o.templates.push(t);
    }
}

/// Whether the file `id` is a database the program has open (FEEDBACK55.md, Q-701; branch
/// feedback55).
pub(crate) fn open_id(id: crate::statics::FileId) -> bool {
    opened_files().ids.contains_key(&id)
}

/// Whether bytes begin a SQLite database, its write-ahead log or its rollback journal
/// (FEEDBACK55.md, Q-702; branch feedback55).
pub(crate) fn sqlite_bytes(head: &[u8]) -> bool {
    head.starts_with(b"SQLite format 3\0") || head.starts_with(&[0x37, 0x7f, 0x06, 0x82]) || head.starts_with(&[0x37, 0x7f, 0x06, 0x83]) || head.starts_with(&[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7])
}

/// Whether the file at `p` is a SQLite database, by its first bytes (FEEDBACK55.md, Q-702;
/// branch feedback55).
pub(crate) fn sqlite_file(p: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    std::fs::File::open(p).and_then(|f| f.take(16).read_exact(&mut head)).is_ok() && head.starts_with(b"SQLite format 3\0")
}

/// The file whose `-wal`, `-shm` or `-journal` `p` would be, by its name as macOS and Windows
/// take it (`APP.DB-WAL`, `app.db-ſhm`, `app.db-wal.`). Each letter of those endings folds from
/// one letter, so the name without its ending is that many letters fewer (FEEDBACK55.md, Q-702
/// and Q-704; branch feedback55).
pub(crate) fn journal_of(p: &Path) -> Option<std::path::PathBuf> {
    let name = p.file_name()?.to_string_lossy();
    let name = crate::builtins::name_as_taken(&name);
    let low = crate::builtins::fold_name(name);
    let suffix = ["-wal", "-shm", "-journal"].into_iter().find(|s| low.ends_with(s))?;
    let cut = name.char_indices().rev().nth(suffix.len() - 1).map(|(i, _)| i).filter(|&i| i > 0)?;
    Some(p.with_file_name(&name[..cut]))
}

/// Why `p` (a path as `builtins::real_as_made` gives it) is no file for the program to write, if
/// it is a database it has open, or the `-wal`, `-shm` or `-journal` of one or of another SQLite
/// database beside it, by its name in any case; what is a second hard link to one is
/// `builtins::own_id`'s to say (FEEDBACK55.md, Q-702 and Q-704; branch feedback55). A database no
/// connection has open is a file like another. Costs a look in memory while the program has none
/// open, but for a journal's name.
pub(crate) fn database_file(p: &Path) -> Option<&'static str> {
    let journal = journal_of(p);
    if !opened_files().files.is_empty() {
        // Folded and looked at before the lock is taken again: no call to the system under it.
        let low = crate::builtins::fold_name(&p.to_string_lossy());
        let stem = journal.as_deref().map(|j| (crate::builtins::fold_name(&j.to_string_lossy()), std::fs::metadata(j).ok().and_then(|m| crate::statics::id_of(&m))));
        let o = opened_files();
        let ours = |(low, id): &(String, Option<crate::statics::FileId>)| o.folded.contains_key(low) || id.is_some_and(|id| o.ids.contains_key(&id));
        if o.folded.contains_key(&low) || stem.as_ref().is_some_and(ours) {
            return Some(OPEN);
        }
    }
    // Only a SQLite database's: `travel-journal` beside the notes `travel` is a file
    // (FEEDBACK55.md, Q-702; branch feedback55).
    journal.filter(|db| sqlite_file(db)).map(|_| "it is the journal of the SQLite database beside it")
}

/// Whether a database the program has open is in the folder `dir`, folded (FEEDBACK55.md, Q-705;
/// branch feedback55).
pub(crate) fn opened_under(dir: &Path) -> bool {
    let mut prefix = dir.to_string_lossy().into_owned();
    if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
        prefix.push(std::path::MAIN_SEPARATOR);
    }
    opened_files().folded.range(prefix.clone()..).next().is_some_and(|(k, _)| k.starts_with(&prefix))
}

/// Whether `p`, folded, is where a target with `{tenant}` puts a tenant's database (or its
/// journal): one name in the place of `{tenant}` (FEEDBACK55.md, Q-702; branch feedback55).
pub(crate) fn tenant_file(p: &Path) -> bool {
    let o = opened_files();
    if o.templates.is_empty() {
        return false;
    }
    let s = p.to_string_lossy();
    let mut forms = vec![&*s];
    forms.extend(["-wal", "-shm", "-journal"].iter().filter_map(|x| s.strip_suffix(x)));
    o.templates.iter().any(|(before, after)| {
        forms.iter().any(|f| {
            f.len() > before.len() + after.len() && f.starts_with(before.as_str()) && f.ends_with(after.as_str()) && {
                let name = &f[before.len()..f.len() - after.len()];
                !name.contains(['/', std::path::MAIN_SEPARATOR])
            }
        })
    })
}

/// Puts a database in WAL mode, safely when several connections (several
/// servers) open a new file at once. While the file is still empty, SQLite
/// can take one connection's log for a stale one and drop what was
/// committed to it; so a new file first gets its first page, under the
/// write lock, from whichever connection comes first. Then the switch to
/// WAL, which SQLite refuses at once (busy, without waiting) while another
/// connection is reading, is tried again for a few seconds.
fn wal(c: &Connection) -> Result<(), rusqlite::Error> {
    let pages = |c: &Connection| c.query_row("pragma page_count", [], |r| r.get::<_, i64>(0));
    if pages(c)? == 0 {
        c.execute_batch("begin immediate")?;
        let made = match pages(c) {
            Ok(0) => c.execute_batch("create table _quillo_new (x); drop table _quillo_new"),
            other => other.map(|_| ()),
        };
        c.execute_batch(if made.is_ok() { "commit" } else { "rollback" })?;
        made?;
    }
    let start = std::time::Instant::now();
    loop {
        match c.query_row("pragma journal_mode = wal", [], |r| r.get::<_, String>(0)) {
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::DatabaseBusy && start.elapsed() < std::time::Duration::from_secs(5) => {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
            r => return r.map(|_| ()),
        }
    }
}

/// Opens a path (SQLite) or a postgres:// URL. Connecting to Postgres
/// waits on the network, so the interpreter lock is released meanwhile.
/// A MySQL database that is not there is an error, as PostgreSQL's is: a typo in a URL made a
/// new, empty database (FEEDBACK55.md, Q-736; branch feedback55, the user's decision); only a
/// tenant's is made, by its driver (`tenant_backend`).
fn open_backend(it: &mut Interp, target: &str) -> Result<Backend, String> {
    open_backend_as(it, target, true, false)
}

/// `open_backend`; with `make` false, a Postgres schema (`?schema=`) must
/// exist already; with `tenant` (and `make`), MySQL's database is made when not there.
fn open_backend_as(it: &mut Interp, target: &str, make: bool, tenant: bool) -> Result<Backend, String> {
    if is_mysql_url(target) {
        #[cfg(feature = "mysql")]
        {
            let url = target.to_string();
            let make = make && tenant;
            return it.unlocked(move || crate::my::Pool::open_as(&url, make)).map(Backend::Mysql);
        }
        #[cfg(not(feature = "mysql"))]
        return Err("this Quillo was built without MySQL (cargo build --features mysql)".into());
    }
    if pg::is_url(target) {
        let url = target.to_string();
        return it.unlocked(move || pg::Pool::open_as(&url, make)).map(Backend::Postgres);
    }
    let full = if target == ":memory:" || Path::new(target).is_absolute() {
        target.to_string()
    } else {
        it.base_dir.join(target).display().to_string()
    };
    connect(&full).map(|c| Backend::Sqlite(Mutex::new(c))).map_err(|e| e.to_string())
}

/// The database a call is on: a connection's (`db.connect`), or the program's.
pub(crate) fn backend_on(it: &mut Interp, on: Option<usize>) -> Result<Arc<Backend>, String> {
    let Some(i) = on else { return backend(it) };
    let slot = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(i).map(|(_, s)| s.clone());
    match slot {
        Some(DbSlot::One(b)) => Ok(b),
        Some(DbSlot::PerTenant(t)) => tenant_backend(it, &t),
        Some(DbSlot::Shards(_)) => Err("these are shards: `.on(key)` is the one for a key, `.all(sql)` asks every one".into()),
        None => Err("this connection is not one of this program's".into()),
    }
}

/// The slot a call is on, if it has been opened.
/// After a request: a SQLite connection left in a transaction outside `db.transaction` is
/// rolled back, and the log says so. Every later request's writes went into it, and were lost
/// when the server stopped (FEEDBACK44.md, Q-580). No other request runs while a transaction is
/// open (Q-579), so one still open here is this request's.
pub fn left_open(it: &Interp, what: &str) {
    if it.sqlite_tx > 0 || crate::httpd::held() {
        return;
    }
    let mut slots: Vec<DbSlot> = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().cloned().collect();
    slots.extend(it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().map(|(_, s)| s.clone()));
    for slot in slots {
        if let DbSlot::One(b) = slot {
            if let Backend::Sqlite(m) = &*b {
                let c = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if !c.is_autocommit() {
                    let _ = c.execute_batch("rollback");
                    crate::httpd::error_line_with(&format!("{} left a transaction open on SQLite, so it was rolled back: write `db.transaction(() => ...)`", what), &[]);
                }
            }
        }
    }
}

fn slot_on(it: &Interp, on: Option<usize>) -> Option<DbSlot> {
    match on {
        None => it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),
        Some(i) => it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(i).map(|(_, s)| s.clone()),
    }
}

/// The program's database (the tenant's, for one per tenant), opening the
/// default one if needed.
fn backend(it: &mut Interp) -> Result<Arc<Backend>, String> {
    let slot = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if let Some(slot) = slot {
        return backend_in(it, slot);
    }
    let target = std::env::var("QUILLO_DB").unwrap_or_else(|_| ":memory:".into());
    if target.contains("{tenant}") {
        tenant_template(&it.base_dir, &target);
        let t = Arc::new(TenantDbs::new(Target::Text(target)));
        *it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(DbSlot::PerTenant(t.clone()));
        return tenant_backend(it, &t);
    }
    let b = Arc::new(open_backend(it, &target)?);
    // Opening is done outside the lock, so another request's `db.open` may have got here
    // first, with a database per tenant: that is the program's, and this one is let go
    // (FEEDBACK19.md, Q-159; branch feedback20-fixes).
    let slot = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert(DbSlot::One(b)).clone();
    backend_in(it, slot)
}

/// The database a slot of the program's gives.
fn backend_in(it: &mut Interp, slot: DbSlot) -> Result<Arc<Backend>, String> {
    match slot {
        DbSlot::One(b) => Ok(b),
        DbSlot::PerTenant(t) => tenant_backend(it, &t),
        DbSlot::Shards(_) => Err("the program's database cannot be shards".into()),
    }
}

/// The current tenant's database, opened (and migrated) on first use.
fn tenant_backend(it: &mut Interp, dbs: &Arc<TenantDbs>) -> Result<Arc<Backend>, String> {
    let Some(name) = it.tenant() else {
        return Err("the database is one per tenant ({tenant} in its target), and this code runs for none: see tenant_from and with_tenant".into());
    };
    {
        let mut open = dbs.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(i) = open.get_index_of(&*name) {
            // The most recently used go last.
            let last = open.len() - 1;
            open.move_index(i, last);
            return Ok(open[last].clone());
        }
    }
    let no_such = |it: &mut Interp| {
        // Asked for by the host, so not the program's mistake: see `Interp::handle`
        // (FEEDBACK20.md, Q-167; branch feedback20-fixes).
        it.no_such_tenant = true;
        format!("there is no tenant {:?}: new_tenant({:?}, ...) makes its database", &*name, &*name)
    };
    let target = match tenant_target_or_none(it, dbs, &name)? {
        Some(t) => t,
        None if it.tenant_named => return Err(format!("tenant {}: the function for tenants' databases gave null, not a path or a URL", name)),
        None => return Err(no_such(it)),
    };
    // A request's host can name any tenant, so only the program makes one
    // (`new_tenant`, when signing a tenant up): made-up names make nothing.
    let make = it.tenant_named;
    if !is_server_url(&target) && target != ":memory:" {
        let full = if Path::new(&target).is_absolute() { Path::new(&target).to_path_buf() } else { it.base_dir.join(&target) };
        if !make && !full.exists() {
            return Err(no_such(it));
        }
        if let Some(dir) = full.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    // A server that is down stays an error; only a schema or a database that is not there
    // means no such tenant.
    let b = match open_backend_as(it, &target, make, true) {
        Err(e) if !make && (e.contains("there is no schema") || e.contains("Unknown database") || (e.contains("database") && e.contains("does not exist"))) => {
            return Err(no_such(it));
        }
        b => Arc::new(b.map_err(|e| format!("tenant {}: {}", name, e))?),
    };
    let is_default = matches!(&*it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner), Some(DbSlot::PerTenant(t)) if Arc::ptr_eq(t, dbs));
    let dir = dbs.migrations.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone().or_else(|| if is_default { it.site.migrations.clone() } else { None });
    if let Some(dir) = dir {
        let mb = b.clone();
        let applied = it.unlocked(move || crate::migrate::run(&mb, &dir)).map_err(|e| format!("tenant {}: {}", name, e))?;
        for m in applied {
            eprintln!("quillo: applied migration {} for tenant {}", m, name);
        }
    }
    let mut open = dbs.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Each open server tenant keeps connections (a few each).
    let most = if is_server_url(&target) { dbs.most_open.unwrap_or(OPEN_PG_TENANTS) } else { OPEN_TENANTS };
    while open.len() >= most {
        open.shift_remove_index(0);
    }
    Ok(open.entry(name.to_string()).or_insert(b).clone())
}

/// Makes tenant `name`'s database, and says false when there is one already, in one step
/// on each kind of database, so that of two signups for one name only one makes it
/// (FEEDBACK20.md, Q-165; branch feedback20-fixes).
pub(crate) fn claim_tenant(it: &mut Interp, name: &str, line: u32) -> Result<bool, Ctrl> {
    let dbs = per_tenant(it, None, "new_tenant", line)?;
    let target = match tenant_target_or_none(it, &dbs, name) {
        Ok(Some(t)) => t,
        Ok(None) => return fail(line, format!("`new_tenant`: the function for tenants' databases gave null for {:?}", name)),
        Err(e) => return fail(line, format!("`new_tenant`: {}", e)),
    };
    let made = if pg::is_url(&target) {
        it.unlocked(move || pg::Pool::claim(&target))
    } else if is_mysql_url(&target) {
        #[cfg(feature = "mysql")]
        let r = it.unlocked(move || crate::my::Pool::claim(&target));
        #[cfg(not(feature = "mysql"))]
        let r = Err("this Quillo was built without MySQL (cargo build --features mysql)".to_string());
        r
    } else if target == ":memory:" {
        Ok(true)
    } else {
        let full = if Path::new(&target).is_absolute() { Path::new(&target).to_path_buf() } else { it.base_dir.join(&target) };
        if let Some(dir) = full.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&full) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(format!("{}: {}", full.display(), e)),
        }
    };
    made.or_else(|e| fail(line, format!("`new_tenant`: {}", e)))
}

/// `tenant_target`; None when the function gives null (no such tenant).
fn tenant_target_or_none(it: &mut Interp, dbs: &Arc<TenantDbs>, name: &str) -> Result<Option<String>, String> {
    let f = match &dbs.target {
        Target::Text(t) => return Ok(Some(t.replace("{tenant}", name))),
        Target::Fn(f) => f.clone(),
    };
    let key = Arc::as_ptr(dbs) as usize;
    if it.db_resolving.contains(&key) {
        let why = format!("tenant {}: the function that says where a tenant's database is cannot use that database itself; keep the list in another (db.connect)", name);
        it.db_loop = Some(why.clone());
        return Err(why);
    }
    it.db_resolving.push(key);
    let r = it.call(&f, vec![Value::str(name)], 0);
    it.db_resolving.retain(|k| *k != key);
    if let Some(why) = it.db_loop.take() {
        return Err(why);
    }
    match r {
        Ok(Value::Str(s)) | Err(Ctrl::Return(Value::Str(s))) => Ok(Some(s.to_string())),
        Ok(Value::Nil) | Err(Ctrl::Return(Value::Nil)) => Ok(None),
        Ok(Value::Err(e, _, _)) => Err(format!("tenant {}: {}", name, e.display())),
        Ok(other) => Err(format!("tenant {}: the function for tenants' databases gave {}, not a path or a URL", name, other.type_name())),
        Err(Ctrl::Error(e)) => Err(format!("tenant {}: {}", name, e.msg)),
        Err(_) => Err(format!("tenant {}: the function for tenants' databases did not give a path or a URL", name)),
    }
}

/// A connection of its own for a transaction. In a request, while none is
/// free, the request waits without holding one of the server's waiting
/// threads: those also run the queries and commits of the transactions
/// that hold the connections, so waiting there could stop them all.
pub(crate) fn checkout_for_tx(it: &mut Interp, b: &Arc<Backend>) -> Result<Pinned, String> {
    let Backend::Postgres(pool) = &**b else {
        let b = b.clone();
        return it.unlocked(move || b.checkout());
    };
    let until = std::time::Instant::now() + crate::pg::CHECKOUT_WAIT;
    let mut pause = std::time::Duration::from_millis(2);
    loop {
        let p = b.clone();
        let taken = it.unlocked(move || match &*p {
            Backend::Postgres(pool) => pool.try_checkout(),
            _ => unreachable!(),
        })?;
        if let Some(c) = taken {
            return Ok(Pinned::Pg(c));
        }
        // Inside a SQLite transaction nothing else runs, the requests that would give a
        // connection back included: waiting would hold the whole server (found in review).
        if crate::httpd::held() {
            return Err(held_none_free());
        }
        if std::time::Instant::now() >= until {
            return Err(pool.none_free());
        }
        // Made inside the future: a timer belongs to the runtime that runs it.
        // Outside a request the lock is let go for it: whoever holds the connection needs
        // it to finish (FEEDBACK16.md, Q-134; branch feedback20-fixes).
        if crate::httpd::await_elsewhere(Box::pin(async move { tokio::time::sleep(pause).await })).is_none() {
            it.unlocked(move || std::thread::sleep(pause));
        }
        pause = (pause * 2).min(std::time::Duration::from_millis(50));
    }
}

/// Runs `f` on a server database's connection without holding the
/// interpreter lock: the request's transaction connection if it has one,
/// else one from the pool.
pub(crate) fn pooled_run<T: Send>(
    it: &mut Interp,
    b: &Arc<Backend>,
    f: impl FnOnce(&mut Pinned) -> Result<T, String> + Send,
) -> Result<T, String> {
    let pool = &**b;
    let pinned = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter_mut().find(|tx| Arc::ptr_eq(&tx.backend, b)).and_then(|tx| tx.conn.take());
    match pinned {
        Some(mut c) => {
            let (c, r) = it.unlocked(move || {
                let r = f(&mut c);
                (c, r)
            });
            if let Some(tx) = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter_mut().find(|tx| Arc::ptr_eq(&tx.backend, b)) {
                tx.conn = Some(c);
            }
            r
        }
        None => {
            let held = crate::httpd::held();
            it.unlocked(move || {
                let mut c = match (held, pool) {
                    // As above: in a SQLite transaction, one free now or none.
                    (true, Backend::Postgres(p)) => Pinned::Pg(p.try_checkout()?.ok_or_else(held_none_free)?),
                    _ => pool.checkout()?,
                };
                let r = f(&mut c);
                pool.give_back(c);
                r
            })
        }
    }
}

/// Why a PostgreSQL query in a SQLite transaction found no connection.
fn held_none_free() -> String {
    "no PostgreSQL connection is free, and this request is inside a SQLite transaction, which holds up every other request, those that would give one back too: take the PostgreSQL work out of the SQLite transaction, or allow more with `?pool_size=N`".to_string()
}

/// In the server, outside a transaction, queries use the connections all
/// requests share (pipelined; see `pg::Lanes`). `None`: use `pg_run`.
fn shared_lanes(it: &Interp, b: &Arc<Backend>, pool: &pg::Pool) -> Option<Arc<pg::Lanes>> {
    let pinned = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|t| Arc::ptr_eq(&t.backend, b));
    // Inside a SQLite transaction the lanes would pause the request, and others would run
    // inside it: the pool waits in place instead (FEEDBACK44.md, Q-579; branch feedback40-43).
    (!pinned && crate::httpd::in_task() && !crate::httpd::held() && it.sqlite_tx == 0).then(|| pool.lanes())
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
        // -0.0 as 0.0: SQLite keeps a whole real as an integer, and MariaDB reads 0, so -0.0
        // came back 0.0 there and -0 from PostgreSQL; every database is given 0.0
        // (FEEDBACK55.md, Q-735; branch feedback55).
        Value::Float(f) => Sql::Real(*f + 0.0),
        // SQLite has no decimals: the text, exactly (see `Col::Dec`).
        Value::Dec(d) => Sql::Text(d.to_string()),
        Value::Str(s) => Sql::Text(s.to_string()),
        Value::Html(h) => Sql::Text(h.to_string()),
        other => Sql::Text(json_text(other)),
    }
}

/// What a SQLite column's declared type says about reading it.
#[derive(Clone, Copy, PartialEq)]
enum Col {
    Plain,
    /// `boolean` or `bool`.
    Bool,
    /// `decimal(12,2)`, `decimal_text`, `money`, `numeric(12,2)`: read as
    /// decimals. A bare `numeric` is SQLite's name for any number, and
    /// stays one.
    Dec,
}

fn col_kind(decl: Option<&str>) -> Col {
    let t = decl.unwrap_or("").trim().to_ascii_lowercase();
    if t.starts_with("bool") {
        Col::Bool
    } else if ["decimal", "money", "dec(", "numeric("].iter().any(|p| t.starts_with(p)) || t == "dec" {
        Col::Dec
    } else {
        Col::Plain
    }
}

/// SQLite has no booleans: `true` is stored as 1. In a column declared
/// `boolean` (or `bool`) 0 and 1 come back as `false` and `true`, since 0
/// would be true in Quillo; elsewhere, such as `select x = 1`, they are ints.
///
/// Nor has it decimals. A column declared `decimal(12,2)` (or
/// `numeric(12,2)`, `money`) comes back as decimals, from what SQLite kept: text exactly, an
/// integer, or a real through its shortest text. Such a column has NUMERIC
/// affinity, so SQLite turns "0.10" into the real 0.1 (and more than 15
/// digits are rounded); one declared `decimal_text` has TEXT affinity and
/// keeps the digits as written.
fn from_sql(v: ValueRef, kind: Col) -> Value {
    match v {
        ValueRef::Null => Value::Nil,
        ValueRef::Integer(n @ (0 | 1)) if kind == Col::Bool => Value::Bool(n == 1),
        ValueRef::Integer(n) if kind == Col::Dec => Value::Dec(crate::decimal::Dec::from_i64(n)),
        ValueRef::Real(f) if kind == Col::Dec => crate::decimal::Dec::from_f64(f).map_or(Value::Float(f), Value::Dec),
        ValueRef::Text(t) if kind == Col::Dec => {
            let s = String::from_utf8_lossy(t);
            crate::decimal::Dec::parse(s.trim()).map_or_else(|| Value::str(&*s), Value::Dec)
        }
        ValueRef::Integer(n) => Value::Int(n),
        // -0 as 0.0, as every database is given (FEEDBACK55.md, Q-735; branch feedback55).
        ValueRef::Real(f) => Value::Float(f + 0.0),
        ValueRef::Text(t) => Value::str(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => Value::str(String::from_utf8_lossy(b)),
    }
}

/// Binds `?` parameters from the arguments, or `:name` parameters from a
/// single map argument. As with Postgres and MySQL, every name needs a
/// value; keys the query does not use are left alone, so one map can serve
/// queries put together from optional parts. A map is the values by name only where the SQL
/// names them (`?1` is a number: FEEDBACK55.md, Q-748), and is no value where the SQL has no
/// placeholder (Q-747; branch feedback55); a key is a name with any of SQLite's signs
/// (`:id`, `@id`, `$id`, `#id`), or as written.
fn bind(stmt: &mut rusqlite::Statement, params: &[Value]) -> Result<(), String> {
    let want = stmt.parameter_count();
    if let [Value::Map(m)] = params {
        fn named(n: Option<&str>) -> Option<&str> {
            n.filter(|n| n.starts_with([':', '@', '$', '#']))
        }
        if want == 0 {
            return Ok(());
        }
        if (1..=want).any(|i| named(stmt.parameter_name(i)).is_some()) {
            for i in 1..=want {
                let name = stmt.parameter_name(i).map(str::to_string);
                let v = named(name.as_deref()).and_then(|n| m.get(&n[1..]).or_else(|| m.get(n)));
                match v {
                    Some(v) => stmt.raw_bind_parameter(i, to_sql(v)).map_err(|e| e.to_string())?,
                    None => return Err(format!("no value for {}", name.as_deref().unwrap_or("?"))),
                }
            }
            return Ok(());
        }
    }
    if params.len() != want {
        return Err(crate::sql::wrong_count(want, params.len()));
    }
    for (i, v) in params.iter().enumerate() {
        stmt.raw_bind_parameter(i + 1, to_sql(v)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn rows(conn: &Connection, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
    // Compiled once and kept, as a statement made again and again is (a row read by its key per
    // request, an insert in a loop): faster than Python's sqlite3 there too (branch
    // feedback40-43).
    let mut stmt = conn.prepare_cached(sql).map_err(|e| e.to_string())?;
    bind(&mut stmt, params)?;
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    // Its columns as of its first row: a statement kept from before the table changed is made
    // again by SQLite as it runs, and its columns before that are the old table's.
    let mut cols: Option<Vec<(String, Col)>> = None;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let cols = cols.get_or_insert_with(|| {
            let st: &rusqlite::Statement = row.as_ref();
            st.columns().iter().map(|c| (c.name().to_string(), col_kind(c.decl_type()))).collect()
        });
        let mut m = Map::new();
        for (i, (c, kind)) in cols.iter().enumerate() {
            m.insert(c.into(), from_sql(row.get_ref(i).map_err(|e| e.to_string())?, *kind));
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
/// A connection's tables are topics of their own (`db@<label>:orders`), so
/// that one `orders` changing does not refresh pages showing another.
pub(crate) fn announce(it: &mut Interp, scan: &crate::sql::Scan, on: Option<usize>) {
    announce_as(it, scan, on, true)
}

/// As `announce`, the writes left out when the statement changed nothing. The tables as the
/// statement's database reads them (src/sql.rs; FEEDBACK55.md; branch feedback55).
fn announce_as(it: &mut Interp, scan: &crate::sql::Scan, on: Option<usize>, wrote: bool) {
    let t = &scan.tables;
    let label = on.and_then(|i| it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(i).map(|(l, _)| l.split('#').next().unwrap_or(l).to_string()));
    let prefix = match label {
        Some(label) => format!("db@{}:", label),
        None => "db:".to_string(),
    };
    if it.reads.is_some() {
        for r in &t.reads {
            it.watch(format!("{}{}", prefix, r));
        }
    }
    if wrote && !t.writes.is_empty() {
        it.publish(t.writes.iter().map(|w| format!("{}{}", prefix, w)).collect());
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
        // A global keeps what the transaction gave it, so its announcement stays, made when the
        // outermost one ends: pages showed the old value for good (FEEDBACK55.md, Q-797; branch
        // feedback55).
        let mut kept = mark;
        for i in mark..it.pending.len() {
            if it.pending_raw[i].starts_with("var:") {
                it.pending.swap(kept, i);
                it.pending_raw.swap(kept, i);
                kept += 1;
            }
        }
        it.pending.truncate(kept);
        it.pending_raw.truncate(kept);
    }
    it.tx_depth -= 1;
    if it.tx_depth == 0 {
        let topics = std::mem::take(&mut it.pending);
        let seq = it.shared.publish(&topics);
        // A live loader's own write (FEEDBACK13.md, Q-090; branch feedback13-fixes).
        if it.reads.is_some() && seq > 0 {
            it.own_seqs.push(seq);
        }
        // What a service sends back with its answer: only what committed
        // (FEEDBACK4.md, Q-038; branch bank-fixes).
        let raw = std::mem::take(&mut it.pending_raw);
        it.note_published(raw);
    }
}

/// A transaction begun or ended by hand, one statement alone: a `begin` whose `commit` was
/// never reached left the shared connection in it, and every later request's writes with it
/// (FEEDBACK44.md, Q-580; branch feedback40-43). A script of several statements may hold one
/// of its own, and is checked for leaving it open where it runs. The statements are the
/// database's own (a function's body is one: FEEDBACK55.md, Q-746; branch feedback55).
fn no_tx_control(scan: &crate::sql::Scan, f: &str, line: u32) -> Result<(), Ctrl> {
    // A query has no use for one anywhere.
    if scan.tx_control(f != "db.exec") {
        return fail(line, format!(
            "`{}(\"{}\")`: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it",
            f,
            scan.sql.trim()
        ));
    }
    Ok(())
}

fn sql_arg(args: &[Value], name: &str, line: u32) -> Result<String, Ctrl> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.to_string()),
        other => fail(line, format!("`{}` expects SQL text as its first argument, got {}", name, other.map_or("nothing", |v| v.type_name()))),
    }
}

/// `db.open(target)`: a SQLite file next to the program, ":memory:", or a
/// postgres:// URL.
/// The program's database per tenant, or an error naming `f`.
fn per_tenant(it: &Interp, on: Option<usize>, f: &str, line: u32) -> Result<Arc<TenantDbs>, Ctrl> {
    match slot_on(it, on) {
        Some(DbSlot::PerTenant(t)) => Ok(t),
        _ => fail(line, format!("`{}` needs a database per tenant: db.open(\"data/{{tenant}}.db\")", f)),
    }
}

/// `db.tenant_exists(name)`: whether the tenant has a database (made by
/// `with_tenant`), without making one.
fn tenant_exists(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let dbs = per_tenant(it, on, "db.tenant_exists", line)?;
    let name = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`db.tenant_exists` expects a name, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    if !crate::site::tenant_name_ok(&name) {
        return Ok(Value::Bool(false));
    }
    if dbs.open.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains_key(&*name) {
        return Ok(Value::Bool(true));
    }
    let target = match tenant_target_or_none(it, &dbs, &name) {
        Ok(Some(t)) => t,
        Ok(None) => return Ok(Value::Bool(false)),
        Err(e) => return Ok(db_err(e)),
    };
    if pg::is_url(&target) {
        return Ok(Value::Bool(it.unlocked(move || pg::Pool::open_as(&target, false).is_ok())));
    }
    #[cfg(feature = "mysql")]
    if is_mysql_url(&target) {
        return Ok(Value::Bool(it.unlocked(move || crate::my::Pool::open_as(&target, false).is_ok())));
    }
    let full = if Path::new(&target).is_absolute() { Path::new(&target).to_path_buf() } else { it.base_dir.join(&target) };
    Ok(Value::Bool(full.is_file()))
}

/// `db.tenants()`: the names of the tenants that have a database, sorted
/// (for a job that does something for each). SQLite files, and PostgreSQL
/// schemas (`?schema=t_{tenant}`).
fn tenants(it: &mut Interp, _: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let dbs = per_tenant(it, on, "db.tenants", line)?;
    let Some(template) = dbs.template().map(str::to_string) else {
        return fail(line, "`db.tenants` lists the databases a template names (data/{tenant}.db); where a function says where they are, keep the list of tenants yourself");
    };
    let (before, after) = template.split_once("{tenant}").map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    if is_mysql_url(&template) {
        // Databases named around {tenant}: `app_{tenant}` in mysql://host/app_{tenant}.
        #[cfg(feature = "mysql")]
        {
            let (path, query) = template.split_once('?').unwrap_or((&template, ""));
            let (server, db) = path.rsplit_once('/').unwrap_or((path, ""));
            let (pre, post) = db.split_once("{tenant}").map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
            // The URL's options (TLS's among them) are the server's too.
            let server = if query.is_empty() { format!("{}/", server) } else { format!("{}/?{}", server, query) };
            match it.unlocked(move || crate::my::Pool::open_as(&server, false).and_then(|p| p.databases())) {
                Ok(all) => names.extend(all.iter().filter_map(|d| d.strip_prefix(&pre).and_then(|r| r.strip_suffix(&post)).map(str::to_string))),
                Err(e) => return Ok(db_err(e)),
            }
        }
    } else if pg::is_url(&template) {
        // The schema's name around {tenant}: `t_{tenant}` in `?schema=t_{tenant}&...`.
        let Some(q) = before.rsplit_once("schema=").map(|(_, p)| p.to_string()) else {
            return fail(line, "`db.tenants` lists PostgreSQL schemas (?schema=t_{tenant}), not databases");
        };
        let suffix: String = after.chars().take_while(|c| *c != '&').collect();
        // The URL without the schema: its other options (TLS's among them) kept.
        let base = match template.split_once('?') {
            Some((base, query)) => {
                let rest: Vec<&str> = query.split('&').filter(|p| !p.starts_with("schema=")).collect();
                if rest.is_empty() { base.to_string() } else { format!("{}?{}", base, rest.join("&")) }
            }
            None => template.clone(),
        };
        let found = it.unlocked(move || -> Result<Vec<String>, String> {
            let pool = pg::Pool::open(&base)?;
            let mut c = pool.checkout()?;
            let rows = c.query("select nspname from pg_namespace order by 1", &[], None);
            pool.give_back(c);
            Ok(rows?
                .iter()
                .filter_map(|r| match r {
                    Value::Map(m) => m.get("nspname").map(|v| v.display().to_string()),
                    _ => None,
                })
                .collect())
        });
        match found {
            Ok(all) => {
                for s in all {
                    if let Some(n) = s.strip_prefix(&q).and_then(|r| r.strip_suffix(&suffix)) {
                        names.push(n.to_string());
                    }
                }
            }
            Err(e) => return Ok(db_err(e)),
        }
    } else {
        let full = it.base_dir.join(&before);
        let (dir, prefix) = if before.ends_with('/') || before.is_empty() {
            (full.clone(), String::new())
        } else {
            (full.parent().map(Path::to_path_buf).unwrap_or_default(), full.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
        };
        let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(Value::list(Vec::new())) };
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = file.strip_prefix(&prefix) else { continue };
            if after.contains('/') {
                // A folder per tenant (`data/{tenant}/app.db`): its database inside.
                if dir.join(format!("{}{}", file, after)).is_file() {
                    names.push(rest.to_string());
                }
            } else if let Some(name) = rest.strip_suffix(&after) {
                if e.path().is_file() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.retain(|n| crate::site::tenant_name_ok(n));
    names.sort();
    Ok(Value::list(names.into_iter().map(Value::str).collect()))
}

/// A database slot for `db.open`'s or `db.connect`'s target: one per
/// tenant (a `{tenant}` template, or a function), or one opened now.
fn slot_for(it: &mut Interp, args: &[Value], f: &str, line: u32) -> Result<Result<DbSlot, String>, Ctrl> {
    // `{ replicas: [...] }`: read replicas of a database (one, not one per tenant).
    if let Some(opts) = args.get(1) {
        let Value::Map(o) = opts else {
            return fail(line, format!("`{}`'s options are a map, like {{ replicas: [url, ...] }}", f));
        };
        if let Some(k) = o.keys().find(|k| k.as_str() != "replicas") {
            return fail(line, format!("`{}` has no option `{}` (replicas)", f, k.as_str()));
        }
        let urls: Vec<String> = match o.get("replicas") {
            Some(Value::List(l)) if l.iter().all(|u| matches!(u, Value::Str(_))) => l.iter().map(|u| u.display()).collect(),
            Some(Value::Nil) | None => Vec::new(),
            Some(other) => return fail(line, format!("`replicas` is a list of database URLs, got {}", other.type_name())),
        };
        let primary = match args.first() {
            Some(Value::Str(t)) if !t.contains("{tenant}") => t.to_string(),
            _ => return fail(line, format!("`{}`: replicas are of one database, given by its URL", f)),
        };
        if !urls.is_empty() && !is_server_url(&primary) {
            return fail(line, format!("`{}`: replicas are of a PostgreSQL or MySQL database, not SQLite", f));
        }
        let b = match open_backend(it, &primary) {
            Ok(b) => Arc::new(b),
            Err(e) => return Ok(Err(e)),
        };
        let mut list = Vec::new();
        for u in &urls {
            match open_backend(it, u) {
                Ok(r) => list.push(Arc::new(r)),
                // Named without its user and password, as `db.connect`'s errors are: the URL
                // went to the log whole (FEEDBACK51.md, Q-659; branch feedback51-fixes).
                Err(e) => return Ok(Err(format!("replica {}: {}", target_label(&it.base_dir, u), e))),
            }
        }
        if !list.is_empty() {
            let n = list.len();
            let reps = Arc::new(Replicas { list, next: Default::default(), down: Mutex::new(vec![None; n]) });
            replica_sets().lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((Arc::downgrade(&b), reps));
        }
        return Ok(Ok(DbSlot::One(b)));
    }
    match args.first() {
        Some(Value::Str(t)) if t.contains("{tenant}") => {
            tenant_template(&it.base_dir, t);
            Ok(Ok(DbSlot::PerTenant(Arc::new(TenantDbs::new(Target::Text(t.to_string()))))))
        }
        Some(Value::Str(t)) => Ok(open_backend(it, t).map(|b| DbSlot::One(Arc::new(b)))),
        // Where each tenant's database is, opened when its tenant first uses it.
        Some(fv @ (Value::Func(_) | Value::Builtin(..))) => Ok(Ok(DbSlot::PerTenant(Arc::new(TenantDbs::new(Target::Fn(fv.clone())))))),
        other => fail(
            line,
            format!("`{}` expects a path, a URL, or a function giving one for each tenant, got {}", f, other.map_or("nothing", |v| v.type_name())),
        ),
    }
}

/// The SQLite file the program opened last, for `quillo run` to say which
/// it is: `db.open("app.db")` is next to the program, wherever Quillo runs.
pub static OPENED: Mutex<Option<String>> = Mutex::new(None);

fn open(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match slot_for(it, &args, "db.open", line)? {
        Ok(slot) => {
            let before = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).replace(slot);
            // Another database: pages showing the old one's rows ask again, and what was shared
            // from it is not given out (FEEDBACK43.md, Q-490; branch feedback40-43).
            if before.is_some() {
                // Every tenant's: the slot is the program's, whichever tenant's request opened it.
                let base = if it.base.is_empty() { String::new() } else { format!("{}|", it.base) };
                let topics = vec![format!("{}db:*", base), format!("{}*@db:*", base)];
                // At once: a transaction rolled back does not undo the switch.
                it.shared.publish(&topics);
            }
            if let Some(Value::Str(t)) = args.first() {
                if !is_server_url(t) && &**t != ":memory:" && !t.contains("{tenant}") {
                    let full = if Path::new(&**t).is_absolute() { Path::new(&**t).to_path_buf() } else { it.base_dir.join(&**t) };
                    let full = std::fs::canonicalize(&full).unwrap_or(full);
                    *OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(full.display().to_string());
                }
            }
            Ok(Value::Nil)
        }
        // As the program starts, one whose error nothing takes stops it: see `Interp::eval_unused`
        // (FEEDBACK49.md, Q-602).
        Err(e) => {
            let v = db_err(e);
            if crate::value::loading() {
                if let Value::Err(inner, ..) = &v {
                    OPEN_FAILED.with(|f| *f.borrow_mut() = Arc::downgrade(inner));
                }
            }
            Ok(v)
        }
    }
}

thread_local! {
    /// The error value of the last `db.open` that failed as the program loaded (Q-602), held
    /// weakly: its address is not given to another value while this is held, where an error
    /// made later in the same place was taken for it (found in review).
    static OPEN_FAILED: std::cell::RefCell<std::sync::Weak<Value>> = std::cell::RefCell::new(std::sync::Weak::new());
}

/// Whether `inner` is the error a `db.open` gave as the program loaded.
pub fn open_failed(inner: &Arc<Value>) -> bool {
    OPEN_FAILED.with(|f| f.borrow().upgrade().is_some_and(|e| Arc::ptr_eq(&e, inner)))
}

/// `db.connect(target)`: another database, as a map of the same functions
/// as `db` (`analytics.query(...)`). The same target again (from a route,
/// say) is the same connection, not a new pool.
fn db_connect(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let label = match args.first() {
        // A label for its live updates' topics, the same on every server (and no secret in it).
        Some(Value::Str(t)) => {
            let hash = |x: &str| crate::crypto::hex(&<sha2::Sha256 as sha2::Digest>::digest(x.as_bytes()))[..12].to_string();
            let topics = hash(&target_label(&it.base_dir, t));
            // A server's connection is the same one only for the same URL: another user or other
            // options is another pool, though its tables are the same topics. What follows `#`
            // is not part of the topics (see `announce_as`).
            if is_server_url(t) { format!("{}#{}", topics, hash(t)) } else { topics }
        }
        _ => format!("fn{}", it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()),
    };
    if matches!(args.first(), Some(Value::Str(_))) {
        if let Some(i) = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().position(|(l, _)| *l == label) {
            return Ok(connection(i));
        }
    }
    let slot = match slot_for(it, &args, "db.connect", line)? {
        Ok(s) => s,
        Err(e) => return Ok(db_err(e)),
    };
    let mut conns = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    conns.push((label, slot));
    Ok(connection(conns.len() - 1))
}

/// What names a database, whatever the spelling of its target: a SQLite file by its real path
/// (from the program's folder when inside it, so servers deployed to other folders agree), a
/// server by its scheme, host, port and database, without the credentials or options, which
/// change during a rotation (FEEDBACK43.md, Q-491; branch feedback40-43).
pub fn target_label(base: &Path, t: &str) -> String {
    if is_server_url(t) {
        let (scheme, rest) = t.split_once("://").unwrap_or(("", t));
        let scheme = if scheme == "postgresql" { "postgres" } else { scheme };
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h).to_ascii_lowercase();
        return format!("{}://{}/{}", scheme, host, path);
    }
    if t == ":memory:" || t.contains("{tenant}") {
        return t.to_string();
    }
    let full = if Path::new(t).is_absolute() { Path::new(t).to_path_buf() } else { base.join(t) };
    // A file not made yet: the real path of the nearest folder above it that is there, and the
    // rest as written. Its folder's alone gave an absolute label where /tmp is a link (macOS)
    // and a relative one where it is not (found by CI on Linux).
    let real = full
        .ancestors()
        .find_map(|at| {
            let r = std::fs::canonicalize(at).ok()?;
            let rest = full.strip_prefix(at).ok()?;
            Some(if rest.as_os_str().is_empty() { r } else { r.join(rest) })
        })
        .unwrap_or_else(|| full.clone());
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    match real.strip_prefix(&base) {
        Ok(inside) => inside.display().to_string(),
        Err(_) => real.display().to_string(),
    }
}

/// A connection as programs see it: `db`'s functions, and its number.
fn connection(i: usize) -> Value {
    let mut m = Map::new();
    m.insert(CONN_KEY.into(), Value::Int(i as i64));
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("db.exec", exec),
        ("db.exec_many", exec_many),
        ("db.query", query),
        ("db.one", one),
        ("db.transaction", transaction),
        ("db.migrate", migrate),
        ("db.tenants", tenants),
        ("db.tenant_exists", tenant_exists),
        ("db.sql_name", sql_name_on),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("db.").into(), Value::Builtin(name, *f));
    }
    Value::map(m)
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
    let on = it.db_on.take();
    // Shards: each brought up to date; the names applied to each.
    if let Some(DbSlot::Shards(list)) = slot_on(it, on) {
        let mut out = Vec::new();
        for i in list.iter() {
            it.db_on = Some(*i);
            out.push(migrate(it, args.clone(), line)?);
        }
        return Ok(Value::list(out));
    }
    let full = it.base_dir.join(&dir);
    if on.is_none() {
        std::sync::Arc::make_mut(&mut it.site).migrations = Some(full.clone());
    }
    // One database per tenant: each is migrated when first opened.
    if let Some(DbSlot::PerTenant(t)) = slot_on(it, on) {
        *t.migrations.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(full.clone());
        if it.tenant().is_none() {
            return Ok(Value::list(Vec::new()));
        }
    }
    let b = match backend_on(it, on) {
        Ok(b) => b,
        Err(e) => return fail(line, format!("db.migrate: {}", e)),
    };
    let to = full.clone();
    match it.unlocked(move || crate::migrate::run(&b, &to)) {
        Ok(applied) => {
            for name in &applied {
                eprintln!("quillo: applied migration {}", name);
            }
            Ok(Value::list(applied.into_iter().map(Value::str).collect()))
        }
        Err(e) => fail(line, format!("db.migrate: {}", e)),
    }
}

/// The columns a `create table` or `alter table` gives SQLite as `money`, `decimal` or
/// `numeric(p, s)`: read back as decimals, but kept with NUMERIC affinity, as a float past
/// 15 digits and without the scale. Only the guide said so; the log says it once for each
/// column (FEEDBACK50.md, Q-628; branch feedback50-fixes), 64 at most. Read by SQLite's
/// tokenizer (src/sql.rs), where a regex looked past comments and strings (FEEDBACK55.md;
/// branch feedback55).
pub(crate) fn warn_lossy_columns(sql: &str) {
    static SAID: std::sync::OnceLock<Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    let scan = crate::sql::scan(crate::sql::Dialect::Sqlite, sql);
    for (table, col, ty) in &scan.lossy {
        let key = format!("{}.{}", table, col);
        let mut said = SAID.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if said.len() < 64 && said.insert(key.clone()) {
            eprintln!(
                "quillo: warning: SQLite keeps `{}` ({}) as a number of 15 digits: 12345678901234567.89 comes back as 12345678901234568, and 0.10 as 0.1. Declare it `decimal_text`, or keep minor units in an `integer` column (money.to_minor)",
                key, ty
            );
        }
    }
}

/// `db.exec(sql, ...params)`: `{ changes, last_id }`. Without parameters the
/// text may hold several statements (a schema, for example).
fn exec(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    exec_as(it, args, line, false)
}

/// SQL with no statement in it, refused on every database: SQLite gave the statement before's
/// `changes`, and `db.one` an error saying "not an error", PostgreSQL null (FEEDBACK55.md, Q-753;
/// branch feedback55).
const NO_STATEMENT: &str = "the SQL has no statement in it (it is empty, or only comments)";

/// `db.exec` on SQLite: `{ changes, last_id }` of the statement, the last of a script, as the
/// other databases count them (FEEDBACK55.md, Q-753; branch feedback55). `changes` is the rows
/// it changed, 0 for one that changes none (a `select`, a schema's statement), where SQLite's
/// count was still that of the insert, update or delete before it; `last_id` the id of the one
/// row it inserted, null when it inserted none or several (MySQL gave a multi-row insert's
/// first, SQLite its last, and both the id of an insert long before). A statement that returns
/// rows runs as on the others, where SQLite refused it ("Execute returned results").
///
/// Told by SQLite's counts and its last row id, which are left as SQLite has them: the program's
/// own SQL may read `last_insert_rowid()` (setting it aside stored i64::MIN as a child's parent
/// id; found in review). An insert of one row gives that row's id; an upsert's, only when the id
/// moved, as its update leaves the id of the insert before it.
fn sqlite_exec(c: &Connection, scan: &crate::sql::Scan, sql: &str, params: &[Value]) -> Result<Value, String> {
    let head = scan.heads().last().map_or("", |(a, _)| a);
    let inserts = head.eq_ignore_ascii_case("insert") || head.eq_ignore_ascii_case("replace");
    let upsert = inserts && {
        let text = scan.stmts.last().map_or("", |st| scan.stmt_text(st));
        let mut words = text.split_ascii_whitespace();
        let mut before = "";
        words.any(|w| {
            let found = before.eq_ignore_ascii_case("do") && w.eq_ignore_ascii_case("update");
            before = w;
            found
        })
    };
    let (mut changed, mut id) = (0, None);
    let mut run = |stmt: &mut rusqlite::Statement| -> Result<(), String> {
        let (before, was) = (c.total_changes(), c.last_insert_rowid());
        // Its first step does all it changes, rows it returns or not; they are not wanted.
        stmt.raw_query().next().map_err(|e| e.to_string())?;
        changed = if c.total_changes() == before { 0 } else { c.changes() };
        let now = c.last_insert_rowid();
        id = (changed == 1 && inserts && (!upsert || now != was)).then_some(now);
        Ok(())
    };
    if params.is_empty() {
        let mut batch = rusqlite::Batch::new(c, sql);
        while let Some(mut stmt) = batch.next().map_err(|e| e.to_string())? {
            run(&mut stmt)?;
        }
    } else {
        let mut stmt = c.prepare_cached(sql).map_err(|e| e.to_string())?;
        bind(&mut stmt, params)?;
        run(&mut stmt)?;
    }
    let mut m = Map::new();
    m.insert("changes".into(), Value::Int(changed as i64));
    m.insert("last_id".into(), id.map_or(Value::Nil, Value::Int));
    Ok(Value::map(m))
}

/// `db.exec`, or Quillo's own statement (`own`), which may make its table inside the program's
/// transaction.
fn exec_as(it: &mut Interp, args: Vec<Value>, line: u32, own: bool) -> R {
    let on = it.db_on.take();
    let sql = sql_arg(&args, "db.exec", line)?;
    let b = match backend_on(it, on) {
        Ok(b) => b,
        Err(e) => {
            it.wrote_req = Some(crate::httpd::request_number());
            return Ok(db_err(e));
        }
    };
    // Read as this database reads it, once for each text (FEEDBACK55.md, Q-746; branch
    // feedback55).
    let scan = crate::sql::scan(b.dialect(), &sql);
    if scan.is_empty() {
        return Ok(db_err(NO_STATEMENT));
    }
    no_tx_control(&scan, "db.exec", line)?;
    it.wrote_req = Some(crate::httpd::request_number());
    let params = match given_values(&scan, args[1..].to_vec()) {
        Ok(p) => p,
        Err(e) => return Ok(db_err(e)),
    };
    // A change to the session, noted or refused (FEEDBACK44.md, Q-581).
    if let Err(e) = session_change(it, &b, &scan, &params) {
        return Ok(db_err(e));
    }
    // On a server's shared connection, a script's `begin` anywhere (found in review).
    if !matches!(&*b, Backend::Sqlite(_)) && scan.tx_control(true) {
        return fail(line, "`db.exec`: this SQL begins or ends a transaction, which on a shared connection would hold every later request in it: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it");
    }
    if !own {
        no_commit_inside(it, &b, &scan, "db.exec", line)?;
    }
    let alone = isolated(it, &b, &scan);
    // Timed for /_quillo/health/queries (branch query-stats).
    let started = std::time::Instant::now();
    let res = match &*b {
        Backend::Sqlite(m) => {
            // SQLite takes no parameters in a `create table`.
            if params.is_empty() {
                warn_lossy_columns(&sql);
            }
            let c = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (|| {
                tx_gone(&b, &c)?;
                let before = c.is_autocommit();
                let r = sqlite_exec(&c, &scan, &sql, &params);
                // A script run whole (`sqlite3 .dump`'s `BEGIN TRANSACTION; ... COMMIT;`) may
                // hold its own transaction, but must end it: one left open took every
                // later request's writes into it (FEEDBACK44.md, Q-580).
                if params.is_empty() && before && !c.is_autocommit() {
                    let _ = c.execute_batch("rollback");
                    r?;
                    return Err("this SQL left a transaction open, so it was rolled back: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it".to_string());
                }
                r
            })()
        }
        Backend::Postgres(_) if alone => {
            let sql = sql.clone();
            isolated_run(it, &b, move |c| c.exec(&sql, &params))
        }
        Backend::Postgres(pool) => match shared_lanes(it, &b, pool) {
            Some(lanes) => {
                let sql = sql.clone();
                // Boxed here, so the (large) future is not moved again.
                awaited(crate::httpd::await_elsewhere(Box::pin(lanes.exec(crate::httpd::request_number(), sql, params))))
            }
            None => {
                let sql = sql.clone();
                pooled_run(it, &b, move |c| c.exec(&sql, &params))
            }
        },
        #[cfg(feature = "mysql")]
        Backend::Mysql(_) => {
            let sql = sql.clone();
            pooled_run(it, &b, move |c| c.exec(&sql, &params))
        }
    };
    crate::querystats::record(&sql, started.elapsed().as_secs_f64() * 1000.0, res.is_ok());
    // Functions made or dropped: what the names called are is looked up again (see `isolated`).
    match (scan.defines, &*b) {
        (true, Backend::Postgres(p)) => p.forget_fns(),
        #[cfg(feature = "mysql")]
        (true, Backend::Mysql(p)) => p.forget_fns(),
        _ => {}
    }
    if !own {
        committed_inside(&res, &scan, "db.exec", line)?;
    }
    // An update that changed no row (`set seen = 1 where seen = 0`, once all
    // are seen) is not announced: a loader marking what it shows as read
    // would otherwise wake itself for ever (FEEDBACK13.md, Q-090; branch
    // feedback13-fixes).
    match &res {
        // A schema statement changes no row and still counts (FEEDBACK13.md, Q-090; branch
        // feedback13-fixes).
        Ok(Value::Map(m)) if matches!(m.get("changes"), Some(Value::Int(0))) && scan.changes_rows() => announce_as(it, &scan, on, false),
        Ok(_) => announce(it, &scan, on),
        Err(_) => {}
    }
    Ok(res.unwrap_or_else(db_err))
}

// ---- Quillo's own tables -----------------------------------------------------------------

/// The label of the system database among the connections.
const SYSTEM: &str = "$system";

/// Where Quillo keeps its own tables (`_quillo_jobs`, `_quillo_later`,
/// `_quillo_atomic`), as a connection for `exec_on`: the program's database
/// when it is one (Some(None)); else the system database (Some(Some(i))):
/// `db.system(target)`, `$QUILLO_SYSTEM_DB`, or `quillo-system.db` next to
/// the program, when its databases are one per tenant. None: the program
/// has no database, and no system database is there (or, with `make`, to
/// be made: see `system_on_for`). With `make` false, a system database not
/// there yet is not made.
pub(crate) fn system_on(it: &mut Interp, make: bool) -> Option<Option<usize>> {
    system_on_for(it, make, false)
}

/// `system_on`; with `always`, a program without a database gets the
/// system database too (made with `make`): for what must outlive a restart,
/// such as calls made later.
pub(crate) fn system_on_for(it: &mut Interp, make: bool, always: bool) -> Option<Option<usize>> {
    if let Some(i) = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().position(|(l, _)| l == SYSTEM) {
        return Some(Some(i));
    }
    let slot = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    match slot {
        Some(DbSlot::One(_)) => return Some(None),
        Some(_) => {}
        // Not opened yet: the default (`$QUILLO_DB`), if there is one.
        None => {
            if std::env::var("QUILLO_DB").map_or(true, |v| v.is_empty()) {
                // No database: a system database there already (things kept in
                // it before a restart), or one to make when asked to.
                let given = std::env::var("QUILLO_SYSTEM_DB").map_or(false, |v| !v.is_empty());
                if !given && !always && !it.base_dir.join("quillo-system.db").exists() {
                    return None;
                }
            } else {
                let _ = backend(it);
                if matches!(&*it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner), Some(DbSlot::One(_))) {
                    return Some(None);
                }
            }
        }
    }
    let target = std::env::var("QUILLO_SYSTEM_DB").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "quillo-system.db".into());
    if !make && !is_server_url(&target) && !it.base_dir.join(&target).exists() {
        return None;
    }
    let b = open_backend(it, &target).map_err(|e| eprintln!("quillo: the system database {}: {}", target, e)).ok()?;
    let mut conns = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    conns.push((SYSTEM.to_string(), DbSlot::One(Arc::new(b))));
    Some(Some(conns.len() - 1))
}

/// Whether a program uses `db.atomic` (then transactions it left prepared
/// are looked after: see `recover`).
pub fn uses_atomic(it: &Interp) -> bool {
    it.uses_atomic
}

/// `db.system(target)`: where Quillo keeps its own tables, when the
/// program's databases are one per tenant (or to keep them apart).
fn system(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(Value::Str(t)) = args.first() else {
        return fail(line, "`db.system` expects a path or a database URL");
    };
    if t.contains("{tenant}") {
        return fail(line, "`db.system` is one database for every tenant: no {tenant} in it");
    }
    let b = match open_backend(it, t) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    let mut conns = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    conns.retain(|(l, _)| l != SYSTEM);
    conns.push((SYSTEM.to_string(), DbSlot::One(Arc::new(b))));
    Ok(Value::Nil)
}

/// `db.exec_many(sql, rows)`: the statement once for each row (a list of its `?` values), all
/// of them or none: in a transaction of its own, or a savepoint of the one it is in. `{ changes
/// }`, or the first error, with nothing written. As Python's `executemany` does, an `insert
/// into t (...) values (?, ...)` goes to PostgreSQL and MySQL as inserts of many rows each, not
/// a round trip a row (branch feedback40-43).
fn exec_many(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let sql = sql_arg(&args, "db.exec_many", line)?;
    let rows: Vec<Vec<Value>> = match args.get(1) {
        Some(Value::List(l)) if args.len() == 2 => {
            let mut rows = Vec::with_capacity(l.len());
            for (i, r) in l.iter().enumerate() {
                match r {
                    Value::List(v) => rows.push(v.to_vec()),
                    other => return fail(line, format!("`db.exec_many`: row {} is {}, not a list of the statement's values", i + 1, other.type_name())),
                }
            }
            rows
        }
        _ => {
            let got = args.get(1).map_or("nothing", |v| v.type_name());
            return fail(line, format!("`db.exec_many` takes the SQL and a list of rows, each a list of its values, got {}", got));
        }
    };
    let changes = |n: i64| {
        let mut m = Map::new();
        m.insert("changes".into(), Value::Int(n));
        Value::map(m)
    };
    if rows.is_empty() {
        return Ok(changes(0));
    }
    let b = match backend_on(it, on) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    let server = !matches!(&*b, Backend::Sqlite(_));
    // Its rows are all or none, in a transaction of its own (FEEDBACK55.md, Q-728).
    if b.is_mysql() {
        if let Some(stmt) = crate::sql::scan(b.dialect(), &sql).commits_by_itself() {
            return fail(line, commits_by_itself("db.exec_many", stmt));
        }
    }
    let width = rows[0].len();
    if let Some((i, r)) = rows.iter().enumerate().find(|(_, r)| r.len() != width) {
        return Ok(db_err(format!("row {} has {} value{}, the first has {}", i + 1, r.len(), if r.len() == 1 { "" } else { "s" }, width)));
    }
    // Rows by the statement each, or many to a statement: as many as fit under the servers'
    // 65,535 parameters, and at most 1,000.
    let many = if server { crate::sql::scan(b.dialect(), &sql).values_tuple(width) } else { None };
    let per = many.as_ref().map_or(1, |_| (60_000 / width.max(1)).clamp(1, 1000));
    let mut body = |it: &mut Interp| -> R {
        let mut total = 0;
        for chunk in rows.chunks(per) {
            let text = match &many {
                Some(head) => {
                    let one = format!("({})", vec!["?"; width].join(", "));
                    format!("{}{}", head, vec![one.as_str(); chunk.len()].join(", "))
                }
                None => sql.clone(),
            };
            let mut a = Vec::with_capacity(1 + chunk.len() * width);
            a.push(Value::str(&text));
            for r in chunk {
                a.extend(r.iter().cloned());
            }
            match exec_on(it, on, a)? {
                Value::Map(m) => total += match m.get("changes") { Some(Value::Int(n)) => *n, _ => 0 },
                err => return Ok(err),
            }
        }
        Ok(changes(total))
    };
    crate::txn::run(it, on, &crate::txn::Options::default(), line, &mut body)
}

/// `db.exec` / `db.query` on connection `on` (None: the program's).
pub(crate) fn exec_on(it: &mut Interp, on: Option<usize>, args: Vec<Value>) -> R {
    it.db_on = on;
    exec_as(it, args, 0, true)
}

pub(crate) fn query_on(it: &mut Interp, on: Option<usize>, args: Vec<Value>) -> R {
    it.db_on = on;
    query_as(it, args, 0, None, true)
}

pub(crate) fn kind_on(it: &mut Interp, on: Option<usize>) -> bool {
    backend_on(it, on).map_or(false, |b| b.is_mysql())
}

/// Whether this server runs a scheduled job's run due at `at`: with a
/// database for Quillo's own tables (which other servers may share), only
/// if no one recorded that run before; otherwise always.
/// A job's key in `_quillo_jobs`: its name, or past the 255 characters MySQL's column holds, a
/// part of it and a hash of the whole. `insert ignore` cut a longer one, the update then matched
/// nothing, and the job never ran again (found in review, branch feedback55); the same key on
/// every database. (`schedule` and `every` take names of 64 characters at most, so this is for
/// what may come to it some other way.)
fn job_key(name: &str) -> std::borrow::Cow<'_, str> {
    if name.chars().count() <= 255 {
        return name.into();
    }
    use sha2::Digest;
    let head: String = name.chars().take(180).collect();
    format!("{}#{}", head, crate::crypto::hex(&sha2::Sha256::digest(name.as_bytes()))).into()
}

pub(crate) fn claim_job(it: &mut Interp, name: &str, at: u64) -> bool {
    let Some(on) = system_on(it, true) else { return true };
    let name = &*job_key(name);
    let mysql = kind_on(it, on);
    let table = if mysql {
        "create table if not exists _quillo_jobs (name varchar(255) primary key, at bigint not null)"
    } else {
        "create table if not exists _quillo_jobs (name text primary key, at bigint not null)"
    };
    if !matches!(exec_on(it, on, vec![Value::str(table)]), Ok(Value::Map(_))) {
        return true;
    }
    // A later run claims it. MySQL counts the rows an update matches, as the others do
    // (FEEDBACK55.md, Q-739), so a row left as it was is no longer "no change" there: the row
    // goes in if it is not there, else only a later run moves it (branch feedback55).
    let claimed = |r: R| match r {
        Ok(Value::Map(m)) => !matches!(m.get("changes"), Some(Value::Int(0))),
        _ => true,
    };
    if mysql {
        let first = exec_on(it, on, vec![Value::str("insert ignore into _quillo_jobs (name, at) values (?, ?)"), Value::str(name), Value::Int(at as i64)]);
        if !matches!(first, Ok(Value::Map(ref m)) if matches!(m.get("changes"), Some(Value::Int(0)))) {
            return claimed(first);
        }
        return claimed(exec_on(it, on, vec![Value::str("update _quillo_jobs set at = ? where name = ? and at < ?"), Value::Int(at as i64), Value::str(name), Value::Int(at as i64)]));
    }
    let sql = "insert into _quillo_jobs (name, at) values (?, ?) on conflict (name) do update set at = excluded.at where _quillo_jobs.at < excluded.at";
    claimed(exec_on(it, on, vec![Value::str(sql), Value::str(name), Value::Int(at as i64)]))
}

// ---- calls made later (src/service.rs), kept with the work they go with ----

/// How long a finished call's answer can be asked for (`later.status`).
const LATER_ANSWER_MS: i64 = 86_400_000;
/// How long after its last call a tenant is still looked at: a call in a
/// transaction shows only when that commits.
const LATER_TENANT_MS: i64 = 3_600_000;
/// `at` of a call given up on: never due. It keeps its row, `tries` < 0.
const LATER_NEVER: i64 = 9_000_000_000_000_000;

fn later_table(it: &mut Interp, on: Option<usize>) -> bool {
    let mysql = kind_on(it, on);
    let sql = if mysql {
        "create table if not exists _quillo_later (id varchar(64) primary key, service varchar(255) not null, fn varchar(255) not null, \
         args text not null, tenant varchar(255) not null, rid varchar(64) not null, at bigint not null, tries bigint not null, error text)"
    } else {
        "create table if not exists _quillo_later (id text primary key, service text not null, fn text not null, \
         args text not null, tenant text not null, rid text not null, at bigint not null, tries bigint not null, error text)"
    };
    // MySQL and MariaDB commit an open transaction before `create table`, even
    // one that makes nothing, and the rollback then has nothing to undo
    // (FEEDBACK23.md, Q-182; branch feedback23-fixes): ask, and make it apart.
    if mysql {
        let made = "select 1 as n from information_schema.tables where table_schema = database() and table_name = '_quillo_later'";
        if matches!(query_on(it, on, vec![Value::str(made)]), Ok(Value::List(l)) if !l.is_empty()) {
            return true;
        }
        let Ok(b) = backend_on(it, on) else { return false };
        return apart(it, &b, sql.into(), Vec::new()).is_ok();
    }
    matches!(exec_on(it, on, vec![Value::str(sql)]), Ok(Value::Map(_)))
}

/// A statement on one of Quillo's tables, made the first time it is missing.
/// Not for a program's transaction: PostgreSQL gives up on one at the error.
fn on_own_table(it: &mut Interp, on: Option<usize>, table: &str, make: &str, args: Vec<Value>) -> bool {
    for first in [true, false] {
        match exec_on(it, on, args.clone()) {
            Ok(Value::Map(_)) => return true,
            Ok(Value::Err(e, _, _)) if first && e.display().contains(table) => {
                if !matches!(exec_on(it, on, vec![Value::str(make)]), Ok(Value::Map(_))) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    false
}

fn tenant_dbs(it: &Interp) -> bool {
    matches!(&*it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner), Some(DbSlot::PerTenant(_)))
}

/// Where a call made later is kept: the database the work is in, so that it
/// commits or rolls back with it, on any database and beside `db.system`
/// (FEEDBACK23.md, Q-185; branch feedback23-fixes). With neither, the system
/// database.
fn later_home(it: &mut Interp) -> Option<Option<usize>> {
    let system = system_on_for(it, true, true);
    let slot = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    match slot {
        Some(DbSlot::One(_)) => Some(None),
        Some(DbSlot::PerTenant(_)) if it.tenant().is_some() => Some(None),
        _ => system,
    }
}

/// Where a program's calls are looked for, tenants' databases aside.
pub(crate) fn later_homes(it: &mut Interp) -> Vec<Option<usize>> {
    let mut homes: Vec<Option<usize>> = system_on(it, false).into_iter().collect();
    let one = matches!(&*it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner), Some(DbSlot::One(_)));
    if one && !homes.contains(&None) {
        homes.push(None);
    }
    homes
}

/// Notes in the system database that a tenant has calls: its database is not
/// otherwise looked at. It goes in ahead of the call, and outside any
/// transaction, so a call rolled back leaves a note that comes to nothing.
fn later_tenant_noted(it: &mut Interp, tenant: &str) -> bool {
    let Some(on) = system_on_for(it, true, true) else { return false };
    let mysql = kind_on(it, on);
    let make = if mysql {
        "create table if not exists _quillo_later_tenants (tenant varchar(255) primary key, made bigint not null, at bigint not null)"
    } else {
        "create table if not exists _quillo_later_tenants (tenant text primary key, made bigint not null, at bigint not null)"
    };
    let sql = if mysql {
        "insert into _quillo_later_tenants (tenant, made, at) values (?, ?, ?) on duplicate key update made = values(made), at = values(at)"
    } else {
        "insert into _quillo_later_tenants (tenant, made, at) values (?, ?, ?) on conflict (tenant) do update set made = excluded.made, at = excluded.at"
    };
    let now = now_ms();
    on_own_table(it, on, "_quillo_later_tenants", make, vec![Value::str(sql), Value::str(tenant), Value::Int(now), Value::Int(now)])
}

/// Keeps a call made later; false when there is no database to keep it in.
pub(crate) fn later_put(it: &mut Interp, c: &crate::service::Later) -> bool {
    let Some(on) = later_home(it) else { return false };
    if on.is_none() && tenant_dbs(it) && !later_tenant_noted(it, &c.tenant) {
        return false;
    }
    if !later_table(it, on) {
        return false;
    }
    let sql = "insert into _quillo_later (id, service, fn, args, tenant, rid, at, tries) values (?, ?, ?, ?, ?, ?, ?, ?)";
    let args = vec![
        Value::str(sql),
        Value::str(&c.id),
        Value::str(&c.service),
        Value::str(&c.f),
        Value::str(&c.args),
        Value::str(&c.tenant),
        Value::str(&c.rid),
        Value::Int(c.at),
        Value::Int(c.tries),
    ];
    matches!(exec_on(it, on, args), Ok(Value::Map(_)))
}

/// Calls made later that are due, on connection `on`.
pub(crate) fn later_due(it: &mut Interp, on: Option<usize>, now: i64, limit: usize) -> Vec<crate::service::Later> {
    let sql = format!("select id, service, fn, args, tenant, rid, at, tries from _quillo_later where at <= ? order by at limit {}", limit);
    let Ok(Value::List(rows)) = query_on(it, on, vec![Value::str(sql), Value::Int(now)]) else { return Vec::new() };
    let text = |r: &Map, k: &str| match r.get(k) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    let int = |r: &Map, k: &str| match r.get(k) {
        Some(Value::Int(n)) => *n,
        _ => 0,
    };
    rows.iter()
        .filter_map(|r| match r {
            Value::Map(r) => Some(crate::service::Later {
                id: text(r, "id"),
                service: text(r, "service"),
                f: text(r, "fn"),
                args: text(r, "args"),
                tenant: text(r, "tenant"),
                rid: text(r, "rid"),
                at: int(r, "at"),
                tries: int(r, "tries"),
            }),
            _ => None,
        })
        .collect()
}

/// Takes a due call for this server (others may share the database): only
/// if it is still due at `was`; it is put off to `until` meanwhile.
pub(crate) fn later_take(it: &mut Interp, on: Option<usize>, id: &str, was: i64, until: i64) -> bool {
    let sql = "update _quillo_later set at = ? where id = ? and at = ?";
    match exec_on(it, on, vec![Value::str(sql), Value::Int(until), Value::str(id), Value::Int(was)]) {
        Ok(Value::Map(m)) => !matches!(m.get("changes"), Some(Value::Int(0))),
        _ => false,
    }
}

/// A call that got through: its row goes, and its answer is kept for a day
/// (FEEDBACK23.md, Q-188; branch feedback23-fixes), in a table of its own so
/// that `_quillo_later` stays as small as what is waiting.
pub(crate) fn later_done(it: &mut Interp, on: Option<usize>, id: &str, answer: &str) {
    let make = if kind_on(it, on) {
        "create table if not exists _quillo_later_done (id varchar(64) primary key, at bigint not null, answer longtext not null)"
    } else {
        "create table if not exists _quillo_later_done (id text primary key, at bigint not null, answer text not null)"
    };
    let now = now_ms();
    let sql = "insert into _quillo_later_done (id, at, answer) values (?, ?, ?)";
    on_own_table(it, on, "_quillo_later_done", make, vec![Value::str(sql), Value::str(id), Value::Int(now), Value::str(answer)]);
    if fastrand::u8(..) == 0 {
        let _ = exec_on(it, on, vec![Value::str("delete from _quillo_later_done where at < ?"), Value::Int(now - LATER_ANSWER_MS)]);
    }
    let _ = exec_on(it, on, vec![Value::str("delete from _quillo_later where id = ?"), Value::str(id)]);
}

pub(crate) fn later_again(it: &mut Interp, on: Option<usize>, id: &str, at: i64, tries: i64, why: &str) {
    let sql = "update _quillo_later set at = ?, tries = ?, error = ? where id = ?";
    let _ = exec_on(it, on, vec![Value::str(sql), Value::Int(at), Value::Int(tries), Value::str(why), Value::str(id)]);
}

/// A call given up on: its row stays, with why (FEEDBACK23.md, Q-184; branch
/// feedback23-fixes), so that a person can look, and send it again by setting
/// `at` and `tries` to 0.
pub(crate) fn later_failed(it: &mut Interp, on: Option<usize>, id: &str, why: &str) {
    later_again(it, on, id, LATER_NEVER, -1, why);
}

/// Tenants that may have calls due, and when each was last given one.
pub(crate) fn later_tenants(it: &mut Interp, now: i64, limit: usize) -> Vec<(String, i64)> {
    if !tenant_dbs(it) {
        return Vec::new();
    }
    let Some(on) = system_on(it, false) else { return Vec::new() };
    let sql = format!("select tenant, made from _quillo_later_tenants where at <= ? order by at limit {}", limit);
    let Ok(Value::List(rows)) = query_on(it, on, vec![Value::str(sql), Value::Int(now)]) else { return Vec::new() };
    rows.iter()
        .filter_map(|r| match r {
            Value::Map(r) => match (r.get("tenant"), r.get("made")) {
                (Some(Value::Str(t)), Some(Value::Int(m))) => Some((t.to_string(), *m)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// When the calls waiting on `on` are next due: None if there are none, an
/// error if the database cannot say.
pub(crate) fn later_next(it: &mut Interp, on: Option<usize>) -> Result<Option<i64>, ()> {
    let sql = "select min(at) as at from _quillo_later where tries >= 0";
    match query_on(it, on, vec![Value::str(sql)]) {
        Ok(Value::List(rows)) => Ok(match rows.first() {
            Some(Value::Map(r)) => match r.get("at") {
                Some(Value::Int(at)) => Some(*at),
                _ => None,
            },
            _ => None,
        }),
        // Made in a transaction that rolled back.
        Ok(Value::Err(e, _, _)) if e.display().contains("_quillo_later") => Ok(None),
        _ => Err(()),
    }
}

/// After looking at a tenant: when to look again. With nothing waiting, soon
/// after a call was made, as it may be in a transaction not yet committed,
/// then less often, and after a while not at all.
pub(crate) fn later_tenant_seen(it: &mut Interp, tenant: &str, made: i64, next: Result<Option<i64>, ()>) {
    let Some(on) = system_on(it, false) else { return };
    let now = now_ms();
    let at = match next {
        Ok(Some(at)) => at,
        Ok(None) if now - made >= LATER_TENANT_MS => {
            let sql = "delete from _quillo_later_tenants where tenant = ? and made = ?";
            let _ = exec_on(it, on, vec![Value::str(sql), Value::str(tenant), Value::Int(made)]);
            return;
        }
        Ok(None) => now + (now - made).clamp(1000, 60_000),
        Err(()) => now + 60_000,
    };
    let sql = "update _quillo_later_tenants set at = ? where tenant = ? and made = ?";
    let _ = exec_on(it, on, vec![Value::str(sql), Value::Int(at), Value::str(tenant), Value::Int(made)]);
}

/// What became of call `id`: its tries and why it has not got through, or its
/// answer. Read apart, as a table not made yet must not end a transaction,
/// so a call made in a transaction shows once that has committed.
pub(crate) fn later_status(it: &mut Interp, id: &str) -> Option<(i64, Option<String>, Option<String>)> {
    let mut homes = later_homes(it);
    if tenant_dbs(it) && it.tenant().is_some() {
        homes.insert(0, None);
    }
    for on in homes {
        let Ok(b) = backend_on(it, on) else { continue };
        let waiting = apart(it, &b, "select tries, error from _quillo_later where id = ?".into(), vec![Value::str(id)]);
        if let Ok(Some(Value::Map(r))) = waiting.map(|mut rows| rows.pop()) {
            let tries = if let Some(Value::Int(n)) = r.get("tries") { *n } else { 0 };
            let error = match r.get("error") {
                Some(Value::Str(e)) => Some(e.to_string()),
                _ => None,
            };
            return Some((tries, error, None));
        }
        let done = apart(it, &b, "select answer from _quillo_later_done where id = ?".into(), vec![Value::str(id)]);
        if let Ok(Some(Value::Map(r))) = done.map(|mut rows| rows.pop()) {
            if let Some(Value::Str(a)) = r.get("answer") {
                return Some((0, None, Some(a.to_string())));
            }
        }
    }
    None
}

// ---- calls a service answered, shared by its copies (src/service.rs) ----------------------

/// What another copy of the service did with a call (FEEDBACK10.md, Q-073;
/// branch feedback10-fixes).
pub(crate) enum Claim {
    /// This copy runs it.
    Mine,
    /// Another copy answered it: the status and the body.
    Done(u16, String),
    /// Another copy is running it.
    Running,
}

fn calls_table(it: &mut Interp, on: Option<usize>) -> bool {
    let sql = if kind_on(it, on) {
        "create table if not exists _quillo_calls (id varchar(64) primary key, at bigint not null, status integer not null, answer text not null)"
    } else {
        "create table if not exists _quillo_calls (id text primary key, at bigint not null, status integer not null, answer text not null)"
    };
    matches!(exec_on(it, on, vec![Value::str(sql)]), Ok(Value::Map(_)))
}

/// Takes call `id` for this copy, unless a copy sharing the database has:
/// the database is where copies of a service behind a balancer, each with
/// its own memory, see what the others answered. None without a database.
pub(crate) fn call_claim(it: &mut Interp, id: &str, now_ms: i64, forget_before: i64) -> Option<Claim> {
    let on = system_on(it, false)?;
    if !calls_table(it, on) {
        return None;
    }
    // Now and then, what no copy remembers any longer.
    if fastrand::u8(..) == 0 {
        let _ = exec_on(it, on, vec![Value::str("delete from _quillo_calls where at < ?"), Value::Int(forget_before)]);
    }
    let sql = "insert into _quillo_calls (id, at, status, answer) values (?, ?, 0, '')";
    if matches!(exec_on(it, on, vec![Value::str(sql), Value::str(id), Value::Int(now_ms)]), Ok(Value::Map(_))) {
        return Some(Claim::Mine);
    }
    let rows = query_on(it, on, vec![Value::str("select status, answer, at from _quillo_calls where id = ?"), Value::str(id)]);
    match rows {
        Ok(Value::List(rows)) => match rows.first() {
            Some(Value::Map(r)) => match (r.get("status"), r.get("answer"), r.get("at")) {
                (Some(Value::Int(s)), Some(Value::Str(a)), _) if *s > 0 => Some(Claim::Done(*s as u16, a.to_string())),
                // A copy that died running it left this, and it would be
                // refused for ever. Older than a signature can be, it is free
                // (FEEDBACK23.md, Q-186; branch feedback23-fixes).
                (_, _, Some(Value::Int(at))) if *at < forget_before => {
                    let sql = "update _quillo_calls set at = ? where id = ? and status = 0 and at = ?";
                    match exec_on(it, on, vec![Value::str(sql), Value::Int(now_ms), Value::str(id), Value::Int(*at)]) {
                        Ok(Value::Map(m)) if !matches!(m.get("changes"), Some(Value::Int(0))) => Some(Claim::Mine),
                        _ => Some(Claim::Running),
                    }
                }
                _ => Some(Claim::Running),
            },
            // Gone between the two (forgotten): this copy runs it.
            _ => Some(Claim::Mine),
        },
        _ => None,
    }
}

/// The answer to a call this copy claimed; one that did not finish is let
/// go, to be sent again (FEEDBACK23.md, Q-184; branch feedback23-fixes).
pub(crate) fn call_done(it: &mut Interp, id: &str, status: u16, answer: &str) {
    let Some(on) = system_on(it, false) else { return };
    if status != 200 {
        let _ = exec_on(it, on, vec![Value::str("delete from _quillo_calls where id = ?"), Value::str(id)]);
    } else {
        let sql = "update _quillo_calls set status = ?, answer = ? where id = ?";
        let _ = exec_on(it, on, vec![Value::str(sql), Value::Int(status as i64), Value::str(answer), Value::str(id)]);
    }
}

fn select(it: &mut Interp, on: Option<usize>, sql: String, params: Vec<Value>, limit: Option<usize>) -> Result<Vec<Value>, String> {
    let b = backend_on(it, on)?;
    let scan = crate::sql::scan(b.dialect(), &sql);
    select_in(it, on, b, &scan, sql, params, limit)
}

/// `select` on a database already found, its SQL read.
fn select_in(
    it: &mut Interp,
    on: Option<usize>,
    b: Arc<Backend>,
    scan: &crate::sql::Scan,
    sql: String,
    params: Vec<Value>,
    limit: Option<usize>,
) -> Result<Vec<Value>, String> {
    if it.writes_noted() && !only_reads(scan) && it.no_writes("SQL that writes", 0).is_err() {
        return Err(crate::interp::PREFETCH_REFUSED.into());
    }
    let params = given_values(scan, params)?;
    // Refused before a replica would take it (found in review).
    session_change(it, &b, scan, &params)?;
    let alone = isolated(it, &b, scan);
    // A read to look at, that is not one (`select nextval('s')`, a function that writes): a `live`
    // run through it gave two pages one ticket (FEEDBACK55.md, Q-888; branch feedback55).
    if it.in_run && (alone || !only_reads(scan) || calls_own_on_mysql(it, &b, scan)) {
        it.acted = true;
    }
    // Timed for /_quillo/health/queries (branch query-stats).
    let started = std::time::Instant::now();
    // A read, to a replica: if it fails there, the primary answers (and the
    // replica is left out for ten seconds). One the replica refused as a write (a function
    // that writes, which its words did not show) goes to the primary, and the replica stays in
    // use (FEEDBACK55.md, Q-750; branch feedback55).
    let replica = if alone { None } else { reader(it, &b, scan) };
    if let Some((reps, i)) = replica.filter(|_| !calls_own_on_mysql(it, &b, scan)) {
        let r = reps.list[i].clone();
        let (q, p) = (sql.clone(), params.clone());
        match pooled_run(it, &r, move |c| c.query(&q, &p, limit)) {
            Ok(rows) => {
                crate::querystats::record(&sql, started.elapsed().as_secs_f64() * 1000.0, true);
                announce(it, scan, on);
                return Ok(rows);
            }
            Err(e) if read_only_refusal(&e) => {}
            Err(e) => {
                eprintln!("quillo: a read replica failed ({}); reading from the primary", e);
                reps.down.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[i] = Some(std::time::Instant::now() + std::time::Duration::from_secs(10));
            }
        }
    }
    if !only_reads(scan) {
        it.wrote_req = Some(crate::httpd::request_number());
    }
    let res = match &*b {
        Backend::Sqlite(m) => {
            let c = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            tx_gone(&b, &c).and_then(|_| rows(&c, &sql, &params, limit))
        }
        Backend::Postgres(_) if alone => {
            let sql = sql.clone();
            isolated_run(it, &b, move |c| c.query(&sql, &params, limit))
        }
        Backend::Postgres(pool) => match shared_lanes(it, &b, pool) {
            Some(lanes) => awaited(crate::httpd::await_elsewhere(Box::pin(lanes.query(crate::httpd::request_number(), sql.clone(), params, limit)))),
            None => {
                let sql = sql.clone();
                pooled_run(it, &b, move |c| c.query(&sql, &params, limit))
            }
        },
        #[cfg(feature = "mysql")]
        Backend::Mysql(_) => {
            let sql = sql.clone();
            pooled_run(it, &b, move |c| c.query(&sql, &params, limit))
        }
    };
    crate::querystats::record(&sql, started.elapsed().as_secs_f64() * 1000.0, res.is_ok());
    if res.is_ok() {
        announce(it, scan, on);
    }
    res
}

/// On MySQL, whether a read calls a function of the database's own, which may write and so is
/// the primary's: looked up once a name in `information_schema.routines`, as PostgreSQL's are
/// in `pg_proc` (`isolated`). One that cannot be looked up is taken for one (FEEDBACK55.md,
/// Q-750; found in review, branch feedback55).
fn calls_own_on_mysql(it: &mut Interp, b: &Arc<Backend>, scan: &crate::sql::Scan) -> bool {
    #[cfg(feature = "mysql")]
    if let Backend::Mysql(pool) = &**b {
        if scan.calls.is_empty() {
            return false;
        }
        if let Some(own) = pool.calls_own(&scan.calls) {
            return own;
        }
        let (names, b) = (scan.calls.clone(), b.clone());
        return it
            .unlocked(move || match &*b {
                Backend::Mysql(p) => p.look_up_fns(&names),
                _ => Ok(false),
            })
            .unwrap_or(true);
    }
    let _ = (it, b, scan);
    false
}

/// A replica's refusal of a statement that writes: PostgreSQL's 25006 (a read-only transaction,
/// as a standby's are) or its "recovery is in progress", MySQL's 1290 (`--read-only`), 1792 (a
/// read-only transaction) and 1836 (read-only mode). The statement is the primary's, not the
/// replica down (FEEDBACK55.md, Q-750; branch feedback55).
fn read_only_refusal(e: &str) -> bool {
    e.ends_with("[25006]") || (e.ends_with("[55000]") && e.contains("recovery")) || ["ERROR 1290 ", "ERROR 1792 ", "ERROR 1836 ", "(1290)", "(1792)", "(1836)"].iter().any(|c| e.contains(c))
}

/// `db.query(sql, ...params)`: every row, as maps from column to value.
fn query(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    query_as(it, args, line, None, false)
}

/// `db.one(sql, ...params)`: the first row, or null.
fn one(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    query_as(it, args, line, Some(1), false)
}

/// `db.query`, or `db.one` (`limit` 1), or Quillo's own (`own`).
fn query_as(it: &mut Interp, args: Vec<Value>, line: u32, limit: Option<usize>, own: bool) -> R {
    let on = it.db_on.take();
    let f = if limit.is_some() { "db.one" } else { "db.query" };
    let sql = sql_arg(&args, f, line)?;
    let b = match backend_on(it, on) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(e)),
    };
    let scan = crate::sql::scan(b.dialect(), &sql);
    if scan.is_empty() {
        return Ok(db_err(NO_STATEMENT));
    }
    no_tx_control(&scan, f, line)?;
    if !own {
        no_commit_inside(it, &b, &scan, f, line)?;
    }
    let rows = select_in(it, on, b, &scan, sql, args[1..].to_vec(), limit);
    if !own {
        committed_inside(&rows, &scan, f, line)?;
    }
    Ok(match limit {
        Some(_) => rows.map(|r| r.into_iter().next().unwrap_or(Value::Nil)).unwrap_or_else(db_err),
        None => rows.map(Value::list).unwrap_or_else(db_err),
    })
}

/// `db.transaction(() => { ... })`: commits if the function returns normally,
/// rolls back if it fails, returns an error value, or replies early.
/// Transactions nest (as savepoints). `db.transaction({ isolation, retries },
/// f)` sets the isolation level and runs `f` again on a conflict (see `txn`).
fn transaction(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let (opts, f) = match args.as_slice() {
        [f @ (Value::Func(_) | Value::Builtin(..))] => (crate::txn::Options::default(), f.clone()),
        [o @ Value::Map(_), f @ (Value::Func(_) | Value::Builtin(..))] => (crate::txn::options(o, "db.transaction", line)?, f.clone()),
        _ => {
            let got = args.last().map_or("nothing", |v| v.type_name());
            return fail(line, format!("`db.transaction` expects a function (after its options, if any), got {}", got));
        }
    };
    crate::txn::run(it, on, &opts, line, &mut |it| it.call(&f, Vec::new(), line))
}

/// What `body` gave, or the first error a statement in it dropped: a transaction is never
/// committed past a failure no one took (FEEDBACK27.md, Q-332; branch feedback27-35-fixes).
fn caught(it: &mut Interp, body: &mut dyn FnMut(&mut Interp) -> R) -> R {
    let outer = it.dropped.take();
    it.catching_drops += 1;
    let r = body(it);
    it.catching_drops -= 1;
    let dropped = std::mem::replace(&mut it.dropped, outer);
    match (r, dropped) {
        (Ok(v), Some(e)) if !matches!(v, Value::Err(..)) => Ok(e),
        (r, _) => r,
    }
}

/// What a statement in a transaction SQLite ended is told.
const TX_GONE: &str = "the database ended this transaction after an error, and rolled back all it held, so nothing more runs in it";

thread_local! {
    /// The SQLite databases with a transaction of `db.transaction`'s open on this thread (no
    /// other request runs on it meanwhile: see `Interp::unlocked`).
    static IN_TX: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A statement inside a transaction SQLite has ended by itself: refused, where it was
/// committed on its own (FEEDBACK49.md, Q-591; branch main). Only on the database whose
/// transaction it is: another's statements, the system database's among them, run as ever
/// (found in review).
fn tx_gone(b: &Arc<Backend>, c: &Connection) -> Result<(), String> {
    let mine = Arc::as_ptr(b) as usize;
    if c.is_autocommit() && IN_TX.with(|t| t.borrow().contains(&mine)) {
        return Err(TX_GONE.to_string());
    }
    Ok(())
}

/// One try of a transaction on connection `on`: what `body` gave (or the
/// error that ended it), and whether the database gave up on it because of
/// another transaction (then it is worth another try; told only for the
/// outermost transaction, the one that can be tried again).
pub(crate) fn attempt(
    it: &mut Interp,
    on: Option<usize>,
    opts: &crate::txn::Options,
    line: u32,
    body: &mut dyn FnMut(&mut Interp) -> R,
) -> (R, bool) {
    let b = match backend_on(it, on) {
        Ok(b) => b,
        Err(e) => return (Ok(db_err(e)), false),
    };
    let ok = |r: &R| matches!(r, Ok(v) if !matches!(v, Value::Err(..)));
    let nested_isolation = || fail(line, "a transaction inside another cannot change its isolation: give it to the outermost one");
    match &*b {
        Backend::Sqlite(m) => {
            let outermost = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_autocommit();
            if !outermost && opts.isolation.is_some() {
                return (nested_isolation(), false);
            }
            // With options, the outermost takes the write lock at once.
            let immediate = outermost && opts.given;
            let run = |sql: &str| m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute_batch(sql);
            let busy = |e: &rusqlite::Error| e.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy);
            if let Err(e) = run(if immediate { "begin immediate" } else { "savepoint quillo_tx" }) {
                return (Ok(db_err(&e)), immediate && busy(&e));
            }
            let mark = tx_begin(it);
            it.sqlite_tx += 1;
            let mine = Arc::as_ptr(&b) as usize;
            IN_TX.with(|t| t.borrow_mut().push(mine));
            crate::httpd::hold_in_place(true);
            let r = caught(it, body);
            crate::httpd::hold_in_place(false);
            IN_TX.with(|t| {
                let mut t = t.borrow_mut();
                if let Some(i) = t.iter().rposition(|p| *p == mine) {
                    t.remove(i);
                }
            });
            it.sqlite_tx -= 1;
            // SQLite ended it itself (a full disk, an I/O error) and rolled back all it held:
            // the release would fail, and what ran after the error was committed by itself
            // (FEEDBACK49.md, Q-591; branch main). Nothing more ran in it (`tx_gone`).
            if m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_autocommit() {
                tx_end(it, mark, false);
                let why = match &r {
                    Ok(Value::Err(e, ..)) => format!(": {}", e.display().trim_start_matches("db: ")),
                    _ => String::new(),
                };
                return (Ok(db_err(format!("{}{}", TX_GONE, why))), false);
            }
            let commit = ok(&r);
            let end = match (commit, immediate) {
                (true, true) => "commit",
                (false, true) => "rollback",
                (true, false) => "release quillo_tx",
                (false, false) => "rollback to quillo_tx; release quillo_tx",
            };
            let ended = run(end);
            // The outermost `release` is a commit, and one that fails leaves the transaction
            // open, as `commit` does (FEEDBACK16.md, Q-108; branch feedback20-fixes).
            if ended.is_err() && outermost && commit {
                let _ = run("rollback");
            }
            tx_end(it, mark, commit && ended.is_ok());
            match ended {
                Err(e) if commit => {
                    let again = immediate && busy(&e);
                    (Ok(db_err(e)), again)
                }
                _ => (r, false),
            }
        }
        _ => {
            let pool = &*b;
            // Keep one connection for this request until the outermost
            // transaction ends.
            let pinned = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|t| Arc::ptr_eq(&t.backend, &b));
            if pinned && opts.isolation.is_some() {
                return (nested_isolation(), false);
            }
            if !pinned {
                let mut c = match checkout_for_tx(it, &b) {
                    Ok(c) => c,
                    Err(e) => return (Ok(db_err(e)), false),
                };
                c.watch_conflicts(true);
                it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(TxConn { backend: b.clone(), conn: Some(c), depth: 0 });
            }
            let depth = {
                let mut g = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let tx = g.iter_mut().find(|t| Arc::ptr_eq(&t.backend, &b)).expect("pinned");
                tx.depth += 1;
                tx.depth
            };
            let begin = if depth == 1 { crate::txn::begin_sql(b.is_mysql(), opts.isolation) } else { format!("savepoint quillo_tx_{}", depth) };
            let mut started = { let begin = begin.clone(); pooled_run(it, &b, move |c| c.batch(&begin)) };
            // A pooled connection the server ended while it sat idle (a restart, a failover)
            // fails its first statement: nothing has run, so another is taken and the
            // transaction begun on it, where the program was given "internal error"
            // (FEEDBACK49.md, Q-601; branch main).
            // As many as the pool holds, each perhaps ended, and two more: three gave up while
            // dead idle ones were left, and with eight the first two transactions failed (found
            // in review).
            let most = match &*b {
                Backend::Postgres(p) => p.size() + 2,
                _ => 3,
            };
            let mut tries = 0;
            while depth == 1 && tries < most && started.as_ref().err().is_some_and(|e| kind_of(e) == Some("connection")) {
                tries += 1;
                let dead = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter_mut().find(|t| Arc::ptr_eq(&t.backend, &b)).and_then(|t| t.conn.take());
                if let Some(d) = dead {
                    b.discard(d);
                }
                match checkout_for_tx(it, &b) {
                    Ok(mut c) => {
                        c.watch_conflicts(true);
                        if let Some(t) = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter_mut().find(|t| Arc::ptr_eq(&t.backend, &b)) {
                            t.conn = Some(c);
                        }
                    }
                    Err(e) => {
                        started = Err(e);
                        break;
                    }
                }
                let begin = begin.clone();
                started = pooled_run(it, &b, move |c| c.batch(&begin));
            }
            let mark = tx_begin(it);
            let r = match started {
                Ok(()) => caught(it, body),
                Err(e) => Ok(db_err(e)),
            };
            let conflicted = |it: &Interp| {
                let g = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                g.iter().find(|t| Arc::ptr_eq(&t.backend, &b)).and_then(|t| t.conn.as_ref()).map_or(false, |c| c.conflicted())
            };
            // A statement failed because of another transaction: this one
            // is over, whatever the function made of the error.
            let lost = depth == 1 && conflicted(it);
            let commit = ok(&r) && !lost;
            // PostgreSQL's connection is reset as the outermost ends, in the same round trip:
            // what a function of the program's or a `do` block set in it (`set_config(...,
            // false)`, `pg_temp`) cannot all be read beforehand (FEEDBACK55.md, Q-708; branch
            // feedback55).
            let reset = if matches!(&*b, Backend::Postgres(_)) { format!("; {}", pg::RESET) } else { String::new() };
            let end = match (commit, depth == 1) {
                (true, true) => format!("commit{}", reset),
                (false, true) => format!("rollback{}", reset),
                (true, false) => format!("release savepoint quillo_tx_{}", depth),
                (false, false) => format!("rollback to savepoint quillo_tx_{d}; release savepoint quillo_tx_{d}", d = depth),
            };
            let ended = pooled_run(it, &b, move |c| c.batch(&end));
            // The commit itself can fail so (a serializable transaction's often does).
            let lost = lost || (depth == 1 && conflicted(it));
            tx_end(it, mark, commit && ended.is_ok());
            let done = {
                let mut g = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let i = g.iter().position(|t| Arc::ptr_eq(&t.backend, &b)).expect("pinned");
                g[i].depth -= 1;
                if g[i].depth == 0 {
                    Some(g.remove(i))
                } else {
                    None
                }
            };
            if let Some(TxConn { conn: Some(mut c), .. }) = done {
                c.watch_conflicts(false);
                // Reset with its end, or now where that failed; closed where that fails too.
                if reset.is_empty() || ended.is_ok() || reset_session(&mut c) {
                    pool.give_back(c);
                } else {
                    pool.discard(c);
                }
            }
            match ended {
                Err(e) if commit => (Ok(db_err(e)), lost),
                _ if lost && ok(&r) => (Ok(crate::txn::gave_up()), true),
                _ => (r, lost),
            }
        }
    }
}

// ---- shards -------------------------------------------------------------------------------

/// `db.shards([target, ...])`: data split over several databases by a key
/// (a customer's id, say): `.on(key)` is the shard for a key (a connection,
/// as `db.connect` gives), `.all(sql, ...)` asks every shard and puts the
/// rows together, `.migrate()` brings each up to date. Keys are spread by
/// jump consistent hashing, so a shard added later takes its share of keys
/// from each of the others, and moves no other key.
fn shards(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(Value::List(targets)) = args.first() else {
        return fail(line, "`db.shards` expects a list of paths or database URLs, one per shard");
    };
    if targets.is_empty() {
        return fail(line, "`db.shards` needs one shard at least");
    }
    let mut list = Vec::new();
    for t in targets.iter() {
        if let Value::Str(s) = t {
            if s.contains("{tenant}") {
                return fail(line, "a shard is one database: no {tenant} in it");
            }
        }
        match db_connect(it, vec![t.clone()], line)? {
            Value::Map(m) => match m.get(CONN_KEY) {
                Some(Value::Int(i)) => list.push(*i as usize),
                _ => return fail(line, "`db.shards`: not a connection"),
            },
            err => return Ok(err),
        }
    }
    let n = list.len();
    let mut conns = it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let label = format!("shards{}", conns.len());
    conns.push((label, DbSlot::Shards(Arc::new(list))));
    let mut m = Map::new();
    m.insert(CONN_KEY.into(), Value::Int(conns.len() as i64 - 1));
    // `query` and the rest say what to call instead.
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("db.on", shard_on),
        ("db.all", shard_all),
        ("db.migrate", migrate),
        ("db.query", query),
        ("db.one", one),
        ("db.exec", exec),
        ("db.transaction", transaction),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("db.").into(), Value::Builtin(name, *f));
    }
    m.insert("count".into(), Value::Int(n as i64));
    Ok(Value::map(m))
}

fn shard_list(it: &mut Interp, f: &str, line: u32) -> Result<Arc<Vec<usize>>, Ctrl> {
    let on = it.db_on.take();
    match slot_on(it, on) {
        Some(DbSlot::Shards(list)) => Ok(list),
        _ => fail(line, format!("`{}` is a function of shards (`db.shards([...])`)", f)),
    }
}

/// Which of `n` shards a key goes to: FNV-1a of its text, then jump
/// consistent hashing (Lamping and Veach), the same on every server.
pub fn shard_of(key: &Value, n: usize) -> usize {
    let text = key.display();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let (mut b, mut j): (i64, i64) = (-1, 0);
    while j < n as i64 {
        b = j;
        h = h.wrapping_mul(2862933555777941757).wrapping_add(1);
        j = ((b + 1) as f64 * ((1u64 << 31) as f64 / ((h >> 33) + 1) as f64)) as i64;
    }
    b as usize
}

/// `shards.on(key)`: the shard a key's data is on.
fn shard_on(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let list = shard_list(it, "on", line)?;
    match args.first() {
        None | Some(Value::Nil) => fail(line, "`.on(key)` needs a key: what the data is split by"),
        Some(k) => Ok(connection(list[shard_of(k, list.len())])),
    }
}

/// `shards.all(sql, ...params)`: the rows every shard gives, together.
fn shard_all(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let list = shard_list(it, "all", line)?;
    let sql = sql_arg(&args, "all", line)?;
    let mut rows = Vec::new();
    for (k, i) in list.iter().enumerate() {
        match select(it, Some(*i), sql.clone(), args[1..].to_vec(), None) {
            Ok(r) => rows.extend(r),
            Err(e) => return Ok(db_err(format!("shard {}: {}", k + 1, e))),
        }
    }
    Ok(Value::list(rows))
}

// ---- one transaction over several databases ----------------------------------------------

/// A database taking part in `db.atomic`.
struct Part {
    b: Arc<Backend>,
    kind: Kind,
    prepared: bool,
    /// Its transaction's id: the whole's, and its number (two databases on
    /// one server share the server's ids).
    gid: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Sqlite,
    Pg,
    // Not made in a build without MySQL (the phone's; branch mobile).
    #[cfg_attr(not(feature = "mysql"), allow(dead_code))]
    My,
}

/// How old a transaction prepared and not finished must be for recovery to
/// finish it (a younger one may still be finishing).
const IN_DOUBT_AFTER_MS: i64 = 60_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// A statement on a database, on a connection of its own (not a request's
/// transaction's): for the decisions of `db.atomic`, and for recovery.
fn apart(it: &mut Interp, b: &Arc<Backend>, sql: String, params: Vec<Value>) -> Result<Vec<Value>, String> {
    match &**b {
        Backend::Sqlite(m) => {
            let c = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if sql.trim_start().to_ascii_lowercase().starts_with("select") {
                rows(&c, &sql, &params, None)
            } else {
                let mut stmt = c.prepare(&sql).map_err(|e| e.to_string())?;
                bind(&mut stmt, &params)?;
                stmt.raw_execute().map_err(|e| e.to_string())?;
                Ok(Vec::new())
            }
        }
        _ => {
            let b = b.clone();
            it.unlocked(move || {
                let mut c = b.checkout_nested()?;
                let lower = sql.trim_start().to_ascii_lowercase();
                let r = if lower.starts_with("select") || lower.starts_with("xa recover") {
                    c.query(&sql, &params, None)
                } else if params.is_empty() {
                    c.batch(&sql).map(|_| Vec::new())
                } else {
                    c.exec(&sql, &params).map(|_| Vec::new())
                };
                b.give_back(c);
                r
            })
        }
    }
}

/// The system database's backend, for `db.atomic`'s decisions.
fn system_backend(it: &mut Interp) -> Result<Arc<Backend>, String> {
    match system_on(it, true) {
        Some(on) => backend_on(it, on),
        None => backend(it),
    }
}

fn decisions_table(it: &mut Interp, b: &Arc<Backend>) -> Result<(), String> {
    let sql = if b.is_mysql() {
        "create table if not exists _quillo_atomic (gid varchar(64) primary key, decision varchar(8) not null, at bigint not null)"
    } else {
        "create table if not exists _quillo_atomic (gid text primary key, decision text not null, at bigint not null)"
    };
    apart(it, b, sql.into(), Vec::new()).map(|_| ())
}

/// Records what became of a transaction, unless it was decided already: the
/// first to record it (the transaction, or recovery) decides it.
fn decide(it: &mut Interp, sys: &Arc<Backend>, gid: &str, decision: &str) -> Result<(), String> {
    apart(it, sys, "insert into _quillo_atomic (gid, decision, at) values (?, ?, ?)".into(), vec![Value::str(gid), Value::str(decision), Value::Int(now_ms())])
        .map(|_| ())
}

fn decided(it: &mut Interp, sys: &Arc<Backend>, gid: &str) -> Option<String> {
    let rows = apart(it, sys, "select decision from _quillo_atomic where gid = ?".into(), vec![Value::str(gid)]).ok()?;
    match rows.first() {
        Some(Value::Map(m)) => m.get("decision").map(|d| d.display()),
        _ => None,
    }
}

/// `db.atomic([db, other, ...], () => { ... })`: one transaction over
/// several databases: all of it is kept, or none. The function's writes go
/// to each as usual (`db.exec`, `other.exec`); when it returns normally,
/// each server database prepares its part (PostgreSQL's prepared
/// transactions, MySQL's XA), the decision to commit is recorded in the
/// system database, a SQLite database (one at most) commits, and the
/// prepared parts commit. If any part cannot prepare, or the function fails,
/// returns an error value or replies early, every part rolls back. A server
/// that stops between the decision and the end leaves prepared parts, which
/// recovery finishes as decided (see `recover`). PostgreSQL needs
/// `max_prepared_transactions` above 0.
fn atomic(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::List(dbs)), Some(f @ (Value::Func(_) | Value::Builtin(..)))) = (args.first(), args.get(1)) else {
        return fail(line, "`db.atomic` expects the databases and a function: db.atomic([db, analytics], () => { ... })");
    };
    let f = f.clone();
    // The databases: `db` itself, connections, and shards (each of them).
    let mut ons: Vec<Option<usize>> = Vec::new();
    for d in dbs.iter() {
        let Value::Map(m) = d else {
            return fail(line, format!("`db.atomic`: {} is not a database", d.type_name()));
        };
        match m.get(CONN_KEY) {
            Some(Value::Int(i)) => match slot_on(it, Some(*i as usize)) {
                Some(DbSlot::Shards(list)) => ons.extend(list.iter().map(|i| Some(*i))),
                _ => ons.push(Some(*i as usize)),
            },
            None if matches!(m.get("exec"), Some(Value::Builtin("db.exec", _))) => ons.push(None),
            None => return fail(line, "`db.atomic`: not a database (`db`, or what `db.connect` gives)"),
            Some(_) => return fail(line, "`db.atomic`: not a database"),
        }
    }
    let mut parts: Vec<Part> = Vec::new();
    for on in ons {
        let b = match backend_on(it, on) {
            Ok(b) => b,
            Err(e) => return Ok(db_err(e)),
        };
        if parts.iter().any(|p| Arc::ptr_eq(&p.b, &b)) {
            continue;
        }
        let kind = match &*b {
            Backend::Sqlite(_) => Kind::Sqlite,
            Backend::Postgres(_) => Kind::Pg,
            #[cfg(feature = "mysql")]
            Backend::Mysql(_) => Kind::My,
        };
        parts.push(Part { b, kind, prepared: false, gid: String::new() });
    }
    if parts.iter().filter(|p| p.kind == Kind::Sqlite).count() > 1 {
        return fail(line, "`db.atomic`: one SQLite database at most (it commits last; SQLite cannot prepare a transaction)");
    }
    if parts.iter().any(|p| it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|t| Arc::ptr_eq(&t.backend, &p.b))) || it.sqlite_tx > 0 {
        return fail(line, "`db.atomic` cannot begin inside a transaction already open on one of its databases");
    }
    let sys = match system_backend(it) {
        Ok(b) => b,
        Err(e) => return Ok(db_err(format!("db.atomic: no database to record decisions in ({})", e))),
    };
    if let Err(e) = decisions_table(it, &sys) {
        return Ok(db_err(format!("db.atomic: {}", e)));
    }
    let gid = format!("quillo_{}_{}", now_ms(), crate::service::new_id(16));
    for (k, p) in parts.iter_mut().enumerate() {
        p.gid = format!("{}_{}", gid, k);
    }
    // Each part begins.
    let mut begun: Vec<usize> = Vec::new();
    let mut failed = None;
    for (k, p) in parts.iter().enumerate() {
        let r = match p.kind {
            Kind::Sqlite => match &*p.b {
                Backend::Sqlite(m) => m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute_batch("savepoint quillo_tx").map_err(|e| e.to_string()),
                _ => unreachable!(),
            },
            _ => {
                match checkout_for_tx(it, &p.b) {
                    Ok(c) => {
                        it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(TxConn { backend: p.b.clone(), conn: Some(c), depth: 1 });
                        let begin = if p.kind == Kind::Pg { "begin".to_string() } else { format!("xa start '{}'", p.gid) };
                        pooled_run(it, &p.b, move |c| c.batch(&begin))
                    }
                    Err(e) => Err(e),
                }
            }
        };
        match r {
            Ok(()) => begun.push(k),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    let sqlite = parts.iter().enumerate().any(|(k, p)| p.kind == Kind::Sqlite && begun.contains(&k));
    if sqlite {
        it.sqlite_tx += 1;
        crate::httpd::hold_in_place(true);
    }
    let mark = tx_begin(it);
    it.wrote_req = Some(crate::httpd::request_number());
    let r = match &failed {
        None => it.call(&f, Vec::new(), line),
        Some(e) => Ok(db_err(e)),
    };
    let ok = failed.is_none() && matches!(&r, Ok(v) if !matches!(v, Value::Err(..)));
    let outcome = if ok { commit_parts(it, &mut parts, &sys, &gid) } else { Err(String::new()) };
    if outcome.is_err() {
        abort_parts(it, &mut parts, &begun);
    }
    if sqlite {
        crate::httpd::hold_in_place(false);
        it.sqlite_tx -= 1;
    }
    // The connections go back.
    for p in &parts {
        let taken = {
            let mut g = it.pg_tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            g.iter().position(|t| Arc::ptr_eq(&t.backend, &p.b)).map(|i| g.remove(i))
        };
        // Reset as any transaction's (FEEDBACK55.md, Q-708).
        if let Some(TxConn { conn: Some(mut c), .. }) = taken {
            if !reset_session(&mut c) {
                p.b.discard(c);
            } else {
                p.b.give_back(c);
            }
        }
    }
    tx_end(it, mark, outcome.is_ok());
    match outcome {
        Err(e) if ok => Ok(db_err(format!("db.atomic: {}", e))),
        _ => r,
    }
}

fn commit_parts(it: &mut Interp, parts: &mut [Part], sys: &Arc<Backend>, gid: &str) -> Result<(), String> {
    // 1. Every server database prepares its part.
    for k in 0..parts.len() {
        let (kind, b, pgid) = (parts[k].kind, parts[k].b.clone(), parts[k].gid.clone());
        let r = match kind {
            Kind::Sqlite => continue,
            Kind::Pg => {
                let sql = format!("prepare transaction '{}'", pgid);
                pooled_run(it, &b, move |c| c.batch(&sql))
            }
            Kind::My => {
                let (end, prep) = (format!("xa end '{}'", pgid), format!("xa prepare '{}'", pgid));
                pooled_run(it, &b, move |c| c.batch(&end).and_then(|_| c.batch(&prep)))
            }
        };
        match r {
            Ok(()) => parts[k].prepared = true,
            Err(e) => return Err(format!("a database could not prepare its part: {}", e)),
        }
    }
    // 2. The decision, recorded (first, or not at all: recovery may have
    // decided for a transaction it took to be abandoned). In a SQLite
    // database taking part, it is kept with that part, when it commits.
    let sys_is_part = parts.iter().any(|p| p.kind == Kind::Sqlite && Arc::ptr_eq(&p.b, sys));
    decide(it, sys, gid, "commit").map_err(|e| format!("the decision could not be recorded: {}", e))?;
    // 3. SQLite, last: then the decision stands.
    for p in parts.iter() {
        if let (Kind::Sqlite, Backend::Sqlite(m)) = (p.kind, &*p.b) {
            if let Err(e) = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute_batch("release quillo_tx") {
                if !sys_is_part {
                    let _ = apart(it, sys, "delete from _quillo_atomic where gid = ?".into(), vec![Value::str(gid)]);
                }
                return Err(e.to_string());
            }
        }
    }
    // 4. The prepared parts commit. One that cannot now is committed by
    // recovery, as decided.
    for p in parts.iter() {
        let sql = match p.kind {
            Kind::Sqlite => continue,
            Kind::Pg => format!("commit prepared '{}'", p.gid),
            Kind::My => format!("xa commit '{}'", p.gid),
        };
        let b = p.b.clone();
        if let Err(e) = pooled_run(it, &b, move |c| c.batch(&sql)) {
            eprintln!("quillo: db.atomic {}: a part did not commit yet ({}); recovery will commit it", gid, e);
            return Ok(());
        }
    }
    let _ = apart(it, sys, "delete from _quillo_atomic where gid = ?".into(), vec![Value::str(gid)]);
    Ok(())
}

fn abort_parts(it: &mut Interp, parts: &mut [Part], begun: &[usize]) {
    for (k, p) in parts.iter().enumerate() {
        if !begun.contains(&k) {
            continue;
        }
        match (p.kind, &*p.b) {
            (Kind::Sqlite, Backend::Sqlite(m)) => {
                let _ = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner).execute_batch("rollback to quillo_tx; release quillo_tx");
            }
            (Kind::Pg, _) => {
                let sql = if p.prepared { format!("rollback prepared '{}'", p.gid) } else { "rollback".to_string() };
                let b = p.b.clone();
                let _ = pooled_run(it, &b, move |c| c.batch(&sql));
            }
            (Kind::My, _) => {
                let (prepared, g) = (p.prepared, p.gid.clone());
                let b = p.b.clone();
                let _ = pooled_run(it, &b, move |c| {
                    if !prepared {
                        let _ = c.batch(&format!("xa end '{}'", g));
                    }
                    c.batch(&format!("xa rollback '{}'", g))
                });
            }
            _ => {}
        }
    }
}

/// Finishes transactions of `db.atomic` left prepared by a server that
/// stopped part way (older than a minute): committed if the decision to
/// commit was recorded, rolled back otherwise (recording that, so that a
/// late decision cannot commit them after). Also forgets decisions older
/// than a day.
pub fn recover(it: &mut Interp) {
    let Some(on) = system_on(it, false) else { return };
    let Ok(sys) = backend_on(it, on) else { return };
    let mut servers: Vec<Arc<Backend>> = Vec::new();
    if let Some(DbSlot::One(b)) = it.db.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
        servers.push(b);
    }
    for (_, s) in it.db_conns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter() {
        if let DbSlot::One(b) = s {
            if !servers.iter().any(|x| Arc::ptr_eq(x, b)) {
                servers.push(b.clone());
            }
        }
    }
    let now = now_ms();
    for b in servers {
        let (list, mysql) = match &*b {
            Backend::Sqlite(_) => continue,
            Backend::Postgres(_) => ("select gid from pg_prepared_xacts where gid like 'quillo%'", false),
            #[cfg(feature = "mysql")]
            Backend::Mysql(_) => ("xa recover", true),
        };
        let Ok(rows) = apart(it, &b, list.into(), Vec::new()) else { continue };
        for row in rows {
            let Value::Map(m) = row else { continue };
            // A part's id: quillo_<when made>_<random>_<part>.
            let Some(part) = m.get(if mysql { "data" } else { "gid" }).map(|v| v.display()) else { continue };
            let bits: Vec<&str> = part.split('_').collect();
            let (Some(born), 4) = (bits.get(1).and_then(|t| t.parse::<i64>().ok()), bits.len()) else { continue };
            if bits[0] != "quillo" || now - born < IN_DOUBT_AFTER_MS {
                continue;
            }
            let whole = bits[..3].join("_");
            let decision = match decided(it, &sys, &whole) {
                Some(d) => d,
                None => match decide(it, &sys, &whole, "abort") {
                    Ok(()) => "abort".into(),
                    Err(_) => match decided(it, &sys, &whole) {
                        Some(d) => d,
                        None => continue,
                    },
                },
            };
            let sql = match (decision.as_str(), mysql) {
                ("commit", false) => format!("commit prepared '{}'", part),
                ("commit", true) => format!("xa commit '{}'", part),
                (_, false) => format!("rollback prepared '{}'", part),
                (_, true) => format!("xa rollback '{}'", part),
            };
            match apart(it, &b, sql, Vec::new()) {
                Ok(_) => eprintln!("quillo: db.atomic {} finished by recovery: {}", part, if decision == "commit" { "committed" } else { "rolled back" }),
                Err(e) => eprintln!("quillo: db.atomic {}: recovery could not finish it yet: {}", part, e),
            }
        }
    }
    let _ = apart(it, &sys, "delete from _quillo_atomic where at < ?".into(), vec![Value::Int(now - 86_400_000)]);
}

#[cfg(test)]
mod tests {
    use super::kind_of;

    fn values_tuple(sql: &str, width: usize) -> Option<String> {
        crate::sql::scan(crate::sql::Dialect::Postgres, sql).values_tuple(width)
    }

    // The first words read in place, as every `db.exec` reads them: any case, past comments
    // and spaces, and a transaction's control found among a script's statements.
    // FEEDBACK49.md, Q-601: a connection the server ended is a kind of its own.
    #[test]
    fn a_connection_the_server_ended_is_told_as_one() {
        for m in [
            "db: terminating connection due to administrator command [57P01]",
            "db: the database system is shutting down [57P03]",
            "db: connection failure [08006]",
            "db: connection closed",
            "db: Broken pipe (os error 32)",
            "db: error connecting to server: Connection refused (os error 61)",
            "db: server closed the connection unexpectedly",
        ] {
            assert_eq!(kind_of(m), Some("connection"), "{}", m);
        }
        for m in ["db: syntax error at or near \"x\" [42601]", "db: relation \"t\" does not exist [42P01]", "db: duplicate key value violates unique constraint [23505]"] {
            assert_ne!(kind_of(m), Some("connection"), "{}", m);
        }
    }

    #[test]
    fn the_first_words_are_read_in_place() {
        // As each database reads them now (src/sql.rs; FEEDBACK55.md, Q-746; branch feedback55).
        for d in crate::sql::Dialect::ALL {
            let head = |sql: &str| crate::sql::scan(d, sql).first_words().map(|(a, b)| format!("{} {}", a, b).trim().to_ascii_lowercase());
            assert_eq!(head("  -- a note\n /* and */ Start   Transaction;").as_deref(), Some("start transaction"));
            assert_eq!(head("begin").as_deref(), Some("begin"));
            assert_eq!(head(""), None);
            assert_eq!(head("-- only a note"), None);
            assert_eq!(head("/* never closed"), None);
            assert_eq!(head("insert(1)").as_deref(), Some("insert"));
            assert_eq!(head("SET Local x = 1").as_deref(), Some("set local"));
            assert_eq!(head(" ; "), None);
            for (sql, alone, server) in [
                ("BEGIN", true, true),
                ("  commit ;", true, true),
                ("Start Transaction", true, true),
                ("start", false, false),
                ("starting", false, false),
                ("beginning()", false, false),
                ("ROLLBACK to savepoint a", true, true),
                ("select 'begin'", false, false),
                ("-- begin\nselect 1", false, false),
                ("begin; insert into t values (1)", false, true),
                ("insert into t values (1); COMMIT", false, true),
                ("insert into t values (';begin')", false, false),
                ("", false, false),
                (";;", false, false),
            ] {
                let s = crate::sql::scan(d, sql);
                assert_eq!(s.tx_control(false), alone, "{:?} {}", d, sql);
                assert_eq!(s.tx_control(true), server, "{:?} {}", d, sql);
            }
            assert_eq!(crate::sql::scan(d, "a; ; -- x\n; b").stmts.len(), 2);
        }
    }

    // An insert of one row's `?`s, with nothing after them, goes as many rows a statement;
    // anything else a row at a time (branch feedback40-43).
    #[test]
    fn which_statements_take_many_rows() {
        assert_eq!(values_tuple("insert into t (a, b) values (?, ?)", 2).as_deref(), Some("insert into t (a, b) values "));
        assert_eq!(values_tuple("  INSERT INTO t VALUES (?,?) ; ", 2).as_deref(), Some("  INSERT INTO t values "));
        assert_eq!(values_tuple("insert into t values(?)", 1).as_deref(), Some("insert into t values "));
        // Read by the tokenizer: a quoted name or a comment no longer misleads it (FEEDBACK55.md;
        // branch feedback55).
        assert_eq!(values_tuple("insert into \"values\" values (?)", 1).as_deref(), Some("insert into \"values\" values "));
        assert_eq!(values_tuple("insert into t (a) values (?) -- note", 1).as_deref(), Some("insert into t (a) values "));
        assert_eq!(values_tuple("insert /* values (?) */ into t values (?, ?)", 2).as_deref(), Some("insert /* values (?) */ into t values "));
        for (sql, width) in [
            ("insert into t values (?, ?)", 3),                          // not the rows' width
            ("insert into t values (?, 1)", 2),                          // a value written in
            ("insert into t values (?, ?) on conflict do nothing", 2),   // something after it
            ("insert into t values (?, ?) returning id", 2),
            ("insert into t select ?, ?", 2),
            ("update t set a = ? where b = ?", 2),
            ("insert into t (a) values (?, ?), (?, ?)", 2),               // rows of its own
            ("insert into t (a) values (?,)", 1),
            ("insert into t (a) values ()", 0),
            ("insert into t (a, b) values (?, :b)", 2),
            ("insert into t (a) values (?); insert into u values (?)", 1),
            ("insert into t (a) select ? where 'values (?)' = ''", 1),
            ("ins", 1),
        ] {
            assert_eq!(values_tuple(sql, width), None, "{}", sql);
        }
    }

    // A job's key fits MySQL's column, and tells long names apart (found in review).
    #[test]
    fn a_jobs_key_fits_the_column() {
        let key = |n: &str| super::job_key(n).into_owned();
        assert_eq!(key("nightly"), "nightly");
        let at = "é".repeat(255);
        assert_eq!(key(&at), at);
        let (a, b) = (format!("{}a", "é".repeat(255)), format!("{}b", "é".repeat(255)));
        assert!(key(&a).chars().count() <= 255 && key(&b).chars().count() <= 255);
        assert_ne!(key(&a), key(&b));
        assert!(key(&a).starts_with(&"é".repeat(180)));
        assert_eq!(key(&a), key(&a));
    }

    // A replica's refusal of a statement that writes (FEEDBACK55.md, Q-750).
    #[test]
    fn a_replicas_refusal_of_a_write_is_told_apart() {
        for e in [
            "db: cannot execute nextval() in a read-only transaction [25006]",
            "db: recovery is in progress [55000]",
            "Server error: `ERROR HY000 (1290): The MariaDB server is running with the --read-only option so it cannot execute this statement'",
            "Server error: `ERROR 25006 (1792): Cannot execute statement in a READ ONLY transaction.'",
            "Server error: `ERROR HY000 (1836): Running in read-only mode'",
        ] {
            assert!(super::read_only_refusal(e), "{}", e);
        }
        for e in ["db: relation \"t\" does not exist [42P01]", "db: object not in prerequisite state [55000]", "db: connection closed", "a value of 25006", ""] {
            assert!(!super::read_only_refusal(e), "{}", e);
        }
    }

    // Each database's own words for the same failures (FEEDBACK5.md, Q-053;
    // branch stays-fixes).
    #[test]
    fn kinds_of_failures() {
        let cases = [
            ("UNIQUE constraint failed: booking_nights.listing_id, booking_nights.day", Some("unique")),
            ("duplicate key value violates unique constraint \"booking_nights_pkey\" (Key (listing_id, day)=(3, 2026-11-05) already exists.)", Some("unique")),
            ("Server error: `ERROR 23000 (1062): Duplicate entry '3-2026-11-05' for key 'PRIMARY''", Some("unique")),
            ("FOREIGN KEY constraint failed", Some("foreign_key")),
            ("insert or update on table \"bookings\" violates foreign key constraint \"bookings_listing_id_fkey\"", Some("foreign_key")),
            ("Server error: `ERROR 23000 (1452): Cannot add or update a child row: a foreign key constraint fails'", Some("foreign_key")),
            ("NOT NULL constraint failed: users.email", Some("not_null")),
            ("null value in column \"email\" of relation \"users\" violates not-null constraint", Some("not_null")),
            ("Server error: `ERROR 23000 (1048): Column 'email' cannot be null'", Some("not_null")),
            ("CHECK constraint failed: rating", Some("check")),
            ("new row for relation \"reviews\" violates check constraint \"reviews_rating_check\"", Some("check")),
            ("database is locked", Some("conflict")),
            ("could not serialize access due to concurrent update", Some("conflict")),
            ("Server error: `ERROR 40001 (1213): Deadlock found when trying to get lock'", Some("conflict")),
            ("no such table: listings", None),
            ("near \"selct\": syntax error", None),
            // PostgreSQL in another language: by its SQLSTATE (FEEDBACK43.md, Q-493; branch
            // feedback40-43).
            ("doppelter Schlüsselwert verletzt Unique-Constraint »seats_pkey« (Schlüssel »(k)=(1)« existiert bereits.) [23505]", Some("unique")),
            ("Einfügen oder Aktualisieren in Tabelle »b« verletzt Fremdschlüssel-Constraint »b_a_fkey« [23503]", Some("foreign_key")),
            ("NULL-Wert in Spalte »email« verletzt Not-Null-Constraint [23502]", Some("not_null")),
            ("neue Zeile verletzt Check-Constraint »r_check« [23514]", Some("check")),
            ("konnte Zugriff nicht serialisieren [40001]", Some("conflict")),
            ("Verklemmung (Deadlock) entdeckt [40P01]", Some("conflict")),
            ("konnte Sperre nicht setzen [55P03]", Some("conflict")),
            ("Relation »x« existiert nicht [42P01]", None),
            ("a value [1, 2] in brackets", None),
        ];
        for (msg, want) in cases {
            assert_eq!(kind_of(msg), want, "{}", msg);
        }
    }
}
