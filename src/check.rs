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
        "nil" | "null" => NIL,
        other => BASE.iter().find(|(_, x)| *x == other).map_or(ANY, |(b, _)| *b),
    }
}

/// A money value's type (src/money.rs).
fn money_t() -> T {
    map_of(vec![(Arc::from("amount"), T::of(DEC)), (Arc::from("currency"), T::of(STR))])
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
                    (INT | FLOAT, INT | FLOAT) | (STR, STR) | (DEC, DEC | INT) | (INT, DEC) => Some(BOOL),
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
    ("dec", "any", "dec | error(str)"),
    ("format_number", "num | dec, str | null=, int=", "str"),
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
    ("set", "map | list | null, any, any=", "map | list"),
    ("remove", "list | map | null, any", "list | map"),
    ("merge", "map | null, map | null", "map"),
    ("__merge", "map | null...", "map"),
    ("__concat", "list | null...", "list"),
    ("err", "any=", "error(any)"),
    ("is_err", "any", "bool"),
    ("unwrap", "any", "any"),
    ("raw", "any", "html"),
    ("escape", "any", "str"),
    ("url_encode", "any", "str"),
    ("env", "str, any=", "any"),
    ("now", "", "int"),
    ("sleep", "num", "null"),
    ("file", "str, map | null=", "response"),
    ("redirect", "str, int=", "response"),
    ("respond", "int, any=, map | null=", "response"),
    ("read_file", "str", "str | error(str)"),
    ("write_file", "str, any", "null | error(str)"),
    ("delete_file", "str", "null | error(str)"),
    ("check_program", "str, map | null=", "list | error(str)"),
    ("random", "", "float"),
    ("random_int", "int, int", "int"),
    ("uuid", "", "str"),
    ("abs", "num | dec", "num | dec"),
    ("round", "num | dec, int=, str=", "num | dec"),
    ("floor", "num | dec", "num | dec"),
    ("ceil", "num | dec", "num | dec"),
    ("sqrt", "num", "float"),
    ("pow", "num, num", "num"),
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
    ("tenant", "", "str | null"),
    ("tenant_from", "str | fn", "null"),
    ("with_tenant", "str, fn", "any"),
    ("save_upload", "map, str", "str | error(str)"),
    ("read_upload", "map", "str | error(str)"),
    ("js", "str, any...", "any"),
    ("js_new", "str, any...", "any"),
    ("live", "fn, any...", "any"),
    ("parallel", "any...", "list"),
    ("every", "num, fn", "null"),
    ("stream", "fn, map | null=", "response"),
    ("events", "fn, map | null=", "response"),
    ("listen", "str, fn", "null"),
    ("receive", "str, fn, map | null=", "null | error(str)"),
    ("url", "", "map"),
    ("css", "str", "str"),
    ("__style", "str", "null"),
    ("sql", "str", "str"),
    ("navigate", "str", "null"),
    ("publish", "any...", "null"),
    ("watch", "any...", "null"),
    ("unwatched", "fn", "any"),
    ("json.parse", "str", "any"),
    ("json.stringify", "any, any=", "str"),
    ("db.open", "str | fn, map=", "null | error(str)"),
    ("db.connect", "str | fn, map=", "map | error(str)"),
    ("db.system", "str", "null | error(str)"),
    ("db.shards", "list", "map | error(str)"),
    ("db.atomic", "list, fn", "any"),
    ("db.exec", "str, any...", "map | error(str)"),
    ("db.query", "str, any...", "list | error(str)"),
    ("db.one", "str, any...", "map | null | error(str)"),
    ("db.transaction", "map | fn, fn=", "any"),
    ("db.error_kind", "any", "str | null"),
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
    ("date.valid", "any", "bool"),
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
    ("date.at", "str, str | null=, str | null=", "str"),
    ("date.format", "str, str | map", "str"),
    ("date.ms", "str", "int"),
    ("date.from_ms", "int, str | null=", "str"),
    ("media.rtp", "str, map", "null"),
    ("media.close", "str", "bool"),
    ("media.on", "str, map | null=", "null"),
    ("media.streams", "", "list"),
    // src/money.rs
    ("money.of", "dec | int | str, str, str=", "map | error(str)"),
    ("money.from_minor", "int, str", "map | error(str)"),
    ("money.to_minor", "map", "int"),
    ("money.add", "map, map", "map"),
    ("money.sub", "map, map", "map"),
    ("money.mul", "map, dec | int, str=", "map"),
    ("money.neg", "map", "map"),
    ("money.compare", "map, map", "int"),
    ("money.zero", "str", "map | error(str)"),
    ("money.is_zero", "map", "bool"),
    ("money.allocate", "map, list | int", "list"),
    ("money.round", "map, str=", "map"),
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

/// Modules the browser has too (src/client/runtime.js).
const BROWSER_MODULES: &[&str] = &["json", "math", "stats", "vec", "date", "money", "re"];

pub const MODULES: &[&str] = &["json", "db", "http", "udp", "channel", "media", "crypto", "image", "math", "stats", "vec", "date", "money", "re"];

/// Builtins the browser has (see client.rs).
const BROWSER_ONLY_SERVER: &[&str] =
    &["env", "sleep", "file", "redirect", "respond", "read_file", "write_file", "delete_file", "check_program", "publish", "watch", "unwatched", "crypto.sha256",
      "crypto.hmac", "crypto.token", "crypto.hash_password", "crypto.verify_password", "crypto.equal", "set_cookie",
      "delete_cookie", "session", "cookie", "set_session", "end_session", "translations", "locale_from", "with_locale",
      "schedule", "stream", "events", "image.info", "image.convert", "allow_cross_site", "tenant_from", "with_tenant", "save_upload", "read_upload"];

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
            ps.push(PSig { name: format!("arg{}", i + 1), ty: t.bits, full: t, optional: optional || variadic, annotated: true });
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
                    name: p.name.clone(),
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
    /// Where a file could not be parsed for syntax this Quillo does not
    /// know (file, line): perhaps it is for a newer one. Errors that name
    /// the rule broken (a keyword as a name, say) are not here.
    pub syntax_errors: Vec<(String, u32)>,
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
    let main = overrides.get(path).cloned().map_or_else(|| std::fs::read_to_string(path), Ok);
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
    c.overrides = overrides;
    let main: Arc<str> = Arc::from(file);
    match Parser::new(src).parse_program() {
        Ok(stmts) => {
            let mut seen = HashSet::new();
            if let Ok(p) = Path::new(file).canonicalize() {
                seen.insert(p);
            }
            let mut order = Vec::new();
            c.expand(Arc::new(stmts), main, base_dir, &mut seen, &mut order);
            if opts.css {
                for (file, line, msg) in unused_classes(&order, base_dir) {
                    c.diag_at(file, line, Severity::Warning, msg);
                }
            }
            c.run(order);
            c.state_details();
        }
        Err(e) => {
            if crate::error::may_be_newer_syntax(&e.msg) {
                c.out.syntax_errors.push((main.to_string(), e.line));
            }
            c.diag_at(main, e.line, Severity::Error, e.msg)
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
    ret_cache: HashMap<usize, T>,
    in_progress: HashSet<usize>,
    effect_cache: HashMap<usize, u8>,
    /// Functions of `.qwer` files the program uses, with their files.
    natives: Vec<(Arc<str>, crate::native::Export)>,
    /// Each `state` (by its file, line and name): the types it takes (its
    /// first value's, and what is assigned to it), and the references to it,
    /// whose hover text says so once the program is checked.
    state_types: HashMap<(Arc<str>, u32, String), (T, Vec<usize>)>,
    /// Checking the `live(...)` that sets a component's state.
    live_ok: bool,
    /// The services a program calls (`service billing = "billing.qwe"`): the
    /// server fns each offers, by name.
    services: HashMap<String, HashMap<String, Arc<Sig>>>,
    /// The parameters' types of the lambda about to be checked, from the
    /// list a builtin gives it: `xs | map((x) => ...)`.
    hint: Option<Vec<T>>,
    /// Warn where a value may be null and null does not work (`--strict-null`).
    strict_null: bool,
    /// See `Options::confined`.
    confined: bool,
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
    /// The program's functions that give only SQL-safe text (each result
    /// `sql_name(...)`, say): calling one is as safe as `sql_name` itself.
    sql_name_fns: HashSet<String>,
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
            overrides: HashMap::new(),
            globals,
            scopes: Vec::new(),
            fns: Vec::new(),
            quiet: 0,
            sequential: true,
            assigned: HashSet::new(),
            ret_cache: HashMap::new(),
            in_progress: HashSet::new(),
            effect_cache: HashMap::new(),
            natives: Vec::new(),
            state_types: HashMap::new(),
            live_ok: false,
            services: HashMap::new(),
            hint: None,
            strict_null: false,
            confined: false,
            uses: 0,
            depth: 0,
            main: Arc::from(file),
            file: Arc::from(file),
            line: 0,
            out: Analysis::default(),
            reported: HashSet::new(),
            parse_errors: HashSet::new(),
            unparsed_names: HashSet::new(),
            sql_name_fns: HashSet::new(),
            sql_vars: Vec::new(),
            next_sql: SqlVar::Other,
            url_vars: Vec::new(),
            next_url: false,
        }
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

    fn options(&self) -> Options {
        Options { strict_null: self.strict_null, confined: self.confined, css: false }
    }

    /// The file a `use`, `mount` or `service` names; when confined, none
    /// for a path that could lead outside the program's folder.
    fn file_at(&mut self, base: &Path, path: &str, file: &Arc<str>, line: u32) -> Option<PathBuf> {
        let plain = !path.is_empty() && Path::new(path).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
        if self.confined && !plain {
            self.diag_at(file.clone(), line, Severity::Error, format!("path must stay inside the program's folder: {}", path));
            return None;
        }
        Some(base.join(path))
    }

    /// Lists statements in the order the top level runs them, following `use`.
    fn expand(&mut self, stmts: Arc<Vec<Stmt>>, file: Arc<str>, base: &Path, seen: &mut HashSet<PathBuf>, order: &mut Vec<Item>) {
        for s in stmts.iter() {
            if let Stmt::Service(name, prefix, path, _, line) = s {
                let Some(full) = self.file_at(base, path, &file, *line) else { continue };
                match crate::service::offered(&full) {
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
            }
            if let Stmt::Mount(prefix, path, line) = s {
                // A mounted program is a program of its own: check it alone.
                let Some(full) = self.file_at(base, path, &file, *line) else { continue };
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
            if let Stmt::Use(path, line) = s {
                let Some(full) = self.file_at(base, path, &file, *line) else { continue };
                let key = full.canonicalize().unwrap_or_else(|_| full.clone());
                if !seen.insert(key) {
                    continue;
                }
                let fname: Arc<str> = Arc::from(full.display().to_string());
                let text = self.overrides.get(&full).cloned().map_or_else(|| std::fs::read_to_string(&full), Ok);
                // Rust: its functions' signatures, read without building it.
                if path.ends_with(".qwer") {
                    match text {
                        Ok(src) => {
                            let parsed = crate::native::parse(&src);
                            for (l, msg) in parsed.problems {
                                self.diag_at(fname.clone(), l, Severity::Warning, msg);
                            }
                            for e in parsed.exports {
                                self.natives.push((fname.clone(), e));
                            }
                        }
                        Err(e) => self.diag_at(file.clone(), *line, Severity::Error, format!("cannot read {}: {}", full.display(), e)),
                    }
                    continue;
                }
                if self.uses >= MAX_USES {
                    self.diag_at(file.clone(), *line, Severity::Error, format!("files `use` each other more than {} deep", MAX_USES));
                    continue;
                }
                match text {
                    Ok(src) => match Parser::new(&src).parse_program() {
                        Ok(sub) => {
                            self.uses += 1;
                            self.expand(Arc::new(sub), fname, base, seen, order);
                            self.uses -= 1;
                        }
                        Err(e) => {
                            if crate::error::may_be_newer_syntax(&e.msg) {
                                self.out.syntax_errors.push((fname.to_string(), e.line));
                            }
                            self.parse_errors.insert((fname.clone(), e.line));
                            self.unparsed_names.extend(names_in_text(&src));
                            self.diag_at(fname, e.line, Severity::Error, e.msg)
                        }
                    },
                    Err(e) => self.diag_at(file.clone(), *line, Severity::Error, format!("cannot read {}: {}", full.display(), e)),
                }
                continue;
            }
            order.push(Item { stmt: s.clone(), file: file.clone() });
        }
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
        // Everything top-level is known up front; it becomes usable from
        // top-level code once the program has run its definition.
        for it in &order {
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
                    // What text a global holds, for the SQL warning in the
                    // functions that use it; one reassigned anywhere holds
                    // nothing known.
                    if let Pattern::Bind(n) = p {
                        if !self.assigned.contains(n) {
                            let k = self.sql_kind(e);
                            self.sql_vars.push((0, n.clone(), k));
                            let u = self.url_fixed(e);
                            self.url_vars.push((0, n.clone(), u));
                        }
                    }
                    for (n, t, sig) in binds {
                        let reassigned = self.assigned.contains(&n);
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
        let depth = self.scopes.len().saturating_sub(1);
        while self.sql_vars.last().map_or(false, |(d, _, _)| *d > depth) {
            self.sql_vars.pop();
        }
        while self.url_vars.last().map_or(false, |(d, _, _)| *d > depth) {
            self.url_vars.pop();
        }
        if let Some(scope) = self.scopes.pop() {
            for (name, v) in scope {
                if v.kind == Kind::Let && !v.used && !name.starts_with('_') && self.quiet == 0 {
                    self.diag_at(v.file.clone(), v.line, Severity::Warning, format!("unused variable `{}` (name it `_{}` if that is intended)", name, name));
                }
            }
        }
    }

    fn define(&mut self, name: &str, t: T, sig: Option<Arc<Sig>>, kind: Kind) {
        let sql = std::mem::replace(&mut self.next_sql, SqlVar::Other);
        self.sql_vars.push((self.scopes.len(), name.to_string(), sql));
        let url = std::mem::replace(&mut self.next_url, false);
        self.url_vars.push((self.scopes.len(), name.to_string(), url));
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
                    Kind::State => "state ",
                    Kind::Param => "param ",
                    _ => "let ",
                };
                if MODULES.contains(&name) && v.kind == Kind::Global && def.is_none() {
                    format!("module {}", name)
                } else {
                    format!("{}{}: {}", kw, name, t_text(v.t))
                }
            }
        };
        if v.kind == Kind::State {
            let at = self.out.references.len();
            if let Some(e) = self.state_types.get_mut(&(v.file.clone(), v.line, name.to_string())) {
                e.1.push(at);
            }
        }
        self.out.references.push(Reference { file: self.file.to_string(), line: self.line, name: name.to_string(), def, detail });
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
                if v.kind == Kind::Global && name == "db" && &*v.file == "<builtin>" {
                    self.browser_forbidden(&format!("`{}`", name));
                }
                (v.t, v.sig)
            }
            None => {
                if name == "request" {
                    self.error("`request` only exists in routes and `server fn`s; pass what you need as an argument");
                } else if !self.unparsed_names.contains(name) {
                    let removed = crate::error::removed_note(name);
                    self.error(format!("unknown name `{}`{}", name, if removed.is_empty() { self.suggest(name) } else { removed }));
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
            ret: d.ret.as_ref().map(ty_t),
            err: d.err.as_ref().map(ty_bits),
            allowed: effects.unwrap_or(ALL_EFFECTS),
            restrictor: d.effects.as_ref().map(|e| (d.name.clone(), e.clone())),
            component: d.component.then(|| d.name.clone()),
            nested: false,
            state_init: false,
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
        self.params_hinted(d, None)
    }

    /// Parameters, those without an annotation typed by `hint` if given.
    fn params_hinted(&mut self, d: &FnDef, hint: Option<Vec<T>>) {
        for (i, p) in d.params.iter().enumerate() {
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
        self.fns.push(FnCtx {
            name: format!("{} {}", r.method, route_path(r)),
            ret: None,
            err: None,
            allowed: ALL_EFFECTS,
            restrictor: None,
            component: None,
            nested: false,
            state_init: false,
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
                let sql = if matches!(p, Pattern::Bind(_)) { self.sql_kind(e) } else { SqlVar::Other };
                let url = matches!(p, Pattern::Bind(_)) && self.url_fixed(e);
                for (n, t, sig) in binds {
                    let kind = if matches!(p, Pattern::Bind(_)) { Kind::Let } else { Kind::Bound };
                    let t = if self.assigned.contains(&n) { T::ANY } else { t };
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
                if self.in_component().is_none() {
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
                        if v.kind == Kind::State {
                            let key = (v.file.clone(), v.line, name.clone());
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
            Stmt::Service(_, _, _, _, line) => {
                self.line = *line;
                if !self.scopes.is_empty() {
                    self.error("`service` must be at the top level");
                }
                T::of(NIL)
            }
            Stmt::Type(td) => {
                self.line = td.line;
                if !self.scopes.is_empty() {
                    self.error("`type` must be at the top level");
                }
                // A default must be of its field's type.
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
                    if let Some(Expr::Ident(f, _)) = args.first() {
                        let ret = self.scopes.iter().rev().find_map(|s| s.get(f).cloned()).or_else(|| self.globals.get(f).cloned()).and_then(|v| v.sig).and_then(|s| s.ret);
                        if let Some(r) = ret {
                            t = r;
                        }
                    }
                }
                let assigned = self.assigned.contains(name);
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
            state_init: false,
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
                } else if MODULES.contains(&n.as_str()) {
                    // A module called as a function: `audit(...)`.
                    mask |= builtin_effect(&n);
                }
            } else if let Some((m, f)) = n.split_once('.') {
                if MODULES.contains(&m) {
                    mask |= builtin_effect(&format!("{}.{}", m, f));
                } else if self.services.contains_key(m) {
                    mask |= crate::ast::effect_bit("http");
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
                let x = self.expr(a);
                // `x != null && f(x)`: the right side runs only if the left holds.
                let mut facts = Vec::new();
                match op {
                    BinOp::And => self.facts(a, true, &mut facts),
                    BinOp::Or => self.facts(a, false, &mut facts),
                    _ => {}
                }
                let y = self.under(facts, |c| c.expr(b));
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
                let y = self.expr(b);
                x.without(NIL | ERR).union(y)
            }
            Expr::Pipe(lhs, rhs, line) => {
                let v = self.expr(lhs);
                self.line = *line;
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
                let x = self.under(yes, |s| s.expr(a));
                let y = match b {
                    Some(b) => self.under(no, |s| s.expr(b)),
                    None => T::of(NIL),
                };
                x.union(y)
            }
            Expr::Match(subj, arms, line) => {
                let mut st = self.expr(subj);
                self.line = *line;
                let mut t = T::NEVER;
                for arm in arms {
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
                self.expr(body);
                self.pop();
                T::of(LIST)
            }
            Expr::While(c, body, line) => {
                let ct = self.expr(c);
                self.line = *line;
                self.js_truthiness(ct, c, Truth::Cond);
                let mut yes = Vec::new();
                self.facts(c, true, &mut yes);
                self.under(yes, |s| s.expr(body));
                T::of(NIL)
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
                self.pop();
                if never && t.bits == 0 {
                    T::NEVER
                } else {
                    t
                }
            }
            Expr::Lambda(d) => {
                self.line = d.line;
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
        // `audit(...)`: the module `audit` is a function too.
        if let Expr::Ident(n, _) = callee {
            if n == "audit" && !self.is_local(n) && self.globals.get(n).map_or(false, |v| &*v.file == "<builtin>") {
                return self.module_call(n, "", args, pre);
            }
        }
        // `x.f(args)`: a function stored in map x, or `f(x, args)`.
        if let Expr::Field(obj, name, _) = callee {
            // `billing.later.invoice(args)`: the same call, made later; its id.
            if let Expr::Field(inner, later, _) = &**obj {
                if let (Expr::Ident(m, _), "later") = (&**inner, later.as_str()) {
                    if self.services.contains_key(m) && !self.is_local(m) {
                        self.service_call(m, name, args, pre);
                        return T::of(STR);
                    }
                }
            }
            if let Expr::Ident(m, _) = &**obj {
                if self.services.contains_key(m) && !self.is_local(m) {
                    return self.service_call(m, name, args, pre);
                }
                if MODULES.contains(&m.as_str()) && !self.is_local(m) {
                    return self.module_call(m, name, args, pre);
                }
            }
            // `conn.query(sql)` on a connection from `db.connect`.
            if pre.is_none() {
                if let (Some(what), Some(sql)) = (self.sql_target(callee), args.first()) {
                    if what != format!("db.{}", name) && self.sql_built(sql) {
                        self.warn_sql(&what);
                    }
                }
            }
            let recv = self.expr(obj);
            self.line = line;
            let found = self.scopes.iter().rev().find_map(|s| s.get(name).cloned()).or_else(|| self.globals.get(name).cloned());
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
                    out.push((name.clone(), if null { T::of(NIL) } else { t.without(NIL) }));
                }
            }
            Expr::Ident(n, _) if yes => {
                if let Some(t) = known(n) {
                    out.push((n.clone(), t.without(NIL | ERR)));
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
            "money.add" | "money.sub" | "money.mul" | "money.neg" | "money.round" => Some(money_t()),
            "money.of" | "money.from_minor" | "money.zero" => Some(money_t().union(T::error(STR))),
            "money.allocate" => Some(list_of(money_t())),
            // A decimal gives a decimal; ints and floats as before.
            "abs" | "floor" | "ceil" | "round" if a0.bits == DEC => Some(T::of(DEC)),
            "abs" | "floor" | "ceil" | "round" if a0.bits & DEC == 0 && a0.bits != 0 => Some(T::of(NUM)),
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
    /// where the value is known to be a string or a number (or null).
    fn js_truthiness(&mut self, t: T, e: &Expr, how: Truth) {
        if t.bits & (STR | NUM | DEC) == 0 || t.bits & !(STR | NUM | DEC | NIL) != 0 {
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
            self.error(format!("`{}` has no function `{}`{}", m, f, crate::error::removed_note(&full)));
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

    // ---- SQL built from values ----------------------------------------

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
                if !["redirects", "public_only", "cache", "timeout"].contains(&k.as_str()) {
                    self.error(format!("`http.{}`: unknown option `{}` (the options are redirects, public_only, cache, timeout)", f, k));
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
        if !matches!(f.as_str(), "query" | "one" | "exec") {
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
            Expr::Call(f, args, _) => {
                self.sql_safe_call(f)
                    || (is_join(f) && matches!(args.as_slice(), [Expr::List(items), rest @ ..] if items.iter().chain(rest).all(|x| self.sql_safe(x))))
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
                    let program_fn = self.globals.get(n).and_then(|v| v.sig.clone()).map_or(false, |s| !s.builtin && s.def.is_some());
                    if program_fn {
                        other = true;
                    }
                }
                Expr::Field(o, _, _) => match &**o {
                    Expr::Ident(n, _) if n == "db" => other = true,
                    Expr::Ident(n, _) if self.services.contains_key(n) => service = Some(n.clone()),
                    Expr::Field(inner, later, _) if later == "later" => {
                        if let Expr::Ident(n, _) = &**inner {
                            if self.services.contains_key(n) {
                                service = Some(n.clone());
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

    /// Whether calling `f` gives SQL-safe text: `sql_name`, `sql_param`, or
    /// a function of the program that only returns such text.
    fn sql_safe_call(&self, f: &Expr) -> bool {
        matches!(f, Expr::Ident(n, _) if !self.is_local(n)
            && (n == "sql_name" || n == "sql_param" || self.sql_name_fns.contains(n)))
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
        if args.len() > n && !sig.variadic {
            self.error(format!("`{}` takes {} argument{}, got {}{}", name, n, if n == 1 { "" } else { "s" }, args.len(), hides(&sig, name)));
        }
        for (i, p) in sig.params.iter().enumerate() {
            let what = || if sig.builtin { format!("argument {}", i + 1) } else { format!("argument `{}`", p.name) };
            match args.get(i) {
                None if !p.optional => {
                    self.error(format!("missing argument `{}` in call to `{}`{}", p.name, name, hides(&sig, name)));
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
                    if let Some(why) = mismatch(*a, p.full) {
                        self.error(format!("{} of `{}` {}{}", what(), name, why, hides(&sig, name)));
                    } else if divided && p.ty & INT != 0 && p.ty & FLOAT == 0 && a.bits & FLOAT != 0 {
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
        if sig.builtin && matches!(name, "db.query" | "db.one" | "db.exec") && args.len() == arg_exprs.len() {
            if arg_exprs.first().map_or(false, |e| self.sql_built(e)) {
                self.warn_sql(name);
            }
        }
        if sig.builtin && matches!(sig.name.as_str(), "db.transaction" | "idempotent") {
            self.retried_effects(name, arg_exprs);
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
            match arg_exprs.first() {
                Some(Expr::Ident(f, _)) => {
                    let is_server = self.globals.get(f).and_then(|v| v.sig.clone()).map_or(true, |s| s.server || s.def.is_none() && !s.builtin);
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
                HtmlChild::El(e) => crate::stack::maybe_grow(256 << 10, 4 << 20, || self.html(e)),
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
            Some(Stmt::Return(_)) => true,
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
        _ => "this value".into(),
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
/// The classes the program's sheets style (`css`...``, and `.css` files
/// under public/) that nothing in it can write: no text in its code or its
/// public scripts, no start of a template (`card-${size}` writes any
/// `card-...`), not set by Quillo, nor by a script a sheet names
/// (`/* set elsewhere: leaflet- */`). Each with where it is styled.
fn unused_classes(order: &[Item], base: &Path) -> Vec<(Arc<str>, u32, String)> {
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
                let text = match &args[0] {
                    Expr::Lit(Lit::Str(s)) => Some(s.to_string()),
                    Expr::Template(parts) => Some(parts.iter().map(|p| if let TPart::Lit(t) = p { t.as_str() } else { " " }).collect()),
                    _ => None,
                };
                if let Some(text) = text {
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
            Stmt::Return(Some(e)) => walk_expr(e, f),
            Stmt::Fn(d) => walk_expr(&d.body, f),
            Stmt::Route(r) => walk_expr(&r.body, f),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Return(None) => {}
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

fn collect_assigned(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::Assign(n, _, e, _) => {
                out.insert(n.clone());
                assigned_in(e, out)
            }
            Stmt::Let(_, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => assigned_in(e, out),
            Stmt::SetField(o, _, _, e, _) => {
                assigned_in(o, out);
                assigned_in(e, out)
            }
            Stmt::Return(Some(e)) => assigned_in(e, out),
            Stmt::Fn(d) => assigned_in(&d.body, out),
            Stmt::Route(r) => assigned_in(&r.body, out),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(..) | Stmt::Type(_) | Stmt::Return(None) => {}
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
            Expr::Ident(n, _) => vars.contains(n),
            Expr::Call(f, _, _) => matches!(&**f, Expr::Ident(n, _) if n == "sql_name" || n == "sql_param" || safe.contains(n)),
            Expr::Template(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_)) || matches!(p, TPart::Expr(x) if ok(x, safe, vars))),
            Expr::Binary(BinOp::Add, a, b, _) => ok(a, safe, vars) && ok(b, safe, vars),
            Expr::If(_, a, b) => ok(a, safe, vars) && b.as_deref().map_or(false, |b| ok(b, safe, vars)),
            Expr::Match(_, arms, _) => !arms.is_empty() && arms.iter().all(|a| ok(&a.body, safe, vars)),
            Expr::Block(stmts) => {
                let vars = block_vars(stmts, safe, vars);
                match stmts.last() {
                    Some(Stmt::Expr(x)) => ok(x, safe, &vars),
                    Some(Stmt::Return(Some(x))) => ok(x, safe, &vars),
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
                    vars.insert(n.clone());
                } else {
                    vars.remove(n);
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
                            vars.insert(n.clone());
                        } else {
                            vars.remove(n);
                        }
                    }
                    Stmt::Return(Some(r)) if !ok(r, safe, &vars) => returns_ok = false,
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
