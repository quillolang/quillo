//! Several databases in one program: `db.connect` (each with `db`'s
//! functions, transactions, migrations and live updates of its own), and
//! tenants whose databases a function picks, of any kind (src/db.rs).

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-dbs-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn load(d: &Path, src: &str) -> Interp {
    let mut it = Interp::new(d);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn eval(it: &mut Interp, e: &str) -> String {
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", e), "case.qwe").unwrap();
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

fn request(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut h: Vec<(String, String)> = vec![("Host".into(), "localhost".into())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

const TWO: &str = r#"
db.open("main.db")
db.exec("create table if not exists notes (id integer primary key, text text)")
let analytics = db.connect("analytics.db")
analytics.exec("create table if not exists notes (id integer primary key, text text)")
"#;

#[test]
fn connections_are_databases_of_their_own() {
    let d = dir("two");
    let mut it = load(&d, TWO);
    assert_eq!(eval(&mut it, r#"db.exec("insert into notes (text) values ('main')").changes"#), "1");
    assert_eq!(eval(&mut it, r#"analytics.exec("insert into notes (text) values (?)", "a1").changes"#), "1");
    assert_eq!(eval(&mut it, r#"analytics.exec("insert into notes (text) values (:t)", { t: "a2" }).last_id"#), "2");
    assert_eq!(eval(&mut it, r#"[len(db.query("select * from notes")), len(analytics.query("select * from notes"))]"#), "[1, 2]");
    assert_eq!(eval(&mut it, r#"analytics.one("select text from notes order by id desc").text"#), r#""a2""#);
    assert!(d.join("main.db").is_file() && d.join("analytics.db").is_file());
    // The same target again is the same connection, not a new one.
    assert_eq!(eval(&mut it, r#"db.connect("analytics.db") == analytics"#), "true");
    // A method taken off the connection still knows it in a pipeline.
    assert_eq!(eval(&mut it, r#""select count(*) as n from notes" | analytics.one"#), "{ n: 2 }");
    assert_eq!(eval(&mut it, r#""select count(*) as n from notes" | db.one"#), "{ n: 1 }");
    // Errors are values, as with `db`.
    assert!(eval(&mut it, r#"analytics.query("select * from nowhere")"#).starts_with("err(\"db: no such table"));
    assert!(eval(&mut it, r#"db.connect("/no/such/dir/x.db")"#).starts_with("err(\"db:"));
}

#[test]
fn each_connection_has_its_own_transactions() {
    let d = dir("tx");
    let mut it = load(&d, TWO);
    // The inner one fails and rolls back alone; the outer one commits.
    assert_eq!(
        eval(
            &mut it,
            r#"db.transaction(() => {
    let r = analytics.transaction(() => {
        analytics.exec("insert into notes (text) values ('lost')")
        err("no")
    })
    db.exec("insert into notes (text) values ('kept')")
    r
})"#
        ),
        r#"err("no")"#
    );
    assert_eq!(eval(&mut it, r#"[len(db.query("select * from notes")), len(analytics.query("select * from notes"))]"#), "[0, 0]");
    assert_eq!(
        eval(
            &mut it,
            r#"db.transaction(() => {
    analytics.transaction(() => analytics.exec("insert into notes (text) values ('a')"))
    db.exec("insert into notes (text) values ('m')")
})
[len(db.query("select * from notes")), len(analytics.query("select * from notes"))]"#
        ),
        "[1, 1]"
    );
}

#[test]
fn each_connection_has_its_own_migrations() {
    let d = dir("migrate");
    std::fs::create_dir_all(d.join("migrations/main")).unwrap();
    std::fs::create_dir_all(d.join("migrations/analytics")).unwrap();
    std::fs::write(d.join("migrations/main/001_users.sql"), "create table users (id integer primary key);").unwrap();
    std::fs::write(d.join("migrations/analytics/001_events.sql"), "create table events (id integer primary key);").unwrap();
    let src = "db.open(\"main.db\")\nprint(db.migrate(\"migrations/main\"))\nlet analytics = db.connect(\"analytics.db\")\nprint(analytics.migrate(\"migrations/analytics\"))\n";
    let mut it = load(&d, src);
    assert_eq!(eval(&mut it, r#"[len(db.query("select * from users")), len(analytics.query("select * from events"))]"#), "[0, 0]");
    assert!(eval(&mut it, r#"db.query("select * from events")"#).starts_with("err("));
    assert!(eval(&mut it, r#"analytics.query("select * from users")"#).starts_with("err("));
    // Run again: nothing more to do.
    assert_eq!(eval(&mut it, r#"[db.migrate("migrations/main"), analytics.migrate("migrations/analytics")]"#), "[[], []]");
}

#[test]
fn live_updates_tell_the_databases_apart() {
    let d = dir("live");
    let src = format!("{}\nserver fn main_notes() = db.query(\"select * from notes\")\nserver fn other_notes() = analytics.query(\"select * from notes\")\n", TWO);
    let mut it = load(&d, &src);
    let topics = |it: &mut Interp, f: &str| {
        let r = request(it, "POST", &format!("/_quillo/rpc/{}", f), &[("Content-Type", "application/json"), ("X-Quillo-Live", "1")], "[]");
        let j: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        j["topics"].clone()
    };
    assert_eq!(topics(&mut it, "main_notes"), serde_json::json!(["db:notes"]));
    let other = topics(&mut it, "other_notes");
    let t = other[0].as_str().unwrap().to_string();
    // Its own topic: a label, not the target (which may hold a password).
    assert!(t.starts_with("db@") && t.ends_with(":notes") && !t.contains("analytics"), "{}", t);
}

#[test]
fn a_function_says_where_each_tenant_s_database_is() {
    let d = dir("tenants");
    let src = r#"
let registry = db.connect("registry.db")
registry.exec("create table if not exists tenants (name text primary key, url text)")
registry.exec("insert or replace into tenants values ('acme', 'data/acme.db'), ('zeta', 'stores/zeta.sqlite')")
db.open((tenant) => registry.one("select url from tenants where name = ?", tenant)?.url)
tenant_from((request) => request.headers["x-tenant"])
fn signup(t) = with_tenant(t, () => {
    db.exec("create table if not exists items (n text)")
    db.exec("insert into items values (?)", t)
    "ok"
})
signup("acme")
signup("zeta")
route GET /items = db.query("select n from items") | map((r) => r.n)
"#;
    let mut it = load(&d, src);
    assert!(d.join("data/acme.db").is_file() && d.join("stores/zeta.sqlite").is_file());
    assert_eq!(String::from_utf8_lossy(&request(&mut it, "GET", "/items", &[("X-Tenant", "acme")], "").body), r#"["acme"]"#);
    assert_eq!(String::from_utf8_lossy(&request(&mut it, "GET", "/items", &[("X-Tenant", "zeta")], "").body), r#"["zeta"]"#);
    // A tenant the function knows nothing of has no database.
    assert_eq!(request(&mut it, "GET", "/items", &[("X-Tenant", "ghost")], "").status, 500);
    assert!(eval(&mut it, r#"with_tenant("ghost", () => db.query("select 1"))"#).contains("gave null, not a path or a URL"));
    assert_eq!(eval(&mut it, r#"[db.tenant_exists("acme"), db.tenant_exists("ghost")]"#), "[true, false]");
    // There is no template to list them by.
    assert!(eval(&mut it, "db.tenants()").contains("keep the list of tenants yourself"));
}

#[test]
fn the_function_for_tenants_cannot_use_their_database() {
    let d = dir("loop");
    let mut it = load(&d, "db.open((tenant) => db.one(\"select url from tenants where name = ?\", tenant).url)\n");
    let r = eval(&mut it, r#"with_tenant("acme", () => db.query("select 1"))"#);
    assert!(r.contains("cannot use that database itself"), "{}", r);
}

#[test]
fn quillo_check_knows_the_new_forms() {
    let a = quillo::check::check_source(
        "let analytics = db.connect(\"analytics.db\")\nroute GET / = analytics.query(\"select 1\")\ndb.open((tenant) => `data/${tenant}.db`)\n",
        "main.qwe",
        Path::new("."),
    );
    assert!(a.errors().next().is_none(), "{:?}", a.errors().map(|e| e.message.clone()).collect::<Vec<_>>());
    let a = quillo::check::check_source("db.connect(5)\n", "main.qwe", Path::new("."));
    assert!(a.errors().any(|e| e.message.contains("db.connect")));
}
