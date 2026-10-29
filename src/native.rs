//! Rust in a program: `.qwer` files.
//!
//! ```text
//! // imaging.qwer
//! //! [dependencies]
//! //! image = "0.25"
//!
//! /// How bright a picture is, from 0 to 255.
//! pub fn brightness(path: &str) -> Result<f64, String> { ... }
//!
//! // main.qwe
//! use "imaging.qwer"
//! route GET /b/:name = brightness(`photos/${name}.jpg`)?
//! ```
//!
//! A `.qwer` file is Rust. Its top-level `pub fn`s become the program's
//! functions, whose parameters and results are values Quillo has: numbers,
//! text, booleans, lists, maps, `Option` (null), `Result` (an error value)
//! and `serde_json::Value` (anything). Its `//! [dependencies]` header lists
//! crates, as in Cargo.toml.
//!
//! `use` builds it (once per content: the library is kept in
//! `.quillo/native/` next to it, named by a hash of what went into it) and
//! loads it. The build is a crate made in Quillo's cache, with the file as
//! a module and the code that takes Quillo's values in and gives them back:
//! as JSON, through a C function the library exports, which also turns a
//! panic into an error. A call runs off the event loop, without the lock
//! Quillo code holds, so a slow function holds up no one, and calls from
//! several requests run on several cores.
//!
//! The checker reads the same signatures from the file, without building
//! it. `--watch` and `--reload` build a file again when it changes; a build
//! that fails leaves the program as it was.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type R = Result<Value, Ctrl>;

/// Bumped when the generated code changes, so libraries are built again.
const GLUE_VERSION: &str = "1";

// ---- reading a .qwer file ----------------------------------------------------

/// An exported function, as written.
#[derive(Clone, Debug)]
pub struct Export {
    pub name: String,
    /// Each parameter's name and Rust type.
    pub params: Vec<(String, String)>,
    /// The Rust return type; `None` for `()`.
    pub ret: Option<String>,
    pub line: u32,
    /// Its `///` comment.
    pub doc: String,
}

#[derive(Debug, Default)]
pub struct Parsed {
    /// The `[dependencies]` header's lines, as TOML.
    pub deps: String,
    pub exports: Vec<Export>,
    /// What cannot be exported, by line.
    pub problems: Vec<(u32, String)>,
}

/// Reads a `.qwer` file's header and its top-level `pub fn`s.
pub fn parse(src: &str) -> Parsed {
    let mut out = Parsed::default();
    // The header: `//!` lines at the top, from `[dependencies]` on.
    let mut in_deps = false;
    for line in src.lines() {
        let Some(rest) = line.trim_start().strip_prefix("//!") else { break };
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        if rest.trim().starts_with('[') {
            in_deps = rest.trim() == "[dependencies]";
            continue;
        }
        if in_deps {
            out.deps.push_str(rest);
            out.deps.push('\n');
        }
    }
    let b = src.as_bytes();
    let (mut i, mut depth, mut line) = (0usize, 0i32, 1u32);
    let mut doc = String::new();
    while i < b.len() {
        let c = b[i];
        // Comments: `///` above an item is its documentation.
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            let end = src[i..].find('\n').map_or(b.len(), |n| i + n);
            let text = &src[i..end];
            if depth == 0 && text.starts_with("///") && !text.starts_with("////") {
                doc.push_str(text[3..].strip_prefix(' ').unwrap_or(&text[3..]));
                doc.push('\n');
            }
            i = end;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let end = src[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2);
            line += src[i..end].matches('\n').count() as u32;
            i = end;
            continue;
        }
        if c == b'"' || (c == b'r' && matches!(b.get(i + 1), Some(b'"' | b'#')) && (i == 0 || !is_ident(b[i - 1]))) {
            let end = skip_string(src, i);
            line += src[i..end].matches('\n').count() as u32;
            i = end;
            continue;
        }
        if c == b'\'' {
            i = skip_char(b, i);
            continue;
        }
        match c {
            b'\n' => {
                line += 1;
                // A blank line between a comment and an item parts them.
                if src[i + 1..].starts_with('\n') {
                    doc.clear();
                }
            }
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && c == b'p' && (i == 0 || !is_ident(b[i - 1])) && src[i..].starts_with("pub") && !b.get(i + 3).is_some_and(|x| is_ident(*x)) {
            match item(src, i + 3, line) {
                Some((Ok(mut e), end)) => {
                    e.line = line;
                    e.doc = std::mem::take(&mut doc).trim_end().to_string();
                    line += src[i..end].matches('\n').count() as u32;
                    out.exports.push(e);
                    i = end;
                    continue;
                }
                Some((Err(msg), _)) => out.problems.push((line, msg)),
                None => {}
            }
            doc.clear();
        }
        if depth == 0 && c == b';' {
            doc.clear();
        }
        i += 1;
    }
    // What a type that cannot cross makes of an export: a problem, and not
    // exported.
    let mut keep = Vec::new();
    for e in std::mem::take(&mut out.exports) {
        if e.params.iter().any(|(_, t)| t.starts_with("&mut")) {
            out.problems.push((e.line, format!("`{}` is not exported: a `&mut` parameter would change nothing Quillo sees", e.name)));
            continue;
        }
        let bad = e.params.iter().map(|(_, t)| t.as_str()).chain(e.ret.as_deref()).find_map(|t| quillo_type(t).err());
        match bad {
            Some(msg) => out.problems.push((e.line, format!("`{}` is not exported: {}", e.name, msg))),
            None => keep.push(e),
        }
    }
    out.exports = keep;
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn skip_string(src: &str, i: usize) -> usize {
    let b = src.as_bytes();
    if b[i] == b'r' {
        let hashes = b[i + 1..].iter().take_while(|c| **c == b'#').count();
        let close = format!("\"{}", "#".repeat(hashes));
        let start = i + 1 + hashes + 1;
        return src.get(start..).and_then(|s| s.find(&close)).map_or(b.len(), |n| start + n + close.len());
    }
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return j + 1,
            _ => j += 1,
        }
    }
    b.len()
}

/// A character literal (`'a'`, `'\n'`), or a lifetime (`'a`).
fn skip_char(b: &[u8], i: usize) -> usize {
    if b.get(i + 1) == Some(&b'\\') {
        return b[i + 2..].iter().position(|c| *c == b'\'').map_or(b.len(), |n| i + 2 + n + 1);
    }
    // One character (perhaps several bytes) and a closing quote.
    let rest = std::str::from_utf8(&b[i + 1..]).ok().and_then(|s| s.chars().next());
    match rest {
        Some(ch) if b.get(i + 1 + ch.len_utf8()) == Some(&b'\'') => i + 1 + ch.len_utf8() + 1,
        _ => i + 1,
    }
}

/// After `pub` at depth 0: an exported function, a problem, or `None` for
/// other items (`pub struct`, `pub(crate) fn`, ...).
fn item(src: &str, from: usize, _line: u32) -> Option<(Result<Export, String>, usize)> {
    let rest = &src[from..];
    let words: Vec<&str> = rest.split_whitespace().take(3).collect();
    let (kind, after) = match words.as_slice() {
        ["fn", ..] => ("", rest.find("fn")? + 2),
        ["const", "fn", ..] => ("", rest.find("fn")? + 2),
        [w @ ("async" | "unsafe" | "extern"), ..] if words.contains(&"fn") => (*w, rest.find("fn")? + 2),
        _ => return None,
    };
    let at = from + after;
    let body = &src[at..];
    let name: String = body.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    let name_end = at + body.find(&name)? + name.len();
    // The signature runs to the body's `{` (or a `;`).
    let sig_end = name_end + src[name_end..].find(['{', ';'])?;
    let sig = &src[name_end..sig_end];
    let end = if src.as_bytes()[sig_end] == b'{' { block_end(src, sig_end) } else { sig_end + 1 };
    if !kind.is_empty() {
        return Some((Err(format!("`{}` is not exported: `{} fn` cannot be called from Quillo", name, kind)), end));
    }
    let sig = sig.trim();
    if sig.starts_with('<') || sig.contains(" where ") || sig.contains("impl ") {
        return Some((Err(format!("`{}` is not exported: a generic function has no one type to call it with", name)), end));
    }
    let Some(open) = sig.find('(') else { return Some((Err(format!("`{}`: cannot read its parameters", name)), end)) };
    let close = matching(sig, open)?;
    let mut params = Vec::new();
    for p in split_top(&sig[open + 1..close]) {
        let Some((pat, ty)) = p.split_once(':') else {
            return Some((Err(format!("`{}`: cannot read the parameter `{}`", name, p)), end));
        };
        let pat = pat.trim().trim_start_matches("mut ").trim().to_string();
        params.push((pat, norm(ty)));
    }
    let ret = sig[close + 1..].trim().strip_prefix("->").map(norm).filter(|t| t != "()");
    Some((Ok(Export { name, params, ret, line: 0, doc: String::new() }), end))
}

/// Where the block opening at `open` ends (past its `}`).
fn block_end(src: &str, open: usize) -> usize {
    let b = src.as_bytes();
    let (mut i, mut depth) = (open, 0);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = src[i..].find('\n').map_or(b.len(), |n| i + n);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = src[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2);
                continue;
            }
            b'"' => {
                i = skip_string(src, i);
                continue;
            }
            b'r' if matches!(b.get(i + 1), Some(b'"' | b'#')) && !is_ident(b[i - 1]) => {
                i = skip_string(src, i);
                continue;
            }
            b'\'' => {
                i = skip_char(b, i);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

fn matching(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits at commas outside `<>`, `()` and `[]`.
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0);
    for c in s.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// A type with its spaces taken out where Rust does not need them.
fn norm(t: &str) -> String {
    let mut s = t.split_whitespace().collect::<Vec<_>>().join(" ");
    for (a, b) in [(" <", "<"), ("< ", "<"), (" >", ">"), (" ,", ","), ("& ", "&"), (" ::", "::"), (":: ", "::"), ("[ ", "["), (" ]", "]")] {
        while s.contains(a) {
            s = s.replace(a, b);
        }
    }
    s.replace(",", ", ").replace(",  ", ", ")
}

/// The Quillo type a Rust type crosses as, for the checker.
pub fn quillo_type(t: &str) -> Result<String, String> {
    let t = t.trim();
    let t = t.strip_prefix('&').map_or(t, str::trim);
    let inner = |t: &str, head: &str| t.strip_prefix(head).and_then(|r| r.strip_suffix('>')).map(str::to_string);
    let last = t.rsplit("::").next().unwrap_or(t);
    Ok(match last {
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => "int".into(),
        "f32" | "f64" => "float".into(),
        "bool" => "bool".into(),
        "String" | "str" | "char" => "str".into(),
        "()" => "null".into(),
        "Value" => "any".into(),
        _ if t.starts_with('[') => "list".into(),
        _ => {
            if let Some(x) = inner(t, "Option<") {
                let q = quillo_type(&x)?;
                return Ok(if q == "any" || q.ends_with("| null") { q } else { format!("{} | null", q) });
            }
            if let Some(x) = inner(t, "Result<") {
                let parts = split_top(&x);
                let ok = quillo_type(parts.first().map_or("()", |s| s.as_str()))?;
                return Ok(format!("{} | error(str)", ok));
            }
            for h in ["Vec<", "VecDeque<", "std::collections::VecDeque<"] {
                if let Some(x) = inner(t, h) {
                    quillo_type(&x)?;
                    return Ok("list".into());
                }
            }
            for h in ["HashMap<", "BTreeMap<", "std::collections::HashMap<", "std::collections::BTreeMap<"] {
                if let Some(x) = inner(t, h) {
                    let parts = split_top(&x);
                    if parts.first().map(|k| k.as_str()) != Some("String") {
                        return Err(format!("a map's keys are text: {} must have String keys", t));
                    }
                    quillo_type(parts.get(1).map_or("()", |s| s.as_str()))?;
                    return Ok("map".into());
                }
            }
            return Err(format!(
                "`{}` cannot cross between Rust and Quillo (numbers, bool, String, Vec, HashMap<String, _>, Option, Result and serde_json::Value can)",
                t
            ));
        }
    })
}

// ---- the crate around it ---------------------------------------------------------

/// The Rust that turns Quillo's values (as JSON) into an exported
/// function's arguments, and its result back.
const RUNTIME: &str = r#"
mod __quillo {
    use serde_json::Value;
    use std::collections::{BTreeMap, HashMap, VecDeque};

    fn kind(v: &Value) -> &'static str {
        match v {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(n) if n.is_f64() => "float",
            Value::Number(_) => "int",
            Value::String(_) => "str",
            Value::Array(_) => "list",
            Value::Object(_) => "map",
        }
    }

    pub trait FromQ: Sized {
        fn from_q(v: Value) -> Result<Self, String>;
    }
    pub trait ToQ {
        fn to_q(self) -> Result<Value, String>;
    }

    macro_rules! ints {
        ($($t:ty),*) => {$(
            impl FromQ for $t {
                fn from_q(v: Value) -> Result<Self, String> {
                    match &v {
                        Value::Number(n) if !n.is_f64() => n.as_i64().and_then(|i| <$t>::try_from(i).ok())
                            .or_else(|| n.as_u64().and_then(|i| <$t>::try_from(i).ok()))
                            .ok_or_else(|| format!("{} does not fit in {}", n, stringify!($t))),
                        _ => Err(format!("int, got {}", kind(&v))),
                    }
                }
            }
            impl ToQ for $t {
                fn to_q(self) -> Result<Value, String> {
                    i64::try_from(self).map(Value::from).map_err(|_| format!("{} is too large for Quillo's int", self))
                }
            }
        )*};
    }
    ints!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

    impl FromQ for f64 {
        fn from_q(v: Value) -> Result<Self, String> {
            v.as_f64().ok_or_else(|| format!("float, got {}", kind(&v)))
        }
    }
    impl FromQ for f32 {
        fn from_q(v: Value) -> Result<Self, String> {
            f64::from_q(v).map(|f| f as f32)
        }
    }
    impl ToQ for f64 {
        fn to_q(self) -> Result<Value, String> {
            serde_json::Number::from_f64(self).map(Value::Number).ok_or_else(|| format!("{} is not a number Quillo has", self))
        }
    }
    impl ToQ for f32 {
        fn to_q(self) -> Result<Value, String> {
            (self as f64).to_q()
        }
    }
    impl FromQ for bool {
        fn from_q(v: Value) -> Result<Self, String> {
            v.as_bool().ok_or_else(|| format!("bool, got {}", kind(&v)))
        }
    }
    impl ToQ for bool {
        fn to_q(self) -> Result<Value, String> {
            Ok(Value::Bool(self))
        }
    }
    impl FromQ for String {
        fn from_q(v: Value) -> Result<Self, String> {
            match v {
                Value::String(s) => Ok(s),
                v => Err(format!("str, got {}", kind(&v))),
            }
        }
    }
    impl FromQ for char {
        fn from_q(v: Value) -> Result<Self, String> {
            let s = String::from_q(v)?;
            let mut c = s.chars();
            match (c.next(), c.next()) {
                (Some(ch), None) => Ok(ch),
                _ => Err(format!("one character, got {:?}", s)),
            }
        }
    }
    impl ToQ for String {
        fn to_q(self) -> Result<Value, String> {
            Ok(Value::String(self))
        }
    }
    impl ToQ for &str {
        fn to_q(self) -> Result<Value, String> {
            Ok(Value::String(self.to_string()))
        }
    }
    impl ToQ for char {
        fn to_q(self) -> Result<Value, String> {
            Ok(Value::String(self.to_string()))
        }
    }
    impl ToQ for () {
        fn to_q(self) -> Result<Value, String> {
            Ok(Value::Null)
        }
    }
    impl FromQ for Value {
        fn from_q(v: Value) -> Result<Self, String> {
            Ok(v)
        }
    }
    impl ToQ for Value {
        fn to_q(self) -> Result<Value, String> {
            Ok(self)
        }
    }
    impl<T: FromQ> FromQ for Option<T> {
        fn from_q(v: Value) -> Result<Self, String> {
            if v.is_null() { Ok(None) } else { T::from_q(v).map(Some) }
        }
    }
    impl<T: ToQ> ToQ for Option<T> {
        fn to_q(self) -> Result<Value, String> {
            match self {
                Some(x) => x.to_q(),
                None => Ok(Value::Null),
            }
        }
    }
    impl<T: FromQ> FromQ for Vec<T> {
        fn from_q(v: Value) -> Result<Self, String> {
            match v {
                Value::Array(a) => a.into_iter().enumerate().map(|(i, x)| T::from_q(x).map_err(|e| format!("a list of {} (item {})", e, i))).collect(),
                v => Err(format!("list, got {}", kind(&v))),
            }
        }
    }
    impl<T: FromQ> FromQ for VecDeque<T> {
        fn from_q(v: Value) -> Result<Self, String> {
            Vec::from_q(v).map(VecDeque::from)
        }
    }
    impl<T: ToQ> ToQ for Vec<T> {
        fn to_q(self) -> Result<Value, String> {
            self.into_iter().map(ToQ::to_q).collect::<Result<Vec<_>, _>>().map(Value::Array)
        }
    }
    impl<T: ToQ> ToQ for VecDeque<T> {
        fn to_q(self) -> Result<Value, String> {
            self.into_iter().collect::<Vec<_>>().to_q()
        }
    }
    macro_rules! maps {
        ($($m:ident),*) => {$(
            impl<T: FromQ> FromQ for $m<String, T> {
                fn from_q(v: Value) -> Result<Self, String> {
                    match v {
                        Value::Object(o) => o.into_iter().map(|(k, x)| T::from_q(x).map(|x| (k, x))).collect(),
                        v => Err(format!("map, got {}", kind(&v))),
                    }
                }
            }
            impl<T: ToQ> ToQ for $m<String, T> {
                fn to_q(self) -> Result<Value, String> {
                    let mut o = serde_json::Map::new();
                    for (k, x) in self {
                        o.insert(k, x.to_q()?);
                    }
                    Ok(Value::Object(o))
                }
            }
        )*};
    }
    maps!(HashMap, BTreeMap);

    /// What a function gives back: a value, or (from a `Result`) an error.
    pub trait Back {
        fn back(self) -> Result<Result<Value, String>, String>;
    }
    impl<T: ToQ> Back for T {
        fn back(self) -> Result<Result<Value, String>, String> {
            self.to_q().map(Ok)
        }
    }
    impl<T: ToQ, E: std::fmt::Display> Back for Result<T, E> {
        fn back(self) -> Result<Result<Value, String>, String> {
            match self {
                Ok(x) => x.to_q().map(Ok),
                Err(e) => Ok(Err(e.to_string())),
            }
        }
    }

    pub fn arg<T: FromQ>(args: &mut Vec<Value>, i: usize, f: &str, name: &str) -> Result<T, String> {
        let v = args.get_mut(i).map(Value::take).unwrap_or(Value::Null);
        T::from_q(v).map_err(|e| format!("`{}`: argument `{}` must be {}", f, name, e))
    }

    /// Runs a call: `{"ok": v}`, `{"err": "..."}` (an error value),
    /// `{"bad": "..."}` (arguments it cannot take) or `{"panic": "..."}`.
    pub fn run(f: impl FnOnce() -> Result<Result<Value, String>, String>) -> Vec<u8> {
        let out = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(Ok(Ok(v))) => serde_json::json!({ "ok": v }),
            Ok(Ok(Err(e))) => serde_json::json!({ "err": e }),
            Ok(Err(e)) => serde_json::json!({ "bad": e }),
            Err(p) => {
                let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic".into());
                serde_json::json!({ "panic": msg })
            }
        };
        serde_json::to_vec(&out).unwrap_or_default()
    }
}
"#;

/// The crate's `lib.rs`: the file as a module, and a function per export.
fn glue(qwer: &Path, exports: &[Export]) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "// Made by Quillo from {}; changes here are lost.", qwer.display());
    s.push_str("#![allow(dead_code, unused_imports, unused_mut, clippy::all)]\n");
    let _ = writeln!(s, "#[path = {:?}]\nmod user;\n{}", qwer.display().to_string(), RUNTIME);
    s.push_str("#[no_mangle]\npub extern \"C\" fn quillo_native_abi() -> u32 { 1 }\n");
    let _ = writeln!(s, "#[no_mangle]\npub extern \"C\" fn quillo_native_count() -> u32 {{ {} }}", exports.len());
    s.push_str(
        "#[no_mangle]\npub unsafe extern \"C\" fn quillo_native_free(p: *mut u8, len: usize) {\n    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len)));\n}\n",
    );
    s.push_str("#[no_mangle]\npub unsafe extern \"C\" fn quillo_native_call(index: u32, args: *const u8, len: usize, out_len: *mut usize) -> *mut u8 {\n");
    s.push_str("    let input = std::slice::from_raw_parts(args, len);\n");
    s.push_str("    let out = __quillo::run(|| {\n");
    s.push_str("        let mut a: Vec<serde_json::Value> = serde_json::from_slice(input).map_err(|e| e.to_string())?;\n");
    s.push_str("        match index {\n");
    for (i, e) in exports.iter().enumerate() {
        let _ = writeln!(s, "            {} => {{", i);
        let mut call_args = Vec::new();
        for (j, (pname, ty)) in e.params.iter().enumerate() {
            // A reference is made from a value of its own.
            let (owned, by_ref) = match ty.strip_prefix('&') {
                Some(t) => {
                    let t = t.trim();
                    let owned = if t == "str" {
                        "String".to_string()
                    } else if let Some(inner) = t.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
                        format!("Vec<{}>", inner)
                    } else {
                        t.to_string()
                    };
                    (owned, true)
                }
                None => (ty.clone(), false),
            };
            let _ = writeln!(s, "                let a{}: {} = __quillo::arg(&mut a, {}, {:?}, {:?})?;", j, owned, j, e.name, pname);
            call_args.push(if by_ref { format!("&a{}", j) } else { format!("a{}", j) });
        }
        let _ = writeln!(s, "                __quillo::Back::back(user::{}({}))", e.name, call_args.join(", "));
        s.push_str("            }\n");
    }
    s.push_str("            _ => Err(\"no such function\".to_string()),\n        }\n    });\n");
    s.push_str("    let b = out.into_boxed_slice();\n    *out_len = b.len();\n    Box::into_raw(b) as *mut u8\n}\n");
    s
}

fn manifest(crate_name: &str, deps: &str) -> String {
    let mut deps = deps.to_string();
    if !deps.lines().any(|l| l.trim_start().starts_with("serde_json")) {
        deps.push_str("serde_json = \"1\"\n");
    }
    format!(
        "[package]\nname = \"{}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\npath = \"lib.rs\"\n\n[dependencies]\n{}\n[profile.release]\npanic = \"unwind\"\n\n[workspace]\n",
        crate_name, deps
    )
}

// ---- building ---------------------------------------------------------------------

/// Quillo's cache: the crates it makes, and their shared build directory.
fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("QUILLO_NATIVE_CACHE") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("quillo").join("native")
}

fn stem(qwer: &Path) -> String {
    let s: String = qwer.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("q{}", s)
    } else {
        s
    }
}

fn hex_hash(parts: &[&[u8]]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for p in parts {
        h.update(p);
        h.update([0]);
    }
    crate::crypto::hex(&h.finalize()[..8])
}

/// The crate around a `.qwer` file, written (again) in Quillo's cache: its
/// name and folder.
fn prepare(abs: &Path, parsed: &Parsed) -> Result<(String, PathBuf), String> {
    let crate_name = format!("quillo_{}_{}", stem(abs), &hex_hash(&[abs.display().to_string().as_bytes()])[..8]);
    let dir = cache_dir().join("crates").join(&crate_name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {}", dir.display(), e))?;
    let write = |f: &str, text: String| {
        // Unchanged files are left alone, so cargo has nothing to redo.
        if std::fs::read_to_string(dir.join(f)).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(dir.join(f), text).map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    };
    write("Cargo.toml", manifest(&crate_name, &parsed.deps))?;
    write("lib.rs", glue(abs, &parsed.exports))?;
    Ok((crate_name, dir))
}

/// What the Rust compiler says of a `.qwer` file (as it is on disk), for
/// editors: `cargo check` of its crate, with what it says of the file.
#[derive(Debug, Clone)]
pub struct Note {
    pub error: bool,
    /// 1-based lines, 0-based columns (in characters), as far as `end`.
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub message: String,
}

pub fn check(qwer: &Path) -> Result<Vec<Note>, String> {
    let src = std::fs::read_to_string(qwer).map_err(|e| format!("cannot read {}: {}", qwer.display(), e))?;
    let parsed = parse(&src);
    let abs = qwer.canonicalize().unwrap_or_else(|_| qwer.to_path_buf());
    let (_, dir) = prepare(&abs, &parsed)?;
    let out = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["check", "--message-format=json", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(cache_dir().join("check-target"))
        .output()
        .map_err(|e| format!("cargo did not run ({}): install Rust (https://rustup.rs) for its errors here", e))?;
    let mut notes = Vec::new();
    let here = abs.display().to_string();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(j) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if j["reason"] != "compiler-message" {
            continue;
        }
        let m = &j["message"];
        let level = m["level"].as_str().unwrap_or("");
        if !matches!(level, "error" | "warning") || m["message"].as_str().is_some_and(|t| t.starts_with("aborting due to")) {
            continue;
        }
        let mut text = m["message"].as_str().unwrap_or("").to_string();
        for c in m["children"].as_array().into_iter().flatten() {
            if let Some(t) = c["message"].as_str().filter(|t| !t.is_empty()) {
                text.push_str(&format!("\n{}: {}", c["level"].as_str().unwrap_or("note"), t));
            }
        }
        let spans = m["spans"].as_array().cloned().unwrap_or_default();
        let span = spans.iter().find(|s| s["is_primary"] == true && s["file_name"] == here.as_str()).or_else(|| spans.iter().find(|s| s["file_name"] == here.as_str()));
        let note = match span {
            Some(s) => Note {
                error: level == "error",
                line: s["line_start"].as_u64().unwrap_or(1) as u32,
                col: s["column_start"].as_u64().unwrap_or(1).saturating_sub(1) as u32,
                end_line: s["line_end"].as_u64().unwrap_or(1) as u32,
                end_col: s["column_end"].as_u64().unwrap_or(1).saturating_sub(1) as u32,
                message: text,
            },
            // In the code made around the file (a type it cannot take, a
            // crate that is not there): on its first line.
            None if level == "error" => Note { error: true, line: 1, col: 0, end_line: 1, end_col: 0, message: text },
            None => continue,
        };
        notes.push(note);
    }
    if notes.is_empty() && !out.status.success() {
        let log = String::from_utf8_lossy(&out.stderr);
        let first = log.lines().find(|l| l.contains("error")).unwrap_or("cargo check failed").trim().to_string();
        notes.push(Note { error: true, line: 1, col: 0, end_line: 1, end_col: 0, message: first });
    }
    Ok(notes)
}

/// The library for a `.qwer` file: built now unless one was built from the
/// same content, kept in `.quillo/native/` next to the file.
pub fn build(qwer: &Path) -> Result<(PathBuf, Parsed), String> {
    let src = std::fs::read_to_string(qwer).map_err(|e| format!("cannot read {}: {}", qwer.display(), e))?;
    let parsed = parse(&src);
    let abs = qwer.canonicalize().unwrap_or_else(|_| qwer.to_path_buf());
    let name = stem(qwer);
    let hash = hex_hash(&[src.as_bytes(), GLUE_VERSION.as_bytes(), std::env::consts::ARCH.as_bytes()]);
    let out_dir = abs.parent().unwrap_or(Path::new(".")).join(".quillo").join("native");
    let lib = out_dir.join(format!("{}{}-{}.{}", std::env::consts::DLL_PREFIX, name, hash, std::env::consts::DLL_EXTENSION));
    if lib.is_file() {
        return Ok((lib, parsed));
    }
    // One build at a time in this process (instances load the same files).
    static BUILDING: Mutex<()> = Mutex::new(());
    let _one = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    if lib.is_file() {
        return Ok((lib, parsed));
    }
    let (crate_name, dir) = prepare(&abs, &parsed)?;
    let target = cache_dir().join("target");
    eprintln!("quillo: building {}", qwer.display());
    let out = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--message-format=short", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .output()
        .map_err(|e| {
            format!(
                "cannot build {}: cargo did not run ({}). Install Rust (https://rustup.rs), or build it where Rust is with `quillo build` and bring .quillo/native along",
                qwer.display(),
                e
            )
        })?;
    if !out.status.success() {
        let log = String::from_utf8_lossy(&out.stderr);
        let errors: Vec<&str> = log
            .lines()
            .filter(|l| !l.trim_start().starts_with("Compiling") && !l.trim_start().starts_with("Updating") && !l.trim_start().starts_with("Locking"))
            .collect();
        return Err(format!("cannot build {}:\n{}", qwer.display(), errors.join("\n")));
    }
    let built = target
        .join("release")
        .join(format!("{}{}.{}", std::env::consts::DLL_PREFIX, crate_name, std::env::consts::DLL_EXTENSION));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("cannot make {}: {}", out_dir.display(), e))?;
    // Copied, so the next build of this file does not change a library that
    // is loaded.
    let tmp = lib.with_extension("tmp");
    std::fs::copy(&built, &tmp).map_err(|e| format!("cannot copy {}: {}", built.display(), e))?;
    std::fs::rename(&tmp, &lib).map_err(|e| e.to_string())?;
    // Libraries of this file's earlier contents go (those loaded stay loaded).
    if let Ok(rd) = std::fs::read_dir(&out_dir) {
        let prefix = format!("{}{}-", std::env::consts::DLL_PREFIX, name);
        for e in rd.flatten() {
            let p = e.path();
            let f = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            if p != lib && f.starts_with(&prefix) && f.len() == prefix.len() + 16 + 1 + std::env::consts::DLL_EXTENSION.len() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok((lib, parsed))
}

// ---- loading and calling ---------------------------------------------------------------

type CallFn = unsafe extern "C" fn(u32, *const u8, usize, *mut usize) -> *mut u8;
type FreeFn = unsafe extern "C" fn(*mut u8, usize);

struct Lib {
    call: CallFn,
    free: FreeFn,
    /// Kept loaded for as long as the process runs: a program of an earlier
    /// version may still be calling it.
    _lib: libloading::Library,
}

struct Function {
    lib: Arc<Lib>,
    index: u32,
    name: String,
    file: String,
}

fn functions() -> &'static Mutex<HashMap<&'static str, Arc<Function>>> {
    static F: OnceLock<Mutex<HashMap<&'static str, Arc<Function>>>> = OnceLock::new();
    F.get_or_init(Default::default)
}

fn libraries() -> &'static Mutex<HashMap<PathBuf, Arc<Lib>>> {
    static L: OnceLock<Mutex<HashMap<PathBuf, Arc<Lib>>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

fn open(path: &Path, count: usize) -> Result<Arc<Lib>, String> {
    let mut libs = libraries().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(l) = libs.get(path) {
        return Ok(l.clone());
    }
    let bad = |e: libloading::Error| format!("cannot load {}: {}", path.display(), e);
    // SAFETY: the library is one Quillo made, with the functions below.
    unsafe {
        let lib = libloading::Library::new(path).map_err(bad)?;
        let abi: libloading::Symbol<unsafe extern "C" fn() -> u32> = lib.get(b"quillo_native_abi").map_err(bad)?;
        let n: libloading::Symbol<unsafe extern "C" fn() -> u32> = lib.get(b"quillo_native_count").map_err(bad)?;
        if abi() != 1 || n() as usize != count {
            return Err(format!("{} was made for another version of Quillo; delete it to build it again", path.display()));
        }
        let call: CallFn = *lib.get::<CallFn>(b"quillo_native_call").map_err(bad)?;
        let free: FreeFn = *lib.get::<FreeFn>(b"quillo_native_free").map_err(bad)?;
        let l = Arc::new(Lib { call, free, _lib: lib });
        libs.insert(path.to_path_buf(), l.clone());
        Ok(l)
    }
}

/// `use "x.qwer"`: builds and loads it, and defines its functions.
pub fn use_file(it: &mut Interp, qwer: &Path, only: Option<&[(String, String)]>, line: u32) -> Result<(), Ctrl> {
    let path = qwer.to_path_buf();
    // A build takes seconds: other requests go on meanwhile (on a reload,
    // with the version that runs now).
    let built = it.unlocked(move || build(&path));
    let (lib_path, parsed) = built.map_err(|e| Ctrl::Error(crate::error::QuilloError::new(e, line)))?;
    let lib = open(&lib_path, parsed.exports.len()).map_err(|e| Ctrl::Error(crate::error::QuilloError::new(e, line)))?;
    let file = qwer.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    // `use a, b as c from "x.qwer"`: each must be one of its functions (branch imports).
    if let Some(only) = only {
        for (name, _) in only {
            if !parsed.exports.iter().any(|e| e.name == *name) {
                return Err(Ctrl::Error(crate::error::QuilloError::new(missing_export(&file, name, &parsed.exports), line)));
            }
        }
    }
    for (i, e) in parsed.exports.iter().enumerate() {
        let names: Vec<&str> = match only {
            Some(only) => only.iter().filter(|(n, _)| *n == e.name).map(|(_, a)| a.as_str()).collect(),
            None => vec![e.name.as_str()],
        };
        if names.is_empty() {
            continue;
        }
        // The builtin's name says which function it is: one per library, so
        // a version loaded later has names of its own.
        let key: &'static str = Box::leak(format!("native:{}:{}", lib_path.display(), e.name).into_boxed_str());
        functions().lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_insert_with(|| {
            Arc::new(Function { lib: lib.clone(), index: i as u32, name: e.name.clone(), file: file.clone() })
        });
        for n in names {
            define(&it.globals, n, Value::Builtin(key, call_placeholder));
        }
    }
    Ok(())
}

/// What a `use` is told when it takes a name the file does not have: the nearest of
/// those it has.
pub fn missing_export(file: &str, name: &str, exports: &[Export]) -> String {
    let mut have: Vec<&str> = exports.iter().map(|e| e.name.as_str()).collect();
    have.sort();
    let near = have.iter().min_by_key(|h| edit_distance(name, h)).filter(|h| edit_distance(name, h) <= 2);
    match near {
        Some(n) => format!("{} has no function `{}` (did you mean `{}`?)", file, name, n),
        None => format!("{} has no function `{}`; it has {}", file, name, have.iter().map(|h| format!("`{}`", h)).collect::<Vec<_>>().join(", ")),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + if a[i - 1] == b[j - 1] { 0 } else { 1 });
            prev = cur;
        }
    }
    row[b.len()]
}

/// Never called: `Interp::call` sends a `native:` builtin to `call`.
fn call_placeholder(_: &mut Interp, _: Vec<Value>, line: u32) -> R {
    fail(line, "a Rust function was called without its library")
}

/// A call of a `.qwer` function, off the event loop.
pub fn call(it: &mut Interp, key: &str, args: Vec<Value>, line: u32) -> R {
    let Some(f) = functions().lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned() else {
        return fail(line, "a Rust function was called without its library");
    };
    let input = serde_json::to_vec(&serde_json::Value::Array(args.iter().map(|a| a.to_json()).collect())).unwrap_or_default();
    let lib = f.lib.clone();
    let index = f.index;
    let out = it.unlocked(move || {
        let mut len = 0usize;
        // SAFETY: the library's own functions, with its own buffer freed by it.
        unsafe {
            let p = (lib.call)(index, input.as_ptr(), input.len(), &mut len);
            let bytes = std::slice::from_raw_parts(p, len).to_vec();
            (lib.free)(p, len);
            bytes
        }
    });
    let j: serde_json::Value = serde_json::from_slice(&out).unwrap_or(serde_json::Value::Null);
    if let Some(v) = j.get("ok") {
        return Ok(Value::from_json(v));
    }
    if let Some(e) = j.get("err") {
        return Ok(Value::err(Value::from_json(e)));
    }
    if let Some(e) = j.get("bad").and_then(|e| e.as_str()) {
        return fail(line, e.to_string());
    }
    let why = j.get("panic").and_then(|e| e.as_str()).unwrap_or("it gave back nothing");
    fail(line, format!("`{}` ({}) panicked: {}", f.name, f.file, why))
}

/// What a `native:` builtin is called, for messages: its function's name.
pub fn display_name(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

/// `quillo build`: every `.qwer` file a program uses, built now.
pub fn build_all(main: &Path) -> Result<Vec<PathBuf>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut todo = vec![main.to_path_buf()];
    while let Some(f) = todo.pop() {
        let key = f.canonicalize().unwrap_or_else(|_| f.clone());
        if !seen.insert(key) {
            continue;
        }
        let dir = f.parent().unwrap_or(Path::new(".")).to_path_buf();
        if f.extension().is_some_and(|e| e == "qwer") {
            out.push(build(&f)?.0);
            continue;
        }
        let src = std::fs::read_to_string(&f).map_err(|e| format!("cannot read {}: {}", f.display(), e))?;
        let prog = crate::parser::Parser::new(&src).parse_program().map_err(|e| e.in_file(&f.display().to_string()).to_string())?;
        for s in prog {
            match s {
                crate::ast::Stmt::Use(p, _, _) if !crate::ast::is_script(&p) => todo.push(dir.join(p)),
                crate::ast::Stmt::Mount(_, p, _) => todo.push(dir.join(p)),
                _ => {}
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r##"//! Pictures.
//!
//! [dependencies]
//! image = { version = "0.25", default-features = false }

use std::collections::HashMap;

/// How bright it is.
/// From 0 to 255.
pub fn brightness(path: &str) -> Result<f64, String> {
    let s = "pub fn not_this() {}";
    let r = r#"} pub fn nor_this() {"#;
    let c = '}';
    Ok(0.0)
}

fn private(x: i64) -> i64 { x }

pub fn stats(xs: Vec<f64>, names: &[String]) -> HashMap<String, f64> {
    HashMap::new()
}

pub fn maybe(n: Option<i64>, mut flag: bool) -> Option<String> { None }

pub fn nothing() {}

pub struct Point { pub x: i64 }

impl Point {
    pub fn inside(&self) -> i64 { self.x }
}

pub fn generic<T: Clone>(x: T) -> T { x }

pub fn point() -> Point { Point { x: 1 } }

pub async fn later() {}

pub fn lifetimes<'a>(s: &'a str) -> &'a str { s }
"##;

    #[test]
    fn exports_and_their_types() {
        let p = parse(SRC);
        assert_eq!(p.deps.trim(), "image = { version = \"0.25\", default-features = false }");
        let names: Vec<&str> = p.exports.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["brightness", "stats", "maybe", "nothing"]);
        let b = &p.exports[0];
        assert_eq!((b.line, b.doc.as_str()), (10, "How bright it is.\nFrom 0 to 255."));
        assert_eq!(b.params, vec![("path".to_string(), "&str".to_string())]);
        assert_eq!(b.ret.as_deref(), Some("Result<f64, String>"));
        assert_eq!(p.exports[1].params[1], ("names".to_string(), "&[String]".to_string()));
        assert_eq!(p.exports[2].params[1].0, "flag");
        assert_eq!(p.exports[3].ret, None);
        let problems: Vec<String> = p.problems.iter().map(|(l, m)| format!("{} {}", l, m)).collect();
        assert!(problems.iter().any(|m| m.contains("`generic` is not exported: a generic")), "{:?}", problems);
        assert!(problems.iter().any(|m| m.contains("`point` is not exported: `Point` cannot cross")), "{:?}", problems);
        assert!(problems.iter().any(|m| m.contains("`later` is not exported: `async fn`")), "{:?}", problems);
        assert!(problems.iter().any(|m| m.contains("`lifetimes` is not exported")), "{:?}", problems);
        assert!(!problems.iter().any(|m| m.contains("not_this") || m.contains("nor_this") || m.contains("inside")), "{:?}", problems);
    }

    #[test]
    fn types_as_quillo_sees_them() {
        for (rust, q) in [
            ("i64", "int"),
            ("u8", "int"),
            ("f32", "float"),
            ("&str", "str"),
            ("Vec<Vec<i64>>", "list"),
            ("&[f64]", "list"),
            ("HashMap<String, Vec<f64>>", "map"),
            ("Option<i64>", "int | null"),
            ("Result<f64, String>", "float | error(str)"),
            ("Result<Option<String>, std::io::Error>", "str | null | error(str)"),
            ("serde_json::Value", "any"),
            ("Option<serde_json::Value>", "any"),
        ] {
            assert_eq!(quillo_type(rust).unwrap(), q, "{}", rust);
        }
        assert!(quillo_type("HashMap<i64, f64>").unwrap_err().contains("String keys"));
        assert!(quillo_type("Point").is_err());
    }
}
