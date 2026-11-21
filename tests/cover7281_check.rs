//! Coverage of feedback72-81's `quillo check` work (FEEDBACK72.md, 73, 79, 80 and 81): the lines
//! of src/check.rs no test ran at 85ebb05 (coverage of feedback72-81; branch feedback72-81).

mod support;

use quillo::check::{check_file_opts, check_source, Diagnostic, Options, Severity};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use support::temp_dir;

fn shown(d: &Diagnostic) -> String {
    let sev = match d.severity {
        Severity::Error => "",
        Severity::Warning => " warning:",
        Severity::Hint => " hint:",
    };
    format!("{}{} {}", d.line, sev, d.message)
}

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new(".")).diagnostics.iter().map(shown).collect()
}

fn says(src: &str, want: &[&str]) {
    assert_eq!(diags(src), want, "\n--- source ---\n{}", src);
}

/// A folder of files for one test, gone when it ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, files: &[(&str, &str)]) -> App {
        let d = temp_dir(&format!("cover7281-check-{}", name));
        let app = App(d.canonicalize().unwrap());
        for (f, text) in files {
            app.put(f, text.as_bytes());
        }
        app
    }

    fn put(&self, f: &str, bytes: &[u8]) {
        let p = self.0.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// Each diagnostic of `quillo check file` (style sheets read, as the command reads them), with
    /// its file's name.
    fn check(&self, file: &str) -> Vec<String> {
        let dir = format!("{}/", self.0.display());
        let opts = Options { css: true, ..Default::default() };
        check_file_opts(&self.0.join(file), HashMap::new(), opts).diagnostics.iter().map(|d| format!("{}:{}", d.file.strip_prefix(&dir).unwrap_or(&d.file), shown(d))).collect()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- Q-1261: what the server runs as it makes a page, asked of the route's `with` ---------------

/// A module's function and a site's called in a component, in a state and in markup, need their
/// effects of the route: an error each (the route allows them all, nothing); a name of no module
/// (`tools.stamp`) and a parameter's field (`api.get`) need nothing; the route's own module call
/// is said by the route's check alone (src/check.rs render_effects' module and site calls and
/// render_refs' bound receiver, FEEDBACK80.md Q-1261; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1261_a_modules_and_a_sites_function_in_a_component() {
    let src = |with: &str| {
        format!(
            "site shop = \"http://127.0.0.1:9\"\nlet tools = {{ stamp: () => 1 }}\ncomponent Prices(api) {{\n    state items = shop.items()\n    <p>{{tools.stamp()}} {{api.get()}} {{date.now()}} {{len(items ?? [])}}</p>\n}}\nroute GET /{} = <html><body><Prices api={{tools}}/></body></html>\n",
            with
        )
    };
    let site = "4 `shop.items()` calls the site from the server, and <Prices> also runs in the browser: call it in a `server fn`";
    says(
        &src(" with db"),
        &[
            site,
            "7 `date.now` needs the time effect, but `route GET /` is declared `with db`: <Prices> calls it as it renders (line 5), which the server runs while it makes the page",
            "7 `shop.items` needs the http effect, but `route GET /` is declared `with db`: <Prices> calls it in `state items` (line 4), which the server runs while it makes the page",
        ],
    );
    says(&src(" with db, http, time"), &[site]);
    // The route's own `date.now()` is said once, where the route is checked, with no component.
    says(
        "component Day() {\n    <p>{date.today()}</p>\n}\nroute GET / with db = <html><body><Day/><p>{date.now()}</p></body></html>\n",
        &[
            "4 `date.now` needs the time effect, but `route GET /` is declared `with db`",
            "4 `date.today` needs the time effect, but `route GET /` is declared `with db`: <Day> calls it as it renders (line 2), which the server runs while it makes the page",
        ],
    );
}

/// Whether any component needs what a `with` lacks is read through a module's function, a site's,
/// and past a name of no module: one component of each, the only thing that needs the effect
/// (src/check.rs render_effects_possible, FEEDBACK80.md Q-1261; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1261_what_makes_a_page_worth_reading_is_a_modules_or_a_sites_call() {
    says(
        "component Day() {\n    <p>{date.today()}</p>\n}\nroute GET / with db = <html><body><Day/></body></html>\n",
        &["4 `date.today` needs the time effect, but `route GET /` is declared `with db`: <Day> calls it as it renders (line 2), which the server runs while it makes the page"],
    );
    says(
        "site shop = \"http://127.0.0.1:9\"\ncomponent Items() {\n    state items = shop.items() ?? []\n    <p>{len(items)}</p>\n}\nroute GET / with db = <html><body><Items/></body></html>\n",
        &[
            "3 `shop.items()` calls the site from the server, and <Items> also runs in the browser: call it in a `server fn`",
            "6 warning: `shop.items` needs the http effect, but `route GET /` is declared `with db`: <Items> calls it in `state items` (line 3), which the server runs while it makes the page, where `??` gives its fallback every time",
        ],
    );
    // `tools.stamp` first: it needs nothing, and `now` after it is what is found.
    says(
        "let tools = { stamp: () => 1 }\ncomponent Clock() {\n    <p>{tools.stamp()} {now()}</p>\n}\nroute GET / with db = <html><body><Clock/></body></html>\n",
        &["5 `now` needs the time effect, but `route GET /` is declared `with db`: <Clock> calls it as it renders (line 3), which the server runs while it makes the page"],
    );
}

/// A plain function a component calls twice is read once for the components it shows, and what
/// needs the effect is still said once (src/check.rs render_effects' `seen`, FEEDBACK80.md Q-1261;
/// coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1261_a_plain_function_called_twice() {
    says(
        "fn stamp() -> str with time = str(now())\nfn label(s: str) = s + \"!\"\ncomponent C() {\n    <p>{label(\"a\")} {label(\"b\")} {stamp()}</p>\n}\nroute GET / with db = <html><body><C/></body></html>\n",
        &["6 `stamp` needs the time effect, but `route GET /` is declared `with db`: <C> calls it as it renders (line 4), which the server runs while it makes the page"],
    );
}

/// A call said first as a warning (under a condition, in <C>) and then met where it always runs (in
/// <D>) becomes the error, said of <D> first, with the other component counted
/// (src/check.rs render_need, FEEDBACK80.md Q-1261; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1261_a_warning_then_the_same_call_sure_is_the_error() {
    says(
        "component C(show: bool) {\n    <p>{show ? str(now()) : \"x\"}</p>\n}\ncomponent D() {\n    <p>{str(now())}</p>\n}\nroute GET / with db = <html><body><C show={true}/><D/></body></html>\n",
        &["7 `now` needs the time effect, but `route GET /` is declared `with db`: <D> calls it as it renders (line 5), as does 1 other component, which the server runs while it makes the page"],
    );
}

/// What a function needs is named by what it calls, through plain functions: a site's function and
/// a module's, past a module's that needs nothing (`math.floor`) and a name of no module; a
/// connection's call is named as `db.query` is (src/check.rs effect_source, FEEDBACK80.md Q-1261;
/// coverage of feedback72-81; branch feedback72-81). Changed with the fix of what these tests
/// found: 85ebb05 said `rows` needs db with no "(it calls `conn.query`)", where `db.query` gets
/// "(it calls `db.query`)" (FEEDBACK80.md, Q-1261, found by the coverage tests of feedback72-81).
#[test]
fn q1261_what_needs_the_effect_is_named_through_modules_and_sites() {
    says(
        "site shop = \"http://127.0.0.1:9\"\nlet tools = { stamp: () => 1 }\nfn stamp() = str(math.floor(1.5)) + str(tools.stamp()) + str(date.now())\nfn items() = shop.items()\nserver fn load_it() -> str = stamp() + str(items())\ncomponent Shows() {\n    state v = load_it()\n    <p>{v}</p>\n}\nroute GET / with db = <html><body><Shows/></body></html>\n",
        &["10 warning: `load_it` needs the http, time effects (it calls `shop.items`, `date.now`), but `route GET /` is declared `with db`: <Shows> calls it in `state v` (line 7), which the server runs while it makes the page"],
    );
    says(
        "let conn = db.connect(\"a.db\")\nserver fn rows() -> list = conn.query(\"select 1\")\ncomponent Rows() {\n    state rs = rows()\n    <p>{len(rs ?? [])}</p>\n}\nroute GET / with time = <html><body><Rows/></body></html>\n",
        &["7 warning: `rows` needs the db effect (it calls `conn.query`), but `route GET /` is declared `with time`: <Rows> calls it in `state rs` (line 4), which the server runs while it makes the page"],
    );
}

/// A connection's call is named wherever it is reached: through a plain function, with the
/// connection made with `?`, beside a call that needs another effect, and the first of two; a
/// parameter of the connection's name is no connection, and `db.query` is named as it was
/// (src/check.rs effect_source, FEEDBACK80.md, Q-1261, found by the coverage tests of
/// feedback72-81; branch feedback72-81)
#[test]
fn q1261_a_connections_call_is_named() {
    let page = |with: &str| format!("component Rows() {{\n    state rs = rows()\n    <p>{{len(rs ?? [])}}</p>\n}}\nroute GET / with {} = <html><body><Rows/></body></html>\n", with);
    let warn = |line: u32, needs: &str, with: &str| {
        format!("{} warning: `rows` needs {}, but `route GET /` is declared `with {}`: <Rows> calls it in `state rs` (line {}), which the server runs while it makes the page", line, needs, with, line - 3)
    };
    says(
        &format!("let conn = db.connect(\"a.db\")?\nfn load() -> list = conn.query(\"select 1\")\nserver fn rows() -> list = load()\n{}", page("time")),
        &[&warn(8, "the db effect (it calls `conn.query`)", "time")],
    );
    says(
        &format!("let conn = db.connect(\"a.db\")\nserver fn rows() -> list = conn.query(\"select ?\", str(now()))\n{}", page("http")),
        &[&warn(7, "the db, time effects (it calls `conn.query`, `now`)", "http")],
    );
    says(
        &format!("let conn = db.connect(\"a.db\")\nserver fn rows() -> list = db.query(\"select 1\")\n{}", page("time")),
        &[&warn(7, "the db effect (it calls `db.query`)", "time")],
    );
    // A plain function's parameter called `conn` is not the connection: its `db.query` is named.
    says(
        &format!("let conn = db.connect(\"a.db\")\nfn load(conn) -> list = conn.query(\"select 1\") + db.query(\"select 2\")\nserver fn rows() -> list = load(conn)\n{}", page("time")),
        &[&warn(8, "the db effect (it calls `db.query`)", "time")],
    );
    // A unicode name for the connection, and its `one` beside `query`: the first is named.
    says(
        &format!("let cönn = db.connect(\"a.db\")\nserver fn rows() -> list = [cönn.one(\"select 1\")] + cönn.query(\"select 2\")\n{}", page("time")),
        &[&warn(7, "the db effect (it calls `cönn.one`)", "time")],
    );
}

/// A call whose function is made by another call, taken from a list or from a call's field, in
/// markup and after `|`: what the server runs to get the function is read too (src/check.rs
/// render_refs' unnamed callees, FEEDBACK80.md Q-1261; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1261_what_runs_to_get_the_function_called() {
    let err = |line: u32| format!("{} `now` needs the time effect, but `route GET /` is declared `with db`: <C> calls it as it renders (line 3), which the server runs while it makes the page", line);
    let fmt = "fn fmt(prefix: str) = (x) => prefix + str(x)\n";
    let clock = "fn clock(t: str) = { stamp: (x) => t + str(x) }\n";
    for (head, markup, props) in [
        (fmt, "{fmt(str(now()))(1)}", ""),
        (fmt, "{1 | fmt(str(now()))()}", ""),
        (fmt, "{1 | fs[len(str(now())) % 1]}", " fs={[(x) => x]}"),
        (clock, "{clock(str(now())).stamp(1)}", ""),
        (clock, "{1 | clock(str(now())).stamp()}", ""),
        (clock, "{1 | clock(str(now())).stamp}", ""),
    ] {
        let params = if props.is_empty() { "" } else { "fs: list" };
        let src = format!("{head}component C({params}) {{\n    <p>{markup}</p>\n}}\nroute GET / with db = <html><body><C{props}/></body></html>\n");
        says(&src, &[&err(5)]);
    }
}

/// A `match` guard, a `while`'s condition and body, and a `reply`'s body with and without one: the
/// guard and the body run on a condition (a warning), the condition every time (an error), and a
/// component in `reply`'s body is read (src/check.rs render_refs' match guard, while and reply,
/// FEEDBACK80.md Q-1261; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1261_a_guard_a_while_and_a_reply() {
    says(
        "component C(xs: list) {\n    <ul>{match len(xs) { n if n > now() => <li>many</li>, _ => <li>few</li> }}</ul>\n}\nroute GET / with db = <html><body><C xs={[1]}/></body></html>\n",
        &["4 warning: `now` needs the time effect, but `route GET /` is declared `with db`: <C> calls it as it renders (line 2), which the server runs while it makes the page"],
    );
    says(
        "component C() {\n    let i = 0\n    while i < len(str(now())) {\n        i += random_int(1, 2)\n    }\n    <p>{i}</p>\n}\nroute GET / with db = <html><body><C/></body></html>\n",
        &[
            "8 `now` needs the time effect, but `route GET /` is declared `with db`: <C> calls it as it renders (line 3), which the server runs while it makes the page",
            "8 warning: `random_int` needs the random effect, but `route GET /` is declared `with db`: <C> calls it as it renders (line 4), which the server runs while it makes the page",
        ],
    );
    says(
        "component C() {\n    <p>{now()}</p>\n}\nroute GET /a with db = reply 200 <html><body><C/></body></html>\nroute GET /b with db = reply 200\n",
        &["4 `now` needs the time effect, but `route GET /a` is declared `with db`: <C> calls it as it renders (line 2), which the server runs while it makes the page"],
    );
}

/// A field set in a component's body (`el.title = str(now())`) runs as the page is made; a local
/// `fn` there does not (src/check.rs render_refs' SetField and other statements, FEEDBACK80.md
/// Q-1261; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1261_a_field_set_and_a_local_fn_in_a_components_body() {
    says(
        "component C() {\n    let el = js(\"document.body\")\n    el.title = str(now())\n    fn later() { print(random()) }\n    <p>x</p>\n}\nroute GET / with db = <html><body><C/></body></html>\n",
        &["7 `now` needs the time effect, but `route GET /` is declared `with db`: <C> calls it as it renders (line 3), which the server runs while it makes the page"],
    );
}

// ---- Q-1267 and Q-1018: the ends of statements dropped ------------------------------------------

/// A `match` arm whose value is dropped and whose subject is no name: `[x] | tidy()` is said though
/// the arm binds `x`, while `x | tidy()` names what the arm binds and is let be; a field's field's
/// call (`st.items.sort()`) has no known receiver and is let be (src/check.rs dropped_subject,
/// FEEDBACK80.md Q-1267; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1267_an_ends_subject_that_is_no_name() {
    let tidy = "fn tidy(xs: list) = sort(xs)\n";
    says(
        &format!("{tidy}fn f(xs: list) {{\n    match xs {{ [x] => [x] | tidy(), _ => print(2) }}\n    xs\n}}\nprint(f([]))\n"),
        &["3 warning: `tidy` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out"],
    );
    says(&format!("{tidy}fn f(xs: list) {{\n    match xs {{ [x] => x | tidy(), _ => print(2) }}\n    xs\n}}\nprint(f([]))\n"), &[]);
    says(&format!("{tidy}fn f(xs: list) {{\n    match xs {{ [x] => tidy([x]) + [1], _ => print(2) }}\n    xs\n}}\nprint(f([]))\n"), &[]);
    says("fn f(st: map, n: int) {\n    match n { 0 => st.items.sort(), 1 => st.items.push(1), _ => print(2) }\n    st\n}\nprint(f({ items: [] }, 0))\n", &[]);
}

fn else_ifs(n: usize, indent: &str) -> String {
    let mut s = "if n == 0 { print(0) }".to_string();
    for i in 1..n {
        s.push_str(&format!(" else if n == {} {{ print({}) }}", i, i));
    }
    format!("{}\n{}else {{ xs.push(1) }}", s, indent)
}

/// An `else if` chain as a statement, ten long, and one of eight, nine and seven: its last `else`'s
/// dropped `push` is said, however deep, with room on the stack looked at every eighth
/// (src/check.rs mark_end, FEEDBACK80.md Q-1267; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1267_a_long_else_if_chain() {
    for n in [7, 8, 9, 10, 40] {
        let src = format!("fn f(xs: list, n: int) {{\n    {}\n    xs\n}}\nprint(f([], 3))\n", else_ifs(n, "    "));
        says(&src, &["3 warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`"]);
    }
}

/// A handler named by reference whose `fn` ends in a `match` (an arm not a block, and one), a `for`,
/// a block, or a long `else if` chain: each end's dropped `push` is said
/// (src/check.rs named_tail, FEEDBACK80.md Q-1267; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1267_a_named_handler_ends_in_a_match_a_for_a_block_or_an_else_if_chain() {
    let w = |line: u32| format!("{} warning: `push` gives a new value and changes nothing, and it is dropped here: write `items = items | push(...)`", line);
    says(
        "component G() {\n    state items = []\n    fn a() {\n        match len(items) {\n            0 => items.push(1),\n            _ => { print(1); push(items, 2) }\n        }\n    }\n    fn b() {\n        for x in [1, 2] { items | push(x) }\n    }\n    fn c() {\n        print(0)\n        { items.push(3) }\n    }\n    <div><button onclick={a}>a</button><button onclick={b}>b</button><button onclick={c}>c</button>{len(items)}</div>\n}\nroute GET / = <html><body><G/></body></html>\n",
        &[&w(5), &w(6), &w(10), &w(14)],
    );
    for n in [7, 8, 9, 20] {
        let src = format!(
            "component G() {{\n    state xs = []\n    let n = 3\n    fn go() {{\n        {}\n    }}\n    <button onclick={{go}}>{{len(xs)}}</button>\n}}\nroute GET / = <html><body><G/></body></html>\n",
            else_ifs(n, "        ")
        );
        says(&src, &["6 warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`"]);
    }
}

/// A handler's function that calls itself, or another that calls it back: the search for an error
/// it gives ends, finding none in `go` and `check`'s through `a` and `b`
/// (src/check.rs gives_program_error, FEEDBACK72.md Q-1018; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1018_a_recursive_handler_function_ends() {
    says(
        "fn check(n: int) -> int ! str = n > 0 ? n : err(\"no\")\nfn a(n: int) -> int = n > 0 ? b(n - 1) : 0\nfn b(n: int) -> int = n > 0 ? a(n - 1) : check(n)\nfn go(n: int) -> int = n > 0 ? go(n - 1) : 0\ncomponent C() {\n    <div><button onclick={() => go(3)}>a</button><button onclick={() => a(3)}>b</button></div>\n}\nroute GET / = <html><body><C/></body></html>\n",
        &[
            "6 warning: `go` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
            "6 warning: `a` changes nothing, and what it gives is dropped here: use what it gives, or leave the call out",
            "6 warning: `a` can give an error, and this handler drops it: give a value with `a(...) ?? ...`, test it with `is_err`, or write `let _ = a(...)` if it does not matter",
        ],
    );
}

// ---- Q-1268: a plain `let` a handler changes ----------------------------------------------------

/// A handler's own `let [total, b] = ...` hides the component's `total`, which stays unsaid, while
/// one that destructures other names still changes `other`; a handler made by a call
/// (`onpick={choose(1)}`) names no function of the component's (src/check.rs let_not_state,
/// FEEDBACK80.md Q-1268; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1268_a_handlers_destructured_let_and_a_handler_made_by_a_call() {
    says(
        "component Pair() {\n    let total = 0\n    let other = 0\n    <div><button onclick={() => { let [total, b] = [1, 2]; total = b }}>a</button><button onclick={() => { let [x, y] = [1, 2]; other = x + y }}>b</button>{total} {other}</div>\n}\nroute GET / = <html><body><Pair/></body></html>\n",
        &["3 warning: `other` is changed in a handler but isn't a state, so the page won't show it: write `state other = 0`"],
    );
    says(
        "fn choose(n: int) = () => print(n)\ncomponent Child(onpick) {\n    <button onclick={() => onpick()}>pick</button>\n}\ncomponent P() {\n    let picked = 0\n    <div><Child onpick={choose(1)}/><p>{picked}</p><button onclick={() => { picked = 2 }}>x</button></div>\n}\nroute GET / = <html><body><P/></body></html>\n",
        &["6 warning: `picked` is changed in a handler but isn't a state, so the page won't show it: write `state picked = 0`"],
    );
}

// ---- Q-1019: a click the keyboard can't reach ----------------------------------------------------

/// A `<div>`'s click handler given as a map's field takes no event the check can see: the keyboard
/// can't reach it (src/check.rs takes_event, FEEDBACK72.md Q-1019; coverage of feedback72-81;
/// branch feedback72-81)
#[test]
fn q1019_a_handler_given_as_a_field() {
    says(
        "let actions = { open: () => print(1) }\ncomponent Menu() {\n    <div onclick={actions.open}>menu</div>\n}\nroute GET / = <html><body><Menu/></body></html>\n",
        &["3 warning: a `<div>` with onclick can't be reached with the keyboard: use a `<button>`"],
    );
}

// ---- Q-573: a view whose element is a block -------------------------------------------------------

/// A component whose body is a block ending in a `<ul>` starts a line of its own; one ending in
/// `return <em>...` and a view that chooses between two inline elements are not known to be blocks
/// and are said (src/check.rs root_tag; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q573_a_block_bodys_element_and_others() {
    let src = "component Steps() {\n    state n = 1\n    <ul><li>{n}</li></ul>\n}\ncomponent Note() {\n    state n = 1\n    return <em>{n}</em>\n}\nview Pick(open: bool) = open ? <em>a</em> : <b>b</b>\nroute GET / = <html><body><div>\n    first\n    <Steps/>\n</div><div>\n    then\n    <Note/>\n</div><div>\n    last\n    <Pick open={true}/>\n</div></body></html>\n";
    says(
        src,
        &[
            "15 warning: \"then\" and <Note> are on lines of their own, so no space comes between them on the page: write {\" \"} between them, or put them on one line",
            "18 warning: \"last\" and <Pick> are on lines of their own, so no space comes between them on the page: write {\" \"} between them, or put them on one line",
        ],
    );
}

/// A view whose block ends in `return <ul>...` starts a line of its own, as one written `= <ul>...`
/// does, after a `let` too and next to text in any letters; one that returns an inline element, or
/// whose block ends in a bare `return`, is said, as the bare forms are (src/check.rs root_tag,
/// FEEDBACK48.md, Q-573, found by the coverage tests of feedback72-81; branch feedback72-81)
#[test]
fn q573_a_view_given_with_return() {
    let placed = |view: &str, text: &str| format!("{}print(<div>\n    {}\n    <List><li>x</li></List>\n</div>)\n", view, text);
    let said = |line: u32, word: &str| format!("{} warning: \"{}\" and <List> are on lines of their own, so no space comes between them on the page: write {{\" \"}} between them, or put them on one line", line, word);
    says(&placed("view List(children) {\n    return <ul>{children}</ul>\n}\n", "a list:"), &[]);
    says(&placed("view List(children) = <ul>{children}</ul>\n", "a list:"), &[]);
    says(&placed("view List(children) {\n    <ul>{children}</ul>\n}\n", "a list:"), &[]);
    says(&placed("view List(children) {\n    let kind = \"steps\"\n    return <ol class={kind}>{children}</ol>\n}\n", "ünë líst:"), &[]);
    says(&placed("component List(children) {\n    state n = 1\n    return <ul data-n={n}>{children}</ul>\n}\n", "a list:"), &[]);
    says(&placed("view List(children) {\n    return <span>{children}</span>\n}\n", "a list:"), &[&said(6, "list:")]);
    says(&placed("view List(children) {\n    <span>{children}</span>\n}\n", "a list:"), &[&said(6, "list:")]);
    says(&placed("view List(children) = <span>{children}</span>\n", "líst:"), &[&said(4, "líst:")]);
}

// ---- Q-1015: what a component draws again as it wakes ---------------------------------------------

/// A component written as `component Flat() = <p>{random()}</p>` is said; one whose markup is a
/// `return` shows nothing it drew; a helper showing a component and a server fn's value draws
/// nothing itself, the component it shows being said on its own (src/check.rs drawn_twice and
/// draws_def, FEEDBACK72.md Q-1015; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1015_components_of_other_shapes() {
    let src = "server fn load() -> str = \"x\"\ncomponent Inner() {\n    <i>{random()}</i>\n}\nfn frame() = <div><Inner/>{load()}</div>\ncomponent Dice() {\n    let r = random()\n    print(r)\n    <p>{frame()}</p>\n}\ncomponent Back() {\n    let r = random()\n    print(r)\n    return <p>hi</p>\n}\ncomponent Flat() = <p>{random()}</p>\nroute GET / = <html><body><Dice/><Back/><Flat/></body></html>\n";
    let w = |line: u32, c: &str| format!("{} warning: `random()` here is drawn again in the browser when <{}> wakes, so what the page shows changes under the visitor: keep it in a state, `state x = random()`, which draws once, on the server, and the browser keeps it", line, c);
    says(src, &[&w(3, "Inner"), &w(16, "Flat")]);
}

/// Markup given with `return` is what the component shows, as the markup a body ends with is: a
/// `let` it reads that draws, a draw in it, a `let` read through another, and both forms' `if` whose
/// branches give the markup; a `state`, a value read only in a handler, and a body that returns
/// nothing stay quiet (src/check.rs drawn_twice, FEEDBACK72.md, Q-1015, found by the coverage tests
/// of feedback72-81; branch feedback72-81)
#[test]
fn q1015_markup_given_with_return() {
    let page = "route GET / = <html><body><B/></body></html>\n";
    let comp = |body: &str| format!("component B() {{\n{}}}\n{}", body, page);
    let kept = |line: u32, n: &str| format!("{} warning: `{}` is drawn again in the browser when <B> wakes, so what the page shows changes under the visitor: `state {} = ...` draws once, on the server, and the browser keeps it", line, n, n);
    let call = |line: u32, f: &str| format!("{} warning: `{}()` here is drawn again in the browser when <B> wakes, so what the page shows changes under the visitor: keep it in a state, `state x = {}()`, which draws once, on the server, and the browser keeps it", line, f, f);
    says(&comp("    let r = random()\n    return <p>{r}</p>\n"), &[&kept(2, "r")]);
    says(&comp("    let r = random()\n    <p>{r}</p>\n"), &[&kept(2, "r")]);
    says(&comp("    return <p>{uuid()}</p>\n"), &[&call(2, "uuid")]);
    says(&comp("    let n = random_int(1, 6)\n    let shown = \"🎲 \" + str(n)\n    return <p>{shown}</p>\n"), &[&kept(2, "n")]);
    let branches = |ret: &str| {
        format!(
            "component B(flag: bool) {{\n    let r = random()\n    if flag {{\n        {}<p>{{r}}</p>\n    }} else {{\n        {}<i>no</i>\n    }}\n}}\nroute GET / = <html><body><B flag={{true}}/></body></html>\n",
            ret, ret
        )
    };
    says(&branches("return "), &[&kept(2, "r")]);
    says(&branches(""), &[&kept(2, "r")]);
    says(&comp("    state r = random()\n    return <p>{r}</p>\n"), &[]);
    says(&comp("    let r = random()\n    return <button onclick={() => print(r)}>roll</button>\n"), &[]);
    says(&comp("    let _r = random()\n    return <p></p>\n"), &[]);
}

// ---- Q-1251: links no route answers ---------------------------------------------------------------

/// A link of `..` that the browser resolves to `/b`, said with no suggestion as `/b` is, a
/// template with no value in it (a string once parsed), a template after a `*rest` route's start
/// and after a typed parameter's piece, and a route with a piece the link lacks (src/check.rs dead links,
/// link_start, link_fits and link_distance, FEEDBACK79.md Q-1251; coverage of feedback72-81; branch
/// feedback72-81). Line 2 changed with the fix of what these tests found: 85ebb05 measured the
/// suggestion on the link as written, counting the `a/..`, and said "(did you mean `/`?)", which
/// keeps nothing of `/b` (FEEDBACK79.md, Q-1251, found by the coverage tests of feedback72-81).
#[test]
fn q1251_dots_a_plain_template_a_rest_and_a_typed_piece() {
    let src = "route GET / = <nav>\n    <a href=\"/a/../b\">up</a>\n    <a href={`/nowhere`}>tpl</a>\n    <a href={`/about`}>tpl ok</a>\n    <a href={`/assets/${1}`}>asset</a>\n    <a href={`/u/7/${1}`}>posts</a>\n    <a href={`/u/x/${1}`}>bad id</a>\n    <a href=\"/about\">about</a>\n    <a href=\"/abou\">near</a>\n</nav>\nroute GET /about/team = \"team\"\nroute GET /assets/*path = files(\"public\", path)\nroute GET /u/:id(int)/posts = \"posts\"\n";
    says(
        src,
        &[
            "2 warning: no route answers `/a/../b`, so this link is a 404",
            "3 warning: no route answers `/nowhere`, so this link is a 404",
            "4 warning: no route answers `/about`, so this link is a 404",
            "7 warning: no route answers `/u/x/…`, so this link is a 404",
            "8 warning: no route answers `/about`, so this link is a 404",
            "9 warning: no route answers `/abou`, so this link is a 404",
        ],
    );
}

/// "Did you mean" is measured on the address as the browser asks for it, the pieces the match
/// reads: `/a/../b`, `/%62` and `/./b` are `/b`, which keeps nothing of `/`, as `/b` itself is said;
/// `/%C3%A9` is `/é`, one letter; the near ones are still found through dots and escapes; how many
/// edits a suggestion may take follows the resolved length, 2 for `/abcdefg` however it is written
/// (85ebb05 allowed 3 for `/x/../abcdefg`), 1 for `/abcdef`; a link resolved to the home is not
/// said (src/check.rs dead_links, FEEDBACK79.md, Q-1251, found by the coverage tests of
/// feedback72-81; branch feedback72-81)
#[test]
fn q1251_a_suggestion_is_measured_on_the_resolved_address() {
    let no = |link: &str| format!("1 warning: no route answers `{}`, so this link is a 404", link);
    let near = |link: &str, to: &str| format!("1 warning: no route answers `{}`, so this link is a 404 (did you mean `{}`?)", link, to);
    let page = |links: &[&str], routes: &str| {
        let a: String = links.iter().map(|l| format!("<a href=\"{}\">x</a>", l)).collect();
        format!("route GET / = <nav>{}</nav>\n{}", a, routes)
    };
    let all = |links: &[&str], routes: &str, want: Vec<String>| says(&page(links, routes), &want.iter().map(String::as_str).collect::<Vec<_>>());
    all(&["/a/../b", "/b", "/%62", "/./b"], "", vec![no("/a/../b"), no("/b"), no("/%62"), no("/./b")]);
    all(&["/%C3%A9", "/caf%C3%A9"], "route GET /cafe = \"c\"\n", vec![no("/%C3%A9"), near("/caf%C3%A9", "/cafe")]);
    all(&["/a/../abou", "/%61bou", "/abou"], "route GET /about = \"a\"\n", vec![near("/a/../abou", "/about"), near("/%61bou", "/about"), near("/abou", "/about")]);
    let both = ["/abcdefg", "/x/../abcdefg"];
    all(&both, "route GET /abcdeyz = \"2\"\n", vec![near("/abcdefg", "/abcdeyz"), near("/x/../abcdefg", "/abcdeyz")]);
    all(&both, "route GET /abcdxyz = \"3\"\n", vec![no("/abcdefg"), no("/x/../abcdefg")]);
    all(&["/abcdef", "/x/../abcdef"], "route GET /abcdyz = \"2\"\n", vec![no("/abcdef"), no("/x/../abcdef")]);
    all(&["/abcdef", "/x/../abcdef"], "route GET /abcdez = \"1\"\n", vec![near("/abcdef", "/abcdez"), near("/x/../abcdef", "/abcdez")]);
    all(&["/./", "/a/.."], "", vec![]);
}

// ---- Q-1022, Q-1012 and Q-1019: the files the server serves ---------------------------------------

/// A public page's `class` written with spaces around `=` or a line break after it, the word
/// `class` in text, and a page that ends at `class=` (src/check.rs html_classes, FEEDBACK73.md
/// Q-1022; coverage of feedback72-81; branch feedback72-81)
#[test]
fn q1022_class_attributes_spaced_or_cut_short() {
    let app = App::new(
        "html-classes",
        &[
            ("main.qwe", "route GET /*path = files(\"public\", path)\n"),
            ("public/index.html", "<p>a class act</p>\n<p class = \"spaced\">x</p>\n<p class=\n  \"wrapped\">y</p>\n<p class="),
            ("public/style.css", ".act { }\n.spaced { }\n.wrapped { }\n.lonely { }\n"),
        ],
    );
    assert_eq!(
        app.check("main.qwe"),
        [
            "public/style.css:1 warning: `.act` is styled here, but nothing in the program writes it",
            "public/style.css:4 warning: `.lonely` is styled here, but nothing in the program writes it",
        ]
    );
}

/// A sheet with no `:focus` beside one that takes the ring away: only the other is said
/// (src/check.rs focus_rings_removed, FEEDBACK72.md Q-1019; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1019_a_sheet_without_focus_beside_one_with_it() {
    let page = "route GET / = <html><head><link rel=\"stylesheet\" href=\"/s.css\"/></head><body><button.b>a</button></body></html>\nroute GET /*path = files(\"public\", path)\n";
    let app = App::new("focus", &[("main.qwe", page), ("public/a.css", ".b { color: red }\n"), ("public/s.css", "button { color: blue }\n:focus { outline: none }\n")]);
    assert_eq!(app.check("main.qwe"), ["public/s.css:2 warning: `outline: none` on `:focus` takes the keyboard's ring away everywhere: style `:focus-visible` instead"]);
}

/// A folder `files()` is given as a value is not known, so its pages are not read; written out,
/// they are (src/check.rs unused_classes' folders, FEEDBACK73.md Q-1022; coverage of feedback72-81;
/// branch feedback72-81)
#[test]
fn q1022_a_folder_given_as_a_value() {
    fn files(main: &str) -> [(&str, &str); 3] {
        [("main.qwe", main), ("site/page.html", "<p class=\"served\">x</p>\n"), ("public/style.css", ".served { }\n")]
    }
    let app = App::new("folder-value", &files("let dir = \"site\"\nroute GET /site/*path = files(dir, path)\nroute GET /*path = files(\"public\", path)\n"));
    assert_eq!(app.check("main.qwe"), ["public/style.css:1 warning: `.served` is styled here, but nothing in the program writes it"]);
    let app = App::new("folder-literal", &files("route GET /site/*path = files(\"site\", path)\nroute GET /*path = files(\"public\", path)\n"));
    assert_eq!(app.check("main.qwe"), Vec::<String>::new());
}

/// The served folders' pages are read up to 16 MB in all: a page that fits exactly is read, one
/// byte more and the rest of the folders are not (a sparse 16 MB file, so no disk is used)
/// (src/check.rs unused_classes' room, FEEDBACK73.md Q-1022; coverage of feedback72-81; branch
/// feedback72-81)
#[test]
fn q1022_the_served_pages_stop_at_16_mb() {
    let late = "<p class=\"late\">x</p>\n";
    for (extra, want) in [(0u64, Vec::<String>::new()), (1, vec!["public/style.css:1 warning: `.late` is styled here, but nothing in the program writes it".to_string()])] {
        let app = App::new(
            &format!("room-{}", extra),
            &[
                ("main.qwe", "route GET /big/*path = files(\"big\", path)\nroute GET /site/*path = files(\"site\", path)\nroute GET /*path = files(\"public\", path)\n"),
                ("site/page.html", late),
                ("public/style.css", ".late { }\n"),
            ],
        );
        std::fs::create_dir_all(app.0.join("big")).unwrap();
        let big = std::fs::File::create(app.0.join("big/page.html")).unwrap();
        big.set_len((16 << 20) - late.len() as u64 + extra).unwrap();
        drop(big);
        assert_eq!(app.check("main.qwe"), want, "{} byte(s) over", extra);
    }
}

/// A served folder gives at most 4,096 pages: all 4,096 are read, and of 4,097 one is not, whose
/// class is then said (src/check.rs program_files' cap, FEEDBACK73.md Q-1022; coverage of
/// feedback72-81; branch feedback72-81)
#[test]
fn q1022_a_served_folder_gives_at_most_4096_pages() {
    for n in [4096usize, 4097] {
        let app = App::new(&format!("cap-{}", n), &[("main.qwe", "route GET /site/*path = files(\"site\", path)\nroute GET /*path = files(\"public\", path)\n")]);
        let mut sheet = String::new();
        for i in 0..n {
            app.put(&format!("site/p{}.html", i), format!("<p class=\"c{}\">x</p>\n", i).as_bytes());
            sheet.push_str(&format!(".c{} {{ }}\n", i));
        }
        app.put("public/style.css", sheet.as_bytes());
        let got = app.check("main.qwe");
        if n == 4096 {
            assert_eq!(got, Vec::<String>::new());
        } else {
            assert_eq!(got.len(), 1, "{:?}", got);
            // The one page not read: its class, at its line of the sheet.
            let k: usize = got[0].split("`.c").nth(1).and_then(|s| s.split('`').next()).and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("{:?}", got));
            assert!(k < n, "{:?}", got);
            assert_eq!(got[0], format!("public/style.css:{} warning: `.c{}` is styled here, but nothing in the program writes it", k + 1, k));
        }
    }
}

/// A link under public/ to a folder whose real path is longer than the system takes
/// (`canonicalize` fails though the folder opens) is passed over, and its page's class is said;
/// where the system takes the path, the page is read (src/check.rs program_files, FEEDBACK73.md
/// Q-1022; coverage of feedback72-81; branch feedback72-81)
#[cfg(unix)]
#[test]
fn q1022_a_link_whose_real_path_is_too_long() {
    use std::os::unix::fs::symlink;
    let app = App::new(
        "long-path",
        &[("main.qwe", "route GET /*path = files(\"public\", path)\n"), ("public/style.css", ".deep { }\n.card { }\n"), ("public/index.html", "<p class=\"card\">x</p>\n")],
    );
    // Each system call's path stays short: `s` stands for three pieces of 240 letters, and two
    // more are made through it, so the folder's real path is four pieces deep in `a`.
    let n = "n".repeat(240);
    std::fs::create_dir_all(app.0.join(format!("a/{n}/{n}/{n}"))).unwrap();
    symlink(format!("a/{n}/{n}/{n}"), app.0.join("s")).unwrap();
    std::fs::create_dir(app.0.join(format!("s/{n}"))).unwrap();
    std::fs::write(app.0.join(format!("s/{n}/page.html")), "<p class=\"deep\">x</p>\n").unwrap();
    symlink(format!("../s/{n}"), app.0.join("public/deep")).unwrap();
    let link = app.0.join("public/deep");
    assert!(std::fs::metadata(&link).is_ok_and(|m| m.is_dir()));
    let got = app.check("main.qwe");
    if std::fs::canonicalize(&link).is_err() {
        assert_eq!(got, ["public/style.css:1 warning: `.deep` is styled here, but nothing in the program writes it"]);
    } else {
        assert_eq!(got, Vec::<String>::new());
    }
}
