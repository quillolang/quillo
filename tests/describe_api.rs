//! `quillo docs` and `/_quillo/describe`: a program's API as OpenAPI.

mod support;

use quillo::describe::{contract, of_interp, of_source};
use quillo::Interp;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use support::temp_dir;

fn doc_of(name: &str, src: &str) -> Value {
    let dir = temp_dir(&format!("describe-{}", name));
    let file = dir.join("api.qwe");
    std::fs::write(&file, src).unwrap();
    let doc = of_source(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    doc
}

#[test]
fn the_document_says_what_it_is_and_what_quillo_made_it() {
    let doc = doc_of("head", "route GET /ping = \"pong\"\n");
    assert_eq!(doc["openapi"], "3.0.3");
    assert_eq!(doc["info"], json!({ "title": "api", "version": quillo::version() }));
    assert_eq!(doc["paths"]["/ping"]["get"]["x-quillo-at"], "api.qwe:1");
    assert_eq!(doc["paths"]["/ping"]["get"]["responses"]["200"]["description"], "the route's answer");
    assert!(doc["paths"]["/ping"]["get"].get("parameters").is_none());
    assert!(doc["paths"]["/ping"]["get"].get("requestBody").is_none());
    assert_eq!(doc["x-quillo-server-fns"], json!({}));
}

#[test]
fn the_root_is_a_slash_and_two_methods_share_a_path() {
    let doc = doc_of("root", "route GET / = 1\nroute GET /items = 2\nroute POST /items = 3\n");
    assert!(doc["paths"]["/"]["get"].is_object());
    let items = doc["paths"]["/items"].as_object().unwrap();
    let mut methods: Vec<&str> = items.keys().map(|k| k.as_str()).collect();
    methods.sort();
    assert_eq!(methods, ["get", "post"]);
}

#[test]
fn path_parameters_keep_their_types_and_a_rest_is_a_string() {
    let doc = doc_of(
        "params",
        "route GET /a/:n(int)/:x(float)/:v(num)/:d(dec)/:on(date)/:s(str)/:plain = 1\nroute GET /files/*tail = tail\nroute GET /all/* = rest\n",
    );
    let p = &doc["paths"]["/a/{n}/{x}/{v}/{d}/{on}/{s}/{plain}"]["get"]["parameters"];
    let schemas: Vec<&Value> = p.as_array().unwrap().iter().map(|x| &x["schema"]).collect();
    assert_eq!(
        schemas,
        [
            &json!({ "type": "integer" }),
            &json!({ "type": "number" }),
            &json!({ "type": "number" }),
            &json!({ "type": "string", "format": "decimal" }),
            &json!({ "type": "string", "format": "date" }),
            &json!({ "type": "string" }),
            &json!({ "type": "string" }),
        ]
    );
    assert!(p.as_array().unwrap().iter().all(|x| x["in"] == "path" && x["required"] == true));
    let rest = &doc["paths"]["/files/{tail}"]["get"]["parameters"][0];
    assert_eq!(rest, &json!({ "name": "tail", "in": "path", "required": true, "schema": { "type": "string" }, "description": "the rest of the path" }));
    assert_eq!(doc["paths"]["/all/{rest}"]["get"]["parameters"][0]["name"], "rest");
}

#[test]
fn query_fields_are_required_unless_they_have_a_default_or_may_be_null() {
    let doc = doc_of("query", "route GET /q ? { page: int, size: int = 10, tag: str | null, active: bool } = 1\n");
    let q = doc["paths"]["/q"]["get"]["parameters"].as_array().unwrap().clone();
    let by = |n: &str| q.iter().find(|p| p["name"] == n).unwrap().clone();
    assert_eq!(by("page"), json!({ "name": "page", "in": "query", "required": true, "schema": { "type": "integer" } }));
    assert_eq!(by("size")["required"], false);
    assert_eq!(by("tag"), json!({ "name": "tag", "in": "query", "required": false, "schema": { "type": "string", "nullable": true } }));
    assert_eq!(by("active")["schema"], json!({ "type": "boolean" }));
}

#[test]
fn a_json_body_says_its_fields_and_other_keys_are_not_a_body() {
    let doc = doc_of(
        "body",
        "route POST /a { json: { name: str, age: int | null, kind: \"cat\" | \"dog\", size: 1 | 2, ok: true, tags: [str, ..], meta: { a: int, b: str = \"x\" }, who: str as w, free: _ } } = 1\nroute POST /b { headers: { x: str } } = 2\n",
    );
    let schema = &doc["paths"]["/a"]["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(doc["paths"]["/a"]["post"]["requestBody"]["required"], true);
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["name"], json!({ "type": "string" }));
    assert_eq!(schema["properties"]["age"], json!({ "type": "integer", "nullable": true }));
    assert_eq!(
        schema["properties"]["kind"],
        json!({ "oneOf": [{ "type": "string", "enum": ["cat"] }, { "type": "string", "enum": ["dog"] }] })
    );
    assert_eq!(schema["properties"]["size"], json!({ "oneOf": [{ "type": "integer", "enum": [1] }, { "type": "integer", "enum": [2] }] }));
    assert_eq!(schema["properties"]["ok"], json!({ "type": "boolean", "enum": [true] }));
    assert_eq!(schema["properties"]["tags"], json!({ "type": "array", "items": { "type": "string" } }));
    assert_eq!(schema["properties"]["meta"], json!({ "type": "object", "properties": { "a": { "type": "integer" }, "b": { "type": "string" } }, "required": ["a"] }));
    assert_eq!(schema["properties"]["who"], json!({ "type": "string" }));
    assert_eq!(schema["properties"]["free"], json!({}));
    assert!(schema["required"].as_array().unwrap().contains(&json!("name")));
    assert!(!schema["required"].as_array().unwrap().contains(&json!("age")));
    assert!(doc["paths"]["/b"]["post"].get("requestBody").is_none(), "headers are not a body: {}", doc["paths"]["/b"]);
}

#[test]
fn a_record_type_in_a_body_is_named_and_an_any_is_open() {
    let doc = doc_of("named", "type User = { name: str }\nroute POST /u { json: User } = 1\nroute POST /v { json: { a: any, b: list, c: map, d: [] } } = 2\n");
    let content = &doc["paths"]["/u"]["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(content, &json!({ "x-quillo-type": "User" }));
    let v = &doc["paths"]["/v"]["post"]["requestBody"]["content"]["application/json"]["schema"]["properties"];
    assert_eq!(v["a"], json!({}));
    assert_eq!(v["b"], json!({ "type": "array" }));
    assert_eq!(v["c"], json!({ "type": "object" }));
    assert_eq!(v["d"], json!({ "type": "array", "items": {} }));
}

#[test]
fn a_server_fn_is_a_contract_and_a_plain_fn_is_not_there() {
    let doc = doc_of(
        "fns",
        "fn helper(x) = x\nserver fn plain(a) = a\nserver fn full(ids: list<int>, who: { name: str, age: int }, n: int | str = 1, note: str? = null) -> list<str> ! str = []\n",
    );
    let fns = doc["x-quillo-server-fns"].as_object().unwrap();
    assert_eq!(fns.len(), 2);
    assert_eq!(fns["plain"], json!({ "contract": "plain(a: any)", "params": [{ "name": "a", "schema": {}, "required": true }], "call": "POST /_quillo/rpc/plain" }));
    let full = &fns["full"];
    assert_eq!(full["contract"], "full(ids: list<int>, who: { name: str, age: int }, n?: int | str, note?: str?) -> list<str> ! str");
    assert_eq!(full["params"][0]["schema"], json!({ "type": "array", "items": { "type": "integer" } }));
    assert_eq!(full["params"][1]["schema"], json!({ "type": "object", "properties": { "name": { "type": "string" }, "age": { "type": "integer" } } }));
    assert_eq!(full["params"][2], json!({ "name": "n", "schema": { "oneOf": [{ "type": "integer" }, { "type": "string" }] }, "required": false }));
    assert_eq!(full["params"][3]["schema"], json!({ "type": "string", "nullable": true }));
    assert_eq!(full["returns"], json!({ "type": "array", "items": { "type": "string" } }));
    assert_eq!(full["error"], json!({ "type": "string" }));
}

#[test]
fn the_contract_is_one_line_of_text() {
    let stmts = quillo::parser::Parser::new("server fn f(a: int, b = 2) -> str ! map = \"\"\n").parse_program().unwrap();
    let quillo::ast::Stmt::Fn(d) = &stmts[0] else { panic!("not a function") };
    assert_eq!(contract(d), "f(a: int, b?: any) -> str ! map");
}

#[test]
fn used_files_mounts_and_prefixed_services_are_under_their_prefixes() {
    let dir = temp_dir("describe-tree");
    std::fs::create_dir_all(dir.join("shop")).unwrap();
    std::fs::write(dir.join("lib.qwe"), "use \"main.qwe\"\nroute GET /lib = 1\nserver fn shared() = 1\n").unwrap();
    std::fs::write(dir.join("shop/app.qwe"), "use \"more.qwe\"\nroute GET /cart = 1\nserver fn checkout() = 1\n").unwrap();
    std::fs::write(dir.join("shop/more.qwe"), "route GET /more = 1\n").unwrap();
    std::fs::write(dir.join("billing.qwe"), "route GET /invoices = 1\nserver fn invoice() = 1\n").unwrap();
    std::fs::write(dir.join("quiet.qwe"), "route GET /hidden = 1\n").unwrap();
    std::fs::write(
        dir.join("main.qwe"),
        "use \"lib.qwe\"\nuse Page from \"page.js\"\nmount /shop = \"shop/app.qwe\"\nservice /billing = \"billing.qwe\"\nservice quiet = \"quiet.qwe\"\nroute GET / = 1\n",
    )
    .unwrap();
    let doc = of_source(&dir.join("main.qwe")).unwrap();
    let mut paths: Vec<&str> = doc["paths"].as_object().unwrap().keys().map(|k| k.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["/", "/billing/invoices", "/lib", "/shop/cart", "/shop/more"]);
    let fns = doc["x-quillo-server-fns"].as_object().unwrap();
    let mut names: Vec<&str> = fns.keys().map(|k| k.as_str()).collect();
    names.sort();
    // A service's functions stay its own: only a mount's are served here.
    assert_eq!(names, ["/shop checkout", "shared"]);
    assert_eq!(fns["/shop checkout"]["call"], "POST /shop/_quillo/rpc/checkout");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_cannot_be_read_or_parsed_is_an_error_with_its_name() {
    let dir = temp_dir("describe-bad");
    let e = of_source(&dir.join("missing.qwe")).unwrap_err();
    assert!(e.msg.starts_with("cannot read ") && e.msg.contains("missing.qwe"), "{}", e.msg);
    std::fs::write(dir.join("bad.qwe"), "route GET /\n").unwrap();
    let e = of_source(&dir.join("bad.qwe")).unwrap_err();
    assert!(e.file.as_deref().map_or(false, |f| f.ends_with("bad.qwe")), "{:?}", e.file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_running_program_describes_its_own_and_its_mounts() {
    let dir = temp_dir("describe-interp");
    std::fs::write(dir.join("sub.qwe"), "route GET /inner = 1\nserver fn deep() = 1\n").unwrap();
    let src = "mount /sub = \"sub.qwe\"\nfn hidden() = 1\nserver fn zeta() = 1\nserver fn alpha(n: int) -> int = n\nroute GET /outer/:id(int) = id\n";
    let mut it = Interp::new(dir.to_str().unwrap());
    it.load_source(src, "main.qwe").unwrap();
    let doc = of_interp(&it, "mine");
    assert_eq!(doc["info"]["title"], "mine");
    assert!(doc["paths"]["/outer/{id}"]["get"].is_object());
    assert!(doc["paths"]["/sub/inner"]["get"].is_object());
    let names: Vec<&str> = doc["x-quillo-server-fns"].as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["alpha", "zeta", "/sub deep"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quillo_docs_prints_the_document_and_fails_on_a_bad_file() {
    let dir = temp_dir("describe-cli");
    let file = dir.join("app.qwe");
    std::fs::write(&file, "route GET /x = 1\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("docs").arg(&file).output().unwrap();
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["info"]["title"], "app");
    assert!(doc["paths"]["/x"]["get"].is_object());
    std::fs::write(&file, "route\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("docs").arg(&file).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("quillo: "));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn describe_is_answered_only_when_it_was_asked_for() {
    let dir = temp_dir("describe-serve");
    let file = dir.join("app.qwe");
    std::fs::write(&file, "route GET /x = 1\nserver fn f() = 1\n").unwrap();
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(&file)
        .args(["--port", &port.to_string(), "--describe"])
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !support::listening(port) {
        assert!(std::time::Instant::now() < end, "server did not start");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let a = support::send(port, "GET", "/_quillo/describe", &[], "");
    assert_eq!(a.status, 200, "{}", a.body);
    let doc: Value = serde_json::from_str(&a.body).unwrap();
    assert!(doc["paths"]["/x"]["get"].is_object());
    assert_eq!(doc["x-quillo-server-fns"]["f"]["call"], "POST /_quillo/rpc/f");

    let plain = support::serve(&file, &[]);
    let a = support::send(plain.port, "GET", "/_quillo/describe", &[], "");
    assert_eq!(a.status, 404, "{}", a.body);
    assert!(Path::new(&file).exists());
    let _ = std::fs::remove_dir_all(&dir);
}
