//! FEEDBACK71.md, the security audit's third slice (branch security): a call signed with the
//! shared secret cannot say it is a caller `accept_calls` holds a key for (Q-970); a SPIFFE id
//! names its caller whole, as the user decided on 2026-11-17 (Q-971); a method that
//! is not an HTTP token is an error value and nothing connects (Q-972); a `site`'s calls are made
//! as `http`'s, under the host's policy, following redirects on the site's own origin only, a
//! `file:` one an error value (Q-973); `cache` keeps calls with other certificates, or none,
//! apart (Q-974). Every server is one this test starts, on this machine.

mod support;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::send;

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback71b-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

fn ev(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => format!("ERROR: {}", e),
    }
}

/// The head of a request: read up to its blank line.
fn read_head(s: &mut impl Read) -> String {
    let (mut buf, mut b) = (Vec::new(), [0u8; 1]);
    while !buf.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) => buf.push(b[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// `quillo` with these arguments and this environment to the end, given 60 seconds at most.
fn quillo(d: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args).current_dir(d).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for k in ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy", "QUILLO_SITE_COOKIES"] {
        c.env_remove(k);
    }
    c.envs(env.iter().copied());
    let child = c.spawn().unwrap();
    support::stop_with_tests(&child);
    let id = child.id();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let d2 = done.clone();
    std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(60);
        while !d2.load(Ordering::SeqCst) {
            if Instant::now() > until {
                let _ = Command::new("kill").args(["-KILL", &id.to_string()]).status();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let o = child.wait_with_output().unwrap();
    done.store(true, Ordering::SeqCst);
    o
}

/// `quillo run` of `file` as a server, its log in `log`, once it says where it listens (20 s at
/// most). The system picks the port (`--port 0`) and the log says which, so no other process
/// can take it in between (as support's `serve` does, FEEDBACK55.md); under `--split` the
/// services' lines come first, so the app's port is picked here and its own line waited for.
fn serve(d: &Path, file: &str, args: &[&str], env: &[(&str, &str)], log: &str) -> (support::Guard, u16) {
    let split = args.contains(&"--split");
    let given = if split { support::free_port() } else { 0 };
    let out = std::fs::File::create(d.join(log)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &given.to_string()])
        .args(args)
        .current_dir(d)
        .env_remove("QUILLO_SERVICE_SECRET")
        .env_remove("QUILLO_SERVICE_KEY")
        .env("SHOP_KEY", SHOP)
        .env("JWT_KEY", JWT)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = std::fs::read_to_string(d.join(log)).unwrap_or_default();
        // The program's own line: a service's, under --split, is marked `[name]`.
        let said = text.lines().filter(|l| !l.starts_with('[')).find_map(|l| l.split("listening on http://127.0.0.1:").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u16>().ok());
        if let Some(port) = said.filter(|p| !split || *p == given) {
            if support::listening(port) {
                return (g, port);
            }
        }
        if let Ok(Some(status)) = g.0.try_wait() {
            panic!("{} stopped ({}): {}", file, status, text);
        }
        assert!(Instant::now() < deadline, "{} did not start: {}", file, text);
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---- Q-970: the shared secret and callers with keys -----------------------------------------

const SHOP: &str = "shop-own-key-0123456789abcdef0123456789";
const SECRET: &str = "shared-secret-0123456789abcdef0123456789";
const JWT: &str = "jwt-secret-0123456789abcdef0123456";

const BILLING: &str = r#"accept_calls {
    keys: { shop: env("SHOP_KEY") },
}
server fn refund(account: int, cents: int) for ["shop"] {
    return { refunded: cents, account: account, caller: request.caller }
}
server fn whoami() {
    return request.caller
}
"#;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn call_id() -> String {
    static N: AtomicUsize = AtomicUsize::new(1);
    format!("c{}x{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
}

/// A call of `f` on service `billing` at `port`, as the audit's sign3.py makes one: version 3,
/// signed with `key` as `caller` (`kid` when the key is the caller's own), or version 2, which
/// signs no caller's name. `extra` headers go along.
fn signed(port: u16, v: u8, f: &str, args: &str, key: &str, kid: &str, caller: &str, extra: &[(&str, &str)]) -> (support::Answer, String) {
    let (t, id, deadline) = (now(), call_id(), "10000");
    let head = if v == 2 {
        format!("t={},sig={}", t, quillo::service::signature(key, t, "billing", f, "", &id, args.as_bytes()))
    } else {
        let sig = quillo::svcauth::sign3(key, t, "billing", f, "", &id, kid, caller, deadline, "\n", args.as_bytes());
        if kid.is_empty() {
            format!("v=3,t={},sig={}", t, sig)
        } else {
            format!("v=3,kid={},t={},sig={}", kid, t, sig)
        }
    };
    let mut hs: Vec<(&str, &str)> = vec![("x-quillo-service", &head), ("x-quillo-to", "billing"), ("x-quillo-call", &id), ("x-quillo-caller", caller), ("x-quillo-deadline", deadline)];
    hs.extend_from_slice(extra);
    (send(port, "POST", &format!("/_quillo/service/{}", f), &hs, args), id)
}

/// Its answer as `status body`.
fn said(port: u16, f: &str, args: &str, key: &str, kid: &str, caller: &str) -> String {
    let (a, _) = signed(port, 3, f, args, key, kid, caller, &[]);
    format!("{} {}", a.status, a.body)
}

const NOT_SHOP: &str = r#"401 {"error":"this service takes calls from `shop` signed with its key, not the shared secret (auth: { key, as: \"shop\" } where it declares this service)"}"#;
/// A call under the secret where `accept_calls` names callers another way (Q-970's second half).
const UNNAMED: &str = r#"200 {"ok":{"name":"","by":"quillo"}}"#;
const NO_NAME_REFUND: &str = r#"403 {"error":"a call signed with the shared secret names no caller here, as `accept_calls` takes keys, certificates or tokens: `refund` is for shop"}"#;

#[test]
fn the_shared_secret_cannot_say_it_is_a_caller_with_a_key_of_its_own() {
    let d = dir("keyed", &[("billing.qwe", BILLING)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
    // Shop, with its key.
    assert_eq!(said(bp, "refund", "[7, 500]", SHOP, "shop", "shop"), r#"200 {"ok":{"refunded":500,"account":7,"caller":{"name":"shop","by":"key"}}}"#);
    // The audit's call: the secret, saying it is shop. Refused, for every function, `_contracts`
    // too, which listed shop's own.
    assert_eq!(said(bp, "refund", "[7, 99999]", SECRET, "", "shop"), NOT_SHOP);
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "shop"), NOT_SHOP);
    assert_eq!(said(bp, "_contracts", "[]", SECRET, "", "shop"), NOT_SHOP);
    assert_eq!(said(bp, "nope", "[]", SECRET, "", "shop"), NOT_SHOP, "refused before the function is looked for");
    // Another name: one of the system's services, not named, as `accept_calls` names callers by
    // keys; `for` refuses it, and `_contracts` lists what anyone may call.
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "mailer"), UNNAMED);
    assert_eq!(said(bp, "refund", "[7, 1]", SECRET, "", "mailer"), NO_NAME_REFUND);
    assert_eq!(said(bp, "_contracts", "[]", SECRET, "", "mailer"), r#"200 {"ok":[{"header":"server fn whoami() = null","contract":"whoami()"}]}"#);
    // One either side of the name: another case, another letter, unicode, none at all.
    for other in ["Shop", "shops", "shöp", "", "ѕhop"] {
        assert_eq!(said(bp, "whoami", "[]", SECRET, "", other), UNNAMED, "{:?}", other);
        assert_eq!(said(bp, "refund", "[7, 1]", SECRET, "", other), NO_NAME_REFUND, "{:?}", other);
    }
    // Version 2 signs no name: the one it says is not believed, as before.
    let (v2, _) = signed(bp, 2, "whoami", "[]", SECRET, "", "shop", &[]);
    assert_eq!((v2.status, v2.body.as_str()), (200, r#"{"ok":{"name":"","by":"quillo"}}"#));
    let (v2, _) = signed(bp, 2, "refund", "[7, 1]", SECRET, "", "shop", &[]);
    assert_eq!(format!("{} {}", v2.status, v2.body), NO_NAME_REFUND);
    // Another secret, or shop's key under another name, as before.
    assert!(said(bp, "whoami", "[]", "another-secret-0123456789abcdef012345", "", "mailer").starts_with("401 {\"error\":\"not signed with this server's QUILLO_SERVICE_SECRET"));
    assert_eq!(said(bp, "whoami", "[]", SHOP, "mailer", "mailer"), "401 {\"error\":\"no key here for caller `mailer` (accept_calls { keys })\"}");
}

#[test]
fn a_certificate_still_proves_a_caller_the_secret_cannot_and_the_answer_is_signed_back() {
    let certs = "accept_calls {\n    keys: { shop: env(\"SHOP_KEY\") },\n    client_cert: { header: \"x-forwarded-client-cert\" },\n}\nserver fn whoami() {\n    return request.caller\n}\n";
    let d = dir("cert", &[("billing.qwe", certs)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
    // A call proven several ways is taken by the first that holds: the secret holds for no name
    // with a key, so the certificate decides.
    let (a, id) = signed(bp, 3, "whoami", "[]", SECRET, "", "shop", &[("x-forwarded-client-cert", "Subject=\"CN=shop\"")]);
    assert_eq!((a.status, a.body.as_str()), (200, r#"{"ok":{"name":"shop","by":"mtls","subject":"CN=shop"}}"#));
    let want = format!("sig={}", quillo::svcauth::sign_answer(SECRET, &id, 200, a.body.as_bytes()));
    assert_eq!(a.header("x-quillo-answer"), Some(want.as_str()), "signed back with the secret it was signed with, as a Quillo caller wants");
    let (a, _) = signed(bp, 3, "whoami", "[]", SECRET, "", "shop", &[("x-forwarded-client-cert", "Subject=\"CN=mailer\"")]);
    assert_eq!(a.body, r#"{"ok":{"name":"mailer","by":"mtls","subject":"CN=mailer"}}"#, "the certificate's name, not the one the secret said");
    let (a, _) = signed(bp, 3, "whoami", "[]", SECRET, "", "shop", &[]);
    assert_eq!(format!("{} {}", a.status, a.body), NOT_SHOP, "no certificate: why the secret did not do");
    // Under the secret, another name is no one's; a certificate on the call names it.
    let (a, id) = signed(bp, 3, "whoami", "[]", SECRET, "", "mailer", &[("x-forwarded-client-cert", "Subject=\"CN=mailer\"")]);
    assert_eq!(a.body, r#"{"ok":{"name":"mailer","by":"mtls","subject":"CN=mailer"}}"#);
    assert_eq!(a.header("x-quillo-answer"), Some(format!("sig={}", quillo::svcauth::sign_answer(SECRET, &id, 200, a.body.as_bytes())).as_str()));
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "mailer"), UNNAMED);
    // A certificate the proxy did not set, or one it cannot read: still one of the services.
    let (a, _) = signed(bp, 3, "whoami", "[]", SECRET, "", "mailer", &[("x-forwarded-client-cert", "Hash=abc")]);
    assert_eq!(format!("{} {}", a.status, a.body), UNNAMED);
}

#[test]
fn shared_false_refuses_the_secret_whatever_name_it_says_as_before() {
    let closed = "accept_calls {\n    keys: { shop: env(\"SHOP_KEY\") },\n    shared: false,\n}\nserver fn whoami() {\n    return request.caller\n}\n";
    let d = dir("closed", &[("billing.qwe", closed)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
    let no = r#"401 {"error":"this service takes no calls signed with the shared secret (accept_calls { shared: false })"}"#;
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "shop"), no);
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "mailer"), no);
    assert_eq!(said(bp, "whoami", "[]", SHOP, "shop", "shop"), r#"200 {"ok":{"name":"shop","by":"key"}}"#);
}

#[test]
fn under_split_shop_signs_with_its_key_and_the_secret_every_service_holds_makes_no_one_shop() {
    let shop = "service billing = \"billing.qwe\" { auth: { key: env(\"SHOP_KEY\"), as: \"shop\" } }\nserver fn buy() {\n    return billing.refund(7, 500)\n}\n";
    let mailer = "service billing = \"billing.qwe\"\nserver fn buy() {\n    return billing.refund(7, 1)\n}\nserver fn who() {\n    return billing.whoami()\n}\n";
    let main = "service shop = \"shop.qwe\"\nservice mailer = \"mailer.qwe\"\nroute GET /buy = shop.buy()\nroute GET /mailer-buys = mailer.buy()\nroute GET /mailer-is = mailer.who()\n";
    let d = dir("split", &[("billing.qwe", BILLING), ("shop.qwe", shop), ("mailer.qwe", mailer), ("main.qwe", main)]);
    // The secret is the test's, so that it can sign as a service would.
    let (_app, ap) = serve(&d, "main.qwe", &["--split"], &[("QUILLO_SERVICE_SECRET", SECRET), ("QUILLO_SECRET", "a secret for the feedback71b tests, long enough")], "split.log");
    let log = || std::fs::read_to_string(d.join("split.log")).unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(40);
    let bought = loop {
        let r = send(ap, "GET", "/buy", &[], "");
        if r.status == 200 {
            break r.body;
        }
        assert!(Instant::now() < deadline, "{} {}\n{}", r.status, r.body, log());
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(bought, r#"{"refunded":500,"account":7,"caller":{"name":"shop","by":"key"}}"#);
    // A service with no key: the secret, and so no name, as billing names its callers by keys.
    assert_eq!(send(ap, "GET", "/mailer-is", &[], "").body, r#"{"name":"","by":"quillo"}"#);
    assert_eq!(send(ap, "GET", "/mailer-buys", &[], "").status, 500, "mailer may not refund, as before");
    // Any service holds the secret: signed with it, `shop` is refused at billing itself.
    let at = log();
    let port: u16 = at
        .lines()
        .find_map(|l| l.strip_prefix("quillo: service billing (")?.split("http://127.0.0.1:").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
        .unwrap_or_else(|| panic!("billing's port is not in the log:\n{}", at));
    assert_eq!(said(port, "refund", "[7, 99999]", SECRET, "", "shop"), NOT_SHOP);
    assert_eq!(said(port, "_contracts", "[]", SECRET, "", "shop"), NOT_SHOP);
    assert_eq!(said(port, "whoami", "[]", SECRET, "", "mailer"), UNNAMED);
    assert_eq!(said(port, "refund", "[7, 1]", SECRET, "", "mailer"), NO_NAME_REFUND);
}

/// `accept_calls` as the guide writes it (its `jwt` with a `secret`, not an identity provider's
/// keys), `shared` at its default, beside functions for each kind of caller.
const GUIDE: &str = r#"accept_calls {
    keys: { shop: env("SHOP_KEY"), orders: env("SHOP_KEY") },
    jwt: { secret: env("JWT_KEY"), audience: "billing", issuer: "idp" },
    client_cert: { header: "x-forwarded-client-cert" },
    check: (caller) => caller.name != "mallory",
}
server fn refund(account: int, cents: int) for ["shop"] {
    return { refunded: cents }
}
server fn by_id(n: int) for ["spiffe://cluster.local/ns/shop/sa/shop"] {
    return n
}
server fn by_cn(n: int) for ["ops"] {
    return n
}
server fn by_token(n: int) for ["reports"] {
    return n
}
server fn whoami() {
    return request.caller
}
"#;

fn jwt(claims: &str, key: &str) -> String {
    let b = |s: &[u8]| quillo::crypto::b64url(s);
    let m = format!("{}.{}", b(br#"{"alg":"HS256","typ":"JWT"}"#), b(claims.as_bytes()));
    format!("{}.{}", m, b(&quillo::crypto::hmac_sha256(key.as_bytes(), m.as_bytes())))
}

#[test]
fn where_callers_are_named_by_keys_certificates_or_tokens_the_secret_names_no_one() {
    let d = dir("unnamed", &[("billing.qwe", GUIDE)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
    let refused = |f: &str, who: &str| format!("403 {{\"error\":\"a call signed with the shared secret names no caller here, as `accept_calls` takes keys, certificates or tokens: `{}` is for {}\"}}", f, who);
    // The reviewer's probe: the secret saying it is a SPIFFE id, a CN, a token's `sub`.
    assert_eq!(said(bp, "by_id", "[1]", SECRET, "", SHOP_ID), refused("by_id", SHOP_ID));
    assert_eq!(said(bp, "by_cn", "[1]", SECRET, "", "ops"), refused("by_cn", "ops"));
    assert_eq!(said(bp, "by_token", "[1]", SECRET, "", "reports"), refused("by_token", "reports"));
    // A name with a key of its own: refused at the door (the first half of Q-970).
    assert_eq!(said(bp, "refund", "[7, 1]", SECRET, "", "shop"), NOT_SHOP);
    assert!(said(bp, "whoami", "[]", SECRET, "", "orders").starts_with("401 "));
    // What needs no name, it may call; `check` sees it unnamed; `_contracts` lists only that.
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "reports"), UNNAMED);
    assert_eq!(said(bp, "whoami", "[]", SECRET, "", "mallory"), UNNAMED, "the name it says is not checked either");
    assert_eq!(said(bp, "_contracts", "[]", SECRET, "", "reports"), r#"200 {"ok":[{"header":"server fn whoami() = null","contract":"whoami()"}]}"#);
    assert_eq!(said(bp, "_contracts", "[]", SECRET, "", SHOP_ID), r#"200 {"ok":[{"header":"server fn whoami() = null","contract":"whoami()"}]}"#);
    // Each proven the way it is named: a certificate, a token, a key.
    assert_eq!(by_cert(bp, "by_id", "[1]", &uri(SHOP_ID)), "200 {\"ok\":1}");
    assert_eq!(by_cert(bp, "by_cn", "[2]", "Subject=\"CN=ops\""), "200 {\"ok\":2}");
    let now = now();
    let tok = jwt(&format!(r#"{{"client_id":"reports","iss":"idp","aud":"billing","exp":{}}}"#, now + 300), JWT);
    let bearer = format!("Bearer {}", tok);
    let id = call_id();
    let a = send(bp, "POST", "/_quillo/service/by_token", &[("x-quillo-to", "billing"), ("x-quillo-call", &id), ("authorization", &bearer)], "[3]");
    assert_eq!((a.status, a.body.as_str()), (200, "{\"ok\":3}"));
    // The secret and a token on one call: the token names it.
    let (a, _) = signed(bp, 3, "by_token", "[4]", SECRET, "", "mallory", &[("authorization", &bearer)]);
    assert_eq!((a.status, a.body.as_str()), (200, "{\"ok\":4}"));
    assert_eq!(said(bp, "refund", "[7, 5]", SHOP, "shop", "shop"), r#"200 {"ok":{"refunded":5}}"#);
}

#[test]
fn a_token_alone_is_enough_for_the_secret_to_name_no_one() {
    let jwt_only = "accept_calls {\n    jwt: { secret: env(\"JWT_KEY\"), audience: \"billing\", issuer: \"idp\" },\n}\nserver fn refund(account: int, cents: int) for [\"shop\"] {\n    return cents\n}\nserver fn whoami() {\n    return request.caller\n}\n";
    let d = dir("jwt-only", &[("billing.qwe", jwt_only)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
    for name in ["shop", "reports", "", "anyone"] {
        assert_eq!(said(bp, "refund", "[7, 1]", SECRET, "", name), NO_NAME_REFUND, "{:?}", name);
        assert_eq!(said(bp, "whoami", "[]", SECRET, "", name), UNNAMED, "{:?}", name);
    }
}

#[test]
fn with_the_secret_alone_names_are_taken_as_before() {
    let none = "server fn refund(account: int, cents: int) for [\"shop\"] {\n    return cents\n}\nserver fn whoami() {\n    return request.caller\n}\n";
    let checked = format!("accept_calls {{\n    check: (caller) => caller.name != \"mallory\",\n}}\n{}", none);
    for (name, text) in [("plain", none.to_string()), ("checked", checked)] {
        let d = dir(&format!("secret-alone-{}", name), &[("billing.qwe", &text)]);
        let (_b, bp) = serve(&d, "billing.qwe", &["--as-service"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
        assert_eq!(said(bp, "refund", "[7, 1]", SECRET, "", "shop"), "200 {\"ok\":1}", "{}", name);
        assert_eq!(said(bp, "whoami", "[]", SECRET, "", "mailer"), r#"200 {"ok":{"name":"mailer","by":"quillo"}}"#, "{}", name);
        assert_eq!(said(bp, "refund", "[7, 1]", SECRET, "", "mailer"), r#"403 {"error":"`refund` takes calls from shop only; this is `mailer`"}"#, "{}", name);
        assert!(said(bp, "_contracts", "[]", SECRET, "", "shop").contains("server fn refund("), "{}", name);
    }
}

// ---- Q-971: a SPIFFE id names its caller whole ------------------------------------------------

const SHOP_ID: &str = "spiffe://cluster.local/ns/shop/sa/shop";

/// A call with no signature, its certificate in the header a proxy sets.
fn by_cert(port: u16, f: &str, args: &str, xfcc: &str) -> String {
    let id = call_id();
    let a = send(port, "POST", &format!("/_quillo/service/{}", f), &[("x-quillo-to", "billing"), ("x-quillo-call", &id), ("x-forwarded-client-cert", xfcc)], args);
    format!("{} {}", a.status, a.body)
}

fn uri(u: &str) -> String {
    format!("By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;Subject=\"\";URI={}", u)
}

#[test]
fn a_spiffe_caller_is_named_by_its_whole_id_and_a_cn_by_its_cn() {
    let long = format!("spiffe://cluster.local/ns/{}/sa/shop", "l".repeat(1900));
    let billing = format!(
        "accept_calls {{\n    client_cert: {{ header: \"x-forwarded-client-cert\" }},\n    shared: false,\n}}\n\
         server fn refund(account: int, cents: int) for [\"{SHOP_ID}\"] {{\n    return {{ refunded: cents, caller: request.caller.name }}\n}}\n\
         server fn short(n: int) for [\"shop\"] {{\n    return n\n}}\n\
         server fn far() for [\"{long}\"] {{\n    return \"far\"\n}}\n\
         server fn whoami() {{\n    return request.caller\n}}\n"
    );
    let d = dir("spiffe", &[("billing.qwe", &billing)]);
    let check = quillo(&d, &["check", "billing.qwe"], &[]);
    assert!(check.status.success() && !String::from_utf8_lossy(&check.stderr).contains("warning"), "{}", String::from_utf8_lossy(&check.stderr));
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[], "billing.log");
    // The audit's three ids: only the exact one passes.
    assert_eq!(by_cert(bp, "refund", "[7, 500]", &uri(SHOP_ID)), format!("200 {{\"ok\":{{\"refunded\":500,\"caller\":\"{}\"}}}}", SHOP_ID));
    for evil in ["spiffe://cluster.local/ns/tenant-evil/sa/shop", "spiffe://evil.example/anything/shop"] {
        assert_eq!(by_cert(bp, "refund", "[7, 500]", &uri(evil)), format!("403 {{\"error\":\"`refund` takes calls from {} only; this is `{}`\"}}", SHOP_ID, evil));
    }
    assert_eq!(
        by_cert(bp, "whoami", "[]", &uri(SHOP_ID)),
        format!("200 {{\"ok\":{{\"name\":\"{0}\",\"by\":\"mtls\",\"subject\":\"\",\"uri\":\"{0}\"}}}}", SHOP_ID)
    );
    // A program written for the last part fails closed, and is told what to write.
    for id in [SHOP_ID, "spiffe://cluster.local/ns/tenant-evil/sa/shop"] {
        assert_eq!(by_cert(bp, "short", "[1]", &uri(id)), format!("403 {{\"error\":\"`short` takes calls from shop only; this caller is `{}`: write that in `for`\"}}", id));
    }
    // A certificate with no URI: its CN, a short name, as before.
    assert_eq!(by_cert(bp, "short", "[1]", "Hash=abc;Subject=\"CN=shop,O=Acme\""), "200 {\"ok\":1}");
    assert_eq!(by_cert(bp, "refund", "[7, 1]", "Hash=abc;Subject=\"CN=shop,O=Acme\""), format!("403 {{\"error\":\"`refund` takes calls from {} only; this is `shop`\"}}", SHOP_ID));
    assert_eq!(by_cert(bp, "short", "[1]", "Hash=abc;Subject=\"CN=shop\";URI="), "200 {\"ok\":1}", "an empty URI is none");
    // What `_contracts` lists follows the same names.
    let listed = |xfcc: &str| -> Vec<String> {
        let a = by_cert(bp, "_contracts", "[]", xfcc);
        let j: serde_json::Value = serde_json::from_str(a.split_once(' ').unwrap().1).unwrap();
        j["ok"].as_array().unwrap().iter().map(|f| f["contract"].as_str().unwrap().split('(').next().unwrap().to_string()).collect()
    };
    assert_eq!(listed(&uri(SHOP_ID)), ["refund", "whoami"]);
    assert_eq!(listed(&uri("spiffe://cluster.local/ns/tenant-evil/sa/shop")), ["whoami"]);
    assert_eq!(listed("Subject=\"CN=shop\""), ["short", "whoami"]);
    // Compared as written, among the ids the SPIFFE spec allows: another path is another id,
    // never the one in `for`; what the spec does not allow (a slash at the end, another case of
    // the scheme or the trust domain, `%`, unicode, a query) is no SPIFFE id, and names no one
    // (Q-1000).
    for other in ["spiffe://cluster.local/ns/shop/sa/Shop", "spiffe://cluster.local/ns/shop/sa/shop.v2", "spiffe://cluster.local/ns/shop/sa/shop/x"] {
        let r = by_cert(bp, "refund", "[7, 1]", &uri(other));
        assert_eq!(r, format!("403 {{\"error\":\"`refund` takes calls from {} only; this is `{}`\"}}", SHOP_ID, other));
        assert!(by_cert(bp, "whoami", "[]", &uri(other)).contains(&format!("\"name\":\"{}\"", other)), "{}", other);
    }
    for (other, why) in [
        ("spiffe://cluster.local/ns/shop/sa/shop/", "a part of its path is empty"),
        ("SPIFFE://cluster.local/ns/shop/sa/shop", "its scheme is `spiffe`, in lower case"),
        ("spiffe://Cluster.Local/ns/shop/sa/shop", "its trust domain is lower-case"),
        ("spiffe://cluster.local/ns/shop/sa/%73hop", "its path is letters, digits"),
        ("spiffe://cluster.local/ns/shöp/sa/shop", "its path is letters, digits"),
        ("spiffe://cluster.local/ns/shop/sa/shop?x", "its path is letters, digits"),
    ] {
        let r = by_cert(bp, "refund", "[7, 1]", &uri(other));
        assert!(r.starts_with("401 ") && r.contains("which is not a SPIFFE id: ") && r.contains(why), "{}: {}", other, r);
    }
    // A long id, whole, and one letter short of it.
    assert_eq!(by_cert(bp, "far", "[]", &uri(&long)), "200 {\"ok\":\"far\"}");
    assert!(by_cert(bp, "far", "[]", &uri(&long[..long.len() - 1])).starts_with("403 "));
    // A SPIFFE id is 2,048 bytes at most.
    let most = format!("spiffe://cluster.local/ns/{}", "m".repeat(2048 - 26));
    assert_eq!(most.len(), 2048);
    assert!(by_cert(bp, "whoami", "[]", &uri(&most)).starts_with("200 "));
    assert!(by_cert(bp, "whoami", "[]", &uri(&format!("{}m", most))).contains("longer than 2,048 bytes"));
    // Two URIs for one certificate: no SPIFFE id, and neither is taken.
    let two = format!("By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;URI={};URI=spiffe://cluster.local/ns/x/sa/y", SHOP_ID);
    assert_eq!(
        by_cert(bp, "whoami", "[]", &two),
        "401 {\"error\":\"`x-forwarded-client-cert` names 2 URIs for the client's certificate: a SPIFFE id is one (an X.509-SVID has exactly one), so who calls is not certain\"}"
    );
    // Two elements: one proxy's is taken here, unless `chain: true` (Q-1000).
    let chain = format!("By=spiffe://a/b;URI=spiffe://cluster.local/ns/x/sa/y,By=spiffe://c/d;URI={}", SHOP_ID);
    assert!(by_cert(bp, "refund", "[7, 2]", &chain).starts_with("401 {\"error\":\"`x-forwarded-client-cert` has 2 elements: one proxy's is expected"));
}

#[test]
fn a_certificates_identity_unit_by_unit() {
    use quillo::svcauth::{cert_caller, cert_identity};
    let name = |h: &str| cert_caller(h, false).map(|(n, _)| n);
    assert_eq!(name(&format!("URI={}", SHOP_ID)).unwrap(), SHOP_ID);
    assert_eq!(name(&format!("Subject=\"CN=shop\";URI={}", SHOP_ID)).unwrap(), SHOP_ID, "the URI over the CN");
    assert_eq!(name(&format!("Subject=\"CN=shop\";URI=\"{}\"", SHOP_ID)).unwrap_err(), "quotes `URI`, which Envoy writes bare", "Q-1000");
    assert_eq!(name("Subject=\"CN=shop\"").unwrap(), "shop");
    assert!(name("CN=plain").unwrap_err().contains("a key Envoy does not write"), "a subject alone is `subject: true`'s");
    assert_eq!(quillo::svcauth::subject_caller("CN=plain").unwrap().0, "plain");
    assert_eq!(name("Hash=1").unwrap_err(), "names no certificate's subject");
    assert!(name("URI=spiffe://a/b;uri=spiffe://a/c").unwrap_err().starts_with("names 2 URIs"), "the key in any case");
    assert!(name("URI=spiffe://a/b;URI=spiffe://a/b").unwrap_err().starts_with("names 2 URIs"), "even the same one twice");
    assert_eq!(name("URI=spiffe://a/b;URI=").unwrap(), "spiffe://a/b", "an empty one is none");
    assert!(cert_identity("URI=spiffe://a/b;URI=spiffe://a/c").is_none());
}

// ---- Q-988: the forwarded certificate, read as Envoy writes it ---------------------------------

const PROXY_ELEMENT: &str = "By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;Subject=\"\";URI=spiffe://cluster.local/ns/evil/sa/attacker";
const ATTACKER: &str = "spiffe://cluster.local/ns/evil/sa/attacker";

#[test]
fn an_element_of_the_clients_cannot_take_in_the_proxys_own() {
    let billing = format!(
        "accept_calls {{\n    client_cert: {{ header: \"x-forwarded-client-cert\" }},\n    shared: false,\n}}\n\
         server fn refund(n: int) for [\"{SHOP_ID}\"] {{\n    return n\n}}\n\
         server fn kshop() for [\"shop\"] {{\n    return request.caller\n}}\n\
         server fn whoami() {{\n    return request.caller\n}}\n"
    );
    let d = dir("xfcc", &[("billing.qwe", &billing)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[], "billing.log");
    let open = "401 {\"error\":\"`x-forwarded-client-cert` has a quote that is not closed, so where the proxy's own part begins is not certain\"}";
    // The proxy's own element alone: the attacker, as it is.
    assert_eq!(by_cert(bp, "refund", "[1]", PROXY_ELEMENT), format!("403 {{\"error\":\"`refund` takes calls from {} only; this is `{}`\"}}", SHOP_ID, ATTACKER));
    // The reviewer's probes: an element of the client's, in front (Envoy's APPEND_FORWARD), with
    // a quote it never closes. It took in the proxy's element, and named the caller shop.
    let forged = format!("URI={};X=\",{}", SHOP_ID, PROXY_ELEMENT);
    assert_eq!(by_cert(bp, "refund", "[2]", &forged), open);
    let with_subject = format!("URI={};X=\",{}", SHOP_ID, PROXY_ELEMENT.replace("Subject=\"\"", "Subject=\"CN=attacker,O=Evil\""));
    assert_eq!(by_cert(bp, "refund", "[3]", &with_subject), open);
    let forged_cn = format!("Subject=\"CN=shop;,{}", PROXY_ELEMENT);
    assert_eq!(by_cert(bp, "kshop", "[]", &forged_cn), open);
    // A quote escaped in the proxy's subject is the subject's: the URI after it still counts.
    let escaped = "By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;Subject=\"CN=shop/\\\"\";URI=spiffe://cluster.local/ns/evil/sa/attacker";
    assert_eq!(by_cert(bp, "kshop", "[]", escaped), format!("403 {{\"error\":\"`kshop` takes calls from shop only; this is `{}`\"}}", ATTACKER));
    let a_quote = "By=x;Hash=abc;Subject=\"CN=a\\\"b\";URI=spiffe://cluster.local/ns/evil/sa/attacker";
    assert_eq!(by_cert(bp, "whoami", "[]", a_quote), format!("200 {{\"ok\":{{\"name\":\"{0}\",\"by\":\"mtls\",\"subject\":\"CN=a\\\\\\\"b\",\"uri\":\"{0}\"}}}}", ATTACKER));
    // A client's element in front, quoted as it should be: two elements, one proxy's expected
    // (Q-1000; `chain: true` is tried below).
    let balanced = format!("Hash=x;Subject=\"CN=shop,O=x\";URI={},{}", SHOP_ID, PROXY_ELEMENT);
    assert!(by_cert(bp, "whoami", "[]", &balanced).starts_with("401 {\"error\":\"`x-forwarded-client-cert` has 2 elements"));
    // A backslash outside quotes, or a part that is not key=value, in front of it.
    assert_eq!(by_cert(bp, "whoami", "[]", &format!("X=a\\,{}", PROXY_ELEMENT)), "401 {\"error\":\"`x-forwarded-client-cert` has a backslash outside a quoted value\"}");
    assert_eq!(by_cert(bp, "whoami", "[]", &format!("junk,{}", PROXY_ELEMENT)), "401 {\"error\":\"`x-forwarded-client-cert` has a part that is not key=value\"}");
}

#[test]
fn the_forwarded_certificate_unit_by_unit() {
    use quillo::svcauth::cert_caller;
    let name = |h: &str| cert_caller(h, false).map(|(n, _)| n);
    let subject = |h: &str| {
        cert_caller(h, false).map(|(_, extra)| extra.iter().find(|(k, _)| *k == "subject").map(|(_, v)| v.display()).unwrap_or_default())
    };
    // Balanced quotes; `;`, `,` and `=` inside a quoted value; an escaped quote and backslash.
    assert_eq!(name("Hash=1;Subject=\"CN=shop,O=Acme\\;Inc=1\"").unwrap(), "shop");
    assert_eq!(subject("Hash=1;Subject=\"CN=shop,O=Acme\\;Inc=1\"").unwrap(), "CN=shop,O=Acme\\;Inc=1", "the subject as written between the quotes");
    assert_eq!(name("Subject=\"CN=a\\\"b,O=c\"").unwrap(), "a\"b", "the CN with its escapes decoded (Q-989)");
    assert_eq!(name("Subject=\"CN=a\\\\,O=c\"").unwrap(), "a\\");
    // A subject's own RFC 2253 escapes, `O=Acme\, Inc.`, are a value's.
    assert_eq!(name("Subject=\"CN=shop,O=Acme\\, Inc.\"").unwrap(), "shop");
    assert_eq!(name("Subject=\"CN=shop\";URI=\"spiffe://a/b;c\"").unwrap_err(), "quotes `URI`, which Envoy writes bare", "Q-1000");
    // Several elements, with `chain: true`: the last is the client's.
    let last = |h: &str| cert_caller(h, true).map(|(n, _)| n);
    assert_eq!(last("By=a;URI=spiffe://x/one,By=b;URI=spiffe://x/two").unwrap(), "spiffe://x/two");
    assert_eq!(last("By=a;URI=spiffe://x/one,By=b;Subject=\"CN=c\"").unwrap(), "c");
    assert!(name("By=a;URI=spiffe://x/one,By=b;URI=spiffe://x/two").unwrap_err().starts_with("has 2 elements"));
    // No empty part or element, and no space around a separator: Envoy writes none (Q-1000).
    for h in [",By=b;URI=spiffe://x/two", "By=b;URI=spiffe://x/two,", "By=b;;URI=spiffe://x/two", "By=b;URI=spiffe://x/two;", ","] {
        for take in [name, last] {
            assert_eq!(take(h).unwrap_err(), "has an empty part or element, which Envoy never writes", "{:?}", h);
        }
    }
    assert_eq!(name("").unwrap_err(), "names no certificate's subject");
    // Refused: a quote not closed, anywhere; a backslash outside quotes, or ending the header; a
    // quote inside a value not quoted whole; a part that is not key=value; two subjects.
    for (h, why) in [
        ("Subject=\"CN=shop", "quote that is not closed"),
        ("URI=spiffe://a/b;X=\",By=c;URI=spiffe://d/e", "quote that is not closed"),
        ("Subject=\"CN=a\\\"", "quote that is not closed"),
        ("Subject=\"CN=a\\", "quote that is not closed"),
        ("Subject=\"\"\"", "quote that is not closed"),
        ("URI=spiffe://a\\b", "backslash outside a quoted value"),
        ("Subject=CN=\"shop\"", "quote inside a value that is not quoted whole"),
        ("Subject=\"CN=a\"x\"\"", "quote inside a value that is not quoted whole"),
        ("Hash=1;junk", "not key=value"),
        ("=x", "not key=value"),
        ("\"Subject\"=x", "not key=value"),
        ("Subject=\"CN=a\";Subject=\"CN=b\"", "names 2 subjects"),
    ] {
        let e = name(h).unwrap_err();
        assert!(e.contains(why), "{:?}: {}", h, e);
    }
}

// ---- Q-1000: one proxy's element, unless `chain: true` ---------------------------------------

const SLEEP: &str = "By=spiffe://cluster.local/ns/default/sa/productpage;Hash=4d7c;Subject=\"\";URI=spiffe://cluster.local/ns/default/sa/sleep";
const SLEEP_ID: &str = "spiffe://cluster.local/ns/default/sa/sleep";

#[test]
fn one_proxys_element_by_default_and_the_last_of_several_with_chain_unit_by_unit() {
    use quillo::svcauth::cert_caller;
    let one = |h: &str| cert_caller(h, false).map(|(n, _)| n);
    let last = |h: &str| cert_caller(h, true).map(|(n, _)| n);
    // One element passes either way, as Envoy writes them (its own examples among them).
    for (h, who) in [
        (SLEEP.to_string(), SLEEP_ID),
        (format!("{};DNS=sleep.default.svc", SLEEP), SLEEP_ID),
        // A URI that is no SPIFFE id is shown, and the CN names the caller (Q-1000).
        ("By=http://frontend.lyft.com;Hash=468ed;Cert=\"-----BEGIN%20CERTIFICATE-----%0AMIIC%0A\";Chain=\"-----BEGIN%20CERTIFICATE-----%0A\";Subject=\"CN=Test Client,OU=Lyft,L=San Francisco,ST=CA,C=US\";URI=http://testclient.lyft.com;DNS=lyft.com;DNS=www.lyft.com".to_string(), "Test Client"),
        ("Hash=ab;Subject=\"CN=shop\";URI=".to_string(), "shop"),
        ("Hash=ab;Subject=\"CN=shop\";URI=;DNS=a;DNS=b;DNS=*.c".to_string(), "shop"),
        ("Hash=ab;Subject=\"OU=Sales+CN=J. Smith,O=Widget Inc.,C=US\"".to_string(), "J. Smith"),
        ("by=x;hash=1;subject=\"CN=shop\"".to_string(), "shop"),
    ] {
        assert_eq!(one(&h).as_deref(), Ok(who), "{}", h);
        assert_eq!(last(&h).as_deref(), Ok(who), "{}", h);
    }
    // Two: refused by default, the last taken with `chain: true`.
    let two = format!("By=spiffe://a/gw;Hash=1;Subject=\"\";URI=spiffe://ext/client,{}", SLEEP);
    assert_eq!(
        one(&two).unwrap_err(),
        "has 2 elements: one proxy's is expected, in place of any the client sent (Envoy's SANITIZE_SET); `client_cert: { chain: true }` takes the last of several (APPEND_FORWARD)"
    );
    assert_eq!(last(&two).as_deref(), Ok(SLEEP_ID));
    // The reviewer's SANs with a comma, which Envoy writes bare: two elements, refused by default.
    let comma = "By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;Subject=\"\";URI=spiffe://cluster.local/ns/evil/sa/x,URI=spiffe://cluster.local/ns/shop/sa/shop";
    assert!(one(comma).unwrap_err().starts_with("has 2 elements"));
    let ops = "By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;URI=spiffe://cluster.local/ns/evil/sa/x,Subject=CN=ops";
    assert!(one(ops).unwrap_err().starts_with("has 2 elements"));
    // With `chain: true`, a subject after the comma is bare, where Envoy quotes one: refused.
    assert_eq!(last(ops).unwrap_err(), "writes `Subject` bare, which Envoy quotes");
    // What still stands with `chain: true`: a URI after the comma reads as the last element, as
    // a proxy's would, and names the caller. A SPIFFE id cannot hold a comma; a CA that issues
    // other URIs (or DNS names) with one must not be trusted with `chain: true`.
    assert_eq!(last(comma).as_deref(), Ok(SHOP_ID));
    // A `;` in a SAN, either way: the pair it adds is one Envoy did not write there.
    for (h, why) in [
        (format!("{};URI={}", SLEEP, SHOP_ID), "names 2 URIs"),
        (format!("By=x;Hash=1;Subject=\"\";URI=https://evil.example/x;URI={}", SHOP_ID), "names 2 URIs"),
        (format!("By=x;Hash=1;Subject=\"\";URI=;DNS=evil.example;URI={}", SHOP_ID), "out of Envoy's order"),
        (format!("{};Subject=\"CN=ops\"", SLEEP), "out of Envoy's order"),
        (format!("{};Hash=z", SLEEP), "out of Envoy's order"),
        ("Subject=\"CN=a\";By=x".to_string(), "out of Envoy's order"),
        (format!("{};X=1", SLEEP), "a key Envoy does not write"),
    ] {
        for take in [one, last] {
            let e = take(&h).unwrap_err();
            assert!(e.contains(why), "{}: {}", h, e);
        }
    }
    // pe.py #4: an element of the client's ending in a quote, before a proxy's that writes no
    // subject and whose URI ends in one. The quotes balance; the key, or the quoting, gives it.
    let bait = format!("URI={};Y=\",By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;URI=spiffe://cluster.local/ns/evil/sa/a\"", SHOP_ID);
    let dns = bait.replace(";Y=\"", ";DNS=\"");
    let subject = bait.replace(";Y=\"", ";Subject=\"");
    for take in [one, last] {
        assert!(take(&bait).unwrap_err().contains("`Y`, a key Envoy does not write"));
        assert_eq!(take(&dns).unwrap_err(), "quotes `DNS`, which Envoy writes bare");
        assert!(take(&subject).unwrap_err().contains("out of Envoy's order"));
    }
    // A URI that is no SPIFFE id holds nothing that could end it, or be read as more.
    for u in ["https://x/?a=b", "https://x/a\tb", "urn:x=y"] {
        let e = one(&format!("By=x;Hash=1;URI={}", u)).unwrap_err();
        assert!(e.contains("gives a URI with"), "{}: {}", u, e);
    }
    let shown = quillo::svcauth::cert_caller("By=x;Hash=1;Subject=\"CN=me\";URI=https://x/a-b_c.d~e/f:g@h!$&'()*+", false).unwrap();
    assert_eq!(shown.0, "me");
    assert_eq!(shown.1.iter().find(|(k, _)| *k == "uri").map(|(_, v)| v.display()).as_deref(), Some("https://x/a-b_c.d~e/f:g@h!$&'()*+"), "shown, naming no one");
    assert_eq!(one("By=x;Hash=1;URI=https://victim.example/").unwrap_err(), "names no certificate's subject");
    // A `spiffe://` URI is a SPIFFE id as the spec writes one.
    let spiffe = |u: &str| one(&format!("By=x;Hash=1;URI={}", u));
    for ok in ["spiffe://td", "spiffe://td/a", "spiffe://a.b-c_d/A.b-C_9/x", "spiffe://td/..a/a..", "spiffe://123/456"] {
        assert_eq!(spiffe(ok).as_deref(), Ok(ok), "{}", ok);
    }
    for (bad, why) in [
        ("spiffe://Cluster.local/ns/x", "trust domain"),
        ("spiffe://cluster.local:443/ns/x", "trust domain"),
        ("spiffe://user@cluster.local/ns/x", "trust domain"),
        ("spiffe:///ns/x", "trust domain"),
        ("Spiffe://td/x", "scheme"),
        ("spiffe://td/ns/%73hop", "path is letters"),
        ("spiffe://td/ns//sa", "empty, `.` or `..`"),
        ("spiffe://td/ns/x/", "empty, `.` or `..`"),
        ("spiffe://td/", "empty, `.` or `..`"),
        ("spiffe://td/ns/../sa", "empty, `.` or `..`"),
        ("spiffe://td/./x", "empty, `.` or `..`"),
        ("spiffe://td/x?y", "path is letters"),
        ("spiffe://td/x#y", "path is letters"),
        ("spiffe://td/sh\u{00f6}p", "path is letters"),
    ] {
        let e = spiffe(bad).unwrap_err();
        assert!(e.contains("which is not a SPIFFE id") && e.contains(why), "{}: {}", bad, e);
    }
    let most = format!("spiffe://td/{}", "x".repeat(2048 - 12));
    assert_eq!(spiffe(&most).as_deref(), Ok(most.as_str()));
    assert!(spiffe(&format!("{}x", most)).unwrap_err().contains("longer than 2,048 bytes"));
}

#[test]
fn a_subject_alone_is_read_whole_where_the_program_says_so() {
    use quillo::svcauth::{cert_caller, subject_caller};
    let one = |h: &str| subject_caller(h).map(|(n, extra)| (n, extra.iter().find(|(k, _)| *k == "subject").map(|(_, v)| v.display()).unwrap_or_default()));
    // As nginx's `$ssl_client_s_dn` gives it: its RDNs were split as elements, the last taken.
    assert_eq!(one("CN=admin,CN=evil").unwrap_err(), "gives 2 CNs in the certificate's subject, so who calls is not certain");
    assert_eq!(one("CN=evil,OU=x,CN=shop").unwrap_err(), "gives 2 CNs in the certificate's subject, so who calls is not certain");
    assert_eq!(one("CN=shop,O=Acme\\, Inc.,C=US").unwrap(), ("shop".to_string(), "CN=shop,O=Acme\\, Inc.,C=US".to_string()));
    assert_eq!(one("O=Acme,CN=shop").unwrap().0, "shop");
    assert_eq!(one("CN=Doe\\, John,O=x").unwrap().0, "Doe, John");
    // The slash form is refused, with what to send instead (Q-989).
    assert!(one("/C=US/O=Acme/CN=shop").unwrap_err().contains("send it as RFC 2253 writes it"));
    // A subject with one of Envoy's keys in it is Envoy's header, or one after the client's.
    assert!(one("CN=shop,Hash=abc").unwrap_err().contains("one of Envoy's keys"));
    assert!(one("CN=shop,O=x\\,Hash=4d7c;Subject=\"CN=me\"").unwrap_err().contains("does not parse"));
    assert!(one("\"CN=shop,O=Acme\"").unwrap_err().contains("no attribute's type"));
    // Envoy's header is never read as a subject: the reviewer's prefixes, sent by the client
    // before Envoy's appended element (APPEND_FORWARD), name no one either way.
    for h in [
        "/CN=shop/O=,Hash=4d7c;Subject=\"CN=me\"",
        "/CN=shop/O=,Hash=4d7c;Subject=\"CN=me\";URI=spiffe://td/ns/a/sa/me",
        "CN=shop,O=x\\,Hash=4d7c",
        "CN=shop,O=x\\,Hash=4d7c;Subject=\"CN=me\"",
        "CN=shop,Hash=4d7c;Subject=\"CN=me\"",
    ] {
        for chain in [false, true] {
            let got = cert_caller(h, chain).map(|(n, _)| n);
            assert!(got.is_err(), "{} (chain {}): {:?}", h, chain, got);
        }
    }
}

#[test]
fn one_element_and_every_line_of_the_header_in_a_service() {
    let accept = |chain: &str| format!("accept_calls {{\n    keys: {{ shop: env(\"SHOP_KEY\") }},\n    client_cert: {{ header: \"x-forwarded-client-cert\"{} }},\n}}\nserver fn by_id(n: int) for [\"{SHOP_ID}\"] {{\n    return n\n}}\nserver fn whoami() {{\n    return request.caller\n}}\n", chain);
    let d = dir("one-element", &[("billing.qwe", &accept("")), ("chain.qwe", &accept(", chain: true"))]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[("QUILLO_SERVICE_SECRET", SECRET)], "billing.log");
    let (_c, cp) = serve(&d, "chain.qwe", &["--as-service", "--trust-proxy", "127.0.0.1", "--service-name", "billing"], &[], "chain.log");
    let lines = |port: u16, values: &[&str]| -> String {
        let id = call_id();
        let mut hs: Vec<(&str, &str)> = vec![("x-quillo-to", "billing"), ("x-quillo-call", &id)];
        hs.extend(values.iter().map(|v| ("x-forwarded-client-cert", *v)));
        let a = send(port, "POST", "/_quillo/service/by_id", &hs, "[1]");
        format!("{} {}", a.status, a.body)
    };
    let proxy = "By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;Subject=\"\";URI=spiffe://cluster.local/ns/evil/sa/x";
    let evil = format!("403 {{\"error\":\"`by_id` takes calls from {} only; this is `spiffe://cluster.local/ns/evil/sa/x`\"}}", SHOP_ID);
    // pe.py #13: the client's line first, the proxy's added after it. The first was read alone.
    for port in [bp, cp] {
        assert_eq!(lines(port, &[&format!("By=x;Hash=1;Subject=\"\";URI={}", SHOP_ID)]), "200 {\"ok\":1}");
    }
    let two = [format!("URI={}", SHOP_ID), proxy.to_string()];
    let two: Vec<&str> = two.iter().map(String::as_str).collect();
    assert!(lines(bp, &two).starts_with("401 {\"error\":\"`x-forwarded-client-cert` has 2 elements"), "{}", lines(bp, &two));
    assert_eq!(lines(cp, &two), evil, "`chain: true`: the proxy's line, the last");
    // pe.py #2 and #3: a SAN with a comma, in the proxy's own element.
    let san = format!("{},By=y;Hash=z;URI={}", proxy, SHOP_ID);
    assert!(lines(bp, &[&san]).starts_with("401 {\"error\":\"`x-forwarded-client-cert` has 2 elements"));
    let cn = "By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;URI=spiffe://cluster.local/ns/evil/sa/x,Subject=CN=ops";
    assert!(lines(bp, &[cn]).starts_with("401 "));
    assert_eq!(lines(cp, &[cn]), "401 {\"error\":\"`x-forwarded-client-cert` writes `Subject` bare, which Envoy quotes\"}");
    // The honest setups, in both modes: Istio's sidecar, Envoy with Cert and Chain, a CN-only
    // certificate, several DNS names.
    let whoami = |port: u16, h: &str| {
        let id = call_id();
        let a = send(port, "POST", "/_quillo/service/whoami", &[("x-quillo-to", "billing"), ("x-quillo-call", &id), ("x-forwarded-client-cert", h)], "[]");
        format!("{} {}", a.status, a.body)
    };
    for (h, name) in [
        (format!("{};DNS=sleep.default.svc", SLEEP), SLEEP_ID),
        ("By=http://frontend.lyft.com;Hash=468ed;Cert=\"-----BEGIN%20CERTIFICATE-----%0AMIIC%2Fz%3D%3D%0A-----END%20CERTIFICATE-----%0A\";Chain=\"-----BEGIN%20CERTIFICATE-----%0AMIIC%0A-----END%20CERTIFICATE-----%0A\";Subject=\"CN=Test Client,OU=Lyft,L=San Francisco,ST=CA,C=US\";URI=http://testclient.lyft.com;DNS=lyft.com;DNS=www.lyft.com".to_string(), "Test Client"),
        ("Hash=ab;Subject=\"CN=shop\";URI=".to_string(), "shop"),
        ("Hash=ab;Subject=\"CN=shop\";URI=;DNS=a;DNS=b;DNS=*.c".to_string(), "shop"),
    ] {
        for port in [bp, cp] {
            let got = whoami(port, &h);
            assert!(got.starts_with(&format!("200 {{\"ok\":{{\"name\":\"{}\",\"by\":\"mtls\"", name)), "{}: {}", h, got);
        }
    }
    assert!(whoami(bp, "By=http://frontend.lyft.com;Hash=468ed;Subject=\"CN=Test Client\";URI=http://testclient.lyft.com").contains("\"uri\":\"http://testclient.lyft.com\""), "a URI that is no SPIFFE id is shown");
    // The third review's probes: what the client sends before Envoy's appended element picks no
    // parser; Envoy's header is read as Envoy's, in both modes (Q-1000).
    for h in ["/CN=shop/O=,Hash=4d7c;Subject=\"CN=me\";URI=", "CN=shop,O=x\\,Hash=4d7c", "CN=shop,Hash=4d7c;Subject=\"CN=me\""] {
        for port in [bp, cp] {
            assert!(lines(port, &[h]).starts_with("401 "), "{}: {}", h, lines(port, &[h]));
        }
    }
    // A URI cut short, or spaces Envoy never writes: refused, in both modes.
    for h in [
        "Hash=ab;Subject=\"CN=me\";URI=spiffe://cluster.local/ns/shop/sa/shop;",
        "Hash=ab;Subject=\"CN=me\";URI=spiffe://cluster.local/ns/shop/sa/shop,",
        "Hash=ab;Subject=\"CN=me\";URI=spiffe://cluster.local/ns/shop/sa/shop, ,",
        "Hash=ab;Subject=\"CN=me\";URI=spiffe://cluster.local/ns/shop/sa/shop ;DNS=x",
        "Hash=ab;Subject=\"CN=me\";URI= spiffe://cluster.local/ns/shop/sa/shop",
        "Hash=ab;Subject=\"CN=me\";URI=spiffe://cluster.local/ns/shop/sa/shop\u{3000}",
    ] {
        for port in [bp, cp] {
            assert!(lines(port, &[h]).starts_with("401 "), "{:?}: {}", h, lines(port, &[h]));
        }
    }
    // A CN that reads as a SPIFFE id names no one.
    assert_eq!(
        lines(bp, &[&format!("Hash=ab;Subject=\"CN={}\";URI=", SHOP_ID)]),
        "401 {\"error\":\"`x-forwarded-client-cert` gives a CN with `:` in it, which could pass for a URI's name: it names no caller\"}"
    );
    // A subject alone, read whole where the program says the header is one: nginx's
    // `proxy_set_header X-Client-DN $ssl_client_s_dn`.
    let dn = format!("accept_calls {{\n    client_cert: {{ header: \"x-client-dn\", subject: true }},\n    shared: false,\n}}\nserver fn kshop() for [\"shop\"] {{\n    return request.caller\n}}\n");
    std::fs::write(d.join("dn.qwe"), dn).unwrap();
    let (_n, np) = serve(&d, "dn.qwe", &["--as-service", "--trust-proxy", "127.0.0.1", "--service-name", "billing"], &[], "dn.log");
    let who = |subjects: &[&str]| {
        let id = call_id();
        let mut hs: Vec<(&str, &str)> = vec![("x-quillo-to", "billing"), ("x-quillo-call", &id)];
        hs.extend(subjects.iter().map(|v| ("x-client-dn", *v)));
        let a = send(np, "POST", "/_quillo/service/kshop", &hs, "[]");
        format!("{} {}", a.status, a.body)
    };
    assert_eq!(who(&["CN=shop,O=Acme\\, Inc.,C=US"]), "200 {\"ok\":{\"name\":\"shop\",\"by\":\"mtls\",\"subject\":\"CN=shop,O=Acme\\\\, Inc.,C=US\"}}");
    assert_eq!(who(&["CN=admin,CN=evil"]), "401 {\"error\":\"`x-client-dn` gives 2 CNs in the certificate's subject, so who calls is not certain\"}");
    assert!(who(&["CN=evil,OU=x,CN=shop"]).starts_with("401 "));
    assert!(who(&["/C=US/O=Acme/CN=shop"]).contains("send it as RFC 2253 writes it"));
    assert_eq!(who(&["CN=shop", "CN=evil"]), "401 {\"error\":\"`x-client-dn` has 2 lines: a subject is one\"}");
    assert!(who(&[&format!("Hash=4d7c;Subject=\"CN=shop\";URI={}", SHOP_ID)]).starts_with("401 "), "Envoy's header is no subject");
    // The secret saying it is shop, which has a key, with a certificate header that does not
    // parse: the key's refusal is said, not the header's.
    let (a, _) = signed(bp, 3, "whoami", "[]", SECRET, "", "shop", &[("x-forwarded-client-cert", "Subject=\"CN=a\\")]);
    assert_eq!(format!("{} {}", a.status, a.body), NOT_SHOP);
}

#[test]
fn client_certs_chain_is_an_option_accept_calls_knows() {
    let d = dir("chain-option", &[]);
    for (line, why) in [
        ("accept_calls { client_cert: { header: \"x-forwarded-client-cert\", chain: true } }", ""),
        ("accept_calls { client_cert: { header: \"x-forwarded-client-cert\", chain: false } }", ""),
        ("accept_calls { client_cert: { header: \"x-forwarded-client-cert\", chian: true } }", "`client_cert` has no `chian` (header, chain, subject)"),
        ("accept_calls { client_cert: { header: \"x-forwarded-client-cert\", chain: 1 } }", "`client_cert`'s `chain` is true or false"),
        ("accept_calls { client_cert: { header: \"x-client-dn\", subject: true } }", ""),
        ("accept_calls { client_cert: { header: \"x-client-dn\", subject: false } }", ""),
        ("accept_calls { client_cert: { header: \"x-client-dn\", subject: \"yes\" } }", "`client_cert`'s `subject` is true or false"),
        ("accept_calls { client_cert: { header: \"x-client-dn\", subject: true, chain: true } }", "takes `chain` or `subject`, not both"),
    ] {
        std::fs::write(d.join("y.qwe"), format!("{}\nprint(\"loaded\")\n", line)).unwrap();
        let o = quillo(&d, &["run", "y.qwe", "--no-check"], &[]);
        let (so, se) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        if why.is_empty() {
            assert!(so.contains("loaded"), "{}: {}", line, se);
        } else {
            assert!(se.contains(why), "{}: {}", line, se);
        }
    }
}

// ---- Q-989: a subject's CN, read in its form -------------------------------------------------

#[test]
fn a_subjects_cn_is_its_cn_attribute_read_in_its_form() {
    // In Envoy's quotes as written: a subject in RFC 4514's form escapes its quotes itself.
    let name = |subject: &str| quillo::svcauth::cert_caller(&format!("Hash=1;Subject=\"{}\"", subject), false).map(|(n, _)| n);
    let refused = |subject: &str, why: &str| {
        let e = name(subject).unwrap_err();
        assert!(e.contains(why), "{:?}: {}", subject, e);
    };
    // The reviewer's probe, an OU of `a/CN=shop` ahead of the CN, in both forms.
    assert_eq!(name("OU=a/CN=shop,CN=evil").unwrap(), "evil");
    refused("OU=a/CN=shop,O=x", "names no certificate's subject");
    // The slash form has no one reading, and is refused (the third review).
    for slash in ["/OU=a/CN=shop/CN=evil", "/CN=evil/OU=a/CN=shop", "/O=x\\/CN=admin", "/CN=sho\\x70", "/CN=shop+OU=x/O=Acme", "/C=US/O=Acme/CN=shop"] {
        refused(slash, "OpenSSL's slash form");
    }
    // A CN with `:` in it could pass for a URI's name (the third review).
    refused(&format!("CN={}", SHOP_ID), "a CN with `:` in it");
    refused("CN=a:b", "a CN with `:` in it");
    // A CN inside another attribute's value, escaped there, is that value's.
    refused("O=x\\,CN=shop", "names no certificate's subject");
    refused("O=x\\+CN=shop", "names no certificate's subject");
    // Escaped commas, plus signs, quotes and backslashes, in the CN and in the others, decoded.
    assert_eq!(name("CN=Acme\\, Inc.,O=x").unwrap(), "Acme, Inc.");
    assert_eq!(name("CN=a\\+b").unwrap(), "a+b");
    assert_eq!(name("CN=say \\\"hi\\\"").unwrap(), "say \"hi\"");
    assert_eq!(name("CN=back\\\\slash").unwrap(), "back\\slash");
    assert_eq!(name("CN=\\#1\\;2\\<3\\>4\\=5").unwrap(), "#1;2<3>4=5");
    assert_eq!(name("CN=shop,O=Acme\\, Inc.\\+x\\\"y\\\\z").unwrap(), "shop");
    // Hex escapes: a letter, a comma, UTF-8 in two bytes; bytes that are not UTF-8 are refused.
    assert_eq!(name("CN=\\73hop").unwrap(), "shop");
    assert_eq!(name("CN=a\\2Cb").unwrap(), "a,b");
    assert_eq!(name("CN=caf\\C3\\A9").unwrap(), "café");
    assert_eq!(name("CN=café").unwrap(), "café");
    refused("CN=\\FF", "not UTF-8");
    // Two CNs, however written; a CN in hex (BER) is not read.
    refused("CN=a,CN=b", "gives 2 CNs");
    refused("CN=a+CN=b", "gives 2 CNs");
    refused("cn=a,CN=b", "gives 2 CNs");
    refused("CN=a,commonName=b", "gives 2 CNs");
    refused("CN=a,2.5.4.3=b", "gives 2 CNs");
    refused("CN=#0c0473686f70", "in hex");
    // Several attributes in one RDN.
    assert_eq!(name("CN=a+UID=b").unwrap(), "a");
    assert_eq!(name("UID=b+CN=a,O=x").unwrap(), "a");
    // An empty CN names no one; an escaped space is a name.
    refused("CN=,O=x", "an empty CN");
    refused("CN=   ", "an empty CN");
    assert_eq!(name("CN=\\ ").unwrap(), " ");
    assert_eq!(name("CN=shop\\ ").unwrap(), "shop ", "an escaped space at the end is the value's");
    // What does not parse.
    for (bad, why) in [
        ("CN=a\\\"b\\\"c\\\"", ""),
        ("CN=a;O=b", "`;` in a value, not escaped"),
        ("CN=a<b", "`<` in a value"),
        ("CN=a>b", "`>` in a value"),
        ("CN=a\\q", "escapes nothing"),
        // A backslash at the end escapes Envoy's closing quote: the header itself is refused.
        ("CN=a\\", "quote that is not closed"),
        ("CN=a,", "not type=value"),
        ("=a", "no attribute's type"),
        ("C N=a", "no attribute's type"),
        ("CN", "not type=value"),
        ("/CN=a//O=b", "OpenSSL's slash form"),
        ("/", "OpenSSL's slash form"),
    ] {
        if why.is_empty() {
            assert!(name(bad).is_ok(), "{:?}", bad);
        } else {
            refused(bad, why);
        }
    }
    // The forms that work today.
    for (subject, cn) in [
        ("CN=shop", "shop"),
        ("CN=shop,O=Acme", "shop"),
        ("CN=shop, O=Acme", "shop"),
        ("O=Acme,CN=shop", "shop"),
        ("CN = shop , O = Acme", "shop"),
        ("cn=shop", "shop"),
        ("OU=Sales+CN=J. Smith,O=Widget Inc.,C=US", "J. Smith"),
    ] {
        assert_eq!(name(subject).as_deref(), Ok(cn), "{:?}", subject);
    }
    assert_eq!(quillo::svcauth::subject_caller("CN=plain").map(|(n, _)| n).as_deref(), Ok("plain"), "a subject alone, with `subject: true`");
    assert_eq!(name("").unwrap_err(), "names no certificate's subject");
    // Where a URI names the caller, the subject does not: it is shown, not read.
    assert_eq!(quillo::svcauth::cert_caller(&format!("Subject=\"CN=a,CN=b\";URI={}", SHOP_ID), false).map(|(n, _)| n).as_deref(), Ok(SHOP_ID));
}

#[test]
fn an_ou_with_a_cn_in_it_names_no_one_shop() {
    let billing = "accept_calls {\n    client_cert: { header: \"x-forwarded-client-cert\" },\n    shared: false,\n}\nserver fn kshop() for [\"shop\"] {\n    return request.caller.name\n}\n";
    let d = dir("subject-ou", &[("billing.qwe", billing)]);
    let (_b, bp) = serve(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[], "billing.log");
    let by = |subject: &str| by_cert(bp, "kshop", "[]", &format!("By=spiffe://cluster.local/ns/billing/sa/billing;Hash=abc;Subject=\"{}\"", subject));
    assert_eq!(by("OU=a/CN=shop,CN=evil"), "403 {\"error\":\"`kshop` takes calls from shop only; this is `evil`\"}");
    assert!(by("/OU=a/CN=shop/CN=evil").starts_with("401 {\"error\":\"`x-forwarded-client-cert` gives the subject in OpenSSL's slash form"));
    assert_eq!(by("OU=a/CN=shop,O=x"), "401 {\"error\":\"`x-forwarded-client-cert` names no certificate's subject\"}");
    assert_eq!(by("CN=shop,OU=a/CN=evil"), "200 {\"ok\":\"shop\"}");
    assert_eq!(by("CN=Acme\\, Inc."), "403 {\"error\":\"`kshop` takes calls from shop only; this is `Acme, Inc.`\"}");
}

// ---- Q-972: a method is an HTTP token -------------------------------------------------------

/// A server that keeps each request's first line and answers with it; how many connected.
fn recorder() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let s2 = s2.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let head = read_head(&mut s);
                // The body too: one left unread is a reset when the connection closes.
                let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                let _ = s.read_exact(&mut vec![0u8; len]);
                let first = head.lines().next().unwrap_or("").to_string();
                s2.lock().unwrap().push(first.clone());
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", first.len(), first);
            });
        }
    });
    (port, seen)
}

#[test]
fn a_method_that_is_not_an_http_token_is_an_error_value_and_nothing_connects() {
    let (port, seen) = recorder();
    let url = format!("http://127.0.0.1:{}/items/5", port);
    // The audit's, as Quillo text: `\r\n` in the string are a line break.
    let bad = [
        r"GET /admin/delete?user=1 HTTP/1.1\r\nHost: internal\r\nX-Injected: yes\r\nX-Rest:",
        r"GET\r\nX-Injected: yes",
        r"GET\n",
        r"GET\r",
        "",
        " ",
        "GET ",
        "G ET",
        r"G\tET",
        "GÉT",
        "ПОСТ",
        "GET/",
        "(GET)",
        "G:T",
    ];
    for m in bad {
        let got = ev(&format!("[http.request(\"{m}\", {url:?}), http.stream({url:?}, (p) => null, {{ method: \"{m}\", timeout: 5 }}), http.request(\"{m}\", {url:?}, \"body\", {{}}, {{ timeout: 5 }})]"));
        assert_eq!(got.matches("err(\"http: not an HTTP method: ").count(), 3, "{:?}: {}", m, got);
    }
    // Said as written, escaped, and cut short.
    let one = ev(&format!("error(http.request(\"{}\", {:?}))", bad[0], url));
    assert_eq!(one, r#""http: not an HTTP method: \"GET /admin/delete?user=1 HTTP/1.1\\r\\nHost:\"... (a method is letters, digits and !#$%&'*+-.^_`|~)""#);
    assert!(seen.lock().unwrap().is_empty(), "nothing connected: {:?}", seen.lock().unwrap());

    // Every real method, one letter, every sign a token has, a long one, and lower case, which
    // is sent in upper case as before.
    let long = "L".repeat(300);
    for (m, sent) in [
        ("GET", "GET"),
        ("POST", "POST"),
        ("PUT", "PUT"),
        ("PATCH", "PATCH"),
        ("DELETE", "DELETE"),
        ("OPTIONS", "OPTIONS"),
        ("TRACE", "TRACE"),
        ("CONNECT", "CONNECT"),
        ("get", "GET"),
        ("pAtCh", "PATCH"),
        ("propfind", "PROPFIND"),
        ("M-SEARCH", "M-SEARCH"),
        ("X", "X"),
        ("x", "X"),
        ("!#$%&'*+-.^_`|~09", "!#$%&'*+-.^_`|~09"),
        (long.as_str(), long.as_str()),
    ] {
        let got = ev(&format!("http.request({:?}, {:?}, null, {{}}, {{ timeout: 5 }}).body", m, url));
        assert_eq!(got, format!("{:?}", format!("{} /items/5 HTTP/1.1", sent)), "{}", m);
        let streamed = ev(&format!("let got = \"\"\nhttp.stream({:?}, (p) => {{ got = got + p }}, {{ method: {:?}, timeout: 5 }})\ngot", url, m));
        assert_eq!(streamed, format!("{:?}", format!("{} /items/5 HTTP/1.1", sent)), "stream {}", m);
    }
    // HEAD has no body; its status says it was sent.
    assert_eq!(ev(&format!("http.request(\"HEAD\", {:?}, null, {{}}, {{ timeout: 5 }}).status", url)), "200");
    assert!(seen.lock().unwrap().iter().any(|l| l == "HEAD /items/5 HTTP/1.1"));
    // The other calls' methods are their own, and `http.stream` without one is a GET, or a POST
    // with a body, as before.
    assert_eq!(ev(&format!("http.delete({:?}, {{}}, {{ timeout: 5 }}).body", url)), "\"DELETE /items/5 HTTP/1.1\"");
    assert_eq!(ev(&format!("let got = \"\"\nhttp.stream({:?}, (p) => {{ got = got + p }}, {{ body: \"x\", timeout: 5 }})\ngot", url)), "\"POST /items/5 HTTP/1.1\"");
}

// ---- Q-973: a site's calls --------------------------------------------------------------------

/// A Quillo site's wire, by hand: each `/_quillo/rpc/<fn>` answers as the test needs. What came,
/// as `METHOD path body`.
fn site_server(away: u16) -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let s2 = s2.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let head = read_head(&mut s);
                let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                let mut body = vec![0u8; len];
                let _ = s.read_exact(&mut body);
                let mut first = head.split(' ');
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                let body = String::from_utf8_lossy(&body).into_owned();
                s2.lock().unwrap().push(format!("{} {} {}", method, path, body));
                let to = |loc: String| format!("HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", loc);
                let ok_with = |v: String, more: &str| {
                    let j = format!("{{\"ok\":{}}}", v);
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}", more, j.len(), j)
                };
                let ok = |v: String| ok_with(v, "");
                let reply = match path.as_str() {
                    "/_quillo/rpc/notes" => ok(format!("{:?}", format!("{} notes for {}", method, body))),
                    "/_quillo/rpc/moved" => to("/_quillo/rpc/landed\r\nSet-Cookie: hop=1".into()),
                    "/_quillo/rpc/landed" => ok_with(format!("{:?}", format!("landed by {}", method)), "Set-Cookie: landed=1\r\n"),
                    "/_quillo/rpc/kept" => format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:{}/_quillo/rpc/notes\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", port),
                    "/_quillo/rpc/away" => to(format!("http://127.0.0.1:{}/latest/meta-data/\r\nSet-Cookie: went=away", away)),
                    "/_quillo/rpc/cookie" => ok(format!("{:?}", head.lines().find_map(|l| l.strip_prefix("cookie: ").or_else(|| l.strip_prefix("Cookie: "))).unwrap_or("none"))),
                    "/_quillo/rpc/named" => to(format!("http://localhost:{}/_quillo/rpc/notes", port)),
                    "/_quillo/rpc/file" => to("file:///etc/passwd".into()),
                    "/_quillo/rpc/again" => to("/_quillo/rpc/again".into()),
                    _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
                };
                let _ = s.write_all(reply.as_bytes());
            });
        }
    });
    (port, seen)
}

#[test]
fn a_site_follows_redirects_on_its_own_origin_only_and_a_file_one_is_an_error_value() {
    let (away, away_seen) = recorder();
    let (port, seen) = site_server(away);
    let main = format!(
        "site shop = \"http://127.0.0.1:{port}\"\nprint(shop.notes(1))\nprint(shop.moved())\nprint(shop.kept(2))\nprint(shop.away())\nprint(shop.named())\nprint(shop.file())\nprint(shop.again())\nprint(\"after\")\n"
    );
    let d = dir("site-redirects", &[("main.qwe", &main)]);
    let o = quillo(&d, &["run", "main.qwe"], &[]);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(0), "no crash: {}\n{}", out, err);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 8, "{}\n{}", out, err);
    assert_eq!(lines[0], "POST notes for [1]", "a site that nothing refuses");
    assert_eq!(lines[1], "landed by GET", "a 302 on the same origin: followed, as a GET after a POST, as `http` does");
    assert_eq!(lines[2], "POST notes for [2]", "a 307 on the same origin, written in full: the same method and body");
    assert_eq!(lines[3], format!("err(\"site shop: `away` answered 302, sent on to http://127.0.0.1:{}/latest/meta-data/: a site's call follows redirects on the site's own address only\")", away));
    assert_eq!(lines[4], format!("err(\"site shop: `named` answered 302, sent on to http://localhost:{}/_quillo/rpc/notes: a site's call follows redirects on the site's own address only\")", port), "another name of the same machine is another origin");
    assert_eq!(lines[5], "err(\"site shop: `file` answered 302, sent on to file:///etc/passwd: a site's call follows redirects on the site's own address only\")");
    assert!(lines[6].starts_with(&format!("err(\"site shop: no answer from http://127.0.0.1:{}: ", port)) && lines[6].ends_with("more than 5 redirects\")"), "{}", lines[6]);
    assert_eq!(lines[7], "after");
    assert!(away_seen.lock().unwrap().is_empty(), "the other address was never asked: {:?}", away_seen.lock().unwrap());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.iter().filter(|l| l.starts_with("POST /_quillo/rpc/again") || l.starts_with("GET /_quillo/rpc/again")).count(), 6, "the first call and 5 redirects: {:?}", seen);
    assert!(!seen.iter().any(|l| l.contains("/_quillo/rpc/notes") && l.starts_with("GET")), "the localhost one was not followed: {:?}", seen);
}

#[test]
fn the_cookies_of_every_answer_from_the_sites_own_address_are_kept_those_on_the_way_too() {
    let (away, away_seen) = recorder();
    let (port, _) = site_server(away);
    let main = format!("site shop = \"http://127.0.0.1:{port}\"\nprint(shop.cookie())\nprint(shop.moved())\nprint(shop.cookie())\nprint(is_err(shop.away()))\nprint(shop.cookie())\n");
    let d = dir("site-cookies", &[("main.qwe", &main)]);
    // Where the program is one person's, as a script used as a tool says. The redirect on the
    // site's own address sets `hop`, the answer it leads to `landed` (the client kept only the
    // last answer's); the redirect elsewhere, an answer of the site's too, `went`. Nothing from
    // the other address: it is never asked.
    let o = quillo(&d, &["run", "main.qwe"], &[("QUILLO_SITE_COOKIES", "1")]);
    assert_eq!(String::from_utf8_lossy(&o.stdout), "none\nlanded by GET\nhop=1; landed=1\ntrue\nhop=1; landed=1; went=away\n", "{}", String::from_utf8_lossy(&o.stderr));
    assert!(away_seen.lock().unwrap().is_empty());
}

#[test]
fn a_site_goes_through_the_proxy_says_quillo_and_reads_64_mib_at_most_as_http_does() {
    // A proxy answering for any site: the request line and User-Agent it was sent. Its `big`
    // answer is a byte over 64 MiB.
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let a2 = asked.clone();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let pp = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let a2 = a2.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let head = read_head(&mut s);
                let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                let _ = s.read_exact(&mut vec![0u8; len]);
                let first = head.lines().next().unwrap_or("").to_string();
                let agent = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("user-agent:").map(|v| v.trim().to_string())).unwrap_or_default();
                a2.lock().unwrap().push(format!("{} | {}", first, agent));
                if first.contains("/_quillo/rpc/big") {
                    let n = (64usize << 20) + 1;
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", n);
                    let chunk = vec![b' '; 1 << 20];
                    let mut left = n;
                    while left > 0 {
                        let k = left.min(chunk.len());
                        if s.write_all(&chunk[..k]).is_err() {
                            return;
                        }
                        left -= k;
                    }
                } else {
                    let j = r#"{"ok":"through the proxy"}"#;
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", j.len(), j);
                }
            });
        }
    });
    let proxy = format!("http://127.0.0.1:{}", pp);
    // `.invalid` is no host anywhere (RFC 6761): only the proxy can answer for it.
    let d = dir("site-proxy", &[("main.qwe", "site shop = \"http://shop.invalid\"\nprint(shop.notes(1))\nprint(shop.big())\n")]);
    let o = quillo(&d, &["run", "main.qwe"], &[("HTTP_PROXY", &proxy)]);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.first(), Some(&"through the proxy"), "{}\n{}", out, String::from_utf8_lossy(&o.stderr));
    assert_eq!(lines.get(1).copied(), Some("err(\"site shop: no answer from http://shop.invalid: http://shop.invalid/_quillo/rpc/big: the answer is longer than 64 MiB, the most a site's call reads\")"), "{}", out);
    let agent = format!("quillo/{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(asked.lock().unwrap().first().cloned(), Some(format!("POST http://shop.invalid/_quillo/rpc/notes HTTP/1.1 | {}", agent)));
    // `NO_PROXY` naming the site: asked for directly, where there is no such host.
    let before = asked.lock().unwrap().len();
    let o = quillo(&d, &["run", "main.qwe"], &[("HTTP_PROXY", &proxy), ("NO_PROXY", "shop.invalid")]);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.lines().next().unwrap_or("").starts_with("err(\"site shop: no answer from http://shop.invalid: "), "{}", out);
    assert_eq!(asked.lock().unwrap().len(), before, "not through the proxy");
}

/// A server whose `/slow/<n>` waits 400 ms and sends on to `/slow/<n + 1>`, and answers at 4.
fn slow_redirects() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let head = read_head(&mut s);
                let n: u32 = head.split(' ').nth(1).and_then(|p| p.strip_prefix("/slow/")).and_then(|n| n.parse().ok()).unwrap_or(0);
                std::thread::sleep(Duration::from_millis(400));
                let reply = if n >= 4 {
                    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone".to_string()
                } else {
                    format!("HTTP/1.1 302 Found\r\nLocation: /slow/{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", n + 1)
                };
                let _ = s.write_all(reply.as_bytes());
            });
        }
    });
    port
}

#[test]
fn a_calls_timeout_is_one_for_all_its_redirects() {
    // What a site's call is held to, its 30 s, is `follow`'s `timeout`: tried here at 1 s over
    // five answers of 400 ms each, which each had a whole second before.
    let port = slow_redirects();
    let t = Instant::now();
    let got = ev(&format!("http.get(\"http://127.0.0.1:{}/slow/0\", {{}}, {{ timeout: 1 }})", port));
    let took = t.elapsed();
    assert!(got.contains("took more than 1 s (its timeout)"), "{}", got);
    assert!(took < Duration::from_millis(1800), "{:?}", took);
    // Time enough for all of them.
    assert_eq!(ev(&format!("http.get(\"http://127.0.0.1:{}/slow/0\", {{}}, {{ timeout: 5 }}).body", port)), "\"done\"");
    // And a stream's, the same.
    let streamed = ev(&format!("http.stream(\"http://127.0.0.1:{}/slow/0\", (p) => null, {{ timeout: 1 }})", port));
    assert!(streamed.contains("took more than 1 s (its timeout)"), "{}", streamed);
}

#[test]
fn a_file_redirect_in_a_route_is_what_the_fallback_gives_not_a_500() {
    let (away, _) = recorder();
    let (port, _) = site_server(away);
    let server = format!("site shop = \"http://127.0.0.1:{port}\"\nroute GET /file = `${{shop.file() ?? \"fallback\"}}`\nroute GET /notes = `${{shop.notes(3) ?? \"fallback\"}}`\n");
    let d = dir("site-route", &[("server.qwe", &server)]);
    let (_s, sp) = serve(&d, "server.qwe", &[], &[], "server.log");
    let r = send(sp, "GET", "/file", &[], "");
    assert_eq!((r.status, r.body.as_str()), (200, "fallback"), "{}", std::fs::read_to_string(d.join("server.log")).unwrap_or_default());
    assert_eq!(send(sp, "GET", "/notes", &[], "").body, "POST notes for [3]");
    assert!(!std::fs::read_to_string(d.join("server.log")).unwrap_or_default().contains("internal error"));
}

#[test]
fn the_hosts_policy_holds_for_a_site_as_for_http() {
    let (away, _) = recorder();
    let (port, seen) = site_server(away);
    let main = format!("site shop = \"http://127.0.0.1:{port}\"\nprint(http.get(\"http://127.0.0.1:{port}/direct\"))\nprint(shop.notes(1))\nprint(shop.moved())\n");
    // A policy that lists other hosts: refused, as `http.get` is, and nothing is asked.
    let d = dir("site-hosts", &[("main.qwe", &main), ("quillo.policy", "[http]\nhosts = [\"api.example.com\"]\n")]);
    let o = quillo(&d, &["run", "main.qwe"], &[]);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.first(), Some(&"err(\"http: 127.0.0.1 is not among the hosts quillo.policy allows: api.example.com\")"), "{}\n{}", out, String::from_utf8_lossy(&o.stderr));
    assert_eq!(lines.get(1), Some(&"err(\"site shop: not called: 127.0.0.1 is not among the hosts quillo.policy allows: api.example.com\")"), "{}", out);
    assert_eq!(lines.get(2), Some(&"err(\"site shop: not called: 127.0.0.1 is not among the hosts quillo.policy allows: api.example.com\")"), "{}", out);
    assert!(seen.lock().unwrap().is_empty(), "{:?}", seen.lock().unwrap());
    // `private = false`: this machine is not reached.
    let d = dir("site-private", &[("main.qwe", &main), ("quillo.policy", "[http]\nprivate = false\n")]);
    let o = quillo(&d, &["run", "main.qwe"], &[]);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines: Vec<&str> = out.lines().collect();
    let no = "127.0.0.1 is an address of this machine or its network, which quillo.policy does not let a program reach";
    assert_eq!(lines.get(1).copied(), Some(format!("err(\"site shop: not called: {}\")", no).as_str()), "{}\n{}", out, String::from_utf8_lossy(&o.stderr));
    assert!(seen.lock().unwrap().is_empty(), "{:?}", seen.lock().unwrap());
    // A policy that lists the site: called, and its redirect judged and followed.
    let d = dir("site-listed", &[("main.qwe", &main), ("quillo.policy", "[http]\nhosts = [\"127.0.0.1\"]\n")]);
    let o = quillo(&d, &["run", "main.qwe"], &[]);
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(&lines[1..], ["POST notes for [1]", "landed by GET"], "{}\n{}", out, String::from_utf8_lossy(&o.stderr));
}

// ---- Q-974: `cache` and certificates ---------------------------------------------------------

/// A CA, a server certificate for 127.0.0.1 and client certificates for alice and bob, in `d`;
/// an HTTPS server that answers `balance of <who>` by the certificate it was shown (none:
/// `anonymous`), and how many it answered.
fn bank(d: &Path) -> (u16, Arc<AtomicUsize>) {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca.distinguished_name.push(rcgen::DnType::CommonName, "feedback71b ca");
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    std::fs::write(d.join("ca.pem"), ca_cert.pem()).unwrap();
    let ca_der: CertificateDer<'static> = ca_cert.der().clone();
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()]).unwrap().signed_by(&server_key, &issuer).unwrap();
    let mut clients: Vec<(Vec<u8>, &'static str)> = Vec::new();
    for who in ["alice", "bob"] {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        p.distinguished_name.push(rcgen::DnType::CommonName, who);
        let cert = p.signed_by(&key, &issuer).unwrap();
        std::fs::write(d.join(format!("{}.pem", who)), cert.pem()).unwrap();
        std::fs::write(d.join(format!("{}.key", who)), key.serialize_pem()).unwrap();
        clients.push((cert.der().to_vec(), who));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_der).unwrap();
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone()).allow_unauthenticated().build().unwrap();
    let cfg = Arc::new(
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![server_cert.der().clone()], PrivateKeyDer::Pkcs8(server_key.serialize_der().into()))
            .unwrap(),
    );
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let served = Arc::new(AtomicUsize::new(0));
    let n = served.clone();
    let clients = Arc::new(clients);
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { return };
            let (cfg, n, clients) = (cfg.clone(), n.clone(), clients.clone());
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let Ok(conn) = rustls::ServerConnection::new(cfg) else { return };
                let mut tls = rustls::StreamOwned::new(conn, s);
                let head = read_head(&mut tls);
                if head.is_empty() {
                    return;
                }
                let shown = tls.conn.peer_certificates().and_then(|c| c.first().map(|c| c.to_vec()));
                let who = shown.and_then(|c| clients.iter().find(|(der, _)| *der == c).map(|(_, w)| *w)).unwrap_or("anonymous");
                n.fetch_add(1, Ordering::SeqCst);
                let body = format!("balance of {}", who);
                let _ = write!(tls, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                tls.conn.send_close_notify();
                let _ = tls.flush();
            });
        }
    });
    (port, served)
}

#[test]
fn a_gets_body_and_every_part_of_the_call_are_in_its_cached_answers_key() {
    let (port, seen) = recorder();
    let url = format!("http://127.0.0.1:{}/search", port);
    // The reviewer's probe: two queries in a GET's body; and a URL with a line break (which the
    // client drops) against headers that held the rest of it.
    let src = format!(
        r#"let url = {url:?}
let o = {{ cache: 60, timeout: 5 }}
http.request("GET", url, "query-for-alice", {{}}, o)
http.request("GET", url, "query-for-bob", {{}}, o)
http.request("GET", url, "query-for-alice", {{}}, o)
http.get(url, {{}}, o)
http.request("GET", url, "", {{}}, o)
http.request("GET", url, {{ q: "query-for-alice" }}, {{}}, o)
http.request("GET", url, "query-for-alice", {{}}, o)
http.get("http://127.0.0.1:{port}/a 5 false 60 0 0 false
k:v", {{ z: "1" }}, o)
http.get("http://127.0.0.1:{port}/a", {{ k: "v 5 false 60 0 0 false", z: "1" }}, o)
http.get("http://127.0.0.1:{port}/a", {{ k: "v 5 false 60 0 0 false", z: "1" }}, o)
1"#
    );
    assert_eq!(ev(&src), "1");
    let seen = seen.lock().unwrap();
    // alice, bob, none, an empty body, JSON, the URL with a line break, the headers: alice's
    // and the headers' again were kept.
    assert_eq!(seen.len(), 7, "{:?}", seen);
}

#[test]
fn a_cached_answer_is_given_only_to_calls_with_the_same_certificates() {
    let d = dir("mtls", &[]);
    let (port, served) = bank(&d);
    let p = |f: &str| format!("{:?}", d.join(f).display().to_string());
    let (ca, alice, alice_key, bob, bob_key) = (p("ca.pem"), p("alice.pem"), p("alice.key"), p("bob.pem"), p("bob.key"));
    // The audit's calls, and one either side: the CA alone, alice again, alice with another
    // timeout.
    let src = format!(
        r#"let url = "https://127.0.0.1:{port}/balance"
let alice = http.get(url, {{}}, {{ cache: 60, timeout: 5, ca: {ca}, cert: {alice}, key: {alice_key} }}).body
let bob = http.get(url, {{}}, {{ cache: 60, timeout: 5, ca: {ca}, cert: {bob}, key: {bob_key} }}).body
let none = http.get(url, {{}}, {{ cache: 60, timeout: 5 }})
let ca_only = http.get(url, {{}}, {{ cache: 60, timeout: 5, ca: {ca} }}).body
let again = http.get(url, {{}}, {{ cache: 60, timeout: 5, ca: {ca}, cert: {alice}, key: {alice_key} }}).body
let bob_again = http.get(url, {{}}, {{ cache: 60, timeout: 5, ca: {ca}, cert: {bob}, key: {bob_key} }}).body
let longer = http.get(url, {{}}, {{ cache: 60, timeout: 6, ca: {ca}, cert: {alice}, key: {alice_key} }}).body
[alice, bob, is_err(none), ca_only, again, bob_again, longer]"#
    );
    assert_eq!(ev(&src), r#"["balance of alice", "balance of bob", true, "balance of anonymous", "balance of alice", "balance of bob", "balance of alice"]"#);
    assert_eq!(served.load(Ordering::SeqCst), 4, "alice, bob, the CA alone and alice with another timeout went out; the two again were kept");
}
