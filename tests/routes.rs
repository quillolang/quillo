use wisp::server::{HttpRequest, HttpResponse};
use wisp::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.wisp") {
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
route GET / = <h1>"Hello"</h1>
route GET /text = "plain"
route GET /data = { ok: true, n: [1, 2] }
route GET /empty = nil
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
route GET /hello/:name = "Hello, {name}!"
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
    assert_eq!(get(&mut it, "/search?q=wisp+lang&limit=5").text(), r#"{"q":"wisp lang","limit":5}"#);
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
    required: fn(req) =
        if req.headers.authorization == "Bearer secret" then req
        else reply 401 { error: "unauthorized" }
}
route GET /me = request |> auth.required |> fn(r) = { ip: r.ip }
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
    let user = users |> find(fn(u) = u.id == id) ?? reply 404 { error: "no user {id}" }
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
route GET /boom = 1 + nil
route GET /err = err("nope")
"#,
    );
    let r = get(&mut it, "/boom");
    assert_eq!(r.status, 500);
    assert!(r.text().contains("cannot add int and nil"), "{}", r.text());
    let r = get(&mut it, "/err");
    assert_eq!((r.status, r.text().as_str()), (500, r#"{"error":"nope"}"#));
}

#[test]
fn headers_cookies_and_respond() {
    let mut it = app(
        r#"
route GET /who { cookies: { session: str } } = respond(200, "session {session}", { "X-Wisp": "1" })
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
    assert_eq!((r.status, r.text().as_str(), r.header("x-wisp")), (200, "session abc", Some("1")));
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
