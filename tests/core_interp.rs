//! The interpreter's edges: what each operation refuses and how it says so,
//! markup rendered through every path a view can take, and the names and
//! fields it follows for live data.

use quillo::interp::{coerce, type_matches, Ctrl};
use quillo::server::{HttpRequest, HttpResponse};
use quillo::{eval_str, Interp, QuilloError, Value};

mod support;

fn gives(src: &str) -> String {
    match eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn fails(src: &str) -> (u32, String) {
    match eval_str(src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => (e.line, e.msg),
    }
}

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}\n--- source ---\n{}", e, src);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut all = vec![("Host".to_string(), "localhost".to_string()), ("Content-Type".to_string(), "application/json".to_string())];
    all.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: all, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    send(it, "GET", url, &[], "")
}

/// The page `route GET / = <body>` gives, after `defs`.
fn page(defs: &str, body: &str) -> String {
    let mut it = app(&format!("{}\nroute GET / = {}\n", defs, body));
    let r = get(&mut it, "/");
    assert_eq!(r.status, 200, "{}", r.text());
    r.text()
}

#[test]
fn a_parse_error_is_a_control_flow_error() {
    let c: Ctrl = QuilloError::new("no", 4).into();
    let Ctrl::Error(e) = c else { panic!("not an error") };
    assert_eq!((e.line, e.msg.as_str()), (4, "no"));
}

#[test]
fn values_are_of_the_builtin_types() {
    assert_eq!(gives("fn f(a: html) = a\nf(<b/>)"), "<b></b>");
    assert_eq!(fails("fn f(a: html) = a\nf(1)").1, "argument `a` of `f` must be html, got int 1");
    assert_eq!(gives("fn f(a: fn) = a()\nf(() => 2)"), "2");
    assert_eq!(gives("fn f(a: fn) = a(\"x\")\nf(len)"), "1");
    assert_eq!(fails("fn f(a: fn) = a\nf(1)").1, "argument `a` of `f` must be fn, got int 1");
    assert_eq!(gives("fn f(a: response) = a.status\nf(respond(201, \"x\"))"), "201");
    assert_eq!(fails("fn f(a: response) = a\nf(1)").1, "argument `a` of `f` must be response, got int 1");
}

#[test]
fn a_type_checked_from_outside_the_language() {
    let ty = |src: &str| {
        let e = quillo::parser::Parser::new(&format!("fn f(a: {}) = a", src)).parse_program().unwrap_or_else(|e| panic!("{}", e));
        let quillo::ast::Stmt::Fn(def) = &e[0] else { panic!("not a fn") };
        def.params[0].ty.clone().unwrap()
    };
    assert!(type_matches(&ty("int?"), &Value::Nil));
    assert!(type_matches(&ty("int | str"), &Value::str("x")));
    assert!(!type_matches(&ty("int"), &Value::Nil));
    assert!(!type_matches(&ty("Thing"), &Value::Int(1)));
}

#[test]
fn request_text_becomes_the_type_a_pattern_asks_for() {
    let b = |s: &str| coerce("bool", s).map(|v| v.repr());
    assert_eq!(b("yes").as_deref(), Some("true"));
    for no in ["false", "off", "0", "no"] {
        assert_eq!(b(no).as_deref(), Some("false"));
    }
    assert_eq!(b("maybe"), None);
    assert_eq!(coerce("list", "x").map(|v| v.repr()), None);
    let mut it = app("route GET /x?{ on: bool } = on ? \"yes\" : \"no\"");
    assert_eq!(get(&mut it, "/x?on=off").text(), "no");
    assert_eq!(get(&mut it, "/x?on=maybe").status, 400);
}

#[test]
fn a_field_path_stops_where_the_value_is_no_map() {
    assert_eq!(gives("match { a: 1 } { { a.b: x } => x, _ => \"none\" }"), "\"none\"");
    assert_eq!(gives("match { a: { b: 2 } } { { a.b: x } => x, _ => \"none\" }"), "2");
}

#[test]
fn urls_of_other_kinds_of_value_are_shown_or_blocked() {
    assert_eq!(page("", "<a href={1}>x</a>"), "<a href=\"1\">x</a>");
    assert_eq!(page("", "<a href={2.5}>x</a>"), "<a href=\"2.5\">x</a>");
    assert_eq!(page("", "<a href={<b>javascript:alert(1)</b>}>x</a>"), "<a href=\"&lt;b&gt;javascript:alert(1)&lt;/b&gt;\">x</a>");
    // SMIL values are lists of URLs.
    let s = page("", "<svg><animate attributeName=\"href\" values={1.5}/></svg>");
    assert!(s.contains("values=\"1.5\""), "{}", s);
    let s = page("", "<svg><animate attributeName=\"href\" values={null}/></svg>");
    assert!(!s.contains("values"), "{}", s);
    // Markup is shown as its text, so it is judged as that text.
    assert_eq!(page("", "<a href={raw(\"javascript:alert(1)\")}>x</a>"), "<a href=\"about:blank#blocked\">x</a>");
    let s = page("", "<svg><animate attributeName=\"href\" values={raw(\"javascript:alert(1)\")}/></svg>");
    assert!(!s.contains("javascript:"), "{}", s);
}

#[test]
fn a_script_takes_values_as_data() {
    assert_eq!(page("", "<script>let a = {null};</script>").replace(' ', ""), "<script>leta=;</script>");
    assert_eq!(page("", "<script>let a = {false};</script>").replace(' ', ""), "<script>leta=;</script>");
    assert_eq!(page("", "<script>let a = {[1, \"x\"]};</script>").replace(' ', ""), "<script>leta=[1,\"x\"];</script>");
}

#[test]
fn a_style_sheet_takes_values_as_text() {
    let body = "<style>{null}{false}{[\"a { }\", \"b { }\"]}{1}{\"</style>\"}</style>";
    assert_eq!(page("", body), "<style>a { }\nb { }1<\\/style></style>");
}

#[test]
fn what_cannot_reach_the_browser_is_named() {
    let a = "component A(x) = <i>{1}</i>\n";
    let b = "component B() {\n  state x = <b/>\n  <i/>\n}\n";
    let hint = "; pass it from inside another component, or use a server fn";
    assert_eq!(fails(&format!("{}<A x={{<b/>}}/>", a)), (2, format!("prop `x` of <A> is html, which cannot be sent to the browser{}", hint)));
    assert_eq!(fails(&format!("{}<A x={{respond(200, \"x\")}}/>", a)), (2, format!("prop `x` of <A> is a response, which cannot be sent to the browser{}", hint)));
    assert_eq!(fails(&format!("{}<B/>", b)), (5, "state `x` of <B> is html, which cannot be sent to the browser".to_string()));
}

#[test]
fn what_operations_refuse() {
    let cases: &[(&str, u32, &str)] = &[
        ("\n[] % 2", 2, "cannot take the remainder of list and int"),
        ("[] - 2", 1, "cannot subtract list and int"),
        ("[] * 2", 1, "cannot multiply list and int"),
        ("[] / 2", 1, "cannot divide list and int"),
        ("1.5..3", 1, "`..` needs two ints"),
        ("-\"a\"", 1, "cannot negate str"),
        ("5[1]", 1, "cannot index int with int"),
        ("[1][\"a\"]", 1, "cannot index list with str"),
        ("for x in 5 => x", 1, "cannot loop over int"),
        ("for [a] in [1] => a", 1, "`for` pattern does not match 1"),
        ("reply 99", 1, "reply status must be an HTTP status code, got 99"),
        ("for i in \"a\"..\"b\" { i }", 1, "`..` needs two ints"),
    ];
    for (src, line, msg) in cases {
        assert_eq!(fails(src), (*line, msg.to_string()), "{}", src);
    }
}

#[test]
fn a_long_value_in_a_message_is_cut_short() {
    let long = "x".repeat(60);
    let (_, msg) = fails(&format!("reply \"{}\"", long));
    assert_eq!(msg, format!("reply status must be an HTTP status code, got \"{}…", "x".repeat(39)));
}

#[test]
fn arithmetic_on_the_rarer_kinds() {
    assert_eq!(gives("<a/> + <b/>"), "<a></a><b></b>");
    assert_eq!(gives("2.5 - 1"), "1.5");
    assert_eq!(gives("7.5 % 2"), "1.5");
    assert_eq!(gives("\"abc\"[-1]"), "\"c\"");
    assert_eq!(gives("\"abc\"[-4]"), "null");
    assert_eq!(gives("for x in null => x"), "[]");
    assert_eq!(gives("for i in range(1, 3) => i"), "[1, 2]");
    assert_eq!(gives("let n = 0\nfor i in 2..1 { n += 1 }\nn"), "0");
}

#[test]
fn a_range_of_the_program_s_own_is_called() {
    assert_eq!(gives("fn range(a, b) = [a, b]\nfor i in range(1, 2) => i * 10"), "[10, 20]");
    assert_eq!(gives("let range = (a) => [a]\nfor i in range(1) => i"), "[1]");
    assert_eq!(fails("for i in range(\"a\", 2) { i }").1, "`range` expects an int as argument 1, got str");
}

#[test]
fn parallel_with_one_function_just_calls_it() {
    assert_eq!(gives("parallel(() => 1)"), "[1]");
    assert_eq!(fails("parallel(() => 1 / 0)").1, "division by zero");
}

#[test]
fn a_program_may_end_early() {
    assert_eq!(gives("1\nreturn 2\n3"), "2");
    let (line, msg) = fails("reply 200");
    assert_eq!((line, msg.as_str()), (0, "`reply` used outside of a route"));
}

#[test]
fn definitions_of_the_top_level_stay_there() {
    assert_eq!(fails("fn f() {\n  type Abc = int\n}\nf()"), (2, "`type` must be at the top level".to_string()));
    assert_eq!(fails("fn f() {\n  service x = \"x.qwe\"\n}\nf()"), (2, "`service` must be at the top level".to_string()));
    assert_eq!(fails("fn f() {\n  mount /x = \"x.qwe\"\n}\nf()"), (2, "`mount` must be at the top level".to_string()));
}

#[test]
fn used_files_run_once_and_tell_their_own_errors() {
    let d = support::temp_dir("core-use");
    std::fs::write(d.join("once.qwe"), "ran = ran + 1\n").unwrap();
    std::fs::write(d.join("bad.qwe"), "let a = 1\nlet b = a / 0\n").unwrap();
    std::fs::write(d.join("early.qwe"), "let a = 1\nreturn\nlet c = 2\n").unwrap();
    let load = |src: &str| {
        let mut it = Interp::new(d.to_str().unwrap());
        it.load_source(src, "main.qwe")
    };
    let Ok(v) = load("let ran = 0\nuse once.qwe\nuse once.qwe\nuse \"once.qwe\"\nran") else { panic!("did not load") };
    assert_eq!(v.repr(), "1");
    let Err(e) = load("use bad.qwe\n") else { panic!("loaded") };
    assert_eq!((e.line, e.msg.as_str()), (2, "division by zero"));
    assert!(e.file.as_deref().is_some_and(|f| f.ends_with("bad.qwe")), "{:?}", e.file);
    std::fs::remove_dir_all(&d).unwrap();
}

/// `return` at the top of a used file leaves that file, as at the top of a program.
#[test]
#[ignore = "finding: `return` at the top level of a used file also ends the program that used it, silently"]
fn a_return_in_a_used_file_ends_only_that_file() {
    let d = support::temp_dir("core-use-return");
    std::fs::write(d.join("early.qwe"), "let a = 1\nreturn\nlet c = 2\n").unwrap();
    let mut it = Interp::new(d.to_str().unwrap());
    let Ok(v) = it.load_source("use early.qwe\na + 1", "main.qwe") else { panic!("did not load") };
    assert_eq!(v.repr(), "2");
}

#[test]
fn a_return_in_a_used_file_stops_that_file() {
    let d = support::temp_dir("core-use-stop");
    std::fs::write(d.join("early.qwe"), "let a = 1\nreturn\nlet c = 2\n").unwrap();
    let mut it = Interp::new(d.to_str().unwrap());
    assert!(it.load_source("use early.qwe\n", "main.qwe").is_ok());
    assert!(quillo::value::lookup(&it.globals, "a").is_some());
    assert!(quillo::value::lookup(&it.globals, "c").is_none());
}

#[test]
fn a_name_that_shadows_a_module_is_said_to() {
    let (line, msg) = fails("let json = 1\njson.stringify(2)");
    assert_eq!((line, msg.as_str()), (2, "`json.stringify`: `json` is a variable here (int), which hides the module `json`: rename the variable"));
}

#[test]
fn the_fields_of_a_response() {
    assert_eq!(gives("respond(201, \"x\", { a: \"b\" }).status"), "201");
    assert_eq!(gives("respond(201, \"x\", { a: \"b\" }).headers"), "{ a: \"b\" }");
    assert_eq!(gives("respond(201, \"x\").body"), "\"x\"");
    assert_eq!(fails("respond(201, \"x\").other").1, "response has no field `other`");
}

#[test]
fn a_reply_keeps_the_rest_of_a_response() {
    let mut it = app("route GET / {\n  reply 202 respond(200, \"made\", { \"x-a\": \"b\" })\n}");
    let r = get(&mut it, "/");
    assert_eq!((r.status, r.header("x-a"), r.text().as_str()), (202, Some("b"), "made"));
}

#[test]
fn a_method_call_reaches_functions_but_not_through_a_map() {
    assert_eq!(gives("fn twice(x) = x * 2\n(4).twice"), "8");
    assert_eq!(gives("fn twice(x) = x * 2\n{ a: 1 }.twice"), "null");
    assert_eq!(gives("{ a: 1 }.len"), "1");
}

#[test]
fn a_view_with_a_result_type_is_checked_as_it_renders() {
    let v = "view P() -> html {\n  <b/>\n}\nview Q() -> html {\n  \"text\"\n}";
    assert_eq!(page(v, "<P/>"), "<b></b>");
    assert_eq!(fails(&format!("{}\n<Q/>", v)).1, "`Q` must return html, but returned str \"text\"");
}

#[test]
fn a_view_may_return_part_way() {
    let v = "view P(a) {\n  <i/>\n  if a {\n    return <b/>\n  }\n  <u/>\n}";
    assert_eq!(page(v, "<P a={true}/>"), "<b></b>");
    assert_eq!(page(v, "<P a={false}/>"), "<u></u>");
}

#[test]
fn a_view_s_body_may_be_a_choice() {
    assert_eq!(page("view P(a) = a && <b/>", "<P a={true}/>"), "<b></b>");
    assert_eq!(page("view P(a) = a && <b/>", "<P a={null}/>"), "");
    assert_eq!(page("view P(a) = a && <b/>", "<P a={false}/>"), "");
    assert_eq!(page("view P(a) = a && (a && <b/>)", "<P a={1}/>"), "<b></b>");
    assert_eq!(page("view P(a) = a || <b/>", "<P a={null}/>"), "<b></b>");
    assert_eq!(page("view P(a) = a || <b/>", "<P a={\"x\"}/>"), "x");
}

#[test]
fn a_view_s_match_tries_its_guards_in_order() {
    let v = "view P(a) = match a {\n  n if n > 1 => <b/>\n  1 => <i/>\n}";
    assert_eq!(page(v, "<P a={2}/>"), "<b></b>");
    assert_eq!(page(v, "<P a={1}/>"), "<i></i>");
    assert_eq!(fails(&format!("{}\n<P a={{0}}/>", v)).1, "no `match` arm matches 0");
}

#[test]
fn a_view_s_block_may_end_without_a_value() {
    assert_eq!(page("view P() {\n  let _x = 1\n}", "<P/>"), "");
    assert_eq!(page("view P() {\n}", "<P/>"), "");
}

#[test]
fn a_view_loops_over_what_for_can() {
    assert_eq!(page("view P() = for c in \"ab\" => <i>{c}</i>", "<P/>"), "<i>a</i><i>b</i>");
    assert_eq!(page("view P() = for c in null => <i>{c}</i>", "<P/>"), "");
    assert_eq!(fails("view P() = for c in 5 => <i>{c}</i>\n<P/>").1, "cannot loop over int");
    assert_eq!(fails("view P() = for [a] in [1] => <i>{a}</i>\n<P/>").1, "`for` pattern does not match 1");
}

#[test]
fn a_default_in_a_pattern_is_matched_when_it_is_not_a_type_test() {
    assert_eq!(gives("match {} { { a: [x] = [1] } => x }"), "1");
    assert_eq!(gives("match {} { { a: [x] = 5 } => x, _ => \"no\" }"), "\"no\"");
}

#[test]
fn attributes_of_every_kind_of_value() {
    assert_eq!(page("", "<div data-x={[1, 2]}/>"), "<div data-x=\"[1,2]\"></div>");
    assert_eq!(page("", "<div data-x={{ a: 1 }}/>"), "<div data-x=\"{&quot;a&quot;:1}\"></div>");
    assert_eq!(page("let m = { hidden: true, off: false, none: null, l: [1], m: { a: 1 } }", "<div {...m}/>"), "<div hidden l=\"[1]\" m=\"{&quot;a&quot;:1}\"></div>");
    let s = page("let m = { \"a b\": 1, \"x\": 2 }", "<div {...m}/>");
    assert_eq!(s, "<div x=\"2\"></div>");
    assert_eq!(page("let m = { title: \"t\" }", "<div hidden {...m}/>"), "<div title=\"t\" hidden></div>");
    // `bind:` alone names nothing.
    assert_eq!(page("", "<input bind:={1}/>"), "<input>");
    assert_eq!(page("let m = { title: \"t\" }", "<input bind:={1} {...m}/>"), "<input title=\"t\">");
}

#[test]
fn a_name_that_is_no_view_is_said_to() {
    assert_eq!(fails("fn f(Abc) = <Abc/>\nf(1)").1, "<Abc> is not a view");
}

#[test]
fn the_cache_of_views_can_be_off() {
    let mut it = app("let f = () => 1\nfn g() {\n  f = () => 2\n}\nroute GET / = g()");
    it.view_cache = None;
    assert_eq!(it.view_cache_stats(), (0, 0));
    assert_eq!(get(&mut it, "/").status, 204);
}

#[test]
fn a_sheet_no_longer_kept_gives_every_style() {
    let it = app("view P() {\n  style \"i { color: red }\"\n  <i/>\n}");
    let (css, kept) = it.sheet_by_hash("nope");
    assert!(!kept);
    assert!(css.contains("color: red"), "{}", css);
}

#[test]
fn a_style_element_writes_no_elements() {
    assert_eq!(page("", "<style>{\"a\"}<b/></style>"), "<style>a</style>");
}

fn rpc(it: &mut Interp, name: &str, live: bool) -> serde_json::Value {
    let url = format!("/_quillo/rpc/{}", name);
    let r = send(it, "POST", &url, if live { &[("X-Quillo-Live", "1")] } else { &[] }, "[]");
    serde_json::from_str(&r.text()).unwrap_or_else(|e| panic!("{}: {}", e, r.text()))
}

fn changed_since(it: &mut Interp, since: u64, topics: &str) -> serde_json::Value {
    serde_json::from_str(&get(it, &format!("/_quillo/live?since={}&t={}", since, topics)).text()).unwrap()
}

fn now(it: &mut Interp) -> u64 {
    changed_since(it, 0, "")["seq"].as_u64().unwrap()
}

const WORLD: &str = r#"
fn f1() = 1
fn f2() = 2
let world = { tick: 0, name: "w", rows: [1, 2], sub: { a: 1 }, f: f1 }
let big = 0..40
let plain = 1
server fn load() = world.tick
server fn load_sub() = world.sub
server fn load_module() {
    let f = json.stringify
    f(1)
}
server fn load_local() {
    let m = { a: 1 }
    m.a
}
server fn load_missing() = world.nothing
server fn tick() { world = { ...world, tick: world.tick + 1 } }
server fn rename() { world = { ...world, name: "x" } }
server fn same() { world = { tick: world.tick, name: "w", rows: [1, 2], sub: { a: 1 }, f: world.f } }
server fn big_list() { world = { ...world, rows: 0..40 } }
server fn big_map() { world = { ...world, sub: for k in 0..40 => k } }
server fn new_fn() { world = { ...world, f: f2 } }
server fn drop_name() { world = { tick: world.tick, rows: world.rows, sub: world.sub, f: world.f } }
server fn to_int() { world = 5 }
server fn to_other() { world = 6 }
server fn to_map() { world = { tick: 9 } }
"#;

#[test]
fn a_loader_that_read_one_field_follows_that_field() {
    let mut it = app(WORLD);
    let r = rpc(&mut it, "load", true);
    assert_eq!((r["ok"].clone(), r["topics"].clone()), (serde_json::json!(0), serde_json::json!(["var:world.tick"])));
    let fields = "var:world.tick,var:world.name,var:world.rows,var:world.sub,var:world.f,var:world";
    let changed = |it: &mut Interp, f: &str| {
        let since = now(it);
        rpc(it, f, false);
        let c = changed_since(it, since, fields)["changed"].clone();
        c.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).filter(|t| t != "var:world").collect::<Vec<String>>().join(" ")
    };
    assert_eq!(changed(&mut it, "tick"), "var:world.tick");
    assert_eq!(changed(&mut it, "rename"), "var:world.name");
    // Equal small values are no change, in a new map as in the old.
    assert_eq!(changed(&mut it, "rename"), "");
    assert_eq!(changed(&mut it, "new_fn"), "var:world.f");
}

#[test]
fn a_large_part_is_taken_as_changed_when_it_is_made_anew() {
    let mut it = app(WORLD);
    rpc(&mut it, "load", true);
    let fields = "var:world.rows,var:world.sub,var:world.name";
    let since = now(&mut it);
    rpc(&mut it, "big_list", false);
    assert_eq!(changed_since(&mut it, since, fields)["changed"], serde_json::json!(["var:world.rows"]));
    let since = now(&mut it);
    rpc(&mut it, "big_list", false);
    assert_eq!(changed_since(&mut it, since, fields)["changed"], serde_json::json!(["var:world.rows"]));
    let since = now(&mut it);
    rpc(&mut it, "drop_name", false);
    assert_eq!(changed_since(&mut it, since, fields)["changed"], serde_json::json!(["var:world.name"]));
}

#[test]
fn a_global_that_stops_being_a_map_changes_all_its_fields() {
    let mut it = app(WORLD);
    rpc(&mut it, "load", true);
    let fields = "var:world.tick,var:world.name,var:world.sub";
    let since = now(&mut it);
    rpc(&mut it, "to_int", false);
    assert_eq!(changed_since(&mut it, since, fields)["changed"], serde_json::json!(["var:world.tick", "var:world.name", "var:world.sub"]));
    let since = now(&mut it);
    rpc(&mut it, "to_other", false);
    assert_eq!(changed_since(&mut it, since, fields)["changed"], serde_json::json!([]));
    let since = now(&mut it);
    rpc(&mut it, "to_map", false);
    assert_eq!(changed_since(&mut it, since, fields)["changed"], serde_json::json!(["var:world.tick"]));
}

#[test]
fn only_a_global_map_s_own_fields_are_followed_one_by_one() {
    let mut it = app(WORLD);
    assert_eq!(rpc(&mut it, "load_module", true)["ok"], "1");
    assert_eq!(rpc(&mut it, "load_local", true)["topics"], serde_json::json!([]));
    let r = rpc(&mut it, "load_missing", true);
    assert_eq!((r["ok"].clone(), r["topics"].clone()), (serde_json::Value::Null, serde_json::json!(["var:world"])));
}

#[test]
fn a_cached_view_tells_the_view_around_it_what_it_read() {
    let mut it = app("let n = 1\nview Inner() = <i>{n}</i>\nview Outer(a) = <b>{a}<Inner/></b>\nroute GET /o/:a(int) = <Outer a={a}/>\nroute POST /n { n = n + 1 }");
    assert_eq!(get(&mut it, "/o/1").text(), "<b>1<i>1</i></b>");
    // Outer is new here, and Inner comes from the cache.
    assert_eq!(get(&mut it, "/o/2").text(), "<b>2<i>1</i></b>");
    assert_eq!(it.view_cache_stats().0, 1);
    assert_eq!(get(&mut it, "/o/2").text(), "<b>2<i>1</i></b>");
    assert_eq!(it.view_cache_stats().0, 2);
    send(&mut it, "POST", "/n", &[], "");
    // Outer read nothing itself: what Inner read says it is out of date.
    assert_eq!(get(&mut it, "/o/2").text(), "<b>2<i>2</i></b>");
}

#[test]
fn a_waiting_component_keeps_what_it_followed() {
    let src = r#"
server fn followed() = 1
server fn slow() = "done"
component C() {
    state a = live(followed)
    state b = defer(slow())
    <i>{a} {b}</i>
}
route GET / = <html><body><C/></body></html>
"#;
    let mut it = app(src);
    let body = get(&mut it, "/").text();
    let part = &body[body.find("<template data-qt=\"0\">").expect("the part follows")..];
    assert!(part.contains("data-l=\""), "{}", part);
    assert!(part.contains("followed"), "{}", part);
    assert!(part.contains("done"), "{}", part);
}

#[test]
fn a_waiting_component_that_fails_stays_as_it_was() {
    let src = r#"
server fn bad() = 1 / 0
server fn gone() { reply 404 }
server fn markup() = <b/>
component Bad() {
    state x = defer(bad())
    <i>{x}</i>
}
component Gone() {
    state x = defer(gone())
    <i>{x}</i>
}
component Markup() {
    state x = defer(markup())
    <i>{"m"}</i>
}
route GET /bad = <html><body><Bad/></body></html>
route GET /gone = <html><body><Gone/></body></html>
route GET /markup = <html><body><Markup/></body></html>
"#;
    let mut it = app(src);
    for url in ["/bad", "/gone", "/markup"] {
        let r = get(&mut it, url);
        let body = r.text();
        assert_eq!(r.status, 200, "{}", url);
        assert!(body.contains("data-qd=\"0\""), "{}", body);
        assert!(!body.contains("<template data-qt"), "{}", body);
    }
}

#[test]
fn a_call_on_a_browser_library_is_null_here() {
    let src = "use Chart from \"https://x.example/chart.js\"\nlet made = Chart.new(1)\nlet deep = Chart.a.b.c(2)\nfn f() = { g: () => 7 }\n[made, deep, f().g()]";
    assert_eq!(gives(src), "[null, null, 7]");
    assert_eq!(gives("use Chart from \"https://x.example/chart.js\"\nlet m = { n: () => 8 }\nm.n()"), "8");
}

#[test]
fn a_name_for_another_builtin_is_not_changed_in_place() {
    assert_eq!(gives("let set = push\nfn f() {\n  let xs = [1]\n  xs = set(xs, 2)\n  xs\n}\nf()"), "[1, 2]");
}

#[test]
fn a_response_read_as_bytes_or_as_a_stream() {
    assert!(gives("file(\"Cargo.toml\").body").contains("[package]"));
    assert_eq!(gives("stream((send) => send(\"x\")).body"), "null");
}

#[test]
fn a_script_s_template_stops_at_the_limit() {
    let src = "let s = join(for i in 0..60000 => \"0123456789\", \"\")\nrun_script(\"`${s}${s}`\", { s })";
    assert_eq!(gives(src), "err(\"the script made a value larger than 1 MB\")");
}

#[test]
fn a_request_with_no_host_has_no_tenant() {
    let mut it = app("tenant_from(\"example.com\")\nroute GET /who = tenant() ?? \"none\"");
    let r = it.handle(&HttpRequest { method: "GET".into(), url: "/who".into(), headers: vec![], body: vec![], remote: "127.0.0.1".into() });
    assert_eq!(r.text(), "none");
}

#[test]
fn a_default_in_a_query_pattern_may_read_the_request() {
    let mut it = app("route GET /x?{ a: { b = request.method } as c = {} } = { b, c }");
    assert_eq!(get(&mut it, "/x").text(), "{\"b\":\"GET\",\"c\":{}}");
}
