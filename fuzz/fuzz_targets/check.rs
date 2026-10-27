//! `quillo check` on any program that parses: diagnostics, never a panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;
use std::sync::OnceLock;

/// An empty folder: a `use` of another file finds nothing.
fn dir() -> &'static PathBuf {
    static D: OnceLock<PathBuf> = OnceLock::new();
    D.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("quillo-fuzz-check-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    })
}

fuzz_target!(|src: &str| {
    if quillo::parser::Parser::new(src).parse_program().is_err() {
        return;
    }
    let _ = quillo::check::check_source(src, "fuzz.qwe", dir());
});
