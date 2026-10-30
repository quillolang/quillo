//! `quillo dap`: the Globals scope in order, an expression asked of a frame
//! that isn't there, and a breakpoint in markup with classes.

mod support;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

struct Dap {
    child: Child,
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
        Dap { child, msgs, seq: 1, held: Vec::new() }
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

    fn ask(&mut self, command: &str, args: Value) -> Value {
        let seq = self.seq;
        self.seq += 1;
        let body = json!({ "seq": seq, "type": "request", "command": command, "arguments": args }).to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
        self.wait(command, |m| m["type"] == "response" && m["request_seq"] == seq)
    }

    fn call(&mut self, command: &str, args: Value) -> Value {
        let r = self.ask(command, args);
        assert_eq!(r["success"], true, "{} failed: {}", command, r);
        r["body"].clone()
    }

    /// Starts `dir`'s main.qwe, with breakpoints at `lines`; what they became.
    fn launch(dir: &Path, port: u16, lines: &[u64]) -> (Dap, Vec<Value>) {
        let mut d = Dap::start();
        d.call("initialize", json!({ "adapterID": "quillo", "linesStartAt1": true }));
        d.wait("initialized", |m| m["event"] == "initialized");
        d.call("launch", json!({ "program": dir.join("main.qwe"), "args": ["--port", port.to_string()], "cwd": dir }));
        let bps: Vec<Value> = lines.iter().map(|l| json!({ "line": l })).collect();
        let set = d.call("setBreakpoints", json!({ "source": { "path": dir.join("main.qwe") }, "breakpoints": bps }));
        d.call("configurationDone", json!({}));
        (d, set["breakpoints"].as_array().unwrap().clone())
    }

    fn stopped(&mut self) -> Value {
        self.wait("stop", |m| m["event"] == "stopped")["body"].clone()
    }

    /// The innermost frame's id, and the reference of one of its scopes.
    fn scope(&mut self, name: &str) -> (Value, Value) {
        let st = self.call("stackTrace", json!({ "threadId": 1 }));
        let frame = st["stackFrames"][0]["id"].clone();
        let scopes = self.call("scopes", json!({ "frameId": frame }));
        let s = scopes["scopes"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("{}", scopes)).clone();
        (frame, s["variablesReference"].clone())
    }
}

impl Drop for Dap {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn program(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("debug-corners-{}", name)).canonicalize().unwrap();
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
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

const GLOBALS: &str = "let zebra = 1\nlet apple = 2\nlet mango = 3\nfn add(a, b) {\n    let both = a + b\n    both\n}\nroute GET / = str(add(apple, mango))\n";

#[test]
fn the_globals_are_shown_in_order_of_their_names() {
    let d = program("globals", GLOBALS);
    let port = support::free_port();
    let (mut dap, set) = Dap::launch(&d, port, &[6]);
    assert_eq!(set[0]["verified"], true, "{:?}", set);
    let page = get(port, "/");
    dap.stopped();
    let (_, globals) = dap.scope("Globals");
    let vs = dap.call("variables", json!({ "variablesReference": globals }));
    let names: Vec<&str> = vs["variables"].as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["apple", "mango", "zebra"]);
    dap.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().ends_with("5"));
}

#[test]
fn an_expression_asked_of_no_frame_sees_the_globals() {
    let d = program("frames", GLOBALS);
    let port = support::free_port();
    let (mut dap, _) = Dap::launch(&d, port, &[6]);
    let page = get(port, "/");
    dap.stopped();
    let (frame, _) = dap.scope("Globals");
    let eval = |dap: &mut Dap, code: &str, frame: Value| dap.ask("evaluate", json!({ "expression": code, "frameId": frame, "context": "repl" }));
    // The frame that is stopped sees its locals and the globals.
    assert_eq!(eval(&mut dap, "both + zebra", frame)["body"]["result"], "6");
    // One that isn't there sees only the globals.
    assert_eq!(eval(&mut dap, "zebra + 1", json!(999))["body"]["result"], "2");
    let local = eval(&mut dap, "both", json!(999));
    assert_eq!(local["success"], false, "{}", local);
    assert!(local["message"].as_str().unwrap().contains("unknown name `both`"), "{}", local);
    dap.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().ends_with("5"));
}

const MARKUP: &str = "fn card(xs) {\n    <div.box.wide>\n        {for x in xs {\n            print(x)\n        }}\n    </div>\n}\nroute GET / = card([1])\n";

#[test]
fn a_breakpoint_inside_markup_with_classes_lands_on_its_line() {
    let d = program("classes", MARKUP);
    let (dap, set) = Dap::launch(&d, support::free_port(), &[4]);
    assert_eq!((&set[0]["verified"], &set[0]["line"]), (&json!(true), &json!(4)), "{:?}", set);
    drop(dap);
}

/// It is verified above, so an editor shows it as set; the block's statements
/// run (the `print` prints) without stopping there.
#[test]
#[ignore = "finding: a breakpoint on a statement in a `{for ...}` or `{if ...}` block inside markup is verified but never stops"]
fn a_breakpoint_inside_a_block_in_markup_stops_there() {
    let d = program("in-markup", MARKUP);
    let port = support::free_port();
    let (mut dap, _) = Dap::launch(&d, port, &[4]);
    let page = get(port, "/");
    assert_eq!(dap.stopped()["reason"], "breakpoint");
    dap.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().contains("class=\"box wide\""));
}
