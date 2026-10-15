use crate::ast::FnDef;
use crate::interp::{Ctrl, Interp};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

pub type Map = IndexMap<String, Value>;
pub type BuiltinFn = fn(&mut Interp, Vec<Value>, u32) -> Result<Value, Ctrl>;

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    List(Rc<Vec<Value>>),
    Map(Rc<Map>),
    Func(Rc<Closure>),
    Builtin(&'static str, BuiltinFn),
    /// Already-rendered, safe HTML.
    Html(Rc<str>),
    Response(Rc<Response>),
    /// An error value: `err(x)`, a failed `int("x")`, ...
    Err(Rc<Value>),
}

pub struct Closure {
    pub def: Rc<FnDef>,
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
        Value::Str(Rc::from(s.as_ref()))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(items))
    }

    pub fn map(m: Map) -> Value {
        Value::Map(Rc::new(m))
    }

    pub fn html(s: impl AsRef<str>) -> Value {
        Value::Html(Rc::from(s.as_ref()))
    }

    pub fn err(v: Value) -> Value {
        Value::Err(Rc::new(v))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
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
            Value::Nil => "nil".into(),
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
        (Func(x), Func(y)) => Rc::ptr_eq(x, y),
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

pub type Env = Rc<RefCell<Scope>>;

pub struct Scope {
    vars: HashMap<String, Value>,
    parent: Option<Env>,
}

impl Scope {
    pub fn root() -> Env {
        Rc::new(RefCell::new(Scope { vars: HashMap::new(), parent: None }))
    }

    pub fn child(parent: &Env) -> Env {
        Rc::new(RefCell::new(Scope { vars: HashMap::new(), parent: Some(parent.clone()) }))
    }
}

pub fn lookup(env: &Env, name: &str) -> Option<Value> {
    let mut cur = env.clone();
    loop {
        let parent = {
            let s = cur.borrow();
            if let Some(v) = s.vars.get(name) {
                return Some(v.clone());
            }
            s.parent.clone()
        };
        cur = parent?;
    }
}

pub fn define(env: &Env, name: &str, v: Value) {
    env.borrow_mut().vars.insert(name.to_string(), v);
}

/// Updates an existing variable in the nearest scope that has it.
pub fn assign(env: &Env, name: &str, v: Value) -> bool {
    let mut cur = env.clone();
    loop {
        let parent = {
            let mut s = cur.borrow_mut();
            if let Some(slot) = s.vars.get_mut(name) {
                *slot = v;
                return true;
            }
            s.parent.clone()
        };
        match parent {
            Some(p) => cur = p,
            None => return false,
        }
    }
}
