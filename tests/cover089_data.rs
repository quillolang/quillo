//! Coverage of 0.8.9 (branch feedback75): the lines of src/value.rs, src/record.rs, src/my.rs and
//! src/db.rs added since 0.8.8 that no test ran. A page's marks read with and without a budget and
//! a duplicate key's room given back (FEEDBACK75.md, Q-1052 and Q-1070); a request's records
//! checked to 512 levels, a union's answers kept, types naming themselves behind others and a deep
//! walk on a new piece of stack (Q-1080); a MySQL transaction outside a request, one whose server is
//! gone, and one that waits 30 s for a connection (FEEDBACK78.md, Q-1170). In-process requests run
//! one at a time (`--max-memory` is the process's); servers and MariaDB are this test's own, on the
//! port the system gives, stopped by the test.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::value::Map;
use quillo::{Interp, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

// ---- helpers ------------------------------------------------------------------------------

/// A folder of the test's, removed when the value goes.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str, files: &[(&str, &str)]) -> Dir {
    let d = support::temp_dir(&format!("cov089-data-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    Dir(d)
}

/// A process with a deadline: its status, stdout and stderr, read as it runs, killed past `secs`.
fn within(mut c: Command, secs: u64) -> (std::process::ExitStatus, String, String) {
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = r.read_to_end(&mut s);
            String::from_utf8_lossy(&s).into_owned()
        })
    };
    let (out, err) = (read(Box::new(child.stdout.take().unwrap())), read(Box::new(child.stderr.take().unwrap())));
    let end = Instant::now() + Duration::from_secs(secs);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{:?} did not end within {} s", c, secs);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    (status, out.join().unwrap(), err.join().unwrap())
}

/// `quillo check` of `file` in `d`: whether it passed, and what it said.
fn check(d: &Path, file: &str) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("check").arg(file).current_dir(d);
    let (status, out, err) = within(c, 120);
    (status.success(), out + &err)
}

fn checked(d: &Path, file: &str) {
    let (ok, said) = check(d, file);
    assert!(ok, "{}", said);
}

/// `quillo run` of `file` in `d`, a program with no routes: what it printed, and said.
fn run_script(d: &Path, file: &str, secs: u64) -> (String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("run").arg(file).current_dir(d).env("QUILLO_SECRET", "test secret for the cov089 data tests");
    let (_, out, err) = within(c, secs);
    (out, err)
}

/// A server fn's call by RPC: status and body.
fn rpc(port: u16, f: &str, body: &str) -> (u16, String) {
    let r = support::send(port, "POST", &format!("/_quillo/rpc/{}", f), &[("content-type", "application/json")], body);
    (r.status, r.body)
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

/// A program loaded in this process, having passed `quillo check`.
fn app(name: &str, src: &str) -> Interp {
    let d = dir(name, &[("main.qwe", src)]);
    checked(&d, "main.qwe");
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send_bytes(it: &mut Interp, url: &str, headers: &[(&str, &str)], body: Vec<u8>) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "POST".into(), url: url.into(), headers: h, body, remote: "127.0.0.1".into() })
}

/// A server fn's call in this process, as JSON.
fn rpc_in(it: &mut Interp, f: &str, args: &str) -> (u16, String) {
    let r = send_bytes(it, &format!("/_quillo/rpc/{}", f), &[("Content-Type", "application/json")], args.as_bytes().to_vec());
    (r.status, r.text())
}

/// A server fn's call in this process with a file (`file0`, "hello"), as a page sends one.
fn rpc_with_file(it: &mut Interp, f: &str, args: &str) -> (u16, String) {
    let body = format!(
        "--BB\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n{}\r\n--BB\r\nContent-Disposition: form-data; name=\"file0\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--BB--\r\n",
        args
    );
    let r = send_bytes(it, &format!("/_quillo/rpc/{}", f), &[("Content-Type", "multipart/form-data; boundary=BB"), ("x-quillo-rpc", "1")], body.into_bytes());
    (r.status, r.text())
}

fn json(text: &str) -> Value {
    Value::parse_json(text).unwrap()
}

// ---- value.rs: a page's marks (Q-1052) ----------------------------------------------------

const ECHO: &str = "server fn echo(x) -> str {\n    return `${type_of(x)} ${x}`\n}\nserver fn size(f: map) -> int {\n    return f.size\n}\n";

/// `value::island_marks`, the library's reading of a page's marks with no budget: a decimal, an
/// int past 2^53, a map of one `$` key, an error value, a mark inside a map, unicode kept; a file's
/// mark is the upload given `files` and nothing without them; a value with no mark is the value
/// given, shared, not copied; where `island_marks_held` with no room left stops, it reads on, as
/// with all the room there is (src/value.rs `island_marks`, FEEDBACK75.md Q-1052; coverage of
/// 0.8.9; branch feedback75).
#[test]
fn island_marks_reads_a_pages_marks_with_no_budget() {
    let _one = serial();
    use quillo::value::{island_marks, island_marks_held};
    let v = json(r#"[{"$dec": "12.50"}, {"$int": "9007199254740993"}, {"$": ["$dec", 1]}, {"k": {"$int": "7"}}, "ünï 😀", {"$quillo_err": "no"}]"#);
    let out = island_marks(&v, None);
    let Value::List(items) = &out else { panic!("{}", out.repr()) };
    assert_eq!(items.len(), 6);
    assert_eq!((items[0].type_name(), items[0].display()), ("dec", "12.50".to_string()));
    assert_eq!((items[1].type_name(), items[1].display()), ("int", "9007199254740993".to_string()));
    assert_eq!(items[2].repr(), r#"{ "$dec": 1 }"#);
    assert_eq!(items[3].repr(), "{ k: 7 }");
    assert_eq!(items[4].display(), "ünï 😀");
    assert!(matches!(items[5], Value::Err(..)), "{}", items[5].repr());
    // The same as `island_marks_held` with all the room there is.
    assert_eq!(island_marks_held(&v, None, usize::MAX).unwrap().repr(), out.repr());
    // With no room, the held reading stops; this one has no budget.
    assert!(island_marks_held(&v, None, 0).is_err());
    // A file's mark: the upload given files, nothing past them or without them.
    let mut files = Map::new();
    files.insert("file0".into(), Value::str("hello"));
    let f = json(r#"[{"$quillo_file": 0}, {"$quillo_file": 1}, {"$quillo_file": -1}]"#);
    assert_eq!(island_marks(&f, Some(&files)).repr(), r#"["hello", null, null]"#);
    assert_eq!(island_marks(&f, None).repr(), r#"[{ "$quillo_file": 0 }, { "$quillo_file": 1 }, { "$quillo_file": -1 }]"#);
    // No mark: the value given, not a copy; empty ones too.
    for plain in [r#"[1, "a", {"b": [2.5]}]"#, "[]", "{}", r#""é""#, "null"] {
        let p = json(plain);
        let q = island_marks(&p, None);
        assert_eq!(q.repr(), p.repr());
        if let (Value::List(a), Value::List(b)) = (&p, &q) {
            assert!(Arc::ptr_eq(a, b), "{} was copied", plain);
        }
    }
}

/// In a call with files, a `$dec` or `$int` mark whose value is no text is a map of that key, its
/// value read for marks as 0.8.8 read it (with no files): `{"$dec": {"$int": "7"}}` is
/// `{ "$dec": 7 }`, a list inside keeps its decimal and unicode, a file's mark inside stays a mark;
/// the same answers as the call without files. The map it makes takes the room a one-key map
/// takes, as `{"$": [k, x]}`'s does: with one byte less, the reading stops (src/value.rs
/// `marks_in`, FEEDBACK75.md Q-1052's review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_marks_other_value_in_a_call_with_files_is_a_map_of_its_key() {
    let _one = serial();
    let mut it = app("files", ECHO);
    let ok = |s: &str| (200, format!(r#"{{"ok":{}}}"#, serde_json::Value::String(s.to_string())));
    for (args, want) in [
        (r#"[{"$dec": {"$int": "7"}}]"#, r#"map { "$dec": 7 }"#),
        (r#"[{"$int": [{"$dec": "1.5"}, "ü"]}]"#, r#"map { "$int": [1.5m, "ü"] }"#),
        (r#"[{"$dec": {"$": ["k", {"$int": "12"}]}}]"#, r#"map { "$dec": { k: 12 } }"#),
    ] {
        assert_eq!(rpc_with_file(&mut it, "echo", args), ok(want), "{}", args);
        assert_eq!(rpc_in(&mut it, "echo", args), ok(want), "{} without files", args);
    }
    // The file is still the file, and not inside such a mark.
    assert_eq!(rpc_with_file(&mut it, "size", r#"[{"$quillo_file": 0}]"#), (200, r#"{"ok":5}"#.to_string()));
    let (status, body) = rpc_with_file(&mut it, "echo", r#"[{"$int": [{"$quillo_file": 0}, {"$int": "3"}]}]"#);
    assert_eq!(status, 200);
    assert!(body.contains("$quillo_file") && body.contains(", 3]") && !body.contains("hello"), "{}", body);
    // The room: the least that reads `{"$dec": {"$int": "7"}}` with files, and one byte less.
    use quillo::value::island_marks_held;
    let mut files = Map::new();
    files.insert("file0".into(), Value::str("hello"));
    let least = |v: &Value| (0..1 << 16).find(|r| island_marks_held(v, Some(&files), *r).is_ok()).unwrap();
    let mark = json(r#"{"$dec": {"$int": "7"}}"#);
    let need = least(&mark);
    assert!(need > 0);
    assert!(island_marks_held(&mark, Some(&files), need - 1).is_err());
    assert_eq!(island_marks_held(&mark, Some(&files), need).unwrap().repr(), r#"{ "$dec": 7 }"#);
    assert_eq!(need, least(&json(r#"{"$": ["k", 7]}"#)), "a one-key map takes the same room either way");
    // No mark inside: nothing made, no room needed.
    assert_eq!(least(&json(r#"{"$dec": [1, 2]}"#)), 0);
}

/// Under `--max-memory`, a duplicate key read when a visitor's map is full is charged for the
/// table it would grow to, and given that room back when it replaces a value and nothing grows: a
/// map with a key given twice counts what the map without the first value counts, at 3 entries,
/// at 7 and with unicode keys, and the call answers as before (src/value.rs `settle`,
/// FEEDBACK75.md Q-1070's second review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_duplicate_key_gives_back_the_room_it_was_charged_for() {
    let _one = serial();
    let mut it = app("dup", ECHO);
    let _limit = Limit::set(8 << 20);
    let made = |text: &str| Value::parse_json_body(text, true).unwrap().unwrap().1;
    for (twice, once) in [
        (r#"{"a":1,"b":2,"c":3,"a":4}"#, r#"{"a":4,"b":2,"c":3}"#),
        (r#"{"k0":0,"k1":1,"k2":2,"k3":3,"k4":4,"k5":5,"k6":6,"k3":9}"#, r#"{"k0":0,"k1":1,"k2":2,"k3":9,"k4":4,"k5":5,"k6":6}"#),
        (r#"{"é":1,"ß":2,"😀":3,"é":4}"#, r#"{"é":4,"ß":2,"😀":3}"#),
    ] {
        assert!(made(once) > 0);
        assert_eq!(made(twice), made(once), "{}", twice);
        assert_eq!(Value::parse_json_body(twice, true).unwrap().unwrap().0.repr(), json(once).repr());
    }
    // A key given twice before the map is full is charged nothing more, as before.
    assert_eq!(made(r#"{"a":1,"a":2}"#), made(r#"{"a":2}"#));
    assert_eq!(rpc_in(&mut it, "echo", r#"[{"a":1,"b":2,"c":3,"a":4}]"#), (200, r#"{"ok":"map { a: 4, b: 2, c: 3 }"}"#.to_string()));
}

// ---- record.rs: a request's records (Q-1080) ----------------------------------------------

const RECORDS: &str = r#"type Account = { name: str, nick: str?, role: "member" | "viewer" = "viewer" }
type Grid = int | list<list<Grid>>
type Shape = int | { a: { b: Shape } }
type Pair = Left | Right | int
type Left = list<Pair>
type Right = list<Pair>
type Tree = { v: int, kids: list<Ka> = [] }
type Ka = Kb
type Kb = Kc
type Kc = Kd
type Kd = Ke
type Ke = Kf
type Kf = Kg
type Kg = Tree
server fn holder(x: { owner: Account }) {
    return x
}
server fn maybe(xs: list<int?>) {
    return len(xs)
}
server fn grid(g: Grid) {
    return 1
}
server fn shape(s: Shape) {
    return 1
}
server fn pair(p: Pair) {
    return 1
}
server fn tree(t: Tree) {
    return t.v
}
route GET / = "ok"
"#;

/// `[...[leaf]...]`, `n` lists deep, as the one argument of a call.
fn lists(n: usize, leaf: &str) -> String {
    format!("[{}{}{}]", "[".repeat(n), leaf, "]".repeat(n))
}

/// `{"a":{"b": ...}}` `n` times around `leaf`, as the one argument of a call.
fn shapes(n: usize, leaf: &str) -> String {
    let mut s = leaf.to_string();
    for _ in 0..n {
        s = format!("{{\"a\":{{\"b\":{}}}}}", s);
    }
    format!("[{}]", s)
}

/// A `Tree` of `n` nodes around `leaf`, each `{"v": i, "kids": [<the one below>]}`.
fn trees(n: usize, leaf: &str) -> String {
    let mut s = leaf.to_string();
    for i in 0..n {
        s = format!("{{\"v\":{},\"kids\":[{}]}}", i + 1, s);
    }
    format!("[{}]", s)
}

/// A record inside a map shape (`x: { owner: Account }`) is checked by the request's walk field by
/// field: a right one taken with unicode kept and its optional field left out or given, a value
/// outside its set, a field missing and a text where the record goes refused with the field's
/// path (src/record.rs `req_fits_name`, FEEDBACK75.md Q-1080; coverage of 0.8.9; branch feedback75).
#[test]
fn a_record_inside_a_map_shape_is_checked_field_by_field() {
    let d = dir("records", &[("main.qwe", RECORDS)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(
        rpc(s.port, "holder", r#"[{"owner":{"name":"Zoë 😀","role":"member"}}]"#),
        (200, r#"{"ok":{"owner":{"name":"Zoë 😀","role":"member"}}}"#.to_string())
    );
    assert_eq!(
        rpc(s.port, "holder", r#"[{"owner":{"name":"ada","nick":"a","role":"viewer"}}]"#),
        (200, r#"{"ok":{"owner":{"name":"ada","nick":"a","role":"viewer"}}}"#.to_string())
    );
    for (args, why) in [
        (r#"[{"owner":{"name":"ada","role":"root"}}]"#, r#"(`owner.role` must be \"member\" | \"viewer\", got str \"root\")"#),
        (r#"[{"owner":{"role":"member"}}]"#, "(`owner.name` is missing)"),
        (r#"[{"owner":{"name":5,"role":"member"}}]"#, "(`owner.name` must be str, got int 5)"),
        (r#"[{"owner":"ada"}]"#, r#"(`owner` must be Account, got str \"ada\")"#),
        (r#"[{"owner":{"name":"ada","nick":[],"role":"member"}}]"#, "(`owner.nick` must be str?, got list [])"),
    ] {
        let (status, body) = rpc(s.port, "holder", args);
        assert_eq!(status, 400, "{}: {}", args, body);
        assert!(body.contains("argument `x` of `holder` must be { owner: Account }, got map ") && body.contains(why), "{}: {}", args, body);
    }
}

/// A list of an optional type takes `null` items (`list<int?>`), and still refuses an item of
/// another type; empty and one-item lists too (src/record.rs `req_fits_at`, FEEDBACK75.md Q-1080;
/// coverage of 0.8.9; branch feedback75).
#[test]
fn a_list_of_an_optional_type_takes_null_items() {
    let d = dir("optional", &[("main.qwe", RECORDS)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(rpc(s.port, "maybe", "[[1,null,3]]"), (200, r#"{"ok":3}"#.to_string()));
    assert_eq!(rpc(s.port, "maybe", "[[null]]"), (200, r#"{"ok":1}"#.to_string()));
    assert_eq!(rpc(s.port, "maybe", "[[]]"), (200, r#"{"ok":0}"#.to_string()));
    assert_eq!(
        rpc(s.port, "maybe", r#"[[1,"x"]]"#),
        (400, r#"{"error":"argument `xs` of `maybe` must be list<int?>, got list (`[1]` must be int?, got str \"x\")"}"#.to_string())
    );
}

/// Lists and map shapes nested past 32 levels with no record between (`Grid = int |
/// list<list<Grid>>`, `Shape = int | { a: { b: Shape } }`) go on as the deep walk where a list or a
/// map shape, not a type's name, comes at the 32nd level: a right one 40 lists or 40 maps deep and
/// one as deep as JSON lets in are taken, a wrong leaf and a number where a list goes refused with
/// the way to them (src/record.rs `req_fits_name`, FEEDBACK75.md Q-1080; coverage of 0.8.9; branch
/// feedback75).
#[test]
fn lists_and_map_shapes_past_32_levels_are_checked_to_their_end() {
    let d = dir("deep-shapes", &[("main.qwe", RECORDS)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for n in [30, 32, 34, 40, 126] {
        assert_eq!(rpc(s.port, "grid", &lists(n, "7")), (200, r#"{"ok":1}"#.to_string()), "{} lists", n);
    }
    let (status, body) = rpc(s.port, "grid", &lists(40, r#""ẍ""#));
    assert_eq!(status, 400, "{}", body);
    assert!(body.contains(&format!("(`{}` must be Grid, got str \\\"ẍ\\\")", "[0]".repeat(40))), "{}", body);
    let (status, body) = rpc(s.port, "grid", &lists(41, "7"));
    assert_eq!(status, 400, "{}", body);
    assert!(body.contains(&format!("(`{}` must be list<Grid>, got int 7)", "[0]".repeat(41))), "{}", body);
    for n in [15, 16, 17, 20, 63] {
        assert_eq!(rpc(s.port, "shape", &shapes(n, "1")), (200, r#"{"ok":1}"#.to_string()), "{} shapes", n);
    }
    let (status, body) = rpc(s.port, "shape", &shapes(20, r#""x""#));
    assert_eq!(status, 400, "{}", body);
    assert!(body.contains(&format!("(`{}` must be Shape, got str \\\"x\\\")", vec!["a.b"; 20].join("."))), "{}", body);
    let (status, body) = rpc(s.port, "shape", &shapes(20, r#"{"a":5}"#));
    assert_eq!(status, 400, "{}", body);
}

/// A union of two types that name the same list (`Pair = Left | Right | int`, `Left` and `Right`
/// each `list<Pair>`) is tried once for each list and level: where `Left` failed below, `Right`
/// takes the answer kept for the same list, so 60 lists deep with a wrong leaf, 2^60 ways
/// otherwise, is refused at once, and a right one taken (src/record.rs `req_fits_union`,
/// FEEDBACK75.md Q-1080's review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_unions_answer_for_a_list_is_kept_for_the_other_way() {
    let d = dir("pair", &[("main.qwe", RECORDS)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for n in [8, 9, 20, 60, 126] {
        let t = Instant::now();
        assert_eq!(rpc(s.port, "pair", &lists(n, "7")), (200, r#"{"ok":1}"#.to_string()), "{} lists", n);
        let (status, body) = rpc(s.port, "pair", &lists(n, r#""z""#));
        assert_eq!(status, 400, "{}", body);
        // A long way in is told by its innermost end.
        assert!(body.contains(&format!("{}` must be Pair, got str \\\"z\\\")", "[0]".repeat(n.min(60)))), "{} lists: {}", n, body);
        assert_eq!(body.contains("(`…"), n > 60, "{} lists: {}", n, body);
        assert!(t.elapsed() < Duration::from_secs(20), "{} lists took {:?}", n, t.elapsed());
    }
}

/// A record reached through seven types naming types at each level (`Tree`'s kids are `Ka`, `Ka`
/// is `Kb`, ... `Kg` is `Tree`) counts each as a level of the request's 512: 56 nodes and a leaf
/// are taken; one node more is refused as nested more than 512 deep, the
/// message's path cut short, before its wrong leaf is looked at (src/record.rs `req_make_at`,
/// FEEDBACK75.md Q-1080; coverage of 0.8.9; branch feedback75).
#[test]
fn a_record_past_512_levels_through_types_naming_types_is_refused() {
    let d = dir("tree", &[("main.qwe", RECORDS)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    for n in [0, 1, 55, 56] {
        assert_eq!(rpc(s.port, "tree", &trees(n, r#"{"v":0}"#)), (200, format!(r#"{{"ok":{}}}"#, n)), "{} nodes", n);
    }
    for (n, leaf) in [(57, r#"{"v":0}"#), (58, r#"{"v":0}"#), (57, r#"{"v":"x"}"#)] {
        let (status, body) = rpc(s.port, "tree", &trees(n, leaf));
        assert_eq!(status, 400, "{} nodes: {}", n, body);
        assert!(body.contains("argument `t` of `tree` must be Tree, got map (`…") && body.ends_with("` is nested more than 512 deep)\"}"), "{} nodes: {}", n, body);
        assert!(body.len() < 600, "{}", body.len());
    }
    let (status, body) = rpc(s.port, "tree", &trees(30, r#"{"v":"x"}"#));
    assert_eq!(status, 400);
    assert!(body.contains(".v` must be int, got str \\\"x\\\")"), "{}", body);
}

/// A type that names itself (`Ab = Ab | int`) reached through another (`Wrap = Ab | bool`), in a
/// program run without `quillo check` (which refuses it): the request's walk refuses `Ab` named
/// again after `Wrap` at once and takes what `int` or `bool` takes, alone and in a list (src/record.rs
/// `Chain for &Named`, FEEDBACK75.md Q-1080's second review; coverage of 0.8.9; branch feedback75).
#[test]
fn a_type_naming_itself_behind_another_is_refused_at_once() {
    let src = "type Wrap = Ab | bool\ntype Ab = Ab | int\nserver fn wrap(w: Wrap) {\n    return w\n}\nserver fn many(ws: list<Wrap>) {\n    return len(ws)\n}\nroute GET / = \"ok\"\n";
    let d = dir("self-naming", &[("main.qwe", src)]);
    let (ok, said) = check(&d, "main.qwe");
    assert!(!ok && said.contains("type Ab names itself with nothing between"), "{}", said);
    let s = support::serve_unchecked(&d.join("main.qwe"));
    assert_eq!(rpc(s.port, "wrap", "[3]"), (200, r#"{"ok":3}"#.to_string()));
    assert_eq!(rpc(s.port, "wrap", "[true]"), (200, r#"{"ok":true}"#.to_string()));
    for (args, got) in [(r#"["s"]"#, "str"), ("[[1]]", "list"), ("[null]", "null"), ("[1.5]", "float")] {
        let t = Instant::now();
        assert_eq!(rpc(s.port, "wrap", args), (400, format!(r#"{{"error":"argument `w` of `wrap` must be Wrap, got {}"}}"#, got)), "{}", args);
        assert!(t.elapsed() < Duration::from_secs(10), "{}: {:?}", args, t.elapsed());
    }
    assert_eq!(rpc(s.port, "many", "[[1,true,2]]"), (200, r#"{"ok":3}"#.to_string()));
    assert_eq!(rpc(s.port, "many", "[[]]"), (200, r#"{"ok":0}"#.to_string()));
    assert_eq!(
        rpc(s.port, "many", r#"[[1,"x"]]"#),
        (400, r#"{"error":"argument `ws` of `many` must be list<Wrap>, got list (`[1]` must be Wrap, got str \"x\")"}"#.to_string())
    );
}

/// A service in the same process makes a call's arguments on the caller's stack, however deep the
/// caller's own recursion: from 9,900 levels of it down to none, a record 41 nodes deep and lists
/// 60 deep are made or refused the same at every level, the deep walk going on on a new piece of
/// stack where this one runs low, and the wrong leaf named with its way (src/record.rs
/// `req_fits_at` and `req_make_at`, FEEDBACK75.md Q-1080; coverage of 0.8.9; branch feedback75).
#[test]
fn a_services_deep_arguments_are_made_wherever_the_callers_stack_is() {
    let service = r#"type Node = { v: int, kids: list<Node> = [] }
type Lists = int | list<list<Lists>>
server fn nodes(n: Node) -> int {
    return n.v
}
server fn lists(l: Lists) -> int {
    return 1
}
"#;
    // In a debug build the recursive call nests 40 levels deep in its expression, so that each
    // call keeps room on the stack for 40 levels (24 KB a level there), more than the walk's first
    // 32 levels, which do not look, take: with the 128 KB a call nesting less keeps, a debug
    // build's walk of a record 8 nodes deep from deep in a recursion ran off the stack (0.8.8's
    // walk of a program's own record too, at 20 nodes). An optimized build's walk takes far less:
    // there the call nests as written, and the deep walk finds this stack low.
    let pad = if cfg!(debug_assertions) { 40 } else { 0 };
    let main = format!(
        r#"service deep = "deep.qwe"
fn chain(n: int, leaf) {{
    if n == 0 {{
        return {{ v: leaf, kids: [] }}
    }}
    return {{ v: n, kids: [chain(n - 1, leaf)] }}
}}
fn nest(n: int, leaf) {{
    if n == 0 {{
        return leaf
    }}
    return [[nest(n - 1, leaf)]]
}}
let good = chain(40, 0)
let bad = chain(40, "x")
let fine = nest(30, 7)
let wrong = nest(30, "y")
fn down(n: int) -> int {{
    let a = deep.nodes(good)
    let b = deep.lists(fine)
    let c = deep.nodes(bad)
    let d = deep.lists(wrong)
    let here = if a == 40 && b == 1 && is_err(c) && is_err(d) {{ 1 }} else {{ 0 }}
    if n == 0 {{
        return here
    }}
    return here + {open}down(n - 1){close}
}}
print(down(9900))
print(error(deep.nodes(bad)))
print(error(deep.lists(wrong)))
"#,
        open = "(".repeat(pad),
        close = ")".repeat(pad)
    );
    let d = dir("service-stack", &[("main.qwe", &main), ("deep.qwe", service)]);
    checked(&d, "main.qwe");
    let (out, err) = run_script(&d, "main.qwe", 600);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{}\n{}", out, err);
    assert_eq!(lines[0], "9901", "every level made and refused the same\n{}", err);
    assert!(lines[1].starts_with("service deep: argument `n` of `nodes` must be Node, got map (`…") && lines[1].ends_with(".kids[0].v` must be int, got str \"x\")"), "{}", lines[1]);
    assert_eq!(lines[2], format!("service deep: argument `l` of `lists` must be Lists, got list (`{}` must be Lists, got str \"y\")", "[0]".repeat(60)));
}

// ---- Q-1171: a record in a map shape, or behind types naming types, is made ----------------

const ACCOUNT: &str = "type Account = { name: str, nick: str?, role: \"member\" | \"viewer\" = \"viewer\" }\n";

/// `n` types naming types in a row, `<P>a` to the last, which is `Account` (`Sa = Sb`, ...,
/// `Sg = Account`); the first's name.
fn names_in_a_row(p: char, n: usize) -> (String, String) {
    names_to(p, n, "Account")
}

/// `n` types naming types in a row, `<P>a` to the last, which is `to`; the first's name.
fn names_to(p: char, n: usize, to: &str) -> (String, String) {
    let name = |i: usize| format!("{}{}", p, (b'a' + i as u8) as char);
    let mut s = String::new();
    for i in 0..n {
        let next = if i + 1 == n { to.to_string() } else { name(i + 1) };
        s.push_str(&format!("type {} = {}\n", name(i), next));
    }
    (s, name(0))
}

/// The program of Q-1171's tests: a record in map shapes, a list of them, behind 7, 8, 9 and 20
/// types naming types, and a route's `{ json: { owner: Account } }` beside them.
fn made_program() -> String {
    let mut src = ACCOUNT.to_string();
    let mut fns = String::new();
    for (p, n, f) in [('S', 7, "seven"), ('E', 8, "eight"), ('N', 9, "nine"), ('Z', 20, "twenty")] {
        let (types, first) = names_in_a_row(p, n);
        src.push_str(&types);
        fns.push_str(&format!("server fn {}(xs: list<{}>) {{\n    return xs\n}}\n", f, first));
    }
    src.push_str(&fns);
    src.push_str(
        r#"server fn direct(a: Account) {
    return a
}
server fn named(a: Za) {
    return a
}
server fn shaped(x: { owner: Account }) {
    return x
}
server fn nested(x: { a: { owner: Account } }) {
    return x
}
server fn maybe(x: { owner: Account? }) {
    return x
}
server fn mixed(x: { owner: Account, n: int, tags: list<str> }) {
    return x
}
server fn shapes(xs: list<{ owner: Account }>) {
    return xs
}
route POST /shaped { json: { owner: Account } } = { owner: owner }
"#,
    );
    src
}

/// A server fn's record inside a map shape (`x: { owner: Account }`, nested in another, optional,
/// beside other fields, in a list of shapes) is made as `direct(a: Account)` makes one and as a
/// route's `{ json: { owner: Account } }` does: a field it does not have (`is_admin`) left out, a
/// default put in, unicode kept; the shape's other fields checked and kept as they are, what else
/// the map holds kept, as a shape keeps it; what is missing or wrong refused as before. It was
/// checked, not made: `is_admin` reached the function and a defaulted field left out was refused
/// (src/record.rs `req_make_name` and `req_make_shape`, FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1171; branch feedback75).
#[test]
fn q1171_a_record_in_a_map_shape_is_made_as_a_parameter_of_its_type() {
    let src = made_program();
    let d = dir("q1171-shape", &[("main.qwe", &src)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let ok = |body: &str| (200, format!(r#"{{"ok":{}}}"#, body));
    assert_eq!(rpc(s.port, "direct", r#"[{"name":"ada","is_admin":true}]"#), ok(r#"{"name":"ada","role":"viewer"}"#));
    for (f, args, want) in [
        ("shaped", r#"[{"owner":{"name":"ada","role":"member","is_admin":true}}]"#, r#"{"owner":{"name":"ada","role":"member"}}"#),
        ("shaped", r#"[{"owner":{"name":"Zoë 😀"}}]"#, r#"{"owner":{"name":"Zoë 😀","role":"viewer"}}"#),
        ("shaped", r#"[{"owner":{"name":"ada","nick":null},"note":"ü"}]"#, r#"{"owner":{"name":"ada","role":"viewer"},"note":"ü"}"#),
        ("nested", r#"[{"a":{"owner":{"name":"ada","is_admin":1}}}]"#, r#"{"a":{"owner":{"name":"ada","role":"viewer"}}}"#),
        ("maybe", r#"[{"owner":{"name":"ada","is_admin":true}}]"#, r#"{"owner":{"name":"ada","role":"viewer"}}"#),
        ("maybe", r#"[{"owner":null}]"#, r#"{"owner":null}"#),
        ("maybe", "[{}]", "{}"),
        ("mixed", r#"[{"owner":{"name":"a","x":1},"n":2,"tags":["t"],"z":[0]}]"#, r#"{"owner":{"name":"a","role":"viewer"},"n":2,"tags":["t"],"z":[0]}"#),
        ("shapes", r#"[[{"owner":{"name":"a","x":1}},{"owner":{"name":"b","role":"member"}}]]"#, r#"[{"owner":{"name":"a","role":"viewer"}},{"owner":{"name":"b","role":"member"}}]"#),
        ("shapes", "[[]]", "[]"),
    ] {
        assert_eq!(rpc(s.port, f, args), ok(want), "{} {}", f, args);
    }
    // The route's pattern makes the same.
    let r = support::send(s.port, "POST", "/shaped", &[("content-type", "application/json")], r#"{"owner":{"name":"ada","role":"member","is_admin":true}}"#);
    assert_eq!((r.status, r.body.as_str()), (200, r#"{"owner":{"name":"ada","role":"member"}}"#));
    let r = support::send(s.port, "POST", "/shaped", &[("content-type", "application/json")], r#"{"owner":{"name":"Zoë 😀"}}"#);
    assert_eq!((r.status, r.body.as_str()), (200, r#"{"owner":{"name":"Zoë 😀","role":"viewer"}}"#));
    // Refused as before, with the way to what is wrong.
    for (f, args, why) in [
        ("shaped", r#"[{"owner":{"role":"member"}}]"#, "must be { owner: Account }, got map (`owner.name` is missing)"),
        ("shaped", "[{}]", "must be { owner: Account }, got map (`owner` is missing)"),
        ("shaped", r#"[{"owner":{"name":"ada","role":"root"}}]"#, r#"(`owner.role` must be \"member\" | \"viewer\", got str \"root\")"#),
        ("nested", r#"[{"a":{"owner":"ada"}}]"#, r#"(`a.owner` must be Account, got str \"ada\")"#),
        ("mixed", r#"[{"owner":{"name":"a"},"n":"2","tags":[]}]"#, r#"(`n` must be int, got str \"2\")"#),
        ("mixed", r#"[{"owner":{"name":"a"},"n":2,"tags":[1]}]"#, "(`tags[0]` must be str, got int 1)"),
        ("shapes", r#"[[{"owner":{"name":"a"}},{"owner":{}}]]"#, "(`[1].owner.name` is missing)"),
    ] {
        let (status, body) = rpc(s.port, f, args);
        assert_eq!(status, 400, "{} {}: {}", f, args, body);
        assert!(body.contains(why), "{} {}: {}", f, args, body);
    }
}

/// A list of records named through types naming types in a row is made item by item however many
/// are in the row: through 7, as before, and through 8, 9 and 20, where `has_record` stopped at 8
/// and the items were only checked; a parameter named through 20 is made too, and a missing field
/// refused (src/record.rs `req_has_record`, FEEDBACK75.md follow-up found by 0.8.9's coverage,
/// Q-1171; branch feedback75).
#[test]
fn q1171_records_behind_any_number_of_types_naming_types_are_made() {
    let src = made_program();
    let d = dir("q1171-names", &[("main.qwe", &src)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let items = r#"[[{"name":"ada","is_admin":true},{"name":"bo","role":"member","x":{}}]]"#;
    for f in ["seven", "eight", "nine", "twenty"] {
        assert_eq!(rpc(s.port, f, items), (200, r#"{"ok":[{"name":"ada","role":"viewer"},{"name":"bo","role":"member"}]}"#.to_string()), "{}", f);
        assert_eq!(rpc(s.port, f, "[[]]"), (200, r#"{"ok":[]}"#.to_string()), "{}", f);
        let (status, body) = rpc(s.port, f, r#"[[{"name":"ada"},{"role":"member"}]]"#);
        assert!(status == 400 && body.contains("(`[1].name` is missing)"), "{}: {} {}", f, status, body);
    }
    assert_eq!(rpc(s.port, "named", r#"[{"name":"ü","is_admin":true}]"#), (200, r#"{"ok":{"name":"ü","role":"viewer"}}"#.to_string()));
}

/// An MCP tool's argument and a call to a service in the same process are made by the same walk: a
/// record in a map shape loses `is_admin` and gets its default, through 8 types naming types too,
/// and a missing field is refused (src/record.rs `req_make_shape`, FEEDBACK75.md follow-up found
/// by 0.8.9's coverage, Q-1171; coverage of 0.8.9; branch feedback75).
#[test]
fn q1171_tools_and_services_make_the_record_in_a_map_shape() {
    // A service called by the program's own code.
    let service = format!(
        "{}{}server fn shaped(x: {{ owner: Account }}) {{\n    return x\n}}\nserver fn far(xs: list<Ea>) {{\n    return xs\n}}\n",
        ACCOUNT,
        names_in_a_row('E', 8).0
    );
    let main = r#"service acct = "acct.qwe"
print(acct.shaped({ owner: { name: "Zoë", is_admin: true } }))
print(acct.far([{ name: "ada", role: "member", is_admin: true }, { name: "bo" }]))
"#;
    let bad = r#"service acct = "acct.qwe"
print(error(acct.shaped({ owner: { role: "member" } })))
"#;
    let d = dir("q1171-service", &[("main.qwe", main), ("bad.qwe", bad), ("acct.qwe", &service)]);
    checked(&d, "main.qwe");
    let (ok_bad, check_bad) = check(&d, "bad.qwe");
    assert!(!ok_bad && check_bad.contains("has no field `name`"), "{}", check_bad);
    let (out, err) = run_script(&d, "main.qwe", 120);
    assert_eq!(out, "{ owner: { name: \"Zoë\", role: \"viewer\" } }\n[{ name: \"ada\", role: \"member\" }, { name: \"bo\", role: \"viewer\" }]\n", "{}", err);
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["run", "bad.qwe", "--no-check"]).current_dir(&*d).env("QUILLO_SECRET", "test secret for the cov089 data tests");
    let (_, bad_out, bad_err) = within(c, 120);
    assert_eq!(bad_out, "service acct: argument `x` of `shaped` must be { owner: Account }, got map (`owner.name` is missing)\n", "{}", bad_err);
    // A tool, through the MCP door.
    let _one = serial();
    let src = format!(
        "{}route POST /login ? {{ user: str }} = set_session({{ user: user }})\n\n/// The owner it is given.\nserver fn mcp owner(x: {{ owner: Account }}) -> map with time {{\n    return x\n}}\n",
        ACCOUNT
    );
    let d = dir("q1171-tool", &[("app.qwe", &src)]);
    checked(&d, "app.qwe");
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    quillo::mcp::open_door(&mut it, &d.join("app.qwe"), None);
    let r = send_bytes(&mut it, "/login?user=ana", &[], Vec::new());
    assert_eq!(r.status, 204, "{}", r.text());
    let set = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.clone()).expect("a session cookie");
    let cookie = set.split(';').next().unwrap().to_string();
    let mut tool = |args: &str| {
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{{}}}},"name":"owner","arguments":{}}}}}"#,
            args
        );
        let headers = [("Content-Type", "application/json"), ("Cookie", cookie.as_str()), ("MCP-Protocol-Version", "2026-07-28"), ("Mcp-Method", "tools/call"), ("Mcp-Name", "owner")];
        let r = send_bytes(&mut it, "/_quillo/mcp", &headers, body.into_bytes());
        let text = r.text();
        (r.status, serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|e| panic!("not JSON ({}): {} {}", e, r.status, text)))
    };
    let (status, j) = tool(r#"{"x":{"owner":{"name":"ada","is_admin":true}}}"#);
    // An object is the structured content itself.
    assert_eq!((status, j["result"]["structuredContent"].clone()), (200, serde_json::json!({"owner": {"name": "ada", "role": "viewer"}})), "{}", j);
    let (_, j) = tool(r#"{"x":{"owner":{"role":"member"}}}"#);
    assert!(j["result"]["isError"] == serde_json::json!(true) || j.get("error").is_some(), "{}", j);
    assert!(j.to_string().contains("`owner.name` is missing"), "{}", j);
}

// ---- Q-1180: a whole number where a float goes is widened however the float is reached ------

/// The program of Q-1180's tests: floats behind 1, 8, 9 and 20 types naming types, in map shapes,
/// nested, in a list of shapes, in a record inside a shape, and `int | float`.
fn float_program() -> String {
    let mut src = "type Reading = { at: str, value: float }\n".to_string();
    let mut fns = String::new();
    for (p, n, f) in [('G', 1, "one"), ('E', 8, "eight"), ('N', 9, "nine"), ('Z', 20, "twenty")] {
        let (types, first) = names_to(p, n, "float");
        src.push_str(&types);
        fns.push_str(&format!("server fn {}(xs: list<{}>) {{\n    return xs\n}}\n", f, first));
    }
    src.push_str(&fns);
    src.push_str(
        r#"server fn direct(x: float) {
    return x
}
server fn named(x: Za) {
    return x
}
server fn point(x: { p: float }) {
    return x
}
server fn nested(x: { a: { p: float, q: float? } }) {
    return x
}
server fn points(xs: list<{ p: float }>) {
    return xs
}
server fn reading(x: { r: Reading }) {
    return x
}
server fn either(x: { n: int | float, ns: list<int | float> }) {
    return x
}
server fn far(x: { p: Ea }) {
    return x
}
"#,
    );
    src
}

/// A whole number where a float goes is taken as the float, as `x: float` takes it, however the
/// float is reached: behind 8, 9 and 20 types naming types in a list (behind 1 as before), where
/// `has_float` stopped at 8 and refused it; in a map shape, nested, optional, in a list of shapes,
/// in a record inside a shape and behind 8 types in a shape's field; `int | float` keeps an int as
/// it is; what is no number is refused with its way, as before (src/record.rs `req_to_make` and
/// `req_make_shape`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1180; branch feedback75).
#[test]
fn q1180_a_whole_number_where_a_float_goes_is_widened_however_it_is_reached() {
    let src = float_program();
    let d = dir("q1180-floats", &[("main.qwe", &src)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let ok = |body: &str| (200, format!(r#"{{"ok":{}}}"#, body));
    assert_eq!(rpc(s.port, "direct", "[1]"), ok("1.0"));
    for f in ["one", "eight", "nine", "twenty"] {
        assert_eq!(rpc(s.port, f, "[[1, 2.5, -3]]"), ok("[1.0,2.5,-3.0]"), "{}", f);
        assert_eq!(rpc(s.port, f, "[[]]"), ok("[]"), "{}", f);
        let (status, body) = rpc(s.port, f, r#"[[1, "a"]]"#);
        assert!(status == 400 && body.contains(r#"(`[1]` must be "#) && body.contains(r#", got str \"a\")"#), "{}: {} {}", f, status, body);
    }
    assert_eq!(rpc(s.port, "named", "[7]"), ok("7.0"));
    for (f, args, want) in [
        ("point", r#"[{"p":1}]"#, r#"{"p":1.0}"#),
        ("point", r#"[{"p":1.5,"ü":0}]"#, r#"{"p":1.5,"ü":0}"#),
        ("nested", r#"[{"a":{"p":2}}]"#, r#"{"a":{"p":2.0}}"#),
        ("nested", r#"[{"a":{"p":2,"q":3}}]"#, r#"{"a":{"p":2.0,"q":3.0}}"#),
        ("nested", r#"[{"a":{"p":2,"q":null}}]"#, r#"{"a":{"p":2.0,"q":null}}"#),
        ("points", r#"[[{"p":1},{"p":0.5}]]"#, r#"[{"p":1.0},{"p":0.5}]"#),
        ("reading", r#"[{"r":{"at":"noon","value":20}}]"#, r#"{"r":{"at":"noon","value":20.0}}"#),
        ("either", r#"[{"n":1,"ns":[1,2.5]}]"#, r#"{"n":1,"ns":[1,2.5]}"#),
        ("far", r#"[{"p":4}]"#, r#"{"p":4.0}"#),
    ] {
        assert_eq!(rpc(s.port, f, args), ok(want), "{} {}", f, args);
    }
    for (f, args, why) in [
        ("point", r#"[{"p":"x"}]"#, r#"(`p` must be float, got str \"x\")"#),
        ("point", "[{}]", "(`p` is missing)"),
        ("nested", r#"[{"a":{"p":true}}]"#, "(`a.p` must be float, got bool true)"),
        ("points", r#"[[{"p":1},{"p":[]}]]"#, "(`[1].p` must be float, got list [])"),
        ("reading", r#"[{"r":{"at":"noon","value":"hot"}}]"#, r#"(`r.value` must be float, got str \"hot\")"#),
        ("far", r#"[{"p":"x"}]"#, r#"(`p` must be Ea, got str \"x\")"#),
    ] {
        let (status, body) = rpc(s.port, f, args);
        assert_eq!(status, 400, "{} {}: {}", f, args, body);
        assert!(body.contains(why), "{} {}: {}", f, args, body);
    }
}

/// The program of Q-1180's review's tests: `Num | int` in shapes, parameters and lists, either
/// order, through aliases; decimals and money in shapes, lists and behind a type's name.
const KEPT_AND_CONVERTED: &str = r#"type Num = float
type Nn = Num
type Either = Num | int
type Amount = dec
server fn shape_num_int(x: { n: Num | int }) {
    return x
}
server fn shape_int_num(x: { n: int | Num }) {
    return x
}
server fn shape_alias(x: { n: Nn | int, e: Either, p: Num }) {
    return x
}
server fn direct_num_int(n: Num | int) {
    return n
}
server fn list_num_int(xs: list<Num | int>) {
    return xs
}
server fn shape_dec(x: { a: dec, p: float }) {
    return x
}
server fn direct_dec(a: dec) {
    return a
}
server fn list_dec(xs: list<dec>) {
    return xs
}
server fn shape_amount(x: { a: Amount, more: { b: Amount? } }) {
    return x
}
server fn shape_money(x: { m: money }) {
    return x
}
server fn direct_money(m: money) {
    return m
}
"#;

/// An int stays an int where the union names `int`, whichever comes first and however the float
/// beside it is named (`Num | int`, `int | Num`, `Nn | int`, `type Either = Num | int`), in a map
/// shape's field, a parameter and a list (0.8.8 gave a parameter and a list 1.0; a shape, 1); a
/// float stays a float, and an int where only a float goes (`p: Num`) is widened. A decimal in a
/// shape, a list or behind a type's name takes `"1.50"`, `1.5` and `3` as a `dec` parameter does,
/// and money in a shape is made as a `money` parameter makes it, refused with its reason; text
/// that is no number is refused with its way (src/record.rs `req_make_names`, `req_to_make` and
/// `req_make_shape`, FEEDBACK75.md follow-up found by 0.8.9's coverage, Q-1180's review; branch
/// feedback75).
#[test]
fn q1180_an_int_stays_an_int_beside_int_and_decimals_and_money_are_made_in_shapes() {
    let d = dir("q1180-review", &[("main.qwe", KEPT_AND_CONVERTED)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let ok = |body: &str| (200, format!(r#"{{"ok":{}}}"#, body));
    for (f, args, want) in [
        ("shape_num_int", r#"[{"n":1}]"#, r#"{"n":1}"#),
        ("shape_num_int", r#"[{"n":1.5}]"#, r#"{"n":1.5}"#),
        ("shape_int_num", r#"[{"n":1}]"#, r#"{"n":1}"#),
        ("shape_int_num", r#"[{"n":-2.5}]"#, r#"{"n":-2.5}"#),
        ("shape_alias", r#"[{"n":1,"e":2,"p":3}]"#, r#"{"n":1,"e":2,"p":3.0}"#),
        ("direct_num_int", "[1]", "1"),
        ("direct_num_int", "[0.5]", "0.5"),
        ("list_num_int", "[[1,2.5,-3]]", "[1,2.5,-3]"),
        ("shape_dec", r#"[{"a":"1.50","p":1.5}]"#, r#"{"a":{"$dec":"1.50"},"p":1.5}"#),
        ("shape_dec", r#"[{"a":1.5,"p":2}]"#, r#"{"a":{"$dec":"1.5"},"p":2.0}"#),
        ("shape_dec", r#"[{"a":3,"p":2.5}]"#, r#"{"a":{"$dec":"3"},"p":2.5}"#),
        ("direct_dec", r#"["1.50"]"#, r#"{"$dec":"1.50"}"#),
        ("direct_dec", "[1.5]", r#"{"$dec":"1.5"}"#),
        ("direct_dec", "[3]", r#"{"$dec":"3"}"#),
        ("list_dec", r#"[["1.50",1.5,3]]"#, r#"[{"$dec":"1.50"},{"$dec":"1.5"},{"$dec":"3"}]"#),
        ("shape_amount", r#"[{"a":"0.10","more":{"b":2}}]"#, r#"{"a":{"$dec":"0.10"},"more":{"b":{"$dec":"2"}}}"#),
        ("shape_amount", r#"[{"a":1,"more":{}}]"#, r#"{"a":{"$dec":"1"},"more":{}}"#),
        ("shape_money", r#"[{"m":{"amount":"1.50","currency":"usd"}}]"#, r#"{"m":{"amount":{"$dec":"1.50"},"currency":"USD"}}"#),
        ("direct_money", r#"[{"amount":"1.50","currency":"usd"}]"#, r#"{"amount":{"$dec":"1.50"},"currency":"USD"}"#),
    ] {
        assert_eq!(rpc(s.port, f, args), ok(want), "{} {}", f, args);
    }
    for (f, args, why) in [
        ("shape_num_int", r#"[{"n":"1"}]"#, r#"(`n` must be Num | int, got str \"1\")"#),
        ("shape_dec", r#"[{"a":"abc","p":1}]"#, r#"(`a` must be dec, got str \"abc\")"#),
        ("list_dec", r#"[["1",true]]"#, "(`[1]` must be dec, got bool true)"),
        ("shape_amount", r#"[{"a":"1","more":{"b":"x"}}]"#, r#"(`more.b` must be Amount?, got str \"x\")"#),
        ("shape_money", r#"[{"m":{"amount":"1.555","currency":"USD"}}]"#, "(`m` has more decimals than USD has (2))"),
        ("direct_money", r#"[{"amount":"1.555","currency":"USD"}]"#, "(has more decimals than USD has (2))"),
    ] {
        let (status, body) = rpc(s.port, f, args);
        assert_eq!(status, 400, "{} {}: {}", f, args, body);
        assert!(body.contains(why), "{} {}: {}", f, args, body);
    }
}

/// An int stays an int beside a float where the union names `int` through types naming types
/// (`type Cnt = int`, a chain of them, `type Id = int | str`), in either order, in a map shape's
/// field, a parameter and a list: `float | Cnt` took it as a float (0.8.8 too for a parameter and
/// a list, not a shape); a list of ints is no int, so `float | Ints` widens; a float stays a float
/// (src/record.rs `req_make_names` and `names_int`, FEEDBACK75.md follow-up found by 0.8.9's
/// coverage, Q-1180's second review; branch feedback75).
#[test]
fn q1180_an_int_stays_an_int_beside_an_alias_of_int() {
    let src = r#"type Cnt = int
type Ca = Cb
type Cb = Cc
type Cc = int
type Id = int | str
type Ints = list<int>
server fn shape_float_cnt(x: { v: float | Cnt }) {
    return x
}
server fn shape_cnt_float(x: { v: Cnt | float }) {
    return x
}
server fn direct_float_cnt(v: float | Cnt) {
    return v
}
server fn direct_cnt_float(v: Cnt | float) {
    return v
}
server fn list_float_cnt(xs: list<float | Cnt>) {
    return xs
}
server fn list_cnt_float(xs: list<Cnt | float>) {
    return xs
}
server fn chain(x: { v: float | Ca }, w: float | Ca, xs: list<float | Ca>) {
    return [x, w, xs]
}
server fn id(x: { v: float | Id }, w: float | Id, xs: list<float | Id>) {
    return [x, w, xs]
}
server fn ints(v: float | Ints) {
    return v
}
"#;
    let d = dir("q1180-int-alias", &[("main.qwe", src)]);
    checked(&d, "main.qwe");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let ok = |body: &str| (200, format!(r#"{{"ok":{}}}"#, body));
    for (f, args, want) in [
        ("shape_float_cnt", r#"[{"v":1}]"#, r#"{"v":1}"#),
        ("shape_cnt_float", r#"[{"v":1}]"#, r#"{"v":1}"#),
        ("shape_float_cnt", r#"[{"v":1.5}]"#, r#"{"v":1.5}"#),
        ("direct_float_cnt", "[1]", "1"),
        ("direct_cnt_float", "[1]", "1"),
        ("direct_float_cnt", "[-0.5]", "-0.5"),
        ("list_float_cnt", "[[1,2.5]]", "[1,2.5]"),
        ("list_cnt_float", "[[1,2.5]]", "[1,2.5]"),
        ("chain", r#"[{"v":1}, 2, [3, 4.5]]"#, r#"[{"v":1},2,[3,4.5]]"#),
        ("id", r#"[{"v":1}, 2, [3, "ü", 4.5]]"#, r#"[{"v":1},2,[3,"ü",4.5]]"#),
        ("ints", "[1]", "1.0"),
        ("ints", "[[1, 2]]", "[1,2]"),
    ] {
        assert_eq!(rpc(s.port, f, args), ok(want), "{} {}", f, args);
    }
    let (status, body) = rpc(s.port, "shape_float_cnt", r#"[{"v":"1"}]"#);
    assert!(status == 400 && body.contains(r#"(`v` must be float | Cnt, got str \"1\")"#), "{} {}", status, body);
}

// ---- MySQL (MariaDB): Q-1170 ----------------------------------------------------------------

/// A `quillo run` of `main.qwe` in `d`, and its port; killed when dropped.
fn start(d: &Path, args: &[&str]) -> (support::Guard, u16) {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .args(["--port", "0", "--log", "off"])
        .args(args)
        .current_dir(d)
        .env("QUILLO_SECRET", "test secret for the cov089 data tests")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cannot start quillo");
    let mut g = support::Guard::new(child);
    let port = support::port_in_stderr(&mut g.0, "main.qwe", Instant::now() + Duration::from_secs(60));
    (g, port)
}

/// One request; its answer's status and body, status 0 for none within `wait`.
fn call(port: u16, method: &str, path: &str, wait: Duration) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return (0, "no connection".into()) };
    s.set_read_timeout(Some(wait)).unwrap();
    let head = format!("{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 0\r\n\r\n", method, path);
    if s.write_all(head.as_bytes()).is_err() {
        return (0, "not sent".into());
    }
    let mut out = Vec::new();
    if s.read_to_end(&mut out).is_err() {
        return (0, "no answer in time".into());
    }
    let out = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, body.to_string())
}

/// A MySQL transaction outside a request (a script's top level) takes its connection in place, as
/// before: one commits what it wrote, one that fails rolls back, unicode kept. Then, served, with
/// the database's server gone: a request's transaction, waiting for a connection without a
/// thread, gets the driver's error at once, not the 30 s wait's, and the server answers on
/// (src/my.rs `checkout_later` and src/db.rs `checkout_for_tx`, FEEDBACK78.md Q-1170; coverage of
/// 0.8.9; branch feedback75).
#[test]
fn a_mysql_transaction_outside_a_request_and_with_its_server_gone() {
    let Some(my) = support::mariadb("cov089-data-tx") else {
        eprintln!("skipped: no MariaDB");
        return;
    };
    let url = my.url("cov089_tx");
    let script = format!(
        r#"db.open("{url}")
db.exec("create table t (id int primary key, name varchar(20))")
let a = db.transaction(() => {{
    db.exec("insert into t values (1, ?)", "Zoë")?
    db.one("select count(*) as n from t")?.n
}})
let b = db.transaction(() => {{
    db.exec("insert into t values (2, 'b')")?
    err("changed my mind")
}})
print(a)
print(error(b))
print(db.query("select name from t order by id"))
"#
    );
    let d = dir("mysql-tx", &[("main.qwe", &script)]);
    checked(&d, "main.qwe");
    let (out, err) = run_script(&d, "main.qwe", 120);
    assert_eq!(out, "1\nchanged my mind\n[{ name: \"Zoë\" }]\n", "{}", err);

    let served = format!(
        r#"db.open("{url}")

route POST /tx {{
    let r = db.transaction(() => db.one("select count(*) as n from t")?.n)
    if is_err(r) {{ reply 500 {{ error: error(r) }} }}
    r
}}

route GET /ping = "pong"
"#
    );
    std::fs::write(d.join("main.qwe"), &served).unwrap();
    checked(&d, "main.qwe");
    let (_g, port) = start(&d, &[]);
    let wait = Duration::from_secs(60);
    assert_eq!(call(port, "POST", "/tx", wait), (200, "1".to_string()));
    drop(my);
    let t = Instant::now();
    let (status, body) = call(port, "POST", "/tx", wait);
    assert_eq!(status, 500, "{}", body);
    // The driver's own words, as the system says them ("Connection refused (os error 61)" on
    // macOS, 111 on Linux).
    assert!(body.starts_with(r#"{"error":"db: "#) && body.contains("Connection refused") && !body.contains("no MySQL connection free"), "{}", body);
    assert!(t.elapsed() < Duration::from_secs(25), "the driver's error came after {:?}", t.elapsed());
    assert_eq!(call(port, "GET", "/ping", wait), (200, "pong".to_string()));
}

/// With the pool's one connection held by a transaction, another request's transaction waits for
/// it 30 s, holding no thread (another request is answered meanwhile), then fails with what to do,
/// and the holder commits once let go (src/my.rs `checkout_later`, FEEDBACK78.md Q-1170; coverage
/// of 0.8.9; branch feedback75).
#[test]
fn a_mysql_transaction_waits_30_s_for_a_connection_then_says_so() {
    let Some(my) = support::mariadb("cov089-data-wait") else {
        eprintln!("skipped: no MariaDB");
        return;
    };
    let url = format!("{}?pool_min=1&pool_max=1", my.url("cov089_wait"));
    let src = format!(
        r#"db.open("{url}")
db.exec("create table if not exists held (n int)")

route POST /hold = db.transaction(() => {{
    db.exec("insert into held values (1)")?
    write_file("holding.txt", "yes")
    let rounds = 0
    while is_err(read_file("go.txt")) && rounds < 6000 {{
        sleep(50)
        rounds += 1
    }}
    "held"
}})

route POST /wait {{
    let r = db.transaction(() => db.one("select 2 as two")?.two)
    if is_err(r) {{ reply 500 {{ error: error(r) }} }}
    r
}}

route GET /ping = "pong"
route GET /count = str(db.one("select count(*) as n from held")?.n)
"#
    );
    let d = dir("mysql-wait", &[("main.qwe", &src)]);
    checked(&d, "main.qwe");
    let (_g, port) = start(&d, &[]);
    let wait = Duration::from_secs(300);
    let holder = std::thread::spawn(move || call(port, "POST", "/hold", Duration::from_secs(300)));
    // The holder has the connection once it says so.
    let until = Instant::now() + Duration::from_secs(120);
    while !d.join("holding.txt").exists() {
        assert!(Instant::now() < until, "the holder did not start its transaction");
        std::thread::sleep(Duration::from_millis(20));
    }
    let waiter = std::thread::spawn(move || {
        let t = Instant::now();
        (call(port, "POST", "/wait", Duration::from_secs(300)), t.elapsed())
    });
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(call(port, "GET", "/ping", wait), (200, "pong".to_string()), "a request is answered while a transaction waits");
    let ((status, body), took) = waiter.join().unwrap();
    assert_eq!(
        (status, body.as_str()),
        (500, r#"{"error":"db: no MySQL connection free after 30 s: all the pool's are in use (`?pool_max=N` in the URL allows more)"}"#),
        "after {:?}",
        took
    );
    assert!(took >= Duration::from_secs(29), "{:?}", took);
    std::fs::write(d.join("go.txt"), "go").unwrap();
    assert_eq!(holder.join().unwrap(), (200, "held".to_string()));
    assert_eq!(call(port, "GET", "/count", wait), (200, "1".to_string()));
    assert_eq!(call(port, "POST", "/wait", wait), (200, "2".to_string()));
}
