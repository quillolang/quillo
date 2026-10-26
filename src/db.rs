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
    fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        match self {
            Pinned::Pg(c) => c.query(sql, params, limit),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.query(sql, params, limit),
        }
    }

    fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        match self {
            Pinned::Pg(c) => c.exec(sql, params),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.exec(sql, params),
        }
    }

    fn batch(&mut self, sql: &str) -> Result<(), String> {
        match self {
            Pinned::Pg(c) => c.batch(sql),
            #[cfg(feature = "mysql")]
            Pinned::My(c) => c.batch(sql),
        }
    }
}

impl Backend {
    /// A connection from the pool, for a server database.
    fn checkout(&self) -> Result<Pinned, String> {
        match self {
            Backend::Postgres(p) => p.checkout().map(Pinned::Pg),
            #[cfg(feature = "mysql")]
            Backend::Mysql(p) => p.checkout().map(Pinned::My),
            Backend::Sqlite(_) => Err("SQLite has no pool".into()),
        }
    }

    fn give_back(&self, c: Pinned) {
        match (self, c) {
            (Backend::Postgres(p), Pinned::Pg(c)) => p.give_back(c),
            #[cfg(feature = "mysql")]
            (Backend::Mysql(p), Pinned::My(c)) => p.give_back(c),
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    fn is_mysql(&self) -> bool {
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
    c.execute_batch("pragma foreign_keys = on; pragma journal_mode = wal;")?;
    Ok(c)
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
fn backend_on(it: &mut Interp, on: Option<usize>) -> Result<Arc<Backend>, String> {
    let Some(i) = on else { return backend(it) };
    let slot = it.db_conns.lock().unwrap_or_else(|e| e.into_inner()).get(i).map(|(_, s)| s.clone());
    match slot {
        Some(DbSlot::One(b)) => Ok(b),
        Some(DbSlot::PerTenant(t)) => tenant_backend(it, &t),
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
        DbSlot::PerTenant(_) => unreachable!("set just above"),
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

/// Runs `f` on a server database's connection without holding the
/// interpreter lock: the request's transaction connection if it has one,
/// else one from the pool.
fn pooled_run<T: Send>(
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
/// A connection's tables are topics of their own (`db@<label>:orders`), so
/// that one `orders` changing does not refresh pages showing another.
fn announce(it: &mut Interp, sql: &str, on: Option<usize>) {
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
            let path = template.split('?').next().unwrap_or("");
            let (server, db) = path.rsplit_once('/').unwrap_or((path, ""));
            let (pre, post) = db.split_once("{tenant}").map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
            let server = format!("{}/", server);
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
        let base = template.split('?').next().unwrap_or("").to_string();
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

/// Whether this server runs a scheduled job's run due at `at`: with one
/// database for the program (which other servers may share), only if no
/// one recorded that run before; otherwise always.
pub(crate) fn claim_job(it: &mut Interp, name: &str, at: u64) -> bool {
    if !matches!(&*it.db.lock().unwrap_or_else(|e| e.into_inner()), Some(DbSlot::One(_))) {
        return true;
    }
    let mysql = backend(it).map_or(false, |b| b.is_mysql());
    let table = if mysql {
        "create table if not exists _quillo_jobs (name varchar(255) primary key, at bigint not null)"
    } else {
        "create table if not exists _quillo_jobs (name text primary key, at bigint not null)"
    };
    let made = exec(it, vec![Value::str(table)], 0);
    if !matches!(made, Ok(Value::Map(_))) {
        return true;
    }
    // A later run claims it (MySQL counts a row it leaves as it was as no change).
    let sql = if mysql {
        "insert into _quillo_jobs (name, at) values (?, ?) on duplicate key update at = if(at < values(at), values(at), at)"
    } else {
        "insert into _quillo_jobs (name, at) values (?, ?) \
         on conflict (name) do update set at = excluded.at where _quillo_jobs.at < excluded.at"
    };
    match exec(it, vec![Value::str(sql), Value::str(name), Value::Int(at as i64)], 0) {
        Ok(Value::Map(m)) => !matches!(m.get("changes"), Some(Value::Int(0))),
        _ => true,
    }
}

fn select(it: &mut Interp, on: Option<usize>, sql: String, params: Vec<Value>, limit: Option<usize>) -> Result<Vec<Value>, String> {
    let b = backend_on(it, on)?;
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
                let c = match it.unlocked(|| pool.checkout()) {
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
