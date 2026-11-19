//! FEEDBACK75 (the security audit of 0.8.7): the parts no slice held, done after the slices. The
//! MCP door over HTTP read a message with `value::json_value`, with no budget: what a signed-in
//! visitor sends is now read as a page's JSON is (Q-1052's MCP part). In-process requests are
//! measured by what this thread allocates (`Counting`).

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

// ---- what a thread allocates -------------------------------------------------------------

/// The system's allocator, counting what each thread has allocated and not freed, and the most
/// it has had at once, so a test can say how much one request made, whatever other tests do.
struct Counting;

thread_local! {
    static NOW: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn count(d: isize) {
    let _ = NOW.try_with(|n| {
        let v = n.get() + d;
        n.set(v);
        let _ = PEAK.try_with(|p| {
            if v > p.get() {
                p.set(v)
            }
        });
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(l.size() as isize);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count(l.size() as isize);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count(-(l.size() as isize));
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // Both blocks are held while one is copied to the other.
        count(new as isize);
        let q = unsafe { System.realloc(p, l, new) };
        count(-(l.size() as isize));
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// `f`, and the most bytes this thread held at once while it ran, past what it held before.
fn peak_of<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let start = NOW.with(|n| n.get());
    PEAK.with(|p| p.set(start));
    let r = f();
    (r, (PEAK.with(|p| p.get()) - start).max(0) as usize)
}

/// In-process tests one at a time: some set `--max-memory`, which is the process's.
fn serial() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `--max-memory` for the process while the value lives; none again after.
struct Limit;

impl Limit {
    fn set(bytes: u64) -> Limit {
        quillo::httpd::limit_memory(bytes);
        Limit
    }
}

impl Drop for Limit {
    fn drop(&mut self) {
        quillo::httpd::limit_memory(0);
    }
}

const MB: usize = 1 << 20;

// ---- the MCP door in-process ---------------------------------------------------------------

/// A program with a tool, loaded from its own folder, its door open as `quillo run` opens it.
struct Door {
    it: Interp,
    dir: PathBuf,
    cookie: String,
}

impl Drop for Door {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const TOOLS: &str = "route POST /login ? { user: str } = set_session({ user: user })\n\n/// How many it is given.\nserver fn mcp tally(xs: list) -> int with time {\n    return len(xs)\n}\n";

fn send(it: &mut Interp, url: &str, headers: &[(&str, &str)], body: Vec<u8>) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "POST".into(), url: url.into(), headers: h, body, remote: "127.0.0.1".into() })
}

fn door(name: &str, src: &str) -> Door {
    let dir = std::env::temp_dir().join(format!("quillo-f75f-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("app.qwe");
    std::fs::write(&file, src).unwrap();
    let mut it = Interp::new(dir.to_str().unwrap());
    it.load_file(&file).unwrap_or_else(|e| panic!("{}", e));
    quillo::mcp::open_door(&mut it, &file, None);
    let r = send(&mut it, "/login?user=ana", &[], Vec::new());
    assert_eq!(r.status, 204, "{}", r.text());
    let set = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.clone()).expect("a session cookie");
    let cookie = set.split(';').next().unwrap().to_string();
    Door { it, dir, cookie }
}

/// A `tools/call` of `tool` with `args` (a JSON object's text), as MCP 2026-07-28 sends one.
fn tool_call(tool: &str, args: &str) -> Vec<u8> {
    let head = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{{}}}},"name":"{}","arguments":"#, tool);
    let mut b = Vec::with_capacity(head.len() + args.len() + 2);
    b.extend_from_slice(head.as_bytes());
    b.extend_from_slice(args.as_bytes());
    b.extend_from_slice(b"}}");
    b
}

/// A `tools/call` of `tally` on `xs` (JSON text).
fn tally_call(xs: &str) -> Vec<u8> {
    tool_call("tally", &format!("{{\"xs\":{}}}", xs))
}

fn zeros(n: usize) -> String {
    let mut s = String::with_capacity(2 * n + 2);
    s.push('[');
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('0');
    }
    s.push(']');
    s
}

fn call(d: &mut Door, body: Vec<u8>) -> (u16, serde_json::Value) {
    call_of(d, "tally", body)
}

fn call_of(d: &mut Door, tool: &str, body: Vec<u8>) -> (u16, serde_json::Value) {
    let cookie = d.cookie.clone();
    let headers = [("Content-Type", "application/json"), ("Cookie", cookie.as_str()), ("MCP-Protocol-Version", "2026-07-28"), ("Mcp-Method", "tools/call"), ("Mcp-Name", tool)];
    let r = send(&mut d.it, "/_quillo/mcp", &headers, body);
    let text = r.text();
    (r.status, serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({}): {} {}", e, r.status, text)))
}

/// What a tool's call answered: its result, or the error's message.
fn said(j: &serde_json::Value) -> String {
    if let Some(m) = j["error"]["message"].as_str() {
        return m.to_string();
    }
    j["result"]["structuredContent"]["result"].to_string()
}

/// A message to the HTTP door is read as a page's JSON: under `--max-memory 8MB`, 1 MB of zeros
/// stops at the limit with a parse error that says so, having made little, where `json_value`
/// built serde's whole tree whatever the limit (about 75 MB for 1 MB); with no limit it is read
/// as before, and small messages, under 1 KiB, go the plain way they went (FEEDBACK75.md,
/// Q-1052's MCP part; branch feedback75)
#[test]
fn the_http_doors_messages_are_read_within_the_memory_a_visitor_has() {
    let _one = serial();
    let mut d = door("budget", TOOLS);
    // A small call, as before.
    let (status, j) = call(&mut d, tally_call("[1,2,3]"));
    assert_eq!((status, said(&j)), (200, "3".to_string()), "{}", j);
    // 1 MB with no limit: read, as 0.8.8 read it.
    let (status, j) = call(&mut d, tally_call(&zeros(500_000)));
    assert_eq!((status, said(&j)), (200, "500000".to_string()), "{}", j);
    {
        let _limit = Limit::set(8 << 20);
        let ((status, j), peak) = peak_of(|| call(&mut d, tally_call(&zeros(500_000))));
        assert_eq!(status, 400, "{}", j);
        let m = said(&j);
        assert!(m.starts_with("Parse error: ") && m.contains("--max-memory"), "{}", m);
        assert!(peak < 16 * MB, "under 8 MB, 1 MB of zeros made {} MB", peak / MB);
        // Within the limit, read.
        let (status, j) = call(&mut d, tally_call(&zeros(10_000)));
        assert_eq!((status, said(&j)), (200, "10000".to_string()), "{}", j);
        // The edge: 1 KiB and one byte past it, both read (the second by the budgeted reader).
        let pad = |n: usize| tally_call(&format!("[\"{}\"]", "x".repeat(n)));
        let base = pad(0).len();
        for total in [1024, 1025] {
            let (status, j) = call(&mut d, pad(total - base));
            assert_eq!((status, said(&j)), (200, "1".to_string()), "{} bytes: {}", total, j);
        }
        // Bad JSON past 1 KiB is still the parse error it was.
        let mut bad = tally_call(&zeros(1000));
        bad.truncate(bad.len() - 2);
        let (status, j) = call(&mut d, bad);
        assert_eq!((status, said(&j).starts_with("Parse error: ")), (400, true), "{}", j);
    }
    // Back as it was with no limit.
    let (status, j) = call(&mut d, tally_call(&zeros(200_000)));
    assert_eq!((status, said(&j)), (200, "200000".to_string()), "{}", j);
}

/// A tool's arguments the door made are not checked a second time by the program's walk, 0.8.8's,
/// exponential on a deep valid union, and nothing else is left unchecked by it: a valid `Tt` 22
/// and 62 levels deep answers at once (seconds and more over the door, 2^k), one wrong deep down
/// is refused; a default is the program's own value, checked as before; a function the tool calls
/// checks what it is given; the tool called from the program's own code checks its parameter,
/// after a call through the door too (FEEDBACK75.md, Q-1080's MCP part; branch feedback75)
#[test]
fn a_tools_made_arguments_are_checked_once_and_the_rest_as_before() {
    let _one = serial();
    let src = r#"type Tt = Aa | Bb
type Aa = { k: list<Tt> = [], a: int }
type Bb = { k: list<Tt> = [], b: int }
route POST /login ? { user: str } = set_session({ user: user })

/// One for any tree.
server fn mcp tt(t: Tt) -> int with time {
    return 1
}

fn inner(a: Aa) -> int {
    return a.a
}

/// What its `Aa` holds.
server fn mcp via(t: Tt) -> int with time {
    return inner(t)
}

/// n, with a default that is no `Tt`.
server fn mcp dflt(n: int, t: Tt = json.parse("{\"c\":1}")) -> int with time {
    return n
}

route POST /direct = str(tt(json.parse("{\"c\":1}")))
route POST /fine = str(tt(json.parse("{\"k\":[],\"b\":1}")))
"#;
    let mut d = door("made-once", src);
    let nested = |k: usize, leaf: &str| format!("{}{}{}", "{\"k\":[".repeat(k), leaf, "],\"b\":1}".repeat(k));
    let result = |j: &serde_json::Value| j["result"]["structuredContent"]["result"].clone();
    let refused = |j: &serde_json::Value| j["result"]["isError"] == serde_json::json!(true) || j.get("error").is_some();
    // 62 levels is the most a message takes, its own three levels around them (128 in all).
    for k in [0, 1, 22, 62] {
        let t = std::time::Instant::now();
        let (status, j) = call_of(&mut d, "tt", tool_call("tt", &format!("{{\"t\":{}}}", nested(k, r#"{"b":1}"#))));
        assert_eq!((status, result(&j)), (200, serde_json::json!(1)), "k={}: {}", k, j);
        // Well under a millisecond in a release build, where the second check alone took 0.6 s
        // at 63 levels by RPC.
        assert!(t.elapsed() < std::time::Duration::from_millis(250), "k={}: {:?}", k, t.elapsed());
        let (_, j) = call_of(&mut d, "tt", tool_call("tt", &format!("{{\"t\":{}}}", nested(k, r#"{"c":1}"#))));
        assert!(refused(&j), "k={}: {}", k, j.to_string().chars().take(300).collect::<String>());
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "k={}: {:?}", k, t.elapsed());
    }
    // The function it calls checks what it is given: a `Bb` is not an `Aa`.
    let (_, j) = call_of(&mut d, "via", tool_call("via", r#"{"t":{"a":5}}"#));
    assert_eq!(result(&j), serde_json::json!(5), "{}", j);
    let (_, j) = call_of(&mut d, "via", tool_call("via", r#"{"t":{"b":5}}"#));
    assert!(refused(&j), "{}", j);
    // A default is the program's own value, checked as before; given, the argument is made.
    let (_, j) = call_of(&mut d, "dflt", tool_call("dflt", r#"{"n":1}"#));
    assert!(refused(&j), "{}", j);
    let (_, j) = call_of(&mut d, "dflt", tool_call("dflt", r#"{"n":1,"t":{"b":1}}"#));
    assert_eq!(result(&j), serde_json::json!(1), "{}", j);
    // Called from the program's code, before and after a call through the door: its parameter
    // is checked.
    for _ in 0..2 {
        assert_eq!(send(&mut d.it, "/direct", &[], Vec::new()).status, 500);
        let fine = send(&mut d.it, "/fine", &[], Vec::new());
        assert_eq!((fine.status, fine.text()), (200, "1".to_string()));
        let (_, j) = call_of(&mut d, "tt", tool_call("tt", r#"{"t":{"b":1}}"#));
        assert_eq!(result(&j), serde_json::json!(1), "{}", j);
    }
}
