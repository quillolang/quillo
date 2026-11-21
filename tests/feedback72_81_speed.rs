//! What the 0.8.10 release benchmark's two fixes keep as it was (branch feedback72-81). `quillo
//! check` of quillolang.org's site was 2.2% slower than 0.8.9's: it made each name's references and
//! hover text, which only `quillo lsp` reads, and now a check that is not an editor's leaves them
//! out (`check::Options::references`, `Options::plain`). vs-python's "filter map sum" went behind
//! Python: `call1` asked its three shortcuts' conditions apart, one of them twice, on each call of a
//! lambda; now once (`interp::given`).

mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use quillo::check::{check_file_opts, Analysis, Options};

/// A folder of its own with `files` in it, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("f7281speed-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        for (f, src) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, src).unwrap();
        }
        Dir(d)
    }

    fn main(&self) -> PathBuf {
        self.0.join("main.qwe")
    }
}

/// The quillo under test: this build's, or the one QUILLO_TEST_BIN names.
fn quillo() -> PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

/// `quillo <cmd> main.qwe` in `d`, under a deadline: whether it succeeded, and what it printed.
fn quillo_in(d: &Path, cmd: &str) -> (bool, String) {
    let mut c = Command::new(quillo()).arg(cmd).arg("main.qwe").current_dir(d).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let (mut so, mut se) = (c.stdout.take().unwrap(), c.stderr.take().unwrap());
    let t1 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let t2 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            panic!("quillo {} took too long", cmd);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (status.success(), t1.join().unwrap() + &t2.join().unwrap())
}

/// Everything a check found but the editor's references and the scopes' names, as text.
fn findings(a: &Analysis) -> String {
    let tools: Vec<(&str, &str)> = a.tools.iter().chain(a.server_fns.iter()).map(|t| (t.def.name.as_str(), t.file.as_str())).collect();
    format!(
        "{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{}",
        a.diagnostics, a.related, a.symbols, a.syntax_errors, a.classes, a.files, a.imports, a.respell, a.taken, tools, a.walked
    )
}

const PROGRAM: &[(&str, &str)] = &[
    (
        "main.qwe",
        r#"use lib/a.qwe
fn out() {
    return deep()
}
fn spare() {
    let never = 1
    return 2
}
component Counter() {
    state n = 0
    return <button onclick={() => n = n + 1}>{n}</button>
}
let parsed = json.parse("[1]")
route GET / {
    return <main><Counter/><p>{out()} {len(parsed)} {spare()}</p></main>
}
"#,
    ),
    ("lib/a.qwe", "use lib/b.qwe\nfn a() {\n    return deep()\n}\n"),
    ("lib/b.qwe", "fn deep() {\n    return \"d\"\n}\n"),
];

#[test]
fn a_plain_check_finds_what_an_editors_does() {
    let d = Dir::new("plain", PROGRAM);
    let editor = check_file_opts(&d.main(), Default::default(), Options::default());
    let plain = check_file_opts(&d.main(), Default::default(), Options::plain());
    // The same findings: a name another file gives and this one does not take is still told (it is
    // found where a name is looked up, as the references were made).
    assert_eq!(findings(&editor), findings(&plain));
    let said: Vec<&str> = plain.diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert!(said.iter().any(|m| m.starts_with("`deep` comes from lib/b.qwe, which this file does not take")), "{:#?}", said);
    assert!(said.iter().any(|m| m.contains("`never`")), "{:#?}", said);
    assert_eq!(plain.imports.len(), 1, "{:#?}", plain.imports);
    // Only the editor's check gathers its references, the hover text of a state among them, a
    // module's function, and the scopes' names.
    assert!(plain.references.is_empty(), "{:#?}", plain.references);
    assert!(plain.locals.is_empty(), "{:#?}", plain.locals);
    let r = editor.references.iter().find(|r| r.name == "deep" && r.line == 3).expect("deep's reference");
    assert!(r.def.as_ref().is_some_and(|(f, l)| f.ends_with("b.qwe") && *l == 1), "{:?}", r.def);
    assert!(editor.references.iter().any(|r| r.name == "n" && r.detail.starts_with("state n: ")), "{:#?}", editor.references);
    assert!(editor.references.iter().any(|r| r.name == "parse" && r.line == 13 && !r.detail.is_empty()), "{:#?}", editor.references);
    assert!(editor.locals.iter().any(|l| l.name == "never"), "{:#?}", editor.locals);
}

#[test]
fn an_option_left_out_still_gathers_the_references() {
    // Options built from the default, as a program embedding Quillo writes them, keep the editor's
    // part: only `Options::plain` and `references: false` leave it out.
    let d = Dir::new("default", PROGRAM);
    let opts = Options { strict_null: true, ..Default::default() };
    assert!(opts.references && Options::default().references && !Options::plain().references);
    assert!(!check_file_opts(&d.main(), Default::default(), opts).references.is_empty());
    assert!(!quillo::check::check_file(&d.main()).references.is_empty());
    let none = Options { references: false, ..Default::default() };
    assert!(check_file_opts(&d.main(), Default::default(), none).references.is_empty());
}

#[test]
fn quillo_check_says_what_it_said() {
    let d = Dir::new("cli", PROGRAM);
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(ok, "{}", out);
    assert!(out.contains("main.qwe:3: warning: `deep` comes from lib/b.qwe, which this file does not take"), "{}", out);
    assert!(out.contains("main.qwe: ok (1 route, 4 functions, 1 component, in 3 files)"), "{}", out);
}

/// `src` run (checked first, which must pass), with what it printed and whether it ended well.
fn run(name: &str, src: &str) -> (bool, String) {
    let d = Dir::new(name, &[("main.qwe", src)]);
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(ok, "quillo check:\n{}\n{}", out, src);
    quillo_in(&d.0, "run")
}

#[test]
fn a_lambdas_answer_without_a_call_is_as_before() {
    let (ok, out) = run(
        "given",
        r#"let rows = [{ a: 2 }, { a: 1 }]
let y = 9
let z = { a: 3 }
let holder = []
fn hold(f) {
    holder = push(holder, f)
    return 1
}
fn each(xs, f) {
    return map(xs, f)
}
fn ordered(xs, f) {
    return sort_by(xs, f)
}
// The program's own: given without a call.
print(map([1, 2], (x) => x))
print(map([1, 2], (_x) => 5))
print(map(rows, (x) => x.a))
print(sort_by(rows, (x) => x.a))
print(filter([0, 1, 2], (x) => x))
let pick = (x) => x.a
print(pick({ a: 7 }))
// Near misses: called as functions.
print(map([1, 2], (x) => y))
print(map(rows, (x) => z.a))
print(map([1, 2], (x = 3) => x))
print(map(json.parse("[1, 2]"), (x: int) => x))
// A script's, called back while it runs: run as the script's.
print(run_script("each([1, 2], (x) => x)", { each: each }))
print(run_script("each([1, 2], (_x) => 5)", { each: each }))
print(run_script("each(rows, (x) => x.a)", { each: each, rows: rows }))
print(run_script("ordered(rows, (x) => x.a)", { ordered: ordered, rows: rows }))
print(run_script("map(rows, (x) => x.a)", { rows: rows }))
// Kept past the script: not run, by any way in.
print(run_script("hold((x) => x)", { hold: hold }))
print(run_script("hold((_x) => 5)", { hold: hold }))
print(run_script("hold((x) => x.a)", { hold: hold }))
for f in holder {
    print(map(rows, f))
    print(f({ a: 1 }))
    print(filter(rows, f))
}
"#,
    );
    assert!(ok, "{}", out);
    let ended = "err(\"a script's function runs only while its script does\")";
    let ended_each = format!("[{}, {}]", ended, ended);
    let mut want: Vec<String> = [
        "[1, 2]",
        "[5, 5]",
        "[2, 1]",
        "[{ a: 1 }, { a: 2 }]",
        "[0, 1, 2]",
        "7",
        "[9, 9]",
        "[3, 3]",
        "[1, 2]",
        "[1, 2]",
        "[1, 2]",
        "[5, 5]",
        "[2, 1]",
        "[{ a: 1 }, { a: 2 }]",
        "[2, 1]",
        "1",
        "1",
        "1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for _ in 0..3 {
        want.extend([ended_each.clone(), ended.to_string(), ended.to_string()]);
    }
    assert_eq!(out.lines().collect::<Vec<_>>(), want, "{}", out);
}

#[test]
fn a_typed_parameter_is_checked_and_a_component_is_refused() {
    // Neither is answered without a call: the type stops the program as a call's does, and a
    // component given to `map` is said to be one, whatever its body.
    let (ok, out) = run("typed", "print(map(json.parse(\"[1, \\\"b\\\"]\"), (x: int) => 5))\n");
    assert!(!ok && out.contains("argument `x` of `<fn>` must be int, got str \"b\""), "{}", out);
    let (ok, out) = run("component", "component Tag(p) {\n    return <b>{p}</b>\n}\nprint(map([1], Tag))\n");
    assert!(!ok && out.contains("`Tag` is a component, which the page runs from its tag"), "{}", out);
}

/// What each check says, as `file:line message`.
fn said(a: &Analysis) -> Vec<String> {
    a.diagnostics.iter().map(|d| format!("{}:{} {}", Path::new(&d.file).file_name().unwrap().to_string_lossy(), d.line, d.message)).collect()
}

// Q-1016's `//` warning reads a local's type from the scopes' names, which a plain check gathered
// no more: `total // 4` on a parameter went unsaid, and a local shadowing a top-level number was
// taken for it (found in review of this fix). They are gathered where a file has such a comment.
#[test]
fn a_plain_check_says_the_comment_warnings_an_editors_does() {
    let comment = "`//` starts a comment here, so `// 4` is left out";
    for (name, src, warned) in [
        ("param", "fn share(total: int) {\n    let each = total // 4\n    return each\n}\nprint(share(8))\n", true),
        ("local", "fn share() {\n    let n = 8\n    let each = n // 4\n    return each\n}\nprint(share())\n", true),
        ("shadow", "let n = 3\nfn f(n: str) {\n    let x = n // 4\n    return x\n}\nprint(f(\"a\"))\n", false),
        ("untyped", "fn tries(retries) {\n    return retries // 4\n}\nprint(tries(1))\n", false),
    ] {
        let d = Dir::new(name, &[("main.qwe", src)]);
        let editor = check_file_opts(&d.main(), Default::default(), Options::default());
        let plain = check_file_opts(&d.main(), Default::default(), Options::plain());
        assert_eq!(said(&editor), said(&plain), "{}", src);
        assert_eq!(said(&plain).iter().any(|m| m.contains(comment)), warned, "{}: {:#?}", name, said(&plain));
        // Still no references in the plain one.
        assert!(plain.references.is_empty());
    }
    // A mounted file is checked on its own, by a plain sub-check, for the editor too.
    let d = Dir::new("mounted", &[
        ("main.qwe", "mount /b = \"b.qwe\"\nroute GET / = \"app\"\n"),
        ("b.qwe", "fn share(total: int) {\n    let each = total // 4\n    return each\n}\nroute GET / = str(share(8))\n"),
    ]);
    for opts in [Options::default(), Options::plain()] {
        let a = check_file_opts(&d.main(), Default::default(), opts);
        assert!(said(&a).iter().any(|m| m.starts_with("b.qwe:2 ") && m.contains(comment)), "{:#?}", said(&a));
    }
    let (ok, out) = quillo_in(&d.0, "check");
    assert!(ok && out.contains("b.qwe:2: warning: `//` starts a comment here, so `// 4` is left out"), "{}", out);
}

// A page's runtime holds what its code needs: Q-1272's message for an error shown as nothing
// (`errShown`) named a local `comp` and the word `match`, which the runtime's shake reads as
// references, and every page took `match`, `matchInto`, `typeIs` and the money and date checks, 5.3
// KB of a counter's runtime (the 0.8.10 release benchmark). The message reads as before.
#[test]
fn a_counters_runtime_takes_no_match() {
    let d = Dir::new("counter", &[(
        "main.qwe",
        "component Counter() {\n    state n = 0\n    return <button onclick={() => { n += 1 }}>Clicked {n} times</button>\n}\nroute GET / {\n    return <main><Counter/></main>\n}\n",
    )]);
    let server = support::serve(&d.main(), &[]);
    let page = support::send(server.port, "GET", "/", &[], "").body;
    let at = page.find("/_quillo/r/").expect("the page takes a runtime file");
    let path: String = page[at..].chars().take_while(|c| *c != '"' && *c != '\'' && *c != ')').collect();
    let rt = support::send(server.port, "GET", &path, &[], "").body;
    assert!(rt.contains("function errShown("), "{}", &rt[..rt.len().min(300)]);
    for unneeded in ["function matchInto(", "function typeIs(", "function moneyFromData(", "function comp("] {
        assert!(!rt.contains(unneeded), "{} in a counter's runtime", unneeded);
    }
    // As written for the console: `match`, not its escape.
    assert!(rt.contains("or \\`ma\\u0074ch\\`)"), "the escape is kept as written");
    drop(server);
}
