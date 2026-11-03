//! FEEDBACK49.md: the failure side (branch main). The channel, service and JWKS issues are
//! tested beside their kind: tests/wt_edges.rs (Q-593, Q-594), tests/feedback38.rs (Q-596,
//! Q-597), tests/services.rs (Q-595), and in src/udp.rs and src/media.rs (Q-604).

mod support;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = support::temp_dir(&format!("f49-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// `quillo run` on `file` until it stops by itself (or 20 s): its exit and what it said.
fn start_alone(d: &PathBuf, file: &str) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", &support::free_port().to_string()])
        .current_dir(d)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut said = String::new();
    use std::io::Read;
    let _ = child.stderr.take().unwrap().read_to_string(&mut said);
    let _ = child.stdout.take().unwrap().read_to_string(&mut said);
    (status, said)
}

// Q-602: a database that cannot be opened at start ran the app on memory, answering writes.
#[test]
fn a_database_that_cannot_be_opened_stops_the_program_as_it_starts() {
    let junk: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let d = dir(
        "db-open",
        &[
            ("junk.db", &junk),
            ("file.qwe", b"db.open(\"junk.db\")\nroute GET /w = \"ok\"\n"),
            ("server.qwe", b"db.open(\"postgres://quillo@127.0.0.1:1/none\")\nroute GET /w = \"ok\"\n"),
            ("in_fn.qwe", b"fn setup() { db.open(\"junk.db\") }\nsetup()\nroute GET /w = \"ok\"\n"),
            ("taken.qwe", b"let r = db.open(\"junk.db\")\nprint(is_err(r))\nlet d = db.open(\"junk.db\") ?? \"fallback\"\nlet gone = db.open(\"/no/such/dir/x.db\") | is_err\n// Errors made later, perhaps in the same memory, are not that one (found in review).\nfor i in 0..3000 { err(\"x\") }\nroute GET /w = str(is_err(r))\n"),
            ("later.qwe", b"db.open(\"good.db\")\ndb.exec(\"create table if not exists t (i int)\")\nroute GET /o {\n    let r = db.open(\"junk.db\")\n    `${is_err(r)} ${db.one(\"select count(*) as n from t\").n}`\n}\n"),
            ("memory.qwe", b"db.open(\":memory:\")\ndb.exec(\"create table t (i int)\")\nroute GET /w = \"ok\"\n"),
        ],
    );
    for (file, why) in [("file.qwe", "file is not a database"), ("server.qwe", "Connection refused"), ("in_fn.qwe", "file is not a database")] {
        let (status, said) = start_alone(&d, file);
        assert_eq!(status, Some(1), "{}: {}", file, said);
        assert!(said.contains(why) && said.contains("the program's database cannot be opened, so the program does not start"), "{}: {}", file, said);
        assert!(!said.contains("listening"), "{}: {}", file, said);
    }
    // Taken by the program, the error is a value, and it starts.
    let s = support::serve(&d.join("taken.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/w", &[], "").body, "true");
    // In memory on purpose, it starts; a `db.open` later that fails gives the error and leaves
    // the database as it was.
    let s = support::serve(&d.join("memory.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/w", &[], "").body, "ok");
    let s = support::serve(&d.join("later.qwe"), &[]);
    assert_eq!(support::send(s.port, "GET", "/o", &[], "").body, "true 0");
}

fn run_src(d: &PathBuf, src: &str) -> String {
    let mut it = quillo::Interp::new(d);
    it.load_source(src, "t.qwe").map(|v| v.display()).unwrap_or_else(|e| format!("error: {}", e))
}

// Q-591: SQLite out of space ends the transaction itself; Quillo went on, the rest committed on
// its own, and the closing release failed with "no such savepoint".
#[test]
fn a_transaction_sqlite_ends_keeps_nothing_and_says_why() {
    let d = dir("full", &[]);
    let prog = |options: &str| {
        format!(
            r#"db.open("{name}.db")
db.exec("create table if not exists t (i int, s text)")
db.exec("pragma max_page_count = 12")
let big = repeat("x", 500)
let r = db.transaction({options}() => {{
    for i in range(0, 120) {{ db.exec("insert into t values (?, ?)", i, big) }}
    "done"
}})
let n = db.one("select count(*) as c from t").c
`${{is_err(r)}} ${{n}} ${{r}}`"#,
            name = if options.is_empty() { "plain" } else { "given" },
            options = options
        )
    };
    for options in ["", "{ isolation: \"serializable\" }, "] {
        let out = run_src(&d, &prog(options));
        assert!(out.starts_with("true 0 "), "{}: {}", options, out);
        assert!(out.contains("the database ended this transaction after an error, and rolled back all it held"), "{}", out);
        assert!(out.contains("full"), "the database's own reason: {}", out);
        assert!(!out.contains("no such savepoint"), "{}", out);
    }
    // Taken with `?` in the body: the same, nothing kept.
    let out = run_src(&d, "db.open(\"q.db\")\ndb.exec(\"create table if not exists t (i int, s text)\")\ndb.exec(\"pragma max_page_count = 12\")\nlet r = db.transaction(() => {\n    for i in range(0, 120) { db.exec(\"insert into t values (?, ?)\", i, repeat(\"y\", 500))? }\n    \"done\"\n})\n`${is_err(r)} ${db.one(\"select count(*) as c from t\").c}`");
    assert!(out.starts_with("true 0"), "{}", out);
    // After it, the database is as before and takes a transaction again.
    let out = run_src(&d, "db.open(\"q.db\")\ndb.exec(\"pragma max_page_count = 1000\")\nlet r = db.transaction(() => {\n    db.exec(\"insert into t values (1, 'a')\")\n    db.exec(\"insert into t values (2, 'b')\")\n    \"ok\"\n})\n`${r} ${db.one(\"select count(*) as c from t\").c}`");
    assert_eq!(out, "ok 2");
    // A transaction inside another that SQLite ends: the outer one ends with it.
    let out = run_src(&d, "db.open(\"nest.db\")\ndb.exec(\"create table if not exists t (i int, s text)\")\ndb.exec(\"pragma max_page_count = 12\")\nlet r = db.transaction(() => {\n    db.exec(\"insert into t values (0, 'first')\")\n    let inner = db.transaction(() => {\n        for i in range(0, 120) { db.exec(\"insert into t values (?, ?)\", i, repeat(\"z\", 500)) }\n        \"inner\"\n    })\n    db.exec(\"insert into t values (-1, 'after')\")\n})\n`${is_err(r)} ${db.one(\"select count(*) as c from t\").c}`");
    assert!(out.starts_with("true 0"), "{}", out);
}

// And under `idempotent`: the stored answer was committed with the rows after the error, so the
// retry was given a success.
#[test]
fn an_idempotent_call_whose_transaction_sqlite_ended_stores_no_answer() {
    let d = dir(
        "full-idem",
        &[(
            "main.qwe",
            b"db.open(\"app.db\")\ndb.exec(\"create table if not exists t (i int, s text)\")\nroute POST /fill {\n    db.exec(\"pragma max_page_count = 16\")\n    idempotent(() => {\n        for i in range(0, 120) { db.exec(\"insert into t values (?, ?)\", i, repeat(\"x\", 500)) }\n        \"all 120 rows written\"\n    })\n}\nroute GET /n = str(db.one(\"select count(*) as c from t\").c)\n",
        )],
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let key = [("idempotency-key", "fill-1")];
    let first = support::send(s.port, "POST", "/fill", &key, "");
    assert_ne!(first.status, 200, "{}", first.body);
    assert_eq!(support::send(s.port, "GET", "/n", &[], "").body, "0");
    // The same key again is not given a success that never happened.
    let again = support::send(s.port, "POST", "/fill", &key, "");
    assert_ne!(again.status, 200, "{}", again.body);
    assert!(again.header("idempotent-replayed").is_none(), "{:?}", again.headers);
    assert_eq!(support::send(s.port, "GET", "/n", &[], "").body, "0");
}

// Q-600: a request followed at once by the client's FIN was answered, and then the connection
// stayed open until the idle limit, as no event said the client was done.
#[test]
fn a_request_and_the_clients_end_at_once_are_answered_and_the_connection_closed() {
    use std::io::{Read, Write};
    let d = dir("fin", &[("main.qwe", b"route GET / = \"hello\"\nroute POST /echo = request.body\n")]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    for i in 0..20 {
        let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        // Kept alive, as far as the request says; the end follows straight after it.
        let req = if i % 2 == 0 {
            "GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_string()
        } else {
            "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nworld".to_string()
        };
        c.write_all(req.as_bytes()).unwrap();
        c.shutdown(std::net::Shutdown::Write).unwrap();
        let started = std::time::Instant::now();
        let mut out = Vec::new();
        let r = c.read_to_end(&mut out);
        assert!(r.is_ok(), "try {}: the connection was not closed: {:?} after {:?}", i, r, started.elapsed());
        let out = String::from_utf8_lossy(&out);
        assert!(out.starts_with("HTTP/1.1 200"), "{}", out);
        assert!(out.ends_with(if i % 2 == 0 { "hello" } else { "world" }), "{}", out);
    }
    // Two requests pipelined before the end: both answered, then closed.
    let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    c.shutdown(std::net::Shutdown::Write).unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    assert_eq!(out.matches("HTTP/1.1 200").count(), 2, "{}", out);
    // A request cut short by the end is not answered, and the connection closes.
    let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").unwrap();
    c.shutdown(std::net::Shutdown::Write).unwrap();
    let mut out = Vec::new();
    assert!(c.read_to_end(&mut out).is_ok());
    assert!(!String::from_utf8_lossy(&out).contains("hello"));
}

// Q-605: numbers silently wrong at the edges.
#[test]
fn stats_refuse_nan_and_infinity_and_money_stays_in_range() {
    let d = dir("numbers", &[]);
    let fails = |src: &str| run_src(&d, src);
    for (src, part) in [
        ("stats.histogram([1, 2, math.inf], 3)", "`stats.histogram`: the data hold inf at index 2"),
        ("stats.histogram([1, 2, 3], [1, math.nan, 3])", "`stats.histogram`: the edges hold NaN at index 1"),
        ("stats.median([3, math.nan, 1])", "`stats.median`: the data hold NaN at index 1"),
        ("stats.quantiles([1, -math.inf, 3, 4])", "`stats.quantiles`: the data hold -inf at index 1"),
        ("stats.percentile([1, math.nan], 50)", "`stats.percentile`: the data hold NaN at index 1"),
        ("stats.rank([math.nan, 1])", "`stats.rank`: the data hold NaN at index 0"),
        ("stats.describe([1, math.inf])", "`stats.describe`: the data hold inf at index 1"),
        ("stats.median_high([math.inf])", "`stats.median_high`: the data hold inf at index 0"),
        ("stats.histogram([-1e308, 1e308], 3)", "the data spread over more than a float can hold"),
    ] {
        let out = fails(src);
        assert!(out.contains(part), "{}: {}", src, out);
        assert!(out.contains("NaN and infinity cannot be counted or put in order") || part.contains("spread"), "{}", out);
    }
    // A mean or a spread has them as Python's statistics has them.
    assert_eq!(fails("math.isnan(stats.mean([math.inf, -math.inf]))"), "true");
    assert_eq!(fails("stats.mean([math.inf, 1])"), "inf");
    // Finite data, and the largest finite numbers that fit, are as before.
    assert_eq!(fails("stats.median([3, 1, 2])"), "2");
    assert_eq!(fails("stats.histogram([1, 2, 3], 3).counts"), "[1, 1, 1]");
    assert_eq!(fails("stats.histogram([-1e300, 1e300], 2).counts"), "[1, 1]");
    assert_eq!(fails("stats.histogram([], 2).counts"), "[0, 0]");
    // Money past what `money.of` takes is an error value from each way of making it.
    for src in [
        "money.mul(money.of(1, \"USD\"), 9223372036854775807)",
        "let big = money.of(90000000000000000, \"USD\")\nmoney.add(big, big)",
        "money.sub(money.of(-90000000000000000, \"USD\"), money.of(90000000000000000, \"USD\"))",
        "money.of(9223372036854775807, \"USD\")",
    ] {
        let out = fails(src);
        assert!(out.starts_with("err(") && out.contains("is too large an amount"), "{}: {}", src, out);
    }
    // At the edge, an amount still fits, and the arithmetic is as before.
    assert_eq!(fails("money.to_minor(money.add(money.of(\"92233720368547758.06\", \"USD\"), money.of(\"0.01\", \"USD\")))"), "9223372036854775807");
    assert_eq!(fails("money.neg(money.of(5, \"EUR\"))"), "{ amount: -5.00m, currency: \"EUR\" }");
    // The least amount that fits, negated, does not (found in review).
    let out = fails("money.neg(money.from_minor(-9223372036854775807 - 1, \"USD\"))");
    assert!(out.starts_with("err(") && out.contains("is too large an amount"), "{}", out);
    assert_eq!(fails("money.to_minor(money.neg(money.from_minor(-9223372036854775807, \"USD\")))"), "9223372036854775807");
    assert_eq!(fails("money.mul(money.of(2, \"JPY\"), 3)"), "{ amount: 6m, currency: \"JPY\" }");
}

// Q-606 and Q-592's JSON part: lists that share parts grew without bound in JSON, and a list
// nested 100,000 deep overflowed the stack and stopped the server.
#[test]
fn json_too_large_or_too_deep_is_an_error_and_the_server_goes_on() {
    let d = dir(
        "json-big",
        &[(
            "main.qwe",
            b"fn shared(n) {\n    let xs = [1]\n    for i in range(0, n) { xs = [xs, xs] }\n    xs\n}\nfn deep(n) {\n    let x = []\n    for i in range(0, n) { x = [x] }\n    x\n}\nfn told(r) = is_err(r) ? `ERR ${r.error}` : r\nroute GET /shared/:n(int) = told(json.stringify(shared(n)))\nroute GET /deep/:n(int) = told(json.stringify(deep(n)))\nroute GET /len/:n(int) = str(len(json.stringify(shared(n))))\nroute GET /answer/shared = shared(26)\nroute GET /answer/deep = deep(20000)\nroute GET /ping = \"pong\"\n",
        )],
    );
    let s = support::serve(&d.join("main.qwe"), &[]);
    let t = std::time::Instant::now();
    let a = support::send(s.port, "GET", "/shared/26", &[], "");
    assert!(a.body.contains("json.stringify: its JSON would be more than 256 MiB"), "{}", a.body);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    let a = support::send(s.port, "GET", "/deep/20000", &[], "");
    assert!(a.body.contains("json.stringify: it is nested more than 10000 deep"), "{}", a.body);
    // A route's own value, written as its answer: a 500, and the server goes on.
    assert_eq!(support::send(s.port, "GET", "/answer/shared", &[], "").status, 500);
    assert_eq!(support::send(s.port, "GET", "/answer/deep", &[], "").status, 500);
    assert_eq!(support::send(s.port, "GET", "/ping", &[], "").body, "pong");
    // Shared parts under the limit are written, fast, with no tree made first: 24 rounds took
    // 3.5 GB and 3 s (found in review).
    let t = std::time::Instant::now();
    assert_eq!(support::send(s.port, "GET", "/len/24", &[], "").body, "100663293");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // Under the limits, as before.
    assert_eq!(support::send(s.port, "GET", "/shared/3", &[], "").body, "[[[[1],[1]],[[1],[1]]],[[[1],[1]],[[1],[1]]]]");
    assert!(support::send(s.port, "GET", "/deep/9999", &[], "").body.starts_with("[[[["));
}
