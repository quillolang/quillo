//! Errors and start-up (FEEDBACK16.md, FEEDBACK20.md, FEEDBACK21.md; branch feedback20-fixes): how
//! an error says how it got there, what the command line refuses, and what a phone app starts from.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use support::send;

fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback22-{}", name));
    for (f, src) in files {
        if let Some(up) = Path::new(f).parent() {
            std::fs::create_dir_all(d.join(up)).unwrap();
        }
        std::fs::write(d.join(f), src).unwrap();
    }
    d
}

fn quillo(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quillo")).args(args).current_dir(dir).env("HOME", dir).stdin(Stdio::null()).output().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A server in `dir`, its log in a file there; stopped when dropped.
fn serving(dir: &Path, args: &[&str]) -> (support::Guard, u16, PathBuf) {
    let port = support::free_port();
    let log = dir.join("log.txt");
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string()])
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let guard = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start: {}", std::fs::read_to_string(&log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(20));
    }
    (guard, port, log)
}

/// The log, once `what` is in it.
fn logged(log: &Path, what: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(log).unwrap();
        if text.contains(what) {
            return text;
        }
        assert!(Instant::now() < deadline, "no {:?} in:\n{}", what, text);
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---- Q-174: how an error got there -----------------------------------------------------------

const PRICE: &str = "public fn share(total, people) {\n    return total / people\n}\n";
const CHECKOUT: &str = "import price.qwe use share\n\nfn checkout(n) = share(100, n)\nfn split_bill(n) = share(200, n)\n";

#[test]
fn a_failed_program_says_how_it_got_there() {
    let d = folder("chain", &[("price.qwe", PRICE), ("main.qwe", &format!("{}\nprint(checkout(2))\nprint(split_bill(0))\n", CHECKOUT))]);
    let o = quillo(&d, &["run", "main.qwe"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stderr(&o), "quillo: price.qwe:2: division by zero\n  in share, called at main.qwe:4\n  in split_bill, called at main.qwe:7\n");
}

/// Hot functions run in machine code, where no call is seen.
#[test]
fn a_chain_comes_out_the_same_from_compiled_code() {
    let src = "fn rate(n) = 100 / n\nfn pay(n) = rate(n) + 1\nfn twice(n) = pay(n) * 2\nprint(twice(1))\nprint(twice(2))\nprint(twice(4))\nprint(twice(0))\n";
    let d = folder("chain-jit", &[("main.qwe", src)]);
    let o = quillo(&d, &["run", "main.qwe"]);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "202\n102\n52\n");
    assert_eq!(stderr(&o), "quillo: main.qwe:1: division by zero\n  in rate, called at main.qwe:2\n  in pay, called at main.qwe:3\n  in twice, called at main.qwe:7\n");
}

#[test]
fn a_deep_recursion_keeps_its_first_and_last_calls() {
    let src = "fn down(n, d) {\n    if n == 0 { return 1 / d }\n    down(n - 1, d)\n}\ndown(9000, 0)\n";
    let d = folder("chain-deep", &[("main.qwe", src)]);
    let text = stderr(&quillo(&d, &["run", "main.qwe"]));
    assert_eq!(text.lines().count(), 12, "{}", text);
    assert!(text.starts_with("quillo: main.qwe:2: division by zero\n  in down, called at main.qwe:3\n"), "{}", text);
    assert!(text.contains("\n  ... 8991 more calls\n"), "{}", text);
    assert!(text.ends_with("  in down, called at main.qwe:5\n"), "{}", text);
}

const FAILS: &str = "fn share(n) = 100 / n\nfn checkout(n) = share(n)\nroute GET /pay = checkout(0)\nserver fn pay() = checkout(0)\n";

#[test]
fn the_chain_is_for_the_log_and_not_for_a_visitor() {
    let d = folder("chain-log", &[("main.qwe", FAILS)]);
    let (_guard, port, log) = serving(&d, &[]);
    let a = send(port, "GET", "/pay", &[], "");
    assert_eq!((a.status, a.body.as_str()), (500, "internal error\n"));
    let text = logged(&log, "called at main.qwe:3");
    assert!(text.contains("main.qwe:1: division by zero\n  in share, called at main.qwe:2\n  in checkout, called at main.qwe:3\n"), "{}", text);
    let a = send(port, "POST", "/_quillo/rpc/pay", &[], "[]");
    assert_eq!(a.status, 500);
    assert!(!a.body.contains("qwe"), "{}", a.body);
    logged(&log, "error in server fn `pay`: main.qwe:1: division by zero\n  in share, called at main.qwe:2\n");
}

#[test]
fn under_watch_the_page_has_the_chain() {
    let d = folder("chain-watch", &[("main.qwe", FAILS)]);
    let (_guard, port, _) = serving(&d, &["--watch"]);
    let a = send(port, "GET", "/pay", &[], "");
    assert_eq!(a.body, "internal error: main.qwe:1: division by zero\n  in share, called at main.qwe:2\n  in checkout, called at main.qwe:3\n");
}

#[test]
fn a_log_of_objects_has_the_calls_as_a_field() {
    let d = folder("chain-json", &[("main.qwe", FAILS)]);
    let (_guard, port, log) = serving(&d, &["--log", "json"]);
    send(port, "GET", "/pay", &[], "");
    let text = logged(&log, "division by zero");
    let line = text.lines().find(|l| l.contains("division by zero")).unwrap();
    let j: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {}", e, line));
    assert_eq!(j["error"], "main.qwe:1: division by zero");
    assert_eq!(j["calls"], serde_json::json!(["in share, called at main.qwe:2", "in checkout, called at main.qwe:3"]));
}

#[test]
fn an_error_value_says_where_it_was_made_in_the_log_only() {
    let src = "import lib.qwe use find, load\nroute GET /mine = find()\nroute GET /db = load()\n";
    let lib = "public fn find() {\n    return err(\"not found\")\n}\npublic fn load() {\n    db.open(\":memory:\")\n    let rows = db.query(\"select nope\")?\n    rows\n}\n";
    let d = folder("value-place", &[("main.qwe", src), ("lib.qwe", lib)]);
    let (_guard, port, log) = serving(&d, &[]);
    let a = send(port, "GET", "/mine", &[], "");
    assert_eq!(a.status, 500);
    assert!(!a.body.contains("lib.qwe"), "{}", a.body);
    assert!(a.body.contains("not found"), "the program's own text reaches the visitor: {}", a.body);
    logged(&log, "lib.qwe:2: not found");
    let a = send(port, "GET", "/db", &[], "");
    assert_eq!(a.status, 500);
    assert!(a.body.contains("internal error") && !a.body.contains("nope"), "{}", a.body);
    logged(&log, "lib.qwe:6: db: no such column: nope");
}

/// Two errors with the same text are the same value wherever they were made.
#[test]
fn where_an_error_was_made_changes_nothing_a_program_sees() {
    let src = "let a = err(\"x\")\nlet b = err(\"x\")\nprint(a == b)\nprint(a)\nprint(str(a))\n";
    let d = folder("value-same", &[("main.qwe", src)]);
    let o = quillo(&d, &["run", "main.qwe"]);
    assert!(stderr(&o).is_empty(), "{}", stderr(&o));
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.starts_with("true\n"), "{}", out);
    assert!(!out.contains("main.qwe"), "{}", out);
}

// ---- Q-173: what is left out of a visitor's page ---------------------------------------------

const PAGE: &str = "fn share(a, b) = a / b\ncomponent Pay() {\n    state n = 7\n    <button @click={() => { n = share(n, 0) }}>pay {n}</button>\n}\nroute GET / = <html><body><Pay/></body></html>\n";

/// The scripts a page names, as they are served.
fn scripts(port: u16) -> Vec<(String, String)> {
    let page = send(port, "GET", "/", &[], "").body;
    let mut got = Vec::new();
    for part in page.split("/_quillo/").skip(1) {
        let end = part.find(['"', '\'', '<', ' ']).unwrap_or(part.len());
        let path = format!("/_quillo/{}", &part[..end]);
        if path.ends_with(".js") {
            let body = send(port, "GET", &path, &[], "").body;
            for inner in body.split('"').filter(|s| s.contains("/_quillo/src/")) {
                got.push((inner.to_string(), body.clone()));
            }
            got.push((path, body));
        }
    }
    assert!(!got.is_empty(), "{}", page);
    got
}

#[test]
fn only_a_development_page_carries_where_its_lines_came_from() {
    let d = folder("places", &[("main.qwe", PAGE)]);
    let (guard, port, _) = serving(&d, &["--watch"]);
    let loader = scripts(port);
    let at = loader.iter().find(|(p, _)| p.contains("/_quillo/src/")).map(|(p, _)| p.clone()).expect("a development bundle");
    let share = send(port, "GET", &format!("{}main.qwe/share.js", at), &[], "").body;
    assert!(share.contains("PLACES[\"main.qwe/share.js\"] = [\"main.qwe\", {"), "{}", share);
    let runtime = send(port, "GET", &format!("{}quillo.js", at), &[], "").body;
    assert!(runtime.contains("const placed ="), "the runtime as it is written");
    drop(guard);

    let (_guard, port, _) = serving(&d, &[]);
    for (path, body) in scripts(port) {
        assert!(!body.contains("PLACES") && !body.contains("placed"), "{}", path);
    }
}

// ---- Q-125, Q-139, Q-140: what the command line refuses --------------------------------------

const OK: &str = "print(\"ran\")\n";

#[test]
fn commands_with_one_program_refuse_a_second() {
    let d = folder("one-program", &[("ok.qwe", OK), ("bad.qwe", "fn f() = nope\n")]);
    for cmd in ["check", "docs", "build", "fix"] {
        let o = quillo(&d, &[cmd, "ok.qwe", "bad.qwe"]);
        assert_eq!(o.status.code(), Some(2), "{}", cmd);
        assert_eq!(stderr(&o), format!("quillo: unknown argument `bad.qwe`: `{}` takes one program\n", cmd));
    }
}

#[test]
fn commands_refuse_an_option_they_do_not_have() {
    let d = folder("unknown-option", &[("ok.qwe", OK)]);
    let o = quillo(&d, &["check", "ok.qwe", "--strict-nul"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(stderr(&o), "quillo: unknown option `--strict-nul` for `check` (did you mean `--strict-null`?)\n");
    assert!(o.stdout.is_empty(), "no `ok` for what was not checked");
    for cmd in ["docs", "build", "fix"] {
        let o = quillo(&d, &[cmd, "ok.qwe", "--strict-null"]);
        assert_eq!((o.status.code(), stderr(&o)), (Some(2), format!("quillo: unknown option `--strict-null` for `{}`\n", cmd)));
    }
    // `run`'s options are not `check`'s, and not offered to it.
    let o = quillo(&d, &["check", "ok.qwe", "--port", "8080"]);
    assert_eq!((o.status.code(), stderr(&o)), (Some(2), "quillo: unknown option `--port` for `check`\n".to_string()));
}

#[test]
fn what_check_does_know_still_works() {
    let d = folder("check-known", &[("ok.qwe", OK)]);
    assert!(quillo(&d, &["check", "--strict-null", "ok.qwe"]).status.success());
    assert!(quillo(&d, &["check", "ok.qwe", "--strict-null"]).status.success());
    let o = quillo(&d, &["check", "--help"]);
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("quillo check <file.qwe>"));
}

#[test]
fn a_file_alone_is_read_as_run_reads_it() {
    let d = folder("file-alone", &[("ok.qwe", OK), ("other.qwe", OK)]);
    assert!(quillo(&d, &["ok.qwe", "--port=8081"]).status.success());
    assert!(quillo(&d, &["ok.qwe", "--log", "off"]).status.success());
    let o = quillo(&d, &["ok.qwe", "--log=nope"]);
    assert_eq!((o.status.code(), stderr(&o)), (Some(2), "quillo: --log is text, json or off\n".to_string()));
    let o = quillo(&d, &["ok.qwe", "--port"]);
    assert_eq!((o.status.code(), stderr(&o)), (Some(2), "quillo: `--port` needs a value\n".to_string()));
}

#[test]
fn an_option_without_its_value_says_so() {
    let d = folder("no-value", &[("ok.qwe", OK)]);
    for option in ["--port", "--workers", "--env-file", "--log"] {
        let o = quillo(&d, &["run", "ok.qwe", option]);
        assert_eq!((o.status.code(), stderr(&o)), (Some(2), format!("quillo: `{}` needs a value\n", option)));
    }
    let o = quillo(&d, &["run", "ok.qwe", "--with", "  "]);
    assert_eq!((o.status.code(), stderr(&o)), (Some(2), "quillo: --with needs a command to run\n".to_string()));
    let o = quillo(&d, &["run", "ok.qwe", "--open", "/admin"]);
    assert_eq!((o.status.code(), stderr(&o)), (Some(2), "quillo: --open is for `quillo app`, which opens a window on that page\n".to_string()));
    // Offered the one it was near, as before, and never itself.
    let o = quillo(&d, &["run", "ok.qwe", "--prot", "1"]);
    assert_eq!(stderr(&o), "quillo: unknown option `--prot` (did you mean `--port`?)\n");
}

// ---- Q-129: the locale -----------------------------------------------------------------------

#[test]
fn the_locale_is_the_first_of_the_three_that_is_set() {
    let d = folder("locale", &[("ok.qwe", "print(platform().locale)\n")]);
    let locale = |vars: &[(&str, &str)]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
        c.args(["run", "ok.qwe"]).current_dir(&d).env_remove("LC_ALL").env_remove("LC_MESSAGES").env_remove("LANG").stdin(Stdio::null());
        String::from_utf8_lossy(&c.envs(vars.iter().copied()).output().unwrap().stdout).into_owned()
    };
    assert_eq!(locale(&[("LC_ALL", ""), ("LC_MESSAGES", ""), ("LANG", "en_US.UTF-8")]), "en-US\n");
    assert_eq!(locale(&[("LC_ALL", ""), ("LC_MESSAGES", "fr_CA.UTF-8"), ("LANG", "en_US.UTF-8")]), "fr-CA\n");
    assert_eq!(locale(&[("LC_ALL", "de_DE"), ("LC_MESSAGES", "fr_CA"), ("LANG", "en_US")]), "de-DE\n");
    assert_eq!(locale(&[("LC_ALL", ""), ("LANG", "")]), "null\n");
}

// ---- Q-169: the file a phone app starts from -------------------------------------------------

/// What `mobile/app-config.sh` makes of a program's folder, as the build scripts source it.
fn config(prog: &Path) -> Output {
    let root = env!("CARGO_MANIFEST_DIR");
    let script = format!("prog='{}'; . '{}/mobile/app-config.sh'; echo \"$APP_MAIN\"", prog.display(), root);
    Command::new("sh").args(["-c", &script, &format!("{}/mobile/ios/build.sh", root)]).output().unwrap()
}

fn id_and(line: &str) -> String {
    format!("id = \"org.example.notes\"\n{}\n", line)
}

#[test]
fn app_toml_names_the_file_a_phone_app_starts_from() {
    let d = folder("toml-default", &[("main.qwe", OK)]);
    assert_eq!(String::from_utf8_lossy(&config(&d).stdout), "main.qwe\n", "with no app.toml");
    std::fs::write(d.join("app.toml"), id_and("")).unwrap();
    assert_eq!(String::from_utf8_lossy(&config(&d).stdout), "main.qwe\n", "with no key");
    let d = folder("toml-main", &[("mobile.qwe", OK), ("apps/phone.qwe", OK), ("app.toml", &id_and("main = \"mobile.qwe\"   # the phone's"))]);
    assert_eq!(String::from_utf8_lossy(&config(&d).stdout), "mobile.qwe\n");
    std::fs::write(d.join("app.toml"), id_and("main = \"apps/phone.qwe\"")).unwrap();
    assert_eq!(String::from_utf8_lossy(&config(&d).stdout), "apps/phone.qwe\n");
}

#[test]
fn app_toml_refuses_a_file_that_cannot_be_started() {
    for (main, says) in [
        ("gone.qwe", "main gone.qwe: no such file"),
        ("../outside.qwe", "is not a .qwe file in the program's folder"),
        ("/etc/passwd.qwe", "is not a .qwe file in the program's folder"),
        ("notes.txt", "is not a .qwe file in the program's folder"),
    ] {
        let d = folder("toml-bad", &[("main.qwe", OK), ("notes.txt", "x"), ("app.toml", &id_and(&format!("main = \"{}\"", main)))]);
        let o = config(&d);
        assert_eq!(o.status.code(), Some(1), "{}", main);
        assert!(stderr(&o).contains(says), "{}: {}", main, stderr(&o));
    }
}

#[test]
fn the_launchers_take_the_file_from_app_toml() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("mobile");
    let read = |f: &str| std::fs::read_to_string(root.join(f)).unwrap();
    assert!(read("ios/build.sh").contains("plutil -replace QuilloMain -string \"$APP_MAIN\" \"$plist\""));
    assert!(read("android/build.sh").contains("QUILLO_APP_MAIN=\"$APP_MAIN\""));
    assert!(read("android/app/build.gradle.kts").contains("resValue(\"string\", \"quillo_main\", env(\"QUILLO_APP_MAIN\", \"main.qwe\"))"));
    assert!(read("android/app/src/main/java/org/quillolang/app/MainActivity.kt").contains("Quillo.start(bundle.path, data.path, activity.getString(R.string.quillo_main), device(activity))"));
}

/// The page at `/`, as an app's web view gets it.
fn on_the_phone(s: &quillo::mobile::Started) -> String {
    let launch = &s.url[s.url.find("/?").unwrap()..];
    let first = send(s.port, "GET", launch, &[], "");
    let cookie = first.header("set-cookie").expect("the launch URL gives a cookie");
    send(s.port, "GET", "/", &[("Cookie", cookie.split(';').next().unwrap())], "").body
}

#[test]
fn a_phone_app_starts_from_the_file_it_is_told_and_its_files_are_beside_that() {
    let d = folder(
        "phone",
        &[
            ("bundle/main.qwe", "route GET / = \"the wrong one\"\n"),
            ("bundle/apps/phone.qwe", "import ../lib/api.qwe use hello\nwrite_file(\"note.txt\", \"mine\")\nroute GET / = hello()\n"),
            ("bundle/lib/api.qwe", "public fn hello() = \"from the phone's file\"\n"),
        ],
    );
    let data = d.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let s = quillo::mobile::start(&d.join("bundle"), &data, "apps/phone.qwe", "").unwrap();
    assert_eq!(on_the_phone(&s), "from the phone's file");
    assert!(data.join("app/apps/note.txt").is_file(), "beside the program, as `quillo run` has it");
    assert_eq!(quillo::mobile::start(&d.join("bundle"), &data, "gone.qwe", "").err().unwrap(), "no gone.qwe in the app's program");
}
