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
        ("allow_cross_site", allow_cross_site),
        ("save_upload", save_upload),
        ("read_upload", read_upload),
        ("tenant", tenant),
        ("tenant_from", tenant_from),
        ("with_tenant", with_tenant),
        ("__service", crate::service::call),
        ("__service_later", crate::service::later),
        ("__record", crate::record::construct),
        ("redirect", redirect),
        ("respond", respond),
        ("read_file", read_file),
        ("write_file", write_file),
        ("delete_file", delete_file),
        ("check_program", check_program),
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
        ("join_on", join_on),
        ("index_of", index_of),
        ("repeat", repeat),
        ("markdown", markdown),
        ("highlight", highlight),
        ("run_script", run_script),
        ("ask", ask),
        ("notify", ask),
        ("live", live),
        ("parallel", parallel),
        ("every", every),
        ("js", js),
        ("js_new", js),
        ("css", tag_text),
        ("sql", tag_text),
        ("sql_name", sql_name),
        ("sql_param", sql_param),
        ("url", url),
        ("navigate", navigate),
        ("publish", publish),
        ("watch", watch),
        ("redact", crate::redact::redact),
    ];
    for (name, f) in fns.iter().chain(crate::session::BUILTINS).chain(crate::i18n::BUILTINS).chain(crate::jobs::BUILTINS).chain(crate::stream::BUILTINS) {
        define(env, name, Value::Builtin(name, *f));
    }
    let mut json = Map::new();
    json.insert("parse".into(), Value::Builtin("json.parse", json_parse));
    json.insert("stringify".into(), Value::Builtin("json.stringify", json_stringify));
    define(env, "json", Value::map(json));
    let mut db = Map::new();
    crate::db::install(&mut db);
    define(env, "db", Value::map(db));
    let mut audit = Map::new();
    crate::audit::install(&mut audit);
    define(env, "audit", Value::map(audit));
    let mut http = Map::new();
    crate::fetch::install(&mut http);
    define(env, "http", Value::map(http));
    let mut udp = Map::new();
    crate::udp::install(&mut udp);
    define(env, "udp", Value::map(udp));
    let mut channel = Map::new();
    crate::wt::install(&mut channel);
    define(env, "channel", Value::map(channel));
    let mut media = Map::new();
    crate::media::install(&mut media);
    define(env, "media", Value::map(media));
    let mut crypto = Map::new();
    crate::crypto::install(&mut crypto);
    crate::cipher::install(&mut crypto);
    define(env, "crypto", Value::map(crypto));
    let mut image = Map::new();
    crate::imaging::install(&mut image);
    define(env, "image", Value::map(image));
    let mut math = Map::new();
    crate::numeric::install_math(&mut math);
    define(env, "math", Value::map(math));
    let mut stats = Map::new();
    crate::numeric::install_stats(&mut stats);
    define(env, "stats", Value::map(stats));
    let mut vec = Map::new();
    crate::numeric::install_vec(&mut vec);
    define(env, "vec", Value::map(vec));
    let mut date = Map::new();
    crate::dates::install(&mut date);
    define(env, "date", Value::map(date));
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
    // Values under sensitive keys, card numbers and IBANs are masked (see `redact`).
    let line = args.iter().map(|v| crate::redact::value(v).display()).collect::<Vec<_>>().join(" ");
    println!("{}", crate::redact::text(&line));
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
        out.push(it.call1(&f, x.clone(), line)?);
    }
    Ok(Value::list(out))
}

fn filter(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "filter", line)?;
    let f = f_arg(&args, 1, "filter", line)?;
    let mut out = Vec::new();
    for x in l.iter() {
        if it.call1(&f, x.clone(), line)?.truthy() {
            out.push(x.clone());
        }
    }
    Ok(Value::list(out))
}

fn find(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "find", line)?;
    let f = f_arg(&args, 1, "find", line)?;
    for x in l.iter() {
        if it.call1(&f, x.clone(), line)?.truthy() {
            return Ok(x.clone());
        }
    }
    Ok(Value::Nil)
}

fn any(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "any", line)?;
    let f = f_arg(&args, 1, "any", line)?;
    for x in l.iter() {
        if it.call1(&f, x.clone(), line)?.truthy() {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

fn all(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "all", line)?;
    let f = f_arg(&args, 1, "all", line)?;
    for x in l.iter() {
        if !it.call1(&f, x.clone(), line)?.truthy() {
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
        // Numbers as `+` adds them, without its general case for each.
        let next = match (&acc, x) {
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a + b)),
            (Value::Int(a), Value::Int(b)) => a.checked_add(*b).map(Value::Int),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a + *b as f64)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 + b)),
            _ => None,
        };
        acc = match next {
            Some(v) => v,
            None => crate::interp::binop(crate::ast::BinOp::Add, acc, x.clone(), line)?,
        };
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
    // Numbers, compared as numbers, on all the cores (the order is the same: stable).
    if let Some(v) = crate::numeric::sort_numbers(&l) {
        return Ok(v);
    }
    sort_values(l.iter().map(|v| (v.clone(), v.clone())).collect(), "sort", line)
}

fn sort_by(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "sort_by", line)?;
    let f = f_arg(&args, 1, "sort_by", line)?;
    let mut items = Vec::with_capacity(l.len());
    for x in l.iter() {
        items.push((it.call1(&f, x.clone(), line)?, x.clone()));
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
    Ok(m.get(k.as_str()).cloned().unwrap_or_else(|| arg(&args, 2)))
}

fn has(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Bool(m_arg(&args, 0, "has", line)?.contains_key(arg(&args, 1).display().as_str())))
}

fn set(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = m_arg(&args, 0, "set", line)?;
    let mut m = Arc::unwrap_or_clone(m);
    m.insert(arg(&args, 1).display().into(), arg(&args, 2));
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
            m.shift_remove(arg(&args, 1).display().as_str());
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
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn not_found() -> Value {
    Value::Response(Arc::new(Response { status: 404, headers: Vec::new(), body: Body::Value(Value::str("not found")) }))
}

/// `file("public/app.css")`: serves a file relative to the program's folder
/// (see `statics`): a browser whose copy is current gets a bodiless 304, a
/// byte range gets a 206 (audio, video), and small files come from memory
/// while unchanged. Browsers check back each time; `file(path, { cache:
/// "1y" })` lets them keep it that long without asking, for names that
/// change with their content. Paths containing `..` or starting at `/` are
/// refused with a 404.
fn file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "file", line)?;
    let rel = Path::new(&*p);
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Ok(not_found());
    }
    let full = it.base_dir.join(rel);
    let Ok(info) = crate::statics::info(&full) else { return Ok(not_found()) };
    let keep = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(Value::Map(m)) => match m.get("cache") {
            None | Some(Value::Nil) => None,
            Some(v) => match duration_seconds(v) {
                Some(s) => Some(s),
                None => return fail(line, format!("`file`'s cache is seconds or a duration like \"30d\", got {}", v.repr())),
            },
        },
        Some(other) => return fail(line, format!("`file` expects options as a map, got {}", other.type_name())),
    };
    let cache_control = match keep {
        Some(s) => format!("public, max-age={}, immutable", s),
        None => "no-cache".into(),
    };
    let mut headers: Vec<(String, String)> = vec![
        ("etag".into(), info.etag.clone()),
        ("last-modified".into(), info.last_modified.clone()),
        ("cache-control".into(), cache_control),
    ];
    let answer = |status: u16, headers: Vec<(String, String)>, body: Vec<u8>| {
        Ok(Value::Response(Arc::new(Response { status, headers, body: Body::Bytes(body) })))
    };
    // What the browser already has, or wants a part of.
    let asked = |name: &str, it: &mut Interp| -> Option<String> {
        let Some(Value::Map(req)) = it.request_value() else { return None };
        let Some(Value::Map(h)) = req.get("headers") else { return None };
        h.get(name).map(|v| v.display())
    };
    let fresh = match asked("if-none-match", it) {
        Some(v) => crate::statics::matches(&v, &info.etag),
        None => asked("if-modified-since", it).map_or(false, |v| v == info.last_modified),
    };
    if fresh {
        return answer(304, headers, Vec::new());
    }
    headers.push(("content-type".into(), mime(&full).into()));
    headers.push(("accept-ranges".into(), "bytes".into()));
    // Larger files are sent from disk as they are written (see `statics`).
    let large = info.len > 1 << 20;
    if let Some(r) = asked("range", it) {
        let same = asked("if-range", it).map_or(true, |v| v == info.etag || v == info.last_modified);
        match crate::statics::range(&r, info.len).filter(|_| same) {
            Some(Ok((start, end))) if large => {
                headers.push(("content-range".into(), format!("bytes {}-{}/{}", start, end, info.len)));
                headers.push((crate::statics::SEND_HEADER.into(), crate::statics::mark(&full, start, end - start + 1)));
                return answer(206, headers, Vec::new());
            }
            Some(Ok((start, end))) => {
                let from = full.clone();
                return match it.unlocked(move || crate::statics::read_range(&from, start, end)) {
                    Ok(bytes) => {
                        headers.push(("content-range".into(), format!("bytes {}-{}/{}", start, end, info.len)));
                        answer(206, headers, bytes)
                    }
                    Err(_) => Ok(not_found()),
                };
            }
            Some(Err(())) => {
                headers.push(("content-range".into(), format!("bytes */{}", info.len)));
                return answer(416, headers, Vec::new());
            }
            None => {}
        }
    }
    if large {
        headers.push((crate::statics::SEND_HEADER.into(), crate::statics::mark(&full, 0, info.len)));
        return answer(200, headers, Vec::new());
    }
    // Small files are kept in memory.
    match crate::statics::read(&full, &info) {
        Ok(b) => answer(200, headers, b.to_vec()),
        Err(_) => Ok(not_found()),
    }
}

/// Seconds from `3600` or `"1h"` (s, m, h, d, w, y).
pub(crate) fn duration_seconds(v: &Value) -> Option<u64> {
    match v {
        Value::Int(n) if *n >= 0 => Some(*n as u64),
        Value::Str(s) => {
            let s = s.trim();
            let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
            let n: u64 = n.parse().ok()?;
            let per = match unit {
                "" | "s" => 1,
                "m" => 60,
                "h" => 3600,
                "d" => 86400,
                "w" => 7 * 86400,
                "y" => 365 * 86400,
                _ => return None,
            };
            Some(n * per)
        }
        _ => None,
    }
}

/// `save_upload(file, "uploads/avatar.png")`: writes a file from
/// `request.files` into the program's folder (folders made as needed) and
/// gives the path back; an error value if it cannot.
fn save_upload(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let dest = s_arg(&args, 1, "save_upload", line)?;
    let rel = Path::new(&*dest);
    if dest.is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return fail(line, format!("`save_upload` needs a path inside the program's folder, got {:?}", &*dest));
    }
    let Some(body) = it.upload_body.clone() else {
        return Ok(Value::err(Value::str("save_upload: the current request has no files")));
    };
    let range = match crate::upload::bytes(&arg(&args, 0), &body) {
        Ok(b) => (b.as_ptr() as usize - body.as_ptr() as usize, b.len()),
        Err(e) => return Ok(Value::err(Value::str(format!("save_upload: {}", e)))),
    };
    let full = it.base_dir.join(rel);
    let written = it.unlocked(move || {
        if let Some(dir) = full.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&full, &body[range.0..range.0 + range.1])
    });
    match written {
        Ok(()) => Ok(Value::Str(dest)),
        Err(e) => Ok(Value::err(Value::str(format!("save_upload: {}", e)))),
    }
}

/// `read_upload(file)`: a file from `request.files`, as text.
fn read_upload(it: &mut Interp, args: Vec<Value>, _: u32) -> R {
    let Some(body) = it.upload_body.clone() else {
        return Ok(Value::err(Value::str("read_upload: the current request has no files")));
    };
    match crate::upload::bytes(&arg(&args, 0), &body) {
        Ok(b) => Ok(Value::str(String::from_utf8_lossy(b))),
        Err(e) => Ok(Value::err(Value::str(format!("read_upload: {}", e)))),
    }
}

/// `allow_cross_site("/payments/return")`, at the top of a program: routes
/// under the path take form posts from other sites' pages (a payment
/// provider's return, a single sign-on callback), which are otherwise
/// refused.
fn allow_cross_site(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "allow_cross_site", line)?;
    if !p.starts_with('/') {
        return fail(line, format!("`allow_cross_site` needs a path that starts with /, got {:?}", &*p));
    }
    Arc::make_mut(&mut it.site).cross_site_ok.push(p.to_string());
    Ok(Value::Nil)
}

/// `tenant()`: the tenant the code runs for (see `tenant_from`), or null.
fn tenant(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(it.tenant().map_or(Value::Nil, |t| Value::Str(t)))
}

/// `tenant_from("example.com")`, at the top of a program: each request's
/// tenant is the part of its host before the domain (`acme.example.com` is
/// `acme`). `tenant_from((request) => ...)`: what the function gives for the
/// request, a name or null. Names are lowercase letters, digits, `-` and
/// `_`; anything else is no tenant.
fn tenant_from(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let rule = match args.first() {
        Some(Value::Str(d)) => {
            let d = d.trim_start_matches('.').to_ascii_lowercase();
            if d.is_empty() || d.contains('/') || d.contains(':') {
                return fail(line, format!("`tenant_from` needs a domain such as \"example.com\", got {:?}", d));
            }
            crate::site::TenantRule::Subdomain(d)
        }
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => crate::site::TenantRule::Function(f.clone()),
        other => {
            return fail(line, format!("`tenant_from` expects a domain or a function of the request, got {}", other.map_or("nothing", |v| v.type_name())))
        }
    };
    Arc::make_mut(&mut it.site).tenant_rule = Some(rule);
    Ok(Value::Nil)
}

/// `with_tenant("acme", () => ...)`: runs the function for that tenant (a
/// scheduled job, a script), with its database and live updates.
fn with_tenant(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = s_arg(&args, 0, "with_tenant", line)?;
    if !crate::site::tenant_name_ok(&name) {
        return fail(line, format!("`with_tenant`: {:?} cannot be a tenant's name (lowercase letters, digits, - and _)", &*name));
    }
    let f = match args.get(1) {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => f.clone(),
        other => return fail(line, format!("`with_tenant` expects a function second, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let outer = it.tenant.replace(Some(Arc::from(&*name)));
    let named = std::mem::replace(&mut it.tenant_named, true);
    let r = it.call(&f, Vec::new(), line);
    it.tenant = outer;
    it.tenant_named = named;
    r
}

/// `redirect(url, status = 303)`. 303 makes browsers follow with a GET,
/// which is what you want after a form POST.
fn redirect(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let url = s_arg(&args, 0, "redirect", line)?;
    let status = if args.len() > 1 { i_arg(&args, 1, "redirect", line)? } else { 303 };
    if !(300..=399).contains(&status) {
        return fail(line, format!("`redirect` status must be 300-399, got {}", status));
    }
    let status = status as u16;
    if !crate::server::valid_header("location", &url) {
        return fail(line, "`redirect` URL cannot contain line breaks or other control characters");
    }
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
    let headers = if args.len() > 2 { m_arg(&args, 2, "respond", line)? } else { Arc::new(Map::new()) };
    let headers: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.display())).collect();
    // Header text often comes from the request; a line break in it would
    // let the sender add headers (or a whole response) of their own.
    if let Some((k, _)) = headers.iter().find(|(k, v)| !crate::server::valid_header(k, v)) {
        return fail(
            line,
            format!("`respond` header `{}`: names are letters, digits and -, values cannot contain line breaks", k.escape_debug()),
        );
    }
    Ok(Value::Response(Arc::new(Response {
        status: status as u16,
        headers,
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
pub(crate) fn local_path(it: &Interp, p: &str) -> Option<std::path::PathBuf> {
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

/// `check_program("main.qwe")`: what `quillo check` says of a program (and
/// the files it uses), as `{ file, line, severity, message }` maps; for
/// tools that write Quillo code, such as a plugin editor.
fn check_program(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "check_program", line)?;
    let Some(full) = local_path(it, &p) else {
        return Ok(Value::err(Value::str(format!("check_program: path must stay inside the program's folder: {}", p))));
    };
    // `{ "path": "text" }`: check as if those files said that.
    let mut overrides = std::collections::HashMap::new();
    if let Some(Value::Map(m)) = args.get(1) {
        for (k, v) in m.iter() {
            match local_path(it, k) {
                Some(p) => {
                    overrides.insert(p, v.display().to_string());
                }
                None => return Ok(Value::err(Value::str(format!("check_program: path must stay inside the program's folder: {}", k)))),
            }
        }
    }
    let base = it.base_dir.to_string_lossy().into_owned();
    // The code may come from the program's users: it reads nothing outside the folder.
    let opts = crate::check::Options { confined: true, ..Default::default() };
    let found = it.unlocked(move || crate::check::check_file_opts(&full, overrides, opts).diagnostics);
    Ok(Value::list(
        found
            .into_iter()
            .map(|d| {
                let mut m = Map::new();
                let file = d.file.strip_prefix(&base).unwrap_or(&d.file).trim_start_matches('/').trim_start_matches("./").to_string();
                m.insert("file".into(), Value::str(file));
                m.insert("line".into(), Value::Int(d.line as i64));
                m.insert("severity".into(), Value::str(if d.severity == crate::check::Severity::Error { "error" } else { "warning" }));
                m.insert("message".into(), Value::str(d.message));
                Value::map(m)
            })
            .collect(),
    ))
}

/// `delete_file(path)`: removes a file, or a folder and what is in it,
/// inside the program's folder (never the folder itself). Nothing there is
/// no error.
fn delete_file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "delete_file", line)?;
    let Some(full) = local_path(it, &p).filter(|f| f.as_path() != &*it.base_dir) else {
        return Ok(Value::err(Value::str(format!("delete_file: path must be inside the program's folder: {}", p))));
    };
    let r = if full.is_dir() { std::fs::remove_dir_all(&full) } else { std::fs::remove_file(&full) };
    Ok(match r {
        Ok(()) => Value::Nil,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Nil,
        Err(e) => Value::err(Value::str(format!("delete_file: {}: {}", p, e))),
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

/// `uuid()`: a random (version 4) UUID string. The bytes come from the
/// operating system, so a uuid can serve as a session id or token, which
/// `random()` and `random_int()` cannot.
fn uuid(_: &mut Interp, _: Vec<Value>, line: u32) -> R {
    let mut b = [0u8; 16];
    if getrandom::getrandom(&mut b).is_err() {
        return fail(line, "`uuid`: the operating system gave no random bytes");
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
        if it.call1(&f, x.clone(), line)?.truthy() {
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
        let k = it.call1(&f, x.clone(), line)?.display();
        groups.entry(k).or_default().push(x.clone());
    }
    Ok(Value::map(groups.into_iter().map(|(k, v)| (k.into(), Value::list(v))).collect()))
}

/// `join_on(rows, others, key, other_key = key, as)`: each row with its
/// match among `others` (the first whose `other_key` equals the row's
/// `key`, value and type) under `as`, or null: rows from two databases put
/// together, as no single SQL query can. `as` is `key` without `_id`
/// (`user_id` gives `user`), else "joined".
fn join_on(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let rows = l_arg(&args, 0, "join_on", line)?;
    let others = l_arg(&args, 1, "join_on", line)?;
    let key = s_arg(&args, 2, "join_on", line)?;
    let other_key: Arc<str> = match arg(&args, 3) {
        Value::Nil => key.clone(),
        Value::Str(s) => s,
        other => return fail(line, format!("`join_on`'s fourth argument is the others' key (a str), got {}", other.type_name())),
    };
    let name: String = match arg(&args, 4) {
        Value::Nil => key.strip_suffix("_id").filter(|s| !s.is_empty()).unwrap_or("joined").to_string(),
        Value::Str(s) => s.to_string(),
        other => return fail(line, format!("`join_on`'s fifth argument is the field to put the match in (a str), got {}", other.type_name())),
    };
    let mut index: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for o in others.iter() {
        if let Value::Map(m) = o {
            if let Some(k) = m.get(&*other_key) {
                index.entry(k.repr()).or_insert_with(|| o.clone());
            }
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    for r in rows.iter() {
        let Value::Map(m) = r else {
            return fail(line, format!("`join_on` joins maps (rows), got {}", r.type_name()));
        };
        let found = m.get(&*key).and_then(|k| index.get(&k.repr())).cloned().unwrap_or(Value::Nil);
        let mut m = (**m).clone();
        m.insert(name.as_str().into(), found);
        out.push(Value::map(m));
    }
    Ok(Value::list(out))
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

/// `markdown(text)`: Markdown as HTML (CommonMark, with tables,
/// strikethrough and task lists). HTML written in the text is shown as text,
/// and `javascript:` links are blocked, so it is safe for what visitors
/// write; `markdown(text, { html: true })` keeps the HTML, for text you
/// wrote yourself.
fn markdown(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};
    let s = s_arg(&args, 0, "markdown", line)?;
    // `{ html: true }` keeps the HTML in the text; `{ inline: true }` keeps
    // only what is inside a line (bold, italic, code, links): no paragraphs,
    // headings or lists, lines broken by <br>, a list's items with their
    // marks as text ("• ", "1. ") (a headline, a caption).
    let (html_ok, inline) = match args.get(1) {
        None | Some(Value::Nil) => (false, false),
        Some(Value::Map(m)) => (m.get("html").map_or(false, |v| v.truthy()), m.get("inline").map_or(false, |v| v.truthy())),
        Some(v) => return fail(line, format!("`markdown` expects options as a map, got {}", v.type_name())),
    };
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let safe = |u: CowStr<'static>| if crate::interp::script_url(&u) { CowStr::Borrowed(crate::interp::BLOCKED_URL) } else { u };
    let events = Parser::new_ext(&s, opts).map(|e| match e {
        Event::Html(h) | Event::InlineHtml(h) if !html_ok => Event::Text(h),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
            Event::Start(Tag::Link { link_type, dest_url: safe(dest_url.into_static()), title, id })
        }
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
            Event::Start(Tag::Image { link_type, dest_url: safe(dest_url.into_static()), title, id })
        }
        e => e,
    });
    // A fenced code block that names its language comes out highlighted
    // (after the HTML in the text was made text, above).
    let mut code: Option<(String, String)> = None;
    let events = events.filter_map(move |e| match (e, code.as_mut()) {
        (Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(lang))), None) if !lang.trim().is_empty() => {
            code = Some((lang.split_whitespace().next().unwrap_or("").to_string(), String::new()));
            None
        }
        (Event::Text(t), Some((_, body))) => {
            body.push_str(&t);
            None
        }
        (Event::End(pulldown_cmark::TagEnd::CodeBlock), Some(_)) => {
            let (lang, body) = code.take().unwrap();
            let class: String = lang.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            Some(Event::Html(CowStr::from(format!("<pre class=\"hl\"><code class=\"language-{}\">{}</code></pre>\n", class, crate::highlight::highlight(&body, &lang)))))
        }
        (e, _) => Some(e),
    });
    let mut lines = 0;
    // The lists being read: the next number of each (None: bullets).
    let mut lists: Vec<Option<u64>> = Vec::new();
    let events = events.flat_map(move |e| {
        use pulldown_cmark::TagEnd;
        if !inline {
            return vec![e];
        }
        let mut line = |out: &mut Vec<Event>| {
            lines += 1;
            if lines > 1 {
                out.push(Event::Html(CowStr::Borrowed("<br>")));
            }
        };
        let mut out = Vec::new();
        match e {
            Event::Start(Tag::Item) => {
                line(&mut out);
                let mark = match lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => "• ".to_string(),
                };
                out.push(Event::Text(CowStr::from(mark)));
            }
            // An item's paragraph (a loose list) starts no line of its own.
            Event::Start(Tag::Paragraph) if !lists.is_empty() => {}
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::CodeBlock(_)) => line(&mut out),
            Event::Start(Tag::List(first)) => lists.push(first),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::BlockQuote(_) | Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell) => {}
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock | TagEnd::BlockQuote(_)
                | TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell) => {}
            Event::Rule | Event::TaskListMarker(_) => {}
            e => out.push(e),
        }
        out
    });
    let mut out = String::with_capacity(s.len() * 3 / 2);
    pulldown_cmark::html::push_html(&mut out, events);
    if inline {
        out = out.trim_end().to_string();
    }
    Ok(Value::html(out))
}

/// `highlight(code, lang)`: code as HTML, coloured by the spans of its
/// keywords, strings, comments and so on (see highlight.rs), escaped.
fn highlight(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let code = s_arg(&args, 0, "highlight", line)?;
    let lang = match args.get(1) {
        None | Some(Value::Nil) => Arc::from(""),
        Some(_) => s_arg(&args, 1, "highlight", line)?,
    };
    Ok(Value::html(crate::highlight::highlight(&code, &lang)))
}

/// `ask` and `notify` are the browser's (a dialog, a note on the page): on
/// the server there is no one to ask.
fn ask(_: &mut Interp, _: Vec<Value>, line: u32) -> R {
    fail(line, "`ask` and `notify` speak to someone in the browser: call them from a component (an event handler)")
}

/// `run_script(source, vars)`: users' code, run in a sandbox (script.rs).
fn run_script(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let src = s_arg(&args, 0, "run_script", line)?;
    let vars = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(_) => Some(m_arg(&args, 1, "run_script", line)?),
    };
    Ok(crate::script::run_script(it, &src, vars.as_deref()))
}

// ---- live updates ------------------------------------------------------

/// `state items = live(load_items, ...args)` in a component: calls the server fn
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
    it.shared.track(&topics);
    let mut info = Map::new();
    info.insert("f".into(), Value::str(&name));
    info.insert("a".into(), Value::list(rest));
    info.insert("t".into(), Value::list(topics.into_iter().map(Value::str).collect()));
    it.last_live = Some(Value::map(info));
    r
}

/// `every(ms, f)` in a component: in the browser, calls `f` every `ms`
/// milliseconds while the component's renders keep asking for it. Rendering
/// on the server starts nothing.
fn every(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match (args.first(), args.get(1)) {
        (Some(Value::Int(_) | Value::Float(_)), Some(Value::Func(_) | Value::Builtin(..))) => Ok(Value::Nil),
        _ => fail(line, "`every` takes milliseconds and a function: every(100, step)"),
    }
}

/// `js("Chart", ...)` and `js_new(...)` in a component call the page's
/// scripts, in the browser; rendering on the server, they give null.
fn js(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match args.first() {
        Some(Value::Str(_)) => Ok(Value::Nil),
        _ => fail(line, "`js` takes the name of what it calls first: js(\"Chart\", ...)"),
    }
}

/// `parallel(f, g, ...)` or `parallel([f, g, ...])`: calls the functions at
/// once; their results, in order.
fn parallel(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let fs = match args.as_slice() {
        [Value::List(l)] => l.to_vec(),
        _ => args,
    };
    it.run_parallel(fs, line)
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

// ---- navigation --------------------------------------------------------

/// `url()`: `{ path, query, hash }` of the page. On the server, the request
/// being handled; in the browser, the address bar (and components that call
/// it re-render when it changes).
fn url(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let current = it.request_value();
    let (path, query) = match &current {
        Some(Value::Map(m)) => (m.get("path").cloned().unwrap_or(Value::str("/")), m.get("query").cloned().unwrap_or(Value::map(Map::new()))),
        _ => (Value::str("/"), Value::map(Map::new())),
    };
    let mut m = Map::new();
    m.insert("path".into(), path);
    m.insert("query".into(), query);
    m.insert("hash".into(), Value::str(""));
    Ok(Value::map(m))
}

/// `navigate(path)`: in the browser, goes to another page without reloading.
/// On the server there is no page to change; use `redirect` there.
fn navigate(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    s_arg(&args, 0, "navigate", line)?;
    Ok(Value::Nil)
}

/// A name a program puts in SQL (a table's, a column's): letters, digits
/// and `_`, not starting with a digit, at most 64 characters. Anything else
/// is refused, so the name cannot carry SQL of its own.
fn sql_ident(v: &Value, f: &str, line: u32) -> Result<String, Ctrl> {
    let Value::Str(s) = v else { return fail(line, format!("`{}` takes a name (a str) or a list of them, got {}", f, v.type_name())) };
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit());
    if !ok {
        return fail(line, format!("`{}`: {:?} is not a name SQL can take (letters, digits and _)", f, &**s));
    }
    Ok(s.to_string())
}

fn sql_idents(args: &[Value], f: &str, line: u32, each: impl Fn(String) -> String) -> R {
    match args.first() {
        Some(Value::List(items)) => {
            let parts: Result<Vec<String>, Ctrl> = items.iter().map(|v| sql_ident(v, f, line).map(&each)).collect();
            Ok(Value::str(&parts?.join(", ")))
        }
        Some(v) => Ok(Value::str(&each(sql_ident(v, f, line)?))),
        None => fail(line, format!("`{}` takes a name", f)),
    }
}

/// `sql_name("o_pitch")`: a table's or a column's name, checked and quoted,
/// for SQL that cannot take it as a `?`; a list gives them comma-separated.
fn sql_name(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    sql_idents(&args, "sql_name", line, |s| format!("\"{}\"", s))
}

/// `sql_param("title")`: a named parameter, `:title`, checked; a list gives
/// them comma-separated (for `insert ... values (...)` with a map).
fn sql_param(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    sql_idents(&args, "sql_param", line, |s| format!(":{}", s))
}

/// `css` and `sql`: css`...` and sql`...` are the text as written; the tag
/// tells editors to highlight it as CSS or SQL.
fn tag_text(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::str(&*s_arg(&args, 0, "css", line)?))
}
