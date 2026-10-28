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
//! door describes what is behind it.

use crate::ast::*;
use crate::error::QuilloError;
use crate::interp::Interp;
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
    parts.document(title)
}

/// The document of the program in `path`, read without running it.
pub fn of_source(path: &Path) -> Result<J, QuilloError> {
    let mut parts = Parts::default();
    let mut seen = Vec::new();
    collect_file(path, path.parent().unwrap_or(Path::new(".")), "", &mut parts, &mut seen, 0)?;
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
}

fn collect_interp(it: &Interp, prefix: &str, parts: &mut Parts) {
    for r in it.routes.iter() {
        parts.routes.push((prefix.to_string(), r.clone()));
    }
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
        collect_interp(sub, &format!("{}{}", prefix, p), parts);
    }
}

fn collect_file(path: &Path, base: &Path, prefix: &str, parts: &mut Parts, seen: &mut Vec<std::path::PathBuf>, depth: u32) -> Result<(), QuilloError> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&key) || depth > 64 {
        return Ok(());
    }
    seen.push(key);
    let src = std::fs::read_to_string(path).map_err(|e| QuilloError::new(format!("cannot read {}: {}", path.display(), e), 0))?;
    let file: Arc<str> = Arc::from(path.display().to_string());
    let stmts = crate::parser::Parser::new(&src).in_file(Some(file)).parse_program().map_err(|e| e.in_file(&path.display().to_string()))?;
    for s in stmts {
        match s {
            Stmt::Route(r) => parts.routes.push((prefix.to_string(), r)),
            Stmt::Fn(d) if d.server => parts.fns.push((prefix.to_string(), d)),
            Stmt::Use(p, _) => collect_file(&base.join(&p), base, prefix, parts, seen, depth + 1)?,
            // A service with a prefix: its routes are served here.
            Stmt::Service(_, Some(p), file, _, _) => {
                let full = base.join(&file);
                let dir = full.parent().unwrap_or(base).to_path_buf();
                let mut inner = Parts::default();
                collect_file(&full, &dir, &format!("{}{}", prefix, p), &mut inner, seen, depth + 1)?;
                parts.routes.extend(inner.routes);
            }
            Stmt::Mount(p, file, _) => {
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
        let mut paths = Map::new();
        for (prefix, r) in &self.routes {
            let (path, mut params) = openapi_path(prefix, &r.path);
            if let Some(Pattern::Record(fields)) = &r.query {
                for f in fields {
                    let (schema, required) = field_schema(f);
                    params.push(json!({ "name": f.path.join("."), "in": "query", "required": required, "schema": schema }));
                }
            }
            let mut op = Map::new();
            op.insert("responses".into(), json!({ "200": { "description": "the route's answer" } }));
            if !params.is_empty() {
                op.insert("parameters".into(), J::Array(params));
            }
            if let Some(body) = request_body(r.body_pat.as_ref()) {
                op.insert("requestBody".into(), body);
            }
            let file = r.file.as_deref().map(|f| Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            op.insert("x-quillo-at".into(), json!(format!("{}:{}", file.unwrap_or_default(), r.line)));
            let entry = paths.entry(path).or_insert_with(|| J::Object(Map::new()));
            if let J::Object(m) = entry {
                m.insert(r.method.to_ascii_lowercase(), J::Object(op));
            }
        }
        let mut fns = Map::new();
        for (prefix, d) in &self.fns {
            let params: Vec<J> = d
                .params
                .iter()
                .map(|p| json!({ "name": p.name, "schema": p.ty.as_ref().map_or(json!({}), ty_schema), "required": p.default.is_none() }))
                .collect();
            let mut f = Map::new();
            f.insert("contract".into(), json!(contract(d)));
            f.insert("params".into(), J::Array(params));
            if let Some(r) = &d.ret {
                f.insert("returns".into(), ty_schema(r));
            }
            if let Some(e) = &d.err {
                f.insert("error".into(), ty_schema(e));
            }
            f.insert("call".into(), json!(format!("POST {}/_quillo/rpc/{}", prefix, d.name)));
            fns.insert(format!("{}{}", if prefix.is_empty() { String::new() } else { format!("{} ", prefix) }, d.name), J::Object(f));
        }
        json!({
            "openapi": "3.0.3",
            "info": { "title": title, "version": crate::version() },
            "paths": paths,
            "x-quillo-server-fns": fns,
        })
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
            Seg::Param(n, t) => {
                path.push_str(&format!("{{{}}}", n));
                let schema = t.as_deref().map_or(json!({ "type": "string" }), |t| name_schema(t));
                params.push(json!({ "name": n, "in": "path", "required": true, "schema": schema }));
            }
            Seg::Rest(n) => {
                path.push_str(&format!("{{{}}}", n));
                params.push(json!({ "name": n, "in": "path", "required": true, "schema": { "type": "string" }, "description": "the rest of the path" }));
            }
        }
    }
    if path.is_empty() {
        path.push('/');
    }
    (path, params)
}

/// A route's payload pattern: `{ form: {...} }`, `{ json: {...} }`.
fn request_body(p: Option<&Pattern>) -> Option<J> {
    let Some(Pattern::Record(fields)) = p else { return None };
    let mut content = Map::new();
    for f in fields {
        let kind = match f.path.first().map(String::as_str) {
            Some("form") => "application/x-www-form-urlencoded",
            Some("json") => "application/json",
            _ => continue,
        };
        content.insert(kind.into(), json!({ "schema": pattern_schema(&f.pat) }));
    }
    (!content.is_empty()).then(|| json!({ "required": true, "content": content }))
}

fn field_schema(f: &RecField) -> (J, bool) {
    let optional = f.default.is_some() || matches!(&f.pat, Pattern::Alt(ps) if ps.iter().any(|p| matches!(p, Pattern::Lit(Lit::Nil))));
    (pattern_schema(&f.pat), !optional)
}

fn pattern_schema(p: &Pattern) -> J {
    match p {
        Pattern::Type(t) => name_schema(t),
        Pattern::Lit(Lit::Str(s)) => json!({ "type": "string", "enum": [s.to_string()] }),
        Pattern::Lit(Lit::Int(n)) => json!({ "type": "integer", "enum": [n] }),
        Pattern::Lit(Lit::Bool(b)) => json!({ "type": "boolean", "enum": [b] }),
        Pattern::Alt(ps) => {
            let some: Vec<J> = ps.iter().filter(|p| !matches!(p, Pattern::Lit(Lit::Nil))).map(pattern_schema).collect();
            let nullable = some.len() < ps.len();
            let mut s = if some.len() == 1 { some.into_iter().next().unwrap() } else { json!({ "oneOf": some }) };
            if nullable {
                s["nullable"] = json!(true);
            }
            s
        }
        Pattern::As(q, _) => pattern_schema(q),
        Pattern::List(items, _) => json!({ "type": "array", "items": items.first().map_or(json!({}), pattern_schema) }),
        Pattern::Record(fields) => {
            let mut props = Map::new();
            let mut required = Vec::new();
            for f in fields {
                let (schema, req) = field_schema(f);
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

fn ty_schema(t: &Ty) -> J {
    let mut alts: Vec<J> = t
        .names
        .iter()
        .filter(|n| *n != "null")
        .map(|n| match (n.as_str(), &t.elem, &t.fields) {
            ("list", Some(e), _) => json!({ "type": "array", "items": ty_schema(e) }),
            ("map", _, Some(fs)) => {
                let props: Map<String, J> = fs.iter().map(|(k, v)| (k.clone(), ty_schema(v))).collect();
                json!({ "type": "object", "properties": props })
            }
            (n, _, _) => name_schema(n),
        })
        .collect();
    let nullable = t.optional || t.names.iter().any(|n| n == "null");
    let mut s = if alts.len() == 1 { alts.remove(0) } else { json!({ "oneOf": alts }) };
    if nullable {
        s["nullable"] = json!(true);
    }
    s
}

/// A Quillo type's name as JSON Schema.
fn name_schema(n: &str) -> J {
    match n {
        "int" => json!({ "type": "integer" }),
        "float" | "num" => json!({ "type": "number" }),
        "dec" => json!({ "type": "string", "format": "decimal" }),
        "str" => json!({ "type": "string" }),
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
