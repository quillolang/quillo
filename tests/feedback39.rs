//! FEEDBACK39.md: a program inside an app with no HTTP server (Q-465 to Q-470; branch
//! feedback36-39). The app hands the program each request; nothing listens.

use quillo::embed::{request_of, Frame, Program, Reply};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

fn bundle(name: &str, src: &str, extra: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("quillo-embed-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (from, to) = (root.join("bundle"), root.join("data"));
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("main.qwe"), src).unwrap();
    for (f, body) in extra {
        std::fs::write(from.join(f), body).unwrap();
    }
    (from, to)
}

fn get(p: &Program, path: &str) -> quillo::server::HttpResponse {
    p.request_whole(request_of("GET", &format!("quillo://app{}", path), vec![], vec![]))
}

fn text(r: &quillo::server::HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

fn header<'a>(r: &'a quillo::server::HttpResponse, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// One test for one process: the platform's runtime, and jobs answered here, are set once.
#[test]
fn a_program_runs_in_the_app_with_no_socket() {
    let src = r#"
db.open("app.db")
db.exec("create table if not exists notes (t text)")
let hits = 0
every(50, () => { hits = hits + 1 })
route GET / = <html><body><h1>Notes</h1></body></html>
route GET /hits = { hits: hits }
route POST /notes {
    db.exec("insert into notes values (?)", request.json.t)
    { saved: request.json.t }
}
route GET /notes = db.query("select t from notes")
route GET /login {
    set_session({ user: "ada" })
    "in"
}
route GET /me = session()?.user ?? "nobody"
route GET /where = { remote: request.ip, runtime: platform().runtime }
route GET /s = stream((send) => {
    send("one ")
    send("two")
})
route GET /big = file("big.bin")
route POST /form = redirect("/notes")
route GET /peers = len(channel.peers("chat"))
route GET /hi = channel.broadcast("chat", "hello")
server fn ping() = "pong"
"#;
    let big = vec![7u8; 3 << 20];
    let (from, to) = bundle("main", src, &[("big.bin", &big)]);
    let p = Program::open(&from, &to, "", r#"{ "model": "Test1,1" }"#, "mobile").unwrap();

    // Pages, through the scheme: no port, no launch token.
    let home = get(&p, "/");
    assert_eq!(home.status, 200);
    assert!(text(&home).contains("<h1>Notes</h1>"), "{}", text(&home));
    let w: serde_json::Value = serde_json::from_slice(&get(&p, "/where").body).unwrap();
    assert_eq!(w["remote"], "app");
    assert_eq!(w["runtime"], "mobile");

    // A body, and the database in the app's own folder.
    let saved = p.request_whole(request_of("POST", "quillo://app/notes", vec![("content-type".into(), "application/json".into())], br#"{"t":"milk"}"#.to_vec()));
    assert_eq!(text(&saved), r#"{"saved":"milk"}"#);
    assert!(text(&get(&p, "/notes")).contains("milk"));
    assert!(to.join("app/app.db").exists());

    // The web view keeps no cookies for a custom scheme: the jar does, and keeps them on disk.
    assert_eq!(text(&get(&p, "/me")), "nobody");
    assert_eq!(text(&get(&p, "/login")), "in");
    assert_eq!(text(&get(&p, "/me")), "ada");
    p.suspend();
    let jar = std::fs::read_to_string(to.join(".quillo/cookies.json")).unwrap();
    assert!(jar.contains("quillo_session"), "{}", jar);

    // A server fn, from the program's own origin.
    let rpc = p.request_whole(request_of(
        "POST",
        "quillo://app/_quillo/rpc/ping",
        vec![("content-type".into(), "application/json".into()), ("origin".into(), "quillo://app".into())],
        b"[]".to_vec(),
    ));
    assert_eq!(text(&rpc), r#"{"ok":"pong"}"#, "{}", rpc.status);

    // A stream comes in parts, as it is made.
    struct Parts(Vec<Vec<u8>>, u16, mpsc::Sender<(u16, Vec<Vec<u8>>)>);
    impl Reply for Parts {
        fn head(&mut self, status: u16, _: &[(String, String)]) {
            self.1 = status;
        }
        fn body(&mut self, part: &[u8]) -> bool {
            self.0.push(part.to_vec());
            true
        }
        fn end(&mut self) {
            let _ = self.2.send((self.1, std::mem::take(&mut self.0)));
        }
    }
    let (tx, rx) = mpsc::channel();
    p.request(request_of("GET", "/s", vec![], vec![]), Box::new(Parts(Vec::new(), 0, tx)));
    let (status, parts) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(status, 200);
    assert_eq!(parts.concat(), b"one two");
    assert!(parts.len() >= 2, "sent as it came: {:?}", parts);

    // A large file is sent from disk, by its own thread, as to a client: all of it, and ranges.
    let whole = get(&p, "/big");
    assert_eq!(whole.status, 200);
    assert_eq!(whole.body.len(), big.len());
    let part = p.request_whole(request_of("GET", "/big", vec![("range".into(), "bytes=10-19".into())], vec![]));
    assert_eq!(part.status, 206);
    assert_eq!(part.body, vec![7u8; 10]);

    // A form the scheme could not carry, kept for the page: a redirect is the page's to follow.
    let (id, resp) = p.post(request_of("POST", "/form", vec![("content-type".into(), "application/x-www-form-urlencoded".into())], b"a=1".to_vec()));
    assert!(id.is_none() && resp.status == 303 && header(&resp, "location") == Some("/notes"));
    let (id, _) = p.post(request_of("POST", "/notes", vec![("content-type".into(), "application/json".into())], br#"{"t":"eggs"}"#.to_vec()));
    let id = id.unwrap();
    assert_eq!(text(&get(&p, &format!("/_quillo/posted/{}", id))), r#"{"saved":"eggs"}"#);
    assert_eq!(get(&p, &format!("/_quillo/posted/{}", id)).status, 404, "shown once");

    // Ticks reach the program with no connection to itself (Q-468).
    std::thread::sleep(Duration::from_millis(400));
    let hits: serde_json::Value = serde_json::from_slice(&get(&p, "/hits").body).unwrap();
    assert!(hits["hits"].as_i64().unwrap() >= 3, "{}", hits);

    // Live data over the bridge: a connection, and the program's message on it.
    let (tx, rx) = mpsc::channel();
    let conn = p
        .connect(
            request_of("GET", "/_quillo/live?since=0&t=notes", vec![("origin".into(), "quillo://app".into())], vec![]),
            Box::new(move |f| tx.send(f).is_ok()),
        )
        .unwrap();
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(&first, Frame::Text(t) if t.contains("seq")), "{:?}", first);
    assert!(p.send(conn, Frame::Text("ping".into())));
    p.close(conn);
    assert!(!p.send(conn, Frame::Text("after".into())), "closed");

    // Channels over the bridge, where a page has no WebTransport or WebSocket (Q-470): a ticket,
    // a connection, joining, and the program's message to the channel.
    let t = p.request_whole(request_of("POST", "quillo://app/_quillo/wt-ticket", vec![("origin".into(), "quillo://app".into())], vec![]));
    let ticket: serde_json::Value = serde_json::from_slice(&t.body).unwrap();
    let ticket = ticket["ticket"].as_str().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    let conn = p
        .connect(request_of("GET", &format!("quillo://app/_quillo/wt?t={}", ticket), vec![("origin".into(), "quillo://app".into())], vec![]), Box::new(move |f| tx.send(f).is_ok()))
        .unwrap();
    assert!(p.send(conn, Frame::Text("[\"j\",\"chat\"]".into())));
    let joined = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(joined, Frame::Text("[\"j\",\"chat\",true]".into()));
    assert_eq!(text(&get(&p, "/peers")), "1");
    assert_eq!(text(&get(&p, "/hi")), "1");
    let said = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(&said, Frame::Text(t) if t.contains("hello")), "{:?}", said);
    p.close(conn);

    // The shell's script and the bridge's messages, as an app's web view sends them.
    let js = quillo::embed::shell_js("window.ipc.postMessage", "bridge");
    assert!(js.contains("window.ipc.postMessage(JSON.stringify(m))") && js.contains("\"bridge\""));
    let (tx, rx) = mpsc::channel::<String>();
    let page: std::sync::Arc<dyn Fn(String) + Send + Sync> = std::sync::Arc::new(move |m| {
        let _ = tx.send(m);
    });
    let msg = |m: serde_json::Value| quillo::embed::bridge_message(&p, &m.to_string(), page.clone());
    msg(serde_json::json!({ "op": "fetch", "id": 1, "method": "POST", "url": "quillo://app/notes", "headers": { "content-type": "application/json" }, "body": "eyJ0IjoiamFtIn0=" }));
    let a: serde_json::Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!((a["op"].as_str(), a["id"].as_u64(), a["status"].as_u64()), (Some("answer"), Some(1), Some(200)), "{}", a);
    msg(serde_json::json!({ "op": "post", "id": 2, "method": "POST", "url": "quillo://app/form", "headers": {}, "body": "" }));
    let a: serde_json::Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!((a["op"].as_str(), a["location"].as_str()), (Some("posted"), Some("/notes")));
    msg(serde_json::json!({ "op": "post", "id": 3, "method": "POST", "url": "quillo://app/notes", "headers": { "content-type": "application/json" }, "body": "eyJ0IjoidGVhIn0=" }));
    let a: serde_json::Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap();
    let kept = a["page"].as_str().unwrap();
    assert_eq!(text(&get(&p, &format!("/_quillo/posted/{}", kept))), r#"{"saved":"tea"}"#);
    msg(serde_json::json!({ "op": "open", "id": 4, "url": "quillo://app/_quillo/live?since=0&t=notes" }));
    let mut seen = Vec::new();
    for _ in 0..2 {
        seen.push(serde_json::from_str::<serde_json::Value>(&rx.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap());
    }
    assert!(seen.iter().any(|m| m["op"] == "opened") && seen.iter().any(|m| m["op"] == "frame"), "{:?}", seen);
    msg(serde_json::json!({ "op": "send", "id": 4, "text": "x" }));
    msg(serde_json::json!({ "op": "send", "id": 4, "bin": "AAE=" }));
    msg(serde_json::json!({ "op": "close", "id": 4 }));
    msg(serde_json::json!({ "op": "open", "id": 5, "url": "quillo://app/hello" }));
    let a = loop {
        let m: serde_json::Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(10)).unwrap()).unwrap();
        if m["id"] == 5 {
            break m;
        }
        assert!(m["id"] == 4 && (m["op"] == "closed" || m["op"] == "frame"), "{}", m);
    };
    assert_eq!(a["op"], "failed", "a page, not a connection: {}", a);
    msg(serde_json::json!({ "op": "send", "id": 99, "text": "nobody" }));
    msg(serde_json::json!({ "op": "nonsense" }));
    quillo::embed::bridge_message(&p, "not json", page.clone());

    // Nothing listens: the process has no socket bound for the program.
    p.stop();
}
