//! Builtins' rarer arguments, each with the value or the exact words it gives: the browser's
//! own names on the server, a cursor counted in UTF-16, what the parser writes for callers and
//! builds, `markdown_page`'s options, `files` without a request, and stdin that cannot be read.

mod support;

use quillo::Interp;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

fn value(src: &str) -> String {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn error(src: &str) -> String {
    match Interp::new(".").load_source(src, "case.qwe") {
        Ok(v) => panic!("it gave {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => format!("{} {}", e.line, e.msg),
    }
}

/// A folder of files for one test, gone when it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str, files: &[(&str, &str)]) -> Folder {
        let d = temp_dir(&format!("cover-builtins-{}", name));
        for (f, text) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Folder(d.canonicalize().unwrap())
    }

    fn value(&self, src: &str) -> String {
        let mut it = Interp::new(self.0.to_str().unwrap());
        match it.load_source(src, "case.qwe") {
            Ok(v) => v.repr(),
            Err(e) => panic!("{}\n--- source ---\n{}", e, src),
        }
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_browser_s_own_functions_do_nothing_on_the_server() {
    assert_eq!(value("[run_after(1, () => 2), page_updated(), js_has(\"x\")]"), "[null, null, false]");
}

#[test]
fn cursor_index_counts_utf16_units_into_characters() {
    // `é` is one unit, the emoji two; past the end is the length; below zero is zero.
    assert_eq!(value("[cursor_index(\"h\u{e9}llo\", 2), cursor_index(\"a\u{1F600}b\", 3), cursor_index(\"ab\", -1), cursor_index(\"ab\", 10)]"), "[2, 2, 0, 2]");
    assert_eq!(error("cursor_index(5, 1)"), "1 `cursor_index` expects a str as argument 1");
}

#[test]
fn what_the_parser_writes_cannot_be_called_by_hand() {
    assert_eq!(error("__callers(1)"), "1 `__callers` is written by `server fn ... for [...]`");
    assert_eq!(error("__built(1)"), "1 `__built` is the route of a `build` declaration");
}

#[test]
fn markdown_page_s_options_are_a_map_with_a_title_and_a_language_tag() {
    assert_eq!(error("markdown_page(\"# Hi\", 5)"), "1 `markdown_page` expects options as a map, got int");
    assert_eq!(error("markdown_page(\"# Hi\", { title: 5 })"), "1 `markdown_page`'s title is a str, got 5");
    assert_eq!(error("markdown_page(\"# Hi\", { lang: \"e n\" })"), "1 `markdown_page`'s lang is a language tag such as \"en\", got \"e n\"");
    let page = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Hi</title></head><body><h1>Hi</h1>\n</body></html>";
    assert_eq!(value("str(markdown_page(\"# Hi\", null))"), format!("{:?}", page));
    assert_eq!(value("str(markdown_page(\"# Hi\", { title: null, lang: null }))"), format!("{:?}", page));
}

#[test]
fn files_outside_a_request_serves_what_it_finds_and_misses_the_rest() {
    let f = Folder::new("files", &[("site/docs/a.txt", "a"), ("site/page.html", "<p>page</p>"), ("site/missing/404.html", "gone"), ("plain/x.txt", "x")]);
    // A path of null is the folder; a folder with no index, outside a request, is not found.
    assert_eq!(f.value("let r = files(\"site\", null)\nr.status"), "404");
    assert_eq!(f.value("files(\"site\", \"docs\").status"), "404");
    // A file, and a name found with its extension.
    assert_eq!(f.value("let r = files(\"site\", \"docs/a.txt\")\n[r.status, str(r.body)]"), "[200, \"a\"]");
    assert_eq!(f.value("files(\"site\", \"page\").status"), "200");
    // A name with no file under any extension, and a folder whose 404 page is its own.
    assert_eq!(f.value("files(\"site\", \"nope\").status"), "404");
    assert_eq!(f.value("let r = files(\"site/missing\", \"x\")\n[r.status, str(r.body)]"), "[404, \"gone\"]");
    assert_eq!(f.value("files(\"plain\", \"y\", { not_found: null }).status"), "404");
}

#[test]
fn a_file_written_where_the_program_s_own_file_is_not_there_is_written() {
    let f = Folder::new("write", &[]);
    // `case.qwe` is not a file on disk: nothing of it is there to compare with.
    assert_eq!(f.value("write_file(\"w.txt\", \"x\")\nread_file(\"w.txt\")"), "\"x\"");
    assert_eq!(f.value("read_file(\"\")"), "err(\"read_file: path must stay inside the program's folder: \")");
}

fn quillo_with_stdin(dir: &std::path::Path, script: &str, stdin: Stdio) -> (String, String) {
    std::fs::write(dir.join("main.qwe"), script).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe"])
        .current_dir(dir)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("quillo did not finish in 30 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn stdin_that_cannot_be_read_is_an_error_value() {
    let f = Folder::new("stdin", &[]);
    // A folder as stdin opens, and reading it fails.
    let as_stdin = || Stdio::from(std::fs::File::open(&f.0).unwrap());
    let (out, err) = quillo_with_stdin(&f.0, "print(stdin())\n", as_stdin());
    assert_eq!(out.trim(), "err(\"stdin: Is a directory (os error 21)\")", "{}", err);
    let (out, err) = quillo_with_stdin(&f.0, "print(read_line())\n", as_stdin());
    assert_eq!(out.trim(), "err(\"read_line: Is a directory (os error 21)\")", "{}", err);
}

#[test]
fn files_at_a_folder_asked_for_with_its_slash_serves_its_index() {
    use quillo::server::HttpRequest;
    let f = Folder::new("files-index", &[("site/index.html", "<p>home</p>")]);
    let mut it = Interp::new(f.0.to_str().unwrap());
    it.load_source("route GET / = files(\"site\", \"\")\n", "case.qwe").unwrap();
    let headers = vec![("Host".to_string(), "localhost".to_string())];
    let r = it.handle(&HttpRequest { method: "GET".into(), url: "/".into(), headers, body: Vec::new(), remote: "127.0.0.1".into() });
    assert_eq!(r.status, 200);
    assert_eq!(r.text(), "<p>home</p>");
}
