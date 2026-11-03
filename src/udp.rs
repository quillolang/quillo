//! UDP: packets in and out of the server.
//!
//! ```text
//! udp.listen(9000, (p) => { readings = readings + [json.parse(p.text)] })
//! udp.reply(p, "ok")                       // from the port it came to
//! udp.send("10.0.0.5", 514, "<14>started")
//! ```
//!
//! `udp.listen` binds its port when the program runs, so a port taken is an
//! error at once. A thread per socket receives packets into a queue; another
//! hands them to the program the way scheduled jobs are run: it asks the
//! server (`POST /_quillo/udp/<port>`, with this process's job secret) to
//! run the handler for a batch of them, one batch at a time. So the handler
//! runs like a request, on the version of the program running now, waiting
//! for the database without holding anyone up; packets reach it in the
//! order they came; and a handler that falls behind gets bigger batches,
//! not more requests. A queue that is full drops its oldest packet.
//!
//! A socket stays bound across reloads while the program still listens on
//! its port, and is closed once a reloaded program no longer does.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

type R = Result<Value, Ctrl>;

/// The largest payload one UDP packet carries (over IPv4).
const MAX_PACKET: usize = 65_507;
/// Most packets, and bytes, handed to the program at once.
const BATCH_PACKETS: usize = 4096;
const BATCH_BYTES: usize = 1 << 20;
/// Most bytes a port's queue holds, whatever `queue` says of packets: 60 KB packets filled
/// 655 MB of one (FEEDBACK49.md, Q-604; branch main).
const QUEUE_BYTES: usize = 64 << 20;
/// How long a socket the running program no longer listens on is kept, in
/// case a program being loaded (which binds before it runs) wants it.
const GRACE: Duration = Duration::from_secs(3);

/// Packets dropped because a queue was full, for `--metrics`.
pub static DROPPED: AtomicU64 = AtomicU64::new(0);
/// Packets received, for `--metrics`.
pub static RECEIVED: AtomicU64 = AtomicU64::new(0);

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[("udp.listen", listen), ("udp.send", send), ("udp.reply", reply)];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("udp.").into(), Value::Builtin(name, *f));
    }
}

/// What `udp.listen` registered: the program's handler for a port.
pub struct Listener {
    pub port: u16,
    pub f: Value,
}

struct Packet {
    data: Vec<u8>,
    from: SocketAddr,
    at: u64,
}

/// A bound socket, and the packets it has received that the program has
/// not had yet.
/// What a port has received and the program has not had yet: the packets, their bytes, and
/// the bytes from each sender.
#[derive(Default)]
struct Queue {
    packets: VecDeque<Packet>,
    bytes: usize,
    from: HashMap<std::net::IpAddr, usize>,
}

impl Queue {
    fn push(&mut self, p: Packet) {
        self.bytes += p.data.len();
        *self.from.entry(p.from.ip()).or_default() += p.data.len();
        self.packets.push_back(p);
    }

    fn taken(&mut self, p: &Packet) {
        self.bytes -= p.data.len();
        if let Some(n) = self.from.get_mut(&p.from.ip()) {
            *n -= p.data.len();
            if *n == 0 {
                self.from.remove(&p.from.ip());
            }
        }
    }

    fn pop_front(&mut self) -> Option<Packet> {
        let p = self.packets.pop_front()?;
        self.taken(&p);
        Some(p)
    }

    /// A packet in, within `cap` packets and `bytes`: how many were dropped for it. A sender
    /// holding more than half of a full queue that others wait in has its new packet dropped,
    /// where dropping the oldest let one sender push out the rest (FEEDBACK49.md, Q-604;
    /// branch main). Alone, its oldest go, as before.
    fn admit(&mut self, p: Packet, cap: usize, bytes: usize) -> u64 {
        let n = p.data.len();
        let full = |q: &Queue| q.packets.len() >= cap || q.bytes + n > bytes;
        let mut dropped = 0;
        if full(self) {
            let mine = self.from.get(&p.from.ip()).copied().unwrap_or(0);
            if mine * 2 > self.bytes && self.from.len() > 1 {
                return 1;
            }
            while full(self) && self.pop_front().is_some() {
                dropped += 1;
            }
        }
        self.push(p);
        dropped
    }

    /// The first `n` packets, out of the queue.
    fn take(&mut self, n: usize) -> Vec<Packet> {
        let out: Vec<Packet> = self.packets.drain(..n).collect();
        for p in &out {
            self.taken(p);
        }
        out
    }
}

struct Bound {
    socket: UdpSocket,
    queue: Mutex<Queue>,
    arrived: Condvar,
    cap: usize,
    closed: AtomicBool,
    /// Whether its delivery has started (once the server listens).
    delivering: AtomicBool,
    /// When the running program was last seen listening on it.
    wanted: Mutex<Instant>,
}

fn sockets() -> &'static Mutex<HashMap<u16, Arc<Bound>>> {
    static S: OnceLock<Mutex<HashMap<u16, Arc<Bound>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `udp.listen(port, f, { host, queue })`, at the top of a program.
fn listen(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let port = match args.first() {
        Some(Value::Int(n)) if (1..=65535).contains(n) => *n as u16,
        other => return fail(line, format!("`udp.listen` takes a port from 1 to 65535 first, got {}", other.map_or("nothing".into(), |v| v.repr()))),
    };
    let f = match args.get(1) {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => return fail(line, format!("`udp.listen` needs a function to call with each packet, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let (mut host, mut cap) = ("0.0.0.0".to_string(), 10_000usize);
    match args.get(2) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(o)) => {
            for (k, v) in o.iter() {
                match (k.as_str(), v) {
                    ("host", Value::Str(h)) => host = h.to_string(),
                    ("queue", Value::Int(n)) if *n > 0 => cap = *n as usize,
                    ("host", _) | ("queue", _) => return fail(line, format!("`udp.listen`: `{}` is {}", k.as_str(), if k.as_str() == "host" { "an address, as a str" } else { "a number of packets above 0" })),
                    _ => return fail(line, format!("`udp.listen` has no option `{}` (it takes host and queue)", k.as_str())),
                }
            }
        }
        Some(v) => return fail(line, format!("`udp.listen` expects options as a map, got {}", v.type_name())),
    }
    let site = Arc::make_mut(&mut it.site);
    if site.udp.iter().any(|l| l.port == port) {
        return fail(line, format!("`udp.listen`: the program already listens on UDP port {}", port));
    }
    if let Err(e) = bind(&host, port, cap) {
        return fail(line, format!("`udp.listen`: cannot listen on UDP port {}: {}", port, e));
    }
    site.udp.push(Arc::new(Listener { port, f }));
    Ok(Value::Nil)
}

/// The socket for `port`, bound now unless it already is (by this program
/// before a reload, or by another instance of it).
fn bind(host: &str, port: u16, cap: usize) -> std::io::Result<()> {
    let mut all = lock(sockets());
    if let Some(b) = all.get(&port) {
        *lock(&b.wanted) = Instant::now();
        return Ok(());
    }
    let socket = UdpSocket::bind((host, port))?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;
    roomy(&socket);
    let b = Arc::new(Bound {
        socket,
        queue: Mutex::new(Queue::default()),
        arrived: Condvar::new(),
        cap,
        closed: AtomicBool::new(false),
        delivering: AtomicBool::new(false),
        wanted: Mutex::new(Instant::now()),
    });
    let r = b.clone();
    std::thread::Builder::new().name(format!("quillo-udp-{}", port)).spawn(move || receive(r))?;
    all.insert(port, b);
    Ok(())
}

/// Asks for a larger receive buffer than the system's default (a few
/// hundred KB), so a burst waits in the kernel instead of being lost
/// before the receiving thread takes it.
fn roomy(socket: &UdpSocket) {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let size: libc::c_int = 8 << 20;
        // The system may give less; any is better than the default.
        for want in [size, size / 2, size / 4] {
            let ok = unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    &want as *const libc::c_int as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                )
            };
            if ok == 0 {
                break;
            }
        }
    }
}

/// Receives packets into the socket's queue until it is closed.
fn receive(b: Arc<Bound>) {
    let mut buf = vec![0u8; MAX_PACKET + 1];
    while !b.closed.load(Relaxed) && !crate::httpd::stopping() {
        let (n, from) = match b.socket.recv_from(&mut buf) {
            Ok(x) => x,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => continue,
            // An ICMP "port unreachable" for something this socket sent shows
            // up here on some systems; it is not the socket's end.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
            Err(e) => {
                eprintln!("quillo: UDP port {}: {}", b.socket.local_addr().map_or(0, |a| a.port()), e);
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        RECEIVED.fetch_add(1, Relaxed);
        let dropped = lock(&b.queue).admit(Packet { data: buf[..n].to_vec(), from, at: now_ms() }, b.cap, QUEUE_BYTES);
        DROPPED.fetch_add(dropped, Relaxed);
        b.arrived.notify_one();
    }
}

/// Starts handing packets to the server at `addr`, and keeps the sockets
/// in step with the program: `ports` gives the ports it listens on now
/// (they change when `--watch` reloads it).
pub fn start(addr: SocketAddr, ports: impl Fn() -> Vec<u16> + Send + 'static) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Relaxed) {
        return;
    }
    let addr = crate::jobs::loopback(addr);
    let spawned = std::thread::Builder::new().name("quillo-udp".into()).spawn(move || loop {
        if crate::httpd::stopping() {
            for b in lock(sockets()).values() {
                b.closed.store(true, Relaxed);
                b.arrived.notify_all();
            }
            return;
        }
        let wanted = ports();
        let mut all = lock(sockets());
        all.retain(|port, b| {
            if wanted.contains(port) {
                *lock(&b.wanted) = Instant::now();
            } else if lock(&b.wanted).elapsed() > GRACE {
                b.closed.store(true, Relaxed);
                b.arrived.notify_all();
                return false;
            }
            true
        });
        for (port, b) in all.iter() {
            if !b.delivering.swap(true, Relaxed) {
                let (b, port) = (b.clone(), *port);
                let _ = std::thread::Builder::new().name(format!("quillo-udp-in-{}", port)).spawn(move || deliver_loop(addr, port, b));
            }
        }
        drop(all);
        std::thread::sleep(Duration::from_millis(250));
    });
    if let Err(e) = spawned {
        eprintln!("quillo: cannot start UDP delivery: {}", e);
    }
}

/// Hands a socket's packets to the program, a batch at a time.
fn deliver_loop(addr: SocketAddr, port: u16, b: Arc<Bound>) {
    let mut conn: Option<Loopback> = None;
    loop {
        let batch: Vec<Packet> = {
            let mut q = lock(&b.queue);
            while q.packets.is_empty() {
                if b.closed.load(Relaxed) {
                    return;
                }
                q = b.arrived.wait_timeout(q, Duration::from_millis(250)).unwrap_or_else(std::sync::PoisonError::into_inner).0;
            }
            let (mut n, mut bytes) = (0, 0);
            while n < q.packets.len() && n < BATCH_PACKETS && (n == 0 || bytes + q.packets[n].data.len() <= BATCH_BYTES) {
                bytes += q.packets[n].data.len();
                n += 1;
            }
            q.take(n)
        };
        let body = encode(&batch);
        let path = format!("/_quillo/udp/{}", port);
        // A batch the server never took is said, and counted, where it went with no word (Q-604).
        let lost = |why: &str| {
            DROPPED.fetch_add(batch.len() as u64, Relaxed);
            eprintln!("quillo: UDP port {}: {} packet(s) dropped: {}", port, batch.len(), why);
        };
        // A server that is not answering yet (or any more) is tried again.
        let mut tries = 0;
        loop {
            let c = match conn.take() {
                Some(c) => c,
                None => match Loopback::connect(addr) {
                    Ok(c) => c,
                    Err(_) => {
                        tries += 1;
                        if tries > 50 || b.closed.load(Relaxed) || crate::httpd::stopping() {
                            if tries > 50 {
                                lost("the server could not be reached for 5 s");
                            }
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                },
            };
            match c.post(&path, &body) {
                Ok((c, status)) => {
                    if status >= 500 {
                        eprintln!("quillo: UDP port {}: the handler failed on a batch of {} packet(s)", port, batch.len());
                    }
                    conn = Some(c);
                    break;
                }
                // The connection was closed (the server reloads or stops
                // keeping it): once more on a new one.
                Err(_) => {
                    tries += 1;
                    if tries > 50 || crate::httpd::stopping() {
                        if tries > 50 {
                            lost("the server closed the connection each time");
                        }
                        break;
                    }
                }
            }
        }
    }
}

/// A batch, as the delivery request's body: for each packet, its sender
/// (address length, address bytes, port), its time and its bytes.
fn encode(batch: &[Packet]) -> Vec<u8> {
    let mut out = Vec::with_capacity(batch.iter().map(|p| p.data.len() + 32).sum());
    for p in batch {
        match p.from.ip() {
            std::net::IpAddr::V4(ip) => {
                out.push(4);
                out.extend_from_slice(&ip.octets());
            }
            std::net::IpAddr::V6(ip) => {
                out.push(16);
                out.extend_from_slice(&ip.octets());
            }
        }
        out.extend_from_slice(&p.from.port().to_be_bytes());
        out.extend_from_slice(&p.at.to_be_bytes());
        out.extend_from_slice(&(p.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&p.data);
    }
    out
}

fn decode(mut b: &[u8]) -> Option<Vec<Packet>> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let n = *b.first()? as usize;
        if b.len() < 1 + n + 2 + 8 + 4 {
            return None;
        }
        let ip: std::net::IpAddr = match n {
            4 => <[u8; 4]>::try_from(&b[1..5]).ok()?.into(),
            16 => <[u8; 16]>::try_from(&b[1..17]).ok()?.into(),
            _ => return None,
        };
        b = &b[1 + n..];
        let port = u16::from_be_bytes([b[0], b[1]]);
        let at = u64::from_be_bytes(b[2..10].try_into().ok()?);
        let len = u32::from_be_bytes(b[10..14].try_into().ok()?) as usize;
        b = &b[14..];
        if b.len() < len {
            return None;
        }
        out.push(Packet { data: b[..len].to_vec(), from: SocketAddr::new(ip, port), at });
        b = &b[len..];
    }
    Some(out)
}

/// The delivery thread's own connection to the server, kept open between
/// batches.
/// With no port (a program inside an app), none: each batch is answered here (FEEDBACK39.md,
/// Q-468; branch feedback36-39).
pub struct Loopback(Option<std::net::TcpStream>, Vec<u8>, SocketAddr);

impl Loopback {
    pub fn connect(addr: SocketAddr) -> std::io::Result<Loopback> {
        if crate::jobs::local() {
            return Ok(Loopback(None, Vec::new(), addr));
        }
        let s = crate::jobs::dial(addr, Duration::from_secs(5))?;
        // Not on a local socket, which has no Nagle to turn off.
        let _ = s.set_nodelay(true);
        Ok(Loopback(Some(s), Vec::new(), addr))
    }

    fn post(self, path: &str, body: &[u8]) -> std::io::Result<(Loopback, u16)> {
        self.post_for(path, body).map(|(c, status, _)| (c, status))
    }

    /// Sends the request and reads the answer (its status and body); the
    /// connection is given back for the next one.
    pub fn post_for(mut self, path: &str, body: &[u8]) -> std::io::Result<(Loopback, u16, Vec<u8>)> {
        use std::io::{Read, Write};
        let Some(stream) = self.0.as_mut() else {
            let (status, answer) = crate::jobs::post_self(self.2, path, body)?;
            return Ok((self, status, answer));
        };
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: localhost\r\nX-Quillo-Job: {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
            path,
            crate::jobs::token(),
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;
        let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "bad answer");
        let buf = &mut self.1;
        buf.clear();
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = std::str::from_utf8(&buf[..end]).map_err(|_| bad())?;
        let status = head.split(' ').nth(1).and_then(|s| s.parse::<u16>().ok()).ok_or_else(bad)?;
        let len = head
            .lines()
            .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().to_string()))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let closing = head.lines().any(|l| l.to_ascii_lowercase().starts_with("connection:") && l.to_ascii_lowercase().contains("close"));
        while buf.len() - end < len {
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let answer = buf[end..end + len].to_vec();
        if closing {
            // Answered, but not to be used again: the next batch connects anew.
            return Loopback::connect(self.2).map(|c| (c, status, answer));
        }
        Ok((self, status, answer))
    }
}

/// `POST /_quillo/udp/<port>`: the delivery thread's batch, each packet given
/// to the handler in turn.
pub fn answer(it: &mut Interp, port: &str, body: &[u8]) -> crate::server::HttpResponse {
    let plain = crate::server::plain;
    let Ok(port) = port.parse::<u16>() else {
        return plain(404, "not listening on that port\n");
    };
    let Some(l) = it.site.udp.iter().find(|l| l.port == port).cloned() else {
        // A mounted program's port.
        for (_, sub) in it.mounts.iter_mut() {
            if ports(sub).contains(&port) {
                return answer(sub, &port.to_string(), body);
            }
        }
        return plain(404, "not listening on that port\n");
    };
    let Some(batch) = decode(body) else {
        return plain(400, "bad batch\n");
    };
    let mut failed = 0;
    for p in batch {
        let args = match &l.f {
            Value::Func(c) if c.def.params.is_empty() => Vec::new(),
            _ => vec![packet_value(p, l.port)],
        };
        match it.call(&l.f, args, 0) {
            Ok(_) | Err(Ctrl::Return(_)) | Err(Ctrl::Reply(_)) => {}
            Err(Ctrl::Error(e)) => {
                eprintln!("quillo: error in the handler for UDP port {}: {}", l.port, e);
                failed += 1;
            }
        }
    }
    if failed > 0 {
        plain(500, &format!("{} packet(s) failed\n", failed))
    } else {
        plain(200, "ok\n")
    }
}

/// The ports a program listens on, with those of the programs it mounts.
pub fn ports(it: &Interp) -> Vec<u16> {
    let mut out: Vec<u16> = it.site.udp.iter().map(|l| l.port).collect();
    for (_, sub) in &it.mounts {
        out.extend(ports(sub));
    }
    out
}

fn packet_value(p: Packet, port: u16) -> Value {
    let mut from = Map::new();
    from.insert("host".into(), Value::str(p.from.ip().to_string()));
    from.insert("port".into(), Value::Int(p.from.port() as i64));
    let mut m = Map::new();
    m.insert("text".into(), std::str::from_utf8(&p.data).map_or(Value::Nil, Value::str));
    m.insert("bytes".into(), Value::list(p.data.iter().map(|&b| Value::Int(b as i64)).collect()));
    m.insert("from".into(), Value::map(from));
    m.insert("port".into(), Value::Int(port as i64));
    m.insert("at".into(), Value::Int(p.at as i64));
    Value::map(m)
}

/// What a value sends: text as it is, a list of byte values as those
/// bytes, anything else as JSON.
fn payload(v: &Value, f: &str, line: u32) -> Result<Vec<u8>, Ctrl> {
    let out = match v {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::List(xs) if !xs.is_empty() && xs.iter().all(|x| matches!(x, Value::Int(0..=255))) => {
            xs.iter().map(|x| if let Value::Int(n) = x { *n as u8 } else { 0 }).collect()
        }
        Value::Nil => return fail(line, format!("`{}` has nothing to send", f)).map(|_: Value| Vec::new()),
        v => v.json_text().into_bytes(),
    };
    if out.len() > MAX_PACKET {
        return fail(line, format!("`{}`: {} bytes is more than a UDP packet holds ({})", f, out.len(), MAX_PACKET)).map(|_: Value| Vec::new());
    }
    Ok(out)
}

/// A socket for sending from no port in particular.
fn any_socket(v6: bool) -> std::io::Result<&'static UdpSocket> {
    static V4: OnceLock<UdpSocket> = OnceLock::new();
    static V6: OnceLock<UdpSocket> = OnceLock::new();
    let (cell, at) = if v6 { (&V6, "[::]:0") } else { (&V4, "0.0.0.0:0") };
    if let Some(s) = cell.get() {
        return Ok(s);
    }
    let s = UdpSocket::bind(at)?;
    Ok(cell.get_or_init(|| s))
}

fn send_from(it: &mut Interp, from: Option<u16>, host: &str, port: u16, data: Vec<u8>, f: &str, line: u32) -> R {
    // An address as written needs no lookup; a name may wait on DNS.
    let to = match host.parse::<std::net::IpAddr>() {
        Ok(ip) => Some(SocketAddr::new(ip, port)),
        Err(_) => {
            let h = format!("{}:{}", host, port);
            // IPv4 first: most UDP services listen on it alone, and
            // `localhost` is often ::1 before 127.0.0.1.
            it.unlocked(move || h.to_socket_addrs().ok().and_then(|a| {
                let all: Vec<SocketAddr> = a.collect();
                all.iter().find(|a| a.is_ipv4()).or(all.first()).copied()
            }))
        }
    };
    let Some(to) = to else {
        return Ok(Value::err(Value::str(format!("cannot find the address of {}", host))));
    };
    let sent = match from {
        Some(p) => match lock(sockets()).get(&p).cloned() {
            Some(b) => b.socket.send_to(&data, to),
            None => return fail(line, format!("`{}`: the program does not listen on UDP port {}", f, p)),
        },
        None => any_socket(to.is_ipv6()).and_then(|s| s.send_to(&data, to)),
    };
    Ok(match sent {
        Ok(_) => Value::Nil,
        Err(e) => Value::err(Value::str(format!("cannot send to {}: {}", to, e))),
    })
}

/// `udp.send(host, port, data, { from })`.
fn send(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let host = match args.first() {
        Some(Value::Str(h)) => h.to_string(),
        other => return fail(line, format!("`udp.send` takes a host first, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let port = match args.get(1) {
        Some(Value::Int(n)) if (1..=65535).contains(n) => *n as u16,
        other => return fail(line, format!("`udp.send` takes a port from 1 to 65535 second, got {}", other.map_or("nothing".into(), |v| v.repr()))),
    };
    let data = payload(args.get(2).unwrap_or(&Value::Nil), "udp.send", line)?;
    let from = match args.get(3) {
        None | Some(Value::Nil) => None,
        Some(Value::Map(o)) => match o.get("from") {
            None | Some(Value::Nil) => None,
            Some(Value::Int(p)) if (1..=65535).contains(p) => Some(*p as u16),
            Some(v) => return fail(line, format!("`udp.send`'s `from` is a port the program listens on, got {}", v.repr())),
        },
        Some(v) => return fail(line, format!("`udp.send` expects options as a map, got {}", v.type_name())),
    };
    send_from(it, from, &host, port, data, "udp.send", line)
}

/// `udp.reply(packet, data)`: back to where the packet came from, from the
/// port it came to.
fn reply(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let bad = || fail(line, "`udp.reply` takes a packet that `udp.listen` gave, then what to send".to_string());
    let Some(Value::Map(p)) = args.first() else { return bad() };
    let (Some(Value::Int(port)), Some(Value::Map(from))) = (p.get("port"), p.get("from")) else { return bad() };
    let (Some(Value::Str(host)), Some(Value::Int(to))) = (from.get("host"), from.get("port")) else { return bad() };
    let (port, host, to) = (*port as u16, host.to_string(), *to as u16);
    let data = payload(args.get(1).unwrap_or(&Value::Nil), "udp.reply", line)?;
    send_from(it, Some(port), &host, to, data, "udp.reply", line)
}

#[cfg(test)]
mod tests {
    use super::*;

    // FEEDBACK49.md, Q-604: a queue counted packets, not bytes, and a full one let one sender
    // push out the others.
    #[test]
    fn a_queue_is_held_to_its_bytes_and_one_sender_cannot_push_out_the_rest() {
        let p = |from: &str, n: usize| Packet { data: vec![1; n], from: from.parse().unwrap(), at: 0 };
        let mut q = Queue::default();
        // Bytes: ten packets of 100 in 500 bytes keep the last five.
        let dropped: u64 = (0..10).map(|_| q.admit(p("10.0.0.1:1", 100), 1000, 500)).sum();
        assert_eq!((dropped, q.packets.len(), q.bytes), (5, 5, 500));
        // One sender alone: its oldest go, as before.
        assert_eq!(q.admit(p("10.0.0.1:1", 100), 1000, 500), 1);
        // Another arrives: room is made for it from the first one's oldest.
        assert_eq!(q.admit(p("10.0.0.2:9", 100), 1000, 500), 1);
        assert_eq!(q.from.len(), 2);
        // The first, holding more than half, now has its new packets dropped, and the
        // second's stay.
        for _ in 0..20 {
            assert_eq!(q.admit(p("10.0.0.1:2", 100), 1000, 500), 1);
        }
        assert_eq!(q.from[&"10.0.0.2".parse::<std::net::IpAddr>().unwrap()], 100);
        // The second still gets in, and the packet limit counts as the byte limit does.
        assert_eq!(q.admit(p("10.0.0.2:9", 100), 1000, 500), 1);
        assert_eq!(q.from[&"10.0.0.2".parse::<std::net::IpAddr>().unwrap()], 200);
        let mut few = Queue::default();
        let dropped: u64 = (0..5).map(|_| few.admit(p("10.0.0.1:1", 1), 3, 1 << 20)).sum();
        assert_eq!((dropped, few.packets.len()), (2, 3));
        // Taken out, the counts follow, and a sender with none left is forgotten.
        let all = q.packets.len();
        assert_eq!(q.take(all).len(), all);
        assert_eq!((q.bytes, q.from.len()), (0, 0));
        // An empty packet is let in, and a packet larger than the whole limit empties the queue
        // for it.
        assert_eq!(q.admit(p("10.0.0.3:1", 0), 1000, 500), 0);
        assert_eq!(q.admit(p("10.0.0.3:1", 600), 1000, 500), 1);
        assert_eq!(q.packets.len(), 1);
    }

    #[test]
    fn batches_round_trip() {
        let batch = vec![
            Packet { data: b"hello".to_vec(), from: "127.0.0.1:5000".parse().unwrap(), at: 7 },
            Packet { data: vec![], from: "[::1]:6000".parse().unwrap(), at: 8 },
            Packet { data: vec![0, 255, 10], from: "10.1.2.3:1".parse().unwrap(), at: u64::MAX },
        ];
        let back = decode(&encode(&batch)).unwrap();
        assert_eq!(back.len(), 3);
        for (a, b) in batch.iter().zip(&back) {
            assert_eq!((&a.data, a.from, a.at), (&b.data, b.from, b.at));
        }
        assert!(decode(&encode(&batch)[..10]).is_none());
    }
}
