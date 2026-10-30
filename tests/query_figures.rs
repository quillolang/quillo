//! `/_quillo/health/queries`: how long queries take, recorded while asked for,
//! each statement shown only to whoever has the health token.

mod support;

use quillo::querystats::{has_token, record, report, set, WINDOW};
use support::{send, serve, temp_dir};

const TOKEN: &str = "a health token long enough";

/// One test, as the figures are the process's.
#[test]
fn figures_are_of_the_last_queries_and_statements_are_for_the_token_holder() {
    // Off: nothing is kept, and it says how to turn it on.
    let off = report(true);
    assert_eq!(off["enabled"], false);
    assert!(off["how"].as_str().unwrap().contains("--query-stats"));
    record("select 1", 5.0, true);
    set(true);
    let none = report(true);
    assert_eq!(none["queries"]["count"], 0);
    assert_eq!(none["queries"]["min_ms"], serde_json::Value::Null);
    assert_eq!(none["statements"].as_array().unwrap().len(), 0);

    for i in 0..(WINDOW + 20) {
        record("select  *\n  from notes\twhere id = ?", i as f64, true);
    }
    record("delete from notes", 1000.25, false);
    // Quillo's own tables are not counted.
    record("select * from _quillo_jobs", 1.0, true);
    let r = report(true);
    assert_eq!(r["queries"]["count"], WINDOW);
    assert_eq!(r["queries"]["total"], WINDOW + 21);
    assert_eq!(r["queries"]["errors"], 1);
    assert_eq!(r["queries"]["max_ms"], 1000.25);
    assert_eq!(r["queries"]["min_ms"], 21.0);
    let list = r["statements"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["sql"], "delete from notes");
    assert_eq!(list[0]["errors"], 1);
    assert_eq!(list[1]["sql"], "select * from notes where id = ?");
    assert_eq!(list[1]["total"], WINDOW + 20);
    assert_eq!(list[1]["count"], WINDOW);
    assert!(report(false)["statements"].as_str().unwrap().starts_with("hidden"));

    // A long statement is kept to 300 characters.
    record(&format!("select '{}'", "x".repeat(400)), 2000.0, true);
    assert_eq!(report(true)["statements"][0]["sql"].as_str().unwrap().len(), 300);

    // At most 500 statements: the one used longest ago makes room.
    set(true);
    record("the first", 1.0, true);
    for i in 0..499 {
        record(&format!("statement {}", i), 1.0, true);
    }
    record("the first", 1.0, true);
    record("one more", 1.0, true);
    // Only the slowest 100 are shown: make the ones asked about the slowest.
    record("the first", 100_000.0, true);
    record("statement 1", 100_000.0, true);
    let shown: Vec<String> = report(true)["statements"].as_array().unwrap().iter().map(|s| s["sql"].as_str().unwrap().to_string()).collect();
    assert_eq!(shown.len(), 100);
    assert!(shown.contains(&"the first".to_string()), "{:?}", &shown[..3]);
    assert_eq!(report(true)["queries"]["total"], 504);

    // Changing it forgets what there was.
    set(false);
    set(true);
    assert_eq!(report(true)["queries"]["total"], 0);
    set(false);

    // The token: long enough, sent as a bearer token, exactly.
    std::env::remove_var("QUILLO_HEALTH_TOKEN");
    assert!(!has_token(Some("Bearer anything")));
    std::env::set_var("QUILLO_HEALTH_TOKEN", "short");
    assert!(!has_token(Some("Bearer short")));
    std::env::set_var("QUILLO_HEALTH_TOKEN", TOKEN);
    assert!(has_token(Some(&format!("Bearer {}", TOKEN))));
    assert!(has_token(Some(&format!("Bearer  {} ", TOKEN))));
    assert!(!has_token(None));
    assert!(!has_token(Some(TOKEN)));
    assert!(!has_token(Some("Bearer a health token long enougH")));
    assert!(!has_token(Some("Bearer a health token")));
    std::env::remove_var("QUILLO_HEALTH_TOKEN");
}

#[test]
fn a_running_server_is_switched_on_and_off_with_the_token() {
    let d = temp_dir("query-figures");
    let f = d.join("app.qwe");
    std::fs::write(&f, "db.open(\"app.db\")\ndb.exec(\"create table if not exists t (a int)\")\nroute GET / = str(db.one(\"select count(*) as c from t\").c)\n").unwrap();
    let srv = serve(&f, &[("QUILLO_HEALTH_TOKEN", TOKEN)]);
    let address = "/_quillo/health/queries";
    let auth = format!("Bearer {}", TOKEN);
    let with_token = [("Authorization", auth.as_str())];
    let json = |a: support::Answer| serde_json::from_str::<serde_json::Value>(&a.body).unwrap();

    let a = send(srv.port, "GET", address, &[], "");
    assert_eq!((a.status, a.header("cache-control")), (200, Some("no-store")));
    assert_eq!(json(a)["enabled"], false);
    // Only with the token.
    let a = send(srv.port, "POST", address, &[], "");
    assert_eq!(a.status, 403);
    assert!(a.body.contains("QUILLO_HEALTH_TOKEN"), "{}", a.body);

    assert_eq!(json(send(srv.port, "POST", &format!("{}?on=1", address), &with_token, ""))["enabled"], true);
    assert_eq!(send(srv.port, "GET", "/", &[], "").body, "0");
    let seen = json(send(srv.port, "GET", address, &with_token, ""));
    assert_eq!(seen["queries"]["count"], 1);
    assert_eq!(seen["statements"][0]["sql"], "select count(*) as c from t");
    assert!(json(send(srv.port, "GET", address, &[], ""))["statements"].is_string());

    for off in ["on=0", "on=false", "off"] {
        assert_eq!(json(send(srv.port, "POST", &format!("{}?{}", address, off), &with_token, ""))["enabled"], false);
        assert_eq!(json(send(srv.port, "POST", address, &with_token, ""))["enabled"], true);
    }
    let _ = std::fs::remove_dir_all(&d);
}
