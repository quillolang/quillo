//! FEEDBACK43.md's bugs (Q-488 to Q-493), and FEEDBACK41.md's Q-476: what reaches a database or
//! a plugin is held to `with`, a switched database is announced, one database has one name, and
//! a failure's kind is read from its code.

mod support;

use quillo::check::{check_source, Severity};
use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::Path;

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut hs: Vec<(String, String)> = vec![("Host".into(), "localhost".into())];
    hs.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: hs, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

fn json(r: &HttpResponse) -> serde_json::Value {
    serde_json::from_slice(&r.body).unwrap_or_else(|e| panic!("{}: {}", e, text(r)))
}

// ---- Q-488: a connection's calls need the db effect -------------------------------------------

#[test]
fn a_connections_calls_need_the_db_effect_as_dbs_do() {
    let d = diags(
        r#"fn direct() with http = db.query("select 1 as x")
let a = db.connect("b.db")
fn via() with http = a.query("select 1 as x")
fn passed(c) with http = c.query("select 1 as x")
fn helper() = a.exec("delete from t")
fn outer() with http = helper()
fn fine() with db = a.query("select 1 as x")
fn typed(c: map) with http = c.query("x")
fn named(c) with http = c.sql_name("x")
fn loose(c) = c.query("x")
fn shadowed() with http {
    let a = { query: (s) => s }
    a.query("x")
}
"#,
    );
    assert_eq!(
        d,
        [
            "1 `db.query` needs the db effect, but `direct` is declared `with http`",
            "3 `a.query` needs the db effect, but `via` is declared `with http`",
            "4 warning: `c.query` reads like a database call, which needs the db effect, but `passed` is declared `with http`: if `c` is a connection, add db",
            "6 warning: `helper` may use the db effect, but `outer` is declared `with http`",
        ],
    );
}

// ---- Q-489 and Q-476: plugins and save_upload ---------------------------------------------------

#[test]
fn a_plugins_effects_are_what_its_header_says() {
    let p = quillo::native::parse("//! effects: fs, http\n//! [dependencies]\n//! x = \"1\"\npub fn f() -> bool { true }\n");
    assert_eq!(p.exports[0].effects, quillo::ast::effect_bit("fs") | quillo::ast::effect_bit("http"));
    assert_eq!(p.deps.trim(), "x = \"1\"", "the dependencies are read after it");
    assert_eq!(quillo::native::parse("//! effects: none\npub fn f() -> bool { true }\n").exports[0].effects, 0);
    assert_eq!(quillo::native::parse("//! effects:\npub fn f() -> bool { true }\n").exports[0].effects, 0);
    // An alias is the db effect; a word that is not an effect is said.
    let p = quillo::native::parse("//! effects: db.analytics, disk\npub fn f() -> bool { true }\n");
    assert_eq!(p.exports[0].effects, quillo::ast::effect_bit("db"));
    assert_eq!(p.problems, [(1, "`disk` is not an effect (they are db, http, fs, env, time, random, net, or none)".to_string())]);
    // Nothing said: anything.
    assert_eq!(quillo::native::parse("pub fn f() -> bool { true }\n").exports[0].effects, quillo::ast::ALL_EFFECTS);
    assert_eq!(quillo::ast::builtin_effect("native:/nowhere/lib.so:f"), quillo::ast::ALL_EFFECTS);
}

#[test]
fn a_call_to_a_service_that_is_not_here_needs_the_http_effect() {
    let d = diags("service gone = \"gone.qwe\"\nfn f() with time = gone.add(1, 2)\nfn g() with http = gone.add(1, 2)\nfn h() = gone.add(1)\nfn k() with db = h()\n");
    assert_eq!(
        d,
        [
            "1 warning: service gone: gone.qwe is not here, so its calls are not checked; it must run elsewhere (QUILLO_SERVICE_GONE or --service-url)",
            "2 `gone.add` needs the http effect, but `f` is declared `with time`",
            "5 warning: `h` may use the http effect, but `k` is declared `with db`",
        ]
    );
}

#[test]
fn only_a_databases_sql_name_makes_safe_sql() {
    let d = diags("let q = { sql_name: (s) => s }\nfn f(x) = db.query(`select ${q.sql_name(x)} from t`)\nfn g(x) = db.query(`select ${db.sql_name(x)} from t`)\n");
    assert!(d.len() == 1 && d[0].starts_with("2 warning:"), "{:?}", d);
}

#[test]
fn save_upload_needs_the_fs_effect() {
    let d = diags("fn keep(u) with time = save_upload(u, \"up.txt\")\nfn fine(u) with fs = save_upload(u, \"up.txt\")\n");
    assert_eq!(d, ["1 `save_upload` needs the fs effect, but `keep` is declared `with time`"]);
}

#[test]
fn a_plugin_that_says_nothing_is_refused_inside_a_with() {
    let d = support::temp_dir("f43-plugins");
    // Written where the test's folder is, not the process's (the repository, under cargo test).
    let (one, two) = (d.join("store.txt").display().to_string(), d.join("store2.txt").display().to_string());
    std::fs::write(d.join("store.qwer"), format!("pub fn store_put(v: &str) -> Result<bool, String> {{\n    std::fs::write({:?}, v).map_err(|e| e.to_string())?;\n    Ok(true)\n}}\n", one)).unwrap();
    std::fs::write(d.join("declared.qwer"), format!("//! effects: fs\npub fn put2(v: &str) -> Result<bool, String> {{\n    std::fs::write({:?}, v).map_err(|e| e.to_string())?;\n    Ok(true)\n}}\n", two)).unwrap();
    let main = r#"use "store.qwer"
use "declared.qwer"
fn pure_time() with time = store_put("x")
fn files_ok() with fs = put2("y")
fn time_only() with time = put2("z")
route GET /outside = str(store_put("outside any with"))
route GET /pure = str(pure_time())
route GET /files = str(files_ok())
route GET /time = str(time_only())
"#;
    std::fs::write(d.join("main.qwe"), main).unwrap();
    let checked = quillo::check::check_file(&d.join("main.qwe"));
    let said: Vec<String> = checked.diagnostics.iter().map(|x| format!("{} {}", x.line, x.message)).collect();
    assert_eq!(
        said,
        [
            "3 `store_put` is Rust whose file does not say what it does, so it may do anything and is refused in `pure_time`, whose `with` limits what it may do: put `//! effects: ...` (or `//! effects: none`) at the top of store.qwer",
            "5 `put2` needs the fs effect, but `time_only` is declared `with time`",
        ]
    );
    // Run anyway: the same, at run time.
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/outside", &[], "")), "true");
    assert_eq!(text(&send(&mut it, "GET", "/files", &[], "")), "true");
    let pure = send(&mut it, "GET", "/pure", &[], "");
    assert_eq!(pure.status, 500);
    assert_eq!(std::fs::read_to_string(d.join("store.txt")).unwrap(), "outside any with", "the call in a `with` wrote nothing");
    assert_eq!(send(&mut it, "GET", "/time", &[], "").status, 500);
    assert_eq!(std::fs::read_to_string(d.join("store2.txt")).unwrap(), "y", "only the call its `with` allows wrote");
}

// ---- Q-490: a database opened again is announced --------------------------------------------

const SWITCH: &str = r#"db.open("a.db")
db.exec("create table if not exists notes (text text)")
db.exec("delete from notes")
db.exec("insert into notes values ('from a')")
server fn load_notes() = db.query("select text from notes")
route GET /switch = {
    db.open("c.db")
    db.exec("create table if not exists notes (text text)")
    db.exec("delete from notes")
    db.exec("insert into notes values ('from c')")
    "switched"
}
route GET /quiet = { db.open("a.db"); "back to a" }
"#;

#[test]
fn a_database_opened_again_is_announced_and_not_answered_from_the_old_one() {
    let d = support::temp_dir("f43-switch");
    std::fs::write(d.join("main.qwe"), SWITCH).unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let live = [("X-Quillo-Live", "1"), ("Content-Type", "application/json")];
    let load = |it: &mut Interp| json(&send(it, "POST", "/_quillo/rpc/load_notes", &live, "[]"));
    assert_eq!(load(&mut it)["ok"][0]["text"], "from a");
    assert_eq!(text(&send(&mut it, "GET", "/switch", &[], "")), "switched");
    let now = load(&mut it);
    assert_eq!(now["ok"][0]["text"], "from c");
    let seq = now["seq"].as_u64().unwrap();
    // Opened again and nothing written: still said, and the next live call is not given c's rows.
    assert_eq!(text(&send(&mut it, "GET", "/quiet", &[], "")), "back to a");
    let changed = json(&send(&mut it, "GET", &format!("/_quillo/live?since={}&t=db%3Anotes,var%3Ax", seq), &[], ""));
    assert_eq!(changed["changed"], serde_json::json!(["db:notes"]), "{}", changed);
    assert_eq!(load(&mut it)["ok"][0]["text"], "from a");
    // A connection's own tables are not the program's database: not said.
    assert!(it.shared.unchanged_since(&["db@abc:notes".into(), "var:x".into()], seq));
}

#[test]
fn a_database_switched_in_a_tenants_request_is_every_tenants() {
    let s = quillo::shared::Shared::new();
    let seq = s.live_seq();
    s.publish(&["db:*".to_string(), "*@db:*".to_string()]);
    assert!(!s.unchanged_since(&["acme@db:notes".into()], seq));
    assert!(!s.unchanged_since(&["zeta@db:notes".into()], seq));
    assert!(!s.unchanged_since(&["db:notes".into()], seq));
    assert!(s.unchanged_since(&["acme@db@x:notes".into(), "var:x".into()], seq));
}

#[test]
fn a_wildcard_reaches_only_its_own_prefix() {
    let s = quillo::shared::Shared::new();
    let seq = s.live_seq();
    s.publish(&["acme@db:*".to_string()]);
    assert!(!s.unchanged_since(&["acme@db:notes".into()], seq));
    assert!(s.unchanged_since(&["db:notes".into(), "zeta@db:notes".into(), "notes".into()], seq));
    let later = s.live_seq();
    assert!(s.unchanged_since(&["acme@db:notes".into()], later), "and only once");
}

// ---- Q-491: one database, one name ------------------------------------------------------------

#[test]
fn two_urls_of_one_database_are_two_pools_with_the_same_topics() {
    let Some(pg) = support::postgres("f43pools") else { return };
    let url = pg.database("f43pools");
    let d = support::temp_dir("f43-pools");
    std::fs::write(
        d.join("main.qwe"),
        format!(
            r#"let a = db.connect("{url}?application_name=reader")
let b = db.connect("{url}?application_name=writer")
let a2 = db.connect("{url}?application_name=reader")
a.exec("create table if not exists notes (text text)")
server fn read_a() = a.query("select current_setting('application_name') as app, count(*) as n from notes")
server fn read_b() = b.query("select current_setting('application_name') as app")
route GET /same = `${{a == b}} ${{a == a2}}`
"#
        ),
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/same", &[], "")), "false true", "another URL is another pool");
    let live = [("X-Quillo-Live", "1"), ("Content-Type", "application/json")];
    let ra = json(&send(&mut it, "POST", "/_quillo/rpc/read_a", &live, "[]"));
    let rb = json(&send(&mut it, "POST", "/_quillo/rpc/read_b", &live, "[]"));
    assert_eq!((ra["ok"][0]["app"].as_str(), rb["ok"][0]["app"].as_str()), (Some("reader"), Some("writer")));
    let topic = |r: &serde_json::Value| r["topics"].as_array().unwrap().iter().filter_map(|t| t.as_str()).find(|t| t.starts_with("db@")).map(String::from);
    let ta = topic(&ra).unwrap();
    assert!(ta.ends_with(":notes") && !ta.contains('#'), "{}", ta);
}

#[test]
fn one_file_spelled_two_ways_is_one_connection() {
    let d = support::temp_dir("f43-labels");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(
        d.join("main.qwe"),
        r#"let b1 = db.connect("b.db")
let b2 = db.connect("./b.db")
let b3 = db.connect("sub/../b.db")
let other = db.connect("other.db")
server fn write_b1() = b1.exec("insert into notes values ('x')")
server fn read_b2() = b2.query("select count(*) as n from notes")
route GET /same = `${b1 == b2} ${b2 == b3} ${b1 == other}`
b1.exec("create table if not exists notes (text text)")
"#,
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/same", &[], "")), "true true false");
    let live = [("X-Quillo-Live", "1"), ("Content-Type", "application/json")];
    let read = json(&send(&mut it, "POST", "/_quillo/rpc/read_b2", &live, "[]"));
    let topic = read["topics"].as_array().unwrap().iter().find(|t| t.as_str().unwrap().starts_with("db@")).unwrap().clone();
    let seq = read["seq"].as_u64().unwrap();
    send(&mut it, "POST", "/_quillo/rpc/write_b1", &[("Content-Type", "application/json")], "[]");
    assert!(!it.shared.unchanged_since(&[topic.as_str().unwrap().to_string()], seq), "a write through b1 reaches a page reading through b2");
}

#[test]
fn a_targets_label_leaves_out_how_it_is_spelled() {
    use quillo::db::target_label;
    let d = support::temp_dir("f43-label-text");
    std::fs::create_dir_all(d.join("data")).unwrap();
    std::fs::write(d.join("data/app.db"), "").unwrap();
    for t in ["data/app.db", "./data/app.db", "data/../data/app.db", &d.join("data/app.db").display().to_string()] {
        assert_eq!(target_label(&d, t), "data/app.db", "{}", t);
    }
    // Not made yet: the real path of the nearest folder there, so inside the program's folder,
    // a folder not made yet too (where /tmp is a link, as on macOS, and where it is not).
    assert_eq!(target_label(&d, "./data/new.db"), "data/new.db");
    assert_eq!(target_label(&d, "nofolder/x.db"), "nofolder/x.db");
    assert_eq!(target_label(&d, &d.join("a/b/x.db").display().to_string()), "a/b/x.db");
    // Outside the folder: the whole real path.
    let outside = support::temp_dir("f43-label-outside");
    let o = target_label(&d, &outside.join("x.db").display().to_string());
    assert!(o.ends_with("/x.db") && Path::new(&o).is_absolute(), "{}", o);
    // A server: no credentials, no options, the scheme's two spellings one.
    let pg = "postgres://db.internal:5432/shop";
    for t in ["postgres://app:old-secret@db.internal:5432/shop", "postgresql://app:new-secret@DB.internal:5432/shop?sslmode=require", "postgres://db.internal:5432/shop#x"] {
        assert_eq!(target_label(&d, t), pg, "{}", t);
    }
    assert_eq!(target_label(&d, "mysql://root:pw@127.0.0.1:3306/shop"), "mysql://127.0.0.1:3306/shop");
    assert_eq!(target_label(&d, "postgres://db.internal"), "postgres://db.internal/");
    assert_eq!(target_label(&d, ":memory:"), ":memory:");
    assert_eq!(target_label(&d, "tenants/{tenant}.db"), "tenants/{tenant}.db");
}

// ---- Q-492: a name quoted for the database it is for ------------------------------------------

#[test]
fn a_name_is_quoted_for_the_connection_it_is_called_on() {
    let d = support::temp_dir("f43-sql-name");
    std::fs::write(
        d.join("main.qwe"),
        r#"let s = db.connect("s.db")
s.exec("create table items (id int, title text)")
s.exec("insert into items values (1, 'Lamp')")
let col = "title"
route GET /names = `${sql_name(col)} ${db.sql_name(col)} ${s.sql_name(col)} ${s.sql_name(["a", "b"])}`
route GET /rows = json.stringify(s.query(`select ${s.sql_name(col)} from items`))
route GET /bad = str(s.sql_name("x; drop table items"))
"#,
    )
    .unwrap();
    let checked = quillo::check::check_file(&d.join("main.qwe"));
    assert!(checked.diagnostics.is_empty(), "a quoted name is not built SQL: {:?}", checked.diagnostics.iter().map(|x| &x.message).collect::<Vec<_>>());
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/names", &[], "")), r#""title" "title" "title" "a", "b""#);
    assert_eq!(text(&send(&mut it, "GET", "/rows", &[], "")), r#"[{"title":"Lamp"}]"#);
    assert_eq!(send(&mut it, "GET", "/bad", &[], "").status, 500);
    // It asks nothing of the database: no effect needed.
    assert!(diags("let s = db.connect(\"s.db\")\nfn f() with http = s.sql_name(\"x\")\nfn g() with time = db.sql_name(\"x\")\n").is_empty());
}

#[test]
fn on_mysql_beside_sqlite_a_name_is_quoted_as_mysql_reads_it() {
    let Some(m) = support::mariadb("f43sqlname") else { return };
    let d = support::temp_dir("f43-sql-name-mysql");
    let url = m.url("mysql");
    std::fs::write(
        d.join("main.qwe"),
        format!(
            r#"db.open("app.db")
let s = db.connect("{url}")
s.exec("drop table if exists items")
s.exec("create table items (id int, title text)")
s.exec("insert into items values (1, 'Lamp'), (2, 'Desk')")
let col = "title"
route GET /names = `${{sql_name(col)}} ${{s.sql_name(col)}}`
route GET /rows = json.stringify(s.query(`select ${{s.sql_name(col)}} from items order by ${{s.sql_name("id")}}`))
"#
        ),
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/names", &[], "")), "\"title\" `title`");
    assert_eq!(text(&send(&mut it, "GET", "/rows", &[], "")), r#"[{"title":"Lamp"},{"title":"Desk"}]"#);
}

// ---- Q-493: a failure's kind by its code -------------------------------------------------------

#[test]
fn postgres_failures_carry_their_sqlstate_and_their_kind() {
    let Some(pg) = support::postgres("f43kinds") else { return };
    let url = pg.database("f43kinds");
    let d = support::temp_dir("f43-kinds");
    std::fs::write(
        d.join("main.qwe"),
        format!(
            r#"db.open("{url}")
db.exec("create table seats (k int primary key, n int not null check (n > 0))")
db.exec("insert into seats values (1, 1)")
route GET /unique = {{ let e = db.exec("insert into seats values (1, 1)"); `${{db.error_kind(e)}} ${{e.error}}` }}
route GET /not_null = db.error_kind(db.exec("insert into seats values (2, null)"))
route GET /check = db.error_kind(db.exec("insert into seats values (3, 0)"))
route GET /other = str(db.error_kind(db.exec("insert into nowhere values (1)")))
"#
        ),
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    let unique = text(&send(&mut it, "GET", "/unique", &[], ""));
    assert!(unique.starts_with("unique db: ") && unique.ends_with("[23505]"), "{}", unique);
    assert_eq!(text(&send(&mut it, "GET", "/not_null", &[], "")), "not_null");
    assert_eq!(text(&send(&mut it, "GET", "/check", &[], "")), "check");
    assert_eq!(text(&send(&mut it, "GET", "/other", &[], "")), "null");
}

// ---- Statements kept compiled (branch feedback40-43) -------------------------------------------

#[test]
fn a_statement_kept_from_before_a_table_changed_reads_the_table_as_it_is() {
    let d = support::temp_dir("f43-cached");
    std::fs::write(
        d.join("main.qwe"),
        r#"db.open("a.db")
db.exec("create table t (a int)")
db.exec("insert into t values (?)", 1)
let first = json.stringify(db.query("select * from t"))
db.exec("alter table t add column b text")
db.exec("insert into t values (?, ?)", 2, "x")
let second = json.stringify(db.query("select * from t"))
db.exec("drop table t")
db.exec("create table t (z text, y int)")
db.exec("insert into t values (?, ?)", "q", 9)
let third = json.stringify(db.query("select * from t"))
let one = json.stringify(db.one("select * from t where y = ?", 9))
route GET / = [first, second, third, one] | join("\n")
"#,
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    assert_eq!(
        text(&send(&mut it, "GET", "/", &[], "")),
        "[{\"a\":1}]\n[{\"a\":1,\"b\":null},{\"a\":2,\"b\":\"x\"}]\n[{\"z\":\"q\",\"y\":9}]\n{\"z\":\"q\",\"y\":9}"
    );
}
