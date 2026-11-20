//! FEEDBACK75 (the security audit of 0.8.7), slice B, wave 1: JSON and pages that made memory or
//! CPU without bound. A server fn's arguments and every reader of `value::json_value` with no
//! budget (Q-1052), a visitor's JSON whose lists and maps were barely counted (Q-1070), live runs
//! kept by their arguments' bytes (Q-1057), a page's `data-q-` ids looked for in quadratic time
//! (Q-1054) and the markdown door's answer copied three times (Q-1069). In-process requests are
//! measured by what this thread allocates (`Counting`); servers are this test's own, on the port
//! the system gives, stopped by the test.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::{Interp, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const DEBUG_SLOWER: f64 = if cfg!(debug_assertions) { 10.0 } else { 1.0 };

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

// ---- programs in-process ------------------------------------------------------------------

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send_bytes(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: Vec<u8>) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body, remote: "127.0.0.1".into() })
}

fn rpc(it: &mut Interp, f: &str, body: &str) -> (u16, String) {
    let r = send_bytes(it, "POST", &format!("/_quillo/rpc/{}", f), &[("Content-Type", "application/json")], body.as_bytes().to_vec());
    (r.status, r.text())
}

/// `[[0,0,...]]`: one argument, a list of `n` zeros, two bytes a value.
fn zeros_arg(n: usize) -> String {
    let mut s = String::with_capacity(2 * n + 4);
    s.push_str("[[");
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('0');
    }
    s.push_str("]]");
    s
}

/// `[x,x,...]` of `n` copies of `x`.
fn list_of(x: &str, n: usize) -> String {
    let mut s = String::with_capacity((x.len() + 1) * n + 2);
    s.push('[');
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(x);
    }
    s.push(']');
    s
}

// ---- servers ------------------------------------------------------------------------------

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback75b-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// Removes a test's folder when the test ends, passed or not.
struct Gone(PathBuf);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `quillo run` of `file` as a server with `args`, its log in `log`, on the port the system
/// picks, once it says where it listens (20 s at most).
fn serve(d: &Path, file: &str, args: &[&str], env: &[(&str, &str)], log: &str) -> (support::Guard, u16) {
    let out = std::fs::File::create(d.join(log)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", file, "--port", "0"])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "a secret for the feedback75b tests, long enough")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_log(&mut g.0, &d.join(log), Duration::from_secs(20));
    (g, port)
}

/// `quillo check` of a program this test runs: every snippet passes it.
fn checked(d: &Path, file: &str) {
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", file]).current_dir(d).output().unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// One request of raw bytes, its status and body.
fn raw(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let mut head = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", method, path, body.len());
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, body.to_string())
}

/// A process's resident memory, in KB, as `ps` says it.
fn rss_kb(pid: u32) -> u64 {
    let out = Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0)
}

/// The most resident memory a process had while `f` ran, past what it had before, in KB: `ps`
/// asked every 5 ms.
fn rss_growth<R>(pid: u32, f: impl FnOnce() -> R) -> (R, u64) {
    let before = rss_kb(pid);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let most = Arc::new(std::sync::atomic::AtomicU64::new(before));
    let watch = {
        let (stop, most) = (stop.clone(), most.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                most.fetch_max(rss_kb(pid), std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    };
    let r = f();
    std::thread::sleep(Duration::from_millis(50));
    most.fetch_max(rss_kb(pid), std::sync::atomic::Ordering::Relaxed);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    watch.join().unwrap();
    (r, most.load(std::sync::atomic::Ordering::Relaxed).saturating_sub(before))
}

// ---- Q-1052: a server fn's arguments, and json_value ---------------------------------------

const ADD: &str = "server fn add(a: int, b: int) -> int {\n    return a + b\n}\nserver fn tally(xs: list) -> int {\n    return len(xs)\n}\nserver fn echo(x) -> str {\n    return `${type_of(x)} ${x}`\n}\nserver fn file_size(f: map) -> int {\n    return f.size\n}\n";

const STOP_8MB: &str = r#"{"error":"this request's value would be bigger than --max-memory allows (8388608 bytes)"}"#;

/// A server fn's call of 1 MB of zeros, `[[0,0,...]]`, is read into values with a budget and makes
/// what they take, not serde's tree and a copy of it (90 MB on 0.8.7); 4 MB, past the budget, is
/// refused as bad JSON having made no more than the budget's room, where it made 230 MB; under
/// `--max-memory 8MB` the values are counted at what they take and stop at the limit
/// (FEEDBACK75.md, Q-1052; branch feedback75)
#[test]
fn a_server_fns_arguments_are_read_with_a_budget() {
    let _one = serial();
    let mut it = app(ADD);
    let (r, peak) = peak_of(|| rpc(&mut it, "add", &zeros_arg(500_000)));
    assert_eq!(r, (400, r#"{"error":"missing argument `b` for `add`"}"#.to_string()));
    assert!(peak < 48 * MB, "1 MB of zeros made {} MB", peak / MB);
    let (r, peak) = peak_of(|| rpc(&mut it, "add", &zeros_arg(2_000_000)));
    assert_eq!(r, (400, r#"{"error":"the body must be a JSON array of arguments"}"#.to_string()));
    assert!(peak < 64 * MB, "4 MB of zeros made {} MB", peak / MB);
    // The budget's edge, a request's body's in 0.8.8: a million values (the list in the
    // arguments is one), and one past it.
    assert_eq!(rpc(&mut it, "tally", &zeros_arg(999_999)), (200, r#"{"ok":999999}"#.to_string()));
    assert_eq!(rpc(&mut it, "tally", &zeros_arg(1_000_000)).0, 400);
    {
        let _limit = Limit::set(8 << 20);
        let (r, peak) = peak_of(|| rpc(&mut it, "add", &zeros_arg(500_000)));
        assert_eq!(r, (500, STOP_8MB.to_string()));
        assert!(peak < 16 * MB, "under 8 MB, 1 MB of zeros made {} MB", peak / MB);
        assert_eq!(rpc(&mut it, "tally", &zeros_arg(100_000)), (200, r#"{"ok":100000}"#.to_string()));
        assert_eq!(rpc(&mut it, "add", "[1, 2]"), (200, r#"{"ok":3}"#.to_string()));
    }
    // Back as it was with no limit.
    assert_eq!(rpc(&mut it, "tally", &zeros_arg(500_000)), (200, r#"{"ok":500000}"#.to_string()));
}

/// Under `--max-memory`, what a call's marks make is counted with what its JSON made: 140,000
/// `{"$":["k",0]}` (3.8 KB of gzip) made 152 MB and a 200 under 64 MB; now the request's stop
/// within the limit's room; 10,000 are read (FEEDBACK75.md, Q-1052's review; branch feedback75)
#[test]
fn a_calls_marks_are_held_with_its_json() {
    let _one = serial();
    let mut it = app(ADD);
    let _limit = Limit::set(64 << 20);
    let marks = |n: usize| format!("[{}]", list_of(r#"{"$":["k",0]}"#, n));
    let (r, peak) = peak_of(|| rpc(&mut it, "tally", &marks(140_000)));
    assert_eq!(r, (500, r#"{"error":"this request's value would be bigger than --max-memory allows (67108864 bytes)"}"#.to_string()));
    assert!(peak < 80 * MB, "{} MB", peak / MB);
    assert_eq!(rpc(&mut it, "tally", &marks(10_000)), (200, r#"{"ok":10000}"#.to_string()));
    assert_eq!(rpc(&mut it, "echo", r#"[{"$":["$k",{"$dec":"1.5"}]}]"#), (200, r#"{"ok":"map { \"$k\": 1.5m }"}"#.to_string()));
}

/// Under `--max-memory 8MB`, what a call's marks make is held to what its JSON left of the limit:
/// error marks whose JSON takes about 85% of it are read, but the errors made of them do not fit,
/// and the call is the request's stop; as many one-key maps with no mark, and half as many marks,
/// are answered (FEEDBACK75.md, Q-1052's second review; branch feedback75)
#[test]
fn a_calls_marks_have_the_room_its_json_left() {
    let _one = serial();
    let mut it = app(ADD);
    let _limit = Limit::set(8 << 20);
    let args = |x: &str, n: usize| format!("[{}]", list_of(x, n));
    // How many error marks make about 85% of the limit as JSON.
    let made = |n: usize| Value::parse_json_body(&args(r#"{"$quillo_err":0}"#, n), true).unwrap().unwrap().1;
    let mut n = 10_000;
    n = n * (8 << 20) * 85 / 100 / made(n);
    let m = made(n);
    assert!(m < 8 << 20 && m > (8 << 20) * 70 / 100, "{} of {}", m, 8 << 20);
    assert_eq!(rpc(&mut it, "tally", &args(r#"{"$quillo_err":0}"#, n)), (500, STOP_8MB.to_string()));
    assert_eq!(rpc(&mut it, "tally", &args(r#"{"$quillo_eRR":0}"#, n)), (200, format!(r#"{{"ok":{}}}"#, n)));
    assert_eq!(rpc(&mut it, "tally", &args(r#"{"$quillo_err":0}"#, n / 2)), (200, format!(r#"{{"ok":{}}}"#, n / 2)));
}

/// What a page sends a server fn reads as it did through serde's tree: plain values, unicode,
/// empty and one-item lists and maps, a decimal, an int past 2^53, an error value and a map of one
/// `$` key as the page marks them, and the marks inside lists and maps; what is no array of
/// arguments, nests past 128, has a byte-order mark or is not UTF-8 is refused as before; every
/// answer here is 0.8.8's (FEEDBACK75.md, Q-1052; branch feedback75)
#[test]
fn a_server_fns_arguments_read_as_before() {
    let _one = serial();
    let mut it = app(ADD);
    let ok = |s: &str| (200, format!(r#"{{"ok":{}}}"#, serde_json::Value::String(s.to_string())));
    assert_eq!(rpc(&mut it, "add", "[1, 2]"), (200, r#"{"ok":3}"#.to_string()));
    assert_eq!(rpc(&mut it, "add", " [ 40 ,\n 2 ] "), (200, r#"{"ok":42}"#.to_string()));
    assert_eq!(rpc(&mut it, "echo", r#"["ünï 😀"]"#), ok("str ünï 😀"));
    assert_eq!(rpc(&mut it, "echo", "[[]]"), ok("list []"));
    assert_eq!(rpc(&mut it, "echo", "[{}]"), ok("map {}"));
    assert_eq!(rpc(&mut it, "echo", "[[7]]"), ok("list [7]"));
    assert_eq!(rpc(&mut it, "echo", r#"[{"a": 1}]"#), ok("map { a: 1 }"));
    assert_eq!(rpc(&mut it, "echo", "[1.5]"), ok("float 1.5"));
    assert_eq!(rpc(&mut it, "echo", "[null]"), ok("null null"));
    assert_eq!(rpc(&mut it, "echo", r#"[{"$dec": "12.50"}]"#), ok("dec 12.50"));
    assert_eq!(rpc(&mut it, "echo", r#"[{"$int": "9007199254740993"}]"#), ok("int 9007199254740993"));
    assert_eq!(rpc(&mut it, "echo", r#"[{"$": ["$dec", 1]}]"#), ok("map { \"$dec\": 1 }"));
    assert_eq!(rpc(&mut it, "echo", r#"[[{"$dec": "1.5"}, {"k": {"$int": "12"}}]]"#), ok("list [1.5m, { k: 12 }]"));
    // A mark with a value of another kind is a map, as it was.
    assert_eq!(rpc(&mut it, "echo", r#"[{"$dec": 5}]"#), ok("map { \"$dec\": 5 }"));
    // No file is anything without files.
    assert_eq!(rpc(&mut it, "echo", r#"[{"$quillo_file": 0}]"#), ok("null null"));
    // An error value, as the page marks it (what of it a visitor is told is the server fn's
    // answer's business, as before).
    let (status, body) = rpc(&mut it, "echo", r#"[{"$quillo_err": "no"}]"#);
    assert!(status == 200 && body.starts_with(r#"{"ok":"error err("#), "{}", body);
    assert_eq!(rpc(&mut it, "echo", r#"[{"k":1,"k":2}]"#), ok("map { k: 2 }"));
    assert_eq!(rpc(&mut it, "echo", "[18446744073709551615]"), ok("float 18446744073709552000"));
    for bad in ["{}", "3", "", "[1, 2", "\u{FEFF}[1, 2]"] {
        assert_eq!(rpc(&mut it, "add", bad), (400, r#"{"error":"the body must be a JSON array of arguments"}"#.to_string()), "{:?}", bad);
    }
    let r = send_bytes(&mut it, "POST", "/_quillo/rpc/add", &[("Content-Type", "application/json")], b"[1, \"\xff\"]".to_vec());
    assert_eq!((r.status, r.text()), (400, r#"{"error":"the body must be a JSON array of arguments"}"#.to_string()));
    let deep = |n: usize| format!("[{}0{}]", "[".repeat(n), "]".repeat(n));
    assert_eq!(rpc(&mut it, "echo", &deep(127)).0, 200);
    assert_eq!(rpc(&mut it, "echo", &deep(128)), (400, r#"{"error":"the arguments' JSON it nests more than 128 deep"}"#.to_string()));
    // Past 128 and not UTF-8: said too deep, as 0.8.8 said it.
    let mut bad = deep(128).into_bytes();
    bad.insert(1, 0xff);
    let r = send_bytes(&mut it, "POST", "/_quillo/rpc/echo", &[("Content-Type", "application/json")], bad);
    assert_eq!((r.status, r.text()), (400, r#"{"error":"the arguments' JSON it nests more than 128 deep"}"#.to_string()));
    // Ordinary arguments 0.8.8 read with no limit are read: 2 MB of one-key maps.
    assert_eq!(rpc(&mut it, "tally", &format!("[{}]", list_of(r#"{"a":0}"#, 250_000))), (200, r#"{"ok":250000}"#.to_string()));
}

/// A call with files: its arguments are the `args` part, read with the same budget, and each
/// `{"$quillo_file": i}`, in a list or a map too, is the upload in part `file<i>`, but not inside
/// an error's mark, as 0.8.8 read it; 3 MB of zeros in the `args` part under `--max-memory 8MB` is
/// the request's stop, where it passed every limit and made 250 MB (FEEDBACK75.md, Q-1052; branch
/// feedback75)
#[test]
fn a_call_with_files_reads_its_arguments_with_the_budget() {
    let _one = serial();
    let mut it = app(ADD);
    let form = |args: &str| {
        format!(
            "--BB\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n{}\r\n--BB\r\nContent-Disposition: form-data; name=\"file0\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--BB--\r\n",
            args
        )
        .into_bytes()
    };
    let headers = [("Content-Type", "multipart/form-data; boundary=BB"), ("x-quillo-rpc", "1")];
    let mut call = |f: &str, args: &str| {
        let r = send_bytes(&mut it, "POST", &format!("/_quillo/rpc/{}", f), &headers, form(args));
        (r.status, r.text())
    };
    assert_eq!(call("file_size", r#"[{"$quillo_file": 0}]"#), (200, r#"{"ok":5}"#.to_string()));
    assert_eq!(call("tally", r#"[[{"$quillo_file": 0}, {"$quillo_file": 1}, {"$quillo_file": "x"}]]"#), (200, r#"{"ok":3}"#.to_string()));
    assert_eq!(call("echo", r#"[[{"$quillo_file": 1}]]"#), (200, r#"{"ok":"list [null]"}"#.to_string()));
    let (status, body) = call("echo", r#"[{"$quillo_err": {"$quillo_file": 0}}]"#);
    assert!(status == 200 && body.contains("$quillo_file") && !body.contains("a.txt"), "{}", body);
    let (status, body) = call("echo", r#"[{"$dec": [{"$quillo_file": 0}]}]"#);
    assert!(status == 200 && body.contains("$quillo_file") && !body.contains("a.txt"), "{}", body);
    let _limit = Limit::set(8 << 20);
    let (r, peak) = peak_of(|| call("add", &zeros_arg(1_500_000)));
    assert_eq!(r, (500, STOP_8MB.to_string()));
    assert!(peak < 32 * MB, "3 MB of zeros in the args part made {} MB", peak / MB);
}

/// `json_value`, for JSON Quillo trusts, reads as 0.8.8 did, with no budget; `json_from_page`, for
/// a page's channel messages and a join, has the budget `Value::parse_json` has: 1,100,000 zeros
/// are refused, 999,000 read, and it says so; both at most 128 deep, and the same values as
/// serde's own reader (FEEDBACK75.md, Q-1052 and its review; branch feedback75)
#[test]
fn json_value_trusts_and_json_from_page_has_a_budget() {
    let _one = serial();
    use quillo::value::{json_from_page, json_value};
    let many = list_of("0", 1_100_000);
    assert_eq!(json_value(many.as_bytes()).unwrap().as_array().unwrap().len(), 1_100_000);
    let e = json_from_page(many.as_bytes()).unwrap_err();
    assert!(e.contains("more values than JSON of its length may have"), "{}", e);
    assert_eq!(json_from_page(list_of("0", 999_000).as_bytes()).unwrap().as_array().unwrap().len(), 999_000);
    let wide = list_of("\"0123456789abcdef\"", 1_100_000);
    assert_eq!(json_from_page(wide.as_bytes()).unwrap().as_array().unwrap().len(), 1_100_000);
    let deep = |n: usize| format!("{}0{}", "[".repeat(n), "]".repeat(n));
    for read in [json_value, json_from_page] {
        assert!(read(deep(128).as_bytes()).is_ok());
        assert_eq!(read(deep(129).as_bytes()).unwrap_err(), quillo::value::json_too_deep());
        for text in [r#"{"a":[1,2.5,"ü😀",null,true,{}],"b":{"c":[]},"a2":-3,"big":18446744073709551615}"#, "[]", "{}", "[1]", "\"x\"", "0", r#"{"k":1,"k":2}"#] {
            let ours = read(text.as_bytes()).unwrap();
            let theirs: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(ours, theirs, "{}", text);
            assert_eq!(ours.to_string(), theirs.to_string(), "{}", text);
        }
        assert!(read(b"[1,").is_err());
        assert!(read(b"[1] x").is_err());
        assert!(read(b"").is_err());
    }
}

/// Under `--max-memory 1MB`, `json_from_page` counts serde's values at what they take (80 bytes a
/// value) and stops at the limit: 100,000 zeros are refused, and said so, 1,000 read;
/// `json_value` reads what Quillo trusts as 0.8.8 did, and a 4.2 MB answer under 64 MB too
/// (FEEDBACK75.md, Q-1052 and its review; branch feedback75)
#[test]
fn json_from_page_is_held_to_the_memory_limit() {
    let _one = serial();
    use quillo::value::{json_from_page, json_value};
    {
        let _limit = Limit::set(1 << 20);
        let (e, peak) = peak_of(|| json_from_page(list_of("0", 100_000).as_bytes()).unwrap_err());
        assert!(e.contains("--max-memory allows (1048576 bytes)"), "{}", e);
        assert!(peak < 4 * MB, "{} MB", peak / MB);
        assert_eq!(json_from_page(list_of("0", 1_000).as_bytes()).unwrap().as_array().unwrap().len(), 1_000);
        let long = format!("[\"{}\"]", "a".repeat(2 << 20));
        assert!(json_from_page(long.as_bytes()).unwrap_err().contains("--max-memory"));
        assert_eq!(json_value(list_of("0", 100_000).as_bytes()).unwrap().as_array().unwrap().len(), 100_000);
    }
    let _limit = Limit::set(64 << 20);
    let answer = serde_json::json!({ "ok": (0..600_000).collect::<Vec<u32>>() }).to_string();
    assert!(answer.len() > 4_000_000);
    assert_eq!(json_value(answer.as_bytes()).unwrap()["ok"].as_array().unwrap().len(), 600_000);
}

/// A split service's answer of 1,500,000 values is read as it was (0.8.8: a 200), and one of
/// 600,000 (4.2 MB) under `--max-memory 64MB`: the budget that refused them is a visitor's only
/// (FEEDBACK75.md, Q-1052's review; branch feedback75)
#[test]
fn a_split_services_answer_is_read_as_before() {
    let app_src = "service risk = \"risk.qwe\"\nroute GET /score = str(risk.score())\nroute GET /big = str(len(risk.big()))\nroute GET /mid = str(len(risk.mid()))\n";
    let risk = "server fn score() = 7\nserver fn big() = range(0, 1500000)\nserver fn mid() = range(0, 600000)\n";
    let d = dir("split", &[("app.qwe", app_src), ("risk.qwe", risk)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let secret = [("QUILLO_SERVICE_SECRET", "a service secret for the feedback75b tests, long enough")];
    for (args, path, want) in [(&["--split"][..], "/big", "1500000"), (&["--split", "--max-memory", "64MB"][..], "/mid", "600000")] {
        let (_g, port) = serve(&d, "app.qwe", args, &secret, "split.log");
        let mut a = support::send(port, "GET", "/score", &[], "");
        let end = Instant::now() + Duration::from_secs(20);
        while a.body != "7" && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(200));
            a = support::send(port, "GET", "/score", &[], "");
        }
        assert_eq!(a.body, "7", "the service did not answer: {}", std::fs::read_to_string(d.join("split.log")).unwrap_or_default());
        let a = support::send(port, "GET", path, &[], "");
        assert_eq!((a.status, a.body.as_str()), (200, want), "{:?} {}", args, path);
    }
}

/// A service's join larger than a join (4 KB) is refused before its JSON is read, its signature
/// unchecked: 1 MB of it made 100 MB and was then told it was not signed; at the limit and under
/// it, and a small one, it is the signature that refuses it, as before; under `--max-memory`, the
/// request's start does not read its body either (FEEDBACK75.md, Q-1052; branch feedback75)
#[test]
fn a_join_is_refused_by_its_size_before_it_is_read() {
    let d = dir("join", &[("app.qwe", "route GET / = \"ok\"\n")]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (g, port) = serve(&d, "app.qwe", &["--max-memory", "64MB"], &[("QUILLO_SERVICE_SECRET", "a service secret for the test")], "join.log");
    let headers = [("Content-Type", "application/json"), ("x-quillo-call", "abc123"), ("x-quillo-service", "t=1,sig=00")];
    let padded = |n: usize| {
        let head = r#"{"service":"billing","port":1,"pad":""#;
        format!("{}{}\"}}", head, "x".repeat(n - head.len() - 2))
    };
    assert_eq!(padded(4096).len(), 4096);
    let (r, grew) = rss_growth(g.0.id(), || raw(port, "POST", "/_quillo/join", &headers, format!(r#"{{"service":"billing","port":1,"pad":{}}}"#, list_of("0", 500_000)).as_bytes()));
    assert_eq!(r, (413, r#"{"error":"a join's body is at most 4096 bytes"}"#.to_string()));
    assert!(grew < 16 * 1024, "the join's 1 MB grew the server by {} KB", grew);
    assert_eq!(raw(port, "POST", "/_quillo/join", &headers, padded(4097).as_bytes()).0, 413);
    let unsigned = r#"{"error":"not signed with this server's QUILLO_SERVICE_SECRET (or too old)"}"#.to_string();
    assert_eq!(raw(port, "POST", "/_quillo/join", &headers, padded(4096).as_bytes()), (401, unsigned.clone()));
    assert_eq!(raw(port, "POST", "/_quillo/join", &headers, br#"{"service":"billing","port":1}"#), (401, unsigned));
    assert_eq!(raw(port, "POST", "/_quillo/join", &headers, b"").0, 400);
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "ok");
}

/// Only the join's own path skips the request's start's held reading of a body: a route's path
/// that ends in `/_quillo/join` (with a query or not) has its 1.1 million fields held to
/// `--max-memory 64MB`, a 500 as on 0.8.8 (where they made 251 MB and a 200); the join's own path
/// is the join's (FEEDBACK75.md, Q-1052's review; branch feedback75)
#[test]
fn only_the_joins_own_path_skips_the_held_body() {
    let d = dir("join-path", &[("app.qwe", "route GET / = \"ok\"\nroute POST /*rest = `${len(keys(request.form))} ${len(request.body)}`\n")]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (g, port) = serve(&d, "app.qwe", &["--max-memory", "64MB"], &[], "join-path.log");
    let mut body = Vec::with_capacity(10 << 20);
    for i in 0..1_100_000 {
        if i > 0 {
            body.push(b'&');
        }
        body.extend_from_slice(format!("a{}=", i).as_bytes());
    }
    let form = [("Content-Type", "application/x-www-form-urlencoded")];
    for path in ["/x/y", "/x/_quillo/join", "/x/_quillo/join?q=1"] {
        let (r, grew) = rss_growth(g.0.id(), || raw(port, "POST", path, &form, &body));
        assert_eq!(r.0, 500, "{}: {}", path, r.1);
        assert!(grew < 160 * 1024, "{}: grew the server by {} KB", path, grew);
    }
    assert_eq!(raw(port, "POST", "/_quillo/join", &form, &body).0, 403);
    assert_eq!(raw(port, "POST", "/x/y", &form, b"a=1&b=2"), (200, "2 7".to_string()));
}

// ---- Q-1070: a visitor's JSON ------------------------------------------------------------

/// With no `--max-memory`, a visitor's JSON is read as 0.8.8 read it: records, arrays of maps and
/// nested data at their usual densities (5 MB of 290,000 `{"id":N,"v":1}`, 2 MB of one-key maps,
/// a million empty maps), and refused only past the same budget of values (FEEDBACK75.md, Q-1070's
/// review; branch feedback75)
#[test]
fn with_no_limit_a_visitors_json_is_read_as_before() {
    let _one = serial();
    let body = |t: &str| Value::parse_json_body(t, false).unwrap().map(|(v, made)| (v.json_text().len(), made));
    let records: Vec<String> = (0..290_000).map(|i| format!(r#"{{"id":{},"v":1}}"#, i)).collect();
    let records = format!("[{}]", records.join(","));
    assert!(records.len() > 5_000_000);
    for t in [records.as_str(), &list_of(r#"{"a":0}"#, 250_000), &list_of("{}", 1_000_000), &list_of("[]", 1_000_000), &list_of("0", 1_000_000), &list_of("\"0123456789abcdef\"", 1_100_000)] {
        let (len, made) = body(t).unwrap_or_else(|e| panic!("{}: {}", &t[..40], e));
        assert_eq!((len, made), (Value::parse_json(t).unwrap().json_text().len(), 0), "{}", &t[..40]);
    }
    assert!(body(&list_of("0", 1_000_001)).is_err_and(|e| e.to_string().contains("more values than JSON of its length may have")));
    for t in ["[]", "{}", "[1]", r#"{"a":1}"#, r#"["ünï 😀"]"#, r#"[{"id":1,"name":"a","tags":["x","y"]}]"#] {
        assert_eq!(Value::parse_json_body(t, false).unwrap().unwrap().0.json_text(), Value::parse_json(t).unwrap().json_text(), "{}", t);
    }
}

/// Under `--max-memory`, a visitor's JSON counts each text, list and map at what it takes and stops
/// at the limit in all: under 8 MB, 30,000 empty maps are read and what they made is said, 100,000
/// are the request's stop within the limit's room, 7.4 MB of `{"a":0}` too (116 MB on 0.8.7's
/// count); a program's own JSON as before (FEEDBACK75.md, Q-1070 and its review; branch
/// feedback75)
#[test]
fn under_a_memory_limit_a_visitors_json_has_its_room() {
    let _one = serial();
    let _limit = Limit::set(8 << 20);
    let maps = list_of("{}", 30_000);
    let (r, peak) = peak_of(|| Value::parse_json_body(&maps, true));
    let (_, made) = r.unwrap().unwrap();
    assert!(made > 30_000 * 64 && made <= 8 << 20 && peak <= (8 << 20) + MB, "made {} peak {}", made, peak);
    let maps = list_of("{}", 100_000);
    let (r, peak) = peak_of(|| Value::parse_json_body(&maps, true));
    assert!(r.is_err());
    assert!(peak < 12 * MB, "{} MB", peak / MB);
    let unheld = Value::parse_json_body(&list_of("{}", 100_000), false).unwrap();
    assert!(unheld.is_err_and(|e| e.to_string().contains("--max-memory allows (8388608 bytes)")));
    let one_key = list_of(r#"{"a":0}"#, 920_000);
    let (r, peak) = peak_of(|| Value::parse_json_body(&one_key, true));
    assert!(r.is_err());
    assert!(peak < 12 * MB, "{} MB", peak / MB);
    // Long texts, and long keys, count their bytes.
    assert!(Value::parse_json_body(&list_of("\"0123456789abcdef\"", 200_000), true).is_err());
    assert!(Value::parse_json_body(&list_of("\"0123456789abcdef\"", 20_000), true).is_ok());
    assert!(Value::parse_json_body(&format!("{{\"{}\":1}}", "k".repeat(9 << 20)), true).is_err());
    // Empty, one item, unicode: as before.
    for t in ["[]", "{}", "[1]", r#"{"a":1}"#, r#"["ünï 😀"]"#] {
        assert_eq!(Value::parse_json_body(t, true).unwrap().unwrap().0.json_text(), Value::parse_json(t).unwrap().json_text(), "{}", t);
    }
    // A program's own JSON is held as before: each list compared alone.
    assert!(Value::parse_json(&list_of("{}", 100_000)).is_ok());
}

/// On every path under `--max-memory`, whose body is read at the request's start: 3 MB of empty
/// maps ends the request within the limit's room, a 500 to a 404's path and to a route that never
/// reads it, as 7.4 MB of `{"a":0}` does under 8 MB (0.8.7's count let both through: 136 MB and
/// 116 MB, and a 200); 300,000 maps are read, small JSON as before (FEEDBACK75.md, Q-1070 and its
/// review; branch feedback75)
#[test]
fn every_path_takes_a_visitors_json_with_its_room() {
    let src = "route POST /ignore = \"ignored\"\nroute POST /json = `${len(request.json ?? [])}`\nroute POST /api/items { json: { name: str, qty: int } } = `${qty} x ${name}`\n";
    let d = dir("every-path", &[("app.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let json = [("Content-Type", "application/json")];
    {
        let (g, port) = serve(&d, "app.qwe", &["--max-memory", "64MB"], &[], "every.log");
        let maps = list_of("{}", 1_000_000);
        for path in ["/ignore", "/no-such-path", "/json"] {
            let (r, grew) = rss_growth(g.0.id(), || raw(port, "POST", path, &json, maps.as_bytes()));
            assert_eq!(r.0, 500, "{}: {}", path, r.1);
            assert!(grew < 96 * 1024, "{}: 3 MB of empty maps grew the server by {} KB", path, grew);
        }
        assert_eq!(raw(port, "POST", "/json", &json, list_of("{}", 300_000).as_bytes()), (200, "300000".to_string()));
        assert_eq!(raw(port, "POST", "/api/items", &json, r#"{"name":"té","qty":3}"#.as_bytes()), (200, "3 x té".to_string()));
        assert_eq!(raw(port, "POST", "/json", &json, b"[]"), (200, "0".to_string()));
        assert_eq!(raw(port, "POST", "/json", &json, b"[1]"), (200, "1".to_string()));
    }
    let (g, port) = serve(&d, "app.qwe", &["--max-memory", "8MB"], &[], "every8.log");
    let (r, grew) = rss_growth(g.0.id(), || raw(port, "POST", "/ignore", &json, list_of(r#"{"a":0}"#, 920_000).as_bytes()));
    assert_eq!(r.0, 500, "{}", r.1);
    assert!(grew < 48 * 1024, "7.4 MB of one-key maps grew the server by {} KB", grew);
    assert_eq!(raw(port, "POST", "/json", &json, list_of(r#"{"a":0}"#, 10_000).as_bytes()), (200, "10000".to_string()));
}

/// A request with two content types, under `--max-memory` and long enough to be read at its start:
/// a server fn's call reads the first, as 0.8.8 did, where the start's reading took the last
/// (`text/plain`) and the call was refused (FEEDBACK75.md, Q-1052's review; branch feedback75)
#[test]
fn a_call_with_two_content_types_is_read_as_before() {
    let d = dir("two-types", &[("app.qwe", ADD)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = serve(&d, "app.qwe", &["--max-memory", "64MB"], &[], "types.log");
    let body = format!("[1, 2{}]", " ".repeat(2000));
    assert_eq!(raw(port, "POST", "/_quillo/rpc/add", &[("Content-Type", "application/json"), ("Content-Type", "text/plain")], body.as_bytes()), (200, r#"{"ok":3}"#.to_string()));
    assert_eq!(raw(port, "POST", "/_quillo/rpc/add", &[("Content-Type", "application/json")], body.as_bytes()), (200, r#"{"ok":3}"#.to_string()));
    let form = format!("--BB\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n{}\r\n--BB--\r\n", body);
    assert_eq!(raw(port, "POST", "/_quillo/rpc/add", &[("Content-Type", "multipart/form-data; boundary=BB"), ("Content-Type", "application/json"), ("x-quillo-rpc", "1")], form.as_bytes()), (200, r#"{"ok":3}"#.to_string()));
}

// ---- Q-1057: live runs ---------------------------------------------------------------------

/// A shared live run is kept by a hash of its key: 300 runs whose arguments are a megabyte each
/// keep none of the 300 MB they would; the same key finds its run, another's (one byte apart, an
/// empty one, a unicode host) does not (FEEDBACK75.md, Q-1057; branch feedback75)
#[test]
fn shared_live_runs_keep_no_arguments() {
    use quillo::livecache::{Answer, LiveCache};
    let mut c = LiveCache::default();
    let a = Arc::new(Answer::new(serde_json::json!(3), false));
    let start = NOW.with(|n| n.get());
    for i in 0..300u32 {
        let mut args = vec![b' '; MB];
        args[0] = b'[';
        args[1..5].copy_from_slice(&i.to_le_bytes());
        c.share((String::new(), "add".into(), args, "localhost".into()), 1, Vec::new(), a.clone());
    }
    let kept = (NOW.with(|n| n.get()) - start).max(0) as usize;
    assert!(kept < 2 * MB, "300 runs of a megabyte's arguments kept {} KB", kept >> 10);
    let key = |args: &[u8], host: &str| (String::new(), "add".to_string(), args.to_vec(), host.to_string());
    c.share(key(b"[1,2]", "h"), 5, vec!["t".into()], a.clone());
    let got = c.shared(&key(b"[1,2]", "h"), |_, _| true).expect("the same call's run");
    assert_eq!((got.1, got.2), (vec!["t".to_string()], 5));
    assert!(Arc::ptr_eq(&got.0, &a));
    assert!(c.shared(&key(b"[1,3]", "h"), |_, _| true).is_none());
    assert!(c.shared(&key(b"[1,2]", "h2"), |_, _| true).is_none());
    assert!(c.shared(&key(b"", "h"), |_, _| true).is_none());
    c.share(key(b"", "ünï"), 6, Vec::new(), a.clone());
    assert_eq!(c.shared(&key(b"", "ünï"), |_, _| true).map(|s| s.2), Some(6));
    // A run's topics changed: not given, as before.
    assert!(c.shared(&key(b"[1,2]", "h"), |_, _| false).is_none());
}

/// A run past its time is let go at the next second's first share, however few there are: under
/// 256 runs, one stayed until the same call came again (FEEDBACK75.md, Q-1057; branch
/// feedback75)
#[test]
fn a_live_run_past_its_time_is_let_go() {
    use quillo::livecache::{Answer, LiveCache, SHARED_FOR};
    let mut c = LiveCache::default();
    let old = Arc::new(Answer::new(serde_json::json!("old"), false));
    let key = |n: u8| (String::new(), "f".to_string(), vec![n], String::new());
    c.share(key(1), 1, Vec::new(), old.clone());
    assert_eq!(Arc::strong_count(&old), 2);
    std::thread::sleep(SHARED_FOR + Duration::from_millis(1100));
    c.share(key(2), 2, Vec::new(), Arc::new(Answer::new(serde_json::json!("new"), false)));
    assert_eq!(Arc::strong_count(&old), 1, "the old run is kept");
    assert!(c.shared(&key(1), |_, _| true).is_none());
    assert_eq!(c.shared(&key(2), |_, _| true).map(|s| s.2), Some(2));
}

/// A live server fn call with arguments of its own each time, 40 of 200 KB each, is answered and
/// leaves the server no larger than plain calls do; the same call is answered from its run
/// (FEEDBACK75.md, Q-1057; branch feedback75)
#[test]
fn live_calls_with_their_own_arguments_are_not_kept() {
    let d = dir("live", &[("app.qwe", ADD)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (g, port) = serve(&d, "app.qwe", &[], &[], "live.log");
    let json = [("Content-Type", "application/json"), ("x-quillo-live", "1")];
    let pid = g.0.id();
    let body = |i: usize| format!("[1,2{}]", " ".repeat(200_000 + i));
    // Warm: the allocator's first growth is not the cache's.
    for i in 0..5 {
        assert_eq!(raw(port, "POST", "/_quillo/rpc/add", &json, body(i).as_bytes()).0, 200);
    }
    let before = rss_kb(pid);
    for i in 5..45 {
        let (status, text) = raw(port, "POST", "/_quillo/rpc/add", &json, body(i).as_bytes());
        assert_eq!(status, 200);
        assert!(text.starts_with(r#"{"ok":3,"h":""#), "{}", text);
    }
    std::thread::sleep(Duration::from_millis(200));
    let grew = rss_kb(pid).saturating_sub(before);
    // Held, the 40 calls' arguments are 8 MB.
    assert!(grew < 6 * 1024, "40 live calls of 200 KB grew the server by {} KB", grew);
    let (s1, a1) = raw(port, "POST", "/_quillo/rpc/add", &json, b"[20,22]");
    let (s2, a2) = raw(port, "POST", "/_quillo/rpc/add", &json, b"[20,22]");
    assert_eq!((s1, s2), (200, 200));
    let seq = |a: &str| a.split("\"seq\":").nth(1).and_then(|s| s.split(',').next()).map(str::to_string);
    assert!(a1.starts_with(r#"{"ok":42,"#) && seq(&a1) == seq(&a2), "{} / {}", a1, a2);
}

/// The runs kept for sharing weigh at most `RUNS_BYTES` in all, each its answer's text at 41 bytes
/// a byte (its tree with it): a run of 1 MB lets the one before it go, and one of 2 MB is not kept;
/// 20 live calls answering 800 KB each kept 612 MB after they ended (FEEDBACK75.md, Q-1057's
/// review; branch feedback75)
#[test]
fn shared_live_runs_weigh_at_most_their_room() {
    use quillo::livecache::{Answer, LiveCache, RUNS_BYTES};
    assert_eq!(RUNS_BYTES, 64 << 20);
    let mut c = LiveCache::default();
    let key = |n: u8| (String::new(), "f".to_string(), vec![n], String::new());
    let big = |n: usize| Arc::new(Answer::new(serde_json::Value::String("x".repeat(n)), false));
    let first = big(MB);
    c.share(key(1), 1, Vec::new(), first.clone());
    assert_eq!(c.shared(&key(1), |_, _| true).map(|s| s.2), Some(1));
    c.share(key(2), 2, Vec::new(), big(MB));
    assert_eq!(Arc::strong_count(&first), 1, "the first run is kept past the room");
    assert!(c.shared(&key(1), |_, _| true).is_none());
    assert_eq!(c.shared(&key(2), |_, _| true).map(|s| s.2), Some(2));
    let huge = big(2 * MB);
    c.share(key(3), 3, Vec::new(), huge.clone());
    assert!(c.shared(&key(3), |_, _| true).is_none());
    assert_eq!(Arc::strong_count(&huge), 1);
    // Small runs: many at once, as before; the same key again replaces its run.
    for i in 0..200u8 {
        c.share(key(i), u64::from(i), Vec::new(), Arc::new(Answer::new(serde_json::json!(i), false)));
    }
    assert_eq!(c.shared(&key(7), |_, _| true).map(|s| s.2), Some(7));
    assert_eq!(c.shared(&key(199), |_, _| true).map(|s| s.2), Some(199));
}

/// 20 live calls of a server fn answering what it is sent, 800 KB each, leave the server no more
/// than the runs' room larger once they end, where they kept 612 MB (FEEDBACK75.md, Q-1057's
/// review; branch feedback75)
#[test]
fn live_answers_are_kept_within_their_room() {
    let d = dir("live-answers", &[("app.qwe", "server fn echo(x) = x\nroute GET / = \"ok\"\n")]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (g, port) = serve(&d, "app.qwe", &[], &[], "answers.log");
    let pid = g.0.id();
    let json = [("Content-Type", "application/json"), ("x-quillo-live", "1")];
    let zeros = list_of("0", 400_000);
    let before = rss_kb(pid);
    for i in 0..20 {
        let (status, text) = raw(port, "POST", "/_quillo/rpc/echo", &json, format!("[{}{}]", zeros, " ".repeat(i)).as_bytes());
        assert_eq!(status, 200);
        assert!(text.starts_with(r#"{"ok":[0,0,"#) || text.starts_with(r#"{"patch":"#), "{}", &text[..60.min(text.len())]);
    }
    std::thread::sleep(Duration::from_secs(4));
    let grew = rss_kb(pid).saturating_sub(before);
    assert!(grew < 200 * 1024, "20 live answers of 800 KB left the server {} KB larger", grew);
    assert_eq!(support::send(port, "GET", "/", &[], "").body, "ok");
}

// ---- Q-1054: a page's style ids ------------------------------------------------------------

/// A page's `data-q-` ids are looked through in linear time: 200,000 a visitor wrote take
/// milliseconds, none kept but the program's own (`ids_known`), each of those once, in the order
/// found; `ids_in`, which keeps them all, is linear too (FEEDBACK75.md, Q-1054; branch feedback75)
#[test]
fn a_pages_style_ids_are_found_in_linear_time() {
    use std::collections::HashSet;
    let known: HashSet<[u8; 8]> = [*b"ab12cd34", *b"00ff00ff"].into_iter().collect();
    let mut html = String::new();
    for i in 0..200_000u32 {
        html.push_str(&format!("data-q-{:08x} ", i));
    }
    html.push_str("<p data-q-ab12cd34><b data-q-00ff00ff data-q-ab12cd34 data-q-00ff00ff>");
    let t = Instant::now();
    assert_eq!(quillo::styles::ids_known(html.as_bytes(), &known), vec!["ab12cd34", "00ff00ff"]);
    assert!(t.elapsed().as_secs_f64() < 0.5 * DEBUG_SLOWER, "{:?}", t.elapsed());
    let t = Instant::now();
    let all = quillo::styles::ids_in(html.as_bytes());
    assert!(t.elapsed().as_secs_f64() < 1.0 * DEBUG_SLOWER, "{:?}", t.elapsed());
    assert_eq!(all.len(), 200_002);
    assert_eq!((all[0].as_str(), all[199_999].as_str(), all[200_000].as_str()), ("00000000", "00030d3f", "ab12cd34"));
    // None known, nothing looked for; nothing, one, cut short, not hex, upper case (not the
    // program's), next to unicode, the same one again and again.
    assert!(quillo::styles::ids_known(html.as_bytes(), &HashSet::new()).is_empty());
    assert!(quillo::styles::ids_known(b"", &known).is_empty());
    assert_eq!(quillo::styles::ids_known(b"data-q-ab12cd34", &known), vec!["ab12cd34"]);
    assert!(quillo::styles::ids_known(b"<p data-q-ab12cd", &known).is_empty());
    assert!(quillo::styles::ids_known(b"<p data-q-zb12cd34 data-q-", &known).is_empty());
    assert!(quillo::styles::ids_known(b"<p data-q-AB12CD34>", &known).is_empty());
    assert_eq!(quillo::styles::ids_known("é data-q-00ff00ffé".as_bytes(), &known), vec!["00ff00ff"]);
    assert_eq!(quillo::styles::ids_known("data-q-ab12cd34 ".repeat(1000).as_bytes(), &known), vec!["ab12cd34"]);
    assert_eq!(quillo::styles::ids_known("data-q-ab12cd34 data-q-00ff00ff ".repeat(1000).as_bytes(), &known), vec!["ab12cd34", "00ff00ff"]);
}

/// A page showing a visitor's text of 80,000 `data-q-` words is answered in milliseconds under
/// `--max-compute 2s`, where each view took 4.8 s and held the instance; a component's page still
/// links its sheet, and one with no component none (FEEDBACK75.md, Q-1054; branch feedback75)
#[test]
fn a_comment_of_style_ids_does_not_hold_its_page() {
    let src = "component Card(title: str) {\n    style css`.c { color: red }`\n    <div class=\"c\">{title}</div>\n}\nlet comments = []\nroute POST /comment {\n    comments = comments + [request.body]\n    return \"saved\"\n}\nroute GET /comments = <html><head></head><body><h1>Comments</h1>{for c in comments => <p>{c}</p>}</body></html>\nroute GET /card = <html><head></head><body><Card title=\"a\"/></body></html>\nroute GET /plain = <html><head></head><body><p>hi</p></body></html>\n";
    let d = dir("ids", &[("app.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = serve(&d, "app.qwe", &["--max-compute", "2s"], &[], "ids.log");
    let words: Vec<String> = (0..80_000).map(|i| format!("data-q-{:08x}", i)).collect();
    let (status, _) = raw(port, "POST", "/comment", &[("Content-Type", "text/plain")], words.join(" ").as_bytes());
    assert_eq!(status, 200);
    for _ in 0..2 {
        let t = Instant::now();
        let a = support::send(port, "GET", "/comments", &[], "");
        assert_eq!(a.status, 200);
        assert!(a.body.len() > 1_280_000 && a.body.contains("data-q-0001387f"), "{}", a.body.len());
        assert!(t.elapsed().as_secs_f64() < 0.5 * DEBUG_SLOWER, "a view took {:?}", t.elapsed());
    }
    let card = support::send(port, "GET", "/card", &[], "").body;
    assert!(card.contains("<link rel=\"stylesheet\" href=\"/_quillo/s/"), "{}", card);
    let plain = support::send(port, "GET", "/plain", &[], "").body;
    assert!(!plain.contains("rel=\"stylesheet\""), "{}", plain);
}

// ---- Q-1069: the markdown door -------------------------------------------------------------

/// Shapes of Markdown that make the most for their size, and ordinary ones, `n` bytes each.
fn markdown_shapes(n: usize) -> Vec<(&'static str, String)> {
    let fill = |unit: &str| unit.repeat(n / unit.len());
    let doc = "# Title\n\nSome *text* with a [link](https://e.com/a) and `code`.\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> a quote\n\n1. first\n2. second\n\n";
    vec![
        ("brackets", fill("[")),
        ("quotes", fill(">")),
        ("lt", fill("<")),
        ("nested lists", fill("1. ")),
        ("quote lines", fill("> ")),
        ("images", fill("![")),
        ("emphasis", fill("*a ")),
        ("headings", fill("# h\n")),
        ("links", fill("[a](b) ")),
        ("table", format!("|a|b|\n|-|-|\n{}", fill("|1|2|\n"))),
        ("entities", fill("&amp;")),
        ("paragraphs", fill("word word word\n\n")),
        ("prose", fill("the quick brown fox jumps over the lazy dog ")),
        ("quotation marks", fill("\"")),
        ("rust code", format!("```rust\n{}\n```", fill("1 "))),
        ("rust code lines", format!("```rust\n{}```\n", fill("let a = f(1); // c\n"))),
        ("js code", format!("```js\n{}\n```", fill("a(\"b\", 1) "))),
        ("unknown code", format!("```zzz\n{}\n```", fill("1 "))),
        ("plain fence", format!("~~~\n{}\n~~~", fill("1 "))),
        ("indented code", fill("    1 1 1\n")),
        ("quoted fence", format!("> ```js\n{}", fill("> a(1)\n"))),
        ("listed fence", format!("- ```rust\n{}", fill("  1 2\n"))),
        ("control characters", fill("\u{1}")),
        ("pipeless rows", format!("|a|\n|-|\n{}", fill("x\n"))),
        ("wide pipeless rows", format!("|{}\n|{}\n{}", "a|".repeat(50), "-|".repeat(50), fill("x\n"))),
        ("document", fill(doc)),
    ]
}

/// What the markdown door works out a text will take (`server::markdown_room`) is at least what
/// making it takes, for the shapes that make the most for their size (1 MB of `[` made 178 MB) and
/// for ordinary ones: what it lets through under `--max-memory` fits there (FEEDBACK75.md, Q-1069;
/// branch feedback75)
#[test]
fn the_markdown_doors_estimate_covers_what_is_made() {
    let _one = serial();
    let mut it = app("route GET / = \"hi\"");
    let mut short = Vec::new();
    for size in [100_000, 10_000] {
        for (name, text) in markdown_shapes(size) {
            let body = serde_json::json!([text]).to_string();
            let (r, peak) = peak_of(|| send_bytes(&mut it, "POST", "/_quillo/markdown", &[("Content-Type", "application/json")], body.into_bytes()));
            assert_eq!(r.status, 200, "{}", name);
            let room = quillo::server::markdown_room(&text);
            eprintln!("{:7} {:20} made {:7} KB, worked out {:7} KB ({:.2})", size, name, peak >> 10, room >> 10, room as f64 / peak as f64);
            if peak > room {
                short.push(format!("{} at {}: made {} KB, worked out {} KB", name, size, peak >> 10, room >> 10));
            }
        }
    }
    // The second review's: 900 KB of Rust code in one fence, about 1 KB of gzip.
    let code = format!("```rust\n{}\n```", "1 ".repeat(450_000));
    let body = serde_json::json!([code]).to_string();
    let (r, peak) = peak_of(|| send_bytes(&mut it, "POST", "/_quillo/markdown", &[("Content-Type", "application/json")], body.into_bytes()));
    assert_eq!(r.status, 200);
    let room = quillo::server::markdown_room(&code);
    eprintln!("the review's Rust code: made {} KB, worked out {} KB", peak >> 10, room >> 10);
    if peak > room {
        short.push(format!("the review's Rust code: made {} KB, worked out {} KB", peak >> 10, room >> 10));
    }
    assert!(short.is_empty(), "{:#?}", short);
}

/// Under `--max-memory 8MB`, the markdown door refuses, with the limit's stop, a text whose tree
/// would pass it, before it is made (1 MB of `[` or `>` made 178 and 194 MB), and answers ordinary
/// text as before: a 60 KB document, empty, one character, unicode, options; with no limit, a
/// megabyte as before (FEEDBACK75.md, Q-1069; branch feedback75)
#[test]
fn the_markdown_door_holds_its_tree_to_the_limit() {
    let _one = serial();
    let mut it = app("route GET / = \"hi\"");
    let json = [("Content-Type", "application/json")];
    let mut md = |body: &[u8]| {
        let r = send_bytes(&mut it, "POST", "/_quillo/markdown", &json, body.to_vec());
        (r.status, r.text())
    };
    let quotes = serde_json::json!([">".repeat(MB - 40)]).to_string();
    let brackets = serde_json::json!(["[".repeat(MB - 40)]).to_string();
    {
        let _limit = Limit::set(8 << 20);
        for body in [&quotes, &brackets] {
            let (r, peak) = peak_of(|| md(body.as_bytes()));
            assert_eq!(r, (500, STOP_8MB.to_string()));
            assert!(peak < 8 * MB, "{} MB", peak / MB);
        }
        let doc = markdown_shapes(60_000).pop().unwrap().1;
        let (status, text) = md(serde_json::json!([doc]).to_string().as_bytes());
        assert!(status == 200 && text.starts_with(r#"{"ok":"<h1>Title</h1>"#), "{}", &text[..100.min(text.len())]);
        assert_eq!(md(br#"["*a*"]"#), (200, r#"{"ok":"<p><em>a</em></p>\n"}"#.to_string()));
        assert_eq!(md(br#"[""]"#), (200, r#"{"ok":""}"#.to_string()));
        assert_eq!(md(br#"["a"]"#), (200, r#"{"ok":"<p>a</p>\n"}"#.to_string()));
        assert_eq!(md(r#"["ünï *😀*", {"inline": true}]"#.as_bytes()), (200, r#"{"ok":"ünï <em>😀</em>"}"#.to_string()));
    }
    let (status, text) = md(quotes.as_bytes());
    assert!(status == 200 && text.starts_with(r#"{"ok":"<blockquote>\n<blockquote>"#) && text.ends_with(r#"</blockquote>\n"}"#), "{}", status);
}

/// The markdown door reads its JSON as a visitor's and writes its answer once, from the HTML: the
/// same answers as serde's (quotes, backslashes, new lines, unicode, empty), and a body that is
/// not UTF-8 or not `[text, options]` refused as before (FEEDBACK75.md, Q-1069; branch feedback75)
#[test]
fn the_markdown_door_answers_as_before() {
    let _one = serial();
    let mut it = app("route GET / = \"hi\"");
    let json = [("Content-Type", "application/json")];
    let mut md = |body: &[u8]| {
        let r = send_bytes(&mut it, "POST", "/_quillo/markdown", &json, body.to_vec());
        (r.status, r.text())
    };
    assert_eq!(md(br#"["*a*"]"#), (200, r#"{"ok":"<p><em>a</em></p>\n"}"#.to_string()));
    assert_eq!(md(br#"[""]"#), (200, r#"{"ok":""}"#.to_string()));
    assert_eq!(md(r#"["ünï *😀*", {"inline": true}]"#.as_bytes()), (200, r#"{"ok":"ünï <em>😀</em>"}"#.to_string()));
    let (status, text) = md(br#"["a \"q\" \\ b\n\n`c\td`"]"#);
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(text, serde_json::json!({ "ok": v["ok"] }).to_string());
    assert!(v["ok"].as_str().unwrap().contains("<code>c\td</code>"), "{}", text);
    for bad in [&b"{}"[..], b"[]", b"[1, 2, 3]", b"\xff", b"[\"a\""] {
        assert_eq!(md(bad), (400, r#"{"error":"the body must be a JSON array: [text, options]"}"#.to_string()), "{:?}", bad);
    }
}

// ==== Wave 2: FEEDBACK75's Q-1058, Q-1063, Q-1064, Q-1068, Q-1071, Q-1081, Q-1086, Q-1087,
// Q-1121, and FEEDBACK78's Q-1158 and Q-1159 =====================================================

/// `quillo check` of a program, its output (stdout and stderr) and whether it passed.
fn check_said(d: &Path, file: &str) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", file]).current_dir(d).output().unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// The page states a page carries (its `.g=` script, as JSON), or none.
fn states_of(page: &str) -> Option<serde_json::Value> {
    let at = page.find(".g=")? + 3;
    let text = &page[at..at + page[at..].find("</script>")?];
    let json: String = serde_json::from_str(text).ok()?;
    serde_json::from_str(&json).ok()
}

const PAGE_STATES: &str = "fn load_me() {\n    return { id: 1, name: \"Ana\", password_hash: \"$argon2id$HASH\", totp_secret: \"JBSWY3DP\" }\n}\nstate me = load_me()\nstate api = env(\"SECRET_API_KEY\") ?? \"none\"\nstate total = 3\nstate label = \"ünï ✓\"\nfn shown() {\n    return label\n}\ncomponent Counter() {\n    state n = 0\n    <button onclick={() => { n = n + 1 }}>Clicked {n}</button>\n}\ncomponent Shows() {\n    <p>{total} {shown()}</p>\n}\nroute GET / = <html><body><h1>Hello {me.name}</h1><Counter/></body></html>\nroute GET /shows = <html><body><h1>Hello {me.name}</h1><Shows/></body></html>\nroute GET /plain = <html><body><h1>Hello {me.name}</h1></body></html>\nroute GET /whole = <html><head><script src=\"/_quillo/quillo.js\"></script></head><body><Counter/></body></html>\n";

/// A page carries only the page states its components read: none for a component that reads none
/// (a user row with its password hash and an `env` secret went to every page with a component),
/// those a component reads, through a function too, and with the whole program's bundle those any
/// component reads; a page with no component nothing, as before (FEEDBACK75.md, Q-1058, the user's
/// choice; branch feedback75)
#[test]
fn a_page_carries_only_the_states_its_components_read() {
    let d = dir("page-states", &[("main.qwe", PAGE_STATES)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let (_g, port) = serve(&d, "main.qwe", &[], &[("SECRET_API_KEY", "sk_live_PAGESTATE77")], "states.log");
    let page = support::send(port, "GET", "/", &[], "").body;
    assert!(page.contains("<h1>Hello Ana</h1>") && page.contains("Clicked"), "{}", page);
    assert_eq!(states_of(&page), None, "{}", page);
    for secret in ["HASH", "JBSWY3DP", "sk_live_PAGESTATE77"] {
        assert!(!page.contains(secret), "{}: {}", secret, page);
    }
    let page = support::send(port, "GET", "/shows", &[], "").body;
    assert_eq!(states_of(&page), Some(serde_json::json!({ "total": 3, "label": "ünï ✓" })), "{}", page);
    assert!(!page.contains("HASH") && !page.contains("sk_live"), "{}", page);
    let page = support::send(port, "GET", "/whole", &[], "").body;
    assert_eq!(states_of(&page), Some(serde_json::json!({ "total": 3, "label": "ünï ✓" })), "{}", page);
    let page = support::send(port, "GET", "/plain", &[], "").body;
    assert_eq!((states_of(&page), page.contains("Hello Ana")), (None, true), "{}", page);
}

/// `quillo check` warns of a page state a component reads that comes from `env`, a file or a
/// `select *` row, through the program's functions on either side, at the state's line; not of
/// one only the route's HTML reads, one from named columns, or one no component reads
/// (FEEDBACK75.md, Q-1058, the user's choice: a warning; branch feedback75)
#[test]
fn check_warns_of_a_page_state_from_the_server_that_a_component_reads() {
    let src = "fn load_me() with db {\n    return db.one(\"select * from users where id = ?\", 1)\n}\nfn named() with db {\n    return db.one(\"select id, name from users where id = ?\", 1)\n}\nfn key() {\n    return env(\"API_KEY\") ?? \"none\"\n}\nstate me = load_me()\nstate api = key()\nstate cfg = read_file(\"cfg.txt\") ?? \"\"\nstate row = named()\nstate secret = env(\"OTHER\") ?? \"\"\nstate theme = \"dark\"\nfn shown() {\n    return api\n}\ncomponent Badge() {\n    <p>{me.name} {theme} {row.name} {cfg}</p>\n}\ncomponent Other() {\n    <p>{shown()}</p>\n}\nroute GET / = <html><body><p>{secret}</p><Badge/><Other/></body></html>\n";
    let d = dir("check-states", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    let (ok, said) = check_said(&d, "main.qwe");
    assert!(ok, "{}", said);
    let warn = |line: u32, name: &str, c: &str, from: &str| format!("main.qwe:{}: warning: `{}` is a page state that <{}> reads, so its whole value is written into each page that shows <{}>, and it comes from {}: give the component what it shows, as a prop or a state of those fields alone", line, name, c, c, from);
    assert!(said.contains(&warn(10, "me", "Badge", "a `select *` row")), "{}", said);
    assert!(said.contains(&warn(11, "api", "Other", "`env`")), "{}", said);
    assert!(said.contains(&warn(12, "cfg", "Badge", "a file (`read_file`)")), "{}", said);
    assert!(!said.contains("`row` is a page state") && !said.contains("`secret` is a page state") && !said.contains("`theme` is a page state"), "{}", said);
    assert_eq!(said.matches("is a page state that").count(), 3, "{}", said);
}

/// A component reading a top-level constant made by `crypto`, `args`, `read_line`, `stdin`,
/// `run_script` or a site's call is refused at load, as one from `env` is: their values went into
/// the public bundle (a generated signing key, a command-line secret); a constant of plain values
/// still goes (FEEDBACK75.md, Q-1063; branch feedback75)
#[test]
fn the_bundle_refuses_what_is_the_servers_own() {
    let _one = serial();
    let load = |head: &str| {
        let src = format!("{}\ncomponent Pay() {{\n    state n = 0\n    <button onclick={{() => {{ n = n + 1 }}}}>Pay {{n}} {{K}}</button>\n}}\nroute GET / = <html><body><Pay/></body></html>\n", head);
        let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
        match it.load_source(&src, "case.qwe") {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    let refused = "`K` is read from the server's environment, files, database, a service, a plugin, its command line or input, a site or `crypto`, and would be sent to every browser";
    for head in [
        "let K = crypto.token()",
        "let K = crypto.sha256(\"x\")",
        "let K = args()",
        "let K = false ? read_line() : \"x\"",
        "let K = false ? stdin() : \"x\"",
        "let K = false ? run_script(\"1\") : \"x\"",
        "site shop = \"https://shop.example.com\"\nlet K = shop",
        "fn secret_of() {\n    return crypto.token()\n}\nlet K = secret_of()",
    ] {
        let said = load(head);
        assert!(said.contains(refused), "{}: {}", head, said);
    }
    for head in ["let K = \"Shop\"", "let K = 3 + 4", "let K = [\"ünï\", 1]"] {
        assert_eq!(load(head), "", "{}", head);
    }
}

/// A stream's and an event stream's `send` write a builtin's error as `internal error`, its text
/// (an internal address with its key) to the log, as a route's answer does; an error the program
/// wrote, and plain values, go as before (FEEDBACK75.md, Q-1068; branch feedback75)
#[test]
fn streams_hide_a_builtins_error() {
    let src = "server fn fetch_report() with http {\n    return http.get(\"http://127.0.0.1:1/internal/report?api_key=sk_live_SECRET123\")\n}\nroute GET /plain = { result: fetch_report() }\nroute GET /ev = events((send) => {\n    send({ step: 1, result: fetch_report() })\n    send({ step: 2, mine: err(\"out of stock\"), name: \"ünï\" })\n})\nroute GET /st = stream((send) => {\n    send({ step: 1, result: fetch_report() })\n})\n";
    let d = dir("stream-errors", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let (_g, port) = serve(&d, "main.qwe", &["--no-wt"], &[], "stream.log");
    assert_eq!(support::send(port, "GET", "/plain", &[], "").body, r#"{"result":{"error":"internal error"}}"#);
    let ev = support::send(port, "GET", "/ev", &[], "").body;
    assert!(ev.contains(r#"data: {"step":1,"result":{"error":"internal error"}}"#), "{}", ev);
    assert!(ev.contains(r#"data: {"step":2,"mine":{"error":"out of stock"},"name":"ünï"}"#), "{}", ev);
    assert!(!ev.contains("sk_live"), "{}", ev);
    let st = support::send(port, "GET", "/st", &[], "").body;
    assert!(st.contains(r#"{"step":1,"result":{"error":"internal error"}}"#) && !st.contains("sk_live"), "{}", st);
    let end = Instant::now() + Duration::from_secs(5);
    let mut log = String::new();
    while log.matches("sk_live_SECRET123").count() < 3 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
        log = std::fs::read_to_string(d.join("stream.log")).unwrap_or_default();
    }
    assert!(log.matches("sk_live_SECRET123").count() >= 3, "the log has each error: {}", log);
}

/// A server fn's argument that fails as it is made (a record's default) answers 500 `internal
/// error in f`, its file, line and message to the log, as a body's failure does, where the 400 told
/// any caller; a `for [...]` function is `no server fn` to a page, naming no service; a default that
/// replies, and plain calls, answer as before (FEEDBACK75.md, Q-1086; branch feedback75)
#[test]
fn a_server_fns_internals_stay_in_the_log() {
    let _one = serial();
    let mut it = app("fn fee() -> int {\n    return 100 - int(\"x\")\n}\ntype Fee = { amount: int, fee: int = fee() }\nserver fn with_fee(f: Fee) = f.amount\nfn stop() {\n    reply 418 \"no\"\n}\ntype Thing = { a: int = stop() }\nserver fn thing(t: Thing) = 1\nserver fn admin_only() for [\"billing\"] = \"secret\"\nserver fn open_fn(n: int) = n + 1\n");
    let r = send_bytes(&mut it, "POST", "/_quillo/rpc/with_fee", &[("Content-Type", "application/json")], br#"[{"amount":5}]"#.to_vec());
    assert_eq!((r.status, r.text()), (500, r#"{"error":"internal error in `with_fee`"}"#.to_string()));
    assert!(r.headers.iter().any(|(k, v)| k == "x-quillo-answer" && v == "failed"), "{:?}", r.headers);
    assert_eq!(rpc(&mut it, "admin_only", "[]"), (404, r#"{"error":"no server fn `admin_only`"}"#.to_string()));
    assert_eq!(rpc(&mut it, "nothing", "[]"), (404, r#"{"error":"no server fn `nothing`"}"#.to_string()));
    assert_eq!(rpc(&mut it, "thing", "[{}]"), (400, r#"{"error":"argument `t` of `thing` could not be made"}"#.to_string()));
    assert_eq!(rpc(&mut it, "open_fn", "[1]"), (200, r#"{"ok":2}"#.to_string()));
    assert_eq!(rpc(&mut it, "with_fee", r#"[{"amount":5,"fee":1}]"#), (200, r#"{"ok":5}"#.to_string()));
    assert_eq!(rpc(&mut it, "open_fn", r#"["x"]"#).0, 400);
}

/// A program's `site_headers` reach the answers of a program it mounts, each header the answer
/// lacks; the mount's own rule and the route's own header win (FEEDBACK75.md, Q-1087; branch
/// feedback75)
#[test]
fn site_headers_reach_a_mounted_program() {
    let app = "site_headers({ \"content-security-policy\": \"default-src 'self'\", \"strict-transport-security\": \"max-age=31536000\", \"referrer-policy\": \"same-origin\" })\nmount /notes = \"notes.qwe\"\nroute GET / = \"app\"\n";
    let notes = "site_headers({ \"referrer-policy\": \"no-referrer\" })\nroute GET / = \"notes\"\nroute GET /own = respond(200, \"own\", { \"content-security-policy\": \"default-src 'none'\" })\n";
    let d = dir("mount-headers", &[("app.qwe", app), ("notes.qwe", notes)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = serve(&d, "app.qwe", &[], &[], "mount.log");
    let h = |p: &str, k: &str| support::send(port, "GET", p, &[], "").header(k).map(str::to_string);
    for p in ["/", "/notes/", "/notes/own"] {
        assert_eq!(h(p, "strict-transport-security").as_deref(), Some("max-age=31536000"), "{}", p);
    }
    assert_eq!(h("/notes/", "content-security-policy").as_deref(), Some("default-src 'self'"));
    assert_eq!(h("/notes/own", "content-security-policy").as_deref(), Some("default-src 'none'"));
    assert_eq!(h("/notes/", "referrer-policy").as_deref(), Some("no-referrer"));
    assert_eq!(h("/", "referrer-policy").as_deref(), Some("same-origin"));
    assert_eq!(support::send(port, "GET", "/notes/", &[], "").body, "notes");
}

/// Under `--split`, a service under a prefix: the app's `site_headers` on its answers (Q-1087); the
/// visitor's address, as the app took it, its `request.ip`, so a "this machine only" check refuses
/// them, and a client's own word for it is not believed (Q-1064); the app's session cookie not
/// passed to it, its own and other cookies kept (Q-1081); a large file streamed through, the app no
/// larger for it, a stream as it comes, and each `Set-Cookie` once (Q-1071) (FEEDBACK75.md; branch
/// feedback75)
#[test]
fn a_split_service_gets_the_visitor_and_streams_through() {
    let app = "site_headers({ \"content-security-policy\": \"default-src 'self'\", \"referrer-policy\": \"same-origin\" })\nservice /shop = \"shop.qwe\"\nroute GET / = \"app\"\nroute GET /whoami = { ip: request.ip }\nroute GET /setc {\n    set_session({ user: \"rootuser\" })\n    return \"set\"\n}\n";
    let shop = "route GET / = \"shop\"\nroute GET /whoami = { ip: request.ip, word: request.headers[\"x-quillo-client\"] }\nroute GET /admin {\n    if request.ip != \"127.0.0.1\" {\n        reply 403 \"local only\"\n    }\n    return \"admin panel\"\n}\nroute GET /in {\n    set_session({ user: \"shopper\" })\n    return \"in\"\n}\nroute GET /me = { user: session().user, cookies: keys(request.cookies) }\nroute GET /big = file(\"big.bin\")\nroute GET /small = file(\"small.txt\")\nroute GET /setc {\n    set_cookie(\"a\", \"1\")\n    set_cookie(\"b\", \"2\")\n    set_cookie(\"c\", \"3\")\n    return \"ok\"\n}\nroute GET /st = stream((send) => {\n    send(\"one\\n\")\n    sleep(800)\n    send(\"two\\n\")\n})\n";
    let d = dir("split-through", &[("app.qwe", app), ("shop.qwe", shop), ("small.txt", "ünï small\n")]);
    let _gone = Gone(d.clone());
    std::fs::write(d.join("big.bin"), vec![7u8; 24 << 20]).unwrap();
    checked(&d, "app.qwe");
    let (g, port) = serve(&d, "app.qwe", &["--split", "--trust-proxy", "127.0.0.1"], &[], "split2.log");
    let end = Instant::now() + Duration::from_secs(20);
    while support::send(port, "GET", "/shop/", &[], "").body != "shop" {
        assert!(Instant::now() < end, "the service did not start: {}", std::fs::read_to_string(d.join("split2.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(200));
    }
    // Q-1087
    let a = support::send(port, "GET", "/shop/", &[], "");
    assert_eq!((a.header("content-security-policy"), a.header("referrer-policy")), (Some("default-src 'self'"), Some("same-origin")));
    // Q-1064
    let xff = [("X-Forwarded-For", "203.0.113.9")];
    assert_eq!(support::send(port, "GET", "/whoami", &xff, "").body, r#"{"ip":"203.0.113.9"}"#);
    assert_eq!(support::send(port, "GET", "/shop/whoami", &xff, "").body, r#"{"ip":"203.0.113.9","word":null}"#);
    let a = support::send(port, "GET", "/shop/admin", &xff, "");
    assert_eq!((a.status, a.body.as_str()), (403, "local only"));
    let forged = [("X-Forwarded-For", "203.0.113.9"), ("x-quillo-client", "127.0.0.1;t=1;sig=00")];
    assert_eq!(support::send(port, "GET", "/shop/whoami", &forged, "").body, r#"{"ip":"203.0.113.9","word":null}"#);
    assert_eq!(support::send(port, "GET", "/shop/admin", &[], "").body, "admin panel");
    // Q-1081
    let cookie_of = |a: &support::Answer| a.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().to_string()).collect::<Vec<_>>();
    let root = cookie_of(&support::send(port, "GET", "/setc", &[], ""));
    let own = cookie_of(&support::send(port, "GET", "/shop/in", &[], ""));
    assert!(root.len() == 1 && root[0].starts_with("__Host-quillo_session="), "{:?}", root);
    assert!(own.len() == 1 && own[0].starts_with("quillo_session="), "{:?}", own);
    let line = format!("{}; {}; other=1", own[0], root[0]);
    assert_eq!(support::send(port, "GET", "/shop/me", &[("Cookie", &line)], "").body, r#"{"user":"shopper","cookies":["quillo_session","other"]}"#);
    assert_eq!(support::send(port, "GET", "/shop/me", &[("Cookie", &root[0])], "").body, r#"{"user":null,"cookies":[]}"#);
    // Q-1071
    let a = support::send(port, "GET", "/shop/setc", &[], "");
    assert_eq!(cookie_of(&a), vec!["a=1", "b=2", "c=3"]);
    assert_eq!(support::send(port, "GET", "/shop/small", &[], "").body, "ünï small\n");
    let ((status, len), grew) = rss_growth(g.0.id(), || {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        s.write_all(b"GET /shop/big HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        let head_end = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&out[..12]).to_string(), out.len() - head_end - 4)
    });
    assert!(status.ends_with("200") && len >= 24 << 20, "{} {}", status, len);
    assert!(grew < 12 * 1024, "24 MB through the app grew it by {} KB", grew);
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET /shop/st HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let t = Instant::now();
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    let mut first_at = None;
    while let Ok(n) = s.read(&mut buf) {
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
        if first_at.is_none() && String::from_utf8_lossy(&got).contains("one") {
            first_at = Some(t.elapsed());
        }
    }
    let text = String::from_utf8_lossy(&got);
    assert!(text.contains("one") && text.contains("two"), "{}", text);
    assert!(first_at.unwrap() < Duration::from_millis(600) && t.elapsed() >= Duration::from_millis(700), "{:?} {:?}", first_at, t.elapsed());
}

/// A component's `raw(t("key"))` puts its key in the page's translations as `t` does: the page
/// showed the key once its components took over (FEEDBACK75.md, Q-1121; branch feedback75)
#[test]
fn raw_t_in_a_component_carries_its_key() {
    let src = "translations(\"locales\")\ncomponent Greet() {\n    state n = 0\n    <div><p>{raw(t(\"hello\", { name: \"Ana\" }))}</p><p>{t(\"plain\")}</p><button onclick={() => { n = n + 1 }}>{n}</button></div>\n}\nroute GET / = <html><body><Greet/></body></html>\n";
    let d = dir("raw-t", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    std::fs::create_dir_all(d.join("locales")).unwrap();
    std::fs::write(d.join("locales/en.json"), r#"{"hello": "Hello <b>{name}</b> ü", "plain": "Plain", "other": "Other"}"#).unwrap();
    checked(&d, "main.qwe");
    let (_g, port) = serve(&d, "main.qwe", &[], &[], "raw-t.log");
    let page = support::send(port, "GET", "/", &[], "").body;
    assert!(page.contains("Hello <b>Ana</b> ü"), "{}", page);
    let at = page.find("/_quillo/t/").unwrap_or_else(|| panic!("{}", page));
    let path = &page[at..at + page[at..].find(".js").unwrap() + 3];
    let file = support::send(port, "GET", path, &[], "").body;
    assert!(file.contains("\"hello\":\"Hello \\u003cb>{name}\\u003c/b> ü\"") && file.contains("\"plain\":\"Plain\"") && !file.contains("\"other\""), "{}", file);
}

// ---- Wave 2's review: Q-1064 (with httpd's part), Q-1087, Q-1071, Q-1063, Q-1058 -------------------

/// A request read whole off the wire: its status, its headers (names in lower case) and its body,
/// unchunked, as bytes.
fn wire(port: u16, path: &str, headers: &[(&str, &str)]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let mut head = format!("GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n", path);
    for (k, v) in headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let end = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let text = String::from_utf8_lossy(&out[..end]).to_string();
    let mut lines = text.split("\r\n");
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let hs: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let mut body = out[end + 4..].to_vec();
    if hs.iter().any(|(k, v)| k == "transfer-encoding" && v.contains("chunked")) {
        let (mut raw, mut plain) = (&body[..], Vec::new());
        loop {
            let at = raw.windows(2).position(|w| w == b"\r\n").unwrap();
            let n = usize::from_str_radix(String::from_utf8_lossy(&raw[..at]).trim(), 16).unwrap();
            if n == 0 {
                break;
            }
            plain.extend_from_slice(&raw[at + 2..at + 2 + n]);
            raw = &raw[at + 4 + n..];
        }
        body = plain;
    }
    (status, hs, body)
}

fn named<'a>(hs: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    hs.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect()
}

/// `'sha256-...'` of a script's text, as a policy names it.
fn csp_hash(text: &[u8]) -> String {
    use sha2::Digest;
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let d = sha2::Sha256::digest(text);
    let mut out = String::from("'sha256-");
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out.push('\'');
    out
}

/// Whether the page's policy names the hash of each of its inline scripts, and of each of its
/// `<style>` elements, and how many scripts there were.
fn policy_lets_through(policy: &str, page: &[u8]) -> (bool, usize) {
    let text = String::from_utf8_lossy(page);
    let inline = |open: &str, close: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = &*text;
        while let Some(i) = rest.find(open) {
            let after = &rest[i + open.len()..];
            let Some(end) = after.find(close) else { break };
            out.push(after[..end].to_string());
            rest = &after[end + close.len()..];
        }
        out
    };
    let scripts = inline("<script>", "</script>");
    let styles = inline("<style>", "</style>");
    let all = scripts.iter().chain(styles.iter()).all(|t| policy.contains(&csp_hash(t.as_bytes())));
    (all, scripts.len())
}

const SPLIT_APP: &str = "site_headers({ \"content-security-policy\": \"default-src 'self'\", \"referrer-policy\": \"same-origin\" })\nservice /shop = \"shop.qwe\"\nroute GET / = \"app\"\n";
const SPLIT_SHOP: &str = "state label = \"ünï\"\ncomponent Count() {\n    state n = 0\n    <button id=\"b\" onclick={() => { n = n + 1 }}>{label} {n}</button>\n}\nroute GET / = \"shop\"\nroute GET /del {\n    delete_cookie(\"x\")\n    return \"deleted\"\n}\nroute GET /page = <html><body><Count/></body></html>\nroute GET /bare = respond(200, <html><body><Count/></body></html>, { \"content-security-policy\": \"script-src 'self'\" })\nroute GET /text = \"a\".repeat(24000)\nroute GET /json = { rows: range(500) }\nroute GET /big = \"b\".repeat(3600000)\nroute GET /bin = file(\"big.bin\")\n";

/// Starts the app of `SPLIT_APP` with `--split --trust-proxy 127.0.0.1`, once its service answers.
fn split_shop(name: &str) -> (Gone, support::Guard, u16) {
    let d = dir(name, &[("app.qwe", SPLIT_APP), ("shop.qwe", SPLIT_SHOP)]);
    std::fs::write(d.join("big.bin"), vec![7u8; 3 << 20]).unwrap();
    checked(&d, "app.qwe");
    let (g, port) = serve(&d, "app.qwe", &["--split", "--trust-proxy", "127.0.0.1"], &[], "split.log");
    let end = Instant::now() + Duration::from_secs(20);
    while support::send(port, "GET", "/shop/", &[], "").body != "shop" {
        assert!(Instant::now() < end, "the service did not start: {}", std::fs::read_to_string(d.join("split.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(200));
    }
    (Gone(d), g, port)
}

/// Under `--split`, a visitor not on this machine: the service takes its address from the app's word
/// as the request is read, and the app's word on the scheme is still believed, from the peer the
/// request came from, so a deletion behind an `https` proxy is `Secure`, as in 0.8.8 and for a
/// visitor on this machine, where it was not; a visitor's own words and mark are not believed
/// (FEEDBACK75.md, Q-1064 and its review; branch feedback75)
#[test]
fn a_split_service_keeps_the_scheme_of_a_visitor_elsewhere() {
    let (_gone, _g, port) = split_shop("split-scheme");
    let deletion = |hs: &[(&str, &str)]| support::send(port, "GET", "/shop/del", hs, "").header("set-cookie").map(str::to_string).unwrap_or_default();
    let https = ("X-Forwarded-Proto", "https");
    let visitor = ("X-Forwarded-For", "203.0.113.9");
    assert!(deletion(&[https, visitor]).contains("; Secure"), "{}", deletion(&[https, visitor]));
    assert!(deletion(&[https]).contains("; Secure"));
    assert!(deletion(&[https, ("X-Forwarded-For", "2001:db8::7")]).contains("; Secure"));
    assert!(!deletion(&[("X-Forwarded-Proto", "http"), visitor]).contains("Secure"));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs().to_string();
    let forged_scheme = format!("https;t={};sig={}", now, "0".repeat(64));
    let forged = [("X-Forwarded-Proto", "http"), visitor, ("x-quillo-from-app", "1"), ("x-quillo-scheme", forged_scheme.as_str())];
    assert!(!deletion(&forged).contains("Secure"), "{}", deletion(&forged));
    assert!(!deletion(&[visitor, ("x-quillo-from-app", "1")]).contains("Secure"));
}

/// A program's `site_headers` policy reaches a mounted program's page and a split service's with
/// Quillo's scripts let through it: their hashes are in the policy the visitor gets, every inline
/// script's and the islands' style's, where the bare policy blocked the page's components; the
/// mount's own policy wins, its scripts let through too; a visitor's own policy word is not
/// taken; and in Chrome the component runs (FEEDBACK75.md, Q-1087, review; branch feedback75)
#[test]
fn a_parents_policy_lets_a_mounted_pages_scripts_through() {
    let app = "site_headers({ \"content-security-policy\": \"default-src 'self'\", \"strict-transport-security\": \"max-age=60\" })\nmount /notes = \"notes.qwe\"\nroute GET / = \"app\"\n";
    let notes = "site_headers(\"/own\", { \"content-security-policy\": \"script-src 'self'\" })\nsite_headers(\"/none\", { \"content-security-policy\": null })\nstate label = \"ünï\"\ncomponent Count() {\n    state n = 0\n    <button id=\"b\" onclick={() => { n = n + 1 }}>{label} {n}</button>\n}\nroute GET / = <html><body><Count/></body></html>\nroute GET /own = <html><body><Count/></body></html>\nroute GET /none = <html><body><Count/></body></html>\nroute GET /text = \"plain\"\n";
    let d = dir("mount-policy", &[("app.qwe", app), ("notes.qwe", notes)]);
    let _gone = Gone(d.clone());
    checked(&d, "app.qwe");
    let (_g, port) = serve(&d, "app.qwe", &[], &[], "mount-policy.log");
    let (status, hs, page) = wire(port, "/notes/", &[]);
    let policy = named(&hs, "content-security-policy");
    assert_eq!((status, policy.len()), (200, 1), "{:?}", hs);
    assert!(policy[0].starts_with("default-src 'self'; script-src 'self' 'sha256-"), "{}", policy[0]);
    assert_eq!(policy_lets_through(policy[0], &page), (true, 3), "{}\n{}", policy[0], String::from_utf8_lossy(&page));
    assert_eq!(named(&hs, "strict-transport-security"), ["max-age=60"]);
    let (_, hs, page) = wire(port, "/notes/own", &[]);
    let policy = named(&hs, "content-security-policy");
    assert!(policy.len() == 1 && policy[0].starts_with("script-src 'self' 'sha256-") && policy_lets_through(policy[0], &page).0, "{:?}", policy);
    let (_, hs, _) = wire(port, "/notes/none", &[]);
    assert!(named(&hs, "content-security-policy").is_empty(), "{:?}", hs);
    assert_eq!(named(&wire(port, "/notes/text", &[]).1, "content-security-policy"), ["default-src 'self'"]);
    // A split service's page, the same.
    let (_shop_gone, _sg, shop) = split_shop("split-policy");
    let (_, hs, page) = wire(shop, "/shop/page", &[]);
    let policy = named(&hs, "content-security-policy");
    assert!(policy.len() == 1 && policy[0].starts_with("default-src 'self'; script-src 'self' 'sha256-"), "{:?}", hs);
    assert_eq!(policy_lets_through(policy[0], &page), (true, 3), "{}", policy[0]);
    let (_, hs, page) = wire(shop, "/shop/bare", &[]);
    let policy = named(&hs, "content-security-policy");
    assert!(policy.len() == 1 && policy[0].starts_with("script-src 'self' 'sha256-") && policy_lets_through(policy[0], &page).0, "{:?}", policy);
    let mine = format!("t={};sig={};{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), "0".repeat(64), "W1siY29udGVudC1zZWN1cml0eS1wb2xpY3kiLCJyZXBvcnQtdXJpIGh0dHBzOi8vZXZpbC5leGFtcGxlIl1d");
    let (_, hs, _) = wire(shop, "/shop/text", &[("x-quillo-policy", &mine)]);
    assert_eq!(named(&hs, "content-security-policy"), ["default-src 'self'"], "{:?}", hs);
    assert!(named(&hs, "x-quillo-policy-taken").is_empty(), "{:?}", hs);
    // A service run apart (`--service-url`) takes no word of its app's: its page keeps the policy
    // it came with (none, as in 0.8.8), which the app's would block; its other answers get the
    // app's.
    let apart = dir("apart-policy", &[("app.qwe", SPLIT_APP), ("shop.qwe", SPLIT_SHOP)]);
    let _apart_gone = Gone(apart.clone());
    std::fs::write(apart.join("big.bin"), b"x").unwrap();
    let secret = [("QUILLO_SERVICE_SECRET", "a shared secret for the apart test, long enough")];
    let (_svc, svc_port) = serve(&apart, "shop.qwe", &["--base", "/shop"], &secret, "svc.log");
    let url = format!("shop=http://127.0.0.1:{}", svc_port);
    let (_app, app_port) = serve(&apart, "app.qwe", &["--service-url", &url], &secret, "app.log");
    let (status, hs, _) = wire(app_port, "/shop/page", &[]);
    assert_eq!((status, named(&hs, "content-security-policy").len(), named(&hs, "x-quillo-policy-taken").len()), (200, 0, 0), "{:?}", hs);
    assert_eq!(named(&wire(app_port, "/shop/text", &[]).1, "content-security-policy"), ["default-src 'self'"]);
    let Some(chrome) = chrome_path() else {
        eprintln!("skipping the browser half: no Chrome");
        return;
    };
    let script = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  document.getElementById("b").click();
  await tick(300);
  document.body.dataset.result = JSON.stringify({ text: document.getElementById("b").textContent });
})();"#;
    for url in [format!("http://127.0.0.1:{}/notes/", port), format!("http://127.0.0.1:{}/shop/page", shop)] {
        let r = in_chrome(&chrome, &url, script);
        assert_eq!(r["text"].as_str(), Some("ünï 1"), "{}: {}", url, r);
    }
}

fn chrome_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `url` in headless Chrome, with `script` run in the page: what it stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir("feedback75b-chrome");
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    let _ = std::fs::remove_dir_all(&d);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

/// A split service's answers reach a visitor compressed as in 0.8.8: what the app reads whole
/// gzipped with its length (24 KB of text 58 bytes, 1,900 of JSON 931), a text past a megabyte
/// gzipped as it goes, a file past it as it comes; plain for a visitor that takes no gzip, `Vary`
/// saying so. They went out uncompressed and chunked, 24,000 and 3.6 MB (FEEDBACK75.md, Q-1071,
/// review; branch feedback75)
#[test]
fn a_split_services_answers_are_compressed_as_before() {
    use std::io::Read as _;
    let (_gone, _g, port) = split_shop("split-gzip");
    let gz = [("Accept-Encoding", "gzip")];
    let unzip = |b: &[u8]| {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(b).read_to_end(&mut out).unwrap();
        out
    };
    let (status, hs, body) = wire(port, "/shop/text", &gz);
    assert_eq!((status, named(&hs, "content-encoding"), named(&hs, "content-length"), body.len()), (200, vec!["gzip"], vec!["58"], 58), "{:?}", hs);
    assert_eq!(unzip(&body), vec![b'a'; 24000]);
    let (_, hs, body) = wire(port, "/shop/json", &gz);
    assert_eq!((named(&hs, "content-encoding"), named(&hs, "content-length"), body.len()), (vec!["gzip"], vec!["931"], 931), "{:?}", hs);
    assert!(String::from_utf8(unzip(&body)).unwrap().starts_with("{\"rows\":[0,1,2"));
    let (status, hs, body) = wire(port, "/shop/big", &gz);
    assert_eq!((status, named(&hs, "content-encoding"), named(&hs, "content-length")), (200, vec!["gzip"], Vec::<&str>::new()), "{:?}", hs);
    assert!(body.len() < 10_000 && named(&hs, "vary").iter().any(|v| v.contains("accept-encoding")), "{} {:?}", body.len(), hs);
    assert_eq!(unzip(&body), vec![b'b'; 3_600_000]);
    let (_, hs, body) = wire(port, "/shop/big", &[]);
    assert_eq!((named(&hs, "content-encoding"), body.len()), (Vec::<&str>::new(), 3_600_000), "{:?}", hs);
    assert!(named(&hs, "vary").iter().any(|v| v.contains("accept-encoding")), "{:?}", hs);
    let (_, hs, body) = wire(port, "/shop/text", &[]);
    assert_eq!((named(&hs, "content-encoding"), body.len()), (Vec::<&str>::new(), 24000));
    let (_, hs, body) = wire(port, "/shop/bin", &gz);
    assert_eq!((named(&hs, "content-encoding"), body.len()), (Vec::<&str>::new(), 3 << 20), "{:?}", hs);
}

/// The bundle's guard reads a name as the program binds it: a lambda's parameter, a function's
/// local, a loop's variable named `args` or `crypto` is not the builtin, and a component reading
/// a constant made with them loads, as in 0.8.8; a source called as a method (`"40 + 2"
/// .run_script()`) and `uuid()` are the server's own and refused (FEEDBACK75.md, Q-1063, review;
/// branch feedback75)
#[test]
fn the_bundle_guard_reads_names_as_the_program_binds_them() {
    let _one = serial();
    let load = |head: &str| {
        let src = format!("{}\ncomponent Pay() {{\n    state n = 0\n    <button onclick={{() => {{ n = n + 1 }}}}>Pay {{n}} {{K}}</button>\n}}\nroute GET / = <html><body><Pay/></body></html>\n", head);
        let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
        match it.load_source(&src, "case.qwe") {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    let refused = "`K` is read from the server's environment, files, database, a service, a plugin, its command line or input, a site or `crypto`, and would be sent to every browser";
    for head in [
        "let K = [1, 2].map((args) => args * 2)",
        "let FMT = { sum: (args) => args.a + args.b }\nlet K = FMT.sum({ a: 1, b: 2 })",
        "fn helper(xs) {\n    let args = xs\n    return len(args)\n}\nlet K = helper([1])",
        "let K = { run: (crypto) => crypto }",
        "fn each_arg(xs) = xs.map((args) => args + 1)\nlet K = each_arg([1])",
        "fn total(xs) {\n    let t = 0\n    for args in xs {\n        t += args\n    }\n    return t\n}\nlet K = total([1, 2])",
        "fn pick(x) {\n    return match x {\n        { file } => file,\n        _ => \"none\",\n    }\n}\nlet K = pick({ file: \"a\" })",
        "let K = { file: \"ünï\" }.file",
    ] {
        assert_eq!(load(head), "", "{}", head);
    }
    for head in [
        "let K = \"40 + 2\".run_script()",
        "let K = false ? \"x\".read_file() : \"y\"",
        "let K = uuid()",
        "fn token_of() {\n    return uuid()\n}\nlet K = token_of()",
        "fn helper(xs) {\n    let n = xs\n    return args()\n}\nlet K = helper([1])",
        "let K = [1].map((x) => crypto.token())",
    ] {
        let said = load(head);
        assert!(said.contains(refused), "{}: {}", head, said);
    }
}

/// A plugin function whose file says it does nothing (`//! effects: none`) gives the page what it
/// is given, and a component may show a constant made with it; one that may read the environment
/// is the server's own, called or as a method (FEEDBACK75.md, Q-1063, review; branch feedback75)
#[test]
fn a_plugin_that_does_nothing_is_not_the_servers_own() {
    let comp = "component Pay() {\n    state n = 0\n    <button onclick={() => { n = n + 1 }}>Pay {n} {K}</button>\n}\nroute GET / = <html><body><Pay/></body></html>\n";
    let d = dir("plugin-effects", &[
        ("hi.qwer", "//! effects: none\npub fn hi(name: &str) -> String { format!(\"hi {}\", name) }\n"),
        ("home.qwer", "//! effects: env\npub fn home(name: &str) -> String { format!(\"{}{}\", name, std::env::var(\"HOME\").unwrap_or_default()) }\n"),
    ]);
    let _gone = Gone(d.clone());
    // What the server says as it starts: where it listens, or why it did not load.
    let run = |head: &str| {
        std::fs::write(d.join("main.qwe"), format!("use \"hi.qwer\"\nuse \"home.qwer\"\n{}\n{}", head, comp)).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["build", "main.qwe"]).current_dir(&d).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let log = std::fs::File::create(d.join("start.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "main.qwe", "--no-build", "--port", "0"]).current_dir(&d).stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap();
        let mut g = support::Guard::new(child);
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let said = std::fs::read_to_string(d.join("start.log")).unwrap_or_default();
            if said.contains("listening") || g.0.try_wait().ok().flatten().is_some() || Instant::now() > end {
                return std::fs::read_to_string(d.join("start.log")).unwrap_or_default();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let said = run("let K = hi(\"ünï\")");
    assert!(said.contains("listening"), "{}", said);
    let refused = "`K` is read from the server's environment, files, database, a service, a plugin, its command line or input, a site or `crypto`, and would be sent to every browser";
    assert!(run("let K = home(\"x\")").contains(refused));
    assert!(run("let K = \"x\".home()").contains(refused));
}

/// `quillo check`'s page-state warning follows a view a component shows (`<Shows/>`), and not a
/// loop's variable of the state's name; the server fn error does not either; a chain of 8,000
/// calls under one state checks in about the time it takes without the state, where a fixpoint
/// over every function took seconds (FEEDBACK75.md, Q-1058, review; branch feedback75)
#[test]
fn check_follows_page_states_through_views_and_scopes() {
    let src = "state secret = env(\"SECRET\") ?? \"\"\nstate me = env(\"ME\") ?? \"\"\nstate key = read_file(\"k.txt\") ?? \"\"\nview Shows() {\n    <i>{secret}</i>\n}\ncomponent Box() {\n    state n = 0\n    <div><Shows/><button onclick={() => { n = n + 1 }}>{n}</button></div>\n}\ncomponent People() {\n    state people = [\"a\", \"b\"]\n    <ul>{for me in people { <li>{me}</li> }}</ul>\n}\ncomponent Keys() {\n    <p>{[1, 2].map((key) => key + 1)}</p>\n}\nserver fn count(people: list) {\n    let t = 0\n    for me in people {\n        t += 1\n    }\n    return t\n}\nroute GET / = <html><body><Box/><People/><Keys/></body></html>\n";
    let d = dir("check-views", &[("main.qwe", src)]);
    let _gone = Gone(d.clone());
    let (ok, said) = check_said(&d, "main.qwe");
    assert!(ok, "{}", said);
    assert!(said.contains("main.qwe:1: warning: `secret` is a page state that <Box> reads"), "{}", said);
    assert_eq!(said.matches("is a page state that").count(), 1, "{}", said);
    assert!(!said.contains("runs for no page"), "{}", said);
    let chain = |state: bool| {
        let mut src = String::from(if state { "state top = 1\n" } else { "let top = 1\n" });
        for i in 0..8000 {
            src.push_str(&format!("fn f{}() {{\n    return f{}()\n}}\n", i, i + 1));
        }
        src.push_str("fn f8000() {\n    return top\n}\nroute GET / = f0()\n");
        std::fs::write(d.join("chain.qwe"), src).unwrap();
        let t = Instant::now();
        let (ok, said) = check_said(&d, "chain.qwe");
        assert!(ok, "{}", said);
        t.elapsed()
    };
    let plain = chain(false);
    let with_state = chain(true);
    assert!(with_state < plain * 3 + Duration::from_millis(500), "with a state {:?}, without {:?}", with_state, plain);
}

// ---- FEEDBACK78 (Q-1158, Q-1159): kept apart from FEEDBACK75's above ---------------------------

/// The request log writes `[hidden]` for the values of query parameters whose names mark a secret,
/// as words of a longer name too, and percent-encoded; others, empty values and URLs with no query
/// as they were; the desktop window's launch token as before (FEEDBACK78.md, Q-1158; branch
/// feedback75)
#[test]
fn the_log_hides_secrets_in_a_query() {
    use quillo::server::log_url;
    for (url, logged) in [
        ("/a", "/a"),
        ("/a?", "/a?"),
        ("/a?page=2&q=ünï", "/a?page=2&q=ünï"),
        ("/sign-in?user=nobody&password=guess", "/sign-in?user=nobody&password=[hidden]"),
        ("/cb?code=abc&state=xyz&scope=openid", "/cb?code=[hidden]&state=[hidden]&scope=openid"),
        ("/r?token=t1&access_token=t2&refresh-token=t3&idToken=t4&user.password=p", "/r?token=[hidden]&access_token=[hidden]&refresh-token=[hidden]&idToken=[hidden]&user.password=[hidden]"),
        ("/s?SAMLRequest=x&SAMLResponse=y&RelayState=z&ticket=t", "/s?SAMLRequest=[hidden]&SAMLResponse=[hidden]&RelayState=[hidden]&ticket=[hidden]"),
        ("/k?key=1&api_key=2&apiKey=3&client_secret=4&otp=5&pass=6&pwd=7&passwd=8", "/k?key=[hidden]&api_key=[hidden]&apiKey=[hidden]&client_secret=[hidden]&otp=[hidden]&pass=[hidden]&pwd=[hidden]&passwd=[hidden]"),
        ("/e?p%61ssword=x&PASSWORD=y&password=&token", "/e?p%61ssword=[hidden]&PASSWORD=[hidden]&password=&token"),
        ("/l?_quillo_launch=abc&x=1", "/l?_quillo_launch=[hidden]&x=1"),
        // `monkey` ends with `key`, as `apikey` does: hidden since the review's rule for words run
        // together (Q-1158, review), where it was written out.
        ("/n?keyword=a&passage=b&monkey=c", "/n?keyword=a&passage=b&monkey=[hidden]"),
    ] {
        assert_eq!(log_url(url), logged, "{}", url);
    }
}

/// The log hides a secret's value whatever the case of its name (`API_KEY`), in brackets
/// (`user[password]`), with its words run together (`apikey`), or with a word that marks one
/// anywhere in it (`id_token_hint`, `code_verifier`), and signatures and credentials (`X-Amz-
/// Signature`, `sig`, `jwt`, `auth`); names that only look alike stay (FEEDBACK78.md, Q-1158, review;
/// branch feedback75)
#[test]
fn the_log_hides_secrets_named_any_way() {
    use quillo::server::log_url;
    for name in [
        "API_KEY", "ACCESS_TOKEN", "CLIENT_SECRET", "apikey", "accesstoken", "clientsecret", "user%5Bpassword%5D", "user[password]", "id_token_hint", "code_verifier", "X-Amz-Signature",
        "X-Amz-Credential", "sig", "signature", "jwt", "auth", "APIKey", "newPassword", "password_confirm", "oauth_token", "RelayState", "x.Credentials", "AUTH", "Api-Key",
    ] {
        assert_eq!(log_url(&format!("/cb?page=2&{}=s3cr3t&q=ünï", name)), format!("/cb?page=2&{}=[hidden]&q=ünï", name), "{}", name);
    }
    for name in ["page", "q", "keyword", "passage", "author", "authority", "sort_by", "tokens_left", "signed", "id", "user[name]", "PAGE_SIZE", "statement"] {
        let url = format!("/cb?{}=v", name);
        assert_eq!(log_url(&url), url, "{}", name);
    }
}

/// The request log, as a server writes it: a password in the query is `[hidden]` (FEEDBACK78.md,
/// Q-1158; branch feedback75)
#[test]
fn a_servers_log_hides_a_password_in_the_query() {
    let d = dir("log-hidden", &[("main.qwe", "route POST /sign-in ? { user: str, password: str } = \"ok\"\n")]);
    let _gone = Gone(d.clone());
    checked(&d, "main.qwe");
    let (_g, port) = serve(&d, "main.qwe", &["--log", "text"], &[], "log.log");
    assert_eq!(support::send(port, "POST", "/sign-in?user=nobody&password=guess", &[], "").status, 200);
    let end = Instant::now() + Duration::from_secs(5);
    let mut log = String::new();
    while !log.contains("/sign-in") && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
        log = std::fs::read_to_string(d.join("log.log")).unwrap_or_default();
    }
    assert!(log.contains("POST /sign-in?user=nobody&password=[hidden] -> 200") && !log.contains("guess"), "{}", log);
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// `__t`'s value in Node, with `prelude` run first.
fn in_node(prelude: &str, src: &str) -> String {
    let mut it = Interp::new(".");
    it.load_source(&format!("{}\nfn __t() {{\n{}\n}}\n", src, "return run()"), "case.qwe").unwrap_or_else(|e| panic!("{}", e));
    let js = format!("{}\n{}", prelude, quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}", e)));
    let out = support::node(&js).expect("node");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A page's object made of getters with a `toJSON` (a passkey's credential, a push subscription, a
/// position) reaches a server fn as its `toJSON` gives it, where it arrived as `{}`; `json.stringify`
/// writes it so too; one whose `toJSON` gives such an object again is `{}`, not a loop; a `Date` is
/// its text; plain maps, lists and unicode as before (FEEDBACK78.md, Q-1159; branch feedback75)
#[test]
fn a_page_objects_to_json_reaches_a_server_fn() {
    if !node_available() {
        return;
    }
    let prelude = r#"
class Cred { get id() { return "Ab_c"; } get type() { return "public-key"; } toJSON() { return { id: this.id, type: this.type, response: { clientDataJSON: "eyJ9", n: 2 } }; } }
class Loop { toJSON() { return new Loop(); } }
globalThis.mkCred = () => new Cred();
globalThis.mkLoop = () => new Loop();
globalThis.mkDate = () => new Date(Date.UTC(2026, 10, 20, 1, 2, 3));
globalThis.fetch = async (url, o) => ({ ok: true, text: async () => JSON.stringify({ ok: JSON.parse(o.body) }) });
"#;
    let src = "server fn echo(a, b, c, d) = [a, b, c, d]\nfn run() {\n    let cred = js(\"mkCred\")\n    return [echo(cred, js(\"mkLoop\"), js(\"mkDate\"), { k: \"ünï\", l: [1, 2] }), json.stringify(cred)]\n}";
    assert_eq!(
        in_node(prelude, src),
        r#"[[{ id: "Ab_c", type: "public-key", response: { clientDataJSON: "eyJ9", n: 2 } }, {}, "2026-11-20T01:02:03.000Z", { k: "ünï", l: [1, 2] }], "{\"id\":\"Ab_c\",\"type\":\"public-key\",\"response\":{\"clientDataJSON\":\"eyJ9\",\"n\":2}}"]"#
    );
}
