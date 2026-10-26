//! UDP: packets to a program's ports, and from it (src/udp.rs).

use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-udp-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start(file: &Path, port: u16, extra: &[&str]) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start quillo");
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    Server(child)
}

fn get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", path).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()
}

/// Waits until `f` gives Some, or fails after `secs`.
fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn client() -> UdpSocket {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

fn recv(s: &UdpSocket) -> (String, u16) {
    let mut buf = [0u8; 2048];
    let (n, from) = s.recv_from(&mut buf).expect("no packet came back");
    (String::from_utf8_lossy(&buf[..n]).into_owned(), from.port())
}

#[test]
fn packets_reach_the_handler_in_order_and_are_answered_from_its_port() {
    let d = dir("order");
    let up = free_udp_port();
    std::fs::write(
        d.join("main.qwe"),
        format!(
            "let got = []\n\
             udp.listen({up}, (p) => {{\n\
             \x20   got = got + [{{ text: p.text, bytes: p.bytes, host: p.from.host, port: p.port }}]\n\
             \x20   udp.reply(p, `ack ${{p.text ?? \"binary\"}}`)\n\
             }})\n\
             route GET /got = got\n\
             route GET /count = str(len(got))\n"
        ),
    )
    .unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[]);
    let c = client();
    for i in 0..200 {
        c.send_to(format!("n{}", i).as_bytes(), ("127.0.0.1", up)).unwrap();
    }
    for i in 0..200 {
        assert_eq!(recv(&c), (format!("ack n{}", i), up));
    }
    // Bytes that are not text.
    c.send_to(&[0, 255, 7], ("127.0.0.1", up)).unwrap();
    assert_eq!(recv(&c).0, "ack binary");
    let got: serde_json::Value = serde_json::from_str(&get(port, "/got")).unwrap();
    let got = got.as_array().unwrap();
    assert_eq!(got.len(), 201);
    let texts: Vec<String> = got[..200].iter().map(|g| g["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(texts, (0..200).map(|i| format!("n{}", i)).collect::<Vec<_>>());
    assert_eq!(got[0]["bytes"], serde_json::json!([110, 48]));
    assert_eq!((got[0]["host"].as_str(), got[0]["port"].as_u64()), (Some("127.0.0.1"), Some(up as u64)));
    assert_eq!((&got[200]["text"], &got[200]["bytes"]), (&serde_json::Value::Null, &serde_json::json!([0, 255, 7])));
}

#[test]
fn a_program_sends_text_bytes_and_values() {
    let d = dir("send");
    let (up, to) = (free_udp_port(), client());
    let at = to.local_addr().unwrap().port();
    std::fs::write(
        d.join("main.qwe"),
        format!(
            "udp.listen({up}, () => null)\n\
             route GET /text = udp.send(\"127.0.0.1\", {at}, \"plain\")\n\
             route GET /bytes = udp.send(\"localhost\", {at}, [104, 105])\n\
             route GET /value = udp.send(\"127.0.0.1\", {at}, {{ t: 21.5, ok: true }})\n\
             route GET /from = udp.send(\"127.0.0.1\", {at}, \"mine\", {{ from: {up} }})\n\
             route GET /nowhere = str(udp.send(\"no-such-host.invalid\", 9, \"x\").error)\n"
        ),
    )
    .unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[]);
    get(port, "/text");
    assert_eq!(recv(&to).0, "plain");
    get(port, "/bytes");
    assert_eq!(recv(&to).0, "hi");
    get(port, "/value");
    assert_eq!(recv(&to).0, r#"{"t":21.5,"ok":true}"#);
    get(port, "/from");
    assert_eq!(recv(&to), ("mine".to_string(), up));
    assert!(get(port, "/nowhere").contains("cannot find the address"));
}

#[test]
fn a_handler_that_fails_does_not_stop_the_packets_after_it() {
    let d = dir("fails");
    let up = free_udp_port();
    std::fs::write(
        d.join("main.qwe"),
        format!(
            "let got = []\n\
             udp.listen({up}, (p) => {{\n\
             \x20   if p.text == \"bad\" {{ return 1 / (len(p.bytes) - 3) }}\n\
             \x20   got = got + [p.text]\n\
             }})\n\
             route GET /got = got | join(\",\")\n"
        ),
    )
    .unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[]);
    let c = client();
    for t in ["a", "bad", "b"] {
        c.send_to(t.as_bytes(), ("127.0.0.1", up)).unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    eventually(5, "the packet after the failing one was lost", || (get(port, "/got") == "a,b").then_some(()));
}

#[test]
fn a_full_queue_drops_its_oldest_packets() {
    let d = dir("full");
    let up = free_udp_port();
    std::fs::write(
        d.join("main.qwe"),
        format!(
            "let got = []\n\
             udp.listen({up}, (p) => {{\n\
             \x20   sleep(100)\n\
             \x20   got = got + [int(p.text)]\n\
             }}, {{ queue: 5 }})\n\
             route GET /got = got\n"
        ),
    )
    .unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &["--metrics"]);
    let c = client();
    for i in 0..60 {
        c.send_to(i.to_string().as_bytes(), ("127.0.0.1", up)).unwrap();
    }
    // The last packet always arrives; most of the ones between were dropped.
    let got: Vec<i64> = eventually(20, "the last packet never arrived", || {
        let got: Vec<i64> = serde_json::from_str(&get(port, "/got")).unwrap();
        got.last().filter(|&&n| n == 59).map(|_| got.clone())
    });
    assert!(got.len() < 30, "{:?}", got);
    assert!(got.windows(2).all(|w| w[0] < w[1]), "out of order: {:?}", got);
    let metrics = get(port, "/_quillo/metrics");
    let dropped = metrics.lines().find_map(|l| l.strip_prefix("quillo_udp_dropped_total ")).unwrap();
    assert!(dropped.trim().parse::<u64>().unwrap() >= 30, "{}", metrics);
    assert!(metrics.contains("quillo_udp_received_total 60"), "{}", metrics);
}

#[test]
fn a_reloaded_program_keeps_its_ports_and_adds_and_closes_others() {
    let d = dir("reload");
    let (a, b) = (free_udp_port(), free_udp_port());
    let program = |v: &str, ports: &[u16]| {
        let mut s = format!("let V = \"{}\"\nroute GET /v = V\n", v);
        for p in ports {
            s.push_str(&format!("udp.listen({p}, (p) => {{ udp.reply(p, `${{V}} on {p}`) }})\n"));
        }
        s
    };
    std::fs::write(d.join("main.qwe"), program("one", &[a])).unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &["--reload"]);
    let c = client();
    c.send_to(b"x", ("127.0.0.1", a)).unwrap();
    assert_eq!(recv(&c).0, format!("one on {}", a));

    std::fs::write(d.join("main.qwe"), program("two", &[a, b])).unwrap();
    eventually(10, "the program was not loaded again", || (get(port, "/v") == "two").then_some(()));
    // The same socket, now with the new version's handler; and a new one.
    c.send_to(b"x", ("127.0.0.1", a)).unwrap();
    assert_eq!(recv(&c).0, format!("two on {}", a));
    c.send_to(b"x", ("127.0.0.1", b)).unwrap();
    assert_eq!(recv(&c).0, format!("two on {}", b));

    std::fs::write(d.join("main.qwe"), program("three", &[b])).unwrap();
    eventually(10, "the program was not loaded again", || (get(port, "/v") == "three").then_some(()));
    // A port no longer listened on is let go of.
    eventually(10, "the port the program left was never closed", || UdpSocket::bind(("0.0.0.0", a)).ok());
    c.send_to(b"x", ("127.0.0.1", b)).unwrap();
    assert_eq!(recv(&c).0, format!("three on {}", b));
}

#[test]
fn a_mounted_program_gets_its_own_packets() {
    let d = dir("mounted");
    let up = free_udp_port();
    std::fs::write(d.join("sensors.qwe"), format!("udp.listen({up}, (p) => {{ udp.reply(p, `sensors got ${{p.text}}`) }})\nroute GET / = \"sensors\"\n")).unwrap();
    std::fs::write(d.join("main.qwe"), "mount /sensors = \"sensors.qwe\"\nroute GET / = \"main\"\n").unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[]);
    let c = client();
    c.send_to(b"t=21", ("127.0.0.1", up)).unwrap();
    assert_eq!(recv(&c).0, "sensors got t=21");
}

#[test]
fn a_port_already_taken_is_an_error_when_the_program_starts() {
    let d = dir("taken");
    let held = UdpSocket::bind("0.0.0.0:0").unwrap();
    let up = held.local_addr().unwrap().port();
    std::fs::write(d.join("main.qwe"), format!("udp.listen({up}, (p) => null)\nroute GET / = \"hi\"\n")).unwrap();
    let mut it = quillo::Interp::new(&d);
    let e = it.load_file(&d.join("main.qwe")).err().expect("it should fail");
    assert!(e.to_string().contains(&format!("cannot listen on UDP port {}", up)), "{}", e);
}

#[test]
fn many_packets_go_through_in_batches() {
    let d = dir("many");
    let up = free_udp_port();
    std::fs::write(
        d.join("main.qwe"),
        format!("let n = 0\nlet sum = 0\nudp.listen({up}, (p) => {{\n    n += 1\n    sum += len(p.bytes)\n}})\nroute GET /n = str(n)\n"),
    )
    .unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[]);
    let c = client();
    let total = 100_000;
    let payload = [7u8; 64];
    let t = Instant::now();
    for i in 0..total {
        c.send_to(&payload, ("127.0.0.1", up)).unwrap();
        // A little room now and then, as a real sender's packets have.
        if i % 2000 == 1999 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let got: u64 = eventually(30, "the packets never all arrived", || {
        let n: u64 = get(port, "/n").parse().unwrap();
        let settled = n == total || t.elapsed() > Duration::from_secs(10);
        settled.then_some(n)
    });
    let secs = t.elapsed().as_secs_f64();
    eprintln!("udp: {} of {} packets handled in {:.2}s ({:.0}/s)", got, total, secs, got as f64 / secs);
    assert!(got as f64 >= total as f64 * 0.95, "only {} of {} arrived", got, total);
}

#[test]
fn the_checker_knows_udp() {
    use quillo::check::check_source;
    let diags = |src: &str| -> Vec<String> {
        check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(|d| d.message.clone()).collect()
    };
    assert!(diags("udp.listen(9000, (p) => udp.reply(p, p.text))\nfn f() with net = udp.send(\"h\", 1, \"x\")").is_empty());
    let d = diags("fn f() with db = udp.send(\"h\", 1, \"x\")");
    assert!(d.len() == 1 && d[0].contains("`udp.send` needs the net effect"), "{:?}", d);
    let d = diags("udp.listen(\"9000\", (p) => null)");
    assert!(d.len() == 1 && d[0].contains("int"), "{:?}", d);
    let d = diags("udp.nope()");
    assert!(d.len() == 1 && d[0].contains("`udp` has no function `nope`"), "{:?}", d);
}
