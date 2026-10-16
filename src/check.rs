//! Static checking: `quillo check`, `quillo run` and the language server.
//!
//! Types are sets of possible runtime types (`int | null`, ...); code without
//! annotations is `any`. An error is reported only when no combination of
//! the possible types can work, so the checker has no false alarms on
//! dynamic code, and annotations make it stricter. It reports:
//!
//! - unknown names, names used before they are defined, assignment to
//!   undeclared variables;
//! - calls: not a function, wrong number of arguments, argument types;
//! - operators whose operand types can never work, division by literal zero;
//! - return types, `! E` error types and `?`;
//! - effects: a `with` function using builtins or functions it does not allow;
//! - views and components: unknown tags and attributes, missing props;
//! - code in components that cannot run in the browser;
//! - unused `let` variables (a warning).
//!
//! It also records every definition and reference for the language server.

use crate::ast::*;
use crate::parser::Parser;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---- types -------------------------------------------------------------

const NIL: u16 = 1;
const BOOL: u16 = 2;
const INT: u16 = 4;
const FLOAT: u16 = 8;
const STR: u16 = 16;
const LIST: u16 = 32;
const MAP: u16 = 64;
const FN: u16 = 128;
const HTML: u16 = 256;
const RESP: u16 = 512;
const ERR: u16 = 1024;
const NUM: u16 = INT | FLOAT;
const ANY: u16 = 2047;
const BASE: [(u16, &str); 11] = [
    (NIL, "null"),
    (BOOL, "bool"),
    (INT, "int"),
    (FLOAT, "float"),
    (STR, "str"),
    (LIST, "list"),
    (MAP, "map"),
    (FN, "fn"),
    (HTML, "html"),
    (RESP, "response"),
    (ERR, "error"),
];

/// A set of possible types; `err` is what an error value may contain.
#[derive(Clone, Copy, PartialEq, Debug)]
struct T {
    bits: u16,
    err: u16,
}

impl T {
    const fn of(bits: u16) -> T {
        T { bits, err: if bits & ERR != 0 { ANY } else { 0 } }
    }
    const ANY: T = T { bits: ANY, err: ANY };
    const NEVER: T = T { bits: 0, err: 0 };
    fn error(payload: u16) -> T {
        T { bits: ERR, err: payload }
    }
    fn union(self, o: T) -> T {
        T { bits: self.bits | o.bits, err: self.err | o.err }
    }
    fn without(self, bits: u16) -> T {
        let b = self.bits & !bits;
        T { bits: b, err: if b & ERR != 0 { self.err } else { 0 } }
    }
    fn is_any(self) -> bool {
        self.bits == ANY
    }
}

fn bits_text(bits: u16) -> String {
    if bits == ANY {
        return "any".into();
    }
    if bits == 0 {
        return "nothing".into();
    }
    let mut names: Vec<&str> = Vec::new();
    let mut b = bits;
    if b & NUM == NUM {
        names.push("num");
        b &= !NUM;
    }
    for (bit, n) in BASE {
        if b & bit != 0 && bit != NIL {
            names.push(n);
        }
    }
    if b & NIL != 0 {
        names.push("null");
    }
    names.join(" | ")
}

fn ty_bits(t: &Ty) -> u16 {
    let mut b = if t.optional { NIL } else { 0 };
    for n in &t.names {
        b |= name_bits(n);
    }
    b
}

fn name_bits(n: &str) -> u16 {
    match n {
        "any" => ANY,
        "num" => NUM,
        "nil" | "null" => NIL,
        other => BASE.iter().find(|(_, x)| *x == other).map_or(ANY, |(b, _)| *b),
    }
}

fn lit_t(l: &Lit) -> T {
    T::of(match l {
        Lit::Nil => NIL,
        Lit::Bool(_) => BOOL,
        Lit::Int(_) => INT,
        Lit::Float(_) => FLOAT,
        Lit::Str(_) => STR,
    })
}

/// Results of `a op b` for every pair of possible types, or `None` when
/// no pair works.
fn binop_t(op: BinOp, a: T, b: T) -> Option<T> {
    if a.bits == 0 || b.bits == 0 {
        return Some(T::NEVER);
    }
    match op {
        BinOp::Eq | BinOp::Ne => return Some(T::of(BOOL)),
        // `a && b` and `a || b` give one of their operands.
        BinOp::And | BinOp::Or => return Some(a.union(b)),
        _ => {}
    }
    let mut out = 0u16;
    let mut ok = false;
    for (x, _) in BASE {
        if a.bits & x == 0 {
            continue;
        }
        for (y, _) in BASE {
            if b.bits & y == 0 {
                continue;
            }
            let r = match op {
                BinOp::Add => match (x, y) {
                    (INT, INT) => Some(INT),
                    (STR, _) | (_, STR) => Some(STR),
                    (HTML, HTML) => Some(HTML),
                    (LIST, LIST) => Some(LIST),
                    (MAP, MAP) => Some(MAP),
                    (INT | FLOAT, INT | FLOAT) => Some(FLOAT),
                    _ => None,
                },
                BinOp::Div => match (x, y) {
                    (INT, INT) => Some(INT | FLOAT),
                    (INT | FLOAT, INT | FLOAT) => Some(FLOAT),
                    _ => None,
                },
                BinOp::Sub | BinOp::Mul | BinOp::Mod => match (x, y) {
                    (INT, INT) => Some(INT),
                    (INT | FLOAT, INT | FLOAT) => Some(FLOAT),
                    _ => None,
                },
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (x, y) {
                    (INT | FLOAT, INT | FLOAT) | (STR, STR) => Some(BOOL),
                    _ => None,
                },
                BinOp::Range => match (x, y) {
                    (INT, INT) => Some(LIST),
                    _ => None,
                },
                _ => Some(ANY),
            };
            if let Some(r) = r {
                ok = true;
                out |= r;
            }
        }
    }
    ok.then(|| T::of(out))
}

// ---- builtin signatures --------------------------------------------------

/// `(name, params, returns)`. A parameter is a type, `T=` when it may be left
/// out, and `T...` for any number. `error(str)` is an error value.
const BUILTINS: &[(&str, &str, &str)] = &[
    ("print", "any...", "null"),
    ("str", "any=", "str"),
    ("int", "any", "int | error(str)"),
    ("float", "any", "float | error(str)"),
    ("len", "str | list | map | html | null", "int"),
    ("type_of", "any", "str"),
    ("upper", "str", "str"),
    ("lower", "str", "str"),
    ("trim", "str", "str"),
    ("split", "str, str=", "list"),
    ("join", "list | null, str=", "str"),
    ("contains", "str | list | map | null, any", "bool"),
    ("starts_with", "str, str", "bool"),
    ("ends_with", "str, str", "bool"),
    ("replace", "str, str, str", "str"),
    ("slice", "list | str, int, int=", "list | str"),
    ("map", "list | null, fn", "list"),
    ("filter", "list | null, fn", "list"),
    ("find", "list | null, fn", "any"),
    ("any", "list | null, fn", "bool"),
    ("all", "list | null, fn", "bool"),
    ("reduce", "list | null, any, fn", "any"),
    ("sum", "list | null", "any"),
    ("min", "list | null", "any"),
    ("max", "list | null", "any"),
    ("sort", "list | null", "list"),
    ("sort_by", "list | null, fn", "list"),
    ("reverse", "list | str", "list | str"),
    ("first", "list | null", "any"),
    ("last", "list | null", "any"),
    ("push", "list | null, any=", "list"),
    ("range", "int, int=", "list"),
    ("enumerate", "list | null", "list"),
    ("keys", "map | null", "list"),
    ("values", "map | null", "list"),
    ("get", "map | null, any, any=", "any"),
    ("has", "map | null, any", "bool"),
    ("set", "map | null, any, any=", "map"),
    ("remove", "list | map | null, any", "list | map"),
    ("merge", "map | null, map | null", "map"),
    ("err", "any=", "error(any)"),
    ("is_err", "any", "bool"),
    ("unwrap", "any", "any"),
    ("raw", "any", "html"),
    ("escape", "any", "str"),
    ("env", "str, any=", "any"),
    ("now", "", "int"),
    ("sleep", "num", "null"),
    ("file", "str", "response"),
    ("redirect", "str, int=", "response"),
    ("respond", "int, any=, map | null=", "response"),
    ("read_file", "str", "str | error(str)"),
    ("write_file", "str, any", "null | error(str)"),
    ("random", "", "float"),
    ("random_int", "int, int", "int"),
    ("uuid", "", "str"),
    ("abs", "num", "num"),
    ("round", "num, int=", "num"),
    ("floor", "num", "num"),
    ("ceil", "num", "num"),
    ("sqrt", "num", "float"),
    ("pow", "num, num", "num"),
    ("unique", "list | null", "list"),
    ("flatten", "list | null", "list"),
    ("zip", "list | null, list | null", "list"),
    ("take", "list | str | null, int", "list | str"),
    ("drop", "list | str | null, int", "list | str"),
    ("count", "list | null, fn=", "int"),
    ("group_by", "list | null, fn", "map"),
    ("index_of", "list | str, any", "int | null"),
    ("repeat", "str, int", "str"),
    ("live", "fn, any...", "any"),
    ("parallel", "fn...", "list"),
    ("every", "num, fn", "null"),
    ("url", "", "map"),
    ("css", "str", "str"),
    ("sql", "str", "str"),
    ("navigate", "str", "null"),
    ("publish", "any...", "null"),
    ("watch", "any...", "null"),
    ("json.parse", "str", "any"),
    ("json.stringify", "any, any=", "str"),
    ("db.open", "str", "null | error(str)"),
    ("db.exec", "str, any...", "map | error(str)"),
    ("db.query", "str, any...", "list | error(str)"),
    ("db.one", "str, any...", "map | null | error(str)"),
    ("db.transaction", "fn", "any"),
    ("http.get", "str, map | null=", "map | error(str)"),
    ("http.delete", "str, map | null=", "map | error(str)"),
    ("http.post", "str, any=, map | null=", "map | error(str)"),
    ("http.put", "str, any=, map | null=", "map | error(str)"),
    ("http.patch", "str, any=, map | null=", "map | error(str)"),
    ("http.request", "str, str, any=, map | null=", "map | error(str)"),
];

const MODULES: &[&str] = &["json", "db", "http"];

/// Builtins the browser has (see client.rs).
const BROWSER_ONLY_SERVER: &[&str] =
    &["env", "sleep", "file", "redirect", "respond", "read_file", "write_file", "publish", "watch"];

fn parse_t(s: &str) -> T {
    let mut t = T::NEVER;
    for part in s.split('|').map(str::trim) {
        if let Some(inner) = part.strip_prefix("error(").and_then(|p| p.strip_suffix(')')) {
            t = t.union(T::error(name_bits(inner)));
        } else {
            t = t.union(T::of(name_bits(part)));
        }
    }
    t
}

#[derive(Clone, Debug)]
struct PSig {
    name: String,
    ty: u16,
    optional: bool,
    annotated: bool,
}

/// What is known about a function: a builtin or a `fn`/`view`/`component`.
#[derive(Clone, Debug)]
struct Sig {
    name: String,
    params: Vec<PSig>,
    variadic: bool,
    /// Declared return type, or a builtin's.
    ret: Option<T>,
    err: Option<u16>,
    effects: Option<u8>,
    server: bool,
    component: bool,
    builtin: bool,
    def: Option<Arc<FnDef>>,
}

impl Sig {
    fn builtin(name: &str, params: &str, ret: &str) -> Sig {
        let mut ps = Vec::new();
        let mut variadic = false;
        for (i, p) in params.split(',').map(str::trim).filter(|p| !p.is_empty()).enumerate() {
            let (p, optional) = match p.strip_suffix('=') {
                Some(x) => (x, true),
                None => (p, false),
            };
            let p = match p.strip_suffix("...") {
                Some(x) => {
                    variadic = true;
                    x
                }
                None => p,
            };
            ps.push(PSig { name: format!("arg{}", i + 1), ty: parse_t(p).bits, optional: optional || variadic, annotated: true });
        }
        let effects = Some(builtin_effect(name));
        Sig {
            name: name.into(),
            params: ps,
            variadic,
            ret: Some(parse_t(ret)),
            err: None,
            effects,
            server: false,
            component: false,
            builtin: true,
            def: None,
        }
    }

    fn of_def(def: &Arc<FnDef>) -> Sig {
        Sig {
            name: def.name.clone(),
            params: def
                .params
                .iter()
                .map(|p| PSig {
                    name: p.name.clone(),
                    ty: p.ty.as_ref().map_or(ANY, ty_bits),
                    optional: p.default.is_some(),
                    annotated: p.ty.is_some(),
                })
                .collect(),
            variadic: false,
            ret: def.ret.as_ref().map(|t| T::of(ty_bits(t))),
            err: def.err.as_ref().map(ty_bits),
            effects: def.effects.as_ref().map(|e| effect_mask(e)),
            server: def.server,
            component: def.component,
            builtin: false,
            def: Some(def.clone()),
        }
    }

    /// `fn add(a: int, b: int = 10) -> int ! str with db`
    fn text(&self) -> String {
        if let Some(d) = &self.def {
            return def_text(d);
        }
        let ps: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let t = bits_text(p.ty);
                if self.variadic && p.optional {
                    format!("...{}", t)
                } else if p.optional {
                    format!("{}?", t)
                } else {
                    t
                }
            })
            .collect();
        let ret = self.ret.map(|t| {
            let mut s = bits_text(t.bits & !ERR);
            if t.bits & ERR != 0 {
                if t.bits & !ERR != 0 {
                    s = format!("{} | ", s);
                } else {
                    s.clear();
                }
                s.push_str(&format!("error({})", bits_text(t.err)));
            }
            s
        });
        let mut s = format!("{}({})", self.name, ps.join(", "));
        if let Some(r) = ret {
            s.push_str(&format!(" -> {}", r));
        }
        let e = self.effects.unwrap_or(0);
        if e != 0 {
            s.push_str(&format!("  (effect: {})", effect_names(e).join(", ")));
        }
        s
    }
}

/// Source-like text for a definition: `fn add(a: int, b: int = 10) -> int`.
pub fn def_text(d: &FnDef) -> String {
    let kw = if d.component {
        "component"
    } else if d.server {
        "server fn"
    } else {
        "fn"
    };
    let ps: Vec<String> = d
        .params
        .iter()
        .map(|p| {
            let mut s = p.name.clone();
            if let Some(t) = &p.ty {
                s.push_str(&format!(": {}", t.text()));
            }
            if p.default.is_some() {
                s.push_str(" = …");
            }
            s
        })
        .collect();
    let mut s = format!("{} {}({})", kw, d.name, ps.join(", "));
    if let Some(r) = &d.ret {
        s.push_str(&format!(" -> {}", r.text()));
    }
    if let Some(e) = &d.err {
        s.push_str(&format!(" ! {}", e.text()));
    }
    if let Some(e) = &d.effects {
        s.push_str(&format!(" with {}", e.join(", ")));
    }
    s
}

// ---- results -----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub severity: Severity,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let sev = if self.severity == Severity::Error { "error" } else { "warning" };
        if self.line > 0 {
            write!(f, "{}:{}: {}: {}", self.file, self.line, sev, self.message)
        } else {
            write!(f, "{}: {}: {}", self.file, sev, self.message)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    ServerFn,
    View,
    Component,
    Variable,
    Builtin,
    Route,
}

/// A top-level definition.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
    pub detail: String,
}

/// A name used on a line, and what it refers to.
#[derive(Clone, Debug)]
pub struct Reference {
    pub file: String,
    pub line: u32,
    pub name: String,
    /// Where it is defined (`None` for builtins).
    pub def: Option<(String, u32)>,
    pub detail: String,
}

#[derive(Default, Debug)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
}

impl Analysis {
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error)
    }
}

/// Every builtin with its signature text, for completion.
pub fn builtin_signatures() -> Vec<(String, String)> {
    BUILTINS.iter().map(|(n, p, r)| (n.to_string(), Sig::builtin(n, p, r).text())).collect()
}

/// Checks a program file and the files it `use`s.
pub fn check_file(path: &Path) -> Analysis {
    match std::fs::read_to_string(path) {
        Ok(src) => {
            let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            check_source(&src, &path.display().to_string(), base)
        }
        Err(e) => {
            let mut a = Analysis::default();
            a.diagnostics.push(Diagnostic {
                file: path.display().to_string(),
                line: 0,
                severity: Severity::Error,
                message: format!("cannot read file: {}", e),
            });
            a
        }
    }
}

/// Checks source text (the main file need not be saved); `use`d files are
/// read from `base_dir`.
pub fn check_source(src: &str, file: &str, base_dir: &Path) -> Analysis {
    let mut c = Checker::new(file);
    let main: Arc<str> = Arc::from(file);
    match Parser::new(src).parse_program() {
        Ok(stmts) => {
            let mut seen = HashSet::new();
            if let Ok(p) = Path::new(file).canonicalize() {
                seen.insert(p);
            }
            let mut order = Vec::new();
            c.expand(Arc::new(stmts), main, base_dir, &mut seen, &mut order);
            c.run(order);
        }
        Err(e) => c.diag_at(main, e.line, Severity::Error, e.msg),
    }
    c.finish()
}

// ---- the checker ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Global,
    Param,
    Let,
    State,
    Fn,
    Bound,
}

#[derive(Clone)]
struct Var {
    t: T,
    sig: Option<Arc<Sig>>,
    file: Arc<str>,
    line: u32,
    kind: Kind,
    used: bool,
    /// For globals: defined by the time top-level code reaches this point.
    ready: bool,
}

struct FnCtx {
    name: String,
    ret: Option<T>,
    err: Option<u16>,
    /// Effects allowed here, and the `with` function that limited them.
    allowed: u8,
    restrictor: Option<(String, Vec<String>)>,
    /// Inside a component: the code also runs in the browser.
    component: Option<String>,
    /// Inside a lambda or local fn of a component (event handlers).
    nested: bool,
    /// What the function gives back other than at the end of its body:
    /// values of `return`, and the errors `?` passes on.
    exits: T,
}

/// One statement of the program, in the order the top level runs them.
struct Item {
    stmt: Stmt,
    file: Arc<str>,
}

struct Checker {
    globals: HashMap<String, Var>,
    scopes: Vec<HashMap<String, Var>>,
    fns: Vec<FnCtx>,
    /// > 0 while inferring a type: no diagnostics, no references.
    quiet: u32,
    /// Checking top-level code as it runs (not a function or route body).
    sequential: bool,
    assigned: HashSet<String>,
    ret_cache: HashMap<usize, T>,
    in_progress: HashSet<usize>,
    effect_cache: HashMap<usize, u8>,
    /// Checking the `live(...)` that sets a component's `~state`.
    live_ok: bool,
    file: Arc<str>,
    line: u32,
    out: Analysis,
    reported: HashSet<(Arc<str>, u32, String)>,
}

fn def_key(d: &Arc<FnDef>) -> usize {
    Arc::as_ptr(d) as usize
}

impl Checker {
    fn new(file: &str) -> Self {
        let mut globals = HashMap::new();
        let builtin_file: Arc<str> = Arc::from("<builtin>");
        for (name, params, ret) in BUILTINS {
            if name.contains('.') {
                continue;
            }
            let sig = Arc::new(Sig::builtin(name, params, ret));
            globals.insert(
                name.to_string(),
                Var { t: T::of(FN), sig: Some(sig), file: builtin_file.clone(), line: 0, kind: Kind::Global, used: true, ready: true },
            );
        }
        for m in MODULES {
            globals.insert(
                m.to_string(),
                Var { t: T::of(MAP), sig: None, file: builtin_file.clone(), line: 0, kind: Kind::Global, used: true, ready: true },
            );
        }
        Checker {
            globals,
            scopes: Vec::new(),
            fns: Vec::new(),
            quiet: 0,
            sequential: true,
            assigned: HashSet::new(),
            ret_cache: HashMap::new(),
            in_progress: HashSet::new(),
            effect_cache: HashMap::new(),
            live_ok: false,
            file: Arc::from(file),
            line: 0,
            out: Analysis::default(),
            reported: HashSet::new(),
        }
    }

    fn finish(mut self) -> Analysis {
        self.out.diagnostics.sort_by(|a, b| (a.file != *self.file, &a.file, a.line).cmp(&(b.file != *self.file, &b.file, b.line)));
        self.out
    }

    fn diag_at(&mut self, file: Arc<str>, line: u32, severity: Severity, msg: impl Into<String>) {
        if self.quiet > 0 {
            return;
        }
        let msg = msg.into();
        if !self.reported.insert((file.clone(), line, msg.clone())) {
            return;
        }
        self.out.diagnostics.push(Diagnostic { file: file.to_string(), line, severity, message: msg });
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.diag_at(self.file.clone(), self.line, Severity::Error, msg);
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.diag_at(self.file.clone(), self.line, Severity::Warning, msg);
    }

    /// Lists statements in the order the top level runs them, following `use`.
    fn expand(&mut self, stmts: Arc<Vec<Stmt>>, file: Arc<str>, base: &Path, seen: &mut HashSet<PathBuf>, order: &mut Vec<Item>) {
        for s in stmts.iter() {
            if let Stmt::Mount(prefix, path, line) = s {
                // A mounted program is a program of its own: check it alone.
                let full = base.join(path);
                if !full.is_file() {
                    self.diag_at(file.clone(), *line, Severity::Error, format!("cannot read {}: no such file", full.display()));
                } else {
                    let sub = check_file(&full);
                    self.out.diagnostics.extend(sub.diagnostics);
                    self.out.symbols.extend(sub.symbols.into_iter().filter(|s| s.kind == SymbolKind::Route).map(|mut s| {
                        s.name = s.name.replacen(' ', &format!(" {}", prefix), 1);
                        s
                    }));
                }
                order.push(Item { stmt: s.clone(), file: file.clone() });
                continue;
            }
            if let Stmt::Use(path, line) = s {
                let full = base.join(path);
                let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                if !seen.insert(key) {
                    continue;
                }
                let fname: Arc<str> = Arc::from(full.display().to_string());
                match std::fs::read_to_string(&full) {
                    Ok(src) => match Parser::new(&src).parse_program() {
                        Ok(sub) => self.expand(Arc::new(sub), fname, base, seen, order),
                        Err(e) => self.diag_at(fname, e.line, Severity::Error, e.msg),
                    },
                    Err(e) => self.diag_at(file.clone(), *line, Severity::Error, format!("cannot read {}: {}", full.display(), e)),
                }
                continue;
            }
            order.push(Item { stmt: s.clone(), file: file.clone() });
        }
    }

    fn run(&mut self, order: Vec<Item>) {
        for it in &order {
            collect_assigned(std::slice::from_ref(&it.stmt), &mut self.assigned);
        }
        // Everything top-level is known up front; it becomes usable from
        // top-level code once the program has run its definition.
        for it in &order {
            match &it.stmt {
                Stmt::Fn(d) => {
                    let sig = Arc::new(Sig::of_def(d));
                    self.globals.insert(
                        d.name.clone(),
                        Var { t: T::of(FN), sig: Some(sig), file: it.file.clone(), line: d.line, kind: Kind::Global, used: false, ready: false },
                    );
                }
                Stmt::Let(p, _, line) => {
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    for n in names {
                        self.globals.entry(n).or_insert(Var {
                            t: T::ANY,
                            sig: None,
                            file: it.file.clone(),
                            line: *line,
                            kind: Kind::Global,
                            used: false,
                            ready: false,
                        });
                    }
                }
                _ => {}
            }
        }
        for it in &order {
            self.file = it.file.clone();
            match &it.stmt {
                Stmt::Fn(d) => {
                    if let Some(g) = self.globals.get_mut(&d.name) {
                        g.ready = true;
                    }
                    let kind = if d.component {
                        SymbolKind::Component
                    } else if d.server {
                        SymbolKind::ServerFn
                    } else if is_view(d) {
                        SymbolKind::View
                    } else {
                        SymbolKind::Function
                    };
                    self.out.symbols.push(Symbol { name: d.name.clone(), kind, file: it.file.to_string(), line: d.line, detail: def_text(d) });
                }
                Stmt::Route(r) => {
                    self.out.symbols.push(Symbol {
                        name: format!("{} {}", r.method, route_path(r)),
                        kind: SymbolKind::Route,
                        file: it.file.to_string(),
                        line: r.line,
                        detail: format!("route {} {}", r.method, route_path(r)),
                    });
                }
                Stmt::Let(p, e, line) => {
                    self.line = *line;
                    self.scopes.clear();
                    let t = self.expr(e);
                    let mut binds = Vec::new();
                    self.bind_pattern(p, t, &mut binds);
                    for (n, t, sig) in binds {
                        let reassigned = self.assigned.contains(&n);
                        if let Some(g) = self.globals.get_mut(&n) {
                            g.ready = true;
                            if g.file == it.file && g.line == *line {
                                g.t = if reassigned { T::ANY } else { t };
                                g.sig = if reassigned { None } else { sig };
                            }
                        }
                        let detail = format!("let {}: {}", n, bits_text(if reassigned { ANY } else { t.bits }));
                        self.out.symbols.push(Symbol { name: n, kind: SymbolKind::Variable, file: it.file.to_string(), line: *line, detail });
                    }
                }
                other => {
                    self.scopes.clear();
                    self.stmt(other, false);
                }
            }
        }
        // Function, component and route bodies run later, when everything
        // top-level exists.
        self.sequential = false;
        for it in &order {
            self.file = it.file.clone();
            self.scopes.clear();
            match &it.stmt {
                Stmt::Fn(d) => self.fn_body(d),
                Stmt::Route(r) => self.route(r),
                _ => {}
            }
        }
    }

    // ---- scopes ------------------------------------------------------

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for (name, v) in scope {
                if v.kind == Kind::Let && !v.used && !name.starts_with('_') && self.quiet == 0 {
                    self.diag_at(v.file.clone(), v.line, Severity::Warning, format!("unused variable `{}` (name it `_{}` if that is intended)", name, name));
                }
            }
        }
    }

    fn define(&mut self, name: &str, t: T, sig: Option<Arc<Sig>>, kind: Kind) {
        let v = Var { t, sig, file: self.file.clone(), line: self.line, kind, used: false, ready: true };
        match self.scopes.last_mut() {
            Some(s) => {
                // `let x` twice in one scope reuses the variable.
                let used = s.get(name).map_or(false, |old| old.used);
                s.insert(name.to_string(), Var { used, ..v });
            }
            None => {
                self.globals.insert(name.to_string(), Var { kind: Kind::Global, ..v });
            }
        }
    }

    fn record(&mut self, name: &str, v: &Var) {
        if self.quiet > 0 {
            return;
        }
        let def = if &*v.file == "<builtin>" { None } else { Some((v.file.to_string(), v.line)) };
        let detail = match &v.sig {
            Some(s) => s.text(),
            None => {
                let kw = match v.kind {
                    Kind::State => "~",
                    Kind::Param => "param ",
                    _ => "let ",
                };
                if MODULES.contains(&name) && v.kind == Kind::Global && def.is_none() {
                    format!("module {}", name)
                } else {
                    format!("{}{}: {}", kw, name, bits_text(v.t.bits))
                }
            }
        };
        self.out.references.push(Reference { file: self.file.to_string(), line: self.line, name: name.to_string(), def, detail });
    }

    /// Looks a name up, marks it used and records the reference.
    fn lookup(&mut self, name: &str) -> Option<Var> {
        for i in (0..self.scopes.len()).rev() {
            if let Some(v) = self.scopes[i].get_mut(name) {
                v.used = true;
                let v = v.clone();
                self.record(name, &v);
                return Some(v);
            }
        }
        let v = self.globals.get(name).cloned()?;
        self.record(name, &v);
        Some(v)
    }

    fn ident(&mut self, name: &str) -> (T, Option<Arc<Sig>>) {
        match self.lookup(name) {
            Some(v) => {
                if v.kind == Kind::Global && !v.ready && self.sequential && self.fns.is_empty() {
                    self.error(format!("`{}` is used before it is defined (line {})", name, v.line));
                }
                if v.kind == Kind::Global && &*v.file != "<builtin>" {
                    self.browser_global(name, &v);
                }
                if v.kind == Kind::Global && (name == "db" || name == "http") && &*v.file == "<builtin>" {
                    self.browser_forbidden(&format!("`{}`", name));
                }
                (v.t, v.sig)
            }
            None => {
                if name == "request" {
                    self.error("`request` only exists in routes and `server fn`s; pass what you need as an argument");
                } else {
                    self.error(format!("unknown name `{}`{}", name, self.suggest(name)));
                }
                (T::ANY, None)
            }
        }
    }

    fn suggest(&self, name: &str) -> String {
        let mut best: Option<(usize, String)> = None;
        let mut consider = |cand: &str| {
            let d = edit_distance(name, cand);
            if d <= 2 && d < name.len() && best.as_ref().map_or(true, |(bd, _)| d < *bd) {
                best = Some((d, cand.to_string()));
            }
        };
        for s in &self.scopes {
            s.keys().for_each(|k| consider(k));
        }
        self.globals.keys().for_each(|k| consider(k));
        match best {
            Some((_, n)) => format!(" (did you mean `{}`?)", n),
            None => String::new(),
        }
    }

    // ---- browser rules (inside components) ---------------------------

    fn in_component(&self) -> Option<String> {
        self.fns.iter().rev().find_map(|f| f.component.clone())
    }

    fn browser_forbidden(&mut self, what: &str) {
        if let Some(c) = self.in_component() {
            self.error(format!("{} is not available in the browser, where <{}> also runs; use a `server fn`", what, c));
        }
    }

    fn browser_global(&mut self, name: &str, v: &Var) {
        if self.in_component().is_none() || v.sig.is_some() {
            return;
        }
        if self.assigned.contains(name) {
            let c = self.in_component().unwrap();
            self.error(format!(
                "<{}> runs in the browser, but `{}` is a server variable that changes; pass it as a prop or read it with a `server fn`",
                c, name
            ));
        }
    }

    // ---- statements and functions -------------------------------------

    fn fn_body(&mut self, d: &Arc<FnDef>) {
        self.line = d.line;
        let effects = d.effects.as_ref().map(|e| effect_mask(e));
        if d.server {
            for p in &d.params {
                if let Some(t) = &p.ty {
                    if ty_bits(t) & (FN | HTML | RESP) != 0 && ty_bits(t) != ANY {
                        self.error(format!("parameter `{}` of server fn `{}` is {}, which the browser cannot send", p.name, d.name, t.text()));
                    }
                }
            }
        }
        self.fns.push(FnCtx {
            name: d.name.clone(),
            ret: d.ret.as_ref().map(|t| T::of(ty_bits(t))),
            err: d.err.as_ref().map(ty_bits),
            allowed: effects.unwrap_or(ALL_EFFECTS),
            restrictor: d.effects.as_ref().map(|e| (d.name.clone(), e.clone())),
            component: d.component.then(|| d.name.clone()),
            nested: false,
            exits: T::NEVER,
        });
        self.push();
        if d.server {
            self.define("request", T::of(MAP), None, Kind::Bound);
        }
        self.params(d);
        let t = self.expr(&d.body);
        self.check_return_expr(&d.body, t);
        self.pop();
        self.fns.pop();
    }

    fn params(&mut self, d: &FnDef) {
        for p in &d.params {
            let mut t = p.ty.as_ref().map_or(T::ANY, |t| T::of(ty_bits(t)));
            if let Some(def) = &p.default {
                let dt = self.expr(def);
                if p.ty.is_none() {
                    t = T::ANY;
                } else if let Some(ty) = &p.ty {
                    if dt.bits != 0 && dt.bits & ty_bits(ty) == 0 {
                        self.error(format!("the default of `{}` is {}, but it must be {}", p.name, bits_text(dt.bits), ty.text()));
                    }
                }
            }
            self.define(&p.name, t, None, Kind::Param);
        }
    }

    /// Checks what a function body returns against its `-> T` and `! E`,
    /// looking into each branch.
    fn check_return_expr(&mut self, e: &Expr, t: T) {
        let Some(f) = self.fns.last() else { return };
        let (ret, err, name) = (f.ret, f.err, f.name.clone());
        if ret.is_none() && err.is_none() {
            return;
        }
        match e {
            Expr::If(_, a, b) => {
                let ta = self.quiet_expr_type(a);
                self.check_return_expr(a, ta);
                match b {
                    Some(b) => {
                        let tb = self.quiet_expr_type(b);
                        self.check_return_expr(b, tb);
                    }
                    None => self.check_return_value(T::of(NIL), ret, err, &name),
                }
            }
            Expr::Block(_) | Expr::Match(..) => self.check_return_value(t, ret, err, &name),
            _ => self.check_return_value(t, ret, err, &name),
        }
    }

    fn quiet_expr_type(&mut self, e: &Expr) -> T {
        // Branch types were already checked (with diagnostics) as part of
        // the whole expression; this only recomputes them.
        self.quiet += 1;
        let t = self.expr(e);
        self.quiet -= 1;
        t
    }

    fn check_return_value(&mut self, t: T, ret: Option<T>, err: Option<u16>, name: &str) {
        let vals = t.bits & !ERR;
        if let Some(r) = ret {
            if vals != 0 && t.bits & ERR == 0 && vals & r.bits == 0 {
                self.error(format!("`{}` must return {}, but this returns {}", name, bits_text(r.bits), bits_text(vals)));
            }
        }
        if let Some(e) = err {
            if t.bits & ERR != 0 && t.err != 0 && t.err & e == 0 {
                self.error(format!("`{}` declares errors as `! {}`, but this returns err({})", name, bits_text(e), bits_text(t.err)));
            }
        }
    }

    fn route(&mut self, r: &Arc<RouteDef>) {
        self.line = r.line;
        self.fns.push(FnCtx {
            name: format!("{} {}", r.method, route_path(r)),
            ret: None,
            err: None,
            allowed: ALL_EFFECTS,
            restrictor: None,
            component: None,
            nested: false,
            exits: T::NEVER,
        });
        self.push();
        self.define("request", T::of(MAP), None, Kind::Bound);
        let mut seen = HashSet::new();
        for seg in &r.path {
            match seg {
                Seg::Param(n, ty) => {
                    if !seen.insert(n.clone()) {
                        self.error(format!("path parameter `{}` appears twice", n));
                    }
                    let t = ty.as_deref().map_or(STR, name_bits);
                    self.define(n, T::of(t), None, Kind::Bound);
                }
                Seg::Rest(n) => self.define(n, T::of(STR), None, Kind::Bound),
                Seg::Lit(_) => {}
            }
        }
        let mut binds = Vec::new();
        if let Some(q) = &r.query {
            self.bind_pattern(q, T::of(MAP), &mut binds);
        }
        if let Some(b) = &r.body_pat {
            self.bind_pattern(b, T::of(MAP), &mut binds);
        }
        for (n, t, sig) in binds {
            self.define(&n, t, sig, Kind::Bound);
        }
        self.expr(&r.body);
        self.pop();
        self.fns.pop();
    }

    fn stmt(&mut self, s: &Stmt, last: bool) -> T {
        match s {
            Stmt::Expr(e) => {
                let t = self.expr(e);
                // `let total = a` and then `-b` on the next line are two
                // statements; the second does nothing.
                if let (false, Expr::Unary(UnOp::Neg, _, line)) = (last, e) {
                    self.line = *line;
                    self.warn(
                        "a line starting with `-` is a statement of its own, and this one does nothing: \
                         to continue the line above, write `- x` with a space or end that line with `-`",
                    );
                }
                if last {
                    t
                } else {
                    T::of(NIL)
                }
            }
            Stmt::Let(p, e, line) => {
                self.line = *line;
                let t = self.expr(e);
                let mut binds = Vec::new();
                self.bind_pattern(p, t, &mut binds);
                for (n, t, sig) in binds {
                    let kind = if matches!(p, Pattern::Bind(_)) { Kind::Let } else { Kind::Bound };
                    let t = if self.assigned.contains(&n) { T::ANY } else { t };
                    self.define(&n, t, sig, kind);
                }
                T::of(NIL)
            }
            Stmt::Assign(name, op, e, line) => {
                self.line = *line;
                let rhs = self.expr(e);
                let found = self.scopes.iter().rev().find_map(|s| s.get(name).cloned()).or_else(|| self.globals.get(name).cloned());
                match found {
                    None => self.error(format!("cannot assign to `{}`: it was never declared (use `let {} = …`)", name, name)),
                    Some(v) => {
                        if v.kind == Kind::Global && &*v.file == "<builtin>" {
                            self.error(format!("cannot assign to `{}`: it is a builtin", name));
                        }
                        if v.kind == Kind::Global && self.in_component().is_some() && v.sig.is_none() {
                            self.browser_global(name, &v);
                        }
                        if !matches!(op, AssignOp::Set) {
                            let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                            if binop_t(bop, v.t, rhs).is_none() {
                                self.error(format!(
                                    "cannot {} {} and {}",
                                    if matches!(op, AssignOp::Add) { "add" } else { "subtract" },
                                    bits_text(v.t.bits),
                                    bits_text(rhs.bits)
                                ));
                            }
                        }
                        if v.kind != Kind::Global {
                            for i in (0..self.scopes.len()).rev() {
                                if let Some(x) = self.scopes[i].get_mut(name) {
                                    x.used = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                T::of(NIL)
            }
            Stmt::Fn(d) => {
                self.line = d.line;
                if !self.scopes.is_empty() {
                    if d.server || d.component {
                        self.error("`server fn` and `component` must be declared at the top level");
                    }
                    let sig = Arc::new(Sig::of_def(d));
                    self.define(&d.name, T::of(FN), Some(sig), Kind::Fn);
                    self.lambda_body(d);
                }
                T::of(NIL)
            }
            Stmt::Route(r) => {
                self.line = r.line;
                if !self.scopes.is_empty() {
                    self.error("routes must be declared at the top level");
                }
                T::of(NIL)
            }
            Stmt::Use(_, line) => {
                self.line = *line;
                if !self.scopes.is_empty() {
                    self.error("`use` must be at the top level");
                }
                T::of(NIL)
            }
            Stmt::Mount(_, _, line) => {
                self.line = *line;
                if !self.scopes.is_empty() {
                    self.error("`mount` must be at the top level");
                }
                T::of(NIL)
            }
            Stmt::Return(e) => {
                let t = match e {
                    Some(e) => self.expr(e),
                    None => T::of(NIL),
                };
                if let Some(f) = self.fns.last_mut() {
                    f.exits = f.exits.union(t);
                }
                if let Some(f) = self.fns.last() {
                    let (ret, err, name) = (f.ret, f.err, f.name.clone());
                    if let Some(e) = e {
                        self.check_return_expr(e, t);
                    } else {
                        self.check_return_value(t, ret, err, &name);
                    }
                }
                T::NEVER
            }
            Stmt::State(name, e, line) => {
                self.line = *line;
                // The initializer may wait on the server (it runs once).
                let saved = self.fns.last().map(|f| f.nested);
                if let Some(f) = self.fns.last_mut() {
                    f.nested = true;
                }
                let is_live = matches!(e, Expr::Call(c, _, _) if matches!(&**c, Expr::Ident(n, _) if n == "live"));
                self.live_ok = is_live;
                let t = self.expr(e);
                self.live_ok = false;
                if let (Some(f), Some(n)) = (self.fns.last_mut(), saved) {
                    f.nested = n;
                }
                self.define(name, if self.assigned.contains(name) { T::ANY } else { t }, None, Kind::State);
                T::of(NIL)
            }
        }
    }

    /// The body of a lambda or local fn, checked where it is written.
    fn lambda_body(&mut self, d: &Arc<FnDef>) -> T {
        let (outer_allowed, outer_restrictor, component) = match self.fns.last() {
            Some(f) => (f.allowed, f.restrictor.clone(), f.component.clone()),
            None => (ALL_EFFECTS, None, None),
        };
        let allowed = d.effects.as_ref().map_or(outer_allowed, |e| effect_mask(e));
        let restrictor = match &d.effects {
            Some(e) => Some((d.name.clone(), e.clone())),
            None => outer_restrictor.clone(),
        };
        if let Some(e) = &d.effects {
            let mask = effect_mask(e);
            if mask & !outer_allowed != 0 {
                self.effect_error(&d.name, mask & !outer_allowed, outer_restrictor.as_ref());
            }
        }
        self.fns.push(FnCtx {
            name: d.name.clone(),
            ret: d.ret.as_ref().map(|t| T::of(ty_bits(t))),
            err: d.err.as_ref().map(ty_bits),
            allowed,
            restrictor,
            component,
            nested: true,
            exits: T::NEVER,
        });
        let saved_seq = self.sequential;
        self.sequential = false;
        self.push();
        self.params(d);
        let t = self.expr(&d.body);
        self.check_return_expr(&d.body, t);
        self.pop();
        self.sequential = saved_seq;
        let t = t.union(self.fns.pop().map_or(T::NEVER, |f| f.exits));
        let t = match &d.ret {
            Some(r) => T::of(ty_bits(r)).union(t.without(!ERR)),
            None => t,
        };
        self.ret_cache.insert(def_key(d), t);
        t
    }

    /// What calling a top-level function returns: its annotation, or what
    /// its body produces (worked out once, quietly).
    fn fn_ret(&mut self, sig: &Sig) -> T {
        let Some(d) = &sig.def else { return sig.ret.unwrap_or(T::ANY) };
        if d.component {
            return T::of(HTML);
        }
        let key = def_key(d);
        if let Some(t) = self.ret_cache.get(&key) {
            return *t;
        }
        if self.in_progress.contains(&key) {
            return sig.ret.unwrap_or(T::ANY).union(T::error(ANY));
        }
        self.in_progress.insert(key);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_fns = std::mem::take(&mut self.fns);
        let saved_line = self.line;
        let saved_seq = self.sequential;
        self.sequential = false;
        self.quiet += 1;
        self.fns.push(FnCtx {
            name: d.name.clone(),
            ret: None,
            err: None,
            allowed: ALL_EFFECTS,
            restrictor: None,
            component: None,
            nested: false,
            exits: T::NEVER,
        });
        self.push();
        if d.server {
            self.define("request", T::of(MAP), None, Kind::Bound);
        }
        self.params(d);
        let body = self.expr(&d.body);
        let body = body.union(self.fns.last().map_or(T::NEVER, |f| f.exits));
        self.scopes.clear();
        self.quiet -= 1;
        self.fns = saved_fns;
        self.scopes = saved_scopes;
        self.line = saved_line;
        self.sequential = saved_seq;
        self.in_progress.remove(&key);
        let mut t = match sig.ret {
            Some(r) => r.union(body.without(!ERR)),
            None => body,
        };
        if let Some(e) = sig.err {
            if t.bits & ERR != 0 {
                t.err = e;
            }
        }
        self.ret_cache.insert(key, t);
        t
    }

    /// Effects a top-level function may use, declared or worked out from
    /// what it calls.
    fn fn_effects(&mut self, sig: &Sig) -> u8 {
        if let Some(e) = sig.effects {
            return e;
        }
        let Some(d) = &sig.def else { return 0 };
        let key = def_key(d);
        if let Some(e) = self.effect_cache.get(&key) {
            return *e;
        }
        self.effect_cache.insert(key, 0);
        let mut names = Vec::new();
        called_names(&d.body, &mut names);
        let mut mask = 0;
        for n in names {
            if let Some(g) = self.globals.get(&n).cloned() {
                if let Some(s) = &g.sig {
                    mask |= if s.builtin { s.effects.unwrap_or(0) } else { self.fn_effects(s) };
                }
            } else if let Some((m, f)) = n.split_once('.') {
                if MODULES.contains(&m) {
                    mask |= builtin_effect(&format!("{}.{}", m, f));
                }
            }
        }
        self.effect_cache.insert(key, mask);
        mask
    }

    fn effect_error(&mut self, what: &str, missing: u8, restrictor: Option<&(String, Vec<String>)>) {
        let names = effect_names(missing);
        let need = format!("the {} effect{}", names.join(", "), if names.len() == 1 { "" } else { "s" });
        match restrictor {
            Some((f, decl)) => self.error(format!("`{}` needs {}, but `{}` is declared `with {}`", what, need, f, decl.join(", "))),
            None => self.error(format!("`{}` needs {}, which is not allowed here", what, need)),
        }
    }

    // ---- patterns ------------------------------------------------------

    /// Names a pattern binds, with their types.
    fn bind_pattern(&mut self, p: &Pattern, t: T, out: &mut Vec<(String, T, Option<Arc<Sig>>)>) {
        match p {
            Pattern::Wild | Pattern::Lit(_) => {}
            Pattern::Bind(n) => out.push((n.clone(), t, None)),
            Pattern::Type(_) => {}
            Pattern::Alt(ps) => {
                for q in ps {
                    self.bind_pattern(q, T::ANY, out);
                }
            }
            Pattern::As(q, n) => {
                self.bind_pattern(q, t, out);
                let nt = match &**q {
                    q if q.is_type_test() => type_test_t(q),
                    Pattern::Lit(l) => lit_t(l),
                    _ => t,
                };
                out.push((n.clone(), nt, None));
            }
            Pattern::List(ps, rest) => {
                for q in ps {
                    self.bind_pattern(q, T::ANY, out);
                }
                if let Some(Some(n)) = rest {
                    out.push((n.clone(), T::of(LIST), None));
                }
            }
            Pattern::Record(fs) => {
                for f in fs {
                    let mut ft = T::ANY;
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                    if f.pat.is_type_test() {
                        ft = type_test_t(&f.pat);
                        if f.default.is_some() {
                            ft = ft.union(T::of(NIL));
                        }
                    }
                    let before = out.len();
                    self.bind_pattern(&f.pat, ft, out);
                    if out.len() == before && f.pat.is_type_test() {
                        out.push((f.path.last().unwrap().clone(), ft, None));
                    }
                }
            }
        }
    }

    // ---- expressions ---------------------------------------------------

    fn expr(&mut self, e: &Expr) -> T {
        match e {
            Expr::Lit(l) => lit_t(l),
            Expr::Template(parts) => {
                for p in parts {
                    if let TPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
                T::of(STR)
            }
            Expr::Ident(n, line) => {
                self.line = *line;
                let (t, _) = self.ident(n);
                t
            }
            Expr::List(items) => {
                for i in items {
                    self.expr(i);
                }
                T::of(LIST)
            }
            Expr::Map(entries) => {
                for (_, v) in entries {
                    self.expr(v);
                }
                T::of(MAP)
            }
            Expr::Unary(op, x, line) => {
                let t = self.expr(x);
                self.line = *line;
                match op {
                    UnOp::Not => {
                        self.js_truthiness(t, x, Truth::Not);
                        T::of(BOOL)
                    }
                    UnOp::Neg => {
                        if t.bits != 0 && t.bits & NUM == 0 {
                            self.error(format!("cannot negate {}", bits_text(t.bits)));
                        }
                        T::of(t.bits & NUM)
                    }
                }
            }
            Expr::Binary(op, a, b, line) => {
                let x = self.expr(a);
                let y = self.expr(b);
                self.line = *line;
                match op {
                    BinOp::Or => self.js_truthiness(x, a, Truth::Or),
                    BinOp::And => self.js_truthiness(x, a, Truth::And),
                    _ => {}
                }
                if matches!(op, BinOp::Div | BinOp::Mod) && matches!(**b, Expr::Lit(Lit::Int(0))) {
                    self.error("division by zero");
                }
                match binop_t(*op, x, y) {
                    Some(t) => t,
                    None => {
                        let verb = match op {
                            BinOp::Add => "add",
                            BinOp::Sub => "subtract",
                            BinOp::Mul => "multiply",
                            BinOp::Div => "divide",
                            BinOp::Mod => "take the remainder of",
                            BinOp::Range => "make a range from",
                            _ => "compare",
                        };
                        let join = if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) { "with" } else { "and" };
                        self.error(format!("cannot {} {} {} {}", verb, bits_text(x.bits), join, bits_text(y.bits)));
                        T::ANY
                    }
                }
            }
            Expr::Coalesce(a, b) => {
                let x = self.expr(a);
                let y = self.expr(b);
                x.without(NIL | ERR).union(y)
            }
            Expr::Pipe(lhs, rhs, line) => {
                let v = self.expr(lhs);
                self.line = *line;
                match &**rhs {
                    Expr::Call(callee, args, l) => {
                        self.line = *l;
                        self.call(callee, args, Some(v))
                    }
                    other => {
                        let (ft, sig) = self.callee(other);
                        self.apply(ft, sig, vec![v], &[], &callee_name(other))
                    }
                }
            }
            Expr::Call(callee, args, line) => {
                self.line = *line;
                self.call(callee, args, None)
            }
            Expr::Field(obj, name, line) => {
                let t = self.expr(obj);
                self.line = *line;
                self.field(t, obj, name)
            }
            Expr::Index(obj, idx, line) => {
                let o = self.expr(obj);
                let i = self.expr(idx);
                self.line = *line;
                let ok = [(LIST, INT), (MAP, STR), (STR, INT), (NIL, ANY)]
                    .iter()
                    .any(|(a, b)| o.bits & a != 0 && i.bits & b != 0);
                if !ok && o.bits != 0 && i.bits != 0 {
                    self.error(format!("cannot index {} with {}", bits_text(o.bits), bits_text(i.bits)));
                }
                T::ANY
            }
            Expr::Try(x) => {
                let t = self.expr(x);
                if t.bits & ERR != 0 {
                    if let Some(f) = self.fns.last_mut() {
                        f.exits = f.exits.union(T::error(t.err));
                    }
                    if let Some(f) = self.fns.last() {
                        if let Some(e) = f.err {
                            let name = f.name.clone();
                            if t.err != 0 && t.err != ANY && t.err & e == 0 {
                                self.error(format!(
                                    "`?` passes on err({}), but `{}` declares errors as `! {}`",
                                    bits_text(t.err),
                                    name,
                                    bits_text(e)
                                ));
                            }
                        }
                    }
                }
                t.without(ERR)
            }
            Expr::If(c, a, b) => {
                let ct = self.expr(c);
                self.js_truthiness(ct, c, Truth::Cond);
                let x = self.expr(a);
                let y = match b {
                    Some(b) => self.expr(b),
                    None => T::of(NIL),
                };
                x.union(y)
            }
            Expr::Match(subj, arms, line) => {
                let st = self.expr(subj);
                self.line = *line;
                let mut t = T::NEVER;
                for arm in arms {
                    self.push();
                    let mut binds = Vec::new();
                    self.bind_pattern(&arm.pat, st, &mut binds);
                    for (n, bt, sig) in binds {
                        self.define(&n, bt, sig, Kind::Bound);
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    t = t.union(self.expr(&arm.body));
                    self.pop();
                }
                t
            }
            Expr::For(pat, iter, body, line) => {
                let it = self.expr(iter);
                self.line = *line;
                if it.bits != 0 && it.bits & (LIST | MAP | STR | NIL) == 0 {
                    self.error(format!("cannot loop over {}", bits_text(it.bits)));
                }
                self.push();
                let item = if it.bits & !(STR | NIL) == 0 && it.bits & STR != 0 { T::of(STR) } else { T::ANY };
                let mut binds = Vec::new();
                self.bind_pattern(pat, item, &mut binds);
                for (n, bt, sig) in binds {
                    self.define(&n, bt, sig, Kind::Bound);
                }
                self.expr(body);
                self.pop();
                T::of(LIST)
            }
            Expr::Block(stmts) => {
                self.push();
                let mut t = T::of(NIL);
                let mut never = false;
                for (i, s) in stmts.iter().enumerate() {
                    let st = self.stmt(s, i + 1 == stmts.len());
                    if matches!(s, Stmt::Return(_)) || (st.bits == 0 && matches!(s, Stmt::Expr(_))) {
                        never = true;
                    }
                    if i + 1 == stmts.len() {
                        t = st;
                    }
                }
                self.pop();
                if never && t.bits == 0 {
                    T::NEVER
                } else {
                    t
                }
            }
            Expr::Lambda(d) => {
                self.line = d.line;
                self.lambda_body(d);
                T::of(FN)
            }
            Expr::Reply(status, body, line) => {
                let st = self.expr(status);
                if let Some(b) = body {
                    self.expr(b);
                }
                self.line = *line;
                if let Some(c) = self.in_component() {
                    self.error(format!("`reply` only works on the server, but <{}> also runs in the browser", c));
                }
                if let Expr::Lit(Lit::Int(n)) = &**status {
                    if !(100..=599).contains(n) {
                        self.error(format!("reply status must be an HTTP status code (100-599), got {}", n));
                    }
                } else if st.bits != 0 && st.bits & INT == 0 {
                    self.error(format!("reply status must be an int, got {}", bits_text(st.bits)));
                }
                T::NEVER
            }
            Expr::Html(el, _) => {
                self.html(el);
                T::of(HTML)
            }
        }
    }

    fn callee(&mut self, e: &Expr) -> (T, Option<Arc<Sig>>) {
        match e {
            Expr::Ident(n, line) => {
                self.line = *line;
                self.ident(n)
            }
            Expr::Lambda(d) => {
                self.line = d.line;
                self.lambda_body(d);
                (T::of(FN), Some(Arc::new(Sig::of_def(d))))
            }
            other => (self.expr(other), None),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], pre: Option<T>) -> T {
        let line = self.line;
        // `x.f(args)`: a function stored in map x, or `f(x, args)`.
        if let Expr::Field(obj, name, _) = callee {
            if let Expr::Ident(m, _) = &**obj {
                if MODULES.contains(&m.as_str()) && !self.is_local(m) {
                    return self.module_call(m, name, args, pre);
                }
            }
            let recv = self.expr(obj);
            self.line = line;
            let mut argts: Vec<T> = pre.into_iter().collect();
            let arg_types: Vec<T> = args.iter().map(|a| self.expr(a)).collect();
            self.line = line;
            if recv.bits & MAP != 0 {
                // May be a function stored in the map: nothing more to know.
                let found = self.scopes.iter().rev().find_map(|s| s.get(name).cloned()).or_else(|| self.globals.get(name).cloned());
                if let Some(v) = found {
                    self.record(name, &v);
                }
                return T::ANY;
            }
            let found = self.scopes.iter().rev().find_map(|s| s.get(name).cloned()).or_else(|| self.globals.get(name).cloned());
            match found {
                Some(v) if v.t.bits & FN != 0 => {
                    self.record(name, &v);
                    let mut all = vec![recv];
                    all.append(&mut argts);
                    all.extend(arg_types);
                    return self.apply(v.t, v.sig.clone(), all, args, name);
                }
                _ => {
                    if recv.bits != 0 {
                        self.error(format!("{} has no method `{}`", bits_text(recv.bits), name));
                    }
                    return T::ANY;
                }
            }
        }
        if let Expr::Ident(n, _) = callee {
            self.warn_shadowed_builtin(n);
        }
        let (ft, sig) = self.callee(callee);
        self.line = line;
        let mut argts: Vec<T> = pre.into_iter().collect();
        for a in args {
            argts.push(self.expr(a));
        }
        self.line = line;
        self.apply(ft, sig, argts, args, &callee_name(callee))
    }

    /// Strings and numbers are always true in Quillo, where only `null`,
    /// `false` and errors are false; in JavaScript, `""` and `0` are false
    /// too. Code that reads the same in both then means something else, and
    /// nothing fails: `name || "anonymous"` keeps an empty name. Warned only
    /// where the value is known to be a string or a number (or null).
    fn js_truthiness(&mut self, t: T, e: &Expr, how: Truth) {
        if t.bits & (STR | NUM) == 0 || t.bits & !(STR | NUM | NIL) != 0 {
            return;
        }
        let x = match e {
            Expr::Ident(n, _) => n.clone(),
            Expr::Field(o, n, _) if matches!(**o, Expr::Ident(..)) => format!("{}.{}", callee_name(o), n),
            _ => "x".into(),
        };
        let what = if x == "x" { "this value".to_string() } else { format!("`{}`", x) };
        let nullable = t.bits & NIL != 0;
        // What JavaScript would test. A value that may be null is checked
        // for null first: `null != 0` holds, and `null > 0` is an error.
        let mut empties = Vec::new();
        if t.bits & STR != 0 {
            empties.push("\"\"");
        }
        if t.bits & NUM != 0 {
            empties.push("0");
        }
        let empty = empties.join(" or ");
        let mut tests: Vec<String> = empties.iter().map(|e| format!("{} != {}", x, e)).collect();
        if nullable {
            tests.insert(0, format!("{} != null", x));
        }
        let test = tests.join(" && ");
        let unless_null = if nullable { " unless it is null" } else { "" };
        let msg = match how {
            Truth::Or => format!(
                "{what} counts as true even when it is {empty} (unlike JavaScript), so `||` keeps it{unless_null}: write `{test} ? {x} : ...`{}",
                if nullable { format!(", or `{x} ?? ...` to replace only null") } else { String::new() }
            ),
            Truth::And => format!(
                "{what} counts as true even when it is {empty} (unlike JavaScript), so `&&` always goes on{unless_null}: write `{test} && ...`"
            ),
            Truth::Not => format!(
                "{what} counts as true even when it is {empty} (unlike JavaScript), so `!{x}` is false{unless_null}: write `{}`",
                test.replace("!=", "==").replace("&&", "||")
            ),
            Truth::Cond => format!(
                "{what} counts as true even when it is {empty} (unlike JavaScript), so this condition holds{unless_null}: write `{test}`{}",
                if nullable { format!(", or `{x} != null` to test only for null") } else { String::new() }
            ),
        };
        self.warn(msg);
    }

    /// `last(items)` where `last` is a local variable: almost always a
    /// mistake, and invisible when the variable's type is unknown.
    fn warn_shadowed_builtin(&mut self, name: &str) {
        let local = self.scopes.iter().rev().find_map(|s| s.get(name).cloned());
        let Some(v) = local else { return };
        if v.t.bits & FN != 0 && v.t.bits != ANY || v.sig.is_some() {
            return;
        }
        if BUILTINS.iter().any(|(n, _, _)| *n == name) {
            self.warn(format!(
                "`{}` here is your variable (line {}), not the builtin `{}`; rename the variable to call the builtin",
                name, v.line, name
            ));
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains_key(name))
    }

    /// `json.parse(s)`, `db.query(...)`, `http.get(...)`.
    fn module_call(&mut self, m: &str, f: &str, args: &[Expr], pre: Option<T>) -> T {
        let line = self.line;
        let full = format!("{}.{}", m, f);
        let mut argts: Vec<T> = pre.into_iter().collect();
        for a in args {
            argts.push(self.expr(a));
        }
        self.line = line;
        let Some((_, p, r)) = BUILTINS.iter().find(|(n, _, _)| *n == full) else {
            self.error(format!("`{}` has no function `{}`", m, f));
            return T::ANY;
        };
        if !self.quiet_mode() {
            self.out.references.push(Reference {
                file: self.file.to_string(),
                line,
                name: f.to_string(),
                def: None,
                detail: Sig::builtin(&full, p, r).text(),
            });
        }
        if m != "json" {
            self.browser_forbidden(&format!("`{}`", full));
        }
        let sig = Arc::new(Sig::builtin(&full, p, r));
        self.apply(T::of(FN), Some(sig), argts, args, &full)
    }

    fn quiet_mode(&self) -> bool {
        self.quiet > 0
    }

    /// A call of something of type `ft`: arguments, effects, browser rules
    /// and the result type.
    fn apply(&mut self, ft: T, sig: Option<Arc<Sig>>, args: Vec<T>, arg_exprs: &[Expr], name: &str) -> T {
        if ft.bits != 0 && ft.bits & FN == 0 {
            self.error(format!("cannot call `{}`: it is {}, not a function", name, bits_text(ft.bits)));
            return T::ANY;
        }
        let Some(sig) = sig else { return T::ANY };
        if sig.component {
            self.error(format!("`{}` is a component; write <{} ... /> instead of calling it", name, sig.name));
        }
        let n = sig.params.len();
        if args.len() > n && !sig.variadic {
            self.error(format!("`{}` takes {} argument{}, got {}", name, n, if n == 1 { "" } else { "s" }, args.len()));
        }
        for (i, p) in sig.params.iter().enumerate() {
            match args.get(i) {
                None if !p.optional => {
                    self.error(format!("missing argument `{}` in call to `{}`", p.name, name));
                    break;
                }
                Some(a) if p.annotated && a.bits != 0 && a.bits & p.ty == 0 => {
                    let what = if sig.builtin { format!("argument {}", i + 1) } else { format!("argument `{}`", p.name) };
                    self.error(format!("{} of `{}` must be {}, got {}", what, name, bits_text(p.ty), bits_text(a.bits)));
                }
                _ => {}
            }
        }
        if sig.builtin && matches!(name, "db.query" | "db.one" | "db.exec") {
            if arg_exprs.first().map_or(false, sql_built_from_values) {
                self.warn(format!(
                    "`{}` with SQL built from values: a value containing a quote can change the query \
                     (SQL injection); write `?` in the SQL and pass the values after it",
                    name
                ));
            }
        }
        // Effects.
        let (allowed, restrictor) = match self.fns.last() {
            Some(f) => (f.allowed, f.restrictor.clone()),
            None => (ALL_EFFECTS, None),
        };
        let needs = if sig.builtin { sig.effects.unwrap_or(0) } else { sig.effects.unwrap_or(0) };
        if needs & !allowed != 0 {
            self.effect_error(name, needs & !allowed, restrictor.as_ref());
        } else if allowed != ALL_EFFECTS && !sig.builtin && sig.effects.is_none() {
            let inferred = self.fn_effects(&sig);
            if inferred & !allowed != 0 {
                let names = effect_names(inferred & !allowed);
                if let Some((f, decl)) = &restrictor {
                    self.warn(format!(
                        "`{}` may use the {} effect{}, but `{}` is declared `with {}`",
                        name,
                        names.join(", "),
                        if names.len() == 1 { "" } else { "s" },
                        f,
                        decl.join(", ")
                    ));
                }
            }
        }
        // The browser.
        if let Some(c) = self.in_component() {
            if sig.builtin && BROWSER_ONLY_SERVER.contains(&sig.name.as_str()) {
                self.error(format!("`{}` is not available in the browser, where <{}> also runs; use a `server fn`", sig.name, c));
            }
            let nested = self.fns.last().map_or(false, |f| f.nested);
            if sig.server && !nested {
                self.error(format!(
                    "`{}` waits on the server, so <{}> can only call it from an event handler (@click={{fn() {{ ... }}}}) or to set state: ~x = {}(...)",
                    name, c, name
                ));
            }
        }
        if sig.builtin && sig.name == "live" {
            match arg_exprs.first() {
                Some(Expr::Ident(f, _)) => {
                    let is_server = self.globals.get(f).and_then(|v| v.sig.clone()).map_or(true, |s| s.server || s.def.is_none() && !s.builtin);
                    if !is_server {
                        self.error(format!("`live` needs a server fn: `{}` is not one", f));
                    }
                }
                Some(_) => self.error("`live` needs a server fn by name: ~items = live(load_items, ...)"),
                None => {}
            }
            if let Some(c) = self.in_component() {
                if !self.live_ok {
                    self.error(format!("in <{}>, `live(...)` sets state: ~items = live(load_items, ...)", c));
                }
            }
            return T::ANY;
        }
        if sig.builtin && sig.name == "err" {
            return T::error(args.first().map_or(NIL, |a| a.bits));
        }
        if sig.builtin && sig.name == "unwrap" {
            return args.first().map_or(T::ANY, |a| a.without(ERR));
        }
        self.fn_ret(&sig)
    }

    fn field(&mut self, t: T, obj: &Expr, name: &str) -> T {
        let _ = obj;
        if t.bits & (MAP | NIL) != 0 || t.is_any() {
            if t.bits & MAP == 0 && t.bits & ERR != 0 && name == "error" {
                return T::of(t.err);
            }
            return T::ANY;
        }
        if t.bits & ERR != 0 && name == "error" {
            return T::of(t.err);
        }
        if t.bits & RESP != 0 && matches!(name, "status" | "headers" | "body") {
            return T::ANY;
        }
        let found = self.scopes.iter().rev().find_map(|s| s.get(name).cloned()).or_else(|| self.globals.get(name).cloned());
        match found {
            Some(v) if v.t.bits & FN != 0 => {
                self.record(name, &v);
                self.apply(v.t, v.sig.clone(), vec![t], &[], name)
            }
            _ => {
                if t.bits != 0 {
                    self.error(format!("{} has no field `{}`", bits_text(t.bits), name));
                }
                T::ANY
            }
        }
    }

    fn html(&mut self, el: &HtmlEl) {
        self.line = el.line;
        if let Some(id) = &el.id {
            self.expr(id);
        }
        for c in &el.classes {
            self.expr(c);
        }
        let is_view = el.tag.chars().next().map_or(false, |c| c.is_uppercase());
        let mut given: Vec<(String, T)> = Vec::new();
        let mut spread = false;
        for (name, v) in &el.attrs {
            let t = match v {
                AttrVal::Bool => T::of(BOOL),
                AttrVal::Expr(e) => self.expr(e),
                AttrVal::Spread(e) => {
                    let t = self.expr(e);
                    self.line = el.line;
                    if t.bits != 0 && t.bits & (MAP | NIL) == 0 {
                        self.error(format!("`{{...}}` in <{}> needs a map, got {}", el.tag, bits_text(t.bits)));
                    }
                    spread = true;
                    continue;
                }
            };
            self.line = el.line;
            if let Some(ev) = name.strip_prefix('@') {
                if is_view {
                    self.error(format!("<{}> is a view; `@{}` handlers go on HTML elements", el.tag, ev));
                } else if t.bits != 0 && t.bits & FN == 0 {
                    self.error(format!("`@{}` needs a function, got {}", ev, bits_text(t.bits)));
                }
            } else if let Some(prop) = name.strip_prefix("bind:") {
                if !matches!(prop, "value" | "checked") {
                    self.error(format!("`bind:{}` is not supported (use bind:value or bind:checked)", prop));
                }
                match v {
                    AttrVal::Expr(Expr::Ident(n, _)) => {
                        let found = self.scopes.iter().rev().find_map(|s| s.get(n).cloned());
                        if found.is_none() && self.in_component().is_some() && self.globals.contains_key(n) {
                            self.error(format!("`bind:{}` needs component state or a local variable, like ~{}", prop, n));
                        }
                    }
                    _ => self.error(format!("`bind:{}` needs a variable: bind:{}={{name}}", prop, prop)),
                }
            }
            given.push((name.clone(), t));
        }
        for c in &el.children {
            match c {
                HtmlChild::El(e) => self.html(e),
                HtmlChild::Expr(x) => {
                    self.expr(x);
                }
            }
        }
        if !is_view {
            return;
        }
        self.line = el.line;
        let (t, sig) = self.ident(&el.tag);
        let Some(sig) = sig else {
            if t.bits != 0 && t.bits & FN == 0 {
                self.error(format!("<{}> is not a view: it is {}", el.tag, bits_text(t.bits)));
            }
            return;
        };
        if sig.builtin {
            self.error(format!("<{}> is a builtin function, not a view", el.tag));
            return;
        }
        for (n, t) in &given {
            if n == "key" || n.starts_with('@') {
                continue;
            }
            match sig.params.iter().find(|p| &p.name == n) {
                None => self.error(format!("<{}> has no parameter `{}`", el.tag, n)),
                Some(p) => {
                    if p.annotated && t.bits != 0 && t.bits & p.ty == 0 {
                        self.error(format!("`{}` of <{}> must be {}, got {}", n, el.tag, bits_text(p.ty), bits_text(t.bits)));
                    }
                }
            }
        }
        for p in &sig.params {
            if p.name != "children" && !p.optional && !spread && !given.iter().any(|(n, _)| n == &p.name) {
                self.error(format!("<{}> needs `{}`", el.tag, p.name));
            }
        }
        if let Some(c) = self.in_component() {
            if sig.server {
                self.error(format!("<{}> is a server fn, not a view (in <{}>)", el.tag, c));
            }
        }
    }
}

fn is_view(d: &FnDef) -> bool {
    d.name.chars().next().map_or(false, |c| c.is_uppercase())
}

fn route_path(r: &RouteDef) -> String {
    let mut s = String::new();
    for seg in &r.path {
        s.push('/');
        match seg {
            Seg::Lit(l) => s.push_str(l),
            Seg::Param(n, Some(t)) => s.push_str(&format!(":{}({})", n, t)),
            Seg::Param(n, None) => s.push_str(&format!(":{}", n)),
            Seg::Rest(n) => s.push_str(&format!("*{}", n)),
        }
    }
    if s.is_empty() {
        s.push('/');
    }
    s
}

/// Where a value is used as true or false.
#[derive(Clone, Copy)]
enum Truth {
    Or,
    And,
    Not,
    Cond,
}

fn callee_name(e: &Expr) -> String {
    match e {
        Expr::Ident(n, _) => n.clone(),
        Expr::Field(_, n, _) => n.clone(),
        _ => "this function".into(),
    }
}

/// Names called in a body (not inside nested function definitions'
/// declarations, but including lambdas, which usually run during the call).
fn called_names(e: &Expr, out: &mut Vec<String>) {
    walk_expr(e, &mut |x| match x {
        Expr::Call(c, _, _) => match &**c {
            Expr::Ident(n, _) => out.push(n.clone()),
            Expr::Field(obj, n, _) => match &**obj {
                Expr::Ident(m, _) if MODULES.contains(&m.as_str()) => out.push(format!("{}.{}", m, n)),
                _ => out.push(n.clone()),
            },
            _ => {}
        },
        Expr::Pipe(_, rhs, _) => {
            if let Expr::Ident(n, _) = &**rhs {
                out.push(n.clone())
            }
        }
        _ => {}
    });
}

/// Calls `f` on every expression inside `e`, including `e`.
pub fn walk_expr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Lit(_) | Expr::Ident(..) => {}
        Expr::Template(parts) => {
            for p in parts {
                if let TPart::Expr(x) = p {
                    walk_expr(x, f)
                }
            }
        }
        Expr::List(xs) => xs.iter().for_each(|x| walk_expr(x, f)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| walk_expr(x, f)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => walk_expr(x, f),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            walk_expr(a, f);
            walk_expr(b, f)
        }
        Expr::Call(c, args, _) => {
            walk_expr(c, f);
            args.iter().for_each(|x| walk_expr(x, f))
        }
        Expr::If(c, a, b) => {
            walk_expr(c, f);
            walk_expr(a, f);
            if let Some(b) = b {
                walk_expr(b, f)
            }
        }
        Expr::Match(s, arms, _) => {
            walk_expr(s, f);
            for a in arms {
                if let Some(g) = &a.guard {
                    walk_expr(g, f)
                }
                walk_expr(&a.body, f)
            }
        }
        Expr::For(_, i, b, _) => {
            walk_expr(i, f);
            walk_expr(b, f)
        }
        Expr::Block(stmts) => walk_stmts(stmts, f),
        Expr::Lambda(d) => walk_expr(&d.body, f),
        Expr::Reply(s, b, _) => {
            walk_expr(s, f);
            if let Some(b) = b {
                walk_expr(b, f)
            }
        }
        Expr::Html(el, _) => walk_html(el, f),
    }
}

fn walk_stmts(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    for s in stmts {
        match s {
            Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => walk_expr(e, f),
            Stmt::Return(Some(e)) => walk_expr(e, f),
            Stmt::Fn(d) => walk_expr(&d.body, f),
            Stmt::Route(r) => walk_expr(&r.body, f),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Return(None) => {}
        }
    }
}

fn walk_html(el: &HtmlEl, f: &mut dyn FnMut(&Expr)) {
    if let Some(id) = &el.id {
        walk_expr(id, f)
    }
    el.classes.iter().for_each(|c| walk_expr(c, f));
    for (_, v) in &el.attrs {
        if let AttrVal::Expr(e) | AttrVal::Spread(e) = v {
            walk_expr(e, f)
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(e) => walk_html(e, f),
            HtmlChild::Expr(x) => walk_expr(x, f),
        }
    }
}

fn collect_assigned(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign(n, _, e, _) => {
                out.insert(n.clone());
                assigned_in(e, out)
            }
            Stmt::Let(_, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => assigned_in(e, out),
            Stmt::Return(Some(e)) => assigned_in(e, out),
            Stmt::Fn(d) => assigned_in(&d.body, out),
            Stmt::Route(r) => assigned_in(&r.body, out),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Return(None) => {}
        }
    }
}

fn assigned_in(e: &Expr, out: &mut HashSet<String>) {
    walk_expr(e, &mut |x| {
        if let Expr::Block(stmts) = x {
            for s in stmts {
                if let Stmt::Assign(n, ..) = s {
                    out.insert(n.clone());
                }
            }
        }
    });
}

/// Whether SQL text is put together from values: `${x}` in a template,
/// sql`...${x}...`, or `"..." + x`.
fn sql_built_from_values(e: &Expr) -> bool {
    let interpolates = |parts: &[TPart]| parts.iter().any(|p| matches!(p, TPart::Expr(_)));
    match e {
        Expr::Template(parts) => interpolates(parts),
        Expr::Call(_, args, _) => matches!(args.as_slice(), [Expr::Template(parts)] if interpolates(parts)),
        Expr::Binary(BinOp::Add, a, b, _) => {
            let text = |x: &Expr| matches!(x, Expr::Lit(Lit::Str(_)) | Expr::Template(_));
            (text(a) && !matches!(**b, Expr::Lit(_))) || (text(b) && !matches!(**a, Expr::Lit(_))) || sql_built_from_values(a)
        }
        _ => false,
    }
}

/// The type a type-testing pattern accepts: `int | null` is int or null.
fn type_test_t(p: &Pattern) -> T {
    match p {
        Pattern::Type(ty) => T::of(name_bits(ty)),
        Pattern::Lit(Lit::Nil) => T::of(NIL),
        Pattern::Alt(ps) => ps.iter().fold(T::NEVER, |t, q| t.union(type_test_t(q))),
        _ => T::ANY,
    }
}

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
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}
