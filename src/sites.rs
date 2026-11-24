//! `site shop = "https://shop.example.com"`: a deployed Quillo site, whose server fns a program
//! calls as its own, `shop.notes()` (FEEDBACK33.md, Q-412; branch feedback27-35-fixes). A call
//! is the wire the site's own pages use, `POST <url>/_quillo/rpc/<fn>` with the arguments as a
//! JSON array, and its answer, `{"ok": value}` or `{"err": ...}`, is the value or an error
//! value. In an app of one person, a phone's or a desktop window's, the cookies the site sets
//! are kept for it, in `.quillo/sites/` in the program's folder, so whoever signed in stays
//! signed in, as in a browser. On a server they are not kept: one visitor's session with the
//! site would be every visitor's.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type R = Result<Value, Ctrl>;

/// Each site's cookies, by name: what it set and has not taken away.
static JARS: Mutex<Option<HashMap<String, Vec<(String, String)>>>> = Mutex::new(None);

/// The values `site` declarations made, kept so their maps are never freed and their
/// addresses never reused. A site is one of these maps itself, shared, never a copy: any map
/// with `$site` and `$url` keys was one, so visitor JSON (`request.json.keys()` on
/// `{"$site": "x", "$url": "http://10.0.0.5/#"}`) and a script's literal made the server POST
/// where they said, with the site's cookies (FEEDBACK51.md, Q-640; branch feedback51-fixes).
static DECLARED: Mutex<Vec<Arc<Map>>> = Mutex::new(Vec::new());

/// A value's site, if a `site` declaration made it.
fn declared(v: &Value) -> Option<Arc<Map>> {
    let Value::Map(m) = v else { return None };
    let ours = m.contains_key("$site") && DECLARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|d| Arc::ptr_eq(d, m));
    ours.then(|| m.clone())
}

/// Whether a value is a site a `site` declaration made.
pub fn is_site(v: &Value) -> bool {
    declared(v).is_some()
}

/// `__site(name, url)`: the value `site` declares.
pub fn declare(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::Str(name) | Value::Css(name)), Some(url)) = (args.first(), args.get(1)) else {
        return fail(line, "`site` takes a name and an address: site shop = \"https://shop.example.com\"");
    };
    let (Value::Str(url) | Value::Css(url)) = url else {
        return fail(line, format!("`site {}` needs an address, as text, got {}", name, url.type_name()));
    };
    let u = url.trim().trim_end_matches('/');
    let ok = (u.starts_with("https://") || u.starts_with("http://")) && u.split("://").nth(1).is_some_and(|rest| !rest.is_empty() && !rest.contains(['?', '#', ' ']));
    if !ok {
        return fail(line, format!("`site {}` needs an address like https://shop.example.com, got {:?}", name, &**url));
    }
    // A name for its cookies' file: letters, digits and `_`, as a declaration's is.
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return fail(line, format!("`site` takes a name of ASCII letters, digits and `_`, got {:?}", &**name));
    }
    let mut m = Map::new();
    m.insert("$site".into(), Value::Str(name.clone()));
    m.insert("$url".into(), Value::str(u));
    // Its host as the file that declares it was confirmed for: a site not confirmed is declared,
    // and each call of it refused (FEEDBACK83.md, Q-1300, found in review; branch feedback82-85).
    if let Err(why) = crate::confirm::http_ok(crate::confirm::held_id(it.cur_file.as_deref()), u) {
        m.insert("$refused".into(), Value::str(why));
    }
    // The same declaration run again (a reload, a script) is the one made before: the list
    // holds each site once.
    let mut all = DECLARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let same = |d: &Map, k: &str| matches!((d.get(k), m.get(k)), (Some(Value::Str(a) | Value::Css(a)), Some(Value::Str(b) | Value::Css(b))) if a == b);
    let m = match all.iter().find(|d| same(d, "$site") && same(d, "$url") && d.contains_key("$refused") == m.contains_key("$refused")) {
        Some(d) => d.clone(),
        None => {
            let m = Arc::new(m);
            all.push(m.clone());
            m
        }
    };
    drop(all);
    Ok(Value::Map(m))
}

/// Whether cookies are kept: in an app of one person, or where whoever runs the program says it
/// is one person's (`QUILLO_SITE_COOKIES=1`, for a script used as a tool).
fn keeps_cookies() -> bool {
    matches!(crate::platform::runtime(), "mobile" | "app") || std::env::var("QUILLO_SITE_COOKIES").is_ok_and(|v| v == "1" || v == "true")
}

fn jar_file(it: &Interp, name: &str) -> std::path::PathBuf {
    it.base_dir.join(".quillo").join("sites").join(format!("{}.json", name))
}

fn jar(it: &Interp, name: &str) -> Vec<(String, String)> {
    let mut g = JARS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let all = g.get_or_insert_with(HashMap::new);
    if let Some(j) = all.get(name) {
        return j.clone();
    }
    // Read as Quillo's own (`statics::open_below`): no link below `.quillo`, of one name only
    // (FEEDBACK85.md, Q-1340, found by the review of 9da2632; branch feedback82-85).
    let file = jar_file(it, name);
    let read: Vec<(String, String)> = crate::statics::quillo_split(&file).and_then(|(root, rel)| crate::statics::read_below(&root, &rel).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    all.insert(name.to_string(), read.clone());
    read
}

fn keep(it: &Interp, name: &str, set: &[String]) {
    let mut cookies = jar(it, name);
    for line in set {
        let mut parts = line.split(';');
        let Some((k, v)) = parts.next().and_then(|p| p.split_once('=')) else { continue };
        let (k, v) = (k.trim().to_string(), v.trim().to_string());
        let gone = v.is_empty() || parts.any(|a| {
            let a = a.trim().to_ascii_lowercase();
            a == "max-age=0" || a.starts_with("max-age=-")
        });
        cookies.retain(|(n, _)| *n != k);
        if !gone {
            cookies.push((k, v));
        }
    }
    let file = jar_file(it, name);
    if let Some(d) = file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&file, serde_json::to_vec(&cookies).unwrap_or_default());
    JARS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(name.to_string(), cookies);
}

/// A site's answer read as plain JSON (`Value::parse_json_held`) with an island's marks made into
/// what they stand for, as `interp::from_island_json` reads them from serde's tree: `{"$dec": t}` a
/// decimal, `{"$int": t}` an int, `{"$quillo_err": x}` an error value, `{"$": [k, x]}` the map of
/// that one key. Changed where it is: nothing grows (coverage of 0.8.7, Q-1109; branch
/// coverage-087).
fn island_marks(v: Value) -> Value {
    match v {
        Value::List(mut l) => {
            for x in Arc::make_mut(&mut l).iter_mut() {
                *x = island_marks(std::mem::replace(x, Value::Nil));
            }
            Value::List(l)
        }
        Value::Map(mut m) => {
            let mark = m.len() == 1 && m.get_index(0).is_some_and(|(k, x)| match (&**k, x) {
                ("$dec" | "$int" | "$css", Value::Str(_) | Value::Css(_)) | ("$quillo_err", _) => true,
                ("$", Value::List(a)) => a.len() == 2 && matches!(a[0], Value::Str(_) | Value::Css(_)),
                _ => false,
            });
            if mark {
                let (k, x) = Arc::make_mut(&mut m).pop().expect("one entry");
                return match (&*k, x) {
                    ("$dec", Value::Str(s) | Value::Css(s)) => crate::decimal::Dec::parse(&s).map_or(Value::Str(s), Value::Dec),
                    ("$int", Value::Str(s) | Value::Css(s)) => s.parse::<i64>().map_or(Value::Str(s), Value::Int),
                    // Another site's trusted CSS is text here, as all it sends (FEEDBACK79.md,
                    // Q-1250; branch feedback72-81).
                    ("$css", Value::Str(s) | Value::Css(s)) => Value::Str(s),
                    ("$", Value::List(a)) => {
                        let mut a = Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone());
                        let (x, k) = (a.pop().unwrap_or(Value::Nil), a.pop().unwrap_or(Value::Nil));
                        let mut one = Map::new();
                        one.insert(k.display().into(), island_marks(x));
                        Value::map(one)
                    }
                    (_, x) => Value::err(island_marks(x)),
                };
            }
            for x in Arc::make_mut(&mut m).values_mut() {
                *x = island_marks(std::mem::replace(x, Value::Nil));
            }
            Value::Map(m)
        }
        other => other,
    }
}

static SAID_SERVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `shop.notes(a, b)`, called as `__site_call(site, "notes", a, b)`.
pub fn call(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut args = args.into_iter();
    let (Some(site), Some(Value::Str(f) | Value::Css(f))) = (args.next().as_ref().and_then(declared), args.next()) else {
        return fail(line, "a site's function is called on a value a `site` declaration made");
    };
    let name = site.get("$site").map(|v| v.display()).unwrap_or_default();
    if let Some(why) = site.get("$refused") {
        return Ok(Value::err(Value::str(format!("site {}: not called: {}", name, why.display()))));
    }
    let url = site.get("$url").map(|v| v.display()).unwrap_or_default();
    let body = serde_json::Value::Array(args.map(|a| crate::interp::island_json(&a)).collect()).to_string();
    let keeps = keeps_cookies();
    let cookies = if keeps { jar(it, &name) } else { Vec::new() };
    let target = format!("{}/_quillo/rpc/{}", url, f);
    // Made as `http`'s calls are, in src/fetch.rs: the host's `[http]` policy holds for a site
    // too, a redirect is followed on the site's own origin only, and a `file:` one is an error
    // value, not a crash of the route (FEEDBACK71.md, Q-973; branch security). The policy is
    // asked here first, to say it refused, and again at each address `site_post` goes to.
    let answer = it.unlocked(move || {
        crate::policy::get().http(&target).map_err(|e| (true, Some(e)))?;
        let mut headers = vec![("accept".to_string(), "application/json".to_string())];
        if !cookies.is_empty() {
            let c: Vec<String> = cookies.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            headers.push(("cookie".into(), c.join("; ")));
        }
        crate::fetch::site_post(&target, &headers, body.into_bytes()).map_err(|e| (false, e))
    });
    let (status, headers, set, text) = match answer {
        Ok(a) => a,
        // An answer past `--max-memory`: the request's stop, which no `??` takes, as `http`'s is,
        // where it was an error value saying the stop's words (coverage of 0.8.7, Q-1105; branch
        // coverage-087).
        Err((_, None)) => return Err(crate::httpd::OverMemory.at(line)),
        Err((true, Some(e))) => return Ok(Value::err(Value::str(format!("site {}: not called: {}", name, e)))),
        Err((false, Some(e))) => return Ok(Value::err(Value::str(format!("site {}: no answer from {}: {}", name, url, e)))),
    };
    if !set.is_empty() {
        if keeps {
            keep(it, &name, &set);
        } else if !SAID_SERVER.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!("quillo: site {}: the cookies it set are not kept on a server, where one visitor's session with it would be every visitor's", name);
        }
    }
    // A redirect elsewhere, which is not followed: where it went, not what is there. The cookies
    // of that answer, and of those on the way, are the site's own and were kept above: every
    // answer a site's call reads is from the site's origin (FEEDBACK71.md, Q-973; branch
    // security).
    if let Some((_, to)) = headers.iter().find(|(k, _)| k == "location").filter(|_| matches!(status, 301 | 302 | 303 | 307 | 308)) {
        let to: String = to.chars().take(200).collect();
        return Ok(Value::err(Value::str(format!("site {}: `{}` answered {}, sent on to {}: a site's call follows redirects on the site's own address only", name, f, status, to))));
    }
    // As deep as every reader takes (FEEDBACK55.md, Q-875's second review; branch feedback55).
    // Read straight into values, each list, map and text held to `--max-memory` as it is made, as
    // `http`'s answers are, and past it the request's stop: a tree of serde's was made first, and
    // an answer of 698 KB, `{"ok":[0,0,...]}`, a list of 349,001 values (11 MB) under a 1 MB
    // limit (coverage of 0.8.7, Q-1109; branch coverage-087).
    let mut j = match Value::parse_json_held(&text).map_err(|o| o.at(line))? {
        Ok(Value::Map(m)) => Arc::try_unwrap(m).unwrap_or_else(|m| (*m).clone()),
        _ => Map::new(),
    };
    if let Some(v) = j.shift_remove("ok") {
        return Ok(island_marks(v));
    }
    if let Some(e) = j.shift_remove("err") {
        return Ok(Value::err(island_marks(e)));
    }
    let why = match j.get("error") {
        Some(Value::Str(e) | Value::Css(e)) => e.to_string(),
        _ => text.chars().take(200).collect(),
    };
    Ok(Value::err(Value::str(format!("site {}: `{}` answered {}: {}", name, f, status, why))))
}
