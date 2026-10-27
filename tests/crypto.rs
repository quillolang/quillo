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

/// Secrets in the environment, as a server starting finds them: signing
/// secrets shorter than 32 bytes are warned about.
#[test]
fn short_secrets_are_warned_about() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    assert!(quillo::crypto::secret_problems(env(&[])).is_empty());
    let w = quillo::crypto::secret_problems(env(&[
        ("QUILLO_SECRET", "hunter2"),
        ("QUILLO_SERVICE_SECRET", "0123456789abcdef0123456789abcde"),
        // 32: enough.
        ("LIMER_ADMIN_SECRET", "0123456789abcdef0123456789abcdef"),
    ]));
    assert_eq!(w.len(), 2, "{:?}", w);
    assert!(w[0].starts_with("QUILLO_SECRET is 7 bytes") && w[1].starts_with("QUILLO_SERVICE_SECRET is 31 bytes"), "{:?}", w);
}

/// `quillo run` says so on stderr, and starts all the same.
#[test]
fn quillo_run_checks_its_secrets_first() {
    let d = std::env::temp_dir().join(format!("quillo-secrets-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("app.qwe");
    std::fs::write(&f, "print(\"ran\")\n").unwrap();
    let run = |env: &[(&str, &str)]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(&f)
            .env_remove("QUILLO_SECRET")
            .env_remove("QUILLO_SERVICE_SECRET")
            .env_remove("LIMER_ADMIN_SECRET")
            .envs(env.iter().copied())
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (code, out, err) = run(&[("QUILLO_SECRET", "short")]);
    assert_eq!((code, out.trim()), (Some(0), "ran"), "{}", err);
    assert!(err.contains("QUILLO_SECRET is 5 bytes"), "{}", err);
    let _ = std::fs::remove_dir_all(&d);
}
