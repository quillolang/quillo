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

fn opts_of(url: &str) -> Result<Opts, String> {
    // `mariadb://` is the same protocol.
    let url = match url.strip_prefix("mariadb://") {
        Some(rest) => format!("mysql://{}", rest),
        None => url.to_string(),
    };
    Opts::from_url(&url).map_err(|e| e.to_string())
}

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    crate::pg::runtime().block_on(f)
}

/// The database's connections.
pub struct Pool {
    pool: Option<mysql_async::Pool>,
}

pub struct Conn(Option<mysql_async::Conn>);

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
        drop(Conn(Some(c)));
        Ok(Pool { pool: Some(pool) })
    }

    pub fn checkout(&self) -> Result<Conn, String> {
        let pool = self.pool.as_ref().expect("open");
        run(pool.get_conn()).map(|c| Conn(Some(c))).map_err(|e| e.to_string())
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
        run(async move {
            let mut result = c.exec_iter(sql, params).await.map_err(|e| e.to_string())?;
            let cols = result.columns().unwrap_or_else(|| Vec::new().into());
            let mut out = Vec::new();
            while let Some(row) = result.next().await.map_err(|e| e.to_string())? {
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
            result.drop_result().await.map_err(|e| e.to_string())?;
            Ok(out)
        })
    }

    /// `{ changes, last_id }`. Without parameters the text may hold several
    /// statements.
    pub fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        let c = self.conn();
        if params.is_empty() {
            run(c.query_drop(sql)).map_err(|e| e.to_string())?;
        } else {
            let (sql, params) = translate(sql, params)?;
            run(c.exec_drop(sql, params)).map_err(|e| e.to_string())?;
        }
        let mut m = Map::new();
        m.insert("changes".into(), Value::Int(c.affected_rows() as i64));
        m.insert("last_id".into(), Value::Int(c.last_insert_id().unwrap_or(0) as i64));
        Ok(Value::map(m))
    }

    pub fn batch(&mut self, sql: &str) -> Result<(), String> {
        let c = self.conn();
        run(c.query_drop(sql)).map_err(|e| e.to_string())
    }
}

fn to_my(v: &Value) -> Result<My, String> {
    Ok(match v {
        Value::Nil => My::NULL,
        Value::Bool(b) => My::Int(*b as i64),
        Value::Int(n) => My::Int(*n),
        Value::Float(f) => My::Double(*f),
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
                ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => match (text.parse::<i64>(), text.parse::<f64>()) {
                    (Ok(n), _) => Value::Int(n),
                    (_, Ok(f)) if f.fract() == 0.0 && f.abs() < 9.0e15 => Value::Int(f as i64),
                    (_, Ok(f)) => Value::Float(f),
                    _ => Value::str(text),
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
                values.push(to_my(m.get(name).unwrap_or(&Value::Nil))?);
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
    fn placeholders() {
        let mut m = Map::new();
        m.insert("id".into(), Value::Int(1));
        m.insert("name".into(), Value::str("x"));
        let (sql, p) = translate("select :id, ':no', `:no`, :name -- :no\n, :id", &[Value::map(m)]).unwrap();
        assert_eq!(sql, "select ?, ':no', `:no`, ? -- :no\n, ?");
        assert!(matches!(p, Params::Positional(v) if v.len() == 3));
        let (sql, _) = translate("select ? + ?", &[Value::Int(1), Value::Int(2)]).unwrap();
        assert_eq!(sql, "select ? + ?");
    }
}
