//! `live()` at a thousand pages, and a form sent twice (FEEDBACK24.md, Q-189 to Q-196; branch
//! feedback24-fixes).

mod support;

use std::path::PathBuf;
use support::{send, serve, Answer};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback24-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

const FORM: &str = r#"
db.open("shop.db")
db.exec("create table if not exists paid (cents int)")

route GET / = <html><body>
    <form method="post" action="/pay" idempotent>
        <input name="cents" value="500"/>
    </form>
</body></html>

route POST /pay = idempotent(() => {
    sleep(300)
    db.exec("insert into paid (cents) values (?)", request.form.cents)
    redirect("/done")
})

route POST /strict = idempotent({ required: true }, () => {
    db.exec("insert into paid (cents) values (?)", request.form.cents)
    "paid"
})

route POST /typed { form: { cents: int } } = idempotent(() => {
    db.exec("insert into paid (cents) values (?)", cents)
    "paid"
})

route GET /done = "done"
route GET /count = len(db.query("select cents from paid"))
route GET /cached {
    respond(200, <form method="post" idempotent></form>, { "cache-control": "public, max-age=3600" })
}
"#;

fn field(page: &str) -> String {
    let at = page.find("name=\"_idempotency_key\"").expect("the form has its key");
    let value = page[at..].split("value=\"").nth(1).unwrap();
    value.split('"').next().unwrap().to_string()
}

fn post(port: u16, path: &str, body: &str) -> Answer {
    send(port, "POST", path, &[("content-type", "application/x-www-form-urlencoded"), ("sec-fetch-site", "same-origin")], body)
}

fn a_form_sent_twice_runs_once(name: &str, target: &str) {
    let d = dir(name, &[("main.qwe", &FORM.replace("shop.db", target))]);
    let s = serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "");
    assert!(!page.body.contains(" idempotent"), "{}: {}", name, page.body);
    let key = field(&page.body);
    assert_ne!(key, field(&send(s.port, "GET", "/", &[], "").body), "{}: each page has its own", name);

    // Two clicks: the browser shows the second's answer, so that must be the first's.
    let port = s.port;
    let body = format!("_idempotency_key={}&cents=500", key);
    let both: Vec<_> = (0..2).map(|_| { let b = body.clone(); std::thread::spawn(move || post(port, "/pay", &b)) }).collect();
    let answers: Vec<(u16, Option<String>)> = both.into_iter().map(|t| { let a = t.join().unwrap(); (a.status, a.header("location").map(String::from)) }).collect();
    assert_eq!(answers, vec![(303, Some("/done".to_string())); 2], "{}", name);
    assert_eq!(send(s.port, "GET", "/count", &[], "").body, "1", "{}: the work ran once", name);
    // Back, and the same again.
    assert_eq!(post(s.port, "/pay", &body).status, 303, "{}", name);
    assert_eq!(send(s.port, "GET", "/count", &[], "").body, "1", "{}", name);
    // Back, edited, and sent: another payment, not a 422.
    assert_eq!(post(s.port, "/pay", &format!("_idempotency_key={}&cents=700", key)).status, 303, "{}", name);
    assert_eq!(send(s.port, "GET", "/count", &[], "").body, "2", "{}", name);
    // `required` is met by the form's key, and a typed form does not mind the field.
    assert_eq!(post(s.port, "/strict", "cents=1").status, 400, "{}", name);
    assert_eq!(post(s.port, "/strict", &format!("_idempotency_key={}&cents=1", uuid(name, 1))).status, 200, "{}", name);
    assert_eq!(post(s.port, "/typed", &format!("_idempotency_key={}&cents=1", uuid(name, 2))).status, 200, "{}", name);
    assert_eq!(send(s.port, "GET", "/count", &[], "").body, "4", "{}", name);
}

fn uuid(name: &str, n: u32) -> String {
    format!("{}-{}-{}", name, n, std::process::id())
}

#[test]
fn a_plain_form_sent_twice_runs_once_on_sqlite() {
    a_form_sent_twice_runs_once("form-sqlite", "shop.db");
}

#[test]
fn a_plain_form_sent_twice_runs_once_on_postgres() {
    let Some(pg) = support::postgres("feedback24-form") else { return };
    a_form_sent_twice_runs_once("form-postgres", &pg.url);
}

#[test]
fn a_plain_form_sent_twice_runs_once_on_mariadb() {
    let Some(m) = support::mariadb("feedback24-form") else { return };
    a_form_sent_twice_runs_once("form-mariadb", &m.url("form24"));
}

/// A key is for one page of one visitor: no cache may keep it.
#[test]
fn a_page_with_a_forms_key_is_never_kept() {
    let d = dir("kept", &[("main.qwe", FORM)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let a = send(s.port, "GET", "/cached", &[], "");
    assert_eq!(a.header("cache-control"), Some("no-store"), "{}", a.body);
    let ahead = send(s.port, "GET", "/", &[("x-quillo-prefetch", "1")], "");
    assert_eq!(ahead.status, 200);
    assert_eq!(ahead.header("cache-control"), None, "{}", ahead.body);
}

// ---- live data ----------------------------------------------------------------------------

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A server with `--metrics` and its log kept, for what no answer shows.
struct Run {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run(d: &std::path::Path, flags: &[&str]) -> Run {
    let port = support::free_port();
    let log = d.join("log.txt");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string(), "--metrics"])
        .args(flags)
        .env("QUILLO_SECRET", "a test secret for live data, long enough to use")
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(d.join("out.txt")).unwrap())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut r = Run { child, port, log };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        if let Ok(Some(s)) = r.child.try_wait() {
            panic!("quillo stopped ({}): {}", s, std::fs::read_to_string(&r.log).unwrap_or_default());
        }
        assert!(Instant::now() < deadline, "the server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    r
}

impl Run {
    fn metric(&self, name: &str) -> i64 {
        let text = send(self.port, "GET", "/_quillo/metrics", &[], "").body;
        text.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.trim().parse().ok())).unwrap_or_else(|| panic!("no {} in\n{}", name, text))
    }

    fn get(&self, path: &str) -> String {
        send(self.port, "GET", path, &[], "").body
    }

    /// A page's refresh of a live loader.
    fn refresh(&self, f: &str, cookie: &str) -> serde_json::Value {
        let a = send(self.port, "POST", &format!("/_quillo/rpc/{}", f), &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin"), ("cookie", cookie)], "[]");
        serde_json::from_str(&a.body).unwrap_or_else(|_| panic!("{} {}", a.status, a.body))
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// A page's WebSocket for live data.
struct Page(TcpStream);

fn open(port: u16, since: u64, topics: &str, xff: Option<&str>) -> Option<Page> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let forwarded = xff.map_or(String::new(), |x| format!("X-Forwarded-For: {}\r\n", x));
    write!(s, "GET /_quillo/live?since={}&s=&t={} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n{}Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", since, topics, forwarded).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).ok()?;
        head.push(b[0]);
    }
    String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101").then_some(Page(s))
}

impl Page {
    /// The next text frame, skipping pings; None if none comes in `wait`.
    fn frame(&mut self, wait: Duration) -> Option<String> {
        self.0.set_read_timeout(Some(wait)).unwrap();
        loop {
            let mut h = [0u8; 2];
            self.0.read_exact(&mut h).ok()?;
            let mut len = (h[1] & 0x7f) as usize;
            if len == 126 {
                let mut l = [0u8; 2];
                self.0.read_exact(&mut l).ok()?;
                len = u16::from_be_bytes(l) as usize;
            }
            let mut data = vec![0u8; len];
            self.0.read_exact(&mut data).ok()?;
            if h[0] & 0x0f == 1 {
                return Some(String::from_utf8_lossy(&data).into_owned());
            }
        }
    }

    fn frames(&mut self, wait: Duration) -> usize {
        let mut n = 0;
        while self.frame(wait).is_some() {
            n += 1;
        }
        n
    }

    /// As a browser leaving does: a close frame, masked.
    fn leave(mut self) {
        let _ = self.0.write_all(&[0x88, 0x80, 1, 2, 3, 4]);
        std::thread::sleep(Duration::from_millis(50));
    }
}

const LIVE: &str = r#"
db.open("live.db")
db.exec("create table if not exists rows (id integer primary key, title text)")
db.exec("create table if not exists other (id integer primary key)")
db.exec("insert into rows (title) values ('one'), ('two')")
let visitors = 0

server fn rows() = db.query("select * from rows order by id")
server fn slow() {
    sleep(300)
    db.query("select * from rows order by id")
}
server fn mine() {
    print("ran")
    { who: cookie("who"), rows: db.query("select * from rows order by id") }
}
server fn counted() {
    print("counted")
    { price: 12.50m, rows: db.query("select * from rows order by id") }
}
server fn big() = db.query("select id, printf('%0100d', id) as padding from (with recursive n(id) as (select 1 union all select id + 1 from n where id < 500) select id from n)")

component Rows() {
    state r = live(counted)
    <p id="rows">{str(r.price)} {len(r.rows)}</p>
}
component Mine() {
    state r = live(mine)
    <p id="mine">{r.who}</p>
}

route GET / = <html><body><Rows/></body></html>
route GET /mine = <html><body><Mine/></body></html>
route GET /write = db.exec("update rows set title = ? where id = 1", str(random_int(0, 1000000)))
route GET /other = db.exec("insert into other (id) values (null)")
route GET /visit { visitors = visitors + 1; "seen" }
route GET /fill {
    for i in range(0, 40) {
        db.exec("insert into rows (title) values (?)", `row ${i}`)
    }
    "filled"
}
route GET /ip = request.ip
"#;

fn live_app(name: &str) -> PathBuf {
    dir(name, &[("main.qwe", LIVE)])
}

fn seq_of(r: &Run) -> u64 {
    r.refresh("rows", "")["seq"].as_u64().unwrap()
}

/// Q-189: behind a proxy, each visitor is known, and the limit is theirs.
#[test]
fn a_proxy_named_is_believed_and_no_other() {
    let d = live_app("proxy");
    let seen = |r: &Run, xff: &str| send(r.port, "GET", "/ip", &[("x-forwarded-for", xff)], "").body;
    let plain = run(&d, &[]);
    assert_eq!(seen(&plain, "203.0.113.9"), "127.0.0.1", "anyone can send the header");
    drop(plain);
    let r = run(&d, &["--trust-proxy", "127.0.0.1"]);
    assert_eq!(seen(&r, "203.0.113.9"), "203.0.113.9");
    // What the client wrote comes first: the proxy's own entry is the one to believe.
    assert_eq!(seen(&r, "6.6.6.6, 203.0.113.9"), "203.0.113.9");
    assert_eq!(seen(&r, "203.0.113.9, 127.0.0.1"), "203.0.113.9");
    assert_eq!(seen(&r, "not an address"), "127.0.0.1");
    drop(r);
    let r = run(&d, &["--trust-proxy", "10.0.0.0/8"]);
    assert_eq!(seen(&r, "203.0.113.9"), "127.0.0.1", "this peer is not one of them");
}

#[test]
fn trust_proxy_refuses_what_is_not_an_address() {
    let d = live_app("proxy-bad");
    for bad in ["nonsense", "10.0.0.0/99", ""] {
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).args(["--port", "0", "--trust-proxy", bad]).output().unwrap();
        assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("--trust-proxy"), "{}: {}", bad, String::from_utf8_lossy(&o.stderr));
    }
}

#[test]
fn past_a_clients_limit_a_page_is_refused_and_it_is_said() {
    let d = live_app("limit");
    let r = run(&d, &[]);
    let since = seq_of(&r);
    let held: Vec<Page> = (0..256).map(|_| open(r.port, since, "db:rows", None).expect("a place")).collect();
    assert!(open(r.port, since, "db:rows", None).is_none(), "page 257");
    assert_eq!(r.metric("quillo_live_pages"), 256);
    assert_eq!(r.metric("quillo_live_refused_total"), 1);
    assert!(r.log().contains("--trust-proxy"), "{}", r.log());
    drop(held);
    drop(r);
    // Named proxy: each visitor has places of their own. Or a limit of one's own.
    let r = run(&d, &["--trust-proxy", "127.0.0.1"]);
    let since = seq_of(&r);
    let held: Vec<Page> = (0..300).map(|i| open(r.port, since, "db:rows", Some(&format!("203.0.113.{}", i % 250))).expect("a place")).collect();
    assert_eq!(r.metric("quillo_live_pages"), 300);
    drop(held);
    drop(r);
    let r = run(&d, &["--live-per-client", "300"]);
    let since = seq_of(&r);
    let held: Vec<Page> = (0..300).map(|_| open(r.port, since, "db:rows", None).expect("a place")).collect();
    assert_eq!(held.len(), 300);
    assert!(open(r.port, since, "db:rows", None).is_none());
}

/// Q-190: the first change goes at once, and a burst after it is one more frame.
#[test]
fn a_burst_of_changes_is_a_few_frames() {
    let d = live_app("burst");
    let r = run(&d, &[]);
    let mut page = open(r.port, seq_of(&r), "db:rows", None).unwrap();
    let start = Instant::now();
    r.get("/write");
    assert!(page.frame(Duration::from_secs(2)).expect("told").contains("db:rows"));
    assert!(start.elapsed() < Duration::from_millis(90), "{:?}", start.elapsed());
    for _ in 0..30 {
        r.get("/write");
    }
    let frames = page.frames(Duration::from_millis(400));
    assert!((1..=8).contains(&frames), "{} frames for 30 writes", frames);
}

#[test]
fn a_gap_that_is_not_one_is_refused() {
    let bad = "server fn load() = 1\ncomponent C() {\n    state x = live({ every: \"soon\" }, load)\n    <p>{x}</p>\n}\nroute GET / = <html><body><C/></body></html>\n";
    let good = bad.replace("soon", "2s");
    let d = dir("gap", &[("bad.qwe", bad), ("good.qwe", &good)]);
    let check = |f: &str| Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join(f)).output().unwrap();
    let o = check("bad.qwe");
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("every"), "{}", String::from_utf8_lossy(&o.stderr));
    let o = check("good.qwe");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// Q-191: calls of one loader wait for the run under way, and a write elsewhere
/// does not cost them it.
#[test]
fn calls_made_together_are_one_run() {
    let d = live_app("together");
    let r = run(&d, &[]);
    let port = r.port;
    let calls: Vec<_> = (0..12).map(|_| std::thread::spawn(move || send(port, "POST", "/_quillo/rpc/slow", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]").status)).collect();
    std::thread::sleep(Duration::from_millis(100));
    // Not what it read: the run stands.
    r.get("/other");
    r.get("/visit");
    assert!(calls.into_iter().all(|c| c.join().unwrap() == 200));
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"ran\"}"), 1);
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"shared\"}"), 11);
}

#[test]
fn a_run_that_read_what_changed_is_not_shared() {
    let d = live_app("stale");
    let r = run(&d, &[]);
    let port = r.port;
    let first = std::thread::spawn(move || send(port, "POST", "/_quillo/rpc/slow", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin")], "[]").status);
    std::thread::sleep(Duration::from_millis(100));
    r.get("/write");
    assert_eq!(first.join().unwrap(), 200);
    r.refresh("slow", "");
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"ran\"}"), 2);
}

/// Each viewer's run is their own, on a page load and on a refresh, and one
/// viewer's is never another's.
#[test]
fn a_run_that_read_the_viewer_stays_theirs() {
    let d = live_app("viewer");
    let r = run(&d, &[]);
    let page = |who: &str| send(r.port, "GET", "/mine", &[("cookie", &format!("who={}", who))], "").body;
    assert!(page("ana").contains(">ana<"));
    assert!(page("bob").contains(">bob<"), "{}", page("bob"));
    assert_eq!(r.refresh("mine", "who=ana")["ok"]["who"], "ana");
    assert_eq!(r.refresh("mine", "who=bob")["ok"]["who"], "bob");
    // Learned, nobody waits on them.
    let port = r.port;
    let start = Instant::now();
    let calls: Vec<_> = (0..8).map(|i| std::thread::spawn(move || {
        let cookie = format!("who=v{}", i);
        send(port, "POST", "/_quillo/rpc/mine", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin"), ("cookie", &cookie)], "[]").body
    })).collect();
    for (i, c) in calls.into_iter().enumerate() {
        assert!(c.join().unwrap().contains(&format!("v{}", i)));
    }
    assert!(start.elapsed() < Duration::from_secs(2));
}

/// Q-192: a change wakes those following it, and nobody else.
#[test]
fn a_change_wakes_only_who_follows_it() {
    let d = live_app("wakes");
    let r = run(&d, &[]);
    let since = seq_of(&r);
    let mut near: Vec<Page> = (0..5).map(|_| open(r.port, since, "db:rows", None).unwrap()).collect();
    let _far: Vec<Page> = (0..20).map(|_| open(r.port, since, "db:nothing", None).unwrap()).collect();
    for _ in 0..10 {
        r.get("/other");
        r.get("/visit");
    }
    assert_eq!(r.metric("quillo_live_wakes_total"), 0);
    r.get("/write");
    assert!(near[0].frame(Duration::from_secs(2)).is_some());
    assert_eq!(r.metric("quillo_live_wakes_total"), 5);
}

/// Q-193: a page load shares a run, and comes out the same.
#[test]
fn page_loads_share_a_run() {
    let d = live_app("arrivals");
    let r = run(&d, &[]);
    let first = r.get("/");
    assert!(first.contains(">12.50 2<"), "{}", first);
    for _ in 0..5 {
        let again = r.get("/");
        assert!(again.contains(">12.50 2<"), "{}", again);
    }
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"ran\"}"), 1);
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"shared\"}"), 5);
    assert_eq!(std::fs::read_to_string(d.join("out.txt")).unwrap().matches("counted").count(), 1);
    // A change ends it.
    r.get("/write");
    r.get("/");
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"ran\"}"), 2);
}

#[test]
fn a_whole_answer_many_pages_share_is_gzipped_once() {
    let d = live_app("gzip");
    let r = run(&d, &[]);
    let raw = |r: &Run| {
        let mut s = TcpStream::connect(("127.0.0.1", r.port)).unwrap();
        write!(s, "POST /_quillo/rpc/big HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAccept-Encoding: gzip\r\nSec-Fetch-Site: same-origin\r\nX-Quillo-Live: 1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]").unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let at = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&out[..at]).to_ascii_lowercase(), out[at + 4..].to_vec())
    };
    let (head, first) = raw(&r);
    let (head2, second) = raw(&r);
    assert!(head.contains("content-encoding: gzip") && head2.contains("content-encoding: gzip"), "{}", head2);
    assert_eq!(first, second);
    let mut text = String::new();
    flate2::read::GzDecoder::new(&second[..]).read_to_string(&mut text).unwrap();
    assert!(text.contains("\"padding\""));
    assert_eq!(r.metric("quillo_live_runs_total{outcome=\"shared\"}"), 1);
}

/// Q-194: a page that leaves gives its place back within seconds, not at the next pings.
#[test]
fn a_page_that_left_is_noticed() {
    let d = live_app("left");
    let r = run(&d, &[]);
    let since = seq_of(&r);
    let page = open(r.port, since, "db:rows", None).unwrap();
    let dropped = open(r.port, since, "db:rows", None).unwrap();
    assert_eq!(r.metric("quillo_live_pages"), 2);
    page.leave();
    drop(dropped);
    let start = Instant::now();
    while r.metric("quillo_live_pages") != 0 {
        assert!(start.elapsed() < Duration::from_secs(8), "still {} pages", r.metric("quillo_live_pages"));
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Q-195: what an operator needs to see, in numbers, and enough descriptors.
#[test]
fn live_data_can_be_seen_from_outside() {
    let d = live_app("metrics");
    let r = run(&d, &[]);
    r.get("/fill");
    let one = r.refresh("rows", "");
    r.refresh("rows", "");
    let h = one["h"].as_str().unwrap().to_string();
    let had = |h: &str| send(r.port, "POST", "/_quillo/rpc/rows", &[("x-quillo-live", "1"), ("sec-fetch-site", "same-origin"), ("x-quillo-had", h)], "[]").body;
    assert!(had(&h).contains("\"patch\":{\"$=\":1}"));
    r.get("/write");
    assert!(had(&h).contains("\"patch\""));
    assert_eq!(r.metric("quillo_live_answers_total{kind=\"same\"}"), 1);
    assert_eq!(r.metric("quillo_live_answers_total{kind=\"patch\"}"), 1);
    assert_eq!(r.metric("quillo_live_answers_total{kind=\"whole\"}"), 2);
}

#[cfg(unix)]
#[test]
fn a_low_descriptor_limit_is_raised() {
    let d = live_app("descriptors");
    let port = support::free_port();
    let script = format!("ulimit -S -n 256; exec '{}' run '{}' --port {} --live-per-client 1000", env!("CARGO_BIN_EXE_quillo"), d.join("main.qwe").display(), port);
    let child = Command::new("sh").args(["-c", &script]).env("QUILLO_SECRET", "a test secret for live data, long enough to use").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    support::stop_with_tests(&child);
    let _server = support::Guard::new(child);
    let start = Instant::now();
    while !support::listening(port) {
        assert!(start.elapsed() < Duration::from_secs(20), "the server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Past the limit it started with, each a descriptor.
    let pages: Vec<Page> = (0..400).map(|i| open(port, 1, "db:nothing", None).unwrap_or_else(|| panic!("page {}", i))).collect();
    assert_eq!(pages.len(), 400);
}
