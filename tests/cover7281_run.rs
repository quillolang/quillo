//! Coverage of feedback72-81 (85ebb05), slice "run": the lines no test ran of src/db.rs (readiness
//! with a database per tenant, FEEDBACK79.md Q-1254; SQLite closed on a clean stop, FEEDBACK80.md
//! Q-1260), src/interp.rs (an error value in markup, FEEDBACK81.md Q-1272; trusted CSS and a script's
//! runs, FEEDBACK79.md Q-1250 and Q-1274; a file in a form field, Q-1252; a deferred component that
//! listens to scrolls, FEEDBACK72.md Q-1010) and src/server.rs (a program with nothing to serve over
//! HTTP, its own socket's fallbacks, FEEDBACK72.md Q-1011).
//! Branch feedback72-81.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A folder of its own, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("c7281r-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        for (f, src) in files {
            if let Some(parent) = Path::new(f).parent() {
                std::fs::create_dir_all(d.join(parent)).unwrap();
            }
            std::fs::write(d.join(f), src).unwrap();
        }
        Dir(d)
    }

    fn file(&self, f: &str) -> PathBuf {
        self.0.join(f)
    }
}

/// The process's end within `within`, or None (it is then killed).
fn wait_for(c: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            return Some(s);
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// `quillo <args>` in `dir`, under a deadline: its exit code, stdout and stderr.
fn quillo_in(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("PORT")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read as it goes, so that a long output cannot fill the pipe and hold the process.
    let (mut so, mut se) = (c.stdout.take().unwrap(), c.stderr.take().unwrap());
    let t1 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let t2 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let status = wait_for(&mut c, Duration::from_secs(180)).unwrap_or_else(|| panic!("quillo {:?} took too long", args));
    (status.code(), t1.join().unwrap(), t2.join().unwrap())
}

/// `quillo check` takes `file` (warnings allowed).
fn checks(file: &Path) {
    let (code, out, err) = quillo_in(file.parent().unwrap(), &["check", file.to_str().unwrap()], &[]);
    assert_eq!(code, Some(0), "quillo check {}:\n{}{}", file.display(), out, err);
}

/// `src` as main.qwe, checked, then run as a script with `extra`: its exit code, stdout, stderr.
fn script(src: &str, extra: &[&str]) -> (Option<i32>, String, String) {
    let d = Dir::new("script", &[("main.qwe", src)]);
    checks(&d.file("main.qwe"));
    let mut args = vec!["run", "main.qwe"];
    args.extend_from_slice(extra);
    quillo_in(&d.0, &args, &[])
}

/// A running server, its log in a file, stopped as Ctrl-C does when dropped.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).stderr(Stdio::null()).status();
        }
        if wait_for(&mut self.child, Duration::from_secs(10)).is_none() {
            let _ = self.child.kill();
        }
        let _ = std::fs::remove_file(&self.log);
    }
}

impl Run {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn post(&self, path: &str, headers: &[(&str, &str)], body: &str) -> support::Answer {
        support::send(self.port, "POST", path, headers, body)
    }

    /// Signals it, and waits for its end; its exit status.
    fn stop(&mut self, signal: &str) -> std::process::ExitStatus {
        let _ = Command::new("kill").args([signal, &self.child.id().to_string()]).status();
        wait_for(&mut self.child, Duration::from_secs(120)).expect("the server did not stop")
    }
}

/// `quillo run file --port 0 extra`, checked first, its log in a file.
fn serve(file: &Path, extra: &[&str]) -> Run {
    static N: AtomicUsize = AtomicUsize::new(0);
    checks(file);
    let log = file.with_extension(format!("log{}", N.fetch_add(1, Ordering::SeqCst)));
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", "0"])
        .args(extra)
        .current_dir(file.parent().unwrap())
        .env("QUILLO_SECRET", "a test secret for cover7281_run, long enough")
        .env_remove("PORT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let mut r = Run { child, port: 0, log };
    r.port = support::port_in_log(&mut r.child, &r.log, Duration::from_secs(60));
    let deadline = Instant::now() + Duration::from_secs(60);
    while support::send(r.port, "GET", "/_quillo/health", &[], "").status != 200 {
        assert!(Instant::now() < deadline, "the server does not answer");
        std::thread::sleep(Duration::from_millis(20));
    }
    r
}

/// A program loaded in this process, in folder `d` (checked first).
fn app(d: &Dir, src: &str) -> Interp {
    std::fs::write(d.file("main.qwe"), src).unwrap();
    checks(&d.file("main.qwe"));
    let mut it = Interp::new(&d.0);
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}\n--- source ---\n{}", e, src);
    }
    it
}

/// A request to a program loaded in this process.
fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut all = vec![("Content-Type".to_string(), "application/json".to_string())];
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        all.push(("Host".to_string(), "localhost".to_string()));
    }
    all.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: all, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

/// The files of `d` a SQLite database keeps beside its own while open (`-wal`, `-shm`), sorted.
fn journals(d: &Dir) -> Vec<String> {
    let mut left: Vec<String> =
        std::fs::read_dir(&d.0).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with("-wal") || n.ends_with("-shm")).collect();
    left.sort();
    left
}

/// `select count(*) from table` of database `file` of `d`, read here (with its WAL, if one is
/// beside it); `alone`: of a copy of the file alone, as a restore by copying has it.
fn rows(d: &Dir, file: &str, table: &str, alone: bool) -> i64 {
    let path = if alone {
        let copy = d.file(&format!("copy-{}", file));
        std::fs::copy(d.file(file), &copy).unwrap();
        copy
    } else {
        d.file(file)
    };
    let c = rusqlite::Connection::open(&path).unwrap();
    let n = c.query_row(&format!("select count(*) from {}", table), [], |r| r.get(0)).unwrap();
    drop(c);
    if alone {
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), end));
        }
    }
    n
}

// ---- Q-1260: SQLite closed on a clean stop (src/db.rs close_all) ----------------------------

/// A connection a panic left poisoned (a bug of Quillo's while a request held it, which the server
/// answers 500 and lives on) is closed at a clean stop all the same: its `-wal` and `-shm` go, and
/// the file alone holds every row. The only test here that opens SQLite in this process, as
/// `close_all` closes every database the process has open (src/db.rs close_all, FEEDBACK80.md
/// Q-1260; coverage of feedback72-81; branch feedback72-81).
#[test]
fn a_connection_a_panic_poisoned_is_closed_on_a_clean_stop_all_the_same() {
    let src = "db.open(\"p.db\")\ndb.exec(\"create table t (x int)\")?\ndb.exec(\"insert into t values (1), (2), (3)\")?\n";
    let d = Dir::new("poisoned", &[]);
    let it = app(&d, src);
    assert_eq!(journals(&d), ["p.db-shm", "p.db-wal"], "WAL mode: its files while it is open");
    let b = match &*it.db.lock().unwrap() {
        Some(quillo::db::DbSlot::One(b)) => b.clone(),
        _ => panic!("the program's database is not one"),
    };
    let quillo::db::Backend::Sqlite(m) = &*b else { panic!("not SQLite") };
    let panicked = std::thread::scope(|s| {
        s.spawn(|| {
            let _held = m.lock().unwrap();
            panic!("a panic holding the connection, on purpose (cover7281_run)");
        })
        .join()
        .is_err()
    });
    assert!(panicked && m.is_poisoned());
    quillo::db::close_all(Duration::from_secs(5));
    assert_eq!(journals(&d), Vec::<String>::new());
    assert_eq!(rows(&d, "p.db", "t", true), 3);
    drop(it);
}

/// Two databases, the program's and one it connected to; a transaction that computes, and a query
/// that never ends.
const DRAIN: &str = r#"db.open("library.db")
db.exec("create table if not exists loans (id integer primary key, book text)")?
let other = db.connect("other.db")
other.exec("create table if not exists notes (id integer primary key, t text)")?
route POST /loan with db { db.exec("insert into loans (book) values ('a book')")?; "ok" }
route POST /note with db { other.exec("insert into notes (t) values ('ü ✓')")?; "ok" }
route POST /tx with db, fs {
    db.transaction(() => {
        db.exec("insert into loans (book) values ('in a transaction')")?
        write_file("tx.begun", "x")?
        let n = 0
        for i in 0..100000000000 { n = n + i % 7 }
        n
    })
}
route POST /forever with db, fs {
    write_file("forever.begun", "x")?
    db.one("with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) as n from c").n
}
"#;

/// A stop `--drain` cuts short: the program's database, in a transaction a request still computes
/// in, or held by a query still running past the 200 ms the close waits, is left as it is (its
/// `-wal` stays, and what the transaction had not committed is not the file's); the other database
/// is closed whole after it (src/db.rs close_all, FEEDBACK80.md Q-1260; coverage of feedback72-81;
/// branch feedback72-81).
#[test]
fn a_drain_cut_short_leaves_a_database_in_a_transaction_or_held_and_closes_the_rest() {
    for (path, begun) in [("/tx", "tx.begun"), ("/forever", "forever.begun")] {
        let d = Dir::new("drain", &[("main.qwe", DRAIN)]);
        let mut r = serve(&d.file("main.qwe"), &["--drain", "300ms"]);
        assert_eq!(r.post("/loan", &[], "").body, "ok");
        assert_eq!(r.post("/note", &[], "").body, "ok");
        let port = r.port;
        let busy = std::thread::spawn(move || support::send(port, "POST", path, &[], "").status);
        eventually(120, &format!("{} began", path), || d.file(begun).exists().then_some(()));
        let status = r.stop("-TERM");
        let _ = busy.join();
        assert!(status.success(), "{}: {}", path, r.log());
        assert!(r.log().contains(&format!("stopping with requests in progress: POST {}", path)), "{}", r.log());
        // Left open: its WAL stays; the other closed whole.
        assert_eq!(journals(&d), ["library.db-shm", "library.db-wal"], "{}: {}", path, r.log());
        assert_eq!(rows(&d, "other.db", "notes", true), 1, "{}", path);
        // Read with its WAL, the program's has the committed row, and not the transaction's.
        assert_eq!(rows(&d, "library.db", "loans", false), 1, "{}", path);
    }
}

// ---- Q-1254: readiness with a database per tenant (src/db.rs tenants_ready) ------------------

const TENANTS: &str = r#"tenant_from("localhost")
db.open(TARGET)
route POST /teams/:name = new_tenant(name, () => "made")
route GET / with db = `tenant ${tenant()}: ${len(db.query("select 1 as one"))} row`
"#;

fn tenants(target: &str) -> String {
    TENANTS.replace("TARGET", &serde_json::to_string(target).unwrap())
}

/// What `/_quillo/ready` says of the program's database: ready, and its check's word.
fn database_ready(it: &mut Interp) -> (bool, String) {
    let (ok, detail) = quillo::ready::report(it);
    (ok, detail["checks"]["database"].as_str().unwrap_or_else(|| panic!("{}", detail)).to_string())
}

/// `database_ready` of the tenants program on `target`, loaded in this process (nothing opened).
fn ready_of(target: &str) -> (bool, String) {
    let d = Dir::new("ready", &[]);
    let mut it = app(&d, &tenants(target));
    database_ready(&mut it)
}

/// PostgreSQL's tenants: with none open, whether the server takes a connection; once one is open,
/// whether the tenant used last answers `select 1`, which it does not once the server is stopped
/// (src/db.rs tenants_ready, FEEDBACK79.md Q-1254; coverage of feedback72-81; branch
/// feedback72-81).
#[test]
fn ready_per_tenant_on_postgres_asks_the_tenant_used_last() {
    let Some(pg) = support::postgres("c7281ready") else {
        eprintln!("no PostgreSQL here: skipped");
        return;
    };
    let d = Dir::new("pg", &[]);
    let mut it = app(&d, &tenants(&format!("{}?schema=t_{{tenant}}", pg.url)));
    assert_eq!(database_ready(&mut it), (true, "ready (per tenant)".to_string()));
    assert_eq!(send(&mut it, "POST", "/teams/acme", &[], "").text(), "made");
    assert_eq!(send(&mut it, "GET", "/", &[("Host", "acme.localhost")], "").text(), "tenant acme: 1 row");
    assert_eq!(database_ready(&mut it), (true, "ready (per tenant)".to_string()));
    // The server gone: the tenant open asks it, and it does not answer.
    drop(pg);
    assert_eq!(database_ready(&mut it), (false, "down".to_string()));
    assert_eq!(send(&mut it, "GET", "/", &[("Host", "acme.localhost")], "").status, 500);
}

/// A server's host as IPv6 (`[::1]:port`, or `[::1]` and `?port=`): ready while something listens
/// there, down once nothing does; a host name no lookup finds, and a socket's path longer than a
/// socket's address holds (which libpq cannot reach either), are down at once; a path of 103 bytes
/// is still reached. Each URL is asked once, as an answer is kept a while (src/db.rs server_reach,
/// Reach::reachable and unix_connects, FEEDBACK79.md Q-1254; coverage of feedback72-81; branch
/// feedback72-81).
#[test]
fn ready_per_tenant_reads_an_ipv6_host_and_says_down_for_what_it_cannot_reach() {
    match std::net::TcpListener::bind("[::1]:0") {
        Ok(l) => {
            let port = l.local_addr().unwrap().port();
            for url in [format!("postgres://v6@[::1]:{}/app_{{tenant}}", port), format!("postgres://v6@[::1]/app_{{tenant}}?port={}", port)] {
                assert_eq!(ready_of(&url), (true, "ready (per tenant)".to_string()), "{}", url);
            }
            drop(l);
            // Refused now (another test could take the port in between, a race this accepts).
            assert_eq!(ready_of(&format!("postgres://v6-gone@[::1]:{}/app_{{tenant}}", port)), (false, "down".to_string()));
        }
        Err(e) => eprintln!("no [::1] here ({}): the IPv6 part skipped", e),
    }
    assert_eq!(ready_of("postgres://q@no-such-host.invalid:5432/app_{tenant}"), (false, "down".to_string()));
    // A folder as the host, its socket past `sun_path` (104 bytes on macOS, 108 on Linux).
    let long = format!("/tmp/c7281-{}", "s".repeat(110));
    assert_eq!(ready_of(&format!("postgres://app@{}/app_{{tenant}}", long.replace('/', "%2F"))), (false, "down".to_string()));
    assert_eq!(ready_of(&format!("postgres:///app_{{tenant}}?host={}", long)), (false, "down".to_string()));
    // At the edge: a socket's path of 103 bytes, listened on, is ready; one of 104, which macOS
    // cannot hold, is down.
    #[cfg(unix)]
    {
        let mut dir = format!("/tmp/c7281b-{}-", std::process::id());
        while dir.len() < 89 {
            dir.push('b');
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = format!("{}/.s.PGSQL.5432", dir);
        assert_eq!(sock.len(), 103);
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        assert_eq!(ready_of(&format!("postgres://edge@{}/app_{{tenant}}", dir.replace('/', "%2F"))), (true, "ready (per tenant)".to_string()));
        assert_eq!(ready_of(&format!("postgres://edge@{}x/app_{{tenant}}", dir.replace('/', "%2F"))), (false, "down".to_string()));
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A program with no base folder (`Interp::new("")`, as `quillo::eval_str` makes one) has its
/// tenants' files made in the working folder: ready, as with that folder given, for `{tenant}.db`
/// and for a folder not there yet under it; a folder under one this process may not write in is
/// down, as before. Nothing is made (src/db.rs tenants_folder_ready, an empty base; FEEDBACK79.md
/// Q-1254, found by the coverage tests of feedback72-81; branch feedback72-81).
#[test]
fn ready_per_tenant_with_no_base_folder_looks_in_the_working_folder() {
    let ready_in = |base: &str, target: &str| {
        let src = tenants(target);
        let mut it = Interp::new(base);
        if let Err(e) = it.load_source(&src, "main.qwe") {
            panic!("{}\n{}", e, src);
        }
        database_ready(&mut it)
    };
    let missing = format!("c7281-none-{}", std::process::id());
    assert!(!Path::new(&missing).exists());
    for target in ["{tenant}.db".to_string(), format!("{}/{{tenant}}.db", missing), format!("{}/deeper/t-{{tenant}}.db", missing)] {
        assert_eq!(ready_in("", &target), (true, "ready (per tenant)".to_string()), "{}", target);
        assert_eq!(ready_in(".", &target), (true, "ready (per tenant)".to_string()), "{}", target);
    }
    assert!(!Path::new(&missing).exists(), "ready made a folder");
    // SAFETY: geteuid has no preconditions and cannot fail.
    #[cfg(unix)]
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(ready_in("", "/c7281-not-writable/{tenant}.db"), (false, "down".to_string()));
    }
}

/// A server's URL the driver would not read is down, without asking anyone: an IPv6 host with no
/// `]` closing it, something after its `]`, a port that is no number after a host (an IPv6 one or
/// not, `notaport`, 65536) or as `?port=`, and a later host's bad port. Each is given `?port=` of a
/// port listened on, which 85ebb05 asked (or took for ready without asking): one either side, the
/// port 65535 is a number, and an empty port is the default, so those are asked, and ready while
/// listened on (src/db.rs server_reach and server_ready; FEEDBACK79.md Q-1254, found by the coverage
/// tests of feedback72-81; branch feedback72-81).
#[test]
fn ready_per_tenant_says_down_for_a_url_the_driver_would_not_read() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let down = (false, "down".to_string());
    for url in [
        format!("postgres://q@127.0.0.1:notaport/app_{{tenant}}?port={}", port),
        format!("postgres://q@127.0.0.1:65536/app_{{tenant}}?port={}", port),
        format!("postgres://q@127.0.0.1/app_{{tenant}}?port=x{}", port),
        format!("postgres://q@127.0.0.1:{},127.0.0.1:é/app_{{tenant}}", port),
        format!("postgres://q@[::1/app_{{tenant}}?port={}", port),
        format!("postgres://q@[::1]x/app_{{tenant}}?port={}", port),
        format!("postgres://q@[::1]:notaport/app_{{tenant}}?port={}", port),
    ] {
        assert_eq!(ready_of(&url), down, "{}", url);
    }
    for url in [format!("postgres://q@127.0.0.1:{}/app_{{tenant}}", port), format!("postgres://q@127.0.0.1:/app_{{tenant}}?port={}", port), format!("postgres://q@127.0.0.1/app_{{tenant}}?port={}&schema=t_{{tenant}}", port)] {
        assert_eq!(ready_of(&url), (true, "ready (per tenant)".to_string()), "{}", url);
    }
    // 65535 is a port: asked, and down while nothing listens there (another program may: then
    // ready, which this accepts).
    let (_, said) = ready_of("postgres://q@127.0.0.1:65535/app_{tenant}");
    assert!(said == "down" || said == "ready (per tenant)", "{}", said);
    match std::net::TcpListener::bind("[::1]:0") {
        Ok(l6) => {
            let p6 = l6.local_addr().unwrap().port();
            assert_eq!(ready_of(&format!("postgres://q@[::1]:{}/app_{{tenant}}", p6)), (true, "ready (per tenant)".to_string()));
            assert_eq!(ready_of(&format!("postgres://q@[::1]:/app_{{tenant}}?port={}", p6)), (true, "ready (per tenant)".to_string()));
        }
        Err(e) => eprintln!("no [::1] here ({}): the IPv6 part skipped", e),
    }
    drop(l);
}

// ---- Q-1055: a script's clock (src/script.rs run_script_for) ---------------------------------

/// How long `src` takes in this process, on a thread where no script has run yet, the first time
/// and the second.
fn first_and_again(src: &'static str) -> (Duration, Duration) {
    std::thread::spawn(move || {
        let mut took = [Duration::ZERO; 2];
        for t in took.iter_mut() {
            let at = Instant::now();
            let v = quillo::eval_str(src).unwrap_or_else(|e| panic!("{}", e));
            *t = at.elapsed();
            assert_eq!(v.repr(), "1", "{}", src);
        }
        (took[0], took[1])
    })
    .join()
    .unwrap()
}

/// The first script on a thread has all of its time: its clock starts once its world is there,
/// which that script makes, with every builtin Quillo has; 85ebb05 counted making them against the
/// script's 100 ms, so in a slow build the first script calling a helper (whose return looks at the
/// clock) stopped before it began (tests/scripts.rs `scripts_compute_from_what_they_are_given` on
/// CI's debug build). Given half of what making the world takes here as its timeout, the first
/// script on a new thread still answers, at one millisecond the least; a script that runs past its
/// time still stops (src/script.rs run_script_for; FEEDBACK75.md Q-1055, found by the coverage
/// tests of feedback72-81; branch feedback72-81).
#[test]
fn the_first_script_on_a_thread_has_all_of_its_time() {
    const PLAIN: &str = "fn h() = 1\nrun_script(\"h()\", { h: h }, { timeout: 60000 })";
    // What making a script's world takes: the first script on a thread less the next.
    let (first, again) = (0..3).map(|_| first_and_again(PLAIN)).min_by_key(|(f, a)| f.saturating_sub(*a)).unwrap();
    let world = first.saturating_sub(again);
    let ms = (world.as_millis() / 2).clamp(1, 60_000) as u64;
    eprintln!("making the world: {:?} (first {:?}, again {:?}); timeout {} ms", world, first, again, ms);
    let src = format!("fn h() = 1\nrun_script(\"h()\", {{ h: h }}, {{ timeout: {} }})", ms);
    let said = std::thread::spawn(move || quillo::eval_str(&src).map(|v| v.repr()).unwrap_or_else(|e| e.to_string())).join().unwrap();
    assert_eq!(said, "1", "the world took {:?}, the timeout {} ms", world, ms);
    let slow = std::thread::spawn(|| quillo::eval_str("fn h() = 1\nrun_script(\"let n = 0\\nwhile true { n = n + h() }\", { h: h }, { timeout: 1 })").map(|v| v.repr()).unwrap_or_else(|e| e.to_string())).join().unwrap();
    assert!(slow.contains("the script took longer than its 1 ms") || slow.contains("too many steps"), "{}", slow);
}

// ---- Q-1272: an error value in markup shows nothing (src/interp.rs) -------------------------

/// An error shows nothing, and is said once a render with where it was made: one a view's `?` gave
/// back part-way (what the view wrote before it taken back; a second view's error in the same
/// render not said again), the left side of `&&` (an error is false) and of `||` (a list holding one is
/// true), and in SVG's foreign content a value alone and one in a list with markup; `||`'s
/// fallback still shows (src/interp.rs render_call_plain, render_expr_into, render_value_ns,
/// FEEDBACK81.md Q-1272; coverage of feedback72-81; branch feedback72-81).
#[test]
fn an_error_in_markup_shows_nothing_wherever_it_comes_from() {
    let (code, out, err) = script(
        r#"view Row(x: str) = <li class="r">{int(x)?}</li>
view Row2(x: str) {
    let n = int(x)?
    <li>{n}</li>
}
fn gone() = err("gone ✓")
print(<ul><Row x="1"/><Row x="zz"/><Row2 x="2"/><Row2 x="yy"/></ul>)
print(<p>{gone() && "y"}</p>)
print(<p>{[gone()] || "y"}</p>)
print(<p>{false || "fallback"}</p>)
print(<svg><text>{gone()}</text></svg>)
print(<svg>{[<g/>, gone()]}</svg>)
"#,
        &[],
    );
    assert_eq!(code, Some(0), "{}{}", out, err);
    assert_eq!(out, "<ul><li class=\"r\">1</li><li>2</li></ul>\n<p></p>\n<p></p>\n<p>fallback</p>\n<svg><text></text></svg>\n<svg><g></g></svg>\n", "{}", err);
    let said: Vec<&str> = err.lines().filter(|l| l.contains("an error no statement took")).collect();
    let tail = "(give a value for it with `??`, or show something else with `is_err` or `match`)";
    assert_eq!(
        said,
        [
            format!("quillo: error: main.qwe:7: an error no statement took, made at main.qwe:1, shown as nothing: not an int: \"zz\" {}", tail),
            format!("quillo: error: main.qwe:8: an error no statement took, made at main.qwe:6, shown as nothing: gone ✓ {}", tail),
            format!("quillo: error: main.qwe:9: an error no statement took, made at main.qwe:6, shown as nothing: gone ✓ {}", tail),
            format!("quillo: error: main.qwe:11: an error no statement took, made at main.qwe:6, shown as nothing: gone ✓ {}", tail),
            format!("quillo: error: main.qwe:12: an error no statement took, made at main.qwe:6, shown as nothing: gone ✓ {}", tail),
        ],
        "{}",
        err
    );
}

// ---- Q-1250: trusted CSS (src/interp.rs) ----------------------------------------------------

/// Trusted CSS added to a decimal, on either side, is plain text, as with any other number; and a
/// program's helper a script calls gives a builtin trusted CSS as the plain text it is, so that
/// `str` of it in a style is one value there as outside the script (src/interp.rs
/// binop's decimal arm and plain_args, FEEDBACK79.md Q-1250; coverage of feedback72-81; branch
/// feedback72-81).
#[test]
fn trusted_css_and_a_decimal_add_as_text_and_a_scripts_builtins_get_plain_text() {
    let (code, out, err) = script(
        r#"let C = css`red; position: fixed`
fn up() = upper(C)
fn size() = len(C)
fn shown() = str(<p style={{ color: str(C) }}>x</p>)
print(css`a` + 1.5m)
print(1.5m + css`b`)
print(type_of(css`a` + 0.25m))
print(css`` + 1m)
print(run_script("up()", { up: up }))
print(type_of(run_script("up()", { up: up })))
print(run_script("size()", { size: size }))
print(<p style={{ color: run_script("up()", { up: up }) }}>x</p>)
print(shown())
print(run_script("shown()", { shown: shown }))
"#,
        &[],
    );
    assert_eq!(code, Some(0), "{}{}", out, err);
    assert_eq!(out, "a1.5\n1.5b\nstr\n1\nRED; POSITION: FIXED\nstr\n20\n<p style=\"color: RED\\3b  POSITION: FIXED\">x</p>\n<p style=\"color: red\\3b  position: fixed\">x</p>\n<p style=\"color: red\\3b  position: fixed\">x</p>\n", "{}", err);
}

/// A style template's value past `--max-memory` as it is written: an error holding a list, and a
/// list, whose text is larger than the limit, stop the request with the limit's words, at the
/// template's line; under it, or with no limit, the page is written (src/interp.rs css_other_into,
/// FEEDBACK79.md Q-1250; coverage of feedback72-81; branch feedback72-81).
#[test]
fn a_style_value_past_max_memory_stops_as_any_value_does() {
    for (value, words) in [("err(l)", "an error"), ("l", "a list")] {
        let src = format!("let s = repeat(\"x\", 300000)\nlet l = [s, s, s, s]\nlet one = [s]\nprint(len(str(<p style={{`color: ${{{}}}`}}>e</p>)))\n", value);
        let (code, out, err) = script(&src, &["--max-memory", "1MB"]);
        assert_eq!(code, Some(1), "{}: {}{}", words, out, err);
        assert!(err.contains("main.qwe:4: this request's value would be bigger than --max-memory allows (1048576 bytes)"), "{}: {}", words, err);
        assert_eq!(out, "", "{}", words);
        // No limit: written.
        let (code, out, err) = script(&src, &[]);
        assert_eq!(code, Some(0), "{}: {}{}", words, out, err);
        assert!(out.trim().parse::<usize>().unwrap() > 1_200_000, "{}: {}", words, out);
        // Under the limit: written.
        let (code, out, err) = script(&src.replace("${l}", "${one}").replace("err(l)", "err(one)"), &["--max-memory", "1MB"]);
        assert_eq!(code, Some(0), "{}: {}{}", words, out, err);
        assert!(out.trim().parse::<usize>().unwrap() > 300_000, "{}: {}", words, out);
    }
}

/// A program's own `css` function: css`...` calls it as any tagged template calls its function,
/// and what it gives is plain text, which a style takes as one value; `check` says it hides the
/// builtin (src/interp.rs css_tagged, FEEDBACK79.md Q-1250; coverage of feedback72-81; branch
/// feedback72-81).
#[test]
fn a_programs_own_css_function_is_called_and_trusts_nothing() {
    let (code, out, err) = script(
        r#"fn css(s: str) -> str = upper(s)
print(css`color: red`)
print(type_of(css`a`))
print(css``)
print(<p style={css`color: ${"blue; x: 1"}`}>x</p>)
"#,
        &[],
    );
    assert_eq!(code, Some(0), "{}{}", out, err);
    assert_eq!(out, "COLOR: RED\nstr\n\n<p style=\"COLOR: BLUE\\3B  X: 1\">x</p>\n", "{}", err);
    assert!(err.contains("main.qwe:1: warning: `css` hides the builtin `css`"), "{}", err);
}

/// A program's helper a script calls writes a component's style as it does outside a script: the
/// style template at its tag, whose values the template escaped, a literal style there, a style
/// map there, and css`...` in the helper are trusted CSS, the program's, so a visitor's `;` given
/// to it stays inside one value, escaped once, and the program's own declarations stay as written;
/// unicode too. What the helper gives the script is plain text there, as before. (src/interp.rs
/// eval_unary's UnOp::Css and css_tagged, FEEDBACK79.md Q-1250 and Q-1274; coverage of
/// feedback72-81; branch feedback72-81)
///
/// Changed with the fix of what this test found (FEEDBACK79.md, Q-1250, found by the coverage
/// tests of feedback72-81): at 85ebb05 the helper's style in a script was text, which its component
/// escaped again (`color: red\5c 3b  top: 0`), and a literal style lost its declarations
/// (`red\3b  top: 1px`), where the same call outside a script wrote them once; the test asserted
/// only that the `;` stayed in one value. Trust is now decided by where the code was written.
#[test]
fn a_helpers_component_style_in_a_script_is_as_outside_one() {
    let (code, out, err) = script(
        r#"view Box(style, label) = <div style={`padding: 8px; ${style}`}>{label}</div>
fn w(x) = str(<Box style={`color: ${x}`} label="t"/>)
fn lit() = str(<Box style="color: red; top: 1px" label="l"/>)
fn tagged(x) = str(<Box style={css`color: ${x}; top: 2px`} label="c"/>)
fn kept(x) = css`color: ${x}; top: 3px`
print(w("red; top: 0"))
print(run_script("w(v)", { w: w, v: "red; top: 0" }))
print(w("rouge; fond: été"))
print(run_script("w(v)", { w: w, v: "rouge; fond: été" }))
print(w(""))
print(run_script("w(v)", { w: w, v: "" }))
print(lit())
print(run_script("l()", { l: lit }))
print(tagged("red; x: 1"))
print(run_script("t(v)", { t: tagged, v: "red; x: 1" }))
print(<Box style={kept("red; x: 1")} label="k"/>)
print(<Box style={run_script("k(v)", { k: kept, v: "red; x: 1" })} label="k"/>)
"#,
        &[],
    );
    assert_eq!(code, Some(0), "{}{}", out, err);
    let lines: Vec<&str> = out.lines().collect();
    let twice = |a: &'static str| [a, a];
    let want: Vec<&str> = [
        twice(r#"<div style="padding: 8px; color: red\3b  top: 0">t</div>"#),
        twice(r#"<div style="padding: 8px; color: rouge\3b  fond: été">t</div>"#),
        twice(r#"<div style="padding: 8px; color: ">t</div>"#),
        twice(r#"<div style="padding: 8px; color: red; top: 1px">l</div>"#),
        twice(r#"<div style="padding: 8px; color: red\3b  x: 1; top: 2px">c</div>"#),
    ]
    .concat();
    assert_eq!(lines[..10], want[..], "{}", out);
    // Given back to the program from the script: text, which the style escapes as one value.
    assert_eq!(lines[10], r#"<div style="padding: 8px; color: red\3b  x: 1; top: 3px">k</div>"#, "{}", out);
    assert_eq!(lines[11], r#"<div style="padding: 8px; color: red\5c 3b  x: 1\3b  top: 3px">k</div>"#, "{}", out);
}

/// A script's own css`...` makes text, never trusted CSS, as the parser writes it for a script:
/// given back, given to a helper of the program's, handed back by a callback, kept by a helper, in
/// a list, or added to, each is written by a style as one value; empty and unicode too; outside a
/// script the same css`...` is trusted. (src/parser.rs, a tagged template in a script's source;
/// FEEDBACK79.md Q-1250, found by the coverage tests of feedback72-81; branch feedback72-81)
#[test]
fn a_scripts_own_css_is_text_wherever_it_goes() {
    let (code, out, err) = script(
        r#"view Box(style) = <p style={`a: 0; ${style}`}>x</p>
let kept = []
fn show(s) = str(<Box style={s}/>)
fn back(f) = str(<Box style={f()}/>)
fn keep(s) {
    kept = push(kept, s)
    return 1
}
print(show(css`b: 1; c: 2`))
print(<Box style={run_script("css`b: 1; c: 2`", { css: css })}/>)
print(run_script("show(css`b: 1; c: 2`)", { show: show, css: css }))
print(run_script("back(() => css`b: 1; c: 2`)", { back: back, css: css }))
print(run_script("keep(css`b: 1; c: 2`)", { keep: keep, css: css }))
print(show(kept[0]))
print(<Box style={run_script("[css`b: 1; c: 2`][0]", { css: css })}/>)
print(<Box style={run_script("css`b: ${v}`", { css: css, v: "é; c: 2" })}/>)
print(<Box style={run_script("css``", { css: css })}/>)
print(run_script("type_of(css`b`) + len(css`bé`)", { css: css }))
"#,
        &[],
    );
    assert_eq!(code, Some(0), "{}{}", out, err);
    let one = r#"<p style="a: 0; b: 1\3b  c: 2">x</p>"#;
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            r#"<p style="a: 0; b: 1; c: 2">x</p>"#,
            one,
            one,
            one,
            "1",
            one,
            one,
            r#"<p style="a: 0; b: é\5c 3b  c: 2">x</p>"#,
            r#"<p style="a: 0; ">x</p>"#,
            "str2",
        ],
        "{}",
        out
    );
}

/// A global map's field a live loader follows, changed from text to trusted CSS of the same text
/// and back, is changed each time (a style writes the two apart); the same trusted CSS again, or
/// the same text again, is no change (src/interp.rs changed_fields, FEEDBACK79.md Q-1250; coverage
/// of feedback72-81; branch feedback72-81).
#[test]
fn a_followed_field_from_text_to_the_same_trusted_css_is_a_change() {
    let d = Dir::new("followed", &[]);
    let mut it = app(
        &d,
        r#"let world = { name: "w", n: 1 }
server fn load() = world.name
server fn to_css() { world = { ...world, name: css`w` } }
server fn to_text() { world = { ...world, name: "w" } }
"#,
    );
    let rpc = |it: &mut Interp, name: &str, live: bool| -> serde_json::Value {
        let r = send(it, "POST", &format!("/_quillo/rpc/{}", name), if live { &[("X-Quillo-Live", "1")] } else { &[] }, "[]");
        serde_json::from_str(&r.text()).unwrap_or_else(|e| panic!("{}: {}", e, r.text()))
    };
    let live = |it: &mut Interp, since: u64, topics: &str| -> serde_json::Value {
        let r = send(it, "GET", &format!("/_quillo/live?since={}&t={}", since, topics), &[], "");
        serde_json::from_str(&r.text()).unwrap_or_else(|e| panic!("{}: {}", e, r.text()))
    };
    let r = rpc(&mut it, "load", true);
    assert_eq!((r["ok"].clone(), r["topics"].clone()), (serde_json::json!("w"), serde_json::json!(["var:world.name"])), "{}", r);
    let mut changed = |it: &mut Interp, f: &str| {
        let since = live(it, 0, "")["seq"].as_u64().unwrap();
        rpc(it, f, false);
        live(it, since, "var:world.name,var:world.n")["changed"].clone()
    };
    assert_eq!(changed(&mut it, "to_css"), serde_json::json!(["var:world.name"]));
    assert_eq!(changed(&mut it, "to_css"), serde_json::json!([]));
    assert_eq!(changed(&mut it, "to_text"), serde_json::json!(["var:world.name"]));
    assert_eq!(changed(&mut it, "to_text"), serde_json::json!([]));
}

// ---- Q-1010: a deferred component that listens to scrolls (src/interp.rs render_deferred) ----

const DEFERRED: &str = r#"server fn load() -> list = ["a", "b"]
component Rows(xs: list) {
    state r = 7
    {for x in xs => <tr key={x}><td>{x} {r}</td></tr>}
}
component Feed() {
    state items = defer(load())
    state at = 0
    <div id="feed" @scroll={() => { at += 1 }}><p>{len(items ?? [])} {at}</p></div>
}
component Slow() {
    state rows = defer(load())
    state at = 0
    <div @scroll={() => { at += 1 }}><table><tbody><Rows xs={rows ?? []}/></tbody></table><p>{at}</p></div>
}
component Still() {
    state items = defer(load())
    <p>{len(items ?? [])}</p>
}
route GET / = <html><body><Feed/></body></html>
route GET /rows = <html><body><Slow/></body></html>
route GET /still = <html><body><Still/></body></html>
"#;

/// A component that waits for a deferred state and listens to scrolls says so in the part that
/// comes later too (`data-sc`), with `data-qm` before it when it holds components marked by
/// comments (table rows); one that does not listen says neither (src/interp.rs render_deferred,
/// FEEDBACK72.md Q-1010; coverage of feedback72-81; branch feedback72-81).
#[test]
fn a_deferred_part_says_it_listens_to_scrolls() {
    let d = Dir::new("deferred", &[]);
    let mut it = app(&d, DEFERRED);
    let island = |body: &str, from: &str| -> String {
        let at = body.find(from).unwrap_or_else(|| panic!("no {} in {}", from, body));
        let open = &body[at..];
        let island = &open[open.find("<quillo-island").unwrap_or_else(|| panic!("{}", open))..];
        island[..island.find('>').unwrap() + 1].to_string()
    };
    let a = send(&mut it, "GET", "/", &[], "");
    let body = a.text();
    assert_eq!(a.status, 200, "{}", body);
    assert!(island(&body, "<body>").ends_with(" data-qd=\"0\" data-sc>"), "{}", body);
    let part = island(&body, "<template data-qt=\"0\">");
    assert!(part.ends_with("data-sc>") && !part.contains("data-qm"), "{}", body);
    assert!(body.contains("<p>2 0</p>"), "{}", body);
    let body = send(&mut it, "GET", "/rows", &[], "").text();
    assert!(island(&body, "<template data-qt=\"0\">").ends_with(" data-qm data-sc>"), "{}", body);
    assert!(body.contains("<td>a 7</td>") && body.contains("<td>b 7</td>"), "{}", body);
    let body = send(&mut it, "GET", "/still", &[], "").text();
    let part = island(&body, "<template data-qt=\"0\">");
    assert!(!part.contains("data-sc") && !part.contains("data-qm"), "{}", body);
    assert!(body.contains("<p>2</p>"), "{}", body);
}

// ---- Q-1252: a file in a form field (src/interp.rs files_in_form) ---------------------------

const UPLOAD: &str = r#"route POST /a { form: { name: str = "(none)" } } = `name is ${name}`
route POST /n { form: { n: int | null = 7 } } = `n is ${n}`
"#;

/// A multipart body of `parts`: (field, file name or none, content).
fn multipart(parts: &[(&str, Option<&str>, &str)]) -> (String, String) {
    let b = "XqUiLlObOuNdArY7281";
    let mut body = String::new();
    for (field, file, content) in parts {
        body.push_str(&format!("--{}\r\n", b));
        match file {
            Some(f) => body.push_str(&format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: text/plain\r\n\r\n", field, f)),
            None => body.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", field)),
        }
        body.push_str(content);
        body.push_str("\r\n");
    }
    body.push_str(&format!("--{}--\r\n", b));
    (format!("multipart/form-data; boundary={}", b), body)
}

/// Several files under the name of a field the form pattern names: the first is said, by its
/// name; a field whose pattern is no one type says it is not what its pattern takes (src/interp.rs
/// file_in_form and files_in_form, FEEDBACK79.md Q-1252; coverage of feedback72-81; branch
/// feedback72-81).
#[test]
fn several_files_in_a_form_field_say_the_first_and_a_patterns_words() {
    let d = Dir::new("upload", &[("main.qwe", UPLOAD)]);
    let r = serve(&d.file("main.qwe"), &[]);
    let upload = |path: &str, parts: &[(&str, Option<&str>, &str)]| {
        let (ctype, body) = multipart(parts);
        r.post(path, &[("Content-Type", &ctype)], &body).body
    };
    assert_eq!(upload("/a", &[("name", Some("one ✓.txt"), "1"), ("name", Some("two.txt"), "2")]), "name is (none)");
    assert_eq!(upload("/n", &[("n", Some("n.txt"), "5")]), "n is 7");
    assert_eq!(upload("/n", &[("n", None, "5")]), "n is 5");
    let said = eventually(60, "the two warnings logged", || {
        let l: Vec<String> = r.log().lines().filter(|l| l.contains("is a file")).map(str::to_string).collect();
        (l.len() >= 2).then_some(l)
    });
    assert!(said[0].ends_with("`form.name` is a file (one ✓.txt), not a str; it is in request.files.name (the route ran with its default)"), "{:?}", said);
    assert!(said[1].ends_with("`form.n` is a file (n.txt), not what its pattern takes; it is in request.files.n (the route ran with its default)"), "{:?}", said);
    assert_eq!(said.len(), 2, "{}", r.log());
}

// ---- Q-1011: no port, and no socket of its own either (src/server.rs serve_portless) --------

/// The TCP addresses `pid` listens on, as lsof says; None without lsof.
fn tcp_listening(pid: u32) -> Option<Vec<String>> {
    let o = Command::new("lsof").args(["-nP", "-a", "-p", &pid.to_string(), "-iTCP", "-sTCP:LISTEN", "-Fn"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.strip_prefix('n').map(str::to_string)).collect())
}

/// Whether `pid` has a Unix socket of its own (`quillo-self-*`), as lsof says; None without lsof.
fn has_own_socket(pid: u32) -> Option<bool> {
    let o = Command::new("lsof").args(["-nP", "-a", "-p", &pid.to_string(), "-U", "-Fn"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).lines().any(|l| l.contains("quillo-self-")))
}

/// A program with nothing to serve over HTTP, the default port taken, under a umask that takes
/// rights from the folder it makes for its own socket (the owner's search right, 100; all of them,
/// 777): its folder is its owner's alone all the same (mode 700), so it serves itself on its own
/// socket and on no TCP port, says nothing of where, its ticks still reach it, and once SIGTERM has
/// stopped it cleanly its folder is gone; under 777 0.8.9 left a folder of mode 000 behind at each
/// start (src/server.rs serve_portless, FEEDBACK72.md Q-1011, found by the coverage tests of
/// feedback72-81; branch feedback72-81).
///
/// Changed with that fix (FEEDBACK72.md, Q-1011, found by the coverage tests of feedback72-81):
/// this test was `with_no_socket_of_its_own_a_program_that_serves_nothing_takes_a_port_of_its_own`,
/// which, under `umask 100`, saw the program fall back to a port of 127.0.0.1 because its socket's
/// folder had lost its search right; the folder's mode is now set whatever the umask, so that
/// fallback is left for a system where no folder can be made, and the program keeps its own socket.
#[cfg(unix)]
#[test]
fn under_any_umask_a_program_that_serves_nothing_keeps_its_own_socket() {
    // Root makes its socket in any folder: nothing to see then.
    // SAFETY: geteuid has no preconditions and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("running as root: skipped");
        return;
    }
    // Held here, or by anyone else (another program on the default port).
    let _held = std::net::TcpListener::bind("127.0.0.1:8080").ok();
    for mask in ["100", "777"] {
        let d = Dir::new("portless", &[("main.qwe", "every(100, () => { print(\"tick ✓\") })\n")]);
        checks(&d.file("main.qwe"));
        // A temporary folder short enough for a socket's path in it, so that one is made there.
        let tmp = Dir(PathBuf::from(format!("/tmp/c7281p-{}-{}", std::process::id(), mask)));
        let _ = std::fs::remove_dir_all(&tmp.0);
        std::fs::create_dir_all(&tmp.0).unwrap();
        let log = d.file("out.txt");
        let out = std::fs::File::create(&log).unwrap();
        let quillo = env!("CARGO_BIN_EXE_quillo");
        let child = Command::new("sh")
            .args(["-c", &format!("umask {}; exec \"$0\" run main.qwe", mask), quillo])
            .current_dir(&d.0)
            .env("TMPDIR", &tmp.0)
            .env_remove("PORT")
            .stdin(Stdio::null())
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap();
        let mut p = support::Guard::new(child);
        let said = eventually(120, "three ticks, or the port", || {
            let t = std::fs::read_to_string(&log).unwrap_or_default();
            if let Ok(Some(s)) = p.0.try_wait() {
                panic!("umask {}: it stopped ({}): {}", mask, s, t);
            }
            (t.matches("tick ✓").count() >= 3 || t.contains("listening on")).then_some(t)
        });
        if !said.contains("8080 is taken") {
            eprintln!("8080 was let go meanwhile: skipped ({})", said);
            return;
        }
        assert!(said.contains("quillo: 8080 is taken; this program serves nothing over HTTP, so it runs without a port (give --port for /_quillo/health)"), "umask {}: {}", mask, said);
        assert!(!said.contains("listening") && !said.contains("cannot listen"), "umask {}: {}", mask, said);
        // Its folder, the one thing in the temporary folder, its owner's alone.
        let made: Vec<PathBuf> = std::fs::read_dir(&tmp.0).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
        assert_eq!(made.len(), 1, "umask {}: {:?}", mask, made);
        assert!(made[0].file_name().unwrap().to_string_lossy().starts_with("quillo-self-"), "{:?}", made);
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&made[0]).unwrap().permissions().mode() & 0o777, 0o700, "umask {}", mask);
        }
        if let Some(ports) = tcp_listening(p.0.id()) {
            assert_eq!(ports, Vec::<String>::new(), "umask {}", mask);
            assert_eq!(has_own_socket(p.0.id()), Some(true), "umask {}", mask);
        }
        let _ = Command::new("kill").args(["-TERM", &p.0.id().to_string()]).status();
        let status = wait_for(&mut p.0, Duration::from_secs(120)).expect("SIGTERM did not stop it");
        assert!(status.success(), "umask {}: {}", mask, std::fs::read_to_string(&log).unwrap_or_default());
        assert_eq!(std::fs::read_dir(&tmp.0).unwrap().count(), 0, "umask {}: its folder is gone", mask);
    }
}
