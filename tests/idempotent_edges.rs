//! `idempotent(f)` at its edges: its options, what is kept and given again,
//! and what happens when the claim on a key is lost.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

fn folder(name: &str) -> PathBuf {
    quillo::server::SHOW_ERRORS.store(true, Relaxed);
    support::temp_dir(&format!("idempotent-{}", name))
}

fn app(name: &str, src: &str) -> (Interp, PathBuf) {
    let d = folder(name);
    std::fs::write(d.join("app.qwe"), src).unwrap();
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    (it, d)
}

fn request(it: &mut Interp, url: &str, headers: &[(&str, &str)], body: &[u8]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "POST".into(), url: url.into(), headers: h, body: body.to_vec(), remote: "127.0.0.1".into() })
}

fn post(it: &mut Interp, url: &str, key: &str) -> HttpResponse {
    request(it, url, &[("Idempotency-Key", key)], b"")
}

fn says(r: HttpResponse, status: u16, text: &str) {
    assert!(r.status == status && r.text().contains(text), "{}: {}", r.status, r.text());
}

const WITH_DB: &str = "db.open(\"app.db\")\ndb.exec(\"create table if not exists runs (what text)\")\nfn ran() = db.one(\"select count(*) as c from runs\").c\n";

#[test]
fn its_options_are_checked() {
    let (mut it, _d) = app(
        "options",
        &format!(
            r#"{}
route POST /not-a-map = idempotent(3, () => 1)
route POST /unknown = idempotent({{ colour: "red" }}, () => 1)
route POST /age = idempotent({{ max_age: "soon" }}, () => 1)
route POST /zero-age = idempotent({{ max_age: 0 }}, () => 1)
route POST /retries = idempotent({{ transaction: false, retries: 2 }}, () => 1)
route POST /isolation = idempotent({{ transaction: false, isolation: "serializable" }}, () => 1)
route POST /transaction = idempotent({{ transaction: "yes" }}, () => 1)
route POST /no-function = idempotent(1)
route POST /too-many = idempotent({{}}, () => 1, 2)
route POST /nested = db.transaction(() => idempotent(() => 1))
route POST /fine = idempotent({{ transaction: true, max_age: "1h" }}, () => "ran")
"#,
            WITH_DB
        ),
    );
    says(post(&mut it, "/not-a-map", "k"), 500, "`idempotent` expects options as a map, got int");
    says(post(&mut it, "/unknown", "k"), 500, "`idempotent` has no option `colour`");
    says(post(&mut it, "/age", "k"), 500, "max_age is seconds or a duration");
    says(post(&mut it, "/zero-age", "k"), 500, "max_age is seconds or a duration");
    says(post(&mut it, "/retries", "k"), 500, "takes no isolation or retries");
    says(post(&mut it, "/isolation", "k"), 500, "takes no isolation or retries");
    says(post(&mut it, "/transaction", "k"), 500, "`idempotent`'s transaction is true or false, got \"yes\"");
    says(post(&mut it, "/no-function", "k"), 500, "`idempotent` expects a function");
    says(post(&mut it, "/too-many", "k"), 500, "`idempotent` expects a function");
    says(post(&mut it, "/nested", "k"), 500, "call it outside `db.transaction`");
    says(post(&mut it, "/fine", "k"), 200, "ran");
}

#[test]
fn outside_a_request_or_without_a_key_it_simply_runs() {
    let (mut it, _d) = app(
        "plain",
        &format!(
            "{}\nlet at_start = idempotent(() => \"ran at the start\")\nroute POST /start = at_start\nroute POST /plain = idempotent({{ transaction: false }}, () => {{ db.exec(\"insert into runs values ('x')\"); ran() }})\n",
            WITH_DB
        ),
    );
    says(post(&mut it, "/start", "k"), 200, "ran at the start");
    says(request(&mut it, "/plain", &[], b""), 200, "1");
    says(request(&mut it, "/plain", &[], b""), 200, "2");
}

#[test]
fn a_table_that_cannot_keep_keys_is_said_so() {
    let (mut it, _d) = app("bad-table", "db.open(\"app.db\")\ndb.exec(\"create table _quillo_idempotency (x int)\")\nroute POST /x = idempotent({ transaction: false }, () => \"ran\")\n");
    says(post(&mut it, "/x", "k"), 503, "could not record the Idempotency-Key");
}

#[test]
fn a_key_that_cannot_be_one_is_refused() {
    let (mut it, _d) = app("bad-key", &format!("{}\nroute POST /x = idempotent(() => \"ran\")\n", WITH_DB));
    says(post(&mut it, "/x", ""), 400, "an Idempotency-Key is 1 to 255 visible ASCII characters");
    says(post(&mut it, "/x", "has space"), 400, "visible ASCII");
    says(post(&mut it, "/x", &"k".repeat(256)), 400, "visible ASCII");
    says(post(&mut it, "/x", &"k".repeat(255)), 200, "ran");
}

#[test]
fn a_binary_answer_is_given_again_as_it_was() {
    let (mut it, d) = app("binary", &format!("{}\nroute POST /x = idempotent(() => {{ db.exec(\"insert into runs values ('x')\"); file(\"data.bin\") }})\n", WITH_DB));
    let bytes = [0xff, 0xfe, 0x00, 0x41];
    std::fs::write(d.join("data.bin"), bytes).unwrap();
    let first = post(&mut it, "/x", "k");
    assert_eq!((first.status, first.body.as_slice()), (200, &bytes[..]));
    let again = post(&mut it, "/x", "k");
    assert_eq!((again.status, again.body.as_slice()), (200, &bytes[..]));
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
}

#[test]
fn a_streamed_answer_cannot_be_kept() {
    let (mut it, _d) = app(
        "stream",
        &format!(
            "{}\nroute POST /tx = idempotent(() => stream((send) => send(\"x\")))\nroute POST /plain = idempotent({{ transaction: false }}, () => stream((send) => send(\"x\")))\nroute POST /early = idempotent({{ transaction: false }}, () => {{ return stream((send) => send(\"x\")) }})\n",
            WITH_DB
        ),
    );
    says(post(&mut it, "/tx", "a"), 500, "`idempotent` cannot keep a streamed answer");
    says(post(&mut it, "/plain", "b"), 500, "`idempotent` cannot keep a streamed answer");
    says(post(&mut it, "/early", "c"), 500, "`idempotent` cannot keep a streamed answer");
    // The claim was given up: the key can be used by another request.
    let (mut other, _d2) = app("stream-after", &format!("{}\nroute POST /plain = idempotent({{ transaction: false }}, () => \"fine\")\n", WITH_DB));
    says(post(&mut other, "/plain", "b"), 200, "fine");
}

#[test]
fn every_answer_is_kept_without_a_transaction_even_a_failure() {
    let (mut it, _d) = app(
        "no-tx",
        &format!(
            r#"{}
route POST /fails = idempotent({{ transaction: false }}, () => {{ db.exec("insert into runs values ('f')"); missing_table() }})
fn missing_table() = db.query("select * from nothing_here").x.y.z
route POST /count = str(ran())
"#,
            WITH_DB
        ),
    );
    let first = post(&mut it, "/fails", "k");
    assert_eq!(first.status, 500);
    let again = post(&mut it, "/fails", "k");
    assert_eq!(again.status, 500);
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
    says(request(&mut it, "/count", &[], b""), 200, "1");
}

#[test]
fn a_claim_lost_without_a_transaction_still_answers() {
    let (mut it, _d) = app(
        "gone",
        &format!("{}\nroute POST /x = idempotent({{ transaction: false }}, () => {{ db.exec(\"delete from _quillo_idempotency\"); \"answered\" }})\n", WITH_DB),
    );
    says(post(&mut it, "/x", "k"), 200, "answered");
    // Nothing was kept.
    let r = post(&mut it, "/x", "k");
    assert_eq!(r.header("idempotent-replayed"), None);
}

#[test]
fn a_claim_taken_over_in_a_transaction_is_not_committed() {
    let (mut it, _d) = app(
        "lost",
        &format!(
            r#"{}
route POST /x = idempotent(() => {{
    db.exec("insert into runs values ('x')")
    db.exec("update _quillo_idempotency set token = 'someone else'")
    "done"
}})
route POST /count = str(ran())
"#,
            WITH_DB
        ),
    );
    says(post(&mut it, "/x", "k"), 409, "was running at the same time; try again");
    // The work went with it.
    says(request(&mut it, "/count", &[], b""), 200, "0");
}

/// The table as Quillo makes it, for a test to put a trigger on.
const TABLE: &str = "db.open(\"app.db\")\ndb.exec(\"create table if not exists runs (what text)\")\nfn ran() = db.one(\"select count(*) as c from runs\").c\ndb.exec(\"create table _quillo_idempotency (id varchar(64) primary key, path text not null, fingerprint varchar(64) not null, token varchar(32) not null, done int not null default 0, lease_until bigint not null, expires_at bigint not null, status int, headers text, body text, body_b64 int)\")\n";

#[test]
fn a_failure_to_keep_an_answer_rolls_the_work_back() {
    let (mut it, _d) = app(
        "store-fails",
        &format!(
            r#"{}
db.exec("create trigger no_answers before update of done on _quillo_idempotency begin select raise(abort, 'not today'); end")
route POST /x = idempotent(() => {{ db.exec("insert into runs values ('x')"); "done" }})
route POST /no-tx = idempotent({{ transaction: false }}, () => {{ db.exec("insert into runs values ('y')"); "answered" }})
route POST /count = str(ran())
"#,
            TABLE
        ),
    );
    let r = post(&mut it, "/x", "k");
    assert!(r.status >= 500, "{}: {}", r.status, r.text());
    says(request(&mut it, "/count", &[], b""), 200, "0");
    // Without a transaction the work is done, and answered.
    says(post(&mut it, "/no-tx", "l"), 200, "answered");
    says(request(&mut it, "/count", &[], b""), 200, "1");
}

#[test]
fn a_claim_that_cannot_be_given_up_is_taken_over_in_time() {
    let (mut it, _d) = app(
        "release-fails",
        &format!(
            r#"{}
db.exec("create trigger no_release before delete on _quillo_idempotency begin select raise(abort, 'not today'); end")
route POST /x = idempotent(() => {{ reply 503 "try again" }})
"#,
            TABLE
        ),
    );
    says(post(&mut it, "/x", "k"), 503, "try again");
    // The claim stayed, for its lease.
    says(post(&mut it, "/x", "k"), 409, "still running");
}

#[test]
fn a_key_that_keeps_slipping_away_is_said_to_be_running() {
    let (mut it, _d) = app(
        "take-over",
        &format!(
            r#"{}
db.exec("create trigger no_take_over before update of token on _quillo_idempotency begin select raise(ignore); end")
route POST /x = idempotent(() => "ran")
route POST /stale {{ db.exec("update _quillo_idempotency set done = 0, lease_until = 1"); "stale" }}
"#,
            TABLE
        ),
    );
    says(post(&mut it, "/x", "k"), 200, "ran");
    says(request(&mut it, "/stale", &[], b""), 200, "stale");
    says(post(&mut it, "/x", "k"), 409, "still running");
}

#[test]
fn a_server_fn_answers_as_server_fns_do() {
    let (mut it, _d) = app(
        "rpc",
        &format!(
            r#"{}
server fn pay() = idempotent({{ transaction: false }}, () => "paid")
server fn refuses() = idempotent({{ transaction: false }}, () => err("no"))
server fn fails() = idempotent({{ transaction: false }}, () => nothing_here())
server fn answers() = idempotent({{ transaction: false }}, () => respond(201, "made"))
server fn replies() = idempotent({{ transaction: false }}, () => {{ reply 202 "early" }})
fn nothing_here() = db.query("select * from none").a.b
"#,
            WITH_DB
        ),
    );
    let call = |it: &mut Interp, name: &str, key: &str| request(it, &format!("/_quillo/rpc/{}", name), &[("Content-Type", "application/json"), ("Idempotency-Key", key)], b"[]");
    says(call(&mut it, "pay", "a"), 200, r#"{"ok":"paid"}"#);
    says(call(&mut it, "refuses", "b"), 200, r#"{"err":"#);
    says(call(&mut it, "fails", "c"), 500, "internal error in `fails`");
    says(call(&mut it, "answers", "d"), 201, "made");
    says(call(&mut it, "replies", "e"), 202, "early");
    // Each is given again.
    let again = call(&mut it, "pay", "a");
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
    assert_eq!(again.text(), r#"{"ok":"paid"}"#);
}

#[test]
fn a_form_with_files_is_the_same_request_with_another_boundary() {
    let (mut it, _d) = app("multipart", &format!("{}\nroute POST /up = idempotent(() => {{ db.exec(\"insert into runs values ('u')\"); str(ran()) }})\n", WITH_DB));
    let form = |b: &str, text: &str| format!("--{b}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a.txt\"\r\n\r\n{text}\r\n--{b}--\r\n");
    let upload = |it: &mut Interp, b: &str, text: &str| {
        let ctype = format!("multipart/form-data; boundary={}", b);
        request(it, "/up", &[("Content-Type", &ctype), ("Idempotency-Key", "k")], form(b, text).as_bytes())
    };
    says(upload(&mut it, "first", "hello"), 200, "1");
    let again = upload(&mut it, "second", "hello");
    assert_eq!(again.text(), "1");
    assert_eq!(again.header("idempotent-replayed"), Some("true"));
    says(upload(&mut it, "third", "other"), 422, "used with another request");
}

#[test]
fn an_answer_past_its_time_is_forgotten_and_the_request_runs_again() {
    let (mut it, _d) = app(
        "expired",
        &format!(
            r#"{}
route POST /x = idempotent(() => {{ db.exec("insert into runs values ('x')"); str(ran()) }})
route POST /age = {{ db.exec("update _quillo_idempotency set expires_at = 1"); "aged" }}
route POST /stale = {{ db.exec("update _quillo_idempotency set done = 0, lease_until = 1"); "stale" }}
route POST /running = {{ db.exec("update _quillo_idempotency set done = 0"); "running" }}
route POST /no-tx = idempotent({{ transaction: false }}, () => "ran")
"#,
            WITH_DB
        ),
    );
    says(post(&mut it, "/x", "k"), 200, "1");
    assert_eq!(post(&mut it, "/x", "k").header("idempotent-replayed"), Some("true"));
    says(request(&mut it, "/age", &[], b""), 200, "aged");
    let r = post(&mut it, "/x", "k");
    assert_eq!((r.text().as_str(), r.header("idempotent-replayed")), ("2", None));
    // A first request that stopped part way: a transaction took it back, so
    // the next takes the key over; it is still running while its lease lasts.
    says(request(&mut it, "/running", &[], b""), 200, "running");
    says(post(&mut it, "/x", "k"), 409, "still running");
    says(request(&mut it, "/stale", &[], b""), 200, "stale");
    says(post(&mut it, "/x", "k"), 200, "3");
    // Without a transaction it is not known whether the work happened.
    says(post(&mut it, "/no-tx", "n"), 200, "ran");
    says(request(&mut it, "/stale", &[], b""), 200, "stale");
    says(post(&mut it, "/no-tx", "n"), 409, "whether it took effect is not known");
}

/// A second connection commits while the first request runs, as a request
/// that took the claim over would. PostgreSQL only: SQLite has one writer.
#[test]
fn a_request_that_lost_its_claim_gets_the_answer_of_the_one_that_took_it() {
    let Some(pg) = support::postgres("idem-edges") else { return };
    let url = pg.database("idem");
    let (mut it, _d) = app(
        "pg-lost",
        &format!(
            r#"
db.open("{url}")
let other = db.connect("{url}")
db.exec("create table if not exists runs (what text)")
route POST /answer = idempotent(() => {{
    db.exec("insert into runs values ('x')")
    other.exec("update _quillo_idempotency set token = 'theirs', done = 1, status = 201, headers = '[]', body = 'their answer', body_b64 = 0")
    "mine"
}})
route POST /refused = idempotent(() => {{
    other.exec("update _quillo_idempotency set token = 'theirs', fingerprint = 'another'")
    "mine"
}})
route POST /count = str(db.one("select count(*) as c from runs").c)
"#
        ),
    );
    let r = post(&mut it, "/answer", "k");
    assert_eq!((r.status, r.text().as_str()), (201, "their answer"));
    assert_eq!(r.header("idempotent-replayed"), Some("true"));
    says(request(&mut it, "/count", &[], b""), 200, "0");
    says(post(&mut it, "/refused", "l"), 422, "used with another request");
}
