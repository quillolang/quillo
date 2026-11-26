//! FEEDBACK86.md to FEEDBACK89.md, what `quillo check` says (slice A of FEEDBACK86-89; branch
//! feedback86-89): Q-1360, a dropped `map` or `filter` whose function does nothing else; Q-1361, a
//! `state` set from another state, or a `let` that reads one, which keeps its first value;
//! Q-1370, a misspelled field of a value a record's constructor made or a fn declared `-> Lamp`
//! gives; Q-1380, a field of a state a server fn loads, given to a typed parameter with no null
//! test; Q-1381, `redirect` in a server fn a component calls; Q-1390, a field read or a method
//! called on what a `-> T ! E` fn or a service's call gives, before `?`, `??`, `is_err` or
//! `match`; Q-1393, a service two programs of the app declare, said once; FEEDBACK90.md's Q-1400,
//! `n && <markup>` on an int state a handler assigns.

mod support;

use quillo::check::{check_file, check_file_opts, check_source, Diagnostic, Options, Severity};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::temp_dir;

fn shown(d: &Diagnostic) -> String {
    let sev = match d.severity {
        Severity::Error => " error:",
        Severity::Warning => " warning:",
        Severity::Hint => " hint:",
    };
    format!("{}{} {}", d.line, sev, d.message)
}

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(shown).collect()
}

/// The diagnostics that hold `part`, each as `line warning: message`, of a program with no error.
fn about(src: &str, part: &str) -> Vec<String> {
    let all = diags(src);
    assert!(!all.iter().any(|d| d.contains(" error:")), "an error in a program meant to have none:\n{}\n--- source ---\n{}", all.join("\n"), src);
    all.into_iter().filter(|d| d.contains(part)).collect()
}

/// The errors that hold `part`.
fn errors(src: &str, part: &str) -> Vec<String> {
    diags(src).into_iter().filter(|d| d.contains(" error:") && d.contains(part)).collect()
}

/// The lines of the diagnostics that hold `part`.
fn lines(src: &str, part: &str) -> Vec<u32> {
    about(src, part).iter().map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect()
}

/// The line of code `needle` is on in `src`, not a comment's: a repro's header may change, its
/// code does not.
fn line_of(src: &str, needle: &str) -> u32 {
    src.lines().position(|l| !l.trim_start().starts_with("//") && l.contains(needle)).unwrap_or_else(|| panic!("no `{}` in the code", needle)) as u32 + 1
}

/// A folder of files for one test, gone when it ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, files: &[(&str, &str)]) -> App {
        let d = temp_dir(&format!("feedback86-89-check-{}", name));
        for (f, text) in files {
            std::fs::write(d.join(f), text).unwrap();
        }
        App(d.canonicalize().unwrap())
    }

    /// Each diagnostic with its file's name.
    fn check(&self) -> Vec<String> {
        let dir = format!("{}/", self.0.display());
        check_file(&self.0.join("main.qwe")).diagnostics.iter().map(|d| format!("{}:{}", d.file.strip_prefix(&dir).unwrap_or(&d.file), shown(d))).collect()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- Q-1360: a dropped `map` or `filter` --------------------------------------------------------

const DROPPED: &str = "gives a new value and changes nothing, and it is dropped here";

#[test]
fn q1360_the_repro_each_form_in_a_components_fn() {
    let src = std::fs::read_to_string("feedback/repros/Q-1360-dropped-map-in-a-fn.qwe").unwrap();
    let at = |f: &str| line_of(&src, &format!("fn {}()", f));
    assert_eq!(
        about(&src, DROPPED),
        [
            format!("{} warning: `map` gives a new value and changes nothing, and it is dropped here: write `xs = xs | map(...)`", at("m1")),
            format!("{} warning: `map` gives a new value and changes nothing, and it is dropped here: write `xs = xs | map(...)`", at("m2")),
            format!("{} warning: `filter` gives a new value and changes nothing, and it is dropped here: write `xs = xs | filter(...)`", at("f1")),
            format!("{} warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`", at("p1")),
        ]
    );
}

#[test]
fn q1360_an_inline_handler_a_top_level_fn_the_top_level_and_a_method() {
    let src = "let xs = [1, 2]
xs | map((x) => x + 1)
fn f(ys: list<int>) {
    ys.filter((y) => y > 1)
    len(ys)
}
component C() {
    state zs = [1]
    <button onclick={() => { zs | map((z) => z * 2) }}>go</button>
}
route GET / = <html><body><C/>{str(f(xs))}</body></html>
";
    assert_eq!(lines(src, DROPPED), [2, 4, 9]);
}

#[test]
fn q1360_a_function_by_name_the_programs_or_a_builtin() {
    let src = "fn double(x) = x * 2
fn noisy(x) {
    print(x)
    x
}
component C() {
    state xs = [1, 2]
    fn a() { xs | map(double) }
    fn b() { xs | map(str) }
    fn c() { xs | map(noisy) }
    <div><button onclick={a}>a</button><button onclick={b}>b</button><button onclick={c}>c</button></div>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(lines(src, DROPPED), [8, 9]);
}

#[test]
fn q1360_what_stays_unwarned() {
    // Run for its function (Q-505's own form), one that assigns, one that calls what acts, a
    // value taken, what a function gives back, a component's own function given (it may be any),
    // a nested lambda that prints, and a function held in a variable.
    let src = "component C() {
    state xs = [1, 2]
    state total = 0
    fn show(x) = x + 1
    fn a() { xs.map((v) => print(v)) }
    fn b() { xs | map((x) => { total += x; x }) }
    fn c() { xs | filter((x) => { navigate(\"/\"); true }) }
    fn d() { xs = xs | map((x) => x * 2) }
    fn e() { xs | map(show) }
    fn f() { xs | map((x) => [x] | map((y) => print(y))) }
    let hook = (x) => x
    fn g() { xs | map(hook) }
    <div><button onclick={a}>a</button><button onclick={b}>b</button><button onclick={c}>c</button>
        <button onclick={d}>d</button><button onclick={e}>e</button><button onclick={f}>f</button><button onclick={g}>g</button></div>
}
fn doubled(ys) = ys | map((y) => y * 2)
route GET / = <html><body><C/>{str(doubled([1]))}</body></html>
";
    assert_eq!(about(src, DROPPED), Vec::<String>::new());
}

#[test]
fn q1360_a_local_that_hides_the_programs_function_is_not_read_as_it() {
    // `twice` here is the component's own, which assigns: the program's of that name changes
    // nothing, and is not the one the lambda calls.
    let src = "fn twice(x) = x * 2
component C() {
    state n = 0
    state xs = [1]
    let twice = (x) => { n += 1; x }
    fn a() { xs | map((x) => twice(x)) }
    <button onclick={a}>{n}</button>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(about(src, DROPPED), Vec::<String>::new());
}

#[test]
fn q1360_empty_one_item_and_unicode() {
    let src = "let ñames = [\"ä\"]
ñames | map((n) => upper(n))
map([], (x) => x)
[1] | filter((x) => x > 0)
";
    // A literal on the left of `|` is no name to assign, as for `push`.
    assert_eq!(
        about(src, DROPPED),
        [
            "2 warning: `map` gives a new value and changes nothing, and it is dropped here: write `ñames = ñames | map(...)`",
            "3 warning: `map` gives a new value and changes nothing, and it is dropped here: write `x = x | map(...)`",
        ]
    );
}

#[test]
fn q1360_a_chain_and_a_field_of_a_name() {
    // Found in review: piped through more calls of the kind, or from a field, as `push` too.
    let src = "component C() {
    state xs = [1, 2]
    state m = { items: [1], deep: { list: [2] } }
    fn a() { xs | map((x) => x * 2) | filter((x) => x > 1) }
    fn b() { m.items | map((i) => i * 2) }
    fn c() { m.items | push(4) }
    fn d() { xs | sort() | push(1) }
    fn e() { m.deep.list | filter((x) => x > 0) }
    fn f() { xs | map((x) => { print(x); x }) | filter((x) => x > 1) }
    fn g() { xs = xs | map((x) => x * 2) | filter((x) => x > 1) }
    <div><button onclick={a}>a</button><button onclick={b}>b</button><button onclick={c}>c</button><button onclick={d}>d</button>
        <button onclick={e}>e</button><button onclick={f}>f</button><button onclick={g}>g</button></div>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(
        about(src, DROPPED),
        [
            "4 warning: `filter` gives a new value and changes nothing, and it is dropped here: write `xs = xs | map(...) | filter(...)`",
            "5 warning: `map` gives a new value and changes nothing, and it is dropped here: write `m = { ...m, items: m.items | map(...) }`",
            "6 warning: `push` gives a new value and changes nothing, and it is dropped here: write `m = { ...m, items: m.items | push(...) }`",
            "7 warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | sort(...) | push(...)`",
            "8 warning: `filter` gives a new value and changes nothing, and it is dropped here: use what `m.deep.list | filter(...)` gives, or leave it out",
        ]
    );
}

// ---- Q-1361: a state set from another state -----------------------------------------------------

const ONCE: &str = "is set once, as <";

#[test]
fn q1361_the_repro() {
    let src = std::fs::read_to_string("feedback/repros/Q-1361-state-initializer-reads-state.qwe").unwrap();
    assert_eq!(
        about(&src, ONCE),
        [
            format!("{} warning: `state b = ...` is set once, as <C> starts: it does not follow `a` when that changes; for a value that follows it, write `let b = ...`", line_of(&src, "state b = a * 2")),
            format!("{} warning: `state d = ...` is set once, as <C> starts: it does not follow `c` when that changes; for a value that follows it, write `let d = ...`", line_of(&src, "state d = c + 1")),
        ]
    );
}

#[test]
fn q1361_the_groceries_total() {
    let src = "component Groceries(start: list) {
    state items = start
    let open = items | filter((i) => !i.done)
    state total = open | map((i) => i.cents * i.qty) | sum()
    fn toggle(id) {
        items = items | map((i) => i.id == id ? { ...i, done: !i.done } : i)
    }
    <div><button onclick={() => { toggle(1) }}>t</button><p>{len(open)} left · {total}</p></div>
}
route GET / = <html><body><Groceries start={[]}/></body></html>
";
    assert_eq!(
        about(src, ONCE),
        ["4 warning: `state total = ...` is set once, as <Groceries> starts: it does not follow `open` when that changes; for a value that follows it, write `let total = ...`"]
    );
}

#[test]
fn q1361_what_changes_bound_live_and_loaded() {
    // A state `bind:` changes, one `live` gives, and one a server fn loads change too.
    let src = "server fn load_name() -> str = \"Ada\"
server fn names() -> list = [\"a\"]
component C() {
    state draft = \"\"
    state shout = upper(draft)
    state feed = live(names)
    state n = len(feed)
    state name = load_name()
    state first = name
    <div><input bind:value={draft}/><p>{shout} {n} {first}</p></div>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(lines(src, ONCE), [5, 7, 9]);
}

#[test]
fn q1361_what_stays_unwarned() {
    // From a prop alone; from a state nothing changes; one the component assigns itself; one a
    // server fn loads with a state's value; one the program's function makes (it may load); a
    // lambda's own `a`; and one kept as it started on purpose, worked out with what it was set
    // from (`len(rows) - start`) or what it was set from assigned from it.
    let src = "server fn search(q: str) -> list = [q]
fn shown(x) = x + 1
component C(start: int) {
    state s = start
    state a = 1
    state fixed = 0
    state b = fixed * 2
    state c = a * 2
    state found = search(str(a))
    state d = shown(a)
    state e = [1, 2] | map((a) => a * 2)
    state rows = [1]
    state at = len(rows)
    state day = 1
    state base_day = day
    <div>
        <button onclick={() => { a += 1; c += 1; rows = rows | push(2) }}>+1</button>
        <button onclick={() => { day = base_day + 1 }}>next</button>
        <p>{s} {b} {c} {found} {d} {e} {len(rows) - at} {day}</p>
    </div>
}
route GET / = <html><body><C start={7}/></body></html>
";
    assert_eq!(about(src, ONCE), Vec::<String>::new());
}

#[test]
fn q1361_one_state_only_and_unicode() {
    let one = "component C() {
    state a = 1
    let b = a * 2
    <button onclick={() => { a += 1 }}>{b}</button>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(about(one, ONCE), Vec::<String>::new());
    let unicode = "component Zähler() {
    state ä = 1
    state ö = ä + 1
    <button onclick={() => { ä += 1 }}>{ö}</button>
}
route GET / = <html><body><Zähler/></body></html>
";
    assert_eq!(
        about(unicode, ONCE),
        ["3 warning: `state ö = ...` is set once, as <Zähler> starts: it does not follow `ä` when that changes; for a value that follows it, write `let ö = ...`"]
    );
}

#[test]
fn q1361_a_label_set_from_a_state_feedback90() {
    // FEEDBACK90.md's "seen again": the details toggle's label.
    let src = "component Details() {
    state open = false
    state label = open ? \"Hide details\" : \"Show details\"
    <div><button onclick={() => { open = !open }}>{label}</button></div>
}
route GET / = <html><body><Details/></body></html>
";
    assert_eq!(
        about(src, ONCE),
        ["3 warning: `state label = ...` is set once, as <Details> starts: it does not follow `open` when that changes; for a value that follows it, write `let label = ...`"]
    );
}

#[test]
fn q1361_a_function_is_read_when_it_is_called() {
    // Found in review: `show` reads `a` each time it is called, and so does a function by name.
    let src = "fn twice(x) = x * 2
component C() {
    state a = 1
    state show = () => a * 2
    state pick = twice
    state b = a * 2
    <button onclick={() => { a += 1 }}>{show()} {pick(a)} {b}</button>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(lines(src, ONCE), [line_of(src, "state b = a * 2")]);
}

#[test]
fn q1361_what_a_view_binds_is_not_the_next_components() {
    // Found in review: a view's `bind:` before the component silenced the warning on its `b`.
    let src = "view Field(label: str) {
    let b = \"\"
    <label>{label}<input bind:value={b}/></label>
}
component C() {
    state a = 1
    state b = a * 2
    <div><Field label=\"x\"/><button onclick={() => { a += 1 }}>{b}</button></div>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(lines(src, ONCE), [line_of(src, "state b = a * 2")]);
}

/// The CPU time this thread has run, for a time that a busy machine does not stretch.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid timespec for the call to write into.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0 {
        return Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32);
    }
    panic!("no thread clock")
}

/// The review's component: `state a = 1`, then `k` states set from it, each shown with it in one
/// operation (so each is kept as it started, and none is said), or alone (each said).
fn many_states(k: usize, with_a: bool) -> String {
    let mut src = String::from("component C() {\n    state a = 1\n");
    for i in 0..k {
        src.push_str(&format!("    state s{i} = a + {i}\n"));
    }
    src.push_str("    <div><button onclick={() => { a += 1 }}>+</button>\n");
    for i in 0..k {
        if with_a {
            src.push_str(&format!("        <p>{{str(s{i} * 2 + len(str(a)) + max([1, 2, s{i}]))}}</p>\n"));
        } else {
            src.push_str(&format!("        <p>{{str(s{i} * 2 + max([1, 2, s{i}]))}}</p>\n"));
        }
    }
    src.push_str("    </div>\n}\nroute GET / = <html><body><C/></body></html>\n");
    src
}

#[test]
fn q1361_many_states_are_read_in_linear_time() {
    // Found in review: each warned state walked the component again, and each operation in it its
    // own part again: 3,200 took 2.2 s, 0.04 s on 0.8.11, and a confined check went past its time.
    // Twice the states take about twice as long, not four times; timed by the CPU time of the
    // least of three checks, the ratio tells the two apart on any machine.
    let d = temp_dir("feedback86-89-check-linear");
    let k = if cfg!(debug_assertions) { 400 } else { 1600 };
    for (with_a, confined) in [(true, false), (true, true), (false, false)] {
        let check = |src: &str| {
            std::fs::write(d.join("main.qwe"), src).unwrap();
            let opts = Options { confined, time: Some(Duration::from_secs(600)), ..Default::default() };
            let mut least = Duration::MAX;
            let mut said = 0;
            for _ in 0..3 {
                let t = thread_cpu();
                let a = check_file_opts(&d.join("main.qwe"), HashMap::new(), opts.clone());
                least = least.min(thread_cpu() - t);
                said = a.diagnostics.iter().filter(|x| x.message.contains(ONCE)).count();
            }
            (least, said)
        };
        let (small, said_small) = check(&many_states(k, with_a));
        let (big, said_big) = check(&many_states(2 * k, with_a));
        assert_eq!((said_small, said_big), if with_a { (0, 0) } else { (k, 2 * k) }, "with_a {}", with_a);
        assert!(big < small * 3 + Duration::from_millis(20), "with_a {} confined {}: {:?} for {} states, {:?} for {}", with_a, confined, small, k, big, 2 * k);
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- Q-1370: a record's value, made or returned -------------------------------------------------

#[test]
fn q1370_the_repro() {
    let src = std::fs::read_to_string("feedback/repros/Q-1370-record-made-value-field-typo.qwe").unwrap();
    assert_eq!(
        errors(&src, "no field"),
        [
            format!("{} error: this map has no field `wats` (did you mean `watts`?)", line_of(&src, "${desk.wats}")),
            format!("{} error: this map has no field `wats` (did you mean `watts`?)", line_of(&src, "${make().wats}")),
        ]
    );
}

#[test]
fn q1370_in_a_function_and_the_fields_it_has() {
    let src = "type Lamp = { name: str, watts: int = 40, note: str? }
fn label() {
    let l = Lamp({ name: \"Desk\" })
    `${l.name} ${l.watts} ${l.note} ${l.nmae}`
}
fn failed() -> str {
    let l = Lamp({ name: \"Desk\" })
    is_err(l) ? str(l.message) + str(l.error) : l.name
}
route GET / = label() + failed()
";
    assert_eq!(errors(src, ""), ["4 error: this map has no field `nmae` (did you mean `name`?)"]);
}

#[test]
fn q1370_a_map_or_an_error_of_the_programs_own_and_money() {
    // The same rule on any map that may be an error, whose fields are known: one a function of
    // the program's makes, and money.
    let src = "fn inv(n: str) = if n == \"\" { err(\"none\") } else { { id: 1, ñame: n } }
route GET /a = `${inv(\"a\").ñame} ${inv(\"a\").idd}`
route GET /b = `${money.of(\"1.50\", \"EUR\").amont}`
";
    assert_eq!(
        errors(src, ""),
        ["2 error: this map has no field `idd` (did you mean `id`?)", "3 error: this map has no field `amont` (did you mean `amount`?)"]
    );
}

#[test]
fn q1370_a_services_record_is_its_own_not_the_callers_of_that_name() {
    // Found in review: main's `Invoice` typed billing's result, and `.total`, billing's, was an
    // error that stopped `quillo run`; `?` too, as 0.8.11 already refused.
    let app = App::new(
        "q1370-service-records",
        &[
            (
                "main.qwe",
                "type Invoice = { id: int }
service billing = \"billing.qwe\"
route GET /a = str(billing.invoice(\"y\").total)
route GET /b = str(billing.invoice(\"y\")?.total)
route GET /c = str(billing.invoice(\"y\").totl)
route GET /d = str(billing.take({ id: 1 }))
route GET /e = str(Invoice({ id: 1 }).total)
",
            ),
            ("billing.qwe", "type Invoice = { id: int, total: int }\nserver fn invoice(name: str) -> Invoice = Invoice({ id: 1, total: 1200 })\nserver fn take(i: Invoice) -> int = i.total\n"),
        ],
    );
    let errors: Vec<String> = app.check().into_iter().filter(|d| d.contains(" error:")).collect();
    assert_eq!(
        errors,
        [
            "main.qwe:5 error: this map has no field `totl` (did you mean `total`?)",
            "main.qwe:6 error: argument `i` of `billing.take` has no field `total`, which { id: int, total: int } needs",
            "main.qwe:7 error: this map has no field `total` (it has id)",
        ]
    );
}

#[test]
fn q1370_a_service_two_programs_declare_keeps_its_records() {
    // Checked once, from `a.qwe`: its types are kept for main's calls too.
    let app = App::new(
        "q1370-service-records-again",
        &[
            ("main.qwe", "type Invoice = { ïd: int }\nservice a = \"a.qwe\"\nservice billing = \"billing.qwe\"\nroute GET / = str(billing.invoice(\"y\")?.total)\n"),
            ("a.qwe", "service billing = \"billing.qwe\"\nserver fn one() -> int = 1\n"),
            ("billing.qwe", "type Invoice = { id: int, total: int }\nserver fn invoice(name: str) -> Invoice = Invoice({ id: 1, total: 1200 })\n"),
        ],
    );
    let errors: Vec<String> = app.check().into_iter().filter(|d| d.contains(" error:")).collect();
    assert_eq!(errors, Vec::<String>::new());
}

// ---- Q-1390: a field read past an error ---------------------------------------------------------

const PAST: &str = "as if it could not";

#[test]
fn q1390_the_repro() {
    let src = std::fs::read_to_string("feedback/repros/Q-1390-error-result-field-read-without-question-mark.qwe").unwrap();
    assert_eq!(
        about(&src, PAST),
        [format!("{} warning: `inv` may hold the error `invoice(...)` gives, and `.id` is read from it as if it could not: when it is one, this fails and the error's own message is lost. Write `let inv = invoice(...)?` to pass it on, or `?? ...` after the call to give a value in its place, or test `is_err(inv)` first", line_of(&src, "Invoice #{inv.id}"))]
    );
}

const INVOICE: &str = "fn invoice(name: str) -> map ! str = if name == \"\" { err(\"An invoice is for someone\") } else { { id: 1, name } }\n";

#[test]
fn q1390_a_call_read_straight_and_a_method() {
    let src = format!(
        "{INVOICE}route GET /a = `${{invoice(\"\").id}}`
route GET /b {{
    let inv = invoice(\"x\")
    str(inv.keys())
}}
"
    );
    assert_eq!(
        about(&src, PAST),
        [
            "2 warning: `invoice(...)` may give an error, and `.id` is read from it as if it could not: when it is one, this fails and the error's own message is lost. Add `?` after the call to pass it on, or `?? ...` to give a value in its place",
            "5 warning: `inv` may hold the error `invoice(...)` gives, and `.keys()` is called on it as if it could not: when it is one, this fails and the error's own message is lost. Write `let inv = invoice(...)?` to pass it on, or `?? ...` after the call to give a value in its place, or test `is_err(inv)` first",
        ]
    );
}

#[test]
fn q1390_what_takes_the_error_out() {
    let src = format!(
        "{INVOICE}fn a() -> int ! str {{
    let inv = invoice(\"x\")?
    inv.id
}}
fn b() = (invoice(\"\") ?? {{ id: 0 }}).id
fn c() {{
    let inv = invoice(\"\")
    if is_err(inv) {{ return 0 }}
    inv.id
}}
fn d() {{
    let inv = invoice(\"\")
    is_err(inv) ? inv.message : inv.id
}}
fn e() {{
    let inv = invoice(\"\")
    match inv {{
        {{ id }} => id
        _ => inv.id
    }}
}}
fn f() {{
    let inv = invoice(\"\")
    str(inv.error) + str(inv.message)
}}
route GET / = str([a(), b(), c(), d(), e(), f()])
"
    );
    assert_eq!(about(&src, PAST), Vec::<String>::new());
}

#[test]
fn q1390_a_function_that_tests_it_in_the_condition() {
    // app/forum's: `show(r)` tests `is_err(r)` itself; given whole to a call in the condition, the
    // value is taken as tested where it holds, and past an `if` that leaves. A field read in the
    // condition is no test of it.
    let src = format!(
        "{INVOICE}fn shown(r) = !is_err(r)
fn a() {{
    let r = invoice(\"\")
    if shown(r) {{ r.id }} else {{ 0 }}
}}
fn b() {{
    let r = invoice(\"\")
    shown(r) && r.id > 0
}}
fn c() {{
    let r = invoice(\"\")
    if !shown(r) {{ return 0 }}
    r.id
}}
fn d() {{
    let r = invoice(\"\")
    if r.id > 0 {{ 1 }} else {{ 0 }}
}}
route GET / = str([a(), b(), c(), d()])
"
    );
    assert_eq!(lines(&src, PAST), [18]);
}

#[test]
fn q1390_on_the_left_of_a_fallback() {
    // Found in review: `??` takes a read that fails, as the guide's `v.foo ?? "none"`; on its
    // right, a read is no fallback's.
    let src = format!(
        "{INVOICE}server fn lädt(id: int) -> map ! str = id == 0 ? err(\"none\") : {{ name: \"Lantern\" }}
fn a() {{
    let inv = invoice(\"\")
    print(inv.id ?? -1)
    print(str(inv.keys() ?? []))
    print(inv.name.len ?? 0)
    0 ?? inv.id
}}
component C() {{
    state t = lädt(1)
    <p>{{t.name ?? \"unnamed\"}}</p>
}}
route GET / = <html><body><C/>{{str(a())}}</body></html>
"
    );
    assert_eq!(lines(&src, PAST), [line_of(&src, "0 ?? inv.id")]);
}

#[test]
fn q1390_only_a_test_that_tells_an_error_apart_and_where_it_holds() {
    // Found in review: any call in a condition counted, in both branches.
    let src = format!(
        "{INVOICE}fn shown(r) = !is_err(r)
fn named(r: map) = r.name != \"\"
fn a() {{
    let r = invoice(\"\")
    if len(r) > 0 {{ 1 }} else {{ r.id }}
}}
fn b() {{
    let r = invoice(\"\")
    if str(r) == \"\" {{ return 0 }}
    r.id
}}
fn c() {{
    let r = invoice(\"\")
    if shown(r) {{ 0 }} else {{ r.id }}
}}
fn d() {{
    let r = invoice(\"\")
    if named(r) {{ r.id }} else {{ 0 }}
}}
fn e() {{
    let r = invoice(\"\")
    if type_of(r) == \"map\" {{ r.id }} else {{ 0 }}
}}
fn f() {{
    let r = invoice(\"\")
    if type_of(r) == \"error\" {{ return 0 }}
    r.id
}}
fn g() {{
    let r = invoice(\"\")
    if type_of(r) == \"error\" {{ r.id }} else {{ 0 }}
}}
route GET / = str([a(), b(), c(), d(), e(), f(), g()])
"
    );
    assert_eq!(lines(&src, PAST), [6, 11, 15, 19, 32]);
}

#[test]
fn q1390_a_test_held_by_a_let() {
    // Found in review: `let ok = !is_err(inv)`, then `if ok`, is a test of `inv`; `if !ok` is not.
    let src = format!(
        "{INVOICE}fn a() {{
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    if ok {{ inv.id }} else {{ 0 }}
}}
fn b() {{
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    if !ok {{ inv.id }} else {{ 0 }}
}}
fn c() {{
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    if !ok {{ return 0 }}
    inv.id
}}
route GET / = str([a(), b(), c()])
"
    );
    assert_eq!(lines(&src, PAST), [line_of(&src, "if !ok { inv.id }")]);
}

#[test]
fn q1390_only_what_its_signature_says_may_fail() {
    // A function whose type has an error from its body but declares none, and a builtin's error:
    // not this warning (decided: a fn declared `! E`, or a service's call).
    let src = "fn lookup(n: str) = if n == \"\" { err(\"none\") } else { { id: 1 } }
route GET / {
    let a = lookup(\"\")
    let n = int(\"7\")
    `${a.id} ${n}`
}
";
    assert_eq!(about(src, PAST), Vec::<String>::new());
}

#[test]
fn q1390_a_state_and_unicode() {
    let src = "server fn lädt(id: int) -> map ! str = id == 0 ? err(\"none\") : { name: \"Lantern\" }
component C() {
    state t = lädt(1)
    <p>{t.name}</p>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(
        about(src, PAST),
        ["4 warning: `t` may hold the error `lädt(...)` gives, and `.name` is read from it as if it could not: when it is one, this fails and the error's own message is lost. Write `let t = lädt(...)?` to pass it on, or `?? ...` after the call to give a value in its place, or test `is_err(t)` first"]
    );
}

#[test]
fn q1390_a_services_call() {
    let app = App::new(
        "q1390-service",
        &[
            (
                "main.qwe",
                "service billing = \"billing.qwe\"
route GET /a {
    let inv = billing.invoice(\"x\", 1200)
    `Invoice #${inv.id}`
}
route GET /b = str(billing.summary().n)
route GET /c = str((billing.summary() ?? { n: 0 }).n)
",
            ),
            ("billing.qwe", "server fn invoice(name: str, cents: int) -> map ! str = name == \"\" ? err(\"An invoice is for someone\") : { id: 1, cents }\nserver fn summary() -> map = { n: 1 }\n"),
        ],
    );
    let said: Vec<String> = app.check().into_iter().filter(|d| d.contains(PAST)).collect();
    assert_eq!(
        said,
        [
            "main.qwe:4 warning: `inv` may hold the error `billing.invoice(...)` gives (a call to another service can always fail), and `.id` is read from it as if it could not: when it is one, this fails and the error's own message is lost. Write `let inv = billing.invoice(...)?` to pass it on, or `?? ...` after the call to give a value in its place, or test `is_err(inv)` first",
            "main.qwe:6 warning: `billing.summary(...)` may give an error (a call to another service can always fail), and `.n` is read from it as if it could not: when it is one, this fails and the error's own message is lost. Add `?` after the call to pass it on, or `?? ...` to give a value in its place",
        ]
    );
}

// ---- Q-1393: a service two programs declare -----------------------------------------------------

#[test]
fn q1393_the_repro_says_each_once() {
    let app = App::new(
        "q1393",
        &[
            ("main.qwe", "service billing = \"billing.qwe\"\nservice ledger = \"ledger.qwe\"\nroute GET / = \"ok\"\n"),
            ("billing.qwe", "service ledger = \"ledger.qwe\"\nserver fn count() -> int = 2\n"),
            ("ledger.qwe", "server fn count() -> int = 1\n"),
        ],
    );
    let hides: Vec<String> = app.check().into_iter().filter(|d| d.contains("hides the builtin `count`")).map(|d| d.split(" warning").next().unwrap().to_string()).collect();
    assert_eq!(hides, ["billing.qwe:2", "ledger.qwe:1"]);
}

#[test]
fn q1393_three_programs_one_service_and_a_second_check() {
    let app = App::new(
        "q1393-three",
        &[
            ("main.qwe", "service a = \"a.qwe\"\nservice b = \"b.qwe\"\nservice ledger = \"lédger.qwe\"\nroute GET / = \"ok\"\n"),
            ("a.qwe", "service ledger = \"lédger.qwe\"\nserver fn one() -> int = 1\n"),
            ("b.qwe", "service ledger = \"lédger.qwe\"\nserver fn two() -> int = 2\n"),
            ("lédger.qwe", "server fn count() -> int = 1\n"),
        ],
    );
    let hides = |all: Vec<String>| all.into_iter().filter(|d| d.contains("hides the builtin `count`")).count();
    assert_eq!(hides(app.check()), 1);
    // The next check on the thread says it again: once a run, not once a thread.
    assert_eq!(hides(app.check()), 1);
}

// ---- Q-1380: a loading state's field given to a typed parameter ----------------------------------

const LOADING: &str = "is null until";

const MAYBE: &str = "-> map? = find(trails, (t) => t.id == id)";

fn trail(page: &str, server: &str, view: &str) -> String {
    format!(
        "let trails = [{{ id: 5, name: \"Lantern Ridge\", level: \"hard\" }}]
server fn load_trail(id: int) {server}
view Level({view}) = <span>{{level}}</span>
view Whole(t: map) = <b>{{t.name}}</b>
component TrailPage(id: int) {{
    state trail = load_trail(id)
{page}
}}
component App() {{
    <div>{{url().path == \"/\" ? <a href=\"/t\">Open</a> : <TrailPage id={{5}}/>}}</div>
}}
route GET /*rest = <html><body><App/></body></html>
"
    )
}

#[test]
fn q1380_the_repro() {
    let src = std::fs::read_to_string("feedback/repros/Q-1380-null-state-render-stays-blank.qwe").unwrap();
    assert_eq!(
        about(&src, LOADING),
        [format!("{} warning: `trail.level` is null until `load_trail` answers when the browser makes <TrailPage> (<App> shows it), and `level` of <Level> must be str: the draw fails then. Show something else until it comes, `trail == null ? <p>Loading…</p> : ...`", line_of(&src, "<Level level={trail.level}/>"))]
    );
}

#[test]
fn q1380_a_server_fn_declared_not_null_and_the_whole_state() {
    // Null at first in the browser whatever the server fn declares.
    let src = trail("    <main><Level level={trail.level}/><Whole t={trail}/></main>", "-> map = { id, name: \"Lantern Ridge\", level: \"hard\" }", "level: str");
    assert_eq!(lines(&src, LOADING), [7, 7]);
    assert_eq!(about(&src, LOADING)[1].contains("`trail` is null until `load_trail` answers"), true);
}

#[test]
fn q1380_what_tests_for_null() {
    for page in [
        "    <main>{trail == null ? <p>Loading</p> : <Level level={trail.level}/>}</main>",
        "    <main>{trail != null && <Level level={trail.level}/>}</main>",
        "    <main>{trail ? <Level level={trail.level}/> : null}</main>",
        "    if trail == null { return <p>Loading</p> }\n    <main><Level level={trail.level}/></main>",
        "    <main>{match trail { null => <p>Loading</p>, _ => <Level level={trail.level}/> }}</main>",
    ] {
        let src = trail(page, MAYBE, "level: str");
        assert_eq!(about(&src, LOADING), Vec::<String>::new(), "{}", page);
    }
}

#[test]
fn q1380_what_takes_null_or_is_no_server_fn() {
    // A parameter that takes null, one with a default, one with no type; and a state no server fn
    // loads.
    for view in ["level: str?", "level: str = \"easy\"", "level"] {
        let src = trail("    <main><Level level={trail.level}/></main>", MAYBE, view);
        assert_eq!(about(&src, LOADING), Vec::<String>::new(), "{}", view);
    }
    let plain = "fn load(id: int) -> map? = { level: \"hard\" }
view Level(level: str) = <span>{level}</span>
component P(id: int) {
    state t = load(id)
    <main><Level level={t.level}/></main>
}
component App() {
    <div><P id={1}/></div>
}
route GET / = <html><body><App/></body></html>
";
    assert_eq!(about(plain, LOADING), Vec::<String>::new());
}

#[test]
fn q1380_a_component_only_a_routes_page_holds() {
    // Made by the server, which loads its states first (the Single-Page Apps course's lessons 1
    // and 2): no question; held by another component as well, it is.
    let src = "server fn list_trails() -> list = [{ id: 1 }]
view TrailList(trails: list) = <ul>{for t in trails => <li>{t.id}</li>}</ul>
component App() {
    state trails = list_trails()
    <div><TrailList trails={trails}/></div>
}
route GET / = <html><body><App/></body></html>
";
    assert_eq!(about(src, LOADING), Vec::<String>::new());
    let nested = format!("{}component Shell() {{\n    <div><App/></div>\n}}\nroute GET /s = <html><body><Shell/></body></html>\n", src);
    assert_eq!(lines(&nested, LOADING), [5]);
    assert!(about(&nested, LOADING)[0].contains("when the browser makes <App> (<Shell> shows it)"));
}

#[test]
fn q1380_made_by_a_view_or_a_function_a_component_uses() {
    // Found in review: <TrailPage> made by a view, or a function, that <App> uses is made in the
    // browser too; one only a route's page holds is not.
    let base = "server fn load_trail(id: int) -> map? = { level: \"hard\" }
view Level(level: str) = <span>{level}</span>
component TrailPage(id: int) {
    state trail = load_trail(id)
    <main><Level level={trail.level}/></main>
}
";
    let shell = format!("{base}view Shell(on: bool) = <div>{{on ? <TrailPage id={{5}}/> : <p>none</p>}}</div>
component App() {{
    state on = false
    <div><button onclick={{() => {{ on = !on }}}}>go</button><Shell on={{on}}/></div>
}}
route GET / = <html><body><App/></body></html>
");
    assert_eq!(lines(&shell, LOADING), [5]);
    assert!(about(&shell, LOADING)[0].contains("when the browser makes <TrailPage> (<Shell> shows it)"));
    let func = format!("{base}fn page(on: bool) = on ? <TrailPage id={{5}}/> : null
component App() {{
    state on = false
    <div><button onclick={{() => {{ on = !on }}}}>go</button>{{page(on)}}</div>
}}
route GET / = <html><body><App/></body></html>
");
    assert!(about(&func, LOADING)[0].contains("(<page> shows it)"), "{:?}", about(&func, LOADING));
    let route_only = format!("{base}view Shell(on: bool) = <div>{{on ? <TrailPage id={{5}}/> : <p>none</p>}}</div>
route GET / = <html><body><Shell on={{true}}/></body></html>
");
    assert_eq!(about(&route_only, LOADING), Vec::<String>::new());
}

#[test]
fn q1380_a_null_test_held_by_a_let() {
    // Found in review: `let ready = trail != null`, then `{ready ? ... : ...}`, tests `trail`.
    let page = "    let ready = trail != null\n    <main>{ready ? <Level level={trail.level}/> : <p>Loading</p>}</main>";
    assert_eq!(about(&trail(page, MAYBE, "level: str"), LOADING), Vec::<String>::new());
    let wrong = "    let ready = trail != null\n    <main>{ready ? <p>Loading</p> : <Level level={trail.level}/>}</main>";
    assert_eq!(lines(&trail(wrong, MAYBE, "level: str"), LOADING), [8]);
}

#[test]
fn q1380_unicode_and_a_field_of_a_field() {
    let src = "server fn wëg(id: int) -> map? = { info: { level: \"hard\" } }
view Level(level: str) = <span>{level}</span>
component P(id: int) {
    state wäy = wëg(id)
    <main><Level level={wäy.info.level}/></main>
}
component Ä() {
    <div><P id={1}/></div>
}
route GET / = <html><body><Ä/></body></html>
";
    assert_eq!(lines(src, LOADING), [5]);
    let said = &about(src, LOADING)[0];
    assert!(said.starts_with("5 warning: `wäy.info.level` is null until `wëg` answers when the browser makes <P> (<Ä> shows it)"), "{}", said);
}

// ---- Q-1381: `redirect` in a server fn a component calls ------------------------------------------

const REDIRECT: &str = "a server fn cannot redirect";

#[test]
fn q1381_the_repro() {
    let src = std::fs::read_to_string("feedback/repros/Q-1381-redirect-in-server-fn.qwe").unwrap();
    assert_eq!(
        about(&src, REDIRECT),
        [format!("{} warning: a server fn cannot redirect: <Form> calls `log_hike`, and its answer goes to the page's code, not to the browser, so the page stays where it is and the call gives an error. Return a value (the new address, an id) and `navigate(...)` to it in <Form>", line_of(&src, "redirect(\"/hikes/7\")"))]
    );
}

#[test]
fn q1381_each_redirect_once_and_what_is_no_question() {
    // Two redirects in one, a `reply 303` among them, called from two components and a handler's
    // function; one only a route calls; one with no redirect; and the program's own `redirect`.
    let src = "server fn save(n: str) {
    if n == \"\" { reply 303 redirect(\"/new\") }
    redirect(\"/saved\")
}
server fn from_route() = redirect(\"/x\")
server fn fine(n: str) -> str = n
component A() {
    state r = \"\"
    fn go() { r = str(save(\"a\")) + fine(\"x\") }
    <button onclick={go}>{r}</button>
}
component B() {
    <button onclick={() => { save(\"b\") }}>b</button>
}
route GET /a = <html><body><A/><B/></body></html>
route POST /r = from_route()
";
    assert_eq!(lines(src, REDIRECT), [2, 3]);
    assert!(about(src, REDIRECT)[0].contains("<A> calls `save`"));
    let own = "fn redirect(to: str) = to
server fn save() = redirect(\"/x\")
component A() {
    <button onclick={() => { save() }}>ünï</button>
}
route GET / = <html><body><A/></body></html>
";
    assert_eq!(about(own, REDIRECT), Vec::<String>::new());
}

// ---- FEEDBACK90.md, Q-1400: `n && <markup>` on an int state a handler assigns --------------------

const ZERO: &str = "counts as true even when it is";

#[test]
fn q1400_the_repro() {
    let src = std::fs::read_to_string("feedback/repros/Q-1400-zero-and-badge-after-assignment.qwe").unwrap();
    assert_eq!(
        about(&src, ZERO),
        [format!("{} warning: `unread` counts as true even when it is 0 (unlike JavaScript), so `&&` always goes on: write `unread != 0 && ...`", line_of(&src, "Inbox{unread &&"))]
    );
}

#[test]
fn q1400_each_way_a_state_is_tested_and_what_it_is_given() {
    // `!`, `if`, `? :`, `||` and `&&`, on states set by `+=`, `=` and to null; one given what is
    // not known says nothing, nor one of text or null (an error's message, as the starter app's),
    // as one never assigned says what it said.
    let src = "fn load() = json.parse(\"1\")
component C() {
    state n = 0
    state m = 0
    state z = 0
    state w = 0
    state ünread = 0
    state fixed = 0
    state problem = null
    <div>
        <button onclick={() => { n += 1; m = 2; z = null; w = load(); ünread -= 1; problem = \"failed\" }}>go</button>
        <p>{!n ? \"none\" : \"some\"}</p>
        <p>{m ? \"m\" : \"no m\"}</p>
        <p>{z || \"zero\"}</p>
        <p>{w && <b>w</b>}</p>
        <p>{ünread && <b>{ünread}</b>}</p>
        <p>{fixed && <b>f</b>}</p>
        <p>{problem && <b>{problem}</b>}</p>
    </div>
}
route GET / = <html><body><C/></body></html>
";
    assert_eq!(
        about(src, ZERO),
        [
            "12 warning: `n` counts as true even when it is 0 (unlike JavaScript), so `!n` is false: write `n == 0`",
            "13 warning: `m` counts as true even when it is 0 (unlike JavaScript), so this condition holds: write `m != 0`",
            "14 warning: `z` counts as true even when it is 0 (unlike JavaScript), so `||` keeps it unless it is null: write `z != null && z != 0 ? z : ...`, or `z ?? ...` to replace only null",
            "16 warning: `ünread` counts as true even when it is 0 (unlike JavaScript), so `&&` always goes on: write `ünread != 0 && ...`",
            "17 warning: `fixed` counts as true even when it is 0 (unlike JavaScript), so `&&` always goes on: write `fixed != 0 && ...`",
        ]
    );
}

// ---- FEEDBACK90.md, Q-1401: a body made again whole runs its assignments again -----------------

const RUNS_ONCE: &str = "runs once, as <";

#[test]
fn q1401_a_body_made_again_whole_is_not_said_to_run_once() {
    // A `return`, a `?`, a `let` or a prop assigned on a line of the body: the browser makes the
    // body again whole when what it read changes, and the assignment to the state follows (as
    // src/client.rs `runs_again_whole` decides, the same function); a plain body still runs it once.
    let src = "component R(gone: bool) {
    state open = false
    state label = \"Show\"
    if gone { return <p>gone</p> }
    label = open ? \"Hide\" : \"Show\"
    <button onclick={() => { open = !open }}>{label}</button>
}
component Q(text: str) {
    state open = false
    state label = \"Show\"
    let n = int(text)?
    label = open ? \"Hide\" : \"Show\"
    <button onclick={() => { open = !open }}>{label} {n}</button>
}
component L() {
    state open = false
    state label = \"Show\"
    let note = \"\"
    note = open ? \"open\" : \"shut\"
    label = open ? \"Hide\" : \"Show\"
    <button onclick={() => { open = !open }}>{label} {note}</button>
}
component P(a: int, b: int) {
    b = a * 2
    <p>{b}</p>
}
component Plain() {
    state open = false
    state label = \"Show\"
    label = open ? \"Hide\" : \"Show\"
    <button onclick={() => { open = !open }}>{label}</button>
}
route GET / = <html><body><R gone={false}/><Q text=\"5\"/><L/><P a={1} b={0}/><Plain/></body></html>
";
    let plain = src.lines().position(|l| l.starts_with("component Plain")).unwrap() as u32 + 4;
    assert_eq!(lines(src, RUNS_ONCE), [plain]);
}

// ---- FEEDBACK89.md, Q-1392: what `idempotent` reaches outside its transaction --------------------

const REACHES: &str = "`idempotent` reaches";

/// The warning for a request to another site, slice D's words, with the function named.
fn reaches_site(f: &str) -> String {
    format!("warning: `idempotent` reaches `{}`, which may change data at another site, outside its transaction: when its answer is lost, the key is given up and a retry with it sends the request again. Send the site a key of its own that is the same on each try (its `Idempotency-Key` header, made from this request's key); where it takes none, look up whether the first request's change is there before sending it again. Or give `idempotent` `{{ transaction: false }}`, which keeps every answer, failures too: a kept failure replays until the key expires, so its page should offer a fresh form", f)
}

#[test]
fn q1392_a_quillo_service_chain_is_not_said() {
    // The repro: billing's `idempotent` calls `ledger.post`, whose call id is made of the run's key
    // and its place in it, so a retry answers from the ledger's record.
    let said: Vec<String> = check_file(Path::new("feedback/repros/Q-1392-chain/main.qwe")).diagnostics.iter().map(shown).filter(|d| d.contains(REACHES)).collect();
    assert_eq!(said, Vec::<String>::new());
}

#[test]
fn q1392_a_request_that_may_change_another_site() {
    let src = "let u = \"https://example.com/x\"
route POST /post = idempotent(() => str(http.post(u, { a: 1 })))
route POST /put = idempotent(() => str(http.put(u, { a: 1 })))
route POST /patch = idempotent(() => str(http.patch(u, { a: 1 })))
route POST /delete = idempotent(() => str(http.delete(u)))
route POST /request = idempotent(() => str(http.request(\"post\", u, { a: 1 })))
route POST /unknown = idempotent(() => str(http.request(request.form.how ?? \"PUT\", u)))
route POST /get = idempotent(() => str(http.get(u)))
route POST /head = idempotent(() => str(http.request(\"HEAD\", u)))
route POST /keyed = idempotent(() => str(http.post(u, { a: 1 }, { \"Idempotency-Key\": \"k-1\" })))
route POST /keyed2 = idempotent(() => str(http.request(\"DELETE\", u, null, { \"idempotency-key\": \"k-2\" })))
route POST /kept = idempotent({ transaction: false }, () => str(http.post(u, { a: 1 })))
route POST /ünï = idempotent(() => str(http.patch(u, { ä: 1 })))
";
    let said: Vec<String> = diags(src).into_iter().filter(|d| d.contains(REACHES)).collect();
    let at = |line: u32, f: &str| format!("{} {}", line, reaches_site(f));
    assert_eq!(said, [at(2, "http.post"), at(3, "http.put"), at(4, "http.patch"), at(5, "http.delete"), at(6, "http.request"), at(7, "http.request"), at(13, "http.patch")]);
    // Two: the first named, the other counted.
    let two = "let u = \"https://example.com/x\"\nroute POST /two = idempotent(() => str([http.post(u, 1), http.put(u, 2)]))\n";
    let said: Vec<String> = diags(two).into_iter().filter(|d| d.contains(REACHES)).collect();
    assert_eq!(said.len(), 1);
    assert!(said[0].starts_with("2 warning: `idempotent` reaches `http.post`, which may change data at another site, outside its transaction (and 1 more such call): when its answer is lost"), "{}", said[0]);
}

// ---- Round 3 (found in review) ------------------------------------------------------------------

/// A route whose `inv` may be an error, `k` `let`s each of `ops` of the one before and a test, and
/// `uses` tests of the last.
fn alias_chain(k: usize, ops: usize, uses: usize) -> String {
    let mut src = String::from("fn load_it() -> int ! str = 1\nroute GET / {\n    let inv = load_it()\n    let a1 = !is_err(inv)\n");
    for i in 2..=k {
        let prev = format!("a{}", i - 1);
        let parts: Vec<String> = vec![prev; ops];
        src.push_str(&format!("    let a{} = {} && !is_err(inv)\n", i, parts.join(" && ")));
    }
    for _ in 0..uses {
        src.push_str(&format!("    if a{} {{ print(str(inv)) }}\n", k));
    }
    src.push_str("    \"ok\"\n}\n");
    src
}

#[test]
fn q1390_a_test_held_by_lets_of_lets_is_read_once_each_in_linear_time() {
    // Found in review: each `let` read again for each place it is named, 4^k times: 725 bytes took
    // 4.1 s and 580 MB. Twice the `let`s take about twice as long, not far more; timed by the CPU
    // time of the least of three checks.
    let d = temp_dir("feedback86-89-check-aliases");
    let k = if cfg!(debug_assertions) { 300 } else { 1500 };
    let check = |src: &str| {
        std::fs::write(d.join("main.qwe"), src).unwrap();
        let mut least = Duration::MAX;
        let mut said = Vec::new();
        for _ in 0..3 {
            let t = thread_cpu();
            let a = check_file_opts(&d.join("main.qwe"), HashMap::new(), Options::default());
            least = least.min(thread_cpu() - t);
            said = a.diagnostics.iter().map(|x| x.message.clone()).collect::<Vec<_>>();
        }
        (least, said)
    };
    let (small, said_small) = check(&alias_chain(k, 4, 2));
    let (big, said_big) = check(&alias_chain(2 * k, 4, 2));
    assert_eq!((said_small.clone(), said_big), (Vec::<String>::new(), Vec::<String>::new()), "{:?}", said_small);
    assert!(big < small * 3 + Duration::from_millis(20), "{:?} for {} lets, {:?} for {}", small, k, big, 2 * k);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q1390_a_confined_check_of_many_tests_through_lets_ends_at_its_time() {
    // The review's 725 bytes, `let`s of four `let`s twelve deep (4.1 s and 580 MB, past a confined
    // check's 2 s): now read in a few steps, it ends with nothing to say. And a program whose check
    // goes past a short budget, with many such tests, ends with the one error well within ten times
    // its time.
    let d = temp_dir("feedback86-89-check-aliases-confined");
    let slow = if cfg!(debug_assertions) { 10 } else { 1 };
    let review = "type Inv = { id: int }\nfn load() -> Inv ! str = Inv({ id: 1 })\nroute GET / {\n    let inv = load()\n    let a1 = !is_err(inv)\n"
        .to_string()
        + &(2..=12).map(|i| format!("    let a{0} = a{1} && a{1} && a{1} && a{1} && !is_err(inv)\n", i, i - 1)).collect::<String>()
        + "    if a12 { print(inv.id) }\n    \"ok\"\n}\n";
    std::fs::write(d.join("main.qwe"), &review).unwrap();
    let t = std::time::Instant::now();
    let a = check_file_opts(&d.join("main.qwe"), HashMap::new(), Options { confined: true, ..Default::default() });
    assert_eq!(a.diagnostics.iter().map(|x| x.message.clone()).collect::<Vec<_>>(), Vec::<String>::new());
    assert!(t.elapsed() < Duration::from_secs(2) * slow, "{:?}", t.elapsed());
    let k = if cfg!(debug_assertions) { 4000 } else { 20000 };
    std::fs::write(d.join("main.qwe"), alias_chain(k, 4, k)).unwrap();
    let budget = Duration::from_millis(300);
    let t = std::time::Instant::now();
    let a = check_file_opts(&d.join("main.qwe"), HashMap::new(), Options { confined: true, time: Some(budget), ..Default::default() });
    let took = t.elapsed();
    let said: Vec<String> = a.diagnostics.iter().map(|x| x.message.clone()).collect();
    assert_eq!(said, ["the code is too complex to check: its types grew past what a check keeps, or the check took too long"]);
    assert!(took < budget * 10 * slow, "{:?}", took);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn q1390_a_function_called_on_it_that_takes_it_whole() {
    // Found in review: `inv.is_err()` calls `is_err(inv)`, `inv.str()` and `inv.type_of()` take it
    // whole, and `inv.is_err()` or `inv.type_of() == "map"` is a test of it; `inv.keys()` is not.
    let src = format!(
        "{INVOICE}fn a() {{
    let inv = invoice(\"\")
    if inv.is_err() {{ return \"bad\" }}
    return str(inv.id)
}}
fn b() {{
    let inv = invoice(\"\")
    print(inv.str())
    print(inv.type_of())
    if inv.type_of() == \"map\" {{ print(inv.id) }}
    return str(inv.keys())
}}
route GET / = a() + b()
"
    );
    assert_eq!(lines(&src, PAST), [line_of(&src, "return str(inv.keys())")]);
}

#[test]
fn q1390_a_test_of_its_error_or_a_while_that_leaves() {
    // Found in review: `inv.error != null`, `error(inv) != null` and `inv.error` tell an error apart
    // as `is_err` does, and so does a `while` that leaves.
    let src = format!(
        "{INVOICE}fn a() {{
    let inv = invoice(\"\")
    if inv.error != null {{ return 0 }}
    return inv.id
}}
fn b() {{
    let inv = invoice(\"\")
    if error(inv) != null {{ return 0 }}
    return inv.id
}}
fn c() {{
    let inv = invoice(\"\")
    if inv.error {{ return 0 }}
    return inv.id
}}
fn d() {{
    let inv = invoice(\"\")
    while is_err(inv) {{ return 0 }}
    return inv.id
}}
fn e() {{
    let inv = invoice(\"\")
    if inv.error == null {{ return 0 }}
    return inv.id
}}
route GET / = str([a(), b(), c(), d(), e()])
"
    );
    assert_eq!(lines(&src, PAST), [line_of(&src, "if inv.error == null") + 1]);
}

#[test]
fn q1400_an_or_on_a_text_state_keeps_its_warning() {
    // Found in review: 0.8.11's FEEDBACK19.md Q-151 warning, `{name || "anonymous"}` on a text state
    // something assigns, from what its fallback says; a number state's from its own type.
    let src = "component A() {
    state name = \"\"
    state nick = null
    state total = 0
    <div>
        <p>{name || \"anonymous\"}</p>
        <p>{nick || \"no nick\"}</p>
        <p>{total || \"none\"}</p>
        <button onclick={() => { name = \"Ada\"; nick = \"a\"; total = total + 1 }}>set</button>
        <button onclick={() => { name = \"\" }}>clear</button>
    </div>
}
route GET / = <html><body><A/></body></html>
";
    assert_eq!(
        about(src, ZERO),
        [
            "6 warning: `name` counts as true even when it is \"\" (unlike JavaScript), so `||` keeps it unless it is null: write `filled(name) ?? ...`, or `name ?? ...` to replace only null",
            "7 warning: `nick` counts as true even when it is \"\" (unlike JavaScript), so `||` keeps it unless it is null: write `filled(nick) ?? ...`, or `nick ?? ...` to replace only null",
            "8 warning: `total` counts as true even when it is 0 (unlike JavaScript), so `||` keeps it: write `total != 0 ? total : ...`",
        ]
    );
}

#[test]
fn q1392_a_method_or_headers_held_by_a_let() {
    // Found in review: the method and the headers read through a `let`, the program's or the
    // function's; headers not known (a call's, a map spread in) may carry the key.
    let src = "let GET_M = \"GET\"
let KEYED = { \"Idempotency-Key\": \"k\" }
fn key_of(k: str) = { \"Idempotency-Key\": k }
route POST /a = idempotent(() => str(http.request(GET_M, \"https://a.example/x\")))
route POST /b {
    let m = \"get\"
    return idempotent(() => str(http.request(m, \"https://a.example/x\")))
}
route POST /c {
    let h = { \"Idempotency-Key\": \"abc\", authorization: \"Bearer t\" }
    return idempotent(() => str(http.post(\"https://a.example/pay\", { n: 1 }, h)))
}
route POST /d = idempotent(() => str(http.post(\"https://a.example/pay\", { n: 1 }, key_of(\"k\"))))
route POST /e = idempotent(() => str(http.post(\"https://a.example/pay\", { n: 1 }, { ...key_of(\"k\") })))
route POST /f = idempotent(() => str(http.put(\"https://a.example/pay\", { n: 1 }, KEYED)))
route POST /g {
    let plain = { authorization: \"Bearer t\" }
    return idempotent(() => str(http.post(\"https://a.example/pay\", { n: 1 }, plain)))
}
route POST /h {
    let way = \"DELETE\"
    return idempotent(() => str(http.request(way, \"https://a.example/x\")))
}
";
    let said: Vec<u32> = diags(src).into_iter().filter(|d| d.contains(REACHES)).map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect();
    assert_eq!(said, [line_of(src, "route POST /g") + 2, line_of(src, "route POST /h") + 2]);
    // One name given two values in two routes: each read by its own binding (round 4), so the GET
    // is not said and the POST is.
    let two = "route POST /a {\n    let m = \"get\"\n    return idempotent(() => str(http.request(m, \"https://a.example/x\")))\n}\nroute POST /b {\n    let m = \"POST\"\n    return idempotent(() => str(http.request(m, \"https://a.example/x\")))\n}\n";
    let said: Vec<u32> = diags(two).into_iter().filter(|d| d.contains(REACHES)).map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect();
    assert_eq!(said, [7]);
}

#[test]
fn q1393_a_service_that_calls_back_reads_its_callers_types() {
    // Found in review: `b` was checked while `a`, which it calls, was still being checked, with no
    // types of `a`'s, and its misspelled field went unsaid.
    let app = App::new(
        "q1393-cycle",
        &[
            ("main.qwe", "service a = \"a.qwe\"\nservice b = \"b.qwe\"\nroute GET / = str(a.make(1)) + str(b.x())\n"),
            ("a.qwe", "type Invoice = { id: int, total: int }\nservice b = \"b.qwe\"\nserver fn make(n: int) -> Invoice = Invoice({ id: n, total: 1 })\nserver fn y() -> int = b.x() ?? 0\n"),
            ("b.qwe", "service a = \"a.qwe\"\nserver fn x() -> int {\n    let inv = a.make(1)?\n    return inv.nope\n}\n"),
        ],
    );
    let errors: Vec<String> = app.check().into_iter().filter(|d| d.contains(" error:")).collect();
    assert_eq!(errors, ["b.qwe:4 error: this map has no field `nope` (it has id, total)", "b.qwe:4 error: `x` must return int, but this returns null"]);
}

// ---- Round 4 (found in review): each walk linear, bindings read where they are bound ------------

/// Asserts that checking `make(2 * k)` takes less than three times as long as `make(k)` (plus 20 ms),
/// by the thread's CPU time, the least of three; what grows with the square takes four times.
fn grows_linearly(name: &str, k: usize, make: &dyn Fn(usize) -> String) {
    let d = temp_dir(&format!("feedback86-89-check-lin-{}", name));
    let check = |src: &str| {
        std::fs::write(d.join("main.qwe"), src).unwrap();
        let mut least = Duration::MAX;
        for _ in 0..3 {
            let t = thread_cpu();
            check_file_opts(&d.join("main.qwe"), HashMap::new(), Options::default());
            least = least.min(thread_cpu() - t);
        }
        least
    };
    let (small, big) = (check(&make(k)), check(&make(2 * k)));
    let _ = std::fs::remove_dir_all(&d);
    assert!(big < small * 3 + Duration::from_millis(20), "{}: {:?} at {}, {:?} at {}", name, small, k, big, 2 * k);
}

/// Asserts that a confined check of `src`, given 300 ms, ends well within ten times that (a debug
/// build ten times more), with nothing said but, past its time, the one error.
fn confined_ends(name: &str, src: &str) {
    let d = temp_dir(&format!("feedback86-89-check-conf-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    let budget = Duration::from_millis(300);
    let t = std::time::Instant::now();
    let a = check_file_opts(&d.join("main.qwe"), HashMap::new(), Options { confined: true, time: Some(budget), ..Default::default() });
    let took = t.elapsed();
    let _ = std::fs::remove_dir_all(&d);
    let errors: Vec<String> = a.diagnostics.iter().filter(|x| x.severity == Severity::Error).map(|x| x.message.clone()).collect();
    assert!(errors.is_empty() || errors == ["the code is too complex to check: its types grew past what a check keeps, or the check took too long"], "{}: {:?}", name, errors);
    assert!(took < budget * 10 * if cfg!(debug_assertions) { 10 } else { 1 }, "{}: {:?}", name, took);
}

const LOADED: &str = "type Inv = { id: int }\nfn load() -> Inv ! str = Inv({ id: 1 })\n";

/// `let`s that rebind the names they read, `uses` tests of the first.
fn rebinding(uses: usize) -> String {
    let mut src = format!("{LOADED}route GET / {{\n    let inv = load()\n    let v1 = true\n    let v2 = true\n    let v3 = true\n");
    for v in ["v1", "v2", "v3"] {
        src.push_str(&format!("    let {v} = v1 && v2 && v3 && !is_err(inv)\n"));
    }
    for _ in 0..uses {
        src.push_str("    if v1 { print(inv.id) }\n");
    }
    src + "    \"ok\"\n}\n"
}

/// A big test held by a `let`, named by `k` others.
fn big_test_named(k: usize) -> String {
    let ones = vec!["1"; 2000].join(",");
    let mut src = format!("{LOADED}route GET / {{\n    let inv = load()\n    let a1 = len([{ones}]) > 0 && !is_err(inv)\n");
    for i in 2..=k {
        src.push_str(&format!("    let a{i} = a1 && !is_err(inv)\n"));
    }
    src + &format!("    if a{k} {{ print(inv.id) }}\n    \"ok\"\n}}\n")
}

/// `k` requests in one function, its headers a parameter.
fn many_requests(k: usize) -> String {
    let calls = vec!["http.request(m, u, 1, h)"; k].join(",\n        ");
    format!("fn big(u: str, m: str, h: map) {{\n    let all = [\n        {calls}\n    ]\n    return len(all)\n}}\nroute POST / = idempotent(() => str(big(\"https://a.example/x\", \"POST\", {{}})))\n")
}

/// `k` assignments `total = total + run(...)`, each in the function the one before gives.
fn nested_totals(k: usize) -> String {
    let mut inner = String::from("1");
    for _ in 0..k {
        inner = format!("run(() => {{ total = total + {inner} }})");
    }
    format!("fn run(g) = 1\ncomponent A() {{\n    state total = 0\n    <div><p>{{total}}</p><button onclick={{() => {{ total = total + {inner} }}}}>+</button></div>\n}}\nroute GET / = <html><body><A/></body></html>\n")
}

#[test]
fn round4_each_walk_grows_linearly() {
    // Found in review: a `let` that rebinds a name read itself again (shi3_1: 3.5 s, 323 bytes); a
    // big test was copied into each `let` that named it (fc3: 3.3 s, 3.18 GB); each request's
    // headers were looked for through the whole function (lit12000: 5.8 s); each nested assignment
    // typed what it adds again (nest24: 23.7 s).
    let (a, b, c, d) = if cfg!(debug_assertions) { (200, 200, 500, 6) } else { (1000, 1000, 3000, 9) };
    grows_linearly("rebinding", a, &rebinding);
    grows_linearly("big-test", b, &big_test_named);
    grows_linearly("requests", c, &many_requests);
    grows_linearly("nested", d, &nested_totals);
}

#[test]
fn round4_each_ends_within_a_confined_checks_time() {
    let big = if cfg!(debug_assertions) { 1 } else { 4 };
    confined_ends("rebinding", &rebinding(4000 * big));
    confined_ends("big-test", &big_test_named(2000 * big));
    confined_ends("requests", &many_requests(6000 * big));
    confined_ends("nested", &nested_totals(24));
}

#[test]
fn round4_a_test_is_read_where_its_names_mean_what_they_meant() {
    // Found in review: `ok` tested the `inv` before it, not the one that rebinds it; and a rebinding
    // `let` read through itself said a variable was unused.
    let src = format!(
        "{LOADED}route GET /a {{
    let inv = load()
    let ok = !is_err(inv)
    let inv = load()
    if ok {{ return str(inv.id) }}
    return \"no\"
}}
route GET /b {{
    let first = load()
    let ok = !is_err(first)
    for n in [1, 2] {{
        let first = load()
        if ok {{ print(first.id) }}
    }}
    return \"no\"
}}
route GET /c {{
    let ok = true
    for n in [1, 2] {{
        let inv = load()
        let ok = ok && !is_err(inv)
        if ok {{ print(inv.id) }}
    }}
    return \"no\"
}}
"
    );
    let all = diags(&src);
    let past: Vec<u32> = all.iter().filter(|d| d.contains(PAST)).map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect();
    assert_eq!(past, [line_of(&src, "if ok { return str(inv.id) }"), line_of(&src, "if ok { print(first.id) }")], "{:?}", all);
    assert!(!all.iter().any(|d| d.contains("not used")), "{:?}", all);
}

#[test]
fn round4_a_test_at_the_top_level_in_a_match_and_against_false() {
    let src = format!(
        "{LOADED}let inv = load()
let ok = !is_err(inv)
fn a() {{
    if ok {{ return inv.id }}
    return 0
}}
route GET /b {{
    let inv = load()
    let fine = !is_err(inv)
    match fine {{
        true => print(inv.id)
        _ => print(\"bad\")
    }}
    if is_err(inv) == false {{ print(inv.id) }}
    if is_err(inv) != true {{ print(inv.id) }}
    if is_err(inv) == true {{ print(inv.id) }}
    return str(a())
}}
"
    );
    assert_eq!(lines(&src, PAST), [line_of(&src, "if is_err(inv) == true")]);
}

#[test]
fn round4_an_error_test_is_never_dropped_by_the_cap() {
    let many: Vec<String> = (0..32).map(|i| format!("n > {}", i)).collect();
    let ors: Vec<String> = (0..32).map(|i| format!("n == {}", i)).collect();
    let src = format!(
        "{LOADED}route GET /a {{
    let n = 5
    let inv = load()
    let ok = {} && !is_err(inv)
    if ok {{ return str(inv.id) }}
    return \"no\"
}}
route GET /c {{
    let n = 5
    let inv = load()
    let bad = {} || is_err(inv)
    if bad {{ return \"no\" }}
    return str(inv.id)
}}
",
        many.join(" && "),
        ors.join(" || ")
    );
    assert_eq!(about(&src, PAST), Vec::<String>::new());
}

#[test]
fn round4_a_request_reads_the_binding_its_names_have() {
    // Found in review: a parameter `headers` or `method` was read as another route's `let` of that
    // name, or a top-level one. A parameter is not known: a method not known is said, headers not
    // known are not.
    let src = "let method = \"GET\"
fn send(url: str, body: map, headers: map) = http.post(url, body, headers)
fn call(method: str, url: str) = http.request(method, url)
fn send2(url: str, headers: map) {
    if url == \"\" {
        let headers = {}
        print(headers)
    }
    return http.post(url, { n: 1 }, headers)
}
route POST /a = idempotent(() => str(send(\"https://a.example/x\", { n: 1 }, { \"Idempotency-Key\": \"k\" })))
route POST /b = idempotent(() => str(call(\"POST\", \"https://a.example/x\")))
route POST /c = idempotent(() => str(send2(\"https://a.example/x\", { \"Idempotency-Key\": \"k\" })))
route GET /d {
    let headers = { accept: \"text/plain\" }
    return str(http.get(\"https://a.example/x\", headers))
}
";
    let said: Vec<u32> = diags(src).into_iter().filter(|d| d.contains(REACHES)).map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect();
    assert_eq!(said, [line_of(src, "route POST /b")]);
}

#[test]
fn round4_a_use_after_the_service_line_in_a_cycle() {
    // Found in review: `a` takes its `Invoice` with a `use` written after `service b`; `b` reads `a`
    // while `a` is still being checked, and missed `z.amount`.
    let app = App::new(
        "q1393-cycle-use",
        &[
            ("main.qwe", "service a = \"a.qwe\"\nservice b = \"b.qwe\"\nroute GET / = `${a.make(1)} ${b.bad(1)}`\n"),
            ("a.qwe", "service b = \"b.qwe\"\nuse types.qwe\nserver fn make(n: int) -> Invoice = Invoice({ id: n, total: 1 })\nserver fn total_of(n: int) -> int = b.bad(n) ?? 0\n"),
            ("types.qwe", "public type Invoice = { id: int, total: int }\n"),
            ("b.qwe", "service a = \"a.qwe\"\nserver fn bad(n: int) -> int {\n    let z = a.make(n)?\n    return z.amount\n}\n"),
        ],
    );
    let errors: Vec<String> = app.check().into_iter().filter(|d| d.contains("no field")).collect();
    assert_eq!(errors, ["b.qwe:4 error: this map has no field `amount` (it has id, total)"]);
}

// ---- Round 5 (found in review) ------------------------------------------------------------------

#[test]
fn round5_a_binding_is_assigned_only_by_its_own_code() {
    // Found in review: `ok` assigned in another function, or another file, turned this one's off;
    // `m` assigned elsewhere made a GET a change.
    let app = App::new(
        "round5-assigned",
        &[
            ("main.qwe", "import other.qwe use b\nfn invoice(name: str) -> map ! str = if name == \"\" { err(\"none\") } else { { id: 1, name } }\nfn a() {\n    let inv = invoice(\"\")\n    let ok = !is_err(inv)\n    if ok { return inv.id }\n    return 0\n}\nfn c() {\n    let ok = false\n    ok = true\n    let m = \"POST\"\n    m = \"PUT\"\n    return ok\n}\nroute POST / {\n    let m = \"GET\"\n    return idempotent(() => str([a(), b(), c()]) + str(http.request(m, \"https://a.example/y\")))\n}\n"),
            ("other.qwe", "public fn b() {\n    let ok = false\n    for x in [1, 2] { ok = x > 1 }\n    return ok\n}\n"),
        ],
    );
    let said: Vec<String> = app.check().into_iter().filter(|d| d.contains(PAST) || d.contains(REACHES)).collect();
    assert_eq!(said, Vec::<String>::new());
    // Its own code assigning it still turns it off.
    let own = format!("{INVOICE}fn a() {{\n    let inv = invoice(\"\")\n    let ok = !is_err(inv)\n    ok = true\n    if ok {{ return inv.id }}\n    return 0\n}}\nroute GET / = str(a())\n");
    assert_eq!(lines(&own, PAST), [6]);
}

/// A test with a long part, held by `ok`, named by `n` `let`s.
fn wide_test_named(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
    let mut src = format!("{INVOICE}fn f() {{\n    let inv = invoice(\"\")\n    let ok = !is_err(inv) && inv.name == str([{}])\n", items.join(", "));
    for i in 0..n {
        src.push_str(&format!("    let b{i} = ok && inv.id > {i}\n"));
    }
    src + &format!("    if b{} {{ return inv.id }}\n    return 0\n}}\nroute GET / = str(f())\n", n - 1)
}

#[test]
fn round5_whether_a_test_tells_an_error_apart_is_worked_out_once() {
    // Found in review: each `let` naming `ok` walked its long test again (hq_32000: 11 s).
    let k = if cfg!(debug_assertions) { 1000 } else { 4000 };
    grows_linearly("wide-test", k, &wide_test_named);
    confined_ends("wide-test", &wide_test_named(4 * k));
    assert_eq!(about(&wide_test_named(50), PAST), Vec::<String>::new());
}

#[test]
fn round5_a_test_of_many_names() {
    // Found in review: `fine(inv, a, ..., h)` read nine names and was left out.
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let lets: String = names.iter().map(|n| format!("    let {n} = 1\n")).collect();
    let src = format!(
        "{INVOICE}fn fine(r, {}) = !is_err(r)\nfn held() {{\n    let inv = invoice(\"\")\n{lets}    let ok = fine(inv, {})\n    if ok {{ return inv.id }}\n    return 0\n}}\nroute GET / = str(held())\n",
        names.join(", "),
        names.join(", ")
    );
    assert_eq!(about(&src, PAST), Vec::<String>::new());
}

#[test]
fn round5_a_lambda_held_by_a_let() {
    // Found in review: an `http.request` reached through `let f = (m) => ...` was not followed.
    let src = "route POST /g {
    let f = (m) => http.request(m, \"https://a.example/g\")
    return idempotent(() => str(f(\"POST\")))
}
route POST /h {
    let get = () => http.get(\"https://a.example/h\")
    return idempotent(() => str(get()))
}
";
    let said: Vec<u32> = diags(src).into_iter().filter(|d| d.contains(REACHES)).map(|d| d.split(' ').next().unwrap().parse().unwrap()).collect();
    assert_eq!(said, [3]);
}

// ---- Round 6 (found in review): each assignment changes the binding it reaches ------------------

#[test]
fn round6_a_name_another_function_or_component_assigns() {
    // Found in review: `inv`, `r` or `trail` assigned in another function or component, a binding of
    // its own there, turned this one's warning off.
    let src = format!("{INVOICE}fn a() {{\n    let inv = invoice(\"\")\n    return inv.id\n}}\nfn b() {{\n    let inv = 1\n    inv = 2\n    return inv\n}}\nroute GET / = str([a(), b()])\n");
    assert_eq!(lines(&src, PAST), [4]);
    let app = App::new(
        "round6-service",
        &[
            ("main.qwe", "service billing = \"billing.qwe\"\nfn a() {\n    let r = billing.make(\"\")\n    return r.id\n}\nfn b() {\n    let r = 1\n    r = 2\n    return r\n}\nroute GET / = str([a(), b()])\n"),
            ("billing.qwe", "server fn make(name: str) -> map = { id: 1, name }\n"),
        ],
    );
    let said: Vec<String> = app.check().into_iter().filter(|d| d.contains(PAST)).map(|d| d.split(" warning").next().unwrap().to_string()).collect();
    assert_eq!(said, ["main.qwe:4"]);
    let page = "    <main><Level level={trail.level}/></main>\n}\ncomponent Other() {\n    state trail = 1\n    <button onclick={() => trail = 2}>x</button>";
    let src = trail(page, MAYBE, "level: str").replace("<TrailPage id={5}/>", "<div><TrailPage id={5}/><Other/></div>");
    assert_eq!(lines(&src, LOADING), [line_of(&src, "<Level level")]);
}

#[test]
fn round6_top_level_codes_own_bindings() {
    // Found in review: in a lambda a top-level `let` holds, or a top-level `for`, a binding assigned
    // there was read as never assigned.
    let held = format!("{INVOICE}let h = () => {{\n    let inv = invoice(\"\")\n    let ok = !is_err(inv)\n    ok = true\n    if ok {{ return inv.id }}\n    return 0\n}}\nroute GET / = str(h())\n");
    assert_eq!(lines(&held, PAST), [6]);
    let looped = format!("{INVOICE}for x in [1] {{\n    let inv = invoice(\"\")\n    let ok = !is_err(inv)\n    ok = true\n    if ok {{ print(inv.id) }}\n}}\nroute GET / = \"x\"\n");
    assert_eq!(lines(&looped, PAST), [6]);
    let method = "let handler = () => {
    let m = \"GET\"
    m = \"POST\"
    return idempotent(() => str(http.request(m, \"https://a.example/y\")))
}
route POST / = handler()
route POST /b {
    let m = \"GET\"
    m = \"POST\"
    return idempotent(() => str(http.request(m, \"https://a.example/y\")))
}
";
    assert_eq!(lines(method, REACHES), [4, 10]);
}

#[test]
fn round6_a_lambdas_parameter_or_another_blocks_let_of_the_name() {
    // Found in review: a nested lambda's parameter `ok`, or a `let ok` of another block, assigned,
    // turned the test `ok` holds off.
    let src = format!(
        "{INVOICE}fn a() {{
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    let flip = (ok) => {{ ok = !ok
        return ok }}
    if ok {{ return inv.id + flip(true) }}
    return 0
}}
fn b(xs) {{
    for x in xs {{ let ok = false
        ok = true }}
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    if ok {{ return inv.id }}
    return 0
}}
route GET / = str([a(), b([1])])
"
    );
    assert_eq!(about(&src, PAST), Vec::<String>::new());
    let page = "    let ready = trail != null\n    let flip = (ready) => { ready = !ready\n        return ready }\n    <main>{ready ? <Level level={trail.level}/> : <p>Loading {flip(true)}</p>}</main>\n}\ncomponent Other() {\n    let ready = false\n    <button onclick={() => ready = true}>x</button>";
    let src = trail(page, MAYBE, "level: str").replace("<TrailPage id={5}/>", "<div><TrailPage id={5}/><Other/></div>");
    assert_eq!(about(&src, LOADING), Vec::<String>::new());
}

#[test]
fn round6_a_local_lambda_is_followed_by_its_own_binding() {
    // Found in review: a route's `send` that hides the program's was read as the program's; a
    // lambda's own `f` was looked up where `idempotent` is; `idempotent(send)` and `let g = send`
    // with a local `send` were not followed.
    let shadow = "let send = () => http.post(\"https://a.example/a\", { a: 1 })
route POST /a = idempotent(send)
route POST /b = idempotent(() => str(send()))
fn wrap() = send()
route POST /c = idempotent(() => str(wrap()))
route POST /d {
    let send = () => http.get(\"https://a.example/d\")
    return idempotent(() => str(send()))
}
route POST /e {
    let f = (m) => http.request(m, \"https://a.example/e\")
    let g = (m) => f(m)
    return idempotent(() => str(g(\"POST\")))
}
route POST /f {
    let f = () => http.post(\"https://a.example/f\", 1)
    let g = () => {
        let f = () => 1
        return f()
    }
    return idempotent(() => str(g()))
}
";
    assert_eq!(lines(shadow, REACHES), [2, 3, 5, 13]);
    // `helper(send)`, a lambda given to a function, is not followed: a known limit.
    let local = "route POST /a {
    let send = () => http.post(\"https://a.example/a\", { a: 1 })
    return idempotent(send)
}
route POST /b {
    let send = () => http.post(\"https://a.example/b\", { a: 1 })
    let g = send
    return idempotent(() => str(g()))
}
route POST /c {
    let send = () => http.get(\"https://a.example/c\")
    send = () => http.post(\"https://a.example/c\", 1)
    return idempotent(() => str(send()))
}
route POST /d {
    let inner = () => http.post(\"https://a.example/d\", 1)
    let outer = () => inner()
    return idempotent(() => str(outer()))
}
fn helper(cb) = cb()
route POST /e {
    let send = () => http.post(\"https://a.example/e\", 1)
    return idempotent(() => str(helper(send)))
}
";
    assert_eq!(lines(local, REACHES), [3, 8, 18]);
}

/// `k` functions, each with a binding assigned in a block, a lambda and a loop, and one of another
/// name tested; all of the same names.
fn many_bindings(k: usize) -> String {
    let mut src = INVOICE.to_string();
    for i in 0..k {
        src.push_str(&format!(
            "fn f{i}(xs) {{\n    let inv = invoice(\"\")\n    let ok = !is_err(inv)\n    let n = 0\n    for x in xs {{ n += x }}\n    let bump = (ok) => {{ ok = !ok\n        return ok }}\n    if ok {{ return inv.id + n }}\n    return 0\n}}\n"
        ));
    }
    src + "route GET / = \"x\"\n"
}

#[test]
fn round6_bindings_are_resolved_in_linear_time() {
    let k = if cfg!(debug_assertions) { 400 } else { 1600 };
    grows_linearly("many-bindings", k, &many_bindings);
    confined_ends("many-bindings", &many_bindings(4 * k));
    assert_eq!(about(&many_bindings(3), PAST), Vec::<String>::new());
}

// ---- Round 7 (found in review) ------------------------------------------------------------------

#[test]
fn round7_the_programs_binding_is_assigned_only_where_no_local_takes_it() {
    // Found in review: a function's own `inv = 2` or `ok = true` counted against the top level's
    // `inv` and `ok`.
    let kept = format!("{INVOICE}let inv = invoice(\"\")\nroute GET / = str(inv.id)\nfn other() {{\n    let inv = 1\n    inv = 2\n    return inv\n}}\nroute GET /o = str(other())\n");
    assert_eq!(lines(&kept, PAST), [3]);
    let held = format!("{INVOICE}let inv = invoice(\"\")\nlet ok = !is_err(inv)\nroute GET / = ok ? str(inv.id) : \"none\"\nfn other() {{\n    let ok = false\n    ok = true\n    return ok\n}}\nroute GET /o = str(other())\n");
    assert_eq!(about(&held, PAST), Vec::<String>::new());
    // The program's own, assigned by a function, still is.
    let set = format!("{INVOICE}let inv = invoice(\"\")\nlet ok = !is_err(inv)\nroute GET / = ok ? str(inv.id) : \"none\"\nfn other() {{\n    ok = true\n    return ok\n}}\nroute GET /o = str(other())\n");
    assert_eq!(lines(&set, PAST), [4]);
}

#[test]
fn round7_a_state_set_once_by_its_own_binding() {
    // Found in review: a lambda's parameter `a`, or a helper's `let a`, made `state c = a * 2` said.
    let src = "component C() {
    state a = 1
    state c = a * 2
    <div>
        <button onclick={() => { [1] | map((a) => { a += 1
            return a }) }}>x</button>
        <p>{c}</p>
    </div>
}
component D() {
    state a = 1
    state c = a * 2
    fn bump() {
        let a = 5
        a += 1
    }
    <div><button onclick={() => { bump() }}>x</button><p>{c}</p></div>
}
component E() {
    state a = 1
    state c = a * 2
    <div><button onclick={() => { a += 1 }}>x</button><p>{c}</p></div>
}
route GET / = <html><body><C/><D/><E/></body></html>
";
    assert_eq!(lines(src, ONCE), [21]);
}

#[test]
fn round7_a_local_fn_is_followed_by_its_binding() {
    // Found in review: a local `fn go` was not followed, and the program's `send` was read for a
    // local `fn send`.
    let src = "fn send() = 1
route POST /a {
    fn go() { return http.post(\"https://a.example/a\", 1) }
    return idempotent(() => str(go()))
}
route POST /b {
    fn go() { return http.post(\"https://a.example/b\", 1) }
    return idempotent(go)
}
route POST /c {
    fn send() { return http.post(\"https://a.example/c\", 1) }
    return idempotent(() => str(send()))
}
";
    assert_eq!(lines(src, REACHES), [4, 8, 12]);
    let own = "fn send(n) = http.post(\"https://a.example/x\", n)
route POST /b {
    fn send(n) { return n <= 0 ? 0 : send(n - 1) }
    return idempotent(() => str(send(3)))
}
route POST /d {
    fn go(n) { return n <= 0 ? http.post(\"https://a.example/d\", 1) : go(n - 1) }
    return idempotent(() => str(go(3)))
}
";
    assert_eq!(lines(own, REACHES), [8]);
}

#[test]
fn round7_a_let_after_code_assigns_the_one_before_is_its_own() {
    // Found in review: `ok = true` before `let ok = !is_err(inv)`, or `inv = ...` before `let inv`,
    // turned the second's test or warning off. A closure's assignment of the first still reaches
    // the second, which it may run after.
    let src = format!(
        "{INVOICE}fn a() {{
    let ok = false
    ok = true
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    if ok {{ return inv.id }}
    return 0
}}
fn b() {{
    let inv = {{ id: 0 }}
    inv = {{ id: 2 }}
    let inv = invoice(\"\")
    return inv.id
}}
fn c() {{
    let ok = false
    let set = () => {{ ok = true }}
    let inv = invoice(\"\")
    let ok = !is_err(inv)
    set()
    if ok {{ return inv.id }}
    return 0
}}
route GET / = str([a(), b(), c()])
"
    );
    assert_eq!(lines(&src, PAST), [14, 22]);
}

/// Closures that assign `x`, then `k` `let x`s of one block.
fn closures_then_lets(k: usize) -> String {
    let mut src = String::from("fn f(c) {\n    let x = 0\n    if c {\n");
    for i in 0..k {
        src.push_str(&format!("        print(() => {{ x = {i} }})\n"));
    }
    for i in 0..k {
        src.push_str(&format!("        let x = {i}\n"));
    }
    src + "        return x\n    }\n    return x\n}\nroute GET / = str(f(true))\n"
}

/// `k` lambdas, each calling the two before it, and `k` `idempotent`s of the last.
fn many_sites(k: usize) -> String {
    let mut src = String::from("route POST / {\n    let f0 = () => http.post(\"https://a.example/x\", 1)\n    let f1 = () => f0()\n");
    for i in 2..k {
        src.push_str(&format!("    let f{i} = () => {{\n        print(f{}())\n        return f{}()\n    }}\n", i - 1, i - 2));
    }
    for _ in 0..k {
        src.push_str(&format!("    print(idempotent(() => str(f{}())))\n", k - 1));
    }
    src + "    return \"ok\"\n}\n"
}

#[test]
fn round7_closures_and_sites_are_read_in_linear_time() {
    // Found in review: each closure marked each `let` of the name (kl.py); each `idempotent` read
    // all it reaches again (lbb).
    let k = if cfg!(debug_assertions) { 2000 } else { 16000 };
    grows_linearly("closures-then-lets", k, &closures_then_lets);
    confined_ends("closures-then-lets", &closures_then_lets(4 * k));
    let k = if cfg!(debug_assertions) { 150 } else { 600 };
    grows_linearly("many-sites", k, &many_sites);
    confined_ends("many-sites", &many_sites(4 * k));
    assert_eq!(lines(&many_sites(4), REACHES).len(), 4);
}

#[test]
fn round7_what_a_file_is_taken_for_is_said_once_each_in_one_order() {
    // Found in review: the names were in the order the files were read, a name taken by two
    // files twice.
    let app = App::new(
        "round7-taken",
        &[
            ("main.qwe", "import kit.qwe use Beta, Alpha\nimport other.qwe use page\nroute GET / = <html><body><Beta/><Alpha/>{page()}</body></html>\n"),
            ("other.qwe", "import kit.qwe use Alpha, Beta\nfn page() = <div><Alpha/><Beta/></div>\n"),
            ("kit.qwe", "view Alpha() = <b>a</b>\nview Beta() = <i>b</i>\nroute GET /extra = \"x\"\n"),
        ],
    );
    // Said by `quillo check`, which scans what each file is taken for; with a deadline.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_quillo"))
        .args(["check", "main.qwe"])
        .current_dir(&app.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("quillo check did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let said: Vec<&str> = text.lines().filter(|l| l.contains("is taken for")).collect();
    assert_eq!(said, ["kit.qwe:3: warning: kit.qwe is taken for Alpha, Beta, and also adds GET /extra"], "{}", text);
}

// ---- Round 8 (found in review) ------------------------------------------------------------------

/// A chain of 199 functions, the last calling `k` others, each calling the head of a chain of `k`
/// that ends in `http.post`.
fn fan199(k: usize) -> String {
    let mut o: Vec<String> = (0..199).map(|i| format!("fn f{i}(x) = f{}(x)", i + 1)).collect();
    let mut last = String::from("fn f199(x) {\n");
    for s in (0..k).step_by(100) {
        let calls: Vec<String> = (s..(s + 100).min(k)).map(|j| format!("g{j}(x)")).collect();
        last.push_str(&format!("    print({})\n", calls.join(" + ")));
    }
    o.push(last + "    return 0\n}");
    o.extend((0..k).map(|j| format!("fn g{j}(x) = h0(x)")));
    o.extend((0..k).map(|i| format!("fn h{i}(x) = h{}(x)", i + 1)));
    o.push(format!("fn h{k}(x) = http.post(\"https://a.example/z\", x)"));
    o.push("route POST /x = idempotent(() => str(f0(1)))".to_string());
    o.join("\n") + "\n"
}

/// 64 functions in one cycle, each calling `k / 20` that post, and `k` `idempotent`s of the first.
fn cycle(k: usize) -> String {
    let f = (k / 20).max(1);
    let mut o: Vec<String> = (0..f).map(|j| format!("fn leaf{j}(x) = http.post(\"https://a.example/{j}\", x)")).collect();
    let calls: Vec<String> = (0..f).map(|j| format!("leaf{j}(x)")).collect();
    for i in 0..64 {
        o.push(format!("fn f{i}(x) {{\n    if x <= 0 {{ return {} }}\n    return f{}(x - 1)\n}}", calls.join(" + "), (i + 1) % 64));
    }
    o.push("route POST /x {".to_string());
    for j in 0..k {
        o.push(format!("    print(idempotent(() => str(f0({j}))))"));
    }
    o.join("\n") + "\n    return \"ok\"\n}\n"
}

#[test]
fn round8_what_idempotent_reaches_deep_or_in_a_cycle_is_read_once() {
    // Found in review: past 200 calls deep each function was read through again, and a cycle kept
    // nothing (fan199 at 16,000: 15.4 s; a cycle of 4,000: 13.7 s).
    let k = if cfg!(debug_assertions) { 1000 } else { 4000 };
    grows_linearly("fan199", k, &fan199);
    confined_ends("fan199", &fan199(4 * k));
    // A sum of more than 200 is past what the parser nests: the cycle's calls stay below.
    let c = if cfg!(debug_assertions) { 500 } else { 2000 };
    grows_linearly("cycle", c, &cycle);
    confined_ends("cycle", &cycle(2 * c));
    assert_eq!(lines(&fan199(3), REACHES), [fan199(3).lines().count() as u32]);
    assert_eq!(lines(&cycle(3), REACHES).len(), 3);
    // Past its time, a confined check ends with the one error, soon.
    let d = temp_dir("feedback86-89-check-r8-time");
    std::fs::write(d.join("main.qwe"), fan199(8 * k)).unwrap();
    let t = std::time::Instant::now();
    let a = check_file_opts(&d.join("main.qwe"), HashMap::new(), Options { confined: true, time: Some(Duration::from_millis(30)), ..Default::default() });
    let took = t.elapsed();
    let _ = std::fs::remove_dir_all(&d);
    let errors: Vec<String> = a.diagnostics.iter().map(|x| x.message.clone()).collect();
    assert_eq!(errors, ["the code is too complex to check: its types grew past what a check keeps, or the check took too long"]);
    assert!(took < Duration::from_secs(if cfg!(debug_assertions) { 30 } else { 3 }), "{:?}", took);
}

#[test]
fn round8_bind_changes_the_binding_it_reaches() {
    // Found in review: `bind:value={a}` on a lambda's parameter, a `for`'s `a` or a helper's `let a`
    // made the state `a` taken as bound.
    let src = "component C() {
    state a = 1
    state c = a * 2
    <div>{[1,2] | map((a) => <input bind:value={a}/>)}<p>{c}</p></div>
}
component D() {
    state a = 1
    state c = a * 2
    <div>{for a in [1] { <input bind:value={a}/> }}<p>{c}</p></div>
}
component E() {
    state a = 1
    state c = a * 2
    let f = () => {
        let a = 3
        return <input bind:value={a}/>
    }
    <div>{f()}<p>{c}</p></div>
}
component G() {
    state a = 1
    state total = a * 2
    <div><input bind:value={a}/><p>{total}</p></div>
}
route GET / = <html><body><C/><D/><E/><G/></body></html>
";
    assert_eq!(lines(src, ONCE), [line_of(src, "state total = a * 2")]);
    // A `let` a `bind:` changes holds no test.
    let held = format!("{INVOICE}component P() {{\n    let inv = invoice(\"\")\n    let ok = !is_err(inv)\n    <div><input type=\"checkbox\" bind:checked={{ok}}/>{{ok ? inv.id : 0}}</div>\n}}\nroute GET / = <html><body><P/></body></html>\n");
    assert_eq!(lines(&held, PAST), [5]);
}

#[test]
fn round8_a_test_holds_for_the_binding_it_tested() {
    // Found in review: a name tested by a `match` or by a function given it whole, and declared
    // again inside, was taken as tested there.
    let src = "fn invoice(name: str) -> map ! str = if name == \"\" { err(\"none\") } else { { id: 1, name } }
fn shown(r) = !is_err(r)
// match on outer inv, inner inv shadow inside arm: should warn line 8
fn a() {
    let inv = invoice(\"\")
    return match inv {
        { id } => {
            let inv = invoice(\"y\")
            inv.id
        }
        _ => 0
    }
}
// is_err test on outer, inner shadow inside branch: should warn line 17
fn b() {
    let inv = invoice(\"\")
    if !is_err(inv) {
        let inv = invoice(\"z\")
        return inv.id
    }
    return 0
}
// given whole to a call in a condition, shadow inside: should warn line 24
fn c() {
    let r = invoice(\"\")
    if shown(r) {
        let r = invoice(\"q\")
        return r.id
    }
    return 0
}
// early return test, then shadow in a lambda param: lambda's r is a plain param, no warn
fn d() {
    let r = invoice(\"\")
    if is_err(r) { return 0 }
    let g = (r) => r.id
    return g({ id: 3 }) + r.id
}
// test in a lambda doesn't test outer: should warn line 41
fn e() {
    let r = invoice(\"\")
    let t = (r) => is_err(r)
    if !t(1) { return r.id }
    return 0
}
// ?? left inside nested lambda shadow
fn f() {
    let r = invoice(\"\")
    return (r.id ?? 0) + 1
}
route GET / = str([a(), b(), c(), d(), e(), f()])
";
    assert_eq!(lines(src, PAST), [9, 19, 28, 43]);
}
