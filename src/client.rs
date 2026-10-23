//! Compiles components, and what they use, to JavaScript for the browser.
//!
//! The bundle served at `/_quillo/quillo.js` is the runtime (`client/runtime.js`)
//! followed by the compiled program: every `component`, the functions,
//! views and constants they reference, and a stub for each `server fn`.
//! Code that cannot run in a browser (`request`, `db`, a global the server
//! changes, ...) is a load-time error that names the component.

use crate::ast::*;
use crate::error::QuilloError;
use crate::interp::{not_sendable, Interp};
use crate::value::*;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

const RUNTIME: &str = include_str!("client/runtime.js");

/// Builtins that also exist in the browser.
const CLIENT_BUILTINS: &[&str] = &[
    "print", "str", "int", "float", "len", "type_of", "upper", "lower", "trim", "split", "join", "contains",
    "starts_with", "ends_with", "replace", "slice", "map", "filter", "find", "any", "all", "reduce", "sum", "min",
    "max", "sort", "sort_by", "reverse", "first", "last", "push", "range", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "raw", "escape", "now", "random", "random_int", "uuid", "abs",
    "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten", "zip", "take", "drop", "count", "group_by",
    "index_of", "repeat", "url", "navigate", "css", "sql", "parallel", "every", "t", "locale", "js", "js_new",
];

pub fn bundle(it: &Interp) -> Result<String, QuilloError> {
    let mut c = Compiler::new(it);
    let mut names: Vec<(String, Arc<FnDef>)> = Vec::new();
    for (name, v) in scope_vars(&it.globals) {
        if let Value::Func(f) = v {
            if f.def.component {
                names.push((name, f.def.clone()));
            }
        }
    }
    names.sort_by_key(|(_, d)| d.line);
    for (name, _) in &names {
        c.global(name, 0)?;
    }
    let mut program = String::new();
    if !it.base.is_empty() {
        let _ = writeln!(program, "BASE = {};", js_str(&it.base));
    }
    for b in &c.blocks {
        program.push_str(b);
        program.push('\n');
    }
    for d in &c.defs {
        program.push_str(d);
        program.push('\n');
    }
    if !it.server_fns.is_empty() {
        program.push_str("const server = {};\n");
        for name in it.server_fns.iter() {
            let _ = writeln!(program, "server[{}] = W.server({});", js_str(name), js_str(name));
        }
        program.push_str("window.quillo = { server, components: C, rpc };\n");
    }
    if !names.is_empty() {
        program.push_str(
            "if (document.readyState === \"loading\") document.addEventListener(\"DOMContentLoaded\", boot); else boot();\n",
        );
    }
    Ok(assemble("(function () {", &program, "})();"))
}

/// A script that runs the runtime, compiles the global function `name`,
/// calls it and prints the result's repr: for testing against the server.
pub fn test_script(it: &Interp, name: &str) -> Result<String, QuilloError> {
    let mut c = Compiler::new(it);
    let (js, is_async) = c.global(name, 0)?;
    let mut program = String::new();
    for b in &c.blocks {
        program.push_str(b);
        program.push('\n');
    }
    for d in &c.defs {
        program.push_str(d);
        program.push('\n');
    }
    let _ = writeln!(
        program,
        "try {{ console.log(W.repr({}{}())); }} catch (e) {{ console.log(\"ERROR: \" + (e instanceof Ret ? W.repr(e.v) : e.message)); }}",
        if is_async { "await " } else { "" },
        js
    );
    Ok(assemble("(async function () {", &program, "})();"))
}

/// The parts of the runtime `program` needs, then the program, minified.
fn assemble(open: &str, program: &str, close: &str) -> String {
    let rt = runtime_units();
    let mut keep = vec![false; rt.units.len()];
    let mut todo: Vec<String> = names_in(program);
    while let Some(n) = todo.pop() {
        if let Some(&i) = rt.by_name.get(&n) {
            if !keep[i] {
                keep[i] = true;
                todo.extend(names_in(&rt.units[i].1));
            }
        }
    }
    let mut out = String::with_capacity(program.len() + 16 * 1024);
    out.push_str(open);
    out.push('\n');
    minify_into(&rt.preamble, &mut out);
    for (i, (_, text)) in rt.units.iter().enumerate() {
        if keep[i] {
            minify_into(text, &mut out);
        }
    }
    minify_into(program, &mut out);
    out.push_str(close);
    out.push('\n');
    out
}

/// The runtime cut into its top-level declarations (`function f`, `const x`,
/// `class C`, `B.name = ...`, `W.name = ...`), in order.
struct Runtime {
    preamble: String,
    units: Vec<(String, String)>,
    by_name: HashMap<String, usize>,
}

fn runtime_units() -> &'static Runtime {
    static RT: std::sync::OnceLock<Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        let mut preamble = String::new();
        let mut units: Vec<(String, String)> = Vec::new();
        for line in RUNTIME.lines() {
            let decl = ["async function ", "function ", "class ", "const ", "let "]
                .iter()
                .find_map(|p| line.strip_prefix(p))
                .map(|rest| rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect::<String>())
                .or_else(|| {
                    ["B.", "W."].iter().find_map(|p| {
                        line.strip_prefix(p).map(|rest| {
                            let n: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                            format!("{}{}", p, n)
                        })
                    })
                });
            match decl {
                Some(name) if !name.is_empty() && !name.ends_with('.') => units.push((name, format!("{}\n", line))),
                _ => match units.last_mut() {
                    Some(u) => {
                        u.1.push_str(line);
                        u.1.push('\n');
                    }
                    None => {
                        preamble.push_str(line);
                        preamble.push('\n');
                    }
                },
            }
        }
        let by_name = units.iter().enumerate().map(|(i, (n, _))| (n.clone(), i)).collect();
        Runtime { preamble, units, by_name }
    })
}

/// The identifiers in some JavaScript, with `B.x` and `W.x` kept whole
/// (and also giving `B` or `W`). Strings are scanned too: that can only
/// keep more of the runtime than needed, never less.
fn names_in(js: &str) -> Vec<String> {
    let chars: Vec<char> = js.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut line_start = true;
    while i < chars.len() {
        let c = chars[i];
        // A whole-line comment names nothing the code needs.
        if line_start && c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '\n' {
            line_start = true;
        } else if !c.is_whitespace() {
            line_start = false;
        }
        if ident(c) && (i == 0 || !ident(chars[i - 1]) && chars[i - 1] != '.') {
            let start = i;
            while i < chars.len() && ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if (word == "B" || word == "W") && chars.get(i) == Some(&'.') {
                let s2 = i + 1;
                let mut j = s2;
                while j < chars.len() && ident(chars[j]) {
                    j += 1;
                }
                out.push(format!("{}.{}", word, chars[s2..j].iter().collect::<String>()));
            }
            out.push(word);
        } else {
            i += 1;
        }
    }
    out
}

/// `@mount` is no DOM event: the runtime's `$mount` handles it, and naming
/// it so in the program brings that part of the runtime into the bundle.
fn event_name(ev: &str) -> &str {
    if ev == "mount" {
        "$mount"
    } else {
        ev
    }
}

/// Drops comment lines, blank lines and indentation.
fn minify_into(js: &str, out: &mut String) {
    for line in js.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        out.push_str(t);
        out.push('\n');
    }
}

/// What a name refers to inside compiled code.
#[derive(Clone)]
enum Local {
    /// A variable, with its JS name; `Some(async)` when it holds a known function.
    Var(String, Option<bool>),
    /// Component state: read as `$i.s.name`.
    State,
}

struct FnCtx {
    scopes: Vec<HashMap<String, Local>>,
    temps: usize,
    is_async: bool,
    needs_async: bool,
    /// The component being compiled, for error messages and `~` rules.
    component: Option<String>,
    /// `for` loops given a memo so far (see `memo` in the runtime).
    memos: usize,
    /// `for` loops being compiled, around the code being compiled.
    loops: usize,
}

/// What a loop's body reads from around it, noted while it is compiled:
/// its rows can be reused while all of it stays the same.
struct Reads {
    /// Where the body starts: its function, and the scopes open there.
    ctx: usize,
    scopes: usize,
    /// JS for each value read.
    deps: Vec<String>,
    /// It calls something that may answer differently each time.
    impure: bool,
}

/// Builtins that give the same answer for the same arguments and take no
/// function (which could do anything).
const PURE: &[&str] = &[
    "str", "int", "float", "len", "type_of", "upper", "lower", "trim", "split", "join", "contains", "starts_with",
    "ends_with", "replace", "slice", "sum", "min", "max", "sort", "reverse", "first", "last", "push", "range",
    "enumerate", "keys", "values", "get", "has", "set", "remove", "merge", "err", "is_err", "unwrap", "raw", "escape",
    "abs", "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten", "zip", "take", "drop", "index_of", "repeat",
];

enum Resolved {
    Local(String, Option<bool>),
    State(String),
    /// JS expression and whether calling it waits on the server.
    Global(String, bool),
}

struct Compiler<'a> {
    it: &'a Interp,
    assigned: HashSet<String>,
    /// Globals read from the server's environment, files or database.
    private: HashSet<String>,
    defs: Vec<String>,
    /// Block definitions (see `block`), emitted before everything else.
    blocks: Vec<String>,
    /// Loops being compiled, innermost last (see `Reads`).
    reads: Vec<Reads>,
    /// Holes compiled so far for an input's live `value`, `checked` or
    /// `selected`, which every render sets afresh.
    live_holes: usize,
    /// Globals compiled (or being compiled): JS name and whether they are async.
    globals: HashMap<String, (String, bool)>,
    ctxs: Vec<FnCtx>,
    counter: usize,
    line: u32,
    /// The component or function being compiled, for error messages.
    what: String,
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn ty_js(ty: &Option<Ty>) -> String {
    match ty {
        Some(t) => format!("[{}], {}", t.names.iter().map(|n| js_str(n)).collect::<Vec<_>>().join(", "), t.optional),
        None => "null, false".into(),
    }
}

fn collect_assigned(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign(n, _, e, _) => {
                out.insert(n.clone());
                expr_assigned(e, out);
            }
            Stmt::Let(_, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => expr_assigned(e, out),
            Stmt::Return(Some(e)) => expr_assigned(e, out),
            Stmt::Fn(d) => expr_assigned(&d.body, out),
            Stmt::Route(r) => expr_assigned(&r.body, out),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Return(None) => {}
        }
    }
}

fn expr_assigned(e: &Expr, out: &mut HashSet<String>) {
    let mut f = |x: &Expr| expr_assigned(x, out);
    match e {
        Expr::Block(stmts) => collect_assigned(stmts, out),
        Expr::Lambda(d) => expr_assigned(&d.body, out),
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => {
            for p in parts {
                if let TPart::Expr(x) = p {
                    f(x)
                }
            }
        }
        Expr::List(xs) => xs.iter().for_each(f),
        Expr::Map(es) => es.iter().for_each(|(_, x)| f(x)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => f(x),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            f(a);
            f(b)
        }
        Expr::Call(c, args, _) => {
            f(c);
            args.iter().for_each(f)
        }
        Expr::If(c, a, b) => {
            f(c);
            f(a);
            if let Some(b) = b {
                f(b)
            }
        }
        Expr::Match(s, arms, _) => {
            f(s);
            for a in arms {
                if let Some(g) = &a.guard {
                    expr_assigned(g, out)
                }
                expr_assigned(&a.body, out)
            }
        }
        Expr::For(_, i, b, _) => {
            f(i);
            f(b)
        }
        Expr::Reply(s, b, _) => {
            f(s);
            if let Some(b) = b {
                f(b)
            }
        }
        Expr::Html(el, _) => html_assigned(el, out),
    }
}

/// Calls `f` on `e` and every expression inside it.
fn each_expr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    let stmts = |ss: &[Stmt], f: &mut dyn FnMut(&Expr)| {
        for st in ss {
            match st {
                Stmt::Let(_, x, _) | Stmt::Assign(_, _, x, _) | Stmt::Expr(x) | Stmt::State(_, x, _) | Stmt::Return(Some(x)) => {
                    each_expr(x, f)
                }
                Stmt::Fn(d) => each_expr(&d.body, f),
                Stmt::Route(r) => each_expr(&r.body, f),
                Stmt::Use(..) | Stmt::Mount(..) | Stmt::Return(None) => {}
            }
        }
    };
    match e {
        Expr::Block(ss) => stmts(ss, f),
        Expr::Lambda(d) => each_expr(&d.body, f),
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => parts.iter().for_each(|p| if let TPart::Expr(x) = p { each_expr(x, f) }),
        Expr::List(xs) => xs.iter().for_each(|x| each_expr(x, f)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| each_expr(x, f)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => each_expr(x, f),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            each_expr(a, f);
            each_expr(b, f)
        }
        Expr::Call(c, args, _) => {
            each_expr(c, f);
            args.iter().for_each(|x| each_expr(x, f))
        }
        Expr::If(c, a, b) => {
            each_expr(c, f);
            each_expr(a, f);
            if let Some(b) = b {
                each_expr(b, f)
            }
        }
        Expr::Match(x, arms, _) => {
            each_expr(x, f);
            for a in arms {
                if let Some(g) = &a.guard {
                    each_expr(g, f)
                }
                each_expr(&a.body, f)
            }
        }
        Expr::For(_, i, b, _) => {
            each_expr(i, f);
            each_expr(b, f)
        }
        Expr::Reply(x, b, _) => {
            each_expr(x, f);
            if let Some(b) = b {
                each_expr(b, f)
            }
        }
        Expr::Html(el, _) => each_html_expr(el, f),
    }
}

/// Whether `body` can leave its function early (`return`, or `?` on an
/// error), which compiled code does by throwing `W.Ret`; a function whose
/// body cannot needs no catch. (Functions inside count too: it errs on the
/// side of a catch.)
fn leaves_early(body: &Expr) -> bool {
    let mut found = false;
    each_expr(body, &mut |e| match e {
        Expr::Try(_) => found = true,
        Expr::Block(ss) if ss.iter().any(|s| matches!(s, Stmt::Return(_))) => found = true,
        _ => {}
    });
    found
}

fn each_html_expr(el: &HtmlEl, f: &mut dyn FnMut(&Expr)) {
    for x in el.id.iter().chain(el.classes.iter()) {
        each_expr(x, f)
    }
    for (_, v) in &el.attrs {
        if let AttrVal::Expr(x) | AttrVal::Spread(x) = v {
            each_expr(x, f)
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(e) => each_html_expr(e, f),
            HtmlChild::Expr(x) => each_expr(x, f),
        }
    }
}

/// Builtins whose results may be private to the server: its environment,
/// its files, its database, other services.
const PRIVATE_SOURCES: &[&str] = &["env", "read_file", "file", "db", "http"];

/// Top-level variables whose value comes, directly or through other such
/// variables, from `PRIVATE_SOURCES`. Components get other globals' values
/// written into the browser bundle, which anyone can download; these they
/// must ask a `server fn` for.
fn collect_private(program: &[Arc<Vec<Stmt>>]) -> HashSet<String> {
    let mut private: HashSet<String> = HashSet::new();
    loop {
        let before = private.len();
        for stmts in program {
            for st in stmts.iter() {
                let Stmt::Let(p, e, _) = st else { continue };
                let mut tainted = false;
                each_expr(e, &mut |x| {
                    if let Expr::Ident(n, _) = x {
                        tainted |= PRIVATE_SOURCES.contains(&n.as_str()) || private.contains(n);
                    }
                });
                if tainted {
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    private.extend(names);
                }
            }
        }
        if private.len() == before {
            return private;
        }
    }
}

fn html_assigned(el: &HtmlEl, out: &mut HashSet<String>) {
    for (_, v) in &el.attrs {
        if let AttrVal::Expr(e) | AttrVal::Spread(e) = v {
            expr_assigned(e, out)
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(e) => html_assigned(e, out),
            HtmlChild::Expr(e) => expr_assigned(e, out),
        }
    }
}

/// Names a pattern binds, in order.
fn pattern_names(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Bind(n) => out.push(n.clone()),
        Pattern::Alt(ps) => ps.iter().for_each(|q| pattern_names(q, out)),
        Pattern::As(q, n) => {
            pattern_names(q, out);
            out.push(n.clone())
        }
        Pattern::List(ps, rest) => {
            ps.iter().for_each(|q| pattern_names(q, out));
            if let Some(Some(n)) = rest {
                out.push(n.clone())
            }
        }
        Pattern::Record(fs) => {
            for f in fs {
                let before = out.len();
                pattern_names(&f.pat, out);
                if out.len() == before && f.pat.is_type_test() {
                    out.push(f.path.last().unwrap().clone())
                }
            }
        }
        Pattern::Wild | Pattern::Lit(_) | Pattern::Type(_) => {}
    }
    let mut seen = HashSet::new();
    out.retain(|n| seen.insert(n.clone()));
}

fn lit_js(l: &Lit) -> String {
    match l {
        Lit::Nil => "null".into(),
        Lit::Bool(b) => b.to_string(),
        Lit::Int(n) => n.to_string(),
        Lit::Float(f) if f.is_finite() => format!("{:?}", f),
        Lit::Float(_) => "null".into(),
        Lit::Str(s) => js_str(s),
    }
}

impl<'a> Compiler<'a> {
    fn new(it: &'a Interp) -> Self {
        let mut assigned = HashSet::new();
        for stmts in it.program.iter() {
            collect_assigned(stmts, &mut assigned);
        }
        Compiler {
            it,
            assigned,
            private: collect_private(&it.program),
            defs: Vec::new(),
            blocks: Vec::new(),
            reads: Vec::new(),
            live_holes: 0,
            globals: HashMap::new(),
            ctxs: Vec::new(),
            counter: 0,
            line: 0,
            what: String::new(),
        }
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, QuilloError> {
        let msg = msg.into();
        let where_ = if self.what.is_empty() { String::new() } else { format!("{} (compiled for the browser): ", self.what) };
        Err(QuilloError::new(format!("{}{}", where_, msg), self.line))
    }

    fn fresh(&mut self, name: &str) -> String {
        self.counter += 1;
        let clean: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        format!("${}_{}", clean, self.counter)
    }

    fn ctx(&mut self) -> &mut FnCtx {
        self.ctxs.last_mut().expect("inside a function")
    }

    fn temp(&mut self) -> String {
        let c = self.ctx();
        c.temps += 1;
        format!("$t{}", c.temps - 1)
    }

    fn push_scope(&mut self) {
        self.ctx().scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.ctx().scopes.pop();
    }

    /// Declares a name in the innermost scope; returns its JS name. A second
    /// `let` of the same name in one scope reuses the variable, as on the server.
    fn declare(&mut self, name: &str, func: Option<bool>) -> String {
        if let Some(Local::Var(js, _)) = self.ctx().scopes.last().and_then(|s| s.get(name)).cloned() {
            self.ctx().scopes.last_mut().unwrap().insert(name.to_string(), Local::Var(js.clone(), func));
            return js;
        }
        let js = self.fresh(name);
        self.ctx().scopes.last_mut().unwrap().insert(name.to_string(), Local::Var(js.clone(), func));
        js
    }

    fn resolve(&mut self, name: &str) -> Result<Resolved, QuilloError> {
        let mut found = None;
        'outer: for (ci, ctx) in self.ctxs.iter().enumerate().rev() {
            for (si, scope) in ctx.scopes.iter().enumerate().rev() {
                match scope.get(name) {
                    Some(Local::Var(js, f)) => found = Some((ci, si, Resolved::Local(js.clone(), *f))),
                    Some(Local::State) => found = Some((ci, si, Resolved::State(name.to_string()))),
                    None => continue,
                }
                break 'outer;
            }
        }
        if let Some((ci, si, r)) = found {
            self.note_read(ci, si, &r);
            return Ok(r);
        }
        let (js, is_async) = self.global(name, self.line)?;
        Ok(Resolved::Global(js, is_async))
    }

    /// For the loops being compiled: a name found in scope `si` of function
    /// `ci`. A variable from around a loop is something its rows depend on
    /// (handlers in them keep it), and so is state read while rendering;
    /// handlers read state afresh when they run.
    fn note_read(&mut self, ci: usize, si: usize, r: &Resolved) {
        let here = self.ctxs.len() - 1;
        for reads in &mut self.reads {
            if ci > reads.ctx || (ci == reads.ctx && si >= reads.scopes) {
                continue;
            }
            let dep = match r {
                Resolved::Local(js, _) => js.clone(),
                Resolved::State(n) if here == reads.ctx => format!("$i.s[{}]", js_str(n)),
                _ => continue,
            };
            if !reads.deps.contains(&dep) {
                reads.deps.push(dep);
            }
        }
    }

    /// For the loops being compiled: a call while rendering, of something
    /// `pure` or not.
    fn note_call(&mut self, pure: bool) {
        let here = self.ctxs.len() - 1;
        for reads in &mut self.reads {
            if here == reads.ctx && !pure {
                reads.impure = true;
            }
        }
    }

    /// A top-level name used by browser code: compiled once, on first use.
    fn global(&mut self, name: &str, line: u32) -> Result<(String, bool), QuilloError> {
        if let Some(g) = self.globals.get(name) {
            return Ok(g.clone());
        }
        if name == "request" {
            return self.err("`request` is only available on the server; pass what you need as a prop");
        }
        let Some(v) = lookup(&self.it.globals, name) else {
            return self.err(format!("unknown name `{}`", name));
        };
        let js = format!("G[{}]", js_str(name));
        match &v {
            Value::Builtin(b, _) => {
                if CLIENT_BUILTINS.contains(b) {
                    // `parallel` waits for what it calls, so it is awaited.
                    let r = (format!("B.{}", b), *b == "parallel");
                    self.globals.insert(name.to_string(), r.clone());
                    Ok(r)
                } else {
                    self.err(format!("`{}` is not available in the browser; call it from a `server fn`", b))
                }
            }
            Value::Map(m) if name == "json" && m.values().all(|x| matches!(x, Value::Builtin(..))) => {
                let r = ("B.json".to_string(), false);
                self.globals.insert(name.to_string(), r.clone());
                Ok(r)
            }
            Value::Map(m) if (name == "db" || name == "http") && m.values().all(|x| matches!(x, Value::Builtin(..))) => {
                self.err(format!("`{}` is not available in the browser; use it from a `server fn`", name))
            }
            Value::Func(f) if f.def.server => {
                let r = (js.clone(), true);
                self.globals.insert(name.to_string(), r.clone());
                self.defs.push(format!("{} = W.server({});", js, js_str(name)));
                Ok(r)
            }
            Value::Func(f) => {
                let def = f.def.clone();
                // Recursive references see the function as not async until
                // known. If it turns out async, it is compiled again with
                // them awaited, and so is what it compiled meanwhile, which
                // may call it back.
                let (defs_before, known): (usize, HashSet<String>) = (self.defs.len(), self.globals.keys().cloned().collect());
                let mut assume_async = false;
                let (code, is_async) = loop {
                    self.globals.insert(name.to_string(), (js.clone(), assume_async));
                    let saved_ctxs = std::mem::take(&mut self.ctxs);
                    let saved_what = std::mem::replace(
                        &mut self.what,
                        if def.component { format!("<{}>", def.name) } else { format!("`{}`", def.name) },
                    );
                    let saved_line = self.line;
                    self.line = def.line.max(line);
                    let res = if def.component { self.component(&def) } else { self.function(&def) };
                    self.ctxs = saved_ctxs;
                    self.what = saved_what;
                    self.line = saved_line;
                    let (code, is_async) = res?;
                    if is_async && !assume_async {
                        self.defs.truncate(defs_before);
                        self.globals.retain(|k, _| known.contains(k));
                        assume_async = true;
                        continue;
                    }
                    break (code, is_async);
                };
                let code = if def.component {
                    format!("{} = W.component({}, {});", js, js_str(&def.name), code)
                } else {
                    format!("{} = {};", js, code)
                };
                self.defs.push(code);
                self.globals.insert(name.to_string(), (js.clone(), is_async));
                Ok((js, is_async))
            }
            other => {
                if self.assigned.contains(name) {
                    return self.err(format!(
                        "`{}` is a server variable that the program changes; pass it as a prop or read it with a `server fn`",
                        name
                    ));
                }
                if self.private.contains(name) {
                    return self.err(format!(
                        "`{}` is read from the server's environment, files or database, and would be sent to \
                         every browser; pass what the page needs as a prop or read it with a `server fn`",
                        name
                    ));
                }
                if let Some(bad) = not_sendable(other) {
                    return self.err(format!("`{}` is {}, which cannot be sent to the browser", name, bad));
                }
                self.defs.push(format!("{} = {};", js, other.to_json()));
                let r = (js, false);
                self.globals.insert(name.to_string(), r.clone());
                Ok(r)
            }
        }
    }

    fn new_ctx(&mut self, component: Option<String>, is_async: bool) {
        self.ctxs.push(FnCtx { scopes: vec![HashMap::new()], temps: 0, is_async, needs_async: false, component, memos: 0, loops: 0 });
    }

    fn temps_decl(n: usize) -> String {
        if n == 0 {
            return String::new();
        }
        format!("let {};\n", (0..n).map(|i| format!("$t{}", i)).collect::<Vec<_>>().join(", "))
    }

    /// Parameter defaults and checks; returns the JS parameter names.
    fn params(&mut self, def: &FnDef, out: &mut String, check_arity: bool) -> Result<Vec<String>, QuilloError> {
        let fname = js_str(&def.name);
        if check_arity {
            let n = def.params.len();
            let _ = writeln!(out, "if (arguments.length > {}) W.arity(arguments.length, {}, {});", n, n, fname);
        }
        let mut names = Vec::new();
        for p in &def.params {
            let js = self.declare(&p.name, None);
            match &p.default {
                Some(d) => {
                    let dv = self.expr(d)?;
                    let _ = writeln!(out, "if ({} === undefined) {} = {};", js, js, dv);
                }
                None => {
                    let _ = writeln!(out, "if ({} === undefined) W.missing({}, {});", js, js_str(&p.name), fname);
                }
            }
            if let Some(ty) = &p.ty {
                let call = format!("W.argty({}, {}, {}, {});", js, ty_js(&Some(ty.clone())), js_str(&p.name), fname);
                // The common types checked in place; `argty` says what is wrong.
                let checks: Option<Vec<String>> = ty
                    .names
                    .iter()
                    .map(|t| match t.as_str() {
                        "int" => Some(format!("Number.isInteger({})", js)),
                        "float" | "num" => Some(format!("typeof {} === \"number\"", js)),
                        "str" => Some(format!("typeof {} === \"string\"", js)),
                        "bool" => Some(format!("typeof {} === \"boolean\"", js)),
                        "list" => Some(format!("Array.isArray({})", js)),
                        _ => None,
                    })
                    .collect();
                match checks {
                    Some(mut cs) if ty.names.iter().all(|t| t != "any") => {
                        if ty.optional {
                            cs.push(format!("{} == null", js));
                        }
                        let _ = writeln!(out, "if (!({})) {}", cs.join(" || "), call);
                    }
                    _ => {
                        let _ = writeln!(out, "{}", call);
                    }
                }
            }
            names.push(js);
        }
        Ok(names)
    }

    /// A function or view. Compiled once synchronously; if it turns out to
    /// call the server, again as an `async function`.
    fn function(&mut self, def: &FnDef) -> Result<(String, bool), QuilloError> {
        if def.effects.as_ref().map_or(false, |e| e.iter().any(|x| x != "time" && x != "random")) {
            return self.err(format!(
                "`{}` declares `with {}`, which the browser cannot do; make it a `server fn`",
                def.name,
                def.effects.as_ref().unwrap().join(", ")
            ));
        }
        let first = self.function_with(def, false)?;
        if !first.1 {
            return Ok((first.0, false));
        }
        let (code, _) = self.function_with(def, true)?;
        Ok((code, true))
    }

    fn function_with(&mut self, def: &FnDef, is_async: bool) -> Result<(String, bool), QuilloError> {
        let saved_line = self.line;
        self.line = def.line;
        self.new_ctx(None, is_async);
        let mut pre = String::new();
        let res = (|| -> Result<(Vec<String>, String), QuilloError> {
            let names = self.params(def, &mut pre, true)?;
            let body = self.body(&def.body)?;
            Ok((names, body))
        })();
        let ctx = self.ctxs.pop().unwrap();
        self.line = saved_line;
        let (names, body) = res?;
        let checked = def.ret.is_some() || def.err.is_some();
        let run = if leaves_early(&def.body) {
            format!(
                "{}try {{\n{}}} catch ($e) {{ if ($e instanceof W.Ret) return $e.v; throw $e; }}\n",
                Self::temps_decl(ctx.temps),
                body
            )
        } else {
            format!("{}{}", Self::temps_decl(ctx.temps), body)
        };
        let inner = if checked {
            let call = if is_async { format!("await (async () => {{\n{}}})()", run) } else { format!("(() => {{\n{}}})()", run) };
            format!(
                "return W.retty({}, {}, {}, {});\n",
                call,
                ty_js(&def.ret),
                js_str(&def.name),
                match &def.err {
                    Some(t) => ty_js(&Some(t.clone())),
                    None => "null, false".into(),
                }
            )
        } else {
            run
        };
        let func = format!("{}function ({}) {{\n{}{}}}", if is_async { "async " } else { "" }, names.join(", "), pre, inner);
        // Only named functions need their name and parameters at hand (views
        // are called with props by name); a lambda, made again on each
        // render, goes without.
        let code = if def.name == "<fn>" {
            func
        } else {
            format!(
                "W.meta({}, {}, [{}])",
                func,
                js_str(&def.name),
                def.params.iter().map(|p| js_str(&p.name)).collect::<Vec<_>>().join(", ")
            )
        };
        Ok((code, ctx.needs_async))
    }

    /// A component's render function: `function ($i, ...props)`.
    fn component(&mut self, def: &FnDef) -> Result<(String, bool), QuilloError> {
        self.new_ctx(Some(def.name.clone()), false);
        let mut pre = String::new();
        let res = (|| -> Result<(Vec<String>, String), QuilloError> {
            let names = self.params(def, &mut pre, false)?;
            let body = self.body(&def.body)?;
            Ok((names, body))
        })();
        let ctx = self.ctxs.pop().unwrap();
        let (names, body) = res?;
        let code = format!(
            "[{}], function ($i{}{}) {{\n{}{}{}}}",
            def.params.iter().map(|p| js_str(&p.name)).collect::<Vec<_>>().join(", "),
            if names.is_empty() { "" } else { ", " },
            names.join(", "),
            pre,
            Self::temps_decl(ctx.temps),
            body
        );
        Ok((code, false))
    }

    /// A function body as statements ending in `return`.
    fn body(&mut self, e: &Expr) -> Result<String, QuilloError> {
        match e {
            Expr::Block(stmts) => self.stmts(stmts),
            other => Ok(format!("return {};\n", self.expr(other)?)),
        }
    }

    /// Statements of a block, in a new scope, returning the last value.
    fn stmts(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        self.push_scope();
        let res = self.stmts_inner(stmts);
        self.pop_scope();
        res
    }

    fn stmts_inner(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        let mut body = String::new();
        let mut decls: Vec<String> = Vec::new();
        let n = stmts.len();
        for (i, s) in stmts.iter().enumerate() {
            let last = i + 1 == n;
            match s {
                Stmt::Expr(e) => {
                    let v = self.expr(e)?;
                    if last {
                        let _ = writeln!(body, "return {};", v);
                    } else {
                        let _ = writeln!(body, "{};", v);
                    }
                }
                Stmt::Let(p, e, line) => {
                    self.line = *line;
                    let func = if let (Pattern::Bind(_), Expr::Lambda(d)) = (p, e) { Some(self.lambda_is_async(d)?) } else { None };
                    let v = self.expr(e)?;
                    if let Pattern::Bind(n) = p {
                        let js = self.declare(n, func);
                        decls.push(js.clone());
                        let _ = writeln!(body, "{} = {};", js, v);
                    } else {
                        let pat = self.pattern(p)?;
                        let t = self.temp();
                        let _ = writeln!(body, "{} = W.match({}, {});", t, pat, v);
                        let _ = writeln!(body, "if ({} === null) W.fail(\"`let` pattern does not match \" + W.short({}));", t, v);
                        let mut names = Vec::new();
                        pattern_names(p, &mut names);
                        for n in names {
                            let js = self.declare(&n, None);
                            decls.push(js.clone());
                            let _ = writeln!(body, "{} = {}[{}];", js, t, js_str(&n));
                        }
                    }
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::Assign(name, op, e, line) => {
                    self.line = *line;
                    let rhs = self.expr(e)?;
                    let target = self.resolve(name)?;
                    let cur = match &target {
                        Resolved::Local(js, _) => js.clone(),
                        Resolved::State(n) => format!("$i.s[{}]", js_str(n)),
                        Resolved::Global(..) => {
                            return self.err(format!(
                                "cannot assign to `{}` in the browser: it is a server variable (use `~{}` state in the component, or a `server fn`)",
                                name, name
                            ))
                        }
                    };
                    let v = match op {
                        AssignOp::Set => rhs,
                        AssignOp::Add => format!("W.add({}, {})", cur, rhs),
                        AssignOp::Sub => format!("W.sub({}, {})", cur, rhs),
                    };
                    match target {
                        Resolved::State(n) => {
                            let _ = writeln!(body, "$i.set({}, {});", js_str(&n), v);
                        }
                        _ => {
                            let _ = writeln!(body, "{} = {};", cur, v);
                        }
                    }
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::Fn(def) => {
                    self.line = def.line;
                    if def.server || def.component {
                        return self.err("`server fn` and `component` must be declared at the top level");
                    }
                    let is_async = self.lambda_is_async(def)?;
                    let js = self.declare(&def.name, Some(is_async));
                    decls.push(js.clone());
                    let (code, _) = self.function(def)?;
                    let _ = writeln!(body, "{} = {};", js, code);
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::State(name, e, line) => {
                    self.line = *line;
                    if self.ctxs.len() != 1 || self.ctx().scopes.len() != 2 || self.ctx().component.is_none() {
                        return self.err("`~` state can only be declared directly in a component's body");
                    }
                    // `~x = live(load, ...args)`: a subscription.
                    if let Some((fname, args)) = self.live_call(e)? {
                        let mut argv = Vec::new();
                        for a in args {
                            argv.push(self.expr(a)?);
                        }
                        let _ = writeln!(body, "W.live($i, {}, {}, [{}]);", js_str(name), js_str(&fname), argv.join(", "));
                        self.ctx().scopes.last_mut().unwrap().insert(name.clone(), Local::State);
                        if last {
                            body.push_str("return null;\n");
                        }
                        continue;
                    }
                    // The initializer may wait on the server; it runs once.
                    self.new_ctx(None, false);
                    // Let the initializer see the component's names.
                    let outer = std::mem::take(&mut self.ctxs[0].scopes);
                    self.ctxs.last_mut().unwrap().scopes = outer.clone();
                    self.ctxs.last_mut().unwrap().scopes.push(HashMap::new());
                    let first = self.expr(e);
                    let mut init_ctx = self.ctxs.pop().unwrap();
                    let mut init = first?;
                    if init_ctx.needs_async {
                        self.ctxs.push(FnCtx { scopes: outer.clone(), temps: 0, is_async: true, needs_async: false, component: None, memos: 0, loops: 0 });
                        self.ctxs.last_mut().unwrap().scopes.push(HashMap::new());
                        let again = self.expr(e);
                        init_ctx = self.ctxs.pop().unwrap();
                        init = again?;
                    }
                    self.ctxs[0].scopes = outer;
                    let _ = writeln!(
                        body,
                        "$i.state({}, {}() => {{ {}return {}; }});",
                        js_str(name),
                        if init_ctx.is_async { "async " } else { "" },
                        Self::temps_decl(init_ctx.temps),
                        init
                    );
                    self.ctx().scopes.last_mut().unwrap().insert(name.clone(), Local::State);
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                Stmt::Return(e) => {
                    let v = match e {
                        Some(e) => self.expr(e)?,
                        None => "null".into(),
                    };
                    let _ = writeln!(body, "throw new W.Ret({});", v);
                }
                Stmt::Route(r) => {
                    self.line = r.line;
                    return self.err("routes cannot be declared in browser code");
                }
                Stmt::Use(_, line) => {
                    self.line = *line;
                    return self.err("`use` must be at the top level");
                }
                Stmt::Mount(_, _, line) => {
                    self.line = *line;
                    return self.err("`mount` must be at the top level");
                }
            }
        }
        if stmts.is_empty() {
            body.push_str("return null;\n");
        }
        decls.sort();
        decls.dedup();
        let head = if decls.is_empty() { String::new() } else { format!("let {};\n", decls.join(", ")) };
        Ok(head + &body)
    }

    /// `live(server_fn, ...args)`, if `e` is such a call: the function's
    /// name and the arguments.
    fn live_call<'e>(&mut self, e: &'e Expr) -> Result<Option<(String, &'e [Expr])>, QuilloError> {
        let Expr::Call(callee, args, line) = e else { return Ok(None) };
        let Expr::Ident(n, _) = &**callee else { return Ok(None) };
        if n != "live" || self.ctxs.iter().any(|c| c.scopes.iter().any(|s| s.contains_key("live"))) {
            return Ok(None);
        }
        if !matches!(lookup(&self.it.globals, "live"), Some(Value::Builtin("live", _))) {
            return Ok(None);
        }
        self.line = *line;
        let fname = match args.first() {
            Some(Expr::Ident(f, _)) => match lookup(&self.it.globals, f) {
                Some(Value::Func(c)) if c.def.server => f.clone(),
                _ => return self.err(format!("`live` needs a server fn: `{}` is not one", f)),
            },
            _ => return self.err("`live` needs a server fn by name: ~items = live(load_items, ...)"),
        };
        Ok(Some((fname, &args[1..])))
    }

    /// Whether a function waits on the server (it calls a `server fn`, or a
    /// function that does). Compiles it in a scratch context to find out.
    fn lambda_is_async(&mut self, def: &FnDef) -> Result<bool, QuilloError> {
        let saved_counter = self.counter;
        let saved_defs = self.defs.len();
        let r = self.function_with(def, false).map(|(_, a)| a);
        self.counter = saved_counter.max(self.counter);
        let _ = saved_defs;
        r
    }

    /// `{ ... }` in expression position.
    fn block_expr(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        let inner = self.stmts(stmts)?;
        Ok(self.iife(&inner))
    }

    fn iife(&mut self, body: &str) -> String {
        if self.ctx().is_async {
            format!("(await (async () => {{\n{}}})())", body)
        } else {
            format!("(() => {{\n{}}})()", body)
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<String, QuilloError> {
        Ok(match e {
            Expr::Lit(l) => lit_js(l),
            Expr::Template(parts) => {
                let mut out = Vec::new();
                for p in parts {
                    match p {
                        TPart::Lit(s) => out.push(js_str(s)),
                        TPart::Expr(x) => out.push(format!("W.display({})", self.expr(x)?)),
                    }
                }
                if out.is_empty() {
                    "\"\"".into()
                } else {
                    format!("({})", out.join(" + "))
                }
            }
            Expr::Ident(n, line) => {
                self.line = *line;
                match self.resolve(n)? {
                    Resolved::Local(js, _) => js,
                    Resolved::State(n) => format!("$i.s[{}]", js_str(&n)),
                    Resolved::Global(js, _) => js,
                }
            }
            Expr::List(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.push(self.expr(i)?);
                }
                format!("[{}]", out.join(", "))
            }
            Expr::Map(entries) => {
                let mut out = Vec::new();
                for (k, v) in entries {
                    out.push(format!("{}: {}", js_str(k), self.expr(v)?));
                }
                format!("{{{}}}", out.join(", "))
            }
            Expr::Unary(op, x, line) => {
                self.line = *line;
                let v = self.expr(x)?;
                match op {
                    UnOp::Not => format!("!W.truthy({})", v),
                    UnOp::Neg => format!("W.neg({})", v),
                }
            }
            Expr::Binary(op, a, b, line) => {
                self.line = *line;
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                match op {
                    BinOp::And => {
                        let t = self.temp();
                        format!("(W.truthy({t} = {x}) ? {y} : {t})")
                    }
                    BinOp::Or => {
                        let t = self.temp();
                        format!("(W.truthy({t} = {x}) ? {t} : {y})")
                    }
                    BinOp::Eq => format!("W.eq({}, {})", x, y),
                    BinOp::Ne => format!("!W.eq({}, {})", x, y),
                    BinOp::Add => format!("W.add({}, {})", x, y),
                    BinOp::Sub => format!("W.sub({}, {})", x, y),
                    BinOp::Mul => format!("W.mul({}, {})", x, y),
                    BinOp::Div => format!("W.div({}, {})", x, y),
                    BinOp::Mod => format!("W.mod({}, {})", x, y),
                    BinOp::Lt => format!("W.lt({}, {})", x, y),
                    BinOp::Le => format!("W.le({}, {})", x, y),
                    BinOp::Gt => format!("W.gt({}, {})", x, y),
                    BinOp::Ge => format!("W.ge({}, {})", x, y),
                    BinOp::Range => format!("W.range({}, {})", x, y),
                }
            }
            Expr::Coalesce(a, b) => {
                let t = self.temp();
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                format!("((({t} = {x}) === null || {t} === undefined || {t} instanceof W.Err) ? {y} : {t})")
            }
            Expr::Pipe(lhs, rhs, line) => {
                self.line = *line;
                let v = self.expr(lhs)?;
                match &**rhs {
                    Expr::Call(callee, args, l) => {
                        self.line = *l;
                        self.call(callee, args, Some(v))?
                    }
                    other => {
                        let f = self.callee_value(other)?;
                        self.await_if(f.1, format!("W.call({}, [{}])", f.0, v))
                    }
                }
            }
            Expr::Call(callee, args, line) => {
                self.line = *line;
                self.call(callee, args, None)?
            }
            Expr::Field(obj, name, line) => {
                self.line = *line;
                let o = self.expr(obj)?;
                let fb = self.fallback(name);
                // Names Object.prototype has need the careful check for a
                // map's own key; any other is read directly when present.
                const PROTO: &[&str] = &[
                    "constructor", "hasOwnProperty", "isPrototypeOf", "propertyIsEnumerable", "toLocaleString", "toString",
                    "valueOf", "__proto__", "__defineGetter__", "__defineSetter__", "__lookupGetter__", "__lookupSetter__",
                ];
                if PROTO.contains(&name.as_str()) {
                    format!("W.field({}, {}, {})", o, js_str(name), fb)
                } else {
                    format!("W.fieldp({}, {}, {})", o, js_str(name), fb)
                }
            }
            Expr::Index(obj, idx, line) => {
                self.line = *line;
                format!("W.index({}, {})", self.expr(obj)?, self.expr(idx)?)
            }
            Expr::Try(x) => format!("W.tryv({})", self.expr(x)?),
            Expr::If(c, a, b) => {
                let cv = self.expr(c)?;
                let av = self.branch(a)?;
                let bv = match b {
                    Some(b) => self.branch(b)?,
                    None => "null".into(),
                };
                format!("(W.truthy({}) ? {} : {})", cv, av, bv)
            }
            Expr::Match(subj, arms, line) => {
                self.line = *line;
                let s = self.expr(subj)?;
                let mut body = format!("const $v = {};\nlet $b;\n", s);
                for arm in arms {
                    let pat = self.pattern(&arm.pat)?;
                    self.push_scope();
                    let mut names = Vec::new();
                    pattern_names(&arm.pat, &mut names);
                    let mut binds = String::new();
                    for n in &names {
                        let js = self.declare(n, None);
                        let _ = write!(binds, "const {} = $b[{}]; ", js, js_str(n));
                    }
                    let guard = match &arm.guard {
                        Some(g) => Some(self.expr(g)?),
                        None => None,
                    };
                    let b = self.expr(&arm.body);
                    self.pop_scope();
                    let b = b?;
                    match guard {
                        Some(g) => {
                            let _ = writeln!(body, "if (($b = W.match({}, $v)) !== null) {{ {}if (W.truthy({})) return {}; }}", pat, binds, g, b);
                        }
                        None => {
                            let _ = writeln!(body, "if (($b = W.match({}, $v)) !== null) {{ {}return {}; }}", pat, binds, b);
                        }
                    }
                }
                body.push_str("W.fail(\"no `match` arm matches \" + W.short($v));\n");
                self.iife(&body)
            }
            Expr::For(pat, iter, body, line) => {
                self.line = *line;
                let it = self.expr(iter)?;
                let p = if matches!(pat, Pattern::Bind(_)) { None } else { Some(self.pattern(pat)?) };
                // In a component, rows with keys are remembered by item (see
                // `memo` in the runtime); keyed, a reused row is matched with
                // itself. Only a loop that runs once per render: one inside
                // another runs for each of its items, and rows of one run
                // must not turn up in another.
                let memo = self.ctx().component.is_some()
                    && self.ctx().loops == 0
                    && matches!(&**body, Expr::Html(el, _) if el.attrs.iter().any(|(n, _)| n == "key"));
                if memo {
                    let (ctx, scopes) = (self.ctxs.len() - 1, self.ctx().scopes.len());
                    self.reads.push(Reads { ctx, scopes, deps: Vec::new(), impure: false });
                }
                self.push_scope();
                self.ctx().loops += 1;
                let mut names = Vec::new();
                pattern_names(pat, &mut names);
                let mut binds = String::new();
                for n in &names {
                    let js = self.declare(n, None);
                    match &p {
                        None => {
                            let _ = write!(binds, "const {} = $x; ", js);
                        }
                        Some(_) => {
                            let _ = write!(binds, "const {} = $b[{}]; ", js, js_str(n));
                        }
                    }
                }
                // A row that is one block: kept rows have only the holes that
                // read what changed recomputed (see `hole` in the runtime).
                let live_before = self.live_holes;
                let row = match &**body {
                    Expr::Html(el, _) if memo && blockable(el) => Some(self.block_parts(el)),
                    _ => None,
                };
                let b = match &row {
                    Some(_) => Ok(String::new()),
                    None => self.expr(body),
                };
                self.ctx().loops -= 1;
                self.pop_scope();
                let mut reads = if memo { self.reads.pop() } else { None };
                // Rows with inputs are made each time, so the inputs show the
                // state even after the user typed something else.
                if let Some(r) = &mut reads {
                    if self.live_holes > live_before {
                        r.impure = true;
                    }
                }
                let b = b?;
                let row = row.transpose()?;
                let check = match &p {
                    Some(p) => format!(
                        "const $b = W.match({}, $x); if ($b === null) W.fail(\"`for` pattern does not match \" + W.short($x)); ",
                        p
                    ),
                    None => String::new(),
                };
                let made = |row: &(String, BlockParts, String, Vec<String>)| format!("W.b({}, [{}], {})", row.0, row.1.vals.join(", "), row.2);
                let inner = match (reads, &row) {
                    (Some(r), _) if !r.impure => {
                        let slot = self.ctx().memos;
                        self.ctx().memos += 1;
                        // Each hole's deps as bits of `$c`, what changed (see `memo`).
                        let refresh = match &row {
                            Some(row) if r.deps.len() <= 31 && row.3.is_empty() => {
                                let mut out = String::new();
                                for (k, deps) in row.1.deps.iter().enumerate() {
                                    let mask = deps.iter().filter_map(|d| r.deps.iter().position(|x| x == d)).fold(0u32, |m, i| m | 1 << i);
                                    if mask == 0 {
                                        continue;
                                    }
                                    // Handlers are new closures; other values are passed on only
                                    // when they changed.
                                    if row.1.holes[k].0 == 2 {
                                        let _ = write!(out, "if ($c & {}) W.hole($v, {}, {}); ", mask as i32, k, row.1.vals[k]);
                                    } else {
                                        let _ = write!(
                                            out,
                                            "if ($c & {}) {{ const $h = {}; if ($h !== $v.vals[{}]) W.hole($v, {}, $h); }} ",
                                            mask as i32,
                                            row.1.vals[k],
                                            k,
                                            k
                                        );
                                    }
                                }
                                Some(out)
                            }
                            _ => None,
                        };
                        let row_js = match &row {
                            Some(row) => made(row),
                            None => b.clone(),
                        };
                        let refresh = match &refresh {
                            Some(refresh) => format!("($v, $x, $c) => {{ {}{}{}}}", check, binds, refresh),
                            None => "null".to_string(),
                        };
                        format!(
                            "return W.rows(W.memo($i, {}, [{}]), W.iter({}), ($x) => {{ {}{}return {}; }}, {});\n",
                            slot,
                            r.deps.join(", "),
                            it,
                            check,
                            binds,
                            row_js,
                            refresh
                        )
                    }
                    (_, row) => format!(
                        "const $out = [];\nfor (const $x of W.iter({})) {{ {}{}$out.push({}); }}\nreturn $out;\n",
                        it,
                        check,
                        binds,
                        match row {
                            Some(row) => made(row),
                            None => b.clone(),
                        }
                    ),
                };
                self.iife(&inner)
            }
            Expr::Block(stmts) => self.block_expr(stmts)?,
            Expr::Lambda(def) => {
                if def.effects.is_some() {
                    self.line = def.line;
                }
                let (code, _) = self.function(def)?;
                code
            }
            Expr::Reply(_, _, line) => {
                self.line = *line;
                return self.err("`reply` only works on the server");
            }
            Expr::Html(el, _) => self.html(el)?,
        })
    }

    /// A branch of `if`: blocks become IIFEs, other expressions stay inline.
    fn branch(&mut self, e: &Expr) -> Result<String, QuilloError> {
        self.expr(e)
    }

    fn await_if(&mut self, is_async: bool, call: String) -> String {
        if !is_async {
            return call;
        }
        if self.ctx().component.is_some() {
            // Reported by `call`, which knows the name.
            return call;
        }
        let c = self.ctx();
        c.needs_async = true;
        if c.is_async {
            format!("(await {})", call)
        } else {
            call
        }
    }

    /// What `name` means as a function for `x.name` and `x.name()`, or "null".
    fn fallback(&mut self, name: &str) -> String {
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(js, _)) => return js.clone(),
                    Some(Local::State) => return format!("$i.s[{}]", js_str(name)),
                    None => {}
                }
            }
        }
        match lookup(&self.it.globals, name) {
            Some(Value::Builtin(b, _)) if CLIENT_BUILTINS.contains(&b) => format!("B.{}", b),
            Some(Value::Func(f)) if !f.def.component => {
                let line = self.line;
                match self.global(name, line) {
                    Ok((js, _)) => js,
                    Err(_) => "null".into(),
                }
            }
            _ => "null".into(),
        }
    }

    /// The function value an expression names, and whether calling it waits.
    fn callee_value(&mut self, e: &Expr) -> Result<(String, bool), QuilloError> {
        if let Expr::Ident(n, line) = e {
            self.line = *line;
            return Ok(match self.resolve(n)? {
                Resolved::Local(js, f) => (js, f.unwrap_or(false)),
                Resolved::State(n) => (format!("$i.s[{}]", js_str(&n)), false),
                Resolved::Global(js, a) => (js, a),
            });
        }
        Ok((self.expr(e)?, false))
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], pre: Option<String>) -> Result<String, QuilloError> {
        let mut argv: Vec<String> = pre.into_iter().collect();
        if let Expr::Field(obj, name, _) = callee {
            let recv = self.expr(obj)?;
            let fb = self.fallback(name);
            for a in args {
                argv.push(self.expr(a)?);
            }
            let is_async = self.is_async_name(name);
            self.note_call(PURE.contains(&name.as_str()));
            let call = format!("W.mcall({}, {}, {}, [{}])", recv, js_str(name), fb, argv.join(", "));
            return self.finish_call(name, is_async, call);
        }
        let (f, is_async) = self.callee_value(callee)?;
        if f == "B.js" || f == "B.js_new" {
            self.js_name(&f[2..], args.first())?;
        }
        let pure = matches!(callee, Expr::Ident(n, _) if PURE.contains(&n.as_str()) && f == format!("B.{}", n));
        self.note_call(pure);
        for a in args {
            argv.push(self.expr(a)?);
        }
        let name = match callee {
            Expr::Ident(n, _) => n.clone(),
            _ => "this function".into(),
        };
        let call = if f.starts_with("B.") || f.starts_with("G[") {
            format!("{}({})", f, argv.join(", "))
        } else {
            format!("W.call({}, [{}])", f, argv.join(", "))
        };
        self.finish_call(&name, is_async, call)
    }

    /// `js` and `js_new` name what they call in the code itself: a name
    /// that came from data could be anything on the page, `eval` included.
    fn js_name(&mut self, f: &str, name: Option<&Expr>) -> Result<(), QuilloError> {
        let Some(Expr::Lit(Lit::Str(path))) = name else {
            return self.err(format!("`{}` needs the name of what it calls written out, like {}(\"Chart\", ...)", f, f));
        };
        let ok = !path.is_empty() && path.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'));
        if !ok {
            return self.err(format!("`{}`: {:?} is not a dotted name such as \"L.map\"", f, path));
        }
        let banned = ["eval", "Function", "setTimeout", "setInterval", "execScript"];
        let last = path.rsplit('.').next().unwrap_or("");
        if banned.contains(&last) || path.starts_with("document.write") || last == "constructor" || last == "__proto__" {
            return self.err(format!("`{}` does not call {}: it runs text as code", f, path));
        }
        Ok(())
    }

    fn is_async_name(&mut self, name: &str) -> bool {
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(_, f)) => return f.unwrap_or(false),
                    Some(Local::State) => return false,
                    None => {}
                }
            }
        }
        match lookup(&self.it.globals, name) {
            Some(Value::Func(f)) if !f.def.component => {
                let line = self.line;
                self.global(name, line).map(|(_, a)| a).unwrap_or(false)
            }
            _ => false,
        }
    }

    fn finish_call(&mut self, name: &str, is_async: bool, call: String) -> Result<String, QuilloError> {
        if is_async && self.ctx().component.is_some() {
            return self.err(format!(
                "`{}` waits on the server, so a component can only call it from an event handler \
                 (@click={{fn() {{ ... }}}}) or to set state: ~x = {}(...)",
                name, name
            ));
        }
        Ok(self.await_if(is_async, call))
    }

    fn pattern(&mut self, p: &Pattern) -> Result<String, QuilloError> {
        Ok(match p {
            Pattern::Wild => "{k: \"w\"}".into(),
            Pattern::Bind(n) => format!("{{k: \"b\", n: {}}}", js_str(n)),
            Pattern::Lit(l) => format!("{{k: \"l\", v: {}}}", lit_js(l)),
            Pattern::Type(t) => format!("{{k: \"t\", t: {}}}", js_str(t)),
            Pattern::Alt(ps) => {
                let mut out = Vec::new();
                for q in ps {
                    out.push(self.pattern(q)?);
                }
                format!("{{k: \"a\", ps: [{}]}}", out.join(", "))
            }
            Pattern::As(q, n) => format!("{{k: \"as\", p: {}, n: {}}}", self.pattern(q)?, js_str(n)),
            Pattern::List(ps, rest) => {
                let mut out = Vec::new();
                for q in ps {
                    out.push(self.pattern(q)?);
                }
                let rest = match rest {
                    None => "undefined".to_string(),
                    Some(None) => "\"\"".to_string(),
                    Some(Some(n)) => js_str(n),
                };
                format!("{{k: \"ls\", ps: [{}], rest: {}}}", out.join(", "), rest)
            }
            Pattern::Record(fs) => {
                let mut out = Vec::new();
                for f in fs {
                    let d = match &f.default {
                        Some(d) => {
                            let saved = self.ctx().needs_async;
                            let v = self.expr(d)?;
                            if self.ctx().needs_async && !saved {
                                return self.err("a pattern default cannot wait on the server");
                            }
                            format!("() => {}", v)
                        }
                        None => "null".into(),
                    };
                    out.push(format!(
                        "{{path: [{}], p: {}, d: {}, ty: {}}}",
                        f.path.iter().map(|k| js_str(k)).collect::<Vec<_>>().join(", "),
                        self.pattern(&f.pat)?,
                        d,
                        f.pat.is_type_test()
                    ));
                }
                format!("{{k: \"r\", fs: [{}]}}", out.join(", "))
            }
        })
    }

    fn html(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        self.line = el.line;
        if blockable(el) {
            return self.block(el);
        }
        let mut kids = Vec::new();
        for c in &el.children {
            kids.push(match c {
                HtmlChild::El(e) => self.html(e)?,
                HtmlChild::Expr(x) => self.expr(x)?,
            });
        }
        let kids = format!("[{}]", kids.join(", "));
        let mut key = "null".to_string();
        if el.tag.chars().next().map_or(false, |c| c.is_uppercase()) {
            self.line = el.line;
            let (f, _) = self.callee_value(&Expr::Ident(el.tag.clone(), el.line))?;
            // A view is called while rendering; a component renders itself.
            let component = matches!(lookup(&self.it.globals, &el.tag), Some(Value::Func(c)) if c.def.component);
            self.note_call(component);
            let mut props = Vec::new();
            let mut spreads = Vec::new();
            for (name, v) in &el.attrs {
                let v = match v {
                    AttrVal::Bool => "true".to_string(),
                    AttrVal::Expr(e) => self.expr(e)?,
                    AttrVal::Spread(e) => {
                        spreads.push(self.expr(e)?);
                        continue;
                    }
                };
                if name == "key" {
                    key = v;
                } else {
                    props.push(format!("{}: {}", js_str(name), v));
                }
            }
            if spreads.is_empty() {
                return Ok(format!("W.comp({}, {{{}}}, {}, {})", f, props.join(", "), kids, key));
            }
            return Ok(format!("W.comp({}, {{{}}}, {}, {}, [{}])", f, props.join(", "), kids, key, spreads.join(", ")));
        }
        let mut props = Vec::new();
        let mut on: Vec<String> = Vec::new();
        if let Some(id) = &el.id {
            props.push(format!("id: {}", self.expr(id)?));
        }
        let mut classes = Vec::new();
        for c in &el.classes {
            classes.push(self.expr(c)?);
        }
        let mut spreads = Vec::new();
        for (name, v) in &el.attrs {
            let val = match v {
                AttrVal::Bool => "true".to_string(),
                AttrVal::Expr(e) => self.expr(e)?,
                AttrVal::Spread(e) => {
                    spreads.push(self.expr(e)?);
                    continue;
                }
            };
            if let Some(ev) = name.strip_prefix('@') {
                on.push(format!("{}: {}", js_str(event_name(ev)), val));
            } else if let Some(prop) = name.strip_prefix("bind:") {
                let AttrVal::Expr(Expr::Ident(var, _)) = v else {
                    return self.err(format!("`bind:{}` needs a variable: bind:{}={{name}}", prop, prop));
                };
                let (event, read) = match prop {
                    "value" => ("input", "e.target.value"),
                    "checked" => ("change", "e.target.checked"),
                    other => return self.err(format!("`bind:{}` is not supported (use bind:value or bind:checked)", other)),
                };
                let set = match self.resolve(var)? {
                    Resolved::State(n) => format!("$i.set({}, {})", js_str(&n), read),
                    Resolved::Local(js, _) => format!("({} = {})", js, read),
                    Resolved::Global(..) => return self.err(format!("`bind:{}` needs component state, like ~{}", prop, var)),
                };
                self.live_holes += 1;
                props.push(format!("{}: {}", js_str(prop), val));
                on.push(format!("{}: (e) => {{ {}; }}", js_str(event), set));
            } else if name == "key" {
                key = val;
            } else if name == "class" {
                classes.push(val);
            } else {
                if matches!(name.as_str(), "value" | "checked" | "selected") {
                    self.live_holes += 1;
                }
                props.push(format!("{}: {}", js_str(name), val));
            }
        }
        if !classes.is_empty() {
            props.push(format!("class: W.cls([{}])", classes.join(", ")));
        }
        let on = if on.is_empty() { "null".to_string() } else { format!("{{{}}}", on.join(", ")) };
        if !spreads.is_empty() {
            return Ok(format!(
                "W.hs({}, [{}], {{{}}}, {}, {}, {})",
                js_str(&el.tag),
                spreads.join(", "),
                props.join(", "),
                on,
                kids,
                key
            ));
        }
        Ok(format!("W.h({}, {{{}}}, {}, {}, {})", js_str(&el.tag), props.join(", "), on, kids, key))
    }
}

/// Whether `el` can be the root of a block: a plain element (not a
/// component), without `{...spread}` attributes or `bind:`, which the
/// virtual DOM handles.
fn blockable(el: &HtmlEl) -> bool {
    !el.tag.starts_with(|c: char| c.is_uppercase())
        && el.tag != "foreignObject"
        && el.attrs.iter().all(|(name, v)| !matches!(v, AttrVal::Spread(_)) && !name.starts_with("bind:"))
}

/// Whether `el` can be part of its parent's block: also without a `key`,
/// which makes an element start afresh when it changes.
fn inlineable(el: &HtmlEl) -> bool {
    blockable(el) && el.attrs.iter().all(|(name, _)| name != "key")
}

/// A value fixed when the program is written: text, a number or a bool.
fn fixed(e: &Expr) -> Option<&Lit> {
    match e {
        Expr::Lit(l) => Some(l),
        _ => None,
    }
}

/// What a block is made of while it is compiled.
#[derive(Default)]
struct BlockParts {
    /// Positions of the nodes with holes, as child indexes from the root.
    paths: Vec<Vec<usize>>,
    /// Kind, node and name of each hole (see `block` in the runtime).
    holes: Vec<(u8, usize, String)>,
    /// The JS for each hole's value, in order.
    vals: Vec<String>,
    /// What each value reads from around the loop it is in (see `Reads`).
    deps: Vec<Vec<String>>,
}

impl<'a> Compiler<'a> {
    /// Markup of a fixed shape, as a block (see `block` in the runtime): its
    /// template is defined once, and each render passes the holes' values.
    fn block(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        let (name, parts, key, _) = self.block_parts(el)?;
        Ok(format!("W.b({}, [{}], {})", name, parts.vals.join(", "), key))
    }

    /// The block for `el`, defined: its name, holes, key, and what the key
    /// reads from around the loop it is in.
    fn block_parts(&mut self, el: &HtmlEl) -> Result<(String, BlockParts, String, Vec<String>), QuilloError> {
        let mut parts = BlockParts::default();
        let tpl = self.block_el(el, &mut Vec::new(), &mut parts)?;
        let mut key = ("null".to_string(), Vec::new());
        for (name, v) in &el.attrs {
            if name == "key" {
                if let AttrVal::Expr(e) = v {
                    key = self.noted(|c| c.expr(e))?;
                }
            }
        }
        let name = format!("$b{}", self.blocks.len());
        let holes: Vec<String> = parts.holes.iter().map(|(k, at, n)| format!("[{}, {}, {}]", k, at, js_str(n))).collect();
        self.blocks.push(format!(
            "const {} = W.block({}, [{}], {}, {});",
            name,
            tpl,
            holes.join(", "),
            block_mount(&parts),
            block_patch(&parts)
        ));
        Ok((name, parts, key.0, key.1))
    }

    /// Compiles with `f`, noting what is read from around the innermost loop
    /// being compiled (nothing outside one).
    fn noted<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, QuilloError>) -> Result<(T, Vec<String>), QuilloError> {
        let Some(outer) = self.reads.last() else { return Ok((f(self)?, Vec::new())) };
        let (ctx, scopes) = (outer.ctx, outer.scopes);
        self.reads.push(Reads { ctx, scopes, deps: Vec::new(), impure: false });
        let r = f(self);
        let reads = self.reads.pop().expect("pushed");
        Ok((r?, reads.deps))
    }

    /// `el`'s template, `[tag, [[name, value], ...], [child, ...]]`, adding
    /// its holes to `parts`; `path` is where it is in the block.
    fn block_el(&mut self, el: &HtmlEl, path: &mut Vec<usize>, parts: &mut BlockParts) -> Result<String, QuilloError> {
        self.line = el.line;
        let mut fixed_attrs: Vec<String> = Vec::new();
        let mut at: Option<usize> = None;
        let mut hole = |parts: &mut BlockParts, kind: u8, name: &str, (val, deps): (String, Vec<String>)| {
            let i = *at.get_or_insert_with(|| {
                parts.paths.push(path.clone());
                parts.paths.len() - 1
            });
            parts.holes.push((kind, i, name.to_string()));
            parts.vals.push(val);
            parts.deps.push(deps);
        };
        if let Some(id) = &el.id {
            match fixed(id) {
                Some(l) => fixed_attrs.push(format!("[\"id\", {}]", lit_js(l))),
                None => {
                    let v = self.noted(|c| c.expr(id))?;
                    hole(parts, 0, "id", v);
                }
            }
        }
        let mut classes: Vec<&Expr> = el.classes.iter().collect();
        for (name, v) in &el.attrs {
            if let ("class", AttrVal::Expr(e)) = (name.as_str(), v) {
                classes.push(e);
            }
        }
        if classes.iter().all(|e| matches!(fixed(e), Some(Lit::Str(_)))) {
            let names: Vec<&str> = classes.iter().filter_map(|e| match fixed(e) {
                Some(Lit::Str(s)) if !s.is_empty() => Some(&**s),
                _ => None,
            }).collect();
            if !names.is_empty() {
                fixed_attrs.push(format!("[\"class\", {}]", js_str(&names.join(" "))));
            }
        } else {
            let (js, deps) = self.noted(|c| classes.iter().map(|e| c.expr(e)).collect::<Result<Vec<_>, _>>())?;
            let v = if js.len() == 1 { format!("W.cls1({})", js[0]) } else { format!("W.cls([{}])", js.join(", ")) };
            hole(parts, 1, "", (v, deps));
        }
        for (name, v) in &el.attrs {
            if name == "key" || name == "class" {
                continue;
            }
            if let Some(ev) = name.strip_prefix('@') {
                let f = match v {
                    AttrVal::Expr(e) => self.noted(|c| c.expr(e))?,
                    _ => ("true".to_string(), Vec::new()),
                };
                hole(parts, 2, event_name(ev), f);
                continue;
            }
            // value, checked and selected are the element's live state,
            // set on the element itself rather than as attributes.
            let live = matches!(name.as_str(), "value" | "checked" | "selected");
            match v {
                AttrVal::Bool if !live => fixed_attrs.push(format!("[{}, true]", js_str(name))),
                AttrVal::Expr(e) if !live && fixed(e).is_some() => match fixed(e) {
                    Some(Lit::Nil | Lit::Bool(false)) => {}
                    Some(l) => fixed_attrs.push(format!("[{}, {}]", js_str(name), lit_js(l))),
                    None => unreachable!(),
                },
                _ => {
                    let val = match v {
                        AttrVal::Expr(e) => self.noted(|c| c.expr(e))?,
                        _ => ("true".to_string(), Vec::new()),
                    };
                    if live {
                        self.live_holes += 1;
                    }
                    hole(parts, 0, name, val);
                }
            }
        }
        // Children: in the template when they are all fixed text and
        // elements; otherwise the element's content is a hole.
        let all_fixed = el.children.iter().all(|c| match c {
            HtmlChild::El(e) => inlineable(e),
            HtmlChild::Expr(e) => matches!(fixed(e), Some(Lit::Str(_))),
        });
        let mut kids = Vec::new();
        if all_fixed {
            for (i, c) in el.children.iter().enumerate() {
                match c {
                    HtmlChild::El(e) => {
                        path.push(i);
                        kids.push(self.block_el(e, path, parts)?);
                        path.pop();
                    }
                    HtmlChild::Expr(e) => {
                        if let Some(Lit::Str(s)) = fixed(e) {
                            kids.push(js_str(s));
                        }
                    }
                }
            }
        } else if let [HtmlChild::Expr(e)] = el.children.as_slice() {
            // The only child: an empty text node in the template, which text
            // fills (see the runtime's `block`).
            kids.push("\"\"".to_string());
            let (v, deps) = self.noted(|c| c.expr(e))?;
            path.push(0);
            parts.paths.push(path.clone());
            path.pop();
            parts.holes.push((3, parts.paths.len() - 1, String::new()));
            parts.vals.push(v);
            parts.deps.push(deps);
        } else {
            let (js, deps) = self.noted(|c| {
                el.children
                    .iter()
                    .map(|ch| match ch {
                        HtmlChild::El(e) => c.html(e),
                        HtmlChild::Expr(x) => c.expr(x),
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?;
            hole(parts, 4, "", (format!("[{}]", js.join(", ")), deps));
        }
        Ok(format!("[{}, [{}], [{}]]", js_str(&el.tag), fixed_attrs.join(", "), kids.join(", ")))
    }
}

/// A block's mount function, `($v, $r, $s)`: from `$r`, a fresh copy of
/// the template, it finds the nodes with holes (by `firstChild` and
/// `nextSibling`, each step taken once) and fills the holes from `$v.vals`;
/// `$s` says whether it is SVG.
fn block_mount(parts: &BlockParts) -> String {
    fn node(p: &[usize], known: &mut HashMap<Vec<usize>, String>, code: &mut String) -> String {
        if let Some(v) = known.get(p) {
            return v.clone();
        }
        let (parent, last) = (&p[..p.len() - 1], p[p.len() - 1]);
        // From the nearest sibling already found, else from the parent.
        let from_sibling = (0..last).rev().find_map(|b| {
            let mut q = parent.to_vec();
            q.push(b);
            known.get(&q).map(|v| format!("{}{}", v, ".nextSibling".repeat(last - b)))
        });
        let walk = match from_sibling {
            Some(w) => w,
            None => format!("{}.firstChild{}", node(parent, known, code), ".nextSibling".repeat(last)),
        };
        let v = format!("$n{}", known.len());
        let _ = write!(code, "const {} = {}; ", v, walk);
        known.insert(p.to_vec(), v.clone());
        v
    }
    let mut known = HashMap::new();
    known.insert(Vec::new(), "$r".to_string());
    let mut code = String::new();
    let els: Vec<String> = parts.paths.iter().map(|p| node(p, &mut known, &mut code)).collect();
    let _ = write!(code, "$v.els = [{}]; const $x = $v.vals; ", els.join(", "));
    for (i, (kind, at, name)) in parts.holes.iter().enumerate() {
        let e = &els[*at];
        let _ = match kind {
            0 => write!(code, "setProp({}, {}, $x[{}], undefined); ", e, js_str(name), i),
            1 => write!(code, "if (!isNil($x[{i}]) && $x[{i}] !== \"\") setClass({}, $x[{i}], $s); ", e, i = i),
            2 => write!(code, "listen({}, {}, $x[{}]); ", e, js_str(name), i),
            3 => write!(
                code,
                "if (typeof $x[{i}] === \"string\" || typeof $x[{i}] === \"number\") {e}.data = $x[{i}]; else showNodes($v, {i}, {e}.parentNode, $x[{i}], $s); ",
                i = i,
                e = e
            ),
            _ => write!(code, "showNodes($v, {}, {}, $x[{}], $s); ", i, e, i),
        };
    }
    format!("function ($v, $r, $s) {{ {}}}", code)
}

/// A block's patch function, `($a, $v)`: block `$v` takes over `$a`'s DOM
/// and updates the holes whose values changed (see `updateHole` in the
/// runtime, which it calls for anything but the plain cases).
fn block_patch(parts: &BlockParts) -> String {
    let mut code = String::from("const $e = ($v.els = $a.els), $x = $v.vals, $o = $a.vals; $v.parts = $a.parts; $v.svg = $a.svg; ");
    for (i, (kind, at, name)) in parts.holes.iter().enumerate() {
        let _ = match kind {
            // Inputs' live value follows the state even when unchanged.
            0 if matches!(name.as_str(), "value" | "checked" | "selected") => {
                write!(code, "setProp($e[{}], {}, $x[{}], $o[{}]); ", at, js_str(name), i, i)
            }
            0 => write!(code, "if ($x[{i}] !== $o[{i}]) setProp($e[{}], {}, $x[{i}], $o[{i}]); ", at, js_str(name), i = i),
            1 => write!(code, "if ($x[{i}] !== $o[{i}]) setClass($e[{}], $x[{i}], $v.svg); ", at, i = i),
            2 if name == "click" => write!(code, "$e[{}].__click = $x[{}]; ", at, i),
            2 => write!(code, "listen($e[{}], {}, $x[{}]); ", at, js_str(name), i),
            3 => write!(
                code,
                "if ($x[{i}] !== $o[{i}]) {{ if ((typeof $x[{i}] === \"string\" || typeof $x[{i}] === \"number\") && !($v.parts && $v.parts[{i}])) $e[{at}].data = $x[{i}]; else updateHole($v, {i}, $x[{i}], $o[{i}]); }} ",
                i = i,
                at = at
            ),
            _ => write!(code, "if ($x[{i}] !== $o[{i}]) updateHole($v, {i}, $x[{i}], $o[{i}]); ", i = i),
        };
    }
    format!("function ($a, $v) {{ {}}}", code)
}
