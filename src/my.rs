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
    let url = if rest.is_empty() { base.to_string() } else { format!("{}?{}", base, rest.join("&")) };
    let opts = Opts::from_url(&url).map_err(|e| e.to_string())?;
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
}

/// A connection; and, in a transaction that retries, whether a statement
/// failed because of another transaction (see `crate::txn`; None: not
/// watching).
pub struct Conn(Option<mysql_async::Conn>, Option<bool>);

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
        let c = run(pool.get_conn()).map_err(|e| e.to_string())?;
        drop(Conn(Some(c), None));
        Ok(Pool { pool: Some(pool) })
    }

    pub fn checkout(&self) -> Result<Conn, String> {
        let pool = self.pool.as_ref().expect("open");
        run(pool.get_conn()).map(|c| Conn(Some(c), None)).map_err(|e| e.to_string())
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
        let (sql, params) = translate(sql, params)?;
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
        r.map_err(|e| self.failed(e))
    }

    /// `{ changes, last_id }`. Without parameters the text may hold several
    /// statements.
    pub fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        let r = if params.is_empty() {
            run(self.conn().query_drop(sql))
        } else {
            let (sql, params) = translate(sql, params)?;
            run(self.conn().exec_drop(sql, params))
        };
        r.map_err(|e| self.failed(e))?;
        let c = self.conn();
        let mut m = Map::new();
        m.insert("changes".into(), Value::Int(c.affected_rows() as i64));
        m.insert("last_id".into(), Value::Int(c.last_insert_id().unwrap_or(0) as i64));
        Ok(Value::map(m))
    }

    pub fn batch(&mut self, sql: &str) -> Result<(), String> {
        let r = run(self.conn().query_drop(sql));
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
        Value::Float(f) => My::Double(*f),
        // As text, which MySQL converts to a DECIMAL exactly.
        Value::Dec(d) => My::Bytes(d.to_string().into_bytes()),
        Value::Str(s) => My::Bytes(s.as_bytes().to_vec()),
        Value::Html(h) => My::Bytes(h.as_bytes().to_vec()),
        Value::List(_) | Value::Map(_) => My::Bytes(v.json_text().into_bytes()),
        other => return Err(format!("a {} cannot be stored in the database", other.type_name())),
    })
}

fn from_my(v: My, col: &mysql_async::Column) -> Value {
    let ty = col.column_type();
    match v {
        My::NULL => Value::Nil,
        My::Int(n) if ty == ColumnType::MYSQL_TYPE_TINY && col.column_length() == 1 => Value::Bool(n != 0),
        My::Int(n) => Value::Int(n),
        My::UInt(n) => i64::try_from(n).map(Value::Int).unwrap_or(Value::Float(n as f64)),
        My::Float(f) => Value::Float(f as f64),
        My::Double(f) => Value::Float(f),
        My::Bytes(b) => {
            let text = String::from_utf8_lossy(&b);
            match ty {
                // An int without a scale (as `sum` of ints gives), else a
                // decimal with its scale, else (too many digits) the text.
                ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => match text.parse::<i64>() {
                    Ok(n) => Value::Int(n),
                    Err(_) => crate::decimal::Dec::parse(&text).map_or_else(|| Value::str(&*text), Value::Dec),
                },
                ColumnType::MYSQL_TYPE_JSON => {
                    serde_json::from_str::<serde_json::Value>(&text).map(|j| Value::from_json(&j)).unwrap_or_else(|_| Value::str(text))
                }
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

/// `:name` placeholders (with one map of values) to MySQL's `?`, skipping
/// quotes, backticks and comments; `?` as they are.
fn translate(sql: &str, params: &[Value]) -> Result<(String, Params), String> {
    let named = match params {
        [Value::Map(m)] => Some(m.clone()),
        _ => None,
    };
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut values = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\'' | b'"' | b'`' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != c {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.push_str(&sql[start..i]);
                continue;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                let end = sql[i..].find('\n').map_or(b.len(), |n| i + n);
                out.push_str(&sql[i..end]);
                i = end;
                continue;
            }
            b'#' => {
                let end = sql[i..].find('\n').map_or(b.len(), |n| i + n);
                out.push_str(&sql[i..end]);
                i = end;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = sql[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2);
                out.push_str(&sql[i..end]);
                i = end;
                continue;
            }
            b':' if named.is_some() && b.get(i + 1).map_or(false, |n| n.is_ascii_alphabetic() || *n == b'_') && (i == 0 || b[i - 1] != b':') => {
                let start = i + 1;
                let mut end = start;
                while end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'_') {
                    end += 1;
                }
                let name = &sql[start..end];
                let m = named.as_ref().unwrap();
                values.push(to_my(m.get(name).ok_or_else(|| format!("no value for :{}", name))?)?);
                out.push('?');
                i = end;
                continue;
            }
            _ => {}
        }
        let ch = sql[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    if named.is_none() {
        values = params.iter().map(to_my).collect::<Result<_, _>>()?;
    }
    Ok((out, if values.is_empty() { Params::Empty } else { Params::Positional(values) }))
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

    #[test]
    fn placeholders() {
        let mut m = Map::new();
        m.insert("id".into(), Value::Int(1));
        m.insert("name".into(), Value::str("x"));
        let (sql, p) = translate("select :id, ':no', `:no`, :name -- :no\n, :id", &[Value::map(m)]).unwrap();
        assert_eq!(sql, "select ?, ':no', `:no`, ? -- :no\n, ?");
        assert!(matches!(p, Params::Positional(v) if v.len() == 3));
        let (sql, _) = translate("select ? + ?", &[Value::Int(1), Value::Int(2)]).unwrap();
        assert_eq!(sql, "select ? + ?");
        // Every name needs a value; keys the query does not use are ignored.
        let mut m = Map::new();
        m.insert("a".into(), Value::Int(1));
        m.insert("z".into(), Value::Int(2));
        assert!(translate("select :a", &[Value::map(m.clone())]).is_ok());
        assert_eq!(translate("select :a + :b", &[Value::map(m)]).err().unwrap(), "no value for :b");
    }
}
