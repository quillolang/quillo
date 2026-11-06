use crate::ast::FnDef;
use crate::interp::{Ctrl, Interp};
use crate::sym::{Name, Sym};
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
    /// An error value: `err(x)`, a failed `int("x")`, ... The flag says the program wrote it,
    /// to be read: what a builtin makes can say what no visitor should see, so it has to be
    /// marked, as the text cannot tell them apart (FEEDBACK21.md, Q-171; branch
    /// feedback20-fixes). The place is where it was made (FEEDBACK21.md, Q-174).
    Err(Arc<Value>, bool, Place),
}

/// Where an error value was made, for the log: a file and a line in four bytes, which fit
/// beside its flag, so that no other value grows for it. Files go by a number, from the table
/// below; a program of more than `FILE_BITS` worth loses the file, and past `LINE_BITS` the
/// line (FEEDBACK21.md, Q-174; branch feedback20-fixes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Place(u32);

const LINE_BITS: u32 = 20;
const FILE_BITS: u32 = 32 - LINE_BITS;
static PLACE_FILES: std::sync::Mutex<Vec<Arc<str>>> = std::sync::Mutex::new(Vec::new());

impl Place {
    pub const NONE: Place = Place(0);

    pub fn new(file: Option<&Arc<str>>, line: u32) -> Place {
        let line = if line < 1 << LINE_BITS { line } else { 0 };
        let file = file.map_or(0, |f| {
            let mut all = PLACE_FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let at = all.iter().position(|x| x == f).unwrap_or_else(|| {
                all.push(f.clone());
                all.len() - 1
            });
            if at + 1 < 1 << FILE_BITS { at as u32 + 1 } else { 0 }
        });
        Place(file << LINE_BITS | line)
    }

    /// `text`, after where it was made.
    pub fn before(self, text: &str) -> String {
        match self.to_string().as_str() {
            "" => text.to_string(),
            at => format!("{}: {}", at, text),
        }
    }
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let (file, line) = ((self.0 >> LINE_BITS) as usize, self.0 & ((1 << LINE_BITS) - 1));
        let name = file.checked_sub(1).and_then(|i| PLACE_FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(i).cloned());
        match (name, line) {
            (Some(n), 0) => write!(f, "{}", crate::error::shown_path(&n)),
            (Some(n), l) => write!(f, "{}:{}", crate::error::shown_path(&n), l),
            (None, 0) => Ok(()),
            (None, l) => write!(f, "line {}", l),
        }
    }
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
        Value::Err(Arc::new(v), false, Place::NONE)
    }

    /// An error the program means a visitor to read.
    pub fn own_err(v: Value) -> Value {
        Value::Err(Arc::new(v), true, Place::NONE)
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
            Value::Err(..) => "error",
        }
    }

    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false) | Value::Err(..))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Text form used by string interpolation and `str()`. An error the program
    /// did not write stays hidden in it, on a server (see `hiding`).
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
        let mut out = String::new();
        self.repr_into(&mut out);
        out
    }

    /// `repr`, written into `out`. A value nests as deeply as code may
    /// (`parser::MAX_NESTING`), and `str` of it runs on the caller's stack,
    /// so each level takes little: lists, maps and errors here, the rest
    /// (and their formatting's locals) in `scalar_repr`.
    fn repr_into(&self, out: &mut String) {
        self.repr_at(out, 0)
    }

    /// `repr_into`, `depth` levels down: past `DEEPEST`, `…` (FEEDBACK51.md, Q-661).
    fn repr_at(&self, out: &mut String, depth: usize) {
        if depth > DEEPEST && matches!(self, Value::List(_) | Value::Map(_) | Value::Err(..)) {
            return out.push('…');
        }
        match self {
            Value::List(l) => {
                out.push('[');
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.repr_at(out, depth + 1);
                }
                out.push(']');
            }
            Value::Map(m) if m.is_empty() => out.push_str("{}"),
            Value::Map(m) => {
                out.push_str("{ ");
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    key_into(k, out);
                    v.repr_at(out, depth + 1);
                }
                out.push_str(" }");
            }
            Value::Err(e, own, at) => {
                out.push_str("err(");
                if !own && hiding() {
                    crate::server::log_hidden(e, at);
                    out.push_str("\"internal error\"");
                } else {
                    e.repr_at(out, depth + 1);
                }
                out.push(')');
            }
            other => other.scalar_repr(out),
        }
    }

    #[inline(never)]
    fn scalar_repr(&self, out: &mut String) {
        use std::fmt::Write;
        let _ = match self {
            Value::Nil => write!(out, "null"),
            Value::Bool(b) => write!(out, "{}", b),
            Value::Int(n) => write!(out, "{}", n),
            Value::Float(f) => write!(out, "{}", fmt_float(*f)),
            Value::Dec(d) => write!(out, "{}m", d),
            Value::Str(s) => write!(out, "{}", serde_json::to_string(&**s).unwrap_or_default()),
            Value::Func(c) => write!(out, "<fn {}>", c.def.name),
            Value::Builtin(n, _) if n.starts_with("native:") => write!(out, "<rust fn {}>", crate::native::display_name(n)),
            Value::Builtin(n, _) => write!(out, "<builtin {}>", n),
            Value::Html(h) => write!(out, "{}", h),
            Value::Response(r) => write!(out, "<response {}>", r.status),
            Value::List(_) | Value::Map(_) | Value::Err(..) => Ok(self.repr_into(out)),
        };
    }

    pub fn to_json(&self) -> serde_json::Value {
        self.to_json_at(0)
    }

    /// `to_json`, `depth` levels down: past `DEEPEST`, null (FEEDBACK51.md, Q-661).
    fn to_json_at(&self, depth: usize) -> serde_json::Value {
        use serde_json::Value as J;
        if depth > DEEPEST && matches!(self, Value::List(_) | Value::Map(_) | Value::Err(..)) {
            return J::Null;
        }
        match self {
            Value::Nil => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(n) => J::from(*n),
            Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
            // As text, so nothing that reads it rounds it to a float.
            Value::Dec(d) => J::String(d.to_string()),
            Value::Str(s) => J::String(s.to_string()),
            Value::Html(h) => J::String(h.to_string()),
            Value::List(l) => J::Array(l.iter().map(|v| v.to_json_at(depth + 1)).collect()),
            Value::Map(m) => J::Object(m.iter().map(|(k, v)| (k.to_string(), v.to_json_at(depth + 1))).collect()),
            Value::Err(e, _, _) => serde_json::json!({ "error": e.to_json_at(depth + 1) }),
            Value::Response(r) => serde_json::json!({ "status": r.status }),
            Value::Func(_) | Value::Builtin(..) => J::String(self.repr()),
        }
    }

    /// `json_into`, held to `max` bytes and `depth` levels as it writes: `[xs, xs]` twenty-four
    /// times over made a 100 MB text from 3.5 GB of memory, and a list nested 100,000 deep
    /// overflowed the stack and stopped the server (FEEDBACK49.md, Q-606 and Q-592; branch
    /// main). Checked while written, the text itself counted, escapes and all, and no tree
    /// made first: a walk before it cost every JSON answer a third more (found in review).
    pub fn json_into_within(&self, out: &mut Vec<u8>, max: usize, depth: usize) -> Result<(), String> {
        // Scalars are written where they stand, and the size is looked at once a container
        // ends: what makes JSON grow past its limit is containers, shared or nested.
        fn go(v: &Value, out: &mut Vec<u8>, max: usize, left: usize, depth: usize) -> Result<(), String> {
            let inner = |x: &Value, out: &mut Vec<u8>| -> Result<(), String> {
                match x {
                    Value::List(_) | Value::Map(_) | Value::Err(..) => go(x, out, max, left - 1, depth),
                    Value::Func(_) | Value::Builtin(..) => Err(FUNCTION_IN_JSON.into()),
                    scalar => {
                        scalar.json_into(out);
                        Ok(())
                    }
                }
            };
            // Where in the value a function was met, from the outside in: `items[2].f`.
            let at = |e: String, step: &dyn Fn() -> String| -> String {
                match e.strip_prefix(FUNCTION_IN_JSON) {
                    Some(rest) => format!("{}{}{}", FUNCTION_IN_JSON, step(), rest),
                    None => e,
                }
            };
            match v {
                Value::List(_) | Value::Map(_) | Value::Err(..) if left == 0 => return Err(format!("it is nested more than {} deep", depth)),
                Value::List(l) => {
                    out.push(b'[');
                    for (i, x) in l.iter().enumerate() {
                        if i > 0 {
                            out.push(b',');
                        }
                        inner(x, out).map_err(|e| at(e, &|| format!("[{}]", i)))?;
                    }
                    out.push(b']');
                }
                Value::Map(m) => {
                    out.push(b'{');
                    for (i, (k, x)) in m.iter().enumerate() {
                        if i > 0 {
                            out.push(b',');
                        }
                        json_str_into(k, out);
                        out.push(b':');
                        inner(x, out).map_err(|e| at(e, &|| if fmt_key(k).starts_with('"') { format!("[{}]", fmt_key(k)) } else { format!(".{}", k) }))?;
                    }
                    out.push(b'}');
                }
                Value::Err(e, _, _) => {
                    out.extend_from_slice(b"{\"error\":");
                    inner(e, out).map_err(|e| at(e, &|| ".error".into()))?;
                    out.push(b'}');
                }
                Value::Func(_) | Value::Builtin(..) => return Err(FUNCTION_IN_JSON.into()),
                scalar => scalar.json_into(out),
            }
            if out.len() > max {
                return Err(format!("its JSON would be more than {} MiB", max >> 20));
            }
            Ok(())
        }
        // A function has no JSON: it was written as its text, `"<fn f>"`, where JavaScript leaves
        // it out and Python refuses it; said, and where it is (FEEDBACK55.md, Q-792; branch
        // feedback55).
        go(self, out, max, depth + 1, depth).map_err(|e| match e.strip_prefix(FUNCTION_IN_JSON) {
            Some("") => "a function cannot be written as JSON".into(),
            Some(path) => format!("`{}` is a function, which cannot be written as JSON", path.trim_start_matches('.')),
            None => e,
        })
    }

    /// The JSON text of `to_json`, written straight into `out`.
    pub fn json_into(&self, out: &mut Vec<u8>) {
        self.json_into_at(out, 0)
    }

    fn json_into_at(&self, out: &mut Vec<u8>, depth: usize) {
        if depth > DEEPEST && matches!(self, Value::List(_) | Value::Map(_) | Value::Err(..)) {
            return out.extend_from_slice(b"null");
        }
        match self {
            Value::Nil => out.extend_from_slice(b"null"),
            Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Int(n) => {
                let _ = serde_json::to_writer(&mut *out, n);
            }
            // As JavaScript writes it (FEEDBACK55.md, Q-781; branch feedback55).
            Value::Float(f) => json_float_into(*f, out),
            Value::Dec(d) => json_str_into(&d.to_string(), out),
            Value::Str(s) | Value::Html(s) => json_str_into(s, out),
            Value::List(l) => {
                out.push(b'[');
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    v.json_into_at(out, depth + 1);
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
                    v.json_into_at(out, depth + 1);
                }
                out.push(b'}');
            }
            Value::Err(e, _, _) => {
                out.extend_from_slice(b"{\"error\":");
                e.json_into_at(out, depth + 1);
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

    /// JSON text as a value, read as `from_json` reads it, with no tree of serde_json's
    /// made and thrown away between: half of the time it took (FEEDBACK29.md; branch
    /// feedback29-fixes).
    ///
    /// At most `max(1,000,000, text.len() / 8)` values: a body of tiny ones, `[0,0,...]`,
    /// took 100 times its size, 1 GB for a visitor's 10 MB (FEEDBACK51.md, Q-660; branch
    /// feedback51-fixes). Past it, an error that says so.
    pub fn parse_json(text: &str) -> serde_json::Result<Value> {
        let budget = (text.len() / 8).max(JSON_VALUES);
        let outer = JSON_LEFT.with(|l| l.replace(budget));
        let r = serde_json::from_str::<Parsed>(text).map(|p| p.0);
        JSON_LEFT.with(|l| l.set(outer));
        r
    }
}

/// A value read from JSON (`Value::parse_json`).
struct Parsed(Value);

struct ParsedKey(Key);

impl<'de> serde::Deserialize<'de> for Parsed {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Parsed, D::Error> {
        d.deserialize_any(ParsedVisitor)
    }
}

impl<'de> serde::Deserialize<'de> for ParsedKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<ParsedKey, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = ParsedKey;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E>(self, s: &str) -> Result<ParsedKey, E> {
                Ok(ParsedKey(Key::new(s)))
            }
        }
        d.deserialize_str(V)
    }
}

struct ParsedVisitor;

/// The fewest values `Value::parse_json` takes from any text.
pub const JSON_VALUES: usize = 1_000_000;

thread_local! {
    /// How many more values the JSON being read may have (see `Value::parse_json`).
    static JSON_LEFT: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// One more value read from JSON, or an error past the budget.
fn one_more<E: serde::de::Error>() -> Result<(), E> {
    JSON_LEFT.with(|l| match l.get() {
        0 => Err(E::custom("more values than JSON of its length may have")),
        n => {
            l.set(n - 1);
            Ok(())
        }
    })
}

impl<'de> serde::de::Visitor<'de> for ParsedVisitor {
    type Value = Parsed;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("JSON")
    }

    fn visit_unit<E>(self) -> Result<Parsed, E> {
        Ok(Parsed(Value::Nil))
    }

    fn visit_bool<E>(self, b: bool) -> Result<Parsed, E> {
        Ok(Parsed(Value::Bool(b)))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Parsed, E> {
        Ok(Parsed(Value::Int(n)))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Parsed, E> {
        Ok(Parsed(i64::try_from(n).map_or(Value::Float(n as f64), Value::Int)))
    }

    fn visit_f64<E>(self, n: f64) -> Result<Parsed, E> {
        Ok(Parsed(Value::Float(n)))
    }

    fn visit_str<E>(self, s: &str) -> Result<Parsed, E> {
        Ok(Parsed(Value::str(s)))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Parsed, A::Error> {
        let mut out = Vec::new();
        while let Some(Parsed(v)) = seq.next_element()? {
            one_more()?;
            out.push(v);
        }
        Ok(Parsed(Value::list(out)))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Parsed, A::Error> {
        let mut out = Map::new();
        while let Some((ParsedKey(k), Parsed(v))) = map.next_entry()? {
            one_more()?;
            out.insert(k, v);
        }
        Ok(Parsed(Value::map(out)))
    }
}

fn fmt_float(f: f64) -> String {
    if JS_NUMBERS.with(|j| j.get()) {
        return js_number(f);
    }
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{:.1}", f)
    } else {
        // Rust's `{}`, with the digit JavaScript takes on a tie (FEEDBACK55.md, Q-777; branch
        // feedback55): the same text, no exponent, `NaN`, `inf` and `-inf` as before.
        js_number(f)
    }
}

/// Up to 17 digits, with no zero at either end, and where the point goes: x = 0.digits × 10^n.
struct Digits {
    d: [u8; 32],
    k: usize,
    n: i32,
}

/// The shortest digits that give `f` (finite, not zero) back. Of two as short and as near, the
/// even one, as JavaScript's Number::toString takes it; Rust's `{}` takes the other, so
/// `1916218016406818.25` was `...818.3` here and `...818.2` in the browser (FEEDBACK55.md, Q-777;
/// branch feedback55). Read from serde_json's text (`1.5e-6`, `1e+16`, `0.001`, `2.0`), five
/// times as fast as Rust's `{:e}`, and the tie made sure of here.
fn shortest(f: f64) -> Digits {
    let mut text = [0u8; 40];
    let len = {
        let mut w = &mut text[..];
        let _ = serde_json::to_writer(&mut w, &f.abs());
        40 - w.len()
    };
    let t = &text[..len];
    let (mant, exp) = match t.iter().position(|&c| c == b'e') {
        Some(i) => (&t[..i], std::str::from_utf8(&t[i + 1..]).ok().and_then(|e| e.trim_start_matches('+').parse::<i32>().ok()).unwrap_or(0)),
        None => (t, 0),
    };
    let mut out = Digits { d: [0; 32], k: 0, n: 0 };
    let (mut point, mut lead, mut whole, mut zeros) = (false, true, 0i32, 0i32);
    for &c in mant {
        if c == b'.' {
            point = true;
        } else if lead && c == b'0' {
            zeros += point as i32;
        } else if out.k < out.d.len() {
            lead = false;
            whole += !point as i32;
            out.d[out.k] = c;
            out.k += 1;
        }
    }
    while out.k > 1 && out.d[out.k - 1] == b'0' {
        out.k -= 1;
    }
    out.n = if whole > 0 { whole } else { -zeros } + exp;
    if let Some((q, lower)) = tie_at(f) {
        let mut d = 0u128;
        for &c in &out.d[..out.k] {
            d = d * 10 + (c - b'0') as u128;
        }
        // Digits that end where the tie is are one of the two: the even one.
        if out.n - out.k as i32 == q && (d == lower || d == lower + 1) {
            let even = if lower % 2 == 0 { lower } else { lower + 1 };
            let text = even.to_string();
            out.n = text.len() as i32 + q;
            let text = text.trim_end_matches('0');
            out.d[..text.len()].copy_from_slice(text.as_bytes());
            out.k = text.len();
        }
    }
    out
}

/// Where `f` lies exactly halfway between two decimals of its shortest length that both give
/// it back: their last place, 10^q, and the lower as digits. That needs few binary digits after
/// the point, so a float almost never gets past the first test: x = m × 2^v (m odd) is halfway
/// between multiples of 10^q (q < 0) when v = q - 1, and both are near enough when 10^q is the
/// largest power of ten not above the spacing of floats there (2^e).
#[inline(always)]
fn tie_at(f: f64) -> Option<(i32, u128)> {
    let bits = f.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = (bits & ((1 << 52) - 1)) | (1 << 52);
    let e = biased - 1075;
    // floor(e × log10 2), for e < 0: e × log10 2 is never whole there.
    let q = -(((-e) * 78913) >> 18) - 1;
    // Below the normal floats the spacing is too fine for a short tie; inf and NaN have none.
    if e + mantissa.trailing_zeros() as i32 != q - 1 || biased == 0 || biased == 0x7ff || e >= 0 {
        return None;
    }
    tie_found(f, mantissa, q)
}

#[inline(never)]
fn tie_found(f: f64, mantissa: u64, q: i32) -> Option<(i32, u128)> {
    let tz = mantissa.trailing_zeros();
    let half = ((mantissa >> tz) as u128).checked_mul(5u128.checked_pow((-q) as u32)?)?;
    let lower = (half - 1) / 2;
    // At a power of two the floats below are closer together: both must give it back.
    let back = |d: u128| format!("{}e{}", d, q).parse::<f64>().ok() == Some(f.abs());
    (back(lower) && back(lower + 1)).then_some((q, lower))
}

/// A float in JSON as JavaScript writes it (`String(x)`): its shortest digits, with an exponent
/// from 1e21 up and below 1e-6, where serde_json's went to one from 1e16 and below 1e-5, so
/// `json.stringify` gave other text on each side; a whole one ends in `.0`, as before, so it
/// reads back as a float (FEEDBACK55.md, Q-781; branch feedback55). -0.0 stays `-0.0`, which
/// reads back as itself on both sides; inf and NaN are null, as JSON has neither.
fn json_float_into(f: f64, out: &mut Vec<u8>) {
    if !f.is_finite() {
        return out.extend_from_slice(b"null");
    }
    if f == 0.0 {
        return out.extend_from_slice(if f.is_sign_negative() { b"-0.0" } else { b"0.0" });
    }
    // serde_json's text is JavaScript's where there is no tie, but for the exponent it writes from
    // 1e16 and below 1e-5: kept as it is unless it has one of those (the usual case, three times
    // as fast as writing the digits again).
    if tie_at(f).is_none() {
        let start = out.len();
        let _ = serde_json::to_writer(&mut *out, &f);
        if (1e-4..1e15).contains(&f.abs()) {
            return;
        }
        let Some(at) = out[start..].iter().position(|&c| c == b'e') else { return };
        let e = std::str::from_utf8(&out[start + at + 1..]).ok().and_then(|e| e.trim_start_matches('+').parse::<i32>().ok());
        if e.is_some_and(|e| !(-7 < e && e < 21)) {
            return;
        }
        out.truncate(start);
    }
    let s = shortest(f);
    let (d, k, n) = (&s.d[..s.k], s.k as i32, s.n);
    if f < 0.0 {
        out.push(b'-');
    }
    if k <= n && n <= 21 {
        out.extend_from_slice(d);
        out.extend(std::iter::repeat_n(b'0', (n - k) as usize));
        out.extend_from_slice(b".0");
    } else if 0 < n && n <= 21 {
        out.extend_from_slice(&d[..n as usize]);
        out.push(b'.');
        out.extend_from_slice(&d[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.extend_from_slice(b"0.");
        out.extend(std::iter::repeat_n(b'0', (-n) as usize));
        out.extend_from_slice(d);
    } else {
        out.push(d[0]);
        if k > 1 {
            out.push(b'.');
            out.extend_from_slice(&d[1..]);
        }
        let _ = std::io::Write::write_fmt(out, format_args!("e{}{}", if n > 1 { "+" } else { "-" }, (n - 1).abs()));
    }
}

thread_local! {
    /// While the server renders a component, whose code also runs in the
    /// browser: floats are written as JavaScript writes them, so the page
    /// the server sends is the one the browser would make (see
    /// `js_numbers`).
    static JS_NUMBERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A server hides errors the program did not write: their text may hold an address
/// with its password, or a path. Turned into text they would lose the mark that says
/// so, and go out in a template string or `str()` with a 200, so here they are hidden
/// as they are turned into text, and said in the log (FEEDBACK26.md, Q-197; branch
/// feedback26-fixes). Not under `--watch`, where errors are shown to their author, and
/// not in a script, which has no visitors.
pub static HIDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    /// For `told`: text that goes to the log, not to a visitor.
    static TOLD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Set once a server starts: what is for a script alone (`stdin`, `exit`) is refused then.
pub static SERVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn serving() -> bool {
    SERVING.load(std::sync::atomic::Ordering::Relaxed)
}

thread_local! {
    /// While a program's top level runs, as it loads: nothing follows a global yet (no live
    /// page, no cached view, no other request), so one may be changed where it is, as a
    /// local is. That is where a script does all it does (FEEDBACK29.md, Q-353; branch
    /// feedback29-fixes).
    static LOADING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn loading() -> bool {
    LOADING.with(|l| l.get())
}

/// Sets whether a program is loading on this thread; what it was.
pub fn set_loading(on: bool) -> bool {
    LOADING.with(|l| l.replace(on))
}

pub fn hiding() -> bool {
    HIDING_HERE.with(|h| h.get()).unwrap_or_else(|| HIDING.load(std::sync::atomic::Ordering::Relaxed)) && !TOLD.with(|t| t.get())
}

thread_local! {
    /// Whether errors are hidden on this thread alone, whatever `HIDING` says (`hiding_here`).
    static HIDING_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// `f`, run with errors hidden (or shown) on this thread alone, as a server in production (or
/// under `--watch`) hides them: for a test, which set `HIDING` for every test of its process and
/// hid what the others' errors said (found in review of FEEDBACK55.md; branch feedback55).
pub fn hiding_here<R>(on: bool, f: impl FnOnce() -> R) -> R {
    let was = HIDING_HERE.with(|h| h.replace(Some(on)));
    let r = f();
    HIDING_HERE.with(|h| h.set(was));
    r
}

/// `f`, which makes text that stays on this machine, with errors as they are. It
/// does not wait: another request on this thread must not run inside it.
pub fn told<R>(f: impl FnOnce() -> R) -> R {
    let was = TOLD.with(|t| t.replace(true));
    let r = f();
    TOLD.with(|t| t.set(was));
    r
}

/// Runs `f` writing floats as JavaScript does (or not), as it was after.
pub fn js_numbers<R>(on: bool, f: impl FnOnce() -> R) -> R {
    let was = JS_NUMBERS.with(|j| j.replace(on));
    let r = f();
    JS_NUMBERS.with(|j| j.set(was));
    r
}

/// `f` (finite) in exponent form from its shortest digits, the even one on a tie (`shortest`), as
/// `{:e}` writes it: a decimal made from a float is the browser's, where `{:e}` took the other
/// digit (`dec(1e15 + 0.3)` was ...0.3m here and ...0.2m there; FEEDBACK55.md, Q-777; found in
/// review). Zero is `0e0`.
pub fn shortest_e(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0e0" } else { "0e0" }.into();
    }
    let s = shortest(f);
    // SAFETY: digits only.
    let digits = unsafe { std::str::from_utf8_unchecked(&s.d[..s.k]) };
    let mut text = String::with_capacity(s.k + 8);
    if f < 0.0 {
        text.push('-');
    }
    text.push_str(&digits[..1]);
    if s.k > 1 {
        text.push('.');
        text.push_str(&digits[1..]);
    }
    text.push('e');
    text.push_str(&(s.n - 1).to_string());
    text
}

/// A number as the page writes it: `8` for 8.0, with no exponent. An infinity
/// is `inf`, as everywhere else on both sides (FEEDBACK16.md, Q-120; branch
/// feedback20-fixes).
pub fn js_number(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.into();
    }
    if f == 0.0 {
        return "0".into();
    }
    // The shortest digits that give f back, and where the point goes: f = 0.digits × 10^n,
    // the even one on a tie, as the browser writes it (FEEDBACK55.md, Q-777; branch feedback55).
    let s = shortest(f);
    // SAFETY: digits only.
    let (digits, k, n) = (unsafe { std::str::from_utf8_unchecked(&s.d[..s.k]) }, s.k as i32, s.n);
    let mut text = String::with_capacity(k.max(n).max(1 - n) as usize + 3);
    if f < 0.0 {
        text.push('-');
    }
    // Never in exponent form, on either side (FEEDBACK25.md, Q-309; branch
    // feedback26-fixes).
    if k <= n {
        text.push_str(digits);
        text.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n {
        text.push_str(&digits[..n as usize]);
        text.push('.');
        text.push_str(&digits[n as usize..]);
    } else {
        text.push_str("0.");
        text.extend(std::iter::repeat_n('0', (-n) as usize));
        text.push_str(digits);
    }
    text
}

/// A map's key and its colon, as `repr` writes them.
#[inline(never)]
fn key_into(k: &str, out: &mut String) {
    out.push_str(&fmt_key(k));
    out.push_str(": ");
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

/// The mark `json_into_within` puts before where it met a function.
const FUNCTION_IN_JSON: &str = "\u{1}fn";

/// How deep a value is shown, written as JSON or compared, at most: past it, `…`, null, or not
/// ordered. A list nested 50,000 deep, as code that nests from data makes it, overflowed the
/// stack of the request and stopped the server (FEEDBACK51.md, Q-661; branch feedback51-fixes).
pub const DEEPEST: usize = 10_000;

/// Whether two values are the same, as `==` says: item by item, from a list of pairs still to
/// compare rather than by recursion, however deep they nest (Q-661).
pub fn equal(a: &Value, b: &Value) -> bool {
    use Value::*;
    let mut todo = vec![(a, b)];
    while let Some((a, b)) = todo.pop() {
        let same = match (a, b) {
            (List(x), List(y)) => x.len() == y.len() && {
                todo.extend(x.iter().zip(y.iter()));
                true
            },
            (Map(x), Map(y)) => {
                x.len() == y.len()
                    && x.iter().all(|(k, v)| match y.get(k) {
                        Some(w) => {
                            todo.push((v, w));
                            true
                        }
                        None => false,
                    })
            }
            (Err(x, ..), Err(y, ..)) => {
                todo.push((x, y));
                true
            }
            _ => scalar_equal(a, b),
        };
        if !same {
            return false;
        }
    }
    true
}

fn scalar_equal(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Nil, Nil) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        // Exactly, as Python has it: through a float, 9007199254740993 == 9007199254740992.0
        // (FEEDBACK55.md, Q-787; branch feedback55).
        (Int(i), Float(f)) | (Float(f), Int(i)) => int_float_cmp(*i, *f) == Some(Ordering::Equal),
        (Dec(_), Int(_) | Float(_) | Dec(_)) | (Int(_) | Float(_), Dec(_)) => {
            dec_pair(a, b).map_or(false, |(x, y)| x == y)
        }
        (Str(x), Str(y)) => x == y,
        (Html(x), Html(y)) => x == y,
        (Func(x), Func(y)) => Arc::ptr_eq(x, y),
        (Builtin(x, _), Builtin(y, _)) => x == y,
        _ => false,
    }
}

/// Ordered values; None for those that are not (a decimal and a float
/// are not: one is exact, the other not).
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    compare_at(a, b, 0)
}

fn compare_at(a: &Value, b: &Value, depth: usize) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        (Value::Dec(x), Value::Dec(y)) => Some(x.cmp(*y)),
        (Value::Dec(x), Value::Int(y)) => Some(x.cmp(crate::decimal::Dec::from_i64(*y))),
        (Value::Int(x), Value::Dec(y)) => Some(crate::decimal::Dec::from_i64(*x).cmp(*y)),
        // Item by item, which is how to sort by several keys: `sort_by(rows, (r) =>
        // [r.a, r.b])` (FEEDBACK25.md, Q-320; branch feedback26-fixes).
        (Value::List(x), Value::List(y)) => {
            if depth > DEEPEST {
                return None;
            }
            for (p, q) in x.iter().zip(y.iter()) {
                match compare_at(p, q, depth + 1)? {
                    Ordering::Equal => {}
                    other => return Some(other),
                }
            }
            Some(x.len().cmp(&y.len()))
        }
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        // An int and a float exactly, not the int made a float: `sort` put 9007199254740993
        // before 9007199254740992.0 (FEEDBACK55.md, Q-787; branch feedback55).
        (Value::Int(i), Value::Float(f)) => int_float_cmp(*i, *f),
        (Value::Float(f), Value::Int(i)) => int_float_cmp(*i, *f).map(Ordering::reverse),
        _ => None,
    }
}

/// An int against a float, exactly (None for NaN), as Python compares them: the int as a float
/// decides it unless the two are the same float, and then the float is whole and within an
/// int's reach, or 2^63, above every int. Rounding keeps order, so `i as f64 < f` means `i < f`
/// (FEEDBACK55.md, Q-787; branch feedback55). The JIT compiles the same steps (src/jit.rs,
/// `int_float_cmp_ir`).
#[inline]
pub fn int_float_cmp(i: i64, f: f64) -> Option<Ordering> {
    let fi = i as f64;
    if fi < f {
        Some(Ordering::Less)
    } else if fi > f {
        Some(Ordering::Greater)
    } else if fi == f {
        Some(if f >= 9223372036854775808.0 { Ordering::Less } else { i.cmp(&(f as i64)) })
    } else {
        None
    }
}

/// `items` without those `==` calls equal to one kept before them, in order: `unique`
/// (FEEDBACK55.md, Q-791; branch feedback55). Each was compared with every one kept, and 20,000
/// took 1.4 s; here an item is looked for only among those kept under its key, which `==` gives
/// equal values alike, and which values that are not equal share only rarely, whatever a visitor
/// sends (found in review: one key for all below 1e-10 made 20,000 tiny floats quadratic again):
///
/// - A float by its bits (-0.0 as 0.0), an int by the float that is exactly it (an int and a
///   float are equal only exactly) or else by itself, text by itself, lists item by item, maps
///   pair by pair in any order.
/// - `==` is exact everywhere but between a float and a decimal: the float is its shortest text
///   as a decimal, rounded to 28 places, so `1e-30 == 0m` and `2^60 == 1152921504606847000m`,
///   though `1e-30 != 0` and `2^60 != 1152921504606847000`. So a decimal is looked for by its
///   value among the decimals kept, the decimals floats kept are equal to, and the int it is; a
///   float or an int among the decimals kept by the decimal it is equal to.
/// - A list or a map holding a decimal, where those meet item by item, has a rough key: each
///   number by the float nearest the decimal it is equal to (a float's own, rounded so), which
///   every value it is equal to shares; it is looked for among those of its rough key.
///
/// What holds NaN (or an answer) is equal to nothing: kept without a look. Keyed for the process,
/// text by SipHash, so a visitor's values cannot be made to share keys.
pub fn unique_of(items: &[Value]) -> Vec<Value> {
    use std::collections::{HashMap, HashSet};
    use std::hash::BuildHasher;
    type Fast<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<Hashed>>;
    static KEY: std::sync::OnceLock<(std::collections::hash_map::RandomState, u64)> = std::sync::OnceLock::new();
    let (key, seed) = KEY.get_or_init(|| {
        let k = std::collections::hash_map::RandomState::new();
        let seed = k.hash_one(0u8);
        (k, seed)
    });
    const DEEP: u32 = 32;
    const END: u32 = u32::MAX;
    fn mix(h: u64, x: u64) -> u64 {
        // MurmurHash3's finish, after the two are put together.
        let mut x = (h.rotate_left(23) ^ x).wrapping_add(0x9e37_79b9_7f4a_7c15);
        x = (x ^ (x >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        x = (x ^ (x >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^ (x >> 33)
    }
    // A float's bits, -0.0 as 0.0.
    fn bits(f: f64) -> u64 {
        (f + 0.0).to_bits()
    }
    // A decimal's value, as a key: 12.5m and 12.50m are one.
    fn value(d: crate::decimal::Dec) -> (i128, u32) {
        let n = d.normalized();
        (n.mantissa(), n.scale())
    }
    struct K<'a> {
        key: &'a std::collections::hash_map::RandomState,
        seed: u64,
        rough: bool,
        dec: bool,
        never: bool,
    }
    impl K<'_> {
        fn go(&mut self, v: &Value, depth: u32) -> u64 {
            let seed = self.seed;
            match v {
                Value::Nil => mix(seed, 1),
                Value::Bool(b) => mix(seed, 2 + *b as u64),
                Value::Float(f) if f.is_nan() => {
                    self.never = true;
                    0
                }
                Value::Float(f) if self.rough => mix(seed ^ 3, bits(crate::decimal::Dec::from_f64(*f).map_or(*f, |d| d.to_f64()))),
                Value::Float(f) => mix(seed ^ 3, bits(*f)),
                Value::Int(n) if self.rough || (*n as f64) as i128 == *n as i128 => mix(seed ^ 3, bits(*n as f64)),
                Value::Int(n) => mix(seed ^ 7, *n as u64),
                Value::Dec(d) => {
                    self.dec = true;
                    mix(seed ^ 3, bits(d.to_f64()))
                }
                Value::Str(s) | Value::Html(s) => self.key.hash_one(&**s),
                Value::List(_) | Value::Map(_) | Value::Err(..) if depth > DEEP => mix(seed, 5),
                Value::List(l) => l.iter().fold(mix(seed, 11 ^ l.len() as u64), |h, x| mix(h, self.go(x, depth + 1))),
                Value::Map(m) => m.iter().fold(mix(seed, 13 ^ m.len() as u64), |h, (k, x)| {
                    let kh = self.key.hash_one(&**k);
                    h.wrapping_add(mix(kh, self.go(x, depth + 1)))
                }),
                Value::Err(e, ..) => mix(seed, self.go(e, depth + 1) ^ 17),
                Value::Builtin(n, _) => self.key.hash_one(n),
                Value::Func(f) => mix(seed, Arc::as_ptr(f) as u64),
                Value::Response(_) => {
                    self.never = true;
                    mix(seed, 19)
                }
            }
        }
    }
    let holder = |v: &Value| matches!(v, Value::List(_) | Value::Map(_) | Value::Err(..));
    let mut probe = K { key, seed: *seed, rough: false, dec: false, never: false };
    let exact: Vec<(u64, bool, bool)> = items
        .iter()
        .map(|x| {
            probe.dec = false;
            probe.never = false;
            let h = probe.go(x, 0);
            (h, probe.dec, probe.never)
        })
        .collect();
    let decs = exact.iter().any(|(_, d, _)| *d);
    let held_decs = items.iter().zip(&exact).any(|(x, (_, d, _))| *d && holder(x));
    let mut out: Vec<Value> = Vec::new();
    // What is kept, by key: where each key's chain starts, and the next.
    let mut first: Fast<u64, u32> = Default::default();
    let mut next: Vec<u32> = Vec::new();
    // The decimals kept, by value, and those the floats kept are equal to.
    let mut kept_decs: HashSet<(i128, u32)> = HashSet::new();
    let mut float_decs: HashSet<(i128, u32)> = HashSet::new();
    // Lists and maps, by rough key: those holding a decimal, and those not.
    let mut with_dec: Fast<u64, Vec<u32>> = Default::default();
    let mut without: Fast<u64, Vec<u32>> = Default::default();
    let mut rough = K { key, seed: *seed, rough: true, dec: false, never: false };
    let in_chain = |h: u64, x: &Value, out: &[Value], first: &Fast<u64, u32>, next: &[u32]| {
        let mut i = first.get(&h).copied().unwrap_or(END);
        while i != END {
            if equal(x, &out[i as usize]) {
                return true;
            }
            i = next[i as usize];
        }
        false
    };
    for (x, &(h, dec, never)) in items.iter().zip(&exact) {
        if never {
            out.push(x.clone());
            next.push(END);
            continue;
        }
        let r = if held_decs && holder(x) { rough.go(x, 0) } else { 0 };
        let seen = match x {
            Value::Dec(d) => {
                let v = value(*d);
                kept_decs.contains(&v)
                    || float_decs.contains(&v)
                    || d.trunc_i64().filter(|_| v.1 == 0).is_some_and(|i| {
                        let mut p = K { key, seed: *seed, rough: false, dec: false, never: false };
                        in_chain(p.go(&Value::Int(i), 0), x, &out, &first, &next)
                    })
            }
            _ if dec => [with_dec.get(&r), without.get(&r)].iter().flatten().any(|l| l.iter().any(|&i| equal(x, &out[i as usize]))),
            _ => {
                in_chain(h, x, &out, &first, &next)
                    || (decs
                        && match x {
                            Value::Float(f) => crate::decimal::Dec::from_f64(*f).is_some_and(|d| kept_decs.contains(&value(d))),
                            Value::Int(i) => kept_decs.contains(&value(crate::decimal::Dec::from_i64(*i))),
                            _ if held_decs && holder(x) => with_dec.get(&r).is_some_and(|l| l.iter().any(|&i| equal(x, &out[i as usize]))),
                            _ => false,
                        })
            }
        };
        if seen {
            continue;
        }
        let at = out.len() as u32;
        out.push(x.clone());
        next.push(END);
        match x {
            Value::Dec(d) => {
                kept_decs.insert(value(*d));
            }
            _ if dec => with_dec.entry(r).or_default().push(at),
            _ => {
                // At the chain's start: the newest first, as good as any.
                if let Some(head) = first.insert(h, at) {
                    next[at as usize] = head;
                }
                match x {
                    Value::Float(f) if decs => {
                        if let Some(d) = crate::decimal::Dec::from_f64(*f) {
                            float_decs.insert(value(d));
                        }
                    }
                    _ if held_decs && holder(x) => without.entry(r).or_default().push(at),
                    _ => {}
                }
            }
        }
    }
    out
}

/// A hasher for keys made mixed already (`unique_of`).
#[derive(Default)]
pub struct Hashed(u64);

impl std::hash::Hasher for Hashed {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, b: &[u8]) {
        for x in b {
            self.0 = (self.0 << 8) | *x as u64;
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = x;
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

/// The lock on the globals' variables. Quillo code runs under the interpreter
/// lock, so this is never contended; unlike `std::sync::Mutex` (which on
/// some systems allocates an OS lock), it is one flag in place. A scope inside
/// them is reached only from Quillo code, which that lock has one thread at a
/// time running, and a name looked up from deep in a program would pay for this
/// at each scope on the way (see `Scope::vars`; FEEDBACK29.md; branch
/// feedback29-fixes).
struct VarsCell {
    locked: std::sync::atomic::AtomicBool,
    vars: std::cell::UnsafeCell<Vars>,
}

// SAFETY: access goes through `lock`, which hands out one guard at a time, or through
// `inner`, under the interpreter lock.
unsafe impl Sync for VarsCell {}

struct VarsGuard<'a>(&'a VarsCell, bool);

impl VarsCell {
    fn new(vars: Vars) -> Self {
        VarsCell { locked: std::sync::atomic::AtomicBool::new(false), vars: std::cell::UnsafeCell::new(vars) }
    }

    fn lock(&self) -> VarsGuard<'_> {
        use std::sync::atomic::Ordering::{Acquire, Relaxed};
        while self.locked.compare_exchange_weak(false, true, Acquire, Relaxed).is_err() {
            std::hint::spin_loop();
        }
        VarsGuard(self, true)
    }

    #[inline(always)]
    fn inner(&self) -> VarsGuard<'_> {
        VarsGuard(self, false)
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
    #[inline(always)]
    fn drop(&mut self) {
        if self.1 {
            self.0.locked.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

/// A function's or block's few variables are found fastest by comparing
/// names in order; the globals (every builtin and definition) by hashing.
enum Vars {
    /// The first few in place (no allocation besides the scope's own),
    /// then a vector.
    /// Last, a bit for each name it holds (`Sym::bit`): most scopes a name is looked for
    /// in do not have it, and this says so in one read.
    Few([Option<(Sym, Value)>; INLINE], Vec<(Sym, Value)>, u64),
    Many(HashMap<Sym, Value, BuildFx>),
}

const INLINE: usize = 4;

/// Past this many, a scope's variables move to a hash table.
const FEW: usize = 16;

impl Vars {
    fn few() -> Vars {
        Vars::Few([None, None, None, None], Vec::new(), 0)
    }

    #[inline]
    fn get(&self, name: Sym) -> Option<&Value> {
        match self {
            Vars::Few(_, _, bits) if *bits & name.bit() == 0 => None,
            Vars::Few(slots, more, _) => {
                for (n, v) in slots.iter().flatten() {
                    if *n == name {
                        return Some(v);
                    }
                }
                more.iter().find(|(n, _)| *n == name).map(|(_, x)| x)
            }
            Vars::Many(m) => m.get(&name),
        }
    }

    #[inline]
    fn get_mut(&mut self, name: Sym) -> Option<&mut Value> {
        match self {
            Vars::Few(_, _, bits) if *bits & name.bit() == 0 => None,
            Vars::Few(slots, more, _) => {
                for (n, v) in slots.iter_mut().flatten() {
                    if *n == name {
                        return Some(v);
                    }
                }
                more.iter_mut().find(|(n, _)| *n == name).map(|(_, x)| x)
            }
            Vars::Many(m) => m.get_mut(&name),
        }
    }

    #[inline]
    fn insert(&mut self, name: Sym, value: Value) {
        if let Some(slot) = self.get_mut(name) {
            *slot = value;
            return;
        }
        match self {
            Vars::Few(slots, more, bits) => {
                *bits |= name.bit();
                if let Some(free) = slots.iter_mut().find(|s| s.is_none()) {
                    *free = Some((name, value));
                } else if more.len() + INLINE < FEW {
                    more.push((name, value));
                } else {
                    let mut m: HashMap<Sym, Value, BuildFx> = slots.iter_mut().filter_map(|s| s.take()).chain(more.drain(..)).collect();
                    m.insert(name, value);
                    *self = Vars::Many(m);
                }
            }
            Vars::Many(m) => {
                m.insert(name, value);
            }
        }
    }

    /// Empties it, keeping its room, for a scope used again.
    #[inline]
    fn clear(&mut self) {
        match self {
            Vars::Few(slots, more, bits) => {
                *bits = 0;
                for s in slots.iter_mut() {
                    if let Some((_, v)) = s.take() {
                        drop_value(v);
                    }
                }
                if !more.is_empty() {
                    for (_, v) in more.drain(..) {
                        drop_value(v);
                    }
                }
            }
            Vars::Many(_) => *self = Vars::few(),
        }
    }

    fn entries(&self) -> Vec<(String, Value)> {
        match self {
            Vars::Few(slots, more, _) => slots.iter().flatten().chain(more.iter()).map(|(k, x)| (k.to_string(), x.clone())).collect(),
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

    #[inline(always)]
    fn write_usize(&mut self, n: usize) {
        self.0 = (self.0.rotate_left(5) ^ n as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
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

    /// A loop step's scope given its variable's next value, if nothing else holds it and it
    /// holds that variable alone: the value back if not, for `reuse` and `define`.
    #[inline]
    pub fn rebind(e: &mut Env, name: Sym, v: Value) -> Result<(), Value> {
        if Arc::strong_count(e) != 1 {
            return Err(v);
        }
        let mut vars = e.vars();
        match &mut *vars {
            Vars::Few([Some((n, slot)), None, None, None], more, _) if *n == name && more.is_empty() => {
                *slot = v;
                Ok(())
            }
            _ => Err(v),
        }
    }

    /// Empties a scope for another loop step, if nothing else holds it (a
    /// function made in the last step would): then it can be used again.
    pub fn reuse(e: &mut Env) -> bool {
        // Nothing takes a weak hold of a scope, so one owner is enough to know it.
        if Arc::strong_count(e) != 1 {
            return false;
        }
        e.vars().clear();
        true
    }

    #[inline(always)]
    fn vars(&self) -> VarsGuard<'_> {
        if self.parent.is_some() {
            self.vars.inner()
        } else {
            self.vars.lock()
        }
    }
}

/// The scope around `env`, if any.
pub fn scope_parent(env: &Env) -> Option<&Env> {
    env.parent.as_ref()
}

/// The variables defined directly in a scope (not its parents).
pub fn scope_vars(env: &Env) -> Vec<(String, Value)> {
    env.vars().entries()
}

#[inline]
pub fn lookup(env: &Env, name: impl Name) -> Option<Value> {
    let name = name.known()?;
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
pub fn lookup_fn(env: &Env, name: impl Name) -> Option<Value> {
    let name = name.known()?;
    let mut cur: &Scope = env;
    loop {
        if let Some(v @ (Value::Func(_) | Value::Builtin(..))) = cur.vars().get(name) {
            return Some(v.clone());
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Calls `f` with the nearest variable `name` where it is, a global too, without copying it.
#[inline]
pub fn with_var<R>(env: &Env, name: Sym, f: impl FnOnce(&Value) -> R) -> Option<R> {
    let mut cur: &Scope = env;
    loop {
        if let Some(v) = cur.vars().get(name) {
            return Some(f(v));
        }
        cur = cur.parent.as_deref()?;
    }
}

/// The builtin the nearest variable `name` holds, if it holds one: its name and function.
#[inline]
pub fn builtin_named(env: &Env, name: Sym) -> Option<(&'static str, BuiltinFn)> {
    with_var(env, name, |v| match v {
        Value::Builtin(n, f) => Some((*n, *f)),
        _ => None,
    })?
}

/// Calls `f` with local variable `name` where it is, without copying it;
/// None if it is not a local (globals are read through `lookup`, which the
/// view cache follows).
#[inline]
pub fn with_local<R>(env: &Env, name: impl Name, f: impl FnOnce(&Value) -> R) -> Option<R> {
    let name = name.known()?;
    let mut cur: &Scope = env;
    loop {
        let parent = cur.parent.as_deref()?;
        if let Some(v) = cur.vars().get(name) {
            return Some(f(v));
        }
        cur = parent;
    }
}

#[inline]
pub fn define(env: &Env, name: impl Name, v: Value) {
    env.vars().insert(name.make(), v);
}

/// Like `lookup`, and whether the variable is a global.
pub fn lookup_global(env: &Env, name: impl Name) -> Option<(Value, bool)> {
    let name = name.known()?;
    let mut cur: &Scope = env;
    loop {
        if let Some(v) = cur.vars().get(name) {
            return Some((v.clone(), cur.parent.is_none()));
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Like `assign`; `Some(true)` when the variable was a global.
pub fn assign_global(env: &Env, name: impl Name, v: Value) -> Option<bool> {
    let name = name.known()?;
    let mut cur: &Scope = env;
    loop {
        if let Some(slot) = cur.vars().get_mut(name) {
            *slot = v;
            return Some(cur.parent.is_none());
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Takes a variable's value out of the nearest scope that has it, leaving
/// null there, unless that is the globals (but while the program loads): so
/// that in `m = set(m, k, v)` the map has one owner, and is changed rather
/// than copied.
pub fn take_local(env: &Env, name: impl Name) -> Option<Value> {
    let name = name.known()?;
    let mut cur: &Scope = env;
    loop {
        let global = cur.parent.is_none();
        if let Some(slot) = cur.vars().get_mut(name) {
            return (!global || loading()).then(|| std::mem::replace(slot, Value::Nil));
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Updates an existing variable in the nearest scope that has it.
pub fn assign(env: &Env, name: impl Name, v: Value) -> bool {
    assign_global(env, name, v).is_some()
}

/// Whether the nearest variable of this name may be taken out to be changed: a local, or
/// a global while the program loads.
#[inline]
pub fn takable(env: &Env, name: Sym) -> bool {
    let mut cur: &Scope = env;
    loop {
        if cur.vars().get(name).is_some() {
            return cur.parent.is_some() || loading();
        }
        match cur.parent.as_deref() {
            Some(p) => cur = p,
            None => return false,
        }
    }
}

/// Whether the nearest variable of this name holds a list or a text that may be grown
/// where it is: a local, or a global while the program loads.
pub fn growable(env: &Env, name: Sym) -> bool {
    let mut cur: &Scope = env;
    loop {
        if let Some(v) = cur.vars().get(name) {
            return matches!(v, Value::List(_) | Value::Str(_)) && (cur.parent.is_some() || loading());
        }
        match cur.parent.as_deref() {
            Some(p) => cur = p,
            None => return false,
        }
    }
}

/// `f` on the place of the nearest variable of this name, if there is one.
pub fn with_slot<R>(env: &Env, name: Sym, f: impl FnOnce(&mut Value) -> R) -> Option<R> {
    let mut cur: &Scope = env;
    loop {
        if let Some(slot) = cur.vars().get_mut(name) {
            return Some(f(slot));
        }
        cur = cur.parent.as_deref()?;
    }
}

/// Adds `t` to the end of `s` where it is, if nothing else holds `s`: false if something
/// does. `Arc<str>` has no room to spare, so this is what CPython does for `s += t`, which
/// is to ask the allocator for the block to be larger: it nearly always can, or it moves
/// it, in steps that grow with the block, so that a loop's appends are not quadratic
/// (FEEDBACK29.md, Q-354; branch feedback29-fixes). `the_text_block_is_laid_out_as_assumed`
/// holds `Arc`'s layout to what is assumed here.
pub fn append_text(s: &mut Arc<str>, t: &str) -> bool {
    use std::alloc::{realloc, Layout};
    const HEADER: usize = 2 * std::mem::size_of::<usize>();
    const ALIGN: usize = std::mem::align_of::<usize>();
    let round = |n: usize| (n + ALIGN - 1) & !(ALIGN - 1);
    if Arc::get_mut(s).is_none() {
        return false;
    }
    if t.is_empty() {
        return true;
    }
    let old = s.len();
    let new = old + t.len();
    let Ok(layout) = Layout::from_size_align(round(HEADER + old), ALIGN) else { return false };
    let Ok(_) = Layout::from_size_align(round(HEADER + new), ALIGN) else { return false };
    let held = std::mem::replace(s, Arc::from(""));
    // SAFETY: nothing else holds the block (asked above), which starts `HEADER` before the
    // text and has `layout` (see the test); `realloc` gives a block of the size `Arc` works
    // out for the longer text, with the counts as they were, and the new bytes are text
    // added to the end of text. If it fails, the old block is still there and is taken back.
    unsafe {
        let text = Arc::into_raw(held) as *const u8 as *mut u8;
        let block = text.sub(HEADER);
        let grown = realloc(block, layout, round(HEADER + new));
        if grown.is_null() {
            *s = Arc::from_raw(std::ptr::slice_from_raw_parts(text as *const u8, old) as *const str);
            return false;
        }
        let text = grown.add(HEADER);
        std::ptr::copy_nonoverlapping(t.as_ptr(), text.add(old), t.len());
        *s = Arc::from_raw(std::ptr::slice_from_raw_parts(text as *const u8, new) as *const str);
    }
    true
}

/// `push` (a value to add) or `set` (a place and a value) of `vals`, the first left free for
/// what is changed, on a list or map where it is: false, with nothing changed, for what the
/// builtin is to refuse or that is otherwise.
pub fn update(slot: &mut Value, vals: &mut [Value]) -> bool {
    match (slot, &mut *vals) {
        (Value::List(l), [_, x]) => Arc::make_mut(l).push(std::mem::replace(x, Value::Nil)),
        (Value::List(l), [_, Value::Int(i), x]) => {
            let n = l.len() as i64;
            let i = if *i < 0 { n + *i } else { *i };
            if i < 0 || i >= n {
                return false;
            }
            Arc::make_mut(l)[i as usize] = std::mem::replace(x, Value::Nil);
        }
        (Value::Map(m), [_, Value::Str(k), x]) => {
            Arc::make_mut(m).insert(Key::new(k), std::mem::replace(x, Value::Nil));
        }
        _ => return false,
    }
    true
}

/// `whole[key]` given `item` (a `push`) or changed at one place (`set`), where it is:
/// false, with nothing changed, if that is not how it goes. A map without the key takes
/// `start` for it. See `Interp::in_place_nested`.
pub fn update_inside(whole: &mut Value, key: &Value, start: Option<Value>, mut vals: Vec<Value>) -> bool {
    fn at(l: &[Value], i: i64) -> Option<usize> {
        let n = l.len() as i64;
        let i = if i < 0 { n + i } else { i };
        (i >= 0 && i < n).then_some(i as usize)
    }
    let last = vals.pop().unwrap_or(Value::Nil);
    let change = |x: &mut Value, last: Value, inner: Option<&Value>| -> bool {
        match (x, inner) {
            (Value::List(l), None) => Arc::make_mut(l).push(last),
            (Value::List(l), Some(Value::Int(i))) => {
                let Some(i) = at(l, *i) else { return false };
                Arc::make_mut(l)[i] = last;
            }
            // An error is no key: the long way gives it back (FEEDBACK55.md, Q-784; found in
            // review).
            (Value::Map(_), Some(Value::Err(..))) => return false,
            (Value::Map(m), Some(k)) => {
                Arc::make_mut(m).insert(k.key_text().into(), last);
            }
            _ => return false,
        }
        true
    };
    let inner = vals.first();
    match (whole, key) {
        (Value::List(l), Value::Int(i)) => {
            let Some(i) = at(l, *i) else { return false };
            change(&mut Arc::make_mut(l)[i], last, inner)
        }
        (Value::Map(_), Value::Err(..)) => false,
        (Value::Map(m), k) => {
            let text;
            let k: &str = match k {
                Value::Str(s) => s,
                k => {
                    text = k.key_text();
                    &text
                }
            };
            let m = Arc::make_mut(m);
            if let Some(x) = m.get_mut(k) {
                return change(x, last, inner);
            }
            let Some(mut x) = start else { return false };
            let done = change(&mut x, last, inner);
            if done {
                m.insert(k.into(), x);
            }
            done
        }
        _ => false,
    }
}

/// `x += n` where `x` is a local number and it fits: false for anything else, which
/// `binop` then works out, or refuses.
#[inline]
pub fn bump_local(env: &Env, name: Sym, n: &Value, sub: bool) -> bool {
    let mut cur: &Scope = env;
    while let Some(parent) = cur.parent.as_deref() {
        if let Some(slot) = cur.vars().get_mut(name) {
            match (&mut *slot, n) {
                (Value::Int(a), Value::Int(b)) => {
                    let Some(x) = (if sub { a.checked_sub(*b) } else { a.checked_add(*b) }) else { return false };
                    *a = x;
                }
                (Value::Float(a), Value::Float(b)) => *a = if sub { *a - b } else { *a + b },
                _ => return false,
            }
            return true;
        }
        cur = parent;
    }
    false
}

/// Sets a variable that is not a global, which nothing outside follows; the value back
/// if there is none. An assignment's usual case, and it is one walk up the scopes.
#[inline]
pub fn assign_local(env: &Env, name: Sym, v: Value) -> Result<(), Value> {
    let mut cur: &Scope = env;
    loop {
        let Some(parent) = cur.parent.as_deref() else { return Err(v) };
        if let Some(slot) = cur.vars().get_mut(name) {
            *slot = v;
            return Ok(());
        }
        cur = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `append_text` takes for granted of `Arc<str>`: the counts, then the text, in a
    /// block of the size it works out. A new Rust that lays it out otherwise fails here.
    #[test]
    fn the_text_block_is_laid_out_as_assumed() {
        const HEADER: usize = 2 * std::mem::size_of::<usize>();
        for n in [0usize, 1, 7, 8, 9, 15, 16, 17, 100] {
            let s: Arc<str> = Arc::from("x".repeat(n));
            let t = s.clone();
            // SAFETY: both counts are in the live block just before the text, or this test
            // has found that they are not, which is the point of it.
            let strong = unsafe { *(Arc::as_ptr(&s) as *const u8).sub(HEADER).cast::<usize>() };
            let weak = unsafe { *(Arc::as_ptr(&s) as *const u8).sub(HEADER).add(HEADER / 2).cast::<usize>() };
            assert_eq!((strong, weak), (2, 1), "{}", n);
            drop(t);
        }
    }

    #[test]
    fn text_is_added_where_it_is_only_if_nothing_else_holds_it() {
        let mut s: Arc<str> = Arc::from("ab");
        assert!(append_text(&mut s, "c"));
        assert_eq!(&*s, "abc");
        let t = s.clone();
        assert!(!append_text(&mut s, "d"));
        assert_eq!((&*s, &*t), ("abc", "abc"));
        drop(t);
        assert!(append_text(&mut s, ""));
        assert_eq!(&*s, "abc");
        // A weak hold is a hold.
        let w = Arc::downgrade(&s);
        assert!(!append_text(&mut s, "d"));
        drop(w);
        // Across every size of block, the way a loop goes, with letters of all lengths.
        let mut s: Arc<str> = Arc::from("");
        let mut want = String::new();
        for i in 0..200_000 {
            let piece = ["a", "é", "語", "😀"][i % 4];
            assert!(append_text(&mut s, piece));
            want.push_str(piece);
        }
        assert_eq!(&*s, want.as_str());
        assert_eq!(Arc::strong_count(&s), 1);
    }

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

#[cfg(test)]
mod js_number_tests {
    /// What Node's `String(x)` gives for each, with no exponent.
    #[test]
    fn numbers_are_written_as_javascript_writes_them() {
        let xs = [8.0, 2.5, 0.1 + 0.2, 1e21, 1.5e-7, 1e-6, 123456789012345680000.0, -0.000001234, 1e300, 5e-324, -42.0, 100.0, 0.5, 1.0 / 3.0, 9007199254740992.0, 1e16, 12345.6789];
        let js = format!(
            "8 2.5 0.30000000000000004 1{} 0.00000015 0.000001 123456789012345680000 -0.000001234 1{} 0.{}5 -42 100 0.5 0.3333333333333333 9007199254740992 10000000000000000 12345.6789",
            "0".repeat(21),
            "0".repeat(300),
            "0".repeat(323)
        );
        assert_eq!(xs.iter().map(|x| super::js_number(*x)).collect::<Vec<_>>().join(" "), js);
    }
}
