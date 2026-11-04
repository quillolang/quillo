//! FEEDBACK38.md: authentication between services (Q-458 to Q-464; branch feedback36-39): a key
//! for each caller, `for [...]`, `request.caller`, tokens, client certificates from a proxy, a
//! check of the program's own, rotation, joining as a service, and answers signed back.

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::send;

const SHOP: &str = "shop-key-0123456789abcdef0123456789";
const SHOP_OLD: &str = "old-shop-key-0123456789abcdef01234";
const NOTIFY: &str = "notify-key-0123456789abcdef012345";
const JWT: &str = "jwt-secret-0123456789abcdef0123456";

fn dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = support::temp_dir(&format!("feedback38-{}", name));
    for (f, text) in files {
        std::fs::write(d.join(f), text).unwrap();
    }
    d
}

/// `quillo run` with these arguments and this environment, once it listens.
fn run(d: &Path, file: &str, args: &[&str], env: &[(&str, &str)]) -> (support::Guard, u16) {
    let port = support::free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .arg("run")
        .arg(file)
        .args(["--port", &port.to_string()])
        .args(args)
        .current_dir(d)
        .env_remove("QUILLO_SERVICE_SECRET")
        .envs(env.iter().copied())
        .env("SHOP_KEY", SHOP)
        .env("SHOP_KEY_OLD", SHOP_OLD)
        .env("NOTIFY_KEY", NOTIFY)
        .env("JWT_KEY", JWT)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if std::env::var("QUILLO_TEST_LOG").is_ok() { Stdio::inherit() } else { Stdio::null() })
        .spawn()
        .unwrap();
    let g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "{} did not start", file);
        std::thread::sleep(Duration::from_millis(20));
    }
    (g, port)
}

fn b64(b: &[u8]) -> String {
    quillo::crypto::b64url(b)
}

/// An HS256 token for `claims`, signed with `key`.
fn jwt(claims: &str, key: &str) -> String {
    let m = format!("{}.{}", b64(br#"{"alg":"HS256","typ":"JWT"}"#), b64(claims.as_bytes()));
    format!("{}.{}", m, b64(&quillo::crypto::hmac_sha256(key.as_bytes(), m.as_bytes())))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// A call to a service with no signature of Quillo's: `headers` alone prove who calls.
fn call(port: u16, to: &str, f: &str, headers: &[(&str, &str)]) -> support::Answer {
    let id = format!("c{}", fastrand_id());
    let mut hs: Vec<(&str, &str)> = vec![("x-quillo-to", to), ("x-quillo-call", &id)];
    hs.extend_from_slice(headers);
    send(port, "POST", &format!("/_quillo/service/{}", f), &hs, "[]")
}

fn fastrand_id() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!("{}{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
}

const BILLING: &str = r#"accept_calls {
    keys: { shop: [env("SHOP_KEY"), env("SHOP_KEY_OLD")], notify: env("NOTIFY_KEY") },
    jwt: { secret: env("JWT_KEY"), audience: "billing", issuer: "idp" },
    client_cert: { header: "x-forwarded-client-cert" },
    check: (caller) => {
        if caller.name == "mallory" { return false }
        if caller.name == "eve" { reply 418 { error: "not you" } }
        { ...caller, checked: true }
    },
}
server fn refund(account: int, cents: int) for ["shop"] {
    { refunded: cents, caller: request.caller.name, by: request.caller.by }
}
server fn whoami() {
    { caller: request.caller, forwarded: request.forwarded, user: request.user }
}
"#;

#[test]
fn each_caller_has_a_key_and_a_server_fn_names_its_callers() {
    let d = dir(
        "keys",
        &[
            ("billing.qwe", BILLING),
            ("main.qwe", "service billing = \"billing.qwe\" { auth: { key: env(\"SHOP_KEY\"), as: \"shop\" }, forward: [\"authorization\"] }\nroute GET /refund = billing.refund(7, 500)\nroute GET /who = billing.whoami()\n"),
            ("old.qwe", "service billing = \"billing.qwe\" { auth: { key: env(\"SHOP_KEY_OLD\"), as: \"shop\" } }\nroute GET /who = billing.whoami()\n"),
            ("notify.qwe", "service billing = \"billing.qwe\" { auth: { key: env(\"NOTIFY_KEY\"), as: \"notify\" } }\nroute GET /refund = billing.refund(7, 99999)\nroute GET /who = billing.whoami()\n"),
            ("forged.qwe", "service billing = \"billing.qwe\" { auth: { key: env(\"NOTIFY_KEY\"), as: \"shop\" } }\nroute GET /who = billing.whoami()\n"),
        ],
    );
    let (_b, bp) = run(&d, "billing.qwe", &["--as-service", "--trust-proxy", "127.0.0.1"], &[]);
    let url = format!("http://127.0.0.1:{}", bp);
    let env = [("QUILLO_SERVICE_BILLING", url.as_str())];
    let (_a, ap) = run(&d, "main.qwe", &[], &env);
    let (_o, op) = run(&d, "old.qwe", &[], &env);
    let (_n, np) = run(&d, "notify.qwe", &[], &env);
    let (_f, fp) = run(&d, "forged.qwe", &[], &env);
    assert_eq!(send(ap, "GET", "/refund", &[], "").body, r#"{"refunded":500,"caller":"shop","by":"key"}"#, "Q-458");
    assert_eq!(send(op, "GET", "/who", &[], "").body, r#"{"caller":{"name":"shop","by":"key","checked":true},"forwarded":{},"user":null}"#, "an older key of shop's, while it is rotated (Q-461)");
    assert_eq!(send(np, "GET", "/refund", &[], "").status, 500, "notify may not refund");
    assert!(send(np, "GET", "/who", &[], "").body.contains(r#""name":"notify""#));
    assert_eq!(send(fp, "GET", "/who", &[], "").status, 500, "notify's key does not sign as shop");
    // The visitor's token goes along, and is checked: request.user (Q-460).
    let tok = jwt(&format!(r#"{{"sub":"alice","iss":"idp","aud":"billing","exp":{}}}"#, now() + 300), JWT);
    let who = send(ap, "GET", "/who", &[("Authorization", &format!("Bearer {}", tok))], "").body;
    assert!(who.contains(r#""user":{"sub":"alice""#) && who.contains(r#""forwarded":{"authorization":"Bearer "#), "{}", who);

    // A bearer token alone proves the caller: its claims are request.caller's.
    let svc = format!(r#"{{"client_id":"reports","iss":"idp","aud":"billing","exp":{},"scope":"billing:read billing:write"}}"#, now() + 300);
    let r = call(bp, "billing", "whoami", &[("Authorization", &format!("Bearer {}", jwt(&svc, JWT)))]);
    assert!(r.body.contains(r#""caller":{"name":"reports","by":"jwt","scopes":["billing:read","billing:write"]"#), "{}", r.body);
    for (claims, why) in [
        (format!(r#"{{"sub":"x","iss":"idp","aud":"orders","exp":{}}}"#, now() + 300), "is not for billing"),
        (format!(r#"{{"sub":"x","iss":"other","aud":"billing","exp":{}}}"#, now() + 300), "another issuer"),
        (r#"{"sub":"x","iss":"idp","aud":"billing"}"#.to_string(), "no exp"),
        (format!(r#"{{"sub":"x","iss":"idp","aud":"billing","exp":{}}}"#, now() - 600), "expired"),
        (format!(r#"{{"sub":"x","iss":"idp","aud":["a","billing"],"exp":{},"nbf":{}}}"#, now() + 300, now() + 600), "not good yet"),
    ] {
        let r = call(bp, "billing", "whoami", &[("Authorization", &format!("Bearer {}", jwt(&claims, JWT)))]);
        assert!(r.status == 401 && r.body.contains(why), "{}: {}", why, r.body);
    }
    let r = call(bp, "billing", "whoami", &[("Authorization", &format!("Bearer {}x", jwt(&svc, JWT)))]);
    assert!(r.body.contains("the token's signature is wrong"), "{}", r.body);
    let five = call(bp, "billing", "whoami", &[("Authorization", "Bearer not.a.jwt.at.all")]);
    assert!(five.body.contains("three parts"), "{} {}", five.status, five.body);
    let unsigned = format!("{}.{}.", b64(br#"{"alg":"none"}"#), b64(svc.as_bytes()));
    assert!(call(bp, "billing", "whoami", &[("Authorization", &format!("Bearer {}", unsigned))]).body.contains("an unsigned token"));
    let rs = format!("{}.{}.AA", b64(br#"{"alg":"RS256"}"#), b64(svc.as_bytes()));
    assert!(call(bp, "billing", "whoami", &[("Authorization", &format!("Bearer {}", rs))]).body.contains("no jwks to check it with"));

    // A certificate a proxy checked, from a proxy --trust-proxy names (Q-462).
    let xfcc = "By=spiffe://x/billing;Hash=ab;Subject=\"CN=shop,O=Acme\";URI=spiffe://cluster.local/ns/prod/sa/shop";
    let r = call(bp, "billing", "whoami", &[("x-forwarded-client-cert", xfcc)]);
    assert!(r.body.contains(r#""name":"shop","by":"mtls","subject":"CN=shop,O=Acme","uri":"spiffe://cluster.local/ns/prod/sa/shop""#), "{}", r.body);
    let r = call(bp, "billing", "whoami", &[("x-forwarded-client-cert", "Subject=\"CN=ops\"")]);
    assert!(r.body.contains(r#""name":"ops","by":"mtls""#), "{}", r.body);
    assert!(call(bp, "billing", "whoami", &[("x-forwarded-client-cert", "Hash=ab")]).body.contains("names no certificate's subject"));
    // The check of the program's own.
    let r = call(bp, "billing", "whoami", &[("x-forwarded-client-cert", "Subject=\"CN=mallory\"")]);
    assert_eq!(r.status, 403, "{}", r.body);
    let r = call(bp, "billing", "whoami", &[("x-forwarded-client-cert", "Subject=\"CN=eve\"")]);
    assert_eq!((r.status, r.body.as_str()), (418, r#"{"error":"not you"}"#));
    // No proof at all; and the shared secret, which billing has none of.
    assert!(call(bp, "billing", "whoami", &[]).body.contains("this call proves no caller"));

    // The document says who may call what (Q-458).
    let doc = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["docs", "billing.qwe"]).current_dir(&d).output().unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&doc.stdout).unwrap();
    assert_eq!(doc["x-quillo-server-fns"]["refund"]["x-quillo-callers"], serde_json::json!(["shop"]));
}

#[test]
fn a_proxy_header_is_believed_only_from_a_trusted_proxy_and_the_shared_secret_can_be_refused() {
    let d = dir(
        "proxy",
        &[("billing.qwe", "accept_calls { shared: false, client_cert: { header: \"x-forwarded-client-cert\" } }\nserver fn whoami() = request.caller\n"), ("main.qwe", "service billing = \"billing.qwe\"\nroute GET /who = billing.whoami()\n")],
    );
    let secret = [("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef")];
    let (_b, bp) = run(&d, "billing.qwe", &["--as-service"], &secret);
    let r = call(bp, "billing", "whoami", &[("x-forwarded-client-cert", "Subject=\"CN=shop\"")]);
    assert!(r.body.contains("is believed only from a proxy --trust-proxy names"), "{}", r.body);
    let r = call(bp, "billing", "whoami", &[("x-forwarded-client-cert", "Subject=\"CN=shop\""), ("x-quillo-from-trusted-proxy", "1")]);
    assert!(r.body.contains("is believed only from a proxy"), "the marker cannot be sent by a client: {}", r.body);
    let url = format!("http://127.0.0.1:{}", bp);
    let (_a, ap) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_BILLING", url.as_str()), secret[0]]);
    assert_eq!(send(ap, "GET", "/who", &[], "").status, 500, "shared: false");
}

#[test]
fn the_shared_secret_signs_the_caller_and_rotates() {
    let d = dir("shared", &[("billing.qwe", "server fn whoami() = request.caller\n"), ("main.qwe", "service billing = \"billing.qwe\"\nroute GET /who = billing.whoami()\n")]);
    let (_b, bp) = run(&d, "billing.qwe", &["--as-service"], &[("QUILLO_SERVICE_SECRET", "new-secret-0123456789abcdef0123456,old-secret-0123456789abcdef0123456")]);
    let url = format!("http://127.0.0.1:{}", bp);
    let (_a, ap) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_BILLING", url.as_str()), ("QUILLO_SERVICE_SECRET", "old-secret-0123456789abcdef0123456")]);
    assert_eq!(send(ap, "GET", "/who", &[], "").body, r#"{"name":"main","by":"quillo"}"#, "signed with the old one, taken (Q-461)");
    let (_c, cp) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_BILLING", url.as_str()), ("QUILLO_SERVICE_SECRET", "new-secret-0123456789abcdef0123456"), ("QUILLO_SERVICE_SECRET_PREVIOUS", "x")]);
    assert!(send(cp, "GET", "/who", &[], "").body.contains("\"by\":\"quillo\""));
    let (_e, ep) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_BILLING", url.as_str()), ("QUILLO_SERVICE_SECRET", "another-secret-0123456789abcdef01")]);
    assert_eq!(send(ep, "GET", "/who", &[], "").status, 500);
}

/// A token endpoint for the client credentials grant: the first token it gives has expired.
fn token_endpoint() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let given = n.clone();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut b = [0u8; 4096];
            let got = c.read(&mut b).unwrap_or(0);
            let req = String::from_utf8_lossy(&b[..got]).into_owned();
            let k = given.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let ok = req.contains("grant_type=client_credentials") && (req.contains("client_secret=s3cret") || req.contains("authorization: Basic"));
            let exp = if k == 0 { now() - 600 } else { now() + 600 };
            let body = if ok {
                format!(r#"{{"access_token":"{}","expires_in":3600}}"#, jwt(&format!(r#"{{"client_id":"shop","iss":"idp","aud":"billing","exp":{},"scope":"billing"}}"#, exp), JWT))
            } else {
                r#"{"error":"invalid_client"}"#.to_string()
            };
            let status = if ok { "200 OK" } else { "401 No" };
            let _ = write!(c, "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, body.len(), body);
        }
    });
    (port, n)
}

#[test]
fn oauth_tokens_are_fetched_kept_and_fetched_again_when_refused() {
    let (tp, given) = token_endpoint();
    let main = format!(
        "service billing = \"billing.qwe\" {{ auth: {{ oauth: {{ token_url: \"http://127.0.0.1:{}/token\", client_id: \"shop\", client_secret: \"s3cret\", scope: \"billing\", audience: \"billing\" }} }} }}\nroute GET /who = billing.whoami()\n",
        tp
    );
    let basic = main.replace("audience: \"billing\"", "audience: \"billing\", method: \"basic\"");
    let bad = main.replace("s3cret", "wrong");
    let bearer = "fn token() = env(\"TOKEN\")\nservice billing = \"billing.qwe\" { auth: { bearer: token } }\nroute GET /who = billing.whoami()\n";
    let d = dir(
        "oauth",
        &[
            ("billing.qwe", "accept_calls { jwt: { secret: env(\"JWT_KEY\"), audience: \"billing\", issuer: \"idp\" } }\nserver fn whoami() = request.caller.name + \" \" + str(request.caller.scopes)\n"),
            ("main.qwe", &main),
            ("basic.qwe", &basic),
            ("bad.qwe", &bad),
            ("bearer.qwe", bearer),
        ],
    );
    let (_b, bp) = run(&d, "billing.qwe", &["--as-service"], &[]);
    let url = format!("http://127.0.0.1:{}", bp);
    let env = [("QUILLO_SERVICE_BILLING", url.as_str())];
    let (_a, ap) = run(&d, "main.qwe", &[], &env);
    assert_eq!(send(ap, "GET", "/who", &[], "").body, "shop [\"billing\"]", "the first token had expired: fetched again, once");
    assert_eq!(send(ap, "GET", "/who", &[], "").body, "shop [\"billing\"]");
    assert_eq!(given.load(std::sync::atomic::Ordering::SeqCst), 2, "the second call took the kept token");
    let (_k, kp) = run(&d, "basic.qwe", &[], &env);
    assert_eq!(send(kp, "GET", "/who", &[], "").body, "shop [\"billing\"]");
    let (_x, xp) = run(&d, "bad.qwe", &[], &env);
    assert_eq!(send(xp, "GET", "/who", &[], "").status, 500);
    let tok = jwt(&format!(r#"{{"sub":"svc","iss":"idp","aud":"billing","exp":{}}}"#, now() + 300), JWT);
    let mut e = env.to_vec();
    e.push(("TOKEN", &tok));
    let (_t, tp2) = run(&d, "bearer.qwe", &[], &e);
    assert_eq!(send(tp2, "GET", "/who", &[], "").body, "svc []");
}

/// An impostor of a service: answers every call, unsigned.
fn impostor() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut b = [0u8; 8192];
            let _ = c.read(&mut b);
            let body = r#"{"ok":{"refunded":0,"by":"an impostor"}}"#;
            let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    port
}

#[test]
fn an_answer_is_the_services_and_a_copy_joins_with_its_own_key() {
    let fake = impostor();
    let fake_url = format!("http://127.0.0.1:{}", fake);
    let d = dir(
        "join",
        &[
            ("billing.qwe", "accept_calls { keys: { app: env(\"SHOP_KEY\") } }\nserver fn refund() = { refunded: 500 }\n"),
            ("main.qwe", "accept_calls { keys: { billing: env(\"NOTIFY_KEY\") } }\nservice billing = \"billing.qwe\" { auth: { key: env(\"SHOP_KEY\"), as: \"app\" } }\nroute GET /refund = billing.refund()\n"),
            ("shared.qwe", "service billing = \"billing.qwe\"\nroute GET /refund = billing.refund()\n"),
        ],
    );
    // A call with a key of its own takes no answer the service did not sign (Q-459).
    let (_a, ap) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_BILLING", fake_url.as_str())]);
    assert_eq!(send(ap, "GET", "/refund", &[], "").status, 500, "the impostor's unsigned answer is refused");
    // The real one, signed back.
    let (_b, bp) = run(&d, "billing.qwe", &["--as-service"], &[]);
    let url = format!("http://127.0.0.1:{}", bp);
    let (_c, cp) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_BILLING", url.as_str())]);
    assert_eq!(send(cp, "GET", "/refund", &[], "").body, r#"{"refunded":500}"#);
    let answer = call(bp, "billing", "refund", &[]);
    assert_eq!(answer.status, 401);

    // Joining: billing's key (NOTIFY_KEY here) proves a copy is billing; the shared secret does not.
    let secret = ("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef");
    let (_app, app_port) = run(&d, "main.qwe", &[], &[secret]);
    let app = format!("http://127.0.0.1:{}", app_port);
    let join = |key: Option<&str>, args: &[&str]| -> String {
        let port = support::free_port();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_quillo"));
        cmd.args(["run", "billing.qwe", "--as-service", "--port", &port.to_string(), "--join", &app]).args(args).current_dir(&d).env(secret.0, secret.1).env("SHOP_KEY", SHOP).stderr(Stdio::piped()).stdout(Stdio::null());
        if let Some(k) = key {
            cmd.env("QUILLO_SERVICE_KEY", k);
        }
        let mut child = cmd.spawn().unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let _ = child.kill();
        let mut e = String::new();
        let _ = child.stderr.take().unwrap().read_to_string(&mut e);
        let _ = child.wait();
        e
    };
    let refused = join(None, &[]);
    assert!(refused.contains("signed with its key (QUILLO_SERVICE_KEY on the copy)"), "{}", refused);
    let wrong = join(Some("not-billings-key-0123456789abcdef0"), &[]);
    assert!(wrong.contains("not signed with"), "{}", wrong);
    let joined = join(Some(NOTIFY), &[]);
    assert!(joined.contains("joined"), "{}", joined);
    let elsewhere = join(Some(NOTIFY), &["--join-as", "https://example.test:8443"]);
    assert!(elsewhere.contains("which is not where it joined from"), "{}", elsewhere);
    let here = join(Some(NOTIFY), &["--join-as", "http://127.0.0.1:1"]);
    assert!(here.contains("joined"), "{}", here);
    // An app that takes no copies, or only some.
    let (_no, no_port) = run(&d, "shared.qwe", &["--no-join"], &[secret]);
    let raw_join = |port: u16| send(port, "POST", "/_quillo/join", &[("x-quillo-service", "t=1,sig=x"), ("x-quillo-call", "abc")], "{\"service\":\"billing\",\"port\":1}");
    assert!(raw_join(no_port).body.contains("this app takes no copies (--no-join)"));
    let (_some, some_port) = run(&d, "shared.qwe", &["--allow-join", "risk"], &[secret]);
    assert!(raw_join(some_port).body.contains("this app takes copies of risk only (--allow-join)"));
}

// FEEDBACK49.md, Q-596: under the shared secret, an unsigned answer from whatever answered at
// the service's address was taken as the service's.
#[test]
fn under_the_shared_secret_an_unsigned_answer_is_refused_too() {
    let fake = impostor();
    let fake_url = format!("http://127.0.0.1:{}", fake);
    let d = dir(
        "shared-answer",
        &[
            ("billing.qwe", "server fn refund() = { refunded: 500 }\n"),
            ("main.qwe", "service billing = \"billing.qwe\"\nroute GET /refund = billing.refund()\n"),
        ],
    );
    let secret = ("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcdef");
    let (_a, ap) = run(&d, "main.qwe", &[], &[secret, ("QUILLO_SERVICE_BILLING", fake_url.as_str())]);
    let a = send(ap, "GET", "/refund", &[], "");
    assert_eq!(a.status, 500, "the impostor's unsigned answer is refused: {}", a.body);
    assert!(!a.body.contains("an impostor"), "{}", a.body);
    // The real service, under the same secret, signs back.
    let (_b, bp) = run(&d, "billing.qwe", &["--as-service"], &[secret]);
    let url = format!("http://127.0.0.1:{}", bp);
    let (_c, cp) = run(&d, "main.qwe", &[], &[secret, ("QUILLO_SERVICE_BILLING", url.as_str())]);
    assert_eq!(send(cp, "GET", "/refund", &[], "").body, r#"{"refunded":500}"#);
    // Under another secret the service refuses the call; its 401 is an error here, not a
    // result, signed or not.
    let (_d, dp) = run(&d, "main.qwe", &[], &[("QUILLO_SERVICE_SECRET", "another-secret-0123456789abcdef01"), ("QUILLO_SERVICE_BILLING", url.as_str())]);
    let other = send(dp, "GET", "/refund", &[], "");
    assert_eq!(other.status, 500, "{}", other.body);
}

#[test]
fn what_auth_and_accept_calls_take() {
    let d = dir(
        "opts",
        &[
            ("billing.qwe", "server fn f() = 1\n"),
            ("lit.qwe", "service billing = \"billing.qwe\" { auth: { key: \"in-the-source\" } }\n"),
            ("lit2.qwe", "service billing = \"billing.qwe\" { auth: { oauth: { token_url: \"x\", client_id: \"a\", client_secret: `s` } } }\n"),
            ("typo.qwe", "service billing = \"billing.qwe\" { auht: { key: env(\"K\") } }\n"),
            ("for.qwe", "server fn f() for [] = 1\n"),
            ("for2.qwe", "server fn f() for [shop] = 1\n"),
            ("for3.qwe", "server fn f() for \"shop\" = 1\n"),
        ],
    );
    let check = |f: &str| String::from_utf8_lossy(&Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", f]).current_dir(&d).output().unwrap().stderr).into_owned();
    assert!(check("lit.qwe").contains("its `key` is written in the source"));
    assert!(check("lit2.qwe").contains("its `client_secret` is written in the source"));
    assert!(check("typo.qwe").contains("did you mean `auth`?"));
    assert!(check("for.qwe").contains("`for []` takes no caller"));
    assert!(check("for2.qwe").contains("a caller is named in plain text"));
    assert!(check("for3.qwe").contains("the services it takes calls from"));
    let bad = [
        ("{ auth: 3 }", "`auth` is \"quillo\" or a map"),
        ("{ auth: { nope: 1 } }", "`auth` has no `nope`"),
        ("{ auth: { key: \"\" } }", "is empty"),
        ("{ auth: { key: \"k\", bearer: \"t\" } }", "takes one way to prove who calls"),
        ("{ auth: { bearer: 3 } }", "`bearer` is a function giving a token"),
        ("{ auth: { oauth: 3 } }", "`oauth` is a map"),
        ("{ auth: { oauth: { token_url: \"x\" } } }", "`oauth` needs `client_id`"),
        ("{ auth: { oauth: { token_url: \"x\", client_id: \"a\", client_secret: \"b\", method: \"jwt\" } } }", "\"post\" or \"basic\""),
        ("{ auth: { oauth: { token_url: \"x\", nope: 1 } } }", "`oauth` has no `nope`"),
        ("{ auth: { mtls: 3 } }", "`mtls` is a map"),
        ("{ auth: { mtls: { cert: \"c\" } } }", "needs `cert` and `key`"),
        ("{ auth: { mtls: { nope: 1 } } }", "`mtls` has no `nope`"),
        ("{ auth: { key: 3 } }", "`key` is text"),
        ("{ forward: 3 }", "`forward` is a list of header names"),
        ("{ forward: [\"no spaces\"] }", "a list of header names"),
        ("{ auth: \"quillo\", forward: [\"authorization\"] }", ""),
    ];
    for (opts, why) in bad {
        let src = format!("service billing = \"billing.qwe\" {}\nprint(\"loaded\")\n", opts);
        std::fs::write(d.join("x.qwe"), src).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "x.qwe", "--no-check"]).current_dir(&d).env("K", "k").output().unwrap();
        let (so, se) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        if why.is_empty() {
            assert!(so.contains("loaded"), "{}: {}", opts, se);
        } else {
            assert!(se.contains(why), "{}: {}", opts, se);
        }
    }
    let accept = [
        ("accept_calls(3)", "takes a map"),
        ("accept_calls { nope: 1 }", "has no `nope`"),
        ("accept_calls {}", "with nothing in it"),
        ("accept_calls { keys: 3 }", "`keys` is a map"),
        ("accept_calls { keys: { a: \"\" } }", "the key of `a` is empty"),
        ("accept_calls { keys: { a: 3 } }", "text or a list of text"),
        ("accept_calls { shared: 3 }", "`shared` is true or false"),
        ("accept_calls { jwt: 3 }", "`jwt` is a map"),
        ("accept_calls { jwt: { audience: \"a\" } }", "needs `jwks`"),
        ("accept_calls { jwt: { secret: \"s\" } }", "needs `audience`"),
        ("accept_calls { jwt: { nope: 1 } }", "`jwt` has no `nope`"),
        ("accept_calls { client_cert: 3 }", "`client_cert` is a map"),
        ("accept_calls { client_cert: {} }", "needs `header`"),
        ("accept_calls { check: 3 }", "`check` is a function"),
        ("accept_calls { shared: false }", ""),
    ];
    for (line, why) in accept {
        std::fs::write(d.join("y.qwe"), format!("{}\nprint(\"loaded\")\n", line)).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["run", "y.qwe", "--no-check"]).current_dir(&d).output().unwrap();
        let (so, se) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        if why.is_empty() {
            assert!(so.contains("loaded"), "{}: {}", line, se);
        } else {
            assert!(se.contains(why), "{}: {}", line, se);
        }
    }
}

#[test]
fn signatures_and_identities_unit_by_unit() {
    use quillo::svcauth::*;
    let s = signed("v=3,kid=shop,t=12,sig=ab");
    assert_eq!((s.v, s.kid, s.t, s.sig), (3, "shop", 12, "ab"));
    let s = signed("t=12,sig=ab");
    assert_eq!((s.v, s.kid), (2, ""));
    assert_ne!(sign3("k", 1, "a", "f", "", "c", "shop", "shop", "100", "", b"[]"), sign3("k", 1, "a", "f", "", "c", "shop", "shop", "200", "", b"[]"), "the deadline is signed");
    assert_ne!(sign_answer("k", "c", 200, b"x"), sign_answer("k", "c", 500, b"x"));
    assert_eq!(cert_identity("Subject=\"CN=a,O=b\"").unwrap().0, "a");
    assert_eq!(cert_identity("CN=plain").unwrap().0, "plain");
    assert_eq!(cert_identity("By=x;URI=spiffe://c/ns/p/sa/svc,By=y;URI=spiffe://c/ns/p/sa/last").unwrap().0, "last", "the client's is the last");
    assert!(cert_identity("Hash=1").is_none());
    let rules = JwtRules { secret: Some("s".into()), audience: Some("a".into()), ..Default::default() };
    let t = |c: &str| format!("{}.{}", c, "x");
    assert!(verify_jwt(&t("a.b"), &rules).is_err());
    assert!(verify_jwt("a.b", &rules).unwrap_err().contains("three parts"));
    let es = format!("{}.{}.AA", b64(br#"{"alg":"ES512"}"#), b64(b"{}"));
    assert!(verify_jwt(&es, &rules).unwrap_err().contains("not taken"));
    assert!(verify_jwt(&format!("{}.{}.x", b64(b"nope"), b64(b"{}")), &rules).unwrap_err().contains("header is not JSON"));
    assert!(verify_jwt(&format!("{}.{}.x", b64(br#"{"alg":"HS256"}"#), b64(b"nope")), &rules).unwrap_err().contains("claims are not JSON"));
    assert!(verify_jwt(&format!("{}.{}.!!", b64(br#"{"alg":"HS256"}"#), b64(b"{}")), &rules).unwrap_err().contains("not base64url"));
    let no_secret = JwtRules { audience: Some("a".into()), jwks: Some("http://127.0.0.1:1/jwks".into()), ..Default::default() };
    assert!(verify_jwt(&jwt(r#"{"exp":1}"#, "s"), &no_secret).unwrap_err().contains("no secret"));
    for alg in ["RS256", "ES256", "EdDSA"] {
        let tok = format!("{}.{}.AA", b64(format!(r#"{{"alg":"{}","kid":"k"}}"#, alg).as_bytes()), b64(b"{}"));
        // Nothing there: said without the URL, which is the server's to know (FEEDBACK49.md,
        // Q-597), and tried once for the three.
        assert_eq!(verify_jwt(&tok, &no_secret).unwrap_err(), "the identity provider's keys could not be fetched", "{}", alg);
    }
}

#[test]
fn tokens_are_checked_against_a_jwks() {
    // Ed25519, the one a test can make without a key library: ring signs.
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let x = b64(key.public_key().as_ref());
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let fetched = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let f2 = fetched.clone();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut b = [0u8; 2048];
            let _ = c.read(&mut b);
            f2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = format!(r#"{{"keys":[{{"kty":"OKP","crv":"Ed25519","kid":"ed-1","use":"sig","x":"{}"}}]}}"#, x);
            let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let sign = |kid: &str, claims: &str| {
        let m = format!("{}.{}", b64(format!(r#"{{"alg":"EdDSA","kid":"{}"}}"#, kid).as_bytes()), b64(claims.as_bytes()));
        format!("{}.{}", m, b64(key.sign(m.as_bytes()).as_ref()))
    };
    let rules = quillo::svcauth::JwtRules { jwks: Some(format!("http://127.0.0.1:{}/jwks", port)), audience: Some("billing".into()), issuer: Some("idp".into()), secret: None };
    let good = sign("ed-1", &format!(r#"{{"sub":"a","iss":"idp","aud":"billing","exp":{}}}"#, now() + 60));
    assert_eq!(quillo::svcauth::verify_jwt(&good, &rules).unwrap()["sub"], "a");
    assert_eq!(quillo::svcauth::verify_jwt(&good, &rules).unwrap()["sub"], "a");
    assert_eq!(fetched.load(std::sync::atomic::Ordering::SeqCst), 1, "kept");
    let unknown = sign("ed-9", &format!(r#"{{"sub":"a","iss":"idp","aud":"billing","exp":{}}}"#, now() + 60));
    assert!(quillo::svcauth::verify_jwt(&unknown, &rules).unwrap_err().contains("no key `ed-9`"));
    assert!(quillo::svcauth::verify_jwt(&unknown, &rules).unwrap_err().contains("no key `ed-9`"));
    assert_eq!(fetched.load(std::sync::atomic::Ordering::SeqCst), 1, "fetched again at most once a minute");
    let caller = quillo::svcauth::jwt_caller(&serde_json::json!({ "azp": "x", "scp": ["a", "b"] })).unwrap();
    assert_eq!(caller.display(), "{ name: \"x\", by: \"jwt\", scopes: [\"a\", \"b\"], claims: { azp: \"x\", scp: [\"a\", \"b\"] } }");
}

/// An identity provider for the tests: answers its JWKS (`keys`) as `how` says, counting fetches.
fn idp(how: &'static str, keys: String) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>, std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/jwks", l.local_addr().unwrap().port());
    let (fetched, down) = (std::sync::Arc::new(AtomicUsize::new(0)), std::sync::Arc::new(AtomicBool::new(false)));
    let (f2, d2) = (fetched.clone(), down.clone());
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let (f3, d3, keys) = (f2.clone(), d2.clone(), keys.clone());
            std::thread::spawn(move || {
                let mut b = [0u8; 2048];
                let _ = c.read(&mut b);
                f3.fetch_add(1, SeqCst);
                match how {
                    "silent" => std::thread::sleep(Duration::from_secs(30)),
                    "slow" => std::thread::sleep(Duration::from_millis(800)),
                    _ => {}
                }
                if how == "500" || d3.load(SeqCst) {
                    let _ = write!(c, "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\noops");
                } else {
                    let body = format!(r#"{{"keys":[{}]}}"#, keys);
                    let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                }
            });
        }
    });
    (url, fetched, down)
}

// FEEDBACK49.md, Q-597: no negative cache, no stale keys, a wait of ten seconds on a provider
// that does not answer, and the JWKS URL in the caller's error.
#[test]
fn a_jwks_that_fails_is_fetched_once_a_minute_and_the_old_keys_are_kept() {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::sync::atomic::Ordering::SeqCst;
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let jwk = format!(r#"{{"kty":"OKP","crv":"Ed25519","kid":"ed-1","use":"sig","x":"{}"}}"#, b64(key.public_key().as_ref()));
    let token = {
        let m = format!("{}.{}", b64(br#"{"alg":"EdDSA","kid":"ed-1"}"#), b64(format!(r#"{{"sub":"a","exp":{}}}"#, now() + 60).as_bytes()));
        format!("{}.{}", m, b64(key.sign(m.as_bytes()).as_ref()))
    };
    let rules = |url: &str| quillo::svcauth::JwtRules { jwks: Some(url.to_string()), audience: None, issuer: None, secret: None };

    // A provider that fails: one fetch for five calls, and no URL in what the caller is told.
    let (url, fetched, _) = idp("500", jwk.clone());
    for _ in 0..5 {
        let e = quillo::svcauth::verify_jwt(&token, &rules(&url)).unwrap_err();
        assert_eq!(e, "the identity provider's keys could not be fetched");
    }
    assert_eq!(fetched.load(SeqCst), 1);
    // A minute on, it is tried again.
    quillo::svcauth::age_jwks(&url, Duration::from_secs(61));
    assert!(quillo::svcauth::verify_jwt(&token, &rules(&url)).is_err());
    assert_eq!(fetched.load(SeqCst), 2);

    // One that never answers: three seconds for the first call, none for the next.
    let (url, fetched, _) = idp("silent", jwk.clone());
    let t = Instant::now();
    assert!(quillo::svcauth::verify_jwt(&token, &rules(&url)).is_err());
    assert!(t.elapsed() < Duration::from_secs(6), "{:?}", t.elapsed());
    let t = Instant::now();
    for _ in 0..4 {
        assert!(quillo::svcauth::verify_jwt(&token, &rules(&url)).is_err());
    }
    assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
    assert_eq!(fetched.load(SeqCst), 1);

    // Five callers at once, on a slow one: one fetch, and all take its keys.
    let (url, fetched, down) = idp("slow", jwk.clone());
    let all: Vec<_> = (0..5).map(|_| {
        let (token, url) = (token.clone(), url.clone());
        std::thread::spawn(move || quillo::svcauth::verify_jwt(&token, &rules(&url)).map(|c| c["sub"].clone()))
    }).collect();
    for h in all {
        assert_eq!(h.join().unwrap().unwrap(), "a");
    }
    assert_eq!(fetched.load(SeqCst), 1);
    // Old keys, and the provider down: they are used, and it is asked once a minute.
    down.store(true, SeqCst);
    quillo::svcauth::age_jwks(&url, Duration::from_secs(601));
    assert_eq!(quillo::svcauth::verify_jwt(&token, &rules(&url)).unwrap()["sub"], "a");
    assert_eq!(quillo::svcauth::verify_jwt(&token, &rules(&url)).unwrap()["sub"], "a");
    assert_eq!(fetched.load(SeqCst), 2);
    // Back up, a minute later: fetched again.
    down.store(false, SeqCst);
    quillo::svcauth::age_jwks(&url, Duration::from_secs(61));
    assert_eq!(quillo::svcauth::verify_jwt(&token, &rules(&url)).unwrap()["sub"], "a");
    assert_eq!(fetched.load(SeqCst), 3);
    // A key it does not have, after a fetch that worked, is told without the URL.
    let other = {
        let m = format!("{}.{}", b64(br#"{"alg":"EdDSA","kid":"ed-9"}"#), b64(format!(r#"{{"sub":"a","exp":{}}}"#, now() + 60).as_bytes()));
        format!("{}.{}", m, b64(key.sign(m.as_bytes()).as_ref()))
    };
    let e = quillo::svcauth::verify_jwt(&other, &rules(&url)).unwrap_err();
    assert_eq!(e, "no key `ed-9` in the identity provider's keys");
}

// The signatures the provider's keys check, wrong and right (Q-597's "never run in a test").
#[test]
fn rs256_and_es256_tokens_with_a_wrong_signature_are_refused() {
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
    let rng = ring::rand::SystemRandom::new();
    let ec = |rng: &ring::rand::SystemRandom| {
        let p = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, rng).unwrap();
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, p.as_ref(), rng).unwrap()
    };
    let (mine, theirs) = (ec(&rng), ec(&rng));
    let point = mine.public_key().as_ref();
    let ec_jwk = format!(r#"{{"kty":"EC","crv":"P-256","kid":"ec-1","x":"{}","y":"{}"}}"#, b64(&point[1..33]), b64(&point[33..65]));
    // An RSA key of 2048 bits: any odd modulus does for a signature that cannot be right.
    let mut n = vec![0xc3u8; 256];
    n[255] |= 1;
    let rsa_jwk = format!(r#"{{"kty":"RSA","kid":"rsa-1","n":"{}","e":"AQAB"}}"#, b64(&n));
    let (url, _, _) = idp("ok", format!("{},{}", ec_jwk, rsa_jwk));
    let rules = quillo::svcauth::JwtRules { jwks: Some(url), audience: None, issuer: None, secret: None };
    let claims = b64(format!(r#"{{"sub":"a","exp":{}}}"#, now() + 60).as_bytes());
    let es = |by: &EcdsaKeyPair| {
        let m = format!("{}.{}", b64(br#"{"alg":"ES256","kid":"ec-1"}"#), claims);
        format!("{}.{}", m, b64(by.sign(&rng, m.as_bytes()).unwrap().as_ref()))
    };
    assert_eq!(quillo::svcauth::verify_jwt(&es(&mine), &rules).unwrap()["sub"], "a");
    assert_eq!(quillo::svcauth::verify_jwt(&es(&theirs), &rules).unwrap_err(), "the token's signature is wrong");
    let rs = format!("{}.{}.{}", b64(br#"{"alg":"RS256","kid":"rsa-1"}"#), claims, b64(&[7u8; 256]));
    assert_eq!(quillo::svcauth::verify_jwt(&rs, &rules).unwrap_err(), "the token's signature is wrong");
    // A key of the wrong kind for the token is no key.
    let wrong = format!("{}.{}.{}", b64(br#"{"alg":"RS256","kid":"ec-1"}"#), claims, b64(&[7u8; 256]));
    assert_eq!(quillo::svcauth::verify_jwt(&wrong, &rules).unwrap_err(), "no key `ec-1` in the identity provider's keys");
}

#[allow(dead_code)]
fn unused(_: TcpStream) {}
