//! FEEDBACK72.md, FEEDBACK79.md, FEEDBACK80.md and FEEDBACK81.md: what `quillo check` follows
//! through calls, and the values it sees dropped (slice C1; branch feedback72-81).

mod support;

use quillo::check::{check_file, check_source, Diagnostic, Severity};
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

/// The diagnostics that hold `part`, each as `line warning: message`.
fn about(src: &str, part: &str) -> Vec<String> {
    diags(src).into_iter().filter(|d| d.contains(part)).collect()
}

/// A folder of files for one test, gone when it ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, files: &[(&str, &str)]) -> App {
        let d = temp_dir(&format!("feedback72-81-check-{}", name));
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

// ---- Q-1258: a group's guard and the server fns its pages reach through helpers ----------------

const GUARD: &str = "fn agent(r) = session().name != null ? r : reply 303 redirect(\"/\")\n";

fn guard_warning(line: u32, f: &str, route: &str) -> String {
    format!(
        "{} warning: `{}` is called by `{}`'s page, which `agent` guards, but a group guards its routes, not the server fns their pages call: anyone can call it at POST /_quillo/rpc/{}; if it is not for everyone, check the request in it too, as `agent(request)`",
        line, f, route, f
    )
}

// (FEEDBACK79.md, Q-1258; branch feedback72-81)
#[test]
fn q1258_a_component_a_helper_shows_is_the_pages() {
    let src = format!(
        "{GUARD}server fn load_tickets() -> list = [\"printer jam\"]\ncomponent TicketList() {{\n    state ts = load_tickets()\n    <ul>{{for t in ts ?? [] => <li>{{t}}</li>}}</ul>\n}}\nfn team_home() = <html><body><TicketList/></body></html>\nroutes /desk | agent {{\n    route GET / = team_home()\n}}\n"
    );
    says(&src, &[&guard_warning(2, "load_tickets", "GET /desk")]);
}

// (FEEDBACK79.md, Q-1258; branch feedback72-81)
#[test]
fn q1258_helpers_are_followed_as_far_as_they_go_and_a_recursive_one_ends() {
    // Three helpers deep, one of them calling itself, and the server fn named, not called.
    let src = format!(
        "{GUARD}server fn load_replies() -> list = []\ncomponent Replies(id: int) {{\n    state rs = live(load_replies)\n    <ol>{{for r in rs ?? [] => <li>{{r}}</li>}}</ol>\n}}\nfn body_of(id: int, n: int) = n > 0 ? body_of(id, n - 1) : <Replies id={{id}}/>\nfn layout(inner) = <html><body>{{inner}}</body></html>\nfn ticket(id: int) = layout(body_of(id, 3))\nroutes /t | agent {{\n    route GET /:id(int) = ticket(id)\n}}\n"
    );
    says(&src, &[&guard_warning(2, "load_replies", "GET /t/:id(int)")]);
}

// (FEEDBACK79.md, Q-1258; branch feedback72-81)
#[test]
fn q1258_what_stays_as_it_was() {
    // Written in the route itself: warned as before.
    let direct = format!(
        "{GUARD}server fn load() -> list = []\ncomponent L() {{\n    state xs = load()\n    <p>{{len(xs ?? [])}}</p>\n}}\nroutes /desk | agent {{\n    route GET / = <html><body><L/></body></html>\n}}\n"
    );
    says(&direct, &[&guard_warning(2, "load", "GET /desk")]);
    // A server fn that checks who calls it, through a helper: nothing to say.
    let checks = format!(
        "{GUARD}server fn load() -> list = session().name != null ? [] : []\ncomponent L() {{\n    state xs = load()\n    <p>{{len(xs ?? [])}}</p>\n}}\nfn home() = <html><body><L/></body></html>\nroutes /desk | agent {{\n    route GET / = home()\n}}\n"
    );
    says(&checks, &[]);
    // No group: no guard to be misread.
    let open = "server fn load() -> list = []\ncomponent L() {\n    state xs = load()\n    <p>{len(xs ?? [])}</p>\n}\nfn home() = <html><body><L/></body></html>\nroute GET / = home()\n";
    says(open, &[]);
}

// (FEEDBACK79.md, Q-1258; branch feedback72-81)
#[test]
fn q1258_the_middleware_is_not_the_page() {
    // The guard's own helper reaches a server fn: that runs before the page, for every request,
    // and is not said as the page's.
    let src = "server fn audit() -> int = 1\nfn note() = audit()\nfn agent(r) = note() > 0 ? r : reply 303 redirect(\"/\")\nroutes /desk | agent {\n    route GET / = <html><body>desk</body></html>\n}\n";
    says(src, &[]);
}

// (FEEDBACK79.md, Q-1258; branch feedback72-81)
#[test]
fn q1258_a_server_fn_is_not_read_through() {
    // `outer` is the page's; `inner` runs on the server, inside `outer`, and is not said.
    let src = format!(
        "{GUARD}server fn inner() -> int = 1\nserver fn outer() -> int = inner() + 1\nfn home() = <html><body><p>{{outer()}}</p></body></html>\nroutes /desk | agent {{\n    route GET / = home()\n    route GET /again = home()\n}}\n"
    );
    says(&src, &[&guard_warning(3, "outer", "GET /desk")]);
}

// (FEEDBACK79.md, Q-1258, found in review; branch feedback72-81)
#[test]
fn q1258_a_name_bound_there_is_no_call_and_a_lambda_page_is_followed() {
    // `todos` is the view's parameter, not `server fn todos()`; the page is a top-level lambda.
    let src = format!(
        "{GUARD}server fn todos() -> list = []\nserver fn load_tickets() -> list = [\"printer jam\"]\nview TodoList(todos: list) = <ul>{{for t in todos => <li>{{t}}</li>}}</ul>\ncomponent TicketList() {{\n    state ts = load_tickets()\n    <ul>{{for t in ts ?? [] => <li>{{t}}</li>}}</ul>\n}}\nlet home = () => <html><body><TicketList/><TodoList todos={{[]}}/></body></html>\nroutes /desk | agent {{\n    route GET / = home()\n}}\n"
    );
    says(&src, &[&guard_warning(3, "load_tickets", "GET /desk")]);
    // A route's own parameter named as a server fn.
    let src = format!("{GUARD}server fn id() -> int = 1\nfn show(id: int) = <p>{{id}}</p>\nroutes /t | agent {{\n    route GET /:id(int) = <html><body>{{show(id)}}</body></html>\n}}\n");
    says(&src, &[]);
}

// (FEEDBACK79.md, Q-1258; branch feedback72-81)
#[test]
fn q1258_unicode_names() {
    let src = format!(
        "{GUARD}server fn charger_billets() -> list = [\"café\"]\ncomponent Liste() {{\n    state ts = charger_billets()\n    <ul>{{for t in ts ?? [] => <li>{{t}}</li>}}</ul>\n}}\nfn accueil_équipe() = <html><body><Liste/></body></html>\nroutes /bureau | agent {{\n    route GET / = accueil_équipe()\n}}\n"
    );
    says(&src, &[&guard_warning(2, "charger_billets", "GET /bureau")]);
}

// ---- Q-1261: the effects a component's state initializer needs, asked of the route ------------

fn effect_src(stamp: &str, load: &str, route_with: &str, page: &str) -> String {
    format!(
        "{stamp}\n{load}\ncomponent Shows() {{\n    state v = load_it()\n    <p>{{v}}</p>\n}}\n{page}\nroute GET /{route_with} = <html><body>{{shows()}}</body></html>\n"
    )
}

// (FEEDBACK80.md, Q-1261; branch feedback72-81)
#[test]
fn q1261_the_repro_names_what_needs_the_effect() {
    let src = "fn stamp() -> str with time = str(now())\nserver fn load_it() -> str = stamp()\ncomponent Shows() {\n    state v = load_it()\n    <p>{v}</p>\n}\nroute GET / with db = <html><body><Shows/></body></html>\n";
    says(
        src,
        &["7 warning: `load_it` needs the time effect (it calls `stamp`), but `route GET /` is declared `with db`: <Shows> calls it in `state v` (line 4), which the server runs while it makes the page"],
    );
}

// (FEEDBACK80.md, Q-1261; branch feedback72-81)
#[test]
fn q1261_a_server_fn_that_declares_the_effect_is_an_error() {
    let src = effect_src("", "server fn load_it() -> str with time = str(now())", " with db", "fn shows() = <Shows/>");
    says(
        &src,
        &["8 `load_it` needs the time effect, but `route GET /` is declared `with db`: <Shows> calls it in `state v` (line 4), which the server runs while it makes the page"],
    );
}

// (FEEDBACK80.md, Q-1261; branch feedback72-81)
#[test]
fn q1261_what_is_allowed_says_nothing() {
    let stamp = "fn stamp() -> str with time = str(now())";
    let load = "server fn load_it() -> str = stamp()";
    // The route allows it, among others; one effect each side of it.
    says(&effect_src(stamp, load, " with db, time", "fn shows() = <Shows/>"), &[]);
    says(&effect_src(stamp, load, " with time", "fn shows() = <Shows/>"), &[]);
    // No `with`: anything goes.
    says(&effect_src(stamp, load, "", "fn shows() = <Shows/>"), &[]);
    // A server fn needing nothing.
    says(&effect_src("", "server fn load_it() -> str = \"x\"", " with db", "fn shows() = <Shows/>"), &[]);
}

// (FEEDBACK80.md, Q-1261; branch feedback72-81)
#[test]
fn q1261_two_effects_a_nested_component_and_a_builtin() {
    // The component inside another, reached through a helper; the server fn calls `now` and
    // `env` itself.
    let src = "server fn load_it() -> str = str(now()) + env(\"X\")\ncomponent Inner() {\n    state v = load_it()\n    <p>{v}</p>\n}\ncomponent Outer() {\n    <div><Inner/></div>\n}\nfn page() = <html><body><Outer/></body></html>\nroute GET / with db = page()\n";
    says(
        src,
        &["10 warning: `load_it` needs the env, time effects (it calls `env`, `now`), but `route GET /` is declared `with db`: <Inner> calls it in `state v` (line 3), which the server runs while it makes the page"],
    );
}

// (FEEDBACK80.md, Q-1261; branch feedback72-81)
#[test]
fn q1261_a_recursive_chain_ends() {
    let src = "fn tick(n: int) -> str with time = n > 0 ? tick(n - 1) : str(now())\nfn down(n: int) -> str = n > 0 ? down(n - 1) : tick(n)\nserver fn load_it() -> str = down(3)\ncomponent Shows() {\n    state v = load_it()\n    <p>{v}</p>\n}\nroute GET / with db = <html><body><Shows/></body></html>\n";
    says(
        src,
        &["8 warning: `load_it` needs the time effect (it calls `tick`), but `route GET /` is declared `with db`: <Shows> calls it in `state v` (line 5), which the server runs while it makes the page"],
    );
}

// (FEEDBACK80.md, Q-1261, found in review; branch feedback72-81)
#[test]
fn q1261_a_component_made_in_a_handler_runs_in_the_browser() {
    // The review's program: 0.8.9 answers GET / and the RPC, both 200; it stays accepted.
    let src = "server fn load_it() -> str with time = str(now())\ncomponent Dialog() {\n    state v = load_it()\n    <p>{v}</p>\n}\ncomponent Page() {\n    state modal = null\n    <div><button onclick={() => { modal = <Dialog/> }}>open</button>{modal}</div>\n}\nroute GET / with db = <html><body><Page/></body></html>\n";
    says(src, &[]);
    // Under a condition: a warning, though `load_it` declares the effect.
    let src = "server fn load_it() -> str with time = str(now())\ncomponent Dialog() {\n    state v = load_it()\n    <p>{v}</p>\n}\ncomponent Page(open: bool) {\n    <div>{if open { <Dialog/> }}</div>\n}\nroute GET / with db = <html><body><Page open={true}/></body></html>\n";
    says(src, &["9 warning: `load_it` needs the time effect, but `route GET /` is declared `with db`: <Dialog> calls it in `state v` (line 3), which the server runs while it makes the page"]);
}

// (FEEDBACK80.md, Q-1261, found in review; branch feedback72-81)
#[test]
fn q1261_plain_functions_markup_builtins_and_one_saying_for_many_routes() {
    let src = "server fn load_it() -> str with time = str(now())\nfn helper() = load_it()\nfn stamp() -> str with time = str(now())\ncomponent A() {\n    state v = helper()\n    <p>{v}</p>\n}\ncomponent B() {\n    state v = stamp()\n    <p>{v}</p>\n}\ncomponent C() {\n    <p>{stamp()} {now()}</p>\n}\nroute GET /a with db = <html><body><A/></body></html>\nroute GET /b with db = <html><body><B/></body></html>\nroute GET /c with db = <html><body><C/></body></html>\nroute GET /c2 with db = <html><body><C/></body></html>\n";
    says(
        src,
        &[
            "15 warning: `helper` needs the time effect (it calls `load_it`), but `route GET /a` is declared `with db`: <A> calls it in `state v` (line 5), which the server runs while it makes the page",
            "16 `stamp` needs the time effect, but `route GET /b` is declared `with db`, as are 2 other routes whose pages run it: <B> calls it in `state v` (line 9), which the server runs while it makes the page",
            "17 `now` needs the time effect, but `route GET /c` is declared `with db`, as is 1 other route whose page runs it: <C> calls it as it renders (line 13), which the server runs while it makes the page",
        ],
    );
}

// (FEEDBACK80.md, Q-1261; branch feedback72-81)
#[test]
fn q1261_a_component_in_another_file_is_named_there() {
    let app = App::new(
        "q1261",
        &[
            ("main.qwe", "use \"parts.qwe\"\nroute GET / with db = <html><body><Shows/></body></html>\n"),
            ("parts.qwe", "fn stamp() -> str with time = str(now())\nserver fn load_it() -> str = stamp()\ncomponent Shows() {\n    state v = load_it()\n    <p>{v}</p>\n}\n"),
        ],
    );
    assert_eq!(
        app.check(),
        ["main.qwe:2 warning: `load_it` needs the time effect (it calls `stamp`), but `route GET /` is declared `with db`: <Shows> calls it in `state v` (parts.qwe:4), which the server runs while it makes the page"]
    );
}

// ---- Q-1267: a dropped new value, wherever it is dropped ---------------------------------------

const DROPPED_PUSH: &str = "`push` gives a new value and changes nothing, and it is dropped here: write `items = items | push(...)`";

// (FEEDBACK80.md, Q-1267; branch feedback72-81)
#[test]
fn q1267_the_repro_warns_in_each_handler() {
    // Line 22's `xs` has no type: a map of functions or a page object may hold its own `push`
    // (decided in review), and it is let be.
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/feedback/repros/Q-1267-dropped-push-in-a-fn.qwe")).unwrap();
    let got = diags(&src);
    let lines: Vec<&str> = got.iter().map(|d| d.split(' ').next().unwrap()).collect();
    assert_eq!(lines, ["8", "9", "10", "16"], "{:#?}", got);
    assert!(got.iter().all(|d| d.ends_with(DROPPED_PUSH)), "{:#?}", got);
}

// (FEEDBACK80.md, Q-1267; branch feedback72-81)
#[test]
fn q1267_a_value_used_is_no_value_dropped() {
    // The function's value, returned, assigned, kept in a `let`, passed on, in each spelling.
    for body in [
        "xs.push(1)",
        "push(xs, 1)",
        "xs | push(1)",
        "return xs.push(1)",
        "let ys = xs.push(1)\n    ys",
        "xs = xs.push(1)\n    xs",
        "print(xs.push(1))\n    xs",
        "if len(xs) > 0 { xs.push(1) } else { xs }",
        "match len(xs) { 0 => xs | push(1), _ => xs }",
    ] {
        let src = format!("fn add(xs: list) {{\n    {}\n}}\nprint(add([]))\n", body);
        says(&src, &[]);
    }
    // A local fn not named by any handler gives its value to whoever calls it.
    says(
        "component G() {\n    state items = [\"milk\"]\n    fn more() { items | push(\"eggs\") }\n    <button onclick={() => { items = more() }}>{len(items)}</button>\n}\nroute GET / = <html><body><G/></body></html>\n",
        &[],
    );
}

// (FEEDBACK80.md, Q-1267; branch feedback72-81)
#[test]
fn q1267_the_ends_of_a_statement_dropped() {
    let w = |line: u32, x: &str| format!("{} warning: `push` gives a new value and changes nothing, and it is dropped here: write `{} = {} | push(...)`", line, x, x);
    // An `if`'s branch, an `else`, a `match` arm, a `for` body and a `while` body.
    says("fn f(xs: list) {\n    if len(xs) > 0 { xs.push(1) }\n    xs\n}\nprint(f([]))\n", &[&w(2, "xs")]);
    says("fn f(xs: list) {\n    if len(xs) > 0 { print(1) } else { xs.push(1) }\n    xs\n}\nprint(f([]))\n", &[&w(2, "xs")]);
    says("fn f(xs: list) {\n    match len(xs) { 0 => xs.push(1), _ => print(2) }\n    xs\n}\nprint(f([]))\n", &[&w(2, "xs")]);
    says("fn f(xs: list) {\n    let out = []\n    for x in xs { out.push(x) }\n    out\n}\nprint(f([]))\n", &[&w(3, "out")]);
    says("fn f(xs: list) {\n    let out = []\n    let i = 0\n    while i < len(xs) {\n        i += 1\n        out.push(i)\n    }\n    out\n}\nprint(f([]))\n", &[&w(6, "out")]);
    // At the top level too, as a script writes it.
    says("let out = []\nfor x in [1, 2] { out | push(x) }\nprint(out)\n", &[&w(2, "out")]);
}

// (FEEDBACK80.md, Q-1267; branch feedback72-81)
#[test]
fn q1267_deeply_nested_ends_are_followed() {
    // 120 `if`s, one in another, as a statement: the innermost `push` is dropped.
    let mut inner = "xs.push(1)".to_string();
    for _ in 0..120 {
        inner = format!("if true {{ {} }}", inner);
    }
    let src = format!("fn f(xs: list) {{\n    {}\n    xs\n}}\nprint(f([]))\n", inner);
    let got = about(&src, "`push` gives a new value");
    assert_eq!(got.len(), 1, "{:?}", got);
}

// (FEEDBACK80.md, Q-1267; branch feedback72-81)
#[test]
fn q1267_a_receiver_of_no_known_type_is_let_be() {
    // Typed, it is said; untyped, it may be a map holding its own `push` (found in review).
    says("fn add(xs: list) {\n    xs.push(1)\n    xs\n}\nprint(add([]))\n", &["2 warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`"]);
    says("fn go(n: int) = n\nfn add(stack) {\n    stack.push(1)\n    stack\n}\nprint(add({ push: go }))\n", &[]);
    for f in ["remove()", "set(\"a\", 1)", "append(1)", "sort()"] {
        says(&format!("fn paint(el) {{\n    el.{}\n    el\n}}\nprint(paint(1))\n", f), &[]);
    }
}

// (FEEDBACK80.md, Q-1267, found in review; branch feedback72-81)
#[test]
fn q1267_each_end_is_read_in_its_own_scope() {
    // The block's own `let`s and the loop's own name, not the outer ones of the same names.
    says(
        "let row = [1, 2]\nfn f(xs: list, c: bool, rows: list) {\n    if c { let xs = js(\"document.body\"); xs.remove() }\n    if c { let ys = [1]; ys.push(2) }\n    for row in rows { row.remove() }\n    xs\n}\nprint(f([], true, []))\nprint(row)\n",
        &["4 warning: `push` gives a new value and changes nothing, and it is dropped here: write `ys = ys | push(...)`"],
    );
}

// (FEEDBACK80.md, Q-1267; branch feedback72-81)
#[test]
fn q1267_a_handler_by_name() {
    // The component's own `let` holding a lambda, and a `fn` whose last line ends in an `if`.
    let src = "component G() {\n    state items = []\n    let a = () => { items.push(1) }\n    fn b() {\n        print(1)\n        if len(items) < 3 { push(items, 2) }\n    }\n    <div><button onclick={a}>a</button><button @click={b}>b</button>{len(items)}</div>\n}\nroute GET / = <html><body><G/></body></html>\n";
    says(
        src,
        &[
            "3 warning: `push` gives a new value and changes nothing, and it is dropped here: write `items = items | push(...)`",
            "6 warning: `push` gives a new value and changes nothing, and it is dropped here: write `items = items | push(...)`",
        ],
    );
    // A handler that assigns: as it should be.
    says(
        "component G() {\n    state items = []\n    fn a() { items = items | push(1) }\n    <button onclick={a}>{len(items)}</button>\n}\nroute GET / = <html><body><G/></body></html>\n",
        &[],
    );
}

// ---- Q-1268: a plain `let` a handler changes and the markup shows ------------------------------

fn not_state(line: u32, name: &str, init: &str) -> String {
    format!("{} warning: `{}` is changed in a handler but isn't a state, so the page won't show it: write `state {} = {}`", line, name, name, init)
}

// (FEEDBACK80.md, Q-1268; branch feedback72-81)
#[test]
fn q1268_the_repro() {
    says(
        "component Clicks() {\n    let clicks = 0\n    <button onclick={() => { clicks += 1 }}>{clicks}</button>\n}\nroute GET / = <html><body><Clicks/></body></html>\n",
        &[&not_state(2, "clicks", "0")],
    );
}

// (FEEDBACK80.md, Q-1268; branch feedback72-81)
#[test]
fn q1268_through_a_named_handler_a_function_it_calls_and_a_timer() {
    says(
        "component Clicks() {\n    let clicks = 0\n    fn bump() { clicks = clicks + 1 }\n    fn inc() { bump() }\n    <button onclick={inc}>{clicks}</button>\n}\nroute GET / = <html><body><Clicks/></body></html>\n",
        &[&not_state(2, "clicks", "0")],
    );
    says(
        "component Ticks() {\n    let ticks = 0\n    every(1000, () => { ticks += 1 })\n    <p title={str(ticks)}>tick</p>\n}\nroute GET / = <html><body><Ticks/></body></html>\n",
        &[&not_state(2, "ticks", "0")],
    );
    // A name of more than one byte a letter, and a value `written` does not show.
    says(
        "component Compte() {\n    let comptés = [1, 2]\n    <button onclick={() => { comptés = [] }}>{len(comptés)}</button>\n}\nroute GET / = <html><body><Compte/></body></html>\n",
        &[&not_state(2, "comptés", "...")],
    );
}

// (FEEDBACK80.md, Q-1268, found in review; branch feedback72-81)
#[test]
fn q1268_a_callback_given_to_a_child() {
    says(
        "component Child(onpick) {\n    <button onclick={() => onpick(1)}>pick</button>\n}\ncomponent P() {\n    let picked = 0\n    fn handler(n) { picked = n }\n    <div><Child onpick={handler}/><p>{picked}</p></div>\n}\nroute GET / = <html><body><P/></body></html>\n",
        &[&not_state(5, "picked", "0")],
    );
}

// (FEEDBACK80.md, Q-1268; branch feedback72-81)
#[test]
fn q1268_what_is_let_be() {
    // Changed and not shown; shown and not changed; a state; a `let` of the handler's own.
    for src in [
        "component C() {\n    let clicks = 0\n    <button onclick={() => { clicks += 1; print(clicks) }}>go</button>\n}\n",
        "component C() {\n    let clicks = 0\n    <button onclick={() => { print(clicks) }}>{clicks}</button>\n}\n",
        "component C() {\n    state clicks = 0\n    <button onclick={() => { clicks += 1 }}>{clicks}</button>\n}\n",
        "component C() {\n    let clicks = 0\n    <button onclick={() => { let clicks = 5\n        clicks += 1\n        print(clicks) }}>{clicks}</button>\n}\n",
        "component C(tabs: list) {\n    let i = 0\n    <div><button onclick={() => { i += 1 }}>next</button><ul>{for i in tabs => <li>{i}</li>}</ul></div>\n}\n",
        "component C() {\n    let clicks = 0\n    <button onclick={(clicks) => { clicks += 1 }}>{clicks}</button>\n}\n",
    ] {
        let src = format!("{}route GET / = <html><body><C/></body></html>\n", src);
        assert_eq!(about(&src, "isn't a state"), Vec::<String>::new(), "\n{}", src);
    }
}

// ---- Q-1272: an error value shown in markup ----------------------------------------------------

fn shows_error(line: u32, what: &str, test: &str) -> String {
    format!("{} warning: {} can be an error value here, and the page would show nothing for it: give a fallback with `??`, or test `{}`", line, what, test)
}

const NEWS: &str = "server fn load_news() -> str ! str = err(\"503 Service Unavailable\")\n";

fn news(body: &str) -> String {
    format!("{NEWS}component News() {{\n    state news = load_news()\n{body}\n}}\nroute GET / = <html><body><News/></body></html>\n")
}

// (FEEDBACK81.md, Q-1272; branch feedback72-81)
#[test]
fn q1272_the_repro() {
    says(&news("    <p>News: {news}</p>"), &[&shows_error(4, "`news`", "is_err(news)")]);
}

// (FEEDBACK81.md, Q-1272; branch feedback72-81)
#[test]
fn q1272_a_value_tested_or_given_a_fallback() {
    let warned = |body: &str| about(&news(body), "can be an error value").len();
    // A fallback.
    assert_eq!(warned("    <p>News: {news ?? \"none\"}</p>"), 0);
    // Tested before: `is_err` false takes the error out of `news`'s type; the same markup
    // untested is said (the pair shows it is the test that counts).
    assert_eq!(warned("    if is_err(news) {\n        return <p>unavailable</p>\n    }\n    <p>News: {news}</p>"), 0);
    assert_eq!(warned("    if len(\"x\") == 0 {\n        return <p>unavailable</p>\n    }\n    <p>News: {news}</p>"), 1);
    assert_eq!(warned("    <p>{if !is_err(news) { <b>{news}</b> } else { \"later\" }}</p>"), 0);
    assert_eq!(warned("    <p>{if len(\"x\") > 0 { <b>{news}</b> } else { \"later\" }}</p>"), 1);
    // A server fn that cannot fail, and a value of any type.
    says("server fn load() -> str = \"ok\"\ncomponent N() {\n    state v = load()\n    <p>{v}</p>\n}\nroute GET / = <html><body><N/></body></html>\n", &[]);
    says("fn any_of(x) = x\ncomponent N() {\n    <p>{any_of(1)}</p>\n}\nroute GET / = <html><body><N/></body></html>\n", &[]);
}

// (FEEDBACK81.md, Q-1272; branch feedback72-81)
#[test]
fn q1272_a_let_set_from_a_call_that_can_fail() {
    says(
        "fn parse(s: str) -> int ! str = s == \"\" ? err(\"empty\") : len(s)\ncomponent N(s: str) {\n    let n = parse(s)\n    let m = parse(s) ?? 0\n    <p>{n} {m} {parse(s)}</p>\n}\nroute GET / = <html><body><N s=\"1\"/></body></html>\n",
        &[&shows_error(5, "`n`", "is_err(n)"), &shows_error(5, "`parse(...)`", "is_err(...)")],
    );
}

// (FEEDBACK81.md, Q-1272, decided in review; branch feedback72-81)
#[test]
fn q1272_a_builtins_error_is_left_to_the_page() {
    says("route GET /:n = <html><body><p>{int(n)}</p><p>{dec(n)}</p></body></html>\n", &[]);
    says("component N(s: str) {\n    let n = int(s)\n    <p>{n}</p>\n}\nroute GET / = <html><body><N s=\"1\"/></body></html>\n", &[]);
}

// (FEEDBACK81.md, Q-1272, found in review; branch feedback72-81)
#[test]
fn q1272_narrowing_outside_markup() {
    // After `if is_err(r) { return ... }`, `r` holds no error: what the checker says of it changes.
    let tested = "fn save(n: int) -> str ! str { if n < 0 { return err(\"neg\") }; \"x\" }\nfn f() {\n    let r = save(1)\n    if is_err(r) { return 0 }\n    len(r)\n}\nprint(f())\n";
    let untested = tested.replace("    if is_err(r) { return 0 }\n", "");
    let r = |src: &str| -> Vec<String> { check_source(src, "t.qwe", Path::new(".")).locals.iter().filter(|l| l.name == "r").map(|l| l.detail.clone()).collect() };
    assert_eq!(r(tested), ["let r: str"]);
    assert!(r(&untested).iter().all(|d| d.contains("error")), "{:?}", r(&untested));
    assert_eq!(diags(tested), Vec::<String>::new());
}

// ---- Q-1018: an error dropped as the value of an `if`, or as a handler's ----------------------

fn error_dropped(line: u32, f: &str) -> String {
    format!("{} warning: `{}` can give an error, and it is dropped here: write `{}(...)?` to pass it on, or `let _ = {}(...)` if it does not matter", line, f, f, f)
}

const SAVE: &str = "fn save(n: int) -> int ! str { if n < 0 { return err(\"negative\") }; n }\n";

// (FEEDBACK72.md, Q-1018; branch feedback72-81)
#[test]
fn q1018_the_repro_warns_three_times() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/feedback/repros/Q-1018-an-error-dropped-in-an-if-gets-no-warning.qwe")).unwrap();
    says(&src, &[&error_dropped(13, "save"), &error_dropped(18, "save"), &error_dropped(23, "save")]);
}

// (FEEDBACK72.md, Q-1018; branch feedback72-81)
#[test]
fn q1018_an_error_taken_is_not_dropped() {
    for body in [
        "    if n != 0 { save(n) } else { 0 }",
        "    let r = if n != 0 { save(n) } else { 0 }\n    r",
        "    if n != 0 { let _ = save(n) }\n    1",
        "    if n != 0 { save(n) ?? 0 }\n    1",
        "    match n { 0 => 1, _ => save(n) }",
    ] {
        let src = format!("{SAVE}fn b(n: int) {{\n{}\n}}\nprint(b(1))\n", body);
        says(&src, &[]);
    }
}

// (FEEDBACK72.md, Q-1018; branch feedback72-81)
#[test]
fn q1018_a_match_arm_an_else_and_a_handler() {
    says(&format!("{SAVE}fn b(n: int) {{\n    match n {{ 0 => print(0), _ => save(n) }}\n    1\n}}\nprint(b(1))\n"), &[&error_dropped(3, "save")]);
    says(&format!("{SAVE}fn b(n: int) {{\n    if n == 0 {{ print(0) }} else {{ save(n) }}\n    1\n}}\nprint(b(1))\n"), &[&error_dropped(3, "save")]);
    // A handler's value goes nowhere: written there, or by name, said at the handler with what a
    // handler can do, not `?`.
    says(
        &format!("{SAVE}component C() {{\n    fn go() {{ save(1) }}\n    <div><button onclick={{() => save(-1)}}>a</button><button onclick={{go}}>b</button></div>\n}}\nroute GET / = <html><body><C/></body></html>\n"),
        &[
            "4 warning: `save` can give an error, and this handler drops it: give a value with `save(...) ?? ...`, test it with `is_err`, or write `let _ = save(...)` if it does not matter",
            "4 warning: `onclick={go}` drops the error `go` can give: handle it in `go`, or write `onclick={() => { go() ?? ... }}`",
        ],
    );
}

// (FEEDBACK72.md, Q-1018, found in review; branch feedback72-81)
#[test]
fn q1018_a_named_handler_used_elsewhere_too() {
    // `go` gives its error to `api` too: nothing is said in `go`, and `go2`'s `return err(...)`
    // counts as well.
    let src = format!("{SAVE}fn go() -> int ! str {{\n    print(1)\n    save(1)\n}}\nfn go2() {{\n    return err(\"no\")\n}}\nfn api() -> int ! str = go()? + 1\ncomponent C() {{\n    <div><button onclick={{go}}>a</button><button @click={{go2}}>b</button></div>\n}}\nroute GET / = <html><body><C/></body></html>\nprint(api())\n");
    says(
        &src,
        &[
            "11 warning: `onclick={go}` drops the error `go` can give: handle it in `go`, or write `onclick={() => { go() ?? ... }}`",
            "11 warning: `onclick={go2}` drops the error `go2` can give: handle it in `go2`, or write `onclick={() => { go2() ?? ... }}`",
        ],
    );
}

// (FEEDBACK80.md, Q-1267, found in review; branch feedback72-81)
#[test]
fn q1018_the_line_after_a_named_handler() {
    // What is said after a handler is said at the element, in its file, not at `go`'s last line.
    let src = format!("view V() = <b>v</b>\n{SAVE}fn go() {{\n    print(1)\n    save(1)\n}}\ncomponent C() {{\n    <div><V @click={{go}}/></div>\n}}\nroute GET / = <html><body><C/></body></html>\n");
    says(&src, &["8 <V> is a view; `@click` handlers go on HTML elements"]);
    let app = App::new(
        "handler-line",
        &[
            ("main.qwe", "use \"parts.qwe\"\ncomponent C() {\n    <div>\n        <button onclick={go}>b</button>\n        <V @click={go}/>\n    </div>\n}\nroute GET / = <html><body><C/></body></html>\n"),
            ("parts.qwe", "let xs = [1]\nview V() = <b>v</b>\nfn go() {\n    print(1)\n    xs | push(2)\n}\n"),
        ],
    );
    assert_eq!(
        app.check(),
        [
            "main.qwe:5 <V> is a view; `@click` handlers go on HTML elements",
            "parts.qwe:5 warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`",
        ]
    );
}

