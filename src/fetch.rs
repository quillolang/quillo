//! `http`: calling other servers.
//!
//! ```text
//! let r = http.get("https://api.example.com/users/1")
//! r.status  r.ok  r.headers  r.body  r.json
//! http.post(url, { name: "Ada" })          // maps and lists are sent as JSON
//! http.request("PUT", url, body, { authorization: "Bearer ..." })
//! ```
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
        Value::Map(m) => m.iter().map(|(k, v)| (k.clone(), v.display())).collect(),
        Value::Nil => Vec::new(),
        other => return fail(line, format!("`{}` expects headers as a map, got {}", fname, other.type_name())),
    };
    let payload: Option<(&str, Vec<u8>)> = match body {
        Value::Nil => None,
        Value::Str(s) => Some(("text/plain; charset=utf-8", s.as_bytes().to_vec())),
        Value::Html(h) => Some(("text/html; charset=utf-8", h.as_bytes().to_vec())),
        other => Some(("application/json", other.to_json().to_string().into_bytes())),
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
    m.insert("headers".into(), Value::map(reply.headers.into_iter().map(|(k, v)| (k, Value::str(v))).collect()));
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

/// `http.post(url, body = nil, headers = {})`
fn post(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.post", "POST", &a(&args, 0), &a(&args, 1), &a(&args, 2), line)
}

fn put(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.put", "PUT", &a(&args, 0), &a(&args, 1), &a(&args, 2), line)
}

fn patch(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    do_request(it, "http.patch", "PATCH", &a(&args, 0), &a(&args, 1), &a(&args, 2), line)
}

/// `http.request(method, url, body = nil, headers = {})`
fn request(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let method = match a(&args, 0) {
        Value::Str(m) => m.to_ascii_uppercase(),
        other => return fail(line, format!("`http.request` expects a method string first, got {}", other.type_name())),
    };
    do_request(it, "http.request", &method, &a(&args, 1), &a(&args, 2), &a(&args, 3), line)
}
