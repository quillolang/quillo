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
        match self {
            Value::List(l) => {
                out.push('[');
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.repr_into(out);
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
                    v.repr_into(out);
                }
                out.push_str(" }");
            }
            Value::Err(e, own, at) => {
                out.push_str("err(");
                if !own && hiding() {
                    crate::server::log_hidden(e, at);
                    out.push_str("\"internal error\"");
                } else {
                    e.repr_into(out);
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
            Value::Err(e, _, _) => serde_json::json!({ "error": e.to_json() }),
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
            Value::Err(e, _, _) => {
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

    /// JSON text as a value, read as `from_json` reads it, with no tree of serde_json's
    /// made and thrown away between: half of the time it took (FEEDBACK29.md; branch
    /// feedback29-fixes).
    pub fn parse_json(text: &str) -> serde_json::Result<Value> {
        serde_json::from_str::<Parsed>(text).map(|p| p.0)
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
            out.push(v);
        }
        Ok(Parsed(Value::list(out)))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Parsed, A::Error> {
        let mut out = Map::new();
        while let Some((ParsedKey(k), Parsed(v))) = map.next_entry()? {
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
        format!("{}", f)
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
    HIDING.load(std::sync::atomic::Ordering::Relaxed) && !TOLD.with(|t| t.get())
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
    // The shortest digits that give f back, and where the point goes:
    // f = 0.digits × 10^n.
    let sci = format!("{:e}", f.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    // Never in exponent form, on either side (FEEDBACK25.md, Q-309; branch
    // feedback26-fixes).
    let text = if k <= n {
        format!("{}{}", digits, "0".repeat((n - k) as usize))
    } else if 0 < n {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else {
        format!("0.{}{}", "0".repeat((-n) as usize), digits)
    };
    if f < 0.0 { format!("-{}", text) } else { text }
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
        (Err(x, ..), Err(y, ..)) => equal(x, y),
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
        // Item by item, which is how to sort by several keys: `sort_by(rows, (r) =>
        // [r.a, r.b])` (FEEDBACK25.md, Q-320; branch feedback26-fixes).
        (Value::List(x), Value::List(y)) => {
            for (p, q) in x.iter().zip(y.iter()) {
                match compare(p, q)? {
                    Ordering::Equal => {}
                    other => return Some(other),
                }
            }
            Some(x.len().cmp(&y.len()))
        }
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
