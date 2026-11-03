//! `.qwer` files past the usual path (src/native.rs): a signature whose brackets never close, two
//! files built together (and when that cannot be, each built alone), and a built library that
//! cannot be read. These build with the `cargo` running the tests, into a cache under the test
//! target's own folder.

mod support;

use quillo::native;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn cache(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cover-native-cache").join(name)
}

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cover-native-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// `quillo` in `d` with this cache, given ten minutes at most (a first build compiles serde_json).
fn quillo(d: &Path, args: &[&str], cache: &Path) -> Output {
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(d)
        .env("QUILLO_NATIVE_CACHE", cache)
        .env("CARGO_NET_OFFLINE", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    support::stop_with_tests(&child);
    let id = child.id();
    let done = Arc::new(AtomicBool::new(false));
    let d2 = done.clone();
    std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(600);
        while !d2.load(Ordering::SeqCst) {
            if Instant::now() > until {
                let _ = Command::new("kill").args(["-KILL", &id.to_string()]).status();
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    let o = child.wait_with_output().unwrap();
    done.store(true, Ordering::SeqCst);
    o
}

fn out(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn a_signature_whose_brackets_never_close_is_no_function_and_the_next_is_read() {
    let p = native::parse("pub fn broken(x: Vec<<i64) { }\npub fn fine() -> i64 { 2 }\n");
    let names: Vec<&str> = p.exports.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"broken"), "{:?}", names);
    assert!(names.contains(&"fine"), "{:?}", names);
}

const MAIN: &str = "use \"a.qwer\"\nuse \"b.qwer\"\nprint(one() + two())\n";

#[test]
fn two_files_are_built_together_and_a_library_that_cannot_be_read_is_not_loaded() {
    use std::os::unix::fs::PermissionsExt;
    let c = cache("together");
    let d = dir("together", &[("a.qwer", "pub fn one() -> i64 { 1 }\n"), ("b.qwer", "pub fn two() -> i64 { 2 }\n"), ("main.qwe", MAIN)]);
    let o = quillo(&d, &["run", "main.qwe"], &c);
    assert!(out(&o).contains("building ") && out(&o).contains(" together"), "{}", out(&o));
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "3", "{}", out(&o));
    // Built and locked; then made unreadable, and loaded only as built (--no-build): refused.
    let lib = std::fs::read_dir(d.join(".quillo/native")).unwrap().flatten().map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().starts_with(&format!("{}a-", std::env::consts::DLL_PREFIX))).unwrap();
    std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o000)).unwrap();
    let o = quillo(&d, &["run", "main.qwe", "--no-build"], &c);
    std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(o.status.code(), Some(0));
    let said = out(&o);
    assert!(said.contains("will not load the library of") && said.contains("cannot read") && said.to_ascii_lowercase().contains("permission denied"), "{}", said);
}

#[test]
fn what_cannot_be_built_together_is_built_alone_and_says_why() {
    let c = cache("apart");
    // Workspaces of earlier runs go; their build folder stays, so cargo has less to do.
    let _ = std::fs::remove_dir_all(c.join("workspaces"));
    let d = dir("apart", &[("a.qwer", "pub fn one() -> i64 { nope_not_here }\n"), ("b.qwer", "pub fn two() -> i64 { nada_not_here }\n"), ("main.qwe", MAIN)]);
    // Together, cargo fails; alone, the first one used says why.
    let o = quillo(&d, &["run", "main.qwe"], &c);
    let said = out(&o);
    assert_ne!(o.status.code(), Some(0));
    assert!(said.contains(" together") && said.contains("nope_not_here"), "{}", said);
    let ws: Vec<PathBuf> = std::fs::read_dir(c.join("workspaces")).unwrap().flatten().map(|e| e.path()).filter(|p| p.join("Cargo.toml").is_file()).collect();
    assert_eq!(ws.len(), 1, "{:?}", ws);
    let ws = &ws[0];
    // A member's manifest that cannot be written: nothing is built together.
    let member = std::fs::read_dir(ws.join("m")).unwrap().flatten().map(|e| e.path()).next().unwrap();
    std::fs::remove_file(member.join("Cargo.toml")).unwrap();
    std::fs::create_dir(member.join("Cargo.toml")).unwrap();
    let o = quillo(&d, &["run", "main.qwe"], &c);
    assert!(!out(&o).contains(" together") && out(&o).contains("nope_not_here"), "{}", out(&o));
    std::fs::remove_dir(member.join("Cargo.toml")).unwrap();
    // The workspace's own manifest that cannot be written: the same.
    std::fs::remove_file(ws.join("Cargo.toml")).unwrap();
    std::fs::create_dir(ws.join("Cargo.toml")).unwrap();
    let o = quillo(&d, &["run", "main.qwe"], &c);
    assert!(!out(&o).contains(" together") && out(&o).contains("nope_not_here"), "{}", out(&o));
    std::fs::remove_dir(ws.join("Cargo.toml")).unwrap();
}

#[test]
fn a_cache_that_cannot_be_made_is_said() {
    let blocker = cache("blocked-file");
    std::fs::create_dir_all(blocker.parent().unwrap()).unwrap();
    std::fs::write(&blocker, "a file where the cache would go").unwrap();
    let d = dir("no-cache", &[("a.qwer", "pub fn one() -> i64 { 1 }\n"), ("b.qwer", "pub fn two() -> i64 { 2 }\n"), ("main.qwe", MAIN)]);
    let o = quillo(&d, &["run", "main.qwe"], &blocker.join("inside"));
    assert_ne!(o.status.code(), Some(0));
    assert!(out(&o).contains("cannot make") && !out(&o).contains(" together"), "{}", out(&o));
}
