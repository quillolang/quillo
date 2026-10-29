//! `quillo lsp` over stdio, as an editor would use it.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

struct Client {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("lsp")
            .env("QUILLO_NATIVE_CACHE", std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"))
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

#[test]
fn language_server_round_trip() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.qwe"), "fn shout(s: str) -> str = upper(s) + \"!\"\n").unwrap();
    let main = dir.join("app.qwe");
    let uri = format!("file://{}", main.canonicalize().unwrap_or(main.clone()).display()).replace("file:///private/", "file:///private/");
    let text = "use \"lib.qwe\"\nfn add(a: int, b: int = 10) -> int = a + b\nlet total = add(1)\nroute GET / = shout(`hi ${totl}`)\n";
    std::fs::write(&main, text).unwrap();

    let mut c = Client::start();
    let mut notes = Vec::new();
    let init = c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    assert_eq!(init["capabilities"]["hoverProvider"], json!(true));
    c.notify("initialized", json!({}));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));

    // Diagnostics: the typo, underlined where it is.
    let d = loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    assert_eq!(d.as_array().unwrap().len(), 1, "{}", d);
    assert!(d[0]["message"].as_str().unwrap().contains("unknown name `totl` (did you mean `total`?)"));
    assert_eq!(d[0]["range"], json!({ "start": { "line": 3, "character": 26 }, "end": { "line": 3, "character": 30 } }));

    // Hover on `add` in `add(1)`.
    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 13 } }), &mut notes);
    assert!(h["contents"]["value"].as_str().unwrap().contains("fn add(a: int, b: int = …) -> int"), "{}", h);

    // Definition of `shout`, in the used file.
    let def = c.request("textDocument/definition", json!({ "textDocument": { "uri": uri }, "position": { "line": 3, "character": 15 } }), &mut notes);
    assert!(def["uri"].as_str().unwrap().ends_with("/lib.qwe"), "{}", def);
    assert_eq!(def["range"]["start"], json!({ "line": 0, "character": 3 }));

    // Completion after `db.` lists its functions; elsewhere, names and keywords.
    let mut changed = text.to_string();
    changed.push_str("let rows = db.");
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": changed }] }));
    let items = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": 4, "character": 14 } }), &mut notes);
    let labels: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"query") && labels.contains(&"transaction") && !labels.contains(&"len"), "{:?}", labels);
    let items = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 0 } }), &mut notes);
    let labels: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(["add", "total", "shout", "len", "route", "component", "str"].iter().all(|l| labels.contains(l)), "{:?}", labels);
    // Every module, and their functions after the dot.
    assert!(["udp", "channel", "media", "math", "vec", "money"].iter().all(|l| labels.contains(l)), "{:?}", labels);
    for (line, want) in [("let x = udp.", ["listen", "send", "reply"]), ("let x = channel.", ["open", "broadcast", "peers"]), ("let x = media.", ["rtp", "streams", "rtp"])] {
        let changed = format!("{}{}", text, line);
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 3 }, "contentChanges": [{ "text": changed }] }));
        let items = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": 4, "character": line.len() } }), &mut notes);
        let labels: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(want.iter().all(|l| labels.contains(l)), "{}: {:?}", line, labels);
    }

    // Outline.
    let syms = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }), &mut notes);
    let names: Vec<&str> = syms.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["add", "total", "GET /"]);

    let r = c.request("shutdown", Value::Null, &mut notes);
    assert_eq!(r, Value::Null);
    c.notify("exit", Value::Null);
    assert!(c.child.wait().unwrap().success());
}

/// A file another `use`s is part of that program: its names come from the
/// other files, so it is checked as the program, from its main file.
#[test]
fn a_used_file_is_checked_as_part_of_its_program() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-used-{}", std::process::id())).canonicalize().unwrap_or_else(|_| {
        let d = std::env::temp_dir().join(format!("quillo-lsp-used-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    });
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("main.qwe"), "use \"lib/util.qwe\"\nuse \"lib/pages.qwe\"\nroute GET / = home()\n").unwrap();
    std::fs::write(dir.join("lib/util.qwe"), "fn shout(s: str) -> str = upper(s) + \"!\"\n").unwrap();
    let pages = dir.join("lib/pages.qwe");
    std::fs::write(&pages, "fn home() = shout(\"hi\")\n").unwrap();
    let uri = format!("file://{}", pages.display());

    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("initialized", json!({}));
    let diags = |c: &mut Client| loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    // `shout` is from another file of the program: no error.
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": "fn home() = shout(\"hi\")\n" } }));
    assert_eq!(diags(&mut c), json!([]));
    // A real mistake still is one, from the text being edited.
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "fn home() = shuot(\"hi\")\n" }] }));
    let d = diags(&mut c);
    assert_eq!(d.as_array().unwrap().len(), 1, "{}", d);
    assert!(d[0]["message"].as_str().unwrap().contains("unknown name `shuot` (did you mean `shout`?)"), "{}", d);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `.qwer` file: what Quillo cannot export is said as it is typed, and
/// the Rust compiler's errors once it is opened or saved; hover shows how
/// Quillo sees an export.
#[test]
fn rust_files_get_quillo_and_compiler_diagnostics() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-qwer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let path = dir.join("lib.qwer");
    let text = "pub struct P;\n\n/// Adds two numbers.\npub fn add(a: i64, b: i64) -> i64 {\n    a + b + \"x\"\n}\n\npub fn point() -> P { P }\n";
    std::fs::write(&path, text).unwrap();
    let uri = format!("file://{}", path.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "rust", "version": 1, "text": text } }));
    // First what Quillo makes of it, then the compiler's errors with it.
    let mut quillo_seen = false;
    let diags = loop {
        let m = c.recv();
        if m["method"] != "textDocument/publishDiagnostics" || m["params"]["uri"] != json!(uri) {
            continue;
        }
        let d = m["params"]["diagnostics"].as_array().unwrap().clone();
        quillo_seen |= d.iter().any(|x| x["source"] == "quillo");
        if d.iter().any(|x| x["source"] == "rustc") {
            break d;
        }
    };
    assert!(quillo_seen);
    let quillo: Vec<&Value> = diags.iter().filter(|x| x["source"] == "quillo").collect();
    assert!(quillo.len() == 1 && quillo[0]["message"].as_str().unwrap().contains("`point` is not exported"), "{:?}", diags);
    let rustc: Vec<&Value> = diags.iter().filter(|x| x["source"] == "rustc" && x["severity"] == 1).collect();
    assert!(!rustc.is_empty(), "{:?}", diags);
    // On the line of the mistake, in the file itself.
    assert_eq!(rustc[0]["range"]["start"]["line"], 4, "{:?}", rustc);
    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": uri }, "position": { "line": 3, "character": 8 } }), &mut notes);
    let h = h["contents"]["value"].as_str().unwrap_or("").to_string();
    assert!(h.contains("add(int, int) -> int") && h.contains("Adds two numbers."), "{}", h);
    let syms = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }), &mut notes);
    let names: Vec<&str> = syms.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["add"]);
    // Fixed and saved: the compiler has nothing more to say.
    let fixed = text.replace(" + \"x\"", "");
    std::fs::write(&path, &fixed).unwrap();
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": fixed }] }));
    c.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri } }));
    loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            let d = m["params"]["diagnostics"].as_array().unwrap();
            if !d.iter().any(|x| x["source"] == "rustc" && x["severity"] == 1) && d.iter().all(|x| x["source"] != "quillo" || x["message"].as_str().unwrap().contains("point")) && !d.is_empty() {
                break;
            }
        }
    }
    c.request("shutdown", Value::Null, &mut notes);
    c.notify("exit", Value::Null);
    assert!(c.child.wait().unwrap().success());
}

/// A word in markup's text is text, even when a component has its name:
/// hover and go to definition say nothing of it, but do of the component.
#[test]
fn text_with_a_components_name_is_not_the_component() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.canonicalize().unwrap().join("main.qwe");
    let text = "component Notes() {\n    state items = []\n    <ul>{len(items)}</ul>\n}\n\nroute GET / =\n    <div>\n        <h1>Notes</h1>\n        <Notes/>\n    </div>\n\ncomponent Form() {\n    state draft = \"\"\n    state problem = null\n    state count = 0\n    <form @submit={() => { problem = \"empty\"; count += 1; draft = \"\" }}><input bind:value={draft}/>{problem}{count}</form>\n}\n";
    std::fs::write(&path, text).unwrap();
    let uri = format!("file://{}", path.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    let at = |line: u32, character: u32| json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });
    // `Notes` in `<h1>Notes</h1>`: text.
    assert_eq!(c.request("textDocument/hover", at(7, 13), &mut notes), Value::Null);
    assert_eq!(c.request("textDocument/definition", at(7, 13), &mut notes), Value::Null);
    // `<Notes/>`, and the definition itself: the component.
    let h = c.request("textDocument/hover", at(8, 10), &mut notes);
    assert!(h["contents"]["value"].as_str().unwrap_or("").contains("component Notes"), "{}", h);
    // State is shown as it is declared, with the types it takes, though
    // it is assigned (where checking takes it as anything).
    let shown = |c: &mut Client, notes: &mut Vec<Value>, line: u32, ch: u32| -> String {
        let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": ch } }), notes);
        h["contents"]["value"].as_str().unwrap_or("").to_string()
    };
    assert!(shown(&mut c, &mut notes, 2, 15).contains("state items: list"));
    assert!(shown(&mut c, &mut notes, 1, 12).contains("state items: list"), "on its declaration");
    assert!(shown(&mut c, &mut notes, 12, 12).contains("state draft: str"), "{}", shown(&mut c, &mut notes, 12, 12));
    assert!(shown(&mut c, &mut notes, 13, 12).contains("state problem: str | null") || shown(&mut c, &mut notes, 13, 12).contains("state problem: null | str"), "{}", shown(&mut c, &mut notes, 13, 12));
    let at_count = text.lines().nth(15).unwrap().find("{count}").unwrap() as u32 + 2;
    let used = shown(&mut c, &mut notes, 15, at_count);
    assert!(used.contains("state count: int"), "{}", used);
    let d = c.request("textDocument/definition", at(8, 10), &mut notes);
    assert_eq!(d["range"]["start"]["line"], 0, "{}", d);
    let h = c.request("textDocument/hover", at(0, 12), &mut notes);
    assert!(h["contents"]["value"].as_str().unwrap_or("").contains("component Notes"), "{}", h);
    c.request("shutdown", Value::Null, &mut notes);
    c.notify("exit", Value::Null);
    assert!(c.child.wait().unwrap().success());
}

/// Text that does not parse may be for a newer Quillo than the server
/// running: its error says which quillo that is.
#[test]
fn a_syntax_error_says_which_quillo_read_it() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-syntax-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.qwe");
    // A word this Quillo does not know: an unclosed `(` is only a mistake
    // (FEEDBACK.md, Q-008; branch feedback6-fixes).
    std::fs::write(&file, "let x = 1 as int\n").unwrap();
    let uri = format!("file://{}", file.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("initialized", json!({}));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": "let x = 1 as int\n" } }));
    let d = loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    let msg = d[0]["message"].as_str().unwrap();
    assert!(msg.contains("this is quillo ") && msg.contains("newer Quillo"), "{}", msg);
    // A mistake that parses says nothing of the sort.
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "print(nope)\n" }] }));
    let d = loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    assert!(!d[0]["message"].as_str().unwrap().contains("this is quillo"), "{}", d);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A missing end tag: told at the element, with a quick fix that adds it;
/// `</` is completed with the element open there, and `quillo/closeTag`
/// gives the end tag once a start tag's `>` is typed (branch
/// editor-end-tags).
#[test]
fn a_missing_end_tag_is_told_completed_and_fixed() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-tags-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let uri = format!("file://{}/app.qwe", dir.display());
    let text = "component List() {\n    <ul>\n        <li>one\n    </ul>\n}\n";
    let mut c = Client::start();
    let mut notes = Vec::new();
    let init = c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    assert!(init["capabilities"]["codeActionProvider"].is_object(), "{}", init);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    let d = loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    assert_eq!(d[0]["message"], json!("`<li>` is never closed: add `</li>` before `</ul>` on line 4"), "{}", d);
    assert_eq!(d[0]["range"], json!({ "start": { "line": 2, "character": 9 }, "end": { "line": 2, "character": 11 } }));

    let acts = c.request(
        "textDocument/codeAction",
        json!({ "textDocument": { "uri": uri }, "range": d[0]["range"], "context": { "diagnostics": d } }),
        &mut notes,
    );
    assert_eq!(acts[0]["title"], json!("Add </li>"), "{}", acts);
    assert_eq!(
        acts[0]["edit"]["changes"][&uri][0],
        json!({ "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 0 } }, "newText": "        </li>\n" })
    );

    let typing = "component List() {\n    <ul>\n        <li>one</\n}\n";
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": typing }] }));
    let items = c.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 17 }, "context": { "triggerKind": 2, "triggerCharacter": "/" } }),
        &mut notes,
    );
    assert_eq!(items.as_array().unwrap().len(), 1, "{}", items);
    assert_eq!(items[0]["textEdit"]["newText"], json!("li>"));
    let close = c.request("quillo/closeTag", json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 17 } }), &mut notes);
    assert_eq!(close, json!("li>"));

    let typing = "component List() {\n    <ul>\n        <li>\n    </ul>\n}\n";
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 3 }, "contentChanges": [{ "text": typing }] }));
    let close = c.request("quillo/closeTag", json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 12 } }), &mut notes);
    assert_eq!(close, json!("$0</li>"));
    // Not after `/>`, a void element, or a comparison.
    for (src, col) in [("fn f() = <p><br/>\n", 17), ("fn f() = <p><br>\n", 16), ("fn f(a, b) = a > b\n", 16)] {
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 4 }, "contentChanges": [{ "text": src }] }));
        let close = c.request("quillo/closeTag", json!({ "textDocument": { "uri": uri }, "position": { "line": 0, "character": col } }), &mut notes);
        assert_eq!(close, Value::Null, "{}", src);
    }
}

/// A class in markup leads to its rules: in the CSS files under public/, in
/// `css(...)`, and in a component's `style`; after `<span.` the classes are
/// offered (branch class-links).
#[test]
fn classes_in_markup_lead_to_their_rules() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-classes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("public/css")).unwrap();
    std::fs::write(dir.join("public/css/site.css"), "body { margin: 0; }\n.chip { color: red; }\n.chip-x { color: blue; }\n").unwrap();
    let uri = format!("file://{}/app.qwe", dir.display());
    let text = "let sheet = css(\"\n.card { padding: 4px; }\n\")\ncomponent Tag() {\n    style \".tag { color: green; }\"\n    <b.tag>x</b>\n}\nroute GET / = <div.card><span.chip class=\"card\">a</span></div>\n";
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    let def = |c: &mut Client, notes: &mut Vec<Value>, line: u32, character: u32| {
        c.request("textDocument/definition", json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }), notes)
    };
    // `.chip` in `<span.chip`: the rule in site.css, not `.chip-x`.
    let d = def(&mut c, &mut notes, 7, 33);
    assert_eq!(d.as_array().map(|a| a.len()), Some(1), "{}", d);
    assert!(d[0]["uri"].as_str().unwrap().ends_with("/public/css/site.css"), "{}", d);
    assert_eq!(d[0]["range"], json!({ "start": { "line": 1, "character": 1 }, "end": { "line": 1, "character": 5 } }));
    // `card` in `class="card"` and in `<div.card`: the rule in `css(...)`.
    for col in [44, 21] {
        let d = def(&mut c, &mut notes, 7, col);
        assert_eq!(d[0]["uri"], json!(uri), "{}", d);
        assert_eq!(d[0]["range"]["start"], json!({ "line": 1, "character": 1 }), "{}", d);
    }
    // `.tag`: the component's own `style`.
    let d = def(&mut c, &mut notes, 5, 8);
    assert_eq!(d[0]["range"]["start"], json!({ "line": 4, "character": 12 }), "{}", d);

    let typing = text.replace("<span.chip", "<span.");
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": typing }] }));
    let items = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": 7, "character": 30 } }), &mut notes);
    let labels: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(["card", "tag", "chip", "chip-x"].iter().all(|l| labels.contains(l)) && !labels.contains(&"len"), "{:?}", labels);
}

/// Private names: hover and the outline say `private`, ranges cover the
/// name as written, and another file's private view is not offered after
/// `<` (branch public-private-editor).
#[test]
fn private_names_are_shown_as_private_and_kept_to_their_file() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-private-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let lib = "private fn page_of(n: int) = n\nprivate view Post(p: map) = <b>{p.title}</b>\npublic fn first() = page_of(1)\n";
    std::fs::write(dir.join("lib/forum.qwe"), lib).unwrap();
    let main = "use \"lib/forum.qwe\"\nroute GET / = <main></main>\n";
    std::fs::write(dir.join("main.qwe"), main).unwrap();
    let lib_uri = format!("file://{}/lib/forum.qwe", dir.display());
    let main_uri = format!("file://{}/main.qwe", dir.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": lib_uri, "languageId": "quillo", "version": 1, "text": lib } }));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": main_uri, "languageId": "quillo", "version": 1, "text": main } }));

    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": lib_uri }, "position": { "line": 2, "character": 22 } }), &mut notes);
    assert!(h["contents"]["value"].as_str().unwrap().contains("private fn page_of(n: int)"), "{}", h);
    let syms = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": lib_uri } }), &mut notes);
    assert_eq!(syms[0]["name"], json!("page_of"));
    assert!(syms[0]["detail"].as_str().unwrap().starts_with("private fn page_of"), "{}", syms);
    assert_eq!(syms[0]["selectionRange"]["start"], json!({ "line": 0, "character": 11 }));
    let def = c.request("textDocument/definition", json!({ "textDocument": { "uri": lib_uri }, "position": { "line": 2, "character": 22 } }), &mut notes);
    assert_eq!(def["range"]["start"], json!({ "line": 0, "character": 11 }), "{}", def);

    let typing = main.replace("<main>", "<main><");
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": main_uri, "version": 2 }, "contentChanges": [{ "text": typing }] }));
    let items = c.request(
        "textDocument/completion",
        json!({ "textDocument": { "uri": main_uri }, "position": { "line": 1, "character": 21 }, "context": { "triggerKind": 2, "triggerCharacter": "<" } }),
        &mut notes,
    );
    let labels: Vec<&str> = items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap()).collect();
    assert!(labels.contains(&"div") && !labels.contains(&"Post"), "{:?}", labels);
}
