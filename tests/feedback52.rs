//! FEEDBACK52.md: what `quillo check` says of a `return` where a block's value is used, of a
//! function that gives a value on some paths and null at its end, and of code after a `return`
//! (Q-678 to Q-680; branch feedback52). All three are warnings: the programs run as they did.
//! Each program is checked in this process, or by the quillo that QUILLO_TEST_BIN names (main's,
//! say, to see a test fail there).

mod support;

use quillo::check::{check_source, Severity};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What `quillo check` says of `src`: a line a diagnostic, its line, `warning:` for a warning,
/// and the message.
fn diags(src: &str) -> Vec<String> {
    let Some(bin) = std::env::var_os("QUILLO_TEST_BIN") else {
        return check_source(src, "t.qwe", Path::new("."))
            .diagnostics
            .iter()
            .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
            .collect();
    };
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = support::temp_dir(&format!("feedback52-{}", N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(dir.join("t.qwe"), src).unwrap();
    let mut child = Command::new(bin).arg("check").arg("t.qwe").current_dir(&dir).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            panic!("quillo check took over 30 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    text.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("t.qwe:")?;
            let (line, rest) = rest.split_once(": ")?;
            line.parse::<u32>().ok()?;
            match rest.split_once(": ")? {
                ("warning", m) => Some(format!("{} warning: {}", line, m)),
                ("error", m) => Some(format!("{} {}", line, m)),
                _ => None,
            }
        })
        .collect()
}

/// What these three say, and the `return` at a file's top level (Q-818), of `src`.
fn said(src: &str) -> Vec<String> {
    diags(src)
        .into_iter()
        .filter(|d| ["this `return` leaves", "every way of this `", "returns a value on line", "gives null when this `if`", "never runs: "].iter().any(|m| d.contains(m)))
        .collect()
}

/// What Q-678 says of an `if` or a `match` each of whose ways leaves (review of 1a1b3ae, M1).
fn every(line: u32, what: &str, f: &str, give: &str) -> String {
    format!("{line} warning: every way of this `{what}` leaves {f}, so the value it is used for is never given: to give it, {give}; to leave, write the `{what}` as a statement")
}

// ---- Q-678: `return` in a block whose value is used ------------------------------------------

#[test]
fn a_return_where_the_value_of_a_for_an_if_or_a_match_is_kept_is_said() {
    // The report's three, as P1 has them. `label` and `kind` have the shape of a guard, one way
    // leaving and another giving a value, and are not said since the review of 1a1b3ae (M1):
    // `label2` and `kind2`, each of whose ways leaves, are.
    let src = r#"fn doubled_for(xs) {
    let ys = for x in xs => { return x * 2 }
    return ys
}
fn label(n) {
    let l = if n == 1 { return "one" } else { "many" }
    return `${n} ${l}`
}
fn kind(x) {
    let k = match x {
        0 => { return "zero" }
        _ => "other"
    }
    return `kind ${k}`
}
fn label2(n) {
    let l = if n == 1 { return "one" } else { return "many" }
    return `${n} ${l}`
}
fn kind2(x) {
    let k = match x {
        0 => { return "zero" }
        _ => { return "other" }
    }
    return `kind ${k}`
}
print([doubled_for([1, 2, 3]), label(1), kind(0), label2(1), kind2(0)])
"#;
    assert_eq!(
        said(src),
        [
            "2 warning: this `return` leaves `doubled_for` in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so write the value without `return`".to_string(),
            every(17, "if", "`label2`", "write `\"one\"`"),
            every(22, "match", "`kind2`", "write `\"zero\"`"),
        ]
    );
}

#[test]
fn a_return_in_an_argument_an_operand_or_a_block_kept_is_said() {
    let src = r#"fn arg(c) {
    print(if c { return 1 } else { return 2 })
    3
}
fn operand(c) {
    let n = 1 + (if c { return 1 } else { return 2 })
    n
}
fn block() {
    let v = { return 5 }
    v
}
fn kept_for(xs) {
    print(for x in xs => { return x })
    0
}
fn out_for(xs) = for x in xs => { return x }
fn both_leave(c) {
    let l = if c { return "a" } else { return "b" }
    l
}
fn no_value(c) {
    let l = if c { return } else { return }
    l
}
fn grüße(c) {
    let l = if c { return "ünï" } else { return "ß" }
    l
}
component Box(c) {
    <div>{if c { return <p>a</p> } else { return <p>b</p> }}</div>
}
"#;
    assert_eq!(
        said(src),
        [
            every(2, "if", "`arg`", "write `1`"),
            every(6, "if", "`operand`", "write `1`"),
            "10 warning: this `return` leaves `block`, so the block never gives the value it is used for: to give it, write `5`; to leave, write the `return` as a statement".to_string(),
            "14 warning: this `return` leaves `kept_for` in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so write `x`".to_string(),
            "17 warning: this `return` leaves `out_for` in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so write `x`".to_string(),
            // Every way leaves: the `if` never gives the value the `let` waits for. Said once.
            every(19, "if", "`both_leave`", "write `\"a\"`"),
            every(23, "if", "`no_value`", "write the value"),
            every(27, "if", "`grüße`", "write `\"ünï\"`"),
            every(31, "if", "`Box`", "write the value without `return`"),
        ]
    );
}

#[test]
fn a_return_that_is_an_early_exit_or_gives_the_functions_value_is_not_said() {
    // A guard leaves with null, an error, or what a call gives; a value of another kind than
    // the other way's may be one too. Last in a function, or as `return`'s value, a `return`
    // gives what the `if` would have given.
    let src = r#"fn first_word(s: str) {
    let m = re.find(s, "\\w+") ?? return null
    m.text
}
fn guard(x) {
    let n = if x == null { return null } else { x.n }
    let m = if x.bad { return err("bad") } else { 2 }
    let k = match x.kind { "a" => return 1, _ => x.k }
    let ok = x.ok || return false
    let z = x.z && return 0
    let l = if x.long { return 1 } else { "short" }
    let one = if x.one { return 1 }
    n + m + k
}
fn delegate(field, value) {
    let r = match field {
        "title" => value
        "column" => { return move_it(value) }
        _ => "other"
    }
    r
}
fn move_it(v) = v
fn tail(c) {
    if c { return 1 } else { 2 }
}
fn tail_match(x) = match x { 0 => return "zero", _ => "other" }
fn out(c) {
    return if c { return 1 } else { 2 }
}
fn early(xs) {
    for x in xs { if x == 0 { return 0 } }
    let ys = for x in xs => { if x < 0 { return err("negative") }; x * 2 }
    let zs = for x in xs => { if x < 0 { return null } else { x } }
    len(ys) + len(zs)
}
fn mapped(xs) {
    return xs | map((x) => { return x * 2 })
}
fn run(f) = f()
fn outer() {
    let v = run(() => { return 7 })
    return v + 1
}
fn text(x) {
    let shown = `${x ?? return "none"}`
    shown
}
print([first_word("a b"), delegate("title", 1), tail(true), tail_match(0), out(true), early([1]), mapped([1]), outer(), text(1)])
"#;
    assert_eq!(said(src), Vec::<String>::new());
}

#[test]
fn a_return_in_markup_a_view_a_component_and_a_route_as_the_guide_writes_them_is_not_said() {
    // Part B's P2 and P3; `{ return ... }` as an element's child keeps its own message (Q-448).
    let src = r#"fn twice(n) {
    return n * 2
}
view Card(title) {
    return <section><h2>{title}</h2></section>
}
component Counter() {
    state n = 0
    let shown = (k) => { return `k=${twice(k)}` }
    if n > 2 { return <p id="done">done {n}</p> }
    return <button id="b" @click={() => { n += 1 }}>{shown(n)}</button>
}
route GET / {
    return <main><Card title="a"/><Counter/></main>
}
route GET /early {
    if request.query.x == "1" { return <p>early</p> }
    <p>late</p>
}
route GET /r {
    if request.query.x == "1" { reply 404 "no" }
    reply 200 "yes"
}
route GET /pre {
    <pre>f() { return 1 }</pre>
}
"#;
    let d = diags(src);
    assert_eq!(said(src), Vec::<String>::new());
    assert!(d.iter().any(|m| m.contains("`{ return ... }` in <pre> leaves the function")), "{:?}", d);
}

// ---- Q-679: a value on some paths, null at the end --------------------------------------------

#[test]
fn a_function_that_returns_a_value_and_reaches_its_end_with_null_is_said() {
    let src = r#"fn both(c) {
    if c { return 1 }
}
fn none2(c) { if c { 1 } }
fn ends_let(c) {
    if c { return "yes" }
    let x = 1
}
fn grade(n) {
    if n > 90 { "A" } else if n > 80 { "B" }
}
fn holds(xs, v) {
    for x in xs {
        if x == v { return true }
    }
}
fn admins(r) { if session().user == "root" { return r } }
fn one_item(c) {
    if c { return [1] }
    while c { c = false }
}
fn local() {
    fn inner(c) { if c { return {} } }
    inner(true)
}
fn ünï(c) { if c { return "é" } }
print([both(false), none2(false), ends_let(false), grade(1), holds([1], 2), one_item(false), local(), ünï(false)])
"#;
    let null_end = |f: &str, at: u32| format!("`{f}` returns a value on line {at} but gives null when it reaches its end: end it with a value too, or `null` if null is meant");
    let no_else = |f: &str| format!("`{f}` gives null when this `if` is false, as it has no `else`: write one, or `else {{ null }}` if null is meant");
    assert_eq!(
        said(src),
        [
            format!("2 warning: {}", null_end("both", 2)),
            format!("4 warning: {}", no_else("none2")),
            format!("7 warning: {}", null_end("ends_let", 6)),
            format!("10 warning: {}", no_else("grade")),
            "13 warning: `holds` returns a value on line 14 but, when it reaches its end, gives the list of nulls its `for` makes: end it with the value it should give, or `null`".to_string(),
            format!("17 warning: {}", null_end("admins", 17)),
            format!("20 warning: {}", null_end("one_item", 19)),
            format!("23 warning: {}", null_end("inner", 23)),
            format!("26 warning: {}", null_end("ünï", 26)),
        ]
    );
}

#[test]
fn a_function_whose_end_gives_a_value_or_null_as_meant_is_not_said() {
    let src = r#"fn both2(c) {
    if c { return 1 }
    "two"
}
fn explicit(c) { if c { 1 } else { null } }
fn validate(x) {
    if x < 0 { return err("negative") }
}
fn early_null(x) {
    if x == null { return null }
    let y = x
}
fn bare(x) {
    if x { return }
    print(x)
}
fn effect(c) { if c { print("x") } }
fn badge(n) { if n > 0 { <span>{n}</span> } }
fn typed(c: bool) -> int {
    if c { return 1 }
}
view Maybe(x) { if x { return <p>{x}</p> } }
component Shown(c) {
    if c { return <p>yes</p> }
}
let arrow = (c) => { if c { return 1 } }
fn empty() { }
fn call_end(c) {
    if c { return 1 }
    print("no")
}
fn all_arms(x) {
    match x {
        0 => { return "zero" }
        _ => { return "other" }
    }
}
fn loops() {
    let i = 0
    while true {
        i += 1
        if i > 3 { return i }
    }
}
fn each(xs) {
    for x in xs { print(x) }
}
fn replies(c) {
    if c { return 1 }
    reply 400 "no"
}
fn answered(c) {
    if c { return reply 404 "no" }
}
fn called(c, f) { if c { f && f() } }
print([both2(false), explicit(false), validate(1), early_null(1), bare(1), effect(false), badge(0), Maybe(null), arrow(false), empty(), call_end(false), all_arms(0), loops(), each([])])
"#;
    let d = diags(src);
    assert_eq!(said(src), Vec::<String>::new());
    // A typed function has its own, stronger check.
    assert!(d.contains(&"20 `typed` must return int, but this returns null".to_string()), "{:?}", d);
}

#[test]
fn a_list_made_on_purpose_and_a_function_handed_to_other_code_are_not_said() {
    // As quillo-components' activity_rings_clean, touch_chart_ticks and voice_message_bars
    // are written: an early `return []`, then the `for` whose list is the answer.
    let src = r#"fn clean(points) {
    if type_of(points) != "list" { return [] }
    for p in points => p * 2
}
fn bars(peaks, n: int) {
    if len(peaks) == 0 { return for _i in range(n) => 0.3 }
    for i in range(n) => {
        let p = peaks[i % len(peaks)]
        p
    }
}
fn rounds(xs) {
    if xs == [] { return 0 }
    for x in xs { if x > 1 { x } }
}
fn admins(r) {
    if r.headers["x-user"] == "root" {
        return r
    }
}
routes /admin | admins { route GET /x = "admin" }
fn keep(x) {
    if x > 1 { return true }
}
fn named(x) {
    if x.ok { return x.name }
}
fn twice(x) {
    if x > 1 { return x * 2 }
}
fn thrice(x) {
    if x > 1 { return x * 3 }
}
fn role(name) = (r) => r
routes /r | role("a") { route GET /y = "y" }
component C() {
    <button @click={named}>{len(filter([1, 2], keep))}</button>
}
route GET / {
    let piped = [3] | map(twice)
    let thrown = 2 | thrice
    <main>{clean([1])}{bars([], 2)}{rounds([1])}{piped}{thrown}<C/></main>
}
"#;
    // `x | thrice` is a call of it.
    let thrice = "32 warning: `thrice` returns a value on line 32 but gives null when it reaches its end: end it with a value too, or `null` if null is meant";
    assert_eq!(said(src), [thrice]);
    // Called, not handed over, `keep` is said; called and handed over, it is not.
    let called = src.replace("filter([1, 2], keep)", "[keep(1)]");
    assert_eq!(said(&called), ["23 warning: `keep` returns a value on line 23 but gives null when it reaches its end: end it with a value too, or `null` if null is meant", thrice]);
    assert_eq!(said(&src.replace("filter([1, 2], keep)", "filter([keep(1)], keep)")), [thrice]);
}

// ---- Q-680: code that never runs -------------------------------------------------------------

#[test]
fn code_after_a_return_a_reply_or_an_if_or_match_that_leaves_each_way_is_said() {
    let src = r#"fn after() {
    return 1
    print("never")
}
fn after_reply() {
    reply 403 "no"
    print("never")
}
fn after_both(c) {
    if c { return 1 } else { return 2 }
    print("never")
}
fn after_match(x) {
    match x {
        0 => return "zero"
        _ => return "other"
    }
    print("never")
}
fn nested(c) {
    if c {
        return 1
        print("never")
    }
    2
}
fn literal_after() {
    return 1
    "never"
}
let handler = () => {
    return 1
    print("never")
}
route GET /dead {
    return <p>a</p>
    print("never")
}
fn öde() {
    { return "ü" }
    print("never")
    print("nor this")
}
let mapped = [1] | map((x) => {
    return x
    print("never")
})
"#;
    assert_eq!(
        said(src),
        [
            "3 warning: this never runs: the `return` on line 2 always leaves `after`",
            "7 warning: this never runs: the `reply` on line 6 always ends the request",
            "11 warning: this never runs: each way of the `if` on line 10 leaves `after_both`",
            "18 warning: this never runs: each arm of the `match` on line 14 leaves `after_match`",
            "23 warning: this never runs: the `return` on line 22 always leaves `nested`",
            "28 warning: what follows never runs: the `return` on line 28 always leaves `literal_after`",
            "33 warning: this never runs: the `return` on line 32 always leaves `handler`",
            "37 warning: this never runs: the `return` on line 36 always leaves the route",
            "41 warning: this never runs: the `return` on line 40 always leaves `öde`",
            "46 warning: this never runs: the `return` on line 45 always leaves the function",
        ]
    );
}

#[test]
fn code_after_a_return_on_one_way_only_or_last_is_not_said_and_a_files_return_is_said_once() {
    let src = r#"fn last_return() {
    print("a")
    return 1
}
fn one_way(c) {
    if c { return 1 }
    print("runs")
    2
}
fn match_some(x) {
    match x {
        0 => return "zero"
        _ => print("other")
    }
    print("runs")
}
fn guarded(x) {
    let y = x ?? return null
    print(y)
}
fn no_else(c) {
    if c { return 1 } else if !c { return 2 }
    print("runs when neither")
}
print([last_return(), one_way(false), match_some(1), guarded(1), no_else(false)])
return
print("b")
"#;
    assert_eq!(said(src), ["27 warning: this never runs: the `return` at line 26 leaves the file first, so nothing below it is defined"]);
}

// ---- every form the walk reads ---------------------------------------------------------------

/// Each place a value goes: a `return` there is said where every way leaves. How the other way's
/// value is written no longer matters (`kinds`, quiet since the review of 1a1b3ae, M1). A type's
/// default, a service's options, a route's `= expr`, an `if true` with no line of its own.
#[test]
fn every_place_a_value_goes_and_every_kind_written_is_read() {
    let src = r#"type Item = { role: str = if true { "a" } else { "b" } }
service billing = "billing.qwe" { timeout: "2s" }
service shop = "shop.qwe"
mount /docs = "docs.qwe"
let top = if true { return 1 } else { return 2 }
fn kinds(c, d = if c { 1 } else { 2 }) {
    let b = if c { return true } else { !c }
    let n = if c { return -1 } else { 2 % 3 }
    let s = if c { return "a" + d } else { `x` }
    let t = if c { return d - 1 } else { 1 + d }
    let q = if c { return c + d } else { c + d }
    let l = if c { return [1] } else { [] }
    let m = if c { return { a: 1 } } else { { b: 2 } }
    let e = if c { return 1 == d } else { c < d }
    let g = match d { k if k > 1 => { return 1 } _ => 2 }
    [b, n, s, t, q, l, m, e, g]
}
fn places(x, xs) {
    let a = xs[if x { return 1 } else { return 2 }]
    let r = (if x { return 1 } else { return 2 })?
    let f = (if x { return { a: 1 } } else { return { a: 2 } }).a
    let p = xs | take(if x { return 1 } else { return 2 })
    while x { x = false }
    if x { reply 400 if x { return "a" } else { return "b" } }
    [a, r, f, p]
}
component Page(xs) {
    state on = false
    let el = js("document.body")
    el.title = if on { return <p>a</p> } else { return <p>b</p> }
    <main#top.card class={if on { return "a" } else { return "b" }} {...{ hidden: on }}>
        <p>{xs}</p>
    </main>
}
fn ends(c, d, x) {
    if c { return 1 }
    if d { let y = 1 } else { 2 }
}
fn ends2(c, d) {
    if c { return 1 }
    if d { 2 } else { let y = 1 }
}
fn ends3(c, x) {
    if c { return 1 }
    match x {
        0 => { let y = 1 }
        _ => 2
    }
}
fn ends4(c) {
    if c { return 1 }
    { let y = 1 }
}
fn ends5(c, d) {
    if c { return 1 }
    if d { let y = 1 }
}
fn lit_cond() { if true { return 1 } }
fn lit_both() {
    if true { return 1 } else { return 2 }
    print("never")
}
fn none_for(xs) {
    if xs == [] { return 0 }
    for x in xs { }
}
route GET /a = if request.query.x == "1" { return "one" } else { "two" }
route GET /b {
    let v = if request.query.x == "1" { return "one" } else { return "two" }
    v
}
print([kinds(true), places(true, [1]), ends(true, true, 1), ends2(true, true), ends3(true, 1), ends4(true), ends5(true, true), lit_cond(), lit_both(), none_for([])])
"#;
    let plain = "write the value without `return`";
    assert_eq!(
        said(src),
        [
            every(5, "if", "the file", "write `1`"),
            every(19, "if", "`places`", "write `1`"),
            every(20, "if", "`places`", "write `1`"),
            every(21, "if", "`places`", plain),
            every(22, "if", "`places`", "write `1`"),
            every(24, "if", "`places`", "write `\"a\"`"),
            every(30, "if", "`Page`", plain),
            every(31, "if", "`Page`", "write `\"a\"`"),
            "37 warning: `ends` returns a value on line 36 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "41 warning: `ends2` returns a value on line 40 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "46 warning: `ends3` returns a value on line 44 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "52 warning: `ends4` returns a value on line 51 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "56 warning: `ends5` returns a value on line 55 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "58 warning: `lit_cond` returns a value on line 58 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "61 warning: this never runs: each way of the `if` on line 60 leaves `lit_both`".to_string(),
            "65 warning: `none_for` returns a value on line 64 but, when it reaches its end, gives the list of nulls its `for` makes: end it with the value it should give, or `null`".to_string(),
            every(69, "if", "the route", "write `\"one\"`"),
        ]
    );
}

// ---- the review of 1a1b3ae (FEEDBACK52.md, Q-678 to Q-680; branch feedback52) ---------------

/// M1: a guard whose early value is written as the other way's kind is a guard still: said by
/// the "same written kind" rule, with a fix that changed what it gives. The guide's own `"a" =>
/// return 1` (USERGUIDE.md, "Coming from JavaScript"), and the report's `label`, which has the
/// same shape as `ratio`: only an `if` each of whose ways leaves is said now.
#[test]
fn a_guard_whose_early_value_is_written_as_the_other_ways_is_not_said() {
    let src = r#"fn ratio(a, b) {
    let d = if b == 0 { return 0 } else { a / b }
    d * 100
}
fn half(n) {
    let y = n > 0 ? n * 2 : return -1
    y + 1
}
fn code(kind) {
    let n = match kind { "a" => return 1, "b" => 2, _ => 3 }
    n * 10
}
fn label(n) {
    let l = if n == 1 { return "one" } else if n == 2 { "two" } else { "many" }
    return `${n} ${l}`
}
fn flag(c) {
    let b = if c { return true } else { false }
    !b
}
component Items() {
    state loading = true
    let shown = if loading { return <p>Loading</p> } else { <ul></ul> }
    <main>{shown}</main>
}
print([ratio(1, 2), half(2), code("b"), label(1), flag(false)])
"#;
    assert_eq!(said(src), Vec::<String>::new());
}

/// M2 and LOW 1: an `else if` chain is one `if` of many ways: each `else if` was an `if` of its
/// own, and a `let` was said once a level, 31 times for 32 ways. Said once, of its first
/// `return`, as is a kept `if` inside a kept `if`'s way.
#[test]
fn an_else_if_chain_is_one_if_and_a_let_is_said_once() {
    let three = r#"fn pick(c, d) {
    let l = if c { return 1 } else if d { return 2 } else { return 3 }
    l
}
fn mid(c, d) {
    let l = if c { return 1 } else if d { reply 400 "no" } else { return 3 }
    l
}
fn nested(c, d) {
    let l = if c { if d { return 1 } else { return 2 } } else { return 3 }
    l
}
fn after(c, d) {
    if c { return 1 } else if d { return 2 } else { return 3 }
    print("never")
}
fn gives(c, d) {
    let l = if c { return 1 } else if d { return 2 } else { 3 }
    l
}
fn open(c, d) {
    let l = if c { return 1 } else if d { return 2 }
    l
}
"#;
    assert_eq!(
        said(three),
        [
            every(2, "if", "`pick`", "write `1`"),
            every(6, "if", "`mid`", "write `1`"),
            every(10, "if", "`nested`", "write `1`"),
            "15 warning: this never runs: each way of the `if` on line 14 leaves `after`".to_string(),
        ]
    );
    // 32 ways, and 32 `if`s each kept in the one before's way.
    let mut chain = "fn many(n) {\n    let l = if n == 0 { return 0 }".to_string();
    let mut deep = "fn deep(n) {\n    let l = ".to_string();
    for i in 1..32 {
        chain.push_str(&format!(" else if n == {i} {{ return {i} }}"));
        deep.push_str(&format!("if n > {i} {{ "));
    }
    chain.push_str(" else { return 99 }\n    l\n}\n");
    deep.push_str("return 0");
    for i in (1..32).rev() {
        deep.push_str(&format!(" }} else {{ return {i} }}"));
    }
    deep.push_str("\n    l\n}\n");
    assert_eq!(said(&chain), [every(2, "if", "`many`", "write `0`")]);
    assert_eq!(said(&deep).len(), 1, "{:?}", said(&deep));
}

/// M3: a markup helper's null end shows nothing, as a view's does. `FnDef` does not keep `view`,
/// so `view card` is known by its markup: a markup `return` is no value. A text or a number
/// returned beside it is.
#[test]
fn a_markup_helper_and_a_lowercase_view_are_not_said_for_a_null_end() {
    let src = r#"fn badge(n) {
    if n == 0 { return <i>none</i> }
    if n > 9 { <b>9+</b> }
}
view card(x) {
    if x { return <p>{x}</p> }
}
fn row(x) {
    if x { return <tr></tr> }
    let y = 1
}
fn mixed(n) {
    if n == 0 { return 0 }
    if n > 9 { <b>9+</b> }
}
route GET / = <main>{badge(1)}{card("a")}{row(true)}{mixed(1)}</main>
"#;
    assert_eq!(said(src), ["14 warning: `mixed` returns a value on line 13 but gives null when it reaches its end: end it with a value too, or `null` if null is meant"]);
}

/// INFO: a `return` at the end of a line takes no value from the next, which never runs: said so.
#[test]
fn a_bare_return_says_why_the_next_line_never_runs() {
    let src = r#"fn f(x) {
    return
    x + 1
}
fn g(x) {
    return
    (x + 1)
}
fn h(x) {
    return null
    x + 1
}
print([f(1), g(1), h(1)])
"#;
    let bare = |line: u32, at: u32, f: &str| format!("{line} warning: this never runs: a `return` at the end of a line gives null, so the one on line {at} leaves `{f}` before this");
    assert_eq!(said(src), [bare(3, 2, "f"), bare(7, 6, "g"), "11 warning: this never runs: the `return` on line 10 always leaves `h`".to_string()]);
}

/// LOW 2: what a way does is worked out once, on the walk's way back up. 16 functions with a kept
/// `if` 250 deep, and statements nested 250 deep, each said once.
#[test]
fn deep_ifs_are_read_once_and_said_once() {
    let mut kept = String::new();
    let mut stmts = String::new();
    for f in 0..16 {
        kept.push_str(&format!("fn k{f}(x) {{\n    let v = "));
        stmts.push_str(&format!("fn s{f}(x) {{\n"));
        for i in 0..250 {
            kept.push_str(&format!("if x > {i} {{ "));
            stmts.push_str(&format!("if x > {i} {{\n"));
        }
        kept.push_str("return 0");
        stmts.push_str("return 0\n");
        for i in (0..250).rev() {
            kept.push_str(&format!(" }} else {{ return {i} }}"));
            stmts.push_str(&format!("}} else {{\nreturn {i}\n}}\n"));
        }
        kept.push_str("\n    v\n}\n");
        stmts.push_str("print(x)\n}\n");
    }
    let k = said(&kept);
    assert_eq!(k.len(), 16, "{:?}", &k[..k.len().min(3)]);
    assert!(k.iter().all(|m| m.contains("every way of this `if` leaves `k")), "{:?}", &k[..3]);
    let s = said(&stmts);
    assert_eq!(s.len(), 16, "{:?}", &s[..s.len().min(3)]);
    assert!(s.iter().all(|m| m.contains("this never runs: each way of the `if` on line")), "{:?}", &s[..3]);
}

// ---- the second review (FEEDBACK52.md, Q-678 to Q-680; branch feedback52) ---------------------

/// MEDIUM 3: an `if` or `match` in a way of a kept one, or on the right of `??`, counts toward
/// whether that way leaves, and only the outermost decides. Each of these is a guard, and the
/// advice ("write `1`") changed what it gives: `ratio(4, 0)` gave 100. A plain block there too.
#[test]
fn an_if_or_match_in_a_guards_way_is_not_said_and_the_outermost_decides() {
    let src = r#"fn ratio(a, b) {
    let d = if b != 0 { a / b } else { if a > 0 { return 1 } else { return -1 } }
    d * 100
}
fn arm(k, s) {
    let n = match k { 1 => 10, _ => if s { return -1 } else { return 0 } }
    n + 1
}
fn inner_match(x, y) {
    let n = if x { 1 } else { match y { 1 => return 1, _ => return 2 } }
    n + 1
}
fn rounds(xs, s) {
    let ys = for x in xs => { if x > 0 { x } else { if s { return -1 } else { return 0 } } }
    len(ys)
}
fn either(x, s) {
    let m = x ?? if s { return 1 } else { return 2 }
    m + 1
}
fn both_sides(x, s) {
    let m = x || match s { 1 => return 1, _ => return 2 }
    m
}
fn blocks(c) {
    let d = if c { 1 } else { { return 2 } }
    d + 1
}
fn still(c, a) {
    let d = if c { return 1 } else { if a { return 2 } else { return 3 } }
    d
}
fn deeper(c, a, b) {
    let d = if c { if a { return 2 } else { return 3 } } else { match b { 1 => return 4, _ => return 5 } }
    d
}
fn each_round(xs) {
    let ys = for x in xs => { if x > 0 { return x } else { return 0 } }
    ys
}
print([ratio(4, 0), arm(2, true), inner_match(false, 1), rounds([1], true), either(null, true), both_sides(false, 1), blocks(false), still(true, true), deeper(true, true, 1), each_round([1])])
"#;
    assert_eq!(
        said(src),
        [
            // Said once, at the outermost, of its first `return` as written.
            every(30, "if", "`still`", "write `1`"),
            every(34, "if", "`deeper`", "write `2`"),
            // A kept `for`'s body is a value of its own: each round leaves.
            every(38, "if", "`each_round`", "write `x`"),
        ]
    );
}

/// LOW 4: a `return` with a statement after it on its own line takes no value from a next line:
/// the plain message. One at the end of its line keeps the other.
#[test]
fn a_bare_return_mid_line_gets_the_plain_message() {
    let src = r#"fn mid_line() {
    print("a"); return; print("b")
}
fn end_of_line(x) {
    print("a"); return
    x
}
fn literal_next() {
    return; "b"
}
print([mid_line(), end_of_line(1), literal_next()])
"#;
    assert_eq!(
        said(src),
        [
            "2 warning: this never runs: the `return` on line 2 always leaves `mid_line`".to_string(),
            "6 warning: this never runs: a `return` at the end of a line gives null, so the one on line 5 leaves `end_of_line` before this".to_string(),
            // A literal has no line: taken to be on the next, as B4 did (third review, LOW 5).
            "9 warning: what follows never runs: a `return` at the end of a line gives null, so the one on line 9 leaves `literal_next` before this".to_string(),
        ]
    );
}

/// LOW 5: an `else if` chain with no `else` at its end whose ways all leave is, as a `for`'s
/// body, an `if` with no `else`: each round gives null or leaves, and the loop finds.
#[test]
fn a_finding_loop_with_an_else_if_is_said() {
    let src = r#"fn finds(xs) {
    for x in xs { if x > 10 { return x } else if x < -10 { return -x } }
}
fn three(xs) {
    for x in xs {
        if x > 10 { return x } else if x < -10 { return -x } else if x == 0 { return 0 }
    }
}
fn gives(xs) {
    for x in xs { if x > 10 { return x } else if x < -10 { x } }
}
fn closed(xs) {
    for x in xs { if x > 10 { return x } else if x < -10 { return -x } else { x } }
}
print([finds([1]), three([1]), gives([1]), closed([1])])
"#;
    let list = |line: u32, f: &str, at: u32| format!("{line} warning: `{f}` returns a value on line {at} but, when it reaches its end, gives the list of nulls its `for` makes: end it with the value it should give, or `null`");
    assert_eq!(said(src), [list(2, "finds", 2), list(5, "three", 6)]);
}

// ---- the third review (FEEDBACK52.md, Q-678 to Q-680; branch feedback52) ----------------------

/// LOW 5: a literal has no line, so a `return` before one on the next line lost the reason the
/// next line never runs. LOW 6: a way that leaves through a block of its own was not said, or
/// named a later `return`: the first `return` a way leaves by is now worked out as the walk
/// comes back up.
#[test]
fn a_return_before_a_literal_or_in_a_block_of_its_own_is_read_as_written() {
    let src = r#"fn total(xs) {
    return
    0
}
fn none_text() {
    return
    "none"
}
fn pair() {
    return
    [0, 0]
}
fn called(x) {
    return
    print(x)
}
fn mid_line() {
    print("a"); return; print("b")
}
fn nested_a(c) {
    let d = if c { print("x"); { return 1 } } else { { return 2 } }
    d
}
fn nested_b(c) {
    let d = if c { { return 1 } } else { return 2 }
    d
}
fn nested_m(k) {
    let d = match k { 1 => { { return 1 } }, _ => return 2 }
    d
}
fn guard_block(c) {
    let d = if c { 1 } else { print("x"); { return 2 } }
    d
}
fn stmt_first(c, a) {
    let d = if c { if a { return 1 } else { return 2 }; return 3 } else { return 4 }
    d
}
print([total([]), none_text(), pair(), called(1), mid_line(), nested_a(true), nested_b(true), nested_m(1), guard_block(true), stmt_first(true, true)])
"#;
    let bare = |at: u32, f: &str| format!("{at} warning: what follows never runs: a `return` at the end of a line gives null, so the one on line {at} leaves `{f}` before this");
    assert_eq!(
        said(src),
        [
            bare(2, "total"),
            bare(6, "none_text"),
            bare(10, "pair"),
            "15 warning: this never runs: a `return` at the end of a line gives null, so the one on line 14 leaves `called` before this".to_string(),
            "18 warning: this never runs: the `return` on line 18 always leaves `mid_line`".to_string(),
            every(21, "if", "`nested_a`", "write `1`"),
            every(25, "if", "`nested_b`", "write `1`"),
            every(29, "match", "`nested_m`", "write `1`"),
            "37 warning: this never runs: each way of the `if` on line 37 leaves `stmt_first`".to_string(),
            every(37, "if", "`stmt_first`", "write `1`"),
        ]
    );
}
