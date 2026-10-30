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

/// `routes /prefix | middleware { pattern } { ... }`: each route takes the
/// group's prefix, its middleware (a `reply` or a response from one answers in
/// the route's place) and its request pattern; groups nest (branch route-groups).
#[test]
fn route_groups_take_prefix_middleware_and_pattern() {
    let mut it = app(
        r#"
let seen = []
fn note(r) { seen = seen | push(r.path) }
fn signed(r) = r.headers.authorization == "Bearer ok" ? r : reply 401 "sign in first"
fn admins(r) = r.headers["x-role"] == "admin" ? null : redirect("/login")
route GET / = "home"
routes /admin | note | signed | admins {
    route GET / = "admin home"
    route GET /users = ["ana"]
    routes /teams/:team(int) | note {
        route GET /members = { team }
    }
}
routes /api { headers: { host: "api.example.com" } } {
    route GET /me = "the api"
}
route GET /api/me = "not the api's host"
route GET /seen = seen
let routes = 10
fn half() { routes / 2 }
route GET /half = half()
"#,
    );
    let with = |it: &mut Interp, url: &str, hs: &[(&str, &str)]| {
        let mut headers: Vec<(String, String)> = vec![("Host".into(), "localhost".into())];
        headers.extend(hs.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers, body: vec![], remote: "127.0.0.1".into() })
    };
    assert_eq!(get(&mut it, "/").text(), "home");
    assert_eq!(get(&mut it, "/admin/users").status, 401);
    assert_eq!(with(&mut it, "/admin/users", &[("Authorization", "Bearer ok")]).status, 303);
    let ok = [("Authorization", "Bearer ok"), ("X-Role", "admin")];
    assert_eq!(with(&mut it, "/admin/users", &ok).text(), r#"["ana"]"#);
    assert_eq!(with(&mut it, "/admin", &ok).text(), "admin home");
    assert_eq!(with(&mut it, "/admin/teams/3/members", &ok).text(), r#"{"team":3}"#);
    assert_eq!(with(&mut it, "/admin/teams/x/members", &ok).status, 404);
    assert_eq!(get(&mut it, "/seen").text(), r#"["/admin/users","/admin/users","/admin/users","/admin","/admin/teams/3/members","/admin/teams/3/members"]"#);
    assert_eq!(with(&mut it, "/api/me", &[]).text(), "not the api's host");
    let api = it.handle(&HttpRequest { method: "GET".into(), url: "/api/me".into(), headers: vec![("Host".into(), "api.example.com".into())], body: vec![], remote: "127.0.0.1".into() });
    assert_eq!(api.text(), "the api");
    assert_eq!(get(&mut it, "/half").text(), "5");
}

#[test]
fn route_groups_are_checked() {
    let msg = |src: &str| Interp::new(env!("CARGO_MANIFEST_DIR")).load_source(src, "test.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert!(msg("fn f() {\n    routes /x {\n        route GET /a = 1\n    }\n}\n").contains("top level"));
    assert!(msg("routes /x {\n}\n").contains("no routes"));
    assert!(msg("routes /x {\n    let a = 1\n}\n").contains("holds `route` and `routes`"));
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
    let mut it = app(r#"route GET /static/*path = file("tests/programs/public/" + path)"#);
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
    // The runtime file first, then the page's own (FEEDBACK7.md, Q-064;
    // branch feedback7-fixes).
    let tail = &page[page.rfind("<script").unwrap()..];
    let srcs = scripts_of(tail);
    assert!(srcs[0].starts_with("/_quillo/r/") && srcs[1].starts_with("/_quillo/b/") && tail.ends_with("</script></body></html>"), "{}", page);
    // Without a </body>, the loader goes at the end.
    let bare = get(&mut it, "/bare").text();
    assert!(scripts_of(&bare)[1].starts_with("/_quillo/b/") && bare.ends_with("})()</script>"), "{}", bare);
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
    // (3.5 KB), and the checks on what is handed to the page's functions
    // (an event's element's methods, say: 400 bytes), and components that
    // are a table's rows (800 bytes), and a textarea that follows the text
    // between its tags (200 bytes), and fields the browser corrected told
    // from typing, components inside others that run by themselves started,
    // and callbacks the page calls with more arguments than they take
    // (500 bytes: FEEDBACK5.md, Q-046, Q-048, Q-050; branch stays-fixes);
    // what says why a page was made again is in development bundles only,
    // and newlines that change nothing go; an island inside another whose
    // state no longer fits is made again by itself, the others resumed
    // (250 bytes: Q-014; branch feedback6-fixes). No fixed limit in bytes any
    // more: what is asked is that a page carries less than one that uses more
    // (branch open-fixes).
    assert!(js.len() < js2.len(), "{} vs {}", js.len(), js2.len());
    // No comments or indentation.
    assert!(!js.lines().any(|l| l.starts_with(' ') || l.trim_start().starts_with("//")));
}

/// The scripts a page loads, in order.
fn scripts_of(page: &str) -> Vec<String> {
    let at = page.find("for(const s of ").map(|i| i + "for(const s of ".len()).expect(page);
    let end = page[at..].find(']').unwrap() + at + 1;
    serde_json::from_str(&page[at..end]).unwrap()
}

/// Ten optional widgets: the page at /0110000001 shows W1, W2 and W9.
const WIDGETS: &str = r#"
component W(n: int) {
    state c = 0
    <button @click={() => { c += 1 }}>widget {n}: {c}</button>
}
component W0() = <W n={0}/>
component W1() = <W n={1}/>
component W2() = <W n={2}/>
component W3() = <W n={3}/>
component W4() = <W n={4}/>
component W5() = <W n={5}/>
component W6() = <W n={6}/>
component W7() = <W n={7}/>
component W8() = <W n={8}/>
component W9() = <W n={9}/>
fn on(bits: str, i: int) = slice(bits, i, i + 1) == "1"
route GET /:bits = <html><body>
    {on(bits, 0) ? <W0/> : null}{on(bits, 1) ? <W1/> : null}{on(bits, 2) ? <W2/> : null}{on(bits, 3) ? <W3/> : null}{on(bits, 4) ? <W4/> : null}
    {on(bits, 5) ? <W5/> : null}{on(bits, 6) ? <W6/> : null}{on(bits, 7) ? <W7/> : null}{on(bits, 8) ? <W8/> : null}{on(bits, 9) ? <W9/> : null}
</body></html>
"#;

/// The runtime is a file every page shares, and a page's bundle is its
/// own code (FEEDBACK7.md, Q-064; branch feedback7-fixes).
#[test]
fn pages_share_the_runtime_file() {
    let mut it = app(WIDGETS);
    let one = scripts_of(&get(&mut it, "/1000000000").text());
    let other = scripts_of(&get(&mut it, "/0100000000").text());
    assert_eq!(one.len(), 2, "{:?}", one);
    assert!(one[0].starts_with("/_quillo/r/") && one[1].starts_with("/_quillo/b/"), "{:?}", one);
    assert_eq!(one[0], other[0]);
    assert_ne!(one[1], other[1]);
    let rt = get(&mut it, &one[0]);
    assert_eq!(rt.header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert!(rt.text().contains("window.__quillo_rt"));
    let b = get(&mut it, &one[1]).text();
    assert!(b.len() < 3_000 && b.contains("widget ") && b.contains("window.__quillo_rt"), "{} bytes", b.len());
}

/// However many sets of components pages are made of, the server keeps a
/// bounded number of bundles; a page naming one no longer kept is sent
/// the whole program's (FEEDBACK7.md, Q-063; branch feedback7-fixes).
#[test]
fn bundles_kept_are_bounded() {
    let mut it = app(WIDGETS);
    let first = scripts_of(&get(&mut it, "/1000000000").text()).pop().unwrap();
    for n in 1..1024u32 {
        get(&mut it, &format!("/{:010b}", n));
    }
    assert!(it.bundles.lock().unwrap().len() <= quillo::client::KEPT);
    let again = get(&mut it, &first);
    assert_eq!((again.status, again.header("cache-control")), (200, Some("no-cache")));
    assert!(again.text().contains("widget "));
}

/// `/_quillo/ready` says whether what the program needs is there: its
/// database, and the checks it adds (FEEDBACK10.md, Q-076; branch
/// feedback10-fixes).
#[test]
fn ready_says_what_the_program_needs() {
    let mut it = app(
        r#"
db.open(":memory:")
let balanced = true
ready("ledger", () => balanced)
server fn unbalance() { balanced = false }
route GET / = "home"
"#,
    );
    let r = get(&mut it, "/_quillo/ready");
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(serde_json::from_str::<serde_json::Value>(&r.text()).unwrap(), serde_json::json!({ "ready": true, "checks": { "database": "ready", "ledger": "ready" } }));
    rpc(&mut it, "unbalance", "[]");
    let r = get(&mut it, "/_quillo/ready");
    assert_eq!((r.status, r.header("cache-control")), (503, Some("no-store")));
    assert!(r.text().contains(r#""ledger":"not ready""#), "{}", r.text());
}

/// Tailwind's variants and fractions in the shorthand: a `:` was taken for
/// an attribute, silently (FEEDBACK11.md, Q-079; branch shorthand-colon).
#[test]
fn shorthand_classes_take_variants_and_fractions() {
    let mut it = app("route GET / = <div.md:flex.w-1/2><em.hover:underline>a</em><i.w-1/3/><b.x :y>b</b></div>\n");
    let page = get(&mut it, "/").text();
    assert!(page.contains(r#"<div class="md:flex w-1/2"><em class="hover:underline">a</em><i class="w-1/3"></i>"#), "{}", page);
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

fn live_as(it: &mut Interp, name: &str, viewer: &str, had: Option<&serde_json::Value>) -> serde_json::Value {
    let mut headers = vec![
        ("Host".to_string(), "localhost".to_string()),
        ("Content-Type".into(), "application/json".into()),
        ("X-Quillo-Live".into(), "1".into()),
        ("Cookie".into(), format!("who={}", viewer)),
    ];
    if let Some(h) = had {
        headers.push(("X-Quillo-Had".into(), h.as_str().unwrap().into()));
    }
    let r = it.handle(&HttpRequest { method: "POST".into(), url: format!("/_quillo/rpc/{}", name), headers, body: b"[]".to_vec(), remote: "127.0.0.1:50000".into() });
    serde_json::from_str(&r.text()).unwrap()
}

/// One change wakes every page, and they all ask: one run serves them, and
/// each is sent what changed (FEEDBACK2.md, Q-028; branch feedback6-fixes).
#[test]
fn live_runs_are_shared_and_answers_are_patches() {
    let mut it = app(
        r#"
let rows = for i in range(500) => { id: i, title: `Row ${i}` }
let count = 0
server fn load_rows() with random = { stamp: random(), rows }
server fn mine() with random = { stamp: random(), who: request.headers["cookie"] }
server fn counted() with random { count += 1; random() }
server fn rename(id: int) { rows = for r in rows => r.id == id ? merge(r, { title: "Renamed" }) : r }
"#,
    );
    // Not the viewer's: one run for both.
    let a = live_as(&mut it, "load_rows", "a", None);
    let b = live_as(&mut it, "load_rows", "b", None);
    assert_eq!(a["ok"]["stamp"], b["ok"]["stamp"]);
    assert_eq!(a["h"], b["h"]);
    // The viewer's: one run each, and the same viewer's pages share theirs.
    let ma = live_as(&mut it, "mine", "a", None);
    let mb = live_as(&mut it, "mine", "b", None);
    assert_ne!(ma["ok"]["stamp"], mb["ok"]["stamp"]);
    assert_eq!(live_as(&mut it, "mine", "a", None)["ok"]["stamp"], ma["ok"]["stamp"]);
    // A run that writes is never shared.
    assert_ne!(live_as(&mut it, "counted", "a", None)["ok"], live_as(&mut it, "counted", "b", None)["ok"]);
    // After a change, a page that says what it has is sent a patch to it.
    rpc(&mut it, "rename", "[7]");
    let p = live_as(&mut it, "load_rows", "a", Some(&a["h"]));
    assert!(p.get("ok").is_none() && p["patch"].to_string().len() < 200, "{}", p);
    let whole = live_as(&mut it, "load_rows", "c", None);
    assert_eq!(quillo::livecache::apply(&a["ok"], &p["patch"]), whole["ok"]);
    assert_eq!(whole["ok"]["rows"][7]["title"], "Renamed");
    assert_eq!(p["h"], whole["h"]);
    // The same value again: "the same".
    assert_eq!(live_as(&mut it, "load_rows", "a", Some(&p["h"]))["patch"], serde_json::json!({ "$=": 1 }));
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
    // With the value's hash, which the first refresh may be a patch to
    // (FEEDBACK2.md, Q-028; branch feedback6-fixes).
    assert!(page.contains(r#"data-l="{&quot;xs&quot;:{&quot;f&quot;:&quot;load&quot;,&quot;a&quot;:[&quot;&gt; &quot;],&quot;t&quot;:[&quot;var:items&quot;],&quot;h&quot;:&quot;"#), "{}", page);
    assert!(page.contains("<li>&gt; a</li>"), "{}", page);
    let js = get(&mut it, "/_quillo/quillo.js").text();
    assert!(js.contains(r#"W.live($i, "xs", "load", () => [$p.p])"#), "{}", js);
}

#[test]
fn every_path_of_a_single_page_app_renders_on_the_server() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/programs/spa.qwe")).unwrap();
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
    // `BASE` is set in the runtime file the page's bundle takes (Q-064).
    let srcs = scripts_of(&page);
    assert!(srcs[0].starts_with("/shop/_quillo/r/") && srcs[1].starts_with("/shop/_quillo/b/"), "{:?}", srcs);
    assert!(get(&mut it, &srcs[0]).text().contains("BASE = \"/shop\""));
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
    // `tidy` is defined before the component, not in its loop.
    let rows = &js[js.find("W.each(").expect("the loop follows its list")..];
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
            assert_eq!((r.status, r.header("cache-control")), (503, Some("no-store")), "{} {:?}", url, h);
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

/// A run-time error names the file its line is in, as `quillo check` does:
/// a program of several files has many a line 3.
#[test]
fn errors_name_the_file_of_their_line() {
    let d = std::env::temp_dir().join(format!("quillo-error-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("ui")).unwrap();
    std::fs::write(d.join("ui/cards.qwe"), "view TaskCard(code: str, title: str) = <li>{code}: {title}</li>\nfn days(xs) = xs.day\nfn worse(x) ! str = err(42)\n").unwrap();
    std::fs::write(
        d.join("main.qwe"),
        "use \"ui/cards.qwe\"\nfn data() = json.parse(\"[1]\")\nroute GET /a = <ul><TaskCard code={data()[0]} title=\"t\"/></ul>\n\
         route GET /b = days(data())\nroute GET /c = data().nope\nroute GET /d = worse(1)\nroute GET /e = data() | map((x) => x.day)\n",
    )
    .unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&d.join("main.qwe")).unwrap();
    // The message, for the program's author, is in the answer with --watch.
    quillo::server::SHOW_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
    let cases = [
        // The argument is wrong where the call is.
        ("/a", "main.qwe:3: argument `code` of `TaskCard`"),
        ("/b", "cards.qwe:2: list has no field `day`"),
        ("/c", "main.qwe:5: list has no field `nope`"),
        ("/d", "cards.qwe:3: `worse` declares errors"),
        ("/e", "main.qwe:7: int has no field `day`"),
    ];
    for (path, want) in cases {
        let r = get(&mut it, path);
        let body = String::from_utf8_lossy(&r.body).to_string();
        assert!(r.status == 500 && body.contains(want), "{}: {}", path, body);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `cookie(name)` reads the request's cookie from any function it runs.
#[test]
fn cookie_reads_the_requests_cookie_anywhere() {
    let mut it = app("fn zone() = cookie(\"tz\") ?? \"UTC\"\nroute GET /tz = zone()\n");
    let r = it.handle(&HttpRequest {
        method: "GET".into(),
        url: "/tz".into(),
        headers: vec![("Host".into(), "localhost".into()), ("Cookie".into(), "a=1; tz=Asia/Manila".into())],
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    });
    assert_eq!(String::from_utf8_lossy(&r.body), "Asia/Manila");
    assert_eq!(String::from_utf8_lossy(&get(&mut it, "/tz").body), "UTC");
}

/// `state x = defer(load())`: the page first with the state null and the island
/// waiting; then the component again, its other states as they were and the deferred
/// one worked out, as a template the page swaps in. Whole here, with no server to
/// stream it (branch defer).
#[test]
fn deferred_state_follows_the_page() {
    let mut it = app(r#"
let calls = { fast: 0, slow: 0 }
server fn fast() { calls = { ...calls, fast: calls.fast + 1 }; "fast" }
server fn slow() { calls = { ...calls, slow: calls.slow + 1 }; ["a", "b"] }

component Inner() {
    state x = defer(slow())
    <i id="inner">{x == null ? "none" : len(x)}</i>
}

component Page() {
    state f = fast()
    state s = defer(slow())
    <div>
        <b>{f}</b>
        {s == null ? <p id="wait">waiting</p> : <ul>{for v in s => <li>{v}</li>}</ul>}
        <Inner/>
    </div>
}
route GET / = <html><body><Page/><footer>end</footer></body></html>
route GET /calls = calls
"#);
    let r = get(&mut it, "/");
    let body = String::from_utf8_lossy(&r.body).to_string();
    // The page: waiting, the placeholder shown, the swap defined, the end of the page last.
    let island = body.find("data-qd=\"0\"").expect("the island waits");
    assert!(body[island..].contains("id=\"wait\""), "{}", body);
    assert!(body.contains("window.__quillo_swap"), "{}", body);
    // A component inside another does not defer: worked out in place.
    assert!(body.contains("<i id=\"inner\">2</i>"), "{}", body);
    // Its part: the component again, with the list.
    let part = body.find("<template data-qt=\"0\">").expect("the part");
    assert!(part > island);
    assert!(body[part..].contains("<li>a</li><li>b</li>") && body[part..].contains("__quillo_swap(\"0\")"), "{}", body);
    assert!(!body[part..].contains("data-qd"), "the part does not wait: {}", body);
    assert!(body.trim_end().ends_with("</html>") && body.rfind("<footer>end</footer>").unwrap() < part, "{}", body);
    // The page's own fast state ran once. The slow one: once deferred, and inside
    // twice, as a component inside a deferring one renders again with it.
    let calls = String::from_utf8_lossy(&get(&mut it, "/calls").body).to_string();
    assert_eq!(calls, r#"{"fast":1,"slow":3}"#);
}

/// `use Chart with preload from ...` and `with preconnect` put a `<link>` in the head
/// of the pages that can load the library, and no others (branch library-hints).
#[test]
fn a_page_gets_ready_for_the_libraries_named_with_a_hint() {
    let mut it = app(r#"
use Chart with preload from "https://cdn.example/chart.umd.js?v=1&x=2"
use L as Map, Draw with preconnect from https://tiles.example:8443/leaflet.js
use Other with preconnect from "//tiles.example:8443/other.js"
use Local with preload from assets/local.js
use Quiet from "https://quiet.example/quiet.js"
component Sales() {
    state chart = null
    <canvas @mount={(el) => { chart = Chart.new(el, {}) }}></canvas>
}
component Where() {
    <div @mount={(el) => { Map.map(el); Draw.go(); Other.go() }}></div>
}
component Own() {
    <div @mount={(el) => { Local.go(el) }}></div>
}
component Still() {
    <div @mount={(el) => { Quiet.go(el) }}></div>
}
route GET /sales = <html><head><title>s</title></head><body><Sales/></body></html>
route GET /where = <html><head><title>w</title></head><body><Where/></body></html>
route GET /own = <html><head><title>o</title></head><body><Own/></body></html>
route GET /still = <html><head><title>q</title></head><body><Still/></body></html>
route GET /plain = <html><head><title>p</title></head><body><p>no library</p></body></html>
"#);
    let page = |it: &mut _, p: &str| String::from_utf8_lossy(&get(it, p).body).to_string();
    let head = |page: &str| page[..page.find("</head>").expect("a head")].to_string();
    // The script's address, escaped; no `crossorigin`, which the loader's `<script>`
    // does not have: the browser would fetch it twice.
    let sales = page(&mut it, "/sales");
    assert!(head(&sales).contains(r#"<link rel="preload" as="script" href="https://cdn.example/chart.umd.js?v=1&amp;x=2">"#), "{}", sales);
    assert!(!sales.contains("rel=\"preconnect\""), "{}", sales);
    // Its host, once for all the libraries from there.
    let place = page(&mut it, "/where");
    assert!(head(&place).contains(r#"<link rel="preconnect" href="https://tiles.example:8443">"#), "{}", place);
    assert!(head(&place).contains(r#"<link rel="preconnect" href="//tiles.example:8443">"#), "{}", place);
    assert!(!place.contains("rel=\"preload\""), "{}", place);
    assert_eq!(place.matches(r#"href="https://tiles.example:8443""#).count(), 1, "{}", place);
    let own = page(&mut it, "/own");
    assert!(head(&own).contains(r#"<link rel="preload" as="script" href="/assets/local.js">"#), "{}", own);
    // No hint, or no library on the page: as before.
    for p in ["/still", "/plain"] {
        let got = page(&mut it, p);
        assert!(!got.contains("rel=\"preload\"") && !got.contains("rel=\"preconnect\""), "{}", got);
    }
}

/// An ES module is no `<script>`: its bundle imports it, a preload is a
/// `modulepreload`, and a connection is made for CORS (branch library-hints).
#[test]
fn a_page_imports_the_modules_its_components_use() {
    let mut it = app(r#"
use Chart with module, preload from "https://cdn.example/chart.js"
use default as dayjs with module, preconnect from "https://day.example/dayjs.js"
use Own from assets/own.mjs
use Gauge from "https://cdn.example/gauge.umd.js"
component Sales() {
    state chart = null
    <canvas @mount={(el) => { chart = Chart.new(el, {}); dayjs(); Gauge.go(); Own.go() }}></canvas>
}
component Plain() {
    state n = 0
    <button @click={() => { n += 1 }}>{n}</button>
}
route GET /sales = <html><head><title>s</title></head><body><Sales/></body></html>
route GET /plain = <html><head><title>p</title></head><body><Plain/></body></html>
"#);
    let page = |it: &mut _, p: &str| String::from_utf8_lossy(&get(it, p).body).to_string();
    let sales = page(&mut it, "/sales");
    let head = &sales[..sales.find("</head>").expect("a head")];
    assert!(head.contains(r#"<link rel="modulepreload" href="https://cdn.example/chart.js">"#), "{}", sales);
    assert!(head.contains(r#"<link rel="preconnect" href="https://day.example" crossorigin>"#), "{}", sales);
    // The plain script is loaded by the page, the modules are not.
    let body = &sales[head.len()..];
    assert!(body.contains("gauge.umd.js") && !body.contains("chart.js") && !body.contains("own.mjs"), "{}", sales);
    let at = sales.find("/_quillo/b/").expect("the bundle");
    let end = at + sales[at..].find('"').expect("its end");
    let bundle = String::from_utf8_lossy(&get(&mut it, &sales[at..end]).body).to_string();
    assert!(bundle.contains(r#"["Chart", "Chart", "https://cdn.example/chart.js"]"#), "{}", bundle);
    assert!(bundle.contains(r#"["dayjs", "default", "https://day.example/dayjs.js"]"#), "{}", bundle);
    assert!(bundle.contains(r#"["Own", "Own", "/assets/own.mjs"]"#), "{}", bundle);
    let plain = page(&mut it, "/plain");
    assert!(!plain.contains("modulepreload") && !plain.contains("crossorigin"), "{}", plain);
}

/// A page loads a browser library's script, before its bundle, only when a
/// component on it uses the library (branch js-libraries).
#[test]
fn a_page_loads_the_libraries_its_components_use() {
    let mut it = app(r#"
use Chart from "https://cdn.example/chart.umd.js"
use L from assets/leaflet.js
component Sales() {
    state chart = null
    <canvas @mount={(el) => { chart = Chart.new(el, {}) }}></canvas>
}
component Map() {
    <div @mount={(el) => { L.map(el) }}></div>
}
component Plain() {
    state n = 0
    <button @click={() => { n += 1 }}>{n}</button>
}
route GET /sales = <html><body><Sales/></body></html>
route GET /both = <html><body><Map/><Sales/></body></html>
route GET /plain = <html><body><Plain/></body></html>
"#);
    let page = |it: &mut _, p: &str| String::from_utf8_lossy(&get(it, p).body).to_string();
    let sales = page(&mut it, "/sales");
    let chart = sales.find("\"https://cdn.example/chart.umd.js\"").expect("the script");
    assert!(chart < sales.find("/_quillo/").expect("the bundle"), "{}", sales);
    assert!(!sales.contains("leaflet.js"), "{}", sales);
    // In the order the program names them, a path from the program's own address.
    let both = page(&mut it, "/both");
    let (c, l) = (both.find("chart.umd.js").expect("chart"), both.find("\"/assets/leaflet.js\"").expect("leaflet"));
    assert!(c < l, "{}", both);
    let plain = page(&mut it, "/plain");
    assert!(!plain.contains("chart.umd.js") && !plain.contains("leaflet.js"), "{}", plain);
}
