//! Slice A2 of FEEDBACK86-89 (Quillo 0.8.12; branch feedback86-89), what `quillo check` says:
//! FEEDBACK86.md's Q-1361, a state's initializer read by binding, so a lambda's parameter hides a
//! state's name only inside that lambda; FEEDBACK35.md's Q-431 warning ("a value from the page shown
//! in markup") for the component's own `let`s only, matched by binding (0.8.11's, 18 false on
//! quillolang.org); FEEDBACK45.md's Q-523, a name taken `as` another that the file binds too, said
//! as the file wrote it.

mod support;

use quillo::check::{check_file, check_source, Diagnostic, Severity};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::path::{Path, PathBuf};
use support::temp_dir;

fn shown(d: &Diagnostic) -> String {
    let sev = match d.severity {
        Severity::Error => " error:",
        Severity::Warning => " warning:",
        Severity::Hint => " hint:",
    };
    format!("{}{} {}", d.line, sev, d.message)
}

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(shown).collect()
}

/// The lines of the diagnostics that hold `part`, of a program with no error.
fn lines(src: &str, part: &str) -> Vec<u32> {
    let all = diags(src);
    assert!(!all.iter().any(|d| d.contains(" error:")), "an error in a program meant to have none:\n{}\n--- source ---\n{}", all.join("\n"), src);
    all.iter().filter(|d| d.contains(part)).map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect()
}

/// A folder of files for one test, gone when it ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, files: &[(&str, &str)]) -> App {
        let d = temp_dir(&format!("f812-check2-{}", name));
        for (f, text) in files {
            std::fs::write(d.join(f), text).unwrap();
        }
        App(d.canonicalize().unwrap())
    }

    /// Each diagnostic with its file's name.
    fn check(&self) -> Vec<String> {
        let dir = format!("{}/", self.0.display());
        check_file(&self.0.join("main.qwe")).diagnostics.iter().map(|d| format!("{}:{}", d.file.strip_prefix(&dir).unwrap_or(&d.file), shown(d))).collect()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- Q-1361: a state's initializer, by binding ------------------------------------------------

const SET_ONCE: &str = "is set once, as <C> starts";

/// A component whose handler assigns the state `a`, and `state b = <init>`.
fn follows(init: &str) -> String {
    format!(
        "component C() {{\n    state a = [1, 2]\n    state b = {}\n    <div>\n        <button onclick={{() => {{ a = [3, 4, 5] }}}}>x</button>\n        <p>{{b}}</p>\n    </div>\n}}\nroute GET / = <html><body><C/></body></html>\n",
        init
    )
}

#[test]
fn q1361_a_lambdas_parameter_hides_the_state_only_inside_the_lambda() {
    // The review's d1: `a` read outside the lambda, which binds an `a` of its own.
    assert_eq!(lines(&follows("len(a) + len([3].map((a) => a))"), SET_ONCE), vec![3]);
    // The lambda first, the state after it.
    assert_eq!(lines(&follows("len([3].map((a) => a)) + len(a)"), SET_ONCE), vec![3]);
}

#[test]
fn q1361_nested_lambdas_and_a_for_inside_a_lambda() {
    assert_eq!(lines(&follows("len(a) + len([3].map((x) => [x].map((a) => a)))"), SET_ONCE), vec![3]);
    assert_eq!(lines(&follows("len(a) + len([[1]].map((x) => for a in x => a))"), SET_ONCE), vec![3]);
    // A block's `let a` hides it in the block, not after it.
    assert_eq!(lines(&follows("len({ let a = [9]\n a }) + len(a)"), SET_ONCE), vec![3]);
}

#[test]
fn q1361_what_stays_unwarned_where_only_the_bindings_inside_read_the_name() {
    for init in [
        "len([3].map((a) => a))",
        "[3].map((a) => a + 1)",
        "[[1]].map((x) => for a in x => a)",
        "[3].map((x) => [x].map((a) => a))",
        "{ let a = [9]\n len(a) }",
        "len([])",
    ] {
        assert_eq!(lines(&follows(init), SET_ONCE), Vec::<u32>::new(), "{}", init);
    }
}

// ---- Q-431: a value from the page shown in markup, by binding --------------------------------

const PAGE_VALUE: &str = "is a value from the page shown in markup";

/// The review's f1: a handler's `let r = js(...)`, and a `for r` in the markup.
const F1: &str = r#"component C() {
    state said = ""
    let share = () => {
        let r = js("Promise.resolve", 1)
        said = is_err(r) ? "no" : "yes"
    }
    <div>
        <button onclick={share}>go</button>
        {for r in ["a", "b"] => <span>{r}</span>}
        <p>{said}</p>
    </div>
}
route GET / = <html><body><C/></body></html>
"#;

#[test]
fn q431_a_handlers_let_is_no_value_of_the_component() {
    assert_eq!(lines(F1, PAGE_VALUE), Vec::<u32>::new());
    // Shown by name, with no `for`: still the handler's own.
    let direct = F1.replace(r#"{for r in ["a", "b"] => <span>{r}</span>}"#, "{share}");
    assert_eq!(lines(&direct, PAGE_VALUE), Vec::<u32>::new());
}

#[test]
fn q431_nested_lambdas_and_a_local_fn_are_not_the_components() {
    let nested = r#"component C() {
    state said = ""
    let share = () => {
        let go = () => {
            let r = js("Date.now")
            said = str(r)
        }
        go()
    }
    fn later() {
        let r = js("Date.now")
        said = str(r)
    }
    <div>
        <button onclick={share}>go</button>
        <button onclick={later}>later</button>
        {for r in ["a", "b"] => <span>{r}</span>}
        <p>{said}</p>
    </div>
}
route GET / = <html><body><C/></body></html>
"#;
    assert_eq!(lines(nested, PAGE_VALUE), Vec::<u32>::new());
}

/// A component with its own `let r = js(...)` and `markup` in its <div>.
fn page_value(markup: &str) -> String {
    format!("component C() {{\n    let r = js(\"Date.now\")\n    <div>\n        {}\n    </div>\n}}\nroute GET / = <html><body><C/></body></html>\n", markup)
}

#[test]
fn q431_the_components_own_let_shown_still_warns() {
    // The form that must keep working.
    assert_eq!(lines(&page_value("<p>{r}</p>"), PAGE_VALUE), vec![4]);
    assert_eq!(lines(&page_value("<p>{str(r)} ms</p>"), PAGE_VALUE), vec![4]);
    // Hidden in one place and shown in another: said once, where it is shown.
    assert_eq!(lines(&page_value("{for r in [1] => <i>{r}</i>}\n        <p>{r}</p>"), PAGE_VALUE), vec![5]);
}

#[test]
fn q431_a_for_or_a_lambda_in_markup_that_hides_the_components_let_is_not_it() {
    for markup in [
        "{for r in [1, 2] => <i>{r}</i>}",
        "{[1, 2].map((r) => r + 1)}",
        "{[[1]].map((x) => for r in x => <i>{r}</i>)}",
        "{[1].map((x) => [x].map((r) => r))}",
    ] {
        assert_eq!(lines(&page_value(markup), PAGE_VALUE), Vec::<u32>::new(), "{}", markup);
    }
}

// ---- Q-523: a name taken `as` another that the file binds too --------------------------------

const LIB: &str = "public let inv = 0\npublic fn bump() {\n    inv += 1\n    return inv\n}\npublic fn price(n: int) -> int {\n    return n * 2\n}\n";

#[test]
fn q523_the_files_own_binding_is_said_as_written_not_as_the_other_files_name() {
    // The review's main3: 0.8.11 said "unused variable `inv` (name it `_inv` ...)".
    let app = App::new(
        "main3",
        &[("lib.qwe", LIB), ("main.qwe", "import lib.qwe use inv as total, bump\nfn a() {\n    let total = 3\n    return 1\n}\nroute GET / = str([a(), bump()])\n")],
    );
    let said = app.check();
    assert!(said.iter().any(|d| d == "main.qwe:3 warning: unused variable `total` (name it `_total` if that is intended)"), "{:#?}", said);
    assert!(!said.iter().any(|d| d.contains("`inv`") || d.contains('\u{2}')), "{:#?}", said);
}

#[test]
fn q523_two_aliases_of_one_name_bound_each_by_its_binding() {
    // Changed twice: round 2 expected each `let` said apart; round 3, where messages were rewritten,
    // slice A's `inv`. Now each binding is told by its declaration (review of round 3: take the name
    // from where it is written): linking makes the two `let`s one variable, the second, `tally`.
    let app = App::new(
        "two",
        &[("lib.qwe", LIB), ("main.qwe", "import lib.qwe use inv as total, inv as tally, bump\nfn a() {\n    let total = 3\n    let tally = 4\n    return 1\n}\nroute GET / = str([a(), bump(), total, tally])\n")],
    );
    let said: Vec<String> = app.check().into_iter().filter(|d| d.contains("unused variable")).collect();
    assert_eq!(said, vec!["main.qwe:4 warning: unused variable `tally` (name it `_tally` if that is intended)".to_string()]);
}

#[test]
fn q523_the_other_files_value_through_the_alias_still_works_beside_a_binding() {
    // Read, assigned and shown through the alias in other functions, a `for` and a lambda's
    // parameter of that name: nothing said.
    let main = "import lib.qwe use inv as total, bump\nfn a() {\n    let total = \"x\"\n    return total\n}\nfn b() {\n    total = 5\n    return total + 1\n}\nlet f = () => {\n    let g = (total) => total\n    let n = 0\n    for total in [1, 2] {\n        n += total\n    }\n    return g(n)\n}\nroute GET / = str([a(), b(), f(), bump(), total])\n";
    let app = App::new("uses", &[("lib.qwe", LIB), ("main.qwe", main)]);
    assert_eq!(app.check(), Vec::<String>::new());
}

#[test]
fn q523_a_call_through_the_alias_says_the_alias_and_the_old_name_says_both() {
    let main = "import lib.qwe use price as p\nfn a(p: int) {\n    return p\n}\nfn b() {\n    return p(\"x\")\n}\nroute GET / = str([a(1), b(), p(1, 2), price(3)])\n";
    let app = App::new("call", &[("lib.qwe", LIB), ("main.qwe", main)]);
    let said = app.check();
    for want in [
        "main.qwe:6 error: argument `n` of `p` must be int, got str",
        "main.qwe:8 error: `p` takes 1 argument, got 2",
        // The module's name where the program wrote it.
        "main.qwe:8 warning: this file takes `price` as `p`: write `p`, or take `price` too",
    ] {
        assert!(said.iter().any(|d| d == want), "no {:?} in {:#?}", want, said);
    }
    assert!(!said.iter().any(|d| d.contains('\u{2}')), "{:#?}", said);
}

#[test]
fn q523_an_alias_the_file_does_not_bind_is_said_as_written_too() {
    // Changed from `q523_an_alias_the_file_does_not_bind_is_renamed_as_before`, which held that a
    // file not binding the alias kept 0.8.11's renaming (and so its messages naming `price`): the
    // coordinator's decision of 2026-11-26 on slice A2 marks every lowercase alias, as a message
    // naming `price` where the program wrote `p` is this finding. Its other names stay untouched.
    let main = "import lib.qwe use price as p, bump\nfn a(n: int) {\n    let unused = 1\n    return n\n}\nroute GET / = str([a(1), p(2), bump(), p(\"x\")])\n";
    let app = App::new("plain", &[("lib.qwe", LIB), ("main.qwe", main)]);
    assert_eq!(
        app.check(),
        vec![
            "main.qwe:3 warning: unused variable `unused` (name it `_unused` if that is intended)".to_string(),
            "main.qwe:6 error: argument `n` of `p` must be int, got str".to_string(),
        ]
    );
}

#[test]
fn q523_unbound_alias_calls_say_the_alias_and_the_old_name_says_price() {
    let main = "import lib.qwe use price as p\nfn b() {\n    return p(\"x\")\n}\nroute GET / = str([b(), p(1, 2), price(3)])\n";
    let app = App::new("unbound", &[("lib.qwe", LIB), ("main.qwe", main)]);
    let said = app.check();
    for want in [
        "main.qwe:3 error: argument `n` of `p` must be int, got str",
        "main.qwe:5 error: `p` takes 1 argument, got 2",
        "main.qwe:5 warning: this file takes `price` as `p`: write `p`, or take `price` too",
    ] {
        assert!(said.iter().any(|d| d == want), "no {:?} in {:#?}", want, said);
    }
    assert!(!said.iter().any(|d| d.contains("of `price`") || d.contains("`price` takes") || d.contains('\u{2}')), "{:#?}", said);
}

#[test]
fn q523_two_unbound_aliases_of_one_function_each_said_as_written() {
    // Back to round 2's expectation (round 3 kept slice A's `price`, as rewritten text could not
    // tell the two apart): each call is told by its own node.
    let main = "import lib.qwe use price as p, price as q\nroute GET / = str([p(1, 2), q(\"x\")])\n";
    let app = App::new("unbound2", &[("lib.qwe", LIB), ("main.qwe", main)]);
    let said = app.check();
    for want in ["main.qwe:2 error: `p` takes 1 argument, got 2", "main.qwe:2 error: argument `n` of `q` must be int, got str"] {
        assert!(said.iter().any(|d| d == want), "no {:?} in {:#?}", want, said);
    }
}

#[test]
fn q523_an_alias_given_as_a_value_and_an_unknown_name_near_it() {
    // Passed as a value, and called through a `let`, it is still the other file's function; a
    // misspelling suggests what the file can write.
    let main = "import lib.qwe use price as cost\nlet f = cost\nroute GET / = str([f(2), [1].map(cost), cots(1)])\n";
    let app = App::new("value", &[("lib.qwe", LIB), ("main.qwe", main)]);
    let said = app.check();
    assert!(said.iter().all(|d| !d.contains('\u{2}') && !d.contains("price")), "{:#?}", said);
    assert!(said.iter().any(|d| d.contains("unknown name `cots`")), "{:#?}", said);
}

#[test]
fn q523_in_a_used_file_too() {
    let mid = "import lib.qwe use inv as total, bump\npublic fn go() {\n    let total = 3\n    return bump()\n}\n";
    let app = App::new("used", &[("lib.qwe", LIB), ("mid.qwe", mid), ("main.qwe", "import mid.qwe use go\nroute GET / = str(go())\n")]);
    let said = app.check();
    assert!(said.iter().any(|d| d == "mid.qwe:3 warning: unused variable `total` (name it `_total` if that is intended)"), "{:#?}", said);
}

#[test]
fn q523_unicode_alias() {
    let main = "import lib.qwe use inv as größe, bump\nfn a() {\n    let größe = 3\n    return 1\n}\nroute GET / = str([a(), bump(), größe])\n";
    let app = App::new("unicode", &[("lib.qwe", LIB), ("main.qwe", main)]);
    let said = app.check();
    assert!(said.iter().any(|d| d == "main.qwe:3 warning: unused variable `größe` (name it `_größe` if that is intended)"), "{:#?}", said);
}

// ---- The review of A2 round 2: what the mark in the program broke --------------------------------

const SAVE: &str = "public fn save() {\n    db.exec(\"insert into t (n) values (1)\")\n    return 1\n}\n";

#[test]
fn q523_review_a_get_route_writing_through_an_alias_is_still_said() {
    // FEEDBACK70.md's Q-965 through `use save as keep`: the review's getw/main.qwe.
    let main = "import lib.qwe use save as keep\ndb.open(\":memory:\")\ndb.exec(\"create table if not exists t (n int)\")\nroute GET /k {\n    keep()\n    return \"ok\"\n}\n";
    let app = App::new("getw", &[("lib.qwe", SAVE), ("main.qwe", main)]);
    let said = app.check();
    assert!(said.iter().any(|d| d.starts_with("main.qwe:4 warning: `GET /k` writes") && d.contains("in `save` at lib.qwe:2")), "{:#?}", said);
    // A route's parameter of the alias's name is said as written, the function's own place by its name.
    let main = "import lib.qwe use save as p\nserver fn twice(p: int) -> int {\n    return p * 2\n}\nroute GET /t/:p = str(twice(int(p) ?? 0))\n";
    let app = App::new("getw2", &[("lib.qwe", SAVE), ("main.qwe", main)]);
    let said = app.check();
    assert!(said.iter().any(|d| d.starts_with("main.qwe:5 warning: `GET /t/:p` writes") && d.contains("in `save` at lib.qwe:2") && d.contains("route writes GET /t/:p")), "{:#?}", said);
}

#[test]
fn q523_review_a_tools_parameter_of_the_aliass_name_is_taken() {
    // The review's getw/mcp3.qwe: refused with U+0002 in the tool's schema.
    let main = "import lib.qwe use save as p\ndb.open(\":memory:\")\ndb.exec(\"create table if not exists t (n int)\")\n/// Twice the number.\nfn mcp go(p: int) -> int {\n    return p * 2\n}\n";
    let app = App::new("mcp3", &[("lib.qwe", SAVE), ("main.qwe", main)]);
    let said = app.check();
    assert!(!said.iter().any(|d| d.contains(" error:") || d.contains('\u{2}')), "{:#?}", said);
}

#[test]
fn q523_review_an_alias_and_the_original_bound_in_one_scope() {
    // The review's both/main.qwe: linking makes `p` and `price` one variable, so `p * 2` multiplies
    // the text, as the program does when it runs.
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\n";
    let main = "import lib.qwe use price as p\nfn a() -> int {\n    let p = 1\n    let price = \"x\"\n    return p * 2\n}\nroute GET / = str(a())\n";
    let app = App::new("both", &[("lib.qwe", lib), ("main.qwe", main)]);
    assert_eq!(app.check(), vec!["main.qwe:5 error: cannot multiply str and int".to_string()]);
}

/// `quillo lsp` on a folder, with a deadline: killed after 60 s.
struct Lsp {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
    done: std::sync::mpsc::Sender<()>,
}

impl Lsp {
    fn start() -> Lsp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("lsp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let pid = child.id().to_string();
        std::thread::spawn(move || {
            if wait.recv_timeout(std::time::Duration::from_secs(60)) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                let _ = Command::new("kill").args(["-9", &pid]).status();
            }
        });
        Lsp { child, out, next: 1, done }
    }

    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut len = 0;
        loop {
            let mut line = String::new();
            assert!(self.out.read_line(&mut line).unwrap() > 0, "the language server ended");
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0; len];
        self.out.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let m = self.recv();
            if m.get("id") == Some(&json!(id)) {
                return m["result"].clone();
            }
        }
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.done.send(());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn q523_review_an_editor_is_told_the_alias_with_no_mark() {
    // The review's lsp case: completion, hover, signatureHelp and symbols of a file with
    // `use price as p` and parameters and locals named `p`.
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\n";
    let text = "import lib.qwe use price as p\nfn a(p: int) -> int {\n    let q = p + 1\n    return q\n}\nfn b() -> int {\n    let p = 3\n    return p\n}\nroute GET / = str([a(1), b(), p(2)])\n";
    let app = App::new("lsp", &[("lib.qwe", lib), ("main.qwe", text)]);
    let uri = format!("file://{}", app.0.join("main.qwe").display());
    let mut c = Lsp::start();
    c.request("initialize", json!({ "rootUri": format!("file://{}", app.0.display()), "capabilities": {} }));
    c.send(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    c.send(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } } }));
    let at = |line: u32, ch: u32| json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": ch } });
    let hover = c.request("textDocument/hover", at(2, 12));
    let completion = c.request("textDocument/completion", at(2, 13));
    let signature = c.request("textDocument/signatureHelp", at(9, 32));
    let call = c.request("textDocument/hover", at(9, 30));
    let symbols = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } }));
    for v in [&hover, &completion, &signature, &call, &symbols] {
        assert!(!v.to_string().contains("\\u0002") && !v.to_string().contains('\u{2}'), "{}", v);
    }
    assert_eq!(hover["contents"]["value"], json!("```quillo\nparam p: int\n```"));
    let items = completion.as_array().cloned().unwrap_or_else(|| completion["items"].as_array().cloned().unwrap_or_default());
    let detail = |label: &str| items.iter().find(|i| i["label"] == json!(label)).map(|i| i["detail"].clone());
    assert_eq!(detail("p"), Some(json!("param p: int")));
    // This file's own function's signature names its parameter as written (back to round 3's
    // expectation, for the review of round 4, LOW 3).
    assert_eq!(detail("a"), Some(json!("fn a(p: int) -> int")));
    assert_eq!(signature["signatures"][0]["label"], json!("fn p(n: int) -> int"));
    assert_eq!(signature["signatures"][0]["parameters"][0]["label"], json!([5, 11]));
    assert_eq!(call["contents"]["value"], json!("```quillo\nfn p(n: int) -> int\n```"));
    let a = symbols.as_array().unwrap().iter().find(|s| s["name"] == json!("a")).unwrap();
    assert_eq!(a["detail"], json!("fn a(p: int) -> int"));
}

/// Opens `files` in `quillo lsp` and gives each request's result: `(method, line, character)`,
/// then documentSymbol last.
fn lsp_results(name: &str, files: &[(&str, &str)], asks: &[(&str, u32, u32)]) -> Vec<Value> {
    let app = App::new(name, files);
    let text = files.iter().find(|(f, _)| *f == "main.qwe").unwrap().1;
    let uri = format!("file://{}", app.0.join("main.qwe").display());
    let mut c = Lsp::start();
    c.request("initialize", json!({ "rootUri": format!("file://{}", app.0.display()), "capabilities": {} }));
    c.send(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    c.send(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": text } } }));
    let mut out: Vec<Value> = asks
        .iter()
        .map(|(m, line, ch)| c.request(&format!("textDocument/{}", m), json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": ch } })))
        .collect();
    out.push(c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": uri } })));
    for v in &out {
        assert!(!v.to_string().contains('\u{2}') && !v.to_string().contains("\\u0002"), "{}", v);
    }
    out
}

fn hover_text(v: &Value) -> String {
    v["contents"]["value"].as_str().unwrap_or("").to_string()
}

// ---- The review of A2 round 3: words that are not the alias ------------------------------------

#[test]
fn q523_review_f1_a_field_named_as_the_function_is_suggested_as_the_field() {
    let lib = "public type Item = { price: int, name: str }\npublic fn price(n: int) -> int {\n    return n * 2\n}\npublic fn make() -> Item {\n    return { price: 1, name: \"a\" }\n}\n";
    let main = "import lib.qwe use price as p, Item, make\nfn f(i: Item) -> int {\n    return i.prise + p(1)\n}\nfn g() -> int {\n    return make().prise\n}\nroute GET / = str(f(make()) + g())\n";
    let said = App::new("f1", &[("lib.qwe", lib), ("main.qwe", main)]).check();
    for line in [3, 6] {
        assert!(said.iter().any(|d| d == &format!("main.qwe:{} error: this map has no field `prise` (did you mean `price`?)", line)), "{:#?}", said);
    }
    assert!(!said.iter().any(|d| d.contains("`p`")), "{:#?}", said);
}

#[test]
fn q523_review_f2_a_name_the_file_wrote_with_the_aliass_letters_stays_as_written() {
    let lib = "public fn total(n: int) -> int {\n    return n * 2\n}\npublic fn show(total: str) -> str {\n    return total\n}\n";
    let main = "import lib.qwe use total as tot\nfn f() -> int {\n    return _total + tot(2)\n}\nfn g(xs: list) -> int {\n    for _total in xs {\n        print(_total)\n    }\n    return _total2\n}\nroute GET / = str(f() + g([1]))\n";
    let said = App::new("f2", &[("lib.qwe", lib), ("main.qwe", main)]).check();
    assert!(said.iter().any(|d| d.starts_with("main.qwe:3 error: unknown name `_total`")), "{:#?}", said);
    assert!(said.iter().any(|d| d.starts_with("main.qwe:9 error: unknown name `_total2`")), "{:#?}", said);
    assert!(!said.iter().any(|d| d.contains("_tot`") || d.contains("`tot`")), "{:#?}", said);
    // The other file's parameter `total`, in an editor, as it is declared.
    let r = lsp_results("f2b", &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use total as tot, show\nroute GET / = show(str(tot(1)))\n")], &[("hover", 1, 15), ("signatureHelp", 1, 19)]);
    assert_eq!(hover_text(&r[0]), "```quillo\nfn show(total: str) -> str\n```");
    assert_eq!(r[1]["signatures"][0]["label"], json!("fn show(total: str) -> str"));
}

#[test]
fn q523_review_f5_a_call_is_told_by_its_alias_where_the_text_also_has_the_word() {
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\n";
    let main = "import lib.qwe use price as p\nfn f() -> str {\n    return \"price\" + p(1)\n}\nroute GET / = f()\n";
    let r = lsp_results("f5", &[("lib.qwe", lib), ("main.qwe", main)], &[("hover", 2, 22), ("signatureHelp", 2, 23)]);
    assert_eq!(hover_text(&r[0]), "```quillo\nfn p(n: int) -> int\n```");
    assert_eq!(r[1]["signatures"][0]["label"], json!("fn p(n: int) -> int"));
    let said = App::new("f5c", &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use price as p\nfn f() -> str {\n    return \"price\" + str(p(\"x\"))\n}\nroute GET / = f()\n")]).check();
    assert!(said.iter().any(|d| d == "main.qwe:3 error: argument `n` of `p` must be int, got str"), "{:#?}", said);
}

#[test]
fn q523_review_f6_another_files_sql_is_quoted_as_it_is() {
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\npublic fn bump() {\n    db.exec(\"update items set price = 1\")\n    return 1\n}\n";
    let main = "import lib.qwe use price as p, bump\ndb.open(\":memory:\")\nroute GET /b {\n    bump()\n    return str(p(1))\n}\n";
    let said = App::new("f6", &[("lib.qwe", lib), ("main.qwe", main)]).check();
    assert!(said.iter().any(|d| d.contains("`db.exec(\"update items set price = 1\")`, in `bump` at lib.qwe:5")), "{:#?}", said);
}

#[test]
fn q523_review_f7_both_halves_of_a_message_say_what_the_file_wrote() {
    let lib = "public let hits = 0\npublic fn tick() {\n    hits = hits + 1\n    return hits\n}\n";
    let main = "import lib.qwe use hits as h, tick\ncomponent C() {\n    <p>{h}</p>\n}\nroute GET / {\n    tick()\n    return <html><body><C/></body></html>\n}\n";
    let said = App::new("f7", &[("lib.qwe", lib), ("main.qwe", main)]).check();
    assert!(said.iter().any(|d| d.starts_with("main.qwe:3 error: <C> runs in the browser, but `h` is a server variable") && d.contains("write `state h = ...`")), "{:#?}", said);
}

#[test]
fn q523_review_l1_unicode_names_in_an_editor() {
    let lib = "public fn größe(länge: int, b: str) -> int {\n    return länge\n}\npublic fn ñame(x: int) -> int {\n    return x\n}\n";
    let main = "import lib.qwe use größe as gé, ñame\nroute GET / = str(gé(1, \"x\") + ñame(2))\n";
    let r = lsp_results("l1", &[("lib.qwe", lib), ("main.qwe", main)], &[("hover", 1, 18), ("signatureHelp", 1, 21), ("hover", 1, 31)]);
    assert_eq!(hover_text(&r[0]), "```quillo\nfn gé(länge: int, b: str) -> int\n```");
    assert_eq!(r[1]["signatures"][0]["label"], json!("fn gé(länge: int, b: str) -> int"));
    assert_eq!(r[1]["signatures"][0]["parameters"][0]["label"], json!([6, 16]));
    assert_eq!(hover_text(&r[2]), "```quillo\nfn ñame(x: int) -> int\n```");
}

#[test]
fn q523_review_l2_another_files_symbols_keep_their_declared_details() {
    let lib = "public type Item = { price: int, name: str }\npublic fn price(n: int) -> int {\n    return n * 2\n}\npublic fn total(n: int) -> int {\n    return n\n}\npublic fn show(total: str, price: int) -> str {\n    return total\n}\n";
    let main = "import lib.qwe use price as p, total as t, show, Item\nfn f(i: Item) -> str {\n    return show(\"a\", p(t(1)))\n}\nroute GET / = f({ name: \"x\" })\n";
    let r = lsp_results("l2", &[("lib.qwe", lib), ("main.qwe", main)], &[("hover", 2, 11), ("signatureHelp", 2, 16), ("hover", 2, 22), ("hover", 2, 24), ("completion", 2, 20)]);
    assert_eq!(hover_text(&r[0]), "```quillo\nfn show(total: str, price: int) -> str\n```");
    assert_eq!(r[1]["signatures"][0]["label"], json!("fn show(total: str, price: int) -> str"));
    assert_eq!(hover_text(&r[2]), "```quillo\nfn p(n: int) -> int\n```");
    assert_eq!(hover_text(&r[3]), "```quillo\nfn t(n: int) -> int\n```");
    let items = r[4].as_array().cloned().unwrap_or_default();
    let i = items.iter().find(|x| x["label"] == json!("i")).unwrap();
    assert_eq!(i["detail"], json!("param i: { name: str, price: int }"));
}

#[test]
fn q523_review_lsp3_a_routes_symbol_says_its_parameter_as_written() {
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\n";
    let main = "import lib.qwe use price as p\nroute GET /t/:p = str(p)\nroute GET /u = str(p(1))\n";
    let r = lsp_results("lsp3", &[("lib.qwe", lib), ("main.qwe", main)], &[]);
    let t = r[0].as_array().unwrap().iter().find(|s| s["name"].as_str().is_some_and(|n| n.starts_with("GET /t"))).unwrap().clone();
    assert_eq!((t["name"].clone(), t["detail"].clone()), (json!("GET /t/:p"), json!("route GET /t/:p")));
}

// ---- The review of A2 round 3: Q-431's gaps ------------------------------------------------------

#[test]
fn q431_review_a_components_prop_it_shows_is_a_page_value_an_elements_attribute_is_not() {
    // q4 changed from a warning at line 3 for the coordinator's decision on A2 round 5: a page
    // value in an HTML element's attribute keeps the browser's value without remaking the component
    // (runtime.js `attrDiffers`, FEEDBACK55.md Q-774), so nothing is said. q1 stays: Child shows `v`.
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    <div title={r}>x</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    assert_eq!(lines("component Child(v: any) {\n    <p>{v}</p>\n}\ncomponent C() {\n    let r = js(\"document.title\")\n    <div>\n        <Child v={r}/>\n    </div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![7]);
    // A handler of an HTML element is no showing.
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    <div><button onclick={() => print(r)}>x</button></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
}

#[test]
fn q431_review_a_let_held_lambda_called_in_markup_shows_what_it_reads() {
    // q10.
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    let shown = () => r\n    <div>{shown()}</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![4]);
    // Its own parameter of the name is not the page value.
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    let shown = (r) => r\n    <div>{shown(1)} {str(r)}</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![4]);
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    let shown = (r) => r\n    <div>{shown(1)}</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
}

#[test]
fn q431_review_a_callback_a_component_calls_later_is_no_showing() {
    // q17: `onPick` is called in K's handler, so not as K is drawn.
    assert_eq!(lines("component K(onPick: fn) {\n    <button onclick={() => onPick(1)}>x</button>\n}\ncomponent C() {\n    let r = js(\"document.title\")\n    <div>{<K onPick={(i) => print(r)}/>}</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    // A view that calls it as it is drawn: shown (FEEDBACK66.md, Q-952's case).
    assert_eq!(lines("view Row(onShow) = <p>{onShow()}</p>\ncomponent C() {\n    let w = js(\"Math.abs\", -2)\n    <div><Row onShow={() => w}/></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![4]);
}

// ---- The review of A2 round 4 -------------------------------------------------------------------

/// a.qwe takes `price` as `p` and calls it on `n` lines; main.qwe calls its own `zed` wrongly on
/// `n` lines after: the review's s1 and s3 shapes.
fn many_files(n: usize, a_return: bool) -> App {
    let mut a = String::from("import lib.qwe use price as p\n\nlet a0 = 1\n");
    if a_return {
        a.push_str("return\n");
    }
    for i in 0..n {
        a.push_str(&format!("let a{} = p({}) + p(p({}))\n", i + 1, i, i));
    }
    let mut main = String::from("import a.qwe use a0\n\nfn zed(x: int) -> int {\n    return x\n}\n\n");
    for i in 0..n {
        main.push_str(&format!("let r{} = [zed(), zed(1, 2), zed(\"s\")]\n", i));
    }
    main.push_str("print(a0)\n");
    App::new(&format!("many-{}-{}", n, a_return), &[("lib.qwe", "fn price(n: int) -> int {\n    return n\n}\n"), ("a.qwe", &a), ("main.qwe", &main)])
}

#[test]
fn q523_review_s1_s3_t1_another_files_alias_never_names_this_files_calls() {
    // Kept by a node's address, a file's alias went to another file's calls once the first was
    // dropped: "missing argument `x` in call to `p`" for `zed`.
    for (n, a_return) in [(1, false), (50, false), (200, false), (200, true)] {
        let said = many_files(n, a_return).check();
        let zed: Vec<&String> = said.iter().filter(|d| d.starts_with("main.qwe:")).collect();
        assert_eq!(zed.len(), 3 * n, "{} {}: {:#?}", n, a_return, zed);
        assert!(zed.iter().all(|d| d.contains("`zed`")), "{} {}: {:#?}", n, a_return, zed);
    }
    // t1: two files take `price` under two names; each file's calls by its own.
    let lib = "fn price(n: int) -> int {\n    return n\n}\n";
    let a = "import lib.qwe use price as p\n\nlet a0 = p(\"a\")\n";
    let b = "import lib.qwe use price as q\n\nfn zed(x: int) -> int {\n    return q(x, 1)\n}\nlet b0 = zed(zed(1, 2))\n";
    let said = App::new("t1", &[("lib.qwe", lib), ("a.qwe", a), ("b.qwe", b), ("main.qwe", "import a.qwe use a0\nimport b.qwe use b0\n\nprint(a0, b0)\n")]).check();
    for want in ["a.qwe:3 error: argument `n` of `p` must be int, got str", "b.qwe:4 error: `q` takes 1 argument, got 2", "b.qwe:6 error: `zed` takes 1 argument, got 2"] {
        assert!(said.iter().any(|d| d == want), "no {:?} in {:#?}", want, said);
    }
}

#[test]
fn q523_review_d1_a_top_level_blocks_local_and_a_components_by_their_binding() {
    let main = "import lib.qwe use price as p\n\nfn f() -> int {\n    let p = 3\n    return 5\n}\nlet t = {\n    let p = 3\n    5\n}\ncomponent C() {\n    let p = 4\n    <p>hi</p>\n}\nprint(p(1), t, f())\n";
    let said: Vec<String> = App::new("d1", &[("lib.qwe", "fn price(n: int) -> int {\n    return n\n}\n"), ("main.qwe", main)]).check().into_iter().filter(|d| d.contains("unused")).collect();
    assert_eq!(
        said,
        [4, 8, 12].iter().map(|l| format!("main.qwe:{} warning: unused variable `p` (name it `_p` if that is intended)", l)).collect::<Vec<_>>()
    );
}

#[test]
fn q523_review_f4_the_builtin_note_says_the_call_as_written() {
    let lib = "public fn count(n: int) -> int {\n    return n\n}\n";
    let main = "import lib.qwe use count as c\nfn f() -> int {\n    return c(\"x\")\n}\nroute GET / = str(f())\n";
    let said = App::new("f4", &[("lib.qwe", lib), ("main.qwe", main)]).check();
    assert!(
        said.iter().any(|d| d == "main.qwe:3 error: argument `n` of `c` must be int, got str (`c` is the program's `count`, from lib.qwe:1, which hides the builtin `count`)"),
        "{:#?}",
        said
    );
}

#[test]
fn q523_review_this_files_own_signature_in_an_editor_names_its_parameter_as_written() {
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\n";
    let text = "import lib.qwe use price as p\nfn a(p: int) -> int {\n    return p\n}\nroute GET / = str(a(1) + p(2))\n";
    let r = lsp_results("ownsig", &[("lib.qwe", lib), ("main.qwe", text)], &[("hover", 4, 19), ("completion", 4, 18), ("signatureHelp", 4, 20)]);
    assert_eq!(hover_text(&r[0]), "```quillo\nfn a(p: int) -> int\n```");
    let items = r[1].as_array().cloned().unwrap_or_default();
    assert_eq!(items.iter().find(|i| i["label"] == json!("a")).map(|i| i["detail"].clone()), Some(json!("fn a(p: int) -> int")));
    assert_eq!(r[2]["signatures"][0]["label"], json!("fn a(p: int) -> int"));
    let a = r[3].as_array().unwrap().iter().find(|s| s["name"] == json!("a")).unwrap().clone();
    assert_eq!(a["detail"], json!("fn a(p: int) -> int"));
}

#[test]
fn q431_review_c1_c3_c4_how_a_component_reaches_its_callback() {
    let warned = |k: &str| lines(&format!("{}component C() {{\n    let r = js(\"document.title\")\n    <div><K f={{(i) => r}}/></div>\n}}\nroute GET / = <html><body><C/></body></html>\n", k), PAGE_VALUE).len();
    // c1: a lambda of K's that calls it, given only as a handler: later.
    assert_eq!(warned("component K(f: fn) {\n    let done = () => { f(1) }\n    <button @click={done}>x</button>\n}\n"), 0);
    // The same lambda called as K is drawn: shown.
    assert_eq!(warned("component K(f: fn) {\n    let h = () => f(1)\n    <p>{h()}</p>\n}\n"), 1);
    // c3: given down to L, which calls it as it is drawn.
    assert_eq!(warned("component L(g: fn) {\n    <p>{g(1)}</p>\n}\ncomponent K(f: fn) {\n    <div><L g={f}/></div>\n}\n"), 1);
    // Given down to a handler of L's: later.
    assert_eq!(warned("component L(g: fn) {\n    <button @click={() => g(1)}>x</button>\n}\ncomponent K(f: fn) {\n    <div><L g={f}/></div>\n}\n"), 0);
    // c4: through a `let` of it.
    assert_eq!(warned("component K(f: fn) {\n    let cb = f\n    <p>{cb(1)}</p>\n}\n"), 1);
}

#[test]
fn q431_review_c7_c9_a_lambda_through_another_and_names_where_it_is_written() {
    // c7.
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    let g = () => r\n    let f = () => g()\n    <div>{f()}</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![5]);
    // c9: `f` reads the page's `r` where it is written, whatever hides `r` where it is called.
    assert_eq!(lines("component C() {\n    let r = js(\"document.title\")\n    let f = () => r\n    <div>{[1].map((r) => f())}</div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![4]);
}

#[test]
fn q431_review_an_elements_attribute_is_no_showing_nested_or_not() {
    for markup in ["<div class={w > 2 ? \"a\" : \"b\"} data-x={r}>x</div>", "<div>{w > 2 ? <i title={r}>x</i> : null}</div>", "<div><input value={r}/></div>"] {
        let src = format!("component C() {{\n    let r = js(\"document.title\")\n    let w = 3\n    {}\n}}\nroute GET / = <html><body><C/></body></html>\n", markup);
        assert_eq!(lines(&src, PAGE_VALUE), Vec::<u32>::new(), "{}", markup);
    }
}

#[test]
fn q523_review_completion_offers_the_alias_not_the_name_the_file_did_not_take() {
    let lib = "public fn price(n: int) -> int {\n    return n * 2\n}\npublic fn total(n: int) -> int {\n    return n\n}\n";
    let labels = |r: &Value| r.as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["label"].as_str().map(String::from)).collect::<Vec<_>>();
    // Taken only as `p`: `p` offered, `price` not; `total`, taken as itself, offered.
    let r = lsp_results("compl1", &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use price as p, total\nroute GET / = str(p(1) + total(2))\n")], &[("completion", 1, 18)]);
    let items = labels(&r[0]);
    assert!(items.iter().any(|i| i == "p") && items.iter().any(|i| i == "total"), "{:?}", items);
    assert!(!items.iter().any(|i| i == "price"), "{:?}", items);
    // Taken as itself too: both offered.
    let r = lsp_results("compl2", &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use price as p, price\nroute GET / = str(p(1) + price(2))\n")], &[("completion", 1, 18)]);
    let items = labels(&r[0]);
    assert!(items.iter().any(|i| i == "p") && items.iter().any(|i| i == "price"), "{:?}", items);
}

// ---- The review of A2 round 5 -------------------------------------------------------------------

/// The CPU time this thread has run, for a time that a busy machine does not stretch.
fn thread_cpu() -> std::time::Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid timespec for the call to write into.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0 {
        return std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32);
    }
    panic!("no thread clock")
}

/// The least CPU time of three checks of `src`.
fn least_cpu(src: &str) -> std::time::Duration {
    (0..3)
        .map(|_| {
            let t = thread_cpu();
            let _ = check_source(src, "t.qwe", Path::new("."));
            thread_cpu() - t
        })
        .min()
        .unwrap()
}

/// The review's syn/: `n` components, each with page values shown only in handlers and an
/// element's attribute, and a callback given to a component that calls it in a handler.
fn syn(n: usize) -> String {
    let mut s = String::from("component K(f: fn, t: str) {\n    <button @click={() => f(1)}>{t}</button>\n}\n");
    for i in 0..n {
        s.push_str(&format!("component C{i}(w: str) {{\n    let a = js(\"document.title\")\n    let b = js(\"location.href\")\n    <div title={{a}}>\n"));
        for j in 0..8 {
            s.push_str(&format!("        <button class=\"c{j}\" @click={{() => print(a, b)}}>{{w}} {j}</button>\n"));
        }
        s.push_str("        <K f={(x) => print(b)} t={w}/>\n        <span>{w}</span>\n    </div>\n}\n");
    }
    s.push_str("route GET / = <html><body><C0 w=\"x\"/></body></html>\n");
    s
}

#[test]
fn q431_review_syn_the_check_grows_with_the_program_not_faster() {
    // Found in review: each markup child scanned every page value of the program, +83% at 6,000
    // components. Four times the components take about four times as long, not far more; timed by
    // the least CPU time of three checks (a bound, not fixed seconds).
    let (n, m) = if cfg!(debug_assertions) { (100, 400) } else { (750, 3000) };
    let (small, big) = (least_cpu(&syn(n)), least_cpu(&syn(m)));
    assert!(diags(&syn(n)).iter().all(|d| !d.contains(PAGE_VALUE)), "{:?}", diags(&syn(n)));
    assert!(big < small * 6 + std::time::Duration::from_millis(50), "{:?} for {}, {:?} for {}", small, n, big, m);
}

#[test]
fn q431_review_r3c_a_wide_deep_chain_of_components_is_read_once() {
    // Found in review: a walk limited by depth took 29.8 s on ten components each giving a
    // callback to ten of the next. Depth 8 takes about as long as depth 4, not 10^4 times.
    let chain = |depth: usize| {
        let mut s = format!("component F{depth}(g: fn) {{\n    <button @click={{() => g(1)}}>x</button>\n}}\n");
        for k in (0..depth).rev() {
            let kids: String = (0..10).map(|_| format!("<F{} g={{g}}/>", k + 1)).collect();
            s.push_str(&format!("component F{k}(g: fn) {{\n    <div>{kids}</div>\n}}\n"));
        }
        s.push_str("component C() {\n    let r = js(\"document.title\")\n    <div><F0 g={(i) => r}/></div>\n}\nroute GET / = <html><body><C/></body></html>\n");
        s
    };
    assert_eq!(lines(&chain(8), PAGE_VALUE), Vec::<u32>::new());
    let (small, big) = (least_cpu(&chain(4)), least_cpu(&chain(8)));
    assert!(big < small * 4 + std::time::Duration::from_millis(50), "{:?} at depth 4, {:?} at 8", small, big);
}

#[test]
fn q431_review_r2_a_component_that_draws_itself_is_not_taken_to_show_or_call() {
    // r2: the callback only in a handler, given to itself.
    assert_eq!(lines("component Tree(n: int, onPick: fn) {\n    <div>\n        <button @click={() => onPick(n)}>x</button>\n        {if n > 0 { <Tree n={n - 1} onPick={onPick}/> } else { <span>.</span> }}\n    </div>\n}\ncomponent C() {\n    let r = js(\"document.title\")\n    <div><Tree n={3} onPick={(i) => print(r)}/></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    // r2s: the prop only in an attribute, given to itself.
    assert_eq!(lines("component Tree(n: int, label: str) {\n    <div title={label}>\n        {if n > 0 { <Tree n={n - 1} label={label}/> } else { <span>.</span> }}\n    </div>\n}\ncomponent C() {\n    let r = js(\"document.title\")\n    <div><Tree n={3} label={r}/></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    // r2u: two components giving it to each other.
    assert_eq!(lines("component A(label: str, n: int) {\n    <div title={label}>{if n > 0 { <B label={label} n={n - 1}/> } else { <i>.</i> }}</div>\n}\ncomponent B(label: str, n: int) {\n    <div title={label}>{if n > 0 { <A label={label} n={n - 1}/> } else { <i>.</i> }}</div>\n}\ncomponent C() {\n    let r = js(\"document.title\")\n    <div><A label={r} n={2}/></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    // A cycle where one of them does show it: proven, so said.
    assert_eq!(lines("component A(label: str, n: int) {\n    <div>{label}{if n > 0 { <B label={label} n={n - 1}/> } else { <i>.</i> }}</div>\n}\ncomponent B(label: str, n: int) {\n    <div>{if n > 0 { <A label={label} n={n - 1}/> } else { <i>.</i> }}</div>\n}\ncomponent C() {\n    let r = js(\"document.title\")\n    <div><B label={r} n={2}/></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), vec![9]);
}

#[test]
fn q431_review_r4_a_prop_shown_through_what_is_made_from_it() {
    let card = |inner: &str, card: &str| lines(&format!("{inner}component Card(title: str) {{\n{card}}}\ncomponent C() {{\n    let r = js(\"document.title\")\n    <div><Card title={{r}}/></div>\n}}\nroute GET / = <html><body><C/></body></html>\n"), PAGE_VALUE).len();
    // r4a: given on in an expression.
    assert_eq!(card("component Inner(t: str) {\n    <b>{t}</b>\n}\n", "    <div><Inner t={\"x\" + title}/></div>\n"), 1);
    // r4c: a `let` made from it.
    assert_eq!(card("", "    let t = title.upper()\n    <div><b>{t}</b></div>\n"), 1);
    // r4d: a lambda that shows it.
    assert_eq!(card("", "    let show = () => <b>{title}</b>\n    <div>{show()}</div>\n"), 1);
    // A `let` made from it, used only in an attribute: not shown.
    assert_eq!(card("", "    let t = title.upper()\n    <div title={t}>x</div>\n"), 0);
}

#[test]
fn q523_review_k4_routes_on_one_line_each_by_its_own_path() {
    let main = "import lib.qwe use price as p\nfn bump() -> int {\n    db.exec(\"update items set price = 1\")\n    return 1\n}\nroute GET /a/:p = str(p); route GET /b = str(bump())\nroute GET /c = str(bump()); route GET /d/:p = str(p)\n";
    let said: Vec<String> = App::new("k4", &[("lib.qwe", "public fn price(n: int) -> int {\n    return n\n}\n"), ("main.qwe", main)]).check().into_iter().filter(|d| d.contains("writes")).collect();
    assert!(said.iter().any(|d| d.starts_with("main.qwe:6 warning: `GET /b` writes")), "{:#?}", said);
    assert!(said.iter().any(|d| d.starts_with("main.qwe:7 warning: `GET /c` writes")), "{:#?}", said);
    assert!(!said.iter().any(|d| d.contains("/a/") || d.contains("/d/")), "{:#?}", said);
}

#[test]
fn q523_review_m1_a_match_arms_name_as_written() {
    let main = "import lib.qwe use price as p\nlet a = for p in [1, 2] { 0 }\nlet b = match 3 { p => 1 }\nprint(a, b, p(1))\n";
    let said = App::new("m1", &[("lib.qwe", "public fn price(n: int) -> int {\n    return n\n}\n"), ("main.qwe", main)]).check();
    assert!(said.iter().any(|d| d == "main.qwe:3 warning: `p` takes any value and is not used: write `_` for that"), "{:#?}", said);
}

#[test]
fn q523_review_completion_reads_the_parsed_uses() {
    let lib = "public fn price(n: int) -> int {\n    return n\n}\npublic fn tax(n: int) -> int {\n    return n\n}\n";
    let labels = |r: &Value| r.as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["label"].as_str().map(String::from)).collect::<Vec<_>>();
    // lc2: a list over two lines that takes it as itself too.
    let r = lsp_results("lc2", &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use price as p,\n    price\nlet x = 1\nprint(x, p(1), price(2))\n")], &[("completion", 2, 8)]);
    assert!(labels(&r[0]).iter().any(|i| i == "price"), "{:?}", labels(&r[0]));
    // A whole-file import takes every name as itself: `use *`, and 0.6's `use "lib.qwe"`.
    for whole in ["import lib.qwe use *", "use \"lib.qwe\""] {
        let main = format!("{}\nimport lib.qwe use price as p\nlet x = 1\nprint(x, p(1), price(2))\n", whole);
        let r = lsp_results("whole", &[("lib.qwe", lib), ("main.qwe", &main)], &[("completion", 2, 8)]);
        assert!(labels(&r[0]).iter().any(|i| i == "price") && labels(&r[0]).iter().any(|i| i == "p"), "{}: {:?}", whole, labels(&r[0]));
    }
    // Only `as`: not offered.
    let r = lsp_results("onlyas", &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use price as p, tax\nlet x = 1\nprint(x, p(1), tax(2))\n")], &[("completion", 1, 8)]);
    assert!(!labels(&r[0]).iter().any(|i| i == "price"), "{:?}", labels(&r[0]));
}

// ---- The review of A2 round 6 -------------------------------------------------------------------

#[test]
fn q431_review_f1_w1_an_inline_lambda_given_to_a_component_is_handed_on_not_called() {
    // f1: List gives Row `() => onPick(it)`, which Row calls only in a handler.
    assert_eq!(lines("component Row(item: str, onSelect: fn) {\n    <li><button @click={() => onSelect()}>{item}</button></li>\n}\ncomponent List(items: list, onPick: fn) {\n    <ul>{for it in items { <Row item={it} onSelect={() => onPick(it)}/> }}</ul>\n}\ncomponent Page() {\n    let box = js(\"document.activeElement\")\n    <div><List items={[\"a\", \"b\"]} onPick={(i) => box.focus()}/></div>\n}\nroute GET / = <html><body><Page/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    // w1: handed on inline or through a `let`, to a leaf that calls it as drawn (said) or in a
    // handler (not).
    let src = "component LeafDraw(cb: fn) {\n    <p>{cb()}</p>\n}\ncomponent LeafClick(cb: fn) {\n    <button @click={() => cb()}>x</button>\n}\ncomponent MidDraw(onPick: fn) {\n    <div><LeafDraw cb={() => onPick(1)}/></div>\n}\ncomponent MidClick(onPick: fn) {\n    <div><LeafClick cb={() => onPick(1)}/></div>\n}\ncomponent MidLet(onPick: fn) {\n    let go = () => onPick(1)\n    <div><LeafClick cb={go}/></div>\n}\ncomponent MidLetDraw(onPick: fn) {\n    let go = () => onPick(1)\n    <div><LeafDraw cb={go}/></div>\n}\ncomponent C0() {\n    let w = js(\"document.title\")\n    <div><MidDraw onPick={(i) => w}/></div>\n}\ncomponent C1() {\n    let w = js(\"document.title\")\n    <div><MidClick onPick={(i) => w}/></div>\n}\ncomponent C2() {\n    let w = js(\"document.title\")\n    <div><MidLet onPick={(i) => w}/></div>\n}\ncomponent C3() {\n    let w = js(\"document.title\")\n    <div><MidLetDraw onPick={(i) => w}/></div>\n}\nroute GET / = <html><body><C0/><C1/><C2/><C3/></body></html>\n";
    assert_eq!(lines(src, PAGE_VALUE), vec![23, 35]);
}

#[test]
fn q431_review_u1_u6_a_component_the_program_does_not_have_does_neither() {
    let none = |src: &str| assert_eq!(lines(src, PAGE_VALUE), Vec::<u32>::new(), "{}", src);
    // u1: a parameter's component.
    none("component Shell(Icon: fn) {\n    let w = js(\"window.innerWidth\")\n    <div><Icon size={w}/></div>\n}\ncomponent Dot(size: int) {\n    <span class=\"dot\" style=\"width: 4px\"></span>\n}\ncomponent App() {\n    <Shell Icon={Dot}/>\n}\nroute GET / = <html><body><App/></body></html>\n");
    // u2: a lambda given to one.
    none("component Shell(Icon: fn) {\n    let w = js(\"window.innerWidth\")\n    <div><Icon onPick={() => w}/></div>\n}\ncomponent App() {\n    <Shell Icon={(onPick) => 1}/>\n}\nroute GET / = <html><body><App/></body></html>\n");
    // u4: a browser library's.
    none("client \"https://cdn.jsdelivr.net/npm/chart.js@4.4.0/dist/chart.umd.js\" use Chart\ncomponent Shell() {\n    let w = js(\"window.innerWidth\")\n    <div><Chart width={w}/></div>\n}\nroute GET / = <html><body><Shell/></body></html>\n");
    // u5, u6: handed through a middle component to one it was given, and to a leaf that does not.
    none("component Leaf(cb: fn) {\n    <button @click={() => cb(1)}>x</button>\n}\ncomponent Mid(cb: fn, Other: fn) {\n    <div><Other cb={cb}/><Leaf cb={cb}/></div>\n}\ncomponent Shell() {\n    let w = js(\"window.innerWidth\")\n    <div><Mid cb={(i) => w} Other={Leaf}/></div>\n}\nroute GET / = <html><body><Shell/></body></html>\n");
    none("component Leaf(label: str) {\n    <span title={label}>x</span>\n}\ncomponent Mid(label: str, Other: fn) {\n    <div><Other label={label}/><Leaf label={label}/></div>\n}\ncomponent Shell() {\n    let w = js(\"document.title\")\n    <div><Mid label={w} Other={Leaf}/></div>\n}\nroute GET / = <html><body><Shell/></body></html>\n");
    // u3: a typo of a component's name is said as unknown, and the page value is not.
    let all = diags("component Inner(n: int) {\n    <span>{n}</span>\n}\ncomponent Shell() {\n    let w = js(\"window.innerWidth\")\n    <div><Iner n={w}/></div>\n}\nroute GET / = <html><body><Shell/></body></html>\n");
    assert!(!all.iter().any(|d| d.contains(PAGE_VALUE)), "{:?}", all);
}

#[test]
fn q431_review_a_spread_given_to_a_component() {
    let show = "component Show(y: str) {\n    <p>{y}</p>\n}\n";
    let quiet = "component Quiet(y: str) {\n    <p title={y}>q</p>\n}\n";
    let c = |defs: &str, body: &str| lines(&format!("{defs}component C() {{\n    let w = js(\"document.title\")\n{body}}}\nroute GET / = <html><body><C/></body></html>\n"), PAGE_VALUE);
    // Written out, and a `let` of a map: followed.
    assert_eq!(c(show, "    <div><Show {...{y: w}}/></div>\n"), vec![6]);
    assert_eq!(c(show, "    let m = {y: w}\n    <div><Show {...m}/></div>\n"), vec![7]);
    // To a component that does not show it, and another spread: nothing.
    assert_eq!(c(quiet, "    <div><Quiet {...{y: w}}/></div>\n"), Vec::<u32>::new());
    assert_eq!(c(show, "    <div><Show {...{z: 1, y: \"a\"}}/>{str(1)}</div>\n"), Vec::<u32>::new());
}

/// `k` chains, each `depth` components handing `n` props down, the leaf showing every other one
/// (the review's fp/ shape).
fn prop_chains(k: usize, depth: usize, n: usize) -> String {
    let ps: Vec<String> = (0..n).map(|i| format!("p{i}")).collect();
    let params = ps.iter().map(|p| format!("{p}: str")).collect::<Vec<_>>().join(", ");
    let mut s = String::new();
    for c in 0..k {
        for d in (0..depth).rev() {
            if d == depth - 1 {
                let body: String = ps.iter().enumerate().map(|(i, p)| if i % 2 == 0 { format!("<b>{{{p}}}</b>") } else { format!("<i title={{{p}}}>.</i>") }).collect();
                s.push_str(&format!("component N{c}_{d}({params}) {{\n    <div>{body}</div>\n}}\n"));
            } else {
                let attrs = ps.iter().map(|p| format!("{p}={{{p}}}")).collect::<Vec<_>>().join(" ");
                s.push_str(&format!("component N{c}_{d}({params}) {{\n    <div><N{c}_{} {attrs}/><N{}_{} {attrs}/></div>\n}}\n", d + 1, (c + 1) % k, d + 1));
            }
        }
        let given = ps.iter().enumerate().map(|(i, p)| if i == 0 { format!("{p}={{w}}") } else { format!("{p}=\"x\"") }).collect::<Vec<_>>().join(" ");
        s.push_str(&format!("component Top{c}() {{\n    let w = js(\"document.title\")\n    <div><N{c}_0 {given}/></div>\n}}\n"));
    }
    s
}

/// One component with `n` props, each made into a `let`, each shown only in an attribute (the
/// review's fp/wide shape).
fn wide(n: usize) -> String {
    let ps: Vec<String> = (0..n).map(|i| format!("p{i}")).collect();
    let mut s = format!("component W({}) {{\n", ps.iter().map(|p| format!("{p}: str")).collect::<Vec<_>>().join(", "));
    for (i, p) in ps.iter().enumerate() {
        s.push_str(&format!("    let l{i} = {p}.upper()\n"));
    }
    s.push_str("    <div>\n");
    for i in 0..n {
        s.push_str(&format!("        <b title={{l{i}}}>x</b>\n"));
    }
    let given = (0..n).map(|i| if i == 0 { "p0={w}".to_string() } else { format!("p{i}=\"x\"") }).collect::<Vec<_>>().join(" ");
    s.push_str(&format!("    </div>\n}}\ncomponent Top() {{\n    let w = js(\"document.title\")\n    <div><W {given}/></div>\n}}\n"));
    s
}

#[test]
fn q431_review_the_prop_facts_grow_with_the_program() {
    // Found in review: the pairs as strings, a walk per parameter: +60% on 11,000 pairs, and a
    // component with N derived props O(N^2). Four times the size takes about four times as long.
    let (k, n) = if cfg!(debug_assertions) { (10, 200) } else { (40, 800) };
    let (small, big) = (least_cpu(&prop_chains(k, 10, 5)), least_cpu(&prop_chains(4 * k, 10, 5)));
    assert!(big < small * 6 + std::time::Duration::from_millis(50), "{:?} for {} chains, {:?} for {}", small, k, big, 4 * k);
    assert_eq!(lines(&prop_chains(2, 4, 3), PAGE_VALUE).len(), 2);
    let (small, big) = (least_cpu(&wide(n)), least_cpu(&wide(4 * n)));
    assert!(big < small * 6 + std::time::Duration::from_millis(50), "{:?} for {} props, {:?} for {}", small, n, big, 4 * n);
    assert_eq!(lines(&wide(50), PAGE_VALUE), Vec::<u32>::new());
}

#[test]
fn q523_review_rt_r2_a_route_never_reached_is_said_as_written() {
    let main = "import lib.qwe use save as p\nfn keep() -> int {\n    return p()\n}\ndb.open(\":memory:\")\nroute GET /a/:p { keep(); return \"a\" }; route GET /a/:p { keep(); return \"b\" }\n";
    let said = App::new("rt2", &[("lib.qwe", "public fn save() -> int {\n    return 1\n}\n"), ("main.qwe", main)]).check();
    assert!(said.iter().any(|d| d == "main.qwe:6 warning: this route is never reached: `GET /a/:p` (line 6) takes every request it would"), "{:#?}", said);
}

#[test]
fn q523_review_tk_t7_a_whole_import_gives_only_what_its_file_defines() {
    let lib = "public fn price(n: int) -> int {\n    return n\n}\npublic fn tax(n: int) -> int {\n    return n\n}\n";
    let labels = |r: &Value| r.as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["label"].as_str().map(String::from)).collect::<Vec<_>>();
    let main = "import other.qwe use *\nimport lib.qwe use price as p\nlet x = 1\nprint(x, p(1), helper())\n";
    // other.qwe has no `price`: still not offered.
    let r = lsp_results("t7", &[("lib.qwe", lib), ("other.qwe", "public fn helper() -> int {\n    return 1\n}\n"), ("main.qwe", main)], &[("completion", 2, 8)]);
    assert!(!labels(&r[0]).iter().any(|i| i == "price") && labels(&r[0]).iter().any(|i| i == "helper"), "{:?}", labels(&r[0]));
    // other.qwe has one: offered.
    let r = lsp_results("t7b", &[("lib.qwe", lib), ("other.qwe", "public fn price(n: int) -> int {\n    return n + 1\n}\npublic fn helper() -> int {\n    return 1\n}\n"), ("main.qwe", main)], &[("completion", 2, 8)]);
    assert!(labels(&r[0]).iter().any(|i| i == "price"), "{:?}", labels(&r[0]));
}

// ---- The review of A2 round 7 -------------------------------------------------------------------

/// The review's chain: `D0..D{n-1}` hand `v` down and the last shows it; each `P_i` gives `D_i` a
/// page value, so each answer is found at the chain's end.
fn chain(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        if i == n - 1 {
            s.push_str(&format!("component D{i}(v: str) {{\n    <p>{{v}}</p>\n}}\n"));
        } else {
            s.push_str(&format!("component D{i}(v: str) {{\n    <div><D{} v={{v}}/></div>\n}}\n", i + 1));
        }
    }
    for i in 0..n {
        s.push_str(&format!("component P{i}() {{\n    let w = js(\"document.title\")\n    <div><D{i} v={{w}}/></div>\n}}\n"));
    }
    s
}

/// The review's gen2: `Hub_i` gives `v` to Show and to one shared quiet chain of `m`.
fn hubs(n: usize, m: usize) -> String {
    let mut s = String::from("component Show(v: str) {\n    <p>{v}</p>\n}\n");
    for i in 0..m {
        let next = if i < m - 1 { format!("<B{} v={{v}}/>", i + 1) } else { String::new() };
        s.push_str(&format!("component B{i}(v: str) {{\n    <div title={{v}}>{next}</div>\n}}\n"));
    }
    for i in 0..n {
        s.push_str(&format!("component Hub{i}(v: str) {{\n    <div><Show v={{v}}/><B0 v={{v}}/></div>\n}}\ncomponent P{i}() {{\n    let w = js(\"document.title\")\n    <div><Hub{i} v={{w}}/></div>\n}}\n"));
    }
    s
}

#[test]
fn q431_review_every_answer_at_once_grows_with_the_program() {
    // Found in review: one search per answer, keeping only its own, was quadratic: a chain of
    // 6,000 took 18G instructions. Twice the size takes less than three times as long.
    let n = if cfg!(debug_assertions) { 1000 } else { 3000 };
    assert_eq!(lines(&chain(20), PAGE_VALUE).len(), 20);
    let (small, big) = (least_cpu(&chain(n)), least_cpu(&chain(2 * n)));
    assert!(big < small * 3 + std::time::Duration::from_millis(30), "{:?} for {}, {:?} for {}", small, n, big, 2 * n);
    assert_eq!(lines(&hubs(10, 5), PAGE_VALUE).len(), 10);
    let (small, big) = (least_cpu(&hubs(n, 50)), least_cpu(&hubs(2 * n, 50)));
    assert!(big < small * 3 + std::time::Duration::from_millis(30), "{:?} for {}, {:?} for {}", small, n, big, 2 * n);
}

#[test]
fn q431_review_a_spread_as_the_program_hands_it() {
    let show = "component Show(title: str) {\n    <p>{title}</p>\n}\n";
    let c = |body: &str| lines(&format!("{show}component C() {{\n    let w = js(\"document.title\")\n{body}}}\nroute GET / = <html><body><C/></body></html>\n"), PAGE_VALUE);
    // sp1: an attribute written out wins over a spread, before or after it; of a key written
    // twice in a map, the last.
    assert_eq!(c("    <div><Show {...{title: w}} title=\"x\"/></div>\n"), Vec::<u32>::new());
    assert_eq!(c("    <div><Show title=\"x\" {...{title: w}}/></div>\n"), Vec::<u32>::new());
    assert_eq!(c("    let m = {title: w}\n    <div><Show {...m} title=\"x\"/></div>\n"), Vec::<u32>::new());
    assert_eq!(c("    <div><Show {...{title: w, title: \"x\"}}/></div>\n"), Vec::<u32>::new());
    assert_eq!(c("    <div><Show {...{title: \"x\", title: w}}/></div>\n"), vec![6]);
    // sp3: a map not written out, or a `let` assigned again, is unknown: nothing.
    assert_eq!(c("    let m = set({}, \"title\", w)\n    <div><Show {...m}/></div>\n"), Vec::<u32>::new());
    assert_eq!(c("    let m = {title: w}\n    m = {title: \"x\"}\n    <div><Show {...m}/></div>\n"), Vec::<u32>::new());
    assert_eq!(c("    <div><Show {...{\"title\": w}}/></div>\n"), vec![6]);
    // sp2: handed on through a spread an attribute overrides: not shown.
    let mid = |m: &str| lines(&format!("{show}{m}component P() {{\n    let w = js(\"document.title\")\n    <div><Mid t={{w}}/></div>\n}}\nroute GET / = <html><body><P/></body></html>\n"), PAGE_VALUE).len();
    assert_eq!(mid("component Mid(t: str) {\n    <div><Show {...{title: t}} title=\"fixed\"/></div>\n}\n"), 0);
    assert_eq!(mid("component Mid(t: str) {\n    <div><Show {...{title: t}}/></div>\n}\n"), 1);
}

#[test]
fn q431_review_sh1_a_name_that_hides_the_parameter_is_not_it() {
    let card = |body: &str| lines(&format!("component Card(title: str, items: list) {{\n{body}}}\ncomponent P() {{\n    let w = js(\"document.title\")\n    <div><Card title={{w}} items={{[\"a\"]}}/></div>\n}}\nroute GET / = <html><body><P/></body></html>\n"), PAGE_VALUE).len();
    assert_eq!(card("    <ul>{for title in items { <li>{title}</li> }}</ul>\n"), 0);
    assert_eq!(card("    <ul>{items.map((title) => <li>{title}</li>)}</ul>\n"), 0);
    assert_eq!(card("    <p>{match 1 { title => \"x\" }}</p>\n"), 0);
    // The parameter itself, beside one that hides it: shown.
    assert_eq!(card("    <ul>{for t in items { <li>{title}</li> }}</ul>\n"), 1);
}

#[test]
fn q431_review_an_inner_fn_called_as_drawn_calls_the_callback() {
    let src = "component InnerFn(cb: fn) {\n    fn h() { return cb() }\n    <p>{h()}</p>\n}\ncomponent P() {\n    let w = js(\"document.title\")\n    <div><InnerFn cb={() => w}/></div>\n}\nroute GET / = <html><body><P/></body></html>\n";
    assert_eq!(lines(src, PAGE_VALUE), vec![7]);
    // Only in a handler: not.
    let src = "component InnerFn(cb: fn) {\n    fn h() { return cb() }\n    <button @click={() => h()}>x</button>\n}\ncomponent P() {\n    let w = js(\"document.title\")\n    <div><InnerFn cb={() => w}/></div>\n}\nroute GET / = <html><body><P/></body></html>\n";
    assert_eq!(lines(src, PAGE_VALUE), Vec::<u32>::new());
}

/// The review's fp quiet shape: chains that hand every prop down to a leaf that shows them only in
/// attributes, so nothing is said; `page` says whether each top gives a page value.
fn quiet_chains(k: usize, depth: usize, page: bool) -> String {
    let ps: Vec<String> = (0..5).map(|i| format!("p{i}")).collect();
    let params = ps.iter().map(|p| format!("{p}: str")).collect::<Vec<_>>().join(", ");
    let mut s = String::new();
    for c in 0..k {
        for d in (0..depth).rev() {
            if d == depth - 1 {
                let body: String = ps.iter().map(|p| format!("<b title={{{p}}}>.</b>")).collect();
                s.push_str(&format!("component N{c}_{d}({params}) {{\n    <div>{body}</div>\n}}\n"));
            } else {
                let attrs = ps.iter().map(|p| format!("{p}={{{p}}}")).collect::<Vec<_>>().join(" ");
                s.push_str(&format!("component N{c}_{d}({params}) {{\n    <div><N{c}_{} {attrs}/><N{}_{} {attrs}/></div>\n}}\n", d + 1, (c + 1) % k, d + 1));
            }
        }
        let w = if page { "js(\"document.title\")" } else { "\"t\"" };
        let given = ps.iter().map(|p| format!("{p}={{w}}")).collect::<Vec<_>>().join(" ");
        s.push_str(&format!("component Top{c}() {{\n    let w = {w}\n    <div><N{c}_0 {given}/></div>\n}}\n"));
    }
    s
}

#[test]
#[cfg(debug_assertions)]
fn q431_review_the_quiet_chains_cost_little_more_than_none() {
    // Found in review: with page values given and none shown, the scan of each component cost
    // +16% to +20% over the same program without them; what components do with their parameters
    // is now gathered as the check walks them (round 9 of the review). Held by a count of that
    // work (`quillo::check::prop_work_done`, debug builds), not by a time, which load makes fail
    // (round 9's review: +7.4% on time where the instructions differ by +0.5%): linear in the
    // program, a few steps for each attribute and component (19,500, 39,000 and 78,000 steps for
    // 5,250, 10,500 and 21,000 of them).
    assert_eq!(lines(&quiet_chains(4, 5, true), PAGE_VALUE), Vec::<u32>::new());
    let work = |k: usize| {
        let src = quiet_chains(k, 10, true);
        let before = quillo::check::prop_work_done();
        let _ = check_source(&src, "t.qwe", Path::new("."));
        quillo::check::prop_work_done() - before
    };
    let (w1, w2, w4) = (work(50), work(100), work(200));
    // Each chain: 10 components of 5 parameters, 9 of them with two elements of 5 attributes, and
    // its top with one of 5.
    let size = |k: u64| k * (10 + 9 * 10 + 5);
    assert!(w1 > 0 && w2 <= w1 * 21 / 10 && w4 <= w2 * 21 / 10, "{w1} {w2} {w4}: not linear");
    assert!(w4 <= 6 * size(200), "{w4} steps for {} attributes and components", size(200));
}

#[test]
fn q523_review_tk_k1_k3_a_whole_import_by_linkings_own_files() {
    let lib = "public fn price(n: int) -> int {\n    return n\n}\n";
    let labels = |r: &Value| r.as_array().cloned().unwrap_or_default().iter().filter_map(|i| i["label"].as_str().map(String::from)).collect::<Vec<_>>();
    let price = |files: &[(&str, &str)], line: u32| labels(&lsp_results("tk", files, &[("completion", line, 8)])[0]).iter().any(|i| i == "price");
    // k1: the whole file's `price` is private: not this file's. One with no mark is public (a
    // file's names are by default), so it is.
    let k1 = |other: &str| price(&[("lib.qwe", lib), ("other.qwe", other), ("main.qwe", "use other.qwe\nimport lib.qwe use price as p\nlet x = 1\nprint(x, p(1), helper())\n")], 2);
    assert!(!k1("private fn price(n: int) -> int {\n    return n + 1\n}\npublic fn helper() -> int {\n    return price(1)\n}\n"));
    assert!(k1("fn price(n: int) -> int {\n    return n + 1\n}\npublic fn helper() -> int {\n    return price(1)\n}\n"));
    // k2: a file of the same name in another folder is not the one taken whole.
    let dir_files = [
        ("lib.qwe", lib),
        ("util.qwe", "public fn helper() -> int {\n    return 1\n}\n"),
        ("x.qwe", "use sub/util.qwe\npublic fn foo() -> int {\n    return price(1)\n}\n"),
        ("main.qwe", "use util.qwe\nimport lib.qwe use price as p\nimport x.qwe use foo\nlet x = 1\nprint(x, p(1), helper(), foo())\n"),
    ];
    let app = App::new("k2", &dir_files);
    std::fs::create_dir_all(app.0.join("sub")).unwrap();
    std::fs::write(app.0.join("sub/util.qwe"), "public fn price(n: int) -> int {\n    return n + 2\n}\n").unwrap();
    let uri = format!("file://{}", app.0.join("main.qwe").display());
    let mut c = Lsp::start();
    c.request("initialize", json!({ "rootUri": format!("file://{}", app.0.display()), "capabilities": {} }));
    c.send(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    c.send(json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen", "params": { "textDocument": { "uri": uri, "languageId": "quillo", "version": 1, "text": dir_files[3].1 } } }));
    let r = c.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": 3, "character": 8 } }));
    assert!(!labels(&r).iter().any(|i| i == "price"), "{:?}", labels(&r));
    // k3: a whole file that takes this one whole.
    assert!(!price(&[("lib.qwe", lib), ("a.qwe", "use main.qwe\npublic fn helper() -> int {\n    return 1\n}\n"), ("main.qwe", "use a.qwe\nimport lib.qwe use price as p\nlet x = 1\nprint(x, p(1), helper())\n")], 2));
}

#[test]
fn q431_review_round9_a_page_value_said_after_the_run_keeps_its_place_on_its_line() {
    // Round 9 of the review: said after the run, a page value's warning still comes where the walk
    // found it among the line's others, before the map's own warning here, as before.
    let src = "component Show(title: str) {\n    <p>{title}</p>\n}\ncomponent S5() {\n    let w = js(\"document.title\")\n    <div><Show {...{title: \"x\", title: w}}/></div>\n}\nroute GET / = <html><body><S5/></body></html>\n";
    let on6: Vec<String> = diags(src).into_iter().filter(|d| d.starts_with("6 ")).collect();
    assert_eq!(on6.len(), 2, "{:?}", on6);
    assert!(on6[0].contains(PAGE_VALUE) && on6[1].contains("written twice"), "{:?}", on6);
}

#[test]
fn q431_review_round9_a_call_in_an_attribute_that_is_no_lambda_runs_as_drawn() {
    // Found in review of round 9: `<Show v={cb()}/>` calls `cb` as the component is drawn, and
    // so do `"a" + cb()`, `cb() ?? ""`, a `map`'s lambda calling it, a spread, a `let`, and the
    // same inside `<Wrap>`; only a lambda given to a component is the component's to call. Round
    // 8 said all of these; round 9 missed five.
    assert_eq!(lines("component Show(v: str) {\n    <p>{v}</p>\n}\ncomponent Comp(cb: fn) {\n    <div><Show v={cb()}/></div>\n}\ncomponent Comp2(cb: fn) {\n    <div><p>{cb()}</p></div>\n}\ncomponent P() {\n    let w = js(\"document.title\")\n    <div><Comp cb={() => w}/></div>\n}\ncomponent P2() {\n    let w = js(\"document.title\")\n    <div><Comp2 cb={() => w}/></div>\n}\nroute GET / = <html><body><P/><P2/></body></html>\n", PAGE_VALUE), vec![12, 16]);
    assert_eq!(lines("component Show(v: str) {\n    <p>{v}</p>\n}\ncomponent Wrap(children: html) {\n    <div>{children}</div>\n}\ncomponent C_call(cb: fn) {\n    <div><Show v={cb()}/></div>\n}\ncomponent P_call() {\n    let w = js(\"document.title\")\n    <div><C_call cb={() => w}/></div>\n}\ncomponent C_map(cb: fn) {\n    <div><Show v={join([1].map((x) => cb()), \"\")}/></div>\n}\ncomponent P_map() {\n    let w = js(\"document.title\")\n    <div><C_map cb={() => w}/></div>\n}\ncomponent C_fallback(cb: fn) {\n    <div><Show v={cb() ?? \"\"}/></div>\n}\ncomponent P_fallback() {\n    let w = js(\"document.title\")\n    <div><C_fallback cb={() => w}/></div>\n}\ncomponent C_tmpl(cb: fn) {\n    <div><Show v={\"a\" + cb()}/></div>\n}\ncomponent P_tmpl() {\n    let w = js(\"document.title\")\n    <div><C_tmpl cb={() => w}/></div>\n}\ncomponent C_spreadcall(cb: fn) {\n    <div><Show {...{v: cb()}}/></div>\n}\ncomponent P_spreadcall() {\n    let w = js(\"document.title\")\n    <div><C_spreadcall cb={() => w}/></div>\n}\ncomponent C_letcall(cb: fn) {\n    let t = cb()\n    <div><Show v={t}/></div>\n}\ncomponent P_letcall() {\n    let w = js(\"document.title\")\n    <div><C_letcall cb={() => w}/></div>\n}\ncomponent C_nested(cb: fn) {\n    <div><Wrap><Show v={cb()}/></Wrap></div>\n}\ncomponent P_nested() {\n    let w = js(\"document.title\")\n    <div><C_nested cb={() => w}/></div>\n}\nroute GET / = <html><body><P_call/><P_map/><P_fallback/><P_tmpl/><P_spreadcall/><P_letcall/><P_nested/></body></html>\n", PAGE_VALUE), vec![12, 19, 26, 33, 40, 48, 55]);
}

/// The diagnostics on `line`, in the order they are said.
fn said_on(src: &str, line: u32) -> Vec<String> {
    diags(src).into_iter().filter(|d| d.starts_with(&format!("{line} "))).collect()
}

#[test]
fn q431_review_round9_page_values_come_first_in_their_child_as_before() {
    // Found in review of round 9: a page value read after another diagnostic in the same `{...}`
    // came after it; it is said where its child starts, as headB said it (the editor has the same
    // list).
    let o1 = said_on("component C() {\n    let r = js(\"Date.now\")\n    <div>\n        <p>{str(len(1, 2)) + str(r)}</p>\n    </div>\n}\nroute GET / = <html><body><C/></body></html>\n", 4);
    assert!(o1.len() == 3 && o1[0].contains(PAGE_VALUE) && o1[0].contains("`r`"), "{:?}", o1);
    let o3 = said_on("component C() {\n    let r = js(\"Date.now\")\n    let q = js(\"Date.now\")\n    <div>\n        <p>{str(q) + str(len(1, 2)) + str(r)}</p>\n    </div>\n}\nroute GET / = <html><body><C/></body></html>\n", 5);
    assert!(o3.len() == 4 && o3[0].contains("`q`") && o3[1].contains("`r`") && o3[2].contains(" error:"), "{:?}", o3);
    let o4 = said_on("component C() {\n    let r = js(\"Date.now\")\n    <div>\n        <p>{[1].map((x) => len(x, 2)) + [r]}</p>\n    </div>\n}\nroute GET / = <html><body><C/></body></html>\n", 4);
    assert!(o4.len() == 3 && o4[0].contains(PAGE_VALUE), "{:?}", o4);
}

#[test]
fn q431_review_round9_a_tag_held_by_a_parameter_or_local_is_unknown() {
    // Found in review of round 9: `<Show>` was looked up by name, so a parameter `Show` or a local
    // `fn Show` was taken for the program's component that shows its `v`. Either is unknown, so
    // nothing is said. (`let Show = Quiet` binds nothing, as the check says, so <Show> there is the
    // program's.)
    let s1 = diags("component Show(v: str) {\n    <p>{v}</p>\n}\ncomponent Quiet(v: str) {\n    <p title={v}>x</p>\n}\ncomponent P(Show: fn) {\n    let w = js(\"document.title\")\n    <div><Show v={w}/></div>\n}\ncomponent R() {\n    let Show = Quiet\n    let w = js(\"document.title\")\n    <div><Show v={w}/></div>\n}\nroute GET / = <html><body><P Show={Quiet}/><R/></body></html>\n");
    assert!(!s1.iter().any(|d| d.starts_with("9 ") && d.contains(PAGE_VALUE)), "{:?}", s1);
    assert_eq!(lines("component Show(v: str) {\n    <p>{v}</p>\n}\ncomponent R() {\n    fn Show(v: str) {\n        return <p title={v}>x</p>\n    }\n    let w = js(\"document.title\")\n    <div><Show v={w}/></div>\n}\nroute GET / = <html><body><R/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
}

#[test]
#[cfg(debug_assertions)]
fn q431_review_round10_a_program_with_no_page_value_does_none_of_the_work() {
    // Found in review of round 10: a program that never calls `js(...)` has no page value, so
    // what its components do with their parameters is not gathered at all.
    let before = quillo::check::prop_work_done();
    let _ = check_source(&quiet_chains(20, 10, false), "t.qwe", Path::new("."));
    assert_eq!(quillo::check::prop_work_done() - before, 0);
    // One `js(` anywhere, even in another component, and it is.
    let before = quillo::check::prop_work_done();
    let _ = check_source(&(quiet_chains(20, 10, false) + "component J() {\n    let w = js (\"document.title\")\n    <p title={w}>x</p>\n}\n"), "t.qwe", Path::new("."));
    assert!(quillo::check::prop_work_done() - before > 0);
}

#[test]
fn q431_review_round10b_js_with_a_comment_before_its_paren_is_still_a_page_value() {
    // Found in review of round 10b: `js /* c */ (...)` is a call, and the look at the program's
    // text that decides whether any page value can be there skipped only spaces and tabs.
    for src in ["component C() {\n    let w = js /* c */ (\"document.title\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", "component C() {\n    let w = js_new/**/(\"Date\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", "component C() {\n    let w = js/*x*/(\"document.title\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n"] {
        assert_eq!(lines(src, PAGE_VALUE), vec![3], "{}", src);
    }
}

#[test]
fn q431_review_round10b_a_js_of_the_programs_own_is_no_page_value() {
    // Found in review of round 10b (from 0.8.11): `js` was matched by name, so the program's own
    // `fn js`, a local `let js`, and a module's `js` taken under another name were page values.
    assert_eq!(lines("fn js(s: str) -> str {\n    return s\n}\ncomponent C() {\n    let w = js(\"document.title\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    assert_eq!(lines("component C() {\n    let js = (s) => s\n    let w = js(\"document.title\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE), Vec::<u32>::new());
    for (name, lib) in [("gmod1", "public let js = (s: str) => s\n"), ("gmod2", "public fn js/* */(s: str) -> str {\n    return s\n}\n")] {
        let app = App::new(name, &[("lib.qwe", lib), ("main.qwe", "import lib.qwe use js as j\ncomponent C() {\n    let w = j(\"document.title\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n")]);
        let said = app.check();
        assert!(!said.iter().any(|d| d.contains(PAGE_VALUE)), "{name}: {:?}", said);
    }
}

#[test]
fn q431_review_round11_a_js_bound_inside_the_value_is_no_page_value() {
    // Found in review of round 11: a `js` bound inside the `let`'s own value (a match arm, a
    // lambda's parameter, a block's `let`, a `map` callback) was taken for the builtin, as the
    // check asked after that scope was gone; each call is now resolved as it is read.
    for src in ["component C() {\n    let f = (s) => s\n    let w = match f {\n        js => js(\"document.title\")\n    }\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", "component C() {\n    let w = ((js) => js(\"document.title\"))((s) => s)\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", "component C() {\n    let w = {\n        let js = (s) => s\n        js(\"document.title\")\n    }\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", "component C() {\n    let w = [(s) => s].map((js) => js(\"document.title\"))\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n"] {
        assert_eq!(lines(src, PAGE_VALUE), Vec::<u32>::new(), "{}", src);
    }
    // The builtin still is one, piped into too.
    assert_eq!(lines("component C() {\n    let w = js(\"document.title\")\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE).len(), 1);
    assert_eq!(lines("component C() {\n    let w = \"document.title\" | js()\n    <div><p>{w}</p></div>\n}\nroute GET / = <html><body><C/></body></html>\n", PAGE_VALUE).len(), 1);
}
