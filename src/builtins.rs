//! The standard library: functions every program can call.

use crate::interp::{escape, fail, Ctrl, Interp};
use crate::value::*;
use indexmap::IndexMap;
use std::path::{Component, Path};
use std::sync::Arc;

type R = Result<Value, Ctrl>;

pub fn install(env: &Env) {
    let fns: &[(&'static str, BuiltinFn)] = &[
        ("print", print),
        ("error", error_of),
        ("run_after", nothing_here),
        ("page_updated", nothing_here),
        ("js_has", no_page),
        ("cursor_index", cursor_index),
        ("args", program_args),
        ("stdin", stdin_text),
        ("read_line", read_line),
        ("exit", exit_with),
        ("str", to_str),
        ("int", to_int),
        ("float", to_float),
        ("dec", to_dec),
        ("format_number", crate::money::format_number_fn),
        ("len", len),
        ("type_of", type_of),
        ("upper", upper),
        ("lower", lower),
        ("normalize", normalize),
        ("trim", trim),
        ("filled", filled),
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
        // `{ ...m, name: v }` (src/parser.rs), which a variable named merge
        // cannot change.
        ("__merge", spread_map),
        ("__css_value", css_value_fn),
        ("__concat", spread_list),
        ("err", err),
        ("is_err", is_err),
        ("unwrap", unwrap),
        ("raw", raw),
        ("escape", escape_fn),
        ("url_encode", url_encode),
        ("env", env_var),
        // Where the program runs: the system, the device, how it runs (branch mobile).
        ("platform", crate::platform::platform),
        ("now", now),
        ("sleep", sleep),
        ("file", file),
        ("files", files),
        ("__built", built),
        ("classes", classes),
        ("accept_calls", accept_calls),
        ("__callers", callers),
        ("allow_cross_site", allow_cross_site),
        ("site_headers", site_headers),
        ("__site", crate::sites::declare),
        ("save_upload", save_upload),
        ("read_upload", read_upload),
        ("tenant", tenant),
        ("tenant_from", tenant_from),
        ("with_tenant", with_tenant),
        ("new_tenant", new_tenant),
        ("__service", crate::service::call),
        ("__service_later", crate::service::later),
        ("__record", crate::record::construct),
        ("redirect", redirect),
        ("local_path", local_path_fn),
        ("respond", respond),
        ("read_file", read_file),
        ("write_file", write_file),
        ("delete_file", delete_file),
        ("check_program", check_program),
        ("random", random),
        ("random_int", random_int),
        ("uuid", uuid),
        ("defer", defer),
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
        ("markdown_page", markdown_page),
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
        ("__style", style_noop),
        ("ready", crate::ready::add_check),
        ("style_with", style_with),
        ("sql", tag_text),
        ("sql_name", sql_name),
        ("sql_param", sql_param),
        ("url", url),
        ("navigate", navigate),
        ("waiting", waiting),
        ("__answer", answer_with),
        ("publish", publish),
        ("watch", watch),
        ("unwatched", unwatched),
    ];
    for (name, f) in fns.iter().chain(crate::session::BUILTINS).chain(crate::i18n::BUILTINS).chain(crate::jobs::BUILTINS).chain(crate::stream::BUILTINS).chain(crate::idempotency::BUILTINS).chain(crate::mcp::BUILTINS) {
        define(env, name, Value::Builtin(name, *f));
    }
    let mut re = Map::new();
    crate::re::install(&mut re);
    define(env, "re", Value::map(re));
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
    define(env, "crypto", Value::map(crypto));
    let mut image = Map::new();
    crate::imaging::install(&mut image);
    define(env, "image", Value::map(image));
    let mut later = Map::new();
    crate::service::install_later(&mut later);
    define(env, "later", Value::map(later));
    let mut money = Map::new();
    crate::money::install(&mut money);
    define(env, "money", Value::map(money));
    let mut math = Map::new();
    crate::numeric::install_math(&mut math);
    define(env, "math", Value::map(math));
    let mut bit = Map::new();
    crate::numeric::install_bit(&mut bit);
    define(env, "bit", Value::map(bit));
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

/// What followed `--` on the command line.
pub static PROGRAM_ARGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// In the browser only: on the server, while a component renders, nothing happens.
fn nothing_here(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::Nil)
}

fn no_page(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::Bool(false))
}

/// `cursor_index(s, n)`: the character a field's cursor at `n` is before.
fn cursor_index(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Value::Str(s) = arg(&args, 0) else { return fail(line, "`cursor_index` expects a str as argument 1") };
    let n = i_arg(&args, 1, "cursor_index", line)?.max(0) as usize;
    let (mut units, mut chars) = (0, 0);
    for c in s.chars() {
        if units >= n {
            break;
        }
        units += c.len_utf16();
        chars += 1;
    }
    Ok(Value::Int(chars))
}

fn program_args(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::list(PROGRAM_ARGS.get().map_or(Vec::new(), |a| a.iter().map(Value::str).collect())))
}

// A script as a command-line tool: what is piped to it, and the status it ends with
// (FEEDBACK29.md, Q-357; branch feedback29-fixes). Not while the program serves, where the
// input is the terminal's and a request is not the one to end the process.

fn for_scripts(name: &str, line: u32) -> Result<(), Ctrl> {
    if crate::value::serving() {
        return fail(line, format!("`{}` is for a script: a program that serves reads requests, and stops when it is told to", name));
    }
    Ok(())
}

/// `stdin()`: all that is piped in, as text; "" for nothing.
fn stdin_text(_: &mut Interp, _: Vec<Value>, line: u32) -> R {
    use std::io::Read;
    for_scripts("stdin", line)?;
    let mut b = Vec::new();
    Ok(match std::io::stdin().lock().read_to_end(&mut b) {
        Ok(_) => Value::str(String::from_utf8_lossy(&b)),
        Err(e) => Value::err(Value::str(format!("stdin: {}", e))),
    })
}

/// `read_line()`: the next line in, without its line break; null at the end.
fn read_line(_: &mut Interp, _: Vec<Value>, line: u32) -> R {
    use std::io::BufRead;
    for_scripts("read_line", line)?;
    let mut s = String::new();
    Ok(match std::io::stdin().lock().read_line(&mut s) {
        Ok(0) => Value::Nil,
        Ok(_) => {
            if s.ends_with('\n') {
                s.pop();
                if s.ends_with('\r') {
                    s.pop();
                }
            }
            Value::str(s)
        }
        Err(e) => Value::err(Value::str(format!("read_line: {}", e))),
    })
}

/// `exit(n)`: ends the script with status `n`, 0 to 255, once what it printed is out.
fn exit_with(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    use std::io::Write;
    for_scripts("exit", line)?;
    let n = match args.first() {
        None => 0,
        Some(Value::Int(n)) if (0..=255).contains(n) => *n as i32,
        Some(v) => return fail(line, format!("`exit` takes a status from 0 to 255, got {}", crate::interp::short(v))),
    };
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(n)
}

/// `error(r)`: what an error says, and null for any other value (FEEDBACK26.md, Q-199;
/// branch feedback26-fixes).
fn error_of(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match arg(&args, 0) {
        Value::Err(e, _, _) => (*e).clone(),
        _ => Value::Nil,
    })
}

/// Output nobody reads any longer (`| head -1`, a log shipper that stopped): a script ends there
/// as a shell's commands do, status 141, and a server goes on, its pages answered as before
/// (FEEDBACK31.md, Q-379; branch feedback27-35-fixes).
static STDOUT_GONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `print` writes to stderr: under `quillo mcp`, stdout carries MCP's messages and nothing else
/// (FEEDBACK56.md, C.1; branch feedback56). A script's `print` goes to stdout as before.
pub static PRINT_TO_STDERR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn print_line(line: &str) {
    use std::io::Write;
    if PRINT_TO_STDERR.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = writeln!(std::io::stderr().lock(), "{}", line);
        return;
    }
    if STDOUT_GONE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut out = std::io::stdout().lock();
    let gone = writeln!(out, "{}", line).is_err_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe);
    if gone && !crate::value::serving() {
        std::process::exit(141);
    }
    STDOUT_GONE.fetch_or(gone, std::sync::atomic::Ordering::Relaxed);
}

fn print(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    // The program's own log: what it hides from visitors, it sees.
    let line = crate::value::told(|| args.iter().map(|v| v.display()).collect::<Vec<_>>().join(" "));
    print_line(&line);
    Ok(Value::Nil)
}

fn to_str(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::str(arg(&args, 0).display()))
}

fn to_int(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match arg(&args, 0) {
        Value::Int(n) => Value::Int(n),
        // A float past an int's range is an error value, as a dec is
        // (FEEDBACK16.md, Q-111; branch feedback20-fixes).
        Value::Float(f) if f.is_finite() => match f.trunc() {
            t if (-9223372036854775808.0..9223372036854775808.0).contains(&t) => Value::Int(t as i64),
            _ => Value::err(Value::str(format!("{} is too large for an int", Value::Float(f).display()))),
        },
        Value::Bool(b) => Value::Int(b as i64),
        // The whole part (toward zero).
        Value::Dec(d) => d.trunc_i64().map_or_else(|| Value::err(Value::str(format!("{} is too large for an int", d))), Value::Int),
        Value::Str(s) => crate::interp::coerce_loose("int", &s).unwrap_or_else(|| Value::err(Value::str(format!("not an int: {:?}", &*s)))),
        other => Value::err(Value::str(format!("cannot convert {} to int", other.type_name()))),
    })
}

fn to_float(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match arg(&args, 0) {
        Value::Int(n) => Value::Float(n as f64),
        Value::Float(f) => Value::Float(f),
        Value::Dec(d) => Value::Float(d.to_f64()),
        Value::Str(s) => {
            crate::interp::coerce_loose("float", &s).unwrap_or_else(|| Value::err(Value::str(format!("not a number: {:?}", &*s))))
        }
        other => Value::err(Value::str(format!("cannot convert {} to float", other.type_name()))),
    })
}

/// `dec(x)`: a decimal from text (exactly), an int, or a float (through its
/// shortest text, so `dec(0.1)` is 0.1m).
fn to_dec(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    use crate::decimal::Dec;
    Ok(match arg(&args, 0) {
        Value::Dec(d) => Value::Dec(d),
        Value::Int(n) => Value::Dec(Dec::from_i64(n)),
        Value::Float(f) => match Dec::from_f64(f) {
            Some(d) => Value::Dec(d),
            None => Value::err(Value::str("cannot convert float to dec: it is too large, or not a number")),
        },
        Value::Str(s) => match Dec::parse(s.trim()) {
            Some(d) => Value::Dec(d),
            None => Value::err(Value::str(format!("not a decimal: {:?}", &*s))),
        },
        other => Value::err(Value::str(format!("cannot convert {} to dec", other.type_name()))),
    })
}

fn len(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Int(match arg(&args, 0) {
        Value::Str(s) => it.text_len(&s) as i64,
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

/// `normalize(s, "NFC")`: one Unicode form for text that looks the same, so `café` typed and
/// `cafe` with a combining accent are one name; `"NFKC"` also folds look-alikes such as `ﬁ`, for
/// comparing user names (FEEDBACK31.md, Q-394; branch feedback27-35-fixes).
fn normalize(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    use unicode_normalization::UnicodeNormalization;
    let s = s_arg(&args, 0, "normalize", line)?;
    let form = match args.get(1) {
        None | Some(Value::Nil) => "NFC".into(),
        Some(Value::Str(f)) => f.to_ascii_uppercase(),
        Some(other) => return fail(line, format!("`normalize` expects a form (\"NFC\", \"NFD\", \"NFKC\" or \"NFKD\") as argument 2, got {}", other.type_name())),
    };
    Ok(Value::str(match form.as_str() {
        "NFC" => s.nfc().collect::<String>(),
        "NFD" => s.nfd().collect(),
        "NFKC" => s.nfkc().collect(),
        "NFKD" => s.nfkd().collect(),
        _ => return fail(line, format!("`normalize`: the form is \"NFC\", \"NFD\", \"NFKC\" or \"NFKD\", got {:?}", form)),
    }))
}

/// The value, or null for `""`: what `||` does in JavaScript, for text, as
/// `filled(name) ?? "anonymous"` (FEEDBACK19.md, Q-151; branch feedback19-fixes).
fn filled(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(args.into_iter().next().filter(|v| !matches!(v, Value::Str(s) if s.is_empty())).unwrap_or(Value::Nil))
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
        // A predicate that failed has said neither yes nor no: as a no, a failed
        // permission check looks like nothing to see (FEEDBACK26.md, Q-198; branch
        // feedback26-fixes).
        match it.call1(&f, x.clone(), line)? {
            e @ Value::Err(..) => return Ok(e),
            r if r.truthy() => out.push(x.clone()),
            _ => {}
        }
    }
    Ok(Value::list(out))
}

fn find(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "find", line)?;
    let f = f_arg(&args, 1, "find", line)?;
    for x in l.iter() {
        match it.call1(&f, x.clone(), line)? {
            e @ Value::Err(..) => return Ok(e),
            r if r.truthy() => return Ok(x.clone()),
            _ => {}
        }
    }
    Ok(Value::Nil)
}

fn any(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "any", line)?;
    let f = f_arg(&args, 1, "any", line)?;
    for x in l.iter() {
        match it.call1(&f, x.clone(), line)? {
            e @ Value::Err(..) => return Ok(e),
            r if r.truthy() => return Ok(Value::Bool(true)),
            _ => {}
        }
    }
    Ok(Value::Bool(false))
}

fn all(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "all", line)?;
    let f = f_arg(&args, 1, "all", line)?;
    for x in l.iter() {
        match it.call1(&f, x.clone(), line)? {
            e @ Value::Err(..) => return Ok(e),
            r if !r.truthy() => return Ok(Value::Bool(false)),
            _ => {}
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
    for (i, x) in l.iter().enumerate() {
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
            // Text is no number: `+` joined it, and amounts read from a form or a CSV summed to
            // "012.505.00" (FEEDBACK55.md, Q-783; branch feedback55).
            None if matches!(x, Value::Str(_)) => return fail(line, sum_of_text(x, i)),
            None => crate::interp::binop(crate::ast::BinOp::Add, acc, x.clone(), line)?,
        };
    }
    Ok(acc)
}

/// What `sum` says of text at item `i` (the browser's `sum` says the same).
#[cold]
fn sum_of_text(x: &Value, i: usize) -> String {
    format!("`sum` adds numbers; got the str {} at item {}: use `float(...)` or `dec(...)` first", x.repr(), i)
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
                None => {
                    let hint = crate::interp::dec_hint(x, &b);
                    return fail(line, format!("`{}` cannot compare {} with {}{}", name, x.type_name(), b.type_name(), hint));
                }
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
            bad = Some((a.0.type_name(), b.0.type_name(), crate::interp::dec_hint(&a.0, &b.0)));
            std::cmp::Ordering::Equal
        })
    });
    if let Some((x, y, hint)) = bad {
        return fail(line, format!("`{}` cannot compare {} with {}{}", name, x, y, hint));
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
    let mut l = l_arg(&args, 0, "push", line)?;
    drop(args);
    Arc::make_mut(&mut l).push(item);
    Ok(Value::List(l))
}

/// How many ints `range(a, b, step)` gives. A step that is not 0.
pub(crate) fn range_count(a: i64, b: i64, step: i64) -> u64 {
    let (span, by) = if step > 0 { (b as i128 - a as i128, step as i128) } else { (a as i128 - b as i128, -(step as i128)) };
    if span <= 0 {
        return 0;
    }
    ((span + by - 1) / by).min(u64::MAX as i128) as u64
}

fn range(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // With a step, as `for` and a list of every tenth want (FEEDBACK29.md, Q-360; branch
    // feedback29-fixes).
    if args.len() > 2 {
        let (a, b, step) = (i_arg(&args, 0, "range", line)?, i_arg(&args, 1, "range", line)?, i_arg(&args, 2, "range", line)?);
        if step == 0 {
            return fail(line, "`range`: a step of 0 never gets anywhere");
        }
        let n = range_count(a, b, step);
        if n > 10_000_000 {
            return fail(line, "range is too large");
        }
        return Ok(Value::list((0..n as i64).map(|i| Value::Int(a + i * step)).collect()));
    }
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
    if let (Some(Value::Map(m)), Some(Value::Str(k))) = (args.first(), args.get(1)) {
        return Ok(m.get(&**k).cloned().unwrap_or_else(|| arg(&args, 2)));
    }
    let m = m_arg(&args, 0, "get", line)?;
    // An error is no key: its text was one (FEEDBACK55.md, Q-784; branch feedback55).
    if let k @ Value::Err(..) = arg(&args, 1) {
        return Ok(k);
    }
    let k = arg(&args, 1).key_text();
    Ok(m.get(k.as_str()).cloned().unwrap_or_else(|| arg(&args, 2)))
}

fn has(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = m_arg(&args, 0, "has", line)?;
    // An error is no key (FEEDBACK55.md, Q-784; branch feedback55).
    if let k @ Value::Err(..) = arg(&args, 1) {
        return Ok(k);
    }
    Ok(Value::Bool(m.contains_key(arg(&args, 1).key_text().as_str())))
}

fn set(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    set_of(args, line)
}

/// `set` itself, needing no interpreter: compiled loops make its errors with it (src/jitloop.rs).
pub(crate) fn set_of(args: Vec<Value>, line: u32) -> R {
    // `set(xs, i, v)`: a list with item i (from the end when negative) v. Asked without
    // a copy: one kept for the length of this block would make each `set` a copy of the
    // list, and a loop over it quadratic (FEEDBACK25.md, Q-314; branch feedback26-fixes).
    if matches!(args.first(), Some(Value::List(_))) {
        let i = i_arg(&args, 1, "set", line)?;
        let v = arg(&args, 2);
        let mut args = args;
        let Value::List(mut l) = std::mem::replace(&mut args[0], Value::Nil) else { unreachable!() };
        drop(args);
        let n = l.len() as i64;
        let at = if i < 0 { n + i } else { i };
        if at < 0 || at >= n {
            return fail(line, format!("`set`: index {} is outside a list of {}", i, n));
        }
        Arc::make_mut(&mut l)[at as usize] = v;
        return Ok(Value::List(l));
    }
    let mut m = m_arg(&args, 0, "set", line)?;
    // An error is no key: `set({}, int("q"), 1)` made its text one (FEEDBACK55.md, Q-784; branch
    // feedback55).
    if let k @ Value::Err(..) = arg(&args, 1) {
        return Ok(k);
    }
    let (k, v) = (arg(&args, 1).key_text(), arg(&args, 2));
    // The arguments let go first: a map no one else holds is changed, not
    // copied (see `in_place` in src/interp.rs).
    drop(args);
    Arc::make_mut(&mut m).insert(k.into(), v);
    Ok(Value::Map(m))
}

fn remove(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::List(l) => {
            let x = arg(&args, 1);
            Ok(Value::list(l.iter().filter(|v| !equal(v, &x)).cloned().collect()))
        }
        _ => {
            let m = m_arg(&args, 0, "remove", line)?;
            // An error is no key (FEEDBACK55.md, Q-784; branch feedback55).
            if let k @ Value::Err(..) = arg(&args, 1) {
                return Ok(k);
            }
            let k = arg(&args, 1).key_text();
            drop(args);
            let mut m = Arc::unwrap_or_clone(m);
            m.shift_remove(k.as_str());
            Ok(Value::map(m))
        }
    }
}

fn merge(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (a, b) = (m_arg(&args, 0, "merge", line)?, m_arg(&args, 1, "merge", line)?);
    drop(args);
    let mut m = Arc::unwrap_or_clone(a);
    for (k, v) in b.iter() {
        m.insert(k.clone(), v.clone());
    }
    Ok(Value::map(m))
}

// `{ ...m, k: v }` and `[...xs, 3]`, which the parser makes of spreads: an
// error says what was spread, not these functions' names (FEEDBACK3.md,
// Q-036; branch feedback6-fixes). Null spreads nothing, in a map or a list.
fn spread_map(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut out = Map::new();
    for v in args {
        match v {
            Value::Map(m) => out.extend(m.iter().map(|(k, v)| (k.clone(), v.clone()))),
            Value::Nil => {}
            other => return fail(line, format!("`{{...x}}` spreads a map (or null) into a map, but x is {}", other.type_name())),
        }
    }
    Ok(Value::map(out))
}

fn spread_list(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut out = Vec::new();
    for v in args {
        match v {
            Value::List(l) => out.extend(l.iter().cloned()),
            Value::Nil => {}
            other => return fail(line, format!("`[...x]` spreads a list (or null) into a list, but x is {}", other.type_name())),
        }
    }
    Ok(Value::list(out))
}

// ---- errors ------------------------------------------------------------

fn err(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::own_err(arg(&args, 0)))
}

fn is_err(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::Bool(matches!(arg(&args, 0), Value::Err(..))))
}

fn unwrap(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::Err(e, _, _) => fail(line, format!("unwrap of an error: {}", e.display())),
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

/// `url_encode(v)`: `v` as text for a URL, every byte but letters, digits
/// and `-_.~` written `%XX`. A map gives a query string (`a=1&b=x%20y`),
/// leaving out keys whose value is null. The browser's gives the same.
/// The first error in `v`, itself or held in its lists and maps at any depth, in their order.
fn first_error(v: &Value) -> Option<Value> {
    let mut todo = vec![v];
    while let Some(x) = todo.pop() {
        match x {
            Value::Err(..) => return Some(x.clone()),
            Value::List(l) => todo.extend(l.iter().rev()),
            Value::Map(m) => todo.extend(m.values().rev()),
            _ => {}
        }
    }
    None
}

fn url_encode(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    let mut out = String::new();
    // An error is given back, not its text in the URL, wherever it is: in a map's value, in a
    // list there, a map in a list (FEEDBACK55.md, Q-784; branch feedback55; nested ones found
    // later). The first, in order.
    if let Some(e) = first_error(&arg(&args, 0)) {
        return Ok(e);
    }
    match arg(&args, 0) {
        Value::Map(m) => {
            for (k, v) in m.iter().filter(|(_, v)| !matches!(v, Value::Nil)) {
                if !out.is_empty() {
                    out.push('&');
                }
                enc(k, &mut out);
                out.push('=');
                enc(&v.display(), &mut out);
            }
        }
        v => enc(&v.display(), &mut out),
    }
    Ok(Value::str(out))
}

fn env_var(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = s_arg(&args, 0, "env", line)?;
    // A name the policy does not give: as if not set (FEEDBACK41.md, Q-478; branch feedback40-43).
    if crate::policy::get().env_name(&name).is_err() {
        return Ok(arg(&args, 1));
    }
    Ok(crate::dotenv::var(&it.env_file, &name).map(Value::str).unwrap_or_else(|| arg(&args, 1)))
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
    // A wait the server's stopping, or a reload, ends: a job that runs for ever
    // (a game's loop) then stops there, answered as a `reply` would be, instead
    // of holding the server for the whole drain and ending as an internal error,
    // or going on with an old version's code (FEEDBACK12.md, Q-081 and Q-082;
    // branch feedback12-fixes).
    // Only in a job or a tick: a request in progress is answered at a stop, and
    // finishes on its version at a reload, as the guide says (FEEDBACK12.md,
    // Q-081 and Q-082; branch feedback12-sleep).
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    let retired = it.retired.clone();
    let in_job = it.in_job;
    // A `parallel` function ends its sleep once another has failed (FEEDBACK55.md, Q-804; branch
    // feedback55).
    let stop = it.stop.clone();
    let slice = std::time::Duration::from_millis(if stop.is_some() { 20 } else { 50 });
    let why = move || {
        if in_job && crate::httpd::stopping() {
            return Some("the server is stopping");
        }
        if in_job && retired.load(std::sync::atomic::Ordering::Relaxed) {
            return Some("the program was loaded again");
        }
        if stop.as_ref().is_some_and(|s| s.stopped()) {
            return Some(crate::interp::PARALLEL_STOPPED);
        }
        None
    };
    let sliced = in_job || it.stop.is_some();
    // A request waits on a timer, which wakes it: it held one of the threads that wait for
    // requests for the whole sleep, and 65 sleeping in a stream's loop held up every other
    // request that waited for anything (FEEDBACK55.md, Q-802; branch feedback55). A sleep of a
    // few milliseconds waits on a thread as before: the timer's are whole milliseconds, rounded
    // up, and `sleep(1)` took 2.5 ms (found in review).
    let timed = if it.sqlite_tx == 0 && ms >= SHORT_SLEEP_MS {
        let why = why.clone();
        crate::httpd::await_elsewhere(Box::pin(async move {
            loop {
                let left = until.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return None;
                }
                if let Some(w) = why() {
                    return Some(w);
                }
                tokio::time::sleep(if sliced { left.min(slice) } else { left }).await;
            }
        }))
    } else {
        None
    };
    let ended = match timed {
        Some(r) => r.unwrap_or(None),
        None => it.unlocked(move || loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            if let Some(w) = why() {
                return Some(w);
            }
            std::thread::sleep(left.min(slice));
        }),
    };
    match ended {
        None => Ok(Value::Nil),
        Some(why) if why == crate::interp::PARALLEL_STOPPED => fail(line, why),
        Some(why) => Err(Ctrl::Reply(Arc::new(Response {
            status: 503,
            headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
            body: Body::Value(Value::str(&format!("{}\n", why))),
        }))),
    }
}

/// Sleeps shorter than this wait on a thread, for their accuracy (see `sleep`).
const SHORT_SLEEP_MS: u64 = 5;

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
        "xml" | "xsl" => "application/xml",
        "txt" | "text" => "text/plain; charset=utf-8",
        // Text a site serves that went out as octet-stream, which `nosniff` keeps a browser
        // from guessing at: subtitles, calendars, feeds, HLS (FEEDBACK36.md, Q-451; branch
        // feedback36-39).
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "vtt" => "text/vtt; charset=utf-8",
        "ics" => "text/calendar; charset=utf-8",
        "rss" => "application/rss+xml; charset=utf-8",
        "atom" => "application/atom+xml; charset=utf-8",
        "jsonld" => "application/ld+json",
        "geojson" => "application/geo+json",
        "yaml" | "yml" => "application/yaml; charset=utf-8",
        "toml" => "application/toml; charset=utf-8",
        "m3u8" => "application/vnd.apple.mpegurl",
        "mpd" => "application/dash+xml",
        "ts" => "video/mp2t",
        "m4s" => "video/iso.segment",
        "srt" => "application/x-subrip; charset=utf-8",
        "tsv" => "text/tab-separated-values; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "heic" => "image/heic",
        "jxl" => "image/jxl",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "opus" => "audio/opus",
        "ogg" | "oga" => "audio/ogg",
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
    file_as(it, args, line, None, false)
}

/// `file`; `hosted` for `files`, with the folder it serves: it serves none of the program's own
/// files under another name and no SQLite database, and what it sends is the file it judged (see
/// `open_hosted`; FEEDBACK55.md, Q-701 and Q-702; branch feedback55). `whole` for the page of a
/// miss, which is no copy the browser holds or a part of, so the request's conditionals and range
/// are not applied to it (FEEDBACK55.md, Q-722; branch feedback55).
fn file_as(it: &mut Interp, args: Vec<Value>, line: u32, hosted: Option<&Path>, whole: bool) -> R {
    // A file that is not what it was when it was looked at (saved anew, written to) is looked at
    // again, and answered with its own head (FEEDBACK55.md, Q-701; branch feedback55).
    for _ in 0..3 {
        if let Some(v) = file_once(it, &args, line, hosted, whole)? {
            return Ok(v);
        }
    }
    // Another file in its place each time: a moment's wait, not a miss (FEEDBACK55.md, Q-701;
    // branch feedback55).
    Ok(Value::Response(Arc::new(Response {
        status: 503,
        headers: vec![("retry-after".into(), "1".into()), ("cache-control".into(), "no-store".into())],
        body: Body::Value(Value::str("the file is being replaced; try again\n")),
    })))
}

/// Why `files` did not open a file: it keeps it to itself, or another took its place as it was
/// opened (saved anew), which is looked at again (FEEDBACK55.md, Q-701; branch feedback55).
#[derive(Debug, PartialEq)]
pub(crate) enum Judged {
    Refused,
    Replaced,
}

/// `file_as` once: None when the file changed as it was read (FEEDBACK55.md, Q-701; branch
/// feedback55).
fn file_once(it: &mut Interp, args: &[Value], line: u32, hosted: Option<&Path>, whole: bool) -> Result<Option<Value>, Ctrl> {
    let p = s_arg(args, 0, "file", line)?;
    let rel = Path::new(&*p);
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Ok(Some(not_found()));
    }
    let Some(full) = inside(it, it.base_dir.join(rel)) else { return Ok(Some(not_found())) };
    let Ok(info) = crate::statics::info(&full) else { return Ok(Some(not_found())) };
    // For `files`: the program's own under another name, and the journal of a database by the
    // name asked for, at once; then a small file's copy in memory, which it judged, or the file
    // opened, judged and read whole off the lock, and kept, whatever the answer is to be, so a
    // conditional, a range or a HEAD that follows reads nothing; a large one when it is sent, or
    // here, off the lock, for an answer of none of it or of a part (FEEDBACK55.md, Q-701 and
    // Q-702; branch feedback55).
    let sent = hosted.map(|served| crate::statics::Hosted { served: served.to_path_buf(), program: it.base_dir.to_path_buf(), etag: info.etag.clone(), id: info.id });
    let mut copy: Option<Arc<[u8]>> = None;
    if let Some(h) = &sent {
        if info.id.is_some_and(|id| own_id(id, info.links, info.born).is_some()) || crate::db::journal_of(&full).is_some_and(|db| crate::db::sqlite_file(&db)) {
            return Ok(Some(not_found()));
        }
        if crate::statics::small(&info) {
            copy = crate::statics::judged_copy(&full, &info);
            if copy.is_none() {
                let (h, at, looked, buf) = (h.clone(), full.clone(), info.clone(), crate::statics::buffer(info.len));
                let job = move || -> Result<Arc<[u8]>, Judged> {
                    let now = std::time::SystemTime::now();
                    let (f, m) = open_hosted(&h.served, &h.program, &at)?;
                    // Another file in its place: looked at again; the same, written to: what is
                    // there, as many bytes as were looked at, kept only as it was then, and not
                    // changed so lately that a write could hide in its clock's tick.
                    if crate::statics::replaced(h.id, &h.etag, &at, &m) {
                        return Err(Judged::Replaced);
                    }
                    let b = crate::statics::read_into(&f, buf).map_err(|_| Judged::Refused)?;
                    // The bytes sent are the bytes judged (FEEDBACK55.md, Q-702; branch
                    // feedback55).
                    if crate::db::sqlite_bytes(&b) {
                        return Err(Judged::Refused);
                    }
                    if b.len() as u64 == looked.len && f.metadata().is_ok_and(|m| crate::statics::same(&looked, &at, &m) && !crate::statics::racy(&m, now)) {
                        crate::statics::keep_judged(&at, &looked, b.clone());
                    }
                    Ok(b)
                };
                // Read once into its copy, made on this thread (`statics::buffer`). One of up to
                // 64 KiB here, where the read costs less than handing it to another thread
                // (30 µs against 68 a file), as is any where the request cannot wait elsewhere (in
                // a SQLite transaction); a larger one off the lock, a few at a time: its place
                // waited for on the async runtime, with no thread held, then read on one of its
                // threads for blocking work (FEEDBACK55.md, Q-701; branch feedback55).
                let read = if info.len <= 64 << 10 || !crate::httpd::in_task() || crate::httpd::held() {
                    job()
                } else {
                    let waited = crate::httpd::await_elsewhere(Box::pin(async move {
                        let _place = crate::statics::read_places().acquire().await;
                        tokio::task::spawn_blocking(job).await
                    }));
                    match waited {
                        Some(Ok(Ok(r))) => r,
                        Some(Ok(Err(e))) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
                        _ => Err(Judged::Refused),
                    }
                };
                match read {
                    Ok(b) => copy = Some(b),
                    Err(Judged::Refused) => return Ok(Some(not_found())),
                    Err(Judged::Replaced) => return Ok(None),
                }
            }
        }
    }
    // A large file judged for an answer that sends none of it: Some(false) for one saved anew in
    // its place (FEEDBACK55.md, Q-701 and Q-702; branch feedback55).
    // One judged before and not changed since needs no more than the look at it (FEEDBACK55.md,
    // Q-701; branch feedback55).
    let judge_large = |it: &mut Interp| -> Option<bool> {
        let Some(h) = sent.clone() else { return Some(true) };
        if copy.is_some() || crate::statics::judged_large(&full, &info) {
            return Some(true);
        }
        let (at, looked) = (full.clone(), info.clone());
        it.unlocked(move || match (std::time::SystemTime::now(), open_hosted(&h.served, &h.program, &at)) {
            (_, Ok((_, m))) if crate::statics::replaced(h.id, &h.etag, &at, &m) => Some(false),
            (now, Ok((f, m))) => {
                if sqlite_head(&f) {
                    return None;
                }
                if crate::statics::same(&looked, &at, &m) && !crate::statics::racy(&m, now) {
                    crate::statics::keep_judged_large(&at, &looked);
                }
                Some(true)
            }
            (_, Err(Judged::Replaced)) => Some(false),
            (_, Err(Judged::Refused)) => None,
        })
    };
    // Options it does not know are refused, naming those it does, as a mistyped `cache` would
    // otherwise go unseen (FEEDBACK27.md, Q-331; branch feedback27-35-fixes). `download` names
    // the file a browser saves, and `type` is its content type (FEEDBACK30.md, limitations).
    let mut download = None;
    let mut given_type = None;
    if let Some(Value::Map(m)) = args.get(1) {
        for (k, v) in m.iter() {
            match (&**k, v) {
                ("cache", _) => {}
                ("download", Value::Str(n)) if !n.is_empty() => download = Some(n.clone()),
                // What is wrong with a str, said (FEEDBACK55.md, Q-726; branch feedback55).
                ("download", Value::Str(_)) => return fail(line, "`file`'s download is empty: give the name a browser saves the file as, or leave it out"),
                ("type", Value::Str(t)) => given_type = Some(content_type(t, "file", line)?),
                ("download" | "type", Value::Nil) => {}
                ("download" | "type", other) => return fail(line, format!("`file`'s {} is a str, got {}", k, other.repr())),
                (k, _) => return fail(line, format!("`file` has no option `{}`: it takes `cache`, `download` and `type`", k)),
            }
        }
    }
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
        Ok(Some(Value::Response(Arc::new(Response { status, headers, body: Body::Bytes(body) }))))
    };
    // What the browser already has, or wants a part of.
    let asked = |name: &str, it: &mut Interp| -> Option<String> {
        if whole {
            return None;
        }
        let Some(Value::Map(req)) = it.request_value() else { return None };
        let Some(Value::Map(h)) = req.get("headers") else { return None };
        h.get(name).map(|v| v.display())
    };
    if crate::statics::WATCH_SERVED.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(Value::Map(req)) = it.request_value() {
            if let Some(path) = req.get("path") {
                crate::statics::served(&full, &path.display());
            }
        }
    }
    // As RFC 9110 §13.2.2 orders them: `If-Match`, else `If-Unmodified-Since`, refuse with a 412
    // when the file is not the one meant; then `If-None-Match`, else `If-Modified-Since`, give a
    // 304 for a file not changed since the date given, a later one or one in another of HTTP's
    // forms too (FEEDBACK36.md, Q-441; branch feedback36-39).
    // `If-Match` and `If-Range` take the file's own tag with or without its `W/`, as nginx does:
    // every file's tag is weak, as its gzipped copy is the same content, and a part or a check
    // before a write is of its bytes as they are, so a strong comparison would have every resume
    // and seek fetch the whole file again (FEEDBACK55.md, Q-723; branch feedback55). A matching
    // `If-None-Match` is a 412 for a method that is not GET or HEAD, which `If-Modified-Since`
    // does not apply to (RFC 9110 §13.1.2 and §13.1.3; FEEDBACK55.md, Q-723; branch feedback55).
    let since = |v: Option<String>| v.and_then(|v| crate::dates::http_seconds(&v));
    let changed = match asked("if-match", it) {
        Some(v) => !crate::statics::matches(&v, &info.etag),
        None => since(asked("if-unmodified-since", it)).is_some_and(|t| info.modified > t),
    };
    let (reads, head) = match it.request_value() {
        Some(Value::Map(req)) => match req.get("method") {
            Some(Value::Str(m)) => (&**m == "GET" || &**m == "HEAD", &**m == "HEAD"),
            _ => (true, false),
        },
        _ => (true, false),
    };
    let fresh = !changed
        && match asked("if-none-match", it) {
            Some(v) => crate::statics::matches(&v, &info.etag),
            None if reads => since(asked("if-modified-since", it)).is_some_and(|t| info.modified <= t),
            None => false,
        };
    if changed || fresh {
        match judge_large(it) {
            None => return Ok(Some(not_found())),
            Some(false) => return Ok(None),
            Some(true) => {}
        }
        return answer(if fresh && reads { 304 } else { 412 }, headers, Vec::new());
    }
    headers.push(("content-type".into(), given_type.map_or_else(|| mime(&full).to_string(), |t| t.to_string())));
    headers.push(("accept-ranges".into(), "bytes".into()));
    if let Some(n) = download {
        headers.push(("content-disposition".into(), crate::upload::attachment(&n)));
    }
    // Larger files are sent from disk as they are written (see `statics`).
    let large = info.len > 1 << 20;
    if let Some(r) = asked("range", it) {
        // The file's own tag, with or without its `W/`, or its date as it is (FEEDBACK55.md,
        // Q-723; branch feedback55).
        let same = asked("if-range", it).map_or(true, |v| crate::statics::same_tag(&v, &info.etag) || v.trim() == info.last_modified);
        match crate::statics::range(&r, info.len).filter(|_| same) {
            // A part of a large file is sent from disk only when it is large itself: a small one
            // takes no place among the large sends (FEEDBACK55.md, Q-725; branch feedback55).
            Some(Ok((start, end))) if large && !crate::statics::small_len(end - start + 1) => {
                headers.push(("content-range".into(), format!("bytes {}-{}/{}", start, end, info.len)));
                headers.push((crate::statics::SEND_HEADER.into(), crate::statics::mark(&full, start, end - start + 1, sent.as_ref())));
                return answer(206, headers, Vec::new());
            }
            Some(Ok((start, end))) => {
                let n = end - start + 1;
                // From the copy judged, or for `files` a large one opened and judged off the lock
                // (FEEDBACK55.md, Q-701; branch feedback55). A file written to in place since the
                // look gives what it has there, the range said as sent.
                let part = if let Some(c) = &copy {
                    Ok(c.get(start as usize..c.len().min(end as usize + 1)).unwrap_or_default().to_vec())
                } else if let Some(h) = sent.clone() {
                    let at = full.clone();
                    it.unlocked(move || match open_hosted(&h.served, &h.program, &at) {
                        Ok((f, m)) if !crate::statics::replaced(h.id, &h.etag, &at, &m) => {
                            // The part sent judged when it is the start, read with 16 bytes at
                            // least; the file's start else (FEEDBACK55.md, Q-702; branch
                            // feedback55).
                            let mut part = crate::statics::read_at(&f, start, if start == 0 { n.max(16) } else { n })?;
                            if if start == 0 { crate::db::sqlite_bytes(&part) } else { sqlite_head(&f) } {
                                return Err(std::io::ErrorKind::NotFound.into());
                            }
                            part.truncate(n as usize);
                            Ok(part)
                        }
                        Ok(_) | Err(Judged::Replaced) => Err(crate::statics::changed()),
                        Err(Judged::Refused) => Err(std::io::ErrorKind::NotFound.into()),
                    })
                } else {
                    let from = full.clone();
                    it.unlocked(move || crate::statics::read_range(&from, start, end))
                };
                return match part {
                    Ok(bytes) if !bytes.is_empty() => {
                        headers.push(("content-range".into(), format!("bytes {}-{}/{}", start, start + bytes.len() as u64 - 1, info.len)));
                        answer(206, headers, bytes)
                    }
                    Ok(_) => Ok(None),
                    Err(e) if crate::statics::is_changed(&e) => Ok(None),
                    Err(_) => Ok(Some(not_found())),
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
        if head {
            match judge_large(it) {
                None => return Ok(Some(not_found())),
                Some(false) => return Ok(None),
                Some(true) => {}
            }
        }
        headers.push((crate::statics::SEND_HEADER.into(), crate::statics::mark(&full, 0, info.len, sent.as_ref())));
        return answer(200, headers, Vec::new());
    }
    // Small files are kept in memory: for `files`, the copy of the file judged, under what it was;
    // a HEAD is answered as the GET is, its body dropped as it is sent, so its length and its
    // gzip are the GET's (FEEDBACK55.md, Q-701; branch feedback55).
    let body = match copy {
        Some(c) => Ok(c),
        None => crate::statics::read(&full, &info),
    };
    match body {
        Ok(b) => answer(200, headers, b.to_vec()),
        Err(e) if crate::statics::is_changed(&e) => Ok(None),
        Err(_) => Ok(Some(not_found())),
    }
}

/// The program's own files: its `.env` files, the source it loaded and the server's TLS key, by
/// identity, so that a second hard link to one, under any name, is one of them too, and stays one
/// when the file is saved anew (a rename over it: an editor's save, `git pull`, a deploy) and the
/// link keeps what it held. Each with when it was made: its number used again for another file
/// (as ext4 does) is not it; where the system says not, with the path it was found at, and one
/// no longer there is let go. The settings and the key are known by their paths as well: one
/// saved anew is judged by what is there when it is asked for, and a file opened at one of those
/// paths is one of them (FEEDBACK55.md, Q-701; branch feedback55). The databases it has open are
/// `db`'s to say.
#[derive(Default)]
struct Own {
    ids: std::collections::HashMap<crate::statics::FileId, Kept>,
    named: Vec<(std::path::PathBuf, &'static str)>,
}

struct Kept {
    path: std::path::PathBuf,
    why: &'static str,
    born: Option<std::time::SystemTime>,
}

static OWN: std::sync::LazyLock<std::sync::RwLock<Own>> = std::sync::LazyLock::new(Default::default);

fn own() -> std::sync::RwLockReadGuard<'static, Own> {
    OWN.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) const OWN_SOURCE: &str = "it is one of the program's own files";
pub(crate) const OWN_SETTINGS: &str = "it is the program's settings (.env)";
pub const OWN_TLS_KEY: &str = "it is the server's TLS key";

/// Notes `p`, if it is there, as one of the program's own files. A source file's earlier
/// versions at that path are let go, so `--watch` saving one again and again keeps one each; the
/// settings' and the key's are kept, eight at most (FEEDBACK55.md, Q-701; branch feedback55).
pub fn protect(p: &Path, why: &'static str) {
    let Ok(m) = std::fs::metadata(p) else { return };
    let Some(id) = crate::statics::id_of(&m) else { return };
    let mut o = OWN.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if why == OWN_SOURCE {
        o.ids.retain(|k, e| *k == id || e.path != p);
    } else if o.ids.values().filter(|e| e.path == p).count() >= 8 {
        let oldest = o.ids.iter().filter(|(k, e)| e.path == p && **k != id).min_by_key(|(_, e)| e.born).map(|(k, _)| *k);
        if let Some(k) = oldest {
            o.ids.remove(&k);
        }
    }
    o.ids.insert(id, Kept { path: p.to_path_buf(), why, born: m.created().ok() });
}

/// `protect`, and the path kept as well, as given and made absolute, for a file judged by what is
/// at its path when it is asked for: the settings and the TLS key. A link given is kept as a
/// link: certbot's `live/privkey.pem` leads to another file at each renewal, and so may
/// `--env-file`'s (FEEDBACK55.md, Q-701; branch feedback55).
pub fn protect_named(p: &Path, why: &'static str) {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    protect(&p, why);
    let mut o = OWN.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !o.named.iter().any(|(n, _)| *n == p) {
        o.named.push((p, why));
    }
}

/// Why the file `id` is what a path of the settings or the TLS key leads to now, if it is: each
/// looked at (there are few), off the lock (FEEDBACK55.md, Q-701; branch feedback55).
pub(crate) fn named_id(id: crate::statics::FileId) -> Option<&'static str> {
    let named: Vec<(std::path::PathBuf, &'static str)> = own().named.clone();
    named.into_iter().find(|(p, _)| std::fs::metadata(p).ok().and_then(|m| crate::statics::id_of(&m)) == Some(id)).map(|(p, why)| {
        protect(&p, why);
        why
    })
}

/// Notes the settings in `dir`, its `.env` and each `.env.*` (`.env.production`), and the file
/// `--env-file` named, which is kept for each reload too (FEEDBACK55.md, Q-701; branch
/// feedback55).
pub fn protect_settings(dir: &Path, named: Option<&Path>) {
    static NAMED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
    let mut kept = NAMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(f) = named {
        *kept = Some(f.to_path_buf());
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let n = fold_name(&e.file_name().to_string_lossy());
        if n == ".env" || n.starts_with(".env.") {
            protect_named(&e.path(), OWN_SETTINGS);
        }
    }
    // `.env` too when it is not there yet: one made later is judged by its path.
    protect_named(&dir.join(".env"), OWN_SETTINGS);
    if let Some(f) = &*kept {
        protect_named(f, OWN_SETTINGS);
    }
}

/// Why the file `id`, with `links` names and made at `born`, is the program's own, if it is:
/// found by its number (the same file, made then; or, where the system does not say, still at
/// its path), a database it has open, or, for a file with another name, what is at a path of the
/// settings or the key now (FEEDBACK55.md, Q-701; branch feedback55). An ordinary file of one
/// name costs a look in memory.
pub(crate) fn own_id(id: crate::statics::FileId, links: u64, born: Option<std::time::SystemTime>) -> Option<&'static str> {
    let found = own().ids.get(&id).map(|e| (e.path.clone(), e.why, e.born));
    if let Some((path, why, then)) = found {
        let still = match (then, born) {
            (Some(then), Some(born)) => then == born,
            // Looked at off the lock.
            _ => std::fs::metadata(&path).ok().and_then(|m| crate::statics::id_of(&m)) == Some(id),
        };
        if still {
            return Some(why);
        }
        let mut o = OWN.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if o.ids.get(&id).is_some_and(|e| e.path == path && e.born == then) {
            o.ids.remove(&id);
        }
    }
    if crate::db::open_id(id) {
        return Some(crate::db::OPEN);
    }
    if links > 1 {
        return named_id(id);
    }
    None
}

/// `own_id` of what a look found (FEEDBACK55.md, Q-701; branch feedback55).
pub(crate) fn own_meta(m: &std::fs::Metadata) -> Option<&'static str> {
    #[cfg(unix)]
    let links = std::os::unix::fs::MetadataExt::nlink(m);
    #[cfg(not(unix))]
    let links = 1;
    own_id(crate::statics::id_of(m)?, links, m.created().ok())
}

/// The file at `full` opened for `files`, serving `served` in the program's folder `program`, if
/// it serves it: a file of bytes with a name, not one of the program's own by number or what a
/// path of the settings or the key leads to now, where it really is (as the system says of the
/// file opened, so no link swapped in on the way since it was judged takes it out of the folder
/// or to what it keeps), and no journal of a SQLite database by the name it really has; its first
/// bytes are the caller's to judge, as they are sent (`sqlite_head`) (FEEDBACK55.md, Q-701 and
/// Q-702; branch feedback55). The folders are named as the system names the file, so
/// `/System/Volumes/Data/private/tmp` is `/private/tmp`.
pub(crate) fn open_hosted(served: &Path, program: &Path, full: &Path) -> Result<(std::fs::File, std::fs::Metadata), Judged> {
    use Judged::{Refused, Replaced};
    let (f, m) = crate::statics::open_regular(full).map_err(|_| Refused)?;
    if own_meta(&m).is_some() || crate::statics::id_of(&m).is_some_and(|id| named_id(id).is_some()) {
        return Err(Refused);
    }
    // Unlinked since it was opened (another saved in its place): no name is left to judge it by,
    // and the one there now is looked at instead (on Linux it is named `... (deleted)`).
    let real = crate::statics::path_of(&f).or_else(|| full.canonicalize().ok()).ok_or(Replaced)?;
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&f.metadata().map_err(|_| Refused)?) == 0 {
        return Err(Replaced);
    }
    let (program, served) = (crate::statics::real_dir(program).ok_or(Refused)?, crate::statics::real_dir(served).ok_or(Refused)?);
    if !real.starts_with(&program) || hidden_target(&program, &served, &real) {
        return Err(Refused);
    }
    if crate::db::journal_of(&real).is_some_and(|db| crate::db::sqlite_file(&db)) {
        return Err(Refused);
    }
    Ok((f, m))
}

/// Whether the file opened begins as a SQLite database, its log or its journal: for an answer of
/// none of it, or of a part from further in. What is sent from the start is judged by the bytes
/// sent (FEEDBACK55.md, Q-702; branch feedback55).
pub(crate) fn sqlite_head(f: &std::fs::File) -> bool {
    crate::statics::read_at(f, 0, 16).map_or(true, |h| crate::db::sqlite_bytes(&h))
}

/// Whether `part` has the form of a short name Windows gives a file (`GIT~1`, `ENV~1`, `PROGRA~2.TXT`),
/// which is another name of a file of any name: `/GIT~1/config` was `.git/config`. Std has no
/// `GetLongPathName` to say which, so on Windows such a part is kept back (FEEDBACK55.md, Q-701;
/// branch feedback55).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn short_name(part: &str) -> bool {
    part.find('~').is_some_and(|i| part[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
}

/// A name as macOS and Windows compare names: `APP.DB` is `app.db`, `ſ` is `s`, and `é` is
/// `é` however it is written (FEEDBACK55.md, Q-704; branch feedback55).
pub(crate) fn fold_name(name: &str) -> String {
    if name.is_ascii() {
        return name.to_ascii_lowercase();
    }
    use unicode_normalization::UnicodeNormalization;
    name.nfc().collect::<String>().to_uppercase().to_lowercase()
}

/// A file's name as Windows takes it: what precedes a `:` (`.env::$DATA` is `.env`'s data), and
/// no dots or spaces at the end (`app.db.` is `app.db`); held to on every system (FEEDBACK55.md,
/// Q-704; branch feedback55).
pub(crate) fn name_as_taken(name: &str) -> &str {
    name.split(':').next().unwrap_or(name).trim_end_matches(['.', ' '])
}

/// `accept_calls { keys, shared, jwt, client_cert, check }`: how calls from other services are
/// taken, at the top level of the program they call (FEEDBACK38.md, Q-458 to Q-462; branch
/// feedback36-39). See src/svcauth.rs.
fn accept_calls(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match crate::svcauth::accept_of(args.first().unwrap_or(&Value::Nil)) {
        Ok(a) => {
            it.accept = Some(Arc::new(a));
            Ok(Value::Nil)
        }
        Err(e) => fail(line, e),
    }
}

/// `server fn refund(...) for ["shop"]`: the callers it takes, as the parser writes it.
fn callers(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::Str(f)), Some(Value::List(l))) = (args.first(), args.get(1)) else {
        return fail(line, "`__callers` is written by `server fn ... for [...]`");
    };
    let names = l.iter().map(|v| v.display()).collect();
    Arc::make_mut(&mut it.fn_callers).insert(f.to_string(), names);
    Ok(Value::Nil)
}

/// `classes()`: the classes the program's markup writes (FEEDBACK37.md, Q-456; branch
/// feedback36-39).
fn classes(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    let files: Vec<&[crate::ast::Stmt]> = it.program.iter().map(|f| f.as_slice()).collect();
    Ok(Value::list(crate::styles::written_classes(&files).into_iter().map(Value::str).collect()))
}

/// What `build "/path" = expr` has made: by path and the version of the program (its function's
/// address), being made, made, or failed; and the last good value of each path, which a reload
/// that fails keeps serving.
struct Builds {
    slots: std::collections::HashMap<(String, usize), Result<Option<Value>, String>>,
    good: std::collections::HashMap<String, Value>,
}

static BUILDS: std::sync::Mutex<Option<Builds>> = std::sync::Mutex::new(None);
static BUILT: std::sync::Condvar = std::sync::Condvar::new();

/// `build "/app.css" = expr`'s route: the value, made by the first request for it (the server's
/// own, as the port opens) and kept; one that comes meanwhile waits, off the lock. A failure at
/// the first start stops the program, as `style_with`'s does; after a reload, the last good one
/// is served (FEEDBACK37.md, Q-454; branch feedback36-39).
fn built(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::Str(path)), Some(f @ Value::Func(c))) = (args.first(), args.get(1)) else {
        return fail(line, "`__built` is the route of a `build` declaration");
    };
    let key = (path.to_string(), Arc::as_ptr(&c.def) as usize);
    let lock = || BUILDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        let state = {
            let mut b = lock();
            let b = b.get_or_insert_with(|| Builds { slots: Default::default(), good: Default::default() });
            match b.slots.get(&key) {
                None => {
                    b.slots.insert(key.clone(), Ok(None));
                    None
                }
                Some(s) => Some(s.clone()),
            }
        };
        match state {
            // This request makes it.
            None => break,
            Some(Ok(Some(v))) => return Ok(built_answer(path, &v)),
            Some(Ok(None)) => {
                let key = key.clone();
                it.unlocked(move || {
                    let mut b = lock();
                    while matches!(b.as_ref().and_then(|b| b.slots.get(&key)), Some(Ok(None))) {
                        b = BUILT.wait(b).unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                });
            }
            Some(Err(e)) => {
                let good = lock().as_ref().and_then(|b| b.good.get(&**path).cloned());
                return match good {
                    Some(v) => Ok(built_answer(path, &v)),
                    None => Ok(Value::err(Value::str(e))),
                };
            }
        }
    }
    let started = std::time::Instant::now();
    let made = it.call(f, Vec::new(), line);
    let made = match made {
        Ok(Value::Err(e, ..)) => Err(e.display()),
        Ok(v) => Ok(v),
        Err(Ctrl::Error(e)) => Err(e.msg.clone()),
        Err(Ctrl::Return(v)) => Ok(v),
        Err(other) => return Err(other),
    };
    let mut b = lock();
    let b = b.as_mut().expect("made above");
    let first = !b.good.contains_key(&**path);
    let answer = match &made {
        Ok(v) => {
            b.slots.insert(key, Ok(Some(v.clone())));
            b.good.insert(path.to_string(), v.clone());
            eprintln!("quillo: built {} in {:.1} s", path, started.elapsed().as_secs_f64());
            built_answer(path, v)
        }
        Err(e) => {
            b.slots.insert(key, Err(e.clone()));
            eprintln!("quillo: build {} failed: {}", path, e);
            if first && !crate::server::SHOW_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
                crate::httpd::flush_log();
                std::process::exit(1);
            }
            b.good.get(&**path).map_or_else(|| Value::err(Value::str(e.as_str())), |v| built_answer(path, v))
        }
    };
    BUILT.notify_all();
    Ok(answer)
}

/// A build's value as its answer: text and bytes in the type its path's extension names, with
/// an ETag, so a browser's copy is checked rather than fetched whole.
fn built_answer(path: &str, v: &Value) -> Value {
    let (body, ctype) = match v {
        Value::Response(_) => return v.clone(),
        Value::Str(s) => (s.as_bytes().to_vec(), mime(Path::new(path)).to_string()),
        Value::Html(h) => (h.as_bytes().to_vec(), "text/html; charset=utf-8".to_string()),
        other => {
            let mut out = Vec::new();
            other.json_into(&mut out);
            (out, "application/json".to_string())
        }
    };
    let etag = format!("\"{}\"", &crate::crypto::hex(&<sha2::Sha256 as sha2::Digest>::digest(&body))[..16]);
    let headers = vec![("content-type".into(), ctype), ("etag".into(), etag), ("cache-control".into(), "no-cache".into())];
    Value::Response(Arc::new(Response { status: 200, headers, body: Body::Bytes(body) }))
}

/// `files("site", path, { index, extensions, not_found, cache })`: a folder of files as a static
/// host serves it. `/` and `/docs/` are their `index.html`, `/about` finds `about.html`, a folder
/// named without its `/` is redirected to it, and a miss is the folder's `404.html` when it has
/// one. Files whose names start with a dot are not served (but `.well-known/`), nor Quillo's own
/// (`.qwe`, `.qwer`) (FEEDBACK36.md, Q-450; branch feedback36-39), whether named so or reached
/// through a link, nor a SQLite database that is open (FEEDBACK55.md, Q-701 and Q-702; branch
/// feedback55).
/// Set by `quillo serve`, whose URLs are its own: a file asked for with a `/` after it is moved to
/// its path without it (FEEDBACK55.md, Q-721; branch feedback55).
pub static SERVES_FOLDER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn files(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let dir = s_arg(&args, 0, "files", line)?;
    let path = match args.get(1) {
        Some(Value::Str(p)) => p.clone(),
        None | Some(Value::Nil) => "".into(),
        Some(v) => return fail(line, format!("`files` takes the path asked for as argument 2, a str, got {}", v.repr())),
    };
    let (mut index, mut extensions, mut not_found_page) = ("index.html".to_string(), vec!["html".to_string()], Some("404.html".to_string()));
    let mut file_opts = Map::new();
    match args.get(2) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            for (k, v) in m.iter() {
                match (&**k, v) {
                    ("index", Value::Str(s)) if !s.is_empty() && !s.contains('/') => index = s.to_string(),
                    ("extensions", Value::List(l)) => {
                        extensions = l.iter().map(|e| e.display().trim_start_matches('.').to_string()).filter(|e| !e.is_empty()).collect();
                    }
                    ("not_found", Value::Str(s)) => not_found_page = Some(s.to_string()).filter(|s| !s.is_empty()),
                    ("not_found", Value::Nil) => not_found_page = None,
                    ("cache", v) => {
                        file_opts.insert("cache".into(), v.clone());
                    }
                    ("index" | "extensions" | "not_found", other) => {
                        return fail(line, format!("`files`'s {} is {}, got {}", k, if &**k == "extensions" { "a list of str" } else { "a file name" }, other.repr()))
                    }
                    (k, _) => return fail(line, format!("`files` has no option `{}`: it takes `index`, `extensions`, `not_found` and `cache`", k)),
                }
            }
        }
        Some(v) => return fail(line, format!("`files` expects options as a map, got {}", v.type_name())),
    }
    let rel = path.trim_matches('/');
    let hidden = hidden_path(rel);
    let base = it.base_dir.join(&*dir);
    // What a link leads to is judged as its name is: `notes.txt -> ../main.qwe` served the
    // program's source and `settings.txt -> ../.env` its settings (FEEDBACK55.md, Q-701; branch
    // feedback55). A link out of the folder is not served, as `stays_in` says, though it leads
    // into a folder `--allow-path` names: an index, a name found with its extension or the page of
    // a miss is judged as the path asked for is, which no `--allow-path` let out either; `file`
    // serves one, named on purpose.
    let judged = |it: &Interp, led: &Option<Option<std::path::PathBuf>>| match led {
        None => false,
        Some(None) => true,
        // The folders as `real` is named: resolved as it was (FEEDBACK55.md, Q-701; branch
        // feedback55).
        Some(Some(real)) => match (it.base_dir.canonicalize(), base.canonicalize()) {
            (Ok(program), Ok(served)) => !hidden_target(&program, &served, real),
            _ => false,
        },
    };
    // The file itself is judged as it is read, by `kept_from_host` (FEEDBACK55.md, Q-701 and
    // Q-702; branch feedback55).
    let serve = |it: &mut Interp, name: String| file_as(it, vec![Value::str(name), Value::map(file_opts.clone())], line, Some(&base), false);
    // Whether a file in a folder whose way there was looked at is there, and may be served, in
    // the one look that said whether it was there: only a link, or a way with one, is followed
    // (FEEDBACK55.md, Q-701; branch feedback55).
    let found = |it: &Interp, full: &Path, way: &Option<Option<std::path::PathBuf>>| match std::fs::symlink_metadata(full) {
        Ok(m) if m.is_file() && way == &Some(None) => Some(true),
        Ok(m) if m.is_file() || (m.file_type().is_symlink() && full.is_file()) => Some(judged(it, &leads_to(&it.base_dir, full))),
        _ => None,
    };
    let joined = |rel: &str| if rel.is_empty() { dir.to_string() } else { format!("{}/{}", dir.trim_end_matches('/'), rel) };
    // Judged before it is looked at: whether a folder outside is there is not said (Q-475).
    let led = if hidden || rel.split('/').any(|s| s == "..") { None } else { leads_to(&it.base_dir, &base.join(rel)) };
    let mut answer = None;
    let asked = |it: &mut Interp| match it.request_value() {
        Some(Value::Map(r)) => (r.get("path").map(|p| p.display()), r.get("query_string").map(|q| q.display()).filter(|q| !q.is_empty())),
        _ => (None, None),
    };
    // The path asked for, `p` as the page's links resolve against it: one `/` first, as `//docs/`
    // is another host to a browser, and so is `/\docs/` (FEEDBACK51.md, Q-671; branch
    // feedback51-fixes); written as a `Location` holds it (FEEDBACK55.md, Q-719; branch
    // feedback55).
    let moved = |p: &str, q: Option<String>, kept: Option<&str>| -> R {
        let to = format!("/{}{}", p.trim_start_matches(['/', '\\']), q.map_or(String::new(), |q| format!("?{}", q)));
        let mut headers = vec![("location".to_string(), location(&to).unwrap_or_else(|| to.clone()))];
        headers.extend(kept.map(|k| ("cache-control".to_string(), k.to_string())));
        Ok(Value::Response(Arc::new(Response { status: 301, headers, body: Body::Value(Value::Nil) })))
    };
    // Under `quillo serve`, which owns its URLs, a file asked for as a folder, `/a.txt/` or
    // `/about/`, has one URL, as a folder has: it is moved to its path without the `/` (for an
    // hour), as `/docs` is to `/docs/`, when it would be served there, and else it is the miss a
    // file not there is, which says nothing of a file kept back (FEEDBACK55.md, Q-721; branch
    // feedback55). A program's routes say what their URLs are, a `/` at the end too, so for them
    // `files` serves what is asked for as it did. A route's `*p` has no `/` at its end: the
    // request's path says, when it ends in the path given.
    let as_folder = |it: &mut Interp| {
        SERVES_FOLDER.load(std::sync::atomic::Ordering::Relaxed)
            && match it.request_value() {
                Some(Value::Map(r)) => matches!(r.get("path"), Some(Value::Str(p)) if p.ends_with('/')
                    && crate::server::percent_decode(p.trim_end_matches('/'), false).ends_with(&format!("/{}", rel))),
                _ => false,
            }
    };
    let unslashed = |it: &mut Interp, name: String| -> R {
        let served = matches!(file_as(it, vec![Value::str(name), Value::map(file_opts.clone())], line, Some(&base), true)?, Value::Response(r) if r.status == 200);
        match asked(it) {
            (Some(p), q) if served => moved(p.trim_end_matches('/'), q, Some("max-age=3600")),
            _ => Ok(not_found()),
        }
    };
    if judged(it, &led) {
        let target = base.join(rel);
        if target.is_dir() {
            // `/docs` is `/docs/`, so that the page's own relative links resolve.
            if let (Some(p), q) = asked(it) {
                if !p.ends_with('/') {
                    return moved(&format!("{}/", p), q, None);
                }
            }
            if found(it, &target.join(&index), &led) == Some(true) {
                answer = Some(serve(it, joined(&format!("{}/{}", rel, index).trim_start_matches('/').to_string()))?);
            }
        } else if target.is_file() {
            answer = Some(if as_folder(it) { unslashed(it, joined(rel))? } else { serve(it, joined(rel))? });
        } else if Path::new(rel).extension().is_none() {
            for ext in &extensions {
                match found(it, &base.join(format!("{}.{}", rel, ext)), &led) {
                    Some(true) if as_folder(it) => answer = Some(unslashed(it, joined(&format!("{}.{}", rel, ext)))?),
                    Some(true) => answer = Some(serve(it, joined(&format!("{}.{}", rel, ext)))?),
                    Some(false) => {}
                    None => continue,
                }
                break;
            }
        }
    }
    // A file kept back is a miss like another, with the folder's page for one (FEEDBACK55.md,
    // Q-701; branch feedback55).
    if let Some(a) = answer.filter(|a| !matches!(a, Value::Response(r) if r.status == 404)) {
        return Ok(a);
    }
    if let Some(page) = not_found_page.filter(|p| base.join(p).is_file() && judged(it, &leads_to(&it.base_dir, &base.join(p)))) {
        // The page whole, whatever the request's conditionals and range, which are of the URL
        // asked for: its 206 or 304 was thrown away for the plain page (FEEDBACK55.md, Q-722;
        // branch feedback55).
        if let Value::Response(r) = file_as(it, vec![Value::str(joined(&page)), Value::map(file_opts.clone())], line, Some(&base), true)? {
            if r.status == 200 {
                let mut r = (*r).clone();
                r.status = 404;
                // Not the page's validators: they are of another URL's answer. Nor `cache`'s
                // year: the file may be there tomorrow (FEEDBACK55.md, Q-715; branch feedback55).
                r.headers.retain(|(k, _)| !["etag", "last-modified", "accept-ranges", "cache-control"].contains(&k.as_str()));
                r.headers.push(("cache-control".into(), "no-cache".into()));
                return Ok(Value::Response(Arc::new(r)));
            }
        }
    }
    Ok(not_found())
}

/// Whether `files` keeps a path to itself: a part starting with `.` (but `.well-known` first),
/// or the program's own source. The name as the file system takes it: on macOS and Windows
/// `lib.QWE` is `lib.qwe`, and on Windows so are `lib.qwe.`, `lib.qwe ` and `lib.qwe::$DATA`
/// (FEEDBACK51.md, Q-656; branch feedback51-fixes).
pub(crate) fn hidden_path(rel: &str) -> bool {
    #[cfg(windows)]
    if rel.contains(['\\', ':']) || rel.split('/').any(short_name) {
        return true;
    }
    // Folded as the file system folds it: `ſ` is `s` and `K` (U+212A) `k` on macOS, so `plug.rſ`
    // was `plug.rs` (found in review).
    let last = rel.rsplit('/').next().unwrap_or("").trim_end_matches(['.', ' ']).to_uppercase().to_lowercase();
    rel.split('/').enumerate().any(|(i, seg)| seg.starts_with('.') && !(i == 0 && seg == ".well-known")) || last.ends_with(".qwe") || crate::native::is_rust(&last)
}

/// Whether `files`, serving the folder `served` of the program's folder `program` (both as the
/// system names `real`), keeps to itself `real`, where a link led or a file opened is: in the
/// folder served, a file `hidden_path` keeps by its own name, or one in a dot folder but a
/// package manager's store right under a `node_modules` (pnpm's `.pnpm`, Deno's `.deno`, Bun's
/// `.bun`, Yarn's `.store`, as `node_modules/htmx.org -> .pnpm/htmx.org@1.9.10/...` leads there)
/// or a build tool's output (`.vitepress/dist`, `.output/public`, `.next/static`,
/// `.svelte-kit/output/client`, `.nuxt/dist/client`, `.vercel/output/static`), and `.well-known`
/// at the top; out of it, in the program's, a path it keeps from there (`../.env`,
/// `../.git/config`). Every other dot folder may hold secrets: `.git`, `.ssh`, `.vscode`'s
/// `sftp.json`, `.config`'s credentials, `.docker`, `.kube`, `.cargo`, `.idea`, `.quillo`'s
/// cookies, a tool's server code beside its output, and `.cache`, where tools keep tokens too
/// (FEEDBACK55.md, Q-701; branch feedback55).
fn hidden_target(program: &Path, served: &Path, real: &Path) -> bool {
    const STORES: [&str; 4] = [".pnpm", ".deno", ".bun", ".store"];
    const TOOLS: [&str; 6] = [".vitepress/dist", ".output/public", ".next/static", ".svelte-kit/output/client", ".nuxt/dist/client", ".vercel/output/static"];
    if let Ok(rel) = real.strip_prefix(served) {
        let parts: Vec<String> = rel.components().map(|c| fold_name(name_as_taken(&c.as_os_str().to_string_lossy()))).collect();
        let Some((name, folders)) = parts.split_last() else { return false };
        let above = served.file_name().map(|n| fold_name(&n.to_string_lossy()));
        let store = |i: usize| STORES.contains(&folders[i].as_str()) && if i == 0 { above.as_deref() == Some("node_modules") } else { folders[i - 1] == "node_modules" };
        let tool = |i: usize| TOOLS.iter().any(|t| t.split('/').count() + i <= folders.len() && t.split('/').enumerate().all(|(k, seg)| folders[i + k] == seg));
        let kept = |(i, p): (usize, &String)| p.starts_with('.') && !(i == 0 && p == ".well-known") && !store(i) && !tool(i);
        return folders.iter().enumerate().any(kept) || hidden_path(name);
    }
    let Ok(rel) = real.strip_prefix(program) else { return true };
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    hidden_path(&parts.join("/"))
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
            // Too long to count is a bad duration (FEEDBACK16.md, Q-130; branch feedback20-fixes).
            n.checked_mul(per)
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
    let Some(full) = inside(it, it.base_dir.join(rel)) else {
        return fail(line, format!("`save_upload` needs a path inside the program's folder, and {:?} leads out of it through a link", &*dest));
    };
    if let Some(why) = own_file(it, &full, true) {
        return Ok(Value::err(Value::str(format!("save_upload: will not write {}: {}; save uploads under a folder of their own, such as uploads/", dest, why))));
    }
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

/// `site_headers({ "content-security-policy": "..." })`, or `site_headers("/admin", {...})` for
/// the answers under a path: headers sent with every answer that does not set them itself. A
/// null value takes away one Quillo sends by itself, `"x-frame-options": null` (FEEDBACK31.md,
/// Q-391; branch feedback27-35-fixes).
fn site_headers(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (prefix, m) = match args.as_slice() {
        [Value::Map(m)] => ("/".to_string(), m.clone()),
        [Value::Str(p), Value::Map(m)] if p.starts_with('/') => (p.trim_end_matches('/').to_string(), m.clone()),
        [Value::Str(p), _] if !p.starts_with('/') => return fail(line, format!("`site_headers` needs a path that starts with /, got {:?}", &**p)),
        _ => return fail(line, "`site_headers` takes a map of headers, or a path and a map: site_headers(\"/admin\", { \"cache-control\": \"no-store\" })"),
    };
    let prefix = if prefix.is_empty() { "/".to_string() } else { prefix };
    let mut hs = Vec::new();
    for (k, v) in m.iter() {
        let name = k.to_ascii_lowercase();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            return fail(line, format!("`site_headers`: {:?} is not a header's name", &**k));
        }
        if matches!(name.as_str(), "content-length" | "transfer-encoding" | "connection" | "set-cookie" | "content-type") {
            return fail(line, format!("`site_headers`: `{}` is the answer's own; a route sets it", name));
        }
        let value = match v {
            Value::Nil => None,
            // A `location` that runs code or carries a page, as `respond`'s (FEEDBACK55.md, Q-719;
            // branch feedback55; found in review).
            Value::Str(s) if name == "location" && runs_or_carries(s) => {
                return fail(line, format!("`site_headers`: `location` is not given a {} URL, got {}", carried_scheme(s).unwrap_or("data:"), v.repr()))
            }
            Value::Str(s) if !s.contains(['\r', '\n']) => Some(s.to_string()),
            Value::Int(_) | Value::Bool(_) => Some(v.display().to_string()),
            other => return fail(line, format!("`site_headers`: the value of `{}` is text on one line, or null, got {}", name, other.repr())),
        };
        hs.push((name, value));
    }
    Arc::make_mut(&mut it.site).headers.push((prefix, hs));
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

/// `new_tenant(name, f)`: `with_tenant` for a signup, which makes the tenant's database and
/// gives an error when there is one, where `with_tenant` would run `f` in the customer's
/// (FEEDBACK20.md, Q-165; branch feedback20-fixes).
fn new_tenant(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let name = s_arg(&args, 0, "new_tenant", line)?;
    if !crate::site::tenant_name_ok(&name) {
        return fail(line, format!("`new_tenant`: {:?} cannot be a tenant's name (lowercase letters, digits, - and _)", name));
    }
    if !matches!(args.get(1), Some(Value::Func(_) | Value::Builtin(..))) {
        return fail(line, format!("`new_tenant` expects a function second, got {}", args.get(1).map_or("nothing", |v| v.type_name())));
    }
    if !crate::db::claim_tenant(it, &name, line)? {
        return Ok(Value::own_err(Value::str(format!("there is already a tenant {:?}", &*name))));
    }
    with_tenant(it, args, line)
}

/// `redirect(url, status = 303)`. 303 makes browsers follow with a GET,
/// which is what you want after a form POST.
fn redirect(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let url = s_arg(&args, 0, "redirect", line)?;
    let status = if args.len() > 1 { i_arg(&args, 1, "redirect", line)? } else { 303 };
    // The statuses that redirect: a 300 is a choice for the visitor, a 304 says the copy held is
    // current, a 305 is gone from HTTP, and 309 to 399 mean nothing (FEEDBACK55.md, Q-719; branch
    // feedback55).
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return fail(line, format!("`redirect` status is 301, 302, 303, 307 or 308, got {}", status));
    }
    let status = status as u16;
    // A tab too, which a header may hold, and browsers take out of a URL: `https://good.com\t@evil.com`
    // is evil.com's (FEEDBACK55.md, Q-719; branch feedback55; found in review).
    if !crate::server::valid_header("location", &url) || url.contains('\t') {
        return fail(line, "`redirect` URL cannot contain line breaks, tabs or other control characters");
    }
    if let Some(scheme) = crate::interp::script_scheme(&url) {
        return fail(line, format!("`redirect` is not given a {} URL", scheme));
    }
    // Nor a page of its own (FEEDBACK55.md, Q-719; branch feedback55; found in review).
    if runs_or_carries(&url) {
        return fail(line, "`redirect` is not given a data: URL");
    }
    // A host past ASCII that IDNA cannot write is no host a browser goes to (FEEDBACK55.md, Q-719;
    // branch feedback55).
    let Some(to) = location(&url) else {
        return Ok(Value::err(Value::str(format!("`redirect`: the host of {:?} is no name a browser goes to", &*url))));
    };
    Ok(Value::Response(Arc::new(Response {
        status,
        headers: vec![("location".into(), to)],
        body: Body::Value(Value::Nil),
    })))
}

/// `url` as a `Location` holds it, going where a browser would have taken it: what a URL may not
/// hold (a space, `"`, `<`, `>`, `^`, a backquote, `{`, `|`, `}`, a lone `%`, any byte past
/// ASCII) percent-encoded, and the rest, a `%XX` already there too, as it is, where `/a b` and
/// `/a"><script>` went as written; a host past ASCII as IDNA writes it, `bücher.de` as
/// `xn--bcher-kva.de`, or None when IDNA cannot (FEEDBACK55.md, Q-719; branch feedback55). In a URL
/// of a scheme browsers read `\` in as `/` (http, https, ws, wss, ftp, file, and one with no
/// scheme), a `\` before the query is written `/`, as they read it: `https://good.com\@evil.com/`
/// is good.com's, which `%5C` would have made evil.com's (found in review).
pub(crate) fn location(url: &str) -> Option<String> {
    let fine = |c: u8| c.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=".contains(&c);
    let scheme = url.find(':').filter(|&i| i > 0 && url.as_bytes()[0].is_ascii_alphabetic() && url[..i].bytes().all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c)));
    let special = scheme.map_or(true, |i| ["http", "https", "ws", "wss", "ftp", "file"].iter().any(|s| url[..i].eq_ignore_ascii_case(s)));
    let before = url.find(['?', '#']).unwrap_or(url.len());
    let mut url: std::borrow::Cow<str> = if scheme.is_none() && url.starts_with('/') && url[..before].contains('\\') {
        // A path of the site's own, `/` first: a `\` in its leading run of `/` and `\` is written
        // `%5C`, so that the path stays on the site, as a check that it starts with `/` and not
        // `//` meant, and `--base` still prefixes it: `/\evil.com` written `//evil.com` left the
        // site (FEEDBACK55.md, Q-719; branch feedback55; found in review). The path's other `\`
        // are `/`, as browsers read them.
        let lead = url.bytes().take_while(|b| matches!(b, b'/' | b'\\')).count();
        format!("{}{}{}", url[..lead].replace('\\', "%5C"), url[lead..before].replace('\\', "/"), &url[before..]).into()
    } else if special && url[..before].contains('\\') {
        format!("{}{}", url[..before].replace('\\', "/"), &url[before..]).into()
    } else {
        url.into()
    };
    // A path of the site's own without its `.` and `..` segments (`%2e` too, as browsers read it),
    // so that one under `--base` cannot climb out of it: `/p\..\..\evil` was `/app/p/../../evil`
    // (RFC 3986 §5.2.4; FEEDBACK55.md, Q-719; branch feedback55; found in review).
    if scheme.is_none() && url.starts_with('/') && !url.starts_with("//") {
        let end = url.find(['?', '#']).unwrap_or(url.len());
        if url[..end].split('/').any(|seg| dots(seg) > 0) {
            url = format!("{}{}", without_dots(&url[..end]), &url[end..]).into();
        }
    }
    if url.bytes().all(fine) {
        return Some(url.into_owned());
    }
    let encode = |part: &str, out: &mut String| {
        let b = part.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            let escape = c == b'%' && b.get(i + 1..i + 3).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit));
            if fine(c) || escape {
                out.push(c as char);
            } else {
                out.push_str(&format!("%{:02X}", c));
            }
        }
    };
    // `scheme://authority` or `//authority`, the authority up to the path, query or fragment.
    let at = match scheme {
        Some(i) if url[i..].starts_with("://") => Some(i + 3),
        None if url.starts_with("//") => Some(2),
        _ => None,
    };
    let mut out = String::with_capacity(url.len() + 8);
    let rest = match at {
        Some(from) => {
            let to = url[from..].find(['/', '?', '#']).map_or(url.len(), |n| from + n);
            out.push_str(&url[..from]);
            let authority = &url[from..to];
            let (user, host_port) = authority.rsplit_once('@').map_or((None, authority), |(u, h)| (Some(u), h));
            if let Some(u) = user {
                encode(u, &mut out);
                out.push('@');
            }
            let (host, port) = match host_port.rsplit_once(':') {
                Some((h, p)) if !h.starts_with('[') && p.bytes().all(|c| c.is_ascii_digit()) => (h, Some(p)),
                _ => (host_port, None),
            };
            if host.is_ascii() {
                out.push_str(host);
            } else {
                let ascii = url::quirks::domain_to_ascii(host);
                if ascii.is_empty() {
                    return None;
                }
                out.push_str(&ascii);
            }
            if let Some(p) = port {
                out.push(':');
                out.push_str(p);
            }
            &url[to..]
        }
        None => &url[..],
    };
    encode(rest, &mut out);
    Some(out)
}

/// A path segment that is `.` (1) or `..` (2), in any of the forms browsers read so (`%2e`, `.%2E`),
/// else 0 (FEEDBACK55.md, Q-719; branch feedback55).
fn dots(seg: &str) -> u8 {
    if seg.is_empty() || seg.len() > 6 {
        return 0;
    }
    match seg.to_ascii_lowercase().as_str() {
        "." | "%2e" => 1,
        ".." | ".%2e" | "%2e." | "%2e%2e" => 2,
        _ => 0,
    }
}

/// `path` (starting `/`) without its dot segments, as RFC 3986 §5.2.4 removes them: `/a/b/../c` is
/// `/a/c`, `/a/..` is `/`, and a `..` past the root stays at it (FEEDBACK55.md, Q-719; branch
/// feedback55).
fn without_dots(path: &str) -> String {
    let segs: Vec<&str> = path[1..].split('/').collect();
    let mut out: Vec<&str> = Vec::with_capacity(segs.len());
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        match dots(seg) {
            0 => out.push(seg),
            n => {
                if n == 2 {
                    out.pop();
                }
                if last {
                    out.push("");
                }
            }
        }
    }
    format!("/{}", out.join("/"))
}

/// Whether a URL would run code or carry a page of its own when followed: `javascript:`,
/// `vbscript:` or `data:`, however browsers would read it (FEEDBACK55.md, Q-719; branch
/// feedback55).
pub(crate) fn runs_or_carries(url: &str) -> bool {
    carried_scheme(url).is_some()
}

/// Which of those schemes `url` has, for a refusal to name the one given: each said
/// "javascript:" (or all three), whatever it was given (FEEDBACK55.md; branch feedback55).
pub(crate) fn carried_scheme(url: &str) -> Option<&'static str> {
    let start: String = url.trim_start_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).take(5).collect();
    crate::interp::script_scheme(url).or_else(|| start.eq_ignore_ascii_case("data:").then_some("data:"))
}

/// `respond(status, body, headers = {})` builds a response value.
fn respond(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let status = i_arg(&args, 0, "respond", line)?;
    // A 1xx is not an answer: the client waits on for one that never comes, or takes the next
    // request's for it (FEEDBACK36.md, Q-433; branch feedback36-39).
    if (100..200).contains(&status) {
        return fail(line, format!("`respond` status must be 200-599, got {}: a 1xx is an interim answer, and the client would wait for the real one", status));
    }
    if !(200..=599).contains(&status) {
        return fail(line, format!("`respond` status must be 200-599, got {}", status));
    }
    let headers = if args.len() > 2 { m_arg(&args, 2, "respond", line)? } else { Arc::new(Map::new()) };
    let headers = answer_headers(&headers, "respond", line)?;
    Ok(Value::Response(Arc::new(Response {
        status: status as u16,
        headers,
        body: Body::Value(arg(&args, 1)),
    })))
}

/// The headers a program gives its answer (`respond`'s, `stream`'s and `events`'), names in lower
/// case. `null` is no such header, Quillo's own for it neither, as in `site_headers` (it holds
/// its place as `server::DROPPED`); a list is one header an item for `set-cookie`, and an error
/// for another header, as a map is: each went out as text, `x-content-type-options: null` and
/// `set-cookie: ["a=1", "b=2"]` (FEEDBACK55.md, Q-710; branch feedback55).
pub(crate) fn answer_headers(h: &Map, f: &str, line: u32) -> Result<Vec<(String, String)>, Ctrl> {
    let mut out = Vec::with_capacity(h.len());
    for (k, v) in h.iter() {
        let k = k.to_ascii_lowercase();
        match v {
            Value::Nil => out.push((k, crate::server::DROPPED.to_string())),
            Value::List(items) if k == "set-cookie" => {
                for item in items.iter() {
                    match item {
                        Value::List(_) | Value::Map(_) | Value::Nil => {
                            return fail(line, format!("`{}` header `set-cookie` takes a list of cookie lines, one a header, got {} in it", f, item.type_name()))
                        }
                        item => out.push((k.clone(), item.display())),
                    }
                }
            }
            Value::List(_) | Value::Map(_) => {
                let only = if matches!(v, Value::List(_)) { ": only `set-cookie` takes a list, one header an item; join the values with \", \"" } else { "" };
                return fail(line, format!("`{}` header `{}` is a str, got {}{}", f, k.escape_debug(), v.type_name(), only));
            }
            v => out.push((k, v.display())),
        }
    }
    // A `location` that runs code or carries a page is refused, as `redirect` refuses it: an app
    // went to it (FEEDBACK55.md, Q-719; branch feedback55; found in review).
    if let Some((_, v)) = out.iter().find(|(k, v)| k == "location" && runs_or_carries(v)) {
        return fail(line, format!("`{}` header `location` is not given a {} URL, got {}", f, carried_scheme(v).unwrap_or("data:"), Value::str(v.as_str()).repr()));
    }
    // Header text often comes from the request; a line break in it would
    // let the sender add headers (or a whole response) of their own.
    if let Some((k, _)) = out.iter().find(|(k, v)| !crate::server::valid_header(k, if v == crate::server::DROPPED { "" } else { v })) {
        return fail(line, format!("`{}` header `{}`: names are letters, digits and -, values cannot contain line breaks", f, k.escape_debug()));
    }
    Ok(out)
}

/// A content type a program gives (`file`'s and `stream`'s `type`), or an error that says what is
/// wrong with it: "`file`'s type is a str" was said of a str, and a control character dropped the
/// header as the answer was written (FEEDBACK55.md, Q-726; branch feedback55).
pub(crate) fn content_type(t: &str, f: &str, line: u32) -> Result<String, Ctrl> {
    if t.trim().is_empty() {
        return fail(line, format!("`{}`'s type is empty: give a content type, like \"text/csv\", or leave it out", f));
    }
    if !crate::server::valid_header("content-type", t) {
        return fail(line, format!("`{}`'s type cannot hold line breaks or other control characters, got {}", f, Value::str(t).repr()));
    }
    Ok(t.to_string())
}

fn json_parse(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "json.parse", line)?;
    Ok(match Value::parse_json(&s) {
        Ok(v) => v,
        Err(e) => Value::err(Value::str(format!("invalid JSON: {}", e))),
    })
}

/// The most JSON `json.stringify` and an answer make, and how deep (FEEDBACK49.md, Q-606 and
/// Q-592; branch main).
pub const JSON_MOST: usize = 256 << 20;
pub const JSON_DEEPEST: usize = 10_000;

fn json_stringify(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    // As `str` is (FEEDBACK26.md, Q-197; branch feedback26-fixes).
    let v = arg(&args, 0);
    let v = if crate::value::hiding() { crate::server::hidden(&v).unwrap_or(v) } else { v };
    // Written as it is held to its limits, with no tree of serde's made first (FEEDBACK49.md,
    // Q-606; found in review).
    let mut out = Vec::new();
    if let Err(why) = v.json_into_within(&mut out, JSON_MOST, JSON_DEEPEST) {
        return Ok(Value::err(Value::str(format!("json.stringify: {}", why))));
    }
    // SAFETY: only JSON text, which is UTF-8, was written.
    let text = unsafe { String::from_utf8_unchecked(out) };
    Ok(Value::str(if arg(&args, 1).truthy() { indented(&text) } else { text }))
}

/// Compact JSON text indented by two spaces, as serde_json's pretty writer has it: `[]` and
/// `{}` stay as they are, and a string's own brackets and commas are text.
fn indented(json: &str) -> String {
    let b = json.as_bytes();
    let mut out = String::with_capacity(json.len() * 2);
    let mut level = 0usize;
    let newline = |out: &mut String, level: usize| {
        out.push('\n');
        for _ in 0..level {
            out.push_str("  ");
        }
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                out.push_str(&json[start..=i.min(b.len() - 1)]);
            }
            b'[' | b'{' if b.get(i + 1) == Some(&if c == b'[' { b']' } else { b'}' }) => {
                out.push(c as char);
                out.push(b[i + 1] as char);
                i += 1;
            }
            b'[' | b'{' => {
                out.push(c as char);
                level += 1;
                newline(&mut out, level);
            }
            b']' | b'}' => {
                level = level.saturating_sub(1);
                newline(&mut out, level);
                out.push(c as char);
            }
            b',' => {
                out.push(',');
                newline(&mut out, level);
            }
            b':' => out.push_str(": "),
            _ => {
                // The rest of a number, `true`, `false` or `null`.
                let start = i;
                while i + 1 < b.len() && !matches!(b[i + 1], b',' | b']' | b'}' | b':') {
                    i += 1;
                }
                out.push_str(&json[start..=i]);
            }
        }
        i += 1;
    }
    out
}

// ---- files -------------------------------------------------------------

/// Folders outside the program's own that its files may be in: `quillo run --allow-path DIR`,
/// for a tool its owner starts from a shell (FEEDBACK29.md, Q-357; branch feedback29-fixes).
/// Named, never assumed: the same program may serve strangers tomorrow.
static ALLOWED_PATHS: std::sync::RwLock<Vec<std::path::PathBuf>> = std::sync::RwLock::new(Vec::new());

/// Adds a folder for `--allow-path`, as it really is (links followed); why not, if not.
pub fn allow_path(dir: &str) -> Result<(), String> {
    let real = Path::new(dir).canonicalize().map_err(|e| format!("{}: {}", dir, e))?;
    if !real.is_dir() {
        return Err(format!("{} is not a folder", dir));
    }
    ALLOWED_PATHS.write().unwrap_or_else(std::sync::PoisonError::into_inner).push(real);
    Ok(())
}

/// A path under the program's folder; `..` and absolute paths are refused, unless they
/// lead into a folder `--allow-path` named.
pub(crate) fn local_path(it: &Interp, p: &str) -> Option<std::path::PathBuf> {
    let rel = Path::new(p);
    if p.is_empty() {
        return None;
    }
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return allowed_path(it, p);
    }
    inside(it, it.base_dir.join(rel))
}

/// `full` if, links followed, it is still in the program's folder (or one `--allow-path`
/// named): a link inside the folder may lead anywhere, and a cloned repository or an unpacked
/// archive can bring one (FEEDBACK41.md, Q-475; branch feedback40-43).
pub(crate) fn inside(it: &Interp, full: std::path::PathBuf) -> Option<std::path::PathBuf> {
    if stays_in(&it.base_dir, &full) {
        return Some(full);
    }
    let dirs = ALLOWED_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    let real = real_path(&full);
    if real.as_ref().is_some_and(|r| dirs.iter().any(|d| r.starts_with(d))) {
        return Some(full);
    }
    // Said once, as a deploy's `uploads -> ../shared/uploads` is then a 404 with no other word.
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let to = real.map_or_else(|| "nowhere (the link is dangling)".to_string(), |r| r.display().to_string());
        eprintln!(
            "quillo: {} leads out of the program's folder through a link, to {}, and is refused; `--allow-path FOLDER` lets the program reach a folder outside",
            full.strip_prefix(&*it.base_dir).unwrap_or(&full).display(),
            to
        );
    }
    None
}

/// Whether `full`, a path under `root`, stays under it with its links followed. Each part is
/// looked at (`lstat`, about a microsecond), and only a path with a link in it is resolved,
/// which costs ten times that: a static file costs what it did. A link that leads nowhere does
/// not stay: writing through it would make its target, wherever that is.
pub(crate) fn stays_in(root: &Path, full: &Path) -> bool {
    leads_to(root, full).is_some()
}

/// Where `full`, a path under `root`, leads, as `stays_in` finds it: `Some(None)` when no part
/// of it is a link, `Some(Some(real))` when a link in it leads to `real`, under `root`, and None
/// when it leads out of `root` or nowhere. What a link leads to is then judged as its name is
/// (FEEDBACK55.md, Q-701; branch feedback55).
pub(crate) fn leads_to(root: &Path, full: &Path) -> Option<Option<std::path::PathBuf>> {
    let resolved = || match (real_path(full), root.canonicalize()) {
        (Some(real), Ok(base)) if real.starts_with(&base) => Some(Some(real)),
        _ => None,
    };
    let Ok(rel) = full.strip_prefix(root) else { return resolved() };
    let mut at = root.to_path_buf();
    for c in rel.components() {
        at.push(c);
        match std::fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => return resolved(),
            Ok(_) => {}
            // What follows is not there: no link in it.
            Err(_) => return Some(None),
        }
    }
    Some(None)
}

/// Where `p` really is: the nearest part of it that is there, resolved, and the names after it.
/// None for a link that leads nowhere.
fn real_path(p: &Path) -> Option<std::path::PathBuf> {
    let mut at = p;
    let mut rest = Vec::new();
    let real = loop {
        if let Ok(r) = at.canonicalize() {
            break r;
        }
        if std::fs::symlink_metadata(at).is_ok() {
            return None;
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    };
    Some(rest.iter().rev().fold(real, |r, name| r.join(name)))
}

/// `p` as it really is, if that is inside a folder `--allow-path` named. A file not made yet
/// is where the nearest folder above it that is there really is, and what follows that is
/// plain names: a `..` past it is refused.
fn allowed_path(it: &Interp, p: &str) -> Option<std::path::PathBuf> {
    let dirs = ALLOWED_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    if dirs.is_empty() {
        return None;
    }
    let full = if Path::new(p).is_absolute() { std::path::PathBuf::from(p) } else { it.base_dir.join(p) };
    let mut at = full.as_path();
    let mut rest = Vec::new();
    let real = loop {
        if let Ok(r) = at.canonicalize() {
            break r;
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    };
    let real = rest.iter().rev().fold(real, |r, name| r.join(name));
    dirs.iter().any(|d| real.starts_with(d)).then_some(real)
}

/// Why `full` must not be written by the program, if it is one of its own files: a `.qwe` or
/// `.qwer` it loaded, which `--reload` would run, a `.env`, the SQLite database it has open
/// with its journal, or a migration (FEEDBACK30.md, Q-361; branch feedback27-35-fixes). An
/// upload saved under the visitor's name at the folder's root could otherwise replace
/// `main.qwe` and run as the program.
pub(crate) fn own_file(it: &Interp, full: &Path, upload: bool) -> Option<&'static str> {
    let real = real_as_made(full);
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The name as Windows takes it: `.env::$DATA` is `.env` (FEEDBACK55.md, Q-704; branch
    // feedback55). The file of the name as written is judged too: `x:y` may be a hard link.
    let taken = name_as_taken(&name);
    let target = if taken.len() == name.len() || taken.is_empty() { real.clone() } else { real.with_file_name(taken) };
    let both = if target == real { vec![&real] } else { vec![&real, &target] };
    // In the case the file system ignores: on macOS `.ENV` was written, and read as `.env` at the
    // next start, and so `.ENV.LOCAL`, `data/APP.DB-JOURNAL` and an upload to `MIGRATIONS/`; the
    // same names are refused where case counts (FEEDBACK55.md, Q-704; branch feedback55).
    let folded_path = |p: &Path| std::path::PathBuf::from(fold_name(&p.to_string_lossy()));
    // The file, or a folder that holds one (`delete_file("lib")`).
    let folded_target = folded_path(&target);
    if it.loaded.iter().any(|f| folded_path(&real_as_made(f)).starts_with(&folded_target)) {
        return Some(OWN_SOURCE);
    }
    let folded = fold_name(taken);
    if folded == ".env" || folded.starts_with(".env.") {
        return Some(OWN_SETTINGS);
    }
    // One of them under another name: a second hard link writes into it (FEEDBACK55.md, Q-701;
    // branch feedback55).
    for f in &both {
        if let Some(why) = std::fs::metadata(f).ok().and_then(|m| own_meta(&m)) {
            return Some(why);
        }
    }
    // What a path of the settings or the TLS key leads to now, by any name (FEEDBACK55.md, Q-701;
    // branch feedback55).
    if let Some(why) = both.iter().find_map(|f| std::fs::metadata(f).ok().and_then(|m| crate::statics::id_of(&m)).and_then(named_id)) {
        return Some(why);
    }
    // And the paths themselves, while nothing is there: `--env-file`'s and `--tls-key`'s, as
    // given or as they lead (FEEDBACK55.md, Q-701; branch feedback55).
    let named: Vec<(std::path::PathBuf, &'static str)> = own().named.clone();
    if let Some((_, why)) = named.iter().find(|(n, _)| [n.clone(), real_as_made(n)].iter().any(|n| both.iter().any(|f| fold_name(&n.to_string_lossy()) == fold_name(&f.to_string_lossy())))) {
        return Some(why);
    }
    // On Windows, a short name may be any file's (FEEDBACK55.md, Q-701; branch feedback55).
    #[cfg(windows)]
    if full.strip_prefix(&*it.base_dir).unwrap_or(full).components().any(|c| short_name(&c.as_os_str().to_string_lossy())) {
        return Some("a short name Windows gives a file may be one of the program's own files");
    }
    // A database it has open, by any name, and the folder holding one (FEEDBACK55.md, Q-702 and
    // Q-705; branch feedback55).
    if let Some(why) = both.iter().find_map(|f| crate::db::database_file(f)) {
        return Some(why);
    }
    if crate::db::opened_under(&folded_target) {
        return Some("it holds the database the program has open");
    }
    // A program may write its own migrations, and its tenants' databases; a visitor's upload may
    // not, though the tenant's has not been opened since the start (FEEDBACK55.md, Q-702; branch
    // feedback55).
    if upload && folded_target.starts_with(folded_path(&real_as_made(&it.base_dir).join("migrations"))) {
        return Some("it is in the program's migrations");
    }
    if upload && crate::db::tenant_file(&folded_target) {
        return Some("it is where a tenant's database is");
    }
    None
}

/// Where `p` is: the nearest part of it that is there, as it really is, and the names after it,
/// so a folder made later is judged the same as one made before (`own_file`'s, shared with `db`
/// for its tenants' folders: FEEDBACK55.md, Q-702; branch feedback55).
pub(crate) fn real_as_made(p: &Path) -> std::path::PathBuf {
    let mut rest = Vec::new();
    let mut at = p;
    loop {
        if let Ok(r) = at.canonicalize() {
            return rest.iter().rev().fold(r, |r: std::path::PathBuf, n: &std::ffi::OsString| r.join(n));
        }
        match (at.parent(), at.file_name()) {
            (Some(up), Some(n)) => {
                rest.push(n.to_os_string());
                at = up;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// `local_path(next)`: an address on this site, as a browser would read it, or null. Tabs and
/// line breaks are dropped and `\` is `/`, as browsers do, and anything with a scheme or a host
/// (`//evil.example`, `/\evil.example`, `https:...`) is null, so `redirect(local_path(next) ??
/// "/")` cannot send a visitor to another site (FEEDBACK31.md, Q-390; branch feedback27-35-fixes).
pub fn local_target(s: &str) -> Option<String> {
    let s: String = s.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).map(|c| if c == '\\' { '/' } else { c }).collect();
    let s = s.trim_matches(|c: char| c <= ' ');
    if !s.starts_with('/') || s.starts_with("//") || s.chars().any(|c| c.is_control()) {
        return None;
    }
    // A `:` before the path's first `/`, `?` or `#` would be a scheme; none can be here, as it
    // starts with `/`, but `/%2F/x` decodes to `//x` in some servers: refused too.
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("/%2f") || lower.starts_with("/%5c") {
        return None;
    }
    Some(s.to_string())
}

fn local_path_fn(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match args.first() {
        Some(Value::Str(s)) => local_target(s).map_or(Value::Nil, Value::str),
        _ => Value::Nil,
    })
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
    if let Some(why) = own_file(it, &full, false) {
        return Ok(Value::err(Value::str(format!("write_file: will not write {}: {}", p, why))));
    }
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
    // The file checked is read as Quillo too, as what it uses is: not `.env` (FEEDBACK55.md,
    // Q-703; branch feedback55). One in a folder `--allow-path` named is the program's to name.
    let plain = Path::new(&*p).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if plain && !crate::check::confined_reads(&it.base_dir, &p, &full, false) {
        return Ok(Value::err(Value::str(format!("check_program: {}: {}", crate::check::CONFINED_READS, p))));
    }
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
    if let Some(why) = own_file(it, &full, false) {
        return Ok(Value::err(Value::str(format!("delete_file: will not delete {}: {}", p, why))));
    }
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
/// `defer(x)`: `x`. As a component's `state` initializer, the server sends the page
/// first and the component after it (src/defer.rs; branch defer).
fn defer(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(args.into_iter().next().unwrap_or(Value::Nil))
}

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
    if let Some(Value::Dec(_)) = args.get(i) {
        return fail(line, format!("`{}` expects a number as argument {}, got dec{}", f, i + 1, crate::interp::FLOAT_ONLY));
    }
    want(args, i, f, "a number", line, |v| v.as_f64())
}

fn abs(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::Dec(d) => Ok(Value::Dec(d.abs())),
        Value::Int(n) => n.checked_abs().map(Value::Int).ok_or_else(|| Ctrl::Error(Box::new(crate::QuilloError::new("integer overflow", line)))),
        _ => Ok(Value::Float(num_arg(&args, 0, "abs", line)?.abs())),
    }
}

/// What `round`, `floor` and `ceil` give for a float made whole: the int, wherever one holds it.
/// Past 9e15 it was a float, so `type_of(round(1e16))` said float though `round(x)` is an int:
/// the browser has ints past 2^53 now (Q-776), as the server (FEEDBACK55.md, Q-788 and Q-789;
/// branch feedback55). Past 2^63, and inf or NaN, the float.
fn to_int_or_float(f: f64) -> Value {
    if (-9223372036854775808.0..9223372036854775808.0).contains(&f) {
        Value::Int(f as i64)
    } else {
        Value::Float(f)
    }
}

/// A decimal rounded: `round(d, digits = 0, mode = "half_even")`. It keeps
/// exactly `digits` decimals (`round(12.5m, 2)` is 12.50).
fn round_dec(d: crate::decimal::Dec, args: &[Value], line: u32) -> R {
    use crate::decimal::Rounding;
    let digits = match args.get(1) {
        None | Some(Value::Nil) => 0,
        Some(Value::Int(n)) => (*n).clamp(-28, 28) as i32,
        Some(other) => return fail(line, format!("`round` expects an int as argument 2, got {}", other.type_name())),
    };
    let mode = match args.get(2) {
        None | Some(Value::Nil) => Rounding::HalfEven,
        Some(Value::Str(s)) => match Rounding::parse(s) {
            Some(m) => m,
            None => return fail(line, format!("`round`: unknown rounding {:?} (one of {})", &**s, Rounding::NAMES)),
        },
        Some(other) => return fail(line, format!("`round` expects a str as argument 3, got {}", other.type_name())),
    };
    d.round(digits, mode).map(Value::Dec).map_err(|e| Ctrl::Error(Box::new(crate::QuilloError::new(e.message(), line))))
}

/// `round(x)` gives an int; `round(x, digits)` a float with that many
/// decimals. A decimal stays one (`round_dec`).
fn round(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if let Value::Dec(d) = arg(&args, 0) {
        return round_dec(d, &args, line);
    }
    if !matches!(arg(&args, 2), Value::Nil) {
        return fail(line, "`round` takes a rounding mode for a dec only: convert with dec(x)");
    }
    // An int rounded to `digits` places is itself, and to fewer than none, a multiple of a power
    // of ten, exactly: it went through a float, `round(9007199254740993, 2)` another number
    // (FEEDBACK55.md, Q-789; branch feedback55).
    if let (Value::Int(n), true) = (arg(&args, 0), args.len() > 1) {
        return round_int(n, i_arg(&args, 1, "round", line)?, line);
    }
    let x = num_arg(&args, 0, "round", line)?;
    if args.len() > 1 {
        let d = i_arg(&args, 1, "round", line)?;
        // Fewer than no digits: to tens, hundreds, as a decimal has them; they were taken as 0
        // (FEEDBACK55.md, Q-789; branch feedback55).
        if d < 0 {
            return Ok(Value::Float(round_left(x, d, line)?));
        }
        let d = d.min(15) as i32;
        // From the float's shortest decimal, a tie away from zero, as `format_number` rounds:
        // `round(1.005, 2)` was 1.0, as 1.005 * 100 is 100.49999... in binary, while
        // `format_number` wrote 1.01 (FEEDBACK50.md, Q-626; branch feedback50-fixes). Away from
        // a tie the binary product rounds the same, and an int over a power of ten is the
        // nearest float to the decimal: that is the quick way, and gives no -0. Below 1e9 the
        // product is within 4e-7 of the decimal's, inside the window (found in review).
        let p = 10f64.powi(d);
        let y = x * p;
        let near_tie = y.abs() >= 1.0e9 || ((y - y.trunc()).abs() - 0.5).abs() < 1e-6;
        if !near_tie {
            return Ok(Value::Float(y.round() / p + 0.0));
        }
        if let Some(r) = crate::decimal::Dec::from_f64(x).and_then(|v| v.round(d, crate::decimal::Rounding::HalfUp).ok()) {
            return Ok(Value::Float(r.to_f64()));
        }
        // Past what a decimal holds: as before, but `x` itself where `x * p` is past a float.
        if !y.is_finite() {
            return Ok(Value::Float(x));
        }
        return Ok(Value::Float((x * p).round() / p));
    }
    Ok(whole_of(&args).unwrap_or_else(|| to_int_or_float(x.round())))
}

/// `floor(a / b)` (or `ceil`, `round`, `math.floor`, `math.ceil`, `math.trunc` of it) of two ints,
/// in ints: what is left over decides, a half away from zero for `round` (FEEDBACK55.md, Q-788;
/// branch feedback55). The smallest int over -1 is past an int.
pub(crate) fn int_div_rounded(f: &str, a: i64, b: i64, line: u32) -> R {
    if b == 0 {
        return fail(line, "division by zero");
    }
    let (a, b) = (a as i128, b as i128);
    let (q, r) = (a / b, a % b);
    let q = match f {
        "floor" | "math.floor" if r != 0 && (r < 0) != (b < 0) => q - 1,
        "ceil" | "math.ceil" if r != 0 && (r < 0) == (b < 0) => q + 1,
        "round" if r != 0 && 2 * r.abs() >= b.abs() => q + if (a < 0) == (b < 0) { 1 } else { -1 },
        _ => q,
    };
    i64::try_from(q).map(Value::Int).or_else(|_| fail(line, "integer overflow"))
}

/// How many arguments the builtin `name` takes at most, from check.rs's table (`usize::MAX`: any
/// number, or one the table does not have), for each call to be held to (FEEDBACK55.md, Q-786;
/// branch feedback55). Asked by where the name is, in a table of the names seen: a load and a
/// compare, which a call of a builtin does not notice.
#[inline(always)]
pub(crate) fn most_args(name: &'static str) -> usize {
    use std::sync::atomic::Ordering::Relaxed;
    let p = name.as_ptr() as u64;
    let at = most_slot(p);
    let e = MOST[at].load(Relaxed);
    if e >> 8 == p {
        return if e & 0xff == 0xff { usize::MAX } else { (e & 0xff) as usize };
    }
    let e = MOST[(at + 1) & (MOST.len() - 1)].load(Relaxed);
    if e >> 8 == p {
        return if e & 0xff == 0xff { usize::MAX } else { (e & 0xff) as usize };
    }
    most_args_found(name, at)
}

static MOST: [std::sync::atomic::AtomicU64; 1024] = [const { std::sync::atomic::AtomicU64::new(0) }; 1024];

/// Slots a name is looked for in after its own, when another name has that one.
const MOST_PROBES: usize = 8;

#[inline(always)]
fn most_slot(p: u64) -> usize {
    ((p >> 3) ^ (p >> 11)) as usize & (MOST.len() - 1)
}

/// A name not in its slot: in one of the next, or else found and kept in the first free one.
/// Two names of one slot took it from each other at each call, each then asking check.rs's
/// table, a hash of the name: which two did was a matter of where the linker put them, and
/// one build's loop of builtin calls was a tenth slower for it (FEEDBACK55.md, string speed;
/// branch feedback55).
#[cold]
#[inline(never)]
fn most_args_found(name: &'static str, at: usize) -> usize {
    use std::sync::atomic::Ordering::Relaxed;
    let p = name.as_ptr() as u64;
    let decode = |e: u64| if e & 0xff == 0xff { usize::MAX } else { (e & 0xff) as usize };
    for k in 2..MOST_PROBES {
        let e = MOST[(at + k) & (MOST.len() - 1)].load(Relaxed);
        if e >> 8 == p {
            return decode(e);
        }
        if e == 0 {
            break;
        }
    }
    let n = crate::check::most_args(name);
    // Where a pointer has its top byte, nothing is kept (and the table is asked each time).
    if p >> 56 == 0 {
        let e = p << 8 | n.map_or(0xff, |n| n.min(0xfe) as u64);
        for k in 0..MOST_PROBES {
            let slot = &MOST[(at + k) & (MOST.len() - 1)];
            match slot.compare_exchange(0, e, Relaxed, Relaxed) {
                Ok(_) => break,
                Err(had) if had == e => break,
                Err(_) => {}
            }
        }
    }
    n.unwrap_or(usize::MAX)
}

/// `round(n, digits)` of an int: itself for `digits` from 0, else the nearest multiple of
/// 10^-digits, a half away from zero as a float's is; past what an int holds, the overflow.
fn round_int(n: i64, digits: i64, line: u32) -> R {
    if digits >= 0 {
        return Ok(Value::Int(n));
    }
    let p = 10i128.pow(digits.unsigned_abs().min(20) as u32);
    let q = (n.unsigned_abs() as i128 + p / 2) / p * p;
    i64::try_from(if n < 0 { -q } else { q }).map(Value::Int).or_else(|_| fail(line, "integer overflow"))
}

/// `round(x, digits)` of a float with `digits` below 0: to the nearest multiple of 10^-digits, a
/// half away from zero, from its shortest decimal as `round` to places is, worked out on those
/// digits and made the float nearest to it: exact at every `digits`. Past a decimal's 28 digits it
/// went through floats, and `round(1e30, -29)` was 999999999999999900000000000000 (the user's
/// rule: a number is exact or an error; FEEDBACK55.md, Q-789; found writing the docs). Past the
/// largest float, an error, as Python's.
pub(crate) fn round_left(x: f64, digits: i64, line: u32) -> Result<f64, Ctrl> {
    if !x.is_finite() || x == 0.0 {
        return Ok(x + 0.0);
    }
    // x = 0.d1d2...dk × 10^n, its shortest digits.
    let text = crate::value::shortest_e(x.abs());
    let (mant, exp) = text.split_once('e').unwrap_or((&text, "0"));
    let mut ds: Vec<u8> = mant.bytes().filter(|c| c.is_ascii_digit()).collect();
    let n = exp.parse::<i128>().unwrap_or(0) + 1;
    let m = digits.unsigned_abs() as i128;
    let keep = n - m;
    let kept: Vec<u8> = if keep < 0 {
        return Ok(0.0);
    } else if keep as usize >= ds.len() {
        // A multiple of 10^m already.
        return Ok(x);
    } else {
        let up = ds[keep as usize] >= b'5';
        ds.truncate(keep as usize);
        if up {
            // One more in the last place kept, carried.
            let mut i = ds.len();
            loop {
                if i == 0 {
                    ds.insert(0, b'1');
                    break;
                }
                i -= 1;
                if ds[i] == b'9' {
                    ds[i] = b'0';
                } else {
                    ds[i] += 1;
                    break;
                }
            }
        }
        ds
    };
    if kept.is_empty() {
        return Ok(0.0);
    }
    // SAFETY: digits only.
    let digits_text = unsafe { std::str::from_utf8_unchecked(&kept) };
    let r: f64 = format!("{}e{}", digits_text, m).parse().unwrap_or(f64::INFINITY);
    if r.is_infinite() {
        return fail(line, "`round`: the rounded value is past the largest float");
    }
    Ok(if x < 0.0 { -r } else { r } + 0.0)
}

/// An int, which `round`, `floor` and `ceil` give back as it is: past 2^53 it went through a
/// float, `ceil(9007199254740993)` a float of another number, where the browser gives the int
/// (FEEDBACK55.md, Q-776; branch feedback55).
fn whole_of(args: &[Value]) -> Option<Value> {
    matches!(args.first(), Some(Value::Int(_))).then(|| args[0].clone())
}

fn floor(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if let Value::Dec(d) = arg(&args, 0) {
        return round_dec(d, &[Value::Nil, Value::Int(0), Value::str("floor")], line);
    }
    if let Some(n) = whole_of(&args) {
        return Ok(n);
    }
    Ok(to_int_or_float(num_arg(&args, 0, "floor", line)?.floor()))
}

fn ceil(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if let Value::Dec(d) = arg(&args, 0) {
        return round_dec(d, &[Value::Nil, Value::Int(0), Value::str("ceiling")], line);
    }
    if let Some(n) = whole_of(&args) {
        return Ok(n);
    }
    Ok(to_int_or_float(num_arg(&args, 0, "ceil", line)?.ceil()))
}

fn sqrt(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::Float(num_arg(&args, 0, "sqrt", line)?.sqrt()))
}

/// `pow(x, y)`: an int when both are ints and y >= 0 (and it fits), else a float.
fn pow(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // A decimal to an int's power, exactly (FEEDBACK50.md, Q-632; branch feedback50-fixes).
    if let Value::Dec(x) = arg(&args, 0) {
        return match arg(&args, 1) {
            Value::Int(n) => x.powi(n).map(Value::Dec).or_else(|e| fail(line, e.message())),
            other => fail(line, format!("`pow` takes a dec to an int's power, got {} as the power: for a fraction of a power, float(x) and math.pow", other.type_name())),
        };
    }
    if let (Value::Int(x), Value::Int(y)) = (arg(&args, 0), arg(&args, 1)) {
        // As `*` does: a float this size prints digits that were made up
        // (FEEDBACK29.md, Q-351; branch feedback29-fixes). `math.pow` is the float.
        if let Ok(y) = u32::try_from(y) {
            return match x.checked_pow(y) {
                Some(r) => Ok(Value::Int(r)),
                None => fail(line, "integer overflow"),
            };
        }
        // A power from 2^32 up: the int it is for -1, 0 and 1, the overflow for any other base;
        // it was a float, `pow(-1, 9007199254740993)` 1.0 and `pow(2, 4294967296)` inf
        // (FEEDBACK55.md, Q-790; branch feedback55).
        if y > 0 {
            return match x {
                0 | 1 => Ok(Value::Int(x)),
                -1 => Ok(Value::Int(if y % 2 == 0 { 1 } else { -1 })),
                _ => fail(line, "integer overflow"),
            };
        }
    }
    Ok(Value::Float(num_arg(&args, 0, "pow", line)?.powf(num_arg(&args, 1, "pow", line)?)))
}

// ---- more lists --------------------------------------------------------

fn unique(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let l = l_arg(&args, 0, "unique", line)?;
    // In time linear in the list, keeping what `==` keeps (FEEDBACK55.md, Q-791; branch
    // feedback55): see `value::unique_of`.
    Ok(Value::list(crate::value::unique_of(&l)))
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
        match it.call1(&f, x.clone(), line)? {
            e @ Value::Err(..) => return Ok(e),
            r if r.truthy() => n += 1,
            _ => {}
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
        // The function's error is given back, as `filter`'s is: its text was a key
        // (FEEDBACK55.md, Q-784; branch feedback55).
        let k = match it.call1(&f, x.clone(), line)? {
            e @ Value::Err(..) => return Ok(e),
            k => k.key_text(),
        };
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

/// `markdown_page(text, { title, lang, html })`: Markdown as a whole page, with a doctype and a
/// `<head>`, which a fragment lacks, so a browser showed it in quirks mode; its title is the
/// option's, or the text's first heading (FEEDBACK36.md, Q-451; branch feedback36-39).
fn markdown_page(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let text = s_arg(&args, 0, "markdown_page", line)?;
    let opts = match args.get(1) {
        None | Some(Value::Nil) => Arc::new(Map::new()),
        Some(Value::Map(m)) => m.clone(),
        Some(v) => return fail(line, format!("`markdown_page` expects options as a map, got {}", v.type_name())),
    };
    for k in opts.keys() {
        if !["title", "lang", "html"].contains(&&**k) {
            return fail(line, format!("`markdown_page` has no option `{}`: it takes `title`, `lang` and `html`", k));
        }
    }
    let heading = || markdown_title(&text, opts.get("html").is_some_and(|v| v.truthy()));
    let title = match opts.get("title") {
        Some(Value::Str(t)) => t.to_string(),
        None | Some(Value::Nil) => heading().unwrap_or_default(),
        Some(v) => return fail(line, format!("`markdown_page`'s title is a str, got {}", v.repr())),
    };
    let lang = match opts.get("lang") {
        Some(Value::Str(l)) if !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => format!(" lang=\"{}\"", l),
        None | Some(Value::Nil) => String::new(),
        Some(v) => return fail(line, format!("`markdown_page`'s lang is a language tag such as \"en\", got {}", v.repr())),
    };
    let mut inner = vec![Value::str(&*text)];
    if let Some(h) = opts.get("html") {
        inner.push(Value::map([("html".to_string(), h.clone())].into_iter().map(|(k, v)| (k.into(), v)).collect()));
    }
    let body = markdown(it, inner, line)?.display();
    Ok(Value::Html(format!(
        "<!DOCTYPE html><html{}><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title></head><body>{}</body></html>",
        lang,
        crate::interp::escape(&title),
        body
    ).into()))
}

/// The text of the first level-1 heading with any, `# Title` or a setext one (`Title` over `===`),
/// as `markdown` shows it: its words without their marks (`*Emph*` is Emph), HTML in it as the text
/// it is shown as unless `html` keeps it, and none from a `#` line in a code block or a comment, a
/// closing `#`, or a heading inside a quote or a list (FEEDBACK55.md, Q-831; branch feedback55).
fn markdown_title(text: &str, html: bool) -> Option<String> {
    use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
    let (mut depth, mut title) = (0usize, None::<String>);
    for e in Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS) {
        match (e, title.as_mut()) {
            (Event::Start(Tag::BlockQuote(_) | Tag::List(_)), None) => depth += 1,
            (Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_)), None) => depth -= 1,
            (Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }), None) if depth == 0 => title = Some(String::new()),
            (Event::End(TagEnd::Heading(_)), Some(t)) => match t.trim() {
                "" => title = None,
                words => return Some(words.to_string()),
            },
            (Event::Text(w) | Event::Code(w), Some(t)) => t.push_str(&w),
            (Event::InlineHtml(w), Some(t)) if !html => t.push_str(&w),
            (Event::SoftBreak | Event::HardBreak, Some(t)) => t.push(' '),
            _ => {}
        }
    }
    None
}

/// `markdown(text)`: Markdown as HTML (CommonMark, with tables,
/// strikethrough and task lists). HTML written in the text is shown as text,
/// and `javascript:` links are blocked, so it is safe for what visitors
/// write; `markdown(text, { html: true })` keeps the HTML, for text you
/// wrote yourself.
pub(crate) fn markdown(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
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
    // Past `--max-compute` in a block's highlighting: the rest are not highlighted, and it fails
    // (FEEDBACK55.md, Q-706; branch feedback55).
    let over = std::cell::Cell::new(false);
    let over = &over;
    let events = events.filter_map(move |e| match (e, code.as_mut()) {
        // The language is the info's first word, to a comma: `rust,ignore` is Rust
        // (FEEDBACK55.md, Q-834; branch feedback55).
        (Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(lang))), None) if !lang.trim().is_empty() => {
            code = Some((lang.split(|c: char| c.is_whitespace() || c == ',').find(|w| !w.is_empty()).unwrap_or("").to_string(), String::new()));
            None
        }
        (Event::Text(t), Some((_, body))) => {
            body.push_str(&t);
            None
        }
        (Event::End(pulldown_cmark::TagEnd::CodeBlock), Some(_)) => {
            let (lang, body) = code.take().unwrap();
            let class: String = lang.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            let html = if over.get() { None } else { crate::highlight::highlight_within(&body, &lang) };
            over.set(html.is_none());
            html.map(|h| Event::Html(CowStr::from(format!("<pre class=\"hl\"><code class=\"language-{}\">{}</code></pre>\n", class, h))))
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
    if over.get() {
        return fail(line, crate::httpd::over_compute());
    }
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
    // A long text counts against `--max-compute` (FEEDBACK55.md, Q-706; branch feedback55).
    match crate::highlight::highlight_within(&code, &lang) {
        Some(h) => Ok(Value::html(h)),
        None => fail(line, crate::httpd::over_compute()),
    }
}

/// `ask` and `notify` are the browser's (a dialog, a note on the page): on
/// the server there is no one to ask. `ask(question, choices, default)`, three arguments, is a
/// tool's question to its user, asked by the AI client (FEEDBACK56.md, Part E; branch feedback56).
fn ask(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if args.len() >= 3 {
        return crate::mcp::ask(it, args, line);
    }
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

/// `{ every: "1s" }` before `live`'s loader, as milliseconds: the least time
/// between a page's refreshes (FEEDBACK24.md, Q-190; branch feedback24-fixes).
pub fn live_every(e: &crate::ast::Expr) -> Result<u64, String> {
    use crate::ast::{Expr, Lit};
    let wrong = || "`live`'s options are { every: \"1s\" } here, as written: the least time between refreshes, as 500, \"500ms\", \"2s\" or \"1m\"".to_string();
    let Expr::Map(fields) = e else { return Err(wrong()) };
    let [(k, v)] = fields.as_slice() else { return Err(wrong()) };
    if k != "every" {
        return Err(wrong());
    }
    match v {
        Expr::Lit(Lit::Int(n)) if *n > 0 => Ok(*n as u64),
        Expr::Lit(Lit::Str(s)) => {
            let at = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
            let n: u64 = s[..at].parse().map_err(|_| wrong())?;
            match &s[at..] {
                "ms" if n > 0 => Ok(n),
                "s" if n > 0 => Ok(n * 1000),
                "m" if n > 0 => Ok(n * 60_000),
                _ => Err(wrong()),
            }
        }
        _ => Err(wrong()),
    }
}

/// `state items = live(load_items, ...args)` in a component: calls the server fn
/// now, and the browser calls it again whenever something it read changes.
fn live(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // `live({ every: "1s" }, load, ...)`: for the page, which makes the refreshes.
    let args = if matches!(args.first(), Some(Value::Map(_))) { args[1..].to_vec() } else { args };
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
    // A page load shares a run as a refresh does: a thousand arrivals are not a
    // thousand runs (FEEDBACK24.md, Q-193; branch feedback24-fixes).
    let scope = it.run_scope();
    let everyone = crate::livecache::everyone(&scope);
    let key = it.live_key(&name, &rest, &scope);
    let (found, landing) = match &key {
        Some(k) => it.live_turn(k),
        None => (None, None),
    };
    let (r, mut topics, kept, personal) = match found {
        Some((a, topics, _)) => {
            crate::metrics::live_run(true);
            if let Some(o) = &mut it.reads {
                o.extend(topics.iter().cloned());
            }
            (Ok(crate::interp::from_island_json(&a.value)), topics, Some(a), false)
        }
        None => {
            crate::metrics::live_run(false);
            let seq = it.shared.live_seq();
            let (cookies, own, read, acted) = (it.set_cookies.len(), it.own_seqs.len(), it.read_request, it.acted);
            it.read_request = false;
            it.acted = false;
            let outer = it.reads.replace(std::collections::HashSet::new());
            let in_run = std::mem::replace(&mut it.in_run, true);
            let r = it.call(&f, rest.clone(), line);
            it.in_run = in_run;
            let topics = std::mem::replace(&mut it.reads, outer).unwrap_or_default();
            if let Some(o) = &mut it.reads {
                o.extend(topics.iter().cloned());
            }
            let mut topics: Vec<String> = topics.into_iter().collect();
            topics.sort();
            let personal = it.read_request;
            it.read_request |= read;
            // What it did is told to a run around it; one that read the viewer or acted is no one
            // else's, even the viewer's other tab (FEEDBACK55.md, Q-881 and Q-888; branch
            // feedback55).
            let did = it.acted;
            it.acted |= acted;
            let nobody_elses = personal || did;
            let answer = r.as_ref().ok().filter(|v| !matches!(v, Value::Err(..) | Value::Response(_)) && crate::server::hidden(v).is_none()).map(|v| {
                Arc::new(crate::value::js_numbers(false, || crate::livecache::Answer::new(crate::interp::island_json(v), false)))
            });
            if let (Some(k), Some(a)) = (key, &answer) {
                if !nobody_elses && it.set_cookies.len() == cookies && it.own_seqs.len() == own && it.shared.unchanged_since(&topics, seq) {
                    let mut cache = it.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    cache.share(k, seq, topics.clone(), a.clone());
                }
            }
            if let Some(mut l) = landing {
                l.personal = nobody_elses;
            }
            (r, topics, answer, personal)
        }
    };
    topics.sort();
    it.shared.track(&topics);
    let mut info = Map::new();
    info.insert("f".into(), Value::str(&name));
    info.insert("a".into(), Value::list(rest));
    info.insert("t".into(), Value::list(topics.into_iter().map(Value::str).collect()));
    // The value's hash, kept: the page's first refresh can be a patch
    // (FEEDBACK2.md, Q-028; branch feedback6-fixes).
    if let Ok(v) = &r {
        if !matches!(v, Value::Err(..)) {
            let a = kept.unwrap_or_else(|| Arc::new(crate::value::js_numbers(false, || crate::livecache::Answer::new(crate::interp::island_json(v), false))));
            // A viewer's value is theirs (FEEDBACK55.md, Q-884; branch feedback55); with no
            // request to tell them by, it is not kept.
            if let Some(owner) = if personal { it.who() } else { Some(everyone) } {
                it.shared.live_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).keep(a.h, &a.text, owner);
            }
            info.insert("h".into(), Value::str(&crate::livecache::hash_text(a.h)));
        }
    }
    it.last_live = Some(Value::map(info));
    r
}

/// `every(ms, f)` in a component: in the browser, calls `f` every `ms`
/// milliseconds while the component's renders keep asking for it. Rendering
/// on the server starts nothing.
/// `style_with("npx postcss")`: the command each component's and view's
/// `style` goes through when the program loads, before it is scoped
/// (FEEDBACK11.md, Q-079; branch feedback11-fixes). At the top level.
fn style_with(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // A function takes all the styles, marked, and gives them back made (FEEDBACK37.md, Q-455;
    // branch feedback36-39).
    if let Some(f @ (Value::Func(_) | Value::Builtin(..))) = args.first() {
        it.style_with_fn = Some(f.clone());
        return Ok(Value::Nil);
    }
    let cmd = s_arg(&args, 0, "style_with", line)?;
    if cmd.trim().is_empty() {
        return fail(line, "`style_with` takes the command to run, like style_with(\"npx postcss\")");
    }
    it.style_with = Some(cmd.to_string());
    Ok(Value::Nil)
}

/// `style ...` in a component's or view's body: nothing as the code runs;
/// the program takes the CSS when it loads (FEEDBACK8.md, Q-067; branch
/// feedback8-fixes).
fn style_noop(it: &mut Interp, args: Vec<Value>, _: u32) -> R {
    // A style whose body has no element to carry its mark is noted as it runs, and the
    // view around it is not kept, as a kept one would not run this (see
    // `name_unmarked_styles` in src/parser.rs).
    if let Some(Value::Str(id)) = args.get(2) {
        if !it.styles_used.iter().any(|u| **u == **id) {
            it.styles_used.push(id.to_string());
        }
        it.mark_impure();
    }
    Ok(Value::Nil)
}

fn every(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // Met while a page is made, it is the component's, which runs in the browser.
    if it.in_component {
        return match (args.first(), args.get(1)) {
            (Some(Value::Int(_) | Value::Float(_)), Some(Value::Func(_) | Value::Builtin(..))) => Ok(Value::Nil),
            _ => fail(line, "`every` takes milliseconds and a function: every(100, step)"),
        };
    }
    // At the top of a program, as it starts, it runs on the server: a tick (FEEDBACK12.md, Q-080;
    // branch feedback12-fixes). Anywhere else (a route, a job, a tick) it said yes and never ran
    // (FEEDBACK55.md, Q-803; branch feedback55).
    if !crate::value::loading() || it.current_request.is_some() || it.pending_request.is_some() {
        return fail(line, crate::jobs::not_at_top("every"));
    }
    crate::jobs::every_at_top(it, &args, line)
}

/// `js("Chart", ...)` and `js_new(...)` in a component call the page's
/// scripts, in the browser; rendering on the server, they give null.
fn js(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match args.first() {
        Some(Value::Str(_)) => Ok(Value::Nil),
        _ => fail(line, "`js` takes the name of what it calls first: js(\"Chart\", ...)"),
    }
}

/// A name from a browser library (`client "...js" use Chart`), called while the
/// server renders: null, as `js(...)` gives (branch js-libraries).
pub fn js_lib(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::Nil)
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
fn publish(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let topics = topics_arg("publish", &args, line)?;
    it.publish(topics);
    Ok(Value::Nil)
}

/// `watch("topic", ...)`: inside a `live` loader, also refresh on `publish("topic")`.
fn watch(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    for t in topics_arg("watch", &args, line)? {
        it.watch(t);
    }
    Ok(Value::Nil)
}

/// The topics `publish` and `watch` are given, each a name a page can follow (FEEDBACK55.md,
/// Q-801; branch feedback55).
fn topics_arg(f: &str, args: &[Value], line: u32) -> Result<Vec<String>, Ctrl> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        let t = a.display();
        if let Some(why) = crate::shared::topic_problem(f, &t) {
            return fail(line, why);
        }
        out.push(t);
    }
    Ok(out)
}

/// `unwatched(f)`: calls `f`; what it reads (tables, globals, `watch`es)
/// does not make the running `live` loader run again. For data a loader
/// needs but should not follow, such as who is signed in.
fn unwatched(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let f = f_arg(&args, 0, "unwatched", line)?;
    let outer = it.reads.take();
    let r = it.call(&f, vec![], line);
    it.reads = outer;
    r
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
    // As in the browser, where it would run (FEEDBACK51.md, Q-645; branch feedback51-fixes).
    if let Some(scheme) = crate::interp::script_scheme(&s_arg(&args, 0, "navigate", line)?) {
        return fail(line, format!("`navigate` is not given a {} URL", scheme));
    }
    Ok(Value::Nil)
}

/// What a `routes` group's middleware gave, before a route's body: a response
/// (`redirect(...)`, `respond(...)`) answers the request in the route's place,
/// as a `reply` in the middleware does; anything else lets the route go on
/// (branch route-groups).
/// What a group's middleware gave (`routes /admin | signed_in { ... }`): an answer stops the
/// request, and the request given back, or null, lets the route go on. Anything else refuses
/// it, so a middleware fails closed: `false` is a 403, an error value the route's error, and any
/// other value a 500 naming the middleware. It let the route run (FEEDBACK47.md, Q-534; branch
/// feedback40-43).
fn answer_with(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut args = args.into_iter();
    let given = args.next().unwrap_or(Value::Nil);
    let name = args.next().map(|n| n.display()).unwrap_or_else(|| "the middleware".into());
    // The request it was given, or a copy of it (`merge(r, ...)`, whose headers are the
    // request's own), lets the route go on. Any map did: a refusal written as JSON, `{ error:
    // "bad api key" }`, let everyone in; and so did null, so a guard that ends with no value
    // (an `if` that gives the request back, and nothing else) let in everyone it was written to
    // keep out. Null now says the group's routes do not match, as a pattern that misses: the
    // next route is tried, and with none the server answers as if they were not there
    // (FEEDBACK51.md, Q-651; decided 2026-11-04; branch feedback51-fixes).
    let itself = match (&given, &it.current_request) {
        (Value::Map(m), Some(Value::Map(r))) => Arc::ptr_eq(m, r) || matches!((m.get("headers"), r.get("headers")), (Some(Value::Map(a)), Some(Value::Map(b))) if Arc::ptr_eq(a, b)),
        _ => false,
    };
    match given {
        Value::Response(r) => Err(Ctrl::Reply(r)),
        _ if itself => Ok(Value::Nil),
        Value::Nil => Err(Ctrl::Reply(Arc::new(crate::server::no_match(line)))),
        Value::Map(_) => fail(
            line,
            format!("`{}` gave a map that is not the request it was given: middleware gives back the request to let the route go on, or an answer to stop it (`reply 403 {{ error: ... }}`)", name),
        ),
        Value::Bool(false) => Err(Ctrl::Reply(Arc::new(Response {
            status: 403,
            headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
            body: Body::Value(Value::str("forbidden\n")),
        }))),
        e @ Value::Err(..) => Err(Ctrl::Return(e)),
        other => fail(
            line,
            format!(
                "`{}` gave {}: middleware gives back the request to let the route go on, or an answer to stop it (`reply`, `respond`, `redirect`); `false` refuses with a 403, and null says the route does not match",
                name,
                other.type_name()
            ),
        ),
    }
}

/// `waiting(state)`: in a component, whether a live state waits for the
/// answer to new arguments. The server renders with the answer, so here it
/// is never waiting (FEEDBACK13.md, Q-091; branch feedback13-fixes).
fn waiting(_: &mut Interp, _: Vec<Value>, _: u32) -> R {
    Ok(Value::Bool(false))
}

/// A name a program puts in SQL (a table's, a column's): letters, digits
/// and `_`, not starting with a digit, at most 63 bytes. Anything else
/// is refused, so the name cannot carry SQL of its own.
///
/// Letters and digits of any script, quoted, as every database takes them, and the marks that
/// go on a letter (Hindi's vowel signs, Thai's tone marks, an accent written apart):
/// `sql_name("名前")` stopped the program, "letters" meaning ASCII. Of the first 65,536
/// characters only, which is all MySQL takes in a name, and in 63 bytes, all PostgreSQL keeps of
/// one, so that two names are two on every database (FEEDBACK55.md, Q-756; branch feedback55;
/// 63 and the marks found in review).
fn sql_ident(v: &Value, f: &str, line: u32) -> Result<String, Ctrl> {
    let Value::Str(s) = v else { return fail(line, format!("`{}` takes a name (a str) or a list of them, got {}", f, v.type_name())) };
    let mark = unicode_normalization::char::is_combining_mark;
    let mut after_letter = false;
    let mut chars_ok = true;
    for c in s.chars() {
        let here = if (c as u32) >= 0x10000 {
            false
        } else if c.is_alphanumeric() {
            true
        } else {
            after_letter && mark(c)
        };
        chars_ok &= here || c == '_';
        after_letter = here;
    }
    let ok = !s.is_empty() && s.len() <= 63 && chars_ok && !s.starts_with(char::is_numeric);
    if !ok {
        let why = if s.len() > 63 && chars_ok { "at most 63 bytes" } else { "letters, digits and _" };
        return fail(line, format!("`{}`: {:?} is not a name SQL can take ({})", f, &**s, why));
    }
    Ok(s.to_string())
}

pub(crate) fn sql_idents(args: &[Value], f: &str, line: u32, each: impl Fn(String) -> String) -> R {
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
fn sql_name(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // MySQL reads "x" as text (unless in ANSI_QUOTES mode): a name there is `x`.
    if crate::db::program_db_is_mysql(it) {
        return sql_idents(&args, "sql_name", line, |s| format!("`{}`", s));
    }
    sql_idents(&args, "sql_name", line, |s| format!("\"{}\"", s))
}

/// `sql_param("title")`: a named parameter, `:title`, checked; a list gives
/// them comma-separated (for `insert ... values (...)` with a map).
fn sql_param(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    sql_idents(&args, "sql_param", line, |s| format!(":{}", s))
}

/// A value put in CSS, `${v}` in css`...` or a style map's: `{`, `}`, `;` and `\` as CSS's
/// escapes, which make them part of a word, so the value is one value and adds no
/// declaration or rule. `?q=red} input[value^=a]{background:url(//evil/a)}` added a rule that
/// read the page's fields (FEEDBACK51.md, Q-668; branch feedback51-fixes).
pub fn css_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '{' => out.push_str("\\7b "),
            '}' => out.push_str("\\7d "),
            ';' => out.push_str("\\3b "),
            '\\' => out.push_str("\\5c "),
            c => out.push(c),
        }
    }
    out
}

/// `__css_value(v)`: what css`...` makes of each `${v}` (see `css_value`). And `&` as CSS's
/// `\26 `: a `<style>` inside `<svg>` or `<math>` is markup, whose character references the
/// browser decodes, and `&#125` (a `}` with no `;`) added a rule there (FEEDBACK55.md, Q-707;
/// branch feedback55). The template's own text, CSS nesting's `&` say, stays as written.
fn css_value_fn(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(Value::str(&css_value(&arg(&args, 0).display()).replace('&', "\\26 ")))
}

/// `css` and `sql`: css`...` and sql`...` are the text as written; the tag
/// tells editors to highlight it as CSS or SQL.
fn tag_text(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::str(&*s_arg(&args, 0, "css", line)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::Relaxed;

    /// Names of one slot in `most_args`' table are each kept, in the slots after it, and each
    /// still says how many arguments its builtin takes (FEEDBACK55.md, string speed; branch
    /// feedback55).
    #[test]
    fn names_of_one_slot_are_each_kept() {
        // `len` at addresses of the test's choosing: many of one block.
        let block: &'static str = Box::leak("len".repeat(40_000).into_boxed_str());
        let names: Vec<&'static str> = (0..40_000).map(|i| &block[i * 3..i * 3 + 3]).collect();
        // Three of a slot whose neighbours no other name has yet.
        let free = |home: usize| (0..MOST_PROBES).all(|k| MOST[(home + k) & (MOST.len() - 1)].load(Relaxed) == 0);
        let home = names.iter().map(|n| most_slot(n.as_ptr() as u64)).find(|h| free(*h)).expect("a free slot");
        let same: Vec<&'static str> = names.iter().copied().filter(|n| most_slot(n.as_ptr() as u64) == home).take(3).collect();
        assert_eq!(same.len(), 3);
        for _ in 0..3 {
            for n in &same {
                assert_eq!(most_args(n), 1);
            }
        }
        // Anywhere among the slots a name is looked for in: tests run at once, and another's
        // first call of a builtin may have taken one of these meanwhile.
        let kept: Vec<u64> = (0..MOST_PROBES).map(|k| MOST[(home + k) & (MOST.len() - 1)].load(Relaxed)).collect();
        for n in &same {
            assert!(kept.contains(&((n.as_ptr() as u64) << 8 | 1)), "{:x?}", kept);
        }
        // A name the table does not have takes any number, kept too.
        let other: &'static str = Box::leak("no_such_builtin".to_string().into_boxed_str());
        assert_eq!(most_args(other), usize::MAX);
        assert_eq!(most_args(other), usize::MAX);
    }
}
