//! The standard library: functions every program can call.

use crate::interp::{fail, Ctrl, Interp};
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
        ("moves_in_place", moves_in_place),
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
        ("page_head", page_head),
        ("page_url", page_url),
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
    // A person's devices told of something, over Web Push (FEEDBACK67.md, B.3; branch pwa).
    let mut notification = Map::new();
    crate::push::install(&mut notification);
    define(env, "notification", Value::map(notification));
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
    // A builtin's own argument is never trusted CSS here: `Interp::call` made it text.
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
    let (Value::Str(s) | Value::Css(s)) = arg(&args, 0) else { return fail(line, "`cursor_index` expects a str as argument 1") };
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

fn print(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // A top-level `print` of markup is a render of its own: an error shown in the next is said
    // again (FEEDBACK81.md, Q-1272, found in review; branch feedback72-81).
    it.err_shown = false;
    // The program's own log: what it hides from visitors, it sees. Its line held as any text a
    // request makes (FEEDBACK71.md, Q-977; branch security).
    let text = crate::value::told(|| {
        let mut out = Held::new();
        for (i, v) in args.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            v.display_into(&mut out);
        }
        out.into_text()
    });
    print_line(&text.map_err(|o| o.at(line))?);
    Ok(Value::Nil)
}

fn to_str(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // A list's or map's text written through a `Held` text, which stops at `--max-memory`
    // (FEEDBACK71.md, Q-977; branch security).
    Ok(Value::str(arg(&args, 0).display_held().map_err(|o| o.at(line))?))
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
        Value::Str(s) | Value::Css(s) => crate::interp::coerce_loose("int", &s).unwrap_or_else(|| Value::err(Value::str(format!("not an int: {:?}", &*s)))),
        other => Value::err(Value::str(format!("cannot convert {} to int", other.type_name()))),
    })
}

fn to_float(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match arg(&args, 0) {
        Value::Int(n) => Value::Float(n as f64),
        Value::Float(f) => Value::Float(f),
        Value::Dec(d) => Value::Float(d.to_f64()),
        Value::Str(s) | Value::Css(s) => {
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
        Value::Str(s) | Value::Css(s) => match Dec::parse(s.trim()) {
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
    let s = s_arg(&args, 0, "upper", line)?;
    cased_length(&s, true, line)?;
    Ok(Value::str(s.to_uppercase()))
}

fn lower(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "lower", line)?;
    cased_length(&s, false, line)?;
    Ok(Value::str(s.to_lowercase()))
}

/// The length of `s` in upper (or lower) case, compared with `--max-memory` before it is made:
/// a letter can take three times its bytes (`ΐ` is three letters in upper case), and 1.2 MB of
/// them made 3.6 MB under a 1 MB limit (FEEDBACK71.md, Q-977, found in review; branch security).
/// Counted, exactly, only where the limit is set and the text could reach 64 KB.
fn cased_length(s: &str, up: bool, line: u32) -> Result<(), Ctrl> {
    if s.len() >= crate::httpd::LARGE / 3 && crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
        let n: usize = if up { s.chars().map(|c| c.to_uppercase().map(char::len_utf8).sum::<usize>()).sum() } else { s.chars().map(|c| c.to_lowercase().map(char::len_utf8).sum::<usize>()).sum() };
        made(n, line)?;
    }
    Ok(())
}

/// `normalize(s, "NFC")`: one Unicode form for text that looks the same, so `café` typed and
/// `cafe` with a combining accent are one name; `"NFKC"` also folds look-alikes such as `ﬁ`, for
/// comparing user names (FEEDBACK31.md, Q-394; branch feedback27-35-fixes).
fn normalize(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    use unicode_normalization::UnicodeNormalization;
    let s = s_arg(&args, 0, "normalize", line)?;
    let form = match args.get(1) {
        None | Some(Value::Nil) => "NFC".into(),
        Some(Value::Str(f) | Value::Css(f)) => f.to_ascii_uppercase(),
        Some(other) => return fail(line, format!("`normalize` expects a form (\"NFC\", \"NFD\", \"NFKC\" or \"NFKD\") as argument 2, got {}", other.type_name())),
    };
    // Written through a `Held` text where the limit is set: a character can take 18 of its kind
    // (`ﷺ` in NFKD), and 900 KB of them made a 16 MB text (FEEDBACK71.md, Q-977, found in
    // review; branch security).
    fn held(cs: impl Iterator<Item = char>, line: u32) -> Result<String, Ctrl> {
        let mut out = Held::new();
        for c in cs {
            out.push(c);
            if out.is_over() {
                break;
            }
        }
        out.into_text().map_err(|o| o.at(line))
    }
    let limited = crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed);
    Ok(Value::str(match form.as_str() {
        "NFC" if limited => held(s.nfc(), line)?,
        "NFD" if limited => held(s.nfd(), line)?,
        "NFKC" if limited => held(s.nfkc(), line)?,
        "NFKD" if limited => held(s.nfkd(), line)?,
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
    Ok(args.into_iter().next().filter(|v| !matches!(v, Value::Str(s) | Value::Css(s) if s.is_empty())).unwrap_or(Value::Nil))
}

fn trim(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::str(s_arg(&args, 0, "trim", line)?.trim()))
}

fn split(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let s = s_arg(&args, 0, "split", line)?;
    let sep = if args.len() > 1 { s_arg(&args, 1, "split", line)? } else { Arc::from(" ") };
    // The list of parts asked about first under `--max-memory`: a text of a million letters split
    // by "" is a million items (FEEDBACK71.md, Q-977; branch security). Counted only when the
    // list could be large, and the limit is set.
    if s.len().saturating_add(1).saturating_mul(ITEM_BYTES) >= crate::httpd::LARGE && crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
        let parts = if sep.is_empty() { s.chars().count() } else { s.matches(&*sep).count() + 1 };
        made(parts.saturating_mul(ITEM_BYTES), line)?;
    }
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
    // The text it makes is compared with `--max-memory` first, where a list that holds one long
    // text many times joins to a far longer one (FEEDBACK71.md, Q-977; branch security). Texts
    // only: their length summed as they are looked at, and one text made to it with nothing
    // copied first (each was copied, then joined: 100 short texts took 2.8 times the
    // instructions). Else each item's text, as before, a list's or a map's compared first, and
    // then the whole.
    let mut total = sep.len().saturating_mul(l.len().saturating_sub(1));
    let texts = l.iter().all(|v| match v {
        Value::Str(s) | Value::Css(s) => {
            total = total.saturating_add(s.len());
            true
        }
        _ => false,
    });
    if texts {
        if total >= crate::httpd::LARGE {
            crate::interp::large(total, line)?;
        }
        let mut out = String::with_capacity(total);
        for (i, v) in l.iter().enumerate() {
            if i > 0 {
                out.push_str(&sep);
            }
            if let Value::Str(s) | Value::Css(s) = v {
                out.push_str(s);
            }
        }
        return Ok(Value::str(out));
    }
    // Else each item's text written where it goes, through a `Held` text, which stops at
    // `--max-memory`: no item's text made first (FEEDBACK71.md, Q-977, found in review: 200 long
    // texts and a number made 180 MB of parts before the whole was compared; branch security).
    let mut out = Held::new();
    for (i, v) in l.iter().enumerate() {
        if i > 0 {
            out.push_str(&sep);
        }
        v.display_into(&mut out);
        if out.is_over() {
            break;
        }
    }
    Ok(Value::str(out.into_text().map_err(|o| o.at(line))?))
}

/// A large value of `n` bytes, asked about before it is made, under `--max-memory`
/// (FEEDBACK71.md, Q-977; branch security).
#[inline(always)]
pub(crate) fn made(n: usize, line: u32) -> Result<(), Ctrl> {
    crate::httpd::making(n).map_err(|s| s.at(line))
}

/// What a list's place for each item takes (see `value::ITEM_BYTES`).
const ITEM_BYTES: usize = crate::value::ITEM_BYTES;

fn contains(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let needle = arg(&args, 1);
    Ok(Value::Bool(match arg(&args, 0) {
        // A list's text held, as `str` holds it (FEEDBACK71.md, Q-977; branch security).
        Value::Str(s) | Value::Css(s) => s.contains(&*needle.display_held().map_err(|o| o.at(line))?),
        Value::List(l) => l.iter().any(|v| equal(v, &needle)),
        Value::Map(m) => m.contains_key(&*needle.display_held().map_err(|o| o.at(line))?),
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
    // The text it makes, compared with `--max-memory` first: each "a" given a long text
    // (FEEDBACK71.md, Q-977; branch security). The most it could make is compared first, and only
    // where that passes the limit are the matches counted: counted whatever the limit, a 108 KB
    // `replace(s, "<", "&lt;")` took 80% longer (found in review).
    let most = if to.len() <= from.len() { s.len() } else { s.len().saturating_add((s.len() / from.len().max(1) + 1).saturating_mul(to.len() - from.len())) };
    if most >= crate::httpd::LARGE && most as u64 > crate::httpd::value_most() {
        let times = if to.len() <= from.len() { 0 } else if from.is_empty() { s.chars().count() + 1 } else { s.matches(&*from).count() };
        made(s.len().saturating_add(times.saturating_mul(to.len().saturating_sub(from.len()))), line)?;
    }
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
        Value::Str(st) | Value::Css(st) => {
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
            None if matches!(x, Value::Str(_) | Value::Css(_)) => return fail(line, sum_of_text(x, i)),
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
    // In a script, the keys compared are paid for as what a builtin reads: each could be one list
    // near 1 MB (FEEDBACK75.md, Q-1055; branch feedback75).
    if it.fuel != u64::MAX {
        it.script_read(items.iter().map(|(k, _)| k), line)?;
    }
    sort_values(items, "sort_by", line)
}

fn reverse(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match arg(&args, 0) {
        Value::List(l) => Ok(Value::list(l.iter().rev().cloned().collect())),
        Value::Str(s) | Value::Css(s) => Ok(Value::str(s.chars().rev().collect::<String>())),
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
    // A list another holds is copied as it was (no bigger than the list it copies); one with no
    // room left grows out of the way, the room it grows to compared with `--max-memory` first
    // (`value::pushed_growing`), so a push with room is the one it was (FEEDBACK71.md, Q-977,
    // found in review: asked before `make_mut`, a push cost an instruction more).
    let v = crate::value::sole(&mut l);
    if v.len() == v.capacity() {
        let mut item = item;
        if !crate::value::pushed_growing(v, &mut item) {
            return Err(crate::httpd::OverMemory.at(line));
        }
        return Ok(Value::List(l));
    }
    v.push(item);
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
        // Asked about first under `--max-memory` (FEEDBACK71.md, Q-977; branch security).
        made((n as usize).saturating_mul(ITEM_BYTES), line)?;
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
    if let (Some(Value::Map(m)), Some(Value::Str(k) | Value::Css(k))) = (args.first(), args.get(1)) {
        return Ok(m.get(&**k).cloned().unwrap_or_else(|| arg(&args, 2)));
    }
    let m = m_arg(&args, 0, "get", line)?;
    // An error is no key: its text was one (FEEDBACK55.md, Q-784; branch feedback55).
    if let k @ Value::Err(..) = arg(&args, 1) {
        return Ok(k);
    }
    let k = arg(&args, 1).key_text_held().map_err(|o| o.at(line))?;
    Ok(m.get(k.as_str()).cloned().unwrap_or_else(|| arg(&args, 2)))
}

fn has(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let m = m_arg(&args, 0, "has", line)?;
    // An error is no key (FEEDBACK55.md, Q-784; branch feedback55).
    if let k @ Value::Err(..) = arg(&args, 1) {
        return Ok(k);
    }
    Ok(Value::Bool(m.contains_key(arg(&args, 1).key_text_held().map_err(|o| o.at(line))?.as_str())))
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
        crate::value::sole(&mut l)[at as usize] = v;
        return Ok(Value::List(l));
    }
    let mut m = m_arg(&args, 0, "set", line)?;
    // An error is no key: `set({}, int("q"), 1)` made its text one (FEEDBACK55.md, Q-784; branch
    // feedback55).
    if let k @ Value::Err(..) = arg(&args, 1) {
        return Ok(k);
    }
    // A list as a key is its text, written held (FEEDBACK71.md, Q-977; branch security).
    let (k, v) = (arg(&args, 1).key_text_held().map_err(|o| o.at(line))?, arg(&args, 2));
    // The arguments let go first: a map no one else holds is changed, not
    // copied (see `in_place` in src/interp.rs).
    drop(args);
    // A map with no room for a new key grows: the room it grows to compared with `--max-memory`
    // first, out of the way (FEEDBACK71.md, Q-977; branch security). One another holds is copied
    // as it was.
    let mm = crate::value::sole(&mut m);
    if mm.len() == mm.capacity() && !crate::value::map_may_grow_to(mm, &k) {
        return Err(crate::httpd::OverMemory.at(line));
    }
    mm.insert(k.into(), v);
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
            let k = arg(&args, 1).key_text_held().map_err(|o| o.at(line))?;
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
    // Held to `--max-memory` as it grows, where the limit is set: what a key of both takes is
    // taken once (FEEDBACK71.md, Q-977, found in review; branch security).
    if crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed) {
        for (k, v) in b.iter() {
            crate::value::insert_held(&mut m, k.clone(), v.clone()).map_err(|o| o.at(line))?;
        }
        return Ok(Value::map(m));
    }
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
    // As `merge`, held as it grows where the limit is set (FEEDBACK71.md, Q-977; branch security).
    let limited = crate::httpd::MEMORY_LIMITED.load(std::sync::atomic::Ordering::Relaxed);
    for v in args {
        match v {
            Value::Map(m) if limited => {
                for (k, v) in m.iter() {
                    crate::value::insert_held(&mut out, k.clone(), v.clone()).map_err(|o| o.at(line))?;
                }
            }
            Value::Map(m) => out.extend(m.iter().map(|(k, v)| (k.clone(), v.clone()))),
            Value::Nil => {}
            other => return fail(line, format!("`{{...x}}` spreads a map (or null) into a map, but x is {}", other.type_name())),
        }
    }
    Ok(Value::map(out))
}

fn spread_list(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // Its length, known before it is made, compared with `--max-memory` (FEEDBACK71.md, Q-977,
    // found in review: ten spreads of a list made it ten times over; branch security).
    let n = args.iter().map(|v| if let Value::List(l) = v { l.len() } else { 0 }).fold(0usize, usize::saturating_add);
    crate::value::list_of(n).map_err(|o| o.at(line))?;
    let mut out = Vec::with_capacity(n);
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
        Value::Err(e, _, _) => fail(line, format!("unwrap of an error: {}", e.display_held().map_err(|o| o.at(line))?)),
        v => Ok(v),
    }
}

// ---- web ---------------------------------------------------------------

// A list's text held as `str` holds it, and the escaped text written through a `Held` text: an
// escape can take six bytes for one (FEEDBACK71.md, Q-977, found in review; branch security).
fn raw(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    Ok(Value::html(arg(&args, 0).display_held().map_err(|o| o.at(line))?))
}

fn escape_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let t = arg(&args, 0).display_held().map_err(|o| o.at(line))?;
    let mut out = Held::with_capacity(t.len());
    crate::interp::escape_into(&t, &mut out);
    Ok(Value::str(out.into_text().map_err(|o| o.at(line))?))
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

fn url_encode(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // Through a `Held` text, which stops at `--max-memory`: a byte can take three (FEEDBACK71.md,
    // Q-977, found in review; branch security).
    fn enc(s: &str, out: &mut Held) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                out.push(b as char);
            } else {
                let e = [b'%', HEX[(b >> 4) as usize], HEX[(b & 15) as usize]];
                // SAFETY: ASCII.
                out.push_str(unsafe { std::str::from_utf8_unchecked(&e) });
            }
            if out.is_over() {
                return;
            }
        }
    }
    let mut out = Held::new();
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
                enc(&v.display_held().map_err(|o| o.at(line))?, &mut out);
            }
        }
        v => enc(&v.display_held().map_err(|o| o.at(line))?, &mut out),
    }
    Ok(Value::str(out.into_text().map_err(|o| o.at(line))?))
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
    // A stream's too: what it sends next fails at a stop anyway, and the drain has no limit
    // unless `--drain` gives one (the user's graceful stop, 2026-11-09; branch feedback56). Not in
    // a transaction, which a stop lets finish (found in review).
    let streaming = it.stream_out.is_some() && it.tx_depth == 0 && it.sqlite_tx == 0;
    // A `parallel` function ends its sleep once another has failed (FEEDBACK55.md, Q-804; branch
    // feedback55).
    let stop = it.stop.clone();
    let slice = std::time::Duration::from_millis(if stop.is_some() { 20 } else { 50 });
    let why = move || {
        if (in_job || streaming) && crate::httpd::stopping() {
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
                let nap = if sliced { left.min(slice) } else { left };
                if streaming {
                    // Woken by the stop, not every 50 ms: ten thousand streams asleep cost nothing
                    // (found in review).
                    let mut stop = std::pin::pin!(crate::httpd::stop_notice().notified());
                    stop.as_mut().enable();
                    if let Some(w) = why() {
                        return Some(w);
                    }
                    let _ = tokio::time::timeout(nap, stop).await;
                    continue;
                }
                if let Some(w) = why() {
                    return Some(w);
                }
                tokio::time::sleep(nap).await;
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
        Some(why) if streaming && crate::httpd::stopping() => crate::stream::ended_by_stop(it, line).unwrap_or_else(|| fail(line, why)),
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
    // A path its file was not confirmed to read is not there for it (FEEDBACK83.md, Q-1300, found
    // in review; branch feedback82-85): one look at the hold where nothing is held.
    if crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &p, false).is_err() {
        return Ok(Some(not_found()));
    }
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
        if info.id.is_some_and(|id| own_id(id, info.links, info.born).is_some()) || sqlite_journal(&full) {
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
                    // Begun as a database's header could be, and changed as it was read: looked at
                    // again (`statics::steady_head`).
                    if crate::db::sqlite_lead(&b) && !crate::statics::steady_head(&f, &b) {
                        return Err(Judged::Replaced);
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
                ("download", Value::Str(n) | Value::Css(n)) if !n.is_empty() => download = Some(n.clone()),
                // What is wrong with a str, said (FEEDBACK55.md, Q-726; branch feedback55).
                ("download", Value::Str(_) | Value::Css(_)) => return fail(line, "`file`'s download is empty: give the name a browser saves the file as, or leave it out"),
                ("type", Value::Str(t) | Value::Css(t)) => given_type = Some(content_type(t, "file", line)?),
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
            Some(Value::Str(m) | Value::Css(m)) => (&**m == "GET" || &**m == "HEAD", &**m == "HEAD"),
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
                            if if start == 0 { crate::db::sqlite_bytes(&part) || crate::db::sqlite_lead(&part) && !crate::statics::steady_head(&f, &part) } else { sqlite_head(&f) } {
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
/// A policy the program runs under, by its file's identity: a hard link to it under another name
/// was served (FEEDBACK75.md, Q-1088, found in review; branch feedback75).
pub const OWN_POLICY: &str = "it is the program's policy";
/// What the program's files were confirmed to reach (FEEDBACK83.md, Q-1300; branch feedback82-85).
pub const OWN_CONFIRMED: &str = "it is what the program was confirmed to reach (quillo.confirmed)";
/// What Quillo keeps for the program beside its files (FEEDBACK85.md, Q-1340, found by the review
/// of b7f7c26; branch feedback82-85).
pub(crate) const OWN_QUILLO: &str = "it is in the program's .quillo folder, which holds its built plugins, their lock and its keys";

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
    // Or in a folder `--allow-path` named, which `files` serves as `file` does (FEEDBACK85.md,
    // Q-1340; branch feedback82-85), by whichever name it was given (`statics::within`): one named
    // `/System/Volumes/Data/...` held no file the system names `/private/...` (FEEDBACK85.md,
    // Q-1340, found by the coverage tests; branch feedback82-85).
    if !(crate::statics::within(&real, &program) || allowed_dir(&real).is_some()) || hidden_target(&program, &served, &real) {
        return Err(Refused);
    }
    if sqlite_journal(&real) {
        return Err(Refused);
    }
    Ok((f, m))
}

/// Whether `p`, by its name a `-wal`, `-shm` or `-journal`, is a SQLite database's, by the first
/// bytes of the file beside it (`db::sqlite_file`), read only where a look (which opens nothing)
/// finds that a file of bytes: a FIFO there, or a link to one, is no database, and opening it
/// waited for a writer for ever, a request to `files()` for `q-wal` with it (FEEDBACK85.md,
/// Q-1340, found by the review of b7f7c26; branch feedback82-85). A name of no journal costs a
/// look at its letters.
fn sqlite_journal(p: &Path) -> bool {
    crate::db::journal_of(p).is_some_and(|db| std::fs::metadata(&db).is_ok_and(|m| m.is_file()) && crate::db::sqlite_file(&db))
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
        // Who may call the program's `server fn`s: a taken file's, only where it was confirmed
        // (`site_wide`).
        Ok(_) if !site_wide(it, || "accept_calls".into()) => Ok(Value::Nil),
        Ok(a) => {
            it.accept = Some(Arc::new(a));
            Ok(Value::Nil)
        }
        Err(e) => fail(line, e),
    }
}

/// `server fn refund(...) for ["shop"]`: the callers it takes, as the parser writes it.
fn callers(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::Str(f) | Value::Css(f)), Some(Value::List(l))) = (args.first(), args.get(1)) else {
        return fail(line, "`__callers` is written by `server fn ... for [...]`");
    };
    // A file says the callers of its own `server fn` only, where a quillo.confirmed holds the run
    // (FEEDBACK83.md, Q-1301, found by the review of 4bc1ac9; branch feedback82-85).
    if crate::confirm::held() {
        let declared = match crate::value::lookup(&it.globals, &**f) {
            Some(Value::Func(c)) => c.def.file.clone(),
            _ => None,
        };
        if !crate::confirm::callers_ok(it.cur_file.as_deref(), declared.as_deref(), f) {
            return Ok(Value::Nil);
        }
    }
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
    let (Some(Value::Str(path) | Value::Css(path)), Some(f @ Value::Func(c))) = (args.first(), args.get(1)) else {
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
        Value::Str(s) | Value::Css(s) => (s.as_bytes().to_vec(), mime(Path::new(path)).to_string()),
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
        Some(Value::Str(p) | Value::Css(p)) => p.clone(),
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
                    ("index", Value::Str(s) | Value::Css(s)) if !s.is_empty() && !s.contains('/') => index = s.to_string(),
                    ("extensions", Value::List(l)) => {
                        extensions = l.iter().map(|e| e.display().trim_start_matches('.').to_string()).filter(|e| !e.is_empty()).collect();
                    }
                    ("not_found", Value::Str(s) | Value::Css(s)) => not_found_page = Some(s.to_string()).filter(|s| !s.is_empty()),
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
    let base = it.base_dir.join(&*dir);
    // And the policy in use, whatever `--policy` named it: `quillo serve . --policy prod.policy`
    // served /prod.policy (FEEDBACK75.md, Q-1088, found in review; branch feedback75).
    let hidden = hidden_path(rel) || policy_in_use(&base.join(rel));
    // What a link leads to is judged as its name is: `notes.txt -> ../main.qwe` served the
    // program's source and `settings.txt -> ../.env` its settings (FEEDBACK55.md, Q-701; branch
    // feedback55). A link out of the folder is not served, as `stays_in` says, unless it leads
    // into a folder `--allow-path` names, as for `file`, whose word in the log it says when it
    // does not: `files` refused such a link with no word and whatever `--allow-path` said
    // (FEEDBACK85.md, Q-1340; branch feedback82-85). An index, a name found with its extension or
    // the page of a miss is judged so too.
    let led_by = |it: &Interp, full: &Path| leads_to(&it.base_dir, full).or_else(|| led_out(it, full).map(Some));
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
        Ok(m) if m.is_file() || (m.file_type().is_symlink() && full.is_file()) => Some(judged(it, &led_by(it, full))),
        _ => None,
    };
    let joined = |rel: &str| if rel.is_empty() { dir.to_string() } else { format!("{}/{}", dir.trim_end_matches('/'), rel) };
    // Judged before it is looked at: whether a folder outside is there is not said (Q-475).
    let led = if hidden || rel.split('/').any(|s| s == "..") { None } else { led_by(it, &base.join(rel)) };
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
                Some(Value::Map(r)) => matches!(r.get("path"), Some(Value::Str(p) | Value::Css(p)) if p.ends_with('/')
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
    if let Some(page) = not_found_page.filter(|p| base.join(p).is_file() && judged(it, &led_by(it, &base.join(p)))) {
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
                // Still a file, sent as it is: no script added to it for having no `ETag`
                // (FEEDBACK69.md, Q-964; branch q964-miss-page).
                r.headers.push((crate::server::AS_WRITTEN.into(), "1".into()));
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
    // And the program's policy, which names its secrets' variables, its inner hosts and its
    // limits: `quillo serve` read DIR/quillo.policy as its own and served it (FEEDBACK75.md,
    // Q-1088; branch feedback75).
    rel.split('/').enumerate().any(|(i, seg)| seg.starts_with('.') && !(i == 0 && seg == ".well-known")) || last.ends_with(".qwe") || crate::native::is_rust(&last) || last == crate::policy::PROGRAM_FILE
}

/// Whether `path` is a policy file the program runs under (`policy::get().files`), by its name as
/// the file system folds it first, then by the file itself (FEEDBACK75.md, Q-1088, found in review;
/// branch feedback75). Only a request for a file of such a name looks at the disk.
fn policy_in_use(path: &Path) -> bool {
    let files = &crate::policy::get().files;
    let Some(name) = path.file_name().filter(|_| !files.is_empty()) else { return false };
    let fold = |n: &std::ffi::OsStr| n.to_string_lossy().trim_end_matches(['.', ' ']).to_uppercase().to_lowercase();
    let asked = fold(name);
    files.iter().any(|f| f.file_name().is_some_and(|n| fold(n) == asked) && same_file(f, path))
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
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
    if policy_in_use(real) {
        return true;
    }
    const STORES: [&str; 4] = [".pnpm", ".deno", ".bun", ".store"];
    const TOOLS: [&str; 6] = [".vitepress/dist", ".output/public", ".next/static", ".svelte-kit/output/client", ".nuxt/dist/client", ".vercel/output/static"];
    // Each folder by whichever name it was given, the place a link led to by another
    // (`statics::rel_in`; FEEDBACK85.md, Q-1340, found by the coverage tests; branch feedback82-85).
    if let Some(rel) = crate::statics::rel_in(real, served) {
        let parts: Vec<String> = rel.components().map(|c| fold_name(name_as_taken(&c.as_os_str().to_string_lossy()))).collect();
        let Some((name, folders)) = parts.split_last() else { return false };
        let above = served.file_name().map(|n| fold_name(&n.to_string_lossy()));
        let store = |i: usize| STORES.contains(&folders[i].as_str()) && if i == 0 { above.as_deref() == Some("node_modules") } else { folders[i - 1] == "node_modules" };
        let tool = |i: usize| TOOLS.iter().any(|t| t.split('/').count() + i <= folders.len() && t.split('/').enumerate().all(|(k, seg)| folders[i + k] == seg));
        let kept = |(i, p): (usize, &String)| p.starts_with('.') && !(i == 0 && p == ".well-known") && !store(i) && !tool(i);
        return folders.iter().enumerate().any(kept) || hidden_path(name);
    }
    // Out of the program's folder, into one `--allow-path` named: judged by its name there, as
    // in the program's own (FEEDBACK85.md, Q-1340; branch feedback82-85).
    let rel = match crate::statics::rel_in(real, program) {
        Some(rel) => rel,
        None => match allowed_dir(real) {
            Some(d) => return crate::statics::rel_in(real, &d).map_or(true, |rel| hidden_path(&rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))),
            None => return true,
        },
    };
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    hidden_path(&parts.join("/"))
}

/// The folder `--allow-path` named that `real`, a path as it really is, is in (FEEDBACK85.md,
/// Q-1340; branch feedback82-85), by whichever name each was given (`statics::within`).
fn allowed_dir(real: &Path) -> Option<std::path::PathBuf> {
    let dirs = ALLOWED_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    dirs.iter().find(|d| crate::statics::within(real, d)).cloned()
}

/// Seconds from `3600` or `"1h"` (s, m, h, d, w, y).
pub(crate) fn duration_seconds(v: &Value) -> Option<u64> {
    match v {
        Value::Int(n) if *n >= 0 => Some(*n as u64),
        Value::Str(s) | Value::Css(s) => {
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
    // The paths its file was confirmed to write (FEEDBACK83.md, Q-1300, found in review; branch
    // feedback82-85).
    if let Err(why) = crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &dest, true) {
        return Ok(Value::err(Value::str(format!("save_upload: {}", why))));
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

/// Whether a call that sets `item` (as `quillo.confirmed` lists it: `allow_cross_site /`) for the
/// whole program or site is applied: where a quillo.confirmed holds the run, a file taken for some
/// of its names sets only what was confirmed for it (`confirm::site_ok`), and said once where it is
/// not; the program's own files set what they set. A kit taken for `Badge` called
/// `allow_cross_site("/")` and `site_headers` with `x-frame-options: null` and credentialed CORS
/// for another site, and every route of the main file took that site's posts (FEEDBACK83.md,
/// Q-1301, found by the review of 4bc1ac9; branch feedback82-85). One look at the hold where none
/// is; asked after the arguments are judged, so a wrong one is said as before.
pub(crate) fn site_wide(it: &Interp, item: impl FnOnce() -> String) -> bool {
    !crate::confirm::held() || !crate::confirm::held_taken_at(&it.cur_file) || crate::confirm::site_ok(it.cur_file.as_deref(), &item())
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
    if !site_wide(it, || format!("allow_cross_site {}", p)) {
        return Ok(Value::Nil);
    }
    Arc::make_mut(&mut it.site).cross_site_ok.push(p.to_string());
    Ok(Value::Nil)
}

/// `moves_in_place()`, at the top of a program: from then on every same-site link, and
/// `navigate`, moves to its route's page in place: the page asked for, what differs swapped in, the
/// head the new page's, a component on both pages (the same, with the same props) and the page's
/// states kept; a full load where that is not safe (src/client/runtime.js, `W.inPlace`;
/// FEEDBACK84.md, Q-1323, Q-1327; decided (user, 2026-11-22): an opt-in switch at the top; branch
/// feedback82-85).
fn moves_in_place(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    if !args.is_empty() {
        return fail(line, "`moves_in_place()` takes nothing");
    }
    if !site_wide(it, || "moves_in_place".into()) {
        return Ok(Value::Nil);
    }
    Arc::make_mut(&mut it.site).in_place = true;
    Ok(Value::Nil)
}

/// `site_headers({ "content-security-policy": "..." })`, or `site_headers("/admin", {...})` for
/// the answers under a path: headers sent with every answer that does not set them itself. A
/// null value takes away one Quillo sends by itself, `"x-frame-options": null` (FEEDBACK31.md,
/// Q-391; branch feedback27-35-fixes).
fn site_headers(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (prefix, m) = match args.as_slice() {
        [Value::Map(m)] => ("/".to_string(), m.clone()),
        [Value::Str(p) | Value::Css(p), Value::Map(m)] if p.starts_with('/') => (p.trim_end_matches('/').to_string(), m.clone()),
        [Value::Str(p) | Value::Css(p), _] if !p.starts_with('/') => return fail(line, format!("`site_headers` needs a path that starts with /, got {:?}", &**p)),
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
        // FEEDBACK69.md, Q-964, found in review; branch q964-miss-page. And a server fn's own
        // answer's mark (FEEDBACK79.md, Q-1250, found in the review of the trusted CSS; branch feedback72-81).
        if name == crate::server::AS_WRITTEN || name == crate::server::MARKED {
            return fail(line, format!("`site_headers`: `{}` is Quillo's own", name));
        }
        let value = match v {
            Value::Nil => None,
            // A `location` that runs code or carries a page, as `respond`'s (FEEDBACK55.md, Q-719;
            // branch feedback55; found in review).
            Value::Str(s) | Value::Css(s) if name == "location" && runs_or_carries(s) => {
                return fail(line, format!("`site_headers`: `location` is not given a {} URL, got {}", carried_scheme(s).unwrap_or("data:"), v.repr()))
            }
            Value::Str(s) | Value::Css(s) if !s.contains(['\r', '\n']) => Some(s.to_string()),
            Value::Int(_) | Value::Bool(_) => Some(v.display().to_string()),
            other => return fail(line, format!("`site_headers`: the value of `{}` is text on one line, or null, got {}", name, other.repr())),
        };
        hs.push((name, value));
    }
    if !site_wide(it, || format!("site_headers {}", prefix)) {
        return Ok(Value::Nil);
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
        Some(Value::Str(d) | Value::Css(d)) => {
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
    if !site_wide(it, || "tenant_from".into()) {
        return Ok(Value::Nil);
    }
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
            url = format!("{}{}", one_slash_first(without_dots(&url[..end])), &url[end..]).into();
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
    // Compared as they are, with no lower-case copy, and a segment that starts with neither `.`
    // nor `%` let go at once: `local_path` looks at each segment of a path with a `.` in it,
    // `/docs/page.html` too (FEEDBACK71.md, Q-968; branch feedback71).
    if seg.is_empty() || seg.len() > 6 || !matches!(seg.as_bytes()[0], b'.' | b'%') {
        return 0;
    }
    let is = |form: &str| seg.eq_ignore_ascii_case(form);
    if is(".") || is("%2e") {
        1
    } else if is("..") || is(".%2e") || is("%2e.") || is("%2e%2e") {
        2
    } else {
        0
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

/// A path of the site's own with one `/` first, where taking its dot segments out left more:
/// `/.//evil.com` is `//evil.com` without them, which a `Location` holds as another host, so the
/// path that stays on the site is `/evil.com`. A path that had no dot segments is left as it was
/// given (FEEDBACK71.md, Q-968; branch feedback71).
fn one_slash_first(path: String) -> String {
    if path.starts_with("//") {
        format!("/{}", path.trim_start_matches('/'))
    } else {
        path
    }
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
        // The mark of a miss's page sent as written is files()'s alone (FEEDBACK69.md, Q-964,
        // found in review; branch q964-miss-page).
        if k == crate::server::AS_WRITTEN {
            return fail(line, format!("`{}` header `{}` is Quillo's own", f, k));
        }
        // The mark of a server fn's own answer, which tells the page to take trusted CSS from it:
        // never on a program's answer, left out, so that one passing on another server fn's
        // answer and its headers works as before (FEEDBACK79.md, Q-1250, found in the second review of the trusted CSS; branch feedback72-81).
        if k == crate::server::MARKED {
            continue;
        }
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
            v => out.push((k, v.display_held().map_err(|o| o.at(line))?)),
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
    // Each list, map and text it makes held to `--max-memory` as it is made (FEEDBACK71.md,
    // Q-977; branch security).
    Ok(match Value::parse_json_held(&s).map_err(|o| o.at(line))? {
        Ok(v) => v,
        Err(e) => Value::err(Value::str(format!("invalid JSON: {}", e))),
    })
}

/// The most JSON `json.stringify` and an answer make, and how deep (FEEDBACK49.md, Q-606 and
/// Q-592; branch main).
pub const JSON_MOST: usize = 256 << 20;
pub const JSON_DEEPEST: usize = 10_000;

fn json_stringify(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // As `str` is (FEEDBACK26.md, Q-197; branch feedback26-fixes).
    let v = arg(&args, 0);
    let v = if crate::value::hiding() { crate::server::hidden(&v).unwrap_or(v) } else { v };
    // Written as it is held to its limits, with no tree of serde's made first (FEEDBACK49.md,
    // Q-606; found in review).
    let mut out = Vec::new();
    // Held, once it is large, to `--max-memory` too, as it is written: past it, the request's
    // stop, told by its kind (a map's key in the error's text was taken for it: found in review)
    // (FEEDBACK71.md, Q-977; branch security).
    match v.json_into_held(&mut out, JSON_MOST, JSON_DEEPEST) {
        Ok(()) => {}
        Err(None) => return Err(crate::httpd::OverMemory.at(line)),
        Err(Some(why)) => return Ok(Value::err(Value::str(format!("json.stringify: {}", why)))),
    }
    // SAFETY: only JSON text, which is UTF-8, was written.
    let text = unsafe { String::from_utf8_unchecked(out) };
    if arg(&args, 1).truthy() {
        // Indented, it is longer, and its indent grows with its depth: written through a `Held`
        // text, which stops at `--max-memory` as it grows (FEEDBACK71.md, Q-977, found in review:
        // a list nested 9,000 deep made 458 MB before it was compared; branch security).
        return Ok(Value::str(indented(&text).map_err(|o| o.at(line))?));
    }
    Ok(Value::str(text))
}

/// Compact JSON text indented by two spaces, as serde_json's pretty writer has it: `[]` and
/// `{}` stay as they are, and a string's own brackets and commas are text.
fn indented(json: &str) -> Result<String, crate::httpd::OverMemory> {
    let b = json.as_bytes();
    let mut out = Held::with_capacity(json.len() * 2);
    let mut level = 0usize;
    let newline = |out: &mut Held, level: usize| {
        out.push('\n');
        for _ in 0..level {
            out.push_str("  ");
            if out.is_over() {
                return;
            }
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
        if out.is_over() {
            break;
        }
        i += 1;
    }
    out.into_text()
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

/// A folder for `--allow-path` already resolved, as a policy's `[fs] paths` judged it: not looked
/// up again by its name, which could lead elsewhere by then (FEEDBACK82.md, Q-1280, found in
/// review; branch feedback82-85).
pub fn allow_real_path(real: std::path::PathBuf) {
    ALLOWED_PATHS.write().unwrap_or_else(std::sync::PoisonError::into_inner).push(real);
}

/// The folders `--allow-path` named, for what must stay out of the program's reach (FEEDBACK82.md,
/// Q-1285; branch feedback82-85).
pub(crate) fn allowed_paths() -> Vec<std::path::PathBuf> {
    ALLOWED_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
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
    led_out(it, &full).map(|_| full)
}

/// Where `full`, a path under the program's folder that a link leads out of it, really is, when
/// that is in a folder `--allow-path` named; else None, and the log says why, once. `file`'s and
/// `files`'s, so a folder linked into the program is served by both or by neither, with the same
/// word (FEEDBACK85.md, Q-1340; branch feedback82-85).
pub(crate) fn led_out(it: &Interp, full: &Path) -> Option<std::path::PathBuf> {
    let dirs = ALLOWED_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    let real = real_path(full);
    // By whichever name each was given: `public/s -> ../../shared` was refused for a folder named
    // `/System/Volumes/Data/.../shared`, the log saying it led outside (FEEDBACK85.md, Q-1340, found
    // by the coverage tests; branch feedback82-85).
    if let Some(r) = real.as_ref().filter(|r| dirs.iter().any(|d| crate::statics::within(r, d))) {
        return Some(r.clone());
    }
    // Said once, as a deploy's `uploads -> ../shared/uploads` is then a 404 with no other word.
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let to = real.map_or_else(|| "nowhere (the link is dangling)".to_string(), |r| r.display().to_string());
        eprintln!(
            "quillo: {} leads out of the program's folder through a link, to {}, and is refused; `--allow-path FOLDER` lets the program reach a folder outside",
            full.strip_prefix(&*it.base_dir).unwrap_or(full).display(),
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
    // The folder and where the link leads by whichever name each has: a link written to the
    // other name of a place in the folder is in it (FEEDBACK85.md, Q-1340, found by the coverage
    // tests; branch feedback82-85).
    let resolved = || match (real_path(full), root.canonicalize()) {
        (Some(real), Ok(base)) if crate::statics::within(&real, &base) => Some(Some(real)),
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
/// plain names: a `..` past it is refused. A link that leads nowhere is refused too, as
/// `stays_in` refuses one in the program's folder (`real_path`): `write_file("../shared/dl")`,
/// with `shared/dl` a link to `../outside/made.txt`, made that file outside every folder named
/// (FEEDBACK29.md, Q-357's folders; found fixing FEEDBACK83.md Q-1300 for the coverage tests;
/// branch feedback82-85).
fn allowed_path(it: &Interp, p: &str) -> Option<std::path::PathBuf> {
    let dirs = ALLOWED_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    if dirs.is_empty() {
        return None;
    }
    let full = if Path::new(p).is_absolute() { std::path::PathBuf::from(p) } else { it.base_dir.join(p) };
    let real = real_path(&full)?;
    // By whichever name each was given: `--allow-path /System/Volumes/Data/.../shared` refused
    // `../shared/a.txt`, which realpath names `/private/...` from a program there, telling the
    // program to give `--allow-path ../shared` (FEEDBACK85.md, Q-1340, found by the coverage tests;
    // branch feedback82-85). What may be written there is `own_file`'s, by either name too.
    dirs.iter().any(|d| crate::statics::within(&real, d)).then_some(real)
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
    // Each in the one form the system names files in: a program started as
    // `/System/Volumes/Data/.../main.qwe` had its files by that name, and the same files named
    // `/private/...` (or the other way round) were none of its own, so `delete_file` of its folder
    // by the other name removed it, program and database (FEEDBACK85.md, Q-1340, found by the
    // coverage tests; branch feedback82-85).
    let sys = |p: &Path| folded_path(&crate::statics::sys_form_of(p));
    // The file, or a folder that holds one (`delete_file("lib")`).
    let folded_target = sys(&target);
    let loaded: Vec<std::path::PathBuf> = it.loaded.iter().map(|f| real_as_made(f)).collect();
    if loaded.iter().any(|f| sys(f).starts_with(&folded_target)) {
        return Some(OWN_SOURCE);
    }
    // Its `.quillo` folders, and what is in them, there yet or not, or a folder that holds one:
    // the libraries of its plugins, their lock and its session key were files like others, and a
    // program, or an upload by the name a visitor gave, put another library in for the next start,
    // as Q-361 closed for main.qwe (FEEDBACK85.md, Q-1340, found by the review of b7f7c26; branch
    // feedback82-85). Quillo writes them itself, never through here. Looked for only where the
    // path, as it really is, has a `.quillo` in it, the target is a folder, or it has a second
    // name: a write of another file looks at nothing more.
    let quillo = std::cell::OnceCell::new();
    let quillo = || quillo.get_or_init(|| quillo_dirs(it, &loaded));
    if folded_target.components().any(|c| c.as_os_str() == ".quillo") && quillo().iter().any(|q| folded_target.starts_with(sys(q))) {
        return Some(OWN_QUILLO);
    }
    let folded = fold_name(taken);
    if folded == ".env" || folded.starts_with(".env.") {
        return Some(OWN_SETTINGS);
    }
    // What the program was confirmed to reach, by its name whether it is there yet or not
    // (FEEDBACK83.md, Q-1300, found in review; branch feedback82-85).
    if folded == crate::confirm::FILE {
        return Some(OWN_CONFIRMED);
    }
    // One of them under another name: a second hard link writes into it (FEEDBACK55.md, Q-701;
    // branch feedback55).
    let mut a_folder = false;
    let mut linked = None;
    for f in &both {
        if let Ok(m) = std::fs::metadata(f) {
            if let Some(why) = own_meta(&m) {
                return Some(why);
            }
            a_folder |= m.is_dir();
            #[cfg(unix)]
            if m.is_file() && std::os::unix::fs::MetadataExt::nlink(&m) > 1 {
                linked = crate::statics::id_of(&m);
            }
        }
    }
    // A second name of a file in a `.quillo` folder: looked for there, as few files are.
    if let Some(id) = linked {
        if quillo().iter().any(|q| holds_file(q, id, 8)) {
            return Some(OWN_QUILLO);
        }
    }
    // A folder that holds one of its files, by the folders themselves (`statics::holds_any`): by
    // the names alone, `/System/Volumes/Data` (which only the long name names), `/.nofollow/...`
    // or a bind mount held none, and `delete_file` removed the program with it; 0.8.10 refused the
    // first for a program started by the long name (FEEDBACK85.md, Q-1340, found by the review of
    // 3e793e3; branch feedback82-85). A file costs nothing more: the look above said what it is.
    if a_folder && crate::statics::holds_any(&real, loaded.iter().cloned()) {
        return Some(OWN_SOURCE);
    }
    if a_folder && (quillo().iter().any(|q| sys(q).starts_with(&folded_target)) || crate::statics::holds_any(&real, quillo().iter().cloned())) {
        return Some(OWN_QUILLO);
    }
    // What a path of the settings or the TLS key leads to now, by any name (FEEDBACK55.md, Q-701;
    // branch feedback55).
    if let Some(why) = both.iter().find_map(|f| std::fs::metadata(f).ok().and_then(|m| crate::statics::id_of(&m)).and_then(named_id)) {
        return Some(why);
    }
    // And the paths themselves, while nothing is there: `--env-file`'s and `--tls-key`'s, as
    // given or as they lead (FEEDBACK55.md, Q-701; branch feedback55).
    let named: Vec<(std::path::PathBuf, &'static str)> = own().named.clone();
    if let Some((_, why)) = named.iter().find(|(n, _)| [n.clone(), real_as_made(n)].iter().any(|n| both.iter().any(|f| sys(n) == sys(f)))) {
        return Some(why);
    }
    // On Windows, a short name may be any file's (FEEDBACK55.md, Q-701; branch feedback55).
    #[cfg(windows)]
    if full.strip_prefix(&*it.base_dir).unwrap_or(full).components().any(|c| short_name(&c.as_os_str().to_string_lossy())) {
        return Some("a short name Windows gives a file may be one of the program's own files");
    }
    // A database it has open, by any name, and the folder holding one (FEEDBACK55.md, Q-702 and
    // Q-705; branch feedback55). db.rs keeps them by the name realpath gave as they were opened,
    // which may be either of a folder's two (`/private/...`, `/System/Volumes/Data/private/...`):
    // asked by both (FEEDBACK85.md, Q-1340, found by the coverage tests; branch feedback82-85).
    let names = |p: &Path| {
        let (short, long) = (crate::statics::sys_form_of(p).into_owned(), crate::statics::long_form(p));
        if short == long { vec![short] } else { vec![short, long] }
    };
    // Judged without opening anything: for a journal's name db.rs reads the first bytes of the file
    // it is of (`sqlite_file`), and a FIFO there, or a link to one, had that open wait for a writer,
    // under the interpreter's lock, for ever. What is no file of bytes is no database (FEEDBACK85.md,
    // Q-1340, found by the review of b7f7c26; branch feedback82-85).
    let unopened = |f: &Path| crate::db::journal_of(f).map_or(true, |db| std::fs::metadata(&db).map_or(true, |m| m.is_file()));
    if let Some(why) = both.iter().flat_map(|f| names(f)).filter(|f| unopened(f)).find_map(|f| crate::db::database_file(&f)) {
        return Some(why);
    }
    let targets: Vec<std::path::PathBuf> = names(&target).iter().map(|t| folded_path(t)).collect();
    if targets.iter().any(|t| crate::db::opened_under(t)) {
        return Some("it holds the database the program has open");
    }
    // A program may write its own migrations, and its tenants' databases; a visitor's upload may
    // not, though the tenant's has not been opened since the start (FEEDBACK55.md, Q-702; branch
    // feedback55).
    if upload && folded_target.starts_with(sys(&real_as_made(&it.base_dir).join("migrations"))) {
        return Some("it is in the program's migrations");
    }
    if upload && targets.iter().any(|t| crate::db::tenant_file(t)) {
        return Some("it is where a tenant's database is");
    }
    None
}

/// The program's `.quillo` folders, there or not, by the names the system gives: beside its main
/// file (`base_dir`) and beside each file it loaded, as a plugin's `.quillo/native` and
/// `plugins.lock` are beside its source (FEEDBACK85.md, Q-1340, found by the review of b7f7c26;
/// branch feedback82-85).
fn quillo_dirs(it: &Interp, loaded: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for d in crate::statics::real_dir(&it.base_dir).into_iter().chain(loaded.iter().filter_map(|f| f.parent().map(Path::to_path_buf))) {
        let q = crate::statics::sys_form(d).join(".quillo");
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

/// Whether the file `id` is in the folder `dir`, `depth` folders down at most, no link followed.
fn holds_file(dir: &Path, id: crate::statics::FileId, depth: u32) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| match std::fs::symlink_metadata(e.path()) {
        Ok(m) if m.is_dir() => depth > 0 && holds_file(&e.path(), id, depth - 1),
        Ok(m) => crate::statics::id_of(&m) == Some(id),
        Err(_) => false,
    })
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
    // A copy without them is made only for an address that has a tab, a line break or a `\`; the
    // checks below read the address as it is, with no lower-case copy, as one is read for each
    // request a login's `next` comes in (FEEDBACK71.md, Q-968; branch feedback71).
    let cleaned: String;
    let s = if s.bytes().any(|b| matches!(b, b'\t' | b'\n' | b'\r' | b'\\')) {
        cleaned = s.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).map(|c| if c == '\\' { '/' } else { c }).collect();
        cleaned.as_str()
    } else {
        s
    };
    let s = s.trim_matches(|c: char| c <= ' ');
    if !s.starts_with('/') || s.starts_with("//") || s.bytes().any(|b| b < 0x20 || b == 0x7f) || (!s.is_ascii() && s.chars().any(|c| c.is_control())) {
        return None;
    }
    // A `:` before the path's first `/`, `?` or `#` would be a scheme; none can be here, as it
    // starts with `/`, but `/%2F/x` decodes to `//x` in some servers: refused too.
    let refused = |p: &str| p.starts_with("//") || p.get(..4).is_some_and(|h| h.eq_ignore_ascii_case("/%2f") || h.eq_ignore_ascii_case("/%5c"));
    if refused(s) {
        return None;
    }
    // Nor one that starts so once its `.` and `..` are taken out, as a browser takes them out:
    // `/.//evil.example`, `/%2e//evil.example` and `/a/..//evil.example` are `//evil.example`. A
    // path with no segment that starts with `.` or `%` has none, and is not walked: one look at
    // its bytes, up to the query or fragment, so `/docs/page.html` is not (FEEDBACK71.md, Q-968;
    // branch feedback71).
    let mut end = s.len();
    let mut maybe = false;
    let mut after_slash = false;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'?' | b'#' => {
                end = i;
                break;
            }
            b'.' | b'%' if after_slash => maybe = true,
            _ => {}
        }
        after_slash = b == b'/';
    }
    let path = &s[..end];
    if maybe && path.split('/').any(|seg| dots(seg) > 0) && refused(&without_dots(path)) {
        return None;
    }
    Some(s.to_string())
}

fn local_path_fn(_: &mut Interp, args: Vec<Value>, _: u32) -> R {
    Ok(match args.first() {
        Some(Value::Str(s) | Value::Css(s)) => local_target(s).map_or(Value::Nil, Value::str),
        _ => Value::Nil,
    })
}

/// The way out of the folder, for a refusal of a path outside it: `--allow-path` and the folder to
/// name, as written; `file`'s word in the log names it, `read_file`'s did not, and a program grew
/// a Rust function to read one file (FEEDBACK85.md, Q-1341; branch feedback82-85).
pub(crate) fn way_out(p: &str, verb: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let path = Path::new(p);
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.display().to_string(),
        _ => p.to_string(),
    };
    format!("; `--allow-path {}` (on `quillo run` or `quillo mcp`) lets the program {} it", dir, verb)
}

/// `way_out`, or where a link is what leads `p` out, that link: where it leads (and the folder to
/// name for it), or that it leads nowhere. The link nearest the name whose own folder is the
/// program's or one `--allow-path` names is the one: a link that leads nowhere in `../shared`,
/// given as `--allow-path ../shared`, was told to give `--allow-path ../shared` (FEEDBACK83.md,
/// found by the docs agent writing up 2a1d3f7; branch feedback82-85). An error's word: the cost
/// is a refusal's.
pub(crate) fn why_out(it: &Interp, p: &str, verb: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let full = if Path::new(p).is_absolute() { std::path::PathBuf::from(p) } else { it.base_dir.join(p) };
    let named = allowed_paths();
    let roots: Vec<std::path::PathBuf> = it.base_dir.canonicalize().into_iter().chain(named.iter().cloned()).collect();
    for at in full.ancestors().take(64) {
        let Ok(m) = std::fs::symlink_metadata(at) else { continue };
        if !m.file_type().is_symlink() {
            continue;
        }
        // By whichever name each was given (FEEDBACK85.md, Q-1340, found by the coverage tests;
        // branch feedback82-85).
        if !at.parent().and_then(|d| d.canonicalize().ok()).is_some_and(|d| roots.iter().any(|r| crate::statics::within(&d, r))) {
            break;
        }
        let shown = at.strip_prefix(&*it.base_dir).map_or_else(|_| at.display().to_string(), |r| r.display().to_string());
        let to = std::fs::read_link(at).map(|t| t.display().to_string()).unwrap_or_default();
        return match real_path(at) {
            None => format!("; {} is a link that leads nowhere (to {})", shown, to),
            Some(real) => {
                let past = real.join(full.strip_prefix(at).unwrap_or(Path::new("")));
                let outside = if named.is_empty() { "outside the program's folder" } else { "outside the program's folder and those `--allow-path` names" };
                format!("; {} is a link to {}, {}{}", shown, real.display(), outside, way_out(&past.display().to_string(), verb))
            }
        };
    }
    way_out(p, verb)
}

/// Whether `p` names the program's folder itself, which no `--allow-path` lets a program delete:
/// by the folder, whatever name it is given (`/System/Volumes/Data/...` or `/private/...` on
/// macOS, `../app` from it), not by the text of the name it was started by (FEEDBACK85.md, Q-1340,
/// found by the coverage tests; branch feedback82-85).
fn full_is_base(it: &Interp, p: &str) -> bool {
    p.is_empty() || local_path(it, p).is_some_and(|f| is_base(it, &f))
}

/// Whether `full` is the program's folder, by the folder itself (`full_is_base`).
fn is_base(it: &Interp, full: &Path) -> bool {
    full == &*it.base_dir || same_file(full, &it.base_dir)
}

/// `read_file(path)`: the file's text, or an error value.
fn read_file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "read_file", line)?;
    let Some(full) = local_path(it, &p) else {
        return Ok(Value::err(Value::str(format!("read_file: path must stay inside the program's folder: {}{}", p, why_out(it, &p, "read")))));
    };
    // The paths its file was confirmed for (FEEDBACK83.md, Q-1300; branch feedback82-85).
    if let Err(why) = crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &p, false) {
        return Ok(Value::err(Value::str(format!("read_file: {}", why))));
    }
    // Its bytes and its text, asked about first under `--max-memory`, by the size the open file
    // gives, as `fs::read` reads one: what it does, and the same calls (FEEDBACK71.md, Q-977;
    // branch security). Where the limit is set, read no further than it, whatever the size said:
    // a FIFO, or a file that grows as it is read, says less than it gives (found in review); and
    // its text made through a `Held` text, U+FFFD three bytes for each byte that is not UTF-8.
    let most = crate::httpd::most_bytes();
    let read = std::fs::File::open(&full).and_then(|mut f| {
        let size = f.metadata().map(|m| m.len().min(usize::MAX as u64) as usize).unwrap_or(0);
        if let Err(stop) = crate::httpd::making(size) {
            return Ok(Err(stop));
        }
        let mut b = Vec::new();
        b.try_reserve_exact(size).map_err(|e| std::io::Error::new(std::io::ErrorKind::OutOfMemory, e))?;
        if most == usize::MAX {
            std::io::Read::read_to_end(&mut f, &mut b)?;
            return Ok(Ok(b));
        }
        let mut chunk = vec![0u8; 64 << 10];
        loop {
            let n = match std::io::Read::read(&mut f, &mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if !crate::value::bytes_room(&mut b, n) {
                return Ok(Err(crate::httpd::OverMemory));
            }
            b.extend_from_slice(&chunk[..n]);
        }
        Ok(Ok(b))
    });
    let read = match read {
        Ok(Err(stop)) => return Err(stop.at(line)),
        Ok(Ok(b)) => Ok(b),
        Err(e) => Err(e),
    };
    Ok(match read {
        Ok(b) if most == usize::MAX => Value::str(String::from_utf8_lossy(&b)),
        Ok(b) => Value::str(crate::value::lossy_held(b).map_err(|o| o.at(line))?),
        Err(e) => Value::err(Value::str(format!("read_file: {}: {}", p, e))),
    })
}

/// `write_file(path, text)`: null, or an error value. Creates missing folders.
fn write_file(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let p = s_arg(&args, 0, "write_file", line)?;
    // A list's text held as `str` holds it (FEEDBACK71.md, Q-977; branch security).
    let text = arg(&args, 1).display_held().map_err(|o| o.at(line))?;
    let Some(full) = local_path(it, &p) else {
        return Ok(Value::err(Value::str(format!("write_file: path must stay inside the program's folder: {}{}", p, why_out(it, &p, "write")))));
    };
    if let Err(why) = crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &p, true) {
        return Ok(Value::err(Value::str(format!("write_file: {}", why))));
    }
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
    if let Err(why) = crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &p, false) {
        return Ok(Value::err(Value::str(format!("check_program: {}", why))));
    }
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
    let opts = crate::check::Options { confined: true, ..crate::check::Options::plain() };
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
    let Some(full) = local_path(it, &p).filter(|f| !is_base(it, f)) else {
        return Ok(Value::err(Value::str(format!("delete_file: path must be inside the program's folder: {}{}", p, if full_is_base(it, &p) { String::new() } else { why_out(it, &p, "delete") }))));
    };
    if let Some(why) = own_file(it, &full, false) {
        return Ok(Value::err(Value::str(format!("delete_file: will not delete {}: {}", p, why))));
    }
    if let Err(why) = crate::confirm::path_ok(it.cur_file.as_deref(), &it.base_dir, &p, true) {
        return Ok(Value::err(Value::str(format!("delete_file: {}", why))));
    }
    // A folder is deleted whole: what is in it is judged first, each file by itself, as
    // `remove_dir_all` walks it (FEEDBACK85.md, Q-1340, found by the review of 3e793e3; branch
    // feedback82-85).
    if std::fs::symlink_metadata(&full).is_ok_and(|m| m.is_dir()) {
        match own_inside(it, &full) {
            Ok(None) => {}
            Ok(Some(why)) => return Ok(Value::err(Value::str(format!("delete_file: will not delete {}: {}", p, why)))),
            Err(e) => return Ok(Value::err(Value::str(format!("delete_file: {}: {}", p, e)))),
        }
    }
    let r = if full.is_dir() { std::fs::remove_dir_all(&full) } else { std::fs::remove_file(&full) };
    Ok(match r {
        Ok(()) => Value::Nil,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Nil,
        Err(e) => Value::err(Value::str(format!("delete_file: {}: {}", p, e))),
    })
}

/// Why the folder `dir`, which `delete_file` would delete whole, holds one of the program's own
/// files, if it does: each file in it judged by itself, its device and inode (`own_id`: its
/// sources, settings, policy and key, and a database it has open), a journal by the database it
/// is of, and a folder that holds a file the program loaded or is one of its `.quillo` folders
/// (`kept`), walked as `remove_dir_all` walks it. By the folder's name, `own_file` asks db.rs by
/// the names it keeps a database by, which a database opened by another (`/.nofollow/...`, a bind
/// mount) is not: its folder went with it (FEEDBACK85.md, Q-1340, found by the review of 3e793e3;
/// branch feedback82-85). And, found by the review of b7f7c26 (FEEDBACK85.md, Q-1340; branch
/// feedback82-85):
/// - Nothing is opened to be judged, only looked at: a journal's database read by its first bytes
///   (`db::database_file`) waited for ever on a FIFO, under the interpreter's lock.
/// - A link is judged by what it leads to, as naming it is (`own_file`): one the program's code was
///   used through, the file `--env-file` names, or the database opened through it went with its
///   folder, and the next start read what was put in its place. Only the link goes, as before.
/// - The walk is by each folder opened (`openat`, `fstatat`), as `remove_dir_all` deletes: by a
///   path, a tree deeper than a path may be long was "File name too long" and stayed.
/// A folder in it that cannot be read is an error, and nothing is deleted: `remove_dir_all`
/// deleted what it could and stopped there. A look at each file, as the deletion makes.
fn own_inside(it: &Interp, dir: &Path) -> std::io::Result<Option<&'static str>> {
    let loaded: Vec<std::path::PathBuf> = it.loaded.iter().map(|f| real_as_made(f)).collect();
    let mut kept: std::collections::HashMap<crate::statics::FileId, &'static str> = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for f in &loaded {
        for chain in [crate::statics::long_form(f), f.clone()] {
            for a in chain.ancestors().skip(1) {
                if !seen.insert(a.to_path_buf()) {
                    break;
                }
                if let Some(id) = std::fs::metadata(a).ok().and_then(|m| crate::statics::id_of(&m)) {
                    kept.insert(id, OWN_SOURCE);
                }
            }
        }
    }
    for q in quillo_dirs(it, &loaded) {
        if let Some(id) = std::fs::metadata(&q).ok().and_then(|m| crate::statics::id_of(&m)) {
            kept.entry(id).or_insert(OWN_QUILLO);
        }
    }
    let named: Vec<(std::path::PathBuf, &'static str)> = own().named.iter().map(|(n, why)| (std::path::PathBuf::from(fold_name(&crate::statics::sys_form_of(n).to_string_lossy())), *why)).collect();
    let said = |why: &'static str| if why == crate::db::OPEN { "it holds the database the program has open" } else { why };
    walk_kept(dir, &real_as_made(dir), &kept, &named).map(|w| w.map(said))
}

/// `own_inside`'s walk, on systems that open a folder by its parent's descriptor.
#[cfg(unix)]
fn walk_kept(dir: &Path, real: &Path, kept: &std::collections::HashMap<crate::statics::FileId, &'static str>, named: &[(std::path::PathBuf, &'static str)]) -> std::io::Result<Option<&'static str>> {
    use std::ffi::{CStr, CString, OsStr};
    use std::os::unix::ffi::OsStrExt;
    struct Opened(*mut libc::DIR);
    impl Drop for Opened {
        fn drop(&mut self) {
            // SAFETY: a stream fdopendir gave, closed once.
            unsafe { libc::closedir(self.0) };
        }
    }
    let open = |at: libc::c_int, name: &CStr| -> std::io::Result<Opened> {
        // SAFETY: a descriptor of a folder open here (or AT_FDCWD) and a NUL-ended name.
        let fd = unsafe { libc::openat(at, name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the descriptor just opened, which the stream then owns.
        let d = unsafe { libc::fdopendir(fd) };
        if d.is_null() {
            let e = std::io::Error::last_os_error();
            // SAFETY: the descriptor no stream took.
            unsafe { libc::close(fd) };
            return Err(e);
        }
        Ok(Opened(d))
    };
    let look = |at: libc::c_int, name: &CStr, follow: bool| -> Option<libc::stat> {
        // SAFETY: an all-zero stat is a valid value to be written over.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: as `open`; the stat is this frame's.
        (unsafe { libc::fstatat(at, name.as_ptr(), &mut st, if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW }) } == 0).then_some(st)
    };
    let id = |st: &libc::stat| (st.st_dev as u64, st.st_ino as u64);
    let own = |st: &libc::stat| own_id(id(st), st.st_nlink as u64, None).or_else(|| kept.get(&id(st)).copied());
    let top = CString::new(dir.as_os_str().as_bytes()).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let mut stack = vec![(open(libc::AT_FDCWD, &top)?, real.to_path_buf())];
    while let Some(last) = stack.last() {
        let (d, at) = (last.0 .0, last.1.clone());
        // SAFETY: the stream is open while it is on the stack.
        let e = unsafe { libc::readdir(d) };
        if e.is_null() {
            stack.pop();
            continue;
        }
        // SAFETY: readdir's entry holds a NUL-ended name, valid until the next readdir of `d`.
        let name = unsafe { CStr::from_ptr((*e).d_name.as_ptr()) }.to_owned();
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        // SAFETY: the stream's own descriptor.
        let fd = unsafe { libc::dirfd(d) };
        let Some(st) = look(fd, &name, false) else {
            // Gone since it was listed: nothing of it to delete.
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::NotFound {
                continue;
            }
            return Err(e);
        };
        let here = at.join(OsStr::from_bytes(name.to_bytes()));
        let kind = st.st_mode & libc::S_IFMT;
        if kind == libc::S_IFLNK {
            // Looked at where it leads (a look, which opens nothing); a link that leads nowhere,
            // by its own name, as `--env-file`'s path is kept while nothing is there.
            if let Some(to) = look(fd, &name, true) {
                if let Some(why) = own(&to) {
                    return Ok(Some(why));
                }
                if to.st_mode & libc::S_IFMT == libc::S_IFDIR {
                    if let Ok(r) = here.canonicalize() {
                        let forms = [crate::statics::sys_form_of(&r).into_owned(), crate::statics::long_form(&r)];
                        if forms.iter().any(|f| crate::db::opened_under(Path::new(&fold_name(&f.to_string_lossy())))) {
                            return Ok(Some(crate::db::OPEN));
                        }
                    }
                }
            }
            let folded = std::path::PathBuf::from(fold_name(&crate::statics::sys_form_of(&here).to_string_lossy()));
            if let Some((_, why)) = named.iter().find(|(n, _)| *n == folded) {
                return Ok(Some(why));
            }
            continue;
        }
        if let Some(why) = own(&st) {
            return Ok(Some(why));
        }
        if kind == libc::S_IFDIR {
            let next = open(fd, &name)?;
            stack.push((next, here));
        } else if let Some(stem) = crate::db::journal_of(Path::new(OsStr::from_bytes(name.to_bytes()))) {
            let stem = CString::new(stem.as_os_str().as_bytes()).unwrap_or_default();
            if look(fd, &stem, true).is_some_and(|s| s.st_mode & libc::S_IFMT == libc::S_IFREG && crate::db::open_id(id(&s))) {
                return Ok(Some(crate::db::OPEN));
            }
        }
    }
    Ok(None)
}

/// `own_inside`'s walk elsewhere, by paths.
#[cfg(not(unix))]
fn walk_kept(dir: &Path, _: &Path, kept: &std::collections::HashMap<crate::statics::FileId, &'static str>, _: &[(std::path::PathBuf, &'static str)]) -> std::io::Result<Option<&'static str>> {
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d)? {
            let path = e?.path();
            let m = std::fs::symlink_metadata(&path)?;
            let followed = if m.file_type().is_symlink() { std::fs::metadata(&path).ok() } else { Some(m.clone()) };
            if let Some(why) = followed.as_ref().and_then(own_meta).or_else(|| followed.as_ref().and_then(crate::statics::id_of).and_then(|id| kept.get(&id).copied())) {
                return Ok(Some(why));
            }
            if m.is_dir() {
                todo.push(path);
            }
        }
    }
    Ok(None)
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
        Some(Value::Str(s) | Value::Css(s)) => match Rounding::parse(s) {
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
    // Its length, known before it is made, compared with `--max-memory`: 300 lists of 30,000 made
    // 9 million items (FEEDBACK71.md, Q-977, found in review; branch security).
    let n = l.iter().map(|x| if let Value::List(inner) = x { inner.len() } else { 1 }).fold(0usize, usize::saturating_add);
    crate::value::list_of(n).map_err(|o| o.at(line))?;
    let mut out = Vec::with_capacity(n);
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
        Value::Str(s) | Value::Css(s) => Ok(Value::str(s.chars().take(n).collect::<String>())),
        _ => Ok(Value::list(l_arg(&args, 0, "take", line)?.iter().take(n).cloned().collect())),
    }
}

fn drop_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let n = i_arg(&args, 1, "drop", line)?.max(0) as usize;
    match arg(&args, 0) {
        Value::Str(s) | Value::Css(s) => Ok(Value::str(s.chars().skip(n).collect::<String>())),
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
            // In a script, the key written out is paid for as what a builtin reads
            // (FEEDBACK75.md, Q-1055; branch feedback75).
            k if it.fuel != u64::MAX => {
                it.script_read([&k], line)?;
                k.key_text_held().map_err(|o| o.at(line))?
            }
            k => k.key_text_held().map_err(|o| o.at(line))?,
        };
        // A new group where the map has no room left: the room it grows to compared with
        // `--max-memory` first (FEEDBACK71.md, Q-977; branch security).
        if groups.len() == groups.capacity() && groups.capacity() >= crate::httpd::LARGE / (2 * crate::value::ENTRY_BYTES) && !groups.contains_key(&k) {
            crate::value::map_of(groups.capacity().saturating_mul(2)).map_err(|o| o.at(line))?;
        }
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
        Value::Str(s) | Value::Css(s) => s,
        other => return fail(line, format!("`join_on`'s fourth argument is the others' key (a str), got {}", other.type_name())),
    };
    let name: String = match arg(&args, 4) {
        Value::Nil => key.strip_suffix("_id").filter(|s| !s.is_empty()).unwrap_or("joined").to_string(),
        Value::Str(s) | Value::Css(s) => s.to_string(),
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
        Value::Str(s) | Value::Css(s) => {
            let needle = x.display_held().map_err(|o| o.at(line))?;
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
    // Asked about first under `--max-memory` (FEEDBACK71.md, Q-977; branch security).
    made(s.len() * n as usize, line)?;
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
        Some(Value::Str(t) | Value::Css(t)) => t.to_string(),
        None | Some(Value::Nil) => heading().unwrap_or_default(),
        Some(v) => return fail(line, format!("`markdown_page`'s title is a str, got {}", v.repr())),
    };
    let lang = match opts.get("lang") {
        Some(Value::Str(l) | Value::Css(l)) if !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => format!(" lang=\"{}\"", l),
        None | Some(Value::Nil) => String::new(),
        Some(v) => return fail(line, format!("`markdown_page`'s lang is a language tag such as \"en\", got {}", v.repr())),
    };
    let mut inner = vec![Value::str(&*text)];
    if let Some(h) = opts.get("html") {
        inner.push(Value::map([("html".to_string(), h.clone())].into_iter().map(|(k, v)| (k.into(), v)).collect()));
    }
    let body = markdown(it, inner, line)?.display();
    // The page held as its body is (FEEDBACK71.md, Q-977; branch security).
    let mut page = Held::with_capacity(body.len() + title.len() + 200);
    page.push_str("<!DOCTYPE html><html");
    page.push_str(&lang);
    page.push_str("><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>");
    crate::interp::escape_into(&title, &mut page);
    page.push_str("</title></head><body>");
    page.push_str(&body);
    page.push_str("</body></html>");
    Ok(Value::Html(page.into_text().map_err(|o| o.at(line))?.into()))
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
    // Written through a `Held` text, which stops at `--max-memory`: a link's long address used 200
    // times made 160 MB (FEEDBACK71.md, Q-977, found in review; branch security).
    let mut out = Held::with_capacity(s.len() * 3 / 2);
    let _ = pulldown_cmark::html::write_html_fmt(&mut out, events);
    if over.get() {
        return fail(line, crate::httpd::over_compute());
    }
    let mut out = out.into_text().map_err(|o| o.at(line))?;
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
/// the server there is no one to ask. A tool's question to its user, asked by the AI client, is
/// `about(question, choices, default)` (`mcp::about`), which was `ask` with three arguments: `ask`
/// takes two, and a third is refused before it is called (`Interp::call`; FEEDBACK56.md, the
/// user's decision 2026-11-09; branch feedback56).
fn ask(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    // In a tool's call, the page's form is a mistake the call stops at, which no `??` takes
    // (FEEDBACK56.md, Part E, second review; branch feedback56).
    if it.asking.is_some() || it.in_tool {
        return crate::mcp::ask_in_a_tool(it, args.len(), line);
    }
    fail(line, IN_THE_BROWSER)
}

/// What `ask` and `notify` are for, said where they are called on the server, as they run and by
/// `quillo check` where it knows the side: a page that is open, and what reaches the rest
/// (FEEDBACK67.md, Q-959; branch pwa).
pub const IN_THE_BROWSER: &str = "`ask` and `notify` speak to someone in the browser, on a page that is open: call them from a component (an event handler); to reach someone who is not on the page, `notification.send(user, { title, body, url })` sends to their devices, and `publish` updates the pages that are open";

/// `run_script(source, vars, { timeout })`: users' code, run in a sandbox (script.rs), for 100 ms
/// unless `timeout` says (FEEDBACK75.md, Q-1055; branch feedback75).
fn run_script(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let src = s_arg(&args, 0, "run_script", line)?;
    let vars = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(_) => Some(m_arg(&args, 1, "run_script", line)?),
    };
    let opts = match args.get(2) {
        None | Some(Value::Nil) => None,
        Some(_) => Some(m_arg(&args, 2, "run_script", line)?),
    };
    let ms = crate::script::timeout_of(opts.as_deref()).or_else(|why| fail(line, why))?;
    // `--max-memory`'s stop said at this call, the script's lines being its own.
    crate::script::run_script_for(it, &src, vars.as_deref(), ms).map_err(|c| match c {
        Ctrl::Error(e) if e.memory_stop => crate::httpd::OverMemory.at(line),
        c => c,
    })
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
            // The server's own answer, kept: its trusted CSS stays trusted (FEEDBACK79.md,
            // Q-1250; branch feedback72-81).
            (Ok(crate::interp::from_own_island_json(&a.value)), topics, Some(a), false)
        }
        None => {
            crate::metrics::live_run(false);
            let seq = it.shared.live_seq();
            let (cookies, own, read, acted) = (it.set_cookies.len(), it.own_seqs.len(), it.read_request, it.acted);
            it.read_request = false;
            it.acted = false;
            let outer = it.reads.replace(std::collections::HashSet::new());
            let in_run = std::mem::replace(&mut it.in_run, true);
            let page_reads = crate::value::page_reads();
            let r = it.call(&f, rest.clone(), line);
            it.in_run = in_run;
            let topics = std::mem::replace(&mut it.reads, outer).unwrap_or_default();
            if let Some(o) = &mut it.reads {
                o.extend(topics.iter().cloned());
            }
            let mut topics: Vec<String> = topics.into_iter().collect();
            topics.sort();
            // A page state is one visitor's page's: a run that read one is theirs alone, as one
            // that read the visitor is (FEEDBACK68.md, Q-963, found in review: a loader reading
            // one through a helper gave the first visitor's to the next, and to a call with no
            // page; branch global-state).
            let personal = it.read_request || crate::value::page_reads() != page_reads;
            it.read_request |= read;
            // What it did is told to a run around it; one that read the viewer or acted is no one
            // else's, even the viewer's other tab (FEEDBACK55.md, Q-881 and Q-888; branch
            // feedback55).
            let did = it.acted;
            it.acted |= acted;
            let nobody_elses = personal || did;
            // Held to `--max-memory` where it is set (FEEDBACK71.md, Q-977; branch security).
            let answer = match r.as_ref().ok().filter(|v| !matches!(v, Value::Err(..) | Value::Response(_)) && crate::server::hidden(v).is_none()) {
                Some(v) => Some(Arc::new(crate::value::js_numbers(false, || crate::livecache::Answer::of(v, false)).ok_or_else(|| crate::httpd::OverMemory.at(line))?)),
                None => None,
            };
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
            let a = match kept {
                Some(a) => a,
                None => Arc::new(crate::value::js_numbers(false, || crate::livecache::Answer::of(v, false)).ok_or_else(|| crate::httpd::OverMemory.at(line))?),
            };
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
        if site_wide(it, || "style_with".into()) {
            it.style_with_fn = Some(f.clone());
        }
        return Ok(Value::Nil);
    }
    let cmd = s_arg(&args, 0, "style_with", line)?;
    if cmd.trim().is_empty() {
        return fail(line, "`style_with` takes the command to run, like style_with(\"npx postcss\")");
    }
    // A command run over all the program's CSS: a taken file's, only where it was confirmed.
    if !site_wide(it, || "style_with".into()) {
        return Ok(Value::Nil);
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
    if let Some(Value::Str(id) | Value::Css(id)) = args.get(2) {
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
        Some(Value::Str(_) | Value::Css(_)) => Ok(Value::Nil),
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
        let t = a.display_held().map_err(|o| o.at(line))?;
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
    // `{ replace, scroll }`, both true or false, as the browser checks them (FEEDBACK84.md,
    // Q-1326; branch feedback82-85).
    match args.get(1) {
        None | Some(Value::Nil) => {}
        Some(Value::Map(m)) => {
            for (k, v) in m.iter() {
                if !matches!(&**k, "replace" | "scroll") {
                    return fail(line, format!("`navigate` has no option `{}`: it takes replace and scroll", &**k));
                }
                if !matches!(v, Value::Bool(_) | Value::Nil) {
                    return fail(line, format!("`navigate`'s `{}` is true or false, got {}", &**k, v.repr()));
                }
            }
        }
        Some(other) => return fail(line, format!("`navigate`'s options are a map, `{{ replace: true, scroll: false }}`, got {}", other.repr())),
    }
    Ok(Value::Nil)
}

/// `page_head({ title, description, image, canonical })`: what goes in the head of the page being
/// made, its tags written there once it is (src/head.rs), the innermost call winning each key; an
/// `image` or `canonical` that is a path is made the page's whole address (`page_url`). Null says
/// nothing. Outside a page it does nothing (FEEDBACK84.md, Q-1320, Q-1321; FEEDBACK81.md, Q-1275;
/// branch feedback82-85; decided (user, 2026-11-22): a builtin, Nuxt's way).
fn page_head(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(Value::Map(m)) = args.first() else {
        return fail(line, "`page_head` takes a map: page_head({ title: \"Home\", description: \"...\" })");
    };
    let mut said: Vec<(usize, String)> = Vec::with_capacity(m.len());
    for (k, v) in m.iter() {
        let Some(i) = crate::head::KEYS.iter().position(|n| *n == &**k) else {
            return fail(line, format!("`page_head` has no key `{}`: it takes title, description, image and canonical", &**k));
        };
        let text = match v {
            Value::Nil => continue,
            Value::Str(s) | Value::Css(s) => s.to_string(),
            Value::Int(_) | Value::Float(_) => v.display().to_string(),
            other => return fail(line, format!("`page_head`: `{}` is text or null, got {}", &**k, other.repr())),
        };
        let text = if i >= 2 { whole_address(it, &text, "page_head", line)? } else { text };
        said.push((i, text));
    }
    let (depth, comps) = (it.head_depth, it.islands_open());
    let head = it.page_head.get_or_insert_with(Default::default);
    for (i, t) in said {
        head.say(i, depth, comps, t);
    }
    head.called();
    Ok(Value::Nil)
}

/// `page_url()`: the page's public address, `https://shop.example/shop/posts/3`: the scheme the
/// request came over (a `--trust-proxy` proxy's `X-Forwarded-Proto`, else `http`), its host, the
/// program's prefix (`--base`, a mount) and its path, without the query. `page_url("/img/card.png")`:
/// that path's (FEEDBACK84.md, Q-1328; branch feedback82-85).
fn page_url(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let path = match args.first() {
        None | Some(Value::Nil) => {
            let p = match it.request_value() {
                Some(Value::Map(m)) => m.get("path").map(|p| p.display().to_string()).unwrap_or_else(|| "/".into()),
                _ => "/".into(),
            };
            p
        }
        Some(_) => s_arg(&args, 0, "page_url", line)?.to_string(),
    };
    Ok(Value::str(whole_address(it, &path, "page_url", line)?))
}

/// `path` as a whole address of this program's, for `page_url` and `page_head`: one already whole
/// (`http://`, `https://`) as it is; a path from `/` with the scheme, host and prefix before it.
fn whole_address(it: &mut Interp, path: &str, f: &str, line: u32) -> Result<String, Ctrl> {
    let lower = path.get(..8).unwrap_or(path).to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Ok(path.to_string());
    }
    if !path.starts_with('/') || path.starts_with("//") {
        return fail(line, format!("`{}` takes a path that starts with / or a whole http(s) address, got {:?}", f, path));
    }
    let https = crate::session::came_over_https(it);
    // Under --watch, the first name --allowed-hosts gives, rather than what the visitor sent
    // (review of slice H).
    let named = crate::server::ALLOWED_HOSTS.get().and_then(|all| all.iter().find(|h| !h.starts_with('.')).cloned());
    let host = named.or_else(|| match it.request_value() {
        Some(Value::Map(m)) => m.get("host").map(|h| h.display().to_string()).filter(|h| !h.is_empty()),
        _ => None,
    })
    .unwrap_or_else(|| "localhost".into());
    Ok(format!("{}://{}{}{}", if https { "https" } else { "http" }, host, it.base, path))
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
        // Its headers, where it is a taken file's code that answers, as its `reply`'s are held
        // (`confirm::reply_headers_ok`; FEEDBACK83.md, Q-1301, found by the review of bc460fd; branch
        // feedback82-85). A group's own answer is its file's.
        Value::Response(r) if !r.headers.is_empty() && !crate::confirm::reply_headers_ok(&it.cur_file) => Err(Ctrl::Reply(Arc::new(Response { headers: Vec::new(), ..(*r).clone() }))),
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
    let (Value::Str(s) | Value::Css(s)) = v else { return fail(line, format!("`{}` takes a name (a str) or a list of them, got {}", f, v.type_name())) };
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
///
/// Also in a `style` attribute's template, whose values the parser gives here (FEEDBACK79.md,
/// Q-1250; branch feedback72-81): a text with nothing to escape given back as it is, the rest
/// written once. Null, a bool, a number and an error as they are, which nothing in them can make
/// a declaration of: `${x ?? "black"}`, `${big && "x: 1;"}` and `${a || b}` choose as before,
/// their values given here (found in the second review).
fn css_value_fn(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    css_value_of(arg(&args, 0), line)
}

/// What `__css_value` gives for `v`, which the interpreter works out without a call where the
/// parser put it (`Interp::eval`; FEEDBACK79.md, Q-1250, third review's speed check). An error
/// stays an error, so `??` still chooses, its contents escaped as a value's: shown, they are
/// written unquoted where they are markup or a map's keys, and `err(<b>{v}</b>)` added
/// declarations, and Q-668's `}` was back in css`...` in a `<style>` (third review).
pub fn css_value_of(v: Value, line: u32) -> R {
    match &v {
        // Trusted CSS passes as it is: a css`...` the program wrote, given through a name, a
        // prop, a list or a fn's answer (FEEDBACK79.md, Q-1250, the user's decision; branch
        // feedback72-81).
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Dec(_) | Value::Css(_) => return Ok(v),
        Value::Str(s) if !crate::interp::css_special(s) => return Ok(v),
        Value::Err(e, own, at) => {
            let t = match &**e {
                Value::Str(s) | Value::Css(s) => s.to_string(),
                other => other.display_held().map_err(|o| o.at(line))?,
            };
            let mut out = String::with_capacity(t.len() + 16);
            crate::interp::css_escape_into(&t, &mut out, false);
            return Ok(Value::Err(Arc::new(Value::str(&out)), *own, *at));
        }
        _ => {}
    }
    let t = v.display_held().map_err(|o| o.at(line))?;
    if !crate::interp::css_special(&t) {
        return Ok(Value::str(&t));
    }
    let mut out = String::with_capacity(t.len() + 16);
    crate::interp::css_escape_into(&t, &mut out, false);
    Ok(Value::str(&out))
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
