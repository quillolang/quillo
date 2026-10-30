//! `http` at its edges (src/fetch.rs), against servers on this machine: its
//! options, what `cache` keeps and lets go, and streams that end badly.

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn ev(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn fails(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

/// A request as a test's server sees it.
struct Req {
    head: String,
    body: Vec<u8>,
}

fn read_request(s: &mut TcpStream) -> Option<Req> {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
    while buf.len() < end + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    Some(Req { head, body: buf[end..end + len].to_vec() })
}

/// A server calling `f` with each request, on a thread of its own; its port.
fn server(f: impl Fn(Req, &mut TcpStream) + Send + Sync + 'static) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let f = Arc::new(f);
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let f = f.clone();
            std::thread::spawn(move || {
                if let Some(r) = read_request(&mut s) {
                    f(r, &mut s);
                }
            });
        }
    });
    port
}

fn reply(s: &mut TcpStream, ctype: &str, body: &[u8]) {
    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", ctype, body.len());
    let _ = s.write_all(body);
}

#[test]
fn options_are_checked() {
    let u = "\"http://127.0.0.1:9/\"";
    let opt = |o: &str| fails(&format!("http.get({}, {{}}, {})", u, o));
    assert!(opt("5").contains("`http.get` takes options as a map, got int"));
    assert!(opt("{ redirects: 21 }").contains("`redirects` is a whole number from 0 to 20, got 21"));
    assert!(opt("{ public_only: 1 }").contains("`public_only` is true or false, got int"));
    assert!(opt("{ cache: -1 }").contains("`cache` is seconds, a whole number from 0 to 86400, got -1"));
    assert!(opt("{ timeout: \"soon\" }").contains("`timeout` is seconds, more than 0 and up to 300, got soon"));
    assert!(opt("{ timeout: 0 }").contains("`timeout` is seconds, more than 0 and up to 300, got 0"));
    assert!(opt("{ timeout: 301.5 }").contains("got 301.5"));
    assert!(opt("{ retries: 1 }").contains("unknown option `retries` (the options are redirects, public_only, cache, timeout)"));
    assert!(fails(&format!("http.stream({}, (x) => x, {{ retries: 1 }})", u)).contains("(the options are method, body, headers, redirects, public_only, cache, timeout)"));
    assert!(fails(&format!("http.post({}, null, {{}}, {{ cache: 5 }})", u)).contains("`cache` keeps the answers of GET requests, not POST"));
    assert!(fails("http.get(5)").contains("`http.get` expects a URL string, got int"));
    assert!(fails(&format!("http.get({}, 5)", u)).contains("expects headers as a map, got int"));
    assert!(fails("http.request(1, \"http://x/\")").contains("expects a method string first, got int"));
}

#[test]
fn a_body_says_what_it_is_and_a_timeout_ends_a_call() {
    let port = server(|r, s| {
        let ctype = r.head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-type:").map(|v| v.trim().to_string())).unwrap_or_default();
        let body = format!("{} {} {}", r.head.split(' ').next().unwrap(), ctype, String::from_utf8_lossy(&r.body));
        reply(s, "text/plain", body.as_bytes());
    });
    let url = format!("http://127.0.0.1:{}/", port);
    assert_eq!(ev(&format!("http.post({:?}, <b>bold</b>).body", url)), "\"POST text/html; charset=utf-8 <b>bold</b>\"");
    assert_eq!(ev(&format!("http.request(\"patch\", {:?}, {{ a: 1 }}, {{}}, {{ timeout: 5 }}).body", url)), "\"PATCH application/json {\\\"a\\\":1}\"");
    assert_eq!(ev(&format!("http.put({:?}, \"x\", {{ \"content-type\": \"text/x\" }}, {{ timeout: 2.5 }}).body", url)), "\"PUT text/x x\"");

    let quiet = server(|_, _| std::thread::sleep(Duration::from_secs(3)));
    let out = ev(&format!("http.get(\"http://127.0.0.1:{}/\", {{}}, {{ timeout: 0.1 }})", quiet));
    assert!(out.contains("took more than 0.1 s (its timeout)"), "{}", out);
}

/// One test at a time fills the cache to its limit.
static CACHE: Mutex<()> = Mutex::new(());

#[test]
fn a_kept_answer_ends_when_its_time_is_up() {
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let port = server(move |_, s| {
        let n = h.fetch_add(1, Ordering::SeqCst) + 1;
        reply(s, "text/plain", n.to_string().as_bytes());
    });
    let src = format!("http.get(\"http://127.0.0.1:{}/\", {{}}, {{ cache: 1 }}).body", port);
    assert_eq!(ev(&src), "\"1\"");
    assert_eq!(ev(&src), "\"1\"");
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(ev(&src), "\"2\"");
}

#[test]
fn an_answer_too_large_to_keep_is_not_kept() {
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let port = server(move |_, s| {
        h.fetch_add(1, Ordering::SeqCst);
        reply(s, "text/plain", &vec![b'x'; 17 << 20]);
    });
    let src = format!("len(http.get(\"http://127.0.0.1:{}/\", {{}}, {{ cache: 60 }}).body)", port);
    assert_eq!(ev(&src), (17 << 20).to_string());
    assert_eq!(ev(&src), (17 << 20).to_string());
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn two_calls_at_once_each_keep_their_answer_and_one_stays() {
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let port = server(move |_, s| {
        let n = h.fetch_add(1, Ordering::SeqCst) + 1;
        // Both are here before either hears.
        while h.load(Ordering::SeqCst) < 2 {
            std::thread::sleep(Duration::from_millis(5));
        }
        reply(s, "text/plain", n.to_string().as_bytes());
    });
    let src = format!("http.get(\"http://127.0.0.1:{}/\", {{}}, {{ cache: 60 }}).body", port);
    let (a, b) = (src.clone(), src.clone());
    let one = std::thread::spawn(move || ev(&a));
    let two = std::thread::spawn(move || ev(&b));
    let mut got = vec![one.join().unwrap(), two.join().unwrap()];
    got.sort();
    assert_eq!(got, vec!["\"1\"", "\"2\""]);
    // Whichever came last is the one kept.
    let kept = ev(&src);
    assert!(got.contains(&kept), "{}", kept);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
}

#[test]
fn a_full_cache_lets_go_of_what_ends_soonest() {
    let _one = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let hits: Arc<Mutex<Vec<String>>> = Default::default();
    let h = hits.clone();
    let port = server(move |r, s| {
        h.lock().unwrap().push(r.head.split(' ').nth(1).unwrap().to_string());
        reply(s, "text/plain", &vec![b'x'; 15 << 20]);
    });
    let get = |path: &str, secs: u32| ev(&format!("len(http.get(\"http://127.0.0.1:{}/{}\", {{}}, {{ cache: {} }}).body)", port, path, secs));
    let n = (15 << 20).to_string();
    for (path, secs) in [("gone", 1), ("soon", 1000), ("later", 2000), ("last", 3000)] {
        assert_eq!(get(path, secs), n);
    }
    std::thread::sleep(Duration::from_millis(1100));
    // Past 64 MB: what has ended goes, then what ends soonest.
    assert_eq!(get("new", 4000), n);
    assert_eq!(get("soon", 1000), n);
    assert_eq!(hits.lock().unwrap().len(), 5, "what ended is gone, and what had not is kept");
    assert_eq!(get("newer", 5000), n);
    assert_eq!(hits.lock().unwrap().len(), 6);
    assert_eq!(get("later", 2000), n);
    assert_eq!(get("last", 3000), n);
    assert_eq!(get("new", 4000), n);
    assert_eq!(get("newer", 5000), n);
    assert_eq!(hits.lock().unwrap().len(), 6, "those kept were not asked for again");
    assert_eq!(get("soon", 1000), n);
    assert_eq!(hits.lock().unwrap().len(), 7, "what ended soonest was let go");
}

#[test]
fn a_stream_takes_what_a_call_takes() {
    assert!(fails("http.stream(5, (x) => x)").contains("`http.stream` expects a URL string, got int"));
    assert!(fails("http.stream(\"http://x/\", 5)").contains("takes a function for what arrives, got int"));
    assert!(fails("http.stream(\"http://x/\", (x) => x, 5)").contains("takes options as a map, got int"));
    assert!(fails("http.stream(\"http://x/\", (x) => x, { cache: 5 })").contains("`cache` keeps whole answers, of `http.get`"));
    assert_eq!(ev("http.stream(\"ftp://x/\", (x) => x)"), "err(\"http: not an http(s) URL: ftp://x/\")");
    assert_eq!(ev("http.get(\"ftp://x/\")"), "err(\"http: not an http(s) URL: ftp://x/\")");

    let port = server(|r, s| {
        let ctype = r.head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-type:").map(|v| v.trim().to_string())).unwrap_or_default();
        let auth = r.head.lines().find_map(|l| l.strip_prefix("x-who: ").map(str::to_string)).unwrap_or_default();
        let body = format!("{} [{}] [{}] {}", r.head.split(' ').next().unwrap(), ctype, auth, String::from_utf8_lossy(&r.body));
        reply(s, "text/plain", body.as_bytes());
    });
    let got = |opts: &str| ev(&format!("let got = \"\"\nhttp.stream(\"http://127.0.0.1:{}/\", (t) => {{ got = got + t }}, {})\ngot", port, opts));
    assert_eq!(got("null"), "\"GET [] [] \"");
    assert_eq!(got("{ body: \"text\" }"), "\"POST [text/plain; charset=utf-8] [] text\"");
    assert_eq!(got("{ body: { a: 1 }, method: \"put\" }"), "\"PUT [application/json] [] {\\\"a\\\":1}\"");
    assert_eq!(got("{ body: \"x\", headers: { \"x-who\": \"ana\", \"content-type\": \"text/x\" }, timeout: 5 }"), "\"POST [text/x] [ana] x\"");
}

#[test]
fn a_stream_that_fails_gives_its_body_or_its_error() {
    let port = server(|r, s| match r.head.split(' ').nth(1).unwrap() {
        "/missing" => {
            let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope");
        }
        "/cut" => {
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 100\r\nConnection: close\r\n\r\nstart");
        }
        _ => std::thread::sleep(Duration::from_secs(3)),
    });
    let v = ev(&format!("let r = http.stream(\"http://127.0.0.1:{}/missing\", (t) => panic(t))\n[r.status, r.ok, r.body]", port));
    assert_eq!(v, "[404, false, \"nope\"]");
    let v = ev(&format!("let got = []\nlet r = http.stream(\"http://127.0.0.1:{}/cut\", (t) => {{ got = got + [t] }})\n[got, is_err(r)]", port));
    assert_eq!(v, "[[\"start\"], true]");
    let v = ev(&format!("http.stream(\"http://127.0.0.1:{}/slow\", (t) => null, {{ timeout: 0.1 }})", port));
    assert!(v.contains("took more than 0.1 s (its timeout)"), "{}", v);
    let v = ev("http.stream(\"http://127.0.0.1:1/\", (t) => null)");
    assert!(v.starts_with("err(\"http: "), "{}", v);
}

#[test]
fn a_line_or_event_with_no_end_is_given_up_on() {
    let port = server(|r, s| {
        let ctype = if r.head.contains("/lines") { "application/x-ndjson" } else { "text/event-stream" };
        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nConnection: close\r\n\r\n", ctype);
        let chunk = vec![b'x'; 1 << 20];
        for _ in 0..18 {
            if s.write_all(&chunk).is_err() {
                return;
            }
        }
    });
    let v = ev(&format!("http.stream(\"http://127.0.0.1:{}/lines\", (t) => null)", port));
    assert_eq!(v, format!("err(\"http: a line longer than {} bytes\")", 16 << 20));
    let v = ev(&format!("http.stream(\"http://127.0.0.1:{}/events\", (t) => null)", port));
    assert_eq!(v, format!("err(\"http: a event longer than {} bytes\")", 16 << 20));
}

#[test]
fn a_stream_is_cut_up_at_whole_characters_events_and_lines() {
    let port = server(|r, s| {
        let (ctype, parts): (&str, Vec<&[u8]>) = match r.head.split(' ').nth(1).unwrap() {
            "/split" => ("text/plain", vec![b"caf\xc3", b"\xa9 ok"]),
            "/bad" => ("text/plain", vec![b"a\xff", b"b\xc3"]),
            "/events" => ("text/event-stream; charset=utf-8", vec![b"data: zero\n\nid: 7\ndata: one\n\r\n", b": note\n\n", b"event: x\ndata: {\"n\": 2}\r\n\r\n", b"retry: 5\n"]),
            _ => ("application/jsonl", vec![b"{\"a\":1}\n\n", b"[2]"]),
        };
        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nConnection: close\r\n\r\n", ctype);
        for p in parts {
            let _ = s.write_all(p);
            let _ = s.flush();
            std::thread::sleep(Duration::from_millis(60));
        }
    });
    let stream = |path: &str, f: &str| ev(&format!("let got = []\nlet r = http.stream(\"http://127.0.0.1:{}{}\", {})\n[got, r.status]", port, path, f));
    assert_eq!(stream("/split", "(t) => { got = got + [t] }"), "[[\"caf\", \"é ok\"], 200]");
    assert_eq!(stream("/bad", "(t) => { got = got + [t] }"), "[[\"a\u{fffd}\", \"b\", \"\u{fffd}\"], 200]");
    assert_eq!(stream("/events", "(d, name) => { got = got + [[d, name]] }"), "[[[\"zero\", null], [\"one\", null], [{ n: 2 }, \"x\"]], 200]");
    assert_eq!(stream("/lines", "(j) => { got = got + [j] }"), "[[{ a: 1 }, [2]], 200]");
    // `false` stops it, and a function may take less than it is given.
    assert_eq!(stream("/events", "() => { got = got + [1]\n false }"), "[[1], 200]");
    assert_eq!(stream("/lines", "(j) => { got = got + [j]\n null }"), "[[{ a: 1 }, [2]], 200]");
}

#[test]
fn addresses_on_the_public_internet_are_told_from_the_others() {
    use quillo::fetch::public_ip;
    let ip = |s: &str| public_ip(s.parse().unwrap());
    for a in ["8.8.8.8", "1.1.1.1", "2606:4700::1111", "::ffff:8.8.8.8", "64:ff9b::808:808", "2002:808:808::1", "2001:4860::8888"] {
        assert!(ip(a), "{}", a);
    }
    for a in [
        "0.0.0.0", "0.1.2.3", "127.0.0.1", "10.0.0.1", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "192.0.0.8", "192.0.2.1", "198.18.0.1", "198.19.0.1", "224.0.0.1", "240.0.0.1",
        "255.255.255.255", "::", "::1", "ff02::1", "::127.0.0.1", "fc00::1", "fd00::1", "fe80::1", "fec0::1", "2001:db8::1", "2001:1::1", "100::1", "::ffff:127.0.0.1", "64:ff9b::7f00:1", "2002:7f00:1::1",
    ] {
        assert!(!ip(a), "{}", a);
    }
}
