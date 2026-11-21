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
    /// Each element's and shape's size (the types it is made of, counted as a tree) and depth,
    /// by index, so that none grows past `MOST_TYPE_SIZE` or `MOST_TYPE_DEPTH`; the cells all
    /// the tables hold, to `MOST_TYPE_CELLS`; and the unions already made, by their two sides
    /// (FEEDBACK75.md, Q-1053; branch feedback75).
    elem_sizes: Vec<(u32, u32)>,
    shape_sizes: Vec<(u32, u32)>,
    cells: usize,
    made: u32,
    elem_unions: HashMap<(u32, u32), u32, crate::value::BuildFx>,
    shape_unions: HashMap<(u32, u32), u32, crate::value::BuildFx>,
}

/// What a type may grow to before what it holds is taken as unknown, as for `any`: `[x, {a: x}]`
/// made again on each of 26 lines doubled the type each time, and a 699-byte program checked by
/// `check_program` took the server's memory until it was killed (FEEDBACK75.md, Q-1053; branch
/// feedback75). Unknown never makes an error, so a type cut short only says less.
/// That is a confined check's (`check_program`, of code from a program's users); another's are far
/// larger, so that a data table written out (1,000 records of 10 fields, a translation table of 21
/// languages) keeps its fields known and its mistakes said, as in 0.8.8 (FEEDBACK75.md, Q-1053,
/// found in review; branch feedback75): the union memo and `side_by_side` keep them cheap.
const MOST_TYPE_SIZE: u32 = 10_000;
const MOST_TYPE_DEPTH: u32 = 16;
const MOST_TYPE_SIZE_OWN: u32 = 1_000_000;
const MOST_TYPE_DEPTH_OWN: u32 = 64;
/// The cells (elements, and fields of shapes) one check keeps: past them a confined check ends with
/// one error, as past `CONFINED_TIME`; another's further types are unknown, said nothing of, as
/// 0.8.8 checked and ran such a program (FEEDBACK75.md, Q-1053, found in review; branch
/// feedback75).
const MOST_TYPE_CELLS: usize = 1 << 20;
/// What one check measures for "did you mean" (`Checker::suggest`, `osa_band`'s cost, about 3 to 6
/// ns a unit): a confined check's, about 200,000 names compared, and another's, about 1 to 2 s, in
/// which 3,000 unknown names among 3,000 functions of 67 letters, or of letters not ASCII, each get
/// theirs, as in 0.8.8, where they took 82 s and 5 s (FEEDBACK75.md, Q-1120, found in review;
/// branch feedback75).
const MOST_SUGGEST_COMPARED: u64 = 1_000_000;
const MOST_SUGGEST_COST: u64 = 320_000_000;
/// How long a confined check (`check_program`, of code from a program's users) may take.
const CONFINED_TIME: std::time::Duration = std::time::Duration::from_secs(2);
/// What a check that ran past its budget says, in place of all else.
const TOO_COMPLEX: &str = "the code is too complex to check: its types grew past what a check keeps, or the check took too long";

/// The most a type's size and depth may be in the check on this thread.
fn type_limits() -> (u32, u32) {
    if CONFINED.with(|c| c.get()) {
        (MOST_TYPE_SIZE, MOST_TYPE_DEPTH)
    } else {
        (MOST_TYPE_SIZE_OWN, MOST_TYPE_DEPTH_OWN)
    }
}

/// A type's size and depth, from its parts' recorded in the tables.
fn t_size(t: T, tables: &Tables) -> (u32, u32) {
    let (mut size, mut depth) = (1u32, 1u32);
    if t.bits & LIST != 0 && t.elem != 0 {
        let (s, d) = tables.elem_sizes[t.elem as usize - 1];
        size = size.saturating_add(s);
        depth = depth.max(d.saturating_add(1));
    }
    if t.bits & MAP != 0 && t.shape != 0 {
        let (s, d) = tables.shape_sizes[t.shape as usize - 1];
        size = size.saturating_add(s);
        depth = depth.max(d.saturating_add(1));
    }
    (size, depth)
}

/// Whether one more entry of `cells` cells fits in the tables; past them the check is too
/// complex, and ends (FEEDBACK75.md, Q-1053; branch feedback75).
fn room_for(tables: &mut Tables, cells: usize) -> bool {
    if OVER_BUDGET.with(|o| o.get()) {
        return false;
    }
    // A confined check's time, looked at every 256th type made (FEEDBACK75.md, Q-1053; branch
    // feedback75).
    tables.made = tables.made.wrapping_add(1);
    let late = tables.made % 256 == 0 && DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d);
    if late {
        OVER_BUDGET.with(|o| o.set(true));
        return false;
    }
    if tables.cells.saturating_add(cells) > MOST_TYPE_CELLS {
        if CONFINED.with(|c| c.get()) {
            OVER_BUDGET.with(|o| o.set(true));
        }
        return false;
    }
    tables.cells += cells;
    true
}

/// Whether a confined check's budget is spent, its clock read when `k` is a multiple of 64
/// (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1127; branch feedback75).
fn budget_spent(k: usize) -> bool {
    if k % 64 == 0 && DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d) {
        OVER_BUDGET.with(|o| o.set(true));
    }
    OVER_BUDGET.with(|o| o.get())
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
    /// When a confined check must end, and whether the check on this thread has run past its
    /// time or its tables' room (FEEDBACK75.md, Q-1053; branch feedback75).
    static DEADLINE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
    /// Whether the check on this thread is a confined one, and so its types' limits.
    static CONFINED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static OVER_BUDGET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// Set while a service is checked for a program: its `server fn`s' facts are wanted
    /// (`Analysis::server_fns`; FEEDBACK56.md, Q-853, B.3, found in review; branch feedback56).
    static SERVER_FN_FACTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What the walk of a tool's code is to read next: a function, or a type whose records' defaults
/// run when one is made (FEEDBACK56.md, Q-858, found in review; branch feedback56).
#[derive(Clone)]
enum Next {
    Fn(Arc<FnDef>),
    Type(String),
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
        // Past its size or depth what a list holds is unknown (FEEDBACK75.md, Q-1053; branch
        // feedback75).
        let (size, depth) = t_size(e, &t);
        let (most_size, most_depth) = type_limits();
        if size > most_size || depth > most_depth || !room_for(&mut t, 1) {
            return 0;
        }
        t.elems.push(e);
        t.elem_sizes.push((size, depth));
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
        // Past its size or depth a map's fields are unknown (FEEDBACK75.md, Q-1053; branch
        // feedback75).
        let (mut size, mut depth) = (0u32, 0u32);
        for (_, ft) in shape.iter() {
            let (s, d) = t_size(*ft, &t);
            size = size.saturating_add(s);
            depth = depth.max(d);
        }
        let (most_size, most_depth) = type_limits();
        if size > most_size || depth > most_depth || !room_for(&mut t, shape.len().max(1)) {
            return 0;
        }
        t.shapes.push(shape.clone());
        t.shape_sizes.push((size, depth));
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
        // At the MCP door, `{ client, version }` as the client says it; null on any other
        // request (FEEDBACK56.md, Q-859; branch feedback56).
        (Arc::from("mcp"), T::ANY),
        // What a proxy `--trust-proxy` names says of the visitor, in the headers
        // `--client-cert-header` and `--user-header` name: `{ name, subject, uri }`, and the user;
        // null else (FEEDBACK78.md, A.13 and A.14; branch feedback75).
        (Arc::from("client_cert"), T::of(MAP | NIL)),
        (Arc::from("remote_user"), T::of(STR | NIL)),
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

/// A union already made of the two sides, in either order (FEEDBACK75.md, Q-1053; branch
/// feedback75): `[x, {a: x}]` on each line joined the same parts again, twice as often each line.
fn union_made(shapes: bool, a: u32, b: u32, make: impl FnOnce() -> u32) -> u32 {
    fn pick(t: &mut Tables, shapes: bool) -> &mut HashMap<(u32, u32), u32, crate::value::BuildFx> {
        if shapes { &mut t.shape_unions } else { &mut t.elem_unions }
    }
    let key = (a.min(b), a.max(b));
    if let Some(i) = TABLES.with(|t| pick(&mut t.borrow_mut(), shapes).get(&key).copied()) {
        return i;
    }
    let i = make();
    TABLES.with(|t| pick(&mut t.borrow_mut(), shapes).insert(key, i));
    i
}

fn union_elems(a: u32, b: u32) -> u32 {
    union_made(false, a, b, || elem_id(elem_of(a).union(elem_of(b))))
}

/// One map or the other: every field either has, null where one has none.
fn union_shapes(a: u32, b: u32) -> u32 {
    union_made(true, a, b, || union_shapes_made(a, b))
}

/// The fields of two shapes, which `shape_id` keeps sorted by name, side by side in one pass: each
/// field with the other's of its name, or `None`. It looked up each of one's fields in the other,
/// and two maps of 10,000 fields took 10^8 steps a union (FEEDBACK75.md, Q-1053; branch
/// feedback75).
fn side_by_side<'s>(x: &'s Shape, y: &'s Shape) -> Vec<(&'s Arc<str>, Option<T>, Option<T>)> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(x.len().max(y.len()));
    loop {
        match (x.get(i), y.get(j)) {
            (Some((kx, tx)), Some((ky, ty))) if kx == ky => {
                out.push((kx, Some(*tx), Some(*ty)));
                (i, j) = (i + 1, j + 1);
            }
            (Some((kx, tx)), Some((ky, _))) if kx < ky => {
                out.push((kx, Some(*tx), None));
                i += 1;
            }
            (_, Some((ky, ty))) => {
                out.push((ky, None, Some(*ty)));
                j += 1;
            }
            (Some((kx, tx)), None) => {
                out.push((kx, Some(*tx), None));
                i += 1;
            }
            (None, None) => return out,
        }
    }
}

fn union_shapes_made(a: u32, b: u32) -> u32 {
    let (Some(x), Some(y)) = (shape_of(a), shape_of(b)) else { return 0 };
    let nil = T::of(NIL);
    shape_id(side_by_side(&x, &y).into_iter().map(|(k, tx, ty)| (k.clone(), tx.unwrap_or(nil).union(ty.unwrap_or(nil)))).collect())
}

/// `merge(a, b)` and `a + b` on maps: b's fields over a's.
fn merge_maps(a: T, b: T) -> T {
    match (a.fields(), b.fields()) {
        // In one pass, as `side_by_side` reads two shapes (FEEDBACK75.md, Q-1053; branch
        // feedback75).
        (Some(x), Some(y)) if a.bits & NIL == 0 && b.bits & NIL == 0 => {
            map_of(side_by_side(&x, &y).into_iter().filter_map(|(k, tx, ty)| ty.or(tx).map(|t| (k.clone(), t))).collect())
        }
        _ => T::of(MAP),
    }
}

/// A type as the checker knows it: `list<int>`, `{ id: int, title: str } | null`. Past 64
/// lists and maps, what is left is said by its kind: the parts of a type are shared, and a type
/// made of two of the one before it, 26 times over, was written out as 2^26 copies
/// (FEEDBACK75.md, Q-1053; branch feedback75).
fn t_text(t: T) -> String {
    t_text_in(t, &mut 64)
}

fn t_text_in(t: T, left: &mut u32) -> String {
    if t.is_any() || (t.elem == 0 && t.shape == 0) || *left == 0 {
        return bits_text(t.bits);
    }
    *left -= 1;
    let mut parts: Vec<String> = Vec::new();
    let rest = t.bits & !(NIL | if t.elem != 0 { LIST } else { 0 } | if t.shape != 0 { MAP } else { 0 });
    if rest != 0 {
        parts.push(bits_text(rest));
    }
    if t.elem != 0 && t.bits & LIST != 0 {
        parts.push(format!("list<{}>", t_text_in(elem_of(t.elem), left)));
    }
    if let Some(s) = shape_of(t.shape).filter(|_| t.bits & MAP != 0) {
        let mut fs: Vec<String> = s.iter().take(6).map(|(k, ft)| format!("{}: {}", k, t_text_in(*ft, left))).collect();
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
/// components it shows, as far as they go, and in the functions it calls (FEEDBACK79.md, Q-1258;
/// branch feedback72-81). `bound`: the names the route binds itself. `explored`: the functions
/// earlier routes' pages read through, by their middleware, whose server fns those routes found,
/// not read again: a warning is said for the first route only, and 150 routes of one layout read
/// it 150 times. `refs` keeps the functions each one names, by `def_key`, so each body is read once.
fn server_fns_reached(
    fns: &HashMap<String, Arc<FnDef>>,
    body: &Expr,
    bound: &HashSet<String>,
    refs: &mut HashMap<usize, Arc<[Arc<str>]>>,
    explored: &mut HashMap<Vec<String>, HashSet<Arc<str>>>,
) -> Vec<Arc<FnDef>> {
    let get = |n: &str| fns.get(n).cloned();
    // The program's functions code names or shows as a tag, each once, but for a name it binds
    // itself (a parameter, a `let`, a pattern's): `view TodoList(todos: list)` reads its
    // parameter, not `server fn todos()`.
    let named = |e: &Expr, bound: &HashSet<String>| -> Vec<Arc<str>> {
        let mut out: Vec<Arc<str>> = Vec::new();
        let mut once: HashSet<Arc<str>> = HashSet::new();
        let mut add = |n: &str, out: &mut Vec<Arc<str>>| {
            if !once.contains(n) && !bound.contains(n) && get(n).is_some() {
                let n: Arc<str> = Arc::from(n);
                once.insert(n.clone());
                out.push(n);
            }
        };
        walk_expr(e, &mut |x| match x {
            Expr::Ident(n, _) => add(n, &mut out),
            Expr::Html(el, _) => walk_html_els(el, &mut |h| {
                if h.tag.starts_with(|c: char| c.is_uppercase()) {
                    add(&h.tag, &mut out);
                }
            }),
            _ => {}
        });
        out
    };
    // A group's middleware, called first (`__answer`), runs before the page and is none of it.
    let mut skip: HashSet<String> = HashSet::new();
    if let Expr::Block(stmts) = body {
        for st in stmts {
            let Stmt::Expr(Expr::Call(f, args, _)) = st else { break };
            if !matches!(&**f, Expr::Ident(n, _) if n == "__answer") {
                break;
            }
            if let Some(Expr::Call(m, _, _)) = args.first() {
                walk_expr(m, &mut |x| {
                    if let Expr::Ident(n, _) = x {
                        skip.insert(n.to_string());
                    }
                });
            }
        }
    }
    let mut key: Vec<String> = skip.iter().cloned().collect();
    key.sort();
    let seen = explored.entry(key).or_default();
    let mut server: Vec<Arc<FnDef>> = Vec::new();
    let mut todo: Vec<Arc<str>> = named(body, bound);
    todo.reverse();
    while let Some(n) = todo.pop() {
        let Some(d) = get(&n) else { continue };
        if d.server {
            if seen.insert(n) {
                server.push(d);
            }
            continue;
        }
        // A server fn is not read through: what it calls runs on the server, not on the page.
        if skip.contains(&*n) || !seen.insert(n) {
            continue;
        }
        let more = refs.entry(def_key(&d)).or_insert_with(|| named(&d.body, &fn_bound(&d)).into()).clone();
        todo.extend(more.iter().rev().cloned());
    }
    server
}

/// The names a function binds itself, anywhere in it: its parameters, its `let`s, `state`s and
/// local `fn`s, its patterns' and its lambdas' parameters. Read as hiding the program's functions
/// of those names everywhere in it (Q-1258, Q-1261).
fn fn_bound(d: &FnDef) -> HashSet<String> {
    let mut out: HashSet<String> = d.params.iter().map(|p| p.name.to_string()).collect();
    bound_in(&d.body, &mut out);
    out
}

/// What `e` binds, anywhere in it (`fn_bound`).
fn bound_in(e: &Expr, out: &mut HashSet<String>) {
    let mut names: Vec<String> = Vec::new();
    walk_expr(e, &mut |x| match x {
        Expr::Block(stmts) => {
            for st in stmts {
                match st {
                    Stmt::Let(p, ..) => pattern_names(p, &mut names),
                    Stmt::State(n, ..) => names.push(n.to_string()),
                    Stmt::Fn(f) => names.push(f.name.clone()),
                    _ => {}
                }
            }
        }
        Expr::For(p, ..) => pattern_names(p, &mut names),
        Expr::Match(_, arms, _) => arms.iter().for_each(|a| pattern_names(&a.pat, &mut names)),
        Expr::Lambda(l) => names.extend(l.params.iter().map(|p| p.name.to_string())),
        _ => {}
    });
    out.extend(names);
}

/// The names a route binds: its path's parameters, its patterns' names, `request`, and what its
/// body binds (Q-1258, Q-1261).
fn route_bound(r: &RouteDef) -> HashSet<String> {
    let mut out: HashSet<String> = ["request", "__request"].iter().map(|s| s.to_string()).collect();
    for seg in &r.path {
        match seg {
            Seg::Param(n, _, _) | Seg::Rest(n) => {
                out.insert(n.to_string());
            }
            Seg::Lit(_) => {}
        }
    }
    let mut names = Vec::new();
    for p in r.query.iter().chain(r.body_pat.iter()).chain(r.guards.iter()) {
        pattern_names(p, &mut names);
    }
    out.extend(names);
    bound_in(&r.body, &mut out);
    out
}

/// One call or tag of what the server runs as it makes a page (`Checker::render_refs`): a
/// function called (`f(...)`, `x | f`, a module's `db.query`), a component or view shown as a tag,
/// or, in a `state`'s value, a server fn named (`live(load)`); its line, whether it runs only on a
/// condition (in an `if`'s branch, a `match` arm, after `&&` or `??`, in a loop's body), and the
/// `state` it sets, if it is one's value (FEEDBACK80.md, Q-1261; branch feedback72-81); and
/// whether what it gives is caught or put off (`caught`).
#[derive(Clone)]
struct RenderRef {
    name: Arc<str>,
    line: u32,
    cond: bool,
    caught: u8,
    state: Option<Arc<str>>,
    tag: bool,
}

/// A refused effect is an error value at run time, not a stop: the left side of `??` takes it and
/// the page shows the fallback (`load_it() ?? "fallback"`, `now() ?? "no clock"`), and a state's
/// value under `defer(...)` (`state v = defer(load_it())`) runs after the page is made, which keeps
/// its placeholder and logs the error. Each is said as a warning, never an error. `defer(...)`
/// anywhere else (`{defer(str(now()))}`, `{defer(<Footer/>)}`), `live(...)` and `is_err(...)` are
/// not caught so, and the page is a 500 (0.8.9; found in review of FEEDBACK80.md's Q-1261; branch
/// feedback72-81).
const CAUGHT_FALLBACK: u8 = 1;
const CAUGHT_DEFERRED: u8 = 2;
/// What `render_effects` found for one route: (what needs it, the effects it lacks, how it is
/// caught) -> an error, what to say, and the components and views that call it, the one said of
/// first. A call caught by `??` and one not are said apart, each with its own end (found in review).
type Found = HashMap<(Arc<str>, u8, u8), (bool, (String, String, String, String), Vec<String>)>;

/// The calls and tags of `e` the server runs as it makes a page (`RenderRef`): not in a function
/// written there, which runs later, in the browser (a handler's, a timer's), nor under a name `e`
/// binds itself. One level deeper per node, with room on the stack looked at every eighth, as
/// `Returns::deep` does.
fn render_refs(e: &Expr, cond: bool, caught: u8, state: Option<&Arc<str>>, bound: &HashSet<String>, out: &mut Vec<RenderRef>, depth: u32) {
    if (depth + 1) % 8 == 0 {
        crate::stack::maybe_grow(256 << 10, 4 << 20, || render_refs_here(e, cond, caught, state, bound, out, depth + 1))
    } else {
        render_refs_here(e, cond, caught, state, bound, out, depth + 1)
    }
}

fn render_refs_here(e: &Expr, cond: bool, caught: u8, state: Option<&Arc<str>>, bound: &HashSet<String>, out: &mut Vec<RenderRef>, depth: u32) {
    let call = |n: String, line: u32, out: &mut Vec<RenderRef>| {
        out.push(RenderRef { name: Arc::from(n), line, cond, caught, state: state.cloned(), tag: false });
    };
    let named = |c: &Expr| -> Option<String> {
        match c {
            Expr::Ident(n, _) if !bound.contains(n.as_str()) => Some(n.to_string()),
            Expr::Field(o, f, _) => match &**o {
                Expr::Ident(m, _) if !bound.contains(m.as_str()) => Some(format!("{}.{}", m, f)),
                _ => None,
            },
            _ => None,
        }
    };
    let go = |x: &Expr, c: bool, out: &mut Vec<RenderRef>| render_refs(x, c, caught, state, bound, out, depth);
    match e {
        Expr::Lit(_) | Expr::Lambda(_) => {}
        // In a `state`'s value a server fn named is run for its first value (`live(load)`).
        Expr::Ident(n, line) => {
            if state.is_some() && !bound.contains(n.as_str()) {
                call(n.to_string(), *line, out);
            }
        }
        Expr::Template(parts) => parts.iter().for_each(|p| {
            if let TPart::Expr(x) = p {
                go(x, cond, out)
            }
        }),
        Expr::List(xs) => xs.iter().for_each(|x| go(x, cond, out)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| go(x, cond, out)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => go(x, cond, out),
        Expr::Binary(BinOp::And | BinOp::Or, a, b, _) => {
            go(a, cond, out);
            go(b, true, out);
        }
        Expr::Coalesce(a, b) => {
            render_refs(a, cond, caught.max(CAUGHT_FALLBACK), state, bound, out, depth);
            go(b, true, out);
        }
        Expr::Binary(_, a, b, _) | Expr::Index(a, b, _) => {
            go(a, cond, out);
            go(b, cond, out);
        }
        Expr::Call(c, args, line) => {
            match named(c) {
                Some(n) => call(n, *line, out),
                None => go(c, cond, out),
            }
            args.iter().for_each(|x| go(x, cond, out));
        }
        Expr::Pipe(a, b, line) => {
            go(a, cond, out);
            match &**b {
                Expr::Call(c, args, _) => {
                    match named(c) {
                        Some(n) => call(n, *line, out),
                        None => go(c, cond, out),
                    }
                    args.iter().for_each(|x| go(x, cond, out));
                }
                other => match named(other) {
                    Some(n) => call(n, *line, out),
                    None => go(other, cond, out),
                },
            }
        }
        Expr::If(c, a, b) => {
            go(c, cond, out);
            go(a, true, out);
            if let Some(b) = b {
                go(b, true, out);
            }
        }
        Expr::Match(x, arms, _) => {
            go(x, cond, out);
            for a in arms {
                if let Some(g) = &a.guard {
                    go(g, true, out);
                }
                go(&a.body, true, out);
            }
        }
        Expr::For(_, i, b, _) => {
            go(i, cond, out);
            go(b, true, out);
        }
        Expr::While(c, b, _) => {
            go(c, cond, out);
            go(b, true, out);
        }
        Expr::Reply(a, b, _) => {
            go(a, cond, out);
            if let Some(b) = b {
                go(b, cond, out);
            }
        }
        Expr::Block(stmts) => {
            for st in stmts.iter() {
                match st {
                    // Only a state's value is put off by `defer(...)`: `{defer(str(now()))}` in
                    // markup runs as the page is made, a 500 on 0.8.9 (found in review).
                    Stmt::State(n, x, _) => {
                        let name: Arc<str> = Arc::from(n.as_str());
                        match x {
                            Expr::Call(c, args, line) if matches!(&**c, Expr::Ident(d, _) if d == "defer") && !bound.contains("defer") => {
                                out.push(RenderRef { name: Arc::from("defer"), line: *line, cond, caught, state: Some(name.clone()), tag: false });
                                args.iter().for_each(|a| render_refs(a, cond, caught.max(CAUGHT_DEFERRED), Some(&name), bound, out, depth));
                            }
                            _ => render_refs(x, cond, caught, Some(&name), bound, out, depth),
                        }
                    }
                    Stmt::Let(_, x, _) | Stmt::Expr(x) | Stmt::Assign(_, _, x, _) | Stmt::Return(Some(x), _) => go(x, cond, out),
                    Stmt::SetField(o, _, _, x, _) => {
                        go(o, cond, out);
                        go(x, cond, out);
                    }
                    _ => {}
                }
            }
        }
        Expr::Html(el, _) => render_refs_html(el, cond, caught, state, bound, out, depth),
    }
}

/// `render_refs` of an element: its tag, if a view's or a component's, its attributes' values but
/// its handlers', and its children.
fn render_refs_html(el: &HtmlEl, cond: bool, caught: u8, state: Option<&Arc<str>>, bound: &HashSet<String>, out: &mut Vec<RenderRef>, depth: u32) {
    if el.tag.starts_with(|c: char| c.is_uppercase()) {
        out.push(RenderRef { name: Arc::from(el.tag.as_str()), line: el.line, cond, caught, state: state.cloned(), tag: true });
    }
    for x in el.id.iter().chain(el.classes.iter()) {
        render_refs(x, cond, caught, state, bound, out, depth);
    }
    for (a, v) in &el.attrs {
        if a.starts_with('@') {
            continue;
        }
        if let AttrVal::Expr(x) | AttrVal::Spread(x) = v {
            render_refs(x, cond, caught, state, bound, out, depth);
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(child) => {
                if (depth + 1) % 8 == 0 {
                    crate::stack::maybe_grow(256 << 10, 4 << 20, || render_refs_html(child, cond, caught, state, bound, out, depth + 1))
                } else {
                    render_refs_html(child, cond, caught, state, bound, out, depth + 1)
                }
            }
            HtmlChild::Expr(x) => render_refs(x, cond, caught, state, bound, out, depth),
        }
    }
}

/// How a value nothing takes is said of (`Checker::mark_tails`): at the top level, the builtins'
/// new values only; in a function's code, the program's own functions' and errors too; in a
/// handler's, errors with what a handler can do (FEEDBACK80.md, Q-1267, and FEEDBACK72.md, Q-1018;
/// branch feedback72-81).
#[derive(Clone, Copy, PartialEq)]
enum DropHow {
    Plain,
    Stmt,
    Handler,
}

/// The name a value dropped is about: `xs` of `xs.push(1)`, `push(xs, 1)`, `xs | push(1)` and
/// `xs + 1`.
fn dropped_subject(e: &Expr) -> Option<&str> {
    match e {
        Expr::Call(f, args, _) => match (&**f, args.first()) {
            (Expr::Field(o, _, _), _) => match &**o {
                Expr::Ident(n, _) => Some(n.as_str()),
                _ => None,
            },
            (_, Some(Expr::Ident(n, _))) => Some(n.as_str()),
            _ => None,
        },
        Expr::Pipe(l, _, _) | Expr::Binary(_, l, _, _) => match &**l {
            Expr::Ident(n, _) => Some(n.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// The functions `body` gives what they give of: called last in it, or in a `return`, through its
/// `if`s, `match`es, loops and blocks, not in a lambda written there, whose `return` is its own
/// (`Checker::gives_program_error`). Read with lists, not by recursion.
fn returned_calls(body: &Expr) -> Vec<String> {
    let mut ends: Vec<&Expr> = vec![body];
    // Each `return`'s value, wherever its block is.
    let mut todo: Vec<&Expr> = vec![body];
    while let Some(e) = todo.pop() {
        match e {
            Expr::Block(stmts) => {
                for st in stmts.iter() {
                    match st {
                        Stmt::Return(Some(x), _) => {
                            ends.push(x);
                            todo.push(x);
                        }
                        Stmt::Let(_, x, _) | Stmt::Expr(x) | Stmt::Assign(_, _, x, _) | Stmt::State(_, x, _) => todo.push(x),
                        _ => {}
                    }
                }
            }
            Expr::If(c, a, b) => {
                todo.push(c);
                todo.push(a);
                if let Some(b) = b {
                    todo.push(b);
                }
            }
            Expr::Match(x, arms, _) => {
                todo.push(x);
                todo.extend(arms.iter().map(|a| &a.body));
            }
            Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
                todo.push(i);
                todo.push(b);
            }
            _ => {}
        }
    }
    let mut out: Vec<String> = Vec::new();
    while let Some(e) = ends.pop() {
        match e {
            Expr::Call(f, _, _) => {
                if let Expr::Ident(n, _) = &**f {
                    if !out.iter().any(|o| o == n.as_str()) {
                        out.push(n.to_string());
                    }
                }
            }
            Expr::If(_, a, b) => {
                ends.push(a);
                if let Some(b) = b {
                    ends.push(b);
                }
            }
            Expr::Match(_, arms, _) => ends.extend(arms.iter().map(|a| &a.body)),
            Expr::Block(stmts) => {
                if let Some(Stmt::Expr(x)) = stmts.last() {
                    ends.push(x);
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether a function of the program can give an error: it declares `! T`, or its body makes one
/// or passes one on with `?` (`Checker::error_dropped`'s, and Q-1272's, FEEDBACK81.md; branch
/// feedback72-81).
/// A lambda written in it is its own: a `?` or an `err(...)` there leaves that lambda, not this
/// function (`[n].map((x) => int(str(x))?)`; found in review; branch feedback72-81).
fn can_fail(d: &FnDef) -> bool {
    if d.err.is_some() {
        return true;
    }
    let mut found = false;
    let mut inner: HashSet<*const Expr> = HashSet::new();
    walk_expr(&d.body, &mut |e| {
        if inner.contains(&(e as *const Expr)) {
            return;
        }
        if let Expr::Lambda(l) = e {
            walk_fn(l, &mut |x| {
                inner.insert(x as *const Expr);
            });
            return;
        }
        found |= matches!(e, Expr::Try(_)) || matches!(e, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "err"));
    });
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

/// Where a route that reads who the visitor is (the session, a cookie, a field of the request
/// that is theirs) marks its answer for shared caches: a `cache-control` written out that a
/// shared cache may keep. Not `file`'s `cache`, which says how long and not who keeps it, so
/// that Quillo choosing `private` there is no surprise (FEEDBACK59.md, Q-903; branch
/// cache-and-migrate).
fn public_after_visitor(r: &RouteDef, fns: &HashMap<String, Arc<FnDef>>) -> Option<u32> {
    let mut visitor = r.reads_visitor;
    let mut at = None;
    let mut called: Vec<String> = Vec::new();
    walk_expr(&r.body, &mut |x| match x {
        Expr::Call(f, args, line) => {
            if let Expr::Ident(n, _) = &**f {
                match n.as_str() {
                    "session" | "cookie" | "set_session" | "set_cookie" | "end_session" => visitor = true,
                    n if fns.contains_key(n) => called.push(n.to_string()),
                    // `respond(status, body, { "cache-control": ... })`, on its line.
                    "respond" => {
                        if let Some(Expr::Map(m)) = args.get(2) {
                            if m.iter().any(|(k, v)| k.eq_ignore_ascii_case("cache-control") && matches!(v, Expr::Lit(Lit::Str(s)) if crate::server::kept_by_visitor_alone(s).is_some())) {
                                at.get_or_insert(*line);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    });
    let mut seen = HashSet::new();
    at.filter(|_| visitor || called.iter().any(|n| seen.insert(n.clone()) && fns.get(n).is_some_and(|d| visitor_in(fns, &d.body, &mut seen))))
}

/// Whether an offline route reads who the visitor is other than through `session()`: its patterns
/// or its code name a field of the request that is theirs, it calls `cookie()`, there or in the
/// functions it calls, its group's middleware reads the request, or a `server fn` it calls does.
fn beyond_session(r: &RouteDef, fns: &HashMap<String, Arc<FnDef>>) -> bool {
    r.reads_visitor || beyond_in(fns, &r.body, &mut HashSet::new())
}

fn beyond_in(fns: &HashMap<String, Arc<FnDef>>, body: &Expr, seen: &mut HashSet<String>) -> bool {
    let mut found = false;
    let mut called: Vec<String> = Vec::new();
    walk_expr(body, &mut |x| match x {
        Expr::Ident(n, _) if n == "cookie" => found = true,
        // A group's middleware, given the request: it reads it unless it only hands it back.
        Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if n == "__answer") => match args.first() {
            Some(Expr::Call(m, _, _)) => match &**m {
                Expr::Lambda(d) => found |= crate::parser::middleware_reads_beyond(d),
                Expr::Ident(n, _) => found |= fns.get(n.as_str()).is_some_and(|d| crate::parser::middleware_reads_beyond(d)),
                _ => {}
            },
            _ => {}
        },
        Expr::Ident(n, _) if fns.contains_key(n.as_str()) => called.push(n.to_string()),
        _ => {}
    });
    found || called.into_iter().any(|n| seen.insert(n.clone()) && fns.get(&n).is_some_and(|d| (d.server && d.request_use().1) || beyond_in(fns, &d.body, seen)))
}

/// Why an offline route's page is never kept because of its groups' middleware, read as the
/// server reads it (`Interp::middleware_beyond`): a lambda held by a top-level `let` that reads
/// the request past the session, or middleware the server cannot follow at all, which it takes to
/// read the visitor, so the page is never kept: a field of a map (`gates.team`), a builtin
/// (`unwrap`), a call that makes the function (`make("admin")`). The check said nothing of those,
/// where the server kept none of their pages. A `let` that holds anything but a lambda written
/// there (or a name a pattern binds) may hold a function the server can read, and is not said
/// (coverage of 0.8.7, Q-1102; branch coverage-087).
fn unkept_by_middleware(r: &RouteDef, fns: &HashMap<String, Arc<FnDef>>, lets: &HashMap<String, Option<&Expr>>) -> Option<String> {
    let Expr::Block(stmts) = &r.body else { return None };
    stmts.iter().find_map(|s| {
        let Stmt::Expr(Expr::Call(f, args, _)) = s else { return None };
        if !matches!(&**f, Expr::Ident(n, _) if n == "__answer") {
            return None;
        }
        let Some(Expr::Call(m, _, _)) = args.first() else { return None };
        match &**m {
            Expr::Lambda(_) => return None,
            Expr::Ident(n, _) if fns.contains_key(n.as_str()) => return None,
            Expr::Ident(n, _) => match lets.get(n.as_str()) {
                Some(Some(Expr::Lambda(d))) => return crate::parser::middleware_reads_beyond(d).then(|| OFFLINE_READS.to_string()),
                Some(_) => return None,
                None => {}
            },
            _ => {}
        }
        let name = match &**m {
            Expr::Call(c, _, _) => dotted(c).map(|c| format!("{}(...)", c)),
            other => Some(written(other)).filter(|w| w != "..."),
        };
        Some(format!(
            "its group's middleware{} is no function of the program's or lambda written in place, which Quillo follows, so it counts as such a read",
            name.map_or(String::new(), |n| format!(", `{}`,", n))
        ))
    })
}

/// What an offline route reads, said when its page is not kept (`beyond_session`).
const OFFLINE_READS: &str = "a cookie, a header, the request's own fields of theirs";

/// Whether code reads who the visitor is through the session or a cookie, there or in the
/// functions it calls, as far as they go.
fn visitor_in(fns: &HashMap<String, Arc<FnDef>>, body: &Expr, seen: &mut HashSet<String>) -> bool {
    let mut found = false;
    let mut called: Vec<String> = Vec::new();
    walk_expr(body, &mut |x| {
        if let Expr::Ident(n, _) = x {
            if matches!(n.as_str(), "session" | "cookie" | "set_session" | "set_cookie" | "end_session") {
                found = true;
            } else if fns.contains_key(n.as_str()) {
                called.push(n.to_string());
            }
        }
    });
    found || called.into_iter().any(|n| seen.insert(n.clone()) && fns.get(&n).is_some_and(|d| visitor_in(fns, &d.body, seen)))
}

/// The fields a route's pattern checks against values written there (`service: "a" | "b"`, a
/// webhook's `kind: "push"`), which bind no name: each name, and where it is
/// (FEEDBACK58.md, Q-874; branch cache-and-migrate).
fn literal_fields(p: &Pattern, place: &str, out: &mut Vec<(String, String)>) {
    let Pattern::Record(fields) = p else { return };
    for f in fields {
        let at = if place.is_empty() { f.path.join(".") } else { format!("{}.{}", place, f.path.join(".")) };
        match &f.pat {
            Pattern::Record(_) => literal_fields(&f.pat, &at, out),
            Pattern::Lit(_) => out.extend(f.path.last().map(|n| (n.clone(), at))),
            Pattern::Alt(ps) if ps.iter().all(|q| matches!(q, Pattern::Lit(_))) => out.extend(f.path.last().map(|n| (n.clone(), at))),
            _ => {}
        }
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
    ("run_script", "str, map | null=, map | null=", "any"),
    ("sql_name", "str | list", "str"),
    ("sql_param", "str | list", "str"),
    // `ask(message, options)` on a page, `about(question, choices, default)` in a tool, once
    // `ask` with three arguments (FEEDBACK56.md, Part E, and the user's decision 2026-11-09;
    // branch feedback56).
    ("ask", "str, map | null=", "bool"),
    ("about", "str, map, str | null", "str | null"),
    ("confirm", "str", "bool"),
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
    // A failed migration stops what runs it, as it must at start (Q-602): no error value to
    // handle, which `list | error(str)` promised (FEEDBACK62.md, Q-923; branch cache-and-migrate).
    ("db.migrate", "str=", "list"),
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
    // The MCP door's tokens (FEEDBACK56.md, Q-859; branch feedback56).
    ("mcp_token", "map | null=", "str"),
    ("mcp_tokens", "map | null=", "list"),
    ("revoke_mcp_token", "str, map | null=", "bool"),
    ("image.info", "str", "map | error(str)"),
    ("image.convert", "str, str, map | null=", "map | error(str)"),
    ("crypto.sha256", "str", "str"),
    ("crypto.hmac", "str, str", "str"),
    ("crypto.token", "int=", "str"),
    ("crypto.hash_password", "str", "str"),
    ("crypto.verify_password", "str, str | null", "bool"),
    ("crypto.equal", "str, str", "bool"),
    ("http.stream", "str, fn, map | null=", "map | error(str)"),
    // A person's devices (FEEDBACK67.md, B.3; branch pwa): `state` is null while the server renders.
    ("notification.ask", "map | null=", "str | error(str)"),
    ("notification.state", "", "str | null"),
    ("notification.stop", "", "str"),
    ("notification.send", "any, map", "map | error(str)"),
    ("notification.user", "any", "str | error(str)"),
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
    // `ask` with choices and a default: the page's dialog, and a tool's question is `about`
    // (FEEDBACK56.md, the user's decision 2026-11-09; branch feedback56).
    if name == "ask" {
        return crate::mcp::ASK_OF_THREE.to_string();
    }
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

pub const MODULES: &[&str] = &["json", "db", "http", "udp", "channel", "media", "crypto", "image", "later", "math", "bit", "stats", "vec", "date", "money", "re", "notification"];

/// `notification.send` and the rest, called on a variable of that name: always the module's, never
/// a map of the program's own (FEEDBACK67.md, B.3; branch pwa).
fn notification_named(m: &str, f: &str) -> bool {
    m == "notification" && BUILTINS.iter().any(|(n, _, _)| n.strip_prefix("notification.") == Some(f))
}

/// What the modules hold that is not a function.
const MODULE_VALUES: &[&str] = &["math.pi", "math.e", "math.tau", "math.inf", "math.nan"];

/// Builtins the browser has (see client.rs). `about`, a tool's question, as `confirm`
/// (FEEDBACK56.md, the user's decision 2026-11-09; branch feedback56).
const BROWSER_ONLY_SERVER: &[&str] =
    &["env", "sleep", "file", "redirect", "respond", "read_file", "write_file", "delete_file", "check_program", "publish", "watch", "unwatched", "crypto.sha256",
      "crypto.hmac", "crypto.token", "crypto.hash_password", "crypto.verify_password", "crypto.equal", "set_cookie",
      "delete_cookie", "session", "cookie", "set_session", "end_session", "mcp_token", "mcp_tokens", "revoke_mcp_token", "confirm", "about", "translations", "locale_from", "with_locale",
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
    // A tool says so, in the editor's hover and outline (FEEDBACK56.md, Q-853; branch feedback56).
    let kw = match (d.component, d.server, d.mcp) {
        (true, ..) => "component",
        (_, true, true) => "server fn mcp",
        (_, true, false) => "server fn",
        (_, false, true) => "fn mcp",
        _ => "fn",
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
    /// Where else a warning is about, by its file and line: a GET write said once for the routes
    /// that do it, each route's file, line and name, for an editor's `relatedInformation`
    /// (FEEDBACK70.md, Q-965, second review; branch security).
    pub related: Vec<(String, u32, Vec<(String, u32, String)>)>,
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
    /// The program's tools (`fn mcp`), in the order the program declares them, each with what
    /// its code was found to do, for the MCP door (src/mcp.rs; FEEDBACK56.md, Q-858; branch
    /// feedback56).
    pub tools: Vec<ToolFacts>,
    /// The program's named types, when it has tools: their schemas (src/mcp.rs).
    pub types: crate::record::Types,
    /// Checked as another program's service: what each of its `server fn`s was found to do, so a
    /// tool calling one is said to stay inside the program only when it does (FEEDBACK56.md,
    /// Q-853, B.3, found in review; branch feedback56).
    pub server_fns: Vec<ToolFacts>,
    /// How many functions' and records' code the walk of what tools do read, each once, the
    /// services' a tool calls included: what it costs (found in review).
    pub walked: usize,
    /// The services the program offers the tools of, `service mcp /billing = "billing.qwe"`, in
    /// the order of those lines: each one's own tools and types, as its own check read them, for
    /// the program's door to list as `billing.<name>` (FEEDBACK56.md, Part F; branch feedback56).
    pub service_tools: Vec<ServiceTools>,
}

/// A service's tools, offered by the program checked (`Analysis::service_tools`).
#[derive(Debug, Clone)]
pub struct ServiceTools {
    /// The service's name, which its tools' names start with.
    pub name: String,
    pub tools: Vec<ToolFacts>,
    pub types: crate::record::Types,
}

/// What a tool's code was found to do: the code it runs, which is its body and its parameters'
/// defaults, each function it names and each lambda in them, followed as far as the checker can
/// (FEEDBACK56.md, Q-858 and B.3; branch feedback56). Each `Option` holds the first thing seen,
/// in words, for a message.
#[derive(Debug, Clone)]
pub struct ToolFacts {
    pub def: Arc<FnDef>,
    pub file: String,
    /// The effects the code reaches.
    pub effects: u8,
    /// Why the checker cannot see all it may run: a call of a parameter or variable, `js` or
    /// `run_script`, a native plugin's function, the program's variable that may hold a
    /// function, or a module or service passed as a value. None: it sees every call.
    pub uncertain: Option<String>,
    /// A write: a database call other than `db.query` or `db.one` of a `select` written in the
    /// program, a builtin that changes what the server keeps (a session, a cookie, a tenant,
    /// what pages watch, the program's settings), or the program's variable assigned.
    pub writes: Option<String>,
    /// A request that goes outside the program: `http.*`, a `site`'s functions, the net effect,
    /// another database (`db.connect`), a function a statement calls that its database does not
    /// ship. A call of one of the program's own services is not one when its code stays inside.
    pub open: Option<String>,
    /// It calls `db`: the database may be reached, and a tool that only reads is run in a
    /// read-only transaction (FEEDBACK56.md, Q-858, found in review; branch feedback56).
    pub db: bool,
    /// It calls `confirm` or `about`: it asks the user itself (FEEDBACK56.md, Part E; branch
    /// feedback56).
    pub asks: bool,
    /// A write its code surely does (`Checker::get_facts`), for a GET route of the program that
    /// calls it as a service's: one that only reads is no write there (FEEDBACK70.md, Q-965;
    /// branch security). None where it was not asked (a tool's).
    pub sure_writes: Option<String>,
}

/// How `auth` written in a service's options proves who calls without signing: `bearer`, `oauth`,
/// or `mtls` with no `key` (`service::unsigned_auth`, as it runs).
fn unsigned_auth_written(opts: Option<&Expr>) -> Option<&'static str> {
    let Some(Expr::Map(entries)) = opts else { return None };
    let Some((_, Expr::Map(auth))) = entries.iter().find(|(k, _)| k.as_str() == "auth") else { return None };
    let has = |k: &str| auth.iter().any(|(n, v)| n.as_str() == k && !matches!(v, Expr::Lit(Lit::Nil)));
    match (has("key"), has("bearer"), has("oauth"), has("mtls")) {
        (true, ..) => None,
        (_, true, ..) => Some("bearer"),
        (_, _, true, _) => Some("oauth"),
        (_, _, _, true) => Some("mtls"),
        _ => None,
    }
}

/// What a service's function was found to do, for a tool of the program that calls it
/// (`Checker::service_fn`; FEEDBACK56.md, B.3 and Part F; branch feedback56).
#[derive(Debug, Clone, Copy)]
struct ServiceFn {
    /// It stays inside the program: seen whole, reaching no other site.
    inside: bool,
    /// It asks the user (`confirm`, `about`), through the door of the tool's call it is in.
    asks: bool,
    /// It writes, or may.
    writes: bool,
    /// The checker sees all it may run.
    sure: bool,
    /// Its code surely writes (`ToolFacts::sure_writes`; FEEDBACK70.md, Q-965).
    sure_writes: bool,
}

/// What some code was found to do (`ToolFacts`' findings, without whose they are).
#[derive(Debug, Clone, Default)]
struct Seen {
    effects: u8,
    uncertain: Option<String>,
    writes: Option<String>,
    open: Option<String>,
    db: bool,
    asks: bool,
    /// The functions of another program (a service's, a site's) it calls that may change data,
    /// `billing.charge`, in the order found (`idempotent_reaches_outside`).
    elsewhere: Vec<String>,
}

impl Seen {
    /// What `o` found too, the first reason of each kind kept.
    fn join(&mut self, o: &Seen) {
        self.effects |= o.effects;
        self.db |= o.db;
        self.asks |= o.asks;
        for c in &o.elsewhere {
            if !self.elsewhere.contains(c) {
                self.elsewhere.push(c.clone());
            }
        }
        for (mine, theirs) in [(&mut self.uncertain, &o.uncertain), (&mut self.writes, &o.writes), (&mut self.open, &o.open)] {
            if mine.is_none() {
                mine.clone_from(theirs);
            }
        }
    }
}

/// One function's own code, or one record's defaults, as read once a check: what it was found to
/// do, and what it names, to read next (FEEDBACK56.md, Q-858, found in review; branch feedback56).
struct Reached {
    seen: Seen,
    next: Vec<Next>,
}

/// A function's code, in the order it runs (`Checker::ordered_fn`): the first change it makes and
/// the first question it asks, as `quillo check` says them, and the questions it asks after a
/// change, each at its line (FEEDBACK56.md, Part E; branch feedback56).
#[derive(Default)]
struct Ordered {
    changes: Option<String>,
    asks: Option<String>,
    late: Vec<(Arc<str>, u32, String)>,
}

/// The walk of one function's code in the order it runs.
struct OrderWalk<'a> {
    top_lets: &'a HashSet<String>,
    file: Arc<str>,
    /// Its parameters and what it binds: a name assigned that is one of these is no program's
    /// variable.
    locals: HashSet<String>,
    /// The first change made so far on the way the walk is on.
    changed: Option<String>,
    asks: Option<String>,
    late: Vec<(Arc<str>, u32, String)>,
    /// How many loops deep: a loop's body is walked twice, its second turn after its first's
    /// changes, three deep at most.
    loops: u32,
    /// The way the walk is on has left the function (a `return`, a `reply`): what follows on it is
    /// never reached, and a change on it is none after the `if` or `match` it is a way of. It had
    /// been: `if id == 0 { save(0); return false }` then `confirm(...)` was refused, and the
    /// program could not start (FEEDBACK56.md, found in the coverage review; branch feedback56).
    ended: bool,
    /// The first change on a way that left: the function's change all the same, for its callers,
    /// and a lambda's for the call it is given to.
    left: Option<String>,
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
    /// How long a confined check may take: 2 s unless given (FEEDBACK75.md, Q-1053; branch
    /// feedback75).
    pub time: Option<std::time::Duration>,
}

/// A problem the native parser found in a `.rs` file, as a confined check says it: the function's
/// name, when it is one, and none of the file's text (FEEDBACK75.md, Q-1095; branch feedback75).
fn unquoted_native(msg: &str) -> String {
    let name = msg.strip_prefix('`').and_then(|rest| rest.split_once('`')).filter(|(n, tail)| {
        tail.starts_with(" is not exported") && !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    match name {
        Some((n, _)) => format!("`{}` is not exported: its parameters, its result or its name cannot cross between Rust and Quillo (a check of code from a program's users quotes no file's text)", n),
        None => "this line of a Rust file cannot be used from Quillo (a check of code from a program's users quotes no file's text)".to_string(),
    }
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
    // A program's sources are read to `MOST_PROGRAM` together, the main file among them
    // (FEEDBACK71.md, Q-982, found in review; branch security).
    crate::error::with_source_budget(|| check_file_budgeted(path, overrides, opts))
}

fn check_file_budgeted(path: &Path, overrides: HashMap<PathBuf, String>, opts: Options) -> Analysis {
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
    crate::error::with_source_budget(|| check_source_budgeted(src, file, base_dir, overrides, opts))
}

fn check_source_budgeted(src: &str, file: &str, base_dir: &Path, overrides: HashMap<PathBuf, String>, opts: Options) -> Analysis {
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
        // A check's budget: its tables' room, and for a confined one its time (FEEDBACK75.md,
        // Q-1053; branch feedback75).
        OVER_BUDGET.with(|o| o.set(false));
        CONFINED.with(|c| c.set(opts.confined));
        DEADLINE.with(|d| d.set(opts.confined.then(|| std::time::Instant::now() + opts.time.unwrap_or(CONFINED_TIME))));
    }
    // A mounted program's types are its own.
    let outer_records = RECORDS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    let mut a = check_source_in(src, file, base_dir, overrides, opts);
    // Past its budget, a check ends with one error in place of what it said, which a walk cut
    // short cannot be sure of (FEEDBACK75.md, Q-1053; branch feedback75). The tables are given
    // back at once, not kept until the thread's next check.
    if outermost && OVER_BUDGET.with(|o| o.get()) {
        a.diagnostics = vec![Diagnostic { file: file.to_string(), line: 0, severity: Severity::Error, message: TOO_COMPLEX.to_string() }];
        TABLES.with(|t| *t.borrow_mut() = Tables::default());
    }
    if outermost {
        DEADLINE.with(|d| d.set(None));
        CONFINED.with(|c| c.set(false));
    }
    // What AI clients are told of the program, its main file's `//!` block among it, past what a
    // client may keep (FEEDBACK56.md, Part D; branch feedback56).
    // A character in the block that a reader of the file does not see is an error, so the program
    // does not start with it (found in review).
    if outermost {
        if let Some((line, message)) = crate::mcp::unseen_in_block(src) {
            a.diagnostics.push(Diagnostic { file: file.to_string(), line, severity: Severity::Error, message });
        }
        // Its file's name, told to AI clients as its name where it has tools (FEEDBACK71.md,
        // Q-980, found in review; branch security).
        if !a.tools.is_empty() || !a.service_tools.is_empty() {
            if let Some(message) = crate::mcp::unseen_in_name(file) {
                a.diagnostics.push(Diagnostic { file: file.to_string(), line: 0, severity: Severity::Error, message });
            }
        }
        if let Some((line, message)) = crate::mcp::instructions_warning(file, src, &a) {
            a.diagnostics.push(Diagnostic { file: file.to_string(), line, severity: Severity::Warning, message });
        }
        // The app.toml beside the program, when there is one: its mistakes before it runs, at
        // their lines (FEEDBACK67.md, B.1; branch pwa).
        let shown = Path::new(file).parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| crate::pwa::FILE.to_string(), |p| p.join(crate::pwa::FILE).display().to_string());
        a.diagnostics.extend(crate::pwa::check(base_dir, &shown));
    }
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
    // And the comments right after a value (FEEDBACK72.md, Q-1016; branch feedback72-81).
    let (parsed, noted) = crate::lexer::noting_value_comments(|| parser.parse_program());
    c.value_comments.extend(noted.into_iter().map(|(l, w, n)| (main.clone(), l, w, n)));
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
            // A script beside an app is not the app (FEEDBACK72.md, Q-1012; branch feedback72-81).
            let serves = serves_pages(&order);
            let mut focus = Vec::new();
            c.out.classes = class_rules(&order, base_dir, serves, &mut focus);
            for it in &order {
                if !c.out.files.iter().any(|f| **f == *it.file) {
                    c.out.files.push(it.file.to_string());
                }
            }
            if opts.css {
                for (file, line, msg) in unused_classes(&order, base_dir, &c.unparsed_texts, serves) {
                    c.diag_at(file, line, Severity::Warning, msg);
                }
            }
            if opts.css || opts.classes {
                for (file, line, sev, msg) in unstyled_classes(&order, base_dir, &c.out.classes) {
                    if sev == Severity::Warning || opts.classes {
                        c.diag_at(file, line, sev, msg);
                    }
                }
                // (FEEDBACK72.md, Q-1019; branch feedback72-81)
                for (file, line) in focus {
                    c.diag_at(file, line, Severity::Warning, "`outline: none` on `:focus` takes the keyboard's ring away everywhere: style `:focus-visible` instead");
                }
            }
            c.route_reach(&order);
            // The top-level names given a whole number, for a comment that says one's value
            // (`let total = pages // 7`; FEEDBACK72.md, Q-1016, found in review).
            let consts: HashMap<String, i64> = if c.value_comments.is_empty() {
                HashMap::new()
            } else {
                order
                    .iter()
                    .filter_map(|it| match &it.stmt {
                        Stmt::Let(Pattern::Bind(n), Expr::Lit(Lit::Int(v)), _) => Some((n.to_string(), *v)),
                        _ => None,
                    })
                    .collect()
            };
            c.run(order);
            c.unused_said();
            c.value_comments_said(&consts);
            c.state_details();
            c.as_written();
            // The tools' types, for their schemas (FEEDBACK56.md, Q-858; branch feedback56).
            if !c.out.tools.is_empty() {
                c.out.types = RECORDS.with(|r| r.borrow().clone());
                // What AI clients are told of each tool holds no character a reader of the file
                // does not see, as the `//!` block may not (FEEDBACK71.md, Q-980; branch
                // security). A file's text is read again only for a tool that has one.
                let overrides = &c.overrides;
                let text_of = |f: &str| if f == file { Some(src.to_string()) } else { overrides.get(Path::new(f)).cloned().or_else(|| crate::error::read_source(f).ok()) };
                for (f, line, msg) in crate::mcp::unseen_in_tools(&c.out, &text_of) {
                    c.diag_at(Arc::from(f), line, Severity::Error, msg);
                }
            }
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
    /// `state cart = []` at the top: the page's, one value for each page made (FEEDBACK68.md;
    /// branch global-state).
    PageState,
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
    /// A `server fn`'s, or a function's inside one: it runs for no page, so it reads no page
    /// state (FEEDBACK68.md; branch global-state).
    server: bool,
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
    /// Where this code surely runs on the server, as `quillo check` says it ("in a route", "in a
    /// `server fn`", "in a tick", "in a job"): None where it may run in the browser too, in a
    /// component or a function either side may call (FEEDBACK67.md, Q-959; branch pwa).
    on_server: Option<&'static str>,
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
    /// `//` comments right after a value that hold only a number or a name, by file and line
    /// (FEEDBACK72.md, Q-1016; branch feedback72-81).
    value_comments: Vec<(Arc<str>, u32, String, String)>,
    /// The top-level names holding text written in the program, or `` css`...` ``: CSS put
    /// together from them adds no rule a visitor wrote (FEEDBACK72.md, Q-1017; branch feedback72-81).
    css_texts: HashSet<String>,
    /// Each link, `redirect` and `navigate` the walk met, for `dead_links`: the file, the line, the
    /// address's literal start, whether that is all of it, and what it is (FEEDBACK79.md, Q-1251;
    /// branch feedback72-81).
    page_links: Vec<(Arc<str>, u32, String, LinkEnd, &'static str)>,
    /// Whether the walk met a call of `random`, `random_int` or `uuid` anywhere: with none, no
    /// component is read for one (FEEDBACK72.md, Q-1015; branch feedback72-81). And the functions
    /// and components whose code makes one, for those that call them.
    draws_seen: bool,
    drawers: HashSet<String>,
    /// The functions being read in their own `let`.
    letting: Vec<String>,
    /// The route's fields checked against values written in its pattern, which bind no name:
    /// each name, and where it is (FEEDBACK58.md, Q-874; branch cache-and-migrate).
    set_fields: Vec<(String, String)>,
    /// A component's `let`s whose value comes from the page (`js(...)`), by component: shown in
    /// markup, they make the browser remake the component (FEEDBACK35.md, Q-431; branch
    /// feedback27-35-fixes).
    page_lets: Vec<(String, String)>,
    /// The `let`s and `state`s set from a call that can fail (`state news = load_news()`, a server
    /// fn declared `! str`), by where each is declared: shown in markup as they are, they may show
    /// an error value (FEEDBACK81.md, Q-1272; branch feedback72-81).
    failing_vars: HashSet<(Arc<str>, u32, String)>,
    /// `can_fail` of each function asked, by `def_key`: asked of each call dropped or shown, it
    /// walked the function's body each time (FEEDBACK72.md, Q-1018; branch feedback72-81).
    fails: std::cell::RefCell<HashMap<usize, bool>>,
    /// Whether each function's body makes an error or passes one on, `err(...)`, `error(...)` or
    /// `?`, by `def_key`, for `own_answer_dropped` (FEEDBACK80.md, Q-1267; branch feedback72-81).
    makes_errors: std::cell::RefCell<HashMap<usize, bool>>,
    /// The names each component's code assigns, by `def_key` (FEEDBACK80.md, Q-1268; branch
    /// feedback72-81).
    component_assigned: HashMap<usize, HashSet<String>>,
    /// The program's functions each function names or shows, by `def_key`, for `page_reach`
    /// (FEEDBACK79.md, Q-1258; branch feedback72-81).
    page_refs: HashMap<usize, Arc<[Arc<str>]>>,
    /// `render_refs` of each function's body, by `def_key` (FEEDBACK80.md, Q-1261; branch
    /// feedback72-81).
    render_refs_cache: HashMap<usize, Arc<[RenderRef]>>,
    /// `render_effects_possible`, by the effects a route allows.
    render_possible: HashMap<u8, bool>,
    /// The blocks whose last statement's value nothing takes, marked before they are checked, and
    /// how it is said of (`mark_tails`; FEEDBACK80.md, Q-1267; branch feedback72-81).
    drop_marks: HashMap<*const Expr, DropHow>,
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
    /// Whether a call of the program's function can change anything, by its definition
    /// (`changes_anything`; FEEDBACK64.md, Q-931; branch feedback64).
    changes_cache: HashMap<usize, Result<bool, usize>>,
    /// How deep `changes_anything` is, and the least depth of a function still being read that it
    /// reached.
    changes_depth: usize,
    changes_low: usize,
    /// Functions read in a cycle whose first function is still being read: they take its answer.
    changes_pending: Vec<usize>,
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
    /// Elements around this one that take keys or focus themselves: a key handler, `tabindex`, or
    /// a `role` (FEEDBACK72.md, Q-1019, found in review; branch feedback72-81).
    keys_above: usize,
    /// A click no key reaches, said after `page_handlers` unless its handler never runs at all
    /// (Q-1019, found in review).
    unreachable: Vec<(Arc<str>, u32, String)>,
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
    /// > 0 while checking code that may run in the browser though no component holds it: a
    /// handler written in markup, and what a component or view tag is given. Where it is not,
    /// `FnCtx::on_server` says whether the code surely runs on the server (FEEDBACK67.md, Q-959;
    /// branch pwa). A page state's initializer is the top level's: the server runs it for each
    /// request, whatever the bundle does with it too.
    browser_too: u32,
    /// The function a top-level `every` or `schedule` is given, checked next: its body runs on
    /// the server, as a tick or a job (FEEDBACK67.md, Q-959; branch pwa).
    job_arg: Option<&'static str>,
    /// The services a program calls (`service billing = "billing.qwe"`): the
    /// server fns each offers, by name.
    services: HashMap<String, HashMap<String, Arc<Sig>>>,
    /// Services whose file is not here: their calls are taken as they are written.
    services_elsewhere: HashSet<String>,
    /// The services' files, by name, where they are here to read.
    service_files: HashMap<String, PathBuf>,
    /// Of each service a tool calls, read once when one does: what each of its functions does;
    /// None where its code could not be read (FEEDBACK56.md, Q-853, B.3, found in review; Part F;
    /// branch feedback56).
    service_closed: std::cell::RefCell<HashMap<String, Option<HashMap<String, ServiceFn>>>>,
    /// What each function's own code and each record's defaults were found to do, read once a
    /// check however many tools reach them, and how many were read (found in review).
    reached_fns: std::cell::RefCell<HashMap<usize, Arc<Reached>>>,
    reached_types: std::cell::RefCell<HashMap<String, Arc<Reached>>>,
    /// Of each function a tool that asks reaches, in the order its code runs: what it changes
    /// and asks first, and the questions it asks after a change (FEEDBACK56.md, Part E; branch
    /// feedback56).
    ordered: std::cell::RefCell<HashMap<usize, Arc<Ordered>>>,
    /// Each `idempotent(f)` that runs `f` in its transaction: where, and `f`, read once every
    /// body is known (`idempotent_reaches_outside`).
    idempotent_at: Vec<(Arc<str>, u32, Arc<FnDef>)>,
    walked: std::cell::Cell<usize>,
    /// What "did you mean" measured in this check (`suggest`), and the names' lower case it keeps.
    suggested: std::cell::Cell<u64>,
    lowered: std::cell::RefCell<Lowered>,
    /// Expressions walked, and whether the check has run past its budget (FEEDBACK75.md,
    /// Q-1053; branch feedback75).
    steps: u32,
    budget_left: u32,
    over_budget: bool,
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
    /// The program's functions that give back SQL built from values, as `notes_of(owner)` with
    /// `return "... '" + owner + "'"` (FEEDBACK75.md, Q-1085; branch feedback75).
    sql_built_fns: HashMap<String, bool>,
    /// What `SqlVar::Parts` stands for: the fields or items of a map or list written out that hold
    /// built SQL (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
    sql_parts: Vec<SqlParts>,
    /// While `gives_sql_built` reads a function as given SQL-safe values: `str`, `trim`, `lower`
    /// and `upper` of safe text are safe too.
    sql_pass_safe: bool,
    /// Every name given built SQL in this check, in any scope: where `sql_var` is looked at for a
    /// call of a variable's lambda (FEEDBACK75.md, Q-1085; branch feedback75).
    sql_built_vars: HashSet<String>,
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
/// The parts of a map or a list written out that hold SQL built from values: its fields by name,
/// its items by place, and whether joining it builds SQL (FEEDBACK75.md, Q-1085, found in review;
/// branch feedback75).
/// Past 64 parts, any part is taken as built: a part's place is kept for so many only.
#[derive(Debug, Default)]
struct SqlParts {
    fields: Vec<String>,
    items: Vec<usize>,
    joined: bool,
    many: bool,
}

impl SqlParts {
    fn field(&self, k: &str) -> bool {
        self.many || self.fields.iter().any(|f| f == k)
    }
    fn item(&self, i: usize) -> bool {
        self.many || self.items.contains(&i)
    }
}

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
    /// A map or list written out with built SQL in some parts (`Checker::sql_parts`).
    Parts(u32),
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
            failing_vars: HashSet::new(),
            fails: Default::default(),
            makes_errors: Default::default(),
            component_assigned: HashMap::new(),
            page_refs: HashMap::new(),
            render_refs_cache: HashMap::new(),
            render_possible: HashMap::new(),
            drop_marks: HashMap::new(),
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
            changes_cache: HashMap::new(),
            changes_depth: 0,
            changes_low: usize::MAX,
            changes_pending: Vec::new(),
            natives: Vec::new(),
            js_libs: Vec::new(),
            links: Default::default(),
            keys: HashMap::new(),
            everywhere: HashSet::new(),
            told: HashSet::new(),
            early: Vec::new(),
            input_above: 0,
            keys_above: 0,
            unreachable: Vec::new(),
            scope_ids: Vec::new(),
            scope_serial: 0,
            gone: Vec::new(),
            block_kind: None,
            unused: Vec::new(),
            scripts: HashMap::new(),
            state_types: HashMap::new(),
            live_ok: false,
            browser_too: 0,
            job_arg: None,
            services: HashMap::new(),
            services_elsewhere: HashSet::new(),
            service_files: HashMap::new(),
            service_closed: Default::default(),
            reached_fns: Default::default(),
            ordered: Default::default(),
            idempotent_at: Vec::new(),
            reached_types: Default::default(),
            walked: Default::default(),
            suggested: Default::default(),
            lowered: Default::default(),
            steps: 0,
            budget_left: 256,
            over_budget: false,
            hint: None,
            strict_null: false,
            letting: Vec::new(),
            set_fields: Vec::new(),
            plain_dollars: HashSet::new(),
            value_comments: Vec::new(),
            css_texts: HashSet::new(),
            page_links: Vec::new(),
            draws_seen: false,
            drawers: HashSet::new(),
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
            sql_built_fns: HashMap::new(),
            sql_parts: Vec::new(),
            sql_pass_safe: false,
            sql_built_vars: HashSet::new(),
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
        Options { strict_null: self.strict_null, confined: self.confined, css: false, classes: false, time: None }
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
        // And the comments right after a value (FEEDBACK72.md, Q-1016; branch feedback72-81).
        let mut comments: Vec<(Arc<str>, u32, String, String)> = Vec::new();
        self.links = crate::modules::link(Path::new(file), stmts.clone(), base, &mut |path, full| {
            // As `file_at` judges it (FEEDBACK55.md, Q-703; branch feedback55).
            if confined && (!Path::new(path).components().all(|c| matches!(c, std::path::Component::Normal(_))) || !crate::builtins::stays_in(&root, full) || !confined_reads(&root, path, full, false)) {
                return None;
            }
            let text = overrides.get(full).cloned().map_or_else(|| crate::error::read_source(full), Ok).ok()?;
            let mut p = Parser::new(&text);
            let (parsed, noted) = crate::lexer::noting_value_comments(|| p.parse_program().ok().map(Arc::new));
            let shown = full.display().to_string();
            dollars.extend(p.plain_dollars().into_iter().map(|(t, n)| (shown.clone(), t, n)));
            let shown: Arc<str> = Arc::from(shown);
            comments.extend(noted.into_iter().map(|(l, w, n)| (shown.clone(), l, w, n)));
            parsed
        });
        self.plain_dollars.extend(dollars);
        self.value_comments.extend(comments);
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
            if let Stmt::Service(name, prefix, path, opts, line, mcp) = s {
                self.everywhere.insert(name.clone());
                // `service mcp crm = "https://..."`: kept for later, its calls not checked
                // (FEEDBACK56.md, Q-853; branch feedback56).
                if *mcp && prefix.is_none() {
                    self.diag_at(file.clone(), *line, Severity::Error, crate::service::mcp_calls_later(name));
                    self.services_elsewhere.insert(name.clone());
                    self.services.entry(name.clone()).or_default();
                    order.push(Item { stmt: s.clone(), file: file.clone() });
                    continue;
                }
                // `service mcp /billing`: its tools' calls are signed both ways, which a bearer
                // token, OAuth or a certificate alone does not do (FEEDBACK56.md, Part F, security
                // review).
                if let (true, Some(how)) = (*mcp, unsigned_auth_written(opts.as_ref())) {
                    self.diag_at(file.clone(), *line, Severity::Error, crate::service::tools_unsigned(name, how));
                }
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
                        // Read again only when a tool calls it (`service_stays_inside`).
                        self.service_files.insert(name.clone(), full.clone());
                        if first {
                            let sub = check_file_opts(&full, HashMap::new(), self.options());
                            SERVICE_CHECKS.with(|c| c.borrow_mut().remove(&key));
                            // `service mcp /billing`: billing's own tools are this program's too,
                            // as `billing.<name>`, a name MCP allows (FEEDBACK56.md, Part F, B.1;
                            // branch feedback56). Not those of a service billing offers in turn.
                            if *mcp && !self.out.service_tools.iter().any(|s| s.name == *name) {
                                if sub.tools.is_empty() {
                                    self.diag_at(file.clone(), *line, Severity::Warning, format!(
                                        "`service mcp`: {} has no tools (`fn mcp name(...)`), so none of it is offered to AI clients here; `service {}` serves it as this does",
                                        path, prefix.as_deref().unwrap_or("")
                                    ));
                                }
                                for t in &sub.tools {
                                    let joined = format!("{}.{}", name, t.def.name);
                                    if joined.chars().count() > 128 {
                                        self.diag_at(file.clone(), *line, Severity::Error, format!(
                                            "a tool's name is 1 to 128 of A-Z, a-z, 0-9, `_`, `-` and `.`, as MCP allows: {}'s tool `{}` is offered here as `{}`, {} characters",
                                            name, t.def.name, joined, joined.chars().count()
                                        ));
                                    }
                                }
                                self.out.service_tools.push(ServiceTools { name: name.clone(), tools: sub.tools.clone(), types: sub.types.clone() });
                            }
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
                    // Its pages are answered by it, apart from the program's service worker, which
                    // marks and keeps the main program's alone (FEEDBACK67.md, B.2, review; branch pwa).
                    if let Ok(text) = crate::error::read_text(&full) {
                        for st in crate::parser::Parser::new(&text).parse_program().unwrap_or_default() {
                            if let Stmt::Route(r) = &st {
                                if r.offline {
                                    self.diag_at(Arc::from(full.display().to_string().as_str()), r.line, Severity::Warning, format!("`route offline {}` is in a program mounted at `{}`, whose pages Quillo's service worker does not keep: put the route in the main program", r.shown(), prefix));
                                }
                            }
                        }
                    }
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
                                // A confined check quotes no file's text: a type was quoted from
                                // its `:` to the next `,` or `)`, across lines (FEEDBACK75.md,
                                // Q-1095; branch feedback75).
                                let msg = if self.confined { unquoted_native(&msg) } else { msg };
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
        // A top-level `let`'s variable, as yet unused and without a signature. Not a page state's:
        // `state n` and `let n`, or `state n` twice, are two (FEEDBACK68.md; branch global-state).
        if is_let && g.sig.is_none() && !g.used && g.file == *file && g.kind != Kind::PageState {
            return;
        }
        let at = |f: &str, l: u32| self.shown_at(f, l);
        let msg = format!("`{}` is defined twice, at {} and {}: the second replaces the first everywhere, so rename one", name, at(&g.file, g.line), at(file, line));
        self.diag_at(file.clone(), line, Severity::Error, msg);
    }

    fn run(&mut self, order: Vec<Item>) {
        for it in &order {
            // A component's own, kept apart too, for `let_not_state` (FEEDBACK80.md, Q-1268; branch
            // feedback72-81).
            match &it.stmt {
                Stmt::Fn(d) if d.component => {
                    let mut own = HashSet::new();
                    collect_assigned(std::slice::from_ref(&it.stmt), &mut own);
                    self.assigned.extend(own.iter().cloned());
                    self.component_assigned.insert(def_key(d), own);
                }
                _ => collect_assigned(std::slice::from_ref(&it.stmt), &mut self.assigned),
            }
            global_assigned(std::slice::from_ref(&it.stmt), &mut self.assigned_globals);
        }
        // `let theme = css`...``, and a name or a `+` of such, never changed: what CSS joined from
        // it holds, as far as eight rounds find (FEEDBACK72.md, Q-1017; branch feedback72-81).
        for _ in 0..8 {
            let mut grew = false;
            for it in &order {
                if let Stmt::Let(Pattern::Bind(n), e, _) = &it.stmt {
                    if !self.css_texts.contains(n.as_str()) && !self.assigned.contains(n.as_str()) && css_written(e, &self.css_texts) {
                        self.css_texts.insert(n.to_string());
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
        // Helpers such as `fn table_of(id) = sql_name(`obj_${id}`)`, found
        // again until no more are: one may call another.
        let defs: Vec<&FnDef> = order.iter().filter_map(|it| if let Stmt::Fn(d) = &it.stmt { Some(&**d) } else { None }).collect();
        // Each function read once, and again only when one it calls is found: a round over every
        // function for each one found took 23 s on a chain of 30,000 (FEEDBACK75.md, Q-1053,
        // found in review; branch feedback75). A confined check's clock is read as it goes.
        // Read in an order that has the functions a function calls read first, so a chain of
        // helpers is found in one pass, not one helper a round with its callers read again each
        // round (found in review).
        let callers = callers_by_name(&defs);
        let order_read = callees_first(&defs, &callers);
        let mut graphs: Vec<GraphSlot> = defs.iter().map(|_| GraphSlot::Unread).collect();
        let mut queue = Worklist::new(&order_read);
        while let Some(i) = queue.next() {
            let d = defs[i];
            if d.component || self.sql_name_fns.contains(&d.name) || !gives_sql_safe(&d.body, &self.sql_name_fns, &mut graphs[i]) {
                continue;
            }
            self.sql_name_fns.insert(d.name.clone());
            queue.again(callers.get(d.name.as_str()));
        }
        // And those that give back SQL built from values: from what they are given, or whatever
        // they are given (FEEDBACK75.md, Q-1085; branch feedback75). Only a function a result of
        // which may be built text is read at all.
        let maybe: Vec<bool> = defs.iter().map(|d| !d.component && !self.sql_name_fns.contains(&d.name) && fn_results(d).iter().any(|x| may_build(x))).collect();
        let mut queue = Worklist::new(&order_read);
        while let Some(i) = queue.next() {
            let d = defs[i];
            if !maybe[i] || self.sql_built_fns.get(&d.name) == Some(&true) {
                continue;
            }
            let was = self.sql_built_fns.get(&d.name).copied();
            let Some(always) = self.gives_sql_built(d) else { continue };
            if was != Some(always) {
                self.sql_built_fns.insert(d.name.clone(), always);
                queue.again(callers.get(d.name.as_str()));
            }
        }
        self.check_nesting(&order);
        self.page_states_through_calls(&order);
        self.page_states_to_pages(&order);
        self.stream_sends_in_transactions(&order);
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
        // The types that name themselves with nothing between (`type Ab = Ab | int`): a value of
        // one could be anything, so each is an error, found once over the definitions; the
        // program's walk carries no chain to find one as it runs (FEEDBACK75.md, Q-1080, found in
        // review; branch feedback75).
        let looped = RECORDS.with(|r| crate::record::self_naming(&r.borrow()));
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
                Stmt::Service(name, _, _, _, line, _) => self.defines(name, &it.file, *line, false),
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
                Stmt::Service(name, _, _, _, line, _) => {
                    self.globals.insert(
                        name.clone(),
                        Var { t: T::of(MAP), sig: None, file: it.file.clone(), line: *line, kind: Kind::Global, used: true, ready: false },
                    );
                }
                Stmt::Type(td) => {
                    if looped.contains(&td.name) {
                        // Through a `?`, a list or a record in the way, as `list<Opt>?` says
                        // nothing a list does not (FEEDBACK75.md, Q-1080, found in review; branch
                        // feedback75).
                        let n = &td.name;
                        let way = if RECORDS.with(|r| crate::record::self_naming_optional(&r.borrow(), n)) {
                            format!("put a record or a list in the way (`type {} = {{ next: {}? }}`)", n, n)
                        } else {
                            format!("put a record, a list or a map shape in the way (`type {} = list<{}> | int`)", n, n)
                        };
                        self.diag_at(it.file.clone(), td.line, Severity::Error, format!("type {} names itself with nothing between: {}", n, way));
                    }
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
                Stmt::State(n, _, line) => {
                    self.defines(n, &it.file, *line, false);
                    self.globals.insert(
                        n.clone(),
                        Var { t: T::ANY, sig: None, file: it.file.clone(), line: *line, kind: Kind::PageState, used: false, ready: false },
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
                Stmt::Service(name, _, _, _, _, _) => {
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
                            if matches!(k, SqlVar::Built | SqlVar::Parts(_)) {
                                self.sql_built_vars.insert(n.to_string());
                            }
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
                    self.answer_dropped(other, false);
                    self.op_dropped(other);
                    // And what it ends with, `for x in xs { out.push(x) }` (FEEDBACK80.md, Q-1267;
                    // branch feedback72-81).
                    if let Stmt::Expr(e) = other {
                        self.mark_tails(e, DropHow::Plain, 0);
                    }
                    self.stmt(other, false);
                }
            }
        }
        // Function, component and route bodies run later, when everything
        // top-level exists.
        self.sequential = false;
        let mut ends: Vec<(Arc<str>, u32, String, String)> = Vec::new();
        let mut r = Returns { said: Vec::new(), fns: Vec::new(), ends: Vec::new(), depth: 0 };
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
            // Where each `return` is, what follows one, and how each function ends: read from
            // the code as written, in one walk of each top-level statement (FEEDBACK52.md, Q-678
            // to Q-680; branch feedback52).
            match &it.stmt {
                Stmt::Fn(d) => r.function(d, Whose::Named(&d.name), true),
                Stmt::Route(rd) if rd.method == crate::ast::GUARD_METHOD => {}
                s => {
                    r.stmt(s, Goes::Dropped);
                }
            }
            for (line, msg) in r.said.drain(..) {
                self.diag_at(it.file.clone(), line, Severity::Warning, msg);
            }
            ends.extend(r.ends.drain(..).map(|(line, name, msg)| (it.file.clone(), line, name, msg)));
        }
        // A null end is said of a function the program calls. One it hands to other code (a
        // group's middleware, a `check:`, a callback, a handler) gives what that code makes of
        // null: no match for middleware (decided 2026-11-04), a refusal for `check`, false for
        // `filter` (FEEDBACK52.md, Q-679; branch feedback52).
        if !ends.is_empty() {
            let handed: HashSet<String> = handed_over(&order, &ends.iter().map(|e| e.2.as_str()).collect()).into_iter().map(String::from).collect();
            for (file, line, name, msg) in ends {
                if !handed.contains(&name) {
                    self.diag_at(file, line, Severity::Warning, msg);
                }
            }
        }
        // What each tool's code does, once every body is known, in the order the tools are
        // declared: for its hints and for whether it only reads (FEEDBACK56.md, Q-858; branch
        // feedback56).
        let top_lets: HashSet<String> = order
            .iter()
            .filter_map(|it| match &it.stmt {
                Stmt::Let(p, _, _) => {
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    Some(names)
                }
                _ => None,
            })
            .flatten()
            .collect();
        self.idempotent_reaches_outside(&top_lets);
        // What the components a route's page shows run under its `with` (FEEDBACK80.md, Q-1261;
        // branch feedback72-81).
        self.render_effects(&order);
        // Links to no route, handlers in markup the server alone makes, and what a component draws
        // again as it wakes (FEEDBACK79.md, Q-1251; FEEDBACK73.md, Q-1020; FEEDBACK72.md, Q-1015;
        // branch feedback72-81).
        self.dead_links(&order);
        self.page_handlers(&order);
        self.drawn_twice(&defs, &callers);
        // A GET that writes, run from another site with the visitor's session (FEEDBACK70.md,
        // Q-965; branch security).
        self.get_writes_said(&order, &top_lets);
        let mut server_fn_reach: Option<(HashMap<String, Arc<FnDef>>, HashMap<String, Arc<str>>, HashSet<String>)> = None;
        let mut server_fn_seen = GetSeen::default();
        let mut late = Vec::new();
        for it in &order {
            if let Stmt::Fn(d) = &it.stmt {
                if d.mcp && !d.component && !is_view(d) {
                    let facts = self.tool_facts(d, &it.file, &top_lets);
                    // A question after a change, in the code of a tool that asks (FEEDBACK56.md,
                    // Part E; branch feedback56).
                    if facts.asks {
                        self.ordered_fn(d, &it.file, &top_lets);
                    }
                    self.out.tools.push(facts);
                }
                // Checked as a service: its server fns too, for the program calling them
                // (FEEDBACK56.md, Q-853, B.3, found in review; branch feedback56).
                if d.server && SERVER_FN_FACTS.with(|f| f.get()) {
                    let mut facts = self.tool_facts(d, &it.file, &top_lets);
                    // What it surely writes, for a GET route that calls it (FEEDBACK70.md, Q-965;
                    // branch security).
                    let (fns, files, conns) = server_fn_reach.get_or_insert_with(|| get_program(&order));
                    let cx = GetCx { fns, files, top_lets: &top_lets, conns };
                    let params: Vec<String> = d.params.iter().map(|p| p.name.to_string()).collect();
                    let own = self.get_own(&fn_code(d), &params, &cx);
                    facts.sure_writes = self.get_facts(&own, &it.file, &cx, &mut server_fn_seen, false).writes.into_iter().next().map(|w| w.what);
                    self.out.server_fns.push(facts);
                }
            }
        }
        for o in self.ordered.borrow().values() {
            late.extend(o.late.iter().cloned());
        }
        late.sort();
        // An error: the program does not start with a tool that would act on a yes asked after
        // the change it made (FEEDBACK56.md, Part E, found in review; branch feedback56).
        for (file, line, msg) in late {
            self.diag_at(file, line, Severity::Error, msg);
        }
        self.out.walked = self.walked.get();
    }

    // ---- what a tool does (FEEDBACK56.md, Q-858 and B.3; branch feedback56) --------------

    /// What a tool's code does: each function it names is read once, however many times and
    /// however deep it is reached, a recursive one too, and the defaults of each record it takes,
    /// gives or makes, which run as one is made, the door's arguments too. What each one's own
    /// code does is kept for the check, so a function many tools reach is read once (found in
    /// review).
    fn tool_facts(&self, d: &Arc<FnDef>, file: &Arc<str>, top_lets: &HashSet<String>) -> ToolFacts {
        let r = self.reach_all(d, top_lets);
        ToolFacts { def: d.clone(), file: file.to_string(), effects: r.effects, uncertain: r.uncertain, writes: r.writes, open: r.open, db: r.db, asks: r.asks, sure_writes: None }
    }

    /// What `d`'s code does, with every function it names, each read once (`tool_facts`).
    fn reach_all(&self, d: &Arc<FnDef>, top_lets: &HashSet<String>) -> Seen {
        let mut r = Seen::default();
        let mut todo = vec![Next::Fn(d.clone())];
        let (mut seen, mut types) = (HashSet::new(), HashSet::new());
        while let Some(next) = todo.pop() {
            let local = match next {
                Next::Fn(f) if seen.insert(def_key(&f)) => self.local_fn(&f, top_lets),
                Next::Type(n) if types.insert(n.clone()) => self.local_type(&n, top_lets),
                _ => continue,
            };
            r.join(&local.seen);
            todo.extend(local.next.iter().cloned());
        }
        r
    }

    /// One function's own code, read once a check.
    fn local_fn(&self, f: &Arc<FnDef>, top_lets: &HashSet<String>) -> Arc<Reached> {
        if let Some(l) = self.reached_fns.borrow().get(&def_key(f)) {
            return l.clone();
        }
        let (mut seen, mut next) = (Seen::default(), Vec::new());
        self.reach_def(f, top_lets, &mut seen, &mut next);
        self.walked.set(self.walked.get() + 1);
        let l = Arc::new(Reached { seen, next });
        self.reached_fns.borrow_mut().insert(def_key(f), l.clone());
        l
    }

    /// One type: a record's defaults, and the types its fields name; read once a check.
    fn local_type(&self, n: &str, top_lets: &HashSet<String>) -> Arc<Reached> {
        if let Some(l) = self.reached_types.borrow().get(n) {
            return l.clone();
        }
        let (mut seen, mut next) = (Seen::default(), Vec::new());
        if let Some(td) = record(n) {
            let mut names = Vec::new();
            ty_names(&td.ty, &mut names);
            next.extend(names.into_iter().map(Next::Type));
            let defaults: Vec<&Expr> = td.defaults.iter().map(|(_, e)| e).collect();
            self.reach_code(&defaults, top_lets, &mut seen, &mut next);
            self.walked.set(self.walked.get() + 1);
        }
        let l = Arc::new(Reached { seen, next });
        self.reached_types.borrow_mut().insert(n.to_string(), l.clone());
        l
    }

    /// One function: the types of its parameters and its result, which a call makes, and its
    /// code.
    fn reach_def(&self, d: &FnDef, top_lets: &HashSet<String>, r: &mut Seen, todo: &mut Vec<Next>) {
        let mut names = Vec::new();
        d.params.iter().filter_map(|p| p.ty.as_ref()).chain(d.ret.iter()).for_each(|t| ty_names(t, &mut names));
        todo.extend(names.into_iter().map(Next::Type));
        let code: Vec<&Expr> = d.params.iter().filter_map(|p| p.default.as_ref()).chain(std::iter::once(&d.body)).collect();
        self.reach_code(&code, top_lets, r, todo);
    }

    /// Code: what it calls and names, the program's variables it assigns, the types its lambdas
    /// and patterns make.
    fn reach_code(&self, code: &[&Expr], top_lets: &HashSet<String>, r: &mut Seen, todo: &mut Vec<Next>) {
        // Its own `fn`s inside it: a call of one is a call of code read here. One named as a
        // function of the program's is taken for that one too (found in review).
        let mut local_fns = HashSet::new();
        for e in code {
            walk_expr(e, &mut |x| {
                if let Expr::Block(stmts) = x {
                    local_fns.extend(stmts.iter().filter_map(|s| if let Stmt::Fn(f) = s { Some(f.name.clone()) } else { None }));
                }
            });
        }
        // The program's variables it assigns, a local of the same name taken for one, so none is
        // missed: the server's state, every caller's.
        let mut assigned = HashSet::new();
        code.iter().for_each(|e| assigned_in(e, &mut assigned));
        if let Some(n) = assigned.iter().filter(|n| top_lets.contains(n.as_str())).min() {
            note(&mut r.writes, format!("assigns the program's variable `{}`", n));
        }
        // The callees and receivers a call names, so they are not read again as values.
        let mut called: HashSet<*const Expr> = HashSet::new();
        for e in code {
            walk_expr(e, &mut |x| {
                if called.contains(&(x as *const Expr)) {
                    return;
                }
                match x {
                    Expr::Call(callee, args, _) => self.reach_call(callee, args.first(), &local_fns, &mut called, r, todo),
                    // `a | f(b)` calls `f(a, b)`: `a` is the statement of `"select ..." | db.query()`.
                    Expr::Pipe(lhs, rhs, _) => {
                        called.insert(&**rhs as *const Expr);
                        match &**rhs {
                            Expr::Call(c, _, _) => self.reach_call(c, Some(lhs), &local_fns, &mut called, r, todo),
                            other => self.reach_call(other, Some(lhs), &local_fns, &mut called, r, todo),
                        }
                    }
                    Expr::Ident(n, _) => self.reach_name(n, None, false, &local_fns, r, todo),
                    // `<Card/>` runs the view or the component.
                    Expr::Html(el, _) => walk_html_els(el, &mut |e| {
                        if e.tag.starts_with(|c: char| c.is_uppercase()) {
                            self.reach_name(&e.tag, None, true, &local_fns, r, todo);
                        }
                    }),
                    // `db.exec` given as a value is called by what takes it, with what it is given.
                    Expr::Field(obj, f, _) => {
                        if let Expr::Ident(m, _) = &**obj {
                            if self.module_named(m) {
                                called.insert(&**obj as *const Expr);
                                if !MODULE_VALUES.contains(&format!("{}.{}", m, f).as_str()) {
                                    self.reach_builtin(&format!("{}.{}", m, f), None, false, r);
                                }
                            } else if self.services.contains_key(m.as_str()) || self.sites.contains(m.as_str()) {
                                note(&mut r.uncertain, format!("`{}.{}` is passed as a value", m, f));
                            }
                        }
                    }
                    // A record a lambda or a pattern makes: its defaults run (found in review).
                    Expr::Lambda(d) => {
                        let mut names = Vec::new();
                        d.params.iter().filter_map(|p| p.ty.as_ref()).chain(d.ret.iter()).for_each(|t| ty_names(t, &mut names));
                        todo.extend(names.into_iter().map(Next::Type));
                    }
                    Expr::Match(_, arms, _) => arms.iter().for_each(|a| pattern_type_names(&a.pat, todo)),
                    Expr::For(p, _, _, _) => pattern_type_names(p, todo),
                    Expr::Block(stmts) => {
                        for st in stmts {
                            match st {
                                Stmt::Let(p, _, _) => pattern_type_names(p, todo),
                                Stmt::Fn(f) => {
                                    let mut names = Vec::new();
                                    f.params.iter().filter_map(|p| p.ty.as_ref()).chain(f.ret.iter()).for_each(|t| ty_names(t, &mut names));
                                    todo.extend(names.into_iter().map(Next::Type));
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            });
        }
    }

    /// `about`'s question, choices and default, where they are written: a question with no text,
    /// no choice, a choice that is not text, a default that is not one of the choices. It was
    /// `ask`'s, with three arguments (FEEDBACK56.md, the user's decision 2026-11-09; branch
    /// feedback56).
    fn about_written(&mut self, args: &[Expr]) {
        if matches!(&args[0], Expr::Lit(Lit::Str(q)) if q.trim().is_empty()) {
            self.error("`about`'s question is empty: say what the user is asked");
        }
        let Expr::Map(entries) = &args[1] else { return };
        if entries.is_empty() {
            self.error("`about` has no choices: give one at least, as in { public: \"Everyone\", draft: \"Only me\" }");
            return;
        }
        for (k, v) in entries {
            if let Expr::Lit(l) = v {
                if !matches!(l, Lit::Str(_)) {
                    self.error(format!("`about`'s choice `{}` is not text: each choice is the text the user reads, under the key `about` gives", k));
                }
            }
        }
        // Keys a spread may add are not known.
        if entries.iter().any(|(k, _)| k.starts_with("...")) {
            return;
        }
        if let Expr::Lit(Lit::Str(d)) = &args[2] {
            if !entries.iter().any(|(k, _)| k.as_str() == &**d) {
                let keys: Vec<String> = entries.iter().map(|(k, _)| format!("`{}`", k)).collect();
                self.error(format!("`about`'s default {:?} is not one of its choices: {}", &**d, keys.join(", ")));
            }
        }
    }

    /// Whether `m` is a module, not hidden by the program's own name.
    fn module_named(&self, m: &str) -> bool {
        MODULES.contains(&m) && self.globals.get(m).map_or(true, |g| &*g.file == "<builtin>")
    }

    /// A call: of a name, of a module's function, of a service or a site, a method, or a
    /// lambda written there; anything else gives a function the checker cannot see.
    #[allow(clippy::too_many_arguments)]
    fn reach_call(&self, callee: &Expr, first: Option<&Expr>, local_fns: &HashSet<String>, called: &mut HashSet<*const Expr>, r: &mut Seen, todo: &mut Vec<Next>) {
        let http = crate::ast::effect_bit("http");
        // A callee named is not a value read; a call that gives the function is read as a call.
        if matches!(callee, Expr::Ident(..) | Expr::Field(..)) {
            called.insert(callee as *const Expr);
        }
        match callee {
            Expr::Ident(n, _) => self.reach_name(n, first, true, local_fns, r, todo),
            Expr::Field(obj, f, _) => match &**obj {
                Expr::Ident(m, _) if self.module_named(m) => {
                    called.insert(&**obj as *const Expr);
                    self.reach_builtin(&format!("{}.{}", m, f), first, true, r);
                }
                // The program's own service: inside the program only when its code, read as a
                // program of its own, stays inside (B.3, found in review).
                Expr::Ident(m, _) if self.services.contains_key(m.as_str()) => {
                    called.insert(&**obj as *const Expr);
                    r.effects |= http;
                    self.reach_service(m, f, r);
                    // A change unless it is seen whole to write nothing, as a tool's question
                    // after it is held to (FEEDBACK56.md, Part F, security review).
                    if !self.service_fn(m, f).is_some_and(|x| x.sure && !x.writes) {
                        note(&mut r.writes, format!("calls `{}.{}`, a call of another program", m, f));
                        r.elsewhere.push(format!("{}.{}", m, f));
                    }
                }
                Expr::Field(inner, later, _) if later == "later" && matches!(&**inner, Expr::Ident(s, _) if self.services.contains_key(s.as_str())) => {
                    called.insert(&**obj as *const Expr);
                    called.insert(&**inner as *const Expr);
                    r.effects |= http;
                    if let Expr::Ident(m, _) = &**inner {
                        self.reach_service(m, f, r);
                        note(&mut r.writes, format!("`{}.later.{}` makes a call later", m, f));
                    }
                }
                Expr::Ident(m, _) if self.sites.contains(m.as_str()) => {
                    called.insert(&**obj as *const Expr);
                    r.effects |= http;
                    note(&mut r.open, format!("calls the site `{}`", m));
                    note(&mut r.writes, format!("calls the site `{}`, which may change anything", m));
                    r.elsewhere.push(format!("{}.{}", m, f));
                }
                // `xs.map(f)` is `map(xs, f)`, unless the value is a map that holds a function of
                // that name, which came from code read here or from a value said unseen.
                _ if local_fns.contains(f.as_str()) || self.globals.get(f.as_str()).is_some_and(|g| g.sig.is_some()) => {
                    self.reach_name(f, None, true, local_fns, r, todo)
                }
                _ => note(&mut r.uncertain, format!("`.{}(...)` calls a function a value holds", f)),
            },
            Expr::Lambda(_) => {}
            _ => note(&mut r.uncertain, "calls a function an expression gives".to_string()),
        }
    }

    /// A call of the service `s`'s `f`: outside the program unless its code was read and stays
    /// inside (FEEDBACK56.md, Q-853, B.3, found in review; branch feedback56). The tool asks the
    /// user when `f` does, through its door, as it runs with the call's asking; and one whose code
    /// is not read, or not all seen, may (FEEDBACK56.md, Part F; branch feedback56). A call made
    /// later of one that asks is followed too, so that its order is read: its question would be
    /// refused as it runs, and `order_call` says so.
    fn reach_service(&self, s: &str, f: &str, r: &mut Seen) {
        if !self.service_stays_inside(s, f) {
            note(&mut r.open, format!("calls `{}.{}`, whose code reaches outside the program or is not read here", s, f));
        }
        match self.service_fn(s, f) {
            Some(x) if x.sure => r.asks |= x.asks,
            Some(_) => note(&mut r.uncertain, format!("calls `{}.{}`, whose code the checker cannot see whole", s, f)),
            None => note(&mut r.uncertain, format!("calls `{}.{}`, whose code is not read here", s, f)),
        }
    }

    /// Whether the service `s`'s function `f` stays inside the program (`service_fn`).
    fn service_stays_inside(&self, s: &str, f: &str) -> bool {
        self.service_fn(s, f).is_some_and(|x| x.inside)
    }

    /// What the service `s`'s function `f` does: the service read as a program of its own, its
    /// server fns walked, once, when a tool first calls it, not for every program that has
    /// services (found in review). None where it was not read.
    fn service_fn(&self, s: &str, f: &str) -> Option<ServiceFn> {
        if let Some(read) = self.service_closed.borrow().get(s) {
            return read.as_ref().and_then(|m| m.get(f)).copied();
        }
        let read = self.service_files.get(s).and_then(|full| {
            let key = full.canonicalize().unwrap_or_else(|_| full.clone());
            // One being read up the stack, calling back: not read again.
            if !SERVICE_CHECKS.with(|c| c.borrow_mut().insert(key.clone())) {
                return None;
            }
            let asked = SERVER_FN_FACTS.with(|x| x.replace(true));
            let sub = check_file_opts(full, HashMap::new(), self.options());
            SERVER_FN_FACTS.with(|x| x.set(asked));
            SERVICE_CHECKS.with(|c| c.borrow_mut().remove(&key));
            self.walked.set(self.walked.get() + sub.walked);
            let net = crate::ast::effect_bit("net");
            Some(
                sub.server_fns
                    .iter()
                    .map(|x| {
                        let sure = x.uncertain.is_none();
                        (x.def.name.clone(), ServiceFn { inside: sure && x.open.is_none() && x.effects & net == 0, asks: x.asks, writes: x.writes.is_some(), sure, sure_writes: x.sure_writes.is_some() })
                    })
                    .collect::<HashMap<_, _>>(),
            )
        });
        let found = read.as_ref().and_then(|m| m.get(f)).copied();
        self.service_closed.borrow_mut().insert(s.to_string(), read);
        found
    }

    /// A name called (`called`) or read as a value: the program's function is read too, a
    /// builtin's effects taken, and a variable that may hold a function makes the rest unseen.
    #[allow(clippy::too_many_arguments)]
    fn reach_name(&self, n: &str, first: Option<&Expr>, called: bool, local_fns: &HashSet<String>, r: &mut Seen, todo: &mut Vec<Next>) {
        // A local `fn` is read where it is written; the program's of that name is read too, as
        // the local one may be in another block (found in review).
        if local_fns.contains(n) && !self.globals.contains_key(n) {
            return;
        }
        let Some(g) = self.globals.get(n) else {
            if called {
                note(&mut r.uncertain, format!("`{}(...)` calls a parameter or a variable", n));
            }
            return;
        };
        match &g.sig {
            Some(s) if s.native.is_some() => note(&mut r.uncertain, format!("`{}` is a native plugin's function, whose code the checker cannot read", n)),
            Some(s) if s.builtin => self.reach_builtin(n, first, called, r),
            Some(s) => match &s.def {
                Some(d) => todo.push(Next::Fn(d.clone())),
                // A record made: its defaults run (found in review).
                None => todo.push(Next::Type(n.to_string())),
            },
            None if &*g.file == "<builtin>" => {
                if !called {
                    note(&mut r.uncertain, format!("the module `{}` is passed as a value", n));
                } else {
                    r.effects |= builtin_effect(n);
                }
            }
            None if self.services.contains_key(n) || self.sites.contains(n) => note(&mut r.uncertain, format!("`{}` is passed as a value", n)),
            None if !called && plain_data(g.t) => {}
            None => note(&mut r.uncertain, format!("`{}` is the program's variable, which may hold a function", n)),
        }
    }

    /// A builtin reached: called with `first` as its first argument, or given as a value. What
    /// changes something is what the view cache says acts (`viewcache::kind`), one list for
    /// every builtin there is, a random number apart (found in review).
    fn reach_builtin(&self, name: &str, first: Option<&Expr>, called: bool, r: &mut Seen) {
        let effect = builtin_effect(name);
        r.effects |= effect;
        let acts = crate::viewcache::kind(name) == crate::viewcache::Kind::Acts && effect != crate::ast::effect_bit("random");
        // It reaches the database (found in review).
        if (name.starts_with("db.") && !matches!(name, "db.error_kind" | "db.sql_name")) || matches!(name, "idempotent" | "later.status" | "with_tenant" | "new_tenant") {
            r.db = true;
        }
        // Another database is another place (found in review).
        if matches!(name, "db.connect" | "db.shards") {
            note(&mut r.open, format!("`{}` reaches another database", name));
        }
        // It asks the user: a read, of the user (FEEDBACK56.md, Part E; branch feedback56). `about`
        // is a tool's question, once `ask`'s with three arguments; `ask`, the page's, is refused in
        // a tool's call, which runs with its asking to stop there (the user's decision 2026-11-09).
        if matches!(name, "ask" | "about" | "confirm") {
            r.asks = true;
        }
        match name {
            "js" | "js_new" | "run_script" => note(&mut r.uncertain, format!("`{}` runs code the checker cannot read", name)),
            // A `select` written in the program reads, as every database reads it; a statement
            // built as the program runs, or given elsewhere, may write. One that calls a function
            // its database does not ship may write too, and reach outside (found in review).
            "db.query" | "db.one" => match first.filter(|_| called).map(sql_written) {
                Some(Some(sql)) if !reads_written(&sql) => note(&mut r.writes, format!("`{}(\"{}\")` is not a `select` that only reads", name, sql.trim())),
                Some(Some(sql)) => {
                    if let Some(f) = crate::sql::foreign_call(&sql) {
                        let why = format!("`{}(\"{}\")` calls `{}`, which its database does not ship as a function that only computes", name, sql.trim(), f);
                        note(&mut r.writes, why.clone());
                        note(&mut r.open, why);
                    }
                }
                _ => note(&mut r.writes, format!("`{}` of a statement not written in the program", name)),
            },
            "db.error_kind" | "db.sql_name" => {}
            // `with_tenant` runs its function on another tenant's database, outside the
            // transaction a tool that only reads is run in.
            n if n.starts_with("db.") || n == "idempotent" || n == "later.status" || n == "with_tenant" => note(&mut r.writes, format!("`{}`", n)),
            n if acts => note(&mut r.writes, format!("`{}` changes something", n)),
            _ => {}
        }
        // `receive` and `listen` send a request with no effect of their own.
        if matches!(name, "receive" | "listen") {
            r.effects |= crate::ast::effect_bit("http");
        }
        if name.starts_with("http.") || matches!(name, "receive" | "listen") || effect & crate::ast::effect_bit("net") != 0 {
            note(&mut r.open, format!("`{}`", name));
        }
    }

    // ---- a tool asks before it changes anything (FEEDBACK56.md, Part E; branch feedback56) ----
    //
    // A tool's call that stops to ask runs again from the top with the answer, so what it changed
    // before the question is changed twice, or acted on before the yes: here an error where the
    // order can be seen (found in review), and at run time the rounds' database writes are rolled
    // back at a stop and any other change before a question refused. A change is what viewcache
    // says acts (but a random number), a statement written in the program that is not a `select`
    // or calls a function its database does not ship (slice 3's rule), a call of a service or a
    // site, the program's variable assigned, or a call of the program's function that does one;
    // code in a lambda given to a call is taken to run during it.

    /// What `d`'s code does in the order it runs, read once a check; a function reached again
    /// while it is read (recursion) is taken to do nothing more.
    fn ordered_fn(&self, d: &Arc<FnDef>, file: &Arc<str>, top_lets: &HashSet<String>) -> Arc<Ordered> {
        if let Some(o) = self.ordered.borrow().get(&def_key(d)) {
            return o.clone();
        }
        self.ordered.borrow_mut().insert(def_key(d), Arc::new(Ordered::default()));
        let file = d.file.clone().unwrap_or_else(|| file.clone());
        let locals = d.params.iter().map(|p| p.name.to_string()).collect();
        let mut w = OrderWalk { top_lets, file, locals, changed: None, asks: None, late: Vec::new(), loops: 0, ended: false, left: None };
        for p in &d.params {
            if let Some(e) = &p.default {
                self.order_expr(e, &mut w);
            }
        }
        self.order_expr(&d.body, &mut w);
        let o = Arc::new(Ordered { changes: w.changed.or(w.left), asks: w.asks, late: w.late });
        self.ordered.borrow_mut().insert(def_key(d), o.clone());
        o
    }

    fn order_expr(&self, e: &Expr, w: &mut OrderWalk) {
        match e {
            Expr::Lit(_) | Expr::Ident(..) | Expr::Lambda(_) => {}
            Expr::Template(parts) => parts.iter().for_each(|p| {
                if let TPart::Expr(x) = p {
                    self.order_expr(x, w)
                }
            }),
            Expr::List(xs) => xs.iter().for_each(|x| self.order_expr(x, w)),
            Expr::Map(es) => es.iter().for_each(|(_, x)| self.order_expr(x, w)),
            Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => self.order_expr(x, w),
            Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Index(a, b, _) => {
                self.order_expr(a, w);
                self.order_expr(b, w)
            }
            // `a | f(b)` is `f(a, b)`, `a` worked out first.
            Expr::Pipe(a, rhs, line) => {
                self.order_expr(a, w);
                match &**rhs {
                    Expr::Call(c, args, l) => self.order_call(c, Some(a), args, *l, w),
                    other => self.order_call(other, Some(a), &[], *line, w),
                }
            }
            Expr::Call(c, args, line) => self.order_call(c, args.first(), args, *line, w),
            // Either way may be the one taken: after it, the change of either.
            Expr::If(c, a, b) => {
                self.order_expr(c, w);
                let ways: Vec<&Expr> = std::iter::once(&**a).chain(b.as_deref()).collect();
                self.order_ways(&ways, b.is_some(), w);
            }
            // A value no arm takes is an error: the arms are all its ways.
            Expr::Match(s, arms, _) => {
                self.order_expr(s, w);
                let before = w.changed.clone();
                let (mut after, mut all_end) = (None, !arms.is_empty());
                for a in arms {
                    w.changed = before.clone();
                    w.ended = false;
                    if let Some(g) = &a.guard {
                        self.order_expr(g, w);
                    }
                    self.order_expr(&a.body, w);
                    if w.ended {
                        w.left = w.left.take().or(w.changed.take());
                    } else {
                        after = after.or(w.changed.take());
                    }
                    all_end &= w.ended;
                }
                w.changed = before.or(after);
                w.ended = all_end;
            }
            Expr::For(_, items, body, _) => {
                self.order_expr(items, w);
                self.order_loop(&[], body, w);
            }
            Expr::While(c, body, _) => self.order_loop(&[c], body, w),
            Expr::Block(stmts) => {
                for st in stmts {
                    if w.ended {
                        break;
                    }
                    match st {
                        Stmt::Let(p, x, _) => {
                            self.order_expr(x, w);
                            let mut names = Vec::new();
                            pattern_names(p, &mut names);
                            w.locals.extend(names);
                        }
                        Stmt::Assign(n, _, x, _) => {
                            self.order_expr(x, w);
                            if w.top_lets.contains(n.as_str()) && !w.locals.contains(n.as_str()) && w.changed.is_none() {
                                w.changed = Some(format!("assigning the program's variable `{}`", n));
                            }
                        }
                        Stmt::SetField(o, _, _, x, _) => {
                            self.order_expr(o, w);
                            self.order_expr(x, w)
                        }
                        Stmt::Expr(x) | Stmt::State(_, x, _) => self.order_expr(x, w),
                        Stmt::Return(x, _) => {
                            if let Some(x) = x {
                                self.order_expr(x, w);
                            }
                            w.ended = true;
                        }
                        Stmt::Fn(f) => {
                            w.locals.insert(f.name.clone());
                        }
                        _ => {}
                    }
                }
            }
            Expr::Reply(st, b, _) => {
                self.order_expr(st, w);
                if let Some(b) = b {
                    self.order_expr(b, w)
                }
                w.ended = true;
            }
            Expr::Html(el, _) => {
                let mut parts = Vec::new();
                walk_html_els(el, &mut |e| {
                    parts.extend(e.id.iter().cloned());
                    parts.extend(e.classes.iter().cloned());
                    for (_, v) in &e.attrs {
                        if let AttrVal::Expr(x) | AttrVal::Spread(x) = v {
                            parts.push(x.clone());
                        }
                    }
                    for c in &e.children {
                        if let HtmlChild::Expr(x) = c {
                            parts.push(x.clone());
                        }
                    }
                });
                parts.iter().for_each(|x| self.order_expr(x, w));
            }
        }
    }

    /// Ways one of which is taken: each from the change before them, and after them the change of
    /// any that goes on (`OrderWalk::ended`). `all`: they are all the ways, an `if` with its `else`;
    /// without one, the way past it goes on.
    fn order_ways(&self, ways: &[&Expr], all: bool, w: &mut OrderWalk) {
        let before = w.changed.clone();
        let (mut after, mut all_end) = (None, all);
        for x in ways {
            w.changed = before.clone();
            w.ended = false;
            self.order_expr(x, w);
            if w.ended {
                w.left = w.left.take().or(w.changed.take());
            } else {
                after = after.or(w.changed.take());
            }
            all_end &= w.ended;
        }
        w.changed = before.or(after);
        w.ended = all_end;
    }

    /// A loop: its test and body, then again, as a second turn comes after the first's changes.
    fn order_loop(&self, test: &[&Expr], body: &Expr, w: &mut OrderWalk) {
        let turns = if w.loops < 3 { 2 } else { 1 };
        w.loops += 1;
        for _ in 0..turns {
            test.iter().for_each(|t| self.order_expr(t, w));
            self.order_expr(body, w);
            // A turn that left ends no other, and the loop may run none: the way goes on after it.
            w.ended = false;
        }
        w.loops -= 1;
    }

    /// A call: the receiver, the arguments in order, the lambdas given (taken to run during it),
    /// then what the call itself does.
    fn order_call(&self, callee: &Expr, first: Option<&Expr>, args: &[Expr], line: u32, w: &mut OrderWalk) {
        let module = |o: &Expr| matches!(o, Expr::Ident(m, _) if self.module_named(m) || self.services.contains_key(m.as_str()) || self.sites.contains(m.as_str()));
        match callee {
            Expr::Ident(..) => {}
            Expr::Field(obj, _, _) if module(obj) || matches!(&**obj, Expr::Field(inner, l, _) if l == "later" && module(inner)) => {}
            Expr::Field(obj, _, _) => self.order_expr(obj, w),
            Expr::Lambda(_) => {}
            other => self.order_expr(other, w),
        }
        for a in args.iter().filter(|a| !matches!(a, Expr::Lambda(_))) {
            self.order_expr(a, w);
        }
        // A lambda's `return` or `reply` ends it, not the way the call is on: what it changed on
        // any of its ways is changed once the call is made.
        let lambdas = args.iter().chain(std::iter::once(callee)).filter_map(|a| if let Expr::Lambda(d) = a { Some(d) } else { None });
        for d in lambdas {
            let (ended, left) = (w.ended, w.left.take());
            self.order_expr(&d.body, w);
            w.changed = w.changed.take().or(w.left.take());
            (w.ended, w.left) = (ended, left);
        }
        // `ask`, the page's dialog, in a tool's code: refused, as the call is when it runs (second
        // review), pointing at `about` and `confirm`; with a third argument it is refused anywhere
        // (`Checker::apply`; FEEDBACK56.md, the user's decision 2026-11-09; branch feedback56). A
        // piped value and a method's receiver are its first.
        let piped = first.is_some() && first.map(|f| f as *const Expr) != args.first().map(|a| a as *const Expr);
        let builtin_ask = || self.globals.get("ask").is_some_and(|g| &*g.file == "<builtin>");
        let given = match callee {
            Expr::Ident(n, _) if n == "ask" && !w.locals.contains("ask") && builtin_ask() => Some(args.len() + piped as usize),
            Expr::Field(obj, n, _) if n == "ask" && !module(obj) && builtin_ask() => Some(args.len() + 1 + piped as usize),
            _ => None,
        };
        if given.is_some_and(|n| n < 3) {
            w.late.push((w.file.clone(), line, crate::mcp::ASK_IN_A_TOOL.to_string()));
        }
        // A call made later of a service's function that asks: no one will be there to answer
        // (FEEDBACK56.md, Part F; branch feedback56).
        if let Expr::Field(obj, f, _) = callee {
            if let Expr::Field(inner, later, _) = &**obj {
                if let (true, Expr::Ident(m, _)) = (later == "later", &**inner) {
                    if self.services.contains_key(m.as_str()) && self.service_fn(m, f).is_some_and(|x| x.asks) {
                        w.late.push((w.file.clone(), line, format!("`{}.later.{}` asks the user: {}", m, f, crate::mcp::NOBODY)));
                    }
                }
            }
        }
        let (asks, changes) = self.call_does(callee, first, w);
        if let Some(a) = asks {
            if let Some(c) = &w.changed {
                w.late.push((w.file.clone(), line, format!("{} comes after {}: {}", a, c, crate::mcp::ASK_FIRST)));
            }
            w.asks.get_or_insert(a);
        }
        if let Some(c) = changes {
            w.changed.get_or_insert(c);
        }
    }

    /// What a call does in itself: whether it asks the user, and what it changes, each said.
    fn call_does(&self, callee: &Expr, first: Option<&Expr>, w: &OrderWalk) -> (Option<String>, Option<String>) {
        let builtin = |name: &str| -> (Option<String>, Option<String>) {
            // `about`, once `ask` with three arguments (FEEDBACK56.md, the user's decision
            // 2026-11-09; branch feedback56).
            if matches!(name, "ask" | "about" | "confirm") {
                return (Some(format!("`{}`", name)), None);
            }
            let acts = crate::viewcache::kind(name) == crate::viewcache::Kind::Acts && builtin_effect(name) != crate::ast::effect_bit("random");
            let statement = matches!(name, "db.query" | "db.one" | "db.all") && first.and_then(sql_written).is_some_and(|sql| !reads_written(&sql) || crate::sql::foreign_call(&sql).is_some());
            (None, (acts || statement).then(|| format!("`{}`", crate::native::display_name(name))))
        };
        let program = |f: &str| -> (Option<String>, Option<String>) {
            match self.globals.get(f).and_then(|g| g.sig.as_ref()) {
                Some(s) if s.builtin || s.native.is_some() => builtin(if s.native.is_some() { s.name.as_str() } else { f }),
                Some(s) => match &s.def {
                    Some(d) => {
                        let o = self.ordered_fn(d, &w.file, w.top_lets);
                        (o.asks.as_ref().map(|a| format!("`{}`, which asks the user ({}),", f, a)), o.changes.as_ref().map(|c| format!("`{}`, which changes something ({})", f, c)))
                    }
                    None => (None, None),
                },
                None => (None, None),
            }
        };
        match callee {
            Expr::Ident(n, _) if w.locals.contains(n.as_str()) => (None, None),
            Expr::Ident(n, _) => program(n),
            Expr::Field(obj, f, _) => match &**obj {
                Expr::Ident(m, _) if self.module_named(m) => builtin(&format!("{}.{}", m, f)),
                // A service's function runs with the call's asking: it asks the user when it does,
                // and changes something unless it is seen whole to write nothing (FEEDBACK56.md,
                // Part F; branch feedback56).
                Expr::Ident(m, _) if self.services.contains_key(m.as_str()) && !self.sites.contains(m.as_str()) => {
                    let x = self.service_fn(m, f);
                    let asks = x.filter(|x| x.sure && x.asks).map(|_| format!("`{}.{}`, which asks the user,", m, f));
                    (asks, (!x.is_some_and(|x| x.sure && !x.writes)).then(|| format!("`{}.{}`, a call of another program", m, f)))
                }
                Expr::Ident(m, _) if self.services.contains_key(m.as_str()) || self.sites.contains(m.as_str()) => (None, Some(format!("`{}.{}`, a call of another program", m, f))),
                Expr::Field(inner, later, _) if later == "later" && matches!(&**inner, Expr::Ident(m, _) if self.services.contains_key(m.as_str())) => (None, Some(format!("`{}`, a call of another program", f))),
                _ if !w.locals.contains(f.as_str()) => program(f),
                _ => (None, None),
            },
            _ => (None, None),
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
        // The names the program's top-level `let`s bind, with what `let name = ...` gives, for
        // middleware named by one; only in a program with offline routes (coverage of 0.8.7,
        // Q-1102; branch coverage-087).
        let offline = routes.iter().any(|(_, r)| r.offline);
        let lets: HashMap<String, Option<&Expr>> = order
            .iter()
            .filter(|_| offline)
            .filter_map(|it| match &it.stmt {
                Stmt::Let(Pattern::Bind(n), init, _) => Some(vec![(n.to_string(), Some(init))]),
                Stmt::Let(p, _, _) => {
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    Some(names.into_iter().map(|n| (n, None)).collect())
                }
                _ => None,
            })
            .flatten()
            .collect();
        // A route that reads who the visitor is, there or in a function it calls, and marks its
        // answer for every cache: sent as `private`, said where it is written (FEEDBACK59.md, Q-903;
        // branch cache-and-migrate).
        for (file, r) in &routes {
            // A page kept for offline reads who the visitor is through the session alone, which its
            // kept copy is shown again for; one that reads them another way is not kept
            // (FEEDBACK67.md, B.2, review; branch pwa).
            // Its groups' middleware as the server reads it too (coverage of 0.8.7, Q-1102; branch
            // coverage-087).
            let why = if !r.offline {
                None
            } else if beyond_session(r, &fns) {
                Some(OFFLINE_READS.to_string())
            } else {
                unkept_by_middleware(r, &fns, &lets)
            };
            if let Some(why) = why {
                self.diag_at(file.clone(), r.line, Severity::Warning, format!("`route offline {}` reads who the visitor is other than through `session()` ({}): its page is not kept for offline, and the offline page is shown in its place; read who they are with `session()`", r.shown(), why));
            }
            if let Some(line) = public_after_visitor(r, &fns) {
                self.diag_at(file.clone(), line, Severity::Warning, "this route reads who the visitor is (the session, a cookie, a header) and its `cache-control` lets a shared cache keep the answer: Quillo sends it as `private`, as a cache in front would give it to the next visitor, and the time stays for the visitor's own browser. Write `private` in it to say so".to_string());
            }
        }
        let mut told: HashSet<String> = HashSet::new();
        let mut explored: HashMap<Vec<String>, HashSet<Arc<str>>> = HashMap::new();
        // A page written as a top-level lambda, `let home = () => <TicketList/>`, is followed as a
        // function is (FEEDBACK79.md, Q-1258, found in review; branch feedback72-81).
        let mut page_fns = fns.clone();
        for it in order {
            if let Stmt::Let(Pattern::Bind(n), Expr::Lambda(d), _) = &it.stmt {
                page_fns.entry(n.to_string()).or_insert_with(|| d.clone());
            }
        }
        for (_, r) in &routes {
            let Some(guard) = middleware_of(r) else { continue };
            for d in server_fns_reached(&page_fns, &r.body, &route_bound(r), &mut self.page_refs, &mut explored) {
                // Each server fn looked at once, for the first route whose page reaches it: one
                // layout's helpers reach the same ones from every route (Q-1258).
                if !told.insert(d.name.clone()) || guards_itself(&fns, &d.body, &mut HashSet::new()) {
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
        if matches!(sql, SqlVar::Built | SqlVar::Parts(_)) {
            self.sql_built_vars.insert(name.to_string());
        }
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
                let kind = if kind == Kind::State { Kind::PageState } else { Kind::Global };
                self.globals.insert(name.to_string(), Var { kind, ..v });
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
                if v.kind == Kind::PageState {
                    self.page_state_here(name);
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
                            // A field the route's pattern checks against a set written out there
                            // binds nothing, and says so (FEEDBACK58.md, Q-874; branch
                            // cache-and-migrate).
                            let set = self.set_fields.iter().find(|(n, _)| n == name).map(|(_, at)| {
                                format!(": the route's pattern checks `{}` against the values written there, which binds no name. Give the set a name (`type Choice = \"a\" | \"b\"`) and write `{}: Choice` there to have `{}`, or read `request.{}`", at, name, name, at)
                            });
                            let hint = if let Some(set) = set { set } else if !removed.is_empty() { removed } else { self.lookalike(name).unwrap_or_else(|| self.suggest(name)) };
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
        let count = name.chars().count();
        // The first by name and line, as when they were sorted, without sorting every name for
        // each unknown one; two ASCII names look alike only if they are one, and letters are taken
        // one for one (FEEDBACK75.md, Q-1120, found in review; branch feedback75).
        let cands = self.scopes.iter().flat_map(|s| s.iter().map(|(k, v)| (k.as_str(), v.line))).chain(self.globals.iter().filter(|(_, v)| &*v.file != "<builtin>").map(|(k, v)| (k.as_str(), v.line)));
        let (other, line) = cands
            .filter(|(c, _)| *c != name && !(name.is_ascii() && c.is_ascii()) && c.chars().count() == count && c.chars().map(latin_lookalike).collect::<String>() == key)
            .min()?;
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
        // The same answer every run, whatever the order: `closest` breaks ties by name. Past what
        // one check may measure, no more are looked for: 4,000 names a letter off 4,000 functions
        // took 11 s, and 3,000 of 67 letters 82 s (FEEDBACK75.md, Q-1120, found in review; branch
        // feedback75). A confined check measures less, and names of at most 64 letters.
        let (most, most_len) = if self.confined { (MOST_SUGGEST_COMPARED, 64) } else { (MOST_SUGGEST_COST, 1024) };
        let mut cost = self.suggested.get();
        if cost > most {
            return String::new();
        }
        let cands = self.scopes.iter().flat_map(|s| s.keys().map(|k| k.as_str())).chain(self.globals.keys().map(|k| k.as_str()));
        let near = closest_within(name, cands, &mut cost, most_len, &mut self.lowered.borrow_mut());
        self.suggested.set(cost);
        match near {
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

    /// Where the code being checked surely runs on the server, said as "in a route": a route's
    /// own code, a `server fn`'s, what a top-level `every` or `schedule` is given, and the top
    /// level itself, which runs as the program starts. None where it may run in the browser too
    /// (FEEDBACK67.md, Q-959; branch pwa).
    fn on_the_server(&self) -> Option<&'static str> {
        if self.browser_too > 0 {
            return None;
        }
        match self.fns.last() {
            Some(f) => f.on_server,
            None => self.sequential.then_some("at the top level"),
        }
    }

    /// A `server fn` that reads a page state through the program's functions (`return mine()`,
    /// `fn mine() { return cart }`): it ran on the page's render, where there is one, and failed
    /// at its next call or live refresh, where there is none (FEEDBACK68.md, found in review;
    /// branch global-state). Each function's reads and calls, followed to the end. A name a
    /// function takes as a parameter is its own.
    fn page_states_through_calls(&mut self, order: &[Item]) {
        let states: HashSet<String> = order.iter().filter_map(|it| if let Stmt::State(n, ..) = &it.stmt { Some(n.clone()) } else { None }).collect();
        if states.is_empty() {
            return;
        }
        let defs: Vec<(&Arc<str>, &Arc<FnDef>)> = order.iter().filter_map(|it| if let Stmt::Fn(d) = &it.stmt { Some((&it.file, d)) } else { None }).collect();
        let graph = CallGraph::of(defs.iter().map(|(_, d)| *d), &|n| states.contains(n));
        // What each reaches: (the page state, through which function), from those that read one
        // themselves up to all who call them, once each: a fixpoint over every function took a
        // chain of 16,000 calls 4.5 s (FEEDBACK75.md, Q-1058, review; branch feedback75).
        let mut through: Vec<Option<usize>> = vec![None; defs.len()];
        graph.reach(&mut (0..defs.len()).filter(|&i| graph.reads[i].is_some()), &mut through);
        for (i, (file, d)) in defs.iter().enumerate() {
            if !d.server {
                continue;
            }
            if let Some(t) = through[i].filter(|&t| t != i) {
                let state = graph.reads[t].as_deref().unwrap_or_default();
                let verb = if graph.sets_only[t] { "sets" } else { "reads" };
                let msg = format!("`{}` is the page's state, and server fn `{}` runs for no page (it {} it through `{}`): pass it as an argument", state, d.name, verb, defs[t].1.name);
                self.diag_at((*file).clone(), d.line, Severity::Error, msg);
            }
        }
    }

    /// A page state a component reads goes, whole, into the HTML of each page that shows the
    /// component: one made from `env`, a file or a `select *` row (a user's password hash, a TOTP
    /// seed, an API key) is said here, as a warning, the user's choice (FEEDBACK75.md, Q-1058;
    /// branch feedback75). Through the program's functions too, on either side, followed to the
    /// end. A name a function takes as a parameter is its own.
    fn page_states_to_pages(&mut self, order: &[Item]) {
        let states: Vec<(Arc<str>, String, Expr, u32)> = order.iter().filter_map(|it| if let Stmt::State(n, e, l) = &it.stmt { Some((it.file.clone(), n.clone(), e.clone(), *l)) } else { None }).collect();
        if states.is_empty() {
            return;
        }
        let names: HashSet<&str> = states.iter().map(|s| s.1.as_str()).collect();
        let defs: Vec<&Arc<FnDef>> = order.iter().filter_map(|it| if let Stmt::Fn(d) = &it.stmt { Some(d) } else { None }).collect();
        let graph = CallGraph::of(defs.iter().copied(), &|n| names.contains(n));
        let own_source: Vec<Option<&'static str>> = defs.iter().map(|d| server_source(&d.body)).collect();
        let mut source_of: Vec<Option<usize>> = vec![None; defs.len()];
        graph.reach(&mut (0..defs.len()).filter(|&i| own_source[i].is_some()), &mut source_of);
        for (file, name, init, line) in &states {
            let from = server_source(init).or_else(|| {
                let mut calls = Vec::new();
                called_names(init, &mut calls);
                calls.iter().find_map(|c| graph.index.get(c.as_str()).and_then(|&i| source_of[i]).and_then(|j| own_source[j]))
            });
            let Some(from) = from else { continue };
            let readers: Vec<usize> = (0..defs.len()).filter(|&i| graph.all_reads[i].iter().any(|r| r == name)).collect();
            if readers.is_empty() {
                continue;
            }
            let mut reads: Vec<Option<usize>> = vec![None; defs.len()];
            graph.reach(&mut readers.into_iter(), &mut reads);
            let reader = defs.iter().enumerate().find(|(i, d)| d.component && reads[*i].is_some()).map(|(_, d)| d);
            if let Some(c) = reader {
                let msg = format!(
                    "`{}` is a page state that <{}> reads, so its whole value is written into each page that shows <{}>, and it comes from {}: give the component what it shows, as a prop or a state of those fields alone",
                    name, c.name, c.name, from
                );
                self.diag_at(file.clone(), *line, Severity::Warning, msg);
            }
        }
    }

    /// A `stream`'s or an `events`' `send` called in the function `db.transaction` or `db.atomic`
    /// runs: on SQLite the transaction keeps the instance's lock, so its client's reading held up
    /// every other request; Quillo keeps up to 4 MiB of what is sent and then ends the transaction
    /// (FEEDBACK75.md, Q-1067; branch feedback75). Written out: `send` itself, by the name the
    /// stream's function gives it, where nothing in between binds that name again.
    fn stream_sends_in_transactions(&mut self, order: &[Item]) {
        use crate::client::{each_free, Bound, Met};
        let mut found: Vec<(Arc<str>, u32, String, &'static str)> = Vec::new();
        for (k, it) in order.iter().enumerate() {
            // A confined check past its budget says nothing more: its clock is read every 64th item,
            // and none is read once it is spent. This pass read on, 23.7 s past a 2 s budget
            // (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1127; branch feedback75).
            if budget_spent(k) {
                return;
            }
            let roots: Vec<&Expr> = match &it.stmt {
                Stmt::Route(r) => vec![&r.body],
                Stmt::Fn(d) => vec![&d.body],
                Stmt::Let(_, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => vec![e],
                _ => continue,
            };
            for root in roots {
                // Only code that starts a stream is read with what it binds: a plain walk finds it,
                // and a function of 100,000 `let`s keeps none of their names (FEEDBACK75.md
                // follow-up found by 0.8.9's coverage, Q-1127; branch feedback75).
                let mut starts = false;
                walk_expr(root, &mut |x| {
                    starts |= matches!(x, Expr::Call(c, args, _) if matches!(&**c, Expr::Ident(n, _) if n == "stream" || n == "events") && matches!(args.first(), Some(Expr::Lambda(_))))
                });
                if !starts {
                    continue;
                }
                each_free(root, true, &mut Bound::new(), &mut |m, bound| {
                    let Met::Expr(Expr::Call(c, args, _)) = m else { return };
                    let streams = matches!(&**c, Expr::Ident(n, _) if (n == "stream" || n == "events") && !bound.has(n.as_str()));
                    let Some(Expr::Lambda(f)) = args.first().filter(|_| streams) else { return };
                    let Some(send) = f.params.first().map(|p| p.name.to_string()) else { return };
                    each_free(&f.body, true, &mut Bound::of(f.params.iter().skip(1).map(|p| p.name.to_string())), &mut |m, bound| {
                        let Met::Expr(Expr::Call(c, targs, _)) = m else { return };
                        let tx = match &**c {
                            Expr::Field(o, name, _) if matches!(&**o, Expr::Ident(db, _) if db == "db" && !bound.has("db")) => match name.as_str() {
                                "transaction" => "db.transaction",
                                "atomic" => "db.atomic",
                                _ => return,
                            },
                            _ => return,
                        };
                        if bound.has(&send) {
                            return;
                        }
                        for t in targs {
                            if let Expr::Lambda(g) = t {
                                each_free(&g.body, true, &mut Bound::of(g.params.iter().map(|p| p.name.to_string())), &mut |m, _| {
                                    if let Met::Expr(Expr::Ident(n, line)) = m {
                                        if *n == send.as_str() {
                                            found.push((it.file.clone(), *line, send.clone(), tx));
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
            }
        }
        for (file, line, send, tx) in found {
            let msg = format!(
                "`{}` inside `{}` waits for the client to read: on SQLite the transaction holds every other request of the instance meanwhile, so Quillo keeps up to 4 MiB of what it sends, then ends the transaction (it rolls back) if the client has not read within a second. Read the rows in the transaction and send them after it",
                send, tx
            );
            self.diag_at(file, line, Severity::Warning, msg);
        }
    }

    /// A page state is the page's: a `server fn` runs for no page, so it is passed what it needs
    /// (FEEDBACK68.md, the user's choice of 2026-11-15; branch global-state).
    fn page_state_here(&mut self, name: &str) {
        if self.fns.last().is_some_and(|f| f.server) {
            let f = self.fns.iter().find(|f| f.server).map_or(String::new(), |f| f.name.clone());
            self.error(format!("`{}` is the page's state, and server fn `{}` runs for no page: pass it as an argument", name, f));
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
                "<{}> runs in the browser, but `{}` is a server variable that changes; pass it as a prop or read it with a `server fn`, or, for one each page has of its own, write `state {} = ...` at the top",
                c, name, name
            ));
        }
    }

    // ---- statements and functions -------------------------------------

    fn fn_body(&mut self, d: &Arc<FnDef>) {
        self.line = d.line;
        let effects = d.effects.as_ref().map(|e| effect_mask(e));
        let tool = d.mcp && self.tool_rules(d);
        // A tool's parameters are said of the tool, whose JSON the browser sends too (FEEDBACK56.md,
        // Q-853; branch feedback56).
        if d.server && !tool {
            for p in &d.params {
                if let Some(t) = &p.ty {
                    if ty_bits(t) & (FN | HTML | RESP) != 0 && ty_bits(t) != ANY {
                        self.error(format!("parameter `{}` of server fn `{}` is {}, which the browser cannot send", p.name, d.name, t.text()));
                    }
                }
            }
        }
        self.fns.push(FnCtx {
            server: d.server,
            name: d.name.clone(),
            ret: d.ret.as_ref().map(ty_t),
            err: d.err.as_ref().map(ty_bits),
            allowed: effects.unwrap_or(ALL_EFFECTS) & crate::policy::ceiling(),
            restrictor: d.effects.as_ref().map(|e| (d.name.clone(), e.clone())),
            component: d.component.then(|| d.name.clone()),
            nested: false,
            state_init: false,
            on_server: (d.server && !d.component).then_some("in a `server fn`"),
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
            self.let_not_state(d);
        }
        if tool {
            let answer = t.union(self.fns.last().map_or(T::NEVER, |f| f.exits));
            self.tool_answer(d, answer);
        }
        self.pop();
        self.fns.pop();
        self.fn_ended();
    }

    /// The rules of a tool, `fn mcp name(...)` or `server fn mcp name(...)` at a file's top level
    /// (FEEDBACK56.md, Q-853, Part A.1; branch feedback56): its name is the tool's, as MCP allows
    /// one; each parameter comes as JSON, and says its type; a `///` comment says what it does,
    /// and a `with` what it may do, which the door tells the AI client. False for a view or a
    /// component, which is no tool at all.
    fn tool_rules(&mut self, d: &Arc<FnDef>) -> bool {
        const NAME: &str = "a tool's name is 1 to 128 of A-Z, a-z, 0-9, `_`, `-` and `.`, as MCP allows";
        self.line = d.line;
        if d.component || is_view(d) {
            let what = if d.component { "component" } else { "view" };
            self.error(format!("a {} is not a tool: `fn mcp` is for a function that answers with data, not a page", what));
            return false;
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
        if let Some(c) = d.name.chars().find(|c| !allowed(*c)) {
            self.error(format!("{}: `{}` has `{}`", NAME, d.name, c));
        } else if d.name.chars().count() > 128 {
            self.error(format!("{}: this one has {} characters", NAME, d.name.chars().count()));
        }
        for p in &d.params {
            match &p.ty {
                Some(t) if ty_bits(t) & (FN | HTML | RESP) != 0 && ty_bits(t) != ANY => {
                    self.error(format!("parameter `{}` of tool `{}` is {}, which cannot come as JSON", p.name, d.name, t.text()))
                }
                Some(_) => {}
                None => self.warn(format!("parameter `{}` of tool `{}` has no type: its schema is `{{}}`, and the model guesses what to send", p.name, d.name)),
            }
        }
        if d.doc.is_none() {
            self.warn(format!("tool `{}` has no `///` comment: an AI client sees only its name and its parameters; say what it does on a `///` line above it", d.name));
        }
        // What it is seen to do (`fn_effects`), to write: a tool seen to do nothing is fine as
        // written, as `with` takes one effect at least, and no effect it does not use is
        // suggested, which would tell the client it may (FEEDBACK56.md, Q-853, found in review;
        // branch feedback56).
        if d.effects.is_none() {
            let seen = effect_names(self.fn_effects(&Sig::of_def(d)));
            if !seen.is_empty() {
                self.warn(format!(
                    "tool `{}` has no `with`, so an AI client is told it may change anything and reach anywhere; it uses {}: `with {}`",
                    d.name,
                    seen.join(" and "),
                    seen.join(", ")
                ));
            }
        }
        // A `reply` of a 2xx status in its code reaches the AI client as an error, as any `reply`
        // there does (the pre-release defaults, 2026-11-09: kept so, and said; FEEDBACK56.md;
        // branch feedback56).
        let mut fine = Vec::new();
        crate::client::each_expr(&d.body, &mut |x| {
            if let Expr::Reply(st, _, line) = x {
                if let Expr::Lit(Lit::Int(n)) = &**st {
                    if (200..300).contains(n) {
                        fine.push((*n, *line));
                    }
                }
            }
        });
        for (n, line) in fine {
            self.line = line;
            self.warn(format!("`reply {}` in tool `{}` reaches the AI client as an error, as any `reply` in a tool's call does: a tool answers with the value it gives back, so return it", n, d.name));
        }
        self.line = d.line;
        true
    }

    /// A tool's answer, where the checker sees it: html, or a response as `redirect(...)` makes,
    /// is a page, which the door gives the AI client as an error (FEEDBACK56.md, Q-853, Part
    /// A.1; branch feedback56). Its declared result, else what its body gives.
    fn tool_answer(&mut self, d: &FnDef, body: T) {
        const PAGE: u16 = HTML | RESP;
        let t = d.ret.as_ref().map(ty_t).filter(|r| !r.vague()).unwrap_or(body);
        let vals = t.bits & !(ERR | NIL);
        if t.vague() || vals & PAGE == 0 {
            return;
        }
        let what = if vals & HTML != 0 { "html" } else { "a response, as `redirect(...)` makes" };
        self.line = d.line;
        if vals & !PAGE == 0 {
            self.error(format!("a tool answers with data, not a page: `{}` answers with {}", d.name, what));
        } else {
            self.warn(format!("a tool answers with data, not a page: `{}` may answer with {}", d.name, what));
        }
    }

    /// A component's plain `let` that a handler changes and its markup shows: the page shows what
    /// it held as the component started and never the change, as only a `state` is followed, and
    /// nothing said so (FEEDBACK80.md, Q-1268; branch feedback72-81). Said where it is declared. A
    /// handler is a function given to an `@event` (`onclick`), written there or the component's own
    /// by name, `onclick={inc}`, with its own functions it calls, and a timer's or a channel's
    /// callback (`each_later`); what the markup shows is read outside them. One changed and not
    /// shown, or shown and only set as the component starts, is let be.
    #[inline(never)]
    fn let_not_state(&mut self, d: &Arc<FnDef>) {
        let Expr::Block(stmts) = &d.body else { return };
        let lets: Vec<(&str, &Expr, u32)> = stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Let(Pattern::Bind(n), e, line) if !matches!(e, Expr::Lambda(_)) => Some((n.as_str(), e, *line)),
                _ => None,
            })
            .collect();
        // One the component's code does not assign is no question: most are not.
        let Some(assigned) = self.component_assigned.get(&def_key(d)) else { return };
        let wanted: HashSet<&str> = lets.iter().map(|(n, _, _)| *n).filter(|n| assigned.contains(*n)).collect();
        if self.quiet > 0 || wanted.is_empty() {
            return;
        }
        let local: HashMap<&str, &FnDef> = stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Fn(f) => Some((f.name.as_str(), &**f)),
                Stmt::Let(Pattern::Bind(n), Expr::Lambda(f), _) => Some((n.as_str(), &**f)),
                _ => None,
            })
            .collect();
        // Which of the `wanted` names a function's code assigns and does not declare itself (its
        // parameters, its `let`s), and the component's functions it names, which it may call.
        fn assigns(f: &FnDef, wanted: &HashSet<&str>, local: &HashMap<&str, &FnDef>, changed: &mut HashSet<String>, named: &mut Vec<String>) {
            let mut own: Vec<String> = Vec::new();
            let mut set: Vec<String> = Vec::new();
            let mine = |n: &str, own: &mut Vec<String>| {
                if wanted.contains(n) {
                    own.push(n.to_string());
                }
            };
            f.params.iter().for_each(|p| mine(&p.name, &mut own));
            walk_fn(f, &mut |x| match x {
                Expr::Block(stmts) => {
                    for s in stmts {
                        match s {
                            Stmt::Assign(n, ..) if wanted.contains(n.as_str()) => set.push(n.to_string()),
                            Stmt::Let(Pattern::Bind(n), ..) => mine(n, &mut own),
                            Stmt::Let(p, ..) => {
                                let mut names = Vec::new();
                                pattern_names(p, &mut names);
                                names.iter().for_each(|n| mine(n, &mut own));
                            }
                            _ => {}
                        }
                    }
                }
                Expr::Lambda(l) => l.params.iter().for_each(|p| mine(&p.name, &mut own)),
                Expr::Ident(n, _) if local.contains_key(n.as_str()) => named.push(n.to_string()),
                _ => {}
            });
            changed.extend(set.into_iter().filter(|n| !own.contains(n)));
        }
        let code: Vec<&Stmt> = stmts.iter().filter(|s| !matches!(s, Stmt::Fn(_) | Stmt::Let(_, Expr::Lambda(_), _))).collect();
        // What the handlers change, first: most components' handlers change no `let` of theirs,
        // and their markup is not read again.
        let mut changed: HashSet<String> = HashSet::new();
        let mut named: Vec<String> = Vec::new();
        for st in &code {
            walk_stmts(std::slice::from_ref(*st), &mut |x| {
                let Expr::Html(el, _) = x else { return };
                walk_html_els(el, &mut |h| {
                    // A handler, or a function given to a view or component as one, `<Child
                    // onpick={pick}/>` (found in review).
                    let child = h.tag.starts_with(|c: char| c.is_uppercase());
                    for (_, v) in h.attrs.iter().filter(|(a, _)| a.starts_with('@') || (child && a.starts_with("on"))) {
                        match v {
                            AttrVal::Expr(Expr::Lambda(f)) => assigns(f, &wanted, &local, &mut changed, &mut named),
                            AttrVal::Expr(Expr::Ident(n, _)) => named.push(n.to_string()),
                            _ => {}
                        }
                    }
                });
            });
            if let Stmt::Expr(e) | Stmt::Let(_, e, _) = st {
                each_later(e, &mut |l| {
                    if let Expr::Lambda(f) = l {
                        assigns(f, &wanted, &local, &mut changed, &mut named);
                    }
                });
            }
        }
        // The component's own functions the handlers name, and those they name, each once.
        let mut done: HashSet<String> = HashSet::new();
        while let Some(n) = named.pop() {
            if let Some(f) = local.get(n.as_str()).filter(|_| done.insert(n.clone())) {
                assigns(f, &wanted, &local, &mut changed, &mut named);
            }
        }
        if !lets.iter().any(|(n, _, _)| changed.contains(*n)) {
            return;
        }
        // What runs later, by where it is: its reads are no showing.
        let mut inside: HashSet<*const Expr> = HashSet::new();
        for st in &code {
            walk_stmts(std::slice::from_ref(*st), &mut |x| {
                if let Expr::Lambda(f) = x {
                    walk_fn(f, &mut |y| {
                        inside.insert(y as *const Expr);
                    });
                }
            });
        }
        let mut shown: HashSet<String> = HashSet::new();
        // Markup inside markup's code (`<li>` in `{for i in tabs => <li>{i}</li>}`) is read with
        // it, where its names are bound, not again alone.
        let mut nested: HashSet<*const Expr> = HashSet::new();
        for st in &code {
            walk_stmts(std::slice::from_ref(*st), &mut |x| {
                let Expr::Html(el, _) = x else { return };
                if inside.contains(&(x as *const Expr)) || nested.contains(&(x as *const Expr)) {
                    return;
                }
                walk_html(el, &mut |y| {
                    if let Expr::Html(..) = y {
                        nested.insert(y as *const Expr);
                    }
                });
                walk_html_els(el, &mut |h| {
                    // A name the shown code binds itself, `{for i in tabs => <li>{i}</li>}`, is
                    // not the component's `let` (found in review).
                    let mut read = |e: &Expr| {
                        let mut own = HashSet::new();
                        bound_in(e, &mut own);
                        walk_expr(e, &mut |y| {
                            if let (Expr::Ident(n, _), false) = (y, inside.contains(&(y as *const Expr))) {
                                if !own.contains(n.as_str()) {
                                    shown.insert(n.to_string());
                                }
                            }
                        })
                    };
                    let child = h.tag.starts_with(|c: char| c.is_uppercase());
                    for (a, v) in &h.attrs {
                        let handler = a.starts_with('@') || (child && a.starts_with("on") && matches!(v, AttrVal::Expr(Expr::Lambda(_) | Expr::Ident(..))));
                        if let (AttrVal::Expr(e) | AttrVal::Spread(e), false) = (v, handler) {
                            read(e);
                        }
                    }
                    for c in &h.children {
                        if let HtmlChild::Expr(e) = c {
                            read(e);
                        }
                    }
                });
            });
        }
        for (n, e, line) in lets {
            if changed.contains(n) && shown.contains(n) {
                self.line = line;
                self.warn(format!("`{}` is changed in a handler but isn't a state, so the page won't show it: write `state {} = {}`", n, n, written(e)));
            }
        }
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
            server: false,
            name: format!("{} {}", r.method, route_path(r)),
            ret: None,
            err: None,
            // `route ... with db` (branch feedback40-43).
            allowed: r.effects.as_ref().map_or(ALL_EFFECTS, |e| effect_mask(e)) & crate::policy::ceiling(),
            restrictor: r.effects.as_ref().map(|e| (format!("route {}", r.shown()), e.clone())),
            component: None,
            nested: false,
            state_init: false,
            on_server: Some("in a route"),
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
        let mut set_fields = Vec::new();
        if let Some(q) = &r.query {
            literal_fields(q, "query", &mut set_fields);
        }
        if let Some(b) = &r.body_pat {
            literal_fields(b, "", &mut set_fields);
        }
        self.set_fields = set_fields;
        self.expr(&r.body);
        self.set_fields.clear();
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
                    let later = runs_later(e);
                    walk_expr(e, &mut |x| {
                        from_page |= matches!(x, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "js" || n == "js_new")) && !later.contains(&(x as *const Expr));
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
                    // Set from a call that can fail (FEEDBACK81.md, Q-1272; branch feedback72-81).
                    if kind == Kind::Let && t.bits & ERR != 0 && !t.vague() && self.shows_failure(e) {
                        self.failing_vars.insert((self.file.clone(), self.line, n.clone()));
                    }
                    self.define(&n, t, sig, kind);
                }
                self.next_sql = SqlVar::Other;
                self.next_url = false;
                T::of(NIL)
            }
            // `el.value = x`: a field of one of the page's objects, which only
            // a component has; a map's fields are made anew, not set.
            Stmt::SetField(o, name, op, e, line) => {
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
                } else if !t.vague() && t.bits & MAP != 0 {
                    // `m.k = 5`: a map is a value, made anew; what to write, with what was written
                    // (FEEDBACK64.md, Q-934; branch feedback64).
                    self.error(map_field_advice(o, name, *op, e));
                } else if self.in_component().is_none() && !helper {
                    self.error(format!("cannot set `{}`: only the page's objects (an element, an event) have fields that are set, in a component", name));
                } else if !t.vague() {
                    self.error(format!("{} has no field `{}` to set: only the page's objects (an element, an event) do", t_text(t), name));
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
                        if matches!(k, SqlVar::Built | SqlVar::Parts(_)) {
                            self.sql_built_vars.insert(name.to_string());
                        }
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
                        if v.kind == Kind::PageState {
                            self.page_state_here(name);
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
                    } else if d.mcp {
                        // (FEEDBACK56.md, Q-853; branch feedback56)
                        self.error("`fn mcp` must be declared at the top level, as `server fn` is: a tool is an address of the program's");
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
            Stmt::Service(name, _, _, opts, line, _) => {
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
                // Set from a call that can fail (FEEDBACK81.md, Q-1272; branch feedback72-81).
                if t.bits & ERR != 0 && !assigned && self.shows_failure(e) {
                    self.failing_vars.insert((self.file.clone(), self.line, name.clone()));
                }
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
        // Where it runs: as the code around it does, but a handler or a prop written in markup,
        // which may run in the browser; at the top level, only what `every` or `schedule` is
        // given, as a `let` of one may be called from a component (FEEDBACK67.md, Q-959; branch
        // pwa).
        let job = if self.fns.is_empty() { self.job_arg.take() } else { None };
        let on_server = match self.fns.last() {
            _ if self.browser_too > 0 => None,
            _ if d.server => Some("in a `server fn`"),
            Some(f) => f.on_server,
            None => job,
        };
        self.fns.push(FnCtx {
            server: d.server || self.fns.last().is_some_and(|f| f.server),
            name: d.name.clone(),
            ret: d.ret.as_ref().map(ty_t),
            err: d.err.as_ref().map(ty_bits),
            allowed,
            restrictor,
            component,
            nested: true,
            state_init: false,
            on_server,
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
            server: d.server || self.fns.last().is_some_and(|f| f.server),
            name: d.name.clone(),
            ret: None,
            err: None,
            allowed: crate::policy::ceiling(),
            restrictor: None,
            component: None,
            nested: false,
            state_init: false,
            on_server: None,
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

    /// What the server runs while it makes a page needs the effects of the route's `with`, in the
    /// components the page shows too: their `state`s' values and markup (`state v = load_it()`,
    /// `{stamp()}`, `{now()}`), but not their handlers, which run in the browser. A 500 was all
    /// that said so (FEEDBACK80.md, Q-1261; branch feedback72-81). Read for every route `with`
    /// effects once all bodies are checked, and said once for each function and effect, at the
    /// first route, with how many others: one header's server fn in 150 pages was 150 warnings.
    /// An error when what needs the effect declares it (`with time`, a builtin) and runs every
    /// time; a warning when its effects are worked out from what it calls, or it runs on a
    /// condition (a component under `if`, a call in a branch).
    fn render_effects(&mut self, order: &[Item]) {
        let routes: Vec<(Arc<str>, Arc<RouteDef>)> = order
            .iter()
            .filter_map(|it| match &it.stmt {
                Stmt::Route(r) if r.method != crate::ast::GUARD_METHOD && r.effects.is_some() => Some((it.file.clone(), r.clone())),
                _ => None,
            })
            .collect();
        // (what needs it, the effects it lacks, an error) -> what to say, the components that call
        // it and the routes: once for each, naming one of each (found in review: 400 cards calling
        // `now()` were said of one card).
        struct Said {
            msg: (String, String, String, String),
            components: Vec<String>,
            routes: Vec<(Arc<str>, u32, String, String)>,
        }
        let mut said: Vec<((Arc<str>, u8, bool, u8), Said)> = Vec::new();
        for (file, r) in routes {
            let decl = r.effects.clone().unwrap_or_default();
            let allowed = effect_mask(&decl);
            if !self.render_effects_possible(allowed) {
                continue;
            }
            let mut found: Found = HashMap::new();
            // (code, runs every time, the component or view it is in, caught): what is reached,
            // each once for each way it is reached.
            let bound = route_bound(&r);
            let mut root = Vec::new();
            render_refs(&r.body, false, 0, None, &bound, &mut root, 0);
            // In the order the page writes them, so the first said of is the first shown.
            let mut todo: std::collections::VecDeque<(Arc<[RenderRef]>, bool, Option<Arc<FnDef>>, u8)> = std::collections::VecDeque::from([(root.into(), true, None, 0)]);
            let mut seen: HashSet<(usize, bool, u8)> = HashSet::new();
            while let Some((refs, sure, component, caught)) = todo.pop_front() {
                for rf in refs.iter() {
                    let s = sure && !rf.cond;
                    let k = caught.max(rf.caught);
                    let Some(sig) = self.globals.get(&*rf.name).and_then(|v| v.sig.clone()) else {
                        // A module's function or a service's, called in a component or a view.
                        if let (Some(c), Some((m, _))) = (&component, rf.name.split_once('.')) {
                            let needs = if MODULES.contains(&m) {
                                builtin_effect(&rf.name)
                            } else if self.services.contains_key(m) || self.sites.contains(m) {
                                crate::ast::effect_bit("http")
                            } else {
                                0
                            };
                            self.render_need(&mut found, &file, rf, c, needs & !allowed, true, sure, k, Vec::new());
                        }
                        continue;
                    };
                    let def = sig.def.clone().filter(|_| !sig.builtin);
                    if sig.component || (rf.tag && def.is_some()) {
                        // A component or a view shown: its own code is read and said of as its,
                        // a view the route's page shows too (found in review: `<Footer/>` with
                        // `{str(now())}` was a 500 check said nothing of).
                        let Some(d) = def else { continue };
                        if seen.insert((def_key(&d), s, k)) {
                            let refs = self.render_refs_of(&d);
                            todo.push_back((refs, s, Some(d.clone()), k));
                        }
                        continue;
                    }
                    let Some(c) = &component else {
                        // The route's own calls are checked where it is; a function it calls is
                        // read for the components and views it shows.
                        if let Some(d) = def.filter(|d| !d.server) {
                            if seen.insert((def_key(&d), s, k)) {
                                let refs = self.render_refs_of(&d);
                                todo.push_back((refs, s, None, k));
                            }
                        }
                        continue;
                    };
                    let declared = sig.builtin || sig.effects.is_some();
                    let needs = match sig.effects {
                        Some(e) => e,
                        None if sig.builtin => 0,
                        None => self.fn_effects(&sig),
                    };
                    let missing = needs & !allowed;
                    let mut through: Vec<String> = Vec::new();
                    if let (Some(d), false) = (&def, declared || missing == 0) {
                        for bit in (0..8).map(|i| 1u8 << i).filter(|b| missing & b != 0) {
                            if let Some(n) = self.effect_source(d, bit).filter(|n| !through.contains(n)) {
                                through.push(n);
                            }
                        }
                    }
                    self.render_need(&mut found, &file, rf, c, missing, declared, sure, k, through);
                    // A plain function called there is read for the components it shows.
                    if let Some(d) = def.filter(|d| !d.server) {
                        if seen.insert((def_key(&d), s, k)) {
                            let refs: Vec<RenderRef> = self.render_refs_of(&d).iter().filter(|x| x.tag).cloned().collect();
                            todo.push_back((refs.into(), s, component.clone(), k));
                        }
                    }
                }
            }
            let mut found: Vec<((Arc<str>, u8, u8), (bool, (String, String, String, String), Vec<String>))> = found.into_iter().collect();
            found.sort_by(|a, b| (&a.0 .0, a.0 .1, a.0 .2).cmp(&(&b.0 .0, b.0 .1, b.0 .2)));
            for ((n, missing, kind), (error, msg, components)) in found {
                let route = (file.clone(), r.line, r.shown(), decl.join(", "));
                match said.iter_mut().find(|(k, _)| *k == (n.clone(), missing, error, kind)) {
                    Some((_, s)) => {
                        s.routes.push(route);
                        for c in components {
                            if !s.components.contains(&c) {
                                s.components.push(c);
                            }
                        }
                    }
                    None => said.push(((n, missing, error, kind), Said { msg, components, routes: vec![route] })),
                }
            }
        }
        for ((_, _, error, _), s) in said {
            let (what, calls, site, tail) = s.msg;
            let (file, line, route, decl) = s.routes[0].clone();
            let others = match s.routes.len() - 1 {
                0 => String::new(),
                1 => ", as is 1 other route whose page runs it".to_string(),
                k => format!(", as are {} other routes whose pages run it", k),
            };
            let also = match s.components.len() - 1 {
                0 => String::new(),
                1 => ", as does 1 other component".to_string(),
                k => format!(", as do {} other components", k),
            };
            let msg = format!(
                "{}{}, but `route {}` is declared `with {}`{}: <{}> calls it {}{}, {}",
                what, calls, route, decl, others, s.components[0], site, also, tail
            );
            self.diag_at(file, line, if error { Severity::Error } else { Severity::Warning }, msg);
        }
    }

    /// One call of `render_effects` that lacks `missing`: kept, with what to say, the error
    /// winning over the warning for one route, and every component or view that calls it.
    #[allow(clippy::too_many_arguments)]
    fn render_need(&mut self, found: &mut Found, route_file: &Arc<str>, rf: &RenderRef, c: &Arc<FnDef>, missing: u8, declared: bool, sure: bool, caught: u8, through: Vec<String>) {
        if missing == 0 {
            return;
        }
        let error = declared && sure && !rf.cond && caught == 0;
        let key = (rf.name.clone(), missing, caught);
        if let Some((e, _, components)) = found.get_mut(&key) {
            if !components.contains(&c.name) {
                components.push(c.name.clone());
            }
            if *e || !error {
                return;
            }
        }
        let names = effect_names(missing);
        let what = format!("`{}` needs the {} effect{}", rf.name, names.join(", "), if names.len() == 1 { "" } else { "s" });
        let calls = if through.is_empty() {
            String::new()
        } else {
            format!(" (it calls {})", through.iter().map(|t| format!("`{}`", t)).collect::<Vec<_>>().join(", "))
        };
        let file = self.globals.get(&c.name).map(|v| v.file.clone());
        let at = match file {
            Some(f) if f != *route_file => self.shown_at(&f, rf.line),
            _ => format!("line {}", rf.line),
        };
        let site = match &rf.state {
            Some(s) => format!("in `state {}` ({})", s, at),
            None => format!("as it renders ({})", at),
        };
        let tail = match caught {
            CAUGHT_FALLBACK => "which the server runs while it makes the page, where `??` gives its fallback every time",
            CAUGHT_DEFERRED => "which the server runs after the page is sent (`defer`), so the page keeps its placeholder and the error is logged",
            _ => "which the server runs while it makes the page",
        };
        let mut components = found.remove(&key).map(|(_, _, cs)| cs).unwrap_or_default();
        components.retain(|n| *n != c.name);
        components.insert(0, c.name.clone());
        found.insert(key, (error, (what, calls, site, tail.to_string()), components));
    }

    /// `render_refs` of a function's body, read once.
    fn render_refs_of(&mut self, d: &Arc<FnDef>) -> Arc<[RenderRef]> {
        if let Some(refs) = self.render_refs_cache.get(&def_key(d)) {
            return refs.clone();
        }
        let mut out = Vec::new();
        render_refs(&d.body, false, 0, None, &fn_bound(d), &mut out, 0);
        let refs: Arc<[RenderRef]> = out.into();
        self.render_refs_cache.insert(def_key(d), refs.clone());
        refs
    }

    /// Whether any component of the program runs, as it renders, something needing an effect
    /// `allowed` lacks: worked out once for each `with`, so that, as in most programs, when none
    /// does, no route's page is read for it (Q-1261).
    fn render_effects_possible(&mut self, allowed: u8) -> bool {
        if let Some(b) = self.render_possible.get(&allowed) {
            return *b;
        }
        // Views too, which a route's page may show (Q-1261, found in review).
        let components: Vec<Arc<FnDef>> = self
            .globals
            .values()
            .filter_map(|v| v.sig.as_ref().filter(|s| !s.builtin && (s.component || s.def.as_ref().is_some_and(|d| is_view(d)))).and_then(|s| s.def.clone()))
            .collect();
        let mut possible = false;
        'all: for c in components {
            for rf in self.render_refs_of(&c).iter().filter(|r| !r.tag) {
                let needs = match self.globals.get(&*rf.name).and_then(|v| v.sig.clone()) {
                    Some(sig) => match sig.effects {
                        Some(e) => e,
                        None if sig.builtin || sig.component => 0,
                        None => self.fn_effects(&sig),
                    },
                    None => match rf.name.split_once('.') {
                        Some((m, _)) if MODULES.contains(&m) => builtin_effect(&rf.name),
                        Some((m, _)) if self.services.contains_key(m) || self.sites.contains(m) => crate::ast::effect_bit("http"),
                        _ => 0,
                    },
                };
                if needs & !allowed != 0 {
                    possible = true;
                    break 'all;
                }
            }
        }
        self.render_possible.insert(allowed, possible);
        possible
    }

    /// The function or builtin that needs one of the `missing` effects, among those `d` calls and
    /// those they call, nearest first: what the run names when the call is made (Q-1261). Read as
    /// `fn_effects` reads them, with a list of what is left to read and not by recursion, so a long
    /// chain of calls takes no stack.
    fn effect_source(&mut self, d: &Arc<FnDef>, missing: u8) -> Option<String> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut todo: std::collections::VecDeque<Arc<FnDef>> = std::collections::VecDeque::from([d.clone()]);
        seen.insert(def_key(d));
        while let Some(d) = todo.pop_front() {
            let mut names = Vec::new();
            called_names(&d.body, &mut names);
            for n in names {
                if let Some(s) = self.globals.get(&n).and_then(|g| g.sig.clone()) {
                    match (&s.def, s.effects) {
                        (_, Some(e)) if e & missing != 0 => return Some(n),
                        (Some(def), None) if !s.builtin => {
                            if seen.insert(def_key(def)) && self.fn_effects(&s) & missing != 0 {
                                todo.push_back(def.clone());
                            }
                        }
                        _ => {}
                    }
                } else if MODULES.contains(&n.as_str()) && builtin_effect(&n) & missing != 0 {
                    return Some(n);
                } else if let Some((m, _)) = n.split_once('.') {
                    let bit = if MODULES.contains(&m) {
                        builtin_effect(&n)
                    } else if (self.services.contains_key(m) || self.sites.contains(m)) && !d.params.iter().any(|p| p.name == m) {
                        crate::ast::effect_bit("http")
                    } else {
                        0
                    };
                    if bit & missing != 0 {
                        return Some(n);
                    }
                }
            }
        }
        None
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

    /// `idempotent(f)`, `f` run in its transaction (no `transaction: false`): kept for the end of
    /// the check, when every function `f` may call is known (`idempotent_reaches_outside`).
    fn idempotent_site(&mut self, args: &[Expr]) {
        // A body walked quietly, for a callee's type, is checked where it is written: kept from
        // there it was said twice, the second time at another file's line (found in review).
        if self.quiet > 0 {
            return;
        }
        let f = match args {
            [f] => f,
            [Expr::Map(opts), f, ..] if !opts.iter().any(|(k, v)| k == "transaction" && !matches!(v, Expr::Lit(Lit::Bool(true)))) => f,
            _ => return,
        };
        let def = match f {
            Expr::Lambda(d) => d.clone(),
            Expr::Ident(n, _) => match self.globals.get(n.as_str()).and_then(|g| g.sig.as_ref()).and_then(|s| s.def.clone()) {
                Some(d) => d,
                None => return,
            },
            _ => return,
        };
        self.idempotent_at.push((self.file.clone(), self.line, def));
    }

    /// An `idempotent` whose function changes data in another program (a service's function that
    /// writes or may, a site's): that change is outside its transaction, so when the call's answer
    /// is lost the claim is given up, and a retry with the key runs it there again, under a new
    /// call id. Warned of, with the way out the guide gives: lost answers are what a program of
    /// services lives with, so the program says where they matter (the user's decision,
    /// 2026-11-09; branch feedback56).
    fn idempotent_reaches_outside(&mut self, top_lets: &HashSet<String>) {
        for (file, line, def) in std::mem::take(&mut self.idempotent_at) {
            let r = self.reach_all(&def, top_lets);
            let Some(first) = r.elsewhere.first() else { continue };
            let more = match r.elsewhere.len() - 1 {
                0 => String::new(),
                1 => " (and 1 more such call)".to_string(),
                n => format!(" (and {} more such calls)", n),
            };
            self.diag_at(
                file,
                line,
                Severity::Warning,
                format!(
                    "`idempotent` reaches `{}`, which may change data in another program, outside its transaction{}: when that call's answer is lost, the key is given up and a retry with it runs the call there again, under a new call id. Give `idempotent` `{{ transaction: false }}`, which keeps every answer, failures too, or send an id the other program's change is keyed by, so that a second run changes nothing",
                    first, more
                ),
            );
        }
    }

    // ---- a GET that writes, with the visitor's session (FEEDBACK70.md, Q-965; branch security) --
    //
    // Quillo refuses a POST, a PUT, a PATCH or a DELETE that another site's page sends, but not a
    // GET: a link on another site, in an email or in a chat sends the visitor to it with their
    // `SameSite=Lax` cookies, and its write is done as them. Said of a GET route (and an ANY one,
    // which answers a GET) whose code surely writes (what the prefetch guard refuses at run time)
    // and surely reads who the visitor is, where the text makes both certain: SQL built as the
    // program runs is not guessed at. `route writes GET /x` says the write is meant. Check time
    // only: nothing of it runs at run time.

    /// Each GET route that writes and reads who the visitor is, said; three or more whose one write
    /// is the same (a helper's, a group's middleware's) said once, at it; and `offline` with
    /// `writes`.
    fn get_writes_said(&mut self, order: &[Item], top_lets: &HashSet<String>) {
        let (fns, files, conns) = get_program(order);
        let cx = GetCx { fns: &fns, files: &files, top_lets, conns: &conns };
        let mut seen = GetSeen::default();
        let mut said: Vec<(Arc<str>, u32, String)> = Vec::new();
        let mut related: Vec<(Arc<str>, u32, Vec<(Arc<str>, u32, String)>)> = Vec::new();
        // The routes whose writes are the same, by those writes (in file and line order).
        let mut by_write: Vec<(Vec<GetWrite>, Vec<(Arc<str>, u32, String, String)>)> = Vec::new();
        for it in order {
            let Stmt::Route(r) = &it.stmt else { continue };
            if r.method != "GET" && r.method != "ANY" {
                continue;
            }
            // Its worker keeps an offline page ahead with a prefetch, which writes nothing.
            if r.offline && r.writes {
                said.push((it.file.clone(), r.line, format!(
                    "`route offline writes {}`: Quillo's service worker keeps an offline page ahead with a prefetch, which writes nothing, so this one is refused then, and a copy kept from a visit is shown offline without the write; drop `offline`, or `writes` and the write (a `POST` the page sends)",
                    r.shown()
                )));
            }
            if r.writes {
                continue;
            }
            let root = self.get_own(&[&r.body], &[], &cx);
            let pattern_read = r.body_pat.iter().chain(r.guards.iter()).find_map(pattern_takes);
            // Most routes reach no write, or no read: said by what each function can reach, worked
            // out once a check, before any walk of the route's own.
            let can_write = !root.writes.is_empty() || root.calls.iter().any(|c| self.reaches(c, &cx, &mut seen).0);
            let can_read = root.reads.is_some() || pattern_read.is_some() || root.calls.iter().any(|c| self.reaches(c, &cx, &mut seen).1);
            if !(can_write && can_read) {
                continue;
            }
            let mut facts = self.get_facts(&root, &it.file, &cx, &mut seen, true);
            if facts.reads.is_none() {
                facts.reads = pattern_read.map(|f| (format!("its request pattern takes the request's {}", f), None));
            }
            let Some(v) = facts.reads else { continue };
            if facts.writes.is_empty() {
                continue;
            }
            let read = match v {
                (r, Some(f)) => format!("{}, in `{}`", r, f),
                (r, None) => r,
            };
            let route = (it.file.clone(), r.line, r.shown(), read);
            // Grouped only with routes whose writes are the same, all of them: a route that does
            // another write is said by itself, with each.
            let mut key = facts.writes.clone();
            key.sort_by(|a, b| (&a.file, a.line, &a.what).cmp(&(&b.file, b.line, &b.what)));
            match by_write.iter_mut().find(|(k, _)| *k == key) {
                Some((_, routes)) => routes.push(route),
                None => by_write.push((key, vec![route])),
            }
        }
        // The writes as a message lists them: three, and how many more.
        let listed = |ws: &[GetWrite]| {
            let mut out: Vec<String> = ws.iter().take(3).map(GetWrite::shown).collect();
            if ws.len() > 3 {
                out.push(format!("and {} more", ws.len() - 3));
            }
            out.join("; ")
        };
        for (ws, routes) in by_write {
            if routes.len() < 3 {
                for route in &routes {
                    said.push(get_one(route, &listed(&ws)));
                }
                continue;
            }
            let w = &ws[0];
            let at = |(f, l, shown, _): &(Arc<str>, u32, String, String)| if *f == w.file { format!("`{}` (line {})", shown, l) } else { format!("`{}` ({})", shown, self.shown_at(f, *l)) };
            let mut list: Vec<String> = routes.iter().take(3).map(at).collect();
            if routes.len() > 3 {
                list.push(format!("{} more", routes.len() - 3));
            }
            let last = list.pop().unwrap_or_default();
            let (these, are) = if ws.len() == 1 { ("this write", "is") } else { ("these writes", "are") };
            said.push((w.file.clone(), w.line, format!(
                "{} ({}) {} done by {} GET routes that read who the visitor is: {} and {}. A link on another site, in an email or in a chat runs them with the visitor's session (`SameSite=Lax` cookies go with a followed link). Make them `POST`s, or, if the write is meant (a presence ping, a page's count of views), tag them: `route writes GET ...`, `routes writes /...`",
                these, listed(&ws), are, routes.len(), list.join(", "), last
            )));
            related.push((w.file.clone(), w.line, routes.iter().map(|(f, l, shown, _)| (f.clone(), *l, format!("`{}`", shown))).collect()));
        }
        said.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        for (file, line, msg) in said {
            self.diag_at(file, line, Severity::Warning, msg);
        }
        // Where each route of a grouped warning is, for an editor (`Analysis::related`).
        if self.quiet == 0 {
            self.out.related.extend(related.into_iter().map(|(f, l, rs)| (f.to_string(), l, rs.into_iter().map(|(f, l, s)| (f.to_string(), l, s)).collect())));
        }
    }

    /// One function's own code, read once a check (`GetSeen::own`).
    fn own_of(&self, name: &str, cx: &GetCx, seen: &mut GetSeen) -> Option<Arc<GetOwn>> {
        if let Some(o) = seen.own.get(name) {
            return Some(o.clone());
        }
        let d = cx.fns.get(name)?;
        let params: Vec<String> = d.params.iter().map(|p| p.name.to_string()).collect();
        let o = Arc::new(self.get_own(&fn_code(d), &params, cx));
        seen.own.insert(name.to_string(), o.clone());
        Some(o)
    }

    /// Whether the program's function `name` can reach a write, and a read of who the visitor is,
    /// through the functions it calls: worked out once a check for each strongly connected group of
    /// functions (Tarjan's, with a stack of its own: a chain of calls however long takes no stack),
    /// so the answer is the same in whatever order the routes and functions are written.
    fn reaches(&self, name: &str, cx: &GetCx, seen: &mut GetSeen) -> (bool, bool) {
        if let Some(r) = seen.reach.get(name) {
            return *r;
        }
        if !cx.fns.contains_key(name) {
            return (false, false);
        }
        let (mut index, mut low): (HashMap<String, usize>, HashMap<String, usize>) = (HashMap::new(), HashMap::new());
        let (mut stack, mut on): (Vec<String>, HashSet<String>) = (Vec::new(), HashSet::new());
        let mut work: Vec<(String, usize)> = Vec::new();
        let mut next = 0;
        let mut visit = |n: &str, index: &mut HashMap<String, usize>, low: &mut HashMap<String, usize>, stack: &mut Vec<String>, on: &mut HashSet<String>, work: &mut Vec<(String, usize)>| {
            index.insert(n.to_string(), next);
            low.insert(n.to_string(), next);
            next += 1;
            stack.push(n.to_string());
            on.insert(n.to_string());
            work.push((n.to_string(), 0));
        };
        visit(name, &mut index, &mut low, &mut stack, &mut on, &mut work);
        while let Some((v, at)) = work.last().cloned() {
            let calls = self.own_of(&v, cx, seen).map(|o| o.calls.clone()).unwrap_or_default();
            if let Some(w) = calls.get(at) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                if !cx.fns.contains_key(w.as_str()) || seen.reach.contains_key(w.as_str()) {
                    continue;
                }
                if !index.contains_key(w.as_str()) {
                    visit(w, &mut index, &mut low, &mut stack, &mut on, &mut work);
                } else if on.contains(w.as_str()) {
                    let lw = index[w.as_str()];
                    let lv = low.get_mut(&v).expect("visited");
                    *lv = (*lv).min(lw);
                }
                continue;
            }
            work.pop();
            let lv = low[&v];
            if let Some((u, _)) = work.last() {
                let lu = low.get_mut(u).expect("visited");
                *lu = (*lu).min(lv);
            }
            if lv != index[&v] {
                continue;
            }
            // A group of functions that call each other: one answer for all of them.
            let mut group = Vec::new();
            while let Some(m) = stack.pop() {
                on.remove(&m);
                let last = m == v;
                group.push(m);
                if last {
                    break;
                }
            }
            let (mut w, mut r) = (false, false);
            for m in &group {
                if let Some(o) = self.own_of(m, cx, seen) {
                    w |= !o.writes.is_empty();
                    r |= o.reads.is_some();
                    for c in &o.calls {
                        if let Some((cw, cr)) = seen.reach.get(c.as_str()) {
                            w |= *cw;
                            r |= *cr;
                        }
                    }
                }
            }
            for m in group {
                seen.reach.insert(m, (w, r));
            }
        }
        seen.reach.get(name).copied().unwrap_or((false, false))
    }

    /// What `root` (a route's or a function's own code) surely does with the program's functions it
    /// calls: breadth first, each function visited once, so what is nearest is said whatever order
    /// the routes and the functions are written in, and a chain of calls however long takes no
    /// stack. `all`: every write reached, else the first.
    fn get_facts(&self, root: &GetOwn, file: &Arc<str>, cx: &GetCx, seen: &mut GetSeen, all: bool) -> GetFacts {
        let mut f = GetFacts { writes: Vec::new(), reads: root.reads.clone().map(|r| (r, None)) };
        let add = |f: &mut GetFacts, w: GetWrite| {
            if !f.writes.contains(&w) {
                f.writes.push(w);
            }
        };
        for (what, line) in &root.writes {
            add(&mut f, GetWrite { what: what.clone(), at: None, file: file.clone(), line: *line });
        }
        // The names to visit, in the order reached: those from `next` on are still to visit.
        let mut visited: HashSet<String> = HashSet::new();
        let mut names: Vec<String> = root.calls.clone();
        let mut next = 0;
        while (all || f.writes.is_empty()) || f.reads.is_none() {
            let Some(n) = names.get(next).cloned() else { break };
            next += 1;
            if !visited.insert(n.clone()) {
                continue;
            }
            // Past what reaches neither, once the walk has a read.
            let (cw, cr) = self.reaches(&n, cx, seen);
            if !cw && (!cr || f.reads.is_some()) {
                continue;
            }
            let Some(o) = self.own_of(&n, cx, seen) else { continue };
            if all || f.writes.is_empty() {
                let at = cx.files.get(&n).cloned().unwrap_or_else(|| file.clone());
                for (what, line) in &o.writes {
                    add(&mut f, GetWrite { what: what.clone(), at: Some(n.clone()), file: at.clone(), line: *line });
                }
            }
            if f.reads.is_none() {
                f.reads = o.reads.clone().map(|r| (r, Some(n.clone())));
            }
            names.extend(o.calls.iter().cloned());
        }
        f
    }

    /// What `code` (with its function's `params`) surely does on the server by itself (`GetWalk`).
    fn get_own(&self, code: &[&Expr], params: &[String], cx: &GetCx) -> GetOwn {
        let names = ["request", "__request"];
        let mut w = GetWalk::new(self, cx, &names, params);
        w.exprs(code);
        w.done()
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

    /// Whether the check has run past its budget, looked at once `budget_left` runs out: then each
    /// expression asks again, and is not walked (FEEDBACK75.md, Q-1053; branch feedback75).
    #[cold]
    fn past_budget(&mut self) -> bool {
        if !self.over_budget {
            self.over_budget = OVER_BUDGET.with(|o| {
                if DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d) {
                    o.set(true);
                }
                o.get()
            });
        }
        self.budget_left = if self.over_budget { 1 } else { 256 };
        self.over_budget
    }

    fn expr(&mut self, e: &Expr) -> T {
        // The parser refuses code that nests deeper; a guard all the same.
        if self.depth >= 2 * crate::parser::MAX_NESTING {
            self.error("the code nests too deeply");
            return T::ANY;
        }
        // A check past its budget walks no further: what it says is replaced by one error at
        // its end. The time is looked at every 256th expression, counted down so an expression
        // pays one subtraction (FEEDBACK75.md, Q-1053; branch feedback75).
        self.budget_left -= 1;
        if self.budget_left == 0 && self.past_budget() {
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
                // Each key's place, so a map of 100,000 fields is not read once for each, as it
                // was in 14.9 s (FEEDBACK75.md, Q-1053, found in review; branch feedback75).
                let mut at: HashMap<&str, usize> = HashMap::new();
                for (k, v) in entries {
                    let t = self.expr(v);
                    // A key written twice: the last value is kept, with nothing said, however each
                    // was written (`a`, `"a"`, `{ a, a: 2 }`). A spread between them is no map
                    // literal's key: the parser makes a map of each part (FEEDBACK55.md, Q-794;
                    // branch feedback55).
                    match at.get(k.as_str()) {
                        Some(&i) => {
                            self.warn(format!("the key `{}` is written twice in this map: the last value is the one kept", k));
                            fields[i].1 = t;
                        }
                        None => {
                            at.insert(k.as_str(), fields.len());
                            fields.push((Arc::from(k.as_str()), t));
                        }
                    }
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
                // The function a top-level `every` or `schedule` is given runs on the server
                // (FEEDBACK67.md, Q-959; branch pwa).
                let job = match callee.as_ref() {
                    _ if !self.fns.is_empty() || !self.sequential || !matches!(args.get(1), Some(Expr::Lambda(_))) => None,
                    Expr::Ident(n, _) if n == "every" && self.builtin_in_scope(n) => Some("in a tick"),
                    Expr::Ident(n, _) if n == "schedule" && self.builtin_in_scope(n) => Some("in a job"),
                    _ => None,
                };
                if job.is_some() {
                    self.job_arg = job;
                }
                let t = self.call(callee, args, None);
                if job.is_some() {
                    self.job_arg = None;
                }
                t
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
                // Its body's value goes nowhere, each round (FEEDBACK80.md, Q-1267, and
                // FEEDBACK72.md, Q-1018; branch feedback72-81).
                self.mark_end(body, DropHow::Stmt, &[], 0);
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
                // Its last statement's value goes nowhere, marked by what holds it (`mark_tails`;
                // FEEDBACK80.md, Q-1267; branch feedback72-81).
                let dropped = if self.quiet == 0 && !self.drop_marks.is_empty() { self.drop_marks.remove(&(e as *const Expr)) } else { None };
                self.push();
                let mut t = T::of(NIL);
                let mut never = false;
                for (i, s) in stmts.iter().enumerate() {
                    // Past its budget a check reads no further statement: each `let` still read what
                    // it holds (its SQL, its URL) among all before it, and 85,000 of them went on
                    // 15 s past a 2 s budget (FEEDBACK75.md follow-up found by 0.8.9's coverage,
                    // Q-1127; branch feedback75).
                    if self.over_budget {
                        break;
                    }
                    if i + 1 < stmts.len() {
                        self.answer_dropped(s, true);
                        self.op_dropped(s);
                        self.redirect_dropped(s);
                        // What it ends with is dropped with it (FEEDBACK80.md, Q-1267, and
                        // FEEDBACK72.md, Q-1018; branch feedback72-81).
                        if let Stmt::Expr(e) = s {
                            self.mark_tails(e, DropHow::Stmt, 0);
                        }
                    } else if let (Some(how), Stmt::Expr(x)) = (dropped, s) {
                        self.drop_check(x, how);
                        self.mark_tails(x, how, 0);
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
            self.warn(format!("`@{}` never fires: the browser's event names are lower case, `on{}` (or React's `on{}`)", word, word.to_ascii_lowercase(), {
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
            self.warn(format!("`{p}{word}` is no {} the browser knows: did you mean `{p}{near}`?", if prefix.is_empty() { "attribute" } else { "event" }, p = if prefix.is_empty() { "" } else { "on" }));
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
        if let Stmt::Expr(e) = s {
            self.op_dropped_e(e);
        }
    }

    /// `op_dropped` of an expression whose value nothing takes, a statement's or the end of one
    /// (`tails_dropped`; FEEDBACK80.md, Q-1267; branch feedback72-81).
    fn op_dropped_e(&mut self, e: &Expr) {
        let Expr::Binary(op, l, _, line) = e else { return };
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

    /// What a statement whose value nothing takes ends with goes with it: the last statement of
    /// each block, `if` branch, `match` arm and `for` body it holds, as the run takes each of them
    /// for what it does alone (`Interp::eval_unused`, and a `match`'s value is its arm's). Each is
    /// said of as a statement of its own is: a new value dropped, `if c { xs.push(v) }`
    /// (FEEDBACK80.md, Q-1267), and an error dropped, `if c { save(n) }` (FEEDBACK72.md, Q-1018;
    /// branch feedback72-81). A block's last statement is said of where the block is checked, in
    /// its own scope, with its own names (`if c { let ys = [1]; ys.push(2) }`): the block is marked
    /// here (`drop_marks`) and the mark taken there. A way that is no block (a `match` arm's
    /// `0 => xs.push(1)`) is said of here, unless what it names is one its pattern binds. One level
    /// deeper per way, with room on the stack looked at every eighth, as `Returns::deep` does.
    fn mark_tails(&mut self, e: &Expr, how: DropHow, depth: u32) {
        if self.quiet > 0 {
            return;
        }
        match e {
            Expr::If(_, a, b) => {
                self.mark_end(a, how, &[], depth);
                if let Some(b) = b {
                    self.mark_end(b, how, &[], depth);
                }
            }
            Expr::Match(_, arms, _) => {
                for a in arms {
                    let mut names = Vec::new();
                    pattern_names(&a.pat, &mut names);
                    self.mark_end(&a.body, how, &names, depth);
                }
            }
            Expr::For(p, _, body, _) => {
                let mut names = Vec::new();
                pattern_names(p, &mut names);
                self.mark_end(body, how, &names, depth);
            }
            Expr::Block(_) => {
                self.drop_marks.insert(e as *const Expr, how);
            }
            _ => {}
        }
    }

    /// One way of `mark_tails`: a block marked, or a way that is none said of here.
    fn mark_end(&mut self, end: &Expr, how: DropHow, bound: &[String], depth: u32) {
        if let Expr::Block(_) = end {
            self.drop_marks.insert(end as *const Expr, how);
            return;
        }
        if !dropped_subject(end).is_some_and(|n| bound.iter().any(|b| b == n)) {
            self.drop_check(end, how);
        }
        if (depth + 1) % 8 == 0 {
            crate::stack::maybe_grow(256 << 10, 4 << 20, || self.mark_tails(end, how, depth + 1));
        } else {
            self.mark_tails(end, how, depth + 1);
        }
    }

    /// An expression whose value nothing takes, said of as `how` says.
    fn drop_check(&mut self, e: &Expr, how: DropHow) {
        if let Expr::Call(_, _, line) | Expr::Pipe(_, _, line) | Expr::Binary(_, _, _, line) = e {
            self.line = *line;
        }
        self.answer_dropped_e(e, matches!(how, DropHow::Stmt | DropHow::Handler));
        self.op_dropped_e(e);
        match how {
            DropHow::Stmt => self.error_dropped_e(e, T::ANY),
            DropHow::Handler => self.handler_error_dropped(e),
            DropHow::Plain => {}
        }
    }

    /// An error a handler's last call gives, which goes nowhere: said with what a handler can do
    /// about it, not `?`, which leaves the handler with it (FEEDBACK72.md, Q-1018, found in review;
    /// branch feedback72-81). Only of the program's own functions that make an error themselves
    /// (`err(...)`, `?`, `! E`): not a server fn's, whose failed call the runtime reports in the
    /// console, as `onclick={() => pay(o.id)}` is the guide's own way (decided in review), nor one
    /// a builtin gives, as elsewhere.
    fn handler_error_dropped(&mut self, e: &Expr) {
        let Expr::Call(f, _, line) = e else { return };
        let Expr::Ident(name, _) = &**f else { return };
        // The component's own function too, as `onclick={go}` reads it (found in review).
        let Some(d) = self.handler_fn(name) else { return };
        if self.gives_program_error(&d) {
            self.line = *line;
            self.warn(format!("`{0}` can give an error, and this handler drops it: give a value with `{0}(...) ?? ...`, test it with `is_err`, or write `let _ = {0}(...)` if it does not matter", name));
        }
    }

    /// A function written as a handler, `onclick={() => { ... }}`, before it is checked: its value
    /// goes nowhere, so its last statement is marked as dropped, or, written with no block, said
    /// of here (FEEDBACK80.md, Q-1267, and FEEDBACK72.md, Q-1018; branch feedback72-81).
    fn handler_marks(&mut self, d: &FnDef) {
        if self.quiet > 0 {
            return;
        }
        let params: Vec<String> = d.params.iter().map(|p| p.name.to_string()).collect();
        self.mark_end(&d.body, DropHow::Handler, &params, 0);
    }

    /// A function given to a handler by name, `onclick={go}`, the component's own `fn` or
    /// `let go = () => ...`, or the program's: what `go()` gives goes nowhere. Said at the handler,
    /// not in `go`, whose other callers may take it: an error `go` makes itself (`err(...)`, `?`,
    /// `! E`; not a server fn's, nor a builtin's, as `handler_error_dropped` has it for
    /// `onclick={() => go()}`), and a new value its last line drops, `items.push(1)`,
    /// unless that names what `go` binds itself. Read with `go`'s file, and none of this
    /// component's names when `go` is the program's; the line is put back after (FEEDBACK80.md,
    /// Q-1267, and FEEDBACK72.md, Q-1018, found in review; branch feedback72-81).
    #[inline(never)]
    fn handler_dropped(&mut self, v: &AttrVal, ev: &str) {
        if self.quiet > 0 {
            return;
        }
        let AttrVal::Expr(Expr::Ident(n, _)) = v else { return };
        let local = self.scopes.iter().rev().find_map(|s| s.get(n.as_str()));
        let global = if local.is_some() { None } else { self.globals.get(n.as_str()).filter(|v| &*v.file != "<builtin>") };
        let Some(sig) = local.or(global).and_then(|v| v.sig.clone()).filter(|s| !s.builtin && !s.component && s.native.is_none()) else { return };
        let Some(d) = sig.def.clone() else { return };
        let global = global.map(|v| v.file.clone());
        // As the inline handler is judged: `onclick={save}` and `onclick={() => save()}` agree
        // (found in review).
        if !sig.server && self.gives_program_error(&d) {
            self.warn(format!(
                "`on{0}={{{1}}}` drops the error `{1}` can give: handle it in `{1}`, or write `on{0}={{() => {{ {1}() ?? ... }}}}`",
                ev, n
            ));
        }
        let last = match &d.body {
            Expr::Block(stmts) => match stmts.last() {
                Some(Stmt::Expr(x)) => x,
                _ => return,
            },
            e => e,
        };
        let line = self.line;
        let saved = global.map(|file| (std::mem::replace(&mut self.file, file), std::mem::take(&mut self.scopes), std::mem::take(&mut self.scope_ids)));
        let bound = fn_bound(&d);
        self.named_tail(last, &bound, 0);
        if let Some((file, scopes, ids)) = saved {
            self.file = file;
            self.scopes = scopes;
            self.scope_ids = ids;
        }
        self.line = line;
    }

    /// `handler_dropped`'s last line and what it ends with, read after `go` was checked: a new
    /// value dropped, but for what `go` binds itself (`bound`), whose type is `go`'s to know.
    fn named_tail(&mut self, e: &Expr, bound: &HashSet<String>, depth: u32) {
        if !dropped_subject(e).is_some_and(|n| bound.contains(n)) {
            self.drop_check(e, DropHow::Plain);
        }
        let ends: Vec<&Expr> = match e {
            Expr::If(_, a, b) => std::iter::once(&**a).chain(b.as_deref()).collect(),
            Expr::Match(_, arms, _) => arms.iter().map(|a| &a.body).collect(),
            Expr::For(_, _, body, _) => vec![&**body],
            Expr::Block(_) => vec![e],
            _ => return,
        };
        for end in ends {
            let last = match end {
                Expr::Block(stmts) => match stmts.last() {
                    Some(Stmt::Expr(x)) => x,
                    _ => continue,
                },
                x => x,
            };
            if (depth + 1) % 8 == 0 {
                crate::stack::maybe_grow(256 << 10, 4 << 20, || self.named_tail(last, bound, depth + 1));
            } else {
                self.named_tail(last, bound, depth + 1);
            }
        }
    }

    /// `own`: the program's own functions too (Q-931), in a function's, a route's or a handler's
    /// code; not at the top level, where a call alone is a script's or a probe's.
    fn answer_dropped(&mut self, s: &Stmt, own: bool) {
        if let Stmt::Expr(e) = s {
            self.answer_dropped_e(e, own);
        }
    }

    /// `answer_dropped` of an expression whose value nothing takes (FEEDBACK80.md, Q-1267; branch
    /// feedback72-81).
    fn answer_dropped_e(&mut self, e: &Expr, own: bool) {
        if own && self.own_answer_dropped(e) {
            return;
        }
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
            // `cache.set(k, v)` on a map with a field `set`: its own function, called (found in
            // review of FEEDBACK64.md's Q-931; branch feedback64).
            if t.and_then(|t| t.fields()).is_some_and(|fs| fs.iter().any(|(k, _)| &**k == name)) {
                return;
            }
        }
        let x = given.unwrap_or_else(|| "x".to_string());
        self.warn(format!("`{}` gives a new value and changes nothing, and it is dropped here: write `{} = {} | {}(...)`", name, x, x, name));
    }

    /// `down(val)` alone, `down` the program's own function, one that changes nothing: what it
    /// gives is all it does, and it is dropped. A `server fn`'s arguments are copies too, which a
    /// newcomer reads as the page's own: the button did nothing, and nothing said so
    /// (FEEDBACK64.md, Q-931; branch feedback64). Whether it warned.
    fn own_answer_dropped(&mut self, e: &Expr) -> bool {
        let (name, args, piped, line) = match e {
            Expr::Call(f, args, line) => match &**f {
                Expr::Ident(n, _) => (n.to_string(), args.as_slice(), None, *line),
                _ => return false,
            },
            Expr::Pipe(l, r, line) => match &**r {
                Expr::Call(f, args, _) => match &**f {
                    Expr::Ident(n, _) => (n.to_string(), args.as_slice(), Some(&**l), *line),
                    _ => return false,
                },
                Expr::Ident(n, _) => (n.to_string(), &[][..], Some(&**l), *line),
                _ => return false,
            },
            _ => return false,
        };
        if self.is_local(&name) {
            return false;
        }
        let Some(sig) = self.globals.get(&name).and_then(|v| v.sig.clone()) else { return false };
        // A function declared, under its name: not a variable holding one (`let hook = (x) => x`),
        // which the walk below takes as one that may hold any.
        let Some(d) = sig.def.clone().filter(|d| !sig.builtin && sig.native.is_none() && !sig.component && d.name == name) else { return false };
        // A call the checker refuses already (an argument missing, one too many): that is said.
        let given = args.len() + usize::from(piped.is_some());
        if given < d.params.iter().filter(|p| p.default.is_none()).count() || given > d.params.len() {
            return false;
        }
        // A function given to it changes something only if it is called, and the walk below takes a
        // parameter called, or given to a builtin that calls functions (`run_after(ms, done)`), as a
        // change: `flash(2000, () => { saved = false })` (found in review).
        let held = |c: &Self, n: &str| c.scopes.iter().rev().find_map(|s| s.get(n)).or_else(|| c.globals.get(n)).map(|v| v.t);
        // One that may give an error is run for it as well (`check_age(a)?`): the check of a
        // dropped error says that, by the same signs, its type, `! T`, or an error made or passed
        // on in its body (`error_dropped`; found in review).
        let ret = sig.ret.unwrap_or_else(|| self.fn_ret(&sig));
        // Its body read once for each function, not for each call dropped: the ends of statements
        // dropped ask it far more often (FEEDBACK80.md, Q-1267; branch feedback72-81).
        let fails = d.err.is_some()
            || sig.err.is_some()
            || (ret.bits != ANY && ret.bits & ERR != 0)
            || *self.makes_errors.borrow_mut().entry(def_key(&d)).or_insert_with(|| {
                let mut found = false;
                walk_expr(&d.body, &mut |e| {
                    found |= matches!(e, Expr::Try(_)) || matches!(e, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "err" || n == "error"));
                });
                found
            });
        if fails {
            return false;
        }
        if self.changes_anything(&d) {
            return false;
        }
        self.line = line;
        let first = piped.or(args.first());
        let given = match first {
            Some(Expr::Ident(n, _)) => Some(n.to_string()),
            _ => None,
        };
        // `x = f(x)` only where what it gives can be held by `x`: `cart = total_of(cart)` would make
        // a list an int (found in review).
        // What the variable holds: its type, or, not known, the type of the parameter it is given
        // to, which it passed (`down(val: int)`).
        let had = given.as_ref().and_then(|x| held(self, x)).filter(|h| !h.vague()).or_else(|| d.params.first().and_then(|p| p.ty.as_ref()).map(ty_t));
        // Whole, its items and fields too (found in review: `list<int>` given `list<str>`); and not
        // a server variable in a component, which the browser cannot assign.
        let server_var = given.as_ref().is_some_and(|x| !self.is_local(x) && self.globals.contains_key(x)) && self.in_component().is_some();
        let fits = !server_var && had.is_some_and(|h| !h.vague() && !ret.vague() && ret.bits != 0 && ret.bits & !h.bits == 0 && mismatch(ret, h).is_none());
        let call = match (piped, args.len()) {
            (Some(_), 0) => format!("{} | {}", given.as_deref().unwrap_or("x"), name),
            (Some(_), _) => format!("{} | {}(...)", given.as_deref().unwrap_or("x"), name),
            (None, 1) => format!("{}({})", name, given.as_deref().unwrap_or("...")),
            (None, 0) => format!("{}()", name),
            (None, _) => format!("{}({}, ...)", name, given.as_deref().unwrap_or("...")),
        };
        let fix = match &given {
            Some(x) if fits => format!(": write `{} = {}`", x, call),
            _ => ": use what it gives, or leave the call out".to_string(),
        };
        let copies = match (&given, sig.server || d.server) {
            (Some(x), true) => format!("; a `server fn`'s arguments are copies, so the server cannot change `{}`", x),
            _ => String::new(),
        };
        self.warn(format!("`{}` changes nothing, and what it gives is dropped here{}{}", name, fix, copies));
        true
    }

    /// Whether a call of `d` can change anything: assign a name of the program's or one it did
    /// not declare, set a page object's field, call a builtin that acts or reaches the database,
    /// the network or files, ask the user, call what the checker cannot read (a function held in a
    /// variable, a native plugin's, a service's, a component), hand a function of its own to a
    /// builtin that calls functions, or call a method on what may be a page object or a map of
    /// functions. Its own functions are read through; one read again while it is read (recursion)
    /// adds nothing (FEEDBACK64.md, Q-931; branch feedback64).
    fn changes_anything(&mut self, d: &Arc<FnDef>) -> bool {
        let key = Arc::as_ptr(d) as usize;
        match self.changes_cache.get(&key) {
            Some(Ok(c)) => return *c,
            Some(Err(depth)) => {
                self.changes_low = self.changes_low.min(*depth);
                return false;
            }
            None => {}
        }
        let depth = self.changes_depth;
        self.changes_depth += 1;
        self.changes_cache.insert(key, Err(depth));
        let mark = self.changes_pending.len();
        let saved = std::mem::replace(&mut self.changes_low, usize::MAX);
        let c = self.changes_in(d);
        let low = self.changes_low;
        self.changes_depth -= 1;
        self.changes_low = saved.min(low);
        // As Tarjan's strongly connected components: one that reached no function still being read
        // above it is done, and so is every function of its cycle read meanwhile, each reaching
        // all the others; one that did waits, kept as part of that cycle so it is not read again
        // on another path (found in review: a cycle's members were read once a path, exponential
        // in the paths).
        if low >= depth {
            for k in self.changes_pending.drain(mark..) {
                self.changes_cache.insert(k, Ok(c));
            }
            self.changes_cache.insert(key, Ok(c));
        } else if c {
            self.changes_cache.insert(key, Ok(true));
        } else {
            self.changes_cache.insert(key, Err(low));
            self.changes_pending.push(key);
        }
        c
    }

    fn changes_in(&mut self, d: &FnDef) -> bool {
        #[derive(PartialEq, Eq, Hash)]
        enum Recv {
            Literal,
            Name(String),
            Other,
        }
        #[derive(PartialEq, Eq, Hash)]
        enum Found {
            Changes,
            Assigns(String),
            Calls(String),
            Names(String),
            Method(Recv, String),
            /// What is given to the builtin `.0` as argument `.1`: a name, or another expression.
            Passed(String, usize, Option<String>),
        }
        if d.component || d.effects.as_ref().is_some_and(|e| !e.is_empty()) {
            return true;
        }
        let params: HashMap<String, Option<T>> = d.params.iter().map(|p| (p.name.to_string(), p.ty.as_ref().map(ty_t))).collect();
        // What it declares anywhere in it (a `let`, a loop's, a match's, a lambda's), and its own
        // functions, whose code is read here.
        let mut declared: HashSet<String> = HashSet::new();
        let mut fns: HashSet<String> = HashSet::new();
        let mut found: HashSet<Found> = HashSet::new();
        let mut names = Vec::new();
        let recv = |o: &Expr| match o {
            Expr::Lit(_) | Expr::List(_) | Expr::Map(_) | Expr::Template(_) => Recv::Literal,
            Expr::Ident(n, _) => Recv::Name(n.to_string()),
            _ => Recv::Other,
        };
        // What a call is given, by position, a list's items at the list's (`parallel([f, g])`):
        // a lambda is code read here, a literal no function (found in review).
        fn given(callee: &str, at: usize, a: &Expr, found: &mut HashSet<Found>) {
            match a {
                Expr::Lambda(_) | Expr::Lit(_) | Expr::Template(_) | Expr::Map(_) | Expr::Binary(..) | Expr::Unary(..) => {}
                Expr::List(items) => items.iter().for_each(|x| given(callee, at, x, found)),
                Expr::Ident(n, _) => {
                    found.insert(Found::Passed(callee.to_string(), at, Some(n.to_string())));
                }
                _ => {
                    found.insert(Found::Passed(callee.to_string(), at, None));
                }
            }
        }
        let passed = |callee: &str, args: &[Expr], from: usize, found: &mut HashSet<Found>| {
            for (i, a) in args.iter().enumerate() {
                given(callee, i + from, a, found);
            }
        };
        walk_fn(d, &mut |x| match x {
            Expr::Block(stmts) => {
                for s in stmts {
                    match s {
                        Stmt::Let(p, _, _) => pattern_names(p, &mut names),
                        Stmt::Fn(f) => {
                            fns.insert(f.name.to_string());
                        }
                        Stmt::Assign(n, ..) => {
                            found.insert(Found::Assigns(n.to_string()));
                        }
                        Stmt::SetField(..) | Stmt::State(..) | Stmt::Route(..) | Stmt::Service(..) | Stmt::Mount(..) | Stmt::Use(..) | Stmt::Type(..) => {
                            found.insert(Found::Changes);
                        }
                        Stmt::Expr(_) | Stmt::Return(..) => {}
                    }
                }
            }
            Expr::For(p, ..) => pattern_names(p, &mut names),
            Expr::Match(_, arms, _) => arms.iter().for_each(|a| pattern_names(&a.pat, &mut names)),
            Expr::Lambda(l) => names.extend(l.params.iter().map(|p| p.name.to_string())),
            Expr::Call(c, args, _) => match &**c {
                Expr::Ident(n, _) => {
                    found.insert(Found::Calls(n.to_string()));
                    passed(n, args, 0, &mut found);
                }
                Expr::Field(o, m, _) => {
                    found.insert(Found::Method(recv(o), m.clone()));
                    passed(m, args, 1, &mut found);
                }
                _ => {
                    found.insert(Found::Changes);
                }
            },
            // `a | f(b)` gives `a` as argument 1 (found in review: `f | unwatched`).
            Expr::Pipe(l, r, _) => match &**r {
                Expr::Call(c, args, _) => {
                    if let Expr::Ident(n, _) = &**c {
                        passed(n, args, 1, &mut found);
                        passed(n, std::slice::from_ref(&**l), 0, &mut found);
                    }
                }
                Expr::Ident(n, _) => {
                    found.insert(Found::Calls(n.to_string()));
                    passed(n, std::slice::from_ref(&**l), 0, &mut found);
                }
                _ => {
                    found.insert(Found::Changes);
                }
            },
            Expr::Ident(n, _) => {
                found.insert(Found::Names(n.to_string()));
            }
            Expr::Reply(..) => {
                found.insert(Found::Changes);
            }
            Expr::Html(el, _) if el.tag.starts_with(|c: char| c.is_ascii_uppercase()) => {
                found.insert(Found::Calls(el.tag.to_string()));
            }
            _ => {}
        });
        declared.extend(names);
        let own = |n: &str| params.contains_key(n) || declared.contains(n);
        for f in found {
            let changes = match f {
                Found::Changes => true,
                // A name of the program's, assigned, whatever local of that name is declared
                // somewhere in it; any other not its own (found in review: a lambda's `total` hid
                // the global's).
                Found::Assigns(n) => !params.contains_key(&n) && (self.globals.get(&n).is_some_and(|v| v.sig.is_none() && &*v.file != "<builtin>") || !declared.contains(&n)),
                Found::Calls(n) | Found::Names(n) if fns.contains(&n) => false,
                // A value of its own, called: it may run anything.
                Found::Calls(n) if own(&n) => true,
                Found::Names(n) if own(&n) => false,
                Found::Calls(n) => self.name_changes(&n, true),
                Found::Names(n) => self.name_changes(&n, false),
                // What is given where a builtin takes a function: `map(xs, f)`, `run_after(ms, done)`
                // call it. Its own function is code read here; any other is taken as called
                // (found in review: a variable holding a lambda, a map's field, a service's).
                Found::Passed(callee, at, n) => {
                    // A parameter typed a function, not `any` (`str(x)` takes anything and calls
                    // nothing); `parallel` and `defer` run what they are given, typed `any`.
                    let takes_fn = matches!(callee.as_str(), "parallel" | "defer")
                        || self.globals.get(&callee).and_then(|v| v.sig.clone()).filter(|s| s.builtin).is_some_and(|s| {
                            s.params.get(at).or(if s.variadic { s.params.last() } else { None }).is_some_and(|p| p.ty & FN != 0 && p.ty | NIL | ERR != ANY)
                        });
                    takes_fn
                        && match n {
                            Some(n) if fns.contains(&n) => false,
                            Some(n) if own(&n) => true,
                            Some(n) => self.name_changes(&n, true),
                            None => true,
                        }
                }
                Found::Method(Recv::Literal, m) => !self.globals.get(&m).is_some_and(|v| &*v.file == "<builtin>") || builtin_changes(&m),
                Found::Method(Recv::Name(o), m) => {
                    if fns.contains(&o) {
                        true
                    } else if own(&o) {
                        // A builtin called as a method only on what it is declared to be, a list, a
                        // text or a number: anything else may be a page object (`el.remove()`) or a
                        // map holding functions (`cache.set(k, v)`) (found in review).
                        let t = params.get(&o).copied().flatten();
                        !t.is_some_and(|t| !t.vague() && t.bits & !(LIST | STR | NUM | BOOL | NIL) == 0) || !self.globals.get(&m).is_some_and(|v| &*v.file == "<builtin>") || builtin_changes(&m)
                    } else if self.globals.get(&format!("{}.{}", o, m)).is_some_and(|v| &*v.file == "<builtin>") {
                        builtin_changes(&format!("{}.{}", o, m))
                    } else if self.services.contains_key(&o) || self.sites.contains(&o) {
                        true
                    } else {
                        let t = self.globals.get(&o).map(|v| v.t);
                        !t.is_some_and(|t| !t.vague() && t.bits & !(LIST | STR | NUM | BOOL | NIL) == 0) || !self.globals.get(&m).is_some_and(|v| &*v.file == "<builtin>") || builtin_changes(&m)
                    }
                }
                Found::Method(Recv::Other, _) => true,
            };
            if changes {
                return true;
            }
        }
        false
    }

    /// Whether calling `n`, or naming it (a function named may be called later), a name not
    /// `d`'s own, can change anything.
    fn name_changes(&mut self, n: &str, called: bool) -> bool {
        // A builtin the checker keeps apart from its names (`str`, `int`), as the others.
        let Some(v) = self.globals.get(n) else { return if BUILTINS.iter().any(|(b, _, _)| *b == n) { builtin_changes(n) } else { called } };
        if &*v.file == "<builtin>" {
            return builtin_changes(n);
        }
        if v.sig.as_ref().is_some_and(|s| s.native.is_some() || s.component) {
            return true;
        }
        // A variable, called: it may hold any function by then, whatever it was given first; named,
        // when it may hold one, as it may be called later (found in review). A function declared
        // is the one its signature was made of, under its name.
        if !v.sig.as_ref().and_then(|s| s.def.as_ref()).is_some_and(|d| d.name == n) {
            return called || v.t.bits & FN != 0;
        }
        match v.sig.clone() {
            Some(sig) if sig.native.is_none() && !sig.component => match sig.def.clone() {
                Some(d) => self.changes_anything(&d),
                None => true,
            },
            _ => true,
        }
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
        if let Stmt::Expr(e) = s {
            self.error_dropped_e(e, t);
        }
    }

    /// `error_dropped` of an expression whose value nothing takes, of the type `t` (`T::ANY` when
    /// not worked out: its function then says whether it can fail) (FEEDBACK72.md, Q-1018; branch
    /// feedback72-81).
    fn error_dropped_e(&mut self, e: &Expr, t: T) {
        let Expr::Call(f, _, _) = e else { return };
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
        // passes one on with `?` (`can_fail`).
        let fails = (t.bits != ANY && t.bits & ERR != 0) || self.globals.get(name).and_then(|v| v.sig.as_ref()).and_then(|s| s.def.as_ref()).is_some_and(|d| self.can_fail(d));
        if !fails {
            return;
        }
        if self.globals.get(name).is_some_and(|v| &*v.file != "<builtin>" && v.sig.as_ref().is_some_and(|s| !s.builtin && !s.component)) {
            self.warn(format!("`{}` can give an error, and it is dropped here: write `{}(...)?` to pass it on, or `let _ = {}(...)` if it does not matter", name, name, name));
        }
    }

    /// `can_fail(d)`, worked out once for each function.
    fn can_fail(&self, d: &Arc<FnDef>) -> bool {
        *self.fails.borrow_mut().entry(def_key(d)).or_insert_with(|| can_fail(d))
    }

    /// Whether a call of `d` can give an error the program makes, for a handler that drops it: `d`
    /// makes one itself (`can_fail`: `err(...)`, `?`, `! E`), or gives what a program function that
    /// can calls (`fn go() { save(1) }`, `save` declared `! str`), as far as that goes. Not a server
    /// fn's, whose failed call the runtime reports, nor one only a builtin gives (decided in review
    /// of FEEDBACK72.md's Q-1018; branch feedback72-81). Read with a list, not by recursion.
    fn gives_program_error(&self, d: &Arc<FnDef>) -> bool {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut todo: Vec<Arc<FnDef>> = vec![d.clone()];
        while let Some(d) = todo.pop() {
            if d.server || !seen.insert(def_key(&d)) {
                continue;
            }
            if self.can_fail(&d) {
                return true;
            }
            let bound = fn_bound(&d);
            for n in returned_calls(&d.body) {
                if bound.contains(&n) {
                    continue;
                }
                let next = self.globals.get(n.as_str()).filter(|v| &*v.file != "<builtin>").and_then(|v| v.sig.as_ref()).filter(|s| !s.builtin && !s.component && !s.server).and_then(|s| s.def.clone());
                todo.extend(next);
            }
        }
        false
    }

    /// The function a handler names or calls last, `onclick={go}` or `onclick={() => go()}`: the
    /// component's own, or the program's (not a builtin's).
    fn handler_fn(&self, n: &str) -> Option<Arc<FnDef>> {
        let local = self.scopes.iter().rev().find_map(|s| s.get(n));
        let found = match local {
            Some(v) => v,
            None => self.globals.get(n).filter(|v| &*v.file != "<builtin>")?,
        };
        found.sig.as_ref().filter(|s| !s.builtin && !s.component && !s.server && s.native.is_none()).and_then(|s| s.def.clone())
    }

    /// Whether a value shown in markup comes from a call that can fail, not handled: the program's
    /// function or server fn that can (`can_fail`: `server fn load() -> str ! str`, `?`,
    /// `err(...)`), called there, or a `let` or `state` set from one (`failing_vars`). Not a
    /// builtin's (`int(n)`, `dec(rate)`, a date read), whose error the page shows as nothing and
    /// logs (decided in review), nor the program's function whose type has an error only from a
    /// builtin in it it took as sure (`fn price_text() -> str` ending in `re.replace(...)`)
    /// (FEEDBACK81.md, Q-1272; branch feedback72-81).
    #[inline(never)]
    fn shows_failure(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(n, _) => self
                .scopes
                .iter()
                .rev()
                .find_map(|s| s.get(n.as_str()))
                .or_else(|| self.globals.get(n.as_str()))
                .is_some_and(|v| self.failing_vars.contains(&(v.file.clone(), v.line, n.to_string()))),
            Expr::Call(f, _, _) => match &**f {
                Expr::Ident(n, _) if !self.is_local(n) => self.globals.get(n.as_str()).is_some_and(|v| {
                    &*v.file != "<builtin>" && v.sig.as_ref().filter(|s| !s.component && !s.builtin).and_then(|s| s.def.as_ref()).is_some_and(|d| self.can_fail(d))
                }),
                _ => false,
            },
            _ => false,
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
        // Where `redirect` and `navigate` send the visitor (FEEDBACK79.md, Q-1251; branch
        // feedback72-81).
        if let (Expr::Ident(n, _), None) = (callee, &pre) {
            if DRAWS.contains(&n.as_str()) && self.builtin_in_scope(n) {
                self.draws_seen = true;
                let names: Vec<String> = self.fns.iter().map(|f| f.name.clone()).collect();
                self.drawers.extend(names);
            }
            if (n == "redirect" || n == "navigate") && self.builtin_in_scope(n) {
                if let Some((t, whole)) = args.first().and_then(link_start) {
                    // A 307 or 308 is followed with the request's own method (found in review).
                    // One not written as a number may be either (found in review).
                    let same = match args.get(1) {
                        None | Some(Expr::Lit(Lit::Int(_))) => matches!(args.get(1), Some(Expr::Lit(Lit::Int(307 | 308)))),
                        Some(_) => true,
                    };
                    let what = match (n.as_str(), same) {
                        ("navigate", _) => "navigate",
                        (_, true) => "redirect-same",
                        _ => "redirect",
                    };
                    self.page_links.push((self.file.clone(), line, t, whole, what));
                }
            }
        }
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
            // `about(question, choices, default)` with its choices written: what it refuses as it
            // runs (FEEDBACK56.md, Part E, and the user's decision 2026-11-09; branch feedback56).
            if n == "about" && args.len() == 3 && !self.is_local(n) && self.globals.get(n.as_str()).map_or(false, |v| &*v.file == "<builtin>") {
                self.about_written(args);
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
                    // is what `stats.load()` means: its function is called. Not for one named
                    // `notification`, a message's natural name, whose `send` and the rest are the
                    // module's (FEEDBACK67.md, B.3; branch pwa).
                    let own_map = matches!(hidden, Some((_, _, bits)) if bits & MAP != 0) && !notification_named(m, name);
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
                if MODULES.contains(&m.as_str()) && recv.bits != 0 && (recv.bits & MAP == 0 || notification_named(m, name)) {
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
            // `is_err(x)` false: `x` holds no error, after `if is_err(x) { return ... }` too
            // (FEEDBACK81.md, Q-1272; branch feedback72-81).
            Expr::Call(f, args, _) if !yes && args.len() == 1 && matches!(&**f, Expr::Ident(n, _) if n == "is_err") && !self.is_local("is_err") => {
                let Expr::Ident(n, _) = &args[0] else { return };
                let Some(v) = self.scopes.iter().rev().find_map(|s| s.get(n.as_str())).or_else(|| self.globals.get(n.as_str())) else { return };
                if !v.t.vague() && v.t.bits & ERR != 0 && v.t.bits & !ERR != 0 && !self.assigned.contains(n.as_str()) {
                    out.push((n.to_string(), v.t.without(ERR)));
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
        if m == "notification" {
            self.notification_rules(f, args, piped);
        } else if m == "http" && f != "stream" {
            self.http_rules(f, args, piped);
        } else if full == "http.stream" && self.in_component().is_some() {
            self.error("`http.stream` is the server's; in the browser, `receive(url, f, ...)` in a handler and `listen(url, f)` read a stream");
        } else if !BROWSER_MODULES.contains(&m) && full != "channel.open" {
            self.browser_forbidden(&format!("`{}`", full));
        }
        if m == "db" && !piped {
            self.sql_params(&full, args);
        }
        if m == "db" && matches!(f, "transaction" | "atomic") {
            self.sends_in_a_transaction(args);
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

    /// `notification`'s sides (FEEDBACK67.md, B.3; branch pwa): `send` and `user` are the
    /// server's, never a component's; `ask`, `state` and `stop` the browser's, from a handler (and
    /// `state` to set a state), and refused where the code surely runs on the server, as `ask` and
    /// `notify` are (Q-959).
    fn notification_rules(&mut self, f: &str, args: &[Expr], piped: bool) {
        // Options written in the call that are none, and a message written out with no title:
        // said before the program runs, as when it runs (as `http`'s options are).
        let (at, known): (usize, &[&str]) = match f {
            "ask" => (0, &["user"]),
            "send" => (1, crate::push::OPTIONS),
            _ => (usize::MAX, &[]),
        };
        if let (false, Some(Expr::Map(fields))) = (piped, args.get(at)) {
            for (k, _) in fields.iter().filter(|(k, _)| !known.contains(&k.as_str())) {
                let what = if f == "ask" { "the option is user".to_string() } else { format!("the options are {}", known.join(", ")) };
                self.error(format!("`notification.{}`: unknown option `{}` ({})", f, k, what));
            }
            if f == "send" && !fields.iter().any(|(k, _)| k == "title") {
                self.error("`notification.send` needs a `title`: the line the device shows first");
            }
        }
        match f {
            "send" | "user" => {
                if let Some(c) = self.in_component() {
                    self.error(format!("`notification.{}` is the server's, and <{}> also runs in the browser: call it from a `server fn`", f, c));
                }
            }
            "ask" | "state" | "stop" => {
                if let Some(place) = self.on_the_server() {
                    self.error(format!("`notification.{}` {} runs on the server; {}", f, place, crate::push::BROWSER_ONLY));
                } else if let Some(c) = self.in_component() {
                    let (nested, init) = self.fns.last().map_or((false, false), |x| (x.nested, x.state_init));
                    if (!nested && !init) || (init && f != "state") {
                        let or = if f == "state" { ", or set a state with it: state on = notification.state()" } else { "" };
                        self.error(format!("`notification.{}` in <{}> runs in the browser, and the server renders <{}> first: call it from an event handler (onclick){}", f, c, c, or));
                    }
                }
            }
            _ => {}
        }
    }

    /// `notification.send` written in the function `db.transaction` or `db.atomic` runs: it fails
    /// there, an error value, as waiting in one holds up every request (FEEDBACK67.md, B.3, review;
    /// branch pwa).
    fn sends_in_a_transaction(&mut self, args: &[Expr]) {
        if !self.builtin_in_scope("notification") {
            return;
        }
        let mut at = Vec::new();
        for a in args {
            if let Expr::Lambda(d) = a {
                walk_fn(d, &mut |e| {
                    if let Expr::Call(c, _, line) = e {
                        if matches!(&**c, Expr::Field(o, f, _) if f == "send" && matches!(&**o, Expr::Ident(n, _) if *n == "notification")) {
                            at.push(*line);
                        }
                    }
                });
            }
        }
        let line = self.line;
        for l in at {
            self.line = l;
            self.warn(crate::push::IN_A_TRANSACTION);
        }
        self.line = line;
    }

    /// A handler's `notification.ask()` after it has waited (a `server fn`, `http`, a service,
    /// `parallel`, `receive`, `markdown`, `notification.stop`): browsers show the prompt only close
    /// to the click, and WebKit only in it, so the ask is better first (FEEDBACK67.md, B.3; branch
    /// pwa). Followed as the code runs: a wait in one branch of an `if` or a `match` is not before
    /// an ask in another (review); a function written in the handler is its own.
    fn ask_after_wait(&mut self, d: &FnDef) {
        // A variable that hides the module is said of already.
        if !self.builtin_in_scope("notification") {
            return;
        }
        let server: HashSet<String> = self.bodies.iter().filter(|(_, b)| b.server && !b.component).map(|(n, _)| n.clone()).collect();
        let services: HashSet<String> = self.services.keys().cloned().collect();
        let mut late: Vec<(u32, String)> = Vec::new();
        let mut w = Waits { server: &server, services: &services, late: &mut late };
        w.expr(&d.body, None);
        for (line, w) in late {
            self.line = line;
            self.warn(format!("`notification.ask` comes after `{}` waits in this handler: browsers show the permission prompt only close to the click, so ask first, then wait", w));
        }
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

    /// What a `let` of `e` holds. A list or a map written out with built SQL in it, or text
    /// next to a value in a list, and a lambda that gives back built SQL, hold it too
    /// (FEEDBACK75.md, Q-1085; branch feedback75).
    fn sql_kind(&mut self, e: &Expr) -> SqlVar {
        let parts = match e {
            Expr::List(items) => {
                // Each name in it read once, not once for each place it is in: a list of 250,000 of
                // one variable looked the variable up 750,000 times (found in review).
                let mut names: HashMap<&str, (bool, bool, bool)> = HashMap::new();
                let mut kind = |x: &Expr| match x {
                    Expr::Ident(n, _) => *names.entry(n.as_str()).or_insert_with(|| (self.sql_built(x), self.sql_text(x), self.sql_safe(x))),
                    _ => (self.sql_built(x), self.sql_text(x), self.sql_safe(x)),
                };
                let kinds: Vec<(bool, bool, bool)> = items.iter().map(&mut kind).collect();
                let built: Vec<usize> = kinds.iter().enumerate().filter(|(_, k)| k.0).map(|(i, _)| i).collect();
                let joined = !built.is_empty() || (kinds.iter().any(|k| k.1) && kinds.iter().any(|k| !k.2));
                let many = built.len() > 64;
                joined.then(|| SqlParts { fields: Vec::new(), items: if many { Vec::new() } else { built }, joined, many })
            }
            Expr::Map(es) => {
                let fields: Vec<String> = es.iter().filter(|(_, x)| self.sql_built(x)).map(|(k, _)| k.clone()).collect();
                let many = fields.len() > 64;
                (!fields.is_empty()).then(|| SqlParts { fields: if many { Vec::new() } else { fields }, items: Vec::new(), joined: false, many })
            }
            _ => None,
        };
        if let Some(p) = parts {
            self.sql_parts.push(p);
            return SqlVar::Parts(self.sql_parts.len() as u32 - 1);
        }
        let lambda = matches!(e, Expr::Lambda(d) if self.sql_built(&d.body) || matches!(&d.body, Expr::Block(stmts) if stmts.iter().any(|s| matches!(s, Stmt::Return(Some(x), _) if self.sql_built(x)))));
        if lambda || self.sql_built(e) {
            SqlVar::Built
        } else if self.sql_safe(e) {
            SqlVar::Safe
        } else if is_connect(e) {
            SqlVar::Conn
        } else {
            SqlVar::Other
        }
    }

    /// A rule for every stream a name ending in `*` starts that never reads which stream is asked
    /// for, its second parameter (`(peer, name) =>`): a page it lets in for one stream may send into
    /// every one, or watch every one, as the guide's call did (FEEDBACK75.md, Q-1050, the user's
    /// decision of 2026-11-18; branch feedback75). A function the checker cannot read is not said.
    /// Only `media.on`'s: a channel's name holds no `*` (src/wt.rs `name_ok`).
    fn star_rules(&mut self, arg_exprs: &[Expr]) {
        let fname = "media.on";
        let Some(Expr::Lit(Lit::Str(name))) = arg_exprs.first() else { return };
        let Some(prefix) = name.strip_suffix('*') else { return };
        let Some(Expr::Map(es)) = arg_exprs.get(1) else { return };
        for (rule, does) in [("publish", "send into"), ("watch", "watch")] {
            let Some((_, f)) = es.iter().find(|(k, _)| k == rule) else { continue };
            let def = match f {
                Expr::Lambda(d) => Some(d.clone()),
                Expr::Ident(n, _) => self.scopes.iter().rev().find_map(|sc| sc.get(n.as_str())).or_else(|| self.globals.get(n.as_str())).and_then(|v| v.sig.as_ref()).and_then(|s| s.def.clone()),
                _ => None,
            };
            let Some(d) = def else { continue };
            // A rule that lets no one in needs no name (found in review).
            let nobody = |e: &Expr| matches!(e, Expr::Lit(Lit::Bool(false)));
            if nobody(&d.body) || matches!(&d.body, Expr::Block(stmts) if matches!(stmts.as_slice(), [Stmt::Expr(e) | Stmt::Return(Some(e), _)] if nobody(e))) {
                continue;
            }
            let reads = d.params.get(1).is_some_and(|p| {
                let mut seen = false;
                for code in fn_code(&d) {
                    walk_expr(code, &mut |x| seen |= matches!(x, Expr::Ident(n, _) if *n == p.name));
                }
                seen
            });
            if !reads {
                self.warn(format!(
                    "`{}(\"{}\")`'s `{}` never reads which stream is asked for: it is called as `(peer, name)`, and one that lets a page in for one `{}` stream lets it {} every one; check `name`, as `{}: (peer, name) => name == \"{}\" + session().user`",
                    fname, name, rule, prefix, does, rule, prefix
                ));
            }
        }
    }

    /// Whether a call of `n` gives back built SQL: a function of the program's that does, or a
    /// lambda a variable holds that does. A name a global function or a builtin has, and no local,
    /// is not looked for among the variables (FEEDBACK75.md, Q-1085; branch feedback75).
    /// Whether variable `n` holds built SQL here, looked for only where some variable of its name
    /// was given some (FEEDBACK75.md, Q-1085; branch feedback75).
    fn var_built(&self, n: &str) -> bool {
        self.sql_built_vars.contains(n) && self.sql_var(n) == SqlVar::Built
    }

    fn calls_built(&self, n: &str, args: &[Expr]) -> bool {
        let given = |a: &Expr| self.sql_safe(a) || matches!(a, Expr::List(items) if items.iter().all(|x| self.sql_safe(x)));
        match self.sql_built_fns.get(n) {
            Some(always) if !self.is_local(n) => *always || !args.iter().all(given),
            _ => self.var_built(n),
        }
    }

    /// The parts of a map or list variable `n` that hold built SQL, if it holds some.
    fn parts_of(&self, n: &str) -> Option<&SqlParts> {
        if !self.sql_built_vars.contains(n) {
            return None;
        }
        match self.sql_var(n) {
            SqlVar::Parts(i) => self.sql_parts.get(i as usize),
            _ => None,
        }
    }

    /// Whether the program's function `d` gives back SQL built from values: what it returns,
    /// with its `let`s read as the checker reads them, its parameters none of the program's
    /// texts (FEEDBACK75.md, Q-1085; branch feedback75).
    /// What `d` gives back: None if never built SQL, Some(false) if built from what it is given
    /// (`notes_of(owner)`), Some(true) if built whatever it is given. Its code is read in order, its
    /// `let`s and assignments as the checker reads them (`s = s + n`, in a loop too), and each
    /// `return`, inside an `if` too; not the lambdas in it (FEEDBACK75.md, Q-1085; branch
    /// feedback75).
    fn gives_sql_built(&mut self, d: &FnDef) -> Option<bool> {
        let read = |c: &mut Self, safe: bool| {
            let mark = c.sql_vars.len();
            // The parts a read makes are its own: let go of after it (found in review).
            let parts = c.sql_parts.len();
            let given = if safe { SqlVar::Safe } else { SqlVar::Other };
            c.sql_vars.extend(d.params.iter().map(|p| (usize::MAX, p.name.to_string(), given)));
            let was = std::mem::replace(&mut c.sql_pass_safe, safe);
            let mut built = false;
            match &d.body {
                Expr::Block(stmts) => c.sql_flow(stmts, true, &mut built, 0),
                e => built = c.sql_built(e),
            }
            c.sql_pass_safe = was;
            c.sql_vars.truncate(mark);
            c.sql_parts.truncate(parts);
            built
        };
        if !read(self, false) {
            return None;
        }
        Some(read(self, true))
    }

    /// `gives_sql_built`'s reading of a block: `last` when its last expression is the function's
    /// result. The stack grows every 8th block, as `Returns::deep` grows it.
    fn sql_flow(&mut self, stmts: &[Stmt], last: bool, built: &mut bool, depth: u32) {
        let mark = self.sql_vars.len();
        for (i, st) in stmts.iter().enumerate() {
            // A confined check's clock, read every 256th statement (FEEDBACK75.md, Q-1053, found
            // in review; branch feedback75).
            self.steps = self.steps.wrapping_add(1);
            if self.steps % 256 == 0 && DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d) {
                OVER_BUDGET.with(|o| o.set(true));
            }
            if OVER_BUDGET.with(|o| o.get()) {
                break;
            }
            match st {
                Stmt::Let(p, x, _) => {
                    self.sql_flow_in(x, built, depth);
                    let k = if matches!(p, Pattern::Bind(_)) { self.sql_kind(x) } else { SqlVar::Other };
                    let mut names = Vec::new();
                    pattern_names(p, &mut names);
                    for n in names {
                        if matches!(k, SqlVar::Built | SqlVar::Parts(_)) {
                            self.sql_built_vars.insert(n.clone());
                        }
                        self.sql_vars.push((usize::MAX, n, k));
                    }
                }
                Stmt::Assign(n, op, x, _) => {
                    self.sql_flow_in(x, built, depth);
                    let was = self.sql_var(n);
                    let now = match op {
                        AssignOp::Set => Some(self.sql_kind(x)),
                        AssignOp::Add if matches!(was, SqlVar::Safe | SqlVar::Built) && !self.sql_safe(x) => Some(SqlVar::Built),
                        _ => None,
                    };
                    if let Some(k) = now {
                        if matches!(k, SqlVar::Built | SqlVar::Parts(_)) {
                            self.sql_built_vars.insert(n.to_string());
                        }
                        if let Some(v) = self.sql_vars.iter_mut().rev().find(|(_, m, _)| m == n.as_str()) {
                            v.2 = k;
                        }
                    }
                }
                Stmt::Return(Some(x), _) => *built |= self.sql_built(x),
                Stmt::Expr(x) => {
                    if last && i + 1 == stmts.len() {
                        *built |= self.sql_built(x);
                    }
                    self.sql_flow_in(x, built, depth);
                }
                _ => {}
            }
        }
        self.sql_vars.truncate(mark);
    }

    /// The blocks inside `e` that run where it does (a choice's, a loop's), for `sql_flow`; a loop
    /// over SQL-safe values gives SQL-safe values. Not a lambda's. A branch that is one expression
    /// is read as it is, not copied into a statement: 200 `match` arms one inside another, around a
    /// list of 200,000, held a copy at each, 2.6 GB (FEEDBACK75.md, Q-1085, found in review; branch
    /// feedback75).
    fn sql_flow_in(&mut self, e: &Expr, built: &mut bool, depth: u32) {
        match e {
            Expr::Block(_) => self.sql_flow_body(e, built, depth),
            Expr::If(_, a, b) => {
                self.sql_flow_body(a, built, depth);
                if let Some(b) = b {
                    self.sql_flow_body(b, built, depth);
                }
            }
            Expr::For(p, it, body, _) => {
                let k = if self.sql_safe(it) || matches!(&**it, Expr::List(items) if items.iter().all(|x| self.sql_safe(x))) { SqlVar::Safe } else { SqlVar::Other };
                let mark = self.sql_vars.len();
                let mut names = Vec::new();
                pattern_names(p, &mut names);
                self.sql_vars.extend(names.into_iter().map(|n| (usize::MAX, n, k)));
                self.sql_flow_body(body, built, depth);
                self.sql_vars.truncate(mark);
            }
            Expr::While(_, body, _) => self.sql_flow_body(body, built, depth),
            Expr::Match(_, arms, _) => {
                for a in arms {
                    let mark = self.sql_vars.len();
                    let mut names = Vec::new();
                    pattern_names(&a.pat, &mut names);
                    self.sql_vars.extend(names.into_iter().map(|n| (usize::MAX, n, SqlVar::Other)));
                    self.sql_flow_body(&a.body, built, depth);
                    self.sql_vars.truncate(mark);
                }
            }
            _ => {}
        }
    }

    /// A branch's code for `sql_flow`: a block's statements, or the blocks inside one expression;
    /// the stack grown every 8th level, as `Returns::deep` grows it.
    fn sql_flow_body(&mut self, x: &Expr, built: &mut bool, depth: u32) {
        let mut go = |c: &mut Self| match x {
            Expr::Block(stmts) => c.sql_flow(stmts, false, built, depth + 1),
            other => c.sql_flow_in(other, built, depth + 1),
        };
        if depth % 8 == 7 {
            crate::stack::maybe_grow(256 << 10, 4 << 20, || go(self))
        } else {
            go(self)
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
        // Text that `str`, `trim`, `lower`, `upper` and `replace` give back is what they were
        // given, and `replace` puts its third into it (FEEDBACK75.md, Q-1085; branch feedback75).
        let passes = |f: &str| matches!(f, "str" | "trim" | "lower" | "upper" | "replace");
        let replaced = |text: &Expr, with: Option<&Expr>| self.sql_built(text) || (self.sql_text(text) && with.is_some_and(|w| !self.sql_safe(w)));
        match e {
            Expr::Ident(n, _) => self.var_built(n),
            Expr::Template(parts) => interpolates(parts),
            Expr::Call(f, _, _) if self.sql_safe_call(f) => false,
            // A function of the program's that gives back built SQL, whatever it is given or from a
            // value it is given, and a lambda a variable holds that does (FEEDBACK75.md, Q-1085;
            // branch feedback75).
            Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if self.calls_built(n, args)) => true,
            Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if passes(n) && self.builtin_in_scope(n)) => match args.first() {
                Some(text) => replaced(text, if self.builtin_named(f, "replace") { args.get(2) } else { None }),
                None => false,
            },
            Expr::Call(f, args, _) if matches!(&**f, Expr::Field(_, n, _) if passes(n)) => match &**f {
                Expr::Field(text, n, _) => replaced(text, if n == "replace" { args.get(1) } else { None }),
                _ => false,
            },
            Expr::Call(f, args, _) => match args.as_slice() {
                [Expr::Template(parts)] => interpolates(parts),
                [Expr::List(items), rest @ ..] if is_join(f) => items.iter().chain(rest).any(|x| !self.sql_safe(x)),
                // A list a variable holds, built from values (FEEDBACK75.md, Q-1085; branch
                // feedback75).
                [Expr::Ident(n, _), ..] if is_join(f) => self.parts_of(n).is_some_and(|p| p.joined),
                _ => false,
            },
            // The field or item of a map or list a variable holds that is built SQL, and only
            // that one (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
            Expr::Field(o, k, _) => matches!(&**o, Expr::Ident(n, _) if self.parts_of(n).is_some_and(|p| p.field(k))),
            Expr::Index(o, k, _) => match (&**o, &**k) {
                (Expr::Ident(n, _), Expr::Lit(Lit::Str(k))) => self.parts_of(n).is_some_and(|p| p.field(k)),
                (Expr::Ident(n, _), Expr::Lit(Lit::Int(i))) => self.parts_of(n).is_some_and(|p| usize::try_from(*i).is_ok_and(|i| p.item(i))),
                (Expr::Ident(n, _), _) => self.parts_of(n).is_some_and(|p| p.many || !p.items.is_empty() || !p.fields.is_empty()),
                _ => false,
            },
            Expr::Pipe(lhs, rhs, _) => match (&**lhs, &**rhs) {
                (Expr::List(items), Expr::Call(f, rest, _)) if is_join(f) => items.iter().chain(rest).any(|x| !self.sql_safe(x)),
                (Expr::List(items), f) if is_join(f) => items.iter().any(|x| !self.sql_safe(x)),
                (Expr::Ident(n, _), Expr::Call(f, _, _)) if is_join(f) => self.parts_of(n).is_some_and(|p| p.joined),
                (Expr::Ident(n, _), f) if is_join(f) => self.parts_of(n).is_some_and(|p| p.joined),
                (text, Expr::Call(f, rest, _)) if matches!(&**f, Expr::Ident(n, _) if passes(n) && self.builtin_in_scope(n)) => {
                    replaced(text, if self.builtin_named(f, "replace") { rest.get(1) } else { None })
                }
                (text, f @ Expr::Ident(n, _)) if passes(n) && self.builtin_in_scope(n) => {
                    let _ = f;
                    self.sql_built(text)
                }
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
            // Read as given safe values (`gives_sql_built`), `str(id)` of a safe `id` is safe
            // (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
            Expr::Call(f, args, _) if self.sql_pass_safe && matches!(&**f, Expr::Ident(n, _) if matches!(n.as_str(), "str" | "trim" | "lower" | "upper")) => {
                args.len() == 1 && self.sql_safe(&args[0])
            }
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
        // A builtin whose docs write its last parameter `args...` takes the rest one by one:
        // `live(load, ...args)`, as `live(load, args)` read as a list (FEEDBACK73.md, Q-1024;
        // branch feedback72-81).
        let spread_last = sig.builtin && sig.variadic && crate::builtin_docs::DOCS.iter().any(|(n, p, _)| *n == name && p.trim_end().ends_with("..."));
        let shape = || {
            let last = sig.params.len().saturating_sub(1);
            let ps: Vec<String> = sig.params.iter().enumerate().map(|(i, p)| if spread_last && i == last { format!("...{}", p.name) } else { p.name.clone() }).collect();
            format!("{}({})", name, ps.join(", "))
        };
        let too_many = args.len() > n && !sig.variadic;
        if too_many {
            // `max(1, 2)`: one list, said once, not also as argument 1's type.
            if sig.builtin && n == 1 && sig.params[0].ty & !NIL == LIST && args.first().is_some_and(|a| a.bits != 0 && a.bits & LIST == 0) {
                self.error(format!("`{}` takes one list: put the items in it, `{}([...])`", name, name));
                return sig.ret.unwrap_or(T::ANY);
            }
            if sig.builtin && sig.name == "ask" {
                // A page's dialog takes no default: a tool's question with choices is `about`,
                // here as in a page and as it runs (FEEDBACK56.md, the user's decision 2026-11-09;
                // branch feedback56).
                self.error(crate::mcp::ASK_OF_THREE);
            } else {
                let shown = if sig.builtin { format!(": {}", shape()) } else { String::new() };
                self.error(format!("`{}` takes {} argument{}, got {}{}{}", name, n, if n == 1 { "" } else { "s" }, args.len(), shown, hides(&sig, name)));
            }
        }
        // `set(xs, [0], v)`: on a list the index is the item's place, an int, as it runs; a map
        // takes any key (an int as text, Q-019). Argument 2's type follows argument 1's, which the
        // one type a parameter has cannot say (FEEDBACK64.md, Q-932; branch feedback64).
        if sig.builtin && name == "set" {
            if let (Some(on), Some(at)) = (args.first(), args.get(1)) {
                // A list only: on null `set` makes a map, which takes the key (found in review).
                if on.bits == LIST && at.bits != 0 && at.bits & INT == 0 {
                    self.error(format!("argument 2 of `set` on a list must be int, got {}: the item's place, `xs = set(xs, 0, v)`", bits_text(at.bits)));
                }
            }
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
        if sig.builtin && sig.name == "media.on" {
            self.star_rules(arg_exprs);
        }
        if sig.builtin && matches!(sig.name.as_str(), "db.transaction" | "idempotent") {
            self.retried_effects(name, arg_exprs);
            if sig.name == "idempotent" {
                self.idempotent_site(arg_exprs);
            }
            // What a key's `user` cannot be, where it shows (FEEDBACK55.md, Q-887; branch feedback55).
            if sig.name == "idempotent" {
                if let [Expr::Map(opts), _, ..] = arg_exprs {
                    for (k, v) in opts {
                        match v {
                            Expr::Map(_) if k == "user" => self.error(crate::idempotency::user_not_a_name("a map")),
                            Expr::List(_) if k == "user" => self.error(crate::idempotency::user_not_a_name("a list")),
                            _ => {}
                        }
                    }
                }
            }
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
                    "`{}` waits on the server, so <{}> can only call it from an event handler (onclick={{() => {{ ... }}}}) or to set state: state x = {}(...)",
                    name, c, name
                ));
            }
        }
        // The server: `ask` and `notify` there fail every call, where the checker knows the side
        // (FEEDBACK67.md, Q-959; branch pwa). One error a call: given too many, that is said; in a
        // tool's own code, `ask` is said of the tool (ASK_IN_A_TOOL).
        if sig.builtin && matches!(sig.name.as_str(), "ask" | "notify") && !too_many {
            let tool = self.fns.first().and_then(|f| self.bodies.get(&f.name)).is_some_and(|d| d.mcp);
            if let Some(place) = self.on_the_server().filter(|_| !(tool && sig.name == "ask")) {
                self.error(format!("`{}` {} runs on the server; {}", sig.name, place, crate::builtins::IN_THE_BROWSER));
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
            let event = if n == "checked" { "onchange" } else { "oninput" };
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
            self.markup_habits(el);
        }
        // Where a link goes (FEEDBACK79.md, Q-1251; branch feedback72-81): an element's `href`, not a
        // view's or component's prop of that name, which may be anything (found in review).
        for (n, v) in el.attrs.iter().filter(|_| !is_view && !el.tag.eq_ignore_ascii_case("base")) {
            if let (true, AttrVal::Expr(x)) = (n.eq_ignore_ascii_case("href"), v) {
                if let Some((t, whole)) = link_start(x) {
                    self.page_links.push((self.file.clone(), el.line, t, whole, "link"));
                }
            }
        }
        let mut given: Vec<(String, T)> = Vec::new();
        let mut spread = false;
        for (name, v) in &el.attrs {
            let t = match v {
                AttrVal::Bool => T::of(BOOL),
                // A handler, and what a view or component is given (a function it may call in
                // the browser), are not said to run on the server (FEEDBACK67.md, Q-959; branch
                // pwa).
                AttrVal::Expr(e) if is_view || name.starts_with('@') => {
                    if let (true, false, Expr::Lambda(d)) = (name.starts_with('@'), is_view, e) {
                        self.handler_marks(d);
                    }
                    self.browser_too += 1;
                    let t = self.expr(e);
                    self.browser_too -= 1;
                    if let (true, Expr::Lambda(d)) = (name.starts_with('@'), e) {
                        self.ask_after_wait(d);
                    }
                    t
                }
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
                // And a function given by name, an error dropped, and what the last statement ends
                // with (FEEDBACK80.md, Q-1267, and FEEDBACK72.md, Q-1018; branch feedback72-81).
                if !is_view {
                    self.handler_dropped(v, ev);
                }
                if is_view {
                    self.error(format!("<{}> is a view; `@{}` handlers go on HTML elements", el.tag, ev));
                } else if t.bits != 0 && t.bits & FN == 0 {
                    // `onclick={text}` is read as this too: say what a handler is.
                    // Alpine's `@click.prevent="..."`: one message, the modifier's (FEEDBACK72.md,
                    // Q-1031, found in review; branch feedback72-81).
                    match ev.split_once('.').and_then(|(b, m)| modifier_advice(&format!("on{}", b), m)) {
                        Some(how) => self.error(format!("`@{}` never fires: {}", ev, how)),
                        None => self.error(format!("`@{}` needs a function, got {}: a handler is a function, `on{}={{() => ...}}`; text is never run as code", ev, bits_text(t.bits), ev)),
                    }
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
        // A table that takes focus and keys, a dialog that hears Escape: a click inside it has the
        // keyboard's way already (Q-1019, found in review).
        let keys = !is_view && el.attrs.iter().any(|(a, v)| {
            matches!(a.as_str(), "@keydown" | "@keyup" | "@keypress" | "tabindex")
                || (a == "role" && !matches!(v, AttrVal::Expr(Expr::Lit(Lit::Str(r))) if &**r == "presentation" || &**r == "none"))
        });
        if keys {
            self.keys_above += 1;
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
                        let later = runs_later(x);
                        walk_expr(x, &mut |y| {
                            if let Expr::Ident(n, _) = y {
                                if !later.contains(&(y as *const Expr)) {
                                    shown.push(n.clone());
                                }
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
                    // An error value shown as it is, `{news}` with `state news = load_news()` and
                    // `load_news` declared `! str`: nothing a visitor should read. A fallback
                    // (`??`), `?`, and an `if` or a `match` that tests it take the error out of its
                    // type, or name it (FEEDBACK81.md, Q-1272; branch feedback72-81).
                    if t.bits & ERR != 0 && !t.vague() && self.shows_failure(x) {
                        self.line = el.line;
                        let what = match x {
                            Expr::Call(f, _, _) => dotted(f).map_or_else(|| "...".to_string(), |f| format!("{}(...)", f)),
                            e => written(e),
                        };
                        let test = if matches!(x, Expr::Ident(..)) { format!("is_err({})", what) } else { "is_err(...)".to_string() };
                        let what = if what == "..." { "this".to_string() } else { format!("`{}`", what) };
                        self.warn(format!("{} can be an error value here, and the page would show nothing for it: give a fallback with `??`, or test `{}`", what, test));
                    }
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
        if keys {
            self.keys_above -= 1;
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

    // ---- markup, links and habits (FEEDBACK72.md, FEEDBACK73.md and FEEDBACK79.md; branch
    // feedback72-81) --------------------------------------------------------------------------

    /// What an element's attributes say of a habit from elsewhere, each with what to write: Vue's
    /// modifiers, `v-model` and `v-on:` (FEEDBACK72.md, Q-1031), a click on an element the keyboard
    /// never reaches and an image with no `alt` (Q-1019), and CSS put together from values with `+`
    /// or a template (Q-1017; branch feedback72-81).
    fn markup_habits(&mut self, el: &HtmlEl) {
        let has = |n: &str| el.attrs.iter().any(|(a, _)| a.eq_ignore_ascii_case(n));
        let spread = el.attrs.iter().any(|(_, v)| matches!(v, AttrVal::Spread(_)));
        for (name, v) in &el.attrs {
            self.line = el.line;
            // A function given; text is refused where the handler is checked, with this advice.
            let code = matches!(v, AttrVal::Expr(e) if !matches!(e, Expr::Lit(_) | Expr::Template(_)));
            if let (Some((ev, mods)), true) = (name.strip_prefix('@').and_then(|e| e.split_once('.')), code) {
                if let Some(how) = modifier_advice(&format!("on{}", ev), mods) {
                    self.warn(format!("`{}` never fires: {}", name, how));
                }
            } else if name == "v-model" && code {
                // `v-model="msg"`, text, is petite-vue's or Alpine's attribute, read by that
                // library; braces are Quillo's (found in review).
                let shown = match v {
                    AttrVal::Expr(Expr::Ident(n, _)) => n.to_string(),
                    _ => "...".to_string(),
                };
                let checkbox = el.tag == "input"
                    && el.attrs.iter().any(|(a, t)| a == "type" && matches!(t, AttrVal::Expr(Expr::Lit(Lit::Str(s))) if s.eq_ignore_ascii_case("checkbox")));
                self.warn(format!("`v-model` is Vue's: write `{}={{{}}}`, which is two-way by itself", if checkbox { "checked" } else { "value" }, shown));
            } else if let (Some(ev), true) = (name.strip_prefix("v-on:"), code) {
                self.warn(format!("`{}` is Vue's: write `on{}={{...}}`", name, ev.split('.').next().unwrap_or(ev)));
            } else if name == "style" {
                // A template string's values are written as CSS values; text joined with `+` is
                // one string, which nothing can take apart.
                if let AttrVal::Expr(x @ Expr::Binary(BinOp::Add, ..)) = v {
                    if self.css_from_values(x) {
                        self.warn("`style` built from values with `+`: a value containing `;` can add declarations (CSS injection); write a style map, `style={{ color: c }}`, whose values are written as CSS values");
                    }
                }
            }
        }
        self.line = el.line;
        // What a click alone reaches: a mouse, not the keyboard, nor a screen reader's list of what
        // can be pressed.
        const STILL: &[&str] = &[
            "div", "span", "p", "li", "td", "th", "tr", "img", "section", "article", "header", "footer", "main", "aside", "nav", "ul", "ol",
            "dl", "dt", "dd", "h1", "h2", "h3", "h4", "h5", "h6", "figure", "figcaption", "strong", "em", "b", "i", "small", "pre", "code",
            "blockquote", "table", "tbody", "thead", "tfoot",
        ];
        // Not a click on what is around the content, read by its `e.target` (a backdrop, a list
        // handling its items' clicks, a box focusing its field), nor an element hidden from a
        // screen reader or with a key handler of its own: in the website's kit 12 of 17 were such.
        // A handler that takes the event (`(e) => pick(e)`, `pick` declared `fn pick(e)`) handles
        // what was clicked inside, a list's buttons say: the keyboard reaches those (found in
        // review).
        let takes_event = |me: &Self, v: &AttrVal| match v {
            AttrVal::Expr(Expr::Lambda(d)) => !d.params.is_empty(),
            AttrVal::Expr(Expr::Ident(n, _)) => {
                let var = me.scopes.iter().rev().find_map(|s| s.get(n.as_str())).or_else(|| me.globals.get(n.as_str()));
                var.and_then(|v| v.sig.as_ref()).is_some_and(|s| !s.params.is_empty())
            }
            _ => false,
        };
        let clicked = el.attrs.iter().find(|(a, v)| a == "@click" && matches!(v, AttrVal::Expr(_)));
        let still = || ["role", "tabindex", "contenteditable", "aria-hidden", "@keydown", "@keyup", "@keypress"].iter().all(|a| !has(a));
        if let Some((_, v)) = clicked {
            if !spread && self.keys_above == 0 && STILL.contains(&el.tag.as_str()) && still() && !takes_event(self, v) && !reachable_inside(el) {
                let an = if el.tag.starts_with(['a', 'e', 'i', 'o']) || (el.tag.len() == 2 && el.tag.starts_with('h')) { "an" } else { "a" };
                let msg = format!("{} `<{}>` with onclick can't be reached with the keyboard: use a `<button>`", an, el.tag);
                // Said once the page's handlers are known: one in markup the server alone makes
                // never runs, and that is all there is to say of it (found in review).
                if self.quiet == 0 {
                    self.unreachable.push((self.file.clone(), el.line, msg));
                }
            }
        }
        if el.tag == "img" && !spread && !["alt", "role", "aria-label", "aria-labelledby", "aria-hidden", "hidden"].iter().any(|a| has(a)) {
            self.warn("this `<img>` has no `alt`: `alt=\"\"` if it is decoration, or words for what it shows");
        }
        // `<style>{`p { color: ${c} }`}</style>`: the SQL warning's twin.
        if el.tag.eq_ignore_ascii_case("style") {
            for c in &el.children {
                if let HtmlChild::Expr(x @ (Expr::Template(_) | Expr::Binary(BinOp::Add, ..))) = c {
                    if self.css_from_values(x) {
                        self.warn("`<style>` with CSS built from values: a value containing `}` can add rules (CSS injection); write `` css`...` ``, which writes each `${...}` as one CSS value");
                    }
                }
            }
        }
    }

    /// Whether CSS put together with `+` or a template holds a value that may be anyone's text: not
    /// a literal, a `` css`...` `` (whose values it writes as CSS values) or a name holding one, text
    /// written in the program, or a number (FEEDBACK72.md, Q-1017; branch feedback72-81). Read with
    /// a list of what is left, not by recursion, so a long chain of `+` takes no stack.
    fn css_from_values(&mut self, e: &Expr) -> bool {
        let mut todo = vec![e];
        while let Some(x) = todo.pop() {
            match x {
                Expr::Lit(_) => {}
                Expr::Template(parts) => todo.extend(parts.iter().filter_map(|p| if let TPart::Expr(y) = p { Some(y) } else { None })),
                Expr::Binary(BinOp::Add, a, b, _) => {
                    todo.push(a);
                    todo.push(b);
                }
                Expr::Call(f, _, _) if self.builtin_named(f, "css") => {}
                // A number's text: `str(pct)`, `format_number(pct)` (found in review).
                Expr::Call(f, args, _)
                    if (self.builtin_named(f, "str") || self.builtin_named(f, "format_number"))
                        && args.first().is_some_and(|a| {
                            let t = self.quiet_expr_type(a);
                            t.bits != 0 && t.bits & !(INT | FLOAT | DEC) == 0
                        }) => {}
                Expr::Ident(n, _) if !self.is_local(n) && self.css_texts.contains(n.as_str()) => {}
                _ if self.url_fixed(x) => {}
                _ => {
                    let t = self.quiet_expr_type(x);
                    if t.bits == 0 || t.bits & !(INT | FLOAT | DEC | BOOL | NIL) != 0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A link, `redirect` or `navigate` to an address of the program's that no route answers: a 404
    /// found only when someone follows it, where Dioxus's build fails (FEEDBACK79.md, Q-1251; branch
    /// feedback72-81). Only what is written is read: a literal, or the literal start of a template
    /// or of a `+`, from `/` (not `//`), its `?query` and `#hash` left out; a `mount`'s or a
    /// `service`'s prefix takes what is under it, and a route ending in `*rest` all below it. Not in
    /// a program checked inside another (a mounted one's addresses are under its prefix), nor in one
    /// with no routes (a file of views for other programs).
    fn dead_links<'a>(&mut self, order: &'a [Item]) {
        if DEPTH.with(|d| d.get()) > 1 {
            return;
        }
        let mut routes: Vec<&'a RouteDef> = Vec::new();
        let mut prefixes: Vec<Vec<String>> = Vec::new();
        for it in order {
            match &it.stmt {
                Stmt::Route(r) if r.method != crate::ast::GUARD_METHOD => routes.push(r),
                Stmt::Mount(p, _, _) | Stmt::Service(_, Some(p), _, _, _, _) => prefixes.push(p.split('/').filter(|s| !s.is_empty()).map(String::from).collect()),
                _ => {}
            }
        }
        if routes.is_empty() {
            return;
        }
        // The routes as a tree of their pieces, so a link meets only the routes that can take it
        // (every link against every route made 3,000 routes and 9,000 links 39% slower to check,
        // and routes that share a first piece, `/app/...`, or start with one, `/:lang/...`, 135%;
        // found in reviews). And by their first piece and how many pieces they have, for "did you
        // mean".
        let tree = LinkTree::of(&routes);
        let mut scratch: Vec<(usize, usize, bool)> = Vec::new();
        // By the first piece, by how many pieces and the first letter, and by how many: made at the
        // first dead link, for its "did you mean".
        type Near<'r> = (HashMap<&'r str, Vec<usize>>, HashMap<(usize, Option<char>), Vec<usize>>, HashMap<usize, Vec<usize>>);
        let near_of = |routes: &[&'a RouteDef]| -> Near<'a> {
            let (mut by_first, mut by_shape, mut by_len): Near = Default::default();
            for (i, r) in routes.iter().enumerate() {
                let initial = match r.path.first() {
                    Some(Seg::Lit(l)) => {
                        by_first.entry(l.as_str()).or_default().push(i);
                        l.chars().next()
                    }
                    _ => None,
                };
                let len = r.path.iter().position(|s| matches!(s, Seg::Rest(_))).unwrap_or(r.path.len());
                by_shape.entry((len, initial)).or_default().push(i);
                by_len.entry(len).or_default().push(i);
            }
            (by_first, by_shape, by_len)
        };
        let mut near_index: Option<Near> = None;
        // What the suggestions may cost: routes looked at for one dead link, and edit distances in
        // all; past them, a dead link is said with none (found in review).
        const LOOKED_AT: usize = 64;
        let mut budget: u32 = 5_000;
        // As the walk met them; one met twice is said once (`diag_at`).
        for (file, line, text, end, what) in std::mem::take(&mut self.page_links) {
            if !text.starts_with('/') || text.starts_with("//") || text.starts_with("/\\") {
                continue;
            }
            let (path, end) = match text.find(['?', '#']) {
                Some(i) => (&text[..i], LinkEnd::Whole),
                None => (text.as_str(), end),
            };
            let (whole, last) = (end == LinkEnd::Whole, end == LinkEnd::Last);
            if path == "/_quillo" || path.starts_with("/_quillo/") || path == "/manifest.webmanifest" || path == "/apple-touch-icon.png" {
                continue;
            }
            // Pieces as the browser asks for them: `.` and `..` (`%2e` too) taken out, as it does
            // (found in review).
            let (head, partial) = match (whole, path.rsplit_once('/')) {
                (false, Some((h, p))) => (h, Some(std::borrow::Cow::Borrowed(p))),
                _ => (path, None),
            };
            let mut pieces: Vec<std::borrow::Cow<str>> = Vec::new();
            for s in head.split('/').skip(1) {
                if !s.contains(['.', '%']) {
                    pieces.push(std::borrow::Cow::Borrowed(s));
                    continue;
                }
                match s.to_ascii_lowercase().replace("%2e", ".").as_str() {
                    "." => {}
                    ".." => {
                        pieces.pop();
                    }
                    _ if s.contains('%') => pieces.push(std::borrow::Cow::Owned(crate::server::percent_decode(s, false))),
                    _ => pieces.push(std::borrow::Cow::Borrowed(s)),
                }
            }
            pieces.retain(|s| !s.is_empty());
            // A program with no `/` of its own is part of a site whose home is elsewhere (behind a
            // proxy, or a fragment such as a guard's `redirect("/")`): its home is not said.
            if pieces.is_empty() && partial.is_none() {
                continue;
            }
            if prefixes.iter().any(|p| pieces.len() >= p.len() && pieces[..p.len()] == p[..]) {
                continue;
            }
            // A 307 or 308 keeps the method, so any route of the path takes it (found in review).
            let gets = |r: &RouteDef| what == "redirect-same" || r.method == "GET" || r.method == "ANY";
            let mut other: Option<&RouteDef> = None;
            let taken = tree.find(&pieces, partial.as_deref(), last, &mut scratch, &mut |i| {
                let r = routes[i];
                // The tree matched a whole address piece by piece already.
                if partial.is_some() && !link_fits(&r.path, &pieces, partial.as_deref(), last) {
                    return false;
                }
                if gets(r) {
                    return true;
                }
                other = other.or(Some(r));
                false
            });
            if taken {
                continue;
            }
            let shown = if whole { path.to_string() } else { format!("{}…", path) };
            let asks = match what {
                "link" => "a link asks with GET",
                "redirect" => "a redirect is followed with GET",
                _ => "`navigate` asks with GET",
            };
            if let Some(r) = other {
                self.diag_at(file, line, Severity::Warning, format!("only `{} {}` answers `{}`, and {}, so it gets a 405", r.method, route_path(r), shown, asks));
                continue;
            }
            let most = (path.chars().count() / 4).clamp(1, 3);
            let written = path.chars().filter(|c| *c != '/').count();
            // Routes of about as many pieces: first those of the same first piece, then of the same
            // first letter, then the rest, as far as the limits go.
            let n = pieces.len();
            let lens = if whole { n.saturating_sub(1)..=n + 1 } else { n..=n + 2 };
            let (by_first, by_shape, by_len) = near_index.get_or_insert_with(|| near_of(&routes));
            let first_len = pieces.first().map(|p| p.chars().count());
            let initial = pieces.first().and_then(|p| p.chars().next());
            let same_first = pieces.first().and_then(|f| by_first.get(&**f)).into_iter().flatten();
            let same_initial = lens.clone().flat_map(|len| by_shape.get(&(len, initial)).into_iter().flatten());
            let rest = lens.clone().flat_map(|len| by_len.get(&len).into_iter().flatten());
            let mut near: Option<(usize, usize, &RouteDef)> = None;
            let mut looked: Vec<usize> = Vec::new();
            for i in same_first.chain(same_initial).chain(rest) {
                if budget == 0 || looked.len() >= LOOKED_AT {
                    break;
                }
                if looked.contains(i) {
                    continue;
                }
                looked.push(*i);
                {
                    let r = routes[*i];
                    if !gets(r) {
                        continue;
                    }
                    if let (Some(Seg::Lit(l)), Some(fl)) = (r.path.first(), first_len) {
                        if l.chars().count().abs_diff(fl) > most {
                            continue;
                        }
                    }
                    budget -= 1;
                    // Not one that keeps nothing of what was written (`/@` for `/`).
                    if let Some(d) = link_distance(&r.path, &pieces, partial.as_deref()).filter(|d| *d <= most && *d < written) {
                        // The nearest; of those as near, the first written.
                        if near.is_none_or(|(b, j, _)| (d, *i) < (b, j)) {
                            near = Some((d, *i, r));
                        }
                    }
                }
            }
            let near = near
                .map(|(_, _, r)| {
                    // The link's own piece where a parameter takes it, the parameter as written
                    // where it does not, and `…` for what the template gives.
                    let mut s = String::new();
                    for (i, seg) in r.path.iter().enumerate() {
                        s.push('/');
                        match (seg, pieces.get(i)) {
                            (Seg::Lit(l), _) => s.push_str(l),
                            (Seg::Param(n, ty, prefix), Some(p)) => {
                                let p: &str = p;
                                let v = p.strip_prefix(prefix.as_str()).unwrap_or(p);
                                if v.is_empty() || ty.as_ref().is_some_and(|t| crate::interp::coerce(t, v).is_none()) {
                                    s.push_str(&format!("{}:{}{}", prefix, n, ty.as_ref().map_or(String::new(), |t| format!("({})", t))));
                                } else {
                                    s.push_str(&format!("{}{}", prefix, v));
                                }
                            }
                            _ => s.push('…'),
                        }
                    }
                    if s.is_empty() {
                        s.push('/');
                    }
                    s
                })
                .filter(|s| *s != shown)
                .map(|s| format!(" (did you mean `{}`?)", s))
                .unwrap_or_default();
            let ends = match what {
                "link" => "this link is a 404",
                "navigate" => "this `navigate` ends at a 404",
                _ => "this redirect ends at a 404",
            };
            self.diag_at(file, line, Severity::Warning, format!("no route answers `{}`, so {}{}", shown, ends, near));
        }
    }

    /// An event handler in a view a page places outside any component, itself or through the
    /// functions and views that make the page's markup: the server writes the bare element, and only
    /// a component's code reaches the browser, so the handler never runs (FEEDBACK73.md, Q-1020;
    /// branch feedback72-81). A view placed both in a page and in a component is said of for the
    /// page. Each function is read once, with a list of what is left to read. A route's own markup,
    /// and a plain function's that a page places so, lose their handlers the same way (decided in
    /// review).
    fn page_handlers(&mut self, order: &[Item]) {
        let components: HashSet<String> = self.globals.iter().filter(|(_, v)| v.sig.as_ref().is_some_and(|s| s.component)).map(|(n, _)| n.clone()).collect();
        // A function or view reached, with where the page first reached it.
        let mut todo: Vec<(String, Arc<str>, u32, String)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut said: Vec<(Arc<str>, u32, String)> = Vec::new();
        for it in order {
            let Stmt::Route(r) = &it.stmt else { continue };
            if r.method == crate::ast::GUARD_METHOD {
                continue;
            }
            let route = format!("{} {}", r.method, route_path(r));
            let mut marks = PageMarks::default();
            page_marks(&r.body, &components, &HashSet::new(), &mut marks, 0);
            for (ev, line) in marks.handlers {
                said.push((it.file.clone(), line, format!("`on{}` here never runs: the page of `{}` is made on the server, and only a component's handlers reach the browser: put this markup in a component", ev, route)));
            }
            for (n, line) in marks.reached {
                if seen.insert(n.clone()) {
                    todo.push((n, it.file.clone(), line, route.clone()));
                }
            }
            while let Some((n, file, line, route)) = todo.pop() {
                let body = match self.globals.get(&n).and_then(|v| v.sig.as_ref()).and_then(|s| s.def.clone().filter(|_| !s.builtin && !s.server && !s.component)) {
                    Some(d) => d,
                    None => match self.bodies.get(&n) {
                        Some(d) if !d.server && !d.component => d.clone(),
                        _ => continue,
                    },
                };
                let mut marks = PageMarks::default();
                let params: HashSet<String> = body.params.iter().map(|p| p.name.to_string()).collect();
                page_marks(&body.body, &components, &params, &mut marks, 0);
                let home = self.globals.get(&n).map_or_else(|| file.clone(), |v| v.file.clone());
                if !marks.handlers.is_empty() {
                    let at = self.shown_at(&file, line);
                    let view = is_view(&body);
                    for (ev, l) in marks.handlers {
                        let msg = if view {
                            format!("`on{}` in view {} never runs: {} is placed in a page ({}), not in a component. Make it a component, or place it inside one", ev, n, n, at)
                        } else {
                            format!("`on{}` in `{}` never runs: `{}` makes markup for the page of `{}` ({}), on the server, and only a component's handlers reach the browser: put it in a component", ev, n, n, route, at)
                        };
                        said.push((home.clone(), l, msg));
                    }
                }
                for (m, l) in marks.reached {
                    if seen.insert(m.clone()) {
                        todo.push((m, home.clone(), l, route.clone()));
                    }
                }
            }
        }
        let dead: HashSet<(Arc<str>, u32)> = said.iter().map(|(f, l, _)| (f.clone(), *l)).collect();
        for (file, line, msg) in said {
            self.diag_at(file, line, Severity::Warning, msg);
        }
        for (file, line, msg) in std::mem::take(&mut self.unreachable) {
            if !dead.contains(&(file.clone(), line)) {
                self.diag_at(file, line, Severity::Warning, msg);
            }
        }
    }

    /// A component's `let`, or its markup, that draws `random`, `random_int` or `uuid`, itself
    /// or through a function or a view it calls: the browser runs the component's body again when
    /// the component wakes, draws again, and puts its value in place of the one the server showed
    /// (FEEDBACK72.md, Q-1015; branch feedback72-81). A `state`'s value is worked out once, on the
    /// server, and the browser keeps it (tried in Chrome); a handler's code is the browser's alone.
    fn drawn_twice(&mut self, defs: &[&FnDef], callers: &HashMap<&str, Vec<usize>>) {
        if !self.draws_seen {
            return;
        }
        // The functions that draw, themselves or by calling one that does: a component that is
        // none of them, and places no view that draws, draws nothing, and is not read again (the
        // website's 1,133 components each read once had cost its check 1%; found in review).
        let mut drawing: HashSet<String> = self.drawers.clone();
        let mut todo: Vec<String> = drawing.iter().cloned().collect();
        while let Some(n) = todo.pop() {
            for &i in callers.get(n.as_str()).into_iter().flatten() {
                if drawing.insert(defs[i].name.clone()) {
                    todo.push(defs[i].name.clone());
                }
            }
        }
        let views_draw = drawing.iter().any(|n| self.globals.get(n).and_then(|v| v.sig.as_ref()).and_then(|s| s.def.as_ref()).is_some_and(|d| is_view(d) && !d.component));
        let mut comps: Vec<(Arc<FnDef>, Arc<str>)> = self
            .globals
            .values()
            .filter_map(|v| v.sig.as_ref().filter(|s| s.component).and_then(|s| s.def.clone()).map(|d| (d, v.file.clone())))
            .collect();
        comps.sort_by(|a, b| (&a.1, a.0.line).cmp(&(&b.1, b.0.line)));
        let mut memo: HashMap<usize, Option<&'static str>> = HashMap::new();
        for (c, file) in comps {
            if !views_draw && !drawing.contains(&c.name) {
                continue;
            }
            // Most components draw nothing: their calls, read once, say so.
            let refs = self.render_refs_of(&c);
            if !refs.iter().any(|r| r.state.is_none() && !form_key_at(&c, r) && self.draws(&r.name, &mut memo).is_some()) {
                continue;
            }
            // What the page shows: the markup the body ends with, and each `let` it reads, or that
            // a `let` it reads reads (a value drawn and used only in a handler, or not at all,
            // changes nothing shown; found in review). A `let` reads only those before it, so one
            // pass from the end finds them.
            let (lets, markup): (Vec<(&str, &Expr)>, Option<&Expr>) = match &c.body {
                Expr::Block(stmts) => (
                    stmts.iter().filter_map(|s| if let Stmt::Let(Pattern::Bind(n), e, _) = s { Some((n.as_str(), e)) } else { None }).collect(),
                    match stmts.last() {
                        Some(Stmt::Expr(e)) => Some(e),
                        _ => None,
                    },
                ),
                e => (Vec::new(), Some(e)),
            };
            let read_now = |e: &Expr, out: &mut HashSet<String>| {
                let later = runs_later(e);
                walk_expr(e, &mut |x| {
                    if let Expr::Ident(n, _) = x {
                        if !later.contains(&(x as *const Expr)) {
                            out.insert(n.to_string());
                        }
                    }
                });
            };
            let mut shown: HashSet<String> = HashSet::new();
            if let Some(m) = markup {
                read_now(m, &mut shown);
            }
            for (n, e) in lets.iter().rev() {
                if shown.contains(*n) {
                    read_now(e, &mut shown);
                }
            }
            let mut own: Vec<(Option<&str>, &Expr)> = lets.into_iter().filter(|(n, _)| shown.contains(*n)).map(|(n, e)| (Some(n), e)).collect();
            own.extend(markup.map(|m| (None, m)));
            let bound = fn_bound(&c);
            for (name, e) in own {
                let mut refs = Vec::new();
                render_refs(e, false, 0, None, &bound, &mut refs, 0);
                for rf in refs.iter().filter(|r| r.state.is_none() && !form_key_at(&c, r)) {
                    let Some((through, drawn)) = self.draws(&rf.name, &mut memo) else { continue };
                    let how = if through.is_empty() || rf.tag { String::new() } else { format!(" (`{}` calls `{}`)", through, drawn) };
                    let msg = match name {
                        Some(n) => format!(
                            "`{}` is drawn again in the browser when <{}> wakes{}, so what the page shows changes under the visitor: `state {} = ...` draws once, on the server, and the browser keeps it",
                            n, c.name, how, n
                        ),
                        None if rf.tag => format!(
                            "<{}> here calls `{}`, drawn again in the browser when <{}> wakes, so what the page shows changes under the visitor: give it the value from a state, `state x = {}()`, which draws once, on the server, and the browser keeps it",
                            through, drawn, c.name, drawn
                        ),
                        None => {
                            let f = if through.is_empty() { drawn } else { through.as_str() };
                            format!(
                                "`{}()` here is drawn again in the browser when <{}> wakes{}, so what the page shows changes under the visitor: keep it in a state, `state x = {}()`, which draws once, on the server, and the browser keeps it",
                                f, c.name, how, f
                            )
                        }
                    };
                    self.diag_at(file.clone(), rf.line, Severity::Warning, msg);
                }
            }
        }
    }

    /// Whether calling `name` draws a new value each time (Q-1015): one of the builtins that do, or
    /// a plain function or view that calls one, as far as their calls go; the name it is reached
    /// through (empty for the builtin itself) and the builtin.
    fn draws(&mut self, name: &str, memo: &mut HashMap<usize, Option<&'static str>>) -> Option<(String, &'static str)> {
        let sig = self.globals.get(name).and_then(|v| v.sig.clone())?;
        if sig.builtin {
            return DRAWS.iter().find(|d| **d == name).map(|d| (String::new(), *d));
        }
        let d = sig.def.clone().filter(|_| !sig.server && !sig.component)?;
        self.draws_def(&d, memo).map(|b| (name.to_string(), b))
    }

    /// The builtin that draws which `d`'s code, or code it calls, calls (Q-1015): each function read
    /// once for the whole check, depth first with a list of what is being read, not by recursion; one
    /// being read counts as drawing nothing, so a recursive one ends.
    fn draws_def(&mut self, d: &Arc<FnDef>, memo: &mut HashMap<usize, Option<&'static str>>) -> Option<&'static str> {
        if let Some(r) = memo.get(&def_key(d)) {
            return *r;
        }
        memo.insert(def_key(d), None);
        let refs = self.render_refs_of(d);
        let mut stack: Vec<(Arc<FnDef>, Arc<[RenderRef]>, usize)> = vec![(d.clone(), refs, 0)];
        let mut found: Option<&'static str> = None;
        while let Some(top) = stack.last_mut() {
            if let Some(b) = found {
                memo.insert(def_key(&top.0), Some(b));
                stack.pop();
                continue;
            }
            let Some(rf) = top.1.get(top.2).cloned() else {
                stack.pop();
                continue;
            };
            top.2 += 1;
            let Some(s) = self.globals.get(&*rf.name).and_then(|v| v.sig.clone()) else { continue };
            if s.builtin {
                found = DRAWS.iter().find(|b| **b == &*rf.name).copied().filter(|_| !form_key_at(&top.0, &rf));
            } else if let Some(g) = s.def.clone().filter(|_| !s.server && !s.component) {
                match memo.get(&def_key(&g)) {
                    Some(r) => found = *r,
                    None => {
                        memo.insert(def_key(&g), None);
                        let refs = self.render_refs_of(&g);
                        stack.push((g, refs, 0));
                    }
                }
            }
        }
        found
    }

    /// `pages // 2`, Python's whole-number division, is `pages` and a comment: said of a `//`
    /// comment right after a value on its line that holds only a number, or a name the program has
    /// there (FEEDBACK72.md, Q-1016; branch feedback72-81). A comment of words is left alone.
    /// A whole number the name it follows is given at the top level is that name's value, written
    /// as a result (`consts`; found in review).
    fn value_comments_said(&mut self, consts: &HashMap<String, i64>) {
        // Whether `name` is a number where it is read: a local in scope there, else a top-level
        // `let`, typed or given one; never a function, a field, or of no known type (`retries //
        // 3` with `retries` a parameter, `amount // price` with `price` a function; found in
        // review).
        let numeric = |me: &Self, name: &str, file: &Arc<str>, line: u32| {
            let detail = me
                .out
                .locals
                .iter()
                .rev()
                .find(|l| l.name == name && l.file == **file && l.line <= line && line <= l.end)
                .map(|l| l.detail.as_str())
                .or_else(|| me.out.symbols.iter().find(|s| s.name == name && s.kind == SymbolKind::Variable).map(|s| s.detail.as_str()));
            detail.and_then(|d| d.split_once(": ")).is_some_and(|(kw, t)| {
                !kw.contains('(') && t.split(" | ").all(|p| matches!(p, "int" | "float" | "dec" | "num"))
            })
        };
        for (file, line, word, after) in std::mem::take(&mut self.value_comments) {
            let number = word.starts_with(|c: char| c.is_ascii_digit());
            if number && consts.get(&after).is_some_and(|v| word.parse::<i64>().ok() == Some(*v)) {
                continue;
            }
            let named = !number && numeric(self, &word, &file, line);
            if (number || named) && numeric(self, &after, &file, line) {
                self.diag_at(file, line, Severity::Warning, format!("`//` starts a comment here, so `// {}` is left out: whole-number division is `floor(a / b)`", word));
            }
        }
    }
}

/// Whether an element holds one the keyboard reaches: a button, a link, a field, a summary, or one
/// with `tabindex`, as written inside it or in its `{...}` (FEEDBACK72.md, Q-1019, found in review;
/// branch feedback72-81).
fn reachable_inside(el: &HtmlEl) -> bool {
    let reachable = |h: &HtmlEl| {
        matches!(h.tag.as_str(), "button" | "input" | "select" | "textarea" | "summary")
            || (h.tag == "a" && h.attrs.iter().any(|(n, _)| n == "href"))
            || h.attrs.iter().any(|(n, _)| n == "tabindex")
    };
    let mut found = false;
    for c in &el.children {
        let mut look = |h: &HtmlEl| found |= reachable(h);
        match c {
            HtmlChild::El(h) => walk_html_els(h, &mut look),
            HtmlChild::Expr(x) => walk_expr(x, &mut |e| {
                if let Expr::Html(h, _) = e {
                    walk_html_els(h, &mut look);
                }
            }),
        }
        if found {
            return true;
        }
    }
    false
}

/// The builtins that give another value at each call, which a component's body draws again in the
/// browser (Q-1015). Not `now`: what a page shows of it (`ago(t, now())`, a year, a day) is the same
/// text a moment later, so the browser takes the component up as it is; in app/ and the website
/// its 46 warnings were all such (survey of branch feedback72-81).
const DRAWS: &[&str] = &["random", "random_int", "uuid"];

/// Whether `rf`, a `uuid` call of `d`'s, is the key Quillo writes into a `<form idempotent>` on that
/// line, which the program never wrote (Q-1015).
fn form_key_at(d: &FnDef, rf: &RenderRef) -> bool {
    if &*rf.name != "uuid" {
        return false;
    }
    let mut found = false;
    walk_fn(d, &mut |e| {
        if let Expr::Html(el, _) = e {
            walk_html_els(el, &mut |h| {
                found |= h.tag == "input"
                    && h.line == rf.line
                    && h.attrs.iter().any(|(n, v)| n == "name" && matches!(v, AttrVal::Expr(Expr::Lit(Lit::Str(s))) if &**s == crate::idempotency::FORM_FIELD));
            });
        }
    });
    found
}

/// What to write for a handler with modifiers, Vue's `@click.prevent` (`handler` as the guide
/// writes it, `onclick`; `mods`, what follows its first dot): there are none, and the event
/// object does what they do (FEEDBACK72.md, Q-1031 and Q-1032; branch feedback72-81).
pub(crate) fn modifier_advice(handler: &str, mods: &str) -> Option<String> {
    if mods.is_empty() {
        return None;
    }
    let mods: Vec<&str> = mods.split('.').collect();
    if handler == "onsubmit" && mods.contains(&"prevent") {
        return Some("there are no modifiers, and `onsubmit` already keeps the page from reloading: write `onsubmit={...}`".to_string());
    }
    let mut does: Vec<&str> = Vec::new();
    for m in &mods {
        match *m {
            "prevent" => does.push("e.preventDefault();"),
            "stop" => does.push("e.stopPropagation();"),
            _ => {}
        }
    }
    does.push("...");
    Some(format!("there are no modifiers; write `{}={{(e) => {{ {} }}}}`", handler, does.join(" ")))
}

/// Whether `e` is text the program writes (Q-1017): a literal, a `` css`...` ``, a name of
/// `known`, or a template or a `+` of those. Read with a list of what is left, not by recursion.
fn css_written(e: &Expr, known: &HashSet<String>) -> bool {
    let mut todo = vec![e];
    while let Some(x) = todo.pop() {
        match x {
            Expr::Lit(Lit::Str(_)) => {}
            Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(c, _) if c == "css") => {}
            Expr::Ident(n, _) if known.contains(n.as_str()) => {}
            Expr::Template(parts) => todo.extend(parts.iter().filter_map(|p| if let TPart::Expr(y) = p { Some(y) } else { None })),
            Expr::Binary(BinOp::Add, a, b, _) => {
                todo.push(a);
                todo.push(b);
            }
            _ => return false,
        }
    }
    true
}

/// What a link's value says of its address (Q-1251): a literal (`Whole`), or the literal start of a
/// template or of a `+`, then one value that ends it (`Last`: `/todos/${id}`, one more piece's
/// worth, found in review) or more (`Open`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum LinkEnd {
    Whole,
    Last,
    Open,
}

fn link_start(e: &Expr) -> Option<(String, LinkEnd)> {
    match e {
        Expr::Lit(Lit::Str(s)) => Some((s.to_string(), LinkEnd::Whole)),
        Expr::Template(parts) => match parts.as_slice() {
            [TPart::Lit(s)] => Some((s.clone(), LinkEnd::Whole)),
            [TPart::Lit(s), TPart::Expr(_)] => Some((s.clone(), LinkEnd::Last)),
            [TPart::Lit(s), ..] => Some((s.clone(), LinkEnd::Open)),
            _ => None,
        },
        Expr::Binary(BinOp::Add, a, b, _) => {
            let mut left = &**a;
            let mut more = false;
            while let Expr::Binary(BinOp::Add, x, _, _) = left {
                left = x;
                more = true;
            }
            match left {
                Expr::Lit(Lit::Str(s)) if !more && !matches!(&**b, Expr::Lit(_)) => Some((s.to_string(), LinkEnd::Last)),
                Expr::Lit(Lit::Str(s)) => Some((s.to_string(), LinkEnd::Open)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The routes' paths as a tree of pieces (Q-1251, found in review): each node's routes that end
/// there and that take the rest there, and its children by a written piece and by a parameter (one
/// child for each prefix and type, so `/:lang/...` routes share it).
#[derive(Default)]
struct LinkNode {
    lits: HashMap<String, usize, crate::value::BuildFx>,
    params: Vec<(String, Option<String>, usize)>,
    ends: Vec<usize>,
    rests: Vec<usize>,
}

struct LinkTree {
    nodes: Vec<LinkNode>,
}

impl LinkTree {
    fn of(routes: &[&RouteDef]) -> LinkTree {
        let mut nodes = vec![LinkNode::default()];
        'routes: for (i, r) in routes.iter().enumerate() {
            let mut at = 0;
            for seg in &r.path {
                let next = nodes.len();
                at = match seg {
                    Seg::Rest(_) => {
                        nodes[at].rests.push(i);
                        continue 'routes;
                    }
                    Seg::Lit(l) => *nodes[at].lits.entry(l.clone()).or_insert(next),
                    Seg::Param(_, ty, prefix) => match nodes[at].params.iter().find(|(p, t, _)| p == prefix && t == ty) {
                        Some((_, _, c)) => *c,
                        None => {
                            nodes[at].params.push((prefix.clone(), ty.clone(), next));
                            next
                        }
                    },
                };
                if at == next {
                    nodes.push(LinkNode::default());
                }
            }
            nodes[at].ends.push(i);
        }
        LinkTree { nodes }
    }

    /// Calls `take` with each route that may take an address of these pieces (and, with `start`,
    /// a piece starting so and anything after), until it says yes; whether it did. Read with a list
    /// of what is left, not by recursion.
    fn find<S: AsRef<str>>(&self, pieces: &[S], start: Option<&str>, last: bool, todo: &mut Vec<(usize, usize, bool)>, take: &mut dyn FnMut(usize) -> bool) -> bool {
        todo.clear();
        todo.push((0, 0, false));
        while let Some((n, depth, below)) = todo.pop() {
            let node = &self.nodes[n];
            if node.rests.iter().any(|&i| take(i)) {
                return true;
            }
            // Under a template's start: every route below.
            if below {
                if node.ends.iter().any(|&i| take(i)) {
                    return true;
                }
                todo.extend(node.lits.values().map(|&c| (c, depth + 1, true)));
                todo.extend(node.params.iter().map(|(_, _, c)| (*c, depth + 1, true)));
                continue;
            }
            if depth == pieces.len() {
                match start {
                    None => {
                        if node.ends.iter().any(|&i| take(i)) {
                            return true;
                        }
                    }
                    Some(st) => {
                        let kids = node
                            .lits
                            .iter()
                            .filter(|(k, _)| k.starts_with(st))
                            .map(|(_, &c)| c)
                            .chain(node.params.iter().filter(|(p, _, _)| st.starts_with(p.as_str()) || p.starts_with(st)).map(|(_, _, c)| *c));
                        if last {
                            // One piece more, ending the address: those that end there.
                            for c in kids {
                                let kid = &self.nodes[c];
                                if kid.ends.iter().chain(kid.rests.iter()).any(|&i| take(i)) {
                                    return true;
                                }
                            }
                        } else {
                            todo.extend(kids.map(|c| (c, depth + 1, true)));
                        }
                    }
                }
                continue;
            }
            let piece = pieces[depth].as_ref();
            if let Some(&c) = node.lits.get(piece) {
                todo.push((c, depth + 1, false));
            }
            for (prefix, ty, c) in &node.params {
                let Some(v) = piece.strip_prefix(prefix.as_str()) else { continue };
                if (v.is_empty() && !prefix.is_empty()) || ty.as_ref().is_some_and(|t| crate::interp::coerce(t, v).is_none()) {
                    continue;
                }
                todo.push((*c, depth + 1, false));
            }
        }
        false
    }
}

/// Whether a route's path can take an address whose pieces are `pieces` (Q-1251): all of it, or,
/// with `partial`, those and then a piece starting with `partial` and anything after, as the
/// rest of a template may be (`last`: that piece ends the address). As `server::match_path`
/// matches, a typed parameter by its type.
fn link_fits<S: AsRef<str>>(def: &[Seg], pieces: &[S], partial: Option<&str>, last: bool) -> bool {
    for (i, seg) in def.iter().enumerate() {
        match (pieces.get(i).map(AsRef::as_ref), partial) {
            (Some(p), _) => match seg {
                Seg::Rest(_) => return true,
                Seg::Lit(l) => {
                    if l != p {
                        return false;
                    }
                }
                Seg::Param(_, ty, prefix) => {
                    let Some(v) = p.strip_prefix(prefix.as_str()) else { return false };
                    if (v.is_empty() && !prefix.is_empty()) || ty.as_ref().is_some_and(|t| crate::interp::coerce(t, v).is_none()) {
                        return false;
                    }
                }
            },
            // What the template gives from here on is not known: this piece starts with `start`.
            (None, Some(start)) => {
                let takes = match seg {
                    Seg::Rest(_) => return true,
                    Seg::Lit(l) => l.starts_with(start),
                    Seg::Param(_, _, prefix) => start.starts_with(prefix.as_str()) || prefix.starts_with(start),
                };
                return takes && (!last || def.len() == i + 1 || matches!(def.get(i + 1), Some(Seg::Rest(_))));
            }
            (None, None) => return matches!(seg, Seg::Rest(_)),
        }
    }
    // A template's start after a `/` (`/todos/${id}`) is a piece of its own still (found in
    // review).
    def.len() == pieces.len() && partial.is_none()
}

/// How far an address is from a route's path, piece by piece (Q-1251): a literal by its edits, a
/// parameter nothing when the piece fits its type, and a piece one has and the other lacks by its
/// length; none past a `*rest`.
fn link_distance<S: AsRef<str>>(def: &[Seg], pieces: &[S], partial: Option<&str>) -> Option<usize> {
    // A template's start is a piece too, though empty (`/trail/${id}`).
    let n = pieces.len() + usize::from(partial.is_some());
    let mut d = 0;
    for i in 0..def.len().max(n) {
        let piece = pieces.get(i).map(AsRef::as_ref).or(if i == pieces.len() { partial } else { None });
        d += match (def.get(i), piece) {
            (Some(Seg::Rest(_)), _) => return Some(d),
            // The start of a piece, against as much of the literal.
            (Some(Seg::Lit(l)), Some(p)) if i == pieces.len() => edit_distance(&l.chars().take(p.chars().count()).collect::<String>(), p),
            (Some(Seg::Lit(l)), Some(p)) => edit_distance(l, p),
            // The start of a piece: its prefix, if any, can still follow.
            (Some(Seg::Param(_, _, prefix)), Some(p)) if i == pieces.len() => usize::from(!(p.starts_with(prefix.as_str()) || prefix.starts_with(p))),
            // A prefix left out is an edit; a value that does not fit two.
            (Some(Seg::Param(_, ty, prefix)), Some(p)) => {
                let (v, miss) = p.strip_prefix(prefix.as_str()).map_or((p, 1), |v| (v, 0));
                miss + if v.is_empty() || ty.as_ref().is_some_and(|t| crate::interp::coerce(t, v).is_none()) { 2 } else { 0 }
            }
            // What a template gives after its start may be this piece.
            (Some(_), None) if partial.is_some() => 0,
            (Some(Seg::Lit(l)), None) => l.chars().count().max(1),
            (Some(Seg::Param(..)), None) => 2,
            (None, Some(p)) if i == pieces.len() => p.chars().count().max(1),
            (None, Some(p)) => p.chars().count().max(1),
            (None, None) => 0,
        };
    }
    Some(d)
}

/// What `page_marks` found in a page's markup (Q-1020): the handlers on its elements, by event and
/// line, and the views and functions it places or calls, by name and line.
#[derive(Default)]
struct PageMarks {
    handlers: Vec<(String, u32)>,
    reached: Vec<(String, u32)>,
}

/// The handlers on the elements of markup the server makes, and the views and functions it places
/// or calls (Q-1020): not inside a component's element (its own, or given to it), nor in a function
/// written there, which runs later or is given to code that may run it in the browser. One level
/// deeper per node, with room on the stack looked at every eighth, as `render_refs` does.
fn page_marks(e: &Expr, components: &HashSet<String>, params: &HashSet<String>, out: &mut PageMarks, depth: u32) {
    if (depth + 1) % 8 == 0 {
        crate::stack::maybe_grow(256 << 10, 4 << 20, || page_marks_here(e, components, params, out, depth + 1))
    } else {
        page_marks_here(e, components, params, out, depth + 1)
    }
}

fn page_marks_here(e: &Expr, components: &HashSet<String>, params: &HashSet<String>, out: &mut PageMarks, depth: u32) {
    let go = |x: &Expr, out: &mut PageMarks| page_marks(x, components, params, out, depth);
    match e {
        Expr::Lit(_) | Expr::Ident(..) | Expr::Lambda(_) => {}
        Expr::Template(parts) => parts.iter().for_each(|p| {
            if let TPart::Expr(x) = p {
                go(x, out)
            }
        }),
        Expr::List(xs) => xs.iter().for_each(|x| go(x, out)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| go(x, out)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => go(x, out),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Index(a, b, _) => {
            go(a, out);
            go(b, out);
        }
        Expr::Call(c, args, line) => {
            match &**c {
                Expr::Ident(n, _) => out.reached.push((n.to_string(), *line)),
                other => go(other, out),
            }
            args.iter().for_each(|x| go(x, out));
        }
        Expr::Pipe(a, b, line) => {
            go(a, out);
            match &**b {
                Expr::Ident(n, _) => out.reached.push((n.to_string(), *line)),
                Expr::Call(c, args, _) => {
                    match &**c {
                        Expr::Ident(n, _) => out.reached.push((n.to_string(), *line)),
                        other => go(other, out),
                    }
                    args.iter().for_each(|x| go(x, out));
                }
                other => go(other, out),
            }
        }
        Expr::If(c, a, b) => {
            go(c, out);
            go(a, out);
            if let Some(b) = b {
                go(b, out);
            }
        }
        Expr::Match(x, arms, _) => {
            go(x, out);
            for a in arms {
                if let Some(g) = &a.guard {
                    go(g, out);
                }
                go(&a.body, out);
            }
        }
        Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
            go(i, out);
            go(b, out);
        }
        Expr::Reply(a, b, _) => {
            go(a, out);
            if let Some(b) = b {
                go(b, out);
            }
        }
        Expr::Block(stmts) => {
            for st in stmts.iter() {
                match st {
                    Stmt::Let(_, x, _) | Stmt::Expr(x) | Stmt::Assign(_, _, x, _) | Stmt::Return(Some(x), _) | Stmt::State(_, x, _) => go(x, out),
                    Stmt::SetField(o, _, _, x, _) => {
                        go(o, out);
                        go(x, out);
                    }
                    _ => {}
                }
            }
        }
        Expr::Html(el, _) => page_marks_html(el, components, params, out, depth),
    }
}

fn page_marks_html(el: &HtmlEl, components: &HashSet<String>, params: &HashSet<String>, out: &mut PageMarks, depth: u32) {
    if components.contains(&el.tag) {
        return;
    }
    let element = el.tag.starts_with(|c: char| c.is_ascii_lowercase());
    if !element {
        out.reached.push((el.tag.clone(), el.line));
    }
    for x in el.id.iter().chain(el.classes.iter()) {
        page_marks(x, components, params, out, depth);
    }
    for (a, v) in &el.attrs {
        match (a.strip_prefix('@'), v) {
            // One that only hands the event to what the view was given (`on_click && on_click()`)
            // loses nothing where it is given nothing, as a kit's views are in a page (app/stays).
            (Some(ev), AttrVal::Expr(x)) if element => {
                let forwards = match x {
                    Expr::Ident(n, _) => params.contains(n.as_str()),
                    // Nothing set, and no name read but its own and the view's parameters.
                    Expr::Lambda(d) if !assigns_any(&d.body) => {
                        let bound = fn_bound(d);
                        let mut own = true;
                        walk_fn(d, &mut |y| own &= !matches!(y, Expr::Ident(n, _) if !bound.contains(n.as_str()) && !params.contains(n.as_str())));
                        own
                    }
                    _ => false,
                };
                if !forwards {
                    out.handlers.push((ev.to_string(), el.line));
                }
            }
            (_, AttrVal::Expr(x) | AttrVal::Spread(x)) => page_marks(x, components, params, out, depth),
            _ => {}
        }
    }
    for c in &el.children {
        match c {
            HtmlChild::El(child) => {
                if (depth + 1) % 8 == 0 {
                    crate::stack::maybe_grow(256 << 10, 4 << 20, || page_marks_html(child, components, params, out, depth + 1))
                } else {
                    page_marks_html(child, components, params, out, depth + 1)
                }
            }
            HtmlChild::Expr(x) => page_marks(x, components, params, out, depth),
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
            // Found by name in the sorted fields, not one by one (FEEDBACK75.md, Q-1053; branch
            // feedback75).
            match af.binary_search_by(|(n, _)| n.cmp(k)).ok().map(|i| &af[i]) {
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

// ---- what a `return` does where it is written (FEEDBACK52.md, Q-678 to Q-680; branch feedback52) ----

/// Where a value goes, for what a `return` in it does.
#[derive(Clone, Copy, PartialEq)]
enum Goes {
    /// Nowhere: a statement, run for what it does.
    Dropped,
    /// To the end of the function: its last statement, which gives it, if it is a value. A
    /// `for` there may be a loop run for what it does, whose list is given by the way.
    End,
    /// Out of the function: a `return`'s value, an `= expr` body. A `return` in it leaves with
    /// what it would have given anyway.
    Out,
    /// To something else: a `let`, an argument, an operand.
    Kept,
    /// Kept, through a way of a kept `if` or `match`, or the right of `??`, `&&` or `||`: that
    /// one alone says whether its ways leave. An `if`, `match` or block here counts toward its way
    /// but is not said itself: `if b != 0 { a / b } else { if a > 0 { return 1 } else { return
    /// -1 } }` is a guard (second review, MEDIUM 3; branch feedback52).
    Inner,
}

/// A value written as one: a literal other than null, a text, a list, a map, an operator. A way
/// of an `if` with no `else` that is one gives null when the `if` is false. Markup is not one:
/// null shows nothing, which is what such an `if` is for; nor `&&` and `||`, which also call
/// (`on_empty && on_empty()`, found in quillo-components' swipe_deck).
fn written_value(e: &Expr) -> bool {
    match e {
        Expr::Binary(op, ..) => !matches!(op, BinOp::And | BinOp::Or),
        e => matches!(e, Expr::Lit(Lit::Str(_) | Lit::Int(_) | Lit::Float(_) | Lit::Dec(_) | Lit::Bool(_)) | Expr::Template(_) | Expr::List(_) | Expr::Map(_) | Expr::Unary(..)),
    }
}

/// The last expression of a way of an `if` or `match`: what it gives, as written.
fn way_value(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::Block(stmts) => match stmts.last() {
            Some(Stmt::Expr(x)) => Some(x),
            _ => None,
        },
        e => Some(e),
    }
}

/// Whether a way of an `if` ends in a value written as one.
fn way_gives(e: &Expr) -> bool {
    way_value(e).is_some_and(written_value)
}

/// The first `return` of a block itself, not of a block in it: its value and its line.
fn own_return(e: &Expr) -> Option<(Option<&Expr>, u32)> {
    let Expr::Block(stmts) = e else { return None };
    stmts.iter().find_map(|s| if let Stmt::Return(v, line) = s { Some((v.as_ref(), *line)) } else { None })
}

/// How a statement always leaves, so that what follows it never runs: by a `return` (`Bare`
/// with no value), a `reply`, or an `if` or `match` each of whose ways does.
#[derive(Clone, Copy)]
enum Leaves {
    Return(u32),
    Bare(u32),
    Reply(u32),
    If(u32),
    Match(u32),
}

impl Leaves {
    fn line(self) -> u32 {
        match self {
            Leaves::Return(l) | Leaves::Bare(l) | Leaves::Reply(l) | Leaves::If(l) | Leaves::Match(l) => l,
        }
    }
}

/// How a function's end gives null without saying so, when it can be reached: a last
/// statement that is no value, an `if` with no `else` (and whether a way of it gives a value
/// written as one), or a `for` whose rounds give nothing, which gives a list of nulls.
#[derive(Clone, Copy)]
enum End {
    Null(u32),
    NoElse(u32, bool),
    List(u32),
}

/// The end of an `if` and its `else`, from each way's: the first way's own, unless the `else`
/// has no `else` of its own, which is then the end, and a way of it a value if either is.
fn ends_of(a: Option<End>, b: Option<End>, a_gives: bool) -> Option<End> {
    match (a, b) {
        (_, Some(End::NoElse(l, v))) => Some(End::NoElse(l, v || a_gives)),
        (Some(x), _) => Some(x),
        (None, b) => b,
    }
}

/// What the walk works out of a value or a statement on its way back up, once each: worked
/// out again at each level, it took a time of the square of the depth (review of 1a1b3ae,
/// LOW 2).
#[derive(Clone, Copy, Default)]
struct Flow<'a> {
    /// It always leaves.
    leaves: Option<Leaves>,
    /// It never lets the statement after it run: it leaves, or it is `while true`, which only
    /// a `return` ends.
    never: bool,
    /// Last in a function, how it gives null without saying so.
    end: Option<End>,
    /// As a `for`'s body, each round gives null or leaves: it ends in no value (a `let`, an
    /// assignment, a `return`), or in an `if` with no `else` whose way leaves.
    empty_rounds: bool,
    /// A `return` in it whose value is kept has been said: an `if` around it is not said again.
    said: bool,
    /// When it always leaves, the first `return` it leaves by, as written: its value and its
    /// line. Looked for afterwards in a way's own statements and the `if` it ended in, one in a
    /// block of its own was missed (third review, LOW 6).
    ret: Option<(Option<&'a Expr>, u32)>,
}

/// What an `else if` chain gives, from its last `if` back: whether every way leaves (an `else`
/// at its end too), how it ends, whether a `return` in it was said, and how its first way leaves.
struct Chain<'a> {
    all: bool,
    end: Option<End>,
    said: bool,
    first: Option<Leaves>,
    /// No `else` at its end, and each way leaves: as a `for`'s body, each round gives null or
    /// leaves, as an `if` with no `else` does (second review, LOW 5).
    open: bool,
    /// The first `return`, as written, of the first way that leaves by one.
    ret: Option<(Option<&'a Expr>, u32)>,
}

/// Of `names`, the functions the program hands to other code rather than calls: named where a
/// value goes, as an argument, a field, an attribute, or a group's middleware.
fn handed_over<'a>(order: &[Item], names: &HashSet<&'a str>) -> HashSet<&'a str> {
    // Each name's times written, and times called: the walk sees a callee as a call and again
    // as a name.
    let mut uses: HashMap<&'a str, (u32, u32)> = HashMap::new();
    let mut handed = HashSet::new();
    let mut count = |e: &Expr, called: bool| {
        if let Expr::Ident(n, _) = e {
            if let Some(k) = names.get(n.as_str()) {
                let u = uses.entry(*k).or_default();
                if called {
                    u.1 += 1;
                } else {
                    u.0 += 1;
                }
            }
        }
    };
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |x| match x {
            Expr::Ident(..) => count(x, false),
            Expr::Pipe(_, f, _) => count(f, true),
            // `__answer(admins(__request), "admins")` starts each route of `routes /x | admins`.
            Expr::Call(f, args, _) => match (&**f, args.first()) {
                (Expr::Ident(a, _), Some(Expr::Call(m, _, _))) if a == "__answer" => {
                    if let Expr::Ident(n, _) = &**m {
                        if let Some(k) = names.get(n.as_str()) {
                            handed.insert(*k);
                        }
                    }
                }
                _ => count(f, true),
            },
            _ => {}
        });
    }
    handed.extend(uses.into_iter().filter(|(_, (all, called))| all > called).map(|(n, _)| n));
    handed
}

/// A `return` that gives something: not null, an error (`return err(...)` says it failed, and
/// its end that it did not), a `reply`, which answers the request, or markup, whose null shows
/// nothing, as a view's does (review of 1a1b3ae, M3: `FnDef` does not keep `view`, so a
/// lowercase view is known by its markup).
fn gives_a_value(e: &Expr) -> bool {
    !matches!(e, Expr::Lit(Lit::Nil) | Expr::Reply(..) | Expr::Html(..)) && !matches!(e, Expr::Call(f, _, _) if matches!(&**f, Expr::Ident(n, _) if n == "err"))
}

/// Whose a `return` is, as a message names it.
#[derive(Clone, Copy)]
enum Whose<'a> {
    Named(&'a str),
    Arrow,
    Route,
    File,
}

impl Whose<'_> {
    fn text(self) -> String {
        match self {
            Whose::Named(n) => format!("`{}`", n),
            Whose::Arrow => "the function".to_string(),
            Whose::Route => "the route".to_string(),
            Whose::File => "the file".to_string(),
        }
    }
}

/// A walk of the program's top-level statements for these: where each `return` is, what
/// follows it, and how each named function ends.
struct Returns<'a> {
    /// What to say, by line.
    said: Vec<(u32, String)>,
    /// The functions being read, the innermost last: whose, and the line of its first `return`
    /// that gives a value.
    fns: Vec<(Whose<'a>, Option<u32>)>,
    /// Null ends, by line, with the function's name: said once the whole program is read.
    ends: Vec<(u32, String, String)>,
    /// How deep the walk is (`deep`).
    depth: u32,
}

impl<'a> Returns<'a> {
    fn name(&self) -> String {
        self.fns.last().map_or(Whose::File, |f| f.0).text()
    }

    fn function(&mut self, d: &'a FnDef, whose: Whose<'a>, named: bool) {
        self.fns.push((whose, None));
        for p in &d.params {
            if let Some(e) = &p.default {
                self.value(e, Goes::Kept);
            }
        }
        let end = match &d.body {
            Expr::Block(stmts) => self.stmts(stmts, Goes::End).end,
            // An `= expr` body that is a `for` gives its list as meant.
            e => self.value(e, Goes::Out).end.filter(|x| !matches!(x, End::List(_))),
        };
        let gave = self.fns.pop().and_then(|f| f.1);
        // An untyped function's null end: a typed one is held to its type already (Q-679). A
        // view or a component shows nothing for null, which is what such an end is for.
        if !named || d.ret.is_some() || d.component || is_view(d) {
            return;
        }
        // An `if` whose condition has no line (`if true`) is said on the function's.
        let at_least = |l: u32| if l == 0 { d.line } else { l };
        let msg = match (end, gave) {
            (Some(End::Null(line) | End::NoElse(line, _)), Some(at)) => (
                at_least(line),
                format!("`{}` returns a value on line {} but gives null when it reaches its end: end it with a value too, or `null` if null is meant", d.name, at),
            ),
            (Some(End::List(line)), Some(at)) => (
                line,
                format!(
                    "`{}` returns a value on line {} but, when it reaches its end, gives the list of nulls its `for` makes: end it with the value it should give, or `null`",
                    d.name, at
                ),
            ),
            (Some(End::NoElse(line, true)), None) => (
                at_least(line),
                format!("`{}` gives null when this `if` is false, as it has no `else`: write one, or `else {{ null }}` if null is meant", d.name),
            ),
            _ => return,
        };
        self.ends.push((msg.0, d.name.clone(), msg.1));
    }

    fn stmts(&mut self, stmts: &'a [Stmt], goes: Goes) -> Flow<'a> {
        let n = stmts.len();
        // An empty block gives null.
        let mut out = Flow { empty_rounds: true, ..Flow::default() };
        let mut first: Option<(usize, Leaves)> = None;
        let mut ret = None;
        let mut never = false;
        for (i, s) in stmts.iter().enumerate() {
            let last = i + 1 == n;
            let f = self.stmt(s, if last { goes } else { Goes::Dropped });
            if first.is_none() {
                first = f.leaves.map(|l| (i, l));
                ret = f.ret;
            }
            never |= f.never;
            if last {
                out = match s {
                    Stmt::Expr(_) => f,
                    // Any other statement gives null.
                    s => Flow { end: top_line(s).map(End::Null), empty_rounds: true, ..f },
                };
            }
        }
        out.leaves = first.map(|(_, l)| l);
        out.ret = ret;
        out.never = never;
        if never {
            out.end = None;
        }
        // What follows a statement that always leaves (Q-680).
        let Some((i, how)) = first else { return out };
        if i + 1 == n {
            return out;
        }
        let f = self.name();
        // `return` and the value meant for it on the next line (review of 1a1b3ae); one with a
        // statement after it on its own line, `print("a"); return; print("b")`, is a plain one
        // (second review, LOW 4). A literal has no line, and is taken to be on the next, as is
        // most likely: `return`, then `0` (third review, LOW 5).
        let next_line = top_line(&stmts[i + 1]).unwrap_or(u32::MAX);
        let why = match how {
            Leaves::Return(l) => format!("the `return` on line {} always leaves {}", l, f),
            Leaves::Bare(l) if next_line > l => format!("a `return` at the end of a line gives null, so the one on line {} leaves {} before this", l, f),
            Leaves::Bare(l) => format!("the `return` on line {} always leaves {}", l, f),
            Leaves::Reply(l) => format!("the `reply` on line {} always ends the request", l),
            Leaves::If(l) => format!("each way of the `if` on line {} leaves {}", l, f),
            Leaves::Match(l) => format!("each arm of the `match` on line {} leaves {}", l, f),
        };
        // A statement with no line of its own (a literal) is said where the leaving is.
        match stmts[i + 1..].iter().find_map(top_line) {
            Some(line) => self.said.push((line, format!("this never runs: {}", why))),
            None => self.said.push((how.line(), format!("what follows never runs: {}", why))),
        }
        out
    }

    fn stmt(&mut self, s: &'a Stmt, goes: Goes) -> Flow<'a> {
        match s {
            Stmt::Let(Pattern::Bind(n), Expr::Lambda(d), _) => self.function(d, Whose::Named(n.as_str()), false),
            Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::State(_, e, _) => {
                self.value(e, Goes::Kept);
            }
            Stmt::SetField(o, _, _, e, _) => {
                self.value(o, Goes::Kept);
                self.value(e, Goes::Kept);
            }
            Stmt::Expr(e) => {
                let f = self.value(e, goes);
                let forever = matches!(e, Expr::While(c, ..) if matches!(**c, Expr::Lit(Lit::Bool(true))));
                return Flow { never: f.leaves.is_some() || forever, ..f };
            }
            Stmt::Return(v, line) => {
                if let Some(v) = v {
                    if gives_a_value(v) {
                        if let Some(f) = self.fns.last_mut() {
                            f.1.get_or_insert(*line);
                        }
                    }
                    self.value(v, Goes::Out);
                }
                let how = if v.is_some() { Leaves::Return(*line) } else { Leaves::Bare(*line) };
                return Flow { leaves: Some(how), never: true, ret: Some((v.as_ref(), *line)), ..Flow::default() };
            }
            Stmt::Fn(d) => self.function(d, Whose::Named(&d.name), true),
            Stmt::Route(r) => {
                self.fns.push((Whose::Route, None));
                match &r.body {
                    Expr::Block(stmts) => self.stmts(stmts, Goes::End),
                    e => self.value(e, Goes::Out),
                };
                self.fns.pop();
            }
            Stmt::Service(_, _, _, Some(e), _, _) => {
                self.value(e, Goes::Kept);
            }
            Stmt::Type(td) => td.defaults.iter().for_each(|(_, e)| {
                self.value(e, Goes::Kept);
            }),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(_, _, _, None, _, _) => {}
        }
        Flow::default()
    }

    /// A way of an `if` or `match`, or a `for`'s body: its statements, where its value goes.
    /// What a `return` of its own means is said by what it is a way of.
    fn way(&mut self, e: &'a Expr, goes: Goes) -> Flow<'a> {
        match e {
            Expr::Block(stmts) => self.stmts(stmts, goes),
            e => self.value(e, goes),
        }
    }

    /// A `return` of a block whose value is kept: it leaves the function (Q-678).
    fn kept_return(&mut self, v: Option<&Expr>, line: u32, what: &str) {
        let f = self.name();
        // Each way leaves: the value may have been meant (`return` written by habit), or the
        // leaving, with the `if` meant as a statement (review of 1a1b3ae, M1).
        let give = match v.map(written) {
            Some(w) if w != "..." => format!("write `{}`", w),
            Some(_) => "write the value without `return`".to_string(),
            None => "write the value".to_string(),
        };
        let msg = match what {
            "for" => format!("this `return` leaves {} in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so {}", f, give),
            "block" => format!("this `return` leaves {}, so the block never gives the value it is used for: to give it, {}; to leave, write the `return` as a statement", f, give),
            _ => format!("every way of this `{}` leaves {}, so the value it is used for is never given: to give it, {}; to leave, write the `{}` as a statement", what, f, give, what),
        };
        self.said.push((line, msg));
    }

    /// An `if` and the `else if`s after it, read as the ways of one `if` (review of 1a1b3ae,
    /// M2): read each as an `if` of its own, a way that leaves was hidden in the next, and one
    /// `let` was said once a level.
    fn chain(&mut self, c: &'a Expr, a: &'a Expr, b: Option<&'a Expr>, goes: Goes) -> Chain<'a> {
        self.value(c, Goes::Kept);
        let fa = self.way(a, goes);
        let gives = way_gives(a);
        let mut r = match b {
            Some(Expr::If(c2, a2, b2)) => {
                let r = self.deep(|r| r.chain(c2, a2, b2.as_deref(), goes));
                Chain { all: fa.leaves.is_some() && r.all, end: ends_of(fa.end, r.end, gives), said: r.said, first: None, open: fa.leaves.is_some() && r.open, ret: r.ret }
            }
            Some(b) => {
                let fb = self.way(b, goes);
                Chain { all: fa.leaves.is_some() && fb.leaves.is_some(), end: ends_of(fa.end, fb.end, gives), said: fb.said, first: None, open: false, ret: fb.ret }
            }
            None => Chain {
                all: false,
                end: fa.end.or(Some(End::NoElse(crate::debug::expr_line(c), gives))),
                said: false,
                first: None,
                open: fa.leaves.is_some(),
                ret: None,
            },
        };
        r.said |= fa.said;
        r.first = fa.leaves;
        r.ret = fa.ret.or(r.ret);
        r
    }

    /// On a new piece of stack when this one runs low, as `Checker::expr`: a program nested as
    /// deep as the parser lets it is read on a request's stack and less (tests/security.rs's
    /// small-stack test stopped the process in a debug build; FEEDBACK52.md, Q-678 to Q-680;
    /// branch feedback52).
    fn value(&mut self, e: &'a Expr, goes: Goes) -> Flow<'a> {
        self.deep(|r| r.value_here(e, goes))
    }

    /// `f` one level deeper, on a new piece of stack when this one runs low. Looked at every
    /// eighth level only, so that a node costs no look at the stack's limit (a thread-local);
    /// seven levels of the walk take far less than the 256 KB left.
    fn deep<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.depth += 1;
        let r = if self.depth % 8 == 0 { crate::stack::maybe_grow(256 << 10, 4 << 20, || f(self)) } else { f(self) };
        self.depth -= 1;
        r
    }

    fn value_here(&mut self, e: &'a Expr, goes: Goes) -> Flow<'a> {
        const KEPT: Goes = Goes::Kept;
        match e {
            Expr::Lit(_) | Expr::Ident(..) => {}
            Expr::Template(parts) => {
                for p in parts {
                    if let TPart::Expr(x) = p {
                        self.value(x, KEPT);
                    }
                }
            }
            Expr::List(xs) => xs.iter().for_each(|x| {
                self.value(x, KEPT);
            }),
            Expr::Map(es) => es.iter().for_each(|(_, x)| {
                self.value(x, KEPT);
            }),
            Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => {
                self.value(x, KEPT);
            }
            // `find(xs) ?? return null`, `ok || return false`: the right runs on one side only,
            // the early exit the guide shows.
            Expr::Coalesce(a, b) | Expr::Binary(BinOp::And | BinOp::Or, a, b, _) => {
                self.value(a, KEPT);
                self.way(b, Goes::Inner);
            }
            Expr::Binary(_, a, b, _) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
                self.value(a, KEPT);
                self.value(b, KEPT);
            }
            Expr::Call(c, args, _) => {
                self.value(c, KEPT);
                args.iter().for_each(|x| {
                    self.value(x, KEPT);
                });
            }
            // Where its value is kept, an `if` is said when every way leaves, so that it never
            // gives that value. One way's `return` is a guard's early exit, `let d = if b == 0 {
            // return 0 } else { a / b }`, whatever it gives (review of 1a1b3ae, M1).
            Expr::If(c, a, b) => {
                let r = self.chain(c, a, b.as_deref(), if goes == KEPT { Goes::Inner } else { goes });
                let mut said = r.said;
                if let (true, Some((v, line))) = (goes == KEPT && r.all && !said, r.ret) {
                    self.kept_return(v, line, "if");
                    said = true;
                }
                let leaves = r.all.then(|| match crate::debug::expr_line(c) {
                    0 => Leaves::If(r.first.map_or(0, Leaves::line)),
                    l => Leaves::If(l),
                });
                let ret = if r.all { r.ret } else { None };
                return Flow { leaves, never: false, end: r.end, empty_rounds: r.open, said, ret };
            }
            Expr::Match(s, arms, line) => {
                self.value(s, KEPT);
                let (mut all, mut end, mut said, mut ret) = (!arms.is_empty(), None, false, None);
                for a in arms {
                    if let Some(g) = &a.guard {
                        self.value(g, KEPT);
                    }
                    let f = self.way(&a.body, if goes == KEPT { Goes::Inner } else { goes });
                    all &= f.leaves.is_some();
                    end = end.or(f.end);
                    said |= f.said;
                    ret = ret.or(f.ret);
                }
                if let (true, Some((v, l))) = (goes == KEPT && all && !said, ret) {
                    self.kept_return(v, l, "match");
                    said = true;
                }
                // A `match` no arm of which fits fails, which leaves too.
                return Flow { leaves: all.then_some(Leaves::Match(*line)), end, said, ret: if all { ret } else { None }, ..Flow::default() };
            }
            Expr::For(_, items, body, line) => {
                self.value(items, KEPT);
                // Its list is kept: a `return` in its body leaves in the first round. Last in
                // a function, it may be a loop that finds, `for x in xs { if ... { return x } }`.
                let kept = matches!(goes, Goes::Kept | Goes::Out | Goes::Inner);
                let mut said = false;
                if let (true, Some((v, l))) = (kept, own_return(body)) {
                    self.kept_return(v, l, "for");
                    said = true;
                }
                let f = self.way(body, if kept { KEPT } else { Goes::Dropped });
                // A `for` whose rounds give nothing makes a list of nulls, which is never what is
                // meant: a loop that finds. One whose rounds give a value makes the list that is
                // meant (`return []` first, then `for x in xs => ...`).
                return Flow { end: f.empty_rounds.then_some(End::List(*line)), said, ..Flow::default() };
            }
            Expr::While(c, body, line) => {
                self.value(c, KEPT);
                self.way(body, Goes::Dropped);
                let forever = matches!(**c, Expr::Lit(Lit::Bool(true)));
                return Flow { end: (!forever).then_some(End::Null(*line)), ..Flow::default() };
            }
            Expr::Block(stmts) => {
                let mut said = false;
                if let (true, Some((v, line))) = (goes == KEPT, own_return(e)) {
                    self.kept_return(v, line, "block");
                    said = true;
                }
                let mut f = self.stmts(stmts, goes);
                f.said |= said;
                return f;
            }
            Expr::Lambda(d) => self.function(d, Whose::Arrow, false),
            Expr::Reply(s, b, line) => {
                self.value(s, KEPT);
                if let Some(b) = b {
                    self.value(b, KEPT);
                }
                return Flow { leaves: Some(Leaves::Reply(*line)), ..Flow::default() };
            }
            Expr::Html(el, _) => self.html(el),
        }
        Flow::default()
    }

    fn html(&mut self, el: &'a HtmlEl) {
        if let Some(id) = &el.id {
            self.value(id, Goes::Kept);
        }
        el.classes.iter().for_each(|c| {
            self.value(c, Goes::Kept);
        });
        for (_, v) in &el.attrs {
            if let AttrVal::Expr(e) | AttrVal::Spread(e) = v {
                self.value(e, Goes::Kept);
            }
        }
        for c in &el.children {
            match c {
                HtmlChild::El(e) => self.deep(|r| r.html(e)),
                // `{ return ... }` as an element's child is said as pasted text (Q-448).
                HtmlChild::Expr(x) => {
                    self.way(x, Goes::Kept);
                }
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
        Stmt::Return(_, l) | Stmt::Mount(_, _, l) | Stmt::Service(_, _, _, _, l, _) => *l,
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

/// The first reason seen, kept (FEEDBACK56.md, Q-858; branch feedback56).
fn note(slot: &mut Option<String>, why: String) {
    if slot.is_none() {
        *slot = Some(why);
    }
}

/// Whether a value of type `t` holds no function: text, numbers, booleans, null, and lists and
/// maps of them whose items and fields are known (FEEDBACK56.md, Q-858; branch feedback56).
fn plain_data(t: T) -> bool {
    const SCALAR: u16 = NIL | BOOL | INT | FLOAT | STR | DEC | ERR;
    if t.vague() || t.bits == 0 || t.bits & !(SCALAR | LIST | MAP) != 0 {
        return false;
    }
    if t.bits & LIST != 0 && !(t.elem != 0 && plain_data(elem_of(t.elem))) {
        return false;
    }
    t.bits & MAP == 0 || shape_of(t.shape).is_some_and(|s| s.iter().all(|(_, f)| plain_data(*f)))
}

/// The names a type is made of: `list<Ticket>`, `{ t: Ticket }`, `Ticket?` (FEEDBACK56.md,
/// Q-858, found in review; branch feedback56).
fn ty_names(t: &Ty, out: &mut Vec<String>) {
    out.extend(t.names.iter().filter(|n| crate::ast::is_type_name(n)).cloned());
    if let Some(e) = &t.elem {
        ty_names(e, out);
    }
    for (_, f) in t.fields.iter().flatten() {
        ty_names(f, out);
    }
}

/// The types a pattern makes a value into: `Ticket as t`, `[Ticket]`, `{ t: Ticket }`.
fn pattern_type_names(p: &Pattern, todo: &mut Vec<Next>) {
    match p {
        Pattern::Type(n) => todo.push(Next::Type(n.clone())),
        Pattern::Alt(ps) | Pattern::List(ps, _) => ps.iter().for_each(|q| pattern_type_names(q, todo)),
        Pattern::As(q, _) => pattern_type_names(q, todo),
        Pattern::Record(fields) => fields.iter().for_each(|f| pattern_type_names(&f.pat, todo)),
        Pattern::Wild | Pattern::Bind(_) | Pattern::Lit(_) => {}
    }
}

/// A statement written in the program: a string, or a template without `${...}`.
fn sql_written(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(Lit::Str(s)) => Some(s.to_string()),
        Expr::Template(parts) => parts.iter().map(|p| if let TPart::Lit(s) = p { Some(s.as_str()) } else { None }).collect(),
        _ => None,
    }
}

/// Whether a statement is one `select` that only reads, as SQLite, PostgreSQL and MySQL each read
/// it (`sql::Scan::reads_only`): no `for update`, `into`, `nextval(...)` or other function of the
/// database's that writes. A `with` before it counts as a write (FEEDBACK56.md, Q-858; branch
/// feedback56).
/// The functions it calls are looked at apart (`sql::foreign_call`).
fn reads_written(sql: &str) -> bool {
    crate::sql::Dialect::ALL.iter().all(|d| {
        let scan = crate::sql::scan(*d, sql);
        scan.reads_only() && scan.first_words().is_some_and(|(w, _)| w.eq_ignore_ascii_case("select"))
    })
}

/// Whether SQL written in the program surely writes, as SQLite, PostgreSQL and MySQL each read it:
/// not one that only reads (`sql::Scan::reads_only`, what the prefetch guard asks at run time), and
/// it names a table it writes (`insert into`, `update`, `delete from`, `create`, `alter` or `drop
/// table`, `truncate`) or a statement of it starts with a word that changes the database. A
/// `select ... for update`, a `select nextval(...)` or a `pragma` is no sure write (FEEDBACK70.md,
/// Q-965; branch security).
fn writes_written(sql: &str) -> bool {
    const CHANGING: &[&str] = &["insert", "update", "delete", "replace", "merge", "create", "alter", "drop", "truncate"];
    crate::sql::Dialect::ALL.iter().all(|d| {
        let scan = crate::sql::scan(*d, sql);
        !scan.reads_only() && (!scan.tables.writes.is_empty() || scan.heads().any(|(a, _)| CHANGING.iter().any(|w| a.eq_ignore_ascii_case(w))))
    })
}

/// SQL as a message shows it: on one line, cut after 60 characters.
fn sql_shown(sql: &str) -> String {
    let one: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(60) {
        Some((at, _)) => format!("{}...", &one[..at]),
        None => one,
    }
}

/// The builtins called by name that change something, as the prefetch guard refuses them
/// (`interp::CHANGES`); `db.*`, `http.*`, `image.convert`, `notification.send` and a service's
/// functions are read in `Checker::get_call`, and `db.atomic` is what its function does
/// (FEEDBACK70.md, Q-965; branch security).
const GET_WRITES: &[&str] = &["idempotent", "set_cookie", "delete_cookie", "set_session", "end_session", "mcp_token", "revoke_mcp_token", "write_file", "delete_file", "save_upload", "publish"];

/// What one function's own code, or a route's, surely does, read once (`GetWalk`): its writes and
/// the lines they are on, its first read of who the visitor is, and the program's functions it
/// calls, hands on or shows (a component), in the order written (FEEDBACK70.md, Q-965; branch
/// security).
#[derive(Debug, Default)]
struct GetOwn {
    writes: Vec<(String, u32)>,
    reads: Option<String>,
    calls: Vec<String>,
}

/// What a route or a `server fn` surely does with the functions it calls (`Checker::get_facts`):
/// each write it reaches, nearest first, with the function it is in (None: its own code), file
/// and line; and its nearest read of who the visitor is, with the function it is in.
#[derive(Debug, Default, Clone)]
struct GetFacts {
    writes: Vec<GetWrite>,
    reads: Option<(String, Option<String>)>,
}

#[derive(Debug, Clone, PartialEq)]
struct GetWrite {
    what: String,
    at: Option<String>,
    file: Arc<str>,
    line: u32,
}

impl GetWrite {
    /// As a message says it: `db.exec("...")`, in `seen`.
    fn shown(&self) -> String {
        match &self.at {
            Some(f) => format!("{}, in `{}`", self.what, f),
            None => self.what.clone(),
        }
    }
}

/// The request's headers or cookies a request pattern takes (`get_writes_said`): a record's field,
/// or one an alternative's record takes, or one under `as`. A pattern of alternatives was not
/// looked into, so `{ cookies: c } | { headers: h }` read no one (coverage of 0.8.7, Q-1104;
/// branch coverage-087).
fn pattern_takes(p: &Pattern) -> Option<String> {
    match p {
        Pattern::Record(fs) => fs.iter().filter_map(|f| f.path.first()).find(|f| matches!(f.as_str(), "headers" | "cookies")).cloned(),
        Pattern::Alt(ps) => ps.iter().find_map(pattern_takes),
        Pattern::As(p, _) => pattern_takes(p),
        _ => None,
    }
}

/// One route's warning: its write(s) and its read of who the visitor is.
fn get_one(route: &(Arc<str>, u32, String, String), writes: &str) -> (Arc<str>, u32, String) {
    let (file, line, shown, read) = route;
    (file.clone(), *line, format!(
        "`{}` writes ({}) and reads who the visitor is ({}): a link on another site, in an email or in a chat runs it with their session (`SameSite=Lax` cookies go with a followed link). Make it a `POST`, or, if the write is meant (a confirm-your-address link, a sign-in callback, a presence ping), tag it: `route writes GET ...`",
        shown, writes, read
    ))
}

/// What reading a function's code for `GetOwn` needs of the program: its functions, the files they
/// are in, its top-level `let`s, and those of them that hold a connection (`db.connect`).
struct GetCx<'a> {
    fns: &'a HashMap<String, Arc<FnDef>>,
    files: &'a HashMap<String, Arc<str>>,
    top_lets: &'a HashSet<String>,
    conns: &'a HashSet<String>,
}

/// Each function's own code as read, and whether it can reach a write and a read of who the visitor
/// is, through the functions it calls (`Checker::reaches`): kept for the check, so a function many
/// routes reach is read once.
#[derive(Default)]
struct GetSeen {
    own: HashMap<String, Arc<GetOwn>>,
    reach: HashMap<String, (bool, bool)>,
}

/// The program's functions by name, the files they are in, and the top-level `let`s that hold a
/// connection, for `GetCx`.
fn get_program(order: &[Item]) -> (HashMap<String, Arc<FnDef>>, HashMap<String, Arc<str>>, HashSet<String>) {
    let (mut fns, mut files, mut conns) = (HashMap::new(), HashMap::new(), HashSet::new());
    for it in order {
        match &it.stmt {
            Stmt::Fn(d) => {
                fns.insert(d.name.clone(), d.clone());
                files.insert(d.name.clone(), it.file.clone());
            }
            Stmt::Let(Pattern::Bind(n), e, _) if is_connect(e) => {
                conns.insert(n.to_string());
            }
            _ => {}
        }
    }
    (fns, files, conns)
}

/// A function's code as it runs: its parameters' defaults, then its body.
fn fn_code(d: &FnDef) -> Vec<&Expr> {
    d.params.iter().filter_map(|p| p.default.as_ref()).chain(std::iter::once(&d.body)).collect()
}

/// `o.name`, `o?.name` or `o["name"]`: the object, past a `?`, and the name.
fn field_of(x: &Expr) -> Option<(&Expr, &str)> {
    let (o, n) = match x {
        Expr::Field(o, n, _) => (&**o, n.as_str()),
        Expr::Index(o, k, _) => match &**k {
            Expr::Lit(Lit::Str(n)) => (&**o, &**n),
            _ => return None,
        },
        _ => return None,
    };
    let mut o = o;
    while let Expr::Try(inner) = o {
        o = inner;
    }
    Some((o, n))
}

/// A field of the request that says nothing of who the visitor is: nobody's
/// (`parser::NOBODYS_FIELDS`, what a cache keeps pages by or what was sent), and `ip`, the address,
/// which a link on another site gives nothing of the visitor's to write as.
fn nobodys_field(f: &str) -> bool {
    crate::parser::NOBODYS_FIELDS.contains(&f) || f == "ip"
}

/// What a name the code binds stands for where it is in scope (`GetWalk`).
#[derive(Debug, Clone, Copy)]
enum Bound {
    Plain,
    /// `let c = db.connect(...)`: its functions are `db`'s.
    Conn,
    /// `let f = () => ...`, a local `fn`: read where it is called or handed on.
    Held(usize),
    /// `let m = "POST"`, `let q = "insert ..."`: read as the text it holds until something assigns
    /// it (`GetWalk::texts`; FEEDBACK75.md, Q-1084; branch feedback75).
    Text(usize),
    /// `let w = save_visit`: the program's function, called where it is called or handed on, until
    /// something assigns it (`GetWalk::aliases`; FEEDBACK75.md, Q-1084; branch feedback75).
    Alias(usize),
}

/// One walk of a function's or a route's code for `GetOwn`, in the order it runs, each name the code
/// binds in scope only where it is bound: after its `let`, in its lambda, its loop, its arm. Left
/// out, as code that runs elsewhere or later: a handler written in markup (`@click`), a lambda given
/// as an attribute, and a lambda a `let` holds until the code calls it or hands it on. A
/// component's props, and what a component the page shows runs to be made, run here. The request
/// is read as `parser::request_use` reads it (Q-886): going anywhere but to a field of nobody's,
/// `ip` and `headers.host` taken as nobody's too; its headers and cookies named where the text
/// names them (FEEDBACK70.md, Q-965, second review; branch security).
struct GetWalk<'a, 'c> {
    ck: &'a Checker,
    cx: &'a GetCx<'c>,
    scope: Vec<(String, Bound)>,
    /// The names that are the request where nothing binds them.
    names: &'a [&'a str],
    o: GetOwn,
    /// What reading the request at each of its names is (None: nobody's).
    said: HashMap<*const Expr, Option<String>>,
    /// The reads of the request, in order.
    uses: Vec<String>,
    held: Vec<Arc<FnDef>>,
    held_read: HashSet<usize>,
    /// What `Bound::Text` and `Bound::Alias` stand for, and those assigned since, which stand for
    /// nothing known from there on: the code is read in the order it runs, a held lambda where it is
    /// called, so a use read before the assignment runs before it (a loop's first round).
    texts: Vec<Expr>,
    aliases: Vec<String>,
    spoiled: HashSet<(bool, usize)>,
}

impl<'a, 'c> GetWalk<'a, 'c> {
    fn new(ck: &'a Checker, cx: &'a GetCx<'c>, names: &'a [&'a str], params: &[String]) -> Self {
        GetWalk { ck, cx, scope: params.iter().map(|p| (p.clone(), Bound::Plain)).collect(), names, o: GetOwn::default(), said: HashMap::new(), uses: Vec::new(), held: Vec::new(), held_read: HashSet::new(), texts: Vec::new(), aliases: Vec::new(), spoiled: HashSet::new() }
    }

    /// Whether a `let` of `init` is followed (`Bound::Text`, `Bound::Alias`): a text as written,
    /// or a name for a function, a held lambda, such a text or a connection (FEEDBACK75.md,
    /// Q-1084; branch feedback75).
    fn follows(&self, init: &Expr) -> bool {
        match init {
            Expr::Lit(Lit::Str(_)) => true,
            Expr::Template(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_))),
            Expr::Ident(f, _) => match self.bound(f.as_str()) {
                Some(b) => !matches!(b, Bound::Plain),
                None => self.cx.fns.contains_key(f.as_str()),
            },
            _ => false,
        }
    }

    /// The text `e` surely is: written in place, or held by a `let` nothing assigns again
    /// (FEEDBACK75.md, Q-1084; branch feedback75).
    fn text_of<'e>(&'e self, e: &'e Expr) -> &'e Expr {
        match e {
            Expr::Ident(n, _) => match self.bound(n.as_str()) {
                Some(Bound::Text(k)) if !self.spoiled.contains(&(true, k)) => &self.texts[k],
                _ => e,
            },
            e => e,
        }
    }

    /// The program's function a name stands for, if it still does.
    fn alias(&self, k: usize) -> Option<String> {
        (!self.spoiled.contains(&(false, k))).then(|| self.aliases[k].clone())
    }

    fn bound(&self, n: &str) -> Option<Bound> {
        self.scope.iter().rev().find(|(m, _)| m == n).map(|(_, b)| *b)
    }

    /// Whether `e` is the request, by one of its names nothing here binds.
    fn is_request(&self, e: &Expr) -> bool {
        matches!(e, Expr::Ident(n, _) if self.names.contains(&n.as_str()) && self.bound(n.as_str()).is_none())
    }

    fn write(&mut self, what: String, line: u32) {
        self.o.writes.push((what, line));
    }

    fn exprs(&mut self, code: &[&Expr]) {
        for e in code {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &Expr) {
        self.request_at(e);
        match e {
            Expr::Lit(_) => {}
            Expr::Ident(n, _) => {
                if self.is_request(e) {
                    let what = match self.said.get(&(e as *const Expr)) {
                        Some(None) => None,
                        Some(Some(w)) => Some(w.clone()),
                        None => Some("the request".to_string()),
                    };
                    self.uses.extend(what);
                }
                let _ = n;
            }
            Expr::Template(parts) => {
                for p in parts {
                    if let TPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            Expr::List(xs) => xs.iter().for_each(|x| self.expr(x)),
            Expr::Map(es) => es.iter().for_each(|(_, x)| self.expr(x)),
            Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => self.expr(x),
            Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Index(a, b, _) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::Pipe(lhs, rhs, line) => {
                self.expr(lhs);
                match &**rhs {
                    Expr::Call(c, args, l) => {
                        self.call(c, Some(lhs), args, *l);
                        self.expr(c);
                        args.iter().for_each(|a| self.expr(a));
                    }
                    other => {
                        self.call(other, Some(lhs), &[], *line);
                        self.expr(other);
                    }
                }
            }
            Expr::Call(c, args, line) => {
                self.call(c, None, args, *line);
                self.expr(c);
                // What `every` and `schedule` are given runs later (in a component, in the browser:
                // the page's clock), and a channel's or a port's handler when a message comes.
                let later = match &**c {
                    Expr::Ident(n, _) => matches!(n.as_str(), "every" | "schedule") && self.bound(n.as_str()).is_none(),
                    Expr::Field(o, f, _) => matches!(&**o, Expr::Ident(m, _) if self.bound(m.as_str()).is_none() && matches!((m.as_str(), f.as_str()), ("channel", "on") | ("udp", "listen"))),
                    _ => false,
                };
                args.iter().filter(|a| !(later && matches!(a, Expr::Lambda(_)))).for_each(|a| self.expr(a));
            }
            Expr::If(c, a, b) => {
                self.expr(c);
                self.expr(a);
                if let Some(b) = b {
                    self.expr(b);
                }
            }
            Expr::Match(s, arms, _) => {
                self.expr(s);
                for a in arms {
                    let mark = self.scope.len();
                    self.pattern(&a.pat);
                    self.bind_pattern(&a.pat, Bound::Plain);
                    if let Some(g) = &a.guard {
                        self.expr(g);
                    }
                    self.expr(&a.body);
                    self.scope.truncate(mark);
                }
            }
            Expr::For(p, i, b, _) => {
                self.expr(i);
                let mark = self.scope.len();
                self.pattern(p);
                self.bind_pattern(p, Bound::Plain);
                self.expr(b);
                self.scope.truncate(mark);
            }
            Expr::While(c, b, _) => {
                self.expr(c);
                self.expr(b);
            }
            Expr::Block(stmts) => self.block(stmts),
            Expr::Lambda(d) => self.lambda(d),
            Expr::Reply(s, b, _) => {
                self.expr(s);
                if let Some(b) = b {
                    self.expr(b);
                }
            }
            Expr::Html(el, _) => self.html(el),
        }
    }

    /// The names a pattern binds, in scope from here.
    fn bind_pattern(&mut self, p: &Pattern, b: Bound) {
        let mut names = Vec::new();
        pattern_names(p, &mut names);
        self.scope.extend(names.into_iter().map(|n| (n, b)));
    }

    /// A pattern's defaults, which run where it matches.
    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Record(fields) => {
                for f in fields {
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                    self.pattern(&f.pat);
                }
            }
            Pattern::List(ps, _) | Pattern::Alt(ps) => ps.iter().for_each(|p| self.pattern(p)),
            Pattern::As(p, _) => self.pattern(p),
            Pattern::Bind(_) | Pattern::Wild | Pattern::Lit(_) | Pattern::Type(_) => {}
        }
    }

    fn block(&mut self, stmts: &[Stmt]) {
        let mark = self.scope.len();
        for (k, s) in stmts.iter().enumerate() {
            // A confined check past its budget reads no further: each name was looked for among
            // all those bound before it, and a route calling a function of 85,000 `let`s went on
            // 8 s past the budget (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1127; branch
            // feedback75).
            if budget_spent(k) {
                break;
            }
            match s {
                Stmt::Let(Pattern::Bind(n), Expr::Lambda(d), _) => {
                    self.held.push(d.clone());
                    self.scope.push((n.to_string(), Bound::Held(self.held.len() - 1)));
                }
                Stmt::Let(Pattern::Bind(n), init, _) if self.follows(init) => {
                    self.expr(init);
                    let b = match init {
                        Expr::Ident(f, _) => match self.bound(f.as_str()) {
                            Some(b) => b,
                            None => {
                                self.aliases.push(f.to_string());
                                Bound::Alias(self.aliases.len() - 1)
                            }
                        },
                        text => {
                            self.texts.push(text.clone());
                            Bound::Text(self.texts.len() - 1)
                        }
                    };
                    self.scope.push((n.to_string(), b));
                }
                Stmt::Let(p, init, _) => {
                    // `let { cookies } = request`: what it takes.
                    if let Pattern::Record(fields) = p {
                        if self.is_request(init) {
                            let what = fields.iter().filter_map(|f| f.path.first()).find(|f| !nobodys_field(f)).map(|f| match f.as_str() {
                                "headers" | "cookies" => format!("the request's {}", f),
                                f => format!("`request.{}`", f),
                            });
                            self.said.entry(init as *const Expr).or_insert(what);
                        }
                    }
                    self.expr(init);
                    self.pattern(p);
                    let b = if matches!(p, Pattern::Bind(_)) && is_connect(init) { Bound::Conn } else { Bound::Plain };
                    self.bind_pattern(p, b);
                }
                Stmt::Assign(n, _, e, line) => {
                    self.expr(e);
                    // What it held is known no more (FEEDBACK75.md, Q-1084; branch feedback75).
                    match self.bound(n.as_str()) {
                        Some(Bound::Text(k)) => {
                            self.spoiled.insert((true, k));
                        }
                        Some(Bound::Alias(k)) => {
                            self.spoiled.insert((false, k));
                        }
                        _ => {}
                    }
                    if self.cx.top_lets.contains(n.as_str()) && self.bound(n.as_str()).is_none() {
                        self.write(format!("assigns the program's variable `{}`", n), *line);
                    }
                }
                Stmt::SetField(o, _, _, e, _) => {
                    self.expr(o);
                    self.expr(e);
                }
                Stmt::Expr(e) | Stmt::Return(Some(e), _) => self.expr(e),
                Stmt::State(n, e, _) => {
                    self.expr(e);
                    self.scope.push((n.clone(), Bound::Plain));
                }
                Stmt::Fn(d) => {
                    self.held.push(d.clone());
                    self.scope.push((d.name.clone(), Bound::Held(self.held.len() - 1)));
                }
                _ => {}
            }
        }
        self.scope.truncate(mark);
    }

    fn lambda(&mut self, d: &FnDef) {
        let mark = self.scope.len();
        self.scope.extend(d.params.iter().map(|p| (p.name.to_string(), Bound::Plain)));
        let code = fn_code(d);
        self.exprs(&code);
        self.scope.truncate(mark);
    }

    /// A held lambda, called or handed on: read once, where it is.
    fn read_held(&mut self, k: usize) {
        if self.held_read.insert(k) {
            let d = self.held[k].clone();
            self.lambda(&d);
        }
    }

    fn html(&mut self, el: &HtmlEl) {
        if let Some(id) = &el.id {
            self.expr(id);
        }
        el.classes.iter().for_each(|c| self.expr(c));
        for (name, v) in &el.attrs {
            if let AttrVal::Expr(v) | AttrVal::Spread(v) = v {
                if !name.starts_with('@') && !matches!(v, Expr::Lambda(_)) {
                    self.expr(v);
                }
            }
        }
        // A component the page shows: what the server runs to make it.
        if el.tag.starts_with(char::is_uppercase) && self.bound(&el.tag).is_none() && self.cx.fns.contains_key(el.tag.as_str()) {
            self.o.calls.push(el.tag.clone());
        }
        for c in &el.children {
            match c {
                HtmlChild::El(e) => self.html(e),
                HtmlChild::Expr(x) => self.expr(x),
            }
        }
    }

    /// Before `e` is read: what reading the request there is, the outermost first
    /// (`request.headers["user-agent"]`, the request handed to a function, to a group's middleware).
    fn request_at(&mut self, e: &Expr) {
        if let Some((o, name)) = field_of(e) {
            if let Some((r, kind)) = field_of(o).filter(|(r, k)| self.is_request(r) && matches!(*k, "headers" | "cookies")) {
                let what = (kind == "cookies" || !name.eq_ignore_ascii_case("host")).then(|| format!("the `{}` {}", name, if kind == "headers" { "header" } else { "cookie" }));
                self.said.entry(r as *const Expr).or_insert(what);
            } else if self.is_request(o) {
                let what = match name {
                    f if nobodys_field(f) => None,
                    "headers" | "cookies" => Some(format!("the request's {}", name)),
                    f => Some(format!("`request.{}`", f)),
                };
                self.said.entry(o as *const Expr).or_insert(what);
            }
        }
        if let Expr::Call(c, args, _) = e {
            let callee = match &**c {
                Expr::Ident(n, _) => n.to_string(),
                Expr::Field(_, n, _) => n.clone(),
                _ => String::new(),
            };
            // The request handed to a group's middleware is the middleware's to read.
            if callee == "__answer" {
                if let Some(Expr::Call(_, given, _)) = args.first() {
                    let mine: Vec<*const Expr> = given.iter().filter(|a| self.is_request(a)).map(|a| a as *const Expr).collect();
                    for a in mine {
                        self.said.insert(a, None);
                    }
                }
            }
            let handed: Vec<*const Expr> = args.iter().filter(|a| self.is_request(a)).map(|a| a as *const Expr).collect();
            for a in handed {
                let what = if callee.is_empty() { "the request".to_string() } else { format!("the request, given to `{}`", callee) };
                self.said.entry(a).or_insert(Some(what));
            }
        }
    }

    /// A call: of a function of the program's (followed by `get_facts`), of a held lambda, of a
    /// builtin that writes or reads who the visitor is, of a connection's or a service's function,
    /// or a group's middleware; and the functions it is handed (`db.transaction(save)`).
    fn call(&mut self, callee: &Expr, piped: Option<&Expr>, args: &[Expr], line: u32) {
        for a in piped.into_iter().chain(args.iter()) {
            if let Expr::Ident(n, _) = a {
                match self.bound(n.as_str()) {
                    Some(Bound::Held(k)) => self.read_held(k),
                    Some(Bound::Alias(k)) => self.o.calls.extend(self.alias(k)),
                    Some(_) => {}
                    None if self.cx.fns.contains_key(n.as_str()) => self.o.calls.push(n.to_string()),
                    None => {}
                }
            }
        }
        let first = piped.or(args.first());
        match callee {
            Expr::Ident(n, _) => match self.bound(n.as_str()) {
                Some(Bound::Held(k)) => self.read_held(k),
                Some(Bound::Alias(k)) => self.o.calls.extend(self.alias(k)),
                Some(_) => {}
                None if self.cx.fns.contains_key(n.as_str()) => self.o.calls.push(n.to_string()),
                None => match n.as_str() {
                    "session" | "cookie" => {
                        self.o.reads.get_or_insert_with(|| format!("`{}()`", n));
                    }
                    "__answer" => self.middleware(first),
                    n if GET_WRITES.contains(&n) => self.write(format!("`{}`", n), line),
                    _ => {}
                },
            },
            Expr::Field(obj, name, _) => match &**obj {
                Expr::Ident(m, _) => match self.bound(m.as_str()) {
                    // A connection's functions are `db`'s (`db::connection`).
                    Some(Bound::Conn) => self.db(name, first, &format!("{}.{}", m, name), line),
                    Some(_) => self.method(name, line),
                    None if self.cx.conns.contains(m.as_str()) => self.db(name, first, &format!("{}.{}", m, name), line),
                    None if self.ck.module_named(m) => {
                        let full = format!("{}.{}", m, name);
                        match full.as_str() {
                            f if f.starts_with("db.") => self.db(name, first, f, line),
                            "http.post" | "http.put" | "http.patch" | "http.delete" | "image.convert" | "notification.send" => self.write(format!("`{}`", full), line),
                            "http.request" => {
                                if let Some(Expr::Lit(Lit::Str(method))) = first.map(|f| self.text_of(f)) {
                                    if !["GET", "HEAD", "OPTIONS"].iter().any(|g| method.eq_ignore_ascii_case(g)) {
                                        let shown = format!("`http.request(\"{}\", ...)`", method);
                                        self.write(shown, line);
                                    }
                                }
                            }
                            // A stream sent with a method that writes, or a body, which makes it a
                            // POST; a packet; a message to a channel's pages, as the prefetch guard
                            // reads them (FEEDBACK75.md, Q-1084 and Q-1062; branch feedback75).
                            "http.stream" => {
                                let opts = if piped.is_some() { args.get(1) } else { args.get(2) };
                                if let Some(Expr::Map(es)) = opts {
                                    let method = es.iter().find(|(k, _)| k == "method").map(|(_, v)| self.text_of(v));
                                    let shown = match method {
                                        Some(Expr::Lit(Lit::Str(m))) if !["GET", "HEAD", "OPTIONS"].iter().any(|g| m.eq_ignore_ascii_case(g)) => Some(format!("`http.stream` with `method: \"{}\"`", m)),
                                        Some(Expr::Lit(Lit::Nil)) | None if es.iter().any(|(k, v)| k == "body" && !matches!(v, Expr::Lit(Lit::Nil))) => Some("`http.stream` with a `body`".to_string()),
                                        // A GET, or a method the text does not make certain.
                                        _ => None,
                                    };
                                    if let Some(shown) = shown {
                                        self.write(shown, line);
                                    }
                                }
                            }
                            "udp.send" | "udp.reply" | "channel.send" | "channel.broadcast" => self.write(format!("`{}`", full), line),
                            _ => {}
                        }
                    }
                    // A call of another program, unless its code is read and does not surely write.
                    None if self.ck.services.contains_key(m.as_str()) && !self.cx.fns.contains_key(m.as_str()) => {
                        if !self.ck.service_fn(m, name).is_some_and(|x| !x.sure_writes) {
                            self.write(format!("`{}.{}`, a call of the service `{}`", m, name, m), line);
                        }
                    }
                    None => self.method(name, line),
                },
                Expr::Field(inner, later, _) if later == "later" && matches!(&**inner, Expr::Ident(s, _) if self.ck.services.contains_key(s.as_str()) && self.bound(s.as_str()).is_none()) => {
                    if let Expr::Ident(s, _) = &**inner {
                        self.write(format!("`{}.later.{}`, a call of the service `{}` made later", s, name, s), line);
                    }
                }
                _ => self.method(name, line),
            },
            _ => {}
        }
    }

    /// `x.f(...)`, which is `f(x, ...)`.
    fn method(&mut self, name: &str, line: u32) {
        if self.cx.fns.contains_key(name) {
            self.o.calls.push(name.to_string());
        } else if GET_WRITES.contains(&name) {
            self.write(format!("`{}`", name), line);
        }
    }

    /// A group's middleware, handed the request: what it reads of it, written in place, a function
    /// of the program's, or one such a function makes (`| require("admin")`). Its own `session()`
    /// is found where its code is read.
    fn middleware(&mut self, first: Option<&Expr>) {
        let Some(Expr::Call(m, _, _)) = first else { return };
        let mut made: Vec<Arc<FnDef>> = Vec::new();
        match &**m {
            Expr::Lambda(d) => made.push(d.clone()),
            Expr::Ident(n, _) if self.bound(n.as_str()).is_none() => made.extend(self.cx.fns.get(n.as_str()).cloned()),
            Expr::Call(f, _, _) => {
                if let Some(d) = if let Expr::Ident(n, _) = &**f { self.cx.fns.get(n.as_str()) } else { None } {
                    walk_expr(&d.body, &mut |y| {
                        if let Expr::Lambda(l) = y {
                            made.push(l.clone());
                        }
                    });
                }
            }
            _ => {}
        }
        if self.o.reads.is_none() {
            self.o.reads = made.iter().find_map(|d| self.middleware_read(d)).map(|r| format!("{}, in its group's middleware", r));
        }
    }

    /// What a group's middleware reads of who the visitor is from the request it is handed, its
    /// first parameter: its uses there, but that handing it back is how it lets the route go on, as
    /// `parser::middleware_reads_visitor` reads it (Q-886). The first use that says what it reads.
    fn middleware_read(&self, d: &FnDef) -> Option<String> {
        let p = d.params.first()?.name.to_string();
        let names = [p.as_str()];
        let mut w = GetWalk::new(self.ck, self.cx, &names, &[]);
        w.exprs(&fn_code(d));
        if w.uses.len() as u32 <= crate::parser::given_back(&d.body, &p) {
            return None;
        }
        w.uses.iter().find(|u| *u != "the request").or(w.uses.first()).cloned()
    }

    /// A call of `db`'s function `name` (`shown` as written): a statement written in the program
    /// that only reads is no write; one built as it runs is `db.exec`'s, which writes; `db.query`
    /// and `db.one` write only where their written SQL surely does. `sql\`...\`` is its text.
    fn db(&mut self, name: &str, first: Option<&Expr>, shown: &str, line: u32) {
        // SQL a `let` holds is read as written there, for what may only read (FEEDBACK75.md,
        // Q-1084; branch feedback75); `db.exec` writes whatever its SQL a `let` holds says, and is
        // said as 0.8.8 said it.
        let first = if matches!(name, "query" | "one") { first.map(|e| self.text_of(e)) } else { first };
        let sql = first.and_then(|e| match e {
            Expr::Call(f, args, _) if args.len() == 1 && matches!(&**f, Expr::Ident(n, _) if n == "sql" && self.bound("sql").is_none() && !self.cx.fns.contains_key("sql")) => sql_written(&args[0]),
            e => sql_written(e),
        });
        match name {
            "exec" | "exec_many" => match sql {
                Some(sql) if reads_written(&sql) => {}
                Some(sql) => self.write(format!("`{}(\"{}\")`", shown, sql_shown(&sql)), line),
                None => self.write(format!("`{}`", shown), line),
            },
            "query" | "one" => {
                if let Some(sql) = sql.filter(|s| writes_written(s)) {
                    self.write(format!("`{}(\"{}\")`, SQL that writes", shown, sql_shown(&sql)), line);
                }
            }
            "migrate" => self.write(format!("`{}`", shown), line),
            _ => {}
        }
    }

    /// What the code was found to do: the request read where nothing else read who the visitor is.
    fn done(mut self) -> GetOwn {
        if self.o.reads.is_none() {
            self.o.reads = self.uses.into_iter().next();
        }
        self.o
    }
}

/// Where code reads what is the server's own, as a page state's warning says it
/// (`page_states_to_pages`): `env`, a file, or a `select *` row with every column.
/// The program's functions as `page_states_through_calls` and `page_states_to_pages` follow them:
/// for each, the page states it reads where nothing of its own binds the name (a parameter, a
/// `let`, a loop's), and who calls it, by a call, a pipe or a tag (`<Shows/>`, a view a component
/// shows) (FEEDBACK75.md, Q-1058, review; branch feedback75).
struct CallGraph<'a> {
    index: HashMap<&'a str, usize>,
    /// The first page state each reads itself (or, reading none, sets), and all it reads or sets.
    reads: Vec<Option<String>>,
    all_reads: Vec<Vec<String>>,
    /// Whether it only sets the state `reads` names.
    sets_only: Vec<bool>,
    callers: Vec<Vec<usize>>,
}

impl<'a> CallGraph<'a> {
    fn of(defs: impl Iterator<Item = &'a Arc<FnDef>>, state: &dyn Fn(&str) -> bool) -> CallGraph<'a> {
        use crate::client::Met;
        let defs: Vec<&'a Arc<FnDef>> = defs.collect();
        let index: HashMap<&'a str, usize> = defs.iter().enumerate().map(|(i, d)| (d.name.as_str(), i)).collect();
        let mut reads = Vec::with_capacity(defs.len());
        let mut all_reads = Vec::with_capacity(defs.len());
        let mut sets_only = Vec::with_capacity(defs.len());
        let mut callers: Vec<Vec<usize>> = vec![Vec::new(); defs.len()];
        for (i, d) in defs.iter().enumerate() {
            let mut read: Vec<String> = Vec::new();
            let mut first_read: Option<String> = None;
            let mut calls = Vec::new();
            called_names(&d.body, &mut calls);
            crate::client::free_fn(d, &mut crate::client::Bound::new(), &mut |m, _| {
                let (n, reading) = match m {
                    Met::Expr(Expr::Ident(n, _)) => (n.as_str(), true),
                    Met::Assigned(n) => (n, false),
                    Met::Tag(t) if t.starts_with(|c: char| c.is_ascii_uppercase()) => return calls.push(t.to_string()),
                    Met::Tag(_) | Met::Expr(_) => return,
                };
                if !state(n) {
                    return;
                }
                if reading && first_read.is_none() {
                    first_read = Some(n.to_string());
                }
                if !read.iter().any(|r| r == n) {
                    read.push(n.to_string());
                }
            });
            for c in calls {
                if let Some(&j) = index.get(c.as_str()) {
                    if j != i && callers[j].last() != Some(&i) {
                        callers[j].push(i);
                    }
                }
            }
            // The message says `sets` of one that only sets it (FEEDBACK75.md, Q-1058, recheck).
            sets_only.push(first_read.is_none() && !read.is_empty());
            reads.push(first_read.or_else(|| read.first().cloned()));
            all_reads.push(read);
        }
        CallGraph { index, reads, all_reads, sets_only, callers }
    }

    /// From `from` up to all who call them, each labelled with the one of `from` it came from: a
    /// work list, each function once.
    fn reach(&self, from: &mut dyn Iterator<Item = usize>, label: &mut [Option<usize>]) {
        let mut work: Vec<usize> = Vec::new();
        for i in from {
            if label[i].is_none() {
                label[i] = Some(i);
                work.push(i);
            }
        }
        while let Some(i) = work.pop() {
            for &c in &self.callers[i] {
                if label[c].is_none() {
                    label[c] = label[i];
                    work.push(c);
                }
            }
        }
    }
}

fn server_source(e: &Expr) -> Option<&'static str> {
    let mut found = None;
    walk_expr(e, &mut |x| {
        if found.is_some() {
            return;
        }
        found = match x {
            Expr::Ident(n, _) if n == "env" => Some("`env`"),
            Expr::Ident(n, _) if n == "read_file" => Some("a file (`read_file`)"),
            Expr::Lit(Lit::Str(s)) if selects_every_column(s) => Some("a `select *` row"),
            Expr::Template(parts) if parts.iter().any(|p| matches!(p, TPart::Lit(s) if selects_every_column(s))) => Some("a `select *` row"),
            _ => None,
        };
    });
    found
}

/// Whether SQL text selects every column: `select *` or `select t.*`, in any case.
fn selects_every_column(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    lower.match_indices("select").any(|(i, _)| {
        let rest = lower[i + 6..].trim_start();
        let rest = rest.strip_prefix("distinct").map_or(rest, str::trim_start);
        let word: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        rest.starts_with('*') || (!word.is_empty() && rest[word.len()..].starts_with(".*"))
    })
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
/// (branch class-links). And each rule that takes the keyboard's focus ring away, in `focus`
/// (FEEDBACK72.md, Q-1019; branch feedback72-81): of the sheets under public/ only for a program
/// that `serves` pages, as a script kept beside an app is not the app (Q-1012).
fn class_rules(order: &[Item], base: &Path, serves: bool, focus: &mut Vec<(Arc<str>, u32)>) -> Vec<(String, String, u32)> {
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
    let own = sheets.len();
    let mut files = program_files(&base.join("public"), usize::MAX, false, &|p| p.extension().is_some_and(|x| x == "css"));
    files.sort();
    for p in files {
        // A regular file of a bounded size, as a program's own file is read: a style sheet that
        // links to `/dev/zero` took the machine's memory (FEEDBACK71.md, Q-982; branch security).
        if let Ok(text) = crate::error::read_text(&p) {
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
    focus.extend(focus_rings_removed(if serves { &sheets } else { &sheets[..own] }));
    out
}

/// The files under `dir` that `keep` takes, as deep as eight folders and at most `most` of them,
/// `hidden` leaving out folders named with a dot and node_modules. What the server serves the check
/// reads: `dir` and the folders in it are followed wherever their links point (`public ->
/// ../shared/public`), each real folder once, so links to `.` or to each other end; five links
/// `site/sN -> .` took two minutes, 5^8 folders, before (found in reviews of FEEDBACK73.md's Q-1022;
/// branch feedback72-81). Read with a list of what is left, not by recursion.
fn program_files(dir: &Path, most: usize, hidden: bool, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(root) = dir.canonicalize() else { return out };
    let mut seen: HashSet<PathBuf> = HashSet::from([root.clone()]);
    let mut todo: Vec<(PathBuf, PathBuf, u32)> = vec![(dir.to_path_buf(), root, 0)];
    while let Some((at, real, depth)) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&at) else { continue };
        for e in rd.flatten() {
            if out.len() >= most {
                return out;
            }
            let p = e.path();
            let name = e.file_name();
            let Ok(ft) = e.file_type() else { continue };
            let is_dir = if ft.is_symlink() { std::fs::metadata(&p).is_ok_and(|m| m.is_dir()) } else { ft.is_dir() };
            if !is_dir {
                if keep(&p) {
                    out.push(p);
                }
                continue;
            }
            let n = name.to_string_lossy();
            if depth >= 8 || (hidden && (n.starts_with('.') || n == "node_modules")) {
                continue;
            }
            let inner = if ft.is_symlink() {
                match p.canonicalize() {
                    Ok(c) => c,
                    Err(_) => continue,
                }
            } else {
                real.join(&name)
            };
            if seen.insert(inner.clone()) {
                todo.push((p, inner, depth + 1));
            }
        }
    }
    out
}

/// Whether a program serves pages: it has a route or a mount, and markup, a component, or a
/// `files()`, `file()` or `markdown_page()` to answer with. Only such a program's checks read the
/// sheets under `public/`: `quillo check tool.qwe`, a script kept beside an app, was told of every
/// class the app's sheets style (FEEDBACK72.md, Q-1012; branch feedback72-81).
fn serves_pages(order: &[Item]) -> bool {
    let mut routes = false;
    for it in order {
        match &it.stmt {
            Stmt::Route(r) if r.method != crate::ast::GUARD_METHOD => {
                if matches!(r.body, Expr::Html(..)) {
                    return true;
                }
                routes = true;
            }
            Stmt::Mount(..) => routes = true,
            _ => {}
        }
    }
    if !routes {
        return false;
    }
    if order.iter().any(|it| matches!(&it.stmt, Stmt::Fn(d) if d.component)) {
        return true;
    }
    let mut pages = false;
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| match e {
            Expr::Html(..) => pages = true,
            Expr::Call(f, _, _) => pages |= matches!(&**f, Expr::Ident(n, _) if n == "files" || n == "file" || n == "markdown_page"),
            _ => {}
        });
        if pages {
            return true;
        }
    }
    false
}

/// Each rule of `sheets` that takes the focus ring away, `:focus { outline: none }`, where no rule
/// styles `:focus-visible`: the keyboard then shows nothing of where it is, on every element
/// (FEEDBACK72.md, Q-1019; branch feedback72-81). By file and line.
fn focus_rings_removed(sheets: &[(Arc<str>, u32, String)]) -> Vec<(Arc<str>, u32)> {
    if sheets.iter().any(|(_, _, t)| t.contains(":focus-visible")) || !sheets.iter().any(|(_, _, t)| t.contains(":focus")) {
        return Vec::new();
    }
    // Each rule: its selectors, in lower case and trimmed, its declarations without spaces, and
    // where it starts.
    let mut rules: Vec<(Vec<String>, String, Arc<str>, u32)> = Vec::new();
    for (file, line, text) in sheets {
        if !text.contains(":focus") {
            continue;
        }
        // Comments blanked, their line breaks kept.
        let mut src = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(i) = rest.find("/*") {
            src.push_str(&rest[..i]);
            let end = rest[i + 2..].find("*/").map_or(rest.len(), |k| i + 2 + k + 2);
            src.extend(rest[i..end].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
            rest = &rest[end..];
        }
        src.push_str(rest);
        let mut boundary = 0;
        for (i, c) in src.char_indices() {
            match c {
                '{' => {
                    let selector = &src[boundary..i];
                    let body_end = src[i + 1..].find(['{', '}']).map_or(src.len(), |k| i + 1 + k);
                    let body: String = src[i + 1..body_end].chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
                    let at = boundary + (selector.len() - selector.trim_start().len());
                    let sels = selector.split(',').map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()).collect();
                    rules.push((sels, body, file.clone(), line + src[..at].matches('\n').count() as u32));
                    boundary = i + 1;
                }
                '}' | ';' => boundary = i + 1,
                _ => {}
            }
        }
    }
    // Only for every element, or every element of a type (`:focus`, `*:focus`, `a:focus`): a class's,
    // an id's, or one under `:focus-within` is a component's own look (found in review).
    let global = |s: &str| {
        s.strip_suffix(":focus").is_some_and(|el| el.is_empty() || el == "*" || el.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    let none = |d: &str| d.strip_prefix("outline:").is_some_and(|v| matches!(v.trim_end_matches("!important"), "none" | "0" | "0px"));
    // A ring of another kind, in any rule of that selector: a shadow, a border, an outline set
    // again, a background or a colour (found in review).
    let ring = |d: &str| {
        !none(d)
            && !d.ends_with(":none")
            && !d.ends_with(":0")
            && ["box-shadow:", "border", "outline", "background", "color:"].iter().any(|p| d.starts_with(p))
    };
    let mut out = Vec::new();
    for (sels, body, file, line) in &rules {
        if !body.split(';').any(none) {
            continue;
        }
        let bare = sels.iter().filter(|s| global(s)).any(|s| !rules.iter().any(|(o, b, _, _)| o.contains(s) && b.split(';').any(ring)));
        if bare {
            out.push((file.clone(), *line));
        }
    }
    out
}

/// The classes a page of HTML writes: each `class` attribute's words, and the words of its
/// scripts, which may set more, as a script under `public/` is read (FEEDBACK73.md, Q-1022;
/// branch feedback72-81).
fn html_classes(text: &str, into: &mut HashSet<String>) {
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut at = 0;
    while let Some(k) = lower[at..].find("class") {
        let i = at + k;
        at = i + 5;
        if i == 0 || !b[i - 1].is_ascii_whitespace() {
            continue;
        }
        let mut j = at;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if b.get(j) != Some(&b'=') {
            continue;
        }
        j += 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let (from, to) = match b.get(j) {
            Some(q @ (b'"' | b'\'')) => (j + 1, lower[j + 1..].find(*q as char).map_or(b.len(), |e| j + 1 + e)),
            Some(_) => (j, lower[j..].find(|c: char| c.is_ascii_whitespace() || c == '>').map_or(b.len(), |e| j + e)),
            None => break,
        };
        into.extend(text[from..to].split_whitespace().map(String::from));
        at = to;
    }
    let mut at = 0;
    while let Some(k) = lower[at..].find("<script") {
        let start = at + k;
        let end = lower[start..].find("</script").map_or(lower.len(), |e| start + e);
        into.extend(text[start..end].split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).filter(|w| !w.is_empty()).map(String::from));
        at = end;
    }
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
    for p in program_files(&base.join("public"), usize::MAX, false, &|_| true) {
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        let Ok(text) = crate::error::read_text(&p) else { continue };
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
/// The expressions in `e` that run later, not while `e` is worked out: an HTML element's handlers, and
/// the callbacks of `channel.open`, `every`, `listen`, `run_after` and `receive`. A name a
/// handler sends on is not shown, and a `js(...)` in a channel's callback is not where its handle
/// comes from: both made `quillo check` say a page's value was shown, with a fix that did not
/// compile (FEEDBACK66.md, Q-952; branch feedback66). A function called at once (`map`'s) runs
/// as `e` does.
fn runs_later(e: &Expr) -> HashSet<*const Expr> {
    let mut out = HashSet::new();
    each_later(e, &mut |l| {
        walk_expr(l, &mut |x| {
            out.insert(x as *const Expr);
        })
    });
    out
}

/// The functions in `e` that run later (see `runs_later`), each once. In a component, that is
/// in the browser only: the debugger does not stop there (FEEDBACK66.md, Q-951; branch feedback66).
pub(crate) fn each_later(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    walk_expr(e, &mut |x| match x {
        Expr::Call(c, args, _) => {
            let later = match &**c {
                Expr::Ident(n, _) => ["every", "listen", "run_after", "receive"].contains(&n.as_str()),
                Expr::Field(o, n, _) => n == "open" && matches!(&**o, Expr::Ident(c, _) if c == "channel"),
                _ => false,
            };
            if later {
                args.iter().filter(|a| matches!(a, Expr::Lambda(_))).for_each(&mut *f);
            }
        }
        // An HTML element's: a view's or a component's `onShow` is a parameter, which it may
        // call as it is made, on the server (found in review).
        Expr::Html(el, _) => walk_html_els(el, &mut |h| {
            if !h.tag.starts_with(|c: char| c.is_ascii_lowercase()) {
                return;
            }
            for (name, v) in &h.attrs {
                if let AttrVal::Expr(l @ Expr::Lambda(_)) = v {
                    if name.starts_with('@') || name.starts_with("on") {
                        f(l);
                    }
                }
            }
        }),
        _ => {}
    });
}

fn walk_html_els(el: &HtmlEl, f: &mut dyn FnMut(&HtmlEl)) {
    f(el);
    for c in &el.children {
        if let HtmlChild::El(child) = c {
            walk_html_els(child, f);
        }
    }
}

fn unused_classes(order: &[Item], base: &Path, unparsed: &[String], serves: bool) -> Vec<(Arc<str>, u32, String)> {
    let mut sheets: Vec<(Arc<str>, u32, String)> = Vec::new();
    let mut sheet_args: HashSet<*const Expr> = HashSet::new();
    let mut written: HashSet<String> = HashSet::new();
    let mut starts: HashSet<String> = HashSet::new();
    let words = |s: &str, into: &mut HashSet<String>| {
        for w in s.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).filter(|w| !w.is_empty()) {
            into.insert(w.to_string());
        }
    };
    // The folders `files()` serves besides public/ (FEEDBACK73.md, Q-1022; branch feedback72-81).
    let mut folders: Vec<String> = Vec::new();
    for it in order {
        walk_stmts(std::slice::from_ref(&it.stmt), &mut |e| match e {
            Expr::Call(f, args, line) if matches!(&**f, Expr::Ident(n, _) if n == "css") && args.len() == 1 => {
                if let Some(text) = sheet_text(&args[0]) {
                    sheet_args.insert(&args[0] as *const Expr);
                    sheets.push((it.file.clone(), *line, text));
                }
            }
            Expr::Call(f, args, _) if matches!(&**f, Expr::Ident(n, _) if n == "files") => {
                if let Some(Expr::Lit(Lit::Str(d))) = args.first() {
                    let d = d.trim_start_matches("./").trim_end_matches('/');
                    if d != "public" && !d.contains("..") && !Path::new(d).is_absolute() && !folders.iter().any(|x| x == d) {
                        folders.push(d.to_string());
                    }
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
    // What markup writes that no string says: a class map's keys and `class:name`. Told as
    // unused, a rule in use looked safe to delete (FEEDBACK61.md, Q-898; branch feedback61).
    let items: Vec<&[Stmt]> = order.iter().map(|it| std::slice::from_ref(&it.stmt)).collect();
    written.extend(crate::styles::written_classes(&items));
    // A file that does not parse may write any class its text holds
    // (FEEDBACK13.md, Q-092; branch feedback13-fixes).
    for src in unparsed {
        words(src, &mut written);
    }
    // What the app's services send it is theirs to say: their text counts.
    for it in order {
        if let Stmt::Service(_, _, path, _, _, _) = &it.stmt {
            if let Ok(src) = crate::error::read_text(base.join(path)) {
                words(&src, &mut written);
            }
        }
    }
    // The app's own files under public/: its sheets, and what its scripts write. Only a program
    // that serves pages has them (FEEDBACK72.md, Q-1012; branch feedback72-81).
    let mut public = if serves { program_files(&base.join("public"), usize::MAX, false, &|_| true) } else { Vec::new() };
    public.sort();
    for p in public {
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        let Ok(text) = crate::error::read_text(&p) else { continue };
        match ext {
            "css" => sheets.push((Arc::from(p.display().to_string()), 1, text)),
            "js" | "mjs" => words(&text, &mut written),
            // And what its pages write (FEEDBACK73.md, Q-1022; branch feedback72-81).
            "html" | "htm" => html_classes(&text, &mut written),
            _ => {}
        }
    }
    // The pages of the other folders `files()` serves, and the scripts in them (Q-1022): at most
    // 4096 files of each and 16 MB in all, not in a folder named with a dot or node_modules. Not
    // their .js: an app's `dist` of 24 MB of bundles made each check read them all, 0.07G → 2.35G
    // instructions (found in review).
    if !sheets.is_empty() {
        let mut room: u64 = 16 << 20;
        'folders: for d in folders {
            for p in program_files(&base.join(&d), 4096, true, &|p| p.extension().is_some_and(|x| x == "html" || x == "htm")) {
                let size = std::fs::metadata(&p).map_or(0, |m| m.len());
                if size > room {
                    break 'folders;
                }
                room -= size;
                let Ok(text) = crate::error::read_text(&p) else { continue };
                html_classes(&text, &mut written);
            }
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
                walk_pattern(&a.pat, f);
                if let Some(g) = &a.guard {
                    walk_expr(g, f)
                }
                walk_expr(&a.body, f)
            }
        }
        Expr::For(p, i, b, _) => {
            walk_pattern(p, f);
            walk_expr(i, f);
            walk_expr(b, f)
        }
        Expr::While(i, b, _) => {
            walk_expr(i, f);
            walk_expr(b, f)
        }
        Expr::Block(stmts) => walk_stmts(stmts, f),
        Expr::Lambda(d) => walk_fn(d, f),
        Expr::Reply(s, b, _) => {
            walk_expr(s, f);
            if let Some(b) = b {
                walk_expr(b, f)
            }
        }
        Expr::Html(el, _) => walk_html(el, f),
    }
}

/// A function's parameters' defaults, and its body: what it can run. A default is code as its
/// body is: `server fn load(who = request.cookies.uid)` read the visitor where the body did not,
/// and went to every viewer (FEEDBACK55.md, Q-880; branch feedback55).
/// `Checker::ask_after_wait`'s walk, in the order the code runs: each expression given what waited
/// before it (the call's name), and giving what has waited once it is done; `notification.ask`
/// reached after a wait is noted (FEEDBACK67.md, B.3, review; branch pwa).
struct Waits<'a> {
    server: &'a HashSet<String>,
    services: &'a HashSet<String>,
    late: &'a mut Vec<(u32, String)>,
}

impl Waits<'_> {
    fn all<'e>(&mut self, es: impl IntoIterator<Item = &'e Expr>, mut before: Option<String>) -> Option<String> {
        for e in es {
            before = self.expr(e, before);
        }
        before
    }

    fn stmts(&mut self, stmts: &[Stmt], mut before: Option<String>) -> Option<String> {
        for s in stmts {
            before = match s {
                Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) | Stmt::Return(Some(e), _) => self.expr(e, before),
                Stmt::SetField(o, _, _, e, _) => self.all([o, e], before),
                _ => before,
            };
        }
        before
    }

    fn expr(&mut self, e: &Expr, before: Option<String>) -> Option<String> {
        match e {
            Expr::Call(callee, args, line) => {
                let named = match &**callee {
                    Expr::Ident(n, _) => Some((n.to_string(), self.server.contains(&n.to_string()) || matches!(&**n, "parallel" | "receive" | "markdown"))),
                    Expr::Field(o, f, _) => match &**o {
                        Expr::Ident(m, _) => Some((format!("{}.{}", m, f), &**m == "http" || self.services.contains(&m.to_string()) || (&**m == "notification" && f == "stop"))),
                        _ => None,
                    },
                    _ => None,
                };
                let callee_done = if named.is_some() { before } else { self.expr(callee, before) };
                let after = self.all(args, callee_done);
                match named {
                    Some((name, _)) if name == "notification.ask" => {
                        if let Some(w) = &after {
                            self.late.push((*line, w.clone()));
                        }
                        after
                    }
                    Some((name, true)) => after.or(Some(name)),
                    _ => after,
                }
            }
            Expr::If(c, a, b) => {
                let c = self.expr(c, before);
                let a = self.expr(a, c.clone());
                let b = match b {
                    Some(b) => self.expr(b, c.clone()),
                    None => c,
                };
                a.or(b)
            }
            Expr::Match(s, arms, _) => {
                let s = self.expr(s, before);
                let mut out = s.clone();
                for arm in arms {
                    let g = arm.guard.as_ref().map_or(s.clone(), |g| self.expr(g, s.clone()));
                    let r = self.expr(&arm.body, g);
                    out = out.or(r);
                }
                out
            }
            Expr::Coalesce(a, b) => {
                let a = self.expr(a, before);
                let b = self.expr(b, a.clone());
                a.or(b)
            }
            Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
                let i = self.expr(i, before);
                let b = self.expr(b, i.clone());
                i.or(b)
            }
            Expr::Block(stmts) => self.stmts(stmts, before),
            // A function written here runs when it is called, which this does not follow.
            Expr::Lambda(_) | Expr::Lit(_) | Expr::Ident(..) | Expr::Html(..) => before,
            Expr::Template(parts) => self.all(parts.iter().filter_map(|p| if let TPart::Expr(x) = p { Some(x) } else { None }), before),
            Expr::List(xs) => self.all(xs, before),
            Expr::Map(es) => self.all(es.iter().map(|(_, x)| x), before),
            Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => self.expr(x, before),
            Expr::Binary(_, a, b, _) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => self.all([&**a, &**b], before),
            Expr::Reply(a, b, _) => self.all(std::iter::once(&**a).chain(b.as_deref()), before),
        }
    }
}

pub fn walk_fn(d: &FnDef, f: &mut dyn FnMut(&Expr)) {
    d.params.iter().filter_map(|p| p.default.as_ref()).for_each(|e| walk_expr(e, f));
    walk_expr(&d.body, f)
}

/// The expressions a pattern holds: its fields' defaults. No `_` here, in `walk_expr` or in
/// `walk_stmts`: a form added later does not compile until it says what it holds, as a form
/// these miss is code nobody looks at, and for `names_request` a run shared with every viewer
/// (FEEDBACK55.md, Q-880; branch feedback55).
pub fn walk_pattern(p: &Pattern, f: &mut dyn FnMut(&Expr)) {
    match p {
        Pattern::Record(fields) => {
            for field in fields {
                if let Some(d) = &field.default {
                    walk_expr(d, f)
                }
                walk_pattern(&field.pat, f)
            }
        }
        Pattern::List(items, _) | Pattern::Alt(items) => items.iter().for_each(|q| walk_pattern(q, f)),
        Pattern::As(q, _) => walk_pattern(q, f),
        Pattern::Wild | Pattern::Bind(_) | Pattern::Lit(_) | Pattern::Type(_) => {}
    }
}

/// Calls `f` on every expression in `stmts` (FEEDBACK54.md, Part C: a reload looking for a
/// `:memory:` database).
pub(crate) fn each_expr_of(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    walk_stmts(stmts, f)
}

fn walk_stmts(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    for s in stmts {
        match s {
            Stmt::Let(p, e, _) => {
                walk_pattern(p, f);
                walk_expr(e, f)
            }
            Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) => walk_expr(e, f),
            Stmt::SetField(o, _, _, e, _) => {
                walk_expr(o, f);
                walk_expr(e, f)
            }
            Stmt::Return(Some(e), _) => walk_expr(e, f),
            Stmt::Fn(d) => walk_fn(d, f),
            Stmt::Route(r) => {
                r.query.iter().chain(r.body_pat.iter()).chain(r.guards.iter()).for_each(|p| walk_pattern(p, f));
                walk_expr(&r.body, f)
            }
            Stmt::Service(_, _, _, Some(e), _, _) => walk_expr(e, f),
            Stmt::Use(..) | Stmt::Mount(..) | Stmt::Service(_, _, _, None, _, _) | Stmt::Return(None, _) => {}
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
        match v {
            AttrVal::Expr(e) | AttrVal::Spread(e) => walk_expr(e, f),
            AttrVal::Bool => {}
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
///
/// A name past 64 letters is said none, and a candidate whose length is more than 2 from the
/// name's is no nearer than 3: 20 unknown names of 1,000 letters, each compared letter by letter
/// with 1,000 such names, took 31 s (FEEDBACK75.md, Q-1053 and Q-1120, found in review; branch
/// feedback75). The distance is counted no further than 2 (`osa_within`).
fn closest<'a>(name: &str, cands: impl Iterator<Item = &'a str>) -> Option<String> {
    closest_counted(name, cands, &mut 0)
}

/// Names' lower-case letters, kept by `closest_within` for the names not all ASCII.
type Lowered = HashMap<String, Box<[char]>, crate::value::BuildFx>;

/// `closest`, adding to `cost` what it measures (`osa_band`), for a name of at most 64 letters.
fn closest_counted<'a>(name: &str, cands: impl Iterator<Item = &'a str>, cost: &mut u64) -> Option<String> {
    closest_within(name, cands, cost, 64, &mut Lowered::default())
}

/// `closest_counted`, for names of at most `most_len` letters. The bound each candidate is measured
/// to is the best distance found so far, and a candidate's common start and end are not measured
/// (FEEDBACK75.md, Q-1120, found in review; branch feedback75).
/// `lowered` keeps the candidates' lower-case letters that are not all ASCII, from one call to the
/// next.
fn closest_within<'a>(name: &str, cands: impl Iterator<Item = &'a str>, cost: &mut u64, most_len: usize, lowered: &mut Lowered) -> Option<String> {
    let count = name.chars().count();
    if count > most_len {
        return None;
    }
    let lower = name.to_lowercase();
    let lower_chars: Vec<char> = lower.chars().collect();
    let first = lower_chars.first().copied();
    let ascii = name.is_ascii();
    let mut best: Option<(usize, bool, String)> = None;
    let mut buf: Vec<u8> = Vec::new();
    let mut chars: Vec<char> = Vec::new();
    for c in cands {
        let bound = best.as_ref().map_or(2, |b| b.0);
        let d = if c.is_ascii() {
            // ASCII keeps its length in lower case: lengths more than the bound apart are read
            // first, and no lower-case copy is kept.
            if c.len().abs_diff(lower_chars.len()) > bound || (ascii && c.eq_ignore_ascii_case(name)) {
                continue;
            }
            if ascii {
                buf.clear();
                buf.extend(c.bytes().map(|x| x.to_ascii_lowercase()));
                osa_band(lower.as_bytes(), &buf, bound, cost)
            } else {
                chars.clear();
                chars.extend(c.chars().map(|x| x.to_ascii_lowercase()));
                osa_band(&lower_chars, &chars, bound, cost)
            }
        } else {
            // Lower case may lengthen a letter (`İ`): a length 4 apart is the cheap test, the
            // lower case's the sure one.
            if c.len() + bound < lower_chars.len() {
                continue;
            }
            if lowered.get(c).is_none() {
                lowered.insert(c.to_string(), c.chars().flat_map(char::to_lowercase).collect());
            }
            let Some(lc) = lowered.get(c) else { continue };
            // A name not all ASCII costs twice as long to measure: counted so.
            *cost += 4;
            if **lc == *lower_chars || lc.len().abs_diff(lower_chars.len()) > bound {
                continue;
            }
            osa_band(&lower_chars, lc, bound, cost)
        };
        if d > bound || d >= count {
            continue;
        }
        let other_first = c.chars().flat_map(char::to_lowercase).next() != first;
        let key = (d, other_first, c.to_string());
        if best.as_ref().map_or(true, |b| key < *b) {
            best = Some(key);
        }
    }
    best.map(|(_, _, c)| c)
}

/// `osa_distance`, counted no further than `most`: more is `most + 1`. The common start and end of
/// the two are passed over, as they change no distance; then only the cells at most `most` from the
/// diagonal are filled, and it stops at a row all past `most`. What it measured is added to `cost`
/// (FEEDBACK75.md, Q-1120, found in review; branch feedback75).
fn osa_band<C: PartialEq>(a: &[C], b: &[C], most: usize, cost: &mut u64) -> usize {
    let start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[start..], &b[start..]);
    let end = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[..a.len() - end], &b[..b.len() - end]);
    *cost += 4 + (start + end) as u64 / 4;
    if a.len().abs_diff(b.len()) > most {
        return most + 1;
    }
    let far = most + 1;
    let (n, m) = (a.len(), b.len());
    let w = m + 1;
    let mut small = [0usize; 3 * 72];
    let mut large = Vec::new();
    let rows: &mut [usize] = if 3 * w <= small.len() {
        &mut small[..3 * w]
    } else {
        large.resize(3 * w, 0);
        &mut large
    };
    // Row i is at ((i % 3) * w): the one before it and the one before that are the other two. Each
    // row's band is filled with the cells either side of it, all another row reads of it.
    for j in 0..=m.min(most + 2) {
        rows[j] = j.min(far);
    }
    for i in 1..=n {
        let (cur, prev, before) = ((i % 3) * w, ((i - 1) % 3) * w, ((i + 1) % 3) * w);
        let (lo, hi) = (i.saturating_sub(most).max(1), (i + most).min(m));
        rows[cur] = i.min(far);
        if lo > 1 {
            rows[cur + lo - 1] = far;
        }
        if hi < m {
            rows[cur + hi + 1] = far;
        }
        let mut least = rows[cur];
        for j in lo..=hi {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (rows[prev + j] + 1).min(rows[cur + j - 1] + 1).min(rows[prev + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(rows[before + j - 2] + 1);
            }
            rows[cur + j] = v.min(far);
            least = least.min(rows[cur + j]);
        }
        *cost += (hi + 1 - lo.min(hi + 1)) as u64;
        if least > most {
            return far;
        }
    }
    rows[(n % 3) * w + m].min(far)
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

/// What `m.k = v` (or `+=`, `-=`) on a map is written as: a new map, assigned to the variable
/// (FEEDBACK64.md, Q-934; branch feedback64).
fn map_field_advice(o: &Expr, name: &str, op: AssignOp, e: &Expr) -> String {
    let Expr::Ident(m, _) = o else {
        return format!("a map does not change in place: make the new one, `m = set(m, \"{}\", v)`, and assign it to its variable", name);
    };
    let v = written(e);
    let value = match op {
        AssignOp::Set => v,
        AssignOp::Add => format!("{}.{} + {}", m, name, v),
        AssignOp::Sub => format!("{}.{} - {}", m, name, v),
    };
    format!("a map does not change in place: `{m} = {{ ...{m}, {}: {value} }}` or `{m} = set({m}, \"{name}\", {value})`", key_written(name).unwrap_or_else(|| name.to_string()))
}

/// Whether a builtin can change anything, or reach outside: what the view cache says acts (a
/// random number apart), the database, the network, files, or code the checker cannot read, or
/// asking the user (FEEDBACK64.md, Q-931; branch feedback64).
fn builtin_changes(n: &str) -> bool {
    let effect = crate::ast::builtin_effect(n);
    let outside = ["db", "http", "fs", "net"].iter().fold(0, |m, e| m | crate::ast::effect_bit(e));
    (crate::viewcache::kind(n) == crate::viewcache::Kind::Acts && effect != crate::ast::effect_bit("random"))
        || effect & outside != 0
        || n.starts_with("db.")
        || n.starts_with("http.")
        || matches!(n, "js" | "js_new" | "run_script" | "ask" | "about" | "confirm")
        // What the view cache takes for reads, as no page shows them, but a call does for its own
        // sake: a line in the log, a wait, the page moved, a function run for a tenant or a locale.
        || matches!(n, "print" | "sleep" | "navigate" | "live" | "with_tenant" | "with_locale" | "notify" | "exit")
        // A browser's, which the view cache takes for nothing as they do nothing on the server
        // (found in review).
        || matches!(n, "every" | "run_after" | "page_updated")
        // A `live` loader's run follows the topic (found in review); `unwrap` of an error stops
        // the request, which a call may be for.
        || n == "watch"
        || n == "unwrap"
}

/// What `gives_sql_safe` keeps of a function from one reading to the next: whether it assigns, and
/// if it does, its graph of who reads what (FEEDBACK75.md, Q-1085, found in review; branch
/// feedback75).
enum GraphSlot<'e> {
    Unread,
    NoAssign,
    Graph(Box<SafeGraph<'e>>),
}

/// A function's `let`s of one name and its assignments, in the order written (lambdas' and inner
/// blocks' too), each with what it is given; for each name, the ones whose value reads it, the
/// innermost only: a `let` whose value is a block of further `let`s reads what its block gives
/// back, not what they read (FEEDBACK75.md, Q-1085, found in review; branch feedback75).
struct SafeGraph<'e> {
    defs: Vec<(&'static str, &'e Expr, bool)>,
    readers: HashMap<&'static str, Vec<u32>, crate::value::BuildFx>,
    last_let: HashMap<&'static str, u32, crate::value::BuildFx>,
    assigned: HashSet<&'static str, crate::value::BuildFx>,
}

/// Whether `e` assigns a variable, in the blocks it runs (a lambda's too), read without its
/// values' insides: a list of 300,000 is not walked for it.
fn assigns_any(e: &Expr) -> bool {
    fn stmts(ss: &[Stmt], depth: u32) -> bool {
        ss.iter().any(|st| match st {
            Stmt::Assign(..) => true,
            Stmt::Let(_, v, _) | Stmt::Expr(v) | Stmt::Return(Some(v), _) => expr(v, depth),
            _ => false,
        })
    }
    fn expr(e: &Expr, depth: u32) -> bool {
        let go = |ss: &[Stmt]| if depth % 8 == 7 { crate::stack::maybe_grow(256 << 10, 4 << 20, || stmts(ss, depth + 1)) } else { stmts(ss, depth + 1) };
        match e {
            Expr::Block(ss) => go(ss),
            Expr::If(_, a, b) => std::iter::once(&**a).chain(b.as_deref()).any(|x| matches!(x, Expr::Block(ss) if go(ss))),
            Expr::For(_, _, b, _) | Expr::While(_, b, _) => matches!(&**b, Expr::Block(ss) if go(ss)),
            Expr::Match(_, arms, _) => arms.iter().any(|a| matches!(&a.body, Expr::Block(ss) if go(ss))),
            Expr::Lambda(d) => matches!(&d.body, Expr::Block(ss) if go(ss)),
            _ => false,
        }
    }
    expr(e, 0)
}

/// `body`'s `SafeGraph`, in one walk of it; a confined check's clock read every 4,096th part.
fn safe_graph(body: &Expr) -> SafeGraph<'_> {
    let mut g = SafeGraph { defs: Vec::new(), readers: HashMap::default(), last_let: HashMap::default(), assigned: HashSet::default() };
    let mut seen = 0u32;
    graph_expr(&mut g, body, None, 0, &mut seen);
    for (i, (n, _, assign)) in g.defs.iter().enumerate() {
        if *assign {
            g.assigned.insert(n);
        } else {
            g.last_let.insert(n, i as u32);
        }
    }
    g
}

fn graph_expr<'e>(g: &mut SafeGraph<'e>, e: &'e Expr, at: Option<u32>, depth: u32, seen: &mut u32) {
    *seen = seen.wrapping_add(1);
    if *seen % 4096 == 0 && DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d) {
        OVER_BUDGET.with(|o| o.set(true));
    }
    if *seen % 4096 == 0 && OVER_BUDGET.with(|o| o.get()) {
        return;
    }
    if depth % 8 == 7 {
        return crate::stack::maybe_grow(256 << 10, 4 << 20, || graph_here(g, e, at, depth, seen));
    }
    graph_here(g, e, at, depth, seen)
}

fn graph_here<'e>(g: &mut SafeGraph<'e>, e: &'e Expr, at: Option<u32>, depth: u32, seen: &mut u32) {
    let d = depth + 1;
    match e {
        Expr::Ident(n, _) => {
            if let Some(at) = at {
                let r = g.readers.entry(n.as_str()).or_default();
                if r.last() != Some(&at) {
                    r.push(at);
                }
            }
        }
        Expr::Block(stmts) => {
            for st in stmts {
                match st {
                    Stmt::Let(Pattern::Bind(n), v, _) | Stmt::Assign(n, _, v, _) => {
                        let own = g.defs.len() as u32;
                        g.defs.push((n.as_str(), v, matches!(st, Stmt::Assign(..))));
                        graph_expr(g, v, Some(own), d, seen);
                    }
                    Stmt::Let(_, v, _) | Stmt::Expr(v) | Stmt::Return(Some(v), _) | Stmt::State(_, v, _) => graph_expr(g, v, at, d, seen),
                    Stmt::SetField(o, _, _, v, _) => {
                        graph_expr(g, o, at, d, seen);
                        graph_expr(g, v, at, d, seen);
                    }
                    Stmt::Fn(f) => graph_expr(g, &f.body, None, d, seen),
                    _ => {}
                }
            }
        }
        Expr::Lit(_) => {}
        Expr::Template(parts) => {
            for p in parts {
                if let TPart::Expr(x) = p {
                    graph_expr(g, x, at, d, seen);
                }
            }
        }
        Expr::List(xs) => xs.iter().for_each(|x| graph_expr(g, x, at, d, seen)),
        Expr::Map(es) => es.iter().for_each(|(_, x)| graph_expr(g, x, at, d, seen)),
        Expr::Unary(_, x, _) | Expr::Try(x) | Expr::Field(x, _, _) => graph_expr(g, x, at, d, seen),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => {
            graph_expr(g, a, at, d, seen);
            graph_expr(g, b, at, d, seen);
        }
        Expr::Call(c, args, _) => {
            graph_expr(g, c, at, d, seen);
            args.iter().for_each(|x| graph_expr(g, x, at, d, seen));
        }
        Expr::If(c, a, b) => {
            graph_expr(g, c, at, d, seen);
            graph_expr(g, a, at, d, seen);
            if let Some(b) = b {
                graph_expr(g, b, at, d, seen);
            }
        }
        Expr::Match(x, arms, _) => {
            graph_expr(g, x, at, d, seen);
            for a in arms {
                if let Some(gd) = &a.guard {
                    graph_expr(g, gd, at, d, seen);
                }
                graph_expr(g, &a.body, at, d, seen);
            }
        }
        Expr::For(_, i, b, _) | Expr::While(i, b, _) => {
            graph_expr(g, i, at, d, seen);
            graph_expr(g, b, at, d, seen);
        }
        Expr::Lambda(f) => graph_expr(g, &f.body, at, d, seen),
        Expr::Reply(x, b, _) => {
            graph_expr(g, x, at, d, seen);
            if let Some(b) = b {
                graph_expr(g, b, at, d, seen);
            }
        }
        // Markup's names, as `walk_expr` finds them: no `let` is written in it.
        Expr::Html(..) => {
            if let Some(at) = at {
                walk_expr(e, &mut |x| {
                    if let Expr::Ident(n, _) = x {
                        let r = g.readers.entry(n.as_str()).or_default();
                        if r.last() != Some(&at) {
                            r.push(at);
                        }
                    }
                });
            }
        }
    }
}

/// The variables assigned something not SQL-safe, for `gives_sql_safe`: every name a `let` or an
/// assignment gives is taken as safe text to begin with; one whose value (each assignment's, a
/// `let`'s last) is not `ok` with the names still safe is not, and only what reads it is read again
/// (`SafeGraph`). A chain of 20,000 assignments was read again whole for each, 24.7 s
/// (FEEDBACK75.md, Q-1085, found in review; branch feedback75). A confined check's clock is read
/// every 256th.
fn unsafely_assigned(
    g: &SafeGraph<'_>,
    safe: &HashSet<String>,
    ok: fn(&Expr, &HashSet<String>, &HashSet<String>, &HashSet<String>) -> bool,
) -> HashSet<String> {
    let none = HashSet::new();
    let mut known: HashSet<String> = g.defs.iter().map(|(n, _, _)| n.to_string()).collect();
    let mut unsafe_vars: HashSet<String> = HashSet::new();
    let mut queue: std::collections::VecDeque<u32> = (0..g.defs.len() as u32).collect();
    let mut queued = vec![true; g.defs.len()];
    let mut taken = 0u32;
    while let Some(i) = queue.pop_front() {
        queued[i as usize] = false;
        taken = taken.wrapping_add(1);
        if taken % 256 == 0 && DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d) {
            OVER_BUDGET.with(|o| o.set(true));
            break;
        }
        let (n, v, assign) = g.defs[i as usize];
        let counts = if assign { known.contains(n) } else { g.last_let.get(n) == Some(&i) && !g.assigned.contains(n) && known.contains(n) };
        if !counts || ok(v, safe, &known, &none) {
            continue;
        }
        known.remove(n);
        if assign {
            unsafe_vars.insert(n.to_string());
        }
        for &r in g.readers.get(n).into_iter().flatten() {
            if !queued[r as usize] {
                queued[r as usize] = true;
                queue.push_back(r);
            }
        }
    }
    unsafe_vars
}

/// The program's functions that call each, by the name called: each read again when one it calls
/// is found (`Worklist`; FEEDBACK75.md, Q-1053, found in review; branch feedback75).
fn callers_by_name<'d>(defs: &[&'d FnDef]) -> HashMap<&'d str, Vec<usize>> {
    let names: HashSet<&'d str> = defs.iter().map(|d| d.name.as_str()).collect();
    let mut callers: HashMap<&'d str, Vec<usize>> = HashMap::new();
    for (i, d) in defs.iter().enumerate() {
        let mut called = Vec::new();
        for code in fn_code(d) {
            called_names(code, &mut called);
        }
        called.sort_unstable();
        called.dedup();
        for n in called {
            if let Some(&name) = names.get(n.as_str()) {
                callers.entry(name).or_default().push(i);
            }
        }
    }
    callers
}

/// The program's functions in an order that has each after the functions it calls (a cycle's in
/// the order met), from `callers_by_name` (found in review).
fn callees_first(defs: &[&FnDef], callers: &HashMap<&str, Vec<usize>>) -> Vec<usize> {
    let mut calls: Vec<Vec<usize>> = vec![Vec::new(); defs.len()];
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    for (i, d) in defs.iter().enumerate() {
        by_name.entry(d.name.as_str()).or_insert(i);
    }
    for (name, cs) in callers {
        if let Some(&callee) = by_name.get(name) {
            for &c in cs {
                calls[c].push(callee);
            }
        }
    }
    let mut order = Vec::with_capacity(defs.len());
    let mut state = vec![0u8; defs.len()];
    for start in 0..defs.len() {
        if state[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        state[start] = 1;
        while let Some(top) = stack.last_mut() {
            let i = top.0;
            if let Some(&c) = calls[i].get(top.1) {
                top.1 += 1;
                if state[c] == 0 {
                    state[c] = 1;
                    stack.push((c, 0));
                }
            } else {
                state[i] = 2;
                order.push(i);
                stack.pop();
            }
        }
    }
    order
}

/// The functions still to read, each once at first and again when queued by `again`; ending at a
/// confined check's time, read every 64th (FEEDBACK75.md, Q-1053, found in review; branch
/// feedback75).
/// First in, first out: a caller written before its 10,000 callees, read again each time one was
/// found, waits in the queue once for all of them (found in review).
struct Worklist {
    todo: std::collections::VecDeque<usize>,
    queued: Vec<bool>,
    taken: u32,
}

impl Worklist {
    fn new(order: &[usize]) -> Worklist {
        Worklist { todo: order.iter().copied().collect(), queued: vec![true; order.len()], taken: 0 }
    }

    /// The next function to read; a confined check's clock is read for each (found in review).
    fn next(&mut self) -> Option<usize> {
        self.taken = self.taken.wrapping_add(1);
        if DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() >= d) {
            OVER_BUDGET.with(|o| o.set(true));
        }
        if OVER_BUDGET.with(|o| o.get()) {
            return None;
        }
        let i = self.todo.pop_front()?;
        self.queued[i] = false;
        Some(i)
    }

    fn again(&mut self, callers: Option<&Vec<usize>>) {
        for &c in callers.into_iter().flatten() {
            if !self.queued[c] {
                self.queued[c] = true;
                self.todo.push_back(c);
            }
        }
    }
}

/// What a function gives back, as `Checker::gives_sql_built` reads it: its body, or its block's
/// `return`s and last expression (FEEDBACK75.md, Q-1085; branch feedback75).
fn fn_results(d: &FnDef) -> Vec<&Expr> {
    // Each `return` in the blocks that run where the body does, not in a lambda's.
    fn returns<'e>(stmts: &'e [Stmt], out: &mut Vec<&'e Expr>, depth: u32) {
        let inner = |x: &'e Expr, out: &mut Vec<&'e Expr>| {
            let go = |stmts: &'e [Stmt], out: &mut Vec<&'e Expr>| {
                if depth % 8 == 7 {
                    crate::stack::maybe_grow(256 << 10, 4 << 20, || returns(stmts, out, depth + 1))
                } else {
                    returns(stmts, out, depth + 1)
                }
            };
            match x {
                Expr::Block(s) => go(s, out),
                Expr::If(_, a, b) => {
                    for y in std::iter::once(&**a).chain(b.as_deref()) {
                        if let Expr::Block(s) = y {
                            go(s, out);
                        }
                    }
                }
                Expr::For(_, _, b, _) | Expr::While(_, b, _) => {
                    if let Expr::Block(s) = &**b {
                        go(s, out);
                    }
                }
                Expr::Match(_, arms, _) => {
                    for a in arms {
                        if let Expr::Block(s) = &a.body {
                            go(s, out);
                        }
                    }
                }
                _ => {}
            }
        };
        for st in stmts {
            match st {
                Stmt::Return(Some(x), _) => out.push(x),
                Stmt::Expr(x) | Stmt::Let(_, x, _) | Stmt::Assign(_, _, x, _) => inner(x, out),
                _ => {}
            }
        }
    }
    match &d.body {
        Expr::Block(stmts) => {
            let mut out: Vec<&Expr> = Vec::new();
            if let Some(Stmt::Expr(x)) = stmts.last() {
                out.push(x);
            }
            returns(stmts, &mut out, 0);
            out
        }
        e => vec![e],
    }
}

/// Whether a function's result may be text built from values, by its form: not markup, a
/// number, a text as written or a map (FEEDBACK75.md, Q-1085; branch feedback75).
fn may_build(e: &Expr) -> bool {
    may_build_at(e, 0)
}

/// `may_build`, `depth` choices deep: the stack grown every 8th, as `Returns::deep` grows it.
fn may_build_at(e: &Expr, depth: u32) -> bool {
    let inner = |x: &Expr| if depth % 8 == 7 { crate::stack::maybe_grow(256 << 10, 4 << 20, || may_build_at(x, depth + 1)) } else { may_build_at(x, depth + 1) };
    match e {
        Expr::Template(parts) => parts.iter().any(|p| matches!(p, TPart::Expr(_))),
        Expr::Binary(BinOp::Add, ..) | Expr::Call(..) | Expr::Pipe(..) | Expr::Ident(..) | Expr::Field(..) | Expr::Index(..) => true,
        Expr::If(_, a, b) => inner(a) || b.as_deref().is_some_and(inner),
        Expr::Match(_, arms, _) => arms.iter().any(|a| inner(&a.body)),
        Expr::Block(stmts) => matches!(stmts.last(), Some(Stmt::Expr(x) | Stmt::Return(Some(x), _)) if inner(x)),
        _ => false,
    }
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
        let mut name = words.next();
        // `fn mcp total()`: the tag, then the name; `fn mcp(x)` is named `mcp` (FEEDBACK56.md,
        // Q-853, found in review; branch feedback56).
        if name == Some("mcp") && matches!(first, Some("fn" | "view" | "component")) {
            let chars: Vec<char> = line.chars().collect();
            let at = (0..chars.len().saturating_sub(2)).find(|&i| chars[i..i + 3] == ['m', 'c', 'p'] && (i == 0 || !chars[i - 1].is_alphanumeric()));
            if at.is_some_and(|i| crate::highlight::mcp_tag(&chars, i, i + 3)) {
                name = words.next();
            }
        }
        if let Some(name) = name {
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
fn gives_sql_safe<'e>(body: &'e Expr, safe: &HashSet<String>, graph: &mut GraphSlot<'e>) -> bool {
    fn ok(e: &Expr, safe: &HashSet<String>, vars: &HashSet<String>, assigned: &HashSet<String>) -> bool {
        match e {
            Expr::Lit(Lit::Str(_)) => true,
            Expr::Ident(n, _) => vars.contains(n.as_str()),
            Expr::Call(f, _, _) => matches!(&**f, Expr::Ident(n, _) if n == "sql_name" || n == "sql_param" || safe.contains(n.as_str()))
                || matches!(&**f, Expr::Field(o, n, _) if n == "sql_name" && matches!(&**o, Expr::Ident(m, _) if m == "db")),
            Expr::Template(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_)) || matches!(p, TPart::Expr(x) if ok(x, safe, vars, assigned))),
            Expr::Binary(BinOp::Add, a, b, _) => ok(a, safe, vars, assigned) && ok(b, safe, vars, assigned),
            Expr::If(_, a, b) => ok(a, safe, vars, assigned) && b.as_deref().map_or(false, |b| ok(b, safe, vars, assigned)),
            Expr::Match(_, arms, _) => !arms.is_empty() && arms.iter().all(|a| ok(&a.body, safe, vars, assigned)),
            Expr::Block(stmts) => {
                let vars = block_vars(stmts, safe, vars, assigned);
                match stmts.last() {
                    Some(Stmt::Expr(x)) => ok(x, safe, &vars, assigned),
                    Some(Stmt::Return(Some(x), _)) => ok(x, safe, &vars, assigned),
                    _ => false,
                }
            }
            _ => false,
        }
    }
    // The variables a block binds to safe text, up to its end.
    fn block_vars(stmts: &[Stmt], safe: &HashSet<String>, outer: &HashSet<String>, assigned: &HashSet<String>) -> HashSet<String> {
        let mut vars = outer.clone();
        for st in stmts {
            if let Stmt::Let(Pattern::Bind(n), x, _) = st {
                if ok(x, safe, &vars, assigned) && !assigned.contains(n.as_str()) {
                    vars.insert(n.to_string());
                } else {
                    vars.remove(n.as_str());
                }
            }
        }
        vars
    }
    // A variable assigned anywhere in the body something not SQL-safe holds nothing known:
    // `let s = "select ..."` then `s += n` gave back built SQL as safe, and its callers' queries
    // were not warned of (FEEDBACK75.md, Q-1085, found in review; branch feedback75). What is
    // assigned is read with the variables bound safe text, and those assigned, taken as safe until
    // one is found not to be (`c = c + " desc"` keeps `c` safe).
    // The graph of who reads what is made once for each function, and only for one that assigns
    // (found in review).
    if matches!(graph, GraphSlot::Unread) {
        *graph = if assigns_any(body) { GraphSlot::Graph(Box::new(safe_graph(body))) } else { GraphSlot::NoAssign };
    }
    let unsafe_vars = match graph {
        GraphSlot::Graph(g) => unsafely_assigned(g, safe, ok),
        _ => HashSet::new(),
    };
    let assigned = &unsafe_vars;
    // Every `return`, wherever it is (in an `if`, a loop, after `??`), with
    // what its own block bound before it.
    let mut returns_ok = true;
    walk_expr(body, &mut |x| {
        if let Expr::Block(stmts) = x {
            let mut vars = HashSet::new();
            for st in stmts {
                match st {
                    Stmt::Let(Pattern::Bind(n), v, _) => {
                        if ok(v, safe, &vars, assigned) && !assigned.contains(n.as_str()) {
                            vars.insert(n.to_string());
                        } else {
                            vars.remove(n.as_str());
                        }
                    }
                    Stmt::Return(Some(r), _) if !ok(r, safe, &vars, assigned) => returns_ok = false,
                    _ => {}
                }
            }
        }
    });
    returns_ok && ok(body, safe, &HashSet::new(), assigned)
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
