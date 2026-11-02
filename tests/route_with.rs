//! Routes and route groups take `with`, as functions do: what the route's body, and its
//! groups' middleware, may do (branch feedback40-43).

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

fn send(it: &mut Interp, method: &str, url: &str) -> HttpResponse {
    it.handle(&HttpRequest {
        method: method.into(),
        url: url.into(),
        headers: vec![("Host".into(), "localhost".into()), ("Origin".into(), "http://localhost".into()), ("Content-Type".into(), "application/json".into())],
        body: br#"{"text":"from json"}"#.to_vec(),
        remote: "127.0.0.1".into(),
    })
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

const APP: &str = r#"db.open(":memory:")
db.exec("create table notes (text text)")
db.exec("insert into notes values ('hi')")

route GET /notes with db = json.stringify(db.query("select text from notes"))
route GET /leak with db = str(now())
route POST /notes ? { text: str } with db {
    db.exec("insert into notes values (?)", text)
    "saved"
}
route POST /json { json: { text: str } } with db, time = { text, at: now() > 0 }.text

fn stamp(r) = now() > 0 ? r : reply 500 "no clock"
fn peek(r) = db.one("select 1 as x") != null ? r : reply 500 "no db"

routes /admin | stamp with db, time {
    route GET /count = str(len(db.query("select * from notes")))
    route GET /clock with time = str(now() > 0)
    route GET /peek with time = str(len(db.query("select * from notes")))
}
routes /quiet | peek with time {
    route GET /x = "x"
}
route GET /free = str(now() > 0)
"#;

#[test]
fn the_check_holds_a_route_to_its_with() {
    assert_eq!(
        diags(APP),
        [
            "6 `now` needs the time effect, but `route GET /leak` is declared `with db`",
            "19 `db.query` needs the db effect, but `route GET /admin/peek` is declared `with time`",
            "21 warning: `peek` may use the db effect, but `route GET /quiet/x` is declared `with time`",
        ]
    );
}

#[test]
fn a_route_runs_within_its_with() {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(APP, "t.qwe").unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/notes")), r#"[{"text":"hi"}]"#);
    // The body pattern, then `with`, then the body.
    assert_eq!(text(&send(&mut it, "POST", "/json")), "from json");
    assert_eq!(text(&send(&mut it, "POST", "/notes?text=yo")), "saved");
    assert_eq!(text(&send(&mut it, "GET", "/admin/count")), "2");
    assert_eq!(text(&send(&mut it, "GET", "/admin/clock")), "true");
    assert_eq!(text(&send(&mut it, "GET", "/free")), "true");
    // Refused: what it does not allow, in its body or in its group's middleware.
    for path in ["/leak", "/admin/peek", "/quiet/x"] {
        let r = send(&mut it, "GET", path);
        assert_eq!(r.status, 500, "{}: {}", path, text(&r));
    }
    // A request after a refused one runs with what it allows, not what the last one did.
    assert_eq!(text(&send(&mut it, "GET", "/free")), "true");
    assert_eq!(text(&send(&mut it, "GET", "/notes")), r#"[{"text":"hi"},{"text":"yo"}]"#);
}

#[test]
fn a_group_narrows_and_never_widens() {
    assert_eq!(
        diags("routes /a with db {\n    route GET /x with http = \"x\"\n}\n"),
        ["2 this route asks for `http`, which its group does not allow (`with db`)"]
    );
    assert_eq!(
        diags("routes /a with db {\n    routes /b with http {\n        route GET /x = \"x\"\n    }\n}\n"),
        ["2 this group asks for `http`, which the group around it does not allow (`with db`)"]
    );
    // A group inside one takes the outer one's, and may narrow it.
    assert_eq!(
        diags("routes /a with db, time {\n    routes /b with time {\n        route GET /x = str(len(db.query(\"select 1\")))\n    }\n    route GET /y = str(now())\n}\n"),
        ["3 `db.query` needs the db effect, but `route GET /a/b/x` is declared `with time`"]
    );
    assert_eq!(
        diags("route GET /x with disk = \"x\"\n"),
        ["1 expected an effect after `with` (db, http, fs, env, time, random, net), found `disk`"]
    );
}

#[test]
fn a_policy_and_a_route_both_hold() {
    // Under a policy without `time`, a route `with db, time` still may not use time.
    let d = std::env::temp_dir().join(format!("quillo-route-with-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("main.qwe"), "route GET / with db, time = str(now())\n").unwrap();
    std::fs::write(d.join("p.toml"), "[effects]\nallow = [\"db\"]\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe", "--policy", "p.toml"]).current_dir(&d).output().unwrap();
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{}", said);
    // The policy took time away, so the message names the policy, not the route's `with`.
    assert!(said.contains("main.qwe:1: error: `now` needs the time effect, which"), "{}", said);
    assert!(!said.contains("is declared"), "{}", said);
}

#[test]
fn what_an_answer_runs_after_the_body_is_held_too() {
    // A stream's function and a deferred part run once the body has returned: still
    // under the route's `with`.
    let src = r#"fn tick(send) = send(str(now() > 0))
fn clock() = `clock ${now() > 0}`
route GET /s with db = stream(tick)
route GET /s2 with time = stream(tick)
component Late() {
    state at = defer(clock())
    <p>{at}</p>
}
route GET /d with db = <html><body><Late/></body></html>
route GET /d2 with time = <html><body><Late/></body></html>
"#;
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(src, "t.qwe").unwrap();
    assert_eq!(send(&mut it, "GET", "/s").status, 500);
    assert_eq!(text(&send(&mut it, "GET", "/s2")), "true");
    assert!(!text(&send(&mut it, "GET", "/d")).contains("clock true"));
    assert!(text(&send(&mut it, "GET", "/d2")).contains("clock true"));
    // The next request is not held by the last one's.
    assert_eq!(text(&send(&mut it, "GET", "/s2")), "true");
}

#[test]
fn a_group_at_the_root_takes_with() {
    let src = "routes / with time {\n    route GET /x = str(now() > 0)\n    route GET /y = str(len(db.query(\"select 1\")))\n}\n";
    assert_eq!(diags(src), ["3 `db.query` needs the db effect, but `route GET /y` is declared `with time`"]);
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source(src, "t.qwe").unwrap();
    assert_eq!(text(&send(&mut it, "GET", "/x")), "true");
}
