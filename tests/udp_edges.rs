//! UDP at its edges (src/udp.rs): what `udp.listen` and `udp.send` refuse, a
//! batch that makes no sense, and the delivery thread with a server that is
//! not there, drops the connection or says to close it.

mod support;

use quillo::Interp;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-udp-edges-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A UDP port below those the system hands out, so that no other socket of
/// these tests can be given it.
fn free_udp_port() -> u16 {
    loop {
        let p = support::free_port();
        if UdpSocket::bind(("0.0.0.0", p)).is_ok() {
            return p;
        }
    }
}

fn fails(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

fn program(name: &str, src: &str) -> Interp {
    let d = dir(name);
    let mut it = Interp::new(&d);
    it.load_source(src, "main.qwe").unwrap();
    it
}

fn text(r: &quillo::server::HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(file: &Path, port: u16) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let server = Server(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// A request off a connection: its head and body; `None` if it was closed.
fn read_request(s: &mut TcpStream) -> Option<(String, Vec<u8>)> {
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
    let len: usize = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
        .unwrap_or(0);
    while buf.len() < end + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    Some((head, buf[end..end + len].to_vec()))
}

#[test]
fn udp_listen_says_what_it_takes() {
    let f = "fn on(p) = null\n";
    assert!(fails(&format!("{}udp.listen(0, on)", f)).contains("a port from 1 to 65535 first, got 0"));
    assert!(fails("udp.listen()").contains("first, got nothing"));
    assert!(fails("udp.listen(9, 1)").contains("needs a function to call with each packet, got int"));
    assert!(fails("udp.listen(9)").contains("got nothing"));
    assert!(fails(&format!("{}udp.listen(9, on, {{ host: 1 }})", f)).contains("`host` is an address, as a str"));
    assert!(fails(&format!("{}udp.listen(9, on, {{ queue: 0 }})", f)).contains("`queue` is a number of packets above 0"));
    assert!(fails(&format!("{}udp.listen(9, on, {{ size: 1 }})", f)).contains("no option `size` (it takes host and queue)"));
    assert!(fails(&format!("{}udp.listen(9, on, 5)", f)).contains("expects options as a map, got int"));
    let p = free_udp_port();
    assert!(fails(&format!("{f}udp.listen({p}, on, null)\nudp.listen({p}, on)")).contains(&format!("already listens on UDP port {}", p)));
}

#[test]
fn udp_send_says_what_it_takes() {
    let bad = free_udp_port();
    assert!(fails("udp.send(1, 2, \"x\")").contains("takes a host first, got int"));
    assert!(fails("udp.send()").contains("takes a host first, got nothing"));
    assert!(fails("udp.send(\"127.0.0.1\", 0, \"x\")").contains("a port from 1 to 65535 second, got 0"));
    assert!(fails("udp.send(\"127.0.0.1\")").contains("second, got nothing"));
    assert!(fails("udp.send(\"127.0.0.1\", 9)").contains("`udp.send` has nothing to send"));
    assert!(fails("udp.send(\"127.0.0.1\", 9, \"x\".repeat(65508))").contains("65508 bytes is more than a UDP packet holds (65507)"));
    assert!(fails("udp.send(\"127.0.0.1\", 9, \"x\", { from: 0 })").contains("`from` is a port the program listens on, got 0"));
    assert!(fails("udp.send(\"127.0.0.1\", 9, \"x\", 5)").contains("expects options as a map, got int"));
    assert!(fails(&format!("udp.send(\"127.0.0.1\", 9, \"x\", {{ from: {} }})", bad)).contains(&format!("the program does not listen on UDP port {}", bad)));
    assert!(fails("udp.reply(1, \"x\")").contains("takes a packet that `udp.listen` gave"));
}

#[test]
fn udp_send_gives_an_error_value_when_the_packet_cannot_go() {
    let to = UdpSocket::bind("127.0.0.1:0").unwrap();
    to.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let port = to.local_addr().unwrap().port();
    // Options that say nothing send as none would.
    let v = quillo::eval_str(&format!("udp.send(\"localhost\", {}, \"plain\", {{}})\nudp.send(\"127.0.0.1\", {}, \"null\", null)", port, port)).unwrap();
    assert_eq!(v.repr(), "null");
    let mut buf = [0u8; 16];
    let n = to.recv(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"plain");
    let n = to.recv(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"null");
    // From a socket on IPv4, to an IPv6 address.
    // Another test can take a free port before it is listened on: ask for another.
    let out = (0..20)
        .find_map(|_| {
            let src = format!("fn on(p) = null\nudp.listen({m}, on, {{ host: \"127.0.0.1\" }})\nudp.send(\"::1\", 9, \"x\", {{ from: {m} }})", m = free_udp_port());
            quillo::eval_str(&src).ok()
        })
        .expect("no free UDP port in 20 tries");
    assert!(out.repr().contains("cannot send to [::1]:9"), "{}", out.repr());
}

#[test]
fn a_batch_for_a_port_no_one_listens_on_or_that_makes_no_sense_is_refused() {
    let d = dir("batches");
    let port = free_udp_port();
    let other = free_udp_port();
    std::fs::write(d.join("sub.qwe"), format!("fn on(p) = null\nudp.listen({}, on)\nroute GET /x = \"x\"\n", other)).unwrap();
    let src = format!(
        "let got = []\nfn on(p) {{\n got = got + [`${{p.from.host}}:${{p.from.port}} ${{p.text}} ${{p.at}}`]\n}}\nudp.listen({})\nmount /sub = \"sub.qwe\"\nroute GET /got = got\n",
        port
    );
    std::fs::write(d.join("main.qwe"), src.replace(&format!("udp.listen({})", port), &format!("udp.listen({}, on)", port))).unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();

    let a = quillo::udp::answer(&mut it, "not-a-port", b"");
    assert_eq!((a.status, text(&a).as_str()), (404, "not listening on that port\n"));
    // Nor is it a port of a program it mounts.
    let a = quillo::udp::answer(&mut it, "7", b"");
    assert_eq!((a.status, text(&a).as_str()), (404, "not listening on that port\n"));
    // Cut short, an address of no known length, data shorter than it says.
    let bad: &[&[u8]] = &[&[4, 127, 0], &[5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &[4, 127, 0, 0, 1, 0, 80, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 9, b'x']];
    for b in bad {
        let a = quillo::udp::answer(&mut it, &port.to_string(), b);
        assert_eq!((a.status, text(&a).as_str()), (400, "bad batch\n"), "{:?}", b);
    }
    // A sender on IPv6 is shown as such.
    let mut v6 = vec![16];
    v6.extend_from_slice(&"::1".parse::<std::net::Ipv6Addr>().unwrap().octets());
    v6.extend_from_slice(&1234u16.to_be_bytes());
    v6.extend_from_slice(&7u64.to_be_bytes());
    v6.extend_from_slice(&2u32.to_be_bytes());
    v6.extend_from_slice(b"hi");
    assert_eq!(quillo::udp::answer(&mut it, &port.to_string(), &v6).status, 200);
    assert_eq!(quillo::udp::ports(&it).len(), 2);
    let v = it.load_source("got", "after.qwe").unwrap();
    assert_eq!(v.repr(), "[\"::1:1234 hi 7\"]");
}

#[test]
fn a_packet_from_ipv6_reaches_the_handler_with_its_address() {
    if UdpSocket::bind("[::1]:0").is_err() {
        return;
    }
    let d = dir("v6");
    let udp = free_udp_port();
    std::fs::write(
        d.join("main.qwe"),
        format!("let got = \"\"\nfn on(p) {{\n got = p.from.host + \" \" + p.text\n}}\nudp.listen({}, on, {{ host: \"::1\" }})\nroute GET /got = got\n", udp),
    )
    .unwrap();
    let port = support::free_port();
    let _s = start(&d.join("main.qwe"), port);
    let c = UdpSocket::bind("[::1]:0").unwrap();
    let got = eventually(10, "the packet did not arrive", || {
        c.send_to(b"hello", ("::1", udp)).unwrap();
        let a = support::send(port, "GET", "/got", &[], "");
        (a.body.contains("hello")).then_some(a.body)
    });
    assert!(got.contains("::1 hello"), "{}", got);
}

#[test]
fn the_loopback_connection_says_what_is_wrong_with_an_answer() {
    /// A server that answers `reply` to one request, its last 3 bytes after
    /// a moment, then closes.
    fn answers(reply: &'static [u8]) -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            read_request(&mut s);
            let (a, b) = reply.split_at(reply.len().saturating_sub(3));
            s.write_all(a).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            s.write_all(b).unwrap();
        });
        addr
    }
    let post = |reply: &'static [u8]| quillo::udp::Loopback::connect(answers(reply)).unwrap().post_for("/x", b"body");
    let kind = |r: std::io::Result<_>| r.map(|_: (quillo::udp::Loopback, u16, Vec<u8>)| ()).unwrap_err().kind();
    assert_eq!(kind(post(b"")), std::io::ErrorKind::UnexpectedEof);
    assert_eq!(kind(post(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort")), std::io::ErrorKind::UnexpectedEof);
    assert_eq!(kind(post(b"HTTP/1.1 \xff\xfe\r\n\r\n")), std::io::ErrorKind::InvalidData);
    assert_eq!(kind(post(b"nonsense\r\n\r\n")), std::io::ErrorKind::InvalidData);
    // No length: an empty answer, and the connection stays.
    let (_, status, body) = post(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
    assert_eq!((status, body.as_slice()), (204, &b""[..]));
}

/// The ports the delivery thread is told the program listens on.
static WANTED: Mutex<Vec<u16>> = Mutex::new(Vec::new());

#[test]
fn delivery_waits_for_a_server_and_goes_past_one_that_fails() {
    let udp = free_udp_port();
    let port = support::free_port();
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    WANTED.lock().unwrap().push(udp);
    let _it = program("delivery", &format!("fn on(p) = null\nudp.listen({}, on, {{ host: \"127.0.0.1\", queue: 100 }})\n", udp));
    quillo::udp::start(addr, || WANTED.lock().unwrap().clone());
    // Started once: the ports of a second start are never asked for.
    quillo::udp::start(addr, || panic!("a second delivery thread"));

    let c = UdpSocket::bind("127.0.0.1:0").unwrap();
    c.send_to(b"one", ("127.0.0.1", udp)).unwrap();
    // Nothing listens yet: the delivery thread is tried again and again.
    std::thread::sleep(Duration::from_millis(700));
    let l = TcpListener::bind(addr).unwrap();

    // The first connection is dropped without an answer: sent again.
    let (mut s, _) = l.accept().unwrap();
    let (head, first) = read_request(&mut s).unwrap();
    assert!(head.starts_with(&format!("POST /_quillo/udp/{} HTTP/1.1\r\n", udp)), "{}", head);
    assert!(head.contains(&format!("X-Quillo-Job: {}\r\n", quillo::jobs::token())), "{}", head);
    assert!(first.ends_with(b"one"), "{:?}", first);
    drop(s);
    // A handler that failed is said so, and the connection stays.
    let (mut s, _) = l.accept().unwrap();
    let (_, again) = read_request(&mut s).unwrap();
    assert_eq!(again, first);
    s.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n").unwrap();
    c.send_to(b"two", ("127.0.0.1", udp)).unwrap();
    let (_, second) = read_request(&mut s).unwrap();
    assert!(second.ends_with(b"two"), "{:?}", second);
    // Answered, but to close: the next batch comes on a new connection.
    s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok\n").unwrap();
    let (mut s2, _) = l.accept().unwrap();
    drop(s);
    c.send_to(b"three", ("127.0.0.1", udp)).unwrap();
    let (_, third) = read_request(&mut s2).unwrap();
    assert!(third.ends_with(b"three"), "{:?}", third);
    s2.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap();

    // A server that keeps dropping a batch: after 51 tries it is given up on.
    c.send_to(b"four", ("127.0.0.1", udp)).unwrap();
    let (_, fourth) = read_request(&mut s2).unwrap();
    assert!(fourth.ends_with(b"four"), "{:?}", fourth);
    drop(s2);
    for _ in 0..50 {
        let (mut s, _) = l.accept().unwrap();
        assert_eq!(read_request(&mut s).unwrap().1, fourth);
    }
    // A server that is gone: given up on too, after five seconds or so.
    drop(l);
    c.send_to(b"five", ("127.0.0.1", udp)).unwrap();
    std::thread::sleep(Duration::from_millis(5600));
    let l = TcpListener::bind(addr).unwrap();
    c.send_to(b"six", ("127.0.0.1", udp)).unwrap();
    let (mut s, _) = l.accept().unwrap();
    let (_, sixth) = read_request(&mut s).unwrap();
    assert!(sixth.ends_with(b"six") && !sixth.windows(4).any(|w| w == b"five"), "{:?}", sixth);
    s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap();
    WANTED.lock().unwrap().clear();
}
