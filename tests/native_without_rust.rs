//! A `.qwer` file where Rust isn't installed, and one reached through a folder
//! that can't be looked up. One test: it changes the process's PATH and folder.

mod support;

use quillo::native;
use std::os::unix::fs::PermissionsExt;

#[test]
fn without_cargo_each_says_to_install_rust() {
    let d = support::temp_dir("native-without-rust");
    let inner = d.join("up/inner");
    std::fs::create_dir_all(&inner).unwrap();
    let file = inner.join("tool.qwer");
    std::fs::write(&file, "pub fn add(a: i64, b: i64) -> i64 { a + b }\n").unwrap();
    std::env::remove_var("CARGO");
    std::env::set_var("PATH", d.join("no-programs-here"));
    std::env::set_var("QUILLO_NATIVE_CACHE", d.join("cache"));

    let e = native::check(&file).unwrap_err();
    assert!(e.starts_with("cargo did not run (") && e.ends_with("install Rust (https://rustup.rs) for its errors here"), "{}", e);
    let e = native::build(&file).err().unwrap();
    assert!(e.starts_with(&format!("cannot build {}: cargo did not run (", file.display())) && e.contains("Install Rust (https://rustup.rs)"), "{}", e);

    // The file can be read from where we are, but its full path can't be worked out: it goes by
    // the name it was given.
    std::env::set_current_dir(&inner).unwrap();
    std::fs::set_permissions(d.join("up"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let relative = std::path::Path::new("tool.qwer");
    let checked = native::check(relative);
    let built = native::build(relative);
    std::fs::set_permissions(d.join("up"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(checked.unwrap_err().starts_with("cargo did not run ("));
    assert!(built.err().unwrap().starts_with("cannot build tool.qwer: cargo did not run ("));
}
