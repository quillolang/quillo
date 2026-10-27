//! `http`: calling other servers.
//!
//! ```text
//! let r = http.get("https://api.example.com/users/1")
//! r.status  r.ok  r.headers  r.body  r.json
//! http.post(url, { name: "Ada" })          // maps and lists are sent as JSON
//! http.request("PUT", url, body, { authorization: "Bearer ..." })
//! http.stream(url, (piece) => ..., { method, body, headers })
//! http.post(url, body, {}, { redirects: 0, public_only: true })
//! ```
//!
//! Options, after the headers (in `http.stream`'s map): `redirects`, how
//! many redirects to follow (5 unless given); `public_only`, connect only to
//! addresses on the public internet. That is checked on the addresses the
//! host name gives when the connection is made (and again for a redirect),
//! so a name cannot be looked up as public and then connect inside.
//!
//! `http.stream` hands over the answer as it arrives: each event of an event
//! stream (its data, parsed as JSON when it is, and its name), each line of
//! newline-delimited JSON, or else each piece of text. The function may stop
//! it by returning `false`; given `send` (in a `stream` or `events`), what
//! arrives goes straight on to the client.
//!
//! Any HTTP status is a response; a failure to connect is an error value.
//! The interpreter lock is released while waiting, so other requests run.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

type R = Result<Value, Ctrl>;

const MAX_BODY: u64 = 64 << 20;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("http.get", get),
        ("http.delete", delete),
        ("http.post", post),
        ("http.put", put),
        ("http.patch", patch),
        ("http.request", request),
        ("http.stream", stream),
    ];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("http.").into(), Value::Builtin(name, *f));
    }
}

/// A call's options (see the top of this file).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Opts {
    redirects: u32,
    public_only: bool,
}

const DEFAULT_OPTS: Opts = Opts { redirects: 5, public_only: false };

fn opts_of(v: Option<&Value>, fname: &str, line: u32) -> Result<Opts, Ctrl> {
    let m = match v {
        None | Some(Value::Nil) => return Ok(DEFAULT_OPTS),
        Some(Value::Map(m)) => m,
        Some(other) => return fail(line, format!("`{}` takes options as a map, got {}", fname, other.type_name())),
    };
    let redirects = match m.get("redirects") {
        None | Some(Value::Nil) => DEFAULT_OPTS.redirects,
        Some(Value::Int(n)) if (0..=20).contains(n) => *n as u32,
        Some(other) => return fail(line, format!("`{}`: `redirects` is a whole number from 0 to 20, got {}", fname, other.display())),
    };
    let public_only = match m.get("public_only") {
        None | Some(Value::Nil) => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => return fail(line, format!("`{}`: `public_only` is true or false, got {}", fname, other.type_name())),
    };
    Ok(Opts { redirects, public_only })
}

/// Whether `ip` is on the public internet: not this machine, a private or
/// link-local network (where a cloud's metadata service is), or an address
/// kept for other uses.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => public_v4(a),
        IpAddr::V6(a) => {
            // An IPv4 address written as IPv6 is that address.
            if let Some(v4) = a.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = a.segments();
            // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) lead to IPv4 addresses.
            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
                return public_v4(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
            }
            if s[0] == 0x2002 {
                return public_v4(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
            }
            !(a.is_unspecified()
                || a.is_loopback()
                || a.is_multicast()
                || s[0..6] == [0, 0, 0, 0, 0, 0] // IPv4-compatible, and the rest of ::/96
                || (s[0] & 0xfe00) == 0xfc00 // unique local, fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local, fe80::/10
                || (s[0] & 0xffc0) == 0xfec0 // site-local, fec0::/10
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x2001 && s[1] < 0x0200) // Teredo and other IETF uses, 2001::/23
                || s[0] == 0x0100 && s[1..4] == [0, 0, 0]) // discard-only, 100::/64
        }
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let [x, y, ..] = a.octets();
    !(a.is_unspecified()
        || x == 0 // "this network"
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local() // 169.254.0.0/16, metadata services
        || (x == 100 && (64..128).contains(&y)) // shared address space (carrier NAT)
        || (x == 192 && y == 0 && a.octets()[2] == 0) // IETF protocol assignments
        || a.is_documentation()
        || (x == 198 && (y == 18 || y == 19)) // benchmarking
        || a.is_multicast()
        || x >= 240) // reserved, and broadcast
}

/// Looks `netloc` up and keeps only public addresses; none is an error.
fn resolve_public(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
    let all: Vec<SocketAddr> = netloc.to_socket_addrs()?.collect();
    let public: Vec<SocketAddr> = all.into_iter().filter(|a| public_ip(a.ip())).collect();
    if public.is_empty() {
        let host = netloc.rsplit_once(':').map_or(netloc, |(h, _)| h);
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("{} is not an address on the public internet", host)));
    }
    Ok(public)
}

/// The agent for calls with these options (`stream`: for `http.stream`,
/// which has no limit on the whole answer, only on a silence in it).
fn agent(o: Opts, stream: bool) -> ureq::Agent {
    static AGENTS: OnceLock<Mutex<HashMap<(Opts, bool), ureq::Agent>>> = OnceLock::new();
    let mut agents = AGENTS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    agents
        .entry((o, stream))
        .or_insert_with(|| {
            let b = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .redirects(o.redirects)
                .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")));
            let b = if stream { b.timeout_read(Duration::from_secs(300)) } else { b.timeout(Duration::from_secs(60)) };
            let b = if o.public_only { b.resolver(resolve_public) } else { b };
            b.build()
        })
        .clone()
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn send(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(&str, Vec<u8>)>) -> Result<Reply, String> {
    let mut req = agent(o, false).request(method, url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let res = match body {
        Some((ctype, bytes)) => {
            if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                req = req.set("content-type", ctype);
            }
            req.send_bytes(&bytes)
        }
        None => req.call(),
    };
    let resp = match res {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(e.to_string()),
    };
    let status = resp.status();
    let headers = resp
        .headers_names()
        .into_iter()
        .filter_map(|n| resp.header(&n).map(|v| (n.to_ascii_lowercase(), v.to_string())))
        .collect();
    let mut bytes = Vec::new();
    resp.into_reader().take(MAX_BODY).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(Reply { status, headers, body: String::from_utf8_lossy(&bytes).into_owned() })
}

fn do_request(it: &mut Interp, fname: &str, method: &str, url: &Value, body: &Value, headers: &Value, opts: Option<&Value>, line: u32) -> R {
    let o = opts_of(opts, fname, line)?;
    let Value::Str(url) = url else {
        return fail(line, format!("`{}` expects a URL string, got {}", fname, url.type_name()));
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(Value::err(Value::str(format!("http: not an http(s) URL: {}", url))));
    }
    let hdrs: Vec<(String, String)> = match headers {
        Value::Map(m) => m.iter().map(|(k, v)| (k.to_string(), v.display())).collect(),
        Value::Nil => Vec::new(),
        other => return fail(line, format!("`{}` expects headers as a map, got {}", fname, other.type_name())),
    };
    let payload: Option<(&str, Vec<u8>)> = match body {
        Value::Nil => None,
        Value::Str(s) => Some(("text/plain; charset=utf-8", s.as_bytes().to_vec())),
        Value::Html(h) => Some(("text/html; charset=utf-8", h.as_bytes().to_vec())),
        other => Some(("application/json", other.json_text().into_bytes())),
    };
    let (method, url) = (method.to_string(), url.to_string());
    let res = it.unlocked(move || send(o, &method, &url, &hdrs, payload));
    let reply = match res {
        Ok(r) => r,
        Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
    };
    let json = serde_json::from_str::<serde_json::Value>(&reply.body).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil);
    let mut m = Map::new();
    m.insert("status".into(), Value::Int(reply.status as i64));
    m.insert("ok".into(), Value::Bool((200..300).contains(&reply.status)));
    m.insert("headers".into(), Value::map(reply.headers.into_iter().map(|(k, v)| (k.into(), Value::str(v))).collect()));
    m.insert("body".into(), Value::str(reply.body));
    m.insert("json".into(), json);
    Ok(Value::map(m))
}

fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Nil)
}

/// `http.get(url, headers = {}, options = {})`
fn get(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.get", "GET", &a(&args, 0), &Value::Nil, &a(&args, 1), args.get(2), line)
}

/// `http.delete(url, headers = {}, options = {})`
fn delete(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.delete", "DELETE", &a(&args, 0), &Value::Nil, &a(&args, 1), args.get(2), line)
}

/// `http.post(url, body = null, headers = {}, options = {})`
fn post(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.post", "POST", &a(&args, 0), &a(&args, 1), &a(&args, 2), args.get(3), line)
}

fn put(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.put", "PUT", &a(&args, 0), &a(&args, 1), &a(&args, 2), args.get(3), line)
}

fn patch(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.patch", "PATCH", &a(&args, 0), &a(&args, 1), &a(&args, 2), args.get(3), line)
}

/// `http.request(method, url, body = null, headers = {}, options = {})`
fn request(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let method = match a(&args, 0) {
        Value::Str(m) => m.to_ascii_uppercase(),
        other => return fail(line, format!("`http.request` expects a method string first, got {}", other.type_name())),
    };
    do_request(it, "http.request", &method, &a(&args, 1), &a(&args, 2), &a(&args, 3), args.get(4), line)
}

type Body = Box<dyn Read + Send + Sync>;

fn open(o: Opts, method: &str, url: &str, headers: &[(String, String)], body: Option<(&str, Vec<u8>)>) -> Result<(u16, Vec<(String, String)>, Body), String> {
    let mut req = agent(o, true).request(method, url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let res = match body {
        Some((ctype, bytes)) => {
            if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                req = req.set("content-type", ctype);
            }
            req.send_bytes(&bytes)
        }
        None => req.call(),
    };
    let resp = match res {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(e.to_string()),
    };
    let headers = resp.headers_names().into_iter().filter_map(|n| resp.header(&n).map(|v| (n.to_ascii_lowercase(), v.to_string()))).collect();
    Ok((resp.status(), headers, resp.into_reader()))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Events,
    Lines,
    Text,
}

fn parsed(text: &str) -> Value {
    serde_json::from_str::<serde_json::Value>(text).map(|j| Value::from_json(&j)).unwrap_or_else(|_| Value::str(text))
}

/// Longest line or event `http.stream` waits for the end of: a server that
/// never ends one cannot fill the program's memory.
const MAX_PIECE: usize = 16 << 20;

/// What has arrived of a stream and is not handed over yet. `scanned`:
/// how much of `buf` is known to hold no end of a line or event, so each
/// byte is looked at about once however the stream is cut up.
#[derive(Default)]
struct Pending {
    buf: Vec<u8>,
    scanned: usize,
}

/// Where the first event in `b` ends at a blank line (`\n\n`, `\r\n\r\n`
/// or a mix), looking from `from`: its end, and where the next one starts.
fn event_end(b: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while let Some(j) = b[i..].iter().position(|x| *x == b'\n').map(|j| j + i) {
        match (b.get(j + 1), b.get(j + 2)) {
            (Some(b'\n'), _) => return Some((j, j + 2)),
            (Some(b'\r'), Some(b'\n')) => return Some((j, j + 3)),
            _ => i = j + 1,
        }
    }
    None
}

/// The whole pieces at the front of what arrived (all of it at the end), as
/// the arguments the function gets; an error when one is longer than
/// `MAX_PIECE`.
fn pieces(p: &mut Pending, kind: Kind, end: bool) -> Result<Vec<Vec<Value>>, String> {
    let mut out = Vec::new();
    let buf = &mut p.buf;
    match kind {
        Kind::Text => {
            // Whole characters only: the rest of one comes with the next piece.
            let n = match std::str::from_utf8(buf) {
                Ok(_) => buf.len(),
                Err(e) if e.error_len().is_none() && !end => e.valid_up_to(),
                Err(_) => buf.len(),
            };
            if n > 0 {
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                buf.drain(..n);
                out.push(vec![Value::str(text)]);
            }
        }
        Kind::Lines => {
            let mut start = 0;
            let mut at = p.scanned;
            loop {
                let i = match buf[at..].iter().position(|b| *b == b'\n') {
                    Some(i) => at + i,
                    None if end && start < buf.len() => buf.len(),
                    None => break,
                };
                let line = String::from_utf8_lossy(&buf[start..i]);
                let line = line.trim_end_matches('\r');
                if !line.trim().is_empty() {
                    out.push(vec![parsed(line)]);
                }
                start = (i + 1).min(buf.len());
                at = start;
            }
            buf.drain(..start);
            p.scanned = buf.len();
        }
        Kind::Events => {
            let mut start = 0;
            // A blank line may have begun in what was looked at before.
            let mut from = p.scanned.saturating_sub(2);
            loop {
                let (stop, next) = match event_end(buf, from.max(start)) {
                    Some(e) => e,
                    None if end && !String::from_utf8_lossy(&buf[start..]).trim().is_empty() => (buf.len(), buf.len()),
                    None => break,
                };
                let block = String::from_utf8_lossy(&buf[start..stop]).replace("\r\n", "\n");
                let (mut data, mut name): (Vec<&str>, Option<&str>) = (Vec::new(), None);
                for l in block.lines() {
                    let (field, value) = l.split_once(':').map_or((l, ""), |(f, v)| (f, v.strip_prefix(' ').unwrap_or(v)));
                    match field {
                        "data" => data.push(value),
                        "event" => name = Some(value),
                        _ => {}
                    }
                }
                if !data.is_empty() {
                    out.push(vec![parsed(&data.join("\n")), name.map_or(Value::Nil, Value::str)]);
                }
                start = next;
                from = next;
                if start >= buf.len() {
                    break;
                }
            }
            buf.drain(..start.min(buf.len()));
            p.scanned = buf.len();
        }
    }
    if buf.len() > MAX_PIECE {
        return Err(format!("http: a {} longer than {} bytes", if matches!(kind, Kind::Events) { "event" } else { "line" }, MAX_PIECE));
    }
    Ok(out)
}

/// `http.stream(url, f, { method = "GET", body, headers })`: `{ status, ok,
/// headers }` once the answer has ended (with `body`, unread by `f`, when it
/// is not a success), or an error value when it could not be had.
fn stream(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Value::Str(url) = a(&args, 0) else {
        return fail(line, format!("`http.stream` expects a URL string, got {}", a(&args, 0).type_name()));
    };
    let f = match a(&args, 1) {
        f @ (Value::Func(_) | Value::Builtin(..)) => f,
        other => return fail(line, format!("`http.stream` takes a function for what arrives, got {}", other.type_name())),
    };
    let opts = match a(&args, 2) {
        Value::Map(m) => m,
        Value::Nil => std::sync::Arc::new(Map::new()),
        other => return fail(line, format!("`http.stream` takes options as a map, got {}", other.type_name())),
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(Value::err(Value::str(format!("http: not an http(s) URL: {}", url))));
    }
    let o = opts_of(Some(&Value::Map(opts.clone())), "http.stream", line)?;
    let body = opts.get("body").cloned().unwrap_or(Value::Nil);
    let method = match opts.get("method") {
        Some(Value::Str(m)) => m.to_ascii_uppercase(),
        _ if !matches!(body, Value::Nil) => "POST".into(),
        _ => "GET".into(),
    };
    let hdrs: Vec<(String, String)> = match opts.get("headers") {
        Some(Value::Map(m)) => m.iter().map(|(k, v)| (k.to_string(), v.display())).collect(),
        _ => Vec::new(),
    };
    let payload: Option<(&str, Vec<u8>)> = match &body {
        Value::Nil => None,
        Value::Str(s) => Some(("text/plain; charset=utf-8", s.as_bytes().to_vec())),
        other => Some(("application/json", other.json_text().into_bytes())),
    };
    let u = url.to_string();
    let (status, headers, mut reader) = match it.unlocked(move || open(o, &method, &u, &hdrs, payload)) {
        Ok(r) => r,
        Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
    };
    let ok = (200..300).contains(&status);
    let mut m = Map::new();
    m.insert("status".into(), Value::Int(status as i64));
    m.insert("ok".into(), Value::Bool(ok));
    m.insert("headers".into(), Value::map(headers.iter().map(|(k, v)| (k.into(), Value::str(v))).collect()));
    if !ok {
        let bytes = it.unlocked(move || {
            let mut b = Vec::new();
            let _ = reader.take(MAX_BODY).read_to_end(&mut b);
            b
        });
        m.insert("body".into(), Value::str(String::from_utf8_lossy(&bytes)));
        return Ok(Value::map(m));
    }
    let ctype = headers.iter().find(|(k, _)| k == "content-type").map_or("", |(_, v)| v.as_str()).to_ascii_lowercase();
    let kind = if ctype.starts_with("text/event-stream") {
        Kind::Events
    } else if ctype.contains("ndjson") || ctype.contains("jsonl") || ctype.contains("json-seq") {
        Kind::Lines
    } else {
        Kind::Text
    };
    let params = match &f {
        Value::Func(c) => c.def.params.len(),
        _ => usize::MAX,
    };
    let mut pending = Pending::default();
    loop {
        // Read while other requests run; hand over what came, here.
        let (r, got) = it.unlocked(move || {
            let mut chunk = vec![0u8; 16 << 10];
            let n = reader.read(&mut chunk).map(|n| {
                chunk.truncate(n);
                chunk
            });
            (reader, n)
        });
        reader = r;
        let end = match got {
            Ok(c) if c.is_empty() => true,
            Ok(c) => {
                pending.buf.extend_from_slice(&c);
                false
            }
            Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
        };
        let got = match pieces(&mut pending, kind, end) {
            Ok(g) => g,
            Err(e) => return Ok(Value::err(Value::str(e))),
        };
        for mut piece in got {
            piece.truncate(params);
            if matches!(it.call(&f, piece, line)?, Value::Bool(false)) {
                return Ok(Value::map(m));
            }
        }
        if end {
            return Ok(Value::map(m));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(kind: Kind, input: &[u8], step: usize) -> Result<Vec<String>, String> {
        let mut p = Pending::default();
        let mut got = Vec::new();
        for c in input.chunks(step) {
            p.buf.extend_from_slice(c);
            got.extend(pieces(&mut p, kind, false)?);
        }
        got.extend(pieces(&mut p, kind, true)?);
        Ok(got.iter().map(|a| a.iter().map(|v| v.repr()).collect::<Vec<_>>().join(" ")).collect())
    }

    #[test]
    fn stream_pieces_however_they_arrive() {
        let events = b"data: 1\n\nevent: tick\ndata: {\"a\": 2}\r\n\r\n: comment\ndata: x\ndata: y\n\r\ndata: last";
        let want = vec!["1 null", "{ a: 2 } \"tick\"", "\"x\\ny\" null", "\"last\" null"];
        let lines = b"{\"n\": 1}\n\n2\r\nthree\n4";
        for step in [1, 2, 3, 7, 1000] {
            assert_eq!(feed(Kind::Events, events, step).unwrap(), want, "{}", step);
            assert_eq!(feed(Kind::Lines, lines, step).unwrap(), vec!["{ n: 1 }", "2", "\"three\"", "4"], "{}", step);
        }
    }

    #[test]
    fn a_line_or_event_without_end_is_refused_at_a_limit() {
        // What the review saw: a server that never ends a line fills memory,
        // and each piece that came was decoded again from the start.
        let long = vec![b'x'; MAX_PIECE + 1];
        for kind in [Kind::Lines, Kind::Events] {
            let e = feed(kind, &long, 64 << 10).unwrap_err();
            assert!(e.contains("longer than"), "{}", e);
        }
        // Many events in one go, or one in many pieces: looked at once.
        let mut many = Vec::new();
        for i in 0..20_000 {
            many.extend_from_slice(format!("data: {}\n\n", i).as_bytes());
        }
        let t = std::time::Instant::now();
        assert_eq!(feed(Kind::Events, &many, many.len()).unwrap().len(), 20_000);
        let mut one = b"data: ".to_vec();
        one.extend(vec![b'y'; 4 << 20]);
        one.extend_from_slice(b"\n\n");
        assert_eq!(feed(Kind::Events, &one, 1024).unwrap().len(), 1);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    }
}
