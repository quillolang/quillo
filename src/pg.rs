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
    /// The decimals of a `money` value here, once a result has had one (`money_digits`).
    money: Option<u32>,
    /// Statements prepared with the types of some values declared (`declared`), or None where
    /// PostgreSQL would not take them: by the types and the SQL.
    typed: HashMap<String, Option<postgres::Statement>>,
    /// In a transaction Quillo began (`batch`), where a statement that fails to prepare would
    /// end it.
    in_tx: bool,
    /// The pool's schema changes, and the count of them its statements were prepared under
    /// (`Pool::schema`).
    schema: (Arc<Schema>, u64),
}

pub struct Pool {
    config: postgres::Config,
    /// The same, for the connections requests share (`Lanes`).
    async_config: tokio_postgres::Config,
    tls: Connector,
    idle: Mutex<(Vec<Conn>, usize)>,
    freed: Condvar,
    max: usize,
    lanes: OnceLock<Arc<Lanes>>,
    /// Names called as functions, and whether one of them is not PostgreSQL's own (a function
    /// of the program's may change the session: see `db::isolated`).
    own_fns: Mutex<HashMap<String, bool>>,
    /// Its tables known to have changed: by a schema's statement run here, or a kept statement
    /// PostgreSQL refused as prepared before one ("cached plan must not change result type").
    /// Each connection lets go its statements on what changed when it is behind, so one that
    /// saw the change saves the others (in a transaction too) the failure (FEEDBACK55.md, Q-745;
    /// branch feedback55).
    schema: Arc<Schema>,
}

/// What a session changed is set back with, its prepared statements kept (Quillo's own among
/// them): `discard all` but `deallocate all` (FEEDBACK44.md, Q-581).
pub const RESET: &str = "set session authorization default; reset all; close all; discard temp; select pg_advisory_unlock_all(); unlisten *";

/// Which of the names given are functions of the database's own (not in `pg_catalog`).
const OWN_FNS: &str = "select distinct p.proname::text as name from pg_proc p join pg_namespace n on n.oid = p.pronamespace \
                       where p.proname = any($1::text[]) and n.nspname not in ('pg_catalog', 'information_schema')";

impl Pool {
    /// Whether one of these names is a function of the database's own, as far as known: None
    /// when a name was never looked up (`look_up_fns`).
    pub fn calls_own(&self, names: &[Box<str>]) -> Option<bool> {
        let known = self.own_fns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut own = false;
        for n in names {
            own |= *known.get(&**n)?;
        }
        Some(own)
    }

    /// Looks the names up (on a connection of the pool's), and keeps what they are.
    pub fn look_up_fns(&self, names: &[Box<str>]) -> Result<bool, String> {
        let mut c = self.checkout_nested()?;
        let list: Vec<Value> = names.iter().map(|n| Value::str(&**n)).collect();
        let rows = c.query(OWN_FNS, &[Value::list(list)], None);
        self.give_back(c);
        let rows = rows?;
        let own: Vec<String> = rows.iter().filter_map(|r| if let Value::Map(m) = r { m.get("name").map(|v| v.display().to_string()) } else { None }).collect();
        let mut known = self.own_fns.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if known.len() >= 4096 {
            known.clear();
        }
        for n in names {
            known.insert(n.to_string(), own.iter().any(|o| o == &**n));
        }
        Ok(!own.is_empty())
    }

    /// The program made, replaced or dropped functions: what names are is looked up again.
    pub fn forget_fns(&self) {
        self.own_fns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }

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
        // Added to the URL's own `options`, which it replaced: `?options=-c statement_timeout=100`
        // was lost with `&schema=` (FEEDBACK55.md, Q-737; branch feedback55).
        if let Some(s) = &schema {
            let path = search_path(s);
            let options = match config.get_options() {
                Some(o) if !o.trim().is_empty() => format!("{} {}", o, path),
                _ => path,
            };
            config.options(&options);
        }
        let pool = Pool {
            config: config.clone().into(),
            async_config: config,
            tls,
            idle: Mutex::new((Vec::new(), 0)),
            freed: Condvar::new(),
            max,
            lanes: OnceLock::new(),
            own_fns: Mutex::new(HashMap::new()),
            schema: Arc::default(),
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

    /// Makes the schema a URL names, or says false when it is there. Of two at once, the
    /// server lets one make it (FEEDBACK20.md, Q-165; branch feedback20-fixes). A database
    /// of its own (`postgres://host/app_{tenant}`) is made beforehand, so it only says.
    pub fn claim(url: &str) -> Result<bool, String> {
        let Options { url: plain, max, schema, tls } = take_options(url)?;
        let Some(schema) = schema else {
            return Pool::open_as(url, false).map(|_| false).or_else(|e| Err(format!("its database is made beforehand: {}", e)));
        };
        let mut config: tokio_postgres::Config = plain.parse().map_err(|e| format!("bad Postgres URL: {}", e))?;
        let tls = tls.settle(&mut config)?;
        let pool = Pool {
            config: config.clone().into(),
            async_config: config,
            tls,
            idle: Mutex::new((Vec::new(), 0)),
            freed: Condvar::new(),
            max,
            lanes: OnceLock::new(),
            own_fns: Mutex::new(HashMap::new()),
            schema: Arc::default(),
        };
        let mut c = pool.checkout()?;
        match c.batch(&format!("create schema \"{}\"", schema)) {
            Ok(()) => Ok(true),
            Err(e) if e.contains("already exists") => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn connect(&self) -> Result<Conn, String> {
        let client = self.config.connect(self.tls.clone()).map_err(|e| describe(&e))?;
        let schema = (self.schema.clone(), self.schema.count.load(Relaxed));
        Ok(Conn { client, stmts: HashMap::new(), conflict: None, aside: false, money: None, typed: HashMap::new(), in_tx: false, schema })
    }

    /// An idle connection or a new one, or None when all `max` are taken.
    pub fn try_checkout(&self) -> Result<Option<Conn>, String> {
        let mut g = self.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // One the server ended while it sat idle, as the client knows by now, is let go here,
        // not handed out to fail (FEEDBACK49.md, Q-601; found in review).
        while let Some(c) = g.0.pop() {
            if c.client.is_closed() {
                g.1 = g.1.saturating_sub(1);
                continue;
            }
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
                    schema: self.schema.clone(),
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

    /// A connection not to give back (one whose session could not be reset): closed, and
    /// its place in the pool freed.
    /// How many connections it holds at most.
    pub fn size(&self) -> usize {
        self.max
    }

    pub fn discard(&self, c: Conn) {
        drop(c);
        let mut g = self.idle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.1 = g.1.saturating_sub(1);
        drop(g);
        self.freed.notify_one();
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
            // For the tenants' list (see `TenantDbs`), not for the connection.
            Some(("open_tenants", v)) => {
                v.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| format!("bad open_tenants: {}", v))?;
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

/// rustls's connector, which also takes a name it cannot check (a Unix socket's, given as
/// "") and fails only if TLS is then tried: tokio-postgres asks for a connector for every
/// host, TLS or not, and rustls refused the socket's empty name, so no socket could be opened
/// (found by FEEDBACK49.md, Q-602, which made the failed `db.open` stop the program; branch
/// main).
#[derive(Clone)]
pub struct Connector(tokio_postgres_rustls::MakeRustlsConnect);

impl<S> tokio_postgres::tls::MakeTlsConnect<S> for Connector
where
    tokio_postgres_rustls::MakeRustlsConnect: tokio_postgres::tls::MakeTlsConnect<S>,
    <<tokio_postgres_rustls::MakeRustlsConnect as tokio_postgres::tls::MakeTlsConnect<S>>::TlsConnect as tokio_postgres::tls::TlsConnect<S>>::Future: Send + 'static,
{
    type Stream = <tokio_postgres_rustls::MakeRustlsConnect as tokio_postgres::tls::MakeTlsConnect<S>>::Stream;
    type TlsConnect = Named<<tokio_postgres_rustls::MakeRustlsConnect as tokio_postgres::tls::MakeTlsConnect<S>>::TlsConnect>;
    type Error = std::convert::Infallible;

    fn make_tls_connect(&mut self, domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        Ok(Named(self.0.make_tls_connect(domain).ok()))
    }
}

/// A connector for a host, or none for a name TLS cannot check.
pub struct Named<T>(Option<T>);

impl<S, T> tokio_postgres::tls::TlsConnect<S> for Named<T>
where
    T: tokio_postgres::tls::TlsConnect<S>,
    T::Future: Send + 'static,
{
    type Stream = T::Stream;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T::Stream, Self::Error>> + Send>>;

    fn connect(self, stream: S) -> Self::Future {
        match self.0 {
            Some(t) => {
                let f = t.connect(stream);
                Box::pin(async move { f.await.map_err(Into::into) })
            }
            None => Box::pin(async { Err("TLS cannot check this host's name (a Unix socket has none): connect without TLS, sslmode=disable".into()) }),
        }
    }
}

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
        // Over a Unix socket, as libpq has it, no TLS: it was tried, with the socket's path for
        // the server's name, and failed (found by FEEDBACK49.md, Q-602, which made a failed
        // `db.open` stop the program; branch main).
        #[cfg(unix)]
        let socket = !config.get_hosts().is_empty() && config.get_hosts().iter().all(|h| matches!(h, Host::Unix(_)));
        #[cfg(not(unix))]
        let socket = false;
        match self.mode {
            Some(m) => m,
            None if socket => SslMode::Disable,
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
    fn settle(&self, config: &mut tokio_postgres::Config) -> Result<Connector, String> {
        use tokio_postgres::config::SslMode as Driver;
        let mode = self.mode_for(config);
        config.ssl_mode(match mode {
            SslMode::Disable => Driver::Disable,
            SslMode::Prefer => Driver::Prefer,
            _ => Driver::Require,
        });
        self.client_config(mode).map(|c| Connector(tokio_postgres_rustls::MakeRustlsConnect::new(c)))
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
            // Its SQLSTATE, which `db.error_kind` reads: the words are in the server's language
            // (FEEDBACK43.md, Q-493; branch feedback40-43).
            s.push_str(&format!(" [{}]", db.code().code()));
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

/// A pool's schema changes (`Pool::schema`): how many, and the names each touched (None: any),
/// the last `SCHEMA_LOG` of them.
#[derive(Default)]
pub struct Schema {
    count: AtomicU64,
    log: Mutex<std::collections::VecDeque<(u64, Option<Vec<Box<str>>>)>>,
}

/// How many schema changes a pool remembers the names of; one further behind lets go all.
const SCHEMA_LOG: usize = 64;

impl Schema {
    /// A change to the tables, views or functions `names` (lower case; None: any).
    fn note(&self, names: Option<Vec<Box<str>>>) {
        let mut log = self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let n = self.count.load(Relaxed) + 1;
        log.push_back((n, names));
        if log.len() > SCHEMA_LOG {
            log.pop_front();
        }
        self.count.store(n, Relaxed);
    }

    /// The names changed since count `seen`; None: any (one said so, or it is too far back).
    fn since(&self, seen: u64) -> Option<Vec<Box<str>>> {
        let log = self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if log.front().map_or(true, |(n, _)| *n > seen + 1) {
            return None;
        }
        let mut out = Vec::new();
        for (_, names) in log.iter().filter(|(n, _)| *n > seen) {
            out.extend(names.as_ref()?.iter().cloned());
        }
        Some(out)
    }

    /// What a statement changed, if it changed a schema (see `sql::Scan::ddl`).
    fn changed_by(&self, scan: &crate::sql::Scan) {
        if let Some(names) = scan.ddl() {
            self.note(names.map(<[Box<str>]>::to_vec));
        }
    }

    /// After a kept statement was refused as prepared before a change: what it reads changed.
    fn refused(&self, scan: &crate::sql::Scan) {
        let names: Vec<Box<str>> = touched(scan).map(|t| t.to_lowercase().into_boxed_str()).collect();
        self.note((!names.is_empty()).then_some(names));
    }
}

/// The tables and functions a statement reads, writes or calls (tables in lower case, names
/// as PostgreSQL folds them).
fn touched(scan: &crate::sql::Scan) -> impl Iterator<Item = &str> + '_ {
    scan.tables.reads.iter().chain(&scan.tables.writes).map(String::as_str).chain(scan.calls.iter().map(|c| &**c))
}

/// Whether the statement kept as `sql` touches any of `names`. A schema's statement itself
/// (`drop table if exists x` run each time) keeps no plan of its tables, and is kept.
fn touches(sql: &str, names: &[Box<str>]) -> bool {
    let scan = crate::sql::scan(crate::sql::Dialect::Postgres, sql);
    let found = scan.ddl().is_none() && touched(&scan).any(|t| names.iter().any(|n| n.eq_ignore_ascii_case(t)));
    found
}

/// Lets go the statements kept that touch what changed since `seen` (all, when not known).
fn let_go<V>(schema: &Schema, seen: u64, stmts: &mut HashMap<String, V>, typed: &mut HashMap<String, Option<V>>) {
    match schema.since(seen) {
        None => {
            stmts.clear();
            typed.clear();
        }
        Some(names) => {
            stmts.retain(|k, _| !touches(k, &names));
            typed.retain(|k, _| !touches(k.split_once('\0').map_or(k.as_str(), |(_, s)| s), &names));
        }
    }
}

impl Conn {
    /// Its statements on what changed let go, when the pool's tables changed since they were
    /// prepared.
    fn current(&mut self) {
        let now = self.schema.0.count.load(Relaxed);
        if now != self.schema.1 {
            let_go(&self.schema.0, self.schema.1, &mut self.stmts, &mut self.typed);
            self.schema.1 = now;
        }
    }

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

    /// Prepares `sql` for `params`, casting from text where needed, and with the types of
    /// values PostgreSQL would take as text declared (`declared`); and the statement as
    /// PostgreSQL inferred it, whose types say how a value goes as text (`PgParam`).
    fn statement(&mut self, sql: &str, params: &[Value]) -> Result<(postgres::Statement, postgres::Statement), String> {
        self.current();
        let stmt = self.prepare(sql)?;
        let want = stmt.params().len();
        if want != params.len() {
            return Err(crate::sql::wrong_count(want, params.len()));
        }
        let casts = casts_for(sql, stmt.params());
        if let Some(types) = declared(stmt.params(), params) {
            if let Some(s) = self.typed(casts.as_deref().unwrap_or(sql), types)? {
                return Ok((s, stmt));
            }
        }
        match casts {
            None => Ok((stmt.clone(), stmt)),
            Some(cast_sql) => Ok((self.prepare(&cast_sql)?, stmt)),
        }
    }

    /// After PostgreSQL said a kept statement's plan no longer fits its tables ("cached plan
    /// must not change result type", once an `alter table` changed them, here or from another
    /// server): this connection's statements are let go, to be prepared again. Whether to try
    /// once more: not in a transaction, which the error ended. Each request on the connection
    /// failed until the server restarted (FEEDBACK55.md, Q-745; branch feedback55).
    fn replanned(&mut self, e: &postgres::Error, scan: &crate::sql::Scan) -> bool {
        if !plan_changed(e) {
            return false;
        }
        self.schema.0.refused(scan);
        self.current();
        !self.in_tx
    }

    /// `sql` prepared with the types `declared` gives, or with those of them PostgreSQL takes
    /// (not `x = ?` with a text column and an int), or None for none: worked out once, and kept.
    fn typed(&mut self, sql: &str, (sent, mut want): (Vec<Type>, Vec<Type>)) -> Result<Option<postgres::Statement>, String> {
        let key = typed_key(sql, &want);
        if let Some(s) = self.typed.get(&key) {
            return Ok(s.clone());
        }
        let mut s = None;
        for _ in 0..TRIES {
            match self.probe(sql, &want)? {
                Ok(st) => {
                    s = Some(st);
                    break;
                }
                Err(at) => {
                    if !as_sent_near(sql, at, &sent, &mut want) {
                        break;
                    }
                }
            }
        }
        if self.typed.len() >= 256 {
            self.typed.clear();
        }
        self.typed.insert(key, s.clone());
        Ok(s)
    }

    /// `sql` prepared with these types, or where PostgreSQL's refusal points (see `typed`). In
    /// a transaction it is tried in a savepoint, as a statement that fails to prepare ends the
    /// transaction.
    fn probe(&mut self, sql: &str, types: &[Type]) -> Result<Result<postgres::Statement, Option<usize>>, String> {
        if self.in_tx {
            self.client.batch_execute("savepoint quillo_types").map_err(|e| describe(&e))?;
        }
        let s = self.client.prepare_typed(sql, types).map_err(|e| refused_at(&e));
        if self.in_tx {
            let back = if s.is_ok() { "release savepoint quillo_types" } else { "rollback to savepoint quillo_types; release savepoint quillo_types" };
            self.client.batch_execute(back).map_err(|e| describe(&e))?;
        }
        Ok(s)
    }

    pub fn query(&mut self, sql: &str, params: &[Value], limit: Option<usize>) -> Result<Vec<Value>, String> {
        let (scan, params) = translate(sql, params)?;
        let sql = scan.native();
        let mut again = true;
        let rows = loop {
            let (stmt, inferred) = self.statement(sql, &params)?;
            let args = pg_params(&params, &inferred, &scan);
            let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
            match self.client.query(&stmt, &refs) {
                Ok(rows) => break rows,
                Err(e) if again && self.replanned(&e, &scan) => again = false,
                Err(e) => return Err(self.failed(&e)),
            }
        };
        let digits = match (has_money(&rows), self.money) {
            (false, _) => 2,
            (true, Some(d)) => d,
            (true, None) => *self.money.insert(money_digits(self.client.query_one(MONEY_DIGITS, &[]))?),
        };
        values(&rows, limit, digits)
    }

    /// `{ changes, last_id }`; Postgres has no last insert id, so use
    /// `insert ... returning id` with `db.one` for that. `changes` as the other databases count
    /// them: 0 for a statement that reads (PostgreSQL counted a `select`'s rows), and a
    /// script's last statement's, where it was null (FEEDBACK55.md, Q-753; branch feedback55).
    pub fn exec(&mut self, sql: &str, params: &[Value]) -> Result<Value, String> {
        let scan = crate::sql::scan(crate::sql::Dialect::Postgres, sql);
        let changes = if params.is_empty() && scan.stmts.len() > 1 {
            let r = self.client.simple_query(sql);
            last_count(&scan, &r.map_err(|e| self.failed(&e))?)
        } else {
            let params = scan.values(params)?;
            let mut again = true;
            let n = loop {
                let (stmt, inferred) = self.statement(scan.native(), &params)?;
                let args = pg_params(&params, &inferred, &scan);
                let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
                match self.client.execute(&stmt, &refs) {
                    Ok(n) => break n,
                    Err(e) if again && self.replanned(&e, &scan) => again = false,
                    Err(e) => return Err(self.failed(&e)),
                }
            };
            if scan.reads() { 0 } else { n as i64 }
        };
        self.schema.0.changed_by(&scan);
        Ok(exec_result(changes))
    }

    pub fn batch(&mut self, sql: &str) -> Result<(), String> {
        let r = self.client.batch_execute(sql);
        // A migration's schema.
        if r.is_ok() {
            self.schema.0.changed_by(&crate::sql::scan(crate::sql::Dialect::Postgres, sql));
        }
        // Quillo's own `begin`, `commit`, `prepare transaction`... (the program's are refused).
        // One that failed leaves it as it was, but a `commit` ends the transaction anyway.
        let after = in_tx_after(self.in_tx, sql);
        self.in_tx = if r.is_ok() { after } else { self.in_tx && after };
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

/// How many decimals PostgreSQL's `money` has on a connection: those of its `lc_monetary`'s
/// currency (2 for dollars, 0 for yen, 3 for dinars). The binary value is a count of that
/// unit, which was read as cents whatever the currency: ￥1,234 came back as 12.34
/// (FEEDBACK50.md, Q-625; branch feedback50-fixes).
const MONEY_DIGITS: &str = "select scale(1::money::numeric)";

fn money_digits<E: std::fmt::Display>(row: Result<postgres::Row, E>) -> Result<u32, String> {
    let scale: i32 = row.map_err(|e| e.to_string())?.try_get(0).map_err(|e| e.to_string())?;
    Ok(scale.clamp(0, 28) as u32)
}

/// Whether a result has a `money` column.
fn has_money(rows: &[postgres::Row]) -> bool {
    rows.first().map_or(false, |r| r.columns().iter().any(|c| *c.type_() == Type::MONEY))
}

thread_local! {
    /// The `money` decimals of the connection whose rows are being read (`values`).
    static MONEY: std::cell::Cell<u32> = const { std::cell::Cell::new(2) };
}

/// Rows as maps of column name to value; a `money` value with `digits` decimals.
fn values(rows: &[postgres::Row], limit: Option<usize>, digits: u32) -> Result<Vec<Value>, String> {
    MONEY.with(|m| m.set(digits));
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
        types.iter().enumerate().filter(|(_, t)| !encodes_directly(t)).map(|(i, t)| (i + 1, cast_name(t))).collect();
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
    tls: Connector,
    slots: Vec<Mutex<Option<Arc<Lane>>>>,
    /// The pool's schema changes (`Pool::schema`).
    schema: Arc<Schema>,
}

/// How many connections requests share while none is stuck.
const USUAL_LANES: usize = 4;
/// How long a connection may wait for an answer before it counts as stuck.
const STUCK_MS: u64 = 20;

struct Lane {
    client: tokio_postgres::Client,
    stmts: Mutex<HashMap<String, tokio_postgres::Statement>>,
    /// As `Conn::typed`'s (never in a transaction here).
    typed: Mutex<HashMap<String, Option<tokio_postgres::Statement>>>,
    /// Queries sent and not yet answered.
    waiting: AtomicUsize,
    /// When it last answered, or was given work while it had none (`clock`).
    moved: AtomicU64,
    /// The decimals of a `money` value here, once a result has had one (`money_digits`).
    money: OnceLock<u32>,
    /// The pool's count of schema changes its statements were prepared under (`Pool::schema`).
    seen: AtomicU64,
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
        let l = Arc::new(Lane {
            client,
            stmts: Mutex::new(HashMap::new()),
            typed: Mutex::new(HashMap::new()),
            waiting: AtomicUsize::new(0),
            moved: AtomicU64::new(0),
            money: OnceLock::new(),
            seen: AtomicU64::new(self.schema.count.load(Relaxed)),
        });
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

    /// As `Conn::current`.
    fn current(&self, lane: &Lane) {
        let now = self.schema.count.load(Relaxed);
        let seen = lane.seen.swap(now, Relaxed);
        if seen != now {
            let mut stmts = lane.stmts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut typed = lane.typed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let_go(&self.schema, seen, &mut *stmts, &mut *typed);
        }
    }

    async fn statement(&self, lane: &Lane, sql: &str, params: &[Value]) -> Result<(tokio_postgres::Statement, tokio_postgres::Statement), String> {
        self.current(lane);
        let stmt = Self::prepare(lane, sql).await?;
        let want = stmt.params().len();
        if want != params.len() {
            return Err(crate::sql::wrong_count(want, params.len()));
        }
        let casts = casts_for(sql, stmt.params());
        if let Some(types) = declared(stmt.params(), params) {
            if let Some(s) = Self::typed(lane, casts.as_deref().unwrap_or(sql), types).await {
                return Ok((s, stmt));
            }
        }
        match casts {
            None => Ok((stmt.clone(), stmt)),
            Some(cast_sql) => Ok((Self::prepare(lane, &cast_sql).await?, stmt)),
        }
    }

    /// As `Conn::replanned`: the lane's statements are let go (it is never in a transaction).
    fn replanned(&self, lane: &Lane, e: &tokio_postgres::Error, scan: &crate::sql::Scan) -> bool {
        if !plan_changed(e) {
            return false;
        }
        self.schema.refused(scan);
        self.current(lane);
        true
    }

    /// As `Conn::typed`.
    async fn typed(lane: &Lane, sql: &str, (sent, mut want): (Vec<Type>, Vec<Type>)) -> Option<tokio_postgres::Statement> {
        let key = typed_key(sql, &want);
        if let Some(s) = lane.typed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            return s.clone();
        }
        let mut s = None;
        for _ in 0..TRIES {
            match lane.client.prepare_typed(sql, &want).await {
                Ok(st) => {
                    s = Some(st);
                    break;
                }
                Err(e) => {
                    if !as_sent_near(sql, refused_at(&e), &sent, &mut want) {
                        break;
                    }
                }
            }
        }
        let mut g = lane.typed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.len() >= 256 {
            g.clear();
        }
        g.insert(key, s.clone());
        s
    }

    /// Runs a query for request `n` (see `lane`).
    pub async fn query(self: Arc<Self>, n: usize, sql: String, params: Vec<Value>, limit: Option<usize>) -> Result<Vec<Value>, String> {
        let (scan, params) = translate(&sql, &params)?;
        let lane = self.lane(n).await?;
        let _sent = lane.send();
        let mut again = true;
        let rows = loop {
            let (stmt, inferred) = self.statement(&lane, scan.native(), &params).await?;
            let args = pg_params(&params, &inferred, &scan);
            let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
            match lane.client.query(&stmt, &refs).await {
                Ok(rows) => break rows,
                Err(e) if again && self.replanned(&lane, &e, &scan) => again = false,
                Err(e) => return Err(describe(&e)),
            }
        };
        let digits = match (has_money(&rows), lane.money.get()) {
            (false, _) => 2,
            (true, Some(d)) => *d,
            (true, None) => {
                let d = money_digits(lane.client.query_one(MONEY_DIGITS, &[]).await)?;
                *lane.money.get_or_init(|| d)
            }
        };
        values(&rows, limit, digits)
    }

    pub async fn exec(self: Arc<Self>, n: usize, sql: String, params: Vec<Value>) -> Result<Value, String> {
        let lane = self.lane(n).await?;
        let _sent = lane.send();
        let scan = crate::sql::scan(crate::sql::Dialect::Postgres, &sql);
        let changes = if params.is_empty() && scan.stmts.len() > 1 {
            last_count(&scan, &lane.client.simple_query(&sql).await.map_err(|e| describe(&e))?)
        } else {
            let params = scan.values(&params)?;
            let mut again = true;
            let n = loop {
                let (stmt, inferred) = self.statement(&lane, scan.native(), &params).await?;
                let args = pg_params(&params, &inferred, &scan);
                let refs: Vec<&(dyn ToSql + Sync)> = args.iter().map(|a| a as &(dyn ToSql + Sync)).collect();
                match lane.client.execute(&stmt, &refs).await {
                    Ok(n) => break n,
                    Err(e) if again && self.replanned(&lane, &e, &scan) => again = false,
                    Err(e) => return Err(describe(&e)),
                }
            };
            if scan.reads() { 0 } else { n as i64 }
        };
        self.schema.changed_by(&scan);
        Ok(exec_result(changes))
    }
}

// ---- placeholders --------------------------------------------------------

/// Quillo's `?`, `?NNN` and `:name` as `$1, $2, ...`, and the values in that order, read by the
/// tokenizer shared with the other databases (src/sql.rs): a `?` after a value is jsonb's
/// operator, `?||` a placeholder and `||`, `@?` jsonpath's operator, `?1` the first value, and
/// comments nest (FEEDBACK55.md, Q-749; branch feedback55). A query that numbers its own (`$1`)
/// is as written. Read once for each text.
fn translate(sql: &str, params: &[Value]) -> Result<(Arc<crate::sql::Scan>, Vec<Value>), String> {
    let scan = crate::sql::scan(crate::sql::Dialect::Postgres, sql);
    let values = scan.values(params)?;
    Ok((scan, values))
}

/// Replaces `$n` where `f(n)` says so.
fn rewrite_numbered(sql: &str, f: impl Fn(usize) -> Option<String>) -> String {
    let scan = crate::sql::scan(crate::sql::Dialect::Postgres, sql);
    let mut out = String::with_capacity(sql.len() + 16);
    let mut at = 0;
    for &(start, end, n) in scan.marks() {
        if let Some(s) = f(n) {
            out.push_str(&sql[at..start]);
            out.push_str(&s);
            at = end;
        }
    }
    out.push_str(&sql[at..]);
    out
}

/// `{ changes, last_id }`: PostgreSQL has no id of the last row inserted (`returning id` gives
/// it).
fn exec_result(changes: i64) -> Value {
    let mut m = Map::new();
    m.insert("changes".into(), Value::Int(changes));
    m.insert("last_id".into(), Value::Nil);
    Value::map(m)
}

/// A script's `changes`: its last statement's, 0 when that one reads.
fn last_count(scan: &crate::sql::Scan, said: &[tokio_postgres::SimpleQueryMessage]) -> i64 {
    let last = said.iter().rev().find_map(|m| if let tokio_postgres::SimpleQueryMessage::CommandComplete(n) = m { Some(*n) } else { None });
    let reads = scan.heads().last().map_or(true, |(a, _)| crate::sql::reading_word(a));
    if reads { 0 } else { last.unwrap_or(0) as i64 }
}

/// Whether PostgreSQL refused a kept statement because the tables it reads changed since it
/// was prepared: by the routine that says so (`RevalidateCachedQuery`), which does not change
/// with the server's language, and its SQLSTATE, 0A000.
fn plan_changed(e: &tokio_postgres::Error) -> bool {
    e.as_db_error().map_or(false, |d| d.code().code() == "0A000" && d.routine() == Some("RevalidateCachedQuery"))
}

/// Whether Quillo's own statements leave the connection in a transaction: `begin` and `start
/// transaction` begin one; `commit`, `end`, `abort`, `rollback` (not `rollback to`) and
/// `prepare transaction` end it.
fn in_tx_after(in_tx: bool, sql: &str) -> bool {
    let scan = crate::sql::scan(crate::sql::Dialect::Postgres, sql);
    crate::sql::in_tx_after(&scan, in_tx)
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

/// The types to declare for values PostgreSQL inferred another for, or None for none: an int
/// or a float where nothing in the SQL says what it is (`select ? as v`, `greatest(?, ?)`,
/// `substring('hello', ?, ?)`) was taken as text, so 9 came after 10 and `substring` read a
/// pattern; and an int past an `int4` column's range could not be sent, where it is no match
/// (FEEDBACK55.md, Q-731; branch feedback55). An int is declared `int4` when it fits (as
/// functions such as `substring` take), else `int8`; a float `float8`. Where PostgreSQL does
/// not take the types (`name = ?` with a text column), the statement is as inferred.
/// As (the types the values go as, inferred or text for a cast; those with the declared).
fn declared(types: &[Type], params: &[Value]) -> Option<(Vec<Type>, Vec<Type>)> {
    let mut out: Option<(Vec<Type>, Vec<Type>)> = None;
    for (i, (t, v)) in types.iter().zip(params).enumerate() {
        let int = |n: i64| if i32::try_from(n).is_ok() { Type::INT4 } else { Type::INT8 };
        let want = match (v, t) {
            (Value::Int(n), t) if matches!(*t, Type::TEXT | Type::UNKNOWN) => int(*n),
            (Value::Float(_), t) if matches!(*t, Type::TEXT | Type::UNKNOWN) => Type::FLOAT8,
            (Value::Int(n), t) if *t == Type::INT2 && i16::try_from(*n).is_err() => int(*n),
            (Value::Int(n), t) if *t == Type::INT4 && i32::try_from(*n).is_err() => Type::INT8,
            _ => continue,
        };
        let (_, w) = out.get_or_insert_with(|| {
            let sent: Vec<Type> = types.iter().map(|t| if encodes_directly(t) { t.clone() } else { Type::TEXT }).collect();
            (sent.clone(), sent)
        });
        w[i] = want;
    }
    out
}

/// How many times `typed` prepares a statement with types declared: all of them, then without
/// those near where PostgreSQL refused them. Never once for each value: 800 ints in `name in
/// (?, ...)` against a text column made 1,601 tries, 305 ms and as many lines in the server's
/// log (found in review).
const TRIES: usize = 2;

/// Where PostgreSQL said a statement was wrong, as a character from 1, if it did.
fn refused_at(e: &postgres::Error) -> Option<usize> {
    match e.as_db_error()?.position()? {
        postgres::error::ErrorPosition::Original(n) => Some(*n as usize),
        _ => None,
    }
}

/// After a refusal at character `at` of `sql`: the values near it (inside the parentheses
/// around it, or all of them) go as inferred again. Whether another try is worth it: some are
/// still declared, and some changed.
fn as_sent_near(sql: &str, at: Option<usize>, sent: &[Type], want: &mut [Type]) -> bool {
    let Some(at) = at else { return false };
    let byte = sql.char_indices().nth(at.saturating_sub(1)).map_or(sql.len(), |(b, _)| b);
    let mut changed = false;
    for n in crate::sql::values_near(sql, byte) {
        if let (Some(w), Some(s)) = (want.get_mut(n.wrapping_sub(1)), sent.get(n.wrapping_sub(1))) {
            if w != s {
                *w = s.clone();
                changed = true;
            }
        }
    }
    changed && want != sent
}

/// What `Conn::typed` keeps a statement by.
fn typed_key(sql: &str, types: &[Type]) -> String {
    let mut k: String = types.iter().map(|t| format!("{},", t.oid())).collect();
    k.push('\0');
    k.push_str(sql);
    k
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The type a value sent as text is cast to: its own, but `varbit` for `bit`, as a bare `bit` is
/// `bit(1)`, which cut `?::bit(8)` to its first digit (`10100000` to `10000000`). The column's
/// or the SQL's own `bit(n)` then holds it, as it would the text (FEEDBACK55.md, Q-730; branch
/// feedback55).
fn cast_name(t: &Type) -> String {
    match *t {
        Type::BIT => "varbit".into(),
        Type::BIT_ARRAY => "varbit[]".into(),
        _ => type_name(t),
    }
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
        Value::Map(_) => crate::db::json_text(v),
        other => other.display(),
    }
}

/// A value's text for the type PostgreSQL inferred for it (`inferred`), which it casts that
/// text to: a year of 0 or before as PostgreSQL writes it, `0044-03-15 BC`, where ISO 8601's
/// `-0043-03-15` (which Quillo reads) was taken for a time zone (FEEDBACK55.md, Q-743); and the
/// items of a json or jsonb array as JSON, where a list was taken for a sub-array (Q-756; branch
/// feedback55).
fn pg_text_as(v: &Value, inferred: &Type, assigned: bool) -> String {
    match (v, inferred) {
        (Value::Str(s), t) if matches!(*t, Type::DATE | Type::TIMESTAMP | Type::TIMESTAMPTZ) => bc_text(s).unwrap_or_else(|| s.to_string()),
        (Value::List(items), t) if matches!(*t, Type::JSON_ARRAY | Type::JSONB_ARRAY) => {
            let quote = |t: &str| format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""));
            let parts: Vec<String> = items
                .iter()
                .map(|x| match x {
                    Value::Nil => "NULL".to_string(),
                    // JSON text, as a json column's value takes it (Q-733).
                    Value::Str(s) if assigned => quote(s),
                    other => quote(&crate::db::json_text(other)),
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        _ => pg_text(v),
    }
}

/// An ISO 8601 date (or time stamp) whose year is 0 or before (`0000-06-01`, `-0043-03-15`), as
/// PostgreSQL writes it: year 0 is 1 BC, -43 is 44 BC. None for any other text.
fn bc_text(s: &str) -> Option<String> {
    let t = s.trim();
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits < 4 || rest.as_bytes().get(digits) != Some(&b'-') {
        return None;
    }
    let year: i64 = rest[..digits].parse().ok()?;
    if !neg && year != 0 {
        return None;
    }
    Some(format!("{:04}{} BC", year + 1, &rest[digits..]))
}

/// A value, the type PostgreSQL inferred for it (see `pg_text_as`), and whether it is a column's
/// value as written (`sql::Scan::assigned`).
struct PgParam<'a>(&'a Value, &'a Type, bool);

fn pg_params<'a>(params: &'a [Value], inferred: &'a postgres::Statement, scan: &crate::sql::Scan) -> Vec<PgParam<'a>> {
    params.iter().zip(inferred.params()).enumerate().map(|(i, (v, t))| PgParam(v, t, scan.assigned(i + 1))).collect()
}

impl std::fmt::Debug for PgParam<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0.repr())
    }
}

/// A float for `real` or `double precision`: one past the type's range is refused, as the
/// server refuses it in text, where it was stored as Infinity or 0 (`1e39`, `1e-46` for a
/// `real`; text `1e400` for a `double precision`; FEEDBACK55.md, Q-734); -0.0 goes as 0.0
/// (Q-735; branch feedback55).
fn float_for(v: &Value, ty: &Type) -> Result<f64, BoxErr> {
    let (f, nonzero) = match v {
        Value::Int(n) => (*n as f64, *n != 0),
        Value::Float(f) => (*f, *f != 0.0),
        Value::Dec(d) => (d.to_f64(), !d.is_zero()),
        Value::Str(s) => {
            let t = s.trim();
            let f: f64 = t.parse().map_err(|_| bad(v, ty))?;
            if !f.is_finite() && !t.bytes().any(|c| c.is_ascii_digit()) {
                return Err(format!("{} cannot be stored: SQLite keeps NaN as null and MySQL refuses NaN and the infinities, so no database is given one", v.repr()).into());
            }
            (f, t.bytes().take_while(|c| !matches!(c, b'e' | b'E')).any(|c| (b'1'..=b'9').contains(&c)))
        }
        _ => return Err(bad(v, ty)),
    };
    let real = *ty == Type::FLOAT4;
    let kept = if real { f as f32 as f64 } else { f };
    if !kept.is_finite() || (kept == 0.0 && nonzero) {
        let shown = if let Value::Float(f) = v { format!("{:e}", f) } else { v.repr() };
        return Err(format!("{} is out of range for type {}", shown, if real { "real" } else { "double precision" }).into());
    }
    Ok(kept + 0.0)
}

fn bad(v: &Value, t: &Type) -> BoxErr {
    format!("cannot send {} {} as {}", v.type_name(), v.repr(), t).into()
}

impl ToSql for PgParam<'_> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, BoxErr> {
        let v = self.0;
        if matches!(v, Value::Nil) {
            return Ok(IsNull::Yes);
        }
        let int = || -> Result<i64, BoxErr> {
            match v {
                Value::Int(n) => Ok(*n),
                // `as` would give the largest int for one too large: that stores a number no
                // one wrote (FEEDBACK16.md, Q-110; branch feedback20-fixes).
                Value::Float(f) if f.fract() == 0.0 && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(f) => Ok(*f as i64),
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
            Type::FLOAT4 => out.put_f32(float_for(v, ty)? as f32),
            Type::FLOAT8 => out.put_f64(float_for(v, ty)?),
            // A str given as a json column's value is JSON text, as MySQL's JSON column takes it:
            // `json.stringify(m)` was stored as one JSON string (FEEDBACK55.md, Q-733). Elsewhere
            // (`jsonb_set(j, '{a}', ?)`, a comparison) it is a JSON string, as MySQL's
            // `json_set` takes it (found in review; branch feedback55).
            Type::JSON | Type::JSONB => {
                if *ty == Type::JSONB {
                    out.put_u8(1);
                }
                match v {
                    Value::Str(s) if self.2 => out.put_slice(s.as_bytes()),
                    _ => out.put_slice(crate::db::json_text(v).as_bytes()),
                }
            }
            _ => out.put_slice(pg_text_as(v, self.1, self.2).as_bytes()),
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
        // -0 as 0.0, as every database is given (FEEDBACK55.md, Q-735; branch feedback55).
        Type::FLOAT4 => Value::Float(f32_wide(f32::from_be_bytes(take(raw, &mut at)?)) + 0.0),
        Type::FLOAT8 => Value::Float(f64::from_be_bytes(take(raw, &mut at)?) + 0.0),
        Type::NUMERIC => numeric(raw)?,
        Type::JSON => json_cell(raw),
        Type::JSONB => json_cell(raw.get(1..).unwrap_or_default()),
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
                    format!("{}T{}{}", date_text(days), time_text(rest as u64), z)
                }
            })
        }
        Type::TIME => Value::str(time_text(i64::from_be_bytes(take(raw, &mut at)?) as u64)),
        Type::INTERVAL => {
            let us = i64::from_be_bytes(take(raw, &mut at)?);
            let days = i32::from_be_bytes(take(raw, &mut at)?);
            let months = i32::from_be_bytes(take(raw, &mut at)?);
            let mut parts = Vec::new();
            if months != 0 {
                // `unsigned_abs`: the smallest of each has no positive of its own type, and
                // `abs` gave wrong text for it (FEEDBACK19.md, Q-160; branch feedback20-fixes).
                parts.push(format!("{} mon{}", months, if months.unsigned_abs() == 1 { "" } else { "s" }));
            }
            if days != 0 {
                parts.push(format!("{} day{}", days, if days.unsigned_abs() == 1 { "" } else { "s" }));
            }
            if us != 0 || parts.is_empty() {
                let sign = if us < 0 { "-" } else { "" };
                parts.push(format!("{}{}", sign, time_text(us.unsigned_abs())));
            }
            Value::str(parts.join(" "))
        }
        Type::BYTEA => Value::str(format!("\\x{}", raw.iter().map(|b| format!("{:02x}", b)).collect::<String>())),
        _ => match ty.kind() {
            Kind::Array(elem) => array(elem, raw)?,
            Kind::Domain(inner) => decode(inner, raw)?,
            _ => other_type(ty, raw),
        },
    })
}

/// A `real` (or MySQL's `FLOAT`) as the float that prints as it does, through its shortest text:
/// 0.1 read back as 0.10000000149011612, and 3.4e38 as 3.3999999521443642e+38, where psql and
/// Python's drivers show 0.1 (FEEDBACK55.md, Q-744; branch feedback55).
pub(crate) fn f32_wide(f: f32) -> f64 {
    if f.is_finite() {
        format!("{:e}", f).parse().unwrap_or(f as f64)
    } else {
        f as f64
    }
}

/// How deep JSON a database gives may nest: PostgreSQL keeps what its stack allows, past
/// serde_json's 128, which failed the whole query.
const JSON_DEPTH: usize = 1000;

/// JSON a database gives (PostgreSQL's json and jsonb, MySQL's JSON), as a value, read here
/// rather than by serde_json, whose limits failed the whole query for one value (FEEDBACK55.md,
/// Q-732; branch feedback55): a whole number past an int is a `dec` (2^64 - 1 and 2^64 - 2 read
/// as one float, and matched each other's rows); a lone surrogate (`"\ud800"`, which `json`
/// keeps) is U+FFFD; nesting goes to 1,000. What cannot be read even so (a number past a `dec`
/// or a float, `1e400`; deeper nesting) is an error value in that row's column, the other rows
/// and columns read.
pub(crate) fn json_cell(raw: &[u8]) -> Value {
    let mut r = Json { b: raw, i: 0 };
    let v = r.value(0).and_then(|v| {
        r.space();
        if r.i == raw.len() {
            Ok(v)
        } else {
            Err("text after its end".to_string())
        }
    });
    v.unwrap_or_else(|why| Value::err(Value::str(format!("db: this JSON could not be read: {}", why))))
}

/// A number's text in a message: its first digits, and how many there are, past 24.
fn shown(text: &str) -> String {
    if text.len() <= 24 {
        return text.to_string();
    }
    format!("{}... ({} characters)", &text[..16], text.len())
}

struct Json<'a> {
    b: &'a [u8],
    i: usize,
}

impl Json<'_> {
    fn space(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        self.space();
        let deeper = |depth: usize| if depth >= JSON_DEPTH { Err(format!("it nests more than {} deep", JSON_DEPTH)) } else { Ok(()) };
        match self.b.get(self.i) {
            None => Err("it ends early".into()),
            Some(b'{') => {
                deeper(depth)?;
                self.i += 1;
                let mut m = Map::new();
                self.space();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::map(m));
                }
                loop {
                    self.space();
                    if self.b.get(self.i) != Some(&b'"') {
                        return Err("a key is not a string".into());
                    }
                    let k = self.string()?;
                    self.space();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err("no `:` after a key".into());
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    m.insert(k.as_str().into(), v);
                    self.space();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::map(m));
                        }
                        _ => return Err("an object is not closed".into()),
                    }
                }
            }
            Some(b'[') => {
                deeper(depth)?;
                self.i += 1;
                let mut l = Vec::new();
                self.space();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::list(l));
                }
                loop {
                    l.push(self.value(depth + 1)?);
                    self.space();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::list(l));
                        }
                        _ => return Err("an array is not closed".into()),
                    }
                }
            }
            Some(b'"') => Ok(Value::str(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Nil),
            Some(_) => self.number(),
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, String> {
        if self.b[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err("a word that is not true, false or null".into())
        }
    }

    /// A number: an int, a `dec` past an int, a float with a fraction or an exponent.
    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let at = |i: usize| self.b.get(i).copied();
        let mut i = start;
        if at(i) == Some(b'-') {
            i += 1;
        }
        let digits = |mut i: usize| {
            while at(i).map_or(false, |c| c.is_ascii_digit()) {
                i += 1;
            }
            i
        };
        let whole = digits(i);
        if whole == i {
            return Err("a value that is not JSON".into());
        }
        let mut end = whole;
        if at(end) == Some(b'.') {
            end = digits(end + 1);
        }
        if matches!(at(end), Some(b'e' | b'E')) {
            let mut e = end + 1;
            if matches!(at(e), Some(b'+' | b'-')) {
                e += 1;
            }
            end = digits(e);
        }
        self.i = end;
        let text = std::str::from_utf8(&self.b[start..end]).unwrap_or("");
        if end == whole {
            if let Ok(n) = text.parse::<i64>() {
                return Ok(Value::Int(n));
            }
            // Past a dec (about 7.9e28), a float, as jsonb writes a float's whole number out in
            // digits (1.5e30 as 1500000000000000000000000000000): only one past a float is an
            // error (found in review).
            if let Some(d) = crate::decimal::Dec::parse(text) {
                return Ok(Value::Dec(d));
            }
        }
        let f: f64 = text.parse().map_err(|_| format!("the number {} is not one", shown(text)))?;
        let nonzero = self.b[start..end].iter().take_while(|c| !matches!(c, b'e' | b'E')).any(|c| (b'1'..=b'9').contains(c));
        if !f.is_finite() || (f == 0.0 && nonzero) {
            return Err(format!("the number {} is past what a float holds", shown(text)));
        }
        Ok(Value::Float(f + 0.0))
    }

    /// A string: its escapes read, a lone surrogate as U+FFFD.
    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let run = self.b[self.i..].iter().position(|&c| c == b'"' || c == b'\\').ok_or("a string is not closed")?;
            out.extend_from_slice(&self.b[self.i..self.i + run]);
            self.i += run;
            if self.b[self.i] == b'"' {
                self.i += 1;
                return Ok(String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()));
            }
            let c = *self.b.get(self.i + 1).ok_or("a string is not closed")?;
            self.i += 2;
            let one = match c {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let hi = self.hex4()?;
                    let low = (0xD800..0xDC00).contains(&hi) && self.b[self.i..].starts_with(b"\\u");
                    let lo = if low {
                        let save = self.i;
                        self.i += 2;
                        let lo = self.hex4()?;
                        if (0xDC00..0xE000).contains(&lo) {
                            Some(lo)
                        } else {
                            self.i = save;
                            None
                        }
                    } else {
                        None
                    };
                    match lo {
                        Some(lo) => char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)).unwrap_or('\u{fffd}'),
                        None => char::from_u32(hi).unwrap_or('\u{fffd}'),
                    }
                }
                _ => return Err("a string has an escape JSON does not have".into()),
            };
            let mut buf = [0u8; 4];
            out.extend_from_slice(one.encode_utf8(&mut buf).as_bytes());
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.b.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or("a \\u escape without four hex digits")?;
        self.i += 4;
        Ok(h)
    }
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
    let mut lower = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        dims.push(usize::try_from(i32::from_be_bytes(take(raw, &mut at)?)).map_err(|_| "bad array size")?);
        lower.push(i32::from_be_bytes(take(raw, &mut at)?));
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
    let list = if dims.is_empty() { Value::list(Vec::new()) } else { nest(&mut items.into_iter(), &dims) };
    // An array that does not start at 1 (`'[0:2]={1,2,3}'`) as PostgreSQL writes it, which
    // written back keeps its bounds: as a list, written back, every index moved (FEEDBACK55.md,
    // Q-756; branch feedback55).
    if lower.iter().any(|&l| l != 1) {
        let bounds: String = lower.iter().zip(&dims).map(|(l, n)| format!("[{}:{}]", l, *l as i64 + *n as i64 - 1)).collect();
        return Ok(Value::str(format!("{}={}", bounds, pg_text(&list))));
    }
    Ok(list)
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
    // A year before 1 with its sign and four digits, ISO 8601's way (1 BC is 0000, 44 BC is
    // -0043), which Quillo writes back as PostgreSQL's BC (`bc_text`); `{:04}` wrote -043
    // (FEEDBACK55.md, Q-743; branch feedback55).
    if y < 0 {
        format!("-{:04}-{:02}-{:02}", -y, m, d)
    } else {
        format!("{:04}-{:02}-{:02}", y, m, d)
    }
}

/// A value of a type with no reader of its own. Types with a binary form of their own are
/// read as PostgreSQL writes them; text-like ones (text, varchar, enums, citext) are text
/// already, and what is left is shown as `bytea` is, as bytes passed off as text say nothing
/// (FEEDBACK16.md, Q-117; branch feedback20-fixes). By its type alone, always: bytes that
/// happened to be UTF-8 were text, so `regtype` came back as control characters and one
/// `timetz` column as NULs in one row and hex in the next (FEEDBACK55.md, Q-742; branch
/// feedback55). `timetz`, `pg_lsn`, `xid` and `xid8` are read as PostgreSQL writes them.
fn other_type(ty: &Type, raw: &[u8]) -> Value {
    if let ("money", Ok(units)) = (ty.name(), <[u8; 8]>::try_from(raw)) {
        if let Some(d) = crate::decimal::Dec::new(i64::from_be_bytes(units) as i128, MONEY.with(|m| m.get())) {
            return Value::Dec(d);
        }
    }
    let hex = |b: &[u8], sep: &str| b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(sep);
    let text = match (ty.name(), raw) {
        ("inet" | "cidr", [family, bits, _, _, addr @ ..]) => {
            let shown = match (family, addr.len()) {
                (2, 4) => Some(std::net::Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]).to_string()),
                (3, 16) => <[u8; 16]>::try_from(addr).ok().map(|a| std::net::Ipv6Addr::from(a).to_string()),
                _ => None,
            };
            let whole = if addr.len() == 4 { 32 } else { 128 };
            shown.map(|a| if ty.name() == "cidr" || *bits != whole { format!("{}/{}", a, bits) } else { a })
        }
        ("macaddr", b) if b.len() == 6 => Some(hex(b, ":")),
        ("macaddr8", b) if b.len() == 8 => Some(hex(b, ":")),
        ("bit" | "varbit", [a, b, c, d, bytes @ ..]) => {
            let n = u32::from_be_bytes([*a, *b, *c, *d]) as usize;
            Some(bytes.iter().flat_map(|x| (0..8).rev().map(move |i| if x >> i & 1 == 1 { '1' } else { '0' })).take(n).collect())
        }
        ("timetz", b) if b.len() == 12 => {
            let us = i64::from_be_bytes(b[..8].try_into().unwrap_or_default());
            // Seconds west of UTC, as PostgreSQL keeps them; written east, as it writes them.
            let east = -(i32::from_be_bytes(b[8..].try_into().unwrap_or_default()) as i64);
            let (sign, a) = (if east < 0 { '-' } else { '+' }, east.unsigned_abs());
            let mut zone = format!("{}{:02}", sign, a / 3600);
            if a % 3600 != 0 {
                zone.push_str(&format!(":{:02}", a % 3600 / 60));
            }
            if a % 60 != 0 {
                zone.push_str(&format!(":{:02}", a % 60));
            }
            Some(format!("{}{}", time_text(us.max(0) as u64), zone))
        }
        ("pg_lsn", b) if b.len() == 8 => {
            let n = u64::from_be_bytes(b.try_into().unwrap_or_default());
            Some(format!("{:X}/{:X}", n >> 32, n & 0xffff_ffff))
        }
        ("xid" | "cid", b) if b.len() == 4 => return Value::Int(u32::from_be_bytes(b.try_into().unwrap_or_default()) as i64),
        ("xid8", b) if b.len() == 8 => {
            let n = u64::from_be_bytes(b.try_into().unwrap_or_default());
            return i64::try_from(n).map_or_else(|_| crate::decimal::Dec::parse(&n.to_string()).map_or(Value::Nil, Value::Dec), Value::Int);
        }
        _ => None,
    };
    match text {
        Some(t) => Value::str(t),
        None if text_form(ty) => Value::str(String::from_utf8_lossy(raw)),
        None => Value::str(format!("\\x{}", hex(raw, ""))),
    }
}

/// Whether a type's binary form is its text: text and its kin, enums, citext, `xml`, `"char"`,
/// `refcursor`.
fn text_form(ty: &Type) -> bool {
    is_text_type(ty) || matches!(*ty, Type::XML | Type::CHAR | Type::REFCURSOR)
}

/// Microseconds since midnight as HH:MM:SS[.ffffff].
fn time_text(us: u64) -> String {
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
        let p = |s: &str, _: &[Value]| crate::sql::scan(crate::sql::Dialect::Postgres, s).native().to_string();
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
        let (scan, vals) = translate("select :id::text, :name, :id, ':no'", &[Value::map(m)]).unwrap();
        assert_eq!(scan.native(), "select $1::text, $2, $1, ':no'");
        assert_eq!(vals.len(), 2);
        assert_eq!(rewrite_numbered("a = $1 and b = $12", |n| (n == 12).then(|| "X".into())), "a = $1 and b = X");
        let several = |sql: &str| crate::sql::scan(crate::sql::Dialect::Postgres, sql).stmts.len() > 1;
        assert!(several("create table a (x int); create table b (y int);"));
        assert!(!several("insert into a values (';');"));
        // Quillo's own transactions, as `typed` needs to know (FEEDBACK55.md, Q-731).
        assert!(in_tx_after(false, "begin isolation level serializable"));
        assert!(in_tx_after(true, "savepoint quillo_tx_2"));
        assert!(in_tx_after(true, "rollback to savepoint quillo_tx_2; release savepoint quillo_tx_2"));
        assert!(!in_tx_after(true, "commit"));
        assert!(!in_tx_after(true, "prepare transaction 'g'"));
        assert!(!in_tx_after(false, "commit prepared 'g'"));
        assert!(!in_tx_after(true, "ROLLBACK"));
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
        // Other options stay in the URL, for the driver (FEEDBACK55.md, Q-737).
        let o = take_options("postgres://h/db?options=-c%20statement_timeout%3D100&schema=acme").unwrap();
        assert_eq!((o.url.as_str(), o.schema), ("postgres://h/db?options=-c%20statement_timeout%3D100", Some("acme".into())));
        assert!(take_options("postgres://h/db?schema=a;drop").is_err());
    }

    // FEEDBACK55.md, Q-732: JSON as a database gives it, read with no limit that fails the
    // whole query.
    #[test]
    fn json_a_database_gives_is_read_cell_by_cell() {
        // An error's own words (another test may have errors hidden as the server hides them).
        let j = |t: &str| match json_cell(t.as_bytes()) {
            Value::Err(e, _, _) => format!("err(\"{}\")", e.display()),
            v => v.repr(),
        };
        assert_eq!(j(r#"{"a": [1, -2, 0.5, true, false, null, "x"], "b": {}}"#), r#"{ a: [1, -2, 0.5, true, false, null, "x"], b: {} }"#);
        assert_eq!(j("9223372036854775807"), "9223372036854775807");
        assert_eq!(j("9223372036854775808"), "9223372036854775808m");
        assert_eq!(j("-9223372036854775809"), "-9223372036854775809m");
        assert_eq!(j("79228162514264337593543950335"), "79228162514264337593543950335m");
        // Past a dec, a float; past a float, an error.
        assert_eq!(json_cell(b"79228162514264337593543950336").repr(), Value::Float(79228162514264337593543950336.0).repr());
        assert_eq!(json_cell(format!("1{}", "0".repeat(308)).as_bytes()).repr(), Value::Float(1e308).repr());
        assert!(j(&format!("1{}", "0".repeat(309))).contains("past what a float holds"));
        assert_eq!(json_cell(b"1e20").repr(), Value::Float(1e20).repr());
        assert!(j("1e400").contains("the number 1e400 is past what a float holds"));
        assert!(j("-1e-400").contains("past what a float holds"));
        assert_eq!(j("0e-400"), "0.0");
        assert_eq!(j(r#""a\"b\\c\/\n\t\u00e9\ud83d\ude00""#), "\"a\\\"b\\\\c/\\n\\té😀\"");
        assert_eq!(j(r#""\ud800x""#), "\"\u{fffd}x\"");
        assert_eq!(j(r#""\udc00\ud800\u0041""#), "\"\u{fffd}\u{fffd}A\"");
        assert_eq!(j(" [ ] "), "[]");
        let deep = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
        assert!(!j(&deep(1000)).contains("err("));
        assert!(j(&deep(1001)).contains("it nests more than 1000 deep"));
        for bad in ["", "[1,", "{\"a\" 1}", "{1: 2}", "tru", "[1] x", "\"abc", "\"\\q\"", "\"\\u12\"", "-", "nul"] {
            assert!(j(bad).starts_with("err(\"db: this JSON could not be read"), "{:?}: {}", bad, j(bad));
        }
    }

    // FEEDBACK55.md, Q-743, Q-744, Q-734, Q-742, Q-730.
    #[test]
    fn values_both_ways_as_postgresql_writes_them() {
        assert_eq!(date_text(-730_485), "0000-01-01");
        assert_eq!(date_text(-730_486), "-0001-12-31");
        assert_eq!(bc_text("-0043-03-15").as_deref(), Some("0044-03-15 BC"));
        assert_eq!(bc_text("0000-06-01").as_deref(), Some("0001-06-01 BC"));
        assert_eq!(bc_text(" -4713-11-24T00:00:00Z").as_deref(), Some("4714-11-24T00:00:00Z BC"));
        for t in ["0001-01-01", "2026-11-06", "-43-03-15", "infinity", "", "-", "0000", "12:00"] {
            assert_eq!(bc_text(t), None, "{}", t);
        }
        assert_eq!(f32_wide(0.1), 0.1);
        assert_eq!(f32_wide(3.4e38), 3.4e38);
        assert_eq!(f32_wide(-1.5), -1.5);
        assert!(f32_wide(f32::INFINITY).is_infinite() && f32_wide(f32::NAN).is_nan());
        let real = |v: Value| float_for(&v, &Type::FLOAT4).map_err(|e| e.to_string());
        let double = |v: Value| float_for(&v, &Type::FLOAT8).map_err(|e| e.to_string());
        assert!(real(Value::Float(1e39)).unwrap_err().contains("1e39 is out of range for type real"));
        assert!(real(Value::Float(1e-46)).unwrap_err().contains("out of range for type real"));
        assert!(real(Value::str("1e39")).unwrap_err().contains("out of range for type real"));
        assert!(double(Value::str("1e400")).unwrap_err().contains("out of range for type double precision"));
        assert!(double(Value::str("1e-400")).unwrap_err().contains("out of range"));
        assert!(double(Value::str("inf")).unwrap_err().contains("cannot be stored"));
        assert!(double(Value::str("x")).unwrap_err().contains("cannot send"));
        assert_eq!(real(Value::Float(-0.0)).unwrap().to_bits(), 0.0f64.to_bits());
        assert_eq!(double(Value::str("0e-400")).unwrap(), 0.0);
        assert_eq!(real(Value::Int(16_777_217)).unwrap(), 16_777_216.0);
        assert_eq!(double(Value::Dec(crate::decimal::Dec::parse("2.5").unwrap())).unwrap(), 2.5);
        assert_eq!(cast_name(&Type::BIT), "varbit");
        assert_eq!(cast_name(&Type::BIT_ARRAY), "varbit[]");
        assert_eq!(cast_name(&Type::DATE), "date");
        // By its type, whatever the bytes are.
        let tz = |us: i64, west: i32| other_type(&Type::TIMETZ, &[us.to_be_bytes().to_vec(), west.to_be_bytes().to_vec()].concat()).repr();
        assert_eq!(tz(0, 0), "\"00:00:00+00\"");
        assert_eq!(tz(43_200_000_000, -7200), "\"12:00:00+02\"");
        assert_eq!(tz(1_500_000, 19_815), "\"00:00:01.5-05:30:15\"");
        assert_eq!(other_type(&Type::REGTYPE, &[0, 0, 0, 23]).repr(), "\"\\\\x00000017\"");
        assert_eq!(other_type(&Type::REGTYPE, b"abcd").repr(), "\"\\\\x61626364\"");
        assert_eq!(other_type(&Type::PG_LSN, &0x16_b374_d848u64.to_be_bytes()).repr(), "\"16/B374D848\"");
        assert_eq!(other_type(&Type::XID, &7u32.to_be_bytes()).repr(), "7");
        assert_eq!(other_type(&Type::XID8, &u64::MAX.to_be_bytes()).repr(), "18446744073709551615m");
        assert_eq!(other_type(&Type::TEXT, "é".as_bytes()).repr(), "\"é\"");
        assert_eq!(other_type(&Type::XML, b"<a/>").repr(), "\"<a/>\"");
        assert_eq!(other_type(&Type::CHAR, b"x").repr(), "\"x\"");
        assert_eq!(other_type(&Type::TIMETZ, b"short").repr(), "\"\\\\x73686f7274\"");
    }

    // FEEDBACK55.md, Q-745 (found in review): what a connection lets go after a schema change.
    #[test]
    fn statements_on_what_changed_are_let_go() {
        let schema = Schema::default();
        let keep = |names: &[&str]| -> (HashMap<String, u8>, HashMap<String, Option<u8>>) {
            let stmts = names.iter().map(|n| (n.to_string(), 0)).collect();
            let typed = names.iter().map(|n| (format!("23,\0{}", n), Some(0))).collect();
            (stmts, typed)
        };
        let sqls = ["select * from a", "select * from b join c on true", "select f(x) from d"];
        schema.note(Some(vec!["b".into()]));
        let (mut stmts, mut typed) = keep(&sqls);
        let_go(&schema, 0, &mut stmts, &mut typed);
        assert!(stmts.contains_key("select * from a") && !stmts.contains_key("select * from b join c on true") && stmts.len() == 2);
        assert_eq!(typed.len(), 2);
        // A function's change; then one of any name, which lets go all.
        schema.note(Some(vec!["f".into()]));
        let (mut stmts, mut typed) = keep(&sqls);
        let_go(&schema, 1, &mut stmts, &mut typed);
        assert!(!stmts.contains_key("select f(x) from d") && stmts.len() == 2);
        schema.note(None);
        let (mut stmts, mut typed) = keep(&sqls);
        let_go(&schema, 2, &mut stmts, &mut typed);
        assert!(stmts.is_empty() && typed.is_empty());
        // Further behind than it remembers: all.
        for _ in 0..SCHEMA_LOG {
            schema.note(Some(vec!["zz".into()]));
        }
        assert_eq!(schema.since(2), None);
        assert_eq!(schema.since(3).map(|n| n.len()), Some(SCHEMA_LOG));
        assert_eq!(schema.since(schema.count.load(Relaxed) - 1), Some(vec!["zz".into()]));
        assert_eq!(schema.since(schema.count.load(Relaxed)), Some(Vec::new()));
    }

    // FEEDBACK55.md, Q-756: an array that does not start at 1, and a jsonb[]'s lists.
    #[test]
    fn arrays_keep_their_bounds_and_json_arrays_their_items() {
        let be = |xs: &[i32]| xs.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
        assert_eq!(array(&Type::INT4, &be(&[1, 0, 23, 2, 0, 4, 7, 4, 8])).unwrap().repr(), "\"[0:1]={\\\"7\\\",\\\"8\\\"}\"");
        assert_eq!(array(&Type::INT4, &be(&[1, 0, 23, 2, 1, 4, 7, 4, 8])).unwrap().repr(), "[7, 8]");
        assert_eq!(array(&Type::INT4, &be(&[1, 0, 23, 1, -3, 4, 7])).unwrap().repr(), "\"[-3:-3]={\\\"7\\\"}\"");
        assert_eq!(array(&Type::INT4, &be(&[0, 0, 23])).unwrap().repr(), "[]");
        let list = Value::list(vec![Value::list(vec![Value::Int(1)]), Value::str("{\"a\":1}"), Value::Nil, Value::map(Map::new())]);
        assert_eq!(pg_text_as(&list, &Type::JSONB_ARRAY, true), r#"{"[1]","{\"a\":1}",NULL,"{}"}"#);
        assert_eq!(pg_text_as(&list, &Type::JSONB_ARRAY, false), r#"{"[1]","\"{\\\"a\\\":1}\"",NULL,"{}"}"#);
        assert_eq!(pg_text_as(&list, &Type::INT4_ARRAY, true), pg_text(&list));
        assert_eq!(pg_text_as(&Value::str("-0043-03-15"), &Type::DATE, false), "0044-03-15 BC");
        assert_eq!(pg_text_as(&Value::str("-0043-03-15"), &Type::TEXT, false), "-0043-03-15");
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
        // A Unix socket has no TLS, as in libpq, unless the URL says so.
        #[cfg(unix)]
        {
            assert_eq!(mode("postgres://u@%2Ftmp%2Fpg:5432/app"), SslMode::Disable);
            assert_eq!(mode("postgres://u@%2Ftmp%2Fpg:5432/app?sslmode=require"), SslMode::Require);
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
