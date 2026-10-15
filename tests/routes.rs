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
    todos = todos |> push({ text, urgent })
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
route GET /me = request |> auth.required |> (r) => { ip: r.ip }
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
    let user = users |> find((u) => u.id == id) ?? reply 404 { error: `no user ${id}` }
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
    let double = count * 2
    <div.counter>
        <span>{label}: {count} ({double})</span>
        <button @click={() => { count += 1 }}>+</button>
        <input bind:value={label}/>
    </div>
}
route GET / = <html><body><Counter start={2}/></body></html>
route GET /bare = <Counter/>
"#,
    );
    let r = get(&mut it, "/");
    let page = r.text();
    assert!(page.contains(r#"<quillo-island style="display:contents" data-c="Counter" data-p="{&quot;start&quot;:2}" data-s="{&quot;count&quot;:2}">"#), "{}", page);
    assert!(page.contains("<span>n: 2 (4)</span><button>+</button><input value=\"n\"></div></quillo-island>"), "{}", page);
    assert!(page.ends_with("<script src=\"/_quillo/quillo.js\" defer></script></body></html>"), "{}", page);
    // Without a </body>, the script goes at the end.
    assert!(get(&mut it, "/bare").text().ends_with("<script src=\"/_quillo/quillo.js\" defer></script>"));
    let js = get(&mut it, "/_quillo/quillo.js");
    assert_eq!(js.header("content-type"), Some("text/javascript; charset=utf-8"));
    assert!(js.text().contains("W.component(\"Counter\""));
}

#[test]
fn component_props_must_be_sendable() {
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
    todos = todos |> push({ text, done })
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
    assert!(js.contains("function boot(") && js.contains("function mount("));
    // Builtins come along only when used.
    assert!(!js.contains("B.group_by") && !js.contains("B.sort_by"));
    let mut big = app("server fn save(x) = x\ncomponent C() { ~xs = []\n<button @click={() => { xs = save(xs |> sort_by((x) => x)) }}></button> }\nroute GET / = <C/>\n");
    let js2 = get(&mut big, "/_quillo/quillo.js").text();
    assert!(js2.contains("async function rpc") && js2.contains("B.sort_by"));
    assert!(js.len() < js2.len() && js.len() < 12_000, "{} vs {}", js.len(), js2.len());
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
