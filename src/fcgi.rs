//! `quillo fcgi`: the program behind a FastCGI process manager (`mod_fcgid` on a shared server,
//! nginx's `fastcgi_pass`), which starts it with a listening socket on file descriptor 0
//! (FEEDBACK41.md, Q-479; branch feedback40-43).
//!
//! Each FastCGI request is made an HTTP/1.0 request and handed to the server as a connection of
//! its own, one end of a pair of local sockets, as a program inside an app is (src/embed.rs). What
//! the server writes back is the CGI answer: `Status:` and the headers, then the body as it comes,
//! so a stream of events goes through as it is written. The server does all the rest, as for a
//! visitor: routes, sessions, limits, the log.

use crate::httpd::Hand;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;

const BEGIN_REQUEST: u8 = 1;
const ABORT_REQUEST: u8 = 2;
const END_REQUEST: u8 = 3;
const PARAMS: u8 = 4;
const STDIN: u8 = 5;
const STDOUT: u8 = 6;
const GET_VALUES: u8 = 9;
const GET_VALUES_RESULT: u8 = 10;
const UNKNOWN_TYPE: u8 = 11;
const RESPONDER: u16 = 1;
const KEEP_CONN: u8 = 1;
const REQUEST_COMPLETE: u8 = 0;
const CANT_MPX_CONN: u8 = 1;
const UNKNOWN_ROLE: u8 = 3;

/// Most bytes of a request's parameters, beside its body (held to the server's `--max-body`).
const MOST_PARAMS: usize = 1 << 20;
/// Connections at once: more are closed as they come, so idle ones cannot use up threads.
const MOST_CONNS: usize = 256;
/// How long a connection may send nothing while a record is awaited.
const QUIET: std::time::Duration = std::time::Duration::from_secs(60);

static CONNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Where FastCGI connections come from.
pub enum Source {
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixListener),
    Tcp(std::net::TcpListener),
}

/// The listening socket a process manager gives on file descriptor 0.
#[cfg(unix)]
pub fn from_fd0() -> Result<Source, String> {
    use std::os::fd::FromRawFd;
    let mut addr: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    // SAFETY: `addr` and `len` are live and sized for the call to write.
    if unsafe { libc::getsockname(0, &mut addr as *mut _ as *mut libc::sockaddr, &mut len) } != 0 {
        return Err("file descriptor 0 is not a socket: `quillo fcgi` is started by a FastCGI process manager (mod_fcgid), or given --listen unix:PATH or --port".into());
    }
    // SAFETY: descriptor 0 is a socket, given to this process to listen on, and taken once.
    Ok(if addr.ss_family as i32 == libc::AF_UNIX {
        Source::Unix(unsafe { std::os::unix::net::UnixListener::from_raw_fd(0) })
    } else {
        Source::Tcp(unsafe { std::net::TcpListener::from_raw_fd(0) })
    })
}

/// Takes FastCGI connections, each on a thread of its own, and gives each request to the server;
/// a body over `max_body` is refused.
pub fn connections(source: Source, max_body: usize) -> Box<dyn FnOnce(Hand) + Send> {
    use std::sync::atomic::Ordering::SeqCst;
    Box::new(move |hand: Hand| {
        let hand = Arc::new(hand);
        while !hand.stopping() {
            let taken: std::io::Result<Box<dyn Conn>> = match &source {
                #[cfg(unix)]
                Source::Unix(l) => l.accept().map(|(s, _)| {
                    let _ = s.set_read_timeout(Some(QUIET));
                    Box::new(s) as Box<dyn Conn>
                }),
                Source::Tcp(l) => l.accept().map(|(s, _)| {
                    let _ = s.set_read_timeout(Some(QUIET));
                    Box::new(s) as Box<dyn Conn>
                }),
            };
            match taken {
                Ok(c) => {
                    if CONNS.fetch_add(1, SeqCst) >= MOST_CONNS {
                        CONNS.fetch_sub(1, SeqCst);
                        continue;
                    }
                    let hand = hand.clone();
                    let started = std::thread::Builder::new().name("quillo-fcgi".into()).spawn(move || {
                        serve_conn(c, &hand, max_body);
                        CONNS.fetch_sub(1, SeqCst);
                    });
                    if started.is_err() {
                        CONNS.fetch_sub(1, SeqCst);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    eprintln!("quillo: cannot accept a FastCGI connection: {}", e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    })
}

pub trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

/// A record: its type, request id and content.
fn read_record(c: &mut dyn Conn) -> std::io::Result<(u8, u16, Vec<u8>)> {
    let mut h = [0u8; 8];
    c.read_exact(&mut h)?;
    if h[0] != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not FastCGI version 1"));
    }
    let id = u16::from_be_bytes([h[2], h[3]]);
    let len = u16::from_be_bytes([h[4], h[5]]) as usize;
    let mut body = vec![0u8; len + h[6] as usize];
    c.read_exact(&mut body)?;
    body.truncate(len);
    Ok((h[1], id, body))
}

fn record(kind: u8, id: u16, content: &[u8]) -> Vec<u8> {
    let pad = (8 - content.len() % 8) % 8;
    let mut r = Vec::with_capacity(8 + content.len() + pad);
    r.extend_from_slice(&[1, kind]);
    r.extend_from_slice(&id.to_be_bytes());
    r.extend_from_slice(&(content.len() as u16).to_be_bytes());
    r.extend_from_slice(&[pad as u8, 0]);
    r.extend_from_slice(content);
    r.resize(r.len() + pad, 0);
    r
}

fn end_request(id: u16, app: u32, protocol: u8) -> Vec<u8> {
    let mut b = app.to_be_bytes().to_vec();
    b.extend_from_slice(&[protocol, 0, 0, 0]);
    record(END_REQUEST, id, &b)
}

/// Writes `data` as STDOUT records of at most 65535 bytes each.
fn stdout(c: &mut dyn Conn, id: u16, data: &[u8]) -> std::io::Result<()> {
    for part in data.chunks(65535) {
        c.write_all(&record(STDOUT, id, part))?;
    }
    Ok(())
}

/// Name-value pairs, each length one byte, or four with the high bit set.
pub fn pairs(mut b: &[u8]) -> Vec<(String, String)> {
    fn len(b: &mut &[u8]) -> Option<usize> {
        let first = *b.first()?;
        if first >> 7 == 0 {
            *b = &b[1..];
            return Some(first as usize);
        }
        let four = b.get(..4)?;
        let n = (u32::from_be_bytes([four[0] & 0x7f, four[1], four[2], four[3]])) as usize;
        *b = &b[4..];
        Some(n)
    }
    let mut out = Vec::new();
    while !b.is_empty() {
        let (Some(n), Some(v)) = (len(&mut b), len(&mut b)) else { break };
        let (Some(name), Some(value)) = (b.get(..n), b.get(n..n + v)) else { break };
        out.push((String::from_utf8_lossy(name).into_owned(), String::from_utf8_lossy(value).into_owned()));
        b = &b[n + v..];
    }
    out
}

fn pair(name: &str, value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for n in [name.len(), value.len()] {
        if n < 128 {
            out.push(n as u8);
        } else {
            out.extend_from_slice(&((n as u32) | 0x8000_0000).to_be_bytes());
        }
    }
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(value.as_bytes());
    out
}

/// The HTTP/1.0 request a FastCGI request's parameters and body make, and who it is from.
pub fn http_request(params: &[(String, String)], body: &[u8]) -> (Vec<u8>, SocketAddr) {
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let method = get("REQUEST_METHOD").filter(|m| !m.is_empty() && m.bytes().all(|b| b.is_ascii_uppercase())).unwrap_or("GET");
    let uri = match get("REQUEST_URI").filter(|u| u.starts_with('/')) {
        Some(u) => u.to_string(),
        None => {
            let path = format!("{}{}", get("SCRIPT_NAME").unwrap_or(""), get("PATH_INFO").unwrap_or(""));
            let path = if path.starts_with('/') { path } else { format!("/{}", path) };
            match get("QUERY_STRING").filter(|q| !q.is_empty()) {
                Some(q) => format!("{}?{}", path, q),
                None => path,
            }
        }
    };
    // What would split the request line or a header is not passed on; a header's name is letters,
    // digits and `_` alone.
    let clean = |s: &str| !s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0);
    let a_name = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    let uri = if clean(&uri) && !uri.contains(' ') { uri } else { "/".to_string() };
    let mut head = format!("{} {} HTTP/1.0\r\n", method, uri);
    let mut host = false;
    for (k, v) in params {
        let name = match k.as_str() {
            "CONTENT_TYPE" => "content-type".to_string(),
            k => match k.strip_prefix("HTTP_").map(|h| h.to_ascii_uppercase()).filter(|h| a_name(h)) {
                // Its length is the body's here; how it came is the server's business; and the
                // token of this process's own jobs never comes from outside.
                Some(h) if matches!(h.as_str(), "CONTENT_LENGTH" | "CONTENT_TYPE" | "CONNECTION" | "TRANSFER_ENCODING" | "PROXY" | "X_QUILLO_JOB") => continue,
                None => continue,
                Some(h) => h.to_ascii_lowercase().replace('_', "-"),
            },
        };
        if v.is_empty() && name == "content-type" || !clean(v) {
            continue;
        }
        host |= name == "host";
        head.push_str(&format!("{}: {}\r\n", name, v));
    }
    if !host {
        head.push_str(&format!("host: {}\r\n", get("SERVER_NAME").filter(|h| clean(h)).unwrap_or("localhost")));
    }
    head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
    let mut req = head.into_bytes();
    req.extend_from_slice(body);
    let ip = get("REMOTE_ADDR").and_then(|a| a.parse::<std::net::IpAddr>().ok()).unwrap_or([127, 0, 0, 1].into());
    let port = get("REMOTE_PORT").and_then(|p| p.parse().ok()).unwrap_or(0);
    (req, SocketAddr::new(ip, port))
}

/// The CGI answer's head for an HTTP one: `Status:` for the status line, and the headers but
/// those of the connection.
pub fn cgi_head(http: &str) -> String {
    let mut lines = http.split("\r\n");
    let status = lines.next().and_then(|l| l.split_once(' ')).map_or("500 Internal Server Error", |(_, s)| s).to_string();
    let mut out = format!("Status: {}\r\n", status);
    for l in lines.filter(|l| !l.is_empty()) {
        let name = l.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
        if matches!(name.as_str(), "connection" | "keep-alive" | "transfer-encoding") {
            continue;
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out
}

/// Answers one request: given to the server, its answer written back as it comes.
fn answer(c: &mut dyn Conn, id: u16, params: &[(String, String)], body: &[u8], hand: &Hand) -> std::io::Result<()> {
    let (req, peer) = http_request(params, body);
    let (mut ours, theirs) = pair_of_sockets()?;
    hand.give(theirs, peer);
    ours.write_all(&req)?;
    // The head, then the body as it is written.
    let mut buf = vec![0u8; 16 << 10];
    let mut head = Vec::new();
    let rest = loop {
        let n = ours.read(&mut buf)?;
        if n == 0 {
            break Vec::new();
        }
        head.extend_from_slice(&buf[..n]);
        if let Some(i) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break head.split_off(i + 4);
        }
        if head.len() > 1 << 20 {
            break Vec::new();
        }
    };
    let text = String::from_utf8_lossy(head.strip_suffix(b"\r\n\r\n").unwrap_or(&head)).into_owned();
    stdout(c, id, cgi_head(&text).as_bytes())?;
    if !rest.is_empty() {
        stdout(c, id, &rest)?;
    }
    loop {
        let n = ours.read(&mut buf)?;
        if n == 0 {
            break;
        }
        stdout(c, id, &buf[..n])?;
        c.flush()?;
    }
    c.write_all(&record(STDOUT, id, &[]))?;
    c.write_all(&end_request(id, 0, REQUEST_COMPLETE))?;
    c.flush()
}

#[cfg(unix)]
fn pair_of_sockets() -> std::io::Result<(std::os::unix::net::UnixStream, std::net::TcpStream)> {
    let (ours, theirs) = std::os::unix::net::UnixStream::pair()?;
    Ok((ours, crate::httpd::local_as_tcp(theirs)))
}

#[cfg(not(unix))]
fn pair_of_sockets() -> std::io::Result<(std::net::TcpStream, std::net::TcpStream)> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "FastCGI is for Unix"))
}

/// One connection's records, a request at a time (FCGI_MPXS_CONNS is 0: a second request on it
/// before the first ends is refused, as the specification allows).
fn serve_conn(mut c: Box<dyn Conn>, hand: &Hand, max_body: usize) {
    let mut current: Option<(u16, bool, Vec<u8>, Option<Vec<(String, String)>>, Vec<u8>)> = None;
    loop {
        let Ok((kind, id, content)) = read_record(&mut *c) else { return };
        match kind {
            GET_VALUES if id == 0 => {
                let mut out = Vec::new();
                for (k, _) in pairs(&content) {
                    let v = match k.as_str() {
                        "FCGI_MPXS_CONNS" => "0",
                        "FCGI_MAX_CONNS" | "FCGI_MAX_REQS" => "64",
                        _ => continue,
                    };
                    out.extend(pair(&k, v));
                }
                if c.write_all(&record(GET_VALUES_RESULT, 0, &out)).is_err() {
                    return;
                }
            }
            BEGIN_REQUEST if content.len() >= 3 => {
                let role = u16::from_be_bytes([content[0], content[1]]);
                let keep = content[2] & KEEP_CONN != 0;
                if current.is_some() {
                    let _ = c.write_all(&end_request(id, 0, CANT_MPX_CONN));
                    continue;
                }
                if role != RESPONDER {
                    if c.write_all(&end_request(id, 0, UNKNOWN_ROLE)).is_err() || !keep {
                        return;
                    }
                    continue;
                }
                current = Some((id, keep, Vec::new(), None, Vec::new()));
            }
            ABORT_REQUEST => {
                if current.as_ref().is_some_and(|r| r.0 == id) {
                    let keep = current.take().is_some_and(|r| r.1);
                    if c.write_all(&end_request(id, 0, REQUEST_COMPLETE)).is_err() || !keep {
                        return;
                    }
                }
            }
            PARAMS | STDIN => {
                let Some(r) = current.as_mut().filter(|r| r.0 == id) else { continue };
                let over = if kind == PARAMS { r.2.len() + content.len() > MOST_PARAMS } else { r.4.len() + content.len() > max_body };
                if over {
                    let _ = c.write_all(&end_request(id, 1, REQUEST_COMPLETE));
                    return;
                }
                if kind == PARAMS {
                    if content.is_empty() {
                        r.3 = Some(pairs(&r.2));
                    } else {
                        r.2.extend_from_slice(&content);
                    }
                } else if !content.is_empty() {
                    r.4.extend_from_slice(&content);
                } else {
                    let (id, keep, _, params, body) = current.take().unwrap();
                    let params = params.unwrap_or_default();
                    if answer(&mut *c, id, &params, &body, hand).is_err() || !keep {
                        return;
                    }
                }
            }
            // A management record of a kind not known here; others are dropped.
            k if id == 0 => {
                if c.write_all(&record(UNKNOWN_TYPE, 0, &[k, 0, 0, 0, 0, 0, 0, 0])).is_err() {
                    return;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_both_ways() {
        let long = "x".repeat(300);
        let mut b = pair("REQUEST_METHOD", "GET");
        b.extend(pair("LONG", &long));
        b.extend(pair("EMPTY", ""));
        assert_eq!(pairs(&b), [("REQUEST_METHOD".into(), "GET".into()), ("LONG".into(), long), ("EMPTY".into(), String::new())]);
        // Cut short: what is whole is kept.
        assert_eq!(pairs(&b[..b.len() - 3]).len(), 2);
        assert!(pairs(&[0x80, 0, 0]).is_empty());
    }

    #[test]
    fn a_request_made_http() {
        let p = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        let (req, peer) = http_request(
            &p(&[("REQUEST_METHOD", "POST"), ("REQUEST_URI", "/a?b=1"), ("HTTP_HOST", "example.com"), ("HTTP_X_THING", "v"), ("CONTENT_TYPE", "text/plain"), ("HTTP_CONTENT_LENGTH", "99"), ("REMOTE_ADDR", "203.0.113.9"), ("REMOTE_PORT", "4321"), ("HTTP_EVIL", "a\r\nInjected: 1")]),
            b"hi",
        );
        let text = String::from_utf8(req).unwrap();
        assert_eq!(text, "POST /a?b=1 HTTP/1.0\r\nhost: example.com\r\nx-thing: v\r\ncontent-type: text/plain\r\ncontent-length: 2\r\n\r\nhi");
        assert_eq!(peer, "203.0.113.9:4321".parse().unwrap());
        let (req, peer) = http_request(&p(&[("SCRIPT_NAME", "/app"), ("PATH_INFO", "/x"), ("QUERY_STRING", "q=1"), ("SERVER_NAME", "s.test"), ("REQUEST_METHOD", "get me")]), b"");
        assert_eq!(String::from_utf8(req).unwrap(), "GET /app/x?q=1 HTTP/1.0\r\nhost: s.test\r\ncontent-length: 0\r\n\r\n");
        assert_eq!(peer, "127.0.0.1:0".parse().unwrap());
        // Names that would make headers of their own, a length by another spelling, and the
        // token of the process's own jobs: none passed on.
        let (req, _) = http_request(&p(&[("REQUEST_URI", "/"), ("HTTP_A: b\r\nX-Forwarded-For", "7.7.7.7"), ("HTTP_content_length", "5"), ("HTTP_X_QUILLO_JOB", "t"), ("HTTP_", "x"), ("HTTP_OK_ONE", "y")]), b"");
        assert_eq!(String::from_utf8(req).unwrap(), "GET / HTTP/1.0\r\nok-one: y\r\nhost: localhost\r\ncontent-length: 0\r\n\r\n");
        let (req, _) = http_request(&p(&[("REQUEST_URI", "/a b")]), b"");
        assert!(String::from_utf8(req).unwrap().starts_with("GET / HTTP/1.0\r\nhost: localhost\r\n"));
    }

    #[test]
    fn an_answer_made_cgi() {
        assert_eq!(cgi_head("HTTP/1.1 404 Not Found\r\ncontent-type: text/plain\r\nconnection: close\r\nTransfer-Encoding: chunked\r\nx-a: 1"), "Status: 404 Not Found\r\ncontent-type: text/plain\r\nx-a: 1\r\n\r\n");
        assert_eq!(cgi_head(""), "Status: 500 Internal Server Error\r\n\r\n");
    }

    #[test]
    fn records_are_padded_to_eight() {
        let r = record(STDOUT, 3, b"abc");
        assert_eq!(r.len(), 16);
        assert_eq!(&r[..8], &[1, STDOUT, 0, 3, 0, 3, 5, 0]);
        assert_eq!(record(STDOUT, 1, &[]).len(), 8);
    }
}
