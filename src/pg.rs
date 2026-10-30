//! PostgreSQL for `db`: a connection pool, placeholder translation, and
//! conversion between Quillo values and Postgres types.
//!
//! Queries are prepared, so parameters never become SQL text. Quillo's `?`
//! and `:name` placeholders are rewritten to `$1, $2, ...` (a query that
//! already uses `$1` is left alone). Parameters are encoded for the type
//! Postgres expects; for types without a direct encoding (numeric, dates,
//! uuid, arrays, ...) the parameter is sent as text and cast by Postgres.
//! Results come back as Quillo values: numbers, strings, booleans, JSON as
//! maps and lists, arrays as lists, `numeric` as decimals, and dates, times
//! and UUIDs as strings.

use crate::value::*;
use bytes::{BufMut, BytesMut};
use postgres::types::{FromSql, IsNull, Kind, ToSql, Type};
use std::collections::HashMap;
use std::error::Error;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

type BoxErr = Box<dyn Error + Sync + Send>;

/// Default and largest pool size; `?pool_size=N` in the URL changes it.
const DEFAULT_POOL: usize = 8;

/// How long a transaction waits for a connection of its own.
pub const CHECKOUT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

pub fn is_url(s: &str) -> bool {
    s.starts_with("postgres://") || s.starts_with("postgresql://")
}

// ---- connections ---------------------------------------------------------

/// One connection and the statements prepared on it.
pub struct Conn {
    client: postgres::Client,
    stmts: HashMap<String, postgres::Statement>,
    /// In a transaction that retries: whether a statement failed because
    /// of another transaction (see `crate::txn`). None: not watching.
    conflict: Option<bool>,
    /// Opened past the pool's size by `checkout_nested`: closed when given
    /// back, not kept.
    aside: bool,
}

pub struct Pool {
    config: postgres::Config,
    /// The same, for the connections requests share (`Lanes`).
    async_config: tokio_postgres::Config,
    tls: tokio_postgres_rustls::MakeRustlsConnect,
    idle: Mutex<(Vec<Conn>, usize)>,
    freed: Condvar,
    max: usize,
    lanes: OnceLock<Arc<Lanes>>,
}

impl Pool {
    /// Parses the URL and opens one connection to check it. TLS is required,
    /// and the server's certificate and name checked, for a server on
    /// another machine; it is used on this one when the server offers it.
    /// The URL can say otherwise as libpq's do (`sslmode`, `sslrootcert`,
    /// `sslcert`, `sslkey`); see `Tls`.
    pub fn open(url: &str) -> Result<Pool, String> {
        Pool::open_as(url, true)
    }

    /// `open`; with `make` false, the schema (`?schema=`) must exist.
    pub fn open_as(url: &str, make: bool) -> Result<Pool, String> {
        let Options { url, max, schema, tls } = take_options(url)?;
        let mut config: tokio_postgres::Config = url.parse().map_err(|e| format!("bad Postgres URL: {}", e))?;
        let tls = tls.settle(&mut config)?;
        if let Some(s) = &schema {
            config.options(&search_path(s));
        }
        let pool = Pool {
            config: config.clone().into(),
            async_config: config,
            tls,
            idle: Mutex::new((Vec::new(), 0)),
            freed: Condvar::new(),
            max,
            lanes: OnceLock::new(),
        };
        let mut first = pool.checkout()?;
        // A tenant's schema is made when the program first uses it.
        if let Some(s) = &schema {
            let r = if make {
                first.batch(&format!("create schema if not exists \"{}\"", s))
            } else {
                match first.query("select 1 from pg_namespace where nspname = $1", &[Value::str(s)], Some(1)) {
                    Ok(rows) if rows.is_empty() => Err(format!("there is no schema {:?}", s)),
                    other => other.map(|_| ()),
                }
            };
            if let Err(e) = r {
                pool.give_back(first);
                return Err(e);
            }
        }
        pool.give_back(first);
        Ok(pool)
    }

    fn connect(&self) -> Result<Conn, String> {
        let client = self.config.connect(self.tls.clone()).map_err(|e| describe(&e))?;
        Ok(Conn { client, stmts: HashMap::new(), conflict: None, aside: false })
    }

    /// An idle connection or a new one, or None when all `max` are taken.
    pub fn try_checkout(&self) -> Result<Option<Conn>, String> {
        let mut g = self.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(c) = g.0.pop() {
            return Ok(Some(c));
        }
        if g.1 >= self.max {
            return Ok(None);
        }
        g.1 += 1;
        drop(g);
        self.connect().map(Some).inspect_err(|_| {
            self.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner).1 -= 1;
            self.freed.notify_one();
        })
    }

    /// A connection: an idle one, a new one, or the next one returned
    /// within `CHECKOUT_WAIT`, waiting on this thread. A request in the
    /// server waits without a thread instead (`db::checkout_for_tx`).
    pub fn checkout(&self) -> Result<Conn, String> {
        let until = Instant::now() + CHECKOUT_WAIT;
        loop {
            if let Some(c) = self.try_checkout()? {
                return Ok(c);
            }
            if Instant::now() >= until {
                return Err(self.none_free());
            }
            // In slices: one given back between the try and the wait is not
            // missed for long.
            let g = self.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = self.freed.wait_timeout(g, std::time::Duration::from_millis(50));
        }
    }

    /// Why no connection could be had.
    pub fn none_free(&self) -> String {
        format!(
            "no PostgreSQL connection free after {} s: all {} are in use (`?pool_size=N` in the URL allows more)",
            CHECKOUT_WAIT.as_secs(),
            self.max
        )
    }

    /// The connections requests in the server share (see `Lanes`).
    pub fn lanes(&self) -> Arc<Lanes> {
        self.lanes
            .get_or_init(|| {
                Arc::new(Lanes {
                    config: self.async_config.clone(),
                    tls: self.tls.clone(),
                    slots: (0..self.max).map(|_| Mutex::new(None)).collect(),
                })
            })
            .clone()
    }

    /// A connection for work a request does while it may already hold one
    /// (a transaction's): Quillo's own tables, made or written apart. As
    /// `checkout`, but when every connection is taken it opens one more
    /// rather than wait, since the one it would wait for may be the
    /// caller's own.
    pub fn checkout_nested(&self) -> Result<Conn, String> {
        match self.try_checkout()? {
            Some(c) => Ok(c),
            None => self.connect().map(|c| Conn { aside: true, ..c }),
        }
    }

    pub fn give_back(&self, c: Conn) {
        if c.aside {
            return;
        }
        let mut g = self.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if c.client.is_closed() {
            g.1 -= 1;
        } else {
            g.0.push(c);
        }
        drop(g);
        self.freed.notify_one();
    }
}

/// Whether a database host is this machine: the password and the data then
/// cross no network that someone else could be on.
pub(crate) fn local_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.is_empty()
        || h.starts_with('/')
        || h.eq_ignore_ascii_case("localhost")
        || h.to_ascii_lowercase().ends_with(".localhost")
        || h.parse::<std::net::IpAddr>().map_or(false, |a| a.is_loopback())
}

/// The connection option that makes `schema` where tables are looked for
/// and made.
fn search_path(schema: &str) -> String {
    format!("-c search_path=\"{}\"", schema)
}

/// A URL's options that are Quillo's, or that the driver does not read.
struct Options {
    url: String,
    max: usize,
    schema: Option<String>,
    tls: Tls,
}

/// Takes Quillo's own options out of the URL: `pool_size`, and `schema`
/// (`?schema={tenant}`: a schema per tenant in one database); and those
/// about TLS (see `Tls`), which the driver reads only in part.
fn take_options(url: &str) -> Result<Options, String> {
    let mut o = Options { url: url.to_string(), max: DEFAULT_POOL, schema: None, tls: Tls::default() };
    let Some((base, query)) = url.split_once('?') else { return Ok(o) };
    let mut rest = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let value = || crate::server::percent_decode(pair.split_once('=').map_or("", |(_, v)| v), false);
        match pair.split_once('=') {
            Some(("pool_size", v)) => {
                o.max = v.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| format!("bad pool_size: {}", v))?;
            }
            Some(("schema", v)) => {
                if !crate::site::tenant_name_ok(v) {
                    return Err(format!("bad schema: {:?} (lowercase letters, digits, - and _)", v));
                }
                // Postgres's own: a tenant named `public` must not get the
                // schema everyone shares (a prefix, `schema=t_{tenant}`, avoids it).
                if v == "public" || v == "information_schema" || v.starts_with("pg_") {
                    return Err(format!("the schema {:?} is Postgres's own; prefix tenants' (schema=t_{{tenant}})", v));
                }
                o.schema = Some(v.to_string());
            }
            Some(("sslmode", v)) => {
                o.tls.mode = Some(match v {
                    "disable" => SslMode::Disable,
                    "prefer" => SslMode::Prefer,
                    "require" => SslMode::Require,
                    "verify-ca" => SslMode::VerifyCa,
                    "verify-full" => SslMode::VerifyFull,
                    _ => return Err(format!("bad sslmode: {:?} (disable, prefer, require, verify-ca or verify-full)", v)),
                })
            }
            Some(("sslrootcert", _)) => o.tls.root = Some(value()),
            Some(("sslcert", _)) => o.tls.cert = Some(value()),
            Some(("sslkey", _)) => o.tls.key = Some(value()),
            _ => rest.push(pair),
        }
    }
    o.url = if rest.is_empty() { base.to_string() } else { format!("{}?{}", base, rest.join("&")) };
    Ok(o)
}

// ---- TLS -----------------------------------------------------------------

/// libpq's `sslmode`, less `allow` (plain first, which leaves the
/// password to whoever answers).
#[derive(Debug, Clone, Copy, PartialEq)]
enum SslMode {
    /// No TLS.
    Disable,
    /// TLS if the server offers it. Someone in the middle can say it does
    /// not, so this is for servers on this machine.
    Prefer,
    /// TLS always, the certificate unchecked unless `sslrootcert` is given
    /// (then as `VerifyCa`). Nobody can read the connection, but someone in
    /// the middle can pose as the server.
    Require,
    /// TLS, with a certificate signed by a CA in `sslrootcert`, for any name.
    VerifyCa,
    /// TLS, with a certificate signed by a trusted CA, for the host's name.
    VerifyFull,
}

/// What a URL says about TLS, as libpq reads it: `sslmode`; `sslrootcert`,
/// a file of PEM certificates of the CAs to trust instead of the usual
/// public ones (a cloud's own, or a company's), or `system` for the
/// operating system's; and `sslcert` and `sslkey`, a certificate and its
/// key for servers that ask for one.
#[derive(Debug, Default, Clone, PartialEq)]
struct Tls {
    mode: Option<SslMode>,
    root: Option<String>,
    cert: Option<String>,
    key: Option<String>,
}

/// How a server's certificate is checked.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Check {
    Nothing,
    Chain,
    ChainAndName,
}

impl Tls {
    /// The mode for these hosts. Postgres's own default, `prefer`, sends the
    /// password in the clear to anyone in the middle who says the server has
    /// no TLS; so for a server on another machine, TLS is required and its
    /// certificate checked (`verify-full`) unless the URL says otherwise.
    fn mode_for(&self, config: &tokio_postgres::Config) -> SslMode {
        use tokio_postgres::config::Host;
        let remote = config.get_hosts().iter().any(|h| match h {
            Host::Tcp(name) => !local_host(name),
            #[allow(unreachable_patterns)]
            _ => false,
        }) || config.get_hostaddrs().iter().any(|a| !a.is_loopback());
        match self.mode {
            Some(m) => m,
            None if remote || self.root.as_deref() == Some("system") => SslMode::VerifyFull,
            None => SslMode::Prefer,
        }
    }

    /// What the certificate is checked for in `mode`. As libpq does, a root
    /// certificate given makes `prefer` and `require` check the chain.
    fn check(&self, mode: SslMode) -> Check {
        match mode {
            SslMode::VerifyFull => Check::ChainAndName,
            SslMode::VerifyCa => Check::Chain,
            SslMode::Prefer | SslMode::Require if self.root.is_some() => Check::Chain,
            _ => Check::Nothing,
        }
    }

    /// Sets the driver's mode in `config`, and makes the TLS connector.
    fn settle(&self, config: &mut tokio_postgres::Config) -> Result<tokio_postgres_rustls::MakeRustlsConnect, String> {
        use tokio_postgres::config::SslMode as Driver;
        let mode = self.mode_for(config);
        config.ssl_mode(match mode {
            SslMode::Disable => Driver::Disable,
            SslMode::Prefer => Driver::Prefer,
            _ => Driver::Require,
        });
        self.client_config(mode).map(tokio_postgres_rustls::MakeRustlsConnect::new)
    }

    fn client_config(&self, mode: SslMode) -> Result<rustls::ClientConfig, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(self.verifier(mode, provider)?);
        match (&self.cert, &self.key) {
            (None, None) => Ok(builder.with_no_client_auth()),
            (Some(cert), Some(key)) => {
                use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
                let chain = CertificateDer::pem_file_iter(cert)
                    .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
                    .map_err(|e| format!("sslcert {}: {}", cert, e))?;
                let key = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("sslkey {}: {}", key, e))?;
                builder.with_client_auth_cert(chain, key).map_err(|e| format!("sslcert and sslkey: {}", e))
            }
            _ => Err("sslcert and sslkey go together: a certificate, and its key".into()),
        }
    }

    /// What checks the server's certificate in `mode`.
    fn verifier(&self, mode: SslMode, provider: Arc<rustls::crypto::CryptoProvider>) -> Result<Arc<dyn rustls::client::danger::ServerCertVerifier>, String> {
        let check = self.check(mode);
        // libpq's rules, which keep a weak check from passing for a strong one.
        if self.root.as_deref() == Some("system") && mode != SslMode::VerifyFull {
            return Err(format!("sslrootcert=system needs sslmode=verify-full, not {:?}", mode));
        }
        if check == Check::Chain && self.root.is_none() {
            return Err("sslmode=verify-ca needs sslrootcert, the CA certificates to check the server's against".into());
        }
        let algs = provider.signature_verification_algorithms;
        Ok(match check {
            Check::ChainAndName => rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(self.roots()?), provider)
                .build()
                .map_err(|e| e.to_string())?,
            Check::Chain => Arc::new(Verifier { roots: Some(Arc::new(self.roots()?)), algs }),
            Check::Nothing => Arc::new(Verifier { roots: None, algs }),
        })
    }

    /// The CAs whose certificates are trusted: those in `sslrootcert`, the
    /// system's, or the usual public ones.
    fn roots(&self) -> Result<rustls::RootCertStore, String> {
        use rustls::pki_types::{pem::PemObject, CertificateDer};
        let mut roots = rustls::RootCertStore::empty();
        match self.root.as_deref() {
            None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
            Some("system") => {
                let found = rustls_native_certs::load_native_certs();
                roots.add_parsable_certificates(found.certs);
                if roots.is_empty() {
                    return Err(format!("sslrootcert=system: no CA certificates found on this system{}", found.errors.first().map(|e| format!(" ({})", e)).unwrap_or_default()));
                }
            }
            Some(path) => {
                let certs = CertificateDer::pem_file_iter(path)
                    .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
                    .map_err(|e| format!("sslrootcert {}: {}", path, e))?;
                for c in certs {
                    roots.add(c).map_err(|e| format!("sslrootcert {}: {}", path, e))?;
                }
                if roots.is_empty() {
                    return Err(format!("sslrootcert {}: no PEM certificates in it", path));
                }
            }
        }
        Ok(roots)
    }
}

/// Checks a server's certificate for `verify-ca` (signed by one of `roots`,
/// whatever name it is for), or not at all (`require` without a root
/// certificate). Either way the server proves in the handshake that it
/// holds the certificate's key.
#[derive(Debug)]
struct Verifier {
    roots: Option<Arc<rustls::RootCertStore>>,
    algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl rustls::client::danger::ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if let Some(roots) = &self.roots {
            let cert = rustls::server::ParsedCertificate::try_from(end_entity)?;
            rustls::client::verify_server_cert_signed_by_trust_anchor(&cert, roots, intermediates, now, self.algs.all)?;
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.algs.supported_schemes()
    }
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
            if s.contains("UnknownIssuer") {
                s.push_str(" (the server's certificate is signed by a CA not among those trusted: give its CA's certificates with sslrootcert=/path/to/ca.pem)");
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
        match casts_for(sql, stmt.params()) {
            None => Ok(stmt),
            Some(cast_sql) => self.prepare(&cast_sql),
        }
    }

    pub fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        let (sql, params) = translate(sql, params)?;
        let stmt = self.statement(&sql, &params)?;
        let args: Vec<PgParam> = params.into_iter().map(PgParam).collect();
        let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
        let rows = self.client.query(&stmt, &refs);
        let rows = rows.map_err(|e| self.failed(&e))?;
        values(&rows, limit)
    }

    /// `{ changes, last_id }`; Postgres has no last insert id, so use
    /// `insert ... returning id` with `db.one` for that.
    pub fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        let changes = if params.is_empty() && has_several_statements(sql) {
            let r = self.client.batch_execute(sql);
            r.map_err(|e| self.failed(&e))?;
            Value::Nil
        } else {
            let (sql, params) = translate(sql, params)?;
            let stmt = self.statement(&sql, &params)?;
            let args: Vec<PgParam> = params.into_iter().map(PgParam).collect();
            let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
            let r = self.client.execute(&stmt, &refs);
            Value::Int(r.map_err(|e| self.failed(&e))? as i64)
        };
        let mut m = Map::new();
        m.insert("changes".into(), changes);
        m.insert("last_id".into(), Value::Nil);
        Ok(Value::map(m))
    }

    pub fn batch(&mut self, sql: &str) -> Result<(), String> {
        let r = self.client.batch_execute(sql);
        r.map_err(|e| self.failed(&e))
    }

    /// The error's message; a serialization failure or a deadlock (SQLSTATE
    /// 40001, 40P01) is also noted, when watching, for the transaction to
    /// run again.
    fn failed(&mut self, e: &postgres::Error) -> String {
        use postgres::error::SqlState;
        if let (Some(c), Some(code)) = (self.conflict.as_mut(), e.code()) {
            if *code == SqlState::T_R_SERIALIZATION_FAILURE || *code == SqlState::T_R_DEADLOCK_DETECTED {
                *c = true;
            }
        }
        describe(e)
    }

    /// Starts (or stops) noting conflicts with other transactions.
    pub fn watch_conflicts(&mut self, on: bool) {
        self.conflict = on.then_some(false);
    }

    /// Whether a statement failed because of another transaction since
    /// `watch_conflicts(true)`.
    pub fn conflicted(&self) -> bool {
        self.conflict == Some(true)
    }
}

/// Rows as maps of column name to value.
fn values(rows: &[postgres::Row], limit: Option<usize>) -> Result<Vec<Value>, String> {
    let mut out = Vec::with_capacity(rows.len().min(limit.unwrap_or(usize::MAX)));
    for row in rows.iter().take(limit.unwrap_or(usize::MAX)) {
        let mut m = Map::with_capacity(row.len());
        for (i, col) in row.columns().iter().enumerate() {
            let v: PgValue = row.try_get(i).map_err(|e| describe(&e))?;
            m.insert(col.name().into(), v.0);
        }
        out.push(Value::map(m));
    }
    Ok(out)
}

/// The cast-from-text rewrite of `sql` that `statement` needs for these
/// parameter types, if any.
fn casts_for(sql: &str, types: &[Type]) -> Option<String> {
    let casts: Vec<(usize, String)> =
        types.iter().enumerate().filter(|(_, t)| !encodes_directly(t)).map(|(i, t)| (i + 1, type_name(t))).collect();
    if casts.is_empty() {
        return None;
    }
    Some(rewrite_numbered(sql, |n| casts.iter().find(|(i, _)| *i == n).map(|(_, t)| format!("(${}::text)::{}", n, t))))
}

// ---- connections shared by the server's requests --------------------------

/// Threads for the shared connections' runtime (more with `--instances`).
pub static RUNTIME_THREADS: AtomicUsize = AtomicUsize::new(1);

/// The runtime the shared connections run on.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(RUNTIME_THREADS.load(Relaxed).max(1))
            .thread_name("quillo-pg")
            .on_thread_start(crate::alloc::waiter_thread)
            .enable_all()
            .build()
            .expect("a runtime for Postgres")
    })
}

/// Connections many requests use at once, in the server. What is sent to
/// one connection is pipelined (the next query goes out before the answer
/// to the last one comes back), so a few connections keep the database busy
/// without a thread waiting on each query. A request's queries (its
/// `parallel` calls' too) go to the same connection, and requests share a
/// few: the database then reads many queries at a time rather than waking a
/// process for each, which takes it less than half the work. While one of
/// those few is stuck behind a slow query, new requests use others, up to
/// the pool's size. Transactions keep a connection of their own instead
/// (`Pool::checkout`).
pub struct Lanes {
    config: tokio_postgres::Config,
    tls: tokio_postgres_rustls::MakeRustlsConnect,
    slots: Vec<Mutex<Option<Arc<Lane>>>>,
}

/// How many connections requests share while none is stuck.
const USUAL_LANES: usize = 4;
/// How long a connection may wait for an answer before it counts as stuck.
const STUCK_MS: u64 = 20;

struct Lane {
    client: tokio_postgres::Client,
    stmts: Mutex<HashMap<String, tokio_postgres::Statement>>,
    /// Queries sent and not yet answered.
    waiting: AtomicUsize,
    /// When it last answered, or was given work while it had none (`clock`).
    moved: AtomicU64,
}

/// Milliseconds since the first call.
fn clock() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

impl Lane {
    fn stuck(&self, now: u64) -> bool {
        self.waiting.load(Relaxed) > 0 && now.saturating_sub(self.moved.load(Relaxed)) > STUCK_MS
    }

    /// Counts a query as waiting until the guard goes.
    fn send(self: &Arc<Self>) -> Sent {
        if self.waiting.fetch_add(1, Relaxed) == 0 {
            self.moved.store(clock(), Relaxed);
        }
        Sent(self.clone())
    }
}

struct Sent(Arc<Lane>);

impl Drop for Sent {
    fn drop(&mut self) {
        self.0.moved.store(clock(), Relaxed);
        self.0.waiting.fetch_sub(1, Relaxed);
    }
}

impl Lanes {
    /// The connection for request `n`: the same each time among the usual
    /// few, unless it is stuck; then the next that is not, the others in the
    /// pool included. Connects it (again) if needed.
    async fn lane(&self, n: usize) -> Result<Arc<Lane>, String> {
        let usual = self.slots.len().min(USUAL_LANES);
        let first = n % usual;
        let now = clock();
        let order = (0..usual).map(|k| (first + k) % usual).chain(usual..self.slots.len());
        let mut pick = first;
        for i in order {
            let l = self.slots[i].lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            match l {
                Some(l) if l.client.is_closed() => {
                    pick = i;
                    break;
                }
                Some(l) if !l.stuck(now) => return Ok(l),
                Some(_) => {}
                None => {
                    pick = i;
                    break;
                }
            }
        }
        if let Some(l) = self.slots[pick].lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
            if !l.client.is_closed() {
                return Ok(l);
            }
        }
        let (client, conn) = self.config.connect(self.tls.clone()).await.map_err(|e| describe(&e))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let l = Arc::new(Lane { client, stmts: Mutex::new(HashMap::new()), waiting: AtomicUsize::new(0), moved: AtomicU64::new(0) });
        *self.slots[pick].lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(l.clone());
        Ok(l)
    }

    async fn prepare(lane: &Lane, sql: &str) -> Result<tokio_postgres::Statement, String> {
        if let Some(s) = lane.stmts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(sql) {
            return Ok(s.clone());
        }
        let s = lane.client.prepare(sql).await.map_err(|e| describe(&e))?;
        let mut g = lane.stmts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.len() >= 256 {
            g.clear();
        }
        g.insert(sql.to_string(), s.clone());
        Ok(s)
    }

    async fn statement(lane: &Lane, sql: &str, params: &[Value]) -> Result<tokio_postgres::Statement, String> {
        let stmt = Self::prepare(lane, sql).await?;
        let want = stmt.params().len();
        if want != params.len() {
            return Err(format!("the query has {} parameter{}, got {}", want, if want == 1 { "" } else { "s" }, params.len()));
        }
        match casts_for(sql, stmt.params()) {
            None => Ok(stmt),
            Some(cast_sql) => Self::prepare(lane, &cast_sql).await,
        }
    }

    /// Runs a query for request `n` (see `lane`).
    pub async fn query(self: Arc<Self>, n: usize, sql: String, params: Vec<Value>, limit: Option<usize>) -> Result<Vec<Value>, String> {
        let (sql, params) = translate(&sql, &params)?;
        let lane = self.lane(n).await?;
        let _sent = lane.send();
        let stmt = Self::statement(&lane, &sql, &params).await?;
        let args: Vec<PgParam> = params.into_iter().map(PgParam).collect();
        let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
        let rows = lane.client.query(&stmt, &refs).await.map_err(|e| describe(&e))?;
        values(&rows, limit)
    }

    pub async fn exec(self: Arc<Self>, n: usize, sql: String, params: Vec<Value>) -> Result<Value, String> {
        let lane = self.lane(n).await?;
        let _sent = lane.send();
        let changes = if params.is_empty() && has_several_statements(&sql) {
            lane.client.batch_execute(&sql).await.map_err(|e| describe(&e))?;
            Value::Nil
        } else {
            let (sql, params) = translate(&sql, &params)?;
            let stmt = Self::statement(&lane, &sql, &params).await?;
            let args: Vec<PgParam> = params.into_iter().map(PgParam).collect();
            let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
            Value::Int(lane.client.execute(&stmt, &refs).await.map_err(|e| describe(&e))? as i64)
        };
        let mut m = Map::new();
        m.insert("changes".into(), changes);
        m.insert("last_id".into(), Value::Nil);
        Ok(Value::map(m))
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
            let mut values = Vec::new();
            for n in &names {
                let v = m.get(n.as_str()).or_else(|| m.get(format!(":{}", n).as_str())).cloned();
                values.push(v.ok_or_else(|| format!("no value for :{}", n))?);
            }
            return Ok((out, values));
        }
    }
    Ok((positional(sql), params.to_vec()))
}

/// `?` placeholders as `$1, $2, ...`, unless the query already numbers its
/// own. Depends on the SQL alone, so it is worked out once per statement.
fn positional(sql: &str) -> String {
    static SEEN: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    if let Some(done) = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m.get(sql)) {
        return done.clone();
    }
    let out = if has_numbered(sql) {
        sql.to_string()
    } else {
        let mut n = 0;
        scan(sql, |c, i, out| {
            if c[*i] == '?' && !is_question_operator(c, *i) {
                n += 1;
                out.push_str(&format!("${}", n));
            } else {
                out.push(c[*i]);
            }
            *i += 1;
        })
    };
    let mut g = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let m = g.get_or_insert_with(HashMap::new);
    // Statements are usually written in the program, so few; if they are
    // built from values instead, the cache starts over now and then.
    if m.len() >= 1024 {
        m.clear();
    }
    m.insert(sql.to_string(), out.clone());
    out
}

/// Words after which a `?` is a value, not jsonb's `?` operator.
const BEFORE_VALUE: &[&str] = &[
    "select", "where", "and", "or", "not", "limit", "offset", "by", "on", "having", "when", "then", "else", "case",
    "in", "is", "like", "ilike", "similar", "to", "between", "symmetric", "asymmetric", "values", "set", "returning",
    "distinct", "all", "any", "some", "escape", "exists", "return", "first", "next", "using", "from", "for", "zone",
    "overlaps", "placing", "both", "leading", "trailing",
];

/// Whether the `?` at `i` is one of jsonb's operators (`data ? 'key'`,
/// `?|`, `?&`) rather than a parameter. A parameter comes after an
/// operator, a bracket, a comma or a keyword such as `limit`; the operator
/// comes after a value: a column, `)`, `]` or a quoted string. A query that
/// numbers its own parameters (`$1`) is never rewritten, so it can use `?`
/// freely.
fn is_question_operator(c: &[char], i: usize) -> bool {
    if matches!(c.get(i + 1), Some('|') | Some('&')) {
        return true;
    }
    let mut j = i;
    while j > 0 && c[j - 1].is_whitespace() {
        j -= 1;
    }
    let Some(&before) = j.checked_sub(1).and_then(|k| c.get(k)) else { return false };
    if matches!(before, ')' | ']' | '\'' | '"') {
        return true;
    }
    if !(before.is_alphanumeric() || before == '_') {
        return false;
    }
    let mut k = j;
    while k > 0 && (c[k - 1].is_alphanumeric() || c[k - 1] == '_') {
        k -= 1;
    }
    let word = c[k..j].iter().collect::<String>().to_ascii_lowercase();
    !BEFORE_VALUE.contains(&word.as_str())
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
                Value::Dec(d) if d.round(0, crate::decimal::Rounding::Down).ok() == Some(*d) => d.trunc_i64().ok_or_else(|| bad(v, ty)),
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
                    Value::Dec(d) => d.to_f64(),
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

/// Postgres's binary NUMERIC: an int when it has no scale (as `sum` of
/// ints gives) and fits; else a decimal, its scale kept ("12.50"); else,
/// past 28 digits after the point or 29 in all, its exact text. NaN and
/// the infinities are floats.
fn numeric(raw: &[u8]) -> Result<Value, BoxErr> {
    let mut at = 0;
    let ndigits = usize::try_from(i16::from_be_bytes(take(raw, &mut at)?)).map_err(|_| "bad numeric")?;
    let weight = i16::from_be_bytes(take(raw, &mut at)?) as i64;
    let sign = u16::from_be_bytes(take(raw, &mut at)?);
    let dscale = usize::try_from(i16::from_be_bytes(take(raw, &mut at)?)).map_err(|_| "bad numeric")?;
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
    if dscale == 0 {
        if let Ok(n) = format!("{}{}", if neg { "-" } else { "" }, int_part).parse::<i64>() {
            return Ok(Value::Int(n));
        }
    }
    Ok(crate::decimal::Dec::parse(&text).map_or_else(|| Value::str(text), Value::Dec))
}

/// Most dimensions a Postgres array has (its MAXDIM).
const MAX_DIMS: usize = 6;

/// A one- or more-dimensional array as (nested) lists. The sizes it states
/// are checked against what came: a server (or someone in the middle) that
/// says -1 or 2^31 cannot make this take all the memory there is.
fn array(elem: &Type, raw: &[u8]) -> Result<Value, BoxErr> {
    let mut at = 0;
    let ndim = usize::try_from(i32::from_be_bytes(take(raw, &mut at)?)).ok().filter(|n| *n <= MAX_DIMS).ok_or("bad array dimensions")?;
    let _has_null = i32::from_be_bytes(take(raw, &mut at)?);
    let _oid = u32::from_be_bytes(take(raw, &mut at)?);
    let mut dims = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        dims.push(usize::try_from(i32::from_be_bytes(take(raw, &mut at)?)).map_err(|_| "bad array size")?);
        let _lower = i32::from_be_bytes(take(raw, &mut at)?);
    }
    let total: usize = if ndim == 0 { 0 } else { dims.iter().try_fold(1usize, |t, d| t.checked_mul(*d)).ok_or("bad array size")? };
    // Each item is at least its 4-byte length.
    if total > (raw.len() - at) / 4 {
        return Err("array too short".into());
    }
    let mut items = Vec::with_capacity(total);
    for _ in 0..total {
        let len = i32::from_be_bytes(take(raw, &mut at)?);
        if len < 0 {
            items.push(Value::Nil);
        } else {
            let len = len as usize;
            let bytes = raw.get(at..at.saturating_add(len)).ok_or("array too short")?;
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
        // jsonb's `?` after a value is the operator; after `=`, `(`, `,` or a keyword it is a parameter.
        assert_eq!(p("select * from t where data ? 'a' and x = ?", &[]), "select * from t where data ? 'a' and x = $1");
        assert_eq!(p("select x->'k' ? ?, (data) ? ?", &[]), "select x->'k' ? $1, (data) ? $2");
        assert_eq!(p("select ? limit ? offset ?", &[]), "select $1 limit $2 offset $3");
        assert_eq!(p("select * from t where a in (?, ?) and b between ? and ? or c like ?", &[]), "select * from t where a in ($1, $2) and b between $3 and $4 or c like $5");
        assert_eq!(p("select case when ? then ? else ? end", &[]), "select case when $1 then $2 else $3 end");
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
        let o = take_options("postgres://h/db?pool_size=3&sslmode=require").unwrap();
        assert_eq!((o.url.as_str(), o.max, o.schema, o.tls.mode), ("postgres://h/db", 3, None, Some(SslMode::Require)));
        let o = take_options("postgres://h/db?schema=acme").unwrap();
        assert_eq!((o.url.as_str(), o.max, o.schema), ("postgres://h/db", DEFAULT_POOL, Some("acme".into())));
        assert!(take_options("postgres://h/db?schema=a;drop").is_err());
    }

    #[test]
    fn lengths_a_server_sends_are_checked() {
        let be = |xs: &[i32]| xs.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
        // What the review saw: negative sizes cast to huge ones.
        assert!(array(&Type::INT4, &be(&[-1, 0, 23])).is_err());
        assert!(array(&Type::INT4, &be(&[1, 0, 23, -1, 1])).is_err());
        assert!(array(&Type::INT4, &be(&[1, 0, 23, i32::MAX, 1])).is_err());
        assert!(array(&Type::INT4, &be(&[2, 0, 23, 65536, 1, 65536, 1])).is_err());
        assert!(array(&Type::INT4, &be(&[7, 0, 23])).is_err());
        let ok = array(&Type::INT4, &be(&[1, 0, 23, 2, 1, 4, 7, -1])).unwrap();
        assert_eq!(ok.repr(), "[7, null]");
        let n = |h: [i16; 4]| h.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
        assert!(numeric(&n([-1, 0, 0, 0])).is_err());
        assert!(numeric(&n([0, 0, 0, -1])).is_err());
        assert_eq!(numeric(&n([0, 0, 0, 2])).unwrap().repr(), "0.00m");
    }

    #[test]
    fn tls_is_required_for_a_server_on_another_machine() {
        let mode = |url: &str| {
            let o = take_options(url).unwrap();
            let c: tokio_postgres::Config = o.url.parse().unwrap();
            o.tls.mode_for(&c)
        };
        // What the review saw: `prefer`, which someone in the middle can turn off.
        assert_eq!(mode("postgres://u:p@db.example.com/app"), SslMode::VerifyFull);
        assert_eq!(mode("postgres://u:p@10.0.0.5:5432/app"), SslMode::VerifyFull);
        assert_eq!(mode("postgres://u:p@db.example.com/app?hostaddr=127.0.0.1"), SslMode::VerifyFull);
        assert_eq!(mode("postgres://u:p@localhost/app?hostaddr=10.0.0.5"), SslMode::VerifyFull);
        // Said in the URL: as it says. On this machine: Postgres's default.
        assert_eq!(mode("postgres://u:p@db.internal/app?sslmode=disable"), SslMode::Disable);
        assert_eq!(mode("postgres://u:p@db.internal/app?sslmode=verify-ca&sslrootcert=/ca.pem"), SslMode::VerifyCa);
        assert_eq!(mode("postgres://u:p@db.internal/app?sslmode=require"), SslMode::Require);
        for local in ["postgres://u@localhost/app", "postgres://u@127.0.0.1:5432/app", "postgres://u@[::1]/app"] {
            assert_eq!(mode(local), SslMode::Prefer, "{}", local);
        }
        // The operating system's CAs: only to check the server's name too.
        assert_eq!(mode("postgres://u@localhost/app?sslrootcert=system"), SslMode::VerifyFull);
        assert!(take_options("postgres://h/db?sslmode=allow").is_err());
    }

    #[test]
    fn tls_options_leave_the_url_and_say_what_is_checked() {
        let o = take_options("postgres://u@h/db?sslmode=verify-ca&sslrootcert=%2Fetc%2Fdb%20ca.pem&connect_timeout=5&sslcert=/c.pem&sslkey=/k.pem").unwrap();
        assert_eq!(o.url, "postgres://u@h/db?connect_timeout=5");
        assert_eq!(o.tls, Tls { mode: Some(SslMode::VerifyCa), root: Some("/etc/db ca.pem".into()), cert: Some("/c.pem".into()), key: Some("/k.pem".into()) });
        let t = |root: Option<&str>| Tls { root: root.map(str::to_string), ..Tls::default() };
        assert_eq!(t(None).check(SslMode::VerifyFull), Check::ChainAndName);
        assert_eq!(t(None).check(SslMode::Require), Check::Nothing);
        assert_eq!(t(None).check(SslMode::Prefer), Check::Nothing);
        // libpq's: a root certificate given makes `require` check the chain.
        assert_eq!(t(Some("/ca.pem")).check(SslMode::Require), Check::Chain);
        assert_eq!(t(Some("/ca.pem")).check(SslMode::VerifyCa), Check::Chain);
        assert_eq!(t(Some("/ca.pem")).check(SslMode::Disable), Check::Nothing);
        let err = |tls: Tls, mode| tls.client_config(mode).err().unwrap_or_default();
        assert!(err(t(None), SslMode::VerifyCa).contains("needs sslrootcert"));
        assert!(err(t(Some("system")), SslMode::Require).contains("needs sslmode=verify-full"));
        assert!(err(t(Some("/no/such/ca.pem")), SslMode::VerifyFull).contains("sslrootcert /no/such/ca.pem"));
        assert!(err(Tls { cert: Some("/c.pem".into()), ..Tls::default() }, SslMode::Require).contains("go together"));
        assert!(t(None).client_config(SslMode::VerifyFull).is_ok());
        assert!(t(None).client_config(SslMode::Require).is_ok());
    }

    /// The certificates a server could present, checked as each mode does
    /// (the verifier the connection would use, without a server).
    #[test]
    fn certificates_are_checked_as_the_mode_says() {
        use rustls::pki_types::{ServerName, UnixTime};
        let dir = std::env::temp_dir().join(format!("quillo-pg-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ca = |name: &str| {
            let key = rcgen::KeyPair::generate().unwrap();
            let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
            p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            p.distinguished_name.push(rcgen::DnType::CommonName, name);
            let cert = p.self_signed(&key).unwrap();
            std::fs::write(dir.join(format!("{}.pem", name)), cert.pem()).unwrap();
            rcgen::Issuer::new(p, key)
        };
        let ours = ca("ours");
        let _theirs = ca("theirs");
        let server = rcgen::CertificateParams::new(vec!["db.example.internal".to_string()]).unwrap().signed_by(&rcgen::KeyPair::generate().unwrap(), &ours).unwrap();
        let der = server.der().clone();
        let check = |tls: &Tls, mode: SslMode, host: &str| {
            let v = tls.verifier(mode, Arc::new(rustls::crypto::ring::default_provider())).unwrap();
            v.verify_server_cert(&der, &[], &ServerName::try_from(host.to_string()).unwrap(), &[], UnixTime::now()).is_ok()
        };
        let ok = |root: &str, mode: SslMode, host: &str| check(&Tls { root: Some(dir.join(root).to_str().unwrap().to_string()), ..Tls::default() }, mode, host);
        assert!(ok("ours.pem", SslMode::VerifyFull, "db.example.internal"));
        assert!(!ok("ours.pem", SslMode::VerifyFull, "10.0.0.5"));
        assert!(!ok("theirs.pem", SslMode::VerifyFull, "db.example.internal"));
        assert!(ok("ours.pem", SslMode::VerifyCa, "10.0.0.5"));
        assert!(!ok("theirs.pem", SslMode::VerifyCa, "10.0.0.5"));
        assert!(!ok("theirs.pem", SslMode::Require, "db.example.internal"));
        // Without a root certificate, `require` takes any.
        assert!(check(&Tls::default(), SslMode::Require, "x"));
        // And the usual public CAs do not know a private one's.
        assert!(!check(&Tls::default(), SslMode::VerifyFull, "db.example.internal"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
