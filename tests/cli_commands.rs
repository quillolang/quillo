//! The `quillo` command: what each command prints and the status it leaves, for those that
//! end by themselves.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod support;

/// A folder of its own, which is also the home of each `quillo` run in it.
fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("cli-{}", name));
    for (file, text) in files {
        if let Some(parent) = Path::new(file).parent() {
            std::fs::create_dir_all(d.join(parent)).unwrap();
        }
        std::fs::write(d.join(file), text).unwrap();
    }
    d
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args).current_dir(dir).env("HOME", dir).env("QUILLO_REGISTRY", "http://127.0.0.1:9");
    c
}

fn quillo(dir: &Path, args: &[&str]) -> Output {
    command(dir, args).stdin(Stdio::null()).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// What was said on stderr, and the status: for commands that are refused.
fn refused(dir: &Path, args: &[&str]) -> (i32, String) {
    let o = quillo(dir, args);
    (o.status.code().unwrap(), err(&o))
}

/// A program for commands that only look at their options.
const PRINTS: &str = "print(\"ran\")\n";

#[test]
fn version_is_the_crates() {
    let dir = folder("version", &[]);
    for flag in ["version", "--version", "-V"] {
        let o = quillo(&dir, &[flag]);
        assert!(o.status.success());
        assert!(out(&o).starts_with(concat!("quillo ", env!("CARGO_PKG_VERSION"))), "{}", out(&o));
    }
}

#[test]
fn help_is_on_stdout_and_a_wrong_command_gets_it_on_stderr() {
    let dir = folder("help", &[("app.qwe", PRINTS)]);
    for args in [&["help"][..], &["--help"], &["-h"], &["run", "app.qwe", "--help"], &["run", "app.qwe", "-h"]] {
        let o = quillo(&dir, args);
        assert!(o.status.success(), "{:?}", args);
        assert!(out(&o).contains("usage:\n  quillo run <file.qwe> [options]"), "{:?}", args);
        assert!(!out(&o).contains("ran\n"), "`--help` runs nothing");
    }
    let o = quillo(&dir, &["serve"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(out(&o).is_empty());
    assert!(err(&o).contains("usage:\n  quillo run <file.qwe> [options]"));
}

#[test]
fn a_qwe_file_alone_is_run() {
    let dir = folder("alone", &[("app.qwe", PRINTS)]);
    let o = quillo(&dir, &["app.qwe"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "ran\n");
}

#[test]
fn the_program_is_found_wherever_the_options_stand() {
    let dir = folder("program", &[("main.qwe", "print(\"main\")\n"), ("other/main.qwe", "print(\"other\")\n"), ("empty/note.txt", "")]);
    // No program named: this folder's main.qwe.
    assert_eq!(out(&quillo(&dir, &["run"])), "main\n");
    assert_eq!(out(&quillo(&dir, &["run", "--log", "off"])), "main\n");
    // A folder: its main.qwe.
    assert_eq!(out(&quillo(&dir, &["run", "other"])), "other\n");
    assert_eq!(out(&quillo(&dir, &["run", "--log", "off", "other"])), "other\n");
    // In a folder with none, or given one with none, nothing is guessed.
    assert_eq!(refused(&dir.join("empty"), &["run"]), (2, "quillo: no program: name a .qwe file, or add a main.qwe here\n".into()));
    assert_eq!(refused(&dir, &["run", "empty"]), (2, "quillo: no program: empty has no main.qwe; name a .qwe file\n".into()));
    assert_eq!(refused(&dir, &["check", "empty"]), (2, "quillo: no program: empty has no main.qwe; name a .qwe file\n".into()));
}

#[test]
fn an_options_value_may_follow_an_equals_sign() {
    let dir = folder("equals", &[("app.qwe", PRINTS)]);
    let o = quillo(&dir, &["run", "--log=off", "app.qwe"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "ran\n");
    assert_eq!(refused(&dir, &["run", "app.qwe", "--log=loud"]), (2, "quillo: --log is text, json or off\n".into()));
    assert_eq!(refused(&dir, &["run", "app.qwe", "--watch=yes"]), (2, "quillo: `--watch` takes no value: write `--watch` alone\n".into()));
    assert_eq!(refused(&dir, &["run", "app.qwe", "--no-check=1"]), (2, "quillo: `--no-check` takes no value: write `--no-check` alone\n".into()));
}

#[test]
fn an_unknown_option_is_named_with_what_was_probably_meant() {
    let dir = folder("unknown", &[("app.qwe", PRINTS)]);
    for (args, said) in [
        (&["run", "app.qwe", "--prot", "8081"][..], "quillo: unknown option `--prot` (did you mean `--port`?)\n"),
        (&["run", "app.qwe", "--no-chek"], "quillo: unknown option `--no-chek` (did you mean `--no-check`?)\n"),
        (&["run", "app.qwe", "--prot=8081"], "quillo: unknown option `--prot=8081` (did you mean `--port`?)\n"),
        (&["run", "app.qwe", "-p", "8081"], "quillo: unknown option `-p` (the port is `--port`)\n"),
        (&["run", "app.qwe", "--quite-unlike-any-option-there-is"], "quillo: unknown option `--quite-unlike-any-option-there-is`\n"),
        // A second program is not one more option.
        (&["run", "app.qwe", "other.qwe"], "quillo: unknown option `other.qwe`\n"),
    ] {
        assert_eq!(refused(&dir, args), (2, said.to_string()), "{:?}", args);
    }
}

#[test]
fn a_bad_options_value_says_what_it_takes() {
    let dir = folder("values", &[("app.qwe", PRINTS)]);
    for (option, value, said) in [
        ("--service-url", "billing", "--service-url is name=http://host:port (billing=http://10.0.0.5:8081)"),
        ("--service-url", "=http://10.0.0.5:8081", "--service-url is name=http://host:port (billing=http://10.0.0.5:8081)"),
        ("--service-url", "billing=ftp://10.0.0.5", "--service-url is name=http://host:port (billing=http://10.0.0.5:8081)"),
        ("--scale", "risk", "--scale is a service and how many copies of it, 1 to 64: --scale risk=3"),
        ("--scale", "risk=many", "--scale is a service and how many copies of it, 1 to 64: --scale risk=3"),
        ("--scale", "risk=0", "--scale is a service and how many copies of it, 1 to 64: --scale risk=3"),
        ("--scale", "risk=65", "--scale is a service and how many copies of it, 1 to 64: --scale risk=3"),
        ("--scale", "=3", "--scale is a service and how many copies of it, 1 to 64: --scale risk=3"),
        ("--join", "app:8080", "--join is the address of the app to join: --join http://app:8080"),
        ("--base", "shop", "--base is a path such as /shop"),
        ("--base", "/a//b", "--base is a path such as /shop"),
        ("--base", "/shop?x=1", "--base is a path such as /shop"),
        ("--wt-port", "0", "--wt-port needs a port number"),
        ("--wt-port", "seventy", "--wt-port needs a port number"),
        ("--wt-port", "70000", "--wt-port needs a port number"),
        ("--log", "loud", "--log is text, json or off"),
        ("--max-body", "0", "--max-body needs a positive number of bytes"),
        ("--max-body", "lots", "--max-body needs a positive number of bytes"),
        ("--instances", "0", "--instances needs a positive number"),
        ("--instances", "two", "--instances needs a positive number"),
        ("--workers", "0", "--workers needs a positive number"),
        ("--workers", "-1", "--workers needs a positive number"),
    ] {
        let (code, text) = refused(&dir, &["run", "app.qwe", option, value]);
        assert_eq!(code, 2, "{} {}", option, value);
        assert_eq!(text, format!("quillo: {}\n", said), "{} {}", option, value);
    }
}

#[test]
fn an_option_without_its_value_is_refused() {
    let dir = folder("novalue", &[("app.qwe", PRINTS)]);
    for args in [&["run", "app.qwe", "--port"][..], &["run", "app.qwe", "--workers"], &["run", "app.qwe", "--with", "  "]] {
        let (code, text) = refused(&dir, args);
        assert_eq!(code, 2, "{:?}", args);
        assert!(text.contains(args[2]), "{}", text);
    }
}

#[test]
fn an_option_without_its_value_is_not_called_unknown() {
    let dir = folder("novalue-said", &[("app.qwe", PRINTS)]);
    let (_, text) = refused(&dir, &["run", "app.qwe", "--port"]);
    assert!(!text.contains("did you mean `--port`"), "{}", text);
    assert!(!text.contains("unknown option"), "{}", text);
}

#[test]
fn a_qwe_file_alone_takes_options_as_run_does() {
    let dir = folder("alone-options", &[("app.qwe", PRINTS)]);
    let o = quillo(&dir, &["app.qwe", "--log=off"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "ran\n");
}

#[test]
fn check_refuses_what_it_does_not_know() {
    let dir = folder("check-unknown", &[("ok.qwe", PRINTS), ("bad.qwe", "fn f() = nope\n")]);
    assert_eq!(quillo(&dir, &["check", "ok.qwe", "--strict-nul"]).status.code(), Some(2));
    assert_ne!(quillo(&dir, &["check", "ok.qwe", "bad.qwe"]).status.code(), Some(0));
}

#[test]
fn options_that_go_together_are_said_to() {
    let dir = folder("together", &[("app.qwe", PRINTS)]);
    assert_eq!(refused(&dir, &["run", "app.qwe", "--tls-cert", "c.pem"]), (2, "quillo: --tls-cert and --tls-key go together\n".into()));
    assert_eq!(refused(&dir, &["run", "app.qwe", "--tls-key", "k.pem"]), (2, "quillo: --tls-cert and --tls-key go together\n".into()));
    assert_eq!(refused(&dir, &["run", "app.qwe", "--scale", "risk=2"]), (2, "quillo: --scale goes with --split: quillo run app.qwe --split --scale risk=3\n".into()));
    assert_eq!(refused(&dir, &["run", "app.qwe", "--split", "--instances", "2"]), (2, "quillo: --split does not go with --instances or --register\n".into()));
    assert_eq!(refused(&dir, &["run", "app.qwe", "--instances", "2", "--watch"]), (2, "quillo: --instances does not go with --watch or --register\n".into()));
}

#[test]
fn register_and_join_need_a_numeric_port() {
    let dir = folder("numeric", &[("app.qwe", "route GET / = \"hi\"\n")]);
    let o = command(&dir, &["run", "app.qwe", "--register", "http://127.0.0.1:9", "--port", "eighty"])
        .env("LIMER_ADMIN_SECRET", "an admin secret long enough to raise no warning")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!((o.status.code(), err(&o).as_str()), (Some(2), "quillo: --register needs a numeric --port\n"));
    assert_eq!(
        refused(&dir, &["run", "app.qwe", "--as-service", "--join", "http://127.0.0.1:9", "--port", "eighty"]),
        (2, "quillo: --join needs a numeric --port\n".into())
    );
    assert_eq!(
        refused(&dir, &["run", "app.qwe", "--join", "http://127.0.0.1:9", "--port", "8081"]),
        (2, "quillo: --join is for a copy of a service: quillo run risk.qwe --as-service --join http://app:8080\n".into())
    );
}

#[test]
fn an_env_file_is_read_before_the_program_and_must_be_there_when_named() {
    let dir = folder("envfile", &[("app.qwe", "print(env(\"GREETING\"))\n"), ("my.env", "GREETING=from the file\n"), ("sub/app.qwe", "print(env(\"GREETING\"))\n"), ("sub/.env", "GREETING=beside it\n")]);
    assert_eq!(out(&quillo(&dir, &["run", "app.qwe", "--env-file", "my.env"])), "from the file\n");
    // Without the option, the `.env` beside the program, wherever that is.
    assert_eq!(out(&quillo(&dir, &["run", "sub/app.qwe"])), "beside it\n");
    assert_eq!(out(&quillo(&dir, &["run", "app.qwe"])), "null\n");
    let (code, text) = refused(&dir, &["run", "app.qwe", "--env-file", "nope.env"]);
    assert_eq!(code, 2);
    assert!(text.starts_with("quillo: cannot read nope.env: "), "{}", text);
}

#[test]
fn run_stops_at_a_programs_errors_unless_told_not_to_check() {
    let dir = folder("errors", &[("typo.qwe", "fn f() = nope\nprint(\"ran\")\n"), ("syntax.qwe", "print(1) print(2) extra words\n"), ("broken.qwe", "fn zero() = 0\nprint(\"before\")\nprint(1 / zero())\nprint(\"after\")\n")]);
    let o = quillo(&dir, &["run", "typo.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("typo.qwe:1: error: unknown name `nope`"), "{}", err(&o));
    assert!(out(&o).is_empty(), "the checker stops it before it runs");
    assert!(!err(&o).contains("note:"), "a misspelt name is no sign of an old build: {}", err(&o));
    let o = quillo(&dir, &["run", "typo.qwe", "--no-check"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "ran\n");
    // A syntax error may be code for a newer Quillo: it says which one this is.
    let o = quillo(&dir, &["run", "syntax.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("syntax.qwe:1: error: "), "{}", err(&o));
    assert!(err(&o).contains(&format!("note: this is quillo {}", env!("CARGO_PKG_VERSION"))), "{}", err(&o));
    // An error while it runs: what it did stays, the status says it failed.
    let o = quillo(&dir, &["run", "broken.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(out(&o), "before\n");
    assert_eq!(err(&o), "quillo: broken.qwe:3: division by zero\n");
}

#[test]
fn check_reports_what_it_found_and_counts_the_program() {
    let dir = folder(
        "check",
        &[
            ("ok.qwe", "fn add(a, b) = a + b\nserver fn total() = add(1, 2)\nroute GET / = str(add(1, 2))\ncomponent C() { <p>hi</p> }\n"),
            ("one.qwe", "fn f(s: str) = !s\n"),
            ("two.qwe", "fn f(s: str) = !s\nfn g(s: str) = s ? 1 : 2\n"),
            ("both.qwe", "fn f(s: str) = !s\nfn g() = nope\n"),
            ("errs.qwe", "fn f() = nope\nfn g() = nada\n"),
            ("lib.qwe", "fn h() = 1\n"),
            ("uses.qwe", "use \"lib.qwe\"\nroute GET / = str(h())\n"),
            ("syntax.qwe", "print(1) print(2) extra words\n"),
        ],
    );
    let o = quillo(&dir, &["check", "ok.qwe"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "ok.qwe: ok (1 routes, 2 functions, 1 components)\n");

    let o = quillo(&dir, &["check", "one.qwe"]);
    assert!(o.status.success());
    assert!(err(&o).contains("one.qwe:1: warning: `s` counts as true"), "{}", err(&o));
    assert_eq!(out(&o), "one.qwe: ok (0 routes, 1 functions, 0 components), 1 warning\n");

    let o = quillo(&dir, &["check", "two.qwe"]);
    assert!(o.status.success());
    assert_eq!(out(&o), "two.qwe: ok (0 routes, 2 functions, 0 components), 2 warnings\n");

    let o = quillo(&dir, &["check", "both.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).is_empty());
    assert!(err(&o).ends_with("quillo: 1 error, 1 warning\n"), "{}", err(&o));

    let o = quillo(&dir, &["check", "errs.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).ends_with("quillo: 2 errors\n"), "{}", err(&o));

    assert_eq!(out(&quillo(&dir, &["check", "uses.qwe"])), "uses.qwe: ok (1 routes, 1 functions, 0 components, in 2 files)\n");

    let o = quillo(&dir, &["check", "syntax.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains(&format!("note: this is quillo {}", env!("CARGO_PKG_VERSION"))), "{}", err(&o));
    assert!(err(&o).ends_with("quillo: 1 error\n"), "{}", err(&o));
}

#[test]
fn check_warns_of_null_only_when_asked() {
    let src = "fn shout(s: str) = upper(s)\nfn maybe(x: int) = x > 1 ? \"a\" : null\nfn f() = shout(maybe(2))\n";
    let dir = folder("strict", &[("app.qwe", src)]);
    let o = quillo(&dir, &["check", "app.qwe"]);
    assert_eq!(out(&o), "app.qwe: ok (0 routes, 3 functions, 0 components)\n");
    assert!(err(&o).is_empty(), "{}", err(&o));
    for args in [&["check", "app.qwe", "--strict-null"], &["check", "--strict-null", "app.qwe"]] {
        let o = quillo(&dir, args);
        assert!(o.status.success());
        assert!(err(&o).contains("app.qwe:3: warning: argument `s` of `shout` may be null here"), "{}", err(&o));
        assert!(out(&o).ends_with(", 1 warning\n"));
    }
}

#[test]
fn docs_writes_the_programs_api_as_openapi() {
    let dir = folder("docs", &[("shop.qwe", "route GET /items = \"[]\"\nserver fn total(n: int) -> int = n\n"), ("bad.qwe", "route GET / = (\n")]);
    let o = quillo(&dir, &["docs", "shop.qwe"]);
    assert!(o.status.success(), "{}", err(&o));
    let doc: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(doc["openapi"], "3.0.3");
    assert_eq!(doc["info"]["title"], "shop");
    assert!(doc["paths"]["/items"]["get"].is_object(), "{}", doc);
    assert!(doc["x-quillo-server-fns"]["total"].is_object(), "{}", doc);

    let o = quillo(&dir, &["docs", "bad.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).is_empty());
    assert!(err(&o).starts_with("quillo: bad.qwe:2: "), "{}", err(&o));
    let o = quillo(&dir, &["docs", "nope.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo: cannot read nope.qwe"), "{}", err(&o));
}

#[test]
fn build_with_nothing_to_build_says_so() {
    let dir = folder("build", &[("app.qwe", "print(1)\n")]);
    let o = quillo(&dir, &["build", "app.qwe"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "app.qwe uses no .qwer files: nothing to build\n");
    let o = quillo(&dir, &["build", "nope.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo: cannot read nope.qwe"), "{}", err(&o));
}

/// What the prompt said, given these lines.
fn repl(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut child = command(dir, args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn the_repl_evaluates_line_by_line_and_keeps_what_it_was_given() {
    let dir = folder("repl", &[]);
    let o = repl(&dir, &["repl"], "1 + 2\nlet x = 40\nfn more(n) = n + 2\nmore(x)\nprint(\"said\")\n");
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(text.starts_with(&format!("Quillo {}", env!("CARGO_PKG_VERSION"))), "{}", text);
    assert!(text.contains("quillo> 3\n"), "{}", text);
    assert!(text.contains("quillo> 42\n"), "{}", text);
    assert!(text.contains("said\n"), "{}", text);
    assert!(!text.contains("main.qwe"), "{}", text);
    assert!(err(&o).is_empty(), "{}", err(&o));
    // A statement that gives nothing prints nothing: only the prompts, and the last line.
    assert!(text.ends_with("quillo> \n"), "{:?}", text);
}

#[test]
fn the_repl_is_what_quillo_alone_starts() {
    let dir = folder("repl-bare", &[]);
    let o = repl(&dir, &[], "\"strings show as code\"\n");
    assert!(o.status.success());
    assert!(out(&o).contains("quillo> \"strings show as code\"\n"), "{}", out(&o));
}

#[test]
fn the_repl_waits_for_the_rest_of_an_unfinished_statement() {
    let dir = folder("repl-more", &[]);
    let o = repl(&dir, &["repl"], "let y =\n5\ny\nfn f() {\n1 + 1\n}\nf()\n");
    assert!(o.status.success());
    let text = out(&o);
    assert!(text.contains("quillo> ....> quillo> 5\n"), "{}", text);
    assert!(text.contains("....> ....> quillo> 2\n"), "{}", text);
}

#[test]
fn the_repl_says_what_went_wrong_and_goes_on() {
    let dir = folder("repl-errors", &[]);
    let o = repl(&dir, &["repl"], "print(1) print(2) extra words\n1 / 0\nprint(\"first\"); nope(); print(\"not reached\")\n7\n");
    assert!(o.status.success());
    let (text, wrong) = (out(&o), err(&o));
    assert!(wrong.contains("error: line 1: expected"), "{}", wrong);
    assert!(wrong.contains("error: line 1: division by zero"), "{}", wrong);
    assert!(wrong.contains("error: line 1: unknown name `nope`"), "{}", wrong);
    assert!(text.contains("first\n"), "{}", text);
    assert!(!text.contains("not reached"), "an error ends the line: {}", text);
    assert!(text.contains("quillo> 7\n"), "{}", text);
}

#[test]
fn the_repl_shows_what_a_return_or_a_reply_would_give() {
    let dir = folder("repl-control", &[]);
    let o = repl(&dir, &["repl"], "return 4\nreply 404 \"gone\"\nnull\n");
    assert!(o.status.success());
    let text = out(&o);
    assert!(text.contains("quillo> 4\n"), "{}", text);
    assert!(text.contains("quillo> <reply 404>\n"), "{}", text);
    assert!(!text.contains("null"), "a value of null shows nothing: {}", text);
}

#[test]
fn the_repl_points_to_the_program_in_its_folder() {
    let dir = folder("repl-main", &[("main.qwe", PRINTS)]);
    let o = repl(&dir, &["repl"], "");
    assert!(o.status.success());
    assert!(out(&o).contains("main.qwe is here: `quillo run` serves it\n"), "{}", out(&o));
    assert!(!out(&o).contains("ran\n"), "the repl does not run it");
}

#[test]
fn run_of_a_file_that_is_not_there_fails() {
    let dir = folder("missing", &[]);
    let o = quillo(&dir, &["run", "nope.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("nope.qwe"), "{}", err(&o));
    let o = quillo(&dir, &["run", "nope.qwe", "--no-check"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo: "), "{}", err(&o));
    let o = quillo(&dir, &["run", "nope.qwe", "--split"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).starts_with("quillo: cannot read nope.qwe"), "{}", err(&o));
}

#[test]
fn a_command_beside_a_program_needs_the_programs_folder() {
    let dir = folder("with-nofolder", &[]);
    let (code, text) = refused(&dir, &["run", "gone/app.qwe", "--with", "echo hi"]);
    assert_eq!(code, 1);
    assert!(text.starts_with("quillo: --with \"echo hi\": "), "{}", text);
}

#[test]
fn a_short_signing_secret_is_warned_of() {
    let dir = folder("secret", &[("app.qwe", PRINTS)]);
    let o = command(&dir, &["run", "app.qwe"]).env("QUILLO_SECRET", "short").stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success());
    assert!(err(&o).contains("quillo: QUILLO_SECRET is 5 bytes; a signing secret should be at least"), "{}", err(&o));
}

#[test]
fn instances_of_a_program_with_nothing_to_serve_just_run() {
    let dir = folder("instances", &[("app.qwe", PRINTS)]);
    let o = quillo(&dir, &["run", "app.qwe", "--instances", "3"]);
    assert!(o.status.success(), "{}", err(&o));
    // Running it once shows there is nothing to serve; it stops there.
    assert_eq!(out(&o), "ran\n");
}

#[test]
fn a_program_an_instance_cannot_load_fails() {
    let dir = folder("instances-bad", &[("app.qwe", "fn zero() = 0\nroute GET / = \"hi\"\nprint(1 / zero())\n")]);
    let o = quillo(&dir, &["run", "app.qwe", "--instances", "2"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("division by zero"), "{}", err(&o));
}

#[test]
fn a_taken_port_is_reported() {
    let dir = folder("taken", &[("app.qwe", "route GET / = \"hi\"\n")]);
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port().to_string();
    for args in [&["run", "app.qwe", "--port", &port][..], &["run", "app.qwe", "--instances", "2", "--port", &port]] {
        let o = quillo(&dir, args);
        assert_eq!(o.status.code(), Some(1), "{:?}", args);
        assert!(err(&o).contains(&format!("quillo: cannot listen on 127.0.0.1:{}", port)), "{}", err(&o));
    }
}

#[test]
fn platform_says_how_the_program_runs_here() {
    let dir = folder("platform", &[("app.qwe", "print(json.stringify(platform()))\n")]);
    let o = command(&dir, &["run", "app.qwe"]).env("LC_ALL", "fr_CA.UTF-8").env("TZ", "Asia/Tokyo").stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let p: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(p["os"], std::env::consts::OS);
    assert_eq!(p["arch"], std::env::consts::ARCH);
    assert_eq!(p["runtime"], "server");
    assert_eq!(p["device"], "server");
    assert_eq!(p["simulator"], false);
    assert_eq!(p["locale"], "fr-CA");
    assert_eq!(p["time_zone"], "Asia/Tokyo");
    assert!(p["cpus"].as_u64().unwrap() >= 1);
    assert!(p["quillo"].as_str().unwrap().starts_with(env!("CARGO_PKG_VERSION")));
    // No locale to speak of.
    let o = command(&dir, &["run", "app.qwe"]).env("LC_ALL", "C").stdin(Stdio::null()).output().unwrap();
    let p: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(p["locale"].is_null(), "{}", p);
}

#[test]
fn an_empty_lc_all_leaves_the_locale_to_lang() {
    let dir = folder("locale", &[("app.qwe", "print(platform().locale)\n")]);
    let o = command(&dir, &["run", "app.qwe"]).env("LC_ALL", "").env_remove("LC_MESSAGES").env("LANG", "en_US.UTF-8").stdin(Stdio::null()).output().unwrap();
    assert_eq!(out(&o), "en-US\n");
}
