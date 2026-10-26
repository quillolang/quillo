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

#[test]
fn methods_are_the_scripts_own_builtins() {
    assert_eq!(run(r#"run_script("name.upper + `${tags.len}`", { name: "ada", tags: [1, 2] })"#), "\"ADA2\"");
    assert_eq!(run(r#"run_script("items.sort_by((x) => x.n).first.n", { items: [{ n: 2 }, { n: 1 }] })"#), "1");
    let e = run(r#"run_script("\"HOME\".env", {})"#);
    assert!(e.contains("has no field `env`"), "{}", e);
    let e = run(r#"fn secret(x) = 42
run_script("1.secret", {})"#);
    assert!(e.contains("has no field `secret`"), "{}", e);
}

#[test]
fn scripts_cannot_make_huge_values() {
    // Doubling a string: refused once it passes 1 MB.
    let e = run(r#"run_script("let s = \"x\"\nfor i in 0..28 { s = s + s }\nlen(s)", {})"#);
    assert!(e.contains("larger than 1 MB"), "{:.200}", e);
    let e = run(r#"run_script("let xs = [1]\nfor i in 0..28 { xs = xs + xs }\nlen(xs)", {})"#);
    assert!(e.contains("larger than 1 MB"), "{:.200}", e);
    // Lists that share themselves count each time they appear.
    let e = run(r#"run_script("let xs = [1]\nfor i in 0..40 { xs = [xs, xs] }\nstr(xs)", {})"#);
    assert!(e.contains("larger than 1 MB"), "{:.200}", e);
    let e = run(r#"run_script("let xs = 0\nfor i in 0..1000 { xs = [xs] }\n1", {})"#);
    assert!(e.contains("too deeply"), "{:.200}", e);
    // Builtins that make much more than they are given are stopped before.
    let e = run(r#"run_script("let s = join(map(0..1000, (i) => \"x\"), \"\")\nreplace(s + s, \"\", s)", {})"#);
    assert!(e.contains("larger than 1 MB"), "{:.200}", e);
    let e = run(r#"run_script("let s = join(map(0..1000, (i) => \"x\"), \"\")\njoin(map(0..1000, (i) => \"\"), s + s)", {})"#);
    assert!(e.contains("larger than 1 MB"), "{:.200}", e);
    let e = run(r#"run_script("let s = join(map(0..1000, (i) => \"x\"), \"\")\n`${s}${s}`.len", {})"#);
    assert_eq!(e, "2000");
    let e = run(r#"run_script("len(0..10000000)", {})"#);
    assert!(e.contains("larger than 1 MB"), "{:.200}", e);
    // Big values cost steps.
    let e = run(r#"run_script("let s = join(map(0..1000, (i) => \"x\"), \"\")\nlet t = s\nfor i in 0..9 { t = t + t }\nfor i in 0..40 { len(t + \"y\") }", {})"#);
    assert!(e.contains("too many steps"), "{:.200}", e);
}
