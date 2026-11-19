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
