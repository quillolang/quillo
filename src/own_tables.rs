//! Quillo's own tables (`_quillo_*`) held by a database role on PostgreSQL and MySQL/MariaDB
//! (the user's decision of 2026-11-26, "held by a role the program's SQL cannot use"; branch
//! feedback86-89).
//!
//! The program's role (the URL's, P) keeps the program's SQL. Quillo's role (Q), which a host or
//! the program's operator makes, owns Quillo's tables in a schema of its own (`quillo`, or
//! `quillo_<schema>` for a program or tenant in a schema; MySQL: the database `<db>_quillo`) and a
//! few definer functions; P may only call those, and has no privilege on the tables. P is never
//! granted Q, so no SQL of the program's can become Q, a taken file's run from text included.
//!
//! Each call carries a MAC over the statement and its values, under a key derived from the
//! program's own secret (`QUILLO_SECRET`), which the database checks against pads only Q reads:
//! a taken file cannot make one, read another visitor's idempotent answer, or replay a call for
//! another statement or other values. No function hands the key out; after the setup it never
//! crosses the wire, and every MAC goes as a bound value, never in a statement's text.
//!
//! Quillo's statements run in the program's transaction on the program's connection, so its
//! records, later calls and atomic steps still commit with the program's writes (exactly-once).
//! The two documented uses of the program's own SQL keep working as written, through views in
//! the program's schema that give only what they need: `select name from _quillo_migrations` and
//! `update _quillo_later set at = 0, tries = 0 where tries < 0`.
//!
//! The setup, `db.own_setup(q_url)`, which an operator runs once as Q, makes the schema, the
//! functions and the key's pads, and moves 0.8.11's tables; `db.open` finds what it made, prints
//! the SQL or the command when something is missing, and stops when the key no longer matches.
//! Without Q, or without a lasting secret, 0.8.11's reading of a taken file's SQL holds the tables
//! instead, with one warning where a quillo.confirmed holds the run. SQLite is unchanged.

use crate::db::Backend;
use crate::interp::Interp;
use crate::value::*;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, Weak};

/// The schema of Quillo's functions and of the program's own tables when it has no schema.
pub(crate) const SCHEMA: &str = "quillo";

/// What the database's functions say they are: a setup of another version is made again.
const VERSION: &str = "quillo-own 1";

/// A database whose own tables are held by Q: the key its calls are signed with, and where its
/// tables are (PostgreSQL's schema, MySQL's database).
pub(crate) struct Own {
    key: ring::hmac::Key,
    pub(crate) place: String,
    mysql: bool,
}

impl Own {
    /// The MAC of `parts` (separated by 0x01, which no part holds), as hex, streamed directly into
    /// HMAC without an intermediate buffer allocation.
    fn mac(&self, parts: &[&str]) -> String {
        let mut ctx = ring::hmac::Context::with_key(&self.key);
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                ctx.update(b"\x01");
            }
            ctx.update(p.as_bytes());
        }
        crate::crypto::hex(ctx.sign().as_ref())
    }
}

/// Databases held by Q, by their backend's address (held weakly).
static HELD: Mutex<Vec<(usize, Weak<Backend>, Arc<Own>)>> = Mutex::new(Vec::new());
/// Whether any is: a program with none pays one load for each of Quillo's statements.
static ANY: AtomicBool = AtomicBool::new(false);

/// Whether the database `b` is held by Q, and how.
pub(crate) fn mode(b: &Backend) -> Option<Arc<Own>> {
    if !ANY.load(Relaxed) {
        return None;
    }
    let at = b as *const Backend as usize;
    let held = HELD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    held.iter().find(|(a, w, _)| *a == at && w.strong_count() > 0).map(|(_, _, o)| o.clone())
}

fn hold(b: &Arc<Backend>, own: Own) {
    let mut held = HELD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|(_, w, _)| w.strong_count() > 0);
    held.push((Arc::as_ptr(b) as usize, Arc::downgrade(b), Arc::new(own)));
    ANY.store(true, Relaxed);
}

/// The program's lasting secret, which the key comes from: `QUILLO_SECRET` (the dev server sets
/// it from `.quillo/.quillo-secret`). Without one sessions use a random key, and so would this.
fn secret() -> Option<String> {
    std::env::var("QUILLO_SECRET").ok().filter(|s| !s.is_empty())
}

/// The key for the database named `db`, from the program's secret.
fn key_for(db: &str) -> Option<ring::hmac::Key> {
    let s = secret()?;
    let base = crate::crypto::hmac_sha256(s.as_bytes(), b"quillo session");
    let k = crate::crypto::hmac_sha256(&base, format!("quillo own tables\0{}", db).as_bytes());
    Some(ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &k))
}

/// The pads the database checks a MAC with (HMAC-SHA256's), for the key of `db`.
fn pads_for(db: &str) -> Option<([u8; 64], [u8; 64])> {
    let s = secret()?;
    let base = crate::crypto::hmac_sha256(s.as_bytes(), b"quillo session");
    let k = crate::crypto::hmac_sha256(&base, format!("quillo own tables\0{}", db).as_bytes());
    let (mut i, mut o) = ([0x36u8; 64], [0x5cu8; 64]);
    for (n, b) in k.iter().enumerate() {
        i[n] ^= b;
        o[n] ^= b;
    }
    Some((i, o))
}

/// Where a program's or tenant's own tables are on PostgreSQL: `quillo`, or `quillo_<schema>`
/// for one in a schema of its own (63 bytes at most, as PostgreSQL's names).
pub(crate) fn pg_place(schema: Option<&str>) -> String {
    match schema {
        None => SCHEMA.to_string(),
        Some(s) => {
            let mut p = format!("{}_{}", SCHEMA, s);
            while p.len() > 63 {
                p.pop();
            }
            p
        }
    }
}

/// A name quoted as PostgreSQL takes it.
fn pg_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// A name quoted as MySQL takes it.
fn my_ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

/// The values as the JSON array the database takes them in.
fn args_json(params: &[Value]) -> String {
    let vals: Vec<serde_json::Value> = params
        .iter()
        .map(|v| match v {
            Value::Nil => serde_json::Value::Null,
            Value::Int(n) => serde_json::Value::from(*n),
            Value::Float(f) => serde_json::Number::from_f64(*f).map_or(serde_json::Value::Null, serde_json::Value::Number),
            Value::Bool(b) => serde_json::Value::from(*b as i64),
            other => serde_json::Value::String(other.display().to_string()),
        })
        .collect();
    serde_json::to_string(&vals).unwrap_or_else(|_| "[]".into())
}

/// One of Quillo's statements as the call that runs it as Q: its SQL and values. `rows`: its
/// rows are wanted (a query), else its count of changes.
pub(crate) fn call(own: &Own, sql: &str, params: &[Value], rows: bool) -> (String, Vec<Value>) {
    let sql = sql.trim().trim_end_matches(';').trim_end();
    let args = args_json(params);
    let how = if rows { "r" } else { "x" };
    if own.mysql {
        let mac = own.mac(&[&own.place, sql, &args, how]);
        let text = format!("call {}.q_run(?, ?, ?, ?, ?)", my_ident(&own.place));
        return (text, vec![Value::str(&own.place), Value::str(sql), Value::str(&args), Value::str(how), Value::str(&mac)]);
    }
    // Its places as `format`'s (`%1$L`), each value quoted where it goes; a `%` of its own doubled.
    let scan = crate::sql::scan(crate::sql::Dialect::Postgres, sql);
    let mut stmt = String::with_capacity(sql.len() + 16);
    let mut at = 0;
    for &(start, end, slot) in scan.marks() {
        stmt.push_str(&sql[at..start].replace('%', "%%"));
        stmt.push_str(&format!("%{}$L", slot));
        at = end;
    }
    stmt.push_str(&sql[at..].replace('%', "%%"));
    let mac = own.mac(&[&own.place, &stmt, &args, how]);
    let text = format!("select r from {}._quillo_run($1, $2, $3, $4, $5) r", SCHEMA);
    (text, vec![Value::str(&own.place), Value::str(&stmt), Value::str(&args), Value::str(how), Value::str(&mac)])
}

/// What a call gave, as the statement's own result: its rows, or `{ changes, last_id }`.
pub(crate) fn result(own: &Own, rows: Vec<Value>, want_rows: bool) -> Value {
    let rows: Vec<Value> = if own.mysql {
        rows
    } else {
        rows.into_iter().filter_map(|r| if let Value::Map(m) = r { m.get("r").cloned() } else { None }).collect()
    };
    if want_rows {
        return Value::list(rows);
    }
    let n = rows.first().and_then(|r| if let Value::Map(m) = r { m.get("changes").cloned() } else { None }).unwrap_or(Value::Int(0));
    let mut m = Map::new();
    m.insert("changes".into(), n);
    m.insert("last_id".into(), Value::Nil);
    Value::map(m)
}

/// Databases already warned of (one warning each).
static WARNED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn warn_once(db: &str, why: &str) {
    if !crate::confirm::held() {
        return;
    }
    let mut w = WARNED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if w.iter().any(|d| d == db) {
        return;
    }
    w.push(db.to_string());
    eprintln!(
        "quillo: warning: Quillo's own tables in the database {} are held by reading each taken file's SQL, as before: {}. To hold them by a role the program's SQL cannot use, make that role and run `db.own_setup(\"<its URL>\")` once (see \"Quillo's own tables\" in the guide)",
        db, why
    );
}

/// What a probe found.
enum Found {
    Held(Own),
    /// Nothing set up for this program in the database named (or no lasting secret): 0.8.11's
    /// reading holds, and why.
    None(String, String),
}

/// At `db.open` of a server database: whether Q holds its own tables. One whose functions refuse
/// the program's key (its secret changed since the setup) stops the open; one with nothing set up
/// keeps 0.8.11's reading, with one warning where a quillo.confirmed holds the run.
pub(crate) fn probe(it: &mut Interp, b: &Arc<Backend>) -> Result<(), String> {
    let b2 = b.clone();
    let found = it.unlocked(move || match &*b2 {
        Backend::Postgres(p) => pg_probe(p),
        #[cfg(feature = "mysql")]
        Backend::Mysql(p) => my_probe(p),
        #[allow(unreachable_patterns)]
        _ => Ok(Found::None(String::new(), String::new())),
    })?;
    match found {
        Found::Held(own) => hold(b, own),
        Found::None(db, why) if !why.is_empty() => warn_once(&db, &why),
        Found::None(..) => {}
    }
    Ok(())
}

/// What a refused key says.
fn changed(db: &str) -> String {
    format!(
        "Quillo's own tables in the database {} were set up with another QUILLO_SECRET, so the database refuses Quillo's calls: run setup again (`db.own_setup(\"<Quillo's role's URL>\")`), with the secret the program runs with",
        db
    )
}

fn pg_probe(p: &crate::pg::Pool) -> Result<Found, String> {
    let db = p.dbname();
    let place = pg_place(p.schema_name());
    let Some(key) = key_for(&db) else { return Ok(Found::None(db, "QUILLO_SECRET is not set, which the role's key comes from".into())) };
    let own = Own { key, place: place.clone(), mysql: false };
    let mut c = p.checkout_nested()?;
    let ask = |c: &mut crate::pg::Conn, what: &str| {
        let nonce = crate::crypto::hex(&crate::crypto::secret_bytes(16));
        let mac = own.mac(&[what, &place, &nonce]);
        c.query(&format!("select {}._quillo_{}($1, $2, $3) as v", SCHEMA, what), &[Value::str(&place), Value::str(&nonce), Value::str(&mac)], Some(1))
    };
    let said = |r: Result<Vec<Value>, String>| -> Result<String, String> {
        r.map(|rows| match rows.first() {
            Some(Value::Map(m)) => m.get("v").map(|v| v.display().to_string()).unwrap_or_default(),
            _ => String::new(),
        })
    };
    let mut r = said(ask(&mut c, "hello"));
    // A tenant's schema, made by Q the first time (`_quillo_tenant`).
    if matches!(&r, Ok(v) if v == "no place") {
        r = said(ask(&mut c, "tenant")).and_then(|_| said(ask(&mut c, "hello")));
    }
    p.give_back(c);
    match r {
        Ok(v) if v == VERSION => Ok(Found::Held(own)),
        Ok(v) => Ok(Found::None(db, format!("its setup is of another version ({}): run `db.own_setup` again", v))),
        Err(e) if e.contains("not Quillo's") => Err(changed(&db)),
        Err(e) if e.ends_with("[3F000]") || e.ends_with("[42883]") => Ok(Found::None(db, "no role holds them here".into())),
        Err(e) if e.ends_with("[42501]") => Ok(Found::None(db, format!("the program's role may not use Quillo's role's functions ({})", e))),
        Err(e) => Ok(Found::None(db, format!("Quillo's role's functions could not be asked ({})", e))),
    }
}

// ---- PostgreSQL's objects --------------------------------------------------------------------

/// Quillo's own function bodies, passed to the check of whether a database's code uses a
/// session's state (`pg::USES_SESSION`'s `$2`): no session of a request reaches through them (each
/// takes a call's MAC, and sets only a local `search_path`), though they are PL/pgSQL, run
/// `execute` of text and make tables (FEEDBACK75.md, Q-1060, the fifth review; the user's decision
/// of 2026-11-26 on own tables; branch feedback86-89). Each as `pg_proc.prosrc` holds it.
pub(crate) fn bodies() -> &'static [&'static str] {
    static BODIES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    BODIES.get_or_init(|| PG_FUNCTIONS.split("$f$").skip(1).step_by(2).collect())
}

/// The key's pads, and the check every function makes: HMAC-SHA256 over the call, compared
/// through a second hash under a random salt, so the comparison takes the same time wherever the
/// two differ. A call that fails it is refused (42501). Each function's search_path is fixed, with
/// `pg_temp` last, so no temporary table of a session stands for a table of Quillo's.
const PG_FUNCTIONS: &str = r#"
create table if not exists quillo._quillo_secret (ipad bytea not null, opad bytea not null, cmp bytea not null);
create or replace function quillo._quillo_check(msg text, mac text) returns void
language plpgsql stable security definer set search_path = pg_catalog, pg_temp as $f$
declare s record;
begin
  select ipad, opad, cmp into strict s from quillo._quillo_secret;
  if mac is null or mac !~ '^[0-9a-f]{64}$'
     or sha256(s.cmp || decode(mac, 'hex')) <> sha256(s.cmp || sha256(s.opad || sha256(s.ipad || convert_to(msg, 'UTF8')))) then
    raise exception 'Quillo''s own tables: this call is not Quillo''s' using errcode = '42501';
  end if;
end $f$;
create or replace function quillo._quillo_hello(place text, nonce text, mac text) returns text
language plpgsql stable security definer set search_path = pg_catalog, pg_temp as $f$
begin
  perform quillo._quillo_check('hello' || chr(1) || place || chr(1) || nonce, mac);
  if not exists (select 1 from pg_namespace where nspname = place and nspowner = (select oid from pg_roles where rolname = current_user)) then
    return 'no place';
  end if;
  return 'quillo-own 1';
end $f$;
create or replace function quillo._quillo_tenant(place text, nonce text, mac text) returns text
language plpgsql volatile security definer set search_path = pg_catalog, pg_temp as $f$
begin
  perform quillo._quillo_check('tenant' || chr(1) || place || chr(1) || nonce, mac);
  if place !~ '^quillo_' then
    raise exception 'Quillo''s own tables: % is no tenant''s place', place using errcode = '42501';
  end if;
  execute format('create schema if not exists %I', place);
  return 'made';
end $f$;
create or replace function quillo._quillo_run(place text, stmt text, args text, how text, mac text) returns setof json
language plpgsql volatile security definer set search_path = pg_catalog, pg_temp as $f$
declare n bigint; vals text[]; q text;
begin
  perform quillo._quillo_check(place || chr(1) || stmt || chr(1) || args || chr(1) || how, mac);
  if not exists (select 1 from pg_namespace where nspname = place and nspowner = (select oid from pg_roles where rolname = current_user)) then
    raise exception 'Quillo''s own tables: % is not set up', place using errcode = '3F000';
  end if;
  -- Undone as the function returns (its own `set search_path`); a `set` statement, not a call
  -- PostgreSQL's catalog check for session use reads as one (pg::USES_SESSION).
  execute format('set local search_path = %I, pg_catalog, pg_temp', place);
  select coalesce(array_agg(e.v order by e.i), '{}') into vals from json_array_elements_text(args::json) with ordinality as e(v, i);
  q := format(stmt, variadic vals);
  if how = 'r' then
    return query execute 'with t as (' || q || ') select row_to_json(t) from t';
  else
    execute q;
    get diagnostics n = row_count;
    return next json_build_object('changes', n);
  end if;
end $f$;
revoke all on function quillo._quillo_check(text, text), quillo._quillo_hello(text, text, text), quillo._quillo_tenant(text, text, text), quillo._quillo_run(text, text, text, text, text) from public;
"#;

/// Quillo's own tables the program's views need, made before them.
const PG_MIGRATIONS: &str = "create table if not exists _quillo_migrations (name text primary key, sha256 text not null, applied_at text not null)";
const PG_LATER: &str = "create table if not exists _quillo_later (id text primary key, service text not null, fn text not null, \
     args text not null, tenant text not null, rid text not null, at bigint not null, tries bigint not null, error text)";

/// A statement's rows on a connection, or why not.
fn rows(c: &mut crate::pg::Conn, sql: &str, params: &[Value]) -> Result<Vec<Value>, String> {
    c.query(sql, params, None)
}

fn first_text(rows: &[Value], col: &str) -> String {
    match rows.first() {
        Some(Value::Map(m)) => m.get(col).map(|v| v.display().to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// `db.own_setup(q_url)` on PostgreSQL: run by the program's operator once, as Q. Returns what it
/// did, a line each.
pub(crate) fn pg_setup(p: &crate::pg::Pool, q_url: &str) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    let db = p.dbname();
    let Some((ipad, opad)) = pads_for(&db) else {
        return Err("QUILLO_SECRET is not set: Quillo's role's key comes from it (set it as the program runs, then run setup)".into());
    };
    let q = crate::pg::Pool::open_as(q_url, false).map_err(|e| format!("Quillo's role's URL: {}", e))?;
    let mut qc = q.checkout_nested()?;
    let mut pc = p.checkout_nested()?;
    let r = (|| -> Result<(), String> {
        let who = |c: &mut crate::pg::Conn| rows(c, "select current_user::text as u, current_database()::text as d, current_schema()::text as s, (select rolsuper from pg_roles where rolname = current_user) as su", &[]);
        let (pw, qw) = (who(&mut pc)?, who(&mut qc)?);
        let (pu, qu) = (first_text(&pw, "u"), first_text(&qw, "u"));
        if pu == qu {
            return Err(format!("Quillo's role must not be the program's ({}): make a role of its own", pu));
        }
        if first_text(&pw, "d") != first_text(&qw, "d") {
            return Err(format!("Quillo's role's URL is of the database {}, the program's of {}", first_text(&qw, "d"), first_text(&pw, "d")));
        }
        if first_text(&pw, "su") == "true" {
            return Err(format!("the program's role {} is a superuser, which no role can hold anything from: run the program as a plain role", pu));
        }
        let member = rows(&mut pc, "select pg_has_role(current_user, $1, 'MEMBER') as m", &[Value::str(&qu)])?;
        if first_text(&member, "m") == "true" {
            return Err(format!("the program's role {} may become Quillo's role {} (it is granted it): revoke that, or no hold is kept", pu, qu));
        }
        let schema = first_text(&pw, "s");
        let place = pg_place(p.schema_name());
        let (pi, qi) = (pg_ident(&pu), pg_ident(&qu));
        // Quillo's schemas, made by Q: on the database the program's role owns it may let Q.
        let make = |qc: &mut crate::pg::Conn, s: &str| qc.batch(&format!("create schema if not exists {}", pg_ident(s)));
        if let Err(e) = make(&mut qc, SCHEMA) {
            if !e.ends_with("[42501]") {
                return Err(e);
            }
            pc.batch(&format!("grant create on database {} to {}", pg_ident(&db), qi))
                .map_err(|_| format!("Quillo's role {} may not make schemas in the database {}: as its owner, run `grant create on database {} to {};`, then setup again", qu, db, pg_ident(&db), qi))?;
            said.push(format!("granted {} create on the database {}", qu, db));
            make(&mut qc, SCHEMA)?;
        }
        if place != SCHEMA {
            make(&mut qc, &place)?;
        }
        qc.batch("begin")?;
        qc.batch(PG_FUNCTIONS)?;
        qc.exec("delete from quillo._quillo_secret", &[])?;
        qc.exec(
            "insert into quillo._quillo_secret (ipad, opad, cmp) values (decode($1, 'hex'), decode($2, 'hex'), decode($3, 'hex'))",
            &[Value::str(crate::crypto::hex(&ipad)), Value::str(crate::crypto::hex(&opad)), Value::str(crate::crypto::hex(&crate::crypto::secret_bytes(32)))],
        )?;
        qc.batch("revoke all on table quillo._quillo_secret from public")?;
        qc.batch(&format!("grant usage on schema {} to {}", SCHEMA, pi))?;
        qc.batch(&format!(
            "grant execute on function quillo._quillo_hello(text, text, text), quillo._quillo_tenant(text, text, text), quillo._quillo_run(text, text, text, text, text) to {}",
            pi
        ))?;
        qc.batch("commit")?;
        said.push(format!("made the schema {} with its functions, owned by {}; {} may call them, and nothing else", SCHEMA, qu, pu));
        // 0.8.11's tables, the program's role's, in each of its schemas.
        let old = rows(
            &mut pc,
            "select n.nspname::text as s, c.relname::text as t from pg_class c join pg_namespace n on n.oid = c.relnamespace \
             where c.relkind = 'r' and c.relname like '\\_quillo\\_%' and c.relowner = (select oid from pg_roles where rolname = current_user) \
             and n.nspname not like 'quillo%' order by 1, 2",
            &[],
        )?;
        for row in &old {
            let Value::Map(m) = row else { continue };
            let (s, t) = (m.get("s").map(|v| v.display().to_string()).unwrap_or_default(), m.get("t").map(|v| v.display().to_string()).unwrap_or_default());
            let to = if s == schema { place.clone() } else { pg_place(Some(&s)) };
            let moved = pg_move(&mut pc, &mut qc, &s, &t, &to, &pi, &qi)?;
            said.push(format!("moved {}.{} ({} rows) to {}.{}", s, t, moved, to, t));
        }
        // The two documented uses of the program's own SQL, as views in its schema: the
        // migrations' names, and a later call's `at` and `tries` (`tries` to read).
        let _ = pc.batch(&format!("grant usage, create on schema {} to {}", pg_ident(&schema), qi));
        qc.batch(&format!("set search_path = {}, pg_catalog", pg_ident(&place)))?;
        qc.batch(PG_MIGRATIONS)?;
        qc.batch(PG_LATER)?;
        qc.batch("reset search_path")?;
        let (sv, pv) = (pg_ident(&schema), pg_ident(&place));
        let views = [
            format!("create or replace view {}._quillo_migrations as select name from {}._quillo_migrations", sv, pv),
            format!("revoke all on {}._quillo_migrations from public", sv),
            format!("grant select on {}._quillo_migrations to {}", sv, pi),
            format!("create or replace view {}._quillo_later as select at, tries from {}._quillo_later", sv, pv),
            format!("revoke all on {}._quillo_later from public", sv),
            format!("grant select (tries), update (at, tries) on {}._quillo_later to {}", sv, pi),
        ];
        match views.iter().try_for_each(|v| qc.batch(v)) {
            Ok(()) => said.push(format!("made the views {0}._quillo_migrations (name) and {0}._quillo_later (at, tries) for the program's own SQL", schema)),
            Err(e) => said.push(format!(
                "the views for the program's own SQL were not made ({}): as the schema's owner, run `grant usage, create on schema {} to {};`, then setup again",
                e, sv, qi
            )),
        }
        Ok(())
    })();
    if r.is_err() {
        let _ = qc.batch("rollback");
        let _ = pc.batch("rollback");
    }
    q.give_back(qc);
    p.give_back(pc);
    r.map(|()| said)
}

/// Moves one of 0.8.11's tables, `s.t` (the program's role's), into Quillo's place `to`: the
/// program's role lets Q read it and holds it from writers, Q makes its table as that one is (its
/// columns, keys and indexes, under their names) and copies the rows, and the program's role drops
/// its own. Its rows: how many were moved.
fn pg_move(pc: &mut crate::pg::Conn, qc: &mut crate::pg::Conn, s: &str, t: &str, to: &str, pi: &str, qi: &str) -> Result<i64, String> {
    let _ = pi;
    let (old, new) = (format!("{}.{}", pg_ident(s), pg_ident(t)), format!("{}.{}", pg_ident(to), pg_ident(t)));
    // Granted before its transaction, so that Q's connection sees it.
    pc.batch(&format!("grant usage on schema {} to {}", pg_ident(s), qi))?;
    pc.batch(&format!("grant select on {} to {}", old, qi))?;
    pc.batch("begin")?;
    let r = (|| -> Result<i64, String> {
        pc.batch(&format!("lock table {} in exclusive mode", old))?;
        let keys = rows(pc, "select conname::text as n, pg_get_constraintdef(oid) as d from pg_constraint where conrelid = $1::regclass and contype in ('p', 'u')", &[Value::str(&old)])?;
        let idx = rows(
            pc,
            "select pg_get_indexdef(i.indexrelid) as d from pg_index i where i.indrelid = $1::regclass \
             and not exists (select 1 from pg_constraint k where k.conindid = i.indexrelid)",
            &[Value::str(&old)],
        )?;
        let cols = first_text(
            &rows(pc, "select string_agg(quote_ident(attname), ', ' order by attnum) as c from pg_attribute where attrelid = $1::regclass and attnum > 0 and not attisdropped", &[Value::str(&old)])?,
            "c",
        );
        qc.batch("begin")?;
        qc.batch(&format!("create schema if not exists {}", pg_ident(to)))?;
        let there = first_text(&rows(qc, "select to_regclass($1) is not null as b", &[Value::str(&new)])?, "b") == "true";
        if !there {
            qc.batch(&format!("create table {} (like {} including defaults including constraints including identity including generated)", new, old))?;
            for k in &keys {
                let Value::Map(m) = k else { continue };
                let (n, d) = (m.get("n").map(|v| v.display().to_string()).unwrap_or_default(), m.get("d").map(|v| v.display().to_string()).unwrap_or_default());
                qc.batch(&format!("alter table {} add constraint {} {}", new, pg_ident(&n), d))?;
            }
            for i in &idx {
                let Value::Map(m) = i else { continue };
                let d = m.get("d").map(|v| v.display().to_string()).unwrap_or_default();
                // `CREATE INDEX name ON s.t USING ...`: the same on Quillo's.
                if let Some((head, tail)) = d.split_once(" ON ") {
                    let rest = tail.split_once(" USING ").map_or(String::new(), |(_, u)| format!(" USING {}", u));
                    qc.batch(&format!("{} ON {}{}", head, new, rest))?;
                }
            }
        }
        let n = qc.exec(&format!("insert into {} ({}) select {} from {} on conflict do nothing", new, cols, cols, old), &[])?;
        qc.batch("commit")?;
        pc.batch(&format!("drop table {}", old))?;
        pc.batch("commit")?;
        Ok(match &n {
            Value::Map(m) => match m.get("changes") {
                Some(Value::Int(c)) => *c,
                _ => 0,
            },
            _ => 0,
        })
    })();
    if r.is_err() {
        let _ = qc.batch("rollback");
        let _ = pc.batch("rollback");
    }
    r
}

// ---- MySQL and MariaDB -------------------------------------------------------------------------

#[cfg(feature = "mysql")]
fn my_probe(p: &crate::my::Pool) -> Result<Found, String> {
    let mut c = p.checkout()?;
    let db = match c.query("select database() as d", &[], Some(1)) {
        Ok(r) => first_text(&r, "d"),
        Err(e) => {
            p.give_back(c);
            return Err(e);
        }
    };
    if db.is_empty() {
        p.give_back(c);
        return Ok(Found::None(db, "no database selected".into()));
    }
    let place = format!("{}_quillo", db);
    let Some(key) = key_for(&db) else {
        p.give_back(c);
        return Ok(Found::None(db, "QUILLO_SECRET is not set, which the role's key comes from".into()));
    };
    let own = Own { key, place: place.clone(), mysql: true };
    let nonce = crate::crypto::hex(&crate::crypto::secret_bytes(16));
    let mac = own.mac(&["hello", &place, &nonce]);
    let r = c.query(&format!("call {}.q_hello(?, ?, ?)", my_ident(&place)), &[Value::str(&place), Value::str(&nonce), Value::str(&mac)], Some(1));
    p.give_back(c);
    match r {
        Ok(rows) if first_text(&rows, "v") == VERSION => Ok(Found::Held(own)),
        Ok(rows) => Ok(Found::None(db, format!("its setup is of another version ({}): run `db.own_setup` again", first_text(&rows, "v")))),
        Err(e) if e.contains("not Quillo's") => Err(changed(&db)),
        // No such database or procedure (1049, 1305), or not let use it (1370, 1044, 1142).
        Err(e) => Ok(Found::None(db, format!("no role holds them here ({})", e))),
    }
}

/// MySQL's and MariaDB's: the same in procedures of Q's (`SQL SECURITY DEFINER`), in the
/// database `<db>_quillo`, which the program's role has no grant on; its statements' `?` take
/// the values quoted. Results come back as the statement's own rows, or `changes`.
#[cfg(feature = "mysql")]
const MY_PROCEDURES: &[&str] = &[
    "create table if not exists _quillo_secret (ipad varbinary(64) not null, opad varbinary(64) not null, cmp varbinary(32) not null)",
    "drop procedure if exists q_check",
    "create procedure q_check(in msg longtext, in mac varchar(64)) sql security definer
     begin
       declare i varbinary(64); declare o varbinary(64); declare s varbinary(32);
       select ipad, opad, cmp into i, o, s from _quillo_secret limit 1;
       if mac is null or mac not regexp '^[0-9a-f]{64}$'
          or sha2(concat(s, unhex(mac)), 256) <> sha2(concat(s, unhex(sha2(concat(o, unhex(sha2(concat(i, convert(msg using utf8mb4)), 256))), 256))), 256) then
         signal sqlstate '42000' set message_text = 'Quillo''s own tables: this call is not Quillo''s';
       end if;
     end",
    "drop procedure if exists q_hello",
    "create procedure q_hello(in place varchar(64), in nonce varchar(64), in mac varchar(64)) sql security definer
     begin
       call q_check(concat('hello', char(1), place, char(1), nonce), mac);
       select 'quillo-own 1' as v;
     end",
    "drop procedure if exists q_run",
    // Always clear `@quillo_sql` (before EXECUTE and inside a SQLEXCEPTION handler, where it
    // would otherwise remain readable on the pooled connection with unredacted SQL/values).
    "create procedure q_run(in place varchar(64), in stmt longtext, in args longtext, in how varchar(1), in mac varchar(64)) sql security definer
     begin
       declare n int default 0; declare k int default 0; declare at int default 1; declare out_sql longtext default ''; declare v longtext;
       declare exit handler for sqlexception
       begin
         begin
           declare continue handler for sqlexception begin end;
           set @quillo_sql = null;
           deallocate prepare quillo_s;
         end;
         resignal;
       end;
       call q_check(concat(place, char(1), stmt, char(1), args, char(1), how), mac);
       set n = json_length(args);
       while locate('?', stmt, at) > 0 do
         if k >= n then signal sqlstate '42000' set message_text = 'Quillo''s own tables: a value is missing'; end if;
         set v = json_unquote(json_extract(args, concat('$[', k, ']')));
         set out_sql = concat(out_sql, substring(stmt, at, locate('?', stmt, at) - at),
           if(json_type(json_extract(args, concat('$[', k, ']'))) = 'NULL', 'NULL', quote(v)));
         set at = locate('?', stmt, at) + 1;
         set k = k + 1;
       end while;
       set @quillo_sql = concat(out_sql, substring(stmt, at));
       prepare quillo_s from @quillo_sql;
       set @quillo_sql = null;
       execute quillo_s;
       if how <> 'r' then
         select row_count() as changes;
       end if;
       deallocate prepare quillo_s;
     end",
];

/// `db.own_setup(q_url)` on MySQL and MariaDB.
#[cfg(feature = "mysql")]
pub(crate) fn my_setup(p: &crate::my::Pool, q_url: &str) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    let mut pc = p.checkout()?;
    let db = match pc.query("select database() as d", &[], Some(1)) {
        Ok(r) => first_text(&r, "d"),
        Err(e) => {
            p.give_back(pc);
            return Err(e);
        }
    };
    if db.is_empty() {
        p.give_back(pc);
        return Err("the program's database URL selects no database".into());
    }
    let place = format!("{}_quillo", db);
    let Some((ipad, opad)) = pads_for(&db) else {
        p.give_back(pc);
        return Err("QUILLO_SECRET is not set: Quillo's role's key comes from it (set it as the program runs, then run setup)".into());
    };
    let q = match crate::my::Pool::open_as(q_url, false) {
        Ok(q) => q,
        Err(e) => {
            p.give_back(pc);
            return Err(format!("Quillo's role's URL: {}", e));
        }
    };
    let mut qc = match q.checkout() {
        Ok(c) => c,
        Err(e) => {
            p.give_back(pc);
            return Err(e);
        }
    };
    let r = (|| -> Result<(), String> {
        let who = |c: &mut crate::my::Conn| c.query("select current_user() as u", &[], Some(1));
        let (pu, qu) = (first_text(&who(&mut pc)?, "u"), first_text(&who(&mut qc)?, "u"));
        if pu == qu {
            return Err(format!("Quillo's role must not be the program's ({}): make a user of its own", pu));
        }
        // `user@host`, quoted for a grant.
        let grantee = |u: &str| match u.rsplit_once('@') {
            Some((n, h)) => format!("'{}'@'{}'", n.replace('\'', "''"), h.replace('\'', "''")),
            None => format!("'{}'", u.replace('\'', "''")),
        };
        let (pd, qd) = (my_ident(&db), my_ident(&place));
        qc.batch(&format!("create database if not exists {}", qd)).map_err(|e| {
            format!("Quillo's user {} may not make the database {} ({}): as an administrator, run `create database {}; grant all on {}.* to {} with grant option;`, then setup again", qu, place, e, qd, qd, grantee(&qu))
        })?;
        qc.batch(&format!("use {}", qd))?;
        for s in MY_PROCEDURES {
            qc.batch(s)?;
        }
        qc.exec("delete from _quillo_secret", &[])?;
        qc.exec(
            "insert into _quillo_secret (ipad, opad, cmp) values (unhex(?), unhex(?), unhex(?))",
            &[Value::str(crate::crypto::hex(&ipad)), Value::str(crate::crypto::hex(&opad)), Value::str(crate::crypto::hex(&crate::crypto::secret_bytes(32)))],
        )?;
        for proc_ in ["q_hello", "q_run"] {
            qc.batch(&format!("grant execute on procedure {}.{} to {}", qd, proc_, grantee(&pu))).map_err(|e| {
                format!("Quillo's user {} may not grant its procedures ({}): as an administrator, run `grant all on {}.* to {} with grant option;`, then setup again", qu, e, qd, grantee(&qu))
            })?;
        }
        said.push(format!("made the database {} with its procedures, Quillo's user {}'s; {} may call them, and nothing else", place, qu, pu));
        // 0.8.11's tables: renamed into Quillo's database, by whichever user may.
        let old = pc.query("select table_name as t from information_schema.tables where table_schema = database() and table_name like '\\_quillo\\_%' and table_type = 'BASE TABLE'", &[], None)?;
        for row in &old {
            let t = match row {
                Value::Map(m) => m.get("t").map(|v| v.display().to_string()).unwrap_or_default(),
                _ => continue,
            };
            let (from, to) = (format!("{}.{}", pd, my_ident(&t)), format!("{}.{}", qd, my_ident(&t)));
            let n = pc.query(&format!("select count(*) as n from {}", from), &[], Some(1)).map(|r| first_text(&r, "n")).unwrap_or_default();
            pc.batch(&format!("rename table {} to {}", from, to))
                .or_else(|_| qc.batch(&format!("rename table {} to {}", from, to)))
                .map_err(|e| format!("0.8.11's table {} could not be moved ({}): as an administrator, run `rename table {} to {};`, then setup again", t, e, from, to))?;
            said.push(format!("moved {}.{} ({} rows) to {}.{}", db, t, n, place, t));
        }
        // The two documented uses, as views of Q's in the program's database: a view of two tables
        // takes an update but no delete or insert, which the program's grants there would allow.
        qc.batch("create table if not exists _quillo_migrations (name varchar(255) primary key, sha256 char(64) not null, applied_at varchar(32) not null)")?;
        qc.batch(
            "create table if not exists _quillo_later (id varchar(64) primary key, service varchar(255) not null, fn varchar(255) not null, \
             args text not null, tenant varchar(255) not null, rid varchar(64) not null, at bigint not null, tries bigint not null, error text)",
        )?;
        qc.batch("create table if not exists _quillo_one (one int primary key)")?;
        qc.batch("insert ignore into _quillo_one values (1)")?;
        let views = [
            format!("create or replace sql security definer view {}._quillo_migrations as select name from {}._quillo_migrations", pd, qd),
            format!("create or replace sql security definer view {}._quillo_later as select l.at, l.tries from {}._quillo_later l join {}._quillo_one o on o.one = 1", pd, qd, qd),
        ];
        match views.iter().try_for_each(|v| qc.batch(v)) {
            Ok(()) => said.push(format!("made the views {0}._quillo_migrations (name) and {0}._quillo_later (at, tries) for the program's own SQL", db)),
            Err(e) => said.push(format!(
                "the views for the program's own SQL were not made ({}): as an administrator, run `grant create view, select on {}.* to {};`, then setup again",
                e, pd, grantee(&qu)
            )),
        }
        Ok(())
    })();
    q.give_back(qc);
    p.give_back(pc);
    r.map(|()| said)
}
