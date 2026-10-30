//! `quillo install` with nothing to say where its plugins are: no `--registry`
//! and no QUILLO_REGISTRY, which is the default registry.

mod support;

use std::process::{Command, Stdio};

#[test]
fn no_names_and_no_registry_asks_which_plugins_before_going_anywhere() {
    let d = support::temp_dir("install-defaults");
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("install").current_dir(&d).env("HOME", &d).env_remove("QUILLO_REGISTRY").stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.starts_with("quillo install: which plugins? `quillo install csv qr`, or `quillo install --list`\n"), "{}", said);
}

#[test]
fn an_empty_quillo_registry_counts_as_none() {
    let d = support::temp_dir("install-empty-registry");
    let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("install").current_dir(&d).env("HOME", &d).env("QUILLO_REGISTRY", "").stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("which plugins?"));
}
