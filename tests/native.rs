//! Rust in a program: `.qwer` files (src/native.rs). These build with the
//! `cargo` running the tests, into a cache of their own.

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn cache() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache")
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-native-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start(file: &Path, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Server {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(extra)
        .env("QUILLO_NATIVE_CACHE", cache())
        .envs(env.iter().copied())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cannot start quillo");
    support::stop_with_tests(&child);
    let mut server = Server(child);
    // The first build of the cache's dependencies takes a while.
    let deadline = Instant::now() + Duration::from_secs(300);
    while !support::listening(port) {
        if let Ok(Some(_)) = server.0.try_wait() {
            let mut err = String::new();
            let _ = server.0.stderr.take().map(|mut e| e.read_to_string(&mut err));
            panic!("the server stopped:\n{}", err);
        }
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    // What it prints later is not read; it must not fill the pipe.
    if let Some(mut e) = server.0.stderr.take() {
        std::thread::spawn(move || std::io::copy(&mut e, &mut std::io::sink()));
    }
    server
}

fn get(port: u16, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", path).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
}

fn eventually<T>(secs: u64, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "{}", what);
        std::thread::sleep(Duration::from_millis(100));
    }
}

const MATHS: &str = r#"//! Number work, in Rust.

use std::collections::HashMap;

/// The primes below `n`.
pub fn primes_below(n: i64) -> Vec<i64> {
    let n = n.max(0) as usize;
    let mut sieve = vec![true; n];
    let mut out = Vec::new();
    for i in 2..n {
        if sieve[i] {
            out.push(i as i64);
            let mut j = i * i;
            while j < n { sieve[j] = false; j += i; }
        }
    }
    out
}

pub fn greet(name: &str) -> String { format!("Hello, {}!", name) }

pub fn summary(xs: &[f64]) -> HashMap<String, f64> {
    let n = xs.len() as f64;
    HashMap::from([("n".to_string(), n), ("mean".to_string(), xs.iter().sum::<f64>() / n)])
}

pub fn ratio(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 { Err("cannot divide by zero".into()) } else { Ok(a / b) }
}

pub fn first_word(s: Option<String>) -> Option<String> {
    s.and_then(|s| s.split_whitespace().next().map(str::to_string))
}

pub fn echo(v: serde_json::Value) -> serde_json::Value { v }

pub fn boom() -> i64 { panic!("it broke") }

pub fn slow(ms: i64) -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    ms
}
"#;

const MAIN: &str = r#"use "maths.qwer"

route GET /primes/:n(int) = primes_below(n)
route GET /greet/:name = greet(name)
route GET /summary = summary([1.0, 2.0, 4.5])
route GET /ratio/:b(int) = ratio(1.0, float(b)) ?? "no ratio"
route GET /word = { a: first_word("  hello world"), b: first_word(null) }
route GET /echo = echo({ list: [1, 2.5, "x", true, null], nested: { k: "v" } })
route GET /wrong = greet(json.parse("42"))
route GET /boom = boom()
route GET /after = "still here"
route GET /slow = str(slow(1500))
route GET /fast = "fast"
route GET /shown = str(greet)
"#;

#[test]
fn rust_functions_take_and_give_quillo_values() {
    let d = dir("values");
    std::fs::write(d.join("maths.qwer"), MATHS).unwrap();
    std::fs::write(d.join("main.qwe"), MAIN).unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[], &[]);
    assert_eq!(get(port, "/primes/30").1, "[2,3,5,7,11,13,17,19,23,29]");
    assert_eq!(get(port, "/greet/Ana").1, "Hello, Ana!");
    let s: serde_json::Value = serde_json::from_str(&get(port, "/summary").1).unwrap();
    assert_eq!((s["n"].as_f64(), s["mean"].as_f64()), (Some(3.0), Some(2.5)));
    assert_eq!(get(port, "/ratio/4").1, "0.25");
    // An `Err` is an error value: `??` takes the other side.
    assert_eq!(get(port, "/ratio/0").1, "no ratio");
    assert_eq!(get(port, "/word").1, r#"{"a":"hello","b":null}"#);
    assert_eq!(get(port, "/echo").1, r#"{"list":[1,2.5,"x",true,null],"nested":{"k":"v"}}"#);
    // Arguments it cannot take, and a panic: errors in that request alone.
    assert_eq!(get(port, "/wrong").0, 500);
    assert_eq!(get(port, "/boom").0, 500);
    assert_eq!(get(port, "/after").1, "still here");
    assert_eq!(get(port, "/shown").1, "<rust fn greet>");
    // The library is kept next to the file, named by its content.
    let libs: Vec<String> = std::fs::read_dir(d.join(".quillo/native")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(libs.len() == 1 && libs[0].contains("maths-"), "{:?}", libs);
}

#[test]
fn a_slow_rust_call_holds_up_no_one() {
    let d = dir("slow");
    std::fs::write(d.join("maths.qwer"), MATHS).unwrap();
    std::fs::write(d.join("main.qwe"), MAIN).unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[], &[]);
    let t = Instant::now();
    let slow = std::thread::spawn(move || get(port, "/slow").1);
    std::thread::sleep(Duration::from_millis(200));
    let fast = Instant::now();
    assert_eq!(get(port, "/fast").1, "fast");
    assert!(fast.elapsed() < Duration::from_millis(500), "a request waited for the Rust call: {:?}", fast.elapsed());
    // And several run at once, on several threads.
    let more: Vec<_> = (0..3).map(|_| std::thread::spawn(move || get(port, "/slow").1)).collect();
    for m in more {
        assert_eq!(m.join().unwrap(), "1500");
    }
    assert_eq!(slow.join().unwrap(), "1500");
    assert!(t.elapsed() < Duration::from_millis(4000), "the calls ran one after another: {:?}", t.elapsed());
}

#[test]
fn a_qwer_file_has_crates() {
    let d = dir("deps");
    std::fs::write(
        d.join("rand.qwer"),
        "//! [dependencies]\n//! fastrand = \"2\"\n\n/// A number from `lo` up to `hi`, the same each time for a seed.\npub fn seeded(seed: i64, lo: i64, hi: i64) -> i64 {\n    fastrand::Rng::with_seed(seed as u64).i64(lo..hi)\n}\n",
    )
    .unwrap();
    std::fs::write(d.join("main.qwe"), "use \"rand.qwer\"\nroute GET / = str(seeded(7, 0, 1000) == seeded(7, 0, 1000))\n").unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[], &[]);
    assert_eq!(get(port, "/").1, "true");
}

#[test]
fn a_changed_file_is_built_again_and_a_broken_one_changes_nothing() {
    let d = dir("reload");
    let lib = |greeting: &str| format!("pub fn greet(name: &str) -> String {{ format!(\"{}, {{}}!\", name) }}\n", greeting);
    std::fs::write(d.join("hello.qwer"), lib("Hello")).unwrap();
    std::fs::write(d.join("main.qwe"), "use \"hello.qwer\"\nroute GET /:name = greet(name)\n").unwrap();
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &["--reload"], &[]);
    assert_eq!(get(port, "/Ana").1, "Hello, Ana!");
    std::fs::write(d.join("hello.qwer"), lib("Hi")).unwrap();
    eventually(120, "the changed file was not built again", || (get(port, "/Ana").1 == "Hi, Ana!").then_some(()));
    // A change that does not compile: the program goes on as it was.
    std::fs::write(d.join("hello.qwer"), "pub fn greet(name: &str) -> String { name + 1 }\n").unwrap();
    std::thread::sleep(Duration::from_secs(8));
    assert_eq!(get(port, "/Ana").1, "Hi, Ana!");
    std::fs::write(d.join("hello.qwer"), lib("Hey")).unwrap();
    eventually(120, "the fixed file was not built again", || (get(port, "/Ana").1 == "Hey, Ana!").then_some(()));
}

#[test]
fn a_program_built_beforehand_runs_without_cargo() {
    let d = dir("prebuilt");
    std::fs::write(d.join("maths.qwer"), MATHS).unwrap();
    std::fs::write(d.join("main.qwe"), MAIN).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["build"]).arg(d.join("main.qwe")).env("QUILLO_NATIVE_CACHE", cache()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("built "), "{}", String::from_utf8_lossy(&out.stdout));
    let port = free_port();
    let _s = start(&d.join("main.qwe"), port, &[], &[("CARGO", "/nonexistent/cargo")]);
    assert_eq!(get(port, "/greet/Bo").1, "Hello, Bo!");
}

#[test]
fn without_cargo_or_a_library_the_program_says_so() {
    let d = dir("nocargo");
    std::fs::write(d.join("x.qwer"), "pub fn one() -> i64 { 1 }\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"x.qwer\"\nroute GET / = str(one())\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", &free_port().to_string()])
        .env("QUILLO_NATIVE_CACHE", cache())
        .env("CARGO", "/nonexistent/cargo")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("cargo did not run") && err.contains("quillo build"), "{}", err);
}

#[test]
fn the_checker_reads_rust_signatures() {
    use quillo::check::{check_source, Severity};
    let d = dir("check");
    std::fs::write(
        d.join("lib.qwer"),
        "pub struct P;\n/// Adds.\npub fn add(a: i64, b: i64) -> i64 { a + b }\npub fn parse(s: &str) -> Result<f64, String> { s.parse().map_err(|_| \"no\".into()) }\npub fn point() -> P { P }\nfn hidden() {}\n",
    )
    .unwrap();
    let diags = |src: &str| -> Vec<String> {
        check_source(src, &d.join("main.qwe").display().to_string(), &d)
            .diagnostics
            .iter()
            .map(|x| format!("{}{}", if x.severity == Severity::Warning { "warning: " } else { "" }, x.message))
            .collect()
    };
    // The struct one is not exported, and says why.
    let d0 = diags("use \"lib.qwer\"\nlet x = add(1, 2) + parse(\"2.5\") ?? 0.0\nprint(x)");
    assert!(d0.len() == 1 && d0[0].contains("warning: `point` is not exported: `P` cannot cross"), "{:?}", d0);
    let d1 = diags("use \"lib.qwer\"\nprint(add(\"one\", 2))");
    assert!(d1.iter().any(|m| m.contains("add") && m.contains("int")), "{:?}", d1);
    let d2 = diags("use \"lib.qwer\"\nprint(hidden())\nprint(point())");
    assert!(d2.iter().any(|m| m.contains("unknown name `hidden`")) && d2.iter().any(|m| m.contains("unknown name `point`")), "{:?}", d2);
    let d3 = diags("use \"lib.qwer\"\ncomponent C() {\n    <p>{add(1, 2)}</p>\n}\nroute GET / = <C/>");
    assert!(d3.iter().any(|m| m.contains("`add` is Rust (lib.qwer), which runs on the server")), "{:?}", d3);
}
