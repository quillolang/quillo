//! `build "/path" = expr` as its route runs it, by hand: a failure kept until a good value
//! comes, the last good value served after a failure, and each kind of value as its answer.
//! Errors are shown here, as `--dev` shows them, so a first failure does not stop the process.

use quillo::Interp;

fn loaded(src: &str) -> Interp {
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src));
    it
}

fn value(src: &str) -> String {
    let mut it = Interp::new(".");
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    match it.load_source(src, "case.qwe") {
        Ok(v) => v.repr(),
        Err(e) => format!("error {}: {}", e.line, e.msg),
    }
}

#[test]
fn a_build_that_fails_gives_its_error_each_time_it_is_asked() {
    // An error value, and an error raised.
    let src = "fn f() = __built(\"/cover-fail.txt\", () => err(\"no tools\"))\n[f(), f()]";
    assert_eq!(value(src), "[err(\"no tools\"), err(\"no tools\")]");
    let src = "fn f() = __built(\"/cover-raise.txt\", () => 1 / 0)\n[f(), f()]";
    assert_eq!(value(src), "[err(\"division by zero\"), err(\"division by zero\")]");
}

#[test]
fn a_build_that_fails_after_a_good_one_serves_the_good_one() {
    let it = loaded("fn good() = __built(\"/cover-kept.txt\", () => \"made\")\nfn bad() = __built(\"/cover-kept.txt\", () => err(\"broken\"))\n");
    let mut it = it;
    let src = "let a = good()\nlet b = bad()\nlet c = bad()\n[a.status, str(a.body), b.status, str(b.body), c.status, str(c.body)]";
    assert_eq!(it.load_source(src, "more.qwe").unwrap().repr(), "[200, \"made\", 200, \"made\", 200, \"made\"]");
}

#[test]
fn a_build_that_replies_answers_with_that_reply() {
    assert_eq!(value("__built(\"/cover-reply.txt\", () => { reply 404 })"), "error 0: `reply` used outside of a route");
}

#[test]
fn a_build_s_value_is_answered_by_its_kind() {
    let src = "let r = __built(\"/cover-page.html\", () => <p>hi</p>)\nlet j = __built(\"/cover-data.json\", () => [1, 2])\nlet x = __built(\"/cover-moved\", () => redirect(\"/there\"))\n[r.status, r.headers[\"content-type\"], str(r.body), j.headers[\"content-type\"], str(j.body), x.status, x.headers[\"location\"]]";
    assert_eq!(value(src), "[200, \"text/html; charset=utf-8\", \"<p>hi</p>\", \"application/json\", \"[1,2]\", 303, \"/there\"]");
}
