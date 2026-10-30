//! `private` names reach every corner of a file: wherever a name can stand, it
//! still means the private one, and messages show it as it was written.

use quillo::privacy::{demangle, mangle, private_name, tag};
use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "private.qwe") {
        panic!("{}", e);
    }
    it
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    send(it, "GET", url, &[], "")
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn body(it: &mut Interp, url: &str) -> String {
    let r = get(it, url);
    assert_eq!(r.status, 200, "{}: {}", url, r.text());
    r.text()
}

#[test]
fn a_private_name_works_in_each_kind_of_expression() {
    let mut it = app(
        r#"
private let n = 2
private let m = { k: 5 }
private let xs = [1, 2, 3]
private fn twice(a) = a * 2
private fn maybe(a) = if a > 0 { a } else { null }
private fn wrong() -> int ! str = err("no")

fn tries() -> int ! str {
    let v = wrong()?
    v + n
}

route GET /template = `n is ${n}`
route GET /list = str([n, twice(n)])
route GET /map = str({ a: n }.a)
route GET /unary = str(-n)
route GET /field = str(m.k)
route GET /binary = str(n + n)
route GET /coalesce = str(maybe(0) ?? n)
route GET /pipe = str(n | twice)
route GET /index = str(xs[n])
route GET /while {
    let i = 0
    while i < n { i += 1 }
    str(i)
}
route GET /try = str(is_err(tries()))
route GET /if {
    if n > 1 { "big" } else { "small" }
}
route GET /if-alone {
    let s = "no"
    if n > 1 { s = "yes" }
    s
}
route GET /match = match n {
    1 => "one"
    k if k == n => `matched ${k}`
    _ => "other"
}
route GET /for {
    let total = 0
    for x in xs { total += x * n }
    str(total)
}
route GET /lambda = str(map(xs, (x) => x + n))
route GET /reply {
    reply 202 { n: n }
}
route GET /reply-bare {
    reply (n + 200)
}
"#,
    );
    for (url, want) in [
        ("/template", "n is 2"),
        ("/list", "[2, 4]"),
        ("/map", "2"),
        ("/unary", "-2"),
        ("/field", "5"),
        ("/binary", "4"),
        ("/coalesce", "2"),
        ("/pipe", "4"),
        ("/index", "3"),
        ("/while", "2"),
        ("/try", "true"),
        ("/if", "big"),
        ("/if-alone", "yes"),
        ("/match", "matched 2"),
        ("/for", "12"),
        ("/lambda", "[3, 4, 5]"),
    ] {
        assert_eq!(body(&mut it, url), want, "{}", url);
    }
    let r = get(&mut it, "/reply");
    assert_eq!((r.status, r.text()), (202, r#"{"n":2}"#.to_string()));
    assert_eq!(get(&mut it, "/reply-bare").status, 202);
}

#[test]
fn a_private_name_can_be_assigned_and_its_fields_set() {
    let mut it = app(
        r#"
private let count = 0
private let rec = { a: 1 }

fn bump() { count += 1; return count }
fn nothing() { return }

route GET /bump = str(bump())
route GET /field {
    rec = { a: count + 41 }
    str(rec.a)
}
route GET /set-field {
    rec.a = count
    "no"
}
route GET /nothing = str(nothing())
"#,
    );
    assert_eq!(body(&mut it, "/bump"), "1");
    assert_eq!(body(&mut it, "/field"), "42");
    // What the program wrote still runs, and fails as it would by any name.
    assert_eq!(get(&mut it, "/set-field").status, 500);
    assert_eq!(get(&mut it, "/nothing").text(), "null");
}

#[test]
fn a_route_may_name_its_parts_as_private_names() {
    let mut it = app(
        r#"
private fn id() = "the function"
private fn rest() = "the function"
private let q = "the variable"
private let name = "the variable"

route GET /p/:id = id
route GET /t/:id(int) = str(id + 1)
route GET /r/*rest = rest
route GET /q ? { q } = q
route POST /b { json: { name } } = name
route GET /h { headers: { x_a: q } } = q
route GET /d ? { q = name } = q
"#,
    );
    assert_eq!(body(&mut it, "/p/7"), "7");
    assert_eq!(body(&mut it, "/t/7"), "8");
    assert_eq!(body(&mut it, "/r/a/b"), "a/b");
    assert_eq!(body(&mut it, "/q?q=hi"), "hi");
    assert_eq!(send(&mut it, "POST", "/b", &[("Content-Type", "application/json")], r#"{"name":"ann"}"#).text(), "ann");
    assert_eq!(send(&mut it, "GET", "/h", &[("x_a", "given")], "").text(), "given");
    assert_eq!(body(&mut it, "/d"), "the variable");
}

#[test]
#[ignore = "finding: `{ q: str }` binds the key `q`, while the body's `q` is renamed to the private name and reads the global instead of what matched"]
fn a_typed_field_binds_its_key_though_a_private_name_has_that_name() {
    let mut it = app("private let q = \"the variable\"\nroute GET /q ? { q: str } = q\n");
    assert_eq!(body(&mut it, "/q?q=hi"), "hi");
}

#[test]
fn a_private_name_works_in_each_kind_of_pattern() {
    let mut it = app(
        r#"
private let a = "the variable"
private let b = "the variable"
private let c = "the variable"
private let dflt = 9

route GET /as = match [1, 2] {
    [x, y] as a => str(len(a) + x + y)
}
route GET /alt = match 2 {
    1 | 2 => a
    _ => "no"
}
route GET /list = match [1, 2, 3] {
    [a, ..b] => str(a + len(b))
    _ => "no"
}
route GET /bare-rest = match [1, 2, 3] {
    [a, ..] => str(a)
    _ => "no"
}
route GET /record = match { p: 1 } {
    { p: b, r: int = dflt } => str(b + r)
    _ => "no"
}
route GET /wild = match 1 {
    _ if dflt > 0 => c
    _ => "no"
}
route GET /let {
    let [a, b] = [3, 4]
    str(a * b)
}
route GET /param = ((a) => a + 1)(1)
route GET /default = ((x, b = dflt) => x + b)(1)
"#,
    );
    for (url, want) in [
        ("/as", "5"),
        ("/alt", "the variable"),
        ("/list", "3"),
        ("/bare-rest", "1"),
        ("/record", "10"),
        ("/wild", "the variable"),
        ("/let", "12"),
    ] {
        assert_eq!(body(&mut it, url), want, "{}", url);
    }
    assert_eq!(get(&mut it, "/param").text(), "2");
    assert_eq!(get(&mut it, "/default").text(), "10");
}

#[test]
fn a_private_name_works_throughout_html() {
    let mut it = app(
        r#"
private let who = "ann"
private let cls = "wide"
private let props = { title: "t" }
private let show = true
private view Tag(text: str) = <b>{text}</b>
private fn label(s: str) = `[${s}]`

route GET /page = <div.box-{cls} #item-{who} {...props} data-x={label(who)} hidden={!show}>
    <p class={label("c")}>{who}</p>
    <Tag text={who}/>
    <input disabled/>
</div>
"#,
    );
    let html = body(&mut it, "/page");
    for want in [r#"id="item-ann""#, r#"class="box-wide""#, r#"title="t""#, r#"data-x="[ann]""#, "<p class=\"[c]\">ann</p>", "<b>ann</b>", "disabled"] {
        assert!(html.contains(want), "{}: {}", want, html);
    }
}

#[test]
fn a_components_private_state_and_props_keep_working() {
    let mut it = app(
        r#"
private let start = 3
private component Counter(by: int = start) {
    state total = start
    state other = defer(start + 1)
    <button @click={() => { total += by }}>{total}</button>
}
route GET / = <body><Counter/></body>
"#,
    );
    let html = body(&mut it, "/");
    assert!(html.contains(">3</button>"), "{}", html);
}

#[test]
fn a_prop_cannot_share_a_private_name() {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    let Err(e) = it.load_source("private fn title() = \"t\"\nview Card(title: str) = <b>{title}</b>\n", "private.qwe") else { panic!("loaded") };
    assert!(e.to_string().contains("`title` is a private name in this file, and a prop of `Card` too"), "{}", e);
}

#[test]
fn a_services_options_may_use_a_private_name() {
    let d = std::env::temp_dir().join(format!("quillo-private-service-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("billing.qwe"), "server fn count() = 7\n").unwrap();
    std::fs::write(d.join("app.qwe"), "private let wait = \"1s\"\nservice billing = \"billing.qwe\" { timeout: wait }\nroute GET / = str(billing.count())\n").unwrap();
    let mut it = Interp::new(d.to_str().unwrap());
    it.load_file(&d.join("app.qwe")).unwrap_or_else(|e| panic!("{}", e));
    assert_eq!(body(&mut it, "/"), "7");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn messages_show_names_as_written() {
    let t = tag(Some("lib/a.qwe"), "");
    assert_eq!(t.len(), 8);
    // No file: the source names it.
    assert_ne!(tag(None, "one"), tag(None, "two"));
    assert_eq!(tag(None, "one"), tag(None, "one"));
    let m = mangle("helper", &t);
    assert_eq!(private_name(&m), Some("helper"));
    assert_eq!(demangle(&format!("in {} and {}", m, m)), "in helper and helper");
    // What only looks like a private name stays as it is.
    assert_eq!(demangle("a__pv1234567z"), "a__pv1234567z");
    assert_eq!(demangle("a__pv12345678x"), "a__pv12345678x");
    assert_eq!(demangle(&format!("a__pvshort and {}", m)), "a__pvshort and helper");
    assert_eq!(demangle("no marks"), "no marks");
    assert_eq!(private_name("a__pv123"), None);
}
