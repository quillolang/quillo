//! A peer that stops mid-answer: a service's process or the app, as `--split`
//! reaches them. The callers get the failure, not half an answer.

mod support;

use quillo::server::HttpRequest;
use quillo::service;
use quillo::Interp;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::Once;

const SECRET: &str = "a-test-secret-for-services-long-enough";

fn signing() {
    static SET: Once = Once::new();
    SET.call_once(|| std::env::set_var("QUILLO_SERVICE_SECRET", SECRET));
}

/// All of a request, so that hanging up later is no reset for what was left unread.
fn read_request(s: &mut TcpStream) {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&got[..end]).to_ascii_lowercase();
            let length: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
            if got.len() >= end + 4 + length {
                return;
            }
        }
        match s.read(&mut buf) {
            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => return,
        }
    }
}

/// A server on a port of its own, giving `answer` to each request, then hanging up.
fn peer(answer: &'static [u8]) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            read_request(&mut s);
            let _ = s.write_all(answer);
        }
    });
    url
}

const CUT: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nContent-Type: text/plain\r\n\r\nonly the start";

#[test]
fn a_signed_post_answered_in_part_is_a_failure() {
    signing();
    let url = peer(CUT);
    let e = service::signed_post(&url, "/_quillo/channel-relay", "", "_channel-relay", b"{}").unwrap_err();
    assert_eq!(e, "response body closed before all bytes were read");
}

#[test]
fn a_page_passed_on_and_answered_in_part_says_the_part_of_the_site_is_not_answering() {
    let d = support::temp_dir("cut-short-pass-on");
    let mut it = Interp::new(&d);
    let url = peer(CUT);
    let req = HttpRequest { method: "GET".into(), url: "/shop/page".into(), headers: vec![("Host".into(), "site.test".into())], body: Vec::new(), remote: "127.0.0.1".into() };
    let r = service::pass_on(&mut it, &url, "/shop", &req);
    assert_eq!(r.status, 502);
    let text = r.text();
    assert!(text.starts_with("this part of the site is not answering ("), "{}", text);
    assert!(!text.contains("only the start"), "{}", text);
}

#[test]
fn a_channel_call_the_app_answers_with_nonsense_counts_as_none_reached() {
    let d = support::temp_dir("cut-short-relay");
    let url = peer(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot json");
    let src = "print(channel.broadcast(\"room\", \"hi\"))\nprint(channel.peers(\"room\"))\nprint(channel.send(\"room\", \"abc\", \"hi\"))\n";
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "app.qwe"])
        .current_dir(&d)
        .env("QUILLO_APP_URL", &url)
        .env("QUILLO_SERVICE_SECRET", SECRET)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n[]\nfalse\n");
}
