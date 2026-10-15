//! Tree-walking evaluator: expressions, patterns and HTML rendering.

use crate::ast::*;
use crate::error::WispError;
use crate::parser::{Parser, VOID_TAGS};
use crate::value::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Non-local exits: runtime errors, `return` (and `?`), and `reply`.
pub enum Ctrl {
    Error(WispError),
    Return(Value),
    Reply(Rc<Response>),
}

impl From<WispError> for Ctrl {
    fn from(e: WispError) -> Self {
        Ctrl::Error(e)
    }
}

pub fn fail<T>(line: u32, msg: impl Into<String>) -> Result<T, Ctrl> {
    Err(Ctrl::Error(WispError::new(msg, line)))
}

fn undefined(name: &str, line: u32) -> Ctrl {
    Ctrl::Error(WispError::new(format!("unknown name `{}`", name), line))
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
    pub routes: Vec<Rc<RouteDef>>,
    pub base_dir: PathBuf,
    loaded: HashSet<PathBuf>,
}

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

fn type_matches(ty: &Ty, v: &Value) -> bool {
    (ty.optional && matches!(v, Value::Nil)) || type_name_matches(&ty.name, v)
}

fn ty_text(ty: &Ty) -> String {
    format!("{}{}", ty.name, if ty.optional { "?" } else { "" })
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
    let overflow = || Ctrl::Error(WispError::new("integer overflow", line));
    match op {
        BinOp::Add => match (a, b) {
            (Int(x), Int(y)) => x.checked_add(y).map(Int).ok_or_else(overflow),
            (Str(x), y) => Ok(Value::str(format!("{}{}", x, y.display()))),
            (x, Str(y)) => Ok(Value::str(format!("{}{}", x.display(), y))),
            (Html(x), Html(y)) => Ok(Value::html(format!("{}{}", x, y))),
            (List(x), List(y)) => {
                let mut v = Rc::unwrap_or_clone(x);
                v.extend(y.iter().cloned());
                Ok(Value::list(v))
            }
            (Map(x), Map(y)) => {
                let mut m = Rc::unwrap_or_clone(x);
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
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let globals = Scope::root();
        crate::builtins::install(&globals);
        Interp { globals, routes: Vec::new(), base_dir: base_dir.into(), loaded: HashSet::new() }
    }

    /// Parses and runs a program's top level, registering its routes.
    pub fn load_source(&mut self, src: &str, file: &str) -> Result<Value, WispError> {
        let stmts = Parser::new(src).parse_program().map_err(|e| e.in_file(file))?;
        let env = self.globals.clone();
        let mut last = Value::Nil;
        for s in &stmts {
            match self.exec_stmt(s, &env) {
                Ok(v) => last = v,
                Err(Ctrl::Error(e)) => return Err(e.in_file(file)),
                Err(Ctrl::Return(v)) => return Ok(v),
                Err(Ctrl::Reply(_)) => return Err(WispError::new("`reply` used outside of a route", 0).in_file(file)),
            }
        }
        Ok(last)
    }

    pub fn load_file(&mut self, path: &Path) -> Result<Value, WispError> {
        let src = std::fs::read_to_string(path)
            .map_err(|e| WispError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
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
                if !assign(env, name, v) {
                    return fail(
                        *line,
                        format!("cannot assign to `{}`: it was never declared (use `let {} = …`)", name, name),
                    );
                }
                Ok(Value::Nil)
            }
            Stmt::Fn(def) => {
                define(env, &def.name, Value::Func(Rc::new(Closure { def: def.clone(), env: env.clone() })));
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
                    .map_err(|e| WispError::new(format!("cannot read {}: {}", full.display(), e), *line))?;
                let stmts = Parser::new(&src).parse_program().map_err(|e| e.in_file(&full.display().to_string()))?;
                let g = self.globals.clone();
                for s in &stmts {
                    self.exec_stmt(s, &g)?;
                }
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

    pub fn eval(&mut self, e: &Expr, env: &Env) -> Result<Value, Ctrl> {
        match e {
            Expr::Lit(l) => Ok(lit_value(l)),
            Expr::Template(parts) => {
                let mut s = String::new();
                for p in parts {
                    match p {
                        TPart::Lit(t) => s.push_str(t),
                        TPart::Expr(x) => s.push_str(&self.eval(x, env)?.display()),
                    }
                }
                Ok(Value::str(s))
            }
            Expr::Ident(n, line) => lookup(env, n).ok_or_else(|| undefined(n, *line)),
            Expr::List(items) => {
                let mut v = Vec::with_capacity(items.len());
                for i in items {
                    v.push(self.eval(i, env)?);
                }
                Ok(Value::list(v))
            }
            Expr::Map(entries) => {
                let mut m = Map::new();
                for (k, x) in entries {
                    let v = self.eval(x, env)?;
                    m.insert(k.clone(), v);
                }
                Ok(Value::map(m))
            }
            Expr::Unary(op, x, line) => {
                let v = self.eval(x, env)?;
                match op {
                    UnOp::Not => Ok(Value::Bool(!v.truthy())),
                    UnOp::Neg => match v {
                        Value::Int(n) => n.checked_neg().map(Value::Int).ok_or_else(|| {
                            Ctrl::Error(WispError::new("integer overflow", *line))
                        }),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        o => fail(*line, format!("cannot negate {}", o.type_name())),
                    },
                }
            }
            Expr::Binary(BinOp::And, a, b, _) => {
                if !self.eval(a, env)?.truthy() {
                    return Ok(Value::Bool(false));
                }
                Ok(Value::Bool(self.eval(b, env)?.truthy()))
            }
            Expr::Binary(BinOp::Or, a, b, _) => {
                if self.eval(a, env)?.truthy() {
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(self.eval(b, env)?.truthy()))
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
            Expr::Pipe(lhs, rhs, line) => {
                let v = self.eval(lhs, env)?;
                match &**rhs {
                    Expr::Call(callee, args, l) => self.eval_call(callee, args, Some(v), env, *l),
                    other => {
                        let f = self.eval(other, env)?;
                        self.call(&f, vec![v], *line)
                    }
                }
            }
            Expr::Call(callee, args, line) => self.eval_call(callee, args, None, env, *line),
            Expr::Field(obj, name, line) => {
                let v = self.eval(obj, env)?;
                self.get_field(v, name, *line)
            }
            Expr::Index(obj, idx, line) => {
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
                    _ => fail(*line, format!("cannot index {} with {}", o.type_name(), i.type_name())),
                }
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
            Expr::Match(subj, arms, line) => {
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
                fail(*line, format!("no `match` arm matches {}", short(&v)))
            }
            Expr::For(pat, iter, body, line) => {
                let it = self.eval(iter, env)?;
                let items: Vec<Value> = match &it {
                    Value::List(l) => l.to_vec(),
                    Value::Map(m) => m.iter().map(|(k, v)| Value::list(vec![Value::str(k), v.clone()])).collect(),
                    Value::Str(s) => s.chars().map(|c| Value::str(c.to_string())).collect(),
                    Value::Nil => Vec::new(),
                    other => return fail(*line, format!("cannot loop over {}", other.type_name())),
                };
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let scope = Scope::child(env);
                    let mut binds = Vec::new();
                    if self.match_pat(pat, &item, Mode::Strict, env, &mut binds)?.is_none() {
                        return fail(*line, format!("`for` pattern does not match {}", short(&item)));
                    }
                    for (n, x) in binds {
                        define(&scope, &n, x);
                    }
                    out.push(self.eval(body, &scope)?);
                }
                Ok(Value::list(out))
            }
            Expr::Block(stmts) => {
                let scope = Scope::child(env);
                let mut last = Value::Nil;
                for s in stmts {
                    last = self.exec_stmt(s, &scope)?;
                }
                Ok(last)
            }
            Expr::Lambda(def) => Ok(Value::Func(Rc::new(Closure { def: def.clone(), env: env.clone() }))),
            Expr::Reply(status, body, line) => {
                let code = match self.eval(status, env)? {
                    Value::Int(n) if (100..=599).contains(&n) => n as u16,
                    other => {
                        return fail(*line, format!("reply status must be an HTTP status code, got {}", short(&other)))
                    }
                };
                let body = match body {
                    Some(b) => self.eval(b, env)?,
                    None => Value::Nil,
                };
                let resp = match body {
                    Value::Response(r) => Response { status: code, ..(*r).clone() },
                    b => Response { status: code, headers: Vec::new(), body: Body::Value(b) },
                };
                Err(Ctrl::Reply(Rc::new(resp)))
            }
            Expr::Html(el, _) => Ok(Value::html(self.render_el(el, env)?)),
        }
    }

    /// Calls `callee(args)`, with `pre` (the left side of `|>`) as the first
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
        // `xs.len` is `len(xs)`. Maps only fall back to builtins, so a missing
        // key on a record is simply nil.
        if let Some(f) = lookup(&self.globals, name) {
            let usable = match (&v, &f) {
                (Value::Map(_), Value::Builtin(..)) => true,
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
            Value::Builtin(_, func) => func(self, args, line),
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
        let scope = Scope::child(&c.env);
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
            let v = match val {
                AttrVal::Bool => Value::Bool(true),
                AttrVal::Expr(e) => self.eval(e, env)?,
            };
            given.push((name.clone(), v));
        }
        let mut args = Vec::new();
        for p in &c.def.params {
            if p.name == "children" {
                args.push(Some(Value::html(&kids)));
                continue;
            }
            let pos = given.iter().position(|(n, _)| n == &p.name);
            args.push(pos.map(|i| given.remove(i).1));
        }
        if let Some((n, _)) = given.first() {
            return fail(line, format!("<{}> has no parameter `{}`", el.tag, n));
        }
        let v = self.call_closure(&c, args, line)?;
        let mut out = String::new();
        render_value(&v, &mut out);
        Ok(out)
    }
}
