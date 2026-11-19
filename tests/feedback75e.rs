// FEEDBACK75.md, slice E (branch feedback75): the security audit of Quillo 0.8.7, on databases,
// translations, the offline worker, service callers and records. Q-1080: recursive records past 64
// levels were taken unchecked. Q-1079: an Okta user's own token passed as a service's. Q-1059 and
// Q-1083: `raw(t(...))` wrote a missing key from the request as HTML, and a translation's `href`
// took a `javascript:` URL. Q-1060: PostgreSQL's `query_to_xml` and `update pg_settings` changed
// the session every request shares. Q-1078 and Q-1082: the offline worker's gate failed open under
// a sandbox without `allow-same-origin`, and a sign-out that redirects to another site left the
// person's kept pages on the device.

mod support;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A folder with `main.qwe`, removed when the value goes.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn dir(name: &str, src: &str) -> Dir {
    let d = support::temp_dir(&format!("f75e-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    Dir(d)
}

/// A process with a deadline: its status, stdout and stderr, read as it runs (a full pipe never
/// holds it), killed past `secs`.
fn within(mut c: Command, secs: u64) -> (std::process::ExitStatus, String, String) {
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = r.read_to_end(&mut s);
            String::from_utf8_lossy(&s).into_owned()
        })
    };
    let (out, err) = (read(Box::new(child.stdout.take().unwrap())), read(Box::new(child.stderr.take().unwrap())));
    let end = Instant::now() + Duration::from_secs(secs);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{:?} did not end within {} s", c, secs);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    (status, out.join().unwrap(), err.join().unwrap())
}

/// `quillo check` of `d`'s main.qwe: whether it passed, and what it said.
fn check(d: &Path) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("check").arg(d.join("main.qwe"));
    let (status, out, err) = within(c, 60);
    (status.success(), out + &err)
}

/// `quillo run` of `d`'s main.qwe, a program with no routes: what it printed, and said.
fn run_script(d: &Path) -> (String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.arg("run").arg(d.join("main.qwe")).current_dir(d);
    let (_, out, err) = within(c, 60);
    (out, err)
}

fn checked(d: &Path) {
    let (ok, said) = check(d);
    assert!(ok, "{}", said);
}

// ---- Q-1080: recursive records ----------------------------------------------------------------

const RECORDS: &str = r#"type Node = { value: int, kids: list<Node> = [] }
type Team = { name: str, role: "member" | "viewer" = "viewer", sub: list<Team> = [] }
server fn tree(t: Node) {
    return t
}
route POST /teams { json: Team } = { accepted: json }
route GET / = "ok"
"#;

/// `leaf` under `wrappers` nodes, each `{"<field>": ..., "<list>": [<the one below>]}`.
fn nested(wrappers: usize, open: &str, list: &str, leaf: &str) -> String {
    let mut s = String::new();
    for _ in 0..wrappers {
        s.push_str(&format!("{{{},\"{}\":[", open, list));
    }
    s.push_str(leaf);
    for _ in 0..wrappers {
        s.push_str("]}");
    }
    s
}

/// A server fn's recursive record and a route's `{ json: T }`, a node nested past the old limit
/// (33 deep, 66 levels) with a wrong type, a value outside the set and a field the record does
/// not have: refused (400), where it was taken as it came; one less deep, refused as before; at
/// the deepest JSON lets in (64 nodes, 128 levels), checked to its end; one past it, refused by
/// JSON. A right one that deep is made as the record says: its defaults put in, `is_admin` left
/// out, unicode kept (FEEDBACK75.md, Q-1080; branch feedback75).
#[test]
fn q1080_a_recursive_record_nested_past_64_levels_is_checked_through_server_fns_and_routes() {
    let d = dir("q1080-rpc", RECORDS);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let rpc = |body: &str| support::send(s.port, "POST", "/_quillo/rpc/tree", &[("content-type", "application/json")], body);
    let node = |i: usize| format!("\"value\":{}", i);
    let bad = r#"{"value":"NOT AN INT","kids":"not a list","is_admin":true}"#;
    for wrappers in [30, 32, 33, 40, 63] {
        let r = rpc(&format!("[{}]", nested(wrappers, &node(0), "kids", bad)));
        assert_eq!(r.status, 400, "{} deep: {}", wrappers, r.body.chars().take(300).collect::<String>());
        assert!(r.body.contains("must be int, got str") && !r.body.contains("is_admin\":true"), "{} deep: {}", wrappers, r.body.chars().take(300).collect::<String>());
        // A long way in is told by its innermost end (FEEDBACK75.md, Q-1080's review; branch
        // feedback75).
        assert!(r.body.contains("(`…") && r.body.contains(".kids[0].value` must be int"), "{} deep: {}", wrappers, r.body);
        assert!(r.body.len() < 600, "{} deep: {}", wrappers, r.body.len());
    }
    // One past what JSON lets in: refused by JSON, as before.
    let r = rpc(&format!("[{}]", nested(64, &node(0), "kids", bad)));
    assert_eq!(r.status, 400, "{}", r.body.chars().take(300).collect::<String>());
    assert!(r.body.contains("nests more than 128 deep"), "{}", r.body.chars().take(300).collect::<String>());
    // A right one, as deep: made, the leaf's `kids` put in, `is_admin` left out.
    for wrappers in [0, 1, 33, 63] {
        let r = rpc(&format!("[{}]", nested(wrappers, &node(1), "kids", r#"{"value":7,"is_admin":true}"#)));
        assert_eq!(r.status, 200, "{} deep: {}", wrappers, r.body.chars().take(300).collect::<String>());
        let want = format!("{{\"ok\":{}}}", nested(wrappers, &node(1), "kids", r#"{"value":7,"kids":[]}"#));
        assert_eq!(r.body, want, "{} deep", wrappers);
    }
    // Empty: the field missing, said so.
    let r = rpc("[{}]");
    assert!(r.status == 400 && r.body.contains("`value` is missing"), "{} {}", r.status, r.body);

    // The route's pattern: a set's value and a list that is not one, 31 and 33 deep, refused.
    let team = |i: usize| format!("\"name\":\"t{}\"", i);
    let wrong = r#"{"name":"x","role":"owner","is_admin":true,"sub":"not a list"}"#;
    for wrappers in [31, 33, 50] {
        let r = support::send(s.port, "POST", "/teams", &[("content-type", "application/json")], &nested(wrappers, &team(0), "sub", wrong));
        assert!(r.status == 400 || r.status == 404, "{} deep: {} {}", wrappers, r.status, r.body.chars().take(300).collect::<String>());
        assert!(!r.body.contains("owner"), "{} deep: {}", wrappers, r.body.chars().take(300).collect::<String>());
    }
    let right = r#"{"name":"Zoë 🚀","role":"member","is_admin":true}"#;
    let r = support::send(s.port, "POST", "/teams", &[("content-type", "application/json")], &nested(33, &team(0), "sub", right));
    assert_eq!(r.status, 200, "{}", r.body.chars().take(300).collect::<String>());
    let made = nested(33, &team(0), "sub", r#"{"name":"Zoë 🚀","role":"member","sub":[]}"#);
    // Each wrapper was made too: its role's default put in.
    let made = made.replace("\"name\":\"t0\",\"sub\"", "\"name\":\"t0\",\"role\":\"viewer\",\"sub\"");
    assert_eq!(r.body, format!("{{\"accepted\":{}}}", made));
}

/// The program's own values keep 0.8.8's checks and cost (the review's decision): a record made
/// (`Node(v)`) and an argument checked (`fn f(t: Node)`) 300 deep, and a 513-long `Link` passed to
/// `fn f(l: Link)`, are taken (they were a 500 with a 2.6 KB message); a wrong leaf below 64 levels
/// is taken unchecked as before, one above refused; a chain of types naming types (`type Ty1 =
/// Ty2`) is followed; empty, one node and unicode (FEEDBACK75.md, Q-1080 and its reviews; branch
/// feedback75).
#[test]
fn q1080_the_programs_own_values_are_checked_as_before_and_types_that_name_themselves() {
    let src = r#"type Node = { value: int, kids: list<Node> = [] }
type Link = { v: int, next: Link? }
fn chain(n: int, leaf) {
    if n == 0 {
        return leaf
    }
    return { value: n, kids: [chain(n - 1, leaf)] }
}
fn build(n: int) {
    if n == 0 {
        return null
    }
    return { v: n, next: build(n - 1) }
}
fn deep_value(t: Node) = t.value
fn head(l: Link) = l.v
fn said(v) = is_err(v) ? v.error : "made"
let good = { value: 0, kids: [] }
let wrong = json.parse("{\"value\": \"x\", \"kids\": []}")
print(said(Node(chain(300, good))))
print(deep_value(chain(300, good)))
print(head(build(513)))
print(said(Node(chain(40, wrong))))
print(deep_value(chain(40, wrong)))
print(Node({}))
print(Node({ value: 1 }))
print(Node(json.parse("{\"value\": \"ü\"}")))
print(deep_value(chain(10, wrong)))
"#;
    let d = dir("q1080-program", src);
    checked(&d);
    let start = Instant::now();
    let (out, err) = run_script(&d);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 8, "{}\n{}", out, err);
    assert_eq!(&lines[..5], ["made", "300", "513", "made", "40"], "{}", out);
    assert_eq!(lines[5], "err(\"Node: `value` is missing\")");
    assert_eq!(lines[6], "{ value: 1, kids: [] }");
    assert_eq!(lines[7], "err(\"Node: `value` must be int, got str \\\"ü\\\"\")");
    assert!(err.contains("argument `t` of `deep_value` must be Node") && err.contains("must be int, got str \"x\""), "{}", err);

    // Types naming other types in a row are followed however many there are.
    for (n, want) in [(1, "5"), (2, "5"), (9, "5"), (40, "5")] {
        let mut src: String = (1..n).map(|i| format!("type Ty{} = Ty{}\n", i, i + 1)).collect();
        src.push_str(&format!("type Ty{} = int\nfn f(x: Ty1) = x\nprint(f(5))\n", n));
        let d = dir(&format!("q1080-chain{}", n), &src);
        checked(&d);
        let (out, err) = run_script(&d);
        assert_eq!(out.trim(), want, "{} types: {}", n, err);
    }
    assert!(start.elapsed() < Duration::from_secs(30), "{:?}", start.elapsed());
}

/// A type that names itself, through types naming types with nothing that reads deeper into the
/// value between (`type Ab = Ab | int`, `type Ab = Ab?`, `Aa`/`Bb` naming each other), is found
/// once over the definitions (`record::self_naming`), for `quillo check` to refuse; a chain of
/// other types, a record, a list or a map shape between, are not. A request's walk refuses what
/// such a type adds nothing to, at once however it names itself (FEEDBACK75.md, Q-1080's second
/// review; branch feedback75).
#[test]
fn q1080_types_that_name_themselves_are_found_once_and_refused_in_requests() {
    let src = r#"type Ab = Ab | int
type Opt = Opt?
type Cc = Cc | Cc | int
type Aa = Bb | int
type Bb = Aa | str
type Ty1 = Ty2
type Ty2 = Ty3
type Ty3 = int | str
type Node = { value: int, kids: list<Node> = [] }
type Jv = list<Jv> | int
type Sh = { w: int, more: Sh? }
type Boxed = { ab: Ab, cc: Cc, aa: Aa }
route POST /ab { json: Boxed } = { made: json }
"#;
    let mut it = quillo::Interp::new(&std::env::temp_dir());
    it.load_source(src, "app.qwe").unwrap();
    assert_eq!(quillo::record::self_naming_in(&it), ["Aa", "Ab", "Bb", "Cc", "Opt"]);
    let d = dir("q1080-self-naming", src);
    let s = support::serve_unchecked(&d.join("main.qwe"));
    let post = |body: &str| support::send(s.port, "POST", "/ab", &[("content-type", "application/json")], body);
    let r = post(r#"{"ab":3,"cc":4,"aa":"x"}"#);
    assert_eq!(r.body, r#"{"made":{"ab":3,"cc":4,"aa":"x"}}"#);
    for body in [r#"{"ab":"s","cc":4,"aa":1}"#, r#"{"ab":3,"cc":"s","aa":1}"#, r#"{"ab":[1],"cc":4,"aa":1}"#, r#"{"ab":3,"cc":4,"aa":true}"#] {
        let t = Instant::now();
        let r = post(body);
        assert!((r.status == 400 || r.status == 404) && t.elapsed() < Duration::from_secs(1), "{}: {} {:?}", body, r.status, t.elapsed());
    }
}

/// A typed function over a chain the program built takes time in its length, as in 0.8.8: each
/// call checks its argument 64 levels deep at most. Checked to the end, a `size(l: Link?)` over
/// 500 nodes took 4 times what 250 did, and 4 times 0.8.8's (FEEDBACK75.md, Q-1080's review;
/// branch feedback75).
#[test]
fn q1080_a_typed_walk_over_a_long_chain_stays_linear() {
    let src = r#"type Link = { v: int, next: Link? }
fn build(n: int) {
    if n == 0 {
        return null
    }
    return { v: n, next: build(n - 1) }
}
fn size(l: Link?) -> int {
    if l == null {
        return 0
    }
    return 1 + size(l.next)
}
fn timed(l, times: int) {
    let start = now()
    for i in 0..times {
        size(l)
    }
    return now() - start
}
let short = build(250)
let long = build(500)
let runs = for r in 0..5 => [timed(short, 40), timed(long, 40)]
print(min(runs | map((r) => r[0])))
print(min(runs | map((r) => r[1])))
print(size(long))
"#;
    let d = dir("q1080-linear", src);
    checked(&d);
    let (out, err) = run_script(&d);
    let n: Vec<i64> = out.lines().map(|l| l.trim().parse().unwrap_or(-1)).collect();
    assert_eq!(n.len(), 3, "{}\n{}", out, err);
    assert_eq!(n[2], 500);
    // Linear: twice the nodes, about twice the time (four times when checked to the end).
    assert!(n[1] <= 3 * n[0].max(1) + 2, "250 nodes: {} ms, 500 nodes: {} ms", n[0], n[1]);
}

/// A `match` in the program (`match v { Node as n => ... }`) makes the record as 0.8.8 made it: a
/// value the program built 600 deep matches, where it was refused past the request's limit; a
/// wrong leaf 10 levels down still does not. A request's `json` field, nested in the route's
/// pattern, is made by the request's walk: a wrong leaf 33 nodes down is refused (FEEDBACK75.md,
/// Q-1080's second review; branch feedback75).
#[test]
fn q1080_a_programs_match_is_made_as_before_and_a_requests_json_by_the_request() {
    let src = r#"type Node = { value: int, kids: list<Node> = [] }
fn chain(n: int, leaf) {
    if n == 0 {
        return leaf
    }
    return { value: n, kids: [chain(n - 1, leaf)] }
}
fn kind(v) {
    return match v {
        Node as n => `node ${n.value}`,
        _ => "not a node",
    }
}
route POST /m { json: { t: Node } } = { made: t.value }
route GET /prog = [kind(chain(600, { value: 0, kids: [] })), kind(chain(10, { value: "x", kids: [] })), kind(5)]
"#;
    let d = dir("q1080-match", src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let r = support::send(s.port, "GET", "/prog", &[], "");
    assert_eq!(r.body, r#"["node 600","not a node","not a node"]"#);
    let deep = |leaf: &str| format!("{{\"t\":{}}}", nested(33, "\"value\":1", "kids", leaf));
    let r = support::send(s.port, "POST", "/m", &[("content-type", "application/json")], &deep(r#"{"value":"x"}"#));
    assert!(r.status == 400 || r.status == 404, "{} {}", r.status, r.body.chars().take(200).collect::<String>());
    let r = support::send(s.port, "POST", "/m", &[("content-type", "application/json")], &deep(r#"{"value":7}"#));
    assert_eq!((r.status, r.body.as_str()), (200, r#"{"made":1}"#));
}

/// A value from a request is checked to `DEPTH` (512 levels, types named on the way counted) and
/// refused past it: through a type that names eight others at each level of a list, 56 lists
/// deep (512 levels) is taken and 57 refused, one either side too, with a short path in the
/// message (FEEDBACK75.md, Q-1080 and its review; branch feedback75).
#[test]
fn q1080_a_request_value_is_held_to_the_limit_and_one_either_side() {
    let mut src: String = (1..8).map(|i| format!("type Ty{} = Ty{}\n", i, i + 1)).collect();
    src.push_str("type Ty8 = list<Ty1> | int\nserver fn depth(x: Ty1) -> int {\n    return 1\n}\nroute GET / = \"ok\"\n");
    let d = dir("q1080-request-limit", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let call = |lists: usize| {
        let arg = format!("{}1{}", "[".repeat(lists), "]".repeat(lists));
        support::send(s.port, "POST", "/_quillo/rpc/depth", &[("content-type", "application/json")], &format!("[{}]", arg))
    };
    for lists in [0, 1, 55, 56] {
        let r = call(lists);
        assert_eq!((r.status, r.body.as_str()), (200, r#"{"ok":1}"#), "{} lists", lists);
    }
    for lists in [57, 58] {
        let r = call(lists);
        assert_eq!(r.status, 400, "{} lists: {}", lists, r.body);
        assert!(r.body.contains("is nested more than 512 deep") && r.body.len() < 600, "{} lists: {}", lists, r.body);
    }
}

/// A union whose alternatives hold records that hold the union, all valid (`Tt = Aa | Bb`, each
/// with a `k: list<Tt>`): each value is tried once, the first alternative's answer for the level
/// below kept for the second; 22 and 63 levels deep took 7 s and more (2^k), now well under a
/// second by a route's pattern, made as before. A server fn's argument is made the same way, and
/// its typed parameter no longer checks it again by the program's walk, 0.8.8's, which is
/// exponential: the same depths answer within a second by RPC too (FEEDBACK75.md, Q-1080's second
/// review and its follow-up; branch feedback75).
#[test]
fn q1080_a_valid_union_of_recursive_records_is_made_in_linear_time() {
    let src = r#"type Tt = Aa | Bb
type Aa = { k: list<Tt> = [], a: int }
type Bb = { k: list<Tt> = [], b: int }
route POST /t { json: Tt } = { made: json }
server fn tt(t: Tt) -> int {
    return 1
}
"#;
    let d = dir("q1080-valid-union", src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let nested = |k: usize| format!("{}{{\"b\":1}}{}", "{\"k\":[".repeat(k), "],\"b\":1}".repeat(k));
    for k in [0, 1, 22, 63] {
        let t = Instant::now();
        let r = support::send(s.port, "POST", "/t", &[("content-type", "application/json")], &nested(k));
        assert_eq!(r.status, 200, "k={}: {}", k, r.body.chars().take(200).collect::<String>());
        // Made: each level's `k` as it came, the innermost's put in.
        let made = format!("{}{{\"k\":[],\"b\":1}}{}", "{\"k\":[".repeat(k), "],\"b\":1}".repeat(k));
        assert_eq!(r.body, format!("{{\"made\":{}}}", made), "k={}", k);
        assert!(t.elapsed() < Duration::from_secs(1), "k={}: {:?}", k, t.elapsed());
    }
    for k in [0, 1, 6, 22, 63] {
        let t = Instant::now();
        let rpc = support::send(s.port, "POST", "/_quillo/rpc/tt", &[("content-type", "application/json")], &format!("[{}]", nested(k)));
        assert_eq!(rpc.body, r#"{"ok":1}"#, "k={}", k);
        assert!(t.elapsed() < Duration::from_secs(1), "rpc k={}: {:?}", k, t.elapsed());
    }
    // In the program: a typed function and a constructor, as 0.8.8 checked them.
    let prog = dir("q1080-valid-union-program", &format!("{}fn size(t: Tt) -> int {{\n    return len(t.k)\n}}\nlet v = json.parse(read_file(\"v.json\"))\nprint(size(Tt(v)))\nprint(len(Tt(v).k))\n", src.lines().take(3).collect::<Vec<_>>().join("\n") + "\n"));
    std::fs::write(prog.join("v.json"), nested(6)).unwrap();
    checked(&prog);
    let (out, err) = run_script(&prog);
    assert_eq!(out.trim(), "1\n1", "{}", err);
    // The first alternative where both fit; neither where neither does.
    let r = support::send(s.port, "POST", "/t", &[("content-type", "application/json")], r#"{"k":[{"a":2,"b":3}],"a":1}"#);
    assert_eq!(r.body, r#"{"made":{"k":[{"k":[],"a":2}],"a":1}}"#);
    let r = support::send(s.port, "POST", "/t", &[("content-type", "application/json")], &format!("{}{{\"c\":1}}{}", "{\"k\":[".repeat(30), "],\"b\":1}".repeat(30)));
    assert!(r.status == 400 || r.status == 404, "{} {}", r.status, r.body.chars().take(200).collect::<String>());
}

/// A server fn's arguments made from an RPC are not checked a second time, and nothing else is
/// left unchecked by it: a deep valid `Tt` 22 and 63 levels deep answers within a second (it took
/// minutes, 2^k), one wrong deep down is refused; a default is checked as before; a function the
/// server fn calls checks its argument; a server fn called from the program's own code checks its
/// parameter, also after an RPC call (FEEDBACK75.md, Q-1080, found in review; branch feedback75).
#[test]
fn q1080_a_server_fns_made_arguments_are_checked_once_and_the_rest_as_before() {
    let src = r#"type Tt = Aa | Bb
type Aa = { k: list<Tt> = [], a: int }
type Bb = { k: list<Tt> = [], b: int }
server fn tt(t: Tt) -> int {
    return 1
}
fn inner(a: Aa) -> int {
    return a.a
}
server fn via(t: Tt) -> int {
    return inner(t)
}
server fn dflt(n: int, t: Tt = json.parse("{\"c\":1}")) -> int {
    return n
}
route GET /direct = tt(json.parse("{\"c\":1}"))
route GET /fine = tt(json.parse("{\"k\":[],\"b\":1}"))
"#;
    let d = dir("q1080-made-once", src);
    let s = support::serve_unchecked(&d.join("main.qwe"));
    let rpc = |f: &str, body: &str| support::send(s.port, "POST", &format!("/_quillo/rpc/{}", f), &[("content-type", "application/json")], body);
    let nested = |k: usize, leaf: &str| format!("{}{}{}", "{\"k\":[".repeat(k), leaf, "],\"b\":1}".repeat(k));
    for k in [22, 63] {
        let t = Instant::now();
        assert_eq!(rpc("tt", &format!("[{}]", nested(k, r#"{"b":1}"#))).body, r#"{"ok":1}"#, "k={}", k);
        // 0.25 ms in a release build, where the second check alone took 0.6 s at k=63.
        assert!(t.elapsed() < Duration::from_millis(250), "k={}: {:?}", k, t.elapsed());
        let wrong = rpc("tt", &format!("[{}]", nested(k, r#"{"c":1}"#)));
        assert_eq!(wrong.status, 400, "k={}: {}", k, wrong.body.chars().take(200).collect::<String>());
        assert!(t.elapsed() < Duration::from_secs(2), "k={}: {:?}", k, t.elapsed());
    }
    // The function it calls checks what it is given: a `Bb` is not an `Aa`.
    assert_eq!(rpc("via", r#"[{"a":5}]"#).body, r#"{"ok":5}"#);
    let r = rpc("via", r#"[{"b":5}]"#);
    assert_ne!(r.status, 200, "{}", r.body);
    // A default is the program's own value, checked as before; given, the argument is made.
    assert_ne!(rpc("dflt", "[1]").status, 200);
    assert_eq!(rpc("dflt", r#"[1, {"b":1}]"#).body, r#"{"ok":1}"#);
    // Called from the program's code, before and after an RPC call: its parameter is checked.
    for _ in 0..2 {
        assert_eq!(support::send(s.port, "GET", "/direct", &[], "").status, 500);
        assert_eq!(support::send(s.port, "GET", "/fine", &[], "").status, 200);
        assert_eq!(rpc("tt", r#"[{"b":1}]"#).body, r#"{"ok":1}"#);
    }
}

/// `quillo check` refuses a type that names itself with nothing between, alone (`type Ab = Ab |
/// int`) or through another (`Aa`/`Bb`), saying what to put in the way (through a `?`, a record or
/// a list), and says nothing of recursive records: a tree, a linked
/// list, records naming each other, a union of records holding it, a list of itself, a chain of
/// types naming types (FEEDBACK75.md, Q-1080, found in review; branch feedback75).
#[test]
fn q1080_quillo_check_refuses_a_type_that_names_itself() {
    let d = dir("q1080-check-self", "type Ab = Ab | int\nfn f(x: Ab) {\n    return x\n}\nprint(f(1))\n");
    let (ok, said) = check(&d);
    assert!(!ok && said.contains("type Ab names itself") && said.contains("`type Ab = list<Ab> | int`"), "{}", said);
    // Through a `?`, alone or on another type of the loop: a record or a list in the way.
    let d = dir("q1080-check-opt", "type Opt = Opt?\ntype Pa = Pb?\ntype Pb = Pa | int\nfn f(x: Opt, y: Pa) {\n    return x\n}\nprint(f(null, 1))\n");
    let (ok, said) = check(&d);
    assert!(!ok, "{}", said);
    for n in ["Opt", "Pa", "Pb"] {
        assert!(said.contains(&format!("type {} names itself with nothing between: put a record or a list in the way (`type {} = {{ next: {}? }}`)", n, n, n)), "{}: {}", n, said);
    }
    let d = dir("q1080-check-two", "type Aa = Bb | int\ntype Bb = Aa | str\nfn f(x: Aa) {\n    return x\n}\nprint(f(1))\n");
    let (ok, said) = check(&d);
    assert!(!ok && said.contains("type Aa names itself") && said.contains("type Bb names itself"), "{}", said);
    let d = dir(
        "q1080-check-fine",
        r#"type Tree = { value: int, kids: list<Tree> = [] }
type Link = { v: int, next: Link? }
type Person = { name: str, team: Team? }
type Team = { name: str, lead: Person? }
type Expr = Num | Add
type Num = { n: int }
type Add = { l: Expr, r: Expr }
type Jv = list<Jv> | int | str
type Ty1 = Ty2
type Ty2 = int | str
let t = Tree({ value: 1 })
let l = Link({ v: 1, next: Link({ v: 2, next: null }) })
let p = Person({ name: "a", team: Team({ name: "t", lead: null }) })
fn ex(e: Expr) {
    return e.l.n
}
fn jv(j: Jv) {
    return len(j)
}
fn ty(y: Ty1) {
    return y
}
print(t.value + l.next.v + len(p.team.name) + ty(3))
print(ex(Add({ l: Num({ n: 1 }), r: Num({ n: 2 }) })), jv([[1], "a"]))
"#,
    );
    let (ok, said) = check(&d);
    assert!(ok && !said.contains("names itself"), "{}", said);
    let (out, err) = run_script(&d);
    assert!(out.starts_with("7\n"), "{} {}", out, err);
}

/// The review's union of lists: an answer kept for a type made for one step (a list's item type,
/// copied, then dropped) was taken for the next step's copy at the same address, and a valid
/// request was refused (`{"u":[[true]]}` after `{"u":[[1]]}`). Now only the program's types are
/// keys. And a seeded fuzz of valid values of nested unions of lists, maps and records, every one
/// taken as 0.8.8 took it, with a value made invalid refused (FEEDBACK75.md, Q-1080's second
/// review; branch feedback75).
#[test]
fn q1080_valid_values_of_nested_unions_are_taken_as_0_8_8_took_them() {
    let src = r#"type Uu = Lone | Ltwo
type Lone = list<Ai | As>
type Ltwo = list<Ab | Am>
type Ai = list<int>
type As = list<str>
type Ab = list<bool>
type Am = map
type Item = { u: Uu, tag: "x" | "y" = "x", shape: { w: int, h: int? }? }
type Big = { xs: list<Item> }
route POST /big { json: Big } = len(json.xs)
server fn big(b: Big) -> int {
    return len(b.xs)
}
"#;
    let d = dir("q1080-fuzz", src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let post = |body: &str| support::send(s.port, "POST", "/big", &[("content-type", "application/json")], body);
    let rpc = |body: &str| support::send(s.port, "POST", "/_quillo/rpc/big", &[("content-type", "application/json")], &format!("[{}]", body));
    let r = post(r#"{"xs":[{"u":[[1]]},{"u":[[true]]}]}"#);
    assert_eq!((r.status, r.body.as_str()), (200, "2"), "the review's");
    // A deterministic generator (no seed from the clock: a failure is the same each run).
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let leaf = |kind: u64, next: &mut dyn FnMut(u64) -> u64| -> String {
        let n = next(4) as usize;
        let items: Vec<String> = (0..n)
            .map(|i| match kind {
                0 => format!("{}", next(100) as i64 - 50),
                1 => format!("\"s{}ü\"", i),
                _ => (if next(2) == 0 { "true" } else { "false" }).to_string(),
            })
            .collect();
        format!("[{}]", items.join(","))
    };
    for case in 0..250 {
        let items = 1 + next(6);
        let mut xs = Vec::new();
        for _ in 0..items {
            let u = if next(2) == 0 {
                // Lone: lists of ints or of strings, mixed.
                let n = next(4);
                let parts: Vec<String> = (0..n).map(|_| leaf(next(2), &mut next)).collect();
                format!("[{}]", parts.join(","))
            } else {
                // Ltwo: lists of bools or maps, mixed.
                let n = next(4);
                let parts: Vec<String> = (0..n).map(|_| if next(2) == 0 { leaf(2, &mut next) } else { format!("{{\"m\":{}}}", next(9)) }).collect();
                format!("[{}]", parts.join(","))
            };
            let tag = ["", ",\"tag\":\"x\"", ",\"tag\":\"y\""][next(3) as usize];
            let shape = ["", ",\"shape\":{\"w\":1}", ",\"shape\":{\"w\":2,\"h\":3}", ",\"shape\":null"][next(4) as usize];
            xs.push(format!("{{\"u\":{}{}{}}}", u, tag, shape));
        }
        let body = format!("{{\"xs\":[{}]}}", xs.join(","));
        let r = post(&body);
        assert_eq!((r.status, r.body.as_str()), (200, items.to_string().as_str()), "case {}: {}", case, body);
        assert_eq!(rpc(&body).body, format!("{{\"ok\":{}}}", items), "case {}: {}", case, body);
        // Made invalid: a list of a number and a string fits neither list of the union.
        let bad = body.replacen("{\"u\":[", "{\"u\":[[1,\"a\"],", 1);
        let r = post(&bad);
        assert!(r.status == 400 || r.status == 404, "case {}: {} {}", case, r.status, bad);
    }
}

/// A union of records that hold the union (`Expr = Num | Add | Mul`) is tried once for each
/// value in a request's walk: the review's body, a bad leaf 24 and 63 levels down, took 7-9 s at
/// 24 and doubled every level; now well under a second, by a route's pattern and by a server fn's
/// argument. A right value is made as before, by its first branch that fits; an ordinary union of
/// records too (FEEDBACK75.md, Q-1080's review; branch feedback75).
#[test]
fn q1080_a_union_of_recursive_records_is_tried_once_for_each_value() {
    let src = r#"type Expr = Num | Add | Mul
type Num = { n: int }
type Add = { l: Expr, r: Expr }
type Mul = { l: Expr, r: Expr, times: bool = true }
type Shape = Circle | Square
type Circle = { radius: int }
type Square = { side: int }
route POST /e { json: Expr } = { made: json }
route POST /shape { json: Shape } = { made: json }
server fn ev(e: Expr) -> int {
    return 1
}
"#;
    let d = dir("q1080-union", src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let nested = |k: usize, leaf: &str| format!("{}{}{}", "{\"l\":".repeat(k), leaf, ",\"r\":{\"n\":1}}".repeat(k));
    for k in [24, 63] {
        let body = nested(k, "{\"n\":\"bad\"}");
        let t = Instant::now();
        let r = support::send(s.port, "POST", "/e", &[("content-type", "application/json")], &body);
        assert!(r.status == 400 || r.status == 404, "k={}: {} {}", k, r.status, r.body.chars().take(200).collect::<String>());
        let rpc = support::send(s.port, "POST", "/_quillo/rpc/ev", &[("content-type", "application/json")], &format!("[{}]", body));
        assert_eq!(rpc.status, 400, "k={}: {}", k, rpc.body.chars().take(200).collect::<String>());
        assert!(t.elapsed() < Duration::from_secs(1), "k={}: {:?}", k, t.elapsed());
    }
    // Right: made by the first branch that fits (`Add`, which `Mul` would take too).
    let r = support::send(s.port, "POST", "/e", &[("content-type", "application/json")], &nested(3, "{\"n\":7,\"x\":1}"));
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, format!("{{\"made\":{}}}", nested(3, "{\"n\":7}")));
    let r = support::send(s.port, "POST", "/_quillo/rpc/ev", &[("content-type", "application/json")], &format!("[{}]", nested(40, "{\"n\":7}")));
    assert_eq!(r.body, r#"{"ok":1}"#);
    for (body, want) in [(r#"{"side":3}"#, r#"{"made":{"side":3}}"#), (r#"{"radius":2,"side":3}"#, r#"{"made":{"radius":2}}"#)] {
        assert_eq!(support::send(s.port, "POST", "/shape", &[("content-type", "application/json")], body).body, want);
    }
    let r = support::send(s.port, "POST", "/shape", &[("content-type", "application/json")], r#"{"edges":3}"#);
    assert!(r.status == 400 || r.status == 404, "{} {}", r.status, r.body);
}

// ---- Q-1079: service callers' tokens -----------------------------------------------------------

/// Okta's tokens name the client in `cid`: a user's (the login in `sub`) is refused as a
/// service's own; a client's (`sub` is `cid`) is the client's, named so. `client_id` and `azp`
/// come first, as before; unicode names; Q-665's forms as they were (FEEDBACK75.md, Q-1079; branch
/// feedback75).
#[test]
fn q1079_an_okta_users_token_is_not_a_services_own() {
    use quillo::svcauth::jwt_caller;
    use serde_json::json;
    let name = |c: serde_json::Value| jwt_caller(&c).map(|v| v.display().split("name: \"").nth(1).unwrap_or("").split('"').next().unwrap_or("").to_string());
    // The report's: an Okta user's access token, for this audience.
    let e = name(json!({ "iss": "https://idp.test", "aud": "billing", "cid": "shop", "uid": "00u1", "sub": "alice@example.com", "scp": ["billing"] })).unwrap_err();
    assert!(e.contains("the token is one a user signed in for (sub \"alice@example.com\"), not client \"shop\"'s own"), "{}", e);
    assert!(name(json!({ "cid": "shöp", "sub": "älice" })).unwrap_err().contains("not client \"shöp\"'s own"));
    // Okta's client-credentials token: `sub` is the client.
    assert_eq!(name(json!({ "cid": "0oa1shop", "sub": "0oa1shop", "scp": ["billing"] })).unwrap(), "0oa1shop");
    assert_eq!(name(json!({ "cid": "shöp", "sub": "shöp" })).unwrap(), "shöp");
    // `client_id` and `azp` first, as before.
    assert_eq!(name(json!({ "client_id": "a", "azp": "b", "cid": "c", "sub": "a" })).unwrap(), "a");
    assert_eq!(name(json!({ "azp": "b", "cid": "c", "sub": "b" })).unwrap(), "b");
    assert!(name(json!({ "azp": "b", "cid": "c", "sub": "c" })).is_err());
    // An empty `cid` names no client the user may be.
    assert!(name(json!({ "cid": "", "sub": "alice" })).is_err());
    // What Q-665 took as a client's own, still.
    assert_eq!(name(json!({ "sub": "shop" })).unwrap(), "shop");
    assert_eq!(name(json!({ "azp": "shop" })).unwrap(), "shop");
    assert_eq!(name(json!({ "sub": "abc", "azp": "shop", "gty": "client-credentials" })).unwrap(), "shop");
    // Entra ID's v1 tokens name the app in `appid`, after the others: a user's (its `sub` not its
    // `oid`, with scopes) is the app's and refused; the app's own (`sub` its `oid`, no scopes) is
    // taken, named by the app (FEEDBACK75.md, Q-1079's review; branch feedback75).
    let e = name(json!({ "appid": "11aa-app", "oid": "user-oid", "sub": "pairwise-sub", "scp": "user_impersonation" })).unwrap_err();
    assert!(e.contains("not client \"11aa-app\"'s own"), "{}", e);
    assert_eq!(name(json!({ "appid": "11aa-app", "oid": "sp-oid", "sub": "sp-oid", "roles": ["Reports.Read"] })).unwrap(), "11aa-app");
    assert_eq!(name(json!({ "cid": "c", "appid": "a", "sub": "c" })).unwrap(), "c");
}

const JWT: &str = "jwt-secret-0123456789abcdef0123456";

/// An HS256 token for `claims`, for audience billing from issuer idp.
fn token(claims: serde_json::Value) -> String {
    let mut c = claims;
    c["iss"] = "idp".into();
    c["aud"] = "billing".into();
    c["exp"] = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 300).into();
    let m = format!("{}.{}", quillo::crypto::b64url(br#"{"alg":"HS256","typ":"JWT"}"#), quillo::crypto::b64url(c.to_string().as_bytes()));
    format!("{}.{}", m, quillo::crypto::b64url(&quillo::crypto::hmac_sha256(JWT.as_bytes(), m.as_bytes())))
}

/// The report's call: a service whose functions take any caller's own token (no `for`, the
/// guide's default) refuses an Okta user's with 401; the Okta client's own token is taken, named
/// by `cid`, `for ["shop"]` too (FEEDBACK75.md, Q-1079; branch feedback75).
#[test]
fn q1079_a_service_refuses_an_okta_users_token() {
    let d = dir(
        "q1079-svc",
        "accept_calls { jwt: { secret: env(\"JWT_KEY\"), audience: \"billing\", issuer: \"idp\" }, shared: false }\nserver fn whoami() = request.caller.name\nserver fn shop_only() for [\"shop\"] = \"shop's\"\n",
    );
    checked(&d);
    std::fs::copy(d.join("main.qwe"), d.join("billing.qwe")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg("billing.qwe")
        .args(["--port", "0", "--as-service"])
        .current_dir(&*d)
        .env_remove("QUILLO_SERVICE_SECRET")
        .env("JWT_KEY", JWT)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let port = support::port_in_stderr(&mut g.0, "billing.qwe", Instant::now() + Duration::from_secs(20));
    let call = |f: &str, id: &str, claims: serde_json::Value| {
        let auth = format!("Bearer {}", token(claims));
        support::send(port, "POST", &format!("/_quillo/service/{}", f), &[("x-quillo-to", "billing"), ("x-quillo-call", id), ("authorization", &auth)], "[]")
    };
    let user = serde_json::json!({ "cid": "shop", "uid": "00u1", "sub": "alice@example.com", "scp": ["billing"] });
    let r = call("whoami", "q1079a", user.clone());
    assert_eq!(r.status, 401, "{}", r.body);
    assert!(r.body.contains("one a user signed in for"), "{}", r.body);
    assert_eq!(call("shop_only", "q1079b", user).status, 401);
    let own = serde_json::json!({ "cid": "shop", "sub": "shop", "scp": ["billing"] });
    assert_eq!(call("whoami", "q1079c", own.clone()).body, r#"{"ok":"shop"}"#);
    assert_eq!(call("shop_only", "q1079d", own).body, r#"{"ok":"shop's"}"#);
}

// ---- Q-1059, Q-1083: translations in their HTML form -----------------------------------------

const TRANSLATED: &str = r#"translations({ en: { flash: { saved: "<b>Saved</b>", deleted: "Deleted {name}" }, by: "Posted by <a href=\"{site}\">{name}</a>", see: "See {site}", book: "<i>{title}</i>", two: "<a title='a>b' href=' {a}{b}'>{a}</a>", img: "<img srcset=\"/a.png 1x, {u} 2x\" alt=\"{u}\">", tail: "<a href=\"/go/{u}\">x</a>", files: { one: "<a href={u}>{count} file</a>", other: "<a href={u}>{count} files</a>" }, sch: "<a href=\"{scheme}:{rest}\">x</a>", url: "<a href=\"{proto}://{host}/{path}\">x</a>", unq: "<a href={site} title=t>{site}</a>", cls: "<b class={c}>{c}</b>", slash: "<a/href=\"{a}\">x</a>", tab: "<a href=\"&#9;{a}\">x</a>", colon: "<a href=\"javascript&colon;{a}\">x</a>", ping: "<a href=\"/x\" ping=\"/p {u}\">x</a>", odd: "<a href=\"{u}{nope}\">x</a>", amp: "<a href=\"/a?x=1&y={u}\">x</a>" } })
route GET /flash { query: { msg: str } } = <div class="flash">{raw(t(`flash.${msg}`))}</div>
route GET /plain { query: { msg: str } } = <div class="flash">{t(`flash.${msg}`)}</div>
route GET /named { query: { msg: str, name: str } } = <p>{raw(t(`flash.${msg}`, { name: name }))}</p>
route GET /by { query: { site: str, name: str } } = <p>{raw(t("by", { site: site, name: name }))}</p>
route GET /see { query: { site: str } } = <p>{t("see", { site: site })}</p>
route GET /book { query: { title: str } } = <p>{raw(t("book", { title: title }))}</p>
route GET /two { query: { a: str, b: str } } = <p>{raw(t("two", { a: a, b: b }))}</p>
route GET /img { query: { u: str } } = <p>{raw(t("img", { u: u }))}</p>
route GET /tail { query: { u: str } } = <p>{raw(t("tail", { u: u }))}</p>
route GET /files { query: { u: str, n: int } } = <p>{raw(t("files", { u: u, count: n }))}</p>
route GET /sch { query: { scheme: str, rest: str } } = <p>{raw(t("sch", { scheme: scheme, rest: rest }))}</p>
route GET /url { query: { proto: str, host: str, path: str } } = <p>{raw(t("url", { proto: proto, host: host, path: path }))}</p>
route GET /unq { query: { site: str } } = <p>{raw(t("unq", { site: site }))}</p>
route GET /cls { query: { c: str } } = <p>{raw(t("cls", { c: c }))}</p>
route GET /slash { query: { a: str } } = <p>{raw(t("slash", { a: a }))}</p>
route GET /tab { query: { a: str } } = <p>{raw(t("tab", { a: a }))}</p>
route GET /colon { query: { a: str } } = <p>{raw(t("colon", { a: a }))}</p>
route GET /ping { query: { u: str } } = <p>{raw(t("ping", { u: u }))}</p>
route GET /odd { query: { u: str } } = <p>{raw(t("odd", { u: u }))}</p>
route GET /amp { query: { u: str } } = <p>{raw(t("amp", { u: u }))}</p>
"#;

fn query(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// The report's links: a key missing from every locale, from the request, is written by
/// `raw(t(...))` as text, escaped, as `t` writes it (`<img onerror>`, `</div><script>`, quotes,
/// unicode, empty); a key there is the translation's markup, as before, and its `{name}` values
/// escaped (FEEDBACK75.md, Q-1059; branch feedback75).
#[test]
fn q1059_a_missing_key_in_the_html_form_is_escaped_text() {
    let d = dir("q1059", TRANSLATED);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str| {
        let r = support::send(s.port, "GET", path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    assert_eq!(get("/flash?msg=saved"), "<div class=\"flash\"><b>Saved</b></div>");
    assert_eq!(get("/named?msg=deleted&name=%3Cb%3EAda%3C/b%3E"), "<p>Deleted &lt;b&gt;Ada&lt;/b&gt;</p>");
    for msg in ["<img src=x onerror=alert(document.domain)>", "x</div><script>alert(1)</script>", "\"'&<>", "ü<é 🚀", "", "a"] {
        let raw = get(&format!("/flash?msg={}", query(msg)));
        assert_eq!(raw, get(&format!("/plain?msg={}", query(msg))), "{:?}", msg);
        assert!(!raw.contains("<img") && !raw.contains("<script") && raw.starts_with("<div class=\"flash\">flash."), "{:?}: {}", msg, raw);
    }
    assert_eq!(get(&format!("/flash?msg={}", query("<img src=x onerror=alert(1)>"))), "<div class=\"flash\">flash.&lt;img src=x onerror=alert(1)&gt;</div>");
    // A missing key's `{name}` is put in, escaped, as it was.
    assert_eq!(get(&format!("/named?msg={}&name={}", query("<i>{name}"), query("<b>"))), "<p>flash.&lt;i&gt;&lt;b&gt;</p>");
}

/// A key missing from the table, from the request, with `=` and `{` in it (as a form field makes
/// it), is neither read for attributes nor kept: 4 MB of distinct keys took 1.5 GB through that
/// cache. A table text with an attribute that holds a value is read once and kept once, however
/// often it is used (FEEDBACK75.md, Q-1083's second review; branch feedback75).
#[test]
fn q1083_a_missing_key_from_the_request_fills_no_cache() {
    use quillo::server::HttpRequest;
    let mut it = quillo::Interp::new(&std::env::temp_dir());
    it.load_source(&format!("{}\nroute GET /link = <p>{{raw(t(\"by\", {{ site: \"/u\", name: \"Ada\" }}))}}</p>\n", TRANSLATED), "app.qwe").unwrap();
    let get = |it: &mut quillo::Interp, url: &str| {
        let r = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: vec![("Host".into(), "x".into())], body: vec![], remote: "127.0.0.1".into() });
        (r.status, String::from_utf8_lossy(&r.body).into_owned())
    };
    let before = quillo::i18n::attr_spans_kept();
    for i in 0..300 {
        let msg = format!("{}<a href={{x}} title={}>{}", i, "=".repeat(64), "k".repeat(4000));
        let (status, body) = get(&mut it, &format!("/flash?msg={}", query(&msg)));
        assert_eq!(status, 200);
        assert!(body.starts_with("<div class=\"flash\">flash.") && !body.contains("<a"), "{}", &body[..80]);
    }
    assert_eq!(quillo::i18n::attr_spans_kept(), before, "missing keys kept");
    for _ in 0..50 {
        assert_eq!(get(&mut it, "/link").1, "<p>Posted by <a href=\"/u\">Ada</a></p>");
    }
    let by = "Posted by <a href=\"{site}\">{name}</a>".len();
    assert_eq!(quillo::i18n::attr_spans_kept(), before + by, "the table's text, once");
}

/// An attribute of a translation's HTML form that takes a URL and holds a value
/// (`href="{site}"`) is `about:blank#blocked` where what it says, the values put in, is a
/// `javascript:` or `vbscript:` URL as browsers read one (any case, spaces and control characters
/// before it, a tab inside), as markup makes it; other links, paths, text that only names the word
/// and empty are as they were. The scheme may be written in the translation (`{scheme}:{rest}`,
/// `{proto}://...`, `javascript&colon;{a}`), with a character reference before it (`&#9;`), the
/// attribute after a `/` (`<a/href=...>`), in a `srcset` or `ping` list, in a plural form. Only
/// there: the same value as text or in another attribute is shown as written ("JavaScript: The
/// Good Parts" was `about:blank#blocked`). An attribute written without quotes that holds a value
/// is quoted, so that a value with spaces cannot add attributes (`x onfocus=alert(1)`). The text
/// form (`t` alone) is text, unchanged (FEEDBACK75.md, Q-1083 and its reviews; branch
/// feedback75).
#[test]
fn q1083_a_script_url_in_a_translations_html_form_is_blocked() {
    let d = dir("q1083", TRANSLATED);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let by = |site: &str, name: &str| {
        let r = support::send(s.port, "GET", &format!("/by?site={}&name={}", query(site), query(name)), &[], "");
        assert_eq!(r.status, 200, "{}", r.body);
        r.body
    };
    assert_eq!(
        by("javascript:alert(document.cookie)", "<b>Ada</b>"),
        "<p>Posted by <a href=\"about:blank#blocked\">&lt;b&gt;Ada&lt;/b&gt;</a></p>"
    );
    for site in ["JAVASCRIPT:alert(1)", " javascript:x", "\u{1}javascript:x", "java\tscript:x", "jav\nascript:x", "vbscript:msgbox(1)", "VbScript:x"] {
        assert_eq!(by(site, "Ada"), "<p>Posted by <a href=\"about:blank#blocked\">Ada</a></p>", "{:?}", site);
    }
    for (site, shown) in [
        ("https://example.com/ü?a=1&b=2", "https://example.com/ü?a=1&amp;b=2"),
        ("/users/7", "/users/7"),
        ("#top", "#top"),
        ("javascript", "javascript"),
        ("see javascript: later", "see javascript: later"),
        ("", ""),
    ] {
        assert_eq!(by(site, "Ada"), format!("<p>Posted by <a href=\"{}\">Ada</a></p>", shown), "{:?}", site);
    }
    // Only where a URL starts: the same value as text is shown as written.
    assert_eq!(by("/u", "javascript:x"), "<p>Posted by <a href=\"/u\">javascript:x</a></p>");
    let get = |path: &str| support::send(s.port, "GET", path, &[], "").body;
    assert_eq!(get(&format!("/book?title={}", query("JavaScript: The Good Parts"))), "<p><i>JavaScript: The Good Parts</i></p>");
    assert_eq!(get(&format!("/book?title={}", query("javascript:alert(1)"))), "<p><i>javascript:alert(1)</i></p>");
    // Two values: what they make together; the title (another attribute, with a `>` in it) and
    // the text are as written.
    assert_eq!(get(&format!("/two?a={}&b={}", query("java"), query("script:alert(1)"))), "<p><a title='a>b' href='about:blank#blocked'>java</a></p>");
    assert_eq!(get(&format!("/two?a={}&b={}", query("javascript:x"), query("/y"))), "<p><a title='a>b' href='about:blank#blocked'>javascript:x</a></p>");
    assert_eq!(get(&format!("/two?a={}&b={}", query("/a"), query("/b"))), "<p><a title='a>b' href=' /a/b'>/a</a></p>");
    // The scheme written in the translation, around the values.
    for (scheme, rest, blocked) in [("javascript", "alert(1)", true), ("JavaScript", "x", true), ("vbscript", "x", true), ("https", "//ok.example/p?a=1&b=2", false), ("mailto", "ada@example.com", false)] {
        let want = if blocked { "about:blank#blocked".to_string() } else { format!("{}:{}", scheme, rest).replace('&', "&amp;") };
        assert_eq!(get(&format!("/sch?scheme={}&rest={}", query(scheme), query(rest))), format!("<p><a href=\"{}\">x</a></p>", want), "{}", scheme);
    }
    assert_eq!(get(&format!("/url?proto={}&host=h&path=p", query("javascript"))), "<p><a href=\"javascript://h/p\">x</a></p>".replace("javascript://h/p", "about:blank#blocked"));
    assert_eq!(get("/url?proto=https&host=h&path=p"), "<p><a href=\"https://h/p\">x</a></p>");
    // Unquoted: quoted, so that a value cannot end it and add attributes; as a URL, held too.
    assert_eq!(get(&format!("/unq?site={}", query("x autofocus onfocus=alert(1)"))), "<p><a href=\"x autofocus onfocus=alert(1)\" title=t>x autofocus onfocus=alert(1)</a></p>");
    assert_eq!(get(&format!("/unq?site={}", query("javascript:alert(1)"))), "<p><a href=\"about:blank#blocked\" title=t>javascript:alert(1)</a></p>");
    assert_eq!(get(&format!("/unq?site={}", query("/u\"><b>"))), "<p><a href=\"/u&quot;&gt;&lt;b&gt;\" title=t>/u&quot;&gt;&lt;b&gt;</a></p>");
    assert_eq!(get(&format!("/cls?c={}", query("a b onclick=alert(1)"))), "<p><b class=\"a b onclick=alert(1)\">a b onclick=alert(1)</b></p>");
    // `<a/href=...>`, a character reference before the value, the scheme's colon written as one.
    assert_eq!(get(&format!("/slash?a={}", query("javascript:x"))), "<p><a/href=\"about:blank#blocked\">x</a></p>");
    assert_eq!(get(&format!("/tab?a={}", query("javascript:x"))), "<p><a href=\"about:blank#blocked\">x</a></p>");
    assert_eq!(get(&format!("/tab?a={}", query("/ok"))), "<p><a href=\"&#9;/ok\">x</a></p>");
    assert_eq!(get(&format!("/colon?a={}", query("alert(1)"))), "<p><a href=\"about:blank#blocked\">x</a></p>");
    // A `srcset` image or a `ping` URL that would run script holds the whole list; `alt` is text.
    assert_eq!(get(&format!("/img?u={}", query("javascript:x"))), "<p><img srcset=\"about:blank#blocked\" alt=\"javascript:x\"></p>");
    assert_eq!(get(&format!("/img?u={}", query("/b.png"))), "<p><img srcset=\"/a.png 1x, /b.png 2x\" alt=\"/b.png\"></p>");
    assert_eq!(get(&format!("/ping?u={}", query("javascript:x"))), "<p><a href=\"/x\" ping=\"about:blank#blocked\">x</a></p>");
    assert_eq!(get(&format!("/ping?u={}", query("/q"))), "<p><a href=\"/x\" ping=\"/p /q\">x</a></p>");
    // After a URL's start, a value cannot make it a script URL; a placeholder with no value is
    // text, as written.
    assert_eq!(get(&format!("/tail?u={}", query("javascript:x"))), "<p><a href=\"/go/javascript:x\">x</a></p>");
    assert_eq!(get(&format!("/odd?u={}", query("javascript:x"))), "<p><a href=\"about:blank#blocked\">x</a></p>");
    assert_eq!(get(&format!("/odd?u={}", query("/a"))), "<p><a href=\"/a{nope}\">x</a></p>");
    assert_eq!(get(&format!("/amp?u={}", query("javascript:x"))), "<p><a href=\"/a?x=1&y=javascript:x\">x</a></p>");
    // A plural form's link, unquoted: quoted, and held.
    assert_eq!(get(&format!("/files?u={}&n=1", query("javascript:x"))), "<p><a href=\"about:blank#blocked\">1 file</a></p>");
    assert_eq!(get(&format!("/files?u={}&n=2", query("/f"))), "<p><a href=\"/f\">2 files</a></p>");
    // The text form is text: shown as it is, escaped.
    let r = support::send(s.port, "GET", &format!("/see?site={}", query("javascript:alert(1)")), &[], "");
    assert_eq!(r.body, "<p>See javascript:alert(1)</p>");
}

// ---- Chrome ------------------------------------------------------------------------------------

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// One Chrome at a time in this file.
fn chrome_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// `url` in headless Chrome (tests/browser/run.js) with `script` run in the page, `@STAGE@` the
/// stage server's address: what the page stored in `data-result`.
fn in_chrome(chrome: &Path, url: &str, script: &str, stages: u16) -> serde_json::Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let script = script.replace("@STAGE@", &format!("http://127.0.0.1:{}", stages));
    let d = Dir(support::temp_dir(&format!("f75e-chrome-{}", script.len())));
    std::fs::write(d.join("t.js"), &script).unwrap();
    let mut c = Command::new("node");
    c.arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("60000").arg(d.join("t.js"));
    let (status, out, err) = within(c, 100);
    assert!(status.success(), "{}", err);
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{}: {}", e, out))
}

/// The stage server: where a page says how far it got, and the other site a sign-out goes to.
const STAGES: &str = "let stage = 0\nroute GET /stage/:n(int) {\n    stage = n\n    \"ok\"\n}\nroute GET /stage = stage\nroute GET /bye = \"bye\"\n";

/// Waits until the page says it reached `n` (or finished, which fails the test with its answer).
fn stage_reached(stages: u16, n: u32, page: &std::thread::JoinHandle<serde_json::Value>) {
    let end = Instant::now() + Duration::from_secs(60);
    while support::send(stages, "GET", "/stage", &[], "").body != n.to_string() {
        assert!(Instant::now() < end && !page.is_finished(), "the page did not reach stage {}", n);
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The browser's HTML form of `t` says what the server's says, case for case: each translation of
/// `TRANSLATED` with markup, each of its values in turn one of a set that would end a URL's scheme,
/// make one, end an attribute, or be plain (unicode, empty, braces), as Chrome reads both (the
/// server's page parsed, the component's list as the runtime writes it) (FEEDBACK75.md, Q-1083
/// and its reviews; branch feedback75).
#[test]
fn q1083_the_browser_and_the_server_write_every_case_alike() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let values = ["javascript:alert(1)", "JaVaScRiPt:x", " vbscript:x", "java", "script:alert(1)", "alert(1)", "https://ok.example/p?a=1&b=2", "/path", "x autofocus onfocus=alert(1)", "\"><b>", "&amp;", "", "ü 🚀", "{x}"];
    let keys: [(&str, &[&str]); 18] = [
        ("by", &["site", "name"]),
        ("book", &["title"]),
        ("two", &["a", "b"]),
        ("img", &["u"]),
        ("tail", &["u"]),
        ("files", &["u"]),
        ("sch", &["scheme", "rest"]),
        ("url", &["proto", "host", "path"]),
        ("unq", &["site"]),
        ("cls", &["c"]),
        ("slash", &["a"]),
        ("tab", &["a"]),
        ("colon", &["a"]),
        ("ping", &["u"]),
        ("see", &["site"]),
        ("flash.deleted", &["name"]),
        ("odd", &["u"]),
        ("amp", &["u"]),
    ];
    let mut cases = Vec::new();
    for (k, params) in keys {
        for p in params {
            for v in values {
                let mut vars = serde_json::Map::new();
                for q in params {
                    vars.insert(q.to_string(), serde_json::Value::String(if q == p { v.to_string() } else { "d".into() }));
                }
                if k == "files" {
                    vars.insert("count".into(), serde_json::json!(1 + cases.len() % 2));
                }
                cases.push(serde_json::json!({ "k": k, "v": vars }));
            }
        }
    }
    let n = cases.len();
    let program = TRANSLATED.lines().next().unwrap().to_string()
        + "\nlet cases = json.parse(read_file(\"cases.json\"))\n"
        + "component Cases(cases: list) {\n    state shown = false\n    <div>\n        <button id=\"go\" onclick={() => { shown = true }}>go</button>\n        <div id=\"b\">{shown ? (for c in cases => <p class=\"c\">{raw(t(c.k, c.v))}<i hidden>{t(c.k, c.v)}</i></p>) : null}</div>\n    </div>\n}\n"
        + "route GET / = <html><body><Cases cases={cases}/></body></html>\n"
        + "route GET /srv = <html><body>{for c in cases => <p class=\"c\">{raw(t(c.k, c.v))}</p>}</body></html>\n";
    let d = dir("q1083-alike", &program);
    std::fs::write(d.join("cases.json"), serde_json::Value::Array(cases).to_string()).unwrap();
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let script = r##"(async () => {
  const out = {}, tick = (ms) => new Promise((r) => setTimeout(r, ms)), $ = (q) => document.querySelector(q);
  try {
    for (let i = 0; i < 200 && !$("#go"); i++) await tick(30);
    const srv = new DOMParser().parseFromString(await (await fetch("/srv")).text(), "text/html");
    const want = [...srv.querySelectorAll("p.c")].map((p) => p.innerHTML);
    $("#go").click();
    for (let i = 0; i < 200 && document.querySelectorAll("#b p.c").length < want.length; i++) await tick(30);
    const got = [...document.querySelectorAll("#b p.c")].map((p) => { const r = p.querySelector("quillo-raw"); return r ? r.innerHTML : p.firstChild ? p.innerHTML : ""; });
    out.n = got.length;
    out.server = want.length;
    out.differ = got.map((g, i) => g === want[i] ? null : { i, browser: g, server: want[i] }).filter(Boolean).slice(0, 8);
    out.blocked = want.filter((w) => w.includes("about:blank#blocked")).length;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();"##;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script, s.port);
    assert_eq!(r["differ"], serde_json::json!([]), "{}", r);
    assert_eq!((r["n"].as_u64(), r["server"].as_u64()), (Some(n as u64), Some(n as u64)), "{}", r);
    assert!(r["blocked"].as_u64().unwrap_or(0) > 20 && r["errors"] == serde_json::json!([]), "{}", r);
}

/// A component in the browser writes `raw(t(...))` as the server does: a missing key as text (no
/// `<img>` or `<script>` made from it), a `javascript:` URL in the translation's `href` as
/// `about:blank#blocked`, other links and the text form as they were; first as the server wrote
/// the page, then as the browser writes it again (FEEDBACK75.md, Q-1059 and Q-1083; branch
/// feedback75).
#[test]
fn q1059_q1083_the_browser_writes_the_html_form_as_the_server_does() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let src = r#"translations({ en: { flash: { saved: "<b>Saved</b>" }, by: "Posted by <a href=\"{site}\">{name}</a>", book: "<i>{title}</i>", two: "<a href='{a}{b}'>{b}</a>", img: "<img srcset=\"/a.png 1x, {u} 2x\" alt=\"Cover\">" } })
let first_msgs = ["saved", "<img src=x id=bad0>"]
let first_sites = ["javascript:alert(0)", "https://example.com/"]
component Flash() {
    state msgs = first_msgs
    state sites = first_sites
    state title = "JavaScript: The Good Parts"
    state tail = "/x"
    state pic = "/b.png"
    <div>
        <div id="f">{for m in msgs => <p>{raw(t(`flash.${m}`))}</p>}</div>
        <div id="p">{for m in msgs => <p>{t(`flash.${m}`)}</p>}</div>
        <div id="b">{for s in sites => <p>{raw(t("by", { site: s, name: "<i>Ada</i>" }))}</p>}</div>
        <span id="k" hidden>{t("by", { site: "javascript:x", name: "Ada" })}</span>
        <span id="bk">{raw(t("book", { title: title }))}</span>
        <span id="tw">{raw(t("two", { a: "java", b: tail }))}</span>
        <span id="im">{raw(t("img", { u: pic }))}</span>
        <span id="keys" hidden>{t("book", { title: "" })}{t("two", { a: "", b: "" })}{t("img", { u: "" })}</span>
        <button id="go" onclick={() => {
            msgs = ["saved", "<img src=x id=bad1>", "x</p><script>document.body.dataset.bad = 1</script>", "ü<é\"'&", ""]
            sites = ["javascript:alert(1)", "JAVASCRIPT:alert(1)", " javascript:x", "java\tscript:x", "vbscript:x", "https://example.com/ü?a=1&b=2", "javascript", ""]
            title = "javascript:alert(2)"
            tail = "script:alert(3)"
            pic = "javascript:alert(4)"
        }}>go</button>
    </div>
}
route GET / = <html><body><Flash/></body></html>
"#;
    // The text form of "by" beside the HTML form ships the key to the browser: `raw(t("by"))` alone
    // does not (src/client.rs writes `B.t_html` without noting its key; found here, reported).
    let d = dir("q1059-browser", src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let script = r##"(async () => {
  const out = {}, tick = (ms) => new Promise((r) => setTimeout(r, ms)), $ = (q) => document.querySelector(q);
  const read = () => ({
    flash: [...$("#f").children].map((p) => p.textContent),
    bold: document.querySelectorAll("#f b").length,
    plain: [...$("#p").children].map((p) => p.textContent),
    links: [...$("#b").querySelectorAll("a")].map((a) => a.getAttribute("href")),
    names: [...$("#b").querySelectorAll("a")].map((a) => a.textContent),
    made: document.querySelectorAll("#f img, #f script, #b i").length,
    text: $("#k").textContent,
    book: $("#bk i").textContent,
    two: [$("#tw a").getAttribute("href"), $("#tw a").textContent],
    img: [$("#im img").getAttribute("srcset"), $("#im img").getAttribute("alt")],
  });
  try {
    for (let i = 0; i < 200 && !$("#go"); i++) await tick(30);
    await tick(300);
    out.server = read();
    $("#go").click();
    for (let i = 0; i < 100 && $("#f").children.length < 5; i++) await tick(30);
    out.browser = read();
    out.bad = document.body.dataset.bad || null;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();"##;
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), script, s.port);
    let missing = ["flash.<img src=x id=bad1>", "flash.x</p><script>document.body.dataset.bad = 1</script>", "flash.ü<é\"'&", "flash."];
    let mut flash = vec!["Saved"];
    flash.extend(missing);
    let mut plain = vec!["<b>Saved</b>"];
    plain.extend(missing);
    assert_eq!(
        r,
        serde_json::json!({
            "server": {
                "flash": ["Saved", "flash.<img src=x id=bad0>"],
                "bold": 1,
                "plain": ["<b>Saved</b>", "flash.<img src=x id=bad0>"],
                "links": ["about:blank#blocked", "https://example.com/"],
                "names": ["<i>Ada</i>", "<i>Ada</i>"],
                "made": 0,
                "text": "Posted by <a href=\"javascript:x\">Ada</a>",
                "book": "JavaScript: The Good Parts",
                "two": ["java/x", "/x"],
                "img": ["/a.png 1x, /b.png 2x", "Cover"],
            },
            "browser": {
                "flash": flash,
                "bold": 1,
                "plain": plain,
                "links": ["about:blank#blocked", "about:blank#blocked", "about:blank#blocked", "about:blank#blocked", "about:blank#blocked", "https://example.com/ü?a=1&b=2", "javascript", ""],
                "names": vec!["<i>Ada</i>"; 8],
                "made": 0,
                "text": "Posted by <a href=\"javascript:x\">Ada</a>",
                "book": "javascript:alert(2)",
                "two": ["about:blank#blocked", "script:alert(3)"],
                "img": ["about:blank#blocked", "Cover"],
            },
            "bad": null,
            "errors": [],
        })
    );
}

// ---- Q-1060: PostgreSQL's shared connection -------------------------------------------------

/// Read as Quillo reads SQL: `update pg_settings` (as `pg_catalog.pg_settings`, `only`, quoted,
/// in any case) changes the session, as `set` does; reading it, or updating another table by it,
/// does not. PostgreSQL's functions that run SQL from text or read tables and views by name run
/// isolated (`unseen`), however written; a function that only shares the start of a name's word,
/// a plain query, and text that names one are as before (FEEDBACK75.md, Q-1060; branch
/// feedback75).
#[test]
fn q1060_update_pg_settings_lasts_and_functions_that_run_sql_run_isolated() {
    use quillo::sql::{scan, Dialect};
    let lasts = |sql: &str| scan(Dialect::Postgres, sql).changes_session(&[]);
    for sql in [
        "update pg_settings set setting = '1' where name = 'statement_timeout'",
        "UPDATE PG_SETTINGS SET setting = 'x' WHERE name = 'search_path'",
        "update pg_catalog.pg_settings set setting = '1' where name = 'statement_timeout'",
        "update only \"pg_settings\" set setting = '1' where name = 'work_mem'",
        "update pg_settings as s set setting = '1' where s.name = 'work_mem'",
        "with x as (update pg_settings set setting = '1' where name = 'work_mem' returning name) select * from x",
        "select 1; update pg_settings set setting = '1' where name = 'work_mem'",
        "do $$ begin update pg_settings set setting = '1' where name = 'work_mem'; end $$",
    ] {
        assert!(lasts(sql), "{}", sql);
    }
    for sql in [
        "select * from pg_settings where name = 'work_mem'",
        "update t set x = 1 where name in (select name from pg_settings)",
        "update t set pg_settings = 1",
        "select 'update pg_settings set setting = 1'",
        "",
    ] {
        assert!(!lasts(sql), "{}", sql);
    }
    let unseen = |sql: &str| scan(Dialect::Postgres, sql).unseen;
    for sql in [
        "select query_to_xml('select set_config(''app.user_id'', ''alice'', false)', true, false, '')",
        "select pg_catalog.query_to_xml_and_xmlschema('select 1', true, false, '')",
        "select QUERY_TO_XMLSCHEMA(?, true, false, '')",
        "select cursor_to_xml('c', 10, false, true, '')",
        "select table_to_xml('setter', true, false, '')",
        "select schema_to_xml('public', true, false, '')",
        "select database_to_xml(true, false, '')",
        "select * from ts_stat('select to_tsvector(body) from docs')",
        "select \"query_to_xml\"('select 1', true, false, '')",
        "select ts_rewrite('a & b'::tsquery, 'select t, s from aliases')",
    ] {
        assert!(unseen(sql), "{}", sql);
        // Isolated, not refused: the program's own use of them runs.
        assert!(!lasts(sql), "{}", sql);
    }
    for sql in ["select query_to_text('x')", "select xml_to_query from t", "select 'query_to_xml(1)'", "select count(*) from t", "select ts_rank(v, q) from t"] {
        assert!(!unseen(sql), "{}", sql);
    }
    // MySQL and SQLite have no such functions: a name there is the program's.
    assert!(!scan(Dialect::Mysql, "select query_to_xml('x')").unseen);
    assert!(!scan(Dialect::Sqlite, "update pg_settings set setting = 1").changes_session(&[]));
}

/// psql as the server's superuser, on database `db`.
fn psql(pg: &support::Pg, url: &str, sql: &str) {
    let out = Command::new(pg.bin.join("psql")).args([url, "-q", "-v", "ON_ERROR_STOP=1", "-c", sql]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The report's PostgreSQL: rows of Alice's behind the row-level security the guide recommends,
/// read by a plain role on the connection requests share. `query_to_xml`, `table_to_xml` of a
/// view that sets the session, `ts_stat` and `ts_rewrite` set `app.user_id` for good on that
/// connection, and
/// later requests read Alice's rows: they run isolated now, their answers as before and nothing
/// left behind, on 8 requests after each. `update pg_settings` is refused outside a transaction,
/// as `set` is; inside one it holds there and is reset after (FEEDBACK75.md, Q-1060; branch
/// feedback75).
#[test]
fn q1060_query_to_xml_and_update_pg_settings_leave_nothing_on_the_shared_connection() {
    let Some(pg) = support::postgres("f75e-q1060") else {
        eprintln!("skipping: no PostgreSQL");
        return;
    };
    let url = pg.database("f75e_shop");
    psql(
        &pg,
        &url,
        "create role app login; \
         create table orders (id serial primary key, owner text, item text); \
         insert into orders (owner, item) values ('alice', 'alice-secret-ring'), ('bob', 'bob-hat'); \
         alter table orders enable row level security; \
         create policy own on orders using (owner = coalesce(nullif(current_setting('app.user_id', true), ''), '-')); \
         grant select on orders to app; \
         create view setter as select set_config('app.user_id', 'alice', false) as v; \
         grant select on setter to app",
    );
    let app_url = url.replace("postgres://quillo@", "postgres://app@");
    let src = format!(
        r#"db.open("{}")
route GET /xml = db.one("select query_to_xml('select set_config(''app.user_id'', ''alice'', false)', true, false, '')::text as v").v
route GET /xml-plain = db.one("select query_to_xml('select 1 as a, ''ü'' as b', true, false, '')::text as v").v
route GET /table-xml = db.one("select table_to_xml('setter', true, false, '')::text as v").v
route GET /ts = db.query("select word from ts_stat('select to_tsvector(set_config(''app.user_id'', ''alice'', false))')")
route GET /tsr = db.one("select ts_rewrite('a'::tsquery, 'select set_config(''app.user_id'', ''alice'', false)::tsquery, ''b''::tsquery')::text as v").v
route GET /orders = db.query("select item from orders")
route GET /who = db.one("select coalesce(current_setting('app.user_id', true), '') as u").u
route GET /tmo {{
    let r = db.exec("update pg_settings set setting = '1' where name = 'statement_timeout'")
    return is_err(r) ? r.error : "ran"
}}
route GET /tmo-in = db.transaction(() => {{
    db.exec("update pg_settings set setting = '7s' where name = 'statement_timeout'")
    return db.one("select current_setting('statement_timeout') as t").t
}})
route GET /timeout = db.one("select current_setting('statement_timeout') as t").t
route GET /settings = db.one("select count(*) as n from pg_settings where name = 'statement_timeout'").n
"#,
        app_url
    );
    let d = dir("q1060", &src);
    checked(&d);
    let s = support::serve(&d.join("main.qwe"), &[]);
    let get = |path: &str| {
        let r = support::send(s.port, "GET", path, &[], "");
        assert_eq!(r.status, 200, "{}: {}", path, r.body);
        r.body
    };
    let nothing_left = |after: &str| {
        // Eight requests, over every connection the requests share.
        let all: Vec<(String, String)> = (0..8).map(|_| (get("/orders"), get("/who"))).collect();
        assert!(all.iter().all(|(o, w)| o == "[]" && w.is_empty()), "after {}: {:?}", after, all);
    };
    nothing_left("nothing");
    assert!(get("/xml").contains("alice"));
    nothing_left("/xml");
    let plain = get("/xml-plain");
    assert!(plain.contains("<a>1</a>") && plain.contains("<b>ü</b>"), "{}", plain);
    assert!(get("/table-xml").contains("<v>alice</v>"));
    nothing_left("/table-xml");
    assert!(get("/ts").contains("alic"));
    nothing_left("/ts");
    assert_eq!(get("/tsr"), "'a'");
    nothing_left("/tsr");
    // `update pg_settings`: refused outside a transaction, saying what to do; held inside one.
    assert!(get("/tmo").contains("outside `db.transaction` this connection is shared by every request"));
    assert_eq!(get("/timeout"), "0");
    assert_eq!(get("/tmo-in"), "7s");
    for _ in 0..8 {
        assert_eq!(get("/timeout"), "0");
    }
    assert_eq!(get("/settings"), "1");
}

// ---- Q-1078, Q-1082: the offline worker ---------------------------------------------------------

/// The worker's own `gated`, taken from the served text, run in Node on policies.
const GATED: &str = r#"const src = require("fs").readFileSync(process.argv[2], "utf8");
const GATE_HASH = /GATE_HASH = ("[^"]*")/.exec(src)[1];
const body = src.slice(src.indexOf("function start(html)"), src.indexOf("async function state()"));
const gated = new Function("const GATE_HASH = " + GATE_HASH + ";\n" + body + "; return gated;")();
const policies = JSON.parse(process.argv[3]);
console.log(JSON.stringify(policies.map((p) => gated(new Headers(p === null ? {} : { "content-security-policy": p })))));
"#;

/// Whether the gate can run under a kept page's policy: a `sandbox` needs `allow-same-origin` as
/// well as `allow-scripts` (the report's `sandbox allow-scripts allow-forms` was given back, and
/// the gate failed there), in any case, in each of two policies; no policy, or none that sandboxes
/// or decides scripts, as before (FEEDBACK75.md, Q-1078; branch feedback75).
#[test]
fn q1078_a_sandbox_without_allow_same_origin_stops_the_gate() {
    let d = dir("q1078-gated", "route offline GET / = <html><body>x</body></html>\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let worker = support::send(s.port, "GET", "/_quillo/sw.js", &[], "").body;
    assert!(worker.contains("GATE_HASH = "), "{}", worker.len());
    std::fs::write(d.join("sw.js"), worker).unwrap();
    std::fs::write(d.join("gated.js"), GATED).unwrap();
    let cases: Vec<(Option<&str>, bool)> = vec![
        (None, true),
        (Some("sandbox allow-scripts"), false),
        (Some("sandbox allow-scripts allow-forms"), false),
        (Some("sandbox allow-same-origin"), false),
        (Some("sandbox"), false),
        (Some("sandbox allow-scripts allow-same-origin"), true),
        (Some("SANDBOX Allow-Same-Origin ALLOW-SCRIPTS allow-forms"), true),
        (Some("sandbox allow-scripts allow-same-origin; script-src 'self'"), false),
        (Some("sandbox allow-scripts allow-same-origin; script-src 'unsafe-inline'"), true),
        (Some("sandbox allow-scripts allow-same-origin, sandbox allow-scripts"), false),
        (Some("default-src 'self', sandbox allow-scripts"), false),
        (Some("script-src 'unsafe-inline'"), true),
        (Some("style-src 'self'"), true),
    ];
    let policies = serde_json::to_string(&cases.iter().map(|c| c.0).collect::<Vec<_>>()).unwrap();
    let mut c = Command::new("node");
    c.arg(d.join("gated.js")).arg(d.join("sw.js")).arg(&policies);
    let (status, out, err) = within(c, 30);
    assert!(status.success(), "{}", err);
    let got: Vec<bool> = serde_json::from_str(out.trim()).unwrap();
    for (case, got) in cases.iter().zip(got) {
        assert_eq!(got, case.1, "{:?}", case.0);
    }
}

/// What the worker scripts share: frames loaded through the worker, the caches' contents, the
/// sessions dropped, and waits on the server.
const KIT: &str = r#"
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const sw = navigator.serviceWorker;
  const secure = "; Path=/; Secure; SameSite=Lax";
  const stage = (n) => fetch("@STAGE@/stage/" + n, { mode: "no-cors" });
  const frame = (path) => new Promise((ok) => {
    const f = document.createElement("iframe");
    f.onload = () => ok(f);
    f.src = path;
    document.body.appendChild(f);
  });
  // A form sent by a frame's navigation, as a sign-out button sends one.
  const post = (path) => new Promise((ok) => {
    const f = document.createElement("iframe"), form = document.createElement("form");
    f.name = "post" + Math.random().toString(36).slice(2);
    document.body.appendChild(f);
    form.method = "POST"; form.action = path; form.target = f.name;
    document.body.appendChild(form);
    f.onload = () => ok(f);
    form.submit();
  });
  // A frame's page once `ok` holds: its heading and marks, or `opaque` where the page cannot be read.
  const lookUntil = async (path, ok) => {
    const f = await frame(path);
    for (let i = 0; i < 200; i++) { try { if (f.contentDocument.readyState === "complete" && ok(f.contentDocument)) break; } catch (_) {} await tick(25); }
    const d = f.contentDocument;
    if (!d) return { opaque: true };
    const h = d.documentElement, k = h.getAttribute("data-kept");
    return { h1: (d.querySelector("h1") || {}).textContent || null, kept: k === null ? null : /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(k) ? "iso" : k, offline: h.hasAttribute("data-quillo-offline"), session: h.getAttribute("data-kept-session"), hidden: h.hasAttribute("hidden") };
  };
  const look = async (path) => (await lookUntil(path, () => true)).h1;
  // Each cache, its version as V and its session as S, with the paths it keeps.
  const snap = async () => {
    const out = {};
    for (const n of await caches.keys()) {
      if (n.endsWith(":gone") || !(await caches.has(n))) continue;
      const k = (await (await caches.open(n)).keys()).map((r) => decodeURI(new URL(r.url).pathname)).filter((p) => !p.endsWith("/_quillo/sw-state") && !/\/_quillo\//.test(p)).sort();
      if (k.length) out[n.replace(/:[0-9a-f]{16}/, ":V").replace(/:V:.+$/, ":V:S")] = k;
    }
    return out;
  };
  // What the worker keeps of a page is written after it gives the page: read once it has held
  // for 100 ms, 2 s at most.
  const kept = async () => {
    let was = JSON.stringify(await snap());
    for (let i = 0; i < 20; i++) {
      await tick(100);
      const now = JSON.stringify(await snap());
      if (now === was) break;
      was = now;
    }
    return JSON.parse(was);
  };
  // The caches once no session's is left, 5 s at most.
  const unkept = async () => {
    for (let i = 0; i < 100; i++) { const s = await snap(); if (!Object.keys(s).some((k) => k.endsWith(":S"))) return s; await tick(50); }
    return await snap();
  };
  const dropped = async () => { const r = await (await caches.open("quillo::gone")).match("/_quillo/sw-dropped"); return r ? await r.json() : []; };
  // The list once it names `tag` (it is written after the caches go), 5 s at most.
  const droppedWith = async (tag) => { for (let i = 0; i < 100; i++) { const l = await dropped(); if (l.includes(tag)) return l; await tick(50); } return await dropped(); };
  const tagNow = () => (/(?:^|;\s*)quillo_tag=([^;]*)/.exec(document.cookie) || [])[1] || "-";
  const down = async () => { for (let i = 0; i < 600; i++) { try { await fetch("/ping", { cache: "no-store" }); } catch (_) { return true; } await tick(25); } return false; };
  const controlled = async () => { await sw.ready; for (let i = 0; i < 300 && !sw.controller; i++) await tick(20); return !!sw.controller; };
  // The worker's cookie subscriptions taken away: what it reads after a redirect alone is left.
  const unsubscribe = async () => { const r = await sw.ready; if (r.cookies) await r.cookies.unsubscribe(await r.cookies.getSubscriptions()); return r.cookies ? (await r.cookies.getSubscriptions()).length : null; };
"#;

/// Under `content-security-policy: sandbox allow-scripts`, a session's kept page is not given
/// back offline: neither to the person it was kept for nor, after a sign-out the worker did not
/// see, to the next person (whom it was shown, its scripts run, the gate failing on
/// `document.cookie`). With `allow-same-origin` beside it the gate runs as before: shown to that
/// session, and for another the offline page, that session's pages dropped (FEEDBACK75.md, Q-1078;
/// branch feedback75).
#[test]
fn q1078_a_kept_page_sandboxed_without_its_origin_is_not_given_back() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let stages = support::serve(&dir("q1078-stages", STAGES).join("main.qwe"), &[]);
    let src = r#"site_headers("/mine", { "content-security-policy": "sandbox allow-scripts allow-forms" })
site_headers("/both", { "content-security-policy": "sandbox allow-scripts allow-same-origin" })
route GET /in/:who {
    set_session({ user: who })
    redirect("/")
}
route offline GET / = <html><head><title>Home</title></head><body><h1>Home</h1></body></html>
route offline GET /mine = <html><head><title>Mine</title><style>{"html { display: block }"}</style></head><body><h1>Mine {session().user ?? "nobody"}</h1></body></html>
route offline GET /both = <html><head><title>Both</title><style>{"html { display: block }"}</style></head><body><h1>Both {session().user ?? "nobody"}</h1></body></html>
route GET /ping = "pong"
"#;
    let d = dir("q1078-site", src);
    checked(&d);
    let site = support::serve(&d.join("main.qwe"), &[]);
    let script = format!(
        r#"(async () => {{{}
  const out = {{}};
  try {{
    out.controlled = await controlled();
    // The gate alone: the worker does not hear of the cookie's changes here.
    out.subscriptions = await unsubscribe();
    await look("/in/ana");
    const ana = tagNow();
    await frame("/mine");
    out.both = await look("/both");
    out.kept = await kept();
    await stage(1);
    out.down = await down();
    const offline = (d) => d.documentElement.hasAttribute("data-quillo-offline");
    out.mine_ana = await lookUntil("/mine", offline);
    out.both_ana = await lookUntil("/both", (d) => !d.documentElement.hasAttribute("hidden"));
    out.same = out.both_ana.session === ana;
    delete out.both_ana.session;
    // The next person: the cookie gone where the worker did not see it.
    document.cookie = "quillo_tag=; Max-Age=0" + secure;
    out.mine_next = await lookUntil("/mine", offline);
    out.both_next = await lookUntil("/both", offline);
    out.after = await kept();
  }} catch (e) {{ out.exception = String(e && e.stack || e); }}
  document.body.dataset.result = JSON.stringify(out);
}})();"#,
        KIT
    );
    let (c, url, sp) = (chrome.clone(), format!("http://127.0.0.1:{}/", site.port), stages.port);
    let page = std::thread::spawn(move || in_chrome(&c, &url, &script, sp));
    stage_reached(stages.port, 1, &page);
    drop(site);
    let r = page.join().unwrap();
    let offline = serde_json::json!({ "h1": "You are offline", "kept": null, "offline": true, "session": null, "hidden": false });
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "subscriptions": 0, "both": "Both ana",
            "kept": { "quillo::V": ["/"], "quillo::V:S": ["/both", "/mine"] },
            "down": true,
            "mine_ana": offline,
            "both_ana": { "h1": "Both ana", "kept": "iso", "offline": false, "hidden": false },
            "same": true,
            "mine_next": offline, "both_next": offline,
            "after": { "quillo::V": ["/"] },
        })
    );
}

/// A site whose sign-outs redirect to another site (the stage server's /bye), as SSO sign-outs do.
fn signing(stages: u16) -> String {
    format!(
        r#"route GET /in/:who {{
    set_session({{ user: who }})
    redirect("/mine")
}}
route GET /out {{
    end_session()
    redirect("http://127.0.0.1:{0}/bye")
}}
route POST /out {{
    end_session()
    redirect("http://127.0.0.1:{0}/bye")
}}
route GET /go = redirect("/mine")
route offline GET / = <html><head><title>Home</title></head><body><h1>Home</h1></body></html>
route offline GET /mine = <html><head><title>Mine</title></head><body><h1>Mine {{session().user ?? "nobody"}}</h1></body></html>
route GET /ping = "pong"
"#,
        stages
    )
}

/// A sign-out that redirects to another site, by a link (GET) or a form (POST): the worker cannot
/// read the redirect, and kept the person's pages in Cache Storage, readable on the device; it
/// reads `quillo_tag` after the redirect now and drops them (here with its cookie subscription
/// taken away, so that this alone is seen). A redirect on the site, the person still signed in,
/// keeps them (FEEDBACK75.md, Q-1082; branch feedback75).
#[test]
fn q1082_a_sign_out_that_redirects_away_drops_the_kept_pages() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let stages = support::serve(&dir("q1082-stages", STAGES).join("main.qwe"), &[]);
    let d = dir("q1082-site", &signing(stages.port));
    checked(&d);
    let site = support::serve(&d.join("main.qwe"), &[]);
    let script = format!(
        r#"(async () => {{{}
  const out = {{}};
  try {{
    out.controlled = await controlled();
    out.subscriptions = await unsubscribe();
    out.ana = await look("/in/ana");
    const ana = tagNow();
    out.kept = await kept();
    out.go = await look("/go");
    out.still = await kept();
    out.dropped_before = await dropped();
    await frame("/out");
    out.out = await unkept();
    out.dropped = (await droppedWith(ana)).map((t) => t === ana ? "ana" : t);
    out.tag = tagNow();
    out.bob = await look("/in/bob");
    const bob = tagNow();
    out.kept_bob = await kept();
    await post("/out");
    out.posted = await unkept();
    out.dropped_bob = (await droppedWith(bob)).includes(bob);
    out.tag_after = tagNow();
  }} catch (e) {{ out.exception = String(e && e.stack || e); }}
  document.body.dataset.result = JSON.stringify(out);
}})();"#,
        KIT
    );
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", site.port), &script, stages.port);
    let public = serde_json::json!({ "quillo::V": ["/"] });
    let theirs = serde_json::json!({ "quillo::V": ["/"], "quillo::V:S": ["/mine"] });
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "subscriptions": 0,
            "ana": "Mine ana", "kept": theirs, "go": "Mine ana", "still": theirs, "dropped_before": [],
            "out": public, "dropped": ["ana"], "tag": "-",
            "bob": "Mine bob", "kept_bob": theirs,
            "posted": public, "dropped_bob": true, "tag_after": "-",
        })
    );
}

/// The worker subscribes to `quillo_tag` as it starts, and drops a session's kept pages as soon
/// as the cookie names another (a value set by the page) or none (taken away), with no page of
/// the site opened; a person signed in again is kept again (FEEDBACK75.md, Q-1082; branch
/// feedback75).
#[test]
fn q1082_the_worker_follows_the_cookie_itself() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    let _turn = chrome_turn();
    let stages = support::serve(&dir("q1082c-stages", STAGES).join("main.qwe"), &[]);
    let d = dir("q1082c-site", &signing(stages.port));
    checked(&d);
    let site = support::serve(&d.join("main.qwe"), &[]);
    let script = format!(
        r#"(async () => {{{}
  const out = {{}};
  try {{
    out.controlled = await controlled();
    const reg = await sw.ready;
    out.subscribed = (await reg.cookies.getSubscriptions()).map((s) => s.name + " " + (s.url === reg.scope));
    out.carol = await look("/in/carol");
    const carol = tagNow();
    out.kept = await kept();
    document.cookie = "quillo_tag=someone" + secure;
    out.changed = await unkept();
    out.dropped = (await droppedWith(carol)).map((t) => t === carol ? "carol" : t);
    out.dave = await look("/in/dave");
    const dave = tagNow();
    out.kept_dave = await kept();
    document.cookie = "quillo_tag=; Max-Age=0" + secure;
    out.deleted = await unkept();
    out.dropped_dave = (await droppedWith(dave)).includes(dave);
    out.again = await look("/in/dave");
    out.kept_again = await kept();
  }} catch (e) {{ out.exception = String(e && e.stack || e); }}
  document.body.dataset.result = JSON.stringify(out);
}})();"#,
        KIT
    );
    let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", site.port), &script, stages.port);
    let public = serde_json::json!({ "quillo::V": ["/"] });
    let theirs = serde_json::json!({ "quillo::V": ["/"], "quillo::V:S": ["/mine"] });
    assert_eq!(
        r,
        serde_json::json!({
            "controlled": true, "subscribed": ["quillo_tag true"],
            "carol": "Mine carol", "kept": theirs,
            "changed": public, "dropped": ["carol"],
            "dave": "Mine dave", "kept_dave": theirs,
            "deleted": public, "dropped_dave": true,
            "again": "Mine dave", "kept_again": theirs,
        })
    );
}
