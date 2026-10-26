use crate::ast::FnDef;
use crate::interp::{Ctrl, Interp};
use indexmap::IndexMap;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

pub type Map = IndexMap<Key, Value>;

/// A map key, kept in place when short (as nearly all are), so a map such as
/// a database row needs no allocation for its keys. It hashes, compares and
/// reads as the `str` it holds.
#[derive(Clone)]
pub struct Key(KeyText);

#[derive(Clone)]
enum KeyText {
    Short(u8, [u8; 22]),
    Long(Arc<str>),
}

impl Key {
    pub fn new(s: &str) -> Key {
        if s.len() <= 22 {
            let mut b = [0u8; 22];
            b[..s.len()].copy_from_slice(s.as_bytes());
            Key(KeyText::Short(s.len() as u8, b))
        } else {
            Key(KeyText::Long(s.into()))
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            // SAFETY: the bytes were copied from a `str`, whole.
            KeyText::Short(n, b) => unsafe { std::str::from_utf8_unchecked(&b[..*n as usize]) },
            KeyText::Long(s) => s,
        }
    }
}

impl std::ops::Deref for Key {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for Key {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Key {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.as_str().hash(h)
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Key {}

impl PartialEq<str> for Key {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Key {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Key) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Key) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl From<&str> for Key {
    fn from(s: &str) -> Key {
        Key::new(s)
    }
}

impl From<String> for Key {
    fn from(s: String) -> Key {
        Key::new(&s)
    }
}

impl From<&String> for Key {
    fn from(s: &String) -> Key {
        Key::new(s)
    }
}

impl From<Key> for String {
    fn from(k: Key) -> String {
        k.as_str().to_string()
    }
}
pub type BuiltinFn = fn(&mut Interp, Vec<Value>, u32) -> Result<Value, Ctrl>;

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// An exact decimal: `12.50m` (src/decimal.rs).
    Dec(crate::decimal::Dec),
    Str(Arc<str>),
    List(Arc<Vec<Value>>),
    Map(Arc<Map>),
    Func(Arc<Closure>),
    Builtin(&'static str, BuiltinFn),
    /// Already-rendered, safe HTML.
    Html(Arc<str>),
    Response(Arc<Response>),
    /// An error value: `err(x)`, a failed `int("x")`, ...
    Err(Arc<Value>),
}

pub struct Closure {
    pub def: Arc<FnDef>,
    pub env: Env,
}

#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

#[derive(Clone)]
pub enum Body {
    Value(Value),
    Bytes(Vec<u8>),
    /// Sent as the function makes it (`stream`, `events`: src/stream.rs).
    Stream { f: Value, events: bool },
}

impl Value {
    pub fn str(s: impl AsRef<str>) -> Value {
        Value::Str(Arc::from(s.as_ref()))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Arc::new(items))
    }

    pub fn map(m: Map) -> Value {
        Value::Map(Arc::new(m))
    }

    pub fn html(s: impl AsRef<str>) -> Value {
        Value::Html(Arc::from(s.as_ref()))
    }

    pub fn err(v: Value) -> Value {
        Value::Err(Arc::new(v))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Dec(_) => "dec",
            Value::Str(_) => "str",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Func(_) | Value::Builtin(..) => "fn",
            Value::Html(_) => "html",
            Value::Response(_) => "response",
            Value::Err(_) => "error",
        }
    }

    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false) | Value::Err(_))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Text form used by string interpolation and `str()`.
    pub fn display(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            Value::Html(h) => h.to_string(),
            Value::Dec(d) => d.to_string(),
            _ => self.repr(),
        }
    }

    /// The text of a map key made from this value (`set`, `get`,
    /// `group_by`): its display, with a decimal's zeros at the end left
    /// out, so 12.5 and 12.50 are one key.
    pub fn key_text(&self) -> String {
        match self {
            Value::Dec(d) => d.normalized().to_string(),
            _ => self.display(),
        }
    }

    /// Source-like form used by the REPL and error messages.
    pub fn repr(&self) -> String {
        match self {
            Value::Nil => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => fmt_float(*f),
            Value::Dec(d) => format!("{}m", d),
            Value::Str(s) => serde_json::to_string(&**s).unwrap_or_default(),
            Value::List(l) => format!("[{}]", l.iter().map(|v| v.repr()).collect::<Vec<_>>().join(", ")),
            Value::Map(m) if m.is_empty() => "{}".into(),
            Value::Map(m) => format!(
                "{{ {} }}",
                m.iter().map(|(k, v)| format!("{}: {}", fmt_key(k), v.repr())).collect::<Vec<_>>().join(", ")
            ),
            Value::Func(c) => format!("<fn {}>", c.def.name),
            Value::Builtin(n, _) if n.starts_with("native:") => format!("<rust fn {}>", crate::native::display_name(n)),
            Value::Builtin(n, _) => format!("<builtin {}>", n),
            Value::Html(h) => h.to_string(),
            Value::Response(r) => format!("<response {}>", r.status),
            Value::Err(e) => format!("err({})", e.repr()),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Value::Nil => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(n) => J::from(*n),
            Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
            // As text, so nothing that reads it rounds it to a float.
            Value::Dec(d) => J::String(d.to_string()),
            Value::Str(s) => J::String(s.to_string()),
            Value::Html(h) => J::String(h.to_string()),
            Value::List(l) => J::Array(l.iter().map(|v| v.to_json()).collect()),
            Value::Map(m) => J::Object(m.iter().map(|(k, v)| (k.to_string(), v.to_json())).collect()),
            Value::Err(e) => serde_json::json!({ "error": e.to_json() }),
            Value::Response(r) => serde_json::json!({ "status": r.status }),
            Value::Func(_) | Value::Builtin(..) => J::String(self.repr()),
        }
    }

    /// The JSON text of `to_json`, written straight into `out`.
    pub fn json_into(&self, out: &mut Vec<u8>) {
        match self {
            Value::Nil => out.extend_from_slice(b"null"),
            Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Int(n) => {
                let _ = serde_json::to_writer(&mut *out, n);
            }
            Value::Float(f) => {
                let _ = serde_json::to_writer(&mut *out, f);
            }
            Value::Dec(d) => json_str_into(&d.to_string(), out),
            Value::Str(s) | Value::Html(s) => json_str_into(s, out),
            Value::List(l) => {
                out.push(b'[');
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    v.json_into(out);
                }
                out.push(b']');
            }
            Value::Map(m) => {
                out.push(b'{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    json_str_into(k, out);
                    out.push(b':');
                    v.json_into(out);
                }
                out.push(b'}');
            }
            Value::Err(e) => {
                out.extend_from_slice(b"{\"error\":");
                e.json_into(out);
                out.push(b'}');
            }
            Value::Response(r) => out.extend_from_slice(format!("{{\"status\":{}}}", r.status).as_bytes()),
            Value::Func(_) | Value::Builtin(..) => json_str_into(&self.repr(), out),
        }
    }

    /// `json_into`, as a string.
    pub fn json_text(&self) -> String {
        let mut out = Vec::new();
        self.json_into(&mut out);
        // SAFETY: only JSON text, which is UTF-8, was written.
        unsafe { String::from_utf8_unchecked(out) }
    }

    pub fn from_json(j: &serde_json::Value) -> Value {
        use serde_json::Value as J;
        match j {
            J::Null => Value::Nil,
            J::Bool(b) => Value::Bool(*b),
            J::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(0.0)),
            },
            J::String(s) => Value::str(s),
            J::Array(a) => Value::list(a.iter().map(Value::from_json).collect()),
            J::Object(o) => Value::map(o.iter().map(|(k, v)| (k.into(), Value::from_json(v))).collect()),
        }
    }
}

fn fmt_float(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}

fn fmt_key(k: &str) -> String {
    let mut chars = k.chars();
    let ident = chars.next().map_or(false, |c| c.is_alphabetic() || c == '_')
        && k.chars().all(|c| c.is_alphanumeric() || c == '_');
    if ident {
        k.to_string()
    } else {
        serde_json::to_string(k).unwrap_or_default()
    }
}

pub fn equal(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Nil, Nil) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Int(_) | Float(_), Int(_) | Float(_)) => a.as_f64() == b.as_f64(),
        (Dec(_), Int(_) | Float(_) | Dec(_)) | (Int(_) | Float(_), Dec(_)) => {
            dec_pair(a, b).map_or(false, |(x, y)| x == y)
        }
        (Str(x), Str(y)) => x == y,
        (Html(x), Html(y)) => x == y,
        (List(x), List(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| equal(p, q)),
        (Map(x), Map(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).map_or(false, |w| equal(v, w))),
        (Err(x), Err(y)) => equal(x, y),
        (Func(x), Func(y)) => Arc::ptr_eq(x, y),
        (Builtin(x, _), Builtin(y, _)) => x == y,
        _ => false,
    }
}

/// Ordered values; None for those that are not (a decimal and a float
/// are not: one is exact, the other not).
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        (Value::Dec(x), Value::Dec(y)) => Some(x.cmp(*y)),
        (Value::Dec(x), Value::Int(y)) => Some(x.cmp(crate::decimal::Dec::from_i64(*y))),
        (Value::Int(x), Value::Dec(y)) => Some(crate::decimal::Dec::from_i64(*x).cmp(*y)),
        _ => a.as_f64()?.partial_cmp(&b.as_f64()?),
    }
}

/// Two numbers as decimals, one at least a decimal: a float through its
/// shortest text, so `0.1m == 0.1` (as `dec(0.1) == 0.1m`).
fn dec_pair(a: &Value, b: &Value) -> Option<(crate::decimal::Dec, crate::decimal::Dec)> {
    let one = |v: &Value| match v {
        Value::Dec(d) => Some(*d),
        Value::Int(n) => Some(crate::decimal::Dec::from_i64(*n)),
        Value::Float(f) => crate::decimal::Dec::from_f64(*f),
        _ => None,
    };
    Some((one(a)?, one(b)?))
}

/// A JSON string, escaped as `serde_json` escapes it.
fn json_str_into(s: &str, out: &mut Vec<u8>) {
    if s.bytes().all(|b| b >= 0x20 && b != b'"' && b != b'\\') {
        out.reserve(s.len() + 2);
        out.push(b'"');
        out.extend_from_slice(s.as_bytes());
        out.push(b'"');
    } else {
        let _ = serde_json::to_writer(&mut *out, s);
    }
}

// ---- environments ------------------------------------------------------

pub type Env = Arc<Scope>;

thread_local! {
    /// Ended scopes, for reuse (see `Scope::done`).
    static SPARE: std::cell::RefCell<Vec<Env>> = const { std::cell::RefCell::new(Vec::new()) };
}

const SPARE_SCOPES: usize = 64;

thread_local! {
    /// Large values whose scope ended, for the event loop to have freed on
    /// another thread (`defer_drops`); `None` where they are freed at once.
    static LATER: std::cell::RefCell<Option<Vec<Value>>> = const { std::cell::RefCell::new(None) };
}

/// From now on, large lists and maps that go out of scope on this thread are
/// kept for `deferred_drops` rather than freed: an event loop has them freed
/// by a thread with time to spare. (Rows read from the database were made on
/// such a thread, which also frees them fastest.)
pub fn defer_drops() {
    LATER.with(|l| *l.borrow_mut() = Some(Vec::new()));
}

/// The values kept since the last call, to be dropped elsewhere.
pub fn deferred_drops() -> Vec<Value> {
    LATER.with(|l| l.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
}

fn drop_value(v: Value) {
    let large = match &v {
        Value::List(l) => l.len() >= 8 && Arc::strong_count(l) == 1,
        Value::Map(m) => m.len() >= 16 && Arc::strong_count(m) == 1,
        _ => false,
    };
    if large {
        LATER.with(|l| match l.borrow_mut().as_mut() {
            Some(later) => later.push(v),
            None => drop(v),
        });
    }
}

/// Variables, and the scope around them. The parent link is fixed when the
/// scope is made, so looking a name up walks the chain without locking it.
pub struct Scope {
    vars: VarsCell,
    parent: Option<Env>,
}

/// The lock on a scope's variables. Quillo code runs under the interpreter
/// lock, so this is never contended; unlike `std::sync::Mutex` (which on
/// some systems allocates an OS lock), it is one flag in place.
struct VarsCell {
    locked: std::sync::atomic::AtomicBool,
    vars: std::cell::UnsafeCell<Vars>,
}

// Access goes through `lock`, which hands out one guard at a time.
unsafe impl Sync for VarsCell {}

struct VarsGuard<'a>(&'a VarsCell);

impl VarsCell {
    fn new(vars: Vars) -> Self {
        VarsCell { locked: std::sync::atomic::AtomicBool::new(false), vars: std::cell::UnsafeCell::new(vars) }
    }

    fn lock(&self) -> VarsGuard<'_> {
        use std::sync::atomic::Ordering::{Acquire, Relaxed};
        while self.locked.compare_exchange_weak(false, true, Acquire, Relaxed).is_err() {
            std::hint::spin_loop();
        }
        VarsGuard(self)
    }
}

impl std::ops::Deref for VarsGuard<'_> {
    type Target = Vars;
    fn deref(&self) -> &Vars {
        // SAFETY: the guard holds the flag, so no other reference exists.
        unsafe { &*self.0.vars.get() }
    }
}

impl std::ops::DerefMut for VarsGuard<'_> {
    fn deref_mut(&mut self) -> &mut Vars {
        // SAFETY: as above.
        unsafe { &mut *self.0.vars.get() }
    }
}

impl Drop for VarsGuard<'_> {
    fn drop(&mut self) {
        self.0.locked.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// A variable's name, kept in place when short (as nearly all are), so
/// binding a parameter or loop variable needs no allocation.
enum Name {
    Short(u8, [u8; 22]),
    Long(Box<str>),
}

impl Name {
    fn new(s: &str) -> Name {
        if s.len() <= 22 {
            let mut b = [0u8; 22];
            b[..s.len()].copy_from_slice(s.as_bytes());
            Name::Short(s.len() as u8, b)
        } else {
            Name::Long(s.into())
        }
    }

    fn as_str(&self) -> &str {
        match self {
            // SAFETY: the bytes were copied from a `str`, whole.
            Name::Short(n, b) => unsafe { std::str::from_utf8_unchecked(&b[..*n as usize]) },
            Name::Long(s) => s,
        }
    }
}

/// A function's or block's few variables are found fastest by comparing
/// names in order; the globals (every builtin and definition) by hashing.
enum Vars {
    /// The first few in place (no allocation besides the scope's own),
    /// then a vector.
    Few([Option<(Name, Value)>; INLINE], Vec<(Name, Value)>),
    Many(HashMap<Box<str>, Value, BuildFx>),
}

const INLINE: usize = 4;

/// Past this many, a scope's variables move to a hash table.
const FEW: usize = 16;

impl Vars {
    fn few() -> Vars {
        Vars::Few([None, None, None, None], Vec::new())
    }

    fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Vars::Few(slots, more) => slots
                .iter()
                .flatten()
                .chain(more.iter())
                .find(|(n, _)| n.as_str() == name)
                .map(|(_, x)| x),
            Vars::Many(m) => m.get(name),
        }
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        match self {
            Vars::Few(slots, more) => slots
                .iter_mut()
                .flatten()
                .chain(more.iter_mut())
                .find(|(n, _)| n.as_str() == name)
                .map(|(_, x)| x),
            Vars::Many(m) => m.get_mut(name),
        }
    }

    fn insert(&mut self, name: &str, value: Value) {
        if let Some(slot) = self.get_mut(name) {
            *slot = value;
            return;
        }
        match self {
            Vars::Few(slots, more) => {
                if let Some(free) = slots.iter_mut().find(|s| s.is_none()) {
                    *free = Some((Name::new(name), value));
                } else if more.len() + INLINE < FEW {
                    more.push((Name::new(name), value));
                } else {
                    let mut m: HashMap<Box<str>, Value, BuildFx> = slots
                        .iter_mut()
                        .filter_map(|s| s.take())
                        .chain(more.drain(..))
                        .map(|(k, x)| (k.as_str().into(), x))
                        .collect();
                    m.insert(name.into(), value);
                    *self = Vars::Many(m);
                }
            }
            Vars::Many(m) => {
                m.insert(name.into(), value);
            }
        }
    }

    /// Empties it, keeping its room, for a scope used again.
    fn clear(&mut self) {
        match self {
            Vars::Few(slots, more) => {
                for s in slots.iter_mut() {
                    if let Some((_, v)) = s.take() {
                        drop_value(v);
                    }
                }
                for (_, v) in more.drain(..) {
                    drop_value(v);
                }
            }
            Vars::Many(_) => *self = Vars::few(),
        }
    }

    fn entries(&self) -> Vec<(String, Value)> {
        match self {
            Vars::Few(slots, more) => {
                slots.iter().flatten().chain(more.iter()).map(|(k, x)| (k.as_str().to_string(), x.clone())).collect()
            }
            Vars::Many(m) => m.iter().map(|(k, x)| (k.to_string(), x.clone())).collect(),
        }
    }
}

/// FxHash (as in rustc): much faster than the default SipHash for short
/// names. Only for names from the program's source, never for data from
/// requests, so it needs no protection against crafted collisions.
#[derive(Default, Clone, Copy)]
pub struct Fx(u64);

impl std::hash::Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        const K: u64 = 0x517c_c1b7_2722_0a95;
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            let w = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
            self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(K);
        }
        for &b in chunks.remainder() {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(K);
        }
    }

    fn write_u8(&mut self, b: u8) {
        self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub type BuildFx = std::hash::BuildHasherDefault<Fx>;

impl Scope {
    pub fn root() -> Env {
        Arc::new(Scope { vars: VarsCell::new(Vars::Many(HashMap::default())), parent: None })
    }

    pub fn child(parent: &Env) -> Env {
        if let Some(mut e) = SPARE.with(|s| s.borrow_mut().pop()) {
            if let Some(s) = Arc::get_mut(&mut e) {
                s.parent = Some(parent.clone());
                return e;
            }
        }
        Arc::new(Scope { vars: VarsCell::new(Vars::few()), parent: Some(parent.clone()) })
    }

    /// Ends a scope made with `child`. If nothing else holds it (no
    /// function made in it is still around), it is emptied and kept for the
    /// next `child`, saving an allocation per call, block and loop step.
    pub fn done(mut e: Env) {
        let Some(s) = Arc::get_mut(&mut e) else { return };
        s.parent = None;
        s.vars.vars.get_mut().clear();
        SPARE.with(|sp| {
            let mut sp = sp.borrow_mut();
            if sp.len() < SPARE_SCOPES {
                sp.push(e);
            }
        });
    }

    /// Empties a scope for another loop step, if nothing else holds it (a
    /// function made in the last step would): then it can be used again.
    pub fn reuse(e: &mut Env) -> bool {
        match Arc::get_mut(e) {
            Some(s) => {
                s.vars.vars.get_mut().clear();
                true
            }
            None => false,
        }
    }

    fn vars(&self) -> VarsGuard<'_> {
        self.vars.lock()
    }
}

/// The variables defined directly in a scope (not its parents).
pub fn scope_vars(env: &Env) -> Vec<(String, Value)> {
    env.vars().entries()
}

pub fn lookup(env: &Env, name: &str) -> Option<Value> {
    let mut cur: &Scope = env;
    loop {
        if let Some(v) = cur.vars().get(name) {
            return Some(v.clone());
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Whether `env` is `root` or a scope inside it.
pub fn within(env: &Env, root: &Env) -> bool {
    let mut cur: &Scope = env;
    loop {
        if std::ptr::eq(cur, &**root) {
            return true;
        }
        cur = match cur.parent.as_deref() {
            Some(p) => p,
            None => return false,
        };
    }
}

/// The nearest function named `name` in scope, passing over other values
/// of that name: `xs.keys` still calls `keys` where a local `keys` is a list.
pub fn lookup_fn(env: &Env, name: &str) -> Option<Value> {
    let mut cur: &Scope = env;
    loop {
        if let Some(v @ (Value::Func(_) | Value::Builtin(..))) = cur.vars().get(name) {
            return Some(v.clone());
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Calls `f` with local variable `name` where it is, without copying it;
/// None if it is not a local (globals are read through `lookup`, which the
/// view cache follows).
pub fn with_local<R>(env: &Env, name: &str, f: impl FnOnce(&Value) -> R) -> Option<R> {
    let mut cur: &Scope = env;
    loop {
        let parent = cur.parent.as_deref()?;
        if let Some(v) = cur.vars().get(name) {
            return Some(f(v));
        }
        cur = parent;
    }
}

pub fn define(env: &Env, name: &str, v: Value) {
    env.vars().insert(name, v);
}

/// Like `lookup`, and whether the variable is a global.
pub fn lookup_global(env: &Env, name: &str) -> Option<(Value, bool)> {
    let mut cur: &Scope = env;
    loop {
        if let Some(v) = cur.vars().get(name) {
            return Some((v.clone(), cur.parent.is_none()));
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Like `assign`; `Some(true)` when the variable was a global.
pub fn assign_global(env: &Env, name: &str, v: Value) -> Option<bool> {
    let mut cur: &Scope = env;
    loop {
        if let Some(slot) = cur.vars().get_mut(name) {
            *slot = v;
            return Some(cur.parent.is_none());
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Updates an existing variable in the nearest scope that has it.
pub fn assign(env: &Env, name: &str, v: Value) -> bool {
    assign_global(env, name, v).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_short_and_long_read_as_their_text() {
        let long = "a key longer than twenty-two bytes";
        let mut m = Map::new();
        m.insert("id".into(), Value::Int(1));
        m.insert(long.into(), Value::Int(2));
        m.insert(String::from("ünï").into(), Value::Int(3));
        assert!(matches!(m.get("id"), Some(Value::Int(1))));
        assert!(matches!(m.get(long), Some(Value::Int(2))));
        assert!(matches!(m.get("ünï"), Some(Value::Int(3))));
        assert!(m.get("i").is_none());
        let keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, ["id", long, "ünï"]);
        assert!(Key::from("a") < Key::from(long) && Key::from(long) == Key::new(long));
    }

    #[test]
    fn json_text_is_what_serde_json_writes() {
        let mut m = Map::new();
        m.insert("id".into(), Value::Int(-42));
        m.insert("quote \" and \\".into(), Value::str("tab\t, newline\n, \u{1} and フレームワーク <b>"));
        m.insert("f".into(), Value::Float(2.5));
        m.insert("whole".into(), Value::Float(3.0));
        m.insert("big".into(), Value::Float(1e300));
        m.insert("nan".into(), Value::Float(f64::NAN));
        m.insert("none".into(), Value::Nil);
        m.insert("yes".into(), Value::Bool(true));
        m.insert("html".into(), Value::html("<p>\"x\"</p>"));
        m.insert("err".into(), Value::err(Value::str("bad")));
        m.insert("empty".into(), Value::map(Map::new()));
        let v = Value::list(vec![Value::map(m), Value::list(vec![]), Value::Int(i64::MIN), Value::str("")]);
        assert_eq!(v.json_text(), v.to_json().to_string());
    }
}
