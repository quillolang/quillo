//! The bridge between an app's program and its page (src/embed.rs): a page's own origin, and a
//! stream's frames as bytes. A program's channels, a stream's watching too, are answered by the
//! first program a process opens, so this is a process of its own (FEEDBACK51.md, Q-642: a stream
//! with no `watch` rule takes no watchers).

use quillo::embed::{bridge_message, request_of, Program};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

fn open(name: &str, src: &str) -> (Program, PathBuf) {
    let root = std::env::temp_dir().join(format!("quillo-cover-embed-media-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (from, to) = (root.join("bundle"), root.join("data"));
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("main.qwe"), src).unwrap();
    (Program::open(&from, &to, "", "{}", "mobile").unwrap(), to)
}

fn own() -> Vec<(String, String)> {
    vec![("origin".into(), "quillo://app".into())]
}

/// A video frame as a page sends it.
fn wire(stream: &str, data: &[u8]) -> Vec<u8> {
    let mut w = vec![stream.len() as u8];
    w.extend_from_slice(stream.as_bytes());
    w.extend_from_slice(&[quillo::media::VIDEO, 1, 0, 0, 0, 7]);
    w.extend_from_slice(data);
    w
}

#[test]
fn the_bridge_carries_binary_frames_and_a_page_s_own_origin() {
    // A stream pages may watch: one with no `watch` rule takes no watchers (FEEDBACK51.md, Q-642).
    let (p, _) = open("bridge", "media.on(\"cover-bridge\", { watch: (peer) => true })\nroute GET /origin = request.headers.origin ?? \"none\"\n");
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
    // The program says yes to watching a moment later: until then, frames go to no one.
    let w = wire("cover-bridge", b"key");
    let mut f = serde_json::Value::Null;
    for _ in 0..200 {
        quillo::media::from_page(&quillo::media::key("cover-bridge", None), w.clone());
        if let Ok(m) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
            f = serde_json::from_str(&m).unwrap();
            break;
        }
    }
    assert_eq!((f["op"].as_str(), f["id"].as_u64()), (Some("frame"), Some(id)), "{}", f);
    let b64 = f["bin"].as_str().unwrap();
    assert_eq!(b64.len(), (w.len() + 2) / 3 * 4, "{}", f);
    msg(serde_json::json!({ "op": "close", "id": id }));
    assert_eq!(next()["op"], "closed");
    p.stop();
}
