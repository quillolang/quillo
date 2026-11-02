//! Translations.
//!
//! ```text
//! translations("locales")              // locales/en.json, locales/fr.json, ...
//! t("nav.home")                        // "Accueil" for a French visitor
//! t("greeting", { name: user.name })   // "Hello, {name}!"
//! t("cart.items", { count: n })        // { "one": "{count} item", "other": "{count} items" }
//! locale()                             // "fr"
//! ```
//!
//! A request's locale is what `locale_from(fn)` gives for it, else its
//! `locale` cookie, else the best of its `Accept-Language`, else the default
//! (`translations(dir, { default: "en" })`, or `en` when there is one, or the
//! first). A key missing in a locale comes from the default's, and a key
//! missing there is shown as it is.
//!
//! Components call `t` too: a page with components carries its locale's
//! translations for the browser.

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

type R = Result<Value, Ctrl>;

pub struct I18n {
    pub default: Arc<str>,
    /// Each locale's entries by dotted key: a str, or a map of plural forms.
    pub locales: IndexMap<Arc<str>, HashMap<String, Value>>,
}

pub const BUILTINS: &[(&str, BuiltinFn)] = &[
    ("translations", translations),
    ("locale_from", locale_from),
    ("locale", locale),
    ("with_locale", with_locale),
    ("t", t),
];

const PLURALS: &[&str] = &["zero", "one", "two", "few", "many", "other"];

/// Nested maps become dotted keys; a map of plural forms is one entry.
fn flatten(prefix: &str, m: &Map, out: &mut HashMap<String, Value>) -> Result<(), String> {
    for (k, v) in m.iter() {
        let key = if prefix.is_empty() { k.to_string() } else { format!("{}.{}", prefix, k) };
        match v {
            Value::Str(_) => {
                out.insert(key, v.clone());
            }
            Value::Map(inner) if inner.contains_key("other") && inner.keys().all(|k| PLURALS.contains(&&**k)) => {
                if !inner.values().all(|v| matches!(v, Value::Str(_))) {
                    return Err(format!("the plural forms of `{}` must be text", key));
                }
                out.insert(key, v.clone());
            }
            Value::Map(inner) => flatten(&key, inner, out)?,
            other => return Err(format!("`{}` must be text or a map, not {}", key, other.type_name())),
        }
    }
    Ok(())
}

/// `en`, `pt-BR`, `zh-Hant`: a language and optional subtags.
fn locale_name_ok(name: &str) -> bool {
    let mut parts = name.split(['-', '_']);
    let lang = parts.next().unwrap_or("");
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn load_dir(dir: &std::path::Path) -> Result<Vec<(String, Map)>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {}", dir.display(), e))?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        let j: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: invalid JSON: {}", path.display(), e))?;
        match Value::from_json(&j) {
            Value::Map(m) => out.push((name, (*m).clone())),
            _ => return Err(format!("{} must hold a JSON object", path.display())),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// `translations("locales", { default })` or `translations({ en: {...}, fr:
/// {...} })`, at the top of the program.
fn translations(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let sources = match args.first() {
        Some(Value::Str(p)) => {
            let rel = std::path::Path::new(&**p);
            if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
                return fail(line, format!("`translations` needs a folder inside the program's, got {:?}", &**p));
            }
            match load_dir(&it.base_dir.join(rel)) {
                Ok(s) => s,
                Err(e) => return fail(line, format!("`translations`: {}", e)),
            }
        }
        Some(Value::Map(m)) => {
            let mut out = Vec::new();
            for (k, v) in m.iter() {
                match v {
                    Value::Map(inner) => out.push((k.to_string(), (**inner).clone())),
                    other => return fail(line, format!("`translations`: `{}` must be a map, not {}", k, other.type_name())),
                }
            }
            out
        }
        other => {
            return fail(line, format!("`translations` expects a folder or a map of locales, got {}", other.map_or("nothing", |v| v.type_name())))
        }
    };
    let mut locales = IndexMap::new();
    for (name, m) in sources {
        if !locale_name_ok(&name) {
            return fail(line, format!("`translations`: {:?} is not a locale name such as \"en\" or \"pt-BR\"", name));
        }
        let mut flat = HashMap::new();
        if let Err(e) = flatten("", &m, &mut flat) {
            return fail(line, format!("`translations` ({}): {}", name, e));
        }
        // `fr_FR.json`, as gettext and Java name them, is `fr-FR`, the form a request is
        // matched in, so it can be chosen (FEEDBACK30.md, Q-362; branch feedback27-35-fixes).
        let name = name.replace('_', "-");
        if locales.keys().any(|k: &Arc<str>| k.eq_ignore_ascii_case(&name)) {
            return fail(line, format!("`translations`: the locale {:?} is given twice", name));
        }
        warn_unused_forms(&name, &flat);
        locales.insert(Arc::<str>::from(name), flat);
    }
    if locales.is_empty() {
        return fail(line, "`translations` found no locales");
    }
    let default: Arc<str> = match args.get(1) {
        Some(Value::Map(o)) => match o.get("default") {
            Some(Value::Str(d)) => match locales.keys().find(|k| k.eq_ignore_ascii_case(d)) {
                Some(k) => k.clone(),
                None => return fail(line, format!("`translations`: the default {:?} is not among the locales", &**d)),
            },
            _ => first_default(&locales),
        },
        _ => first_default(&locales),
    };
    Arc::make_mut(&mut it.site).i18n = Some(Arc::new(I18n { default, locales }));
    Ok(Value::Nil)
}

fn first_default(locales: &IndexMap<Arc<str>, HashMap<String, Value>>) -> Arc<str> {
    locales.keys().find(|k| &***k == "en").or_else(|| locales.keys().next()).cloned().unwrap_or_else(|| "en".into())
}

/// `locale_from((request) => ...)`: the request's locale, or null for the
/// usual choice (cookie, `Accept-Language`, default).
fn locale_from(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    match args.first() {
        Some(f @ (Value::Func(_) | Value::Builtin(..))) => {
            Arc::make_mut(&mut it.site).locale_rule = Some(f.clone());
            Ok(Value::Nil)
        }
        other => fail(line, format!("`locale_from` expects a function of the request, got {}", other.map_or("nothing", |v| v.type_name()))),
    }
}

impl I18n {
    /// The locale here that best serves `tag`: the same, the same language
    /// without its region, or the same language with one.
    ///
    /// A script is matched before the language: `zh-Hant-TW` and `zh-TW` are served
    /// `zh-Hant`, and `sr-Latn-RS` is served `sr-Latn`, the script of a region being CLDR's
    /// likely one; and a region takes CLDR's parent where there is one, `pt-AO` taking `pt-PT`
    /// (FEEDBACK30.md, Q-363; branch feedback27-35-fixes).
    pub fn best(&self, tag: &str) -> Option<Arc<str>> {
        let tag = tag.trim().replace('_', "-");
        if tag.is_empty() {
            return None;
        }
        let find = |want: &str| self.locales.keys().find(|k| k.eq_ignore_ascii_case(want)).cloned();
        if let Some(k) = find(&tag) {
            return Some(k);
        }
        let parts: Vec<&str> = tag.split('-').collect();
        let lang = parts[0];
        let subtag = |k: &str, sub: &str| k.split('-').skip(1).any(|p| p.eq_ignore_ascii_case(sub));
        let same_lang = |k: &str| k.split('-').next().is_some_and(|l| l.eq_ignore_ascii_case(lang));
        let script = parts[1..].iter().find(|p| p.len() == 4 && p.bytes().all(|b| b.is_ascii_alphabetic())).map(|s| s.to_string());
        let region = parts[1..].iter().find(|p| (p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic())) || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit()))).map(|r| r.to_ascii_uppercase());
        let script = script.or_else(|| likely_script(lang, region.as_deref()).map(str::to_string));
        if let Some(sc) = &script {
            if let Some(r) = &region {
                if let Some(k) = find(&format!("{}-{}-{}", lang, sc, r)) {
                    return Some(k);
                }
            }
            if let Some(k) = find(&format!("{}-{}", lang, sc)) {
                return Some(k);
            }
            if let Some(k) = self.locales.keys().find(|k| same_lang(k) && subtag(k, sc)) {
                return Some(k.clone());
            }
        }
        if let Some(r) = &region {
            if let Some(k) = find(&format!("{}-{}", lang, r)) {
                return Some(k);
            }
            if let Some(k) = parent_region(lang, r).and_then(|p| find(&format!("{}-{}", lang, p))) {
                return Some(k);
            }
        }
        // The language alone, then one of its locales: not one in another script than the one
        // asked for, which would be text the reader may not read.
        let other_script = |k: &str| match &script {
            Some(sc) => k.split('-').skip(1).any(|p| p.len() == 4 && !p.eq_ignore_ascii_case(sc)),
            None => false,
        };
        find(lang).filter(|k| !other_script(k)).or_else(|| self.locales.keys().find(|k| same_lang(k) && !other_script(k)).cloned()).or_else(|| find(lang)).or_else(|| self.locales.keys().find(|k| same_lang(k)).cloned())
    }

    /// The best of an `Accept-Language` header, by its weights.
    fn accept(&self, header: &str) -> Option<Arc<str>> {
        let mut tags: Vec<(f32, usize, &str)> = header
            .split(',')
            .enumerate()
            .filter_map(|(i, part)| {
                let mut bits = part.split(';');
                let tag = bits.next()?.trim();
                let q = bits.find_map(|b| b.trim().strip_prefix("q=")).map_or(1.0, |q| q.trim().parse().unwrap_or(0.0));
                (q > 0.0 && tag != "*").then_some((q, i, tag))
            })
            .collect();
        tags.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        tags.iter().find_map(|(_, _, t)| self.best(t))
    }

    fn entry(&self, locale: &str, key: &str) -> Option<&Value> {
        self.locales.get(locale).and_then(|d| d.get(key)).or_else(|| self.locales.get(&*self.default)?.get(key))
    }

    /// The locale's translations with the default's filling the gaps, as
    /// JSON for the browser: only `wanted`'s keys when the page's code is known to ask
    /// for no others (`sec.*` is every key that starts `sec.`) (FEEDBACK30.md, Q-372;
    /// branch feedback27-35-fixes).
    pub fn browser_json(&self, locale: &str, wanted: Option<&[String]>) -> String {
        let takes = |k: &str| {
            k.starts_with("quillo.") || wanted.is_none_or(|w| w.iter().any(|x| match x.strip_suffix('*') {
                Some(start) => k.starts_with(start),
                None => x == k,
            }))
        };
        let mut all = serde_json::Map::new();
        for l in [&*self.default, locale] {
            if let Some(d) = self.locales.get(l) {
                let mut keys: Vec<_> = d.keys().filter(|k| takes(k)).collect();
                keys.sort();
                for k in keys {
                    all.insert(k.clone(), d[k].to_json());
                }
            }
        }
        let j = serde_json::json!({ "locale": locale, "t": all });
        // Safe inside <script>: no `</script>`, `<!--` or line separators.
        serde_json::to_string(&j)
            .unwrap_or_default()
            .replace('<', "\\u003c")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    }
}

/// The files of translations pages load, `/_quillo/t/<hash>.js`, by their hash: a page's
/// keys in its locale, which the browser keeps, as their address names their content.
static FILES: std::sync::Mutex<Option<HashMap<String, Arc<String>>>> = std::sync::Mutex::new(None);

/// Keeps `json` as a file and gives its hash.
pub fn keep_file(json: String) -> String {
    let js = format!("{}.i18n={};", crate::client::HANDOFF_JS, json);
    let hash = crate::client::hash_text(&js);
    let mut g = FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() > 1024 {
        m.clear();
    }
    m.entry(hash.clone()).or_insert_with(|| Arc::new(js));
    hash
}

pub fn file(hash: &str) -> Option<Arc<String>> {
    FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref()?.get(hash).cloned()
}

fn header(it: &mut Interp, name: &str) -> Option<String> {
    let Some(Value::Map(req)) = it.request_value() else { return None };
    let Some(Value::Map(h)) = req.get("headers") else { return None };
    h.get(name).map(|v| v.display())
}

fn cookie(it: &mut Interp, name: &str) -> Option<String> {
    let Some(Value::Map(req)) = it.request_value() else { return None };
    let Some(Value::Map(c)) = req.get("cookies") else { return None };
    c.get(name).map(|v| v.display())
}

impl Interp {
    /// The locale the code runs in (see the module's notes); none without
    /// `translations`.
    pub fn locale(&mut self) -> Option<Arc<str>> {
        if let Some(l) = &self.locale {
            return Some(l.clone());
        }
        let i18n = self.site.i18n.clone()?;
        let in_request = self.request_value().is_some();
        let mut found = None;
        if in_request {
            if let Some(rule) = self.site.locale_rule.clone() {
                if let Some(Value::Map(req)) = self.request_value() {
                    if let Ok(Value::Str(s)) = self.call(&rule, vec![Value::Map(req)], 0) {
                        found = i18n.best(&s);
                    }
                }
            }
            if found.is_none() {
                found = cookie(self, "locale").and_then(|c| i18n.best(&c));
            }
            if found.is_none() {
                found = header(self, "accept-language").and_then(|h| i18n.accept(&h));
            }
        }
        let l = found.unwrap_or_else(|| i18n.default.clone());
        if in_request {
            self.locale = Some(l.clone());
        }
        Some(l)
    }
}

/// `locale()`: the locale of the request (or of `with_locale`), or null
/// without `translations`.
fn locale(it: &mut Interp, _: Vec<Value>, _: u32) -> R {
    it.note_locale();
    Ok(it.locale().map_or(Value::Nil, Value::Str))
}

/// `with_locale("fr", () => ...)`: runs the function in that locale (for an
/// email to a user in their language, say).
fn with_locale(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let Some(i18n) = it.site.i18n.clone() else { return fail(line, "`with_locale` needs `translations` first") };
    let name = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`with_locale` expects a locale name, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let Some(f) = args.get(1).cloned() else { return fail(line, "`with_locale` needs a function to run") };
    let l = i18n.best(&name).unwrap_or_else(|| i18n.default.clone());
    let saved = it.locale.replace(l);
    let r = it.call(&f, Vec::new(), line);
    it.locale = saved;
    r
}

/// `t(key, vars)`: the key's text in the current locale, with `{name}`
/// replaced by `vars.name`; plural forms are chosen by `vars.count`.
fn t(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    translate(it, args, line, false)
}

/// `raw(t(key, vars))`, for markup written in a translation: the text as
/// HTML, but each value put in it escaped, since those come from data
/// (a user's name), not from whoever wrote the translations.
pub fn t_html(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    translate(it, args, line, true)
}

fn translate(it: &mut Interp, args: Vec<Value>, line: u32, html: bool) -> R {
    let key = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        other => return fail(line, format!("`t` expects a key, got {}", other.map_or("nothing", |v| v.type_name()))),
    };
    let vars = match args.get(1) {
        None | Some(Value::Nil) => None,
        Some(Value::Map(m)) => Some(m.clone()),
        Some(v) => return fail(line, format!("`t` expects its values as a map, got {}", v.type_name())),
    };
    it.note_locale();
    let Some(l) = it.locale() else { return fail(line, "`t` needs `translations` first") };
    let i18n = it.site.i18n.clone().expect("a locale means translations");
    let text = match i18n.entry(&l, &key) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Map(forms)) => {
            // A count read from a form or a query is text, and an amount is a decimal: each is
            // taken at its number, a decimal's written digits counting as CLDR counts them
            // (FEEDBACK30.md, Q-367; branch feedback27-35-fixes).
            let count = vars.as_ref().and_then(|v| v.get("count"));
            let n = count.and_then(count_of);
            let cat = match n {
                // `zero`, where one is written, for 0 in any language, as Rails and i18next read it
                // (FEEDBACK30.md, Q-366).
                Some((n, false)) if n == 0.0 && forms.contains_key("zero") => "zero",
                Some((n, frac)) => plural_of(&l, n, frac),
                None => "other",
            };
            forms.get(cat).or_else(|| forms.get("other")).map(|v| v.display().to_string()).unwrap_or_default()
        }
        _ => key.to_string(),
    };
    if html {
        let escaped = vars.map(|m| m.iter().map(|(k, v)| (k.clone(), Value::str(crate::interp::escape(&v.display())))).collect::<Map>());
        return Ok(Value::html(interpolate(&text, escaped.as_ref())));
    }
    Ok(Value::str(interpolate(&text, vars.as_deref())))
}

/// The script a language is written in, in a region, where it has more than one (CLDR's
/// likely subtags).
fn likely_script(lang: &str, region: Option<&str>) -> Option<&'static str> {
    match (lang.to_ascii_lowercase().as_str(), region) {
        ("zh", Some("TW" | "HK" | "MO")) => Some("Hant"),
        ("zh", _) => Some("Hans"),
        ("sr", Some("ME")) => Some("Latn"),
        ("sr", _) => Some("Cyrl"),
        ("uz", Some("AF")) => Some("Arab"),
        ("uz", _) => Some("Latn"),
        ("pa", Some("PK")) => Some("Arab"),
        ("bs", _) => Some("Latn"),
        _ => None,
    }
}

/// The region whose text a region takes when it has none of its own (CLDR's parent locales).
fn parent_region(lang: &str, region: &str) -> Option<&'static str> {
    const LATAM: &[&str] = &["AR", "BO", "BR", "BZ", "CL", "CO", "CR", "CU", "DO", "EC", "GT", "HN", "MX", "NI", "PA", "PE", "PR", "PY", "SV", "US", "UY", "VE"];
    match lang.to_ascii_lowercase().as_str() {
        "pt" if region != "BR" => Some("PT"),
        "es" if LATAM.contains(&region) => Some("419"),
        "en" if !matches!(region, "US" | "PR" | "AS" | "GU" | "MP" | "UM" | "VI") => Some("GB"),
        _ => None,
    }
}

/// Plural forms a locale's rules never pick are said at load: a translator's English `few`
/// would never be read. `zero` is read for 0 in any language (FEEDBACK30.md, Q-366; branch
/// feedback27-35-fixes).
fn warn_unused_forms(locale: &str, flat: &HashMap<String, Value>) {
    let used = categories(locale);
    let mut keys: Vec<&String> = flat.keys().collect();
    keys.sort();
    for k in keys {
        let Value::Map(forms) = &flat[k] else { continue };
        for f in forms.keys() {
            if PLURALS.contains(&&**f) && *f != *"zero" && !used.contains(&&**f) {
                eprintln!("quillo: warning: translations ({}): `{}` has a `{}` form, which {} never picks (it has {})", locale, k, f, locale, used.join(", "));
            }
        }
    }
}

/// The plural forms a locale's rules can pick.
pub fn categories(locale: &str) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    let samples = (0..=1000).map(|n| (n as f64, false)).chain([(1e6, false), (2e6, false), (1.5, true), (0.5, true), (21.5, true)]);
    for (n, frac) in samples {
        let c = plural_of(locale, n, frac);
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen.sort_by_key(|c| PLURALS.iter().position(|p| p == c));
    seen
}

/// `{name}` replaced by the value's text; other braces are left alone.
pub fn interpolate(text: &str, vars: Option<&Map>) -> String {
    let Some(vars) = vars else { return text.to_string() };
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        match (after[end..].starts_with('}'), vars.get(&after[..end])) {
            (true, Some(v)) if end > 0 => {
                out.push_str(&v.display());
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A `count`'s number, and whether it is written with a fraction's digits (`1.0m`, `"1.5"`).
fn count_of(v: &Value) -> Option<(f64, bool)> {
    match v {
        Value::Int(i) => Some((*i as f64, false)),
        Value::Float(f) if f.is_finite() => Some((*f, f.fract() != 0.0)),
        Value::Dec(d) => Some((d.to_f64(), d.scale() > 0)),
        Value::Str(s) => {
            let t = s.trim();
            let f: f64 = t.parse().ok().filter(|f: &f64| f.is_finite())?;
            Some((f, t.contains('.')))
        }
        _ => None,
    }
}

/// The plural form of `n` in a language (CLDR's rules for whole numbers;
/// fractions take `other`). The browser runtime has the same table.
pub fn plural(locale: &str, n: f64) -> &'static str {
    plural_of(locale, n, n.fract() != 0.0)
}

/// The plural form of `n`, `frac` when it is written with a fraction's digits. CLDR's rules,
/// for the languages below; any other is read as English. The browser runtime has the same
/// table (FEEDBACK30.md, Q-366; branch feedback27-35-fixes).
pub fn plural_of(locale: &str, n: f64, frac: bool) -> &'static str {
    let lang = locale.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    let region = locale.split(['-', '_']).nth(1).unwrap_or("").to_ascii_uppercase();
    if frac || n.fract() != 0.0 || !n.is_finite() {
        return match lang.as_str() {
            "lt" => "many",
            "ro" => "few",
            "cs" | "sk" => "many",
            _ => "other",
        };
    }
    let n = n.abs() as u64;
    let (m10, m100) = (n % 10, n % 100);
    let millions = n != 0 && n % 1_000_000 == 0;
    match lang.as_str() {
        "ja" | "zh" | "ko" | "th" | "vi" | "id" | "ms" | "lo" | "my" | "km" | "yue" => "other",
        "pt" if region == "PT" => match n {
            1 => "one",
            _ if millions => "many",
            _ => "other",
        },
        "fr" | "pt" if n <= 1 => "one",
        "fr" | "pt" if millions => "many",
        "es" | "it" | "ca" if n == 1 => "one",
        "es" | "it" | "ca" if millions => "many",
        "es" | "it" | "ca" => "other",
        "fr" | "pt" | "hi" | "bn" | "fa" | "gu" | "kn" | "zu" | "am" if n <= 1 => "one",
        "fr" | "pt" | "hi" | "bn" | "fa" | "gu" | "kn" | "zu" | "am" => "other",
        "ro" | "mo" => match n {
            1 => "one",
            0 => "few",
            _ if (1..=19).contains(&m100) => "few",
            _ => "other",
        },
        "lt" => {
            if m10 == 1 && !(11..=19).contains(&m100) {
                "one"
            } else if (2..=9).contains(&m10) && !(11..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "lv" => {
            if m10 == 0 || (11..=19).contains(&m100) {
                "zero"
            } else if m10 == 1 && m100 != 11 {
                "one"
            } else {
                "other"
            }
        }
        "sl" => match m100 {
            1 => "one",
            2 => "two",
            3 | 4 => "few",
            _ => "other",
        },
        "is" | "mk" if m10 == 1 && m100 != 11 => "one",
        "is" | "mk" => "other",
        "ga" => match n {
            1 => "one",
            2 => "two",
            3..=6 => "few",
            7..=10 => "many",
            _ => "other",
        },
        "cy" => match n {
            0 => "zero",
            1 => "one",
            2 => "two",
            3 => "few",
            6 => "many",
            _ => "other",
        },
        "mt" => match (n, m100) {
            (1, _) => "one",
            (2, _) => "two",
            (0, _) | (_, 3..=10) => "few",
            (_, 11..=19) => "many",
            _ => "other",
        },
        "ru" | "uk" | "be" => {
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        "sr" | "hr" | "bs" => {
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "pl" => {
            if n == 1 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        "cs" | "sk" => match n {
            1 => "one",
            2..=4 => "few",
            _ => "other",
        },
        "ar" => match (n, m100) {
            (0, _) => "zero",
            (1, _) => "one",
            (2, _) => "two",
            (_, 3..=10) => "few",
            (_, 11..=99) => "many",
            _ => "other",
        },
        "he" => match n {
            1 => "one",
            2 => "two",
            _ => "other",
        },
        _ if n == 1 => "one",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals_and_interpolation() {
        let forms = |l: &str| [0.0, 1.0, 2.0, 5.0, 11.0, 21.0, 22.0, 1.5].map(|n| plural(l, n));
        assert_eq!(forms("en"), ["other", "one", "other", "other", "other", "other", "other", "other"]);
        assert_eq!(forms("fr-CA"), ["one", "one", "other", "other", "other", "other", "other", "other"]);
        assert_eq!(forms("ru"), ["many", "one", "few", "many", "many", "one", "few", "other"]);
        assert_eq!(forms("ja"), ["other"; 8]);
        let mut v = Map::new();
        v.insert("name".into(), Value::str("Ada"));
        v.insert("n".into(), Value::Int(3));
        assert_eq!(interpolate("Hi {name}, {n} new {x} {} {", Some(&v)), "Hi Ada, 3 new {x} {} {");
    }
}
