//! `quillo build` when a step around the compiler fails: where the crate is
//! written, where the library goes, a library that can't be loaded. The compiler
//! here is a script standing in for `cargo`, so that each step fails at will.

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const TOOL: &str = "pub fn add(a: i64, b: i64) -> i64 { a + b }\n";
const MAIN: &str = "use \"tool.qwer\"\nprint(add(1, 2))\n";

/// `cargo` that finds its way as the real one does, then runs `then`.
const CARGO: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do
  case "$1" in
    --manifest-path) manifest="$2"; shift;;
    --target-dir) target="$2"; shift;;
  esac
  shift
done
crate=$(basename "$(dirname "$manifest")")
mkdir -p "$target/release"
"#;

fn makes_a_file_for_a_library() -> String {
    format!("printf 'no library' > \"$target/release/{}$crate.{}\"\n", std::env::consts::DLL_PREFIX, std::env::consts::DLL_EXTENSION)
}

struct App {
    dir: PathBuf,
    cargo: PathBuf,
    cache: PathBuf,
}

fn app(name: &str, then: &str) -> App {
    let dir = support::temp_dir(&format!("native-failures-{}", name)).canonicalize().unwrap();
    let tools = dir.join("tools");
    let src = dir.join("src");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    let cargo = tools.join("cargo");
    std::fs::write(&cargo, format!("{}{}", CARGO, then)).unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(src.join("tool.qwer"), TOOL).unwrap();
    std::fs::write(src.join("main.qwe"), MAIN).unwrap();
    App { cache: dir.join("cache"), cargo, dir: src }
}

impl App {
    fn quillo(&self, what: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .args(what)
            .current_dir(&self.dir)
            .env("CARGO", &self.cargo)
            .env("QUILLO_NATIVE_CACHE", &self.cache)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// What `quillo build` said on failing.
    fn build_fails(&self) -> String {
        let o = self.quillo(&["build", "main.qwe"]);
        assert!(!o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        String::from_utf8_lossy(&o.stderr).into_owned()
    }

    fn builds(&self) {
        let o = self.quillo(&["build", "main.qwe"]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }

    fn built(&self) -> PathBuf {
        std::fs::read_dir(self.dir.join(".quillo/native")).unwrap().map(|e| e.unwrap().path()).find(|p| p.extension().is_some_and(|x| x == std::env::consts::DLL_EXTENSION)).unwrap()
    }
}

fn says(got: String, what: &str) {
    assert!(got.contains(what), "wanted {:?} in: {}", what, got);
}

fn mode(p: &Path, m: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

#[test]
fn a_cache_folder_that_can_not_be_made_is_named() {
    let mut a = app("cache", &makes_a_file_for_a_library());
    std::fs::write(a.dir.join("a-file"), "not a folder").unwrap();
    a.cache = a.dir.join("a-file/cache");
    says(a.build_fails(), &format!("quillo: cannot make {}/crates/quillo_tool_", a.cache.display()));
}

#[test]
fn a_crate_file_it_may_not_write_says_so() {
    let a = app("crate-folder", &makes_a_file_for_a_library());
    a.builds();
    let crates = a.cache.join("crates");
    let folder = std::fs::read_dir(&crates).unwrap().next().unwrap().unwrap().path();
    std::fs::write(a.dir.join("tool.qwer"), format!("{}pub fn sub(a: i64, b: i64) -> i64 {{ a - b }}\n", TOOL)).unwrap();
    mode(&folder.join("lib.rs"), 0o444);
    says(a.build_fails(), "Permission denied");
}

#[test]
fn a_file_where_the_library_folder_goes_is_named() {
    let a = app("no-out-dir", &makes_a_file_for_a_library());
    std::fs::write(a.dir.join(".quillo"), "in the way").unwrap();
    says(a.build_fails(), &format!("cannot make {}", a.dir.join(".quillo/native").display()));
}

#[test]
fn a_build_that_left_no_library_is_named() {
    let a = app("no-library", "exit 0\n");
    let got = a.build_fails();
    says(got.clone(), "cannot copy ");
    says(got, "No such file or directory");
    assert_eq!(std::fs::read_dir(a.dir.join(".quillo/native")).unwrap().count(), 0, "no half-copied file");
}

#[test]
fn a_folder_where_the_library_goes_stops_the_build() {
    let a = app("folder-for-library", &makes_a_file_for_a_library());
    a.builds();
    let lib = a.built();
    std::fs::remove_file(&lib).unwrap();
    std::fs::create_dir_all(lib.join("inside")).unwrap();
    says(a.build_fails(), "Is a directory");
}

#[test]
fn a_library_that_will_not_load_is_named_with_the_way_out() {
    let a = app("not-a-library", &makes_a_file_for_a_library());
    a.builds();
    let o = a.quillo(&["run", "main.qwe"]);
    assert!(!o.status.success());
    says(String::from_utf8_lossy(&o.stderr).into_owned(), &format!("cannot load {}", a.built().display()));
}
