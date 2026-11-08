//! What a program's API is, for others to read (FEEDBACK10.md, Q-077;
//! branch feedback10-fixes).
//!
//! An OpenAPI 3 document of its routes, from their paths and their query
//! and payload patterns (`route POST /signup { form: { name: str, age: int } }`),
//! and, as `x-quillo-server-fns`, each `server fn`'s contract: its
//! parameters, their types, its result and its error. Served at
//! `/_quillo/describe` under `quillo run --describe`, and written by
//! `quillo docs app.qwe` from the source alone. The routes of services
//! mounted in this process are in it under their prefixes, so the front
//! door describes what is behind it. Each of the program's types an operation names is
//! written once under `components/schemas`, and a server fn's `///` comment and `with` are
//! said (FEEDBACK56.md, Q-854 to Q-857; branch feedback56).

use crate::ast::*;
use crate::error::QuilloError;
use crate::interp::Interp;
use crate::record::Types;
use crate::value::{scope_vars, Value};
use serde_json::{json, Map, Value as J};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Set by `quillo run --describe`.
pub static ENABLED: AtomicBool = AtomicBool::new(false);

/// The running program's document.
pub fn of_interp(it: &Interp, title: &str) -> J {
    let mut parts = Parts::default();
    collect_interp(it, "", &mut parts);
    let mut doc = parts.document(title);
    // Under `--base /shop` every address the program makes carries the prefix: a gateway that
    // imports the document is told so (FEEDBACK32.md, Q-398; branch feedback27-35-fixes).
    if !it.base.is_empty() {
        doc["servers"] = json!([{ "url": it.base }]);
    }
    doc
}

/// The document of the program in `path`, read without running it.
pub fn of_source(path: &Path) -> Result<J, QuilloError> {
    let mut parts = Parts::default();
    let mut seen = Vec::new();
    // Its `use`s from the folder of the file a link leads to (FEEDBACK55.md, Q-817; branch feedback55).
    let base = crate::modules::source_dir_or(path, path.parent().unwrap_or(Path::new(".")));
    collect_file(path, &base, "", &mut parts, &mut seen, 0)?;
    let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(parts.document(&title))
}

/// A `server fn`'s contract as one line of plain text, the same wherever
/// its file is read: `assess(n: int, size?: int) -> map ! str`. Calls to a
/// service elsewhere carry it, and the service says when its own differs.
pub fn contract(d: &FnDef) -> String {
    let ps: Vec<String> = d
        .params
        .iter()
        .map(|p| format!("{}{}: {}", p.name, if p.default.is_some() { "?" } else { "" }, p.ty.as_ref().map_or("any".to_string(), |t| t.text())))
        .collect();
    let mut s = format!("{}({})", d.name, ps.join(", "));
    if let Some(r) = &d.ret {
        s.push_str(&format!(" -> {}", r.text()));
    }
    if let Some(e) = &d.err {
        s.push_str(&format!(" ! {}", e.text()));
    }
    s
}

#[derive(Default)]
struct Parts {
    routes: Vec<(String, Arc<RouteDef>)>,
    fns: Vec<(String, Arc<FnDef>)>,
    /// The callers each server fn takes (`for [...]`), for a gateway to enforce too
    /// (FEEDBACK38.md, Q-458; branch feedback36-39).
    callers: std::collections::HashMap<String, Vec<String>>,
    /// The program's named types under each prefix: a mount's and a service's are their own
    /// (FEEDBACK56.md, Q-855; branch feedback56).
    types: std::collections::HashMap<String, Types>,
}

impl Parts {
    /// A service's routes, and the types they name (FEEDBACK56.md, Q-855; branch feedback56).
    fn take_routes(&mut self, inner: Parts) {
        self.routes.extend(inner.routes);
        for (prefix, types) in inner.types {
            self.types.entry(prefix).or_default().extend(types);
        }
    }
}

fn collect_interp(it: &Interp, prefix: &str, parts: &mut Parts) {
    // Not a guarded group's hidden route (FEEDBACK47.md, Q-539).
    for r in it.routes.iter().filter(|r| r.method != crate::ast::GUARD_METHOD) {
        parts.routes.push((prefix.to_string(), r.clone()));
    }
    for (f, who) in it.fn_callers.iter() {
        parts.callers.insert(format!("{}{}", prefix, f), who.clone());
    }
    parts.types.entry(prefix.to_string()).or_default().extend(it.types.iter().map(|(n, td)| (n.clone(), td.clone())));
    let mut fns: Vec<Arc<FnDef>> = scope_vars(&it.globals)
        .into_iter()
        .filter_map(|(n, v)| match v {
            Value::Func(f) if f.def.server && it.server_fns.iter().any(|s| *s == n) => Some(f.def.clone()),
            _ => None,
        })
        .collect();
    fns.sort_by(|a, b| a.name.cmp(&b.name));
    parts.fns.extend(fns.into_iter().map(|d| (prefix.to_string(), d)));
    for (p, sub) in &it.mounts {
        // A service under a prefix serves its routes here, but its server fns are called by
        // other services, signed, not at `/billing/_quillo/rpc`: only a mount's are described,
        // as `quillo docs` does (FEEDBACK32.md, Q-395; branch feedback27-35-fixes).
        if sub.is_service {
            let mut inner = Parts::default();
            collect_interp(sub, &format!("{}{}", prefix, p), &mut inner);
            parts.take_routes(inner);
        } else {
            collect_interp(sub, &format!("{}{}", prefix, p), parts);
        }
    }
    // A service under a prefix that runs as a process of its own (`--split`): its routes from
    // its source, as `quillo docs` reads them, so the document does not depend on how the app
    // was started (FEEDBACK32.md, Q-397; branch feedback27-35-fixes).
    for (p, file) in &it.remote_sources {
        let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut inner = Parts::default();
        let mut seen = Vec::new();
        if collect_file(file, &dir, &format!("{}{}", prefix, p), &mut inner, &mut seen, 1).is_ok() {
            parts.take_routes(inner);
        }
    }
}

/// The files read, each under the prefix of the program that read it: a file the program and a
/// mount both `use` is read for each, its types the mount's too, as the running server loads it in
/// each (FEEDBACK56.md, Q-855, found in review; branch feedback56).
type Seen = Vec<(String, std::path::PathBuf)>;

/// Whether `file` is being read by the program under `prefix` or one it is mounted in: mounting
/// it again would never end.
fn read_above(seen: &Seen, prefix: &str, file: &Path) -> bool {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    seen.iter().any(|(q, f)| *f == file && (q.is_empty() || q == prefix || prefix.starts_with(&format!("{}/", q))))
}

fn collect_file(path: &Path, base: &Path, prefix: &str, parts: &mut Parts, seen: &mut Seen, depth: u32) -> Result<(), QuilloError> {
    let key = (prefix.to_string(), path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    if seen.contains(&key) || depth > 64 {
        return Ok(());
    }
    seen.push(key);
    let src = crate::error::read_source(path).map_err(|e| QuilloError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
    let file: Arc<str> = Arc::from(path.display().to_string());
    let stmts = crate::parser::Parser::new(&src).in_file(Some(file)).parse_program().map_err(|e| e.in_file(&path.display().to_string()))?;
    for s in stmts {
        match s {
            Stmt::Route(r) if r.method != crate::ast::GUARD_METHOD => parts.routes.push((prefix.to_string(), r)),
            Stmt::Fn(d) if d.server => parts.fns.push((prefix.to_string(), d)),
            // For `components/schemas` (FEEDBACK56.md, Q-855; branch feedback56).
            Stmt::Type(td) => {
                parts.types.entry(prefix.to_string()).or_default().insert(td.name.clone(), td);
            }
            Stmt::Expr(crate::ast::Expr::Call(f, args, _)) if matches!(&*f, crate::ast::Expr::Ident(n, _) if n == "__callers") => {
                if let [crate::ast::Expr::Lit(crate::ast::Lit::Str(name)), crate::ast::Expr::List(l)] = args.as_slice() {
                    let who = l.iter().filter_map(|e| if let crate::ast::Expr::Lit(crate::ast::Lit::Str(s)) = e { Some(s.to_string()) } else { None }).collect();
                    parts.callers.insert(format!("{}{}", prefix, name), who);
                }
            }
            Stmt::Use(p, _, _, o) if !crate::ast::is_script(&p) => collect_file(&base.join(crate::modules::resolve_use(base, path, &p, o.implied)), base, prefix, parts, seen, depth + 1)?,
            // A service with a prefix: its routes are served here.
            Stmt::Service(_, Some(p), file, _, _, _) if !read_above(seen, prefix, &base.join(&file)) => {
                let full = base.join(&file);
                let dir = full.parent().unwrap_or(base).to_path_buf();
                let mut inner = Parts::default();
                collect_file(&full, &dir, &format!("{}{}", prefix, p), &mut inner, seen, depth + 1)?;
                parts.take_routes(inner);
            }
            Stmt::Mount(p, file, _) if !read_above(seen, prefix, &base.join(&file)) => {
                let full = base.join(&file);
                let dir = full.parent().unwrap_or(base).to_path_buf();
                collect_file(&full, &dir, &format!("{}{}", prefix, p), parts, seen, depth + 1)?;
            }
            _ => {}
        }
    }
    Ok(())
}

impl Parts {
    fn document(&self, title: &str) -> J {
        // The routes of one method on one shape of path are one operation: several bodies a
        // `oneOf`, several queries one list, a parameter required where every route requires it,
        // and the path's template named as the first route names it, as OpenAPI takes one
        // template per shape (FEEDBACK32.md, Q-396; branch feedback27-35-fixes).
        let mut groups: Vec<(String, String, Vec<&(String, Arc<RouteDef>)>)> = Vec::new();
        for pr in &self.routes {
            let (prefix, r) = pr;
            // A parameter after a prefix (`/@{name}`) is one shape whatever it is named too
            // (FEEDBACK53.md, Q-683; found in review).
            let shape = openapi_path(prefix, &r.path)
                .0
                .split('/')
                .map(|s| match s.find('{') {
                    Some(i) => format!("{}{{}}", &s[..i]),
                    None => s.to_string(),
                })
                .collect::<Vec<_>>()
                .join("/");
            let method = r.method.to_ascii_lowercase();
            match groups.iter_mut().find(|(s, m, _)| *s == shape && *m == method) {
                Some(g) => g.2.push(pr),
                None => groups.push((shape, method, vec![pr])),
            }
        }
        let mut paths = Map::new();
        let mut schemas = Schemas::new(&self.types, "#/components/schemas/");
        // One template for a shape, whatever the method: `GET /u/:a` and `POST /u/:b` were the
        // two paths `/u/{a}` and `/u/{b}`, which OpenAPI does not allow; the later takes the
        // first's names, in order (found in review of FEEDBACK53.md, Q-683).
        let mut named: std::collections::HashMap<String, (String, Vec<J>)> = std::collections::HashMap::new();
        for (shape, method, routes) in &groups {
            let (prefix, first) = routes[0];
            let (path, mut params) = openapi_path(prefix, &first.path);
            let path = match named.get(shape) {
                Some((p, names)) => {
                    for (param, name) in params.iter_mut().zip(names) {
                        param["name"] = name.clone();
                    }
                    p.clone()
                }
                None => {
                    named.insert(shape.clone(), (path.clone(), params.iter().map(|p| p["name"].clone()).collect()));
                    path
                }
            };
            // A path parameter typed otherwise by another route: either type.
            for (_, r) in routes.iter().skip(1) {
                let (_, other) = openapi_path(prefix, &r.path);
                for (p, o) in params.iter_mut().zip(other.iter()).filter(|(p, o)| o["schema"] != p["schema"]) {
                    let mut alts = match &p["schema"]["oneOf"] {
                        J::Array(a) => a.clone(),
                        _ => vec![p["schema"].clone()],
                    };
                    if !alts.contains(&o["schema"]) {
                        alts.push(o["schema"].clone());
                    }
                    p["schema"] = json!({ "oneOf": alts });
                }
            }
            let mut query: Vec<(String, J, usize, usize)> = Vec::new();
            for (prefix, r) in routes.iter() {
                schemas.prefix = prefix.clone();
                if let Some(Pattern::Record(fields)) = &r.query {
                    for f in fields {
                        let (schema, required) = schemas.field(f);
                        let name = f.path.join(".");
                        match query.iter_mut().find(|(n, _, _, _)| *n == name) {
                            Some(q) => {
                                q.2 += 1;
                                q.3 += required as usize;
                            }
                            None => query.push((name, schema, 1, required as usize)),
                        }
                    }
                }
            }
            for (name, schema, seen, required) in query {
                params.push(json!({ "name": name, "in": "query", "required": seen == routes.len() && required == routes.len(), "schema": schema }));
            }
            let mut op = Map::new();
            op.insert("responses".into(), json!({ "200": { "description": "the route's answer" } }));
            if !params.is_empty() {
                op.insert("parameters".into(), J::Array(params));
            }
            let bodies: Vec<J> = routes
                .iter()
                .filter_map(|(prefix, r)| {
                    schemas.prefix = prefix.clone();
                    schemas.request_body(r.body_pat.as_ref())
                })
                .collect();
            match bodies.len() {
                0 => {}
                1 => {
                    op.insert("requestBody".into(), bodies.into_iter().next().unwrap_or_default());
                }
                _ => {
                    // The bodies of each kind, `anyOf` where one body could match two routes: the
                    // first route takes it (FEEDBACK56.md, Q-855, found in review; branch feedback56).
                    let mut kinds: Vec<(String, Vec<J>)> = Vec::new();
                    for (kind, v) in bodies.iter().filter_map(|b| b["content"].as_object()).flatten() {
                        match kinds.iter_mut().find(|(k, _)| k == kind) {
                            Some((_, alts)) => alts.push(v["schema"].clone()),
                            None => kinds.push((kind.clone(), vec![v["schema"].clone()])),
                        }
                    }
                    let content: Map<String, J> = kinds.into_iter().map(|(k, alts)| (k, json!({ "schema": schemas.either(alts) }))).collect();
                    op.insert("requestBody".into(), json!({ "required": true, "content": content }));
                }
            }
            let at = |r: &RouteDef| {
                let file = r.file.as_deref().map(|f| Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                format!("{}:{}", file.unwrap_or_default(), r.line)
            };
            if routes.len() == 1 {
                op.insert("x-quillo-at".into(), json!(at(first)));
            } else {
                op.insert("x-quillo-at".into(), J::Array(routes.iter().map(|(_, r)| json!(at(r))).collect()));
            }
            let entry = paths.entry(path).or_insert_with(|| J::Object(Map::new()));
            if let J::Object(m) = entry {
                m.insert(method.clone(), J::Object(op));
            }
        }
        let mut fns = Map::new();
        for (prefix, d) in &self.fns {
            schemas.prefix = prefix.clone();
            let params: Vec<J> = d
                .params
                .iter()
                .map(|p| json!({ "name": p.name.as_str(), "schema": p.ty.as_ref().map_or(json!({}), |t| schemas.ty(t)), "required": p.default.is_none() }))
                .collect();
            // What it may do, as its `with` says: at the end of its contract, and as a list. The
            // contract services compare (`contract`) stays without it, so that a caller's copy
            // made before is not refused (FEEDBACK56.md, Q-857; branch feedback56).
            let effects = d.effects.as_deref().map(|e| effect_names(effect_mask(e)));
            let mut f = Map::new();
            f.insert("contract".into(), json!(match &effects {
                Some(e) => format!("{} with {}", contract(d), e.join(", ")),
                None => contract(d),
            }));
            // Its `///` comment (FEEDBACK56.md, Q-854; branch feedback56).
            if let Some(doc) = &d.doc {
                f.insert("description".into(), json!(doc));
            }
            f.insert("params".into(), J::Array(params));
            // A result and an error are held to their types' names, not to a map type's fields
            // (`record::fits`, not deep): none is said to be required (FEEDBACK56.md, Q-856, found
            // in review; branch feedback56).
            schemas.held = false;
            if let Some(r) = &d.ret {
                f.insert("returns".into(), schemas.ty(r));
            }
            if let Some(e) = &d.err {
                f.insert("error".into(), schemas.ty(e));
            }
            schemas.held = true;
            f.insert("call".into(), json!(format!("POST {}/_quillo/rpc/{}", prefix, d.name)));
            if let Some(who) = self.callers.get(&format!("{}{}", prefix, d.name)) {
                f.insert("x-quillo-callers".into(), json!(who));
            }
            if let Some(e) = effects {
                f.insert("x-quillo-effects".into(), json!(e));
            }
            // `server fn mcp`: a tool too; a plain `fn mcp`, not a page's to call, is not here
            // (FEEDBACK56.md, Q-853; branch feedback56).
            if d.mcp {
                f.insert("x-quillo-mcp".into(), json!(true));
            }
            fns.insert(format!("{}{}", if prefix.is_empty() { String::new() } else { format!("{} ", prefix) }, d.name), J::Object(f));
        }
        let mut doc = json!({
            "openapi": "3.0.3",
            "info": { "title": title, "version": crate::version() },
            "paths": paths,
            "x-quillo-server-fns": fns,
        });
        let written = schemas.written();
        if !written.is_empty() {
            doc["components"] = json!({ "schemas": written });
        }
        doc
    }
}

/// The schemas of a document: the program's named types under each prefix, and the ones its
/// operations name, each written once and referred to with a local `$ref`, a record with its
/// fields, the ones required, their `enum`s and defaults (FEEDBACK56.md, Q-855 and Q-856; branch
/// feedback56). A record that names itself, or one that names it, is a `$ref` there too, so the
/// walk ends. The walk of a `Ty` and the lookup of a name are the same for JSON Schema 2020-12,
/// whose `$ref`s point elsewhere (`refs`), and an MCP tool's schemas are written in it
/// (`for_tools`; FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
pub(crate) struct Schemas<'a> {
    types: &'a std::collections::HashMap<String, Types>,
    /// The prefix whose types a name is looked up in: the program's own (""), a mount's or a
    /// service's.
    prefix: String,
    /// Where a `$ref` points: `#/components/schemas/`.
    refs: &'static str,
    /// The key of each of the program's types, by prefix and name.
    keys: std::collections::HashMap<(String, String), String>,
    /// Each named type referred to so far: its prefix, its name, its key and whether it is a
    /// record.
    named: Vec<(String, String, String, bool)>,
    /// Whether the fields of a map type written here are checked, so that one is required: in a
    /// parameter and a record, but not in a result or an error, which are held to the names of
    /// their types only, nor in an alias, which a result may name (FEEDBACK56.md, Q-856, found
    /// in review; branch feedback56).
    pub(crate) held: bool,
    /// JSON Schema 2020-12, as MCP takes a tool's schemas, where OpenAPI 3.0 is the default: null
    /// is a type (`["string", "null"]`, a `$ref` `anyOf` it and `{ "type": "null" }`), keywords
    /// stand beside a `$ref`, a union of plain types is a list of types and any other an `anyOf`,
    /// and a record says it has no other fields, as one dropped them from what a client sends
    /// (FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
    json_schema: bool,
    /// A tool's arguments, where a record's other fields are dropped, so it says it has none; a
    /// result's record may hold more, which nothing checks or drops (found in review).
    input: bool,
}

impl<'a> Schemas<'a> {
    /// Each type's key is its name, after its prefix for a mount's or a service's (`shop.Item`),
    /// in the characters a key may have; two that come out the same are told apart in the order
    /// of their names, so a key does not depend on which function names its type first, nor on
    /// the order the functions are listed in (FEEDBACK56.md, Q-855, found in review; branch
    /// feedback56).
    fn new(types: &'a std::collections::HashMap<String, Types>, refs: &'static str) -> Self {
        let mut all: Vec<(&String, &String)> = types.iter().flat_map(|(p, ts)| ts.keys().map(move |n| (p, n))).collect();
        all.sort();
        let mut keys = std::collections::HashMap::new();
        let mut taken = std::collections::HashSet::new();
        for (prefix, n) in all {
            let place = prefix.trim_matches('/').replace('/', ".");
            let base: String = if place.is_empty() { n.to_string() } else { format!("{}.{}", place, n) }
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
                .collect();
            let mut key = base.clone();
            let mut i = 1;
            while !taken.insert(key.clone()) {
                i += 1;
                key = format!("{}-{}", base, i);
            }
            keys.insert((prefix.clone(), n.clone()), key);
        }
        Schemas { types, prefix: String::new(), refs, keys, named: Vec::new(), held: true, json_schema: false, input: false }
    }

    /// For one schema of a tool, its arguments' (`input`) or its result's, JSON Schema 2020-12,
    /// its types under its own `$defs` (FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
    pub(crate) fn for_tools(types: &'a std::collections::HashMap<String, Types>, input: bool) -> Self {
        let mut s = Schemas::new(types, "#/$defs/");
        s.json_schema = true;
        s.input = input;
        s.held = input;
        s
    }

    /// `{ "$ref": ... }` for one of the program's types.
    fn refer(&mut self, n: &str) -> Option<J> {
        let td = self.types.get(&self.prefix)?.get(n)?;
        let key = self.keys.get(&(self.prefix.clone(), n.to_string()))?.clone();
        if !self.named.iter().any(|(_, _, k, _)| *k == key) {
            self.named.push((self.prefix.clone(), n.to_string(), key.clone(), td.is_record()));
        }
        Some(json!({ "$ref": format!("{}{}", self.refs, key) }))
    }

    /// Whether a schema is a `$ref` to a record.
    fn is_record_ref(&self, s: &J) -> bool {
        let key = s.get("$ref").and_then(J::as_str).and_then(|r| r.strip_prefix(self.refs));
        key.is_some_and(|k| self.named.iter().any(|(_, _, m, record)| m == k && *record))
    }

    /// A `$ref` that something is said beside: OpenAPI 3.0 reads nothing next to a `$ref`, so
    /// it goes in an `allOf`, an object's said to be one.
    fn beside_ref(&self, s: J) -> J {
        // 2020-12 reads what is beside a `$ref` (FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
        if s.get("$ref").is_none() || self.json_schema {
            return s;
        }
        let record = self.is_record_ref(&s);
        let mut w = json!({ "allOf": [s] });
        if record {
            w["type"] = json!("object");
        }
        w
    }

    fn nullable(&self, s: J) -> J {
        if self.json_schema {
            return or_null(s);
        }
        let mut s = self.beside_ref(s);
        s["nullable"] = json!(true);
        s
    }

    /// A schema's JSON type, a whole number's a number's; a record's `$ref` an object's.
    fn kind<'j>(&self, s: &'j J) -> Option<&'j str> {
        match s.get("type").and_then(J::as_str) {
            Some("integer") => Some("number"),
            Some(t) => Some(t),
            None if self.is_record_ref(s) => Some("object"),
            None => None,
        }
    }

    /// Whether no value fits both: their JSON types differ (a whole number is a number too),
    /// their `enum`s have no value in common, or both are objects that require a field no value
    /// of which fits both (`kind: "refund"` and `kind: "new"`). A record's `$ref` is an object
    /// whose fields are not looked into.
    fn disjoint(&self, a: &J, b: &J) -> bool {
        let null = |s: &J| s.get("nullable") == Some(&json!(true));
        if null(a) && null(b) {
            return false;
        }
        let (Some(ka), Some(kb)) = (self.kind(a), self.kind(b)) else { return false };
        if ka != kb {
            return true;
        }
        if let (Some(ea), Some(eb)) = (a.get("enum").and_then(J::as_array), b.get("enum").and_then(J::as_array)) {
            return !ea.iter().any(|v| eb.contains(v));
        }
        let required = |s: &J| s.get("required").and_then(J::as_array).cloned().unwrap_or_default();
        let theirs = required(b);
        required(a).iter().filter(|k| theirs.contains(k)).filter_map(J::as_str).any(|k| self.disjoint(&a["properties"][k], &b["properties"][k]))
    }

    /// One of several schemas: `oneOf` when no value can be two of them (an int and a text,
    /// `"cat"` and `"dog"`), `anyOf` when one can (two records, `int | float`, a type not known),
    /// since the value passes as the first it fits. A route's alternatives and several routes'
    /// bodies are said the same way (FEEDBACK56.md, Q-855, found in review; branch feedback56).
    fn either(&self, alts: Vec<J>) -> J {
        // A list of types where each is a type alone (`int | str`), else `anyOf`: the value
        // passes as the first it fits, and composition keywords are kept few, as MCP asks
        // (FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
        if self.json_schema {
            let types: Option<Vec<J>> = alts
                .iter()
                .map(|a| a.as_object().filter(|o| o.len() == 1).and_then(|o| o.get("type")).cloned())
                .collect::<Option<Vec<J>>>()
                .map(|ts| ts.into_iter().flat_map(|t| if let J::Array(a) = t { a } else { vec![t] }).collect());
            return match types {
                Some(mut ts) => {
                    let mut seen = Vec::new();
                    ts.retain(|t| !seen.contains(t) && {
                        seen.push(t.clone());
                        true
                    });
                    json!({ "type": if ts.len() == 1 { ts.remove(0) } else { J::Array(ts) } })
                }
                None => json!({ "anyOf": alts }),
            };
        }
        let overlap = alts.iter().enumerate().any(|(i, a)| alts[i + 1..].iter().any(|b| !self.disjoint(a, b)));
        json!({ if overlap { "anyOf" } else { "oneOf" }: alts })
    }

    /// A type: a list's items, a map's fields, one of the program's types by `$ref`, a union of
    /// texts one `enum`.
    pub(crate) fn ty(&mut self, t: &Ty) -> J {
        let mut alts: Vec<J> = Vec::new();
        let mut texts: Vec<J> = Vec::new();
        let mut texts_at = None;
        for n in t.names.iter().filter(|n| *n != "null") {
            if n.starts_with('"') {
                texts_at.get_or_insert(alts.len());
                texts.push(json!(serde_json::from_str::<String>(n).unwrap_or_default()));
                continue;
            }
            alts.push(match (n.as_str(), &t.elem, &t.fields) {
                ("list", Some(e), _) => json!({ "type": "array", "items": self.ty(e) }),
                ("map", _, Some(fs)) => self.fields(fs, &[]),
                (n, _, _) => self.name(n),
            });
        }
        if let Some(i) = texts_at {
            alts.insert(i, json!({ "type": "string", "enum": texts }));
        }
        let s = if alts.len() == 1 { alts.remove(0) } else { self.either(alts) };
        if may_be_null(t) {
            self.nullable(s)
        } else {
            s
        }
    }

    /// A map's or a record's fields: each one without `?` and without a default is required,
    /// as a call is refused without it, where they are checked (`held`), and a default is said
    /// when it is written as a value.
    fn fields(&mut self, fs: &[(String, Ty)], defaults: &[(String, Expr)]) -> J {
        let mut props = Map::new();
        let mut required = Vec::new();
        for (k, t) in fs {
            let mut s = self.ty(t);
            let default = defaults.iter().find(|(d, _)| d == k);
            if let Some(v) = default.and_then(|(_, e)| literal(e)) {
                s = self.beside_ref(s);
                s["default"] = v;
            }
            if self.held && default.is_none() && !may_be_null(t) {
                required.push(json!(k));
            }
            props.insert(k.clone(), s);
        }
        let mut s = json!({ "type": "object", "properties": props });
        if !required.is_empty() {
            s["required"] = J::Array(required);
        }
        s
    }

    /// A type's name: Quillo's own, or the program's by `$ref`; another as it is named.
    fn name(&mut self, n: &str) -> J {
        match self.refer(n) {
            Some(r) => r,
            // A name Quillo has no schema for is any value, in 2020-12's words alone
            // (FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
            None if self.json_schema => match n {
                // A decimal's text, by a pattern: `format: "decimal"` is no format 2020-12 has,
                // and a validator that checks formats refuses the schema (found in review).
                "dec" => decimal_text(),
                "money" => json!({
                    "type": "object",
                    "required": ["amount", "currency"],
                    "properties": { "amount": decimal_text(), "currency": { "type": "string", "pattern": "^[A-Za-z]{3}$" } }
                }),
                _ => match name_schema(n) {
                    s if s.get("x-quillo-type").is_some() => json!({}),
                    s => s,
                },
            },
            None => name_schema(n),
        }
    }

    /// The named types referred to, each written once, by key: one written may refer to more.
    pub(crate) fn written(&mut self) -> Map<String, J> {
        let mut out = std::collections::BTreeMap::new();
        let mut i = 0;
        while i < self.named.len() {
            let (prefix, name, key, _) = self.named[i].clone();
            let types = self.types;
            let Some(td) = types.get(&prefix).and_then(|t| t.get(&name)) else { break };
            let outer = std::mem::replace(&mut self.prefix, prefix);
            // A record is checked field by field wherever it is named; an alias as the place
            // that names it is.
            // A tool's schemas are each their own, so an alias in its parameters is held as they
            // are (FEEDBACK56.md, Q-853, Part B.2; branch feedback56).
            let hold = td.is_record() || (self.json_schema && self.held);
            let held = std::mem::replace(&mut self.held, hold);
            let mut s = match (&td.ty.fields, td.is_record()) {
                (Some(fs), true) => {
                    let mut s = self.fields(fs, &td.defaults);
                    // What a client sends that a record does not have is dropped: said
                    // (FEEDBACK56.md, Smaller notes; branch feedback56).
                    if self.json_schema && self.input {
                        s["additionalProperties"] = json!(false);
                    }
                    s
                }
                _ => self.ty(&td.ty),
            };
            self.held = held;
            self.prefix = outer;
            if key != name {
                s = self.beside_ref(s);
                s["title"] = json!(name);
            }
            out.insert(key, s);
            i += 1;
        }
        out.into_iter().collect()
    }

    /// A route's payload pattern: `{ form: {...} }`, `{ json: {...} }`.
    fn request_body(&mut self, p: Option<&Pattern>) -> Option<J> {
        let Some(Pattern::Record(fields)) = p else { return None };
        let mut content = Map::new();
        for f in fields {
            let kind = match f.path.first().map(String::as_str) {
                Some("form") => "application/x-www-form-urlencoded",
                Some("json") => "application/json",
                _ => continue,
            };
            content.insert(kind.into(), json!({ "schema": self.pattern(&f.pat) }));
        }
        (!content.is_empty()).then(|| json!({ "required": true, "content": content }))
    }

    fn field(&mut self, f: &RecField) -> (J, bool) {
        let optional = f.default.is_some() || matches!(&f.pat, Pattern::Alt(ps) if ps.iter().any(|p| matches!(p, Pattern::Lit(Lit::Nil))));
        (self.pattern(&f.pat), !optional)
    }

    fn pattern(&mut self, p: &Pattern) -> J {
        match p {
            // A route's `{ json: Note }` by `$ref` too (FEEDBACK56.md, Q-855; branch feedback56).
            Pattern::Type(t) => self.name(t),
            Pattern::Lit(Lit::Str(s)) => json!({ "type": "string", "enum": [s.to_string()] }),
            Pattern::Lit(Lit::Int(n)) => json!({ "type": "integer", "enum": [n] }),
            Pattern::Lit(Lit::Bool(b)) => json!({ "type": "boolean", "enum": [b] }),
            Pattern::Alt(ps) => {
                let some: Vec<J> = ps.iter().filter(|p| !matches!(p, Pattern::Lit(Lit::Nil))).map(|p| self.pattern(p)).collect();
                let nullable = some.len() < ps.len();
                let s = if some.len() == 1 { some.into_iter().next().unwrap() } else { self.either(some) };
                if nullable {
                    self.nullable(s)
                } else {
                    s
                }
            }
            Pattern::As(q, _) => self.pattern(q),
            Pattern::List(items, _) => json!({ "type": "array", "items": items.first().map_or(json!({}), |p| self.pattern(p)) }),
            Pattern::Record(fields) => {
                let mut props = Map::new();
                let mut required = Vec::new();
                for f in fields {
                    let (schema, req) = self.field(f);
                    let name = f.path.join(".");
                    if req {
                        required.push(json!(name));
                    }
                    props.insert(name, schema);
                }
                let mut s = json!({ "type": "object", "properties": props });
                if !required.is_empty() {
                    s["required"] = J::Array(required);
                }
                s
            }
            _ => json!({}),
        }
    }
}

/// Whether a value of the type may be null, or left out of a map or a record.
fn may_be_null(t: &Ty) -> bool {
    t.optional || t.names.iter().any(|n| n == "null")
}

/// A decimal as text, as `dec` takes it: a sign, digits with a point, an exponent
/// (FEEDBACK56.md, Q-853, Part B.2, found in review; branch feedback56).
fn decimal_text() -> J {
    json!({ "type": "string", "pattern": "^[+-]?([0-9]+[.]?[0-9]*|[.][0-9]+)([eE][+-]?[0-9]{1,6})?$", "description": "a decimal number, as text: \"12.50\"" })
}

/// `s`, or null (JSON Schema 2020-12): `null` among its types, and in its `enum`; a `$ref` or an
/// `anyOf` an alternative more; `{}` takes null already (FEEDBACK56.md, Q-853, Part B.2; branch
/// feedback56).
fn or_null(mut s: J) -> J {
    let null = J::String("null".into());
    match s.get("type").cloned() {
        Some(J::String(t)) => s["type"] = J::Array(vec![J::String(t), null]),
        Some(J::Array(mut ts)) => {
            if !ts.contains(&null) {
                ts.push(null);
            }
            s["type"] = J::Array(ts);
        }
        _ if s.as_object().is_some_and(|o| o.is_empty()) => return s,
        _ => {
            if let Some(J::Array(alts)) = s.get_mut("anyOf") {
                if !alts.iter().any(|a| a == &json!({ "type": "null" })) {
                    alts.push(json!({ "type": "null" }));
                }
                return s;
            }
            return json!({ "anyOf": [s, { "type": "null" }] });
        }
    }
    if let Some(J::Array(e)) = s.get_mut("enum") {
        if !e.contains(&J::Null) {
            e.push(J::Null);
        }
    }
    s
}

/// A default written as a value (`"member"`, `[]`, `{ a: 1 }`), as JSON; one worked out when a
/// record is made (`now()`) is not known here.
pub(crate) fn literal(e: &Expr) -> Option<J> {
    match e {
        Expr::Lit(Lit::Bool(b)) => Some(json!(b)),
        Expr::Lit(Lit::Int(n)) => Some(json!(n)),
        Expr::Lit(Lit::Float(f)) => serde_json::Number::from_f64(*f).map(J::Number),
        Expr::Lit(Lit::Dec(d)) => Some(json!(d.to_string())),
        Expr::Lit(Lit::Str(s)) => Some(json!(&**s)),
        Expr::Template(parts) => parts.iter().map(|p| if let TPart::Lit(s) = p { Some(s.as_str()) } else { None }).collect::<Option<String>>().map(J::String),
        Expr::Unary(UnOp::Neg, x, _) => match &**x {
            Expr::Lit(Lit::Int(n)) => n.checked_neg().map(|n| json!(n)),
            Expr::Lit(Lit::Float(f)) => serde_json::Number::from_f64(-*f).map(J::Number),
            _ => None,
        },
        Expr::List(items) => items.iter().map(literal).collect::<Option<Vec<J>>>().map(J::Array),
        Expr::Map(entries) => entries.iter().map(|(k, v)| Some((k.clone(), literal(v)?))).collect::<Option<Map<String, J>>>().map(J::Object),
        _ => None,
    }
}

/// `/users/:id(int)` as `/users/{id}`, with the parameter.
fn openapi_path(prefix: &str, segs: &[Seg]) -> (String, Vec<J>) {
    let mut path = prefix.to_string();
    let mut params = Vec::new();
    for s in segs {
        path.push('/');
        match s {
            Seg::Lit(l) => path.push_str(l),
            Seg::Param(n, t, p) => {
                path.push_str(&format!("{}{{{}}}", p, n));
                let schema = t.as_deref().map_or(json!({ "type": "string" }), |t| name_schema(t));
                params.push(json!({ "name": n, "in": "path", "required": true, "schema": schema }));
            }
            Seg::Rest(n) => {
                path.push_str(&format!("{{{}}}", n));
                // A template matches one segment: said, for a gateway to route the rest itself
                // (FEEDBACK32.md, Q-405; branch feedback27-35-fixes).
                params.push(json!({ "name": n, "in": "path", "required": true, "schema": { "type": "string" }, "description": "the rest of the path, slashes included", "x-quillo-rest": true }));
            }
        }
    }
    if path.is_empty() {
        path.push('/');
    }
    (path, params)
}

/// A Quillo type's name as JSON Schema.
fn name_schema(n: &str) -> J {
    match n {
        "int" => json!({ "type": "integer" }),
        "float" | "num" => json!({ "type": "number" }),
        "dec" => json!({ "type": "string", "format": "decimal" }),
        "str" => json!({ "type": "string" }),
        "date" => json!({ "type": "string", "format": "date" }),
        // Money: an amount as a decimal's text, and an ISO 4217 code (FEEDBACK50.md, Q-621;
        // branch feedback50-fixes).
        "money" => json!({
            "type": "object",
            "required": ["amount", "currency"],
            "properties": { "amount": { "type": "string", "format": "decimal" }, "currency": { "type": "string", "pattern": "^[A-Za-z]{3}$" } }
        }),
        "bool" => json!({ "type": "boolean" }),
        "list" => json!({ "type": "array" }),
        "map" => json!({ "type": "object" }),
        "any" => json!({}),
        other => json!({ "x-quillo-type": other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_contract_and_a_document() {
        let src = "route GET /users/:id(int) ? { full: bool = false } = id\nroute POST /signup { form: { name: str, age: int | null } } = 1\nserver fn assess(n: int, size: int = 3) -> map ! str = { n }\n";
        let dir = std::env::temp_dir().join(format!("quillo-describe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("api.qwe");
        std::fs::write(&file, src).unwrap();
        let doc = of_source(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(doc["x-quillo-server-fns"]["assess"]["contract"], "assess(n: int, size?: int) -> map ! str");
        let get = &doc["paths"]["/users/{id}"]["get"];
        assert_eq!(get["parameters"][0], json!({ "name": "id", "in": "path", "required": true, "schema": { "type": "integer" } }));
        assert_eq!(get["parameters"][1]["required"], false);
        let form = &doc["paths"]["/signup"]["post"]["requestBody"]["content"]["application/x-www-form-urlencoded"]["schema"];
        assert_eq!(form["required"], json!(["name"]));
        assert_eq!(form["properties"]["age"], json!({ "type": "integer", "nullable": true }));
    }
}
