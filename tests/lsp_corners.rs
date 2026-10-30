//! `quillo lsp` on a document with no folder of its own, and on a class rule
//! written in a `.qwe` file.

mod support;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

/// An editor: requests answered in turn, notifications kept apart.
struct Editor {
    child: Child,
    out: BufReader<ChildStdout>,
    next: u64,
    notes: Vec<Value>,
}

impl Editor {
    fn start(cwd: &Path) -> Editor {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("lsp")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut e = Editor { child, out, next: 1, notes: Vec::new() };
        e.request("initialize", json!({ "capabilities": {} }));
        e.notify("initialized", json!({}));
        e
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

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let m = self.recv();
            if m.get("id") == Some(&json!(id)) {
                return m["result"].clone();
            }
            self.notes.push(m);
        }
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } }));
    }

    fn at(&mut self, method: &str, uri: &str, line: usize, character: usize) -> Value {
        self.request(method, json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }))
    }

    /// The messages of the diagnostics published for `uri`, the last time.
    fn diagnostics(&mut self, uri: &str) -> Vec<String> {
        loop {
            let last = self.notes.iter().rev().find(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri);
            if let Some(m) = last {
                return m["params"]["diagnostics"].as_array().unwrap().iter().map(|d| d["message"].as_str().unwrap().to_string()).collect();
            }
            let m = self.recv();
            self.notes.push(m);
        }
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_document_with_no_folder_is_still_checked() {
    let d = support::temp_dir("lsp-corners-check");
    let mut e = Editor::start(&d);
    e.open("file:///", "let x = 1\nprint(zzqq)\n");
    assert_eq!(e.diagnostics("file:///"), ["unknown name `zzqq`"]);
}

#[test]
fn a_class_in_a_rule_in_a_qwe_file_can_be_renamed() {
    let d = support::temp_dir("lsp-corners-rule").canonicalize().unwrap();
    let text = "route GET / = <span.chip>a</span>\ncss(\".chip { a: b }\")\nlet note = \"see .chip\"\n";
    std::fs::write(d.join("main.qwe"), text).unwrap();
    let uri = format!("file://{}", d.join("main.qwe").display());
    let mut e = Editor::start(&d);
    e.open(&uri, text);
    assert_eq!(e.diagnostics(&uri), Vec::<String>::new());
    let at = e.at("textDocument/prepareRename", &uri, 1, 8);
    assert_eq!(at, json!({ "range": { "start": { "line": 1, "character": 6 }, "end": { "line": 1, "character": 10 } }, "placeholder": "chip" }));
    // After a dot, but on a line with no such rule: only text.
    assert_eq!(e.at("textDocument/prepareRename", &uri, 2, 17), Value::Null);
}
