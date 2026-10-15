use crate::ast::FnDef;
use crate::interp::{Ctrl, Interp};
use indexmap::IndexMap;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

pub type Map = IndexMap<String, Value>;
pub type BuiltinFn = fn(&mut Interp, Vec<Value>, u32) -> Result<Value, Ctrl>;

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
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
            _ => self.repr(),
        }
    }

    /// Source-like form used by the REPL and error messages.
    pub fn repr(&self) -> String {
        match self {
            Value::Nil => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => fmt_float(*f),
            Value::Str(s) => serde_json::to_string(&**s).unwrap_or_default(),
            Value::List(l) => format!("[{}]", l.iter().map(|v| v.repr()).collect::<Vec<_>>().join(", ")),
            Value::Map(m) if m.is_empty() => "{}".into(),
            Value::Map(m) => format!(
                "{{ {} }}",
                m.iter().map(|(k, v)| format!("{}: {}", fmt_key(k), v.repr())).collect::<Vec<_>>().join(", ")
            ),
            Value::Func(c) => format!("<fn {}>", c.def.name),
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
            Value::Str(s) => J::String(s.to_string()),
            Value::Html(h) => J::String(h.to_string()),
            Value::List(l) => J::Array(l.iter().map(|v| v.to_json()).collect()),
            Value::Map(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
            Value::Err(e) => serde_json::json!({ "error": e.to_json() }),
            Value::Response(r) => serde_json::json!({ "status": r.status }),
            Value::Func(_) | Value::Builtin(..) => J::String(self.repr()),
        }
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
            J::Object(o) => Value::map(o.iter().map(|(k, v)| (k.clone(), Value::from_json(v))).collect()),
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
        (Str(x), Str(y)) => x == y,
        (Html(x), Html(y)) => x == y,
        (List(x), List(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| equal(p, q)),
        (Map(x), Map(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).map_or(false, |w| equal(v, w))),
        (Err(x), Err(y)) => equal(x, y),
        (Func(x), Func(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        _ => a.as_f64()?.partial_cmp(&b.as_f64()?),
    }
}

// ---- environments ------------------------------------------------------

pub type Env = Arc<Scope>;

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
    Few(Vec<(Name, Value)>),
    Many(HashMap<Box<str>, Value, BuildFx>),
}

/// Past this many, a scope's variables move to a hash table.
const FEW: usize = 16;

impl Vars {
    fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Vars::Few(v) => v.iter().rev().find(|(n, _)| n.as_str() == name).map(|(_, x)| x),
            Vars::Many(m) => m.get(name),
        }
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        match self {
            Vars::Few(v) => v.iter_mut().rev().find(|(n, _)| n.as_str() == name).map(|(_, x)| x),
            Vars::Many(m) => m.get_mut(name),
        }
    }

    fn insert(&mut self, name: &str, value: Value) {
        if let Some(slot) = self.get_mut(name) {
            *slot = value;
            return;
        }
        match self {
            Vars::Few(v) if v.len() < FEW => v.push((Name::new(name), value)),
            Vars::Few(v) => {
                let mut m: HashMap<Box<str>, Value, BuildFx> =
                    std::mem::take(v).into_iter().map(|(k, x)| (k.as_str().into(), x)).collect();
                m.insert(name.into(), value);
                *self = Vars::Many(m);
            }
            Vars::Many(m) => {
                m.insert(name.into(), value);
            }
        }
    }

    fn entries(&self) -> Vec<(String, Value)> {
        match self {
            Vars::Few(v) => v.iter().map(|(k, x)| (k.as_str().to_string(), x.clone())).collect(),
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
        Arc::new(Scope { vars: VarsCell::new(Vars::Few(Vec::new())), parent: Some(parent.clone()) })
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
