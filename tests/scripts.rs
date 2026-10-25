//! `run_script`: code a program's users write, run on the server in a
//! sandbox — only the variables given and builtins that compute, within a
//! budget of steps.

fn run(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}", e),
    }
}

#[test]
fn scripts_compute_from_what_they_are_given() {
    assert_eq!(run(r#"run_script("value > 3", { value: 5 })"#), "true");
    assert_eq!(run(r#"run_script("let n = len(record.tags)\nn >= 2 ? null : `needs two tags, has ${n}`", { record: { tags: ["a"] } })"#), "\"needs two tags, has 1\"");
    assert_eq!(run(r#"run_script("items | filter((x) => x > 1) | map((x) => x * 10) | sum", { items: [1, 2, 3] })"#), "50");
    // Helpers the program passes in are theirs to call.
    assert_eq!(run(r#"fn taken(p) = p == "AB-123"
run_script("taken(value) ? `${value} is taken` : null", { value: "AB-123", taken })"#), "\"AB-123 is taken\"");
}

#[test]
fn scripts_cannot_reach_the_database_files_or_the_program() {
    let e = run(r#"run_script("db.query(\"select 1\")", {})"#);
    assert!(e.contains("unknown name `db`"), "{}", e);
    let e = run(r#"run_script("read_file(\"/etc/passwd\")", {})"#);
    assert!(e.contains("unknown name `read_file`"), "{}", e);
    let e = run(r#"fn secret() = 42
run_script("secret()", {})"#);
    assert!(e.contains("unknown name `secret`"), "{}", e);
    for src in ["fn f() = 1", "reply 200 \"x\"", "<b>x</b>", "use \"other.qwe\""] {
        let e = run(&format!("run_script({:?}, {{}})", src));
        assert!(e.starts_with("err("), "{}: {}", src, e);
    }
    let e = run(r#"run_script("let x = ", {})"#);
    assert!(e.starts_with("err(\"line 1"), "{}", e);
}

#[test]
fn scripts_stop_after_their_budget() {
    let e = run(r#"run_script("let f = (g, n) => g(g, n + 1)\nf(f, 0)", {})"#);
    assert!(e.contains("too many steps") || e.contains("too much recursion"), "{}", e);
    assert_eq!(run(r#"run_script("for x in items => x * 2", { items: [1, 2, 3] })"#), "[2, 4, 6]");
    let big = r#"let big = reduce(split("0123456789", ""), [], (acc, a) => acc + split("0123456789012345678901234567890123456789", ""))
let bigger = reduce(split("012345678901234567890123456789012345678901234567890123456789", ""), [], (acc, a) => acc + big)
run_script("for x in items => x", { items: bigger })"#;
    let e = run(big);
    assert!(e.contains("too many steps"), "{}", e);
    // The budget is the script's: the program runs on, unlimited, after it.
    assert_eq!(run(r#"run_script("1 +", {})
len(range(30000) | map((x) => x))"#), "30000");
}
