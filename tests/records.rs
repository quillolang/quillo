//! Records: `type User = { ... }` (src/record.rs), made, checked in
//! annotations and patterns, held to by `quillo check`, and the same in the
//! browser.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::Path;
use std::process::Command;

const TYPES: &str = r#"
type Address = { city: str, zip: str? }
type User = {
    name: str,
    email: str,
    role: "admin" | "member" = "member",
    address: Address?,
    tags: list<str> = [],
}
type Id = int | str
type Order = { id: Id, lines: list<{ sku: str, qty: int }>, buyer: User? }
type Team = { name: str, members: list<User> = [] }
type Node = { value: int, kids: list<Node> = [] }
"#;

fn run(body: &str) -> String {
    let src = format!("{}\nfn __t() {{\n{}\n}}\n", TYPES, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    }
}

#[test]
fn making_a_record_checks_it_and_fills_it_in() {
    for (e, want) in [
        // Defaults put in; fields the record does not have left out.
        (r#"User({ name: "Ada", email: "ada@x.io", admin: true })"#, r#"{ name: "Ada", email: "ada@x.io", role: "member", tags: [] }"#),
        (r#"User({ name: "Ada", email: "a", role: "admin", tags: ["x"] }).role"#, r#""admin""#),
        (r#"User({ name: "Ada" })"#, r#"err("User: `email` is missing")"#),
        (r#"User({ name: "Ada", email: 5 })"#, r#"err("User: `email` must be str, got int 5")"#),
        (r#"User({ name: "Ada", email: "a", role: "boss" })"#, r#"err("User: `role` must be \"admin\" | \"member\", got str \"boss\"")"#),
        (r#"User({ name: "Ada", email: "a", address: { zip: "1" } })"#, r#"err("User: `address.city` is missing")"#),
        (r#"User({ name: "Ada", email: "a", tags: ["x", 2] })"#, r#"err("User: `tags[1]` must be str, got int 2")"#),
        (r#"User(5)"#, r#"err("User: expects a map of its fields, got int 5")"#),
        // Records inside are made too, lists of them item by item.
        (
            r#"Team({ name: "core", members: [{ name: "Ada", email: "a" }] })"#,
            r#"{ name: "core", members: [{ name: "Ada", email: "a", role: "member", tags: [] }] }"#,
        ),
        (r#"Team({ name: "core", members: [{ name: "Ada", email: "a" }, { name: "Bo" }] })"#, r#"err("Team: `members[1].email` is missing")"#),
        (r#"Order({ id: "A1", lines: [{ sku: "x", qty: "2" }] })"#, r#"err("Order: `lines[0].qty` must be int, got str \"2\"")"#),
        // A default is made for each record: no two share a list.
        (r#"let a = User({ name: "A", email: "a" })
let b = User({ name: "B", email: "b" })
[a.tags, b.tags, a == b]"#, "[[], [], false]"),
        // A record that names itself.
        (r#"Node({ value: 1, kids: [{ value: 2, kids: [{ value: 3 }] }] })"#, "{ value: 1, kids: [{ value: 2, kids: [{ value: 3, kids: [] }] }] }"),
        (r#"Node({ value: 1, kids: [{ value: 2, kids: [{ value: "x" }] }] })"#, r#"err("Node: `kids[0].kids[0].value` must be int, got str \"x\"")"#),
        // Another name for a type.
        ("[Id(5), Id(\"x\"), Id(true)]", r#"[5, "x", err("Id: must be int | str, got bool true")]"#),
        // An error value: `?` and `??` work with it.
        (r#"User({ name: "A" }) ?? "no user""#, r#""no user""#),
        ("type_of(User({ name: \"A\", email: \"a\" }))", r#""map""#),
    ] {
        assert_eq!(run(e), want, "{}", e);
    }
}

#[test]
fn annotations_and_patterns_check_records() {
    let defs = "fn greet(u: User) -> str = `Hi ${u.name}`\nfn first(t: Team) -> User? = t.members[0]\nfn bad() -> User = { name: \"x\" }\n";
    let call = |body: &str| {
        let s = format!("{}\n{}\nfn __t() {{\n{}\n}}\n", TYPES, defs, body);
        let mut it = Interp::new(".");
        it.load_source(&s, "case.qwe").unwrap();
        match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
            Ok(v) => v.repr(),
            Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
            Err(_) => "returned".into(),
        }
    };
    assert_eq!(call(r#"greet(User({ name: "Ada", email: "a" }))"#), r#""Hi Ada""#);
    assert_eq!(
        call(r#"greet({ name: "Ada", email: "a" })"#),
        r#"ERROR: argument `u` of `greet` must be User, got map { name: "Ada", email: "a" } (`role` is missing)"#
    );
    assert_eq!(call(r#"greet("Ada")"#), r#"ERROR: argument `u` of `greet` must be User, got str "Ada""#);
    assert_eq!(call("bad()"), r#"ERROR: `bad` must return User, but returned map { name: "x" } (`email` is missing)"#);
    // Patterns make the value as `User(v)` does.
    assert_eq!(
        call(r#"[{ name: "Bo", email: "b" }, { name: "Bo" }, 5] | map((v) => match v { User as u => u.role, _ => "not a user" })"#),
        r#"["member", "not a user", "not a user"]"#
    );
    assert_eq!(call(r#"match { id: 1, lines: [] } { Order as o => o.lines, _ => null }"#), "[]");
}

fn post(it: &mut Interp, url: &str, ctype: &str, body: &str) -> HttpResponse {
    it.handle(&HttpRequest {
        method: "POST".into(),
        url: url.into(),
        headers: vec![("Host".into(), "localhost".into()), ("Content-Type".into(), ctype.into()), ("Origin".into(), "http://localhost".into())],
        body: body.as_bytes().to_vec(),
        remote: "127.0.0.1".into(),
    })
}

#[test]
fn requests_are_checked_against_records() {
    let src = format!(
        "{}\n{}",
        TYPES,
        r#"
type Signup = { email: str, age: int, news: bool = false }
route POST /users { json: User } = json
route POST /signup { form: Signup } = form
route POST /users = respond(422, "not a user")
route POST /signup = respond(422, "not a signup")
server fn save(u: User) = u.name
"#
    );
    let mut it = Interp::new(".");
    it.load_source(&src, "app.qwe").unwrap();
    let r = post(&mut it, "/users", "application/json", r#"{"name": "Ada", "email": "a@x", "role": "admin", "is_staff": true}"#);
    // What the client sent beyond the record does not get through.
    assert_eq!(String::from_utf8_lossy(&r.body), r#"{"name":"Ada","email":"a@x","role":"admin","tags":[]}"#);
    assert_eq!(post(&mut it, "/users", "application/json", r#"{"name": "Ada"}"#).status, 422);
    // Form fields are text: converted to what the record asks, as query values are.
    let r = post(&mut it, "/signup", "application/x-www-form-urlencoded", "email=a%40x&age=41");
    assert_eq!(String::from_utf8_lossy(&r.body), r#"{"email":"a@x","age":41,"news":false}"#);
    assert_eq!(post(&mut it, "/signup", "application/x-www-form-urlencoded", "email=a&age=old").status, 422);
    // A server fn's arguments, from the browser.
    let r = post(&mut it, "/_quillo/rpc/save", "application/json", r#"[{"name": "Ada"}]"#);
    assert_eq!(r.status, 400);
    assert!(String::from_utf8_lossy(&r.body).contains("must be User, got map (`email` is missing)"), "{}", String::from_utf8_lossy(&r.body));
}

fn check(src: &str) -> Vec<String> {
    quillo::check::check_source(&format!("{}\n{}", TYPES, src), "main.qwe", Path::new(".")).errors().map(|d| d.message.clone()).collect()
}

#[test]
fn quillo_check_holds_code_to_the_fields() {
    assert!(check("fn ok(u: User) -> str = u.name + u.role\nlet a = User({ name: \"A\", email: \"a\" })\nfn team() = Team({ name: \"t\" })\nlet n = Node({ value: 1, kids: [{ value: 2 }] })\n").is_empty(), "{:?}", check("fn ok(u: User) -> str = u.name + u.role\n"));
    let e = check("fn f(u: User) = u.nmae\nlet a = User({ name: \"A\", emial: \"a\" })\nlet b = User({ name: \"A\" })\nlet c = User({ name: 5, email: \"a\" })\nfn g(o: Order) = o.lines[0].skew\n");
    let has = |s: &str| e.iter().any(|m| m.contains(s));
    assert!(has("no field `nmae` (did you mean `name`?)"), "{:?}", e);
    assert!(has("`User` has no field `emial` (did you mean `email`?)"), "{:?}", e);
    assert!(has("has no field `email`"), "{:?}", e);
    assert!(has("`name` as int, but it must be str"), "{:?}", e);
    assert!(has("no field `skew`"), "{:?}", e);
    let e = check("type Bad = { n: int = \"x\" }\ntype User = { a: int }\n");
    assert!(e.iter().any(|m| m.contains("the default of `n` in `Bad` is str, which is not int")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("`User` is defined twice")), "{:?}", e);
    // A type's name starts with a capital letter; an unknown lowercase one is a typo.
    let e = check("fn f(x: strng) = x\n");
    assert!(e.iter().any(|m| m.contains("unknown type `strng`")), "{:?}", e);
}

#[test]
fn unknown_type_names_in_annotations_are_reported() {
    let e = check(
        r#"
type Todo = { id: int, text: str }
type OrderLine = { id: int }
type Usér = { id: int }
type BadRecord = { owner: Usr }
type BadAlias = list<Tdoo>
fn param_typo(u: Usr) = u
fn dup_line(a: Dup, b: Dup) -> Dup = a
server fn ret_and_err() -> Tdoo ! ErrTyp = err("x")
fn nested(xs: list<Usr>, m: { owner: Usr }, u: Usr?, both: int | Usr) = [xs, m, u, both]
component C(u: Usr) = <p>{u}</p>
view Card() = <div/>
fn bad_view_ty(c: Card) = c
fn case_typo(o: Orderline) = o
fn nonascii_case(u: UsÉr) = u
fn fwd_ok(x: Later) -> Later = x
type Later = { y: int }
fn outer() {
    fn inner(u: Usr) = u
    let a = (u: Usr) => u
    let b = (x): Usr => x
    [inner, a, b]
}
fn local_type() {
    type Local = { next: Local? }
}
fn q_caller() = q_target(1)
fn q_target(x: Qmissing) = x
fn far_name(x: Zzzzzz) = x
fn upper_ret() -> Int = 1
type BadStr = list<Str>
fn upper_more(a: Bool, b: Any, c: Float, d: List, e: Map, f: Dec, g: Num, h: Date, i: Money, j: Html, k: Fn, l: Response) = [a, b, c, d, e, f, g, h, i, j, k, l]
fn foreign_more(a: Integer, b: Long, c: String, d: Boolean, e: Object, f: Double, g: Array, h: Vec, i: Dict, j: Decimal, k: Number) = [a, b, c, d, e, f, g, h, i, j, k]
"#,
    );
    let count = |s: &str| e.iter().filter(|m| m.contains(s)).count();
    assert!(e.iter().any(|m| m.contains("there is no type `Usr` (did you mean `User`?)")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("there is no type `Tdoo` (did you mean `Todo`?)")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("there is no type `Orderline` (did you mean `OrderLine`?)")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("there is no type `UsÉr` (did you mean `Usér`?)")), "{:?}", e);
    assert!(!e.iter().any(|m| m.contains("Later")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("there is no type `Card`") && !m.contains("did you mean")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("there is no type `ErrTyp`") && !m.contains("did you mean")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("there is no type `Zzzzzz`") && !m.contains("did you mean")), "{:?}", e);
    assert!(e.iter().any(|m| m.contains("`type` must be at the top level")), "{:?}", e);
    assert!(!e.iter().any(|m| m.contains("there is no type `Local`")), "{:?}", e);
    for (bad, good) in [
        ("Int", "int"),
        ("Float", "float"),
        ("Dec", "dec"),
        ("Num", "num"),
        ("Str", "str"),
        ("Date", "date"),
        ("Money", "money"),
        ("Bool", "bool"),
        ("List", "list"),
        ("Map", "map"),
        ("Html", "html"),
        ("Fn", "fn"),
        ("Any", "any"),
        ("Response", "response"),
        ("Integer", "int"),
        ("Long", "int"),
        ("String", "str"),
        ("Boolean", "bool"),
        ("Object", "any"),
        ("Double", "float"),
        ("Array", "list"),
        ("Vec", "list"),
        ("Dict", "map"),
        ("Decimal", "dec"),
        ("Number", "num"),
    ] {
        let want = format!("there is no type `{}`: the builtin types are written in small letters, `{}`", bad, good);
        assert!(e.iter().any(|m| m.contains(&want)), "missing `{}` in {:?}", want, e);
    }
    // All 7 distinct lines with Usr, both Tdoo lines, same-line dedup on Dup, and quiet-prechecked Qmissing.
    assert_eq!(count("there is no type `Usr`"), 7, "{:?}", e);
    assert_eq!(count("there is no type `Tdoo`"), 2, "{:?}", e);
    assert_eq!(count("there is no type `Dup`"), 1, "{:?}", e);
    assert_eq!(count("there is no type `Qmissing`"), 1, "{:?}", e);

    // Imported type alias works, while failed or unparsed imports suppress cascading errors.
    let dir = support::temp_dir("check-unknown-type-imports");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/geo.qwe"), "type Point = { x: int, y: int }\n").unwrap();
    std::fs::write(dir.join("main.qwe"), "import lib/geo.qwe use Point as Pt\ntype Wrapper = { p: Pt }\nfn id(p: Pt) -> Pt = p\n").unwrap();
    let ok = quillo::check::check_file(&dir.join("main.qwe"));
    assert!(ok.diagnostics.is_empty(), "{:#?}", ok.diagnostics);

    std::fs::write(
        dir.join("bad_whole.qwe"),
        "use \"lib/missing.qwe\"\nfn f(u: Missing) = u\n",
    )
    .unwrap();
    let errs: Vec<String> = quillo::check::check_file(&dir.join("bad_whole.qwe")).errors().map(|d| d.message.clone()).collect();
    assert!(errs.iter().any(|m| m.contains("missing.qwe")), "{:?}", errs);
    assert!(!errs.iter().any(|m| m.contains("there is no type `Missing`")), "{:?}", errs);

    std::fs::write(
        dir.join("bad_named.qwe"),
        "import lib/geo.qwe use Missing\nfn f(u: Missing) = u\n",
    )
    .unwrap();
    let errs: Vec<String> = quillo::check::check_file(&dir.join("bad_named.qwe")).errors().map(|d| d.message.clone()).collect();
    assert!(errs.iter().any(|m| m.contains("has no public name `Missing`")), "{:?}", errs);
    assert!(!errs.iter().any(|m| m.contains("there is no type `Missing`")), "{:?}", errs);

    std::fs::write(
        dir.join("lib/broken.qwe"),
        "type BrokenUser = { id: int }\nfn bad( =\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("bad_parse.qwe"),
        "use \"lib/broken.qwe\"\nfn f(u: BrokenUser) = u\n",
    )
    .unwrap();
    let errs: Vec<String> = quillo::check::check_file(&dir.join("bad_parse.qwe")).errors().map(|d| d.message.clone()).collect();
    assert!(errs.iter().any(|m| m.contains("expected a parameter name")), "{:?}", errs);
    assert!(!errs.iter().any(|m| m.contains("there is no type `BrokenUser`")), "{:?}", errs);
    let _ = std::fs::remove_dir_all(&dir);

    // Confined checks bound suggestion work across many unknown type annotations.
    let mut big = String::new();
    for i in 0..500 {
        big.push_str(&format!("type Rec{:04} = {{ v: int }}\n", i));
    }
    for i in 0..250 {
        big.push_str(&format!("fn f{}(x: Xec{:04}) = x\n", i, i));
    }
    let started = std::time::Instant::now();
    let confined_errs: Vec<String> = quillo::check::check_source_opts(
        &big,
        "confined.qwe",
        Path::new("."),
        quillo::check::Options { confined: true, ..Default::default() },
    )
    .errors()
    .map(|d| d.message.clone())
    .collect();
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert!(confined_errs.iter().any(|m| m.contains("there is no type `Xec0000` (did you mean `Rec0000`?)")), "{:?}", &confined_errs[..5.min(confined_errs.len())]);
    assert!(confined_errs.iter().any(|m| m == "there is no type `Xec0249`"), "{:?}", confined_errs.last());

    // User-defined type shadowing a builtin name is accepted without error.
    let shadowed = quillo::check::check_source("type Date = { y: int }\nfn ok_date(d: Date) -> Date = d\n", "shadow.qwe", Path::new("."));
    assert!(shadowed.diagnostics.is_empty(), "{:#?}", shadowed.diagnostics);

    // Confined checks skip case-only scans and suggestions for names past 64 characters.
    let long_def = format!("Long{}A", "x".repeat(60));
    let long_use = format!("Long{}a", "x".repeat(60));
    let long_errs: Vec<String> = quillo::check::check_source_opts(
        &format!("type {} = {{ v: int }}\nfn f(x: {}) = x\n", long_def, long_use),
        "long_confined.qwe",
        Path::new("."),
        quillo::check::Options { confined: true, ..Default::default() },
    )
    .errors()
    .map(|d| d.message.clone())
    .collect();
    assert_eq!(long_errs, vec![format!("there is no type `{}`", long_use)]);
}

// ---- the browser ------------------------------------------------------------------------------

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn both(body: &str) -> (String, String) {
    let defs = "fn greet(u: User) -> str = `Hi ${u.name}`\n";
    let src = format!("{}\n{}\nfn __t() {{\n{}\n}}\n", TYPES, defs, body);
    let mut it = Interp::new(".");
    it.load_source(&src, "case.qwe").unwrap();
    let server = match it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0) {
        Ok(v) => v.repr(),
        Err(quillo::interp::Ctrl::Error(e)) => format!("ERROR: {}", e.msg),
        Err(_) => "returned".into(),
    };
    let js = quillo::client::test_script(&it, "__t").unwrap_or_else(|e| panic!("{}", e));
    let out = support::node(&js).unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    (server, String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[test]
fn the_browser_makes_and_checks_records_the_same() {
    if !node_available() {
        return;
    }
    for e in [
        r#"User({ name: "Ada", email: "ada@x.io", admin: true })"#,
        r#"[User({ name: "Ada" }), User({ name: "Ada", email: 5 }), User({ name: "Ada", email: "a", role: "boss" })]"#,
        r#"[User({ name: "Ada", email: "a", address: { zip: "1" } }), User({ name: "Ada", email: "a", tags: ["x", 2] }), User(5)]"#,
        r#"Team({ name: "core", members: [{ name: "Ada", email: "a" }] })"#,
        r#"[Team({ name: "c", members: [{ name: "Ada", email: "a" }, { name: "Bo" }] }), Order({ id: "A1", lines: [{ sku: "x", qty: "2" }] })]"#,
        r#"[Id(5), Id("x"), Id(true)]"#,
        r#"[{ name: "Bo", email: "b" }, { name: "Bo" }, 5] | map((v) => match v { User as u => u, _ => "not a user" })"#,
        r#"match { json: { name: "Bo", email: "b", extra: 1 } } { { json: User } => json, _ => null }"#,
        r#"greet(User({ name: "Ada", email: "a" }))"#,
        r#"greet({ name: "Ada", email: "a" })"#,
    ] {
        let (s, b) = both(e);
        assert_eq!(s, b, "{}", e);
    }
}

/// A whole number is made into a float where data is made into a type: JSON,
/// and a browser's JavaScript, have one kind of number, so a reading of 5.0
/// arrives as 5. A union that takes an int keeps it (FEEDBACK18.md, Q-103;
/// branch feedback18-fixes).
#[test]
fn whole_numbers_are_floats_where_data_is_made() {
    let src = r#"
type Spot = { lat: float, acc: float?, path: list<float> = [] }
type Either = { n: float | int }
fn __a() = Spot(json.parse("{\"lat\": 1, \"acc\": 5, \"path\": [1, 2.5]}"))
fn __b() = Either(json.parse("{\"n\": 5}")).n
fn __c() = type_of(Spot(json.parse("{\"lat\": 1}")).lat)
server fn half(x: float) = x / 2
server fn far(at: Spot) = at.lat + 0.5
"#;
    let mut it = Interp::new(".");
    it.load_source(src, "app.qwe").unwrap();
    let call = |it: &mut Interp, f: &str| it.call(&quillo::value::lookup(&it.globals, f).unwrap(), vec![], 0).ok().unwrap().repr();
    assert_eq!(call(&mut it, "__a"), "{ lat: 1.0, acc: 5.0, path: [1.0, 2.5] }");
    assert_eq!(call(&mut it, "__b"), "5");
    assert_eq!(call(&mut it, "__c"), "\"float\"");
    let r = post(&mut it, "/_quillo/rpc/half", "application/json", "[5]");
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert!(String::from_utf8_lossy(&r.body).contains("2.5"), "{}", String::from_utf8_lossy(&r.body));
    let r = post(&mut it, "/_quillo/rpc/far", "application/json", r#"[{"lat": 1}]"#);
    assert!(String::from_utf8_lossy(&r.body).contains("1.5"), "{}", String::from_utf8_lossy(&r.body));
    // Text is still not a float.
    assert_eq!(post(&mut it, "/_quillo/rpc/half", "application/json", r#"["5"]"#).status, 400);
}
