//! `quillo dap`, past the plain path: hit conditions and log messages, the
//! values an editor is shown, where breakpoints land, and requests it refuses.

mod support;

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

    /// The whole response to a request, whether it succeeded or not.
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

    /// The message a request failed with.
    fn refused(&mut self, command: &str, args: Value) -> String {
        let r = self.ask(command, args);
        assert_eq!(r["success"], false, "{} worked: {}", command, r);
        r["message"].as_str().unwrap().to_string()
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

    #[allow(dead_code)]
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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("debug_edges").join(name);
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


fn free_port() -> u16 {
    support::free_port()
}

/// The variables under a reference: name, value text and reference.
fn under(d: &mut Dap, reference: &Value, more: Value) -> Vec<(String, String, u64)> {
    let mut args = json!({ "variablesReference": reference });
    for (k, v) in more.as_object().unwrap() {
        args[k] = v.clone();
    }
    let vs = d.call("variables", args);
    vs["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["name"].as_str().unwrap().to_string(), v["value"].as_str().unwrap().to_string(), v["variablesReference"].as_u64().unwrap_or(0)))
        .collect()
}

/// The innermost frame's locals, whole.
fn locals(d: &mut Dap) -> Vec<(String, String, u64)> {
    let st = d.call("stackTrace", json!({ "threadId": 1 }));
    let frame = st["stackFrames"][0]["id"].clone();
    let scopes = d.call("scopes", json!({ "frameId": frame }));
    let r = scopes["scopes"].as_array().unwrap().iter().find(|s| s["name"] == "Locals").unwrap()["variablesReference"].clone();
    under(d, &r, json!({}))
}

fn frame(d: &mut Dap) -> Value {
    d.call("stackTrace", json!({ "threadId": 1 }))["stackFrames"][0]["id"].clone()
}

#[test]
fn requests_it_cannot_do_are_refused_with_a_reason() {
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo" }));
    d.wait("initialized", |m| m["event"] == "initialized");
    assert_eq!(d.refused("launch", json!({})), "no program to debug: set \"program\" in launch.json");
    let bad = d.refused("launch", json!({ "program": "x.qwe", "cwd": "/nonexistent/quillo" }));
    assert!(bad.starts_with("cannot use /nonexistent/quillo as the folder to run in: "), "{}", bad);
    assert_eq!(d.refused("attach", json!({})), "attaching to a running program is not supported yet: use a \"launch\" configuration");
    assert_eq!(d.refused("gotoTargets", json!({})), "`gotoTargets` is not supported");
    assert_eq!(d.refused("evaluate", json!({ "expression": "1" })), "the program is running");
    // The error is also where the protocol's clients look for it.
    let r = d.ask("scopes", json!({ "frameId": 1 }));
    assert_eq!(r["body"]["error"]["format"], "the program is running");
    assert_eq!(d.call("stackTrace", json!({ "threadId": 1 }))["stackFrames"], json!([]));
    // Going on when nothing is stopped does nothing.
    d.call("continue", json!({ "threadId": 1 }));
    assert_eq!(d.refused("evaluate", json!({ "expression": "1" })), "the program is running");
}

#[test]
fn configuration_with_nothing_launched_says_so() {
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo" }));
    d.call("configurationDone", json!({}));
    // `wait` keeps output apart: ask something to see it has arrived.
    d.call("threads", json!({}));
    assert!(d.output.contains("quillo dap: nothing was launched"), "{}", d.output);
}

#[test]
fn terminate_ends_the_session_with_an_event() {
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo" }));
    d.call("terminate", json!({}));
    d.wait("terminated", |m| m["event"] == "terminated");
    let end = Instant::now() + Duration::from_secs(10);
    while d.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < end, "it did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(d.child.try_wait().unwrap().unwrap().code(), Some(0));
}

#[test]
fn hit_conditions_pick_which_hits_count() {
    let src = "fn spin() {\n    for i in range(1, 7) {\n        let a = i\n        let b = i\n        let c = i\n        let d = i\n        let e = i\n        let f = i\n        let g = i\n        print(a, b, c, d, e, f, g)\n    }\n}\nroute GET / = spin()\n";
    let dir = program("hits", &[("main.qwe", src)]);
    let port = free_port();
    let bp = |line: u64, hit: &str, tag: &str| json!({ "line": line, "hitCondition": hit, "logMessage": format!("{} {{i}}", tag) });
    let (mut d, _) = launch(
        &dir,
        port,
        &[("main.qwe", vec![bp(3, "== 2", "eq"), bp(4, "= 3", "is"), bp(5, "> 4", "gt"), bp(6, "% 2", "every"), bp(7, ">= 5", "ge"), bp(8, "6", "plain"), bp(9, "soon", "any")])],
        json!({}),
    );
    let page = get(port, "/");
    page.join().unwrap();
    d.call("threads", json!({}));
    let mut seen: Vec<&str> = d.output.lines().filter(|l| l.split(' ').count() == 2).collect();
    seen.sort();
    assert_eq!(
        seen,
        ["any 1", "any 2", "any 3", "any 4", "any 5", "any 6", "eq 2", "every 2", "every 4", "every 6", "ge 5", "ge 6", "gt 5", "gt 6", "is 3", "plain 6"],
        "{}",
        d.output
    );
}

#[test]
fn log_messages_show_what_they_can() {
    let src = "fn f(n) {\n    let a = n\n    a + 1\n}\nroute GET / = f(1)\n";
    let dir = program("logs", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(
        &dir,
        port,
        &[("main.qwe", vec![json!({ "line": 2, "logMessage": "n is {n}, then {nope}, then {reply 404}, then {oops" }), json!({ "line": 3, "condition": "nope", "logMessage": "the condition failed" })])],
        json!({}),
    );
    let page = get(port, "/");
    // A condition that fails to run stops nothing here, being a log message: it logs.
    let out = page.join().unwrap();
    assert!(out.contains("200 OK"), "{}", out);
    d.call("threads", json!({}));
    assert!(d.output.contains("n is 1, then <"), "{}", d.output);
    assert!(d.output.contains("`reply` cannot be used here"), "{}", d.output);
    assert!(d.output.contains(", then {oops"), "{}", d.output);
    assert!(d.output.contains("the condition failed"), "{}", d.output);
}

#[test]
fn a_route_with_parameters_is_named_by_its_path() {
    let src = "route GET /notes/:id(int)/*rest = {\n    let a = id\n    a\n}\n";
    let dir = program("route-name", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[("main.qwe", vec![json!({ "line": 2 })])], json!({}));
    let page = get(port, "/notes/7/a/b");
    d.stopped();
    assert_eq!(d.stack(), vec![("GET /notes/:id/*rest".to_string(), 2)]);
    d.call("continue", json!({ "threadId": 1 }));
    page.join().unwrap();
}

#[test]
fn an_error_in_a_body_of_one_expression_stops_there() {
    let src = "route GET /boom = 1 + json.parse(\"{\")\n";
    let dir = program("expr-error", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[], json!({}));
    let page = get(port, "/boom");
    let s = d.stopped();
    assert_eq!(s["reason"], "exception");
    let info = d.call("exceptionInfo", json!({ "threadId": 1 }));
    assert_eq!(info["exceptionId"], "runtime error");
    assert_eq!(info["description"], "cannot add int and error");
    assert_eq!(d.stack()[0].1, 1);
    d.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().contains("500"));
}

#[test]
fn an_error_before_any_statement_still_shows_its_variables() {
    let src = "route GET /m/:id = { a: 1, b: 1 / (len(id) - 1) }\n";
    let dir = program("no-statement", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[], json!({}));
    let page = get(port, "/m/7");
    assert_eq!(d.stopped()["reason"], "exception");
    assert_eq!(d.stack()[0].0, "GET /m/:id");
    assert_eq!(d.vars("Locals"), vec![("id".to_string(), "\"7\"".to_string())]);
    d.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().contains("500"));
}

const VALUES: &str = r#"fn named(x) = x
fn values() {
    let big = range(0, 600)
    let small = [1, 2]
    let empty = []
    let wide = reduce(range(0, 200), {}, (m, i) => set(m, `key${i}`, i))
    let many = range(0, 200) | map((i) => `text ${i}`)
    let long = repeat("x", 500)
    let failed = err("no")
    let answered = respond(201, "body", { "x-a": "1" })
    let sent = stream((out) => 1)
    let fun = named
    let lam = (x) => x
    let builtin = len
    let page = <p class="a">hi</p>
    let nested = [err("deep"), respond(404), named, (y) => y, len, { a: [1] }]
    let done = 1
    done
}
route GET / = values()
"#;

#[test]
fn the_values_an_editor_is_shown() {
    let dir = program("values", &[("main.qwe", VALUES)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[("main.qwe", vec![json!({ "line": 18 })])], json!({}));
    let page = get(port, "/");
    d.stopped();
    let vars = locals(&mut d);
    let value = |n: &str| vars.iter().find(|v| v.0 == n).unwrap_or_else(|| panic!("no {} in {:?}", n, vars)).clone();

    // A long list is cut short, and pages through its items.
    let big = value("big");
    assert!(big.1.starts_with("(600) [0, 1, 2") && big.1.ends_with("…]"), "{}", big.1);
    assert!(big.2 > 0);
    let some = under(&mut d, &json!(big.2), json!({ "start": 590, "count": 3 }));
    assert_eq!(some.iter().map(|v| (v.0.as_str(), v.1.as_str())).collect::<Vec<_>>(), [("590", "590"), ("591", "591"), ("592", "592")]);
    assert_eq!(under(&mut d, &json!(big.2), json!({})).len(), 600);

    assert_eq!(value("small").1, "(2) [1, 2]");
    assert_eq!(value("empty").2, 0);
    let wide = value("wide");
    assert!(wide.1.starts_with("{key0: 0, key1: 1") && wide.1.ends_with("…}"), "{}", wide.1);
    assert!(value("many").1.ends_with("…]"), "{}", value("many").1);
    let long = value("long");
    assert!(long.1.ends_with('…') && long.1.chars().count() < 300, "{}", long.1);

    // An error and a response open to what they hold.
    let failed = value("failed");
    assert_eq!(failed.1, "err(\"no\")");
    assert_eq!(under(&mut d, &json!(failed.2), json!({})).iter().map(|v| (v.0.clone(), v.1.clone())).collect::<Vec<_>>(), [("error".to_string(), "\"no\"".to_string())]);
    let answered = value("answered");
    assert_eq!(answered.1, "response 201");
    let held = under(&mut d, &json!(answered.2), json!({}));
    assert_eq!(held[0].0, "status");
    assert_eq!(held[0].1, "201");
    assert_eq!(held[1].0, "headers");
    assert_eq!(held[2].0, "body");
    assert_eq!(held[2].1, "\"body\"");
    // A file sent has no value for a body.
    assert_eq!(under(&mut d, &json!(value("sent").2), json!({})).len(), 2);

    assert_eq!(value("fun").1, "fn named(…)");
    assert_eq!(value("lam").1, "(…) => …");
    assert_eq!(value("builtin").1, "builtin len");
    assert!(value("page").1.starts_with("html "), "{}", value("page").1);
    let nested = value("nested");
    assert_eq!(nested.1, "(6) [err(\"deep\"), response 404, fn named(…), (…) => …, builtin len, {a: [1]}]");

    // Types, as the editor's tooltip gives them.
    let raw = d.call("variables", json!({ "variablesReference": nested.2 }));
    let types: Vec<&str> = raw["variables"].as_array().unwrap().iter().map(|v| v["type"].as_str().unwrap()).collect();
    assert_eq!(&types[2..4], ["fn named", "fn"]);
    // A long list says how many it has, for an editor to page.
    let f = frame(&mut d);
    let r = d.call("evaluate", json!({ "expression": "big", "frameId": f, "context": "watch" }));
    assert_eq!(r["indexedVariables"], 600);
    // A reference that is no more, and a plain value, have nothing under them.
    assert_eq!(under(&mut d, &json!(100_000), json!({})), vec![]);
    d.call("continue", json!({ "threadId": 1 }));
    page.join().unwrap();
}

#[test]
fn a_program_stops_at_its_top_level_too() {
    let src = "let a = 1\nlet b = a + 1\nroute GET / = b\n";
    let dir = program("top", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[("main.qwe", vec![json!({ "line": 2 })])], json!({}));
    assert_eq!(d.stopped()["reason"], "breakpoint");
    assert_eq!(d.stack(), vec![("<main>".to_string(), 2)]);
    let frames = d.call("stackTrace", json!({ "threadId": 1 }));
    assert_eq!(frames["stackFrames"][0]["source"]["name"], "main.qwe");
    assert_eq!(frames["totalFrames"], 1);
    d.call("continue", json!({ "threadId": 1 }));
    assert!(get(port, "/").join().unwrap().contains("200 OK"));
}

#[test]
fn an_error_at_the_top_level_stops_there() {
    let src = "let a = 1\nlet b = a + json.parse(\"{\")\nroute GET / = b\n";
    let dir = program("top-error", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[], json!({}));
    assert_eq!(d.stopped()["reason"], "exception");
    assert_eq!(d.stack(), vec![("<main>".to_string(), 2)]);
    assert_eq!(d.call("exceptionInfo", json!({ "threadId": 1 }))["description"], "cannot add int and error");
}

#[test]
fn launched_without_a_folder_it_runs_where_it_is() {
    let src = "route GET / = 1\n";
    let dir = program("no-cwd", &[("main.qwe", src)]);
    let port = free_port();
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo" }));
    d.call("launch", json!({ "program": dir.join("main.qwe"), "args": ["--port", port.to_string()] }));
    d.call("configurationDone", json!({}));
    assert!(get(port, "/").join().unwrap().contains("200 OK"));
}

#[test]
fn a_function_made_in_the_console_has_a_frame_with_no_file() {
    let src = "let g = (x) => x\nfn f() {\n    let a = 1\n    g(a)\n}\nroute GET / = f()\n";
    let dir = program("console-fn", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[("main.qwe", vec![json!({ "line": 3 })])], json!({}));
    let page = get(port, "/");
    d.stopped();
    let f = frame(&mut d);
    d.call("evaluate", json!({ "expression": "g = (x) => {\n  let y = x + 1\n  y\n}", "frameId": f, "context": "repl" }));
    d.call("next", json!({ "threadId": 1 }));
    d.stopped();
    d.call("stepIn", json!({ "threadId": 1 }));
    d.stopped();
    let frames = d.call("stackTrace", json!({ "threadId": 1 }));
    assert_eq!(frames["stackFrames"][0]["name"], "<fn>", "{}", frames);
    assert_eq!(frames["stackFrames"][0].get("source"), None, "{}", frames);
    assert_eq!(frames["stackFrames"][1]["name"], "f", "{}", frames);
    d.call("continue", json!({ "threadId": 1 }));
    assert!(page.join().unwrap().contains("\r\n\r\n2"));
}

#[test]
fn hovering_shows_variables_and_runs_nothing() {
    let src = "fn f(m, xs) {\n    let n = 1\n    n\n}\nroute GET / = f({ a: { b: 2 } }, [5])\n";
    let dir = program("hover", &[("main.qwe", src)]);
    let port = free_port();
    let (mut d, _) = launch(&dir, port, &[("main.qwe", vec![json!({ "line": 3 })])], json!({}));
    let page = get(port, "/");
    d.stopped();
    let f = frame(&mut d);
    let hover = |code: &str| json!({ "expression": code, "frameId": f, "context": "hover" });
    assert_eq!(d.call("evaluate", hover("n"))["result"], "1");
    assert_eq!(d.call("evaluate", hover("m.a.b"))["result"], "2");
    assert_eq!(d.call("evaluate", hover("xs[0]"))["result"], "5");
    assert_eq!(d.call("evaluate", hover("\"text\""))["result"], "\"text\"");
    assert_eq!(d.refused("evaluate", hover("print(1)")), "not shown on hover");
    assert_eq!(d.refused("evaluate", hover("n + 1")), "not shown on hover");
    assert_eq!(d.refused("evaluate", hover("(")), "not shown on hover");
    assert_eq!(d.refused("evaluate", hover("xs[len(xs)]")), "not shown on hover");
    // Asked outright, an error is its message, and what was run stays quiet.
    let bad = d.refused("evaluate", json!({ "expression": "nope", "frameId": f, "context": "repl" }));
    assert!(bad.contains("nope"), "{}", bad);
    assert_eq!(d.refused("evaluate", json!({ "expression": "reply 404", "frameId": f, "context": "repl" })), "`reply` cannot be used here");
    assert_eq!(d.refused("evaluate", json!({ "expression": "let a = nope\na", "frameId": f, "context": "repl" })).contains("nope"), true);
    d.call("continue", json!({ "threadId": 1 }));
    page.join().unwrap();
}

#[test]
fn breakpoints_land_on_lines_with_something_to_stop_at() {
    let src = r#"type Point = { x: int }

service billing = "billing.qwe" {
    timeout: {
        print(0)
    },
}

mount /sub = "sub.qwe"

fn forms(xs, el) {
    if len(xs) > 0 {
        print(1)
    } else {
        print(2)
    }
    if len(xs) > 1 { print(3) }
    let m = match xs {
        [a] if a > 0 =>
            a + 1
        _ =>
            0
    }
    let f = () => {
        print(4)
    }
    let table = {
        a: print(5),
        b: print(6),
    }
    el.value =
        print(7)
    {
        print(8)
    }
    reply 404
}

fn sends() {
    reply 200 print(9)
}

view Tag(on) = <div #{print(10)} class={
    print(11)}>
    <b>{print(12)}</b>
    {print(13)}
</div>

route GET /r = reply 500
"#;
    let dir = program("places", &[("main.qwe", src), ("billing.qwe", "server fn f() = 1\n"), ("sub.qwe", "route GET /x = 1\n")]);
    let mut d = Dap::start();
    d.call("initialize", json!({ "adapterID": "quillo" }));
    let lines = src.lines().count() as u64;
    let bps: Vec<Value> = (1..=lines).map(|l| json!({ "line": l })).collect();
    let r = d.call("setBreakpoints", json!({ "source": { "path": dir.join("main.qwe") }, "breakpoints": bps }));
    let placed: Vec<Option<u64>> = r["breakpoints"].as_array().unwrap().iter().map(|b| if b["verified"] == true { b["line"].as_u64() } else { None }).collect();
    let at = |l: usize| placed[l - 1];
    let want = |l: usize| -> Option<u64> {
        match l {
            // A type, a mount, what a block or a statement is not on.
            1 | 2 | 6..=10 | 37 | 38 | 41 | 42 | 44..=48 => None,
            // A service's options are code when they hold statements.
            3..=5 => Some(5),
            11 | 12 => Some(12),
            13 => Some(13),
            14 | 15 => Some(15),
            16 | 17 => Some(17),
            18 => Some(18),
            // An arm's value on its own line is no statement: the next is.
            19..=24 => Some(24),
            25 => Some(25),
            26 | 27 => Some(27),
            28..=31 => Some(31),
            32..=34 => Some(34),
            35 | 36 => Some(36),
            39 | 40 => Some(40),
            43 => Some(43),
            49 => Some(49),
            _ => panic!("line {}", l),
        }
    };
    for l in 1..=placed.len() {
        assert_eq!(at(l), want(l), "line {}", l);
    }
    // A file that does not parse, or is not there, has no stops.
    std::fs::write(dir.join("bad.qwe"), "fn (").unwrap();
    let r = d.call("setBreakpoints", json!({ "source": { "path": dir.join("bad.qwe") }, "breakpoints": [{ "line": 1 }] }));
    assert_eq!(r["breakpoints"][0]["verified"], false);
    assert_eq!(r["breakpoints"][0]["message"], "no code to stop at here");
    let r = d.call("setBreakpoints", json!({ "source": { "path": "/nonexistent/x.qwe" }, "breakpoints": [{ "line": 1 }] }));
    assert_eq!(r["breakpoints"][0]["verified"], false);
}
