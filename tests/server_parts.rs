//! The server's smaller parts, called as the server calls them: WebSocket
//! frames, live patches, multipart bodies, the file cache and the view cache.

use quillo::livecache::{apply, diff, Answer, LiveCache};
use quillo::server::HttpRequest;
use quillo::statics;
use quillo::upload;
use quillo::value::{Map, Value};
use quillo::viewcache::Found;
use quillo::ws::{Message, Reader, Socket};
use quillo::Interp;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

fn pair() -> (TcpStream, TcpStream) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    let (b, _) = l.accept().unwrap();
    a.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    b.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    (a, b)
}

fn upgrade(key: Option<&str>) -> HttpRequest {
    let mut headers = vec![("Upgrade".to_string(), "WebSocket".to_string())];
    if let Some(k) = key {
        headers.push(("Sec-WebSocket-Key".to_string(), format!(" {} ", k)));
    }
    HttpRequest { method: "GET".into(), url: "/_quillo/live".into(), headers, body: Vec::new(), remote: "127.0.0.1".into() }
}

/// A frame as a page sends it: masked.
fn frame(fin: bool, opcode: u8, data: &[u8]) -> Vec<u8> {
    let mask = [0x37, 0xfa, 0x21, 0x3d];
    let mut f = vec![if fin { 0x80 } else { 0 } | opcode];
    match data.len() {
        n if n < 126 => f.push(0x80 | n as u8),
        n if n <= 0xffff => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    f.extend_from_slice(&mask);
    f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// Reads a frame from the server (never masked): its first byte and payload.
fn server_frame(s: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).unwrap();
    let len = match head[1] {
        126 => {
            let mut b = [0u8; 2];
            s.read_exact(&mut b).unwrap();
            u16::from_be_bytes(b) as usize
        }
        127 => {
            let mut b = [0u8; 8];
            s.read_exact(&mut b).unwrap();
            u64::from_be_bytes(b) as usize
        }
        n => n as usize,
    };
    let mut data = vec![0; len];
    s.read_exact(&mut data).unwrap();
    (head[0], data)
}

#[test]
fn a_websocket_handshake_needs_a_key_and_says_what_it_was_given() {
    assert!(quillo::ws::is_upgrade(&upgrade(None)));
    let (_page, server) = pair();
    assert!(Socket::accept(&upgrade(None), server).is_none());
    let (mut page, server) = pair();
    let _socket = Socket::accept(&upgrade(Some("dGhlIHNhbXBsZSBub25jZQ==")), server).unwrap();
    let mut got = vec![0u8; 129];
    let n = page.read(&mut got).unwrap();
    let text = String::from_utf8_lossy(&got[..n]);
    assert!(text.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{}", text);
    assert!(text.contains("sec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"), "{}", text);
}

#[test]
fn a_socket_frames_messages_of_every_size() {
    let (mut page, server) = pair();
    let mut socket = Socket::accept(&upgrade(Some("k")), server).unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut b = [0u8; 1];
        page.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    let writer = std::thread::spawn(move || {
        assert!(socket.send("hi"));
        assert!(socket.send(&"m".repeat(300)));
        assert!(socket.send_binary(&vec![9u8; 70_000]));
        assert!(socket.pong(b"p"));
        assert!(socket.ping());
        socket.close();
    });
    assert_eq!(server_frame(&mut page), (0x81, b"hi".to_vec()));
    assert_eq!(server_frame(&mut page), (0x81, vec![b'm'; 300]));
    assert_eq!(server_frame(&mut page), (0x82, vec![9u8; 70_000]));
    assert_eq!(server_frame(&mut page), (0x8A, b"p".to_vec()));
    assert_eq!(server_frame(&mut page), (0x89, Vec::new()));
    assert_eq!(server_frame(&mut page), (0x88, Vec::new()));
    writer.join().unwrap();
}

#[test]
fn a_socket_tells_when_the_page_has_gone() {
    let (page, server) = pair();
    let mut socket = Socket::accept(&upgrade(Some("k")), server).unwrap();
    drop(page);
    // The first write may land before the reset comes back.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while socket.send("are you there") {
        assert!(std::time::Instant::now() < deadline, "the write never failed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_socket_reads_what_the_page_sends_on_the_same_connection() {
    let (mut page, server) = pair();
    let socket = Socket::accept(&upgrade(Some("k")), server).unwrap();
    let mut reader = socket.reader(1 << 20).unwrap();
    page.write_all(&frame(true, 0x1, b"from the page")).unwrap();
    assert_eq!(reader.next().unwrap(), Message::Text("from the page".into()));
}

#[test]
fn messages_can_come_in_parts_and_at_any_length() {
    let big = vec![7u8; 70_000];
    let mut wire = Vec::new();
    wire.extend(frame(true, 0x1, &[b'a'; 300]));
    wire.extend(frame(false, 0x2, &[1, 2]));
    wire.extend(frame(true, 0xA, b""));
    wire.extend(frame(false, 0x0, &[3]));
    wire.extend(frame(true, 0x0, &[4]));
    wire.extend(frame(true, 0x2, &big));
    wire.extend(frame(true, 0x9, b"p"));
    wire.extend(frame(true, 0x8, b""));
    let mut r = Reader::new(&wire[..], 1 << 20);
    assert_eq!(r.next().unwrap(), Message::Text("a".repeat(300)));
    assert_eq!(r.next().unwrap(), Message::Pong);
    assert_eq!(r.next().unwrap(), Message::Binary(vec![1, 2, 3, 4]));
    assert_eq!(r.next().unwrap(), Message::Binary(big));
    assert_eq!(r.next().unwrap(), Message::Ping(b"p".to_vec()));
    assert_eq!(r.next().unwrap(), Message::Close);
    let end = r.next().unwrap_err();
    assert_eq!(end.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[test]
fn frames_a_page_may_not_send_are_refused() {
    let why = |wire: &[u8], max: usize| Reader::new(wire, max).next().unwrap_err().to_string();
    assert_eq!(why(&[0xC1, 0x80], 100), "reserved bits set");
    assert_eq!(why(&[0x81, 0x01, b'x'], 100), "a page's frames are masked");
    assert_eq!(why(&frame(true, 0x1, &[b'x'; 200]), 100), "message too large");
    assert_eq!(why(&frame(true, 0x0, b"x"), 100), "a continuation of nothing");
    assert_eq!(why(&frame(true, 0x3, b"x"), 100), "unknown opcode");
    assert_eq!(why(&frame(true, 0x1, &[0xff, 0xfe]), 100), "text that is not UTF-8");
    let mut twice = frame(false, 0x1, b"a");
    twice.extend(frame(true, 0x1, b"b"));
    assert_eq!(why(&twice, 100), "a new message before the last one ended");
    // Parts together count toward the limit.
    let mut parts = frame(false, 0x1, &[b'x'; 60]);
    parts.extend(frame(true, 0x0, &[b'x'; 60]));
    assert_eq!(why(&parts, 100), "message too large");
    assert_eq!(quillo::ws::hex(&[0xab, 0x01]), "ab01");
}

fn round(a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    let p = diff(&a, &b);
    assert_eq!(apply(&a, &p), b, "patch {}", p);
    p
}

#[test]
fn patches_make_the_new_value_of_each_shape() {
    assert_eq!(round(json!({ "a": 1 }), json!({ "a": 1 })), json!({ "$=": 1 }));
    // New keys, keys gone, and a key changed.
    assert_eq!(round(json!({ "a": 1, "b": 2 }), json!({ "a": 1, "b": 3, "c": 4 })), json!({ "$m": { "b": { "$v": 3 }, "c": { "$v": 4 } } }));
    assert_eq!(round(json!({ "a": 1, "b": 2, "c": 3 }), json!({ "a": 1, "c": 3 })), json!({ "$m": {}, "$x": ["b"] }));
    // Keys in another order: the map goes whole, as the page keeps their order.
    assert_eq!(round(json!({ "a": 1, "b": 2 }), json!({ "b": 3, "a": 1 })), json!({ "$v": { "b": 3, "a": 1 } }));
    // A map for a list.
    assert_eq!(round(json!({ "a": 1, "b": 2 }), json!([1])), json!({ "$v": [1] }));
    // Single items, runs, changed rows and new ones.
    let old = json!([{ "id": 1, "t": "a" }, { "id": 2, "t": "b" }, "x", "y"]);
    let new = json!([{ "id": 2, "t": "b" }, { "id": 1, "t": "z" }, { "id": 3, "t": "c" }, "y", "x"]);
    let p = round(old, new);
    assert!(p["$l"].as_array().unwrap().iter().any(|op| op.is_number()), "{}", p);
    assert!(p["$l"].as_array().unwrap().iter().any(|op| op.get("$p").is_some()), "{}", p);
    assert!(p["$l"].as_array().unwrap().iter().any(|op| op.get("$v").is_some()), "{}", p);
    // Rows with an id that moved past those that stayed, and tagged values.
    let ids = |r: std::ops::Range<i32>| r.map(|i| json!({ "id": i })).collect::<Vec<_>>();
    assert!(round(json!(ids(0..4)), json!(ids(0..3))).to_string().contains("[0,3]"));
    round(json!({ "$dec": "1.5" }), json!({ "$dec": "2.5" }));
    // What `apply` does not recognize leaves the value as it was.
    assert_eq!(apply(&json!([1]), &json!({ "other": 1 })), json!([1]));
    assert_eq!(apply(&json!([1, 2]), &json!({ "$l": [{ "nothing": 1 }, 1] })), json!([2]));
}

#[test]
fn a_map_in_another_order_goes_whole() {
    let new = json!({ "b": 2, "a": 1 });
    assert_eq!(diff(&json!({ "a": 1, "b": 2 }), &new), json!({ "$v": { "b": 2, "a": 1 } }));
}

#[test]
fn a_run_is_shared_with_the_pages_it_may_be_shown_to() {
    let mut c = LiveCache::default();
    let key = |h: &str| ("loader".to_string(), b"[]".to_vec(), h.to_string());
    let answer = Arc::new(Answer::new(json!([1]), false));
    assert!(c.shared(&key("a"), 1, |_, _| true).is_none());
    c.share(key("a"), Some(7), 5, vec!["t".into()], answer.clone());
    // Only for that viewer, and while what it read has not changed.
    assert!(c.shared(&key("a"), 8, |_, _| true).is_none());
    assert!(c.shared(&key("a"), 7, |_, _| false).is_none());
    let (got, topics, seq) = c.shared(&key("a"), 7, |t, s| t == ["t".to_string()] && s == 5).unwrap();
    assert_eq!((got.text.as_str(), topics, seq), ("[1]", vec!["t".to_string()], 5));
    assert!(!got.err);
    // A run for all viewers.
    c.share(key("b"), None, 1, Vec::new(), answer.clone());
    assert!(c.shared(&key("b"), 123, |_, _| true).is_some());
    // Runs past the limit still count while they are new, and go in time.
    for i in 0..300 {
        c.share(key(&i.to_string()), None, 1, Vec::new(), answer.clone());
    }
    assert!(c.shared(&key("0"), 1, |_, _| true).is_some());
    std::thread::sleep(quillo::livecache::SHARED_FOR + Duration::from_millis(50));
    assert!(c.shared(&key("0"), 1, |_, _| true).is_none());
    c.share(key("new"), None, 1, Vec::new(), answer);
    assert!(c.shared(&key("299"), 1, |_, _| true).is_none());
    assert!(c.shared(&key("new"), 1, |_, _| true).is_some());
}

#[test]
fn values_are_kept_for_patches_until_their_room_is_needed() {
    let mut c = LiveCache::default();
    let v = |n: i32| Arc::new(json!({ "n": n, "pad": 0 }));
    let new = json!({ "n": 1000, "pad": 0 });
    // Not known: no patch, and the whole value goes.
    assert!(c.patch(1, 2, &new).is_none());
    assert_eq!(*c.patch(5, 5, &new).unwrap(), json!({ "$=": 1 }));
    c.keep(1, v(1), 10);
    let p = c.patch(1, 2, &new).unwrap();
    assert_eq!(*p, json!({ "$m": { "n": { "$v": 1000 } } }));
    // Once for all the pages that had it.
    assert!(Arc::ptr_eq(&p, &c.patch(1, 2, &new).unwrap()));
    // Keeping it again costs nothing.
    c.keep(1, v(1), 10);
    // Too large to keep.
    c.keep(2, v(2), 17 << 20);
    assert!(c.patch(2, 3, &new).is_none());
    // Past the room there is, the one unused longest goes, with its patches.
    for h in 10..14 {
        c.keep(h, v(h as i32), 16 << 20);
    }
    assert!(c.patch(10, 3, &new).is_some());
    c.keep(20, v(20), 16 << 20);
    c.keep(21, v(21), 16 << 20);
    assert!(c.patch(1, 3, &new).is_none());
    assert!(c.patch(11, 3, &new).is_none());
    assert!(c.patch(13, 3, &new).is_some());
    // Patches are forgotten together, past their limit.
    let first = c.patch(13, 3, &new).unwrap();
    for to in 100..400 {
        c.patch(13, to, &new);
    }
    assert!(!Arc::ptr_eq(&first, &c.patch(13, 3, &new).unwrap()));
    assert_eq!(quillo::livecache::parse_hash(&quillo::livecache::hash_text(255)), Some(255));
    assert_eq!(quillo::livecache::parse_hash("not hex"), None);
}

const BOUNDARY: &str = "XyZ";

fn part(name: &str, filename: Option<&str>, data: &str) -> String {
    let file = filename.map_or(String::new(), |f| format!("; filename=\"{}\"", f));
    format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"{}\r\n\r\n{}\r\n", BOUNDARY, name, file, data)
}

#[test]
fn a_multipart_type_names_its_boundary() {
    assert_eq!(upload::boundary("multipart/form-data; boundary=abc").as_deref(), Some("abc"));
    assert_eq!(upload::boundary("Multipart/Form-Data; charset=utf-8; BOUNDARY=\"a b\"").as_deref(), Some("a b"));
    assert_eq!(upload::boundary("text/plain; boundary=abc"), None);
    assert_eq!(upload::boundary("multipart/form-data"), None);
    assert_eq!(upload::boundary("multipart/form-data; charset=utf-8"), None);
    assert_eq!(upload::boundary("multipart/form-data; boundary="), None);
    assert_eq!(upload::boundary(&format!("multipart/form-data; boundary={}", "b".repeat(71))), None);
    assert_eq!(upload::boundary("multipart/form-data; odd"), None);
}

#[test]
fn a_field_sent_with_several_files_is_a_list_of_them() {
    let body = format!(
        "{}{}{}{}--{}--\r\n",
        part("photos", Some("c:\\pics\\a.png"), "one"),
        part("photos", Some("/tmp/b.png"), "two"),
        part("photos", Some("c.png"), "three"),
        part("title", None, "Trip"),
        BOUNDARY
    );
    let (form, files) = upload::parse(BOUNDARY, body.as_bytes());
    assert_eq!(form.get("title").map(|v| v.display()).as_deref(), Some("Trip"));
    let Some(Value::List(l)) = files.get("photos") else { panic!("not a list") };
    assert_eq!(l.len(), 3);
    let names: Vec<String> = l.iter().map(|f| if let Value::Map(m) = f { m.get("filename").unwrap().display() } else { String::new() }).collect();
    assert_eq!(names, ["a.png", "b.png", "c.png"]);
    assert_eq!(upload::bytes(&l[1], body.as_bytes()).unwrap(), b"two");
}

#[test]
fn parts_with_no_use_are_passed_over() {
    let body = format!(
        "{}--{}\r\nContent-Type: text/plain\r\n\r\nno name\r\n{}--{}--\r\n",
        part("empty", Some(""), ""),
        BOUNDARY,
        part("kept", None, "yes"),
        BOUNDARY
    );
    let (form, files) = upload::parse(BOUNDARY, body.as_bytes());
    assert_eq!(files.len(), 0);
    assert_eq!(form.len(), 1);
    assert_eq!(form.get("kept").map(|v| v.display()).as_deref(), Some("yes"));
    // A file's own type is kept; without one it is bytes.
    let body = format!("--{b}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a\"\r\nContent-Type: image/png\r\n\r\nx\r\n{}--{b}--", part("g", Some("b"), "y"), b = BOUNDARY);
    let (_, files) = upload::parse(BOUNDARY, body.as_bytes());
    let kind = |k: &str| match files.get(k) {
        Some(Value::Map(m)) => m.get("type").unwrap().display(),
        _ => String::new(),
    };
    assert_eq!((kind("f").as_str(), kind("g").as_str()), ("image/png", "application/octet-stream"));
}

#[test]
fn a_body_cut_short_gives_what_came_whole() {
    let empty = |body: &str| {
        let (form, files) = upload::parse(BOUNDARY, body.as_bytes());
        form.is_empty() && files.is_empty()
    };
    // No boundary at all, and bodies that end at each stage of a part.
    assert!(empty("nothing here"));
    assert!(empty("--XyZ"));
    assert!(empty("--XyZ\r\nContent-Disposition: form-data; name=\"a\""));
    assert!(empty("--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nvalue with no end"));
    assert!(empty("--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nvalue\r\nwith a break"));
    // A line break as far along as a boundary could start.
    assert!(empty("--XyZ\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nvalue\r\nabcde"));
    let (form, _) = upload::parse(BOUNDARY, format!("{}--XyZ", part("a", None, "1")).as_bytes());
    assert_eq!(form.len(), 1);
}

#[test]
fn an_upload_record_must_be_of_this_request() {
    let not_file = |v: Value| upload::bytes(&v, b"body").unwrap_err();
    assert_eq!(not_file(Value::Int(1)), "expects a file from request.files");
    assert_eq!(not_file(Value::map(Map::new())), "expects a file from request.files");
    let mut m = Map::new();
    m.insert("start".into(), Value::Int(1));
    m.insert("end".into(), Value::Int(100));
    assert_eq!(not_file(Value::map(m)), "this file is not part of the current request");
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-parts-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn only_files_are_files() {
    let d = temp("files");
    assert_eq!(statics::info(&d).err().map(|e| e.kind()), Some(std::io::ErrorKind::NotFound));
    let small = d.join("small.txt");
    std::fs::write(&small, "small").unwrap();
    let i = statics::info(&small).unwrap();
    assert_eq!(&*statics::read(&small, &i).unwrap(), b"small");
    // From memory: the file need not be there any more.
    std::fs::remove_file(&small).unwrap();
    assert_eq!(&*statics::read(&small, &i).unwrap(), b"small");
    // A large one is read each time.
    let large = d.join("large.bin");
    std::fs::write(&large, vec![1u8; (1 << 20) + 1]).unwrap();
    let i = statics::info(&large).unwrap();
    assert_eq!(statics::read(&large, &i).unwrap().len(), (1 << 20) + 1);
    std::fs::remove_file(&large).unwrap();
    assert!(statics::read(&large, &i).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn byte_ranges_are_read_as_asked() {
    assert_eq!(statics::range("bytes=-0", 100), Some(Err(())));
    assert_eq!(statics::range("bytes=-500", 100), Some(Ok((0, 99))));
    assert_eq!(statics::range("bytes=0-1", 0), None);
    assert_eq!(statics::range("bytes=a-b", 100), None);
    assert_eq!(statics::range("bytes=5", 100), None);
    assert_eq!(statics::range("bytes=9-5", 100), Some(Err(())));
}

#[test]
fn a_mark_names_a_file_only_if_this_process_made_it() {
    let m = statics::mark(std::path::Path::new("/tmp/a:b"), 5, 10);
    assert_eq!(statics::marked(&m), Some((std::path::PathBuf::from("/tmp/a:b"), 5, 10)));
    assert_eq!(statics::marked("nodot"), None);
    assert_eq!(statics::marked("!!!.!!!"), None);
    let (what, sig) = m.split_once('.').unwrap();
    let other = statics::mark(std::path::Path::new("/etc/passwd"), 0, 10);
    assert_eq!(statics::marked(&format!("{}.{}", what, other.split_once('.').unwrap().1)), None);
    assert_eq!(statics::marked(&format!("{}.{}", other.split_once('.').unwrap().0, sig)), None);
}

#[test]
fn gzipped_copies_are_kept_within_their_budget() {
    let chunk: Arc<[u8]> = Arc::from(vec![0u8; 1 << 20]);
    assert!(statics::gzipped("first", b"body").is_none());
    statics::keep_gzipped("first", b"body", chunk.clone());
    assert_eq!(statics::gzipped("first", b"body").unwrap().len(), 1 << 20);
    // Another body under the same tag is another entry.
    assert!(statics::gzipped("first", b"other").is_none());
    // Kept again: replaced.
    let less: Arc<[u8]> = Arc::from(vec![0u8; 5]);
    statics::keep_gzipped("first", b"body", less);
    assert_eq!(statics::gzipped("first", b"body").unwrap().len(), 5);
    // Too large to keep.
    let large: Arc<[u8]> = Arc::from(vec![0u8; (1 << 20) + 1]);
    statics::keep_gzipped("large", b"body", large);
    assert!(statics::gzipped("large", b"body").is_none());
    // The budget is 32 MiB: the oldest goes first.
    for i in 0..40 {
        statics::keep_gzipped(&format!("many{}", i), b"body", chunk.clone());
    }
    assert!(statics::gzipped("first", b"body").is_none());
    assert!(statics::gzipped("many0", b"body").is_none());
    assert!(statics::gzipped("many39", b"body").is_some());
}

#[test]
fn large_file_sends_have_limited_places() {
    // Each address has eight, an IPv4 address as itself in IPv6 too.
    let mine: Vec<_> = (0..8).map(|_| statics::SendPlace::take("192.0.2.1").unwrap()).collect();
    assert!(statics::SendPlace::take("192.0.2.1").is_none());
    assert!(statics::SendPlace::take("::ffff:192.0.2.1").is_none());
    // A whole /64 is one client.
    let six: Vec<_> = (1..=8).map(|i| statics::SendPlace::take(&format!("2001:db8::{:x}", i)).unwrap()).collect();
    assert!(statics::SendPlace::take("2001:db8::ff").is_none());
    // Text that is no address counts as itself.
    assert!(statics::SendPlace::take("not an address").is_some());
    drop(mine);
    assert!(statics::SendPlace::take("192.0.2.1").is_some());
    // 256 in all.
    let others: Vec<_> = (0..248).map(|i| statics::SendPlace::take(&format!("198.51.{}.{}", i / 8, i)).unwrap()).collect();
    assert!(statics::SendPlace::take("203.0.113.9").is_none());
    drop(six);
    drop(others);
    assert!(statics::SendPlace::take("203.0.113.9").is_some());
}

fn answer_head() -> quillo::server::HttpResponse {
    quillo::server::plain(200, "")
}

#[test]
fn a_file_gone_before_it_is_sent_is_not_found() {
    let (mut client, server) = pair();
    let gone = (std::path::PathBuf::from("/nonexistent/quillo/file"), 0, 10);
    statics::send(server, &answer_head(), gone, false);
    let mut out = String::new();
    client.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 404 Not Found"), "{}", out);
    assert!(out.ends_with("not found\n"), "{}", out);
}

#[test]
fn a_send_gives_the_part_asked_for_and_only_a_head_for_head() {
    let d = temp("send");
    let f = d.join("data.txt");
    std::fs::write(&f, "0123456789").unwrap();
    let (mut client, server) = pair();
    statics::send(server, &answer_head(), (f.clone(), 3, 4), false);
    let mut out = String::new();
    client.read_to_string(&mut out).unwrap();
    assert!(out.contains("content-length: 4\r\n") && out.ends_with("\r\n\r\n3456"), "{}", out);
    let (mut client, server) = pair();
    statics::send(server, &answer_head(), (f.clone(), 3, 4), true);
    let mut out = String::new();
    client.read_to_string(&mut out).unwrap();
    assert!(out.contains("content-length: 4\r\n") && out.ends_with("\r\n\r\n"), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_send_ends_when_the_reader_leaves() {
    let d = temp("send-gone");
    let f = d.join("large.bin");
    let len = 64u64 << 20;
    std::fs::File::create(&f).unwrap().set_len(len).unwrap();
    let (mut client, server) = pair();
    let path = f.clone();
    let send = std::thread::spawn(move || statics::send(server, &answer_head(), (path, 0, len), false));
    let mut some = [0u8; 4096];
    client.read_exact(&mut some).unwrap();
    drop(client);
    send.join().unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn files_served_are_noted_only_while_something_watches() {
    let f = std::path::Path::new("/tmp/noted.css");
    statics::served(f, "/assets/noted.css");
    assert!(!statics::served_files().iter().any(|(p, _)| p == f));
    statics::WATCH_SERVED.store(true, std::sync::atomic::Ordering::Relaxed);
    statics::served(f, "/assets/noted.css");
    statics::WATCH_SERVED.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(statics::served_files().contains(&(f.to_path_buf(), "/assets/noted.css".to_string())));
}

#[test]
fn counting_a_request_in_progress_ends_with_it() {
    let before = quillo::metrics::IN_PROGRESS.load(std::sync::atomic::Ordering::Relaxed);
    let busy = quillo::metrics::InProgress::default();
    assert_eq!(quillo::metrics::IN_PROGRESS.load(std::sync::atomic::Ordering::Relaxed), before + 1);
    drop(busy);
    assert_eq!(quillo::metrics::IN_PROGRESS.load(std::sync::atomic::Ordering::Relaxed), before);
}

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "views.qwe") {
        panic!("{}", e);
    }
    it
}

fn get(it: &mut Interp, url: &str) -> String {
    let r = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: Vec::new(), body: Vec::new(), remote: "127.0.0.1".into() });
    assert_eq!(r.status, 200, "{}: {}", url, r.text());
    r.text()
}

const VIEWS: &str = r#"
let list = [1]
let map = { a: 1 }
let html = <i>"x"</i>
let fail = err("no")
let answer = respond(200, "x")
let cents = dec("12.50")
fn helper() = 1

view Show(x = null) = <b>{str(type_of(x))}</b>

route GET /nothing = <Show/>
route GET /null = <Show x={null}/>
route GET /true = <Show x={true}/>
route GET /false = <Show x={false}/>
route GET /float = <Show x={1.5}/>
route GET /dec = <Show x={cents}/>
route GET /dec-same = <Show x={dec("12.50")}/>
route GET /dec-shown-otherwise = <Show x={dec("12.5")}/>
route GET /html = <Show x={html}/>
route GET /map = <Show x={map}/>
route GET /func = <Show x={helper}/>
route GET /builtin = <Show x={len}/>
route GET /err = <Show x={fail}/>
route GET /response = <Show x={answer}/>
route GET /int = <Show x={1}/>
route GET /str = <Show x={"1"}/>
route GET /list = <Show x={list}/>
"#;

/// Two renders of each kind of argument: the second is the same one.
#[test]
fn a_view_is_reused_for_the_same_argument_of_each_kind() {
    for url in ["/null", "/true", "/float", "/dec", "/html", "/map", "/func", "/builtin", "/err", "/response"] {
        let mut it = app(VIEWS);
        let first = get(&mut it, url);
        let (hits, misses) = it.view_cache_stats();
        assert_eq!((hits, misses), (0, 1), "{}", url);
        assert_eq!(get(&mut it, url), first, "{}", url);
        assert_eq!(it.view_cache_stats(), (1, 1), "{}", url);
    }
}

#[test]
fn a_view_is_not_reused_for_another_argument() {
    let mut it = app(VIEWS);
    for url in ["/nothing", "/null", "/true", "/false", "/int", "/str", "/float", "/list", "/map"] {
        get(&mut it, url);
    }
    assert_eq!(it.view_cache_stats().0, 0);
    // The same digits are the same; 12.5 shows otherwise than 12.50.
    get(&mut it, "/dec");
    get(&mut it, "/dec-same");
    assert_eq!(it.view_cache_stats().0, 1);
    get(&mut it, "/dec-shown-otherwise");
    assert_eq!(it.view_cache_stats().0, 1);
}

#[test]
fn a_view_is_reused_with_the_same_parameters_left_out() {
    let mut it = app("view V(a = 1, b = 2) = <b>{a + b}</b>\nroute GET / = <V b={5}/>\nroute GET /other = <V a={5}/>\n");
    assert_eq!(get(&mut it, "/"), "<b>6</b>");
    assert_eq!(get(&mut it, "/"), "<b>6</b>");
    assert_eq!(it.view_cache_stats().0, 1);
    assert_eq!(get(&mut it, "/other"), "<b>7</b>");
    assert_eq!(it.view_cache_stats().0, 1);
}

#[test]
fn a_view_keeps_its_latest_renders() {
    let mut it = app("view V(n: int) = <b>{n}</b>\nroute GET /:n(int) = <V n={n}/>\n");
    for n in 0..33 {
        get(&mut it, &format!("/{}", n));
    }
    get(&mut it, "/32");
    assert_eq!(it.view_cache_stats().0, 1);
    // The first one has made room.
    get(&mut it, "/0");
    assert_eq!(it.view_cache_stats().0, 1);
}

#[test]
fn a_view_found_impure_stores_nothing_more() {
    let it = app("view V() = <b>\"x\"</b>\n");
    let Some(Value::Func(f)) = quillo::value::lookup(&it.globals, "V") else { panic!("no view") };
    let cache = it.view_cache.clone().unwrap();
    let mut c = cache.lock().unwrap();
    c.store(&f.def, Vec::new(), "<b>x</b>", Vec::new(), 0, None);
    assert!(matches!(c.lookup(&f.def, &[], &it.shared, None), Found::Hit(_)));
    c.forbid(&f.def);
    c.store(&f.def, Vec::new(), "<b>x</b>", Vec::new(), 0, None);
    assert!(matches!(c.lookup(&f.def, &[], &it.shared, None), Found::Uncacheable));
    c.clear();
    assert!(matches!(c.lookup(&f.def, &[], &it.shared, None), Found::Miss));
}
