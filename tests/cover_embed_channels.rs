//! A program inside an app (src/embed.rs) and its channels, over the bridge's WebSocket:
//! messages of every length each way, the program's ping answered by the page's end, and a close
//! from the page answered. One program for the process: a channel's messages reach the program
//! a process opened first, as its jobs do.

use quillo::embed::{request_of, Frame, Program};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

fn own() -> Vec<(String, String)> {
    vec![("origin".into(), "quillo://app".into())]
}

/// A connection to the program's channels: its number and what comes on it.
fn channels(p: &Program) -> (u64, mpsc::Receiver<Frame>) {
    let t = p.request_whole(request_of("POST", "quillo://app/_quillo/wt-ticket", own(), vec![]));
    let ticket: serde_json::Value = serde_json::from_slice(&t.body).unwrap();
    let url = format!("quillo://app/_quillo/wt?t={}", ticket["ticket"].as_str().unwrap());
    let (tx, rx) = mpsc::channel();
    let conn = p.connect(request_of("GET", &url, own(), vec![]), Box::new(move |f| tx.send(f).is_ok())).unwrap();
    (conn, rx)
}

fn next_text(rx: &mpsc::Receiver<Frame>) -> String {
    match rx.recv_timeout(WAIT).unwrap() {
        Frame::Text(t) => t,
        f => panic!("{:?}", f),
    }
}

fn join(p: &Program, conn: u64, rx: &mpsc::Receiver<Frame>, ch: &str) {
    assert!(p.send(conn, Frame::Text(format!("[\"j\",\"{}\"]", ch))));
    assert_eq!(next_text(rx), format!("[\"j\",\"{}\",true]", ch));
}

#[test]
fn channel_messages_of_every_length_a_ping_and_a_close_go_through_the_bridge() {
    let root = std::env::temp_dir().join(format!("quillo-cover-embed-ch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bundle")).unwrap();
    std::fs::write(root.join("bundle/main.qwe"), "channel.on(\"echo\", (m, peer) => channel.send(\"echo\", peer, m))\nroute GET / = \"hi\"\n").unwrap();
    let p = Program::open(&root.join("bundle"), &root.join("data"), "", "{}", "app").unwrap();

    // One connection left quiet, for the program's ping in 20 s.
    let quiet_since = Instant::now();
    let (quiet, quiet_rx) = channels(&p);

    let (conn, rx) = channels(&p);
    join(&p, conn, &rx, "echo");
    // Under 126 bytes, under 64 KiB, and over: each length's own framing, each way.
    for n in [10usize, 300, 70_000] {
        let m = "e".repeat(n);
        assert!(p.send(conn, Frame::Text(format!("[\"m\",\"echo\",\"{}\"]", m))));
        let back = next_text(&rx);
        assert!(back.contains(&m) && back.len() < n + 40, "{} for {}", back.len(), n);
    }
    // The page closes: the program answers with its close, and the connection is gone.
    assert!(p.send(conn, Frame::Close));
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), Frame::Close);
    let deadline = Instant::now() + WAIT;
    while p.send(conn, Frame::Text("[]".into())) {
        assert!(Instant::now() < deadline, "the connection is gone");
        std::thread::sleep(Duration::from_millis(20));
    }

    // The quiet one: the ping is no message to the page, and, answered, the connection stays.
    let left = Duration::from_millis(21_500).saturating_sub(quiet_since.elapsed());
    assert!(quiet_rx.recv_timeout(left).is_err(), "nothing came");
    join(&p, quiet, &quiet_rx, "echo");
    assert!(p.send(quiet, Frame::Text("[\"m\",\"echo\",\"still\"]".into())));
    assert!(next_text(&quiet_rx).contains("still"));
    p.close(quiet);
    p.stop();
}
