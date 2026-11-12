//! FEEDBACK61.md: what complex layout and re-rendering found.

mod support;

use quillo::check::{check_file_opts, Options};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::temp_dir;

/// A program of one file, removed when the test ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, src: &str) -> App {
        let d = temp_dir(&format!("feedback61-{}", name));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        App(d)
    }

    /// The classes `quillo check` calls unused, sorted.
    fn unused(&self) -> Vec<String> {
        let opts = Options { css: true, ..Default::default() };
        let mut out: Vec<String> = check_file_opts(&self.0.join("main.qwe"), Default::default(), opts)
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("nothing in the program writes it"))
            .map(|d| d.message.split('`').nth(1).unwrap().trim_start_matches('.').to_string())
            .collect();
        out.sort();
        out
    }

    /// What `quillo classes` prints, one class a line.
    fn listed(&self) -> Vec<String> {
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("classes").arg(self.0.join("main.qwe")).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A component with these rules and this markup.
fn page(rules: &[&str], markup: &str) -> String {
    let sheet: Vec<String> = rules.iter().map(|r| format!(".{} {{ color: red }}", r)).collect();
    format!(
        "component Part(on: bool, n: list) {{\n    style css`\n{}\n    `\n    <div>{}</div>\n}}\nroute GET / = <html><body><Part on={{true}} n={{[1]}}/></body></html>\n",
        sheet.join("\n"),
        markup
    )
}

#[test]
fn a_class_map_writes_its_keys() {
    let app = App::new("map", &page(&["tab", "active"], "<p class={{ tab: true, active: on }}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
    assert_eq!(app.listed(), ["active", "tab"]);
}

#[test]
fn a_key_counts_whatever_its_value_says() {
    // `false` today may be a variable tomorrow: the rule is not dead because of what it holds now.
    let app = App::new("false", &page(&["off"], "<p class={{ off: false }}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
}

#[test]
fn a_quoted_key_with_a_dash_a_space_or_an_accent_is_written() {
    let app = App::new(
        "quoted",
        &page(&["is-on", "two", "words", "café"], "<p class={{ \"is-on\": on, \"two words\": on, \"café\": on }}>x</p>"),
    );
    assert_eq!(app.unused(), Vec::<String>::new());
    assert_eq!(app.listed(), ["café", "is-on", "two", "words"]);
}

#[test]
fn a_map_in_a_list_or_a_choice_is_read() {
    let app = App::new(
        "nested",
        &page(
            &["inlist", "deep", "yes", "no"],
            "<p class={[\"a\", { inlist: on }, [{ deep: on }]]}>x</p><p class={on ? { yes: true } : { no: true }}>y</p>",
        ),
    );
    assert_eq!(app.unused(), ["a"; 0]);
    assert_eq!(app.listed(), ["a", "deep", "inlist", "no", "yes"]);
}

#[test]
fn a_toggle_writes_its_class() {
    let app = App::new("toggle", &page(&["toggled"], "<p class:toggled={on}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
}

#[test]
fn a_map_is_read_on_a_child_and_in_a_loop() {
    let app = App::new(
        "where",
        &page(&["child", "row"], "<section><p class={{ child: on }}>x</p></section>{for i in n => <p class={{ row: on }}>{i}</p>}"),
    );
    assert_eq!(app.unused(), Vec::<String>::new());
    assert_eq!(app.listed(), ["child", "row"]);
}

#[test]
fn class_name_takes_a_map_as_class_does() {
    let app = App::new("classname", &page(&["named"], "<p className={{ named: on }}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
}

#[test]
fn a_dead_rule_is_still_told() {
    let app = App::new("dead", &page(&["live", "gone"], "<p class={{ live: on }}>x</p>"));
    assert_eq!(app.unused(), ["gone"]);
}

#[test]
fn an_empty_map_writes_nothing() {
    let app = App::new("empty", &page(&["gone"], "<p class={{}}>x</p>"));
    assert_eq!(app.unused(), ["gone"]);
    assert_eq!(app.listed(), Vec::<String>::new());
}

#[test]
fn a_map_that_is_no_class_writes_nothing() {
    // Its keys are fields or properties: `.title` here is dead however many records have a title.
    let src = page(&["title", "color"], "<p style={{ color: \"red\" }}>x</p>");
    let app = App::new("record", &format!("let post = {{ title: 1 }}\nprint(post)\n{}", src));
    assert_eq!(app.unused(), ["color", "title"]);
}

#[test]
fn the_forms_that_worked_still_work() {
    let app = App::new(
        "plain",
        &page(&["short", "said", "built-big"], "<p.short class=\"said\">x</p><p class={`built-${on ? \"big\" : \"small\"}`}>y</p>"),
    );
    assert_eq!(app.unused(), Vec::<String>::new());
}

// ---- Rows in a real browser (Q-889, Q-894) ---------------------------------------------------
// Each program is served from a folder of its own, and a script run in headless Chrome (as
// tests/browser.rs does, through tests/browser/run.js) clicks and reads the page. A path starting
// `/b` shows the component only once `#show` is clicked, so its rows are made in the browser; the
// others take up the server's page.

/// The quillo that serves these pages: this build's, or the one QUILLO_TEST_BIN names (main's,
/// say, to see a test fail there).
fn quillo() -> PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

fn chrome() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output();
    if !node.map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// Fails unless `quillo check` passes the program in `dir`, under a deadline.
fn checked(dir: &Path) {
    let mut child = Command::new(quillo()).arg("check").arg(dir.join("main.qwe")).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the check did not end");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// The page script around `body`: once the runtime has started (and `#show` was clicked, at
/// `/b...`), `body` runs with `$`, `tick` and `click`; what it returns is the result, with the
/// errors the runtime reported and how many components it made again.
fn script(body: &str) -> String {
    format!(
        r#"(async () => {{
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {{}}).booted; i++) await tick(30);
  const QW = window[Symbol.for("quillo")];
  const $ = (id) => document.getElementById(id);
  const click = async (...ids) => {{ for (const id of ids) {{ $(id).click(); await tick(30); }} }};
  if (location.pathname.startsWith("/b")) await click("show");
  let out;
  try {{ out = await (async () => {{
{body}
  }})(); }} catch (e) {{ out = {{ exception: String((e && e.stack) || e) }}; }}
  out.remade = QW.remade ?? 0;
  out.errors = window.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
}})();"#
    )
}

/// `src` served, and `body` run at each of `paths`: what each gave back. None without Chrome.
fn in_pages(name: &str, src: &str, paths: &[&str], body: &str) -> Option<Vec<serde_json::Value>> {
    in_pages_with(name, src, paths, body, false)
}

/// The same; with `focus`, the page has the focus, as in a window, and hears `blur` and `focusout`
/// (QUILLO_PAGE_FOCUS in tests/browser/run.js).
fn in_pages_with(name: &str, src: &str, paths: &[&str], body: &str, focus: bool) -> Option<Vec<serde_json::Value>> {
    let app = App::new(name, src);
    checked(&app.0);
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return None;
    };
    // One Chrome at a time: several at once overload small machines.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let port = support::free_port();
    let _server = support::Guard::new(
        Command::new(quillo())
            .arg("run")
            .arg(app.0.join("main.qwe"))
            .args(["--port", &port.to_string()])
            .current_dir(&app.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::write(app.0.join("t.js"), script(body)).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let mut got = Vec::new();
    for path in paths {
        // run.js ends itself 10 s after its own deadline, whatever Chrome does.
        let mut node = Command::new("node");
        if focus {
            node.env("QUILLO_PAGE_FOCUS", "1");
        } else {
            node.env_remove("QUILLO_PAGE_FOCUS");
        }
        let out = node
            .arg(&run)
            .arg(&chrome)
            .arg(format!("http://127.0.0.1:{}{}", port, path))
            .arg(app.0.join("profile"))
            .arg("60000")
            .arg(app.0.join("t.js"))
            .output()
            .expect("node");
        assert!(out.status.success(), "{}: {}", path, String::from_utf8_lossy(&out.stderr));
        got.push(serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout))));
    }
    Some(got)
}

const BOTH: &[&str] = &["/h", "/b"];

/// The report's list: rows whose root is a `? :`, a host that shows it at once or on `#show`.
const LIST: &str = r###"fn start() = [{ id: 1, done: false }, { id: 2, done: false }, { id: 3, done: false }]
component List() {
    state xs = start()
    state hits = 0
    <div>
        <ul #l>{for x in xs => x.done ? <li key={x.id} class="done" @click={() => { hits += 100 }}>done {x.id}</li> : <li key={x.id} class="todo" @click={() => { hits += 1 }}>todo {x.id}</li>}</ul>
        <p #n>{len(xs)} items</p><p #hits>{hits}</p>
        <button #mark @click={() => { xs = xs | map((i) => i.id == 2 ? merge(i, { done: !i.done }) : i) }}>mark</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #ins @click={() => { xs = [xs[0], { id: 9, done: false }] + slice(xs, 1) }}>ins</button>
        <button #pre @click={() => { xs = [{ id: 8, done: false }] + xs }}>pre</button>
        <button #clear @click={() => { xs = [] }}>clear</button>
        <button #fill @click={() => { xs = start() }}>fill</button>
        <button #one @click={() => { xs = [{ id: 2, done: false }] }}>one</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <List/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

const LIST_TEXT: &str = r#"const lis = () => [...$("l").children].map((e) => e.textContent).join(",") + " | " + $("n").textContent;"#;

// Q-889: a row kept the nodes it was made with. Once a branch at its root had replaced its
// element, a move brought the old one back next to the new (mark, rev), and the place to put a
// new row before was a node no longer on the page, which threw (mark, ins). Each row now names
// its branch's marker, which gives the nodes the branch has now.
#[test]
fn a_row_whose_root_changed_moves_as_it_is_now() {
    let body = format!(
        r#"{LIST_TEXT}
    const out = {{}};
    await click("mark", "rev"); out.markRev = lis();
    await click("rev", "mark"); out.back = lis();
    await click("clear", "fill", "mark", "ins"); out.markIns = lis();
    return out;"#
    );
    let Some(rs) = in_pages("q889-list", LIST, BOTH, &body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "markRev": "todo 3,done 2,todo 1 | 3 items",
            "back": "todo 1,todo 2,todo 3 | 3 items",
            "markIns": "todo 1,todo 9,done 2,todo 3 | 4 items",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889: the ghost was alive: clicking it ran the old branch's handler (103 for one click on each
// of the four rows shown). Three rows now, each running its own branch's handler once.
#[test]
fn a_row_whose_root_changed_leaves_no_old_element_alive() {
    let body = r#"await click("mark", "rev");
    for (const li of [...$("l").children]) { li.click(); await tick(10); }
    await tick(30);
    return { rows: [...$("l").children].map((e) => `${e.className}:${e.textContent}`).join(","), hits: $("hits").textContent };"#;
    let Some(rs) = in_pages("q889-ghost", LIST, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({ "rows": "todo:todo 3,done:done 2,todo:todo 1", "hits": "102", "remade": 0, "errors": [] });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889's edges: a list of branch rows emptied (and emptied again) and filled; one row, with a
// row put in before it after its root changed, and then gone; a branch flipped an odd and an even
// number of times before the move.
#[test]
fn branch_rows_at_the_edges_empty_one_and_flipped_back() {
    let body = format!(
        r#"{LIST_TEXT}
    const out = {{}};
    await click("mark", "clear"); out.cleared = lis();
    await click("clear"); out.again = lis();
    await click("fill", "mark", "rev"); out.filled = lis();
    await click("one", "mark", "pre"); out.onePre = lis();
    await click("mark", "rev"); out.oneRev = lis();
    await click("one", "mark", "clear"); out.oneGone = lis();
    await click("fill", "mark", "mark", "mark", "rev"); out.odd = lis();
    await click("mark", "mark", "rev"); out.even = lis();
    return out;"#
    );
    let Some(rs) = in_pages("q889-edges", LIST, BOTH, &body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "cleared": " | 0 items", "again": " | 0 items", "filled": "todo 3,done 2,todo 1 | 3 items",
            "onePre": "todo 8,done 2 | 2 items", "oneRev": "todo 2,todo 8 | 2 items", "oneGone": " | 0 items",
            "odd": "todo 3,done 2,todo 1 | 3 items", "even": "todo 1,done 2,todo 3 | 3 items",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889: rows that are lists of their own, a view giving one (`#g`) and a `for` as the row
// (`#r`): a group emptied, then the list reversed, lost the next group's rows, or brought the
// emptied rows back. The edge: a group that starts empty, fills, and empties again.
#[test]
fn rows_that_are_lists_keep_their_own_rows() {
    let src = r###"view Group(g) = {for x in g.xs => <li key={x}>{x}</li>}
fn sets(n) = [
    [{ id: 1, xs: ["a", "b"] }, { id: 2, xs: ["c"] }],
    [{ id: 1, xs: ["a", "b"] }, { id: 2, xs: ["c"] }, { id: 3, xs: ["d"] }],
    [{ id: 1, xs: ["a", "b"] }, { id: 2, xs: ["c"] }, { id: 3, xs: ["d", "e"] }],
    [{ id: 1, xs: [] }, { id: 2, xs: ["c"] }]
][n]
fn set_xs(gs, id, xs) = gs | map((g) => g.id == id ? merge(g, { xs: xs }) : g)
component Groups() {
    state gs = sets(0)
    <div>
        <ul #g>{for g in gs => <Group key={g.id} g={g}/>}</ul>
        <ul #r>{for g in gs => {for x in g.xs => <li key={x}>{x}</li>}}</ul>
        <button #empty @click={() => { gs = set_xs(gs, 1, []) }}>empty</button>
        <button #fill @click={() => { gs = set_xs(gs, 1, ["a", "b"]) }}>fill</button>
        <button #grow @click={() => { gs = set_xs(gs, 2, ["c", "c2", "c3"]) }}>grow</button>
        <button #rev @click={() => { gs = reverse(gs) }}>rev</button>
        <button #s1 @click={() => { gs = sets(1) }}>1</button>
        <button #s2 @click={() => { gs = sets(2) }}>2</button>
        <button #s3 @click={() => { gs = sets(3) }}>3</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Groups/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r#"const lis = (id) => [...$(id).children].map((e) => e.textContent).join(",");
    const both = () => { const g = lis("g"), r = lis("r"); return g === r ? g : `${g} | inline ${r}`; };
    const out = {};
    await click("empty", "rev"); out.two = both();
    await click("s1", "empty", "rev"); out.three = both();
    await click("s2", "grow", "rev", "empty", "rev"); out.grown = both();
    await click("s3", "rev"); out.emptyFirst = both();
    await click("fill", "rev"); out.filled = both();
    await click("empty", "rev"); out.emptied = both();
    return out;"#;
    let Some(rs) = in_pages("q889-groups", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "two": "c", "three": "d,c", "grown": "c,c2,c3,d,e", "emptyFirst": "c", "filled": "a,b,c", "emptied": "c",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889: a component that is a table's row, its root a branch. Taken up from the server's page,
// its island was the row's element, which the branch replaced: the old row came back in the
// wrong place, and a row deleted after its branch changed stayed on the page. Its nodes are now
// what it took up, its branch's marker among them. Made in the browser it was right, and stays so.
#[test]
fn a_table_row_component_with_a_branch_at_its_root_moves_and_goes() {
    let src = r###"fn start() = [{ id: 1, done: false }, { id: 2, done: false }, { id: 3, done: false }]
component Tr(t) {
    t.done ? <tr class="d"><td>done {t.id}</td></tr> : <tr><td>todo {t.id}</td></tr>
}
component Tbl() {
    state xs = start()
    <div>
        <table><tbody #tb>{for x in xs => <Tr key={x.id} t={x}/>}</tbody></table>
        <button #mark @click={() => { xs = xs | map((i) => i.id == 2 ? merge(i, { done: !i.done }) : i) }}>mark</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #del @click={() => { xs = xs | filter((i) => i.id != 2) }}>del</button>
        <button #pre @click={() => { xs = [{ id: 8, done: false }] + xs }}>pre</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Tbl/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r#"const rows = () => [...$("tb").querySelectorAll("tr")].map((e) => e.textContent).join(",");
    const out = {};
    await click("mark", "rev"); out.markRev = rows();
    await click("rev", "mark"); out.back = rows();
    await click("mark", "del"); out.deleted = rows();
    await click("rev"); out.rev = rows();
    await click("pre"); out.pre = rows();
    return out;"#;
    let Some(rs) = in_pages("q889-table", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "markRev": "todo 3,done 2,todo 1", "back": "todo 1,todo 2,todo 3", "deleted": "todo 1,todo 3", "rev": "todo 3,todo 1", "pre": "todo 8,todo 3,todo 1",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889 through `live()`: the server marks a row, then reverses the list; no click on the page.
#[test]
fn a_live_list_whose_rows_change_their_root() {
    let src = r###"db.open("rows.db")
db.exec("create table if not exists t (id integer primary key, pos int not null, done int not null)")
db.exec("delete from t")
db.exec("insert into t (id, pos, done) values (1, 1, 0), (2, 2, 0), (3, 3, 0)")
server fn rows() -> list = db.query("select id, done from t order by pos")
component Live() {
    state rs = live(rows)
    <div><ul #l>{for r in rs => r.done == 1 ? <li key={r.id} class="d">done {r.id}</li> : <li key={r.id}>todo {r.id}</li>}</ul><p #n>{len(rs)}</p></div>
}
route POST /go/mark = { db.exec("update t set done = 1 - done where id = 2"); "ok" }
route POST /go/flip = { db.exec("update t set pos = 4 - pos"); "ok" }
route GET / = <html><head><title>t</title></head><body><Live/></body></html>
"###;
    let body = r#"const lis = () => [...$("l").children].map((e) => e.textContent).join(",");
    const until = async (want) => { for (let i = 0; i < 100 && lis() !== want; i++) await tick(50); return lis(); };
    const out = { start: await until("todo 1,todo 2,todo 3") };
    await fetch("/go/mark", { method: "POST" }); out.marked = await until("todo 1,done 2,todo 3");
    await fetch("/go/flip", { method: "POST" }); out.flipped = await until("todo 3,done 2,todo 1");
    await fetch("/go/mark", { method: "POST" }); await until("todo 3,todo 2,todo 1");
    await fetch("/go/flip", { method: "POST" }); out.back = await until("todo 1,todo 2,todo 3");
    await tick(200);
    out.n = $("n").textContent;
    out.shown = $("l").children.length;
    return out;"#;
    let Some(rs) = in_pages("q889-live", src, &["/"], body) else { return };
    let want = serde_json::json!({
        "start": "todo 1,todo 2,todo 3", "marked": "todo 1,done 2,todo 3", "flipped": "todo 3,done 2,todo 1", "back": "todo 1,todo 2,todo 3",
        "n": "3", "shown": 3, "remade": 0, "errors": [],
    });
    assert_eq!(rs[0], want);
}

// Q-889's edges: text in several scripts, and a thousand rows whose roots change (every third,
// and the first) and which move (reversed, turned): the page shows its state after each step.
// None and one row, the same steps.
#[test]
fn a_thousand_unicode_rows_change_their_root_and_move() {
    let src = r###"fn make(n) = range(1, n + 1) | map((i) => { id: i, done: false, t: `ünï 日本 🎉 שלום ${i}` })
component Big(n: int) {
    state xs = make(n)
    <div>
        <ul #l>{for x in xs => x.done ? <li key={x.id} class="d">✓ {x.t}</li> : <li key={x.id}>{x.t}</li>}</ul>
        <button #third @click={() => { xs = xs | map((x) => x.id % 3 == 0 || x.id == 1 ? merge(x, { done: !x.done }) : x) }}>third</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #rot @click={() => { xs = drop(xs, 7) + take(xs, 7) }}>rot</button>
        <pre #truth>{json.stringify(xs)}</pre>
    </div>
}
component Host(n: int, now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Big {n}/> : null}</div>
}
route GET /h/:n = <html><head><title>t</title></head><body><Host n={int(n)} now={true}/></body></html>
route GET /b/:n = <html><head><title>t</title></head><body><Host n={int(n)} now={false}/></body></html>
"###;
    let body = r#"const shows = () => {
      const xs = JSON.parse($("truth").textContent), got = [...$("l").children].map((e) => e.textContent);
      return got.length === xs.length && xs.every((x, i) => got[i] === (x.done ? `✓ ${x.t}` : x.t));
    };
    const out = { ok: [] };
    for (const op of ["third", "rev", "third", "rot", "rev", "third"]) { await click(op); out.ok.push(shows()); }
    out.rows = $("l").children.length;
    out.first = $("l").firstElementChild ? $("l").firstElementChild.textContent : null;
    return out;"#;
    let paths = ["/h/1000", "/b/1000", "/h/1", "/b/0"];
    let Some(rs) = in_pages("q889-big", src, &paths, body) else { return };
    let six = [true; 6];
    let wants = [
        serde_json::json!({ "ok": six, "rows": 1000, "first": "ünï 日本 🎉 שלום 994", "remade": 0, "errors": [] }),
        serde_json::json!({ "ok": six, "rows": 1000, "first": "ünï 日本 🎉 שלום 994", "remade": 0, "errors": [] }),
        serde_json::json!({ "ok": six, "rows": 1, "first": "✓ ünï 日本 🎉 שלום 1", "remade": 0, "errors": [] }),
        serde_json::json!({ "ok": six, "rows": 0, "first": null, "remade": 0, "errors": [] }),
    ];
    for ((r, want), path) in rs.iter().zip(&wants).zip(paths) {
        assert_eq!(r, want, "at {}", path);
    }
}

// Q-889's cause one level down: a part kept its first nodes, and a list in it emptied its own
// record as the part ran again, so a branch switching away from a list that had grown since it
// was made left the new rows on the page, in a list's row (`#c`), outside any (`#d`), and as a
// plain function's list (`#f`, which stays right). The list's nodes stay its own until they are
// taken off the page.
#[test]
fn a_branch_that_was_a_list_takes_all_its_rows_away() {
    let src = r###"fn start() = [{ id: 1, open: true, xs: ["a", "b"] }, { id: 2, open: true, xs: ["c"] }]
fn grow(xs) = xs | map((x) => x.id == 1 ? merge(x, { xs: x.xs + [`z${len(x.xs)}`] }) : x)
fn flip(xs) = xs | map((x) => x.id == 1 ? merge(x, { open: !x.open }) : x)
fn L(x) = {for s in x.xs => <li key={s}>{s}</li>}
component Redo() {
    state xs = start()
    <div>
        <ul #c>{for x in xs => x.open ? {for s in x.xs => <li key={s}>{s}</li>} : <li key={x.id}>closed {x.id}</li>}</ul>
        <div #d>{xs[0].open ? {for s in xs[0].xs => <p key={s}>{s}</p>} : <p>closed</p>}</div>
        <ul #f>{for x in xs => <L key={x.id} x={x}/>}</ul>
        <button #grow @click={() => { xs = grow(xs) }}>grow</button>
        <button #flip @click={() => { xs = flip(xs) }}>flip</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Redo/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r#"const t = (id) => [...$(id).children].map((e) => e.textContent).join(",");
    const all = () => `${t("c")} | ${t("d")} | ${t("f")}`;
    const out = { start: all() };
    await click("grow"); out.grow = all();
    await click("flip"); out.closed = all();
    await click("flip"); out.open = all();
    await click("grow", "flip"); out.grownClosed = all();
    await click("rev"); out.rev = all();
    await click("flip"); out.reopened = all();
    await click("rev"); out.back = all();
    return out;"#;
    let Some(rs) = in_pages("q889-redo", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "start": "a,b,c | a,b | a,b,c", "grow": "a,b,z2,c | a,b,z2 | a,b,z2,c", "closed": "closed 1,c | closed | a,b,z2,c",
            "open": "a,b,z2,c | a,b,z2 | a,b,z2,c", "grownClosed": "closed 1,c | closed | a,b,z2,z3,c", "rev": "c,closed 1 | c | c,a,b,z2,z3",
            "reopened": "c,a,b,z2,z3 | c | c,a,b,z2,z3", "back": "a,b,z2,z3,c | a,b,z2,z3 | a,b,z2,z3,c",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889: the seeded walk that found its shapes, kept as its test, bounded and the same each run.
// Every shape of row whose root can change is on the page at once, over the same items: `? :`
// (`#a`), `if`/`else` (`#i`), a view (`#v`) and a plain function (`#f`) with a branch at the root,
// a view giving a list (`#g`), a `for` as the row (`#r`), a view giving two nodes with a branch
// (`#p`), a table-row component with a branch at its root (`#t`), a branch between a list and an
// element (`#w`), and single-element rows as the control (`#s`). Each step flips a row's root, changes its own list, deletes, moves, adds,
// sorts, empties or fills, as a person's clicks would, and after each every list is compared with
// the state. 400 steps on the server's page, 200 on the browser's.
#[test]
fn a_seeded_walk_keeps_every_list_to_its_state() {
    let src = r###"fn seed() = range(1, 9) | map((i) => { id: i, done: i % 3 == 0, k: i % 4 })
fn subs(id, k) {
    let some = take([`${id}a`, `${id}b`, `${id}c`], k % 4)
    k % 2 == 1 ? reverse(some) : some
}
fn change(xs, id, f) = xs | map((x) => x.id == id ? f(x) : x)

view Choice(x) = x.done ? <li class="d">done {x.id}</li> : <li>todo {x.id}</li>
fn Plain(x) = x.done ? <li class="d">done {x.id}</li> : <li>todo {x.id}</li>
view Group(x) = {for s in subs(x.id, x.k) => <li key={s}>{s}</li>}
view Pair(x) = [<dt>{x.id}</dt>, x.done ? <dd class="d">done</dd> : <dd>todo</dd>]
component Tr(x) {
    x.done ? <tr class="d"><td>done {x.id}</td></tr> : <tr><td>todo {x.id}</td></tr>
}

component Walk() {
    state xs = seed()
    state arg = "0"
    state next = 100
    let a = int(arg)
    let n = len(xs)
    <div>
        <input #arg value={arg}/>
        <button #flip @click={() => { xs = change(xs, a, (x) => merge(x, { done: !x.done })) }}>flip</button>
        <button #sub @click={() => { xs = change(xs, a, (x) => merge(x, { k: x.k + 1 })) }}>sub</button>
        <button #both @click={() => { xs = change(xs, a, (x) => merge(x, { done: !x.done, k: x.k + 1 })) }}>both</button>
        <button #del @click={() => { xs = xs | filter((x) => x.id != a) }}>del</button>
        <button #front @click={() => { xs = (xs | filter((x) => x.id == a)) + (xs | filter((x) => x.id != a)) }}>front</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #rot @click={() => { if n > 0 { xs = drop(xs, a % n) + take(xs, a % n) } }}>rot</button>
        <button #add @click={() => { let p = a % (n + 1); xs = take(xs, p) + [{ id: next, done: a % 2 == 0, k: a % 4 }] + drop(xs, p); next += 1 }}>add</button>
        <button #sort @click={() => { xs = sort_by(xs, (x) => [x.done ? 0 : 1, x.id]) }}>sort</button>
        <button #clear @click={() => { xs = [] }}>clear</button>
        <button #fill @click={() => { xs = seed() }}>fill</button>
        <ul #a>{for x in xs => x.done ? <li key={x.id} class="d">done {x.id}</li> : <li key={x.id}>todo {x.id}</li>}</ul>
        <ul #i>{for x in xs { if x.done { <li key={x.id} class="d">done {x.id}</li> } else { <li key={x.id}>todo {x.id}</li> } }}</ul>
        <ul #v>{for x in xs => <Choice key={x.id} x={x}/>}</ul>
        <ul #f>{for x in xs => <Plain key={x.id} x={x}/>}</ul>
        <ul #g>{for x in xs => <Group key={x.id} x={x}/>}</ul>
        <ul #r>{for x in xs => {for s in subs(x.id, x.k) => <li key={s}>{s}</li>}}</ul>
        <dl #p>{for x in xs => <Pair key={x.id} x={x}/>}</dl>
        <table><tbody #t>{for x in xs => <Tr key={x.id} x={x}/>}</tbody></table>
        <ul #w>{for x in xs => x.done ? {for s in subs(x.id, x.k) => <li key={s}>{s}</li>} : <li key={x.id}>todo {x.id}</li>}</ul>
        <ul #s>{for x in xs => <li key={x.id} class={x.done ? "d" : ""}>{x.done ? "done" : "todo"} {x.id}</li>}</ul>
        <pre #truth>{json.stringify(xs)}</pre>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Walk/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r#"const subs = (id, k) => { const s = [`${id}a`, `${id}b`, `${id}c`].slice(0, k % 4); return k % 2 === 1 ? s.reverse() : s; };
    const word = (x) => `${x.done ? "done" : "todo"} ${x.id}`;
    const texts = (id) => [...$(id).querySelectorAll(id === "t" ? "tr" : ":scope > *")].map((e) => e.textContent).join(",");
    const check = () => {
      const xs = JSON.parse($("truth").textContent), bad = [];
      const want = { a: xs.map(word), i: xs.map(word), v: xs.map(word), f: xs.map(word), s: xs.map(word), t: xs.map(word),
        g: xs.flatMap((x) => subs(x.id, x.k)), r: xs.flatMap((x) => subs(x.id, x.k)), p: xs.flatMap((x) => [String(x.id), x.done ? "done" : "todo"]),
        w: xs.flatMap((x) => (x.done ? subs(x.id, x.k) : [`todo ${x.id}`])) };
      for (const [id, w] of Object.entries(want)) if (texts(id) !== w.join(",")) bad.push(`${id}: ${texts(id)} for ${w.join(",")}`);
      for (const id of ["a", "i", "v", "f", "s"]) {
        const done = [...$(id).children].map((e) => e.className === "d").join(","), w = xs.map((x) => x.done).join(",");
        if (done !== w) bad.push(`${id} classes: ${done} for ${w}`);
      }
      return bad;
    };
    const set = (v) => { $("arg").value = String(v); $("arg").dispatchEvent(new Event("input", { bubbles: true })); };
    const ops = ["flip", "flip", "sub", "both", "del", "front", "rev", "rot", "add", "add", "sort", "clear", "fill"];
    const out = { steps: 0, first: null };
    for (const seed of location.pathname === "/h" ? [1, 2] : [3]) {
      if (out.first) break;
      let s = seed;
      const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 4294967296;
      set(0); $("fill").click(); await tick(0);
      for (let n = 0; n < 200; n++) {
        const ids = JSON.parse($("truth").textContent).map((x) => x.id);
        let op = ops[Math.floor(rnd() * ops.length)];
        // An empty list fills again more often than it is emptied.
        if (op === "clear" && rnd() < 0.5) op = "rev";
        const a = ids.length && rnd() < 0.9 ? ids[Math.floor(rnd() * ids.length)] : Math.floor(rnd() * 20);
        set(a); $(op).click(); await tick(0);
        out.steps++;
        const bad = check(), errs = window.__quilloErrors || [];
        if (bad.length || errs.length) { out.first = { seed, step: n, op: `${op} ${a}`, bad: bad.slice(0, 3), errors: errs.slice(0, 2) }; break; }
      }
    }
    return out;"#;
    let Some(rs) = in_pages("q889-walk", src, BOTH, body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "steps": 400, "first": null, "remade": 0, "errors": [] }), "the server's page");
    assert_eq!(rs[1], serde_json::json!({ "steps": 200, "first": null, "remade": 0, "errors": [] }), "the browser's page");
}

// Q-894: a row of more than one node was moved through a fragment, which takes it off the page:
// a branch-rooted row (its marker is a node) or a view giving two lost their scroll, and a frame in
// one reloaded. Each node now moves by itself, with `moveBefore` where the browser has it, so
// they keep both, as single-element rows did; a focused field keeps the focus and its selection.
const MOVES: &str = r###"view Pair(x) = [<dt class="s" style="height:40px;overflow:auto"><div style="height:400px">{x}</div></dt>, <dd>{x}</dd>]
component Moves() {
    state xs = [1, 2, 3]
    <div>
        <div #one>{for x in xs => <div key={x} class="s" style="height:40px;overflow:auto"><div style="height:400px">{x}</div></div>}</div>
        <div #two>{for x in xs => x > 0 ? <div key={x} class="s" style="height:40px;overflow:auto"><div style="height:400px">{x}</div></div> : <div key={x}>{x}</div>}</div>
        <dl #pair>{for x in xs => <Pair key={x} x={x}/>}</dl>
        <div #deep>{for x in xs => x > 0 ? <section key={x}><div class="s" style="height:40px;overflow:auto"><div style="height:400px">{x}</div></div></section> : <section key={x}>{x}</section>}</div>
        <div #fr1>{for x in xs => <div key={x}><iframe src={`/inner/${x}`}></iframe></div>}</div>
        <div #fr2>{for x in xs => x > 0 ? <div key={x}><iframe src={`/inner/${x}`}></iframe></div> : <div key={x}>{x}</div>}</div>
        <ul #focus>{for x in xs => x > 0 ? <li key={x}><input id={`in-${x}`} value={`text ${x}`}/></li> : <li key={x}>{x}</li>}</ul>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Moves/> : null}</div>
}
fn page(now: bool, safari: bool) = <html><head><title>t</title>{safari ? <script>{raw("delete Element.prototype.moveBefore")}</script> : null}</head><body><Host {now}/></body></html>
route GET /h = page(true, false)
route GET /b = page(false, false)
route GET /hs = page(true, true)
route GET /bs = page(false, true)
route GET /inner/:n = <html><body>inner {n}</body></html>
"###;

const MOVES_BODY: &str = r#"const frames = () => [...document.querySelectorAll("iframe")];
    for (let i = 0; i < 100 && !frames().every((f) => f.contentDocument && f.contentDocument.body && f.contentDocument.body.textContent.startsWith("inner")); i++) await tick(50);
    const loads = { fr1: 0, fr2: 0 };
    for (const id of ["fr1", "fr2"]) for (const f of $(id).querySelectorAll("iframe")) f.addEventListener("load", () => loads[id]++);
    for (const s of document.querySelectorAll(".s")) s.scrollTop = 77;
    const field = $("in-1"); field.focus(); field.setSelectionRange(1, 3);
    await tick(200);
    await click("rev"); await tick(300);
    const out = {};
    for (const id of ["one", "two", "pair", "deep"]) out[id] = [...$(id).querySelectorAll(".s")].map((s) => `${s.textContent}:${s.scrollTop}`).join(",");
    out.focus = `${document.activeElement.id} ${field.selectionStart}-${field.selectionEnd}`;
    out.order = [...$("fr2").children].map((d) => d.querySelector("iframe").src.split("/").pop()).join(",");
    out.reloads = loads;
    out.moveBefore = "moveBefore" in Element.prototype;
    return out;"#;

#[test]
fn moved_rows_of_several_nodes_keep_their_scroll_and_frames() {
    let Some(rs) = in_pages("q894", MOVES, BOTH, MOVES_BODY) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        if r["moveBefore"] == false {
            eprintln!("skipping: this Chrome has no moveBefore");
            return;
        }
        let scrolled = "3:77,2:77,1:77";
        let want = serde_json::json!({
            "one": scrolled, "two": scrolled, "pair": scrolled, "deep": scrolled, "focus": "in-1 1-3", "order": "3,2,1",
            "reloads": { "fr1": 0, "fr2": 0 }, "moveBefore": true, "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-894 without `moveBefore`, as on Safari (the page takes it away before the runtime comes):
// every moved element went off the page and back, single-element rows too, and was scrolled back
// to its start. What the moved rows' elements were scrolled to is put back after the move, as the
// focus is. A frame moved there still loads again: nothing can keep it.
#[test]
fn without_move_before_moved_rows_are_scrolled_back() {
    let paths = ["/hs", "/bs"];
    let Some(rs) = in_pages("q894-safari", MOVES, &paths, MOVES_BODY) else { return };
    for (r, path) in rs.iter().zip(paths) {
        let scrolled = "3:77,2:77,1:77";
        let mut r = r.clone();
        r.as_object_mut().unwrap().remove("reloads");
        let want = serde_json::json!({
            "one": scrolled, "two": scrolled, "pair": scrolled, "deep": scrolled, "focus": "in-1 1-3", "order": "3,2,1",
            "moveBefore": false, "remade": 0, "errors": [],
        });
        assert_eq!(r, want, "at {}", path);
    }
}

// Q-894's edges without `moveBefore`: 70 rows scrolled down and across, and the list's own box
// scrolled too (without scroll anchoring, which would move it by what is deleted above); ten rows
// deleted, then a reverse and a turn. The 60 rows left keep both scrolls, and the box keeps its
// own, which is no longer read or written for the move. With nothing scrolled, a move scrolls
// nothing. (That deleted elements are let go is the next test's.)
#[test]
fn without_move_before_many_scrolled_rows_keep_their_scroll() {
    let src = r###"component Many() {
    state xs = range(1, 71)
    <div>
        <div #l style="height:200px;overflow:auto;overflow-anchor:none">{for x in xs => x > 0 ? <div key={x} class="s" style="height:40px;width:100px;overflow:auto"><div style="height:400px;width:400px">{x}</div></div> : <div key={x}/>}</div>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #turn @click={() => { xs = drop(xs, 3) + take(xs, 3) }}>turn</button>
        <button #cut @click={() => { xs = drop(xs, 10) }}>cut</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Many/> : null}</div>
}
fn page(now: bool) = <html><head><title>t</title><script>{raw("delete Element.prototype.moveBefore")}</script></head><body><Host {now}/></body></html>
route GET /hs = page(true)
route GET /bs = page(false)
"###;
    let body = r#"const at = () => [...$("l").querySelectorAll(".s")].map((s) => `${s.textContent}:${s.scrollTop}/${s.scrollLeft}`);
    const kept = () => at().every((t) => { const [n, sc] = t.split(":"); return sc === `${Number(n) + 5}/3`; });
    const out = {};
    await click("rev"); out.unscrolled = at().filter((t) => !t.endsWith(":0/0")).length;
    await click("rev");
    for (const s of $("l").querySelectorAll(".s")) { s.scrollTop = Number(s.textContent) + 5; s.scrollLeft = 3; }
    $("l").scrollTop = 500;
    await tick(200);
    await click("cut", "rev"); await tick(50); out.rev = [at().length, kept(), at()[0], $("l").scrollTop];
    await click("turn"); await tick(50); out.turn = [at().length, kept(), at()[0], $("l").scrollTop];
    return out;"#;
    let paths = ["/hs", "/bs"];
    let Some(rs) = in_pages("q894-many", src, &paths, body) else { return };
    for (r, path) in rs.iter().zip(paths) {
        let want = serde_json::json!({
            "unscrolled": 0, "rev": [60, true, "70:75/3", 500], "turn": [60, true, "67:72/3", 500], "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-894's scroll record without `moveBefore` held each scrolled element until 64 were known: a
// scrolled panel shown and hidden again stayed alive, with all it held, for good. It holds them
// weakly now: as many are let go as with `moveBefore`, where nothing is recorded, past the 16 at
// which the record first drops those that left. (`gc()` from the page keeps one alive either way,
// so the count is compared, not taken as 0.)
#[test]
fn without_move_before_scrolled_elements_that_leave_are_let_go() {
    let src = r###"component Toggle() {
    state on = true
    state xs = [1, 2, 3]
    <div>
        <button #tog @click={() => { on = !on }}>tog</button>
        {on ? <div class="panel" style="height:40px;overflow:auto"><ul>{for x in xs => <li key={x} style="height:30px">{x}</li>}</ul></div> : <p>gone</p>}
    </div>
}
fn page(safari: bool) = <html><head><title>t</title>{safari ? <script>{raw("delete Element.prototype.moveBefore")}</script> : null}</head><body><Toggle/></body></html>
route GET /h = page(false)
route GET /hs = page(true)
"###;
    let body = r#"const refs = [];
    for (let k = 0; k < 20; k++) {
      const panel = document.querySelector(".panel");
      panel.scrollTop = 7;
      await tick(100);
      refs.push(new WeakRef(panel));
      await click("tog", "tog");
    }
    for (let k = 0; k < 3; k++) { await tick(50); gc(); }
    return { alive: refs.filter((w) => w.deref() !== undefined).length, moveBefore: "moveBefore" in Element.prototype };"#;
    let paths = ["/h", "/hs"];
    let Some(rs) = in_pages("q894-gone", src, &paths, body) else { return };
    assert_eq!(rs[0]["moveBefore"], true, "{}", rs[0]);
    assert_eq!(rs[1]["moveBefore"], false, "{}", rs[1]);
    assert!(rs[0]["alive"].as_u64().unwrap() <= 1, "with moveBefore: {}", rs[0]);
    assert_eq!(rs[1]["alive"], rs[0]["alive"], "without moveBefore: {} (with it: {})", rs[1], rs[0]);
    assert_eq!((&rs[1]["errors"], &rs[1]["remade"]), (&serde_json::json!([]), &serde_json::json!(0)), "{}", rs[1]);
}

// ---- Across a wait (Q-899, Q-901, Q-902) -------------------------------------------------------

// Q-899: a `live` state's load that failed while the socket stayed up gave up, and the page showed
// old data until the next change; one made in the browser whose first load failed followed nothing
// at all, as it never learned what to follow. Now it tries again, after a second, then two, doubling
// up to 30 s, until a load comes, with `waiting()` true meanwhile and the console told once a run of
// failures. The page's fetch is wrapped here to fail the loads the test says: no connection, a 503,
// two in a row, one slow to fail with a change heard meanwhile, all of them for a while; then an
// error value from the loader, which is an answer and is not asked again, and a failure after which
// the component goes, which ends the tries.
const LIVE_FAILS: &str = r###"db.open("live.db")
db.exec("create table if not exists t (id integer primary key, v int not null)")
db.exec("delete from t")
db.exec("insert into t (id, v) values (1, 0)")
// 99 is an error value: the state takes it, as a loader's answer.
server fn load_v(id: int) {
    let r = db.query("select v from t where id = ?", id)
    r[0].v == 99 ? err("ninety-nine") : r
}
component V(id: int) {
    state r = live(load_v, id)
    <p #v class={{ behind: waiting(r) }}>{r == null ? "none" : is_err(r) ? error(r) : r[0].v}</p>
}
component Host(now: bool) {
    state on = now
    <div>
        <button #show @click={() => { on = true }}>show</button>
        <button #off @click={() => { on = false }}>off</button>
        {on ? <V id={1}/> : <p #gone>gone</p>}
    </div>
}
route POST /t/set/:v(int) { db.exec("update t set v = ? where id = 1", v); "ok" }
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /f = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn a_failed_live_load_tries_again_until_one_comes() {
    let body = r##"const real = window.fetch.bind(window);
    // Loads of `load_v` that fail: `fail` more of them, by `mode` (no connection, or a 503), each
    // after `delay` ms; while `down`, all of them.
    const f = { fail: 0, mode: "net", delay: 0, down: false, calls: 0 };
    window.fetch = async (u, init) => {
      if (String(u).includes("/_quillo/rpc/load_v")) {
        f.calls++;
        if (f.down || f.fail > 0) {
          if (!f.down) f.fail--;
          if (f.delay) await tick(f.delay);
          if (f.down || f.mode === "503") return new Response("busy", { status: 503 });
          throw new TypeError("Failed to fetch");
        }
      }
      return real(u, init);
    };
    const set = (v) => real(`/t/set/${v}`, { method: "POST" });
    const shown = () => ($("v") ? $("v").textContent : null);
    const behind = () => $("v").classList.contains("behind");
    const until = async (want, ms) => { const t0 = performance.now(); while (performance.now() - t0 < ms) { if (shown() === String(want)) return Math.round(performance.now() - t0); await tick(20); } return null; };
    const failed = async (n) => { for (let i = 0; i < 200 && f.calls < n; i++) await tick(10); await tick(60); };
    const out = {};
    if (location.pathname === "/f") {
      // Made in the browser: its first load fails.
      await set(0); f.fail = 1;
      await click("show"); await failed(1);
      out.first = [shown(), behind()];
      out.firstCaughtUp = (await until(0, 4000)) !== null && !behind();
      if (!out.firstCaughtUp) return out;
    }
    // (Each case that does not catch up ends the test: the rest would wait out their time.)
    // A change, no failure: as it was.
    await set(1); out.plain = (await until(1, 3000)) !== null;
    if (!out.plain) return out;
    // One load that cannot connect, then one answered 503: the page is behind, says so, and catches up,
    // after a wait.
    for (const [v, mode] of [[2, "net"], [3, "503"]]) {
      f.mode = mode; f.fail = 1; const c = f.calls;
      await set(v); await failed(c + 1);
      out[mode] = [shown(), behind()];
      const t = await until(v, 4000);
      out[mode + "CaughtUp"] = t !== null && !behind() && t >= 300;
      if (!out[mode + "CaughtUp"]) return out;
    }
    // Two failures in a row.
    { f.mode = "net"; f.fail = 2; const c = f.calls; await set(4); await failed(c + 2); out.twice = [shown(), behind(), f.calls - c]; out.twiceCaughtUp = (await until(4, 7000)) !== null; if (!out.twiceCaughtUp) return out; }
    // A change heard while the load that then fails is on its way.
    { f.mode = "net"; f.fail = 1; f.delay = 300; const c = f.calls; await set(5); await tick(120); await set(6); await failed(c + 1); f.delay = 0; out.during = (await until(6, 5000)) !== null; if (!out.during) return out; }
    // Every load refused for a while, then answered again; nothing changes meanwhile.
    { f.down = true; const c = f.calls; await set(7); await failed(c + 1); await tick(1600); out.down = [shown(), behind(), f.calls - c >= 2]; f.down = false; out.back = (await until(7, 8000)) !== null && !behind(); }
    // An error value is an answer: shown, and not asked again.
    { const c = f.calls; await set(99); const ok = (await until("ninety-nine", 3000)) !== null; await tick(1500); out.errValue = [ok, behind(), f.calls - c]; await set(9); out.after = (await until(9, 3000)) !== null; }
    // A failure, then the component goes: no more tries.
    { f.mode = "net"; f.fail = 1; const c = f.calls; await set(8); await failed(c + 1); await click("off"); const n = f.calls; await tick(2500); out.gone = [shown(), f.calls - n]; }
    return out;"##;
    let paths = ["/h", "/f"];
    let Some(rs) = in_pages("q899", LIVE_FAILS, &paths, body) else { return };
    let net = "server fn `load_v` could not be reached: Failed to fetch";
    let busy = "server fn `load_v` failed (503)";
    for (r, path) in rs.iter().zip(paths) {
        let mut want = serde_json::json!({
            "plain": true, "net": ["1", true], "netCaughtUp": true, "503": ["2", true], "503CaughtUp": true,
            "twice": ["3", true, 2], "twiceCaughtUp": true, "during": true, "down": ["6", true, true], "back": true,
            "errValue": [true, false, 1], "after": true, "gone": [null, 0], "remade": 0,
            // Once a run of failures: the two in a row, and all of those while it was down, are one.
            "errors": [net, busy, net, net, busy, net],
        });
        if path == "/f" {
            want["first"] = serde_json::json!(["none", true]);
            want["firstCaughtUp"] = serde_json::json!(true);
            want["errors"].as_array_mut().unwrap().insert(0, serde_json::json!(net));
        }
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-901: after a handler's first wait, what it started belonged to no one: `every` refused ("works in
// a component"), and a `run_after` fired after its component had gone. The handler's part of the page
// is taken up with it after each wait, so both, and `listen`, last as long as it does: they run while
// the component is on the page, stop as it goes, and start nothing once it has gone during the wait.
// `listen` checked `Owner` alone, so it failed in any handler, with a wait or without; it works there
// now as `every` does. Nested components: the inner one's timer goes with it, the outer one's stays.
const TIMERS: &str = r###"server fn slow(ms: int) -> str { sleep(ms); "x" }
route GET /ticks = events((send) => {
    for i in range(100000) {
        send(i)
        sleep(40)
    }
})
component Alive(tick) {
    <div>
        <button #ev @click={() => { slow(30); every(20, () => { tick("ev") }) }}>ev</button>
        <button #ra @click={() => { slow(30); run_after(150, () => { tick("ra") }) }}>ra</button>
        <button #r0 @click={() => { run_after(150, () => { tick("r0") }) }}>r0</button>
        <button #two @click={() => { slow(20); slow(20); every(20, () => { tick("two") }) }}>two</button>
        <button #ls @click={() => { slow(30); listen("/ticks", (n) => { tick("ls") }) }}>ls</button>
        <button #l0 @click={() => { listen("/ticks", (n) => { tick("l0") }) }}>l0</button>
        <button #late @click={() => { slow(250); run_after(10, () => { tick("late-ra") }); every(20, () => { tick("late") }); listen("/ticks", (n) => { tick("late-ls") }) }}>late</button>
    </div>
}
component Inner(tick) {
    <button #in @click={() => { slow(30); every(20, () => { tick("in") }) }}>in</button>
}
component Outer(tick) {
    state inner = true
    <div>
        <button #out @click={() => { slow(30); every(20, () => { tick("out") }) }}>out</button>
        <button #inoff @click={() => { inner = false }}>inner off</button>
        {inner ? <Inner {tick}/> : null}
    </div>
}
component Host(now: bool) {
    state on = now
    state shown = true
    state outer = true
    state log = []
    let tick = (s) => { log = log + [s] }
    <div>
        <button #show @click={() => { on = true }}>show</button>
        <button #hide @click={() => { shown = false }}>hide</button>
        <button #again @click={() => { shown = true }}>again</button>
        <button #outoff @click={() => { outer = false }}>outer off</button>
        <button #clear @click={() => { log = [] }}>clear</button>
        {on && shown ? <Alive {tick}/> : null}
        {on && outer ? <Outer {tick}/> : null}
        <p #log>{join(log, ",")}</p>
    </div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn timers_and_streams_started_after_a_wait_last_as_long_as_their_component() {
    let body = r##"const n = (s) => $("log").textContent.split(",").filter((t) => t === s).length;
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(10); return f(); };
    // Runs (three times), and stops as its component goes.
    const runsThenStops = async (id, off) => {
      const ran = await until(() => n(id) > 2, 4000);
      await click(off); await tick(40);
      const b = n(id); await tick(250);
      return [ran, n(id) === b];
    };
    const out = {};
    await click("ev"); out.every = await runsThenStops("ev", "hide");
    await click("again", "clear");
    // `run_after` after a wait: the component goes first, and it never fires; it stays, and it fires once.
    await click("ra"); await tick(80); await click("hide"); await tick(400); out.runAfterGone = n("ra");
    await click("again", "clear");
    await click("ra"); await until(() => n("ra") > 0, 4000); await tick(300); out.runAfterStays = n("ra");
    await click("clear");
    // With no wait, as it was.
    await click("r0"); await tick(40); await click("hide"); await tick(400); out.runAfterNoWait = n("r0");
    await click("again", "clear");
    // A handler that waits twice.
    await click("two"); out.twice = await runsThenStops("two", "hide");
    await click("again", "clear");
    // `listen` after a wait, and with none.
    for (const id of ["ls", "l0"]) {
      await click(id); out[id] = await runsThenStops(id, "hide");
      await click("again", "clear");
    }
    // The component goes during the wait: nothing starts after it.
    await click("late"); await tick(60); await click("hide"); await tick(800); out.late = [n("late-ra"), n("late"), n("late-ls")];
    await click("again", "clear");
    // Nested: the inner component's `every` goes with it while the outer one's goes on, then that goes with its own.
    await click("out", "in");
    out.nested = await runsThenStops("in", "inoff");
    { const o = n("out"); await tick(100); out.nested.push(n("out") > o); }
    out.outer = await runsThenStops("out", "outoff");
    return out;"##;
    let Some(rs) = in_pages("q901", TIMERS, &["/h", "/b"], body) else { return };
    for (r, path) in rs.iter().zip(["/h", "/b"]) {
        let want = serde_json::json!({
            "every": [true, true], "runAfterGone": 0, "runAfterStays": 1, "runAfterNoWait": 0, "twice": [true, true],
            "ls": [true, true], "l0": [true, true], "late": [0, 0, 0], "nested": [true, true, true], "outer": [true, true],
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-902: a save that ended after the person had followed a link elsewhere took them on to where it
// led. A `navigate` after a wait does nothing now when the page changed meanwhile by anything but
// the run itself: a link, Back, another run's `navigate`, also between two waits. Whether the
// clicked component is still there does not matter: a modal that its Save closes, a panel a sibling
// state takes away, a row or a branch in a branch gone, all move as they did; so do a navigate
// before any wait, one after a wait or two with nothing else moving the page, the run's own move
// followed by a wait and another move (two calls of one `parallel` too), and a change of the hash
// alone, which keeps the page.
const NAVIGATES: &str = r###"server fn slow(ms: int) -> str { sleep(ms); "x" }
component Modal(on_close) {
    <div class="modal"><button #msave @click={() => { on_close(); slow(200); navigate("/q/item") }}>save</button></div>
}
component Panel() {
    <button #psave @click={() => { slow(200); navigate("/q/sib") }}>panel save</button>
}
component Saver() {
    state rows = ["r"]
    state open = true
    state deep = true
    state modal = true
    state panel = true
    <div>
        <button #now @click={() => { navigate("/q/now") }}>now</button>
        <button #save @click={() => { slow(200); navigate("/q/done") }}>save</button>
        <button #twice @click={() => { slow(100); slow(100); navigate("/q/twice") }}>twice</button>
        <button #own @click={() => { navigate("/q/step"); slow(200); navigate("/q/after") }}>own</button>
        <button #par @click={() => { parallel(() => { slow(50); navigate("/q/p1") }, () => { slow(250); navigate("/q/p2") }) }}>par</button>
        <button #pclose @click={() => { panel = false }}>close the panel</button>
        <a #tospot href="#spot">spot</a>
        <ul>{for r in rows => <li key={r}><button #row @click={() => { rows = []; slow(200); navigate("/q/row") }}>row</button></li>}</ul>
        {open ? <div>{deep ? <button #deep @click={() => { open = false; slow(200); navigate("/q/deep") }}>deep</button> : null}</div> : null}
        {modal ? <Modal on_close={() => { modal = false }}/> : null}
        {panel ? <Panel/> : null}
        <p #spot>spot</p>
    </div>
}
component App() {
    let path = url().path
    <div>
        <a #other href="/q/other">other</a>
        <a #home href="/q">home</a>
        <p #where>{path}</p>
        {path == "/q" ? <Saver/> : <p #else>{path}</p>}
    </div>
}
route GET /q = <html><head><title>t</title></head><body><App/></body></html>
route GET /q/:x = <html><head><title>t</title></head><body><App/></body></html>
"###;

#[test]
fn a_late_navigate_is_dropped_only_if_the_page_moved_meanwhile() {
    let body = r##"const at = () => location.pathname;
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(10); return f(); };
    const moved = () => until(() => at() !== "/q", 4000);
    const home = async () => { await click("home"); await until(() => $("save"), 3000); };
    const out = {};
    // Before any wait; after a wait, or two, with nothing else moving the page.
    await click("now"); out.now = [at(), $("where").textContent]; await home();
    await click("save"); await moved(); out.save = at(); await home();
    await click("twice"); await moved(); out.twice = at(); await home();
    // What the button was in gone by the program's own doing: a row, a branch in a branch, the
    // modal its Save closes, a panel a sibling state takes away.
    await click("row"); await moved(); out.row = at(); await home();
    await click("deep"); await moved(); out.deep = at(); await home();
    await click("msave"); { const gone = !$("msave"); await moved(); out.modal = [gone, at()]; } await home();
    await click("psave", "pclose"); { const gone = !$("psave"); await moved(); out.panel = [gone, at()]; } await home();
    // The run's own move, a wait, and another: both; so the two calls of one `parallel`.
    await click("own"); out.own = [at()]; await until(() => at() === "/q/after", 4000); out.own.push(at()); await home();
    await click("par"); await until(() => at() === "/q/p1", 4000); out.par = [at()]; await until(() => at() === "/q/p2", 4000); out.par.push(at()); await home();
    // The page moved during the wait: a link, a link between two waits, Back, another run.
    await click("save"); await tick(20); await click("other"); await tick(600); out.left = [at(), $("where").textContent]; await home();
    await click("twice"); await tick(150); await click("other"); await tick(600); out.leftBetween = at(); await home();
    await click("other"); await home();
    await click("save"); await tick(20); history.back(); await tick(600); out.back = at(); await home();
    await click("save"); await tick(20); await click("now"); await tick(600); out.otherRun = at(); await home();
    // The hash alone changes: the same page.
    await click("save"); await tick(20); await click("tospot"); await moved(); out.hash = [at(), location.hash];
    return out;"##;
    let Some(rs) = in_pages("q902", NAVIGATES, &["/q"], body) else { return };
    let want = serde_json::json!({
        "now": ["/q/now", "/q/now"], "save": "/q/done", "twice": "/q/twice", "row": "/q/row", "deep": "/q/deep",
        "modal": [true, "/q/item"], "panel": [true, "/q/sib"], "own": ["/q/step", "/q/after"], "par": ["/q/p1", "/q/p2"],
        "left": ["/q/other", "/q/other"], "leftBetween": "/q/other", "back": "/q/other", "otherRun": "/q/now",
        "hash": ["/q/done", ""], "remade": 0, "errors": [],
    });
    assert_eq!(rs[0], want);
}

// ---- What Quillo takes off the page (Q-896) ----------------------------------------------------

// Q-896: Chrome sends a focused field `blur` and `focusout` as it is taken off the page, which
// Firefox and Safari do not, so the report's inline edit saved on Enter and again on the blur of the
// field that was going. No handler runs while Quillo takes elements off the page, wherever they go
// from: a branch (and the part around it that stays hears no `focusout` either), a select its own
// change takes away, an element's only content, a component, a row, all the rows, the page left.
// A field that loses the focus while still there runs its handlers, one in a row that moves keeps
// them, and what is sent to an element after it has gone still reaches them (the select's change
// and blur sent after it went). The page has the focus here, as in a window.
const GOES: &str = r###"component Edit(save) {
    state editing = true
    {editing
        ? <input #ed @keydown={(e) => { if e.key == "Enter" { save("enter"); editing = false } }} @blur={() => { save("blur"); editing = false }} @focusout={() => { save("out") }}/>
        : <button #edit @click={() => { editing = true }}>saved</button>}
}
component Field(say) {
    <input #fi @keydown={(e) => { if e.key == "Escape" { say("esc") } }} @blur={() => { say("fi-blur") }}/>
}
component Parts(say) {
    state open = true
    state picking = true
    state only = true
    state comp = true
    state xs = [1, 2, 3]
    <div>
        <div #keep @focusout={() => { say("keep-out") }}>
            {open ? <div #box @focusout={() => { say("box-out") }}><input #in2 @keydown={(e) => { if e.key == "Escape" { open = false } }} @blur={() => { say("in2-blur") }}/></div> : null}
        </div>
        {picking ? <select #sel @change={(e) => { say(`change:${e.value}`); picking = false }} @blur={() => { say("sel-blur") }}><option>a</option><option>b</option></select> : null}
        <p #only>{only ? <input #da @keydown={(e) => { if e.key == "Escape" { only = false } }} @blur={() => { say("da-blur") }}/> : "text"}</p>
        {comp ? <Field say={(s) => { say(s); if s == "esc" { comp = false } }}/> : null}
        <ul #rows>{for x in xs => <li key={x}><input class="r" data-x={x} @keydown={(e) => {
            if e.key == "Delete" { xs = xs | filter((i) => i != x) }
            if e.key == "Backspace" { xs = [] }
            if e.key == "r" { xs = reverse(xs) }
        }} @input={() => { say(`in:${x}`) }} @blur={() => { say(`blur:${x}`) }}/></li>}</ul>
        <input #elsewhere/>
        <button #refill @click={() => { xs = [1, 2, 3]; open = true; picking = true; only = true; comp = true }}>refill</button>
    </div>
}
component Host(now: bool, say) {
    state on = now
    <div>
        <button #show @click={() => { on = true }}>show</button>
        {on ? <div><Edit save={say}/><Parts {say}/></div> : null}
        <input #nav @keydown={(e) => { if e.key == "Enter" { navigate("/away") } }} @blur={() => { say("nav-blur") }}/>
    </div>
}
component Page() {
    let path = url().path
    state log = []
    let say = (s) => { log = log + [s] }
    <div>
        <button #clear @click={() => { log = [] }}>clear</button>
        {path == "/away" ? <p #there>away</p> : <Host now={path == "/h"} {say}/>}
        <p #log>{join(log, ",")}</p>
    </div>
}
route GET /h = <html><head><title>t</title></head><body><Page/></body></html>
route GET /b = <html><head><title>t</title></head><body><Page/></body></html>
route GET /away = <html><head><title>t</title></head><body><Page/></body></html>
"###;

#[test]
fn handlers_of_what_quillo_takes_off_the_page_run_no_more() {
    let body = r##"const log = () => $("log").textContent;
    const key = (el, k) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    const row = (x) => document.querySelector(`.r[data-x="${x}"]`);
    const out = { focused: document.hasFocus() };
    // The report's inline edit: Enter saves, once.
    $("ed").focus(); key($("ed"), "Enter"); await tick(60); out.enter = log(); await click("clear");
    // A real blur still runs: the focus goes to another field, then the field goes.
    await click("edit"); $("ed").focus(); $("elsewhere").focus(); await tick(60); out.blur = log(); await click("clear");
    // A branch holding the focused field goes.
    $("in2").focus(); key($("in2"), "Escape"); await tick(60); out.branch = [log(), !!$("in2")]; await click("clear");
    // A select that its change takes away; then a change and a blur sent to it, which it hears.
    { const s = $("sel"); s.focus(); s.value = "b"; s.dispatchEvent(new Event("change", { bubbles: true })); await tick(60);
      s.dispatchEvent(new Event("change", { bubbles: true })); s.dispatchEvent(new FocusEvent("blur")); await tick(60); out.select = [log(), !!$("sel")]; await click("clear"); }
    // An element's only content, replaced.
    $("da").focus(); key($("da"), "Escape"); await tick(60); out.only = [log(), $("only").textContent]; await click("clear");
    // A component.
    $("fi").focus(); key($("fi"), "Escape"); await tick(60); out.comp = [log(), !!$("fi")]; await click("clear");
    // A row, then all of them.
    row(2).focus(); key(row(2), "Delete"); await tick(60); out.rowGone = [log(), document.querySelectorAll(".r").length]; await click("clear");
    row(1).focus(); key(row(1), "Backspace"); await tick(60); out.rowsCleared = [log(), document.querySelectorAll(".r").length]; await click("clear");
    // A row that moves keeps the focus and its handlers.
    await click("refill");
    { const r1 = row(1); r1.focus(); key(r1, "r"); await tick(60);
      out.moved = [document.activeElement === r1, [...document.querySelectorAll(".r")].map((e) => e.dataset.x).join("")];
      r1.dispatchEvent(new Event("input", { bubbles: true })); $("elsewhere").focus(); await tick(60); out.movedHears = log(); await click("clear"); }
    // The page left.
    $("nav").focus(); key($("nav"), "Enter"); await tick(150); out.away = [location.pathname, log(), !!$("there")];
    return out;"##;
    let Some(rs) = in_pages_with("q896", GOES, BOTH, body, true) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "focused": true, "enter": "enter", "blur": "blur,out", "branch": ["", false], "select": ["change:b,change:b,sel-blur", false],
            "only": ["", "text"], "comp": ["esc", false], "rowGone": ["", 2], "rowsCleared": ["", 0], "moved": [true, "321"],
            "movedHears": "in:1,blur:1", "away": ["/away", "", true], "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-896's edge: only what comes while Quillo takes elements off the page is held back. A card
// dragged into another list, where the drop moves it in state, is taken off the first list and made
// in the second; the browser then sends `dragend` to the card it began with, off the page by now,
// and its handler runs, once, so the drag's state is cleared.
const DRAG: &str = r###"component Board() {
    state a = [1, 2]
    state b = [3]
    state dragging = 0
    state log = []
    let start = (x) => { dragging = x; log = log + [`start:${x}`] }
    let end = () => { log = log + [`end:${dragging}`]; dragging = 0 }
    <div>
        <ul #a>{for x in a => <li key={x} class="card" data-x={x} draggable="true" @dragstart={() => { start(x) }} @dragend={() => { end() }}>{x}</li>}</ul>
        <ul #b @dragover={(e) => { e.preventDefault() }} @drop={(e) => { e.preventDefault(); a = a | filter((x) => x != dragging); b = b + [dragging]; log = log + [`drop:${dragging}`] }}>{for x in b => <li key={x} class="card" data-x={x} draggable="true" @dragstart={() => { start(x) }} @dragend={() => { end() }}>{x}</li>}</ul>
        <p #log>{join(log, ",")}</p><p #dragging>{dragging}</p>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Board/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn a_card_dropped_into_another_list_hears_its_dragend() {
    let body = r##"const list = (id) => [...$(id).querySelectorAll(".card")].map((c) => c.dataset.x).join(",");
    const card = document.querySelector('#a .card[data-x="1"]');
    const dt = new DataTransfer();
    card.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: dt })); await tick(30);
    $("b").dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }));
    $("b").dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt })); await tick(30);
    const gone = !card.isConnected;
    card.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: dt })); await tick(30);
    return { gone, log: $("log").textContent, dragging: $("dragging").textContent, a: list("a"), b: list("b") };"##;
    let Some(rs) = in_pages("q896-drag", DRAG, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({ "gone": true, "log": "start:1,drop:1,end:1", "dragging": "0", "a": "2", "b": "3,1", "remade": 0, "errors": [] });
        assert_eq!(r, &want, "at {}", path);
    }
}
