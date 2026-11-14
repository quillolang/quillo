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
    ///
    /// At most `JSON_NESTING` levels deep, as every reader of JSON takes (`json_value`), the
    /// database's and the browser's too (FEEDBACK55.md, Q-875's reviews; branch feedback55).
    pub fn parse_json(text: &str) -> serde_json::Result<Value> {
        use serde::Deserialize;
        let budget = (text.len() / 8).max(JSON_VALUES);
        let outer = JSON_LEFT.with(|l| l.replace(budget));
        let outer_depth = JSON_DEPTH.with(|d| d.replace(0));
        let mut de = serde_json::Deserializer::from_str(text);
        de.disable_recursion_limit();
        let r = Parsed::deserialize(&mut de).and_then(|p| de.end().map(|_| p.0));
        JSON_LEFT.with(|l| l.set(outer));
        let deep = JSON_DEPTH.with(|d| d.replace(outer_depth)) > JSON_NESTING;
        r.map_err(|e| if deep { <serde_json::Error as serde::de::Error>::custom(json_too_deep()) } else { e })
    }
}

/// How deep JSON may nest, for every reader: a program's (`Value::parse_json`), the database's,
/// sessions', channels', server fns' arguments, services' and idempotent answers (`json_value`),
/// and the browser's. 1,000 let a visitor's body nest past what the server's recursion
/// (`json_into`, `repr`, `equal`, Drop) takes on a parallel function's stack, and the process
/// stopped; serde's own limit took 127 (FEEDBACK55.md, Q-875's second review; branch feedback55).
pub const JSON_NESTING: usize = 128;

/// What reading JSON nested deeper than `JSON_NESTING` says, on both sides.
pub fn json_too_deep() -> String {
    format!("it nests more than {} deep", JSON_NESTING)
}

/// How many levels JSON text `b` nests (its lists and maps, not what is in its strings), counted
/// up to one past `JSON_NESTING`.
pub fn json_depth(b: &[u8]) -> usize {
    let (mut d, mut most, mut i) = (0usize, 0usize, 0);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'[' | b'{' => {
                d += 1;
                most = most.max(d);
                if most > JSON_NESTING {
                    return most;
                }
            }
            b']' | b'}' => d = d.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    most
}

/// How many levels a JSON value nests (its lists and maps).
pub fn json_value_depth(j: &serde_json::Value) -> usize {
    match j {
        serde_json::Value::Array(a) => 1 + a.iter().map(json_value_depth).max().unwrap_or(0),
        serde_json::Value::Object(o) => 1 + o.values().map(json_value_depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// JSON text read as serde's values, at most `JSON_NESTING` deep, as `Value::parse_json` reads a
/// program's: serde's own limit took 127.
pub fn json_value(b: &[u8]) -> Result<serde_json::Value, String> {
    use serde::Deserialize;
    if json_depth(b) > JSON_NESTING {
        return Err(json_too_deep());
    }
    let mut de = serde_json::Deserializer::from_slice(b);
    de.disable_recursion_limit();
    let j = serde_json::Value::deserialize(&mut de).map_err(|e| e.to_string())?;
    de.end().map_err(|e| e.to_string())?;
    Ok(j)
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
    /// How deep the JSON being read is now; past `JSON_NESTING` once it was too deep.
    static JSON_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A list or a map read from JSON, inside what is being read: an error past `JSON_NESTING`, and
/// `f` run with room on the stack for it.
fn nested<R, E: serde::de::Error>(f: impl FnOnce() -> Result<R, E>) -> Result<R, E> {
    let depth = JSON_DEPTH.with(|d| {
        let n = d.get() + 1;
        d.set(n);
        n
    });
    if depth > JSON_NESTING {
        return Err(E::custom(json_too_deep()));
    }
    let r = crate::stack::maybe_grow(64 << 10, 1 << 20, f);
    if r.is_ok() {
        JSON_DEPTH.with(|d| d.set(depth - 1));
    }
    r
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
        nested(|| {
            let mut out = Vec::new();
            while let Some(Parsed(v)) = seq.next_element()? {
                one_more()?;
                out.push(v);
            }
            Ok(Parsed(Value::list(out)))
        })
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Parsed, A::Error> {
        nested(|| {
            let mut out = Map::new();
            while let Some((ParsedKey(k), Parsed(v))) = map.next_entry()? {
                one_more()?;
                out.insert(k, v);
            }
            Ok(Parsed(Value::map(out)))
        })
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
    // Two that hold no others, as `s[i] == "a"` and `n == 0` are, with no list of what is
    // left to compare: making one each time was a fifth of such a loop (FEEDBACK55.md, string
    // speed; branch feedback55).
    if !matches!((a, b), (List(_), List(_)) | (Map(_), Map(_)) | (Err(..), Err(..))) {
        return scalar_equal(a, b);
    }
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

#[inline]
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
/// equal values alike and values that are not equal share only rarely, whatever a visitor sends.
/// Keys are made all the way down, from a stack, not by recursion (found in review: below 32
/// levels all lists had one key).
///
/// - Without a decimal in the list: a float by its bits (-0.0 as 0.0), an int by the float that
///   is exactly it (an int and a float are equal only exactly) or else by itself, text by itself,
///   lists item by item, maps pair by pair in any order.
/// - `==` is exact everywhere but between a float and a decimal: the float is its shortest text
///   as a decimal, rounded to 28 places, so `1e-30 == 0m` and `2^60 == 1152921504606847000m`,
///   though `1e-30 != 0`, `1e-30 != 2e-30` and `2^60 != 1152921504606847000`. With a decimal in
///   the list each number is keyed by the decimal it is equal to (past 2^53, by its nearest float,
///   which the ints, floats and decimals equal to it share). A float whose decimal does not give
///   it back (one below 1e-10 with too many digits for 28 places) is keyed by its bits instead,
///   or many unequal floats would share one key (found in review: `[k * 1e-40, 1m]` was
///   quadratic again); where it meets a small decimal it may be equal to, the item is also kept
///   under its key with each such float made its decimal, and the kinds there (float or decimal):
///   an item is looked for there among those whose kinds there differ from its own.
///
/// What holds NaN (or an answer) is equal to nothing: kept without a look. Keyed for the process,
/// text by SipHash, so a visitor's values cannot be made to share keys.
pub fn unique_of(items: &[Value]) -> Vec<Value> {
    use std::collections::HashMap;
    use std::hash::BuildHasher;
    type Fast<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<Hashed>>;
    static KEY: std::sync::OnceLock<(std::collections::hash_map::RandomState, u64)> = std::sync::OnceLock::new();
    let (key, seed) = KEY.get_or_init(|| {
        let k = std::collections::hash_map::RandomState::new();
        let seed = k.hash_one(0u8);
        (k, seed)
    });
    const END: u32 = u32::MAX;
    const TWO53: f64 = 9007199254740992.0;
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
    enum Step<'a> {
        Val(&'a Value),
        Tok(u64),
    }
    // An item's keys: its own (`main`), the one with each float made its decimal (`canon`), and
    // the kinds of its small numbers that may meet (`pattern`, `small` of them).
    #[derive(Default)]
    struct Keys {
        main: u64,
        canon: u64,
        pattern: u64,
        small: u32,
        dec: bool,
        never: bool,
    }
    struct Walk<'a, 'v> {
        key: &'a std::collections::hash_map::RandomState,
        seed: u64,
        decs: bool,
        // The stack the walk goes by, kept from item to item.
        todo: Vec<Step<'v>>,
    }
    impl<'v> Walk<'_, 'v> {
        // A decimal's token: by its value, or past 2^53 by its nearest float.
        fn dec_tok(d: crate::decimal::Dec, key: &std::collections::hash_map::RandomState) -> u64 {
            let n = d.normalized();
            let (m, sc) = (n.mantissa(), n.scale());
            let big = 10i128.checked_pow(sc).and_then(|p| p.checked_mul(1 << 53)).is_some_and(|edge| m.unsigned_abs() >= edge as u128);
            if big {
                mix(31, bits(d.to_f64()))
            } else {
                mix(37, key.hash_one((m, sc)))
            }
        }
        // `|d| < 1e-10`: a decimal a float its text does not give back may be equal to.
        fn small_dec(d: crate::decimal::Dec) -> bool {
            let n = d.normalized();
            let sc = n.scale();
            n.mantissa() == 0 || (sc > 10 && n.mantissa().unsigned_abs() < 10u128.pow(sc - 10))
        }
        fn keys(&mut self, v: &'v Value) -> Keys {
            let mut k = Keys { main: self.seed, canon: self.seed, pattern: self.seed, ..Keys::default() };
            let mut todo = std::mem::take(&mut self.todo);
            todo.push(Step::Val(v));
            while let Some(step) = todo.pop() {
                let x = match step {
                    Step::Tok(t) => {
                        k.main = mix(k.main, t);
                        k.canon = mix(k.canon, t);
                        continue;
                    }
                    Step::Val(x) => x,
                };
                let (m, c) = match x {
                    Value::Nil => (1, 1),
                    Value::Bool(b) => (2 + *b as u64, 2 + *b as u64),
                    Value::Float(f) if f.is_nan() => {
                        k.never = true;
                        (0, 0)
                    }
                    Value::Float(f) if !self.decs => (mix(3, bits(*f)), mix(3, bits(*f))),
                    Value::Float(f) if f.abs() >= TWO53 || !f.is_finite() => (mix(31, bits(*f)), mix(31, bits(*f))),
                    Value::Float(f) => match crate::decimal::Dec::from_f64(*f) {
                        Some(d) if f.abs() >= 1e-10 || d.to_f64() == *f => {
                            let t = Self::dec_tok(d, self.key);
                            (t, t)
                        }
                        Some(d) => {
                            k.small += 1;
                            k.pattern = mix(k.pattern, 41);
                            (mix(43, bits(*f)), Self::dec_tok(d, self.key))
                        }
                        None => (mix(3, bits(*f)), mix(3, bits(*f))),
                    },
                    Value::Int(n) if !self.decs => {
                        let t = if (*n as f64) as i128 == *n as i128 { mix(3, bits(*n as f64)) } else { mix(7, *n as u64) };
                        (t, t)
                    }
                    Value::Int(n) => {
                        let t = if (*n as f64).abs() >= TWO53 { mix(31, bits(*n as f64)) } else { Self::dec_tok(crate::decimal::Dec::from_i64(*n), self.key) };
                        (t, t)
                    }
                    Value::Dec(d) => {
                        k.dec = true;
                        if Self::small_dec(*d) {
                            k.small += 1;
                            k.pattern = mix(k.pattern, 47);
                        }
                        let t = Self::dec_tok(*d, self.key);
                        (t, t)
                    }
                    Value::Str(s) | Value::Html(s) => {
                        let t = self.key.hash_one(&**s);
                        (t, t)
                    }
                    Value::List(l) => {
                        todo.push(Step::Tok(53));
                        for item in l.iter().rev() {
                            todo.push(Step::Val(item));
                        }
                        (mix(11, l.len() as u64), mix(11, l.len() as u64))
                    }
                    Value::Map(m) => {
                        // In the order of the keys' hashes, so that any order of the pairs is one.
                        let mut pairs: Vec<(u64, &str, &Value)> = m.iter().map(|(name, x)| (self.key.hash_one(&**name), &**name, x)).collect();
                        pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
                        todo.push(Step::Tok(59));
                        for (kh, _, x) in pairs.into_iter().rev() {
                            todo.push(Step::Val(x));
                            todo.push(Step::Tok(kh));
                        }
                        (mix(13, m.len() as u64), mix(13, m.len() as u64))
                    }
                    Value::Err(e, ..) => {
                        todo.push(Step::Tok(61));
                        todo.push(Step::Val(e));
                        (17, 17)
                    }
                    Value::Builtin(n, _) => {
                        let t = self.key.hash_one(n);
                        (t, t)
                    }
                    Value::Func(f) => (Arc::as_ptr(f) as u64, Arc::as_ptr(f) as u64),
                    Value::Response(_) => {
                        k.never = true;
                        (19, 19)
                    }
                };
                k.main = mix(k.main, m);
                k.canon = mix(k.canon, c);
            }
            self.todo = todo;
            k
        }
    }
    let decs = items.iter().any(holds_dec);
    let mut walk = Walk { key, seed: *seed, decs, todo: Vec::new() };
    let mut out: Vec<Value> = Vec::new();
    // What is kept, by its own key: where each key's chain starts, and the next.
    let mut first: Fast<u64, u32> = Default::default();
    let mut next: Vec<u32> = Vec::new();
    // What is kept with small numbers that may meet, by its key with them made decimals: for
    // each kind of them, those kept.
    let mut groups: Fast<u64, Vec<(u64, Vec<u32>)>> = Default::default();
    for x in items {
        let k = walk.keys(x);
        if k.never {
            out.push(x.clone());
            next.push(END);
            continue;
        }
        let mut seen = false;
        let mut i = first.get(&k.main).copied().unwrap_or(END);
        while i != END && !seen {
            seen = equal(x, &out[i as usize]);
            i = next[i as usize];
        }
        if !seen && k.small > 0 {
            if let Some(kinds) = groups.get(&k.canon) {
                seen = kinds.iter().filter(|(p, _)| *p != k.pattern).any(|(_, kept)| kept.iter().any(|&i| equal(x, &out[i as usize])));
            }
        }
        if seen {
            continue;
        }
        let at = out.len() as u32;
        out.push(x.clone());
        next.push(END);
        // At the chain's start: the newest first, as good as any.
        if let Some(head) = first.insert(k.main, at) {
            next[at as usize] = head;
        }
        if k.small > 0 {
            let kinds = groups.entry(k.canon).or_default();
            match kinds.iter_mut().find(|(p, _)| *p == k.pattern) {
                Some((_, kept)) => kept.push(at),
                None => kinds.push((k.pattern, vec![at])),
            }
        }
    }
    out
}

/// Whether `v` holds a decimal anywhere, looked at from a stack.
fn holds_dec(v: &Value) -> bool {
    let mut todo = vec![v];
    while let Some(x) = todo.pop() {
        match x {
            Value::Dec(_) => return true,
            Value::List(l) => todo.extend(l.iter()),
            Value::Map(m) => todo.extend(m.values()),
            Value::Err(e, ..) => todo.push(e),
            _ => {}
        }
    }
    false
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

/// How many items make a list or a map large enough to be freed on another thread (`drop_value`).
const DEFER_AT: usize = 256;

/// How many items `items` hold, theirs and their items' to `depth` 2, counted no further than
/// `DEFER_AT` (`drop_value`).
fn weight<'a>(items: impl Iterator<Item = &'a Value>, depth: u8) -> usize {
    let mut n = 0;
    for v in items {
        n += 1;
        if depth < 2 {
            n += match v {
                Value::List(l) => weight(l.iter(), depth + 1),
                Value::Map(m) => weight(m.values(), depth + 1),
                _ => 0,
            };
        }
        if n >= DEFER_AT {
            return n;
        }
    }
    n
}

/// The values kept since the last call, to be dropped elsewhere.
pub fn deferred_drops() -> Vec<Value> {
    LATER.with(|l| l.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
}

fn drop_value(v: Value) {
    // Large: freed elsewhere it takes the loop no time. A small one is freed here: sent to another
    // thread, its blocks were freed there as another thread's, mimalloc's slow way, and a page
    // of 13 rows from SQLite (made on this thread) paid that each request (2026-11-09, the server
    // benchmark; it had been 8 items for a list and 16 for a map). Counted with what its items
    // hold, two levels down, so that a map of 30 days of 1,000 rows each is large still (found
    // in review).
    let large = match &v {
        Value::List(l) => Arc::strong_count(l) == 1 && weight(l.iter(), 0) >= DEFER_AT,
        Value::Map(m) => Arc::strong_count(m) == 1 && weight(m.values(), 0) >= DEFER_AT,
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
    /// On a thread's list of scopes to look at again (`watch`), which holds it once more
    /// (FEEDBACK65.md, Q-941; branch feedback65). In the room `locked` leaves, so no larger.
    watched: std::sync::atomic::AtomicBool,
    vars: std::cell::UnsafeCell<Vars>,
}

// SAFETY: access goes through `lock`, which hands out one guard at a time, or through
// `inner`, under the interpreter lock.
unsafe impl Sync for VarsCell {}

struct VarsGuard<'a>(&'a VarsCell, bool);

impl VarsCell {
    fn new(vars: Vars) -> Self {
        VarsCell { locked: std::sync::atomic::AtomicBool::new(false), watched: std::sync::atomic::AtomicBool::new(false), vars: std::cell::UnsafeCell::new(vars) }
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

    /// Calls `f` with each value while it says go on; whether it always did.
    fn each_value(&self, mut f: impl FnMut(&Value) -> bool) -> bool {
        match self {
            Vars::Few(slots, more, _) => slots.iter().flatten().chain(more.iter()).all(|(_, v)| f(v)),
            Vars::Many(m) => m.values().all(f),
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
    /// Held only by functions kept in its own variables (`let f = (x) => ...`), which
    /// nothing else holds, it is too: they held each other for ever (FEEDBACK65.md, Q-941;
    /// branch feedback65).
    pub fn done(mut e: Env) {
        let Some(s) = Arc::get_mut(&mut e) else {
            match quick(&e, true) {
                Quick::Plain => {}
                Quick::Outside => watch(e),
                Quick::Look => Scope::done_held(e),
            }
            return;
        };
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
        Scope::reuse_step(e, &mut false)
    }

    /// `reuse` for a loop that remembers, in `outside`, a step whose lambda something outside
    /// held (each kept in a list, say): its later steps are not looked at, at no cost, and are
    /// left as they were (FEEDBACK65.md, Q-941 review 2; branch feedback65).
    #[inline(always)]
    pub fn reuse_step(e: &mut Env, outside: &mut bool) -> bool {
        // Nothing takes a weak hold of a scope, so one owner is enough to know it.
        if Arc::strong_count(e) != 1 {
            if *outside {
                return false;
            }
            return match quick(e, false) {
                Quick::Plain => false,
                Quick::Outside => {
                    *outside = true;
                    false
                }
                Quick::Look => Scope::reuse_held(e, outside),
            };
        }
        e.vars().clear();
        true
    }

    /// `done` of a scope something else holds: if that is only functions kept in it, which
    /// nothing else holds, they are let go of and it is kept as `done` keeps one; if something
    /// outside holds them too (`return f`), it is looked at again later (FEEDBACK65.md, Q-941;
    /// branch feedback65).
    #[cold]
    #[inline(never)]
    fn done_held(mut e: Env) {
        // A record or list of its own holding a lambda made in it that is held besides (`return
        // acts.go`): listed at once, as `quick` lists `return f`, and the next look tells
        // (FEEDBACK65.md, Q-941 review of S2c, 1; branch feedback65).
        if boxed_outside(&e) {
            return watch(e);
        }
        match own(&e, 1) {
            Own::None => {}
            Own::Closed => {
                empty_own(&e);
                Scope::done(e);
            }
            Own::Outside => watch(e),
            Own::Look => match ring(&e, 1) {
                Ring::Broken if Arc::get_mut(&mut e).is_some() => Scope::done(e),
                Ring::Held(w) => watch_as(e, w),
                _ => {}
            },
        }
    }

    /// `reuse` of a scope something else holds: as `done_held`, and then empty, it is used
    /// again. One held from outside is left as it was, at no cost and never listed: a loop that
    /// keeps each step's lambda in a list keeps them all (FEEDBACK65.md, Q-941 review 2; branch
    /// feedback65).
    #[cold]
    #[inline(never)]
    fn reuse_held(e: &mut Env, outside: &mut bool) -> bool {
        // Held from outside, whatever told it: the loop's later steps are not looked at either
        // (FEEDBACK65.md, Q-941 review of S2c, 2; branch feedback65).
        match own(e, 1) {
            Own::Closed => {
                empty_own(e);
                Arc::strong_count(e) == 1
            }
            Own::None => false,
            Own::Outside => {
                *outside = true;
                false
            }
            Own::Look => match ring(e, 1) {
                Ring::Broken => Arc::strong_count(e) == 1,
                Ring::Held(_) => {
                    *outside = true;
                    false
                }
                Ring::None => false,
            },
        }
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

// ---- rings: a scope and the functions kept in it -------------------------------------------
//
// `let f = (x) => ...` keeps a function in a scope's variables, and the function holds the scope
// (its `env`): each held the other, so neither was freed when the scope ended, and what the scope
// held leaked on every call, a page's rows or an upload's bytes (FEEDBACK65.md, Q-941; branch
// feedback65). A function shares the variables it reads (it may assign them), so they stay where
// they are. When a scope ends still held, the holds are counted instead: if every hold on it, and
// on the functions and scopes between, comes from one of them, nothing else can reach any of them,
// and their variables are emptied, which no program can see.

/// The most scopes, the most functions, and the most lists and maps a ring is looked for among:
/// past them it is left as it was.
const RING: usize = 8;
/// The deepest inside a scope a function's own may be for it to be one of the scope's ring.
const RING_DEEP: usize = 16;
/// The most items of lists and maps looked through for a ring, all of them together; a list or
/// map longer than what is left is passed over whole, so a scope's 20,000 rows cost a look at
/// their length (FEEDBACK65.md, Q-941; branch feedback65). Under `DEFER_AT`, so that none of the
/// ring's lists is given to another thread to free.
const RING_ITEMS: usize = 128;

enum Ring {
    /// No function in its variables holds it.
    None,
    /// Some do, but something besides them holds it or one of them: what, to tell later.
    Held(Witness),
    /// Nothing besides them did: their variables are emptied.
    Broken,
}

/// Whether function `c` holds scope `s`: it was made in `s` or in a scope inside it.
#[inline]
fn holds(c: &Closure, s: *const Scope) -> bool {
    let mut y: &Scope = &c.env;
    for _ in 0..RING_DEEP {
        if std::ptr::eq(y, s) {
            return true;
        }
        match y.parent.as_deref() {
            Some(p) => y = p,
            None => return false,
        }
    }
    false
}

/// What a first glance at a scope still held at its end tells, at the cost of a few instructions
/// (FEEDBACK65.md, Q-941 review 2 and 3; branch feedback65).
#[derive(PartialEq)]
enum Quick {
    /// No function, and no list or map only its variable holds: no ring can go through them,
    /// the commonest (a call's that gives back a lambda).
    Plain,
    /// A function made in it, held besides: something outside holds its ring (`return f`, a
    /// list outside), or the function is kept twice, which the later looks tell.
    Outside,
    /// Something to look at (`Scope::done_held`).
    Look,
}

/// A glance at `e`'s variables, a few of them, which fill their places in order. Where a scope
/// `ending` holds a list or map besides such a lambda, the lambda's other hold may be the list's:
/// the scope is listed all the same, and the next look tells (FEEDBACK65.md, Q-941 review of S2c,
/// 1; branch feedback65).
#[inline(always)]
fn quick(e: &Env, ending: bool) -> Quick {
    // A scope `done` ends has a parent: no lock to take (`Scope::vars`).
    if e.parent.is_none() {
        return Quick::Look;
    }
    // A function made in it held besides; anything else to look at.
    let (mut out, mut look) = (false, false);
    match &*e.vars.inner() {
        Vars::Few(slots, more, _) => {
            for x in slots {
                match x {
                    None => break,
                    // Held besides, and by no other of these variables (`let g = f`).
                    Some((_, Value::Func(c)))
                        if std::ptr::eq(Arc::as_ptr(&c.env), Arc::as_ptr(e))
                            && Arc::strong_count(c) > 1
                            && slots.iter().flatten().filter(|(_, v)| matches!(v, Value::Func(d) if Arc::ptr_eq(c, d))).nth(1).is_none() =>
                    {
                        out = true
                    }
                    Some((_, Value::Func(_))) => look = true,
                    Some((_, v @ (Value::List(_) | Value::Map(_)))) => look |= !shared(v),
                    _ => {}
                }
            }
            if !more.is_empty() {
                return Quick::Look;
            }
        }
        Vars::Many(_) => return Quick::Look,
    }
    // Nothing else there could hold it: something outside does.
    match (out, look) {
        (true, false) => Quick::Outside,
        (true, true) if ending => Quick::Outside,
        (false, false) => Quick::Plain,
        _ => Quick::Look,
    }
}

/// Whether a list or map only one of `e`'s variables holds has, among its own items, a function
/// made in `e` that something else holds too. A few variables and items are looked at.
fn boxed_outside(e: &Env) -> bool {
    let s = Arc::as_ptr(e);
    let made = |x: &Value| matches!(x, Value::Func(c) if std::ptr::eq(Arc::as_ptr(&c.env), s) && Arc::strong_count(c) > 1);
    !e.vars().each_value(|v| match v {
        Value::Map(m) if Arc::strong_count(m) == 1 && m.len() <= RING => !m.values().any(made),
        Value::List(l) if Arc::strong_count(l) == 1 && l.len() <= RING => !l.iter().any(made),
        _ => true,
    })
}

/// Whether `v` is a list or map something else holds too.
#[inline]
fn shared(v: &Value) -> bool {
    match v {
        Value::List(l) => Arc::strong_count(l) != 1,
        Value::Map(m) => Arc::strong_count(m) != 1,
        _ => false,
    }
}

/// Whether a function in `e`'s own variables holds it, or one in a list or map only they hold
/// (`let acts = { save: () => ... }`), within `RING_ITEMS`: most scopes still held at their end
/// keep none. Apart, for that to cost little.
#[inline(never)]
fn keeps_own(e: &Env) -> bool {
    let s = Arc::as_ptr(e);
    let own = |v: &Value| matches!(v, Value::Func(c) if holds(c, s));
    let direct = match &*e.vars() {
        Vars::Few(slots, more, _) => slots.iter().flatten().any(|(_, v)| own(v)) || more.iter().any(|(_, v)| own(v)),
        Vars::Many(m) => m.values().any(own),
    };
    let mut left = RING_ITEMS;
    direct || !e.vars().each_value(|v| !boxes_own(v, s, &mut left))
}

/// What a first look at a scope still held at its end finds, in its own variables alone (`own`).
enum Own {
    /// No function in them holds it.
    None,
    /// Functions made in it, kept in them, and nothing else there that could hold one (no list
    /// or map, no function made inside it): every hold on them and on it is theirs and the
    /// caller's. Nothing outside can reach them.
    Closed,
    /// The same, but something besides holds it or one of them: a ring held from outside.
    Outside,
    /// More than that, for `ring` to count.
    Look,
}

/// The commonest rings told at once, without `ring`'s walk: `let f = (x) => ...` in a body,
/// returned or not (FEEDBACK65.md, Q-941 review 3; branch feedback65). Exactly what `ring`
/// would find of such a scope, which holds no list, map or function made inside it.
fn own(e: &Env, handles: usize) -> Own {
    use std::sync::atomic::Ordering::Relaxed;
    let s = Arc::as_ptr(e);
    let mut fs: [(*const Arc<Closure>, usize); RING] = [(std::ptr::null(), 0); RING];
    let (mut n, mut deep) = (0, false);
    e.vars().each_value(|v| {
        match v {
            Value::Func(c) if std::ptr::eq(Arc::as_ptr(&c.env), s) => match fs[..n].iter_mut().find(|f| Arc::ptr_eq(unsafe { &*f.0 }, c)) {
                Some(f) => f.1 += 1,
                None if n == RING => deep = true,
                None => {
                    fs[n] = (c as *const Arc<Closure>, 1);
                    n += 1;
                }
            },
            Value::Func(c) => deep |= holds(c, s),
            // One something else holds too cannot make its lambdas' other holds inside.
            Value::List(_) | Value::Map(_) => deep |= !shared(v),
            _ => {}
        }
        // On to the end, to know whether one made in it is there (no `keeps_own` then).
        n < RING
    });
    if deep {
        return if n > 0 || keeps_own(e) { Own::Look } else { Own::None };
    }
    if n == 0 {
        return Own::None;
    }
    // SAFETY: the pointers are to handles in `e`'s variables, which nothing changes meanwhile.
    let theirs = fs[..n].iter().all(|&(c, k)| {
        let c = unsafe { &*c };
        Arc::strong_count(c) == k && Arc::weak_count(c) == 0
    });
    let more = usize::from(e.vars.watched.load(Relaxed)) + handles;
    if theirs && Arc::strong_count(e) == n + more && Arc::weak_count(e) == 0 {
        Own::Closed
    } else {
        Own::Outside
    }
}

/// Empties a scope `own` found `Closed`.
fn empty_own(e: &Env) {
    // What other threads did before letting go of their holds, seen before anything is dropped.
    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
    e.vars().clear();
}

/// Whether `v`, a list or map held by its holder alone, holds a function that holds `s`, itself
/// or in such a list or map inside it, among the `left` items still to look through.
fn boxes_own(v: &Value, s: *const Scope, left: &mut usize) -> bool {
    // Plain values passed over at a glance (FEEDBACK65.md, Q-941 review of S2c, 1; branch
    // feedback65).
    let look = |x: &Value, left: &mut usize| match x {
        Value::Func(c) => holds(c, s),
        Value::List(_) | Value::Map(_) => boxes_own(x, s, left),
        _ => false,
    };
    match v {
        Value::List(l) if Arc::strong_count(l) == 1 && l.len() <= *left => {
            *left -= l.len();
            l.iter().any(|x| look(x, left))
        }
        Value::Map(m) if Arc::strong_count(m) == 1 && m.len() <= *left => {
            *left -= m.len();
            m.values().any(|x| look(x, left))
        }
        _ => false,
    }
}

/// The address and the strong count of list or map `v`.
fn boxed(v: &Value) -> (*const u8, usize) {
    match v {
        Value::List(l) => (Arc::as_ptr(l) as *const u8, Arc::strong_count(l)),
        Value::Map(m) => (Arc::as_ptr(m) as *const u8, Arc::strong_count(m)),
        _ => (std::ptr::null(), 0),
    }
}

/// A look for a ring through scope `s`: its scopes (`s` first), each with the holds on it from
/// inside; its functions, each with the holds on it; and, when the look goes into them, the lists
/// and maps that hold some of the functions, each with the holds on it. Pointers to the handles
/// and values the ring's own scopes, functions and lists keep, which nothing changes while it
/// looks (the interpreter lock).
struct Walk {
    s: *const Scope,
    scopes: [(*const Env, usize); RING],
    ns: usize,
    funcs: [(*const Arc<Closure>, usize); RING],
    nf: usize,
    boxes: [(*const Value, usize); RING],
    nb: usize,
    /// Items of lists and maps still to look through.
    left: usize,
    /// False once the ring is larger than the arrays.
    room: bool,
    /// Into the lists and maps in lists and maps too (`ring`'s third walk), and whether one was
    /// passed over for want of it.
    nested: bool,
    passed: bool,
}

impl Walk {
    fn new(e: &Env) -> Walk {
        let mut w = Walk {
            s: Arc::as_ptr(e),
            scopes: [(std::ptr::null(), 0); RING],
            ns: 1,
            funcs: [(std::ptr::null(), 0); RING],
            nf: 0,
            boxes: [(std::ptr::null(), 0); RING],
            nb: 0,
            left: RING_ITEMS,
            room: true,
            nested: false,
            passed: false,
        };
        w.scopes[0].0 = e as *const Env;
        w
    }

    /// The variables of each of the ring's scopes, those found on the way too; into lists and
    /// maps if `deep`.
    fn scan(&mut self, deep: bool) {
        let mut i = 0;
        while i < self.ns && self.room {
            // SAFETY: as above, for this and each pointer read below.
            let x: &Env = unsafe { &*self.scopes[i].0 };
            x.vars().each_value(|v| {
                self.visit(v, deep);
                self.room
            });
            i += 1;
        }
    }

    /// Whether `v` is or holds one of the ring's functions; each hold found counted.
    fn visit(&mut self, v: &Value, deep: bool) -> bool {
        match v {
            Value::Func(c) => self.func(c),
            Value::List(_) | Value::Map(_) if deep => self.boxing(v),
            _ => false,
        }
    }

    fn func(&mut self, c: &Arc<Closure>) -> bool {
        if let Some(f) = self.funcs[..self.nf].iter_mut().find(|f| Arc::ptr_eq(unsafe { &*f.0 }, c)) {
            f.1 += 1;
            return true;
        }
        if !holds(c, self.s) {
            return false;
        }
        if self.nf == RING {
            self.room = false;
            return false;
        }
        self.funcs[self.nf] = (c as *const Arc<Closure>, 1);
        self.nf += 1;
        // The scopes from the function's own out to `s`, those not in the ring yet.
        let mut y: &Env = &c.env;
        while !std::ptr::eq(Arc::as_ptr(y), self.s) && !self.scopes[..self.ns].iter().any(|z| Arc::ptr_eq(unsafe { &*z.0 }, y)) {
            if self.ns == RING {
                self.room = false;
                return false;
            }
            self.scopes[self.ns] = (y as *const Env, 0);
            self.ns += 1;
            y = y.parent.as_ref().expect("a scope inside `s`");
        }
        true
    }

    /// List or map `v`: one of the ring's if it holds one of its functions, itself or in a list
    /// or map inside it (FEEDBACK65.md, Q-941; branch feedback65).
    fn boxing(&mut self, v: &Value) -> bool {
        let (at, _) = boxed(v);
        if let Some(b) = self.boxes[..self.nb].iter_mut().find(|b| boxed(unsafe { &*b.0 }).0 == at) {
            b.1 += 1;
            return true;
        }
        let len = match v {
            Value::List(l) => l.len(),
            Value::Map(m) => m.len(),
            _ => return false,
        };
        if len > self.left {
            return false;
        }
        if self.nb == RING {
            self.room = false;
            return false;
        }
        self.left -= len;
        let k = self.nb;
        self.boxes[k] = (v as *const Value, 1);
        self.nb += 1;
        let mut any = false;
        // Plain values passed over at a glance (FEEDBACK65.md, Q-941 review of S2c, 1; branch
        // feedback65).
        let nested = self.nested;
        let mut passed = false;
        let mut each = |x: &Value| match x {
            Value::Func(_) => any |= self.visit(x, true),
            Value::List(_) | Value::Map(_) if nested => any |= self.visit(x, true),
            Value::List(_) | Value::Map(_) => passed = true,
            _ => {}
        };
        match v {
            Value::List(l) => l.iter().for_each(&mut each),
            Value::Map(m) => m.values().for_each(&mut each),
            _ => {}
        }
        self.passed |= passed;
        // None of the ring's in it: not one of it (nor any inside it, each let go of the same way).
        if !any {
            self.nb = k;
        }
        any && self.room
    }

    /// Whether every hold on the ring's functions, lists and scopes is from inside it, besides
    /// `handles` of the caller's on `s` and the watch list's on each marked scope.
    fn closed(&mut self, handles: usize) -> Result<(), Witness> {
        use std::sync::atomic::Ordering::Relaxed;
        fn held(scopes: &mut [(*const Env, usize)], y: &Env) {
            if let Some(z) = scopes.iter_mut().find(|z| Arc::ptr_eq(unsafe { &*z.0 }, y)) {
                z.1 += 1;
            }
        }
        let ns = self.ns;
        // Each function's on its scope, each scope's but `s`'s on the one around it.
        for k in 0..self.nf {
            let c: &Arc<Closure> = unsafe { &*self.funcs[k].0 };
            held(&mut self.scopes[..ns], &c.env);
        }
        for k in 1..ns {
            let y: &Env = unsafe { &*self.scopes[k].0 };
            held(&mut self.scopes[..ns], y.parent.as_ref().expect("a scope inside `s`"));
        }
        // The first held besides is the witness: while its count stays, so does the verdict.
        for &(c, n) in &self.funcs[..self.nf] {
            let c = unsafe { &*c };
            if Arc::strong_count(c) != n || Arc::weak_count(c) != 0 {
                return Err(Witness::held(Value::Func(c.clone())));
            }
        }
        for &(v, n) in &self.boxes[..self.nb] {
            let v = unsafe { &*v };
            let weak = match v {
                Value::List(l) => Arc::weak_count(l),
                Value::Map(m) => Arc::weak_count(m),
                _ => 1,
            };
            if boxed(v).1 != n || weak != 0 {
                return Err(Witness::held(v.clone()));
            }
        }
        for (k, &(y, n)) in self.scopes[..ns].iter().enumerate() {
            let y = unsafe { &*y };
            let more = usize::from(y.vars.watched.load(Relaxed)) + if k == 0 { handles } else { 0 };
            if Arc::strong_count(y) != n + more || Arc::weak_count(y) != 0 {
                // The caller's hold on `s` goes to the list with it, so the count stays.
                return Err(if k == 0 {
                    Witness::Scope(Arc::strong_count(y))
                } else {
                    let y = y.clone();
                    let n = Arc::strong_count(&y);
                    Witness::Inner(y, n)
                });
            }
        }
        Ok(())
    }

    /// Empties the ring's scopes, each where it is, those inside `e` held meanwhile: emptying one
    /// drops functions and lists, and with them their holds on the others. (`e` the caller holds.)
    fn empty(&self, e: &Env) {
        // What other threads did before letting go of their holds, seen before anything is dropped.
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        if self.ns == 1 {
            e.vars().clear();
            return;
        }
        let mut inside: [Option<Env>; RING] = [const { None }; RING];
        for k in 1..self.ns {
            inside[k] = Some(unsafe { &*self.scopes[k].0 }.clone());
        }
        e.vars().clear();
        for y in inside.iter().flatten() {
            y.vars().clear();
        }
    }
}

/// A ring through `e`, which `handles` of the caller's hold besides (the watch list's hold is
/// counted by the mark), broken if nothing else holds any of it. Run under the interpreter lock,
/// as all that runs Quillo code: off it values are only dropped, so a count seen equal to the
/// holds from inside stays so. Lists and maps are looked into only when the plain count fails.
#[cold]
#[inline(never)]
fn ring(e: &Env, handles: usize) -> Ring {
    let mut w = Walk::new(e);
    w.scan(false);
    if w.room && w.nf > 0 && w.closed(handles).is_ok() {
        w.empty(e);
        return Ring::Broken;
    }
    // A list or map of the ring's may hold the holds not found (FEEDBACK65.md, Q-941; branch
    // feedback65): the scope's own first, then those inside them, so that a record's long list of
    // plain values beside its lambda is not read (review of S2c, 1).
    for nested in [false, true] {
        let mut w = Walk::new(e);
        w.nested = nested;
        w.scan(true);
        if !w.room || w.nf == 0 {
            if nested {
                return Ring::None;
            }
            continue;
        }
        match w.closed(handles) {
            Ok(()) => {
                w.empty(e);
                return Ring::Broken;
            }
            // Nothing passed over: the walk into them would see the same. Otherwise its witness
            // goes before the next walk, which counts holds.
            Err(found) if !w.passed || nested => return Ring::Held(found),
            Err(_) => {}
        }
    }
    Ring::None
}

/// What made a listed scope's last look find its ring held from outside, so that the next look
/// can tell nothing changed without walking it again, lists and all: one of its functions, lists
/// or inner scopes held, with its count (the witness's own hold in it), or the scope's own count
/// (FEEDBACK65.md, Q-941 review of S2c, 1; branch feedback65).
enum Witness {
    None,
    Scope(usize),
    Value(Value, usize),
    Inner(Env, usize),
}

impl Witness {
    fn held(v: Value) -> Witness {
        let n = Witness::count(&v);
        Witness::Value(v, n)
    }

    fn count(v: &Value) -> usize {
        match v {
            Value::Func(c) => Arc::strong_count(c),
            _ => boxed(v).1,
        }
    }

    /// Whether what it saw is as it was: then so is the verdict.
    fn same(&self, e: &Env) -> bool {
        match self {
            Witness::None => false,
            Witness::Scope(n) => Arc::strong_count(e) == *n,
            Witness::Value(v, n) => Witness::count(v) == *n,
            Witness::Inner(y, n) => Arc::strong_count(y) == *n,
        }
    }
}

/// Scopes in a ring that something outside still held when they ended (`return f`, a stream's
/// function), with a hold each, looked at again a few at a time, from where the last look
/// stopped: `LOOK_EACH` each time one is listed, and `LOOK_TICK` a second from a server's event
/// loop (`look_again`), so that a server gone quiet frees them too, and no call waits on a look
/// over all of them (FEEDBACK65.md, Q-941 review 1 and 5; branch feedback65). Each is freed
/// once nothing outside holds its ring.
struct Watched {
    /// Each with what its last look saw held, if it saw one (boxed: most have none, and the
    /// list moves its items).
    list: Vec<(Env, Option<Box<Witness>>)>,
    /// Where the next look starts.
    at: usize,
}

/// Scopes looked at again each time one is listed: more than come, so the list never holds more
/// than those held from outside and a few.
const LOOK_EACH: usize = 2;
/// Scopes looked at again each second by an event loop, at most, and for at most `LOOK_TIME`:
/// the rest wait for the next second (FEEDBACK65.md, Q-941 review of S2c, 3; branch feedback65).
const LOOK_TICK: usize = 1024;
const LOOK_TIME: std::time::Duration = std::time::Duration::from_micros(300);

impl Drop for Watched {
    /// At the thread's end its list is left, not dropped: there, off the interpreter lock, a hold
    /// would go without its mark, and a ring looked at elsewhere would count it. Those rings stay,
    /// as all did before.
    fn drop(&mut self) {
        for e in self.list.drain(..) {
            std::mem::forget(e);
        }
    }
}

thread_local! {
    static WATCHED: std::cell::RefCell<Watched> = const { std::cell::RefCell::new(Watched { list: Vec::new(), at: 0 }) };
}

/// Puts `e`, in a ring something outside holds, on this thread's list with its hold, unless on
/// one, and looks again at a few.
fn watch(e: Env) {
    // Told by its first look, when it is still held then: most are not.
    watch_as(e, Witness::None)
}

/// `watch`, with what its last look saw held.
fn watch_as(e: Env, seen: Witness) {
    use std::sync::atomic::Ordering::Relaxed;
    if e.vars.watched.load(Relaxed) {
        return;
    }
    // The look first, so that it is not spent on this one, which something outside holds now.
    look(LOOK_EACH, None);
    e.vars.watched.store(true, Relaxed);
    let seen = (!matches!(seen, Witness::None)).then(|| Box::new(seen));
    WATCHED.with(|w| w.borrow_mut().list.push((e, seen)));
}

/// A second's look at this thread's listed scopes, from an event loop holding the interpreter
/// lock between requests (FEEDBACK65.md, Q-941 review 1; branch feedback65).
pub fn look_again() {
    look(LOOK_TICK, Some(std::time::Instant::now() + LOOK_TIME));
}

/// Looks again at up to `n` listed scopes, from where the last look stopped: one still held
/// from outside stays; one held by the list alone, no ring any more, or whose ring is now closed
/// (emptied) is let go of, its mark first.
fn look(n: usize, until: Option<std::time::Instant>) {
    use std::sync::atomic::Ordering::Relaxed;
    for k in 0..n {
        if k % 16 == 15 && until.is_some_and(|t| std::time::Instant::now() >= t) {
            return;
        }
        // Taken out while it is looked at, with the list's hold.
        let taken = WATCHED.with(|w| {
            let mut w = w.borrow_mut();
            if w.list.is_empty() {
                return None;
            }
            if w.at >= w.list.len() {
                w.at = 0;
            }
            let at = w.at;
            Some(w.list.swap_remove(at))
        });
        let Some((e, seen)) = taken else { return };
        // Kept (with what it saw held, if anything), or let go of.
        let keep: Option<Option<Box<Witness>>> = if Arc::strong_count(&e) == 1 {
            None
        } else if seen.as_ref().is_some_and(|w| w.same(&e)) {
            Some(seen)
        } else {
            // Let go of before the look, which counts holds.
            drop(seen);
            match own(&e, 0) {
                Own::Outside => Some(None),
                Own::Closed => {
                    empty_own(&e);
                    None
                }
                Own::None => None,
                Own::Look => match ring(&e, 0) {
                    Ring::Held(w) => Some(Some(Box::new(w))),
                    _ => None,
                },
            }
        };
        if let Some(seen) = keep {
            // Back where it was, and the look goes on after it.
            WATCHED.with(|w| {
                let mut w = w.borrow_mut();
                w.list.push((e, seen));
                let last = w.list.len() - 1;
                let at = w.at.min(last);
                w.list.swap(at, last);
                w.at = at + 1;
            });
        } else {
            e.vars.watched.store(false, Relaxed);
            done_out(e);
        }
    }
}

/// A listed scope let go of: kept for the next scope as `done` keeps one, and the scope around it
/// too when nothing else holds that now (the call's whose body it was), so that a function giving
/// back its kept lambda costs the next call no allocation.
fn done_out(mut e: Env) {
    let Some(s) = Arc::get_mut(&mut e) else { return Scope::done(e) };
    let around = s.parent.take();
    Scope::done(e);
    if let Some(p) = around.filter(|p| Arc::strong_count(p) == 1 && p.parent.is_some()) {
        Scope::done(p);
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
    // SAFETY: nothing else holds the block (asked above), which starts `HEADER` before the
    // text and has `layout` (see the test); `realloc` gives a block of the size `Arc` works
    // out for the longer text, with the counts as they were, and the new bytes are text
    // added to the end of text. If it fails, the old block is still there and is taken back.
    // The `Arc` is read out of `s` and another written in, with nothing between that can
    // unwind: an empty one put there meanwhile was an allocation for each `+=` (FEEDBACK55.md,
    // string speed; branch feedback55).
    unsafe {
        let held = std::ptr::read(s);
        let text = Arc::into_raw(held) as *const u8 as *mut u8;
        let block = text.sub(HEADER);
        let grown = realloc(block, layout, round(HEADER + new));
        if grown.is_null() {
            std::ptr::write(s, Arc::from_raw(std::ptr::slice_from_raw_parts(text as *const u8, old) as *const str));
            return false;
        }
        let text = grown.add(HEADER);
        std::ptr::copy_nonoverlapping(t.as_ptr(), text.add(old), t.len());
        std::ptr::write(s, Arc::from_raw(std::ptr::slice_from_raw_parts(text as *const u8, new) as *const str));
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
        // Any other key as `set` reads it, its text, but an error, which is no key and `set`
        // gives back (found in review of FEEDBACK55.md, string speed; branch feedback55).
        (Value::Map(m), [_, k, x]) if !matches!(k, Value::Err(..)) => {
            Arc::make_mut(m).insert(Key::from(k.key_text()), std::mem::replace(x, Value::Nil));
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

    /// What is large enough to be freed on another thread: its items and theirs, two levels down
    /// (2026-11-09; found in review): a page of rows is not, a few days of many rows are.
    #[test]
    fn weight_counts_what_items_hold() {
        let row = || Value::map([("id".into(), Value::Int(1)), ("message".into(), Value::str("m"))].into_iter().collect());
        let rows = |n: usize| (0..n).map(|_| row()).collect::<Vec<_>>();
        assert!(weight(rows(13).iter(), 0) < DEFER_AT);
        assert!(weight([].iter(), 0) == 0);
        assert!(weight(rows(90).iter(), 0) >= DEFER_AT);
        let days: Vec<Value> = (0..30).map(|_| Value::list(rows(1000))).collect();
        assert!(weight(days.iter(), 0) >= DEFER_AT);
        assert!(weight((0..300).map(|i| Value::Int(i)).collect::<Vec<_>>().iter(), 0) >= DEFER_AT);
        assert!(weight((0..255).map(|i| Value::Int(i)).collect::<Vec<_>>().iter(), 0) == 255);
        // Counted no further than it needs.
        let huge: Vec<Value> = vec![Value::list((0..1_000_000).map(Value::Int).collect())];
        let w = weight(huge.iter(), 0);
        assert!((DEFER_AT..=DEFER_AT + 1).contains(&w), "{}", w);
    }

    use super::*;

    /// `append_text` adds where the text is when nothing else holds it, of any length and in
    /// any script, and leaves a held one as it was, with no empty one put in its place
    /// meanwhile (FEEDBACK55.md, string speed; branch feedback55).
    #[test]
    fn a_text_is_added_to_where_it_is_when_nothing_else_holds_it() {
        let mut s: Arc<str> = Arc::from("ab");
        assert!(append_text(&mut s, "cé"));
        assert_eq!(&*s, "abcé");
        assert!(append_text(&mut s, ""));
        assert_eq!(&*s, "abcé");
        let held = s.clone();
        assert!(!append_text(&mut s, "x"));
        assert_eq!((&*s, &*held), ("abcé", "abcé"));
        drop(held);
        assert!(append_text(&mut s, "x"));
        assert_eq!(&*s, "abcéx");
        let mut e: Arc<str> = Arc::from("");
        assert!(append_text(&mut e, "😀"));
        for _ in 0..1000 {
            assert!(append_text(&mut e, "y"));
        }
        assert_eq!((e.len(), e.chars().count(), e.chars().next()), (1004, 1001, Some('😀')));
        assert_eq!(Arc::strong_count(&e), 1);
    }

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
