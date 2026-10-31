//! `idempotent` on each kind of database: the work and its answer commit together, from the
//! first request on a new database, when its table is made. MySQL commits an open transaction
//! at `create table`, which is how `.later` lost its rollback there (FEEDBACK23.md, Q-182).

mod support;

use std::path::PathBuf;
use support::{send, serve};

fn app(name: &str, target: &str) -> PathBuf {
    let d = support::temp_dir(&format!("idempotent-{}", name));
    let f = d.join("main.qwe");
    std::fs::write(&f, format!(r#"
db.open("{}")
db.exec("create table if not exists paid (who varchar(32))")
route POST /fails = idempotent(() => {{
    db.exec("insert into paid (who) values ('fails')")
    err("no")
}})
route POST /crashes = idempotent(() => {{
    db.exec("insert into paid (who) values ('crashes')")
    let m = json.parse("[]")
    m.nothing.at_all
}})
route POST /pays = idempotent(() => {{
    db.exec("insert into paid (who) values ('pays')")
    "paid"
}})
route GET /rows = len(db.query("select * from paid"))
"#, target)).unwrap();
    f
}

fn holds_up(name: &str, target: &str) {
    let f = app(name, target);
    let s = serve(&f, &[]);
    let key = |k: &'static str| [("idempotency-key", k)];
    let rows = || send(s.port, "GET", "/rows", &[], "").body;
    // The first thing this database sees of `idempotent`, its table not there yet.
    let r = send(s.port, "POST", "/fails", &key("first"), "");
    assert_eq!(r.status, 500, "{}: {}", name, r.body);
    assert_eq!(rows(), "0", "{}: a failure's work is rolled back", name);
    // A failure's answer is not kept: the same key runs again.
    send(s.port, "POST", "/fails", &key("first"), "");
    assert_eq!(rows(), "0", "{}", name);
    // Stopped by a runtime error, the same.
    assert_eq!(send(s.port, "POST", "/crashes", &key("second"), "").status, 500, "{}", name);
    assert_eq!(rows(), "0", "{}", name);
    // Done once however often it is asked.
    for _ in 0..3 {
        let r = send(s.port, "POST", "/pays", &key("third"), "");
        assert_eq!((r.status, r.body.as_str()), (200, "paid"), "{}", name);
    }
    assert_eq!(rows(), "1", "{}: one payment for one key", name);
    // Another key is another payment.
    assert_eq!(send(s.port, "POST", "/pays", &key("fourth"), "").body, "paid", "{}", name);
    assert_eq!(rows(), "2", "{}", name);
    // Asked at once by many, one runs.
    let port = s.port;
    let all: Vec<_> = (0..8).map(|_| std::thread::spawn(move || send(port, "POST", "/pays", &[("idempotency-key", "fifth")], "").status)).collect();
    let statuses: Vec<u16> = all.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(rows(), "3", "{}: {:?}", name, statuses);
}

#[test]
fn idempotent_holds_up_on_sqlite() {
    holds_up("sqlite", "app.db");
}

#[test]
fn idempotent_holds_up_on_postgres() {
    let Some(pg) = support::postgres("idempotent") else { return };
    holds_up("postgres", &pg.url);
}

#[test]
fn idempotent_holds_up_on_mariadb() {
    let Some(m) = support::mariadb("idempotent") else { return };
    holds_up("mariadb", &m.url("idem"));
}
