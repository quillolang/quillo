//! FEEDBACK64.md: learning Quillo on one counter, a `server fn`, a top-level `let` and `live`
//! (Q-931 to Q-935; branch feedback64).

mod support;

use quillo::check::{check_source, Severity};
use std::path::Path;

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

/// The warnings that say a call of the program's own function is dropped (Q-931).
fn dropped(src: &str) -> Vec<String> {
    diags(src).into_iter().filter(|d| d.contains("changes nothing, and what it gives is dropped")).collect()
}

// ---- Q-931: a dropped call of the program's own function that changes nothing ----

#[test]
fn a_server_fns_answer_dropped_in_a_handler_is_said() {
    let src = r#"server fn down(val: int) -> int {
    val -= 1
    return val
}

component Counter(cnt: int = 0) {
    state val = cnt
    <div>{val}
        <button onclick={() => down(val)}>Down</button>
    </div>
}

route GET / = <Counter/>
"#;
    assert_eq!(
        dropped(src),
        ["9 warning: `down` changes nothing, and what it gives is dropped here: write `val = down(val)`; a `server fn`'s arguments are copies, so the server cannot change `val`"]
    );
    // Its answer taken: nothing to say.
    assert!(dropped(&src.replace("() => down(val)", "() => val = down(val)")).is_empty());
}

#[test]
fn a_plain_fns_answer_dropped_is_said_with_the_fix() {
    let src = r#"fn minus_one(v: int) -> int = v - 1
fn twice(v: int) = minus_one(minus_one(v))
fn add(a: int, b: int) -> int = a + b
fn fact(n: int) -> int = if n <= 1 { 1 } else { n * fact(n - 1) }
fn sum_of(xs: list) -> int {
    let n = 0
    for x in xs { n += x }
    n
}

component C() {
    state val = 0
    <div>
        <button onclick={() => { minus_one(val) }}>a</button>
        <button onclick={() => { twice(val) }}>b</button>
        <button onclick={() => { add(val, 2) }}>c</button>
        <button onclick={() => { val | minus_one }}>d</button>
        <button onclick={() => { val | add(1) }}>e</button>
        <button onclick={() => { fact(5) }}>f</button>
        <button onclick={() => { sum_of([1, 2]) }}>g</button>
    </div>
}

fn work() {
    minus_one(3)
    fact(3)
    2
}

route GET / = <C/>
route GET /w = work()
"#;
    assert_eq!(
        dropped(src),
        [
            "14 warning: `minus_one` changes nothing, and what it gives is dropped here: write `val = minus_one(val)`",
            "15 warning: `twice` changes nothing, and what it gives is dropped here: write `val = twice(val)`",
            "16 warning: `add` changes nothing, and what it gives is dropped here: write `val = add(val, ...)`",
            "17 warning: `minus_one` changes nothing, and what it gives is dropped here: write `val = val | minus_one`",
            "18 warning: `add` changes nothing, and what it gives is dropped here: write `val = val | add(...)`",
            // Recursion adds nothing: still nothing changes.
            "19 warning: `fact` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
            "20 warning: `sum_of` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
            "25 warning: `minus_one` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
            "26 warning: `fact` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
        ]
    );
}

#[test]
fn a_call_that_changes_something_is_not_said() {
    let src = r#"let total = 0
let hook = (x: int) => x
server fn bump() -> int {
    total += 1
    total
}
fn calls_bump() = bump()
fn shout(x: str) {
    print(x)
    x
}
fn waits() {
    sleep(1)
    1
}
fn apply(f: fn, x: int) = f(x)
fn uses_db() -> int with db = 1
fn a(n: int) -> int {
    if n > 0 { b(n - 1) }
    total += 1
    n
}
fn b(n: int) -> int = a(n)
fn via_hook(x: int) = hook(x)
fn clear(e: map) {
    e.target.value = ""
    1
}
fn go() {
    navigate("/next")
    1
}
fn tell() {
    publish("t")
    1
}
fn fetches() = http.get("https://example.com")
fn reads_db() = db.query("select 1")
fn runs_js() = js("1")

component Box() {
    state v = 0
    fn inc() {
        v += 1
        v
    }
    <div>
        <button onclick={() => { inc() }}>a component's own fn: its state</button>
        <button onclick={() => { bump() }}>a global assigned</button>
        <button onclick={() => { calls_bump() }}>through another</button>
        <button onclick={() => { shout("hi") }}>a line in the log</button>
        <button onclick={() => { waits() }}>a wait</button>
        <button onclick={() => { apply(minus, 3) }}>a function given</button>
        <button onclick={() => { uses_db() }}>with db</button>
        <button onclick={() => { b(2) }}>a cycle reaching a global</button>
        <button onclick={() => { via_hook(1) }}>a variable called</button>
        <button onclick={(e) => { clear(e) }}>a page object's field</button>
        <button onclick={() => { go() }}>the page moved</button>
        <button onclick={() => { tell() }}>publish</button>
        <button onclick={() => { fetches() }}>the network</button>
        <button onclick={() => { reads_db() }}>the database</button>
        <button onclick={() => { runs_js() }}>code it cannot read</button>
    </div>
}

fn minus(x: int) -> int = x - 1

route GET / = <Box/>
"#;
    assert_eq!(dropped(src), Vec::<String>::new());
}

#[test]
fn b_reached_from_a_cycle_is_read_again_and_not_kept_as_changing_nothing() {
    // `a` is read first, calling `b`, which calls `a` (still being read) and nothing else: `b`
    // must not be kept as changing nothing, since `a` assigns a global.
    let src = r#"let total = 0
fn a(n: int) -> int {
    if n > 0 { b(n - 1) }
    total += 1
    n
}
fn b(n: int) -> int = a(n)

component Box() {
    <div>
        <button onclick={() => { a(1) }}>first</button>
        <button onclick={() => { b(1) }}>then</button>
    </div>
}

route GET / = <Box/>
"#;
    assert_eq!(dropped(src), Vec::<String>::new());
}

#[test]
fn the_last_statement_of_a_block_is_its_value_and_not_said() {
    let src = r#"fn minus_one(v: int) -> int = v - 1
fn f(v: int) -> int {
    let w = v + 1
    minus_one(w)
}
route GET / = f(1)
"#;
    assert_eq!(dropped(src), Vec::<String>::new());
    // With `?`, a call is run for its error: not said.
    let src = r#"fn check_age(a: int) -> int ! str {
    if a < 0 { return err("negative") }
    a
}
fn f(a: int) -> int ! str {
    check_age(a)?
    a
}
route GET / = f(1) ?? 0
"#;
    assert_eq!(dropped(src), Vec::<String>::new());
}

// ---- Q-932: `set`'s index on a list is an int, said before it runs ----

#[test]
fn set_on_a_list_given_another_index_is_refused_before_it_runs() {
    let msg = |line: u32, got: &str| format!("{} argument 2 of `set` on a list must be int, got {}: the item's place, `xs = set(xs, 0, v)`", line, got);
    let src = r#"let xs = [1, 0]
fn a() = set(xs, [0], 5)
fn b() = set([1, 0], "a", 5)
fn c() = xs | set([0], 1)
fn d() = xs.set(1.5, 2)
fn e(ys: list) = set(ys, "x", 1)
route GET / = [a(), b(), c(), d(), e([1])]
"#;
    assert_eq!(diags(src), [msg(2, "list"), msg(3, "str"), msg(4, "list"), msg(5, "float"), msg(6, "str")]);
    // What it takes: an int on a list (from the end too), any key on a map, and what may be
    // either: on null `set` makes a map (found in review).
    let src = r#"let xs = [1, 0]
let m = { a: 1 }
fn a() = set(xs, 0, 5)
fn b() = set(xs, -1, 5)
fn c() = set(m, "b", 2)
fn d() = set({ a: 1 }, 5, 2)
fn e(k: any) = set(xs, k, 1)
fn f(v: list | map) = set(v, "k", 1)
fn g(n: int) = xs | set(n, 0)
fn h(ys: list | null) = set(ys, "x", 1)
route GET / = [a(), b(), c(), d(), e(0), f({}), g(1), h(null)]
"#;
    assert_eq!(diags(src), Vec::<String>::new());
}

// ---- Q-934: an item or a field assigned is refused with what to write ----

#[test]
fn an_item_or_a_field_assigned_says_what_to_write() {
    let say = |body: &str| {
        let src = format!("let xs = [1, 0]\nlet m = {{ k: 1 }}\nfn f(i: int) {{\n    {}\n    1\n}}\nroute GET / = f(0)\n", body);
        diags(&src)
    };
    assert_eq!(say("xs[0] = 5"), ["4 a list does not change in place: `xs = set(xs, 0, 5)`"]);
    assert_eq!(say("xs[0] += 1"), ["4 a list does not change in place: `xs = set(xs, 0, xs[0] + 1)`"]);
    assert_eq!(say("xs[-1] -= 2"), ["4 a list does not change in place: `xs = set(xs, -1, xs[-1] - 2)`"]);
    // A key that is a name: on a page object, its field, which the parser cannot tell from a map.
    assert_eq!(
        say("m[\"k\"] = 5"),
        ["4 a map does not change in place: `m = { ...m, k: 5 }` or `m = set(m, \"k\", 5)`; on a page object (an element, an event), `m.k = 5`"]
    );
    assert_eq!(say("m[\"my key\"] = 5"), ["4 a map does not change in place: `m = { ...m, \"my key\": 5 }` or `m = set(m, \"my key\", 5)`"]);
    // A key or a value not known here: either may be meant, and `set` takes both.
    assert_eq!(say("m[i] = m.k"), ["4 a list or a map does not change in place: `m = set(m, i, m.k)`"]);
    assert_eq!(say("xs[0] = len(m) * 2"), ["4 a list does not change in place: `xs = set(xs, 0, ...)`"]);
    // Not a variable's: no assignment to suggest by name.
    assert_eq!(
        say("m.k[0] = 2"),
        ["4 a list or a map does not change in place: make the new one with `set`, and assign it to its variable (`xs = set(xs, 0, v)`); a page object's field is set as `el.value = x`"]
    );
    assert_eq!(say("m.k = 5"), ["4 a map does not change in place: `m = { ...m, k: 5 }` or `m = set(m, \"k\", 5)`"]);
    assert_eq!(say("m.k += 1"), ["4 a map does not change in place: `m = { ...m, k: m.k + 1 }` or `m = set(m, \"k\", m.k + 1)`"]);
    // A page object's field is set as before, and other targets are refused as before.
    let src = "component C() {\n    <input @change={(e) => { e.target.value = \"\" }}/>\n}\nroute GET / = <C/>\n";
    assert_eq!(diags(src), Vec::<String>::new());
    assert_eq!(diags("let f = () => 1 = 2\n"), ["1 only variables, and the fields of the page's objects (`el.value = x`), can be assigned to"]);
}

#[test]
fn the_written_fixes_run_and_give_what_was_meant() {
    let d = support::temp_dir("f64-fixes");
    std::fs::write(
        d.join("main.qwe"),
        r#"let xs = [1, 0]
let m = { k: 1, "my key": 5 }
fn f() {
    xs = set(xs, 0, 5)
    xs = set(xs, 0, xs[0] + 1)
    xs = set(xs, -1, 2)
    m = { ...m, k: 5 }
    m = set(m, "k", m.k + 1)
    m = { ...m, "my key": m["my key"] - 2 }
    { xs: xs, m: m }
}
route GET / = f()
"#,
    )
    .unwrap();
    let s = support::serve(&d.join("main.qwe"), &[]);
    let r = support::send(s.port, "GET", "/", &[], "");
    assert_eq!(r.body, r#"{"xs":[6,2],"m":{"k":6,"my key":3}}"#);
}

// ---- Q-933: a failed `server fn` says why under `--watch`, as a route's page does ----

#[test]
fn a_failed_server_fn_says_why_under_watch_only() {
    let d = support::temp_dir("f64-rpc");
    std::fs::write(
        d.join("main.qwe"),
        "server fn down(val: int) -> int = val / (val - val)\nroute GET /boom = 1 / (1 - 1)\nserver fn pay(val: int) = idempotent(() => val / (val - val))\n",
    )
    .unwrap();
    // With a key, `pay` is answered through `idempotent`, which says it alike.
    let call = |port: u16, key: Option<&str>| {
        let mut h = vec![("content-type", "application/json")];
        if let Some(k) = key {
            h.push(("idempotency-key", k));
        }
        let f = if key.is_some() { "pay" } else { "down" };
        let r = support::send(port, "POST", &format!("/_quillo/rpc/{}", f), &h, "[1]");
        (r.status, r.body)
    };
    let s = support::serve_with(&d.join("main.qwe"), &["--watch"]);
    let (status, body) = call(s.port, None);
    assert_eq!(status, 500, "{}", body);
    let e: serde_json::Value = serde_json::from_str(&body).unwrap();
    let said = e["error"].as_str().unwrap();
    assert!(said.contains("main.qwe:1: division by zero") && !said.contains("internal error"), "{}", said);
    // As a route's page says it.
    let page = support::send(s.port, "GET", "/boom", &[], "").body;
    assert!(page.contains("main.qwe:2: division by zero"), "{}", page);
    let (status, body) = call(s.port, Some("k-f64-1"));
    assert!(status == 500 && body.contains("main.qwe:3: division by zero"), "{} {}", status, body);
    drop(s);
    // For a visitor: that it failed, not why.
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!(call(s.port, None), (500, r#"{"error":"internal error in `down`"}"#.to_string()));
    let (status, body) = call(s.port, Some("k-f64-2"));
    assert_eq!((status, body.as_str()), (500, r#"{"error":"internal error in `pay`"}"#));
}


// ---- Q-931, found in review: calls that change something, and advice that fits ----

#[test]
fn what_review_found_changing_something_is_not_said() {
    let src = r#"let total = 0
let store = {}
let cache = {
    set: (k: str, v: any) => {
        store = set(store, k, v)
        v
    },
    get: (k: str) => store[k]
}
fn flash(ms: int, done: fn) -> int {
    run_after(ms, done)
    ms
}
fn each(xs: list, f: fn) = map(xs, f)
fn dismiss(el: any) -> int {
    el.classList.remove("open")
    1
}
fn gone(el: any) = el.remove()
fn lam(items: list, n: int) {
    items | map((total) => total * 2)
    total += n
    n
}
fn iflet(n: int) {
    if n > 0 {
        let total = 5
        total
    }
    total += n
    n
}
fn forvar(n: int) {
    for total in [1] { total }
    total += n
    n
}
fn settle() {
    page_updated()
    null
}
fn remember(k: str, v: any) {
    cache.set(k, v)
    v
}

component C() {
    state saved = false
    <div>
        <button onclick={() => { flash(2000, () => { saved = false }) }}>a</button>
        <button onclick={() => { each([1], (x) => { saved = true }) }}>b</button>
        <button onclick={(e) => { dismiss(e.target) }}>c</button>
        <button onclick={(e) => { gone(e.target) }}>d</button>
        <button onclick={() => { lam([1], 1) }}>e</button>
        <button onclick={() => { iflet(1) }}>f</button>
        <button onclick={() => { forvar(1) }}>g</button>
        <button onclick={() => { settle() }}>h</button>
        <button onclick={() => { remember("k", 1) }}>i</button>
    </div>
}

route GET / = <C/>
"#;
    let said: Vec<String> = diags(src).into_iter().filter(|d| d.contains("warning")).collect();
    assert_eq!(said, Vec::<String>::new());
}

#[test]
fn the_fix_is_an_assignment_only_where_the_answer_fits_the_variable() {
    let src = r#"fn total_of(xs: list) -> int = sum(xs)
fn bumped(n: int) -> int = n + 1
fn check_age(a: int) -> int ! str {
    if a < 0 { return err("negative") }
    a
}
component C() {
    state cart = [1, 2]
    state n = 0
    <div>
        <button onclick={() => { total_of(cart) }}>a</button>
        <button onclick={() => { bumped(n) }}>b</button>
    </div>
}
fn g(a: int) {
    check_age(a)
    1
}
route GET / = <C/>
route GET /g = g(1)
"#;
    let said: Vec<String> = diags(src).into_iter().filter(|d| d.contains("warning")).collect();
    assert_eq!(
        said,
        [
            "11 warning: `total_of` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
            "12 warning: `bumped` changes nothing, and what it gives is dropped here: write `n = bumped(n)`",
            // One of the two warnings, not both.
            "16 warning: `check_age` can give an error, and it is dropped here: write `check_age(...)?` to pass it on, or `let _ = check_age(...)` if it does not matter",
        ]
    );
}

#[test]
fn a_deep_call_graph_is_read_once_each() {
    // Two calls a level over a recursive function: read again each time, it had doubled a level
    // (11 levels, 2.9 s); kept once done, it is linear (found in review).
    let mut src = String::from("fn r(n: int) -> int = if n > 0 { r(n - 1) } else { 0 }\nfn f0(x: int) -> int = r(x) + r(x)\n");
    for i in 1..=16 {
        src.push_str(&format!("fn f{i}(x: int) -> int = f{}(x) + f{}(x)\n", i - 1, i - 1));
    }
    src.push_str("route GET / {\n    f16(1)\n    \"ok\"\n}\n");
    let t = std::time::Instant::now();
    assert_eq!(dropped(&src).len(), 1);
    // Ten times what a release build takes at most, for a debug one.
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
}

// ---- Q-934, found in review: what the advice writes ----

#[test]
fn the_advice_writes_values_and_keys_as_they_are() {
    let say = |body: &str| {
        let src = format!("let xs = [1.5]\nlet m = {{ price: 1.5 }}\nfn f() {{\n    {}\n    1\n}}\nroute GET / = f()\n", body);
        diags(&src)
    };
    // A float stays one.
    assert_eq!(say("xs[0] = 2.0"), ["4 a list does not change in place: `xs = set(xs, 0, 2.0)`"]);
    assert_eq!(say("m.price = 10.0"), ["4 a map does not change in place: `m = { ...m, price: 10.0 }` or `m = set(m, \"price\", 10.0)`"]);
    // A key quotes cannot hold as it is.
    assert_eq!(say("m[\"a\\\"b\"] = 1"), ["4 a map does not change in place: `m = set(m, key, 1)`, the key as you wrote it"]);
    // On a page object, its field.
    let src = "component C() {\n    <input @change={(el) => { el[\"value\"] = \"\" }}/>\n}\nroute GET / = <C/>\n";
    assert_eq!(
        diags(src),
        ["2 a map does not change in place: `el = { ...el, value: \"\" }` or `el = set(el, \"value\", \"\")`; on a page object (an element, an event), `el.value = \"\"`"]
    );
    let src = "component C() {\n    <input @change={(e) => { e.target.style[\"color\"] = \"red\" }}/>\n}\nroute GET / = <C/>\n";
    assert_eq!(
        diags(src),
        ["2 a list or a map does not change in place: make the new one with `set`, and assign it to its variable (`xs = set(xs, 0, v)`); a page object's field is set as `el.value = x`"]
    );
}

// ---- Q-931, found in a second review ----

#[test]
fn a_cycle_of_functions_is_read_once() {
    // Each calls the next two, round: a cycle's members had been read again on every path (16
    // functions 3.9 s, 20 over three minutes).
    let n = 24;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("fn f{i}(x: int) -> int = if x <= 0 {{ 0 }} else {{ f{}(x - 1) + f{}(x - 2) }}\n", (i + 1) % n, (i + 2) % n));
    }
    src.push_str("route GET / {\n    f0(3)\n    \"ok\"\n}\n");
    let t = std::time::Instant::now();
    assert_eq!(dropped(&src).len(), 1);
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    // A cycle in which one of them changes something: none is said, whichever is dropped.
    let src = r#"let seen = 0
fn ping(n: int) -> int = if n > 0 { pong(n - 1) } else { 0 }
fn pong(n: int) -> int {
    if n > 0 { ping(n - 1) }
    seen += 1
    n
}
route GET / {
    ping(2)
    pong(2)
    ping(1)
    "ok"
}
"#;
    assert_eq!(dropped(src), Vec::<String>::new());
}

#[test]
fn a_function_given_by_another_name_or_place_is_taken_as_called() {
    let d = support::temp_dir("f64-given");
    std::fs::write(d.join("billing.qwe"), "let charged = 0\nserver fn charge(x: int) -> int {\n    charged += x\n    x\n}\n").unwrap();
    let src = r#"service billing = "billing.qwe"
let seen = 0
let bump = (x: int) => { seen = seen + x }
fn each_bump(xs: list) -> list = xs | map(bump)
fn save_all(xs: list, store: map) -> list = map(xs, store.save)
fn charge_all(xs: list) -> list = map(xs, billing.charge)
fn both(f: fn, g: fn) -> list = parallel([f, g])
fn quiet(f: fn) = f | unwatched
fn follow() -> int {
    watch("cart")
    1
}
fn must(r: any) = unwrap(r)
fn refuse(n: int) {
    if n < 0 { return error("negative") }
    n
}
route GET / {
    must(1)
    refuse(1)
    each_bump([1])
    save_all([1], {})
    charge_all([1])
    both(() => db.exec("delete from t"), () => 1)
    quiet(() => db.exec("delete from t"))
    follow()
    "ok"
}
"#;
    let said: Vec<String> = check_source(src, "main.qwe", &d)
        .diagnostics
        .iter()
        .filter(|x| x.message.contains("changes nothing, and what it gives"))
        .map(|x| format!("{} {}", x.line, x.message))
        .collect();
    assert_eq!(said, Vec::<String>::new());
}

#[test]
fn the_advice_keeps_items_and_fields_and_what_a_component_can_assign() {
    let src = r#"let count = 0
fn labels(xs: list<int>) -> list<str> = xs | map((x) => str(x))
fn summary(u: { name: str, age: int }) -> { total: int } = { total: u.age }
fn minus_one(v: int) -> int = v - 1
component C() {
    state items = [1, 2]
    state user = { name: "a", age: 1 }
    <div>
        <button onclick={() => { labels(items) }}>a</button>
        <button onclick={() => { summary(user) }}>b</button>
        <button onclick={() => { minus_one(count) }}>c</button>
    </div>
}
route GET / = <C/>
"#;
    let generic = |line: u32, f: &str| format!("{} warning: `{}` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out", line, f);
    assert_eq!(dropped(src), [generic(9, "labels"), generic(10, "summary"), generic(11, "minus_one")]);
}
