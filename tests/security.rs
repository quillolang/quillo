//! Requests that try to make a program do what its author did not write.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn app(src: &str) -> Interp {
    let mut it = Interp::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = it.load_source(src, "test.qwe") {
        panic!("{}", e);
    }
    it
}

fn send(it: &mut Interp, method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "localhost:8080".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
}

#[test]
fn spread_data_cannot_add_event_handlers_or_break_out_of_the_tag() {
    let mut it = app(r#"route POST /a = <a {...request.json}>"hi"</a>"#);
    let body = r#"{"href":"/ok","title":"t","onmouseover":"alert(1)","ONCLICK":"x","srcdoc":"<b>","x><script>alert(2)</script><b y":"1"}"#;
    let r = send(&mut it, "POST", "/a", &[("Content-Type", "application/json")], body);
    let html = r.text();
    assert!(html.starts_with(r#"<a href="/ok" title="t">"#), "{}", html);
    assert!(!html.contains("<script") && !html.to_lowercase().contains("onmouseover") && !html.to_lowercase().contains("onclick"), "{}", html);
    assert!(!html.contains("srcdoc"), "{}", html);
}

#[test]
fn script_urls_are_not_rendered() {
    let mut it = app("route GET /l ? { u: str } = <a href={u}>\"x\"</a>");
    for u in ["javascript:alert(1)", "%20JaVaScRiPt:alert(1)", "java%09script:alert(1)", "vbscript:x"] {
        let r = send(&mut it, "GET", &format!("/l?u={}", u), &[], "");
        assert!(r.text().contains(r#"href="about:blank#blocked""#), "{} -> {}", u, r.text());
    }
    let r = send(&mut it, "GET", "/l?u=/notes", &[], "");
    assert!(r.text().contains(r#"href="/notes""#), "{}", r.text());
}

#[test]
fn request_text_cannot_add_response_headers() {
    let mut it = app(
        r#"
route GET /h ? { v: str } = respond(200, "ok", { "x-echo": v })
route GET /go ? { to: str } = redirect(to)
route GET /status = redirect("/", 70000)
"#,
    );
    for url in ["/h?v=a%0d%0aset-cookie:%20x=1", "/go?to=/x%0d%0aset-cookie:%20x=1", "/go?to=/x%0aset-cookie:%20x=1"] {
        let r = send(&mut it, "GET", url, &[], "");
        assert_eq!(r.status, 500, "{}", url);
        assert!(r.header("set-cookie").is_none(), "{}", url);
    }
    assert_eq!(send(&mut it, "GET", "/h?v=fine", &[], "").header("x-echo"), Some("fine"));
    assert_eq!(send(&mut it, "GET", "/status", &[], "").status, 500);
    assert!(quillo::server::valid_header("x-a", "b\tc"));
    assert!(!quillo::server::valid_header("x a", "b"));
    assert!(!quillo::server::valid_header("x-a", "b\r\nc"));
}

#[test]
fn server_fns_answer_only_the_programs_own_pages() {
    let mut it = app("server fn add(n: int) -> int = n + 1");
    let call = |it: &mut Interp, headers: &[(&str, &str)]| send(it, "POST", "/_quillo/rpc/add", headers, "[41]").status;
    // Content types a page on another site can send without asking.
    assert_eq!(call(&mut it, &[("Content-Type", "text/plain; x=application/json")]), 415);
    assert_eq!(call(&mut it, &[("Content-Type", "text/plain")]), 415);
    let json = ("Content-Type", "application/json");
    assert_eq!(call(&mut it, &[json, ("Sec-Fetch-Site", "cross-site")]), 403);
    assert_eq!(call(&mut it, &[json, ("Sec-Fetch-Site", "same-site")]), 403);
    assert_eq!(call(&mut it, &[json, ("Origin", "https://evil.example")]), 403);
    assert_eq!(call(&mut it, &[json, ("Origin", "null")]), 403);
    assert_eq!(call(&mut it, &[json, ("Sec-Fetch-Site", "same-origin"), ("Origin", "http://localhost:8080")]), 200);
    assert_eq!(call(&mut it, &[("Content-Type", "application/json; charset=utf-8"), ("Origin", "http://localhost:8080")]), 200);
    // Scripts and tests send neither header.
    assert_eq!(call(&mut it, &[json]), 200);
}

#[test]
fn other_sites_cannot_post_forms_here() {
    let mut it = app(
        r#"
allow_cross_site("/sso")
route POST /notes = "saved"
route POST /sso/callback = "signed in"
route GET /notes = "list"
"#,
    );
    let form = ("Content-Type", "application/x-www-form-urlencoded");
    // Another site's page, by the browser's word or by Origin alone.
    for from in [[form, ("Sec-Fetch-Site", "cross-site")], [form, ("Origin", "https://evil.example")]] {
        let r = send(&mut it, "POST", "/notes", &from, "text=hi");
        assert_eq!(r.status, 403, "{:?}", from);
    }
    for ctype in ["multipart/form-data; boundary=x", "text/plain", ""] {
        let r = send(&mut it, "POST", "/notes", &[("Content-Type", ctype), ("Sec-Fetch-Site", "cross-site")], "x");
        assert_eq!(r.status, 403, "{}", ctype);
    }
    // The site's own pages, and programs that send no Origin.
    assert_eq!(send(&mut it, "POST", "/notes", &[form, ("Sec-Fetch-Site", "same-origin")], "text=hi").text(), "saved");
    assert_eq!(send(&mut it, "POST", "/notes", &[form, ("Origin", "http://localhost:8080")], "text=hi").text(), "saved");
    assert_eq!(send(&mut it, "POST", "/notes", &[form], "text=hi").text(), "saved");
    // JSON from another site needs the browser to ask first; reading is fine.
    assert_eq!(send(&mut it, "POST", "/notes", &[("Content-Type", "application/json"), ("Sec-Fetch-Site", "cross-site")], "{}").text(), "saved");
    assert_eq!(send(&mut it, "GET", "/notes", &[("Sec-Fetch-Site", "cross-site")], "").text(), "list");
    // A path the program allows.
    assert_eq!(send(&mut it, "POST", "/sso/callback", &[form, ("Sec-Fetch-Site", "cross-site")], "x=1").text(), "signed in");
    let mut bad = Interp::new(env!("CARGO_MANIFEST_DIR"));
    let e = bad.load_source("allow_cross_site(\"sso\")", "t.qwe").err().expect("a path without / is refused");
    assert!(e.to_string().contains("starts with /"), "{}", e);
}

#[test]
fn json_patterns_need_a_json_body() {
    let mut it = app(r#"route POST /order { json: { type: "order" } } = "accepted""#);
    let body = r#"{"type":"order"}"#;
    assert_eq!(send(&mut it, "POST", "/order", &[("Content-Type", "text/plain")], body).status, 400);
    assert_eq!(send(&mut it, "POST", "/order", &[("Content-Type", "application/x-www-form-urlencoded")], body).status, 400);
    assert_eq!(send(&mut it, "POST", "/order", &[("Content-Type", "application/json")], body).status, 200);
    assert_eq!(send(&mut it, "POST", "/order", &[], body).status, 200);
}

#[test]
fn errors_do_not_show_the_program_to_the_client() {
    let mut it = app("route GET /bad = 1 + \"x\" * 2");
    let r = send(&mut it, "GET", "/bad", &[], "");
    assert_eq!((r.status, r.text().as_str()), (500, "internal error\n"));
}

#[test]
fn uuids_are_version_4() {
    let v = quillo::eval_str("[uuid(), uuid()]").unwrap().repr();
    let ids: Vec<&str> = v.trim_matches(|c| c == '[' || c == ']').split(", ").map(|s| s.trim_matches('"')).collect();
    assert_ne!(ids[0], ids[1]);
    for id in ids {
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!("89ab".contains(&id[19..20]));
    }
}

#[test]
fn url_attributes_are_checked_however_they_are_written() {
    let mut it = app(r#"route GET /a = <a {...request.query} HREF={request.query.u}>x</a>"#);
    let r = send(&mut it, "GET", "/a?HREF=javascript:alert(1)&FormAction=JavaScript:x&u=javascript:y", &[], "");
    let body = r.text().to_lowercase();
    assert!(!body.contains("href=\"javascript:") && !body.contains("formaction=\"javascript:"), "{}", body);
    assert_eq!(body.matches("about:blank#blocked").count(), 2, "{}", body);
}

#[test]
fn scripts_cannot_reach_the_program_through_method_calls() {
    let dir = std::env::temp_dir().join(format!("quillo-script-escape-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("secret.txt"), "hunter2").unwrap();
    std::fs::write(dir.join("victim.txt"), "keep me").unwrap();
    let mut it = Interp::new(&dir);
    let src = r#"fn delete_everything(n) = "deleted"
route POST /rule { json: { code: str } } = json.stringify({ result: run_script(code, {}) })"#;
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    let run = |it: &mut Interp, code: &str| {
        let body = format!("{{\"code\":{:?}}}", code);
        send(it, "POST", "/rule", &[("Content-Type", "application/json")], &body).text()
    };
    for code in [r#""HOME".env"#, r#""secret.txt".read_file"#, r#""victim.txt".delete_file"#, r#""main.qwe".write_file"#, "42.delete_everything"] {
        let out = run(&mut it, code);
        assert!(out.contains("has no field"), "{}: {}", code, out);
        assert!(!out.contains("hunter2") && !out.contains("deleted"), "{}: {}", code, out);
    }
    // The same with the program's own values handed to `map`.
    let out = run(&mut it, r#"map(["secret.txt"], (f) => f.read_file)"#);
    assert!(!out.contains("hunter2"), "{}", out);
    assert_eq!(std::fs::read_to_string(dir.join("victim.txt")).unwrap(), "keep me");
    assert!(!dir.join("main.qwe").exists());
    // A script's own values still have their methods.
    assert_eq!(run(&mut it, r#""abc".upper + [1, 2].len"#), r#"{"result":"ABC2"}"#);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scripts_have_no_effects_of_their_own() {
    // A builtin handed over as a value does not run with effects.
    let e = quillo::eval_str(r#"run_script("e(\"HOME\")", { e: env })"#).unwrap().repr();
    assert!(e.starts_with("err(") && e.contains("env effect"), "{}", e);
    let e = quillo::eval_str(r#"run_script("map([\"Cargo.toml\"], f)", { f: read_file })"#).unwrap().repr();
    assert!(e.starts_with("err(") && e.contains("fs effect"), "{}", e);
    // A helper the program wrote keeps what the program may do, and what the
    // script hands it back runs as the script's.
    let v = quillo::eval_str(r#"fn has_home(f) = f(env("HOME")) != null
run_script("has_home((h) => h)", { has_home })"#).unwrap().repr();
    assert_eq!(v, "true");
    let e = quillo::eval_str(r#"fn call(f) = f()
run_script("call(() => e(\"HOME\"))", { call, e: env })"#).unwrap().repr();
    assert!(e.contains("env effect"), "{}", e);
}

/// Each shape of nesting, `n` deep.
fn nested_shapes(n: usize) -> Vec<(&'static str, String)> {
    vec![
        ("parens", format!("{}1{}", "(".repeat(n), ")".repeat(n))),
        ("negation", format!("{}1", "-".repeat(n))),
        ("not", format!("{}true", "!".repeat(n))),
        ("list", format!("{}1{}", "[".repeat(n), "]".repeat(n))),
        ("map", format!("{}1{}", "{ a: ".repeat(n), " }".repeat(n))),
        ("sum", vec!["1"; n].join(" + ")),
        ("fields", format!("\"x\"{}", ".upper".repeat(n))),
        ("calls", format!("\"x\"{}", ".upper()".repeat(n))),
        ("index", format!("{}1{}{}", "[".repeat(n), "]".repeat(n), "[0]".repeat(n))),
        ("blocks", format!("{}1{}", "{ let x = 1\n".repeat(n), " }".repeat(n))),
        ("ternary", format!("{}1{}", "true ? ".repeat(n), " : 2".repeat(n))),
        ("else if", format!("if false {{ 0 }}{} else {{ 1 }}", " else if false { 0 }".repeat(n))),
        ("lambda", format!("{}1{}", "(() => ".repeat(n), ")()".repeat(n))),
        ("template", format!("{}1{}", "`${".repeat(n), "}`".repeat(n))),
        ("html", format!("{}x{}", "<b>".repeat(n), "</b>".repeat(n))),
        ("pattern", format!("match [1] {{ {}_{} => 1, _ => 2 }}", "[".repeat(n), "]".repeat(n))),
        ("type", format!("((x: {}int{}) => 1)([])", "list<".repeat(n), ">".repeat(n))),
    ]
}

#[test]
fn deeply_nested_source_is_refused_not_a_crash() {
    // The review's case: 100,000 parentheses sent as a script.
    let parens = format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000));
    let e = quillo::eval_str(&format!("run_script({:?}, {{}})", parens)).unwrap().repr();
    assert!(e.contains("nests too deeply"), "{:.200}", e);
    for (shape, src) in nested_shapes(10_000) {
        let e = quillo::eval_str(&format!("let x = {}", src)).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("nests too deeply"), "{}: {:.200}", shape, e);
        let e = quillo::eval_str(&format!("run_script({:?}, {{}})", src)).unwrap().repr();
        assert!(e.contains("nests too deeply"), "{}: {:.200}", shape, e);
        let a = quillo::check::check_source(&format!("let x = {}", src), "deep.qwe", std::path::Path::new("."));
        assert!(a.diagnostics.iter().any(|d| d.message.contains("nests too deeply")), "{}", shape);
    }
    // Patterns and types too.
    let n = 10_000;
    for src in [
        format!("let x = match 1 {{ {}_{} => 1 }}", "[".repeat(n), "]".repeat(n)),
        format!("fn f(x: {}int{}) = 1", "list<".repeat(n), ">".repeat(n)),
    ] {
        let e = quillo::eval_str(&src).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("nests too deeply"), "{:.200}", e);
    }
}

#[test]
fn source_nested_as_deep_as_allowed_runs_and_checks_on_a_small_stack() {
    // The deepest each shape may be: it parses, checks and runs, in a
    // function and in a script, on a thread with less stack than a request's.
    let t = std::thread::Builder::new().stack_size(512 << 10).spawn(|| {
        for (shape, _) in nested_shapes(1) {
            let mut accepted = 0;
            for n in (1..=256).rev() {
                let (_, src) = nested_shapes(n).into_iter().find(|(s, _)| *s == shape).unwrap();
                let src = format!("fn f() = {}\nlen(str(f()))", src);
                if let Ok(v) = quillo::eval_str(&src) {
                    let a = quillo::check::check_source(&src, "deep.qwe", std::path::Path::new("."));
                    assert!(!a.diagnostics.iter().any(|d| d.message.contains("too deeply")), "{}", shape);
                    assert!(v.repr().parse::<i64>().is_ok(), "{}: {}", shape, v.repr());
                    let script = format!("run_script({:?}, {{}})", format!("len(str({}))", nested_shapes(n).into_iter().find(|(s, _)| *s == shape).unwrap().1));
                    let v = quillo::eval_str(&script).unwrap().repr();
                    assert!(v.parse::<i64>().is_ok() || v.contains("nests too deeply"), "{}: {}", shape, v);
                    accepted = n;
                    break;
                }
            }
            assert!(accepted >= 60, "{} is refused from {} deep", shape, accepted + 1);
        }
    });
    t.unwrap().join().unwrap();
}

#[test]
fn check_program_reads_nothing_outside_the_program() {
    let dir = std::env::temp_dir().join(format!("quillo-check-outside-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    std::fs::write(dir.join("outside/.env"), "hunter2").unwrap();
    std::fs::write(dir.join("outside/m.qwe"), "let x = hunter3").unwrap();
    std::fs::write(dir.join("app/lib.qwe"), "fn helper() = 1").unwrap();
    let mut it = Interp::new(dir.join("app"));
    let src = r#"route POST /check { json: { code: str } } = json.stringify(check_program("p.qwe", { "p.qwe": code }))"#;
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}", e);
    }
    let check = |it: &mut Interp, code: &str| {
        let body = format!("{{\"code\":{:?}}}", code);
        send(it, "POST", "/check", &[("Content-Type", "application/json")], &body).text()
    };
    for code in [
        r#"use "../outside/.env""#,
        r#"use "/etc/passwd""#,
        r#"mount /m = "../outside/m.qwe""#,
        r#"service billing = "../outside/m.qwe""#,
    ] {
        let out = check(&mut it, code);
        assert!(out.contains("must stay inside the program's folder"), "{}: {}", code, out);
        assert!(!out.contains("hunter") && !out.contains("root"), "{}: {}", code, out);
    }
    // Files inside it are still read.
    let out = check(&mut it, "use \"lib.qwe\"\nlet y = helper()\nprint(y)");
    assert_eq!(out, "[]");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn programs_that_use_or_mount_each_other_endlessly_are_an_error_not_a_crash() {
    let dir = std::env::temp_dir().join(format!("quillo-check-loop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.qwe"), "mount /a = \"a.qwe\"").unwrap();
    let a = quillo::check::check_file(&dir.join("a.qwe"));
    assert!(a.diagnostics.iter().any(|d| d.message.contains("more than 16 deep")), "{:?}", a.diagnostics);
    // A long chain of `use`, given as text to check.
    let files: std::collections::HashMap<_, _> =
        (0..1000).map(|i| (dir.join(format!("u{}.qwe", i)), format!("use \"u{}.qwe\"", i + 1))).collect();
    let a = quillo::check::check_file_with(&dir.join("u0.qwe"), files);
    assert!(a.diagnostics.iter().any(|d| d.message.contains("more than 64 deep")), "{:?}", a.diagnostics);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spread_keys_cannot_become_handlers_by_bind_or_case() {
    let mut it = app(r#"route GET /q = <div {...request.query}>"hi"</div>"#);
    for q in [
        "bind:onfocus=alert(document.domain)&autofocus=1&tabindex=0",
        "BIND:onfocus=alert(1)",
        "bind:onclick=alert(1)",
        "Bind:OnClick=alert(1)",
        "bind:srcdoc=%3Cscript%3Ealert(1)%3C/script%3E",
        "bind:__x=1",
        "@click=alert(1)",
        "x:onclick=alert(1)",
    ] {
        let html = send(&mut it, "GET", &format!("/q?{}", q), &[], "").text();
        let lower = html.to_lowercase();
        assert!(!lower.contains("onfocus") && !lower.contains("onclick") && !lower.contains("srcdoc"), "{} -> {}", q, html);
        assert!(!html.contains("__x") && !html.contains('@') && !html.contains(':'), "{} -> {}", q, html);
    }
    let html = send(&mut it, "GET", "/q?title=t&xlink:href=/ok", &[], "").text();
    assert!(html.contains(r#"title="t""#) && html.contains(r#"xlink:href="/ok""#), "{}", html);
}

#[test]
fn spread_keys_in_another_case_do_not_replace_written_attributes() {
    let mut it = app(
        r#"route POST /f = <form {...request.json} action="/login" method="post">"x"</form>
route POST /a = <a {...request.json} href="/ok" class="c">"x"</a>"#,
    );
    let json = [("Content-Type", "application/json")];
    let html = send(&mut it, "POST", "/f", &json, r#"{"ACTION":"https://evil.example/steal","Method":"get","title":"t"}"#).text();
    assert!(!html.contains("evil") && !html.contains(r#""get""#), "{}", html);
    assert!(html.contains(r#"action="/login""#) && html.contains(r#"method="post""#) && html.contains(r#"title="t""#), "{}", html);
    for body in [r#"{"Href":"https://evil.example"}"#, r#"{"bind:href":"https://evil.example"}"#, r#"{"HREF":"https://evil.example"}"#] {
        let html = send(&mut it, "POST", "/a", &json, body).text();
        assert!(!html.contains("evil") && html.contains(r#"href="/ok""#), "{} -> {}", body, html);
    }
    // A spread's class is added to the written one, whatever its case.
    let html = send(&mut it, "POST", "/a", &json, r#"{"CLASS":"d"}"#).text();
    assert!(html.contains(r#"class="c d""#) && html.matches("class=").count() == 1, "{}", html);
}

#[test]
fn handlers_and_frame_pages_take_only_text_written_in_the_program() {
    let load = |src: &str| Interp::new(env!("CARGO_MANIFEST_DIR")).load_source(src, "t.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    for (src, needle) in [
        ("route GET /a ? { u: str } = <div onmouseover={`go('${u}')`}>\"x\"</div>", "@mouseover"),
        ("route GET /a ? { u: str } = <div ONCLICK={u}>\"x\"</div>", "@click"),
        ("route GET /a ? { u: str } = <div bind:onclick={u}>\"x\"</div>", "@click"),
        ("route GET /a ? { onclick: str } = <div {onclick}>\"x\"</div>", "@click"),
        ("route GET /a ? { u: str } = <iframe srcdoc={u}></iframe>", "sandbox"),
        ("route GET /a ? { u: str } = <iframe sandbox=\"allow-scripts allow-same-origin\" srcdoc={u}></iframe>", "sandbox"),
    ] {
        let e = load(src);
        assert!(e.contains("takes only text written here") && e.contains(needle), "{}: {}", src, e);
        let a = quillo::check::check_source(src, "t.qwe", std::path::Path::new("."));
        assert!(a.diagnostics.iter().any(|d| d.message.contains("takes only text written here")), "{}", src);
    }
    // Text written in the program, a frame kept away from the site, and a
    // component's parameters are fine.
    let mut it = app(
        r#"view Picker(on_pick) = <p>"x"</p>
route GET /a ? { u: str } = <div>
    <button onclick="history.back()">"Back"</button>
    <iframe sandbox="" srcdoc={u}></iframe>
    <iframe sandbox="allow-scripts" srcdoc={u}></iframe>
    <Picker on_pick={(x) => x}/>
</div>"#,
    );
    let html = send(&mut it, "GET", "/a?u=%3Cb%3Ehi%3C/b%3E", &[], "").text();
    assert!(html.contains(r#"onclick="history.back()""#), "{}", html);
    assert_eq!(html.matches("srcdoc=\"&lt;b&gt;hi&lt;/b&gt;\"").count(), 2, "{}", html);
}

/// A server on this machine that answers `/go` with a redirect to `/secret`,
/// counting the connections it gets.
fn internal_server() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut s = s;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let answer = if req.starts_with("GET /go ") {
                format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{}/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", port)
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret".to_string()
            };
            let _ = s.write_all(answer.as_bytes());
        }
    });
    (port, hits)
}

#[test]
fn http_calls_can_refuse_redirects_and_addresses_inside() {
    let (port, hits) = internal_server();
    let run = |src: String| quillo::eval_str(&src).unwrap().repr();
    // Redirects are followed, unless told not to.
    assert_eq!(run(format!("http.get(\"http://127.0.0.1:{}/go\").body", port)), "\"secret\"");
    assert_eq!(run(format!("http.get(\"http://127.0.0.1:{}/go\", {{}}, {{ redirects: 0 }}).status", port)), "302");
    let before = hits.load(std::sync::atomic::Ordering::SeqCst);
    // Only public addresses: this machine, private and link-local networks
    // (the metadata service) are refused before connecting, however written.
    for url in [
        format!("http://127.0.0.1:{}/secret", port),
        format!("http://localhost:{}/secret", port),
        format!("http://2130706433:{}/secret", port),
        format!("http://[::1]:{}/secret", port),
        format!("http://[::ffff:127.0.0.1]:{}/secret", port),
        "http://169.254.169.254/latest/meta-data/".to_string(),
        "http://10.0.0.1/".to_string(),
        "http://192.168.1.1/".to_string(),
        "http://[fd00::1]/".to_string(),
        "http://0.0.0.0/".to_string(),
    ] {
        let e = run(format!("http.post({:?}, {{ a: 1 }}, {{}}, {{ public_only: true }})", url));
        assert!(e.starts_with("err(") && e.contains("not an address on the public internet"), "{}: {}", url, e);
    }
    let e = run(format!("http.stream(\"http://127.0.0.1:{}/secret\", (x) => x, {{ public_only: true }})", port));
    assert!(e.contains("not an address on the public internet"), "{}", e);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), before);
    for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(quillo::fetch::public_ip(ip.parse().unwrap()), "{}", ip);
    }
    for ip in ["100.64.0.1", "172.16.0.1", "198.18.0.1", "255.255.255.255", "fe80::1", "64:ff9b::7f00:1", "2002:7f00:1::1", "::127.0.0.1"] {
        assert!(!quillo::fetch::public_ip(ip.parse().unwrap()), "{}", ip);
    }
}
