//! The audit log (`audit`), kept out of reach of the program's own SQL;
//! backups (`db.backup`); and sensitive values kept out of messages.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-audit-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(d: &PathBuf, src: &str) -> Interp {
    let mut it = Interp::new(d);
    if let Err(e) = it.load_source(src, "app.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, host: &str, url: &str) -> HttpResponse {
    let headers = vec![
        ("Host".to_string(), host.to_string()),
        ("User-Agent".to_string(), "test-agent/1".to_string()),
        ("X-Request-Id".to_string(), "req-1".to_string()),
    ];
    // As the server gives each request.
    it.request_id = Some("req-1".into());
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: Vec::new(), remote: "10.0.0.7".into() })
}

fn fails(dir: &str, src: &str) -> String {
    match Interp::new(dir).load_source(src, "t.qwe") {
        Ok(v) => panic!("expected an error, got {}", v.repr()),
        Err(e) => e.to_string(),
    }
}

fn json(r: HttpResponse) -> serde_json::Value {
    let t = r.text();
    serde_json::from_str(&t).unwrap_or_else(|_| panic!("not JSON ({}): {}", r.status, t))
}

const APP: &str = r#"
db.open("app.db")
db.exec("create table if not exists notes (id integer primary key, text text)")
route POST /login/:id {
    set_session({ user: int(id) })
    audit("login", { method: "password", password: "hunter2" })
}
route POST /notes/:text {
    db.transaction(() => {
        let id = db.exec("insert into notes (text) values (?)", text).last_id
        audit("note.add", { id, text })
        if text == "bad" { return err("refused") }
        id
    })
}
route GET /log = audit.query(request.query.action != null ? { action: request.query.action } : {})
route GET /verify = audit.verify()
route POST /tamper = db.exec("update _quillo_audit set action = 'x'")
route GET /peek = db.query("select count(*) as n from _quillo_audit")[0].n
"#;

#[test]
fn rows_say_who_did_what_and_rollbacks_leave_none() {
    let d = dir("rows");
    let mut it = app(&d, APP);
    let r = json(send(&mut it, "POST", "localhost", "/login/7"));
    assert_eq!(r["seq"], 1, "{}", r);
    assert_eq!(r["hash"].as_str().unwrap().len(), 64);
    assert_eq!(send(&mut it, "POST", "localhost", "/notes/hello").text(), "1");
    // An error value rolls the change back, and its audit row with it.
    let r = send(&mut it, "POST", "localhost", "/notes/bad");
    assert!(r.text().contains("refused"), "{}", r.text());
    assert_eq!(send(&mut it, "GET", "localhost", "/peek").text(), "2");

    let rows = json(send(&mut it, "GET", "localhost", "/log"));
    assert_eq!(rows.as_array().unwrap().len(), 2, "{}", rows);
    // Newest first.
    assert_eq!(rows[0]["action"], "note.add");
    assert_eq!(rows[0]["details"], serde_json::json!({ "id": 1, "text": "hello" }));
    let login = &rows[1];
    assert_eq!(login["action"], "login");
    assert_eq!(login["actor"], "7");
    assert_eq!(login["ip"], "10.0.0.7");
    assert_eq!(login["agent"], "test-agent/1");
    assert_eq!(login["rid"], "req-1");
    // Sensitive details are not kept.
    assert_eq!(login["details"], serde_json::json!({ "method": "password", "password": "[redacted]" }));
    let at = login["at"].as_str().unwrap();
    assert!(at.ends_with('Z') && at.len() == 24, "{}", at);
    assert_eq!(rows[1]["seq"], 1);
    assert_eq!(rows[0]["seq"], 2);

    let only = json(send(&mut it, "GET", "localhost", "/log?action=note.*"));
    assert_eq!(only.as_array().unwrap().len(), 1);
    let v = json(send(&mut it, "GET", "localhost", "/verify"));
    assert_eq!((&v["ok"], &v["rows"]), (&serde_json::json!(true), &serde_json::json!(2)), "{}", v);
    assert_eq!(v["last"], rows[0]["hash"]);

    // The program's own SQL cannot change it.
    let r = send(&mut it, "POST", "localhost", "/tamper");
    assert!(r.text().contains("append-only"), "{}", r.text());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn verify_finds_the_first_row_changed_or_missing() {
    let d = dir("tamper");
    let mut it = app(&d, APP);
    for t in ["a", "b", "c", "d"] {
        send(&mut it, "POST", "localhost", &format!("/notes/{}", t));
    }
    assert_eq!(json(send(&mut it, "GET", "localhost", "/verify"))["ok"], true);
    // Someone with the database file changes a row...
    let raw = rusqlite::Connection::open(d.join("app.db")).unwrap();
    raw.execute("update _quillo_audit set details = '{\"id\":2,\"text\":\"B\"}' where seq = 2", []).unwrap();
    let v = json(send(&mut it, "GET", "localhost", "/verify"));
    assert_eq!((&v["ok"], &v["broken"]["seq"], &v["rows"]), (&serde_json::json!(false), &serde_json::json!(2), &serde_json::json!(1)), "{}", v);
    assert!(v["broken"]["why"].as_str().unwrap().contains("changed"), "{}", v);
    // ... or puts a row's hash right again: the next row no longer follows.
    let prev: String = raw.query_row("select prev from _quillo_audit where seq = 2", [], |r| r.get(0)).unwrap();
    let r: (String, String, String, String) = raw
        .query_row("select tenant, at, action, details from _quillo_audit where seq = 2", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap();
    let rehash = quillo::audit::row_hash(&prev, &r.0, 2, &r.1, None, &r.2, Some(&r.3), Some("req-1"), Some("10.0.0.7"), Some("test-agent/1"));
    raw.execute("update _quillo_audit set hash = ? where seq = 2", [&rehash]).unwrap();
    let v = json(send(&mut it, "GET", "localhost", "/verify"));
    assert_eq!(v["broken"]["seq"], 3, "{}", v);
    // ... or removes one.
    raw.execute("delete from _quillo_audit where seq = 2", []).unwrap();
    let v = json(send(&mut it, "GET", "localhost", "/verify"));
    assert_eq!(v["broken"]["seq"], 2, "{}", v);
    assert!(v["broken"]["why"].as_str().unwrap().contains("missing"), "{}", v);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn each_tenant_has_its_own_chain_and_actor_can_be_set() {
    let d = dir("tenants");
    let src = r#"
tenant_from("example.com")
db.open("data/{tenant}.db")
audit.actor(() => `user-${session().user ?? "nobody"}`)
route POST /signup/:name = with_tenant(name, () => audit("tenant.made", { name }).seq)
route POST /act = audit("did")
route GET /log = audit.query({ limit: 10 }) | map((r) => [r.seq, r.action, r.actor])
route GET /verify = audit.verify()
"#;
    let mut it = app(&d, src);
    assert_eq!(send(&mut it, "POST", "example.com", "/signup/acme").text(), "1");
    assert_eq!(send(&mut it, "POST", "example.com", "/signup/beta").text(), "1");
    send(&mut it, "POST", "acme.example.com", "/act");
    assert_eq!(json(send(&mut it, "GET", "acme.example.com", "/log")), serde_json::json!([[2, "did", "user-nobody"], [1, "tenant.made", "user-nobody"]]));
    assert_eq!(json(send(&mut it, "GET", "beta.example.com", "/log")), serde_json::json!([[1, "tenant.made", "user-nobody"]]));
    assert_eq!(json(send(&mut it, "GET", "acme.example.com", "/verify"))["rows"], 2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn appends_from_several_connections_keep_one_chain() {
    let d = dir("concurrent");
    // Two programs (as two servers would) on one SQLite file.
    let src = r#"
db.open("shared.db")
route POST /a/:n = audit("tick", { n: int(n) }).seq
route GET /verify = audit.verify()
"#;
    std::thread::scope(|s| {
        for w in 0..3 {
            let d = d.clone();
            s.spawn(move || {
                let mut it = app(&d, src);
                for i in 0..40 {
                    let r = send(&mut it, "POST", "localhost", &format!("/a/{}", w * 100 + i));
                    assert_eq!(r.status, 200, "{}", r.text());
                }
            });
        }
    });
    let mut it = app(&d, src);
    let v = json(send(&mut it, "GET", "localhost", "/verify"));
    assert_eq!((&v["ok"], &v["rows"]), (&serde_json::json!(true), &serde_json::json!(120)), "{}", v);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_checker_knows_audit() {
    let check = |src: &str| -> Vec<String> {
        quillo::check::check_source(src, "t.qwe", std::path::Path::new(".")).errors().map(|d| d.message.clone()).collect()
    };
    assert_eq!(check("fn f(id: int) with db = audit(\"contact.delete\", { id })\nf(1)"), Vec::<String>::new());
    assert_eq!(check("let rows = audit.query({ limit: 5 })\nlet v = audit.verify()\nlen(rows)"), Vec::<String>::new());
    let e = check("fn f() with http = audit(\"x\")\nf()");
    assert!(e.iter().any(|m| m.contains("db")), "{:?}", e);
    let e = check("audit.nope()");
    assert!(e.iter().any(|m| m.contains("no function `nope`")), "{:?}", e);
    let e = check("audit(42)");
    assert!(!e.is_empty(), "{:?}", e);
    // At runtime as well: `with` limits it.
    let e = fails(".", "fn f() with http = audit(\"x\")\nf()");
    assert!(e.contains("db"), "{}", e);
}

#[test]
fn a_backup_made_while_writing_opens_whole() {
    let d = dir("backup");
    let src = r#"
db.open("live.db")
db.exec("create table if not exists t (id integer primary key, v text)")
route POST /write/:n {
    db.transaction(() => { for i in range(0, int(n)) { db.exec("insert into t (v) values (?)", `row ${i}`) } })
    "ok"
}
route POST /backup = db.backup("backups/live-{time}.db", { keep: 2 })
route GET /count = db.one("select count(*) as n from t").n
"#;
    let mut it = app(&d, src);
    send(&mut it, "POST", "localhost", "/write/2000");
    // Another server writes to the file while the copies are made.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (d, stop) = (d.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut other = app(&d, src);
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                assert_eq!(send(&mut other, "POST", "localhost", "/write/50").text(), "ok");
                n += 50;
            }
            n
        })
    };
    let mut made = Vec::new();
    for _ in 0..3 {
        let r = json(send(&mut it, "POST", "localhost", "/backup"));
        assert!(r["bytes"].as_i64().unwrap() > 0, "{}", r);
        made.push(r["path"].as_str().unwrap().to_string());
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let written = writer.join().unwrap();
    assert!(written > 0);
    // Two kept, the newest.
    let mut kept: Vec<_> = std::fs::read_dir(d.join("backups")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    kept.sort();
    assert_eq!(kept.len(), 2, "{:?}", kept);
    assert!(made[2].ends_with(&kept[1]));
    // The copy opens on its own, checks out, and holds whole transactions.
    let copy = rusqlite::Connection::open(d.join("backups").join(&kept[1])).unwrap();
    let ok: String = copy.query_row("pragma integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(ok, "ok");
    let n: i64 = copy.query_row("select count(*) from t", [], |r| r.get(0)).unwrap();
    assert!(n >= 2000 && n % 50 == 0, "{}", n);
    let restored = format!("db.open({:?})\ndb.one(\"select count(*) as n from t\").n", d.join("backups").join(&kept[1]).display().to_string());
    assert_eq!(Interp::new(&d).load_source(&restored, "r.qwe").unwrap().repr(), n.to_string());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn sensitive_values_stay_out_of_error_messages() {
    let e = fails(".", "redact([\"tax_id\"])\nlet c = { name: \"Ada\", tax_id: \"X-99\", password: \"hunter2\", note: \"card 4111 1111 1111 1111\" }\nc + 1");
    assert!(!e.contains("X-99") && !e.contains("hunter2") && !e.contains("4111"), "{}", e);
    let e = fails(".", "fn f(x: int) = x\nf({ iban: \"DE89370400440532013000\" })");
    assert!(!e.contains("DE89"), "{}", e);
    // The values themselves are as they were.
    let v = Interp::new(".").load_source("{ password: \"hunter2\" }.password", "t.qwe").unwrap();
    assert_eq!(v.repr(), "\"hunter2\"");
}
