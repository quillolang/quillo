//! MySQL and MariaDB for `db` (`mysql://user:pass@host:3306/app`): a pool
//! of connections, placeholders, and conversion between Quillo values and
//! MySQL's types. Built with the `mysql` feature (on by default).
//!
//! Statements are prepared, so values never become SQL text, and results
//! come back typed: ints, floats, strings, `tinyint(1)` as a bool, DECIMAL
//! as an int when whole (else a float), JSON columns as maps and lists
//! (MySQL's; MariaDB's JSON is text), and dates and times as ISO text.
//! `?` is MySQL's own placeholder; `:name`, with a map, is translated to it.
//!
//! The driver is async (on the runtime the Postgres pool uses); `db` calls
//! it from a pool thread, while the server answers other requests.

use crate::value::*;
use mysql_async::prelude::*;
use mysql_async::consts::ColumnType;
use mysql_async::{Opts, OptsBuilder, Params, Value as My};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

pub fn is_url(s: &str) -> bool {
    s.starts_with("mysql://") || s.starts_with("mariadb://")
}

/// The connection options of a URL. TLS is required (the server's
/// certificate and name checked) for a server on another machine, as the
/// driver would otherwise send the password in the clear, unless the URL
/// says `require_ssl=false`; on this one, when the URL says
/// `require_ssl=true` or names a CA. The TLS options are read here, as the
/// driver reads them only with `require_ssl=true`:
///
/// - `ssl_ca=/path/ca.pem`: the CAs (PEM) to check the server's certificate
///   against, instead of the usual public ones (a cloud's own, or a
///   company's);
/// - `ssl_server_name=db.example.com`: the name the certificate is checked
///   for, where the host connected to is another (an address, a tunnel);
/// - `verify_ca=false`: the certificate is not checked at all (encrypted,
///   but someone in the middle can pose as the server);
/// - `built_in_roots=true`: the public CAs as well as those in `ssl_ca`;
/// - `ssl_cert` and `ssl_key`: a certificate and its key, for servers that
///   ask for one (`REQUIRE X509`).
fn opts_of(url: &str) -> Result<Opts, String> {
    // `mariadb://` is the same protocol.
    let url = match url.strip_prefix("mariadb://") {
        Some(rest) => format!("mysql://{}", rest),
        None => url.to_string(),
    };
    let (base, query) = url.split_once('?').unwrap_or((&url, ""));
    let (mut require, mut verify_ca, mut verify_identity, mut built_in) = (None, true, true, None);
    let (mut ca, mut cert, mut key, mut name) = (None, None, None, None);
    let mut rest = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let flag = |v: &str| v.parse::<bool>().map_err(|_| format!("{} is true or false, not {:?}", k, v));
        let path = || std::path::PathBuf::from(crate::server::percent_decode(v, false));
        match k {
            // For the tenants' list (see `TenantDbs`), not for the connection.
            "open_tenants" => {
                v.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| format!("bad open_tenants: {}", v))?;
            }
            "require_ssl" => require = Some(flag(v)?),
            "verify_ca" => verify_ca = flag(v)?,
            "verify_identity" => verify_identity = flag(v)?,
            "ssl_server_name" => name = Some(crate::server::percent_decode(v, false)),
            "built_in_roots" => built_in = Some(flag(v)?),
            "ssl_ca" => ca = Some(path()),
            "ssl_cert" => cert = Some(path()),
            "ssl_key" => key = Some(path()),
            _ => rest.push(pair),
        }
    }
    let found_rows = !rest.iter().any(|p| p.starts_with("client_found_rows="));
    let url = if rest.is_empty() { base.to_string() } else { format!("{}?{}", base, rest.join("&")) };
    let opts = Opts::from_url(&url).map_err(|e| e.to_string())?;
    let mut b = OptsBuilder::from_opts(opts.clone());
    // An IPv6 address, `[::1]`, without its brackets, which the driver looked up as a name
    // (FEEDBACK55.md, Q-756; branch feedback55).
    let host = opts.ip_or_hostname();
    if let Some(ip) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        b = b.ip_or_hostname(ip.to_string());
    }
    // `changes` counts the rows a statement matched, as SQLite and PostgreSQL count them: MySQL
    // counted those it changed, so an update to the value a row had was 0, and "update, else
    // insert" inserted a second row (FEEDBACK55.md, Q-739; branch feedback55).
    b = b.client_found_rows(found_rows || opts.client_found_rows());
    let opts: Opts = b.into();
    let remote = opts.socket().is_none() && !crate::pg::local_host(opts.ip_or_hostname());
    if !require.unwrap_or(remote || ca.is_some()) {
        return Ok(opts);
    }
    // The driver's check of the name alone fails with the rustls it is
    // built with (it looks for the error's old wording), so the name is
    // always checked: one the certificate is for is given instead.
    if !verify_identity && verify_ca {
        return Err("verify_identity=false is not supported: connect by a name the server's certificate is for, or give that name with ssl_server_name=".into());
    }
    let identity = match (cert, key) {
        (None, None) => None,
        (Some(c), Some(k)) => Some(mysql_async::ClientIdentity::new(c.into(), k.into())),
        _ => return Err("ssl_cert and ssl_key go together: a certificate, and its key".into()),
    };
    let ssl = mysql_async::SslOpts::default()
        .with_danger_accept_invalid_certs(!verify_ca)
        // As the mysql client's --ssl-ca: those CAs, not the public ones too.
        .with_disable_built_in_roots(!built_in.unwrap_or(ca.is_none()))
        .with_root_certs(ca.into_iter().map(Into::into).collect())
        .with_client_identity(identity)
        .with_danger_tls_hostname_override(name);
    Ok(OptsBuilder::from_opts(opts).ssl_opts(ssl).into())
}

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    crate::pg::runtime().block_on(f)
}

/// The database's connections.
pub struct Pool {
    pool: Option<mysql_async::Pool>,
    /// What its connections asked the server, once for all of them.
    asked: Arc<Asked>,
}

/// What a pool's connections asked the server, kept for all of them.
#[derive(Default)]
struct Asked {
    /// The server's `max_allowed_packet` (0 before asked).
    packet: AtomicUsize,
    /// Tables' columns and their types, in lower case (`Conn::by_columns`); let go when the
    /// program changes a schema.
    columns: Mutex<HashMap<String, Vec<(String, String)>>>,
    /// Names called as functions, and whether each is one of the database's own (`calls_own`).
    fns: Mutex<HashMap<String, bool>>,
}

/// A connection; in a transaction that retries, whether a statement failed because of another
/// transaction (see `crate::txn`; None: not watching); whether Quillo began a transaction on
/// it (`batch`); what its pool asked the server (`Asked`); and whether its session reads `||`
/// as the other databases do (`bars`).
pub struct Conn(Option<mysql_async::Conn>, Option<bool>, bool, Arc<Asked>, bool);

impl Drop for Pool {
    fn drop(&mut self) {
        if let Some(p) = self.pool.take() {
            crate::pg::runtime().spawn(async move {
                let _ = p.disconnect().await;
            });
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // Back to its pool, which takes the runtime to do.
        if let Some(c) = self.0.take() {
            let _in = crate::pg::runtime().enter();
            drop(c);
        }
    }
}

impl Pool {
    /// Connects (to check that it can); with `make`, makes the database
    /// first if the server does not have it (a tenant's, for one).
    pub fn open_as(url: &str, make: bool) -> Result<Pool, String> {
        let opts = opts_of(url)?;
        if make {
            if let Some(name) = opts.db_name().map(str::to_string) {
                let server = OptsBuilder::from_opts(opts.clone()).db_name(None::<String>);
                run(async {
                    let mut c = mysql_async::Conn::new(server).await.map_err(|e| e.to_string())?;
                    c.query_drop(format!("create database if not exists `{}`", name.replace('`', "``"))).await.map_err(|e| e.to_string())?;
                    c.disconnect().await.map_err(|e| e.to_string())
                })?;
            }
        }
        let pool = mysql_async::Pool::new(opts);
        let asked = Arc::new(Asked::default());
        let c = run(pool.get_conn()).map_err(|e| e.to_string())?;
        drop(Conn(Some(c), None, false, asked.clone(), false));
        Ok(Pool { pool: Some(pool), asked })
    }

    /// Makes the database a URL names, or says false when it is there. Of two at once, the
    /// server lets one make it (FEEDBACK20.md, Q-165; branch feedback20-fixes).
    pub fn claim(url: &str) -> Result<bool, String> {
        let opts = opts_of(url)?;
        let name = opts.db_name().unwrap_or("").replace('`', "``");
        let server = OptsBuilder::from_opts(opts.clone()).db_name(None::<String>);
        run(async {
            let mut c = mysql_async::Conn::new(server).await.map_err(|e| e.to_string())?;
            let made = match c.query_drop(format!("create database `{}`", name)).await {
                Ok(()) => Ok(true),
                Err(e) if e.to_string().contains("database exists") => Ok(false),
                Err(e) => Err(e.to_string()),
            };
            let _ = c.disconnect().await;
            made
        })
    }

    pub fn checkout(&self) -> Result<Conn, String> {
        let pool = self.pool.as_ref().expect("open");
        run(pool.get_conn()).map(|c| Conn(Some(c), None, false, self.asked.clone(), false)).map_err(|e| e.to_string())
    }

    /// Whether one of these names is a function of the database's own (which may write, so a
    /// replica is not asked), as far as known: None when a name was never looked up. MySQL's
    /// own functions are not in `information_schema.routines` (FEEDBACK55.md, Q-750; found in
    /// review, branch feedback55).
    pub fn calls_own(&self, names: &[Box<str>]) -> Option<bool> {
        let known = self.asked.fns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut own = false;
        for n in names {
            own |= *known.get(&**n)?;
        }
        Some(own)
    }

    /// Looks the names up, and keeps what they are.
    pub fn look_up_fns(&self, names: &[Box<str>]) -> Result<bool, String> {
        let mut c = self.checkout()?;
        let sql = format!(
            "select lower(routine_name) as name from information_schema.routines where routine_schema = database() and routine_type = 'FUNCTION' and lower(routine_name) in ({})",
            vec!["?"; names.len()].join(", ")
        );
        let rows = c.query(&sql, &names.iter().map(|n| Value::str(&**n)).collect::<Vec<_>>(), None)?;
        let own: Vec<String> = rows.iter().filter_map(|r| if let Value::Map(m) = r { m.get("name").map(|v| v.display().to_string()) } else { None }).collect();
        let mut known = self.asked.fns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if known.len() >= 4096 {
            known.clear();
        }
        for n in names {
            known.insert(n.to_string(), own.iter().any(|o| o == &**n));
        }
        Ok(!own.is_empty())
    }

    /// The program made or dropped functions: what names are is looked up again.
    pub fn forget_fns(&self) {
        self.asked.fns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }

    pub fn give_back(&self, c: Conn) {
        drop(c);
    }

    /// The names of the server's databases (for `db.tenants`).
    pub fn databases(&self) -> Result<Vec<String>, String> {
        let mut c = self.checkout()?;
        let rows = c.query("show databases", &[], None)?;
        Ok(rows.iter().filter_map(|r| if let Value::Map(m) = r { m.values().next().map(|v| v.display()) } else { None }).collect())
    }
}

impl Conn {
    fn conn(&mut self) -> &mut mysql_async::Conn {
        self.0.as_mut().expect("connected")
    }

    pub fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        let (scan, mut params) = translate(sql, params)?;
        let sql = scan.native().to_string();
        self.fits(&sql, &params)?;
        self.bars(&scan)?;
        self.by_columns(&scan, &mut params)?;
        let c = self.conn();
        let r = run(async move {
            let mut result = c.exec_iter(sql, params).await?;
            let cols = result.columns().unwrap_or_else(|| Vec::new().into());
            let mut out = Vec::new();
            while let Some(row) = result.next().await? {
                let mut m = Map::with_capacity(cols.len());
                for (i, col) in cols.iter().enumerate() {
                    let v = row.as_ref(i).cloned().unwrap_or(My::NULL);
                    m.insert(col.name_str().as_ref().into(), from_my(v, col));
                }
                out.push(Value::map(m));
                if limit.map_or(false, |l| out.len() >= l) {
                    break;
                }
            }
            result.drop_result().await?;
            Ok::<_, mysql_async::Error>(out)
        });
        let rows = r.map_err(|e| self.failed(e))?;
        self.still_in_tx()?;
        Ok(rows)
    }

    /// After a statement in a transaction Quillo began: whether the server is still in it, by
    /// the flag its answer carries. A statement that committed by itself (a procedure's `create
    /// table`, `execute immediate`, MariaDB's `begin not atomic` block) ended it, with what ran
    /// before (FEEDBACK55.md, Q-728; branch feedback55).
    fn still_in_tx(&mut self) -> Result<(), String> {
        if !self.2 {
            return Ok(());
        }
        let in_tx = self.conn().last_ok_packet().map_or(true, |ok| ok.status_flags().contains(mysql_async::consts::StatusFlags::SERVER_STATUS_IN_TRANS));
        if in_tx {
            return Ok(());
        }
        self.2 = false;
        Err(crate::db::COMMITTED.to_string())
    }

    /// Before SQL with `||`: the session reads it as text joined, as SQLite and PostgreSQL do,
    /// where MariaDB read it as `or` (`'%' || ? || '%'` was 0 or 1; FEEDBACK55.md, after slice 5;
    /// branch feedback55). PIPES_AS_CONCAT is added to the session's sql_mode, nothing else of
    /// ANSI's, until the pool resets the connection: for such SQL only, as MariaDB sends every
    /// row slower under it (300,000 rows, 20 ms more).
    fn bars(&mut self, scan: &crate::sql::Scan) -> Result<(), String> {
        if !scan.has_bars() || self.4 {
            return Ok(());
        }
        let r = run(self.conn().query_drop("set session sql_mode = concat_ws(',', nullif(@@sql_mode, ''), 'PIPES_AS_CONCAT')"));
        r.map_err(|e| self.failed(e))?;
        self.4 = true;
        Ok(())
    }

    /// Values whose column says what they are, as the others take them (FEEDBACK55.md; found
    /// in review, branch feedback55): a `BIT`'s digits, as Quillo reads one (`00000101`), as its
    /// number, where MySQL took the text's bytes (1406 Data too long) and compared none alike
    /// (Q-741); a float a `FLOAT` would keep as 0 (1e-46), refused as out of its range, as
    /// PostgreSQL's `real` refuses it (Q-734). Columns are known by the SQL beside the value
    /// (`sql::Scan::column`) and asked of the server once a table, only for such values.
    fn by_columns(&mut self, scan: &crate::sql::Scan, params: &mut Params) -> Result<(), String> {
        let Params::Positional(values) = params else { return Ok(()) };
        let want: Vec<(usize, String)> = values
            .iter()
            .enumerate()
            .filter(|(_, v)| match v {
                My::Bytes(b) => !b.is_empty() && b.len() <= 64 && b.iter().all(|c| *c == b'0' || *c == b'1'),
                My::Double(f) => *f != 0.0 && *f as f32 == 0.0,
                _ => false,
            })
            .filter_map(|(i, _)| scan.column(scan.native_slot(i)).map(|c| (i, c.to_string())))
            .collect();
        if want.is_empty() {
            return Ok(());
        }
        let tables: Vec<String> = scan.tables.reads.iter().chain(&scan.tables.writes).map(|t| t.to_lowercase()).collect();
        let types = self.column_types(&tables);
        for (i, col) in want {
            let kinds: Vec<&str> = tables.iter().filter_map(|t| types.get(t)).flatten().filter(|(c, _)| *c == col).map(|(_, ty)| ty.as_str()).collect();
            let only = |ty: &str| !kinds.is_empty() && kinds.iter().all(|k| *k == ty);
            match &values[i] {
                My::Bytes(b) if only("bit") => {
                    let digits = std::str::from_utf8(b).unwrap_or("0");
                    values[i] = My::UInt(u64::from_str_radix(digits, 2).unwrap_or(0));
                }
                My::Double(f) if only("float") => {
                    return Err(format!("value {} is {:e}, out of range for the FLOAT column `{}`, which would keep it as 0", scan.native_slot(i), f, col));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The columns of these tables and their types, asked of the server once a table.
    fn column_types(&mut self, tables: &[String]) -> HashMap<String, Vec<(String, String)>> {
        let asked = self.3.clone();
        let missing: Vec<String> = {
            let known = asked.columns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            tables.iter().filter(|t| !known.contains_key(*t)).cloned().collect()
        };
        if !missing.is_empty() {
            let sql = format!(
                "select lower(table_name) as t, lower(column_name) as c, lower(data_type) as ty from information_schema.columns where table_schema = database() and lower(table_name) in ({})",
                vec!["?"; missing.len()].join(", ")
            );
            let args = Params::Positional(missing.iter().map(|t| My::Bytes(t.as_bytes().to_vec())).collect());
            let rows: Result<Vec<(String, String, String)>, _> = run(self.conn().exec(sql, args));
            if let Ok(rows) = rows {
                let mut known = asked.columns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if known.len() >= 4096 {
                    known.clear();
                }
                for t in &missing {
                    known.insert(t.clone(), rows.iter().filter(|(rt, _, _)| rt == t).map(|(_, c, ty)| (c.clone(), ty.clone())).collect());
                }
            }
        }
        let known = asked.columns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        tables.iter().filter_map(|t| known.get(t).map(|cols| (t.clone(), cols.clone()))).collect()
    }

    /// After SQL that may change a schema (`create`, `alter`, `drop`, `rename`), what columns
    /// were asked is asked again.
    fn changed(&self, scan: &crate::sql::Scan) {
        if scan.heads().any(|(a, _)| ["create", "alter", "drop", "rename"].iter().any(|w| a.eq_ignore_ascii_case(w))) {
            self.3.columns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        }
    }

    /// The server's `max_allowed_packet`: asked once a pool, when a statement is large enough
    /// to need it.
    fn packet_limit(&mut self) -> Result<usize, String> {
        let known = self.3.packet.load(Relaxed);
        if known > 0 {
            return Ok(known);
        }
        let n: Option<usize> = run(self.conn().query_first("select @@max_allowed_packet")).map_err(|e| e.to_string())?;
        let n = n.unwrap_or(usize::MAX);
        self.3.packet.store(n, Relaxed);
        Ok(n)
    }

    /// A statement whose text, or values, are more than the server takes in one packet
    /// (`max_allowed_packet`), refused saying so: the driver sent it, and it failed as "Broken
    /// pipe" or "Packets out of sync", in turn (FEEDBACK55.md, Q-751; branch feedback55). The
    /// values go in one packet with the statement's id up to 16 MB, each in packets of its own
    /// past that.
    fn fits(&mut self, sql: &str, params: &Params) -> Result<(), String> {
        let (mut total, mut largest) = (16, 0);
        if let Params::Positional(v) = params {
            for x in v {
                let n = if let My::Bytes(b) = x { b.len() + 9 } else { 9 };
                total += n;
                largest = largest.max(n);
            }
        }
        let need = sql.len().max(if total < mysql_async::consts::MAX_PAYLOAD_LEN { total } else { largest });
        // Below the least a server can be set to.
        if need < 1024 {
            return Ok(());
        }
        let limit = self.packet_limit()?;
        if need <= limit {
            return Ok(());
        }
        Err(format!(
            "this statement is {} bytes with its values, past the server's max_allowed_packet of {} bytes, the most MySQL and MariaDB take in one: raise max_allowed_packet on the server, or send less at once",
            need, limit
        ))
    }

    /// `{ changes, last_id }`. Without parameters the text may hold several
    /// statements. As the other databases count them (FEEDBACK55.md, Q-753; branch
    /// feedback55): `changes` is the rows matched (`CLIENT_FOUND_ROWS`, Q-739), a row an
    /// upsert or a `replace` took counted once, where MySQL counts an update or a replacement
    /// twice; `last_id` the id of the one row the statement inserted, past 2^63 a `dec` (it was
    /// negative; Q-740), and null for none or several (MySQL gave a multi-row insert's first,
    /// and 0 for none).
    pub fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        let scan = crate::sql::scan(crate::sql::Dialect::Mysql, sql);
        self.bars(&scan)?;
        let r = if params.is_empty() {
            self.fits(sql, &Params::Empty)?;
            run(self.conn().query_drop(sql))
        } else {
            let (scan, mut params) = translate(sql, params)?;
            let sql = scan.native().to_string();
            self.fits(&sql, &params)?;
            self.by_columns(&scan, &mut params)?;
            run(self.conn().exec_drop(sql, params))
        };
        r.map_err(|e| self.failed(e))?;
        self.changed(&scan);
        self.still_in_tx()?;
        let c = self.conn();
        let raw = c.affected_rows();
        let twice = scan.counts_twice();
        let changes = if twice { records(&c.info()).unwrap_or(raw.min(1)) } else { raw };
        let head = scan.heads().last().map(|(a, _)| a.to_ascii_lowercase()).unwrap_or_default();
        // An upsert's update counts 2, and its insert 1.
        let one = if twice && head == "insert" { raw == 1 } else { changes == 1 };
        let id = c.last_insert_id().filter(|&n| n > 0 && one && (head == "insert" || head == "replace"));
        let mut m = Map::new();
        m.insert("changes".into(), Value::Int(changes as i64));
        m.insert("last_id".into(), id.map_or(Value::Nil, unsigned));
        Ok(Value::map(m))
    }

    pub fn batch(&mut self, sql: &str) -> Result<(), String> {
        let scan = crate::sql::scan(crate::sql::Dialect::Mysql, sql);
        // A migration's `||` too (found in review).
        self.bars(&scan)?;
        let r = run(self.conn().query_drop(sql));
        self.changed(&scan);
        // Quillo's own `begin`, `commit`, `rollback` (the program's are refused); and a
        // migration's schema, which commits as it runs there, so not in a transaction after.
        let after = crate::sql::in_tx_after(&scan, self.2);
        let server = self.conn().last_ok_packet().map_or(true, |ok| ok.status_flags().contains(mysql_async::consts::StatusFlags::SERVER_STATUS_IN_TRANS));
        self.2 = if r.is_ok() { after && server } else { self.2 && after };
        r.map_err(|e| self.failed(e))
    }

    /// The error's message; a deadlock (1213, which rolls the whole
    /// transaction back), a lock wait that timed out (1205), a row changed
    /// since this transaction's snapshot (1020: MariaDB's snapshot
    /// isolation, on by default since 11.6) or a table made since the
    /// transaction began (1412) is also noted, when watching, for the
    /// transaction to run again.
    fn failed(&mut self, e: mysql_async::Error) -> String {
        if let (Some(c), mysql_async::Error::Server(s)) = (self.1.as_mut(), &e) {
            if matches!(s.code, 1213 | 1205 | 1020 | 1412) {
                *c = true;
            }
        }
        e.to_string()
    }

    /// Starts (or stops) noting conflicts with other transactions.
    pub fn watch_conflicts(&mut self, on: bool) {
        self.1 = on.then_some(false);
    }

    /// Whether a statement failed because of another transaction since
    /// `watch_conflicts(true)`.
    pub fn conflicted(&self) -> bool {
        self.1 == Some(true)
    }
}

fn to_my(v: &Value) -> Result<My, String> {
    Ok(match v {
        Value::Nil => My::NULL,
        Value::Bool(b) => My::Int(*b as i64),
        Value::Int(n) => My::Int(*n),
        // -0.0 as 0.0, on every database (FEEDBACK55.md, Q-735; branch feedback55).
        Value::Float(f) => My::Double(*f + 0.0),
        // A whole one as an integer, which MySQL compares as one: as text, an unsigned id past
        // 2^63 was compared as a float, and matched its neighbours' rows (FEEDBACK55.md, Q-756;
        // branch feedback55). Else as text, which MySQL converts to a DECIMAL exactly.
        Value::Dec(d) if d.scale() == 0 && i64::try_from(d.mantissa()).is_ok() => My::Int(d.mantissa() as i64),
        Value::Dec(d) if d.scale() == 0 && u64::try_from(d.mantissa()).is_ok() => My::UInt(d.mantissa() as u64),
        Value::Dec(d) => My::Bytes(d.to_string().into_bytes()),
        Value::Str(s) => My::Bytes(s.as_bytes().to_vec()),
        Value::Html(h) => My::Bytes(h.as_bytes().to_vec()),
        Value::List(_) | Value::Map(_) => My::Bytes(crate::db::json_text(v).into_bytes()),
        other => return Err(format!("a {} cannot be stored in the database", other.type_name())),
    })
}

/// The rows a statement took, from the server's note on one of several rows ("Records: 3
/// Duplicates: 1 Warnings: 0").
fn records(info: &str) -> Option<u64> {
    info.strip_prefix("Records: ")?.split_whitespace().next()?.parse().ok()
}

/// An unsigned integer: an int, or past 2^63 a `dec`, where it was a float (2^64 - 1 and
/// 2^64 - 2 read equal, and matched each other's rows) or a negative int (FEEDBACK55.md,
/// Q-740, Q-756; branch feedback55).
fn unsigned(n: u64) -> Value {
    i64::try_from(n).map(Value::Int).unwrap_or_else(|_| Value::Dec(crate::decimal::Dec::new(n as i128, 0).expect("a u64 fits a dec")))
}

fn from_my(v: My, col: &mysql_async::Column) -> Value {
    let ty = col.column_type();
    match v {
        My::NULL => Value::Nil,
        // `tinyint(1)` is MySQL's boolean, but holds any byte: 2 or -1 read `true` (FEEDBACK55.md,
        // Q-756; branch feedback55).
        My::Int(n @ (0 | 1)) if ty == ColumnType::MYSQL_TYPE_TINY && col.column_length() == 1 => Value::Bool(n == 1),
        My::Int(n) => Value::Int(n),
        My::UInt(n) => unsigned(n),
        // -0 as 0.0, as every database is given (FEEDBACK55.md, Q-735; branch feedback55).
        My::Float(f) => Value::Float(crate::pg::f32_wide(f) + 0.0),
        My::Double(f) => Value::Float(f + 0.0),
        // A `BIT(n)` as PostgreSQL gives `bit(n)`: its n digits, "00000101", where its bytes were
        // passed off as text (FEEDBACK55.md, Q-741; branch feedback55).
        My::Bytes(b) if ty == ColumnType::MYSQL_TYPE_BIT && !b.is_empty() => {
            let n = (col.column_length() as usize).clamp(1, b.len() * 8);
            Value::str(b.iter().flat_map(|x| (0..8).rev().map(move |i| if x >> i & 1 == 1 { '1' } else { '0' })).skip(b.len() * 8 - n).collect::<String>())
        }
        My::Bytes(b) => {
            let text = String::from_utf8_lossy(&b);
            match ty {
                // An int without a scale (as `sum` of ints gives), else a
                // decimal with its scale, else (too many digits) the text.
                ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => match text.parse::<i64>() {
                    Ok(n) => Value::Int(n),
                    Err(_) => crate::decimal::Dec::parse(&text).map_or_else(|| Value::str(&*text), Value::Dec),
                },
                // Read as PostgreSQL's json is (FEEDBACK55.md, Q-732; branch feedback55).
                ColumnType::MYSQL_TYPE_JSON => crate::pg::json_cell(&b),
                _ => Value::str(text),
            }
        }
        My::Date(y, mo, d, h, mi, s, us) => {
            if ty == ColumnType::MYSQL_TYPE_DATE {
                Value::str(format!("{:04}-{:02}-{:02}", y, mo, d))
            } else if us > 0 {
                Value::str(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}", y, mo, d, h, mi, s, us))
            } else {
                Value::str(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s))
            }
        }
        My::Time(neg, days, h, mi, s, us) => {
            let hours = days as u64 * 24 + h as u64;
            let sign = if neg { "-" } else { "" };
            if us > 0 {
                Value::str(format!("{}{:02}:{:02}:{:02}.{:06}", sign, hours, mi, s, us))
            } else {
                Value::str(format!("{}{:02}:{:02}:{:02}", sign, hours, mi, s))
            }
        }
    }
}

/// `:name` and `?NNN` placeholders to MySQL's `?`, and the values in that order, read by the
/// tokenizer shared with the other databases (src/sql.rs): quotes, backticks, `#` and `-- `
/// comments skipped. One map is the values by name only where the SQL names them, so a map or
/// a record is the value of a lone `?` as on the others (FEEDBACK55.md, Q-748; branch
/// feedback55). Read once for each text.
fn translate(sql: &str, params: &[Value]) -> Result<(Arc<crate::sql::Scan>, Params), String> {
    let scan = crate::sql::scan(crate::sql::Dialect::Mysql, sql);
    let values = scan.values(params)?.iter().map(to_my).collect::<Result<Vec<_>, _>>()?;
    Ok((scan, if values.is_empty() { Params::Empty } else { Params::Positional(values) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_is_required_for_a_server_on_another_machine() {
        // What the review saw: TLS off unless `require_ssl=true`.
        for url in ["mysql://u:p@db.example.com/app", "mariadb://u:p@10.0.0.5:3306/app", "mysql://u@localhost/app?ssl_ca=/ca.pem"] {
            assert!(opts_of(url).unwrap().ssl_opts().is_some(), "{}", url);
        }
        for url in ["mysql://u:p@db.internal/app?require_ssl=false", "mysql://u@localhost/app", "mysql://u@127.0.0.1:3306/app"] {
            assert!(opts_of(url).unwrap().ssl_opts().is_none(), "{}", url);
        }
    }

    #[test]
    fn tls_options_are_read_whether_or_not_tls_is_asked_for() {
        let ssl = |url: &str| opts_of(url).unwrap().ssl_opts().cloned().unwrap();
        // Checked fully, against the public CAs, unless the URL says otherwise.
        let s = ssl("mysql://u@db.example.com/app");
        assert!(!s.accept_invalid_certs() && !s.skip_domain_validation() && !s.disable_built_in_roots() && s.root_certs().is_empty());
        // The driver dropped these without `require_ssl=true`.
        let s = ssl("mysql://u@10.0.0.5/app?ssl_server_name=db.example.com&pool_min=1");
        assert_eq!(s.tls_hostname_override(), Some("db.example.com"));
        assert!(ssl("mysql://u@db.example.com/app?verify_ca=false").accept_invalid_certs());
        assert!(opts_of("mysql://u@10.0.0.5/app?verify_identity=false").unwrap_err().contains("ssl_server_name"));
        assert!(ssl("mysql://u@10.0.0.5/app?verify_identity=false&verify_ca=false").accept_invalid_certs());
        // A CA of its own: only it, unless the public ones are asked for too.
        let s = ssl("mysql://u@db.example.com/app?ssl_ca=%2Fetc%2Fdb%20ca.pem");
        assert!(s.disable_built_in_roots());
        assert_eq!(format!("{:?}", s.root_certs()), r#"[Path("/etc/db ca.pem")]"#);
        assert!(!ssl("mysql://u@db.example.com/app?ssl_ca=/ca.pem&built_in_roots=true").disable_built_in_roots());
        assert!(ssl("mysql://u@db.example.com/app?ssl_cert=/c.pem&ssl_key=/k.pem").client_identity().is_some());
        assert!(opts_of("mysql://u@db.example.com/app?ssl_cert=/c.pem").is_err());
        assert!(opts_of("mysql://u@db.example.com/app?verify_ca=no").is_err());
    }

    // FEEDBACK55.md, Q-739, Q-740, Q-756.
    #[test]
    fn what_the_driver_is_told_and_counts() {
        // An IPv6 host without its brackets; rows matched unless the URL says otherwise.
        let o = opts_of("mysql://root@[::1]:3306/app").unwrap();
        assert_eq!((o.ip_or_hostname(), o.client_found_rows()), ("::1", true));
        assert_eq!(opts_of("mysql://root@[fe80::1]/app?require_ssl=false").unwrap().ip_or_hostname(), "fe80::1");
        assert_eq!(opts_of("mysql://root@db.internal/app?client_found_rows=false&require_ssl=false").unwrap().client_found_rows(), false);
        assert_eq!(opts_of("mysql://root@127.0.0.1/app").unwrap().ip_or_hostname(), "127.0.0.1");
        assert_eq!(records("Records: 3  Duplicates: 1  Warnings: 0"), Some(3));
        assert_eq!(records("Rows matched: 1  Changed: 0  Warnings: 0"), None);
        assert_eq!(records(""), None);
        assert_eq!(unsigned(9_223_372_036_854_775_807).repr(), "9223372036854775807");
        assert_eq!(unsigned(9_223_372_036_854_775_808).repr(), "9223372036854775808m");
        assert_eq!(unsigned(u64::MAX).repr(), "18446744073709551615m");
        let dec = |t: &str| to_my(&Value::Dec(crate::decimal::Dec::parse(t).unwrap())).unwrap();
        assert_eq!(dec("9223372036854775808"), My::UInt(9_223_372_036_854_775_808));
        assert_eq!(dec("-5"), My::Int(-5));
        assert_eq!(dec("2.50"), My::Bytes(b"2.50".to_vec()));
        assert_eq!(dec("18446744073709551616"), My::Bytes(b"18446744073709551616".to_vec()));
        assert_eq!(to_my(&Value::Float(-0.0)).unwrap(), My::Double(0.0));
    }

    #[test]
    fn placeholders() {
        let mut m = Map::new();
        m.insert("id".into(), Value::Int(1));
        m.insert("name".into(), Value::str("x"));
        let (scan, p) = translate("select :id, ':no', `:no`, :name -- :no\n, :id", &[Value::map(m)]).unwrap();
        assert_eq!(scan.native(), "select ?, ':no', `:no`, ? -- :no\n, ?");
        assert!(matches!(p, Params::Positional(v) if v.len() == 3));
        let (scan, _) = translate("select ? + ?", &[Value::Int(1), Value::Int(2)]).unwrap();
        assert_eq!(scan.native(), "select ? + ?");
        // Every name needs a value; keys the query does not use are ignored.
        let mut m = Map::new();
        m.insert("a".into(), Value::Int(1));
        m.insert("z".into(), Value::Int(2));
        assert!(translate("select :a", &[Value::map(m.clone())]).is_ok());
        assert_eq!(translate("select :a + :b", &[Value::map(m)]).err().unwrap(), "no value for :b");
    }
}
