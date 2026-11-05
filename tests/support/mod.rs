//! What the tests share: throwaway PostgreSQL and MariaDB servers,
//! `quillo run` servers, and plain HTTP. Each is stopped when its value is
//! dropped, even when a test fails.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Whether a server listens on `port`. A port in the range Linux gives to
/// outgoing connections (32768 to 60999) can be connected to by a socket
/// given that same port, which then talks to itself: no server is there.
pub fn listening(port: u16) -> bool {
    match TcpStream::connect(("127.0.0.1", port)) {
        Ok(s) => s.local_addr().map_or(false, |a| a.port() != port),
        Err(_) => false,
    }
}

/// A port no one is listening on, and not given to another test here. Taken
/// from 10000 to 30000, below the ports systems give to outgoing connections
/// (32768 up on Linux, 49152 up on macOS): there, a test waiting for its
/// server could be given the very port it waits on, and hold it so that the
/// server cannot listen.
pub fn free_port() -> u16 {
    static GIVEN: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let port = 10_000 + fastrand::u16(0..20_000);
        let mut given = GIVEN.lock().unwrap();
        if !given.contains(&port) && std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            given.push(port);
            return port;
        }
    }
}

/// A fresh folder for a test.
pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---- PostgreSQL ------------------------------------------------------------

pub struct Pg {
    pub bin: PathBuf,
    dir: PathBuf,
    pub url: String,
}

impl Drop for Pg {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl")).arg("-D").arg(&self.dir).args(["-m", "immediate", "stop"]).output();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pg_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_TEST_PG_BIN") {
        return Some(PathBuf::from(p));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        candidates.extend(std::env::split_paths(&path));
    }
    for root in ["/usr/lib/postgresql", "/opt/homebrew/opt", "/usr/local/opt"] {
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                candidates.push(e.path().join("bin"));
            }
        }
    }
    candidates.into_iter().find(|d| d.join("initdb").exists() && d.join("pg_ctl").exists())
}

/// A throwaway PostgreSQL server, or None when it is not installed.
pub fn postgres(name: &str) -> Option<Pg> {
    let bin = pg_bin()?;
    let dir = std::env::temp_dir().join(format!("quillo-pg-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(bin.join("initdb"))
        .arg("-D")
        .arg(&dir)
        .args(["-U", "quillo", "--auth=trust", "-E", "UTF8", "--no-locale"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    assert!(out.status.success(), "initdb failed: {}", String::from_utf8_lossy(&out.stderr));
    let port = free_port();
    let opts = format!("-p {} -k {} -c listen_addresses=127.0.0.1 -c fsync=off -c max_connections=60", port, dir.display());
    let pg = Pg { bin, dir, url: format!("postgres://quillo@127.0.0.1:{}/postgres", port) };
    let ok = Command::new(pg.bin.join("pg_ctl"))
        .arg("-D")
        .arg(&pg.dir)
        .args(["-o", &opts, "-l"])
        .arg(pg.dir.join("log"))
        .args(["-w", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    assert!(ok.success(), "pg_ctl start failed: {}", std::fs::read_to_string(pg.dir.join("log")).unwrap_or_default());
    Some(pg)
}

impl Pg {
    /// The server's log, as written so far.
    #[allow(dead_code)]
    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("log")).unwrap_or_default()
    }

    /// A fresh database, and its URL.
    pub fn database(&self, name: &str) -> String {
        let out = Command::new(self.bin.join("psql"))
            .args([&self.url, "-q", "-c", &format!("drop database if exists {}", name), "-c", &format!("create database {}", name)])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        self.url.replace("/postgres", &format!("/{}", name))
    }
}

// ---- MariaDB / MySQL -------------------------------------------------------

pub struct Maria {
    child: Child,
    dir: PathBuf,
    pub port: u16,
}

impl Drop for Maria {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A throwaway MariaDB server, or None when it is not installed.
pub fn mariadb(name: &str) -> Option<Maria> {
    let candidates: Vec<PathBuf> = std::env::var("QUILLO_TEST_MYSQL_BIN")
        .map(|d| vec![PathBuf::from(d)])
        .unwrap_or_else(|_| vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into(), "/usr/sbin".into()]);
    let bin = candidates.into_iter().find(|d| d.join("mariadbd").exists() && d.join("mariadb-install-db").exists())?;
    let port = free_port();
    let dir = std::env::temp_dir().join(format!("quillo-my-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let ok = Command::new(bin.join("mariadb-install-db"))
        .arg(format!("--datadir={}", dir.join("data").display()))
        .args(["--auth-root-authentication-method=normal", "--skip-test-db"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    assert!(ok.success(), "mariadb-install-db failed");
    let child = Command::new(bin.join("mariadbd"))
        .arg("--no-defaults")
        .arg(format!("--datadir={}", dir.join("data").display()))
        .arg(format!("--port={}", port))
        .arg("--bind-address=127.0.0.1")
        .arg(format!("--socket={}", dir.join("s").display()))
        .arg(format!("--pid-file={}", dir.join("pid").display()))
        .arg(format!("--log-error={}", dir.join("err.log").display()))
        .arg("--skip-grant-tables")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    stop_with_tests(&child);
    let s = Maria { child, dir, port };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !listening(port) {
        assert!(Instant::now() < deadline, "mariadbd did not start: {}", std::fs::read_to_string(s.dir.join("err.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(s)
}

impl Maria {
    /// A database's URL, the database made first if it is not there: `db.open` makes none, as
    /// PostgreSQL's does not (FEEDBACK55.md, Q-736; branch feedback55). One with `{tenant}` in
    /// it is the tenants', made by `new_tenant`.
    pub fn url(&self, db: &str) -> String {
        let url = self.url_of(db);
        if !db.is_empty() && !db.contains('{') {
            quillo::my::Pool::open_as(&url, true).unwrap_or_else(|e| panic!("making {}: {}", db, e));
        }
        url
    }

    /// A database's URL, as it is.
    #[allow(dead_code)]
    pub fn url_of(&self, db: &str) -> String {
        format!("mysql://root@127.0.0.1:{}/{}", self.port, db)
    }
}

// ---- quillo run ------------------------------------------------------------

/// Stops `child` (a server a test started) once this test process is gone.
/// A test that panics drops what holds its servers, which kills them; one
/// that aborts (a stack overflow, say) runs no destructors, and its servers
/// would live on, holding the pipe `cargo test` reads the output from. A
/// small shell watches both, and leaves when the server has stopped.
pub fn stop_with_tests(child: &Child) {
    let watch = format!(
        "while kill -0 {test} 2>/dev/null && kill -0 {server} 2>/dev/null; do sleep 0.5; done; \
         kill -0 {test} 2>/dev/null || {{ kill -INT {server} 2>/dev/null; sleep 2; kill -KILL {server} 2>/dev/null; }}",
        test = std::process::id(),
        server = child.id()
    );
    // In the background of a shell that ends at once: the watcher is no
    // child of the tests', which would have to wait for it.
    let _ = Command::new("sh")
        .args(["-c", &format!("({}) </dev/null >/dev/null 2>&1 &", watch)])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A process a test started, killed when dropped (and when the test process
/// is gone: see `stop_with_tests`).
pub struct Guard(pub Child);

impl Guard {
    pub fn new(child: Child) -> Guard {
        stop_with_tests(&child);
        Guard(child)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A `quillo run` server, killed when dropped.
pub struct Server {
    child: Child,
    pub port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    /// Stops it at once, as a crash would.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn serve(file: &Path, env: &[(&str, &str)]) -> Server {
    serve_opts(file, env, &[])
}

/// `serve` without the static checks first: for what `quillo check` refuses, run anyway.
#[allow(dead_code)]
pub fn serve_unchecked(file: &Path) -> Server {
    serve_opts(file, &[], &["--no-check"])
}

/// `serve` with options of `quillo run`'s.
#[allow(dead_code)]
pub fn serve_with(file: &Path, opts: &[&str]) -> Server {
    serve_opts(file, &[], opts)
}

fn serve_opts(file: &Path, env: &[(&str, &str)], opts: &[&str]) -> Server {
    // The system picks the port (`--port 0`) and the server says which: a port picked here and
    // bound there could be taken in between by another process on a loaded machine, and then a
    // test's requests went to another test's server (FEEDBACK55.md, found running its slices;
    // branch feedback55). A test that names its own port or socket keeps it.
    // Under --split the services' own lines come first: those tests pick the port here as before.
    let own = opts.iter().any(|o| *o == "--port" || o.starts_with("--port=") || *o == "--listen" || o.starts_with("--listen=") || *o == "--split");
    let port = if own { free_port() } else { 0 };
    let log = std::env::var("QUILLO_TEST_LOG").is_ok();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string(), "--workers", "8"])
        .args(opts)
        .envs(env.iter().copied())
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if own { if log { Stdio::inherit() } else { Stdio::null() } } else { Stdio::piped() })
        .spawn()
        .expect("cannot start quillo");
    stop_with_tests(&child);
    let deadline = Instant::now() + Duration::from_secs(20);
    if own {
        let mut s = Server { child, port };
        while !listening(port) {
            if let Ok(Some(status)) = s.child.try_wait() {
                panic!("quillo run {} stopped: {}", file.display(), status);
            }
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        return s;
    }
    // Read the server's lines to the end, so its pipe never fills: the port from its first
    // "listening on", the rest shown under QUILLO_TEST_LOG.
    let (tx, rx) = std::sync::mpsc::channel();
    let err = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            if log {
                eprintln!("{}", line);
            }
            if let Some(url) = line.split("listening on ").nth(1) {
                let hostport = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("");
                if let Some(p) = hostport.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) {
                    let _ = tx.send(p);
                }
            }
        }
    });
    let mut s = Server { child, port: 0 };
    loop {
        if let Ok(p) = rx.recv_timeout(Duration::from_millis(20)) {
            s.port = p;
            break;
        }
        if let Ok(Some(status)) = s.child.try_wait() {
            panic!("quillo run {} stopped: {}", file.display(), status);
        }
        assert!(Instant::now() < deadline, "server did not start");
    }
    while !listening(s.port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    s
}

/// An HTTP answer: status, headers (lower case) and body.
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Answer {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Sends a request; `headers` as "name: value" lines.
pub fn send(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Answer {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    // One Host: the test's own when it gives one, as a request may have only one (FEEDBACK31.md,
    // Q-393; branch feedback27-35-fixes).
    let host = if headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) { "" } else { "Host: localhost\r\n" };
    let mut head = format!("{} {} HTTP/1.1\r\n{}Connection: close\r\nContent-Length: {}\r\n", method, path, host, body.len());
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) && !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let mut lines = head.lines();
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    Answer { status, headers, body: body.to_string() }
}

/// `db.exec_many` on any database (SQLite, PostgreSQL, MariaDB): rows past one statement's
/// share, none, an update a row at a time, a duplicate that writes nothing, a short row, and
/// a transaction around it that fails (branch feedback40-43). `EXEC_MANY_WANT` is its value.
pub const EXEC_MANY: &str = r#"
db.exec("create table many (id int primary key, name varchar(20))")
let a = db.exec_many("insert into many (id, name) values (?, ?)", for i in 0..2500 => [i, `n${i}`])
let b = db.exec_many("insert into many values (?, ?)", [])
let c = db.exec_many("update many set name = ? where id = ?", [["x", 1], ["ç", 2]])
let d = db.exec_many("insert into many values (?, ?)", [[5000, "a"], [1, "dup"]])
let e = db.exec_many("insert into many values (?, ?)", [[6000, "a"], [6001]])
let f = db.transaction(() => {
    db.exec_many("insert into many values (?, ?);", [[7000, "g"], [7001, "h"]])
    err("changed my mind")
})
let g = db.transaction(() => db.exec_many("  INSERT INTO many VALUES (?,?)  ", [[8000, "i"]]))
[a.changes, b.changes, c.changes, is_err(d), e.error, is_err(f), g.changes,
 db.one("select count(*) as n from many").n, db.one("select name from many where id = 2").name]
"#;

pub const EXEC_MANY_WANT: &str = r#"[2500, 0, 2, true, "db: row 2 has 1 value, the first has 2", true, 1, 2501, "ç"]"#;

/// `node` running `js`, from a file: Linux takes at most 128 KiB in one argument, and a script
/// with the runtime unminified (`QUILLO_RUNTIME_COVERAGE`, as CI runs) is more (found by CI).
#[allow(dead_code)]
pub fn node(js: &str) -> std::io::Result<std::process::Output> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let file = std::env::temp_dir().join(format!("quillo-node-{}-{}.cjs", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::write(&file, js)?;
    let out = std::process::Command::new("node").arg(&file).output();
    let _ = std::fs::remove_file(&file);
    out
}
