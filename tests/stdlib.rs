//! The standard library's corners: what builtins do with null and with the
//! wrong kind of argument, values shown and compared, and the file functions.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::value::{self, Key, Scope, Value};
use quillo::{eval_str, Interp};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn ev(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn ev_err(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e.to_string(),
    }
}

#[track_caller]
fn fails(src: &str, wanted: &str) {
    let e = ev_err(src);
    assert!(e.contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e);
}

fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-stdlib-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn program(dir: &Path, src: &str) -> Interp {
    let mut it = Interp::new(dir);
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn request(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: Vec<u8>) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "x".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body, remote: "127.0.0.1".into() })
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    request(it, "GET", url, &[], vec![])
}

/// The message a top-level statement stops the program with.
fn load_error(dir: &Path, src: &str) -> String {
    let mut it = Interp::new(dir);
    match it.load_source(src, "app.qwe") {
        Ok(_) => panic!("expected an error\n{}", src),
        Err(e) => e.to_string(),
    }
}

/// `src`'s value, run in a program whose folder is `dir`.
fn run_in(dir: &Path, src: &str) -> String {
    let it = program(dir, &format!("let out = {}\n", src));
    value::lookup(&it.globals, "out").unwrap().repr()
}

#[test]
fn every_builtin_has_a_name_a_program_can_write() {
    let names = quillo::builtins::names();
    for name in ["len", "print", "date.add_days", "re.test", "db.query", "json.parse", "money.of", "t"] {
        assert!(names.iter().any(|n| n == name), "{} is missing", name);
    }
    assert!(names.windows(2).all(|w| w[0] < w[1]), "the names are sorted, each once");
    assert!(!names.iter().any(|n| n == "re"), "a module's name is not a function");
}

#[test]
fn numbers_convert_to_int_and_float() {
    assert_eq!(ev("[int(5), int(3.9), int(-3.9), int(true), int(false), int(2.5m), int(\"42\")]"), "[5, 3, -3, 1, 0, 2, 42]");
    assert_eq!(ev("int([1])"), "err(\"cannot convert list to int\")");
    assert_eq!(ev("int(null)"), "err(\"cannot convert null to int\")");
    assert_eq!(ev("[float(1.5), float(2), float(0.5m), float(\"2.5\")]"), "[1.5, 2.0, 0.5, 2.5]");
    assert_eq!(ev("float({})"), "err(\"cannot convert map to float\")");
}

/// The browser's `int` gives 1e30 (`Math.trunc`), and a dec this large is an
/// error value; a float is cut to the largest int, with nothing said.
#[test]
#[ignore = "finding: int(1.0e30) is silently 9223372036854775807, where a dec that large is an error value"]
fn a_float_too_large_for_an_int_is_an_error_value() {
    assert_eq!(ev("is_err(int(1.0e30))"), "true");
    assert_eq!(ev("is_err(int(-1.0e30))"), "true");
}

#[test]
fn len_and_contains_take_null_as_empty() {
    assert_eq!(ev("[len(null), len(\"héllo\"), len([1, 2]), len({ a: 1 }), len(raw(\"<b>\"))]"), "[0, 5, 2, 1, 3]");
    fails("len(5)", "`len` expects a str, list or map, got int");
    assert_eq!(ev("[contains({ a: 1 }, \"a\"), contains({ a: 1 }, \"b\"), contains(null, \"a\")]"), "[true, false, false]");
    fails("contains(5, 1)", "`contains` expects a str, list or map, got int");
}

#[test]
fn list_functions_take_null_as_an_empty_list() {
    assert_eq!(ev("[sum(null), first(null), count(null), sort(null), reverse([])]"), "[0, null, 0, [], []]");
    assert_eq!(ev("count([1, 2, 3])"), "3");
    assert_eq!(ev("count([1, 2, 3], (x) => x > 1)"), "2");
    fails("sum(5)", "`sum` expects a list as argument 1, got int");
    fails("slice(5, 0, 1)", "`slice` expects a list or str, got int");
    fails("reverse(5)", "`reverse` expects a list or str, got int");
    fails("index_of(5, 1)", "`index_of` expects a list or str, got int");
    fails("keys(5)", "`keys` expects a map as argument 1, got int");
    assert_eq!(ev("keys(null)"), "[]");
}

#[test]
fn all_asks_of_every_item() {
    assert_eq!(ev("all([1, 2, 3], (x) => x > 0)"), "true");
    assert_eq!(ev("all([1, 2, 3], (x) => x > 1)"), "false");
    assert_eq!(ev("all([], (x) => false)"), "true");
}

#[test]
fn sum_adds_floats_and_ints_as_plus_does() {
    assert_eq!(ev("sum([1.5, 2.5, 3.0])"), "7.0");
    assert_eq!(ev("sum([1, 2.5])"), "3.5");
    assert_eq!(ev("sum([1.5, 1])"), "2.5");
    assert_eq!(ev("sum([0.5m, 1])"), "1.5m");
    assert_eq!(ev("sum([\"a\", \"b\"])"), "\"0ab\"");
}

#[test]
fn sorting_refuses_what_cannot_be_compared() {
    fails("sort([1, \"a\"])", "`sort` cannot compare");
    fails("sort_by([1, \"a\"], (x) => x)", "`sort_by` cannot compare");
    assert!(ev_err("sort([1.5, 2.5m])").contains("float"), "a float and a dec are named");
}

#[test]
fn a_long_list_of_numbers_sorts_like_a_short_one() {
    let sorted = ev("let l = (0..30000) | map((i) => (i * 7919) % 30011)\nlet s = sort(l)\n[len(s), s[0] <= s[1], all(0..29999, (i) => s[i] <= s[i + 1]), sum(s) == sum(l)]");
    assert_eq!(sorted, "[30000, true, true, true]");
}

#[test]
fn spreading_takes_null_as_nothing() {
    assert_eq!(ev("let x = null\n[...x, 1]"), "[1]");
    assert_eq!(ev("let x = null\n{ ...x, a: 1 }"), "{ a: 1 }");
    fails("let x = 5\n{ ...x, a: 1 }", "`{...x}` spreads a map (or null) into a map, but x is int");
}

#[test]
fn sleep_takes_milliseconds() {
    assert_eq!(ev("sleep(1.5)"), "null");
    assert_eq!(ev("sleep(0)"), "null");
    fails("sleep(-1)", "`sleep` expects milliseconds (a number >= 0), got int");
    fails("sleep(\"soon\")", "`sleep` expects milliseconds (a number >= 0), got str");
}

#[test]
fn rounding_a_dec_says_what_is_wrong() {
    assert_eq!(ev("round(1.25m, 1)"), "1.2m");
    assert_eq!(ev("round(1.25m, null, null)"), "1m");
    fails("round(1.25m, \"x\")", "`round` expects an int as argument 2, got str");
    fails("round(1.25m, 1, 5)", "`round` expects a str as argument 3, got int");
}

#[test]
fn round_of_a_huge_float_stays_a_float() {
    assert_eq!(ev("round(1.0e20)"), ev("1.0e20"));
    assert_eq!(ev("type_of(round(1.0e20))"), "\"float\"");
    assert_eq!(ev("type_of(round(1.5e3))"), "\"int\"");
}

#[test]
fn pow_goes_to_a_float_when_an_int_cannot_hold_it() {
    assert_eq!(ev("pow(2, 10)"), "1024");
    assert_eq!(ev("type_of(pow(10, 30))"), "\"float\"");
    assert_eq!(ev("pow(10, 30) == 1.0e30"), "true");
}

#[test]
fn join_on_says_what_is_wrong() {
    let rows = "[{ user_id: 1 }, { user_id: 2 }]";
    let users = "[{ user_id: 1, name: \"Ada\" }]";
    assert_eq!(
        ev(&format!("join_on({rows}, {users}, \"user_id\")")),
        "[{ user_id: 1, user: { user_id: 1, name: \"Ada\" } }, { user_id: 2, user: null }]"
    );
    // Others with no such key, or not maps, match nothing.
    assert_eq!(ev(&format!("join_on({rows}, [5, {{ x: 1 }}, {{ id: 1 }}], \"user_id\", \"id\", \"who\")")), "[{ user_id: 1, who: { id: 1 } }, { user_id: 2, who: null }]");
    fails(&format!("join_on({rows}, {users}, \"user_id\", 5)"), "`join_on`'s fourth argument is the others' key (a str), got int");
    fails(&format!("join_on({rows}, {users}, \"user_id\", null, 5)"), "`join_on`'s fifth argument is the field to put the match in (a str), got int");
    fails(&format!("join_on([1], {users}, \"user_id\")"), "`join_on` joins maps (rows), got int");
}

#[test]
fn small_refusals() {
    fails("random_int(5, 1)", "`random_int` needs lo <= hi, got 5 and 1");
    fails("repeat(\"a\", -1)", "`repeat` count is negative or too large");
    fails("repeat(\"ab\", 100000000)", "`repeat` count is negative or too large");
    fails("js(5)", "`js` takes the name of what it calls first");
    fails("js_new()", "`js` takes the name of what it calls first");
    fails("sql_name()", "`sql_name` takes a name");
    fails("sql_param()", "`sql_param` takes a name");
    fails("navigate(5)", "`navigate` expects a str as argument 1, got int");
    fails("tenant_from(\"\")", "`tenant_from` needs a domain such as \"example.com\"");
    fails("tenant_from(\"a/b\")", "`tenant_from` needs a domain such as \"example.com\", got \"a/b\"");
    fails("tenant_from(5)", "`tenant_from` expects a domain or a function of the request, got int");
    fails("tenant_from()", "`tenant_from` expects a domain or a function of the request, got nothing");
    fails("with_tenant(\"acme\", 5)", "`with_tenant` expects a function second, got int");
    fails("with_tenant(\"acme\")", "`with_tenant` expects a function second, got nothing");
    fails("style_with(\"  \")", "`style_with` takes the command to run");
}

#[test]
fn what_only_the_browser_does() {
    fails("ask(\"Sure?\")", "`ask` and `notify` speak to someone in the browser");
    fails("notify(\"Saved\")", "`ask` and `notify` speak to someone in the browser");
    assert_eq!(ev("navigate(\"/next\")"), "null");
    assert_eq!(ev("waiting(1)"), "false");
    assert_eq!(ev("js(\"Chart\", 1)"), "null");
}

#[test]
fn a_browser_librarys_name_is_null_on_the_server() {
    assert_eq!(ev("use Chart from \"https://example.com/chart.js\"\nChart(1, 2)"), "null");
}

#[test]
fn every_in_a_request_belongs_to_the_browser() {
    let d = folder("every");
    let mut it = program(&d, "route GET /good = str(every(100, () => 1))\nroute GET /bad = str(every(1, 2))\n");
    assert_eq!(get(&mut it, "/good").text(), "null");
    assert_eq!(get(&mut it, "/bad").status, 500);
}

#[test]
fn highlight_with_no_language_only_escapes() {
    assert_eq!(ev("str(highlight(\"a < b\"))"), ev("str(highlight(\"a < b\", null))"));
    assert!(ev("str(highlight(\"a < b\"))").contains("a &lt; b"));
}

#[test]
fn live_calls_a_server_fn_and_gives_its_answer() {
    assert_eq!(ev("server fn load() = 5\nlive(load)"), "5");
    // One inside another, and a loader that gives an error value.
    assert_eq!(ev("server fn inner() = 5\nserver fn outer() = live(inner) + 1\nlive(outer)"), "6");
    assert_eq!(ev("server fn no() = err(\"nope\")\nlive(no)"), "err(\"nope\")");
    fails("server fn broken() = 1 / 0\nlive(broken)", "division by zero");
    fails("live(len)", "`live` needs a server fn to call, like live(load_notes), got fn");
    fails("fn plain() = 1\nlive(plain)", "`live` needs a server fn to call, like live(load_notes), got fn");
    fails("live()", "`live` needs a server fn to call, like live(load_notes)");
}

#[test]
fn respond_keeps_to_real_statuses() {
    let d = folder("respond");
    let mut it = program(&d, "route GET /a = respond(700, \"x\")\n");
    assert_eq!(get(&mut it, "/a").status, 500);
    fails("respond(99, \"x\")", "`respond` status must be 100-599, got 99");
    fails("respond(700, \"x\")", "`respond` status must be 100-599, got 700");
}

#[test]
fn file_options_are_a_map_with_a_real_duration() {
    let d = folder("file");
    std::fs::write(d.join("a.txt"), "hello").unwrap();
    let mut it = program(
        &d,
        r#"
route GET /plain = file("a.txt", {})
route GET /nil = file("a.txt", { cache: null })
route GET /day = file("a.txt", { cache: "1d" })
route GET /week = file("a.txt", { cache: "2w" })
route GET /seconds = file("a.txt", { cache: 60 })
route GET /kind = file("a.txt", { cache: true })
"#,
    );
    let cache_of = |it: &mut Interp, url: &str| {
        let r = get(it, url);
        assert_eq!((r.status, r.text()), (200, "hello".to_string()), "{}", url);
        r.headers.iter().find(|(k, _)| k == "cache-control").map(|(_, v)| v.clone()).unwrap()
    };
    assert_eq!(cache_of(&mut it, "/plain"), "no-cache");
    assert_eq!(cache_of(&mut it, "/nil"), "no-cache");
    assert_eq!(cache_of(&mut it, "/day"), "public, max-age=86400, immutable");
    assert_eq!(cache_of(&mut it, "/week"), "public, max-age=1209600, immutable");
    assert_eq!(cache_of(&mut it, "/seconds"), "public, max-age=60, immutable");
    assert_eq!(get(&mut it, "/kind").status, 500);
    for (src, wanted) in [
        ("file(\"a.txt\", { cache: \"5x\" })", "`file`'s cache is seconds or a duration like \"30d\", got \"5x\""),
        ("file(\"a.txt\", { cache: true })", "`file`'s cache is seconds or a duration like \"30d\", got true"),
        ("file(\"a.txt\", { cache: -1 })", "`file`'s cache is seconds or a duration like \"30d\", got -1"),
        ("file(\"a.txt\", 5)", "`file` expects options as a map, got int"),
    ] {
        let e = load_error(&d, src);
        assert!(e.contains(wanted), "{}: {}", src, e);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `n * per` overflows: a panic where overflow is checked, and a meaningless
/// max-age where it wraps.
#[test]
#[ignore = "finding: a cache duration too long for a u64 overflows instead of being refused"]
fn a_cache_too_long_to_count_is_refused() {
    let d = folder("file-overflow");
    std::fs::write(d.join("a.txt"), "hello").unwrap();
    let e = load_error(&d, "file(\"a.txt\", { cache: \"999999999999y\" })");
    assert!(e.contains("`file`'s cache is seconds or a duration like \"30d\""), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_read_is_not_found() {
    use std::os::unix::fs::PermissionsExt;
    let d = folder("unreadable");
    std::fs::write(d.join("secret.txt"), "hello").unwrap();
    std::fs::set_permissions(d.join("secret.txt"), std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(d.join("secret.txt")).is_ok() {
        return; // Run as root, which permissions do not stop.
    }
    let mut it = program(&d, "route GET /f = file(\"secret.txt\")\n");
    assert_eq!(get(&mut it, "/f").status, 404);
    assert_eq!(request(&mut it, "GET", "/f", &[("Range", "bytes=0-1")], vec![]).status, 404);
    std::fs::set_permissions(d.join("secret.txt"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn files_are_written_and_deleted_inside_the_program_folder() {
    let d = folder("files");
    std::fs::create_dir_all(d.join("sub/inner")).unwrap();
    std::fs::write(d.join("sub/inner/a.txt"), "a").unwrap();
    std::fs::write(d.join("b.txt"), "b").unwrap();
    assert_eq!(run_in(&d, "delete_file(\"b.txt\")"), "null");
    assert!(!d.join("b.txt").exists());
    assert_eq!(run_in(&d, "delete_file(\"sub\")"), "null");
    assert!(!d.join("sub").exists());
    assert_eq!(run_in(&d, "delete_file(\"never-was.txt\")"), "null");
    let e = run_in(&d, "delete_file(\"../x\")");
    assert_eq!(e, "err(\"delete_file: path must be inside the program's folder: ../x\")");
    let e = run_in(&d, "delete_file(\".\")");
    assert_eq!(e, "err(\"delete_file: path must be inside the program's folder: .\")");
    assert!(d.exists(), "the folder itself stays");

    std::fs::create_dir_all(d.join("dir")).unwrap();
    let e = run_in(&d, "write_file(\"dir\", \"x\")");
    assert!(e.starts_with("err(\"write_file: dir: "), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(unix)]
#[test]
fn a_delete_the_system_refuses_is_an_error_value() {
    use std::os::unix::fs::PermissionsExt;
    let d = folder("locked");
    std::fs::create_dir_all(d.join("locked")).unwrap();
    std::fs::write(d.join("locked/a.txt"), "a").unwrap();
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let e = run_in(&d, "delete_file(\"locked/a.txt\")");
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    if !d.join("locked/a.txt").exists() {
        return; // Run as root, which permissions do not stop.
    }
    assert!(e.starts_with("err(\"delete_file: locked/a.txt: "), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn check_program_stays_inside_the_folder() {
    let d = folder("check");
    std::fs::write(d.join("a.qwe"), "fn f() = 1 +\n").unwrap();
    let found = run_in(&d, "check_program(\"a.qwe\")");
    assert!(found.contains("severity: \"error\""), "{}", found);
    assert!(found.contains("file: \"a.qwe\""), "{}", found);
    assert_eq!(run_in(&d, "check_program(\"a.qwe\", { \"a.qwe\": \"fn f() = 1\\n\" })"), "[]");
    assert_eq!(run_in(&d, "check_program(\"../x.qwe\")"), "err(\"check_program: path must stay inside the program's folder: ../x.qwe\")");
    assert_eq!(
        run_in(&d, "check_program(\"a.qwe\", { \"../x.qwe\": \"\" })"),
        "err(\"check_program: path must stay inside the program's folder: ../x.qwe\")"
    );
    let _ = std::fs::remove_dir_all(&d);
}

fn multipart(name: &str, filename: &str, data: &[u8]) -> Vec<u8> {
    let mut b = format!("--XyZ\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: text/plain\r\n\r\n", name, filename).into_bytes();
    b.extend_from_slice(data);
    b.extend_from_slice(b"\r\n--XyZ--\r\n");
    b
}

#[test]
fn uploads_outside_a_request_or_not_from_it_are_error_values() {
    assert_eq!(ev("save_upload({}, \"x.txt\")"), "err(\"save_upload: the current request has no files\")");
    assert_eq!(ev("read_upload({})"), "err(\"read_upload: the current request has no files\")");
    let d = folder("uploads");
    std::fs::write(d.join("taken"), "a file, not a folder").unwrap();
    let mut it = program(
        &d,
        r#"
route POST /not-a-file = str(save_upload(5, "x.txt"))
route POST /read-not-a-file = str(read_upload({ start: 1 }))
route POST /past-the-end = str(read_upload({ start: 0, end: 100000 }))
route POST /no-room = str(save_upload(request.files.a, "taken/x.txt"))
"#,
    );
    let body = multipart("a", "a.txt", b"hello");
    let post = |it: &mut Interp, url: &str| {
        request(it, "POST", url, &[("Content-Type", "multipart/form-data; boundary=XyZ")], body.clone()).text()
    };
    assert_eq!(post(&mut it, "/not-a-file"), "err(\"save_upload: expects a file from request.files\")");
    assert_eq!(post(&mut it, "/read-not-a-file"), "err(\"read_upload: expects a file from request.files\")");
    assert_eq!(post(&mut it, "/past-the-end"), "err(\"read_upload: this file is not part of the current request\")");
    assert!(post(&mut it, "/no-room").starts_with("err(\"save_upload: "), "{}", post(&mut it, "/no-room"));
    assert_eq!(std::fs::read_to_string(d.join("taken")).unwrap(), "a file, not a folder");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn keys_compare_with_text() {
    let k = Key::from("name");
    assert!(k == *"name");
    assert!(k == "name");
    assert!(k != "other");
    assert_eq!(format!("{:?}", k), "\"name\"");
    let long = Key::from("a key longer than twenty-two bytes");
    assert_eq!(format!("{:?}", long), "\"a key longer than twenty-two bytes\"");
    assert_eq!(String::from(long), "a key longer than twenty-two bytes");
}

#[test]
fn functions_and_responses_have_a_type_and_a_text() {
    assert_eq!(ev("[type_of(len), type_of(() => 1), type_of(redirect(\"/x\"))]"), "[\"fn\", \"fn\", \"response\"]");
    assert_eq!(ev("str(len)"), "\"<builtin len>\"");
    assert_eq!(ev("str(respond(404, \"no\"))"), "\"<response 404>\"");
    assert_eq!(ev("str([1, \"a\"])"), "\"[1, \\\"a\\\"]\"");
    assert_eq!(ev("str(err(\"no\"))"), "\"err(\\\"no\\\")\"");
    assert_eq!(ev("str({ a: 1 })"), "\"{ a: 1 }\"");
}

#[test]
fn json_writes_what_has_no_json_form() {
    assert_eq!(ev("json.stringify(respond(404, \"no\"))"), "\"{\\\"status\\\":404}\"");
    assert_eq!(ev("json.stringify({ r: respond(201, \"x\") })"), "\"{\\\"r\\\":{\\\"status\\\":201}}\"");
    assert_eq!(ev("json.stringify(len)"), "\"\\\"<builtin len>\\\"\"");
    assert_eq!(ev("fn f() = 1\njson.stringify([f])"), "\"[\\\"<fn f>\\\"]\"");
    let v = eval_str("respond(404, \"no\")").unwrap();
    assert_eq!(v.to_json(), serde_json::json!({ "status": 404 }));
    assert_eq!(v.repr(), "<response 404>");
}

#[test]
fn a_route_answers_json_for_what_has_no_json_form() {
    let d = folder("json");
    let mut it = program(&d, "route GET /j = { r: respond(201, \"x\"), f: len }\n");
    assert_eq!(get(&mut it, "/j").text(), "{\"r\":{\"status\":201},\"f\":\"<builtin len>\"}");
}

#[test]
fn numbers_read_as_javascript_writes_them() {
    assert_eq!(value::js_number(f64::NAN), "NaN");
    assert_eq!(value::js_number(f64::INFINITY), "Infinity");
    assert_eq!(value::js_number(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(value::js_number(0.0), "0");
    assert_eq!(value::js_number(-0.0), "0");
    assert_eq!(value::js_number(1.5), "1.5");
    assert_eq!(value::js_number(100.0), "100");
}

#[test]
fn equality_of_html_errors_and_functions() {
    assert_eq!(ev("raw(\"<b>\") == raw(\"<b>\")"), "true");
    assert_eq!(ev("raw(\"<b>\") == raw(\"<i>\")"), "false");
    assert_eq!(ev("err(\"a\") == err(\"a\")"), "true");
    assert_eq!(ev("err(\"a\") == err(\"b\")"), "false");
    assert_eq!(ev("let f = () => 1\nf == f"), "true");
    assert_eq!(ev("let f = () => 1\nlet g = () => 1\nf == g"), "false");
    assert_eq!(ev("len == len"), "true");
    assert_eq!(ev("len == first"), "false");
    assert_eq!(ev("0.5m == \"0.5\""), "false");
}

#[test]
fn a_long_variable_name_and_many_variables_work_like_any_others() {
    let name = "a_variable_with_a_name_longer_than_twenty_two_bytes";
    assert_eq!(ev(&format!("fn f() {{\n let {name} = 41\n {name} += 1\n {name}\n}}\nf()")), "42");
    // A scope past sixteen variables, and used again after it ends.
    let lets: String = (0..24).map(|i| format!(" let v{} = {}\n", i, i)).collect();
    let total: Vec<String> = (0..24).map(|i| format!("v{}", i)).collect();
    let src = format!("fn f() {{\n{} {}\n}}\n[f(), f(), f()]", lets, total.join(" + "));
    assert_eq!(ev(&src), "[276, 276, 276]");
    let mut it = Interp::new(".");
    it.load_source(&src, "many.qwe").unwrap();
}

#[test]
fn a_scope_used_again_starts_empty() {
    // More than the first few variables, with lists large enough to free apart.
    let src = "fn f(n) {
 let a = [1, 2, 3, 4, 5, 6, 7, 8, n]
 let b = [1, 2, 3, 4, 5, 6, 7, 8, n]
 let c = [1, 2, 3, 4, 5, 6, 7, 8, n]
 let d = [1, 2, 3, 4, 5, 6, 7, 8, n]
 let e = [1, 2, 3, 4, 5, 6, 7, 8, n]
 let g = [1, 2, 3, 4, 5, 6, 7, 8, n]
 len(a) + len(b) + len(c) + len(d) + len(e) + len(g) + g[8]
}
[f(1), f(2)]";
    value::defer_drops();
    assert_eq!(ev(src), "[55, 56]");
    let kept = value::deferred_drops();
    assert!(!kept.is_empty(), "large lists wait to be freed elsewhere");
    assert!(kept.iter().all(|v| matches!(v, Value::List(l) if l.len() == 9)), "{}", kept.len());
}

#[test]
fn assign_changes_a_variable_where_it_lives() {
    let root = Scope::root();
    value::define(&root, "x", Value::Int(1));
    let inner = Scope::child(&root);
    assert!(value::assign(&inner, "x", Value::Int(2)));
    assert_eq!(value::lookup(&root, "x").map(|v| v.repr()).as_deref(), Some("2"));
    assert!(!value::assign(&inner, "missing", Value::Int(2)));
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A `quillo run` of `file` with what it says going to `log`.
fn run_server(file: &Path, log: &Path, more: &[&str]) -> (support::Guard, u16) {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(more)
        .env("QUILLO_SECRET", "test secret for the tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(log).unwrap())
        .spawn()
        .unwrap();
    (support::Guard::new(child), port)
}

#[cfg(unix)]
#[test]
fn a_ticks_sleep_ends_when_the_server_stops() {
    let d = folder("tick-sleep");
    let f = d.join("main.qwe");
    std::fs::write(&f, "fn step() {\n write_file(\"began\", \"1\")\n sleep(600000)\n write_file(\"went-on\", \"1\")\n}\nevery(10, step)\nroute GET / = \"ok\"\n").unwrap();
    let (mut server, _port) = run_server(&f, &d.join("log"), &[]);
    wait_for("the tick did not start", || d.join("began").exists());
    assert!(Command::new("kill").args(["-TERM", &server.0.id().to_string()]).status().unwrap().success());
    let status = loop {
        if let Some(s) = server.0.try_wait().unwrap() {
            break s;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{:?}: {}", status, std::fs::read_to_string(d.join("log")).unwrap());
    assert!(!d.join("went-on").exists(), "the code after the sleep does not run");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn under_watch_a_change_to_a_served_file_refreshes_the_pages() {
    let d = folder("watch-served");
    let f = d.join("main.qwe");
    std::fs::write(d.join("note.txt"), "first").unwrap();
    // One `file()` with no request in hand, as well: there is no path to note.
    std::fs::write(&f, "let first = file(\"note.txt\")\nroute GET /note = file(\"note.txt\")\n").unwrap();
    let log = d.join("log");
    let (_server, port) = run_server(&f, &log, &["--watch"]);
    wait_for("the server did not start", || support::listening(port));
    wait_for("the file was not served", || support::send(port, "GET", "/note", &[], "").body == "first");
    // Changed again and again: the first look at a file only notes its time.
    // Asked for each time too: the watcher starts on a thread of its own, and
    // a file served before it has does not count as served.
    let mut n = 0;
    wait_for("the change was not noticed", || {
        n += 1;
        support::send(port, "GET", "/note", &[], "");
        std::fs::write(d.join("note.txt"), format!("change {}", n)).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        std::fs::read_to_string(&log).unwrap().contains("/note changed: refreshing open pages")
    });
    assert!(support::send(port, "GET", "/note", &[], "").body.starts_with("change "));
    // Loaded again, as a new version, the top level's `file()` has no request either.
    std::fs::write(&f, "let first = file(\"note.txt\")\nroute GET /note = \"new version\"\n").unwrap();
    wait_for("the new version was not loaded", || support::send(port, "GET", "/note", &[], "").body == "new version");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_record_is_told_what_is_wrong_in_a_list_of_shapes() {
    let types = "type Line = { sku: str, qty: int? }\ntype Order = { lines: list<{ sku: str, note: str? }> }\ntype Tag = \"a\" | \"b\"\n";
    let run = |body: &str| ev(&format!("{}{}", types, body));
    assert_eq!(run("Order({ lines: [{ sku: \"x\" }] })"), "{ lines: [{ sku: \"x\" }] }");
    assert_eq!(run("Order({ lines: [{ note: \"x\" }] })"), "err(\"Order: `lines[0].sku` is missing\")");
    assert_eq!(run("Order({ lines: [{ sku: 5 }] })"), "err(\"Order: `lines[0].sku` must be str, got int 5\")");
    assert_eq!(run("Line(5)"), "err(\"Line: expects a map of its fields, got int 5\")");
    assert_eq!(run("Line()"), "err(\"Line: `sku` is missing\")");
    assert_eq!(run("Line(null)"), "err(\"Line: `sku` is missing\")");
    assert_eq!(run("Tag(\"a\")"), "\"a\"");
    assert_eq!(run("Tag(\"c\")"), "err(\"Tag: must be \\\"a\\\" | \\\"b\\\", got str \\\"c\\\"\")");
    assert_eq!(run("Tag()"), "err(\"Tag: must be \\\"a\\\" | \\\"b\\\", got null null\")");
}

#[test]
fn a_bare_list_field_takes_any_items() {
    let src = "type Bag = { xs: list }\nfn f(b: Bag) = b\nf({ xs: [1, \"a\"] })";
    assert_eq!(ev(src), "{ xs: [1, \"a\"] }");
}

#[test]
fn a_list_of_optional_records_keeps_its_nulls() {
    let types = "type Line = { sku: str }\ntype Bag = { lines: list<Line?> }\n";
    assert_eq!(ev(&format!("{}Bag({{ lines: [null, {{ sku: \"a\" }}] }})", types)), "{ lines: [null, { sku: \"a\" }] }");
    assert_eq!(ev(&format!("{}Bag({{ lines: [{{ sku: 5 }}] }})", types)), "err(\"Bag: `lines[0].sku` must be str, got int 5\")");
    // A union, or `?`, after the list: see the next test for one that ends the line.
    assert_eq!(ev("type Lines = list<int> | str\nLines(\"a\")"), "\"a\"");
}

/// A line that ends with `>` goes on to the next, as for the comparison.
#[test]
#[ignore = "finding: `type Ids = list<int>` at the end of a line does not parse: the newline after `>` is taken as a continuation"]
fn a_list_type_can_end_a_line() {
    assert_eq!(ev("type Ids = list<int>\nlet x = 1\nIds([x])"), "[1]");
}

#[test]
fn a_type_that_is_not_a_record_can_be_called_with_nothing() {
    assert_eq!(ev("type Tag = \"a\" | \"b\"\ntype Maybe = str?\n[__record(\"Maybe\"), is_err(__record(\"Tag\"))]"), "[null, true]");
}

#[test]
fn a_long_value_is_cut_short_in_a_message() {
    let e = ev("type Line = { sku: str }\nLine({ sku: [repeat(\"x\", 100)] })");
    assert!(e.starts_with("err(\"Line: `sku` must be str, got list [\\\"xxx"), "{}", e);
    assert!(e.ends_with("xxx…\")"), "{}", e);
}

#[test]
fn annotations_look_inside_a_map_shape() {
    let src = "fn f(m: { a: int, b: str? }) = m\nf({ a: 1 })";
    assert_eq!(ev(src), "{ a: 1 }");
    let types = "type Box = { inner: { a: int, b: str? } }\n";
    assert_eq!(ev(&format!("{}fn f(b: Box) = b\nf({{ inner: {{ a: 1 }} }})", types)), "{ inner: { a: 1 } }");
    fails(&format!("{}fn f(b: Box) = b\nf({{ inner: {{ b: \"x\" }} }})", types), "`inner.a` is missing");
}

#[test]
fn a_record_with_no_such_name_cannot_be_made() {
    fails("__record(5)", "`__record` is called by a record's name, as in User({ ... })");
    fails("__record()", "`__record` is called by a record's name");
    fails("__record(\"Nothing\")", "there is no type `Nothing`");
}

#[test]
fn crypto_says_what_it_takes() {
    fails("crypto.sha256(5)", "`crypto.sha256` expects a str as argument 1, got int");
    fails("crypto.hmac(\"key\")", "`crypto.hmac` expects a str as argument 2, got nothing");
    fails("crypto.token(0)", "`crypto.token` takes a number of bytes from 1 to 1024, got 0");
    fails("crypto.token(1025)", "`crypto.token` takes a number of bytes from 1 to 1024, got 1025");
    fails("crypto.token(\"x\")", "`crypto.token` takes a number of bytes from 1 to 1024, got \"x\"");
}

#[test]
fn passwords_are_hashed_in_turn() {
    // More at once than there are cores: the rest wait, and all finish.
    let n = std::thread::available_parallelism().map_or(4, |n| n.get()) + 3;
    let threads: Vec<_> = (0..n)
        .map(|i| std::thread::spawn(move || ev(&format!("crypto.verify_password(\"p{i}\", crypto.hash_password(\"p{i}\"))"))))
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), "true");
    }
}

#[test]
fn highlighting_html_comments_inside_quillo() {
    let html = ev("str(highlight(\"<!-- note --> let a = 1\", \"quillo\"))");
    assert!(html.contains("hl-c"), "{}", html);
    assert!(html.contains("&lt;!-- note --&gt;"), "{}", html);
    let open = ev("str(highlight(\"<!-- never closed\", \"quillo\"))");
    assert!(open.contains("&lt;!-- never closed"), "{}", open);
}

#[test]
fn highlighting_a_less_than_is_no_tag() {
    let html = ev("str(highlight(\"1 < 2\", \"quillo\"))");
    assert!(html.contains("&lt;") && !html.contains("hl-t"), "{}", html);
}

#[test]
fn highlighting_a_string_with_an_escape() {
    let html = ev("str(highlight(\"let a = \\\"x\\\\\\\"y\\\" + 1\", \"quillo\"))");
    assert!(html.contains("hl-s\\\">&quot;x\\\\&quot;y&quot;</span>"), "{}", html);
}
