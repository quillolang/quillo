use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str, ctype: Option<&str>, body: &str) -> HttpResponse {
    let mut headers = vec![("Host".to_string(), "localhost".to_string())];
    if let Some(c) = ctype {
        headers.push(("Content-Type".into(), c.into()));
    }
    it.handle(&HttpRequest {
        method: method.into(),
        url: url.into(),
        headers,
        body: body.as_bytes().to_vec(),
        remote: "127.0.0.1".into(),
    })
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    send(it, "GET", url, None, "")
}

fn post_json(it: &mut Interp, url: &str, body: &str) -> HttpResponse {
    send(it, "POST", url, Some("application/json"), body)
}

#[test]
fn values_choose_content_type() {
    let mut it = app(
        r#"
route GET / = <h1>Hello</h1>
route GET /text = "plain"
route GET /data = { ok: true, n: [1, 2] }
route GET /empty = null
"#,
    );
    let r = get(&mut it, "/");
    assert_eq!((r.status, r.header("content-type"), r.text().as_str()), (200, Some("text/html; charset=utf-8"), "<h1>Hello</h1>"));
    let r = get(&mut it, "/text");
    assert_eq!(r.header("content-type"), Some("text/plain; charset=utf-8"));
    let r = get(&mut it, "/data");
    assert_eq!((r.header("content-type"), r.text().as_str()), (Some("application/json"), r#"{"ok":true,"n":[1,2]}"#));
    assert_eq!(get(&mut it, "/empty").status, 204);
}

#[test]
fn path_params_are_typed() {
    let mut it = app(
        r#"
route GET /add/:a(int)/:b(int) = { sum: a + b }
route GET /hello/:name = `Hello, ${name}!`
route GET /files/*path = path
"#,
    );
    assert_eq!(get(&mut it, "/add/2/40").text(), r#"{"sum":42}"#);
    assert_eq!(get(&mut it, "/add/2/x").status, 404);
    assert_eq!(get(&mut it, "/hello/Ada%20L").text(), "Hello, Ada L!");
    assert_eq!(get(&mut it, "/files/a/b/c.txt").text(), "a/b/c.txt");
    assert_eq!(send(&mut it, "POST", "/hello/x", None, "").status, 405);
    assert_eq!(get(&mut it, "/nope").status, 404);
}

#[test]
fn query_patterns_coerce_and_default() {
    let mut it = app(r#"route GET /search ? { q: str, limit: int = 20 } = { q, limit }"#);
    assert_eq!(get(&mut it, "/search?q=quillo+lang&limit=5").text(), r#"{"q":"quillo lang","limit":5}"#);
    assert_eq!(get(&mut it, "/search?q=x").text(), r#"{"q":"x","limit":20}"#);
    assert_eq!(get(&mut it, "/search?q=x&limit=lots").status, 400);
    assert_eq!(get(&mut it, "/search").status, 400);
}

#[test]
fn payload_based_routing() {
    let mut it = app(
        r#"
route POST /orders { json: { type: "order" | "refund", customer.id: id } } {
    reply 201 { accepted: true, customer: id }
}
route POST /orders { json: { type: "quote", items: list } } = { quote: len(items) * 10 }
route POST /orders = reply 422 { error: "unknown order type" }
"#,
    );
    let r = post_json(&mut it, "/orders", r#"{"type":"refund","customer":{"id":7}}"#);
    assert_eq!((r.status, r.text().as_str()), (201, r#"{"accepted":true,"customer":7}"#));
    let r = post_json(&mut it, "/orders", r#"{"type":"quote","items":[1,2,3]}"#);
    assert_eq!(r.text(), r#"{"quote":30}"#);
    // JSON matching is strict: the string "7" is not an int.
    let r = post_json(&mut it, "/orders", r#"{"type":"quote","items":"nope"}"#);
    assert_eq!(r.status, 422);
}

#[test]
fn form_fields_and_redirects() {
    let mut it = app(
        r#"
let todos = []
route POST /todos { form: { text: str, urgent: bool = false } } {
    todos = todos | push({ text, urgent })
    redirect("/")
}
route GET /todos = todos
"#,
    );
    let r = send(&mut it, "POST", "/todos", Some("application/x-www-form-urlencoded"), "text=buy+milk&urgent=on");
    assert_eq!((r.status, r.header("location")), (303, Some("/")));
    send(&mut it, "POST", "/todos", Some("application/x-www-form-urlencoded"), "text=walk");
    assert_eq!(get(&mut it, "/todos").text(), r#"[{"text":"buy milk","urgent":true},{"text":"walk","urgent":false}]"#);
}

#[test]
fn reply_short_circuits_from_middleware() {
    let mut it = app(
        r#"
let auth = {
    required: (req) => req.headers.authorization == "Bearer secret"
        ? req
        : reply 401 { error: "unauthorized" }
}
route GET /me = request | auth.required | (r) => { ip: r.ip }
"#,
    );
    let r = get(&mut it, "/me");
    assert_eq!((r.status, r.text().as_str()), (401, r#"{"error":"unauthorized"}"#));
    let r = it.handle(&HttpRequest {
        method: "GET".into(),
        url: "/me".into(),
        headers: vec![("Authorization".into(), "Bearer secret".into())],
        body: vec![],
        remote: "10.0.0.1".into(),
    });
    assert_eq!((r.status, r.text().as_str()), (200, r#"{"ip":"10.0.0.1"}"#));
}

#[test]
fn not_found_with_coalesce() {
    let mut it = app(
        r#"
let users = [{ id: 1, name: "Ada" }]
route GET /users/:id(int) {
    let user = users | find((u) => u.id == id) ?? reply 404 { error: `no user ${id}` }
    user.name
}
"#,
    );
    assert_eq!(get(&mut it, "/users/1").text(), "Ada");
    let r = get(&mut it, "/users/2");
    assert_eq!((r.status, r.text().as_str()), (404, r#"{"error":"no user 2"}"#));
}

#[test]
fn runtime_errors_become_500() {
    // The message is shown in development (`--watch`); see tests/security.rs.
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut it = app(
        r#"
route GET /boom = 1 + null
route GET /err = err("nope")
"#,
    );
    let r = get(&mut it, "/boom");
    assert_eq!(r.status, 500);
    assert!(r.text().contains("cannot add int and null"), "{}", r.text());
    let r = get(&mut it, "/err");
    assert_eq!((r.status, r.text().as_str()), (500, r#"{"error":"nope"}"#));
}

#[test]
fn headers_cookies_and_respond() {
    let mut it = app(
        r#"
route GET /who { cookies: { session: str } } = respond(200, `session ${session}`, { "X-Quillo": "1" })
route GET /who = reply 401
"#,
    );
    let r = it.handle(&HttpRequest {
        method: "GET".into(),
        url: "/who".into(),
        headers: vec![("Cookie".into(), "theme=dark; session=abc".into())],
        body: vec![],
        remote: String::new(),
    });
    assert_eq!((r.status, r.text().as_str(), r.header("x-quillo")), (200, "session abc", Some("1")));
    assert_eq!(get(&mut it, "/who").status, 401);
}

#[test]
fn static_files_refuse_traversal() {
    let mut it = app(r#"route GET /static/*path = file("examples/public/" + path)"#);
    let r = get(&mut it, "/static/style.css");
    assert_eq!((r.status, r.header("content-type")), (200, Some("text/css; charset=utf-8")));
    assert_eq!(get(&mut it, "/static/../../Cargo.toml").status, 404);
    assert_eq!(get(&mut it, "/static/%2E%2E/%2E%2E/Cargo.toml").status, 404);
}

#[test]
fn head_requests_have_no_body() {
    let mut it = app(r#"route GET / = "hello""#);
    let r = send(&mut it, "HEAD", "/", None, "");
    assert_eq!((r.status, r.body.len()), (200, 0));
}

#[test]
fn components_render_on_the_server_inside_islands() {
    let mut it = app(
        r#"
component Counter(start: int = 0, label: str = "n") {
    ~count = start
    ~text = label
    let double = count * 2
    <div.counter>
        <span>{label}: {count} ({double})</span>
        <button @click={() => { count += 1 }}>+</button>
        <input bind:value={text}/>
    </div>
}
route GET / = <html><body><Counter start={2}/></body></html>
route GET /bare = <Counter/>
"#,
    );
    let r = get(&mut it, "/");
    let page = r.text();
    assert!(page.contains(r#"<quillo-island style="display:contents" data-c="Counter" data-p="{&quot;start&quot;:2}" data-s="{&quot;count&quot;:2,&quot;text&quot;:&quot;n&quot;}">"#), "{}", page);
    assert!(page.contains("<span>n: 2 (4)</span><button>+</button><input value=\"n\"></div></quillo-island>"), "{}", page);
    // The bundle loads when it is needed (a loader, last in the body).
    let tail = &page[page.rfind("<script").unwrap()..];
    assert!(tail.contains("t.src=\"/_quillo/b/") && tail.ends_with("</script></body></html>"), "{}", page);
    // Without a </body>, the loader goes at the end.
    let bare = get(&mut it, "/bare").text();
    assert!(bare.contains("t.src=\"/_quillo/b/") && bare.ends_with("})()</script>"), "{}", bare);
    let js = get(&mut it, "/_quillo/quillo.js");
    assert_eq!(js.header("content-type"), Some("text/javascript; charset=utf-8"));
    assert!(js.text().contains("W.component(\"Counter\""));
}

#[test]
fn component_props_must_be_sendable() {
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut it = app("component Box(f) = <p>x</p>\nroute GET / = <Box f={() => 1}/>\n");
    let r = get(&mut it, "/");
    assert_eq!(r.status, 500);
    assert!(r.text().contains("prop `f` of <Box> is a function, which cannot be sent to the browser"), "{}", r.text());
}

fn rpc(it: &mut Interp, name: &str, body: &str) -> HttpResponse {
    send(it, "POST", &format!("/_quillo/rpc/{}", name), Some("application/json"), body)
}

#[test]
fn server_fns_are_called_over_json() {
    let mut it = app(
        r#"
let todos = []
server fn add_todo(text: str, done: bool = false) -> list {
    todos = todos | push({ text, done })
    todos
}
server fn check(n: int) ! str = n > 0 ? n : err("must be positive")
server fn whoami() = request.headers["x-user"] ?? reply 401 { error: "who are you?" }
server fn crash() = 1 / 0
fn not_exposed() = "secret"
"#,
    );
    let r = rpc(&mut it, "add_todo", r#"["write docs"]"#);
    assert_eq!((r.status, r.text().as_str()), (200, r#"{"ok":[{"text":"write docs","done":false}]}"#));
    assert_eq!(rpc(&mut it, "check", "[5]").text(), r#"{"ok":5}"#);
    assert_eq!(rpc(&mut it, "check", "[-1]").text(), r#"{"err":"must be positive"}"#);
    let r = rpc(&mut it, "add_todo", "[42]");
    assert_eq!(r.status, 400);
    assert!(r.text().contains("argument `text` of `add_todo` must be str, got int"), "{}", r.text());
    assert_eq!(rpc(&mut it, "add_todo", "[]").status, 400);
    assert_eq!(rpc(&mut it, "add_todo", "[\"a\", true, 3]").status, 400);
    assert_eq!(rpc(&mut it, "add_todo", "{\"text\": 1}").status, 400);
    assert_eq!(rpc(&mut it, "not_exposed", "[]").status, 404);
    assert_eq!(rpc(&mut it, "nope", "[]").status, 404);
    assert_eq!(rpc(&mut it, "crash", "[]").status, 500);
    // `reply` inside a server fn becomes the HTTP response.
    let r = rpc(&mut it, "whoami", "[]");
    assert_eq!((r.status, r.text().as_str()), (401, r#"{"error":"who are you?"}"#));
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    headers.push(("X-User".into(), "ada".into()));
    let r = it.handle(&HttpRequest { method: "POST".into(), url: "/_quillo/rpc/whoami".into(), headers, body: b"[]".to_vec(), remote: "1.2.3.4".into() });
    assert_eq!(r.text(), r#"{"ok":"ada"}"#);
    // Only JSON POSTs: a cross-site form cannot call a server fn.
    assert_eq!(send(&mut it, "POST", "/_quillo/rpc/add_todo", Some("application/x-www-form-urlencoded"), "x=1").status, 415);
    assert_eq!(get(&mut it, "/_quillo/rpc/add_todo").status, 405);
}

#[test]
fn the_browser_bundle_only_carries_what_the_page_uses() {
    let mut small = app("component Hi(name: str) = <p>hi {name}</p>\nroute GET / = <Hi name=\"x\"/>\n");
    let js = get(&mut small, "/_quillo/quillo.js").text();
    // No server fns: no RPC code. Rendering and booting, of course.
    assert!(!js.contains("async function rpc"), "{}", js);
    assert!(js.contains("function boot(") && js.contains("function resume("));
    // Builtins come along only when used.
    assert!(!js.contains("B.group_by") && !js.contains("B.sort_by"));
    let mut big = app("server fn save(x) = x\ncomponent C() { ~xs = []\n<button @click={() => { xs = save(xs | sort_by((x) => x)) }}></button> }\nroute GET / = <C/>\n");
    let js2 = get(&mut big, "/_quillo/quillo.js").text();
    assert!(js2.contains("async function rpc") && js2.contains("B.sort_by"));
    // About 29 KB (9 KB compressed) since markup is compiled into blocks,
    // keyed lists move only what changed and clicks are handled once; the
    // checks on the page's own objects (events' elements) are in it too,
    // what keeps typing and clicks from before the bundle came, and what
    // resumes the server's page instead of making it again (half a KB), and
    // decimals and ints past 2^53, which any value from the server may be
    // (3.5 KB).
    assert!(js.len() < js2.len() && js.len() < 30_000, "{} vs {}", js.len(), js2.len());
    // No comments or indentation.
    assert!(!js.lines().any(|l| l.starts_with(' ') || l.trim_start().starts_with("//")));
}

#[test]
fn the_browser_bundle_can_be_revalidated() {
    let mut it = app("component Hi() = <p>hi</p>\nroute GET / = <Hi/>\n");
    let r = get(&mut it, "/_quillo/quillo.js");
    let etag = r.header("etag").unwrap().to_string();
    let again = it.handle(&HttpRequest {
        method: "GET".into(),
        url: "/_quillo/quillo.js".into(),
        headers: vec![("If-None-Match".into(), etag)],
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    });
    assert_eq!((again.status, again.body.len()), (304, 0));
}

fn live_rpc(it: &mut Interp, name: &str, body: &str) -> serde_json::Value {
    let r = it.handle(&HttpRequest {
        method: "POST".into(),
        url: format!("/_quillo/rpc/{}", name),
        headers: vec![("Content-Type".into(), "application/json".into()), ("X-Quillo-Live".into(), "1".into())],
        body: body.as_bytes().to_vec(),
        remote: "127.0.0.1".into(),
    });
    serde_json::from_str(&r.text()).unwrap()
}

fn changed_since(it: &mut Interp, since: u64, topics: &str) -> serde_json::Value {
    serde_json::from_str(&get(it, &format!("/_quillo/live?since={}&t={}", since, topics)).text()).unwrap()
}

#[test]
fn live_loaders_report_what_they_read_and_writes_announce_changes() {
    let mut it = app(
        r#"
db.exec("create table notes (id integer primary key, text text)")
let visits = 0
server fn load_notes() = db.query("select n.text from notes n")
server fn count_visits() = visits
server fn add_note(text: str) { db.exec("insert into notes (text) values (?)", text) }
server fn visit() { visits += 1 }
server fn custom() { watch("prices") }
server fn tx_fail() = db.transaction(() => {
    db.exec("insert into notes (text) values ('x')")
    err("no")
})
"#,
    );
    let r = live_rpc(&mut it, "load_notes", "[]");
    assert_eq!((r["ok"].clone(), r["topics"].clone()), (serde_json::json!([]), serde_json::json!(["db:notes"])));
    assert_eq!(live_rpc(&mut it, "count_visits", "[]")["topics"], serde_json::json!(["var:visits"]));
    assert_eq!(live_rpc(&mut it, "custom", "[]")["topics"], serde_json::json!(["prices"]));
    // Without the header, no topics are reported.
    assert!(rpc(&mut it, "load_notes", "[]").text() == r#"{"ok":[]}"#);

    let seq = changed_since(&mut it, 0, "")["seq"].as_u64().unwrap();
    rpc(&mut it, "add_note", r#"["hi"]"#);
    assert_eq!(changed_since(&mut it, seq, "db:notes,var:visits")["changed"], serde_json::json!(["db:notes"]));
    rpc(&mut it, "visit", "[]");
    assert_eq!(changed_since(&mut it, seq, "var:visits")["changed"], serde_json::json!(["var:visits"]));
    // A rolled-back transaction announces nothing.
    let seq = changed_since(&mut it, 0, "")["seq"].as_u64().unwrap();
    rpc(&mut it, "tx_fail", "[]");
    assert_eq!(changed_since(&mut it, seq, "db:notes")["changed"], serde_json::json!([]));
}

#[test]
fn live_state_is_rendered_with_its_subscription() {
    let mut it = app(
        r#"
let items = ["a"]
server fn load(prefix: str) = items | map((i) => prefix + i)
component List(p: str) {
    ~xs = live(load, p)
    <ul>{for x in xs => <li>{x}</li>}</ul>
}
route GET / = <List p="> "/>
"#,
    );
    let page = get(&mut it, "/").text();
    assert!(page.contains(r#"data-l="{&quot;xs&quot;:{&quot;f&quot;:&quot;load&quot;,&quot;a&quot;:[&quot;&gt; &quot;],&quot;t&quot;:[&quot;var:items&quot;]}}""#), "{}", page);
    assert!(page.contains("<li>&gt; a</li>"), "{}", page);
    let js = get(&mut it, "/_quillo/quillo.js").text();
    assert!(js.contains(r#"W.live($i, "xs", "load", () => [$p.p])"#), "{}", js);
}

#[test]
fn every_path_of_a_single_page_app_renders_on_the_server() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/spa.qwe")).unwrap();
    let mut it = app(&src);
    for (path, status, heading) in [
        ("/", 200, "Welcome"),
        ("/products", 200, "Products"),
        ("/products/3", 200, "Fountain pen"),
        ("/about", 200, "About"),
        ("/cart", 200, "Cart"),
        ("/products/99", 404, "Not found"),
        ("/products/abc", 404, "Not found"),
        ("/nope", 404, "Not found"),
    ] {
        let r = get(&mut it, path);
        assert_eq!(r.status, status, "{}", path);
        assert!(r.text().contains(&format!("<h2>{}</h2>", heading)), "{}: {}", path, r.text());
    }
}

#[test]
fn mounted_programs_live_under_their_prefix() {
    let dir = std::env::temp_dir().join(format!("quillo-mount-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("shop")).unwrap();
    std::fs::write(
        dir.join("shop/app.qwe"),
        r#"
db.exec("create table t (n int)")
server fn add(n: int) = db.exec("insert into t values (?)", n)
component Box() = <a href="/inside">in</a>
route GET / = <html><body><a href="/items?x=1">items</a><a href="//cdn.example/x.js">cdn</a><form action="/buy"></form><Box/></body></html>
route POST /buy = redirect("/thanks")
route GET /where = url().path
"#,
    )
    .unwrap();
    std::fs::write(dir.join("main.qwe"), "mount /shop = \"shop/app.qwe\"\nroute GET / = \"home\"\n").unwrap();
    let mut it = Interp::new(&dir);
    it.load_file(&dir.join("main.qwe")).unwrap();
    assert_eq!(get(&mut it, "/").text(), "home");
    let page = get(&mut it, "/shop").text();
    assert!(page.contains(r#"<a href="/shop/items?x=1">"#) && page.contains(r#"href="//cdn.example/x.js""#), "{}", page);
    assert!(page.contains(r#"action="/shop/buy""#) && page.contains(r#"<a href="/shop/inside">"#), "{}", page);
    let at = page.find(r#"t.src="/shop/_quillo/b/"#).expect(&page) + r#"t.src=""#.len();
    let src = page[at..at + page[at..].find('"').unwrap()].to_string();
    assert!(get(&mut it, &src).text().contains("BASE = \"/shop\""));
    assert!(get(&mut it, "/shop/_quillo/quillo.js").text().contains("BASE = \"/shop\""));
    assert_eq!(get(&mut it, "/shop/where").text(), "/where");
    let r = send(&mut it, "POST", "/shop/buy", None, "");
    assert_eq!((r.status, r.header("location")), (303, Some("/shop/thanks")));
    assert_eq!(rpc_at(&mut it, "/shop/_quillo/rpc/add", "[1]").status, 200);
    assert_eq!(get(&mut it, "/shopping").status, 404);
    assert_eq!(get(&mut it, "/elsewhere").status, 404);
}

fn rpc_at(it: &mut Interp, url: &str, body: &str) -> HttpResponse {
    send(it, "POST", url, Some("application/json"), body)
}

#[test]
fn routes_that_do_not_name_request_still_answer_what_asks_for_it() {
    // The request map is made only when something asks for it.
    let mut it = app(
        "server fn who() = request.path\n\
         route GET /u/:id = url().path\n\
         route GET /w = who()\n\
         route GET /c = (() => request.path)()\n\
         route GET /p/:id = { id, n: len(keys(request.params)) }\n",
    );
    assert_eq!(get(&mut it, "/u/7").text(), "/u/7");
    assert_eq!(get(&mut it, "/w").text(), "/w");
    assert_eq!(get(&mut it, "/c").text(), "/c");
    assert_eq!(get(&mut it, "/p/3").text(), r#"{"id":"3","n":1}"#);
}

/// A page loads the code of the components on it (and what they use), not
/// the whole program's; a page without any loads none.
#[test]
fn pages_load_only_their_own_components_code() {
    let mut it = app(r#"
fn shout(s) = upper(s)
component Counter() {
    ~n = 0
    <button @click={() => { n = n + 1 }}>{shout("count")} {n}</button>
}
component Chart() {
    ~points = [1, 2, 3]
    <ul>{for p in points => <li>{p}</li>}</ul>
}
route GET /counter = <html><body><Counter/></body></html>
route GET /both = <html><body><Counter/><Chart/></body></html>
route GET /plain = <html><body><p>No components</p></body></html>
"#);
    let page = get(&mut it, "/counter").text();
    let at = page.find("/_quillo/b/").expect(&page);
    let src = &page[at..at + page[at..].find('"').unwrap()];
    let r = get(&mut it, src);
    assert!(r.header("cache-control").unwrap().contains("immutable"));
    let js = r.text();
    assert!(js.contains("Counter") && js.contains("upper") && !js.contains("Chart"), "only Counter and what it uses");
    // Another set of components, another bundle; the same set, the same one.
    let both = get(&mut it, "/both").text();
    assert!(!both.contains(src) && both.contains("/_quillo/b/"));
    assert!(get(&mut it, "/counter").text().contains(src));
    assert!(!get(&mut it, "/plain").text().contains("/_quillo/"));
    // A bundle this program did not make (a page from before a reload): the whole one.
    let old = get(&mut it, "/_quillo/b/0000000000000000.js");
    assert!(old.text().contains("Chart") && old.text().contains("Counter") && old.header("cache-control") == Some("no-cache"));
}

/// A function first compiled inside a loop's body (its first use in the
/// page's bundle) is not something the loop reads from around it: its own
/// parameters stay its own.
#[test]
fn a_function_first_used_inside_a_loop_keeps_its_names() {
    let mut it = app(r#"
fn tidy(zzparam) = upper(zzparam)
component List() {
    ~items = ["a", "b"]
    ~picked = ""
    let mark = "-"
    <ul>{for x in items => <li key={x} @click={() => { picked = tidy(x) }}>{mark}{x}</li>}</ul>
}
route GET / = <html><body><List/></body></html>
"#);
    let page = get(&mut it, "/").text();
    let at = page.find("/_quillo/b/").expect(&page);
    let js = get(&mut it, &page[at..at + page[at..].find('"').unwrap()]).text();
    let rows = js.lines().find(|l| l.contains("W.each(")).expect("the loop follows its list");
    assert!(!rows.contains("zzparam"), "{}", rows);
}

/// Prefetching (src/server.rs, `PREFETCH_JS`): every page carries what asks
/// for the pages it links to, and a page asked for ahead of a click is made
/// without changing anything: one that would is not prefetched.
#[test]
fn pages_are_prefetched_without_changing_anything() {
    let mut it = app(r#"
db.open(":memory:")
db.exec("create table hits (n int)")
let views = 0
route GET / = <html><body><a href="/read">read</a></body></html>
route GET /fragment = <p>no page</p>
route GET /read = <html><body>{len(db.query("select * from hits"))} {views}</body></html>
route GET /write {
    db.exec("insert into hits values (1)")
    <html><body>written</body></html>
}
route GET /returning = str(db.one("insert into hits values (2) returning n") ?? "none")
route GET /count {
    views += 1
    str(views)
}
route GET /login {
    set_session({ user: 1 })
    "in"
}
route GET /cached = respond(200, "mine", { "cache-control": "no-store" })
"#);
    let prefetch = |it: &mut Interp, url: &str, header: (&str, &str)| {
        it.handle(&HttpRequest {
            method: "GET".into(),
            url: url.into(),
            headers: vec![("Host".into(), "localhost".into()), (header.0.into(), header.1.into())],
            body: Vec::new(),
            remote: "127.0.0.1".into(),
        })
    };
    // Pages carry it (once); a fragment of HTML does not.
    assert_eq!(get(&mut it, "/").text().matches("__quillo_pf=1").count(), 1);
    assert!(!get(&mut it, "/fragment").text().contains("__quillo_pf"));
    // Reading: prefetched, and kept by the browser for the click.
    let r = prefetch(&mut it, "/read", ("X-Quillo-Prefetch", "1"));
    assert_eq!((r.status, r.header("cache-control")), (200, Some("private, max-age=30")));
    // Writing (a database, SQL that writes, a global, a session): not prefetched.
    for url in ["/write", "/returning", "/count", "/login"] {
        for h in [("X-Quillo-Prefetch", "1"), ("Sec-Purpose", "prefetch;prerender")] {
            let r = prefetch(&mut it, url, h);
            assert_eq!((r.status, r.header("cache-control")), (204, Some("no-store")), "{} {:?}", url, h);
        }
    }
    assert!(get(&mut it, "/read").text().starts_with("<!doctype html><html><body>0 0"), "nothing changed");
    // Followed, they do what they do.
    assert!(get(&mut it, "/write").text().contains("written"));
    assert_eq!(get(&mut it, "/count").text(), "1");
    assert!(get(&mut it, "/login").header("set-cookie").is_some());
    // What the program says of caching stays as it says.
    assert_eq!(prefetch(&mut it, "/cached", ("X-Quillo-Prefetch", "1")).header("cache-control"), Some("no-store"));
}
