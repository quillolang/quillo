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
    in_pages_args(name, src, paths, body, focus, &[])
}

/// The same, each page given `ms` (60 s otherwise), for one that waits out an outage.
fn in_pages_for(name: &str, src: &str, paths: &[&str], body: &str, focus: bool, ms: u64) -> Option<Vec<serde_json::Value>> {
    in_pages_given(name, src, paths, body, focus, &[], ms)
}

/// The same, with options for the server (`--watch`, for development bundles).
fn in_pages_args(name: &str, src: &str, paths: &[&str], body: &str, focus: bool, args: &[&str]) -> Option<Vec<serde_json::Value>> {
    in_pages_given(name, src, paths, body, focus, args, 60_000)
}

fn in_pages_given(name: &str, src: &str, paths: &[&str], body: &str, focus: bool, args: &[&str], ms: u64) -> Option<Vec<serde_json::Value>> {
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
            .args(args)
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
        // A page script that asks run.js for DevTools commands (`data-cdp` there).
        if body.contains("data-cdp") {
            node.env("QUILLO_PAGE_CDP", "1");
        } else {
            node.env_remove("QUILLO_PAGE_CDP");
        }
        let out = node
            .arg(&run)
            .arg(&chrome)
            .arg(format!("http://127.0.0.1:{}{}", port, path))
            .arg(app.0.join("profile"))
            .arg(ms.to_string())
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

// Q-889's shortcut, found in the second review: the page kept a note of the last part made, so a
// part that left the page stayed alive, with the panel around it and its 2,000 rows, until another
// part was made anywhere; changes of text alone make none. Each part keeps its own note now: a
// panel made in the browser and hidden goes at once, as on main, as it does after a part is made.
#[test]
fn a_part_that_leaves_the_page_is_let_go() {
    let src = r###"component Panel() {
    state on = true
    state other = false
    state n = 0
    let xs = range(0, 2000)
    <div>
        <button #tog @click={() => { on = !on }}>tog</button>
        <button #text @click={() => { n += 1 }}>text</button>
        <button #other @click={() => { other = !other }}>other</button>
        <p #n>{n}</p>
        {on ? <div class="panel"><ul>{for x in xs => <li key={x}>{x}</li>}</ul></div> : null}
        {other ? <ol>{for x in [1, 2] => <li key={x}>{x}</li>}</ol> : null}
    </div>
}
route GET / = <html><head><title>t</title></head><body><Panel/></body></html>
"###;
    let body = r#"const collect = async () => { for (let k = 0; k < 3; k++) { await tick(50); gc(); } };
    const out = {};
    await click("tog", "tog");
    let ref = new WeakRef(document.querySelector(".panel"));
    await click("tog", "text", "text", "text", "text", "text");
    await collect();
    out.textOnly = ref.deref() !== undefined;
    out.n = $("n").textContent;
    await click("tog");
    ref = new WeakRef(document.querySelector(".panel"));
    out.rows = document.querySelectorAll(".panel li").length;
    await click("tog", "other");
    await collect();
    out.afterAPart = ref.deref() !== undefined;
    return out;"#;
    let Some(rs) = in_pages("q889-gone", src, &["/"], body) else { return };
    let want = serde_json::json!({ "textOnly": false, "n": "5", "rows": 2000, "afterAPart": false, "remade": 0, "errors": [] });
    assert_eq!(rs[0], want);
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
// at all, as it never learned what to follow. A failure that passes (no connection, a 503, a 429) is
// tried again now, after a second, then two, doubling up to 30 s, or after the server's Retry-After,
// until a load comes, and the console is told once a run of failures. A refresh in the background
// keeps `waiting()` false meanwhile, as it always was, and its data on the page; a load for new
// arguments, or a first one, keeps it true until it comes. The page's fetch is wrapped here to fail
// the loads the test says: no connection, a 503, two in a row, one slow to fail with a change heard
// meanwhile, all of them for a while; a 403 and a page that is no answer (a sign-in page) are not
// tried again, as before, nor is an error value from the loader, which is an answer.
const LIVE_FAILS: &str = r###"db.open("live.db")
db.exec("create table if not exists t (id integer primary key, v int not null)")
db.exec("delete from t")
db.exec("insert into t (id, v) values (1, 0), (2, 200)")
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
    state id = 1
    <div>
        <button #show @click={() => { on = true }}>show</button>
        <button #off @click={() => { on = false }}>off</button>
        <button #two @click={() => { id = 2 }}>two</button>
        {on ? <V {id}/> : <p #gone>gone</p>}
    </div>
}
route POST /t/set/:v(int) { db.exec("update t set v = ? where id = 1", v); "ok" }
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /f = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

/// The page script's wrapper of `fetch`, for loads of `load_v` that fail: `fail` more of them, by
/// `mode` (no connection, a status, or a sign-in page), each after `delay` ms; while `down`, all of
/// them with a 503. `times` notes when each call began.
const FAILING_FETCH: &str = r##"const real = window.fetch.bind(window);
    const f = { fail: 0, mode: "net", delay: 0, down: false, calls: 0, times: [] };
    window.fetch = async (u, init) => {
      if (String(u).includes("/_quillo/rpc/load_v")) {
        f.calls++; f.times.push(performance.now());
        if (f.down || f.fail > 0) {
          if (!f.down) f.fail--;
          if (f.delay) await tick(f.delay);
          if (f.down || f.mode === "503") return new Response("busy", { status: 503 });
          if (f.mode === "403") return new Response("no", { status: 403 });
          if (f.mode === "429") return new Response("slow down", { status: 429, headers: { "retry-after": "2" } });
          if (f.mode === "page") return new Response("<html>sign in</html>", { status: 200, headers: { "content-type": "text/html" } });
          throw new TypeError("Failed to fetch");
        }
      }
      return real(u, init);
    };
    const set = (v) => real(`/t/set/${v}`, { method: "POST" });
    const failed = async (n) => { for (let i = 0; i < 200 && f.calls < n; i++) await tick(10); await tick(60); };"##;

#[test]
fn a_failed_live_load_tries_again_until_one_comes() {
    let body = format!(
        r##"{FAILING_FETCH}
    const shown = () => ($("v") ? $("v").textContent : null);
    const behind = () => $("v").classList.contains("behind");
    const until = async (want, ms) => {{ const t0 = performance.now(); while (performance.now() - t0 < ms) {{ if (shown() === String(want)) return Math.round(performance.now() - t0); await tick(20); }} return null; }};
    const out = {{}};
    if (location.pathname === "/f") {{
      // Made in the browser: its first load fails, and it waits for it.
      await set(0); f.fail = 1;
      await click("show"); await failed(1);
      out.first = [shown(), behind()];
      out.firstCaughtUp = (await until(0, 4000)) !== null && !behind();
      if (!out.firstCaughtUp) return out;
    }}
    // (Each case that does not catch up ends the test: the rest would wait out their time.)
    // A change, no failure: as it was.
    await set(1); out.plain = (await until(1, 3000)) !== null;
    if (!out.plain) return out;
    // One load that cannot connect, then one answered 503: the page keeps its data, not waiting,
    // and catches up after a wait.
    for (const [v, mode] of [[2, "net"], [3, "503"]]) {{
      f.mode = mode; f.fail = 1; const c = f.calls;
      await set(v); await failed(c + 1);
      out[mode] = [shown(), behind()];
      const t = await until(v, 4000);
      out[mode + "CaughtUp"] = t !== null && !behind() && t >= 300;
      if (!out[mode + "CaughtUp"]) return out;
    }}
    // Two failures in a row.
    {{ f.mode = "net"; f.fail = 2; const c = f.calls; await set(4); await failed(c + 2); out.twice = [shown(), behind(), f.calls - c]; out.twiceCaughtUp = (await until(4, 7000)) !== null; if (!out.twiceCaughtUp) return out; }}
    // A change heard while the load that then fails is on its way.
    {{ f.mode = "net"; f.fail = 1; f.delay = 300; const c = f.calls; await set(5); await tick(120); await set(6); await failed(c + 1); f.delay = 0; out.during = (await until(6, 5000)) !== null; if (!out.during) return out; }}
    // Every load refused for a while, then answered again; nothing changes meanwhile, and the page
    // is never waiting.
    {{ f.down = true; const c = f.calls; let ever = false; await set(7); await failed(c + 1);
      for (let i = 0; i < 80; i++) {{ ever = ever || behind(); await tick(20); }}
      out.down = [shown(), ever, f.calls - c >= 2]; f.down = false; out.back = (await until(7, 8000)) !== null && !behind(); }}
    // A 429 with Retry-After: 2 is tried again after two seconds at least.
    {{ f.mode = "429"; f.fail = 1; const c = f.calls; await set(70); await failed(c + 1); const ok = (await until(70, 6000)) !== null;
      const gap = f.times[c + 1] - f.times[c]; out.retryAfter = [ok, gap >= 1900, gap < 4500]; }}
    // A 403, and a sign-in page: not tried again, the data kept; the next change loads as before.
    for (const [v, mode] of [[71, "403"], [72, "page"]]) {{
      f.mode = mode; f.fail = 1; const c = f.calls;
      await set(v); await failed(c + 1); await tick(2500);
      out[mode] = [shown(), behind(), f.calls - c];
      await set(v + 10); out[mode + "Next"] = (await until(v + 10, 3000)) !== null;
    }}
    // An error value is an answer: shown, and not asked again.
    {{ const c = f.calls; await set(99); const ok = (await until("ninety-nine", 3000)) !== null; await tick(1500); out.errValue = [ok, behind(), f.calls - c]; await set(9); out.after = (await until(9, 3000)) !== null; }}
    // New arguments whose load fails: waiting until they are loaded, the old data shown meanwhile.
    {{ f.mode = "503"; f.fail = 1; const c = f.calls; await click("two"); await failed(c + 1); out.newArgs = [shown(), behind()];
      out.newArgsLoaded = [(await until(200, 4000)) !== null, behind()]; }}
    // A failure, then the component goes: no more tries.
    {{ f.mode = "net"; f.fail = 1; const c = f.calls; await set(8); await failed(c + 1); await click("off"); const n = f.calls; await tick(2500); out.gone = [shown(), f.calls - n]; }}
    return out;"##
    );
    let paths = ["/h", "/f"];
    let Some(rs) = in_pages("q899", LIVE_FAILS, &paths, &body) else { return };
    let net = "server fn `load_v` could not be reached: Failed to fetch";
    let busy = "server fn `load_v` failed (503)";
    for (r, path) in rs.iter().zip(paths) {
        let mut want = serde_json::json!({
            "plain": true, "net": ["1", false], "netCaughtUp": true, "503": ["2", false], "503CaughtUp": true,
            "twice": ["3", false, 2], "twiceCaughtUp": true, "during": true, "down": ["6", false, true], "back": true,
            "retryAfter": [true, true, true],
            "403": ["70", false, 1], "403Next": true, "page": ["81", false, 1], "pageNext": true,
            "errValue": [true, false, 1], "after": true,
            "newArgs": ["9", true], "newArgsLoaded": [true, false], "gone": [null, 0], "remade": 0,
            // Once a run of failures: the two in a row, and all of those while it was down, are one.
            "errors": [net, busy, net, net, busy, "server fn `load_v` failed (429)", "server fn `load_v` failed (403)",
                "server fn `load_v` answered something that is not its answer (text/html, 20 characters)", busy, net],
        });
        if path == "/f" {
            want["first"] = serde_json::json!(["none", true]);
            want["firstCaughtUp"] = serde_json::json!(true);
            want["errors"].as_array_mut().unwrap().insert(0, serde_json::json!(net));
        }
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-899's review: a load of a component that went while the load was on its way, and failed, armed a
// try after the component's cleanup had run, and the tries went on for good. The component goes
// here while a failing load is on its way: by the button, and by the person moving to another page
// during an outage, with a try on its way. No load is asked for after either.
const LIVE_GONE: &str = r###"db.open("gone.db")
db.exec("create table if not exists t (id integer primary key, v int not null)")
db.exec("delete from t")
db.exec("insert into t (id, v) values (1, 0)")
server fn load_v(id: int) -> list = db.query("select v from t where id = ?", id)
component V() {
    state r = live(load_v, 1)
    <p #v>{r == null ? "none" : r[0].v}</p>
}
component App() {
    let path = url().path
    state on = true
    <div>
        <a #away href="/g/away">away</a>
        <button #off @click={() => { on = false }}>off</button>
        {path == "/g" && on ? <V/> : <p #none>none</p>}
    </div>
}
route POST /t/set/:v(int) { db.exec("update t set v = ? where id = 1", v); "ok" }
route GET /g = <html><head><title>t</title></head><body><App/></body></html>
route GET /g/:x = <html><head><title>t</title></head><body><App/></body></html>
"###;

#[test]
fn a_failed_load_of_a_component_that_went_is_not_tried_again() {
    let body = format!(
        r##"{FAILING_FETCH}
    const shown = () => ($("v") ? $("v").textContent : null);
    const until = async (want, ms) => {{ const t0 = performance.now(); while (performance.now() - t0 < ms) {{ if (shown() === String(want)) return true; await tick(20); }} return false; }};
    const out = {{}};
    // The socket is up once a change shows.
    await set(1); out.up = await until(1, 3000);
    // Gone by the button 150 ms into a load that fails after 600.
    {{ f.down = true; f.delay = 600; const c = f.calls; await set(2); await failed(c + 1); await tick(90); await click("off");
      await tick(4000); out.button = [!!$("v"), f.calls - c]; }}
    return out;"##
    );
    let away = format!(
        r##"{FAILING_FETCH}
    const shown = () => ($("v") ? $("v").textContent : null);
    const until = async (want, ms) => {{ const t0 = performance.now(); while (performance.now() - t0 < ms) {{ if (shown() === String(want)) return true; await tick(20); }} return false; }};
    const out = {{}};
    await set(1); out.up = await until(1, 3000);
    // An outage whose loads fail after 400 ms: the person leaves with the second try on its way.
    {{ f.down = true; f.delay = 400; const c = f.calls; await set(2); for (let i = 0; i < 400 && f.calls < c + 2; i++) await tick(10);
      await tick(50); await click("away"); const n = f.calls; await tick(4000); out.away = [location.pathname, !!$("v"), f.calls - n]; }}
    return out;"##
    );
    let Some(rs) = in_pages("q899-gone", LIVE_GONE, &["/g"], &body) else { return };
    // The failure that comes once the component has gone says nothing.
    assert_eq!(rs[0], serde_json::json!({ "up": true, "button": [false, 1], "remade": 0, "errors": [] }));
    let Some(rs) = in_pages("q899-away", LIVE_GONE, &["/g"], &away) else { return };
    assert_eq!(rs[0], serde_json::json!({ "up": true, "away": ["/g/away", false, 0], "remade": 0, "errors": ["server fn `load_v` failed (503)"] }));
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

// Q-901's review: code in a timer's or a stream's callback belonged to no one, so what it started
// outlived the component: a `run_after` started by an `every` tick, by another `run_after` or by a
// `listen` message fired after the component had gone, as did one started by the ticks of an
// `every` in the component's own code. They belong to the component now, and go with it; one in a
// component that stays fires, once.
const NESTED_TIMERS: &str = r###"route GET /ticks = events((send) => {
    for i in range(100000) {
        send(i)
        sleep(40)
    }
})
component Alive(tick) {
    <div>
        <button #evra @click={() => { every(50, () => { run_after(300, () => { tick("evra") }) }) }}>evra</button>
        <button #rara @click={() => { run_after(50, () => { run_after(300, () => { tick("rara") }) }) }}>rara</button>
        <button #lsra @click={() => { listen("/ticks", (n) => { run_after(300, () => { tick("lsra") }) }) }}>lsra</button>
        <button #stays @click={() => { run_after(50, () => { run_after(100, () => { tick("stays") }) }) }}>stays</button>
    </div>
}
component Ticking(tick) {
    every(50, () => { run_after(300, () => { tick("code") }) })
    <p>ticking</p>
}
component Host(now: bool) {
    state on = now
    state shown = true
    state ticking = false
    state log = []
    let tick = (s) => { log = log + [s] }
    <div>
        <button #show @click={() => { on = true }}>show</button>
        <button #hide @click={() => { shown = false }}>hide</button>
        <button #again @click={() => { shown = true }}>again</button>
        <button #tick @click={() => { ticking = !ticking }}>ticking</button>
        <button #clear @click={() => { log = [] }}>clear</button>
        {on && shown ? <Alive {tick}/> : null}
        {on && ticking ? <Ticking {tick}/> : null}
        <p #log>{join(log, ",")}</p>
    </div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn what_a_timer_or_a_stream_starts_goes_with_its_component() {
    let body = r##"const n = (s) => $("log").textContent.split(",").filter((t) => t === s).length;
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(10); return f(); };
    const out = {};
    // Each hidden before the first `run_after` it started would fire (the stream's first message
    // comes at once).
    for (const [id, ms] of [["evra", 110], ["rara", 150], ["lsra", 150]]) {
      await click(id); await tick(ms); await click("hide"); await tick(700); out[id] = n(id);
      await click("again", "clear");
    }
    await click("stays"); await until(() => n("stays") > 0, 3000); await tick(300); out.stays = n("stays");
    await click("clear", "tick"); await tick(120); await click("tick"); await tick(700); out.code = n("code");
    return out;"##;
    let Some(rs) = in_pages("q901-nested", NESTED_TIMERS, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        assert_eq!(r, &serde_json::json!({ "evra": 0, "rara": 0, "lsra": 0, "stays": 1, "code": 0, "remade": 0, "errors": [] }), "at {}", path);
    }
}

// Q-902: a save that ended after the person had followed a link elsewhere took them on to where it
// led. A `navigate` after a wait does nothing now when, since its run began, the person went to
// another page (a link, Back, also between two waits) or a run begun after it moved the page (a
// click that navigates at once). As for a state, the latest call wins: of two searches that each
// show their answer and then go to its page, the later one's page is where the person ends, whichever
// answers first (it was the earlier's, once that had moved the page first). Whether the clicked
// component is still there does not matter: a modal that its Save closes, a panel a sibling state
// takes away, a row or a branch in a branch gone, all move as they did; so do a navigate before any
// wait, one after a wait or two with nothing else moving the page, the run's own move followed by a
// wait and another move (two calls of one `parallel` too), and a change of the hash alone, which
// keeps the page.
const NAVIGATES: &str = r###"server fn slow(ms: int) -> str { sleep(ms); "x" }
server fn search(ms: int, q: str) -> str { sleep(ms); `results for ${q}` }
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
    state shown = ""
    <div>
        <a #other href="/q/other">other</a>
        <a #home href="/q">home</a>
        <button #a1 @click={() => { shown = search(300, "a"); navigate("/q/s-a") }}>a</button>
        <button #b1 @click={() => { shown = search(300, "b"); navigate("/q/s-b") }}>b</button>
        <button #a2 @click={() => { shown = search(500, "a"); navigate("/q/s-a") }}>a, slow</button>
        <button #b2 @click={() => { shown = search(100, "b"); navigate("/q/s-b") }}>b, quick</button>
        <p #shown>{shown}</p>
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
    await click("save"); await tick(20); await click("tospot"); await moved(); out.hash = [at(), location.hash]; await home();
    // Two searches, the second clicked 80 ms after the first: the first answers first, or last.
    for (const [a, b] of [["a1", "b1"], ["a2", "b2"]]) {
      await click(a); await tick(80); await click(b);
      await until(() => $("shown").textContent === "results for b", 4000); await tick(700);
      out[a + b] = [at(), $("shown").textContent]; await home();
    }
    return out;"##;
    let Some(rs) = in_pages("q902", NAVIGATES, &["/q"], body) else { return };
    let want = serde_json::json!({
        "now": ["/q/now", "/q/now"], "save": "/q/done", "twice": "/q/twice", "row": "/q/row", "deep": "/q/deep",
        "modal": [true, "/q/item"], "panel": [true, "/q/sib"], "own": ["/q/step", "/q/after"], "par": ["/q/p1", "/q/p2"],
        "left": ["/q/other", "/q/other"], "leftBetween": "/q/other", "back": "/q/other", "otherRun": "/q/now",
        "hash": ["/q/done", ""], "a1b1": ["/q/s-b", "results for b"], "a2b2": ["/q/s-b", "results for b"],
        "remade": 0, "errors": [],
    });
    assert_eq!(rs[0], want);
}

// Q-902 on a page where nothing reads url(): `navigate` loads the page there, and a link or a form
// the person used meanwhile, whose page takes a while, was cancelled by it. The person's leaving is
// heard as they click or send; the runtime's own load of the page is not. A link to a spot on the
// same page keeps it, and with nothing done meanwhile the save goes where it says. Each page this
// lands on writes where it is as the result, as the page the test began on has gone. A link or a
// form whose answer keeps the page (a 204) counts all the same, as the click is all the page sees:
// the save stays where the person is. A save whose own move is to an address answered 204 leaves the
// page as it is, and its run's move is noted for runs begun before it (CI's coverage: a move that
// loads another page took its record with it).
const PLAIN_NAVIGATES: &str = r###"server fn slow(ms: int) -> str { sleep(ms); "x" }
component Saver() {
    <div>
        <button #save @click={() => { slow(300); navigate("/done") }}>save</button>
        <button #save204 @click={() => { slow(300); navigate("/nothing") }}>save to nothing</button>
        <a #slow href="/slow">slow</a>
        <form #f action="/slow" method="get"><button #send>send</button></form>
        <a #spot href="#spot">spot</a>
        <a #nothing href="/nothing">nothing</a>
        <form #f2 action="/nothing" method="get"><button #send2>send</button></form>
    </div>
}
fn landed(name: str) = <html><head><title>t</title></head><body><p>{name}</p><script>{raw("document.body.dataset.result = JSON.stringify({ at: location.pathname })")}</script></body></html>
fn start() = <html><head><title>t</title></head><body><Saver/></body></html>
route GET /p/link = start()
route GET /p/none = start()
route GET /p/form = start()
route GET /p/spot = start()
route GET /p/stay = start()
route GET /p/stayform = start()
route GET /p/to204 = start()
route GET /nothing = null
route GET /slow { sleep(1500); landed("slow") }
route GET /done = landed("done")
"###;

#[test]
fn a_late_navigate_does_not_cancel_a_page_the_person_asked_for() {
    let body = r#"const p = location.pathname;
    await click(p === "/p/to204" ? "save204" : "save");
    if (p === "/p/link") $("slow").click();
    if (p === "/p/form") $("f").requestSubmit();
    if (p === "/p/spot") $("spot").click();
    if (p === "/p/stay") $("nothing").click();
    if (p === "/p/stayform") $("f2").requestSubmit();
    if (p === "/p/stay" || p === "/p/stayform" || p === "/p/to204") { await tick(700); return { at: location.pathname }; }
    // The page this lands on says where it is.
    await new Promise(() => {});"#;
    let paths = ["/p/link", "/p/none", "/p/form", "/p/spot", "/p/stay", "/p/stayform", "/p/to204"];
    let Some(rs) = in_pages("q902-plain", PLAIN_NAVIGATES, &paths, body) else { return };
    let got: Vec<&str> = rs.iter().map(|r| r["at"].as_str().unwrap_or("")).collect();
    assert_eq!(got, ["/slow", "/done", "/slow", "/done", "/p/stay", "/p/stayform", "/p/to204"]);
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

// ---- A live page's socket (Q-900) --------------------------------------------------------------
// A page that moved to a page following other topics closed its socket and opened another, and the
// server took a close only at its next look, every 5 s, then dropped the connection unanswered: every
// close ended as 1006, a failed connection, after which Firefox held back the page's next socket and
// the page heard nothing for 10 to 45 s and more. A close is answered now at once, with its code, and
// the page's thread and place are let go; a page sends its new topics down the socket it has, as a
// line `since=N&s=S` and the topics, answered at once with where they stand, once its server has said
// the socket takes them (`takes`); and a socket that has not opened in 3 s gives way to the plain
// request. The server's side is tried with a
// plain client, the page's in Chrome.

const LIVE_SOCKET: &str = "route POST /bump/*t {\n    publish(t)\n    \"ok\"\n}\nroute GET / = \"ok\"\n";

/// A server for one program, of the quillo `quillo()` gives, with its metrics.
struct Served {
    _server: support::Guard,
    _app: App,
    port: u16,
}

fn served(name: &str, src: &str) -> Served {
    let app = App::new(name, src);
    checked(&app.0);
    let port = support::free_port();
    let server = support::Guard::new(
        Command::new(quillo())
            .arg("run")
            .arg(app.0.join("main.qwe"))
            .args(["--port", &port.to_string(), "--metrics"])
            .current_dir(&app.0)
            .env("QUILLO_SECRET", "a test secret for live data, long enough to use")
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
    Served { _server: server, _app: app, port }
}

impl Served {
    fn metric(&self, name: &str) -> i64 {
        let text = support::send(self.port, "GET", "/_quillo/metrics", &[], "").body;
        text.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.trim().parse().ok())).unwrap_or_else(|| panic!("no {} in\n{}", name, text))
    }

    /// The live data's number now.
    fn seq(&self) -> u64 {
        let a = support::send(self.port, "GET", "/_quillo/live?since=0&t=none", &[], "");
        serde_json::from_str::<serde_json::Value>(&a.body).unwrap_or_else(|e| panic!("{}: {}", e, a.body))["seq"].as_u64().unwrap()
    }

    /// A change to topic `t` (written as a path).
    fn bump(&self, t: &str) {
        assert_eq!(support::send(self.port, "POST", &format!("/bump/{}", t), &[], "").body, "ok");
    }

    /// Whether `f` holds within `ms`.
    fn within(&self, ms: u64, f: impl Fn(&Served) -> bool) -> bool {
        let until = Instant::now() + Duration::from_millis(ms);
        loop {
            if f(self) {
                return true;
            }
            if Instant::now() > until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A page's live socket, by hand.
struct LiveWs(std::net::TcpStream);

/// `/_quillo/live?` and `query` as a socket; the status line if it is refused.
fn live_ws(port: u16, query: &str) -> Result<LiveWs, String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET /_quillo/live?{} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n", query).unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read_exact(&mut b).is_err() {
            break;
        }
        head.push(b[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    if head.starts_with("HTTP/1.1 101") {
        Ok(LiveWs(s))
    } else {
        Err(head.lines().next().unwrap_or("").to_string())
    }
}

impl LiveWs {
    /// A frame as a page sends it: masked.
    fn send(&mut self, op: u8, fin: bool, data: &[u8]) {
        use std::io::Write;
        let _ = self.0.write_all(&ws_frame(op, fin, data));
    }

    fn text(&mut self, t: &str) {
        self.send(0x1, true, t.as_bytes());
    }

    /// The server's next frame within `wait`, its opcode and payload; none if none came.
    fn frame(&mut self, wait: Duration) -> Option<(u8, Vec<u8>)> {
        use std::io::Read;
        self.0.set_read_timeout(Some(wait)).ok()?;
        let mut h = [0u8; 2];
        self.0.read_exact(&mut h).ok()?;
        let mut n = (h[1] & 0x7f) as u64;
        if n == 126 {
            let mut l = [0u8; 2];
            self.0.read_exact(&mut l).ok()?;
            n = u16::from_be_bytes(l) as u64;
        } else if n == 127 {
            let mut l = [0u8; 8];
            self.0.read_exact(&mut l).ok()?;
            n = u64::from_be_bytes(l);
        }
        let mut data = vec![0u8; n as usize];
        self.0.read_exact(&mut data).ok()?;
        Some((h[0] & 0x0f, data))
    }

    /// The next message the server sends within `wait`, pings passed over; none if none came, or a
    /// close did.
    fn told(&mut self, wait: Duration) -> Option<serde_json::Value> {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let (op, data) = self.frame(left.max(Duration::from_millis(1)))?;
            match op {
                0x1 => return Some(serde_json::from_slice(&data).unwrap()),
                0x9 => continue,
                _ => return None,
            }
        }
    }

    /// Whether the server ends the connection within `wait`.
    fn ended(&mut self, wait: Duration) -> bool {
        use std::io::Read;
        if self.0.set_read_timeout(Some(wait)).is_err() {
            return true;
        }
        let mut b = [0u8; 64];
        loop {
            match self.0.read(&mut b) {
                Ok(0) => return true,
                Ok(_) => continue,
                Err(e) => return matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe),
            }
        }
    }
}

fn ws_frame(op: u8, fin: bool, data: &[u8]) -> Vec<u8> {
    let mut f = vec![if fin { 0x80 } else { 0 } | op];
    if data.len() < 126 {
        f.push(0x80 | data.len() as u8);
    } else if data.len() <= 0xffff {
        f.push(0x80 | 126);
        f.extend_from_slice(&(data.len() as u16).to_be_bytes());
    } else {
        f.push(0x80 | 127);
        f.extend_from_slice(&(data.len() as u64).to_be_bytes());
    }
    let mask = [7, 1, 3, 9];
    f.extend_from_slice(&mask);
    f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// Topics as a page writes them (`liveKey`): each `%` and `,` in them written out.
fn live_key(topics: &[&str]) -> String {
    topics.iter().map(|t| t.replace('%', "%25").replace(',', "%2C")).collect::<Vec<_>>().join(",")
}

#[test]
fn a_close_is_answered_at_once_with_its_code() {
    let s = served("q900-close", LIVE_SOCKET);
    let q = format!("since={}&s=&t=a", s.seq());
    let code = |c: u16| c.to_be_bytes().to_vec();
    let mut reason = code(1001);
    reason.extend_from_slice("bye ✓".as_bytes());
    // What the page sends, and the close it is answered with: its own code, none for none, and 1002
    // (a protocol error) for a code a page may not send or a payload of one byte.
    let cases: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
        ("1000", code(1000), code(1000)),
        ("1001 with a reason", reason, code(1001)),
        ("4000, a program's own", code(4000), code(4000)),
        ("no code", vec![], vec![]),
        ("1005, never sent", code(1005), code(1002)),
        ("one byte", vec![3], code(1002)),
        // Longer than a control frame may be (RFC 6455 §5.5): failed with 1002, where it went
        // unanswered (found in review).
        ("126 bytes", { let mut c = code(1000); c.resize(126, b'r'); c }, code(1002)),
    ];
    for (name, theirs, ours) in cases {
        let mut p = live_ws(s.port, &q).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let t0 = Instant::now();
        p.send(0x8, true, &theirs);
        let got = p.frame(Duration::from_secs(10));
        let took = t0.elapsed();
        assert_eq!(got, Some((0x8, ours)), "{}", name);
        assert!(took < Duration::from_secs(1), "{}: answered after {:?}", name, took);
        assert!(p.ended(Duration::from_secs(2)), "{}: the connection stays", name);
    }
    // A pong, then a close, in one write; and a close after a message of topics.
    let mut p = live_ws(s.port, &q).unwrap();
    let mut both = ws_frame(0xA, true, b"");
    both.extend(ws_frame(0x8, true, &code(1000)));
    use std::io::Write;
    p.0.write_all(&both).unwrap();
    assert_eq!(p.frame(Duration::from_secs(1)), Some((0x8, code(1000))));
    let mut p = live_ws(s.port, &q).unwrap();
    let mut both = ws_frame(0x1, true, b"since=1&s=\nb");
    both.extend(ws_frame(0x8, true, &code(1000)));
    p.0.write_all(&both).unwrap();
    assert_eq!(p.frame(Duration::from_secs(1)).map(|f| f.0), Some(0x8));
    // Every page's thread has ended, at once.
    assert!(s.within(1000, |s| s.metric("quillo_live_pages") == 0), "{} pages", s.metric("quillo_live_pages"));
    // Pages that hang up with no close, cleanly or not, are let go at once too.
    let pages: Vec<LiveWs> = (0..3).map(|_| live_ws(s.port, &q).unwrap()).collect();
    assert!(s.within(1000, |s| s.metric("quillo_live_pages") == 3));
    let mut pages = pages.into_iter();
    let first = pages.next().unwrap();
    first.0.shutdown(std::net::Shutdown::Write).unwrap();
    drop(pages);
    assert!(s.within(1000, |s| s.metric("quillo_live_pages") == 0), "{} pages", s.metric("quillo_live_pages"));
    drop(first);
}

#[test]
fn topics_sent_down_the_open_socket_are_followed() {
    let s = served("q900-topics", LIVE_SOCKET);
    let seq = s.seq();
    let mut p = live_ws(s.port, &format!("since={}&s=&t=a", seq)).unwrap();
    let head = |since: u64, server: &str| format!("since={}&s={}\n", since, server);
    let quiet = Duration::from_millis(400);
    let soon = Duration::from_secs(2);
    // New topics: answered at once, nothing having changed, in the socket's first message, which
    // says it takes them.
    let t0 = Instant::now();
    p.text(&format!("{}b", head(seq, "")));
    let j = p.told(soon).expect("answered");
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_eq!((&j["changed"], &j["takes"], j["seq"].as_u64()), (&serde_json::json!([]), &serde_json::json!("topics"), Some(seq)));
    // The old topic is followed no more, the new one is; no later message says so again.
    s.bump("a");
    assert_eq!(p.told(quiet), None);
    s.bump("b");
    let j = p.told(soon).expect("told of b");
    assert_eq!(j["changed"], serde_json::json!(["b"]));
    assert!(j.get("takes").is_none(), "{}", j);
    // A change made after what the page knows and before the server has its topics (in flight): in
    // the answer.
    let knows = j["seq"].as_u64().unwrap();
    s.bump("c");
    p.text(&format!("{}c,b", head(knows, "")));
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["c"]));
    // None at all: answered, and nothing told; then one topic again.
    p.text(&head(s.seq(), ""));
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!([]));
    s.bump("c");
    assert_eq!(p.told(quiet), None);
    p.text(&format!("{}d", head(s.seq(), "")));
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!([]));
    s.bump("d");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["d"]));
    // Unicode, a comma inside a topic, and the message in two frames.
    p.send(0x1, false, head(s.seq(), "").as_bytes());
    p.send(0x0, true, live_key(&["猫", "é,x"]).as_bytes());
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!([]));
    s.bump("%C3%A9%2Cx");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["é,x"]));
    s.bump("%E7%8C%AB");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["猫"]));
    // A number from another server, or from before a restart: every topic counts as changed.
    p.text("since=5&s=another\nx,y");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["x", "y"]));
    // What is no message of topics is passed over, and the topics stay: text without the line, a
    // binary message, a line too long.
    p.text("zzz");
    p.send(0x2, true, b"since=1&s=\nq");
    p.text(&format!("since=1&s={}\nq", "s".repeat(200)));
    assert_eq!(p.told(quiet), None);
    s.bump("y");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["y"]));
    // Two sets in one write: the last is followed.
    let mut two = ws_frame(0x1, true, format!("{}e", head(s.seq(), "")).as_bytes());
    two.extend(ws_frame(0x1, true, format!("{}f", head(s.seq(), "")).as_bytes()));
    use std::io::Write;
    p.0.write_all(&two).unwrap();
    while let Some(j) = p.told(quiet) {
        assert_eq!(j["changed"], serde_json::json!([]), "{}", j);
    }
    s.bump("e");
    assert_eq!(p.told(quiet), None);
    s.bump("f");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["f"]));
    // Past the address's limit, up to the most a page follows (as `tm=1` takes them), the line
    // before them counted: taken. One byte more: the socket is let go, as a first message that long
    // is (decided by the coordinator after review), with a close of 1009 when it is read before the
    // connection ends, so that its page opens another and is told there.
    const MOST: usize = 128 + 256 * (448 * 3 + 1);
    let longest = ",".repeat(448);
    let mut topics = vec!["q900:t"];
    topics.extend((0..255).map(|_| longest.as_str()));
    let line = head(s.seq(), "");
    let key = live_key(&topics);
    let at_limit = format!("{}{}{}", line, key, ",".repeat(MOST - line.len() - key.len()));
    assert_eq!(at_limit.len(), MOST);
    p.text(&at_limit);
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!([]));
    s.bump("q900:t");
    assert_eq!(p.told(soon).unwrap()["changed"], serde_json::json!(["q900:t"]));
    p.text(&format!("{},", at_limit));
    if let Some(f) = p.frame(Duration::from_secs(2)) {
        assert_eq!(f, (0x8, 1009u16.to_be_bytes().to_vec()), "one byte more");
    }
    assert!(p.ended(Duration::from_secs(2)), "one byte more: the socket stays");
    // The form that worked: topics as the first message of a socket opened with `tm=1`.
    let mut later = live_ws(s.port, &format!("since={}&s=&tm=1", s.seq())).unwrap();
    later.text("g");
    s.bump("g");
    let j = later.told(soon).unwrap();
    assert_eq!((&j["changed"], &j["takes"]), (&serde_json::json!(["g"]), &serde_json::json!("topics")));
    later.text(&format!("{}h", head(s.seq(), "")));
    assert_eq!(later.told(soon).unwrap()["changed"], serde_json::json!([]));
    s.bump("h");
    assert_eq!(later.told(soon).unwrap()["changed"], serde_json::json!(["h"]));
    // A page that asks (`tk=1`, as the runtime does) is told at once, where a socket whose page knows
    // where things stand hears nothing until a change.
    let t0 = Instant::now();
    let mut asks = live_ws(s.port, &format!("since={}&s=&tk=1&t=i", s.seq())).unwrap();
    let j = asks.told(soon).expect("told at once");
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_eq!((&j["changed"], &j["takes"]), (&serde_json::json!([]), &serde_json::json!("topics")));
    s.bump("i");
    let j = asks.told(soon).unwrap();
    assert_eq!((&j["changed"], j.get("takes")), (&serde_json::json!(["i"]), None));
}

#[test]
fn a_clients_places_come_back_as_its_pages_close() {
    let s = served("q900-places", LIVE_SOCKET);
    let seq = s.seq();
    let q = format!("since={}&s=&t=a", seq);
    let mut held: Vec<LiveWs> = (0..256).map(|i| live_ws(s.port, &q).unwrap_or_else(|e| panic!("page {}: {}", i + 1, e))).collect();
    assert!(live_ws(s.port, &q).err().is_some_and(|e| e.contains("503")), "page 257");
    // A page that closes gives its place back at once, and so does one that hangs up.
    let free = |s: &Served| -> Option<LiveWs> {
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            if let Ok(p) = live_ws(s.port, &q) {
                return Some(p);
            }
            if Instant::now() > until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let mut one = held.pop().unwrap();
    one.send(0x8, true, &1000u16.to_be_bytes());
    assert_eq!(one.frame(Duration::from_secs(2)).map(|f| f.0), Some(0x8));
    held.push(free(&s).expect("the closed page's place"));
    drop(held.pop());
    held.push(free(&s).expect("the place of the page that hung up"));
    assert!(live_ws(s.port, &q).is_err(), "page 257, again");
    // New topics down every socket take no place more, and each of the 256 hears its own.
    for p in &mut held {
        p.text(&format!("since={}&s=\nb", seq));
    }
    for p in &mut held {
        assert_eq!(p.told(Duration::from_secs(5)).expect("answered")["changed"], serde_json::json!([]));
    }
    assert!(live_ws(s.port, &q).is_err(), "still 256");
    s.bump("b");
    let told = held.iter_mut().map(|p| p.told(Duration::from_secs(5))).filter(|j| j.as_ref().is_some_and(|j| j["changed"] == serde_json::json!(["b"]))).count();
    assert_eq!(told, 256);
}

/// Two pages following different tables, moved between by links (the page reads `url()`). A script in
/// the head counts the page's sockets; at `/never` they never open, at `/late` each opens 5 s after it
/// is made (as a browser holding sockets back lets one through), at `/fails` the first two never open
/// and fail at 4 s (as a browser's own limit ends one, or a network refusing sockets slowly), at
/// `/outage` sockets and plain requests fail after `?d=` ms once the page sets `__down` (a proxy
/// whose upstream is down answers 504 after its connect timeout), and at `/old` the server's word
/// that a socket takes topics is taken out of its messages, as from a server that does not say it
/// (an older one, or one that cannot watch its sockets).
const PAGES_MOVED: &str = r###"db.open("moves.db")
db.exec("create table if not exists a (id integer primary key, v int not null)")
db.exec("create table if not exists b (id integer primary key, v int not null)")
db.exec("delete from a")
db.exec("delete from b")
db.exec("insert into a (id, v) values (1, 0)")
db.exec("insert into b (id, v) values (1, 0)")
server fn load_a() = db.query("select v from a where id = 1")[0].v
server fn load_b() = db.query("select v from b where id = 1")[0].v
component A() {
    state v = live(load_a)
    <p #va>{v}</p>
}
component B() {
    state v = live(load_b)
    <p #vb>{v}</p>
}
component App() {
    let path = url().path
    let base = split(path, "/")[1]
    <div>
        <a #toa href={`/${base}/a`}>a</a>
        <a #tob href={`/${base}/b`}>b</a>
        {ends_with(path, "/a") ? <A/> : <B/>}
    </div>
}
let count = "window.__made = 0; window.__heard = 0; { const Real = WebSocket; window.WebSocket = class extends Real { constructor(...a) { super(...a); window.__made++; this.addEventListener('message', () => { window.__heard++; }); } }; }"
let never = "window.__made = 0; window.WebSocket = class { constructor() { window.__made++; this.readyState = 0; } send() {} close() { this.readyState = 3; } };"
let fails = "window.__made = 0; window.__failed = []; window.__madeAt = []; { const Real = WebSocket; window.WebSocket = class { constructor(u) { window.__made++; window.__madeAt.push(performance.now()); if (window.__made > 2) return new Real(u); this.readyState = 0; setTimeout(() => { if (this.readyState === 3) return; window.__failed.push(performance.now()); this.readyState = 3; if (this.onclose) this.onclose({ code: 1006 }); }, 4000); } send() {} close() { this.readyState = 3; } }; }"
let outage = "window.__made = 0; window.__asks = []; window.__down = false; window.__socks = []; { const d = +new URLSearchParams(location.search).get('d') || 0; const Real = WebSocket, realFetch = window.fetch.bind(window); window.WebSocket = class { constructor(u) { window.__made++; if (!window.__down) { const r = new Real(u); window.__socks.push(r); return r; } this.readyState = 0; setTimeout(() => { if (this.readyState === 3) return; this.readyState = 3; if (this.onclose) this.onclose({ code: 1006 }); }, d); } send() {} close() { this.readyState = 3; } }; window.fetch = (u, init) => { if (window.__down && String(u).includes('/_quillo/live?')) { window.__asks.push(performance.now()); return new Promise((ok) => setTimeout(() => ok(new Response('down', { status: 504 })), d)); } return realFetch(u, init); }; }"
let late = "window.__made = 0; window.__opened = 0; { const Real = WebSocket; window.WebSocket = class { constructor(u) { window.__made++; this.readyState = 0; setTimeout(() => { if (this.readyState === 3) return; const r = this.r = new Real(u); r.onopen = (e) => { this.readyState = 1; window.__opened = performance.now(); if (this.onopen) this.onopen(e); }; r.onmessage = (e) => { if (this.onmessage) this.onmessage(e); }; r.onclose = (e) => { this.readyState = 3; if (this.onclose) this.onclose(e); }; }, 5000); } send(d) { if (this.r) this.r.send(d); } close() { this.readyState = 3; if (this.r) this.r.close(); } }; }"
let old = "window.__made = 0; { const Real = WebSocket; window.WebSocket = class extends Real { constructor(...a) { super(...a); window.__made++; } set onmessage(f) { super.onmessage = (m) => f({ data: String(m.data).replace(',\"takes\":\"topics\"', '') }); } }; }"
fn page(head: str) = <html><head><title>t</title><script>{raw(head)}</script></head><body><App/></body></html>
route POST /t/a/:v(int) { db.exec("update a set v = ? where id = 1", v); "ok" }
route POST /t/b/:v(int) { db.exec("update b set v = ? where id = 1", v); "ok" }
route GET /m/:x = page(count)
route GET /never/:x = page(never)
route GET /late/:x = page(late)
route GET /fails/:x = page(fails)
route GET /outage/:x = page(outage)
route GET /old/:x = page(old)
"###;

/// The page script's helpers: what `#va` or `#vb` shows, how long until it shows a value, a write.
const MOVES_JS: &str = r##"const shown = (t) => ($("v" + t) ? $("v" + t).textContent : null);
    const until = async (t, want, ms) => { const t0 = performance.now(); while (performance.now() - t0 < ms) { if (shown(t) === String(want)) return Math.round(performance.now() - t0); await tick(20); } return null; };
    const write = (t, v) => fetch(`/t/${t}/${v}`, { method: "POST" });"##;

#[test]
fn a_page_that_moves_keeps_its_socket_and_hears_each_pages_changes() {
    let body = format!(
        r##"{MOVES_JS}
    const wrote = {{ a: 0, b: 0 }}, seen = [];
    // Once the socket has said it takes topics, in its first message.
    for (let i = 0; i < 100 && !window.__heard; i++) await tick(20);
    for (let i = 1; i <= 8; i++) {{
      const t = i % 2 ? "b" : "a";
      $("to" + t).click();
      if ((await until(t, wrote[t], 3000)) === null) return {{ lost: i }};
      wrote[t] = i * 10;
      await write(t, wrote[t]);
      seen.push((await until(t, wrote[t], 3000)) !== null);
    }}
    return {{ seen, made: window.__made }};"##
    );
    let Some(rs) = in_pages("q900-moves", PAGES_MOVED, &["/m/a"], &body) else { return };
    // One socket for all eight pages, where there was one a page.
    assert_eq!(rs[0], serde_json::json!({ "seen": [true, true, true, true, true, true, true, true], "made": 1, "remade": 0, "errors": [] }));
}

#[test]
fn a_socket_that_never_opens_gives_way_to_the_plain_request() {
    let body = format!(
        r##"{MOVES_JS}
    const t0 = performance.now();
    await write("a", 5);
    const first = await until("a", 5, 8000);
    if (first === null) return {{ first, made: window.__made }};
    const at = Math.round(performance.now() - t0);
    // The next change comes down the plain request already waiting, not after another socket's 3 s.
    await write("a", 6);
    const second = await until("a", 6, 8000);
    return {{ firstAfter3s: at >= 2000 && at <= 6000, secondSoon: second !== null && second < 1500, made: window.__made }};"##
    );
    let Some(rs) = in_pages("q900-never", PAGES_MOVED, &["/never/a"], &body) else { return };
    // The socket is never given up: it goes on connecting, as the browser has it.
    assert_eq!(rs[0], serde_json::json!({ "firstAfter3s": true, "secondSoon": true, "made": 1, "remade": 0, "errors": [] }));
}

// Found in review of Q-900: a socket that had not opened in 3 s was closed, still connecting, and
// Firefox, holding a host's sockets back after others failed, held the next as long again; eight tabs
// whose sockets were cut at once stayed off them for 25 to 55 s. It is kept connecting now, the plain
// request carrying the changes meanwhile, and takes over when it opens: the request on its way ends,
// none is made after, and a change the requests told is neither missed nor loaded twice.
#[test]
fn a_socket_that_opens_late_takes_over_from_the_plain_request() {
    let body = format!(
        r##"{MOVES_JS}
    const real = window.fetch.bind(window), asks = [], loads = [];
    window.fetch = (u, init) => {{
      const s = String(u);
      if (s.includes("/_quillo/live?")) {{ const a = {{ at: performance.now(), ended: null }}; asks.push(a); const p = real(u, init); p.then(() => {{}}, () => {{ a.ended = "aborted"; }}); return p; }}
      if (s.includes("/_quillo/rpc/load_a")) loads.push(performance.now());
      return real(u, init);
    }};
    await write("a", 1);
    const first = await until("a", 1, 8000);
    for (let i = 0; i < 300 && !window.__opened; i++) await tick(20);
    if (!window.__opened) return {{ first, opened: false, made: window.__made }};
    await tick(1000);
    await write("a", 2);
    const second = await until("a", 2, 3000);
    await tick(500);
    return {{
      byRequest: first !== null && asks.length > 0 && asks[0].at < window.__opened,
      endedAsItOpened: asks.some((a) => a.ended === "aborted"),
      requestsAfter: asks.filter((a) => a.at > window.__opened).length,
      bySocket: second !== null && second < 1000,
      loads: loads.length,
      made: window.__made,
    }};"##
    );
    let Some(rs) = in_pages("q900-late", PAGES_MOVED, &["/late/a"], &body) else { return };
    let want = serde_json::json!({ "byRequest": true, "endedAsItOpened": true, "requestsAfter": 0, "bySocket": true, "loads": 2, "made": 1, "remade": 0, "errors": [] });
    assert_eq!(rs[0], want);
}

#[test]
fn a_page_whose_server_does_not_take_topics_opens_a_socket_for_them() {
    let body = format!(
        r##"{MOVES_JS}
    $("tob").click();
    if ((await until("b", 0, 3000)) === null) return {{ lost: true }};
    await write("b", 7);
    return {{ seen: (await until("b", 7, 3000)) !== null, made: window.__made }};"##
    );
    let Some(rs) = in_pages("q900-old", PAGES_MOVED, &["/old/a"], &body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "seen": true, "made": 2, "remade": 0, "errors": [] }));
}

// What a waiting page sends, read on a socket made here (`ws::Socket::heard`): a ping answered with
// its pong and a pong dropped, between a message's parts too; a text message given as it comes, and
// one passed over when it is not masked, or has reserved bits set, or a new message begins before it
// ends; a binary message and a stray part passed over; a frame cut short for longer than it may take,
// and a page that hung up, gone. Since the review: a text message longer than asked for ends the
// socket with 1009, and a control frame longer than one may be (a pong, a close) with 1002, where
// both were passed over and a close of 126 bytes went unanswered.
#[test]
fn what_a_waiting_page_sends_is_read_as_it_comes() {
    use quillo::server::HttpRequest;
    use quillo::ws::{Heard, Socket};
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let req = HttpRequest {
        method: "GET".into(),
        url: "/_quillo/live".into(),
        headers: [("Host", "x"), ("Upgrade", "websocket"), ("Connection", "Upgrade"), ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="), ("Sec-WebSocket-Version", "13")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: Vec::new(),
        remote: "127.0.0.1".into(),
    };
    let page = |frames: &[u8]| -> (std::net::TcpStream, Socket) {
        let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let socket = Socket::accept(&req, server).expect("upgraded");
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut b).unwrap();
            head.push(b[0]);
        }
        client.write_all(frames).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        (client, socket)
    };
    let heard = |s: &mut Socket| -> (Heard, String) {
        let mut got = Vec::new();
        let h = s.heard(16, Duration::from_millis(300), |p| got.extend_from_slice(p));
        (h, String::from_utf8_lossy(&got).into_owned())
    };
    let plain = |op: u8, data: &[u8]| -> Vec<u8> {
        let mut f = vec![0x80 | op, data.len() as u8];
        f.extend_from_slice(data);
        f
    };
    // Nothing yet.
    let (_c, mut s) = page(b"");
    assert_eq!(heard(&mut s), (Heard::Nothing, String::new()));
    // A ping, a pong, then text in two parts with a ping between them.
    let mut w = ws_frame(0x9, true, b"p");
    w.extend(ws_frame(0xA, true, b""));
    w.extend(ws_frame(0x1, false, "ab".as_bytes()));
    w.extend(ws_frame(0x9, true, b"q"));
    w.extend(ws_frame(0x0, true, "é".as_bytes()));
    let (mut c, mut s) = page(&w);
    assert_eq!(heard(&mut s), (Heard::Text(true), "abé".into()));
    assert_eq!(heard(&mut s), (Heard::Nothing, String::new()));
    let mut pongs = [0u8; 6];
    c.read_exact(&mut pongs).unwrap();
    assert_eq!(pongs, [0x8a, 1, b'p', 0x8a, 1, b'q']);
    assert!(s.send("still writable"), "the socket is blocking again");
    // At the limit, in parts too, and one byte over it, in parts: the socket ends with 1009, and
    // what came after is not read.
    let mut w = ws_frame(0x1, true, &[b'x'; 16]);
    w.extend(ws_frame(0x1, false, &[b'x'; 9]));
    w.extend(ws_frame(0x0, true, &[b'x'; 7]));
    w.extend(ws_frame(0x1, false, &[b'y'; 10]));
    w.extend(ws_frame(0x0, true, &[b'y'; 7]));
    w.extend(ws_frame(0x1, true, b"next"));
    let (mut c, mut s) = page(&w);
    assert_eq!(heard(&mut s), (Heard::Text(true), "x".repeat(16)));
    assert_eq!(heard(&mut s), (Heard::Text(true), "x".repeat(16)));
    assert_eq!(heard(&mut s).0, Heard::Gone);
    let ended = |c: &mut std::net::TcpStream| -> Vec<u8> {
        c.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut got = Vec::new();
        let _ = c.read_to_end(&mut got);
        got
    };
    assert_eq!(ended(&mut c), [0x88, 2, 0x03, 0xf1]);
    // Not masked, reserved bits set, or a new message before the last ended: passed over.
    let mut odd = ws_frame(0x1, true, b"rsv");
    odd[0] |= 0x40;
    let mut w = plain(0x1, b"bare");
    w.extend(odd);
    w.extend(ws_frame(0x1, false, b"one"));
    w.extend(ws_frame(0x1, true, b"two"));
    w.extend(ws_frame(0x1, true, b"ok"));
    let (_c, mut s) = page(&w);
    assert_eq!(heard(&mut s).0, Heard::Text(false));
    assert_eq!(heard(&mut s).0, Heard::Text(false));
    assert_eq!(heard(&mut s).0, Heard::Text(false));
    assert_eq!(heard(&mut s), (Heard::Text(true), "ok".into()));
    // A binary message and a stray part: passed over, nothing told.
    let mut w = ws_frame(0x2, true, b"bin");
    w.extend(ws_frame(0x0, true, b"stray"));
    let (_c, mut s) = page(&w);
    assert_eq!(heard(&mut s), (Heard::Nothing, String::new()));
    // A pong or a close of 126 bytes, one more than a control frame may have: the socket ends with
    // 1002 (a protocol error). One of 125 is a close as any other, answered with its code.
    for (op, len, want) in [(0xA, 126, 1002u16), (0x8, 126, 1002), (0x8, 125, 1000)] {
        let mut data = 1000u16.to_be_bytes().to_vec();
        data.resize(len, b'r');
        let (mut c, mut s) = page(&ws_frame(op, true, &data));
        assert_eq!(heard(&mut s).0, Heard::Gone, "{:x} of {}", op, len);
        let mut close = vec![0x88, 2];
        close.extend(want.to_be_bytes());
        assert_eq!(ended(&mut c), close, "{:x} of {}", op, len);
    }
    // A frame cut short, for longer than it may take: gone, and the connection shut.
    let full = ws_frame(0x1, true, b"cut short");
    let (mut c, mut s) = page(&full[..8]);
    let t0 = Instant::now();
    assert_eq!(heard(&mut s).0, Heard::Gone);
    assert!(t0.elapsed() >= Duration::from_millis(250), "{:?}", t0.elapsed());
    c.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut rest = Vec::new();
    assert!(c.read_to_end(&mut rest).is_ok() && rest.is_empty(), "{:?}", rest);
    // Hung up, with no close.
    let (c, mut s) = page(b"");
    drop(c);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(heard(&mut s).0, Heard::Gone);
}

// ---- Second review of slice 2 (Q-899, Q-901) ---------------------------------------------------

// Q-901's second review: a `run_after`, an `every` or a `listen` started by a parent's handler that a
// child calls belonged to the child that was clicked, and went with it: a modal's Save that closes
// the modal and clears its notice a moment later never cleared it, nor did a row's remove, nor a
// countdown's end (called from the countdown's own `every`). Each belongs to the component whose
// code calls it now, the page here: the notice clears, and what the Save started goes on until the
// page goes. A child's own timer still goes with the child.
const PARENTS_TIMERS: &str = r###"route GET /ticks = events((send) => {
    for i in range(100000) {
        send(i)
        sleep(40)
    }
})
component Modal(on_save, note) {
    <div>
        <button #msave @click={() => { on_save() }}>save</button>
        <button #mtimer @click={() => { run_after(150, () => { note("modal") }) }}>timer</button>
    </div>
}
component Countdown(on_end) {
    state left = 2
    every(30, () => { left -= 1; if left == 0 { on_end() } })
    <p>{left}</p>
}
component Page(note) {
    state open = true
    state notice = ""
    state rows = [1, 2]
    state counting = false
    <div>
        {open ? <Modal {note} on_save={() => { open = false; notice = "saved"; run_after(150, () => { notice = "" }); every(30, () => { note("pulse") }); listen("/ticks", (n) => { note("heard") }) }}/> : null}
        <button #close @click={() => { open = false }}>close</button>
        <button #reopen @click={() => { open = true }}>reopen</button>
        <ul>{for x in rows => <li key={x}><button class="rm" data-x={x} @click={() => { rows = rows | filter((i) => i != x); notice = `removed ${x}`; run_after(150, () => { notice = "" }) }}>remove</button></li>}</ul>
        {counting ? <Countdown on_end={() => { counting = false; notice = "ended"; run_after(150, () => { notice = "" }) }}/> : null}
        <button #count @click={() => { counting = true }}>count</button>
        <p #notice>{notice}</p>
    </div>
}
component Host(now: bool) {
    state on = now
    state log = []
    let note = (s) => { log = log + [s] }
    <div>
        <button #show @click={() => { on = true }}>show</button>
        <button #off @click={() => { on = false }}>off</button>
        {on ? <Page {note}/> : null}
        <p #log>{join(log, ",")}</p>
    </div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn a_parents_timer_started_from_a_child_it_closes_is_the_parents() {
    let body = r##"const n = (s) => $("log").textContent.split(",").filter((t) => t === s).length;
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(10); return f(); };
    const notice = () => $("notice").textContent;
    const out = {};
    await click("msave");
    out.modal = [!$("msave"), notice(), await until(() => notice() === "", 1500)];
    out.started = [await until(() => n("pulse") > 2, 3000), await until(() => n("heard") > 2, 3000)];
    document.querySelector('.rm[data-x="1"]').click(); await tick(30);
    out.row = [document.querySelectorAll(".rm").length, notice(), await until(() => notice() === "", 1500)];
    await click("count"); await until(() => notice() === "ended", 3000);
    out.countdown = [notice(), await until(() => notice() === "", 1500)];
    // The modal's own timer goes with the modal.
    await click("reopen", "mtimer"); await click("close"); await tick(300); out.modalOwn = n("modal");
    // What the page started goes with the page.
    await click("off"); await tick(60); { const p = n("pulse"), h = n("heard"); await tick(300); out.pageGone = [n("pulse") === p, n("heard") === h]; }
    return out;"##;
    let Some(rs) = in_pages("q901-parent", PARENTS_TIMERS, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "modal": [true, "saved", true], "started": [true, true], "row": [1, "removed 1", true], "countdown": ["ended", true],
            "modalOwn": 0, "pageGone": [true, true], "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-901's second review: each `run_after` left its cleanup in its component's list after it had
// fired, so a poll that starts the next as it ends grew the list by one a tick, for as long as the
// component lived. Ten polls of 400 ticks each leave the heap as it was.
const POLLS: &str = r###"fn step(k: int, done) {
    if k > 0 { run_after(0, () => { step(k - 1, done) }) } else { done() }
}
component Poll() {
    state ended = 0
    <div><button #go @click={() => { for i in range(10) { step(400, () => { ended += 1 }) } }}>go</button><p #ended>{ended}</p></div>
}
route GET /h = <html><head><title>t</title></head><body><Poll/></body></html>
"###;

#[test]
fn a_poll_of_run_afters_leaves_nothing_behind() {
    let body = r##"const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(20); return f(); };
    const heap = async () => { for (let k = 0; k < 3; k++) { gc(); await tick(50); } return performance.memory.usedJSHeapSize; };
    // A first round, so that what the code makes once is made.
    await click("go"); await until(() => $("ended").textContent === "10", 20000);
    const before = await heap();
    await click("go"); await until(() => $("ended").textContent === "20", 20000);
    const after = await heap();
    return { ended: $("ended").textContent, grewKB: Math.round((after - before) / 1024) };"##;
    let Some(rs) = in_pages("q901-poll", POLLS, &["/h"], body) else { return };
    assert_eq!(rs[0]["ended"], "20", "{}", rs[0]);
    assert!(rs[0]["grewKB"].as_i64().unwrap() < 60, "4,000 ticks grew the heap by {} KB: {}", rs[0]["grewKB"], rs[0]);
    assert_eq!((&rs[0]["errors"], &rs[0]["remade"]), (&serde_json::json!([]), &serde_json::json!(0)), "{}", rs[0]);
}

// Q-899's second review: a loader whose own code failed (Quillo's 500 with its error, as for a
// computation past --max-compute) was tried again for good, behind a first load's spinner, where
// main showed the error once and nothing. It is not tried again now, as on main: the state stays as
// it was (nothing, for a first load, which then follows nothing until it is made again, as on main),
// and the console says so; a later failure of the same kind keeps the data shown, and the next
// change loads. A 503 that the program `reply`s is no such answer, and is tried again.
const LIVE_OWN_ERROR: &str = r###"let mode = 0
server fn load_e() {
    if mode == 1 {
        let z = mode - 1
        print(1 / z)
    }
    if mode == 2 { reply 503 "busy" }
    [mode]
}
component E() {
    state r = live(load_e)
    <p #e class={{ behind: waiting(r) }}>{r == null ? "none" : r[0]}</p>
}
component Host(now: bool) {
    state on = now
    state k = 0
    <div><button #show @click={() => { on = true }}>show</button><button #again @click={() => { k += 1 }}>again</button>{on ? <E key={k}/> : null}</div>
}
route POST /t/mode/:m(int) { mode = m; "ok" }
route GET /f = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn a_loader_whose_code_fails_is_shown_once_not_tried_for_good() {
    let body = r##"const real = window.fetch.bind(window);
    let calls = 0;
    window.fetch = (u, init) => { if (String(u).includes("/_quillo/rpc/load_e")) calls++; return real(u, init); };
    const set = (m) => real(`/t/mode/${m}`, { method: "POST" });
    const shown = () => ($("e") ? $("e").textContent : null);
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(20); return f(); };
    const out = {};
    // Made in the browser, its first load fails in its own code: once, and nothing is waited for.
    await set(1); await click("show"); await until(() => calls > 0, 3000); await tick(2500);
    out.first = [shown(), $("e").classList.contains("behind"), calls];
    // Made again, it loads.
    await set(0); await click("again"); out.loaded = await until(() => shown() === "0", 3000);
    // Its code fails on a change: the data stays, not waiting, once; the next change loads.
    { const c = calls; await set(1); await until(() => calls > c, 3000); await tick(2500); out.later = [shown(), $("e").classList.contains("behind"), calls - c]; }
    await set(4); out.next = await until(() => shown() === "4", 3000);
    // A 503 the program replies is tried again; then a change brings the answer.
    { const c = calls; await set(2); await until(() => calls > c + 1, 4000); out.reply503 = [shown(), calls - c >= 2]; }
    await set(3); out.back = await until(() => shown() === "3", 8000);
    return out;"##;
    let Some(rs) = in_pages("q899-own", LIVE_OWN_ERROR, &["/f"], body) else { return };
    let want = serde_json::json!({
        "first": ["none", false, 1], "loaded": true, "later": ["0", false, 1], "next": true, "reply503": ["4", true], "back": true,
        "remade": 0,
        "errors": ["server fn `load_e` failed (500): internal error in `load_e`", "server fn `load_e` failed (500): internal error in `load_e`", "server fn `load_e` failed (503)"],
    });
    assert_eq!(rs[0], want);
}

// Q-896's edge, where Quillo makes a component again: the server's HTML of a component inside
// another, of one at the top and of a table row that is its own root, was changed by a script before
// they were taken up, so each is made again when the person reaches it, by focusing a field in it.
// The field the take-up gave a handler to goes as it is made again, focused: its `blur` reaches no
// handler. The page has the focus here.
const MADE_AGAIN: &str = r###"let blurs = 0
server fn blurred() { blurs += 1 }
component Field() {
    <span class="f"><input class="in" @blur={() => { blurred() }}/><b class="tag">tag</b></span>
}
component Outer() {
    state n = 0
    <div id="outer"><Field/><button #more @click={() => { n += 1 }}>{n}</button></div>
}
component Top() {
    <div class="top"><input class="tin" @blur={() => { blurred() }}/><b class="ttag">tag</b></div>
}
component Line() {
    <tr class="line"><td><input class="lin" @blur={() => { blurred() }}/></td><td><b class="ltag">tag</b></td></tr>
}
route GET /t/blurs = str(blurs)
route GET /h = <html><head><title>t</title></head><body><main><Outer/><Top/><table><tbody><Line/></tbody></table></main></body></html>
"###;

#[test]
fn a_component_made_again_hears_nothing_from_what_it_took_up() {
    let body = r##"const swap = (sel) => { const b = document.querySelector(sel); const em = document.createElement("em"); em.textContent = b.textContent; b.replaceWith(em); };
    swap(".tag"); swap(".ttag"); swap(".ltag");
    const before = QW.remade ?? 0;
    document.querySelector(".in").focus(); await tick(200);
    document.querySelector(".tin").focus(); await tick(200);
    document.querySelector(".lin").focus(); await tick(200);
    $("more").click(); await tick(300);
    const blurs = await (await fetch("/t/blurs")).text();
    return { made: (QW.remade ?? 0) - before, blurs, inputs: document.querySelectorAll("input").length, taken: !!document.querySelector(".tag") && !!document.querySelector(".ttag") && !!document.querySelector(".ltag") };"##;
    let Some(rs) = in_pages_with("q896-remade", MADE_AGAIN, &["/h"], body, true) else { return };
    let r = &rs[0];
    assert_eq!((&r["made"], &r["blurs"], &r["inputs"], &r["taken"]), (&serde_json::json!(3), &serde_json::json!("0"), &serde_json::json!(3), &serde_json::json!(true)), "{}", r);
}

// Q-889's edge: a row of two branches, whose record keeps the two markers, through flips and moves.
const BRANCH_THEN_ELEMENT: &str = r###"view Pair(x) = [x.on ? <b>{`b${x.id}`}</b> : <i>{`i${x.id}`}</i>, x.id > 1 ? <em>{`e${x.id}`}</em> : <u>{`u${x.id}`}</u>]
component Rows() {
    state xs = [{ id: 1, on: true }, { id: 2, on: false }, { id: 3, on: true }]
    <div>
        <div #l>{for x in xs => <Pair key={x.id} {x}/>}</div>
        <button #flip @click={() => { xs = xs | map((x) => x.id == 2 ? merge(x, { on: !x.on }) : x) }}>flip</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #drop @click={() => { xs = xs | filter((x) => x.id != 1) }}>drop</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Rows/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn rows_of_two_branches_move_as_they_are() {
    let body = r##"const seen = () => [...$("l").children].map((e) => e.textContent).join(" ");
    const out = { start: seen() };
    await click("flip", "rev"); out.flipRev = seen();
    await click("flip", "rev"); out.back = seen();
    await click("drop", "rev"); out.dropRev = seen();
    return out;"##;
    let Some(rs) = in_pages("q889-branch-element", BRANCH_THEN_ELEMENT, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "start": "b1 u1 i2 e2 b3 e3", "flipRev": "b3 e3 b2 e2 b1 u1", "back": "b1 u1 i2 e2 b3 e3", "dropRev": "b3 e3 i2 e2",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-901's edge: an error in a `run_after`'s callback, or in one tick of an `every`, is said, and
// what comes after runs as it did: the timer's next tick, and a handler's next timer.
const FAILING_TIMERS: &str = r###"component T() {
    state n = 0
    state m = 0
    <div>
        <button #ra @click={() => { run_after(10, () => { n += 1; let z = n - n; print(1 / z) }) }}>ra</button>
        <button #ev @click={() => { every(20, () => { m += 1; if m == 2 { let z = m - m; print(1 / z) } }) }}>ev</button>
        <p #n>{n}</p><p #m>{m}</p>
    </div>
}
route GET /h = <html><head><title>t</title></head><body><T/></body></html>
"###;

#[test]
fn an_error_in_a_timers_callback_is_said_and_the_rest_goes_on() {
    let body = r##"const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(10); return f(); };
    await click("ra"); await until(() => $("n").textContent === "1", 2000);
    await click("ra"); const again = await until(() => $("n").textContent === "2", 2000);
    await click("ev"); const ticks = await until(() => Number($("m").textContent) > 4, 3000);
    return { again, ticks, errors2: window.__quilloErrors.length };"##;
    let Some(rs) = in_pages("q901-throws", FAILING_TIMERS, &["/h"], body) else { return };
    let r = &rs[0];
    assert_eq!((&r["again"], &r["ticks"], &r["errors2"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(3)), "{}", r);
    assert!(r["errors"].as_array().unwrap().iter().all(|e| e.as_str().unwrap().contains("division by zero")), "{}", r);
}

// A page's thread waits on its own bell (src/shared.rs): a ring ends its wait at once, a change wakes
// the pages parked on it, one page or several (handed to the thread that rings them), and a page busy
// when the change came, not parked, is not rung but finds the change at its next wait. Found in review
// of Q-900: each change handed its pages to that thread, though none was parked during sustained
// writes, and the server spent 40% more; only parked pages are rung now.
#[test]
fn a_change_wakes_the_pages_parked_on_it_and_a_busy_one_finds_it_next() {
    use quillo::shared::{Bell, Shared};
    use std::sync::Arc;
    let s = Arc::new(Shared::new());
    let t = |n: &str| vec![n.to_string()];
    s.publish(&t("x"));
    let since = s.live_seq();
    // Rung: the wait ends at once, with nothing changed, and the bell says so once.
    let bell = Bell::new();
    bell.bind();
    let me = s.follow(t("x"), &bell);
    let ringer = {
        let b = bell.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            b.ring();
        })
    };
    let began = Instant::now();
    assert_eq!(me.wait(since, None, Duration::from_secs(10)), (since, Vec::new()));
    assert!(began.elapsed() < Duration::from_secs(5), "{:?}", began.elapsed());
    ringer.join().unwrap();
    assert!(bell.take() && !bell.take());
    // Parked pages, one and then three: each woken by the change.
    for n in [1usize, 3] {
        let since = s.live_seq();
        let pages: Vec<_> = (0..n)
            .map(|_| {
                let s = s.clone();
                std::thread::spawn(move || {
                    let bell = Bell::new();
                    bell.bind();
                    let f = s.follow(vec!["y".to_string()], &bell);
                    let began = Instant::now();
                    let got = f.wait(since, None, Duration::from_secs(10));
                    (got, began.elapsed())
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        let made = s.publish(&t("y"));
        for p in pages {
            let ((seq, changed), took) = p.join().unwrap();
            assert_eq!((seq, changed), (made, t("y")), "{} pages", n);
            assert!(took < Duration::from_secs(5), "{} pages: {:?}", n, took);
        }
    }
    // A page not parked when the change comes is not rung, and its next wait returns at once.
    let since = s.live_seq();
    let busy = s.follow(t("z"), &bell);
    let other = Bell::new();
    let parked = std::thread::spawn({
        let s = s.clone();
        let other = other.clone();
        move || {
            other.bind();
            let f = s.follow(vec!["z".to_string()], &other);
            f.wait(since, None, Duration::from_secs(10))
        }
    });
    std::thread::sleep(Duration::from_millis(200));
    let made = s.publish(&t("z"));
    assert_eq!(parked.join().unwrap(), (made, t("z")));
    let began = Instant::now();
    assert_eq!(busy.wait(since, None, Duration::from_secs(10)), (made, t("z")));
    assert!(began.elapsed() < Duration::from_millis(500), "{:?}", began.elapsed());
    drop(me);
}

// Found while fixing the review's F2: a socket that failed without opening, after plain requests had
// begun to carry the changes, was followed by one more plain request before the next socket, which
// waited up to 25 s with no change. The second review: made again at once, it ended the request that
// carried the changes, and the page heard nothing for the next socket's first 3 s. The plain requests
// go on now across the failed socket, and the next socket is made after a while, longer each time
// (decided by the coordinator after that review).
#[test]
fn a_socket_that_fails_while_plain_requests_carry_the_changes_leaves_them_carrying() {
    let body = format!(
        r##"{MOVES_JS}
    for (let i = 0; i < 400 && window.__failed.length < 1; i++) await tick(10);
    if (!window.__failed.length) return {{ failed: false }};
    // Right after the socket failed: told by the request that goes on.
    await write("a", 3);
    const first = await until("a", 3, 6000);
    for (let i = 0; i < 1000 && window.__made < 3; i++) await tick(20);
    // Each next socket after a while, longer each time; the third opens and takes over.
    const gaps = [window.__madeAt[1] - window.__failed[0], window.__madeAt[2] - window.__failed[1]].map(Math.round);
    for (let i = 0; i < 100 && !(window.__made >= 3); i++) await tick(20);
    await tick(1000);
    await write("a", 4);
    const second = await until("a", 4, 3000);
    return {{ toldAtOnce: first !== null && first < 1500, firstWait: gaps[0] >= 400 && gaps[0] <= 3500, secondWait: gaps[1] >= 900 && gaps[1] <= 6500, bySocket: second !== null && second < 1000, made: window.__made }};"##
    );
    let Some(rs) = in_pages("q900-fails", PAGES_MOVED, &["/fails/a"], &body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "toldAtOnce": true, "firstWait": true, "secondWait": true, "bySocket": true, "made": 3, "remade": 0, "errors": [] }));
}

// Found in the second review of Q-900: when sockets and plain requests both failed, slowly (a proxy
// whose upstream is down answers 504 after its connect timeout, 5 s here), each socket was made again
// at once, every 5 s for good, and the page never said it was behind. Now the sockets come later each
// time, the requests too, and the page says so once; as it did when they failed at once (`d=0`, the
// control). Afterwards the page catches up, and the first plain request answered ends the wait for the
// next socket, which opens.
#[test]
fn an_outage_whose_requests_fail_slowly_is_waited_out_and_said() {
    let body = format!(
        r##"{MOVES_JS}
    const span = location.search.includes("d=0") ? 15000 : 32000;
    window.__down = true;
    const m0 = window.__made;
    for (const s of window.__socks) s.close();
    await tick(span);
    const sockets = window.__made - m0, asks = window.__asks.length;
    const said = (window.__quilloErrors || []).filter((e) => /not reaching this page/.test(e)).length;
    window.__down = false;
    await write("a", 9);
    const back = await until("a", 9, 50000);
    // The plain request answered again ends the wait for the next socket.
    const open = () => window.__socks.some((s) => s.readyState === 1);
    for (let i = 0; i < 500 && !open(); i++) await tick(20);
    return {{ sockets, asks, said, back: back !== null, socketBack: open() }};"##
    );
    let Some(rs) = in_pages_for("q900-outage", PAGES_MOVED, &["/outage/a?d=5000", "/outage/a?d=0"], &body, false, 120_000) else { return };
    for (r, (most, path)) in rs.iter().zip([(4, "slow"), (6, "at once")]) {
        let (sockets, asks) = (r["sockets"].as_u64().unwrap(), r["asks"].as_u64().unwrap());
        assert!((2..=most).contains(&sockets) && (2..=most).contains(&asks), "{}: {}", path, r);
        assert_eq!((&r["said"], &r["back"], &r["socketBack"], &r["remade"]), (&serde_json::json!(1), &serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(0)), "{}: {}", path, r);
    }
}

// The plain requests made while there is no socket follow the live loop's rules: a refusal waits for
// its Retry-After and is said at once, and a 4xx, or an answer that is no answer (a sign-in page), is
// said at once and not asked again (found in the second review of Q-900).
#[test]
fn plain_requests_while_a_socket_connects_take_retry_after_and_drop_a_4xx() {
    let body = format!(
        r##"{MOVES_JS}
    const real = window.fetch.bind(window), asks = [];
    const mode = location.search.slice(3);
    window.fetch = (u, init) => {{
      if (String(u).includes("/_quillo/live?")) {{
        asks.push(performance.now());
        if (asks.length === 1 && mode === "busy") return Promise.resolve(new Response("busy", {{ status: 503, headers: {{ "retry-after": "4" }} }}));
        if (mode === "403") return Promise.resolve(new Response("no", {{ status: 403 }}));
        if (mode === "page") return Promise.resolve(new Response("<html>sign in</html>", {{ status: 200, headers: {{ "content-type": "text/html" }} }}));
      }}
      return real(u, init);
    }};
    // The pages share the server's table: each writes one more than it shows.
    const before = $("va").textContent, next = +before + 1;
    await write("a", next);
    if (mode === "busy") {{
      const seen = await until("a", next, 12000);
      return {{ seen: seen !== null, waited: asks.length >= 2 && asks[1] - asks[0] >= 3900 }};
    }}
    await tick(9000);
    return {{ asks: asks.length, told: $("va").textContent !== before }};"##
    );
    let paths = ["/never/a?m=busy", "/never/a?m=403", "/never/a?m=page"];
    let Some(rs) = in_pages("q900-refused", PAGES_MOVED, &paths, &body) else { return };
    let said = "live updates are not reaching this page: the server is not taking them";
    assert_eq!(rs[0], serde_json::json!({ "seen": true, "waited": true, "remade": 0, "errors": [said] }));
    assert_eq!(rs[1], serde_json::json!({ "asks": 1, "told": false, "remade": 0, "errors": [said] }));
    assert_eq!(rs[2], serde_json::json!({ "asks": 1, "told": false, "remade": 0, "errors": [said] }));
}

// While a socket connects, a plain request that fails (a 503, or no connection) is tried again after a
// while, longer each time, as the live loop waits, and the change comes with the next.
#[test]
fn a_plain_request_that_fails_while_a_socket_connects_is_tried_again() {
    let body = format!(
        r##"{MOVES_JS}
    const real = window.fetch.bind(window), asks = [];
    let refuse = 2;
    window.fetch = (u, init) => {{
      if (String(u).includes("/_quillo/live?")) {{
        asks.push(performance.now());
        if (refuse === 2) {{ refuse--; return Promise.resolve(new Response("busy", {{ status: 503 }})); }}
        if (refuse === 1) {{ refuse--; return Promise.reject(new TypeError("Failed to fetch")); }}
      }}
      return real(u, init);
    }};
    await write("a", 4);
    const seen = await until("a", 4, 15000);
    return {{ seen: seen !== null, asks: asks.length >= 3, apart: asks.length >= 3 && asks[1] - asks[0] >= 400 && asks[2] - asks[1] >= 900, made: window.__made }};"##
    );
    let Some(rs) = in_pages("q900-retry", PAGES_MOVED, &["/never/a"], &body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "seen": true, "asks": true, "apart": true, "made": 1, "remade": 0, "errors": [] }));
}

// ---- Who a timer belongs to (Q-901, the review of e2eb410; the user's decision of 2026-11-12) ----

// The user's decision ("Component keeps it"): a timer, an `every` or a `listen` that an event
// handler starts belongs to the component whose code the handler is, so it outlives the row or the
// branch holding the button: a stopwatch whose Start is in a branch ticks on once the branch hides.
// One started by an `@mount`, by a timer's own callback, or by a branch's own code ends with its
// element, as before e2eb410, where they had gone to the component: an `every` in each row's `@mount`
// piled up as rows came and went (an interval for every row ever shown), a `listen` in each filled
// the browser's six connections to the site so a `server fn` went unanswered, a toast's `@mount`
// timer hid the next toast of the same name early, and what a branch's timer started outlived the
// branch. A parent's callback that a child's own `if` calls keeps its timer, as one a child's
// handler calls does.
const OWNERS: &str = r###"route GET /ticks/:n = events((send) => {
    for i in range(100000) {
        send(i)
        sleep(100)
    }
})
server fn ping() -> str { "pong" }
component Counting(on_done) {
    state left = 2
    every(30, () => { left -= 1 })
    if left == 0 { on_done() }
    <p>{left}</p>
}
component Board(note) {
    state gen = 0
    state rows = [1, 2, 3, 4, 5]
    state toasts = []
    state polling = false
    state panel = true
    state counting = false
    state notice = ""
    state answer = ""
    if polling { run_after(60, () => { note("c1"); run_after(150, () => { note("c2") }) }) }
    <div>
        <ul #rows>{for x in rows => <li key={x} @mount={(el) => { every(200, () => { note("tick") }) }}>{x}</li>}</ul>
        <ul #streams>{for x in [gen] => <li key={x} @mount={(el) => { listen(`/ticks/${x}`, (n) => { note("msg") }) }}>s{x}</li>}</ul>
        <button #regen @click={() => { gen += 1; rows = rows | map((x) => x + 10) }}>regen</button>
        <button #ping @click={() => { answer = ping() }}>ping</button><p #answer>{answer}</p>
        <ul #toasts>{for t in toasts => <li key={t} class="toast" @mount={(el) => { run_after(600, () => { toasts = toasts | filter((x) => x != t) }) }}>{t}<button class="dismiss" @click={() => { toasts = toasts | filter((x) => x != t) }}>x</button></li>}</ul>
        <button #save @click={() => { toasts = toasts + ["Saved"] }}>save</button>
        <button #poll @click={() => { polling = true }}>poll</button><button #stop @click={() => { polling = false }}>stop</button>
        {panel ? <div><button #start @click={() => { every(40, () => { note("sw") }) }}>start</button></div> : null}
        <button #hide @click={() => { panel = false }}>hide</button>
        {counting ? <Counting on_done={() => { counting = false; notice = "done"; run_after(150, () => { notice = "" }) }}/> : null}
        <button #count @click={() => { counting = true }}>count</button>
        <p #notice>{notice}</p>
    </div>
}
component Host(now: bool) {
    state on = now
    state log = []
    let note = (s) => { log = log + [s] }
    <div><button #show @click={() => { on = true }}>show</button><button #off @click={() => { on = false }}>off</button>{on ? <Board {note}/> : null}<p #log>{join(log, ",")}</p></div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;

#[test]
fn a_timer_is_its_handlers_component_and_otherwise_its_parts() {
    let body = r##"const n = (s) => $("log").textContent.split(",").filter((t) => t === s).length;
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(10); return f(); };
    // Intervals and streams alive, from here on.
    const si = window.setInterval, ci = window.clearInterval, mine = new Set();
    window.setInterval = (f, ms, ...a) => { const id = si(f, ms, ...a); mine.add(id); return id; };
    window.clearInterval = (id) => { mine.delete(id); return ci(id); };
    const real = window.fetch.bind(window); let open = 0;
    window.fetch = (u, init) => { if (String(u).includes("/ticks/")) { open++; if (init && init.signal) init.signal.addEventListener("abort", () => open--); } return real(u, init); };
    const out = {};
    // Rows whose `@mount` starts an `every`, and a `listen`, made again eight times.
    for (let k = 0; k < 8; k++) { await click("regen"); await tick(40); }
    await tick(300);
    out.intervals = mine.size; out.streams = open;
    { const t0 = performance.now(); await click("ping"); out.ping = (await until(() => $("answer").textContent === "pong", 4000)) && performance.now() - t0 < 4000; }
    // A toast hides itself 600 ms after it shows; dismissed and shown again, the new one stays 600 ms.
    await click("save"); await tick(200); document.querySelector(".dismiss").click(); await tick(30); await click("save");
    { const t0 = performance.now(); await until(() => !document.querySelector(".toast"), 3000); out.toastMs = Math.round(performance.now() - t0) >= 500; }
    // What a branch's timer starts goes with the branch.
    await click("poll"); await until(() => n("c1") > 0, 2000); await click("stop"); await tick(400); out.branchTimer = [n("c1"), n("c2")];
    // The decision: a stopwatch whose Start is in a branch ticks on once the branch hides.
    await click("start"); await until(() => n("sw") > 2, 2000); await click("hide"); await tick(40); { const a = n("sw"); await tick(300); out.stopwatch = n("sw") > a + 2; }
    // A parent's callback that a child's own `if` calls, and closes the child with: its notice clears.
    await click("count"); await until(() => $("notice").textContent === "done", 3000); out.childIf = [$("notice").textContent, await until(() => $("notice").textContent === "", 1500)];
    return out;"##;
    let Some(rs) = in_pages("q901-owners", OWNERS, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            // Five rows' `every`, and the stopwatch's is not started yet.
            "intervals": 5, "streams": 1, "ping": true, "toastMs": true, "branchTimer": [1, 0], "stopwatch": true,
            "childIf": ["done", true], "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-899, the review of e2eb410: a program's own `reply 500 { error }`, or `respond(500, ...)`, was the
// server's failure answer byte for byte, so a `live` load that got one was not tried again, where
// q900 had tried it. The server marks its own now (`x-quillo-answer: failed`): the loader's own
// error is shown once, and a 500 the program answers is tried again, as one whose mark a proxy
// dropped is.
const LIVE_500: &str = r###"let mode = 0
server fn load_p() {
    if mode == 1 {
        let z = mode - 1
        print(1 / z)
    }
    if mode == 5 { reply 500 { error: "upstream busy" } }
    mode == 6 ? respond(500, { error: "upstream busy" }) : [mode]
}
component P() {
    state r = live(load_p)
    <p #p class={{ behind: waiting(r) }}>{r == null ? "none" : r[0]}</p>
}
route POST /t/mode/:m(int) { mode = m; "ok" }
route GET /h = <html><head><title>t</title></head><body><P/></body></html>
"###;

#[test]
fn a_programs_own_500_is_tried_again_and_the_servers_is_not() {
    let body = r##"const real = window.fetch.bind(window);
    let calls = 0, strip = false;
    window.fetch = async (u, init) => {
      if (!String(u).includes("/_quillo/rpc/load_p")) return real(u, init);
      calls++;
      const r = await real(u, init);
      if (!strip) return r;
      const h = new Headers(r.headers); h.delete("x-quillo-answer");
      return new Response(await r.text(), { status: r.status, headers: h });
    };
    const set = (m) => real(`/t/mode/${m}`, { method: "POST" });
    const shown = () => $("p").textContent;
    const until = async (f, ms) => { const t0 = performance.now(); while (!f() && performance.now() - t0 < ms) await tick(20); return f(); };
    const failing = async (m) => { const c = calls; await set(m); await until(() => calls > c, 3000); await tick(3000); const got = [shown(), $("p").classList.contains("behind"), calls - c]; await set(0); await until(() => shown() === "0", 8000); return got; };
    const out = {};
    await set(0); out.up = await until(() => shown() === "0", 3000);
    out.reply500 = await failing(5);
    out.respond500 = await failing(6);
    out.own = await failing(1);
    strip = true; out.stripped = await failing(1); strip = false;
    out.reply500[2] = out.reply500[2] >= 2; out.respond500[2] = out.respond500[2] >= 2; out.stripped[2] = out.stripped[2] >= 2;
    return out;"##;
    let Some(rs) = in_pages("q899-500", LIVE_500, &["/h"], body) else { return };
    let busy = "server fn `load_p` failed (500): upstream busy";
    let own = "server fn `load_p` failed (500): internal error in `load_p`";
    let want = serde_json::json!({
        "up": true, "reply500": ["0", false, true], "respond500": ["0", false, true], "own": ["0", false, 1], "stripped": ["0", false, true],
        "remade": 0, "errors": [busy, busy, own, own],
    });
    assert_eq!(rs[0], want);
}

// ---- Slice 3: fields and roots (Q-890, Q-891, Q-892, Q-936, Q-937, Q-938) ------------------------

// Q-890: a function's markup made again (an unrelated state changed) copied the new elements'
// values over the page's: the defaults, where the markup said nothing. What was typed, ticked,
// picked or opened went, the focus staying. Only what the markup itself says, and says otherwise
// than before, is written now. The edges: a field the markup does say follows it when that
// changes, and keeps what the person did while it does not; unicode text; twice in a row.
#[test]
fn a_functions_markup_keeps_what_a_person_typed_ticked_picked_or_opened() {
    let src = r###"fn form(n) = <div><input #f/><input #ck type="checkbox"/><details #d><summary>s</summary>b</details><select #s><option>a</option><option>b</option></select><textarea #ta></textarea><span #n>{n}</span></div>
fn ctl(n, on, txt, v) = <div><input #c1 value={txt}/><input #c2 type="checkbox" checked={on}/><details #c3 open={on}><summary>s</summary>b</details><select #c4 value={v}><option>a</option><option>b</option><option>c</option></select><span>{n}</span></div>
component Fields() {
    state n = 0
    state on = false
    state txt = "one"
    state v = "a"
    <div>
        {form(n)}
        {ctl(n, on, txt, v)}
        <button #inc @click={() => { n += 1 }}>inc</button>
        <button #flip @click={() => { on = !on; txt = txt == "one" ? "two" : "one"; v = v == "a" ? "c" : "a" }}>flip</button>
    </div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Fields/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const typeIn = (id, text) => { const el = $(id); el.focus(); el.select(); document.execCommand("insertText", false, text); };
    const pick = (id, v) => { $(id).value = v; $(id).dispatchEvent(new Event("change", { bubbles: true })); };
    typeIn("f", "héllo 日本");
    $("ck").click();
    $("d").open = true;
    pick("s", "b");
    typeIn("ta", "note");
    const f0 = $("f");
    const fields = () => [$("f").value, $("ck").checked, $("d").open, $("s").value, $("ta").value, document.activeElement.id].join("|");
    const ctl = () => [$("c1").value, $("c2").checked, $("c3").open, $("c4").value].join("|");
    const out = { before: fields() };
    await click("inc");
    out.after = fields();
    out.same = $("f") === f0;
    await click("inc");
    out.twice = fields() + "|" + $("n").textContent;
    out.ctl0 = ctl();
    await click("flip");
    out.ctl1 = ctl();
    typeIn("c1", "mine");
    $("c2").click();
    $("c3").open = false;
    pick("c4", "b");
    await click("inc");
    out.ctlKept = ctl();
    await click("flip");
    out.ctl2 = ctl();
    return out;"##;
    let Some(rs) = in_pages("q890", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "before": "héllo 日本|true|true|b|note|ta", "after": "héllo 日本|true|true|b|note|ta", "same": true, "twice": "héllo 日本|true|true|b|note|ta|2",
            "ctl0": "one|false|false|a", "ctl1": "two|true|true|c", "ctlKept": "mine|false|false|b", "ctl2": "one|false|false|a",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-891: a field its handler rewrites (in capitals, digits only) took its new text with the
// caret at the end, so a correction in the middle came out scrambled (AXBYZ for AXYZB, 1239 for
// 1923). The caret keeps its distance from the end of the text now. The edges: at the end and at
// the start, a selection typed over, a capital that is two letters (ß, SS), a textarea, a field
// that is not focused (written, nothing else), a plain field (as before).
#[test]
fn rewriting_a_field_as_it_is_typed_keeps_the_caret() {
    let src = r###"component Caret() {
    state code = "AB"
    state digits = "123"
    state plain = "ab"
    state big = "aßb"
    state other = "xy"
    <div>
        <input #code value={code} @input={(e) => { code = upper(e.value) }}/>
        <input #digits value={digits} @input={(e) => { digits = re.replace(e.value, "[^0-9]", "") }}/>
        <input #plain value={plain}/>
        <input #big value={big} @input={(e) => { big = upper(e.value) }}/>
        <textarea #area value={code} @input={(e) => { code = upper(e.value) }}></textarea>
        <input #other value={other}/>
        <button #set @click={() => { other = "set from a click" }}>set</button>
    </div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Caret/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const typeAt = async (id, at, keys) => { const el = $(id); el.focus(); el.setSelectionRange(at, at); for (const k of keys) { document.execCommand("insertText", false, k); await tick(20); } return `${el.value}@${el.selectionStart}`; };
    const out = {};
    out.code = await typeAt("code", 1, ["x", "y", "z"]);
    out.digits = await typeAt("digits", 1, ["z", "9"]);
    out.plain = await typeAt("plain", 1, ["x", "y", "z"]);
    out.end = await typeAt("code", $("code").value.length, ["q"]);
    out.start = await typeAt("code", 0, ["w"]);
    const c = $("code");
    c.focus(); c.setSelectionRange(1, 3); document.execCommand("insertText", false, "k"); await tick(20);
    out.over = `${c.value}@${c.selectionStart}-${c.selectionEnd}`;
    out.twoLetters = await typeAt("big", 2, ["ß"]);
    out.area = await typeAt("area", 2, ["e"]);
    out.codeFollows = $("code").value;
    await click("set");
    out.other = $("other").value;
    return out;"##;
    let Some(rs) = in_pages("q891", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "code": "AXYZB@4", "digits": "1923@2", "plain": "axyzb@4", "end": "AXYZBQ@6", "start": "WAXYZBQ@1", "over": "WKYZBQ@2-2",
            "twoLetters": "ASSSSB@5", "area": "WKEYZBQ@3", "codeFollows": "WKEYZBQ", "other": "set from a click",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-892 and Q-936: a component of a table's or a select's parts that is not one element (no
// row, two rows, a list of rows, a hidden branch, a cell in a row) was written in a
// <quillo-island>, which the HTML parser moves out of the table, so the browser made the whole
// page again on load (what was typed before went). Inside another component it is written
// between comments now, which the parser leaves, and taken up. The edges: one row (its own root,
// as before), a component in such a component, one that runs by itself (its timer runs with no
// one touching the page), options in a select (none and two), an empty one outside a table (it
// gets a wrapper, as one made in the browser does), unicode text; rows added, reversed, shown.
#[test]
fn table_parts_that_are_not_one_element_are_taken_up_not_made_again() {
    let src = r###"component Rows(xs: list) { {for x in xs => <tr key={x} class="r"><td>{x}</td></tr>} }
component Two(a: str) { [<tr class="t"><td>{a}1</td></tr>, <tr class="t"><td>{a}2</td></tr>] }
component Maybe(on: bool) { on ? <tr class="m"><td>shown</td></tr> : null }
component Cell(on: bool) { on ? <td class="c">cell</td> : null }
component Opts(xs: list) { {for x in xs => <option key={x} value={x}>{x}</option>} }
component Inner(xs: list) { <Rows xs={xs}/> }
component Ticker() {
    state n = 0
    every(30, () => { n += 1 })
    [<tr class="k"><td>{n}</td></tr>, <tr class="k"><td>{n}</td></tr>]
}
component Page() {
    state xs = ["a", "b"]
    state on = false
    state v = "q"
    <div>
        <input #early/>
        <table><tbody #tb><Rows xs={xs}/><Rows xs={[]}/><Rows xs={["one"]}/><Two a="t"/><Maybe on={on}/><Inner xs={["n1", "ünï"]}/><tr #row><td>x</td><Cell on={on}/></tr></tbody></table>
        <table><tbody #tk><Ticker/></tbody></table>
        <select #sel value={v}><option value="p">p</option><Opts xs={["q", "r"]}/><Opts xs={[]}/></select>
        <div #empty><Maybe on={on}/></div>
        <button #add @click={() => { xs = xs + ["c"] }}>add</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #on @click={() => { on = !on }}>on</button>
    </div>
}
route GET / = <html><head><title>t</title></head><body><Page/></body></html>
"###;
    let body = r##"const rows = (id) => [...$(id).rows].map((r) => r.textContent).join(",");
    const bare = (id) => [...$(id).childNodes].filter((n) => n.nodeType === 8 || n.localName === "quillo-island").length;
    const out = { remadeAtLoad: QW.remade ?? 0 };
    out.load = [rows("tb"), bare("tb"), $("row").cells.length, bare("row")];
    out.select = [[...$("sel").options].map((o) => o.value).join(","), $("sel").value, bare("sel")];
    out.empty = $("empty").querySelectorAll(":scope > quillo-island").length;
    const k0 = $("tk").rows[0].textContent;
    await tick(200);
    out.ticks = Number($("tk").rows[0].textContent) > Number(k0) && $("tk").rows.length === 2;
    await click("add", "rev", "on");
    out.after = [rows("tb"), bare("tb"), $("row").cells.length, $("empty").textContent];
    return out;"##;
    let Some(rs) = in_pages("q892", src, &["/"], body) else { return };
    let want = serde_json::json!({
        "remadeAtLoad": 0,
        "load": ["a,b,one,t1,t2,n1,ünï,x", 0, 1, 0],
        "select": ["p,q,r", "q", 0],
        "empty": 1,
        "ticks": true,
        "after": ["c,b,a,one,t1,t2,shown,n1,ünï,xcell", 0, 2, "shown"],
        "remade": 0, "errors": [],
    });
    assert_eq!(rs[0], want);
}

// Q-892 under `--watch`: development bundles take up what the server marked by comments the same
// way (and say nothing of a component made again, as none is).
#[test]
fn table_parts_marked_by_comments_are_taken_up_under_watch() {
    let src = r###"component Rows(xs: list) { {for x in xs => <tr key={x} class="r"><td>{x}</td></tr>} }
component Two(a: str) { [<tr class="t"><td>{a}1</td></tr>, <tr class="t"><td>{a}2</td></tr>] }
component Maybe(on: bool) { on ? <tr class="m"><td>shown</td></tr> : null }
component Cell(on: bool) { on ? <td class="c">cell</td> : null }
component Opts(xs: list) { {for x in xs => <option key={x} value={x}>{x}</option>} }
component Inner(xs: list) { <Rows xs={xs}/> }
component Ticker() {
    state n = 0
    every(30, () => { n += 1 })
    [<tr class="k"><td>{n}</td></tr>, <tr class="k"><td>{n}</td></tr>]
}
component Page() {
    state xs = ["a", "b"]
    state on = false
    state v = "q"
    <div>
        <input #early/>
        <table><tbody #tb><Rows xs={xs}/><Rows xs={[]}/><Rows xs={["one"]}/><Two a="t"/><Maybe on={on}/><Inner xs={["n1", "ünï"]}/><tr #row><td>x</td><Cell on={on}/></tr></tbody></table>
        <table><tbody #tk><Ticker/></tbody></table>
        <select #sel value={v}><option value="p">p</option><Opts xs={["q", "r"]}/><Opts xs={[]}/></select>
        <div #empty><Maybe on={on}/></div>
        <button #add @click={() => { xs = xs + ["c"] }}>add</button>
        <button #rev @click={() => { xs = reverse(xs) }}>rev</button>
        <button #on @click={() => { on = !on }}>on</button>
    </div>
}
route GET / = <html><head><title>t</title></head><body><Page/></body></html>
"###;
    let body = r##"const rows = (id) => [...$(id).rows].map((r) => r.textContent).join(",");
    const bare = (id) => [...$(id).childNodes].filter((n) => n.nodeType === 8 || n.localName === "quillo-island").length;
    const out = { remadeAtLoad: QW.remade ?? 0 };
    out.load = [rows("tb"), bare("tb"), $("row").cells.length, bare("row")];
    out.select = [[...$("sel").options].map((o) => o.value).join(","), $("sel").value, bare("sel")];
    out.empty = $("empty").querySelectorAll(":scope > quillo-island").length;
    const k0 = $("tk").rows[0].textContent;
    await tick(200);
    out.ticks = Number($("tk").rows[0].textContent) > Number(k0) && $("tk").rows.length === 2;
    await click("add", "rev", "on");
    out.after = [rows("tb"), bare("tb"), $("row").cells.length, $("empty").textContent];
    return out;"##;
    let Some(rs) = in_pages_args("q892-watch", src, &["/"], body, false, &["--watch"]) else { return };
    let want = serde_json::json!({
        "remadeAtLoad": 0,
        "load": ["a,b,one,t1,t2,n1,ünï,x", 0, 1, 0],
        "select": ["p,q,r", "q", 0],
        "empty": 1,
        "ticks": true,
        "after": ["c,b,a,one,t1,t2,shown,n1,ünï,xcell", 0, 2, "shown"],
        "remade": 0, "errors": [],
    });
    assert_eq!(rs[0], want);
}

// Q-937: a component in <svg> whose root is a branch, made in the browser, stood in a
// <quillo-island>, which SVG does not draw: nothing showed. It stands by itself there now, as
// one taken up from the server's page does. The edges: two elements, one that shows nothing at
// first and later something, the branch flipped and back.
#[test]
fn a_branched_svg_component_made_in_the_browser_is_drawn() {
    let src = r###"component Dot(on: bool) { on ? <circle class="dot" cx="5" cy="5" r="4"/> : <rect class="dot" width="8" height="8"/> }
component Pair() { [<rect class="p1" width="3" height="3"/>, <rect class="p2" x="4" width="3" height="3"/>] }
component Later(on: bool) { on ? <rect class="later" width="6" height="6"/> : null }
component Pic() {
    state on = false
    <div><svg #sv width="20" height="20"><Dot on={on}/><Pair/><Later on={on}/></svg><button #on @click={() => { on = !on }}>on</button></div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Pic/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const drawn = (sel) => { const e = document.querySelector("#sv " + sel); return !e ? "none" : e.getBBox ? `${e.localName}:${e.getBBox().width}` : `${e.localName}:no SVG element`; };
    const look = () => [drawn(".dot"), drawn(".p1"), drawn(".p2"), drawn(".later"), $("sv").querySelectorAll("quillo-island").length].join("|");
    const out = { first: look() };
    await click("on"); out.on = look();
    await click("on"); out.back = look();
    return out;"##;
    let Some(rs) = in_pages("q937", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "first": "rect:8|rect:3|rect:3|none|0", "on": "circle:8|rect:3|rect:3|rect:6|0", "back": "rect:8|rect:3|rect:3|none|0",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-938: an option made anew by its component's branch, made in the browser, was in a
// <quillo-island>, whose select did not hear of it: it went on showing another option. The
// component's option stands in the select now. The edges: in an optgroup, a select of that one
// option, a value set after.
#[test]
fn a_select_keeps_its_value_when_an_option_component_is_made_anew() {
    let src = r###"component Opt(o: str, big: bool) { big ? <option value={o}>{o}!</option> : <option value={o}>{o}</option> }
component Pick() {
    state v = "b"
    state big = false
    <div>
        <select #s value={v}>{for o in ["a", "b", "c"] => <Opt key={o} o={o} big={big && o == "b"}/>}</select>
        <select #g value={v}><optgroup label="g">{for o in ["a", "b", "c"] => <Opt key={o} o={o} big={big && o == "b"}/>}</optgroup></select>
        <select #one value={v}><Opt o="b" big={big}/></select>
        <button #big @click={() => { big = !big }}>big</button>
        <button #pick @click={() => { v = "c" }}>pick</button>
    </div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Pick/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const sel = (id) => `${$(id).value}:${$(id).selectedIndex}:${$(id).options.length}:${$(id).querySelectorAll("quillo-island").length}`;
    const look = () => [sel("s"), sel("g"), sel("one"), $("s").options[$("s").selectedIndex].text].join("|");
    const out = { first: look() };
    await click("big"); out.big = look();
    await click("big"); out.back = look();
    await click("pick", "big"); out.picked = look();
    return out;"##;
    let Some(rs) = in_pages("q938", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "first": "b:1:3:0|b:1:3:0|b:0:1:0|b", "big": "b:1:3:0|b:1:3:0|b:0:1:0|b!", "back": "b:1:3:0|b:1:3:0|b:0:1:0|b",
            "picked": "c:2:3:0|c:2:3:0|b:0:1:0|c",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Decided by the coordinator (2026-11-12, for the user to confirm): a table-row component whose
// root is a branch is a row of its table in the browser too, as on the server's page, not one in
// a <quillo-island> inside <tbody> (where `tbody.rows` and `tbody > tr` missed it).
#[test]
fn a_table_row_component_whose_root_is_a_branch_is_a_row_in_the_browser_too() {
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
        <button #pre @click={() => { xs = [{ id: 8, done: true }] + xs }}>pre</button>
    </div>
}
component Host(now: bool) {
    state on = now
    <div><button #show @click={() => { on = true }}>show</button>{on ? <Tbl/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const rows = () => [...$("tb").rows].map((e) => e.textContent).join(",") + "|" + $("tb").querySelectorAll("quillo-island").length + "|" + $("tb").querySelectorAll(":scope > tr").length;
    const out = { first: rows() };
    await click("mark", "rev"); out.markRev = rows();
    await click("del", "pre"); out.delPre = rows();
    return out;"##;
    let Some(rs) = in_pages("q889-tr-root", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "first": "todo 1,todo 2,todo 3|0|3", "markRev": "todo 3,done 2,todo 1|0|3", "delPre": "done 8,todo 3,todo 1|0|3",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-889, found in review of 0851ec8 (8911ac4's recheck): a list's record stayed whole once the
// list had gone, so one row element a script kept held every row's record and item through its
// list's marker (1.2 MB, against main's 0.33). It is let go after the task that took the rows
// away. Each row's item here has 400 bytes only the list holds: kept, 2,000 of them would be
// some 1,100 KB more than with nothing kept; let go, the kept row and its own list, some 60 to
// 140 KB.
#[test]
fn a_row_element_a_script_keeps_keeps_no_removed_list_alive() {
    let src = r###"fn make(n) = range(0, n) | map((i) => { id: i, t: `row ${i}`, pad: repeat("x", 400) + str(i) })
component Held() {
    state on = false
    <div>
        <button #tog @click={() => { on = !on }}>tog</button>
        {on ? <ul #l>{for x in make(2000) => <li key={x.id}>{x.t}</li>}</ul> : <p>gone</p>}
    </div>
}
route GET / = <html><head><title>t</title></head><body><Held/></body></html>
"###;
    let body = r##"const heap = async () => { for (let k = 0; k < 3; k++) { await tick(50); gc(); } await tick(50); return performance.memory.usedJSHeapSize; };
    const base = await heap();
    await click("tog", "tog");
    const unheld = (await heap()) - base;
    await click("tog");
    let keep = [document.querySelectorAll("#l li")[700]];
    await click("tog");
    const held = (await heap()) - base;
    const out = { kept: keep[0].textContent, more: held - unheld < 600 * 1024 ? "under 600 KB" : `${Math.round((held - unheld) / 1024)} KB` };
    keep = null;
    return out;"##;
    let Some(rs) = in_pages("q889-held", src, &["/"], body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "kept": "row 700", "more": "under 600 KB", "remade": 0, "errors": [] }));
}

// Q-892's edges: a component placed in the page that defers (src/defer.rs) is swapped in later,
// and those inside it marked by comments are found only if it says it holds some (`data-qm`), as
// a component written at once does: else the state of one (a random number the browser could not
// make again) was not found, and the component was made again.
#[test]
fn a_deferred_component_takes_up_the_table_parts_marked_inside_it() {
    let src = r###"server fn load() -> list {
    sleep(100)
    ["a", "b"]
}
component Rows(xs: list) {
    state r = random_int(1, 1000000000)
    {for x in xs => <tr key={x} class="r"><td>{x} {r}</td></tr>}
}
component Slow() {
    state rows = defer(load())
    <div><table><tbody #tb><Rows xs={rows ?? []}/></tbody></table><p #n>{len(rows ?? [])}</p></div>
}
route GET / = <html><head><title>t</title></head><body><Slow/></body></html>
"###;
    let body = r##"for (let i = 0; i < 100 && $("n").textContent !== "2"; i++) await tick(30);
    await tick(100);
    const out = { n: $("n").textContent, rows: $("tb").rows.length, marked: [...$("tb").childNodes].filter((x) => x.nodeType === 8).length, remadeAtLoad: QW.remade ?? 0 };
    const cell = $("tb").rows[0].cells[0];
    out.same = /^a \d+$/.test(cell.textContent) && cell.textContent.split(" ")[1] === $("tb").rows[1].cells[0].textContent.split(" ")[1];
    return out;"##;
    let Some(rs) = in_pages("q892-defer", src, &["/"], body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "n": "2", "rows": 2, "marked": 0, "remadeAtLoad": 0, "same": true, "remade": 0, "errors": [] }));
}

// Q-892's edges: a component of table parts marked by comments whose code fails in the browser
// (`js` throws there, gives null on the server) keeps what the server wrote, as one in its own
// <quillo-island> does, and the page around it is taken up: the error is told, nothing made again.
#[test]
fn a_marked_component_whose_code_fails_keeps_the_servers_rows() {
    let src = r###"component Fails() {
    let _bad = js("JSON.parse", "{not json")
    [<tr class="f"><td>a</td></tr>, <tr class="f"><td>b</td></tr>]
}
component Page() {
    state n = 0
    <div><table><tbody #tb><Fails/><tr><td #n>{n}</td></tr></tbody></table><button #inc @click={() => { n += 1 }}>inc</button></div>
}
route GET / = <html><head><title>t</title></head><body><Page/></body></html>
"###;
    let body = r##"const out = { rows: [...$("tb").rows].map((r) => r.textContent).join(","), marked: [...$("tb").childNodes].filter((x) => x.nodeType === 8).length };
    await click("inc");
    out.n = $("n").textContent;
    out.told = (window.__quilloErrors || []).length > 0;
    window.__quilloErrors = [];
    return out;"##;
    let Some(rs) = in_pages("q892-fails", src, &["/"], body) else { return };
    assert_eq!(rs[0], serde_json::json!({ "rows": "a,b,0", "marked": 0, "n": "1", "told": true, "remade": 0, "errors": [] }));
}

// Q-892's edges: what the server marked by comments is not what the browser makes there (another
// component named, its closing comment gone, a row more: changed before anyone reached for the
// component, which is when it is taken up). The component around it is made again, as when one in
// its own <quillo-island> differs, and shows the browser's rows, with no comment or wrapper left.
#[test]
fn marked_comments_that_are_not_what_the_browser_makes_make_their_page_again() {
    let src = r###"component Two(a: str) { [<tr class="t"><td>{a}1</td></tr>, <tr class="t"><td>{a}2</td></tr>] }
component Wrap() { <div><table><tbody #tb><Two a="s"/></tbody></table><button #b>b</button></div> }
route GET / = <html><head><title>t</title></head><body><div><Wrap/></div></body></html>
"###;
    let body = r##"const marks = () => [...$("tb").childNodes].filter((x) => x.nodeType === 8);
    const [open, close] = marks();
    const at = location.search.slice(1);
    if (at === "name") open.data = open.data.replace('data-c="Two"', 'data-c="Nope"');
    else if (at === "end") close.remove();
    else { const tr = document.createElement("tr"); tr.innerHTML = "<td>more</td>"; close.before(tr); }
    $("b").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    for (let i = 0; i < 100 && !(QW.remade > 0); i++) await tick(20);
    await tick(50);
    return { rows: [...$("tb").rows].map((r) => r.textContent).join(","), marked: marks().length, islands: $("tb").querySelectorAll("quillo-island").length };"##;
    let paths = ["/?name", "/?end", "/?extra"];
    let Some(rs) = in_pages("q892-otherwise", src, &paths, body) else { return };
    for (r, path) in rs.iter().zip(paths) {
        assert_eq!(r, &serde_json::json!({ "rows": "s1,s2", "marked": 0, "islands": 0, "remade": 1, "errors": [] }), "at {}", path);
    }
}

// ---- Slice 3's review (16dfab4) ------------------------------------------------------------------

// Q-890's review: with function markup no longer written over, a branch switched for another of
// the same shape (`? :`, `if`, `match`) was the old branch's elements given the new attributes,
// so the next step's fields held what was typed, ticked, picked or opened in the last, and a
// password went into the other form's field. A branch is made anew again, as the guide says. The
// edges: switched back (afresh again), a field bound to a state in both (it shows the state).
#[test]
fn a_branch_switched_for_one_of_the_same_shape_starts_afresh() {
    let src = r###"component Wizard() {
    state step = 1
    state mode = "in"
    state kept = ""
    <div>
        {step == 1 ? <input #a name="user"/> : <input #b name="code"/>}
        {step == 1 ? <input #ka type="checkbox" name="news"/> : <input #kb type="checkbox" name="terms"/>}
        {if step == 1 { <details #da><summary>s</summary>a</details> } else { <details #db><summary>s</summary>b</details> }}
        {if step == 1 { <select #sa><option>a1</option><option>a2</option></select> } else { <select #sb><option>b1</option><option>b2</option></select> }}
        {match step { 1 => <textarea #ta></textarea>, _ => <textarea #tb></textarea> }}
        {step == 1 ? <input #ba value={kept}/> : <input #bb value={kept}/>}
        {mode == "in" ? <form #fi action="/in"><input #pi name="password" type="password"/></form> : <form #fu action="/up"><input #pu name="new_password" type="password"/></form>}
        <button #next @click={() => { step = step == 1 ? 2 : 1 }}>next</button>
        <button #mode @click={() => { mode = mode == "in" ? "up" : "in" }}>mode</button>
    </div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Wizard/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const typeIn = (id, text) => { const el = $(id); el.focus(); el.select(); document.execCommand("insertText", false, text); };
    typeIn("a", "alice"); $("ka").click(); $("da").open = true;
    $("sa").value = "a2"; $("sa").dispatchEvent(new Event("change", { bubbles: true }));
    typeIn("ta", "secret note"); typeIn("ba", "bound"); typeIn("pi", "hunter2");
    await tick(30);
    await click("next", "mode");
    const out = {};
    out.next = [$("b").value, $("kb").checked, $("db").open, $("sb").value, $("tb").value, $("bb").value].join("|");
    out.form = new URLSearchParams(new FormData($("fu"))).toString() + "@" + $("fu").getAttribute("action");
    await click("next", "mode");
    out.back = [$("a").value, $("ka").checked, $("da").open, $("sa").value, $("ta").value, $("ba").value].join("|");
    out.formBack = new URLSearchParams(new FormData($("fi"))).toString();
    return out;"##;
    let Some(rs) = in_pages("q890-branches", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "next": "|false|false|b1||bound", "form": "new_password=@/up", "back": "|false|false|a1||bound", "formBack": "password=",
            "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-890's review: a select in function markup whose options and value changed in one update was
// given its value before its options, and showed another option. It shows its value once its
// options are as the markup says. The edges: back again; one with no value keeps the person's
// pick through it and through a change elsewhere.
#[test]
fn a_select_in_function_markup_shows_its_new_value_among_new_options() {
    let src = r###"fn sel(v, o) = <select #s value={v}><option>{o[0]}</option><option>{o[1]}</option><option>{o[2]}</option></select>
fn same(o) = <select #t><option>{o[0]}</option><option>{o[1]}</option><option>{o[2]}</option></select>
component Sel() {
    state v = "q"
    state opts = ["p", "q", "r"]
    state n = 0
    <div>{sel(v, opts)}{same(opts)}<span>{n}</span>
        <button #swap @click={() => { opts = ["x", "y", "z"]; v = "y" }}>swap</button>
        <button #back @click={() => { opts = ["p", "q", "r"]; v = "r" }}>back</button>
        <button #inc @click={() => { n += 1 }}>inc</button>
    </div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Sel/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const out = { first: [$("s").value, $("t").value].join("|") };
    $("t").value = "r"; $("t").dispatchEvent(new Event("change", { bubbles: true }));
    await click("swap");
    out.swap = [$("s").value, [...$("s").options].map((o) => o.text).join(","), $("t").value].join("|");
    await click("inc");
    out.inc = [$("s").value, $("t").value].join("|");
    await click("back");
    out.back = [$("s").value, $("t").value].join("|");
    return out;"##;
    let Some(rs) = in_pages("q890-select", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({ "first": "q|p", "swap": "y|x,y,z|z", "inc": "y|z", "back": "r|r", "remade": 0, "errors": [] });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-891's review: the caret kept its distance from the end, which is wrong when the write changes
// the text after it (ß in capitals is SS; a card number's groups move). It stays where it was in
// what the old and new text begin with, as far from the end in what they end with, and after the
// new text where they differ. Typed at the start, the middle and the end: capitals with ß, a card
// number in fours, a phone number, a trim. And the `select` the browser sends for the caret put
// back is not the program's (a handler had run on every key typed in the middle).
#[test]
fn the_caret_stays_where_it_was_in_the_text_both_have() {
    let src = r###"fn card(s) {
    let d = slice(re.replace(s, "[^0-9]", ""), 0, 16)
    join(range(0, floor((len(d) + 3) / 4)) | map((i) => slice(d, i * 4, i * 4 + 4)), " ")
}
fn phone(s) {
    let d = slice(re.replace(s, "[^0-9]", ""), 0, 10)
    len(d) <= 3 ? d : len(d) <= 6 ? `(${slice(d, 0, 3)}) ${slice(d, 3)}` : `(${slice(d, 0, 3)}) ${slice(d, 3, 6)}-${slice(d, 6)}`
}
component Caret() {
    state big = "aßb"
    state cc = "1234 5678"
    state ph = "(555) 123-4567"
    state up = "AB"
    state tr = "ab"
    state sel = 0
    <div>
        <input #big value={big} @input={(e) => { big = upper(e.value) }}/>
        <input #cc value={cc} @input={(e) => { cc = card(e.value) }}/>
        <input #ph value={ph} @input={(e) => { ph = phone(e.value) }}/>
        <input #up value={up} @input={(e) => { up = upper(e.value) }} @select={() => { sel += 1 }}/>
        <input #tr value={tr} @input={(e) => { tr = trim(e.value) }}/>
        <p #sel>{sel}</p>
    </div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Caret/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const typeAt = async (id, at, keys) => { const el = $(id); el.focus(); el.setSelectionRange(at, at); for (const k of keys) { document.execCommand("insertText", false, k); await tick(20); } return `${el.value}@${el.selectionStart}`; };
    const end = (id) => $(id).value.length;
    const out = {};
    out.ss = await typeAt("big", 1, ["x", "y"]);
    out.ssEnd = await typeAt("big", end("big"), ["q"]);
    out.ssStart = await typeAt("big", 0, ["w"]);
    out.card = await typeAt("cc", 2, ["9", "8"]);
    out.cardEnd = await typeAt("cc", end("cc"), ["1"]);
    out.cardStart = await typeAt("cc", 0, ["0"]);
    out.phone = await typeAt("ph", 3, ["9"]);
    out.phoneEnd = await typeAt("ph", end("ph"), ["8"]);
    out.up = await typeAt("up", 1, ["x", "y", "z"]);
    out.trimStart = await typeAt("tr", 0, [" "]);
    out.trimMid = await typeAt("tr", 1, [" "]);
    out.trimEnd = await typeAt("tr", end("tr"), [" "]);
    await tick(50);
    out.selects = $("sel").textContent;
    return out;"##;
    let Some(rs) = in_pages("q891-caret", src, BOTH, body) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "ss": "AXYSSB@3", "ssEnd": "AXYSSBQ@7", "ssStart": "WAXYSSBQ@1",
            "card": "1298 3456 78@4", "cardEnd": "1298 3456 781@13", "cardStart": "0129 8345 6781@1",
            "phone": "(559) 512-3456@4", "phoneEnd": "(559) 512-3456@14",
            "up": "AXYZB@4", "trimStart": "ab@0", "trimMid": "a b@2", "trimEnd": "a b@3",
            "selects": "1", "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}

// Q-939: a field the program writes as it is typed in (capitals here) was written while a person
// composed text with an input method, which ended the composition and put what was composed in
// twice (AKBKAか for AかB). It is not written while composing; the program's value is written when
// the composition ends. With Chrome's own input method (DevTools' Input.imeSetComposition).
#[test]
fn a_field_being_composed_in_is_written_once_the_composition_ends() {
    let src = r###"component Ime() {
    state code = "AB"
    <div><input #code value={code} @input={(e) => { code = upper(e.value) }}/><p #said>{code}</p></div>
}
component Host(now: bool) {
    state shown = now
    <div><button #show @click={() => { shown = true }}>show</button>{shown ? <Ime/> : null}</div>
}
route GET /h = <html><head><title>t</title></head><body><Host now={true}/></body></html>
route GET /b = <html><head><title>t</title></head><body><Host now={false}/></body></html>
"###;
    let body = r##"const el = $("code"); el.focus(); el.setSelectionRange(1, 1);
    const cdp = async (cmds) => { document.body.setAttribute("data-cdp", JSON.stringify(cmds)); for (let i = 0; i < 250 && document.body.hasAttribute("data-cdp"); i++) await tick(20); await tick(80); };
    const out = {};
    await cdp([["Input.imeSetComposition", { text: "k", selectionStart: 1, selectionEnd: 1 }]]);
    out.k = el.value;
    await cdp([["Input.imeSetComposition", { text: "ka", selectionStart: 2, selectionEnd: 2 }]]);
    out.ka = el.value;
    await cdp([["Input.insertText", { text: "か" }]]);
    await tick(100);
    out.done = `${el.value}@${el.selectionStart}|${$("said").textContent}`;
    el.setSelectionRange(2, 2);
    await cdp([["Input.imeSetComposition", { text: "e", selectionStart: 1, selectionEnd: 1 }]]);
    out.e = el.value;
    await cdp([["Input.insertText", { text: "é" }]]);
    await tick(100);
    out.accent = `${el.value}@${el.selectionStart}|${$("said").textContent}`;
    return out;"##;
    let Some(rs) = in_pages_with("q939", src, BOTH, body, true) else { return };
    for (r, path) in rs.iter().zip(BOTH) {
        let want = serde_json::json!({
            "k": "AkB", "ka": "AkaB", "done": "AかB@2|AかB", "e": "AかeB", "accent": "AかÉB@3|AかÉB", "remade": 0, "errors": [],
        });
        assert_eq!(r, &want, "at {}", path);
    }
}
