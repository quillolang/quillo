//! `http`: calling other servers.
//!
//! ```text
//! let r = http.get("https://api.example.com/users/1")
//! r.status  r.ok  r.headers  r.body  r.json
//! http.post(url, { name: "Ada" })          // maps and lists are sent as JSON
//! http.request("PUT", url, body, { authorization: "Bearer ..." })
//! http.stream(url, (piece) => ..., { method, body, headers })
//! ```
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
use std::io::Read;
use std::sync::OnceLock;
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

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn send(method: &str, url: &str, headers: &[(String, String)], body: Option<(&str, Vec<u8>)>) -> Result<Reply, String> {
    let mut req = agent().request(method, url);
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

fn do_request(it: &mut Interp, fname: &str, method: &str, url: &Value, body: &Value, headers: &Value, line: u32) -> R {
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
    let res = it.unlocked(move || send(&method, &url, &hdrs, payload));
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

/// `http.get(url, headers = {})`
fn get(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.get", "GET", &a(&args, 0), &Value::Nil, &a(&args, 1), line)
}

/// `http.delete(url, headers = {})`
fn delete(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.delete", "DELETE", &a(&args, 0), &Value::Nil, &a(&args, 1), line)
}

/// `http.post(url, body = null, headers = {})`
fn post(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.post", "POST", &a(&args, 0), &a(&args, 1), &a(&args, 2), line)
}

fn put(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.put", "PUT", &a(&args, 0), &a(&args, 1), &a(&args, 2), line)
}

fn patch(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.patch", "PATCH", &a(&args, 0), &a(&args, 1), &a(&args, 2), line)
}

/// `http.request(method, url, body = null, headers = {})`
fn request(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let method = match a(&args, 0) {
        Value::Str(m) => m.to_ascii_uppercase(),
        other => return fail(line, format!("`http.request` expects a method string first, got {}", other.type_name())),
    };
    do_request(it, "http.request", &method, &a(&args, 1), &a(&args, 2), &a(&args, 3), line)
}

/// For streams: no limit on the whole answer, only on a silence in it.
fn stream_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(300))
            .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

type Body = Box<dyn Read + Send + Sync>;

fn open(method: &str, url: &str, headers: &[(String, String)], body: Option<(&str, Vec<u8>)>) -> Result<(u16, Vec<(String, String)>, Body), String> {
    let mut req = stream_agent().request(method, url);
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

/// The whole pieces at the front of `buf` (all of it at the end), as the
/// arguments the function gets.
fn pieces(buf: &mut Vec<u8>, kind: Kind, end: bool) -> Vec<Vec<Value>> {
    let mut out = Vec::new();
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
            while let Some(i) = buf.iter().position(|b| *b == b'\n').or(if end && !buf.is_empty() { Some(buf.len()) } else { None }) {
                let line: Vec<u8> = buf.drain(..i.min(buf.len())).collect();
                if !buf.is_empty() {
                    buf.remove(0);
                }
                let line = String::from_utf8_lossy(&line);
                let line = line.trim_end_matches('\r');
                if !line.trim().is_empty() {
                    out.push(vec![parsed(line)]);
                }
            }
        }
        Kind::Events => loop {
            // An event ends at a blank line.
            let text = String::from_utf8_lossy(buf).replace("\r\n", "\n");
            let Some(at) = text.find("\n\n").or(if end && !text.trim().is_empty() { Some(text.len()) } else { None }) else { break };
            let block = text[..at].to_string();
            let rest = text[(at + 2).min(text.len())..].to_string();
            *buf = rest.into_bytes();
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
            if buf.is_empty() {
                break;
            }
        },
    }
    out
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
    let (status, headers, mut reader) = match it.unlocked(move || open(&method, &u, &hdrs, payload)) {
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
    let mut buf = Vec::new();
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
                buf.extend_from_slice(&c);
                false
            }
            Err(e) => return Ok(Value::err(Value::str(format!("http: {}", e)))),
        };
        for mut piece in pieces(&mut buf, kind, end) {
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
