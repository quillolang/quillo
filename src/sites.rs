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
use std::sync::Mutex;

type R = Result<Value, Ctrl>;

/// Each site's cookies, by name: what it set and has not taken away.
static JARS: Mutex<Option<HashMap<String, Vec<(String, String)>>>> = Mutex::new(None);

/// `__site(name, url)`: the value `site` declares.
pub fn declare(_: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let (Some(Value::Str(name)), Some(url)) = (args.first(), args.get(1)) else {
        return fail(line, "`site` takes a name and an address: site shop = \"https://shop.example.com\"");
    };
    let Value::Str(url) = url else {
        return fail(line, format!("`site {}` needs an address, as text, got {}", name, url.type_name()));
    };
    let u = url.trim().trim_end_matches('/');
    let ok = (u.starts_with("https://") || u.starts_with("http://")) && u.split("://").nth(1).is_some_and(|rest| !rest.is_empty() && !rest.contains(['?', '#', ' ']));
    if !ok {
        return fail(line, format!("`site {}` needs an address like https://shop.example.com, got {:?}", name, &**url));
    }
    let mut m = Map::new();
    m.insert("$site".into(), Value::Str(name.clone()));
    m.insert("$url".into(), Value::str(u));
    Ok(Value::map(m))
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
    let read: Vec<(String, String)> = std::fs::read(jar_file(it, name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
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

static SAID_SERVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `shop.notes(a, b)`, called as `__site_call(site, "notes", a, b)`.
pub fn call(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let mut args = args.into_iter();
    let (Some(Value::Map(site)), Some(Value::Str(f))) = (args.next(), args.next()) else { return fail(line, "a site's function was called without its site") };
    let name = site.get("$site").map(|v| v.display()).unwrap_or_default();
    let url = site.get("$url").map(|v| v.display()).unwrap_or_default();
    let body = serde_json::Value::Array(args.map(|a| crate::interp::island_json(&a)).collect()).to_string();
    let keeps = keeps_cookies();
    let cookies = if keeps { jar(it, &name) } else { Vec::new() };
    let target = format!("{}/_quillo/rpc/{}", url, f);
    let answer = it.unlocked(move || {
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).build();
        let mut req = agent.post(&target).set("content-type", "application/json").set("accept", "application/json");
        if !cookies.is_empty() {
            let c: Vec<String> = cookies.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            req = req.set("cookie", &c.join("; "));
        }
        match req.send_string(&body) {
            Ok(r) | Err(ureq::Error::Status(_, r)) => {
                let status = r.status();
                let set: Vec<String> = r.all("set-cookie").into_iter().map(str::to_string).collect();
                let text = r.into_string().unwrap_or_default();
                Ok((status, set, text))
            }
            Err(e) => Err(e.to_string()),
        }
    });
    let (status, set, text) = match answer {
        Ok(a) => a,
        Err(e) => return Ok(Value::err(Value::str(format!("site {}: no answer from {}: {}", name, url, e)))),
    };
    if !set.is_empty() {
        if keeps {
            keep(it, &name, &set);
        } else if !SAID_SERVER.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!("quillo: site {}: the cookies it set are not kept on a server, where one visitor's session with it would be every visitor's", name);
        }
    }
    let j: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    if let Some(v) = j.get("ok") {
        return Ok(crate::interp::from_island_json(v));
    }
    if let Some(e) = j.get("err") {
        return Ok(Value::err(crate::interp::from_island_json(e)));
    }
    let why = j.get("error").and_then(|e| e.as_str()).map(str::to_string).unwrap_or_else(|| text.chars().take(200).collect());
    Ok(Value::err(Value::str(format!("site {}: `{}` answered {}: {}", name, f, status, why))))
}
