//! A function and a view failing under `quillo dap`: the program stops where
//! it failed, in the call that failed.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

mod support;

struct Dap {
    child: support::Guard,
    msgs: Receiver<Value>,
    seq: u64,
    held: Vec<Value>,
}

impl Dap {
    fn start() -> Dap {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("dap").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let (tx, msgs) = channel();
        std::thread::spawn(move || loop {
            let mut len = 0;
            loop {
                let mut line = String::new();
                if out.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(v) = line.strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut buf = vec![0; len];
            out.read_exact(&mut buf).unwrap();
            if tx.send(serde_json::from_slice::<Value>(&buf).unwrap()).is_err() {
                return;
            }
        });
        Dap { child: support::Guard::new(child), msgs, seq: 1, held: Vec::new() }
    }

    fn wait(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.held.iter().position(&pred) {
            return self.held.remove(i);
        }
        let end = Instant::now() + Duration::from_secs(20);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            let Ok(m) = self.msgs.recv_timeout(left) else { break };
            if pred(&m) {
                return m;
            }
            self.held.push(m);
        }
        panic!("no {}", what);
    }

    fn call(&mut self, command: &str, args: Value) -> Value {
        let seq = self.seq;
        self.seq += 1;
        let body = json!({ "seq": seq, "type": "request", "command": command, "arguments": args }).to_string();
        let stdin = self.child.0.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
        let r = self.wait(command, |m| m["type"] == "response" && m["request_seq"] == seq);
        assert_eq!(r["success"], true, "{} failed: {}", command, r);
        r["body"].clone()
    }

    /// The error it stopped on, and the frames' names, innermost first.
    fn stopped(&mut self) -> (String, Vec<String>) {
        let s = self.wait("stop", |m| m["event"] == "stopped")["body"].clone();
        assert_eq!(s["reason"], "exception", "{}", s);
        let st = self.call("stackTrace", json!({ "threadId": 1 }));
        let names = st["stackFrames"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect();
        (s["text"].as_str().unwrap().to_string(), names)
    }
}

const MAIN: &str = r#"fn boom(x) = x + {}
view Bad(x) = <i>{x + {}}</i>
view Fine(x) = <i>{x}</i>
route GET /fn = boom(1)
route GET /view = <Bad x={1}/>
route GET /fine = <Fine x={1}/>
"#;

#[test]
fn a_function_and_a_view_stop_where_they_fail() {
    let dir = support::temp_dir("core-debug").canonicalize().unwrap();
    std::fs::write(dir.join("main.qwe"), MAIN).unwrap();
    let port = support::free_port();
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo", "linesStartAt1": true }));
    d.wait("initialized", |m| m["event"] == "initialized");
    d.call("launch", json!({ "program": dir.join("main.qwe"), "args": ["--port", port.to_string()], "cwd": dir }));
    d.call("setExceptionBreakpoints", json!({ "filters": ["errors"] }));
    d.call("configurationDone", json!({}));
    let end = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < end, "the program did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(support::send(port, "GET", "/fine", &[], "").body, quillo::eval_str("<i>{1}</i>").unwrap_or(quillo::Value::Nil).repr());
    for (path, frame) in [("/fn", "boom"), ("/view", "Bad")] {
        let page = std::thread::spawn(move || support::send(port, "GET", path, &[], "").status);
        let (text, frames) = d.stopped();
        assert_eq!(text, "cannot add int and map");
        assert_eq!(frames[0], frame, "{:?}", frames);
        d.call("continue", json!({ "threadId": 1 }));
        assert_eq!(page.join().unwrap(), 500);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
