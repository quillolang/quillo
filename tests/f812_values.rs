//! Quillo 0.8.12's deep values and their scopes (branch feedback86-89): a value nested a million
//! deep freed without recursion, in a request and in `parallel` (FEEDBACK49.md, Q-592;
//! FEEDBACK51.md, Q-661; coverage of 0.8.7, Q-1114), the walks over one, a kept lambda's scope
//! beside a list looked at without `ring`'s walk (FEEDBACK65.md, Q-955), and records marked by the
//! type that made them, which a constructor checks to the end (FEEDBACK75.md, Q-1122).

mod support;

use quillo::value::{look_again, lookup, ring_walks, Items};
use quillo::{Interp, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f812-values-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

/// What `quillo run` prints for a script, under a deadline.
fn run_script(d: &Path) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(d.join("main.qwe"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("the script did not end");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

/// Values nested `n` deep, of each kind: lists, maps, error values, lambdas each kept in the
/// scope of the next, maps and lists in turn, and records made one by one by their constructor.
const DEEP: &str = r#"type Node = { v: int, next: Node? }

fn deep(kind, n) {
    if kind == "list" {
        let x = []
        for i in range(0, n) {
            x = [x]
        }
        return x
    }
    if kind == "map" {
        let x = {}
        for i in range(0, n) {
            x = { a: x }
        }
        return x
    }
    if kind == "err" {
        let x = err("é😀")
        for i in range(0, n) {
            x = err(x)
        }
        return x
    }
    if kind == "fn" {
        let f = (v) => v
        for i in range(0, n) {
            let g = f
            f = (v) => g(v)
        }
        return f
    }
    if kind == "mixed" {
        let x = []
        for i in range(0, n) {
            x = { a: [x, i] }
        }
        return x
    }
    let x = Node({ v: 0 })
    for i in range(0, n) {
        x = Node({ v: i, next: x })
    }
    return x
}

fn built(kind, n) {
    let _x = deep(kind, n)
    return kind
}

fn takes(x: any) {
    return 1
}

fn walk(kind, op, n) {
    let a = deep(kind, n)
    let b = deep(kind, n)
    if op == "str" {
        return len(str(a)) > 0
    }
    if op == "eq" {
        return a == b
    }
    if op == "json" {
        return is_err(json.stringify(a))
    }
    if op == "unique" {
        return len(unique([a, b]))
    }
    if op == "contains" {
        return contains([a], b)
    }
    if op == "typed" {
        return takes(a)
    }
    return len(str(err(a))) > 0
}

route GET /drop/:kind/:n {
    let x = deep(kind, int(n))
    x = 0
    "dropped"
}

route GET /par/:kind/:n = str(parallel(() => built(kind, int(n)), () => built(kind, 3)))

route GET /walk/:kind/:op/:n = str(walk(kind, op, int(n)))

route GET /parwalk/:kind/:op/:n = str(parallel(() => walk(kind, op, int(n))))

route GET /sort/:n {
    let a = deep("list", int(n))
    str(parallel(() => len(sort([a, deep("list", int(n))]))))
}

route GET /ping = "pong"
"#;

const KINDS: [&str; 6] = ["list", "map", "err", "fn", "mixed", "node"];

// Q-592, Q-661, Q-1114: a value nested a million deep, built and let go of in a request, stopped
// the server (stack overflow, no log line) whatever held it; inside `parallel`, 12,000 did. On
// 4818eff every one of these requests ends the server.
#[test]
fn a_value_nested_a_million_deep_is_let_go_of_and_the_server_answers() {
    let d = dir("drop", DEEP);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "");
    for kind in KINDS {
        // The limit and the shallow forms either side: none, one, and a million.
        for n in [0, 1, 1_000_000] {
            let r = get(&format!("/drop/{}/{}", kind, n));
            assert_eq!((r.status, r.body.as_str()), (200, "dropped"), "Q-592: {} {}", kind, n);
        }
        assert_eq!(get("/ping").body, "pong", "Q-592: the server stopped after {}", kind);
        // In `parallel`, on a thread's own smaller stack (Q-1114).
        let r = get(&format!("/par/{}/{}", kind, 1_000_000));
        assert_eq!((r.status, r.body.clone()), (200, format!("[\"{}\", \"{}\"]", kind, kind)), "Q-1114: {}", kind);
        assert_eq!(get("/ping").body, "pong", "Q-1114: the server stopped after {}", kind);
    }
}

// Q-661: the walks over a deep value (`str`, `==`, JSON, `unique`, `contains`, a typed argument,
// an error's text), in a request and in `parallel`, each with the value let go of after: an answer,
// never a stopped server. 4818eff stops at the first.
#[test]
fn the_walks_over_a_deep_value_answer_and_the_server_stays_up() {
    let d = dir("walks", DEEP);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |p: &str| support::send(s.port, "GET", p, &[], "");
    let n = 300_000;
    for kind in ["list", "map", "mixed", "err"] {
        for (op, want) in [("str", "true"), ("eq", "true"), ("json", "true"), ("unique", "1"), ("contains", "true"), ("typed", "1"), ("errtext", "true")] {
            let r = get(&format!("/walk/{}/{}/{}", kind, op, n));
            assert_eq!((r.status, r.body.as_str()), (200, want), "Q-661: {} {}", kind, op);
            let r = get(&format!("/parwalk/{}/{}/{}", kind, op, n));
            assert_eq!((r.status, r.body.clone()), (200, format!("[{}]", want)), "Q-661 in parallel: {} {}", kind, op);
        }
    }
    // Shallow: JSON of a value 3 deep is written.
    assert_eq!(get("/walk/list/json/3").body, "false");
    // Comparing two lists 10,000 deep, the most compared, on `parallel`'s stack; past it they are
    // not ordered: an error, and the server answers.
    assert_eq!(get("/sort/10000").body, "[2]");
    assert_eq!(get("/sort/10002").status, 500);
    assert_eq!(get("/ping").body, "pong");
}

// ---- Q-955: a kept lambda's scope beside a list ----------------------------------------------

const SCOPES: &str = r#"fn beside_list(rows, i) {
    let rules = [1, 2, 3]
    let f = (x) => x + i + len(rules) + len(rows)
    return f
}
fn beside_range(rows, i) {
    let pad = range(0, 100)
    let f = (x) => x + i + len(pad) + len(rows)
    return f
}
fn in_record(rows, i) {
    let acts = { go: (x) => x + i + len(rows), pad: range(0, 120) }
    return acts.go
}
fn at_limit(rows, i) {
    let pad = range(0, 128)
    let f = (x) => x + i + len(pad) + len(rows)
    return f
}
fn over_limit(rows, i) {
    let pad = range(0, 129)
    let f = (x) => x + i + len(pad) + len(rows)
    return f
}
fn empty_list(rows, i) {
    let pad = []
    let f = (x) => x + i + len(pad) + len(rows)
    return f
}
fn unicode(rows, i) {
    let names = { first: "é😀", more: ["ñandú", "ü"] }
    let f = (x) => x + i + len(names.first) + len(rows)
    return f
}
fn boxed_self(rows, i) {
    let f = (x) => x + i + len(rows)
    let _box = [f, 1]
    return f
}
fn alias_rec(rows, i) {
    let f = (x) => x + i + len(rows)
    let m = { go: f }
    let _m2 = m
    let _pad = range(0, 10)
    return 1
}
fn alias_rec_bare(rows, i) {
    let f = (x) => x + i + len(rows)
    let m = { go: f }
    let _m2 = m
    return 1
}
fn alias_list(rows, i) {
    let f = (x) => x + i + len(rows)
    let s = [f]
    let _l = [s]
    return 1
}
fn big_shared(rows, i) {
    let f = (x) => x + i + len(rows)
    let big = range(0, 140) + [f]
    let _small = [big]
    return 1
}
fn big_unique(rows, i) {
    let f = (x) => x + i + len(rows)
    let _big = range(0, 140) + [f]
    return 1
}
fn big_shared_at_limit(rows, i) {
    let f = (x) => x + i + len(rows)
    let big = range(0, 127) + [f]
    let _small = [big]
    return 1
}
fn long_in_long(rows, i) {
    let f = (x) => x + i + len(rows)
    let _outer = range(0, 140) + [range(0, 140) + [f]]
    return 1
}
fn long_map_short_list(rows, i) {
    let f = (x) => x + i + len(rows)
    let m = {}
    for k in range(0, 141) {
        m = set(m, `k${k}`, k)
    }
    m = set(m, "x", [f])
    return len(m) - len(m) + 1
}
fn kept_big(rows, i) {
    let f = (x) => x + i + len(rows)
    let s = range(0, 140)
    let _t = s
    return { go: f, s: s }
}
fn heldret(rows, i) {
    let pad = range(0, 20000)
    let g = (x) => x + i + len(pad) + len(rows)
    return g
}
fn shared_plain(rows, i) {
    let names = rows
    let _also = names
    let f = (x) => x + i + len(names)
    return f
}
"#;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}\n--- source ---\n{}", e, src);
    }
    it
}

/// `name(rows, i)` called `times` times with each lambda kept, each then called with 1: the sum of
/// what they gave, and how many `ring` walks the calls took.
fn kept(it: &mut Interp, name: &str, rows: &Arc<Items>, times: usize) -> (i64, u64, Vec<Value>) {
    let f = lookup(&it.globals, name).unwrap_or_else(|| panic!("no {}", name));
    let walks = ring_walks();
    let mut held = Vec::new();
    let mut sum = 0;
    for i in 0..times {
        let g = it.call(&f, vec![Value::List(rows.clone()), Value::Int(i as i64)], 0).unwrap_or_else(|_| panic!("{} failed", name));
        match it.call(&g, vec![Value::Int(1)], 0) {
            Ok(Value::Int(n)) => sum += n,
            _ => panic!("{}'s lambda failed", name),
        }
        held.push(g);
    }
    (sum, ring_walks() - walks, held)
}

/// The holds left on `rows` besides the test's, after the looks of up to 200 idle seconds.
fn left_after_looks(rows: &Arc<Items>) -> usize {
    for _ in 0..200 {
        if Arc::strong_count(rows) == 1 {
            break;
        }
        look_again();
    }
    Arc::strong_count(rows) - 1
}

// Q-955: a lambda kept beside a list of plain values, or in a record with one, was walked by `ring`
// at its scope's first look, its lists read twice: a walk each call. Now `own` counts their holds
// and keeps a witness: no walk, the same answers, and each scope freed once let go of. On 4818eff
// (with this counter) each kept call walks once.
#[test]
fn a_kept_lambda_beside_a_list_is_told_without_a_walk_and_freed() {
    let mut it = app(SCOPES);
    let rows = Arc::new(Items::new((0..3).map(|i| Value::str(format!("row {} é😀", i))).collect()));
    let times = 300;
    let base: i64 = (0..times as i64).map(|i| 1 + i + 3).sum();
    for (name, extra, walks_most) in [
        ("beside_list", 3, 0),
        ("beside_range", 100, 0),
        ("in_record", 0, 0),
        // The items' limit, and one past it, which `ring` walks as before.
        ("at_limit", 128, 0),
        ("empty_list", 0, 0),
        ("unicode", 2, 0),
        // The lambda in a list of its own scope too: counted there.
        ("boxed_self", 0, 0),
    ] {
        let (sum, walks, held) = kept(&mut it, name, &rows, times);
        assert_eq!(sum, base + extra * times as i64, "{}", name);
        assert!(walks <= walks_most, "Q-955: {} took {} ring walks for {} calls", name, walks, times);
        assert_eq!(Arc::strong_count(&rows) - 1, times, "{}: each kept lambda holds its rows", name);
        drop(held);
        assert_eq!(left_after_looks(&rows), 0, "Q-955: {}'s scopes were not freed once let go of", name);
    }
    // Past the limit: walked by `ring`, and freed all the same.
    let (sum, walks, held) = kept(&mut it, "over_limit", &rows, times);
    assert_eq!(sum, base + 129 * times as i64);
    assert!(walks <= times as u64, "over_limit took {} ring walks", walks);
    drop(held);
    assert_eq!(left_after_looks(&rows), 0);
    // Let go of at once, one by one: freed by the looks the next calls make, and no walk.
    let f = lookup(&it.globals, "in_record").unwrap();
    let walks = ring_walks();
    for i in 0..1000 {
        let g = it.call(&f, vec![Value::List(rows.clone()), Value::Int(i)], 0).unwrap_or_else(|_| panic!("in_record failed"));
        assert!(matches!(it.call(&g, vec![Value::Int(1)], 0), Ok(Value::Int(_))));
    }
    assert_eq!(ring_walks() - walks, 0, "Q-955: in_record let go of at once");
    assert!(Arc::strong_count(&rows) - 1 <= 1);
    assert_eq!(left_after_looks(&rows), 0);
}

// Found in review of Q-955: a list or map the scope holds twice (`let m2 = m`), or that one of its
// own lists holds as well, was passed over by `own` uncounted, and a lambda in it kept its scope
// for ever, each call's: 300 holds left where 4818eff left none (`alias_rec_bare` leaked there
// too). Plain shared values (the rows themselves, twice) still need no walk.
#[test]
fn a_lambda_in_a_list_or_map_the_scope_holds_twice_goes_with_it() {
    let mut it = app(SCOPES);
    let rows = Arc::new(Items::new((0..3).map(|i| Value::str(format!("row {} é😀", i))).collect()));
    for name in ["alias_rec", "alias_rec_bare", "alias_list"] {
        let f = lookup(&it.globals, name).unwrap();
        for i in 0..300 {
            let r = it.call(&f, vec![Value::List(rows.clone()), Value::Int(i)], 0).unwrap_or_else(|_| panic!("{} failed", name));
            assert!(matches!(r, Value::Int(1)), "{}", name);
        }
        assert_eq!(left_after_looks(&rows), 0, "{} left its scopes held", name);
    }
    // A record given back holding the lambda and an aliased long list, let go of at once.
    let f = lookup(&it.globals, "kept_big").unwrap();
    for i in 0..300 {
        let r = it.call(&f, vec![Value::List(rows.clone()), Value::Int(i)], 0).unwrap_or_else(|_| panic!("kept_big failed"));
        assert!(matches!(r, Value::Map(_)));
    }
    assert_eq!(left_after_looks(&rows), 0, "kept_big left its scopes held");
    let (sum, walks, held) = kept(&mut it, "shared_plain", &rows, 300);
    assert_eq!(sum, (0..300i64).map(|i| 1 + i + 3).sum::<i64>());
    assert_eq!(walks, 0, "shared plain values walked");
    drop(held);
    assert_eq!(left_after_looks(&rows), 0);
}

// ---- Q-592's answers: a chain of answers (`respond`) let go of ----------------------------

// Found in review: an answer holding an answer, a million deep, directly, through a list, or
// through a map in turn, was freed one level inside the other and overflowed the stack, as lists
// did (4818eff and W2's first tree both stop after "built").
#[test]
fn a_chain_of_answers_a_million_deep_is_let_go_of() {
    for (name, make) in [("direct", "r = respond(200, r)"), ("list", "r = respond(200, [r])"), ("map", "r = { a: respond(200, r) }")] {
        let src = format!("let r = respond(200, \"é😀\")\nfor i in range(0, 1000000) {{\n    {}\n}}\nprint(\"built\")\nr = 0\nprint(\"done\")\n", make);
        let d = dir(&format!("answers-{}", name), &src);
        assert_eq!(run_script(&d), "built\ndone\n", "{}", name);
    }
}

// ---- Q-1122: records marked by the type that made them ---------------------------------------

const RECORDS: &str = r#"type Node = { v: int, next: Node? }
type Other = { v: int, next: Other?, w: int = 5 }

fn loose(n, bad_at) {
    let x = null
    for i in range(0, n) {
        let v = i == bad_at ? "bad" : i
        x = { v: v, next: x }
    }
    return x
}

fn chain(n) {
    let c = Node({ v: 0 })
    for i in range(1, n) {
        c = Node({ v: i, next: c })
    }
    return c
}

fn shape(x) {
    return match x {
        Node as n => "node",
        _ => "not",
    }
}

print(is_err(Node(loose(100, 20))))
print(is_err(Node(loose(100, 90))))
print(is_err(Node(loose(100, -1))))
print(Node(loose(100, -1)).next.next.v)
let c = chain(100000)
print(c.v)
print(is_err(Node({ v: -1, next: c })))
print(is_err(Node(set(c, "v", "x"))))
print(is_err(Node({ ...c, v: "y" })))
print(is_err(Node(set(c, "next", set(c.next, "v", "z")))))
print(Other(Node({ v: 1 })).w)
print(Other(Node({ v: 1, next: Node({ v: 2 }) })).next.w)
print(Node({ v: 1 }) == { v: 1 })
print(json.stringify(Node({ v: 1, next: Node({ v: 2 }) })))
print(shape(loose(100, 20)))
print(shape(loose(100, -1)))
print(shape(c))
let far = Node(loose(20000, -1))
print(is_err(far))
print(far.error)
print(is_err(Node(loose(9000, -1))))
print(str(parallel(() => is_err(Node(loose(9000, -1))))))
"#;

// Q-1122: a constructor checked a value 64 levels deep and took the rest as it came; records it
// makes now carry their type's mark, a marked map is taken at once, and the rest is checked to the
// end (refused past 10,000 levels). On 4818eff the second-level bad value 80 down is taken (the
// first `false`), the pattern says "node", and the 20,000-deep value is no error.
#[test]
fn a_constructor_checks_to_the_end_and_takes_its_own_records_at_once() {
    let d = dir("records", RECORDS);
    let out = run_script(&d);
    // The deepest's path, shortened to its end.
    let lines: Vec<&str> = out.lines().map(|l| if l.starts_with("Node: `…next.next.") && l.ends_with(".v` is nested more than 10000 deep") { "FAR" } else { l }).collect();
    assert_eq!(
        lines,
        [
            "true",
            "true",
            "false",
            "97",
            "99999",
            "false",
            "true",
            "true",
            "true",
            "5",
            "5",
            "true",
            "{\"v\":1,\"next\":{\"v\":2}}",
            "not",
            "node",
            "node",
            "true",
            "FAR",
            "false",
            "[false]",
        ],
        "{}",
        out
    );
}

// Q-606 leftover: a single `json.stringify` call over shared exponential parts respects
// `--max-compute` without stalling the request past the budget.
#[test]
fn long_json_stringify_stops_within_max_compute() {
    let src = r#"route GET /exp {
    let xs = [1]
    for i in range(0, 24) { xs = [xs, xs] }
    str(len(json.stringify(xs)))
}
route GET /ping = "pong"
"#;
    let d = dir("compute-json", src);
    let s = support::serve_with(&d.join("main.qwe"), &["--max-compute", "150ms"]);
    let started = std::time::Instant::now();
    let r = support::send(s.port, "GET", "/exp", &[], "");
    let took = started.elapsed();
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(took < std::time::Duration::from_secs(3), "took {:?}", took);
    assert_eq!(support::send(s.port, "GET", "/ping", &[], "").body, "pong");
}
