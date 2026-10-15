//! `wisp lsp` over stdio, as an editor would use it.

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
        let mut child = Command::new(env!("CARGO_BIN_EXE_wisp"))
            .arg("lsp")
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
    let dir = std::env::temp_dir().join(format!("wisp-lsp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.wisp"), "fn shout(s: str) -> str = upper(s) + \"!\"\n").unwrap();
    let main = dir.join("app.wisp");
    let uri = format!("file://{}", main.canonicalize().unwrap_or(main.clone()).display()).replace("file:///private/", "file:///private/");
    let text = "use \"lib.wisp\"\nfn add(a: int, b: int = 10) -> int = a + b\nlet total = add(1)\nroute GET / = shout(\"hi {totl}\")\n";
    std::fs::write(&main, text).unwrap();

    let mut c = Client::start();
    let mut notes = Vec::new();
    let init = c.request("initialize", json!({ "capabilities": {} }), &mut notes);
    assert_eq!(init["capabilities"]["hoverProvider"], json!(true));
    c.notify("initialized", json!({}));
    c.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "wisp", "version": 1, "text": text } }));

    // Diagnostics: the typo, underlined where it is.
    let d = loop {
        let m = c.recv();
        if m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == json!(uri) {
            break m["params"]["diagnostics"].clone();
        }
    };
    assert_eq!(d.as_array().unwrap().len(), 1, "{}", d);
    assert!(d[0]["message"].as_str().unwrap().contains("unknown name `totl` (did you mean `total`?)"));
    assert_eq!(d[0]["range"], json!({ "start": { "line": 3, "character": 25 }, "end": { "line": 3, "character": 29 } }));

    // Hover on `add` in `add(1)`.
    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 13 } }), &mut notes);
    assert!(h["contents"]["value"].as_str().unwrap().contains("fn add(a: int, b: int = …) -> int"), "{}", h);

    // Definition of `shout`, in the used file.
    let def = c.request("textDocument/definition", json!({ "textDocument": { "uri": uri }, "position": { "line": 3, "character": 15 } }), &mut notes);
    assert!(def["uri"].as_str().unwrap().ends_with("/lib.wisp"), "{}", def);
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

    // Outline.
    let syms = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }), &mut notes);
    let names: Vec<&str> = syms.as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["add", "total", "GET /"]);

    let r = c.request("shutdown", Value::Null, &mut notes);
    assert_eq!(r, Value::Null);
    c.notify("exit", Value::Null);
    assert!(c.child.wait().unwrap().success());
}
