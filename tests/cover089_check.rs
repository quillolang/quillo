//! Coverage of 0.8.9 (branch feedback75): the lines of the checker, `run_script`, translations,
//! the bundle's guard, `files`, `image.info` and `quillo run`'s options added for FEEDBACK75 and
//! FEEDBACK78 that no test ran. A confined check's budget read at each of its clocks (Q-1053); a
//! stream's `send` bound again or a function handed to the transaction (Q-1067); a `*` stream's
//! rule the checker cannot read (Q-1050); SQL a name holds as other values, piped to `replace`,
//! and helpers found through a cycle of calls or with fields set, guards and markup (Q-1085); a
//! name for a function named again or handed on (Q-1084); "did you mean" with names not ASCII
//! (Q-1120); a script's steps for maps and errors it reads (Q-1055); a translation's attributes
//! written with spaces, bare, with braces and comments, its character references and the cache
//! of its reading (Q-1083); the page states of the whole program's bundle (Q-1058); a local
//! function in the bundle's guard (Q-1063); a link to the policy (Q-1088); a PNG profile's name
//! cut short (Q-1051); `--client-cert-header` without a header's name (FEEDBACK78.md, A.13).
//! Every server is one this test starts, on this machine.

mod support;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("cover089-check-{}", name))
}

/// A folder removed when the test ends, passed or not.
struct Gone(PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a check of `main.qwe` holding `src` in `d` says, as "line: severity: message", and how long
/// it took; `confined` as `check_program` checks a program's users' code, given `time` in place of
/// its 2 s.
fn check_timed(d: &Path, src: &str, confined: bool, time: Option<Duration>) -> (Vec<String>, Duration) {
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let opts = quillo::check::Options { confined, time, ..Default::default() };
    let t = Instant::now();
    let a = quillo::check::check_file_opts(&d.join("main.qwe"), HashMap::new(), opts);
    let took = t.elapsed();
    let said = a
        .diagnostics
        .iter()
        .map(|x| format!("{}: {}: {}", x.line, if x.severity == quillo::check::Severity::Error { "error" } else { "warning" }, x.message))
        .collect();
    (said, took)
}

/// What `quillo check`'s own check of `src` says.
fn check(name: &str, src: &str) -> Vec<String> {
    let d = dir(name);
    let _gone = Gone(d.clone());
    check_timed(&d, src, false, None).0
}

/// The SQL warning, as `check` writes it after the line.
const SQL_BUILT: &str = "warning: `db.query` with SQL built from values: a value containing a quote can change the query (SQL injection); write `?` in the SQL and pass the values after it";

/// The lines of the GET-that-writes warnings among `said`, each with what it says it writes.
fn get_writes(said: &[String]) -> Vec<String> {
    said.iter().filter(|l| l.contains(" and reads who the visitor is (")).map(|l| l.split(" and reads who").next().unwrap().to_string()).collect()
}

// ---- Q-1067: a stream's send inside a transaction --------------------------------------------

/// A `send` the stream's code binds again (a lambda's parameter of that name) is not the stream's,
/// and a transaction handed a function (`db.transaction(save)`) is not read for it: neither is
/// warned of; a `send` written in the transaction's own lambda still is (FEEDBACK75.md, Q-1067;
/// src/check.rs `stream_sends_in_transactions`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_send_bound_again_or_a_function_handed_to_the_transaction_is_not_warned() {
    let src = r#"db.open(":memory:")
db.exec("create table if not exists t (n int)")
fn save() {
    db.exec("insert into t (n) values (1)")
}
route GET /a.txt = stream((send) => {
    let rows = [1, 2].map((send) => db.transaction(() => send + 1))
    send(str(rows))
})
route GET /b.txt = stream((send) => {
    let r = db.transaction(save)
    send(str(r))
})
route GET /c.txt = stream((send) => {
    let r = db.transaction(() => {
        send("ünï")
    })
    send(str(r))
})
"#;
    assert_eq!(
        check("stream-send", src),
        vec!["16: warning: `send` inside `db.transaction` waits for the client to read: on SQLite the transaction holds every other request of the instance meanwhile, so Quillo keeps up to 4 MiB of what it sends, then ends the transaction (it rolls back) if the client has not read within a second. Read the rows in the transaction and send them after it".to_string()]
    );
}

// ---- Q-1050: a `*` stream's rule ---------------------------------------------------------------

/// A `*` stream's rule given as a map's field, an item or a call's value is one the checker cannot
/// read, and is not said; a lambda that never reads the name still is (FEEDBACK75.md, Q-1050;
/// src/check.rs `star_rules`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_star_rule_the_checker_cannot_read_is_not_said() {
    let src = r#"route GET /login ? { user: str } = set_session({ user: user })
let rules = { publish: (peer, name) => name == "cam:" + session().user, watch: (peer) => true }
fn pick(kind) {
    return (peer) => session().user != null
}
media.on("cam:*", { publish: rules.publish, watch: pick("w") })
media.on("mic:*", { publish: rules["watch"] })
media.on("tv:*", { publish: (peer) => session().user != null })
"#;
    let said = check("star", src);
    assert_eq!(said.len(), 1, "{:#?}", said);
    assert!(said[0].starts_with("8: warning: `media.on(\"tv:*\")`'s `publish` never reads which stream is asked for"), "{}", said[0]);
}

// ---- Q-1085: SQL built from values -------------------------------------------------------------

/// A name that holds built SQL in one route and other values in another is read as each holds it:
/// a list's item is warned of only where the list holds built SQL. Text piped to `replace` takes
/// what it puts in (a visitor's value is warned of, `sql_name`'s is not), and to `trim()` passes
/// through (FEEDBACK75.md, Q-1085; src/check.rs `parts_of`, `sql_built`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn sql_a_name_holds_as_other_values_and_piped_text_functions() {
    let src = r#"db.open(":memory:")
route GET /bad1 {
    let stmts = ["select 1", "select * from t where n = " + request.query.n]
    return db.query(stmts[1])
}
route GET /ok1 {
    let stmts = ["select 1", "select 2"]
    return db.query(stmts[1])
}
route GET /bad2 = db.query("select * from t where name = '{n}'" | replace("{n}", request.query.n))
route GET /ok2 = db.query("select * from t" | trim())
route GET /ok3 = db.query("select * from t where name = '{n}'" | replace("{n}", sql_name(request.query.n)))
route GET /ok4 = db.query("select * from t where name = 'ü'" | replace("ü", "é"))
"#;
    assert_eq!(check("sql-names", src), vec![format!("4: {}", SQL_BUILT), format!("10: {}", SQL_BUILT)]);
    // A text function given nothing is its call's error, and no SQL is said built of it.
    assert_eq!(check("sql-empty", "db.open(\":memory:\")\nroute GET /e = db.query(trim())\n"), vec!["2: error: missing argument `s` in call to `trim`: trim(s)".to_string()]);
}

/// A helper with an assignment that sets a page object's field, has a function of its own (with a
/// bare `return`), chooses by a match with a guard and makes markup is still read for what it gives back: `sql_name` text with a literal added is
/// safe, the same with the guard's arm giving a parameter is not (FEEDBACK75.md, Q-1085, found in
/// review; src/check.rs `graph_here`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_helper_that_sets_fields_guards_and_makes_markup_is_read() {
    let helper = |name: &str, arm: &str| {
        format!(
            "fn {name}(col, desc, el) {{\n    let c = sql_name(col)\n    el.title = c\n    fn shout(x) {{\n        print(x)\n        return\n    }}\n    shout(c)\n    let tag = match desc {{\n        d if d == true => {arm},\n        _ => \"\",\n    }}\n    let note = if desc {{\n        el.title = tag\n        <b>{{c}} ünï</b>\n    }} else {{\n        <i>{{tag}}</i>\n    }}\n    print(note)\n    if desc {{ c = c + tag }}\n    return c\n}}\n"
        )
    };
    let src = format!(
        "db.open(\":memory:\")\n{}{}route GET /ok = db.query(\"select * from t order by \" + order_by(request.query.c, true, null))\nroute GET /bad = db.query(\"select * from t order by \" + order_raw(request.query.c, true, null))\n",
        helper("order_by", "\" desc\""),
        helper("order_raw", "col")
    );
    assert_eq!(check("sql-graph", &src), vec![format!("47: {}", SQL_BUILT)]);
}

/// A helper whose SQL-safe answer comes through another that calls it back (a cycle) is found once
/// the other is: `note` is read before `table_of`, which it calls, and again after it, so a query
/// built with `note` is not warned of; one with the same cycle around a helper that gives what it
/// is given is (FEEDBACK75.md, Q-1053 and Q-1085, found in review; src/check.rs `Worklist::again`,
/// `gives_sql_safe`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_safe_helper_is_found_through_a_cycle_of_calls() {
    let src = r#"db.open(":memory:")
fn table_of(kind) {
    print(note(kind))
    return sql_name("t_" + kind)
}
fn note(kind) {
    if kind == "" {
        return table_of("x")
    }
    return table_of(kind)
}
fn raw_of(kind) {
    print(hint(kind))
    return kind
}
fn hint(kind) {
    if kind == "" {
        return raw_of("x")
    }
    return raw_of(kind)
}
route GET /ok = db.query("select * from " + note(request.query.k))
route GET /bad = db.query("select * from " + hint(request.query.k))
"#;
    assert_eq!(check("sql-cycle", src), vec![format!("23: {}", SQL_BUILT)]);
}

// ---- Q-1084: GET routes that write -------------------------------------------------------------

/// A name for a function named again (`let w2 = w`) is called as the function, and one handed to
/// a call (`.map(w)`) is called there: each route that writes so is warned of; through a name for a
/// function that writes nothing, or one assigned another since, none is (FEEDBACK75.md, Q-1084;
/// src/check.rs `GetWalk::block`, `GetWalk::call`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_name_for_a_function_named_again_or_handed_on_is_followed() {
    let src = r#"db.open(":memory:")
fn save_visit(u) { db.exec("insert into visits (user) values (?)", u) }
fn nothing(u) { return u }
route GET /a {
    let w = save_visit
    let w2 = w
    w2(session().user)
    return "ok"
}
route GET /b {
    let w = save_visit
    [session().user].map(w)
    return "ok"
}
route GET /c {
    let w = nothing
    let w2 = w
    w2(session().user)
    [session().user].map(w)
    return "ok"
}
route GET /d {
    let w = save_visit
    w = nothing
    [session().user].map(w)
    return "ok"
}
"#;
    let said = check("get-alias", src);
    let w = "writes (`db.exec(\"insert into visits (user) values (?)\")`, in `save_visit`)";
    assert_eq!(get_writes(&said), vec![format!("4: warning: `GET /a` {}", w), format!("10: warning: `GET /b` {}", w)], "{:#?}", said);
    assert_eq!(said.len(), 2, "{:#?}", said);
}

// ---- Q-1120: did you mean ----------------------------------------------------------------------

/// "Did you mean" passes over a name not all ASCII that is too short to be near (`é` for an
/// unknown name of 8 letters, `ääää` for one of 8), and one that is the unknown name in another
/// case (`größe` for `GRÖßE`, as `total` is for `TOTAL`); one near enough is offered
/// (FEEDBACK75.md, Q-1120, found in review; src/check.rs `closest_within`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn did_you_mean_passes_over_names_not_ascii_too_far_or_the_same() {
    assert_eq!(check("near-short", "let é = 1\nprint(é)\nprint(zzqqxxyy)\n"), vec!["3: error: unknown name `zzqqxxyy`".to_string()]);
    assert_eq!(
        check("near-far", "let ääää = 1\nprint(zqzqzqzq)\nprint(ääab)\n"),
        vec!["2: error: unknown name `zqzqzqzq`".to_string(), "3: error: unknown name `ääab` (did you mean `ääää`?)".to_string()]
    );
    assert_eq!(
        check("near-case", "let größe = 1\nprint(GRÖßE)\nprint(GRÖSSE)\n"),
        vec!["2: error: unknown name `GRÖßE`".to_string(), "3: error: unknown name `GRÖSSE` (did you mean `größe`?)".to_string()]
    );
    assert_eq!(check("near-ascii-case", "let total = 1\nprint(TOTAL)\n"), vec!["2: error: unknown name `TOTAL`".to_string()]);
}

// ---- Q-1053: a confined check's budget ---------------------------------------------------------

const TOO_COMPLEX: &str = "0: error: the code is too complex to check: its types grew past what a check keeps, or the check took too long";

/// What a confined check of `src` says given each of `times`: the one error, or all it says given
/// all the time it needs (`full`), never a part of it. How many ended with the error.
fn ends_whole_at(d: &Path, name: &str, src: &str, full: &[String], times: &[Duration]) -> usize {
    assert!(!full.iter().any(|l| l.contains("too complex")), "{}: {:?}", name, full);
    let mut ended = 0;
    for time in times {
        let (said, _) = check_timed(d, src, true, Some(*time));
        if said == [TOO_COMPLEX] {
            ended += 1;
        } else {
            assert_eq!(said, full, "{}: given {:?}", name, time);
        }
    }
    ended
}

/// `n + 1` times from `from` to `to`.
fn times_between(from: Duration, to: Duration, n: u32) -> Vec<Duration> {
    (0..=n).map(|i| from + to.saturating_sub(from) * i / n).collect()
}

/// A confined check reads its clock where it reads a helper's assignments (`safe_graph`, every
/// 4,096th part), works out what they hold (`unsafely_assigned`, every 256th), reads a helper's
/// SQL statement by statement (`sql_flow`, every 256th) and makes types (`room_for`, every 256th):
/// given any time from none to what it needs, it ends with the one error or says all it says, a
/// helper of 20,000 assignments and 4,000 functions of map types alike. The helper's assignments
/// are read first after the program is parsed: the times given are most dense from the parse's
/// time to the time a check that ends at once takes (FEEDBACK75.md, Q-1053; src/check.rs
/// `room_for`, `sql_flow`, `graph_expr`, `unsafely_assigned`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_confined_check_ends_whole_at_any_time() {
    let d = dir("budget");
    let _gone = Gone(d.clone());
    // A debug build is slower, not different: a smaller program reads the same clocks.
    let scale = if cfg!(debug_assertions) { 4 } else { 1 };
    let steps = 20_000 / scale;
    let helper = format!(
        "db.open(\":memory:\")\nfn big(n) {{\n    let s = \"ünï\"\n{}    s = s + n\n    return \"select * from t order by \" + s\n}}\nroute GET / = db.query(big(request.query.n))\n",
        "    s = s + \" \"\n".repeat(steps)
    );
    let (full, took) = check_timed(&d, &helper, true, Some(Duration::from_secs(3600)));
    assert_eq!(full, vec![format!("{}: {}", steps + 7, SQL_BUILT)]);
    let parse = (0..3)
        .map(|_| {
            let t = Instant::now();
            quillo::parser::Parser::new(&helper).parse_program().unwrap();
            t.elapsed()
        })
        .min()
        .unwrap();
    let (said, at_once) = check_timed(&d, &helper, true, Some(Duration::ZERO));
    assert_eq!(said, vec![TOO_COMPLEX.to_string()]);
    let mut times = times_between(parse * 9 / 10, at_once, 60);
    times.extend(times_between(parse, took, 60));
    assert!(ends_whole_at(&d, "helper", &helper, &full, &times) >= 1);
    let typed: String = (0..4_000 / scale).map(|i| format!("fn f{i}(a: {{k{i}: int}}, b: list<{{k{i}: str}}>, c: list<list<{{k{i}: bool, j{i}: int}}>>) {{ return 1 }}\n")).collect();
    let (full, took) = check_timed(&d, &typed, true, Some(Duration::from_secs(3600)));
    assert_eq!(full, Vec::<String>::new());
    assert!(ends_whole_at(&d, "typed", &typed, &full, &times_between(Duration::ZERO, took, 40)) >= 1);
}

// ---- Q-1083: a translation's attributes --------------------------------------------------------

fn query(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// `src`'s value of `out`, run in an interpreter of its own in `d`.
fn run_in(d: &Path, src: &str) -> String {
    let mut it = quillo::Interp::new(d);
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

const ATTRS: &str = r#"translations({ en: { spaced: "<a href = \"{u}\">{} go</a>", after: "<a download href= \"{u}\" hidden>{u}</a>", braces: "<a href=\"{{u}}\">x</a>", amp: "<a href=\"&#;{u}\">x</a>", hex: "<a href=\"&#x;{u}\">x</a>", eight: "<a href=\"{s}&#00000058;{r}\">x</a>" } })
route GET /spaced { query: { u: str } } = <p>{raw(t("spaced", { u: u }))}</p>
route GET /after { query: { u: str } } = <p>{raw(t("after", { u: u }))}</p>
route GET /braces { query: { u: str } } = <p>{raw(t("braces", { u: u }))}</p>
route GET /amp { query: { u: str } } = <p>{raw(t("amp", { u: u }))}</p>
route GET /hex { query: { u: str } } = <p>{raw(t("hex", { u: u }))}</p>
route GET /eight { query: { s: str, r: str } } = <p>{raw(t("eight", { s: s, r: r }))}</p>
"#;

/// A translation's attribute read as the browser reads it: white space either side of `=`, an
/// attribute with no value before or after it, and a `{` that starts no placeholder (in an attribute
/// and in text). A `javascript:` value is blocked where it makes the link's URL, and kept as written
/// after `{` or after a character reference with no digits (`&#;`, which the browser keeps as text);
/// one of 8 digits is read, as the browser reads it. (Its comment cases went with Q-1176's final
/// review: a text with a value holding a comment is refused at load, see
/// `a_text_with_a_value_holds_plain_html_only`.) (FEEDBACK75.md, Q-1083 and Q-1176;
/// src/i18n.rs `find_attr_spans`, `put_in`, `char_ref`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_translations_attributes_are_read_as_the_browser_reads_them() {
    let d = dir("attrs");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), ATTRS).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str, u: &str| {
        let r = support::send(s.port, "GET", &format!("{}?u={}", path, query(u)), &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    let js = "javascript:alert(1)";
    assert_eq!(get("/spaced", js), "<p><a href = \"about:blank#blocked\">{} go</a></p>");
    assert_eq!(get("/spaced", "/ü?a=1&b=2"), "<p><a href = \"/ü?a=1&amp;b=2\">{} go</a></p>");
    assert_eq!(get("/after", js), "<p><a download href= \"about:blank#blocked\" hidden>javascript:alert(1)</a></p>");
    assert_eq!(get("/after", "/f.txt"), "<p><a download href= \"/f.txt\" hidden>/f.txt</a></p>");
    assert_eq!(get("/braces", js), "<p><a href=\"{javascript:alert(1)}\">x</a></p>");
    assert_eq!(get("/braces", "/a"), "<p><a href=\"{/a}\">x</a></p>");
    assert_eq!(get("/amp", js), "<p><a href=\"&#;javascript:alert(1)\">x</a></p>");
    assert_eq!(get("/hex", js), "<p><a href=\"&#x;javascript:alert(1)\">x</a></p>");
    let r = support::send(s.port, "GET", "/eight?s=javascript&r=alert(1)", &[], "");
    assert_eq!(r.body, "<p><a href=\"about:blank#blocked\">x</a></p>");
    let r = support::send(s.port, "GET", "/eight?s=https&r=//example.com/", &[], "");
    assert_eq!(r.body, "<p><a href=\"https&#00000058;//example.com/\">x</a></p>");
}

/// The reading of the translation table's texts is kept up to 4 MB on the thread: past it, it
/// starts again with the text read, and one larger than 4 MB is not kept; each is read right all
/// the same (FEEDBACK75.md, Q-1083's review; src/i18n.rs `attr_spans`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn the_reading_of_translations_starts_again_past_4_mb() {
    use quillo::server::HttpRequest;
    let d = dir("spans");
    let _gone = Gone(d.clone());
    let mut it = quillo::Interp::new(&d);
    let text = |n: usize| format!("\"<a href=\\\"{{u}}\\\">ü</a>\" + \"x\".repeat({})", n);
    let src = format!(
        "translations({{ en: {{ k1: {}, k2: {}, k3: {}, big: {} }} }})\nroute GET /k {{ query: {{ k: str, u: str }} }} = len(raw(t(k, {{ u: u }})))\nroute GET /head {{ query: {{ k: str, u: str }} }} = slice(str(raw(t(k, {{ u: u }}))), 0, 36)\n",
        text(1_500_000),
        text(1_500_000),
        text(1_500_000),
        text(5_000_000)
    );
    it.load_source(&src, "app.qwe").unwrap();
    let get = |it: &mut quillo::Interp, url: &str| {
        let r = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: vec![("Host".into(), "x".into())], body: vec![], remote: "127.0.0.1".into() });
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        String::from_utf8_lossy(&r.body).into_owned()
    };
    let one = "<a href=\"{u}\">ü</a>".len() + 1_500_000;
    let made = "<a href=\"/a\">ü</a>".len() + 1_500_000;
    let before = quillo::i18n::attr_spans_kept();
    assert_eq!(get(&mut it, "/k?k=k1&u=%2Fa"), made.to_string());
    assert_eq!(quillo::i18n::attr_spans_kept(), before + one);
    assert_eq!(get(&mut it, "/k?k=k2&u=%2Fa"), made.to_string());
    assert_eq!(quillo::i18n::attr_spans_kept(), before + 2 * one);
    // The third passes 4 MB: the kept are let go, and it is kept alone.
    assert_eq!(get(&mut it, "/head?k=k3&u=javascript%3Ax"), "<a href=\"about:blank#blocked\">ü</a>x");
    assert_eq!(quillo::i18n::attr_spans_kept(), one);
    // The first again: read anew, beside the third.
    assert_eq!(get(&mut it, "/head?k=k1&u=javascript%3Ax"), "<a href=\"about:blank#blocked\">ü</a>x");
    assert_eq!(quillo::i18n::attr_spans_kept(), 2 * one);
    // One of 5 MB: none is kept, and it is read right.
    assert_eq!(get(&mut it, "/head?k=big&u=javascript%3Ax"), "<a href=\"about:blank#blocked\">ü</a>x");
    assert_eq!(quillo::i18n::attr_spans_kept(), 0);
}

/// A plural form that is not text, or a count whose form and `other` are missing, as only a table
/// made outside `translations` can hold (which refuses both): the form is shown as its value, in
/// the text form and the HTML form alike, and a missing one is empty; the table's own forms are as
/// they were (FEEDBACK75.md, Q-1083; src/i18n.rs `translate`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_plural_form_that_is_not_text_is_shown_as_its_value() {
    use quillo::value::{Map, Value};
    let d = dir("forms");
    let _gone = Gone(d.clone());
    let mut it = quillo::Interp::new(&d);
    it.load_source("translations({ en: { n: { one: \"{count} file\", other: \"{count} files\" } } })\n", "app.qwe").unwrap();
    let i18n = it.site.i18n.clone().expect("translations");
    let mut locales = i18n.locales.clone();
    let en = locales.get_mut("en").unwrap();
    let mut odd = Map::new();
    odd.insert("one".into(), Value::Int(7));
    odd.insert("other".into(), Value::str("{count} ünï"));
    en.insert("odd".to_string(), Value::map(odd));
    let mut none = Map::new();
    none.insert("one".into(), Value::str("one"));
    en.insert("none".to_string(), Value::map(none));
    Arc::make_mut(&mut it.site).i18n = Some(Arc::new(quillo::i18n::I18n { default: i18n.default.clone(), locales }));
    it.load_source(
        "let a = t(\"odd\", { count: 1 })\nlet b = raw(t(\"odd\", { count: 1, u: \"/x\" }))\nlet c = t(\"odd\", { count: 2 })\nlet e = t(\"none\", { count: 2 })\nlet g = raw(t(\"none\", { count: 2, u: \"/x\" }))\nlet f = t(\"n\", { count: 1 })\n",
        "more.qwe",
    )
    .unwrap();
    let got = |n: &str| quillo::value::lookup(&it.globals, n).unwrap().display();
    assert_eq!(got("a"), "7");
    assert_eq!(got("b"), "7");
    assert_eq!(got("c"), "2 ünï");
    assert_eq!(got("e"), "");
    assert_eq!(got("g"), "");
    assert_eq!(got("f"), "1 file");
}

// ---- Q-1055: what a script's builtins read -----------------------------------------------------

/// `src`'s value of `out`, run in an interpreter of its own.
fn run(src: &str) -> String {
    let mut it = quillo::Interp::new(".");
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

/// `==` on maps pays for their keys and text fields, and on lists of errors for the errors and
/// what they hold: 12,000 rounds of two maps of a field of 502 bytes (1,022 read a round) fit in
/// 20,000 steps, of 503 (1,024) they do not; two lists of 31 errors of a number (992) fit, of 32
/// (1,024) they do not (FEEDBACK75.md, Q-1055; src/script.rs `read_size`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_script_pays_for_the_maps_and_errors_it_reads() {
    let maps = |len: usize| run(&format!("run_script(\"let n = 0\\nfor i in 0..12000 {{ if m == m2 {{ n = n + 1 }} }}\\nn\", {{ m: {{ s: join(map(0..{len}, (i) => \"a\"), \"\") }}, m2: {{ s: join(map(0..{len}, (i) => \"b\"), \"\") }} }}, {{ timeout: 20000 }})"));
    assert_eq!(maps(502), "0");
    assert!(maps(503).contains("the script took too many steps"), "{}", maps(503));
    let errors = |k: usize| run(&format!("run_script(\"let n = 0\\nfor i in 0..12000 {{ if xs == ys {{ n = n + 1 }} }}\\nn\", {{ xs: map(0..{k}, (i) => err(i)), ys: map(0..{k}, (i) => err(i + 1)) }}, {{ timeout: 20000 }})"));
    assert_eq!(errors(31), "0");
    assert!(errors(32).contains("the script took too many steps"), "{}", errors(32));
    // A map of 260 fields of a letter not ASCII (3,530 bytes read a round): three steps a round,
    // too many.
    let wide = run("run_script(\"let n = 0\\nfor i in 0..12000 { if m == m2 { n = n + 1 } }\\nn\", { m: reduce(0..260, {}, (m, i) => set(m, \"k\" + str(i), \"ü\")), m2: {} }, { timeout: 20000 })");
    assert!(wide.contains("the script took too many steps"), "{}", wide);
}

// ---- Q-1058: the page states of the whole program's bundle --------------------------------------

/// The page states a page carries (its `.g=` script, as JSON), or none.
fn states_of(page: &str) -> Option<serde_json::Value> {
    let at = page.find(".g=")? + 3;
    let text = &page[at..at + page[at..].find("</script>")?];
    let json: String = serde_json::from_str(text).ok()?;
    serde_json::from_str(&json).ok()
}

/// A page that loads the whole program's bundle carries the page states any component reads,
/// worked out once for the program: the second page, from what was worked out for the first, the
/// same (FEEDBACK75.md, Q-1058; src/client.rs `all_states`; coverage of 0.8.9; branch feedback75).
#[test]
fn the_whole_bundles_page_states_are_worked_out_once() {
    use quillo::server::HttpRequest;
    let src = "state secret = env(\"COVER089_UNSET\") ?? \"none\"\nstate total = 3\nstate label = \"ünï ✓\"\ncomponent Shows() {\n    state n = 0\n    <p onclick={() => { n = n + 1 }}>{total} {label} {n}</p>\n}\nroute GET /whole = <html><head><script src=\"/_quillo/quillo.js\"></script></head><body><Shows/></body></html>\n";
    let d = dir("whole");
    let _gone = Gone(d.clone());
    let mut it = quillo::Interp::new(&d);
    it.load_source(src, "app.qwe").unwrap();
    for _ in 0..2 {
        let r = it.handle(&HttpRequest { method: "GET".into(), url: "/whole".into(), headers: vec![("Host".into(), "x".into())], body: vec![], remote: "127.0.0.1".into() });
        let page = String::from_utf8_lossy(&r.body).into_owned();
        assert_eq!(r.status, 200, "{}", page);
        assert_eq!(states_of(&page), Some(serde_json::json!({ "total": 3, "label": "ünï ✓" })), "{}", page);
    }
}

// ---- Q-1063: the bundle's guard ------------------------------------------------------------------

/// What loading `head` before a component that shows `K` says: nothing, or why it stops.
fn load_with_k(head: &str) -> String {
    let src = format!("{}\ncomponent Pay() {{\n    state n = 0\n    <button onclick={{() => {{ n = n + 1 }}}}>Pay {{n}} {{K}}</button>\n}}\nroute GET / = <html><body><Pay/></body></html>\n", head);
    let mut it = quillo::Interp::new(env!("CARGO_MANIFEST_DIR"));
    match it.load_source(&src, "case.qwe") {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    }
}

/// A constant made in a block with a function of its own (`fn twice` inside an `if`) is read with
/// the function's name bound there: it goes to the page when the function makes plain values (one
/// named `crypto` too), and is refused when it reads `crypto` (FEEDBACK75.md, Q-1063, review;
/// src/client.rs `free_stmts`; coverage of 0.8.9; branch feedback75).
#[test]
fn the_bundle_guard_reads_a_blocks_own_function() {
    let refused = "`K` is read from the server's environment, files, database, a service, a plugin, its command line or input, a site or `crypto`, and would be sent to every browser";
    for head in [
        "let K = if true {\n    fn twice(x) = x * 2\n    twice(21)\n} else {\n    0\n}",
        "let K = if true {\n    fn crypto() = \"ünï\"\n    crypto()\n} else {\n    \"x\"\n}",
    ] {
        assert_eq!(load_with_k(head), "", "{}", head);
    }
    let said = load_with_k("let K = if true {\n    fn secret() = crypto.token()\n    secret()\n} else {\n    \"x\"\n}");
    assert!(said.contains(refused), "{}", said);
}

// ---- Q-1088: a link to the policy -----------------------------------------------------------------

/// `files` does not serve the policy the program runs under through a link of another name to it
/// (`notes.txt -> quillo.policy`, `p.txt -> prod.policy` given with `--policy`), as it does not by
/// its own name; a link to another file is served (FEEDBACK75.md, Q-1088, found in review;
/// src/builtins.rs `hidden_target`; coverage of 0.8.9; branch feedback75).
#[cfg(unix)]
#[test]
fn files_keeps_a_link_to_the_policy_to_itself() {
    let d = dir("policy-link");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), "route GET /*path = files(\".\", path)\n").unwrap();
    std::fs::write(d.join("quillo.policy"), "[env]\nnames = [\"STRIPE_KEY\"]\n").unwrap();
    std::fs::write(d.join("prod.policy"), "[env]\nnames = [\"OTHER_KEY\"]\n").unwrap();
    std::fs::write(d.join("a.txt"), "ünï").unwrap();
    std::os::unix::fs::symlink("quillo.policy", d.join("notes.txt")).unwrap();
    std::os::unix::fs::symlink("prod.policy", d.join("p.txt")).unwrap();
    std::os::unix::fs::symlink("a.txt", d.join("ok.txt")).unwrap();
    let s = support::serve_with(&d.join("main.qwe"), &["--policy", &d.join("prod.policy").display().to_string()]);
    for p in ["/notes.txt", "/p.txt", "/quillo.policy"] {
        let a = support::send(s.port, "GET", p, &[], "");
        assert_eq!(a.status, 404, "{}: {}", p, a.body);
        assert!(!a.body.contains("KEY"), "{}: {}", p, a.body);
    }
    assert_eq!(support::send(s.port, "GET", "/ok.txt", &[], "").body, "ünï");
    assert_eq!(support::send(s.port, "GET", "/a.txt", &[], "").body, "ünï");
}

// ---- FEEDBACK78 A.13: --client-cert-header --------------------------------------------------------

/// `quillo run` with `args`, which stops at once: its exit code and stderr, killed past 60 s.
fn refused(d: &Path, args: &[&str]) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "main.qwe", "--port", "0"]).args(args).current_dir(d).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    support::stop_with_tests(&child);
    let end = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("quillo run {:?} did not stop", args);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    (status.code(), err)
}

/// `--client-cert-header` with no header's name (a colon, a space, a letter not ASCII) is refused
/// as `quillo run` starts, as `--user-header`'s is (FEEDBACK78.md, A.13; src/main.rs
/// `run_options`; coverage of 0.8.9; branch feedback75).
#[test]
fn client_cert_header_needs_a_headers_name() {
    let d = dir("cert-header");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), "route GET / = \"ok\"\n").unwrap();
    for name in ["x:y", "x y", "ü", "x-client-cert\r\nx: y"] {
        let (code, said) = refused(&d, &["--trust-proxy", "127.0.0.1", "--client-cert-header", name]);
        assert_eq!(code, Some(2), "{:?}: {}", name, said);
        assert!(said.contains("quillo: --client-cert-header needs a header's name, as x-forwarded-client-cert, once"), "{:?}: {}", name, said);
    }
}

// ---- Q-1051: a PNG's colour profile ----------------------------------------------------------------

/// A PNG chunk: its length, kind, data and CRC.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
    out
}

/// A 1×1 RGB PNG with an `iCCP` chunk of `iccp` as it is, before its pixels.
fn png_with_iccp(iccp: &[u8]) -> Vec<u8> {
    let mut ihdr = 1u32.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&[0, 255, 0, 0]).unwrap();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"iCCP", iccp));
    out.extend(chunk(b"IDAT", &z.finish().unwrap()));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// A colour profile whose name the chunk ends inside (no zero after it: empty, `abc`, 80 letters,
/// `Café` in UTF-8) is passed over as the decoder passes it over, and the image is read; one of 81
/// letters is too (FEEDBACK75.md, Q-1051; src/imaging.rs `profiles_from`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_png_profile_whose_name_is_cut_short_is_passed_over() {
    let d = dir("iccp-name");
    let _gone = Gone(d.clone());
    let info = r#"{ width: 1, height: 1, format: "png", alpha: false }"#;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty.png", Vec::new()),
        ("abc.png", b"abc".to_vec()),
        ("eighty.png", vec![b'n'; 80]),
        ("cafe.png", "Café".as_bytes().to_vec()),
        ("long.png", vec![b'n'; 81]),
    ];
    for (name, iccp) in &cases {
        std::fs::write(d.join(name), png_with_iccp(iccp)).unwrap();
        assert_eq!(run_in(&d, &format!("image.info({:?})", name)), info, "{}", name);
    }
}

// ==== Fixes found by this coverage ===============================================================

// ---- Q-1127: names bound in a long block ---------------------------------------------------------

/// `fn big(n)` of `n` `let`s, each of the one before, called once.
fn lets(n: usize) -> String {
    let mut src = String::from("fn big(n) {\n    let a0 = 1\n");
    for i in 1..n {
        src.push_str(&format!("    let a{} = a{}\n", i, i - 1));
    }
    src.push_str(&format!("    return a{}\n}}\nprint(big(1))\n", n - 1));
    src
}

/// The least of three timings of `f`.
fn least(mut f: impl FnMut() -> Duration) -> Duration {
    (0..3).map(|_| f()).min().unwrap()
}

/// A function of many `let`s is checked and loaded in time that grows with it, not with its square:
/// 100,000 took 7.5 s to check and to load, where 0.8.8 took 0.14 s. Four times the `let`s take
/// about four times as long, not sixteen, in `quillo check`'s check, a confined one and a load
/// (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1127; src/client.rs `Bound`, `each_free`;
/// src/check.rs `stream_sends_in_transactions`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_function_of_many_lets_is_checked_and_loaded_in_linear_time() {
    let d = dir("many-lets");
    let _gone = Gone(d.clone());
    let n = if cfg!(debug_assertions) { 10_000 } else { 40_000 };
    let (small, large) = (lets(n), lets(4 * n));
    for confined in [false, true] {
        let check = |src: &str| {
            least(|| {
                let (said, took) = check_timed(&d, src, confined, Some(Duration::from_secs(3600)));
                assert_eq!(said, Vec::<String>::new());
                took
            })
        };
        let (a, b) = (check(&small), check(&large));
        assert!(b < a * 10, "confined {}: {:?} for {} lets, {:?} for {}", confined, a, n, b, 4 * n);
    }
    let load = |src: &str| {
        least(|| {
            let mut it = quillo::Interp::new(&d);
            let t = Instant::now();
            it.load_source(src, "big.qwe").unwrap();
            t.elapsed()
        })
    };
    let (a, b) = (load(&small), load(&large));
    assert!(b < a * 10, "load: {:?} for {} lets, {:?} for {}", a, n, b, 4 * n);
}

/// A confined check of 85,000 lines of `let b = x` (850 KB), alone and called by a route, ends at
/// its time with the one error, where it went on 23.7 s and 8 s past its 2 s (the stream pass, the
/// walk's statements, the GET-that-writes walk): given 300 ms here, it ends well within ten times
/// that (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1127; src/check.rs
/// `stream_sends_in_transactions`, `expr_here`, `GetWalk::block`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_confined_check_of_many_lets_ends_at_its_time() {
    let d = dir("lets-time");
    let _gone = Gone(d.clone());
    let body = "    let b = x\n".repeat(85_000);
    for tail in ["", "route GET / = str(f(1))\n"] {
        let src = format!("fn f(x) {{\n{}    return b\n}}\n{}", body, tail);
        let budget = Duration::from_millis(300);
        let (said, took) = check_timed(&d, &src, true, Some(budget));
        assert_eq!(said, vec![TOO_COMPLEX.to_string()], "{:?}", tail);
        assert!(took < budget * 10 * slow(), "{:?}: {:?}", tail, took);
    }
}

/// How much longer a debug build takes, for time bounds.
fn slow() -> u32 {
    if cfg!(debug_assertions) {
        10
    } else {
        1
    }
}

/// Past 16 names bound, a name is found by its hash and let go of with its scope as before: a
/// lambda's `args` after 20 `let`s is the lambda's, and after the lambda the builtin again; a
/// local `args` bound before 20 more and again by a lambda is still the local after it; a stream's
/// `send` named again after 20 `let`s is not the stream's, and one that is not named again is
/// warned of inside a transaction (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1127;
/// src/client.rs `Bound`; coverage of 0.8.9; branch feedback75).
#[test]
fn names_past_sixteen_are_bound_and_let_go_of_as_before() {
    let twenty: String = (1..=20).map(|i| format!("    let a{} = {}\n", i, i)).collect();
    let refused = "`K` is read from the server's environment, files, database, a service, a plugin, its command line or input, a site or `crypto`, and would be sent to every browser";
    let head = |tail: &str| format!("fn helper(xs) {{\n{}    let n = [1].map((args) => args + a20)\n    return {}\n}}\nlet K = helper([1])", twenty, tail);
    assert_eq!(load_with_k(&head("len(n)")), "");
    let said = load_with_k(&head("args()"));
    assert!(said.contains(refused), "{}", said);
    let again = format!("fn helper(xs) {{\n    let args = len(xs)\n{}    let n = [1].map((args) => args + a20)\n    return args + len(n)\n}}\nlet K = helper([1])", twenty);
    assert_eq!(load_with_k(&again), "");
    let src = format!(
        "db.open(\":memory:\")\nroute GET /a.txt = stream((send) => {{\n{}    let rows = [1].map((send) => db.transaction(() => send + a20))\n    send(str(rows))\n}})\nroute GET /b.txt = stream((send) => {{\n{}    let r = db.transaction(() => {{\n        send(\"ünï\")\n    }})\n    send(str(r) + str(a20))\n}})\n",
        twenty, twenty
    );
    let said: Vec<String> = check("bound-many", &src).into_iter().filter(|l| !l.contains("unused variable")).collect();
    assert_eq!(said.len(), 1, "{:#?}", said);
    assert!(said[0].starts_with(&format!("{}: warning: `send` inside `db.transaction` waits for the client to read", 2 * 20 + 8)), "{}", said[0]);
}

// ---- Q-1177: character references in a translation ----------------------------------------------

const ENDS: &str = r#"translations({ en: { nine: "<a href=\"{s}&#000000058;{r}\">x</a>", long: "<a href=\"{s}&#0000000000000000000000000000000000000000058{r}\">x</a>", hex: "<a href=\"{s}&#X00000003A;{r}\">x</a>", past: "<a href=\"{s}&#99999999999;{r}\">x</a>" } })
route GET /r { query: { k: str, s: str, r: str } } = <p>{raw(t(k, { s: s, r: r }))}</p>
"#;

/// A character reference is read with all its digits, as the browser reads it: `&#000000058;` (9),
/// 40 zeros, `&#X00000003A;`, each a colon, make the link a `javascript:` one, blocked; one past
/// U+10FFFF is U+FFFD and makes none. (The comment cases this test had went with Q-1176's final
/// review: such texts are refused at load.) (FEEDBACK75.md follow-up found by 0.8.9's coverage,
/// Q-1177; src/i18n.rs `char_ref`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_translations_references_are_read_with_all_their_digits() {
    let d = dir("ends");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), ENDS).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: String| {
        let r = support::send(s.port, "GET", &path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    let blocked = "<a href=\"about:blank#blocked\">x</a>";
    for k in ["nine", "long", "hex"] {
        assert_eq!(get(format!("/r?k={}&s=javascript&r=alert(1)", k)), format!("<p>{}</p>", blocked), "{}", k);
    }
    assert_eq!(get("/r?k=nine&s=https&r=//example.com/".to_string()), "<p><a href=\"https&#000000058;//example.com/\">x</a></p>");
    assert_eq!(get("/r?k=past&s=javascript&r=alert(1)".to_string()), "<p><a href=\"javascript&#99999999999;alert(1)\">x</a></p>");
}

// ---- Q-1128: --client-cert-header once -------------------------------------------------------------

/// `--client-cert-header` given twice is refused, as `--user-header` given twice is: the last was
/// taken, and a first that named no header was dropped without a word (FEEDBACK75.md follow-up
/// found by 0.8.9's coverage, Q-1128; src/main.rs `run_options`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn client_cert_header_is_given_once() {
    let d = dir("cert-twice");
    let _gone = Gone(d.clone());
    std::fs::write(d.join("main.qwe"), "route GET / = \"ok\"\n").unwrap();
    for (first, second) in [("a", "b"), ("x:y", "x-forwarded-client-cert"), ("x-client-dn", "x-client-dn")] {
        let (code, said) = refused(&d, &["--trust-proxy", "127.0.0.1", "--client-cert-header", first, "--client-cert-header", second]);
        assert_eq!(code, Some(2), "{} {}: {}", first, second, said);
        assert!(said.contains("quillo: --client-cert-header needs a header's name, as x-forwarded-client-cert, once"), "{}", said);
    }
}

// ---- Q-1176's final review: a text with a value holds plain HTML only ------------------------------

/// What loading `translations(table)` says: nothing, or why the program stops.
fn load_table(table: &str) -> String {
    let mut it = quillo::Interp::new(".");
    match it.load_source(&format!("translations({})\n", table), "t.qwe") {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    }
}

/// A translation text that holds a placeholder and anything the browser reads otherwise than plain
/// HTML is refused as the program loads, its key, locale and the construct named: `<!--`, `<!`
/// (CDATA, a doctype), `<?`, `</` and no letter (or none), and the start tag of each element whose
/// text is not markup or that switches to SVG or MathML, in any case; in a plural form and in a
/// folder's JSON too. The same markup without a placeholder (or with braces that make none) loads
/// as before, and so does plain HTML with one (FEEDBACK75.md follow-up found by 0.8.9's coverage,
/// Q-1176's final review; src/i18n.rs `not_plain`, `translations`; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn a_text_with_a_value_holds_plain_html_only() {
    let refused = |text: &str, what: &str| {
        let said = load_table(&format!("{{ de: {{ k: {} }} }}", serde_json::Value::String(text.to_string())));
        let want = format!("translation `k` (de): a text with a value may hold plain HTML only, not `{}`: put that markup in the component around `t(...)`", what);
        assert!(said.contains(&want), "{:?}: {}", text, said);
    };
    for (text, what) in [
        ("<!-- note --> {u}", "<!--"),
        ("<![CDATA[x]]>{u}", "<!"),
        ("<!DOCTYPE html>{u}", "<!"),
        ("<?x y?>{u}", "<?"),
        ("</ x>{u}", "</"),
        ("</1>{u}", "</"),
        ("{u} </", "</"),
        ("<b>ü</b><script>x</script>{u}", "<script>"),
        ("<style>b{}</style>{u}", "<style>"),
        ("<XMP>{u}</XMP>", "<xmp>"),
        ("<iframe src=\"/x\"></iframe>{u}", "<iframe>"),
        ("<noembed>{u}", "<noembed>"),
        ("<noframes>{u}", "<noframes>"),
        ("<noscript>{u}", "<noscript>"),
        ("<Title>{u}</Title>", "<title>"),
        ("<textarea name=n>{u}</textarea>", "<textarea>"),
        ("<plaintext>{u}", "<plaintext>"),
        ("<template>{u}</template>", "<template>"),
        ("<SVG width=1>{u}</SVG>", "<svg>"),
        ("<svg/>{u}", "<svg>"),
        ("<math><mi>{u}</mi></math>", "<math>"),
    ] {
        refused(text, what);
    }
    let said = load_table(r#"{ en: { files: { one: "{count} <svg/>", other: "{count} files" } } }"#);
    assert!(said.contains("translation `files` (en): a text with a value may hold plain HTML only, not `<svg>`"), "{}", said);
    let said = load_table(r#"{ en: { a: { b: "<b>{u}</b>", c: "<!--{u}-->" } } }"#);
    assert!(said.contains("translation `a.c` (en): a text with a value may hold plain HTML only, not `<!--`"), "{}", said);
    for table in [
        r#"{ en: { k: "<svg><title>x</title></svg><!-- note --><script>x</script><?x?></ x><plaintext>" } }"#,
        r#"{ en: { k: "<svg>{ u}</svg><!--{}--><title>{-}</title>" } }"#,
        r#"{ en: { k: "<b title=\"{t}\">{u}</b> <a href={u}>x</a> &amp; 1 < 2 <span>ü</span></p>" } }"#,
        r#"{ en: { files: { one: "<b>{count}</b> file", other: "<b>{count}</b> files" } } }"#,
    ] {
        assert_eq!(load_table(table), "", "{}", table);
    }
    // From a folder of JSON files too.
    let d = dir("plain-folder");
    let _gone = Gone(d.clone());
    std::fs::create_dir_all(d.join("locales")).unwrap();
    std::fs::write(d.join("locales/fr.json"), r#"{ "k": "<b>{u}</b>", "m": "<svg>{u}</svg>" }"#).unwrap();
    let mut it = quillo::Interp::new(&d);
    let said = it.load_source("translations(\"locales\")\n", "t.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert!(said.contains("translation `m` (fr): a text with a value may hold plain HTML only, not `<svg>`"), "{}", said);
}

// ---- Q-1176 (and its review) and Q-1177 in the browser: runtime.js -----------------------------

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `url` in headless Chrome (tests/browser/run.js) with `script` run in the page: what the page
/// stored in `data-result`. Node and Chrome are stopped past 100 s.
fn in_chrome(chrome: &Path, url: &str, script: &str, d: &Path) -> serde_json::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    std::fs::write(d.join("t.js"), script).unwrap();
    let mut child = Command::new("node")
        .arg(root.join("browser/run.js"))
        .arg(chrome)
        .arg(url)
        .arg(d.join("profile"))
        .arg("60000")
        .arg(d.join("t.js"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let read = |mut r: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = r.read_to_end(&mut s);
            String::from_utf8_lossy(&s).into_owned()
        })
    };
    let (out, err) = (read(Box::new(child.stdout.take().unwrap())), read(Box::new(child.stderr.take().unwrap())));
    let end = Instant::now() + Duration::from_secs(100);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Chrome did not finish within 100 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (out, err) = (out.join().unwrap(), err.join().unwrap());
    assert!(status.success(), "{}", err);
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{}: {}", e, out))
}

/// The translations both sides are compared on: plain HTML, all a text with a value may hold since
/// Q-1176's final review (its comment, raw text and SVG cases are refused at load now): the
/// attribute forms, Q-1177's references, Q-1083's code and animations, and the final review's
/// `<` before a value (A), a value outside any attribute's (B), unquoted values and `=` names (D), a
/// value in an attribute's name (E) and `&` before a value (F).
const PARITY: &[(&str, &str)] = &[
    ("spaced", r#"<a href = "{u}">{} go</a>"#),
    ("after", r#"<a download href= "{u}" hidden>{u}</a>"#),
    ("braces", r#"<a href="{{u}}">x</a>"#),
    ("nine", r#"<a href="{u}&#000000058;{u}">x</a>"#),
    ("long", r#"<a href="{u}&#0000000000000000000000000000000000000000058{u}">x</a>"#),
    ("hex", r#"<a href="{u}&#X00000003A;{u}">x</a>"#),
    ("eight", r#"<a href="{u}&#00000058;{u}">x</a>"#),
    ("past", r#"<a href="{u}&#99999999999;{u}">x</a>"#),
    ("nodigit", r#"<a href="&#;{u}">x</a>"#),
    ("named", r#"<a href="{u}&colon;{u}&amp;&zz;">x</a>"#),
    ("anim", r#"<a><set attributeName="href" to="{u}"/><animate attributeName="href" values="a;{u}"/>x</a>"#),
    ("onclick", r#"<button onclick="{u}" onmouseover={u}>x</button>"#),
    ("srcdoc", r#"<iframe-x srcdoc="{u}"></iframe-x>"#),
    ("ttl", r#"<b title="{u}">{u}</b>"#),
    ("plain", "<b>{u}</b> <a href={u} title={u}>x</a>"),
    ("lt1", "Must be <{u}"),
    ("lt2", "Press <{u}> to save"),
    ("lt3", "1 < 2 and {u}"),
    ("lt4", r#"<{u}<a href="{u}">x</a> 1 < 2"#),
    ("gtloose", r#"<input placeholder="a > b" {u}>"#),
    ("endloose", "<b>x</b {u}>"),
    ("unq", r#"<a title=a"{u}>x</a>"#),
    ("eqval", r#"<a href= ="{u}">x</a>"#),
    ("eqname", r#"<a ="{u}">x</a>"#),
    ("aname", r#"<a {u}="{u}">x</a>"#),
    ("aname2", r#"<a h{u}="{u}" {u}>x</a>"#),
    ("tagname", r#"<a{u} href="/x">x</a>"#),
    ("stag", "<s{u}>{u}</s{u}>"),
    ("amp1", r#"<a href="&{u}">x</a>"#),
    ("amp2", r#"<a href="&#{u}">x</a> {u}&{u}"#),
];

/// The browser's HTML form of `t` writes what the server's does for every case of `PARITY`, each with values that block (`javascript:alert(1)`, `javascript` around a
/// colon a reference makes), end an attribute or add one, and plain ones (unicode, empty): the
/// server's text and the runtime's, each put in a `<quillo-raw>` as the runtime puts it, are the
/// same in Chrome (FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1176, its review C1, and
/// Q-1177; src/client/runtime.js `findAttrSpans`, `readAttrSpans`, `skippedTo`, `charRef`;
/// coverage of 0.8.9; branch feedback75).
#[test]
fn the_browser_and_the_server_read_comments_raw_text_and_references_alike() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let d = dir("parity");
    let _gone = Gone(d.clone());
    let table: Vec<String> = PARITY.iter().map(|(k, v)| format!("{}: {}", k, serde_json::Value::String(v.to_string()))).collect();
    let values = ["javascript:alert(1)", "javascript", "/ü", "x onfocus=alert(1)", "\"><b>", "", "a;javascript:x", "onclick", "#106;avascript:alert(1)", "script>alert(1)</script"];
    let cases: Vec<serde_json::Value> = PARITY.iter().flat_map(|(k, _)| values.iter().map(move |v| serde_json::json!({ "k": k, "v": { "u": v } }))).collect();
    let n = cases.len();
    let program = format!(
        "translations({{ en: {{ {} }} }})\nlet cases = json.parse(read_file(\"cases.json\"))\ncomponent Cases(cases: list) {{\n    state shown = false\n    <div>\n        <button id=\"go\" onclick={{() => {{ shown = true }}}}>go</button>\n        <div id=\"b\">{{shown ? (for c in cases => <p class=\"c\">{{raw(t(c.k, c.v))}}<i hidden>{{t(c.k, c.v)}}</i></p>) : null}}</div>\n    </div>\n}}\nroute GET / = <html><body><Cases cases={{cases}}/></body></html>\nroute GET /srv = json.stringify(map(cases, (c) => str(raw(t(c.k, c.v)))))\n",
        table.join(", ")
    );
    std::fs::write(d.join("main.qwe"), &program).unwrap();
    std::fs::write(d.join("cases.json"), serde_json::Value::Array(cases).to_string()).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let script = r##"(async () => {
  const out = {}, tick = (ms) => new Promise((r) => setTimeout(r, ms)), $ = (q) => document.querySelector(q);
  const put = (h) => { const el = document.createElement("quillo-raw"); el.innerHTML = h; return el.innerHTML; };
  try {
    for (let i = 0; i < 200 && !$("#go"); i++) await tick(30);
    const sent = JSON.parse(await (await fetch("/srv")).text());
    const want = sent.map(put);
    $("#go").click();
    for (let i = 0; i < 300 && document.querySelectorAll("#b p.c").length < want.length; i++) await tick(30);
    const got = [...document.querySelectorAll("#b p.c")].map((p) => { const r = p.querySelector("quillo-raw"); return r ? r.innerHTML : null; });
    out.n = got.length;
    out.server = want.length;
    out.differ = got.map((g, i) => g === want[i] ? null : { i, browser: g, server: want[i] }).filter(Boolean).slice(0, 8);
    out.blocked = sent.filter((w) => w.includes("about:blank#blocked")).length;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();"##;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script, &d);
    assert_eq!(r["differ"], serde_json::json!([]), "{}", r);
    assert_eq!((r["n"].as_u64(), r["server"].as_u64()), (Some(n as u64), Some(n as u64)), "{}", r);
    assert!(r["blocked"].as_u64().unwrap_or(0) >= 25 && r["errors"] == serde_json::json!([]), "{}", r);
}

// ---- Q-1083's recheck (A2) and Q-1176's final review (A, B, D, E, F) ----------------------------

/// `s` with each character a character reference, as a loose value is written.
fn refs(s: &str) -> String {
    s.chars().map(|c| format!("&#{};", c as u32)).collect()
}

/// What each key of `PARITY` gives on the server for each value, served as a page would.
fn served(keys: &[&str], values: &[&str]) -> Vec<Vec<String>> {
    let d = dir("plain-fixes");
    let _gone = Gone(d.clone());
    let table: Vec<String> = PARITY.iter().filter(|(k, _)| keys.contains(k)).map(|(k, v)| format!("{}: {}", k, serde_json::Value::String(v.to_string()))).collect();
    std::fs::write(d.join("main.qwe"), format!("translations({{ en: {{ {} }} }})\nroute GET /c {{ query: {{ k: str, u: str }} }} = raw(t(k, {{ u: u }}))\n", table.join(", "))).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    keys.iter()
        .map(|k| {
            values
                .iter()
                .map(|u| {
                    let r = support::send(s.port, "GET", &format!("/c?k={}&u={}", k, query(u)), &[], "");
                    assert_eq!(r.status, 200, "{}: {}", k, r.body);
                    r.body
                })
                .collect()
        })
        .collect()
}

/// Plain HTML whose value the browser would read as more than a value is written so that it is not:
/// a `<` that is text before a value is `&lt;` (A); a value in a tag outside any attribute's value,
/// past a quoted `>` (B) or in an end tag, is checked and written with references; a value written
/// without quotes is quoted, a `"` of the text in it `&quot;`, and a name may start with `=` (D); a
/// value in an attribute's name or a tag's is written with references and that attribute's value
/// emptied (E); a `&` a value would make a character reference with is `&amp;` (F). An SVG
/// animation's `to` and `values` are checked and `onclick`, `onmouseover` and `srcdoc` emptied (A2);
/// a `title` and text keep a value as written (FEEDBACK75.md follow-up found by 0.8.9's coverage,
/// Q-1083's recheck A2 and Q-1176's final review A, B, D, E, F; src/i18n.rs `read_attr_spans`,
/// `interpolate_html`, `put_in`; coverage of 0.8.9; branch feedback75).
#[test]
fn a_values_place_in_plain_html_is_the_browsers() {
    let keys = ["lt1", "lt2", "lt3", "lt4", "gtloose", "endloose", "unq", "eqval", "eqname", "aname", "aname2", "tagname", "stag", "amp1", "amp2", "anim", "onclick", "srcdoc", "ttl", "plain"];
    let values = ["javascript:alert(1)", "x onclick=alert(1)", "onclick", "#106;avascript:alert(1)", "script>alert(1)</script", "ü 1", "cript"];
    let inject = refs("x onclick=alert(1)");
    let got = served(&keys, &values);
    let at = |k: &str, v: usize| got[keys.iter().position(|x| *x == k).unwrap()][v].clone();
    let b = "about:blank#blocked";
    // A: the `<` is text, whatever comes after it.
    assert_eq!(at("lt1", 4), "Must be &lt;script&gt;alert(1)&lt;/script");
    assert_eq!(at("lt2", 2), "Press &lt;onclick> to save");
    assert_eq!(at("lt3", 1), "1 &lt; 2 and x onclick=alert(1)");
    assert_eq!(at("lt4", 0), format!("&lt;javascript:alert(1)<a href=\"{}\">x</a> 1 < 2", b));
    // B: outside any value, in a start tag or an end tag.
    assert_eq!(at("gtloose", 1), format!("<input placeholder=\"a > b\" {}>", inject));
    assert_eq!(at("gtloose", 0), format!("<input placeholder=\"a > b\" {}>", b));
    assert_eq!(at("endloose", 1), format!("<b>x</b {}>", inject));
    // D: quoted, the text's `"` as `&quot;`; `= "..."` is a value, `="..."` a name.
    assert_eq!(at("unq", 1), "<a title=\"a&quot;x onclick=alert(1)\">x</a>");
    assert_eq!(at("eqval", 0), "<a href= \"=&quot;javascript:alert(1)&quot;\">x</a>");
    assert_eq!(at("eqname", 1), format!("<a =\"{}\">x</a>", inject));
    assert_eq!(at("eqname", 0), format!("<a =\"{}\">x</a>", b));
    // E: the name's value as references, letters too, so that it names no attribute or element;
    // the attribute's value emptied.
    let on = refs("onclick");
    assert_eq!(at("aname", 2), format!("<a {}=\"\">x</a>", on));
    assert_eq!(at("aname2", 2), format!("<a h{}=\"\" {}>x</a>", on, on));
    assert_eq!(at("tagname", 1), format!("<a{} href=\"/x\">x</a>", inject));
    assert_eq!(at("stag", 6), format!("<s{}>cript</s{}>", refs("cript"), refs("cript")));
    // F: `&amp;`, so that the value makes no character reference.
    assert_eq!(at("amp1", 3), "<a href=\"&amp;#106;avascript:alert(1)\">x</a>");
    assert_eq!(at("amp2", 3), "<a href=\"&amp;##106;avascript:alert(1)\">x</a> #106;avascript:alert(1)&amp;#106;avascript:alert(1)");
    // A2: animation values checked, code and pages emptied.
    assert_eq!(at("anim", 0), format!("<a><set attributeName=\"href\" to=\"{}\"/><animate attributeName=\"href\" values=\"{}\"/>x</a>", b, b));
    assert_eq!(at("anim", 5), "<a><set attributeName=\"href\" to=\"ü 1\"/><animate attributeName=\"href\" values=\"a;ü 1\"/>x</a>");
    assert_eq!(at("onclick", 1), "<button onclick=\"\" onmouseover=\"\">x</button>");
    assert_eq!(at("srcdoc", 4), "<iframe-x srcdoc=\"\"></iframe-x>");
    // As before: a title and text keep the value; a quoted unquoted link is blocked or kept.
    assert_eq!(at("ttl", 0), "<b title=\"javascript:alert(1)\">javascript:alert(1)</b>");
    assert_eq!(at("plain", 1), "<b>x onclick=alert(1)</b> <a href=\"x onclick=alert(1)\" title=\"x onclick=alert(1)\">x</a>");
    assert_eq!(at("plain", 0), format!("<b>javascript:alert(1)</b> <a href=\"{}\" title=\"javascript:alert(1)\">x</a>", b));
}

/// `n`-fold patterns of the final review's speed cases, each a text of about `n` times the unit.
fn speed_cases(n: usize) -> Vec<(&'static str, String)> {
    let r = |s: &str, k: usize| s.repeat(k);
    vec![
        ("comments", r("<!--x-->", n / 8) + "<a href=\"{u}\">x</a>"),
        ("comments7", r("<!---->", n / 7) + "{u}"),
        ("comments8", r("<!----!>", n / 8) + "{u}"),
        ("stack", r("<svg><desc>", n / 22) + &r("</x>", n / 8) + "<a href=\"{u}\">x</a>"),
        ("mathmi", r("<math><mi>", n / 20) + &r("</a", n / 6) + "{u}"),
        ("lt", r("<", n) + "{u}"),
        ("aaa", r("<a", n / 2) + "{u}"),
        ("quote", "<a b=\"".to_string() + &r("x", n) + "{u}"),
        ("style", r("<style>", n / 7) + "{u}"),
        ("bogus", r("<!x", n / 3) + "{u}"),
        ("endtags", r("</a", n / 3) + "{u}"),
        ("attrs", "<a ".to_string() + &r("b={u} ", n / 6) + ">"),
        ("names", "<a ".to_string() + &r("{u}=x ", n / 6) + ">"),
        ("cdata", "<svg>".to_string() + &r("<![CDATA[x]]>", n / 13) + "{u}"),
        ("loose", r("<a {u}", n / 6)),
        ("amp", r("&", n) + "{u}"),
        ("ampa", r("&a", n / 2) + "{u}"),
        ("ltgt", r("< {u}", n / 5)),
    ]
}

/// Reading a text of 100 KB of any of the final review's speed cases takes about four times what
/// 25 KB takes, not sixteen, on the server and in the browser's runtime: one comment was read to the
/// text's end for `--!>`, 290 to 380 ms for 14,280 `<!---->`, and each end tag read the whole stack
/// of open elements in runtime.js, 333 ms (both readings are gone with the plain-HTML rule); a `<`
/// that is text looks for the next `>` and value once (FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1176's final review; src/i18n.rs `find_attr_spans`; src/client/runtime.js
/// `findAttrSpans`; coverage of 0.8.9; branch feedback75).
#[test]
fn reading_a_translation_takes_time_in_step_with_its_length() {
    let (small, large) = (speed_cases(25_000), speed_cases(100_000));
    let floor = Duration::from_millis(5 * slow() as u64);
    for ((k, a), (_, b)) in small.iter().zip(&large) {
        let time = |t: &str| least(|| {
            let t0 = Instant::now();
            let spans = quillo::i18n::find_attr_spans(t);
            let took = t0.elapsed();
            assert!(spans.len() <= t.len());
            took
        });
        let (ta, tb) = (time(a), time(b));
        assert!(tb < ta * 10 + floor, "server {}: {:?} for {} bytes, {:?} for {}", k, ta, a.len(), tb, b.len());
    }
    // The runtime's reading, run by Node as runtime.js has it.
    let d = dir("speed-rt");
    let _gone = Gone(d.clone());
    let cases: serde_json::Map<String, serde_json::Value> = small.iter().map(|(k, v)| (format!("{}.small", k), serde_json::Value::String(v.clone()))).chain(large.iter().map(|(k, v)| (format!("{}.large", k), serde_json::Value::String(v.clone())))).collect();
    std::fs::write(d.join("cases.json"), serde_json::Value::Object(cases).to_string()).unwrap();
    std::fs::write(
        d.join("time.js"),
        r#"const fs = require("fs");
const src = fs.readFileSync(process.argv[2], "utf8").split("\n");
const a = src.findIndex((l) => /^const translate = \(k, vars, html\) => \{/.test(l)), b = src.findIndex((l, i) => i > a && /^B\.t = meta\(/.test(l));
const find = new Function(src.find((l) => /^const URL_ATTRS = /.test(l)) + "\n" + src.slice(a, b).join("\n") + "\nreturn findAttrSpans;")();
const out = {};
for (const [k, v] of Object.entries(JSON.parse(fs.readFileSync(process.argv[3], "utf8")))) {
  let best = Infinity;
  for (let r = 0; r < 3; r++) { const t0 = performance.now(); find(v); best = Math.min(best, performance.now() - t0); }
  out[k] = best;
}
console.log(JSON.stringify(out));
"#,
    )
    .unwrap();
    let rt = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/client/runtime.js");
    let mut child = Command::new("node").arg(d.join("time.js")).arg(rt).arg(d.join("cases.json")).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("node did not finish within 120 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut out).unwrap();
    let ms: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{}: {}", e, out));
    for (k, _) in &small {
        let (ta, tb) = (ms[format!("{}.small", k)].as_f64().unwrap(), ms[format!("{}.large", k)].as_f64().unwrap());
        assert!(tb < ta * 10.0 + 5.0 * slow() as f64, "runtime {}: {} ms for 25 KB, {} ms for 100 KB", k, ta, tb);
    }
}
