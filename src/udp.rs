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
struct Bound {
    socket: UdpSocket,
    queue: Mutex<VecDeque<Packet>>,
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
    m.lock().unwrap_or_else(|e| e.into_inner())
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
        queue: Mutex::new(VecDeque::new()),
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
        let mut q = lock(&b.queue);
        if q.len() >= b.cap {
            q.pop_front();
            DROPPED.fetch_add(1, Relaxed);
        }
        q.push_back(Packet { data: buf[..n].to_vec(), from, at: now_ms() });
        drop(q);
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
            while q.is_empty() {
                if b.closed.load(Relaxed) {
                    return;
                }
                q = b.arrived.wait_timeout(q, Duration::from_millis(250)).unwrap_or_else(|e| e.into_inner()).0;
            }
            let (mut n, mut bytes) = (0, 0);
            while n < q.len() && n < BATCH_PACKETS && (n == 0 || bytes + q[n].data.len() <= BATCH_BYTES) {
                bytes += q[n].data.len();
                n += 1;
            }
            q.drain(..n).collect()
        };
        let body = encode(&batch);
        let path = format!("/_quillo/udp/{}", port);
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
pub struct Loopback(std::net::TcpStream, Vec<u8>);

impl Loopback {
    pub fn connect(addr: SocketAddr) -> std::io::Result<Loopback> {
        let s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
        s.set_nodelay(true)?;
        Ok(Loopback(s, Vec::new()))
    }

    fn post(self, path: &str, body: &[u8]) -> std::io::Result<(Loopback, u16)> {
        self.post_for(path, body).map(|(c, status, _)| (c, status))
    }

    /// Sends the request and reads the answer (its status and body); the
    /// connection is given back for the next one.
    pub fn post_for(mut self, path: &str, body: &[u8]) -> std::io::Result<(Loopback, u16, Vec<u8>)> {
        use std::io::{Read, Write};
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: localhost\r\nX-Quillo-Job: {}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
            path,
            crate::jobs::token(),
            body.len()
        );
        self.0.write_all(head.as_bytes())?;
        self.0.write_all(body)?;
        let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "bad answer");
        let buf = &mut self.1;
        buf.clear();
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            let n = self.0.read(&mut chunk)?;
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
            let n = self.0.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let answer = buf[end..end + len].to_vec();
        if closing {
            // Answered, but not to be used again: the next batch connects anew.
            return Loopback::connect(self.0.peer_addr()?).map(|c| (c, status, answer));
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
