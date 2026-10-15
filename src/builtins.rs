//! The standard library: functions every program can call.

use crate::interp::{coerce, escape, fail, Ctrl, Interp};
use crate::value::*;
use indexmap::IndexMap;
use std::path::{Component, Path};
use std::sync::Arc;

type R = Result<Value, Ctrl>;

pub fn install(env: &Env) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("print", print),
        ("str", to_str),
        ("int", to_int),
        ("float", to_float),
        ("len", len),
        ("type_of", type_of),
        ("upper", upper),
        ("lower", lower),
        ("trim", trim),
        ("split", split),
        ("join", join),
        ("contains", contains),
        ("starts_with", starts_with),
        ("ends_with", ends_with),
        ("replace", replace),
        ("slice", slice),
        ("map", map),
        ("filter", filter),
        ("find", find),
        ("any", any),
        ("all", all),
        ("reduce", reduce),
        ("sum", sum),
        ("min", min),
        ("max", max),
        ("sort", sort),
        ("sort_by", sort_by),
        ("reverse", reverse),
        ("first", first),
        ("last", last),
        ("push", push),
        ("range", range),
        ("enumerate", enumerate),
        ("keys", keys),
        ("values", values),
        ("get", get),
        ("has", has),
        ("set", set),
        ("remove", remove),
        ("merge", merge),
        ("err", err),
        ("is_err", is_err),
        ("unwrap", unwrap),
        ("raw", raw),
        ("escape", escape_fn),
        ("env", env_var),
        ("now", now),
        ("sleep", sleep),
        ("file", file),
        ("redirect", redirect),
        ("respond", respond),
        ("read_file", read_file),
        ("write_file", write_file),
        ("random", random),
        ("random_int", random_int),
        ("uuid", uuid),
        ("abs", abs),
        ("round", round),
        ("floor", floor),
        ("ceil", ceil),
        ("sqrt", sqrt),
        ("pow", pow),
        ("unique", unique),
        ("flatten", flatten),
        ("zip", zip),
        ("take", take),
        ("drop", drop_fn),
        ("count", count),
        ("group_by", group_by),
        ("index_of", index_of),
        ("repeat", repeat),
        ("live", live),
        ("publish", publish),
        ("watch", watch),
    ];
    for (name, f) in fns {
        define(env, name, Value::Builtin(name, *f));
    }
    let mut json = Map::new();
    json.insert("parse".into(), Value::Builtin("json.parse", json_parse));
    json.insert("stringify".into(), Value::Builtin("json.stringify", json_stringify));
    define(env, "json", Value::map(json));
    let mut db = Map::new();
    crate::db::install(&mut db);
    define(env, "db", Value::map(db));
    let mut http = Map::new();
    crate::fetch::install(&mut http);
    define(env, "http", Value::map(http));
}

/// Every builtin name, as programs see it (`db.query`, `len`, ...).
pub fn names() -> Vec<String> {
    let env = Scope::root();
    install(&env);
    let mut out = Vec::new();
    for (k, v) in scope_vars(&env) {
        match v {
            Value::Map(m) => out.extend(m.keys().map(|f| format!("{}.{}", k, f))),
            _ => out.push(k),
        }
    }
    out.sort();
    out
}

// ---- argument helpers --------------------------------------------------

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Nil)
}

fn tn(v: Option<&Value>) -> &'static str {
    v.map_or("nothing", |v| v.type_name())
}

fn want<T>(args: &[Value], i: usize, f: &str, what: &str, line: u32, get: impl Fn(&Value) -> Option<T>) -> Result<T, Ctrl> {
    match args.get(i).and_then(|v| get(v)) {
        Some(x) => Ok(x),
        None => fail(line, format!("`{}` expects {} as argument {}, got {}", f, what, i + 1, tn(args.get(i)))),
    }
}

fn s_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<Arc<str>, Ctrl> {
    want(args, i, f, "a str", line, |v| if let Value::Str(s) = v { Some(s.clone()) } else { None })
}

fn l_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<Arc<Vec<Value>>, Ctrl> {
    want(args, i, f, "a list", line, |v| match v {
        Value::List(l) => Some(l.clone()),
        Value::Nil => Some(Arc::new(Vec::new())),
        _ => None,
    })
}

fn i_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<i64, Ctrl> {
    want(args, i, f, "an int", line, |v| if let Value::Int(n) = v { Some(*n) } else { None })
}

fn m_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<Arc<Map>, Ctrl> {
    want(args, i, f, "a map", line, |v| match v {
        Value::Map(m) => Some(m.clone()),
        Value::Nil => Some(Arc::new(Map::new())),
        _ => None,
    })
}

fn f_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<Value, Ctrl> {
    want(args, i, f, "a function", line, |v| {
        if matches!(v, Value::Func(_) | Value::Builtin(..)) {
            Some(v.clone())
        } else {
            None
        }
    })
}

// ---- general -----------------------------------------------------------

fn print(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    println!("{}", args.iter().map(|v| v.display()).collect::<Vec<_>>().join(" "));
    Ok(Value::Nil)
}

fn to_str(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::str(arg(&args, 0).display()))
}

fn to_int(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match arg(&args, 0) {
        Value::Int(n) => Value::Int(n),
        Value::Float(f) if f.is_finite() => Value::Int(f.trunc() as i64),
        Value::Bool(b) => Value::Int(b as i64),
        Value::Str(s) => coerce("int", &s).unwrap_or_else(|| Value::err(Value::str(format!("not an int: {:?}", &*s)))),
        other => Value::err(Value::str(format!("cannot convert {} to int", other.type_name()))),
    })
}

fn to_float(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match arg(&args, 0) {
        Value::Int(n) => Value::Float(n as f64),
        Value::Float(f) => Value::Float(f),
        Value::Str(s) => {
            coerce("float", &s).unwrap_or_else(|| Value::err(Value::str(format!("not a number: {:?}", &*s))))
        }
        other => Value::err(Value::str(format!("cannot convert {} to float", other.type_name()))),
    })
}

fn len(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(match arg(&args, 0) {
        Value::Str(s) => s.chars().count() as i64,
        Value::List(l) => l.len() as i64,
        Value::Map(m) => m.len() as i64,
        Value::Html(h) => h.len() as i64,
        Value::Nil => 0,
        other => return fail(line, format!("`len` expects a str, list or map, got {}", other.type_name())),
    }))
}

fn type_of(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::str(arg(&args, 0).type_name()))
}

// ---- strings -----------------------------------------------------------

fn upper(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::str(s_arg(&args, 0, "upper", line)?.to_uppercase()))
}

fn lower(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::str(s_arg(&args, 0, "lower", line)?.to_lowercase()))
}

fn trim(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::str(s_arg(&args, 0, "trim", line)?.trim()))
}

fn split(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "split", line)?;
    let sep = if args.len() > 1 { s_arg(&args, 1, "split", line)? } else { Arc::from(" ") };
    let parts: Vec<Value> = if sep.is_empty() {
        s.chars().map(|c| Value::str(c.to_string())).collect()
    } else {
        s.split(&*sep).map(Value::str).collect()
    };
    Ok(Value::list(parts))
}

fn join(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "join", line)?;
    let sep = if args.len() > 1 { s_arg(&args, 1, "join", line)? } else { Arc::from("") };
    Ok(Value::str(l.iter().map(|v| v.display()).collect::<Vec<_>>().join(&sep)))
}

fn contains(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let needle = arg(&args, 1);
    Ok(Value::Bool(match arg(&args, 0) {
        Value::Str(s) => s.contains(&*needle.display()),
        Value::List(l) => l.iter().any(|v| equal(v, &needle)),
        Value::Map(m) => m.contains_key(&*needle.display()),
        Value::Nil => false,
        other => return fail(line, format!("`contains` expects a str, list or map, got {}", other.type_name())),
    }))
}

fn starts_with(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(s_arg(&args, 0, "starts_with", line)?.starts_with(&*s_arg(&args, 1, "starts_with", line)?)))
}

fn ends_with(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(s_arg(&args, 0, "ends_with", line)?.ends_with(&*s_arg(&args, 1, "ends_with", line)?)))
}

fn replace(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "replace", line)?;
    let from = s_arg(&args, 1, "replace", line)?;
    let to = s_arg(&args, 2, "replace", line)?;
    Ok(Value::str(s.replace(&*from, &to)))
}

fn clamp_range(len: usize, start: i64, end: Option<i64>) -> (usize, usize) {
    let fix = |i: i64| -> usize {
        let i = if i < 0 { len as i64 + i } else { i };
        i.clamp(0, len as i64) as usize
    };
    let s = fix(start);
    let e = end.map_or(len, fix);
    (s, e.max(s))
}

fn slice(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let start = i_arg(&args, 1, "slice", line)?;
    let end = if args.len() > 2 { Some(i_arg(&args, 2, "slice", line)?) } else { None };
    match arg(&args, 0) {
        Value::List(l) => {
            let (s, e) = clamp_range(l.len(), start, end);
            Ok(Value::list(l[s..e].to_vec()))
        }
        Value::Str(st) => {
            let chars: Vec<char> = st.chars().collect();
            let (s, e) = clamp_range(chars.len(), start, end);
            Ok(Value::str(chars[s..e].iter().collect::<String>()))
        }
        other => fail(line, format!("`slice` expects a list or str, got {}", other.type_name())),
    }
}

// ---- lists -------------------------------------------------------------

fn map(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "map", line)?;
    let f = f_arg(&args, 1, "map", line)?;
    let mut out = Vec::with_capacity(l.len());
    for x in l.iter() {
        out.push(it.call(&f, vec![x.clone()], line)?);
    }
    Ok(Value::list(out))
}

fn filter(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "filter", line)?;
    let f = f_arg(&args, 1, "filter", line)?;
    let mut out = Vec::new();
    for x in l.iter() {
        if it.call(&f, vec![x.clone()], line)?.truthy() {
            out.push(x.clone());
        }
    }
    Ok(Value::list(out))
}

fn find(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "find", line)?;
    let f = f_arg(&args, 1, "find", line)?;
    for x in l.iter() {
        if it.call(&f, vec![x.clone()], line)?.truthy() {
            return Ok(x.clone());
        }
    }
    Ok(Value::Nil)
}

fn any(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "any", line)?;
    let f = f_arg(&args, 1, "any", line)?;
    for x in l.iter() {
        if it.call(&f, vec![x.clone()], line)?.truthy() {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

fn all(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "all", line)?;
    let f = f_arg(&args, 1, "all", line)?;
    for x in l.iter() {
        if !it.call(&f, vec![x.clone()], line)?.truthy() {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn reduce(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "reduce", line)?;
    let mut acc = arg(&args, 1);
    let f = f_arg(&args, 2, "reduce", line)?;
    for x in l.iter() {
        acc = it.call(&f, vec![acc, x.clone()], line)?;
    }
    Ok(acc)
}

fn sum(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "sum", line)?;
    let mut acc = Value::Int(0);
    for x in l.iter() {
        acc = crate::interp::binop(crate::ast::BinOp::Add, acc, x.clone(), line)?;
    }
    Ok(acc)
}

fn extreme(args: &[Value], name: &str, line: u32, want: std::cmp::Ordering) -> R {
    let l = l_arg(args, 0, name, line)?;
    let mut best: Option<Value> = None;
    for x in l.iter() {
        best = Some(match best {
            None => x.clone(),
            Some(b) => match compare(x, &b) {
                Some(o) if o == want => x.clone(),
                Some(_) => b,
                None => return fail(line, format!("`{}` cannot compare {} with {}", name, x.type_name(), b.type_name())),
            },
        });
    }
    Ok(best.unwrap_or(Value::Nil))
}

fn min(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    extreme(&args, "min", line, std::cmp::Ordering::Less)
}

fn max(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    extreme(&args, "max", line, std::cmp::Ordering::Greater)
}

fn sort_values(mut items: Vec<(Value, Value)>, name: &str, line: u32) -> R {
    let mut bad = None;
    items.sort_by(|a, b| {
        compare(&a.0, &b.0).unwrap_or_else(|| {
            bad = Some((a.0.type_name(), b.0.type_name()));
            std::cmp::Ordering::Equal
        })
    });
    if let Some((x, y)) = bad {
        return fail(line, format!("`{}` cannot compare {} with {}", name, x, y));
    }
    Ok(Value::list(items.into_iter().map(|(_, v)| v).collect()))
}

fn sort(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "sort", line)?;
    sort_values(l.iter().map(|v| (v.clone(), v.clone())).collect(), "sort", line)
}

fn sort_by(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "sort_by", line)?;
    let f = f_arg(&args, 1, "sort_by", line)?;
    let mut items = Vec::with_capacity(l.len());
    for x in l.iter() {
        items.push((it.call(&f, vec![x.clone()], line)?, x.clone()));
    }
    sort_values(items, "sort_by", line)
}

fn reverse(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::List(l) => Ok(Value::list(l.iter().rev().cloned().collect())),
        Value::Str(s) => Ok(Value::str(s.chars().rev().collect::<String>())),
        other => fail(line, format!("`reverse` expects a list or str, got {}", other.type_name())),
    }
}

fn first(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(l_arg(&args, 0, "first", line)?.first().cloned().unwrap_or(Value::Nil))
}

fn last(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(l_arg(&args, 0, "last", line)?.last().cloned().unwrap_or(Value::Nil))
}

fn push(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut args = args;
    let item = if args.len() > 1 { args.remove(1) } else { Value::Nil };
    let l = l_arg(&args, 0, "push", line)?;
    drop(args);
    let mut v = Arc::unwrap_or_clone(l);
    v.push(item);
    Ok(Value::list(v))
}

fn range(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = if args.len() > 1 {
        (i_arg(&args, 0, "range", line)?, i_arg(&args, 1, "range", line)?)
    } else {
        (0, i_arg(&args, 0, "range", line)?)
    };
    crate::interp::binop(crate::ast::BinOp::Range, Value::Int(a), Value::Int(b), line)
}

fn enumerate(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "enumerate", line)?;
    Ok(Value::list(l.iter().enumerate().map(|(i, v)| Value::list(vec![Value::Int(i as i64), v.clone()])).collect()))
}

// ---- maps --------------------------------------------------------------

fn keys(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::list(m_arg(&args, 0, "keys", line)?.keys().map(Value::str).collect()))
}

fn values(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::list(m_arg(&args, 0, "values", line)?.values().cloned().collect()))
}

fn get(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = m_arg(&args, 0, "get", line)?;
    let k = arg(&args, 1).display();
    Ok(m.get(&k).cloned().unwrap_or_else(|| arg(&args, 2)))
}

fn has(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(m_arg(&args, 0, "has", line)?.contains_key(&arg(&args, 1).display())))
}

fn set(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = m_arg(&args, 0, "set", line)?;
    let mut m = Arc::unwrap_or_clone(m);
    m.insert(arg(&args, 1).display(), arg(&args, 2));
    Ok(Value::map(m))
}

fn remove(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::List(l) => {
            let x = arg(&args, 1);
            Ok(Value::list(l.iter().filter(|v| !equal(v, &x)).cloned().collect()))
        }
        _ => {
            let mut m = Arc::unwrap_or_clone(m_arg(&args, 0, "remove", line)?);
            m.shift_remove(&arg(&args, 1).display());
            Ok(Value::map(m))
        }
    }
}

fn merge(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut m = Arc::unwrap_or_clone(m_arg(&args, 0, "merge", line)?);
    for (k, v) in m_arg(&args, 1, "merge", line)?.iter() {
        m.insert(k.clone(), v.clone());
    }
    Ok(Value::map(m))
}

// ---- errors ------------------------------------------------------------

fn err(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::err(arg(&args, 0)))
}

fn is_err(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::Bool(matches!(arg(&args, 0), Value::Err(_))))
}

fn unwrap(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::Err(e) => fail(line, format!("unwrap of an error: {}", e.display())),
        v => Ok(v),
    }
}

// ---- web ---------------------------------------------------------------

fn raw(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::html(arg(&args, 0).display()))
}

fn escape_fn(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::str(escape(&arg(&args, 0).display())))
}

fn env_var(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = s_arg(&args, 0, "env", line)?;
    Ok(std::env::var(&*name).map(Value::str).unwrap_or_else(|_| arg(&args, 1)))
}

fn now(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(Value::Int(ms))
}

/// `sleep(ms)`: waits without blocking other requests.
fn sleep(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let ms = match arg(&args, 0) {
        Value::Int(n) if n >= 0 => n as u64,
        Value::Float(f) if f >= 0.0 => f as u64,
        other => return fail(line, format!("`sleep` expects milliseconds (a number >= 0), got {}", short_type(&other))),
    };
    it.unlocked(|| std::thread::sleep(std::time::Duration::from_millis(ms)));
    Ok(Value::Nil)
}

fn short_type(v: &Value) -> &'static str {
    v.type_name()
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn not_found() -> Value {
    Value::Response(Arc::new(Response { status: 404, headers: Vec::new(), body: Body::Value(Value::str("not found")) }))
}

/// `file("public/app.css")`: serves a file relative to the program's folder.
/// Paths containing `..` or starting at `/` are refused with a 404.
fn file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "file", line)?;
    let rel = Path::new(&*p);
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Ok(not_found());
    }
    let full = it.base_dir.join(rel);
    if !full.is_file() {
        return Ok(not_found());
    }
    match std::fs::read(&full) {
        Ok(bytes) => Ok(Value::Response(Arc::new(Response {
            status: 200,
            headers: vec![("content-type".into(), mime(&full).into())],
            body: Body::Bytes(bytes),
        }))),
        Err(_) => Ok(not_found()),
    }
}

/// `redirect(url, status = 303)`. 303 makes browsers follow with a GET,
/// which is what you want after a form POST.
fn redirect(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let url = s_arg(&args, 0, "redirect", line)?;
    let status = if args.len() > 1 { i_arg(&args, 1, "redirect", line)? as u16 } else { 303 };
    Ok(Value::Response(Arc::new(Response {
        status,
        headers: vec![("location".into(), url.to_string())],
        body: Body::Value(Value::Nil),
    })))
}

/// `respond(status, body, headers = {})` builds a response value.
fn respond(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let status = i_arg(&args, 0, "respond", line)?;
    if !(100..=599).contains(&status) {
        return fail(line, format!("`respond` status must be 100-599, got {}", status));
    }
    let headers = m_arg(&args, 2, "respond", line)?;
    Ok(Value::Response(Arc::new(Response {
        status: status as u16,
        headers: headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.display())).collect(),
        body: Body::Value(arg(&args, 1)),
    })))
}

fn json_parse(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "json.parse", line)?;
    Ok(match serde_json::from_str::<serde_json::Value>(&s) {
        Ok(j) => Value::from_json(&j),
        Err(e) => Value::err(Value::str(format!("invalid JSON: {}", e))),
    })
}

fn json_stringify(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    let j = arg(&args, 0).to_json();
    let pretty = arg(&args, 1).truthy();
    let s = if pretty { serde_json::to_string_pretty(&j) } else { serde_json::to_string(&j) };
    Ok(Value::str(s.unwrap_or_default()))
}

// ---- files -------------------------------------------------------------

/// A path under the program's folder; `..` and absolute paths are refused.
fn local_path(it: &Interp, p: &str) -> Option<std::path::PathBuf> {
    let rel = Path::new(p);
    if p.is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    Some(it.base_dir.join(rel))
}

/// `read_file(path)`: the file's text, or an error value.
fn read_file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "read_file", line)?;
    let Some(full) = local_path(it, &p) else {
        return Ok(Value::err(Value::str(format!("read_file: path must stay inside the program's folder: {}", p))));
    };
    Ok(match std::fs::read(&full) {
        Ok(b) => Value::str(String::from_utf8_lossy(&b)),
        Err(e) => Value::err(Value::str(format!("read_file: {}: {}", p, e))),
    })
}

/// `write_file(path, text)`: null, or an error value. Creates missing folders.
fn write_file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "write_file", line)?;
    let text = arg(&args, 1).display();
    let Some(full) = local_path(it, &p) else {
        return Ok(Value::err(Value::str(format!("write_file: path must stay inside the program's folder: {}", p))));
    };
    if let Some(dir) = full.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    Ok(match std::fs::write(&full, text) {
        Ok(()) => Value::Nil,
        Err(e) => Value::err(Value::str(format!("write_file: {}: {}", p, e))),
    })
}

// ---- randomness --------------------------------------------------------

/// `random()`: a float in [0, 1).
fn random(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::Float(fastrand::f64()))
}

/// `random_int(lo, hi)`: an int from lo to hi, both included.
fn random_int(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let lo = i_arg(&args, 0, "random_int", line)?;
    let hi = i_arg(&args, 1, "random_int", line)?;
    if lo > hi {
        return fail(line, format!("`random_int` needs lo <= hi, got {} and {}", lo, hi));
    }
    Ok(Value::Int(fastrand::i64(lo..=hi)))
}

/// `uuid()`: a random (version 4) UUID string.
fn uuid(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let mut b = [0u8; 16];
    for x in b.iter_mut() {
        *x = fastrand::u8(..);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    Ok(Value::str(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])))
}

// ---- math --------------------------------------------------------------

fn num_arg(args: &[Value], i: usize, f: &str, line: u32) -> Result<f64, Ctrl> {
    want(args, i, f, "a number", line, |v| v.as_f64())
}

fn abs(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::Int(n) => n.checked_abs().map(Value::Int).ok_or_else(|| Ctrl::Error(crate::QuilloError::new("integer overflow", line))),
        _ => Ok(Value::Float(num_arg(&args, 0, "abs", line)?.abs())),
    }
}

fn to_int_or_float(f: f64) -> Value {
    if f.is_finite() && f.abs() < 9.0e15 {
        Value::Int(f as i64)
    } else {
        Value::Float(f)
    }
}

/// `round(x)` gives an int; `round(x, digits)` a float with that many decimals.
fn round(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = num_arg(&args, 0, "round", line)?;
    if args.len() > 1 {
        let d = i_arg(&args, 1, "round", line)?.clamp(0, 15) as i32;
        let p = 10f64.powi(d);
        return Ok(Value::Float((x * p).round() / p));
    }
    Ok(to_int_or_float(x.round()))
}

fn floor(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(to_int_or_float(num_arg(&args, 0, "floor", line)?.floor()))
}

fn ceil(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(to_int_or_float(num_arg(&args, 0, "ceil", line)?.ceil()))
}

fn sqrt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(num_arg(&args, 0, "sqrt", line)?.sqrt()))
}

/// `pow(x, y)`: an int when both are ints and y >= 0 (and it fits), else a float.
fn pow(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if let (Value::Int(x), Value::Int(y)) = (arg(&args, 0), arg(&args, 1)) {
        if let Some(r) = u32::try_from(y).ok().and_then(|y| x.checked_pow(y)) {
            return Ok(Value::Int(r));
        }
    }
    Ok(Value::Float(num_arg(&args, 0, "pow", line)?.powf(num_arg(&args, 1, "pow", line)?)))
}

// ---- more lists --------------------------------------------------------

fn unique(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "unique", line)?;
    let mut out: Vec<Value> = Vec::new();
    for x in l.iter() {
        if !out.iter().any(|y| equal(x, y)) {
            out.push(x.clone());
        }
    }
    Ok(Value::list(out))
}

/// `flatten([[1], [2, 3], 4])` is `[1, 2, 3, 4]` (one level).
fn flatten(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "flatten", line)?;
    let mut out = Vec::new();
    for x in l.iter() {
        match x {
            Value::List(inner) => out.extend(inner.iter().cloned()),
            other => out.push(other.clone()),
        }
    }
    Ok(Value::list(out))
}

fn zip(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let a = l_arg(&args, 0, "zip", line)?;
    let b = l_arg(&args, 1, "zip", line)?;
    Ok(Value::list(a.iter().zip(b.iter()).map(|(x, y)| Value::list(vec![x.clone(), y.clone()])).collect()))
}

fn take(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = i_arg(&args, 1, "take", line)?.max(0) as usize;
    match arg(&args, 0) {
        Value::Str(s) => Ok(Value::str(s.chars().take(n).collect::<String>())),
        _ => Ok(Value::list(l_arg(&args, 0, "take", line)?.iter().take(n).cloned().collect())),
    }
}

fn drop_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = i_arg(&args, 1, "drop", line)?.max(0) as usize;
    match arg(&args, 0) {
        Value::Str(s) => Ok(Value::str(s.chars().skip(n).collect::<String>())),
        _ => Ok(Value::list(l_arg(&args, 0, "drop", line)?.iter().skip(n).cloned().collect())),
    }
}

/// `count(xs)` is `len(xs)`; `count(xs, f)` counts items where `f` is true.
fn count(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "count", line)?;
    if args.len() < 2 {
        return Ok(Value::Int(l.len() as i64));
    }
    let f = f_arg(&args, 1, "count", line)?;
    let mut n = 0;
    for x in l.iter() {
        if it.call(&f, vec![x.clone()], line)?.truthy() {
            n += 1;
        }
    }
    Ok(Value::Int(n))
}

/// `group_by(xs, f)`: a map from each key `f` gives to the items with that key.
fn group_by(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "group_by", line)?;
    let f = f_arg(&args, 1, "group_by", line)?;
    let mut groups: IndexMap<String, Vec<Value>> = IndexMap::new();
    for x in l.iter() {
        let k = it.call(&f, vec![x.clone()], line)?.display();
        groups.entry(k).or_default().push(x.clone());
    }
    Ok(Value::map(groups.into_iter().map(|(k, v)| (k, Value::list(v))).collect()))
}

/// `index_of(xs, x)`: the position of `x` in a list or str, or null.
fn index_of(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let x = arg(&args, 1);
    Ok(match arg(&args, 0) {
        Value::List(l) => l.iter().position(|v| equal(v, &x)).map_or(Value::Nil, |i| Value::Int(i as i64)),
        Value::Str(s) => {
            let needle = x.display();
            s.find(&*needle).map_or(Value::Nil, |b| Value::Int(s[..b].chars().count() as i64))
        }
        other => return fail(line, format!("`index_of` expects a list or str, got {}", other.type_name())),
    })
}

fn repeat(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "repeat", line)?;
    let n = i_arg(&args, 1, "repeat", line)?;
    if n < 0 || (s.len() as i64).saturating_mul(n) > 100_000_000 {
        return fail(line, "`repeat` count is negative or too large");
    }
    Ok(Value::str(s.repeat(n as usize)))
}

// ---- live updates ------------------------------------------------------

/// `~items = live(load_items, ...args)` in a component: calls the server fn
/// now, and the browser calls it again whenever something it read changes.
fn live(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = match args.first() {
        Some(Value::Func(c)) if c.def.server => Value::Func(c.clone()),
        Some(other) => {
            return fail(line, format!("`live` needs a server fn to call, like live(load_notes), got {}", other.type_name()))
        }
        None => return fail(line, "`live` needs a server fn to call, like live(load_notes)"),
    };
    let Value::Func(c) = &f else { unreachable!() };
    let name = c.def.name.clone();
    let rest: Vec<Value> = args[1..].to_vec();
    let outer = it.reads.replace(std::collections::HashSet::new());
    let r = it.call(&f, rest.clone(), line);
    let topics = std::mem::replace(&mut it.reads, outer).unwrap_or_default();
    if let Some(o) = &mut it.reads {
        o.extend(topics.iter().cloned());
    }
    let mut topics: Vec<String> = topics.into_iter().collect();
    topics.sort();
    let mut info = Map::new();
    info.insert("f".into(), Value::str(&name));
    info.insert("a".into(), Value::list(rest));
    info.insert("t".into(), Value::list(topics.into_iter().map(Value::str).collect()));
    it.last_live = Some(Value::map(info));
    r
}

/// `publish("topic", ...)`: wakes pages whose `live` data watched the topic.
fn publish(it: &mut Interp, args: Vec<Value>, _: u32) -> R {
    it.publish(args.iter().map(|a| a.display()).collect());
    Ok(Value::Nil)
}

/// `watch("topic", ...)`: inside a `live` loader, also refresh on `publish("topic")`.
fn watch(it: &mut Interp, args: Vec<Value>, _: u32) -> R {
    for a in &args {
        it.watch(a.display());
    }
    Ok(Value::Nil)
}
