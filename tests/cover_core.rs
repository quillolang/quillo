//! The last corners of the language's core, each by what a user sees: the
//! checker, the browser compiler, the interpreter, the parser, the editor's
//! search for a program's main file, and the reading of Rust files.

mod support;

use quillo::check::{check_file, check_file_opts, check_source, Diagnostic, Options, Severity};
use quillo::client;
use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::{Path, PathBuf};
use support::temp_dir;

fn shown(d: &Diagnostic) -> String {
    let sev = match d.severity {
        Severity::Error => "",
        Severity::Warning => " warning:",
        Severity::Hint => " hint:",
    };
    format!("{}{} {}", d.line, sev, d.message)
}

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(shown).collect()
}

/// A folder of files for one test, gone when it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str, files: &[(&str, &str)]) -> Folder {
        let d = temp_dir(&format!("cover-core-{}", name));
        for (f, text) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Folder(d.canonicalize().unwrap())
    }

    fn join(&self, f: &str) -> PathBuf {
        self.0.join(f)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn loaded(src: &str) -> Interp {
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src));
    it
}

fn value(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn bundle(src: &str) -> String {
    client::bundle(&loaded(src)).unwrap_or_else(|e| panic!("{}\n--- source ---\n{}", e, src))
}

/// What loading a program says: the browser's code is compiled with it.
fn refused(src: &str) -> String {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(_) => panic!("it loaded\n--- source ---\n{}", src),
        Err(e) => e.to_string(),
    }
}

fn get(it: &mut Interp, url: &str) -> String {
    let headers = vec![("Host".to_string(), "localhost".to_string())];
    let r: HttpResponse = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers, body: Vec::new(), remote: "127.0.0.1".into() });
    r.text()
}

// ---- the checker -----------------------------------------------------------

/// `f` is first read for what it returns, by `g` above it, before it has
/// been checked itself: the `state` of the component in it is seen in that
/// quiet reading alone, and the mistake is said once, when `f` is checked.
#[test]
fn state_assigned_in_a_component_first_read_quietly() {
    let src = "fn g() = f()\nfn f() {\n    component C() {\n        state x = 1\n        x = 2\n        <b>{x}</b>\n    }\n    1\n}\n";
    assert_eq!(diags(src), ["3 `server fn` and `component` must be declared at the top level"]);
}

/// A method called on a value is a call of the function by that name, for
/// what `with` allows: `m.fetch()` may be the program's `fetch`.
#[test]
fn a_method_called_in_a_helper_counts_for_the_effects_of_the_function_by_its_name() {
    let src = "fn grab() = http.get(\"http://example.com/\")\nfn helper(m) = m.grab()\nfn safe(m) with db {\n    helper(m)\n}\n";
    let said = diags(src);
    assert!(said.iter().any(|d| d.contains("`helper` may use the http effect, but `safe` is declared `with db`")), "{:?}", said);
}

/// An `if` with no `else` among a list's classes, and the reading goes on
/// to the next. (What is in its block is not read: tests/check_edges.rs has
/// that as a finding.)
#[test]
fn the_classes_after_an_if_with_no_else_are_read() {
    let d = Folder::new("class-if", &[("main.qwe", "css(\".ok { a: b }\")\nfn a(on) = <p class={[if on { \"ok\" }, \"okk\"]}>1</p>\n")]);
    let opts = Options { classes: true, ..Default::default() };
    let said: Vec<String> = check_file_opts(&d.join("main.qwe"), Default::default(), opts).diagnostics.iter().map(shown).collect();
    assert_eq!(said, ["2 warning: no style sheet has a rule for `.okk` (did you mean `ok`?)"]);
}

/// A call of a service is a call over HTTP: a function that makes one and
/// is called where `http` is not allowed is told so, as it is for `http.get`.
#[test]
fn a_service_called_in_a_helper_counts_as_the_http_effect() {
    let d = Folder::new("service-effect", &[("main.qwe", "service billing = \"billing.qwe\"\nfn charge() = billing.charge(1)\nfn safe() with db {\n    charge()\n}\n"), ("billing.qwe", "server fn charge(n) = n\n")]);
    let said: Vec<String> = check_file(&d.join("main.qwe")).diagnostics.iter().map(shown).collect();
    assert!(said.iter().any(|m| m.contains("may use the http effect")), "{:?}", said);
}

// ---- the browser compiler --------------------------------------------------

/// A component that returns early is made again whole, and reads its
/// props anew each time.
#[test]
fn a_component_that_returns_early_copies_its_props() {
    let js = bundle("component C(a) {\n    if a {\n        return <b>\"yes\"</b>\n    }\n    <i>\"no\"</i>\n}\nroute GET / = <C a={true}/>\n");
    assert!(js.contains("= $p.a;"), "{}", js);
}

/// A function is a global the browser has, but not one it can change.
#[test]
fn a_handler_cannot_assign_a_function_of_the_server() {
    let e = refused("fn f() = 1\ncomponent C() {\n    <button @click={() => {\n        f = 2\n    }}>\"x\"</button>\n}\nroute GET / = <C/>\n");
    assert!(e.contains("cannot assign to `f` in the browser: it is a server variable"), "{}", e);
}

#[test]
fn state_stays_when_a_component_returns_early() {
    let js = bundle("component C(a) {\n    state x = 1\n    if a {\n        return <b>{x}</b>\n    }\n    <i>{x}</i>\n}\nroute GET / = <C a={true}/>\n");
    assert!(js.contains("= $p.a;"), "{}", js);
}

// ---- the interpreter -------------------------------------------------------

/// A component called as a function makes its HTML with no page around it:
/// its `state` is a variable.
#[test]
fn a_component_called_as_a_function_has_its_state_as_a_variable() {
    assert_eq!(value("component C() {\n    state x = 1\n    <b>{x}</b>\n}\n`${C()}`\n"), "\"<b>1</b>\"");
}

/// A cached view reads `.len` of a global that is no map: a method, not a
/// field it could follow by itself.
#[test]
fn a_cached_view_reads_a_method_of_a_global_that_is_no_map() {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    it.load_source("let greeting = \"hello\"\nview Hi() = <p>{greeting.len}</p>\nroute GET /a = <Hi/>\n", "test.qwe").unwrap();
    assert_eq!(get(&mut it, "/a"), "<p>5</p>");
    assert_eq!(it.view_cache_stats().0, 0);
    assert_eq!(get(&mut it, "/a"), "<p>5</p>");
    assert_eq!(it.view_cache_stats().0, 1);
}

/// Python's `math.fsum` gives the same: the two largest partials here add
/// up exactly, and the sum goes on past them.
#[test]
fn fsum_goes_on_while_its_partials_add_up_exactly() {
    let small = 2f64.powi(-60);
    let big = -1.5 + 2f64.powi(-50);
    let src = format!("math.fsum([1.5, {:e}, {:e}])", small, big);
    let want = 2f64.powi(-50) + small;
    assert_eq!(value(&src).parse::<f64>().unwrap(), want, "{}", src);
}

/// A program in the root folder, and a path that is that folder: nothing
/// above it to make, and no file to write there.
#[test]
fn an_upload_cannot_be_saved_as_the_root_folder() {
    let mut it = Interp::new("/");
    it.load_source("route POST /up {\n    let r = save_upload(request.files.a, \".\")\n    `${r}`\n}\n", "app.qwe").unwrap();
    let body = b"--XyZ\r\nContent-Disposition: form-data; name=\"a\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n--XyZ--\r\n".to_vec();
    let headers = vec![("Host".to_string(), "localhost".to_string()), ("Content-Type".to_string(), "multipart/form-data; boundary=XyZ".to_string())];
    let r = it.handle(&HttpRequest { method: "POST".into(), url: "/up".into(), headers, body, remote: "127.0.0.1".into() });
    assert!(r.text().contains("save_upload: "), "{}", r.text());
}

// ---- style sheets ----------------------------------------------------------

#[test]
fn a_program_with_no_style_has_no_styles() {
    let mut it = loaded("view V() = <p>\"x\"</p>\nroute GET / = <V/>\n");
    assert!(quillo::styles::Styles::collect(&mut it).unwrap().is_empty());
    let mut it = loaded("view V() {\n    style css`p { color: red }`\n    <p>\"x\"</p>\n}\nroute GET / = <V/>\n");
    assert!(!quillo::styles::Styles::collect(&mut it).unwrap().is_empty());
}

// ---- the main file an editor looks for -------------------------------------

/// A Rust file is passed over on the way to the file that names this one.
#[test]
fn the_main_file_is_found_past_a_rust_file_it_uses() {
    let d = Folder::new("entry-rust", &[("main.qwe", "use kv.qwer\nuse lib/a.qwe\n"), ("kv.qwer", "pub fn get() -> i64 { 1 }\n"), ("lib/a.qwe", "fn a() = 1\n")]);
    assert_eq!(quillo::lsp::program_entry(&d.join("lib/a.qwe")), Some(d.join("main.qwe")));
}

/// `use a as from lib/a.qwe`: `from` went for the name, and the rest is no
/// list of names: not a `use` of that file.
#[test]
fn a_use_line_whose_names_run_out_names_no_file() {
    let d = Folder::new("entry-bad", &[("main.qwe", "use a as from lib/a.qwe\n"), ("lib/a.qwe", "fn a() = 1\n")]);
    assert_eq!(quillo::lsp::program_entry(&d.join("lib/a.qwe")), None);
}

// ---- Rust files ------------------------------------------------------------

/// The `;` of an array's type ended the signature: the function is left
/// out, and those after it are read.
#[test]
fn a_rust_signature_with_an_array_is_left_out() {
    let parsed = quillo::native::parse("pub fn first(a: [u8; 3]) -> i64 { a[0] as i64 }\npub fn two() -> i64 { 2 }\n");
    let names: Vec<&str> = parsed.exports.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["two"]);
    assert_eq!(parsed.problems.len(), 1, "{:?}", parsed.problems);
    assert!(parsed.problems[0].1.contains("`first` is not exported"), "{:?}", parsed.problems);
}

#[test]
fn a_rust_signature_with_an_array_says_it_is_not_exported() {
    let parsed = quillo::native::parse("pub fn first(a: [u8; 3]) -> i64 { a[0] as i64 }\n");
    assert_eq!(parsed.problems.len(), 1, "{:?}", parsed.exports.iter().map(|e| &e.name).collect::<Vec<_>>());
    assert!(parsed.problems[0].1.contains("first"), "{:?}", parsed.problems);
}

/// What cargo says beside the errors (`For more information...`, `aborting
/// due to...`) is not shown in the editor.
#[test]
fn only_the_compilers_errors_and_warnings_reach_the_editor() {
    const CHILD: &str = "QUILLO_TEST_FAKE_CARGO";
    if let Some(file) = std::env::var_os(CHILD) {
        let notes = quillo::native::check(Path::new(&file)).unwrap();
        assert_eq!(notes.len(), 1, "{:?}", notes);
        assert!(notes[0].error);
        assert_eq!((notes[0].line, notes[0].col), (1, 3));
        assert_eq!(notes[0].message, "mismatched types");
        return;
    }
    let d = Folder::new("fake-cargo", &[("kv.qwer", "pub fn get() -> i64 { 1 }\n")]);
    let file = d.join("kv.qwer");
    let msg = |level: &str, text: &str, spans: String| format!("{{\"reason\":\"compiler-message\",\"message\":{{\"level\":\"{}\",\"message\":\"{}\",\"children\":[],\"spans\":[{}]}}}}", level, text, spans);
    let span = format!("{{\"is_primary\":true,\"file_name\":\"{}\",\"line_start\":1,\"column_start\":4,\"line_end\":1,\"column_end\":7}}", file.display());
    let lines = [msg("error", "mismatched types", span), msg("error", "aborting due to 1 previous error", String::new()), msg("failure-note", "For more information about this error, try `rustc --explain E0308`.", String::new())];
    std::fs::write(d.join("said.json"), lines.join("\n")).unwrap();
    let fake = d.join("cargo");
    std::fs::write(&fake, format!("#!/bin/sh\ncat '{}'\n", d.join("said.json").display())).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "only_the_compilers_errors_and_warnings_reach_the_editor", "--nocapture"])
        .env(CHILD, &file)
        .env("CARGO", &fake)
        .env("QUILLO_NATIVE_CACHE", d.join("cache"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"), "{}", String::from_utf8_lossy(&out.stdout));
}
