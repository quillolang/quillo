// FEEDBACK70.md, Q-986 and Q-987 (branch security): one reading of the scheme a request came over.
// Q-986, `X-Forwarded-Proto` is believed only from a proxy `--trust-proxy` names, as the form gate
// (Q-966) believes it, for a deletion's `Secure` and a push subscription's origin too; where the
// header from anyone else would have counted, the log says once which address to trust. Q-987,
// under `quillo fcgi` the web server is that proxy: its `HTTPS` parameter (or `REQUEST_SCHEME`) is
// the request's scheme, and a client's own `X-Forwarded-Proto` is dropped.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = r#"route GET /in/:who {
    set_session({ user: who })
    return "in"
}
route GET /del {
    delete_cookie("theme")
    return "deleted"
}
route GET /del-secure {
    delete_cookie("theme", { secure: true })
    return "deleted"
}
route GET /del-host {
    delete_cookie("__Host-x")
    return "deleted"
}
route GET /set {
    set_cookie("theme", "dark")
    return "set"
}
route GET /proto = request.headers["x-forwarded-proto"] ?? "none"
route POST /notes { form: { text: str } } = "saved"
route POST /send/:who = notification.send(who, request.json)
"#;

/// The line said once for a header from a peer `--trust-proxy` does not name.
fn not_named(peer: &str) -> String {
    format!("quillo: a request came with X-Forwarded-Proto from {}, which --trust-proxy does not name: if that address is your proxy, --trust-proxy {}; if not, a visitor sent it and it is ignored", peer, peer)
}

const SAID: &str = "a request came with X-Forwarded-Proto from";

/// A P-256 public key (the curve's generator) and an `auth` of 16 bytes, in base64url, as a
/// browser's subscription carries them.
const P256DH: &str = "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU";
const AUTH: &str = "AQEBAQEBAQEBAQEBAQEBAQ";

fn dir(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f70b-{}", name));
    std::fs::write(d.join("main.qwe"), PROGRAM).unwrap();
    d
}

/// `quillo run main.qwe --port 0` in `d` with `opts` (QUILLO_PUSH_LOCAL=1: a subscription's
/// endpoint on this machine is taken), its log in server.log; the port it said, once it is
/// connected to at `host`.
struct Run {
    _child: support::Guard,
    log: PathBuf,
    port: u16,
    host: &'static str,
}

impl Run {
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn ask(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Answer {
        ask(self.host, self.port, method, path, headers, body)
    }
}

fn run(d: &Path, opts: &[&str], host: &'static str) -> Run {
    let log = d.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg("main.qwe")
        .args(["--port", "0"])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .env("QUILLO_PUSH_LOCAL", "1")
        .current_dir(d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let end = Instant::now() + Duration::from_secs(60);
    let port = loop {
        if let Some(p) = support::port_said_in(&log) {
            break p;
        }
        if let Ok(Some(st)) = g.0.try_wait() {
            panic!("quillo run stopped: {}: {}", st, std::fs::read_to_string(&log).unwrap_or_default());
        }
        assert!(Instant::now() < end, "the server did not start: {}", std::fs::read_to_string(&log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(20));
    };
    while std::net::TcpStream::connect((host, port)).is_err() {
        assert!(Instant::now() < end, "the server did not listen");
        std::thread::sleep(Duration::from_millis(20));
    }
    Run { _child: g, log, port, host }
}

struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// One request to `host`'s `port`, read for at most 30 seconds; a Host of `localhost` unless one
/// is given.
fn ask(host: &str, port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Answer {
    let mut s = std::net::TcpStream::connect((host, port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("{} {} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        head.push_str("Host: localhost\r\n");
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

/// The `Set-Cookie` of `GET <path>` with `headers`. Under `--base` the proxy in front takes the
/// prefix off: the program is asked for `path` as it is, and its cookies' `Path` is the prefix.
fn cookie(r: &Run, path: &str, headers: &[(&str, &str)]) -> String {
    let a = r.ask("GET", path, headers, "");
    assert_eq!(a.status, 200, "{} {}: {}", path, a.status, a.body);
    a.header("set-cookie").unwrap_or("").to_string()
}

/// The start of the deletion of `theme`: its `Path` the prefix under `--base`.
fn deleted(base: &str) -> String {
    format!("theme=; Path={}; Max-Age=0", if base.is_empty() { "/" } else { base })
}

fn secure(c: &str) -> bool {
    c.split(';').any(|p| p.trim() == "Secure")
}

/// A subscription at `endpoint`, as a page posts it to `/_quillo/push`.
fn subscription(endpoint: &str) -> String {
    serde_json::json!({ "endpoint": endpoint, "expirationTime": null, "keys": { "p256dh": P256DH, "auth": AUTH }, "user": null }).to_string()
}

/// The origin kept with the subscription at `endpoint`, read from the system database.
fn origin_kept(d: &Path, endpoint: &str) -> String {
    let db = rusqlite::Connection::open(d.join("quillo-system.db")).unwrap();
    db.query_row("select origin from _quillo_push where endpoint = ?1", [endpoint], |row| row.get::<_, String>(0)).unwrap()
}

/// Ivan signed in at `/in/ivan`: his session cookie.
fn signed_in(r: &Run) -> String {
    let c = cookie(r, "/in/ivan", &[]);
    assert!(c.starts_with("__Host-quillo_session=") || c.starts_with("quillo_session="), "{}", c);
    c.split(';').next().unwrap().to_string()
}

/// Binds the subscription at `endpoint` for Ivan, the request with `headers` and a Host of
/// `shop.example:8443`; the origin kept for it.
fn bound(r: &Run, d: &Path, who: &str, endpoint: &str, headers: &[(&str, &str)]) -> String {
    let mut hs: Vec<(&str, &str)> = vec![("host", "shop.example:8443"), ("cookie", who), ("content-type", "application/json")];
    hs.extend_from_slice(headers);
    let a = r.ask("POST", "/_quillo/push", &hs, &subscription(endpoint));
    assert_eq!(a.status, 200, "{}", a.body);
    origin_kept(d, endpoint)
}

/// A form post to `/notes` with these headers, the Host the server's own: its status.
fn form(r: &Run, headers: &[(&str, &str)]) -> u16 {
    let host = if r.host.contains(':') { format!("[{}]:{}", r.host, r.port) } else { format!("{}:{}", r.host, r.port) };
    let mut hs: Vec<(&str, &str)> = vec![("host", &host), ("content-type", "application/x-www-form-urlencoded")];
    hs.extend_from_slice(headers);
    r.ask("POST", "/notes", &hs, "text=hi").status
}

fn base_of(opts: &[&str]) -> &'static str {
    if opts.contains(&"--base") {
        "/shop"
    } else {
        ""
    }
}

// ---- Q-986 -----------------------------------------------------------------------------------

#[test]
fn q986_the_program_passes_the_check() {
    let d = dir("check");
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe"]).current_dir(&d).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && !said.contains(": error: "), "{}", said);
}

// A proxy `--trust-proxy` names says https: the deletion is `Secure` and the push subscription's
// origin https, by the last item of the last header (a proxy appends its own), in any case; its
// http, or none, neither. Under --base and --instances too; nothing is said in the log.
#[test]
fn q986_a_trusted_proxys_https_gives_a_secure_deletion_and_an_https_origin() {
    let cases: [&[&str]; 3] = [&["--trust-proxy", "127.0.0.1"], &["--trust-proxy", "127.0.0.1", "--base", "/shop"], &["--trust-proxy", "10.0.0.0/8,127.0.0.1", "--instances", "2"]];
    for (i, opts) in cases.iter().enumerate() {
        let d = dir(&format!("trusted-{}", i));
        let r = run(&d, opts, "127.0.0.1");
        let base = base_of(opts);
        for proto in [&[("x-forwarded-proto", "https")][..], &[("x-forwarded-proto", "HTTPS")][..], &[("x-forwarded-proto", "http, https")][..], &[("x-forwarded-proto", "http"), ("x-forwarded-proto", " https ")][..]] {
            let c = cookie(&r, "/del", proto);
            assert!(c.starts_with(&deleted(base)) && secure(&c), "{:?} {:?}: {}", opts, proto, c);
        }
        for proto in [&[][..], &[("x-forwarded-proto", "http")][..], &[("x-forwarded-proto", "https, http")][..], &[("x-forwarded-proto", "https"), ("x-forwarded-proto", "http")][..], &[("x-forwarded-proto", "wss")][..], &[("x-forwarded-proto", "")][..]] {
            let c = cookie(&r, "/del", proto);
            assert!(c.starts_with(&deleted(base)) && !secure(&c), "{:?} {:?}: {}", opts, proto, c);
        }
        let ivan = signed_in(&r);
        assert_eq!(bound(&r, &d, &ivan, "http://127.0.0.1:9/push/https", &[("x-forwarded-proto", "https")]), "https://shop.example:8443");
        assert_eq!(bound(&r, &d, &ivan, "http://127.0.0.1:9/push/http", &[("x-forwarded-proto", "http")]), "http://shop.example:8443");
        assert_eq!(bound(&r, &d, &ivan, "http://127.0.0.1:9/push/none", &[]), "http://shop.example:8443");
        assert!(!r.log().contains(SAID), "{:?}: {}", opts, r.log());
    }
}

// A client `--trust-proxy` does not name says https: neither the deletion's `Secure` nor the
// origin's https follow, and the log says once which address to name, however many such requests
// come, on any of the instances; the form gate compares the hosts alone, as before. It is said
// where the header would have counted, never for a request that does not read it (the parser
// pays nothing for it). No proxy named, another one named, under --base and --instances, and the
// mark of a trusted proxy written by the client itself.
#[test]
fn q986_an_untrusted_clients_https_gives_neither_and_is_said_once() {
    let cases: [&[&str]; 4] = [&[], &["--base", "/shop"], &["--trust-proxy", "10.9.9.9"], &["--instances", "2"]];
    for (i, opts) in cases.iter().enumerate() {
        let d = dir(&format!("untrusted-{}", i));
        let r = run(&d, opts, "127.0.0.1");
        let base = base_of(opts);
        for _ in 0..3 {
            assert_eq!(r.ask("GET", "/proto", &[("x-forwarded-proto", "https")], "").body, "https");
            assert_eq!(r.ask("GET", "/set", &[("x-forwarded-proto", "https")], "").status, 200);
        }
        assert!(!r.log().contains(SAID), "{:?}: {}", opts, r.log());
        for _ in 0..4 {
            let c = cookie(&r, "/del", &[("x-forwarded-proto", "https")]);
            assert!(c.starts_with(&deleted(base)) && !secure(&c), "{:?}: {}", opts, c);
        }
        let c = cookie(&r, "/del", &[("x-quillo-from-trusted-proxy", "1"), ("x-forwarded-proto", "https")]);
        assert!(!secure(&c), "{:?}: {}", opts, c);
        let ivan = signed_in(&r);
        assert_eq!(bound(&r, &d, &ivan, "http://127.0.0.1:9/push/forged", &[("x-forwarded-proto", "https")]), "http://shop.example:8443", "{:?}", opts);
        let http = format!("http://127.0.0.1:{}", r.port);
        assert_eq!(form(&r, &[("x-forwarded-proto", "https"), ("origin", &http)]), 200, "{:?}", opts);
        // The program still reads the header as it came.
        assert_eq!(r.ask("GET", "/proto", &[("x-forwarded-proto", "https")], "").body, "https");
        let log = r.log();
        assert_eq!(log.matches(SAID).count(), 1, "{:?}: {}", opts, log);
        assert!(log.contains(&format!("{}\n", not_named("127.0.0.1"))), "{:?}: {}", opts, log);
    }
}

// Each reader of the scheme says it when it is the first: a write's Origin (the form gate), a
// push subscription, and a deletion.
#[test]
fn q986_each_reader_of_the_header_says_the_line_first() {
    let d = dir("readers");
    let r = run(&d, &[], "127.0.0.1");
    let http = format!("http://127.0.0.1:{}", r.port);
    assert_eq!(form(&r, &[("x-forwarded-proto", "https"), ("origin", &http)]), 200);
    assert_eq!(r.log().matches(SAID).count(), 1, "{}", r.log());
    drop(r);
    let d = dir("readers-push");
    let r = run(&d, &[], "127.0.0.1");
    let ivan = signed_in(&r);
    assert!(!r.log().contains(SAID), "{}", r.log());
    assert_eq!(bound(&r, &d, &ivan, "http://127.0.0.1:9/push/first", &[("x-forwarded-proto", "http")]), "http://shop.example:8443");
    assert_eq!(r.log().matches(SAID).count(), 1, "{}", r.log());
    assert!(!secure(&cookie(&r, "/del", &[("x-forwarded-proto", "https")])));
    assert_eq!(r.log().matches(SAID).count(), 1, "{}", r.log());
}

// Over IPv6 the line names the address as --trust-proxy takes it; and with that address named,
// the header is believed and nothing is said.
#[test]
fn q986_over_ipv6_the_line_names_the_address_to_trust() {
    if std::net::TcpListener::bind(("::1", 0)).is_err() {
        return;
    }
    let d = dir("ipv6");
    let r = run(&d, &["--host", "::1"], "::1");
    assert!(!secure(&cookie(&r, "/del", &[("x-forwarded-proto", "https")])));
    assert!(r.log().contains(&not_named("::1")), "{}", r.log());
    drop(r);
    let d = dir("ipv6-named");
    let r = run(&d, &["--host", "::1", "--trust-proxy", "::1"], "::1");
    assert!(secure(&cookie(&r, "/del", &[("x-forwarded-proto", "https")])));
    assert!(!r.log().contains(SAID), "{}", r.log());
}

// Without the header all is as before, whether a proxy is named or not, under --base too: a
// deletion is not `Secure` unless asked for or its name needs it, a cookie set is, the origin is
// http, and nothing is said.
#[test]
fn q986_with_no_header_things_are_as_before() {
    let cases: [&[&str]; 3] = [&[], &["--trust-proxy", "127.0.0.1"], &["--trust-proxy", "127.0.0.1", "--base", "/shop"]];
    for (i, opts) in cases.iter().enumerate() {
        let d = dir(&format!("none-{}", i));
        let r = run(&d, opts, "127.0.0.1");
        let base = base_of(opts);
        let c = cookie(&r, "/del", &[]);
        assert!(c.starts_with(&deleted(base)) && c.contains("Max-Age=0") && !secure(&c), "{:?}: {}", opts, c);
        assert!(secure(&cookie(&r, "/del-secure", &[])), "{:?}", opts);
        assert!(secure(&cookie(&r, "/del-host", &[])), "{:?}: a __Host- cookie needs it", opts);
        assert!(secure(&cookie(&r, "/set", &[])), "{:?}", opts);
        assert_eq!(r.ask("GET", "/proto", &[], "").body, "none");
        let ivan = signed_in(&r);
        assert_eq!(bound(&r, &d, &ivan, "http://127.0.0.1:9/push/plain", &[]), "http://shop.example:8443");
        assert!(!r.log().contains(SAID), "{:?}: {}", opts, r.log());
    }
}

// ---- Q-987: quillo fcgi -------------------------------------------------------------------------

fn params(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// The HTTP request `quillo fcgi` makes of these parameters: the web server's `HTTPS` (any case,
/// `1` too), else `REQUEST_SCHEME`, written as the scheme last; a client's `X-Forwarded-Proto`,
/// however spelt, never; neither parameter (or an empty `HTTPS`, or a `REQUEST_SCHEME` that is
/// neither scheme), no scheme.
#[test]
fn q987_the_web_servers_scheme_is_written_and_the_clients_dropped() {
    let made = |p: &[(&str, &str)]| String::from_utf8(quillo::fcgi::http_request(&params(p), b"").0).unwrap();
    let with = |(scheme, from): (&str, &str)| format!("GET / HTTP/1.0\r\nhost: localhost\r\nx-forwarded-proto: {}\r\nx-quillo-scheme-from: {}\r\ncontent-length: 0\r\n\r\n", scheme, from);
    let without = "GET / HTTP/1.0\r\nhost: localhost\r\ncontent-length: 0\r\n\r\n";
    for (p, want) in [
        (&[("HTTPS", "on")][..], Some(("https", "HTTPS"))),
        (&[("HTTPS", "ON")][..], Some(("https", "HTTPS"))),
        (&[("HTTPS", " on ")][..], Some(("https", "HTTPS"))),
        (&[("HTTPS", "1")][..], Some(("https", "HTTPS"))),
        (&[("HTTPS", "off")][..], Some(("http", "HTTPS"))),
        (&[("HTTPS", "off"), ("REQUEST_SCHEME", "https")][..], Some(("http", "HTTPS"))),
        (&[("HTTPS", "on"), ("REQUEST_SCHEME", "http")][..], Some(("https", "HTTPS"))),
        (&[("REQUEST_SCHEME", "https")][..], Some(("https", "REQUEST_SCHEME"))),
        (&[("REQUEST_SCHEME", "HTTP")][..], Some(("http", "REQUEST_SCHEME"))),
        (&[("HTTPS", ""), ("REQUEST_SCHEME", "http")][..], Some(("http", "REQUEST_SCHEME"))),
        (&[("HTTPS", "")][..], None),
        (&[("REQUEST_SCHEME", "wss")][..], None),
        (&[][..], None),
        (&[("HTTP_X_FORWARDED_PROTO", "https")][..], None),
        (&[("HTTP_x_forwarded_proto", "https")][..], None),
        (&[("HTTP_X_QUILLO_SCHEME_FROM", "REQUEST_SCHEME")][..], None),
        (&[("HTTP_X_FORWARDED_PROTO", "http"), ("HTTPS", "on")][..], Some(("https", "HTTPS"))),
        (&[("HTTPS", "on"), ("HTTP_X_FORWARDED_PROTO", "http"), ("HTTP_X_QUILLO_SCHEME_FROM", "x")][..], Some(("https", "HTTPS"))),
    ] {
        let mut p = p.to_vec();
        p.insert(0, ("REQUEST_URI", "/"));
        assert_eq!(made(&p), want.map_or(without.to_string(), with), "{:?}", p);
    }
    // The other forwarded headers pass as before.
    let text = made(&[("REQUEST_URI", "/"), ("HTTP_X_FORWARDED_FOR", "7.7.7.7"), ("HTTPS", "on")]);
    assert_eq!(text, "GET / HTTP/1.0\r\nx-forwarded-for: 7.7.7.7\r\nhost: localhost\r\nx-forwarded-proto: https\r\nx-quillo-scheme-from: HTTPS\r\ncontent-length: 0\r\n\r\n");
}

/// A FastCGI parameter pair, its lengths in one byte or four.
#[cfg(unix)]
fn pair(k: &str, v: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for n in [k.len(), v.len()] {
        if n < 128 {
            out.push(n as u8);
        } else {
            out.extend_from_slice(&((n as u32) | 0x8000_0000).to_be_bytes());
        }
    }
    out.extend_from_slice(k.as_bytes());
    out.extend_from_slice(v.as_bytes());
    out
}

#[cfg(unix)]
fn rec(kind: u8, content: &[u8]) -> Vec<u8> {
    let pad = (8 - content.len() % 8) % 8;
    let mut r = vec![1, kind, 0, 1];
    r.extend_from_slice(&(content.len() as u16).to_be_bytes());
    r.extend_from_slice(&[pad as u8, 0]);
    r.extend_from_slice(content);
    r.resize(r.len() + pad, 0);
    r
}

/// `quillo fcgi main.qwe --listen unix:f.sock` in `d`, its output in fcgi.log.
#[cfg(unix)]
struct Fcgi {
    _child: support::Guard,
    dir: PathBuf,
}

#[cfg(unix)]
impl Fcgi {
    fn start(d: &Path) -> Fcgi {
        let log = std::fs::File::create(d.join("fcgi.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["fcgi", "main.qwe", "--listen", "unix:f.sock"])
            .env("QUILLO_SECRET", "test secret for the tests")
            .env("QUILLO_PUSH_LOCAL", "1")
            .current_dir(d)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let f = Fcgi { _child: support::Guard::new(child), dir: d.to_path_buf() };
        let end = Instant::now() + Duration::from_secs(60);
        while std::os::unix::net::UnixStream::connect(d.join("f.sock")).is_err() {
            assert!(Instant::now() < end, "never listened: {}", f.log());
            std::thread::sleep(Duration::from_millis(20));
        }
        f
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("fcgi.log")).unwrap_or_default()
    }

    /// One request as a web server sends it, from 203.0.113.9 for example.com unless `p` says
    /// otherwise: its status, CGI head and body.
    fn ask(&self, method: &str, uri: &str, p: &[(&str, &str)], body: &[u8]) -> (u16, String, String) {
        let mut s = std::os::unix::net::UnixStream::connect(self.dir.join("f.sock")).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let mut all: Vec<(&str, &str)> = vec![("REQUEST_METHOD", method), ("REQUEST_URI", uri), ("REMOTE_ADDR", "203.0.113.9"), ("REMOTE_PORT", "5555")];
        if !p.iter().any(|(k, _)| *k == "HTTP_HOST") {
            all.push(("HTTP_HOST", "example.com"));
        }
        all.extend_from_slice(p);
        let mut ps = Vec::new();
        for (k, v) in &all {
            ps.extend(pair(k, v));
        }
        let mut m = rec(1, &[0, 1, 0, 0, 0, 0, 0, 0]);
        m.extend(rec(4, &ps));
        m.extend(rec(4, &[]));
        if !body.is_empty() {
            m.extend(rec(5, body));
        }
        m.extend(rec(5, &[]));
        s.write_all(&m).unwrap();
        let mut out = Vec::new();
        loop {
            let mut h = [0u8; 8];
            s.read_exact(&mut h).unwrap();
            let n = u16::from_be_bytes([h[4], h[5]]) as usize;
            let mut c = vec![0u8; n + h[6] as usize];
            s.read_exact(&mut c).unwrap();
            c.truncate(n);
            match h[1] {
                6 => out.extend_from_slice(&c),
                3 => break,
                t => panic!("record {}", t),
            }
        }
        let out = String::from_utf8_lossy(&out).into_owned();
        let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
        let status = head.strip_prefix("Status: ").and_then(|s| s.get(..3)).and_then(|s| s.parse().ok()).unwrap_or(0);
        (status, head.to_string(), body.to_string())
    }

    /// A form post to /notes, as an old browser sends it (no Sec-Fetch-Site): its status.
    fn form(&self, p: &[(&str, &str)]) -> u16 {
        let mut all = vec![("CONTENT_TYPE", "application/x-www-form-urlencoded")];
        all.extend_from_slice(p);
        self.ask("POST", "/notes", &all, b"text=hi").0
    }

    /// The deletion's `Set-Cookie` with these parameters.
    fn deletion(&self, p: &[(&str, &str)]) -> String {
        let (status, head, body) = self.ask("GET", "/del", p, b"");
        assert_eq!(status, 200, "{}", body);
        head.lines().find_map(|l| l.strip_prefix("set-cookie: ").or_else(|| l.strip_prefix("Set-Cookie: "))).unwrap_or("").to_string()
    }
}

// The report's case: under `quillo fcgi`, the web server's `HTTPS=on` makes an `http://` Origin
// another site's for an old browser (no Sec-Fetch-Site), and an `https://` one the site's own;
// without `HTTPS` (and no `REQUEST_SCHEME`) the scheme is not known and the hosts alone are
// compared, as before; `REQUEST_SCHEME` says it where `HTTPS` is not given. A client's own
// `X-Forwarded-Proto` is not believed, nor seen by the program; the web server's is. The browser's
// own Sec-Fetch-Site still wins. The refusal for the scheme is said once, naming the web server, and
// nothing is said of the header the web server wrote.
#[cfg(unix)]
#[test]
fn q987_under_fcgi_the_web_servers_https_is_the_requests_scheme() {
    let d = dir("fcgi-scheme");
    let f = Fcgi::start(&d);
    let (http, https) = ("http://example.com", "https://example.com");
    for on in ["on", "ON", "1"] {
        assert_eq!(f.form(&[("HTTPS", on), ("HTTP_ORIGIN", http)]), 403, "{}", on);
        assert_eq!(f.form(&[("HTTPS", on), ("HTTP_ORIGIN", https)]), 200, "{}", on);
    }
    assert_eq!(f.form(&[("HTTPS", "on"), ("HTTP_ORIGIN", "https://evil.example")]), 403);
    assert_eq!(f.form(&[("HTTPS", "on"), ("HTTP_ORIGIN", http), ("HTTP_SEC_FETCH_SITE", "same-origin")]), 200);
    assert_eq!(f.form(&[("HTTPS", "on")]), 200, "no Origin: a webhook");
    // Not known: the hosts alone.
    assert_eq!(f.form(&[("HTTP_ORIGIN", http)]), 200);
    assert_eq!(f.form(&[("HTTP_ORIGIN", https)]), 200);
    assert_eq!(f.form(&[("HTTP_ORIGIN", "https://evil.example")]), 403);
    // REQUEST_SCHEME, and HTTPS's other values.
    assert_eq!(f.form(&[("REQUEST_SCHEME", "https"), ("HTTP_ORIGIN", http)]), 403);
    assert_eq!(f.form(&[("REQUEST_SCHEME", "https"), ("HTTP_ORIGIN", https)]), 200);
    assert_eq!(f.form(&[("REQUEST_SCHEME", "http"), ("HTTP_ORIGIN", https)]), 403);
    assert_eq!(f.form(&[("REQUEST_SCHEME", "http"), ("HTTP_ORIGIN", http)]), 200);
    assert_eq!(f.form(&[("HTTPS", "off"), ("HTTP_ORIGIN", http)]), 200);
    // A client's own header: not believed, either way, nor passed to the program.
    assert_eq!(f.form(&[("HTTP_X_FORWARDED_PROTO", "https"), ("HTTP_ORIGIN", http)]), 200);
    assert_eq!(f.form(&[("HTTP_X_FORWARDED_PROTO", "http"), ("HTTPS", "on"), ("HTTP_ORIGIN", http)]), 403);
    assert_eq!(f.form(&[("HTTP_X_FORWARDED_PROTO", "http"), ("HTTPS", "on"), ("HTTP_ORIGIN", https)]), 200);
    assert_eq!(f.ask("GET", "/proto", &[("HTTP_X_FORWARDED_PROTO", "https")], b"").2, "none");
    assert_eq!(f.ask("GET", "/proto", &[("HTTP_X_FORWARDED_PROTO", "http"), ("HTTPS", "on")], b"").2, "https");
    assert_eq!(f.ask("GET", "/proto", &[("REQUEST_SCHEME", "http")], b"").2, "http");
    let log = f.log();
    assert_eq!(log.matches("quillo: warning: a request was taken as another site's: its Origin is http:// and the web server's HTTPS parameter says it came over https. Browsers send no Sec-Fetch-Site to an http:// page, so if this site is served over http:// too and the web server says https there (its HTTPS parameter), every browser's write from an http:// page is refused").count(), 1, "{}", log);
    assert!(!log.contains(SAID) && !log.contains("the proxy (--trust-proxy)"), "{}", log);
}

// `came_over_https` follows the web server's `HTTPS` (or `REQUEST_SCHEME`) under `quillo fcgi`:
// the deletion is `Secure` and the push subscription's origin https with it, neither without it,
// and a client's own `X-Forwarded-Proto` changes neither.
#[cfg(unix)]
#[test]
fn q987_under_fcgi_the_deletion_and_the_push_origin_follow_https() {
    let d = dir("fcgi-https");
    let f = Fcgi::start(&d);
    for p in [&[("HTTPS", "on")][..], &[("REQUEST_SCHEME", "https")][..], &[("HTTPS", "on"), ("HTTP_X_FORWARDED_PROTO", "http")][..]] {
        let c = f.deletion(p);
        assert!(c.starts_with("theme=; Path=/") && secure(&c), "{:?}: {}", p, c);
    }
    for p in [&[][..], &[("HTTP_X_FORWARDED_PROTO", "https")][..], &[("HTTPS", "off")][..], &[("REQUEST_SCHEME", "http")][..]] {
        let c = f.deletion(p);
        assert!(c.starts_with("theme=; Path=/") && !secure(&c), "{:?}: {}", p, c);
    }
    let (status, head, _) = f.ask("GET", "/in/ivan", &[], b"");
    assert_eq!(status, 200, "{}", head);
    let ivan = head.lines().filter_map(|l| l.strip_prefix("set-cookie: ")).find(|v| v.contains("quillo_session=")).map(|v| v.split(';').next().unwrap().to_string()).expect("a session cookie");
    let bind = |endpoint: &str, p: &[(&str, &str)]| {
        let mut all = vec![("HTTP_HOST", "shop.example"), ("HTTP_COOKIE", ivan.as_str()), ("CONTENT_TYPE", "application/json")];
        all.extend_from_slice(p);
        let (status, _, body) = f.ask("POST", "/_quillo/push", &all, subscription(endpoint).as_bytes());
        assert_eq!(status, 200, "{}", body);
        origin_kept(&d, endpoint)
    };
    assert_eq!(bind("http://127.0.0.1:9/push/on", &[("HTTPS", "on")]), "https://shop.example");
    assert_eq!(bind("http://127.0.0.1:9/push/scheme", &[("REQUEST_SCHEME", "https")]), "https://shop.example");
    assert_eq!(bind("http://127.0.0.1:9/push/off", &[]), "http://shop.example");
    assert_eq!(bind("http://127.0.0.1:9/push/client", &[("HTTP_X_FORWARDED_PROTO", "https")]), "http://shop.example");
    assert!(!f.log().contains(SAID), "{}", f.log());
}

/// `quillo run main.qwe --split` in `d` with `opts`, its log in `log`, once its service under
/// `/billing` answers: the app, its port, and the service's own port (from the app's line).
fn split(d: &Path, opts: &[&str], log: &Path) -> (support::Guard, u16, u16) {
    // Under --split the services' lines come first: the app's port is picked here.
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg("main.qwe")
        .args(["--split", "--port", &port.to_string()])
        .args(opts)
        .env("QUILLO_SECRET", "test secret for the tests")
        .env("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef")
        .current_dir(d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(log).unwrap())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        if support::listening(port) && ask("127.0.0.1", port, "GET", "/billing/proto", &[], "").body == "none" {
            break;
        }
        if let Ok(Some(st)) = g.0.try_wait() {
            panic!("quillo run --split stopped: {}: {}", st, std::fs::read_to_string(log).unwrap_or_default());
        }
        assert!(Instant::now() < end, "the service did not answer: {}", std::fs::read_to_string(log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let own = text.split("service billing (billing.qwe) on http://127.0.0.1:").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok()).expect("the service's port");
    (g, port, own)
}

/// The program of the split tests: the app and its service under `/billing`, each deleting a cookie.
fn split_dir(name: &str) -> PathBuf {
    let d = support::temp_dir(name);
    let del = "route GET /del {\n    delete_cookie(\"theme\")\n    return \"deleted\"\n}\n";
    let form = "route POST /notes { form: { text: str } } = \"saved\"\n";
    std::fs::write(d.join("main.qwe"), format!("service /billing = \"billing.qwe\"\nroute GET / = \"shop\"\n{}{}", del, form)).unwrap();
    std::fs::write(d.join("billing.qwe"), format!("route GET /proto = request.headers[\"x-forwarded-proto\"] ?? \"none\"\n{}", del)).unwrap();
    d
}

// A service `--split` started is given no `--trust-proxy`: what it believes of the scheme is its
// app's verdict, signed with the shared secret on each request the app passes on, never the
// visitor's header passed on with it (review). A named proxy's https makes the service's deletion
// `Secure`, as the app's; with no proxy named, neither is. It says nothing of the header, where its
// app says once what to name (no proxy named), or nothing (its proxy named). The service still sees
// the header as it came. A push subscription is the app's alone: a service serves no
// `/_quillo/push`.
#[test]
fn q986_a_split_service_says_nothing_of_the_header_its_app_passes_on() {
    let d = split_dir("f70b-split");
    for (i, (opts, said)) in [(&[][..], 1), (&["--trust-proxy", "127.0.0.1"][..], 0)].into_iter().enumerate() {
        let log = d.join(format!("split-{}.log", i));
        let (_g, port, _) = split(&d, opts, &log);
        let deletion = |path: &str, hs: &[(&str, &str)]| {
            let a = ask("127.0.0.1", port, "GET", path, hs, "");
            assert_eq!(a.status, 200, "{:?} {}: {}", opts, path, a.body);
            secure(a.header("set-cookie").unwrap_or(""))
        };
        for _ in 0..3 {
            assert_eq!(ask("127.0.0.1", port, "GET", "/billing/proto", &[("x-forwarded-proto", "https")], "").body, "https", "{:?}", opts);
            // The service's deletion and the app's own: Secure where the app's proxy is named.
            assert_eq!(deletion("/billing/del", &[("x-forwarded-proto", "https")]), said == 0, "{:?}", opts);
            assert_eq!(deletion("/del", &[("x-forwarded-proto", "https")]), said == 0, "{:?}", opts);
            assert!(!deletion("/billing/del", &[("x-forwarded-proto", "http")]), "{:?}", opts);
            assert!(!deletion("/billing/del", &[]), "{:?}", opts);
        }
        // A word a client writes without the secret is dropped by the app: the service takes the
        // app's verdict alone (one signed with the secret is believed by the app too, whose
        // verdict it then is).
        let forged = word("https", now()).replace("sig=", "sig=0");
        assert!(!deletion("/billing/del", &[("x-quillo-scheme", &forged)]), "{:?}", opts);
        let forged = word("http", now()).replace("sig=", "sig=0");
        assert_eq!(deletion("/billing/del", &[("x-forwarded-proto", "https"), ("x-quillo-scheme", &forged)]), said == 0, "{:?}", opts);
        // The service's lines come to the app's log through a pipe: given time to.
        std::thread::sleep(Duration::from_millis(500));
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        assert_eq!(text.matches(SAID).count(), said, "{:?}: {}", opts, text);
        if said == 1 {
            assert!(text.contains(&not_named("127.0.0.1")), "{}", text);
        }
    }
}

/// The app's word on a scheme as `quillo run --split` signs it, with the tests' service secret.
fn word(scheme: &str, t: u64) -> String {
    let sig = quillo::crypto::hex(&quillo::crypto::hmac_sha256(b"0123456789abcdef0123456789abcdef", format!("quillo-scheme-1\n{}\n{}", scheme, t).as_bytes()));
    format!("{};t={};sig={}", scheme, t, sig)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

// The word is believed by a service `--split` started alone, from a peer on this machine (its app,
// here the test), signed with the shared secret, within five minutes: a bad signature, an old or
// future time, a scheme that is neither, a word cut short or with more after it, none (recheck).
#[test]
fn q986_a_split_service_believes_a_word_signed_recent_and_local_only() {
    let d = split_dir("f70b-word");
    let (_g, _, own) = split(&d, &[], &d.join("word.log"));
    let del = |w: &str| {
        let a = ask("127.0.0.1", own, "GET", "/del", &[("x-quillo-scheme", w)], "");
        assert_eq!(a.status, 200, "{}", a.body);
        secure(a.header("set-cookie").unwrap_or(""))
    };
    let t = now();
    // With the visitor's header beside it: the word decides.
    let a = ask("127.0.0.1", own, "GET", "/del", &[("x-quillo-scheme", &word("https", t)), ("x-forwarded-proto", "http")], "");
    assert!(secure(a.header("set-cookie").unwrap_or("")), "{:?}", a.headers);
    assert!(del(&word("https", t)));
    assert!(del(&word("https", t - 290)) && del(&word("https", t + 290)));
    assert!(!del(&word("http", t)));
    let bad = word("https", t).replace("sig=", "sig=0");
    for w in [word("https", t - 400), word("https", t + 400), bad, word("wss", t), word("HTTPS", t), "https".to_string(), format!("{};x", word("https", t)), format!("https;t={}", t), String::new()] {
        assert!(!del(&w), "{}", w);
    }
    // From another machine (a peer that is no loopback address): not believed. Reached over the
    // machine's own non-loopback address, where it has one.
    if let Some(ip) = own_address() {
        let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(["run", "billing.qwe", "--port", "0", "--host", "0.0.0.0", "--as-service"])
            .env("QUILLO_SECRET", "test secret for the tests")
            .env("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef")
            .env("QUILLO_STOP_WITH_APP", "1")
            .current_dir(&d)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(d.join("far.log")).unwrap())
            .spawn()
            .unwrap();
        let mut g = support::Guard::new(child);
        let far = support::port_in_log(&mut g.0, &d.join("far.log"), Duration::from_secs(30));
        let a = ask(&ip, far, "GET", "/del", &[("x-quillo-scheme", &word("https", now()))], "");
        assert_eq!(a.status, 200, "{}", a.body);
        assert!(!secure(a.header("set-cookie").unwrap_or("")), "from {}: {:?}", ip, a.headers);
        let a = ask("127.0.0.1", far, "GET", "/del", &[("x-quillo-scheme", &word("https", now()))], "");
        assert!(secure(a.header("set-cookie").unwrap_or("")), "{:?}", a.headers);
    }
}

/// This machine's address on its network, if it has one other than loopback.
fn own_address() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

// Any server but a service `--split` started ignores the word, a valid one too, from this machine:
// the app with its proxy named takes the proxy's https for the deletion and for the form gate,
// whatever "http" word a client adds; with no proxy named, a valid "https" word changes nothing,
// and the line is said for the header beside it (recheck).
#[test]
fn q986_the_app_ignores_a_clients_word() {
    let d = split_dir("f70b-app-word");
    let (_g, port, _) = split(&d, &["--trust-proxy", "127.0.0.1"], &d.join("app.log"));
    let http = word("http", now());
    let a = ask("127.0.0.1", port, "GET", "/del", &[("x-forwarded-proto", "https"), ("x-quillo-scheme", &http)], "");
    assert!(a.status == 200 && secure(a.header("set-cookie").unwrap_or("")), "{:?}", a.headers);
    let host = format!("127.0.0.1:{}", port);
    let post = |origin: &str| {
        let hs = [("host", host.as_str()), ("content-type", "application/x-www-form-urlencoded"), ("x-forwarded-proto", "https"), ("x-quillo-scheme", http.as_str()), ("origin", origin)];
        ask("127.0.0.1", port, "POST", "/notes", &hs, "text=hi").status
    };
    assert_eq!(post(&format!("http://{}", host)), 403);
    assert_eq!(post(&format!("https://{}", host)), 200);
    let d = dir("app-word-none");
    let log = d.join("server.log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", "0"])
        .env("QUILLO_SECRET", "test secret for the tests")
        .env("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef")
        .current_dir(&d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_log(&mut g.0, &log, Duration::from_secs(30));
    let a = ask("127.0.0.1", port, "GET", "/del", &[("x-quillo-scheme", &word("https", now())), ("x-forwarded-proto", "https")], "");
    assert!(a.status == 200 && !secure(a.header("set-cookie").unwrap_or("")), "{:?}", a.headers);
    assert_eq!(std::fs::read_to_string(&log).unwrap_or_default().matches(SAID).count(), 1);
}

// The warning for a `Secure` cookie sent over plain HTTP to another machine reads the header as the
// gate does, by its last item: a named proxy's `http, https` (the visitor at 203.0.113.9 by its
// `X-Forwarded-For`) keeps it quiet, where the whole value was compared; without the header it is
// said, once.
#[test]
fn q986_the_plain_http_cookie_warning_reads_the_last_item() {
    let d = dir("warn");
    let r = run(&d, &["--trust-proxy", "127.0.0.1"], "127.0.0.1");
    let warned = "quillo: warning: the Secure cookie `theme` was sent to http://shop.example over plain HTTP, which browsers drop. Serve the site over HTTPS (a proxy with TLS in front, named with --trust-proxy, sending x-forwarded-proto: https), or run with --plain-http on a network you trust\n";
    let from = [("host", "shop.example"), ("x-forwarded-for", "203.0.113.9")];
    for proto in ["http, https", "https", " HTTPS "] {
        let mut hs = from.to_vec();
        hs.push(("x-forwarded-proto", proto));
        assert!(secure(&cookie(&r, "/set", &hs)));
        assert!(!r.log().contains(warned), "{}: {}", proto, r.log());
    }
    for _ in 0..2 {
        assert!(secure(&cookie(&r, "/set", &from)));
    }
    assert_eq!(r.log().matches(warned).count(), 1, "{}", r.log());
    assert!(!r.log().contains(SAID), "{}", r.log());
}

// Under `quillo fcgi` the refusal for the scheme names the web server's parameter it came from:
// `REQUEST_SCHEME` here, saying http where the browser's page was https (TLS ended in front of the
// web server), with how to set `HTTPS` from the front's header.
#[cfg(unix)]
#[test]
fn q987_under_fcgi_the_warning_names_request_scheme() {
    let d = dir("fcgi-request-scheme");
    let f = Fcgi::start(&d);
    assert_eq!(f.form(&[("REQUEST_SCHEME", "http"), ("HTTP_ORIGIN", "https://example.com")]), 403);
    assert_eq!(f.form(&[("HTTPS", "on"), ("HTTP_ORIGIN", "http://example.com")]), 403);
    let log = f.log();
    assert_eq!(log.matches("taken as another site's").count(), 1, "{}", log);
    assert!(log.contains("quillo: warning: a request was taken as another site's: its Origin is https:// and the web server's REQUEST_SCHEME parameter says it came over http. If browsers reach this site over https://, TLS ends in front of the web server and its REQUEST_SCHEME parameter is its own hop's scheme, and older browsers' writes are all refused: have it set HTTPS from the front's header (Apache: SetEnvIf X-Forwarded-Proto https HTTPS=on). Said once\n"), "{}", log);
}
