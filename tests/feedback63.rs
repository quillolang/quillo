// FEEDBACK63.md, Q-927: a `server fn mcp` that reads `request` works over `quillo mcp` as it
// does over HTTP: `request.mcp` is who the client says it is, and a stage that looks for a
// header finds none and answers as the program chose (branch cache-and-migrate).

mod support;

use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TOOLS: &str = r#"/// Who the client says it is.
server fn mcp who() -> str {
    return str(request.mcp)
}
fn guard(r) {
    return r
}
/// The guide's stage.
server fn mcp staged() -> str {
    return request | guard | (_) => "ok"
}
fn signed(r) {
    if get(r.headers, "authorization") == null { reply 401 "sign in first" }
    return r
}
/// Refused without a header, as the program says.
server fn mcp guarded() -> str {
    return request | signed | (_) => "in"
}
/// What else the request holds there.
server fn mcp shape() -> str {
    return `${request.method} ${len(request.headers)} ${len(request.cookies)} ${len(request.query)} ${request.json == null}`
}
"#;

fn line(id: i64, name: &str, meta: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": {}, "_meta": meta } }).to_string()
}

/// `quillo mcp` given `lines` on stdin: each answer, by id.
fn over_stdio(d: &Path, lines: &[String]) -> (Vec<Value>, String) {
    let input = d.join("in.jsonl");
    std::fs::write(&input, lines.join("\n") + "\n").unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("mcp")
        .arg(d.join("main.qwe"))
        .current_dir(d)
        .stdin(std::fs::File::open(&input).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    while g.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "quillo mcp did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
    use std::io::Read;
    let (mut out, mut err) = (String::new(), String::new());
    g.0.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    g.0.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    (out.lines().map(|l| serde_json::from_str(l).unwrap()).collect(), err)
}

/// Whether the call failed, and what it said: a tool's answer is `{"result": ...}` as text.
fn text(a: &Value) -> (bool, String) {
    let r = &a["result"];
    let t = r["content"][0]["text"].as_str().unwrap_or("").to_string();
    let said = serde_json::from_str::<Value>(&t).ok().and_then(|v| v["result"].as_str().map(str::to_string)).unwrap_or(t);
    (r["isError"].as_bool().unwrap_or(false), said)
}

#[test]
fn a_tool_reading_request_answers_on_stdio() {
    let d = support::temp_dir("f63-stdio");
    std::fs::write(d.join("main.qwe"), TOOLS).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {}, "io.modelcontextprotocol/clientInfo": { "name": "probe", "version": "1" } });
    // No client named: its fields null, not an error.
    let bare = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} });
    let (answers, err) = over_stdio(&d, &[line(1, "who", meta.clone()), line(2, "staged", meta.clone()), line(3, "guarded", meta.clone()), line(4, "shape", meta.clone()), line(5, "who", bare)]);
    assert_eq!(answers.len(), 5, "{}", err);
    assert_eq!(text(&answers[0]), (false, "{ client: \"probe\", version: \"1\" }".into()), "{}", err);
    assert_eq!(text(&answers[1]), (false, "ok".into()), "{}", err);
    // The stage's own refusal reaches the client as its text, not `internal error`.
    let (refused, said) = text(&answers[2]);
    assert!(refused && said.contains("sign in first"), "{}: {}", said, err);
    assert_eq!(text(&answers[3]), (false, "POST 0 0 0 true".into()), "{}", err);
    assert_eq!(text(&answers[4]), (false, "{ client: null, version: null }".into()), "{}", err);
    assert!(!err.contains("unknown name"), "{}", err);
    let _ = std::fs::remove_dir_all(&d);
}

// Over HTTP, as before: `request.mcp` from the call, and the request's own headers.
#[test]
fn over_http_the_request_is_the_calls_own() {
    let d = support::temp_dir("f63-http");
    std::fs::write(d.join("main.qwe"), format!("{}route POST /login {{ set_session({{ user: \"ana\" }}); \"ok\" }}\n", TOOLS)).unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let login = support::send(s.port, "POST", "/login", &[("sec-fetch-site", "same-origin")], "");
    let cookie = login.headers.iter().find(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).unwrap();
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {}, "io.modelcontextprotocol/clientInfo": { "name": "probe", "version": "1" } });
    let call = |name: &str| {
        let body = line(1, name, meta.clone());
        let a = support::send(
            s.port,
            "POST",
            "/_quillo/mcp",
            &[("content-type", "application/json"), ("cookie", &cookie), ("mcp-protocol-version", "2026-07-28"), ("mcp-method", "tools/call"), ("mcp-name", name)],
            &body,
        );
        assert_eq!(a.status, 200, "{}", a.body);
        text(&serde_json::from_str(&a.body).unwrap())
    };
    assert_eq!(call("who"), (false, "{ client: \"probe\", version: \"1\" }".into()));
    assert_eq!(call("staged"), (false, "ok".into()));
    let _ = std::fs::remove_dir_all(&d);
}
