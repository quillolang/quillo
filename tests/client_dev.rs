//! Development bundles: each function in a file of its own, with a source map
//! back to its `.qwe` line. Every test here has `client::DEV` on, which is
//! why they have a test program of their own.

mod support;

use quillo::client;
use quillo::Interp;
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard};
use support::temp_dir;

/// One at a time: `the_oldest_bundles_are_let_go` lets the others' go too.
fn alone() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// A program in `dir`, loaded from `file`.
fn program(dir: &Path, file: &str, src: &str) -> Interp {
    client::DEV.store(true, Ordering::Relaxed);
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, src).unwrap();
    let mut it = Interp::new(dir.to_str().unwrap());
    it.load_source(src, path.to_str().unwrap()).unwrap_or_else(|e| panic!("{}", e));
    it
}

/// The loader a page runs: where its files are, and their names in order.
fn listed(loader: &str) -> (String, Vec<String>) {
    let at = loader.split("const at = \"").nth(1).unwrap().split('"').next().unwrap().to_string();
    let list = loader.split("for (const f of [").nth(1).unwrap().split(']').next().unwrap();
    (at, serde_json::from_str::<Vec<String>>(&format!("[{}]", list)).unwrap())
}

fn hash_of(at: &str) -> String {
    at.trim_end_matches('/').rsplit('/').next().unwrap().to_string()
}

/// A file of the bundle, and its source map when it has one.
fn file(hash: &str, name: &str) -> (String, Option<Value>) {
    let text = client::dev_file(hash, name).unwrap_or_else(|| panic!("no {}", name));
    let map = text.split("base64,").nth(1).map(|b64| {
        let bytes = b64decode(b64.trim());
        serde_json::from_slice(&bytes).unwrap()
    });
    (text, map)
}

fn b64decode(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut bits, mut n) = (0u32, 0);
    for c in s.bytes().filter(|c| *c != b'=') {
        bits = bits << 6 | A.iter().position(|a| *a == c).unwrap() as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    out
}

/// A source map's source lines (from 1), in the order of its segments.
fn mapped_lines(map: &Value) -> Vec<i64> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lines = Vec::new();
    let mut src_line = 0i64;
    for seg in map["mappings"].as_str().unwrap().split([';', ',']).filter(|s| !s.is_empty()) {
        let mut fields = Vec::new();
        let (mut v, mut shift) = (0i64, 0);
        for c in seg.bytes() {
            let d = A.iter().position(|a| *a == c).unwrap() as i64;
            v |= (d & 31) << shift;
            shift += 5;
            if d & 32 == 0 {
                fields.push(if v & 1 == 1 { -(v >> 1) } else { v >> 1 });
                v = 0;
                shift = 0;
            }
        }
        src_line += fields[2];
        lines.push(src_line + 1);
    }
    lines
}

#[test]
fn each_function_has_a_file_mapped_to_its_lines() {
    let _one = alone();
    let d = temp_dir("client-dev-lines").canonicalize().unwrap();
    let padding = "\n".repeat(120);
    let src = format!(
        "{}fn helper(x) {{\n  let total = x + 1\n  total * 2\n}}\ncomponent Counter() {{\n  state n = helper(1)\n  <button @click={{() => {{\n    n += 1\n  }}}}>{{n}}</button>\n}}\nroute GET / = <Counter/>\n",
        padding
    );
    let it = program(&d, "main.qwe", &src);
    let (at, files) = listed(&client::bundle(&it).unwrap());
    assert!(at.starts_with("/_quillo/src/"), "{}", at);
    assert_eq!(files, ["quillo.js", "program.js", "main.qwe/helper.js", "main.qwe/Counter.js", "boot.js"]);
    let hash = hash_of(&at);
    let (runtime, none) = file(&hash, "quillo.js");
    assert!(runtime.contains("function boot"), "the runtime as it is written");
    assert!(none.is_none());
    let (program_js, _) = file(&hash, "program.js");
    assert!(program_js.contains("DEV = true;"));
    let (helper, map) = file(&hash, "main.qwe/helper.js");
    let map = map.expect("a source map");
    assert!(helper.starts_with("\"use strict\";\n"));
    assert!(!helper.contains('\u{1}'), "a line mark was left in");
    assert_eq!(map["version"], 3);
    assert_eq!(map["sources"][0].as_str().unwrap(), format!("file://{}", d.join("main.qwe").display()));
    assert_eq!(map["sourcesContent"][0].as_str().unwrap(), src);
    let lines = mapped_lines(&map);
    assert!(lines.contains(&122) && lines.contains(&123), "{:?}", lines);
    // The text goes in the first map made from the file, and the others name it.
    let (_, counter) = file(&hash, "main.qwe/Counter.js");
    let counter = counter.expect("a source map");
    assert_eq!(counter["sourcesContent"][0], Value::Null);
    assert_eq!(counter["names"], serde_json::json!(["n"]));
    assert!(mapped_lines(&counter).contains(&128), "{:?}", mapped_lines(&counter));
    assert_eq!(client::dev_file(&hash, "nothing.js"), None);
    assert_eq!(client::dev_file("nothing", "quillo.js"), None);
}

#[test]
fn constants_and_views_go_in_files_between_the_functions() {
    let _one = alone();
    let d = temp_dir("client-dev-pieces").canonicalize().unwrap();
    let src = "let first = 5\nlet second = \"two\"\nfn uses() = first + 1\nlet third = 7\nview Tag(t) = <b>{t}</b>\nfn after() = third + 1\ncomponent C() = <p><Tag t={uses()}/>{second}{after()}{icon}</p>\nlet icon = <i></i>\n";
    let it = program(&d, "main.qwe", src);
    let (at, files) = listed(&client::bundle(&it).unwrap());
    let hash = hash_of(&at);
    let joined: Vec<String> = files.iter().map(|f| client::dev_file(&hash, f).unwrap()).collect();
    assert_eq!(files.first().unwrap(), "quillo.js");
    assert_eq!(files.last().unwrap(), "boot.js");
    // A value of the program's is written with its name.
    assert!(joined.iter().any(|t| t.contains("\"first\"") && t.contains("= 5;")), "{:?}", files);
    assert!(joined.iter().any(|t| t.contains("\"two\"")), "{:?}", files);
    // A view keeps its `<Tag/>` form beside it.
    let tag = files.iter().position(|f| f == "main.qwe/Tag.js").unwrap();
    assert!(joined[tag].contains("W.view("), "{}", joined[tag]);
    // A file of constants has no map and no marks.
    let plain = files.iter().position(|f| f.starts_with("program-")).unwrap_or_else(|| panic!("{:?}", files));
    assert!(!joined[plain].contains("sourceMappingURL"));
    assert!(!joined[plain].contains('\u{1}'));
}

#[test]
fn a_view_with_no_file_of_its_own_gets_one() {
    let _one = alone();
    let d = temp_dir("client-dev-builtin-view").canonicalize().unwrap();
    let mut it = program(&d, "main.qwe", "view Tag(t) = <b>{t}</b>\ncomponent C() = <Tag t=\"x\"/>\n");
    // Loaded from text with no file of its own, it has no map either.
    it.load_source("view Other(t) = <i>{t}</i>\ncomponent D() = <Other t=\"y\"/>\n", "").unwrap_or_else(|e| panic!("{}", e));
    let (at, files) = listed(&client::bundle(&it).unwrap());
    let hash = hash_of(&at);
    let texts: Vec<String> = files.iter().map(|f| client::dev_file(&hash, f).unwrap()).collect();
    assert!(texts.iter().any(|t| t.contains("W.view(") && t.contains("Other")), "{:?}", files);
}

#[test]
fn a_mounted_programs_base_is_in_the_addresses() {
    let _one = alone();
    let d = temp_dir("client-dev-base").canonicalize().unwrap();
    let mut it = program(&d, "main.qwe", "component C() = <p>x</p>\n");
    it.base = "/shop".to_string();
    let (at, files) = listed(&client::bundle(&it).unwrap());
    assert!(at.starts_with("/shop/_quillo/src/"), "{}", at);
    let (text, _) = file(&hash_of(&at), &files[1]);
    assert!(text.contains("BASE = \"/shop\";"), "{}", text);
}

#[test]
fn a_file_outside_the_program_goes_by_its_name_and_paths_are_escaped() {
    let _one = alone();
    let d = temp_dir("client-dev-outside").canonicalize().unwrap();
    let inside = d.join("app");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(d.join("shared lib.qwe"), "fn café() = 1\n").unwrap();
    let it = program(&inside, "main.qwe", "use \"../shared lib.qwe\"\ncomponent C() = <p>{café()}</p>\n");
    let (at, files) = listed(&client::bundle(&it).unwrap());
    assert!(files.iter().any(|f| f == "shared%20lib.qwe/caf%C3%A9.js"), "{:?}", files);
    let (_, map) = file(&hash_of(&at), "shared%20lib.qwe/caf%C3%A9.js");
    assert!(map.unwrap()["sources"][0].as_str().unwrap().ends_with("/shared%20lib.qwe"));
}

#[test]
fn the_oldest_bundles_are_let_go() {
    let _one = alone();
    let d = temp_dir("client-dev-many").canonicalize().unwrap();
    let hash_of_nth = |n: usize| {
        let it = program(&d, "main.qwe", &format!("component C() = <p>{}</p>\n", n));
        hash_of(&listed(&client::bundle(&it).unwrap()).0)
    };
    let first = hash_of_nth(0);
    assert!(client::dev_file(&first, "quillo.js").is_some());
    for n in 1..=300 {
        hash_of_nth(n);
    }
    assert!(client::dev_file(&first, "quillo.js").is_none(), "the bundle was kept");
}
