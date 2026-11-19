//! Records: types with a name and fields.
//!
//! ```text
//! type Address = { city: str, zip: str? }
//! type User = {
//!     name: str,
//!     email: str,
//!     role: "admin" | "member" = "member",   // one of these; a default
//!     address: Address?,
//!     tags: list<str> = [],
//! }
//! type Id = int | str                        // another name for a type
//!
//! User({ name: "Ada", email: "ada@example.com" })   // a User, or an error value
//! fn greet(u: User) = `Hi ${u.name}`                // checked when called
//! route POST /users { json: User } = save(json)     // the body, as a User
//! match v { User as u => ..., _ => ... }
//! ```
//!
//! A record is a map: JSON, the browser and every map function see it as
//! one. What its name adds is checking. `User(v)` makes one from a map: each
//! field is checked (records in it are made the same way), missing ones get
//! their defaults, and fields the record does not have are left out, so
//! what a client sends cannot add any. It gives an error value saying what
//! is wrong, as `int("x")` does. A record in an annotation or a pattern is
//! checked field by field (in a pattern, made as `User(v)` makes it), and
//! `quillo check` holds code to the fields before it runs.

use crate::ast::{is_type_name, Ty, TypeDef};
use crate::interp::{type_name_matches, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::sync::Arc;

pub type Types = HashMap<String, Arc<TypeDef>>;

/// Where in a value it goes wrong, and how; `what` empty when it is just
/// not the type (said where that is known).
#[derive(Debug, Default, Clone)]
pub struct Why {
    path: String,
    what: String,
}

impl Why {
    fn inside(mut self, seg: &str) -> Why {
        // A path past this long says its innermost end: a value nested 500 deep was told its
        // whole way in, a few KB each time it was refused (FEEDBACK75.md, Q-1080's review; branch
        // feedback75).
        if self.path.starts_with('…') {
            return self;
        }
        if self.path.len() > 200 {
            self.path.insert(0, '…');
            return self;
        }
        self.path = if self.path.is_empty() {
            seg.to_string()
        } else if self.path.starts_with('[') {
            format!("{}{}", seg, self.path)
        } else {
            format!("{}.{}", seg, self.path)
        };
        self
    }

    /// Says what was wanted here, if nothing deeper did.
    fn here(mut self, ty: &Ty, v: &Value) -> Why {
        if self.what.is_empty() {
            self.what = format!("must be {}, got {} {}", ty.text(), v.type_name(), short(v));
        }
        self
    }

    fn missing(field: &str) -> Why {
        Why { path: field.to_string(), what: "is missing".into() }
    }

    /// "`address.city` must be str, got int 5"; None for a plain mismatch.
    pub fn text(&self) -> Option<String> {
        match (self.path.is_empty(), self.what.is_empty()) {
            (_, true) => None,
            (true, false) => Some(self.what.clone()),
            (false, false) => Some(format!("`{}` {}", self.path, self.what)),
        }
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

/// The program's own values (`fits`: its functions' arguments and answers; a record's
/// constructor) are checked this deep and taken as they are below, by 0.8.8's walk unchanged,
/// which costs what it did: held to `DEPTH`, a chain the program built 513 long was refused, and
/// a function over one, checked at each call, took time in its square (FEEDBACK75.md, Q-1080's
/// review; branch feedback75). A type that names itself (`type Ab = Ab | int`) is found when the
/// program is checked (`self_naming`), not by this walk.
const PROGRAM_DEPTH: usize = 64;

/// Deeper than this, a value from a request (`make`: a server fn's arguments, a route's patterns)
/// is refused as nested too deep. Past 64 levels it was taken as it came, unchecked: the 33rd
/// node of a recursive record (a field, then a list item, each a level) could have any type, a
/// value outside a set and fields such as `is_admin`. JSON from a request nests 128 levels at
/// most (`value::JSON_NESTING`), so every value read from one is checked to its end, with a type
/// named on the way at each level (FEEDBACK75.md, Q-1080; branch feedback75).
const DEPTH: usize = 4 * crate::value::JSON_NESTING;

/// Up to this deep a request's walk is `Shallow`: a record, a list, a map shape or a type naming
/// another tests `depth` once. Deeper, it goes on as `Deep`, which holds every value to `DEPTH`
/// and grows the stack where it runs low.
const SHALLOW: usize = 32;

/// From this level down, a request's walk keeps a union's answers for a list or a map (`KEPT`):
/// the levels above, a few unions at most, are tried every way.
const MEMO_FROM: usize = 8;

/// The types that name themselves: one whose way, through types naming types and nothing that
/// reads deeper into the value (no record, list or map shape between), comes back to it
/// (`type Ab = Ab | int`, `type Aa = Bb | int` with `type Bb = Aa | str`). Found over the
/// definitions alone, once, for `quillo check` to refuse: the program's walk carries no chain to
/// find one as it runs, and took any value as one; a request's walk refuses what such a type adds
/// nothing to (FEEDBACK75.md, Q-1080's review; branch feedback75).
pub fn self_naming(types: &Types) -> Vec<String> {
    let mut found: Vec<String> = types.keys().filter(|n| names_reach(types, n, n)).cloned().collect();
    found.sort();
    found
}

/// Whether a type that names itself does so through an optional one (`type Opt = Opt?`, or a
/// `?` on another type of its loop): only a record or a list in the way makes sense of that,
/// which `quillo check` says (FEEDBACK75.md, Q-1080, found in review; branch feedback75).
pub fn self_naming_optional(types: &Types, name: &str) -> bool {
    types.iter().any(|(n, t)| t.ty.optional && (n == name || names_reach(types, name, n)) && names_reach(types, n, name))
}

/// Whether `to` is reached from the type `from` through types naming types alone: no record, list
/// or map shape between.
fn names_reach(types: &Types, from: &str, to: &str) -> bool {
    let Some(td) = types.get(from).filter(|t| !t.is_record()) else { return false };
    let mut seen: Vec<&str> = Vec::new();
    let mut todo: Vec<&str> = td.ty.names.iter().map(|n| n.as_str()).collect();
    while let Some(n) = todo.pop() {
        if n == to {
            return true;
        }
        if seen.contains(&n) {
            continue;
        }
        seen.push(n);
        if let Some(t) = types.get(n).filter(|t| !t.is_record()) {
            todo.extend(t.ty.names.iter().map(|m| m.as_str()));
        }
    }
    false
}

/// `self_naming` of a loaded program's types.
pub fn self_naming_in(it: &Interp) -> Vec<String> {
    self_naming(&it.types)
}

/// What a request's walk is: up to `SHALLOW` (`Shallow`) or past it (`Deep`).
trait Mode: Copy {
    const DEEP: bool;
}

#[derive(Clone, Copy)]
struct Shallow;

#[derive(Clone, Copy)]
struct Deep;

impl Mode for Shallow {
    const DEEP: bool = false;
}

impl Mode for Deep {
    const DEEP: bool = true;
}

/// Whether a request's walk goes on as `Deep` here (see `SHALLOW`).
#[inline(always)]
fn deepens<M: Mode>(depth: usize) -> bool {
    !M::DEEP && depth >= SHALLOW
}

thread_local! {
    /// In a request's walk (`make`), what each union of types (`Num | Add | Mul`) gave for a list
    /// or a map from `MEMO_FROM` down: by the union's and the value's addresses (the value kept,
    /// so that its address is no other's), its level and the walk's kind (`fits`, `make`, `make`
    /// lenient). Each pair is tried once. A union of records that hold the union was tried every
    /// way at every level, 2^k for a value k deep: 7-9 s for a 2 KB body that fails at k=24, past
    /// `--max-compute`, and as long for one that fits, each way that failed at a level having
    /// walked all below it (FEEDBACK75.md, Q-1080's review; branch feedback75). Only the
    /// program's types are keys: they live for the walk, where a type made for a step and dropped
    /// gave its address to the next, which took the first one's answer. Emptied as the walk ends.
    static KEPT: std::cell::RefCell<HashMap<(usize, usize, usize, u8), (Value, Result<Value, Why>)>> = std::cell::RefCell::new(HashMap::new());
    /// Whether `KEPT` holds anything: the walks that keep nothing, most of them, look at this
    /// alone as they end.
    static ANY_KEPT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The address a list's or a map's items are at; none for another value.
fn held_at(v: &Value) -> Option<usize> {
    match v {
        Value::List(l) => Some(Arc::as_ptr(l) as *const u8 as usize),
        Value::Map(m) => Some(Arc::as_ptr(m) as *const u8 as usize),
        _ => None,
    }
}

/// What union `ty` gave for the value at `at` in this walk, if it was tried.
fn kept(ty: &Ty, at: usize, depth: usize, kind: u8) -> Option<Result<Value, Why>> {
    KEPT.with(|k| k.borrow().get(&(ty as *const Ty as usize, at, depth, kind)).map(|(_, r)| r.clone()))
}

fn keep(ty: &Ty, v: &Value, at: usize, depth: usize, kind: u8, r: &Result<Value, Why>) {
    KEPT.with(|k| k.borrow_mut().insert((ty as *const Ty as usize, at, depth, kind), (v.clone(), r.clone())));
    ANY_KEPT.with(|a| a.set(true));
}

/// Empties `KEPT` as a request's walk ends, however it ends: a panic in it (a bug of Quillo's,
/// which the server answers 500 and lives on) left it for the thread's next request.
struct Forget;

impl Drop for Forget {
    #[inline(always)]
    fn drop(&mut self) {
        if ANY_KEPT.with(|a| a.get()) {
            forget_all();
        }
    }
}

#[cold]
#[inline(never)]
fn forget_all() {
    KEPT.with(|k| {
        if let Ok(mut k) = k.try_borrow_mut() {
            *k = HashMap::new();
            ANY_KEPT.with(|a| a.set(false));
        }
    });
}

/// What a value nested past `DEPTH` is told.
#[cold]
fn too_deep() -> Why {
    Why { path: String::new(), what: format!("is nested more than {} deep", DEPTH) }
}

/// Whether a deep walk's stack runs low here: it goes on on a new piece.
#[inline(always)]
fn low() -> bool {
    crate::stack::remaining().is_some_and(|r| r < 256 << 10)
}

/// The types named in a row to reach a type (`type Id = int | str`), since the value was last
/// read a level deeper: such a step reads nothing of the value, and a type named again on the way
/// is one that names itself (`type Ab = Ab | int`), which that step refuses: held to `DEPTH`
/// alone, past which it is refused, `type Ab = Ab | Ab | int` would try 2^DEPTH ways
/// (FEEDBACK75.md, Q-1080; branch feedback75). The chain is a type of its own at each length, so
/// that a field or an item, where it starts again (`Unnamed`), carries nothing: a chain passed
/// with every value cost a call with a record argument 2% more (the in-process bench). Each such
/// step is still a level of `DEPTH`: a chain of the program's types, all different, is held to it
/// as the value's levels are, and so is the walk's stack.
trait Chain: Copy {
    /// The chain one longer.
    type Next<'b>: Chain
    where
        Self: 'b;
    /// `t` named after these, kept in `slot` where it must be, unless it is among them already
    /// (a type is its definition: one pointer, compared as one).
    fn then<'b>(self, t: &'b TypeDef, slot: &'b mut Option<Named<'b>>) -> Option<Self::Next<'b>>
    where
        Self: 'b;
}

/// No type named yet.
#[derive(Clone, Copy)]
struct Unnamed;

/// One type named.
#[derive(Clone, Copy)]
struct First<'a>(&'a TypeDef);

/// Two or more.
#[derive(Clone, Copy)]
struct Named<'a> {
    t: &'a TypeDef,
    up: Up<'a>,
}

#[derive(Clone, Copy)]
enum Up<'a> {
    First(&'a TypeDef),
    More(&'a Named<'a>),
}

impl Chain for Unnamed {
    type Next<'b>
        = First<'b>
    where
        Self: 'b;
    #[inline(always)]
    fn then<'b>(self, t: &'b TypeDef, _: &'b mut Option<Named<'b>>) -> Option<First<'b>>
    where
        Self: 'b,
    {
        Some(First(t))
    }
}

impl<'a> Chain for First<'a> {
    type Next<'b>
        = &'b Named<'b>
    where
        Self: 'b;
    fn then<'b>(self, t: &'b TypeDef, slot: &'b mut Option<Named<'b>>) -> Option<&'b Named<'b>>
    where
        Self: 'b,
    {
        (!std::ptr::eq(t, self.0)).then(|| &*slot.insert(Named { t, up: Up::First(self.0) }))
    }
}

impl<'a> Chain for &'a Named<'a> {
    type Next<'b>
        = &'b Named<'b>
    where
        Self: 'b;
    fn then<'b>(self, t: &'b TypeDef, slot: &'b mut Option<Named<'b>>) -> Option<&'b Named<'b>>
    where
        Self: 'b,
    {
        let mut x: &Named = self;
        loop {
            if std::ptr::eq(x.t, t) {
                return None;
            }
            match x.up {
                Up::First(f) if std::ptr::eq(f, t) => return None,
                Up::First(_) => break,
                Up::More(up) => x = up,
            }
        }
        Some(&*slot.insert(Named { t, up: Up::More(self) }))
    }
}


/// Whether `v` is of type `ty`. At the top of an annotation (`deep` false),
/// lists and map shapes are checked as before (`list<int>` is any list);
/// records are checked field by field, and everything inside them too.
pub fn fits(types: &Types, ty: &Ty, v: &Value, deep: bool) -> Result<(), Why> {
    fits_at(types, ty, v, deep, 0)
}

fn fits_at(types: &Types, ty: &Ty, v: &Value, deep: bool, depth: usize) -> Result<(), Why> {
    if (ty.optional && matches!(v, Value::Nil)) || depth > PROGRAM_DEPTH {
        return Ok(());
    }
    let mut best = Why::default();
    for n in &ty.names {
        match fits_name(types, n, ty, v, deep, depth) {
            Ok(()) => return Ok(()),
            Err(w) if best.what.is_empty() && !w.what.is_empty() => best = w,
            Err(_) => {}
        }
    }
    Err(best)
}

// In line in `fits_at`, as 0.8.8's was: `make_name` calls it too, and left out of line it cost a
// typed argument's check 5% more (the in-process bench; FEEDBACK75.md, Q-1080's review; branch
// feedback75).
#[inline(always)]
fn fits_name(types: &Types, n: &str, ty: &Ty, v: &Value, deep: bool, depth: usize) -> Result<(), Why> {
    if n.starts_with('"') {
        let want: String = serde_json::from_str(n).unwrap_or_default();
        return match v {
            Value::Str(s) if **s == *want => Ok(()),
            _ => Err(Why::default()),
        };
    }
    if let Some(td) = types.get(n) {
        if !td.is_record() {
            return fits_at(types, &td.ty, v, deep, depth + 1);
        }
        let Value::Map(m) = v else { return Err(Why::default()) };
        for (k, fty) in td.ty.fields.as_deref().unwrap_or_default() {
            match field(m, k).filter(|x| !matches!(x, Value::Nil)) {
                None if fty.optional => {}
                None => return Err(Why::missing(k)),
                Some(x) => fits_at(types, fty, x, true, depth + 1).map_err(|w| w.here(fty, x).inside(k))?,
            }
        }
        return Ok(());
    }
    if is_type_name(n) || !type_name_matches(n, v) {
        return Err(Why::default());
    }
    if deep {
        match (n, v) {
            ("list", Value::List(items)) => {
                if let Some(e) = &ty.elem {
                    for (i, x) in items.iter().enumerate() {
                        fits_at(types, e, x, true, depth + 1).map_err(|w| w.here(e, x).inside(&format!("[{}]", i)))?;
                    }
                }
            }
            ("map", Value::Map(m)) => {
                for (k, fty) in ty.fields.as_deref().unwrap_or_default() {
                    match field(m, k).filter(|x| !matches!(x, Value::Nil)) {
                        None if fty.optional => {}
                        None => return Err(Why::missing(k)),
                        Some(x) => fits_at(types, fty, x, true, depth + 1).map_err(|w| w.here(fty, x).inside(k))?,
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A record's or a map shape's field, looked up as 0.8.8's walk had `IndexMap::get` in line: with
/// the request's walk beside it, the compiler left `get` a call, 10 instructions a field more;
/// `get_index`, not `m[i]`, which brings a formatted panic in (FEEDBACK75.md, Q-1080's review;
/// branch feedback75).
#[inline(always)]
fn field<'a>(m: &'a Map, k: &str) -> Option<&'a Value> {
    m.get_index_of(k).and_then(|i| m.get_index(i)).map(|(_, x)| x)
}

/// Whether a type has a record in it (then a pattern makes the value).
fn has_record(types: &Types, ty: &Ty, depth: usize) -> bool {
    depth < 8
        && (ty.names.iter().any(|n| types.get(n).map_or(false, |td| td.is_record() || has_record(types, &td.ty, depth + 1)))
            || ty.elem.as_deref().map_or(false, |e| has_record(types, e, depth + 1)))
}

/// Whether a type has a `float` in it, which a whole number is made into.
fn has_float(types: &Types, ty: &Ty, depth: usize) -> bool {
    depth < 8
        && (ty.names.iter().any(|n| n == "float" || types.get(n).map_or(false, |td| !td.is_record() && has_float(types, &td.ty, depth + 1)))
            || ty.elem.as_deref().map_or(false, |e| has_float(types, e, depth + 1)))
}

/// `v` made into a value of type `ty`: records made from maps (defaults
/// put in, other fields left out), lists of them item by item, the rest
/// checked as it is. `lenient`: text converts to the scalar asked for, as
/// form fields and query values do in patterns.
pub fn make(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool) -> Result<Result<Value, Why>, Ctrl> {
    let _forget = Forget;
    req_make_at::<_, Shallow>(it, ty, v, lenient, 0, Unnamed)
}

/// `make` for a value of the program's own (a record's constructor): 0.8.8's walk, held to
/// `PROGRAM_DEPTH`.
pub(crate) fn make_program(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool) -> Result<Result<Value, Why>, Ctrl> {
    make_at(it, ty, v, lenient, 0)
}

fn make_at(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    if (ty.optional && matches!(v, Value::Nil)) || depth > PROGRAM_DEPTH {
        return Ok(Ok(v.clone()));
    }
    let mut best = Why::default();
    for n in &ty.names {
        let r = make_name(it, n, ty, v, lenient, depth)?;
        match r {
            Ok(x) => return Ok(Ok(x)),
            Err(w) if best.what.is_empty() && !w.what.is_empty() => best = w,
            Err(_) => {}
        }
    }
    Ok(Err(best))
}

fn make_name(it: &mut Interp, n: &str, ty: &Ty, v: &Value, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    if let Some(td) = it.types.get(n).cloned() {
        if !td.is_record() {
            return make_at(it, &td.ty, v, lenient, depth + 1);
        }
        let Value::Map(m) = v else { return Ok(Err(Why::default())) };
        return make_record(it, &td, m, lenient, depth);
    }
    match (n, v) {
        ("list", Value::List(items)) if ty.elem.as_deref().map_or(false, |e| has_record(&it.types, e, 0) || has_float(&it.types, e, 0)) => {
            let e = ty.elem.as_deref().unwrap();
            let mut out = Vec::with_capacity(items.len());
            for (i, x) in items.iter().enumerate() {
                match make_at(it, e, x, lenient, depth + 1)? {
                    Ok(y) => out.push(y),
                    Err(w) => return Ok(Err(w.here(e, x).inside(&format!("[{}]", i)))),
                }
            }
            Ok(Ok(Value::list(out)))
        }
        // A float takes a whole number: JSON, and a browser's JavaScript, have
        // one kind of number, so 5.0 arrives as 5. Only data made into a type
        // is widened; a call within the program still wants a float
        // (FEEDBACK18.md, Q-103; branch feedback18-fixes).
        ("float", Value::Int(i)) if !ty.names.iter().any(|n| n == "int") => Ok(Ok(Value::Float(*i as f64))),
        // A decimal field takes an int, numeric text or a JSON number.
        ("dec", _) => Ok(crate::interp::as_dec(v).map(Value::Dec).ok_or_else(Why::default)),
        // Money, held to `money.of`'s rules, with what is wrong (FEEDBACK50.md, Q-621; branch
        // feedback50-fixes).
        ("money", _) => Ok(crate::money::from_data(v).map_err(|what| Why { path: String::new(), what })),
        // Text converts to a scalar, not to one of a set of strings (`"oil_change" | "tire_rotation"`):
        // that is text the form or the query gives as it is, which `fits` reads below. The set
        // refused every value from a form or a query, and took it from JSON (FEEDBACK58.md,
        // Q-874; branch cache-and-migrate).
        (t, Value::Str(s)) if lenient && !type_name_matches(t, v) && !t.starts_with('"') => match crate::interp::coerce(t, s) {
            Some(x) => Ok(Ok(x)),
            None => Ok(Err(Why::default())),
        },
        // The type's other names checked as they are, from the top as 0.8.8 checked them, by the
        // type itself: no copy of its list's or map shape's parts made for the step and dropped
        // (FEEDBACK75.md, Q-1080's review; branch feedback75), which also halves a record's making.
        _ => Ok(fits_name(&it.types, n, ty, v, true, 0).map(|_| v.clone())),
    }
}

fn make_record(it: &mut Interp, td: &TypeDef, m: &Map, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    let mut out = Map::with_capacity(m.len());
    for (k, fty) in td.ty.fields.as_deref().unwrap_or_default() {
        let given = m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)).cloned();
        let x = match given {
            Some(x) => x,
            None => match td.defaults.iter().find(|(d, _)| d == k) {
                // Made again for each record: a default list is each one's own.
                Some((_, e)) => {
                    let globals = it.globals.clone();
                    it.eval(e, &globals)?
                }
                None if fty.optional => continue,
                None => return Ok(Err(Why::missing(k))),
            },
        };
        match make_at(it, fty, &x, lenient, depth + 1)? {
            Ok(y) => {
                out.insert(k.as_str().into(), y);
            }
            Err(w) => return Ok(Err(w.here(fty, &x).inside(k))),
        }
    }
    Ok(Ok(Value::map(out)))
}

// ---- A request's walk: `make` (FEEDBACK75.md, Q-1080 and its reviews; branch feedback75) ----
//
// Values from a request, held to `DEPTH`, the chain of types naming types carried to refuse a
// type that names itself as it runs, unions' answers kept (`KEPT`). The program's own walk above
// is 0.8.8's, unchanged.

/// `depth`: the value's levels above `v`, and the types named on the way; `chain`: the types
/// named in a row to reach `ty`.
fn req_fits_at<C: Chain, M: Mode>(types: &Types, ty: &Ty, v: &Value, deep: bool, depth: usize, chain: C) -> Result<(), Why> {
    if ty.optional && matches!(v, Value::Nil) {
        return Ok(());
    }
    if M::DEEP {
        if depth > DEPTH {
            return Err(too_deep());
        }
        if low() {
            return crate::stack::maybe_grow(256 << 10, 4 << 20, || req_fits_at::<C, Deep>(types, ty, v, deep, depth, chain));
        }
    }
    if depth >= MEMO_FROM && ty.names.len() > 1 {
        if let Some(at) = held_at(v) {
            return req_fits_union::<C, M>(types, ty, v, deep, depth, chain, at);
        }
    }
    req_fits_names::<C, M>(types, ty, v, deep, depth, chain)
}

/// `req_fits_at` for a union and a list or a map, in a request's walk: each pair tried once (see
/// `KEPT`).
#[inline(never)]
fn req_fits_union<C: Chain, M: Mode>(types: &Types, ty: &Ty, v: &Value, deep: bool, depth: usize, chain: C, at: usize) -> Result<(), Why> {
    if let Some(r) = kept(ty, at, depth, 0) {
        return r.map(|_| ());
    }
    let r = req_fits_names::<C, M>(types, ty, v, deep, depth, chain);
    keep(ty, v, at, depth, 0, &r.as_ref().map(|_| Value::Nil).map_err(Why::clone));
    r
}

#[inline(always)]
fn req_fits_names<C: Chain, M: Mode>(types: &Types, ty: &Ty, v: &Value, deep: bool, depth: usize, chain: C) -> Result<(), Why> {
    let mut best = Why::default();
    for n in &ty.names {
        match req_fits_name::<C, M>(types, n, ty, v, deep, depth, chain) {
            Ok(()) => return Ok(()),
            Err(w) if best.what.is_empty() && !w.what.is_empty() => best = w,
            Err(_) => {}
        }
    }
    Err(best)
}

// In line where it is used, as when `req_fits_names` alone called it: `req_make_name` calls it too, and a
// call left out of line cost a typed argument's check 5-7% more (the in-process bench).
#[inline(always)]
fn req_fits_name<C: Chain, M: Mode>(types: &Types, n: &str, ty: &Ty, v: &Value, deep: bool, depth: usize, chain: C) -> Result<(), Why> {
    if n.starts_with('"') {
        let want: String = serde_json::from_str(n).unwrap_or_default();
        return match v {
            Value::Str(s) if **s == *want => Ok(()),
            _ => Err(Why::default()),
        };
    }
    if let Some(td) = types.get(n) {
        if deepens::<M>(depth) {
            return req_fits_deep(types, n, ty, v, deep, depth, chain);
        }
        if !td.is_record() {
            let mut slot = None;
            let Some(next) = chain.then(td, &mut slot) else { return Err(Why::default()) };
            return req_fits_at::<_, M>(types, &td.ty, v, deep, depth + 1, next);
        }
        let Value::Map(m) = v else { return Err(Why::default()) };
        for (k, fty) in td.ty.fields.as_deref().unwrap_or_default() {
            match m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)) {
                None if fty.optional => {}
                None => return Err(Why::missing(k)),
                Some(x) => req_fits_at::<_, M>(types, fty, x, true, depth + 1, Unnamed).map_err(|w| w.here(fty, x).inside(k))?,
            }
        }
        return Ok(());
    }
    if is_type_name(n) || !type_name_matches(n, v) {
        return Err(Why::default());
    }
    if deep {
        match (n, v) {
            ("list", Value::List(items)) => {
                if deepens::<M>(depth) {
                    return req_fits_deep(types, n, ty, v, deep, depth, chain);
                }
                if let Some(e) = &ty.elem {
                    for (i, x) in items.iter().enumerate() {
                        req_fits_at::<_, M>(types, e, x, true, depth + 1, Unnamed).map_err(|w| w.here(e, x).inside(&format!("[{}]", i)))?;
                    }
                }
            }
            ("map", Value::Map(m)) => {
                if deepens::<M>(depth) {
                    return req_fits_deep(types, n, ty, v, deep, depth, chain);
                }
                for (k, fty) in ty.fields.as_deref().unwrap_or_default() {
                    match m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)) {
                        None if fty.optional => {}
                        None => return Err(Why::missing(k)),
                        Some(x) => req_fits_at::<_, M>(types, fty, x, true, depth + 1, Unnamed).map_err(|w| w.here(fty, x).inside(k))?,
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// `req_fits_name` past `SHALLOW`, out of the way of the shallow values.
#[cold]
#[inline(never)]
fn req_fits_deep<C: Chain>(types: &Types, n: &str, ty: &Ty, v: &Value, deep: bool, depth: usize, chain: C) -> Result<(), Why> {
    req_fits_name::<C, Deep>(types, n, ty, v, deep, depth, chain)
}

/// `depth` and `chain` as `req_fits_at` has them.
fn req_make_at<C: Chain, M: Mode>(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool, depth: usize, chain: C) -> Result<Result<Value, Why>, Ctrl> {
    if ty.optional && matches!(v, Value::Nil) {
        return Ok(Ok(v.clone()));
    }
    if M::DEEP {
        if depth > DEPTH {
            return Ok(Err(too_deep()));
        }
        if low() {
            return crate::stack::maybe_grow(256 << 10, 4 << 20, || req_make_at::<C, Deep>(it, ty, v, lenient, depth, chain));
        }
    }
    if depth >= MEMO_FROM && ty.names.len() > 1 {
        if let Some(at) = held_at(v) {
            return req_make_union::<C, M>(it, ty, v, lenient, depth, chain, at);
        }
    }
    req_make_names::<C, M>(it, ty, v, lenient, depth, chain)
}

/// `req_make_at` for a union, as `req_fits_union`.
#[inline(never)]
fn req_make_union<C: Chain, M: Mode>(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool, depth: usize, chain: C, at: usize) -> Result<Result<Value, Why>, Ctrl> {
    let kind = 1 + lenient as u8;
    if let Some(r) = kept(ty, at, depth, kind) {
        return Ok(r);
    }
    let r = req_make_names::<C, M>(it, ty, v, lenient, depth, chain)?;
    keep(ty, v, at, depth, kind, &r);
    Ok(r)
}

#[inline(always)]
fn req_make_names<C: Chain, M: Mode>(it: &mut Interp, ty: &Ty, v: &Value, lenient: bool, depth: usize, chain: C) -> Result<Result<Value, Why>, Ctrl> {
    let mut best = Why::default();
    for n in &ty.names {
        let r = req_make_name::<C, M>(it, n, ty, v, lenient, depth, chain)?;
        match r {
            Ok(x) => return Ok(Ok(x)),
            Err(w) if best.what.is_empty() && !w.what.is_empty() => best = w,
            Err(_) => {}
        }
    }
    Ok(Err(best))
}

fn req_make_name<C: Chain, M: Mode>(it: &mut Interp, n: &str, ty: &Ty, v: &Value, lenient: bool, depth: usize, chain: C) -> Result<Result<Value, Why>, Ctrl> {
    if let Some(td) = it.types.get(n).cloned() {
        if deepens::<M>(depth) {
            return req_make_deep(it, n, ty, v, lenient, depth, chain);
        }
        if !td.is_record() {
            let mut slot = None;
            let Some(next) = chain.then(&td, &mut slot) else { return Ok(Err(Why::default())) };
            return req_make_at::<_, M>(it, &td.ty, v, lenient, depth + 1, next);
        }
        let Value::Map(m) = v else { return Ok(Err(Why::default())) };
        return req_make_record::<M>(it, &td, m, lenient, depth);
    }
    match (n, v) {
        ("list", Value::List(items)) if ty.elem.as_deref().map_or(false, |e| has_record(&it.types, e, 0) || has_float(&it.types, e, 0)) => {
            if deepens::<M>(depth) {
                return req_make_deep(it, n, ty, v, lenient, depth, chain);
            }
            let e = ty.elem.as_deref().unwrap();
            let mut out = Vec::with_capacity(items.len());
            for (i, x) in items.iter().enumerate() {
                match req_make_at::<_, M>(it, e, x, lenient, depth + 1, Unnamed)? {
                    Ok(y) => out.push(y),
                    Err(w) => return Ok(Err(w.here(e, x).inside(&format!("[{}]", i)))),
                }
            }
            Ok(Ok(Value::list(out)))
        }
        // A float takes a whole number: JSON, and a browser's JavaScript, have
        // one kind of number, so 5.0 arrives as 5. Only data made into a type
        // is widened; a call within the program still wants a float
        // (FEEDBACK18.md, Q-103; branch feedback18-fixes).
        ("float", Value::Int(i)) if !ty.names.iter().any(|n| n == "int") => Ok(Ok(Value::Float(*i as f64))),
        // A decimal field takes an int, numeric text or a JSON number.
        ("dec", _) => Ok(crate::interp::as_dec(v).map(Value::Dec).ok_or_else(Why::default)),
        // Money, held to `money.of`'s rules, with what is wrong (FEEDBACK50.md, Q-621; branch
        // feedback50-fixes).
        ("money", _) => Ok(crate::money::from_data(v).map_err(|what| Why { path: String::new(), what })),
        // Text converts to a scalar, not to one of a set of strings (`"oil_change" | "tire_rotation"`):
        // that is text the form or the query gives as it is, which `fits` reads below. The set
        // refused every value from a form or a query, and took it from JSON (FEEDBACK58.md,
        // Q-874; branch cache-and-migrate).
        (t, Value::Str(s)) if lenient && !type_name_matches(t, v) && !t.starts_with('"') => match crate::interp::coerce(t, s) {
            Some(x) => Ok(Ok(x)),
            None => Ok(Err(Why::default())),
        },
        // The type's other names checked as they are, by the type itself: a copy of its list's or
        // map shape's parts, made for the step and dropped, gave its address to the next step's,
        // which took the answer kept for the first (`KEPT`; FEEDBACK75.md, Q-1080's review; branch
        // feedback75). The program's own values from the top, as 0.8.8 checked them.
        _ => Ok(req_fits_name::<C, M>(&it.types, n, ty, v, true, depth, chain).map(|_| v.clone())),
    }
}

/// `req_make_name` past `SHALLOW`, as `req_fits_deep`.
#[cold]
#[inline(never)]
fn req_make_deep<C: Chain>(it: &mut Interp, n: &str, ty: &Ty, v: &Value, lenient: bool, depth: usize, chain: C) -> Result<Result<Value, Why>, Ctrl> {
    req_make_name::<C, Deep>(it, n, ty, v, lenient, depth, chain)
}

fn req_make_record<M: Mode>(it: &mut Interp, td: &TypeDef, m: &Map, lenient: bool, depth: usize) -> Result<Result<Value, Why>, Ctrl> {
    let mut out = Map::with_capacity(m.len());
    for (k, fty) in td.ty.fields.as_deref().unwrap_or_default() {
        let given = m.get(k.as_str()).filter(|x| !matches!(x, Value::Nil)).cloned();
        let x = match given {
            Some(x) => x,
            None => match td.defaults.iter().find(|(d, _)| d == k) {
                // Made again for each record: a default list is each one's own.
                Some((_, e)) => {
                    let globals = it.globals.clone();
                    it.eval(e, &globals)?
                }
                None if fty.optional => continue,
                None => return Ok(Err(Why::missing(k))),
            },
        };
        match req_make_at::<_, M>(it, fty, &x, lenient, depth + 1, Unnamed)? {
            Ok(y) => {
                out.insert(k.as_str().into(), y);
            }
            Err(w) => return Ok(Err(w.here(fty, &x).inside(k))),
        }
    }
    Ok(Ok(Value::map(out)))
}

/// `__record(name, fields)`: what `User(fields)` calls.
pub fn construct(it: &mut Interp, args: Vec<Value>, line: u32) -> Result<Value, Ctrl> {
    let name = match args.first() {
        Some(Value::Str(s)) => s.to_string(),
        _ => return crate::interp::fail(line, "`__record` is called by a record's name, as in User({ ... })"),
    };
    let Some(td) = it.types.get(&name).cloned() else {
        return crate::interp::fail(line, format!("there is no type `{}`", name));
    };
    let v = match args.get(1) {
        None | Some(Value::Nil) if td.is_record() => Value::map(Map::new()),
        Some(v) => v.clone(),
        None => Value::Nil,
    };
    if td.is_record() && !matches!(v, Value::Map(_)) {
        return Ok(Value::own_err(Value::str(format!("{}: expects a map of its fields, got {} {}", name, v.type_name(), short(&v)))));
    }
    let ty = Ty { names: vec![name.clone()], optional: false, elem: None, fields: None };
    Ok(match make_program(it, &ty, &v, false)? {
        Ok(x) => x,
        Err(w) => Value::own_err(Value::str(match w.here(&td.ty, &v).text() {
            Some(t) => format!("{}: {}", name, t),
            None => format!("{}: must be {}, got {} {}", name, td.ty.text(), v.type_name(), short(&v)),
        })),
    })
}
