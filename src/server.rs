//! Routing requests to `route` declarations, and the HTTP server.

use crate::ast::Seg;
use crate::interp::{coerce, Ctrl, Interp, Mode};
use crate::value::*;
use std::time::Instant;

pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub remote: String,
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn plain(status: u16, msg: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
        body: msg.as_bytes().to_vec(),
    }
}

pub fn percent_decode(s: &str, plus_is_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        match b[i] {
            b'+' if plus_is_space => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_urlencoded(s: &str) -> Map {
    let mut m = Map::new();
    for pair in s.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        m.insert(percent_decode(k, true), Value::str(percent_decode(v, true)));
    }
    m
}

fn match_path(def: &[Seg], segs: &[String]) -> Option<Vec<(String, Value)>> {
    let mut out = Vec::new();
    for (i, seg) in def.iter().enumerate() {
        match seg {
            Seg::Rest(name) => {
                out.push((name.clone(), Value::str(segs.get(i..).map(|s| s.join("/")).unwrap_or_default())));
                return Some(out);
            }
            Seg::Lit(l) => {
                if segs.get(i)? != l {
                    return None;
                }
            }
            Seg::Param(name, ty) => {
                let s = segs.get(i)?;
                let v = match ty {
                    Some(t) => coerce(t, s)?,
                    None => Value::str(s),
                };
                out.push((name.clone(), v));
            }
        }
    }
    if def.len() == segs.len() {
        Some(out)
    } else {
        None
    }
}

/// The `request` value a route sees.
fn build_request(req: &HttpRequest, path: &str, query: &Map) -> Map {
    let mut headers = Map::new();
    for (k, v) in &req.headers {
        headers.insert(k.to_ascii_lowercase(), Value::str(v));
    }
    let header = |name: &str| headers.get(name).map(|v| v.display()).unwrap_or_default();
    let ctype = header("content-type");
    let mut cookies = Map::new();
    for part in header("cookie").split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            cookies.insert(k.to_string(), Value::str(percent_decode(v, false)));
        }
    }
    let body = String::from_utf8_lossy(&req.body).into_owned();
    let trimmed = body.trim_start();
    let json = if ctype.contains("json") || trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(&body).map(|j| Value::from_json(&j)).unwrap_or(Value::Nil)
    } else {
        Value::Nil
    };
    let form = if ctype.contains("application/x-www-form-urlencoded") { parse_urlencoded(&body) } else { Map::new() };

    let mut m = Map::new();
    m.insert("method".into(), Value::str(&req.method));
    m.insert("path".into(), Value::str(path));
    m.insert("query".into(), Value::map(query.clone()));
    m.insert("params".into(), Value::map(Map::new()));
    m.insert("headers".into(), Value::map(headers.clone()));
    m.insert("cookies".into(), Value::map(cookies));
    m.insert("body".into(), Value::str(body));
    m.insert("json".into(), json);
    m.insert("form".into(), Value::map(form));
    m.insert("ip".into(), Value::str(&req.remote));
    m
}

impl Interp {
    /// Runs the first route whose method, path and patterns all match.
    pub fn handle(&mut self, req: &HttpRequest) -> HttpResponse {
        let (path, qs) = req.url.split_once('?').unwrap_or((req.url.as_str(), ""));
        let segs: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s, false)).collect();
        let method = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
        let query = parse_urlencoded(qs);
        let query_v = Value::map(query.clone());
        let base = build_request(req, path, &query);

        let mut path_matched = false;
        let mut method_matched = false;
        let routes = self.routes.clone();
        for r in &routes {
            let Some(params) = match_path(&r.path, &segs) else { continue };
            path_matched = true;
            if r.method != "ANY" && r.method != method {
                continue;
            }
            method_matched = true;

            let mut reqmap = base.clone();
            reqmap.insert("params".into(), Value::map(params.iter().cloned().collect()));
            let reqv = Value::map(reqmap);
            let scope = Scope::child(&self.globals);
            define(&scope, "request", reqv.clone());
            for (k, v) in params {
                define(&scope, &k, v);
            }

            let mut binds = Vec::new();
            let checks = [(r.query.as_ref(), &query_v, Mode::Lenient), (r.body_pat.as_ref(), &reqv, Mode::Request)];
            let mut matched = true;
            for (pat, val, mode) in checks {
                let Some(pat) = pat else { continue };
                match self.match_pat(pat, val, mode, &scope, &mut binds) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        matched = false;
                        break;
                    }
                    Err(e) => return finish(Err(e)),
                }
            }
            if !matched {
                continue;
            }
            for (k, v) in binds {
                define(&scope, &k, v);
            }

            let mut resp = finish(self.eval(&r.body, &scope));
            if req.method == "HEAD" {
                resp.body.clear();
            }
            return resp;
        }
        if method_matched {
            plain(400, "bad request: no route pattern matches this request\n")
        } else if path_matched {
            plain(405, "method not allowed\n")
        } else {
            plain(404, "not found\n")
        }
    }
}

/// Turns what a route produced into an HTTP response.
pub fn finish(res: Result<Value, Ctrl>) -> HttpResponse {
    match res {
        Ok(v) | Err(Ctrl::Return(v)) => value_response(v),
        Err(Ctrl::Reply(r)) => render_response(&r),
        Err(Ctrl::Error(e)) => {
            eprintln!("wisp: error: {}", e);
            plain(500, &format!("internal error: {}\n", e))
        }
    }
}

fn value_response(v: Value) -> HttpResponse {
    match v {
        Value::Response(r) => render_response(&r),
        other => {
            let status = match &other {
                Value::Nil => 204,
                Value::Err(_) => 500,
                _ => 200,
            };
            render_response(&Response { status, headers: Vec::new(), body: Body::Value(other) })
        }
    }
}

/// Body encoding follows the value: html -> text/html, str -> text/plain,
/// nil -> empty, anything else -> JSON.
fn render_response(r: &Response) -> HttpResponse {
    let (ctype, body): (Option<&str>, Vec<u8>) = match &r.body {
        Body::Bytes(b) => (None, b.clone()),
        Body::Value(v) => match v {
            Value::Nil => (None, Vec::new()),
            Value::Html(h) => (Some("text/html; charset=utf-8"), h.as_bytes().to_vec()),
            Value::Str(s) => (Some("text/plain; charset=utf-8"), s.as_bytes().to_vec()),
            Value::Err(e) => {
                (Some("application/json"), serde_json::json!({ "error": e.to_json() }).to_string().into_bytes())
            }
            other => (Some("application/json"), other.to_json().to_string().into_bytes()),
        },
    };
    let mut headers = r.headers.clone();
    if let Some(ct) = ctype {
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            headers.push(("content-type".into(), ct.into()));
        }
    }
    HttpResponse { status: r.status, headers, body }
}

pub fn serve(interp: &mut Interp, addr: &str) -> Result<(), String> {
    let server = tiny_http::Server::http(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    let n = interp.routes.len();
    eprintln!("wisp: {} route{} listening on http://{}", n, if n == 1 { "" } else { "s" }, addr);
    for mut rq in server.incoming_requests() {
        let start = Instant::now();
        let mut body = Vec::new();
        let _ = rq.as_reader().read_to_end(&mut body);
        let req = HttpRequest {
            method: rq.method().as_str().to_ascii_uppercase(),
            url: rq.url().to_string(),
            headers: rq.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect(),
            body,
            remote: rq.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
        };
        let resp = interp.handle(&req);
        eprintln!(
            "{} {} -> {} ({:.1}ms)",
            req.method,
            req.url,
            resp.status,
            start.elapsed().as_secs_f64() * 1000.0
        );
        let mut out = tiny_http::Response::from_data(resp.body).with_status_code(resp.status);
        for (k, v) in &resp.headers {
            if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                out.add_header(h);
            }
        }
        let _ = rq.respond(out);
    }
    Ok(())
}
