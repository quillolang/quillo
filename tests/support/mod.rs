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
    // Each server, and the one that makes its tables, keeps its temporary files in a folder of
    // its own: in the system's shared one, servers of other tests removed them, and the tables
    // were not made ("Could not remove temporary table ... Unknown table 'mysql.tmp_user_sys'"),
    // now and then under the full test run, in CI too. No option file is read, and what the
    // making says is kept for its failure (FEEDBACK56.md, found in CI; branch feedback56).
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).ok()?;
    let installed = Command::new(bin.join("mariadb-install-db"))
        .arg("--no-defaults")
        .arg(format!("--datadir={}", dir.join("data").display()))
        .arg(format!("--tmpdir={}", tmp.display()))
        .args(["--auth-root-authentication-method=normal", "--skip-test-db"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !installed.status.success() {
        // What the server said as it made the tables is in the data folder's logs.
        let mut logs = String::new();
        for e in std::fs::read_dir(dir.join("data")).into_iter().flatten().flatten() {
            let p = e.path();
            if matches!(p.extension().and_then(|x| x.to_str()), Some("err" | "log")) {
                logs += &format!("\n--- {}\n{}", p.display(), std::fs::read_to_string(&p).unwrap_or_default());
            }
        }
        panic!(
            "mariadb-install-db failed ({}): {}{}{}",
            installed.status,
            String::from_utf8_lossy(&installed.stdout),
            String::from_utf8_lossy(&installed.stderr),
            logs
        );
    }
    let child = Command::new(bin.join("mariadbd"))
        .arg("--no-defaults")
        .arg(format!("--datadir={}", dir.join("data").display()))
        .arg(format!("--port={}", port))
        .arg("--bind-address=127.0.0.1")
        .arg(format!("--socket={}", dir.join("s").display()))
        .arg(format!("--pid-file={}", dir.join("pid").display()))
        .arg(format!("--log-error={}", dir.join("err.log").display()))
        .arg(format!("--tmpdir={}", tmp.display()))
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
    /// Its process, for what a test measures of it (its memory, say).
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

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
    let own = port_given(opts);
    let port = if own { free_port() } else { 0 };
    let log = std::env::var("QUILLO_TEST_LOG").is_ok();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
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
    // In its Server first, so that a panic below stops it.
    let mut s = Server { child, port: 0 };
    s.port = port_in_stderr(&mut s.child, &file.display().to_string(), deadline);
    s
}

/// The port in a server's "listening on" line (`quillo: 2 routes listening on
/// http://127.0.0.1:4100`, with ` (2 instances)` after it under --instances, `FastCGI at
/// 127.0.0.1:4100` for `quillo fcgi`); none for a socket's path, or a line of a service's under
/// --split (`[billing] quillo: ...`). A server whose first load fails under --watch says none.
pub fn port_said(line: &str) -> Option<u16> {
    let url = line.strip_prefix("quillo: ")?.split("listening on ").nth(1)?;
    let url = url.strip_prefix("FastCGI at ").unwrap_or(url);
    let hostport = url.split("://").nth(1).unwrap_or(url).split(|c: char| c == '/' || c.is_whitespace()).next().unwrap_or("");
    hostport.rsplit(':').next().and_then(|p| p.parse::<u16>().ok())
}

/// The port `child`, a `quillo run --port 0` with its stderr piped, says it listens on, once it
/// listens there. Its lines are read to the end on a thread of their own, so the pipe never
/// fills, and shown under QUILLO_TEST_LOG. Panics, naming `what`, when the server stops first or
/// `deadline` passes: hold `child` where a panic stops it (a `Guard`, say) before calling.
pub fn port_in_stderr(child: &mut Child, what: &str, deadline: Instant) -> u16 {
    port_from_stderr(child, what, deadline, std::env::var("QUILLO_TEST_LOG").is_ok())
}

/// `port_in_stderr` with every line the server says shown, as a stderr it inherited showed them.
pub fn port_in_stderr_shown(child: &mut Child, what: &str, deadline: Instant) -> u16 {
    port_from_stderr(child, what, deadline, true)
}

fn port_from_stderr(child: &mut Child, what: &str, deadline: Instant, log: bool) -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    let err = child.stderr.take().expect("the server's stderr is piped");
    // What it said before it listened, for a server that stops or does not start.
    let said = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let kept = said.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut listening = false;
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            if log {
                eprintln!("{}", line);
            }
            if !listening {
                let mut k = kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                k.push_str(&line);
                k.push('\n');
            }
            if let Some(p) = port_said(&line) {
                listening = true;
                let _ = tx.send(p);
            }
        }
    });
    let said = || said.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let port = loop {
        if let Ok(p) = rx.recv_timeout(Duration::from_millis(20)) {
            break p;
        }
        if let Ok(Some(status)) = child.try_wait() {
            // Its last lines may still be on their way.
            std::thread::sleep(Duration::from_millis(50));
            panic!("quillo run {} stopped: {}\n{}", what, status, said());
        }
        assert!(Instant::now() < deadline, "server did not start\n{}", said());
    };
    while !listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    port
}

/// The port `child`, a `quillo run --port 0` whose stderr goes to the file `log`, says it listens
/// on, once it listens there: the system picks it, as `serve_opts` has it, where a port picked
/// here could be taken by another process before the server binds it (CI: feedback31's JSON body
/// answered by another server, prefetching's refused connection, feedback67's sink). Only whole
/// lines are read, as a line can be written in pieces. Panics with the log when `child` stops
/// first or `within` passes: hold `child` where a panic stops it (a `Guard`, say) before calling.
pub fn port_in_log(child: &mut Child, log: &Path, within: Duration) -> u16 {
    let deadline = Instant::now() + within;
    loop {
        if let Some(port) = port_said_in(log) {
            while !listening(port) {
                assert!(Instant::now() < deadline, "the server did not listen on {}:\n{}", port, std::fs::read_to_string(log).unwrap_or_default());
                std::thread::sleep(Duration::from_millis(5));
            }
            return port;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("quillo stopped: {}\n{}", status, std::fs::read_to_string(log).unwrap_or_default());
        }
        assert!(Instant::now() < deadline, "the server did not start:\n{}", std::fs::read_to_string(log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The port of the first whole "listening on" line in the file `log`, if there is one yet: for a
/// helper that waits in its own way (one that gives an error back when the server stops).
pub fn port_said_in(log: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(log).ok()?;
    text.split_inclusive('\n').filter(|l| l.ends_with('\n')).find_map(port_said)
}

/// The port `args` name (`--port N`, `--port=N`): a test gives one to a server that cannot say the
/// port the system picked (its first load fails under --watch).
pub fn port_named(args: &[&str]) -> Option<u16> {
    args.iter().enumerate().find_map(|(i, a)| match a.strip_prefix("--port") {
        Some("") => args.get(i + 1).and_then(|p| p.parse().ok()),
        Some(r) => r.strip_prefix('=').and_then(|p| p.parse().ok()),
        None => None,
    })
}

/// Whether a `quillo run` with `args` must be given its port, not `--port 0`: it names its own
/// (`--port`, `--listen`); a copy that joins (`--join`) or a member that registers (`--register`)
/// tells others the port it was given, and 0 is refused; under `--split` the services' own lines
/// come before the app's.
pub fn port_given(args: &[&str]) -> bool {
    args.iter().any(|o| {
        ["--port", "--listen", "--split", "--join", "--register"].iter().any(|f| o == f || o.strip_prefix(f).map_or(false, |r| r.starts_with('=')))
    })
}

/// The port `child`, a helper of a test's (a Node sink, a fake service) with its stdout piped,
/// says it listens on as its first line, once it listens there: the helper listens on port 0 and
/// prints the port it was given, where a port picked here could be taken by another process
/// before the helper bound it (CI run 44152090200: feedback67's push sink did not start). The
/// rest of its stdout is read to the end, so the pipe never fills. Panics, naming `what`, when the
/// helper stops first or `within` passes: hold `child` where a panic stops it (a `Guard`) first.
pub fn port_on_stdout(child: &mut Child, what: &str, within: Duration) -> u16 {
    let deadline = Instant::now() + within;
    let (tx, rx) = std::sync::mpsc::channel();
    let out = child.stdout.take().expect("the helper's stdout is piped");
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut lines = std::io::BufReader::new(out).lines().map_while(Result::ok);
        if let Some(first) = lines.next() {
            let _ = tx.send(first);
        }
        for _ in lines {}
    });
    let first = loop {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(20)) {
            break line;
        }
        if let Ok(Some(status)) = child.try_wait() {
            panic!("{} stopped: {}", what, status);
        }
        assert!(Instant::now() < deadline, "{} did not start", what);
    };
    let port: u16 = first.trim().parse().unwrap_or_else(|_| panic!("{} said {:?}, not its port", what, first));
    while !listening(port) {
        assert!(Instant::now() < deadline, "{} did not start", what);
        std::thread::sleep(Duration::from_millis(5));
    }
    port
}

/// Whether `pid` is among the processes listening on TCP `port`, as `lsof` says; true when there
/// is no `lsof` to ask. For a server that must take a port given before (started again where the
/// one before was): another process could have taken it in between, and the test would talk to
/// that one (branch security).
pub fn listener_is(port: u16, pid: u32) -> bool {
    match Command::new("lsof").args(["-nP", &format!("-iTCP:{}", port), "-sTCP:LISTEN", "-t"]).stdin(Stdio::null()).stderr(Stdio::null()).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).split_whitespace().any(|p| p == pid.to_string()),
        Err(_) => true,
    }
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
