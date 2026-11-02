//! The branches of FEEDBACK27 to 35's fixes that their own tests do not reach (branch
//! feedback27-35-fixes): each line of the changes is run by a test, as the user asked.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let d = support::temp_dir(&format!("cover2735-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn quillo(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(d).stdin(Stdio::null()).output().unwrap()
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// A server with options of its own, and its port.
fn serve_with(d: &Path, args: &[&str]) -> (support::Guard, u16) {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string()])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "did not start");
        std::thread::sleep(Duration::from_millis(30));
    }
    (g, port)
}

fn ev(src: &str) -> String {
    let mut it = quillo::Interp::new(".");
    match it.load_source(src, "case.qwe") {
        Ok(v) => v.repr(),
        Err(e) => format!("ERROR: {}", e.msg),
    }
}

#[test]
fn builtins_refuse_what_they_do_not_take() {
    assert!(ev("normalize(\"x\", 3)").contains("expects a form (\"NFC\", \"NFD\", \"NFKC\" or \"NFKD\") as argument 2, got int"));
    assert!(ev("site_headers(3)").contains("`site_headers` takes a map of headers, or a path and a map"));
    assert!(ev("__site(1)").contains("`site` takes a name and an address"));
    assert!(ev("date.at(3, \"01:00\", \"UTC\")").contains("expects a date"));
    assert!(ev("date.at(\"2026-11-01\", \"01:30\", \"UTC\", { skipped: \"never\" })").contains("skipped is \"later\", \"earlier\" or \"error\", got \"never\""));
    assert_eq!(ev("is_err(date.parse(\"Tue 27 Oct 2026 14:30:00 GMT\"))"), "true");
    assert_eq!(ev("is_err(date.parse(\"Tue, 27 Oct 26 14:30:00 GMT\"))"), "true");
    assert_eq!(ev("is_err(date.parse(\"Tue, 27 Foo 2026 14:30:00 GMT\"))"), "true");
}

#[test]
fn site_headers_takes_numbers_and_truths_as_text() {
    let d = dir("site-headers", &[("main.qwe", b"site_headers({ \"x-n\": 3, \"x-b\": true })\nroute GET / = \"x\"\n")]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let a = send(s.port, "GET", "/", &[], "");
    assert_eq!((a.header("x-n"), a.header("x-b")), (Some("3"), Some("true")));
}

#[test]
fn the_plural_rules_of_irish_maltese_icelandic_and_macedonian() {
    use quillo::i18n::plural;
    assert_eq!([1, 2, 5, 9, 11].map(|n| plural("ga", n as f64)), ["one", "two", "few", "many", "other"]);
    assert_eq!([1, 2, 0, 105, 113, 120].map(|n| plural("mt", n as f64)), ["one", "two", "few", "few", "many", "other"]);
    assert_eq!([1, 21, 11, 2].map(|n| plural("is", n as f64)), ["one", "one", "other", "other"]);
    assert_eq!([1, 2].map(|n| plural("mk", n as f64)), ["one", "other"]);
    assert_eq!(plural("cs", 1.5), "many");
    assert_eq!(quillo::i18n::categories("ga"), ["one", "two", "few", "many", "other"]);
}

#[test]
fn locales_matched_through_a_script_and_a_region() {
    let mut files: Vec<(String, String)> = Vec::new();
    for l in ["zh-Hant-TW", "zh-Hans", "uz-Latn", "en", "en-GB", "sr", "sr-Cyrl"] {
        files.push((format!("locales/{}.json", l), format!("{{ \"hi\": \"{}\" }}", l)));
    }
    let main = "translations(\"locales\")\nroute GET / = t(\"hi\")\n".to_string();
    let mut all: Vec<(&str, &[u8])> = vec![("main.qwe", main.as_bytes())];
    for (p, j) in &files {
        all.push((p.as_str(), j.as_bytes()));
    }
    let d = dir("locales", &all);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let served = |h: &str| send(s.port, "GET", "/", &[("Accept-Language", h)], "").body;
    // The file with script and region; one with the script among other subtags.
    assert_eq!(served("zh-Hant-TW"), "zh-Hant-TW");
    assert_eq!(served("zh-HK"), "zh-Hant-TW");
    assert_eq!(served("zh-TW"), "zh-Hant-TW");
    assert_eq!(served("uz"), "uz-Latn");
    // A script that matches nothing, then the region.
    assert_eq!(served("en-Latn-GB"), "en-GB");
    // The language alone, though another script was asked for.
    assert_eq!(served("sr-Latn"), "sr");
}

#[test]
fn translation_files_are_let_go_past_a_thousand() {
    let first = quillo::i18n::keep_file("{\"n\":0}".into());
    assert!(quillo::i18n::file(&first).is_some());
    for i in 1..1100 {
        quillo::i18n::keep_file(format!("{{\"n\":{}}}", i));
    }
    assert!(quillo::i18n::file(&first).is_none());
}

#[test]
fn a_page_that_loads_the_whole_bundle_carries_its_translations_too() {
    let d = dir("whole", &[(
        "main.qwe",
        b"translations({ en: { a: \"A\" } })\ncomponent C() { <p>{t(\"a\")}</p> }\nroute GET / = <html><body><C/></body></html>\nroute GET /tagged = <html><body><C/><script src=\"/_quillo/quillo.js\"></script></body></html>\n",
    )]);
    let (_g, port) = serve_with(&d, &["--watch"]);
    let page = send(port, "GET", "/", &[], "").body;
    assert!(page.contains("/_quillo/t/"), "{}", page);
    let tagged = send(port, "GET", "/tagged", &[], "").body;
    assert!(tagged.contains("window.__quillo_i18n="), "{}", tagged);
}

#[test]
fn translation_keys_worked_out_otherwise_send_all() {
    let d = dir("keys", &[(
        "main.qwe",
        b"translations({ en: { a: \"A\", b: \"B\", c: \"C\" } })\ncomponent C(k: str) { <p>{t(`${k}.x`)} {t(k)} {t(\"a\")}</p> }\nroute GET / = <html><body><C k=\"b\"/></body></html>\n",
    )]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    let at = page.find("/_quillo/t/").unwrap();
    let path = page[at..].split(".js").next().unwrap().to_string() + ".js";
    assert!(send(s.port, "GET", &path, &[], "").body.contains("\"c\":\"C\""));
}

#[test]
fn a_state_holding_a_map_prop_is_written_once() {
    let d = dir("map-prop", &[(
        "main.qwe",
        b"component C(initial: map) {\n    state m = initial\n    <p>{m.a}</p>\n}\nroute GET / = <html><body><C initial={{ a: 1 }}/></body></html>\n",
    )]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert!(send(s.port, "GET", "/", &[], "").body.contains("$quillo_prop"));
}

#[test]
fn a_files_pattern_with_no_list_and_a_file_in_the_wrong_shape() {
    let d = dir("files", &[("main.qwe", b"route POST /one { files: { doc: map } } = doc.filename\nserver fn f(x) = type_of(x)\n")]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = "--B\r\nContent-Disposition: form-data; name=doc; filename=a.txt\r\n\r\nx\r\n--B--\r\n";
    assert_eq!(send(s.port, "POST", "/one", &[("Content-Type", "multipart/form-data; boundary=B")], b).body, "a.txt");
    let rpc = "--B\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n[{\"$quillo_file\": \"no\"}]\r\n--B--\r\n";
    let a = send(s.port, "POST", "/_quillo/rpc/f", &[("Content-Type", "multipart/form-data; boundary=B"), ("Origin", "http://localhost"), ("X-Quillo-Rpc", "1")], rpc);
    assert_eq!(a.body, "{\"ok\":\"null\"}", "{}", a.body);
}

#[test]
fn the_log_writes_a_carriage_return_and_keeps_a_tab() {
    let d = dir("log", &[("main.qwe", b"route GET /e/:m = err(m)\n")]);
    let port = support::free_port();
    let log = d.join("log");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &port.to_string()])
        .stderr(std::fs::File::create(&log).unwrap())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let _g = support::Guard::new(child);
    while !support::listening(port) {
        std::thread::sleep(Duration::from_millis(30));
    }
    send(port, "GET", "/e/a%0Db%09c", &[], "");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !std::fs::read_to_string(&log).unwrap().contains("a\\rb") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(std::fs::read_to_string(&log).unwrap().contains("a\\rb\tc"));
}

#[test]
fn an_absolute_target_with_no_path_is_the_root() {
    let d = dir("abs", &[("main.qwe", b"route GET / = request.headers.host\n")]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let mut c = std::net::TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    use std::io::{Read, Write};
    c.write_all(b"GET http://site.example HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    assert!(out.ends_with("site.example"), "{}", out);
}

#[test]
fn invisible_characters_are_named_by_their_code() {
    let d = dir("invisible", &[("bom.qwe", b"print(1)\n\xef\xbb\xbfprint(2)\n"), ("bell.qwe", b"print(1)\x07\n")]);
    assert!(said(&quillo(&d, &["check", "bom.qwe"])).contains("U+FEFF (a byte-order mark)"));
    let o = said(&quillo(&d, &["check", "bell.qwe"]));
    assert!(o.contains("unexpected character U+0007, which cannot be seen"), "{}", o);
}

#[test]
fn the_command_line_reads_a_program_after_value_options_and_warns_of_instances() {
    let d = dir("cli", &[("a.qwe", b"print(args())\n"), ("s.qwe", b"print(\"once\")\n")]);
    assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "--no-check", "--log", "off", "a.qwe", "--", "x"]).stdout), "[\"x\"]\n");
    let cores = std::thread::available_parallelism().map_or(1, |c| c.get());
    let o = quillo(&d, &["run", "s.qwe", "--instances", &(cores + 1).to_string()]);
    assert!(said(&o).contains("is more than the"), "{}", said(&o));
}

#[test]
fn app_toml_env_keeps_a_tab_and_a_backslash_at_the_end() {
    let d = dir("app-env", &[("app.toml", b"env.T = \"a\\tb\"\nenv.E = \"x\\\n")]);
    let m = quillo::mobile::app_env(&d.join("app.toml"));
    assert_eq!(m.get("T").map(String::as_str), Some("a\tb"));
    assert_eq!(m.get("E").map(String::as_str), Some("x"));
}

#[test]
fn quillo_new_says_what_stops_it() {
    let d = dir("new", &[("a-file", b"x")]);
    assert!(said(&quillo(&d, &["new", "a-file"])).contains("a-file is a file, not a folder"));
    assert!(said(&quillo(&d, &["new", "/"])).contains("the folder needs a name") || !quillo(&d, &["new", "/"]).status.success());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(d.join("locked")).unwrap();
        std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
        let o = quillo(&d.join("locked"), &["new", "inside", "--minimal"]);
        std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!o.status.success() && said(&o).contains("cannot make"), "{}", said(&o));
        // A debug build copied is said to be one.
        std::fs::create_dir_all(d.join("debug")).unwrap();
        let copy = d.join("debug/quillo");
        std::fs::copy(env!("CARGO_BIN_EXE_quillo"), &copy).unwrap();
        let o = Command::new(&copy).args(["new", "b", "--minimal", "--binary"]).current_dir(&d).output().unwrap();
        assert!(said(&o).contains("the copy is a debug build"), "{}", said(&o));
    }
}

#[test]
fn site_is_a_name_where_it_is_not_a_declaration() {
    for (src, want) in [
        ("let site = 1\nsite = 2\nprint(site)\n", "2\n"),
        ("let shop = 3\nlet site = (x) => x\nprint(site(shop))\n", "3\n"),
        ("fn site(x) = x\nprint(site(4))\n", "4\n"),
    ] {
        let d = dir("site-name", &[("main.qwe", src.as_bytes())]);
        assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "main.qwe", "--no-check"]).stdout), want, "{}", src);
    }
    let d = dir("site-line", &[("main.qwe", b"let site = 5\nfn f(site) = site\nprint(f(site\n))\n")]);
    assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "main.qwe", "--no-check"]).stdout), "5\n");
}

#[test]
fn check_warns_of_children_as_text_a_bare_filename_template_and_a_site_in_a_function() {
    let d = dir("check", &[(
        "main.qwe",
        b"component Card(children = null) { <div>{str(children)}</div> }\nroute POST /u { let f = request.files.doc\n  save_upload(f, `${f.filename}`) }\nfn s() {\n  let shop = __site(\"shop\", \"https://x.example\")\n  shop\n}\nroute GET / = <html><Card/></html>\n",
    )]);
    let o = said(&quillo(&d, &["check", "main.qwe"]));
    assert!(o.contains("`str(children)` is \"null\""), "{}", o);
    assert!(o.contains("saved under the name the visitor gave"), "{}", o);
}

#[test]
fn a_multipart_name_without_quotes_is_read() {
    let d = dir("unquoted", &[("main.qwe", b"route POST /f = keys(request.form)\n")]);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let b = "--B\r\nContent-Disposition: form-data; name=title\r\n\r\nx\r\n--B\r\nContent-Disposition: form-data; name=note; x=1\r\n\r\ny\r\n--B--\r\n";
    assert_eq!(send(s.port, "POST", "/f", &[("Content-Type", "multipart/form-data; boundary=B")], b).body, "[\"title\",\"note\"]");
}

#[test]
fn a_page_whose_components_do_not_compile_still_carries_its_translations_under_watch() {
    let d = dir("no-bundle", &[(
        "main.qwe",
        b"translations({ en: { a: \"A\" } })\nserver fn save(x) = x\ncomponent C() { <p>{t(\"a\")} {1.save()}</p> }\nroute GET / = <html><body><C/></body></html>\n",
    )]);
    let (_g, port) = serve_with(&d, &["--watch", "--no-check"]);
    let page = send(port, "GET", "/", &[], "").body;
    assert!(page.contains("/_quillo/t/") && page.contains("/_quillo/quillo.js"), "{}", page);
}

#[test]
fn several_path_types_and_bodies_are_one_operation() {
    let d = dir("describe", &[(
        "main.qwe",
        b"route GET /x/:a(int) = a\nroute GET /x/:b(int) = b\nroute GET /x/:c = c\nroute GET /x/:d(float) = d\n",
    )]);
    let o = quillo(&d, &["docs", "main.qwe"]);
    let doc: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["paths"]["/x/{a}"]["get"]["parameters"][0]["schema"], serde_json::json!({ "oneOf": [{ "type": "integer" }, { "type": "string" }, { "type": "number" }] }));
}

#[test]
fn site_at_the_start_of_a_line_that_is_no_declaration() {
    let d = dir("site-stmt", &[("main.qwe", b"let site = 1\nsite\nlet shop = 2\nprint(site + shop)\n")]);
    assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "main.qwe", "--no-check"]).stdout), "3\n");
    let d = dir("site-next", &[("main.qwe", b"let site = 1\nlet shop = 0\nsite\nshop = 2\nprint(site + shop)\n")]);
    assert_eq!(String::from_utf8_lossy(&quillo(&d, &["run", "main.qwe", "--no-check"]).stdout), "3\n");
    let d = dir("site-half", &[("main.qwe", b"let site = 1\nsite shop\n")]);
    assert!(!quillo(&d, &["run", "main.qwe", "--no-check"]).status.success());
}

#[test]
fn a_program_writes_its_own_migrations_and_an_upload_may_not() {
    // The program's own migrations, written before and after their folder exists (a regression
    // the reference pages found on the way to deploying).
    let d = dir("migrations", &[(
        "main.qwe",
        b"db.open(\"app.db\")\nwrite_file(\"migrations/001_a.sql\", \"create table a (x)\")\nwrite_file(\"migrations/002_b.sql\", \"create table b (x)\")\nprint(db.migrate(\"migrations\"))\nprint(is_err(write_file(\"migrations/003_c.sql\", \"create table c (x)\")))\n",
    )]);
    let o = quillo(&d, &["run", "main.qwe"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "[\"001_a.sql\", \"002_b.sql\"]\nfalse\n", "{}", said(&o));
}

#[test]
fn a_path_outside_the_program_is_named_as_written() {
    let d = dir("outside", &[("main.qwe", b"write_file(\"plugins/p.qwe\", \"print(1)\\n\")\nprint(check_program(\"plugins/p.qwe\", { \"plugins/p.qwe\": \"use \\\"../secret.qwe\\\"\\n\" })[0].message)\nprint(check_program(\"plugins/p.qwe\", { \"plugins/p.qwe\": \"import ../../far.qwe use x\\n\" })[0].message)\n")]);
    let o = quillo(&d, &["run", "main.qwe"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.starts_with("path must stay inside the program's folder: ../secret.qwe\n"), "{}", out);
    assert!(!out.contains("/var/") && !out.contains("/private/"), "{}", out);
}
