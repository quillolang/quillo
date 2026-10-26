//! `crypto`: hashes, signatures, tokens and passwords.

fn eval(src: &str) -> serde_json::Value {
    let v = quillo::eval_str(&format!("json.stringify({})", src)).unwrap_or_else(|e| panic!("{}", e));
    serde_json::from_str(&v.display()).unwrap()
}

#[test]
fn hashes_and_signatures() {
    assert_eq!(eval(r#"crypto.sha256("abc")"#), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(
        eval(r#"crypto.hmac("key", "The quick brown fox jumps over the lazy dog")"#),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
    );
    assert_eq!(eval(r#"[crypto.equal("a1", "a1"), crypto.equal("a1", "a2"), crypto.equal("a", "ab")]"#), serde_json::json!([true, false, false]));
}

#[test]
fn tokens_are_random_and_url_safe() {
    let v = eval(r#"[crypto.token(), crypto.token(), crypto.token(16)]"#);
    let (a, b, c) = (v[0].as_str().unwrap(), v[1].as_str().unwrap(), v[2].as_str().unwrap());
    assert_eq!((a.len(), c.len()), (43, 22));
    assert_ne!(a, b);
    assert!(a.bytes().all(|x| x.is_ascii_alphanumeric() || x == b'-' || x == b'_'), "{}", a);
}

#[test]
fn passwords_are_salted_and_checked() {
    let v = eval(
        r#"{
    let h = crypto.hash_password("correct horse")
    let again = crypto.hash_password("correct horse")
    {
        h: h,
        salted: h != again,
        right: crypto.verify_password("correct horse", h),
        wrong: crypto.verify_password("Correct horse", h),
        junk: crypto.verify_password("correct horse", "not a hash"),
        missing: crypto.verify_password("correct horse", null),
    }
}"#,
    );
    assert!(v["h"].as_str().unwrap().starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{}", v["h"]);
    assert_eq!(
        (&v["salted"], &v["right"], &v["wrong"], &v["junk"], &v["missing"]),
        (&true.into(), &true.into(), &false.into(), &false.into(), &false.into())
    );
}

#[test]
fn one_time_codes_from_an_authenticator_app() {
    // RFC 6238's SHA-1 key, "12345678901234567890", in base32; its codes
    // (appendix B) as the 6 digits apps show.
    let key = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    let v = eval(&format!(
        r#"{{
    let k = "{key}"
    [
        crypto.totp_verify(k, "287082", {{ at: 59 }}),
        crypto.totp_verify(k, "081804", {{ at: 1111111109 }}),
        crypto.totp_verify(k, "005924", {{ at: 1234567890 }}),
        crypto.totp_verify(k, "279037", {{ at: 2000000000 }}),
        crypto.totp_verify(k, "353130", {{ at: 20000000000 }}),
        crypto.totp_verify(k, "287 082", {{ at: 59 + 30 }}),
        crypto.totp_verify(k, "287082", {{ at: 59 + 30, window: 0 }}),
        crypto.totp_verify(k, "287082", {{ at: 59, after: 1 }}),
        crypto.totp_verify(k, "287082", {{ at: 59 + 90 }}),
        crypto.totp_verify(k, "287083", {{ at: 59 }}),
        crypto.totp_verify(k, "", {{ at: 59 }}),
    ]
}}"#
    ));
    assert_eq!(v, serde_json::json!([1, 37037036, 41152263, 66666666, 666666666, 1, null, null, null, null, null]));
    let s = eval("crypto.totp_secret()");
    let s = s.as_str().unwrap();
    assert_eq!(s.len(), 32);
    assert!(s.bytes().all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b)), "{}", s);
    assert_eq!(
        eval(&format!(r#"crypto.totp_uri("{}", "ada@acme.test", "Acme & Co")"#, key)),
        format!("otpauth://totp/Acme%20%26%20Co:ada%40acme.test?secret={}&issuer=Acme%20%26%20Co&algorithm=SHA1&digits=6&period=30", key)
    );
}

#[test]
fn recovery_codes_are_kept_as_hashes_and_work_once_each() {
    let v = eval(
        r#"{
    let codes = crypto.recovery_codes(3)
    let hashes = codes | map((c) => c.hash)
    {
        codes: codes | map((c) => c.code),
        hashes: hashes,
        second: crypto.verify_recovery_code(upper(replace(codes[1].code, "-", " ")), hashes),
        wrong: crypto.verify_recovery_code("0000-0000-0000", hashes),
        none: crypto.verify_recovery_code(codes[0].code, []),
    }
}"#,
    );
    let codes = v["codes"].as_array().unwrap();
    assert_eq!(codes.len(), 3);
    for c in codes {
        let c = c.as_str().unwrap();
        assert_eq!((c.len(), &c[4..5], &c[9..10]), (14, "-", "-"), "{}", c);
    }
    assert!(v["hashes"][0].as_str().unwrap().starts_with("$argon2id$"));
    assert_eq!((&v["second"], &v["wrong"], &v["none"]), (&1.into(), &serde_json::Value::Null, &serde_json::Value::Null));
}

// ---- encrypted fields ---------------------------------------------------------------------

const KEYS: &str = "k2026:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=,k2025:HxwdHhsYGRoXFBUWExAREg8MDQ4LCAkKBwQFBgMAAQI";
const OLD_ONLY: &str = "k2025:HxwdHhsYGRoXFBUWExAREg8MDQ4LCAkKBwQFBgMAAQI";

fn eval_keys(src: &str) -> serde_json::Value {
    eval(&src.replace("KEYS", &format!("{:?}", KEYS)).replace("OLD", &format!("{:?}", OLD_ONLY)))
}

#[test]
fn encrypted_values_come_back_and_name_their_key() {
    let v = eval_keys(
        r#"{
    let t = crypto.encrypt("DE89 3704 0044 0532 0130 00", KEYS)
    let old = crypto.encrypt("old secret", OLD)
    {
        t: t,
        again: crypto.encrypt("DE89 3704 0044 0532 0130 00", KEYS) != t,
        back: crypto.decrypt(t, KEYS),
        old_back: crypto.decrypt(old, KEYS),
        moved: crypto.reencrypt(old, KEYS),
        moved_back: crypto.decrypt(crypto.reencrypt(old, KEYS), KEYS),
        kept: crypto.reencrypt(t, KEYS) == t,
        unknown: crypto.decrypt(t, OLD).error,
        changed: crypto.decrypt(t + "A", KEYS).error,
        junk: crypto.decrypt("not a token", KEYS).error,
        is_err: is_err(crypto.decrypt("qe1.k2026.AAAA", KEYS)),
    }
}"#,
    );
    let t = v["t"].as_str().unwrap();
    assert!(t.starts_with("qe1.k2026."), "{}", t);
    assert!(t.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)), "{}", t);
    assert_eq!(v["again"], true);
    assert_eq!(v["back"], "DE89 3704 0044 0532 0130 00");
    assert_eq!(v["old_back"], "old secret");
    assert!(v["moved"].as_str().unwrap().starts_with("qe1.k2026."), "{}", v["moved"]);
    assert_eq!(v["moved_back"], "old secret");
    assert_eq!(v["kept"], true);
    assert!(v["unknown"].as_str().unwrap().contains("no key k2026"), "{}", v["unknown"]);
    assert!(v["changed"].as_str().unwrap().starts_with("crypto.decrypt:"), "{}", v["changed"]);
    assert!(v["junk"].as_str().unwrap().contains("not an encrypted value"), "{}", v["junk"]);
    assert_eq!(v["is_err"], true);
}

#[test]
fn keys_come_from_the_environment() {
    // The only test that sets it.
    std::env::set_var("QUILLO_ENCRYPTION_KEYS", KEYS);
    let v = eval(r#"{ let t = crypto.encrypt("4111 1111 1111 1111") ; [crypto.decrypt(t), crypto.decrypt(t, "k2026:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8")] }"#);
    assert_eq!(v, serde_json::json!(["4111 1111 1111 1111", "4111 1111 1111 1111"]));
    std::env::set_var("QUILLO_ENCRYPTION_KEYS", "k1:short");
    let e = eval(r#"crypto.encrypt("x").error"#);
    assert!(e.as_str().unwrap().contains("not base64") || e.as_str().unwrap().contains("32"), "{}", e);
    std::env::remove_var("QUILLO_ENCRYPTION_KEYS");
    let e = eval(r#"crypto.decrypt("qe1.k1.AAAA").error"#);
    assert!(e.as_str().unwrap().contains("QUILLO_ENCRYPTION_KEYS is not set"), "{}", e);
}

#[test]
fn keys_and_plain_text_stay_out_of_the_browser() {
    let check = |src: &str| -> Vec<String> {
        quillo::check::check_source(src, "t.qwe", std::path::Path::new(".")).errors().map(|d| d.message.clone()).collect()
    };
    let e = check("component C() { <p>{crypto.decrypt(\"x\")}</p> }");
    assert!(!e.is_empty());
    let e = check("fn f(s: str) with db = crypto.encrypt(s)");
    assert!(e.iter().any(|m| m.contains("random")), "{:?}", e);
}

// ---- webhook signatures -------------------------------------------------------------------

#[test]
fn webhooks_signed_as_github_and_shopify_sign_them() {
    // GitHub's documented example.
    let v = eval(
        r#"[
    crypto.verify_webhook("Hello, World!", "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17", "It's a Secret to Everybody"),
    crypto.verify_webhook("Hello, World!", "757107EA0EB2509FC211221CCE984B8A37570B6D7586C22C46F4379C8B043E17", "It's a Secret to Everybody"),
    crypto.verify_webhook("Hello, World!!", "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17", "It's a Secret to Everybody").error,
    crypto.verify_webhook("{\"id\":820982911946154508,\"email\":\"jon@example.com\"}", "vo+n5h/cLxNKGlgJ0Fd1aDzU0aOaajK8Hs9m0hPqWiQ=", "shpss_secret", { scheme: "hmac-sha256" }),
    crypto.verify_webhook("{\"id\":820982911946154508,\"email\":\"jon@example.com\"}", "vo+n5h/cLxNKGlgJ0Fd1aDzU0aOaajK8Hs9m0hPqWiQ=", "another").error,
    crypto.verify_webhook("x", null, "s").error,
    crypto.verify_webhook("x", "sha256=00", null).error,
]"#,
    );
    assert_eq!((&v[0], &v[1], &v[3]), (&true.into(), &true.into(), &true.into()), "{}", v);
    assert!(v[2].as_str().unwrap().contains("does not match"), "{}", v);
    assert!(v[4].as_str().unwrap().contains("does not match"), "{}", v);
    assert!(v[5].as_str().unwrap().contains("no signature header"), "{}", v);
    assert!(v[6].as_str().unwrap().contains("secret is missing"), "{}", v);
}

#[test]
fn stripe_signatures_and_their_time() {
    let body = r#"{\"id\":\"evt_1\",\"object\":\"event\",\"type\":\"payment_intent.succeeded\"}"#;
    let good = "5e4d9743a29fffddf7c29c801815b65d5d42bb9b9516857cb191da0c28b23233";
    let src = format!(
        r#"[
    crypto.verify_webhook("{b}", "t=1492774577,v1={g},v0=6ffbb59b2300aae63f272406069a9788598b792a944a07aba816edb039989a39", "whsec_test_secret", {{ tolerance: "100000d" }}),
    crypto.verify_webhook("{b}", "t=1492774577,v1=0000000000000000000000000000000000000000000000000000000000000000,v1={g}", "whsec_test_secret", {{ tolerance: "100000d" }}),
    crypto.verify_webhook("{b}", "t=1492774577,v1={g}", "whsec_test_secret").error,
    crypto.verify_webhook("{b}", "t=1492774578,v1={g}", "whsec_test_secret", {{ tolerance: "100000d" }}).error,
    crypto.verify_webhook("{b} ", "t=1492774577,v1={g}", "whsec_test_secret", {{ tolerance: "100000d" }}).error,
    crypto.verify_webhook("{b}", "v1={g}", "whsec_test_secret", {{ scheme: "stripe" }}).error,
]"#,
        b = body,
        g = good
    );
    let v = eval(&src);
    assert_eq!((&v[0], &v[1]), (&true.into(), &true.into()), "{}", v);
    assert!(v[2].as_str().unwrap().contains("tolerance"), "{}", v);
    assert!(v[3].as_str().unwrap().contains("no signature matches"), "{}", v);
    assert!(v[4].as_str().unwrap().contains("no signature matches"), "{}", v);
    assert!(v[5].as_str().unwrap().contains("no timestamp"), "{}", v);
    // Signed now: within the default five minutes.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let sig = quillo::crypto::hex(&quillo::crypto::hmac_sha256(b"whsec_x", format!("{}.payload", now).as_bytes()));
    let v = eval(&format!(r#"crypto.verify_webhook("payload", "t={},v1={}", "whsec_x")"#, now, sig));
    assert_eq!(v, true);
}

#[test]
fn a_webhook_route_checks_the_raw_body() {
    use quillo::server::HttpRequest;
    let src = r#"
route POST /hook {
    let ok = crypto.verify_webhook(request.body, request.headers["x-hub-signature-256"], "It's a Secret to Everybody")
    ok ? "ok" : respond(400, ok.error)
}
"#;
    let mut it = quillo::Interp::new(".");
    it.load_source(src, "app.qwe").unwrap();
    let send = |it: &mut quillo::Interp, body: &str, sig: &str| {
        let headers = vec![("Content-Type".to_string(), "application/json".to_string()), ("X-Hub-Signature-256".to_string(), sig.to_string())];
        it.handle(&HttpRequest { method: "POST".into(), url: "/hook".into(), headers, body: body.as_bytes().to_vec(), remote: "1.2.3.4".into() })
    };
    // JSON with spacing a parse and stringify would not keep.
    let body = "{ \"zen\" :  \"Hello\",\n  \"n\": 1.50 }";
    let sig = format!("sha256={}", quillo::crypto::hex(&quillo::crypto::hmac_sha256(b"It's a Secret to Everybody", body.as_bytes())));
    let r = send(&mut it, body, &sig);
    assert_eq!((r.status, r.text()), (200, "ok".to_string()));
    let r = send(&mut it, "{\"zen\":\"Hello\",\"n\":1.5}", &sig);
    assert_eq!(r.status, 400, "{}", r.text());
}
