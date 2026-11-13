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
        .filter(|d| ["this `return` leaves", "returns a value on line", "gives null when this `if`", "never runs: "].iter().any(|m| d.contains(m)))
        .collect()
}

const IF: &str = "an `if` gives its last expression";

// ---- Q-678: `return` in a block whose value is used ------------------------------------------

#[test]
fn a_return_where_the_value_of_a_for_an_if_or_a_match_is_kept_is_said() {
    // The report's three, as P1 has them.
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
print([doubled_for([1, 2, 3]), label(1), kind(0)])
"#;
    assert_eq!(
        said(src),
        [
            "2 warning: this `return` leaves `doubled_for` in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so write the value without `return`",
            "6 warning: this `return` leaves `label`, not only the `if`: an `if` gives its last expression, so write `\"one\"`",
            "11 warning: this `return` leaves `kind`, not only the `match`: an arm gives its last expression, so write `\"zero\"`",
        ]
    );
}

#[test]
fn a_return_in_an_argument_an_operand_or_a_block_kept_is_said() {
    let src = r#"fn arg(c) {
    print(if c { return 1 } else { 2 })
    3
}
fn operand(c) {
    let n = 1 + (if c { return 1 } else { 2 })
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
    let l = if c { return "ünï" } else { "ß" }
    l
}
component Box(c) {
    <div>{if c { return <p>a</p> } else { <p>b</p> }}</div>
}
"#;
    assert_eq!(
        said(src),
        [
            format!("2 warning: this `return` leaves `arg`, not only the `if`: {IF}, so write `1`"),
            format!("6 warning: this `return` leaves `operand`, not only the `if`: {IF}, so write `1`"),
            "10 warning: this `return` leaves `block`, not only the block: a block gives its last expression, so write `5`".to_string(),
            "14 warning: this `return` leaves `kept_for` in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so write `x`".to_string(),
            "17 warning: this `return` leaves `out_for` in the first round, so the `for` never makes its list: a `for` gives what its body gives each round, so write `x`".to_string(),
            // Every way leaves: the `if` never gives the value the `let` waits for. Said once.
            format!("19 warning: this `return` leaves `both_leave`, not only the `if`: {IF}, so write `\"a\"`"),
            format!("23 warning: this `return` leaves `no_value`, not only the `if`: {IF}, so write the value it should give"),
            format!("27 warning: this `return` leaves `grüße`, not only the `if`: {IF}, so write `\"ünï\"`"),
            format!("31 warning: this `return` leaves `Box`, not only the `if`: {IF}, so write the value without `return`"),
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

/// Each place a value goes, and each kind a way's value is written as: a `return` there is said
/// where another way is of its kind, and not where the kinds cannot be told (`c + d`). A type's
/// default, a service's options, a route's `= expr`, an `if true` with no line of its own.
#[test]
fn every_place_a_value_goes_and_every_kind_written_is_read() {
    let src = r#"type Item = { role: str = if true { "a" } else { "b" } }
service billing = "billing.qwe" { timeout: "2s" }
service shop = "shop.qwe"
mount /docs = "docs.qwe"
let top = if true { return 1 } else { 2 }
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
    let a = xs[if x { return 1 } else { 2 }]
    let r = (if x { return 1 } else { 2 })?
    let f = (if x { return { a: 1 } } else { { a: 2 } }).a
    let p = xs | take(if x { return 1 } else { 2 })
    while x { x = false }
    if x { reply 400 if x { return "a" } else { "b" } }
    [a, r, f, p]
}
component Page(xs) {
    state on = false
    let el = js("document.body")
    el.title = if on { return <p>a</p> } else { <p>b</p> }
    <main#top.card class={if on { return "a" } else { "b" }} {...{ hidden: on }}>
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
    let v = if request.query.x == "1" { return "one" } else { "two" }
    v
}
print([kinds(true), places(true, [1]), ends(true, true, 1), ends2(true, true), ends3(true, 1), ends4(true), ends5(true, true), lit_cond(), lit_both(), none_for([])])
"#;
    let w = |line: u32, f: &str, what: &str, fix: &str| format!("{line} warning: this `return` leaves {f}, not only {what}, so {fix}");
    let ifs = "the `if`: an `if` gives its last expression";
    let plain = "write the value without `return`";
    assert_eq!(
        said(src),
        [
            w(5, "the file", ifs, "write `1`"),
            w(7, "`kinds`", ifs, "write `true`"),
            w(8, "`kinds`", ifs, "write `-1`"),
            w(9, "`kinds`", ifs, plain),
            w(10, "`kinds`", ifs, plain),
            w(12, "`kinds`", ifs, plain),
            w(13, "`kinds`", ifs, plain),
            w(14, "`kinds`", ifs, plain),
            w(15, "`kinds`", "the `match`: an arm gives its last expression", "write `1`"),
            w(19, "`places`", ifs, "write `1`"),
            w(20, "`places`", ifs, "write `1`"),
            w(21, "`places`", ifs, plain),
            w(22, "`places`", ifs, "write `1`"),
            w(24, "`places`", ifs, "write `\"a\"`"),
            w(30, "`Page`", ifs, plain),
            w(31, "`Page`", ifs, "write `\"a\"`"),
            "37 warning: `ends` returns a value on line 36 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "41 warning: `ends2` returns a value on line 40 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "46 warning: `ends3` returns a value on line 44 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "52 warning: `ends4` returns a value on line 51 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "56 warning: `ends5` returns a value on line 55 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "58 warning: `lit_cond` returns a value on line 58 but gives null when it reaches its end: end it with a value too, or `null` if null is meant".to_string(),
            "61 warning: this never runs: each way of the `if` on line 60 leaves `lit_both`".to_string(),
            "65 warning: `none_for` returns a value on line 64 but, when it reaches its end, gives the list of nulls its `for` makes: end it with the value it should give, or `null`".to_string(),
            w(69, "the route", ifs, "write `\"one\"`"),
        ]
    );
}
