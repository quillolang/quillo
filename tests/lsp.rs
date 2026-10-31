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
    std::fs::write(&pages, "import lib/util.qwe use shout\nfn home() = shout(\"hi\")\n").unwrap();
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
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": "import lib/util.qwe use shout\nfn home() = shout(\"hi\")\n" } }));
    assert_eq!(diags(&mut c), json!([]));
    // A real mistake still is one, from the text being edited.
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "import lib/util.qwe use shout\nfn home() = shuot(\"hi\")\n" }] }));
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

/// A class: hover shows its rules, references reach its rules and the
/// markup that writes it (from either side), rename changes both, and a
/// class with no rule is told: a warning when a rule is near it, else a
/// hint (branch class-tools).
#[test]
fn classes_are_hovered_found_renamed_and_told_when_unstyled() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-class-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("public/css")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let css = ".chip { color: red; }\n.box .chip { margin: 0; }\n";
    std::fs::write(dir.join("public/css/site.css"), css).unwrap();
    let main = dir.join("main.qwe");
    let text = "route GET / = <div.box><span.chip>a</span><b class=\"chip\">b</b><i.chiip>c</i><i.marker>d</i></div>\n";
    std::fs::write(&main, text).unwrap();
    let uri = format!("file://{}", main.display());
    let css_uri = format!("file://{}/public/css/site.css", dir.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    let init = c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    assert_eq!(init["capabilities"]["referencesProvider"], json!(true));
    assert_eq!(init["capabilities"]["experimental"]["cssClasses"], json!(true));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    let d = loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    let told: Vec<(String, u64)> = d.as_array().unwrap().iter().map(|d| (d["message"].as_str().unwrap().to_string(), d["severity"].as_u64().unwrap())).collect();
    assert!(told.contains(&("no style sheet has a rule for `.chiip` (did you mean `chip`?)".to_string(), 2)), "{:?}", told);
    assert!(told.contains(&("no style sheet has a rule for `.marker`".to_string(), 4)), "{:?}", told);

    let at = |line: u32, character: u32| json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });
    let h = c.request("textDocument/hover", at(0, 31), &mut notes);
    let v = h["contents"]["value"].as_str().unwrap();
    assert!(v.contains("`.chip`: 2 rules") && v.contains("site.css:2") && v.contains(".box .chip { margin: 0; }"), "{}", v);

    let mut p = at(0, 31);
    p["context"] = json!({ "includeDeclaration": true });
    let refs = c.request("textDocument/references", p, &mut notes);
    let found: Vec<(bool, u64, u64)> = refs
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["uri"] == json!(css_uri), r["range"]["start"]["line"].as_u64().unwrap(), r["range"]["start"]["character"].as_u64().unwrap()))
        .collect();
    assert_eq!(found, vec![(true, 0, 1), (true, 1, 6), (false, 0, 29), (false, 0, 52)], "{}", refs);

    // From the style sheet, opened in the editor.
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": css_uri, "languageId": "css", "version": 1, "text": css } }));
    let refs = c.request(
        "textDocument/references",
        json!({ "textDocument": { "uri": css_uri }, "position": { "line": 1, "character": 8 }, "context": { "includeDeclaration": false } }),
        &mut notes,
    );
    assert_eq!(refs.as_array().unwrap().len(), 2, "{}", refs);

    let mut p = at(0, 31);
    p["newName"] = json!("tag");
    let edit = c.request("textDocument/rename", p, &mut notes);
    assert_eq!(edit["changes"][&css_uri].as_array().unwrap().len(), 2, "{}", edit);
    assert_eq!(edit["changes"][&uri].as_array().unwrap().len(), 2, "{}", edit);
    assert_eq!(edit["changes"][&uri][1], json!({ "range": { "start": { "line": 0, "character": 52 }, "end": { "line": 0, "character": 56 } }, "newText": "tag" }));
    assert_eq!(c.request("textDocument/prepareRename", at(0, 3), &mut notes), Value::Null);
}

/// `use` lines: a file reached by a path without quotes is checked as part
/// of its program; the path and the names taken from a `.qwer` file are
/// hovered and followed (a name under another, `as`, too); paths complete
/// to the program's files and names to the file's functions (branch
/// use-editor).
#[test]
fn use_lines_are_hovered_followed_and_completed() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-use-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let main = "use lib/auth\nuse brightness as bright, shout from plugins/img.qwer\nfn site() = \"Q\"\nlet a = bright(\"#fff\")\n";
    std::fs::write(dir.join("main.qwe"), main).unwrap();
    let lib = "fn greet(n) = site() + n\n";
    std::fs::write(dir.join("lib/auth.qwe"), lib).unwrap();
    let rust = "/// How bright a colour is.\npub fn brightness(hex: String) -> f64 {\n    0.5\n}\n\npub fn shout(s: String) -> String {\n    s\n}\n";
    std::fs::write(dir.join("plugins/img.qwer"), rust).unwrap();
    let main_uri = format!("file://{}/main.qwe", dir.display());
    let lib_uri = format!("file://{}/lib/auth.qwe", dir.display());
    let rust_uri = format!("file://{}/plugins/img.qwer", dir.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": main_uri, "languageId": "quillo", "version": 1, "text": main } }));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": lib_uri, "languageId": "quillo", "version": 1, "text": lib } }));
    let at = |uri: &str, line: u32, character: u32| json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });

    let h = c.request("textDocument/hover", at(&lib_uri, 0, 15), &mut notes);
    assert!(h["contents"]["value"].as_str().unwrap().contains("fn site()"), "{}", h);
    let h = c.request("textDocument/hover", at(&main_uri, 1, 21), &mut notes);
    let text = h["contents"]["value"].as_str().unwrap();
    assert!(text.contains("bright(str) -> float") && text.contains("How bright") && text.contains("called `bright` here"), "{}", h);

    let d = c.request("textDocument/definition", at(&main_uri, 0, 6), &mut notes);
    assert_eq!((d["uri"].as_str().unwrap(), &d["range"]["start"]), (lib_uri.as_str(), &json!({ "line": 0, "character": 0 })), "{}", d);
    let expected = json!({ "start": { "line": 1, "character": 7 }, "end": { "line": 1, "character": 17 } });
    for (line, character) in [(1, 8), (1, 21), (3, 9)] {
        let d = c.request("textDocument/definition", at(&main_uri, line, character), &mut notes);
        assert_eq!((d["uri"].as_str().unwrap(), &d["range"]), (rust_uri.as_str(), &expected), "{}:{} {}", line, character, d);
    }

    let labels = |c: &mut Client, notes: &mut Vec<Value>, text: &str, version: u32, character: u32| -> Vec<String> {
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": main_uri, "version": version }, "contentChanges": [{ "text": text }] }));
        let items = c.request("textDocument/completion", at(&main_uri, 0, character), notes);
        items.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(labels(&mut c, &mut notes, "use \n", 2, 4), ["lib/auth.qwe", "plugins/img.qwer"]);
    assert_eq!(labels(&mut c, &mut notes, "use shout from pl\n", 3, 17), ["plugins/img.qwer"]);
    assert_eq!(labels(&mut c, &mut notes, "use  from plugins/img.qwer\n", 4, 4), ["brightness", "shout"]);
    assert_eq!(labels(&mut c, &mut notes, "use shout,  from plugins/img\n", 5, 11), ["brightness"]);
    // What a page does ahead of a browser library (branch library-hints).
    let with = ["module", "preload", "preconnect"];
    assert_eq!(labels(&mut c, &mut notes, "use Chart with \n", 6, 15), with);
    assert_eq!(labels(&mut c, &mut notes, "use Chart with pre\n", 7, 18), with);
    assert_eq!(labels(&mut c, &mut notes, "use Chart with preload \n", 8, 23), ["from"]);
    assert_eq!(labels(&mut c, &mut notes, "use Chart with module, \n", 8, 23), with);
    assert_eq!(labels(&mut c, &mut notes, "use * as THREE with module \n", 8, 27), ["from"]);
    assert_eq!(labels(&mut c, &mut notes, "use *\n", 8, 5), ["as", "from"]);
    assert_eq!(labels(&mut c, &mut notes, "use Chart with preload from pl\n", 9, 30), ["plugins/img.qwer"]);
    assert_eq!(labels(&mut c, &mut notes, "use  with module, preload from plugins/img.qwer\n", 10, 4), ["brightness", "shout"]);
}

/// Inside a call the server gives the signature, the argument reached and a
/// line about it; outside one, nothing. `¦` is the cursor (branch
/// signature-help).
#[test]
fn a_call_shows_its_parameters_and_says_what_it_does() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-signature-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let defs = "// Says hello\n// to someone.\nfn greet(who: str, loud: bool = false) = who\n\nprivate fn twice(n: int) = n * 2\n\n";
    std::fs::write(dir.join("main.qwe"), defs).unwrap();
    let uri = format!("file://{}/main.qwe", dir.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": defs } }));
    let mut version = 1;
    let mut help = |sample: &str| -> Value {
        let text = format!("{}{}", defs, sample);
        let at = text.find('¦').unwrap();
        let (line, column) = (text[..at].matches('\n').count(), text[..at].rsplit('\n').next().unwrap().encode_utf16().count());
        version += 1;
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text.replace('¦', "") }] }));
        c.request("textDocument/signatureHelp", json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": column } }), &mut notes)
    };
    let sig = |h: &Value| h["signatures"][0]["label"].as_str().unwrap_or("").to_string();

    let h = help("let a = contains(¦)\n");
    assert!(sig(&h).starts_with("contains(x: str | list | map | null, y: any) -> bool"), "{}", h);
    assert_eq!(h["activeParameter"], json!(0), "{}", h);
    assert!(h["signatures"][0]["documentation"]["value"].as_str().unwrap().len() > 10, "{}", h);
    let label = sig(&h);
    let span = h["signatures"][0]["parameters"][1]["label"].as_array().unwrap();
    assert_eq!(&label[span[0].as_u64().unwrap() as usize..span[1].as_u64().unwrap() as usize], "y: any");

    assert_eq!(help("let a = contains(\"a,b\", ¦)\n")["activeParameter"], json!(1));
    assert_eq!(help("let a = contains(\"a,¦b\", 1)\n")["activeParameter"], json!(0));
    assert_eq!(help("let a = contains([1, 2¦], 1)\n")["activeParameter"], json!(0));
    assert_eq!(help("let a = contains(\n    \"text\",\n    ¦\n)\n")["activeParameter"], json!(1));
    // A pipe gives the first argument.
    let h = help("let a = [1] | contains(¦)\n");
    assert!(sig(&h).starts_with("contains("), "{}", h);
    assert_eq!(h["activeParameter"], json!(1), "{}", h);
    // The innermost call, wherever it is.
    assert!(sig(&help("let a = contains(len(¦))\n")).starts_with("len("));
    assert!(sig(&help("let a = `${len(¦)}`\n")).starts_with("len("));
    assert!(sig(&help("let a = <p>don't {len(¦)}</p>\n")).starts_with("len("));
    assert!(sig(&help("let a = db.exec(¦)\n")).starts_with("db.exec("));
    assert!(sig(&help("let a = len(`te¦xt`)\n")).starts_with("len("));

    let h = help("let a = greet(\"x\", ¦)\n");
    assert!(sig(&h).starts_with("fn greet(who: str, loud: bool = …)"), "{}", h);
    assert_eq!(h["activeParameter"], json!(1), "{}", h);
    assert_eq!(h["signatures"][0]["documentation"]["value"], json!("Says hello to someone."), "{}", h);
    assert!(sig(&help("let a = twice(¦)\n")).contains("twice(n: int)"));

    // Not in a call.
    for sample in [
        "let a = 1¦\n",
        "let a = len(\"x\")¦\n",
        "fn other(¦) = 1\n",
        "if (¦true) { }\n",
        "let a = (¦1)\n",
        "let a = map([1], (x) => {\n    ¦\n})\n",
        "let a = <p>a (b¦</p>\n",
        "let a = nothing_here(¦)\n",
    ] {
        assert_eq!(help(sample), Value::Null, "{}", sample);
    }
}

/// Each builtin has its line, and a name for each of its parameters (branch
/// signature-help).
#[test]
fn every_builtin_says_what_it_does() {
    use quillo::builtin_docs::DOCS;
    let sigs = quillo::check::builtin_signatures();
    for (name, sig) in sigs.iter().filter(|(n, _)| !n.starts_with("__")) {
        let d = DOCS.iter().find(|d| d.0 == name).unwrap_or_else(|| panic!("{} has no entry in builtin_docs.rs", name));
        let inner = &sig[sig.find('(').unwrap() + 1..];
        let (mut depth, mut types) = (0, if inner.starts_with(')') { 0 } else { 1 });
        for ch in inner.chars() {
            match ch {
                '(' => depth += 1,
                ')' if depth == 0 => break,
                ')' => depth -= 1,
                ',' if depth == 0 => types += 1,
                _ => {}
            }
        }
        assert_eq!(d.1.split(", ").filter(|n| !n.is_empty()).count(), types, "the parameters of {}: {}", name, sig);
        assert!(!d.2.is_empty() && d.2.len() <= 140, "the line for {}", name);
    }
    for d in DOCS {
        assert!(sigs.iter().any(|(n, _)| n == d.0), "{} in builtin_docs.rs is no builtin", d.0);
        assert_eq!(DOCS.iter().filter(|o| o.0 == d.0).count(), 1, "{} twice in builtin_docs.rs", d.0);
    }
}

/// Parameter hints past the usual calls: a plugin's function, parameters whose
/// types hold commas and brackets, text the reading has to step over; and a
/// builtin's line on hover (branch feedback19-fixes).
#[test]
fn hints_know_plugins_awkward_types_and_awkward_text() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-hints-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let plugin = "/// Adds two numbers.\npub fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n";
    let defs = "use lib.qwer\n\nfn pick(kind: \"a,b\" | \"c\", xs: list<int>, m: { a: int, b: int }, last: int) = last\nfn odd(x: \"a\\\"b\", y: int) = y\n\n";
    std::fs::write(dir.join("lib.qwer"), plugin).unwrap();
    std::fs::write(dir.join("main.qwe"), defs).unwrap();
    let uri = format!("file://{}/main.qwe", dir.display());
    let lib = format!("file://{}/lib.qwer", dir.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": defs } }));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": lib, "languageId": "rust", "version": 1, "text": plugin } }));
    let mut version = 1;
    let mut help = |c: &mut Client, sample: &str| -> Value {
        let text = format!("{}{}", defs, sample);
        let at = text.find('¦').unwrap();
        let (line, column) = (text[..at].matches('\n').count(), text[..at].rsplit('\n').next().unwrap().encode_utf16().count());
        version += 1;
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text.replace('¦', "") }] }));
        c.request("textDocument/signatureHelp", json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": column } }), &mut Vec::new())
    };
    let sig = |h: &Value| h["signatures"][0]["label"].as_str().unwrap_or("").to_string();

    // A plugin's function has its parameters' names, from the Rust.
    let h = help(&mut c, "let a = add(1, ¦)\n");
    assert!(sig(&h).contains("add(a: int, b: int)"), "{}", h);
    assert_eq!(h["activeParameter"], json!(1), "{}", h);
    assert_eq!(h["signatures"][0]["documentation"]["value"], json!("Adds two numbers."), "{}", h);
    // Commas within a type's quotes and brackets do not end the parameter.
    let h = help(&mut c, "let a = pick(\"c\", [1], { a: 1, b: 2 }, ¦)\n");
    assert_eq!(h["signatures"][0]["parameters"].as_array().unwrap().len(), 4, "{}", h);
    assert_eq!(h["activeParameter"], json!(3), "{}", h);
    assert!(sig(&h).contains("xs: list<int>"), "{}", h);
    // A signature that does not read to its end has no hint, and nothing fails.
    assert_eq!(help(&mut c, "let a = odd(¦)\n"), Value::Null);
    // An escaped backquote does not end a template.
    assert!(sig(&help(&mut c, "let a = `x\\`y${len(¦)}`\n")).starts_with("len("));
    // A string left open ends with its line.
    assert!(sig(&help(&mut c, "let a = contains(\"open\n    len(¦)\n")).starts_with("len("));

    // A plugin's file has no hints or `use` lines or classes of its own.
    let at_add = json!({ "textDocument": { "uri": lib }, "position": { "line": 1, "character": 8 } });
    assert_eq!(c.request("textDocument/signatureHelp", at_add.clone(), &mut notes), Value::Null);
    assert!(c.request("textDocument/definition", at_add, &mut notes).is_object());

    // A builtin's hover has the line about it.
    let text = "let a = trim(\" x \")\n";
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 99 }, "contentChanges": [{ "text": text }] }));
    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": uri }, "position": { "line": 0, "character": 9 } }), &mut notes);
    let shown = h["contents"]["value"].as_str().unwrap();
    assert!(shown.contains("trim(") && shown.contains("without the whitespace at both ends"), "{}", h);

    let _ = quillo::lsp::Server::default();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Completion offers what is in scope at the cursor, nearest first: not
/// another function's locals, nor a `let` before its line, and a line just
/// typed in a block still sees the block's names (branch signature-help).
#[test]
fn completion_offers_the_variables_in_scope() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-scope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let src = "fn other(far) {\n    let elsewhere = far\n    elsewhere\n}\n\nfn f(first, second) {\n    let early = first\n    if early {\n        let inner = second\n        inner\n    }\n    let late = early\n    for item in [late] {\n        item\n    }\n    late\n}\n\nroute GET /rooms/:room {\n    let found = room\n    found\n}\n";
    std::fs::write(dir.join("main.qwe"), src).unwrap();
    let uri = format!("file://{}/main.qwe", dir.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": src } }));
    let mut version = 1;
    let mut names = |text: &str, line: u32| -> Vec<String> {
        version += 1;
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }));
        let items = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": 8 } }), &mut notes);
        let mut local: Vec<(String, String)> = items.as_array().unwrap().iter().filter(|i| i["sortText"].is_string()).map(|i| (i["sortText"].as_str().unwrap().to_string(), i["label"].as_str().unwrap().to_string())).collect();
        local.sort();
        local.into_iter().map(|l| l.1).collect()
    };
    assert_eq!(names(src, 9), ["inner", "early", "first", "second"]);
    assert_eq!(names(src, 13), ["item", "late", "early", "first", "second"]);
    assert_eq!(names(src, 15), ["late", "early", "first", "second"]);
    assert_eq!(names(src, 20), ["found", "request", "room"]);
    assert_eq!(names(src, 2), ["elsewhere", "far"]);
    assert!(names(src, 4).is_empty());

    // A line typed at the end of a block, and one that does not parse yet.
    let typed = src.replace("    late\n}", "    late\n    \n}");
    assert_eq!(names(&typed, 16), ["late", "early", "first", "second"]);
    let typed = src.replace("    late\n}", "    late\n    let x = \n}");
    assert_eq!(names(&typed, 16), ["late", "early", "first", "second"]);
    // Past the block's end, nothing of it.
    let typed = src.replace("    late\n}\n", "    late\n}\n\n");
    assert!(names(&typed, 18).is_empty());
}

/// One name in two files, and names taken as others (FEEDBACK19.md, Q-154; branch
/// feedback19-fixes): each file's word goes to what that file means by it.
#[test]
fn names_of_files_are_found_as_each_file_wrote_them() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-taken-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("lib/a.qwe"), "fn helper(x: int) -> str = \"a\"\nview Card(n: int) = <b>{n}</b>\nfn only_a() = 1\nprivate fn secret() = 1\n").unwrap();
    std::fs::write(dir.join("lib/b.qwe"), "fn helper(y: str, helper2 = 1) -> str = \"b\"\nview Card(n: int) = <i>{n}</i>\nroute GET /b = \"b\"\n").unwrap();
    let text = "use helper as a_helper, Card as Tile from lib/a.qwe\nuse helper, Card from lib/b.qwe\nuse only_a from lib/a\nprint(a_helper(1), helper(\"x\"), only_a())\nroute GET / = <Tile n={1}/>\nroute GET /c = <Card n={2}/>\n";
    let main = dir.join("main.qwe");
    std::fs::write(&main, text).unwrap();
    let uri = format!("file://{}", main.display());

    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("initialized", json!({}));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    let at = |line: u32, character: u32| json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });
    let shown = |r: &Value| r["contents"]["value"].as_str().unwrap_or("").to_string();
    let goes = |r: &Value| (r["uri"].as_str().unwrap_or("").rsplit('/').next().unwrap().to_string(), r["range"]["start"]["line"].as_u64());
    let labels = |r: &Value| r.as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let line3 = "print(a_helper(1), helper(\"x\"), only_a())";
    let col = |word: &str| line3.find(word).unwrap() as u32 + 1;

    // In the code: the alias, and a name two files have.
    let hover = c.request("textDocument/hover", at(3, col("a_helper")), &mut notes);
    assert!(shown(&hover).contains("fn helper(x: int) -> str") && !shown(&hover).contains("private"), "{}", hover);
    assert_eq!(goes(&c.request("textDocument/definition", at(3, col("a_helper")), &mut notes)), ("a.qwe".to_string(), Some(0)));
    let hover = c.request("textDocument/hover", at(3, col("helper(\"x\")")), &mut notes);
    assert!(shown(&hover).contains("fn helper(y: str") && !shown(&hover).contains("private"), "{}", hover);
    assert_eq!(goes(&c.request("textDocument/definition", at(3, col("helper(\"x\")")), &mut notes)), ("b.qwe".to_string(), Some(0)));
    // What a call shows, and what is offered.
    let help = c.request("textDocument/signatureHelp", at(3, col("a_helper(1") + 8), &mut notes);
    assert!(help["signatures"][0]["label"].as_str().unwrap().contains("x: int"), "{}", help);
    let items = labels(&c.request("textDocument/completion", at(3, 0), &mut notes));
    assert!(["a_helper", "helper", "only_a"].iter().all(|n| items.iter().any(|i| i == n)), "{:?}", items);
    assert!(!items.iter().any(|i| i.contains("__pv")), "{:?}", items);
    let tags = labels(&c.request("textDocument/completion", at(4, 15), &mut notes));
    assert!(tags.iter().any(|t| t == "Tile") && tags.iter().any(|t| t == "Card"), "{:?}", tags);

    // On a `use` line, the name goes to the file it is taken from, with or without `.qwe`.
    let hover = c.request("textDocument/hover", at(0, 5), &mut notes);
    assert!(shown(&hover).contains("fn helper(x: int)") && shown(&hover).contains("From a.qwe, called `a_helper` here"), "{}", hover);
    let hover = c.request("textDocument/hover", at(2, 5), &mut notes);
    assert!(shown(&hover).contains("fn only_a()") && !shown(&hover).contains("called"), "{}", hover);
    assert_eq!(goes(&c.request("textDocument/definition", at(1, 5), &mut notes)), ("b.qwe".to_string(), Some(0)));
    assert_eq!(goes(&c.request("textDocument/definition", at(2, 5), &mut notes)), ("a.qwe".to_string(), Some(2)));

    // In a file that defines one, its own is plain, and not called private.
    let lib = dir.join("lib/a.qwe");
    let lib_uri = format!("file://{}", lib.display());
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": lib_uri, "languageId": "quillo", "version": 1, "text": std::fs::read_to_string(&lib).unwrap() } }));
    let hover = c.request("textDocument/hover", json!({ "textDocument": { "uri": lib_uri }, "position": { "line": 0, "character": 4 } }), &mut notes);
    assert!(shown(&hover).starts_with("```quillo\nfn helper(x: int)"), "{}", hover);

    // Naming what a `.qwe` file offers: its public names, less those taken.
    let line = "client /assets/chart.js use Chart\n";
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": line }] }));
    let hover = c.request("textDocument/hover", at(0, 12), &mut notes);
    assert!(shown(&hover).contains("/assets/chart.js"), "{}", hover);
    let typed = "use helper, only from lib/a.qwe\n";
    c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 3 }, "contentChanges": [{ "text": typed }] }));
    let names = labels(&c.request("textDocument/completion", at(0, 16), &mut notes));
    assert!(names.iter().any(|n| n == "only_a") && names.iter().any(|n| n == "Card"), "{:?}", names);
    assert!(!names.iter().any(|n| n == "helper" || n.contains("secret")), "{:?}", names);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The file first, so the editor knows the names as they are typed (FEEDBACK19.md, Q-156;
/// branch feedback19-fixes).
#[test]
fn imports_are_hovered_followed_and_completed_file_first() {
    let dir = std::env::temp_dir().join(format!("quillo-lsp-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::create_dir_all(dir.join("public/assets")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join("lib/forum.qwe"), "fn latest() = 1\nfn older(n: int) = n\nprivate fn hidden() = 2\n").unwrap();
    std::fs::write(dir.join("lib/deep.qwe"), "fn deep() = 3\n").unwrap();
    std::fs::write(dir.join("rusty.qwer"), "/// Shouts.\npub fn shout(s: String) -> String {\n    s\n}\n").unwrap();
    std::fs::write(dir.join("public/assets/map.js"), "").unwrap();
    std::fs::write(dir.join("public/assets/m.mjs"), "").unwrap();
    std::fs::write(dir.join("other.js"), "").unwrap();
    let text = concat!(
        "import lib/forum.qwe use latest, older as past\n", // 0
        "import lib/forum.qwe // the rest\n",                // 1
        "    use *\n",                                       // 2
        "import lib/forum\n",                                // 3
        "        use latest\n",                              // 4
        "import rusty use shout\n",                          // 5
        "client \"https://a.b/c.js\"\n",                      // 6
        "    use Chart\n",                                   // 7
        "let sql = `\n",                                     // 8
        "import\n",                                          // 9
        "client\n",                                          // 10
        "`\n",                                               // 11
        "print(latest(), past(1), shout(\"a\"), deep())\n",  // 12
    );
    let main = dir.join("main.qwe");
    std::fs::write(&main, format!("{}import lib/deep.qwe use deep\n", text)).unwrap();
    let text = format!("{}import lib/deep.qwe use deep\n", text);
    let uri = format!("file://{}", main.display());
    let mut c = Client::start();
    let mut notes = Vec::new();
    c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    c.notify("initialized", json!({}));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    let at = |line: u32, character: u32| json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } });
    let shown = |r: &Value| r["contents"]["value"].as_str().unwrap_or("").to_string();
    let goes = |r: &Value| (r["uri"].as_str().unwrap_or("").rsplit('/').next().unwrap().to_string(), r["range"]["start"]["line"].as_u64());

    // The file, and a name, on one line.
    assert!(shown(&c.request("textDocument/hover", at(0, 10), &mut notes)).contains("run once"));
    assert!(shown(&c.request("textDocument/hover", at(0, 26), &mut notes)).contains("fn latest()"));
    let h = c.request("textDocument/hover", at(0, 44), &mut notes);
    assert!(shown(&h).contains("fn older(n: int)") && shown(&h).contains("called `past` here"), "{}", h);
    assert_eq!(goes(&c.request("textDocument/definition", at(0, 10), &mut notes)), ("forum.qwe".to_string(), Some(0)));
    assert_eq!(goes(&c.request("textDocument/definition", at(0, 35), &mut notes)), ("forum.qwe".to_string(), Some(1)));
    // On two lines: the file from its own line, a comment after it, and a name from the next,
    // deeper than the file's name is long; the space before it is neither.
    assert_eq!(goes(&c.request("textDocument/definition", at(3, 10), &mut notes)), ("forum.qwe".to_string(), Some(0)));
    assert!(shown(&c.request("textDocument/hover", at(4, 13), &mut notes)).contains("fn latest()"));
    assert_eq!(goes(&c.request("textDocument/definition", at(4, 13), &mut notes)), ("forum.qwe".to_string(), Some(0)));
    assert!(shown(&c.request("textDocument/hover", at(1, 10), &mut notes)).contains("run once"));
    assert!(c.request("textDocument/hover", at(4, 2), &mut notes).is_null());
    // A file's Rust names, and an address, which has nothing to read.
    assert!(shown(&c.request("textDocument/hover", at(5, 18), &mut notes)).contains("Shouts."));
    assert!(shown(&c.request("textDocument/hover", at(6, 12), &mut notes)).contains("no such file"));
    // The word on its own, as in a string, is not an import, and an open line with no `use`
    // after it is not one either.
    assert!(!shown(&c.request("textDocument/hover", at(9, 2), &mut notes)).contains("run once"));
    // What the words are.
    assert!(shown(&c.request("textDocument/hover", at(0, 2), &mut notes)).contains("import lib/auth.qwe use login"));
    // A file opened alone finds its program through these lines, past the Rust file above it.
    let deep = dir.join("lib/deep.qwe");
    let deep_uri = format!("file://{}", deep.display());
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": deep_uri, "languageId": "quillo", "version": 1, "text": "fn deep() = shout(\"a\")\n" } }));
    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": deep_uri }, "position": { "line": 0, "character": 13 } }), &mut notes);
    assert!(shown(&h).contains("shout"), "{}", h);

    // Typing, a stage at a time.
    let base = text.clone();
    let mut version = 2;
    let mut typed = |c: &mut Client, notes: &mut Vec<Value>, above: &str, line: &str| {
        let whole = format!("{}{}", above, line);
        c.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": whole }] }));
        version += 1;
        let n = above.matches('\n').count() as u32;
        let r = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": n, "character": line.chars().count() as u32 } }), notes);
        r.as_array().unwrap_or(&Vec::new()).iter().map(|i| i["label"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    let files = ["lib/deep.qwe", "lib/forum.qwe", "rusty.qwer"];
    assert_eq!(typed(&mut c, &mut notes, &base, "import "), files);
    assert_eq!(typed(&mut c, &mut notes, &base, "import li"), files);
    assert_eq!(typed(&mut c, &mut notes, &base, "import \"li"), files);
    assert_eq!(typed(&mut c, &mut notes, &base, "import lib/forum.qwe "), ["use"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "import lib/forum.qwe use "), ["*", "latest", "older"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "import lib/forum use la"), ["*", "latest", "older"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "import lib/forum.qwe use latest, "), ["older"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "import lib/forum.qwe use latest as "), Vec::<String>::new());
    assert_eq!(typed(&mut c, &mut notes, &base, "import lib/forum.qwe use * "), ["as"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "import rusty.qwer use "), ["*", "shout"]);
    let scripts = ["other.js", "/assets/m.mjs", "/assets/map.js"];
    assert_eq!(typed(&mut c, &mut notes, &base, "client "), [&["module"][..], &scripts[..]].concat());
    assert_eq!(typed(&mut c, &mut notes, &base, "preload module "), scripts);
    assert_eq!(typed(&mut c, &mut notes, &base, "preconnect \"htt"), scripts);
    assert_eq!(typed(&mut c, &mut notes, &base, "client /assets/map.js "), ["use"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "client /assets/map.js use "), ["*", "default"]);
    assert_eq!(typed(&mut c, &mut notes, &base, "client module /a.js use go, "), ["default"]);
    // A name is a name: such a line is offered what any line is.
    assert!(typed(&mut c, &mut notes, &base, "client = cl").iter().any(|l| l == "let"));
    assert!(typed(&mut c, &mut notes, &base, "le").iter().any(|l| l == "let"));
    assert!(typed(&mut c, &mut notes, &base, "impor").iter().any(|l| l == "import"));
    assert!(typed(&mut c, &mut notes, &base, "  cli").iter().any(|l| l == "client"));
    assert!(!typed(&mut c, &mut notes, &base, "let x = pre").iter().any(|l| l == "preload"));

    // The next line: the names of the file above; below anything else, or first in a file,
    // it is a `use` of a file.
    let above = format!("{}import lib/forum.qwe\n", base);
    assert_eq!(typed(&mut c, &mut notes, &above, "    use "), ["*", "latest", "older"]);
    assert_eq!(typed(&mut c, &mut notes, &above, "    use la"), ["*", "latest", "older"]);
    assert_eq!(typed(&mut c, &mut notes, &format!("{}let x = 1\n", base), "use li"), files);
    assert_eq!(typed(&mut c, &mut notes, "", "use "), files);
    let _ = std::fs::remove_dir_all(&dir);
}
