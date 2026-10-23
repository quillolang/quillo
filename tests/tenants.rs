//! One program for many tenants: each request's tenant, its own database
//! (made when the program signs the tenant up, and migrated), and live
//! updates that stay within it.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-tenants-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("migrations")).unwrap();
    std::fs::write(d.join("migrations/001_notes.sql"), "create table notes (id integer primary key, text text not null);").unwrap();
    d
}

fn app(d: &PathBuf, src: &str) -> Interp {
    let mut it = Interp::new(d);
    if let Err(e) = it.load_source(src, "app.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, host: &str, url: &str, body: &str) -> HttpResponse {
    let headers = vec![("Host".to_string(), host.to_string()), ("Content-Type".to_string(), "application/json".to_string())];
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

const APP: &str = r#"
tenant_from("example.com")
db.open("data/{tenant}.db")
db.migrate()
route POST /signup/:name = with_tenant(name, () => len(db.query("select 1 as x")))
route GET /who = tenant() ?? "none"
route POST /notes { json: { text: str } } {
    db.exec("insert into notes (text) values (?)", text)
    "ok"
}
route GET /notes = db.query("select text from notes order by id") | map((n) => n.text)
"#;

#[test]
fn each_subdomain_is_a_tenant_with_its_own_data() {
    let d = dir("subdomain");
    let mut it = app(&d, APP);
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/who", "").text(), "acme");
    assert_eq!(send(&mut it, "GET", "Beta.Example.com:8080", "/who", "").text(), "beta");
    assert_eq!(send(&mut it, "GET", "example.com", "/who", "").text(), "none");
    // Not a name a tenant can have: no tenant.
    assert_eq!(send(&mut it, "GET", "a.b.example.com", "/who", "").text(), "none");
    assert_eq!(send(&mut it, "GET", "evil.com", "/who", "").text(), "none");

    // A host can name any tenant; only the program makes one.
    let r = send(&mut it, "GET", "ghost.example.com", "/notes", "");
    assert_eq!(r.status, 500, "{}", r.text());
    assert!(!d.join("data/ghost.db").exists());
    assert_eq!(send(&mut it, "POST", "example.com", "/signup/acme", "").text(), "1");
    assert_eq!(send(&mut it, "POST", "example.com", "/signup/beta", "").text(), "1");

    assert_eq!(send(&mut it, "POST", "acme.example.com", "/notes", r#"{"text":"acme's"}"#).text(), "ok");
    assert_eq!(send(&mut it, "POST", "beta.example.com", "/notes", r#"{"text":"beta's"}"#).text(), "ok");
    assert_eq!(send(&mut it, "GET", "acme.example.com", "/notes", "").text(), r#"["acme's"]"#);
    assert_eq!(send(&mut it, "GET", "beta.example.com", "/notes", "").text(), r#"["beta's"]"#);
    // Each tenant has its own file, migrated when made.
    assert!(d.join("data/acme.db").is_file() && d.join("data/beta.db").is_file());
    // Without a tenant there is no database to use.
    let r = send(&mut it, "GET", "example.com", "/notes", "");
    assert_eq!(r.status, 500, "{}", r.text());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_change_wakes_only_the_tenants_own_live_pages() {
    let d = dir("live");
    let mut it = app(&d, APP);
    let live = |it: &mut Interp, since: u64| -> serde_json::Value {
        let url = format!("/_quillo/live?since={}&t=acme@db:notes,beta@db:notes,db:notes", since);
        serde_json::from_str(&send(it, "GET", "acme.example.com", &url, "").text()).unwrap()
    };
    send(&mut it, "POST", "example.com", "/signup/acme", "");
    let before = live(&mut it, 0)["seq"].as_u64().unwrap();
    send(&mut it, "POST", "acme.example.com", "/notes", r#"{"text":"x"}"#);
    assert_eq!(live(&mut it, before)["changed"], serde_json::json!(["acme@db:notes"]));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_function_can_name_the_tenant_and_scripts_pick_one() {
    let d = dir("function");
    let src = r#"
tenant_from((request) => request.headers["x-tenant"])
db.open("data/{tenant}.db")
db.migrate()
with_tenant("seed", () => db.exec("insert into notes (text) values ('seeded')"))
route GET /notes = db.query("select text from notes") | map((n) => n.text)
"#;
    let mut it = app(&d, src);
    let get = |it: &mut Interp, t: &str| {
        let headers = vec![("Host".to_string(), "x".to_string()), ("X-Tenant".to_string(), t.to_string())];
        it.handle(&HttpRequest { method: "GET".into(), url: "/notes".into(), headers, body: Vec::new(), remote: "127.0.0.1".into() }).text()
    };
    assert_eq!(get(&mut it, "seed"), r#"["seeded"]"#);
    assert_eq!(get(&mut it, "other"), "internal error\n");
    assert!(!d.join("data/other.db").exists());
    // A name that is not safe in a file name is no tenant.
    let mut bad = Interp::new(&d);
    let e = bad.load_source("with_tenant(\"../x\", () => 1)", "t.qwe").err().expect("refused").to_string();
    assert!(e.contains("cannot be a tenant"), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}
