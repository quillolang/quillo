//! `quillo lsp`, past the plain path: the use lines, classes, Rust files and
//! completions an editor sends, and what it answers when there is nothing.

mod support;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Client {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Client {
    fn start() -> Client {
        Client::start_with(&[])
    }

    fn start_with(env: &[(&str, &str)]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("lsp")
            .env("QUILLO_NATIVE_CACHE", std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"))
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Client { child, out, next: 1 }
    }

    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut len = 0;
        loop {
            let mut line = String::new();
            self.out.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0; len];
        self.out.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Sends a request and returns its result, collecting notifications.
    fn request(&mut self, method: &str, params: Value, notes: &mut Vec<Value>) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let m = self.recv();
            if m.get("id") == Some(&json!(id)) {
                return m["result"].clone();
            }
            notes.push(m);
        }
    }
}


impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A folder of files for one test.
fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("lsp-{}", name)).canonicalize().unwrap();
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn uri_of(p: &Path) -> String {
    format!("file://{}", p.display())
}

/// An editor's session: requests with their notifications kept apart.
struct Session {
    c: Client,
    notes: Vec<Value>,
}

impl Session {
    fn start() -> Session {
        Session::with(Client::start())
    }

    fn with(mut c: Client) -> Session {
        let mut notes = Vec::new();
        c.request("initialize", json!({ "capabilities": {} }), &mut notes);
        c.notify("initialized", json!({}));
        Session { c, notes }
    }

    fn open(&mut self, uri: &str, text: &str) {
        let lang = if uri.ends_with(".css") { "css" } else { "quillo" };
        self.c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": lang, "version": 1, "text": text } }));
    }

    fn at(&mut self, method: &str, uri: &str, line: usize, character: usize, more: Value) -> Value {
        let mut p = json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });
        for (k, v) in more.as_object().unwrap() {
            p[k] = v.clone();
        }
        self.c.request(method, p, &mut self.notes)
    }

    fn hover(&mut self, uri: &str, line: usize, character: usize) -> String {
        let h = self.at("textDocument/hover", uri, line, character, json!({}));
        h["contents"]["value"].as_str().unwrap_or("").to_string()
    }

    fn labels(&mut self, uri: &str, line: usize, character: usize, more: Value) -> Vec<String> {
        let r = self.at("textDocument/completion", uri, line, character, more);
        r.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect()
    }

    /// The next diagnostics for `uri`.
    fn diags(&mut self, uri: &str) -> Value {
        if let Some(i) = self.notes.iter().position(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri)) {
            return self.notes.remove(i)["params"]["diagnostics"].clone();
        }
        loop {
            let m = self.c.recv();
            if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
                return m["params"]["diagnostics"].clone();
            }
        }
    }
}

#[test]
fn the_program_a_file_belongs_to_is_looked_for_upward() {
    let d = folder("entry", &[
        ("app/main.qwe", "use \"lib/one.qwe\"\n"),
        ("app/lib/one.qwe", "use \"lib/two.qwe\"\n"),
        ("app/lib/two.qwe", "use \"lib/one.qwe\"\nfn f() = 1\n"),
        ("app/lib/alone.qwe", "fn g() = 2\n"),
        ("app/other.qwe", "use \"nothere.qwe\"\n"),
    ]);
    // Through a file that uses it, past a cycle, from a folder above.
    assert_eq!(quillo::lsp::program_entry(&d.join("app/lib/two.qwe")).unwrap().file_name().unwrap(), "main.qwe");
    assert_eq!(quillo::lsp::program_entry(&d.join("app/lib/alone.qwe")), None);
    assert_eq!(quillo::lsp::program_entry(&d.join("app/nothere.qwe")), None);
    std::fs::write(d.join("app/.git"), "").unwrap();
    // Not above the project.
    let inner = d.join("top.qwe");
    std::fs::write(&inner, "use \"app/lib/alone.qwe\"\n").unwrap();
    assert_eq!(quillo::lsp::program_entry(&d.join("app/lib/alone.qwe")), None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_closed_file_has_no_diagnostics_left() {
    let d = folder("close", &[("main.qwe", "print(nope)\n")]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, "print(nope)\n");
    assert_eq!(s.diags(&uri).as_array().unwrap().len(), 1);
    s.c.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri } }));
    assert_eq!(s.diags(&uri), json!([]));
    // Closed, it is known no more.
    assert_eq!(s.hover(&uri, 0, 6), "");
}

#[test]
fn what_it_does_not_know_is_an_error_for_a_request_and_nothing_for_a_notification() {
    let mut s = Session::start();
    s.c.notify("workspace/didChangeConfiguration", json!({}));
    let id = 99;
    s.c.send(json!({ "jsonrpc": "2.0", "id": id, "method": "textDocument/formatting", "params": {} }));
    let m = loop {
        let m = s.c.recv();
        if m["id"] == json!(id) {
            break m;
        }
    };
    assert_eq!(m["error"]["code"], -32601);
    assert_eq!(m["error"]["message"], "unsupported: textDocument/formatting");
}

#[test]
fn exit_without_shutdown_is_a_failure() {
    let mut c = Client::start();
    c.notify("exit", Value::Null);
    assert_eq!(c.child.wait().unwrap().code(), Some(1));
}

#[test]
fn keywords_are_explained_on_hover() {
    let text = "route GET / = 1\nfn f() with db = 1\nlet plain = 1\nprivate fn g() = 1\npublic fn h() = 1\n";
    let d = folder("keywords", &[("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    assert!(s.hover(&uri, 0, 2).starts_with("`route METHOD /path/:param"));
    assert!(s.hover(&uri, 1, 8).contains("Declares the effects"));
    assert!(s.hover(&uri, 3, 2).contains("A name for this file only"));
    assert!(s.hover(&uri, 4, 2).contains("The default, said out loud"));
    // A word that is neither a keyword nor a name has nothing to say.
    assert_eq!(s.at("textDocument/hover", &uri, 0, 7, json!({})), Value::Null);
    assert_eq!(s.at("textDocument/hover", &uri, 40, 0, json!({})), Value::Null);
}

#[test]
fn every_use_line_is_read() {
    let text = "use * from \"kv.qwer\"\nuse * as THREE with module, preload from \"https://cdn.test/three.js\"\nuse Chart with preload from \"https://cdn.test/chart.js\"\nuse get as read from kv\nuse \"lib.qwe\"\nuse \"gone.qwe\"\nuse lib\nuse get, put from \"kv.qwer\"\nuse * nonsense\nuse @\nuse\nuse a-b from \"kv.qwer\"\n";
    let d = folder("uses", &[("kv.qwer", "/// Reads a key.\npub fn get(key: &str) -> Option<String> { let _ = key; None }\npub fn put(key: &str, value: &str) -> bool { let _ = (key, value); true }\n"), ("lib.qwe", "fn shout() = 1\n"), ("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    let all = s.hover(&uri, 0, 13);
    assert!(all.starts_with("`kv.qwer`: Rust, every function it exports"), "{}", all);
    assert!(all.contains("get(str) -> str | null") && all.contains("put(str, str) -> bool"), "{}", all);
    // Under another name, a function goes by that name.
    let one = s.hover(&uri, 3, 12);
    assert!(one.starts_with("```quillo\nread(str) -> str | null\n```\nReads a key.\n"), "{}", one);
    assert!(one.ends_with("*From kv.qwer, called `read` here.*"), "{}", one);
    let taken = s.hover(&uri, 3, 22);
    assert!(taken.contains("`kv.qwer`: Rust, the functions taken") && taken.contains("read(str) -> str | null") && !taken.contains("put("), "{}", taken);
    let named = s.hover(&uri, 7, 21);
    assert!(named.contains("get(str)") && named.contains("put("), "{}", named);
    let plain = s.hover(&uri, 4, 7);
    assert_eq!(plain, "`lib.qwe`: run once, its public names this file's to use");
    assert_eq!(s.hover(&uri, 6, 5), "`lib.qwe`: run once, its public names this file's to use");
    assert_eq!(s.hover(&uri, 5, 7), "`gone.qwe`: no such file");
    assert_eq!(s.at("textDocument/definition", &uri, 5, 7, json!({})), Value::Null);
    // A library's address, and lines that are no `use` of anything.
    assert_eq!(s.at("textDocument/definition", &uri, 1, 50, json!({})), Value::Null);
    assert_eq!(s.at("textDocument/definition", &uri, 2, 40, json!({})), Value::Null);
    for line in [8, 9, 10, 11] {
        assert_eq!(s.at("textDocument/definition", &uri, line, 5, json!({})), Value::Null, "line {}", line);
    }
    // After `as`, a name of yours; after `*`, what may come.
    let line = "use * as ";
    let text2 = format!("{}\n", line);
    s.c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": text2 }] }));
    assert_eq!(s.labels(&uri, 0, line.len(), json!({})), Vec::<String>::new());
    let text3 = "use * \n";
    s.c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 3 }, "contentChanges": [{ "text": text3 }] }));
    assert_eq!(s.labels(&uri, 0, 6, json!({})), ["as", "from"]);
    let text4 = "use Chart with module, \n";
    s.c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 4 }, "contentChanges": [{ "text": text4 }] }));
    assert_eq!(s.labels(&uri, 0, 23, json!({})), ["module", "preload", "preconnect"]);
    let text5 = "use Chart with preload \n";
    s.c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 5 }, "contentChanges": [{ "text": text5 }] }));
    assert_eq!(s.labels(&uri, 0, 23, json!({})), ["from"]);
    let text6 = "use Chart with other \n";
    s.c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 6 }, "contentChanges": [{ "text": text6 }] }));
    assert!(!s.labels(&uri, 0, 21, json!({})).iter().any(|l| l == "preload"));
}

#[test]
fn a_path_with_spaces_goes_as_a_uri() {
    let text = "use \"my lib.qwe\"\nprint(shout())\n";
    let d = folder("spaces", &[("my lib.qwe", "fn shout() = 1\n"), ("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    let def = s.at("textDocument/definition", &uri, 1, 7, json!({}));
    assert!(def["uri"].as_str().unwrap().ends_with("/my%20lib.qwe"), "{}", def);
}

#[test]
fn only_quillo_files_get_quillo_answers() {
    let text = "pub fn get(key: &str) -> String { key.to_string() }\n";
    let d = folder("rust-quiet", &[("kv.qwer", text), ("a.css", ".x { }\n")]);
    let uri = uri_of(&d.join("kv.qwer"));
    let css = uri_of(&d.join("a.css"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.open(&css, ".x { }\n");
    assert_eq!(s.at("textDocument/completion", &uri, 0, 3, json!({})), json!([]));
    assert_eq!(s.at("textDocument/completion", &css, 0, 1, json!({})), json!([]));
    assert_eq!(s.at("quillo/closeTag", &uri, 0, 3, json!({})), Value::Null);
    assert_eq!(s.c.request("textDocument/codeAction", json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } } }), &mut s.notes), json!([]));
    // In a Rust file the word under the cursor is not a class.
    assert_eq!(s.at("textDocument/prepareRename", &uri, 0, 8, json!({})), Value::Null);
    assert_eq!(s.at("textDocument/references", &uri, 0, 8, json!({ "context": { "includeDeclaration": true } })), json!([]));
    // Nor is a class of a file that is not open anything at all.
    let gone = uri_of(&d.join("gone.qwe"));
    assert_eq!(s.at("textDocument/completion", &gone, 0, 0, json!({})), json!([]));
    assert_eq!(s.at("textDocument/prepareRename", &gone, 0, 0, json!({})), Value::Null);
}

#[test]
fn a_class_is_renamed_only_to_a_class_name() {
    let text = "route GET / = <span.chip>a</span>\ncss(\".chip { a: b }\")\n";
    let d = folder("rename", &[("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    let p = s.at("textDocument/prepareRename", &uri, 0, 22, json!({}));
    assert_eq!(p, json!({ "range": { "start": { "line": 0, "character": 20 }, "end": { "line": 0, "character": 24 } }, "placeholder": "chip" }));
    for bad in ["1a", "a b", "--"] {
        s.c.send(json!({ "jsonrpc": "2.0", "id": 500, "method": "textDocument/rename", "params": { "textDocument": { "uri": uri }, "position": { "line": 0, "character": 22 }, "newName": bad } }));
        let m = loop {
            let m = s.c.recv();
            if m["id"] == json!(500) {
                break m;
            }
        };
        assert_eq!(m["error"]["code"], -32602, "{}", m);
        assert_eq!(m["error"]["message"], format!("`{}` is not a class name", bad));
    }
    // A leading dot is forgiven.
    let e = s.at("textDocument/rename", &uri, 0, 22, json!({ "newName": ".tag" }));
    assert_eq!(e["changes"][&uri].as_array().unwrap().len(), 2, "{}", e);
    // Away from any word, and on a word that is no class.
    assert_eq!(s.at("textDocument/prepareRename", &uri, 0, 5, json!({})), Value::Null);
    assert_eq!(s.at("textDocument/prepareRename", &uri, 0, 13, json!({})), Value::Null);
}

#[test]
fn a_style_sheet_not_open_with_its_program_finds_the_program() {
    let css = ".chip { color: red; }\n";
    let text = "route GET / = <span.chip>a</span>\n";
    let d = folder("sheet", &[("public/site.css", css), ("main.qwe", text), ("loose/public/x.css", css)]);
    let uri = uri_of(&d.join("public/site.css"));
    let mut s = Session::start();
    s.open(&uri, css);
    let refs = s.at("textDocument/references", &uri, 0, 3, json!({ "context": { "includeDeclaration": true } }));
    assert_eq!(refs.as_array().unwrap().len(), 2, "{}", refs);
    assert!(refs[1]["uri"].as_str().unwrap().ends_with("/main.qwe"), "{}", refs);
    // A sheet with no program above it.
    let loose = uri_of(&d.join("loose/public/x.css"));
    s.open(&loose, css);
    assert_eq!(s.at("textDocument/references", &loose, 0, 3, json!({ "context": { "includeDeclaration": true } })), json!([]));
}

#[test]
fn hovering_a_class_shows_its_rules_or_that_it_has_none() {
    let rules: String = (0..7).map(|i| format!(".chip.v{} {{ color: red; }}\n", i)).collect();
    let sheet = format!("{}.long {{\n  color: red;\n  margin: 0;\n}}\n", rules);
    let text = "route GET / = <span.chip class=\"long plain\">a</span>\n";
    let d = folder("hover-class", &[("public/site.css", &sheet), ("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    let many = s.hover(&uri, 0, 22);
    assert!(many.starts_with("`.chip`: 7 rules") && many.ends_with("… and 2 more"), "{}", many);
    let long = s.hover(&uri, 0, 33);
    assert!(long.contains("`.long`: 1 rule") && long.contains("margin: 0;"), "{}", long);
    assert_eq!(s.hover(&uri, 0, 39), "`.plain`: no style sheet of the program has a rule for it");
}

#[test]
fn completions_where_they_mean_something() {
    let text = "private fn own() = 1\nview Card() = <p></p>\ncomponent Box() = <p></p>\nlet total = 1\nroute GET /a = 1\n";
    let lib = "private fn hidden() = 1\nfn shown() = 1\n";
    let d = folder("complete", &[("lib.qwe", lib), ("main.qwe", "use \"lib.qwe\"\n")]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    let body = format!("use \"lib.qwe\"\n{}", text);
    s.open(&uri, &body);
    s.diags(&uri);
    let names = s.labels(&uri, 5, 0, json!({}));
    for want in ["own", "shown", "Card", "Box", "total"] {
        assert!(names.iter().any(|n| n == want), "{}: {:?}", want, names);
    }
    assert!(!names.iter().any(|n| n == "hidden"), "{:?}", names);
    let r = s.at("textDocument/completion", &uri, 5, 0, json!({}));
    let kind = |n: &str| r.as_array().unwrap().iter().find(|i| i["label"] == n).unwrap()["kind"].clone();
    assert_eq!(kind("Card"), 7);
    assert_eq!(kind("total"), 6);
    assert_eq!(kind("own"), 3);

    let change = |s: &mut Session, text: &str, version: u32| {
        s.c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }));
    };
    // After `route`, its methods; after `with`, the effects.
    let t = format!("{}route ", body);
    change(&mut s, &t, 2);
    let got = s.labels(&uri, 6, 6, json!({}));
    assert!(["GET", "POST", "DELETE"].iter().all(|m| got.iter().any(|g| g == m)), "{:?}", got);
    let t = format!("{}fn z() with ", body);
    change(&mut s, &t, 3);
    let got = s.labels(&uri, 6, 12, json!({}));
    assert!(["db", "http", "time"].iter().all(|m| got.iter().any(|g| g == m)), "{:?}", got);
    // A dot after what is no module is a word like any other.
    let t = format!("{}total.", body);
    change(&mut s, &t, 4);
    assert!(s.labels(&uri, 6, 6, json!({})).iter().any(|g| g == "total"));
    // After `<` markup starts: the program's views, and elements.
    let t = format!("{}let x = <", body);
    change(&mut s, &t, 5);
    let got = s.labels(&uri, 6, 9, json!({}));
    assert!(["Card", "Box", "div"].iter().all(|m| got.iter().any(|g| g == m)), "{:?}", got);
    // After a word, `<` compares.
    let t = format!("{}let y = total <", body);
    change(&mut s, &t, 6);
    assert!(s.labels(&uri, 6, 15, json!({})).iter().all(|g| g != "div"));
    // `/` is division.
    let t = format!("{}let y = total /", body);
    change(&mut s, &t, 7);
    assert_eq!(s.labels(&uri, 6, 15, json!({ "context": { "triggerCharacter": "/" } })), Vec::<String>::new());
}

#[test]
fn classes_complete_from_the_programs_rules() {
    let text = "css(\".chip { a: b } .card { a: b }\")\nlet a = <span.ch>x</span>\nlet b = <span class=\"ch\">x</span>\nlet c = <span class={on ? \"ch\" : \"\"}>x</span>\nlet d = <span class={on ? `c`: \"\"}>x</span>\n";
    let d = folder("class-complete", &[("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    let want = ["card", "chip"];
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    assert_eq!(sorted(s.labels(&uri, 1, 16, json!({}))), want);
    assert_eq!(sorted(s.labels(&uri, 2, 23, json!({}))), want);
    assert_eq!(sorted(s.labels(&uri, 3, 29, json!({}))), want);
    assert_eq!(sorted(s.labels(&uri, 4, 28, json!({}))), want);
}

#[test]
fn a_close_tag_is_offered_only_after_an_open_tag() {
    let text = "let a = <div>\nlet b = 1\n";
    let d = folder("close-tag", &[("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    s.diags(&uri);
    assert_eq!(s.at("quillo/closeTag", &uri, 0, 13, json!({})), json!("$0</div>"));
    assert_eq!(s.at("quillo/closeTag", &uri, 1, 3, json!({})), Value::Null);
    // Past the end of the text, the end of the text.
    assert_eq!(s.at("quillo/closeTag", &uri, 50, 0, json!({})), Value::Null);
}

#[test]
fn a_fix_is_offered_where_the_error_is() {
    let text = "let x = 1\nlet y = 2\nview V() = <div>\n  <p>a</p>\n";
    let d = folder("fixes", &[("main.qwe", text)]);
    let uri = uri_of(&d.join("main.qwe"));
    let mut s = Session::start();
    s.open(&uri, text);
    let ds = s.diags(&uri);
    let action = |s: &mut Session, first: u32, last: u32, diags: Value| {
        let p = json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": first, "character": 0 }, "end": { "line": last, "character": 0 } }, "context": { "diagnostics": diags } });
        s.c.request("textDocument/codeAction", p, &mut s.notes)
    };
    assert_eq!(action(&mut s, 0, 0, json!([])), json!([]));
    let line = ds[0]["range"]["start"]["line"].as_u64().unwrap() as u32;
    let got = action(&mut s, line, line, ds.clone());
    assert_eq!(got.as_array().unwrap().len(), 1, "{}", got);
    assert!(got[0]["title"].as_str().unwrap().contains("</div>"), "{}", got);
}

#[test]
fn rust_files_are_checked_one_at_a_time() {
    let text = "pub fn get(key: &str) -> String { key.to_string() }\n";
    let d = folder("rust-again", &[("kv.qwer", text)]);
    let uri = uri_of(&d.join("kv.qwer"));
    let mut s = Session::start();
    s.open(&uri, text);
    // Saved while the first check runs: it goes round again.
    s.c.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri } }));
    s.c.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri } }));
    // One as it opened, one as each was saved, and each of the two checks'.
    for _ in 0..5 {
        assert_eq!(s.diags(&uri), json!([]));
    }
}

#[test]
fn a_rust_error_with_no_width_is_shown_on_its_line() {
    let text = "pub fn get(key: &str) -> String {\n    key.to_string()\n";
    let d = folder("rust-eof", &[("kv.qwer", text)]);
    let uri = uri_of(&d.join("kv.qwer"));
    let mut s = Session::start();
    s.open(&uri, text);
    let said = loop {
        let got = s.diags(&uri);
        if let Some(m) = got.as_array().unwrap().iter().find(|d| d["message"].as_str().unwrap().contains("unclosed delimiter")) {
            break m.clone();
        }
    };
    assert_eq!(said["source"], "rustc", "{}", said);
    assert_eq!(said["severity"], 1);
    // The line's text, not a point at its end.
    assert_eq!(said["range"], json!({ "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 19 } }));
}

#[test]
fn with_no_cargo_a_rust_file_says_so() {
    let text = "pub fn get(key: &str) -> String { key.to_string() }\n";
    let d = folder("rust-nocargo", &[("kv.qwer", text)]);
    let uri = uri_of(&d.join("kv.qwer"));
    let mut s = Session::with(Client::start_with(&[("CARGO", "/nonexistent/cargo")]));
    s.open(&uri, text);
    let found = loop {
        let got = s.diags(&uri);
        if let Some(first) = got.as_array().and_then(|a| a.first()) {
            break first.clone();
        }
    };
    assert_eq!(found["severity"], 3);
    assert!(found["message"].as_str().unwrap().starts_with("cargo did not run ("), "{}", found);
    assert_eq!(found["range"]["start"]["line"], 0);
}
