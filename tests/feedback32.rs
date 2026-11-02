//! The program's API document (FEEDBACK32.md, Q-395 to Q-398 and Q-405; branch
//! feedback27-35-fixes): `--describe` and `quillo docs` agree on a service's functions, several
//! routes on one path are one operation, `--split` keeps the services' routes, and the title and
//! the base path are the same however the program was started. Q-399's routing to joined copies
//! is in tests/services_edge.rs.

mod support;

use serde_json::{json, Value as J};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback32-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn docs(d: &Path) -> J {
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("docs").arg(d.join("main.qwe")).output().unwrap();
    serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)))
}

struct Running(Child, u16);

impl Drop for Running {
    fn drop(&mut self) {
        // Stopped as a person would, so that under `--split` it stops its services too.
        #[cfg(unix)]
        {
            let _ = Command::new("kill").args(["-TERM", &self.0.id().to_string()]).status();
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline && matches!(self.0.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(d: &Path, args: &[&str]) -> Running {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string(), "--describe"])
        .args(args)
        .env("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef")
        .current_dir(d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "did not start");
        std::thread::sleep(Duration::from_millis(30));
    }
    Running(child, port)
}

fn described(port: u16) -> J {
    serde_json::from_str(&send(port, "GET", "/_quillo/describe", &[], "").body).unwrap()
}

const BILLING: &[(&str, &str)] = &[
    ("main.qwe", "service /billing = \"billing.qwe\"\nroute GET / = \"shop\"\nserver fn hello() = \"hi\"\n"),
    ("billing.qwe", "route POST /buy = \"bought\"\nroute GET /files/*path = path\nserver fn invoice(name: str, cents: int) -> map = { name, cents }\n"),
];

#[test]
fn describe_and_docs_agree_on_a_services_functions() {
    let d = dir("agree", BILLING);
    let from_source = docs(&d);
    let r = run(&d, &[]);
    let live = described(r.1);
    assert_eq!(live["x-quillo-server-fns"], from_source["x-quillo-server-fns"], "{}", live);
    assert!(live["x-quillo-server-fns"].get("/billing invoice").is_none());
    assert_eq!(live["x-quillo-server-fns"]["hello"]["call"], "POST /_quillo/rpc/hello");
    let mut a: Vec<&String> = live["paths"].as_object().unwrap().keys().collect();
    let mut b: Vec<&String> = from_source["paths"].as_object().unwrap().keys().collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    // Q-398: one title both ways, the file's name.
    assert_eq!(live["info"]["title"], "main");
    assert_eq!(from_source["info"]["title"], "main");
    assert!(live.get("servers").is_none());
    // Q-405: a rest parameter says it takes slashes.
    assert_eq!(live["paths"]["/billing/files/{path}"]["get"]["parameters"][0]["x-quillo-rest"], true);
}

#[test]
fn under_split_the_services_routes_stay_and_its_own_document_answers() {
    let d = dir("split", BILLING);
    let r = run(&d, &["--split"]);
    let deadline = Instant::now() + Duration::from_secs(20);
    while send(r.1, "POST", "/billing/buy", &[], "").body != "bought" {
        assert!(Instant::now() < deadline, "the service did not answer");
        std::thread::sleep(Duration::from_millis(100));
    }
    let doc = described(r.1);
    assert!(doc["paths"].get("/billing/buy").is_some(), "{}", doc);
    assert!(doc["x-quillo-server-fns"].get("/billing invoice").is_none());
    let own = send(r.1, "GET", "/billing/_quillo/describe", &[], "");
    assert_eq!(own.status, 200, "{}", own.body);
    let own: J = serde_json::from_str(&own.body).unwrap();
    assert!(own["paths"].get("/buy").is_some(), "{}", own);
}

#[test]
fn under_base_the_document_says_where_it_is_served() {
    let d = dir("base", &[("main.qwe", "route GET /users/:id(int) = id\n")]);
    let r = run(&d, &["--base", "/shop"]);
    let doc = described(r.1);
    assert_eq!(doc["servers"], json!([{ "url": "/shop" }]));
    assert!(doc["paths"].get("/users/{id}").is_some());
}

#[test]
fn several_routes_on_one_path_are_one_operation() {
    let d = dir("several", &[(
        "main.qwe",
        r#"route POST /orders { json: { kind: "refund", id: int } } = "refund"
route POST /orders { json: { kind: "new", item: str } } = "new"
route GET /search ? { q: str } = "by q"
route GET /search ? { tag: str, page: int = 1 } = "by tag"
route GET /both ? { q: str } = "a"
route GET /both ? { q: str, more: str = "" } = "b"
route GET /users/:id(int) = id
route GET /users/:name = name
route DELETE /users/:id(int) = id
route GET /one ? { x: int } = x
"#,
    )]);
    let doc = docs(&d);
    let mut paths: Vec<&String> = doc["paths"].as_object().unwrap().keys().collect();
    paths.sort();
    assert_eq!(paths, ["/both", "/one", "/orders", "/search", "/users/{id}"]);
    let body = &doc["paths"]["/orders"]["post"]["requestBody"]["content"]["application/json"]["schema"]["oneOf"];
    assert_eq!(body.as_array().map(|a| a.len()), Some(2), "{}", doc);
    assert_eq!(body[0]["properties"]["kind"]["enum"], json!(["refund"]));
    assert_eq!(body[1]["properties"]["kind"]["enum"], json!(["new"]));
    assert_eq!(doc["paths"]["/orders"]["post"]["x-quillo-at"], json!(["main.qwe:1", "main.qwe:2"]));
    let search = doc["paths"]["/search"]["get"]["parameters"].as_array().unwrap();
    let names: Vec<&str> = search.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["q", "tag", "page"]);
    assert!(search.iter().all(|p| p["required"] == false), "{:?}", search);
    // Required where every route requires it.
    let both = doc["paths"]["/both"]["get"]["parameters"].as_array().unwrap();
    assert_eq!((both[0]["name"].as_str(), both[0]["required"].as_bool()), (Some("q"), Some(true)));
    assert_eq!(both[1]["required"], false);
    let users = &doc["paths"]["/users/{id}"];
    assert_eq!(users["get"]["parameters"][0]["schema"], json!({ "oneOf": [{ "type": "integer" }, { "type": "string" }] }));
    assert_eq!(users["delete"]["parameters"][0]["schema"], json!({ "type": "integer" }));
    // One route stays as it was: one place, one schema.
    assert_eq!(doc["paths"]["/one"]["get"]["x-quillo-at"], "main.qwe:10");
}

/// Under coverage a test binary that calls nothing of the library is not run (see
/// tests/support): this one does.
#[test]
fn the_library_is_linked() {
    assert!(!quillo::version().is_empty());
}
