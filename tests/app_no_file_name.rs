//! `quillo app` given a path that names no file, which it can't take a window
//! title from. It stops there: a test can't see a window.

#![cfg(feature = "app")]

mod support;

use std::process::{Command, Stdio};

#[test]
fn a_path_with_no_file_name_is_refused_before_any_window() {
    let d = support::temp_dir("app-no-name");
    let out = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["app", "no-such-folder/.."])
        .current_dir(&d)
        .env("HOME", &d)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(1), "{}", said);
    assert!(said.contains("no-such-folder/.."), "{}", said);
}
