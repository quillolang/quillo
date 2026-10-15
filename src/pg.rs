//! PostgreSQL for `db`: a connection pool, placeholder translation, and
//! conversion between Quillo values and Postgres types.
//!
//! Queries are prepared, so parameters never become SQL text. Quillo's `?`
//! and `:name` placeholders are rewritten to `$1, $2, ...` (a query that
//! already uses `$1` is left alone). Parameters are encoded for the type
//! Postgres expects; for types without a direct encoding (numeric, dates,
//! uuid, arrays, ...) the parameter is sent as text and cast by Postgres.
//! Results come back as Quillo values: numbers, strings, booleans, JSON as
//! maps and lists, arrays as lists, and dates, times and UUIDs as strings.

use crate::value::*;
use bytes::{BufMut, BytesMut};
use postgres::types::{FromSql, IsNull, Kind, ToSql, Type};
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Condvar, Mutex};

type BoxErr = Box<dyn Error + Sync + Send>;

/// Default and largest pool size; `?pool_size=N` in the URL changes it.
const DEFAULT_POOL: usize = 8;

pub fn is_url(s: &str) -> bool {
    s.starts_with("postgres://") || s.starts_with("postgresql://")
}

// ---- connections ---------------------------------------------------------

/// One connection and the statements prepared on it.
pub struct Conn {
    client: postgres::Client,
    stmts: HashMap<String, postgres::Statement>,
}

pub struct Pool {
    config: postgres::Config,
    tls: tokio_postgres_rustls::MakeRustlsConnect,
    idle: Mutex<(Vec<Conn>, usize)>,
    freed: Condvar,
    max: usize,
}

fn tls() -> tokio_postgres_rustls::MakeRustlsConnect {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("TLS protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    tokio_postgres_rustls::MakeRustlsConnect::new(config)
}

impl Pool {
    /// Parses the URL and opens one connection to check it. TLS is used
    /// when the server offers it (`sslmode=require` insists on it).
    pub fn open(url: &str) -> Result<Pool, String> {
        let (url, max) = take_pool_size(url)?;
        let config: postgres::Config = url.parse().map_err(|e| format!("bad Postgres URL: {}", e))?;
        let pool = Pool { config, tls: tls(), idle: Mutex::new((Vec::new(), 0)), freed: Condvar::new(), max };
        let first = pool.checkout()?;
        pool.give_back(first);
        Ok(pool)
    }

    fn connect(&self) -> Result<Conn, String> {
        let client = self.config.connect(self.tls.clone()).map_err(|e| describe(&e))?;
        Ok(Conn { client, stmts: HashMap::new() })
    }

    /// A connection: an idle one, a new one, or the next one returned.
    pub fn checkout(&self) -> Result<Conn, String> {
        let mut g = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(c) = g.0.pop() {
                return Ok(c);
            }
            if g.1 < self.max {
                g.1 += 1;
                drop(g);
                return self.connect().inspect_err(|_| {
                    self.idle.lock().unwrap_or_else(|e| e.into_inner()).1 -= 1;
                    self.freed.notify_one();
                });
            }
            g = self.freed.wait(g).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn give_back(&self, c: Conn) {
        let mut g = self.idle.lock().unwrap_or_else(|e| e.into_inner());
        if c.client.is_closed() {
            g.1 -= 1;
        } else {
            g.0.push(c);
        }
        drop(g);
        self.freed.notify_one();
    }
}

/// Takes Quillo's own `pool_size` option out of the URL.
fn take_pool_size(url: &str) -> Result<(String, usize), String> {
    let Some((base, query)) = url.split_once('?') else { return Ok((url.to_string(), DEFAULT_POOL)) };
    let mut max = DEFAULT_POOL;
    let mut rest = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        match pair.split_once('=') {
            Some(("pool_size", v)) => {
                max = v.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| format!("bad pool_size: {}", v))?;
            }
            _ => rest.push(pair),
        }
    }
    let url = if rest.is_empty() { base.to_string() } else { format!("{}?{}", base, rest.join("&")) };
    Ok((url, max))
}

/// The server's message for a database error, else the error itself.
fn describe(e: &postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => {
            let mut s = db.message().to_string();
            if let Some(d) = db.detail() {
                s.push_str(&format!(" ({})", d));
            }
            s
        }
        None => {
            let mut s = e.to_string();
            let mut src = e.source();
            while let Some(inner) = src {
                s.push_str(&format!(": {}", inner));
                src = inner.source();
            }
            s
        }
    }
}

// ---- running SQL ---------------------------------------------------------

impl Conn {
    fn prepare(&mut self, sql: &str) -> Result<postgres::Statement, String> {
        if let Some(s) = self.stmts.get(sql) {
            return Ok(s.clone());
        }
        let s = self.client.prepare(sql).map_err(|e| describe(&e))?;
        if self.stmts.len() >= 256 {
            self.stmts.clear();
        }
        self.stmts.insert(sql.to_string(), s.clone());
        Ok(s)
    }

    /// Prepares `sql` for `params`, casting from text where needed.
    fn statement(&mut self, sql: &str, params: &[Value]) -> Result<postgres::Statement, String> {
        let stmt = self.prepare(sql)?;
        let want = stmt.params().len();
        if want != params.len() {
            return Err(format!("the query has {} parameter{}, got {}", want, if want == 1 { "" } else { "s" }, params.len()));
        }
        let casts: Vec<(usize, String)> = stmt
            .params()
            .iter()
            .enumerate()
            .filter(|(_, t)| !encodes_directly(t))
            .map(|(i, t)| (i + 1, type_name(t)))
            .collect();
        if casts.is_empty() {
            return Ok(stmt);
        }
        let cast_sql = rewrite_numbered(sql, |n| {
            casts.iter().find(|(i, _)| *i == n).map(|(_, t)| format!("(${}::text)::{}", n, t))
        });
        self.prepare(&cast_sql)
    }

    pub fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        let (sql, params) = translate(sql, params)?;
        let stmt = self.statement(&sql, &params)?;
        let args: Vec<PgParam> = params.into_iter().map(PgParam).collect();
        let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
        let rows = self.client.query(&stmt, &refs).map_err(|e| describe(&e))?;
        let mut out = Vec::new();
        for row in rows.iter().take(limit.unwrap_or(usize::MAX)) {
            let mut m = Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                let v: PgValue = row.try_get(i).map_err(|e| describe(&e))?;
                m.insert(col.name().to_string(), v.0);
            }
            out.push(Value::map(m));
        }
        Ok(out)
    }

    /// `{ changes, last_id }`; Postgres has no last insert id, so use
    /// `insert ... returning id` with `db.one` for that.
    pub fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        let changes = if params.is_empty() && has_several_statements(sql) {
            self.client.batch_execute(sql).map_err(|e| describe(&e))?;
            Value::Nil
        } else {
            let (sql, params) = translate(sql, params)?;
            let stmt = self.statement(&sql, &params)?;
            let args: Vec<PgParam> = params.into_iter().map(PgParam).collect();
            let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
            Value::Int(self.client.execute(&stmt, &refs).map_err(|e| describe(&e))? as i64)
        };
        let mut m = Map::new();
        m.insert("changes".into(), changes);
        m.insert("last_id".into(), Value::Nil);
        Ok(Value::map(m))
    }

    pub fn batch(&mut self, sql: &str) -> Result<(), String> {
        self.client.batch_execute(sql).map_err(|e| describe(&e))
    }
}

// ---- placeholders --------------------------------------------------------

/// Calls `f` for each piece of SQL outside quotes, comments and
/// dollar-quoted strings, and copies everything else through.
fn scan(sql: &str, mut f: impl FnMut(&[char], &mut usize, &mut String)) -> String {
    let c: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len() + 16);
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        // 'text', E'text', "identifier"
        if ch == '\'' || ch == '"' {
            let backslash = ch == '\'' && i > 0 && (c[i - 1] == 'E' || c[i - 1] == 'e');
            out.push(ch);
            i += 1;
            while i < c.len() {
                out.push(c[i]);
                if backslash && c[i] == '\\' && i + 1 < c.len() {
                    out.push(c[i + 1]);
                    i += 2;
                    continue;
                }
                if c[i] == ch {
                    if i + 1 < c.len() && c[i + 1] == ch {
                        out.push(ch);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // -- comment
        if ch == '-' && c.get(i + 1) == Some(&'-') {
            while i < c.len() && c[i] != '\n' {
                out.push(c[i]);
                i += 1;
            }
            continue;
        }
        // /* comment */
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                out.push(c[i]);
                i += 1;
            }
            out.push_str("*/");
            i = (i + 2).min(c.len());
            continue;
        }
        // $tag$ ... $tag$
        if ch == '$' && !c.get(i + 1).map_or(false, |d| d.is_ascii_digit()) {
            let mut j = i + 1;
            while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            if c.get(j) == Some(&'$') {
                let tag: String = c[i..=j].iter().collect();
                let body_start = j + 1;
                let rest: String = c[body_start..].iter().collect();
                let end = rest.find(&tag).map_or(c.len(), |e| body_start + rest[..e].chars().count() + tag.chars().count());
                out.extend(c[i..end].iter());
                i = end;
                continue;
            }
        }
        f(&c, &mut i, &mut out);
    }
    out
}

fn ident_at(c: &[char], start: usize) -> usize {
    let mut j = start;
    while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
        j += 1;
    }
    j
}

/// Rewrites `?` and `:name` placeholders to `$n`, and orders the values.
fn translate(sql: &str, params: &[Value]) -> Result<(String, Vec<Value>), String> {
    // `:name` with one map of values.
    if let [Value::Map(m)] = params {
        let mut names: Vec<String> = Vec::new();
        let mut uses_names = false;
        let out = scan(sql, |c, i, out| {
            let prev_colon = *i > 0 && c[*i - 1] == ':';
            if c[*i] == ':' && !prev_colon && c.get(*i + 1).map_or(false, |d| d.is_alphabetic() || *d == '_') {
                let end = ident_at(c, *i + 1);
                let name: String = c[*i + 1..end].iter().collect();
                let n = match names.iter().position(|x| *x == name) {
                    Some(p) => p + 1,
                    None => {
                        names.push(name);
                        names.len()
                    }
                };
                out.push_str(&format!("${}", n));
                uses_names = true;
                *i = end;
            } else {
                out.push(c[*i]);
                *i += 1;
            }
        });
        if uses_names {
            for k in m.keys() {
                let k = k.trim_start_matches([':', '@', '$']);
                if !names.iter().any(|n| n == k) {
                    return Err(format!("the query has no parameter :{}", k));
                }
            }
            let mut values = Vec::new();
            for n in &names {
                let v = m.get(n).or_else(|| m.get(&format!(":{}", n))).cloned();
                values.push(v.ok_or_else(|| format!("no value for :{}", n))?);
            }
            return Ok((out, values));
        }
    }
    // `?` placeholders, unless the query already numbers its own.
    if has_numbered(sql) {
        return Ok((sql.to_string(), params.to_vec()));
    }
    let mut n = 0;
    let out = scan(sql, |c, i, out| {
        if c[*i] == '?' && !matches!(c.get(*i + 1), Some('|') | Some('&')) {
            n += 1;
            out.push_str(&format!("${}", n));
        } else {
            out.push(c[*i]);
        }
        *i += 1;
    });
    Ok((out, params.to_vec()))
}

fn has_numbered(sql: &str) -> bool {
    let mut found = false;
    scan(sql, |c, i, out| {
        if c[*i] == '$' && c.get(*i + 1).map_or(false, |d| d.is_ascii_digit()) {
            found = true;
        }
        out.push(c[*i]);
        *i += 1;
    });
    found
}

/// Replaces `$n` where `f(n)` says so.
fn rewrite_numbered(sql: &str, f: impl Fn(usize) -> Option<String>) -> String {
    scan(sql, |c, i, out| {
        if c[*i] == '$' && c.get(*i + 1).map_or(false, |d| d.is_ascii_digit()) {
            let mut j = *i + 1;
            while j < c.len() && c[j].is_ascii_digit() {
                j += 1;
            }
            let n: usize = c[*i + 1..j].iter().collect::<String>().parse().unwrap_or(0);
            match f(n) {
                Some(s) => out.push_str(&s),
                None => out.extend(c[*i..j].iter()),
            }
            *i = j;
        } else {
            out.push(c[*i]);
            *i += 1;
        }
    })
}

fn has_several_statements(sql: &str) -> bool {
    let mut parts = 0;
    let mut current = false;
    scan(sql, |c, i, out| {
        if c[*i] == ';' {
            if current {
                parts += 1;
            }
            current = false;
        } else if !c[*i].is_whitespace() {
            current = true;
        }
        out.push(c[*i]);
        *i += 1;
    });
    parts + current as usize > 1
}

// ---- parameters ----------------------------------------------------------

fn is_text_type(t: &Type) -> bool {
    matches!(*t, Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN)
        || matches!(t.kind(), Kind::Enum(_))
        || t.name() == "citext"
}

fn encodes_directly(t: &Type) -> bool {
    matches!(*t, Type::BOOL | Type::INT2 | Type::INT4 | Type::INT8 | Type::FLOAT4 | Type::FLOAT8 | Type::JSON | Type::JSONB)
        || is_text_type(t)
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn type_name(t: &Type) -> String {
    if t.schema() == "pg_catalog" || t.schema().is_empty() {
        t.name().to_string()
    } else {
        format!("{}.{}", quote_ident(t.schema()), quote_ident(t.name()))
    }
}

/// A value's text for Postgres to parse (lists become array literals).
fn pg_text(v: &Value) -> String {
    match v {
        Value::List(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|x| match x {
                    Value::Nil => "NULL".to_string(),
                    Value::List(_) => pg_text(x),
                    other => format!("\"{}\"", pg_text(other).replace('\\', "\\\\").replace('"', "\\\"")),
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Map(_) => v.to_json().to_string(),
        other => other.display(),
    }
}

struct PgParam(Value);

impl std::fmt::Debug for PgParam {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0.repr())
    }
}

fn bad(v: &Value, t: &Type) -> BoxErr {
    format!("cannot send {} {} as {}", v.type_name(), v.repr(), t).into()
}

impl ToSql for PgParam {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, BoxErr> {
        let v = &self.0;
        if matches!(v, Value::Nil) {
            return Ok(IsNull::Yes);
        }
        let int = || -> Result<i64, BoxErr> {
            match v {
                Value::Int(n) => Ok(*n),
                Value::Float(f) if f.fract() == 0.0 && f.is_finite() => Ok(*f as i64),
                Value::Str(s) => s.trim().parse().map_err(|_| bad(v, ty)),
                _ => Err(bad(v, ty)),
            }
        };
        match *ty {
            Type::BOOL => match v {
                Value::Bool(b) => out.put_u8(*b as u8),
                _ => return Err(bad(v, ty)),
            },
            Type::INT2 => out.put_i16(i16::try_from(int()?).map_err(|_| bad(v, ty))?),
            Type::INT4 => out.put_i32(i32::try_from(int()?).map_err(|_| bad(v, ty))?),
            Type::INT8 => out.put_i64(int()?),
            Type::FLOAT4 | Type::FLOAT8 => {
                let f = match v {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    Value::Str(s) => s.trim().parse().map_err(|_| bad(v, ty))?,
                    _ => return Err(bad(v, ty)),
                };
                if *ty == Type::FLOAT4 {
                    out.put_f32(f as f32)
                } else {
                    out.put_f64(f)
                }
            }
            Type::JSON => out.put_slice(v.to_json().to_string().as_bytes()),
            Type::JSONB => {
                out.put_u8(1);
                out.put_slice(v.to_json().to_string().as_bytes());
            }
            _ => out.put_slice(pg_text(v).as_bytes()),
        }
        Ok(IsNull::No)
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    postgres::types::to_sql_checked!();
}

// ---- results -------------------------------------------------------------

struct PgValue(Value);

impl<'a> FromSql<'a> for PgValue {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, BoxErr> {
        Ok(PgValue(decode(ty, raw)?))
    }

    fn from_sql_null(_: &Type) -> Result<Self, BoxErr> {
        Ok(PgValue(Value::Nil))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

fn take<const N: usize>(raw: &[u8], at: &mut usize) -> Result<[u8; N], BoxErr> {
    let b: [u8; N] = raw.get(*at..*at + N).ok_or("value too short")?.try_into()?;
    *at += N;
    Ok(b)
}

fn decode(ty: &Type, raw: &[u8]) -> Result<Value, BoxErr> {
    let mut at = 0;
    Ok(match *ty {
        Type::BOOL => Value::Bool(raw.first().copied().unwrap_or(0) != 0),
        Type::INT2 => Value::Int(i16::from_be_bytes(take(raw, &mut at)?) as i64),
        Type::INT4 => Value::Int(i32::from_be_bytes(take(raw, &mut at)?) as i64),
        Type::INT8 => Value::Int(i64::from_be_bytes(take(raw, &mut at)?)),
        Type::OID => Value::Int(u32::from_be_bytes(take(raw, &mut at)?) as i64),
        Type::FLOAT4 => Value::Float(f32::from_be_bytes(take(raw, &mut at)?) as f64),
        Type::FLOAT8 => Value::Float(f64::from_be_bytes(take(raw, &mut at)?)),
        Type::NUMERIC => numeric(raw)?,
        Type::JSON => json(raw)?,
        Type::JSONB => json(raw.get(1..).unwrap_or_default())?,
        Type::UUID => {
            let h: String = raw.iter().map(|b| format!("{:02x}", b)).collect();
            if h.len() != 32 {
                return Err("bad uuid".into());
            }
            Value::str(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
        }
        Type::DATE => {
            let d = i32::from_be_bytes(take(raw, &mut at)?);
            Value::str(match d {
                i32::MAX => "infinity".into(),
                i32::MIN => "-infinity".into(),
                d => date_text(d as i64),
            })
        }
        Type::TIMESTAMP | Type::TIMESTAMPTZ => {
            let us = i64::from_be_bytes(take(raw, &mut at)?);
            Value::str(match us {
                i64::MAX => "infinity".into(),
                i64::MIN => "-infinity".into(),
                us => {
                    let days = us.div_euclid(86_400_000_000);
                    let rest = us.rem_euclid(86_400_000_000);
                    let z = if *ty == Type::TIMESTAMPTZ { "Z" } else { "" };
                    format!("{}T{}{}", date_text(days), time_text(rest), z)
                }
            })
        }
        Type::TIME => Value::str(time_text(i64::from_be_bytes(take(raw, &mut at)?))),
        Type::INTERVAL => {
            let us = i64::from_be_bytes(take(raw, &mut at)?);
            let days = i32::from_be_bytes(take(raw, &mut at)?);
            let months = i32::from_be_bytes(take(raw, &mut at)?);
            let mut parts = Vec::new();
            if months != 0 {
                parts.push(format!("{} mon{}", months, if months.abs() == 1 { "" } else { "s" }));
            }
            if days != 0 {
                parts.push(format!("{} day{}", days, if days.abs() == 1 { "" } else { "s" }));
            }
            if us != 0 || parts.is_empty() {
                let sign = if us < 0 { "-" } else { "" };
                parts.push(format!("{}{}", sign, time_text(us.abs())));
            }
            Value::str(parts.join(" "))
        }
        Type::BYTEA => Value::str(format!("\\x{}", raw.iter().map(|b| format!("{:02x}", b)).collect::<String>())),
        _ => match ty.kind() {
            Kind::Array(elem) => array(elem, raw)?,
            Kind::Domain(inner) => decode(inner, raw)?,
            _ => match std::str::from_utf8(raw) {
                // text, varchar, enums, citext and other text-like types
                Ok(s) => Value::str(s),
                Err(_) => Value::str(String::from_utf8_lossy(raw)),
            },
        },
    })
}

fn json(raw: &[u8]) -> Result<Value, BoxErr> {
    let j: serde_json::Value = serde_json::from_slice(raw)?;
    Ok(Value::from_json(&j))
}

/// Postgres's binary NUMERIC: an int when it has no fraction and fits,
/// else a float.
fn numeric(raw: &[u8]) -> Result<Value, BoxErr> {
    let mut at = 0;
    let ndigits = i16::from_be_bytes(take(raw, &mut at)?) as usize;
    let weight = i16::from_be_bytes(take(raw, &mut at)?) as i64;
    let sign = u16::from_be_bytes(take(raw, &mut at)?);
    let dscale = i16::from_be_bytes(take(raw, &mut at)?) as usize;
    match sign {
        0xC000 => return Ok(Value::Float(f64::NAN)),
        0xD000 => return Ok(Value::Float(f64::INFINITY)),
        0xF000 => return Ok(Value::Float(f64::NEG_INFINITY)),
        _ => {}
    }
    let mut digits = Vec::with_capacity(ndigits);
    for _ in 0..ndigits {
        digits.push(i16::from_be_bytes(take(raw, &mut at)?) as i64);
    }
    let mut int_part = String::new();
    for k in 0..=weight.max(-1) {
        let d = digits.get(k as usize).copied().unwrap_or(0);
        if int_part.is_empty() {
            if d != 0 || k == weight {
                int_part = d.to_string();
            }
        } else {
            int_part.push_str(&format!("{:04}", d));
        }
    }
    if int_part.is_empty() {
        int_part.push('0');
    }
    let mut frac = String::new();
    let mut k = weight + 1;
    while frac.len() < dscale {
        let d = if k < 0 { 0 } else { digits.get(k as usize).copied().unwrap_or(0) };
        frac.push_str(&format!("{:04}", d));
        k += 1;
    }
    frac.truncate(dscale);
    let neg = sign == 0x4000;
    let text = format!("{}{}{}{}", if neg { "-" } else { "" }, int_part, if frac.is_empty() { "" } else { "." }, frac);
    if frac.chars().all(|c| c == '0') {
        if let Ok(n) = format!("{}{}", if neg { "-" } else { "" }, int_part).parse::<i64>() {
            return Ok(Value::Int(n));
        }
    }
    Ok(text.parse::<f64>().map(Value::Float).unwrap_or_else(|_| Value::str(text)))
}

/// A one- or more-dimensional array as (nested) lists.
fn array(elem: &Type, raw: &[u8]) -> Result<Value, BoxErr> {
    let mut at = 0;
    let ndim = i32::from_be_bytes(take(raw, &mut at)?) as usize;
    let _has_null = i32::from_be_bytes(take(raw, &mut at)?);
    let _oid = u32::from_be_bytes(take(raw, &mut at)?);
    let mut dims = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        dims.push(i32::from_be_bytes(take(raw, &mut at)?) as usize);
        let _lower = i32::from_be_bytes(take(raw, &mut at)?);
    }
    let total: usize = if ndim == 0 { 0 } else { dims.iter().product() };
    let mut items = Vec::with_capacity(total);
    for _ in 0..total {
        let len = i32::from_be_bytes(take(raw, &mut at)?);
        if len < 0 {
            items.push(Value::Nil);
        } else {
            let len = len as usize;
            let bytes = raw.get(at..at + len).ok_or("array too short")?;
            at += len;
            items.push(decode(elem, bytes)?);
        }
    }
    fn nest(items: &mut std::vec::IntoIter<Value>, dims: &[usize]) -> Value {
        match dims {
            [] => Value::Nil,
            [n] => Value::list(items.take(*n).collect()),
            [n, rest @ ..] => Value::list((0..*n).map(|_| nest(items, rest)).collect()),
        }
    }
    Ok(if dims.is_empty() { Value::list(Vec::new()) } else { nest(&mut items.into_iter(), &dims) })
}

/// Days since 2000-01-01 as YYYY-MM-DD.
fn date_text(days: i64) -> String {
    // Howard Hinnant's civil-from-days, shifted from 1970 to 2000.
    let z = days + 10957 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Microseconds since midnight as HH:MM:SS[.ffffff].
fn time_text(us: i64) -> String {
    let secs = us / 1_000_000;
    let frac = us % 1_000_000;
    let base = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
    if frac == 0 {
        base
    } else {
        format!("{}.{}", base, format!("{:06}", frac).trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        let p = |s: &str, v: &[Value]| translate(s, v).unwrap().0;
        assert_eq!(p("select * from t where a = ? and b = ?", &[Value::Int(1), Value::Int(2)]), "select * from t where a = $1 and b = $2");
        assert_eq!(p("select '?', \"q?\", e'\\'?' -- ?\n, $$ ? $$, ? /* ? */", &[Value::Int(1)]), "select '?', \"q?\", e'\\'?' -- ?\n, $$ ? $$, $1 /* ? */");
        assert_eq!(p("select $1, ?", &[]), "select $1, ?");
        assert_eq!(p("select data ?| array['a'] from t where x = ?", &[]), "select data ?| array['a'] from t where x = $1");
        let mut m = Map::new();
        m.insert("id".into(), Value::Int(1));
        m.insert("name".into(), Value::str("x"));
        let (sql, vals) = translate("select :id::text, :name, :id, ':no'", &[Value::map(m)]).unwrap();
        assert_eq!(sql, "select $1::text, $2, $1, ':no'");
        assert_eq!(vals.len(), 2);
        assert_eq!(rewrite_numbered("a = $1 and b = $12", |n| (n == 12).then(|| "X".into())), "a = $1 and b = X");
        assert!(has_several_statements("create table a (x int); create table b (y int);"));
        assert!(!has_several_statements("insert into a values (';');"));
    }

    #[test]
    fn dates_and_numbers() {
        assert_eq!(date_text(0), "2000-01-01");
        assert_eq!(date_text(-1), "1999-12-31");
        assert_eq!(date_text(9131), "2024-12-31");
        assert_eq!(time_text(3_723_000_001), "01:02:03.000001");
        assert_eq!(pg_text(&Value::list(vec![Value::Int(1), Value::str("a\"b"), Value::Nil])), "{\"1\",\"a\\\"b\",NULL}");
        assert_eq!(take_pool_size("postgres://h/db?pool_size=3&sslmode=require").unwrap(), ("postgres://h/db?sslmode=require".into(), 3));
    }
}
