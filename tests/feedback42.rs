//! FEEDBACK42.md: what a tool can see of a Quillo page, and Q-482's `sleep`.

use quillo::check::{check_source, Severity};
use std::path::Path;

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

#[test]
fn sleep_in_seconds_is_said() {
    let d = diags("server fn a() -> str {\n    sleep(0.2)\n    \"a\"\n}\nserver fn b() { sleep(200) }\nserver fn c(n: float) { sleep(n) }\nserver fn d() { sleep(0) }\nserver fn e() { sleep(1) }\nserver fn f() { sleep(0.999) }\nserver fn g() { sleep(1.5) }\n");
    assert_eq!(
        d,
        [
            "2 warning: `sleep` takes milliseconds: `sleep(0.2)` waits 0.2 ms; for 0.2 seconds write `sleep(200)`",
            "9 warning: `sleep` takes milliseconds: `sleep(0.999)` waits 0.999 ms; for 0.999 seconds write `sleep(999)`",
        ]
    );
    // A function of the program's own named `sleep` is its own business.
    assert!(diags("fn sleep(s) = s\nprint(sleep(0.5))\n").iter().all(|m| !m.contains("milliseconds")));
}
