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
