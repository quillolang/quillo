//! Edge tests of two fixes of the bugs the coverage tests of 0038a97 found (branch feedback82-85):
//! a connection's functions are held to the file whose code made the connection, wherever they go
//! and however they are read (src/interp.rs held_maker, call_maker and held_to; FEEDBACK83.md
//! Q-1300), and a tag's attributes are read as the browser's tokenizer reads them (src/head.rs
//! attr_of; FEEDBACK84.md Q-1320). Each test asserts what a developer sees: what each call gives,
//! the files a run made, the page's head.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

// ---- helpers, as tests/cover8285_run2.rs has them -----------------------------------------------

/// A test's folder, removed with the record of what its machine built when the test ends.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(record_dir(&self.0));
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str) -> Dir {
    Dir(temp_dir(&format!("cov8285i5-{}", name)))
}

/// Where a test's program keeps the record of what its machine built: beside its folder.
fn record_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(format!("{}-built", dir.file_name().unwrap().to_string_lossy()))
}

const SECRET: &str = "test secret for the tests, 32 bytes or more";

/// `quillo` with `args` in `dir`, ended after 120 s: its exit code and what it wrote.
fn quillo(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let log = dir.join("cmd.log");
    let file = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(args)
        .current_dir(dir)
        .env_remove("QUILLO_POLICY_PATH")
        .env_remove("QUILLO_DB")
        .env_remove("CARGO_TARGET_DIR")
        .env("QUILLO_BUILT_RECORD_DIR", record_dir(dir))
        .env("QUILLO_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    (code, out)
}

/// `quillo check main.qwe`, then `quillo run main.qwe`, a program with no routes, in `d`: what the
/// run printed and logged; each must end with 0.
fn checked_run(d: &Path) -> String {
    let (code, out) = quillo(d, &["check", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    let (code, out) = quillo(d, &["run", "main.qwe"]);
    assert_eq!(code, Some(0), "{}", out);
    out
}

fn write(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(p, text).unwrap();
}

// ---- src/interp.rs: a connection's functions, held to the file that made the connection ---------

/// The helper's file, confirmed to write data/lib.db: each function calls what it is given, in
/// another way.
const LIB: &str = "fn apply(f, x) = f(x)
fn applied(x, f) = f(x)
fn run_on(c, s) = c.exec(s)
fn first_on(xs, s) = xs[0].exec(s)
fn call_got(g, s) = g()(s)
fn via_got(g, s) = g().exec(s)
fn hold(x) = () => x
fn run_err(e, s) = e.error.exec(s)
fn run_mod(d, s) = d.connect(\":memory:\").exec(s)
fn deep_on(w, s) = w.a.b.exec(s)
fn index_on(c, s) = c[\"exec\"](s)
fn values_on(c, s) {
  for f in values(c) {
    if type_of(f) == \"fn\" {
      return f(s)
    }
  }
  return \"none\"
}
fn destructured_on(c, s) {
  let { exec } = c
  return exec(s)
}
fn each_on(xs, s) = map(xs, (f) => f(s))[0]
fn lib_conn() = db.connect(\":memory:\")
";

/// Each way a kit confirmed for nothing hands a connection, or one of its functions, to the
/// helper: `c`, `s` (shards), `top` and `top_exec` (made at the kit's top level) and `connect`
/// (`db`'s, destructured) are the kit's; `{sql}` is an attach of data/lib.db.
const WAYS: [(&str, &str); 31] = [
    ("a field", "apply(c.exec, {sql})"),
    ("handed whole", "run_on(c, {sql})"),
    ("in a list", "first_on([c], {sql})"),
    ("a list's field", "apply([c][0].exec, {sql})"),
    ("set in a map made at run time", "apply(set({}, \"e\", c.exec).e, {sql})"),
    ("spread", "run_on({ ...c }, {sql})"),
    ("merged", "run_on(merge({}, c), {sql})"),
    ("a field of a field", "deep_on({ a: { b: c } }, {sql})"),
    ("piped", "{sql} | applied(c.exec)"),
    ("a lambda that gives the function", "call_got(() => c.exec, {sql})"),
    ("a lambda that gives the connection", "via_got(() => c, {sql})"),
    ("in the helper's lambda's scope", "via_got(hold(c), {sql})"),
    ("made in place", "apply(db.connect(\":memory:\").exec, {sql})"),
    ("by index", "index_on(c, {sql})"),
    ("among its values", "values_on(c, {sql})"),
    ("destructured", "destructured_on(c, {sql})"),
    ("made at the top level", "run_on(top, {sql})"),
    ("its function kept at the top level", "apply(top_exec, {sql})"),
    ("connect taken as a value", "run_on(apply(db.connect, \":memory:\"), {sql})"),
    ("connect by index", "run_on(db[\"connect\"](\":memory:\"), {sql})"),
    ("connect piped", "run_on(\":memory:\" | db.connect, {sql})"),
    ("connect destructured", "run_on(connect(\":memory:\"), {sql})"),
    ("db handed whole", "run_mod(db, {sql})"),
    ("in an error", "run_err(err(c), {sql})"),
    ("given to map", "each_on([c.exec], {sql})"),
    ("a shards' function", "apply(s.exec, {sql})"),
    ("a shard", "run_on(s.on(1), {sql})"),
    ("a shard's field", "apply(s.on(2).exec, {sql})"),
    ("a shard by a lambda", "via_got(() => s.on(3), {sql})"),
    ("its own db.exec", "db.exec({sql})"),
    ("its own connection", "c.exec({sql})"),
];

/// The kit's `steal()`: each of `ways` as a line `label: what it gave`, each attaching its own
/// name of data/lib.db.
fn kit(ways: &[(&str, &str)]) -> String {
    let mut lines = Vec::new();
    for (i, (label, call)) in ways.iter().enumerate() {
        let sql = format!("\"attach database 'data/lib.db' as l{}\"", i);
        lines.push(format!("    \"{}: \" + said({})", label, call.replace("{sql}", &sql)));
    }
    format!(
        "use ../lib/config.qwe\nlet top = db.connect(\":memory:\")\nlet top_exec = top.exec\ncomponent Badge() {{ <b>b</b> }}\nfn steal() {{\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let c = db.connect(\":memory:\")\n  let s = db.shards([\":memory:\"])\n  let {{ connect }} = db\n  return join([\n{}\n  ], \"\\n\")\n}}\n",
        lines.join(",\n")
    )
}

/// The main file: the kit's lines, then its own connection's, by name, as a value to its own
/// helper, piped, in a transaction, and made by the name `connect` with nothing to make it of.
const MAIN: &str = "use lib/config.qwe
import kit/k.qwe use Badge, steal
db.open(\"data/app.db\")
print(\"kit:\\n\" + steal())
let m = db.connect(\":memory:\")
print(\"main by name: \" + str(m.one(\"select 1 as one\")))
print(\"main as a value: \" + str(apply(m.query, \"select 2 as two\")))
print(\"main piped: \" + str(\"select 3 as three\" | m.one))
print(\"main in a transaction: \" + str(m.transaction(() => m.one(\"select 4 as four\"))))
print(\"main empty: \" + str(is_err(db.connect(\"\"))))
";

/// Under a quillo.confirmed a connection is held to the file whose code made it, whichever way its
/// functions go: a kit confirmed for nothing hands its connection, or a function of it, to a helper
/// whose file is confirmed to write data/lib.db, in 29 ways (a field, the connection whole, in a
/// list, a map made at run time, a spread, a field of a field, a pipe, a lambda that gives either,
/// one made in place, by index, among its values, destructured, made at the top level, `connect`
/// taken as a value, by index, piped or destructured, `db` handed whole, in an error, given to
/// `map`, a shards' function, a shard); each attach is refused for the kit, as its own `db.exec`
/// and `c.exec` are, and data/lib.db is not made. The main file's own connection answers by name,
/// as a value, piped and in a transaction as before (src/interp.rs held_maker, call_maker, held_to,
/// FEEDBACK83.md Q-1300, found by the coverage tests of 0038a97; branch feedback82-85).
#[test]
fn a_connections_functions_are_held_to_the_file_that_made_it_however_they_are_reached() {
    let d = dir("ways");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "lib/config.qwe", LIB);
    write(&d, "kit/k.qwe", &kit(&WAYS));
    write(&d, "main.qwe", MAIN);
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/app.db\"]\n\n[\"lib/config.qwe\"]\nwrites = [\"data/lib.db\"]\n");
    let out = checked_run(&d);
    let refused = "db: data/lib.db is not confirmed for kit/k.qwe (quillo confirm)";
    let want: Vec<String> = WAYS.iter().map(|(label, _)| format!("{}: {}", label, refused)).collect();
    assert!(out.contains(&format!("kit:\n{}\n", want.join("\n"))), "{}", out);
    assert!(!d.join("data/lib.db").exists(), "the kit made data/lib.db: {}", out);
    assert!(out.contains("main by name: { one: 1 }\nmain as a value: [{ two: 2 }]\nmain piped: { three: 3 }\nmain in a transaction: { four: 4 }\n"), "{}", out);
    // A `db.connect` that makes no connection gives its error as it is (the hold's, said once).
    assert!(out.contains("main empty: true\n"), "{}", out);
}

/// Without a quillo.confirmed nothing is held, and a connection's functions run as they did: the
/// kit's attaches of data/lib.db through the helper, a field, the connection whole, one made in
/// place, a shard and `connect` taken as a value, each work and make it (src/interp.rs held_maker,
/// call_maker, FEEDBACK83.md Q-1300, found by the coverage tests of 0038a97; branch feedback82-85).
#[test]
fn without_a_quillo_confirmed_a_connections_functions_run_as_before() {
    let d = dir("unheld");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let ways = [WAYS[0], WAYS[1], WAYS[12], WAYS[26], WAYS[18]];
    write(&d, "lib/config.qwe", LIB);
    write(&d, "kit/k.qwe", &kit(&ways));
    write(&d, "main.qwe", MAIN);
    let out = checked_run(&d);
    let want: Vec<String> = ways.iter().map(|(label, _)| format!("{}: {{ changes: 0, last_id: null }}", label)).collect();
    assert!(out.contains(&format!("kit:\n{}\n", want.join("\n"))), "{}", out);
    assert!(d.join("data/lib.db").exists(), "{}", out);
    assert!(out.contains("main by name: { one: 1 }\nmain as a value: [{ two: 2 }]\n"), "{}", out);
}

/// A connection is the file's whose code made it, as a builtin that code names is: one the helper
/// makes and hands to the kit attaches what the helper's file may (data/lib.db); the main file's,
/// handed to the kit, attaches what the main file may (data/app2.db) and not data/lib.db, by name
/// or as a value; the kit's own attaches nothing. The kit is in a folder of a name not ASCII,
/// which each refusal names as it is (src/interp.rs held_maker, call_maker, FEEDBACK83.md Q-1300,
/// found by the coverage tests of 0038a97; branch feedback82-85).
#[test]
fn a_connection_is_held_to_the_file_whose_code_made_it() {
    let d = dir("maker");
    std::fs::create_dir_all(d.join("data")).unwrap();
    write(&d, "lib/config.qwe", LIB);
    write(
        &d,
        "kït/k.qwe",
        "use ../lib/config.qwe\ncomponent Badge() { <b>b</b> }\nfn steal(m) {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  return join([\n    \"the helper's: \" + said(lib_conn().exec(\"attach database 'data/lib.db' as a\")),\n    \"main's: \" + said(m.exec(\"attach database 'data/app2.db' as b\")),\n    \"main's, not main's file: \" + said(m.exec(\"attach database 'data/lib.db' as c\")),\n    \"main's as a value: \" + said(apply(m.exec, \"attach database 'data/app2.db' as d\")),\n    \"its own: \" + said(db.connect(\":memory:\").exec(\"attach database 'data/app2.db' as e\"))\n  ], \"\\n\")\n}\n",
    );
    write(&d, "main.qwe", "use lib/config.qwe\nimport kït/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\nprint(\"kit:\\n\" + steal(db.connect(\":memory:\")))\n");
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/app.db\", \"data/app2.db\"]\n\n[\"lib/config.qwe\"]\nwrites = [\"data/lib.db\"]\n");
    let out = checked_run(&d);
    let ok = "{ changes: 0, last_id: null }";
    let want = format!(
        "kit:\nthe helper's: {ok}\nmain's: {ok}\nmain's, not main's file: db: data/lib.db is not confirmed for main.qwe (quillo confirm)\nmain's as a value: {ok}\nits own: db: data/app2.db is not confirmed for kït/k.qwe (quillo confirm)\n"
    );
    assert!(out.contains(&want), "{}", out);
    assert!(d.join("data/lib.db").exists() && d.join("data/app2.db").exists(), "{}", out);
}

/// Past the stand-ins a connection is refused, never held to the code that calls it: with 4,095
/// of the 4,096 taken by the main file's 1,024 helpers, the kit's `db.connect` takes the last, and
/// its connection's functions find none, so each call of them is refused, by name and handed to a
/// helper confirmed for data/lib.db; the main file's `db.connect` after it is refused; the kit's
/// own `db.exec` is held to the kit as ever, and nothing is attached (src/interp.rs held_maker,
/// held_to, FEEDBACK83.md Q-1300, found by the coverage tests of 0038a97; branch feedback82-85).
#[test]
fn past_the_stand_ins_a_connection_is_refused_never_held_to_its_caller() {
    let d = dir("limit");
    std::fs::create_dir_all(d.join("data")).unwrap();
    let mut main = String::from("use lib/config.qwe\n");
    for i in 1..=1024 {
        let taken = if i < 1024 { "read_file, write_file, delete_file, save_upload" } else { "read_file, write_file, delete_file" };
        write(&d, &format!("f/t{}.qwe", i), &format!("let t{} = [{}]\n", i, taken));
        main.push_str(&format!("use f/t{}.qwe\n", i));
    }
    main.push_str("import kit/k.qwe use Badge, steal\ndb.open(\"data/app.db\")\nprint(steal())\nlet m = db.connect(\":memory:\")\nprint(\"main: \" + (is_err(m) ? m.error : str(m)))\n");
    write(&d, "main.qwe", &main);
    write(&d, "lib/config.qwe", LIB);
    write(
        &d,
        "kit/k.qwe",
        "use ../lib/config.qwe\ncomponent Badge() { <b>b</b> }\nfn steal() {\n  let said = (r) => is_err(r) ? r.error : str(r)\n  let c = db.connect(\":memory:\")\n  return \"kit: \" + join([said(c.one(\"select 1 as x\")), said(apply(c.exec, \"attach database 'data/lib.db' as l\")), said(db.exec(\"attach database 'data/lib.db' as l2\"))], \" | \")\n}\n",
    );
    write(&d, "quillo.confirmed", "[\"main.qwe\"]\nwrites = [\"data/app.db\"]\n\n[\"lib/config.qwe\"]\nwrites = [\"data/lib.db\"]\n");
    let out = checked_run(&d);
    let full = "refused: more than 4096 builtins that reach out are taken as values in the program's files, and none holds this one (call it by its name)";
    assert!(out.contains(&format!("kit: {full} | {full} | db: data/lib.db is not confirmed for kit/k.qwe (quillo confirm)\nmain: {full}\n")), "{}", out);
    assert!(!d.join("data/lib.db").exists(), "{}", out);
}

// ---- src/head.rs attr_of: a tag's attributes as the browser's tokenizer reads them -------------

fn said_head(pairs: &[(&str, &str)]) -> quillo::head::Head {
    let mut h = quillo::head::Head::default();
    for (k, v) in pairs {
        h.say(quillo::head::KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
        h.called();
    }
    h
}

/// `page` with `h`'s tags in its head.
fn with_head(page: &str, h: &quillo::head::Head) -> String {
    let mut b = page.as_bytes().to_vec();
    quillo::head::apply(&mut b, h);
    String::from_utf8(b).unwrap()
}

/// What `page_head({ title: "T" })` writes.
const TT: &str = "<title>T</title><meta property=\"og:title\" content=\"T\">";

/// What `page_head({ description: "D" })` writes.
const DD: &str = "<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\">";

/// What `page_head({ canonical: "/c" })` writes.
const CC: &str = "<link rel=\"canonical\" href=\"/c\"><meta property=\"og:url\" content=\"/c\">";

/// A head's tag is cut where the browser reads it as one of a key `page_head` says, and left where
/// it does not, its attributes read by the tokenizer's states: a name that starts with `=` (`="x`,
/// `=`, `='x`, `=name`), `/` between attributes and after a name, spaces and line breaks around a
/// name's `=`, a value next to the name after it, duplicate names (the first is kept), names and
/// values in capitals, an unquoted value (that runs to a `/` or ends at the tag's `>`), a name with
/// no value, quotes in a name, a value that starts with `=`, a name and a value not ASCII, none at
/// all. Each head as parse5 7.3.0 reads it, scripting on and off: a description meta, an og:title
/// or a canonical link exactly where it is cut (src/head.rs attr_of, FEEDBACK84.md Q-1320, found by
/// the coverage tests of 0038a97; branch feedback82-85).
#[test]
fn a_tags_attributes_are_read_as_the_browsers_tokenizer_reads_them() {
    let d = said_head(&[("description", "D")]);
    let doc = |head: &str| format!("<!doctype html><html><head>{head}</head><body>b</body></html>");
    // (the head, whether the browser reads a description meta in it: cut)
    let cases: [(&str, bool); 25] = [
        ("<meta =\"x name=\"description\" content=\"old\">", true),
        ("<meta = name=\"description\" content=\"old\">", true),
        ("<meta ='x name=description content=old>", true),
        ("<meta =name=\"description\" content=\"old\">", false),
        ("<meta name =  \"description\" content=\"old\">", true),
        ("<meta name\n=\n'description' content=old>", true),
        ("<meta/name=\"description\"/content=\"old\">", true),
        ("<meta / name=\"description\">", true),
        ("<meta name/=\"description\">", false),
        ("<meta a=\"x\"name=\"description\" content=\"old\">", true),
        ("<meta name=\"x\"/name=\"description\" content=\"old\">", false),
        ("<meta name=\"description\" name=\"x\" content=\"old\">", true),
        ("<META NAME=\"DESCRIPTION\" CONTENT=\"old\">", true),
        ("<meta Name=Description content=old>", true),
        ("<meta name=description>", true),
        ("<meta name=description/>", false),
        ("<meta name content=\"description\">", false),
        ("<meta name>", false),
        ("<meta \"name\"=\"description\">", false),
        ("<meta name==\"description\">", false),
        ("<meta näme=\"x\" name=\"description\" content=\"old\">", true),
        ("<meta name=\"déscription\" content=\"old\">", false),
        ("<meta name=\"\">", false),
        ("<meta>", false),
        ("", false),
    ];
    for (head, cut) in cases {
        let want = if cut { DD.to_string() } else { format!("{head}{DD}") };
        assert_eq!(with_head(&doc(head), &d), doc(&want), "{:?}", head);
    }
    // An unquoted value ends at the tag's `>`: the meta has no name, and the text after it ends
    // the head, before which the tags go.
    assert_eq!(with_head(&doc("<meta content=a>b name=description>"), &d), doc(&format!("<meta content=a>{DD}b name=description>")));
    // An og:title's meta and a canonical link whose first attribute starts with `=`: cut.
    let t = said_head(&[("title", "T")]);
    assert_eq!(with_head(&doc("<meta =\"x property=\"og:title\" content=\"old\"><title>Old</title>"), &t), doc(TT));
    let c = said_head(&[("canonical", "/c")]);
    assert_eq!(with_head(&doc("<link =\"x rel=\"canonical\" href=\"/old\">"), &c), doc(CC));
    assert_eq!(with_head(&doc("<link =rel=\"canonical\" href=\"/old\">"), &c), doc(&format!("<link =rel=\"canonical\" href=\"/old\">{CC}")));
}
