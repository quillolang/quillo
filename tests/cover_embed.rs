//! A program inside an app (src/embed.rs), at its corners: programs that cannot be opened, a
//! cookie jar read back from disk, loading again on a change, a WebSocket asked for as a page, a
//! large file asked for by HEAD or left midway, connections that the page or the program ends,
//! and the bridge's binary frames. A program's channels are answered by the first program a
//! process opens, so they are tested in a process of their own (tests/cover_embed_channels.rs).

use quillo::embed::{bridge_message, request_of, Frame, Program, Reply};
use quillo::server::HttpResponse;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

/// A bundle holding `main.qwe`, and the app's data folder (not made yet).
fn bundle(name: &str, src: &str, extra: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("quillo-cover-embed-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (from, to) = (root.join("bundle"), root.join("data"));
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("main.qwe"), src).unwrap();
    for (f, body) in extra {
        std::fs::write(from.join(f), body).unwrap();
    }
    (from, to)
}

fn open(name: &str, src: &str) -> (Program, PathBuf) {
    let (from, to) = bundle(name, src, &[]);
    (Program::open(&from, &to, "", "{}", "mobile").unwrap(), to)
}

fn get(p: &Program, path: &str) -> HttpResponse {
    p.request_whole(request_of("GET", &format!("quillo://app{}", path), vec![], vec![]))
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

fn own() -> Vec<(String, String)> {
    vec![("origin".into(), "quillo://app".into())]
}

/// A media frame as a page sends one: the stream's name, its kind, whether it is a key frame.
fn wire(stream: &str, data: &[u8]) -> Vec<u8> {
    let mut w = vec![stream.len() as u8];
    w.extend_from_slice(stream.as_bytes());
    w.extend_from_slice(&[quillo::media::VIDEO, 1, 0, 0, 0, 7]);
    w.extend_from_slice(data);
    w
}

#[test]
fn an_app_with_no_main_file_a_broken_one_or_no_routes_is_not_opened() {
    let (from, to) = bundle("unopened", "route GET / = \"hi\"\n", &[]);
    let e = Program::open(&from, &to, "nope.qwe", "{}", "mobile").err().unwrap();
    assert_eq!(e, "no nope.qwe in the app's program");

    let (from, to) = bundle("broken", "route GET / = (\n", &[]);
    let e = Program::open(&from, &to, "", "{}", "mobile").err().unwrap();
    assert!(e.contains("main.qwe"), "{}", e);

    let (from, to) = bundle("routeless", "let x = 1\n", &[]);
    let e = Program::open(&from, &to, "", "{}", "mobile").err().unwrap();
    assert!(e.ends_with("main.qwe declares no routes, so there is nothing to show"), "{}", e);
}

#[test]
fn a_request_given_a_bare_path_is_the_apps_own() {
    let r = request_of("get", "notes?x=1#top", vec![("Host".into(), "evil.example".into()), ("Cookie".into(), "a=1".into())], vec![]);
    assert_eq!((r.method.as_str(), r.url.as_str()), ("GET", "/notes?x=1"));
    assert_eq!(r.headers, vec![("host".to_string(), "app".to_string())]);
    assert_eq!(quillo::embed::deliver_js("{\"op\":\"x\"}"), "document.dispatchEvent(new CustomEvent(\"quillo-wire-receive\", { detail: {\"op\":\"x\"} }))");
}

const COOKIES: &str = r#"
route GET /c = `${cookie("keep") ?? "none"} ${cookie("old") ?? "none"} ${cookie("bad") ?? "none"}`
route GET /set {
    set_cookie("t", "v")
    "set"
}
route GET /del {
    delete_cookie("t")
    "deleted"
}
route GET /t = cookie("t") ?? "none"
"#;

#[test]
fn the_jar_kept_on_disk_is_read_back_and_a_deleted_cookie_leaves_it() {
    let (from, to) = bundle("jar", COOKIES, &[]);
    std::fs::create_dir_all(to.join(".quillo")).unwrap();
    let jar = to.join(".quillo/cookies.json");
    // One kept, one past its end, one with no text for its value.
    std::fs::write(&jar, r#"{"keep":{"value":"1","path":"/"},"old":{"value":"x","path":"/","until":1},"bad":{"value":3,"path":"/"}}"#).unwrap();
    let p = Program::open(&from, &to, "", "{}", "mobile").unwrap();
    assert_eq!(text(&get(&p, "/c")), "1 none none");
    assert_eq!(text(&get(&p, "/set")), "set");
    assert_eq!(text(&get(&p, "/t")), "v");
    assert!(std::fs::read_to_string(&jar).unwrap().contains("\"t\""));
    assert_eq!(text(&get(&p, "/del")), "deleted");
    assert_eq!(text(&get(&p, "/t")), "none");
    let kept = std::fs::read_to_string(&jar).unwrap();
    assert!(!kept.contains("\"t\"") && kept.contains("\"keep\""), "{}", kept);
    p.stop();

    // A jar that is not a map of cookies is no jar.
    let (from, to) = bundle("jar-list", COOKIES, &[]);
    std::fs::create_dir_all(to.join(".quillo")).unwrap();
    std::fs::write(to.join(".quillo/cookies.json"), "[1, 2]").unwrap();
    let p = Program::open(&from, &to, "", "{}", "mobile").unwrap();
    assert_eq!(text(&get(&p, "/c")), "none none none");
    p.stop();
}

#[test]
fn watching_loads_the_program_again_when_its_file_changes() {
    let (p, to) = open("watch", "route GET / = \"one\"\n");
    assert_eq!(text(&get(&p, "/")), "one");
    p.watch();
    p.resume();
    // Let the watcher see the file as it is first.
    std::thread::sleep(Duration::from_millis(500));
    std::fs::write(to.join("app/main.qwe"), "route GET / = \"two\"\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while text(&get(&p, "/")) != "two" {
        assert!(Instant::now() < deadline, "never loaded again");
        std::thread::sleep(Duration::from_millis(50));
    }
    p.stop();
}

#[test]
fn a_websocket_asked_for_as_a_page_is_answered_101_and_ended() {
    let (p, _) = open("ws-page", "route GET / = \"hi\"\n");
    let mut hs = own();
    for (k, v) in [("upgrade", "websocket"), ("connection", "Upgrade"), ("sec-websocket-version", "13"), ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")] {
        hs.push((k.into(), v.into()));
    }
    let r = p.request_whole(request_of("GET", "quillo://app/_quillo/live?since=0", hs, vec![]));
    assert_eq!(r.status, 101);
    assert!(r.headers.iter().any(|(k, v)| k == "sec-websocket-accept" && v == "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "{:?}", r.headers);
    assert!(r.body.is_empty());
    p.stop();
}

#[test]
fn a_large_file_asked_for_by_head_has_its_length_and_a_page_gone_midway_stops_it() {
    let big = vec![9u8; 3 << 20];
    let (from, to) = bundle("big", "route GET /big = file(\"big.bin\")\n", &[("big.bin", &big)]);
    let p = Program::open(&from, &to, "", "{}", "mobile").unwrap();
    let head = p.request_whole(request_of("HEAD", "/big", vec![], vec![]));
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert!(head.headers.iter().any(|(k, v)| k == "content-length" && v == &big.len().to_string()), "{:?}", head.headers);

    // A reply that takes one part and is gone: no more is read for it, and it is not ended.
    struct OnePart(Vec<usize>, bool, Option<mpsc::Sender<(Vec<usize>, bool)>>);
    impl Reply for OnePart {
        fn head(&mut self, _: u16, _: &[(String, String)]) {}
        fn body(&mut self, part: &[u8]) -> bool {
            self.0.push(part.len());
            false
        }
        fn end(&mut self) {
            self.1 = true;
        }
    }
    impl Drop for OnePart {
        fn drop(&mut self) {
            if let Some(tx) = self.2.take() {
                let _ = tx.send((std::mem::take(&mut self.0), self.1));
            }
        }
    }
    let (tx, rx) = mpsc::channel();
    p.request(request_of("GET", "/big", vec![], vec![]), Box::new(OnePart(Vec::new(), false, Some(tx))));
    let (parts, ended) = rx.recv_timeout(WAIT).unwrap();
    assert_eq!(parts.len(), 1, "{:?}", parts);
    assert!(parts[0] > 0 && parts[0] < big.len());
    assert!(!ended);
    // The program answers the next as before.
    assert_eq!(get(&p, "/big").body.len(), big.len());
    p.stop();
}

#[test]
fn a_page_that_stops_taking_messages_ends_its_connection() {
    let (p, _) = open("stops", "route GET / = \"hi\"\n");
    let (tx, rx) = mpsc::channel();
    let conn = p
        .connect(
            request_of("GET", "/_quillo/live?since=0&t=notes", own(), vec![]),
            Box::new(move |f| {
                let _ = tx.send(f);
                false
            }),
        )
        .unwrap();
    assert!(matches!(rx.recv_timeout(WAIT).unwrap(), Frame::Text(t) if t.contains("seq")));
    let deadline = Instant::now() + WAIT;
    while p.send(conn, Frame::Text("x".into())) {
        assert!(Instant::now() < deadline, "the connection is gone");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "nothing more, not even its end");
    p.stop();
}

#[test]
fn stopping_the_program_ends_the_connections_still_open() {
    let (p, _) = open("stop", "route GET / = \"hi\"\n");
    let (tx, rx) = mpsc::channel();
    let conn = p.connect(request_of("GET", "/_quillo/live?since=0&t=notes", own(), vec![]), Box::new(move |f| tx.send(f).is_ok())).unwrap();
    assert!(matches!(rx.recv_timeout(WAIT).unwrap(), Frame::Text(_)));
    p.stop();
    let deadline = Instant::now() + WAIT;
    loop {
        match rx.recv_timeout(WAIT).unwrap() {
            Frame::Close => break,
            _ => assert!(Instant::now() < deadline),
        }
    }
    assert!(!p.send(conn, Frame::Text("x".into())));
}

#[test]
fn the_bridge_carries_binary_frames_and_a_page_s_own_origin() {
    let (p, _) = open("bridge", "route GET /origin = request.headers.origin ?? \"none\"\n");
    let (tx, rx) = mpsc::channel::<String>();
    let page: std::sync::Arc<dyn Fn(String) + Send + Sync> = std::sync::Arc::new(move |m| {
        let _ = tx.send(m);
    });
    let msg = |m: serde_json::Value| bridge_message(&p, &m.to_string(), page.clone());
    let next = || serde_json::from_str::<serde_json::Value>(&rx.recv_timeout(WAIT).unwrap()).unwrap();

    // A URL that is a path alone: the app's own origin.
    msg(serde_json::json!({ "op": "fetch", "id": 1, "method": "GET", "url": "/origin" }));
    let a = next();
    assert_eq!((a["op"].as_str(), a["status"].as_u64(), a["body"].as_str()), (Some("answer"), Some(200), Some("cXVpbGxvOi8vYXBw")), "{}", a);

    let t = p.request_whole(request_of("POST", "quillo://app/_quillo/wt-ticket", own(), vec![]));
    let ticket: serde_json::Value = serde_json::from_slice(&t.body).unwrap();
    let id = 7001;
    msg(serde_json::json!({ "op": "open", "id": id, "url": format!("quillo://app/_quillo/wt?t={}", ticket["ticket"].as_str().unwrap()) }));
    assert_eq!(next()["op"], "opened");
    // Watching a stream; a send with neither text nor bytes is nothing.
    msg(serde_json::json!({ "op": "send", "id": id, "text": "[\"v\",\"cover-bridge\"]" }));
    msg(serde_json::json!({ "op": "send", "id": id }));
    msg(serde_json::json!({ "op": "send", "id": id, "text": "[\"j\",\"chat\"]" }));
    let j = next();
    assert_eq!((j["op"].as_str(), j["text"].as_str()), (Some("frame"), Some("[\"j\",\"chat\",true]")), "{}", j);
    // A stream's frame reaches the page as bytes.
    let w = wire("cover-bridge", b"key");
    quillo::media::from_page(w.clone());
    let f = next();
    assert_eq!((f["op"].as_str(), f["id"].as_u64()), (Some("frame"), Some(id)), "{}", f);
    let b64 = f["bin"].as_str().unwrap();
    assert_eq!(b64.len(), (w.len() + 2) / 3 * 4, "{}", f);
    msg(serde_json::json!({ "op": "close", "id": id }));
    assert_eq!(next()["op"], "closed");
    p.stop();
}
