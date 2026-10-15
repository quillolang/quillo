//! Routing requests to `route` declarations, and the HTTP server.

use crate::ast::Seg;
use crate::interp::{coerce, Ctrl, Interp, Mode};
use crate::shared::Shared;
use crate::value::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
        let mut resp = self.handle_inner(req);
        // Pages with components load the browser bundle.
        let html = resp.header("content-type").map_or(false, |c| c.starts_with("text/html"));
        if html && req.method != "HEAD" {
            let body = String::from_utf8_lossy(&resp.body);
            if body.contains("<wisp-island") && !body.contains("/_wisp/wisp.js") {
                let tag = "<script src=\"/_wisp/wisp.js\" defer></script>";
                let body = match body.rfind("</body>") {
                    Some(i) => format!("{}{}{}", &body[..i], tag, &body[i..]),
                    None => format!("{}{}", body, tag),
                };
                resp.body = body.into_bytes();
            }
        }
        resp
    }

    fn handle_inner(&mut self, req: &HttpRequest) -> HttpResponse {
        let (path, qs) = req.url.split_once('?').unwrap_or((req.url.as_str(), ""));
        if let Some(rest) = path.strip_prefix("/_wisp/") {
            return self.handle_internal(rest, req, path, qs);
        }
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

            self.current_request = Some(reqv);
            let mut resp = finish(self.eval(&r.body, &scope));
            self.current_request = None;
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

impl Interp {
    /// `/_wisp/...`: the browser bundle and `server fn` calls.
    fn handle_internal(&mut self, rest: &str, req: &HttpRequest, path: &str, qs: &str) -> HttpResponse {
        if rest == "wisp.js" {
            return HttpResponse {
                status: 200,
                headers: vec![
                    ("content-type".into(), "text/javascript; charset=utf-8".into()),
                    ("cache-control".into(), "no-cache".into()),
                ],
                body: self.client_js.as_bytes().to_vec(),
            };
        }
        let Some(name) = rest.strip_prefix("rpc/") else {
            return plain(404, "not found\n");
        };
        if req.method != "POST" {
            return plain(405, "method not allowed\n");
        }
        let ctype = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str());
        if !ctype.map_or(false, |c| c.contains("application/json")) {
            return rpc_error(415, "server fn calls must send JSON (content-type: application/json)");
        }
        if !self.server_fns.iter().any(|n| n == name) {
            return rpc_error(404, &format!("no server fn `{}`", name));
        }
        let Some(Value::Func(c)) = lookup(&self.globals, name) else {
            return rpc_error(404, &format!("no server fn `{}`", name));
        };
        let args = match serde_json::from_slice::<serde_json::Value>(&req.body) {
            Ok(serde_json::Value::Array(a)) => a.iter().map(Value::from_json).collect::<Vec<_>>(),
            _ => return rpc_error(400, "the body must be a JSON array of arguments"),
        };
        if let Err(msg) = check_args(&c.def, &args) {
            return rpc_error(400, &msg);
        }
        let query = parse_urlencoded(qs);
        self.current_request = Some(Value::map(build_request(req, path, &query)));
        let res = self.call_closure(&c, args.into_iter().map(Some).collect(), 0);
        self.current_request = None;
        match res {
            Ok(v) | Err(Ctrl::Return(v)) => match v {
                Value::Response(r) => render_response(&r),
                Value::Err(e) => json_response(200, serde_json::json!({ "err": e.to_json() })),
                v => json_response(200, serde_json::json!({ "ok": v.to_json() })),
            },
            Err(Ctrl::Reply(r)) => render_response(&r),
            Err(Ctrl::Error(e)) => {
                eprintln!("wisp: error in server fn `{}`: {}", name, e);
                rpc_error(500, &format!("internal error in `{}`", name))
            }
        }
    }
}

/// Checks a browser's arguments against a `server fn`'s parameters.
fn check_args(def: &crate::ast::FnDef, args: &[Value]) -> Result<(), String> {
    if args.len() > def.params.len() {
        return Err(format!("`{}` takes {} arguments, got {}", def.name, def.params.len(), args.len()));
    }
    for (i, p) in def.params.iter().enumerate() {
        match args.get(i) {
            None if p.default.is_none() => return Err(format!("missing argument `{}` for `{}`", p.name, def.name)),
            Some(v) => {
                if let Some(ty) = &p.ty {
                    if !crate::interp::type_matches(ty, v) {
                        return Err(format!("argument `{}` of `{}` must be {}, got {}", p.name, def.name, ty.text(), v.type_name()));
                    }
                }
            }
            None => {}
        }
    }
    Ok(())
}

fn json_response(status: u16, j: serde_json::Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("content-type".into(), "application/json".into())],
        body: j.to_string().into_bytes(),
    }
}

fn rpc_error(status: u16, msg: &str) -> HttpResponse {
    json_response(status, serde_json::json!({ "error": msg }))
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

/// A program as currently loaded, or the error that stopped it from loading.
pub enum Loaded {
    Ready(Interp),
    Failed(String),
}

/// The running program, which `--watch` swaps out when its files change.
pub struct App {
    slot: Mutex<(u64, Arc<Loaded>)>,
    shared: Arc<Shared>,
    main_file: PathBuf,
    base_dir: PathBuf,
    /// Development mode: reload on change and refresh open pages.
    pub watch: bool,
    /// Run the static checker before each reload.
    pub static_check: bool,
}

impl App {
    /// `loaded` must have been loaded with `shared`; the caller holds the lock.
    pub fn new(loaded: Loaded, shared: Arc<Shared>, main_file: &Path, base_dir: &Path, watch: bool) -> Self {
        App {
            slot: Mutex::new((1, Arc::new(loaded))),
            shared,
            main_file: main_file.to_path_buf(),
            base_dir: base_dir.to_path_buf(),
            watch,
            static_check: true,
        }
    }

    fn current(&self) -> (u64, Arc<Loaded>) {
        let g = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        (g.0, g.1.clone())
    }

    /// Loads the program again from disk and makes it current. A program that
    /// fails to load does not replace one that works.
    pub fn reload(&self) {
        let mut problems = String::new();
        if self.static_check {
            let analysis = crate::check::check_file(&self.main_file);
            for d in &analysis.diagnostics {
                eprintln!("{}", d);
            }
            problems = analysis.errors().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
        }
        self.shared.gil.acquire();
        let mut it = Interp::with_shared(&self.base_dir, self.shared.clone());
        let res = if problems.is_empty() {
            it.load_file(&self.main_file)
        } else {
            Err(crate::error::WispError::new(problems, 0))
        };
        self.shared.gil.release();
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        match res {
            Ok(_) => {
                eprintln!("wisp: reloaded {} ({} routes)", self.main_file.display(), it.routes.len());
                *slot = (slot.0 + 1, Arc::new(Loaded::Ready(it)));
            }
            Err(e) => {
                eprintln!("wisp: {}", e);
                if matches!(*slot.1, Loaded::Failed(_)) {
                    *slot = (slot.0 + 1, Arc::new(Loaded::Failed(e.to_string())));
                } else {
                    eprintln!("wisp: still serving the previous version");
                }
            }
        }
    }

    fn watched_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.main_file.clone()];
        if let Loaded::Ready(it) = &*self.current().1 {
            files.extend(it.files());
        }
        files
    }
}

fn mtimes(files: &[PathBuf]) -> Vec<Option<std::time::SystemTime>> {
    files.iter().map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).collect()
}

/// Polls the program's files and reloads it when one changes.
fn watch(app: Arc<App>) {
    let mut files = app.watched_files();
    let mut seen = mtimes(&files);
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = mtimes(&files);
        if now != seen {
            // Editors often write in several steps; let them finish.
            std::thread::sleep(Duration::from_millis(50));
            app.reload();
            files = app.watched_files();
            seen = mtimes(&files);
        }
    }
}

const DEV_SCRIPT: &str = "<script>(()=>{let v=null;setInterval(async()=>{try{const t=await (await fetch('/_wisp/dev')).text();if(v!==null&&t!==v)location.reload();v=t}catch(e){}},600)})()</script>";

/// In watch mode, HTML pages get a script that reloads them after a change.
fn inject_dev_script(resp: &mut HttpResponse) {
    let html = resp.header("content-type").map_or(false, |c| c.starts_with("text/html"));
    if !html {
        return;
    }
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    resp.body = match body.rfind("</body>") {
        Some(i) => format!("{}{}{}", &body[..i], DEV_SCRIPT, &body[i..]),
        None => format!("{}{}", body, DEV_SCRIPT),
    }
    .into_bytes();
}

fn error_page(msg: &str, dev: bool) -> HttpResponse {
    let mut r = HttpResponse {
        status: 500,
        headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
        body: format!(
            "<!doctype html><title>Wisp error</title><body style=\"font-family:monospace\"><h1>The program did not load</h1><pre>{}</pre></body>",
            crate::interp::escape(msg)
        )
        .into_bytes(),
    };
    if dev {
        inject_dev_script(&mut r);
    }
    r
}

fn to_request(rq: &mut tiny_http::Request) -> HttpRequest {
    let mut body = Vec::new();
    let _ = rq.as_reader().read_to_end(&mut body);
    HttpRequest {
        method: rq.method().as_str().to_ascii_uppercase(),
        url: rq.url().to_string(),
        headers: rq.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect(),
        body,
        remote: rq.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
    }
}

fn worker(app: Arc<App>, server: Arc<tiny_http::Server>) {
    let mut generation = 0;
    let mut local: Option<Interp> = None;
    let mut program = app.current().1;
    while let Ok(mut rq) = server.recv() {
        let start = Instant::now();
        let req = to_request(&mut rq);
        let (g, current) = app.current();
        if g != generation {
            generation = g;
            local = match &*current {
                Loaded::Ready(it) => Some(it.fork()),
                Loaded::Failed(_) => None,
            };
            program = current;
        }
        let mut resp = if app.watch && req.url == "/_wisp/dev" {
            plain(200, &generation.to_string())
        } else {
            match (&mut local, &*program) {
                (Some(it), _) => {
                    app.shared.gil.acquire();
                    let r = it.handle(&req);
                    app.shared.gil.release();
                    r
                }
                (None, Loaded::Failed(e)) => error_page(e, app.watch),
                (None, Loaded::Ready(_)) => unreachable!(),
            }
        };
        if app.watch {
            inject_dev_script(&mut resp);
        }
        if req.url != "/_wisp/dev" {
            eprintln!(
                "{} {} -> {} ({:.1}ms)",
                req.method,
                req.url,
                resp.status,
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        let mut out = tiny_http::Response::from_data(resp.body).with_status_code(resp.status);
        for (k, v) in &resp.headers {
            if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                out.add_header(h);
            }
        }
        let _ = rq.respond(out);
    }
}

/// Serves the app with `workers` threads. The calling thread must hold the
/// interpreter lock; it gives it up here.
pub fn serve(app: App, addr: &str, workers: usize) -> Result<(), String> {
    let server = tiny_http::Server::http(addr).map_err(|e| format!("cannot listen on {}: {}", addr, e))?;
    let server = Arc::new(server);
    let app = Arc::new(app);
    if let Loaded::Ready(it) = &*app.current().1 {
        let n = it.routes.len();
        eprintln!("wisp: {} route{} listening on http://{}", n, if n == 1 { "" } else { "s" }, addr);
    }
    if app.watch {
        eprintln!("wisp: watching for changes");
    }
    app.shared.gil.release();
    let mut handles = Vec::new();
    for i in 0..workers.max(1) {
        let (app, server) = (app.clone(), server.clone());
        let h = std::thread::Builder::new()
            .name(format!("wisp-worker-{}", i))
            .stack_size(WORKER_STACK)
            .spawn(move || {
                worker(app, server)
            })
            .map_err(|e| format!("cannot start worker thread: {}", e))?;
        handles.push(h);
    }
    if app.watch {
        let app = app.clone();
        std::thread::Builder::new()
            .name("wisp-watch".into())
            .stack_size(WORKER_STACK)
            .spawn(move || {
                watch(app)
            })
            .map_err(|e| format!("cannot start file watcher: {}", e))?;
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// Stack for threads that run Wisp code. Calls grow the stack on demand
/// beyond this; it only has to cover deeply nested source and data.
pub const WORKER_STACK: usize = 16 << 20;
