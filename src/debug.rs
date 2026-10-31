//! The debugger: breakpoints, stepping and looking at a running program's
//! variables, for editors, over the Debug Adapter Protocol (`quillo dap`).
//!
//! The interpreter calls in here before each statement, and as functions
//! start and end, only while a debugger is attached (`on`). A program that
//! stops keeps the interpreter lock: no other Quillo code runs until it goes
//! on, so what it shows stays as it is. While stopped, the thread that
//! stopped answers the editor's questions (the stack, variables, `evaluate`)
//! itself, since it holds what they are about; the protocol is read and
//! written on a thread of its own.

use crate::ast::*;
use crate::error::QuilloError;
use crate::interp::{Ctrl, Interp};
use crate::parser::Parser;
use crate::value::{scope_parent, scope_vars, Env, Value};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

/// Whether a debugger is attached: the interpreter calls in here only then.
static ON: AtomicBool = AtomicBool::new(false);
/// Whether anything can stop the program now (a breakpoint, a step, a pause
/// asked for): when not, statements only note where they are.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether a runtime error stops the program.
static ERRORS: AtomicBool = AtomicBool::new(true);

thread_local! {
    /// While the debugger runs code of the program's (a condition, a log
    /// message, `evaluate`): nothing in it stops.
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline(always)]
pub fn on() -> bool {
    ON.load(Relaxed)
}

/// A call in progress, as the debugger shows it.
pub(crate) struct Frame {
    name: Arc<str>,
    file: Option<Arc<str>>,
    line: u32,
    /// The scope of the statement it is at (none before its first).
    env: Option<Env>,
    /// The lines of the statements in progress, innermost last.
    running: Vec<u32>,
}

/// What an interpreter handle keeps for the debugger: its calls in
/// progress, outermost first.
#[derive(Default)]
pub(crate) struct Trace {
    frames: Vec<Frame>,
    /// The error being passed up has stopped the program already.
    error_seen: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Run,
    In,
    /// Over and out stop at the calls this deep or less (fewer for out).
    Over(usize),
    Out(usize),
}

struct Bp {
    id: u64,
    line: u32,
    condition: Option<String>,
    hit: Option<String>,
    log: Option<String>,
    hits: u64,
}

struct State {
    /// By file (an absolute path).
    breakpoints: HashMap<String, Vec<Bp>>,
    step: Step,
    pause: bool,
    stopped: bool,
}

enum Cmd {
    /// A question for the stopped thread, and where its answer goes.
    Ask(String, Json, Sender<Result<Json, String>>),
    Resume,
}

struct Debugger {
    state: Mutex<State>,
    cmds: Mutex<Receiver<Cmd>>,
    send_cmd: Mutex<Sender<Cmd>>,
    out: Mutex<Box<dyn Write + Send>>,
    seq: AtomicU64,
}

static D: OnceLock<Debugger> = OnceLock::new();
static NEXT_BP: AtomicU64 = AtomicU64::new(1);

fn dbg() -> &'static Debugger {
    D.get().expect("debugger not started")
}

fn state() -> std::sync::MutexGuard<'static, State> {
    dbg().state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn rearm(st: &State) {
    ACTIVE.store(st.pause || st.step != Step::Run || st.breakpoints.values().any(|b| !b.is_empty()), Relaxed);
}

// ---- where the interpreter calls in ---------------------------------------

/// The line a statement starts on, when stopping there makes sense:
/// declarations (functions, routes, types) are not stops.
pub fn stmt_line(s: &Stmt) -> Option<u32> {
    let l = match s {
        Stmt::Let(_, _, l) | Stmt::Assign(_, _, _, l) | Stmt::SetField(_, _, _, _, l) | Stmt::State(_, _, l) | Stmt::Use(_, _, l, _) => *l,
        Stmt::Expr(e) | Stmt::Return(Some(e), _) => expr_line(e),
        Stmt::Return(None, _) | Stmt::Fn(_) | Stmt::Route(_) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) => 0,
    };
    (l > 0).then_some(l)
}

/// The line an expression starts on, as far as the parser recorded (0 when
/// it did not).
pub fn expr_line(e: &Expr) -> u32 {
    match e {
        Expr::Ident(_, l)
        | Expr::Unary(_, _, l)
        | Expr::Pipe(_, _, l)
        | Expr::Call(_, _, l)
        | Expr::Field(_, _, l)
        | Expr::Index(_, _, l)
        | Expr::Match(_, _, l)
        | Expr::For(_, _, _, l)
        | Expr::While(_, _, l)
        | Expr::Reply(_, _, l)
        | Expr::Html(_, l) => *l,
        // An operator's line is where the operator is: the left side may start earlier.
        Expr::Binary(_, a, _, l) => match expr_line(a) {
            0 => *l,
            a => a.min(*l),
        },
        Expr::Coalesce(a, _) | Expr::Try(a) | Expr::If(a, _, _) => expr_line(a),
        Expr::List(xs) => xs.first().map_or(0, expr_line),
        Expr::Map(es) => es.first().map_or(0, |(_, e)| expr_line(e)),
        Expr::Template(parts) => parts
            .iter()
            .find_map(|p| match p {
                TPart::Expr(e) => Some(expr_line(e)).filter(|&l| l > 0),
                TPart::Lit(_) => None,
            })
            .unwrap_or(0),
        Expr::Block(stmts) => stmts.first().and_then(stmt_line).unwrap_or(0),
        Expr::Lambda(def) => def.line,
        Expr::Lit(_) => 0,
    }
}

/// Before statement `s`: notes where the program is, and stops if it should.
/// Then runs it, stopping on the error it fails with.
#[cold]
#[inline(never)]
pub(crate) fn stmt(it: &mut Interp, s: &Stmt, env: &Env) -> Result<Value, Ctrl> {
    around(it, s, env, |it| it.run_stmt(s, env))
}

/// `run` as statement `s` of the program: a stop before it, and its line among
/// those in progress. A block in markup writes its last statement to the page
/// instead of running it (FEEDBACK16.md, Q-148; branch feedback20-fixes).
pub(crate) fn around<T>(it: &mut Interp, s: &Stmt, env: &Env, run: impl FnOnce(&mut Interp) -> Result<T, Ctrl>) -> Result<T, Ctrl> {
    let line = stmt_line(s);
    let frame = it.trace.frames.len().checked_sub(1);
    if let Some(line) = line {
        at(it, line, env);
        if let Some(f) = frame.and_then(|i| it.trace.frames.get_mut(i)) {
            f.running.push(line);
        }
    }
    let r = run(it);
    if line.is_some() {
        if let Some(f) = frame.and_then(|i| it.trace.frames.get_mut(i)) {
            f.running.pop();
        }
    }
    if let Err(Ctrl::Error(e)) = &r {
        failed(it, e, env);
    }
    r
}

/// A function's body that is one expression, not a block: a stop of its own.
pub(crate) fn body(it: &mut Interp, body: &Expr, env: &Env) {
    if !matches!(body, Expr::Block(_)) {
        let line = expr_line(body);
        if line > 0 {
            at(it, line, env);
        }
    }
}

/// A call starts: a frame for it.
pub(crate) fn enter(it: &mut Interp, name: &str, file: Option<Arc<str>>, line: u32) {
    it.trace.frames.push(Frame { name: name.into(), file, line, env: None, running: Vec::new() });
}

pub(crate) fn leave(it: &mut Interp) {
    it.trace.frames.pop();
}

impl Interp {
    /// A route's body, in a frame of its own.
    pub(crate) fn debug_route(&mut self, r: &RouteDef, env: &Env) -> Result<Value, Ctrl> {
        enter(self, &route_name(r), r.file.clone(), r.line);
        body(self, &r.body, env);
        let v = self.eval(&r.body, env);
        if let Err(Ctrl::Error(e)) = &v {
            failed(self, e, env);
        }
        leave(self);
        v
    }
}

/// The name a route's frame goes by: `GET /notes/:id`.
pub fn route_name(r: &RouteDef) -> String {
    let mut s = String::new();
    for seg in &r.path {
        s.push('/');
        match seg {
            Seg::Lit(l) => s.push_str(l),
            Seg::Param(n, _) => {
                s.push(':');
                s.push_str(n);
            }
            Seg::Rest(n) => {
                s.push('*');
                s.push_str(n);
            }
        }
    }
    format!("{} {}", r.method, if s.is_empty() { "/" } else { &s })
}

fn at(it: &mut Interp, line: u32, env: &Env) {
    // Code the debugger runs (a condition, `evaluate`) leaves the frames as they are.
    if QUIET.with(|q| q.get()) {
        return;
    }
    it.trace.error_seen = false;
    let depth = it.trace.frames.len();
    let (same_line, file) = match it.trace.frames.last_mut() {
        Some(f) => {
            // A statement inside one on the same line (`if x { y = 1 }`, a
            // loop on one line) is not a stop of its own.
            let same = f.running.last() == Some(&line);
            f.line = line;
            f.env = Some(env.clone());
            (same, f.file.clone())
        }
        None => (false, None),
    };
    if !ACTIVE.load(Relaxed) || same_line {
        return;
    }
    let mut st = state();
    let stepped = st.pause
        || match st.step {
            Step::Run => false,
            Step::In => true,
            Step::Over(d) => depth <= d,
            Step::Out(d) => depth < d,
        };
    if stepped {
        let reason = if st.pause { "pause" } else { "step" };
        drop(st);
        stop(it, reason, None, vec![]);
        return;
    }
    let Some(file) = file else { return };
    let Some(bps) = st.breakpoints.get_mut(&*file) else { return };
    let Some(bp) = bps.iter_mut().find(|b| b.line == line) else { return };
    let (id, condition, hit, log) = (bp.id, bp.condition.clone(), bp.hit.clone(), bp.log.clone());
    drop(st);
    if let Some(c) = condition {
        // A condition that fails to run stops, so the mistake is seen.
        match quietly(it, &c, env) {
            Ok(v) if !v.truthy() => return,
            _ => {}
        }
    }
    let hits = {
        let mut st = state();
        match st.breakpoints.get_mut(&*file).and_then(|bs| bs.iter_mut().find(|b| b.id == id)) {
            Some(b) => {
                b.hits += 1;
                b.hits
            }
            None => return,
        }
    };
    if let Some(h) = hit {
        if !hit_matches(&h, hits) {
            return;
        }
    }
    if let Some(msg) = log {
        let text = interpolate(it, &msg, env);
        output("console", &format!("{}\n", text));
        return;
    }
    stop(it, "breakpoint", None, vec![id]);
}

/// The error `e` is on its way out of the statement that failed: the
/// program stops there (once, not at each statement it passes on its way).
pub(crate) fn failed(it: &mut Interp, e: &QuilloError, env: &Env) {
    if !ERRORS.load(Relaxed) || it.trace.error_seen || QUIET.with(|q| q.get()) {
        return;
    }
    it.trace.error_seen = true;
    if let Some(f) = it.trace.frames.last_mut() {
        if e.line > 0 {
            f.line = e.line;
        }
        if f.env.is_none() {
            f.env = Some(env.clone());
        }
    }
    stop(it, "exception", Some(e.msg.clone()), vec![]);
}

/// `3`: from the third hit on; `>= 3`, `> 3`, `== 3`, `% 3` (every third).
fn hit_matches(h: &str, hits: u64) -> bool {
    let h = h.trim();
    let (op, n) = ["==", ">=", ">", "%", "="]
        .iter()
        .find_map(|op| h.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or((">=", h));
    let Ok(n) = n.trim().parse::<u64>() else { return true };
    match op {
        "==" | "=" => hits == n,
        ">" => hits > n,
        "%" => n > 0 && hits % n == 0,
        _ => hits >= n,
    }
}

/// A log message's `{expression}`s, run and put in its place.
fn interpolate(it: &mut Interp, msg: &str, env: &Env) -> String {
    let mut out = String::new();
    let mut rest = msg;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else {
            rest = &rest[i..];
            break;
        };
        let code = &rest[i + 1..i + j];
        match quietly(it, code, env) {
            Ok(v) => out.push_str(&v.display()),
            Err(e) => out.push_str(&format!("<{}>", e)),
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

/// Runs code typed into the debugger, in `env`, with nothing in it stopping.
fn quietly(it: &mut Interp, code: &str, env: &Env) -> Result<Value, String> {
    let stmts = Parser::new(code).parse_program().map_err(|e| e.msg)?;
    let was = QUIET.with(|q| q.replace(true));
    let mut last = Ok(Value::Nil);
    for s in &stmts {
        last = match it.exec_stmt(s, env) {
            Ok(v) | Err(Ctrl::Return(v)) => Ok(v),
            Err(Ctrl::Error(e)) => Err(e.msg),
            Err(Ctrl::Reply(_)) => Err("`reply` cannot be used here".into()),
        };
        if last.is_err() {
            break;
        }
    }
    QUIET.with(|q| q.set(was));
    last
}

// ---- stopped ----------------------------------------------------------------

/// What the editor can open while the program is stopped (variables'
/// `variablesReference`s, which last until it goes on).
enum Node {
    Locals(Env),
    Globals,
    Value(Value),
}

struct Stopped {
    nodes: Vec<Node>,
    error: Option<String>,
}

impl Stopped {
    fn node(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len()
    }
}

fn stop(it: &mut Interp, reason: &str, error: Option<String>, hit: Vec<u64>) {
    {
        let mut st = state();
        st.stopped = true;
        st.pause = false;
        st.step = Step::Run;
        rearm(&st);
    }
    let mut body = json!({ "reason": reason, "threadId": 1, "allThreadsStopped": true });
    if let Some(e) = &error {
        body["description"] = json!("Runtime error");
        body["text"] = json!(e);
    }
    if !hit.is_empty() {
        body["hitBreakpointIds"] = json!(hit);
    }
    event("stopped", body);
    let mut here = Stopped { nodes: Vec::new(), error };
    let rx = dbg().cmds.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Ask(command, args, reply) => {
                let _ = reply.send(answer(it, &mut here, &command, &args));
            }
            Cmd::Resume => break,
        }
    }
}

fn answer(it: &mut Interp, here: &mut Stopped, command: &str, args: &Json) -> Result<Json, String> {
    match command {
        "stackTrace" => {
            let frames: Vec<Json> = it
                .trace
                .frames
                .iter()
                .enumerate()
                .rev()
                .map(|(i, f)| {
                    let mut fr = json!({ "id": i + 1, "name": &*f.name, "line": f.line, "column": 1 });
                    if let Some(file) = &f.file {
                        let name = std::path::Path::new(&**file).file_name().map(|n| n.to_string_lossy().into_owned());
                        fr["source"] = json!({ "name": name, "path": &**file });
                    }
                    fr
                })
                .collect();
            Ok(json!({ "stackFrames": frames, "totalFrames": frames.len() }))
        }
        "scopes" => {
            let env = frame_env(it, args);
            let mut scopes = Vec::new();
            if let Some(env) = env.filter(|e| !Arc::ptr_eq(e, &it.globals)) {
                let r = here.node(Node::Locals(env));
                scopes.push(json!({ "name": "Locals", "presentationHint": "locals", "variablesReference": r, "expensive": false }));
            }
            let r = here.node(Node::Globals);
            scopes.push(json!({ "name": "Globals", "variablesReference": r, "expensive": false }));
            if let Some(req) = it.request_value() {
                let r = here.node(Node::Value(req));
                scopes.push(json!({ "name": "Request", "variablesReference": r, "expensive": false }));
            }
            Ok(json!({ "scopes": scopes }))
        }
        "variables" => {
            let r = args["variablesReference"].as_u64().unwrap_or(0) as usize;
            let vars: Vec<(String, Value)> = match here.nodes.get(r.wrapping_sub(1)) {
                Some(Node::Locals(env)) => locals(it, env),
                Some(Node::Globals) => {
                    let mut vs: Vec<_> = scope_vars(&it.globals).into_iter().filter(|(_, v)| user_global(v)).collect();
                    vs.sort_by(|a, b| a.0.cmp(&b.0));
                    vs
                }
                Some(Node::Value(v)) => children(v, args["start"].as_u64(), args["count"].as_u64()),
                None => vec![],
            };
            let vars: Vec<Json> = vars.into_iter().map(|(n, v)| variable(here, &n, &v)).collect();
            Ok(json!({ "variables": vars }))
        }
        "evaluate" => {
            let code = args["expression"].as_str().unwrap_or("");
            let env = frame_env(it, args).unwrap_or_else(|| it.globals.clone());
            // Hovering shows a variable or a field of one; it calls nothing.
            if args["context"] == "hover" {
                let plain = Parser::new(code).parse_expr().ok().filter(no_calls);
                if plain.is_none() {
                    return Err("not shown on hover".into());
                }
            }
            let v = quietly(it, code, &env)?;
            let mut var = variable(here, "", &v);
            let body = var.as_object_mut().unwrap();
            let result = body.remove("value").unwrap_or_default();
            body.insert("result".into(), result);
            Ok(var)
        }
        "exceptionInfo" => Ok(json!({
            "exceptionId": "runtime error",
            "description": here.error.clone().unwrap_or_default(),
            "breakMode": "always",
        })),
        _ => Err(format!("unknown request {}", command)),
    }
}

fn frame_env(it: &Interp, args: &Json) -> Option<Env> {
    let n = it.trace.frames.len();
    let i = args["frameId"].as_u64().map_or(n, |i| i as usize);
    it.trace.frames.get(i.checked_sub(1)?)?.env.clone()
}

fn no_calls(e: &Expr) -> bool {
    match e {
        Expr::Ident(..) | Expr::Lit(_) => true,
        Expr::Field(x, _, _) => no_calls(x),
        Expr::Index(x, i, _) => no_calls(x) && no_calls(i),
        _ => false,
    }
}

/// The variables a scope sees, short of the globals: the innermost of each
/// name.
fn locals(it: &Interp, env: &Env) -> Vec<(String, Value)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut cur = Some(env);
    while let Some(e) = cur {
        if Arc::ptr_eq(e, &it.globals) {
            break;
        }
        let mut vs = scope_vars(e);
        vs.sort_by(|a, b| a.0.cmp(&b.0));
        for (n, v) in vs {
            if seen.insert(n.clone()) {
                out.push((n, v));
            }
        }
        cur = scope_parent(e);
    }
    out
}

/// Globals the program made: not the builtins, the modules (`db`, `http`)
/// or functions.
fn user_global(v: &Value) -> bool {
    match v {
        Value::Builtin(..) | Value::Func(_) => false,
        Value::Map(m) => !m.values().any(|v| matches!(v, Value::Builtin(..))),
        _ => true,
    }
}

const PAGE: usize = 500;

fn children(v: &Value, start: Option<u64>, count: Option<u64>) -> Vec<(String, Value)> {
    match v {
        Value::List(xs) => {
            let start = start.unwrap_or(0) as usize;
            let count = count.map_or(xs.len(), |c| c as usize);
            xs.iter().enumerate().skip(start).take(count).map(|(i, x)| (i.to_string(), x.clone())).collect()
        }
        Value::Map(m) => m.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        Value::Err(x, _, _) => vec![("error".into(), (**x).clone())],
        Value::Response(r) => {
            let mut out = vec![("status".to_string(), Value::Int(r.status as i64))];
            let headers = r.headers.iter().map(|(k, v)| Value::list(vec![Value::str(k), Value::str(v)])).collect();
            out.push(("headers".into(), Value::list(headers)));
            if let crate::value::Body::Value(b) = &r.body {
                out.push(("body".into(), b.clone()));
            }
            out
        }
        _ => vec![],
    }
}

fn variable(here: &mut Stopped, name: &str, v: &Value) -> Json {
    let mut var = json!({ "name": name, "value": preview(v, 120), "type": type_text(v) });
    let r = match v {
        Value::List(xs) if !xs.is_empty() => {
            if xs.len() > PAGE {
                var["indexedVariables"] = json!(xs.len());
            }
            here.node(Node::Value(v.clone()))
        }
        Value::Map(m) if !m.is_empty() => here.node(Node::Value(v.clone())),
        Value::Err(..) | Value::Response(_) => here.node(Node::Value(v.clone())),
        _ => 0,
    };
    var["variablesReference"] = json!(r);
    var
}

fn type_text(v: &Value) -> String {
    match v {
        Value::Func(c) => match c.def.name.as_str() {
            "<fn>" => "fn".into(),
            n => format!("fn {}", n),
        },
        v => v.type_name().to_string(),
    }
}

/// A value in a line: lists and maps show what they start with.
fn preview(v: &Value, budget: usize) -> String {
    let mut s = String::new();
    write_preview(v, budget, &mut s, true);
    s
}

fn write_preview(v: &Value, budget: usize, out: &mut String, top: bool) {
    let full = |out: &mut String| out.len() >= budget;
    match v {
        Value::List(xs) => {
            if top {
                out.push_str(&format!("({}) ", xs.len()));
            }
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if full(out) {
                    out.push('…');
                    break;
                }
                if i > 0 {
                    out.push_str(", ");
                }
                write_preview(x, budget, out, false);
            }
            out.push(']');
        }
        Value::Map(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if full(out) {
                    out.push('…');
                    break;
                }
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(k);
                out.push_str(": ");
                write_preview(x, budget, out, false);
            }
            out.push('}');
        }
        Value::Func(c) => out.push_str(&match c.def.name.as_str() {
            "<fn>" => "(…) => …".to_string(),
            n => format!("fn {}(…)", n),
        }),
        Value::Builtin(n, _) => out.push_str(&format!("builtin {}", n)),
        Value::Response(r) => out.push_str(&format!("response {}", r.status)),
        Value::Html(h) => {
            out.push_str("html ");
            let left = budget.saturating_sub(out.len()).max(20);
            push_cut(out, &format!("{:?}", &**h), left);
        }
        Value::Err(x, _, _) => {
            out.push_str("err(");
            write_preview(x, budget, out, false);
            out.push(')');
        }
        other => {
            let left = budget.saturating_sub(out.len()).max(20);
            push_cut(out, &other.repr(), left);
        }
    }
}

fn push_cut(out: &mut String, s: &str, max: usize) {
    if s.chars().count() > max {
        out.extend(s.chars().take(max));
        out.push('…');
    } else {
        out.push_str(s);
    }
}

// ---- where breakpoints can go --------------------------------------------

/// The lines of `src` where the program can stop, and where each top-level
/// item starts: a breakpoint on a line with nothing to stop at moves to the
/// next line that has, in the same item.
fn stops_in(src: &str) -> (Vec<u32>, Vec<u32>) {
    let Ok(stmts) = Parser::new(src).parse_program() else { return (vec![], vec![]) };
    let mut lines = Vec::new();
    let mut items = Vec::new();
    for s in &stmts {
        let start = match s {
            Stmt::Fn(d) => d.line,
            Stmt::Route(r) => r.line,
            Stmt::Type(t) => t.line,
            Stmt::Mount(_, _, l) | Stmt::Service(_, _, _, _, l) => *l,
            s => stmt_line(s).unwrap_or(0),
        };
        if start > 0 {
            items.push(start);
        }
        walk_stmt(s, &mut lines);
    }
    lines.sort_unstable();
    lines.dedup();
    items.sort_unstable();
    (lines, items)
}

fn walk_stmt(s: &Stmt, out: &mut Vec<u32>) {
    if let Some(l) = stmt_line(s) {
        out.push(l);
    }
    match s {
        Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::State(_, e, _) | Stmt::Expr(e) | Stmt::Return(Some(e), _) => walk(e, out),
        Stmt::SetField(o, _, _, e, _) => {
            walk(o, out);
            walk(e, out)
        }
        Stmt::Fn(d) => walk_body(&d.body, out),
        Stmt::Route(r) => walk_body(&r.body, out),
        Stmt::Service(_, _, _, Some(e), _) => walk(e, out),
        _ => {}
    }
}

fn walk_body(body: &Expr, out: &mut Vec<u32>) {
    if !matches!(body, Expr::Block(_)) {
        let l = expr_line(body);
        if l > 0 {
            out.push(l);
        }
    }
    walk(body, out);
}

fn walk(e: &Expr, out: &mut Vec<u32>) {
    match e {
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => {
            for p in parts {
                if let TPart::Expr(e) = p {
                    walk(e, out);
                }
            }
        }
        Expr::List(xs) => xs.iter().for_each(|x| walk(x, out)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| walk(x, out)),
        Expr::Unary(_, x, _) | Expr::Field(x, _, _) | Expr::Try(x) => walk(x, out),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::Call(f, args, _) => {
            walk(f, out);
            args.iter().for_each(|x| walk(x, out));
        }
        Expr::If(c, a, b) => {
            walk(c, out);
            walk(a, out);
            if let Some(b) = b {
                walk(b, out);
            }
        }
        Expr::Match(s, arms, _) => {
            walk(s, out);
            for a in arms {
                if let Some(g) = &a.guard {
                    walk(g, out);
                }
                walk(&a.body, out);
            }
        }
        Expr::For(_, it, body, _) | Expr::While(it, body, _) => {
            walk(it, out);
            walk(body, out);
        }
        Expr::Block(stmts) => stmts.iter().for_each(|s| walk_stmt(s, out)),
        Expr::Lambda(d) => walk_body(&d.body, out),
        Expr::Reply(s, b, _) => {
            walk(s, out);
            if let Some(b) = b {
                walk(b, out);
            }
        }
        Expr::Html(el, _) => walk_el(el, out),
    }
}

fn walk_el(el: &HtmlEl, out: &mut Vec<u32>) {
    if let Some(id) = &el.id {
        walk(id, out);
    }
    el.classes.iter().for_each(|c| walk(c, out));
    for (_, a) in &el.attrs {
        if let AttrVal::Expr(e) | AttrVal::Spread(e) = a {
            walk(e, out);
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(el) => walk_el(el, out),
            HtmlChild::Expr(e) => walk(e, out),
        }
    }
}

/// Where a breakpoint asked for on `line` goes: that line, or the next one
/// in the same item with something to stop at.
fn place(lines: &[u32], items: &[u32], line: u32) -> Option<u32> {
    let next_item = items.iter().copied().find(|&s| s > line).unwrap_or(u32::MAX);
    lines.iter().copied().find(|&l| l >= line && l < next_item)
}

// ---- the protocol --------------------------------------------------------------

fn send(msg: Json) {
    let Some(d) = D.get() else { return };
    let text = msg.to_string();
    let mut out = d.out.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", text.len(), text);
    let _ = out.flush();
}

fn next_seq() -> u64 {
    dbg().seq.fetch_add(1, Relaxed)
}

fn event(name: &str, body: Json) {
    send(json!({ "seq": next_seq(), "type": "event", "event": name, "body": body }));
}

fn output(category: &str, text: &str) {
    event("output", json!({ "category": category, "output": text }));
}

fn respond(req: &Json, body: Result<Json, String>) {
    let mut msg = json!({
        "seq": next_seq(),
        "type": "response",
        "request_seq": req["seq"],
        "command": req["command"],
        "success": body.is_ok(),
    });
    match body {
        Ok(b) => msg["body"] = b,
        Err(e) => {
            msg["message"] = json!(e);
            msg["body"] = json!({ "error": { "id": 1, "format": e } });
        }
    }
    send(msg);
}

fn read_message(r: &mut impl BufRead) -> Option<Json> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse::<usize>().ok();
        }
    }
    let mut buf = vec![0; len?];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

/// Asks the stopped program; an error when it is running.
fn ask(command: &str, args: &Json) -> Result<Json, String> {
    if !state().stopped {
        return Err("the program is running".into());
    }
    let (tx, rx) = channel();
    let _ = dbg().send_cmd.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(Cmd::Ask(command.into(), args.clone(), tx));
    rx.recv().unwrap_or_else(|_| Err("the program ended".into()))
}

fn resume(step: Step) {
    let mut st = state();
    st.step = step;
    let was = std::mem::replace(&mut st.stopped, false);
    rearm(&st);
    drop(st);
    if was {
        let _ = dbg().send_cmd.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(Cmd::Resume);
    }
}

/// The depth of the stopped program's stack, for stepping over and out.
fn depth() -> usize {
    ask("stackTrace", &json!({})).ok().and_then(|b| b["totalFrames"].as_u64()).unwrap_or(0) as usize
}

/// What `launch` was asked to run.
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub debug: bool,
}

/// `quillo dap`: a debug adapter on stdin and stdout. `start` runs the
/// program (as `quillo run` would) once the editor has set its breakpoints,
/// and gives its exit code.
pub fn serve(start: impl FnOnce(Launch) -> i32 + Send + 'static) -> i32 {
    let out = match protocol_out() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("quillo dap: {}", e);
            return 1;
        }
    };
    let (tx, rx) = channel();
    let _ = D.set(Debugger {
        state: Mutex::new(State { breakpoints: HashMap::new(), step: Step::Run, pause: false, stopped: false }),
        cmds: Mutex::new(rx),
        send_cmd: Mutex::new(tx),
        out: Mutex::new(out),
        seq: AtomicU64::new(1),
    });
    let mut start = Some(start);
    let mut launch: Option<Launch> = None;
    let mut stdin = BufReader::new(std::io::stdin());
    while let Some(req) = read_message(&mut stdin) {
        let args = &req["arguments"];
        let command = req["command"].as_str().unwrap_or("");
        match command {
            "initialize" => {
                respond(
                    &req,
                    Ok(json!({
                        "supportsConfigurationDoneRequest": true,
                        "supportsConditionalBreakpoints": true,
                        "supportsHitConditionalBreakpoints": true,
                        "supportsLogPoints": true,
                        "supportsEvaluateForHovers": true,
                        "supportsExceptionInfoRequest": true,
                        "supportsTerminateRequest": true,
                        "exceptionBreakpointFilters": [
                            { "filter": "errors", "label": "Runtime errors", "default": true,
                              "description": "Stop where a runtime error happens (one that makes a route answer 500)" }
                        ],
                    })),
                );
                event("initialized", json!({}));
            }
            "launch" => {
                let program = args["program"].as_str().unwrap_or("").to_string();
                if program.is_empty() {
                    respond(&req, Err("no program to debug: set \"program\" in launch.json".into()));
                    continue;
                }
                if let Some(cwd) = args["cwd"].as_str().filter(|c| !c.is_empty()) {
                    if let Err(e) = std::env::set_current_dir(cwd) {
                        respond(&req, Err(format!("cannot use {} as the folder to run in: {}", cwd, e)));
                        continue;
                    }
                }
                let mut run_args: Vec<String> =
                    args["args"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                let debug = !args["noDebug"].as_bool().unwrap_or(false);
                // Cached views would not run their code, so their breakpoints would not stop.
                if debug && !run_args.iter().any(|a| a == "--no-view-cache") {
                    run_args.push("--no-view-cache".into());
                }
                if args["stopOnEntry"].as_bool().unwrap_or(false) {
                    let mut st = state();
                    st.step = Step::In;
                    rearm(&st);
                }
                launch = Some(Launch { program, args: run_args, debug });
                respond(&req, Ok(json!({})));
            }
            "attach" => respond(&req, Err("attaching to a running program is not supported yet: use a \"launch\" configuration".into())),
            "setBreakpoints" => {
                let file = args["source"]["path"].as_str().unwrap_or("").to_string();
                let file = std::fs::canonicalize(&file).map(|p| p.display().to_string()).unwrap_or(file);
                let (lines, items) = std::fs::read_to_string(&file).map(|s| stops_in(&s)).unwrap_or_default();
                let mut bps = Vec::new();
                let mut shown = Vec::new();
                for b in args["breakpoints"].as_array().into_iter().flatten() {
                    let want = b["line"].as_u64().unwrap_or(0) as u32;
                    let id = NEXT_BP.fetch_add(1, Relaxed);
                    match place(&lines, &items, want) {
                        Some(line) => {
                            let text = |k: &str| b[k].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
                            bps.push(Bp { id, line, condition: text("condition"), hit: text("hitCondition"), log: text("logMessage"), hits: 0 });
                            shown.push(json!({ "id": id, "verified": true, "line": line }));
                        }
                        None => shown.push(json!({ "id": id, "verified": false, "line": want, "message": "no code to stop at here" })),
                    }
                }
                let mut st = state();
                st.breakpoints.insert(file, bps);
                rearm(&st);
                drop(st);
                respond(&req, Ok(json!({ "breakpoints": shown })));
            }
            "setExceptionBreakpoints" => {
                let on = args["filters"].as_array().map_or(false, |f| f.iter().any(|x| x == "errors"));
                ERRORS.store(on, Relaxed);
                respond(&req, Ok(json!({})));
            }
            "configurationDone" => {
                respond(&req, Ok(json!({})));
                match (launch.take(), start.take()) {
                    (Some(l), Some(start)) => run_program(l, start),
                    _ => output("stderr", "quillo dap: nothing was launched\n"),
                }
            }
            "threads" => respond(&req, Ok(json!({ "threads": [{ "id": 1, "name": "Quillo" }] }))),
            "stackTrace" => {
                let r = ask("stackTrace", args).or_else(|_| Ok(json!({ "stackFrames": [], "totalFrames": 0 })));
                respond(&req, r)
            }
            "scopes" | "variables" | "evaluate" | "exceptionInfo" => respond(&req, ask(command, args)),
            "continue" => {
                resume(Step::Run);
                respond(&req, Ok(json!({ "allThreadsContinued": true })));
            }
            "next" | "stepIn" | "stepOut" => {
                let d = depth();
                resume(match command {
                    "next" => Step::Over(d),
                    "stepOut" => Step::Out(d),
                    _ => Step::In,
                });
                respond(&req, Ok(json!({})));
            }
            "pause" => {
                let mut st = state();
                st.pause = true;
                rearm(&st);
                drop(st);
                respond(&req, Ok(json!({})));
            }
            "disconnect" | "terminate" => {
                respond(&req, Ok(json!({})));
                if command == "terminate" {
                    event("terminated", json!({}));
                }
                std::process::exit(0);
            }
            _ => respond(&req, Err(format!("`{}` is not supported", command))),
        }
    }
    0
}

fn run_program(l: Launch, start: impl FnOnce(Launch) -> i32 + Send + 'static) {
    ON.store(l.debug, Relaxed);
    if let Err(e) = capture_output() {
        output("stderr", &format!("quillo dap: the program's output is not shown here: {}\n", e));
    }
    let spawned = std::thread::Builder::new().stack_size(crate::server::WORKER_STACK).spawn(move || {
        let code = start(l);
        event("exited", json!({ "exitCode": code }));
        event("terminated", json!({}));
    });
    if let Err(e) = spawned {
        output("stderr", &format!("quillo dap: cannot start the program: {}\n", e));
        event("terminated", json!({}));
    }
}

/// Where the protocol is written: what was stdout, before the program's own
/// output (`print`, the request log) is sent to the editor instead.
#[cfg(unix)]
fn protocol_out() -> Result<Box<dyn Write + Send>, String> {
    use std::os::fd::FromRawFd;
    let _ = std::io::stdout().flush();
    let fd = unsafe { libc::dup(1) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // Until the program starts, anything printed goes to stderr, not into the protocol.
    unsafe { libc::dup2(2, 1) };
    Ok(Box::new(unsafe { std::fs::File::from_raw_fd(fd) }))
}

#[cfg(not(unix))]
fn protocol_out() -> Result<Box<dyn Write + Send>, String> {
    Err("debugging is not supported on this system yet".into())
}

/// Sends what the program writes to stdout and stderr to the editor's debug
/// console, as `output` events.
#[cfg(unix)]
fn capture_output() -> Result<(), String> {
    use std::os::fd::FromRawFd;
    for (fd, category) in [(1, "stdout"), (2, "stderr")] {
        let mut ends = [0; 2];
        if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        unsafe {
            libc::dup2(ends[1], fd);
            libc::close(ends[1]);
        }
        let read = unsafe { std::fs::File::from_raw_fd(ends[0]) };
        std::thread::Builder::new()
            .name(format!("quillo-dap-{}", category))
            .spawn(move || {
                let mut r = BufReader::new(read);
                let mut line = Vec::new();
                while r.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
                    output(category, &String::from_utf8_lossy(&line));
                    line.clear();
                }
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn capture_output() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoints_move_to_the_next_line_with_code_in_the_same_item() {
        let src = "fn add(a, b) {\n    let c = a + b\n\n    c\n}\n\nroute GET / = add(1, 2)\n";
        let (lines, items) = stops_in(src);
        assert_eq!(lines, vec![2, 4, 7]);
        assert_eq!(place(&lines, &items, 1), Some(2));
        assert_eq!(place(&lines, &items, 3), Some(4));
        assert_eq!(place(&lines, &items, 5), None);
        assert_eq!(place(&lines, &items, 7), Some(7));
    }

    #[test]
    fn hit_conditions() {
        assert!(!hit_matches("3", 2));
        assert!(hit_matches("3", 3));
        assert!(hit_matches("3", 4));
        assert!(hit_matches("== 3", 3) && !hit_matches("== 3", 4));
        assert!(hit_matches("% 2", 4) && !hit_matches("% 2", 3));
        assert!(hit_matches("> 1", 2) && !hit_matches("> 1", 1));
    }

    #[test]
    fn previews_are_short() {
        let v = Value::list((0..1000).map(Value::Int).collect());
        let p = preview(&v, 40);
        assert!(p.starts_with("(1000) [0, 1, 2"), "{}", p);
        assert!(p.len() < 60, "{}", p);
    }
}
