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
//! - fields a map whose fields are known does not have (`{ title }.titel`);
//! - unused `let` variables (a warning).
//!
//! Lists know what they hold and maps their fields where something says so
//! (a literal, an annotation such as `list<{ id: int }>`, a builtin like
//! `split` or `merge`), and a lambda given to `map`, `filter`, ... takes the
//! list's items. With `Options::strict_null`, values that may be null where
//! null does not work are warnings too, after narrowing (`x != null`, an
//! early `return`, `match` arms after `null`).
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
const DEC: u16 = 2048;
const NUM: u16 = INT | FLOAT;
const ANY: u16 = 4095;
const BASE: [(u16, &str); 12] = [
    (NIL, "null"),
    (BOOL, "bool"),
    (INT, "int"),
    (FLOAT, "float"),
    (DEC, "dec"),
    (STR, "str"),
    (LIST, "list"),
    (MAP, "map"),
    (FN, "fn"),
    (HTML, "html"),
    (RESP, "response"),
    (ERR, "error"),
];

/// A set of possible types; `err` is what an error value may contain.
/// What a list holds (`elem`) and a map's fields (`shape`) are indexes into
/// the tables below, 0 when unknown: they say something only while `bits`
/// has `LIST` (or `MAP`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct T {
    bits: u16,
    err: u16,
    elem: u32,
    shape: u32,
}

impl T {
    const fn of(bits: u16) -> T {
        T { bits, err: if bits & ERR != 0 { ANY } else { 0 }, elem: 0, shape: 0 }
    }
    const ANY: T = T { bits: ANY, err: ANY, elem: 0, shape: 0 };
    const NEVER: T = T { bits: 0, err: 0, elem: 0, shape: 0 };
    fn error(payload: u16) -> T {
        T { bits: ERR, err: payload, elem: 0, shape: 0 }
    }
    fn union(self, o: T) -> T {
        T {
            bits: self.bits | o.bits,
            err: self.err | o.err,
            elem: join_part(self.bits & LIST != 0, self.elem, o.bits & LIST != 0, o.elem, union_elems),
            shape: join_part(self.bits & MAP != 0, self.shape, o.bits & MAP != 0, o.shape, union_shapes),
        }
    }
    fn without(self, bits: u16) -> T {
        let b = self.bits & !bits;
        T {
            bits: b,
            err: if b & ERR != 0 { self.err } else { 0 },
            elem: if b & LIST != 0 { self.elem } else { 0 },
            shape: if b & MAP != 0 { self.shape } else { 0 },
        }
    }
    fn is_any(self) -> bool {
        self.bits == ANY
    }
    /// Anything, give or take null and errors (`x?` and `x ?? y` of `any`):
    /// nothing to warn about.
    fn vague(self) -> bool {
        self.bits | NIL | ERR == ANY
    }
    /// What the list holds, if this is a list (or null) and that is known.
    fn items(self) -> Option<T> {
        (self.bits & LIST != 0 && self.bits & !(LIST | NIL) == 0 && self.elem != 0).then(|| elem_of(self.elem))
    }
    /// The map's fields, if this is a map (or null) and they are known.
    fn fields(self) -> Option<Shape> {
        if self.bits & MAP != 0 && self.bits & !(MAP | NIL) == 0 { shape_of(self.shape) } else { None }
    }
}

// ---- lists' elements and maps' fields ------------------------------------

/// A map's fields, by name.
type Shape = Arc<[(Arc<str>, T)]>;

#[derive(Default)]
struct Tables {
    elems: Vec<T>,
    elem_ids: HashMap<T, u32>,
    shapes: Vec<Shape>,
    shape_ids: HashMap<Shape, u32>,
}

thread_local! {
    /// Service programs being checked: one using another that uses it is not checked twice over.
    static SERVICE_CHECKS: std::cell::RefCell<HashSet<PathBuf>> = std::cell::RefCell::new(HashSet::new());
    static TABLES: std::cell::RefCell<Tables> = std::cell::RefCell::new(Tables::default());
    /// Checks in progress on this thread (a mounted program is checked
    /// inside its parent's check): the tables are emptied by the outermost.
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The program's own types (`type User = { ... }`), found before its
    /// code is checked; each program checked has its own.
    static RECORDS: std::cell::RefCell<HashMap<String, Arc<TypeDef>>> = std::cell::RefCell::new(HashMap::new());
    /// How deep in its own types a type is being worked out (one may name itself).
    static RECORD_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Working out what `User(...)` takes: in it, and in the records inside,
    /// a field with a default may be left out.
    static RECORD_INPUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn record(name: &str) -> Option<Arc<TypeDef>> {
    if !crate::ast::is_type_name(name) {
        return None;
    }
    RECORDS.with(|r| r.borrow().get(name).cloned())
}

/// Works out `f` one level deeper in the program's types; past a few, the
/// type is left open (a map whose fields are not known).
fn in_record<R>(open: R, f: impl FnOnce() -> R) -> R {
    let d = RECORD_DEPTH.with(|d| d.get());
    if d >= 4 {
        return open;
    }
    RECORD_DEPTH.with(|c| c.set(d + 1));
    let r = f();
    RECORD_DEPTH.with(|c| c.set(d));
    r
}

/// A record's fields, as a shape (as `User(...)` takes them, with RECORD_INPUT).
fn record_shape(td: &TypeDef) -> u32 {
    let input = RECORD_INPUT.with(|c| c.get());
    in_record(0, || {
        shape_id(
            td.ty
                .fields
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|(k, ft)| {
                    let t = ty_t(ft);
                    let t = if input && td.defaults.iter().any(|(d, _)| d == k) { t.union(T::of(NIL)) } else { t };
                    (Arc::from(k.as_str()), t)
                })
                .collect(),
        )
    })
}

/// What `User(...)` takes and gives: a map in which the fields with a
/// default, or that may be null, may be left out; a User, or an error value.
/// A `.qwer` function as the checker sees it: its parameters' and result's
/// Quillo types, from their Rust types.
fn native_sig(e: &crate::native::Export, file: &Arc<str>) -> Sig {
    let q = |t: &str| crate::native::quillo_type(t).unwrap_or_else(|_| "any".into());
    let params: Vec<String> = e.params.iter().map(|(_, t)| q(t)).collect();
    let ret = e.ret.as_deref().map_or("null".to_string(), q);
    let mut sig = Sig::builtin(&e.name, &params.join(", "), &ret);
    for (p, (name, _)) in sig.params.iter_mut().zip(&e.params) {
        p.name = name.clone();
    }
    sig.native = Some(file.clone());
    sig.effects = Some(e.effects);
    sig
}

/// How Quillo sees a `.qwer` function: `brightness(str) -> float | error(str)`.
pub fn native_signature(e: &crate::native::Export) -> String {
    native_sig(e, &Arc::from("")).text()
}

fn record_sig(td: &TypeDef) -> Sig {
    let rec = ty_t(&Ty { names: vec![td.name.clone()], optional: false, elem: None, fields: None });
    let was = RECORD_INPUT.with(|c| c.replace(true));
    let input = if td.is_record() {
        let mut t = T::of(MAP | NIL);
        t.shape = record_shape(td);
        t
    } else {
        ty_t(&td.ty).union(T::of(NIL))
    };
    RECORD_INPUT.with(|c| c.set(was));
    Sig {
        name: td.name.clone(),
        params: vec![PSig { name: "fields".into(), ty: input.bits, full: input, optional: true, annotated: true }],
        variadic: false,
        ret: Some(rec.union(T::error(STR))),
        err: None,
        effects: Some(0),
        server: false,
        component: false,
        builtin: false,
        def: None,
        native: None,
    }
}

fn elem_id(e: T) -> u32 {
    if e.is_any() {
        return 0;
    }
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(i) = t.elem_ids.get(&e) {
            return *i;
        }
        t.elems.push(e);
        let i = t.elems.len() as u32;
        t.elem_ids.insert(e, i);
        i
    })
}

fn elem_of(id: u32) -> T {
    if id == 0 {
        return T::ANY;
    }
    TABLES.with(|t| t.borrow().elems[id as usize - 1])
}

fn shape_id(mut fields: Vec<(Arc<str>, T)>) -> u32 {
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    fields.dedup_by(|a, b| a.0 == b.0);
    let shape: Shape = fields.into();
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(i) = t.shape_ids.get(&shape) {
            return *i;
        }
        t.shapes.push(shape.clone());
        let i = t.shapes.len() as u32;
        t.shape_ids.insert(shape, i);
        i
    })
}

fn shape_of(id: u32) -> Option<Shape> {
    if id == 0 {
        return None;
    }
    TABLES.with(|t| Some(t.borrow().shapes[id as usize - 1].clone()))
}

fn list_of(e: T) -> T {
    T { bits: LIST, err: 0, elem: elem_id(e), shape: 0 }
}

/// What `request` holds, and nothing else, so `request.qurey` is said, with `query` offered
/// (FEEDBACK45.md, Q-511; branch feedback40-43). The server makes it in server.rs.
fn request_t() -> T {
    let text = T::of(STR);
    let any_map = T::of(MAP);
    map_of(vec![
        (Arc::from("method"), text),
        (Arc::from("path"), text),
        // As routes read it, and the host as it names one (FEEDBACK47.md, Q-537 and Q-545).
        (Arc::from("route_path"), text),
        (Arc::from("host"), text),
        (Arc::from("query"), any_map),
        (Arc::from("query_string"), text),
        (Arc::from("params"), any_map),
        (Arc::from("headers"), any_map),
        (Arc::from("cookies"), any_map),
        (Arc::from("body"), text),
        (Arc::from("json"), T::ANY),
        (Arc::from("form"), any_map),
        (Arc::from("files"), T::ANY),
        (Arc::from("ip"), text),
        (Arc::from("id"), text),
        // In a function a service answers with (service.rs).
        (Arc::from("call"), text),
        (Arc::from("caller"), T::ANY),
        (Arc::from("user"), T::ANY),
        (Arc::from("forwarded"), T::ANY),
    ])
}

fn map_of(fields: Vec<(Arc<str>, T)>) -> T {
    T { bits: MAP, err: 0, elem: 0, shape: shape_id(fields) }
}

/// The part (element or fields) of a union: unknown if either side's is.
fn join_part(a_has: bool, a: u32, b_has: bool, b: u32, join: fn(u32, u32) -> u32) -> u32 {
    match (a_has, b_has) {
        (true, true) if a == b => a,
        (true, true) if a == 0 || b == 0 => 0,
        (true, true) => join(a, b),
        (true, false) => a,
        (false, true) => b,
        (false, false) => 0,
    }
}

fn union_elems(a: u32, b: u32) -> u32 {
    elem_id(elem_of(a).union(elem_of(b)))
}

/// One map or the other: every field either has, null where one has none.
fn union_shapes(a: u32, b: u32) -> u32 {
    let (Some(x), Some(y)) = (shape_of(a), shape_of(b)) else { return 0 };
    let find = |s: &Shape, k: &str| s.iter().find(|(n, _)| &**n == k).map(|(_, t)| *t);
    let mut out: Vec<(Arc<str>, T)> = Vec::new();
    for (k, t) in x.iter() {
        out.push((k.clone(), t.union(find(&y, k).unwrap_or(T::of(NIL)))));
    }
    for (k, t) in y.iter() {
        if find(&x, k).is_none() {
            out.push((k.clone(), t.union(T::of(NIL))));
        }
    }
    shape_id(out)
}

/// `merge(a, b)` and `a + b` on maps: b's fields over a's.
fn merge_maps(a: T, b: T) -> T {
    match (a.fields(), b.fields()) {
        (Some(x), Some(y)) if a.bits & NIL == 0 && b.bits & NIL == 0 => {
            let mut out: Vec<(Arc<str>, T)> = y.iter().cloned().collect();
            out.extend(x.iter().filter(|(k, _)| !y.iter().any(|(n, _)| n == k)).cloned());
            map_of(out)
        }
        _ => T::of(MAP),
    }
}

/// A type as the checker knows it: `list<int>`, `{ id: int, title: str } | null`.
fn t_text(t: T) -> String {
    if t.is_any() || (t.elem == 0 && t.shape == 0) {
        return bits_text(t.bits);
    }
    let mut parts: Vec<String> = Vec::new();
    let rest = t.bits & !(NIL | if t.elem != 0 { LIST } else { 0 } | if t.shape != 0 { MAP } else { 0 });
    if rest != 0 {
        parts.push(bits_text(rest));
    }
    if t.elem != 0 && t.bits & LIST != 0 {
        parts.push(format!("list<{}>", t_text(elem_of(t.elem))));
    }
    if let Some(s) = shape_of(t.shape).filter(|_| t.bits & MAP != 0) {
        let mut fs: Vec<String> = s.iter().take(6).map(|(k, ft)| format!("{}: {}", k, t_text(*ft))).collect();
        if s.len() > 6 {
            fs.push("…".into());
        }
        parts.push(format!("{{ {} }}", fs.join(", ")));
    }
    if t.bits & NIL != 0 {
        parts.push("null".into());
    }
    parts.join(" | ")
}

/// An annotation's type, with what its lists hold and its maps' fields.
fn ty_t(t: &Ty) -> T {
    let mut out = T::of(ty_bits(t));
    if let (Some(e), true) = (&t.elem, out.bits & LIST != 0) {
        out.elem = elem_id(ty_t(e));
    }
    if let (Some(fs), true) = (&t.fields, out.bits & MAP != 0) {
        out.shape = shape_id(fs.iter().map(|(k, ft)| (Arc::from(k.as_str()), ty_t(ft))).collect());
    }
    if t.names.iter().any(|n| n == "money") && t.fields.is_none() {
        out.shape = money_t().shape;
    }
    // One of the program's own types: its fields (or what it stands for).
    let own: Vec<Arc<TypeDef>> = t.names.iter().filter_map(|n| record(n)).collect();
    if let ([td], true) = (own.as_slice(), t.fields.is_none()) {
        if td.is_record() {
            out.shape = record_shape(td);
        } else {
            let a = in_record(T::ANY, || ty_t(&td.ty));
            if out.elem == 0 {
                out.elem = a.elem;
            }
            if out.shape == 0 {
                out.shape = a.shape;
            }
        }
    }
    out
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
    if n.starts_with('"') {
        return STR;
    }
    if let Some(td) = record(n) {
        return if td.is_record() { MAP } else { in_record(ANY, || ty_bits(&td.ty)) };
    }
    match n {
        "any" => ANY,
        "num" => NUM,
        // A date is text (FEEDBACK13.md, Q-093; branch open-fixes).
        "date" => STR,
        // Money is a map, `{ amount: dec, currency: str }` (FEEDBACK50.md, Q-621; branch
        // feedback50-fixes).
        "money" => MAP,
        "nil" | "null" => NIL,
        other => BASE.iter().find(|(_, x)| *x == other).map_or(ANY, |(b, _)| *b),
    }
}

/// A money value's type (src/money.rs).
fn money_t() -> T {
    map_of(vec![(Arc::from("amount"), T::of(DEC)), (Arc::from("currency"), T::of(STR))])
}

/// Whether a type is money's shape, `{ amount, currency }` (or an error besides): what `+`
/// and `-` take as amounts (FEEDBACK50.md, Q-620; branch feedback50-fixes).
fn is_money(t: T) -> bool {
    t.bits & MAP != 0
        && t.bits & !(MAP | ERR) == 0
        && shape_of(t.shape).map_or(false, |s| s.len() == 2 && s.iter().all(|(k, _)| matches!(&**k, "amount" | "currency")))
}

/// Whether a value may be money: of any type, or a map whose shape is money's or not known.
fn maybe_money(t: T) -> bool {
    t.is_any() || (t.bits & MAP != 0 && t.bits & !(MAP | ERR | NIL) == 0 && (t.shape == 0 || is_money(t.without(NIL))))
}

/// After an error about a decimal and a float: what to do about it.
fn dec_hint(x: T, y: T) -> &'static str {
    let mixed = |a: T, b: T| a.bits & DEC != 0 && b.bits & FLOAT != 0;
    if mixed(x, y) || mixed(y, x) {
        ": a dec is exact and a float is not, so convert one with dec(x) or float(x)"
    } else {
        ""
    }
}

fn lit_t(l: &Lit) -> T {
    T::of(match l {
        Lit::Nil => NIL,
        Lit::Bool(_) => BOOL,
        Lit::Int(_) => INT,
        Lit::Float(_) => FLOAT,
        Lit::Dec(_) => DEC,
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
        // Money adds and subtracts, in `a - b`, `t -= x` and state alike; a map whose shape is
        // not known may be money (FEEDBACK50.md, Q-620; branch feedback50-fixes).
        BinOp::Add | BinOp::Sub if is_money(a) && is_money(b) => return Some(money_t().union(T::error(STR))),
        // One side known to be a map: `any - any` stays a number's.
        BinOp::Sub if maybe_money(a) && maybe_money(b) && !(a.is_any() && b.is_any()) => return Some(T::of(MAP | ERR)),
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
                    (DEC, DEC | INT) | (INT, DEC) => Some(DEC),
                    (HTML, HTML) => Some(HTML),
                    (LIST, LIST) => Some(LIST),
                    (MAP, MAP) => Some(MAP),
                    (INT | FLOAT, INT | FLOAT) => Some(FLOAT),
                    _ => None,
                },
                BinOp::Div => match (x, y) {
                    (INT, INT) => Some(INT | FLOAT),
                    (DEC, DEC | INT) | (INT, DEC) => Some(DEC),
                    (INT | FLOAT, INT | FLOAT) => Some(FLOAT),
                    _ => None,
                },
                BinOp::Sub | BinOp::Mul | BinOp::Mod => match (x, y) {
                    (INT, INT) => Some(INT),
                    (DEC, DEC | INT) | (INT, DEC) => Some(DEC),
                    (INT | FLOAT, INT | FLOAT) => Some(FLOAT),
                    _ => None,
                },
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (x, y) {
                    // Lists order item by item (FEEDBACK29.md, Q-358; branch feedback29-fixes).
                    (INT | FLOAT, INT | FLOAT) | (STR, STR) | (DEC, DEC | INT) | (INT, DEC) | (LIST, LIST) => Some(BOOL),
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
/// The server fns `body` calls, or names (`live(load)`), there and in the views and
/// components it shows, as far as they go.
fn server_fns_reached(fns: &HashMap<String, Arc<FnDef>>, body: &Expr) -> Vec<Arc<FnDef>> {
    let mut found: Vec<Arc<FnDef>> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut pending: Vec<Expr> = vec![body.clone()];
    while let Some(e) = pending.pop() {
        let mut next: Vec<Expr> = Vec::new();
        walk_expr(&e, &mut |x| match x {
            Expr::Ident(n, _) => {
                if let Some(d) = fns.get(n.as_str()).filter(|d| d.server) {
                    if seen.insert(format!("fn {}", n)) {
                        found.push(d.clone());
                    }
                }
            }
            Expr::Html(el, _) => walk_html_els(el, &mut |h| {
                if h.tag.starts_with(|c: char| c.is_uppercase()) && seen.insert(h.tag.to_string()) {
                    if let Some(d) = fns.get(h.tag.as_str()) {
                        next.push(d.body.clone());
                    }
                }
            }),
            _ => {}
        });
        pending.extend(next);
    }
    found
}

/// Whether every request route `r` takes, `e` takes first: its method (or `ANY`), a path that
/// covers `r`'s piece by piece, and no pattern that could let a request through to `r`.
fn route_covers(e: &RouteDef, r: &RouteDef) -> bool {
    if e.query.is_some() || e.body_pat.is_some() || !e.guards.is_empty() || (e.method != r.method && e.method != "ANY") {
        return false;
    }
    for (i, es) in e.path.iter().enumerate() {
        if matches!(es, Seg::Rest(_)) {
            return true;
        }
        let Some(rs) = r.path.get(i) else { return false };
        let ok = match (es, rs) {
            (_, Seg::Rest(_)) => false,
            (Seg::Lit(a), Seg::Lit(b)) => a == b,
            // A parameter after a prefix (`/@:name`) takes a piece that starts with it and
            // goes on (FEEDBACK53.md, Q-683).
            (Seg::Param(_, None, p), Seg::Lit(l)) => l.strip_prefix(p.as_str()).is_some_and(|v| p.is_empty() || !v.is_empty()),
            (Seg::Param(_, None, p), Seg::Param(_, _, q)) => q.starts_with(p.as_str()),
            // Typed, the prefix is part of the text checked: `/:id(int)` does not take `/~5`
            // (found in review).
            (Seg::Param(_, Some(t), p), Seg::Param(_, Some(u), q)) => t == u && (q == p || (t == "str" && q.starts_with(p.as_str()))),
            (Seg::Param(_, Some(t), p), Seg::Lit(l)) => l.strip_prefix(p.as_str()).is_some_and(|v| (p.is_empty() || !v.is_empty()) && crate::interp::coerce(t, v).is_some()),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    e.path.len() == r.path.len()
}

/// The middleware of the group a route is in, by name: its body's first call is `__answer`'s.
fn middleware_of(r: &RouteDef) -> Option<String> {
    let Expr::Block(stmts) = &r.body else { return None };
    match stmts.first() {
        Some(Stmt::Expr(Expr::Call(f, args, _))) if matches!(&**f, Expr::Ident(n, _) if n == "__answer") => match args.get(1) {
            Some(Expr::Lit(Lit::Str(s))) => Some(s.to_string()),
            _ => Some("its middleware".to_string()),
        },
        _ => None,
    }
}

/// Whether a server fn checks who calls it: it reads `request` or `session`, there or in a
/// function of the program it calls (`me()`, `signed_in(request)`), as far as they go.
fn guards_itself(fns: &HashMap<String, Arc<FnDef>>, body: &Expr, seen: &mut HashSet<String>) -> bool {
    let mut found = false;
    let mut called: Vec<String> = Vec::new();
    walk_expr(body, &mut |x| {
        if let Expr::Ident(n, _) = x {
            if n == "request" || n == "session" {
                found = true;
            } else if fns.contains_key(n.as_str()) {
                called.push(n.to_string());
            }
        }
    });
    found || called.into_iter().any(|n| seen.insert(n.clone()) && fns.get(&n).is_some_and(|d| guards_itself(fns, &d.body, seen)))
}

/// The Latin letter a Cyrillic or Greek one looks like, or the letter itself.
fn latin_lookalike(c: char) -> char {
    match c {
        'а' => 'a', 'е' => 'e', 'о' => 'o', 'р' => 'p', 'с' => 'c', 'у' => 'y', 'х' => 'x', 'і' => 'i', 'ј' => 'j', 'ѕ' => 's', 'ԁ' => 'd', 'һ' => 'h',
        'А' => 'A', 'В' => 'B', 'Е' => 'E', 'К' => 'K', 'М' => 'M', 'Н' => 'H', 'О' => 'O', 'Р' => 'P', 'С' => 'C', 'Т' => 'T', 'Х' => 'X', 'І' => 'I', 'Ѕ' => 'S',
        'ο' => 'o', 'ν' => 'v', 'ρ' => 'p', 'ι' => 'i', 'κ' => 'k', 'α' => 'a',
        'Α' => 'A', 'Β' => 'B', 'Ε' => 'E', 'Ζ' => 'Z', 'Η' => 'H', 'Ι' => 'I', 'Κ' => 'K', 'Μ' => 'M', 'Ν' => 'N', 'Ο' => 'O', 'Ρ' => 'P', 'Τ' => 'T', 'Υ' => 'Y', 'Χ' => 'X',
        _ => c,
    }
}

/// Whether every `name` in `src`, but on its `import` and `use` lines, is called: `name(`,
/// not after a `.`. A conservative reading of the text: a string or comment with the word
/// in it keeps the name unmarked.
fn only_called(src: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("import ") || t.starts_with("use ") {
            continue;
        }
        let mut from = 0;
        while let Some(at) = line[from..].find(name).map(|i| i + from) {
            let end = at + name.len();
            from = end;
            let before = line[..at].chars().next_back();
            if before.is_some_and(word) || line[end..].chars().next().is_some_and(word) {
                continue;
            }
            if before == Some('.') || !line[end..].trim_start().starts_with('(') {
                return false;
            }
        }
    }
    true
}

/// The mark on a name a file took only `as` another (Checker::renames_for).
const OLD: char = '\u{1}';

const BUILTINS: &[(&str, &str, &str)] = &[
    ("print", "any...", "null"),
    ("error", "any", "any"),
    ("run_after", "num, fn", "null"),
    ("page_updated", "", "null"),
    ("js_has", "str", "bool"),
    ("cursor_index", "str, int", "int"),
    ("args", "", "list"),
    ("stdin", "", "str | error(str)"),
    ("read_line", "", "str | null | error(str)"),
    ("exit", "int=", "null"),
    // `str()` gave null, which no program means (FEEDBACK45.md, Q-525).
    ("str", "any", "str"),
    ("int", "any", "int | error(str)"),
    ("float", "any", "float | error(str)"),
    ("dec", "any", "dec | error(str)"),
    ("format_number", "num | dec, str | null=, int | map | null=, str | null=", "str"),
    ("len", "str | list | map | html | null", "int"),
    ("type_of", "any", "str"),
    ("upper", "str", "str"),
    ("lower", "str", "str"),
    ("normalize", "str, str | null=", "str"),
    ("trim", "str", "str"),
    ("filled", "any", "any"),
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
    ("range", "int, int=, int=", "list"),
    ("enumerate", "list | null", "list"),
    ("keys", "map | null", "list"),
    ("values", "map | null", "list"),
    ("get", "map | null, any, any=", "any"),
    ("has", "map | null, any", "bool"),
    ("set", "map | list | null, any, any=", "map | list"),
    ("remove", "list | map | null, any", "list | map"),
    ("merge", "map | null, map | null", "map"),
    ("__merge", "map | null...", "map"),
    ("__css_value", "any", "str"),
    ("__concat", "list | null...", "list"),
    ("err", "any=", "error(any)"),
    ("is_err", "any", "bool"),
    ("unwrap", "any", "any"),
    ("raw", "any", "html"),
    ("escape", "any", "str"),
    ("url_encode", "any", "str"),
    ("env", "str, any=", "any"),
    ("platform", "", "map"),
    ("now", "", "int"),
    ("sleep", "num", "null"),
    ("file", "str, map | null=", "response"),
    ("files", "str, str | null=, map | null=", "response"),
    ("redirect", "str, int=", "response"),
    ("__site", "str, any", "any"),
    ("__built", "str, fn", "response"),
    ("classes", "", "list"),
    ("accept_calls", "map", "null"),
    ("__callers", "str, list", "null"),
    ("local_path", "any", "str | null"),
    ("respond", "int, any=, map | null=", "response"),
    ("read_file", "str", "str | error(str)"),
    ("write_file", "str, any", "null | error(str)"),
    ("delete_file", "str", "null | error(str)"),
    ("check_program", "str, map | null=", "list | error(str)"),
    ("random", "", "float"),
    ("random_int", "int, int", "int"),
    ("uuid", "", "str"),
    ("defer", "any", "any"),
    ("abs", "num | dec", "num | dec"),
    ("round", "num | dec, int=, str=", "num | dec"),
    ("floor", "num | dec", "num | dec"),
    ("ceil", "num | dec", "num | dec"),
    ("sqrt", "num", "float"),
    ("pow", "num | dec, num", "num | dec"),
    ("unique", "list | null", "list"),
    ("flatten", "list | null", "list"),
    ("zip", "list | null, list | null", "list"),
    ("take", "list | str | null, int", "list | str"),
    ("drop", "list | str | null, int", "list | str"),
    ("count", "list | null, fn=", "int"),
    ("group_by", "list | null, fn", "map"),
    ("join_on", "list | null, list | null, str, str | null=, str | null=", "list"),
    ("index_of", "list | str, any", "int | null"),
    ("repeat", "str, int", "str"),
    ("markdown", "str, map | null=", "html"),
    ("markdown_page", "str, map | null=", "html"),
    ("highlight", "str, str | null=", "html"),
    ("run_script", "str, map | null=", "any"),
    ("sql_name", "str | list", "str"),
    ("sql_param", "str | list", "str"),
    ("ask", "str, map | null=", "bool"),
    ("notify", "str, map | null=", "null"),
    ("translations", "str | map, map | null=", "null"),
    ("locale_from", "fn", "null"),
    ("locale", "", "str | null"),
    ("with_locale", "str, fn", "any"),
    ("t", "str, map | null=", "str"),
    ("schedule", "str | int, fn, map | null=", "null"),
    ("allow_cross_site", "str", "null"),
    ("site_headers", "str | map, map=", "null"),
    ("tenant", "", "str | null"),
    ("tenant_from", "str | fn", "null"),
    ("with_tenant", "str, fn", "any"),
    ("new_tenant", "str, fn", "any"),
    ("save_upload", "map, str", "str | error(str)"),
    ("read_upload", "map", "str | error(str)"),
    ("js", "str, any...", "any"),
    ("js_new", "str, any...", "any"),
    ("live", "any, any...", "any"),
    ("parallel", "any...", "list"),
    ("every", "num, fn, map | null=", "null"),
    ("stream", "fn, map | null=", "response"),
    ("events", "fn, map | null=", "response"),
    ("listen", "str, fn", "null"),
    ("receive", "str, fn, map | null=", "null | error(str)"),
    ("url", "", "map"),
    ("css", "str", "str"),
    ("__style", "str, bool | null=, str=", "null"),
    ("ready", "str, fn", "null"),
    ("style_with", "str | fn", "null"),
    ("sql", "str", "str"),
    ("navigate", "str", "null"),
    ("waiting", "any", "bool"),
    // What a `routes` group's middleware gave: a response answers (branch route-groups).
    ("__answer", "any, str=", "null"),
    ("publish", "any...", "null"),
    ("watch", "any...", "null"),
    ("unwatched", "fn", "any"),
    ("json.parse", "str", "any"),
    ("json.stringify", "any, any=", "str"),
    ("db.open", "str | fn, map=", "null | error(str)"),
    ("db.connect", "str | fn, map=", "map | error(str)"),
    ("db.system", "str", "null | error(str)"),
    ("later.status", "str", "map | null"),
    ("db.shards", "list", "map | error(str)"),
    ("db.atomic", "list, fn", "any"),
    ("db.exec", "str, any...", "map | error(str)"),
    ("db.exec_many", "str, list", "map | error(str)"),
    ("db.query", "str, any...", "list | error(str)"),
    ("db.one", "str, any...", "map | null | error(str)"),
    ("db.transaction", "map | fn, fn=", "any"),
    ("db.error_kind", "any", "str | null"),
    ("db.sql_name", "str | list", "str"),
    ("idempotent", "map | fn, fn=", "any"),
    ("db.migrate", "str=", "list | error(str)"),
    ("db.tenants", "", "list | error(str)"),
    ("db.tenant_exists", "str", "bool"),
    ("http.get", "str, map | null=, map | null=", "map | error(str)"),
    ("http.delete", "str, map | null=, map | null=", "map | error(str)"),
    ("http.post", "str, any=, map | null=, map | null=", "map | error(str)"),
    ("http.put", "str, any=, map | null=, map | null=", "map | error(str)"),
    ("http.patch", "str, any=, map | null=, map | null=", "map | error(str)"),
    ("http.request", "str, str, any=, map | null=, map | null=", "map | error(str)"),
    ("set_cookie", "str, any=, map | null=", "null"),
    ("cookie", "str", "str | null"),
    ("delete_cookie", "str, map | null=", "null"),
    ("session", "", "map"),
    ("set_session", "map | null, map | null=", "null"),
    ("end_session", "", "null"),
    ("image.info", "str", "map | error(str)"),
    ("image.convert", "str, str, map | null=", "map | error(str)"),
    ("crypto.sha256", "str", "str"),
    ("crypto.hmac", "str, str", "str"),
    ("crypto.token", "int=", "str"),
    ("crypto.hash_password", "str", "str"),
    ("crypto.verify_password", "str, str | null", "bool"),
    ("crypto.equal", "str, str", "bool"),
    ("http.stream", "str, fn, map | null=", "map | error(str)"),
    ("udp.listen", "int, fn, map | null=", "null"),
    ("udp.send", "str, int, any, map | null=", "null | error(str)"),
    ("udp.reply", "map, any", "null | error(str)"),
    ("channel.on", "str, fn, map | null=", "null"),
    ("channel.send", "str, map | str, any, map | null=", "bool"),
    ("channel.broadcast", "str, any, map | null=", "int"),
    ("channel.peers", "str", "list"),
    ("channel.open", "str, fn, map | null=", "map"),
    // src/dates.rs (FEEDBACK2.md, Q-027; branch feedback6-fixes)
    ("date.today", "str | null=", "str"),
    ("date.now", "str | null=", "str"),
    ("date.parse", "str, str | null=", "str | error(str)"),
    ("date.valid", "any, str | null=", "bool"),
    ("date.valid_zone", "any", "bool"),
    ("re.valid", "any, str | null=", "bool"),
    ("date.of", "int, int, int", "str"),
    ("date.parts", "str", "map"),
    ("date.weekday", "str", "int"),
    ("date.is_leap_year", "int | str", "bool"),
    ("date.days_in_month", "int | str, int=", "int"),
    ("date.add_days", "str, int", "str"),
    ("date.add_months", "str, int", "str"),
    ("date.add_years", "str, int", "str"),
    ("date.days_between", "str, str", "int"),
    ("date.months_between", "str, str", "int"),
    ("date.start_of_month", "str", "str"),
    ("date.end_of_month", "str", "str"),
    ("date.to_zone", "str, str | null=", "str"),
    ("date.date_in", "str, str | null=", "str"),
    ("date.at", "str, str | null=, str | map | null=, map | null=", "str"),
    ("date.compare", "str, str", "int"),
    ("date.format", "str, str | map", "str"),
    ("date.ms", "str", "int"),
    ("date.from_ms", "int, str | null=", "str"),
    ("media.rtp", "str, map", "null"),
    ("media.close", "str", "bool"),
    ("media.on", "str, map | null=", "null"),
    ("media.streams", "", "list"),
    // src/money.rs
    ("money.of", "dec | int | str, str, str=", "map | error(str)"),
    ("money.parse", "str, str, str | null=, str=", "map | error(str)"),
    ("money.from_minor", "int, str", "map | error(str)"),
    ("money.to_minor", "map", "int"),
    ("money.add", "map, map", "map | error(str)"),
    ("money.sub", "map, map", "map | error(str)"),
    ("money.mul", "map, dec | int, str=", "map | error(str)"),
    ("money.neg", "map", "map | error(str)"),
    ("money.compare", "map, map", "int"),
    ("money.zero", "str", "map | error(str)"),
    ("money.is_zero", "map", "bool"),
    ("money.allocate", "map, list | int", "list"),
    ("money.round", "map, str=", "map | error(str)"),
    ("money.minor_units", "str", "int | error(str)"),
    ("money.format", "map, str | null=", "str"),
    // src/numeric.rs
    ("math.sqrt", "num", "float"),
    ("math.cbrt", "num", "float"),
    ("math.exp", "num", "float"),
    ("math.exp2", "num", "float"),
    ("math.expm1", "num", "float"),
    ("math.log", "num, num=", "float"),
    ("math.log2", "num", "float"),
    ("math.log10", "num", "float"),
    ("math.log1p", "num", "float"),
    ("math.pow", "num, num", "float"),
    ("math.sin", "num", "float"),
    ("math.cos", "num", "float"),
    ("math.tan", "num", "float"),
    ("math.asin", "num", "float"),
    ("math.acos", "num", "float"),
    ("math.atan", "num", "float"),
    ("math.atan2", "num, num", "float"),
    ("math.sinh", "num", "float"),
    ("math.cosh", "num", "float"),
    ("math.tanh", "num", "float"),
    ("math.asinh", "num", "float"),
    ("math.acosh", "num", "float"),
    ("math.atanh", "num", "float"),
    ("math.degrees", "num", "float"),
    ("math.radians", "num", "float"),
    ("math.hypot", "num...", "float"),
    ("math.dist", "list, list", "float"),
    ("math.floor", "num", "int"),
    ("math.ceil", "num", "int"),
    ("math.trunc", "num", "int"),
    ("math.fabs", "num", "float"),
    ("math.copysign", "num, num", "float"),
    ("math.fmod", "num, num", "float"),
    ("math.remainder", "num, num", "float"),
    ("math.modf", "num", "list"),
    ("math.frexp", "num", "list"),
    ("math.ldexp", "num, int", "float"),
    ("math.factorial", "int", "int"),
    ("math.gcd", "int | list...", "int"),
    ("math.lcm", "int | list...", "int"),
    ("bit.and", "int, int", "int"),
    ("bit.or", "int, int", "int"),
    ("bit.xor", "int, int", "int"),
    ("bit.not", "int", "int"),
    ("bit.shl", "int, int", "int"),
    ("bit.shr", "int, int", "int"),
    ("math.comb", "int, int", "int"),
    ("math.perm", "int, int | null=", "int"),
    ("math.isqrt", "int", "int"),
    ("math.isclose", "num, num, num=, num=", "bool"),
    ("math.isfinite", "num", "bool"),
    ("math.isinf", "num", "bool"),
    ("math.isnan", "num", "bool"),
    ("math.prod", "list | null, num=", "num"),
    ("math.fsum", "list | null", "float"),
    ("math.sumprod", "list, list", "num"),
    ("math.erf", "num", "float"),
    ("math.erfc", "num", "float"),
    ("math.gamma", "num", "float"),
    ("math.lgamma", "num", "float"),
    ("stats.mean", "list", "float"),
    ("stats.fmean", "list, list | null=", "float"),
    ("stats.geometric_mean", "list", "float"),
    ("stats.harmonic_mean", "list", "float"),
    ("stats.median", "list", "num"),
    ("stats.median_low", "list", "num"),
    ("stats.median_high", "list", "num"),
    ("stats.mode", "list", "any"),
    ("stats.multimode", "list | null", "list"),
    ("stats.pvariance", "list, num | null=", "float"),
    ("stats.variance", "list, num | null=", "float"),
    ("stats.pstdev", "list, num | null=", "float"),
    ("stats.stdev", "list, num | null=", "float"),
    ("stats.quantiles", "list, int=, str=", "list"),
    ("stats.percentile", "list, num | list", "any"),
    ("stats.covariance", "list, list", "float"),
    ("stats.correlation", "list, list, str=", "float"),
    ("stats.linear_regression", "list, list, bool=", "map"),
    ("stats.describe", "list | null", "map"),
    ("stats.histogram", "list | null, int | list=", "map"),
    ("stats.zscore", "list | null", "list"),
    ("stats.normalize", "list | null", "list"),
    ("stats.rank", "list | null", "list"),
    ("stats.normal_pdf", "num, num=, num=", "float"),
    ("stats.normal_cdf", "num, num=, num=", "float"),
    ("stats.normal_inv", "num, num=, num=", "float"),
    ("vec.add", "list | num, list | num", "any"),
    ("vec.sub", "list | num, list | num", "any"),
    ("vec.mul", "list | num, list | num", "any"),
    ("vec.div", "list | num, list | num", "any"),
    ("vec.mod", "list | num, list | num", "any"),
    ("vec.pow", "list, num", "list"),
    ("vec.dot", "list, list", "num"),
    ("vec.norm", "list, num=", "float"),
    ("vec.cumsum", "list | null", "list"),
    ("vec.cumprod", "list | null", "list"),
    ("vec.diff", "list | null", "list"),
    ("vec.argmin", "list | null", "int | null"),
    ("vec.argmax", "list | null", "int | null"),
    ("re.test", "str, str, str | null=", "bool"),
    ("re.find", "str, str, str | null=", "map | null"),
    ("re.find_all", "str, str, str | null=", "list"),
    ("re.replace", "str, str, str | fn, str | null=", "str"),
    ("re.split", "str, str, str | null=", "list"),
    ("vec.argsort", "list | null, bool=", "list"),
    ("vec.clip", "list, num | null, num | null=", "list"),
    ("vec.abs", "list", "list"),
    ("vec.sqrt", "list", "list"),
    ("vec.exp", "list", "list"),
    ("vec.log", "list", "list"),
    ("vec.round", "list, int=", "list"),
    ("vec.linspace", "num, num, int=, bool=", "list"),
    ("vec.arange", "num, num=, num=", "list"),
    ("vec.zeros", "int", "list"),
    ("vec.ones", "int", "list"),
    ("vec.full", "int, any", "list"),
    ("vec.rolling_mean", "list, int", "list"),
    ("vec.rolling_sum", "list, int", "list"),
    ("vec.matmul", "list, list", "list"),
    ("vec.transpose", "list | null", "list"),
    ("vec.column", "list | null, str | int", "list"),
];

/// How many arguments the builtin `name` takes at most, as the table above has it: None for one
/// that takes any number (`print`) or is not in it. A call holds to it as it runs, in the browser
/// too, where the rest were dropped (FEEDBACK55.md, Q-786; branch feedback55).
pub fn most_args(name: &str) -> Option<usize> {
    static MOST: std::sync::OnceLock<HashMap<&'static str, usize>> = std::sync::OnceLock::new();
    MOST.get_or_init(|| {
        BUILTINS
            .iter()
            .filter(|(_, params, _)| !params.contains("..."))
            .map(|(name, params, _)| (*name, params.split(',').filter(|p| !p.trim().is_empty()).count()))
            .collect()
    })
    .get(name)
    .copied()
}

/// What a call of the builtin `name` with `got` arguments, more than it takes, is told, here and
/// in the browser: how many it takes, and its call as the docs write it, `sort(xs)`. `sort` given
/// a function to compare with, as in JavaScript, is told how to sort by a key.
pub fn too_many_args(name: &str, got: usize) -> String {
    let most = most_args(name).unwrap_or(0);
    let hint = if name == "sort" { "; to sort by a key, `sort_by(xs, (x) => key)`, and `reverse` of it for the other way" } else { "" };
    format!("`{}` takes {} argument{}, got {}: {}{}", name, most, if most == 1 { "" } else { "s" }, got, call_shape(name), hint)
}

/// A call of the builtin `name` as its docs write it: `sort(xs)`, `split(s, sep)`.
pub fn call_shape(name: &str) -> String {
    let names: Vec<String> = match crate::builtin_docs::DOCS.iter().find(|(n, _, _)| *n == name) {
        Some((_, p, _)) => p.split(',').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect(),
        None => (1..=most_args(name).unwrap_or(0)).map(|i| format!("arg{}", i)).collect(),
    };
    format!("{}({})", name, names.join(", "))
}

/// Modules the browser has too (src/client/runtime.js).
const BROWSER_MODULES: &[&str] = &["json", "math", "bit", "stats", "vec", "date", "money", "re"];

pub const MODULES: &[&str] = &["json", "db", "http", "udp", "channel", "media", "crypto", "image", "later", "math", "bit", "stats", "vec", "date", "money", "re"];

/// What the modules hold that is not a function.
const MODULE_VALUES: &[&str] = &["math.pi", "math.e", "math.tau", "math.inf", "math.nan"];

/// Builtins the browser has (see client.rs).
const BROWSER_ONLY_SERVER: &[&str] =
    &["env", "sleep", "file", "redirect", "respond", "read_file", "write_file", "delete_file", "check_program", "publish", "watch", "unwatched", "crypto.sha256",
      "crypto.hmac", "crypto.token", "crypto.hash_password", "crypto.verify_password", "crypto.equal", "set_cookie",
      "delete_cookie", "session", "cookie", "set_session", "end_session", "translations", "locale_from", "with_locale",
      "schedule", "stream", "events", "image.info", "image.convert", "later.status", "args", "stdin", "read_line", "exit", "allow_cross_site", "site_headers", "tenant_from", "with_tenant", "save_upload", "read_upload", "platform", "markdown_page", "files", "classes", "accept_calls"];

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
    /// The annotation with lists' elements and maps' fields.
    full: T,
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
    /// A `.qwer` file's function: the file it is in.
    native: Option<Arc<str>>,
}

impl Sig {
    fn builtin(name: &str, params: &str, ret: &str) -> Sig {
        let mut ps = Vec::new();
        let mut variadic = false;
        // The parameters' names, as the docs give them (FEEDBACK45.md, Q-525; branch feedback40-43).
        let names: Vec<&str> = crate::builtin_docs::DOCS.iter().find(|(n, _, _)| *n == name).map_or(Vec::new(), |(_, p, _)| p.split(',').map(str::trim).collect());
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
            let t = parse_t(p);
            let pname = names.get(i).filter(|n| !n.is_empty()).map_or_else(|| format!("arg{}", i + 1), |n| n.trim_end_matches("...").to_string());
            ps.push(PSig { name: pname, ty: t.bits, full: t, optional: optional || variadic, annotated: true });
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
            native: None,
        }
    }

    fn of_def(def: &Arc<FnDef>) -> Sig {
        Sig {
            name: def.name.clone(),
            params: def
                .params
                .iter()
                .map(|p| PSig {
                    name: p.name.to_string(),
                    ty: p.ty.as_ref().map_or(ANY, ty_bits),
                    full: p.ty.as_ref().map_or(T::ANY, ty_t),
                    optional: p.default.is_some(),
                    annotated: p.ty.is_some(),
                })
                .collect(),
            variadic: false,
            ret: def.ret.as_ref().map(ty_t),
            err: def.err.as_ref().map(ty_bits),
            effects: def.effects.as_ref().map(|e| effect_mask(e)),
            server: def.server,
            component: def.component,
            builtin: false,
            def: Some(def.clone()),
            native: None,
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
        if e == ALL_EFFECTS && self.native.is_some() {
            // Its file does not say what it does (`//! effects:`).
            s.push_str("  (effects: not declared)");
        } else if e != 0 {
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
            let mut s = p.name.to_string();
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
    /// Shown by an editor, faintly; not by `quillo check` (branch
    /// class-tools).
    Hint,
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
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Hint => "hint",
        };
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

/// A parameter, a `let`, a state or a bound name, and the lines it can be
/// used on: from `line` to `end`, the last its scope has code on.
#[derive(Debug, Clone)]
pub struct Local {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub end: u32,
    pub detail: String,
    /// Written as a statement of its block (`let`, `state`), so it lasts as
    /// long as that block; else its scope starts on `line`.
    pub statement: bool,
}

#[derive(Default, Debug)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    /// Where a file could not be parsed for syntax this Quillo does not
    /// know (file, line): perhaps it is for a newer one. Errors that name
    /// the rule broken (a keyword as a name, say) are not here.
    pub syntax_errors: Vec<(String, u32)>,
    /// The classes the program's style sheets have rules for (name, file,
    /// line): where an editor goes from `<span.chip>`, and what it offers
    /// after `<span.` (branch class-links).
    pub classes: Vec<(String, String, u32)>,
    /// Names that live in a part of a file, for completion (branch
    /// signature-help).
    pub locals: Vec<Local>,
    /// The program's `.qwe` files, where an editor looks for the classes
    /// markup writes (branch class-tools).
    pub files: Vec<String>,
    /// What files use and do not take, for `quillo fix` (src/modules.rs).
    pub imports: Vec<Import>,
    /// Browser libraries taken with a plain `use` (file, line).
    pub respell: Vec<crate::modules::Respell>,
    /// What a file took and calls something of its own, or that two files define: the
    /// file, and the symbol under the name it has there (src/modules.rs).
    pub taken: Vec<(String, Symbol)>,
}

/// A name a file uses from another, which it does not `use` (branch feedback19-fixes).
#[derive(Debug, Clone)]
pub struct Import {
    pub file: String,
    pub name: String,
    /// Its path, as the program writes it.
    pub from: String,
    /// For a browser library, the file and line of the `use` to say again.
    pub script: Option<(String, u32)>,
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

/// How strict a check is.
#[derive(Clone, Copy, Default, Debug)]
pub struct Options {
    /// Also warn where a value may be null and null does not work: an
    /// argument `str | null` for a `str` parameter, `count + 1` with
    /// `count: int | null`. Checks such as `x != null` narrow the type.
    pub strict_null: bool,
    /// Read only files inside the program's folder: `use`, `mount` and
    /// `service` paths made of plain names, as `read_file` takes them. For
    /// code that comes from a program's users (`check_program`).
    pub confined: bool,
    /// Also warn of classes the program styles and never writes (`quillo
    /// check`; FEEDBACK8.md, Q-065; branch feedback8-fixes).
    pub css: bool,
    /// Warn of classes markup writes that no style sheet has a rule for
    /// (`quillo check`, and `quillo lsp`; branch class-tools).
    pub classes: bool,
}

/// Why a confined check reads no other file (FEEDBACK55.md, Q-703; branch feedback55).
pub(crate) const CONFINED_READS: &str = "only Quillo files are read (.qwe, and a plugin's .qwer or .rs for `use`), none whose name or folder starts with a dot";

/// Whether a confined check reads `path`, at `full` under `root`, by its name: a `.qwe` file (or,
/// for a `use`, a plugin's `.qwer` or `.rs`, of which only its functions' signatures are read),
/// with no part of its path starting with a dot, and what a link leads to judged the same; that
/// it stays in the folder is `stays_in`'s to say. `use ".env"` read the settings as Quillo and
/// said their values back as unknown names, and `use "site/.git/config"` the repository's
/// (FEEDBACK55.md, Q-703; branch feedback55).
pub(crate) fn confined_reads(root: &Path, path: &str, full: &Path, rust: bool) -> bool {
    let quillo = |rel: &Path| {
        let parts: Vec<String> = rel.components().filter(|c| !matches!(c, std::path::Component::CurDir)).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        // Named as Quillo names them, exactly: what is let in is not widened by a case
        // (FEEDBACK55.md, Q-703; branch feedback55).
        let last = parts.last().map_or("", |l| l.as_str());
        !parts.is_empty() && !parts.iter().any(|p| p.starts_with('.')) && (last.ends_with(".qwe") || (rust && crate::native::is_rust(last)))
    };
    if !quillo(Path::new(path)) {
        return false;
    }
    match crate::builtins::leads_to(root, full) {
        Some(Some(real)) => root.canonicalize().ok().and_then(|r| real.strip_prefix(r).ok().map(Path::to_path_buf)).is_some_and(|rel| quillo(&rel)),
        Some(None) => true,
        // Out of the folder, into one `--allow-path` named: by the name it has there
        // (FEEDBACK55.md, Q-703; branch feedback55).
        None => full.canonicalize().map_or(true, |real| real.file_name().is_some_and(|n| quillo(Path::new(n)))),
    }
}

/// How many programs deep `mount` and `service` may lead, and files deep
/// `use` may: past that the check stops there with an error, instead of
/// running out of stack.
const MAX_PROGRAMS: u32 = 16;
const MAX_USES: u32 = 64;

/// Checks a program file and the files it `use`s.
pub fn check_file(path: &Path) -> Analysis {
    check_file_with(path, HashMap::new())
}

/// `check_file`, with some files' text given instead of read: how the
/// program would check if they said that (a tool about to write them).
pub fn check_file_with(path: &Path, overrides: HashMap<PathBuf, String>) -> Analysis {
    check_file_opts(path, overrides, Options::default())
}

/// `check_file_with`, with options.
pub fn check_file_opts(path: &Path, overrides: HashMap<PathBuf, String>, opts: Options) -> Analysis {
    let main = overrides.get(path).cloned().map_or_else(|| crate::error::read_source(path), Ok);
    match main {
        Ok(src) => {
            let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            check_source_with(&src, &path.display().to_string(), base, overrides, opts)
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
    check_source_opts(src, file, base_dir, Options::default())
}

/// `check_source`, with options.
pub fn check_source_opts(src: &str, file: &str, base_dir: &Path, opts: Options) -> Analysis {
    check_source_with(src, file, base_dir, HashMap::new(), opts)
}

fn check_source_with(src: &str, file: &str, base_dir: &Path, overrides: HashMap<PathBuf, String>, opts: Options) -> Analysis {
    if DEPTH.with(|d| d.get()) >= MAX_PROGRAMS {
        let mut a = Analysis::default();
        let message = format!("programs mount or call each other more than {} deep", MAX_PROGRAMS);
        a.diagnostics.push(Diagnostic { file: file.to_string(), line: 0, severity: Severity::Error, message });
        return a;
    }
    let outermost = DEPTH.with(|d| {
        d.set(d.get() + 1);
        d.get() == 1
    });
    if outermost {
        TABLES.with(|t| *t.borrow_mut() = Tables::default());
    }
    // A mounted program's types are its own.
    let outer_records = RECORDS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let a = check_source_in(src, file, base_dir, overrides, opts);
    RECORDS.with(|r| *r.borrow_mut() = outer_records);
    DEPTH.with(|d| d.set(d.get() - 1));
    a
}

fn check_source_in(src: &str, file: &str, base_dir: &Path, overrides: HashMap<PathBuf, String>, opts: Options) -> Analysis {
    let mut c = Checker::new(file);
    c.strict_null = opts.strict_null;
    c.confined = opts.confined;
    c.root = base_dir.to_path_buf();
    c.overrides = overrides;
    let main: Arc<str> = Arc::from(file);
    let mut parser = Parser::new(src);
    let parsed = parser.parse_program();
    let parse_failed = parsed.is_err();
    c.plain_dollars.extend(parser.plain_dollars().into_iter().map(|(text, n)| (file.to_string(), text, n)));
    // `use` paths count from the folder of the file a main file that is a link leads to; the
    // rest from the folder given (FEEDBACK55.md, Q-817; branch feedback55).
    let src_dir = crate::modules::source_dir_or(Path::new(file), base_dir);
    match parsed.map(Arc::new).and_then(|stmts| c.link(stmts, src, file, &src_dir)) {
        Ok(stmts) => {
            let mut seen = HashSet::new();
            if let Ok(p) = Path::new(file).canonicalize() {
                seen.insert(p);
            }
            let mut order = Vec::new();
            c.expand(stmts, main, &src_dir, &mut seen, &mut order);
            c.out.classes = class_rules(&order, base_dir);
            for it in &order {
                if !c.out.files.iter().any(|f| **f == *it.file) {
                    c.out.files.push(it.file.to_string());
                }
            }
            if opts.css {
                for (file, line, msg) in unused_classes(&order, base_dir, &c.unparsed_texts) {
                    c.diag_at(file, line, Severity::Warning, msg);
                }
            }
            if opts.css || opts.classes {
                for (file, line, sev, msg) in unstyled_classes(&order, base_dir, &c.out.classes) {
                    if sev == Severity::Warning || opts.classes {
                        c.diag_at(file, line, sev, msg);
                    }
                }
            }
            c.route_reach(&order);
            c.run(order);
            c.unused_said();
            c.state_details();
            c.as_written();
        }
        Err(e) => {
            if crate::error::may_be_newer_syntax(&e.msg) {
                c.out.syntax_errors.push((main.to_string(), e.line));
            }
            // The syntax errors after the first, each from the next statement on, so one run
            // says them all (FEEDBACK45.md, Q-522; branch feedback40-43).
            let more = if parse_failed { crate::parser::more_errors(src, e.line) } else { Vec::new() };
            c.diag_at(main.clone(), e.line, Severity::Error, e.msg);
            for m in more {
                c.diag_at(main.clone(), m.line, Severity::Error, m.msg);
            }
        }
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

fn var_detail(name: &str, v: &Var) -> String {
    match &v.sig {
        Some(s) => s.text(),
        None => {
            let kw = match v.kind {
                Kind::State => "state ",
                Kind::Param => "param ",
                _ => "let ",
            };
            if MODULES.contains(&name) && v.kind == Kind::Global && &*v.file == "<builtin>" {
                format!("module {}", name)
            } else {
                format!("{}{}: {}", kw, name, t_text(v.t))
            }
        }
    }
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

/// An unknown name in a function: where, the error said if nothing defines it, and how many
/// functions and scopes deep it was.
struct Early {
    name: String,
    file: Arc<str>,
    line: u32,
    msg: String,
    fns: usize,
    /// The scopes around it, by number: a `let` in one of them defines it.
    chain: Vec<u64>,
    /// Read in a function inside the one with the `let`: a closure, which may well run after
    /// the `let` (found in review).
    nested: bool,
}

/// A `let` of a block that has ended.
struct Gone {
    name: String,
    file: Arc<str>,
    line: u32,
    kind: &'static str,
    fns: usize,
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
    /// A component's `state` initializer, which the server runs as it renders.
    state_init: bool,
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
    /// Names an import line could not take, said there and not again where used (Q-523).
    failed_imports: HashSet<(PathBuf, String)>,
    /// The files that take a file whole that could not be read (FEEDBACK55.md, Q-819).
    failed_whole: HashSet<PathBuf>,
    /// Top-level functions and `let` lambdas, by name (Q-514).
    bodies: HashMap<String, Arc<FnDef>>,
    /// The route's path parameters with no type, while its body is checked (Q-513).
    text_params: Vec<String>,
    /// How many scopes deep the route's own is, so a local of the same name is not taken
    /// for its parameter (found in review).
    text_params_depth: usize,
    /// The route's parameters that are text the visitor writes, `:name` and `*path` (Q-657).
    path_params: Vec<String>,
    /// `${name}` written in plain strings, by file and line (FEEDBACK45.md, Q-510).
    plain_dollars: HashSet<(String, String, String)>,
    /// The functions being read in their own `let`.
    letting: Vec<String>,
    /// A component's `let`s whose value comes from the page (`js(...)`), by component: shown in
    /// markup, they make the browser remake the component (FEEDBACK35.md, Q-431; branch
    /// feedback27-35-fixes).
    page_lets: Vec<(String, String)>,
    /// The names `site` declared (FEEDBACK33.md, Q-412; branch feedback27-35-fixes).
    sites: HashSet<String>,
    /// Files' text to use instead of what is on disk.
    overrides: HashMap<PathBuf, String>,
    globals: HashMap<String, Var>,
    scopes: Vec<HashMap<String, Var>>,
    fns: Vec<FnCtx>,
    /// > 0 while inferring a type: no diagnostics, no references.
    quiet: u32,
    /// Checking top-level code as it runs (not a function or route body).
    sequential: bool,
    assigned: HashSet<String>,
    /// A `let` only `+=` and `-=` change (by its file, line and name): its first value's type,
    /// which a wrong `-=` is held to (FEEDBACK45.md, Q-525; branch feedback40-43).
    first_t: HashMap<(Arc<str>, u32, String), T>,
    /// Of those, the ones assigned where nothing local has that name.
    assigned_globals: HashSet<String>,
    ret_cache: HashMap<usize, T>,
    in_progress: HashSet<usize>,
    effect_cache: HashMap<usize, u8>,
    /// Functions of `.qwer` files the program uses, with their files.
    natives: Vec<(Arc<str>, crate::native::Export)>,
    /// Names browser libraries give (`client "...js" use Chart`): each with its
    /// script, and the file and line of its `use` (branch js-libraries).
    js_libs: Vec<(String, String, Arc<str>, u32)>,
    /// What each file sees (src/modules.rs).
    links: crate::modules::Links,
    /// Each file's key, as finding it means asking the file system.
    keys: HashMap<Arc<str>, PathBuf>,
    /// Names every file sees: types, and services.
    everywhere: HashSet<String>,
    /// Names already reported as not taken, by the file using them.
    told: HashSet<(Arc<str>, String)>,
    /// Unknown names inside a function, said when it ends unless a `let` further down it
    /// defines them, which makes them names used before they are defined (FEEDBACK45.md, Q-524).
    early: Vec<Early>,
    /// Elements around this one with `@input` or `@change` (Checker::shown_only).
    input_above: usize,
    /// Each scope's number, and the last given (Checker::push).
    scope_ids: Vec<u64>,
    scope_serial: u64,
    /// The `let`s of blocks that have ended, for a read after the block (Q-524).
    gone: Vec<Gone>,
    /// What the next block is the body of: `if`, `for` or `while`.
    block_kind: Option<&'static str>,
    /// Unused variables, said at the end: a read after its block, or before its `let`,
    /// is a use (Q-524).
    unused: Vec<(Arc<str>, u32, String)>,
    /// Browser libraries' names: the file and line of their `use`.
    scripts: HashMap<String, (Arc<str>, u32)>,
    /// Each `state` (by its file, line and name): the types it takes (its
    /// first value's, and what is assigned to it), and the references to it,
    /// whose hover text says so once the program is checked.
    state_types: HashMap<(Arc<str>, u32, String), (T, Vec<usize>)>,
    /// Checking the `live(...)` that sets a component's state.
    live_ok: bool,
    /// The services a program calls (`service billing = "billing.qwe"`): the
    /// server fns each offers, by name.
    services: HashMap<String, HashMap<String, Arc<Sig>>>,
    /// Services whose file is not here: their calls are taken as they are written.
    services_elsewhere: HashSet<String>,
    /// The parameters' types of the lambda about to be checked, from the
    /// list a builtin gives it: `xs | map((x) => ...)`.
    hint: Option<Vec<T>>,
    /// Warn where a value may be null and null does not work (`--strict-null`).
    strict_null: bool,
    /// See `Options::confined`.
    confined: bool,
    /// The folder of the file checked first: what a confined check stays in.
    root: PathBuf,
    /// Files `use`d inside one another, and expressions inside one another,
    /// at the point being checked.
    uses: u32,
    depth: u32,
    /// The program's main file, whose folder names the others in messages.
    main: Arc<str>,
    file: Arc<str>,
    line: u32,
    out: Analysis,
    reported: HashSet<(Arc<str>, u32, String)>,
    /// Where `use`d files could not be parsed: reported first, as they
    /// cause the errors after them.
    parse_errors: HashSet<(Arc<str>, u32)>,
    /// The names those files seem to define (read from their text): not
    /// reported as unknown, since the file that defines them is broken.
    unparsed_names: HashSet<String>,
    /// Those files' text: the classes it holds are not reported as written
    /// by nothing, since the file that writes them is broken (FEEDBACK13.md,
    /// Q-092; branch feedback13-fixes).
    unparsed_texts: Vec<String>,
    /// The program's functions that give only SQL-safe text (each result
    /// `sql_name(...)`, say): calling one is as safe as `sql_name` itself.
    sql_name_fns: HashSet<String>,
    /// The program has `tenant_from`: a global written by a request is every tenant's.
    tenants: bool,
    /// What is known of variables' text for the SQL warning, innermost
    /// last: the depth of the scope each was made in, its name, and what
    /// it holds (see `SqlVar`).
    sql_vars: Vec<(usize, String, SqlVar)>,
    /// What the variable `define` makes next holds (set by `let`).
    next_sql: SqlVar,
    /// Variables holding a URL whose host is written in the program (see
    /// `url_fixed`), as `sql_vars` is kept, and the next one's.
    url_vars: Vec<(usize, String, bool)>,
    next_url: bool,
    /// The ticks (`true`) and jobs the top level adds, by name, with where and on what path
    /// (FEEDBACK55.md, Q-803; branch feedback55).
    job_names: HashMap<(bool, String), Vec<JobSeen>>,
    /// The branches (of an `if` or a `match`) and loops the code being checked is in, outermost
    /// first, and the last branch point's number (see `jobs_as_started`; found in review).
    path: Vec<Step>,
    branches: u32,
}

/// A tick or job the top level adds (see `Checker::job_names`): the call (its arguments' place in
/// the program, which a second look at it finds again), and on what path.
struct JobSeen {
    call: usize,
    path: Vec<Step>,
}

/// Where code is, for whether two of its calls can both run (see `Checker::path`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    /// Arm `.1` of branch point `.0`.
    Arm(u32, u32),
    Loop,
    /// What may run or not: the right side of `&&`, `||` or `??`, a helper's body below its first
    /// branch (found in the second review).
    Maybe,
}

/// Whether code on path `a` and code on path `b` can both run in one go: unless they part in two
/// arms of one branch point.
fn both_run(a: &[Step], b: &[Step]) -> bool {
    match a.iter().zip(b).find(|(x, y)| x != y) {
        Some((Step::Arm(i, _), Step::Arm(j, _))) => i != j,
        _ => true,
    }
}

/// What a variable holds, as far as SQL goes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SqlVar {
    Other,
    /// Text that cannot carry SQL of its own: written in the program, or
    /// made with `sql_name`.
    Safe,
    /// Text put together from values (`${x}` or `+ x`).
    Built,
    /// A connection from `db.connect`.
    Conn,
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
            page_lets: Vec::new(),
            sites: HashSet::new(),
            overrides: HashMap::new(),
            globals,
            scopes: Vec::new(),
            fns: Vec::new(),
            quiet: 0,
            sequential: true,
            assigned: HashSet::new(),
            first_t: HashMap::new(),
            assigned_globals: HashSet::new(),
            ret_cache: HashMap::new(),
            in_progress: HashSet::new(),
            effect_cache: HashMap::new(),
            natives: Vec::new(),
            js_libs: Vec::new(),
            links: Default::default(),
            keys: HashMap::new(),
            everywhere: HashSet::new(),
            told: HashSet::new(),
            early: Vec::new(),
            input_above: 0,
            scope_ids: Vec::new(),
            scope_serial: 0,
            gone: Vec::new(),
            block_kind: None,
            unused: Vec::new(),
            scripts: HashMap::new(),
            state_types: HashMap::new(),
            live_ok: false,
            services: HashMap::new(),
            services_elsewhere: HashSet::new(),
            hint: None,
            strict_null: false,
            letting: Vec::new(),
            plain_dollars: HashSet::new(),
            text_params: Vec::new(),
            text_params_depth: 0,
            path_params: Vec::new(),
            bodies: HashMap::new(),
            failed_imports: HashSet::new(),
            failed_whole: HashSet::new(),
            confined: false,
            root: PathBuf::new(),
            uses: 0,
            depth: 0,
            main: Arc::from(file),
            file: Arc::from(file),
            line: 0,
            out: Analysis::default(),
            reported: HashSet::new(),
            parse_errors: HashSet::new(),
            unparsed_names: HashSet::new(),
            unparsed_texts: Vec::new(),
            sql_name_fns: HashSet::new(),
            tenants: false,
            sql_vars: Vec::new(),
            next_sql: SqlVar::Other,
            url_vars: Vec::new(),
            next_url: false,
            job_names: HashMap::new(),
            path: Vec::new(),
            branches: 0,
        }
    }

    /// `f` checked in arm `arm` of branch point `at`, or in a loop's body.
    fn on_path<R>(&mut self, step: Step, f: impl FnOnce(&mut Self) -> R) -> R {
        self.path.push(step);
        let r = f(self);
        self.path.pop();
        r
    }

    fn branch_point(&mut self) -> u32 {
        self.branches += 1;
        self.branches
    }

    fn finish(mut self) -> Analysis {
        // A file that could not be parsed first: what follows may be its doing.
        let parse = &self.parse_errors;
        let key = |d: &Diagnostic| !parse.iter().any(|(f, l)| **f == *d.file && *l == d.line);
        self.out.diagnostics.sort_by(|a, b| {
            (key(a), a.file != *self.file, &a.file, a.line).cmp(&(key(b), b.file != *self.file, &b.file, b.line))
        });
        self.out
    }

    fn diag_at(&mut self, file: Arc<str>, line: u32, severity: Severity, msg: impl Into<String>) {
        if self.quiet > 0 {
            return;
        }
        // A private name as it was written (src/privacy.rs; branch public-private), and an
        // old name without its mark.
        let msg = crate::privacy::demangle(&msg.into()).replace(OLD, "");
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

    fn options(&self) -> Options {
        Options { strict_null: self.strict_null, confined: self.confined, css: false, classes: false }
    }

    /// The file a `use`, `mount` or `service` names; when confined, none
    /// for a path that could lead outside the program's folder.
    fn file_at(&mut self, base: &Path, path: &str, file: &Arc<str>, line: u32, rust: bool) -> Option<PathBuf> {
        let plain = !path.is_empty() && Path::new(path).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
        // Nor out of it through a link, to read a file elsewhere (FEEDBACK41.md, Q-475; branch
        // feedback40-43).
        if self.confined && (!plain || !crate::builtins::stays_in(&self.root, &base.join(path))) {
            self.diag_at(file.clone(), line, Severity::Error, format!("path must stay inside the program's folder: {}", path));
            return None;
        }
        // Nor a file that is not Quillo's, or hidden, which was read and said back
        // (FEEDBACK55.md, Q-703; branch feedback55).
        if self.confined && !confined_reads(&self.root, path, &base.join(path), rust) {
            self.diag_at(file.clone(), line, Severity::Error, format!("{}: {}", CONFINED_READS, path));
            return None;
        }
        Some(base.join(path))
    }

    /// Works out what each file sees (src/modules.rs), and gives the main file with its
    /// names as they are called in the program.
    fn link(&mut self, stmts: Arc<Vec<Stmt>>, src: &str, file: &str, base: &Path) -> Result<Arc<Vec<Stmt>>, crate::error::QuilloError> {
        let (confined, overrides, root) = (self.confined, &self.overrides, self.root.clone());
        // The `${name}` each file's plain strings hold (Q-510), by the file.
        let mut dollars: Vec<(String, String, String)> = Vec::new();
        self.links = crate::modules::link(Path::new(file), stmts.clone(), base, &mut |path, full| {
            // As `file_at` judges it (FEEDBACK55.md, Q-703; branch feedback55).
            if confined && (!Path::new(path).components().all(|c| matches!(c, std::path::Component::Normal(_))) || !crate::builtins::stays_in(&root, full) || !confined_reads(&root, path, full, false)) {
                return None;
            }
            let text = overrides.get(full).cloned().map_or_else(|| crate::error::read_source(full), Ok).ok()?;
            let mut p = Parser::new(&text);
            let parsed = p.parse_program().ok().map(Arc::new);
            let shown = full.display().to_string();
            dollars.extend(p.plain_dollars().into_iter().map(|(t, n)| (shown.clone(), t, n)));
            parsed
        });
        self.plain_dollars.extend(dollars);
        // Said at the import line, by the path the program wrote, and once: the name it could
        // not take is not said again where it is used (FEEDBACK45.md, Q-523).
        let main_key = Path::new(file).canonicalize().ok();
        // A used file by the path the program writes, as every other message names it, not the
        // machine's (FEEDBACK55.md, Q-822; branch feedback55).
        let shown_file = |links: &crate::modules::Links, f: &Path| if f.canonicalize().ok() == main_key { file.to_string() } else { base.join(links.written(f)).display().to_string() };
        for p in std::mem::take(&mut self.links.problems) {
            let shown = shown_file(&self.links, &p.file);
            if let Some(n) = &p.name {
                self.failed_imports.insert((p.file.clone(), n.clone()));
            }
            self.diag_at(Arc::from(shown), p.line, Severity::Error, p.message);
        }
        for p in std::mem::take(&mut self.links.repeats) {
            let shown = shown_file(&self.links, &p.file);
            self.diag_at(Arc::from(shown), p.line, Severity::Warning, p.message);
        }
        match self.renames_for(&crate::modules::key(Path::new(file)), src) {
            Some(names) => reparsed(src, &names).map(Arc::new),
            None => Ok(stmts),
        }
    }

    /// How the checker renames a file's names: as linking does, and each name the file took
    /// only `as` another marked with OLD, so its uses can be told from the new name's
    /// (FEEDBACK45.md, Q-523). A builtin's name stays the builtin's.
    /// Only a name the file calls and never binds is marked: a parameter, prop, `let` or key
    /// of that name is the file's own, and a local of the other file's name is what the alias
    /// finds as the program runs, which the checker must see (found in review).
    fn renames_for(&self, key: &Path, src: &str) -> Option<HashMap<String, String>> {
        let mut names = self.links.renames(key).cloned().unwrap_or_default();
        for n in self.links.old_names.get(key).into_iter().flat_map(|o| o.keys()) {
            if !BUILTINS.iter().any(|(b, _, _)| b == n) && only_called(src, n) {
                names.insert(n.clone(), format!("{}{}", n, OLD));
            }
        }
        (!names.is_empty()).then_some(names)
    }

    /// Lists statements in the order the top level runs them, following `use`.
    fn expand(&mut self, stmts: Arc<Vec<Stmt>>, file: Arc<str>, base: &Path, seen: &mut HashSet<PathBuf>, order: &mut Vec<Item>) {
        for (i, s) in stmts.iter().enumerate() {
            // A `return` at a file's top level leaves the file there when it runs: what follows is
            // never defined, so it is not here either, and its first line says so. `check` took
            // a `fn` after it as defined, and the program did not start (FEEDBACK55.md, Q-818;
            // branch feedback55).
            if let Stmt::Return(_, at) = s {
                if let Some(next) = stmts[i + 1..].iter().find_map(top_line) {
                    self.diag_at(file.clone(), next, Severity::Warning, format!(
                        "this never runs: the `return` at line {} leaves the file first, so nothing below it is defined", at));
                }
                order.push(Item { stmt: s.clone(), file: file.clone() });
                break;
            }
            if let Stmt::Service(name, prefix, path, _, line) = s {
                self.everywhere.insert(name.clone());
                let Some(full) = self.file_at(base, path, &file, *line, false) else { continue };
                match crate::service::offered(&full) {
                    // A service that runs elsewhere need not be here: its calls are checked when
                    // the app asks it for its functions, as it starts (FEEDBACK40.md, Q-471;
                    // branch feedback40-43).
                    Err(_) if !full.exists() => {
                        self.diag_at(file.clone(), *line, Severity::Warning, format!(
                            "service {}: {} is not here, so its calls are not checked; it must run elsewhere ({} or --service-url)",
                            name, path, crate::service::env_name(name)
                        ));
                        self.services_elsewhere.insert(name.clone());
                        self.services.entry(name.clone()).or_default();
                    }
                    Err(e) => self.diag_at(file.clone(), *line, Severity::Error, e),
                    Ok(defs) => {
                        // Called from here: what it takes and gives, as declared.
                        let sigs = defs
                            .iter()
                            .map(|d| {
                                let mut sig = Sig::of_def(d);
                                sig.def = None;
                                sig.server = false;
                                sig.effects = Some(crate::ast::effect_bit("http"));
                                (d.name.clone(), Arc::new(sig))
                            })
                            .collect();
                        self.services.insert(name.clone(), sigs);
                        // A program of its own: checked alone (once, as services may use each other).
                        let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                        let first = SERVICE_CHECKS.with(|c| c.borrow_mut().insert(key.clone()));
                        if first {
                            let sub = check_file_opts(&full, HashMap::new(), self.options());
                            SERVICE_CHECKS.with(|c| c.borrow_mut().remove(&key));
                            self.out.diagnostics.extend(sub.diagnostics);
                            if let Some(prefix) = prefix {
                                self.out.symbols.extend(sub.symbols.into_iter().filter(|s| s.kind == SymbolKind::Route).map(|mut s| {
                                    s.name = s.name.replacen(' ', &format!(" {}", prefix), 1);
                                    s
                                }));
                            }
                        }
                    }
                }
                order.push(Item { stmt: s.clone(), file: file.clone() });
                continue;
            }
            // A type defined twice is reported with every other name (`defines`).
            if let Stmt::Type(td) = s {
                RECORDS.with(|r| r.borrow_mut().insert(td.name.clone(), td.clone()));
                self.everywhere.insert(td.name.clone());
            }
            if let Stmt::Mount(prefix, path, line) = s {
                // A mounted program is a program of its own: check it alone.
                let Some(full) = self.file_at(base, path, &file, *line, false) else { continue };
                if !full.is_file() {
                    self.diag_at(file.clone(), *line, Severity::Error, format!("cannot read {}: no such file", full.display()));
                } else {
                    let sub = check_file_opts(&full, HashMap::new(), self.options());
                    self.out.diagnostics.extend(sub.diagnostics);
                    self.out.symbols.extend(sub.symbols.into_iter().filter(|s| s.kind == SymbolKind::Route).map(|mut s| {
                        s.name = s.name.replacen(' ', &format!(" {}", prefix), 1);
                        s
                    }));
                }
                order.push(Item { stmt: s.clone(), file: file.clone() });
                continue;
            }
            if matches!(s, Stmt::Expr(Expr::Call(f, _, _)) if matches!(&**f, Expr::Ident(n, _) if n == "tenant_from")) {
                self.tenants = true;
            }
            // The file comes first now (FEEDBACK19.md, Q-156; branch feedback19-fixes).
            if let Stmt::Use(path, names, line, options) = s {
                if options.old {
                    let r = crate::modules::Respell::new(&file, *line, &crate::modules::resolve_use(base, Path::new(&*file), path, options.implied), names.as_deref(), options);
                    self.diag_at(file.clone(), *line, Severity::Warning, format!("write this as `{} use {}` (`quillo fix` does)", r.head, r.names));
                    self.out.respell.push(r);
                }
            }
            if let (Stmt::Use(path, Some(names), line, _), true) = (s, crate::ast::is_script(match s { Stmt::Use(p, ..) => p, _ => "" })) {
                for (global, name) in names {
                    let src = if global == name { path.clone() } else { format!("{} ({})", path, global) };
                    // A page has one of each name: from two scripts, the last was every file's, with
                    // no word, where two `.qwe` files' are an error (FEEDBACK55.md, Q-808; branch
                    // feedback55).
                    if let Some((_, other, f, l)) = self.js_libs.iter().find(|(n, o, ..)| n == name && *o != src) {
                        let msg = format!("`{}` is taken from {} at {}, and here from {}: a page has one `{}`, so take one of them under another name (`use {} as ...`)", name, other, self.shown_at(f, *l), src, name, global);
                        self.diag_at(file.clone(), *line, Severity::Error, msg);
                    }
                    self.js_libs.push((name.clone(), src, file.clone(), *line));
                    self.scripts.entry(name.clone()).or_insert((file.clone(), *line));
                }
                continue;
            }
            if let Stmt::Use(path, names, line, options) = s {
                // `import lib/m.qwe use price as print`: a builtin replaced everywhere, as a
                // `fn print` would be, which is warned of (FEEDBACK45.md, Q-507; branch
                // feedback40-43). A plugin's is said at its own line, below.
                if !crate::native::is_rust(path) {
                    for (name, alias) in names.iter().flatten().filter(|(n, a)| n != a) {
                        if self.globals.get(alias).is_some_and(|g| &*g.file == "<builtin>") {
                            self.diag_at(file.clone(), *line, Severity::Warning, format!(
                                "`{alias}` hides the builtin `{alias}`: taken under that name, `{name}` is this file's `{alias}`, so take it under its own name (`use {name}`) or another"));
                        }
                    }
                }
                // A path without its extension that two files could be: said, as the program
                // says it (FEEDBACK55.md, Q-819; branch feedback55).
                if options.implied {
                    let could = crate::modules::implied_files(base, Path::new(&*file), path);
                    if could.len() > 1 {
                        for (_, alias) in names.iter().flatten() {
                            let k = self.key_of(&file);
                            self.failed_imports.insert((k, alias.clone()));
                        }
                        if names.is_none() {
                            let k = self.key_of(&file);
                            self.failed_whole.insert(k);
                        }
                        self.diag_at(file.clone(), *line, Severity::Error, crate::modules::two_files(&could));
                        continue;
                    }
                }
                let path = &crate::modules::resolve_use(base, Path::new(&*file), path, options.implied);
                let Some(full) = self.file_at(base, path, &file, *line, true) else { continue };
                let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                // A `.qwer` file's names are taken by each `use` that names some (branch imports),
                // by each file whose names for them are its own, and by the first that takes them
                // all, after one that took some (Q-816), as the program defines them.
                let file_key = self.key_of(&file);
                let renames = self.links.rust_names(&file_key).cloned();
                let first = seen.insert(key.clone());
                let again = if crate::native::is_rust(path) { names.is_none() && renames.is_none() && !seen.insert(key.join("\0whole")) } else { !first };
                if again {
                    continue;
                }
                let fname: Arc<str> = Arc::from(full.display().to_string());
                let text = self.overrides.get(&full).cloned().map_or_else(|| crate::error::read_source(&full), Ok);
                // Rust: its functions' signatures, read without building it.
                if crate::native::is_rust(&path) {
                    if let Err(e) = crate::native::plugin_allowed(&full) {
                        self.diag_at(file.clone(), *line, Severity::Error, e);
                    }
                    match text {
                        Ok(src) => {
                            let parsed = crate::native::parse(&src);
                            for (l, msg) in parsed.problems {
                                self.diag_at(fname.clone(), l, Severity::Warning, msg);
                            }
                            let short = full.file_name().map_or(String::new(), |f| f.to_string_lossy().to_string());
                            if let Some(names) = names {
                                for (n, _) in names {
                                    if !parsed.exports.iter().any(|e| e.name == *n) {
                                        self.diag_at(file.clone(), *line, Severity::Error, crate::native::missing_export(&short, n, &parsed.exports));
                                    }
                                }
                            }
                            for e in parsed.exports.iter() {
                                let taken: Vec<String> = match names {
                                    Some(names) => names.iter().filter(|(n, _)| *n == e.name).map(|(_, a)| a.clone()).collect(),
                                    None => vec![e.name.clone()],
                                };
                                for alias in taken {
                                    // The file's own name for it: another file's of the same spelling is
                                    // another name (FEEDBACK55.md, Q-816; branch feedback55).
                                    let alias = match renames.as_ref().and_then(|m| m.get(&alias)) {
                                        Some(Some(other)) => other.clone(),
                                        // Of a file taken whole, another of that name, the file's
                                        // or taken by name, wins.
                                        Some(None) if names.is_none() => continue,
                                        _ => alias,
                                    };
                                    // How to take it under a name that parses: the export's name, after
                                    // the file's (Q-816).
                                    let other_name = crate::native::other_name(&short, &e.name);
                                    // A plugin's function replaces a builtin as a program's would, and its
                                    // user cannot see it: said at the `.qwer` file's line (FEEDBACK27.md,
                                    // Q-326; branch feedback27-35-fixes).
                                    if self.globals.get(&alias).is_some_and(|g| &*g.file == "<builtin>") {
                                        self.diag_at(fname.clone(), e.line, Severity::Warning, format!(
                                            "`{}` hides the builtin `{}`: the plugin's is used everywhere in the program, so rename it, or take it under another name: import {} use {} as {}",
                                            alias, alias, path, e.name, other_name));
                                    }
                                    // One name from two plugins in one file: the last `use`d wins, so say
                                    // so, and how to keep both (branch imports).
                                    if let Some((other, _)) = self.natives.iter().find(|(f, x)| *f != fname && x.name == alias) {
                                        let other = std::path::Path::new(&**other).file_name().map_or(String::new(), |f| f.to_string_lossy().to_string());
                                        self.diag_at(file.clone(), *line, Severity::Warning, format!(
                                            "`{}` comes from {} and {}: this one replaces the other; take one under another name: import {} use {} as {}",
                                            crate::privacy::demangle(&alias), other, short, path, e.name, other_name));
                                    }
                                    if !self.natives.iter().any(|(f, x)| *f == fname && x.name == alias) {
                                        self.natives.push((fname.clone(), crate::native::Export { name: alias, ..e.clone() }));
                                    }
                                }
                            }
                        }
                        Err(e) => self.missing_file(&file, *line, path, &full, names.as_deref(), e),
                    }
                    continue;
                }
                // Only a chain gets here: a file used again is not read again, so no cycle does
                // (FEEDBACK55.md, Q-822; branch feedback55).
                if self.uses >= MAX_USES {
                    self.diag_at(file.clone(), *line, Severity::Error, format!("this `use` is the end of a chain of files more than {} deep, each used by the one before, which is as far as files are followed: take some of them from one file", MAX_USES));
                    continue;
                }
                match text {
                    Ok(src) => match match (self.renames_for(&key, &src), self.links.parsed.get(&key)) {
                        (Some(names), _) => reparsed(&src, &names).map(Arc::new),
                        (None, Some(parsed)) => Ok(parsed.clone()),
                        (None, None) => Parser::new(&src).parse_program().map(Arc::new),
                    } {
                        Ok(sub) => {
                            self.uses += 1;
                            self.expand(sub, fname, base, seen, order);
                            self.uses -= 1;
                        }
                        Err(e) => {
                            if crate::error::may_be_newer_syntax(&e.msg) {
                                self.out.syntax_errors.push((fname.to_string(), e.line));
                            }
                            self.parse_errors.insert((fname.clone(), e.line));
                            self.unparsed_names.extend(names_in_text(&src));
                            self.unparsed_texts.push(src.clone());
                            self.diag_at(fname, e.line, Severity::Error, e.msg)
                        }
                    },
                    Err(e) => self.missing_file(&file, *line, path, &full, names.as_deref(), e),
                }
                continue;
            }
            order.push(Item { stmt: s.clone(), file: file.clone() });
        }
    }

    /// A file a `use` names that cannot be read: said once, at the import line, with a file near
    /// it, and its names not said again as unknown (FEEDBACK45.md, Q-523; branch feedback40-43).
    /// For every form: a file taken whole, whose names are not known, and a Rust one, where
    /// only `import x.qwe use name` was, and a near file of any of the program's kinds
    /// (FEEDBACK55.md, Q-819; branch feedback55).
    fn missing_file(&mut self, file: &Arc<str>, line: u32, path: &str, full: &Path, names: Option<&[(String, String)]>, e: std::io::Error) {
        let k = self.key_of(file);
        match names {
            Some(list) => {
                for (_, alias) in list {
                    self.failed_imports.insert((k.clone(), alias.clone()));
                }
            }
            None => {
                self.failed_whole.insert(k);
            }
        }
        let want = full.file_name().map_or(String::new(), |f| f.to_string_lossy().to_string());
        let near = full.parent().and_then(|d| std::fs::read_dir(d).ok()).and_then(|entries| {
            let kinds = |n: &str| crate::native::ends_with_any_case(n, ".qwe") || crate::native::is_rust(n);
            let names: Vec<String> = entries.flatten().map(|en| en.file_name().to_string_lossy().to_string()).filter(|n| kinds(n)).collect();
            // The same name of another kind first: `helpers.rs` for `helpers.qwe`.
            let stem = |n: &str| n.rsplit_once('.').map_or(n.to_string(), |(s, _)| s.to_string());
            names.iter().find(|n| stem(n) == stem(&want) && **n != want).cloned().or_else(|| closest(&want, names.iter().map(|n| n.as_str())))
        });
        let shown = path.trim_start_matches("./");
        let hint = near.map_or(String::new(), |n| format!(" (did you mean {}?)", Path::new(shown).with_file_name(n).display()));
        self.diag_at(file.clone(), line, Severity::Error, format!("cannot read {}: {}{}", shown, e, hint));
    }

    /// `lib/a.qwe:1`: a place in the program, from the main file's folder.
    fn shown_at(&self, f: &str, l: u32) -> String {
        let dir = Path::new(&*self.main).parent().unwrap_or(Path::new(""));
        let short = Path::new(f).strip_prefix(dir).unwrap_or(Path::new(f));
        format!("{}:{}", short.strip_prefix(".").unwrap_or(short).display(), l)
    }

    /// Before `name`, defined at the top level of `file`, is added: one
    /// the program already has is an error, wherever it is (the second
    /// would replace the first everywhere), and a function taking a
    /// builtin's name a warning (a variable may: `let count = 0`).
    /// `let x` twice in one file stays a variable given again.
    fn defines(&mut self, name: &str, file: &Arc<str>, line: u32, is_let: bool) {
        let Some(g) = self.globals.get(name) else { return };
        if &*g.file == "<builtin>" {
            if is_let {
                return;
            }
            let msg = format!("`{}` hides the builtin `{}`: the program's own is used everywhere, so pick another name", name, name);
            self.diag_at(file.clone(), line, Severity::Warning, msg);
            return;
        }
        // A top-level `let`'s variable, as yet unused and without a signature.
        if is_let && g.sig.is_none() && !g.used && g.file == *file {
            return;
        }
        let at = |f: &str, l: u32| self.shown_at(f, l);
        let msg = format!("`{}` is defined twice, at {} and {}: the second replaces the first everywhere, so rename one", name, at(&g.file, g.line), at(file, line));
        self.diag_at(file.clone(), line, Severity::Error, msg);
    }

    fn run(&mut self, order: Vec<Item>) {
        for it in &order {
            collect_assigned(std::slice::from_ref(&it.stmt), &mut self.assigned);
            global_assigned(std::slice::from_ref(&it.stmt), &mut self.assigned_globals);
        }
        // Helpers such as `fn table_of(id) = sql_name(`obj_${id}`)`, found
        // again until no more are: one may call another.
        let defs: Vec<&FnDef> = order.iter().filter_map(|it| if let Stmt::Fn(d) = &it.stmt { Some(&**d) } else { None }).collect();
        loop {
            let found: Vec<String> = defs
                .iter()
                .filter(|d| !d.component && !self.sql_name_fns.contains(&d.name) && gives_sql_safe(&d.body, &self.sql_name_fns))
                .map(|d| d.name.clone())
                .collect();
            if found.is_empty() {
                break;
            }
            self.sql_name_fns.extend(found);
        }
        self.check_nesting(&order);
        for (file, e) in std::mem::take(&mut self.natives) {
            let sig = native_sig(&e, &file);
            let short = file.rsplit('/').next().unwrap_or(&file).to_string();
            let mut detail = format!("{}  (Rust, {})", sig.text(), short);
            if !e.doc.is_empty() {
                detail = format!("{}\n\n{}", detail, e.doc);
            }
            self.out.symbols.push(Symbol { name: e.name.clone(), kind: SymbolKind::Function, file: file.to_string(), line: e.line, detail });
            self.globals.insert(e.name.clone(), Var { t: T::of(FN), sig: Some(Arc::new(sig)), file, line: e.line, kind: Kind::Global, used: true, ready: true });
        }
        for (name, src, file, line) in std::mem::take(&mut self.js_libs) {
            let detail = format!("{}  (JavaScript, in the browser: {})", name, src);
            self.out.symbols.push(Symbol { name: name.clone(), kind: SymbolKind::Variable, file: file.to_string(), line, detail });
            self.globals.insert(name, Var { t: T::of(ANY), sig: None, file, line, kind: Kind::Global, used: true, ready: true });
        }
        // Everything top-level is known up front; it becomes usable from
        // top-level code once the program has run its definition.
        for it in &order {
            // What each function reads, for a call from top-level code made before it is all
            // defined (FEEDBACK45.md, Q-514; branch feedback40-43).
            match &it.stmt {
                Stmt::Fn(d) => {
                    self.bodies.insert(d.name.clone(), d.clone());
                }
                Stmt::Let(Pattern::Bind(n), Expr::Lambda(d), _) => {
                    self.bodies.insert(n.to_string(), d.clone());
                }
                _ => {}
            }
            match &it.stmt {
                Stmt::Fn(d) => self.defines(&d.name, &it.file, d.line, false),
                Stmt::Service(name, _, _, _, line) => self.defines(name, &it.file, *line, false),
                Stmt::Type(td) => self.defines(&td.name, &it.file, td.line, false),
                Stmt::Let(p, _, line) => {
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    for n in names {
                        self.defines(&n, &it.file, *line, true);
                    }
                }
                _ => {}
            }
            match &it.stmt {
                Stmt::Fn(d) => {
                    let sig = Arc::new(Sig::of_def(d));
                    self.globals.insert(
                        d.name.clone(),
                        Var { t: T::of(FN), sig: Some(sig), file: it.file.clone(), line: d.line, kind: Kind::Global, used: false, ready: false },
                    );
                }
                Stmt::Service(name, _, _, _, line) => {
                    self.globals.insert(
                        name.clone(),
                        Var { t: T::of(MAP), sig: None, file: it.file.clone(), line: *line, kind: Kind::Global, used: true, ready: false },
                    );
                }
                Stmt::Type(td) => {
                    self.globals.insert(
                        td.name.clone(),
                        Var {
                            t: T::of(FN),
                            sig: Some(Arc::new(record_sig(td))),
                            file: it.file.clone(),
                            line: td.line,
                            kind: Kind::Global,
                            used: true,
                            ready: true,
                        },
                    );
                }
                Stmt::Let(p, _, line) => {
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    for n in names {
                        // The program's own `let first = ...` hides the builtin.
                        if self.globals.get(&n).is_some_and(|g| &*g.file != "<builtin>") {
                            continue;
                        }
                        self.globals.insert(
                            n,
                            Var { t: T::ANY, sig: None, file: it.file.clone(), line: *line, kind: Kind::Global, used: false, ready: false },
                        );
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
                Stmt::Service(name, _, _, _, _) => {
                    if let Some(g) = self.globals.get_mut(name) {
                        g.ready = true;
                    }
                    // Its options (Q-072).
                    self.scopes.clear();
                    self.stmt(&it.stmt, false);
                }
                // A guarded group's hidden route is no route of the program's (Q-539).
                Stmt::Route(r) if r.method == crate::ast::GUARD_METHOD => {}
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
                    if let (Pattern::Bind(n), Expr::Call(f, _, _)) = (p, e) {
                        if matches!(&**f, Expr::Ident(s, _) if s == "__site") {
                            self.sites.insert(n.to_string());
                        }
                    }
                    self.line = *line;
                    self.scopes.clear();
                    if self.literal_mismatch(p, e) {
                        self.error("this `let` never matches the value it is given, so it fails every time it runs");
                    }
                    self.capital_let(p, e);
                    let t = self.expr(e);
                    let mut binds = Vec::new();
                    self.bind_pattern(p, t, &mut binds);
                    self.keep_component(p, e, &mut binds);
                    self.keep_lambda(p, e, &mut binds);
                    // What text a global holds, for the SQL warning in the
                    // functions that use it; one reassigned anywhere holds
                    // nothing known.
                    if let Pattern::Bind(n) = p {
                        if !self.assigned.contains(n.as_str()) {
                            let k = self.sql_kind(e);
                            self.sql_vars.push((0, n.to_string(), k));
                            let u = self.url_fixed(e);
                            self.url_vars.push((0, n.to_string(), u));
                        }
                    }
                    for (n, t, sig) in binds {
                        let reassigned = self.assigned.contains(&n);
                        self.note_first_t(&it.file, *line, &n, t);
                        if let Some(g) = self.globals.get_mut(&n) {
                            g.ready = true;
                            if g.file == it.file && g.line == *line {
                                g.t = if reassigned { T::ANY } else { t };
                                g.sig = if reassigned { None } else { sig };
                            }
                        }
                        let detail = format!("let {}: {}", n, if reassigned { "any".to_string() } else { t_text(t) });
                        self.out.symbols.push(Symbol { name: n, kind: SymbolKind::Variable, file: it.file.to_string(), line: *line, detail });
                    }
                }
                other => {
                    self.scopes.clear();
                    // `xs.push(2)`, `i + 1` alone at the top level (FEEDBACK45.md, Q-505).
                    self.answer_dropped(other);
                    self.op_dropped(other);
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
                // A guarded group's hidden route calls the middleware each of its routes does,
                // checked there (FEEDBACK47.md, Q-539).
                Stmt::Route(r) if r.method == crate::ast::GUARD_METHOD => {}
                Stmt::Route(r) => self.route(r),
                _ => {}
            }
        }
    }

    // ---- scopes ------------------------------------------------------

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
        // Each scope's own number, so a `let` can be told to enclose a read (Checker::read_before).
        // Scopes cleared in a body leave 0s, which match nothing.
        let n = self.scopes.len() - 1;
        self.scope_ids.resize(n, 0);
        self.scope_serial += 1;
        self.scope_ids.push(self.scope_serial);
    }

    fn pop(&mut self) {
        let depth = self.scopes.len().saturating_sub(1);
        while self.sql_vars.last().map_or(false, |(d, _, _)| *d > depth) {
            self.sql_vars.pop();
        }
        while self.url_vars.last().map_or(false, |(d, _, _)| *d > depth) {
            self.url_vars.pop();
        }
        let popped = self.scopes.pop();
        self.scope_ids.truncate(self.scopes.len());
        if let Some(scope) = popped {
            for (name, v) in scope {
                // Quillo's own names (`__request`) are no editor's to offer (FEEDBACK47.md, Q-535).
                if self.quiet == 0 && !name.starts_with("__") {
                    let detail = var_detail(&name, &v);
                    let (file, line, end) = (v.file.to_string(), v.line, self.line.max(v.line));
                    let statement = matches!(v.kind, Kind::Let | Kind::State);
                    self.out.locals.push(Local { name: name.clone(), file, line, end, detail, statement });
                }
                if v.kind == Kind::Let && !v.used && !name.starts_with('_') && self.quiet == 0 {
                    self.unused.push((v.file.clone(), v.line, name));
                }
            }
        }
    }

    /// `print(tmp)`, then `let tmp = 1`, in one function: said as the top level says it, and
    /// `tmp` is not unused (FEEDBACK45.md, Q-524; branch feedback40-43). The `let` is at the
    /// read's level or around it.
    fn read_before(&mut self, name: &str) -> bool {
        if self.quiet > 0 || self.early.is_empty() {
            return false;
        }
        let (fns, file) = (self.fns.len(), self.file.clone());
        let Some(scope) = self.scope_ids.get(self.scopes.len().wrapping_sub(1)).copied().filter(|i| *i != 0 && self.scope_ids.len() == self.scopes.len()) else {
            return false;
        };
        let (found, rest): (Vec<Early>, Vec<Early>) =
            std::mem::take(&mut self.early).into_iter().partition(|e| e.name == name && e.file == file && e.fns == fns && e.chain.contains(&scope));
        self.early = rest;
        // A closure's read is fine if it runs after the `let`, as it most often does.
        for e in found.iter().filter(|e| !e.nested) {
            self.diag_at(e.file.clone(), e.line, Severity::Error, format!("`{}` is used before it is defined (line {})", name, self.line));
        }
        !found.is_empty()
    }

    /// A `let` only `+=` and `-=` change keeps its first value's type for them (Q-525).
    fn note_first_t(&mut self, file: &Arc<str>, line: u32, name: &str, t: T) {
        if self.assigned.contains(name) && !self.assigned.contains(&format!("={}", name)) && !t.vague() && t.bits & NIL == 0 {
            self.first_t.insert((file.clone(), line, name.to_string()), t);
        }
    }

    /// Routes no request reaches, and the server fns a guarded page calls that guard nothing
    /// themselves (FEEDBACK47.md, Q-541 and Q-538; branch feedback40-43).
    fn route_reach(&mut self, order: &[Item]) {
        let routes: Vec<(Arc<str>, Arc<RouteDef>)> = order
            .iter()
            .filter_map(|it| match &it.stmt {
                Stmt::Route(r) if r.method != crate::ast::GUARD_METHOD => Some((it.file.clone(), r.clone())),
                _ => None,
            })
            .collect();
        for (i, (file, r)) in routes.iter().enumerate() {
            if r.method == "HEAD" {
                self.diag_at(file.clone(), r.line, Severity::Error, "`route HEAD` is never matched: a HEAD request is answered by the GET route, with its headers and no body");
            }
            if matches!(r.path.first(), Some(Seg::Lit(l)) if l == "_quillo") {
                self.diag_at(file.clone(), r.line, Severity::Error, "a route under `/_quillo/` is never called: Quillo answers those paths itself, so pick another");
            }
            // An earlier route with no pattern takes every request this one would: the first
            // that matches answers. Dangerous when this one is in a guarded group and that one is
            // not, as a page left from before the group was added. Not one with middleware, whose
            // null says it does not match, and the next route is tried (FEEDBACK51.md, Q-651).
            if let Some((efile, e)) = routes[..i].iter().find(|(_, e)| route_covers(e, r) && middleware_of(e).is_none()) {
                let mine = middleware_of(r);
                let theirs = middleware_of(e);
                let unguarded = match (&mine, &theirs) {
                    (Some(m), None) => format!(", without `{}`, this route's middleware", m),
                    _ => String::new(),
                };
                // Another file's route by its file too (found in review).
                let at = if efile == file { format!("line {}", e.line) } else { self.shown_at(efile, e.line) };
                self.diag_at(file.clone(), r.line, Severity::Warning, format!(
                    "this route is never reached: `{}` ({}) takes every request it would{}",
                    e.shown(), at, unguarded
                ));
            }
        }
        // Q-538: a group guards its routes, not the server fns their pages call, each of which is
        // its own `POST /_quillo/rpc/<name>`. The program's functions, views and components by
        // name, as this runs before the rest of the check.
        let fns: HashMap<String, Arc<FnDef>> = order
            .iter()
            .filter_map(|it| match &it.stmt {
                Stmt::Fn(d) => Some((d.name.clone(), d.clone())),
                _ => None,
            })
            .collect();
        let files: HashMap<String, Arc<str>> = order
            .iter()
            .filter_map(|it| match &it.stmt {
                Stmt::Fn(d) => Some((d.name.clone(), it.file.clone())),
                _ => None,
            })
            .collect();
        let mut told: HashSet<String> = HashSet::new();
        for (_, r) in &routes {
            let Some(guard) = middleware_of(r) else { continue };
            for d in server_fns_reached(&fns, &r.body) {
                if guards_itself(&fns, &d.body, &mut HashSet::new()) || !told.insert(d.name.clone()) {
                    continue;
                }
                let file: Arc<str> = files.get(&d.name).cloned().unwrap_or_else(|| self.file.clone());
                self.diag_at(file, d.line, Severity::Warning, format!(
                    "`{}` is called by `{}`'s page, which `{}` guards, but a group guards its routes, not the server fns their pages call: anyone can call it at POST /_quillo/rpc/{}; if it is not for everyone, check the request in it too, as `{}(request)`",
                    d.name, r.shown(), guard, d.name, guard.trim_end_matches("(...)")
                ));
            }
        }
    }

    /// After a function's body: its unknown names that no `let` defined are said, or, in a
    /// function inside another, wait for the outer one's `let`s; its ended blocks are forgotten.
    fn fn_ended(&mut self) {
        let n = self.fns.len();
        for mut e in std::mem::take(&mut self.early) {
            if e.fns <= n {
                self.early.push(e);
            } else if n == 0 {
                self.diag_at(e.file.clone(), e.line, Severity::Error, e.msg);
            } else {
                e.fns = n;
                e.nested = true;
                self.early.push(e);
            }
        }
        self.gone.retain(|g| g.fns <= n);
    }

    /// The unused variables, once every read has been seen.
    fn unused_said(&mut self) {
        for e in std::mem::take(&mut self.early) {
            self.diag_at(e.file.clone(), e.line, Severity::Error, e.msg);
        }
        for (file, line, name) in std::mem::take(&mut self.unused) {
            self.diag_at(file, line, Severity::Warning, format!("unused variable `{}` (name it `_{}` if that is intended)", name, name));
        }
    }

    /// `if c { let x = 1 }`, then `x`: where `x` was, and how to have it after the block (Q-524).
    fn gone_name(&mut self, name: &str) -> Option<String> {
        let (fns, file, line) = (self.fns.len(), self.file.clone(), self.line);
        let g = self.gone.iter().rev().find(|g| g.name == name && g.file == file && g.fns == fns && g.line <= line)?;
        let msg = format!(
            "`{}` is declared inside the `{}` (line {}), and is not seen after it: declare it before the `{}` (`let {} = ...`), and assign it inside",
            name, g.kind, g.line, g.kind, name
        );
        let key = (g.file.clone(), g.line, g.name.clone());
        self.unused.retain(|u| *u != key);
        Some(msg)
    }

    /// A second local `fn` or `state` of one name in one body: the second replaced the first
    /// without a word, as a top-level one does not (FEEDBACK45.md, Q-508; branch feedback40-43).
    fn twice(&mut self, name: &str, kind: Kind) {
        if let Some(at) = self.scopes.last().and_then(|s| s.get(name)).filter(|v| v.kind == kind).map(|v| v.line) {
            self.error(format!("`{}` is defined twice here, at line {} and line {}: the second replaces the first, so rename one", name, at, self.line));
        }
    }

    fn define(&mut self, name: &str, t: T, sig: Option<Arc<Sig>>, kind: Kind) {
        let sql = std::mem::replace(&mut self.next_sql, SqlVar::Other);
        self.sql_vars.push((self.scopes.len(), name.to_string(), sql));
        let url = std::mem::replace(&mut self.next_url, false);
        self.url_vars.push((self.scopes.len(), name.to_string(), url));
        let v = Var { t, sig, file: self.file.clone(), line: self.line, kind, used: false, ready: true };
        let read_before = kind == Kind::Let && self.read_before(name);
        match self.scopes.last_mut() {
            Some(s) => {
                // `let x` twice in one scope reuses the variable.
                let used = read_before || s.get(name).map_or(false, |old| old.used);
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
        let name = name.strip_suffix(OLD).unwrap_or(name);
        self.taken(name, v);
        let def = if &*v.file == "<builtin>" { None } else { Some((v.file.to_string(), v.line)) };
        let detail = var_detail(name, v);
        if v.kind == Kind::State {
            let at = self.out.references.len();
            if let Some(e) = self.state_types.get_mut(&(v.file.clone(), v.line, name.to_string())) {
                e.1.push(at);
            }
        }
        self.out.references.push(Reference { file: self.file.to_string(), line: self.line, name: name.to_string(), def, detail });
    }

    /// A global of another file must be one this file took (FEEDBACK19.md, Q-154; branch
    /// feedback19-fixes). For now that is a warning, and the name still works, so a
    /// program can move a file at a time; `quillo fix` writes the `use`.
    fn taken(&mut self, name: &str, v: &Var) {
        if v.kind != Kind::Global || &*v.file == "<builtin>" || v.file == self.file || self.everywhere.contains(name) {
            return;
        }
        let (file, from) = (self.key_of(&self.file.clone()), self.key_of(&v.file));
        if let Some(choices) = self.links.choices(&file, name) {
            let shown: Vec<String> = choices.iter().map(|f| self.links.written(f)).collect();
            self.error(format!("`{}` comes from {}: take the one you mean: import {} use {}", name, shown.join(" and "), shown[0], name));
            return;
        }
        // A browser name is taken by a file's own `client` line: a file taken whole does not pass
        // its on, as it passes no name it takes (FEEDBACK55.md, Q-822; branch feedback55).
        let script = self.scripts.get(name).cloned();
        let sees = if script.is_some() { self.links.took(&file, name) } else { self.links.sees(&file, &from, name) };
        if sees || !self.told.insert((self.file.clone(), name.to_string())) {
            return;
        }
        let shown = crate::privacy::demangle(name);
        let at = self.links.written(&from);
        self.warn(match script {
            Some(_) => format!("`{}` is a browser library this file does not take: write its line here too (`quillo fix` does)", shown),
            None => format!("`{}` comes from {}, which this file does not take: add `import {} use {}` (`quillo fix` writes it)", shown, at, at, shown),
        });
        self.out.imports.push(crate::check::Import {
            file: self.file.to_string(),
            name: shown,
            from: at,
            script: script.map(|(f, l)| (f.to_string(), l)),
        });
    }

    fn key_of(&mut self, file: &Arc<str>) -> PathBuf {
        self.keys.entry(file.clone()).or_insert_with(|| crate::modules::key(Path::new(&**file))).clone()
    }

    /// Hover text for state: the types it takes over the component, which
    /// checking itself does not narrow to (an assigned state is `any` there).
    fn state_details(&mut self) {
        for ((_, _, name), (t, refs)) in std::mem::take(&mut self.state_types) {
            let text = format!("state {}: {}", name, t_text(t));
            for i in refs {
                if let Some(r) = self.out.references.get_mut(i) {
                    r.detail = text.clone();
                }
            }
        }
    }

    /// The editor is told each name as its file wrote it: `helper` or `a_helper`, not
    /// what linking made of them, which reads as a private name and is the same word for
    /// two functions (FEEDBACK19.md, Q-154; branch feedback19-fixes).
    fn as_written(&mut self) {
        let links = std::mem::take(&mut self.links);
        let mut written: HashMap<String, HashMap<String, String>> = HashMap::new();
        for file in self.out.files.clone() {
            let Some(names) = links.renames(&crate::modules::key(Path::new(&file))) else { continue };
            // What annotations' types are called is no name an editor shows (FEEDBACK55.md,
            // Q-821; branch feedback55).
            let mut names: Vec<(&String, &String)> = names.iter().filter(|(n, _)| !n.starts_with(crate::privacy::TYPE_ALIAS)).collect();
            names.sort();
            for (name, inner) in names.iter().rev() {
                written.entry(file.clone()).or_default().insert(inner.to_string(), name.to_string());
            }
            for (name, inner) in names {
                if let Some(s) = self.out.symbols.iter().find(|s| s.name == *inner && s.file != file) {
                    let shown = Symbol { name: name.clone(), detail: crate::privacy::demangle(&s.detail), ..s.clone() };
                    self.out.taken.push((file.clone(), shown));
                }
            }
        }
        let shown = |file: &str, name: &mut String, detail: &mut String| {
            if let Some(w) = written.get(file).and_then(|names| names.get(name.as_str())) {
                *name = w.clone();
                *detail = crate::privacy::demangle(detail);
            }
        };
        for r in &mut self.out.references {
            shown(&r.file, &mut r.name, &mut r.detail);
        }
        for s in &mut self.out.symbols {
            shown(&s.file, &mut s.name, &mut s.detail);
        }
        for l in &mut self.out.locals {
            shown(&l.file, &mut l.name, &mut l.detail);
        }
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
        let v = match self.globals.get(name) {
            Some(v) => v.clone(),
            None => self.old_name(name)?,
        };
        self.record(name, &v);
        Some(v)
    }

    /// `price` where this file took it `as p`: still the other file's, so the program runs,
    /// but not a name this file took (FEEDBACK45.md, Q-523).
    fn old_name(&mut self, name: &str) -> Option<Var> {
        let base = name.strip_suffix(OLD)?;
        let v = self.globals.get(base).cloned()?;
        let key = self.key_of(&self.file.clone());
        let alias = self.links.old_names.get(&key).and_then(|o| o.get(base)).cloned();
        if let Some(alias) = alias {
            if self.told.insert((self.file.clone(), name.to_string())) {
                self.warn(format!("this file takes `{}` as `{}`: write `{}`, or take `{}` too", base, alias, alias, base));
            }
        }
        Some(v)
    }

    /// What `x.name` finds when `x` does not have it: a function, called on `x`,
    /// which makes it a use of a local of that name (FEEDBACK16.md, Q-137; branch
    /// feedback20-fixes).
    fn method(&mut self, name: &str) -> Option<Var> {
        match self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name)) {
            Some(v) => {
                v.used = true;
                Some(v.clone())
            }
            None => self.globals.get(name).cloned().or_else(|| self.old_name(name)),
        }
    }

    fn ident(&mut self, name: &str) -> (T, Option<Arc<Sig>>) {
        match self.lookup(name) {
            Some(v) => {
                if v.kind == Kind::Global && !v.ready && self.sequential && self.fns.is_empty() {
                    // Another file's line says which file (FEEDBACK45.md, Q-523).
                    let at = if v.file == self.file {
                        format!("line {}", v.line)
                    } else {
                        let key = self.key_of(&v.file);
                        format!("{}:{}", self.links.written(&key), v.line)
                    };
                    self.error(format!("`{}` is used before it is defined ({})", name, at));
                }
                if v.kind == Kind::Global && &*v.file != "<builtin>" {
                    self.browser_global(name, &v);
                }
                if v.kind == Kind::Global && name == "db" && &*v.file == "<builtin>" {
                    self.browser_forbidden(&format!("`{}`", name));
                }
                (v.t, v.sig)
            }
            None => {
                if name == "request" {
                    self.error("`request` only exists in routes and `server fn`s; pass what you need as an argument");
                } else if !self.unparsed_names.contains(name) && !self.failed_import(name) {
                    let removed = crate::error::removed_note(name);
                    // Another file's `private` name (branch public-private).
                    let cwd = std::env::current_dir().ok();
                    let mut private_in: Vec<String> = self.globals.iter()
                        .filter(|(k, _)| crate::privacy::private_name(k) == Some(name))
                        .map(|(_, v)| {
                            let p = std::path::Path::new(&*v.file);
                            cwd.as_ref().and_then(|d| p.strip_prefix(d).ok()).unwrap_or(p).display().to_string().trim_start_matches("./").to_string()
                        })
                        .collect();
                    private_in.sort();
                    if let Some(files) = self.links.shared(name) {
                        // Several files define it, and this one took it from none.
                        let shown: Vec<String> = files.iter().map(|f| self.links.written(f)).collect();
                        self.error(format!("`{}` comes from {}: take the one you mean: import {} use {}", name, shown.join(" and "), shown[0], name));
                    } else if !private_in.is_empty() {
                        self.error(format!("`{}` is private to {}: only {} sees it (make it `public` there, or call something public)", name,
                            private_in.join(" and "), if private_in.len() == 1 { "that file" } else { "each of those files" }));
                    } else {
                        // `let walk = (n) => walk(n - 1)`: it does not exist yet, and a guess
                        // at another name helps nobody (FEEDBACK25.md, Q-324; branch
                        // feedback26-fixes).
                        if self.letting.iter().any(|l| l == name) {
                            self.error(format!("`{}` does not exist until its `let` ends: a function that calls itself is a `fn`", name));
                        } else if let Some(msg) = self.gone_name(name) {
                            self.error(msg);
                        } else {
                            let hint = if !removed.is_empty() { removed } else { self.lookalike(name).unwrap_or_else(|| self.suggest(name)) };
                            let msg = format!("unknown name `{}`{}", name, hint);
                            if self.fns.is_empty() || self.quiet > 0 {
                                self.error(msg);
                            } else {
                                let (file, line, fns) = (self.file.clone(), self.line, self.fns.len());
                                let chain = self.scope_ids.iter().copied().take(self.scopes.len()).filter(|i| *i != 0).collect();
                                self.early.push(Early { name: name.to_string(), file, line, msg, fns, chain, nested: false });
                            }
                        }
                    }
                }
                (T::ANY, None)
            }
        }
    }

    /// A name this file's import line could not take, said there (Q-523): by file, as
    /// another file's own unknown name of that spelling is still said (found in review).
    fn failed_import(&mut self, name: &str) -> bool {
        if self.failed_imports.is_empty() && self.failed_whole.is_empty() {
            return false;
        }
        let k = self.key_of(&self.file.clone());
        // A file taken whole that is not there may have had any name (Q-819).
        self.failed_whole.contains(&k) || self.failed_imports.contains(&(k, name.to_string()))
    }

    /// `a` where the program has `а`, a Cyrillic letter: the same to the eye, another name
    /// (FEEDBACK45.md, Q-524). Says which letter, by its code point.
    fn lookalike(&self, name: &str) -> Option<String> {
        let key: String = name.chars().map(latin_lookalike).collect();
        let mut cands: Vec<(&str, u32)> = self.scopes.iter().flat_map(|s| s.iter().map(|(k, v)| (k.as_str(), v.line))).collect();
        cands.extend(self.globals.iter().filter(|(_, v)| &*v.file != "<builtin>").map(|(k, v)| (k.as_str(), v.line)));
        cands.sort_unstable();
        let (other, line) = cands.into_iter().find(|(c, _)| *c != name && c.chars().map(latin_lookalike).collect::<String>() == key)?;
        let odd = |w: &str| w.chars().find(|c| latin_lookalike(*c) != *c);
        let (c, whose) = match odd(name) {
            Some(c) => (c, "this one".to_string()),
            None => (odd(other)?, format!("the `{}` on line {}", other, line)),
        };
        let script = if ('\u{370}'..='\u{3ff}').contains(&c) { "Greek" } else { "Cyrillic" };
        Some(format!(
            " (`{}` is defined on line {} with other letters that look the same: {} has U+{:04X}, a {} letter, not `{}`)",
            other, line, whose, c as u32, script, latin_lookalike(c)
        ))
    }

    fn suggest(&self, name: &str) -> String {
        // `<DIV CLASS="a">`, pasted from older HTML, whose tags are any case: a capital starts a
        // view's name here (FEEDBACK36.md, Q-449; branch feedback36-39).
        const HTML_TAGS: &[&str] = &["a", "abbr", "article", "aside", "b", "body", "br", "button", "caption", "center", "code", "dd", "div", "dl", "dt", "em",
            "font", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr", "html", "i", "iframe", "img", "input", "label", "li", "link",
            "main", "meta", "nav", "ol", "option", "p", "pre", "script", "section", "select", "small", "span", "strong", "style", "table", "tbody", "td",
            "textarea", "th", "thead", "title", "tr", "u", "ul"];
        let lower = name.to_ascii_lowercase();
        if name.len() > 1 && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) && HTML_TAGS.contains(&lower.as_str()) {
            return format!(" (HTML tags are written in lowercase here, <{}>: a capital letter starts a view's name)", lower);
        }
        // Other languages' names for what Quillo has (FEEDBACK45.md, Q-517; branch feedback40-43).
        if let Some(q) = other_language(name) {
            return format!(" (did you mean `{}`?)", q);
        }
        // The same answer every run: the candidates in a fixed order, ties broken by `closest`.
        let mut cands: Vec<&str> = self.scopes.iter().flat_map(|s| s.keys().map(|k| k.as_str())).collect();
        cands.extend(self.globals.keys().map(|k| k.as_str()));
        cands.sort_unstable();
        cands.dedup();
        match closest(name, cands.into_iter()) {
            Some(n) => format!(" (did you mean `{}`?)", n),
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
        // Not for a parameter or a local of that name, in the program or a kit it uses
        // (FEEDBACK28.md, Q-348; branch feedback29-fixes).
        if self.assigned_globals.contains(name) {
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
            ret: d.ret.as_ref().map(ty_t),
            err: d.err.as_ref().map(ty_bits),
            allowed: effects.unwrap_or(ALL_EFFECTS) & crate::policy::ceiling(),
            restrictor: d.effects.as_ref().map(|e| (d.name.clone(), e.clone())),
            component: d.component.then(|| d.name.clone()),
            nested: false,
            state_init: false,
            exits: T::NEVER,
        });
        self.push();
        if d.server {
            self.define("request", request_t(), None, Kind::Bound);
        }
        self.params(d);
        let t = self.expr(&d.body);
        self.check_return_expr(&d.body, t);
        if d.component {
            self.run_once_assignments(d);
        }
        self.pop();
        self.fns.pop();
        self.fn_ended();
    }

    /// `count = len(items)` in a component's body runs once, as it starts, on the server and in
    /// the browser: it does not follow `items`, or a prop, when they change, which reads as if it
    /// would (as a `let` does). Made an effect, cycles went round (`a = b + 1`, `b = a + 1`), a
    /// conversion rewrote what was being typed, and a parent's equal new value undid a pick: said
    /// instead (FEEDBACK55.md, Q-764; branch feedback55).
    fn run_once_assignments(&mut self, d: &FnDef) {
        let Expr::Block(stmts) = &d.body else { return };
        let mut changing: Vec<&str> = d.params.iter().map(|p| p.name.as_str()).collect();
        changing.extend(stmts.iter().filter_map(|st| if let Stmt::State(n, ..) = st { Some(n.as_str()) } else { None }));
        for st in stmts {
            let Stmt::Assign(name, AssignOp::Set, e, line) = st else { continue };
            if !changing.contains(&name.as_str()) {
                continue;
            }
            let mut read: Option<String> = None;
            crate::client::each_expr(e, &mut |x| {
                if let Expr::Ident(n, _) = x {
                    if read.is_none() && n.as_str() != name.as_str() && changing.contains(&n.as_str()) {
                        read = Some(n.to_string());
                    }
                }
            });
            if let Some(r) = read {
                self.line = *line;
                self.warn(format!(
                    "`{} = ...` runs once, as <{}> starts: it does not follow `{}` when that changes; for a value that follows it, write `let {} = ...` in place of the state",
                    name, d.name, r, name
                ));
            }
        }
    }

    fn params(&mut self, d: &FnDef) {
        self.params_hinted(d, None)
    }

    /// Parameters, those without an annotation typed by `hint` if given.
    fn params_hinted(&mut self, d: &FnDef, hint: Option<Vec<T>>) {
        // `with db, db` (FEEDBACK45.md, Q-508; branch feedback40-43).
        if let Some(effs) = &d.effects {
            if let Some(e) = effs.iter().enumerate().find(|(i, e)| effs[..*i].contains(e)).map(|(_, e)| e) {
                self.warn(format!("`{}` is named twice in this `with`", e));
            }
        }
        for (i, p) in d.params.iter().enumerate() {
            // `a: Int`, `a: String`: Kotlin's and Java's names, taken for a record of the
            // program's own that is not there, and every call failed (FEEDBACK45.md, Q-519;
            // branch feedback40-43).
            if let Some(ty) = &p.ty {
                for n in &ty.names {
                    let builtin = match n.as_str() {
                        "Int" | "Integer" | "Long" => Some("int"),
                        "String" | "Str" => Some("str"),
                        "Bool" | "Boolean" => Some("bool"),
                        "Any" | "Object" => Some("any"),
                        "Float" | "Double" => Some("float"),
                        "List" | "Array" => Some("list"),
                        "Map" | "Dict" => Some("map"),
                        _ => None,
                    };
                    if let Some(b) = builtin.filter(|_| !RECORDS.with(|r| r.borrow().contains_key(n)) && !self.globals.contains_key(n.as_str())) {
                        self.error(format!("there is no type `{}`: the builtin types are written in small letters, `{}`", n, b));
                    }
                }
            }
            // `fn f(a, a)`: the last argument was `a` (FEEDBACK45.md, Q-508; branch
            // feedback40-43).
            if d.params[..i].iter().any(|q| q.name == p.name) && p.name != "_" {
                self.error(format!("parameter `{}` is named twice: each parameter needs a name of its own", p.name));
            }
            let mut t = p.ty.as_ref().map_or(T::ANY, ty_t);
            if let (None, None, Some(h)) = (&p.ty, &p.default, hint.as_ref().and_then(|h| h.get(i))) {
                t = *h;
            }
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
            } else if self.strict_null && !t.vague() && vals & NIL != 0 && vals != NIL && r.bits & NIL == 0 {
                self.warn(format!("`{}` must return {}, but this may be null (it is {})", name, bits_text(r.bits), t_text(t)));
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
        // `? { page: itn }` binds `itn` and tests nothing; `{ json: Usr }` never matches
        // (FEEDBACK45.md, Q-504; branch feedback40-43).
        for p in r.query.iter().chain(r.body_pat.iter()).chain(r.guards.iter()) {
            self.pattern_types(p);
            self.field_type_typos(p);
        }
        self.fns.push(FnCtx {
            name: format!("{} {}", r.method, route_path(r)),
            ret: None,
            err: None,
            // `route ... with db` (branch feedback40-43).
            allowed: r.effects.as_ref().map_or(ALL_EFFECTS, |e| effect_mask(e)) & crate::policy::ceiling(),
            restrictor: r.effects.as_ref().map(|e| (format!("route {}", r.shown()), e.clone())),
            component: None,
            nested: false,
            state_init: false,
            exits: T::NEVER,
        });
        self.push();
        self.define("request", request_t(), None, Kind::Bound);
        // What a group's middleware is given (FEEDBACK47.md, Q-535; branch feedback40-43).
        self.define("__request", request_t(), None, Kind::Bound);
        let mut seen = HashSet::new();
        for seg in &r.path {
            match seg {
                Seg::Param(n, ty, _) => {
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
        // A `routes` group's pattern binds as the route's own does (branch route-groups).
        for g in &r.guards {
            self.bind_pattern(g, T::of(MAP), &mut binds);
        }
        if let Some(b) = &r.body_pat {
            self.bind_pattern(b, T::of(MAP), &mut binds);
        }
        for (n, t, sig) in binds {
            self.define(&n, t, sig, Kind::Bound);
        }
        // `:id + 1` gives "51": an untyped parameter is text (FEEDBACK45.md, Q-513).
        self.text_params_depth = self.scopes.len();
        self.text_params = r.path.iter().filter_map(|seg| match seg {
            Seg::Param(n, None, _) => Some(n.clone()),
            _ => None,
        }).collect();
        self.path_params = r.path.iter().filter_map(|seg| match seg {
            Seg::Param(n, None, _) | Seg::Rest(n) => Some(n.clone()),
            Seg::Param(n, Some(t), _) if &**t == "str" => Some(n.clone()),
            _ => None,
        }).collect();
        self.expr(&r.body);
        // A function in the answer, where it is written: JSON has none, and the answer is an
        // internal error (FEEDBACK55.md, Q-792; found in review: it went out as "<fn f>" before).
        let mut found = None;
        self.function_in_answer(&r.body, "", &[], &mut found);
        if let Some((path, what)) = found {
            self.line = r.line;
            let at = if path.is_empty() { String::new() } else { format!(" at `{}`", path.trim_start_matches('.')) };
            self.warn(format!("this route's answer holds a function{} ({}): JSON has none, so the answer is an internal error (500): leave it out, or call it", at, what));
        }
        self.text_params.clear();
        self.path_params.clear();
        self.pop();
        self.fns.pop();
        self.fn_ended();
    }

    /// The first function written in what `e`, a route's body, answers: a function's name or a
    /// lambda, in the maps and lists written there (through blocks, `if` and `match`), with
    /// where it is and what it is. `local`: the names the blocks around it bind, the last one
    /// counting, each with whether it is a function the block makes.
    fn function_in_answer(&self, e: &Expr, path: &str, local: &[(String, bool)], found: &mut Option<(String, String)>) {
        if found.is_some() {
            return;
        }
        match e {
            Expr::Lambda(_) if !path.is_empty() => *found = Some((path.to_string(), "a function written there".into())),
            // A function the block makes (`fn helper() = 2`, `let f = (x) => x`) is a function
            // in the answer too, as the program's of that name is: check said nothing and the
            // route answered 500 (FEEDBACK55.md, Q-792; branch feedback55).
            Expr::Ident(n, _) if !path.is_empty() && local.iter().rev().find(|(h, _)| h == n.as_str()).is_some_and(|(_, f)| *f) => {
                *found = Some((path.to_string(), format!("the function `{}`", n)));
            }
            // A function of the program's, or a builtin, by its name, where nothing hides it.
            Expr::Ident(n, _) if !path.is_empty() && !local.iter().any(|(h, _)| h == n.as_str()) && !self.is_local(n) && self.globals.get(n.as_str()).is_some_and(|g| g.t.bits == FN) => {
                *found = Some((path.to_string(), format!("the function `{}`", n)));
            }
            Expr::Map(entries) => {
                for (k, v) in entries {
                    let step = if fmt_key_bare(k) { format!("{}.{}", path, k) } else { format!("{}[{}]", path, serde_json::to_string(k).unwrap_or_default()) };
                    self.function_in_answer(v, &step, local, found);
                }
            }
            Expr::List(items) => {
                for (i, v) in items.iter().enumerate() {
                    self.function_in_answer(v, &format!("{}[{}]", path, i), local, found);
                }
            }
            Expr::Block(stmts) => {
                // The names the block binds itself hide the program's: a value, or a function
                // of its own.
                let mut inner = local.to_vec();
                for st in stmts {
                    match st {
                        Stmt::Let(Pattern::Bind(n), Expr::Lambda(_), _) => inner.push((n.to_string(), true)),
                        Stmt::Let(p, _, _) => {
                            let mut names = Vec::new();
                            pattern_names(p, &mut names);
                            inner.extend(names.into_iter().map(|n| (n, false)));
                        }
                        Stmt::Fn(d) => inner.push((d.name.clone(), true)),
                        _ => {}
                    }
                }
                if let Some(Stmt::Expr(last)) = stmts.last() {
                    self.function_in_answer(last, path, &inner, found);
                }
            }
            Expr::If(_, a, b) => {
                self.function_in_answer(a, path, local, found);
                if let Some(b) = b {
                    self.function_in_answer(b, path, local, found);
                }
            }
            Expr::Match(_, arms, _) => {
                for a in arms {
                    let mut names = Vec::new();
                    pattern_names(&a.pat, &mut names);
                    let mut inner = local.to_vec();
                    inner.extend(names.into_iter().map(|n| (n, false)));
                    self.function_in_answer(&a.body, path, &inner, found);
                }
            }
            _ => {}
        }
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
                // A body gives its last expression, so of two elements side by
                // side the first goes nowhere (FEEDBACK21.md, Q-172; branch feedback20-fixes).
                if let (false, Expr::Html(_, line)) = (last, e) {
                    self.line = *line;
                    self.warn("this element is not on the page: only the last one in a body is; wrap them in one element");
                }
                if last {
                    t
                } else {
                    T::of(NIL)
                }
            }
            Stmt::Let(p, e, line) => {
                self.line = *line;
                // `let 1 = 2`, `let [a, b] = [1]`: it fails every time it runs (FEEDBACK45.md,
                // Q-516; branch feedback40-43).
                if self.literal_mismatch(p, e) {
                    self.error("this `let` never matches the value it is given, so it fails every time it runs");
                }
                self.capital_let(p, e);
                let making = matches!((p, e), (Pattern::Bind(_), Expr::Lambda(_)));
                if let (true, Pattern::Bind(n)) = (making, p) {
                    self.letting.push(n.to_string());
                }
                let t = self.expr(e);
                if making {
                    self.letting.pop();
                }
                if let (Pattern::Bind(n), Expr::Call(f, _, _)) = (p, e) {
                    if matches!(&**f, Expr::Ident(s, _) if s == "__site") {
                        self.sites.insert(n.to_string());
                    }
                }
                if let (Pattern::Bind(n), Some(c)) = (p, self.fns.last().and_then(|f| f.component.clone())) {
                    let mut from_page = false;
                    walk_expr(e, &mut |x| {
                        from_page |= matches!(x, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "js" || n == "js_new"));
                    });
                    if from_page && !matches!(e, Expr::Lambda(_)) {
                        self.page_lets.push((c, n.to_string()));
                    }
                }
                let mut binds = Vec::new();
                self.bind_pattern(p, t, &mut binds);
                self.keep_component(p, e, &mut binds);
                self.keep_lambda(p, e, &mut binds);
                let sql = if matches!(p, Pattern::Bind(_)) { self.sql_kind(e) } else { SqlVar::Other };
                let url = matches!(p, Pattern::Bind(_)) && self.url_fixed(e);
                for (n, t, sig) in binds {
                    let kind = if matches!(p, Pattern::Bind(_)) { Kind::Let } else { Kind::Bound };
                    let t = if self.assigned.contains(&n) {
                        let (file, line) = (self.file.clone(), self.line);
                        self.note_first_t(&file, line, &n, t);
                        T::ANY
                    } else {
                        t
                    };
                    self.next_sql = sql;
                    self.next_url = url;
                    self.define(&n, t, sig, kind);
                }
                self.next_sql = SqlVar::Other;
                self.next_url = false;
                T::of(NIL)
            }
            // `el.value = x`: a field of one of the page's objects, which only
            // a component has; a map's fields are made anew, not set.
            Stmt::SetField(o, name, _, e, line) => {
                self.line = *line;
                let t = self.expr(o);
                self.expr(e);
                // A function's parameter of no known type may be a page object a
                // component hands it (`paint(ctx)` setting `ctx.fillStyle`): the
                // browser sets it, and the server says so if it is ever run there
                // (FEEDBACK12.md, Q-085; branch feedback12-fixes).
                let helper = t.vague() && !self.fns.is_empty();
                if matches!(o, Expr::Ident(n, _) if n == "session") {
                    // In a route nothing here is about a page (FEEDBACK26.md, Q-201;
                    // branch feedback26-fixes).
                    self.error(format!("cannot set `{}` on `session`: use `set_session({{ {}: ... }})`", name, name));
                } else if self.in_component().is_none() && !helper {
                    self.error(format!("cannot set `{}`: only the page's objects (an element, an event) have fields that are set, in a component", name));
                } else if !t.vague() {
                    if t.bits & MAP != 0 {
                        self.error(format!("a map's fields are not set in place: write `m = set(m, \"{}\", …)`", name));
                    } else {
                        self.error(format!("{} has no field `{}` to set: only the page's objects (an element, an event) do", t_text(t), name));
                    }
                }
                T::of(NIL)
            }
            Stmt::Assign(name, op, e, line) => {
                self.line = *line;
                let rhs = self.expr(e);
                // `q = ...` and `q += ...`: what the text holds now.
                let was = self.sql_var(name);
                let now = match op {
                    AssignOp::Set => Some(self.sql_kind(e)),
                    AssignOp::Add if matches!(was, SqlVar::Safe | SqlVar::Built) && !self.sql_safe(e) => Some(SqlVar::Built),
                    _ => None,
                };
                if let Some(k) = now {
                    if let Some(v) = self.sql_vars.iter_mut().rev().find(|(_, n, _)| n == name) {
                        v.2 = k;
                    }
                }
                let url = matches!(op, AssignOp::Set) && self.url_fixed(e);
                if let Some(v) = self.url_vars.iter_mut().rev().find(|(_, n, _)| n == name) {
                    v.2 = url;
                }
                let found = self.scopes.iter().rev().find_map(|s| s.get(name.as_str()).cloned()).or_else(|| self.globals.get(name.as_str()).cloned());
                match found {
                    None => self.error(format!("cannot assign to `{}`: it was never declared (use `let {} = …`)", name, name)),
                    Some(v) => {
                        if v.kind == Kind::Global && &*v.file == "<builtin>" {
                            self.error(format!("cannot assign to `{}`: it is a builtin", name));
                        }
                        // Writing another file's global takes it as reading does (FEEDBACK19.md,
                        // Q-154; branch feedback19-fixes).
                        if self.quiet == 0 {
                            self.taken(name, &v);
                        }
                        if v.kind == Kind::Global && self.in_component().is_some() && v.sig.is_none() {
                            self.browser_global(name, &v);
                        }
                        // The one way found for a customer's data to reach another with no
                        // mistake in the SQL. A map keyed by `tenant()` is what the guide says
                        // to do (FEEDBACK20.md, Q-166; branch feedback20-fixes).
                        let keyed = matches!(e, Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if n == "set")
                            && matches!(args.get(1), Some(Expr::Call(k, _, _)) if matches!(&**k, Expr::Ident(n, _) if n == "tenant")));
                        if self.tenants && v.kind == Kind::Global && !self.fns.is_empty() && self.in_component().is_none() && !keyed {
                            self.warn(format!("`{}` is one copy for every tenant: keep a tenant's data in its database, or in a map keyed by tenant()", name));
                        }
                        if v.kind == Kind::State {
                            let key = (v.file.clone(), v.line, name.to_string());
                            if let Some(e) = self.state_types.get_mut(&key) {
                                let now = match op {
                                    AssignOp::Set => rhs,
                                    AssignOp::Add => binop_t(BinOp::Add, e.0, rhs).unwrap_or(rhs),
                                    _ => binop_t(BinOp::Sub, e.0, rhs).unwrap_or(rhs),
                                };
                                e.0 = e.0.union(now);
                            }
                        }
                        if !matches!(op, AssignOp::Set) {
                            let bop = if matches!(op, AssignOp::Add) { BinOp::Add } else { BinOp::Sub };
                            // Reassigned, it is any: by `+=` and `-=` alone, it is still its
                            // first value's kind (Q-525).
                            let left = self.first_t.get(&(v.file.clone(), v.line, name.to_string())).copied().filter(|_| v.t.vague()).unwrap_or(v.t);
                            if binop_t(bop, left, rhs).is_none() {
                                self.error(format!(
                                    "cannot {} {} and {}",
                                    if matches!(op, AssignOp::Add) { "add" } else { "subtract" },
                                    bits_text(left.bits),
                                    bits_text(rhs.bits)
                                ));
                            }
                        }
                        if v.kind != Kind::Global {
                            for i in (0..self.scopes.len()).rev() {
                                if let Some(x) = self.scopes[i].get_mut(name.as_str()) {
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
                    self.twice(&d.name, Kind::Fn);
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
            Stmt::Use(_, _, line, _) => {
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
            Stmt::Service(name, _, _, opts, line) => {
                self.line = *line;
                if !self.scopes.is_empty() {
                    self.error("`service` must be at the top level");
                }
                // The options the loader takes, known here too (FEEDBACK9.md,
                // Q-072; branch feedback9-fixes).
                const OPTIONS: [&str; 6] = ["timeout", "retries", "failures", "cooldown", "auth", "forward"];
                match opts {
                    Some(Expr::Map(fields)) => {
                        for (k, v) in fields {
                            if !OPTIONS.contains(&k.as_str()) {
                                let near = OPTIONS.iter().map(|o| (edit_distance(k, o), *o)).filter(|(d, _)| *d <= 2).min();
                                let hint = near.map_or_else(|| " (timeout, retries, failures, cooldown, auth, forward)".to_string(), |(_, o)| format!(" (did you mean `{}`?)", o));
                                self.error(format!("service {}: no option `{}`{}", name, k, hint));
                            }
                            // A key or a client secret written in the source is in every copy of it,
                            // and in its history (FEEDBACK38.md, Q-458; branch feedback36-39).
                            if k == "auth" {
                                let mut literal = None;
                                if let Expr::Map(a) = v {
                                    for (ak, av) in a {
                                        let inner: Vec<(&str, &Expr)> = match av {
                                            Expr::Map(m) if ak == "oauth" => m.iter().map(|(k, e)| (k.as_str(), e)).collect(),
                                            e => vec![(ak.as_str(), e)],
                                        };
                                        for (ik, ie) in inner {
                                            if ["key", "client_secret"].contains(&ik) && matches!(ie, Expr::Lit(Lit::Str(_)) | Expr::Template(_)) {
                                                literal = Some(ik.to_string());
                                            }
                                        }
                                    }
                                }
                                if let Some(what) = literal {
                                    self.warn(format!("service {}: its `{}` is written in the source, where every copy of the program has it: read it with env(\"...\")", name, what));
                                }
                            }
                            self.expr(v);
                            self.line = *line;
                        }
                    }
                    // The parser only keeps a map here.
                    _ => {}
                }
                T::of(NIL)
            }
            Stmt::Type(td) => {
                self.line = td.line;
                if !self.scopes.is_empty() {
                    self.error("`type` must be at the top level");
                }
                // A default must be of its field's type. It is made with each record, not as
                // top-level code runs, so it may read what is defined later (found in review).
                let sequential = std::mem::replace(&mut self.sequential, false);
                for (k, e) in &td.defaults {
                    let t = self.expr(e);
                    self.line = td.line;
                    if let Some((_, fty)) = td.ty.fields.as_deref().unwrap_or_default().iter().find(|(f, _)| f == k) {
                        let want = ty_t(fty);
                        if t.bits & !ERR != 0 && t.bits & want.bits & !ERR == 0 {
                            self.error(format!("the default of `{}` in `{}` is {}, which is not {}", k, td.name, t_text(t), fty.text()));
                        }
                    }
                }
                self.sequential = sequential;
                T::of(NIL)
            }
            Stmt::Return(e, line) => {
                self.line = *line;
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
                    f.state_init = true;
                }
                let is_live = matches!(e, Expr::Call(c, _, _) if matches!(&**c, Expr::Ident(n, _) if n == "live"));
                self.live_ok = is_live;
                let mut t = self.expr(e);
                self.live_ok = false;
                if let (Some(f), Some(n)) = (self.fns.last_mut(), saved) {
                    f.nested = n;
                    f.state_init = false;
                }
                // `live(load, ...)` holds what `load` gives.
                if let (true, Expr::Call(_, args, _)) = (is_live, e) {
                    let loader = if matches!(args.first(), Some(Expr::Map(_))) { args.get(1) } else { args.first() };
                    if let Some(Expr::Ident(f, _)) = loader {
                        let ret = self.scopes.iter().rev().find_map(|s| s.get(f.as_str()).cloned()).or_else(|| self.globals.get(f.as_str()).cloned()).and_then(|v| v.sig).and_then(|s| s.ret);
                        if let Some(r) = ret {
                            t = r;
                        }
                    }
                }
                let assigned = self.assigned.contains(name);
                self.twice(name, Kind::State);
                self.define(name, if assigned { T::ANY } else { t }, None, Kind::State);
                if self.quiet == 0 {
                    self.state_types.insert((self.file.clone(), *line, name.clone()), (t, Vec::new()));
                    // Its declaration hovers too.
                    let v = Var { t, sig: None, file: self.file.clone(), line: *line, kind: Kind::State, used: false, ready: true };
                    self.record(name, &v);
                }
                T::of(NIL)
            }
        }
    }

    /// The body of a lambda or local fn, checked where it is written.
    fn lambda_body(&mut self, d: &Arc<FnDef>) -> T {
        self.lambda_body_hinted(d, None)
    }

    fn lambda_body_hinted(&mut self, d: &Arc<FnDef>, hint: Option<Vec<T>>) -> T {
        let (outer_allowed, outer_restrictor, component) = match self.fns.last() {
            Some(f) => (f.allowed, f.restrictor.clone(), f.component.clone()),
            None => (crate::policy::ceiling(), None, None),
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
            ret: d.ret.as_ref().map(ty_t),
            err: d.err.as_ref().map(ty_bits),
            allowed,
            restrictor,
            component,
            nested: true,
            state_init: false,
            exits: T::NEVER,
        });
        let saved_seq = self.sequential;
        self.sequential = false;
        self.push();
        self.params_hinted(d, hint);
        let t = self.expr(&d.body);
        self.check_return_expr(&d.body, t);
        self.pop();
        self.sequential = saved_seq;
        let t = t.union(self.fns.pop().map_or(T::NEVER, |f| f.exits));
        self.fn_ended();
        let t = match &d.ret {
            Some(r) => ty_t(r).union(t.without(!ERR)),
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
        let saved_ids = std::mem::take(&mut self.scope_ids);
        let saved_fns = std::mem::take(&mut self.fns);
        let saved_line = self.line;
        let saved_seq = self.sequential;
        self.sequential = false;
        self.quiet += 1;
        self.fns.push(FnCtx {
            name: d.name.clone(),
            ret: None,
            err: None,
            allowed: crate::policy::ceiling(),
            restrictor: None,
            component: None,
            nested: false,
            state_init: false,
            exits: T::NEVER,
        });
        self.push();
        if d.server {
            self.define("request", request_t(), None, Kind::Bound);
        }
        self.params(d);
        let body = self.expr(&d.body);
        let body = body.union(self.fns.last().map_or(T::NEVER, |f| f.exits));
        self.scopes.clear();
        self.quiet -= 1;
        self.fns = saved_fns;
        self.scopes = saved_scopes;
        self.scope_ids = saved_ids;
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
                } else if MODULES.contains(&n.as_str()) {
                    // A module called as a function: `audit(...)`.
                    mask |= builtin_effect(&n);
                }
            } else if let Some((m, f)) = n.split_once('.') {
                if MODULES.contains(&m) {
                    mask |= builtin_effect(&format!("{}.{}", m, f));
                // A site's function too: a request to it (FEEDBACK51.md, Q-640; branch
                // feedback51-fixes).
                } else if (self.services.contains_key(m) || self.sites.contains(m)) && !d.params.iter().any(|p| p.name == m) {
                    mask |= crate::ast::effect_bit("http");
                } else if self.sql_var(m) == SqlVar::Conn && !d.params.iter().any(|p| p.name == m) {
                    // A connection's call, as `db`'s (Q-488).
                    mask |= builtin_effect(&format!("db.{}", f));
                }
            }
        }
        self.effect_cache.insert(key, mask);
        mask
    }

    /// A transaction with `retries` runs its function again after a
    /// conflict, and what it sent to other servers cannot be taken back.
    fn retried_effects(&mut self, name: &str, args: &[Expr]) {
        let [Expr::Map(opts), Expr::Lambda(f), ..] = args else { return };
        if !opts.iter().any(|(k, v)| k == "retries" && !matches!(v, Expr::Lit(Lit::Int(0)))) {
            return;
        }
        let used = self.fn_effects(&Sig::of_def(f)) & (crate::ast::effect_bit("http") | crate::ast::effect_bit("net"));
        if used != 0 {
            self.warn(format!(
                "the function given to `{}` with retries uses the {} effect: it runs again when the transaction conflicts, \
                 and what it sent is not taken back; send it after the transaction",
                name,
                effect_names(used).join(" and ")
            ));
        }
    }

    /// `a.query(...)` on a connection from `db.connect` needs the db effect, as `db.query` does;
    /// on an untyped parameter that may be one, a warning (FEEDBACK43.md, Q-488; branch
    /// feedback40-43).
    fn connection_effects(&mut self, obj: &Expr, name: &str, recv: T) {
        let needs = builtin_effect(&format!("db.{}", name));
        if needs == 0 {
            return;
        }
        let (allowed, restrictor) = match self.fns.last() {
            Some(f) => (f.allowed, f.restrictor.clone()),
            None => return,
        };
        if needs & !allowed == 0 {
            return;
        }
        match obj {
            Expr::Ident(m, _) if self.sql_var(m) == SqlVar::Conn => self.effect_error(&format!("{}.{}", m, name), needs & !allowed, restrictor.as_ref()),
            Expr::Ident(m, _) if self.is_local(m) && recv.bits == T::ANY.bits && matches!(name, "query" | "one" | "exec" | "exec_many" | "transaction") => {
                if let Some((f, decl)) = &restrictor {
                    self.warn(format!(
                        "`{}.{}` reads like a database call, which needs the db effect, but `{}` is declared `with {}`: if `{}` is a connection, add db",
                        m, name, f, decl.join(", "), m
                    ));
                }
            }
            _ => {}
        }
    }

    fn effect_error(&mut self, what: &str, missing: u8, restrictor: Option<&(String, Vec<String>)>) {
        let names = effect_names(missing);
        let need = format!("the {} effect{}", names.join(", "), if names.len() == 1 { "" } else { "s" });
        // An effect the `with` names but the policy took away is the policy's to explain
        // (branch feedback40-43).
        match restrictor.filter(|(_, decl)| effect_mask(decl) & missing == 0) {
            Some((f, decl)) => self.error(format!("`{}` needs {}, but `{}` is declared `with {}`", what, need, f, decl.join(", "))),
            None if crate::policy::ceiling() != ALL_EFFECTS => self.error(format!("`{}` needs {}, which {} does not allow", what, need, crate::policy::get().effects_source())),
            None => self.error(format!("`{}` needs {}, which is not allowed here", what, need)),
        }
    }

    // ---- patterns ------------------------------------------------------

    /// Names a pattern binds, with their types.
    /// `let make = Dead`, where `Dead` is a component: `make` is it too, so `make()` is said
    /// to be a call of a component, which shows and never answers (FEEDBACK28.md, Q-345;
    /// branch feedback29-fixes).
    fn keep_component(&mut self, p: &Pattern, e: &Expr, binds: &mut [(String, T, Option<Arc<Sig>>)]) {
        let (Pattern::Bind(_), Expr::Ident(m, _), [(_, _, sig)]) = (p, e, binds) else { return };
        if let Some(s) = self.lookup(m).and_then(|v| v.sig).filter(|s| s.component) {
            *sig = Some(s);
        }
    }

    /// `Usr` in a pattern, with no `type Usr`: a test that never matches (FEEDBACK45.md, Q-504;
    /// branch feedback40-43). Inside lists, records and alternatives too.
    fn pattern_types(&mut self, p: &Pattern) {
        match p {
            Pattern::Type(n) if n.starts_with(|c: char| c.is_ascii_uppercase()) => {
                let known = RECORDS.with(|r| r.borrow().contains_key(n)) || self.globals.contains_key(n.as_str());
                if !known {
                    let records: Vec<String> = RECORDS.with(|r| r.borrow().keys().cloned().collect());
                    let near = closest(n, records.iter().map(|s| s.as_str()).chain(crate::parser::TYPE_NAMES.iter().copied()));
                    self.error(format!("there is no type `{}`, so this never matches{}", n, near.map_or(String::new(), |m| format!(" (did you mean `{}`?)", m))));
                }
            }
            Pattern::Alt(ps) | Pattern::List(ps, _) => ps.iter().for_each(|q| self.pattern_types(q)),
            Pattern::As(q, _) => self.pattern_types(q),
            Pattern::Record(fs) => fs.iter().for_each(|f| self.pattern_types(&f.pat)),
            _ => {}
        }
    }

    /// A record's field whose pattern is a name near a type's: `{ page: itn }` (Q-504).
    fn field_type_typos(&mut self, p: &Pattern) {
        if let Pattern::Record(fs) = p {
            for f in fs {
                match &f.pat {
                    Pattern::Bind(n) if f.path.last().map_or(true, |l| **n != **l) => {
                        if let Some(ty) = type_typo(n) {
                            self.warn(format!("`{}` here is a name, which takes any value: for the type, `{}`", n, ty));
                        }
                    }
                    q => self.field_type_typos(q),
                }
            }
        }
    }

    /// A literal pattern against a literal of another value, or a list pattern against a list
    /// of another length: it never matches, and the checker knows both sides (FEEDBACK45.md,
    /// Q-516; branch feedback40-43).
    fn literal_mismatch(&self, p: &Pattern, e: &Expr) -> bool {
        let number = |l: &Lit| match l {
            Lit::Int(n) => Some(*n as f64),
            Lit::Float(f) => Some(*f),
            Lit::Dec(d) => d.to_string().parse::<f64>().ok(),
            _ => None,
        };
        match (p, e) {
            // `1` matches `1.0`: numbers by their value (found in review).
            (Pattern::Lit(a), Expr::Lit(b)) if number(a).is_some() && number(b).is_some() => number(a) != number(b),
            (Pattern::Lit(a), Expr::Lit(b)) => format!("{:?}", a) != format!("{:?}", b),
            (Pattern::List(items, None), Expr::List(xs)) => items.len() != xs.len(),
            (Pattern::List(..) | Pattern::Record(_), Expr::Lit(_)) => true,
            (Pattern::Type(t), Expr::Lit(l)) if crate::parser::TYPE_NAMES.contains(&t.as_str()) => !matches!(
                (t.as_str(), l),
                ("int" | "num", Lit::Int(_)) | ("float" | "num", Lit::Float(_)) | ("dec" | "num", Lit::Dec(_)) | ("str", Lit::Str(_)) | ("bool", Lit::Bool(_)) | ("any", _)
            ),
            (Pattern::Record(_), Expr::List(_)) | (Pattern::List(..), Expr::Map(_)) => true,
            _ => false,
        }
    }

    /// `f()` from top-level code, where `f` reads a global of this file defined further down:
    /// the call fails, with "unknown name", which reads as a typo (FEEDBACK45.md, Q-514; branch
    /// feedback40-43). What `f` reads itself, not what the functions it calls read.
    fn reads_later(&mut self, name: &str) {
        let Some(d) = self.bodies.get(name).cloned() else { return };
        let mut bound: HashSet<String> = d.params.iter().map(|p| p.name.to_string()).collect();
        let mut read: Vec<String> = Vec::new();
        sure_reads(&d.body, &mut bound, &mut read);
        for n in read {
            if bound.contains(&n) || n == name {
                continue;
            }
            if let Some(v) = self.globals.get(&n).filter(|v| v.kind == Kind::Global && !v.ready && v.file == self.file) {
                let at = v.line;
                self.error(format!("`{}` reads `{}`, which is defined at line {}, after this call", name, n, at));
                return;
            }
        }
    }

    /// `let f = (a, b) => a + b`: `f(1)` is checked as a `fn`'s call is, with the name `f`
    /// (FEEDBACK45.md, Q-515; branch feedback40-43).
    fn keep_lambda(&mut self, p: &Pattern, e: &Expr, binds: &mut [(String, T, Option<Arc<Sig>>)]) {
        let (Pattern::Bind(n), Expr::Lambda(d), [(_, _, sig)]) = (p, e, binds) else { return };
        // `h = (a, b) => ...` later: no longer the first lambda (found in review).
        if sig.is_none() && !self.assigned.contains(&**n) {
            let mut s = Sig::of_def(d);
            s.name = n.to_string();
            *sig = Some(Arc::new(s));
        }
    }

    /// `let Tag = "h1"`, for React's `<Tag>`: a capital makes it a type in the pattern, and the
    /// `let` fails when it runs (FEEDBACK48.md, Q-577; branch main).
    fn capital_let(&mut self, p: &Pattern, e: &Expr) {
        let Pattern::Type(n) = p else { return };
        if !n.starts_with(|c: char| c.is_uppercase()) || record(n).is_some() {
            return;
        }
        let tag = if matches!(e, Expr::Lit(Lit::Str(_))) { "; and a tag's name is written out, as there are no dynamic tags: choose between them with `match`" } else { "" };
        let mut lower = n.clone();
        if let Some(f) = lower.get_mut(0..1) {
            f.make_ascii_lowercase();
        }
        self.error(format!("`let {} = ...` reads `{}` as a type, as a capitalised name in a pattern is one, so it fails when it runs: name the variable in lower case, `let {} = ...`{}", n, n, lower, tag));
    }

    /// A lambda given to `map`, `filter`, `reduce`, `sort_by` and the like takes the
    /// arguments the builtin passes: one item, or the total and an item for `reduce`
    /// (FEEDBACK45.md, Q-515; branch feedback40-43).
    /// `xs.map(f)` too, whose `args` start after `xs` (`skipped` 1), where only `map(xs, f)` was
    /// checked (FEEDBACK48.md, Q-574; branch main).
    fn lambda_arity(&mut self, name: &str, args: &[Expr], skipped: usize) {
        let (at, passes) = match name {
            "map" | "filter" | "find" | "any" | "all" | "sort_by" | "group_by" | "count" => (1, 1),
            "reduce" => (2, 2),
            _ => return,
        };
        let Some(Expr::Lambda(d)) = args.get(at - skipped) else { return };
        let most = d.params.len();
        let least = d.params.iter().filter(|p| p.default.is_none()).count();
        if passes >= least && passes <= most {
            return;
        }
        if name == "sort_by" && most == 2 {
            self.error("`sort_by` passes one item, not two to compare: give what to sort by, `sort_by(xs, (x) => x.n)`");
        } else if name == "reduce" && most == 1 {
            self.error("`reduce` passes the total so far and an item: `reduce(xs, 0, (total, x) => total + x)`");
        } else {
            let what = if passes == 1 { "one item" } else { "the total so far and an item" };
            let takes = if least == most { format!("{}", most) } else { format!("{} to {}", least, most) };
            // JavaScript's `(x, i)`: how to have the index (Q-574).
            let index = if passes == 1 && most == 2 { ": for the index, `for [i, x] in enumerate(xs) => ...`, which gives a list too".to_string() } else { String::new() };
            self.error(format!("`{}` passes its function {}, but this one takes {}{}", name, what, takes, index));
        }
    }

    fn bind_pattern(&mut self, p: &Pattern, t: T, out: &mut Vec<(String, T, Option<Arc<Sig>>)>) {
        match p {
            Pattern::Wild | Pattern::Lit(_) => {}
            Pattern::Bind(n) => out.push((n.to_string(), t, None)),
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
                    let dt = f.default.as_ref().map(|d| self.expr(d));
                    if f.pat.is_type_test() {
                        ft = type_test_t(&f.pat);
                        // A missing or null field takes the default, so it
                        // is null only if the default may be.
                        if let Some(dt) = dt {
                            ft = ft.union(dt);
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
        // The parser refuses code that nests deeper; a guard all the same.
        if self.depth >= 2 * crate::parser::MAX_NESTING {
            self.error("the code nests too deeply");
            return T::ANY;
        }
        self.depth += 1;
        let t = crate::stack::maybe_grow(256 << 10, 4 << 20, || self.expr_here(e));
        self.depth -= 1;
        t
    }

    fn expr_here(&mut self, e: &Expr) -> T {
        match e {
            Expr::Lit(l) => {
                if let Lit::Str(s) = l {
                    self.plain_dollar(s);
                }
                lit_t(l)
            }
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
                let mut elem = T::NEVER;
                for i in items {
                    elem = elem.union(self.expr(i));
                }
                if elem.bits == 0 { T::of(LIST) } else { list_of(elem) }
            }
            Expr::Map(entries) => {
                let mut fields: Vec<(Arc<str>, T)> = Vec::new();
                for (k, v) in entries {
                    let t = self.expr(v);
                    // A key written twice: the last value is kept, with nothing said, however each
                    // was written (`a`, `"a"`, `{ a, a: 2 }`). A spread between them is no map
                    // literal's key: the parser makes a map of each part (FEEDBACK55.md, Q-794;
                    // branch feedback55).
                    if fields.iter().any(|(n, _)| &**n == k) {
                        self.warn(format!("the key `{}` is written twice in this map: the last value is the one kept", k));
                    }
                    fields.retain(|(n, _)| &**n != k);
                    fields.push((Arc::from(k.as_str()), t));
                }
                // `{}` is a map things are added to: what it holds is not known.
                if fields.is_empty() { T::of(MAP) } else { map_of(fields) }
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
                        if t.bits != 0 && t.bits & (NUM | DEC) == 0 {
                            self.error(format!("cannot negate {}", bits_text(t.bits)));
                        }
                        T::of(t.bits & (NUM | DEC))
                    }
                }
            }
            Expr::Binary(op, a, b, line) => {
                if matches!(op, BinOp::Eq | BinOp::Ne) {
                    self.type_name_compared(a, b);
                }
                let x = self.expr(a);
                // `x != null && f(x)`: the right side runs only if the left holds.
                let mut facts = Vec::new();
                match op {
                    BinOp::And => self.facts(a, true, &mut facts),
                    BinOp::Or => self.facts(a, false, &mut facts),
                    _ => {}
                }
                let maybe = matches!(op, BinOp::And | BinOp::Or);
                let y = if maybe { self.on_path(Step::Maybe, |s| s.under(facts, |c| c.expr(b))) } else { self.under(facts, |c| c.expr(b)) };
                self.line = *line;
                self.js_comparisons(op, a, b, x, y);
                self.line = *line;
                match op {
                    BinOp::Or => self.js_truthiness(fallback_says(x, b), a, Truth::Or),
                    BinOp::And => self.js_truthiness(x, a, Truth::And),
                    _ => {}
                }
                if matches!(op, BinOp::Div | BinOp::Mod) && matches!(**b, Expr::Lit(Lit::Int(0))) {
                    self.error("division by zero");
                }
                // Two money values add and subtract their amounts, not merge as maps do
                // (FEEDBACK50.md, Q-620; branch feedback50-fixes).
                if matches!(op, BinOp::Add | BinOp::Sub) && is_money(x) && is_money(y) {
                    return money_t().union(T::error(STR));
                }
                match binop_t(*op, x, y) {
                    Some(t) => {
                        if self.strict_null {
                            self.null_operands(*op, x, y);
                        }
                        match op {
                            BinOp::Add if x.bits == LIST && y.bits == LIST => x.union(y),
                            BinOp::Add if x.bits == MAP && y.bits == MAP => merge_maps(x, y),
                            BinOp::Range => list_of(T::of(INT)),
                            _ => t,
                        }
                    }
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
                        self.error(format!("cannot {} {} {} {}{}", verb, bits_text(x.bits), join, bits_text(y.bits), dec_hint(x, y)));
                        T::ANY
                    }
                }
            }
            Expr::Coalesce(a, b) => {
                let x = self.expr(a);
                let y = self.on_path(Step::Maybe, |s| s.expr(b));
                x.without(NIL | ERR).union(y)
            }
            Expr::Pipe(lhs, rhs, line) => {
                let v = self.expr(lhs);
                self.line = *line;
                // `a && xs | any(f)` reads as `(a && xs) | any(f)`, which works while
                // `a` holds, the case tried first (FEEDBACK25.md, Q-319; branch feedback26-fixes).
                if let Expr::Binary(op @ (BinOp::And | BinOp::Or), _, _, _) = &**lhs {
                    let sym = if matches!(op, BinOp::And) { "&&" } else { "||" };
                    self.warn(format!("`a {sym} xs | f()` reads as `(a {sym} xs) | f()`, as `|` binds looser than `{sym}`: write `a {sym} (xs | f())`"));
                }
                // `` `select ... ${q}` | db.query ``: the text is the SQL.
                let target = match &**rhs {
                    Expr::Call(callee, _, _) => self.sql_target(callee),
                    other => self.sql_target(other),
                };
                if let Some(name) = target {
                    if self.sql_built(lhs) {
                        self.warn_sql(&name);
                    }
                }
                match &**rhs {
                    // `xs | find(f) != null` is `xs | (find(f) != null)`: `|`
                    // binds looser than an operator, which a call's errors
                    // would not say (FEEDBACK5.md, Q-051; branch feedback6-fixes).
                    Expr::Binary(op, inner, _, _) if matches!(&**inner, Expr::Call(..) | Expr::Ident(..)) && !matches!(op, BinOp::Range) => {
                        let f = match &**inner {
                            Expr::Call(c, _, _) => callee_name(c),
                            other => callee_name(other),
                        };
                        let sym = match op {
                            BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%",
                            BinOp::Eq => "==", BinOp::Ne => "!=", BinOp::Lt => "<", BinOp::Le => "<=", BinOp::Gt => ">",
                            BinOp::Ge => ">=", BinOp::And => "&&", BinOp::Or => "||", BinOp::Range => "..",
                        };
                        self.error(format!(
                            "`xs | {f}(...) {sym} ...` reads as `xs | ({f}(...) {sym} ...)`: `|` binds looser than `{sym}`, so put the pipe in parentheses: `(xs | {f}(...)) {sym} ...`"
                        ));
                        T::ANY
                    }
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
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if self.sequential && self.fns.is_empty() && !self.is_local(name) {
                        self.reads_later(name);
                    }
                }
                self.upload_named_by_visitor(callee, args);
                self.interim_status(callee, args);
                self.cookie_options(callee, args);
                self.jobs_as_started(callee, args);
                self.helper_jobs(callee);
                self.topics_written(callee, args);
                if let Expr::Field(o, f, _) = callee.as_ref() {
                    if let Expr::Ident(s, _) = o.as_ref() {
                        if self.sites.contains(s.as_str()) && !self.is_local(s) {
                            if let Some(c) = self.in_component() {
                                self.error(format!("`{s}.{f}()` calls the site from the server, and <{c}> also runs in the browser: call it in a `server fn`"));
                            }
                            // A request to the site, as a service's call is (FEEDBACK51.md,
                            // Q-640; branch feedback51-fixes).
                            let http = crate::ast::effect_bit("http");
                            if let Some((allowed, restrictor)) = self.fns.last().map(|x| (x.allowed, x.restrictor.clone())) {
                                if http & !allowed != 0 {
                                    self.effect_error(&format!("{}.{}", s, f), http, restrictor.as_ref());
                                }
                            }
                        }
                    }
                }
                // Children given none are null since 0.6's fixes, so `str(children) != ""` is
                // true (FEEDBACK28.md, Q-421; branch feedback27-35-fixes).
                if matches!(callee.as_ref(), Expr::Ident(n, _) if n == "str") && matches!(args.as_slice(), [Expr::Ident(c, _)] if c == "children") && !self.is_local("str") && self.is_local("children") {
                    self.warn("`children` is null when none are given, and `str(children)` is \"null\" then: ask `children != null`");
                }
                // `sleep` takes milliseconds; Python's takes seconds (FEEDBACK42.md, Q-482; branch
                // feedback40-43).
                if let (Expr::Ident(n, _), [Expr::Lit(Lit::Float(f))]) = (callee.as_ref(), args.as_slice()) {
                    let builtin = !self.is_local("sleep") && self.globals.get("sleep").map_or(false, |v| &*v.file == "<builtin>");
                    if n == "sleep" && builtin && *f > 0.0 && *f < 1.0 {
                        let ms = (f * 1000.0).round();
                        self.warn(format!("`sleep` takes milliseconds: `sleep({f})` waits {f} ms; for {f} seconds write `sleep({ms})`"));
                    }
                }
                // `file("public/" + path)` serves `.env` and `main.qwe` when the path is the
                // visitor's: `files` keeps them back (FEEDBACK51.md, Q-657; branch
                // feedback51-fixes).
                if let (Expr::Ident(n, _), Some(a)) = (callee.as_ref(), args.first()) {
                    let param = |e: &Expr| matches!(e, Expr::Ident(p, _) if self.path_params.iter().any(|q| q == &**p));
                    let made = match a {
                        Expr::Binary(BinOp::Add, l, r, _) => matches!(&**l, Expr::Lit(Lit::Str(_))) && param(r),
                        Expr::Template(parts) => parts.iter().any(|p| matches!(p, TPart::Expr(e) if param(e))),
                        _ => false,
                    };
                    let builtin = !self.is_local("file") && self.globals.get("file").map_or(false, |v| &*v.file == "<builtin>");
                    if n == "file" && made && builtin {
                        self.warn("`file` serves the one file it is given, whatever its name: with a path from the request, `files(\"public\", path)` keeps `.env`, `.git` and the program's own files back");
                    }
                }
                self.call(callee, args, None)
            }
            Expr::Field(obj, name, line) => {
                let t = self.expr(obj);
                self.line = *line;
                // In a component's body it reads as JavaScript's `?.` and gives the right
                // value, so nothing shows that it remakes the component on each change
                // (FEEDBACK25.md, Q-305; branch feedback26-fixes). `http.get(u)?.json`,
                // leaving when a call failed, is what `?` is for.
                if matches!(&**obj, Expr::Try(x) if matches!(&**x, Expr::Ident(..) | Expr::Field(..))) && self.in_component().is_some() {
                    self.warn(format!("`x?.{name}` is not optional chaining: `?` leaves on an error, and a component that can leave is remade on each change, which loses the focus of an input. A field of null is null, so write `x.{name}`"));
                }
                self.module_member(obj, name);
                self.field(t, obj, name)
            }
            Expr::Index(obj, idx, line) => {
                let o = self.expr(obj);
                let i = self.expr(idx);
                self.line = *line;
                let ok = [(LIST, INT), (MAP, STR), (MAP, INT), (STR, INT), (NIL, ANY)]
                    .iter()
                    .any(|(a, b)| o.bits & a != 0 && i.bits & b != 0);
                if !ok && o.bits != 0 && i.bits != 0 {
                    self.error(format!("cannot index {} with {}", bits_text(o.bits), bits_text(i.bits)));
                }
                // An item (as in TypeScript: not "or null" past the end, which
                // would make every `parts[1]` a warning under --strict-null).
                if let Some(e) = o.items().filter(|_| i.bits & !NUM == 0) {
                    return e.union(T::of(o.bits & NIL));
                }
                if let (Some(fs), Expr::Lit(Lit::Str(k))) = (o.fields(), &**idx) {
                    return fs.iter().find(|(n, _)| **n == **k).map_or(T::of(NIL), |(_, t)| t.union(T::of(o.bits & NIL)));
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
                let (mut yes, mut no) = (Vec::new(), Vec::new());
                self.facts(c, true, &mut yes);
                self.facts(c, false, &mut no);
                self.block_kind = matches!(&**a, Expr::Block(_)).then_some("if");
                let at = self.branch_point();
                let x = self.on_path(Step::Arm(at, 0), |s| s.under(yes, |s| s.expr(a)));
                let y = match b {
                    Some(b) => {
                        self.block_kind = matches!(&**b, Expr::Block(_)).then_some("if");
                        self.on_path(Step::Arm(at, 1), |s| s.under(no, |s| s.expr(b)))
                    }
                    None => T::of(NIL),
                };
                x.union(y)
            }
            Expr::Match(subj, arms, line) => {
                let mut st = self.expr(subj);
                self.line = *line;
                // `match x { }` matches nothing (FEEDBACK45.md, Q-516; branch feedback40-43).
                if arms.is_empty() {
                    self.error("this `match` has no arms, so nothing matches: add one, `_ => ...` for any value");
                }
                let mut t = T::NEVER;
                // An arm that takes any value (`_`, a name), and the literals seen, for those
                // after it that never match (FEEDBACK45.md, Q-527).
                let mut taken: Option<u32> = None;
                let mut seen_lits: Vec<String> = Vec::new();
                let at = self.branch_point();
                for (n, arm) in arms.iter().enumerate() {
                    self.line = arm.line;
                    if let Some(at) = taken {
                        self.warn(format!("this arm never matches: the one on line {} takes any value", at));
                    } else if let (Pattern::Lit(l), None) = (&arm.pat, &arm.guard) {
                        let key = format!("{:?}", l);
                        if seen_lits.contains(&key) {
                            self.warn("this arm never matches: an arm before it has the same value");
                        }
                        seen_lits.push(key);
                    }
                    if matches!(arm.pat, Pattern::Wild | Pattern::Bind(_)) && arm.guard.is_none() {
                        taken.get_or_insert(arm.line);
                    }
                    self.pattern_types(&arm.pat);
                    if self.literal_mismatch(&arm.pat, subj) {
                        self.warn("this arm never matches: the value it is given is another");
                    }
                    self.push();
                    let mut binds = Vec::new();
                    self.bind_pattern(&arm.pat, st, &mut binds);
                    // `null => ...` first: the arms after it never see null.
                    if matches!(arm.pat, Pattern::Lit(Lit::Nil)) && arm.guard.is_none() {
                        st = st.without(NIL);
                    }
                    for (n, bt, sig) in binds {
                        self.define(&n, bt, sig, Kind::Bound);
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    t = t.union(self.on_path(Step::Arm(at, n as u32), |s| s.expr(&arm.body)));
                    // `intt => ...`: a name, which takes any value, and goes unused; written for
                    // a type, most likely (FEEDBACK45.md, Q-504).
                    if let Pattern::Bind(n) = &arm.pat {
                        let unused = self.scopes.last().and_then(|s| s.get(&**n)).is_some_and(|v| !v.used);
                        if unused && !n.starts_with('_') && arm.guard.is_none() {
                            self.line = arm.line;
                            match type_typo(n) {
                                Some(ty) => self.warn(format!("`{n}` here is a name, which takes any value: for the type, `{ty}`")),
                                None => self.warn(format!("`{n}` takes any value and is not used: write `_` for that")),
                            }
                        }
                    }
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
                let item = match it.items() {
                    Some(e) => e,
                    None if it.bits & !(STR | NIL) == 0 && it.bits & STR != 0 => T::of(STR),
                    None => T::ANY,
                };
                let mut binds = Vec::new();
                self.bind_pattern(pat, item, &mut binds);
                for (n, bt, sig) in binds {
                    self.define(&n, bt, sig, Kind::Bound);
                }
                self.block_kind = matches!(&**body, Expr::Block(_)).then_some("for");
                self.on_path(Step::Loop, |s| s.expr(body));
                self.pop();
                T::of(LIST)
            }
            Expr::While(c, body, line) => {
                let ct = self.expr(c);
                self.line = *line;
                // Only what cannot end: with a call in it, a `reply` may be in that function
                // (FEEDBACK20.md, Q-163; branch feedback20-fixes).
                let body_text = format!("{:?}", body);
                if matches!(&**c, Expr::Lit(Lit::Bool(true))) && !["Call(", "Return(", "Reply("].iter().any(|w| body_text.contains(w)) {
                    self.warn("this `while` never ends: nothing in it can stop it, and it holds its instance (see --max-compute)");
                } else if !format!("{:?}", c).contains("Call(") && !["Return(", "Reply("].iter().any(|w| body_text.contains(w)) {
                    // `while i < 3 { i + 1 }`: nothing in it assigns what the condition reads.
                    // A call in it may assign a global, so then only locals count (FEEDBACK45.md,
                    // Q-505; branch feedback40-43).
                    let mut read = Vec::new();
                    walk_expr(c, &mut |x| {
                        if let Expr::Ident(n, _) = x {
                            read.push(n.to_string());
                        }
                    });
                    let mut set = HashSet::new();
                    assigned_in(body, &mut set);
                    // A call may set what it reads (a closure, a handler, a page's
                    // `w.firstChild` while `parent.insertBefore(w.firstChild, w)` runs): let be,
                    // as `while true` with a call is (found in review). Only names there are: an
                    // unknown one is said as that.
                    let calls = body_text.contains("Call(");
                    let known = read.iter().all(|n| self.is_local(n) || self.globals.contains_key(n.as_str()));
                    if known && !read.is_empty() && !calls && read.iter().all(|n| !set.contains(n)) {
                        self.warn(format!("this `while` never ends: nothing in it changes `{}`, which its condition reads", read[0]));
                    }
                }
                self.js_truthiness(ct, c, Truth::Cond);
                let mut yes = Vec::new();
                self.facts(c, true, &mut yes);
                self.block_kind = matches!(&**body, Expr::Block(_)).then_some("while");
                self.on_path(Step::Loop, |s| s.under(yes, |s| s.expr(body)));
                // It leaves only by a `return`: nothing comes after it (FEEDBACK25.md,
                // Q-322; branch feedback26-fixes).
                if matches!(&**c, Expr::Lit(Lit::Bool(true))) {
                    return T::NEVER;
                }
                T::of(NIL)
            }
            Expr::Block(stmts) => {
                let kind = self.block_kind.take();
                self.push();
                let mut t = T::of(NIL);
                let mut never = false;
                for (i, s) in stmts.iter().enumerate() {
                    if i + 1 < stmts.len() {
                        self.answer_dropped(s);
                        self.op_dropped(s);
                        self.redirect_dropped(s);
                    }
                    let st = self.stmt(s, i + 1 == stmts.len());
                    if i + 1 < stmts.len() {
                        self.error_dropped(s, st);
                    }
                    if matches!(s, Stmt::Return(..)) || (st.bits == 0 && matches!(s, Stmt::Expr(_))) {
                        never = true;
                    }
                    // `if x == null { return ... }`: after it, `x` is not null.
                    if let Stmt::Expr(Expr::If(c, a, None)) = s {
                        if exits(a) {
                            let mut no = Vec::new();
                            self.facts(c, false, &mut no);
                            self.assume(no);
                        }
                    }
                    if i + 1 == stmts.len() {
                        t = st;
                    }
                }
                if let (Some(kind), 0) = (kind, self.quiet) {
                    let lets: Vec<(String, u32)> = self.scopes.last().map_or(Vec::new(), |s| {
                        s.iter().filter(|(_, v)| v.kind == Kind::Let).map(|(n, v)| (n.clone(), v.line)).collect()
                    });
                    for (name, line) in lets {
                        self.gone.push(Gone { name, file: self.file.clone(), line, kind, fns: self.fns.len() });
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
                self.try_at_the_end(d);
                let hint = self.hint.take();
                self.lambda_body_hinted(d, hint);
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
                    if !(200..=599).contains(n) {
                        self.error(format!("reply status must be an HTTP status code (200-599), got {}", n));
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
                self.try_at_the_end(d);
                self.lambda_body(d);
                (T::of(FN), Some(Arc::new(Sig::of_def(d))))
            }
            other => (self.expr(other), None),
        }
    }

    /// `map(xs, (x) => f(x)?)`: `?` leaves the lambda, which was ending anyway, so the
    /// error is one more item and what is around it goes on (FEEDBACK26.md, Q-198;
    /// branch feedback26-fixes).
    fn try_at_the_end(&mut self, d: &FnDef) {
        let last = match &d.body {
            Expr::Block(stmts) => match stmts.last() {
                Some(Stmt::Expr(e)) => e,
                _ => return,
            },
            e => e,
        };
        if matches!(last, Expr::Try(_)) {
            self.warn("`?` here changes nothing: it leaves this function, which was ending anyway, so what called it goes on with the error as a value. To stop at the first failure, use a `for` loop, where `?` leaves the function around it".to_string());
        }
    }

    /// `xs.push(3)` alone on a line changes nothing, as values do not change: it reads as
    /// JavaScript's call (FEEDBACK28.md, Q-347; branch feedback29-fixes).
    /// `xs.push(3)` alone: the builtin's answer, the new list, dropped (FEEDBACK28.md, Q-347;
    /// branch feedback29-fixes). Only the builtin's, on what is known to be a list or a map:
    /// the program's own `set` and a page's object's `held.set(k, v)` change what they hold.
    /// `clas="a"`, `hreff="/x"`, `@clik={...}`, `className`: written to the page and ignored by
    /// the browser, or a handler never called (FEEDBACK45.md, Q-509; branch feedback40-43).
    /// Said only when near a known name: a custom attribute or a library's event is let be.
    fn attr_typo(&mut self, tag: &str, name: &str) {
        const ATTRS: &[&str] = &[
            "class", "id", "href", "src", "srcset", "srcdoc", "sizes", "alt", "title", "style", "type", "name", "value", "placeholder", "disabled",
            "checked", "selected", "action", "method", "target", "rel", "for", "width", "height", "colspan", "rowspan", "lang", "dir", "hidden",
            "tabindex", "role", "min", "max", "step", "pattern", "required", "readonly", "multiple", "autofocus", "autocomplete", "accept",
            "download", "loading", "label", "rows", "cols", "maxlength", "minlength", "content", "charset", "async", "defer", "crossorigin",
            "integrity", "open", "controls", "autoplay", "loop", "muted", "poster", "preload", "enctype", "novalidate", "form", "list", "size",
            "wrap", "spellcheck", "draggable", "contenteditable", "key", "headers", "scope", "span", "start", "reversed", "media", "datetime",
            "cite", "coords", "shape", "usemap", "frameborder", "allow", "allowfullscreen", "sandbox", "referrerpolicy", "nonce", "inputmode",
            "enterkeyhint", "popover", "popovertarget", "inert", "translate", "accesskey", "kind", "srclang", "default", "high", "low", "optimum",
            "decoding", "fetchpriority", "formaction", "formmethod", "playsinline", "capture", "dirname", "ping", "hreflang", "itemprop",
            "itemscope", "itemtype", "slot", "part", "is", "xmlns", "viewBox", "fill", "stroke", "stroke-width", "transform", "points", "path",
            "cx", "cy", "rx", "ry", "x1", "x2", "y1", "y2", "opacity", "offset", "stop-color", "gradientUnits", "preserveAspectRatio", "bind",
            "value", "summary", "valign", "align", "border", "color", "face", "bgcolor", "manifest", "http-equiv", "property", "rev",
            "data", "volume", "currentTime", "playbackRate", "indeterminate", "values", "codebase", "archive", "declare", "standby",
            "frame", "rules", "axis", "abbr", "char", "charoff", "nowrap", "compact", "noshade", "background", "text", "link", "vlink", "alink",
        ];
        // SVG has more attributes than this list could hold (`refX`, `markerWidth`, ...).
        const SVG: &[&str] = &[
            "svg", "g", "path", "circle", "ellipse", "line", "polyline", "polygon", "rect", "text", "tspan", "textPath", "use", "symbol",
            "defs", "marker", "mask", "pattern", "clipPath", "linearGradient", "radialGradient", "stop", "image", "foreignObject", "filter",
            "animate", "animateTransform", "animateMotion", "set", "desc", "metadata", "switch", "view",
        ];
        // React's own names, which reach the page and do nothing there (FEEDBACK48.md, Q-570;
        // branch main).
        let react = match name {
            "defaultValue" => Some("write `value`: a field with no state keeps what is typed".to_string()),
            "defaultChecked" => Some("write `checked`".to_string()),
            "dangerouslySetInnerHTML" => Some("put {raw(html)} between the element's tags".to_string()),
            "ref" => Some("write `@mount={(el) => ...}`, which is given the element".to_string()),
            _ => None,
        };
        if let Some(how) = react {
            self.warn(format!("`{}` is React's, and does nothing in a page: {}", name, how));
            return;
        }
        // And on SVG, React's camelCase for its hyphenated attributes: the page lowercases
        // `strokeWidth` to `strokewidth`, which SVG does not know.
        const SVG_CAMEL: &[&str] = &[
            "strokeWidth", "strokeLinecap", "strokeLinejoin", "strokeDasharray", "strokeDashoffset", "strokeOpacity", "strokeMiterlimit",
            "fillRule", "fillOpacity", "clipRule", "clipPath", "stopColor", "stopOpacity", "fontSize", "fontFamily", "fontWeight", "textAnchor",
            "dominantBaseline", "floodColor", "floodOpacity", "lightingColor", "markerStart", "markerMid", "markerEnd", "vectorEffect",
            "shapeRendering", "colorInterpolationFilters", "letterSpacing", "wordSpacing", "textDecoration", "paintOrder", "imageRendering",
        ];
        if (SVG.contains(&tag) || tag.starts_with("fe")) && SVG_CAMEL.contains(&name) {
            let svg: String = name.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['-', c.to_ascii_lowercase()] } else { vec![c] }).collect();
            self.warn(format!("`{}` is React's: SVG's attribute is `{}`", name, svg));
            return;
        }
        if (SVG.contains(&tag) || tag.starts_with("fe")) && name == "xlinkHref" {
            self.warn("`xlinkHref` is React's: SVG's attribute is `href` (or `xlink:href`)");
            return;
        }
        // SVG's filters (`feTurbulence`'s `seed`, `scale`), and HTML's text handlers
        // (`onload="..."`), framework attributes (`x-data`, `hx-get`): found in review.
        if (SVG.contains(&tag) || tag.starts_with("fe")) && !name.starts_with('@') {
            return;
        }
        if name.starts_with("on") || name.contains('-') || name == "ismap" {
            return;
        }
        const EVENTS: &[&str] = &[
            "click", "dblclick", "input", "change", "submit", "reset", "keydown", "keyup", "keypress", "focus", "blur", "focusin", "focusout",
            "mousedown", "mouseup", "mousemove", "mouseover", "mouseout", "mouseenter", "mouseleave", "pointerdown", "pointerup", "pointermove",
            "pointerover", "pointerout", "pointerenter", "pointerleave", "pointercancel", "touchstart", "touchend", "touchmove", "touchcancel",
            "scroll", "wheel", "contextmenu", "drag", "dragstart", "dragend", "dragenter", "dragleave", "dragover", "drop", "load", "error",
            "resize", "select", "toggle", "close", "cancel", "play", "pause", "ended", "timeupdate", "volumechange", "ratechange", "seeked",
            "seeking", "loadeddata", "loadedmetadata", "canplay", "playing", "waiting", "copy", "cut", "paste", "animationend", "animationstart",
            "transitionend", "invalid", "beforeinput", "compositionstart", "compositionend", "mount", "auxclick", "gotpointercapture",
            "lostpointercapture", "beforetoggle", "durationchange", "progress", "stalled", "suspend", "emptied", "abort", "search", "show",
        ];
        if tag.contains('-') || name.starts_with("data-") || name.starts_with("aria-") || name.contains(':') {
            return;
        }
        if name == "className" {
            self.warn("`className` is React's name: write `class`");
            return;
        }
        if name == "htmlFor" {
            self.warn("`htmlFor` is React's name: write `for`");
            return;
        }
        let (word, list, prefix) = match name.strip_prefix('@') {
            Some(ev) => (ev, EVENTS, "@"),
            None => (name, ATTRS, ""),
        };
        // `@mouseEnter`: the browser's event names are lower case, and compared as written
        // (FEEDBACK48.md, Q-561; branch main).
        if prefix == "@" && word != word.to_ascii_lowercase() && list.contains(&word.to_ascii_lowercase().as_str()) {
            self.warn(format!("`@{}` never fires: the browser's event names are lower case, `@{}` (or React's `on{}`)", word, word.to_ascii_lowercase(), {
                let mut c = word.to_string();
                if let Some(f) = c.get_mut(0..1) {
                    f.make_ascii_uppercase();
                }
                c
            }));
            return;
        }
        if word.chars().count() < 4 || list.contains(&word) || list.iter().any(|a| a.eq_ignore_ascii_case(word)) {
            return;
        }
        // `@changed`, `@shown`, `@unload`: an event made from a known one, as libraries name
        // theirs (found in review); `hreff` stays a slip of `href`.
        let derived = |n: &str| prefix == "@" && (word == format!("{}d", n) || word == format!("{}ed", n) || word == format!("{}n", n) || ["un", "before", "after", "pre", "post"].iter().any(|p| word == format!("{}{}", p, n)));
        if let Some(near) = closest(word, list.iter().copied()).filter(|n| !derived(n)) {
            self.warn(format!("`{prefix}{word}` is no {} the browser knows: did you mean `{prefix}{near}`?", if prefix.is_empty() { "attribute" } else { "event" }));
        }
    }

    /// `if no_user { redirect("/login") }` with more after it: the answer is made, nothing takes
    /// it, and the code goes on past a check written to stop it, as two of app/'s did
    /// (FEEDBACK55.md, Q-877; branch feedback55). `reply 303 redirect(...)` leaves.
    fn redirect_dropped(&mut self, s: &Stmt) {
        fn answers(e: &Expr) -> Vec<(&str, u32)> {
            match e {
                Expr::Call(f, _, line) => match &**f {
                    Expr::Ident(n, _) if n == "redirect" || n == "respond" => vec![(n.as_str(), *line)],
                    _ => Vec::new(),
                },
                Expr::Block(stmts) => match stmts.last() {
                    Some(Stmt::Expr(e)) => answers(e),
                    _ => Vec::new(),
                },
                Expr::If(_, a, b) => answers(a).into_iter().chain(b.iter().flat_map(|b| answers(b))).collect(),
                Expr::Match(_, arms, _) => arms.iter().flat_map(|a| answers(&a.body)).collect(),
                _ => Vec::new(),
            }
        }
        let Stmt::Expr(e) = s else { return };
        for (name, line) in answers(e) {
            if self.is_local(name) || !self.globals.get(name).is_some_and(|v| &*v.file == "<builtin>") {
                continue;
            }
            self.line = line;
            let leave = if name == "redirect" { "reply 303 redirect(...)" } else { "return respond(...)" };
            self.warn(format!("this `{name}(...)` is an answer nothing takes, and the code goes on past it: to leave here, write `{leave}`"));
        }
    }

    /// `i + 1`, `x == 2` alone: worked out and dropped, where an assignment was meant
    /// (FEEDBACK45.md, Q-505; branch feedback40-43).
    fn op_dropped(&mut self, s: &Stmt) {
        let Stmt::Expr(Expr::Binary(op, l, _, line)) = s else { return };
        let Expr::Ident(n, _) = &**l else { return };
        self.line = *line;
        match op {
            BinOp::Eq => self.warn(format!("`{n} == ...` compares, and the answer is dropped: to give `{n}` a value, `{n} = ...`")),
            BinOp::Add => self.warn(format!("`{n} + ...` is worked out and dropped: to change `{n}`, `{n} += ...`")),
            BinOp::Sub => self.warn(format!("`{n} - ...` is worked out and dropped: to change `{n}`, `{n} -= ...`")),
            BinOp::Mul | BinOp::Div => self.warn(format!("this is worked out and dropped: to change `{n}`, `{n} = {n} * ...` (or `/`)")),
            _ => {}
        }
    }

    fn answer_dropped(&mut self, s: &Stmt) {
        let Stmt::Expr(e) = s else { return };
        // What it was given, by name if it was a name: `xs` of `xs.push(2)`, `push(xs, 2)` and
        // `xs | push(2)`, for the message.
        let (name, on, given, line) = match e {
            Expr::Call(f, args, line) => match &**f {
                Expr::Field(o, name, _) => match &**o {
                    Expr::Ident(o, _) => (name.as_str(), Some(o.as_str()), Some(o.to_string()), *line),
                    _ => return,
                },
                Expr::Ident(name, _) if !args.is_empty() => {
                    let first = match &args[0] {
                        Expr::Ident(n, _) => Some(n.to_string()),
                        _ => None,
                    };
                    (name.as_str(), None, first, *line)
                }
                _ => return,
            },
            Expr::Pipe(l, r, line) => match (&**l, &**r) {
                (Expr::Ident(n, _), Expr::Call(f, ..)) => match &**f {
                    Expr::Ident(name, _) => (name.as_str(), None, Some(n.to_string()), *line),
                    _ => return,
                },
                _ => return,
            },
            _ => return,
        };
        self.line = line;
        // Builtins that give a new value and do nothing else; not `map` or `filter`, whose
        // function may be the work (FEEDBACK45.md, Q-505; branch feedback40-43).
        if !matches!(name, "push" | "set" | "merge" | "remove" | "sort" | "sort_by" | "drop" | "take" | "reverse" | "unique" | "insert" | "append" | "concat" | "flatten")
            || self.is_local(name)
        {
            return;
        }
        if !self.globals.get(name).is_some_and(|v| &*v.file == "<builtin>") {
            return;
        }
        if let Some(o) = on {
            let t = self.scopes.iter().rev().find_map(|s| s.get(o)).or_else(|| self.globals.get(o)).map(|v| v.t);
            if !t.is_some_and(|t| t.bits != ANY && t.bits & (LIST | MAP) != 0 && t.bits & !(LIST | MAP | NIL) == 0) {
                return;
            }
        }
        let x = given.unwrap_or_else(|| "x".to_string());
        self.warn(format!("`{}` gives a new value and changes nothing, and it is dropped here: write `{} = {} | {}(...)`", name, x, x, name));
    }

    /// `save_upload(f, f.filename)`, or a path ending in the visitor's name with no folder of the
    /// program's before it: a visitor can then name the file `main.qwe` or `.env`, which are
    /// refused, or any other at the folder's root (FEEDBACK30.md, Q-361; branch feedback27-35-fixes).
    /// `respond(103, ...)`: a 1xx is an interim answer, after which the client waits for the
    /// real one, which never comes (FEEDBACK36.md, Q-433; branch feedback36-39).
    fn interim_status(&mut self, callee: &Expr, args: &[Expr]) {
        if !matches!(callee, Expr::Ident(n, _) if n == "respond") || self.is_local("respond") {
            return;
        }
        if let Some(Expr::Lit(Lit::Int(n))) = args.first() {
            if (100..200).contains(n) {
                self.error(format!("`respond` status must be 200-599, got {}: a 1xx is an interim answer, and the client would wait for the real one", n));
            }
        }
    }

    /// `set_cookie("t", v, { sameSite: "Strict" })`: an option written out that it does not take,
    /// which it refuses when it runs, said before (FEEDBACK55.md, Q-712; branch feedback55).
    fn cookie_options(&mut self, callee: &Expr, args: &[Expr]) {
        let (f, at) = match callee {
            Expr::Ident(n, _) if n == "set_cookie" => ("set_cookie", 2),
            Expr::Ident(n, _) if n == "delete_cookie" => ("delete_cookie", 1),
            _ => return,
        };
        if self.is_local(f) {
            return;
        }
        if let Some(Expr::Map(entries)) = args.get(at) {
            for (k, v) in entries {
                // A value not written out may be 0, which a deletion's `max_age` may be.
                let zero = !matches!(v, Expr::Lit(_)) || matches!(v, Expr::Lit(Lit::Int(0) | Lit::Nil));
                if let Some(why) = crate::session::option_refused(f, k, zero) {
                    self.error(why);
                }
            }
        }
    }

    /// `every` and `schedule` read as the program's start reads them: what it refuses is said
    /// before (`every(0, f)`, a cron field of 60, `@reboot`, a bad `{ name }`, a job's name
    /// twice), and so is either called in a route or a `server fn`, where what it adds would
    /// never run (FEEDBACK55.md, Q-803; branch feedback55).
    fn jobs_as_started(&mut self, callee: &Expr, args: &[Expr]) {
        use crate::value::Value;
        let f = match callee {
            Expr::Ident(n, _) if n == "every" || n == "schedule" => n.as_str(),
            _ => return,
        };
        if self.quiet_mode() || self.is_local(f) || !self.globals.get(f).is_some_and(|v| &*v.file == "<builtin>") {
            return;
        }
        let every = f == "every";
        // A component's `every` is the browser's.
        if every && self.in_component().is_some() {
            return;
        }
        let outer = self.fns.first().map(|c| c.name.clone());
        if let Some(name) = outer {
            // A route's frame is named for its method and path; a `server fn` by its name.
            if name.contains(' ') || self.bodies.get(&name).is_some_and(|d| d.server) {
                self.error(crate::jobs::not_at_top(f));
            }
        }
        let lit = |e: &Expr| -> Option<Value> {
            Some(match e {
                Expr::Lit(Lit::Int(n)) => Value::Int(*n),
                Expr::Lit(Lit::Float(x)) => Value::Float(*x),
                Expr::Lit(Lit::Str(s)) => Value::Str(s.clone()),
                Expr::Lit(Lit::Bool(b)) => Value::Bool(*b),
                Expr::Lit(Lit::Nil) => Value::Nil,
                Expr::Unary(UnOp::Neg, x, _) => match &**x {
                    Expr::Lit(Lit::Int(n)) => Value::Int(n.wrapping_neg()),
                    Expr::Lit(Lit::Float(x)) => Value::Float(-x),
                    _ => return None,
                },
                _ => return None,
            })
        };
        if let Some(v) = args.first().and_then(lit) {
            let refused = if every { crate::jobs::every_ms(&v).err() } else { crate::jobs::when_of(&v).err() };
            if let Some(why) = refused {
                self.error(why);
            }
        }
        // The options, when what they hold is written out.
        let named = match args.get(2) {
            None => Ok(None),
            Some(Expr::Map(entries)) => match entries.iter().find(|(k, _)| k == "name") {
                None => Ok(None),
                Some((_, v)) => match lit(v) {
                    Some(v) => crate::jobs::name_opt(f, Some(&Value::map([("name".into(), v)].into_iter().collect()))),
                    None => return,
                },
            },
            // `null`, or what the types' check says of.
            Some(Expr::Lit(Lit::Nil)) => Ok(None),
            Some(_) => return,
        };
        let named = match named {
            Ok(n) => n,
            Err(why) => return self.error(why),
        };
        let name = match (named, args.get(1)) {
            (Some(n), _) => n,
            // Named after its function, as the start names it.
            (None, Some(Expr::Ident(n, _))) if !self.is_local(n) => match self.bodies.get(n.as_str()) {
                Some(d) if crate::jobs::name_ok(&d.name) => d.name.clone(),
                Some(_) => format!("{}-{}", if every { "every" } else { "job" }, self.line),
                None => return,
            },
            (None, Some(Expr::Lambda(_))) => format!("{}-{}", if every { "every" } else { "job" }, self.line),
            _ => return,
        };
        if let Some(why) = crate::jobs::name_problem(f, &name) {
            return self.error(why);
        }
        // Twice as the top level runs, which the start refuses (found in review, and in the second
        // review): an error where both surely run (on the top level's own path, not in a branch, a
        // loop or after `&&`), as two on one line do; a warning where both may (two `if`s, a loop
        // of one round), as the start then refuses only a second add that happens. Two arms of
        // one `if` or `match` never both run.
        if self.fns.is_empty() {
            let path = self.path.clone();
            self.job_seen(f, every, name, args.as_ptr() as usize, path);
        }
    }

    /// Notes tick or job `name` added by the call at `call` on `path`, and says if it is twice.
    fn job_seen(&mut self, f: &str, every: bool, name: String, call: usize, path: Vec<Step>) {
        let seen = self.job_names.entry((every, name.clone())).or_default();
        if seen.iter().any(|j| j.call == call) {
            return;
        }
        let sure = |p: &[Step]| p.is_empty();
        let looped = path.contains(&Step::Loop);
        let surely_twice = sure(&path) && seen.iter().any(|j| sure(&j.path));
        let other = seen.iter().any(|j| both_run(&j.path, &path));
        seen.push(JobSeen { call, path });
        let what = if every { "tick" } else { "job" };
        if surely_twice {
            self.error(crate::jobs::named_twice(f, &name));
        } else if other {
            self.warn(format!("`{f}`: a {what} named {name:?} is added before too: if both run, the start refuses the second (give one another with {{ name: ... }})"));
        } else if looped {
            self.warn(format!("`{f}`: a {what} named {name:?} in a loop is added each time round: from the second, the start refuses it (give each another with {{ name: ... }})"));
        }
    }

    /// A top-level function called at the top level: the ticks and jobs it adds, as if added here
    /// (`fn setup() { every(1000, tick) }` called twice adds `tick` twice, which the start refused
    /// and `check` passed; found in the second review). One under a branch of the function's may
    /// run or not.
    fn helper_jobs(&mut self, callee: &Expr) {
        let Expr::Ident(n, _) = callee else { return };
        if !self.fns.is_empty() || self.is_local(n) || self.quiet_mode() {
            return;
        }
        let Some(d) = self.bodies.get(n.as_str()).cloned() else { return };
        if d.server || d.component {
            return;
        }
        let mut found: Vec<(bool, String, bool, usize, bool)> = Vec::new();
        let mut sure_stmts: HashSet<usize> = HashSet::new();
        // The calls written as the function's own statements surely run when it does.
        if let Expr::Block(stmts) = &d.body {
            for st in stmts.iter() {
                if let Stmt::Expr(Expr::Call(_, args, _)) = st {
                    sure_stmts.insert(args.as_ptr() as usize);
                }
            }
        } else if let Expr::Call(_, args, _) = &d.body {
            sure_stmts.insert(args.as_ptr() as usize);
        }
        walk_expr(&d.body, &mut |x| {
            let Expr::Call(c, args, _) = x else { return };
            let Expr::Ident(g, _) = &**c else { return };
            if g != "every" && g != "schedule" {
                return;
            }
            let every = g == "every";
            let named = match args.get(2) {
                Some(Expr::Map(entries)) => match entries.iter().find(|(k, _)| k == "name") {
                    Some((_, Expr::Lit(Lit::Str(s)))) => Some(s.to_string()),
                    Some(_) => return,
                    None => None,
                },
                None | Some(Expr::Lit(Lit::Nil)) => None,
                Some(_) => return,
            };
            let (name, written) = match (named, args.get(1)) {
                (Some(n), _) => (n, true),
                (None, Some(Expr::Ident(f, _))) => (f.to_string(), false),
                _ => return,
            };
            let key = args.as_ptr() as usize;
            found.push((every, name, written, key, sure_stmts.contains(&key)));
        });
        for (every, name, written, key, sure) in found {
            // Named after a top-level function, as the start names it, or written out.
            if !crate::jobs::name_ok(&name) || !written && !self.bodies.get(name.as_str()).is_some_and(|b| b.name == name) {
                continue;
            }
            let mut path = self.path.clone();
            if !sure {
                path.push(Step::Maybe);
            }
            // Each call of the function adds them again: the call's place, with the job's.
            let f = if every { "every" } else { "schedule" };
            let call = key ^ (callee as *const Expr as usize).rotate_left(17);
            self.job_seen(f, every, name, call, path);
        }
    }

    /// `watch("")`, or a topic past the longest, written out: refused as it runs (FEEDBACK55.md,
    /// Q-801; branch feedback55).
    fn topics_written(&mut self, callee: &Expr, args: &[Expr]) {
        let f = match callee {
            Expr::Ident(n, _) if n == "watch" || n == "publish" => n.as_str(),
            _ => return,
        };
        if self.quiet_mode() || self.is_local(f) || !self.globals.get(f).is_some_and(|v| &*v.file == "<builtin>") {
            return;
        }
        for a in args {
            if let Expr::Lit(Lit::Str(t)) = a {
                if let Some(why) = crate::shared::topic_problem(f, t) {
                    self.error(why);
                }
            }
        }
    }

    fn upload_named_by_visitor(&mut self, callee: &Expr, args: &[Expr]) {
        if !matches!(callee, Expr::Ident(n, _) if n == "save_upload") || self.is_local("save_upload") {
            return;
        }
        let is_filename = |e: &Expr| matches!(e, Expr::Field(_, n, _) if n == "filename");
        let bare = match args.get(1) {
            Some(e) if is_filename(e) => true,
            Some(Expr::Template(parts)) => match parts.as_slice() {
                [TPart::Expr(e)] => is_filename(e),
                [TPart::Expr(_), TPart::Lit(l), TPart::Expr(e)] if l == "/" => is_filename(e),
                _ => false,
            },
            _ => false,
        };
        if bare {
            self.warn("the file is saved under the name the visitor gave, where they choose it: put it in a folder of the program's, `save_upload(f, `uploads/${uuid()}.pdf`)`, and keep their name in the database");
        }
    }

    /// A call alone on a line whose error would be lost: a `db.transaction`, whose value is
    /// the only word that it rolled back, or a function of the program's that can give one
    /// (FEEDBACK27.md, Q-332; branch feedback27-35-fixes). A builtin's dropped error is logged
    /// when it happens, so `db.exec(...)` alone, the usual way to set a table up, is let be.
    fn error_dropped(&mut self, s: &Stmt, t: T) {
        let Stmt::Expr(Expr::Call(f, _, _)) = s else { return };
        let name = match &**f {
            Expr::Field(o, name, _) if matches!(&**o, Expr::Ident(db, _) if db == "db") && name == "transaction" => {
                self.warn("the transaction's value is dropped, so a rollback is not seen: write `db.transaction(...)?`, or `let r = db.transaction(...)`");
                return;
            }
            Expr::Ident(name, _) => name.as_str(),
            _ => return,
        };
        if self.is_local(name) {
            return;
        }
        // It can fail when its type says so, or it declares `! T`, or its body makes an error or
        // passes one on with `?`.
        let can_fail = |d: &FnDef| {
            let mut found = d.err.is_some();
            walk_expr(&d.body, &mut |e| {
                found |= matches!(e, Expr::Try(_)) || matches!(e, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "err"));
            });
            found
        };
        let fails = (t.bits != ANY && t.bits & ERR != 0) || self.globals.get(name).and_then(|v| v.sig.as_ref()).and_then(|s| s.def.as_ref()).is_some_and(|d| can_fail(d));
        if !fails {
            return;
        }
        if self.globals.get(name).is_some_and(|v| &*v.file != "<builtin>" && v.sig.as_ref().is_some_and(|s| !s.builtin && !s.component)) {
            self.warn(format!("`{}` can give an error, and it is dropped here: write `{}(...)?` to pass it on, or `let _ = {}(...)` if it does not matter", name, name, name));
        }
    }

    /// `type_of(x) == "decimal"` is never true, and nothing says so (FEEDBACK28.md, Q-347;
    /// branch feedback29-fixes).
    fn type_name_compared(&mut self, a: &Expr, b: &Expr) {
        for (c, l) in [(a, b), (b, a)] {
            let (Expr::Call(f, _, _), Expr::Lit(Lit::Str(t))) = (c, l) else { continue };
            if !matches!(&**f, Expr::Ident(n, _) if n == "type_of") {
                continue;
            }
            const NAMES: &[&str] = &["int", "float", "dec", "str", "bool", "list", "map", "fn", "html", "response", "null", "error", "promise", "element", "event", "object"];
            if !NAMES.contains(&&**t) && !RECORDS.with(|r| r.borrow().contains_key(&**t)) {
                self.warn(format!("`type_of` never gives {:?}: it gives int, float, dec, str, bool, list, map, fn, html, response, null or error, or a record's type", &**t));
            }
        }
    }

    /// `re.test(s, "(a+)+$")`: the server's engine takes this in its stride, and the
    /// browser's backtracks, doubling with each character, with no way to stop it
    /// (FEEDBACK25.md, Q-315; branch feedback26-fixes).
    fn pattern_runs_in_runs(&mut self, callee: &Expr, args: &[Expr]) {
        let Expr::Field(m, _, _) = callee else { return };
        if !matches!(&**m, Expr::Ident(n, _) if n == "re") {
            return;
        }
        for a in args {
            if matches!(a, Expr::Lit(Lit::Str(p)) if runs_in_runs(p)) {
                self.warn("this pattern repeats something that repeats, as `(a+)+`: on the wrong text it can hang a browser tab, with no way to stop it. Bound the counts (`{1,8}`), or run it in a `server fn`".to_string());
            }
            if matches!(a, Expr::Lit(Lit::Str(p)) if capture_in_repeat(p)) {
                self.warn("a group inside a group that repeats, as `(?:(a)|b)*`, holds its last match on the server and nothing in the browser, where each round clears it: capture the whole repetition instead, `((?:a|b)*)`".to_string());
            }
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], pre: Option<T>) -> T {
        self.pattern_runs_in_runs(callee, args);
        let line = self.line;
        // `User({ ... })`: a field the record does not have is a mistake.
        if let (Expr::Ident(n, _), Some(Expr::Map(entries)), None) = (callee, args.first(), &pre) {
            if let Some(td) = record(n).filter(|td| td.is_record() && !self.is_local(n)) {
                let fs = td.ty.fields.as_deref().unwrap_or_default();
                let shape: Shape = fs.iter().map(|(k, ft)| (Arc::from(k.as_str()), ty_t(ft))).collect::<Vec<_>>().into();
                for (k, _) in entries {
                    if !k.starts_with("...") && !fs.iter().any(|(f, _)| f == k) {
                        self.error(format!("`{}` has no field `{}`{}", n, k, field_hint(k, &shape)));
                    }
                }
            }
        }
        // `js(name, ...)`: the name is written in the code, as loading the
        // program requires (FEEDBACK18.md, Q-100; branch feedback18-fixes).
        if let (Expr::Ident(n, _), None) = (callee, &pre) {
            if (n == "js" || n == "js_new") && !self.is_local(n) && self.globals.get(n.as_str()).map_or(false, |v| &*v.file == "<builtin>") {
                match args.first() {
                    Some(Expr::Lit(Lit::Str(path))) => {
                        let ok = !path.is_empty() && path.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'));
                        if !ok {
                            self.error(format!("`{}`: {:?} is not a dotted name such as \"L.map\"", n, path));
                        } else if let Some(why) = crate::client::js_path_problem(n, path) {
                            // What `quillo run` refuses, refused here too (FEEDBACK28.md, Q-347;
                            // branch feedback29-fixes).
                            self.error(why);
                        } else if let Some(m @ ("bind" | "call" | "apply")) = path.rsplit_once('.').map(|(_, m)| m) {
                            self.warn(format!("`{}(\"{}\")` is most likely Function's `{}`, which Quillo code does not call, and the handler stops there: give the function itself, or wrap it in a lambda", n, path, m));
                        }
                    }
                    Some(_) => self.error(format!("`{}` needs the name of what it calls written out, like {}(\"Chart\", ...)", n, n)),
                    None => {}
                }
            }
        }
        // `x.f(args)`: a function stored in map x, or `f(x, args)`.
        if let Expr::Field(obj, name, _) = callee {
            // `billing.later.invoice(args)`: the same call, made later; its id.
            if let Expr::Field(inner, later, _) = &**obj {
                if let (Expr::Ident(m, _), "later") = (&**inner, later.as_str()) {
                    if self.services.contains_key(m.as_str()) && !self.is_local(m) {
                        self.service_call(m, name, args, pre);
                        return T::of(STR);
                    }
                }
            }
            if let Expr::Ident(m, _) = &**obj {
                if self.services.contains_key(m.as_str()) && !self.is_local(m) {
                    return self.service_call(m, name, args, pre);
                }
                if MODULES.contains(&m.as_str()) && !self.is_local(m) {
                    // A top-level variable named like a module takes it from
                    // every file (FEEDBACK9.md, Q-069; branch feedback9-fixes).
                    let hidden = self.globals.get(m.as_str()).filter(|g| &*g.file != "<builtin>" && matches!(g.kind, Kind::Global)).map(|g| (g.file.clone(), g.line, g.t.bits));
                    // A map of the program's own (`let stats = { load: ... }`)
                    // is what `stats.load()` means: its function is called.
                    let own_map = matches!(hidden, Some((_, _, bits)) if bits & MAP != 0);
                    if let Some((file, at, _)) = hidden.filter(|_| !own_map) {
                        let made = self.shown_at(&file, at);
                        let here = self.shown_at(&self.file.clone(), line);
                        self.error(format!(
                            "`{}.{}`: `{}` is the program's variable from {}, which hides the module `{}` in every file: rename the variable",
                            m, name, m, made, m
                        ));
                        self.diag_at(file, at, Severity::Warning, format!(
                            "`{}` hides the module `{}` in every file of the program, and {} calls `{}.{}`: pick another name",
                            m, m, here, m, name
                        ));
                        return T::ANY;
                    }
                    if !own_map {
                        return self.module_call(m, name, args, pre);
                    }
                }
            }
            // `conn.query(sql)` on a connection from `db.connect`.
            if pre.is_none() {
                if let (Some(what), Some(sql)) = (self.sql_target(callee), args.first()) {
                    if what != format!("db.{}", name) && self.sql_built(sql) {
                        self.warn_sql(&what);
                    }
                    if what != format!("db.{}", name) {
                        self.sql_params(&what, args);
                    }
                }
            }
            let recv = self.expr(obj);
            self.line = line;
            self.connection_effects(obj, name, recv);
            // `let json = ...` in a function, then `json.stringify(...)`: the
            // variable, not the module, whose method this is not (Q-069).
            if let Expr::Ident(m, _) = &**obj {
                if MODULES.contains(&m.as_str()) && recv.bits != 0 && recv.bits & MAP == 0 {
                    self.error(format!(
                        "`{}.{}`: `{}` here is {}, a variable, which hides the module `{}`: rename the variable",
                        m, name, m, bits_text(recv.bits), m
                    ));
                    return T::ANY;
                }
            }
            // `error` is a builtin too, but `x.error` on what cannot fail is a mistake.
        let found = if name == "error" { None } else { self.method(name) };
            let sig = found.as_ref().and_then(|v| v.sig.clone()).filter(|_| recv.bits & MAP == 0);
            let mut all = vec![recv];
            all.extend(pre);
            for a in args {
                self.hint = lambda_hint(sig.as_deref(), &all, a);
                all.push(self.expr(a));
                self.hint = None;
            }
            self.line = line;
            if recv.bits & MAP != 0 {
                // May be a function stored in the map: nothing more to know.
                if let Some(v) = found {
                    self.record(name, &v);
                }
                return T::ANY;
            }
            match found {
                Some(v) if v.t.bits & FN != 0 => {
                    self.record(name, &v);
                    if pre.is_none() && v.sig.as_ref().is_some_and(|s| s.builtin) {
                        self.lambda_arity(name, args, 1);
                    }
                    return self.apply(v.t, v.sig.clone(), all, args, name);
                }
                _ => {
                    if recv.bits != 0 {
                        // `s.toUpperCase()`: the builtin, called on it (Q-517).
                        let near = other_language(name)
                            .map(str::to_string)
                            // One letter off or two swapped (`uper`), not `foo` for `floor`.
                            .or_else(|| closest(name, self.globals.iter().filter(|(_, v)| &*v.file == "<builtin>" && v.t.bits & FN != 0).map(|(k, _)| k.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter()).filter(|n| osa_distance(name, n) == 1))
                            .map_or(String::new(), |n| format!(" (did you mean `{}`?)", n));
                        self.error(format!("{} has no method `{}`{}", bits_text(recv.bits), name, near));
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
            self.hint = lambda_hint(sig.as_deref(), &argts, a);
            argts.push(self.expr(a));
            self.hint = None;
        }
        self.line = line;
        self.apply(ft, sig, argts, args, &callee_name(callee))
    }

    // ---- narrowing --------------------------------------------------------

    /// What a condition says about variables where it holds (`yes`) or
    /// does not: `x != null` takes null out of `x`, `x == null` leaves only
    /// null, `x` alone takes out null and errors. Only variables whose type
    /// is known and never reassigned.
    fn facts(&self, c: &Expr, yes: bool, out: &mut Vec<(String, T)>) {
        let known = |n: &str| {
            let v = self.scopes.iter().rev().find_map(|s| s.get(n)).or_else(|| self.globals.get(n))?;
            (!v.t.vague() && v.t.bits & NIL != 0 && !self.assigned.contains(n)).then_some(v.t)
        };
        match c {
            Expr::Binary(op @ (BinOp::Eq | BinOp::Ne), a, b, _) => {
                let name = match (&**a, &**b) {
                    (Expr::Ident(n, _), Expr::Lit(Lit::Nil)) | (Expr::Lit(Lit::Nil), Expr::Ident(n, _)) => n,
                    _ => return,
                };
                if let Some(t) = known(name) {
                    let null = matches!(op, BinOp::Eq) == yes;
                    out.push((name.clone().to_string(), if null { T::of(NIL) } else { t.without(NIL) }));
                }
            }
            Expr::Ident(n, _) if yes => {
                if let Some(t) = known(n) {
                    out.push((n.to_string(), t.without(NIL | ERR)));
                }
            }
            Expr::Unary(UnOp::Not, x, _) => self.facts(x, !yes, out),
            Expr::Binary(BinOp::And, a, b, _) if yes => {
                self.facts(a, true, out);
                self.facts(b, true, out);
            }
            Expr::Binary(BinOp::Or, a, b, _) if !yes => {
                self.facts(a, false, out);
                self.facts(b, false, out);
            }
            _ => {}
        }
    }

    /// Takes facts as true in the innermost scope: a variable there changes
    /// type, one further out is copied in (the same definition, for the
    /// language server).
    fn assume(&mut self, facts: Vec<(String, T)>) {
        for (n, t) in facts {
            let found = self.scopes.iter().rev().find_map(|s| s.get(&n).cloned()).or_else(|| self.globals.get(&n).cloned());
            let (Some(v), Some(scope)) = (found, self.scopes.last_mut()) else { continue };
            match scope.get_mut(&n) {
                Some(x) => x.t = t,
                None => {
                    scope.insert(n, Var { t, used: true, ..v });
                }
            }
        }
    }

    /// Checks `f` where the facts hold.
    fn under(&mut self, facts: Vec<(String, T)>, f: impl FnOnce(&mut Self) -> T) -> T {
        if facts.is_empty() {
            return f(self);
        }
        self.push();
        self.assume(facts);
        let t = f(self);
        self.pop();
        t
    }

    /// `--strict-null`: an operand that may be null where null does not work
    /// (`count + 1` with `count: int | null`).
    fn null_operands(&mut self, op: BinOp, x: T, y: T) {
        if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::And | BinOp::Or) {
            return;
        }
        let maybe = |t: T| !t.vague() && t.bits & NIL != 0 && t.bits != NIL;
        if maybe(x) && binop_t(op, T::of(NIL), y.without(NIL)).is_none() {
            self.warn(format!("the left side may be null here (it is {})", t_text(x)));
        } else if maybe(y) && binop_t(op, x.without(NIL), T::of(NIL)).is_none() {
            self.warn(format!("the right side may be null here (it is {})", t_text(y)));
        }
    }

    /// What some builtins give back, from what they are given: `map` a list
    /// of what its function returns, `filter` a list like the one it got,
    /// `first` one of its items or null.
    fn builtin_ret(&mut self, name: &str, args: &[T], arg_exprs: &[Expr]) -> Option<T> {
        let a0 = *args.first()?;
        // A piped value is an argument without an expression of its own.
        let expr_at = |i: usize| arg_exprs.get(i.checked_sub(args.len() - arg_exprs.len())?);
        let lit_key = |i: usize| match expr_at(i) {
            Some(Expr::Lit(Lit::Str(k))) => Some(k.clone()),
            _ => None,
        };
        match name {
            "map" => {
                let r = match expr_at(1)? {
                    Expr::Lambda(d) => *self.ret_cache.get(&def_key(d))?,
                    _ => return None,
                };
                Some(list_of(r))
            }
            "filter" | "sort" | "sort_by" | "reverse" | "unique" | "take" | "drop" | "slice" => a0.items().map(|_| a0.without(!LIST)),
            "first" | "last" if expr_at(0).is_some_and(never_empty) => a0.items(),
            "find" | "first" | "last" => a0.items().map(|e| e.union(T::of(NIL))),
            "push" => Some(list_of(a0.items()?.union(*args.get(1)?))),
            "set" if a0.bits & LIST != 0 && a0.bits & (MAP | NIL) == 0 => Some(list_of(a0.items()?.union(*args.get(2)?))),
            "flatten" => a0.items()?.items().map(list_of),
            "split" | "keys" => Some(list_of(T::of(STR))),
            "range" => Some(list_of(T::of(INT))),
            "sum" => a0.items().filter(|e| e.bits != 0 && e.bits & !(NUM | DEC) == 0).map(|e| T::of(e.bits)),
            "min" | "max" => {
                let e = a0.items().filter(|e| e.bits != 0 && e.bits & !(NUM | DEC) == 0)?;
                Some(T::of(if expr_at(0).is_some_and(never_empty) { e.bits } else { e.bits | NIL }))
            }
            // Money is `{ amount: dec, currency: str }`.
            // An amount too large is an error value (FEEDBACK49.md, Q-605), so these may give
            // one (FEEDBACK50.md, Q-633; branch feedback50-fixes).
            "money.add" | "money.sub" | "money.mul" | "money.neg" | "money.round" => Some(money_t().union(T::error(STR))),
            "money.of" | "money.parse" | "money.from_minor" | "money.zero" => Some(money_t().union(T::error(STR))),
            "money.allocate" => Some(list_of(money_t())),
            // A decimal gives a decimal; ints and floats as before.
            "abs" | "floor" | "ceil" | "round" | "pow" if a0.bits == DEC => Some(T::of(DEC)),
            "abs" | "floor" | "ceil" | "round" | "pow" if a0.bits & DEC == 0 && a0.bits != 0 => Some(T::of(NUM)),
            "merge" => Some(merge_maps(a0, *args.get(1)?)),
            "set" if args.len() == 3 => {
                let fs = a0.fields().filter(|_| a0.bits & NIL == 0)?;
                let k = lit_key(1)?;
                let mut out: Vec<(Arc<str>, T)> = fs.iter().filter(|(n, _)| **n != *k).cloned().collect();
                out.push((Arc::from(&*k), args[2]));
                Some(map_of(out))
            }
            "remove" => {
                let fs = a0.fields().filter(|_| a0.bits & NIL == 0)?;
                let k = lit_key(1)?;
                let out: Vec<(Arc<str>, T)> = fs.iter().filter(|(n, _)| **n != *k).cloned().collect();
                Some(if out.is_empty() { T::of(MAP) } else { map_of(out) })
            }
            _ => None,
        }
    }

    /// Strings and numbers are always true in Quillo, where only `null`,
    /// `false` and errors are false; in JavaScript, `""` and `0` are false
    /// too. Code that reads the same in both then means something else, and
    /// nothing fails: `name || "anonymous"` keeps an empty name. Warned only
    /// where the value is known to be a string or a number (or null), or
    /// where `||`'s fallback says so (see `fallback_says`).
    /// What JavaScript habits mean here (FEEDBACK45.md, Q-513; branch feedback40-43):
    /// `index_of(...) == -1`, which is never true; a function compared with what is not one;
    /// a path's untyped parameter, text, added to a number.
    fn js_comparisons(&mut self, op: &BinOp, a: &Expr, b: &Expr, x: T, y: T) {
        let minus_one = |e: &Expr| matches!(e, Expr::Lit(Lit::Int(-1))) || matches!(e, Expr::Unary(UnOp::Neg, v, _) if matches!(&**v, Expr::Lit(Lit::Int(1))));
        let index_of = |e: &Expr| matches!(e, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "index_of"));
        if matches!(op, BinOp::Eq | BinOp::Ne) && !self.is_local("index_of") && ((index_of(a) && minus_one(b)) || (index_of(b) && minus_one(a))) {
            self.warn("`index_of` gives null when it is not there, never -1: compare with `null`");
            return;
        }
        if matches!(op, BinOp::Eq | BinOp::Ne) {
            for (f, other) in [(a, y), (b, x)] {
                if let Expr::Ident(n, _) = f {
                    let is_fn = if std::ptr::eq(f, a) { x.bits == FN } else { y.bits == FN };
                    if is_fn && other.bits != 0 && other.bits & (FN | NIL) == 0 {
                        self.warn(format!("`{n}` is a function, so this compares the function, not its answer: `{n}()`"));
                        return;
                    }
                }
            }
        }
        if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) && !self.text_params.is_empty() {
            for (p, other) in [(a, y), (b, x)] {
                if let Expr::Ident(n, _) = p {
                    let the_routes = self.scopes.iter().rposition(|s| s.contains_key(&**n)).map_or(false, |i| i + 1 == self.text_params_depth);
                    if the_routes && self.text_params.iter().any(|t| t == &**n) && other.bits != 0 && other.bits & !(INT | FLOAT | NIL) == 0 {
                        self.warn(format!("`{n}` is text from the path, so this does not add numbers: declare `:{n}(int)` to work with it as a number"));
                        return;
                    }
                }
            }
        }
    }

    fn js_truthiness(&mut self, t: T, e: &Expr, how: Truth) {
        // `if ready {` with `ready` a function: always true, the call forgotten (Q-513).
        if t.bits == FN {
            if let Expr::Ident(n, _) = e {
                self.warn(format!("`{n}` is a function, which counts as true: to test its answer, `{n}()`"));
                return;
            }
        }
        if t.bits & (STR | NUM | DEC) == 0 || t.bits & !(STR | NUM | DEC | NIL) != 0 {
            return;
        }
        let x = dotted(e).unwrap_or_else(|| "x".into());
        let what = if x == "x" { "this value".to_string() } else { format!("`{}`", x) };
        let nullable = t.bits & NIL != 0;
        // What JavaScript would test. A value that may be null is checked
        // for null first: `null != 0` holds, and `null > 0` is an error.
        let mut empties = Vec::new();
        if t.bits & STR != 0 {
            empties.push("\"\"");
        }
        if t.bits & (NUM | DEC) != 0 {
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
            // A string that may be null: `filled` says both tests at once
            // (FEEDBACK19.md, Q-151; branch feedback19-fixes).
            Truth::Or if nullable && empties == ["\"\""] => format!(
                "{what} counts as true even when it is {empty} (unlike JavaScript), so `||` keeps it{unless_null}: write `filled({x}) ?? ...`, or `{x} ?? ...` to replace only null"
            ),
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

    /// `"Hello ${name}"`: a plain string is not a template, so `${name}` is its text. Said when
    /// the name is one the program has here and the `$` was not written `\$` (FEEDBACK45.md,
    /// Q-510; branch feedback40-43).
    fn plain_dollar(&mut self, s: &str) {
        if self.plain_dollars.is_empty() || !s.contains("${") {
            return;
        }
        for part in s.split("${").skip(1) {
            let name: String = part.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let key = (self.file.to_string(), s.to_string(), name.clone());
            let known = self.is_local(&name) || self.globals.get(&name).is_some_and(|g| &*g.file != "<builtin>");
            if known && self.plain_dollars.contains(&key) {
                self.warn(format!(
                    "this string is not a template, so `${{{name}}}` stays as text: a template is written in backquotes, and `\\${{` keeps the text"
                ));
                return;
            }
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains_key(name))
    }

    /// `json.title` reads as null at run time: what a module does not have is a
    /// mistake, called or not (FEEDBACK18.md, Q-107; branch feedback20-fixes).
    fn module_member(&mut self, obj: &Expr, name: &str) {
        let Expr::Ident(m, _) = obj else { return };
        if !MODULES.contains(&m.as_str()) || self.is_local(m) || self.globals.get(m.as_str()).is_some_and(|g| &*g.file != "<builtin>") {
            return;
        }
        let full = format!("{}.{}", m, name);
        if MODULE_VALUES.contains(&full.as_str()) || BUILTINS.iter().any(|(n, _, _)| *n == full) {
            return;
        }
        let hint = if self.is_local(name) { format!(" (`{}` alone is the name here: a shape binds its fields)", name) } else { crate::error::removed_note(&full) };
        self.error(format!("`{}` has no `{}`{}", m, name, hint));
    }

    /// `json.parse(s)`, `db.query(...)`, `http.get(...)`.
    /// `billing.invoice(...)`: a service's server fn, as it declares it; it
    /// may also fail on the way (an error value), and the browser cannot call it.
    fn service_call(&mut self, svc: &str, f: &str, args: &[Expr], pre: Option<T>) -> T {
        let line = self.line;
        if let Some(v) = self.globals.get(svc).cloned() {
            self.record(svc, &v);
        }
        if let Some(c) = self.in_component() {
            self.error(format!("`{}.{}` calls another service, which only the server does; <{}> also runs in the browser: call it from a `server fn`", svc, f, c));
        }
        let sig = self.services.get(svc).and_then(|m| m.get(f).cloned());
        let mut argts: Vec<T> = pre.into_iter().collect();
        for a in args {
            argts.push(self.expr(a));
        }
        if sig.is_none() && self.services_elsewhere.contains(svc) {
            // Its functions are not known here, but a call of it is a call over HTTP.
            let http = crate::ast::effect_bit("http");
            let (allowed, restrictor) = match self.fns.last() {
                Some(f) => (f.allowed, f.restrictor.clone()),
                None => (crate::policy::ceiling(), None),
            };
            self.line = line;
            if http & !allowed != 0 {
                self.effect_error(&format!("{}.{}", svc, f), http, restrictor.as_ref());
            }
            return T::ANY;
        }
        self.line = line;
        let Some(sig) = sig else {
            let mut names: Vec<String> = self.services.get(svc).map(|m| m.keys().cloned().collect()).unwrap_or_default();
            names.sort();
            let hint = names.iter().min_by_key(|n| edit_distance(f, n)).filter(|n| edit_distance(f, n) <= 2).map(|n| format!(" (did you mean `{}`?)", n))
                .unwrap_or_else(|| if names.is_empty() { " (it has none)".into() } else { format!(" (it has {})", names.join(", ")) });
            self.error(format!("service `{}` has no server fn `{}`{}", svc, f, hint));
            return T::ANY;
        };
        self.apply(T::of(FN), Some(sig), argts, args, &format!("{}.{}", svc, f)).union(T::error(STR))
    }

    fn module_call(&mut self, m: &str, f: &str, args: &[Expr], pre: Option<T>) -> T {
        let line = self.line;
        let piped = pre.is_some();
        let full = if f.is_empty() { m.to_string() } else { format!("{}.{}", m, f) };
        let mut argts: Vec<T> = pre.into_iter().collect();
        for a in args {
            argts.push(self.expr(a));
        }
        self.line = line;
        let Some((_, p, r)) = BUILTINS.iter().find(|(n, _, _)| *n == full) else {
            // `json.prase`: the module's nearest (FEEDBACK45.md, Q-517; branch feedback40-43).
            let prefix = format!("{}.", m);
            let near = closest(f, BUILTINS.iter().filter_map(|(n, _, _)| n.strip_prefix(prefix.as_str())))
                .map_or(String::new(), |n| format!(" (did you mean `{}.{}`?)", m, n));
            self.error(format!("`{}` has no function `{}`{}{}", m, f, near, crate::error::removed_note(&full)));
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
        // A page opens channels; the rest of `channel` is the server's.
        // `http` calls go from the page too, once it has arrived.
        if m == "http" && f != "stream" {
            self.http_rules(f, args, piped);
        } else if full == "http.stream" && self.in_component().is_some() {
            self.error("`http.stream` is the server's; in the browser, `receive(url, f, ...)` in a handler and `listen(url, f)` read a stream");
        } else if !BROWSER_MODULES.contains(&m) && full != "channel.open" {
            self.browser_forbidden(&format!("`{}`", full));
        }
        if m == "db" && !piped {
            self.sql_params(&full, args);
        }
        if m == "money" && !piped {
            self.money_literals(f, args);
        }
        // A pattern written out is checked now, not when it runs.
        if m == "re" {
            let lit = |i: usize| match args.get(i) {
                Some(Expr::Lit(Lit::Str(s))) => Some(&**s),
                _ => None,
            };
            let flags_at = if f == "replace" { 3 } else { 2 };
            if let (Some(p), true) = (lit(1), args.get(flags_at).is_none() || lit(flags_at).is_some()) {
                if let Err(e) = crate::re::compile(p, lit(flags_at).unwrap_or("")) {
                    self.error(format!("`{}`: {}", full, e));
                }
            }
        }
        let sig = Arc::new(Sig::builtin(&full, p, r));
        self.apply(T::of(FN), Some(sig), argts, args, &full)
    }

    fn quiet_mode(&self) -> bool {
        self.quiet > 0
    }

    /// A rounding mode or a currency code written out in a `money` call that can only fail:
    /// `"half-up"` was a runtime error, `"XYZ"` an error value (FEEDBACK50.md, Q-633; branch
    /// feedback50-fixes).
    fn money_literals(&mut self, f: &str, args: &[Expr]) {
        let (code_at, mode_at) = match f {
            "of" => (Some(1), Some(2)),
            "parse" => (Some(1), Some(3)),
            "from_minor" => (Some(1), None),
            "zero" | "minor_units" => (Some(0), None),
            "mul" => (None, Some(2)),
            "round" => (None, Some(1)),
            _ => (None, None),
        };
        let lit = |i: Option<usize>| match i.and_then(|i| args.get(i)) {
            Some(Expr::Lit(Lit::Str(s))) => Some(s.to_string()),
            _ => None,
        };
        if let Some(code) = lit(code_at) {
            if crate::money::units_of(&code.trim().to_ascii_uppercase()).is_none() {
                self.error(format!("`money.{}`: {:?} is not an ISO 4217 currency code", f, code));
            }
        }
        if let Some(mode) = lit(mode_at) {
            if crate::decimal::Rounding::parse(&mode).is_none() {
                self.error(format!("`money.{}`: unknown rounding {:?} (one of {})", f, mode, crate::decimal::Rounding::NAMES));
            }
        }
    }

    // ---- SQL built from values ----------------------------------------

    /// `db.exec(sql, [a, b])`, Python's way: the parameters are separate arguments, and the
    /// list is one, stored as its JSON text (`["9.99"]`) where the SQL has one `?`
    /// (FEEDBACK50.md, Q-629; branch feedback50-fixes).
    fn sql_params(&mut self, what: &str, args: &[Expr]) {
        if !matches!(what.rsplit('.').next(), Some("exec" | "query" | "one")) {
            return;
        }
        // Written out: a template without `${}` is read as text too.
        let sql = match args.first() {
            Some(Expr::Lit(Lit::Str(s))) => s.to_string(),
            _ => return,
        };
        let [_, Expr::List(_)] = args else { return };
        // As the databases read it (outside quoted text, names and comments), and not where
        // PostgreSQL takes an array: `= any(?)`, `?::int[]` (FEEDBACK55.md, Q-755; branch
        // feedback55).
        let marks = crate::sql::values_for_a_list(&sql);
        if marks == 0 {
            return;
        }
        let what_happens = if marks == 1 { "stored as its JSON text".to_string() } else { format!("and the SQL's {} other `?` {} nothing", marks - 1, if marks == 2 { "gets" } else { "get" }) };
        self.warn(format!(
            "`{what}` takes its parameters one by one, as in `{what}(sql, a, b)`: this list is one parameter, {what_happens}. Pass the values after the SQL, or json.stringify(list) to store a list"
        ));
    }

    fn warn_sql(&mut self, name: &str) {
        self.warn(format!(
            "`{}` with SQL built from values: a value containing a quote can change the query \
             (SQL injection); write `?` in the SQL and pass the values after it",
            name
        ));
    }

    /// What the variable `name` holds, as far as SQL goes.
    /// `http.get(...)` and the rest. In a component they are made by the
    /// browser, from handlers: the server renders the component first, and
    /// runs its `state` initializers. Elsewhere, a URL whose host comes from
    /// a value may be a visitor's, who could then reach the server's own
    /// network: a word, unless the call says `public_only` either way.
    fn http_rules(&mut self, f: &str, args: &[Expr], piped: bool) {
        // An option written in the call that is not one: an error before
        // the program runs, as it is when it runs (FEEDBACK5.md, Q-049;
        // branch stays-fixes).
        let opts_at = match f {
            "get" | "delete" => Some(2),
            "post" | "put" | "patch" => Some(3),
            "request" => Some(4),
            _ => None,
        };
        if let (false, Some(Expr::Map(fields))) = (piped, opts_at.and_then(|i| args.get(i))) {
            for (k, _) in fields {
                if !["redirects", "public_only", "cache", "timeout", "ca", "cert", "key"].contains(&k.as_str()) {
                    self.error(format!("`http.{}`: unknown option `{}` (the options are redirects, public_only, cache, timeout, ca, cert, key)", f, k));
                }
            }
        }
        if let Some(c) = self.in_component() {
            let (nested, init) = self.fns.last().map_or((false, false), |x| (x.nested, x.state_init));
            if !nested || init {
                self.error(format!(
                    "`http.{}` in <{}> runs in the browser, once the page has arrived, and the server renders <{}> first: \
                     call it from an event handler or @mount, or from a `server fn`",
                    f, c, c
                ));
            }
            return;
        }
        let (url_at, opts_at) = match f {
            "get" | "delete" => (0, 2),
            "post" | "put" | "patch" => (0, 3),
            "request" => (1, 4),
            _ => return,
        };
        if piped {
            return;
        }
        let says = match args.get(opts_at) {
            Some(Expr::Map(fields)) => fields.iter().any(|(k, _)| k == "public_only"),
            Some(_) => true,
            None => false,
        };
        if let Some(url) = args.get(url_at) {
            if !says && !self.url_fixed(url) {
                self.warn(format!(
                    "the host `http.{}` goes to comes from a value: if a visitor can choose it, they can reach this \
                     server's own network (localhost, a cloud's metadata service). Pass {{ public_only: true }} as its options \
                     (or public_only: false, when it is meant to reach inside)",
                    f
                ));
            }
        }
    }

    /// Whether the host a URL goes to is written in the program: text as
    /// written, a template or `+` that starts with `scheme://host/` (or with
    /// such a URL and then `/`, `?` or `#`), the environment's setting, or a
    /// variable holding one of those.
    fn url_fixed(&self, e: &Expr) -> bool {
        match e {
            Expr::Lit(Lit::Str(_)) => true,
            Expr::Ident(n, _) => self.url_vars.iter().rev().find(|(_, v, _)| v == n).map_or(false, |v| v.2),
            Expr::Field(base, _, _) => self.url_fixed(base),
            Expr::Map(fields) => fields.iter().all(|(_, v)| self.url_fixed(v)),
            Expr::Call(f, args, _) => matches!(&**f, Expr::Ident(n, _) if n == "env") && matches!(args.first(), Some(Expr::Lit(_))),
            Expr::Coalesce(a, b) => self.url_fixed(a) && self.url_fixed(b),
            Expr::Template(parts) => match parts.as_slice() {
                [TPart::Expr(x)] => self.url_fixed(x),
                _ => parts.iter().all(|p| matches!(p, TPart::Lit(_))) || self.url_starts_fixed(e),
            },
            Expr::Binary(BinOp::Add, ..) => self.url_starts_fixed(e),
            Expr::If(_, a, b) => self.url_fixed(a) && b.as_deref().map_or(true, |b| self.url_fixed(b)),
            Expr::Match(_, arms, _) => arms.iter().all(|a| self.url_fixed(&a.body)),
            Expr::Block(stmts) => matches!(stmts.last(), Some(Stmt::Expr(e)) if self.url_fixed(e)),
            _ => false,
        }
    }

    /// Whether text starts with a host written in the program, ended (by
    /// `/`, `?` or `#`), so that what follows cannot change it: in
    /// `"https://api.example.com" + q`, q could be "@elsewhere".
    fn url_starts_fixed(&self, e: &Expr) -> bool {
        let host_written = |t: &str| {
            let l = t.get(..8).unwrap_or(t).to_ascii_lowercase();
            let rest = if l.starts_with("https://") { &t[8..] } else if l.starts_with("http://") { &t[7..] } else { return false };
            rest.find(['/', '?', '#']).map_or(false, |i| i > 0)
        };
        let path = |e: &Expr| matches!(e, Expr::Lit(Lit::Str(t)) if t.starts_with(['/', '?', '#']));
        match e {
            Expr::Lit(Lit::Str(t)) => host_written(t),
            Expr::Template(parts) => match parts.as_slice() {
                [TPart::Lit(t), ..] => host_written(t),
                [TPart::Expr(x), TPart::Lit(t), ..] => self.url_fixed(x) && t.starts_with(['/', '?', '#']),
                _ => false,
            },
            Expr::Binary(BinOp::Add, ..) => {
                let mut terms = Vec::new();
                let mut cur = e;
                while let Expr::Binary(BinOp::Add, a, b, _) = cur {
                    terms.push(&**b);
                    cur = a;
                }
                terms.push(cur);
                terms.reverse();
                self.url_starts_fixed(terms[0]) || (self.url_fixed(terms[0]) && terms.get(1).map_or(false, |t| path(t)))
            }
            _ => false,
        }
    }

    fn sql_var(&self, name: &str) -> SqlVar {
        self.sql_vars.iter().rev().find(|(_, n, _)| n == name).map_or(SqlVar::Other, |v| v.2)
    }

    /// What a `let` of `e` holds.
    fn sql_kind(&self, e: &Expr) -> SqlVar {
        if self.sql_built(e) {
            SqlVar::Built
        } else if self.sql_safe(e) {
            SqlVar::Safe
        } else if is_connect(e) {
            SqlVar::Conn
        } else {
            SqlVar::Other
        }
    }

    /// The query function `callee` names, if it is one that takes SQL:
    /// `db.query`, or `query` on a connection from `db.connect`.
    fn sql_target(&self, callee: &Expr) -> Option<String> {
        let Expr::Field(obj, f, _) = callee else { return None };
        if !matches!(f.as_str(), "query" | "one" | "exec" | "exec_many") {
            return None;
        }
        match &**obj {
            Expr::Ident(m, _) if m == "db" && !self.is_local(m) => Some(format!("db.{}", f)),
            Expr::Ident(m, _) if self.sql_var(m) == SqlVar::Conn => Some(format!("{}.{}", m, f)),
            other if is_connect(other) => Some(format!("db.connect(...).{}", f)),
            _ => None,
        }
    }

    /// Whether SQL text is put together from values: `${x}` in a template,
    /// sql`...${x}...`, `"..." + x`, `join(["...", x])`, or a variable
    /// holding such text.
    fn sql_built(&self, e: &Expr) -> bool {
        let interpolates = |parts: &[TPart]| parts.iter().any(|p| matches!(p, TPart::Expr(e) if !self.sql_safe(e)));
        match e {
            Expr::Ident(n, _) => self.sql_var(n) == SqlVar::Built,
            Expr::Template(parts) => interpolates(parts),
            Expr::Call(f, _, _) if self.sql_safe_call(f) => false,
            Expr::Call(f, args, _) => match args.as_slice() {
                [Expr::Template(parts)] => interpolates(parts),
                [Expr::List(items), rest @ ..] if is_join(f) => items.iter().chain(rest).any(|x| !self.sql_safe(x)),
                _ => false,
            },
            Expr::Pipe(lhs, rhs, _) => match (&**lhs, &**rhs) {
                (Expr::List(items), Expr::Call(f, rest, _)) if is_join(f) => items.iter().chain(rest).any(|x| !self.sql_safe(x)),
                (Expr::List(items), f) if is_join(f) => items.iter().any(|x| !self.sql_safe(x)),
                _ => false,
            },
            Expr::Binary(BinOp::Add, a, b, _) => {
                self.sql_built(a) || self.sql_built(b) || (self.sql_text(a) && !self.sql_safe(b)) || (self.sql_text(b) && !self.sql_safe(a))
            }
            Expr::If(_, a, b) => self.sql_built(a) || b.as_deref().map_or(false, |b| self.sql_built(b)),
            Expr::Match(_, arms, _) => arms.iter().any(|a| self.sql_built(&a.body)),
            Expr::Block(stmts) => matches!(stmts.last(), Some(Stmt::Expr(e)) if self.sql_built(e)),
            _ => false,
        }
    }

    /// Whether `e` is text: written, a variable holding text, or a join of
    /// those with `+` (`"select * from t " + "order by "`).
    fn sql_text(&self, e: &Expr) -> bool {
        match e {
            Expr::Lit(Lit::Str(_)) | Expr::Template(_) => true,
            Expr::Ident(n, _) => matches!(self.sql_var(n), SqlVar::Safe | SqlVar::Built),
            Expr::Binary(BinOp::Add, a, b, _) => self.sql_text(a) || self.sql_text(b),
            _ => false,
        }
    }

    /// SQL text a program may put together: names made with `sql_name` and
    /// `sql_param` (checked as they are made, so they cannot carry SQL of
    /// their own), text as written (or kept in a variable), and choices
    /// (`if`, `match`), joins and templates of those.
    fn sql_safe(&self, e: &Expr) -> bool {
        match e {
            Expr::Lit(_) => true,
            Expr::Ident(n, _) => self.sql_var(n) == SqlVar::Safe,
            // `join(map(cols, (c) => `${sql_name(c)} = ${sql_param(c)}`), ", ")`, and piped or
            // as methods, the pairs form the guide gives for an update: a list each of whose
            // items is safe, joined with safe text (FEEDBACK55.md, Q-754; branch feedback55).
            _ if self.sql_safe_join(e) => true,
            Expr::Call(f, args, _) => {
                self.sql_safe_call(f)
                    || (self.builtin_named(f, "join") && matches!(args.as_slice(), [Expr::List(items), rest @ ..] if items.iter().chain(rest).all(|x| self.sql_safe(x))))
                    // `` sql`select ...` ``, the tag editors highlight, is the text written in it:
                    // `BASE + "where id = ?"` warned for good (FEEDBACK53.md, Q-029; branch main).
                    || (matches!(&**f, Expr::Ident(n, _) if n == "sql" && !self.is_local(n) && self.globals.get("sql").is_some_and(|g| &*g.file == "<builtin>"))
                        && args.iter().all(|a| self.sql_safe(a)))
            }
            Expr::Template(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_)) || matches!(p, TPart::Expr(e) if self.sql_safe(e))),
            Expr::Binary(BinOp::Add, a, b, _) => self.sql_safe(a) && self.sql_safe(b),
            Expr::If(_, a, b) => self.sql_safe(a) && b.as_deref().map_or(true, |b| self.sql_safe(b)),
            Expr::Match(_, arms, _) => arms.iter().all(|a| self.sql_safe(&a.body)),
            Expr::Block(stmts) => matches!(stmts.last(), Some(Stmt::Expr(e)) if self.sql_safe(e)),
            _ => false,
        }
    }

    /// Markup the browser's HTML parser would not keep as written: a `<div>`
    /// (or other block) inside a `<p>`, which the parser ends first; `<a>`
    /// in `<a>`; `<form>` in `<form>`. The page is then not what the server
    /// sent, and the components in it are made again in the browser. A view
    /// or component counts as its outer element (`<Presence>` as its `<div>`).
    fn check_nesting(&mut self, order: &[Item]) {
        let views: HashMap<&str, &FnDef> = order
            .iter()
            .filter_map(|it| match &it.stmt {
                Stmt::Fn(d) if d.name.starts_with(|c: char| c.is_uppercase()) => Some((d.name.as_str(), &**d)),
                _ => None,
            })
            .collect();
        let mut found = Vec::new();
        for it in order {
            // The walk meets an element before the ones inside it, so those
            // are already marked (by address) when it reaches them.
            let mut inside: HashSet<*const HtmlEl> = HashSet::new();
            walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| {
                if let Expr::Html(el, _) = e {
                    if !inside.contains(&(&**el as *const HtmlEl)) {
                        let mut stack = vec![el.tag.to_ascii_lowercase()];
                        nesting_in(el, &mut stack, &views, &mut inside, &mut |line, msg| found.push((it.file.clone(), line, msg)));
                    }
                }
            });
        }
        for (file, line, msg) in found {
            self.diag_at(file, line, Severity::Error, msg);
        }
    }

    /// The service a `live` loader gets its data from, when that is all it
    /// reads: it calls a service, and neither `watch`es a topic nor reads
    /// the database, nor calls a function of the program that might. Such a
    /// loader follows only the variable holding the service, which never
    /// changes (FEEDBACK4.md, Q-039; branch bank-fixes).
    fn follows_only_a_service(&self, f: &str) -> Option<String> {
        let def = self.globals.get(f)?.sig.clone()?.def.clone()?;
        let mut service = None;
        let mut other = false;
        walk_expr(&def.body, &mut |e| match e {
            Expr::Call(c, _, _) => match &**c {
                Expr::Ident(n, _) if n == "watch" => other = true,
                Expr::Ident(n, _) => {
                    let program_fn = self.globals.get(n.as_str()).and_then(|v| v.sig.clone()).map_or(false, |s| !s.builtin && s.def.is_some());
                    if program_fn {
                        other = true;
                    }
                }
                Expr::Field(o, _, _) => match &**o {
                    Expr::Ident(n, _) if n == "db" => other = true,
                    Expr::Ident(n, _) if self.services.contains_key(n.as_str()) => service = Some(n.to_string()),
                    Expr::Field(inner, later, _) if later == "later" => {
                        if let Expr::Ident(n, _) = &**inner {
                            if self.services.contains_key(n.as_str()) {
                                service = Some(n.to_string());
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        });
        if other { None } else { service }
    }

    /// `join(xs, sep)`, however written (`xs | join(sep)`, `xs.join(sep)`), where every item of
    /// `xs` is SQL-safe text (`sql_items`) and `sep` is too.
    fn sql_safe_join(&self, e: &Expr) -> bool {
        let (list, sep): (&Expr, &[Expr]) = match e {
            Expr::Call(f, args, _) if self.builtin_named(f, "join") => match args.split_first() {
                Some((l, sep)) => (l, sep),
                None => return false,
            },
            Expr::Call(f, args, _) => match &**f {
                Expr::Field(l, n, _) if n == "join" && self.builtin_in_scope("join") => (&**l, args.as_slice()),
                _ => return false,
            },
            Expr::Pipe(l, rhs, _) => match &**rhs {
                Expr::Call(f, sep, _) if self.builtin_named(f, "join") => (&**l, sep.as_slice()),
                f if self.builtin_named(f, "join") => (&**l, &[]),
                _ => return false,
            },
            _ => return false,
        };
        sep.len() <= 1 && sep.iter().all(|x| self.sql_safe(x)) && self.sql_items(list)
    }

    /// Whether `e` is a list whose items are all SQL-safe text: written so (`[sql_name(a), ...]`),
    /// or `map` of a function that gives only such text (`(c) => `${sql_name(c)} = ${sql_param(c)}``,
    /// `sql_name` itself) over any list. The function's parameters are the list's items, which
    /// may be anything: one named as a variable holding safe text is not taken for it.
    fn sql_items(&self, e: &Expr) -> bool {
        let (f, _list) = match e {
            Expr::List(items) => return items.iter().all(|x| self.sql_safe(x)),
            Expr::Call(m, args, _) if self.builtin_named(m, "map") => match args.as_slice() {
                [l, f] => (f, Some(l)),
                _ => return false,
            },
            // `xs.map(f)`: the builtin, where `xs` holds no map, whose own `map` it would be.
            Expr::Call(m, args, _) => match (&**m, args.as_slice()) {
                (Expr::Field(l, n, _), [f]) if n == "map" && self.builtin_in_scope("map") && !self.may_be_map(l) => (f, Some(&**l)),
                _ => return false,
            },
            Expr::Pipe(l, rhs, _) => match &**rhs {
                Expr::Call(m, args, _) if self.builtin_named(m, "map") => match args.as_slice() {
                    [f] => (f, Some(&**l)),
                    _ => return false,
                },
                _ => return false,
            },
            _ => return false,
        };
        match f {
            Expr::Ident(..) => self.sql_safe_call(f),
            Expr::Lambda(def) => self.lambda_gives_sql_safe(def),
            _ => false,
        }
    }

    /// Whether every value the function `def` can give is SQL-safe text: its last value, and each
    /// `return`'s. A name it binds itself (a parameter, a `let`, a `match` arm's or a `for`'s
    /// pattern, an inner function's parameter) or assigns that is a variable holding text here
    /// would be taken for that variable, so the function is not followed then: `return c` and
    /// `{ let q = c; q }` with an outer `q = sql_name(...)` went without the warning (FEEDBACK55.md,
    /// Q-754; found in review).
    fn lambda_gives_sql_safe(&self, def: &FnDef) -> bool {
        // A name the function binds that is one of those, or one this rule trusts as a function
        // (`sql_name`, `join`, the program's own function of safe text): taken for it, so
        // `let sql_name = (x) => x` in it let raw text through (found in review).
        let trusted = ["sql_name", "sql_param", "sql", "join", "map", "db"];
        let tracked = |n: &str| self.sql_var(n) != SqlVar::Other || trusted.contains(&n) || self.sql_name_fns.contains(n);
        if def.params.iter().any(|p| tracked(p.name.as_str())) {
            return false;
        }
        let mut ok = true;
        walk_expr(&def.body, &mut |e| match e {
            Expr::Block(stmts) => {
                for st in stmts {
                    match st {
                        Stmt::Let(p, _, _) => {
                            let mut names = Vec::new();
                            pattern_names(p, &mut names);
                            ok &= !names.iter().any(|n| tracked(n));
                        }
                        Stmt::Assign(n, _, _, _) => ok &= !tracked(n.as_str()),
                        Stmt::Return(Some(v), _) => ok &= self.sql_safe(v),
                        Stmt::Fn(_) | Stmt::Use(..) => ok = false,
                        _ => {}
                    }
                }
            }
            Expr::Match(_, arms, _) => {
                for a in arms {
                    let mut names = Vec::new();
                    pattern_names(&a.pat, &mut names);
                    ok &= !names.iter().any(|n| tracked(n));
                }
            }
            Expr::For(p, _, _, _) => {
                let mut names = Vec::new();
                pattern_names(p, &mut names);
                ok &= !names.iter().any(|n| tracked(n));
            }
            Expr::Lambda(inner) => ok &= !inner.params.iter().any(|p| tracked(p.name.as_str())),
            _ => {}
        });
        ok && self.sql_safe(&def.body)
    }

    /// Whether `f` is the builtin `name`, not a local or the program's own of that name, which
    /// hides it (found in review: a program's `fn join` was followed as the builtin).
    fn builtin_named(&self, f: &Expr, name: &str) -> bool {
        matches!(f, Expr::Ident(n, _) if n == name) && self.builtin_in_scope(name)
    }

    /// Whether `name` here is the builtin of that name.
    fn builtin_in_scope(&self, name: &str) -> bool {
        !self.is_local(name) && self.globals.get(name).is_some_and(|g| &*g.file == "<builtin>")
    }

    /// Whether `e` may hold a map, whose own `map` its `.map(...)` would call: a map written out,
    /// or a variable that may hold one.
    fn may_be_map(&self, e: &Expr) -> bool {
        match e {
            Expr::Map(_) => true,
            Expr::Ident(n, _) => self.scopes.iter().rev().find_map(|sc| sc.get(n.as_str())).or_else(|| self.globals.get(n.as_str())).map_or(true, |v| v.t.bits == 0 || v.t.bits & MAP != 0),
            _ => false,
        }
    }

    /// Whether calling `f` gives SQL-safe text: `sql_name`, `sql_param`, or
    /// a function of the program that only returns such text.
    fn sql_safe_call(&self, f: &Expr) -> bool {
        matches!(f, Expr::Ident(n, _) if !self.is_local(n)
            && (((n == "sql_name" || n == "sql_param") && self.builtin_in_scope(n)) || self.sql_name_fns.contains(n.as_str())))
            || matches!(f, Expr::Field(o, n, _) if n == "sql_name" && matches!(&**o, Expr::Ident(m, _) if (m == "db" && !self.is_local(m)) || self.sql_var(m) == SqlVar::Conn))
    }

    /// A call of something of type `ft`: arguments, effects, browser rules
    /// and the result type.
    fn apply(&mut self, ft: T, sig: Option<Arc<Sig>>, args: Vec<T>, arg_exprs: &[Expr], name: &str) -> T {
        if ft.bits != 0 && ft.bits & FN == 0 {
            // A top-level variable with a builtin's name takes it from every
            // file: say whose it is, here and where it is made (FEEDBACK3.md,
            // Q-035; branch feedback6-fixes).
            let hidden = self.globals.get(name).filter(|g| &*g.file != "<builtin>" && matches!(g.kind, Kind::Global)).map(|g| (g.file.clone(), g.line));
            match hidden {
                Some((file, line)) if BUILTINS.iter().any(|(n, _, _)| *n == name) => {
                    let at = self.shown_at(&file, line);
                    self.error(format!(
                        "cannot call `{}`: it is {}, the program's variable from {}, which hides the builtin `{}` in every file",
                        name, bits_text(ft.bits), at, name
                    ));
                    let here = self.shown_at(&self.file.clone(), self.line);
                    self.diag_at(file, line, Severity::Warning, format!(
                        "`{}` hides the builtin `{}` in every file of the program, and {} calls the builtin: pick another name",
                        name, name, here
                    ));
                }
                _ => self.error(format!("cannot call `{}`: it is {}, not a function", name, bits_text(ft.bits))),
            }
            return T::ANY;
        }
        let Some(sig) = sig else { return T::ANY };
        if sig.component {
            self.error(format!("`{}` is a component; write <{} ... /> instead of calling it", name, sig.name));
        }
        let n = sig.params.len();
        // A builtin's call as its docs write it: `reduce(xs, start, f)` (Q-525).
        let shape = || format!("{}({})", name, sig.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "));
        let too_many = args.len() > n && !sig.variadic;
        if too_many {
            // `max(1, 2)`: one list, said once, not also as argument 1's type.
            if sig.builtin && n == 1 && sig.params[0].ty & !NIL == LIST && args.first().is_some_and(|a| a.bits != 0 && a.bits & LIST == 0) {
                self.error(format!("`{}` takes one list: put the items in it, `{}([...])`", name, name));
                return sig.ret.unwrap_or(T::ANY);
            }
            let shown = if sig.builtin { format!(": {}", shape()) } else { String::new() };
            self.error(format!("`{}` takes {} argument{}, got {}{}{}", name, n, if n == 1 { "" } else { "s" }, args.len(), shown, hides(&sig, name)));
        }
        for (i, p) in sig.params.iter().enumerate() {
            let what = || if sig.builtin { format!("argument {}", i + 1) } else { format!("argument `{}`", p.name) };
            match args.get(i) {
                None if !p.optional => {
                    let given_fn = i > 0 && args.get(i - 1).is_some_and(|a| a.bits == FN) && sig.params[i - 1].ty != FN;
                    if sig.builtin && given_fn && p.ty == FN {
                        // `reduce(xs, (a, x) => a + x)`: the start value left out.
                        self.error(format!("`{}` takes `{}` before the function: {}", name, sig.params[i - 1].name, shape()));
                    } else if sig.builtin {
                        let kind = if p.ty == FN { ", a function," } else { "" };
                        self.error(format!("missing argument `{}`{} in call to `{}`: {}", p.name, kind, name, shape()));
                    } else {
                        self.error(format!("missing argument `{}` in call to `{}`{}", p.name, name, hides(&sig, name)));
                    }
                    break;
                }
                Some(a) if p.annotated && a.bits != 0 && a.bits & p.ty == 0 && matches!(name, "__merge" | "__concat") => {
                    // A spread, which the parser made a call of (Q-036).
                    let (form, of) = if name == "__merge" { ("{...x}", "a map") } else { ("[...x]", "a list") };
                    self.error(format!("`{}` spreads {} (or null), but x is {}", form, of, bits_text(a.bits)));
                }
                Some(a) if p.annotated && a.bits != 0 && a.bits & p.ty == 0 => {
                    self.error(format!("{} of `{}` must be {}, got {}{}", what(), name, bits_text(p.ty), bits_text(a.bits), hides(&sig, name)));
                }
                Some(a) if p.annotated => {
                    // `range(0, n / 4)`: a whole number only when `/` divides
                    // evenly, so it works for some `n` and stops for others
                    // (FEEDBACK4.md, Q-044; branch bank-fixes).
                    let written = arg_exprs.get(i.wrapping_sub(args.len() - arg_exprs.len()));
                    let divided = match written {
                        Some(Expr::Binary(BinOp::Div, x, y, _)) => {
                            !matches!((&**x, &**y), (Expr::Lit(Lit::Int(m)), Expr::Lit(Lit::Int(n))) if *n != 0 && m % n == 0)
                        }
                        _ => false,
                    };
                    // `list<int>` given `[1, "a"]`: each item written out is held to the item type,
                    // as a value given alone is (FEEDBACK45.md, Q-512; branch feedback40-43). Only
                    // an item that cannot be one: inferred unions (`x * 2` is `num | dec`) pass.
                    let bad_item = match (written, p.full.items()) {
                        (Some(Expr::List(items)), Some(we)) if we.bits != ANY => {
                            let mut found = None;
                            for (k, item) in items.iter().enumerate() {
                                self.quiet += 1;
                                let it = self.expr(item);
                                self.quiet -= 1;
                                if it.bits != 0 && it.bits & we.bits == 0 {
                                    found = Some((k + 1, it, we));
                                    break;
                                }
                            }
                            found
                        }
                        _ => None,
                    };
                    if let Some(why) = mismatch(*a, p.full) {
                        self.error(format!("{} of `{}` {}{}", what(), name, why, hides(&sig, name)));
                    } else if let Some((k, it, we)) = bad_item {
                        self.error(format!("{} of `{}` must be {}, but its item {} is {}, not {}{}", what(), name, t_text(p.full), k, t_text(it), t_text(we), hides(&sig, name)));
                    // A parameter that takes decimals takes what `/` gives: rounding
                    // it would lose the cents (FEEDBACK25.md, Q-321; branch feedback26-fixes).
                    } else if divided && p.ty & INT != 0 && p.ty & FLOAT == 0 && p.ty & DEC == 0 && a.bits & FLOAT != 0 {
                        self.warn(format!(
                            "{} of `{}` must be an int, and `/` gives one only when it divides evenly (`7 / 2` is 3.5): write `floor(...)` or `round(...)`",
                            what(), name
                        ));
                    } else if self.strict_null && !a.vague() && a.bits & NIL != 0 && a.bits != NIL && p.ty & NIL == 0 {
                        self.warn(format!("{} of `{}` may be null here (it is {}), but it must be {}", what(), name, t_text(*a), bits_text(p.ty)));
                    }
                }
                _ => {}
            }
        }
        // A piped value is the SQL: the pipe looks at it (see `Expr::Pipe`).
        if sig.builtin {
            self.lambda_arity(name, arg_exprs, 0);
        }
        // A group's middleware that can give neither the request back, nor null, nor an answer
        // stops every request: a predicate's `true` is a 500 (FEEDBACK47.md, Q-534).
        if sig.builtin && name == "__answer" {
            let t = args.first().copied().unwrap_or(T::ANY);
            // Null too, which says the group's routes do not match (FEEDBACK51.md, Q-651): a
            // middleware that only logs, and gives nothing back, leaves them never matching.
            if t.bits != 0 && !t.vague() && t.bits & (MAP | RESP | ERR) == 0 {
                let who = match arg_exprs.get(1) {
                    Some(Expr::Lit(Lit::Str(s))) => s.to_string(),
                    _ => "this middleware".to_string(),
                };
                self.error(format!(
                    "`{}` gives {}: middleware gives back the request to let the route go on (null: the route does not match), or an answer to stop it; a check is written `(r) => ok(r) ? r : reply 403 \"...\"`",
                    who,
                    bits_text(t.bits)
                ));
            }
        }
        // `db.exec("begin")`, refused as it runs (FEEDBACK44.md, Q-580; branch feedback40-43).
        if sig.builtin && matches!(name, "db.query" | "db.one" | "db.exec") {
            if let Some(Expr::Lit(Lit::Str(s))) = arg_exprs.first() {
                // As every database reads it: a function's body is not a `begin` (FEEDBACK55.md,
                // Q-746; branch feedback55).
                if crate::sql::tx_control_everywhere(s) {
                    self.error(format!("`{}(\"{}\")`: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it", name, s.trim()));
                }
            }
        }
        if sig.builtin && matches!(name, "db.query" | "db.one" | "db.exec" | "db.exec_many") && args.len() == arg_exprs.len() {
            if arg_exprs.first().map_or(false, |e| self.sql_built(e)) {
                self.warn_sql(name);
            }
        }
        if sig.builtin && matches!(sig.name.as_str(), "db.transaction" | "idempotent") {
            self.retried_effects(name, arg_exprs);
            // A key misspelled was the default, quietly (FEEDBACK55.md, Q-738; branch feedback55).
            if sig.name == "db.transaction" {
                if let [Expr::Map(opts), _, ..] = arg_exprs {
                    for (k, _) in opts {
                        if !crate::txn::OPTIONS.contains(&k.as_str()) {
                            let near = crate::txn::OPTIONS.iter().map(|o| (edit_distance(k, o), *o)).filter(|(d, _)| *d <= 2).min();
                            let hint = near.map_or_else(|| format!(" ({})", crate::txn::OPTIONS.join(", ")), |(_, o)| format!(" (did you mean `{}`?)", o));
                            self.error(format!("`{}` has no option `{}`{}", name, k, hint));
                        }
                    }
                }
            }
        }
        // Effects.
        let (allowed, restrictor) = match self.fns.last() {
            Some(f) => (f.allowed, f.restrictor.clone()),
            None => (crate::policy::ceiling(), None),
        };
        let needs = if sig.builtin { sig.effects.unwrap_or(0) } else { sig.effects.unwrap_or(0) };
        if needs & !allowed != 0 && needs == ALL_EFFECTS && sig.native.is_some() {
            let msg = crate::native::undeclared(name, sig.native.as_deref().unwrap_or(""), restrictor.as_ref().map(|(f, _)| f.as_str()));
            self.error(msg);
        } else if needs & !allowed != 0 {
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
            let public = sig.name == "env" && matches!(arg_exprs.first(), Some(Expr::Lit(Lit::Str(n))) if n.starts_with("PUBLIC_"));
            if sig.builtin && BROWSER_ONLY_SERVER.contains(&sig.name.as_str()) && !public {
                let hint = if sig.name == "env" { " (or name the variable PUBLIC_..., which says the browser may see it)" } else { "" };
                self.error(format!("`{}` is not available in the browser, where <{}> also runs; use a `server fn`{}", sig.name, c, hint));
            }
            if let Some(f) = &sig.native {
                let f = f.rsplit('/').next().unwrap_or(f).to_string();
                self.error(format!("`{}` is Rust ({}), which runs on the server, not in <{}>; call it from a `server fn`", sig.name, f, c));
            }
            let nested = self.fns.last().map_or(false, |f| f.nested);
            let init = self.fns.last().map_or(false, |f| f.state_init);
            if !sig.builtin && !sig.server && !sig.component && (!nested || init) && self.fn_effects(&sig) & crate::ast::effect_bit("http") != 0 {
                self.error(format!(
                    "`{}` calls `http`, which in <{}> runs in the browser, once the page has arrived, and the server renders <{}> first: \
                     call it from an event handler or @mount, or make it a `server fn`",
                    name, c, c
                ));
            }
            if sig.server && !nested {
                self.error(format!(
                    "`{}` waits on the server, so <{}> can only call it from an event handler (@click={{() => {{ ... }}}}) or to set state: state x = {}(...)",
                    name, c, name
                ));
            }
        }
        if sig.builtin && sig.name == "live" {
            if let Some(Err(e)) = arg_exprs.first().filter(|o| matches!(o, Expr::Map(_))).map(crate::builtins::live_every) {
                self.error(e);
            }
            let loader = if matches!(arg_exprs.first(), Some(Expr::Map(_))) { arg_exprs.get(1) } else { arg_exprs.first() };
            match loader {
                Some(Expr::Ident(f, _)) => {
                    let is_server = self.globals.get(f.as_str()).and_then(|v| v.sig.clone()).map_or(true, |s| s.server || s.def.is_none() && !s.builtin);
                    if !is_server {
                        self.error(format!("`live` needs a server fn: `{}` is not one", f));
                    }
                    if let Some(service) = self.follows_only_a_service(f) {
                        self.warn(format!(
                            "`{}` gets its data from the service `{}`, whose changes reach no page by themselves, so this state never changes: `watch(\"topic\")` in `{}`, and `publish(\"topic\")` where it changes (in the service, or here after the call)",
                            f, service, f
                        ));
                    }
                }
                Some(_) => self.error("`live` needs a server fn by name: state items = live(load_items, ...)"),
                None => {}
            }
            if let Some(c) = self.in_component() {
                if !self.live_ok {
                    self.error(format!("in <{}>, `live(...)` sets state: state items = live(load_items, ...)", c));
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
        if sig.builtin {
            if let Some(t) = self.builtin_ret(&sig.name, &args, arg_exprs) {
                return t;
            }
        }
        // A `server fn` called from a component's handler is a request: when
        // it fails (no connection, an argument refused) the call gives an
        // error value, so `is_err(r) ? r.error : ...` is right. A state's
        // initializer runs on the server and calls it directly
        // (FEEDBACK18.md, Q-104; branch feedback18-fixes).
        if sig.server && self.in_component().is_some() {
            let f = self.fns.last();
            if f.map_or(false, |f| f.nested && !f.state_init) {
                let r = self.fn_ret(&sig);
                return if r.bits & ERR != 0 { r } else { r.union(T::error(STR)) };
            }
        }
        self.fn_ret(&sig)
    }

    fn field(&mut self, t: T, obj: &Expr, name: &str) -> T {
        let _ = obj;
        // A map whose fields are known: one it has, or a mistake (a missing
        // key reads as null, so nothing would fail).
        if let Some(fs) = t.fields() {
            if let Some((_, ft)) = fs.iter().find(|(k, _)| &**k == name) {
                return ft.union(T::of(t.bits & NIL));
            }
            if !matches!(name, "len" | "keys" | "values") {
                self.error(format!("this map has no field `{}`{}", name, field_hint(name, &fs)));
                return T::of(NIL);
            }
        }
        // A field of a map whose shape is known, where the value may also be an error: the
        // field's type (an error has no fields, so reading one stops there anyway).
        // `money.of(...).amount + 0.5` passed as anything (FEEDBACK50.md, Q-633; branch
        // feedback50-fixes).
        if t.bits & MAP != 0 && t.bits & !(MAP | ERR | NIL) == 0 {
            if let Some((_, ft)) = shape_of(t.shape).and_then(|fs| fs.iter().find(|(k, _)| &**k == name).cloned()) {
                return ft.union(T::of(t.bits & NIL));
            }
        }
        if t.bits & (MAP | NIL) != 0 || t.is_any() {
            if t.bits & MAP == 0 && t.bits & ERR != 0 && matches!(name, "error" | "message") {
                return T::of(t.err);
            }
            return T::ANY;
        }
        if t.bits & ERR != 0 && matches!(name, "error" | "message") {
            return T::of(t.err);
        }
        if t.bits & RESP != 0 && matches!(name, "status" | "headers" | "body") {
            return T::ANY;
        }
        // `error` is a builtin too, but `x.error` on what cannot fail is a mistake.
        let found = if name == "error" { None } else { self.method(name) };
        match found {
            Some(v) if v.t.bits & FN != 0 => {
                self.record(name, &v);
                self.apply(v.t, v.sig.clone(), vec![t], &[], name)
            }
            _ => {
                if t.bits != 0 {
                    let near_len = matches!(name, "length" | "lenght" | "lengh" | "size" | "count") || osa_distance(name, "length") <= 2;
                    let hint = if near_len && t.bits & (LIST | STR) != 0 { ": `len(x)` is how many it holds" } else { "" };
                    self.error(format!("{} has no field `{}`{}", bits_text(t.bits), name, hint));
                }
                T::ANY
            }
        }
    }

    /// `<input value={v}/>` where `v` is a parameter, or `value={f.name}` a state's field:
    /// one way, so the input shows what is typed and the program keeps what it had, with
    /// nothing said (FEEDBACK46.md, Q-532; branch feedback40-43). Not when something takes the
    /// value (a handler, `bind:`, `name=` for a form sent to a route), nor when nothing can be
    /// typed (`readonly`, `disabled`), nor when what the element is is decided as it runs. A
    /// `<details open={p}>` opens by itself: `p` is where it starts.
    fn shown_only(&mut self, el: &HtmlEl) {
        // Any handler on the element may take the value (`@blur` saving it, `@mount` wiring
        // its own listeners), and an `input` or `change` reaches one on an element around it
        // (found in review).
        if self.input_above > 0 || el.attrs.iter().any(|(a, v)| a.starts_with('@') || a == "name" || matches!(v, AttrVal::Spread(_))) || el.tag.contains('-') {
            return;
        }
        let set = |n: &str| el.attrs.iter().any(|(a, v)| a == n && !matches!(v, AttrVal::Expr(Expr::Lit(Lit::Bool(false)))));
        if set("disabled") {
            return;
        }
        let kind = match el.attrs.iter().find(|(a, _)| a == "type") {
            None => None,
            Some((_, AttrVal::Expr(Expr::Lit(Lit::Str(t))))) => Some(t.to_ascii_lowercase()),
            Some(_) => return,
        };
        // Only the parameters of the view or component itself: a lambda's or a local fn's are
        // that function's (found in review).
        if self.fns.len() != 1 {
            return;
        }
        let Some(owner) = self.fns.first().map(|f| f.name.clone()).filter(|n| n.starts_with(|c: char| c.is_uppercase())) else { return };
        let multiple = el.attrs.iter().any(|(a, _)| a == "multiple");
        for (n, v) in &el.attrs {
            let AttrVal::Expr(x) = v else { continue };
            // `readonly` holds a text field; a box or a list is still changed (the browser
            // ignores it there): `disabled` is what holds those.
            let (verb, holds) = match (el.tag.as_str(), n.as_str()) {
                ("input", "value") if !matches!(kind.as_deref(), Some("checkbox" | "radio" | "file" | "button" | "submit" | "reset" | "image" | "hidden" | "range" | "color")) => ("typing here", "readonly"),
                ("input", "value") if matches!(kind.as_deref(), Some("range" | "color")) => ("choosing here", "disabled"),
                ("textarea", "value") => ("typing here", "readonly"),
                ("select", "value") if !multiple => ("choosing here", "disabled"),
                ("input", "checked") if kind.as_deref() == Some("checkbox") => ("ticking this", "disabled"),
                _ => continue,
            };
            if el.attrs.iter().any(|(a, _)| *a == format!("bind:{}", n)) || (holds == "readonly" && set("readonly")) {
                continue;
            }
            // `v ?? ""` shows `v` too.
            let mut x = x;
            while let Expr::Coalesce(l, _) = x {
                x = l;
            }
            // The name it starts from: `v`, `user.name`, `form.a.b`.
            let (mut root, mut path) = (x, Vec::new());
            while let Expr::Field(o, f, _) = root {
                path.push(f.clone());
                root = o;
            }
            let Expr::Ident(name, _) = root else { continue };
            let Some(kind_of) = self.scopes.iter().rev().find_map(|s| s.get(name.as_str())).map(|v| v.kind) else { continue };
            path.reverse();
            let shown = std::iter::once(name.to_string()).chain(path.iter().cloned()).collect::<Vec<_>>().join(".");
            let what = match (kind_of, path.is_empty()) {
                (Kind::Param, true) => format!("`{}` is `{}`'s parameter, which `{}=` only shows:", shown, owner, n),
                (Kind::Param, false) => format!("`{}` is a field of `{}`'s parameter `{}`, which `{}=` only shows:", shown, owner, name, n),
                (Kind::State, false) => format!("`{}` is a field of the state `{}`, which `{}=` only shows; give it a state of its own, or", shown, name, n),
                _ => continue,
            };
            let event = if n == "checked" { "@change" } else { "@input" };
            self.line = el.line;
            self.warn(format!("{} changes nothing: {} take what changes with `{}={{(e) => ...}}`, or add `{}`", verb, what, event, holds));
        }
    }

    fn html(&mut self, el: &HtmlEl) {
        for line in &el.dollars {
            self.line = *line;
            self.warn("the `$` before this `{` is text, and shows on the page: `{name}` alone gives the value here (`${name}` is for templates); write `{\"$\"}{name}` if the `$` is meant");
        }
        // As JSX has it, a line break between text and an element is no space (FEEDBACK48.md,
        // Q-573; branch main).
        for (line, a, b) in &el.joined {
            // A view whose element is a block, `<BulletList>`, starts a line of its own anyway.
            let block_view = [a, b].iter().filter_map(|s| s.strip_prefix('<').and_then(|s| s.strip_suffix('>'))).any(|t| {
                t.starts_with(|c: char| c.is_uppercase())
                    && self.ident(t).1.and_then(|s| s.def.clone()).is_some_and(|d| root_tag(&d.body).is_some_and(|r| !crate::parser::inline_tag(r)))
            });
            if block_view {
                continue;
            }
            self.line = *line;
            self.warn(format!("{} and {} are on lines of their own, so no space comes between them on the page: write {{\" \"}} between them, or put them on one line", a, b));
        }
        self.line = el.line;
        if let Some(id) = &el.id {
            self.expr(id);
        }
        for c in &el.classes {
            self.expr(c);
        }
        let is_view = el.tag.chars().next().map_or(false, |c| c.is_uppercase());
        // `<card/>` for `view Card`: an element no browser has, written to the page as it is
        // (FEEDBACK45.md, Q-509; branch feedback40-43).
        const ELEMENTS: &[&str] = &[
            "a", "abbr", "address", "area", "article", "aside", "audio", "b", "base", "bdi", "bdo", "blockquote", "body", "br", "button",
            "canvas", "caption", "cite", "code", "col", "colgroup", "data", "datalist", "dd", "del", "details", "dfn", "dialog", "div", "dl",
            "dt", "em", "embed", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header",
            "hgroup", "hr", "html", "i", "iframe", "img", "input", "ins", "kbd", "label", "legend", "li", "link", "main", "map", "mark", "menu",
            "meta", "meter", "nav", "noscript", "object", "ol", "optgroup", "option", "output", "p", "param", "picture", "pre", "progress", "q",
            "rp", "rt", "ruby", "s", "samp", "script", "search", "section", "select", "slot", "small", "source", "span", "strong", "style", "sub",
            "summary", "sup", "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "time", "title", "tr", "track", "u",
            "ul", "var", "video", "wbr", "svg", "math", "center", "font", "marquee", "big", "tt", "strike", "frame", "frameset", "noframes",
            // SVG's own.
            "g", "path", "circle", "ellipse", "line", "polyline", "polygon", "rect", "text", "tspan", "textpath", "use", "symbol", "defs",
            "marker", "mask", "pattern", "clippath", "lineargradient", "radialgradient", "stop", "image", "foreignobject", "filter",
            "animate", "animatetransform", "animatemotion", "set", "desc", "metadata", "switch", "view", "fegaussianblur", "feoffset",
            "feblend", "fecolormatrix", "feflood", "fecomposite", "femerge", "femergenode", "feturbulence", "fedisplacementmap",
        ];
        if !is_view && !el.tag.contains('-') && !ELEMENTS.contains(&el.tag.to_ascii_lowercase().as_str()) && !self.is_local(&el.tag) {
            let mut cap = el.tag.clone();
            if let Some(f) = cap.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            if cap != el.tag && self.globals.get(&cap).is_some_and(|g| g.sig.is_some() && &*g.file != "<builtin>") {
                self.line = el.line;
                self.warn(format!("<{}> is no HTML element, and is written to the page as it is: the view is <{}>", el.tag, cap));
            }
        }
        if !is_view {
            self.shown_only(el);
        }
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
            if !is_view {
                self.attr_typo(&el.tag, name);
            }
            if let Some(ev) = name.strip_prefix('@') {
                // A handler's answer goes nowhere, its last statement's too: `items.push(1)`
                // there changes nothing (FEEDBACK45.md, Q-505; branch feedback40-43).
                if let AttrVal::Expr(Expr::Lambda(d)) = v {
                    let last = match &d.body {
                        Expr::Block(stmts) => stmts.last().cloned(),
                        e => Some(Stmt::Expr(e.clone())),
                    };
                    if let Some(last) = last {
                        self.answer_dropped(&last);
                        self.op_dropped(&last);
                    }
                }
                if is_view {
                    self.error(format!("<{}> is a view; `@{}` handlers go on HTML elements", el.tag, ev));
                } else if t.bits != 0 && t.bits & FN == 0 {
                    // `onclick={text}` is read as this too: say what a handler is.
                    self.error(format!("`@{}` needs a function, got {}: a handler is a function, `@{}={{() => ...}}` (or `on{}`); text is never run as code", ev, bits_text(t.bits), ev, ev));
                }
            } else if let Some(prop) = name.strip_prefix("bind:") {
                if crate::ast::bind_event(&el.tag, prop).is_none() {
                    self.error(format!("`bind:{}` is not something a person changes in <{}> (see bind in the guide: value, checked, indeterminate, open, muted, volume, currentTime, playbackRate)", prop, el.tag));
                }
                match v {
                    AttrVal::Expr(Expr::Ident(n, _)) => {
                        let found = self.scopes.iter().rev().find_map(|s| s.get(n.as_str()).cloned());
                        if found.is_none() && self.in_component().is_some() && self.globals.contains_key(n.as_str()) {
                            self.error(format!("`bind:{}` needs component state or a local variable, like ~{}", prop, n));
                        }
                    }
                    _ => self.error(format!("`bind:{}` needs a variable: bind:{}={{name}}", prop, prop)),
                }
            }
            given.push((name.clone(), t));
        }
        // `<form @input={...}>`: what is typed inside reaches it (Checker::shown_only).
        let bubbles = el.attrs.iter().any(|(a, _)| a == "@input" || a == "@change");
        if bubbles {
            self.input_above += 1;
        }
        for c in &el.children {
            match c {
                HtmlChild::El(e) => crate::stack::maybe_grow(256 << 10, 4 << 20, || self.html(e)),
                HtmlChild::Expr(x) => {
                    // `<p>{{ name }}</p>`, the habit of Vue, Handlebars and Jinja: a map in braces,
                    // shown as its text (FEEDBACK35.md, Q-430; branch feedback27-35-fixes).
                    match x {
                        Expr::Map(entries) if entries.len() == 1 && matches!(&entries[0].1, Expr::Ident(n, _) if *n == entries[0].0) => {
                            self.line = el.line;
                            let n = &entries[0].0;
                            self.warn(format!("`{{{{ {n} }}}}` shows the map `{{ {n}: ... }}`: to show `{n}`, write `{{{n}}}`"));
                        }
                        Expr::Map(_) => {
                            self.line = el.line;
                            self.warn("a map written as an element's child is shown as its text: one pair of braces shows a value, `{x}`");
                        }
                        // `<pre>f() { return 1 }</pre>`, pasted HTML: the `return` ends the route,
                        // and its value is the whole answer (FEEDBACK36.md, Q-448; branch feedback36-39).
                        Expr::Block(stmts) if stmts.iter().any(|s| matches!(s, Stmt::Return(..))) => {
                            self.line = el.line;
                            self.warn(format!("`{{ return ... }}` in <{}> leaves the function, and its value is the answer, not the page: for the characters, write {{\"{{\"}} and {{\"}}\"}}, or put the text in a string", el.tag));
                        }
                        Expr::Block(_) => {
                            self.line = el.line;
                            self.warn("`{{ ... }}` here is a block, and its value is shown: one pair of braces shows a value, `{x}`");
                        }
                        _ => {}
                    }
                    if let Some(c) = self.in_component() {
                        let mut shown = Vec::new();
                        walk_expr(x, &mut |y| {
                            if let Expr::Ident(n, _) = y {
                                shown.push(n.clone());
                            }
                        });
                        for n in shown {
                            if let Some(i) = self.page_lets.iter().position(|(pc, pn)| *pc == c && *pn == n) {
                                self.page_lets.remove(i);
                                self.line = el.line;
                                self.warn(format!("`{n}` is a value from the page shown in markup: the server has null for it, so the browser makes <{c}> again on each load. Write `state {n} = ...`, which is set once the component is on the page"));
                            }
                        }
                    }
                    // What `quillo run` refuses in a component's script, refused here too (found in
                    // review of FEEDBACK53.md, Q-682).
                    if el.tag == "script" && self.in_component().is_some() && matches!(x, Expr::Template(p) if matches!(p.as_slice(), [TPart::Expr(Expr::Lit(Lit::Str(_)))])) {
                        self.line = el.line;
                        self.error("a string in braces in a <script> is a value, written as JSON: write the code without the braces and quotes, as the script's own text");
                    }
                    let t = self.expr(x);
                    // React's function as children, or `{onClick}` for `{label}`: shown as nothing
                    // (FEEDBACK48.md, Q-571; branch main).
                    if matches!(x, Expr::Lambda(_)) || t.bits == FN {
                        self.line = el.line;
                        self.warn("a function where markup goes shows nothing: call it, {render(x)}, or give it to a view as an attribute, render={(x) => ...}");
                    }
                }
            }
        }
        if bubbles {
            self.input_above -= 1;
        }
        if !is_view {
            return;
        }
        self.line = el.line;
        // React's `<Fragment>` (FEEDBACK48.md, Q-577; branch main).
        if el.tag == "Fragment" && !self.is_local("Fragment") && !self.globals.contains_key("Fragment") {
            self.error(crate::parser::FRAGMENTS);
            return;
        }
        let (t, sig) = self.ident(&el.tag);
        let Some(sig) = sig else {
            if t.bits != 0 && t.bits & FN == 0 {
                // `let Tag = "h1"` then `<Tag>`: React's dynamic tag (Q-577).
                let dynamic = if t.bits & STR != 0 { ": a tag's name is written out, as there are no dynamic tags; choose between them with `match`, or write a view per tag" } else { "" };
                self.error(format!("<{}> is not a view: it is {}{}", el.tag, bits_text(t.bits), dynamic));
            }
            return;
        };
        if sig.builtin {
            self.error(format!("<{}> is a builtin function, not a view", el.tag));
            return;
        }
        // React's `function Card(props)`: a view's parameters are its props, each by its name
        // (FEEDBACK48.md, Q-577; branch main).
        if let [only] = sig.params.as_slice() {
            if only.name == "props" && given.iter().any(|(n, _)| n != "props" && n != "key" && !n.starts_with('@')) {
                let names: Vec<&str> = given.iter().map(|(n, _)| n.as_str()).filter(|n| *n != "key" && !n.starts_with('@')).collect();
                self.error(format!("a view's parameters are its props, each by name: write `view {}({})`, not one `props`", el.tag, names.join(", ")));
                return;
            }
        }
        // What is between its tags goes nowhere without a `children` parameter, where an
        // attribute it lacks was already an error (FEEDBACK48.md, Q-569; branch main).
        if !el.children.is_empty() && !spread && !sig.params.iter().any(|p| p.name == "children") {
            self.error(format!("<{}> takes no children, so what is between its tags is lost: give it a `children` parameter, or write <{} .../>", el.tag, el.tag));
        }
        for (n, t) in &given {
            if n == "key" || n.starts_with('@') {
                continue;
            }
            match sig.params.iter().find(|p| &p.name == n) {
                // `<Card.big #main>` is `class` and `id` given to it (Q-568).
                None if n == "class" || n == "id" => self.error(format!(
                    "<{}> has no parameter `{}`: a view's attributes are its parameters, `.name` giving `class` and `#name` giving `id`; give it {} `{}` parameter and put it on its element",
                    el.tag, n, if n == "id" { "an" } else { "a" }, n
                )),
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

/// The tag of the element a view's body gives, when it ends with one.
fn root_tag(body: &Expr) -> Option<&str> {
    match body {
        Expr::Html(el, _) => Some(el.tag.as_str()),
        Expr::Block(stmts) => match stmts.last() {
            Some(Stmt::Expr(e)) => root_tag(e),
            _ => None,
        },
        _ => None,
    }
}

/// Why a value can never be what an annotation says, past its kind: a
/// list of something else, a map without a field it must have.
fn mismatch(a: T, want: T) -> Option<String> {
    if let (Some(ae), Some(we)) = (a.items(), want.items()) {
        if ae.bits != 0 && ae.bits & we.bits == 0 {
            return Some(format!("must be {}, got {}", t_text(want), t_text(a)));
        }
        return mismatch(ae, we).map(|why| format!("holds an item that {}", why));
    }
    if let (Some(af), Some(wf)) = (a.fields(), want.fields()) {
        for (k, wt) in wf.iter() {
            match af.iter().find(|(n, _)| n == k) {
                None if wt.bits & NIL == 0 => return Some(format!("has no field `{}`, which {} needs", k, t_text(want))),
                Some((_, at)) if at.bits != 0 && at.bits & wt.bits == 0 => {
                    return Some(format!("has `{}` as {}, but it must be {}", k, t_text(*at), t_text(*wt)))
                }
                Some((_, at)) => {
                    if let Some(why) = mismatch(*at, *wt) {
                        return Some(format!("has `{}` that {}", k, why));
                    }
                }
                None => {}
            }
        }
    }
    None
}

/// `(did you mean `title`?)`, or the fields the map has.
fn field_hint(name: &str, fs: &Shape) -> String {
    let close = fs.iter().map(|(k, _)| (edit_distance(name, k), k)).filter(|(d, _)| *d <= 2).min_by_key(|(d, _)| *d);
    match close {
        Some((_, k)) => format!(" (did you mean `{}`?)", k),
        None => {
            let ks: Vec<&str> = fs.iter().map(|(k, _)| &**k).take(8).collect();
            format!(" (it has {})", ks.join(", "))
        }
    }
}

/// What an unannotated lambda given to a builtin takes: `xs | map((x) => ...)`
/// gives `x` what `xs` holds. `before` are the arguments ahead of it.
fn lambda_hint(sig: Option<&Sig>, before: &[T], arg: &Expr) -> Option<Vec<T>> {
    let sig = sig.filter(|s| s.builtin)?;
    if !matches!(arg, Expr::Lambda(_)) {
        return None;
    }
    let items = before.first()?.items()?;
    match (sig.name.as_str(), before.len()) {
        ("map" | "filter" | "find" | "any" | "all" | "sort_by" | "count" | "group_by", 1) => Some(vec![items]),
        ("reduce", 2) => Some(vec![T::ANY, items]),
        _ => None,
    }
}

/// A list that has items whatever happens: `[a, b]`, `xs + [0]`.
fn never_empty(e: &Expr) -> bool {
    match e {
        Expr::List(items) => !items.is_empty(),
        Expr::Binary(BinOp::Add, a, b, _) => never_empty(a) || never_empty(b),
        _ => false,
    }
}

/// A branch that never ends normally: it returns or replies.
fn exits(e: &Expr) -> bool {
    match e {
        Expr::Reply(..) => true,
        Expr::Block(stmts) => match stmts.last() {
            Some(Stmt::Return(..)) => true,
            Some(Stmt::Expr(x)) => exits(x),
            _ => false,
        },
        _ => false,
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
            Seg::Param(n, Some(t), p) => s.push_str(&format!("{}:{}({})", p, n, t)),
            Seg::Param(n, None, p) => s.push_str(&format!("{}:{}", p, n)),
            Seg::Rest(n) => s.push_str(&format!("*{}", n)),
        }
    }
    if s.is_empty() {
        s.push('/');
    }
    s
}

/// A value of no known type before `||`, as its fallback describes it:
/// `request.query.name || "anonymous"` wants text and `row.n || 10` a number,
/// and those are where a blank field or a count of 0 is kept without a word.
/// `""` and `0` are left out, as both languages then give the same answer
/// (FEEDBACK19.md, Q-150; branch feedback19-fixes).
fn fallback_says(left: T, fallback: &Expr) -> T {
    if !left.is_any() {
        return left;
    }
    match fallback {
        Expr::Lit(Lit::Str(s)) if !s.is_empty() => T::of(STR | NIL),
        Expr::Template(_) => T::of(STR | NIL),
        Expr::Lit(Lit::Int(n)) if *n != 0 => T::of(NUM | NIL),
        Expr::Lit(Lit::Float(f)) if *f != 0.0 => T::of(NUM | NIL),
        Expr::Lit(Lit::Dec(d)) if !d.is_zero() => T::of(DEC | NIL),
        _ => left,
    }
}

/// `a.b.c` as it is written, for a name or a chain of fields on one.
fn dotted(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n, _) => Some(n.to_string()),
        Expr::Field(o, n, _) => Some(format!("{}.{}", dotted(o)?, n)),
        _ => None,
    }
}

/// A file parsed again, with its names as they are called in the program.
fn reparsed(src: &str, names: &HashMap<String, String>) -> Result<Vec<Stmt>, crate::error::QuilloError> {
    let mut stmts = Parser::new(src).parse_program()?;
    crate::privacy::rename_as(&mut stmts, names, "a name another file has too")?;
    Ok(stmts)
}

/// `alias` as a name a type can have: `P` and `PT` are `Pt` (Q-821).
pub(crate) fn type_like(alias: &str) -> String {
    let mut cs = alias.chars();
    let first = cs.next().map_or('T', |c| c.to_ascii_uppercase());
    let rest: String = cs.map(|c| c.to_ascii_lowercase()).collect();
    if rest.chars().any(|c| c.is_ascii_lowercase()) { format!("{}{}", first, rest) } else { format!("{}{}t", first, rest) }
}

/// The line a top-level statement starts on, when the parser recorded one (Q-818).
fn top_line(s: &Stmt) -> Option<u32> {
    let l = match s {
        Stmt::Fn(d) => d.line,
        Stmt::Route(r) => r.line,
        Stmt::Type(t) => t.line,
        Stmt::Return(_, l) | Stmt::Mount(_, _, l) | Stmt::Service(_, _, _, _, l) => *l,
        s => crate::debug::stmt_line(s).unwrap_or(0),
    };
    (l > 0).then_some(l)
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
        Expr::Ident(n, _) => n.to_string(),
        Expr::Field(_, n, _) => n.clone(),
        _ => "this value".into(),
    }
}

/// Names called in a body (not inside nested function definitions'
/// declarations, but including lambdas, which usually run during the call).
fn called_names(e: &Expr, out: &mut Vec<String>) {
    walk_expr(e, &mut |x| match x {
        Expr::Call(c, _, _) => match &**c {
            Expr::Ident(n, _) => out.push(n.to_string()),
            Expr::Field(obj, n, _) => match &**obj {
                Expr::Ident(m, _) if MODULES.contains(&m.as_str()) => out.push(format!("{}.{}", m, n)),
                // `billing.charge()` may be a service's, a call over HTTP, or a
                // method: the name of what it is called on goes along for
                // `fn_effects` to tell (FEEDBACK16.md, Q-126; branch feedback20-fixes).
                Expr::Ident(m, _) => {
                    out.push(n.clone());
                    out.push(format!("{}.{}", m, n));
                }
                _ => out.push(n.clone()),
            },
            _ => {}
        },
        Expr::Pipe(_, rhs, _) => match &**rhs {
            Expr::Ident(n, _) => out.push(n.to_string()),
            Expr::Field(obj, n, _) => {
                if let Expr::Ident(m, _) = &**obj {
                    out.push(format!("{}.{}", m, n));
                }
            }
            _ => {}
        },
        _ => {}
    });
}

/// Calls `f` on every expression inside `e`, including `e`.
/// The classes the program's sheets style (`css`...``, and `.css` files
/// under public/) that nothing in it can write: no text in its code or its
/// public scripts, no start of a template (`card-${size}` writes any
/// `card-...`), not set by Quillo, nor by a script a sheet names
/// (`/* set elsewhere: leaflet- */`). Each with where it is styled.
/// A style sheet's text as written: a string, or a template with its
/// `${...}` as a space.
fn sheet_text(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(Lit::Str(s)) => Some(s.to_string()),
        Expr::Template(parts) => Some(parts.iter().map(|p| if let TPart::Lit(t) = p { t.as_str() } else { " " }).collect()),
        _ => None,
    }
}

/// Every class a rule is written for, and where: in `css(...)`, in a
/// component's or view's `style`, and in the CSS files under public/
/// (branch class-links).
fn class_rules(order: &[Item], base: &Path) -> Vec<(String, String, u32)> {
    let mut sheets: Vec<(Arc<str>, u32, String)> = Vec::new();
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| {
            if let Expr::Call(f, args, line) = e {
                if matches!(&**f, Expr::Ident(n, _) if n == "css" || n == "__style") {
                    if let Some(text) = args.first().and_then(sheet_text) {
                        sheets.push((it.file.clone(), *line, text));
                    }
                }
            }
        });
    }
    fn css_files(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 8 {
                css_files(&p, out, depth + 1);
            } else if p.extension().is_some_and(|x| x == "css") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    css_files(&base.join("public"), &mut files, 0);
    files.sort();
    for p in files {
        if let Ok(text) = std::fs::read_to_string(&p) {
            sheets.push((Arc::from(p.display().to_string()), 1, text));
        }
    }
    let mut out = Vec::new();
    for (file, line, text) in &sheets {
        for (class, k) in crate::styles::classes_in(text) {
            out.push((class, file.to_string(), line + k));
        }
    }
    out.dedup();
    out
}

/// The classes markup writes as they are written (`<span.chip>`,
/// `class="a b"`, the strings in `class={...}`), with the file and the
/// line of their element. A class put together (`card-${size}`) is not.
fn class_uses(order: &[Item]) -> Vec<(String, Arc<str>, u32)> {
    // The strings that are the value: not those given to a function on
    // the way (`replace(state, " ", "-")`).
    fn words(e: &Expr, out: &mut Vec<String>) {
        match e {
            Expr::Lit(Lit::Str(s)) => out.extend(s.split_whitespace().map(str::to_string)),
            Expr::If(_, a, b) => {
                words(a, out);
                if let Some(b) = b {
                    words(b, out)
                }
            }
            Expr::Match(_, arms, _) => arms.iter().for_each(|a| words(&a.body, out)),
            // `if on { "a" } else { "b" }`, as `on ? "a" : "b"` (FEEDBACK16.md, Q-128;
            // branch feedback20-fixes).
            Expr::Block(stmts) => {
                if let Some(Stmt::Expr(x)) = stmts.last() {
                    words(x, out)
                }
            }
            Expr::Binary(BinOp::And, _, b, _) => words(b, out),
            Expr::Binary(BinOp::Or, a, b, _) | Expr::Coalesce(a, b) => {
                words(a, out);
                words(b, out)
            }
            Expr::List(xs) => xs.iter().for_each(|x| words(x, out)),
            Expr::Template(parts) => {
                for (k, p) in parts.iter().enumerate() {
                    let TPart::Lit(t) = p else { continue };
                    let ws: Vec<&str> = t.split(char::is_whitespace).collect();
                    for (i, w) in ws.iter().enumerate() {
                        let glued = (i == 0 && k > 0) || (i + 1 == ws.len() && k + 1 < parts.len());
                        if !w.is_empty() && !glued {
                            out.push(w.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn el_uses(el: &HtmlEl, file: &Arc<str>, out: &mut Vec<(String, Arc<str>, u32)>) {
        let mut found = Vec::new();
        for c in &el.classes {
            if let Expr::Lit(Lit::Str(s)) = c {
                found.push(s.to_string());
            }
        }
        for (name, v) in &el.attrs {
            if let (AttrVal::Expr(e), "class") = (v, name.as_str()) {
                words(e, &mut found);
            }
        }
        out.extend(found.into_iter().map(|c| (c, file.clone(), el.line)));
        // Elements written inside `{...}` are reached as expressions.
        for c in &el.children {
            if let HtmlChild::El(child) = c {
                el_uses(child, file, out);
            }
        }
    }
    let mut out = Vec::new();
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| {
            if let Expr::Html(el, _) = e {
                el_uses(el, &it.file, &mut out);
            }
        });
    }
    out
}

/// Classes markup writes that no style sheet of the program has a rule for:
/// a misspelt class, or a rule since removed (branch class-tools). Not
/// told when the program has no sheets of its own or links one from
/// elsewhere, for a class Quillo writes, or for one its scripts use.
fn unstyled_classes(order: &[Item], base: &Path, rules: &[(String, String, u32)]) -> Vec<(Arc<str>, u32, Severity, String)> {
    if rules.is_empty() {
        return Vec::new();
    }
    let uses = class_uses(order);
    let mut linked = false;
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| {
            if let Expr::Html(el, _) = e {
                walk_html_els(el, &mut |el| {
                    if el.tag == "link" {
                        for (n, v) in &el.attrs {
                            if let (AttrVal::Expr(Expr::Lit(Lit::Str(h))), "href") = (v, n.as_str()) {
                                linked |= h.starts_with("http") || h.starts_with("//");
                            }
                        }
                    }
                });
            }
        });
    }
    if linked {
        return Vec::new();
    }
    // What the app's scripts and sheets say of classes they set themselves.
    let mut scripts = String::new();
    let mut elsewhere = Vec::new();
    fn files(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 8 {
                files(&p, out, depth + 1);
            } else {
                out.push(p);
            }
        }
    }
    let mut public = Vec::new();
    files(&base.join("public"), &mut public, 0);
    for p in public {
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        match ext {
            "js" | "mjs" => scripts.push_str(&text),
            "css" => elsewhere.extend(crate::styles::set_elsewhere(&text)),
            _ => {}
        }
    }
    let script_words: HashSet<&str> = scripts.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).collect();
    let styled: HashSet<&str> = rules.iter().map(|(n, _, _)| n.as_str()).collect();
    let mut out = Vec::new();
    let mut said = HashSet::new();
    for (class, file, line) in uses {
        if styled.contains(class.as_str())
            || crate::styles::set_by_quillo(&class)
            || elsewhere.iter().any(|p| class.starts_with(p.as_str()))
            || script_words.contains(class.as_str())
            || !class.trim_start_matches('-').starts_with(|c: char| c.is_alphabetic() || c == '_')
            || !said.insert((class.clone(), file.clone(), line))
        {
            continue;
        }
        // A near miss is likely a typo; a class with no rule and none like
        // it, a name the markup carries for its own sake (`.dashboard`).
        let most = (class.chars().count() / 4).clamp(1, 2);
        let near = styled.iter().filter(|n| edit_distance(n, &class) <= most).min_by_key(|n| (edit_distance(n, &class), **n));
        match near {
            Some(n) => out.push((file, line, Severity::Warning, format!("no style sheet has a rule for `.{}` (did you mean `{}`?)", class, n))),
            None => out.push((file, line, Severity::Hint, format!("no style sheet has a rule for `.{}`", class))),
        }
    }
    out
}

/// Each element of `el`, itself first, as far as its children go (not
/// those inside `{...}`).
fn walk_html_els(el: &HtmlEl, f: &mut dyn FnMut(&HtmlEl)) {
    f(el);
    for c in &el.children {
        if let HtmlChild::El(child) = c {
            walk_html_els(child, f);
        }
    }
}

fn unused_classes(order: &[Item], base: &Path, unparsed: &[String]) -> Vec<(Arc<str>, u32, String)> {
    let mut sheets: Vec<(Arc<str>, u32, String)> = Vec::new();
    let mut sheet_args: HashSet<*const Expr> = HashSet::new();
    let mut written: HashSet<String> = HashSet::new();
    let mut starts: HashSet<String> = HashSet::new();
    let words = |s: &str, into: &mut HashSet<String>| {
        for w in s.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).filter(|w| !w.is_empty()) {
            into.insert(w.to_string());
        }
    };
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| match e {
            Expr::Call(f, args, line) if matches!(&**f, Expr::Ident(n, _) if n == "css") && args.len() == 1 => {
                if let Some(text) = sheet_text(&args[0]) {
                    sheet_args.insert(&args[0] as *const Expr);
                    sheets.push((it.file.clone(), *line, text));
                }
            }
            Expr::Lit(Lit::Str(s)) if !sheet_args.contains(&(e as *const Expr)) => words(s, &mut written),
            // `"denied" => ...`: a value the program knows, which it may write.
            Expr::Match(_, arms, _) => {
                fn texts(p: &Pattern, out: &mut Vec<String>) {
                    match p {
                        Pattern::Lit(Lit::Str(s)) => out.push(s.to_string()),
                        Pattern::Alt(ps) | Pattern::List(ps, _) => ps.iter().for_each(|q| texts(q, out)),
                        Pattern::As(q, _) => texts(q, out),
                        Pattern::Record(fs) => fs.iter().for_each(|f| texts(&f.pat, out)),
                        _ => {}
                    }
                }
                let mut found = Vec::new();
                arms.iter().for_each(|a| texts(&a.pat, &mut found));
                found.iter().for_each(|s| words(s, &mut written));
            }
            Expr::Template(parts) if !sheet_args.contains(&(e as *const Expr)) => {
                for (k, p) in parts.iter().enumerate() {
                    let TPart::Lit(t) = p else { continue };
                    words(t, &mut written);
                    // `card-${size}`: any class starting `card-` may be written.
                    if matches!(parts.get(k + 1), Some(TPart::Expr(_))) {
                        let tail: String = t.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
                        if !tail.is_empty() {
                            starts.insert(tail.chars().rev().collect());
                        }
                    }
                }
            }
            _ => {}
        });
    }
    // A file that does not parse may write any class its text holds
    // (FEEDBACK13.md, Q-092; branch feedback13-fixes).
    for src in unparsed {
        words(src, &mut written);
    }
    // What the app's services send it is theirs to say: their text counts.
    for it in order {
        if let Stmt::Service(_, _, path, _, _) = &it.stmt {
            if let Ok(src) = std::fs::read_to_string(base.join(path)) {
                words(&src, &mut written);
            }
        }
    }
    // The app's own files under public/: its sheets, and what its scripts write.
    fn files(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 8 {
                files(&p, out, depth + 1);
            } else {
                out.push(p);
            }
        }
    }
    let mut public = Vec::new();
    files(&base.join("public"), &mut public, 0);
    public.sort();
    for p in public {
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        match ext {
            "css" => sheets.push((Arc::from(p.display().to_string()), 1, text)),
            "js" | "mjs" => words(&text, &mut written),
            _ => {}
        }
    }
    let elsewhere: Vec<String> = sheets.iter().flat_map(|(_, _, t)| crate::styles::set_elsewhere(t)).collect();
    let mut out = Vec::new();
    let mut said = HashSet::new();
    for (file, line, text) in &sheets {
        for (class, k) in crate::styles::classes_in(text) {
            if written.contains(&class)
                || crate::styles::set_by_quillo(&class)
                || starts.iter().any(|s| class.starts_with(s.as_str()))
                || elsewhere.iter().any(|s| class.starts_with(s.as_str()))
                || !said.insert((class.clone(), file.clone(), line + k))
            {
                continue;
            }
            out.push((file.clone(), line + k, format!("`.{}` is styled here, but nothing in the program writes it", class)));
        }
    }
    out
}

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
        Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
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
            Stmt::SetField(o, _, _, e, _) => {
                walk_expr(o, f);
                walk_expr(e, f)
            }
            Stmt::Return(Some(e), _) => walk_expr(e, f),
            Stmt::Fn(d) => walk_expr(&d.body, f),
            Stmt::Route(r) => walk_expr(&r.body, f),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Return(None, _) => {}
            Stmt::Type(td) => td.defaults.iter().for_each(|(_, e)| walk_expr(e, f)),
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

/// Assignments to what is a global where they are written: each function's parameters and
/// the names it binds are its own. The browser's compile asks the same, so that `quillo run`
/// and `quillo check` agree (FEEDBACK28.md, Q-348; branch feedback29-fixes).
pub(crate) fn global_assigned(stmts: &[Stmt], out: &mut HashSet<String>) {
    // What `e` assigns that it does not bind itself: a `let`, a `state`, a lambda's
    // parameter, a `for`'s or a `match` arm's names, anywhere in it.
    fn in_expr(e: &Expr, mut locals: HashSet<String>, out: &mut HashSet<String>) {
        walk_expr(e, &mut |x| {
            let mut names = Vec::new();
            match x {
                Expr::Block(stmts) => {
                    for s in stmts {
                        match s {
                            Stmt::Let(p, ..) => pattern_names(p, &mut names),
                            Stmt::State(n, ..) => names.push(n.clone()),
                            _ => {}
                        }
                    }
                }
                Expr::Lambda(d) => names.extend(d.params.iter().map(|p| p.name.to_string())),
                Expr::For(p, ..) => pattern_names(p, &mut names),
                Expr::Match(_, arms, _) => arms.iter().for_each(|a| pattern_names(&a.pat, &mut names)),
                _ => {}
            }
            locals.extend(names);
        });
        let mut inner = HashSet::new();
        assigned_in(e, &mut inner);
        out.extend(inner.into_iter().filter(|n| !locals.contains(n)));
    }
    for s in stmts {
        match s {
            Stmt::Assign(n, _, e, _) => {
                out.insert(n.to_string());
                in_expr(e, HashSet::new(), out);
            }
            Stmt::Fn(d) => in_expr(&d.body, d.params.iter().map(|p| p.name.to_string()).collect(), out),
            Stmt::Route(r) => in_expr(&r.body, HashSet::new(), out),
            Stmt::Let(_, e, _) | Stmt::Expr(e) => in_expr(e, HashSet::new(), out),
            _ => {}
        }
    }
}

fn collect_assigned(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign(n, op, e, _) => {
                out.insert(n.to_string());
                // `=` too, apart: `+=` and `-=` alone keep a kind (Checker::note_first_t).
                if matches!(op, AssignOp::Set) {
                    out.insert(format!("={}", n));
                }
                assigned_in(e, out)
            }
            Stmt::Let(_, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => assigned_in(e, out),
            Stmt::SetField(o, _, _, e, _) => {
                assigned_in(o, out);
                assigned_in(e, out)
            }
            Stmt::Return(Some(e), _) => assigned_in(e, out),
            Stmt::Fn(d) => assigned_in(&d.body, out),
            Stmt::Route(r) => assigned_in(&r.body, out),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) | Stmt::Return(None, _) => {}
        }
    }
}

/// The names `e` reads whenever it runs: not inside a function it makes (which may run later,
/// or never), nor in a branch of `if`, `match`, `&&`, `||` or `??` that may not be taken
/// (FEEDBACK45.md, Q-514; found in review). Names it binds go in `bound`.
fn sure_reads(e: &Expr, bound: &mut HashSet<String>, out: &mut Vec<String>) {
    match e {
        Expr::Ident(n, _) => out.push(n.to_string()),
        Expr::Lambda(_) => {}
        Expr::Block(stmts) => {
            for st in stmts {
                match st {
                    Stmt::Let(p, v, _) => {
                        sure_reads(v, bound, out);
                        let mut names = Vec::new();
                        pattern_names(p, &mut names);
                        bound.extend(names);
                    }
                    Stmt::Fn(f) => {
                        bound.insert(f.name.clone());
                    }
                    Stmt::State(n, v, _) => {
                        sure_reads(v, bound, out);
                        bound.insert(n.to_string());
                    }
                    Stmt::Expr(x) | Stmt::Assign(_, _, x, _) | Stmt::Return(Some(x), _) => sure_reads(x, bound, out),
                    _ => {}
                }
            }
        }
        Expr::If(c, ..) => sure_reads(c, bound, out),
        Expr::Match(subj, ..) => sure_reads(subj, bound, out),
        Expr::Binary(BinOp::And | BinOp::Or, a, _, _) => sure_reads(a, bound, out),
        Expr::Binary(_, a, b, _) => {
            sure_reads(a, bound, out);
            sure_reads(b, bound, out);
        }
        Expr::Unary(_, x, _) | Expr::Field(x, _, _) => sure_reads(x, bound, out),
        Expr::Call(f, args, _) => {
            sure_reads(f, bound, out);
            args.iter().for_each(|a| sure_reads(a, bound, out));
        }
        Expr::List(xs) => xs.iter().for_each(|x| sure_reads(x, bound, out)),
        Expr::Map(entries) => entries.iter().for_each(|(_, v)| sure_reads(v, bound, out)),
        _ => {}
    }
}

/// The type a name is a slip of: one letter off, or two swapped (`intt`, `itn`, `strr`,
/// `lsit`). Not a word a program names things by (`data`, `body`, `key`): found in review.
fn type_typo(name: &str) -> Option<&'static str> {
    if name.chars().count() < 3 || matches!(name, "data" | "date" | "body" | "key" | "name" | "val" | "item" | "list" | "map") {
        return None;
    }
    crate::parser::TYPE_NAMES.iter().copied().find(|t| osa_distance(name, t) == 1)
}

/// What another language calls something Quillo has (FEEDBACK45.md, Q-517).
fn other_language(name: &str) -> Option<&'static str> {
    Some(match name {
        "None" | "undefined" | "nil" | "NULL" | "Nothing" => "null",
        "True" | "TRUE" => "true",
        "False" | "FALSE" => "false",
        "parseInt" | "Number" | "parseFloat" | "Integer" => "int",
        "String" | "toString" => "str",
        "toUpperCase" => "upper",
        "toLowerCase" => "lower",
        "length" | "size" | "count_of" => "len",
        "integer" => "int",
        "string" => "str",
        "boolean" => "bool",
        "puts" | "println" | "printf" | "echo" => "print",
        "where" => "filter",
        "console" => "print",
        "JSON" => "json",
        "Math" => "math",
        _ => return None,
    })
}

/// The candidate nearest `name`, by edit distance with a swap of two letters one edit (`lne`,
/// `pirnt`), case aside: at most 2 edits, fewer than `name` has letters. Ties go to the same
/// first letter, then the alphabet, so the answer is the same every run (FEEDBACK45.md, Q-517;
/// branch feedback40-43).
fn closest<'a>(name: &str, cands: impl Iterator<Item = &'a str>) -> Option<String> {
    let lower = name.to_lowercase();
    let first = lower.chars().next();
    let mut best: Option<(usize, bool, String)> = None;
    for c in cands {
        let lc = c.to_lowercase();
        if lc == lower {
            continue;
        }
        let d = osa_distance(&lower, &lc);
        if d > 2 || d >= name.chars().count() {
            continue;
        }
        let other_first = lc.chars().next() != first;
        let key = (d, other_first, c.to_string());
        if best.as_ref().map_or(true, |b| key < *b) {
            best = Some(key);
        }
    }
    best.map(|(_, _, c)| c)
}

/// Levenshtein distance where two letters swapped are one edit (optimal string alignment).
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[a.len()][b.len()]
}

fn assigned_in(e: &Expr, out: &mut HashSet<String>) {
    walk_expr(e, &mut |x| {
        if let Expr::Block(stmts) = x {
            for s in stmts {
                if let Stmt::Assign(n, op, ..) = s {
                    out.insert(n.to_string());
                    if matches!(op, AssignOp::Set) {
                        out.insert(format!("={}", n));
                    }
                }
            }
        }
    });
}

/// `db.connect(...)`, or that with `?`.
fn is_connect(e: &Expr) -> bool {
    match e {
        Expr::Try(x) => is_connect(x),
        Expr::Call(f, _, _) => matches!(&**f, Expr::Field(o, n, _) if n == "connect" && matches!(&**o, Expr::Ident(m, _) if m == "db")),
        _ => false,
    }
}

/// For an error in a call of the program's own function named like a
/// builtin (`notify`, `first`): says that it is not the builtin.
fn hides(sig: &Sig, name: &str) -> String {
    match &sig.def {
        Some(d) if !sig.builtin && BUILTINS.iter().any(|(n, _, _)| *n == name) => {
            format!(" (this `{}` is the program's own, from line {}, which hides the builtin `{}`)", name, d.line, name)
        }
        _ => String::new(),
    }
}

/// Whether a map's key is written bare in a path, as `repr` writes it (`a`, `é`), not `["a b"]`.
fn fmt_key_bare(k: &str) -> bool {
    k.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && k.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn is_join(f: &Expr) -> bool {
    matches!(f, Expr::Ident(n, _) if n == "join")
}

/// The type a type-testing pattern accepts: `int | null` is int or null.
fn type_test_t(p: &Pattern) -> T {
    match p {
        Pattern::Type(ty) => ty_t(&Ty { names: vec![ty.clone()], optional: false, elem: None, fields: None }),
        Pattern::Lit(Lit::Nil) => T::of(NIL),
        Pattern::Alt(ps) => ps.iter().fold(T::NEVER, |t, q| t.union(type_test_t(q))),
        _ => T::ANY,
    }
}

pub(crate) fn pattern_names(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Bind(n) => out.push(n.to_string()),
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

/// Edits from `a` to `b`, two letters swapped one of them (`pirnt`, `lne`), so a swap is as near
/// as a letter left out (FEEDBACK45.md, Q-517; branch feedback40-43).
pub fn edit_distance(a: &str, b: &str) -> usize {
    osa_distance(a, b)
}

/// The names a file seems to define at its top level, read from its text
/// line by line: what is left when it cannot be parsed (`fn name`, `server
/// fn name`, `view Name`, `component Name`, `let name`, `type Name`).
fn names_in_text(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in src.lines() {
        let mut words = line.split(|c: char| c.is_whitespace() || c == '(' || c == '=' || c == '<' || c == ':')
            .filter(|w| !w.is_empty());
        let mut first = words.next();
        if first == Some("server") {
            first = words.next();
        }
        if !matches!(first, Some("fn" | "view" | "component" | "let" | "type")) {
            continue;
        }
        if let Some(name) = words.next() {
            if name.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Whether a function's body only gives SQL-safe text: its result and each
/// `return` is `sql_name(...)` or `sql_param(...)` (whatever their
/// arguments), text as written, a call of a function in `safe`, or a
/// variable bound to one of those in the same block; or choices and joins
/// of them.
fn gives_sql_safe(body: &Expr, safe: &HashSet<String>) -> bool {
    fn ok(e: &Expr, safe: &HashSet<String>, vars: &HashSet<String>) -> bool {
        match e {
            Expr::Lit(Lit::Str(_)) => true,
            Expr::Ident(n, _) => vars.contains(n.as_str()),
            Expr::Call(f, _, _) => matches!(&**f, Expr::Ident(n, _) if n == "sql_name" || n == "sql_param" || safe.contains(n.as_str()))
                || matches!(&**f, Expr::Field(o, n, _) if n == "sql_name" && matches!(&**o, Expr::Ident(m, _) if m == "db")),
            Expr::Template(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_)) || matches!(p, TPart::Expr(x) if ok(x, safe, vars))),
            Expr::Binary(BinOp::Add, a, b, _) => ok(a, safe, vars) && ok(b, safe, vars),
            Expr::If(_, a, b) => ok(a, safe, vars) && b.as_deref().map_or(false, |b| ok(b, safe, vars)),
            Expr::Match(_, arms, _) => !arms.is_empty() && arms.iter().all(|a| ok(&a.body, safe, vars)),
            Expr::Block(stmts) => {
                let vars = block_vars(stmts, safe, vars);
                match stmts.last() {
                    Some(Stmt::Expr(x)) => ok(x, safe, &vars),
                    Some(Stmt::Return(Some(x), _)) => ok(x, safe, &vars),
                    _ => false,
                }
            }
            _ => false,
        }
    }
    // The variables a block binds to safe text, up to its end.
    fn block_vars(stmts: &[Stmt], safe: &HashSet<String>, outer: &HashSet<String>) -> HashSet<String> {
        let mut vars = outer.clone();
        for st in stmts {
            if let Stmt::Let(Pattern::Bind(n), x, _) = st {
                if ok(x, safe, &vars) {
                    vars.insert(n.to_string());
                } else {
                    vars.remove(n.as_str());
                }
            }
        }
        vars
    }
    // Every `return`, wherever it is (in an `if`, a loop, after `??`), with
    // what its own block bound before it.
    let mut returns_ok = true;
    walk_expr(body, &mut |x| {
        if let Expr::Block(stmts) = x {
            let mut vars = HashSet::new();
            for st in stmts {
                match st {
                    Stmt::Let(Pattern::Bind(n), v, _) => {
                        if ok(v, safe, &vars) {
                            vars.insert(n.to_string());
                        } else {
                            vars.remove(n.as_str());
                        }
                    }
                    Stmt::Return(Some(r), _) if !ok(r, safe, &vars) => returns_ok = false,
                    _ => {}
                }
            }
        }
    });
    returns_ok && ok(body, safe, &HashSet::new())
}

/// Elements before which the HTML parser ends an open `<p>` ("close a p
/// element", in the HTML standard's parsing rules).
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "details", "dialog", "dir", "div", "dl", "dd", "dt",
    "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup",
    "hr", "li", "listing", "main", "menu", "nav", "ol", "p", "pre", "search", "section", "summary", "table", "ul",
    "xmp",
];

/// Elements past which an outer `<p>` is out of the parser's reach.
const P_SCOPE_ENDS: &[&str] = &["button", "table", "td", "th", "caption", "template", "object", "marquee", "applet", "svg", "math"];

/// The elements an expression puts on the page where it stands: itself,
/// either branch of `if` or `? :`, each `match` arm, a `for`'s body, a
/// block's last value, the right of `&&`, `||` and `??`, a list's items.
fn markup_roots<'a>(e: &'a Expr, out: &mut Vec<&'a HtmlEl>) {
    match e {
        Expr::Html(el, _) => out.push(el),
        Expr::If(_, a, b) => {
            markup_roots(a, out);
            if let Some(b) = b {
                markup_roots(b, out)
            }
        }
        Expr::Match(_, arms, _) => arms.iter().for_each(|a| markup_roots(&a.body, out)),
        Expr::For(_, _, body, _) => markup_roots(body, out),
        Expr::Block(stmts) => {
            if let Some(Stmt::Expr(x)) = stmts.last() {
                markup_roots(x, out)
            }
        }
        Expr::Binary(BinOp::And | BinOp::Or, _, b, _) => markup_roots(b, out),
        Expr::Coalesce(a, b) => {
            markup_roots(a, out);
            markup_roots(b, out)
        }
        Expr::List(items) => items.iter().for_each(|x| markup_roots(x, out)),
        _ => {}
    }
}

/// The outer elements' tags a view or component renders (a view whose
/// outer element is another view's, that one's), at most a few deep.
fn outer_tags(name: &str, views: &HashMap<&str, &FnDef>, depth: u32, out: &mut Vec<String>) {
    let Some(d) = views.get(name) else { return };
    if depth > 6 {
        return;
    }
    let mut roots = Vec::new();
    markup_roots(&d.body, &mut roots);
    for r in roots {
        if r.facts.component {
            outer_tags(&r.tag, views, depth + 1, out)
        } else {
            out.push(r.tag.to_ascii_lowercase())
        }
    }
}

/// Checks the elements inside `el` (whose tag, and its ancestors', are on
/// `stack`) against the rules of `Checker::check_nesting`.
fn nesting_in(
    el: &HtmlEl,
    stack: &mut Vec<String>,
    views: &HashMap<&str, &FnDef>,
    inside: &mut HashSet<*const HtmlEl>,
    report: &mut dyn FnMut(u32, String),
) {
    let mut kids: Vec<&HtmlEl> = Vec::new();
    for c in &el.children {
        match c {
            HtmlChild::El(k) => kids.push(k),
            HtmlChild::Expr(x) => {
                let mut roots = Vec::new();
                markup_roots(x, &mut roots);
                for r in roots {
                    inside.insert(r as *const HtmlEl);
                    kids.push(r);
                }
            }
        }
    }
    for k in kids {
        let tags = if k.facts.component {
            let mut t = Vec::new();
            outer_tags(&k.tag, views, 0, &mut t);
            t
        } else {
            vec![k.tag.to_ascii_lowercase()]
        };
        let problems: Vec<String> = tags.iter().filter_map(|t| nesting_problem(stack, t)).collect();
        // A view or component whose outer element depends on its arguments
        // (`link ? <a> : <span>`) counts only when every one it can be is wrong.
        if !problems.is_empty() && problems.len() == tags.len() {
            let what = if k.facts.component { format!("<{}>, whose outer element is a <{}>,", k.tag, tags[0]) } else { format!("<{}>", tags[0]) };
            report(k.line, format!("{} {}", what, problems[0]));
        }
        if !k.facts.component {
            // An element that ends the <p> is not inside it: what it holds
            // is checked without the <p> (a <ul>'s <li> is not wrong too).
            let tag = k.tag.to_ascii_lowercase();
            let mut inner: Vec<String> = stack.clone();
            if CLOSES_P.contains(&tag.as_str()) {
                if let Some(at) = inner.iter().rposition(|a| a == "p" || P_SCOPE_ENDS.contains(&a.as_str())) {
                    if inner[at] == "p" {
                        inner.truncate(at);
                    }
                }
            }
            inner.push(tag);
            nesting_in(k, &mut inner, views, inside, report);
        }
    }
}

/// What is wrong with an element `tag` inside the elements on `stack`, if
/// anything: the end of a message that starts by naming it.
fn nesting_problem(stack: &[String], tag: &str) -> Option<String> {
    let open_p = CLOSES_P.contains(&tag)
        && stack.iter().rev().take_while(|a| !P_SCOPE_ENDS.contains(&a.as_str())).any(|a| a == "p");
    if open_p {
        Some("cannot go inside a <p>: the browser ends the <p> before it, so the page is not what was written, and its components are made again in the browser; make the <p> a <div>, or use a <span>".into())
    } else if tag == "a" && stack.iter().any(|a| a == "a") {
        Some("cannot go inside an <a>: the browser ends the outer link before it, so the page is not what was written".into())
    } else if tag == "form" && stack.iter().any(|a| a == "form") {
        Some("cannot go inside a <form>: the browser drops the inner <form>, so its fields belong to the outer one".into())
    } else {
        None
    }
}

/// Whether a pattern has a group holding a run that is itself repeated.
/// Whether a pattern has a capturing group inside a group that repeats, as `(?:(a)|b)*`: there
/// JavaScript clears the group at each round and the server keeps its last match, so the two
/// sides give different groups (FEEDBACK25.md, Q-308; branch feedback27-35-fixes).
fn capture_in_repeat(p: &str) -> bool {
    let cs: Vec<char> = p.chars().collect();
    // For each open group: whether it captures, and whether a capturing group is inside it.
    let mut groups: Vec<(bool, bool)> = Vec::new();
    let (mut i, mut class) = (0, false);
    while i < cs.len() {
        match cs[i] {
            '\\' => i += 1,
            ']' if class => class = false,
            _ if class => {}
            '[' => class = true,
            '(' => {
                let named = cs.get(i + 1) == Some(&'?') && matches!(cs.get(i + 2), Some('<' | 'P')) && !matches!(cs.get(i + 3), Some('=' | '!'));
                let captures = cs.get(i + 1) != Some(&'?') || named;
                groups.push((captures, false));
            }
            ')' => {
                let (captures, holds) = groups.pop().unwrap_or((false, false));
                let repeats = matches!(cs.get(i + 1), Some('+' | '*' | '{'));
                if repeats && holds {
                    return true;
                }
                if let Some(g) = groups.last_mut() {
                    g.1 |= holds || captures;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn runs_in_runs(p: &str) -> bool {
    let cs: Vec<char> = p.chars().collect();
    let mut groups: Vec<bool> = Vec::new();
    let (mut i, mut class) = (0, false);
    while i < cs.len() {
        match cs[i] {
            '\\' => i += 1,
            ']' if class => class = false,
            _ if class => {}
            '[' => class = true,
            '(' => groups.push(false),
            '+' | '*' => {
                if let Some(g) = groups.last_mut() {
                    *g = true;
                }
            }
            ')' => {
                let held = groups.pop().unwrap_or(false);
                if held && matches!(cs.get(i + 1), Some('+' | '*')) {
                    return true;
                }
                if let Some(g) = groups.last_mut() {
                    *g |= held;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}
