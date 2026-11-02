//! Measuring what of src/client/runtime.js the tests run (scripts/runtime-coverage.sh):
//! with QUILLO_RUNTIME_COVERAGE set, the runtime goes out as written with `//@rt N` marks,
//! and tests/browser/rtcover.js maps what Chrome and Node ran back to runtime.js's lines.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VAR: &str = "QUILLO_RUNTIME_COVERAGE";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn rtcover(args: &[&str]) -> Output {
    Command::new("node").arg(root().join("tests/browser/rtcover.js")).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Maps `script` with rtcover.js: how many lines of runtime.js it holds, or the error.
fn mapped(script: &Path) -> String {
    let js = format!(
        "const m = require({:?}).mapScript(require('fs').readFileSync({:?}, 'utf8')); console.log(m ? m.length : 'none')",
        root().join("tests/browser/rtcover.js"),
        script
    );
    let o = Command::new("node").arg("-e").arg(js).env_remove("NODE_OPTIONS").output().unwrap();
    text(&o).trim().to_string()
}

fn script_for(body: &str) -> String {
    let mut it = quillo::Interp::new(".");
    it.load_source(&format!("fn __t() {{\n{}\n}}\n", body), "case.qwe").unwrap();
    quillo::client::test_script(&it, "__t").unwrap()
}

#[test]
fn the_runtime_goes_out_as_written_and_maps_back_to_its_lines() {
    if !node_available() {
        eprintln!("skipping: needs Node");
        return;
    }
    let dir = support::temp_dir("runtime-coverage");
    // The variable is the whole test binary's: this file has one test.
    let before = std::env::var_os(VAR);

    // Not measuring (an empty value too): minified, no marks.
    std::env::remove_var(VAR);
    let plain = script_for("return upper(\"a\")");
    assert!(!plain.contains("//@rt"));
    std::env::set_var(VAR, "");
    assert!(!script_for("return upper(\"a\")").contains("//@rt"));

    std::env::set_var(VAR, &dir);
    let js = script_for("return upper(\"a\")");
    assert!(js.contains("\n//@rt 1\n") && js.contains("\n//@rt end\n"), "{}", js);
    assert!(js.contains("\"use strict\";\n"), "the preamble as written");
    assert!(js.len() > plain.len());
    let f = dir.join("case.js");
    std::fs::write(&f, &js).unwrap();
    let n: usize = mapped(&f).parse().unwrap_or_else(|_| panic!("{}", mapped(&f)));
    assert!(n > 10);

    // A run under the Node hook records the lines that ran, and the report sums them.
    let records = dir.join("records");
    let o = Command::new("node")
        .arg(&f)
        .env(VAR, &records)
        .env("NODE_OPTIONS", format!("--require {}", root().join("tests/browser/rtcover-node.js").display()))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "\"A\"", "{}", text(&o));
    let r = rtcover(&["report", records.to_str().unwrap()]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(text(&r).contains("lines run") && text(&r).contains("(1 records)"), "{}", text(&r));
    let lcov = dir.join("lcov.info");
    let summary = dir.join("runtime.json");
    let r = rtcover(&[
        "report",
        records.to_str().unwrap(),
        "--min",
        "100",
        "--lcov",
        lcov.to_str().unwrap(),
        "--json",
        summary.to_str().unwrap(),
        "--diff",
        "HEAD",
    ]);
    assert!(!r.status.success(), "a part of the runtime is below 100%");
    assert!(text(&r).contains("below the 100.00%"), "{}", text(&r));
    let info = std::fs::read_to_string(&lcov).unwrap();
    assert!(info.contains("SF:") && info.contains(",1\n") && info.ends_with("end_of_record\n"));
    let sum: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&summary).unwrap()).unwrap();
    assert_eq!(sum["file"], "src/client/runtime.js");
    assert!(sum["run"].as_u64().unwrap() > 0 && sum["run"].as_u64() < sum["lines"].as_u64());
    assert!(!sum["missed"].as_array().unwrap().is_empty());

    // Several folders are summed up together; one with nothing in it adds nothing.
    let both = format!("{},{}", records.display(), dir.join("none").display());
    let r = rtcover(&["report", &both]);
    assert!(r.status.success() && text(&r).contains("(1 records)"), "{}", text(&r));

    // A revision that is not there: said, and a failure, not a crash.
    let r = rtcover(&["report", records.to_str().unwrap(), "--diff", "no-such-revision"]);
    assert!(!r.status.success() && text(&r).contains("cannot compare with no-such-revision"), "{}", text(&r));

    // Nothing recorded: the report says the variable was not set, and fails.
    let empty = dir.join("none");
    let r = rtcover(&["report", empty.to_str().unwrap()]);
    assert!(!r.status.success() && text(&r).contains("no coverage records"), "{}", text(&r));
    assert_eq!(rtcover(&[]).status.code(), Some(2));

    // A script whose runtime line is not runtime.js's is refused, not counted wrong.
    std::fs::write(&f, js.replacen("\"use strict\";", "\"use sloppy\";", 1)).unwrap();
    assert!(mapped(&f).contains("is not the script's"), "{}", mapped(&f));
    // A script without the runtime holds none of its lines.
    std::fs::write(&f, "console.log(1)\n").unwrap();
    assert_eq!(mapped(&f), "none");
    // The file as written, as development bundles load it, maps line for line.
    let rt = std::fs::read_to_string(root().join("src/client/runtime.js")).unwrap();
    std::fs::write(&f, format!("(function () {{\n{}}})();\n", rt)).unwrap();
    assert_eq!(mapped(&f), rt.split('\n').count().to_string());

    // The runtime file a page loads, with its `// dev` lines left out, maps too.
    let srv = support::serve(&root().join("tests/browser/app.qwe"), &[(VAR, dir.to_str().unwrap())]);
    let page = support::send(srv.port, "GET", "/", &[], "").body;
    let at = page.find("/_quillo/r/").unwrap_or_else(|| panic!("no runtime file in {}", page));
    let path: String = page[at..].chars().take_while(|c| *c != '"' && *c != '\'').collect();
    let file = support::send(srv.port, "GET", &path, &[], "").body;
    assert!(file.contains("//@rt 1\n") && file.contains("//@rt end\n"), "{}", &file[..file.len().min(300)]);
    std::fs::write(&f, &file).unwrap();
    let n: usize = mapped(&f).parse().unwrap_or_else(|_| panic!("{}", mapped(&f)));
    assert!(n > 1000, "{}", n);

    match before {
        Some(v) => std::env::set_var(VAR, v),
        None => std::env::remove_var(VAR),
    }
}
