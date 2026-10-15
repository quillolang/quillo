//! Tree-walking evaluator: expressions, patterns and HTML rendering.

use crate::ast::*;
use crate::error::QuilloError;
use crate::parser::{Parser, VOID_TAGS};
use crate::shared::Shared;
use crate::value::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Non-local exits: runtime errors, `return` (and `?`), and `reply`.
pub enum Ctrl {
    Error(QuilloError),
    Return(Value),
    Reply(Arc<Response>),
}

impl From<QuilloError> for Ctrl {
    fn from(e: QuilloError) -> Self {
        Ctrl::Error(e)
    }
}

pub fn fail<T>(line: u32, msg: impl Into<String>) -> Result<T, Ctrl> {
    Err(Ctrl::Error(QuilloError::new(msg, line)))
}

fn undefined(name: &str, line: u32) -> Ctrl {
    Ctrl::Error(QuilloError::new(format!("unknown name `{}`", name), line))
}

/// How strictly a pattern compares values. Query strings, form fields and
/// headers arrive as text, so `Lenient` lets `int` match "42" (binding 42).
/// `Request` is lenient for every request field except `json`.
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Strict,
    Lenient,
    Request,
}

pub struct Interp {
    pub globals: Env,
    pub routes: Vec<Arc<RouteDef>>,
    pub base_dir: PathBuf,
    loaded: HashSet<PathBuf>,
    pub shared: Arc<Shared>,
    /// Calls in progress on this thread, to stop runaway recursion cleanly.
    depth: usize,
    /// Effects the running code may use, and the `with` function that limited them.
    allowed: u8,
    restrictor: Option<Arc<FnDef>>,
    /// Names of `server fn`s, callable from the browser.
    pub server_fns: Vec<String>,
    /// Every statement the program has loaded, for the browser compiler.
    pub program: Vec<Arc<Vec<Stmt>>>,
    /// The browser bundle (`/_quillo/quillo.js`), built after loading.
    pub client_js: Arc<String>,
    /// The request being handled, visible as `request` inside `server fn`s.
    pub current_request: Option<Value>,
    /// State of the components being rendered, innermost last, with the
    /// `live(...)` subscriptions of each.
    islands: Vec<(Map, Map)>,
    /// While a `live` loader runs: the topics (tables, globals) it reads.
    pub reads: Option<std::collections::HashSet<String>>,
    /// The subscription the last `live(...)` call set up, for `~state`.
    pub last_live: Option<Value>,
    /// Open `db.transaction`s, and changes to announce when they commit.
    pub tx_depth: usize,
    pub pending: Vec<String>,
    /// The Postgres connection this request's `db.transaction` holds.
    pub pg_tx: std::sync::Mutex<Option<crate::db::PgTx>>,
}

/// Deepest call nesting allowed before a "too much recursion" error.
pub const MAX_DEPTH: usize = 10_000;

pub fn lit_value(l: &Lit) -> Value {
    match l {
        Lit::Nil => Value::Nil,
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Int(n) => Value::Int(*n),
        Lit::Float(f) => Value::Float(*f),
        Lit::Str(s) => Value::Str(s.clone()),
    }
}

pub fn type_name_matches(t: &str, v: &Value) -> bool {
    match t {
        "any" => true,
        "int" => matches!(v, Value::Int(_)),
        "float" => matches!(v, Value::Float(_)),
        "num" => matches!(v, Value::Int(_) | Value::Float(_)),
        "str" => matches!(v, Value::Str(_)),
        "bool" => matches!(v, Value::Bool(_)),
        "list" => matches!(v, Value::List(_)),
        "map" => matches!(v, Value::Map(_)),
        "html" => matches!(v, Value::Html(_)),
        "fn" => matches!(v, Value::Func(_) | Value::Builtin(..)),
        "response" => matches!(v, Value::Response(_)),
        _ => false,
    }
}

pub fn type_matches(ty: &Ty, v: &Value) -> bool {
    (ty.optional && matches!(v, Value::Nil)) || ty.names.iter().any(|n| type_name_matches(n, v))
}

fn ty_text(ty: &Ty) -> String {
    ty.text()
}

/// Converts request text to the type a pattern asks for.
pub fn coerce(t: &str, s: &str) -> Option<Value> {
    let s = s.trim();
    match t {
        "int" => s.parse::<i64>().ok().map(Value::Int),
        "float" => s.parse::<f64>().ok().map(Value::Float),
        "num" => s.parse::<i64>().ok().map(Value::Int).or_else(|| s.parse::<f64>().ok().map(Value::Float)),
        "bool" => match s {
            "true" | "on" | "1" | "yes" => Some(Value::Bool(true)),
            "false" | "off" | "0" | "no" => Some(Value::Bool(false)),
            _ => None,
        },
        "str" | "any" => Some(Value::str(s)),
        _ => None,
    }
}

fn get_path(v: &Value, path: &[String]) -> Option<Value> {
    let mut cur = v.clone();
    for k in path {
        cur = match &cur {
            Value::Map(m) => m.get(k)?.clone(),
            _ => return None,
        };
    }
    Some(cur)
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn render_value(v: &Value, out: &mut String) {
    match v {
        Value::Html(h) => out.push_str(h),
        Value::Nil | Value::Bool(false) => {}
        Value::List(items) => {
            for i in items.iter() {
                render_value(i, out);
            }
        }
        other => out.push_str(&escape(&other.display())),
    }
}

/// Why a value cannot travel to the browser as JSON, if it cannot.
pub fn not_sendable(v: &Value) -> Option<&'static str> {
    match v {
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) => None,
        Value::List(l) => l.iter().find_map(not_sendable),
        Value::Map(m) => m.values().find_map(not_sendable),
        Value::Func(_) | Value::Builtin(..) => Some("a function"),
        Value::Html(_) => Some("html"),
        Value::Response(_) => Some("a response"),
        Value::Err(_) => Some("an error value"),
    }
}

fn short(v: &Value) -> String {
    let r = v.repr();
    if r.chars().count() > 40 {
        format!("{}…", r.chars().take(40).collect::<String>())
    } else {
        r
    }
}

pub fn binop(op: BinOp, a: Value, b: Value, line: u32) -> Result<Value, Ctrl> {
    use Value::*;
    let overflow = || Ctrl::Error(QuilloError::new("integer overflow", line));
    match op {
        BinOp::Add => match (a, b) {
            (Int(x), Int(y)) => x.checked_add(y).map(Int).ok_or_else(overflow),
            (Str(x), y) => Ok(Value::str(format!("{}{}", x, y.display()))),
            (x, Str(y)) => Ok(Value::str(format!("{}{}", x.display(), y))),
            (Html(x), Html(y)) => Ok(Value::html(format!("{}{}", x, y))),
            (List(x), List(y)) => {
                let mut v = Arc::unwrap_or_clone(x);
                v.extend(y.iter().cloned());
                Ok(Value::list(v))
            }
            (Map(x), Map(y)) => {
                let mut m = Arc::unwrap_or_clone(x);
                for (k, v) in y.iter() {
                    m.insert(k.clone(), v.clone());
                }
                Ok(Value::map(m))
            }
            (x, y) => match (x.as_f64(), y.as_f64()) {
                (Some(p), Some(q)) => Ok(Float(p + q)),
                _ => fail(line, format!("cannot add {} and {}", x.type_name(), y.type_name())),
            },
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            if let (Int(x), Int(y)) = (&a, &b) {
                let (x, y) = (*x, *y);
                if y == 0 && matches!(op, BinOp::Div | BinOp::Mod) {
                    return fail(line, "division by zero");
                }
                // `/` divides exactly, as in JavaScript: 6 / 2 is 3, 7 / 2 is 3.5.
                // (The browser cannot tell 2 from 2.0, so this is also the
                // only rule that gives the same answer on both sides.)
                if op == BinOp::Div && x.checked_rem(y) != Some(0) {
                    return Ok(Float(x as f64 / y as f64));
                }
                let r = match op {
                    BinOp::Sub => x.checked_sub(y),
                    BinOp::Mul => x.checked_mul(y),
                    BinOp::Div => x.checked_div(y),
                    _ => x.checked_rem(y),
                };
                return r.map(Int).ok_or_else(overflow);
            }
            let verb = match op {
                BinOp::Sub => "subtract",
                BinOp::Mul => "multiply",
                BinOp::Div => "divide",
                _ => "take the remainder of",
            };
            match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Ok(Float(match op {
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => x / y,
                    _ => x % y,
                })),
                _ => fail(line, format!("cannot {} {} and {}", verb, a.type_name(), b.type_name())),
            }
        }
        BinOp::Eq => Ok(Bool(equal(&a, &b))),
        BinOp::Ne => Ok(Bool(!equal(&a, &b))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let Some(ord) = compare(&a, &b) else {
                return fail(line, format!("cannot compare {} with {}", a.type_name(), b.type_name()));
            };
            Ok(Bool(match op {
                BinOp::Lt => ord.is_lt(),
                BinOp::Le => ord.is_le(),
                BinOp::Gt => ord.is_gt(),
                _ => ord.is_ge(),
            }))
        }
        BinOp::Range => match (a, b) {
            (Int(x), Int(y)) if y.saturating_sub(x) <= 10_000_000 => Ok(Value::list((x..y).map(Int).collect())),
            (Int(_), Int(_)) => fail(line, "range is too large"),
            _ => fail(line, "`..` needs two ints"),
        },
        BinOp::And | BinOp::Or => unreachable!("handled in eval"),
    }
}

impl Interp {
    /// A new interpreter. The calling thread holds its lock (see `shared`).
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self::with_shared(base_dir, Arc::new(Shared::new()))
    }

    /// A new interpreter using an existing lock, for reloading a program
    /// while other threads keep serving. The caller must hold the lock.
    pub fn with_shared(base_dir: impl Into<PathBuf>, shared: Arc<Shared>) -> Self {
        let globals = Scope::root();
        crate::builtins::install(&globals);
        Interp {
            globals,
            routes: Vec::new(),
            base_dir: base_dir.into(),
            loaded: HashSet::new(),
            shared,
            depth: 0,
            allowed: ALL_EFFECTS,
            restrictor: None,
            server_fns: Vec::new(),
            program: Vec::new(),
            client_js: Arc::new(String::new()),
            current_request: None,
            islands: Vec::new(),
            pg_tx: std::sync::Mutex::new(None),
            reads: None,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
        }
    }

    /// Another handle on the same program for a worker thread: globals,
    /// routes and the lock are shared.
    pub fn fork(&self) -> Interp {
        Interp {
            globals: self.globals.clone(),
            routes: self.routes.clone(),
            base_dir: self.base_dir.clone(),
            loaded: self.loaded.clone(),
            shared: self.shared.clone(),
            depth: 0,
            allowed: ALL_EFFECTS,
            restrictor: None,
            server_fns: self.server_fns.clone(),
            program: self.program.clone(),
            client_js: self.client_js.clone(),
            current_request: None,
            islands: Vec::new(),
            pg_tx: std::sync::Mutex::new(None),
            reads: None,
            last_live: None,
            tx_depth: 0,
            pending: Vec::new(),
        }
    }

    /// The source files this program has loaded (the main file and `use`d files).
    pub fn files(&self) -> Vec<PathBuf> {
        self.loaded.iter().cloned().collect()
    }

    /// Announces changes to `topics`, after the current transaction commits.
    pub fn publish(&mut self, topics: Vec<String>) {
        if self.tx_depth > 0 {
            self.pending.extend(topics);
        } else {
            self.shared.publish(&topics);
        }
    }

    /// Notes that the running `live` loader depends on `topic`.
    pub fn watch(&mut self, topic: String) {
        if let Some(r) = &mut self.reads {
            r.insert(topic);
        }
    }

    #[cold]
    fn note_global_read(&mut self, env: &Env, name: &str) {
        if let Some((v, true)) = lookup_global(env, name) {
            let module = matches!(&v, Value::Map(m) if !m.is_empty() && m.values().all(|x| matches!(x, Value::Builtin(..))));
            if !module && !matches!(v, Value::Func(_) | Value::Builtin(..)) {
                self.watch(format!("var:{}", name));
            }
        }
    }

    /// Runs `f` without holding the interpreter lock, so other requests can
    /// run while this one waits. `f` must not touch Quillo values.
    pub fn unlocked<T>(&mut self, f: impl FnOnce() -> T) -> T {
        self.shared.gil.release();
        let r = f();
        self.shared.gil.acquire();
        r
    }

    /// Parses and runs a program's top level, registering its routes.
    pub fn load_source(&mut self, src: &str, file: &str) -> Result<Value, QuilloError> {
        let stmts = Arc::new(Parser::new(src).parse_program().map_err(|e| e.in_file(file))?);
        self.program.push(stmts.clone());
        let env = self.globals.clone();
        let mut last = Value::Nil;
        for s in stmts.iter() {
            match self.exec_stmt(s, &env) {
                Ok(v) => last = v,
                Err(Ctrl::Error(e)) => return Err(e.in_file(file)),
                Err(Ctrl::Return(v)) => {
                    last = v;
                    break;
                }
                Err(Ctrl::Reply(_)) => return Err(QuilloError::new("`reply` used outside of a route", 0).in_file(file)),
            }
        }
        self.build_client().map_err(|e| e.in_file(file))?;
        Ok(last)
    }

    /// Compiles the program's components (and what they use) for the browser.
    pub fn build_client(&mut self) -> Result<(), QuilloError> {
        self.client_js = Arc::new(crate::client::bundle(self)?);
        Ok(())
    }

    pub fn load_file(&mut self, path: &Path) -> Result<Value, QuilloError> {
        let src = std::fs::read_to_string(path)
            .map_err(|e| QuilloError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
        if let Ok(p) = path.canonicalize() {
            self.loaded.insert(p);
        }
        self.load_source(&src, &path.display().to_string())
    }

    pub fn exec_stmt(&mut self, s: &Stmt, env: &Env) -> Result<Value, Ctrl> {
        match s {
            Stmt::Expr(e) => self.eval(e, env),
            Stmt::Let(p, e, line) => {
                let v = self.eval(e, env)?;
                let mut binds = Vec::new();
                if self.match_pat(p, &v, Mode::Strict, env, &mut binds)?.is_none() {
                    return fail(*line, format!("`let` pattern does not match {}", short(&v)));
                }
                for (n, v) in binds {
                    define(env, &n, v);
                }
                Ok(Value::Nil)
            }
            Stmt::Assign(name, op, e, line) => {
                let rhs = self.eval(e, env)?;
                let v = match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add | AssignOp::Sub => {
                        let old = lookup(env, name).ok_or_else(|| undefined(name, *line))?;
                        let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                        binop(bop, old, rhs, *line)?
                    }
                };
                match assign_global(env, name, v) {
                    None => fail(
                        *line,
                        format!("cannot assign to `{}`: it was never declared (use `let {} = …`)", name, name),
                    ),
                    Some(global) => {
                        if global {
                            self.publish(vec![format!("var:{}", name)]);
                        }
                        Ok(Value::Nil)
                    }
                }
            }
            Stmt::Fn(def) => {
                define(env, &def.name, Value::Func(Arc::new(Closure { def: def.clone(), env: env.clone() })));
                if def.server && !self.server_fns.contains(&def.name) {
                    self.server_fns.push(def.name.clone());
                }
                Ok(Value::Nil)
            }
            Stmt::Route(r) => {
                self.routes.push(r.clone());
                Ok(Value::Nil)
            }
            Stmt::Use(path, line) => {
                let full = self.base_dir.join(path);
                let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                if !self.loaded.insert(key) {
                    return Ok(Value::Nil);
                }
                let src = std::fs::read_to_string(&full)
                    .map_err(|e| QuilloError::new(format!("cannot read {}: {}", full.display(), e), *line))?;
                let fname = full.display().to_string();
                let stmts = Arc::new(Parser::new(&src).parse_program().map_err(|e| e.in_file(&fname))?);
                self.program.push(stmts.clone());
                let g = self.globals.clone();
                for s in stmts.iter() {
                    match self.exec_stmt(s, &g) {
                        Err(Ctrl::Error(e)) => return Err(Ctrl::Error(e.in_file(&fname))),
                        other => {
                            other?;
                        }
                    }
                }
                Ok(Value::Nil)
            }
            Stmt::State(name, e, _) => {
                self.last_live = None;
                let v = self.eval(e, env)?;
                let live = self.last_live.take();
                if let Some((state, subs)) = self.islands.last_mut() {
                    state.insert(name.clone(), v.clone());
                    if let Some(l) = live {
                        subs.insert(name.clone(), l);
                    }
                }
                define(env, name, v);
                Ok(Value::Nil)
            }
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Nil,
                };
                Err(Ctrl::Return(v))
            }
        }
    }

    /// Evaluates an expression. Only the cheap cases are handled inline; the
    /// rest live in separate functions so that each nested call uses little
    /// stack (deep recursion in Quillo is deep recursion here).
    pub fn eval(&mut self, e: &Expr, env: &Env) -> Result<Value, Ctrl> {
        match e {
            Expr::Lit(l) => Ok(lit_value(l)),
            Expr::Ident(n, line) => {
                if self.reads.is_some() {
                    self.note_global_read(env, n);
                }
                lookup(env, n).ok_or_else(|| undefined(n, *line))
            }
            // As in JavaScript: the operand that decided the result.
            Expr::Binary(BinOp::And, a, b, _) => {
                let x = self.eval(a, env)?;
                if !x.truthy() {
                    return Ok(x);
                }
                self.eval(b, env)
            }
            Expr::Binary(BinOp::Or, a, b, _) => {
                let x = self.eval(a, env)?;
                if x.truthy() {
                    return Ok(x);
                }
                self.eval(b, env)
            }
            Expr::Binary(op, a, b, line) => {
                let x = self.eval(a, env)?;
                let y = self.eval(b, env)?;
                binop(*op, x, y, *line)
            }
            Expr::Coalesce(a, b) => {
                let v = self.eval(a, env)?;
                if matches!(v, Value::Nil | Value::Err(_)) {
                    self.eval(b, env)
                } else {
                    Ok(v)
                }
            }
            Expr::Pipe(lhs, rhs, line) => self.eval_pipe(lhs, rhs, env, *line),
            Expr::Call(callee, args, line) => self.eval_call(callee, args, None, env, *line),
            Expr::Field(obj, name, line) => {
                let v = self.eval(obj, env)?;
                self.get_field(v, name, *line)
            }
            Expr::Try(x) => {
                let v = self.eval(x, env)?;
                if let Value::Err(_) = v {
                    Err(Ctrl::Return(v))
                } else {
                    Ok(v)
                }
            }
            Expr::If(c, a, b) => {
                if self.eval(c, env)?.truthy() {
                    self.eval(a, env)
                } else if let Some(b) = b {
                    self.eval(b, env)
                } else {
                    Ok(Value::Nil)
                }
            }
            Expr::Block(stmts) => self.eval_block(stmts, env),
            Expr::Lambda(def) => Ok(Value::Func(Arc::new(Closure { def: def.clone(), env: env.clone() }))),
            Expr::Template(parts) => self.eval_template(parts, env),
            Expr::List(items) => self.eval_list(items, env),
            Expr::Map(entries) => self.eval_map(entries, env),
            Expr::Unary(op, x, line) => self.eval_unary(*op, x, env, *line),
            Expr::Index(obj, idx, line) => self.eval_index(obj, idx, env, *line),
            Expr::Match(subj, arms, line) => self.eval_match(subj, arms, env, *line),
            Expr::For(pat, iter, body, line) => self.eval_for(pat, iter, body, env, *line),
            Expr::Reply(status, body, line) => self.eval_reply(status, body.as_deref(), env, *line),
            Expr::Html(el, _) => self.eval_html(el, env),
        }
    }

    #[inline(never)]
    fn eval_pipe(&mut self, lhs: &Expr, rhs: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(lhs, env)?;
        match rhs {
            Expr::Call(callee, args, l) => self.eval_call(callee, args, Some(v), env, *l),
            other => {
                let f = self.eval(other, env)?;
                self.call(&f, vec![v], line)
            }
        }
    }

    #[inline(never)]
    fn eval_block(&mut self, stmts: &[Stmt], env: &Env) -> Result<Value, Ctrl> {
        let scope = Scope::child(env);
        let mut last = Value::Nil;
        for s in stmts {
            last = self.exec_stmt(s, &scope)?;
        }
        Ok(last)
    }

    #[inline(never)]
    fn eval_template(&mut self, parts: &[TPart], env: &Env) -> Result<Value, Ctrl> {
        let mut s = String::new();
        for p in parts {
            match p {
                TPart::Lit(t) => s.push_str(t),
                TPart::Expr(x) => s.push_str(&self.eval(x, env)?.display()),
            }
        }
        Ok(Value::str(s))
    }

    #[inline(never)]
    fn eval_list(&mut self, items: &[Expr], env: &Env) -> Result<Value, Ctrl> {
        let mut v = Vec::with_capacity(items.len());
        for i in items {
            v.push(self.eval(i, env)?);
        }
        Ok(Value::list(v))
    }

    #[inline(never)]
    fn eval_map(&mut self, entries: &[(String, Expr)], env: &Env) -> Result<Value, Ctrl> {
        let mut m = Map::new();
        for (k, x) in entries {
            let v = self.eval(x, env)?;
            m.insert(k.clone(), v);
        }
        Ok(Value::map(m))
    }

    #[inline(never)]
    fn eval_unary(&mut self, op: UnOp, x: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(x, env)?;
        match op {
            UnOp::Not => Ok(Value::Bool(!v.truthy())),
            UnOp::Neg => match v {
                Value::Int(n) => {
                    n.checked_neg().map(Value::Int).ok_or_else(|| Ctrl::Error(QuilloError::new("integer overflow", line)))
                }
                Value::Float(f) => Ok(Value::Float(-f)),
                o => fail(line, format!("cannot negate {}", o.type_name())),
            },
        }
    }

    #[inline(never)]
    fn eval_index(&mut self, obj: &Expr, idx: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let o = self.eval(obj, env)?;
        let i = self.eval(idx, env)?;
        match (&o, &i) {
            (Value::List(l), Value::Int(n)) => {
                let n = if *n < 0 { l.len() as i64 + n } else { *n };
                Ok(if n >= 0 { l.get(n as usize).cloned().unwrap_or(Value::Nil) } else { Value::Nil })
            }
            (Value::Map(m), Value::Str(k)) => Ok(m.get(&**k).cloned().unwrap_or(Value::Nil)),
            (Value::Str(s), Value::Int(n)) => {
                let len = s.chars().count() as i64;
                let n = if *n < 0 { len + n } else { *n };
                Ok(if n >= 0 {
                    s.chars().nth(n as usize).map(|c| Value::str(c.to_string())).unwrap_or(Value::Nil)
                } else {
                    Value::Nil
                })
            }
            (Value::Nil, _) => Ok(Value::Nil),
            _ => fail(line, format!("cannot index {} with {}", o.type_name(), i.type_name())),
        }
    }

    #[inline(never)]
    fn eval_match(&mut self, subj: &Expr, arms: &[Arm], env: &Env, line: u32) -> Result<Value, Ctrl> {
        let v = self.eval(subj, env)?;
        for arm in arms {
            let mut binds = Vec::new();
            if self.match_pat(&arm.pat, &v, Mode::Strict, env, &mut binds)?.is_none() {
                continue;
            }
            let scope = Scope::child(env);
            for (n, x) in binds {
                define(&scope, &n, x);
            }
            if let Some(g) = &arm.guard {
                if !self.eval(g, &scope)?.truthy() {
                    continue;
                }
            }
            return self.eval(&arm.body, &scope);
        }
        fail(line, format!("no `match` arm matches {}", short(&v)))
    }

    #[inline(never)]
    fn eval_for(&mut self, pat: &Pattern, iter: &Expr, body: &Expr, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let it = self.eval(iter, env)?;
        let items: Vec<Value> = match &it {
            Value::List(l) => l.to_vec(),
            Value::Map(m) => m.iter().map(|(k, v)| Value::list(vec![Value::str(k), v.clone()])).collect(),
            Value::Str(s) => s.chars().map(|c| Value::str(c.to_string())).collect(),
            Value::Nil => Vec::new(),
            other => return fail(line, format!("cannot loop over {}", other.type_name())),
        };
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let scope = Scope::child(env);
            let mut binds = Vec::new();
            if self.match_pat(pat, &item, Mode::Strict, env, &mut binds)?.is_none() {
                return fail(line, format!("`for` pattern does not match {}", short(&item)));
            }
            for (n, x) in binds {
                define(&scope, &n, x);
            }
            out.push(self.eval(body, &scope)?);
        }
        Ok(Value::list(out))
    }

    #[inline(never)]
    fn eval_reply(&mut self, status: &Expr, body: Option<&Expr>, env: &Env, line: u32) -> Result<Value, Ctrl> {
        let code = match self.eval(status, env)? {
            Value::Int(n) if (100..=599).contains(&n) => n as u16,
            other => return fail(line, format!("reply status must be an HTTP status code, got {}", short(&other))),
        };
        let body = match body {
            Some(b) => self.eval(b, env)?,
            None => Value::Nil,
        };
        let resp = match body {
            Value::Response(r) => Response { status: code, ..(*r).clone() },
            b => Response { status: code, headers: Vec::new(), body: Body::Value(b) },
        };
        Err(Ctrl::Reply(Arc::new(resp)))
    }

    #[inline(never)]
    fn eval_html(&mut self, el: &HtmlEl, env: &Env) -> Result<Value, Ctrl> {
        Ok(Value::html(self.render_el(el, env)?))
    }

    /// Calls `callee(args)`, with `pre` (the left side of `|`) as the first
    /// argument. `x.name(args)` calls a function stored in map `x` under
    /// `name`, or else the function `name` with `x` first: `xs.sort()`.
    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        pre: Option<Value>,
        env: &Env,
        line: u32,
    ) -> Result<Value, Ctrl> {
        let mut argv = Vec::with_capacity(args.len() + 2);
        let f = if let Expr::Field(obj, name, _) = callee {
            let recv = self.eval(obj, env)?;
            let method = match &recv {
                Value::Map(m) => m.get(name).cloned(),
                _ => None,
            };
            match method {
                Some(f) => f,
                None => match lookup(env, name).filter(|f| matches!(f, Value::Func(_) | Value::Builtin(..))) {
                    Some(f) => {
                        argv.push(recv);
                        f
                    }
                    None => return fail(line, format!("{} has no method `{}`", recv.type_name(), name)),
                },
            }
        } else {
            self.eval(callee, env)?
        };
        if let Some(p) = pre {
            argv.push(p);
        }
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        self.call(&f, argv, line)
    }

    fn get_field(&mut self, v: Value, name: &str, line: u32) -> Result<Value, Ctrl> {
        match &v {
            Value::Nil => return Ok(Value::Nil),
            Value::Map(m) => {
                if let Some(x) = m.get(name) {
                    return Ok(x.clone());
                }
            }
            Value::Response(r) => match name {
                "status" => return Ok(Value::Int(r.status as i64)),
                "headers" => {
                    return Ok(Value::map(r.headers.iter().map(|(k, v)| (k.clone(), Value::str(v))).collect()))
                }
                "body" => {
                    return Ok(match &r.body {
                        Body::Value(b) => b.clone(),
                        Body::Bytes(b) => Value::str(String::from_utf8_lossy(b)),
                    })
                }
                _ => {}
            },
            Value::Err(e) if name == "error" => return Ok((**e).clone()),
            _ => {}
        }
        // `xs.len` is `len(xs)`. On maps only `len`, `keys` and `values` work
        // this way, so any other missing key on a record is simply null.
        if let Some(f) = lookup(&self.globals, name) {
            let usable = match (&v, &f) {
                (Value::Map(_), Value::Builtin(..)) => matches!(name, "len" | "keys" | "values"),
                (Value::Map(_), _) => false,
                (_, Value::Func(_) | Value::Builtin(..)) => true,
                _ => false,
            };
            if usable {
                return self.call(&f, vec![v], line);
            }
        }
        if matches!(v, Value::Map(_)) {
            return Ok(Value::Nil);
        }
        fail(line, format!("{} has no field `{}`", v.type_name(), name))
    }

    pub fn call(&mut self, f: &Value, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
        match f {
            Value::Builtin(name, func) => {
                if self.allowed != ALL_EFFECTS {
                    self.check_builtin_effect(name, line)?;
                }
                func(self, args, line)
            }
            Value::Func(c) => {
                let c = c.clone();
                self.call_closure(&c, args.into_iter().map(Some).collect(), line)
            }
            other => fail(line, format!("cannot call {}: it is not a function", other.type_name())),
        }
    }

    /// `args[i] == None` means "not given": the parameter's default is used.
    pub fn call_closure(&mut self, c: &Closure, args: Vec<Option<Value>>, line: u32) -> Result<Value, Ctrl> {
        let def = c.def.clone();
        if args.len() > def.params.len() {
            return fail(
                line,
                format!(
                    "`{}` takes {} argument{}, got {}",
                    def.name,
                    def.params.len(),
                    if def.params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
            );
        }
        if self.depth >= MAX_DEPTH {
            return fail(line, format!("too much recursion (more than {} nested calls) in `{}`", MAX_DEPTH, def.name));
        }
        // `with` narrows what the call may do; it cannot widen it.
        let saved = match &def.effects {
            Some(effs) => {
                let mask = effect_mask(effs);
                if mask & !self.allowed != 0 {
                    return fail(line, self.effect_denied(&def.name, mask & !self.allowed));
                }
                let prev_mask = std::mem::replace(&mut self.allowed, mask);
                Some((prev_mask, std::mem::replace(&mut self.restrictor, Some(def.clone()))))
            }
            None => None,
        };
        self.depth += 1;
        // Grow the stack on demand, so deep recursion works on any thread.
        let r = stacker::maybe_grow(128 << 10, 4 << 20, || self.call_closure_inner(c, &def, args, line));
        self.depth -= 1;
        if let Some((mask, who)) = saved {
            self.allowed = mask;
            self.restrictor = who;
        }
        let r = r?;
        if let (Some(ety), Value::Err(payload)) = (&def.err, &r) {
            if !type_matches(ety, payload) {
                return fail(
                    def.line,
                    format!(
                        "`{}` declares errors as `! {}`, but returned err({})",
                        def.name,
                        ety.text(),
                        short(payload)
                    ),
                );
            }
        }
        Ok(r)
    }

    /// "`http.get` needs the http effect, but `load` is declared `with db`".
    fn effect_denied(&self, what: &str, missing: u8) -> String {
        let names = effect_names(missing);
        let need = format!("the {} effect{}", names.join(", "), if names.len() == 1 { "" } else { "s" });
        match &self.restrictor {
            Some(f) => format!(
                "`{}` needs {}, but `{}` is declared `with {}`",
                what,
                need,
                f.name,
                f.effects.as_deref().unwrap_or_default().join(", ")
            ),
            None => format!("`{}` needs {}, which is not allowed here", what, need),
        }
    }

    #[cold]
    fn check_builtin_effect(&self, name: &str, line: u32) -> Result<(), Ctrl> {
        let need = builtin_effect(name);
        if need & !self.allowed != 0 {
            return fail(line, self.effect_denied(name, need));
        }
        Ok(())
    }

    fn call_closure_inner(&mut self, c: &Closure, def: &FnDef, args: Vec<Option<Value>>, line: u32) -> Result<Value, Ctrl> {
        let scope = Scope::child(&c.env);
        if def.server {
            if let Some(req) = &self.current_request {
                define(&scope, "request", req.clone());
            }
        }
        let mut it = args.into_iter();
        for p in &def.params {
            let v = match it.next().flatten() {
                Some(v) => v,
                None => match &p.default {
                    Some(d) => self.eval(d, &scope)?,
                    None => return fail(line, format!("missing argument `{}` in call to `{}`", p.name, def.name)),
                },
            };
            if let Some(ty) = &p.ty {
                if !type_matches(ty, &v) {
                    return fail(
                        line,
                        format!(
                            "argument `{}` of `{}` must be {}, got {} {}",
                            p.name,
                            def.name,
                            ty_text(ty),
                            v.type_name(),
                            short(&v)
                        ),
                    );
                }
            }
            define(&scope, &p.name, v);
        }
        let r = match self.eval(&def.body, &scope) {
            Ok(v) | Err(Ctrl::Return(v)) => v,
            Err(e) => return Err(e),
        };
        if let Some(ty) = &def.ret {
            if !matches!(r, Value::Err(_)) && !type_matches(ty, &r) {
                return fail(
                    def.line,
                    format!("`{}` must return {}, but returned {} {}", def.name, ty_text(ty), r.type_name(), short(&r)),
                );
            }
        }
        Ok(r)
    }

    // ---- patterns ------------------------------------------------------

    /// Returns the matched (possibly coerced) value, or `None` on no match.
    /// Bindings are appended to `binds`.
    pub fn match_pat(
        &mut self,
        p: &Pattern,
        v: &Value,
        mode: Mode,
        env: &Env,
        binds: &mut Vec<(String, Value)>,
    ) -> Result<Option<Value>, Ctrl> {
        let lenient = mode == Mode::Lenient;
        Ok(match p {
            Pattern::Wild => Some(v.clone()),
            Pattern::Bind(n) => {
                binds.push((n.clone(), v.clone()));
                Some(v.clone())
            }
            Pattern::Lit(l) => {
                let lv = lit_value(l);
                if equal(&lv, v) {
                    Some(v.clone())
                } else if let (true, Value::Str(s)) = (lenient, v) {
                    coerce(lv.type_name(), s).filter(|c| equal(c, &lv))
                } else {
                    None
                }
            }
            Pattern::Type(t) => {
                if type_name_matches(t, v) {
                    Some(v.clone())
                } else if let (true, Value::Str(s)) = (lenient, v) {
                    coerce(t, s)
                } else {
                    None
                }
            }
            Pattern::Alt(ps) => {
                let mut found = None;
                for q in ps {
                    let mark = binds.len();
                    if let Some(x) = self.match_pat(q, v, mode, env, binds)? {
                        found = Some(x);
                        break;
                    }
                    binds.truncate(mark);
                }
                found
            }
            Pattern::As(q, n) => match self.match_pat(q, v, mode, env, binds)? {
                Some(x) => {
                    binds.push((n.clone(), x.clone()));
                    Some(x)
                }
                None => None,
            },
            Pattern::List(items, rest) => {
                let Value::List(l) = v else { return Ok(None) };
                if l.len() < items.len() || (rest.is_none() && l.len() != items.len()) {
                    return Ok(None);
                }
                for (q, x) in items.iter().zip(l.iter()) {
                    if self.match_pat(q, x, mode, env, binds)?.is_none() {
                        return Ok(None);
                    }
                }
                if let Some(Some(n)) = rest {
                    binds.push((n.clone(), Value::list(l[items.len()..].to_vec())));
                }
                Some(v.clone())
            }
            Pattern::Record(fields) => {
                if !matches!(v, Value::Map(_)) {
                    return Ok(None);
                }
                for f in fields {
                    let sub_mode = match mode {
                        Mode::Request if f.path[0] == "json" => Mode::Strict,
                        Mode::Request => Mode::Lenient,
                        m => m,
                    };
                    let found = get_path(v, &f.path);
                    let val = match (found, &f.default) {
                        (Some(x), _) if !matches!(x, Value::Nil) => x,
                        (_, Some(d)) => self.eval(d, env)?,
                        (Some(x), None) => x,
                        (None, None) => return Ok(None),
                    };
                    let mark = binds.len();
                    let Some(x) = self.match_pat(&f.pat, &val, sub_mode, env, binds)? else {
                        return Ok(None);
                    };
                    // `{ limit: int }` binds `limit`.
                    if binds.len() == mark && matches!(f.pat, Pattern::Type(_)) {
                        binds.push((f.path.last().unwrap().clone(), x));
                    }
                }
                Some(v.clone())
            }
        })
    }

    // ---- HTML ----------------------------------------------------------

    fn render_el(&mut self, el: &HtmlEl, env: &Env) -> Result<String, Ctrl> {
        if el.tag.chars().next().map_or(false, |c| c.is_uppercase()) {
            return self.render_component(el, env);
        }
        let mut out = String::new();
        if el.tag == "html" {
            out.push_str("<!doctype html>");
        }
        out.push('<');
        out.push_str(&el.tag);
        if let Some(id) = &el.id {
            let v = self.eval(id, env)?;
            out.push_str(&format!(" id=\"{}\"", escape(&v.display())));
        }
        let mut classes = Vec::new();
        for c in &el.classes {
            classes.push(self.eval(c, env)?.display());
        }
        let mut rest = String::new();
        for (name, val) in &el.attrs {
            // `@click` handlers only exist in the browser; `bind:value={x}`
            // renders as `value`.
            if name.starts_with('@') || name == "key" {
                continue;
            }
            let name = name.strip_prefix("bind:").unwrap_or(name);
            let v = match val {
                AttrVal::Bool => Value::Bool(true),
                AttrVal::Expr(e) => self.eval(e, env)?,
            };
            if name == "class" {
                if !matches!(v, Value::Nil | Value::Bool(false)) {
                    classes.push(v.display());
                }
                continue;
            }
            match v {
                Value::Bool(true) => {
                    rest.push(' ');
                    rest.push_str(name);
                }
                Value::Bool(false) | Value::Nil => {}
                Value::List(_) | Value::Map(_) => {
                    rest.push_str(&format!(" {}=\"{}\"", name, escape(&v.to_json().to_string())))
                }
                other => rest.push_str(&format!(" {}=\"{}\"", name, escape(&other.display()))),
            }
        }
        if !classes.is_empty() {
            out.push_str(&format!(" class=\"{}\"", escape(&classes.join(" "))));
        }
        out.push_str(&rest);
        out.push('>');
        if VOID_TAGS.contains(&el.tag.as_str()) {
            return Ok(out);
        }
        for c in &el.children {
            self.render_child(c, env, &mut out)?;
        }
        out.push_str("</");
        out.push_str(&el.tag);
        out.push('>');
        Ok(out)
    }

    fn render_child(&mut self, c: &HtmlChild, env: &Env, out: &mut String) -> Result<(), Ctrl> {
        match c {
            HtmlChild::El(el) => out.push_str(&self.render_el(el, env)?),
            HtmlChild::Expr(e) => {
                let v = self.eval(e, env)?;
                render_value(&v, out);
            }
        }
        Ok(())
    }

    /// `<Card user={u}>"kids"</Card>` calls view `Card` with its parameters
    /// taken from attributes by name; a `children` parameter gets the body.
    fn render_component(&mut self, el: &HtmlEl, env: &Env) -> Result<String, Ctrl> {
        let line = el.line;
        let Some(f) = lookup(env, &el.tag) else {
            return fail(line, format!("unknown view <{}>", el.tag));
        };
        let Value::Func(c) = f else {
            return fail(line, format!("<{}> is not a view", el.tag));
        };
        let mut kids = String::new();
        for ch in &el.children {
            self.render_child(ch, env, &mut kids)?;
        }
        let mut given = Vec::new();
        for (name, val) in &el.attrs {
            if name == "key" {
                continue;
            }
            let v = match val {
                AttrVal::Bool => Value::Bool(true),
                AttrVal::Expr(e) => self.eval(e, env)?,
            };
            given.push((name.clone(), v));
        }
        let mut args = Vec::new();
        let mut props = Map::new();
        for p in &c.def.params {
            if p.name == "children" {
                args.push(Some(Value::html(&kids)));
                props.insert(p.name.clone(), Value::str(&kids));
                continue;
            }
            let pos = given.iter().position(|(n, _)| n == &p.name);
            let v = pos.map(|i| given.remove(i).1);
            if let Some(v) = &v {
                props.insert(p.name.clone(), v.clone());
            }
            args.push(v);
        }
        if let Some((n, _)) = given.first() {
            return fail(line, format!("<{}> has no parameter `{}`", el.tag, n));
        }
        if !c.def.component {
            let v = self.call_closure(&c, args, line)?;
            let mut out = String::new();
            render_value(&v, &mut out);
            return Ok(out);
        }
        // A component renders inside an island that carries its props and
        // state to the browser, which takes over from there. A component
        // inside another one gets its props from its parent in the browser
        // too, so only its state travels: its props may be anything,
        // callbacks and HTML included.
        let nested = !self.islands.is_empty();
        self.islands.push((Map::new(), Map::new()));
        let v = self.call_closure(&c, args, line);
        let (state, live) = self.islands.pop().unwrap_or_default();
        let v = v?;
        let props = if nested { Map::new() } else { props };
        for (what, m) in [("prop", &props), ("state", &state)] {
            for (k, x) in m.iter() {
                if let Some(bad) = not_sendable(x) {
                    return fail(
                        line,
                        if what == "prop" {
                            format!(
                                "prop `{}` of <{}> is {}, which cannot be sent to the browser; \
                                 pass it from inside another component, or use a server fn",
                                k, el.tag, bad
                            )
                        } else {
                            format!("{} `{}` of <{}> is {}, which cannot be sent to the browser", what, k, el.tag, bad)
                        },
                    );
                }
            }
        }
        let live_attrs = if live.is_empty() {
            String::new()
        } else {
            format!(
                " data-l=\"{}\" data-v=\"{}\"",
                escape(&Value::map(live).to_json().to_string()),
                self.shared.live_seq()
            )
        };
        let mut out = format!(
            "<quillo-island style=\"display:contents\" data-c=\"{}\" data-p=\"{}\" data-s=\"{}\"{}>",
            escape(&el.tag),
            escape(&Value::map(props).to_json().to_string()),
            escape(&Value::map(state).to_json().to_string()),
            live_attrs
        );
        render_value(&v, &mut out);
        out.push_str("</quillo-island>");
        Ok(out)
    }
}
