//! Services: parts of an app that run in this process or as servers of
//! their own, without the code changing.
//!
//! `service billing = "billing.qwe"` makes `billing.invoice(...)` a call to
//! billing's `server fn invoice`; `service /shop = "shop.qwe"` is the same
//! (named after the file) and serves shop's routes under /shop too.
//!
//! Where a service runs is configuration, not code:
//! - here (the default): loaded once per process and shared by every
//!   program in it that names it, so its variables and database are one;
//! - elsewhere: `--service-url billing=http://…` (or `QUILLO_SERVICE_BILLING`)
//!   sends its calls there, `POST /_quillo/service/<fn>`, signed with the
//!   shared `QUILLO_SERVICE_SECRET` and carrying the tenant; its routes, if
//!   it has a prefix, are passed on to it.
//!
//! Either way what crosses is data, as JSON: a program behaves the same
//! whichever way it runs. `quillo run app.qwe --split` runs each service as
//! a process of its own, to see it so (see `split`).

use crate::ast::{FnDef, Stmt};
use crate::interp::{fail, Ctrl, Interp};
use crate::parser::Parser;
use crate::server::{HttpRequest, HttpResponse};
use crate::value::*;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

type R = Result<Value, Ctrl>;

/// How old a signed call may be (seconds), either way: clocks differ a little.
const WINDOW: u64 = 300;

/// The secret services sign their calls with, if set.
fn secret() -> Option<String> {
    std::env::var("QUILLO_SERVICE_SECRET").ok().filter(|s| !s.is_empty())
}

/// `QUILLO_SERVICE_BILLING` for `billing`.
pub fn env_name(name: &str) -> String {
    format!("QUILLO_SERVICE_{}", name.to_ascii_uppercase())
}

/// Where service `name` runs, when not in this process.
pub fn url_of(shared: &crate::shared::Shared, name: &str) -> Option<String> {
    if let Some(u) = shared.service_urls.lock().unwrap_or_else(|e| e.into_inner()).get(name) {
        return Some(u.trim_end_matches('/').to_string());
    }
    std::env::var(env_name(name)).ok().filter(|u| !u.is_empty()).map(|u| u.trim_end_matches('/').to_string())
}

// ---- what a service offers ---------------------------------------------------------------

/// The statements of a program and the files it `use`s.
fn statements(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<Stmt>) -> Result<(), String> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(key) {
        return Ok(());
    }
    let src = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let stmts = Parser::new(&src).parse_program().map_err(|e| format!("{}: {}", path.display(), e))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    for s in stmts {
        if let Stmt::Use(file, _) = &s {
            statements(&dir.join(file), seen, out)?;
        }
        out.push(s);
    }
    Ok(())
}

/// The `server fn`s a service's program offers (its own and its `use`d files').
pub fn offered(path: &Path) -> Result<Vec<Arc<FnDef>>, String> {
    let mut stmts = Vec::new();
    statements(path, &mut HashSet::new(), &mut stmts)?;
    Ok(stmts
        .into_iter()
        .filter_map(|s| match s {
            Stmt::Fn(d) if d.server => Some(d),
            _ => None,
        })
        .collect())
}

/// Every service a program and those it runs declare: name, prefix, file.
/// For `--split`, which starts each.
pub fn declared(path: &Path) -> Result<Vec<(String, Option<String>, PathBuf)>, String> {
    let mut found: Vec<(String, Option<String>, PathBuf)> = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    let mut done = HashSet::new();
    while let Some(p) = todo.pop() {
        if !done.insert(p.canonicalize().unwrap_or_else(|_| p.clone())) {
            continue;
        }
        let mut stmts = Vec::new();
        statements(&p, &mut HashSet::new(), &mut stmts)?;
        let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
        for s in stmts {
            if let Stmt::Service(name, prefix, file, _) = s {
                let full = dir.join(&file);
                match found.iter_mut().find(|(n, _, _)| *n == name) {
                    Some((_, pre, _)) => {
                        if pre.is_none() {
                            *pre = prefix;
                        }
                    }
                    None => found.push((name, prefix, full.clone())),
                }
                todo.push(full);
            }
        }
    }
    Ok(found)
}

/// The value a service's name stands for: a map of functions, one per
/// `server fn` it offers, each taking that function's parameters.
pub fn namespace(it: &mut Interp, name: &str, defs: &[Arc<FnDef>], line: u32) -> R {
    let mut m = Map::new();
    for d in defs {
        let params: Vec<String> = d.params.iter().map(|p| if p.default.is_some() { format!("{} = null", p.name) } else { p.name.clone() }).collect();
        let names: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
        let required = d.params.iter().filter(|p| p.default.is_none()).count();
        let src = format!("({}) => __service({:?}, {:?}, [{}], {})", params.join(", "), name, d.name, names.join(", "), required);
        let lambda = Parser::new(&src).parse_expr().map_err(|e| Ctrl::Error(crate::error::QuilloError::new(e.msg, line)))?;
        let g = it.globals.clone();
        let f = it.eval(&lambda, &g)?;
        m.insert(d.name.as_str().into(), f);
    }
    Ok(Value::map(m))
}

// ---- calling ---------------------------------------------------------------------------

/// `__service(name, fn, [args], required)`: a call to a service's server fn,
/// here or where it runs. Optional arguments left out are not sent, so the
/// service's own defaults apply. A service out of reach is an error value.
pub fn call(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::Str(name)), Some(Value::Str(f)), Some(Value::List(given))) = (args.first(), args.get(1), args.get(2)) else {
        return fail(line, "`__service` is Quillo's own: call a service as `billing.invoice(...)`");
    };
    let required = match args.get(3) {
        Some(Value::Int(n)) => *n as usize,
        _ => given.len(),
    };
    let mut sent: Vec<Value> = given.iter().cloned().collect();
    while sent.len() > required && matches!(sent.last(), Some(Value::Nil)) {
        sent.pop();
    }
    let tenant = it.tenant();
    match url_of(&it.shared, name) {
        Some(url) => remote(it, &url, name, f, sent, tenant),
        None => local(it, name, f, sent, tenant, line),
    }
}

fn error(msg: String) -> R {
    Ok(Value::err(Value::str(msg)))
}

/// A copy of a value, as the other side of a network would have it (an
/// error stays one, as the answer of a call says so).
fn crossed(v: &Value) -> Value {
    match v {
        Value::Err(e) => Value::err(crossed(e)),
        v => Value::from_json(&v.to_json()),
    }
}

fn local(it: &mut Interp, name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>, line: u32) -> R {
    let found = it.shared.services.lock().unwrap_or_else(|e| e.into_inner()).get(name).map(|(_, s)| s.fork());
    let Some(mut svc) = found else {
        return error(format!("service {}: not running here, and no address for it ({} or --service-url)", name, env_name(name)));
    };
    let Some(Value::Func(c)) = lookup(&svc.globals, f).filter(|_| svc.server_fns.iter().any(|n| n == f)) else {
        return error(format!("service {}: no server fn `{}`", name, f));
    };
    let args: Vec<Value> = args.iter().map(crossed).collect();
    if let Err(msg) = crate::server::check_args(&c.def, &args) {
        return error(format!("service {}: {}", name, msg));
    }
    svc.tenant = Some(tenant.clone());
    svc.tenant_named = tenant.is_some();
    match svc.call_closure(&c, args.into_iter().map(Some).collect(), line) {
        Ok(v) | Err(Ctrl::Return(v)) => Ok(match v {
            Value::Response(_) => Value::err(Value::str(format!("service {}: `{}` answered with a page, not a value", name, f))),
            v => crossed(&v),
        }),
        Err(Ctrl::Reply(_)) => error(format!("service {}: `{}` replied with a page, not a value", name, f)),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: service {}: error in `{}`: {}", name, f, e);
            error(format!("service {}: internal error in `{}`", name, f))
        }
    }
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .redirects(0)
            .user_agent(concat!("quillo/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The signature of a call: the time, the function, the tenant and the
/// arguments, under the shared secret.
fn signature(secret: &str, t: u64, f: &str, tenant: &str, body: &[u8]) -> String {
    let mut msg = format!("{}\n{}\n{}\n", t, f, tenant).into_bytes();
    msg.extend_from_slice(body);
    crate::crypto::hex(&crate::crypto::hmac_sha256(secret.as_bytes(), &msg))
}

fn remote(it: &mut Interp, url: &str, name: &str, f: &str, args: Vec<Value>, tenant: Option<Arc<str>>) -> R {
    let Some(secret) = secret() else {
        return error(format!("service {}: runs at {}, but QUILLO_SERVICE_SECRET is not set to sign the call", name, url));
    };
    let body = serde_json::Value::Array(args.iter().map(|a| a.to_json()).collect()).to_string().into_bytes();
    let tenant = tenant.map(|t| t.to_string()).unwrap_or_default();
    let t = now_secs();
    let sig = signature(&secret, t, f, &tenant, &body);
    let endpoint = format!("{}/_quillo/service/{}", url, f);
    let caller = it.base.clone();
    let res = it.unlocked(move || -> Result<(u16, String), String> {
        let mut rq = agent()
            .post(&endpoint)
            .set("content-type", "application/json")
            .set("x-quillo-service", &format!("t={},sig={}", t, sig))
            .set("x-quillo-caller", if caller.is_empty() { "/" } else { &caller });
        if !tenant.is_empty() {
            rq = rq.set("x-quillo-tenant", &tenant);
        }
        let resp = match rq.send_bytes(&body) {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let mut text = String::new();
        resp.into_reader().take(64 << 20).read_to_string(&mut text).map_err(|e| e.to_string())?;
        Ok((status, text))
    });
    match res {
        Err(e) => error(format!("service {}: unreachable at {} ({})", name, url, e)),
        Ok((200, text)) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(j) if j.get("ok").is_some() => Ok(Value::from_json(&j["ok"])),
            Ok(j) if j.get("err").is_some() => Ok(Value::err(Value::from_json(&j["err"]))),
            _ => error(format!("service {}: an answer that is not one", name)),
        },
        Ok((status, text)) => {
            let why = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|j| j["error"].as_str().map(String::from)).unwrap_or(text);
            error(format!("service {}: {} {}", name, status, why.trim()))
        }
    }
}

// ---- answering -------------------------------------------------------------------------

fn json(status: u16, j: serde_json::Value) -> HttpResponse {
    HttpResponse { status, headers: vec![("content-type".into(), "application/json".into())], body: j.to_string().into_bytes() }
}

fn refused(status: u16, msg: &str) -> HttpResponse {
    json(status, serde_json::json!({ "error": msg }))
}

/// `POST /_quillo/service/<fn>`: another service calling one of this
/// program's server fns. Only a call signed with the shared secret, made
/// in the last few minutes, is answered; it runs in the tenant it names.
pub fn answer(it: &mut Interp, f: &str, req: &HttpRequest) -> HttpResponse {
    if req.method != "POST" {
        return refused(405, "a service call is a POST");
    }
    let Some(secret) = secret() else {
        return refused(403, "service calls need QUILLO_SERVICE_SECRET, on both sides");
    };
    let header = |name: &str| req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let sig = header("x-quillo-service").unwrap_or("");
    let t = sig.split(',').find_map(|p| p.strip_prefix("t=")).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let given = sig.split(',').find_map(|p| p.strip_prefix("sig=")).unwrap_or("");
    let tenant = header("x-quillo-tenant").unwrap_or("");
    let expected = signature(&secret, t, f, tenant, &req.body);
    if now_secs().abs_diff(t) > WINDOW || !crate::crypto::same(expected.as_bytes(), given.as_bytes()) {
        return refused(401, "not signed with this server's QUILLO_SERVICE_SECRET (or too old)");
    }
    if !tenant.is_empty() && !crate::site::tenant_name_ok(tenant) {
        return refused(400, "not a tenant's name");
    }
    let Some(Value::Func(c)) = lookup(&it.globals, f).filter(|_| it.server_fns.iter().any(|n| n == f)) else {
        return refused(404, &format!("no server fn `{}`", f));
    };
    let args = match serde_json::from_slice::<serde_json::Value>(&req.body) {
        Ok(serde_json::Value::Array(a)) => a.iter().map(Value::from_json).collect::<Vec<_>>(),
        _ => return refused(400, "the body must be a JSON array of arguments"),
    };
    if let Err(msg) = crate::server::check_args(&c.def, &args) {
        return refused(400, &msg);
    }
    it.tenant = Some(if tenant.is_empty() { None } else { Some(Arc::from(tenant)) });
    it.tenant_named = !tenant.is_empty();
    match it.call_closure(&c, args.into_iter().map(Some).collect(), 0) {
        Ok(v) | Err(Ctrl::Return(v)) => match v {
            Value::Response(_) => refused(500, &format!("`{}` answered with a page, not a value", f)),
            Value::Err(e) => json(200, serde_json::json!({ "err": e.to_json() })),
            v => json(200, serde_json::json!({ "ok": v.to_json() })),
        },
        Err(Ctrl::Reply(_)) => refused(500, &format!("`{}` replied with a page, not a value", f)),
        Err(Ctrl::Error(e)) => {
            eprintln!("quillo: error in server fn `{}` (called by a service): {}", f, e);
            refused(500, &format!("internal error in `{}`", f))
        }
    }
}

/// A request under a service's prefix, when the service runs elsewhere:
/// passed on to it, without the prefix (as a mounted program sees it).
pub fn pass_on(it: &mut Interp, url: &str, prefix: &str, req: &HttpRequest) -> HttpResponse {
    let rest = &req.url[prefix.len()..];
    let target = format!("{}{}", url, if rest.is_empty() || rest.starts_with('?') { format!("/{}", rest) } else { rest.to_string() });
    const HOP: &[&str] = &["host", "connection", "content-length", "transfer-encoding", "keep-alive", "upgrade", "te", "trailer", "proxy-authorization"];
    let headers: Vec<(String, String)> = req.headers.iter().filter(|(k, _)| !HOP.iter().any(|h| k.eq_ignore_ascii_case(h))).cloned().collect();
    let (method, body, remote) = (req.method.clone(), req.body.clone(), req.remote.clone());
    let host = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.clone()).unwrap_or_default();
    let prefix = prefix.to_string();
    let res = it.unlocked(move || -> Result<HttpResponse, String> {
        let mut rq = agent().request(&method, &target);
        for (k, v) in &headers {
            rq = rq.set(k, v);
        }
        rq = rq.set("x-forwarded-for", &remote).set("x-forwarded-host", &host).set("x-forwarded-prefix", &prefix);
        let resp = match if body.is_empty() { rq.call() } else { rq.send_bytes(&body) } {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let headers = resp
            .headers_names()
            .into_iter()
            .filter(|n| !HOP.iter().any(|h| n.eq_ignore_ascii_case(h)))
            .flat_map(|n| resp.all(&n).into_iter().map(|v| (n.to_ascii_lowercase(), v.to_string())).collect::<Vec<_>>())
            .collect();
        let mut bytes = Vec::new();
        resp.into_reader().take(256 << 20).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, headers, body: bytes })
    });
    res.unwrap_or_else(|e| HttpResponse {
        status: 502,
        headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
        body: format!("this part of the site is not answering ({})\n", e).into_bytes(),
    })
}

// ---- loading -------------------------------------------------------------------------------

/// `service name [/prefix] = "file.qwe"`, run by the program declaring it:
/// the name stands for the service's functions; the service is loaded here
/// (once per process) unless it runs elsewhere; under its prefix, its
/// routes are served (here, or passed on to it).
pub fn declare(it: &mut Interp, name: &str, prefix: Option<&str>, file: &str, line: u32) -> R {
    let full = it.base_dir.join(file);
    let defs = offered(&full).map_err(|e| Ctrl::Error(crate::error::QuilloError::new(e, line)))?;
    let ns = namespace(it, name, &defs, line)?;
    if let Some(url) = url_of(&it.shared, name) {
        if let Some(p) = prefix {
            it.remote_mounts.retain(|(q, _)| q != p);
            it.remote_mounts.push((p.to_string(), url));
        }
    } else {
        let key = full.canonicalize().unwrap_or_else(|_| full.clone());
        let existing = it.shared.services.lock().unwrap_or_else(|e| e.into_inner()).get(name).map(|(p, s)| (p.clone(), s.fork()));
        let svc = match existing {
            Some((p, s)) if p == key => Some(s),
            Some((p, _)) => return fail(line, format!("service `{}` is {} elsewhere in this app, and {} here", name, p.display(), key.display())),
            None if it.shared.services_loading.lock().unwrap_or_else(|e| e.into_inner()).contains(name) => None,
            None => {
                it.shared.services_loading.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string());
                let dir = full.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| it.base_dir.to_path_buf());
                let mut sub = Interp::with_shared(dir, it.shared.clone());
                let loaded = sub.load_file(&full).map_err(|e| Ctrl::Error(e.in_file(&full.display().to_string())));
                it.shared.services_loading.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
                loaded?;
                it.shared.services.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), (key, sub.fork()));
                Some(sub)
            }
        };
        if let Some(svc) = svc {
            Arc::make_mut(&mut it.loaded).extend(svc.files());
            if let Some(p) = prefix {
                let mut m = svc;
                m.base = format!("{}{}", it.base, p);
                it.mounts.retain(|(q, _)| q != p);
                it.mounts.push((p.to_string(), m));
            }
        }
    }
    define(&it.globals, name, ns);
    Ok(Value::Nil)
}

// ---- running split ---------------------------------------------------------------------

/// The processes `--split` started: stopped with it.
pub struct Split {
    children: Vec<std::process::Child>,
}

impl Drop for Split {
    fn drop(&mut self) {
        for c in self.children.iter_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// `quillo run app.qwe --split`: each service the app declares (and those
/// they declare) runs as a process of its own, on a port of its own, its
/// log lines marked with its name; the app (and each service) is told
/// where the others are, and they sign their calls with one secret.
pub fn split(path: &Path, watch: bool, static_check: bool) -> Result<Split, String> {
    let services = declared(path)?;
    if std::env::var("QUILLO_SERVICE_SECRET").map_or(true, |s| s.is_empty()) {
        let mut b = [0u8; 24];
        for x in b.iter_mut() {
            *x = fastrand::u8(..);
        }
        std::env::set_var("QUILLO_SERVICE_SECRET", crate::crypto::hex(&b));
    }
    let mut ports = Vec::new();
    for (name, _, _) in &services {
        let port = free_port().map_err(|e| e.to_string())?;
        std::env::set_var(env_name(name), format!("http://127.0.0.1:{}", port));
        ports.push(port);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut split = Split { children: Vec::new() };
    for ((name, prefix, file), port) in services.iter().zip(&ports) {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("run").arg(file).args(["--port", &port.to_string()]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if let Some(p) = prefix {
            cmd.args(["--base", p]);
        }
        if watch {
            cmd.arg("--watch");
        }
        if !static_check {
            cmd.arg("--no-check");
        }
        let mut child = cmd.spawn().map_err(|e| format!("cannot start service {}: {}", name, e))?;
        for out in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
            let tag = name.clone();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    eprintln!("[{}] {}", tag, l);
                }
            });
        }
        eprintln!("quillo: service {} ({}) on http://127.0.0.1:{}{}", name, file.display(), port, prefix.as_ref().map_or(String::new(), |p| format!(", served under {}", p)));
        split.children.push(child);
    }
    // Ready when each answers.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    for ((name, _, _), port) in services.iter().zip(&ports) {
        while std::net::TcpStream::connect(("127.0.0.1", *port)).is_err() {
            if std::time::Instant::now() > deadline {
                return Err(format!("service {} did not start (see its lines above)", name));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(split)
}
