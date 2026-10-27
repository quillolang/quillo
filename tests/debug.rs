//! `quillo dap` over stdio, as an editor's debugger would use it.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

struct Dap {
    child: std::process::Child,
    msgs: Receiver<Value>,
    seq: u64,
    /// Messages read while waiting for another.
    held: Vec<Value>,
    /// What the program printed, from `output` events.
    output: String,
}

impl Dap {
    fn start() -> Dap {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("dap")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
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
        Dap { child, msgs, seq: 1, held: Vec::new(), output: String::new() }
    }

    fn wait(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.held.iter().position(&pred) {
            return self.held.remove(i);
        }
        let end = Instant::now() + Duration::from_secs(20);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            let Ok(m) = self.msgs.recv_timeout(left) else { break };
            if m["event"] == "output" {
                self.output.push_str(m["body"]["output"].as_str().unwrap_or(""));
                continue;
            }
            if pred(&m) {
                return m;
            }
            self.held.push(m);
        }
        panic!("no {} (output so far: {})", what, self.output);
    }

    fn call(&mut self, command: &str, args: Value) -> Value {
        let seq = self.seq;
        self.seq += 1;
        let body = json!({ "seq": seq, "type": "request", "command": command, "arguments": args }).to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
        let r = self.wait(command, |m| m["type"] == "response" && m["request_seq"] == seq);
        assert_eq!(r["success"], true, "{} failed: {}", command, r);
        r["body"].clone()
    }

    fn stopped(&mut self) -> Value {
        self.wait("stop", |m| m["event"] == "stopped")["body"].clone()
    }

    /// (name, line) of each frame, innermost first.
    fn stack(&mut self) -> Vec<(String, u64)> {
        let st = self.call("stackTrace", json!({ "threadId": 1 }));
        st["stackFrames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["name"].as_str().unwrap().to_string(), f["line"].as_u64().unwrap()))
            .collect()
    }

    /// The variables of the innermost frame's scope `scope`, as name → value text.
    fn vars(&mut self, scope: &str) -> Vec<(String, String)> {
        let st = self.call("stackTrace", json!({ "threadId": 1 }));
        let frame = st["stackFrames"][0]["id"].clone();
        let scopes = self.call("scopes", json!({ "frameId": frame }));
        let Some(s) = scopes["scopes"].as_array().unwrap().iter().find(|s| s["name"] == scope) else { return vec![] };
        let r = s["variablesReference"].clone();
        let vs = self.call("variables", json!({ "variablesReference": r }));
        vs["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (v["name"].as_str().unwrap().to_string(), v["value"].as_str().unwrap().to_string()))
            .collect()
    }

    fn eval(&mut self, code: &str) -> String {
        let st = self.call("stackTrace", json!({ "threadId": 1 }));
        let frame = st["stackFrames"][0]["id"].clone();
        let r = self.call("evaluate", json!({ "expression": code, "frameId": frame, "context": "repl" }));
        r["result"].as_str().unwrap().to_string()
    }
}

impl Drop for Dap {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// GET `path` on a thread of its own (the program stops while answering it).
fn get(port: u16, path: &str) -> std::thread::JoinHandle<String> {
    let path = path.to_string();
    std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        let mut s = loop {
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("cannot connect: {}", e),
            }
        };
        write!(s, "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n", path).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    })
}

fn program(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("debug").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, src) in files {
        std::fs::write(dir.join(f), src).unwrap();
    }
    dir.canonicalize().unwrap()
}

/// Starts the program under the debugger with these breakpoints (file, lines).
fn launch(dir: &Path, port: u16, breakpoints: &[(&str, Vec<Value>)], extra: Value) -> (Dap, Vec<Value>) {
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo", "linesStartAt1": true }));
    d.wait("initialized", |m| m["event"] == "initialized");
    let mut args = json!({ "program": dir.join("main.qwe"), "args": ["--port", port.to_string()], "cwd": dir });
    for (k, v) in extra.as_object().unwrap() {
        args[k] = v.clone();
    }
    d.call("launch", args);
    let mut set = Vec::new();
    for (file, bps) in breakpoints {
        let r = d.call("setBreakpoints", json!({ "source": { "path": dir.join(file) }, "breakpoints": bps }));
        set.extend(r["breakpoints"].as_array().unwrap().iter().cloned());
    }
    d.call("setExceptionBreakpoints", json!({ "filters": ["errors"] }));
    d.call("configurationDone", json!({}));
    (d, set)
}

const MAIN: &str = r#"use "lib.qwe"

let greeting = "hello"

fn total(xs) {
    let sum = 0
    for x in xs {
        sum += x
    }
    sum
}

view Card(title) = <p class="card">{shout(title)}</p>

route GET / = {
    let xs = [1, 2, 3]
    let t = total(xs)
    print(`total is ${t}`)
    <main>{Card(`${greeting} ${t}`)}</main>
}

route GET /boom = {
    let a = 1
    a + json.parse("{}")
}
"#;

const LIB: &str = r#"fn shout(s) {
    let loud = upper(s)
    `${loud}!`
}
"#;

#[test]
fn breakpoints_steps_and_variables() {
    let dir = program("steps", &[("main.qwe", MAIN), ("lib.qwe", LIB)]);
    let port = free_port();
    let line = |l: u64| json!({ "line": l });
    let (mut d, set) = launch(&dir, port, &[("main.qwe", vec![line(17), line(4), line(14)]), ("lib.qwe", vec![line(2)])], json!({}));
    // Line 4 (a blank line before `fn`) has nothing to stop at; 14 is a blank line in a route.
    assert_eq!(set[0]["line"], 17);
    assert_eq!(set[1]["verified"], false);
    assert_eq!(set[2]["verified"], false);
    assert_eq!(set[3]["line"], 2);

    let page = get(port, "/");
    let s = d.stopped();
    assert_eq!(s["reason"], "breakpoint");
    assert_eq!(d.stack(), vec![("GET /".to_string(), 17)]);
    assert_eq!(d.vars("Locals"), vec![("xs".to_string(), "(3) [1, 2, 3]".to_string())]);
    let globals = d.vars("Globals");
    assert!(globals.contains(&("greeting".to_string(), "\"hello\"".to_string())), "{:?}", globals);
    assert!(!globals.iter().any(|(n, _)| n == "math" || n == "total" || n == "print"), "{:?}", globals);
    assert!(d.vars("Request").contains(&("path".to_string(), "\"/\"".to_string())));
    assert_eq!(d.eval("xs.len() * 10"), "30");

    // Into `total`, then over its statements, then out.
    d.call("stepIn", json!({ "threadId": 1 }));
    d.stopped();
    assert_eq!(d.stack(), vec![("total".to_string(), 6), ("GET /".to_string(), 17)]);
    d.call("next", json!({ "threadId": 1 }));
    d.stopped();
    assert_eq!(d.stack()[0], ("total".to_string(), 7));
    d.call("next", json!({ "threadId": 1 }));
    d.stopped();
    assert_eq!(d.stack()[0], ("total".to_string(), 8));
    d.call("stepOut", json!({ "threadId": 1 }));
    d.stopped();
    assert_eq!(d.stack(), vec![("GET /".to_string(), 18)]);
    assert!(d.vars("Locals").contains(&("t".to_string(), "6".to_string())));

    // A breakpoint in a `use`d file, reached through a view.
    d.call("continue", json!({ "threadId": 1 }));
    let s = d.stopped();
    assert_eq!(s["reason"], "breakpoint");
    let st = d.call("stackTrace", json!({ "threadId": 1 }));
    let top = &st["stackFrames"][0];
    assert_eq!(top["name"], "shout");
    assert_eq!(top["line"], 2);
    assert_eq!(top["source"]["path"], dir.join("lib.qwe").display().to_string());
    assert_eq!(st["stackFrames"][1]["name"], "Card");
    assert_eq!(st["stackFrames"][2]["name"], "GET /");
    assert_eq!(d.vars("Locals"), vec![("s".to_string(), "\"hello 6\"".to_string())]);

    d.call("continue", json!({ "threadId": 1 }));
    let body = page.join().unwrap();
    assert!(body.contains("<p class=\"card\">HELLO 6!</p>"), "{}", body);
}

#[test]
fn errors_conditions_and_log_points() {
    let dir = program("errors", &[("main.qwe", MAIN), ("lib.qwe", LIB)]);
    let port = free_port();
    let (mut d, _) = launch(
        &dir,
        port,
        &[("main.qwe", vec![json!({ "line": 8, "condition": "x == 2" }), json!({ "line": 18, "logMessage": "t is {t}, twice {t * 2}" })])],
        json!({}),
    );
    let page = get(port, "/");
    let s = d.stopped();
    assert_eq!(s["reason"], "breakpoint");
    assert_eq!(d.stack()[0], ("total".to_string(), 8));
    let vars = d.vars("Locals");
    assert!(vars.contains(&("x".to_string(), "2".to_string())), "{:?}", vars);
    assert!(vars.contains(&("sum".to_string(), "1".to_string())), "{:?}", vars);
    d.call("continue", json!({ "threadId": 1 }));
    page.join().unwrap();

    // A runtime error stops where it happens.
    let page = get(port, "/boom");
    let s = d.stopped();
    assert_eq!(s["reason"], "exception");
    assert_eq!(s["text"], "cannot add int and map");
    assert_eq!(d.stack(), vec![("GET /boom".to_string(), 24)]);
    assert_eq!(d.vars("Locals"), vec![("a".to_string(), "1".to_string())]);
    let info = d.call("exceptionInfo", json!({ "threadId": 1 }));
    assert_eq!(info["description"], "cannot add int and map");
    d.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().starts_with("HTTP/1.1 500"));

    let end = Instant::now() + Duration::from_secs(5);
    while !(d.output.contains("t is 6, twice 12") && d.output.contains("total is 6")) && Instant::now() < end {
        if let Ok(m) = d.msgs.recv_timeout(Duration::from_millis(100)) {
            if m["event"] == "output" {
                d.output.push_str(m["body"]["output"].as_str().unwrap_or(""));
            }
        }
    }
    assert!(d.output.contains("t is 6, twice 12"), "{}", d.output);
    assert!(d.output.contains("total is 6"), "{}", d.output);
}

#[test]
fn stop_on_entry_and_pause() {
    let dir = program("entry", &[("main.qwe", MAIN), ("lib.qwe", LIB)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[], json!({ "stopOnEntry": true }));
    let s = d.stopped();
    assert_eq!(s["reason"], "step");
    assert_eq!(d.stack(), vec![("<main>".to_string(), 1)]);
    d.call("next", json!({ "threadId": 1 }));
    d.stopped();
    assert_eq!(d.stack(), vec![("<main>".to_string(), 3)]);
    d.call("continue", json!({ "threadId": 1 }));

    // Pausing a server that waits for requests stops the next one to run.
    d.call("pause", json!({ "threadId": 1 }));
    let page = get(port, "/");
    let s = d.stopped();
    assert_eq!(s["reason"], "pause");
    assert_eq!(d.stack(), vec![("GET /".to_string(), 16)]);
    d.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().contains("HELLO 6!"));
}
