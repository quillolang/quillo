//! Views' HTML is reused while their arguments and the globals they read are
//! unchanged, and only then.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str, body: &str) -> HttpResponse {
    let headers = vec![("Host".to_string(), "localhost".to_string()), ("Content-Type".into(), "application/json".into())];
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

fn get(it: &mut Interp, url: &str) -> String {
    send(it, "GET", url, "").text()
}

/// Hits since the last call.
fn hits(it: &Interp, before: &mut u64) -> u64 {
    let (h, _) = it.view_cache_stats();
    let n = h - *before;
    *before = h;
    n
}

#[test]
fn a_view_is_reused_until_a_global_it_read_changes() {
    let mut it = app(
        r#"
let greeting = "hello"
view Hi(name: str) = <p>{greeting}, {name}</p>
route GET /a = <Hi name="ada"/>
route POST /set {
    greeting = request.json.g
    "ok"
}
"#,
    );
    let mut h = 0;
    assert_eq!(get(&mut it, "/a"), "<p>hello, ada</p>");
    assert_eq!(hits(&it, &mut h), 0);
    assert_eq!(get(&mut it, "/a"), "<p>hello, ada</p>");
    assert_eq!(hits(&it, &mut h), 1);
    send(&mut it, "POST", "/set", r#"{"g":"hi"}"#);
    assert_eq!(get(&mut it, "/a"), "<p>hi, ada</p>");
    assert_eq!(hits(&it, &mut h), 0);
    assert_eq!(get(&mut it, "/a"), "<p>hi, ada</p>");
    assert_eq!(hits(&it, &mut h), 1);
}

#[test]
fn different_arguments_are_kept_apart() {
    let mut it = app("view N(n: int, s: str = \"\") = <b>{n}{s}</b>\nroute GET /n/:n(int) = <N n={n} s={request.query.s ?? \"\"}/>");
    let mut h = 0;
    assert_eq!(get(&mut it, "/n/1"), "<b>1</b>");
    assert_eq!(get(&mut it, "/n/2"), "<b>2</b>");
    assert_eq!(get(&mut it, "/n/1?s=x"), "<b>1x</b>");
    assert_eq!(hits(&it, &mut h), 0);
    assert_eq!(get(&mut it, "/n/2"), "<b>2</b>");
    assert_eq!(get(&mut it, "/n/1"), "<b>1</b>");
    assert_eq!(hits(&it, &mut h), 2);
}

#[test]
fn a_list_is_the_same_only_if_it_is_the_same_list() {
    let mut it = app(
        r#"
let xs = [1, 2]
view L(items: list) = <ul>{for x in items => <li>{x}</li>}</ul>
route GET /l = <L items={xs}/>
route GET /fresh = <L items={[1, 2]}/>
route POST /push {
    xs = xs + [3]
    "ok"
}
"#,
    );
    let mut h = 0;
    assert_eq!(get(&mut it, "/l"), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(get(&mut it, "/l"), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(hits(&it, &mut h), 1);
    // A new list each time: equal, but not known to be without looking.
    assert_eq!(get(&mut it, "/fresh"), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(get(&mut it, "/fresh"), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(hits(&it, &mut h), 0);
    send(&mut it, "POST", "/push", "");
    assert_eq!(get(&mut it, "/l"), "<ul><li>1</li><li>2</li><li>3</li></ul>");
}

#[test]
fn a_page_of_views_follows_what_its_innermost_view_read() {
    let mut it = app(
        r#"
let count = 1
view Inner() = <i>{count}</i>
view Outer(title: str) = <div><h1>{title}</h1><Inner/></div>
route GET /o = <Outer title="t"/>
route POST /inc {
    count += 1
    "ok"
}
"#,
    );
    let mut h = 0;
    assert_eq!(get(&mut it, "/o"), "<div><h1>t</h1><i>1</i></div>");
    assert_eq!(get(&mut it, "/o"), "<div><h1>t</h1><i>1</i></div>");
    assert_eq!(hits(&it, &mut h), 1, "the whole page comes from the cache");
    send(&mut it, "POST", "/inc", "");
    assert_eq!(get(&mut it, "/o"), "<div><h1>t</h1><i>2</i></div>");
    // Inner was cached on its own too: Outer's new render reuses it.
    send(&mut it, "POST", "/inc", "");
    assert_eq!(get(&mut it, "/o"), "<div><h1>t</h1><i>3</i></div>");
}

#[test]
fn a_new_function_in_a_global_starts_the_cache_afresh() {
    let mut it = app(
        r#"
let fmt = (x) => `${x}!`
view V() = <p>{fmt("a")}</p>
route GET /v = <V/>
route POST /f {
    fmt = (x) => `${x}?`
    "ok"
}
"#,
    );
    assert_eq!(get(&mut it, "/v"), "<p>a!</p>");
    assert_eq!(get(&mut it, "/v"), "<p>a!</p>");
    send(&mut it, "POST", "/f", "");
    assert_eq!(get(&mut it, "/v"), "<p>a?</p>");
}

#[test]
fn views_that_depend_on_anything_else_run_every_time() {
    let mut it = app(
        r#"
db.open(":memory:")
db.exec("create table t (n integer)")
view R() = <p>{random()}</p>
view U() = <p>{url().path}</p>
view D() = <p>{db.one("select count(*) as n from t").n}</p>
component C() {
    ~n = 0
    <button>{n}</button>
}
view WithC() = <div><C/></div>
route GET /r = <R/>
route GET /u/*rest = <U/>
route GET /d = <D/>
route GET /c = <WithC/>
route POST /add {
    db.exec("insert into t values (1)")
    "ok"
}
"#,
    );
    let mut h = 0;
    assert_ne!(get(&mut it, "/r"), get(&mut it, "/r"));
    assert_eq!(get(&mut it, "/u/x"), "<p>/u/x</p>");
    assert_eq!(get(&mut it, "/u/y"), "<p>/u/y</p>");
    assert_eq!(get(&mut it, "/d"), "<p>0</p>");
    send(&mut it, "POST", "/add", "");
    assert_eq!(get(&mut it, "/d"), "<p>1</p>");
    assert!(get(&mut it, "/c").contains("<quillo-island"));
    assert!(get(&mut it, "/c").contains("<quillo-island"));
    assert_eq!(hits(&it, &mut h), 0);
}
