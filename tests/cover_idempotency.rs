//! `idempotent(f)` where no other test takes it (src/idempotency.rs): in a service's function
//! called as a step, failing or answering with a page.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    quillo::server::SHOW_ERRORS.store(true, Relaxed);
    let d = support::temp_dir(&format!("cover-idempotency-{}", name)).canonicalize().unwrap();
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn app(d: &PathBuf, main: &str) -> Interp {
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join(main)).unwrap_or_else(|e| panic!("{}", e));
    it
}

fn request(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: Vec::new(), remote: "127.0.0.1".into() })
}

const WORK: &str = r#"
db.open("work.db")
db.exec("create table if not exists runs (what text)")
let broken = true
server fn mend() { broken = false }
server fn step() = idempotent({ transaction: false }, () => {
    if broken {
        json.parse("[]").no.such
    }
    db.exec("insert into runs values ('step')")
    "stepped"
})
server fn page() = idempotent(() => redirect("/elsewhere"))
server fn plain_page() = idempotent({ transaction: false }, () => redirect("/elsewhere"))
server fn runs() = db.one("select count(*) as c from runs").c
"#;

const FRONT: &str = r#"
service work = "work.qwe"
route GET /step = work.step()
route GET /mend = work.mend()
route GET /page = work.page()
route GET /plain-page = work.plain_page()
route GET /runs = str(work.runs())
"#;

/// A step whose idempotent work stops with an error fails, its claim given back and nothing
/// kept as its answer: called again once mended, it runs, and its work is done once.
#[test]
fn a_step_whose_work_stops_with_an_error_fails_and_keeps_no_answer() {
    let d = folder("step-error", &[("work.qwe", WORK), ("front.qwe", FRONT)]);
    let mut it = app(&d, "front.qwe");
    let r = request(&mut it, "GET", "/step", &[]);
    assert_eq!(r.status, 500, "{}", r.text());
    assert_eq!(request(&mut it, "GET", "/runs", &[]).text(), "0");
    request(&mut it, "GET", "/mend", &[]);
    let r = request(&mut it, "GET", "/step", &[]);
    assert_eq!((r.status, r.text().as_str()), (200, "stepped"));
    assert_eq!(request(&mut it, "GET", "/runs", &[]).text(), "1");
}

/// A service's function gives a value: one whose idempotent work answers with a page fails
/// (the log says "a service's function gives a value, not a page"; the visitor is told only of
/// an internal error), in a transaction or not, and its redirect goes nowhere.
#[test]
fn a_steps_idempotent_work_answering_with_a_page_is_refused() {
    let d = folder("step-page", &[("work.qwe", WORK), ("front.qwe", FRONT)]);
    let mut it = app(&d, "front.qwe");
    for path in ["/page", "/plain-page"] {
        let r = request(&mut it, "GET", path, &[]);
        assert_eq!(r.status, 500, "{}: {}", path, r.text());
        let name = path.trim_start_matches('/').replace('-', "_");
        assert!(r.text().contains(&format!("internal error in `{}`", name)), "{}: {}", path, r.text());
        assert_eq!(header(&r, "location"), None, "{}", path);
    }
}

fn header<'a>(r: &'a HttpResponse, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}
