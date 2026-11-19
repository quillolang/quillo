//! FEEDBACK78.md, Q-1168: many short transactions at once on one row (branch feedback75). Each
//! statement of a PostgreSQL or MySQL transaction waits on one of the server's waiting threads.
//! Two faults let them stop for good. A thread took one job and left the rest queued while other
//! threads slept, so a transaction's next statement or its commit could wait behind statements
//! waiting on that transaction's row lock. And with every thread waiting on its locks (MySQL's 100
//! connections against 65 threads, or `--workers` under the pool's 8), its commit had no thread.
//! PostgreSQL then waited for good, a stop too; MySQL for 50 s, then error 1205, or with `retries`
//! 50 s a try, saying nothing.
#![cfg(unix)]

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

const AT_ONCE: usize = 100;

/// The program: `POST /take` adds one to the row `hot` in a transaction (an upsert, then a read
/// of it), `POST /own/:i` the same on a row of its own; `GET /n` and `GET /own` count them.
fn program(url: &str, mysql: bool, retries: bool) -> String {
    let upsert = if mysql {
        "db.exec(\"insert into q1168 (k, n) values (?, 1) on duplicate key update n = n + 1\", key)?"
    } else {
        "db.one(\"insert into q1168 (k, n) values (?, 1) on conflict (k) do update set n = q1168.n + 1 returning n\", key)?"
    };
    let opts = if retries { "{ retries: 3 }, " } else { "" };
    format!(
        r#"db.open("{url}")
db.exec("drop table if exists q1168")?
db.exec("create table q1168 (k varchar(40) primary key, n int not null)")?

fn add(key: str) {{
    let r = db.transaction({opts}() => {{
        {upsert}
        db.one("select n from q1168 where k = ?", key)
    }})
    if is_err(r) {{ reply 500 {{ error: error(r) }} }}
    return r
}}

route POST /take = add("hot")
route POST /own/:i = add(`own${{i}}`)
route GET /n = str(db.one("select n from q1168 where k = 'hot'")?.n)
route GET /own {{
    let rows = db.query("select n from q1168 where k <> 'hot'")?
    let total = 0
    for r in rows {{ total = total + r.n }}
    {{ rows: len(rows), n: total }}
}}
"#
    )
}

/// A MariaDB database's URL for these tests: up to 50 connections, kept while idle. With the
/// driver's defaults the pool closed each connection given back past 10 idle ones and opened
/// another, and with 100 requests at once MariaDB's 151 were now and then all taken, by those
/// still closing (`Too many connections`; FEEDBACK78.md, Q-1169). 50 still let the transactions
/// waiting on a row lock take every thread (`--workers 8` makes 9).
fn my_url(my: &support::Maria, db: &str) -> String {
    format!("{}?pool_max=50&inactive_connection_ttl=60", my.url(db))
}

/// A `quillo run` of `src`, with `args`, and its port; killed when dropped.
fn start(dir: &Path, src: &str, args: &[&str]) -> (support::Guard, u16) {
    let file = dir.join("main.qwe");
    std::fs::write(&file, src).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&file)
        .args(["--port", "0", "--log", "off"])
        .args(args)
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cannot start quillo");
    let mut g = support::Guard::new(child);
    let port = support::port_in_stderr(&mut g.0, "main.qwe", Instant::now() + Duration::from_secs(30));
    (g, port)
}

/// One request; its answer's status and body, status 0 for none within `wait`.
fn call(port: u16, method: &str, path: &str, wait: Duration) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return (0, "no connection".into()) };
    s.set_read_timeout(Some(wait)).unwrap();
    let head = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n", method, path);
    if s.write_all(head.as_bytes()).is_err() {
        return (0, "not sent".into());
    }
    let mut out = Vec::new();
    if s.read_to_end(&mut out).is_err() {
        return (0, "no answer in time".into());
    }
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, body.to_string())
}

/// `AT_ONCE` requests let go together (`{i}` in the path numbers them); how many were answered
/// 200, and what the others got.
fn burst(port: u16, path: &str, wait: Duration) -> (usize, Vec<String>) {
    let go = Arc::new(Barrier::new(AT_ONCE));
    let all: Vec<_> = (0..AT_ONCE)
        .map(|i| {
            let (go, path) = (go.clone(), path.replace("{i}", &i.to_string()));
            std::thread::spawn(move || {
                go.wait();
                call(port, "POST", &path, wait)
            })
        })
        .collect();
    let mut ok = 0;
    let mut others = Vec::new();
    for t in all {
        match t.join().unwrap() {
            (200, _) => ok += 1,
            (status, body) => others.push(format!("{} {}", status, body.chars().take(120).collect::<String>())),
        }
    }
    (ok, others)
}

/// Rounds of `AT_ONCE` transactions on one row, each answered within `wait`; then the count.
fn rounds(port: u16, n: usize, wait: Duration, what: &str) {
    for round in 0..n {
        let t = Instant::now();
        let (ok, others) = burst(port, "/take", wait);
        assert_eq!(
            ok,
            AT_ONCE,
            "{}, round {}: {} of {} not answered 200 within {:?} ({:?} in all): {:?}",
            what,
            round,
            AT_ONCE - ok,
            AT_ONCE,
            wait,
            t.elapsed(),
            others.iter().take(3).collect::<Vec<_>>()
        );
    }
    assert_eq!(call(port, "GET", "/n", wait), (200, (n * AT_ONCE).to_string()), "{}: every transaction counted once", what);
}

/// SIGTERM: the stop finishes, as nothing is left in progress.
fn stops(g: &mut support::Guard, what: &str) {
    // SAFETY: a signal to the server this test started.
    unsafe { libc::kill(g.0.id() as i32, libc::SIGTERM) };
    let until = Instant::now() + Duration::from_secs(5);
    while g.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < until, "{}: the stop did not finish within 5 s", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Q-1168 on PostgreSQL. With `--workers 2` the server has 3 waiting threads and the pool 8
// connections: the requests waiting on the row lock took every thread, and the commit that would
// let it go waited for one for good. With 64 (the default), a thread took one job and left the
// rest, and in about 1 burst in 6 the holder's next statement was among those left, while 59
// threads slept. A burst is answered in well under a second now.
#[test]
fn transactions_on_one_row_at_once_all_end_on_postgres() {
    let Some(pg) = support::postgres("f78-row") else {
        eprintln!("skipped: no PostgreSQL");
        return;
    };
    for workers in ["2", "64"] {
        let url = pg.database(&format!("f78_row_{}", workers));
        let dir = support::temp_dir(&format!("f78-row-{}", workers));
        let (mut g, port) = start(&dir, &program(&url, false, false), &["--workers", workers]);
        let what = format!("PostgreSQL, --workers {}", workers);
        rounds(port, 10, Duration::from_secs(10), &what);
        stops(&mut g, &what);
    }
}

// Q-1168 on MySQL (MariaDB), with retries: its pool lets up to 100 transactions hold connections
// (50 here), so 100 at once put a statement waiting on the row lock on every thread (`--workers 8`
// makes 9). Each waited 50 s for MariaDB's lock timeout (1205), which `retries` takes for a conflict and
// tries again: requests went on for 50 to 200 s, with no error said. Under 20 s each, so a wait for
// the lock timeout cannot pass.
#[test]
fn transactions_on_one_row_at_once_all_end_on_mariadb() {
    let Some(my) = support::mariadb("f78-row") else {
        eprintln!("skipped: no MariaDB");
        return;
    };
    let url = my_url(&my, "f78_row");
    let dir = support::temp_dir("f78-my");
    let (mut g, port) = start(&dir, &program(&url, true, true), &["--workers", "8"]);
    rounds(port, 3, Duration::from_secs(20), "MariaDB, retries: 3");
    stops(&mut g, "MariaDB");
}

// The ordinary case beside it: transactions at once on rows of their own wait on nothing but
// their turn for a connection, on either database.
#[test]
fn transactions_on_rows_of_their_own_at_once_all_end() {
    let pg = support::postgres("f78-own");
    let my = support::mariadb("f78-own");
    let mut urls = Vec::new();
    if let Some(pg) = &pg {
        urls.push((pg.database("f78_own"), false));
    }
    if let Some(my) = &my {
        urls.push((my_url(my, "f78_own"), true));
    }
    for (url, mysql) in urls {
        let dir = support::temp_dir(&format!("f78-own-{}", mysql));
        let (mut g, port) = start(&dir, &program(&url, mysql, false), &["--workers", "8"]);
        for round in 0..3 {
            let (ok, others) = burst(port, "/own/{i}", Duration::from_secs(10));
            assert_eq!(ok, AT_ONCE, "mysql {}, round {}: {:?}", mysql, round, others.iter().take(3).collect::<Vec<_>>());
        }
        let (status, body) = call(port, "GET", "/own", Duration::from_secs(10));
        assert_eq!(status, 200, "{}", body);
        assert_eq!(body, r#"{"rows":100,"n":300}"#, "mysql {}: each row taken 3 times", mysql);
        stops(&mut g, "rows of their own");
    }
}

// `parallel` inside a transaction runs its functions one after another, in the transaction (their
// SQL would otherwise run on other connections, outside it, and a write to a row the transaction
// holds would wait for itself for good): what one writes the next reads, a failure rolls back what
// they wrote, and 100 such requests at once on the row all end.
#[test]
fn parallel_inside_a_transaction_runs_in_it() {
    let Some(pg) = support::postgres("f78-par") else {
        eprintln!("skipped: no PostgreSQL");
        return;
    };
    let url = pg.database("f78_par");
    let src = format!(
        r#"db.open("{}")
db.exec("create table p (k varchar(10) primary key, n int not null)")
db.exec("insert into p values ('a', 0)")

route POST /write {{
    let r = db.transaction(() => {{
        db.exec("update p set n = n + 1 where k = 'a'")?
        parallel(() => db.exec("update p set n = n + 10 where k = 'a'").changes, () => db.one("select n from p where k = 'a'").n)
    }})
    if is_err(r) {{ reply 500 {{ error: error(r) }} }}
    r
}}

route POST /fail {{
    let r = db.transaction(() => {{
        parallel(() => db.exec("insert into p values ('b', 5)").changes, () => db.exec("update p set n = n + 100 where k = 'b'").changes)
        err("rolled back")
    }})
    {{ said: error(r), b: len(db.query("select n from p where k = 'b'")) }}
}}

route GET /n = str(db.one("select n from p where k = 'a'")?.n)
"#,
        url
    );
    let dir = support::temp_dir("f78-par");
    let (mut g, port) = start(&dir, &src, &["--workers", "8"]);
    let wait = Duration::from_secs(10);
    assert_eq!(call(port, "POST", "/write", wait), (200, "[1,11]".to_string()), "the second function reads what the first wrote");
    assert_eq!(call(port, "POST", "/fail", wait), (200, r#"{"said":"rolled back","b":0}"#.to_string()));
    let (ok, others) = burst(port, "/write", wait);
    assert_eq!(ok, AT_ONCE, "{:?}", others.iter().take(3).collect::<Vec<_>>());
    assert_eq!(call(port, "GET", "/n", wait), (200, (11 * (AT_ONCE + 1)).to_string()));
    stops(&mut g, "parallel");
}

// Q-1170: a transaction waiting for one of MySQL's connections waits without a thread, as
// PostgreSQL's does. With `pool_max=1` and `--workers 2` (3 threads), ten transactions that each
// hold the connection for 300 ms: those waiting for it took the threads, and a request needing one
// for a millisecond waited behind them for seconds; now it is answered at once.
#[test]
fn a_transaction_waiting_for_a_mysql_connection_holds_no_thread() {
    let Some(my) = support::mariadb("f78-checkout") else {
        eprintln!("skipped: no MariaDB");
        return;
    };
    let url = format!("{}?pool_min=1&pool_max=1", my.url("f78_checkout"));
    let src = format!(
        r#"db.open("{}")

route POST /slow/:i = db.transaction(() => {{
    let r = db.one("select ? as i", i)?
    sleep(300)
    str(r.i)
}})

route GET /quick {{
    sleep(1)
    "quick"
}}
"#,
        url
    );
    let dir = support::temp_dir("f78-checkout");
    let (mut g, port) = start(&dir, &src, &["--workers", "2"]);
    let slow: Vec<_> = (0..10).map(|i| std::thread::spawn(move || call(port, "POST", &format!("/slow/{}", i), Duration::from_secs(30)))).collect();
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    assert_eq!(call(port, "GET", "/quick", Duration::from_secs(20)), (200, "quick".to_string()));
    assert!(t.elapsed() < Duration::from_secs(1), "a request needing a thread waited {:?} behind transactions waiting for a connection", t.elapsed());
    for (i, s) in slow.into_iter().enumerate() {
        assert_eq!(s.join().unwrap(), (200, i.to_string()));
    }
    stops(&mut g, "MySQL checkout");
}
