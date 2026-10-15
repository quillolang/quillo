//! Union types, `! E` error types, `with` effects and the builtins behind them.

use wisp::{Interp, Value};

fn ev_in(dir: &std::path::Path, src: &str) -> Result<Value, String> {
    Interp::new(dir).load_source(src, "test.wisp").map_err(|e| e.to_string())
}

fn ev(src: &str) -> String {
    match ev_in(std::path::Path::new("."), src) {
        Ok(v) => v.repr(),
        Err(e) => panic!("{}\n--- source ---\n{}", e, src),
    }
}

fn ev_err(src: &str) -> String {
    match ev_in(std::path::Path::new("."), src) {
        Ok(v) => panic!("expected an error, got {}\n--- source ---\n{}", v.repr(), src),
        Err(e) => e,
    }
}

#[test]
fn union_and_optional_types() {
    assert_eq!(ev("fn f(x: int | str) = x\n[f(1), f(\"a\")]"), "[1, \"a\"]");
    assert!(ev_err("fn f(x: int | str) = x\nf(1.5)").contains("must be int | str, got float"));
    assert_eq!(ev("fn f(x: int | nil) = x ?? 0\nf(nil)"), "0");
    assert!(ev_err("fn f() -> int | str = [1]\nf()").contains("must return int | str"));
    assert!(ev_err("fn f(x: banana) = x").contains("unknown type `banana`"));
}

#[test]
fn declared_error_types_are_checked() {
    let src = r#"
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err("bad age: {s}")
    }
[parse_age("41"), parse_age("x")]
"#;
    assert_eq!(ev(src), "[41, err(\"bad age: x\")]");
    let e = ev_err("fn f() -> int ! str = err({ code: 1 })\nf()");
    assert!(e.contains("declares errors as `! str`") && e.contains("err({ code: 1 })"), "{}", e);
    // `?` passes the error through, and it is checked at each declaring function.
    let e = ev_err("fn inner() = err(42)\nfn outer() -> int ! str = inner()? + 1\nouter()");
    assert!(e.contains("`outer` declares errors as `! str`"), "{}", e);
}

#[test]
fn effects_limit_what_a_function_may_do() {
    // Undeclared functions may do anything.
    assert_eq!(ev("fn f() = type_of(now())\nf()"), "\"int\"");
    // A declared effect allows the builtins that need it.
    assert_eq!(ev("fn f() with time = type_of(now())\nf()"), "\"int\"");
    let e = ev_err("fn f() with db = now()\nf()");
    assert!(e.contains("`now` needs the time effect, but `f` is declared `with db`"), "{}", e);
    // The limit applies to everything the function calls, including lambdas.
    let e = ev_err("fn helper() = env(\"HOME\")\nfn f() with db = [1] |> map(fn(_) = helper())\nf()");
    assert!(e.contains("`env` needs the env effect"), "{}", e);
    // Calling a function that declares more than the caller allows is an error.
    let e = ev_err("fn load() with http, db = 1\nfn f() with db = load()\nf()");
    assert!(e.contains("`load` needs the http effect, but `f` is declared `with db`"), "{}", e);
    // ...and calling one that declares less is fine.
    assert_eq!(ev("fn pure_part() with db = 2\nfn f() with db, time = pure_part()\nf()"), "2");
    assert!(ev_err("fn f() with teleport = 1").contains("expected an effect after `with`"));
    // Pure builtins are always allowed.
    assert_eq!(ev("fn f(xs) with db = xs |> map(fn(x) = x * 2) |> sum\nf([1, 2])"), "6");
}

#[test]
fn sqlite_database() {
    let src = r#"
db.open(":memory:")
db.exec("create table todos (id integer primary key, text text not null, done integer default 0)")
let a = db.exec("insert into todos (text) values (?)", "write docs")
db.exec("insert into todos (text, done) values (:text, :done)", { text: "ship", done: true })
let all = db.query("select id, text, done from todos order by id")
let one = db.one("select text from todos where id = ?", 2)
let none = db.one("select text from todos where id = ?", 99)
let bad = db.query("select * from nope")
[a, all, one, none, is_err(bad), bad.error |> contains("no such table")]
"#;
    assert_eq!(
        ev(src),
        "[{ changes: 1, last_id: 1 }, [{ id: 1, text: \"write docs\", done: 0 }, { id: 2, text: \"ship\", done: 1 }], \
         { text: \"ship\" }, nil, true, true]"
    );
}

#[test]
fn sqlite_transactions_roll_back_on_failure() {
    let src = r#"
db.open(":memory:")
db.exec("create table t (n integer)")
db.transaction(fn() { db.exec("insert into t values (1)") })
db.transaction(fn() {
    db.exec("insert into t values (2)")
    err("changed my mind")
})
let r = db.transaction(fn() {
    db.exec("insert into t values (3)")
    db.query("select count(*) as c from t")[0].c
})
[r, db.query("select n from t order by n") |> map(fn(row) = row.n)]
"#;
    assert_eq!(ev(src), "[2, [1, 3]]");
    let e = ev_err("db.open(\":memory:\")\ndb.exec(\"create table t (n integer)\")\ndb.transaction(fn() { db.exec(\"insert into t values (1)\")\n 1 / 0 })");
    assert!(e.contains("division by zero"));
}

#[test]
fn sqlite_wrong_parameter_count_is_an_error_value() {
    let r = ev("db.exec(\"create table t (a, b)\")\ndb.exec(\"insert into t values (?, ?)\", 1)");
    assert!(r.contains("err(") && r.contains("2 parameters, got 1"), "{}", r);
}

#[test]
fn files_stay_inside_the_program_folder() {
    let dir = std::env::temp_dir().join(format!("wisp-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let r = ev_in(
        &dir,
        r#"
write_file("notes/a.txt", "hello")
[read_file("notes/a.txt"), is_err(read_file("missing.txt")), is_err(read_file("../x")), is_err(write_file("/tmp/x", "no"))]
"#,
    )
    .unwrap();
    assert_eq!(r.repr(), "[\"hello\", true, true, true]");
    assert_eq!(std::fs::read_to_string(dir.join("notes/a.txt")).unwrap(), "hello");
}

#[test]
fn random_values() {
    assert_eq!(ev("let r = random()\nr >= 0 and r < 1"), "true");
    assert_eq!(ev("range(200) |> map(fn(_) = random_int(1, 3)) |> unique |> sort"), "[1, 2, 3]");
    assert_eq!(ev("let u = uuid()\n[len(u), u[14]]"), "[36, \"4\"]");
    assert!(ev_err("fn roll() with db = random_int(1, 6)\nroll()").contains("the random effect"));
}

#[test]
fn math_and_list_helpers() {
    assert_eq!(ev("[abs(-3), abs(-2.5), round(2.5), round(3.14159, 2), floor(2.7), ceil(2.1), sqrt(9), pow(2, 10), pow(2, 0.5) > 1.41]"),
        "[3, 2.5, 3, 3.14, 2, 3, 3.0, 1024, true]");
    assert_eq!(ev("[unique([1, 2, 1, 3]), flatten([[1], [2, 3], 4]), zip([1, 2], [\"a\", \"b\"])]"),
        "[[1, 2, 3], [1, 2, 3, 4], [[1, \"a\"], [2, \"b\"]]]");
    assert_eq!(ev("[take([1, 2, 3], 2), drop([1, 2, 3], 2), take(\"hello\", 2), count([1, 2, 3, 4], fn(x) = x % 2 == 0)]"),
        "[[1, 2], [3], \"he\", 2]");
    assert_eq!(ev("group_by([1, 2, 3, 4, 5], fn(x) = if x % 2 == 0 then \"even\" else \"odd\")"),
        "{ odd: [1, 3, 5], even: [2, 4] }");
    assert_eq!(ev("[index_of([5, 6, 7], 7), index_of(\"héllo\", \"l\"), index_of([1], 9), repeat(\"ab\", 3)]"),
        "[2, 2, nil, \"ababab\"]");
}

#[test]
fn missing_record_fields_named_like_builtins_are_nil() {
    assert_eq!(ev("let order = { id: 1 }\n[order.count, order.first, order.len, { a: 1 }.keys]"), "[nil, nil, 1, [\"a\"]]");
}
