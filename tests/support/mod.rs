//! What the tests of transactions, idempotent requests and the ledger
//! share: throwaway PostgreSQL and MariaDB servers, `quillo run` servers,
//! and plain HTTP. Each is stopped when its value is dropped, even when a
//! test fails.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A port no one is listening on, and not given to another test here.
pub fn free_port() -> u16 {
    static GIVEN: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut given = GIVEN.lock().unwrap();
        if !given.contains(&port) {
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
    let s = Maria { child, dir, port };
    let deadline = Instant::now() + Duration::from_secs(30);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "mariadbd did not start: {}", std::fs::read_to_string(s.dir.join("err.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(s)
}

impl Maria {
    pub fn url(&self, db: &str) -> String {
        format!("mysql://root@127.0.0.1:{}/{}", self.port, db)
    }
}

// ---- quillo run ------------------------------------------------------------

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
    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string(), "--workers", "8"])
        .envs(env.iter().copied())
        .env("QUILLO_SECRET", "test secret for idempotency and ledger tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if std::env::var("QUILLO_TEST_LOG").is_ok() { Stdio::inherit() } else { Stdio::null() })
        .spawn()
        .expect("cannot start quillo");
    let mut s = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if let Ok(Some(status)) = s.child.try_wait() {
            panic!("quillo run {} stopped: {}", file.display(), status);
        }
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
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
    let mut head = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
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
