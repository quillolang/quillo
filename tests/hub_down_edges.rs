//! The channels' hub with no server where it was told one is (src/wt.rs).

mod support;

use quillo::wt::{self, Out};
use std::time::{Duration, Instant};

#[test]
fn with_no_server_to_ask_no_one_is_let_in() {
    wt::serving(format!("127.0.0.1:{}", support::free_port()).parse().unwrap());
    let exp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 100;
    let sealed = quillo::session::seal("quillo channel ticket", &serde_json::json!({ "s": "{}", "t": "acme", "e": exp }).to_string());
    let (p, mut rx) = wt::connect(wt::ticket_of(&format!("t={}", sealed)).unwrap(), None);
    wt::from_page(&p, "[\"j\",\"room\"]");
    // It is tried for five seconds or so.
    let deadline = Instant::now() + Duration::from_secs(15);
    let out = loop {
        if let Ok(o) = rx.try_recv() {
            break o;
        }
        assert!(Instant::now() < deadline, "no answer");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(matches!(out, Out::Text { json, reliable: true } if json == "[\"j\",\"room\",false]"));
    wt::disconnect(&p);
}
