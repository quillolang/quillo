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
    match slot_on(it, None) {
        Some(DbSlot::One(b)) => b.is_mysql(),
        Some(DbSlot::PerTenant(t)) => match &t.target {
            Target::Text(s) => is_mysql_url(s),
            Target::Fn(_) => t.open.lock().unwrap_or_else(|e| e.into_inner()).values().next().map_or(false, |b| b.is_mysql()),
        },
        Some(DbSlot::Shards(_)) => false,
        None => std::env::var("QUILLO_DB").map_or(false, |t| is_mysql_url(&t)),
    }
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

impl Pinned {
    pub(crate) fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        match self {
            Pinned::Pg(c) => c.query(sql, params, limit),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.query(sql, params, limit),
        }
    }

    pub(crate) fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
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
    let mut sets = replica_sets().lock().unwrap_or_else(|e| e.into_inner());
    sets.retain(|(w, _)| w.strong_count() > 0);
    sets.iter().find(|(w, _)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(b))).map(|(_, r)| r.clone())
}

/// Whether a statement only reads: `select` or `with` (writing nothing),
/// and no `for update` or `for share` (which lock, on the primary).
fn only_reads(sql: &str) -> bool {
    let lower = sql.trim_start().to_ascii_lowercase();
    let first = lower.split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("");
    (first == "select" || first == "with")
        && crate::sqlinfo::tables_of(sql).writes.is_empty()
        && !["for update", "for share", "for no key update", "for key share", "lock in share mode"].iter().any(|k| lower.contains(k))
}

/// Where a read goes: a replica of `b`, unless it is in a transaction on
/// `b`, or this request wrote (it reads what it wrote, on the primary).
fn reader(it: &Interp, b: &Arc<Backend>, sql: &str) -> Option<(Arc<Replicas>, usize)> {
    let reps = replicas_of(b)?;
    if it.wrote_req == Some(crate::httpd::request_number()) || !only_reads(sql) {
        return None;
    }
    if it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|t| Arc::ptr_eq(&t.backend, b)) {
        return None;
    }
    let n = reps.list.len();
    let start = reps.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = std::time::Instant::now();
    let down = reps.down.lock().unwrap_or_else(|e| e.into_inner());
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
}

/// Where a tenant's database is: `{tenant}` in a path or URL, or what a
/// function of the tenant's name gives (a database of any kind each).
enum Target {
    Text(String),
    Fn(Value),
}

impl TenantDbs {
    fn new(target: Target) -> TenantDbs {
        TenantDbs { target, open: Mutex::new(Default::default()), migrations: Mutex::new(None) }
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

fn db_err(e: impl std::fmt::Display) -> Value {
    Value::err(Value::str(format!("db: {}", e)))
}

fn connect(path: &str) -> Result<Connection, rusqlite::Error> {
    let c = if path == ":memory:" { Connection::open_in_memory()? } else { Connection::open(path)? };
    c.execute_batch("pragma foreign_keys = on")?;
    wal(&c)?;
    Ok(c)
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
fn open_backend(it: &mut Interp, target: &str) -> Result<Backend, String> {
    open_backend_as(it, target, true)
}

/// `open_backend`; with `make` false, a Postgres schema (`?schema=`) must
/// exist already.
fn open_backend_as(it: &mut Interp, target: &str, make: bool) -> Result<Backend, String> {
    if is_mysql_url(target) {
        #[cfg(feature = "mysql")]
        {
            let url = target.to_string();
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
    let slot = it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).get(i).map(|(_, s)| s.clone());
    match slot {
        Some(DbSlot::One(b)) => Ok(b),
        Some(DbSlot::PerTenant(t)) => tenant_backend(it, &t),
        Some(DbSlot::Shards(_)) => Err("these are shards: `.on(key)` is the one for a key, `.all(sql)` asks every one".into()),
        None => Err("this connection is not one of this program's".into()),
    }
}

/// The slot a call is on, if it has been opened.
fn slot_on(it: &Interp, on: Option<usize>) -> Option<DbSlot> {
    match on {
        None => it.db.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        Some(i) => it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).get(i).map(|(_, s)| s.clone()),
    }
}

/// The program's database (the tenant's, for one per tenant), opening the
/// default one if needed.
fn backend(it: &mut Interp) -> Result<Arc<Backend>, String> {
    let slot = it.db.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match slot {
        Some(DbSlot::One(b)) => return Ok(b),
        Some(DbSlot::PerTenant(t)) => return tenant_backend(it, &t),
        Some(DbSlot::Shards(_)) => return Err("the program's database cannot be shards".into()),
        None => {}
    }
    let target = std::env::var("QUILLO_DB").unwrap_or_else(|_| ":memory:".into());
    if target.contains("{tenant}") {
        let t = Arc::new(TenantDbs::new(Target::Text(target)));
        *it.db.lock().unwrap_or_else(|e| e.into_inner()) = Some(DbSlot::PerTenant(t.clone()));
        return tenant_backend(it, &t);
    }
    let b = Arc::new(open_backend(it, &target)?);
    let mut slot = it.db.lock().unwrap_or_else(|e| e.into_inner());
    match slot.get_or_insert(DbSlot::One(b)) {
        DbSlot::One(b) => Ok(b.clone()),
        _ => unreachable!("set just above"),
    }
}

/// The current tenant's database, opened (and migrated) on first use.
fn tenant_backend(it: &mut Interp, dbs: &Arc<TenantDbs>) -> Result<Arc<Backend>, String> {
    let Some(name) = it.tenant() else {
        return Err("the database is one per tenant ({tenant} in its target), and this code runs for none: see tenant_from and with_tenant".into());
    };
    {
        let mut open = dbs.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = open.get_index_of(&*name) {
            // The most recently used go last.
            let last = open.len() - 1;
            open.move_index(i, last);
            return Ok(open[last].clone());
        }
    }
    let target = tenant_target(it, dbs, &name)?;
    // A request's host can name any tenant, so only the program makes one
    // (`with_tenant`, when signing a tenant up): made-up names make nothing.
    let make = it.tenant_named;
    if !is_server_url(&target) && target != ":memory:" {
        let full = if Path::new(&target).is_absolute() { Path::new(&target).to_path_buf() } else { it.base_dir.join(&target) };
        if !make && !full.exists() {
            return Err(format!("there is no tenant {:?}: with_tenant({:?}, ...) makes its database", &*name, &*name));
        }
        if let Some(dir) = full.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let b = Arc::new(open_backend_as(it, &target, make).map_err(|e| format!("tenant {}: {}", name, e))?);
    let is_default = matches!(&*it.db.lock().unwrap_or_else(|e| e.into_inner()), Some(DbSlot::PerTenant(t)) if Arc::ptr_eq(t, dbs));
    let dir = dbs.migrations.lock().unwrap_or_else(|e| e.into_inner()).clone().or_else(|| if is_default { it.site.migrations.clone() } else { None });
    if let Some(dir) = dir {
        let mb = b.clone();
        let applied = it.unlocked(move || crate::migrate::run(&mb, &dir)).map_err(|e| format!("tenant {}: {}", name, e))?;
        for m in applied {
            eprintln!("quillo: applied migration {} for tenant {}", m, name);
        }
    }
    let mut open = dbs.open.lock().unwrap_or_else(|e| e.into_inner());
    // Each open server tenant keeps connections (a few each).
    let most = if is_server_url(&target) { OPEN_PG_TENANTS } else { OPEN_TENANTS };
    while open.len() >= most {
        open.shift_remove_index(0);
    }
    Ok(open.entry(name.to_string()).or_insert(b).clone())
}

/// Where tenant `name`'s database is: its template filled in, or what the
/// function gives for it (which may not use that database itself).
fn tenant_target(it: &mut Interp, dbs: &Arc<TenantDbs>, name: &str) -> Result<String, String> {
    match tenant_target_or_none(it, dbs, name)? {
        Some(t) => Ok(t),
        None => Err(format!("tenant {}: the function for tenants' databases gave null, not a path or a URL", name)),
    }
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
        Ok(Value::Err(e)) => Err(format!("tenant {}: {}", name, e.display())),
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
        if std::time::Instant::now() >= until {
            return Err(pool.none_free());
        }
        // Made inside the future: a timer belongs to the runtime that runs it.
        if crate::httpd::await_elsewhere(Box::pin(async move { tokio::time::sleep(pause).await })).is_none() {
            std::thread::sleep(pause);
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
    let pinned = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).iter_mut().find(|tx| Arc::ptr_eq(&tx.backend, b)).and_then(|tx| tx.conn.take());
    match pinned {
        Some(mut c) => {
            let (c, r) = it.unlocked(move || {
                let r = f(&mut c);
                (c, r)
            });
            if let Some(tx) = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).iter_mut().find(|tx| Arc::ptr_eq(&tx.backend, b)) {
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
    let pinned = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|t| Arc::ptr_eq(&t.backend, b));
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
        // SQLite has no decimals: the text, exactly (see `Col::Dec`).
        Value::Dec(d) => Sql::Text(d.to_string()),
        Value::Str(s) => Sql::Text(s.to_string()),
        Value::Html(h) => Sql::Text(h.to_string()),
        other => Sql::Text(other.to_json().to_string()),
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
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::str(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => Value::str(String::from_utf8_lossy(b)),
    }
}

/// Binds `?` parameters from the arguments, or `:name` parameters from a
/// single map argument. As with Postgres and MySQL, every name needs a
/// value; keys the query does not use are left alone, so one map can serve
/// queries put together from optional parts.
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
                    None => {}
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

pub(crate) fn rows(conn: &Connection, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    bind(&mut stmt, params)?;
    let cols: Vec<(String, Col)> = stmt.columns().iter().map(|c| (c.name().to_string(), col_kind(c.decl_type()))).collect();
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
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
pub(crate) fn announce(it: &mut Interp, sql: &str, on: Option<usize>) {
    let t = crate::sqlinfo::tables_of(sql);
    let prefix = match on.and_then(|i| it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).get(i).map(|(l, _)| l.clone())) {
        Some(label) => format!("db@{}:", label),
        None => "db:".to_string(),
    };
    if it.reads.is_some() {
        for r in &t.reads {
            it.watch(format!("{}{}", prefix, r));
        }
    }
    if !t.writes.is_empty() {
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
    if dbs.open.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&*name) {
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
                Err(e) => return Ok(Err(format!("replica {}: {}", u, e))),
            }
        }
        if !list.is_empty() {
            let n = list.len();
            let reps = Arc::new(Replicas { list, next: Default::default(), down: Mutex::new(vec![None; n]) });
            replica_sets().lock().unwrap_or_else(|e| e.into_inner()).push((Arc::downgrade(&b), reps));
        }
        return Ok(Ok(DbSlot::One(b)));
    }
    match args.first() {
        Some(Value::Str(t)) if t.contains("{tenant}") => Ok(Ok(DbSlot::PerTenant(Arc::new(TenantDbs::new(Target::Text(t.to_string())))))),
        Some(Value::Str(t)) => Ok(open_backend(it, t).map(|b| DbSlot::One(Arc::new(b)))),
        // Where each tenant's database is, opened when its tenant first uses it.
        Some(fv @ (Value::Func(_) | Value::Builtin(..))) => Ok(Ok(DbSlot::PerTenant(Arc::new(TenantDbs::new(Target::Fn(fv.clone())))))),
        other => fail(
            line,
            format!("`{}` expects a path, a URL, or a function giving one for each tenant, got {}", f, other.map_or("nothing", |v| v.type_name())),
        ),
    }
}

fn open(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match slot_for(it, &args, "db.open", line)? {
        Ok(slot) => {
            *it.db.lock().unwrap_or_else(|e| e.into_inner()) = Some(slot);
            Ok(Value::Nil)
        }
        Err(e) => Ok(db_err(e)),
    }
}

/// `db.connect(target)`: another database, as a map of the same functions
/// as `db` (`analytics.query(...)`). The same target again (from a route,
/// say) is the same connection, not a new pool.
fn db_connect(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let label = match args.first() {
        // A label for its live updates' topics, the same on every server (and no secret in it).
        Some(Value::Str(t)) => crate::crypto::hex(&<sha2::Sha256 as sha2::Digest>::digest(t.as_bytes()))[..12].to_string(),
        _ => format!("fn{}", it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).len()),
    };
    if matches!(args.first(), Some(Value::Str(_))) {
        if let Some(i) = it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).iter().position(|(l, _)| *l == label) {
            return Ok(connection(i));
        }
    }
    let slot = match slot_for(it, &args, "db.connect", line)? {
        Ok(s) => s,
        Err(e) => return Ok(db_err(e)),
    };
    let mut conns = it.db_conns.lock().unwrap_or_else(|e| e.into_inner());
    conns.push((label, slot));
    Ok(connection(conns.len() - 1))
}

/// A connection as programs see it: `db`'s functions, and its number.
fn connection(i: usize) -> Value {
    let mut m = Map::new();
    m.insert(CONN_KEY.into(), Value::Int(i as i64));
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("db.exec", exec),
        ("db.query", query),
        ("db.one", one),
        ("db.transaction", transaction),
        ("db.migrate", migrate),
        ("db.tenants", tenants),
        ("db.tenant_exists", tenant_exists),
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
        *t.migrations.lock().unwrap_or_else(|e| e.into_inner()) = Some(full.clone());
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

/// `db.exec(sql, ...params)`: `{ changes, last_id }`. Without parameters the
/// text may hold several statements (a schema, for example).
fn exec(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let sql = sql_arg(&args, "db.exec", line)?;
    it.wrote_req = Some(crate::httpd::request_number());
    let params = args[1..].to_vec();
    let b = match backend_on(it, on) {
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
                pooled_run(it, &b, move |c| c.exec(&sql, &params))
            }
        },
        #[cfg(feature = "mysql")]
        Backend::Mysql(_) => {
            let sql = sql.clone();
            pooled_run(it, &b, move |c| c.exec(&sql, &params))
        }
    };
    if res.is_ok() {
        announce(it, &sql, on);
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
    if let Some(i) = it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).iter().position(|(l, _)| l == SYSTEM) {
        return Some(Some(i));
    }
    let slot = it.db.lock().unwrap_or_else(|e| e.into_inner()).clone();
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
                if matches!(&*it.db.lock().unwrap_or_else(|e| e.into_inner()), Some(DbSlot::One(_))) {
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
    let mut conns = it.db_conns.lock().unwrap_or_else(|e| e.into_inner());
    conns.push((SYSTEM.to_string(), DbSlot::One(Arc::new(b))));
    Some(Some(conns.len() - 1))
}

/// Whether a program uses `db.atomic` (then transactions it left prepared
/// are looked after: see `recover`).
pub fn uses_atomic(it: &Interp) -> bool {
    format!("{:?}", it.program).contains("\"atomic\"")
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
    let mut conns = it.db_conns.lock().unwrap_or_else(|e| e.into_inner());
    conns.retain(|(l, _)| l != SYSTEM);
    conns.push((SYSTEM.to_string(), DbSlot::One(Arc::new(b))));
    Ok(Value::Nil)
}

/// `db.exec` / `db.query` on connection `on` (None: the program's).
pub(crate) fn exec_on(it: &mut Interp, on: Option<usize>, args: Vec<Value>) -> R {
    it.db_on = on;
    exec(it, args, 0)
}

pub(crate) fn query_on(it: &mut Interp, on: Option<usize>, args: Vec<Value>) -> R {
    it.db_on = on;
    query(it, args, 0)
}

pub(crate) fn kind_on(it: &mut Interp, on: Option<usize>) -> bool {
    backend_on(it, on).map_or(false, |b| b.is_mysql())
}

/// Whether this server runs a scheduled job's run due at `at`: with a
/// database for Quillo's own tables (which other servers may share), only
/// if no one recorded that run before; otherwise always.
pub(crate) fn claim_job(it: &mut Interp, name: &str, at: u64) -> bool {
    let Some(on) = system_on(it, true) else { return true };
    let mysql = kind_on(it, on);
    let table = if mysql {
        "create table if not exists _quillo_jobs (name varchar(255) primary key, at bigint not null)"
    } else {
        "create table if not exists _quillo_jobs (name text primary key, at bigint not null)"
    };
    if !matches!(exec_on(it, on, vec![Value::str(table)]), Ok(Value::Map(_))) {
        return true;
    }
    // A later run claims it (MySQL counts a row it leaves as it was as no change).
    let sql = if mysql {
        "insert into _quillo_jobs (name, at) values (?, ?) on duplicate key update at = if(at < values(at), values(at), at)"
    } else {
        "insert into _quillo_jobs (name, at) values (?, ?) \
         on conflict (name) do update set at = excluded.at where _quillo_jobs.at < excluded.at"
    };
    match exec_on(it, on, vec![Value::str(sql), Value::str(name), Value::Int(at as i64)]) {
        Ok(Value::Map(m)) => !matches!(m.get("changes"), Some(Value::Int(0))),
        _ => true,
    }
}

// ---- calls made later (src/service.rs), kept with Quillo's own tables ----

fn later_table(it: &mut Interp, on: Option<usize>) -> bool {
    let sql = if kind_on(it, on) {
        "create table if not exists _quillo_later (id varchar(64) primary key, service varchar(255) not null, fn varchar(255) not null, \
         args text not null, tenant varchar(255) not null, rid varchar(64) not null, at bigint not null, tries bigint not null, error text)"
    } else {
        "create table if not exists _quillo_later (id text primary key, service text not null, fn text not null, \
         args text not null, tenant text not null, rid text not null, at bigint not null, tries bigint not null, error text)"
    };
    matches!(exec_on(it, on, vec![Value::str(sql)]), Ok(Value::Map(_)))
}

/// Keeps a call made later; false when there is no database to keep it in.
pub(crate) fn later_put(it: &mut Interp, c: &crate::service::Later) -> bool {
    let Some(on) = system_on_for(it, true, true) else { return false };
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

/// Calls made later that are due.
pub(crate) fn later_due(it: &mut Interp, now: i64, limit: usize) -> Vec<crate::service::Later> {
    let Some(on) = system_on(it, false) else { return Vec::new() };
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
pub(crate) fn later_take(it: &mut Interp, id: &str, was: i64, until: i64) -> bool {
    let Some(on) = system_on(it, false) else { return false };
    let sql = "update _quillo_later set at = ? where id = ? and at = ?";
    match exec_on(it, on, vec![Value::str(sql), Value::Int(until), Value::str(id), Value::Int(was)]) {
        Ok(Value::Map(m)) => !matches!(m.get("changes"), Some(Value::Int(0))),
        _ => false,
    }
}

pub(crate) fn later_done(it: &mut Interp, id: &str) {
    if let Some(on) = system_on(it, false) {
        let _ = exec_on(it, on, vec![Value::str("delete from _quillo_later where id = ?"), Value::str(id)]);
    }
}

pub(crate) fn later_again(it: &mut Interp, id: &str, at: i64, tries: i64, why: &str) {
    if let Some(on) = system_on(it, false) {
        let sql = "update _quillo_later set at = ?, tries = ?, error = ? where id = ?";
        let _ = exec_on(it, on, vec![Value::str(sql), Value::Int(at), Value::Int(tries), Value::str(why), Value::str(id)]);
    }
}

fn select(it: &mut Interp, on: Option<usize>, sql: String, params: Vec<Value>, limit: Option<usize>) -> Result<Vec<Value>, String> {
    if it.prefetching.is_some() && !only_reads(&sql) && it.no_writes("SQL that writes", 0).is_err() {
        return Err("not while the page is prefetched".into());
    }
    let b = backend_on(it, on)?;
    // A read, to a replica: if it fails there, the primary answers (and the
    // replica is left out for ten seconds).
    if let Some((reps, i)) = reader(it, &b, &sql) {
        let r = reps.list[i].clone();
        let (q, p) = (sql.clone(), params.clone());
        match pooled_run(it, &r, move |c| c.query(&q, &p, limit)) {
            Ok(rows) => {
                announce(it, &sql, on);
                return Ok(rows);
            }
            Err(e) => {
                eprintln!("quillo: a read replica failed ({}); reading from the primary", e);
                reps.down.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(std::time::Instant::now() + std::time::Duration::from_secs(10));
            }
        }
    }
    if !only_reads(&sql) {
        it.wrote_req = Some(crate::httpd::request_number());
    }
    let res = match &*b {
        Backend::Sqlite(m) => rows(&m.lock().unwrap_or_else(|e| e.into_inner()), &sql, &params, limit),
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
    if res.is_ok() {
        announce(it, &sql, on);
    }
    res
}

/// `db.query(sql, ...params)`: every row, as maps from column to value.
fn query(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let sql = sql_arg(&args, "db.query", line)?;
    Ok(select(it, on, sql, args[1..].to_vec(), None).map(Value::list).unwrap_or_else(db_err))
}

/// `db.one(sql, ...params)`: the first row, or null.
fn one(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let sql = sql_arg(&args, "db.one", line)?;
    Ok(select(it, on, sql, args[1..].to_vec(), Some(1))
        .map(|r| r.into_iter().next().unwrap_or(Value::Nil))
        .unwrap_or_else(db_err))
}

/// `db.transaction(() => { ... })`: commits if the function returns normally,
/// rolls back if it fails, returns an error value, or replies early.
/// Transactions nest (as savepoints).
fn transaction(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let on = it.db_on.take();
    let f = match args.first() {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => {
            return fail(line, format!("`db.transaction` expects a function, got {}", other.map_or("nothing", |v| v.type_name())))
        }
    };
    let b = match backend_on(it, on) {
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
        _ => {
            let pool = &*b;
            // Keep one connection for this request until the outermost
            // transaction ends.
            let pinned = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|t| Arc::ptr_eq(&t.backend, &b));
            if !pinned {
                let c = match checkout_for_tx(it, &b) {
                    Ok(c) => c,
                    Err(e) => return Ok(db_err(e)),
                };
                it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).push(TxConn { backend: b.clone(), conn: Some(c), depth: 0 });
            }
            let depth = {
                let mut g = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner());
                let tx = g.iter_mut().find(|t| Arc::ptr_eq(&t.backend, &b)).expect("pinned");
                tx.depth += 1;
                tx.depth
            };
            let begin = if depth == 1 { "begin".to_string() } else { format!("savepoint quillo_tx_{}", depth) };
            let started = pooled_run(it, &b, move |c| c.batch(&begin));
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
            let ended = pooled_run(it, &b, move |c| c.batch(&end));
            tx_end(it, mark, commit && ended.is_ok());
            let done = {
                let mut g = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner());
                let i = g.iter().position(|t| Arc::ptr_eq(&t.backend, &b)).expect("pinned");
                g[i].depth -= 1;
                if g[i].depth == 0 {
                    Some(g.remove(i))
                } else {
                    None
                }
            };
            if let Some(TxConn { conn: Some(c), .. }) = done {
                pool.give_back(c);
            }
            match ended {
                Err(e) if commit => Ok(db_err(e)),
                _ => r,
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
    let mut conns = it.db_conns.lock().unwrap_or_else(|e| e.into_inner());
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
            let c = m.lock().unwrap_or_else(|e| e.into_inner());
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
    if parts.iter().any(|p| it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|t| Arc::ptr_eq(&t.backend, &p.b))) || it.sqlite_tx > 0 {
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
                Backend::Sqlite(m) => m.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("savepoint quillo_tx").map_err(|e| e.to_string()),
                _ => unreachable!(),
            },
            _ => {
                match checkout_for_tx(it, &p.b) {
                    Ok(c) => {
                        it.pg_tx.lock().unwrap_or_else(|e| e.into_inner()).push(TxConn { backend: p.b.clone(), conn: Some(c), depth: 1 });
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
    }
    let mark = tx_begin(it);
    it.wrote_req = Some(crate::httpd::request_number());
    let r = match &failed {
        None => it.call(&f, Vec::new(), line),
        Some(e) => Ok(db_err(e)),
    };
    let ok = failed.is_none() && matches!(&r, Ok(v) if !matches!(v, Value::Err(_)));
    let outcome = if ok { commit_parts(it, &mut parts, &sys, &gid) } else { Err(String::new()) };
    if outcome.is_err() {
        abort_parts(it, &mut parts, &begun);
    }
    if sqlite {
        it.sqlite_tx -= 1;
    }
    // The connections go back.
    for p in &parts {
        let taken = {
            let mut g = it.pg_tx.lock().unwrap_or_else(|e| e.into_inner());
            g.iter().position(|t| Arc::ptr_eq(&t.backend, &p.b)).map(|i| g.remove(i))
        };
        if let Some(TxConn { conn: Some(c), .. }) = taken {
            p.b.give_back(c);
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
            if let Err(e) = m.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("release quillo_tx") {
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
                let _ = m.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("rollback to quillo_tx; release quillo_tx");
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
    if let Some(DbSlot::One(b)) = it.db.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        servers.push(b);
    }
    for (_, s) in it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).iter() {
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
