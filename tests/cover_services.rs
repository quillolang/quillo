//! Services and how they prove who calls, past the usual path (src/service.rs, src/svcauth.rs):
//! a `bearer` that gives no token, a literal token, client certificates, OAuth tokens kept,
//! refetched and refused, RS256 and ES256 tokens checked against a JWKS, what `accept_calls`
//! refuses and what its `check` gives, a copy that joins with a bad address, a service known
//! only by its answer, a file read for its headers alone, calls made later that wait in memory,
//! and `--split` with `--no-build` and `--policy`. Every server is one this test starts.

mod support;

use quillo::server::{HttpRequest, HttpResponse};
use quillo::service;
use quillo::svcauth::{self, JwtRules, OAuth};
use quillo::Interp;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

const SECRET: &str = "a-test-secret-for-the-cover-services-tests";

fn signing() {
    static SET: Once = Once::new();
    SET.call_once(|| std::env::set_var("QUILLO_SERVICE_SECRET", SECRET));
}

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("cover-services-{}", name))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn text(r: &HttpResponse) -> String {
    String::from_utf8_lossy(&r.body).into_owned()
}

fn ev(it: &mut Interp, src: &str) -> String {
    match it.load_source(src, "after.qwe") {
        Ok(v) => v.repr(),
        Err(e) => format!("ERROR: {}", e),
    }
}

fn request(method: &str, url: &str, headers: Vec<(String, String)>, body: &str, remote: &str) -> HttpRequest {
    let mut h = vec![("Host".to_string(), "localhost".to_string()), ("Content-Type".to_string(), "application/json".to_string())];
    h.extend(headers);
    HttpRequest { method: method.into(), url: url.into(), headers: h, body: body.as_bytes().to_vec(), remote: remote.into() }
}

/// The headers of a call of `f` on service `to`, signed with the shared secret.
fn signed(to: &str, f: &str, call: &str, body: &str) -> Vec<(String, String)> {
    let t = now_secs();
    let sig = service::signature(SECRET, t, to, f, "", call, body.as_bytes());
    vec![("X-Quillo-Service".to_string(), format!("t={},sig={}", t, sig)), ("X-Quillo-To".to_string(), to.to_string()), ("X-Quillo-Call".to_string(), call.to_string())]
}

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

/// A request's head and body.
fn read_request(s: &mut TcpStream) -> (String, String) {
    let head = read_head(s);
    let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
    let mut body = vec![0u8; len];
    let _ = s.read_exact(&mut body);
    (head, String::from_utf8_lossy(&body).into_owned())
}

/// A server played by the test: `f` is told how many requests came before, and the request,
/// and gives the status and body of its answer. Its URL.
fn fake(f: impl Fn(usize, &str, &str) -> (u16, String) + Send + Sync + 'static) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (f, n) = (Arc::new(f), Arc::new(AtomicUsize::new(0)));
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let (f, n) = (f.clone(), n.clone());
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let (head, body) = read_request(&mut s);
                let (status, out) = f(n.fetch_add(1, Ordering::SeqCst), &head, &body);
                let _ = write!(s, "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, out.len(), out);
            });
        }
    });
    format!("http://127.0.0.1:{}", port)
}

fn nobody() -> String {
    format!("http://127.0.0.1:{}", support::free_port())
}

fn header<'a>(head: &'a str, name: &str) -> &'a str {
    head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())).unwrap_or("none")
}

const WORK: &str = "server fn add(a: int, b: int = 1) -> int = a + b\nserver fn send(x) = x\n";

/// A program that declares service `name` (of `work.qwe`) with these options after `pre`, and
/// calls it at `url`.
fn caller(name: &str, url: &str, pre: &str, options: &str) -> (PathBuf, Interp) {
    let d = dir(name);
    std::fs::write(d.join("work.qwe"), WORK).unwrap();
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert(name.to_string(), url.to_string());
    let src = format!("{}\nservice {} = \"work.qwe\" {}\n", pre, name, options);
    it.load_source(&src, "main.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    (d, it)
}

// ---- the caller's proof ----------------------------------------------------------------------

#[test]
fn a_bearer_that_gives_no_token_says_why_and_a_literal_one_is_sent() {
    // Answers with the token it was given.
    let url = fake(|_, head, _| (200, format!("{{\"ok\":{:?}}}", header(head, "authorization"))));
    let pre = "fn gives_err() = err(\"no token today\")\nfn gives_int() = 5\nfn fails() = nosuchthing()\nfn replies() {\n    reply 401 \"no\"\n}\n";
    for (f, why) in [
        ("gives_err", "its `bearer` gave no token: no token today"),
        ("gives_int", "its `bearer` gives a token as text, not 5"),
        ("fails", "its `bearer` failed: unknown name `nosuchthing`"),
        ("replies", "its `bearer` gave no token"),
    ] {
        let name = format!("bearer_{}", f);
        let (_d, mut it) = caller(&name, &url, pre, &format!("{{ retries: 0, auth: {{ bearer: {} }} }}", f));
        let got = ev(&mut it, &format!("{}.add(1)", name));
        assert!(got.contains(&format!("service {}: {}", name, why)), "{}: {}", f, got);
    }
    let (_d, mut it) = caller("bearer_text", &url, "", "{ retries: 0, auth: { bearer: \"a-literal-token\" } }");
    assert_eq!(ev(&mut it, "bearer_text.add(1)"), "\"Bearer a-literal-token\"");
}

/// A CA and a client certificate it signed, in `d`.
fn client_pki(d: &Path) {
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca.distinguished_name.push(rcgen::DnType::CommonName, "cover services ca");
    std::fs::write(d.join("ca.pem"), ca.self_signed(&ca_key).unwrap().pem()).unwrap();
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let key = rcgen::KeyPair::generate().unwrap();
    let mut p = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    p.distinguished_name.push(rcgen::DnType::CommonName, "shop");
    std::fs::write(d.join("client.pem"), p.signed_by(&key, &issuer).unwrap().pem()).unwrap();
    std::fs::write(d.join("client.key"), key.serialize_pem()).unwrap();
}

#[test]
fn a_client_certificate_is_set_up_for_a_service_and_one_that_cannot_be_read_is_said() {
    signing();
    let url = fake(|_, _, _| (200, "{\"ok\":\"answered\"}".into()));
    let (d, mut it) = caller("mtls_good", &url, "", "{ retries: 0, auth: { mtls: { cert: \"client.pem\", key: \"client.key\", ca: \"ca.pem\" } } }");
    client_pki(&d);
    assert_eq!(ev(&mut it, "mtls_good.add(1)"), "\"answered\"");
    let (_d, mut it) = caller("mtls_bad", &url, "", "{ retries: 0, auth: { mtls: { cert: \"missing.pem\", key: \"missing.key\" } } }");
    let got = ev(&mut it, "mtls_bad.add(1)");
    assert!(got.contains("service mtls_bad: its `mtls`: cert ") && got.contains("missing.pem"), "{}", got);
}

/// A token endpoint: `f` gives the status and body of each answer; the forms it was sent.
fn token_endpoint(f: impl Fn(usize) -> (u16, String) + Send + Sync + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
    let forms: Arc<Mutex<Vec<String>>> = Default::default();
    let seen = forms.clone();
    let url = fake(move |n, _, body| {
        seen.lock().unwrap().push(body.to_string());
        f(n)
    });
    (format!("{}/token", url), forms)
}

fn oauth(url: &str, client_id: &str) -> OAuth {
    OAuth { token_url: url.to_string(), client_id: client_id.to_string(), client_secret: "s3cret!".into(), scope: "read write".into(), audience: String::new(), basic: false }
}

#[test]
fn an_oauth_token_is_fetched_again_when_its_time_is_up_and_its_form_is_encoded() {
    let (url, forms) = token_endpoint(|n| (200, format!("{{\"access_token\":\"t{}\",\"expires_in\":1}}", n)));
    let o = oauth(&url, "shop@acme corp");
    assert_eq!(svcauth::oauth_token(&o, false).unwrap(), "t0");
    assert_eq!(svcauth::oauth_token(&o, false).unwrap(), "t0", "kept");
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(svcauth::oauth_token(&o, false).unwrap(), "t1", "its time was up");
    let forms = forms.lock().unwrap();
    assert_eq!(forms.len(), 2);
    assert_eq!(forms[0], "grant_type=client_credentials&client_id=shop%40acme%20corp&client_secret=s3cret%21&scope=read%20write");
}

#[test]
fn a_token_endpoint_that_is_not_there_is_said() {
    let o = oauth(&format!("{}/token", nobody()), "shop");
    let e = svcauth::oauth_token(&o, true).unwrap_err();
    assert!(e.starts_with(&format!("the token endpoint {}: ", o.token_url)), "{}", e);
}

#[test]
fn a_refused_token_whose_refetch_fails_is_not_sent_again() {
    let (tokens, _) = token_endpoint(|n| if n == 0 { (200, "{\"access_token\":\"first\",\"expires_in\":3600}".into()) } else { (500, "{\"error\":\"down for now\"}".into()) });
    let service = fake(|_, _, _| (401, "{\"error\":\"that token is no good\"}".into()));
    let opts = format!("{{ retries: 0, auth: {{ oauth: {{ token_url: {:?}, client_id: \"shop\", client_secret: \"s\" }} }} }}", tokens);
    let (_d, mut it) = caller("oauth_refused", &service, "", &opts);
    let got = ev(&mut it, "oauth_refused.add(1)");
    assert!(got.contains(&format!("the token endpoint {} answered 500: ", tokens)) && got.contains("down for now"), "{}", got);
}

// ---- tokens checked against a JWKS -------------------------------------------------------------

fn b64(b: &[u8]) -> String {
    quillo::crypto::b64url(b)
}

/// A JWKS of these keys, served; its URL and how often it was fetched.
fn jwks(keys: serde_json::Value) -> String {
    let body = serde_json::json!({ "keys": keys }).to_string();
    format!("{}/jwks", fake(move |_, _, _| (200, body.clone())))
}

fn rules(url: &str) -> JwtRules {
    JwtRules { jwks: Some(url.to_string()), audience: Some("billing".into()), issuer: Some("idp".into()), secret: None }
}

fn claims(aud: serde_json::Value) -> String {
    serde_json::json!({ "sub": "reports", "iss": "idp", "aud": aud, "exp": now_secs() + 120 }).to_string()
}

#[test]
fn an_es256_token_is_checked_against_its_key_and_its_audience() {
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let point = key.public_key().as_ref();
    let url = jwks(serde_json::json!([{ "kty": "EC", "crv": "P-256", "kid": "ec-1", "use": "sig", "x": b64(&point[1..33]), "y": b64(&point[33..]) }]));
    let sign = |claims: &str| {
        let m = format!("{}.{}", b64(br#"{"alg":"ES256","kid":"ec-1"}"#), b64(claims.as_bytes()));
        let sig = key.sign(&rng, m.as_bytes()).unwrap();
        format!("{}.{}", m, b64(sig.as_ref()))
    };
    let r = rules(&url);
    let good = svcauth::verify_jwt(&sign(&claims(serde_json::json!(["orders", "billing"]))), &r).unwrap();
    assert_eq!(good["sub"], "reports", "one of its audiences is this one");
    let e = svcauth::verify_jwt(&sign(&claims(serde_json::json!(5))), &r).unwrap_err();
    assert!(e.contains("billing"), "an audience that is no text or list is not this one: {}", e);
    let other = sign(&claims(serde_json::json!("billing")));
    let (head, _) = other.rsplit_once('.').unwrap();
    let forged = format!("{}.{}", head, sign(&claims(serde_json::json!("other"))).rsplit_once('.').unwrap().1);
    assert_eq!(svcauth::verify_jwt(&forged, &r).unwrap_err(), "the token's signature is wrong");
    let open = JwtRules { issuer: None, audience: None, ..rules(&url) };
    let anyone = serde_json::json!({ "sub": "x", "iss": "elsewhere", "exp": now_secs() + 60 }).to_string();
    assert_eq!(svcauth::verify_jwt(&sign(&anyone), &open).unwrap()["iss"], "elsewhere", "no issuer or audience asked: any");
}

#[test]
fn an_rs256_token_is_checked_against_its_key() {
    // An RSA key made here, by openssl: ring cannot make one.
    let d = dir("rsa");
    let pem = d.join("rsa.pem");
    let made = Command::new("openssl").args(["genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out"]).arg(&pem).stdout(Stdio::null()).stderr(Stdio::null()).status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("openssl is not here: RS256 not tested");
        return;
    }
    use rustls::pki_types::pem::PemObject;
    let der = rustls::pki_types::PrivatePkcs8KeyDer::from_pem_file(&pem).unwrap();
    let key = ring::signature::RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()).unwrap();
    let public = ring::rsa::PublicKeyComponents::<Vec<u8>>::from(key.public());
    let url = jwks(serde_json::json!([{ "kty": "RSA", "kid": "rs-1", "n": b64(&public.n), "e": b64(&public.e) }]));
    let rng = ring::rand::SystemRandom::new();
    let sign = |claims: &str| {
        let m = format!("{}.{}", b64(br#"{"alg":"RS256","kid":"rs-1"}"#), b64(claims.as_bytes()));
        let mut sig = vec![0u8; key.public().modulus_len()];
        key.sign(&ring::signature::RSA_PKCS1_SHA256, &rng, m.as_bytes(), &mut sig).unwrap();
        format!("{}.{}", m, b64(&sig))
    };
    let r = rules(&url);
    assert_eq!(svcauth::verify_jwt(&sign(&claims(serde_json::json!("billing"))), &r).unwrap()["iss"], "idp");
    let good = sign(&claims(serde_json::json!("billing")));
    // A character in the signature's middle: the last one holds only 2 bits of the 256 bytes,
    // and may differ in the 4 that mean nothing (which made this test fail one run in a few).
    let at = good.rfind('.').unwrap() + 40;
    let swap = if good.as_bytes()[at] == b'A' { "B" } else { "A" };
    let tampered = format!("{}{}{}", &good[..at], swap, &good[at + 1..]);
    assert!(svcauth::verify_jwt(&tampered, &r).is_err());
    // Rules that ask no issuer and no audience take any.
    let open = JwtRules { issuer: None, audience: None, ..rules(&url) };
    let anyone = serde_json::json!({ "sub": "x", "iss": "elsewhere", "aud": "orders", "exp": now_secs() + 60 }).to_string();
    assert_eq!(svcauth::verify_jwt(&sign(&anyone), &open).unwrap()["iss"], "elsewhere");
}

#[test]
fn scopes_given_as_a_list_are_the_callers() {
    let c = svcauth::jwt_caller(&serde_json::json!({ "client_id": "reports", "scope": ["billing:read", 7, "billing:write"] }));
    let shown = c.display();
    assert!(shown.contains("name: \"reports\"") && shown.contains("scopes: [\"billing:read\", \"billing:write\"]"), "{}", shown);
}

// ---- the service's side --------------------------------------------------------------------

#[test]
fn accept_calls_refuses_a_key_of_null() {
    let d = dir("keys-null");
    let mut it = Interp::new(&d);
    let e = it.load_source("accept_calls { keys: { shop: null } }\n", "main.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("`accept_calls`: the key of `shop` is empty (is its environment variable set?)"), "{}", e);
}

/// A service program, `name.qwe`, of `src`, loaded as the service.
fn service_program(name: &str, src: &str) -> Interp {
    let d = dir(name);
    let f = d.join(format!("{}.qwe", name));
    std::fs::write(&f, src).unwrap();
    let mut it = Interp::new(&d);
    it.load_file(&f).unwrap_or_else(|e| panic!("{}", e));
    it
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn call_id() -> String {
    format!("cov{}x{}", std::process::id(), CALLS.fetch_add(1, Ordering::SeqCst))
}

#[test]
fn a_call_with_a_key_no_one_here_has_or_a_token_nothing_checks_is_refused() {
    signing();
    let mut it = service_program("svc_proofs", "server fn whoami() = request.caller\n");
    let body = "[]";
    let kid = vec![("X-Quillo-Service".to_string(), format!("v=3,kid=ghost,t={},sig=abc", now_secs())), ("X-Quillo-To".to_string(), "svc_proofs".to_string()), ("X-Quillo-Call".to_string(), call_id())];
    let r = it.handle(&request("POST", "/_quillo/service/whoami", kid, body, "127.0.0.1"));
    assert_eq!(r.status, 401);
    assert!(text(&r).contains("no key here for caller `ghost` (accept_calls { keys })"), "{}", text(&r));
    let bearer = vec![("Authorization".to_string(), "Bearer some.token.here".to_string()), ("X-Quillo-To".to_string(), "svc_proofs".to_string()), ("X-Quillo-Call".to_string(), call_id())];
    let r = it.handle(&request("POST", "/_quillo/service/whoami", bearer, body, "127.0.0.1"));
    assert_eq!(r.status, 401);
    assert!(text(&r).contains("a bearer token, and nothing here checks one (accept_calls { jwt })"), "{}", text(&r));
    // Signed with the shared secret, it is taken.
    let id = call_id();
    let r = it.handle(&request("POST", "/_quillo/service/whoami", signed("svc_proofs", "whoami", &id, body), body, "127.0.0.1"));
    assert_eq!(r.status, 200, "{}", text(&r));
}

#[test]
fn what_accept_calls_check_gives_decides_the_call() {
    signing();
    let call = |it: &mut Interp, to: &str| {
        let id = call_id();
        let r = it.handle(&request("POST", "/_quillo/service/whoami", signed(to, "whoami", &id, "[]"), "[]", "127.0.0.1"));
        (r.status, text(&r))
    };
    let mut no = service_program("chk_err", "accept_calls { check: (caller) => err(`not ${caller.name} today`) }\nserver fn whoami() = request.caller.name\n");
    let (status, out) = call(&mut no, "chk_err");
    assert_eq!(status, 403, "{}", out);
    assert!(out.contains("not main today") || out.contains("today"), "{}", out);
    let mut yes = service_program("chk_true", "accept_calls { check: (caller) => true }\nserver fn whoami() = request.caller.by\n");
    assert_eq!(call(&mut yes, "chk_true"), (200, "{\"ok\":\"quillo\"}".to_string()), "true leaves the caller as it was");
    let mut broken = service_program("chk_broken", "accept_calls { check: (caller) => nosuchthing(caller) }\nserver fn whoami() = 1\n");
    let (status, out) = call(&mut broken, "chk_broken");
    assert_eq!((status, out.contains("internal error in accept_calls's check")), (500, true), "{}", out);
}

#[test]
fn a_copy_of_an_allowed_service_that_gives_no_web_address_is_refused() {
    signing();
    service::set_join_policy(Some(vec!["joiner".to_string()]));
    let d = dir("join-url");
    std::fs::write(d.join("joiner.qwe"), WORK).unwrap();
    let mut it = Interp::new(&d);
    it.load_source("service joiner = \"joiner.qwe\"\n", "main.qwe").unwrap();
    let join = |it: &mut Interp, body: &str| {
        let nonce = call_id();
        let t = now_secs();
        let sig = service::signature(SECRET, t, "_join", "joiner", "", &nonce, body.as_bytes());
        let h = vec![("X-Quillo-Service".to_string(), format!("t={},sig={}", t, sig)), ("X-Quillo-Call".to_string(), nonce)];
        let r = it.handle(&request("POST", "/_quillo/join", h, body, "127.0.0.1:40000"));
        (r.status, text(&r))
    };
    let (status, out) = join(&mut it, "{\"service\":\"joiner\",\"port\":9000,\"url\":\"ftp://127.0.0.1:9000\"}");
    assert_eq!((status, out.as_str()), (400, "{\"error\":\"the copy's url is an http(s) address\"}"));
    let (status, out) = join(&mut it, "{\"service\":\"joiner\",\"port\":9000,\"url\":\"http://127.0.0.1:9000\"}");
    assert_eq!(status, 200, "an allowed service joins: {}", out);
}

// ---- a service known by what it says ------------------------------------------------------------

#[test]
fn a_service_whose_answer_is_no_list_of_functions_cannot_be_declared() {
    signing();
    let url = fake(|_, _, _| (200, "{\"ok\":5}".into()));
    let d = dir("contracts");
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("faraway".to_string(), url);
    let e = it.load_source("service faraway = \"faraway.qwe\"\n", "main.qwe").err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("faraway.qwe is not here, and service faraway did not give its functions: its answer is not a list of functions: 5"), "{}", e);
}

#[test]
fn a_service_file_that_does_not_parse_is_read_for_its_headers() {
    signing();
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let s2 = seen.clone();
    let url = fake(move |_, head, body| {
        s2.lock().unwrap().push(format!("{} {}", head.lines().next().unwrap_or(""), body));
        (200, "{\"ok\":\"from far away\"}".into())
    });
    let d = dir("headers-only");
    // A brace in a default, a route and a service line that do not parse, and a header never ended.
    std::fs::write(d.join("halfway.qwe"), "server fn settings(o = { a: 1 }, n: int = 2) = o\nroute GET / = (\nservice = 3\nserver fn never_ends(a: int,\n    b: int\n").unwrap();
    let mut it = Interp::new(&d);
    it.shared.service_urls.lock().unwrap().insert("halfway".to_string(), url);
    it.load_source("service halfway = \"halfway.qwe\" { retries: 0 }\n", "main.qwe").unwrap_or_else(|e| panic!("{}", e));
    assert_eq!(ev(&mut it, "halfway.settings()"), "\"from far away\"");
    assert!(ev(&mut it, "halfway.never_ends(1, 2)").contains("never_ends"), "a header never ended is no function");
    assert!(seen.lock().unwrap().iter().any(|l| l.starts_with("POST /_quillo/service/settings")), "{:?}", seen.lock().unwrap());
}

// ---- calls made later --------------------------------------------------------------------------

#[test]
fn a_call_made_later_without_a_database_waits_in_memory_and_says_so() {
    let (_d, mut it) = caller("later_mem", &nobody(), "", "{ retries: 0 }");
    let id = ev(&mut it, "later_mem.later.send(1)");
    assert_eq!(ev(&mut it, &format!("later.status({})", id)), "{ state: \"waiting\", tries: 0, error: null }");
    assert!(ev(&mut it, "later.status(5)").contains("`later.status` expects the id a `.later` call gave"));
    assert!(ev(&mut it, "later.status()").contains("`later.status` expects the id"));
}

// ---- --split ------------------------------------------------------------------------------------

#[test]
fn split_passes_no_build_and_the_policy_on_to_its_services() {
    let d = dir("split");
    std::fs::write(d.join("work.qwe"), "server fn add(a: int, b: int) -> int = a + b\n").unwrap();
    std::fs::write(d.join("main.qwe"), "service work = \"work.qwe\"\nroute GET /sum = str(work.add(2, 3))\n").unwrap();
    std::fs::write(d.join("ops.toml"), "[plugins]\nbuild = false\n").unwrap();
    let port = support::free_port();
    let log = std::fs::File::create(d.join("split.log")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["run", "main.qwe", "--port", &port.to_string(), "--split", "--no-build", "--policy", "ops.toml"])
        .current_dir(&d)
        .env("QUILLO_SECRET", "a secret for the cover tests, long enough")
        .env("QUILLO_SERVICE_SECRET", SECRET)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut g = support::Guard::new(child);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if support::listening(port) && support::send(port, "GET", "/sum", &[], "").body == "5" {
            break;
        }
        assert!(Instant::now() < deadline, "{}", std::fs::read_to_string(d.join("split.log")).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(100));
    }
    // The service it started, its child, was given both.
    let children = |pid: u32| -> Vec<(u32, String)> {
        let ps = Command::new("ps").args(["-axo", "pid=,ppid=,command="]).output().unwrap();
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                let (c, p) = (w.next()?.parse::<u32>().ok()?, w.next()?.parse::<u32>().ok()?);
                (p == pid).then(|| (c, w.collect::<Vec<_>>().join(" ")))
            })
            .collect()
    };
    let app = g.0.id();
    let kids = children(app);
    let service = kids.iter().find(|(_, c)| c.contains("--service-name work")).cloned();
    // Stopped as a person stops it, the app stops its service too; whatever is left is stopped here.
    let _ = Command::new("kill").args(["-INT", &app.to_string()]).status();
    let until = Instant::now() + Duration::from_secs(10);
    while g.0.try_wait().ok().flatten().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    for (pid, _) in &kids {
        if children(1).iter().any(|(p, c)| p == pid && c.contains("--as-service")) {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
    let (_, line) = service.unwrap_or_else(|| panic!("no service started: {:?}", kids));
    assert!(line.contains("--as-service") && line.contains("--no-build") && line.contains("--policy ops.toml"), "{}", line);
}
