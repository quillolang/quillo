//! `.qwer` files at their edges (src/native.rs): what the reader of Rust takes
//! and passes over, what the compiler says, and libraries that do not fit.
//! These build with the `cargo` running the tests, into a cache of their own.

mod support;

use quillo::native::{self, Export};
use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Once;

fn cache() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-edges-cache")
}

fn building() {
    static SET: Once = Once::new();
    SET.call_once(|| std::env::set_var("QUILLO_NATIVE_CACHE", cache()));
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-native-edges-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn names(src: &str) -> Vec<String> {
    native::parse(src).exports.iter().map(|e| e.name.clone()).collect()
}

fn problems(src: &str) -> Vec<String> {
    native::parse(src).problems.into_iter().map(|(l, m)| format!("{} {}", l, m)).collect()
}

#[test]
fn comments_are_not_code_and_lines_are_counted_past_them() {
    let p = native::parse("/* pub fn hidden() {}\n   two lines */\npub fn after() -> i64 { 1 }\n");
    assert_eq!((p.exports[0].name.as_str(), p.exports[0].line), ("after", 3));
    assert_eq!(names("pub fn one() {}\n/* pub fn never ended() {}"), ["one"]);
    assert_eq!(names("// pub fn in_a_line() {}\npub fn two() {}\n// and at the end"), ["two"]);
}

#[test]
fn strings_and_characters_are_not_code() {
    let src = r####"const A: &str = "pub fn a() {";
const B: &str = r"pub fn b() {";
const C: &str = r#"pub fn c() {"#;
const D: &str = "with \" a quote pub fn d() {";
const E: char = '{';
const F: char = '\'';
const G: char = '\n';
const H: char = 'é';
const I: &[u8] = br"not raw here";
const J: &'static str = "two
lines";
pub fn real(s: &str) -> String { s.to_string() }
"####;
    let p = native::parse(src);
    assert_eq!(p.exports.iter().map(|e| (e.name.as_str(), e.line)).collect::<Vec<_>>(), [("real", 12)]);
    // Ended by the end of the file.
    assert_eq!(names("pub fn one() {}\nconst S: &str = \"never ends"), ["one"]);
    assert_eq!(names("pub fn one() {}\nconst S: &str = r#\"never ends"), ["one"]);
    assert_eq!(names("pub fn one() {}\nconst S: char = '\\"), ["one"]);
    assert_eq!(names("pub fn one() {}\nconst S: char = '"), ["one"]);
}

#[test]
fn a_comment_documents_the_item_right_below_it() {
    let p = native::parse("/// Adds.\n///  Twice.\n//// not this\npub fn add() {}\n\n/// Parted by a blank line.\n\npub fn none() {}\n\n/// Ended by a semicolon.\nconst X: i64 = 1;\npub fn nor() {}\n");
    let docs: Vec<(&str, &str)> = p.exports.iter().map(|e| (e.name.as_str(), e.doc.as_str())).collect();
    assert_eq!(docs, [("add", "Adds.\n Twice."), ("none", ""), ("nor", "")]);
}

#[test]
fn what_cannot_be_exported_says_why() {
    let p = |src: &str| problems(src).join("\n");
    assert!(p("pub fn f(x: &mut i64) {}").contains("1 `f` is not exported: a `&mut` parameter would change nothing Quillo sees"));
    assert!(p("\n\npub async fn f() {}").contains("3 `f` is not exported: `async fn` cannot be called from Quillo"));
    assert!(p("pub unsafe fn f() {}").contains("`unsafe fn` cannot be called"));
    assert!(p("pub extern \"C\" fn f() {}").contains("`extern fn` cannot be called"));
    assert!(p("pub fn f<T>(x: T) -> T { x }").contains("`f` is not exported: a generic function has no one type"));
    assert!(p("pub fn f(x: impl Clone) {}").contains("a generic function"));
    assert!(p("pub fn f(x: i64) where i64: Clone {}").contains("a generic function"));
    assert!(p("pub fn f;").contains("`f`: cannot read its parameters"));
    assert!(p("pub fn f(x) {}").contains("`f`: cannot read the parameter `x`"));
    assert!(p("pub fn f(x: Point) {}").contains("`f` is not exported: `Point` cannot cross"));
    assert!(p("pub fn f() -> HashMap<i64, f64> { }").contains("a map's keys are text: HashMap<i64, f64> must have String keys"));
    assert!(p("pub fn f() -> Option<Point> { }").contains("`Point` cannot cross"));
    assert!(p("pub fn f() -> Result<Point, String> { }").contains("`Point` cannot cross"));
    assert!(p("pub fn f() -> Vec<Point> { }").contains("`Point` cannot cross"));
    assert!(p("pub fn f() -> HashMap<String, Point> { }").contains("`Point` cannot cross"));
    assert!(native::quillo_type("BTreeMap<u8, u8>").is_err());
}

#[test]
fn an_array_parameter_is_exported_or_said_not_to_be() {
    let p = native::parse("pub fn f(x: [u8; 4]) -> i64 { 1 }");
    assert_eq!(p.exports.len() + p.problems.len(), 1, "{:?}", p);
}

#[test]
fn other_items_are_passed_over() {
    for src in [
        "pub struct P;",
        "pub const X: i64 = 1;",
        "pub use std::fmt;",
        "pub(crate) fn f() {}",
        "pub mod m { pub fn inside() {} }",
        "pub",
        "pub fn",
        // Parameters that never end, and a signature that never does.
        "pub fn f(x: Vec<i64",
        "pub fn f(x: i64",
    ] {
        assert!(names(src).is_empty(), "{}", src);
    }
    assert_eq!(names("public fn f() {}\npub fn g() {}"), ["g"]);
    // `pub const fn` is a function.
    assert_eq!(names("pub const fn k() -> i64 { 1 }"), ["k"]);
}

#[test]
fn a_functions_body_is_skipped_whatever_is_in_it() {
    let src = r####"pub fn a() -> i64 {
    // a brace } in a comment
    /* and } in
       another */
    let s = "}";
    let r = r"}";
    let h = r#"}"#;
    let c = '}';
    let e = '\'';
    let l: &'static str = "x";
    let identr = 1; // a name ending in r, before a quote: r"
    1
}
pub fn b() -> i64 { 2 }
pub fn c() -> i64 {
"####;
    assert_eq!(names(src), ["a", "b", "c"]);
}

#[test]
fn types_are_written_any_way_rust_takes() {
    let p = native::parse("pub fn f ( a : i64 , mut b : Vec < i64 > , c : & str , d : & [ String ] , e : std :: collections :: HashMap < String , i64 > , ) -> Option < String > { None }");
    let e: &Export = &p.exports[0];
    let ps: Vec<(&str, &str)> = e.params.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    assert_eq!(ps, [("a", "i64"), ("b", "Vec<i64>"), ("c", "&str"), ("d", "&[String]"), ("e", "std::collections::HashMap<String, i64>")]);
    assert_eq!(e.ret.as_deref(), Some("Option<String>"));
    assert_eq!(native::parse("pub fn f() -> () {}").exports[0].ret, None);
    assert_eq!(native::quillo_type("std::collections::VecDeque<i64>").unwrap(), "list");
    assert_eq!(native::quillo_type("std::collections::BTreeMap<String, bool>").unwrap(), "map");
}

#[test]
fn a_name_that_is_not_there_gets_the_nearest() {
    let e = |n: &str, f: &str| Export { name: n.into(), params: Vec::new(), ret: None, line: 1, doc: f.into() };
    let have = [e("greet", ""), e("sum", "")];
    assert_eq!(native::missing_export("x.qwer", "gret", &have), "x.qwer has no function `gret` (did you mean `greet`?)");
    assert_eq!(native::missing_export("x.qwer", "completely", &have), "x.qwer has no function `completely`; it has `greet`, `sum`");
    assert_eq!(native::display_name("native:/a/b.dylib:greet"), "greet");
}

const LIB: &str = r#"
pub fn add(a: i64, b: i64) -> i64 { a + b }
pub fn total(xs: &Vec<i64>) -> i64 { xs.iter().sum() }
pub fn hello(name: &String) -> String { format!("hello {}", name) }
"#;

#[test]
fn a_file_is_used_by_name_or_whole_and_built_once() {
    building();
    let d = dir("use");
    std::fs::write(d.join("lib.qwer"), LIB).unwrap();
    let mut it = Interp::new(&d);
    assert_eq!(it.load_source("use add, hello as hi from \"lib.qwer\"\n[add(1, 2), hi(\"ana\")]", "main.qwe").unwrap().repr(), "[3, \"hello ana\"]");
    let e = it.load_source("total([1])", "m2.qwe").err().unwrap().to_string();
    assert!(e.contains("total"), "{}", e);
    let e = it.load_source("use ad from \"lib.qwer\"\n", "m3.qwe").err().unwrap().to_string();
    assert!(e.contains("lib.qwer has no function `ad` (did you mean `add`?)"), "{}", e);
    // Loaded again by another program of the process: the same library.
    let mut other = Interp::new(&d);
    assert_eq!(other.load_source("use \"lib.qwer\"\ntotal([1, 2, 3])", "main.qwe").unwrap().repr(), "6");
    // A name of no library.
    let e = native::call(&mut it, "native:nothing:here", Vec::new(), 7).err();
    assert!(matches!(e, Some(quillo::interp::Ctrl::Error(e)) if e.to_string().contains("a Rust function was called without its library")));
    // Two building one file at once: one builds, and the other takes its library.
    std::fs::write(d.join("twice.qwer"), "pub fn two() -> i64 { 2 }\n").unwrap();
    let f = d.join("twice.qwer");
    let g = f.clone();
    let t = std::thread::spawn(move || native::build(&g).map(|r| r.0));
    let a = native::build(&f).map(|r| r.0).unwrap();
    assert_eq!(t.join().unwrap().unwrap(), a);
}

#[test]
fn a_library_that_does_not_fit_is_refused() {
    building();
    let d = dir("misfit");
    std::fs::write(d.join("one.qwer"), "pub fn one() -> i64 { 1 }\n").unwrap();
    std::fs::write(d.join("two.qwer"), "pub fn two() -> i64 { 2 }\npub fn three() -> i64 { 3 }\n").unwrap();
    let (one, _) = native::build(&d.join("one.qwer")).unwrap();
    let (two, _) = native::build(&d.join("two.qwer")).unwrap();
    // Built with one function, where two are expected.
    std::fs::copy(&one, &two).unwrap();
    let e = Interp::new(&d).load_source("use \"two.qwer\"\n", "main.qwe").err().unwrap().to_string();
    assert!(e.contains("was made for another version of Quillo; delete it to build it again"), "{}", e);
}

#[test]
fn quillo_build_finds_what_a_program_uses_and_mounts() {
    building();
    let d = dir("build-all");
    std::fs::write(d.join("lib.qwer"), LIB).unwrap();
    std::fs::write(d.join("sub.qwe"), "use \"lib.qwer\"\nroute GET / = add(1, 2)\n").unwrap();
    std::fs::write(d.join("other.qwe"), "use \"lib.qwer\"\nuse Chart from \"chart.js\"\nroute GET / = add(1, 2)\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"lib.qwer\"\nuse \"other.qwe\"\nmount /sub = \"sub.qwe\"\nroute GET / = add(1, 2)\n").unwrap();
    let built = native::build_all(&d.join("main.qwe")).unwrap();
    assert_eq!(built.len(), 1, "{:?}", built);
    assert!(built[0].is_file());
    std::fs::write(d.join("bad.qwe"), "fn (\n").unwrap();
    assert!(native::build_all(&d.join("bad.qwe")).unwrap_err().contains("bad.qwe"));
    assert!(native::build_all(&d.join("missing.qwe")).unwrap_err().contains("cannot read"));
}

#[test]
fn a_name_that_cannot_start_a_crate_is_made_one_that_can() {
    building();
    let d = dir("digits");
    std::fs::write(d.join("1st.qwer"), "pub fn first() -> i64 { 1 }\n").unwrap();
    let mut it = Interp::new(&d);
    assert_eq!(it.load_source("use first from \"1st.qwer\"\nfirst()", "main.qwe").unwrap().repr(), "1");
    let (lib, _) = native::build(&d.join("1st.qwer")).unwrap();
    assert!(lib.file_name().unwrap().to_string_lossy().contains("q1st-"), "{}", lib.display());
}

#[test]
fn the_compiler_is_heard_on_what_is_not_the_files_fault_too() {
    building();
    let d = dir("check");
    // The code made around the file cannot take this: on its first line.
    std::fs::write(d.join("around.qwer"), "pub fn f(x: Vec<&str>) -> i64 { x.len() as i64 }\n").unwrap();
    let notes = native::check(&d.join("around.qwer")).unwrap();
    assert!(notes.iter().any(|n| n.error && n.line == 1 && n.col == 0), "{:?}", notes);
    // A crate that cannot be had: what cargo says.
    std::fs::write(d.join("nocrate.qwer"), "//! [dependencies]\n//! serde_json = { path = \"/no/such/crate\" }\n\npub fn f() {}\n").unwrap();
    let notes = native::check(&d.join("nocrate.qwer")).unwrap();
    assert_eq!(notes.len(), 1, "{:?}", notes);
    assert!(notes[0].error && notes[0].message.contains("error"), "{:?}", notes);
    let e = native::build(&d.join("nocrate.qwer")).err().unwrap();
    assert!(e.starts_with("cannot build") && e.contains("no/such/crate"), "{}", e);
    // A warning in the file is its; not one elsewhere.
    std::fs::write(d.join("warns.qwer"), "pub fn f() -> i64 { let unused = 1; 2 }\n").unwrap();
    let notes = native::check(&d.join("warns.qwer")).unwrap();
    assert!(notes.iter().all(|n| !n.error) && notes.iter().any(|n| n.message.contains("unused")), "{:?}", notes);
    assert!(native::check(&d.join("missing.qwer")).unwrap_err().contains("cannot read"));
    assert!(native::build(&d.join("missing.qwer")).unwrap_err().contains("cannot read"));
}

/// `quillo build` of a file that fails at once, and where its crate was made.
fn cache_in(name: &str, env: &[(&str, &Path)], remove: &[&str]) -> PathBuf {
    let d = dir(name);
    std::fs::write(d.join("lib.qwer"), "//! [dependencies]\n//! serde_json = { path = \"/no/such/crate\" }\n\npub fn f() {}\n").unwrap();
    std::fs::write(d.join("main.qwe"), "use \"lib.qwer\"\nroute GET / = \"x\"\n").unwrap();
    let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo"));
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("build").arg(d.join("main.qwe")).env_remove("QUILLO_NATIVE_CACHE").env("CARGO_HOME", cargo_home).stdin(Stdio::null());
    if let Some(r) = std::env::var_os("RUSTUP_HOME").or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".rustup").into_os_string())) {
        c.env("RUSTUP_HOME", r);
    }
    for v in remove {
        c.env_remove(v);
    }
    let out = c.envs(env.iter().map(|(k, v)| (*k, *v))).output().unwrap();
    assert!(!out.status.success());
    d
}

#[test]
fn the_cache_is_where_the_system_says() {
    let base = dir("cache-roots");
    let (xdg, home, tmp) = (base.join("xdg"), base.join("home"), base.join("tmp"));
    for p in [&xdg, &home, &tmp] {
        std::fs::create_dir_all(p).unwrap();
    }
    let crates = |root: &Path| root.join("quillo").join("native").join("crates");
    cache_in("cache-xdg", &[("XDG_CACHE_HOME", &xdg), ("HOME", &home)], &[]);
    assert!(crates(&xdg).is_dir());
    assert!(!crates(&home.join(".cache")).exists());
    cache_in("cache-home", &[("HOME", &home)], &["XDG_CACHE_HOME"]);
    assert!(crates(&home.join(".cache")).is_dir());
    cache_in("cache-tmp", &[("TMPDIR", &tmp)], &["XDG_CACHE_HOME", "HOME"]);
    assert!(crates(&tmp).is_dir());
}
