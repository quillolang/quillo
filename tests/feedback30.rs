//! A clinic's booking desk across five zones (FEEDBACK30.md, Q-361 to Q-377; branch
//! feedback27-35-fixes): uploads that cannot replace the program, locales chosen by their
//! script and region, CLDR's plural rules, counts given as text, times compared by instant,
//! numbers rounded as `Intl` rounds, a state that holds its prop written once, a page's own
//! translations, and the edges of each. The browser's side is in tests/browser/edges30.*.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use support::{send, serve};

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback30-{}", name));
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn run(d: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(d.join("main.qwe")).arg("--no-check").stdin(Stdio::null()).output().unwrap()
}

fn prints(name: &str, src: &str) -> String {
    let d = dir(name, &[("main.qwe", src)]);
    let o = run(&d);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn check(d: &Path) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// A multipart body of `(field, filename, bytes)` parts, and its content type.
fn multipart(parts: &[(&str, &str, &str)]) -> (String, String) {
    let b = "XqbOUNDARYq";
    let mut body = String::new();
    for (field, file, bytes) in parts {
        body.push_str(&format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: text/plain\r\n\r\n{}\r\n", b, field, file, bytes));
    }
    body.push_str(&format!("--{}--\r\n", b));
    (body, format!("multipart/form-data; boundary={}", b))
}

// ---- Q-361: an upload cannot replace the program -----------------------------------------------

const UPLOADS: &str = r#"import lib/a.qwe use word
db.open("app.db")
route POST /drop {
    let f = request.files.doc
    save_upload(f, f.filename) ?? "refused"
}
route POST /into ? { to: str } {
    save_upload(request.files.doc, to) ?? "refused"
}
route GET /write ? { p: str } = is_err(write_file(p, "x")) ? "refused" : "written"
route GET /delete ? { p: str } = is_err(delete_file(p)) ? "refused" : "deleted"
route GET / = word()
"#;

#[test]
fn an_upload_cannot_replace_the_programs_own_files() {
    let d = dir("own", &[("main.qwe", UPLOADS), ("lib/a.qwe", "fn word() = \"clinic\"\n"), (".env", "KEY=1\n"), ("migrations/001.sql", "select 1;\n")]);
    let s = serve(&d.join("main.qwe"), &[]);
    for name in ["main.qwe", ".env", "app.db", "app.db-wal"] {
        let (body, ctype) = multipart(&[("doc", name, "route GET / = \"evil\"")]);
        let a = send(s.port, "POST", "/drop", &[("Content-Type", &ctype)], &body);
        assert_eq!(a.body, "refused", "{}", name);
    }
    for to in ["lib/a.qwe", "./main.qwe", "migrations/002.sql", "lib/../main.qwe"] {
        let (body, ctype) = multipart(&[("doc", "x.qwe", "evil")]);
        assert_eq!(send(s.port, "POST", &format!("/into?to={}", to), &[("Content-Type", &ctype)], &body).body, "refused", "{}", to);
    }
    // A name of its own, under a folder of the program's, is saved as before; so is a .qwe file
    // the program did not load.
    let (body, ctype) = multipart(&[("doc", "report.pdf", "%PDF-1.4")]);
    assert_eq!(send(s.port, "POST", "/into?to=uploads/1.pdf", &[("Content-Type", &ctype)], &body).body, "uploads/1.pdf");
    let (body, ctype) = multipart(&[("doc", "other.qwe", "fn x() = 1")]);
    assert_eq!(send(s.port, "POST", "/drop", &[("Content-Type", &ctype)], &body).body, "other.qwe");
    // write_file and delete_file are refused the same files, a folder holding one included.
    assert_eq!(send(s.port, "GET", "/write?p=main.qwe", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/write?p=notes.txt", &[], "").body, "written");
    assert_eq!(send(s.port, "GET", "/delete?p=lib", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/delete?p=.env", &[], "").body, "refused");
    assert_eq!(send(s.port, "GET", "/delete?p=uploads/1.pdf", &[], "").body, "deleted");
    assert_eq!(std::fs::read_to_string(d.join("main.qwe")).unwrap(), UPLOADS);
    assert_eq!(std::fs::read_to_string(d.join(".env")).unwrap(), "KEY=1\n");
    assert!(d.join("lib/a.qwe").exists());
    assert_eq!(send(s.port, "GET", "/", &[], "").body, "clinic");
}

#[test]
fn check_warns_of_an_upload_saved_under_the_visitors_name() {
    let d = dir("visitor-name", &[(
        "main.qwe",
        "route POST /a { let f = request.files.doc\n  save_upload(f, f.filename) }\nroute POST /b { let f = request.files.doc\n  save_upload(f, `${request.form.folder}/${f.filename}`) }\nroute POST /c { let f = request.files.doc\n  save_upload(f, `uploads/${f.filename}`) }\n",
    )]);
    let out = check(&d);
    assert_eq!(out.matches("saved under the name the visitor gave").count(), 2, "{}", out);
    assert!(!out.contains("main.qwe:6:"), "a folder of the program's is fine: {}", out);
}

// ---- Q-362, Q-363: which locale serves a request -----------------------------------------------

#[test]
fn locales_are_chosen_by_their_script_and_region() {
    let mut files = vec![("main.qwe", "translations(\"locales\")\nroute GET / = `${locale()} ${t(\"hi\")}`\n")];
    let names = ["en_US", "fr_FR", "zh-Hans", "zh-Hant", "sr-Cyrl", "sr-Latn", "pt-BR", "pt-PT", "es", "es-419"];
    let jsons: Vec<(String, String)> = names.iter().map(|n| (format!("locales/{}.json", n), format!("{{ \"hi\": \"{}\" }}", n))).collect();
    for (p, j) in &jsons {
        files.push((p.as_str(), j.as_str()));
    }
    let d = dir("locales", &files);
    let s = serve(&d.join("main.qwe"), &[]);
    let served = |h: &str| send(s.port, "GET", "/", &[("Accept-Language", h)], "").body;
    for (asked, want) in [
        ("fr-FR", "fr-FR fr_FR"),
        ("fr", "fr-FR fr_FR"),
        ("zh-TW,zh;q=0.9", "zh-Hant zh-Hant"),
        ("zh-Hant-TW", "zh-Hant zh-Hant"),
        ("zh-HK", "zh-Hant zh-Hant"),
        ("zh-MO", "zh-Hant zh-Hant"),
        ("zh-CN", "zh-Hans zh-Hans"),
        ("zh-SG", "zh-Hans zh-Hans"),
        ("zh", "zh-Hans zh-Hans"),
        ("sr-Latn-RS", "sr-Latn sr-Latn"),
        ("sr", "sr-Cyrl sr-Cyrl"),
        ("pt-AO", "pt-PT pt-PT"),
        ("pt", "pt-BR pt-BR"),
        ("es-MX", "es-419 es-419"),
        ("es-ES", "es es"),
        ("de", "en-US en_US"),
        ("", "en-US en_US"),
    ] {
        assert_eq!(served(asked), want, "{}", asked);
    }
    // The cookie in either spelling.
    assert_eq!(send(s.port, "GET", "/", &[("Cookie", "locale=fr_FR")], "").body, "fr-FR fr_FR");
}

#[test]
fn the_same_locale_twice_is_refused() {
    let d = dir("twice", &[("main.qwe", "translations(\"locales\")\n"), ("locales/fr_FR.json", "{}"), ("locales/fr-FR.json", "{}")]);
    let o = run(&d);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("the locale \"fr-FR\" is given twice"), "{}", String::from_utf8_lossy(&o.stderr));
}

// ---- Q-366, Q-367: plural forms -----------------------------------------------------------------

#[test]
fn plural_forms_follow_each_languages_rules_and_zero_when_written() {
    let out = prints(
        "plurals",
        r#"translations({
    en: { msgs: { zero: "No messages", one: "{count} message", other: "{count} messages" }, plain: { one: "{count} item", other: "{count} items" } },
    ro: { z: { one: "{count} zi", few: "{count} zile", other: "{count} de zile" } },
    lt: { d: { one: "{count} diena", few: "{count} dienos", many: "{count} dienos*", other: "{count} dienų" } },
    lv: { d: { zero: "{count} z", one: "{count} o", other: "{count} t" } },
    sl: { d: { one: "{count} o", two: "{count} d", few: "{count} f", other: "{count} t" } },
    cy: { d: { zero: "{count} z", one: "{count} o", two: "{count} d", few: "{count} f", many: "{count} m", other: "{count} t" } },
    fr: { d: { one: "{count} o", many: "{count} m", other: "{count} t" } },
    "pt-PT": { d: { one: "{count} o", many: "{count} m", other: "{count} t" } },
    pt: { d: { one: "{count} o", many: "{count} m", other: "{count} t" } },
})
print(t("msgs", { count: 0 }), "|", t("msgs", { count: 1 }), "|", t("plain", { count: 0 }))
print(with_locale("ro", () => [0, 1, 2, 19, 20, 101, 119, 120] | map((n) => t("z", { count: n }))))
print(with_locale("lt", () => [1, 2, 9, 10, 11, 19, 21, 22] | map((n) => t("d", { count: n }))))
print(with_locale("lv", () => [0, 1, 10, 11, 21, 2] | map((n) => t("d", { count: n }))))
print(with_locale("sl", () => [1, 2, 3, 5, 101, 102] | map((n) => t("d", { count: n }))))
print(with_locale("cy", () => [0, 1, 2, 3, 6, 7] | map((n) => t("d", { count: n }))))
print(with_locale("fr", () => [0, 1, 2, 1000000] | map((n) => t("d", { count: n }))))
print(with_locale("pt-PT", () => [0, 1, 2] | map((n) => t("d", { count: n }))), with_locale("pt", () => [0, 1] | map((n) => t("d", { count: n }))))
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "No messages | 1 message | 0 items");
    assert_eq!(lines[1], r#"["0 zile", "1 zi", "2 zile", "19 zile", "20 de zile", "101 zile", "119 zile", "120 de zile"]"#);
    assert_eq!(lines[2], r#"["1 diena", "2 dienos", "9 dienos", "10 dienų", "11 dienų", "19 dienų", "21 diena", "22 dienos"]"#);
    assert_eq!(lines[3], r#"["0 z", "1 o", "10 z", "11 z", "21 o", "2 t"]"#);
    assert_eq!(lines[4], r#"["1 o", "2 d", "3 f", "5 t", "101 o", "102 d"]"#);
    assert_eq!(lines[5], r#"["0 z", "1 o", "2 d", "3 f", "6 m", "7 t"]"#);
    assert_eq!(lines[6], r#"["0 o", "1 o", "2 t", "1000000 m"]"#);
    assert_eq!(lines[7], r#"["0 t", "1 o", "2 t"] ["0 o", "1 o"]"#);
}

#[test]
fn a_form_the_language_never_picks_is_said_at_load() {
    let d = dir("unused-form", &[("main.qwe", "translations({ en: { a: { one: \"1\", few: \"f\", other: \"o\" }, b: { zero: \"z\", one: \"1\", other: \"o\" } }, ja: { a: { one: \"1\", other: \"o\" } } })\n")]);
    let o = run(&d);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("translations (en): `a` has a `few` form, which en never picks (it has one, other)"), "{}", err);
    assert!(err.contains("translations (ja): `a` has a `one` form, which ja never picks (it has other)"), "{}", err);
    // `zero` is read for 0 in any language: no word of it.
    assert!(!err.contains("`b`"), "{}", err);
}

#[test]
fn a_count_given_as_text_or_a_decimal_is_taken_at_its_number() {
    let out = prints(
        "counts",
        r#"translations({ en: { items: { one: "{count} item", other: "{count} items" } }, lt: { d: { one: "o", few: "f", many: "m", other: "t" } } })
print([t("items", { count: "1" }), t("items", { count: " 1 " }), t("items", { count: 1m }), t("items", { count: 1.0m }), t("items", { count: 1.5m }), t("items", { count: "1.5" }), t("items", { count: "x" }), t("items", { count: null }), t("items", { count: true }), t("items", {})])
print(with_locale("lt", () => [t("d", { count: 1.5 }), t("d", { count: "2" }), t("d", { count: 21m })]))
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], r#"["1 item", " 1  item", "1 item", "1.0 items", "1.5 items", "1.5 items", "x items", "null items", "true items", "{count} items"]"#);
    assert_eq!(lines[1], r#"["m", "f", "o"]"#);
}

// ---- Q-368, Q-375, Q-376: date-times ----------------------------------------------------------

#[test]
fn date_times_compare_by_instant_and_a_zero_fraction_is_dropped() {
    let out = prints(
        "compare",
        r#"let a = "2027-03-15T15:00:00Z"
let b = "2027-03-15T15:00:00.000Z"
let c = "2027-03-15T15:00:00.500Z"
print(date.to_zone(b, "UTC"), date.to_zone(c, "UTC"), date.to_zone("2027-03-15T15:00:00.0001Z", "UTC"))
print([date.compare(a, b), date.compare(c, a), date.compare(a, c), date.compare("2027-03-15T16:00:00+01:00", a), date.compare("2027-03-15", a), date.compare("2027-03-16", a)])
print(sort_by([c, a, "2027-03-15T14:59:59.999Z"], (t) => date.ms(t)))
print(date.compare("2027-03-15T15:00:00.123456789Z", "2027-03-15T15:00:00.123456788Z"))
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "2027-03-15T15:00:00Z 2027-03-15T15:00:00.500Z 2027-03-15T15:00:00.0001Z");
    assert_eq!(lines[1], "[0, 1, -1, 0, -1, 1]");
    assert_eq!(lines[2], r#"["2027-03-15T14:59:59.999Z", "2027-03-15T15:00:00Z", "2027-03-15T15:00:00.500Z"]"#);
    assert_eq!(lines[3], "1");
    let d = dir("compare-bad", &[("main.qwe", "date.compare(\"x\", \"2027-03-15\")\n")]);
    let o = run(&d);
    assert!(String::from_utf8_lossy(&o.stderr).contains("`date.compare` expects a date or a date-time as argument 1"));
}

#[test]
fn a_repeated_or_skipped_wall_time_is_read_as_asked() {
    let out = prints(
        "dst",
        r#"let ny = "America/New_York"
print(date.at("2026-11-01", "01:30", ny), date.at("2026-11-01", "01:30", ny, { repeated: "later" }), date.at("2026-11-01", "01:30", ny, { repeated: "earlier" }))
print(date.at("2026-03-08", "02:30", ny), date.at("2026-03-08", "02:30", ny, { skipped: "earlier" }), date.at("2026-03-08", "02:30", ny, { skipped: "error" }))
print(date.at("2026-07-01", "12:00", ny, { repeated: "later", skipped: "error" }), date.at("2026-11-01", "01:30", "UTC", { repeated: "later" }))
print(date.at("2026-10-27T14:30", ny), date.at("2026-10-27T14:30:05", ny), date.at("2026-10-27 14:30", ny), date.at("2026-11-01T01:30", ny, { repeated: "later" }))
print(date.at("2026-10-27T14:30", "Australia/Lord_Howe"), date.at("2026-10-04T02:15", "Australia/Lord_Howe", { skipped: "error" }))
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "2026-11-01T01:30:00-04:00 2026-11-01T01:30:00-05:00 2026-11-01T01:30:00-04:00");
    assert_eq!(lines[1], "2026-03-08T03:30:00-04:00 2026-03-08T01:30:00-05:00 err(\"date.at: 2026-03-08 02:30 is skipped in that zone, as its clocks move forward\")");
    assert_eq!(lines[2], "2026-07-01T12:00:00-04:00 2026-11-01T01:30:00Z");
    assert_eq!(lines[3], "2026-10-27T14:30:00-04:00 2026-10-27T14:30:05-04:00 2026-10-27T14:30:00-04:00 2026-11-01T01:30:00-05:00");
    assert!(lines[4].starts_with("2026-10-27T14:30:00+11:00 err(\"date.at: 2026-10-04 02:15 is skipped"), "{}", lines[4]);
    for (bad, said) in [
        ("date.at(\"2026-11-01\", \"01:30\", \"UTC\", { repeated: \"second\" })", "repeated is \"earlier\" or \"later\""),
        ("date.at(\"2026-11-01\", \"01:30\", \"UTC\", { when: 1 })", "the options are repeated and skipped, not `when`"),
        ("date.at(\"2026-11-01\", \"01:30\", \"UTC\", 3)", "expects options as a map as argument 4"),
        ("date.at(\"2026-10-27T25:30\", \"UTC\")", "expects a date"),
    ] {
        let d = dir("dst-bad", &[("main.qwe", &format!("{}\n", bad))]);
        let o = run(&d);
        assert!(String::from_utf8_lossy(&o.stderr).contains(said), "{}: {}", bad, String::from_utf8_lossy(&o.stderr));
    }
}

#[test]
fn a_local_date_time_is_refused_by_parse_and_an_http_date_read() {
    let out = prints(
        "parse",
        r#"print(is_err(date.parse("2026-10-27T14:30")), date.parse("2026-10-27T14:30").error)
print(date.parse("Tue, 27 Oct 2026 14:30:00 GMT"), is_err(date.parse("Tue, 32 Oct 2026 14:30:00 GMT")), is_err(date.parse("Tue, 27 Oct 2026 14:30:00 PST")))
print(date.parse("2026-10-27T14:30Z"), date.parse("2026-10-27"))
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "true \"2026-10-27T14:30\" has no offset, so it is a wall-clock time and names no instant: read it in its zone with date.at(v, zone)");
    assert_eq!(lines[1], "2026-10-27T14:30:00Z true true");
    assert_eq!(lines[2], "2026-10-27T14:30:00Z 2026-10-27");
}

// ---- Q-369: format_number ---------------------------------------------------------------------

#[test]
fn format_number_rounds_ties_away_from_zero_and_takes_least_and_most_digits() {
    let out = prints(
        "format",
        r#"print([2.5, 0.125, 1.125, 1.005, -2.5, -0.125] | map((x) => format_number(x, "en-US", 2)))
print(format_number(2.5, "en-US", 0), format_number(3.5, "en-US", 0), format_number(12.345, "de-DE", 2), format_number(2.5m, "en-US", 0), format_number(-2.5m, "en-US", 0))
print(format_number(10, "en-US", { max: 3 }), format_number(8.875, "en-US", { max: 3 }), format_number(8.8, "en-US", { min: 2, max: 3 }), format_number(8.8, "en-US", { min: 2 }), format_number(1234567.891, "en-US", { max: 1 }))
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], r#"["2.50", "0.13", "1.13", "1.01", "-2.50", "-0.13"]"#);
    assert_eq!(lines[1], "3 4 12,35 3 -3");
    assert_eq!(lines[2], "10 8.875 8.80 8.80 1,234,567.9");
    for (bad, said) in [
        ("format_number(1, \"en-US\", { min: 3, max: 1 })", "min 3 is more than max 1"),
        ("format_number(1, \"en-US\", { most: 1 })", "digits are `{ min, max }`, not `most`"),
        ("format_number(1, \"en-US\", { max: \"2\" })", "max is an int"),
    ] {
        let d = dir("format-bad", &[("main.qwe", &format!("{}\n", bad))]);
        assert!(String::from_utf8_lossy(&run(&d).stderr).contains(said), "{}", bad);
    }
}

// ---- Q-365, Q-373: files from a form ----------------------------------------------------------

const FILES: &str = r#"route POST /many { files: { docs: [map] } } = docs | map((d) => d.filename)
route POST /up = request.files.doc.filename
route POST /field = keys(request.files)
"#;

#[test]
fn a_list_pattern_takes_one_file_or_none_as_a_list() {
    let d = dir("many", &[("main.qwe", FILES)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let post = |parts: &[(&str, &str, &str)]| {
        let (body, ctype) = multipart(parts);
        send(s.port, "POST", "/many", &[("Content-Type", &ctype)], &body).body
    };
    assert_eq!(post(&[("docs", "a.txt", "a")]), r#"["a.txt"]"#);
    assert_eq!(post(&[("docs", "a.txt", "a"), ("docs", "b.png", "b")]), r#"["a.txt","b.png"]"#);
    assert_eq!(post(&[("other", "c.txt", "c")]), "[]");
    assert_eq!(send(s.port, "POST", "/many", &[], "").body, "[]");
}

#[test]
fn a_files_name_is_read_as_html_writes_it() {
    let d = dir("names", &[("main.qwe", FILES)]);
    let s = serve(&d.join("main.qwe"), &[]);
    let name = |n: &str| {
        let (body, ctype) = multipart(&[("doc", n, "x")]);
        send(s.port, "POST", "/up", &[("Content-Type", &ctype)], &body).body
    };
    assert_eq!(name("résumé %22final%22.pdf"), "résumé \"final\".pdf");
    assert_eq!(name("a;b.txt"), "a;b.txt");
    assert_eq!(name("C:\\fakepath\\report.pdf"), "report.pdf");
    assert_eq!(name("100%25.txt"), "100%25.txt", "only the three escapes HTML writes are read");
    let (body, ctype) = multipart(&[("my %22doc%22", "x.txt", "x")]);
    assert_eq!(send(s.port, "POST", "/field", &[("Content-Type", &ctype)], &body).body, r#"["my \"doc\""]"#);
}

// ---- Q-371, Q-372: what a page carries --------------------------------------------------------

#[test]
fn a_state_that_holds_its_prop_is_written_once() {
    let d = dir(
        "copy",
        &[(
            "main.qwe",
            r#"let rows = for i in 0..1000 => { id: i, text: `row number ${i}` }
component Copy(initial: list) {
    state items = initial
    <ul>{for r in items => <li key={r.id}>{r.text}</li>}</ul>
}
component Changed(initial: list) {
    state items = initial | push({ id: -1, text: "more" })
    <ul>{for r in items => <li key={r.id}>{r.text}</li>}</ul>
}
component Empty(initial: list) {
    state items = initial
    <ul>{for r in items => <li key={r.id}>{r.text}</li>}</ul>
}
route GET /copy = <html><body><Copy initial={rows}/></body></html>
route GET /changed = <html><body><Changed initial={rows}/></body></html>
route GET /empty = <html><body><Empty initial={[]}/></body></html>
"#,
        )],
    );
    let s = serve(&d.join("main.qwe"), &[]);
    let copy = send(s.port, "GET", "/copy", &[], "").body;
    assert!(copy.contains(r#"data-s="{&quot;items&quot;:{&quot;$quillo_prop&quot;:&quot;initial&quot;}}""#), "{}", &copy[..copy.len().min(400)]);
    assert!(copy.len() < 100_000, "{}", copy.len());
    // A state made from the prop, not the prop itself, is written whole; so is an empty list.
    assert!(send(s.port, "GET", "/changed", &[], "").body.len() > 150_000);
    assert!(send(s.port, "GET", "/empty", &[], "").body.contains(r#"data-s="{&quot;items&quot;:[]}""#));
}

#[test]
fn a_page_carries_the_translations_its_components_ask_for_in_a_file_kept() {
    let mut keys = String::new();
    for i in 0..300 {
        keys.push_str(&format!("\"k{}\": \"text {}\", ", i, i));
    }
    let d = dir(
        "keys",
        &[
            ("main.qwe", r#"translations("locales")
component Like() {
    state n = 0
    let part = "x"
    <button @click={() => { n += 1 }}>{t("k1")} {t(`sec.${part}`)} {n}</button>
}
component Any(name: str) { <p>{t(name)}</p> }
route GET / = <html><body><Like/><p>{t("k2")}</p></body></html>
route GET /any = <html><body><Any name="k3"/></body></html>
"#),
            ("locales/en.json", &format!("{{ {} \"sec\": {{ \"x\": \"X\", \"y\": \"Y\" }}, \"quillo\": {{ \"ok\": \"Okay\" }} }}", keys)),
        ],
    );
    let s = serve(&d.join("main.qwe"), &[]);
    let page = send(s.port, "GET", "/", &[], "").body;
    assert!(!page.contains("__quillo_i18n"), "the text is no longer inline");
    let at = page.find("/_quillo/t/").expect("a translations file");
    let path = &page[at..at + page[at..].find(".js").unwrap() + 3];
    let file = send(s.port, "GET", path, &[], "");
    assert_eq!(file.header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert!(file.body.contains("\"k1\":\"text 1\""), "{}", file.body);
    assert!(file.body.contains("\"sec.x\":\"X\"") && file.body.contains("\"sec.y\":\"Y\""), "{}", file.body);
    assert!(file.body.contains("\"quillo.ok\":\"Okay\""), "{}", file.body);
    assert!(!file.body.contains("\"k2\"") && !file.body.contains("\"k299\""), "{}", file.body);
    // A key worked out some other way: every key.
    let any = send(s.port, "GET", "/any", &[], "").body;
    let at = any.find("/_quillo/t/").unwrap();
    let path = &any[at..at + any[at..].find(".js").unwrap() + 3];
    assert!(send(s.port, "GET", path, &[], "").body.contains("\"k299\""));
    // One no longer kept: the default locale whole, not cached.
    let gone = send(s.port, "GET", "/_quillo/t/0123456789abcdef.js", &[], "");
    assert_eq!(gone.header("cache-control"), Some("no-cache"));
    assert!(gone.body.contains("\"k299\""));
}

// ---- Q-374: a decimal in the session ----------------------------------------------------------

#[test]
fn a_decimal_kept_in_the_session_comes_back_a_decimal() {
    let d = dir(
        "session",
        &[(
            "main.qwe",
            r#"route POST /cart { set_session({ total: 12.50m, big: 9007199254740993, list: [1.10m], err: null }); { same: session().total + 1m } }
route GET /cart {
    let s = session()
    { plus: s.total + 1m, sum: sum([s.total, 1m]), kind: type_of(s.total), big: s.big + 1, list: s.list[0] * 2 }
}
"#,
        )],
    );
    let s = serve(&d.join("main.qwe"), &[]);
    let set = send(s.port, "POST", "/cart", &[("Origin", "http://localhost")], "");
    assert_eq!(set.body, r#"{"same":"13.50"}"#);
    let cookie = set.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    let got = send(s.port, "GET", "/cart", &[("Cookie", &cookie)], "");
    assert_eq!(got.body, r#"{"plus":"13.50","sum":"13.50","kind":"dec","big":9007199254740994,"list":"2.20"}"#);
}
