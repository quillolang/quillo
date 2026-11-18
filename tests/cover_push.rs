// Coverage of 0.8.7 (branch coverage-087), slice S3: what FEEDBACK67.md added for notifications,
// Web Push and installable apps that no test had run. `notification.user` and `.send` called
// wrongly, and their messages; the browser's compile of `notification`; the checker's rules where
// a variable hides the module; a push table the server cannot use (each use fails, saying why, and
// `set_session` and `end_session` refuse to go on); rows compared exactly whatever the column's
// collation; a send's bookkeeping, keys, rules and policy failing per device; an IPv6 loopback push
// service; the environment's proxy for https push services; the minute's window of tries moving
// and its table pruned; VAPID headers kept past a thousand; sessions too full for an id, cookies
// that cannot be judged and the cookies kept as opened; an app's files beside routes at their
// paths; the icon's size kept for 16 files; rows past --max-memory.

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A subscription's keys as a browser makes them: a P-256 public key, and a 16-byte secret.
const P256DH: &str = "BF5JjJmHYcp-lHVMIDyVSu7Q4fzLCDsfNqASRtVyiv5i8aoIztqkMiDtok0G5oXrmrnwzD1uoXEWEeXDg1_Yzpg";
const AUTH: &str = "swC8VcTjKc8fIaxz_rdm_Q";
/// Where a program at no mount point and of no tenant keeps its rows (`session::place`).
const PLACE: &str = r#"["",null]"#;
/// The table as Quillo makes it on SQLite (push.rs, `make_table`), for rows written here.
const TABLE: &str = "create table _quillo_push (endpoint varchar(1000) primary key, p256dh text not null, auth text not null, who text not null, session text not null, origin text not null, place text not null, created bigint not null, succeeded bigint not null)";

// ---- folders, servers, requests -----------------------------------------------------------------

/// A test's folder, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str, src: &str) -> Dir {
    let d = support::temp_dir(&format!("cover-push-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    Dir(d)
}

/// `quillo run` of a folder's main.qwe on the port the system gives, its log in server.log:
/// killed when dropped.
struct Run {
    child: std::process::Child,
    log: PathBuf,
    port: u16,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Run {
    /// The log, once it holds `want` (or after 10 s, as it is then).
    fn log_with(&self, want: &str) -> String {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let text = std::fs::read_to_string(&self.log).unwrap_or_default();
            if text.contains(want) || Instant::now() > end {
                return text;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The server, with QUILLO_SECRET and QUILLO_PUSH_LOCAL=1 (the push services here are on this
/// machine), then `env`.
fn run(d: &Path, opts: &[&str], env: &[(&str, &str)]) -> Run {
    let log = d.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", "0"])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .env("QUILLO_PUSH_LOCAL", "1")
        .envs(env.iter().copied())
        .current_dir(d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let mut r = Run { child, log, port: 0 };
    r.port = support::port_in_log(&mut r.child, &r.log, Duration::from_secs(20));
    r
}

/// What a command run to its end said: whether it succeeded, its stdout and its stderr.
struct Ran {
    ok: bool,
    out: String,
    err: String,
}

/// A command run to its end within `secs`, its output read as it comes (a full pipe would stop
/// it), stopped with the tests and killed at the deadline.
fn within(mut c: Command, secs: u64) -> Ran {
    let mut child = support::Guard::new(c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = r.read_to_end(&mut v);
            String::from_utf8_lossy(&v).into_owned()
        })
    };
    let (out, err) = (read(Box::new(child.0.stdout.take().unwrap())), read(Box::new(child.0.stderr.take().unwrap())));
    let end = Instant::now() + Duration::from_secs(secs);
    let ok = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status.success();
        }
        assert!(Instant::now() < end, "{:?} did not end within {} s", c, secs);
        std::thread::sleep(Duration::from_millis(10));
    };
    Ran { ok, out: out.join().unwrap(), err: err.join().unwrap() }
}

/// `quillo check main.qwe` in `d`: whether it passed, and its error and warning lines.
fn check_lines(d: &Path) -> (bool, Vec<String>) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["check", "main.qwe"]).current_dir(d);
    let o = within(c, 30);
    let text = o.out + &o.err;
    (o.ok, text.lines().filter(|l| l.contains(": error: ") || l.contains(": warning: ")).map(String::from).collect())
}

/// The session cookie an answer sets (`name=value`), if it sets one.
fn session_cookie(a: &support::Answer) -> Option<String> {
    a.headers.iter().find(|(k, v)| k == "set-cookie" && v.contains("quillo_session=")).map(|(_, v)| v.split(';').next().unwrap().to_string())
}

/// Signs `who` in (`GET /in/<who>`, with `cookie` when given): the session cookie.
fn sign_in(port: u16, who: &str) -> String {
    let a = support::send(port, "GET", &format!("/in/{}", who), &[], "");
    assert_eq!(a.status, 303, "{}", a.body);
    session_cookie(&a).expect("a session cookie")
}

/// A request's headers: its Host, and the cookie when there is one.
fn with_cookie<'a>(cookie: &'a str, host: &'a str) -> Vec<(&'a str, &'a str)> {
    let mut hs = vec![("host", host)];
    if !cookie.is_empty() {
        hs.push(("cookie", cookie));
    }
    hs
}

/// What a page posts to /_quillo/push for a subscription at `endpoint`, the visitor's session in
/// `cookie`, from the page's own site.
fn bind(port: u16, endpoint: &str, cookie: &str) -> support::Answer {
    bind_keys(port, endpoint, P256DH, AUTH, cookie)
}

/// `bind`, with the subscription's keys given.
fn bind_keys(port: u16, endpoint: &str, p256dh: &str, auth: &str, cookie: &str) -> support::Answer {
    let host = format!("127.0.0.1:{}", port);
    let body = serde_json::json!({ "endpoint": endpoint, "keys": { "p256dh": p256dh, "auth": auth }, "user": null });
    support::send(port, "POST", "/_quillo/push", &with_cookie(cookie, &host), &body.to_string())
}

/// `POST /send/<who>` with the message: what the route gave, as JSON.
fn send_to(port: u16, who: &str, msg: serde_json::Value) -> serde_json::Value {
    let a = support::send(port, "POST", &format!("/send/{}", who), &[], &msg.to_string());
    assert_eq!(a.status, 200, "{}", a.body);
    serde_json::from_str(&a.body).unwrap_or_else(|_| panic!("{}", a.body))
}

/// A program that signs people in and out, and sends to them.
const PUSHER: &str = r#"route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route GET /out {
    end_session()
    redirect("/")
}
route POST /send/:who {
    let r = notification.send(who, request.json)
    return is_err(r) ? { error: r.error } : r
}
route GET / = "home"
"#;

/// The system database of `d`, made here with `sql` before the server opens it.
fn system_db(d: &Path, sql: &str) -> rusqlite::Connection {
    let c = rusqlite::Connection::open(d.join("quillo-system.db")).unwrap();
    c.execute_batch(sql).unwrap();
    c
}

/// A row of `_quillo_push` written here: the endpoint, its p256dh, for `who` (as a send names it:
/// JSON), made on `origin`.
fn row(c: &rusqlite::Connection, endpoint: &str, p256dh: &str, who: &str, origin: &str, created: i64) {
    c.execute(
        "insert into _quillo_push (endpoint, p256dh, auth, who, session, origin, place, created, succeeded) values (?1, ?2, ?3, ?4, 'id s', ?5, ?6, ?7, 0)",
        rusqlite::params![endpoint, p256dh, AUTH, who, origin, PLACE, created],
    )
    .unwrap();
}

fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..c.len() + 1 {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn unb64url(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        _ => 63,
    };
    let mut out = Vec::new();
    for c in s.as_bytes().chunks(4) {
        let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | (val(*b) as u32) << (18 - 6 * i));
        for i in 0..c.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// The claims of a VAPID header's token (`vapid t=<JWT>, k=<key>`).
fn vapid_claims(auth: &str) -> serde_json::Value {
    let t = auth.strip_prefix("vapid t=").and_then(|r| r.split(',').next()).unwrap();
    serde_json::from_slice(&unb64url(t.split('.').nth(1).unwrap())).unwrap()
}

/// A browser's subscription, made here: its P-256 key, which reads one message sent to it.
struct Browser {
    key: ring::agreement::EphemeralPrivateKey,
    p256dh: Vec<u8>,
    auth: [u8; 16],
}

fn browser() -> Browser {
    let rng = ring::rand::SystemRandom::new();
    let key = ring::agreement::EphemeralPrivateKey::generate(&ring::agreement::ECDH_P256, &rng).unwrap();
    let p256dh = key.compute_public_key().unwrap().as_ref().to_vec();
    let mut auth = [0u8; 16];
    ring::rand::SecureRandom::fill(&rng, &mut auth).unwrap();
    Browser { key, p256dh, auth }
}

struct Len(usize);

impl ring::hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, salt).extract(ikm).expand(&[info], Len(n)).unwrap().fill(&mut out).unwrap();
    out
}

/// A message as the browser reads it (RFC 8291, aes128gcm of RFC 8188): the record size and the
/// sender's key in the header, and the record opened with what the browser and the sender agree.
fn decrypt(b: Browser, body: &[u8]) -> (u32, Vec<u8>, Vec<u8>) {
    let (salt, size, id) = (&body[..16], u32::from_be_bytes(body[16..20].try_into().unwrap()), body[20] as usize);
    let sender = body[21..21 + id].to_vec();
    let shared = ring::agreement::agree_ephemeral(b.key, &ring::agreement::UnparsedPublicKey::new(&ring::agreement::ECDH_P256, &sender), |k| k.to_vec()).unwrap();
    let ikm = hkdf(&b.auth, &shared, &[&b"WebPush: info\0"[..], &b.p256dh, &sender].concat(), 32);
    let cek = hkdf(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
    let nonce = hkdf(salt, &ikm, b"Content-Encoding: nonce\0", 12);
    let key = ring::aead::LessSafeKey::new(ring::aead::UnboundKey::new(&ring::aead::AES_128_GCM, &cek).unwrap());
    let mut record = body[21 + id..].to_vec();
    let plain = key.open_in_place(ring::aead::Nonce::try_assume_unique_for_key(&nonce).unwrap(), ring::aead::Aad::empty(), &mut record).expect("the record opens").to_vec();
    (size, sender, plain)
}

// ---- a push service and a proxy, played here ----------------------------------------------------

/// What a push service was sent: the path, and the headers that matter.
#[derive(Clone, Debug)]
struct Got {
    path: String,
    auth: String,
    encoding: String,
    body: Vec<u8>,
}

/// A push service on this machine: 201 to each message, 404 or 410 where the path says so.
struct Sink {
    port: u16,
    /// As a URL writes it: `127.0.0.1` or `[::1]`.
    host: String,
    got: Arc<Mutex<Vec<Got>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let at = if self.host.starts_with('[') { "::1" } else { "127.0.0.1" };
        let _ = TcpStream::connect((at, self.port));
    }
}

impl Sink {
    fn endpoint(&self, path: &str) -> String {
        format!("http://{}:{}{}", self.host, self.port, path)
    }

    fn got(&self) -> Vec<Got> {
        self.got.lock().unwrap().clone()
    }

    fn reset(&self) {
        self.got.lock().unwrap().clear();
    }
}

/// The head of a request on `s` (None at its end), and its body by its content-length.
fn read_request(s: &mut TcpStream, buf: &mut Vec<u8>) -> Option<(String, Vec<(String, String)>, Vec<u8>)> {
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let mut chunk = [0u8; 8192];
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let length = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0usize);
    while buf.len() < head_end + 4 + length {
        let mut chunk = [0u8; 8192];
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = buf[head_end + 4..head_end + 4 + length].to_vec();
    buf.drain(..head_end + 4 + length);
    Some((first, headers, body))
}

/// The sink, listening on `addr` (`127.0.0.1` or `::1`); None where this machine has no such
/// address.
fn sink_on(addr: &str) -> Option<Sink> {
    let l = TcpListener::bind((addr, 0)).ok()?;
    let port = l.local_addr().unwrap().port();
    let got = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (g, st) = (got.clone(), stop.clone());
    std::thread::spawn(move || {
        for conn in l.incoming() {
            if st.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let Ok(mut s) = conn else { continue };
            let g = g.clone();
            std::thread::spawn(move || {
                let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
                let mut buf = Vec::new();
                while let Some((first, headers, body)) = read_request(&mut s, &mut buf) {
                    let path = first.split(' ').nth(1).unwrap_or("").to_string();
                    let h = |k: &str| headers.iter().find(|(n, _)| n == k).map_or(String::new(), |(_, v)| v.clone());
                    let status = if path.contains("/404-") { "404 Not Found" } else if path.contains("/410-") { "410 Gone" } else { "201 Created" };
                    g.lock().unwrap().push(Got { path, auth: h("authorization"), encoding: h("content-encoding"), body });
                    if s.write_all(format!("HTTP/1.1 {}\r\ncontent-length: 0\r\n\r\n", status).as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    let host = if addr.contains(':') { format!("[{}]", addr) } else { addr.to_string() };
    Some(Sink { port, host, got, stop })
}

fn sink() -> Sink {
    sink_on("127.0.0.1").expect("a listener on 127.0.0.1")
}

/// A proxy on this machine that refuses every tunnel: the first lines of what it was asked, in
/// order. It stops listening when dropped.
struct Proxy {
    port: u16,
    asked: Arc<Mutex<Vec<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn refusing_proxy() -> Proxy {
    let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (a, st) = (asked.clone(), stop.clone());
    std::thread::spawn(move || {
        for conn in l.incoming() {
            if st.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            let Ok(mut s) = conn else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
            let mut buf = Vec::new();
            if let Some((first, _, _)) = read_request(&mut s, &mut buf) {
                a.lock().unwrap().push(first);
            }
            let _ = s.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        }
    });
    Proxy { port, asked, stop }
}

/// Requests on one kept connection (a new one where the server closed it): each answer's status.
struct KeptConn {
    port: u16,
    s: Option<TcpStream>,
    buf: Vec<u8>,
}

impl KeptConn {
    fn new(port: u16) -> KeptConn {
        KeptConn { port, s: None, buf: Vec::new() }
    }

    fn status(&mut self, raw: &str) -> u16 {
        for _ in 0..3 {
            if self.s.is_none() {
                let s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
                self.s = Some(s);
                self.buf.clear();
            }
            let s = self.s.as_mut().unwrap();
            if s.write_all(raw.as_bytes()).is_ok() {
                if let Some((first, headers, _)) = read_request(s, &mut self.buf) {
                    if headers.iter().any(|(k, v)| k == "connection" && v.eq_ignore_ascii_case("close")) {
                        self.s = None;
                    }
                    return first.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
                }
            }
            self.s = None;
        }
        panic!("no answer from the server");
    }
}

// ---- notification on the server, called wrongly -------------------------------------------------

/// `notification.user` and `.send`, each called wrongly where the checker cannot see it: handed to
/// a component as a value, through a variable, with what a request brings.
const WRONG: &str = r#"component Sealer(f: fn) {
    let t = f("x")
    <p>{t}</p>
}
component Sender(f: fn) {
    let r = f("x", { title: "t" })
    <p>{r}</p>
}
let tell = notification.send
let seal = notification.user
route GET /seal-in-a-component = <html><body><Sealer f={notification.user}/></body></html>
route GET /send-in-a-component = <html><body><Sender f={notification.send}/></body></html>
route GET /seal-null = notification.user(null)
route GET /seal-nothing = seal()
route GET /send-to-null = notification.send(null, { title: "t" })
route GET /send-nothing = tell("ana")
route POST /send = notification.send("ana", request.json)
route GET / = "home"
"#;

/// `notification.user` with no one (none, null) or while a component renders on the server, and
/// `notification.send` there, to no one, with a message that is no map (text, a list, null, a
/// number, nothing) or whose `silent` is no boolean (text, a number), each stop the request with
/// its 500, and the log says what to do at the call's line; a `silent` of false or null is taken
/// (src/push.rs `user`, `send`, `message`, FEEDBACK67.md B.3; coverage of 0.8.7; branch coverage-087).
#[test]
fn server_calls_given_wrongly_stop_the_request_saying_why() {
    let d = dir("wrong", WRONG);
    let s = run(&d, &[], &[]);
    let map = |got: &str| format!("`notification.send` takes the message as a map ({{ title, body, url }}), got {}", got);
    let silent = |got: &str| format!("`notification.send`'s `silent` is true or false, got {}", got);
    let no_one = "`notification.user` is given who the device is for: an id, as `notification.send` is given it".to_string();
    let cases: Vec<(&str, &str, &str, u32, String)> = vec![
        ("GET", "/seal-in-a-component", "", 2, "`notification.user` seals on the server: make it in a route or a `server fn`, and give it to the page".into()),
        ("GET", "/send-in-a-component", "", 6, "`notification.send` sends from the server; a component runs in the browser too: call it from a `server fn`".into()),
        ("GET", "/seal-null", "", 13, no_one.clone()),
        ("GET", "/seal-nothing", "", 14, no_one),
        ("GET", "/send-to-null", "", 15, "`notification.send` is given who to send to first: the user, as `session().user` names them".into()),
        ("GET", "/send-nothing", "", 16, map("nothing")),
        ("POST", "/send", "\"Grüße ✓\"", 17, map("str")),
        ("POST", "/send", "[1, 2]", 17, map("list")),
        ("POST", "/send", "null", 17, map("null")),
        ("POST", "/send", "3", 17, map("int")),
        ("POST", "/send", r#"{"title":"t","silent":"yes"}"#, 17, silent("str")),
        ("POST", "/send", r#"{"title":"t","silent":1}"#, 17, silent("int")),
    ];
    for (method, path, body, line, says) in &cases {
        let a = support::send(s.port, method, path, &[], body);
        assert_eq!((a.status, a.body.as_str()), (500, "internal error\n"), "{} {} {}", method, path, body);
        let want = format!("main.qwe:{}: {}", line, says);
        assert!(s.log_with(&want).contains(&want), "{} {}: {}", path, body, want);
    }
    // Each said once a call.
    let log = s.log_with("");
    for says in [map("str"), map("list"), map("null"), map("int"), silent("str"), silent("int")] {
        assert_eq!(log.matches(&format!("main.qwe:17: {}", says)).count(), 1, "{}", says);
    }
    for body in [r#"{"title":"t","silent":false}"#, r#"{"title":"t","silent":null}"#, r#"{"title":"t","silent":true}"#] {
        let a = support::send(s.port, "POST", "/send", &[], body);
        assert_eq!((a.status, a.body.as_str()), (200, r#"{"sent":0,"gone":0}"#), "{}", body);
    }
}

// ---- notification in the browser's code ---------------------------------------------------------

/// As a program is compiled for the browser (run without the check): a `notification` function
/// there is none of is refused, and so are `ask`, `state` and `stop` in a component's body, which
/// the server renders first, `state` being told it may set a state instead; `state` setting a state
/// is taken. `quillo check` says the same of each first (src/client.rs `notification_call`,
/// FEEDBACK67.md B.3; coverage of 0.8.7; branch coverage-087).
#[test]
fn the_browsers_compile_says_what_notification_has_and_where() {
    let in_body = |f: &str, or: &str| format!("`notification.{}` in a component runs in the browser, and the server renders the component first: call it from an event handler (@click){}", f, or);
    let checked = |f: &str, or: &str| format!("`notification.{}` in <C> runs in the browser, and the server renders <C> first: call it from an event handler (@click){}", f, or);
    let state_or = ", or set a state with it: state on = notification.state()";
    let cases = [
        ("none", "component C() {\n    state on = \"\"\n    <button @click={() => { on = notification.push() }}>{on}</button>\n}\n", 3, "`notification` has no function `push`".to_string(), "`notification` has no function `push`".to_string()),
        ("tōdai", "component C() {\n    state on = \"\"\n    <button @click={() => { on = notification.tōdai() }}>{on}</button>\n}\n", 3, "`notification` has no function `tōdai`".to_string(), "`notification` has no function `tōdai`".to_string()),
        ("state", "component C() {\n    let b = notification.state()\n    <p>{b}</p>\n}\n", 2, in_body("state", state_or), checked("state", state_or)),
        ("ask", "component C() {\n    let b = notification.ask()\n    <p>{b}</p>\n}\n", 2, in_body("ask", ""), checked("ask", "")),
        ("stop", "component C() {\n    state a = notification.stop()\n    <p>{a}</p>\n}\n", 2, in_body("stop", ""), checked("stop", "")),
    ];
    for (name, comp, line, compiled, check_says) in cases {
        let d = dir(&format!("client-{}", name), &format!("{}route GET / = <html><body><C/></body></html>\n", comp));
        let (ok, lines) = check_lines(&d);
        assert!(!ok, "{}", name);
        assert_eq!(lines.first().map(String::as_str), Some(format!("main.qwe:{}: error: {}", line, check_says).as_str()), "{}: {:?}", name, lines);
        let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.args(["run", "main.qwe", "--no-check", "--port", "0"]).env("QUILLO_SECRET", "test secret for the tests").current_dir(&*d);
        let o = within(c, 30);
        let log = o.err;
        assert!(!o.ok, "{} started: {}", name, log);
        assert!(log.contains(&format!("main.qwe:{}: <C> (compiled for the browser): {}", line, compiled)), "{}: {}", name, log);
    }
    // `state` setting a state compiles, and the server renders null for it.
    let d = dir("client-ok", "component C() {\n    state a = notification.state()\n    <p id=\"a\">[{a ?? \"null\"}]</p>\n}\nroute GET / = <html><body><C/></body></html>\n");
    assert_eq!(check_lines(&d), (true, vec![]));
    let s = run(&d, &[], &[]);
    let a = support::send(s.port, "GET", "/", &[], "");
    assert!(a.status == 200 && a.body.contains("<p id=\"a\">[null]</p>"), "{} {}", a.status, a.body);
}

// ---- the checker, where a variable hides the module ---------------------------------------------

/// A parameter named `notification` hides the module: `quillo check` says so at each use, and says
/// nothing of the module's own rules there (a send in a transaction, an ask after a wait), which
/// would be false of the program's own `send` and `ask`; with the module, both rules speak
/// (src/check.rs `sends_in_a_transaction`, `ask_after_wait`, FEEDBACK67.md B.3; coverage of 0.8.7;
/// branch coverage-087).
#[test]
fn check_leaves_a_hidden_module_to_its_own_rule() {
    let hidden = r#"fn shipped(notification, who: str) = db.transaction(() => notification.send(who, { title: "t" }))
route POST /a = shipped({ send: (w, m) => 1 }, "ana")
server fn save() -> int = 1
component C(notification) {
    state on = ""
    <button @click={() => {
        on = str(save())
        on = notification.ask()
    }}>{on}</button>
}
route GET / = <html><body><C notification={{ ask: () => "mine" }}/></body></html>
"#;
    let hides = |line: u32, f: &str| format!("main.qwe:{}: error: `notification.{}`: `notification` here is any, a variable, which hides the module `notification`: rename the variable", line, f);
    assert_eq!(check_lines(&dir("check-hidden", hidden)), (false, vec![hides(1, "send"), hides(8, "ask")]));
    let module = hidden.replace("fn shipped(notification, who: str)", "fn shipped(n, who: str)").replace("component C(notification)", "component C(n)").replace("notification={{", "n={{");
    let (ok, lines) = check_lines(&dir("check-module", &module));
    assert!(ok, "{:?}", lines);
    assert_eq!(
        lines,
        [
            "main.qwe:1: warning: `notification.send` waits on push services, which inside a transaction holds up every other request (a SQLite transaction keeps the interpreter's lock), and sends even when the transaction is rolled back: call it after the transaction",
            "main.qwe:8: warning: `notification.ask` comes after `save` waits in this handler: browsers show the permission prompt only close to the click, so ask first, then wait",
        ]
    );
}

// ---- a push table the server cannot use ----------------------------------------------------------

/// A `_quillo_push` left by something else, without the `place` column: each use fails and says
/// why. A bind is a 500 ("internal error" to the page, the database's words in the log, the index
/// that could not be made too), and so are a page's look and a `stop`; a send is an error value
/// naming the database's error. A change of person in `set_session` and an `end_session` stop the
/// request rather than leave the next person someone else's notifications, keeping the session as
/// it was; the same person again, and a sign-out with no session, need nothing of the table
/// (src/push.rs `sql`, `indexed`, `answer_push`, `bind`, `send`, `unbind_sessions`; src/session.rs
/// `set_session`, `end_session`; FEEDBACK67.md B.3; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_push_table_that_cannot_be_used_fails_each_use_saying_why() {
    let d = dir("broken", PUSHER);
    assert_eq!(check_lines(&d), (true, vec![]));
    system_db(&d, "create table _quillo_push (endpoint varchar(1000) primary key, p256dh text not null, auth text not null, who text not null, session text not null, origin text not null, created bigint not null, succeeded bigint not null)");
    let s = run(&d, &[], &[]);
    let port = s.port;
    let host = format!("127.0.0.1:{}", port);
    let no_place = |sql: &str, at: u32| format!("db: no such column: place in {} at offset {}", sql, at);
    // A sign-out with no session unbinds nothing, and so does not fail.
    let a = support::send(port, "GET", "/out", &[], "");
    assert_eq!(a.status, 303, "{}", a.body);
    let ana = sign_in(port, "ana");
    let a = bind(port, "http://127.0.0.1:9/push/a", &ana);
    assert_eq!((a.status, a.body.as_str()), (500, r#"{"error":"internal error"}"#));
    let index = format!("quillo: _quillo_push: an index could not be made ({}): sends and sign-outs read the whole table", no_place("create index if not exists _quillo_push_who on _quillo_push (who, place)", 66));
    assert!(s.log_with(&index).contains(&index), "{}", s.log_with(""));
    assert!(s.log_with("no column named").contains("quillo: /_quillo/push: db: table _quillo_push has no column named place"));
    let look = support::send(port, "POST", "/_quillo/push", &with_cookie(&ana, &host), r#"{"endpoint":"http://127.0.0.1:9/push/a","check":true}"#);
    assert_eq!((look.status, look.body.as_str()), (500, r#"{"error":"internal error"}"#));
    let said = format!("quillo: /_quillo/push: {}", no_place("select who, session, place from _quillo_push where endpoint = ?", 21));
    assert!(s.log_with(&said).contains(&said));
    let stop = support::send(port, "DELETE", "/_quillo/push", &with_cookie(&ana, &host), r#"{"endpoint":"http://127.0.0.1:9/push/a"}"#);
    assert_eq!((stop.status, stop.body.as_str()), (500, r#"{"error":"internal error"}"#));
    let said = format!("quillo: /_quillo/push: {}", no_place("delete from _quillo_push where endpoint = ? and place = ? and (session = ? or who = ?)", 48));
    assert!(s.log_with(&said).contains(&said));
    let r = send_to(port, "ana", serde_json::json!({ "title": "t" }));
    assert_eq!(r, serde_json::json!({ "error": format!("notification.send: {}", no_place("select endpoint, p256dh, auth, who, origin, place from _quillo_push where who = ? and place = ?", 44)) }));
    // Bob on Ana's browser: refused, her session kept.
    let unbind = no_place("delete from _quillo_push where session = ? and place = ?", 47);
    let a = support::send(port, "GET", "/in/bob", &[("cookie", &ana)], "");
    assert_eq!((a.status, session_cookie(&a)), (500, None), "{}", a.body);
    let said = format!("main.qwe:2: `set_session`: the notifications of the person before could not be turned off on this device, so the session was not changed: {}", unbind);
    assert!(s.log_with(&said).contains(&said));
    // Ana again: the same person, nothing to unbind.
    let a = support::send(port, "GET", "/in/ana", &[("cookie", &ana)], "");
    assert_eq!(a.status, 303);
    let a = support::send(port, "GET", "/out", &[("cookie", &ana)], "");
    assert_eq!((a.status, session_cookie(&a)), (500, None), "{}", a.body);
    let said = format!("main.qwe:6: `end_session`: this device's notifications could not be turned off, so the session was not ended: {}", unbind);
    assert!(s.log_with(&said).contains(&said));
}

/// A table whose `who` compares without case (a column made `collate nocase`) still keeps Ana's
/// and ANA's devices apart: a send to one reaches only that one's, and ANA's 20 devices are not
/// counted as Ana's when she binds one (none of them replaced as her oldest) (src/push.rs
/// `subscriptions`, `keep_newest`, FEEDBACK67.md B.3; coverage of 0.8.7; branch coverage-087).
#[test]
fn rows_are_compared_exactly_whatever_the_columns_collation() {
    let d = dir("nocase", PUSHER);
    system_db(&d, &TABLE.replace("who text not null", "who text collate nocase not null"));
    let sink = sink();
    let s = run(&d, &[], &[]);
    let upper = sign_in(s.port, "ANA");
    for i in 0..20 {
        assert_eq!(bind(s.port, &sink.endpoint(&format!("/push/ok-ANA-{}", i)), &upper).status, 200);
    }
    let lower = sign_in(s.port, "ana");
    assert_eq!(bind(s.port, &sink.endpoint("/push/ok-ana"), &lower).status, 200);
    let one = serde_json::json!({ "title": "t" });
    assert_eq!(send_to(s.port, "ana", one.clone()), serde_json::json!({ "sent": 1, "gone": 0 }));
    assert_eq!(sink.got().iter().map(|g| g.path.clone()).collect::<Vec<_>>(), ["/push/ok-ana"]);
    sink.reset();
    assert_eq!(send_to(s.port, "ANA", one.clone()), serde_json::json!({ "sent": 20, "gone": 0 }));
    assert!(sink.got().iter().all(|g| g.path.starts_with("/push/ok-ANA-")), "{:?}", sink.got());
    assert_eq!(send_to(s.port, "Ana", one), serde_json::json!({ "sent": 0, "gone": 0 }));
}

// ---- what fails as a message is sent ------------------------------------------------------------

/// Per device, as a message is sent: its row's success that cannot be written is said in the log
/// and the device still counted as sent; a kept p256dh that is no point of the curve, an endpoint
/// the rules no longer take (QUILLO_PUSH_LOCAL gone at a restart), and a host the policy does not
/// allow are each a line naming the push service's host alone, nothing sent (src/push.rs `send`,
/// `deliver`, FEEDBACK67.md B.3; coverage of 0.8.7; branch coverage-087).
#[test]
fn what_fails_for_one_device_is_said_in_the_log() {
    let d = dir("deliver", PUSHER);
    let sink = sink();
    {
        let c = system_db(&d, &TABLE.replace(", succeeded bigint not null", ""));
        let off_curve = { let mut k = vec![4u8]; k.extend([1u8; 64]); b64url(&k) };
        for (path, key) in [("/push/ok-1", P256DH.to_string()), ("/push/ok-2", off_curve)] {
            c.execute(
                "insert into _quillo_push (endpoint, p256dh, auth, who, session, origin, place, created) values (?1, ?2, ?3, '\"ana\"', 'id s', 'http://shop.example', ?4, 1)",
                rusqlite::params![sink.endpoint(path), key, AUTH, PLACE],
            )
            .unwrap();
        }
    }
    let one = serde_json::json!({ "title": "t" });
    let s = run(&d, &[], &[]);
    assert_eq!(send_to(s.port, "ana", one.clone()), serde_json::json!({ "sent": 1, "gone": 0 }));
    assert_eq!(sink.got().iter().map(|g| (g.path.as_str(), g.encoding.as_str())).collect::<Vec<_>>(), [("/push/ok-1", "aes128gcm")]);
    let log = s.log_with("not a P-256 public key");
    assert!(log.contains("quillo: notification.send: 127.0.0.1: the subscription's p256dh is not a P-256 public key"), "{}", log);
    let log = s.log_with("no such column: succeeded");
    assert!(log.lines().any(|l| l == "quillo: notification.send: db: no such column: succeeded"), "{}", log);
    assert!(!log.contains("/push/ok-"), "an endpoint in the log: {}", log);
    drop(s);
    sink.reset();
    // Started again without QUILLO_PUSH_LOCAL: the kept endpoints are refused as they are sent to.
    let s = run(&d, &[], &[("QUILLO_PUSH_LOCAL", "0")]);
    assert_eq!(send_to(s.port, "ana", one.clone()), serde_json::json!({ "sent": 0, "gone": 0 }));
    let said = "quillo: notification.send: 127.0.0.1: the endpoint is plain http to this machine, which is taken only with QUILLO_PUSH_LOCAL=1 (a test's push service)";
    let log = s.log_with(said);
    assert_eq!(log.matches(said).count(), 2, "{}", log);
    drop(s);
    // A policy beside the program that allows one host alone.
    std::fs::write(d.join("quillo.policy"), "[http]\nhosts = [\"push.example.org\"]\n").unwrap();
    let s = run(&d, &[], &[]);
    assert_eq!(send_to(s.port, "ana", one), serde_json::json!({ "sent": 0, "gone": 0 }));
    let log = s.log_with("allows: push.example.org");
    let lines: Vec<&str> = log.lines().filter(|l| l.starts_with("quillo: notification.send:")).collect();
    assert_eq!(lines.len(), 2, "{}", log);
    for l in lines {
        assert!(l.starts_with("quillo: notification.send: 127.0.0.1: 127.0.0.1 is not among the hosts ") && l.ends_with(" allows: push.example.org"), "{}", l);
    }
    assert!(sink.got().is_empty(), "{:?}", sink.got());
}

/// A push service on this machine's IPv6 loopback (`http://[::1]:port`) is taken as 127.0.0.1's
/// is: refused without QUILLO_PUSH_LOCAL=1, bound and sent to with it, VAPID's `aud` its origin
/// with the brackets; what arrives opens with the browser's key, one record of 4096 holding the
/// message and the delimiter 2 (src/push.rs `loopback`, FEEDBACK67.md B.3; coverage of 0.8.7;
/// branch coverage-087).
#[test]
fn an_ipv6_loopback_push_service_is_this_machines() {
    let Some(sink) = sink_on("::1") else {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    };
    let endpoint = sink.endpoint("/push/ok-v6");
    let d = dir("ipv6", PUSHER);
    let s = run(&d, &[], &[("QUILLO_PUSH_LOCAL", "0")]);
    let ana = sign_in(s.port, "ana");
    let a = bind(s.port, &endpoint, &ana);
    assert_eq!(a.status, 400, "{}", a.body);
    assert!(a.body.contains("the endpoint is plain http to this machine, which is taken only with QUILLO_PUSH_LOCAL=1 (a test's push service)"), "{}", a.body);
    drop(s);
    let s = run(&d, &[], &[]);
    let ana = sign_in(s.port, "ana");
    let b = browser();
    assert_eq!(bind_keys(s.port, &endpoint, &b64url(&b.p256dh), &b64url(&b.auth), &ana).status, 200);
    assert_eq!(send_to(s.port, "ana", serde_json::json!({ "title": "über IPv6" })), serde_json::json!({ "sent": 1, "gone": 0 }));
    let got = sink.got();
    assert_eq!(got.len(), 1, "{:?}", got);
    assert_eq!((got[0].path.as_str(), got[0].encoding.as_str()), ("/push/ok-v6", "aes128gcm"));
    let message = format!(r#"{{"web_push":8030,"notification":{{"title":"über IPv6","navigate":"http://127.0.0.1:{}/"}}}}"#, s.port);
    assert_eq!(got[0].body.len(), 86 + message.len() + 1 + 16);
    let (size, sender, plain) = decrypt(b, &got[0].body);
    assert_eq!((size, sender.len(), sender[0]), (4096, 65, 4));
    assert_eq!(plain, [message.as_bytes(), &[2]].concat(), "{}", String::from_utf8_lossy(&plain));
    let claims = vapid_claims(&got[0].auth);
    assert_eq!(claims["aud"], format!("http://[::1]:{}", sink.port));
    assert_eq!(claims["sub"], format!("http://127.0.0.1:{}", s.port));
}

/// An https push service is reached through the environment's proxy (HTTPS_PROXY), as `http.*`
/// goes: its address judged here first (an IP of the public internet, IPv4 and IPv6), then a tunnel
/// asked of the proxy to it on 443; a proxy that refuses is one line in the log naming the host;
/// a proxy the client cannot use is said as the environment's (src/push.rs `agent`, `post`,
/// FEEDBACK67.md B.3; coverage of 0.8.7; branch coverage-087).
#[test]
fn the_environments_proxy_carries_an_https_push() {
    let proxy = refusing_proxy();
    let d = dir("proxy", PUSHER);
    let s = run(&d, &[], &[("HTTPS_PROXY", &format!("http://127.0.0.1:{}", proxy.port)), ("NO_PROXY", ""), ("QUILLO_PUSH_LOCAL", "0")]);
    let ana = sign_in(s.port, "ana");
    for e in ["https://93.184.216.34/push/v4", "https://[2606:4700:4700::1111]/push/v6"] {
        assert_eq!(bind(s.port, e, &ana).status, 200, "{}", e);
    }
    assert_eq!(send_to(s.port, "ana", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 0, "gone": 0 }));
    let mut tunnels: Vec<String> = proxy.asked.lock().unwrap().clone();
    tunnels.sort();
    tunnels.dedup();
    assert_eq!(tunnels, ["CONNECT 93.184.216.34:443 HTTP/1.1", "CONNECT [2606:4700:4700::1111]:443 HTTP/1.1"]);
    let log = s.log_with("quillo: notification.send: [2606");
    let lines: Vec<&str> = log.lines().filter(|l| l.starts_with("quillo: notification.send:")).collect();
    assert_eq!(lines.len(), 2, "{}", log);
    assert!(lines.iter().any(|l| l.starts_with("quillo: notification.send: 93.184.216.34: ")), "{:?}", lines);
    assert!(lines.iter().any(|l| l.starts_with("quillo: notification.send: [2606:4700:4700::1111]: ")), "{:?}", lines);
    assert!(!log.contains("/push/v"), "an endpoint in the log: {}", log);
    drop(s);
    let s = run(&d, &[], &[("HTTPS_PROXY", "gopher://127.0.0.1:1"), ("NO_PROXY", ""), ("QUILLO_PUSH_LOCAL", "0")]);
    assert_eq!(send_to(s.port, "ana", serde_json::json!({ "title": "t" })), serde_json::json!({ "sent": 0, "gone": 0 }));
    let log = s.log_with("the proxy in the environment");
    assert!(log.lines().any(|l| l.starts_with("quillo: notification.send: 93.184.216.34: the proxy in the environment: ")), "{}", log);
}

// ---- the minute's window, and the table of tries ----------------------------------------------

fn look(port: u16, from: &str) -> String {
    let body = r#"{"endpoint":"https://push.example.com/send/x","check":true}"#;
    format!("POST /_quillo/push HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Forwarded-For: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", port, from, body.len(), body)
}

/// A page's looks at its subscription are held to 60 a minute by the client's address (through a
/// `--trust-proxy` proxy, its X-Forwarded-For): the 61st is a 429, and a refused try is not
/// counted. Past 10,000 addresses the table of tries is pruned of those that tried nothing in the
/// last minute, and an address still within its minute stays refused. The window moves: once a
/// minute has gone by, an address whose last try was within it has its older tries dropped and
/// keeps the recent one (59 more), and one that tried nothing in it starts again (60). It costs
/// about 61 s of wall time: the window is a minute of the clock, which no test can move (there is
/// no test clock), so the test waits it out (src/push.rs `too_often`, FEEDBACK67.md B.3; coverage
/// of 0.8.7; branch coverage-087).
#[test]
fn the_minutes_window_moves_and_the_table_of_tries_is_pruned() {
    let d = dir("window", PUSHER);
    let s = run(&d, &["--trust-proxy", "127.0.0.1"], &[]);
    let mut c = KeptConn::new(s.port);
    let (old, straddling) = ("203.0.113.7", "2001:db8::7");
    let looks = |c: &mut KeptConn, from: &str, n: usize| -> Vec<u16> { (0..n).map(|_| c.status(&look(s.port, from))).collect() };
    let t0 = Instant::now();
    assert_eq!(looks(&mut c, old, 60), [200; 60]);
    assert_eq!(looks(&mut c, straddling, 59), [200; 59]);
    let first_tries = Instant::now();
    let a = support::send(s.port, "POST", "/_quillo/push", &[("x-forwarded-for", old)], r#"{"endpoint":"https://push.example.com/send/x","check":true}"#);
    assert_eq!(a.status, 429);
    assert!(a.body.contains("more than 60 looks at a subscription in a minute from this address"), "{}", a.body);
    // Other addresses are counted apart: 10,001 of them, once each.
    for i in 0..10_001u32 {
        let from = format!("10.{}.{}.{}", i >> 16, (i >> 8) & 255, i & 255);
        assert_eq!(c.status(&look(s.port, &from)), 200, "{}", from);
    }
    // Within the minute (10,001 requests take seconds, not one): pruned, the live windows kept.
    assert!(t0.elapsed() < Duration::from_secs(50), "the requests took {:?}", t0.elapsed());
    assert_eq!(looks(&mut c, old, 1), [429], "the pruning dropped a window still in its minute");
    // The straddling address's last try well inside the next minute's look back.
    std::thread::sleep((first_tries + Duration::from_secs(5)).saturating_duration_since(Instant::now()));
    assert_eq!(looks(&mut c, straddling, 2), [200, 429]);
    let wait = (first_tries + Duration::from_millis(60_500)).saturating_duration_since(Instant::now());
    std::thread::sleep(wait);
    let again = looks(&mut c, straddling, 60);
    assert_eq!(&again[..59], [200; 59], "its one try in the minute stays");
    assert_eq!(again[59], 429);
    assert_eq!(looks(&mut c, old, 61)[..], [vec![200; 60], vec![429]].concat()[..], "started again");
}

// ---- VAPID headers, kept ------------------------------------------------------------------------

/// A VAPID header is signed once for each push service and site, and kept: the same header for a
/// second message, and still with 911 kept. Past a thousand kept, they are dropped, and the next
/// message is signed anew. The 1,001 others are 11 people's 91 devices, so that no one send nears
/// the 30 s a send may take (src/push.rs `vapid_header`, FEEDBACK67.md B.3; coverage of 0.8.7;
/// branch coverage-087).
#[test]
fn vapid_headers_kept_past_a_thousand_are_signed_anew() {
    let d = dir("vapid", PUSHER);
    let sink = sink();
    {
        let c = system_db(&d, TABLE);
        c.execute_batch("begin").unwrap();
        row(&c, &sink.endpoint("/push/ok-first"), P256DH, "\"first\"", "http://first.example", 0);
        for i in 0..1001 {
            row(&c, &sink.endpoint(&format!("/push/ok-b{}", i)), P256DH, &format!("\"p{}\"", i / 91), &format!("http://b{}.example", i), i + 1);
        }
        c.execute_batch("commit").unwrap();
    }
    let s = run(&d, &[], &[]);
    let one = serde_json::json!({ "title": "t" });
    let first = |sink: &Sink| sink.got().iter().filter(|g| g.path == "/push/ok-first").map(|g| g.auth.clone()).last().unwrap();
    assert_eq!(send_to(s.port, "first", one.clone()), serde_json::json!({ "sent": 1, "gone": 0 }));
    let signed = first(&sink);
    assert_eq!(vapid_claims(&signed)["sub"], "http://first.example");
    assert_eq!(send_to(s.port, "first", one.clone()), serde_json::json!({ "sent": 1, "gone": 0 }));
    assert_eq!(first(&sink), signed, "kept for the second message");
    for p in 0..10 {
        assert_eq!(send_to(s.port, &format!("p{}", p), one.clone()), serde_json::json!({ "sent": 91, "gone": 0 }));
    }
    assert_eq!(send_to(s.port, "first", one.clone()), serde_json::json!({ "sent": 1, "gone": 0 }));
    assert_eq!(first(&sink), signed, "kept with 911 kept");
    assert_eq!(send_to(s.port, "p10", one.clone()), serde_json::json!({ "sent": 91, "gone": 0 }));
    let subs: std::collections::HashSet<String> = sink.got().iter().filter(|g| g.path.starts_with("/push/ok-b")).map(|g| vapid_claims(&g.auth)["sub"].as_str().unwrap().to_string()).collect();
    assert_eq!(subs.len(), 1001);
    assert_eq!(send_to(s.port, "first", one), serde_json::json!({ "sent": 1, "gone": 0 }));
    let again = first(&sink);
    assert_ne!(again, signed, "the kept headers were dropped past a thousand");
    assert_eq!(vapid_claims(&again)["sub"], "http://first.example");
}

// ---- sessions and the worker's marks ------------------------------------------------------------

const OFFLINE: &str = r#"route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route GET /fill/:n(int) {
    set_session({ pad: repeat("x", n) })
    "filled"
}
route offline GET /mine = <html><body><h1>{len(session().pad ?? session().user ?? "")}</h1></body></html>
route offline GET / = <html><body><h1>Home</h1></body></html>
"#;

fn tag_cookie(a: &support::Answer) -> Option<String> {
    a.headers.iter().find(|(k, v)| k == "set-cookie" && v.starts_with("quillo_tag=")).map(|(_, v)| v.clone())
}

/// The JSON a session cookie holds (its part before the signature).
fn cookie_json(cookie: &str) -> serde_json::Value {
    let value = cookie.split_once('=').unwrap().1;
    serde_json::from_slice(&unb64url(value.split('.').next().unwrap())).unwrap()
}

/// In a program with offline routes, a session too full for its hidden id (it fits only without
/// one) is never named for the worker: setting it takes `quillo_tag` away and says no session's
/// name, and a page of it that reads the session is not kept and names none, the browser's old
/// `quillo_tag` left alone; a smaller session is named and its page kept as private, and one too
/// large for a cookie is refused (src/session.rs `session`, `set_session`, `offline_tag`,
/// `tag_again`, FEEDBACK67.md B.2; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_session_too_full_for_an_id_is_never_named() {
    let d = dir("full", OFFLINE);
    assert_eq!(check_lines(&d), (true, vec![]));
    let s = run(&d, &[], &[]);
    let named = support::send(s.port, "GET", "/fill/2800", &[], "");
    let tag = named.header("x-quillo-session").expect("named").to_string();
    assert_eq!(tag.len(), 16);
    assert_eq!(tag_cookie(&named), Some(format!("quillo_tag={}; Path=/; Max-Age=2592000; Secure; SameSite=Lax", tag)));
    let small = session_cookie(&named).unwrap();
    assert!(cookie_json(&small)["i"].is_string());
    let full = support::send(s.port, "GET", "/fill/2843", &[], "");
    assert_eq!((full.status, full.body.as_str(), full.header("x-quillo-session")), (200, "filled", None));
    assert_eq!(tag_cookie(&full).as_deref(), Some("quillo_tag=; Path=/; Max-Age=0; Secure; SameSite=Lax"));
    let cookie = session_cookie(&full).unwrap();
    let j = cookie_json(&cookie);
    assert!(j.get("i").is_none() && j["d"]["pad"].as_str().map(str::len) == Some(2843), "{}", j);
    let over = support::send(s.port, "GET", "/fill/2900", &[], "");
    assert_eq!(over.status, 500);
    assert!(s.log_with("more than a cookie holds (3900)").contains("more than a cookie holds (3900); keep an id in it and the rest in the database"));
    // Its page: answered, not kept, not named; the old tag left as it is.
    let page = support::send(s.port, "GET", "/mine", &[("cookie", &format!("{}; quillo_tag=old", cookie))], "");
    assert_eq!(page.status, 200);
    assert!(page.body.contains("<h1>2843</h1>"), "{}", page.body);
    assert_eq!((page.header("cache-control"), page.header("x-quillo-session"), page.header("x-quillo-offline"), tag_cookie(&page)), (Some("private"), None, None, None));
    let page = support::send(s.port, "GET", "/mine", &[("cookie", &format!("{}; quillo_tag=old", small))], "");
    assert!(page.body.contains("<h1>2800</h1>"), "{}", page.body);
    assert_eq!((page.header("x-quillo-session"), page.header("x-quillo-offline")), (Some(tag.as_str()), Some("private")));
    assert_eq!(tag_cookie(&page), Some(format!("quillo_tag={}; Path=/; Max-Age=2592000; Secure; SameSite=Lax", tag)));
}

/// A page that did not read the session, in a program with offline routes: a session cookie that
/// no longer opens makes it say "no one" (`-`) and takes `quillo_tag` away; where the cookie cannot
/// be judged it says nothing and leaves the tag: a program with a tenant rule not worked out for
/// the request, and a cookie of the other name than this server's (`quillo_session` where it uses
/// `__Host-quillo_session`). Cookies once opened are kept, the last 256 of them, so that a page
/// need not check a signature again: a speed-up, whose answers are the same without it, so that
/// part is coverage of it alone: 300 sessions, each recognised, the first again after it fell out
/// (src/session.rs `cookie_opens`, src/pwa.rs `mark`, FEEDBACK67.md B.2; coverage of 0.8.7; branch
/// coverage-087).
#[test]
fn a_cookie_that_cannot_be_judged_leaves_the_session_alone() {
    let d = dir("judged", OFFLINE);
    let s = run(&d, &["--workers", "1"], &[]);
    let page = |cookie: &str| support::send(s.port, "GET", "/", &[("cookie", cookie)], "");
    let marks = |a: &support::Answer| (a.status, a.header("x-quillo-session").map(str::to_string), tag_cookie(a), a.header("x-quillo-offline").map(str::to_string));
    let ended = (200, Some("-".to_string()), Some("quillo_tag=; Path=/; Max-Age=0; Secure; SameSite=Lax".to_string()), Some("1".to_string()));
    let left = (200, None, None, Some("1".to_string()));
    assert_eq!(marks(&page("__Host-quillo_session=bogus.x; quillo_tag=t1")), ended);
    assert_eq!(marks(&page("quillo_session=bogus.x; quillo_tag=t1")), left);
    // 300 sessions, each recognised; the first again once 256 more were opened after it.
    let cookies: Vec<String> = (0..300).map(|i| sign_in(s.port, &format!("u{}", i))).collect();
    for c in &cookies {
        assert_eq!(marks(&page(&format!("{}; quillo_tag=t1", c))), left, "{}", c);
    }
    assert_eq!(marks(&page(&format!("{}; quillo_tag=t1", cookies[0]))), left);
    assert_eq!(marks(&page("__Host-quillo_session=bogus.y; quillo_tag=t1")), ended);
    drop(s);
    let d = dir("judged-tenant", &format!("tenant_from(\"example.com\")\n{}", OFFLINE));
    assert_eq!(check_lines(&d), (true, vec![]));
    let s = run(&d, &[], &[]);
    for host in ["acme.example.com", "127.0.0.1"] {
        let a = support::send(s.port, "GET", "/", &[("host", host), ("cookie", "__Host-quillo_session=bogus.x; quillo_tag=t1")], "");
        assert_eq!(marks(&a), left, "{}", host);
    }
}

// ---- an installable app's files -----------------------------------------------------------------

/// With an app.toml, a route of the program's at the manifest's or Apple icon's very path keeps it,
/// even where its query pattern does not match: Quillo serves neither file then, the route's 400
/// stands, and its pages get neither link (src/pwa.rs `answer`, FEEDBACK67.md B.1; coverage of
/// 0.8.7; branch coverage-087).
#[test]
fn an_apps_files_stay_out_of_a_route_at_their_path() {
    let d = dir("own-paths", "route GET /manifest.webmanifest ? { v: int } = { v: v }\nroute GET /apple-touch-icon.png ? { s: int } = `icon ${s}`\nroute GET / = <html><head><title>Shop</title></head><body>x</body></html>\n");
    std::fs::write(d.join("app.toml"), "name = \"Shop\"\ntheme_color = \"teal\"\n").unwrap();
    assert_eq!(check_lines(&d), (true, vec![]));
    let s = run(&d, &[], &[]);
    for path in ["/manifest.webmanifest", "/apple-touch-icon.png"] {
        for method in ["GET", "HEAD"] {
            let a = support::send(s.port, method, path, &[], "");
            assert_eq!(a.status, 400, "{} {}", method, path);
            assert_ne!(a.header("content-type"), Some("application/manifest+json"));
            assert_ne!(a.header("content-type"), Some("image/png"));
        }
    }
    assert_eq!(support::send(s.port, "GET", "/manifest.webmanifest?v=2", &[], "").body, r#"{"v":2}"#);
    assert_eq!(support::send(s.port, "GET", "/apple-touch-icon.png?s=180", &[], "").body, "icon 180");
    assert_eq!(support::send(s.port, "GET", "/", &[], "").body, "<!doctype html><html><head><title>Shop</title></head><body>x</body></html>");
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A `w`×`h` PNG of exactly `len` bytes: a comment fills it out.
fn png(w: u32, h: u32, len: usize) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    let mut bytes = out.into_inner();
    let pad = len - bytes.len() - 12;
    assert!(pad >= 8, "{} bytes already", bytes.len());
    let mut chunk = b"tEXt".to_vec();
    chunk.extend_from_slice(b"Comment\0");
    chunk.extend(std::iter::repeat(b'x').take(pad - 8));
    let mut text = (pad as u32).to_be_bytes().to_vec();
    text.extend_from_slice(&chunk);
    text.extend_from_slice(&crc32(&chunk).to_be_bytes());
    let iend = bytes.len() - 12;
    bytes.splice(iend..iend, text);
    assert_eq!(bytes.len(), len);
    bytes
}

/// What `quillo check` says of an app.toml's icon is known by its file, length and time, for 16
/// files: a file changed in place to the same length and time is told as it was, until 16 other
/// icons were checked after it, when it is read again. What is asserted holds for any 16 kept that
/// let the least recent go, one at a time or all at once (src/pwa.rs `icon_size`, FEEDBACK67.md
/// B.1; coverage of 0.8.7; branch coverage-087).
#[test]
fn an_icons_size_is_kept_for_sixteen_files() {
    let d = dir("icons", "route GET /c/:n(int) = check_program(`p${n}/main.qwe`)\n");
    for i in 0..17 {
        let p = d.join(format!("p{}", i));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("main.qwe"), "route GET / = \"x\"\n").unwrap();
        std::fs::write(p.join("app.toml"), "name = \"Ā\"\nicon = \"i.png\"\n").unwrap();
        std::fs::write(p.join("i.png"), png(64, 64, 600)).unwrap();
    }
    assert_eq!(check_lines(&d), (true, vec![]));
    let s = run(&d, &[], &[]);
    let said = |n: usize| -> String {
        let a = support::send(s.port, "GET", &format!("/c/{}", n), &[], "");
        let j: serde_json::Value = serde_json::from_str(&a.body).unwrap_or_else(|_| panic!("{}", a.body));
        assert_eq!(j.as_array().map(Vec::len), Some(1), "{}", j);
        assert_eq!((j[0]["file"].as_str(), j[0]["line"].as_i64(), j[0]["severity"].as_str()), (Some(format!("p{}/app.toml", n).as_str()), Some(2), Some("warning")));
        j[0]["message"].as_str().unwrap().to_string()
    };
    let size = |w: u32| format!("icon \"i.png\" is {}×{}: Chrome wants 512×512 to offer to install the app, so it is enlarged; give one of 512×512 or more", w, w);
    assert_eq!(said(0), size(64));
    // Changed in place: the same length, its time put back.
    let file = d.join("p0/i.png");
    let time = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::fs::write(&file, png(32, 32, 600)).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(time).unwrap();
    assert_eq!(said(0), size(64), "kept by file, length and time");
    for n in 1..=16 {
        assert_eq!(said(n), size(64));
    }
    assert_eq!(said(0), size(32), "read again once 16 others were checked after it");
}

// ---- rows past --max-memory ---------------------------------------------------------------------

/// A send whose rows hold a cell larger than --max-memory allows stops reading them, and the stop
/// is the request's: a 500, said in the log at the send's line, which the program's `is_err` never
/// sees; under 99,999 bytes too, one less than the cell. Under a limit the rows fit in, the same
/// row is sent to; another person's rows are read and sent to under the small limit (src/push.rs
/// `sql`, `send`, FEEDBACK71.md Q-977 with FEEDBACK67.md B.3; coverage of 0.8.7, Q-1101; branch
/// coverage-087).
#[test]
fn rows_past_max_memory_are_not_read_by_a_send() {
    let d = dir("memory", PUSHER);
    let sink = sink();
    {
        // The large cell is the auth secret: 75,000 bytes once decoded, which encrypting takes.
        let c = system_db(&d, TABLE);
        c.execute(
            "insert into _quillo_push (endpoint, p256dh, auth, who, session, origin, place, created, succeeded) values (?1, ?2, ?3, '\"big\"', 'id s', 'http://shop.example', ?4, 1, 0)",
            rusqlite::params![sink.endpoint("/push/ok-big"), P256DH, "A".repeat(100_000), PLACE],
        )
        .unwrap();
        row(&c, &sink.endpoint("/push/ok-small"), P256DH, "\"small\"", "http://shop.example", 2);
    }
    let one = serde_json::json!({ "title": "t" });
    for (limit, bytes) in [("64KB", 65_536), ("99999", 99_999)] {
        let s = run(&d, &["--max-memory", limit], &[]);
        let a = support::send(s.port, "POST", "/send/big", &[], &one.to_string());
        assert_eq!((a.status, a.body.as_str()), (500, "internal error\n"), "{}", limit);
        let said = format!("main.qwe:10: this request's value would be bigger than --max-memory allows ({} bytes)", bytes);
        assert!(s.log_with(&said).contains(&said), "{}: {}", limit, s.log_with(""));
        assert!(sink.got().is_empty(), "{:?}", sink.got());
        assert_eq!(send_to(s.port, "small", one.clone()), serde_json::json!({ "sent": 1, "gone": 0 }));
        assert_eq!(sink.got().iter().map(|g| g.path.clone()).collect::<Vec<_>>(), ["/push/ok-small"]);
        sink.reset();
    }
    let s = run(&d, &["--max-memory", "1MB"], &[]);
    assert_eq!(send_to(s.port, "big", one), serde_json::json!({ "sent": 1, "gone": 0 }));
    assert_eq!(sink.got().iter().map(|g| g.path.clone()).collect::<Vec<_>>(), ["/push/ok-big"]);
    assert!(!s.log_with("").contains("--max-memory allows"));
}

/// At /_quillo/push, rows past --max-memory met as a bind counts the person's devices, or as a
/// page's look reads its subscription, end the request as a server fn's call is ended: a 500 that
/// says the limit, and the line in the log. A `stop` reads no rows, and is answered. Under a limit
/// the rows fit in, the same bind and look are answered as ever (src/push.rs `sql`, `failed_here`,
/// `bind`, `check`, FEEDBACK67.md B.3; coverage of 0.8.7, Q-1101; branch coverage-087).
#[test]
fn rows_past_max_memory_end_a_push_request() {
    let d = dir("memory-push", PUSHER);
    let sink = sink();
    let looked = "https://push.example.com/send/big";
    {
        // Ana's device with a long endpoint, which a bind of hers reads; a device with a long
        // session, which a look at it reads.
        let c = system_db(&d, TABLE);
        let insert = "insert into _quillo_push (endpoint, p256dh, auth, who, session, origin, place, created, succeeded) values (?1, ?2, ?3, ?4, ?5, 'http://shop.example', ?6, 1, 0)";
        c.execute(insert, rusqlite::params![format!("https://push.example.com/{}", "e".repeat(100_000)), P256DH, AUTH, "\"ana\"", "id s", PLACE]).unwrap();
        c.execute(insert, rusqlite::params![looked, P256DH, AUTH, "\"zed\"", "s".repeat(100_000), PLACE]).unwrap();
    }
    let look = |port: u16, cookie: &str| support::send(port, "POST", "/_quillo/push", &with_cookie(cookie, &format!("127.0.0.1:{}", port)), &serde_json::json!({ "endpoint": looked, "check": true }).to_string());
    let s = run(&d, &["--max-memory", "64KB"], &[]);
    let stopped = r#"{"error":"this request's value would be bigger than --max-memory allows (65536 bytes)"}"#;
    let ana = sign_in(s.port, "ana");
    let a = bind(s.port, &sink.endpoint("/push/ok-1"), &ana);
    assert_eq!((a.status, a.body.as_str()), (500, stopped));
    let a = look(s.port, &ana);
    assert_eq!((a.status, a.body.as_str()), (500, stopped));
    let log = s.log_with("--max-memory allows");
    let lines: Vec<&str> = log.lines().filter(|l| l.contains("this request's value would be bigger than --max-memory allows (65536 bytes)")).collect();
    assert_eq!(lines.len(), 2, "{}", log);
    assert!(lines.iter().all(|l| l.starts_with("quillo: error: POST /_quillo/push")), "{:?}", lines);
    let stop = support::send(s.port, "DELETE", "/_quillo/push", &with_cookie(&ana, &format!("127.0.0.1:{}", s.port)), &serde_json::json!({ "endpoint": looked }).to_string());
    assert_eq!((stop.status, stop.body.as_str()), (200, r#"{"ok":true}"#));
    drop(s);
    let s = run(&d, &["--max-memory", "1MB"], &[]);
    let ana = sign_in(s.port, "ana");
    let a = bind(s.port, &sink.endpoint("/push/ok-2"), &ana);
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"ok":true}"#));
    let a = look(s.port, &ana);
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"state":"default"}"#));
    assert!(!s.log_with("").contains("--max-memory allows"));
}
