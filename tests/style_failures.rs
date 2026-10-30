//! `style_with` when its command can't start or fails: the program doesn't
//! start, and says what went wrong.

mod support;

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn program(d: &Path, cmd: &str) {
    let src = format!("style_with(\"{}\")\ncomponent Card() {{\n style css`.card {{ color: red }}`\n <p.card>x</p>\n}}\nroute GET / = <Card />\n", cmd);
    std::fs::write(d.join("app.qwe"), src).unwrap();
}

fn quillo(d: &Path, path: Option<&str>) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(["run", "app.qwe", "--port", &support::free_port().to_string()]).current_dir(d).stdin(Stdio::null());
    if let Some(p) = path {
        c.env("PATH", p);
    }
    c.output().unwrap()
}

#[test]
fn a_command_that_can_not_start_stops_the_program() {
    let d = support::temp_dir("style-no-shell");
    program(&d, "cat");
    let o = quillo(&d, Some(&d.join("no-programs-here").display().to_string()));
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(said.contains("style_with `cat` failed: No such file or directory"), "{}", said);
}

#[test]
fn a_command_that_fails_gives_what_it_said() {
    let d = support::temp_dir("style-fails");
    program(&d, "echo it went wrong >&2; exit 3");
    let o = quillo(&d, None);
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(said.contains("style_with `echo it went wrong >&2; exit 3` failed: it went wrong"), "{}", said);
}
