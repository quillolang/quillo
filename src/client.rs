//! Compiles components, and what they use, to JavaScript for the browser.
//!
//! The bundle served at `/_quillo/quillo.js` is the runtime (`client/runtime.js`)
//! followed by the compiled program: every `component`, the functions,
//! views and constants they reference, and a stub for each `server fn`.
//! Code that cannot run in a browser (`request`, `db`, a global the server
//! changes, ...) is a load-time error that names the component.
//!
//! A page gets a bundle of its own: the components it was rendered with and
//! what they use (`bundle_for`), at `/_quillo/b/<hash>.js`, kept by the
//! browser for good since its address changes with its content. A page with
//! no components loads no script, and one with components loads it when it
//! is needed (`island_loader` in server.rs, `island.rs`). The runtime resumes
//! each component on the server's elements (`Inst.adopt` in runtime.js):
//! it renders once from the state the server sent and takes the DOM as it
//! is, making it again only where it differs.

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
    "print", "str", "int", "float", "dec", "len", "type_of", "upper", "lower", "trim", "split", "join", "contains",
    "starts_with", "ends_with", "replace", "slice", "map", "filter", "find", "any", "all", "reduce", "sum", "min",
    "max", "sort", "sort_by", "reverse", "first", "last", "push", "range", "enumerate", "keys", "values", "get", "has",
    "set", "remove", "merge", "err", "is_err", "unwrap", "raw", "escape", "now", "random", "random_int", "uuid", "abs",
    "round", "floor", "ceil", "sqrt", "pow", "unique", "flatten", "zip", "take", "drop", "count", "group_by",
    "index_of", "repeat", "format_number", "url", "navigate", "css", "sql", "parallel", "every", "listen", "receive", "__record", "t", "locale", "js", "js_new", "highlight", "ask", "notify",
];

pub fn bundle(it: &Interp) -> Result<String, QuilloError> {
    bundle_of(it, None)
}

/// The bundle for a page: these components and what they use, no more.
pub fn bundle_for(it: &Interp, roots: &[String]) -> Result<String, QuilloError> {
    bundle_of(it, Some(roots))
}

fn bundle_of(it: &Interp, roots: Option<&[String]>) -> Result<String, QuilloError> {
    let mut c = Compiler::new(it);
    let mut names: Vec<(String, Arc<FnDef>)> = Vec::new();
    for (name, v) in scope_vars(&it.globals) {
        if let Value::Func(f) = v {
            if f.def.component && roots.map_or(true, |r| r.contains(&name)) {
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
    for d in c.defs.iter().chain(c.view_defs.iter()) {
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

/// The script pages with `<quillo-media>` load: the player and the
/// connection it uses, and nothing else of the runtime.
pub fn media_script(base: &str) -> String {
    let program = format!("BASE = {};\nmediaDefine();\n", js_str(base));
    assemble("(function () {", &program, "})();")
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
    for d in c.defs.iter().chain(c.view_defs.iter()) {
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
        // (After `.` a property, except after `...`, a spread.)
        let spread = i >= 3 && chars[i - 3..i] == ['.', '.', '.'];
        if ident(c) && (i == 0 || !ident(chars[i - 1]) && (chars[i - 1] != '.' || spread)) {
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
    /// Component state: a signal, read `x.v` and set with `x.set(v)`.
    State(String),
    /// What follows state (a memo, a row's item): read `x.v`.
    Reactive(String),
    /// A component's or view's parameter, read from its props: `$p.name`.
    Prop(String),
}

struct FnCtx {
    scopes: Vec<HashMap<String, Local>>,
    temps: usize,
    is_async: bool,
    needs_async: bool,
    /// The component or view being compiled, for error messages and `state` rules.
    component: Option<String>,
    /// A view's code (props, no state).
    view: bool,
    /// `state` was declared ahead of the code (a body made again whole).
    hoisted: bool,
}

enum Resolved {
    Local(String, Option<bool>),
    State(String),
    Reactive(String),
    Prop(String),
    /// JS expression and whether calling it waits on the server.
    Global(String, bool),
}

struct Compiler<'a> {
    it: &'a Interp,
    assigned: HashSet<String>,
    /// Globals read from the server's environment, files or database.
    private: HashSet<String>,
    defs: Vec<String>,
    /// Templates (see `block` in the runtime), emitted before everything else.
    blocks: Vec<String>,
    /// Names the component being compiled assigns somewhere: its `let`s of
    /// them stay variables.
    mutable: HashSet<String>,
    /// Whether what was just compiled reads something that can change
    /// (state, a memo, a prop, a row's item, `url()`, the component's own
    /// functions): then it is made a memo, an effect or a hole that follows.
    reactive: bool,
    /// The row signal of the `for` row whose template is being compiled
    /// (`row` until that template takes it, `cur_row` while it is
    /// compiled), what the hole being compiled read (`read_log`: signals'
    /// JS, "*" for what may read anything), and which of the template's
    /// values read nothing but the row (`row_mask`): those the row updates
    /// itself, without an effect each (see `direct` in the runtime).
    row: Option<String>,
    cur_row: Option<String>,
    read_log: Option<Vec<String>>,
    row_mask: u32,
    /// Whether the name just resolved belongs to a component's (or view's) code.
    last_here: bool,
    /// Views whose `<Name/>` form (code once, props as getters) is made.
    views: HashSet<String>,
    view_defs: Vec<String>,
    /// Globals compiled (or being compiled): JS name and whether they are async.
    globals: HashMap<String, (String, bool)>,
    ctxs: Vec<FnCtx>,
    counter: usize,
    line: u32,
    /// The component or function being compiled, for error messages.
    what: String,
    /// The program's own types whose shapes are in the bundle (`W.type`).
    types: HashSet<String>,
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
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) | Stmt::Return(None) => {}
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
    each_expr_in(e, f, true)
}

/// The same; with `fns` false, not inside the functions in it.
fn each_expr_in(e: &Expr, f: &mut dyn FnMut(&Expr), fns: bool) {
    f(e);
    let stmts = |ss: &[Stmt], f: &mut dyn FnMut(&Expr)| {
        for st in ss {
            match st {
                Stmt::Let(_, x, _) | Stmt::Assign(_, _, x, _) | Stmt::Expr(x) | Stmt::State(_, x, _) | Stmt::Return(Some(x)) => {
                    each_expr_in(x, f, fns)
                }
                Stmt::Fn(d) if fns => each_expr_in(&d.body, f, fns),
                Stmt::Fn(_) => {}
                Stmt::Route(r) => each_expr_in(&r.body, f, fns),
                Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) | Stmt::Return(None) => {}
            }
        }
    };
    match e {
        Expr::Block(ss) => stmts(ss, f),
        Expr::Lambda(d) if fns => each_expr_in(&d.body, f, fns),
        Expr::Lambda(_) => {}
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => parts.iter().for_each(|p| if let TPart::Expr(x) = p { each_expr_in(x, f, fns) }),
        Expr::List(xs) => xs.iter().for_each(|x| each_expr_in(x, f, fns)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| each_expr_in(x, f, fns)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => each_expr_in(x, f, fns),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            each_expr_in(a, f, fns);
            each_expr_in(b, f, fns)
        }
        Expr::Call(c, args, _) => {
            each_expr_in(c, f, fns);
            args.iter().for_each(|x| each_expr_in(x, f, fns))
        }
        Expr::If(c, a, b) => {
            each_expr_in(c, f, fns);
            each_expr_in(a, f, fns);
            if let Some(b) = b {
                each_expr_in(b, f, fns)
            }
        }
        Expr::Match(x, arms, _) => {
            each_expr_in(x, f, fns);
            for a in arms {
                if let Some(g) = &a.guard {
                    each_expr_in(g, f, fns)
                }
                each_expr_in(&a.body, f, fns)
            }
        }
        Expr::For(_, i, b, _) => {
            each_expr_in(i, f, fns);
            each_expr_in(b, f, fns)
        }
        Expr::Reply(x, b, _) => {
            each_expr_in(x, f, fns);
            if let Some(b) = b {
                each_expr_in(b, f, fns)
            }
        }
        Expr::Html(el, _) => each_html_expr(el, f, fns),
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

fn each_html_expr(el: &HtmlEl, f: &mut dyn FnMut(&Expr), fns: bool) {
    for x in el.id.iter().chain(el.classes.iter()) {
        each_expr_in(x, f, fns)
    }
    for (_, v) in &el.attrs {
        if let AttrVal::Expr(x) | AttrVal::Spread(x) = v {
            each_expr_in(x, f, fns)
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(e) => each_html_expr(e, f, fns),
            HtmlChild::Expr(x) => each_expr_in(x, f, fns),
        }
    }
}

/// Whether `e` is markup, or may give markup (a loop, a branch).
fn has_markup(e: &Expr) -> bool {
    match e {
        Expr::Html(..) | Expr::For(..) => true,
        Expr::If(_, a, b) => has_markup(a) || b.as_deref().map_or(false, has_markup),
        Expr::Match(_, arms, _) => arms.iter().any(|a| has_markup(&a.body)),
        Expr::Block(ss) => matches!(ss.last(), Some(Stmt::Expr(x)) if has_markup(x)),
        _ => false,
    }
}

/// Builtins whose results may be private to the server: its environment,
/// its files, its database, other services.
const PRIVATE_SOURCES: &[&str] = &["env", "read_file", "file", "db", "http"];

/// Top-level variables whose value comes, directly or through other such
/// variables, from `PRIVATE_SOURCES`, a service, or a function that uses
/// any of them (`let CONFIG = load_config()`). Components get other
/// globals' values written into the browser bundle, which anyone can
/// download; these they must ask a `server fn` for.
fn collect_private(program: &[Arc<Vec<Stmt>>]) -> HashSet<String> {
    let mut services: HashSet<String> = HashSet::new();
    for stmts in program {
        for st in stmts.iter() {
            if let Stmt::Service(name, ..) = st {
                services.insert(name.clone());
            }
        }
    }
    let sources: HashSet<&str> = PRIVATE_SOURCES.iter().copied().collect();
    let mut vars: HashSet<String> = HashSet::new();
    // Functions that use what is private: calling one gives a private value.
    // They can be called as methods too (`"KEY".env`).
    let mut fns: HashSet<String> = ["env", "read_file"].iter().map(|s| s.to_string()).collect();
    // Whether `e` uses a private name (other than the `shadowed` ones), its
    // functions' bodies included when `inside` is set.
    let uses = |e: &Expr, inside: bool, shadowed: &[String], vars: &HashSet<String>, fns: &HashSet<String>| {
        let mut found = false;
        each_expr_in(
            e,
            &mut |x| match x {
                Expr::Ident(n, _) if !shadowed.contains(n) => {
                    found |= sources.contains(n.as_str()) || services.contains(n) || vars.contains(n) || fns.contains(n)
                }
                Expr::Field(_, n, _) => found |= fns.contains(n),
                _ => {}
            },
            inside,
        );
        found
    };
    // What a variable's functions use, as before: a function value is made
    // in the browser from its definition, and the server fns it calls stay
    // on the server.
    let uses_in_fns = |e: &Expr, vars: &HashSet<String>| {
        let mut found = false;
        each_expr(e, &mut |x| {
            if let Expr::Ident(n, _) = x {
                found |= sources.contains(n.as_str()) || vars.contains(n);
            }
        });
        found
    };
    loop {
        let before = vars.len() + fns.len();
        for stmts in program {
            for st in stmts.iter() {
                match st {
                    Stmt::Let(p, e, _) if uses(e, false, &[], &vars, &fns) || uses_in_fns(e, &vars) => {
                        let mut names = Vec::new();
                        pattern_names(p, &mut names);
                        vars.extend(names);
                    }
                    Stmt::Fn(d) if !fns.contains(&d.name) => {
                        // Its parameters are its own, whatever their names.
                        let params: Vec<String> = d.params.iter().map(|p| p.name.clone()).collect();
                        if uses(&d.body, true, &params, &vars, &fns) {
                            fns.insert(d.name.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        if vars.len() + fns.len() == before {
            return vars;
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

/// A pattern as JS testing `v` (a JS variable or path) directly, for the
/// simple kinds: names, `_`, literals and lists of them. The test ("" for
/// none) and each name's JS; None for the others, which `W.match` does.
fn simple_pattern(p: &Pattern, v: &str, out: &mut Vec<(String, String)>) -> Option<String> {
    match p {
        Pattern::Wild => Some(String::new()),
        Pattern::Bind(n) => {
            if out.iter().any(|(m, _)| m == n) {
                return None;
            }
            out.push((n.clone(), v.to_string()));
            Some(String::new())
        }
        Pattern::Lit(l) => Some(format!("W.eq({}, {})", lit_js(l), v)),
        Pattern::List(ps, rest) => {
            let k = ps.len();
            let mut conds = vec![format!("Array.isArray({})", v), format!("{}.length {} {}", v, if rest.is_none() { "===" } else { ">=" }, k)];
            for (i, q) in ps.iter().enumerate() {
                let c = simple_pattern(q, &format!("{}[{}]", v, i), out)?;
                if !c.is_empty() {
                    conds.push(c);
                }
            }
            if let Some(Some(n)) = rest {
                if out.iter().any(|(m, _)| m == n) {
                    return None;
                }
                out.push((n.clone(), format!("{}.slice({})", v, k)));
            }
            Some(conds.join(" && "))
        }
        _ => None,
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
        // Past 2^53 a JS number is not whole: a BigInt, as the runtime has them.
        Lit::Int(n) if n.unsigned_abs() > (1 << 53) - 1 => format!("{}n", n),
        Lit::Int(n) => n.to_string(),
        Lit::Float(f) if f.is_finite() => format!("{:?}", f),
        Lit::Dec(d) => format!("new W.Dec({}n, {})", d.mantissa(), d.scale()),
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
            mutable: HashSet::new(),
            reactive: false,
            row: None,
            cur_row: None,
            read_log: None,
            row_mask: 0,
            last_here: false,
            views: HashSet::new(),
            view_defs: Vec::new(),
            globals: HashMap::new(),
            ctxs: Vec::new(),
            counter: 0,
            line: 0,
            what: String::new(),
            types: HashSet::new(),
        }
    }

    /// Puts the shape of the program's own type `name` in the bundle (and
    /// of the types it names), for `User(...)`, annotations and patterns.
    fn need_type(&mut self, name: &str) -> Result<(), QuilloError> {
        let Some(td) = self.it.types.get(name).cloned() else { return Ok(()) };
        if !self.types.insert(name.to_string()) {
            return Ok(());
        }
        let shape = if td.is_record() {
            let mut fs = Vec::new();
            for (k, ft) in td.ty.fields.as_deref().unwrap_or_default() {
                let t = self.ty_shape(ft)?;
                let d = match td.defaults.iter().find(|(d, _)| d == k) {
                    // Made for each record, in the program's top-level scope.
                    Some((_, e)) => {
                        let def = FnDef {
                            name: format!("{}.{}", td.name, k),
                            params: Vec::new(),
                            ret: None,
                            err: None,
                            effects: None,
                            server: false,
                            component: false,
                            line: td.line,
                            body: e.clone(),
                            nesting: 0,
                            hot: Default::default(),
                        };
                        let saved = std::mem::take(&mut self.ctxs);
                        let r = self.function(&def);
                        self.ctxs = saved;
                        let (code, is_async) = r?;
                        if is_async {
                            self.line = td.line;
                            return self.err(format!("the default of `{}` in `{}` waits on the server, which a record made in the browser cannot", k, td.name));
                        }
                        code
                    }
                    None => "null".into(),
                };
                fs.push(format!("[{}, {}, {}]", js_str(k), t, d));
            }
            format!("{{r: [{}]}}", fs.join(", "))
        } else {
            format!("{{a: {}}}", self.ty_shape(&td.ty)?)
        };
        self.defs.push(format!("W.type({}, {});", js_str(name), shape));
        Ok(())
    }

    /// A type for the runtime's record checks: names, null allowed, what a
    /// list holds and a map's fields.
    fn ty_shape(&mut self, t: &Ty) -> Result<String, QuilloError> {
        for n in &t.names {
            self.need_type(n)?;
        }
        let e = match &t.elem {
            Some(e) => self.ty_shape(e)?,
            None => "null".into(),
        };
        let f = match &t.fields {
            Some(fs) => {
                let mut out = Vec::new();
                for (k, ft) in fs {
                    out.push(format!("[{}, {}]", js_str(k), self.ty_shape(ft)?));
                }
                format!("[{}]", out.join(", "))
            }
            None => "null".into(),
        };
        Ok(format!("{{n: [{}], o: {}, e: {}, f: {}, t: {}}}", t.names.iter().map(|n| js_str(n)).collect::<Vec<_>>().join(", "), t.optional, e, f, js_str(&t.text())))
    }

    /// The shapes of the types an annotation names.
    fn need_types_in(&mut self, t: &Ty) -> Result<(), QuilloError> {
        if t.names.iter().any(|n| self.it.types.contains_key(n)) || t.elem.is_some() || t.fields.is_some() {
            self.ty_shape(t)?;
        }
        Ok(())
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

    /// Declares `name` as `local` in the innermost scope.
    fn declare_as(&mut self, name: &str, local: Local) {
        self.ctx().scopes.last_mut().unwrap().insert(name.to_string(), local);
    }

    fn resolve(&mut self, name: &str) -> Result<Resolved, QuilloError> {
        let mut found = None;
        'outer: for (ci, ctx) in self.ctxs.iter().enumerate().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(js, f)) => found = Some((ci, Resolved::Local(js.clone(), *f))),
                    Some(Local::State(js)) => found = Some((ci, Resolved::State(js.clone()))),
                    Some(Local::Reactive(js)) => found = Some((ci, Resolved::Reactive(js.clone()))),
                    Some(Local::Prop(n)) => found = Some((ci, Resolved::Prop(n.clone()))),
                    None => continue,
                }
                break 'outer;
            }
        }
        if let Some((ci, r)) = found {
            // What it gives can change, and so can what a function reading
            // it gives (`find(xs, (x) => x.id == wanted)`), or a function of
            // the component's own.
            self.last_here = self.ctxs[ci].component.is_some();
            if matches!(r, Resolved::State(_) | Resolved::Reactive(_) | Resolved::Prop(_)) {
                self.reactive = true;
            }
            if let Some(log) = &mut self.read_log {
                match &r {
                    Resolved::State(js) | Resolved::Reactive(js) => log.push(js.clone()),
                    Resolved::Prop(_) => log.push("*".into()),
                    _ => {}
                }
            }
            return Ok(r);
        }
        self.last_here = false;
        let (js, is_async) = self.global(name, self.line)?;
        Ok(Resolved::Global(js, is_async))
    }

    /// A top-level name used by browser code: compiled once, on first use.
    fn global(&mut self, name: &str, line: u32) -> Result<(String, bool), QuilloError> {
        if let Some(g) = self.globals.get(name) {
            return Ok(g.clone());
        }
        // What a global reads is its own, not the code that first used it.
        let saved = std::mem::replace(&mut self.reactive, false);
        let log = self.read_log.take();
        let rows = (self.row.take(), self.cur_row.take(), std::mem::replace(&mut self.row_mask, 0));
        let r = self.global_inner(name, line);
        self.reactive = saved;
        self.read_log = log;
        (self.row, self.cur_row, self.row_mask) = rows;
        r
    }

    fn global_inner(&mut self, name: &str, line: u32) -> Result<(String, bool), QuilloError> {
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
                    // `parallel` waits for what it calls, and `ask` for an
                    // answer: they are awaited.
                    let r = (format!("B.{}", b), *b == "parallel" || *b == "ask" || *b == "receive");
                    self.globals.insert(name.to_string(), r.clone());
                    Ok(r)
                } else if b.starts_with("native:") {
                    self.err(format!("`{}` is Rust, which runs on the server, not in the browser; call it from a `server fn`", crate::native::display_name(b)))
                } else {
                    self.err(format!("`{}` is not available in the browser; call it from a `server fn`", b))
                }
            }
            // The module, not a map of the program's own by its name.
            Value::Map(m)
                if matches!(name, "json" | "math" | "stats" | "vec" | "date" | "money" | "channel")
                    && m.values().any(|x| matches!(x, Value::Builtin(b, _) if b.strip_prefix(name).map_or(false, |r| r.starts_with('.')))) =>
            {
                let r = (format!("B.{}", name), false);
                self.globals.insert(name.to_string(), r.clone());
                Ok(r)
            }
            Value::Map(m) if matches!(name, "db" | "http" | "udp" | "media") && m.values().all(|x| matches!(x, Value::Builtin(..))) => {
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
                // `User(...)`: the record's shape goes with it.
                if self.it.types.contains_key(name) {
                    self.need_type(name)?;
                }
                // Recursive references see the function as not async until
                // known. If it turns out async, it is compiled again with
                // them awaited, and so is what it compiled meanwhile, which
                // may call it back.
                let (defs_before, known): (usize, HashSet<String>) = (self.defs.len(), self.globals.keys().cloned().collect());
                let (views_before, view_defs_before) = (self.views.clone(), self.view_defs.len());
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
                        self.views = views_before.clone();
                        self.view_defs.truncate(view_defs_before);
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
                        "`{}` is read from the server's environment, files, database or a service, and would be \
                         sent to every browser; pass what the page needs as a prop or read it with a `server fn`",
                        name
                    ));
                }
                if let Some(bad) = not_sendable(other) {
                    // A constant holding functions (a table of rules, say)
                    // is made in the browser too, from its definition.
                    if let Some(e) = self.definition(name) {
                        return self.global_made(name, &js, e, line);
                    }
                    return self.err(format!("`{}` is {}, which cannot be sent to the browser", name, bad));
                }
                self.defs.push(format!("{} = {};", js, other.to_json()));
                let r = (js, false);
                self.globals.insert(name.to_string(), r.clone());
                Ok(r)
            }
        }
    }

    /// What `let name = ...` at the top of the program gave `name`, for a
    /// name bound once, alone.
    fn definition(&self, name: &str) -> Option<Expr> {
        let mut found = None;
        for stmts in self.it.program.iter() {
            for st in stmts.iter() {
                if let Stmt::Let(Pattern::Bind(n), e, _) = st {
                    if n == name {
                        if found.is_some() {
                            return None;
                        }
                        found = Some(e.clone());
                    }
                }
            }
        }
        found
    }

    /// A constant compiled from its definition: a function of nothing,
    /// called once when the bundle loads.
    fn global_made(&mut self, name: &str, js: &str, e: Expr, line: u32) -> Result<(String, bool), QuilloError> {
        let def = FnDef {
            name: name.to_string(),
            params: Vec::new(),
            ret: None,
            err: None,
            effects: None,
            server: false,
            component: false,
            line,
            body: e,
            nesting: 0,
            hot: Default::default(),
        };
        // Until made, references to it (from functions it holds) find it.
        self.globals.insert(name.to_string(), (js.to_string(), false));
        let saved_ctxs = std::mem::take(&mut self.ctxs);
        let saved_what = std::mem::replace(&mut self.what, format!("`{}`", name));
        let res = self.function(&def);
        self.ctxs = saved_ctxs;
        self.what = saved_what;
        let (code, is_async) = res?;
        if is_async {
            return self.err(format!("`{}` waits on the server to be made, so the browser cannot make it", name));
        }
        self.defs.push(format!("{} = ({})();", js, code));
        Ok((js.to_string(), false))
    }

    fn new_ctx(&mut self, component: Option<String>, is_async: bool) {
        self.ctxs.push(FnCtx { scopes: vec![HashMap::new()], temps: 0, is_async, needs_async: false, component, view: false, hoisted: false });
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
                self.need_types_in(ty)?;
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
        for t in def.ret.iter().chain(def.err.iter()) {
            self.need_types_in(t)?;
        }
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

    /// A component: its parameters' names and its code.
    fn component(&mut self, def: &FnDef) -> Result<(String, bool), QuilloError> {
        let code = self.setup(def, false)?;
        Ok((format!("[{}], {}", def.params.iter().map(|p| js_str(&p.name)).collect::<Vec<_>>().join(", "), code), false))
    }

    /// A component's code, or (with `view`) a view's for `<Name .../>`: it
    /// runs once, with its props as getters: `function ($i, $p)`, a view's
    /// `function ($p)`.
    fn setup(&mut self, def: &FnDef, view: bool) -> Result<String, QuilloError> {
        self.new_ctx(Some(def.name.clone()), false);
        self.ctx().view = view;
        let saved = (std::mem::take(&mut self.mutable), std::mem::replace(&mut self.reactive, false));
        let res = self.setup_inner(def);
        let ctx = self.ctxs.pop().unwrap();
        self.mutable = saved.0;
        self.reactive = saved.1;
        let body = res?;
        Ok(format!("function ({}) {{\n{}{}}}", if view { "$p" } else { "$i, $p" }, Self::temps_decl(ctx.temps), body))
    }

    fn setup_inner(&mut self, def: &FnDef) -> Result<String, QuilloError> {
        let fname = js_str(&def.name);
        let mut out = String::new();
        let mut defaults = Vec::new();
        for p in &def.params {
            if let Some(d) = &p.default {
                let (v, _) = self.reactive_expr(d)?;
                defaults.push(format!("{}: () => ({})", js_str(&p.name), v));
            }
            self.declare_as(&p.name, Local::Prop(p.name.clone()));
        }
        if !defaults.is_empty() {
            let _ = writeln!(out, "$p = W.props($p, {{{}}});", defaults.join(", "));
        }
        for p in &def.params {
            if p.default.is_none() {
                let _ = writeln!(out, "if ($p.{} === undefined) W.missing({}, {});", p.name, js_str(&p.name), fname);
            }
            if let Some(ty) = &p.ty {
                self.need_types_in(ty)?;
                let _ = writeln!(out, "W.argty($p.{}, {}, {}, {});", p.name, ty_js(&Some(ty.clone())), js_str(&p.name), fname);
            }
        }
        // A body that can return early, or changes its own variables as it
        // runs, is made again whole when what it read changes (its state
        // stays).
        let mut exits = false;
        let mut changed = HashSet::new();
        each_expr_in(
            &def.body,
            &mut |e| match e {
                Expr::Try(_) => exits = true,
                Expr::Block(ss) => {
                    for st in ss {
                        match st {
                            Stmt::Return(_) => exits = true,
                            Stmt::Assign(n, ..) => {
                                changed.insert(n.clone());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            false,
        );
        let states: HashSet<&String> = match &def.body {
            Expr::Block(ss) => ss.iter().filter_map(|st| if let Stmt::State(n, ..) = st { Some(n) } else { None }).collect(),
            _ => HashSet::new(),
        };
        expr_assigned(&def.body, &mut self.mutable);
        if exits || changed.iter().any(|n| !states.contains(n)) {
            self.ctx().hoisted = true;
            let mut copy = String::new();
            for p in &def.params {
                let js = self.declare(&p.name, None);
                let _ = writeln!(copy, "let {} = $p.{};", js, p.name);
            }
            let body = self.body(&def.body)?;
            let _ = writeln!(
                out,
                "return W.frag([() => {{\n{}try {{\n{}}} catch ($e) {{ if ($e instanceof W.Ret) return $e.v; throw $e; }}\n}}]);",
                copy, body
            );
            return Ok(out);
        }
        match &def.body {
            Expr::Block(ss) => {
                self.push_scope();
                let r = self.setup_stmts(ss);
                self.pop_scope();
                out.push_str(&r?);
            }
            e => {
                let v = self.markup(e)?;
                let _ = writeln!(out, "return {};", v);
            }
        }
        Ok(out)
    }

    /// Statements of code that runs once: a `let` that reads what changes
    /// is a memo, a statement that does an effect, and the last value is
    /// markup.
    fn setup_stmts(&mut self, stmts: &[Stmt]) -> Result<String, QuilloError> {
        let mut body = String::new();
        let mut decls: Vec<String> = Vec::new();
        let n = stmts.len();
        for (i, st) in stmts.iter().enumerate() {
            let last = i + 1 == n;
            match st {
                Stmt::Expr(e) if last => {
                    let v = self.markup(e)?;
                    let _ = writeln!(body, "return {};", v);
                }
                Stmt::Expr(e) => {
                    let (v, r) = self.reactive_expr(e)?;
                    if r {
                        let _ = writeln!(body, "W.effect(() => {{ {}; }});", v);
                    } else {
                        let _ = writeln!(body, "{};", v);
                    }
                }
                Stmt::Let(p, e, line) if !matches!(e, Expr::Lambda(_)) && !matches!(p, Pattern::Bind(n) if self.mutable.contains(n)) => {
                    self.line = *line;
                    let (v, r) = self.reactive_expr(e)?;
                    if !r {
                        self.stmt(st, last, &mut body, &mut decls)?;
                        continue;
                    }
                    if let Pattern::Bind(name) = p {
                        let m = self.fresh(name);
                        let _ = writeln!(body, "const {} = W.memo(() => ({}));", m, v);
                        self.declare_as(name, Local::Reactive(m));
                    } else {
                        let pat = self.pattern(p)?;
                        let b = self.fresh("b");
                        let _ = writeln!(
                            body,
                            "const {} = W.memo(() => {{ const $v = {}, $b = W.match({}, $v); if ($b === null) W.fail(\"`let` pattern does not match \" + W.short($v)); return $b; }});",
                            b, v, pat
                        );
                        let mut names = Vec::new();
                        pattern_names(p, &mut names);
                        for n in names {
                            let m = self.fresh(&n);
                            let _ = writeln!(body, "const {} = W.memo(() => {}.v[{}]);", m, b, js_str(&n));
                            self.declare_as(&n, Local::Reactive(m));
                        }
                    }
                    if last {
                        body.push_str("return null;\n");
                    }
                }
                _ => self.stmt(st, last, &mut body, &mut decls)?,
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
            self.stmt(s, i + 1 == n, &mut body, &mut decls)?;
        }
        if stmts.is_empty() {
            body.push_str("return null;\n");
        }
        decls.sort();
        decls.dedup();
        let head = if decls.is_empty() { String::new() } else { format!("let {};\n", decls.join(", ")) };
        Ok(head + &body)
    }

    /// One statement into `body` (the last returns its value); variables it
    /// declares into `decls`.
    fn stmt(&mut self, s: &Stmt, last: bool, body: &mut String, decls: &mut Vec<String>) -> Result<(), QuilloError> {
        {
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
                        let t = self.temp();
                        let mut names = Vec::new();
                        if let Some(cond) = simple_pattern(p, &t, &mut names) {
                            let _ = writeln!(body, "{} = {};", t, v);
                            if !cond.is_empty() {
                                let _ = writeln!(body, "if (!({})) W.fail(\"`let` pattern does not match \" + W.short({}));", cond, t);
                            }
                            for (n, at) in names {
                                let js = self.declare(&n, None);
                                decls.push(js.clone());
                                let _ = writeln!(body, "{} = {};", js, at);
                            }
                        } else {
                            let pat = self.pattern(p)?;
                            let _ = writeln!(body, "{} = W.match({}, {});", t, pat, v);
                            let _ = writeln!(body, "if ({} === null) W.fail(\"`let` pattern does not match \" + W.short({}));", t, v);
                            let mut all = Vec::new();
                            pattern_names(p, &mut all);
                            for n in all {
                                let js = self.declare(&n, None);
                                decls.push(js.clone());
                                let _ = writeln!(body, "{} = {}[{}];", js, t, js_str(&n));
                            }
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
                        Resolved::State(js) => format!("{}.v", js),
                        Resolved::Reactive(_) | Resolved::Prop(_) => {
                            return self.err(format!(
                                "cannot assign to `{}`: it follows other values (a prop, a loop's item, a `let` of state); keep it in state: state {} = ...",
                                name, name
                            ))
                        }
                        Resolved::Global(..) => {
                            return self.err(format!(
                                "cannot assign to `{}` in the browser: it is a server variable (use `state {}` in the component, or a `server fn`)",
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
                        Resolved::State(js) => {
                            let _ = writeln!(body, "{}.set({});", js, v);
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
                    if self.ctxs.len() != 1 || self.ctx().scopes.len() != 2 || self.ctx().component.is_none() || self.ctx().view {
                        return self.err("`state` can only be declared directly in a component's body");
                    }
                    let sig = self.fresh(name);
                    // `state x = live(load, ...args)`: a subscription.
                    if let Some((fname, args)) = self.live_call(e)? {
                        let mut argv = Vec::new();
                        for a in args {
                            argv.push(self.reactive_expr(a)?.0);
                        }
                        let _ = writeln!(body, "const {} = W.live($i, {}, {}, () => [{}]);", sig, js_str(name), js_str(&fname), argv.join(", "));
                        self.declare_as(name, Local::State(sig));
                        if last {
                            body.push_str("return null;\n");
                        }
                        return Ok(());
                    }
                    let saved_reactive = self.reactive;
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
                        self.ctxs.push(FnCtx { scopes: outer.clone(), temps: 0, is_async: true, needs_async: false, component: None, view: false, hoisted: false });
                        self.ctxs.last_mut().unwrap().scopes.push(HashMap::new());
                        let again = self.expr(e);
                        init_ctx = self.ctxs.pop().unwrap();
                        init = again?;
                    }
                    self.ctxs[0].scopes = outer;
                    self.reactive = saved_reactive;
                    let _ = writeln!(
                        body,
                        "const {} = $i.sig({}, {}() => {{ {}return {}; }});",
                        sig,
                        js_str(name),
                        if init_ctx.is_async { "async " } else { "" },
                        Self::temps_decl(init_ctx.temps),
                        init
                    );
                    self.declare_as(name, Local::State(sig));
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
                Stmt::Service(_, _, _, _, line) => {
                    self.line = *line;
                    return self.err("`service` must be at the top level");
                }
                Stmt::Type(td) => {
                    self.line = td.line;
                    return self.err("`type` must be at the top level");
                }
            }
        }
        Ok(())
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
            _ => return self.err("`live` needs a server fn by name: state items = live(load_items, ...)"),
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
                    Resolved::State(js) | Resolved::Reactive(js) => format!("{}.v", js),
                    Resolved::Prop(n) => format!("$p.{}", n),
                    // Passed around, it could be called with any name.
                    Resolved::Global(js, _) if js == "B.js" || js == "B.js_new" => {
                        return self.err(format!("`{}` can only be called, with the name of what it calls written out", n));
                    }
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
            Expr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b, line) if self.state_of(a).is_some() || self.state_of(b).is_some() => {
                self.line = *line;
                // `x == state`: what reads it hears only when the state
                // becomes x or stops being it (see `is` in the runtime).
                let (s, other) = match self.state_of(a) {
                    Some(s) => {
                        let _ = self.resolve(match &**a { Expr::Ident(n, _) => n, _ => unreachable!() })?;
                        (s, self.expr(b)?)
                    }
                    None => {
                        let x = self.expr(a)?;
                        let _ = self.resolve(match &**b { Expr::Ident(n, _) => n, _ => unreachable!() })?;
                        (self.state_of(b).unwrap(), x)
                    }
                };
                format!("{}W.is({}, {})", if matches!(op, BinOp::Ne) { "!" } else { "" }, s, other)
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
                let mut fb = self.fallback(name);
                // `x.js` is no call of `js` with the name x holds.
                if fb == "B.js" || fb == "B.js_new" {
                    fb = "null".into();
                }
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
                self.push_scope();
                let r = self.loop_head(pat, iter).and_then(|(head, pre, pat, elem)| {
                    let binds = self.bind_pattern(pat, &elem, "`for` pattern does not match ")?;
                    Ok((head, pre + &binds, self.expr(body)?))
                });
                self.pop_scope();
                let (head, binds, b) = r?;
                let inner = format!("const $out = [];\n{} {{ {}$out.push({}); }}\nreturn $out;\n", head, binds, b);
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

    /// The signal of the state `e` names, if it is a component's state.
    fn state_of(&self, e: &Expr) -> Option<String> {
        let Expr::Ident(n, _) = e else { return None };
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                if let Some(l) = scope.get(n) {
                    return if let Local::State(js) = l { Some(js.clone()) } else { None };
                }
            }
        }
        None
    }

    /// The head of a loop over `iter` (in the loop's scope), what its body
    /// starts with, the pattern each item is matched with, and the item's
    /// JS. `enumerate(xs)` and `range(a, b)` (or `a..b`) are looped over by
    /// number, without making the list.
    fn loop_head<'p>(&mut self, pat: &'p Pattern, iter: &Expr) -> Result<(String, String, &'p Pattern, String), QuilloError> {
        let builtin = |c: &mut Self, callee: &Expr, name: &str| -> Result<bool, QuilloError> {
            Ok(matches!(callee, Expr::Ident(n, _) if n == name) && matches!(c.resolve(name)?, Resolved::Global(js, _) if js == format!("B.{}", name)))
        };
        let plain = |c: &mut Self| -> Result<(String, String, &'p Pattern, String), QuilloError> {
            let l = c.expr(iter)?;
            Ok((format!("for (const $x of W.iter({}))", l), String::new(), pat, "$x".into()))
        };
        match iter {
            Expr::Call(callee, args, _) if args.len() == 1 && builtin(self, callee, "enumerate")? => {
                let Pattern::List(ps, None) = pat else { return plain(self) };
                let [i, x] = ps.as_slice() else { return plain(self) };
                let mut names = Vec::new();
                if simple_pattern(i, "$k", &mut names).map_or(true, |c| !c.is_empty()) {
                    return plain(self);
                }
                let l = self.expr(&args[0])?;
                let mut pre = String::new();
                for (n, at) in names {
                    let js = self.declare(&n, None);
                    let _ = write!(pre, "const {} = {}; ", js, at);
                }
                let _ = write!(pre, "const $x = $l[$k]; ");
                Ok((format!("const $l = W.enumlist({});\nfor (let $k = 0; $k < $l.length; $k++)", l), pre, x, "$x".into()))
            }
            Expr::Call(callee, args, _) if (args.len() == 1 || args.len() == 2) && builtin(self, callee, "range")? => {
                let a = self.expr(&args[0])?;
                let b = if args.len() == 2 { self.expr(&args[1])? } else { "undefined".into() };
                Ok((format!("const $r = W.span({}, {}, false);\nfor (let $x = $r[0]; $x < $r[1]; $x++)", a, b), String::new(), pat, "$x".into()))
            }
            Expr::Binary(BinOp::Range, a, b, _) => {
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                Ok((format!("const $r = W.span({}, {}, true);\nfor (let $x = $r[0]; $x < $r[1]; $x++)", a, b), String::new(), pat, "$x".into()))
            }
            _ => plain(self),
        }
    }

    /// Binds `p`'s names to what `v` (a JS variable) holds, failing with
    /// `what` when it does not match.
    fn bind_pattern(&mut self, p: &Pattern, v: &str, what: &str) -> Result<String, QuilloError> {
        let mut names = Vec::new();
        let mut out = String::new();
        if let Some(cond) = simple_pattern(p, v, &mut names) {
            if !cond.is_empty() {
                let _ = write!(out, "if (!({})) W.fail({} + W.short({})); ", cond, js_str(what), v);
            }
            for (n, at) in names {
                let js = self.declare(&n, None);
                let _ = write!(out, "const {} = {}; ", js, at);
            }
            return Ok(out);
        }
        let pj = self.pattern(p)?;
        let _ = write!(out, "const $b = W.match({}, {}); if ($b === null) W.fail({} + W.short({})); ", pj, v, js_str(what), v);
        let mut all = Vec::new();
        pattern_names(p, &mut all);
        for n in all {
            let js = self.declare(&n, None);
            let _ = write!(out, "const {} = $b[{}]; ", js, js_str(&n));
        }
        Ok(out)
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
                    Some(Local::State(js) | Local::Reactive(js)) => return format!("{}.v", js),
                    Some(Local::Prop(n)) => return format!("$p.{}", n),
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
                Resolved::State(js) | Resolved::Reactive(js) => (format!("{}.v", js), false),
                Resolved::Prop(n) => (format!("$p.{}", n), false),
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
            if fb == "B.js" || fb == "B.js_new" {
                return self.err(format!("`{}` needs the name of what it calls written out, like {}(\"Chart\", ...)", name, name));
            }
            for a in args {
                argv.push(self.expr(a)?);
            }
            let is_async = self.is_async_name(name);
            let call = format!("W.mcall({}, {}, {}, [{}])", recv, js_str(name), fb, argv.join(", "));
            return self.finish_call(name, is_async, call);
        }
        let (f, is_async) = self.callee_value(callee)?;
        // The component's own functions, the program's and url() may read
        // what changes: what calls them follows it.
        if matches!(callee, Expr::Ident(..)) && (self.last_here || f.starts_with("G[") || f == "B.url") {
            self.reactive = true;
            if let Some(log) = &mut self.read_log {
                log.push("*".into());
            }
        }
        if f == "B.js" || f == "B.js_new" {
            // `name | js(...)` would take the name from the pipeline.
            let name = if argv.is_empty() { args.first() } else { None };
            self.js_name(&f[2..], name)?;
        }
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
        let banned = [
            "eval",
            "Function",
            "setTimeout",
            "setInterval",
            "execScript",
            "insertAdjacentHTML",
            "setHTMLUnsafe",
            "parseHTMLUnsafe",
            "createContextualFragment",
            "write",
            "writeln",
            "constructor",
            "__proto__",
        ];
        let last = path.rsplit('.').next().unwrap_or("");
        if banned.contains(&last) {
            return self.err(format!("`{}` does not call {}: it runs text as code", f, path));
        }
        Ok(())
    }

    fn is_async_name(&mut self, name: &str) -> bool {
        for ctx in self.ctxs.iter().rev() {
            for scope in ctx.scopes.iter().rev() {
                match scope.get(name) {
                    Some(Local::Var(_, f)) => return f.unwrap_or(false),
                    Some(_) => return false,
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
                 (@click={{() => {{ ... }}}}) or to set state: state x = {}(...)",
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
            Pattern::Type(t) => {
                self.need_type(t)?;
                format!("{{k: \"t\", t: {}}}", js_str(t))
            }
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

    /// What `e` gives, and whether it reads what changes (see `reactive`).
    fn reactive_expr(&mut self, e: &Expr) -> Result<(String, bool), QuilloError> {
        let saved = std::mem::replace(&mut self.reactive, false);
        let v = self.expr(e);
        let r = std::mem::replace(&mut self.reactive, saved);
        Ok((v?, r))
    }

    /// A value for markup: as it is when fixed, else a function giving it,
    /// which the part of the page showing it follows.
    fn dyn_value(&mut self, e: &Expr) -> Result<String, QuilloError> {
        let (v, r) = self.reactive_expr(e)?;
        Ok(if r { format!("() => ({})", v) } else { v })
    }

    /// What is shown where `e` is in markup. `for` is a list of rows that
    /// each follow their item; `if` and `match` choose a branch, made again
    /// only when another is chosen; anything else is a value (`dyn_value`).
    fn markup(&mut self, e: &Expr) -> Result<String, QuilloError> {
        let saved = self.reactive;
        let r = self.markup_inner(e);
        self.reactive = saved;
        r
    }

    fn markup_inner(&mut self, e: &Expr) -> Result<String, QuilloError> {
        match e {
            Expr::Html(el, _) => self.html(el),
            Expr::For(pat, iter, body, line) => {
                self.line = *line;
                self.each(pat, iter, body)
            }
            Expr::If(c, a, b) if has_markup(e) => {
                let (cv, r) = self.reactive_expr(c)?;
                let av = self.markup(a)?;
                let bv = match b {
                    Some(b) => self.markup(b)?,
                    None => "null".into(),
                };
                Ok(if r {
                    format!("W.choose(() => (W.truthy({}) ? 0 : 1), [() => {}, () => {}])", cv, av, bv)
                } else {
                    format!("W.frag([W.truthy({}) ? {} : {}])", cv, av, bv)
                })
            }
            Expr::Match(subj, arms, line) if has_markup(e) => self.choose_match(subj, arms, *line),
            Expr::Block(ss) if has_markup(e) => {
                if let [Stmt::Expr(x)] = ss.as_slice() {
                    return self.markup(x);
                }
                self.push_scope();
                let r = self.setup_stmts(ss);
                self.pop_scope();
                Ok(format!("(() => {{\n{}}})()", r?))
            }
            _ => self.dyn_value(e),
        }
    }

    /// `for` in markup: `W.each(list, key, row)`. A row is made once for
    /// its key (the `key` of its element, else its place) and gets its item
    /// as a signal, so a row whose item changed changes only where it shows
    /// what changed.
    fn each(&mut self, pat: &Pattern, iter: &Expr, body: &Expr) -> Result<String, QuilloError> {
        let (list, _) = self.reactive_expr(iter)?;
        let p = if matches!(pat, Pattern::Bind(_)) { None } else { Some(self.pattern(pat)?) };
        let mut names = Vec::new();
        pattern_names(pat, &mut names);
        let (root, key) = match body {
            Expr::Html(el, _) => (
                Some(el),
                el.attrs.iter().find_map(|(n, v)| match (n.as_str(), v) {
                    ("key", AttrVal::Expr(k)) => Some(k),
                    _ => None,
                }),
            ),
            _ => (None, None),
        };
        let key_js = match key {
            Some(k) => {
                self.push_scope();
                let k = self.bind_pattern(pat, "$x", "`for` pattern does not match ").and_then(|binds| Ok((binds, self.reactive_expr(k)?.0)));
                self.pop_scope();
                let (binds, k) = k?;
                format!("($x) => {{ {}return {}; }}", binds, k)
            }
            None => "null".into(),
        };
        let x = self.fresh("row");
        self.push_scope();
        let mut binds = String::new();
        match &p {
            None => self.declare_as(&names[0], Local::Reactive(x.clone())),
            Some(pj) => {
                let b = self.fresh("b");
                let _ = write!(
                    binds,
                    "const {} = W.memo(() => {{ const $v = {}.v, $b = W.match({}, $v); if ($b === null) W.fail(\"`for` pattern does not match \" + W.short($v)); return $b; }}); ",
                    b, x, pj
                );
                for n in &names {
                    let m = self.fresh(n);
                    let _ = write!(binds, "const {} = W.memo(() => {}.v[{}]); ", m, b, js_str(n));
                    self.declare_as(n, Local::Reactive(m));
                }
            }
        }
        // The row's own template: what in it reads only the row, the row
        // updates itself.
        if root.is_some() {
            self.row = Some(x.clone());
        }
        let row = match root {
            Some(el) if key.is_some() => self.html_unkeyed(el),
            _ => self.markup(body),
        };
        self.row = None;
        self.pop_scope();
        Ok(format!("W.each(() => ({}), {}, ({}) => {{ {}return {}; }})", list, key_js, x, binds, row?))
    }

    /// `match` in markup: the arm is chosen by the subject (a memo), and
    /// an arm's names follow the subject while it stays chosen.
    fn choose_match(&mut self, subj: &Expr, arms: &[Arm], line: u32) -> Result<String, QuilloError> {
        self.line = line;
        let (s, _) = self.reactive_expr(subj)?;
        let v = self.fresh("v");
        let mut which = format!("const $v = {}.v;\nlet $b;\n", v);
        let mut branches = Vec::new();
        for (i, arm) in arms.iter().enumerate() {
            let pat = self.pattern(&arm.pat)?;
            let mut names = Vec::new();
            pattern_names(&arm.pat, &mut names);
            self.push_scope();
            let mut binds = String::new();
            for n in &names {
                let js = self.declare(n, None);
                let _ = write!(binds, "const {} = $b[{}]; ", js, js_str(n));
            }
            let guard = arm.guard.as_ref().map(|g| self.reactive_expr(g));
            self.pop_scope();
            match guard {
                Some(g) => {
                    let _ = writeln!(which, "if (($b = W.match({}, $v)) !== null) {{ {}if (W.truthy({})) return {}; }}", pat, binds, g?.0, i);
                }
                None => {
                    let _ = writeln!(which, "if (W.match({}, $v) !== null) return {};", pat, i);
                }
            }
            self.push_scope();
            let mut pre = String::new();
            if !names.is_empty() {
                let b = self.fresh("b");
                let _ = write!(pre, "const {} = W.memo(() => W.match({}, {}.v) || {{}}); ", b, pat, v);
                for n in &names {
                    let m = self.fresh(n);
                    let _ = write!(pre, "const {} = W.memo(() => {}.v[{}]); ", m, b, js_str(n));
                    self.declare_as(n, Local::Reactive(m));
                }
            }
            let body = self.markup(&arm.body);
            self.pop_scope();
            branches.push(format!("() => {{ {}return {}; }}", pre, body?));
        }
        which.push_str("W.fail(\"no `match` arm matches \" + W.short($v));\n");
        Ok(format!(
            "(() => {{ const {} = W.memo(() => ({})); return W.choose(() => {{\n{}}}, [{}]); }})()",
            v,
            s,
            which,
            branches.join(", ")
        ))
    }

    /// An element or component; with a `key` that changes, made afresh.
    fn html(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        self.line = el.line;
        let key = el.attrs.iter().find_map(|(n, v)| match (n.as_str(), v) {
            ("key", AttrVal::Expr(k)) => Some(k),
            _ => None,
        });
        let Some(key) = key else { return self.html_unkeyed(el) };
        let (k, r) = self.reactive_expr(key)?;
        let made = self.html_unkeyed(el)?;
        Ok(if r { format!("W.keyed(() => ({}), () => {})", k, made) } else { made })
    }

    fn html_unkeyed(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        let saved = self.reactive;
        let r = if el.tag.starts_with(|c: char| c.is_uppercase()) { self.comp_call(el) } else { self.template(el) };
        self.reactive = saved;
        r
    }

    /// Markup of a fixed shape: its template is defined once (see `tmpl`
    /// in the runtime), and each use gives the values of its holes.
    fn template(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        let row = self.row.take();
        let saved = (std::mem::replace(&mut self.cur_row, row.clone()), std::mem::replace(&mut self.row_mask, 0));
        let mut vals = Vec::new();
        let tpl = self.tpl_el(el, &mut vals);
        let mask = self.row_mask;
        (self.cur_row, self.row_mask) = saved;
        let tpl = tpl?;
        let name = format!("$b{}", self.blocks.len());
        self.blocks.push(format!("const {} = W.block({});", name, tpl));
        Ok(match row {
            Some(row) if mask != 0 => format!("W.t({}, [{}], {}, {})", name, vals.join(", "), row, mask),
            _ => format!("W.t({}, [{}])", name, vals.join(", ")),
        })
    }

    /// A hole's value in the template being compiled (see `dyn_value`),
    /// noting in `row_mask` whether it reads nothing but its row.
    fn hole_value(&mut self, e: &Expr, at: usize) -> Result<String, QuilloError> {
        let Some(row) = self.cur_row.clone() else { return self.dyn_value(e) };
        let saved = self.read_log.replace(Vec::new());
        let r = self.reactive_expr(e);
        let log = std::mem::replace(&mut self.read_log, saved).unwrap_or_default();
        if let Some(outer) = &mut self.read_log {
            outer.extend(log.iter().cloned());
        }
        let (v, reactive) = r?;
        if !reactive {
            return Ok(v);
        }
        if !log.is_empty() && log.iter().all(|s| *s == row) && at < 31 {
            self.row_mask |= 1 << at;
        }
        Ok(format!("() => ({})", v))
    }

    /// `<Name .../>`: a component, a view or a function, with its props as
    /// getters where they read what changes, and its children as markup.
    fn comp_call(&mut self, el: &HtmlEl) -> Result<String, QuilloError> {
        self.line = el.line;
        let (f, _) = self.callee_value(&Expr::Ident(el.tag.clone(), el.line))?;
        if f.starts_with("G[") && matches!(lookup(&self.it.globals, &el.tag), Some(Value::Func(c)) if !c.def.component && !c.def.server) {
            self.need_view(&el.tag)?;
        }
        let mut props = Vec::new();
        let mut spreads = Vec::new();
        for (name, v) in &el.attrs {
            if name == "key" {
                continue;
            }
            match v {
                AttrVal::Bool => props.push(format!("{}: true", js_str(name))),
                AttrVal::Expr(e) => {
                    let (v, r) = self.reactive_expr(e)?;
                    if r {
                        props.push(format!("get {}() {{ return {}; }}", js_str(name), v));
                    } else {
                        props.push(format!("{}: {}", js_str(name), v));
                    }
                }
                AttrVal::Spread(e) => spreads.push(self.reactive_expr(e)?.0),
            }
        }
        let kids = if el.children.is_empty() {
            "null".to_string()
        } else {
            let mut ks = Vec::new();
            for c in &el.children {
                ks.push(match c {
                    HtmlChild::El(e) => self.html(e)?,
                    HtmlChild::Expr(x) => self.markup(x)?,
                });
            }
            format!("W.frag([{}])", ks.join(", "))
        };
        let spreads = if spreads.is_empty() { String::new() } else { format!(", () => [{}]", spreads.join(", ")) };
        Ok(format!("W.c({}, {{{}}}, {}{})", f, props.join(", "), kids, spreads))
    }

    /// The form of view `name` for `<Name .../>`: its code once, with its
    /// props as getters (see `W.view` in the runtime), after the bundle's
    /// definitions.
    fn need_view(&mut self, name: &str) -> Result<(), QuilloError> {
        if !self.views.insert(name.to_string()) {
            return Ok(());
        }
        let Some(Value::Func(f)) = lookup(&self.it.globals, name) else { return Ok(()) };
        let def = f.def.clone();
        let (js, _) = self.global(name, def.line)?;
        let saved_ctxs = std::mem::take(&mut self.ctxs);
        let saved_what = std::mem::replace(&mut self.what, format!("<{}>", def.name));
        let saved_line = self.line;
        self.line = def.line;
        let res = self.setup(&def, true);
        self.ctxs = saved_ctxs;
        self.what = saved_what;
        self.line = saved_line;
        let code = res?;
        self.view_defs.push(format!("W.view({}, {});", js, code));
        Ok(())
    }

    /// `el`'s template, `[tag, [[name, value], ...], [kid, ...], [[kind,
    /// name, i], ...]]` (see `tmpl` in the runtime), adding to `vals` the
    /// values of its holes.
    fn tpl_el(&mut self, el: &HtmlEl, vals: &mut Vec<String>) -> Result<String, QuilloError> {
        self.line = el.line;
        let mut fixed_attrs: Vec<String> = Vec::new();
        let mut holes: Vec<String> = Vec::new();
        let hole = |holes: &mut Vec<String>, vals: &mut Vec<String>, kind: u8, name: &str, v: String| {
            holes.push(format!("[{}, {}, {}]", kind, js_str(name), vals.len()));
            vals.push(v);
        };
        let mut spread_js = Vec::new();
        let mut spread_r = false;
        for (_, v) in &el.attrs {
            if let AttrVal::Spread(e) = v {
                let (v, r) = self.reactive_expr(e)?;
                spread_js.push(v);
                spread_r |= r;
            }
        }
        if let Some(id) = &el.id {
            match fixed(id) {
                Some(l) => fixed_attrs.push(format!("[\"id\", {}]", lit_js(l))),
                None => {
                    let v = self.hole_value(id, vals.len())?;
                    hole(&mut holes, vals, 0, "id", v);
                }
            }
        }
        let mut classes: Vec<&Expr> = el.classes.iter().collect();
        for (name, v) in &el.attrs {
            if let ("class", AttrVal::Expr(e)) = (name.as_str(), v) {
                classes.push(e);
            }
        }
        if spread_js.is_empty() && classes.iter().all(|e| matches!(fixed(e), Some(Lit::Str(_)))) {
            let names: Vec<&str> = classes
                .iter()
                .filter_map(|e| match fixed(e) {
                    Some(Lit::Str(s)) if !s.is_empty() => Some(&**s),
                    _ => None,
                })
                .collect();
            if !names.is_empty() {
                fixed_attrs.push(format!("[\"class\", {}]", js_str(&names.join(" "))));
            }
        } else {
            let mut js = Vec::new();
            let mut r = spread_r;
            // One hole for all of them: only the row read, the row's.
            let row = if spread_js.is_empty() { self.cur_row.clone() } else { None };
            let saved = row.as_ref().map(|_| self.read_log.replace(Vec::new()));
            let got: Result<Vec<(String, bool)>, QuilloError> = classes.iter().map(|e| self.reactive_expr(e)).collect();
            let log = saved.map(|saved| std::mem::replace(&mut self.read_log, saved).unwrap_or_default());
            for (v, x) in got? {
                js.push(v);
                r |= x;
            }
            if let (Some(row), Some(log)) = (&row, log) {
                if r && !log.is_empty() && log.iter().all(|s| s == row) && vals.len() < 31 {
                    self.row_mask |= 1 << vals.len();
                }
                if let Some(outer) = &mut self.read_log {
                    outer.extend(log);
                }
            }
            let v = if !spread_js.is_empty() {
                js.push(format!("...W.spreadcls([{}])", spread_js.join(", ")));
                format!("W.cls([{}])", js.join(", "))
            } else if js.len() == 1 {
                format!("W.cls1({})", js[0])
            } else {
                format!("W.cls([{}])", js.join(", "))
            };
            hole(&mut holes, vals, 1, "", if r { format!("() => {}", v) } else { v });
        }
        let mut written = Vec::new();
        for (name, v) in &el.attrs {
            if matches!(v, AttrVal::Spread(_)) || name == "key" || name == "class" {
                continue;
            }
            written.push(js_str(name));
            if let Some(ev) = name.strip_prefix('@') {
                let f = match v {
                    AttrVal::Expr(e @ Expr::Lambda(_)) => self.expr(e)?,
                    AttrVal::Expr(e) => {
                        let (f, r) = self.reactive_expr(e)?;
                        if r {
                            // The handler it reads when the event comes.
                            format!("($e) => {{ const $f = {}; if (typeof $f === \"function\") return $f.length > 0 ? $f($e) : $f(); }}", f)
                        } else {
                            f
                        }
                    }
                    _ => "true".to_string(),
                };
                hole(&mut holes, vals, 2, event_name(ev), f);
                continue;
            }
            if let Some(prop) = name.strip_prefix("bind:") {
                let AttrVal::Expr(Expr::Ident(var, _)) = v else {
                    return self.err(format!("`bind:{}` needs a variable: bind:{}={{name}}", prop, prop));
                };
                let (event, read) = match prop {
                    "value" => ("input", "e.target.value"),
                    "checked" => ("change", "e.target.checked"),
                    other => return self.err(format!("`bind:{}` is not supported (use bind:value or bind:checked)", other)),
                };
                let saved = self.reactive;
                let (get, set) = match self.resolve(var)? {
                    Resolved::State(js) => (format!("() => {}.v", js), format!("{}.set({})", js, read)),
                    Resolved::Local(js, _) => (js.clone(), format!("({} = {})", js, read)),
                    _ => return self.err(format!("`bind:{}` needs component state, like `state {}`", prop, var)),
                };
                self.reactive = saved;
                hole(&mut holes, vals, 0, prop, get);
                hole(&mut holes, vals, 2, event, format!("(e) => {{ {}; }}", set));
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
                AttrVal::Expr(e) if !live => {
                    let v = self.hole_value(e, vals.len())?;
                    hole(&mut holes, vals, 0, name, v);
                }
                AttrVal::Expr(e) => {
                    let v = self.dyn_value(e)?;
                    hole(&mut holes, vals, 0, name, v);
                }
                _ => hole(&mut holes, vals, 0, name, "true".into()),
            }
        }
        if !spread_js.is_empty() {
            let v = format!("W.spread([{}], [{}])", spread_js.join(", "), written.join(", "));
            hole(&mut holes, vals, 3, "", if spread_r { format!("() => {}", v) } else { v });
        }
        // Children: fixed text and elements are in the template; anything
        // else is a value shown in its place.
        let mut kids = Vec::new();
        for c in &el.children {
            match c {
                HtmlChild::Expr(Expr::Lit(Lit::Str(s))) => kids.push(js_str(s)),
                HtmlChild::El(e) if inlineable(e) => kids.push(self.tpl_el(e, vals)?),
                HtmlChild::El(e) => {
                    let v = self.html(e)?;
                    kids.push(vals.len().to_string());
                    vals.push(v);
                }
                HtmlChild::Expr(e) => {
                    let v = if has_markup(e) { self.markup(e)? } else { self.hole_value(e, vals.len())? };
                    kids.push(vals.len().to_string());
                    vals.push(v);
                }
            }
        }
        self.line = el.line;
        Ok(format!("[{}, [{}], [{}], [{}]]", js_str(&el.tag), fixed_attrs.join(", "), kids.join(", "), holes.join(", ")))
    }
}

/// Whether `el` can be part of its parent's template: an element (not a
/// component) without a `key`, which makes it start afresh when it changes.
fn inlineable(el: &HtmlEl) -> bool {
    !el.tag.starts_with(|c: char| c.is_uppercase()) && el.attrs.iter().all(|(name, _)| name != "key")
}

/// A value fixed when the program is written: text, a number or a bool.
fn fixed(e: &Expr) -> Option<&Lit> {
    match e {
        Expr::Lit(l) => Some(l),
        _ => None,
    }
}
