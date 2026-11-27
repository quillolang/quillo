//! FEEDBACK86-89, slice C (FEEDBACK88.md, the single-page-app course): a component whose draw
//! failed in the browser (a typed view given a field of a server fn's state, null while it loads)
//! is drawn again on its next state change, each failure said once and nothing drawn again without
//! a change (Q-1380); and a server fn that redirects gives the page's call an error value that says
//! so, the redirect not followed (Q-1381). Branch feedback86-89.

mod support;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(name: &str, src: &str) -> Dir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = support::temp_dir(&format!("f8689c-{}-{}", name, N.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        Dir(d)
    }
}

fn quillo() -> PathBuf {
    std::env::var_os("QUILLO_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_quillo")))
}

fn wait_for(c: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            return Some(s);
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Served {
    dir: Dir,
    log: PathBuf,
    port: u16,
    _guard: support::Guard,
}

/// `src` checked (it passes), then served on a port the system picks, its log in a file.
fn served(name: &str, src: &str) -> Served {
    served_with(name, src, &[])
}

/// `served`, with more options for `quillo run` (`--watch`).
fn served_with(name: &str, src: &str, opts: &[&str]) -> Served {
    served_files(name, src, &[], opts)
}

/// `served_with`, with other files beside main.qwe (a mounted program's).
fn served_files(name: &str, src: &str, files: &[(&str, &str)], opts: &[&str]) -> Served {
    let dir = Dir::new(name, src);
    for (f, text) in files {
        std::fs::write(dir.0.join(f), text).unwrap();
    }
    let out = dir.0.join("check.txt");
    let mut c = Command::new(quillo())
        .arg("check")
        .arg(dir.0.join("main.qwe"))
        .current_dir(&dir.0)
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ok = wait_for(&mut c, Duration::from_secs(120)).is_some_and(|s| s.success());
    assert!(ok, "quillo check:\n{}\n{}", std::fs::read_to_string(&out).unwrap_or_default(), src);
    let log = dir.0.join("log.txt");
    let child = Command::new(quillo())
        .arg("run")
        .arg(dir.0.join("main.qwe"))
        .args(["--port", "0"])
        .args(opts)
        .current_dir(&dir.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut child = child;
    let port = support::port_in_log(&mut child, &log, Duration::from_secs(30));
    Served { dir, log, port, _guard: support::Guard::new(child) }
}

impl Served {
    fn call(&self, f: &str, args: &str, headers: &[(&str, &str)]) -> support::Answer {
        let mut h = vec![("Content-Type", "application/json")];
        h.extend_from_slice(headers);
        support::send(self.port, "POST", &format!("/_quillo/rpc/{}", f), &h, args)
    }
    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// One Chrome at a time: several at once overload small CI machines.
fn chrome_turn() -> std::sync::MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

fn chrome() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output();
    if !node.map(|o| o.status.success()).unwrap_or(false) {
        return None;
    }
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/chromium"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// `body` run in a page at `/_t` beside `src`'s routes, with `open(path)`: the program's page at
/// `path` in an iframe, its window and document, once the runtime has booted; `tick(ms)`, and
/// `wait(f)` until f() holds (false after 5 s). What `body` returns is the result, with the
/// errors the page's runtime said (`errors`); and the server's log.
fn in_chrome(name: &str, src: &str, body: &str) -> Option<(serde_json::Value, String)> {
    in_chrome_with(name, src, body, &[])
}

/// `in_chrome`, with options for `quillo run` (`--watch`).
fn in_chrome_with(name: &str, src: &str, body: &str, opts: &[&str]) -> Option<(serde_json::Value, String)> {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return None;
    };
    let _turn = chrome_turn();
    let src = format!("route GET /_t = <html><head><title>t</title></head><body><p>t</p></body></html>\n{}", src);
    let s = served_with(name, &src, opts);
    // `@STOP@`: an address that stops the server (SIGTERM to its own process), for a page offline.
    let body = if body.contains("@STOP@") {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (port, pid) = (l.local_addr().unwrap().port(), s._guard.0.id());
        std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                let mut c = c;
                let _ = std::io::Read::read(&mut c, &mut [0u8; 2048]);
                let _ = Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
                std::thread::sleep(Duration::from_millis(300));
                let _ = std::io::Write::write_all(&mut c, b"HTTP/1.1 200 OK\r\naccess-control-allow-origin: *\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
            }
        });
        body.replace("@STOP@", &format!("http://127.0.0.1:{}/stop", port))
    } else {
        body.to_string()
    };
    let script = format!(
        r#"(async () => {{
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const wait = async (f, ms = 5000) => {{ const end = Date.now() + ms; while (Date.now() < end) {{ try {{ if (f()) return true; }} catch (_) {{}} await tick(20); }} return false; }};
  let f = null;
  const open = async (path) => {{
    f = document.createElement("iframe");
    f.style.width = "800px"; f.style.height = "600px";
    document.body.appendChild(f);
    f.src = path;
    await new Promise((r) => f.addEventListener("load", r, {{ once: true }}));
    await wait(() => f.contentWindow[Symbol.for("quillo")] && f.contentWindow[Symbol.for("quillo")].booted);
    return [f.contentWindow, f.contentDocument];
  }};
  let out;
  try {{ out = await (async () => {{
{body}
  }})(); }} catch (e) {{ out = {{ exception: String((e && e.stack) || e) }}; }}
  out.errors = f && f.contentWindow.__quilloErrors;
  document.body.dataset.result = JSON.stringify(out);
}})();"#
    );
    let t = s.dir.0.join("t.js");
    std::fs::write(&t, script).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let mut c = Command::new("node")
        .env_remove("QUILLO_PAGE_FOCUS")
        .env_remove("QUILLO_PAGE_CDP")
        .arg(&run)
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/_t", s.port))
        .arg(s.dir.0.join("profile"))
        .arg("60000")
        .arg(&t)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node");
    let (mut so, mut se) = (c.stdout.take().unwrap(), c.stderr.take().unwrap());
    let t1 = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = std::io::Read::read_to_end(&mut so, &mut v);
        v
    });
    let t2 = std::thread::spawn(move || {
        let mut v = String::new();
        let _ = std::io::Read::read_to_string(&mut se, &mut v);
        v
    });
    let status = wait_for(&mut c, Duration::from_secs(120));
    let (stdout, stderr) = (t1.join().unwrap(), t2.join().unwrap());
    assert!(status.is_some_and(|s| s.success()), "{}\n{}", stderr, s.log());
    let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&stdout)));
    assert!(v.get("exception").is_none(), "{}\n{}", v, s.log());
    Some((v, s.log()))
}

// ---- Q-1380: a component whose draw failed is drawn again on its next state change -------------

/// The trail page of the course's lesson 3 (repros/Q-1380), and beside it the cases around it,
/// each shown by a button on `/lab`: a component given a prop that fails its view (`Card`), one
/// whose own code fails (`Shout`), a row standing by itself in its table (`Row`), a branch of a
/// component that fails (`Panel`), and a component that waits on nothing (`Empty`).
const TRAILS: &str = r##"
let trails = [{ id: 5, name: "Lantern Ridge — Überweg 山", level: "hard" }]

server fn load_trail(id: int) -> map? = find(trails, (t) => t.id == id)

view Level(level: str) = <span class="lvl">{level}</span>

component TrailPage(id: int) {
    state trail = load_trail(id)
    <main>
        <h1>{trail.name}</h1>
        <Level level={trail.level}/>
        <button #rename onclick={() => { trail = { id: 5, name: "Renamed", level: "easy" } }}>Rename</button>
    </main>
}

component Card(t: map?) {
    <section #card><Level level={t.level}/></section>
}

component Shout(id: int) {
    state trail = load_trail(id)
    let loud = upper(trail.level)
    <b #loud>{loud}</b>
}

component Row(id: int) {
    state r = load_trail(id)
    <tr><td>{r.id}</td><Cell v={r.name}/></tr>
}

view Cell(v: str) = <td>{v}</td>

component Panel(id: int) {
    state trail = load_trail(id)
    state open = true
    <div #panel>{open ? <Level level={trail.level}/> : <i>closed</i>}</div>
}

fn nothing() -> any = null

component Empty() {
    <p #empty><Level level={nothing()}/></p>
}

component Lab() {
    state t = null
    state card = false
    state shout = false
    state row = false
    state panel = false
    state empty = false
    <div>
        <button #bad onclick={() => { t = { level: 5 } }}>bad</button>
        <button #good onclick={() => { t = { level: "easy" } }}>good</button>
        <button #card-on onclick={() => { card = !card }}>card</button>
        <button #shout-on onclick={() => { shout = !shout }}>shout</button>
        <button #row-on onclick={() => { row = !row }}>row</button>
        <button #panel-on onclick={() => { panel = !panel }}>panel</button>
        <button #empty-on onclick={() => { empty = !empty }}>empty</button>
        {card ? <Card t={t}/> : null}
        {shout ? <Shout id={5}/> : null}
        <table><tbody #rows>{row ? <Row id={5}/> : null}</tbody></table>
        {panel ? <Panel id={5}/> : null}
        {empty ? <section #around><h3>around</h3><Empty/></section> : null}
    </div>
}

component App() {
    let path = url().path
    <div id="page">{path == "/" ? <a #go href="/trails/5">Open the trail</a> : path == "/lab" ? <Lab/> : <TrailPage id={5}/>}</div>
}

route GET /*rest = <html><body><App/></body></html>
"##;

const NULL_LEVEL: &str = "argument `level` of `Level` must be str, got null null";

fn calls_in(log: &str, f: &str) -> usize {
    log.matches(&format!("POST /_quillo/rpc/{} ", f)).count()
}

/// The report's case: a script's click is a tap (no hover, so no page asked for ahead), the trail
/// page is drawn in the browser while its state is null, its view fails, said once; the answer
/// draws it, whole and once, its text as the server gave it; then it follows its state in place,
/// as any component does, and is not drawn whole again. Before the fix: no `<main>` for good.
#[test]
fn a_page_drawn_while_its_server_fn_state_loads_is_drawn_when_the_answer_comes() {
    let Some((v, log)) = in_chrome(
        "trail",
        TRAILS,
        r##"
  const [w, d] = await open("/");
  d.getElementById("go").click();
  const drawn = await wait(() => d.querySelector("#page h1") && d.querySelector("#page h1").textContent !== "");
  const h1 = d.querySelector("#page h1");
  const r = { drawn, path: w.location.pathname, h1: h1 && h1.textContent, level: d.querySelector(".lvl") && d.querySelector(".lvl").textContent, mains: d.querySelectorAll("main").length };
  await tick(200);
  r.errorsThen = w.__quilloErrors.length;
  d.getElementById("rename").click();
  await wait(() => d.querySelector("#page h1").textContent === "Renamed");
  r.renamed = d.querySelector("#page h1").textContent;
  r.sameH1 = d.querySelector("#page h1") === h1;
  r.mainsAfter = d.querySelectorAll("main").length;
  return r;
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}\n{}", v, log);
    assert_eq!(v["path"], "/trails/5");
    assert_eq!(v["h1"], "Lantern Ridge — Überweg 山", "{}", v);
    assert_eq!(v["level"], "hard");
    assert_eq!(v["mains"], 1);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
    assert_eq!(v["errorsThen"], 1);
    assert_eq!(v["renamed"], "Renamed");
    assert_eq!(v["sameH1"], true, "drawn again whole on a change after it was drawn: {}", v);
    assert_eq!(v["mainsAfter"], 1);
    assert_eq!(calls_in(&log, "load_trail"), 1, "{}", log);
}

/// A component given a prop its view refuses (null, then an int) is drawn again at each change of
/// the prop, each failure said once, and nothing is drawn again while nothing changes; the third
/// value draws it. Before the fix the first failure emptied the place for good.
#[test]
fn a_draw_that_fails_again_waits_for_the_next_change_and_says_each_failure_once() {
    let Some((v, _)) = in_chrome(
        "again",
        TRAILS,
        r##"
  const [w, d] = await open("/lab");
  d.getElementById("card-on").click();
  await wait(() => w.__quilloErrors.length >= 1);
  await tick(300);
  const r = { first: w.__quilloErrors.slice(), card: !!d.getElementById("card"), island: !!d.querySelector('quillo-island[data-c="Card"]') };
  d.getElementById("bad").click();
  await wait(() => w.__quilloErrors.length >= 2);
  await tick(300);
  r.second = w.__quilloErrors.length;
  r.cardAfterBad = !!d.getElementById("card");
  d.getElementById("good").click();
  r.drawn = await wait(() => d.querySelector("#card .lvl") && d.querySelector("#card .lvl").textContent === "easy");
  r.cards = d.querySelectorAll("#card").length;
  return r;
"##,
    ) else {
        return;
    };
    assert_eq!(v["first"], serde_json::json!([NULL_LEVEL]), "{}", v);
    assert_eq!(v["card"], false, "{}", v);
    assert_eq!(v["island"], true, "the component stands, empty, where it goes: {}", v);
    assert_eq!(v["second"], 2, "{}", v);
    assert_eq!(v["cardAfterBad"], false);
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["cards"], 1);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL, "argument `level` of `Level` must be str, got int 5"]), "{}", v);
}

/// A component whose draw failed and that has gone with its place waits on nothing: the prop's
/// change draws nothing and says nothing; shown again, it is a new one, drawn at once.
#[test]
fn a_component_whose_draw_failed_and_went_is_not_drawn_again() {
    let Some((v, _)) = in_chrome(
        "gone",
        TRAILS,
        r##"
  const [w, d] = await open("/lab");
  d.getElementById("card-on").click();
  await wait(() => w.__quilloErrors.length >= 1);
  const stood = !!d.querySelector('quillo-island[data-c="Card"]');
  d.getElementById("card-on").click();
  await wait(() => !d.querySelector('quillo-island[data-c="Card"]'));
  d.getElementById("good").click();
  await tick(300);
  const r = { stood, gone: !d.querySelector('quillo-island[data-c="Card"]') && !d.getElementById("card"), errorsThen: w.__quilloErrors.length };
  d.getElementById("card-on").click();
  r.drawn = await wait(() => d.querySelector("#card .lvl") && d.querySelector("#card .lvl").textContent === "easy");
  return r;
"##,
    ) else {
        return;
    };
    assert_eq!(v["stood"], true, "{}", v);
    assert_eq!(v["gone"], true, "{}", v);
    assert_eq!(v["errorsThen"], 1, "{}", v);
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
}

/// The component's own code fails (`upper` of the loading state's field), not its markup: drawn
/// again with the answer, its state loaded once.
#[test]
fn a_component_whose_own_code_failed_is_drawn_again() {
    let Some((v, log)) = in_chrome(
        "own",
        TRAILS,
        r##"
  const [w, d] = await open("/lab");
  d.getElementById("shout-on").click();
  const drawn = await wait(() => d.getElementById("loud") && d.getElementById("loud").textContent === "HARD");
  return { drawn, n: d.querySelectorAll("#loud").length };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["n"], 1);
    assert_eq!(v["errors"].as_array().map(Vec::len), Some(1), "{}", v);
    assert!(v["errors"][0].as_str().unwrap_or("").contains("upper"), "{}", v);
    assert_eq!(calls_in(&log, "load_trail"), 1, "{}", log);
}

/// A row component stands by itself in its table: drawn again there, a row of the table (no
/// wrapper), and taken away with its place, then shown and drawn again.
#[test]
fn a_row_standing_by_itself_is_drawn_in_its_table_and_goes_with_its_place() {
    let Some((v, _)) = in_chrome(
        "row",
        TRAILS,
        r##"
  const [w, d] = await open("/lab");
  const rows = d.getElementById("rows");
  d.getElementById("row-on").click();
  const drawn = await wait(() => rows.rows.length === 1 && rows.rows[0].cells.length === 2);
  const tr = rows.querySelector("tr");
  const r = { drawn, cells: Array.from(tr.cells, (c) => c.textContent), own: tr.parentNode === rows && tr.hasAttribute("data-qi"), wrappers: rows.querySelectorAll("quillo-island").length };
  d.getElementById("row-on").click();
  r.goneRows = await wait(() => rows.rows.length === 0);
  r.elements = rows.children.length;
  d.getElementById("row-on").click();
  r.again = await wait(() => rows.rows.length === 1 && rows.rows[0].cells.length === 2);
  return r;
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["cells"], serde_json::json!(["5", "Lantern Ridge — Überweg 山"]), "{}", v);
    assert_eq!(v["own"], true, "{}", v);
    assert_eq!(v["wrappers"], 0);
    assert_eq!(v["goneRows"], true, "{}", v);
    assert_eq!(v["elements"], 0, "{}", v);
    assert_eq!(v["again"], true, "{}", v);
    assert_eq!(v["errors"].as_array().map(Vec::len), Some(2), "one failure for each draw while loading: {}", v);
}

/// A branch of a component whose markup fails is emptied, as a place whose value fails is, and
/// made again when one of the component's states next changes (the answer), not only when what
/// the branch reads (`open`) does.
#[test]
fn a_branch_whose_markup_failed_is_made_again_on_its_components_next_change() {
    let Some((v, _)) = in_chrome(
        "branch",
        TRAILS,
        r##"
  const [w, d] = await open("/lab");
  d.getElementById("panel-on").click();
  const drawn = await wait(() => d.querySelector("#panel .lvl") && d.querySelector("#panel .lvl").textContent === "hard");
  return { drawn, n: d.querySelectorAll("#panel .lvl").length };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["n"], 1);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
}

/// A component with no state and no prop whose draw fails waits on nothing: said once, never drawn
/// again by itself; the markup around it, its parent's, stands (it had gone with it).
#[test]
fn a_draw_with_nothing_to_wait_on_fails_once_and_the_markup_around_it_stays() {
    let Some((v, _)) = in_chrome(
        "empty",
        TRAILS,
        r##"
  const [w, d] = await open("/lab");
  d.getElementById("empty-on").click();
  await wait(() => w.__quilloErrors.length >= 1);
  d.getElementById("bad").click();
  await tick(400);
  return { around: !!d.getElementById("around") && d.querySelector("#around h3").textContent, empty: !!d.getElementById("empty") };
"##,
    ) else {
        return;
    };
    assert_eq!(v["around"], "around", "{}", v);
    assert_eq!(v["empty"], false);
    assert_eq!(v["errors"], serde_json::json!(["argument `level` of `Level` must be str, got null null"]), "{}", v);
}

/// The form that must keep working: the trail page opened at its address comes whole from the
/// server and is taken up as it is, with no error and no call.
#[test]
fn a_page_opened_at_its_address_is_whole_and_taken_up() {
    let Some((v, log)) = in_chrome(
        "whole",
        TRAILS,
        r##"
  const [w, d] = await open("/trails/5");
  const h1 = d.querySelector("#page h1"), r = { first: h1.textContent, level: d.querySelector(".lvl").textContent };
  d.getElementById("rename").click();
  await wait(() => d.querySelector("#page h1").textContent === "Renamed");
  r.h1 = h1.textContent === "Renamed" && d.querySelector("#page h1") === h1;
  return r;
"##,
    ) else {
        return;
    };
    assert_eq!(v["first"], "Lantern Ridge — Überweg 山", "{}", v);
    assert_eq!(v["level"], "hard");
    assert_eq!(v["h1"], true, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
    assert_eq!(calls_in(&log, "load_trail"), 0, "{}", log);
}

/// What a component's states start lives on through a failed draw (the review of slice C): a
/// `live` state's subscription and the effect that follows its arguments (one a `let` gives), a
/// timer an initializer started (`every`); a self-standing row whose place failed after making it
/// is not drawn into nothing; and options drawn again in a `<select>` show its value.
const LAB: &str = r##"db.open(":memory:")
db.exec("create table trails (id integer primary key, name text, level text)")
db.exec("insert into trails (id, name, level) values (5, 'Lantern Ridge', 'hard'), (6, 'Otter Bend', 'easy')")

server fn load_trail(id: int) -> map? = db.one("select id, name, level from trails where id = ?", id)

route POST /api/rename { json: { name: str } } = db.exec("update trails set name = ? where id = 5", name)

view Level(level: str) = <span class="lvl">{level}</span>
view Cell(v: str) = <td>{v}</td>
view Opt(v: str) = <option value={v}>{v}</option>

component LivePage(id: int) {
    let key = id * 1
    state trail = live(load_trail, key)
    <main #lp>
        <h1>{trail.name}</h1>
        <Level level={trail.level}/>
    </main>
}

fn tick_into(f: fn) -> int {
    every(100, f)
    return 0
}

component Ticker(id: int) {
    state trail = load_trail(id)
    state count = 0
    state started = tick_into(() => { count = count + 1 })
    <p #tk><b>{count}</b><Level level={trail.level}/></p>
}

component Row(id: int) {
    state r = load_trail(id)
    <tr><td>{r.id}</td><Cell v={r.name}/></tr>
}

component Opts(id: int) {
    state r = load_trail(id)
    <optgroup label="levels"><option value="easy">easy</option><Opt v={r.level}/></optgroup>
}

fn nothing() -> any = null

component Lab() {
    state id = 5
    state ticker = false
    state row = false
    state opts = false
    state t = nothing()
    state v = "hard"
    <div>
        <button #next onclick={() => { id = 6 }}>next</button>
        <button #ticker-on onclick={() => { ticker = true }}>ticker</button>
        <button #row-on onclick={() => { row = true }}>row</button>
        <button #opts-on onclick={() => { opts = true }}>opts</button>
        <LivePage id={id}/>
        {ticker ? <Ticker id={5}/> : null}
        <table><tbody #rows>{row ? [<Row id={5}/>, <Cell v={t}/>] : null}</tbody></table>
        <select #sel value={v}>{opts ? <Opts id={5}/> : null}</select>
    </div>
}

component App() {
    let path = url().path
    <div id="page">{path == "/" ? <a #go href="/l">go</a> : <Lab/>}</div>
}

route GET /*rest = <html><body><App/></body></html>
"##;

/// A `live` state of a component whose first draw failed still follows the server: a change of
/// its row reaches the component drawn again, and a new `id` (through a `let`) loads again. Before
/// the fix the failed draw ended its subscription, and the component kept the old name and id.
#[test]
fn a_live_state_follows_the_server_after_a_failed_draw() {
    let Some((v, log)) = in_chrome(
        "live",
        LAB,
        r##"
  const [w, d] = await open("/");
  d.getElementById("go").click();
  const drawn = await wait(() => d.querySelector("#lp h1") && d.querySelector("#lp h1").textContent === "Lantern Ridge");
  await tick(500);
  const res = await w.fetch("/api/rename", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name: "Renamed" }) });
  const renamed = await wait(() => d.querySelector("#lp h1").textContent === "Renamed");
  d.getElementById("next").click();
  const next = await wait(() => d.querySelector("#lp h1").textContent === "Otter Bend" && d.querySelector("#lp .lvl").textContent === "easy");
  return { drawn, status: res.status, renamed, next, h1: d.querySelector("#lp h1").textContent, mains: d.querySelectorAll("#lp").length };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}\n{}", v, log);
    assert_eq!(v["status"], 200, "{}", v);
    assert_eq!(v["renamed"], true, "{}\n{}", v, log);
    assert_eq!(v["next"], true, "{}\n{}", v, log);
    assert_eq!(v["mains"], 1);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
}

/// A timer a state's initializer started (`every`) keeps ticking once the component is drawn
/// again. Before the fix the failed draw cleared it with the draw's own.
#[test]
fn a_timer_a_state_started_keeps_ticking_after_a_failed_draw() {
    let Some((v, _)) = in_chrome(
        "ticker",
        LAB,
        r##"
  const [w, d] = await open("/l");
  d.getElementById("ticker-on").click();
  const drawn = await wait(() => d.querySelector("#tk .lvl"));
  const at = +d.querySelector("#tk b").textContent;
  await tick(700);
  return { drawn, more: +d.querySelector("#tk b").textContent - at };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert!(v["more"].as_i64().unwrap_or(0) >= 3, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
}

/// A row standing by itself whose place fails after making it (the cell beside it) is never on
/// the page: its answer draws nothing into nothing, and says nothing more. Before the fix: "Cannot
/// read properties of null (reading 'insertBefore')".
#[test]
fn a_row_whose_place_failed_after_making_it_is_not_drawn_into_nothing() {
    let Some((v, _)) = in_chrome(
        "rowfail",
        LAB,
        r##"
  const [w, d] = await open("/l");
  d.getElementById("row-on").click();
  await wait(() => w.__quilloErrors.length >= 2);
  await tick(500);
  return { rows: d.getElementById("rows").rows.length };
"##,
    ) else {
        return;
    };
    assert_eq!(v["rows"], 0, "{}", v);
    let cell = "argument `v` of `Cell` must be str, got null null";
    assert_eq!(v["errors"], serde_json::json!([cell, cell]), "{}", v);
}

/// Options a component in a `<select>` draws again show the select's value, as a place's do.
/// Before the fix the select showed the first option.
#[test]
fn options_drawn_again_in_a_select_show_its_value() {
    let Some((v, _)) = in_chrome(
        "select",
        LAB,
        r##"
  const [w, d] = await open("/l");
  d.getElementById("opts-on").click();
  const drawn = await wait(() => d.getElementById("sel").options.length === 2);
  return { drawn, value: d.getElementById("sel").value, group: !!d.querySelector("#sel > optgroup[data-qi]") };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["value"], "hard", "{}", v);
    assert_eq!(v["group"], true, "{}", v);
    assert_eq!(v["errors"], serde_json::json!(["argument `v` of `Opt` must be str, got null null"]), "{}", v);
}

/// A search box over a list whose row `b` gives a typed view a field of a state still loading, and
/// a part that reads the box and the state alike: typed in, five letters, the failures are said
/// once each, though each key makes them again; the answer draws both. Before the fix each key
/// said both failures again: twelve.
const SEARCH: &str = r##"server fn load_extra() -> map? {
    sleep(700)
    return { level: "hard" }
}
view Level(level: str) = <b class="lvl">{level}</b>
fn tag(q: str, x: map?) {
    return <span class="tag">{q}<Level level={x.level}/></span>
}
component Search() {
    state q = ""
    state extra = load_extra()
    <div>
        <input #q value={q} oninput={(e) => { q = e.target.value }}/>
        <p #echo>{q}</p>
        <ul #list>{for r in ["a", "b", "c"] => <li>{r}{r == "b" ? <Level level={extra.level}/> : <i>{r}</i>}</li>}</ul>
        <div #tagged>{tag(q, extra)}</div>
    </div>
}
component Box() {
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        {on ? <Search/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_lasting_failure_is_said_once() {
    let Some((v, _)) = in_chrome(
        "search",
        SEARCH,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => w.__quilloErrors.length >= 2 && d.getElementById("q"));
  const q = d.getElementById("q");
  for (const ch of "trail") { q.value += ch; q.dispatchEvent(new w.Event("input", { bubbles: true })); await tick(30); }
  const typed = { echo: d.getElementById("echo").textContent, errors: w.__quilloErrors.length, loaded: !!d.querySelector("#list .lvl") };
  const drawn = await wait(() => d.querySelector("#list .lvl") && d.querySelector("#tagged .lvl"));
  return { typed, drawn, row: d.querySelectorAll("#list li")[1].textContent, tag: d.querySelector("#tagged .tag").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!(v["typed"], serde_json::json!({ "echo": "trail", "errors": 2, "loaded": false }), "{}", v);
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["row"], "bhard");
    assert_eq!(v["tag"], "trailhard");
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL, NULL_LEVEL]), "{}", v);
}

/// A row and an SVG `<g>`, each a component that is its own root, swapped under --watch for code
/// whose first draw fails (a new state loading): drawn once the answer comes, each its own island
/// again, the row among its table's rows and the `<g>` drawn in its `<svg>`. Before the fix each
/// stayed in the `<quillo-island>` its swap made it in.
const SWAP1: &str = r##"let stage = 0
route GET /stage/:n(int) {
    stage = n
    return "ok"
}
route GET /stage = stage
server fn load_level() -> map? {
    return { level: "hard" }
}
view Level(level: str) = <td class="lvl">{level}</td>
view Dot(level: str) = <circle r="3" class={level}/>
component Row() {
    state n = 1
    <tr><td>{n}</td></tr>
}
component Shape() {
    state n = 1
    <g><circle r={n}/></g>
}
route GET / = <html><body><table><tbody id="rows"><Row/></tbody></table><svg id="pic"><Shape/></svg></body></html>
"##;
const SWAP2: &str = r##"let stage = 0
route GET /stage/:n(int) {
    stage = n
    return "ok"
}
route GET /stage = stage
server fn load_level() -> map? {
    return { level: "hard" }
}
view Level(level: str) = <td class="lvl">{level}</td>
view Dot(level: str) = <circle r="3" class={level}/>
component Row() {
    state n = 1
    state extra = load_level()
    <tr><td>{n}</td><Level level={extra.level}/></tr>
}
component Shape() {
    state n = 1
    state extra = load_level()
    <g class={extra == null ? "" : extra.level}><Dot level={extra.level}/></g>
}
route GET / = <html><body><table><tbody id="rows"><Row/></tbody></table><svg id="pic"><Shape/></svg></body></html>
"##;

#[test]
fn a_row_and_an_svg_part_drawn_after_a_failed_swap_are_their_own_islands() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let _turn = chrome_turn();
    let s = served_with("swap", SWAP1, &["--watch"]);
    let script = r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const wait = async (f, ms = 15000) => { const end = Date.now() + ms; while (Date.now() < end) { try { if (f()) return true; } catch (_) {} await tick(25); } return false; };
  const QW = () => window[Symbol.for("quillo")] || {};
  const rows = document.getElementById("rows"), pic = document.getElementById("pic");
  // Both taken up: reached for once the runtime is in.
  rows.querySelector("tr").dispatchEvent(new Event("focusin", { bubbles: true }));
  await wait(() => QW().booted);
  for (const el of [rows.querySelector("tr"), pic.querySelector("g")]) el.dispatchEvent(new Event("focusin", { bubbles: true }));
  const up = await wait(() => (QW().resumed || 0) >= 2);
  const said = [];
  document.addEventListener("quillo:hmr", (e) => said.push(JSON.parse(e.detail).what));
  await fetch("/stage/1");
  const swapped = await wait(() => said.length > 0);
  const drawn = await wait(() => rows.rows.length === 1 && rows.rows[0].cells.length === 2 && pic.querySelector("g.hard"));
  document.body.dataset.result = JSON.stringify({ up, swapped, said, drawn,
    cells: Array.from(rows.querySelectorAll("td"), (c) => c.textContent),
    wrappers: rows.querySelectorAll("quillo-island").length + pic.querySelectorAll("quillo-island").length,
    g: !!pic.querySelector(":scope > g.hard[data-qi] > circle.hard"), errors: window.__quilloErrors });
})();"##;
    let t = s.dir.0.join("t.js");
    std::fs::write(&t, script).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let mut c = Command::new("node")
        .env_remove("QUILLO_PAGE_FOCUS")
        .env_remove("QUILLO_PAGE_CDP")
        .arg(&run)
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/", s.port))
        .arg(s.dir.0.join("profile"))
        .arg("60000")
        .arg(&t)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("node");
    // The page asks for its new code once it is up.
    let end = Instant::now() + Duration::from_secs(60);
    while support::send(s.port, "GET", "/stage", &[], "").body != "1" {
        assert!(Instant::now() < end, "the page did not get ready:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(s.dir.0.join("main.qwe"), SWAP2).unwrap();
    let mut so = c.stdout.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = std::io::Read::read_to_end(&mut so, &mut v);
        v
    });
    assert!(wait_for(&mut c, Duration::from_secs(120)).is_some_and(|x| x.success()), "{}", s.log());
    let out = out.join().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out)));
    assert_eq!(v["up"], true, "{}\n{}", v, s.log());
    assert_eq!(v["said"], serde_json::json!(["swapped"]), "{}\n{}", v, s.log());
    assert_eq!(v["drawn"], true, "{}\n{}", v, s.log());
    assert_eq!(v["cells"], serde_json::json!(["1", "hard"]), "{}", v);
    assert_eq!(v["wrappers"], 0, "{}", v);
    assert_eq!(v["g"], true, "{}", v);
    let errors = v["errors"].as_array().cloned().unwrap_or_default();
    assert_eq!(errors.len(), 2, "{}", v);
    assert!(errors.iter().all(|e| e.as_str().unwrap_or("").contains("must be str, got null null")), "{}", v);
}

// ---- the second review of slice C: one rule for what a failed draw takes, one for what it waits on

/// A branch that failed for a value that is not null (an int, a map without the field, a server
/// fn's error value) is made again when the state changes, as one given null is. Before the fix it
/// waited only on states that were null, and stayed empty.
const PLACE: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
fn blank() -> any = {}
server fn load_err() -> map? {
    return err("nope")
}
component P() {
    state t = nothing()
    state u = blank()
    state v = load_err()
    state show = true
    <div>
        <button #bad onclick={() => { t = { level: 5 } }}>bad</button>
        <button #good onclick={() => { t = { level: "easy" } }}>good</button>
        <button #ugood onclick={() => { u = { level: "fine" } }}>ugood</button>
        <button #vgood onclick={() => { v = { level: "back" } }}>vgood</button>
        <div #panel>{show ? <Level level={t.level}/> : <i>closed</i>}</div>
        <div #upanel>{show ? <Level level={u.level}/> : <i>closed</i>}</div>
        <div #vpanel>{show ? <Level level={v.level}/> : <i>closed</i>}</div>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_branch_that_failed_for_a_value_not_null_is_made_again() {
    let Some((v, _)) = in_chrome(
        "place",
        PLACE,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.getElementById("panel") && w.__quilloErrors.length >= 4);
  d.getElementById("bad").click();
  await wait(() => w.__quilloErrors.length >= 5);
  d.getElementById("good").click();
  const t = await wait(() => d.querySelector("#panel .lvl") && d.querySelector("#panel .lvl").textContent === "easy");
  d.getElementById("ugood").click();
  const u = await wait(() => d.querySelector("#upanel .lvl") && d.querySelector("#upanel .lvl").textContent === "fine");
  d.getElementById("vgood").click();
  const vv = await wait(() => d.querySelector("#vpanel .lvl") && d.querySelector("#vpanel .lvl").textContent === "back");
  return { t, u, v: vv };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["t"], &v["u"], &v["v"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(true)), "{}", v);
    // Three nulls (`v` loading too), then `v`'s error value, then the int.
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL, NULL_LEVEL, NULL_LEVEL, "error has no field `level`", "argument `level` of `Level` must be str, got int 5"]), "{}", v);
}

/// A row of a list whose markup fails (a typed view given a field of a state still loading) shows
/// nothing and is made again with the answer; its list stands, and takes an item added meanwhile.
/// Before the fix the list was left empty for good, with no marker, and a change of it failed.
const LIST: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
server fn load_extra() -> map? {
    sleep(600)
    return { level: "hard" }
}
component L() {
    state extra = load_extra()
    state items = ["a", "b"]
    <div>
        <button #more onclick={() => { items = items + ["c"] }}>more</button>
        <ul #list>{for r in items => <li>{r}<Level level={extra.level}/></li>}</ul>
        <p #bare>{for r in items => <Level level={extra.level}/>}</p>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <L/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_row_whose_markup_failed_is_made_again_and_its_list_stands() {
    let Some((v, _)) = in_chrome(
        "list",
        LIST,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.getElementById("list") && w.__quilloErrors.length >= 2);
  d.getElementById("more").click();
  await tick(100);
  const during = { rows: d.querySelectorAll("#list li").length, bare: d.querySelectorAll("#bare .lvl").length };
  const drawn = await wait(() => d.querySelectorAll("#list .lvl").length === 3 && d.querySelectorAll("#bare .lvl").length === 3);
  return { during, drawn, list: Array.from(d.querySelectorAll("#list li"), (x) => x.textContent), bare: Array.from(d.querySelectorAll("#bare .lvl"), (x) => x.textContent) };
"##,
    ) else {
        return;
    };
    assert_eq!(v["during"], serde_json::json!({ "rows": 0, "bare": 0 }), "{}", v);
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["list"], serde_json::json!(["ahard", "bhard", "chard"]), "{}", v);
    assert_eq!(v["bare"], serde_json::json!(["hard", "hard", "hard"]), "{}", v);
    // Each row's failure said once (the two of each list, and the row added meanwhile).
    let errors = v["errors"].as_array().cloned().unwrap_or_default();
    assert_eq!(errors.len(), 6, "{}", v);
    assert!(errors.iter().all(|e| e == NULL_LEVEL), "{}", v);
}

/// Two views of one component given two states that load one after the other fail with the same
/// words, in two places: each is said. Before the fix the second was taken for the first, lasting.
const TWO: &str = r##"
view Level(level: str) = <span class="lvl">{level}</span>
server fn slow(ms: int, l: str) -> map? {
    sleep(ms)
    return { level: l }
}
component Two() {
    state a = slow(100, "x")
    state b = slow(1200, "y")
    <p #two>
        <Level level={a.level}/>
        <Level level={b.level}/>
    </p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <Two/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn two_failures_with_the_same_words_in_two_places_are_each_said() {
    let Some((v, _)) = in_chrome(
        "two",
        TWO,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  const drawn = await wait(() => d.querySelectorAll("#two .lvl").length === 2, 4000);
  return { drawn };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL, NULL_LEVEL]), "{}", v);
}

/// A live state whose component fails to draw again and again (a prop its view refuses), and
/// whose arguments change meanwhile: once drawn it follows its row on the server, with one load
/// for each change, its subscription neither lost nor doubled.
const LIVE_CYCLES: &str = r##"db.open(":memory:")
db.exec("create table trails (id integer primary key, name text, level text)")
db.exec("insert into trails (id, name, level) values (5, 'Lantern Ridge', 'hard'), (6, 'Otter Bend', 'easy')")

server fn load_trail(id: int) -> map? {
    sleep(150)
    return db.one("select id, name, level from trails where id = ?", id)
}

route POST /api/rename { json: { name: str } } = db.exec("update trails set name = ? where id = 6", name)

view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null

component LivePage(id: int, tag: any) {
    state trail = live(load_trail, id)
    <main #lp>
        <h1>{trail.name}</h1>
        <Level level={tag}/>
    </main>
}

component Lab() {
    state id = 5
    state tag = nothing()
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #next onclick={() => { id = 6 }}>next</button>
        <button #t5 onclick={() => { tag = 5 }}>t5</button>
        <button #t6 onclick={() => { tag = 6 }}>t6</button>
        <button #tok onclick={() => { tag = "ok" }}>tok</button>
        {on ? <LivePage id={id} tag={tag}/> : null}
    </div>
}

route GET /*rest = <html><body><Lab/></body></html>
"##;

#[test]
fn a_live_state_through_failed_draws_loads_once_for_each_change() {
    let Some((v, log)) = in_chrome(
        "cycles",
        LIVE_CYCLES,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(50);
  d.getElementById("next").click();
  await tick(400);
  d.getElementById("t5").click();
  await tick(100);
  d.getElementById("t6").click();
  await tick(100);
  d.getElementById("tok").click();
  const drawn = await wait(() => d.querySelector("#lp h1") && d.querySelector("#lp h1").textContent === "Otter Bend");
  await tick(800);
  const res = await w.fetch("/api/rename", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name: "Renamed" }) });
  const renamed = await wait(() => d.querySelector("#lp h1").textContent === "Renamed");
  await tick(1200);
  return { drawn, status: res.status, renamed };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["drawn"], &v["renamed"]), (&serde_json::json!(true), &serde_json::json!(true)), "{}\n{}", v, log);
    assert_eq!(calls_in(&log, "load_trail"), 3, "one for id 5, one for id 6, one for the rename:\n{}", log);
}

/// A timer a state's initializer starts after it waits (its server fn's answer) is the
/// component's: a draw that fails after it does not end it. Beside it, the same with no failure.
/// Before the fix the timer was the draw's, and the next failed draw ended it.
const LATE_TIMER: &str = r##"
view Level(level: str) = <span class="lvl">{level}</span>
server fn load_a() -> map? {
    sleep(150)
    return { level: "a" }
}
server fn load_b() -> map? {
    sleep(900)
    return { level: "b" }
}
component T2() {
    state count = 0
    fn boot() -> map? {
        let r = load_a()
        every(100, () => { count = count + 1 })
        return r
    }
    state a = boot()
    state b = load_b()
    <p #t2><b>{count}</b><Level level={a.level}/><Level level={b.level}/></p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <T2/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const LATE_TIMER_CTL: &str = r##"
view Level(level: str) = <span class="lvl">{level}</span>
server fn load_a() -> map? {
    sleep(150)
    return { level: "a" }
}
server fn load_b() -> map? {
    sleep(20)
    return { level: "b" }
}
component T2() {
    state count = 0
    fn boot() -> map? {
        let r = load_a()
        every(100, () => { count = count + 1 })
        return r
    }
    state a = boot()
    state b = load_b()
    <p #t2><b>{count}</b><Level level={a.level}/><Level level={b.level}/></p>
}
component T3() {
    state count = 0
    fn boot() -> map? {
        let r = load_a()
        every(100, () => { count = count + 1 })
        return r
    }
    state a = boot()
    <p #t3><b>{count}</b><i>{a == null ? "" : a.level}</i></p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? [<T2/>, <T3/>] : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_timer_started_after_a_wait_outlives_a_failed_draw() {
    let Some((v, _)) = in_chrome(
        "late",
        LATE_TIMER,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  const drawn = await wait(() => d.querySelectorAll("#t2 .lvl").length === 2, 4000);
  const at = +d.querySelector("#t2 b").textContent;
  await tick(800);
  return { drawn, more: +d.querySelector("#t2 b").textContent - at };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert!(v["more"].as_i64().unwrap_or(0) >= 4, "{}", v);
    let Some((v, _)) = in_chrome(
        "latectl",
        LATE_TIMER_CTL,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  const drawn = await wait(() => d.querySelectorAll("#t2 .lvl").length === 2 && d.querySelector("#t3 i").textContent === "a", 4000);
  const at = +d.querySelector("#t2 b").textContent, at3 = +d.querySelector("#t3 b").textContent;
  await tick(800);
  return { drawn, more: +d.querySelector("#t2 b").textContent - at, more3: +d.querySelector("#t3 b").textContent - at3 };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert!(v["more"].as_i64().unwrap_or(0) >= 4 && v["more3"].as_i64().unwrap_or(0) >= 4, "{}", v);
}

/// A component made in a branch whose markup then failed goes with the failed run: its timer stops.
/// And a component whose take-up fails (the browser's value is one its view refuses, where the
/// server's was not) is said once, not again as it is made anew. Before the fix the component in
/// the failed branch lived on, its timer ticking, and the take-up's failure was said twice.
const GONE: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
fn start() -> int {
    every(50, () => { js("tickOnce") })
    return 0
}
component Ticky() {
    state s = start()
    <i>tick</i>
}
component Odd() {
    let n = js("Number", "5") ?? "x"
    <p #odd><Level level={n}/></p>
}
component Holder() {
    state on = false
    state t = nothing()
    <div>
        <button #hold onclick={() => { on = true }}>hold</button>
        {on ? <span #held><Ticky/><Level level={t.level}/></span> : null}
    </div>
}
route GET /odd = <html><body><Odd/></body></html>
route GET /*rest = <html><body><Holder/></body></html>
"##;

#[test]
fn what_a_failed_run_made_goes_with_it_and_a_take_up_failure_is_said_once() {
    let Some((v, _)) = in_chrome(
        "gone",
        GONE,
        r##"
  const [w, d] = await open("/");
  w.tickOnce = () => { w.ticks = (w.ticks || 0) + 1; };
  d.getElementById("hold").click();
  await wait(() => w.__quilloErrors.length >= 1);
  await tick(300);
  const at = w.ticks || 0;
  await tick(500);
  const r = { held: !!d.getElementById("held"), more: (w.ticks || 0) - at, errors1: w.__quilloErrors.slice() };
  const [w2, d2] = await open("/odd");
  await wait(() => w2.__quilloErrors.length >= 1);
  await tick(300);
  r.odd = w2.__quilloErrors.slice();
  r.remade = w2[Symbol.for("quillo")].remade || 0;
  return r;
"##,
    ) else {
        return;
    };
    assert_eq!(v["held"], false, "{}", v);
    assert_eq!(v["more"], 0, "{}", v);
    assert_eq!(v["errors1"], serde_json::json!([NULL_LEVEL]), "{}", v);
    assert_eq!(v["odd"], serde_json::json!(["argument `level` of `Level` must be str, got int 5"]), "{}", v);
    assert_eq!(v["remade"], 1, "{}", v);
}

// ---- the third review of slice C: a failed draw waits on what it read; a draw holds what it builds

/// A page wrapped in a layout (`<Layout>` placing `{children}`) and in a view: the trail page drawn
/// with the answer. Before the fix the layout's `{children}` waited on the layout's states, which
/// never change, and the page stayed blank.
const LAYOUT: &str = r##"let trails = [{ id: 5, name: "Lantern Ridge", level: "hard" }]
server fn load_trail(id: int) -> map? {
    sleep(300)
    return find(trails, (t) => t.id == id)
}
view Level(level: str) = <span class="lvl">{level}</span>
component Layout(children) {
    state wide = false
    <section #layout><header>Trails</header>{children}</section>
}
view Card(children) = <div class="card">{children}</div>
component TrailPage(id: int) {
    state trail = load_trail(id)
    <main #tp>
        <Layout><h1>{trail.name}</h1><Level level={trail.level}/></Layout>
    </main>
}
component ViewPage(id: int) {
    state trail = load_trail(id)
    <main #vp>
        <Card><h1>{trail.name}</h1><Level level={trail.level}/></Card>
    </main>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? [<TrailPage id={5}/>, <ViewPage id={5}/>] : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_page_in_a_layout_is_drawn_when_its_state_comes() {
    let Some((v, _)) = in_chrome(
        "layout",
        LAYOUT,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  const tp = await wait(() => d.querySelector("#tp .lvl"), 3000);
  const vp = await wait(() => d.querySelector("#vp .lvl"), 1000);
  return { tp, vp, h1: d.querySelector("#tp h1") && d.querySelector("#tp h1").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["tp"], &v["vp"], &v["h1"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!("Lantern Ridge")), "{}", v);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL, NULL_LEVEL]), "{}", v);
}

/// A render prop placed in a row (`<li>{render(x)}</li>`) and given as the row itself: each drawn
/// with the answer. Before the fix neither waited on the state the render prop reads.
const SLOT: &str = r##"let trails = [{ id: 5, name: "Lantern Ridge", level: "hard" }]
server fn load_trail(id: int) -> map? {
    sleep(300)
    return find(trails, (t) => t.id == id)
}
view Level(level: str) = <span class="lvl">{level}</span>
component List(items: list, render) {
    <ul #list>{for x in items => <li>{render(x)}</li>}</ul>
}
component Rows(items: list, render) {
    <ul #rows>{for x in items => render(x)}</ul>
}
component Page(id: int) {
    state trail = load_trail(id)
    <main>
        <List items={[1, 2]} render={(x) => <Level level={trail.level}/>}/>
        <Rows items={[1, 2]} render={(x) => <li><Level level={trail.level}/></li>}/>
    </main>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <Page id={5}/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_render_prop_in_a_row_and_as_the_row_is_drawn_when_its_state_comes() {
    let Some((v, _)) = in_chrome(
        "slot",
        SLOT,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  const a = await wait(() => d.querySelectorAll("#list .lvl").length === 2, 3000);
  const b = await wait(() => d.querySelectorAll("#rows .lvl").length === 2, 1000);
  return { a, b };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["a"], &v["b"]), (&serde_json::json!(true), &serde_json::json!(true)), "{}", v);
    assert_eq!(v["errors"].as_array().map(Vec::len), Some(4), "{}", v);
}

/// A page state set to null, a component and a branch drawn while it is, then set back: both
/// drawn. Before the fix they waited on their own component's states only.
const PAGEST: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pick = { level: "start" }
component P() {
    <p #p><Level level={pick.level}/></p>
}
component Q2() {
    state n = 0
    <p #q>{n >= 0 ? <Level level={pick.level}/> : null}</p>
}
component Box() {
    state on = false
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #on onclick={() => { on = true }}>on</button>
        <button #set onclick={() => { pick = { level: "back" } }}>set</button>
        {on ? [<P/>, <Q2/>] : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_draw_that_failed_on_a_page_state_is_drawn_when_it_is_set() {
    let Some((v, _)) = in_chrome(
        "pagest",
        PAGEST,
        r##"
  const [w, d] = await open("/");
  d.getElementById("clear").click(); await tick(50);
  d.getElementById("on").click(); await tick(100);
  const during = { p: !!d.querySelector("#p .lvl"), q: !!d.querySelector("#q .lvl") };
  d.getElementById("set").click();
  const p = await wait(() => d.querySelector("#p .lvl"), 1500);
  const q = await wait(() => d.querySelector("#q .lvl"), 500);
  return { during, p, q, text: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!(v["during"], serde_json::json!({ "p": false, "q": false }), "{}", v);
    assert_eq!((&v["p"], &v["q"], &v["text"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!("back")), "{}", v);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL, NULL_LEVEL]), "{}", v);
}

/// A draw that keeps failing beside a live state whose argument is made in the draw
/// (`let since = now()`): its loads stay bounded, as the draw is not made again from its code.
/// Before the fix each answer drew it again, which gave the live state a new `since`, which loaded
/// again: some 40 loads a second, for ever.
const LOOP: &str = r##"view Name(n: str) = <b>{n}</b>
server fn load_user() -> map? = null
server fn load_items(since: int) -> list {
    sleep(20)
    return [since]
}
component Feed() {
    state user = load_user()
    let since = now()
    state items = live(load_items, since)
    <div #feed><Name n={user.name}/>{len(items)}</div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <Feed/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_failing_draw_beside_a_live_state_does_not_load_for_ever() {
    let Some((v, _)) = in_chrome(
        "loop",
        LOOP,
        r##"
  const [w, d] = await open("/");
  let calls = 0;
  const real = w.fetch.bind(w);
  w.fetch = (u, init) => { if (String(u).includes("load_items")) calls++; return real(u, init); };
  d.getElementById("on").click();
  const at = [];
  for (let i = 0; i < 3; i++) { await tick(1000); at.push(calls); }
  return { at };
"##,
    ) else {
        return;
    };
    let at: Vec<i64> = v["at"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
    assert!(at[2] <= 3 && at[2] == at[1], "{}", v);
    assert_eq!(v["errors"], serde_json::json!(["argument `n` of `Name` must be str, got null null"]), "{}", v);
}

/// A failing row's records of what it built do not pile up: with the answer, after ten changes of
/// the component's other state while it failed, a change of the list writes each row's text and
/// class once. Before the fix every failed try left its records, and a change wrote 72 times.
const DIRECT: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
server fn load_extra() -> map? {
    sleep(2500)
    return { level: "hard" }
}
component L() {
    state extra = load_extra()
    state q = 0
    state items = ["a", "b"]
    <div>
        <button #bump onclick={() => { q += 1 }}>bump</button>
        <button #change onclick={() => { items = items | map((x) => x + "!") }}>change</button>
        <p #q>{q}</p>
        <ul #list>{for r in items => <li class={r}>{r}<Level level={extra.level}/></li>}</ul>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <L/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_failing_rows_records_do_not_pile_up() {
    let Some((v, _)) = in_chrome(
        "direct",
        DIRECT,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.getElementById("list"));
  for (let i = 0; i < 10; i++) { d.getElementById("bump").click(); await tick(30); }
  const drawn = await wait(() => d.querySelectorAll("#list .lvl").length === 2, 6000);
  // Each write of a row's text or class, on the page or off it.
  let writes = 0;
  const data = Object.getOwnPropertyDescriptor(w.CharacterData.prototype, "data");
  Object.defineProperty(w.CharacterData.prototype, "data", { configurable: true, get() { return data.get.call(this); }, set(x) { writes++; data.set.call(this, x); } });
  for (const m of ["add", "remove", "replace"]) { const f = w.DOMTokenList.prototype[m]; w.DOMTokenList.prototype[m] = function (...a) { writes++; return f.apply(this, a); }; }
  const sa = w.Element.prototype.setAttribute; w.Element.prototype.setAttribute = function (...a) { writes++; return sa.apply(this, a); };
  d.getElementById("change").click();
  await tick(100);
  return { drawn, writes, list: Array.from(d.querySelectorAll("#list li"), (x) => x.className + ":" + x.textContent) };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], true, "{}", v);
    assert_eq!(v["writes"], 6, "{}", v);
    assert_eq!(v["list"], serde_json::json!(["a!:a!hard", "b!:b!hard"]), "{}", v);
}

/// A component whose rows and branch fail as it is taken up (the browser's value is one the view
/// refuses): made again, each failure said once. Before the fix the first row's was said twice.
const ODDROW: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
component Odd() {
    let n = js("Number", "5") ?? "x"
    state items = [1, 2]
    <div #odd><ul>{for r in items => <li>{r}<Level level={n}/></li>}</ul><p #pl>{items[0] == 1 ? <Level level={n}/> : null}</p></div>
}
route GET /*rest = <html><body><Odd/></body></html>
"##;

#[test]
fn a_rows_take_up_failure_is_said_once() {
    let Some((v, _)) = in_chrome(
        "oddrow",
        ODDROW,
        r##"
  const [w, d] = await open("/");
  await wait(() => w.__quilloErrors.length >= 1);
  await tick(400);
  return { remade: w[Symbol.for("quillo")].remade || 0 };
"##,
    ) else {
        return;
    };
    assert_eq!(v["remade"], 1, "{}", v);
    let int5 = "argument `level` of `Level` must be str, got int 5";
    assert_eq!(v["errors"], serde_json::json!([int5, int5, int5]), "two rows and a branch: {}", v);
}

/// A component's own `every`, which fills the state its markup needs, outlives a failed draw: the
/// clock is drawn when it ticks. Before the fix it went with the failed draw, and nothing drew it.
const CLOCK: &str = r##"view Clock(t: int) = <span class="clk">{t}</span>
fn nothing() -> any = null
component C() {
    state now = nothing()
    every(100, () => { now = 5 })
    <p #c><Clock t={now}/></p>
}
component D() {
    state now = nothing()
    every(100, () => { now = 7 })
    <p #d>{now == null ? "wait" : <Clock t={now}/>}{<Clock t={now}/>}</p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? [<C/>, <D/>] : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_components_own_timer_outlives_a_failed_draw_and_draws_it() {
    let Some((v, _)) = in_chrome(
        "clock",
        CLOCK,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  const c = await wait(() => d.querySelector("#c .clk"), 1500);
  const dd = await wait(() => d.querySelectorAll("#d .clk").length === 2, 1500);
  return { c, d: dd, text: d.querySelector("#c .clk") && d.querySelector("#c .clk").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["c"], &v["d"], &v["text"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!("5")), "{}", v);
    assert_eq!(v["errors"], serde_json::json!(["argument `t` of `Clock` must be int, got null null", "argument `t` of `Clock` must be int, got null null"]), "{}", v);
}

/// A live state of a component whose own code fails (`upper` of the loading field): once drawn it
/// follows its argument (through a `let`) to a new id; and beside one whose code keeps failing,
/// whose live argument is made in the draw (`now()`), its loads stay bounded, as a failed draw
/// keeps what the live state followed. Before the fix that one loaded some 40 times a second.
const LIVECODE: &str = r##"db.open(":memory:")
db.exec("create table trails (id integer primary key, name text, level text)")
db.exec("insert into trails (id, name, level) values (5, 'Lantern Ridge', 'hard'), (6, 'Otter Bend', 'easy')")

server fn load_trail(id: int) -> map? = db.one("select id, name, level from trails where id = ?", id)
server fn load_user() -> map? = null
server fn load_items(since: int) -> list {
    sleep(20)
    return [since]
}

component LiveCode(id: int) {
    let key = id * 1
    state trail = live(load_trail, key)
    let up = upper(trail.level)
    <main #lc><h1>{trail.name}</h1><b>{up}</b></main>
}

component Feed() {
    state user = load_user()
    let since = now()
    state items = live(load_items, since)
    let n = upper(user.name)
    <div #feed>{n}{len(items)}</div>
}

component Lab() {
    state id = 5
    state feed = false
    <div>
        <button #next onclick={() => { id = 6 }}>next</button>
        <button #feed-on onclick={() => { feed = true }}>feed</button>
        <LiveCode id={id}/>
        {feed ? <Feed/> : null}
    </div>
}

component App() {
    let path = url().path
    <div id="page">{path == "/" ? <a #go href="/l">go</a> : <Lab/>}</div>
}

route GET /*rest = <html><body><App/></body></html>
"##;

#[test]
fn a_live_state_of_failing_code_follows_its_arguments_once_drawn_and_loads_boundedly() {
    let Some((v, _)) = in_chrome(
        "livecode",
        LIVECODE,
        r##"
  const [w, d] = await open("/");
  let calls = 0;
  const real = w.fetch.bind(w);
  w.fetch = (u, init) => { if (String(u).includes("load_items")) calls++; return real(u, init); };
  d.getElementById("go").click();
  const drawn = await wait(() => d.querySelector("#lc b") && d.querySelector("#lc b").textContent === "HARD");
  d.getElementById("next").click();
  const next = await wait(() => d.querySelector("#lc h1").textContent === "Otter Bend" && d.querySelector("#lc b").textContent === "EASY");
  d.getElementById("feed-on").click();
  const at = [];
  for (let i = 0; i < 2; i++) { await tick(1000); at.push(calls); }
  return { drawn, next, at };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["drawn"], &v["next"]), (&serde_json::json!(true), &serde_json::json!(true)), "{}", v);
    let at: Vec<i64> = v["at"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
    assert!(at[1] <= 3 && at[1] == at[0], "{}", v);
    let up = "`upper` expects a str as argument 1, got null";
    assert_eq!(v["errors"], serde_json::json!([up, up]), "{}", v);
}

// ---- the fourth review of slice C: a waiter probes before it draws; a memo that threw tells its
// readers; a failure while the page updates is that effect's (Q-590)

/// A page whose view fails while a state of its ticks, with a component ahead of the failure that
/// loads its own state: that component is made once for the failed draw and once for the draw that
/// succeeds (`#on5`), or once in all where it never succeeds (`#on9`). Before the fix each tick made
/// it again, and it loaded its note 14 to 32 times in 1.5 s.
const SIB: &str = r##"let trails = [{ id: 5, name: "Lantern Ridge", level: "hard" }]
server fn load_trail(id: int) -> map? {
    sleep(300)
    return find(trails, (t) => t.id == id)
}
server fn load_note(id: int) -> str = "note " + str(id)
view Level(level: str) = <span class="lvl">{level}</span>
component Note(id: int) {
    state n = load_note(id)
    <i class="note">{n}</i>
}
component Page(id: int) {
    state trail = load_trail(id)
    state tick = 0
    every(100, () => { tick += 1 })
    <main #pg><Note id={id}/><Level level={trail.level}/><b>{tick}</b></main>
}
component Box() {
    state on = false
    state id = 5
    <div>
        <button #on5 onclick={() => { id = 5; on = true }}>on5</button>
        <button #on9 onclick={() => { id = 9; on = true }}>on9</button>
        {on ? <Page id={id}/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_failed_draw_waits_by_a_probe_and_makes_no_component_again_until_it_passes() {
    let Some((v, log)) = in_chrome(
        "sib5",
        SIB,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on5").click();
  const drawn = await wait(() => d.querySelector("#pg .lvl"), 3000);
  await tick(500);
  return { drawn, notes: d.querySelectorAll(".note").length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["drawn"], &v["notes"]), (&serde_json::json!(true), &serde_json::json!(1)), "{}", v);
    assert_eq!(calls_in(&log, "load_note"), 2, "{}", log);
    let Some((v, log)) = in_chrome(
        "sib9",
        SIB,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on9").click();
  await tick(1500);
  return { drawn: !!d.querySelector("#pg .lvl") };
"##,
    ) else {
        return;
    };
    assert_eq!(v["drawn"], false, "{}", v);
    assert_eq!(calls_in(&log, "load_note"), 1, "{}", log);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
}

/// The same for a branch that fails after making a component, and for rows that do: each made once.
const SIB2: &str = r##"server fn load_trail(id: int) -> map? = null
server fn load_note(id: int) -> str = "note " + str(id)
view Level(level: str) = <span class="lvl">{level}</span>
component Note(id: int) {
    state n = load_note(id)
    <i class="note">{n}</i>
}
component Place(id: int) {
    state trail = load_trail(id)
    state tick = 0
    state show = true
    every(100, () => { tick += 1 })
    <main><b>{tick}</b>{show ? [<Note id={1}/>, <Level level={trail.level}/>] : null}</main>
}
component Rows(id: int) {
    state trail = load_trail(id)
    state tick = 0
    every(100, () => { tick += 1 })
    <main><b>{tick}</b><ul>{for x in [1, 2] => <li><Note id={2}/><Level level={trail.level}/></li>}</ul></main>
}
component Box() {
    state which = 0
    <div>
        <button #place onclick={() => { which = 1 }}>place</button>
        <button #rows onclick={() => { which = 2 }}>rows</button>
        {which == 1 ? <Place id={9}/> : which == 2 ? <Rows id={9}/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_failed_branch_or_row_makes_no_component_again_while_it_fails() {
    for (button, notes, errors) in [("place", 1, 1), ("rows", 2, 2)] {
        let Some((v, log)) = in_chrome(
            "sib2",
            SIB2,
            &format!(
                r##"
  const [w, d] = await open("/");
  d.getElementById("{}").click();
  await tick(1500);
  return {{}};
"##,
                button
            ),
        ) else {
            return;
        };
        assert_eq!(calls_in(&log, "load_note"), notes, "{}: {}", button, log);
        assert_eq!(v["errors"].as_array().map(Vec::len), Some(errors), "{}: {}", button, v);
    }
}

/// A component whose own code fails (`upper` of a state an `every` of its code fills): the timer
/// goes on, fills the state, and the component is drawn, with one timer, not two. Before the fix
/// the failure cleared the timer, and nothing ever drew it.
const CODETIMER: &str = r##"fn nothing() -> any = null
component C() {
    state lvl = nothing()
    every(100, () => { lvl = "hard" })
    let loud = upper(lvl)
    <p #c>{loud}</p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <C/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_timer_of_code_that_failed_goes_on_and_draws_it() {
    let Some((v, _)) = in_chrome(
        "codetimer",
        CODETIMER,
        r##"
  const [w, d] = await open("/");
  const si = w.setInterval, ci = w.clearInterval, mine = new Set();
  w.setInterval = (f, ms, ...a) => { const id = si(f, ms, ...a); mine.add(id); return id; };
  w.clearInterval = (id) => { mine.delete(id); return ci(id); };
  d.getElementById("on").click();
  const c = await wait(() => d.querySelector("#c"), 1500);
  return { c, intervals: mine.size, text: d.getElementById("c") && d.getElementById("c").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["c"], &v["intervals"], &v["text"]), (&serde_json::json!(true), &serde_json::json!(1), &serde_json::json!("HARD")), "{}", v);
    assert_eq!(v["errors"], serde_json::json!(["`upper` expects a str as argument 1, got null"]), "{}", v);
}

/// A body made again whole (it changes its own variable) whose view fails: its `every` goes on
/// and draws it, as a normal body's does, with one timer each. Before the fix the failure cut what
/// the body had started, and nothing drew it.
const WHOLE: &str = r##"view Clock(t: int) = <span class="clk">{t}</span>
fn nothing() -> any = null
component C() {
    state now = nothing()
    every(100, () => { now = 5 })
    let label = "a"
    label = label + "!"
    <p #c><Clock t={now}/>{label}</p>
}
component E() {
    state now = nothing()
    every(100, () => { now = 6 })
    <p #e><Clock t={now}/></p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? [<C/>, <E/>] : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_body_made_again_whole_keeps_its_timer_through_a_failed_view() {
    let Some((v, _)) = in_chrome(
        "whole",
        WHOLE,
        r##"
  const [w, d] = await open("/");
  const si = w.setInterval, ci = w.clearInterval, mine = new Set();
  w.setInterval = (f, ms, ...a) => { const id = si(f, ms, ...a); mine.add(id); return id; };
  w.clearInterval = (id) => { mine.delete(id); return ci(id); };
  d.getElementById("on").click();
  const c = await wait(() => d.querySelector("#c .clk"), 1500);
  const e = await wait(() => d.querySelector("#e .clk"), 500);
  return { c, e, intervals: mine.size, html: d.getElementById("c") && d.getElementById("c").innerHTML };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["c"], &v["e"], &v["intervals"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(2)), "{}", v);
    assert_eq!(v["html"], "<span class=\"clk\">5</span>a!", "{}", v);
}

/// A live state whose component's code fails, then whose view fails: once drawn it follows a new id,
/// as the code that succeeded gave it. Before the fix it was pointed only after a whole draw.
const REPOINT: &str = r##"db.open(":memory:")
db.exec("create table trails (id integer primary key, name text, level text)")
db.exec("insert into trails (id, name, level) values (5, 'Lantern Ridge', 'hard'), (6, 'Otter Bend', 'easy')")
server fn load_trail(id: int) -> map? = db.one("select id, name, level from trails where id = ?", id)
server fn load_extra() -> map? {
    sleep(400)
    return { tag: "x" }
}
view Tag(tag: str) = <i class="tag">{tag}</i>
component LC(id: int) {
    let key = id * 1
    state trail = live(load_trail, key)
    state extra = load_extra()
    let up = upper(trail.level)
    <main #lc><h1>{trail.name}</h1><b>{up}</b><Tag tag={extra.tag}/></main>
}
component Lab() {
    state id = 5
    <div>
        <button #next onclick={() => { id = 6 }}>next</button>
        <LC id={id}/>
    </div>
}
component App() {
    let path = url().path
    <div id="page">{path == "/" ? <a #go href="/l">go</a> : <Lab/>}</div>
}
route GET /*rest = <html><body><App/></body></html>
"##;

#[test]
fn a_live_state_follows_once_its_code_succeeds_though_its_view_then_fails() {
    let Some((v, _)) = in_chrome(
        "repoint",
        REPOINT,
        r##"
  const [w, d] = await open("/");
  d.getElementById("go").click();
  const drawn = await wait(() => d.querySelector("#lc .tag") && d.querySelector("#lc b").textContent === "HARD", 3000);
  await tick(100);
  d.getElementById("next").click();
  const next = await wait(() => d.querySelector("#lc h1").textContent === "Otter Bend" && d.querySelector("#lc b").textContent === "EASY", 2000);
  return { drawn, next };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["drawn"], &v["next"]), (&serde_json::json!(true), &serde_json::json!(true)), "{}", v);
}

/// A branch given a `let` that throws (`upper` of a page state set to null) is drawn when the page
/// state is set back. Before the fix the `let`, left waiting to run again, told none of its readers.
const MEMO: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pick = { level: "start" }
component P() {
    state show = false
    let lvl = upper(pick.level)
    <div #p><button #show onclick={() => { show = true }}>show</button>{show ? <Level level={lvl}/> : null}</div>
}
component Q() {
    let lvl = upper(pick.level)
    <div #q>{lvl}</div>
}
component Box() {
    state on = false
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #set onclick={() => { pick = { level: "back" } }}>set</button>
        <P/>
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_branch_given_a_let_that_threw_is_drawn_when_the_let_can_be_worked_out() {
    let Some((v, _)) = in_chrome(
        "memo",
        MEMO,
        r##"
  const [w, d] = await open("/");
  d.getElementById("clear").click(); await tick(50);
  d.getElementById("show").click(); await tick(100);
  const during = !!d.querySelector("#p .lvl");
  d.getElementById("set").click();
  const p = await wait(() => d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent === "BACK", 1500);
  return { during, p };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["during"], &v["p"]), (&serde_json::json!(false), &serde_json::json!(true)), "{}", v);
    assert_eq!(v["errors"], serde_json::json!(["`upper` expects a str as argument 1, got null"]), "{}", v);
}

/// A `let` that starts throwing while markup reads it, and an expression in a view's argument that
/// does: the part shows nothing, its failure said, the rest of the page goes on updating (`n`), and
/// both follow the page state once it is set. Before the fix the failure went out of the page's
/// updates, uncaught, and no update ever ran again (FEEDBACK49.md, Q-590).
const MEMO2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pick = { level: "start" }
component P() {
    state show = false
    <div #p><button #show onclick={() => { show = true }}>show</button>{show ? <Level level={upper(pick.level)}/> : null}</div>
}
component Q() {
    state n = 0
    let lvl = upper(pick.level)
    <div #q><button #n onclick={() => { n += 1 }}>n</button><i>{n}</i><Level level={lvl}/></div>
}
component Box() {
    state on = false
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #set onclick={() => { pick = { level: "back" } }}>set</button>
        <P/>
        <Q/>
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_let_that_starts_throwing_while_shown_stops_nothing_else() {
    let Some((v, _)) = in_chrome(
        "memo2",
        MEMO2,
        r##"
  const [w, d] = await open("/");
  const unc = []; w.addEventListener("error", (e) => unc.push(String(e.message)));
  d.getElementById("clear").click(); await tick(50);
  d.getElementById("show").click(); await tick(100);
  const during = { p: !!d.querySelector("#p .lvl"), q: d.querySelector("#q .lvl").textContent };
  d.getElementById("n").click(); await tick(50);
  const n = d.querySelector("#q i").textContent;
  d.getElementById("set").click();
  const p = await wait(() => d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent === "BACK", 1500);
  const q = await wait(() => d.querySelector("#q .lvl").textContent === "BACK", 500);
  return { during, n, p, q, unc };
"##,
    ) else {
        return;
    };
    assert_eq!(v["during"], serde_json::json!({ "p": false, "q": "" }), "{}", v);
    assert_eq!((&v["n"], &v["p"], &v["q"]), (&serde_json::json!("1"), &serde_json::json!(true), &serde_json::json!(true)), "{}", v);
    assert_eq!(v["unc"], serde_json::json!([]), "{}", v);
    let up = "`upper` expects a str as argument 1, got null";
    assert_eq!(v["errors"], serde_json::json!([up, up]), "{}", v);
}

/// FEEDBACK49.md's own case: a derived value that divides by zero after a click; the other
/// component goes on counting clicks, and the failure is said (FEEDBACK49.md, Q-590).
const Q590: &str = r##"
component Broken(n: int) {
    state zero = 0
    state go = false
    let x = go ? 1 / zero : 1
    <div><button id="break" @click={() => { go = true }}>break {x}</button></div>
}
component Fine() {
    state n = 0
    <div><button id="ok" @click={() => { n += 1 }}>ok {n}</button></div>
}
route GET / = <html><body><Broken n={1}/><Fine/></body></html>
"##;

#[test]
fn a_derived_value_that_throws_as_the_page_updates_stops_no_other_component() {
    let Some((v, _)) = in_chrome(
        "q590",
        Q590,
        r##"
  const [w, d] = await open("/");
  const unc = []; w.addEventListener("error", (e) => unc.push(String(e.message)));
  d.getElementById("break").click(); await tick(50);
  d.getElementById("ok").click();
  const ok = await wait(() => d.getElementById("ok").textContent === "ok 1", 1500);
  return { ok, unc };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["ok"], &v["unc"]), (&serde_json::json!(true), &serde_json::json!([])), "{}", v);
    assert_eq!(v["errors"].as_array().map(Vec::len), Some(1), "{}", v);
    assert!(v["errors"][0].as_str().unwrap_or("").contains("division by zero"), "{}", v);
}

/// A view's argument that reads and writes a page state, failing: the probe's write wakes no
/// waiter of its own, so the page state is written a few times, not a thousand. Before the fix the
/// page went round until the loop guard stopped it.
const LOOPW: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state hits = 0
fn seen(x: any) -> any {
    hits = hits + 1
    return x
}
component Box() {
    state t = null
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <i #hits>{hits}</i>
        {on ? <p #p><Level level={seen(t)}/></p> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_that_writes_a_state_it_reads_does_not_go_round() {
    let Some((v, _)) = in_chrome(
        "loopw",
        LOOPW,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(500);
  return { hits: +d.getElementById("hits").textContent };
"##,
    ) else {
        return;
    };
    assert!(v["hits"].as_i64().unwrap_or(9999) <= 5, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([NULL_LEVEL]), "{}", v);
}

/// Two mounted programs with a server fn of the same name that redirects: each says so once. Before
/// the fix they shared one set of what was said, and the second said nothing.
#[test]
fn each_mounted_program_says_its_redirect_once() {
    let fnsrc = "server fn go() {\n    return redirect(\"/p\")\n}\nroute GET /p = \"p\"\n";
    let src = format!("mount /b = \"b.qwe\"\n{}", fnsrc);
    let s = served_files("mounts", &src, &[("b.qwe", fnsrc)], &[]);
    let want = format!("{{\"err\":\"{}\"}}", NO_REDIRECT);
    for path in ["/_quillo/rpc/go", "/b/_quillo/rpc/go", "/_quillo/rpc/go", "/b/_quillo/rpc/go"] {
        let a = support::send(s.port, "POST", path, &[("Content-Type", "application/json")], "[]");
        assert_eq!(a.body, want, "{}\n{}", path, s.log());
    }
    assert_eq!(s.log().matches("server fn `go` redirects to").count(), 2, "{}", s.log());
}

/// Under --watch, `v1` served, `script` run in its page (it asks for `/stage/1` once it is ready
/// for the save), `v2` saved then: what the script records, and the log.
fn swap_page(name: &str, v1: &str, v2: &str, script: &str) -> Option<(serde_json::Value, String)> {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return None;
    };
    let _turn = chrome_turn();
    let s = served_with(name, v1, &["--watch"]);
    let t = s.dir.0.join("t.js");
    std::fs::write(&t, script).unwrap();
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js");
    let mut c = Command::new("node")
        .env_remove("QUILLO_PAGE_FOCUS")
        .env_remove("QUILLO_PAGE_CDP")
        .arg(&run)
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/", s.port))
        .arg(s.dir.0.join("profile"))
        .arg("60000")
        .arg(&t)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("node");
    let end = Instant::now() + Duration::from_secs(60);
    while support::send(s.port, "GET", "/stage", &[], "").body != "1" {
        assert!(Instant::now() < end, "the page did not get ready:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(30));
    }
    std::fs::write(s.dir.0.join("main.qwe"), v2).unwrap();
    let mut so = c.stdout.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = std::io::Read::read_to_end(&mut so, &mut v);
        v
    });
    assert!(wait_for(&mut c, Duration::from_secs(120)).is_some_and(|x| x.success()), "{}", s.log());
    let out = out.join().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out)));
    Some((v, s.log()))
}

/// A component swapped under --watch for code whose view fails (a new state loading), with a
/// component inside it whose state the swap keeps (a counter at 3): drawn once the answer comes,
/// the counter still at 3. Before the fix the failed draw used the kept state up, and the counter
/// drawn again started at 0.
const KEPT1: &str = r##"let stage = 0
route GET /stage/:n(int) {
    stage = n
    return "ok"
}
route GET /stage = stage
server fn load_level() -> map? {
    return { level: "hard" }
}
view Level(level: str) = <b class="lvl">{level}</b>
component Inner() {
    state n = 0
    <button #inner onclick={() => { n += 1 }}>{n}</button>
}
component Outer() {
    state k = 1
    <div #outer><Inner/></div>
}
route GET / = <html><body><Outer/></body></html>
"##;
const KEPT2: &str = r##"let stage = 0
route GET /stage/:n(int) {
    stage = n
    return "ok"
}
route GET /stage = stage
server fn load_level() -> map? {
    return { level: "hard" }
}
view Level(level: str) = <b class="lvl">{level}</b>
component Inner() {
    state n = 0
    <button #inner onclick={() => { n += 1 }}>{n}</button>
}
component Outer() {
    state k = 1
    state extra = load_level()
    <div #outer><Inner/><Level level={extra.level}/></div>
}
route GET / = <html><body><Outer/></body></html>
"##;

#[test]
fn a_swap_whose_draw_fails_keeps_the_states_of_the_components_inside() {
    let Some((v, log)) = swap_page(
        "kept",
        KEPT1,
        KEPT2,
        r##"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const wait = async (f, ms = 15000) => { const end = Date.now() + ms; while (Date.now() < end) { try { if (f()) return true; } catch (_) {} await tick(25); } return false; };
  const QW = () => window[Symbol.for("quillo")] || {};
  document.getElementById("inner").dispatchEvent(new Event("focusin", { bubbles: true }));
  const up = await wait(() => QW().booted && (QW().resumed || 0) >= 1);
  for (let i = 0; i < 3; i++) { document.getElementById("inner").click(); await tick(30); }
  const before = document.getElementById("inner").textContent;
  const said = [];
  document.addEventListener("quillo:hmr", (e) => said.push(JSON.parse(e.detail).what));
  await fetch("/stage/1");
  const drawn = await wait(() => document.querySelector("#outer .lvl"));
  document.body.dataset.result = JSON.stringify({ up, before, said, drawn, inner: document.getElementById("inner") && document.getElementById("inner").textContent, errors: window.__quilloErrors });
})();"##,
    ) else {
        return;
    };
    assert_eq!((&v["up"], &v["before"], &v["drawn"]), (&serde_json::json!(true), &serde_json::json!("3"), &serde_json::json!(true)), "{}\n{}", v, log);
    assert_eq!(v["said"], serde_json::json!(["swapped"]), "{}\n{}", v, log);
    assert_eq!(v["inner"], "3", "{}", v);
}

/// A probe writes no `src` and no raw HTML on the nodes it throws away: with a state of the
/// component ticking while its answer loads, an image's `src` is written by the draw that failed
/// and by the one that succeeds, and `raw()`'s HTML likewise, not by each probe between.
const PIC: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
server fn load_trail(id: int) -> map? {
    sleep(400)
    return { level: "hard" }
}
component Pic(id: int) {
    state trail = load_trail(id)
    state tick = 0
    every(50, () => { tick += 1 })
    let url = "/img/" + str(id) + ".png"
    <figure #fig><img #img src={url}/>{raw("<b class=\"raw\">r</b>")}<i>{tick}</i><Level level={trail.level}/></figure>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <Pic id={5}/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_writes_no_src_and_no_raw_html() {
    let Some((v, _)) = in_chrome(
        "pic",
        PIC,
        r##"
  const [w, d] = await open("/");
  let src = 0, raw = 0;
  const sa = w.Element.prototype.setAttribute;
  w.Element.prototype.setAttribute = function (k, x) { if (k === "src") src++; return sa.call(this, k, x); };
  const ih = Object.getOwnPropertyDescriptor(w.Element.prototype, "innerHTML");
  Object.defineProperty(w.Element.prototype, "innerHTML", { configurable: true, get() { return ih.get.call(this); }, set(x) { if (this.localName === "quillo-raw") raw++; ih.set.call(this, x); } });
  d.getElementById("on").click();
  const drawn = await wait(() => d.querySelector("#fig .lvl"), 3000);
  await tick(200);
  return { drawn, src, raw, img: d.getElementById("img").getAttribute("src"), b: !!d.querySelector("#fig .raw") };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["drawn"], &v["img"], &v["b"]), (&serde_json::json!(true), &serde_json::json!("/img/5.png"), &serde_json::json!(true)), "{}", v);
    assert_eq!((&v["src"], &v["raw"]), (&serde_json::json!(2), &serde_json::json!(2)), "{}", v);
}

/// Under --watch too, a row's failure as its component is taken up is said once.
#[test]
fn a_rows_take_up_failure_is_said_once_under_watch() {
    let Some((v, _)) = in_chrome_with(
        "oddroww",
        ODDROW,
        r##"
  const [w, d] = await open("/");
  await wait(() => w.__quilloErrors.length >= 1);
  await tick(400);
  return { remade: w[Symbol.for("quillo")].remade || 0 };
"##,
        &["--watch"],
    ) else {
        return;
    };
    assert_eq!(v["remade"], 1, "{}", v);
    assert_eq!(v["errors"].as_array().map(Vec::len), Some(3), "{}", v);
}

// ---- the fifth review of slice C ---------------------------------------------------------------

/// A `let` that throws, then comes back to the value it had (`pick` cleared, then set to the same
/// record), and one that fails again after it worked: what reads it shows the value again, and
/// the second failure is said. Before the fix the first stayed empty, and the second was silent.
const MEMO5: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pick = { level: "start" }
component Q() {
    state n = 0
    let lvl = upper(pick.level)
    <div #q><button #n onclick={() => { n += 1 }}>n</button><b>{lvl}</b><i>{lvl}</i><u>{n}</u></div>
}
component Box() {
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #same onclick={() => { pick = { level: "start" } }}>same</button>
        <button #set onclick={() => { pick = { level: "back" } }}>set</button>
        <Q/>
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_let_that_comes_back_to_its_last_value_is_shown_and_a_second_failure_is_said() {
    let Some((v, _)) = in_chrome(
        "memo5",
        MEMO5,
        r##"
  const [w, d] = await open("/");
  const s = () => d.querySelector("#q").textContent;
  const out = [s()];
  d.getElementById("clear").click(); await tick(80); out.push("clear:" + s() + ":e" + w.__quilloErrors.length);
  d.getElementById("same").click(); await tick(80); out.push("same:" + s() + ":e" + w.__quilloErrors.length);
  d.getElementById("clear").click(); await tick(80); out.push("clear2:" + s() + ":e" + w.__quilloErrors.length);
  d.getElementById("set").click(); await tick(80); out.push("set:" + s() + ":e" + w.__quilloErrors.length);
  return { out };
"##,
    ) else {
        return;
    };
    assert_eq!(v["out"], serde_json::json!(["nSTARTSTART0", "clear:n0:e2", "same:nSTARTSTART0:e2", "clear2:n0:e4", "set:nBACKBACK0:e4"]), "{}", v);
}

/// A probe loads nothing: images by `src`, `srcset`, `<source srcset>`, SVG `<image href>`, an
/// `<object data>`, a stylesheet, and an image the template names, with a state ticking while the
/// draw fails. Before the fix each probe asked for most of them again: 14 or 15 requests in 1.5 s.
const PROBE1: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
route GET /pic = "x"
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { tick += 1 })
    <div #p><b>{tick}</b>{tick >= 0 ? <div><img srcset={"/pic?a=" + str(tick)}/><img src={"/pic?s=" + str(tick)}/><Level level={t}/></div> : null}</div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const PROBE2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
route GET /pic = "x"
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { tick += 1 })
    <div #p><b>{tick}</b>{tick >= 0 ? <div><svg><image href={"/pic?svg=" + str(tick)} width="1" height="1"/></svg><picture><source srcset={"/pic?src=" + str(tick)}/><img alt="" src="/pic?fixed"/></picture><object data={"/pic?obj=" + str(tick)}></object><link rel="stylesheet" href={"/pic?css=" + str(tick)}/><Level level={t}/></div> : null}</div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_loads_nothing() {
    for (name, src) in [("probe1", PROBE1), ("probe2", PROBE2)] {
        let Some((v, log)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(1500);
  return { b: +d.querySelector("#p b").textContent };
"##,
        ) else {
            return;
        };
        assert!(v["b"].as_i64().unwrap_or(0) >= 5, "{}: {}", name, v);
        assert!(log.matches("GET /pic").count() <= 5, "{}: {}", name, log);
    }
}

/// A view's argument that counts its calls in a page state, with another state ticking while it
/// fails: the count stays at the first draw's, no draw made for real at a wake. Changed for the
/// eighth review of slice C (a probe stops at a write and the draw is made for real once a wake: the
/// count went up once a wake), and again for the thirteenth (FEEDBACK88.md, Q-1380), which decided
/// that a skipped write makes the draw real only when the probe reads the state back after it, or
/// something other than a place that shows it reads it: the count's own read before its write, and
/// the place that shows it, do not.
const LOOPW3: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state hits = 0
fn seen(x: any) -> any {
    hits = hits + 1
    return x
}
component Box() {
    state t = null
    state on = false
    state k = 0
    every(100, () => { k += 1 })
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <i #hits>{hits}</i><u #k>{k}</u>
        {on ? <p #p><Level level={seen(t)}/></p> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probes_writes_to_state_are_dropped() {
    let Some((v, _)) = in_chrome(
        "loopw3",
        LOOPW3,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(500); const h1 = +d.getElementById("hits").textContent, k1 = +d.getElementById("k").textContent; await tick(2000);
  return { h1, k1, hits: +d.getElementById("hits").textContent, k: +d.getElementById("k").textContent };
"##,
    ) else {
        return;
    };
    let n = |k: &str| v[k].as_i64().unwrap_or(-1);
    assert!(n("k") > n("k1"), "{}", v);
    assert_eq!(n("hits"), n("h1"), "{}", v);
    assert!(n("hits") <= 3, "{}", v);
}

/// A `let` that throws, read by three parts: worked out once in the update, its failure given to
/// the others, and once when it works again. Before the fix it ran once for each part.
const ONCE: &str = r##"fn nothing() -> any = null
fn counted(x: any) -> any {
    js("countIt")
    return x
}
state pick = { level: "a" }
component Q() {
    state n = 0
    let lvl = upper(counted(pick).level)
    <div #q><b>{lvl}</b><i>{lvl}</i><s>{lvl}</s><button #n onclick={() => { n += 1 }}>{n}</button></div>
}
component Box() {
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #set onclick={() => { pick = { level: "b" } }}>set</button>
        <Q/>
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_let_that_throws_runs_once_in_an_update() {
    let Some((v, _)) = in_chrome(
        "once",
        ONCE,
        r##"
  const [w, d] = await open("/");
  w.countIt = () => { w.counted = (w.counted || 0) + 1; };
  w.counted = 0;
  d.getElementById("clear").click(); await tick(100);
  const onClear = w.counted;
  w.counted = 0;
  d.getElementById("set").click(); await tick(100);
  return { onClear, onSet: w.counted, q: d.getElementById("q").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["onClear"], &v["onSet"], &v["q"]), (&serde_json::json!(1), &serde_json::json!(1), &serde_json::json!("BBB0")), "{}", v);
}

// ---- the sixth review of slice C ------------------------------------------------------------

/// A `let` built on a `let` that throws comes back with it. Before the fix it was left waiting to
/// be checked, heard of no change again, and its parts stayed empty.
const MEMO7: &str = r##"fn nothing() -> any = null
state pick = { level: "start" }
component Q() {
    state n = 0
    let lvl = upper(pick.level)
    let shown = lvl + "!"
    <div #q><button #n onclick={() => { n += 1 }}>n</button><b>{shown}</b><i>{shown}</i><u>{n}</u></div>
}
component Box() {
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #set onclick={() => { pick = { level: "back" } }}>set</button>
        <Q/>
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_let_built_on_a_let_that_failed_comes_back() {
    let Some((v, _)) = in_chrome(
        "memo7",
        MEMO7,
        r##"
  const [w, d] = await open("/");
  const s = () => d.querySelector("#q").textContent;
  const out = [s()];
  d.getElementById("clear").click(); await tick(80); out.push("clear:" + s() + ":e" + w.__quilloErrors.length);
  d.getElementById("set").click(); await tick(80); out.push("set:" + s() + ":e" + w.__quilloErrors.length);
  d.getElementById("n").click(); await tick(80); out.push("n:" + s());
  return { out };
"##,
    ) else {
        return;
    };
    assert_eq!(v["out"], serde_json::json!(["nSTART!START!0", "clear:n0:e2", "set:nBACK!BACK!0:e2", "n:nBACK!BACK!1"]), "{}", v);
}

/// A probe that makes a template first: the template keeps its link and image, here and (below)
/// everywhere it is used. Before the fix the probe made it without them, for every page after.
const TPLC: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
route GET /pic = "x"
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { if tick == 3 { t = "hard" }
        tick += 1 })
    <div #p><Level level={t}/>{tick >= 0 ? <a #home href="/home"><img #logo alt="" src="/pic?logo"/></a> : null}</div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_that_makes_a_template_first_leaves_its_addresses_in_it() {
    let Some((v, _)) = in_chrome(
        "tplc",
        TPLC,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.querySelector("#p .lvl"), 4000);
  await tick(300);
  const a = d.getElementById("home"), im = d.getElementById("logo");
  return { lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, a: !!a, href: a && a.getAttribute("href"), src: im && im.getAttribute("src"), outer: d.getElementById("p") && d.getElementById("p").outerHTML };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["href"], &v["src"]), (&serde_json::json!("/home"), &serde_json::json!("/pic?logo")), "{}", v);
}

/// The same template used in a footer drawn later.
const TPLC2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
view Home() = <a class="home" href="/home"><img alt="" src="/pic?logo"/></a>
fn nothing() -> any = null
route GET /pic = "x"
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { if tick == 3 { t = "hard" }
        tick += 1 })
    <div #p><Level level={t}/><Home/></div>
}
component Box() {
    state on = false
    state foot = false
    <div><button #on onclick={() => { on = true }}>on</button><button #foot onclick={() => { foot = true }}>foot</button>{on ? <P/> : null}{foot ? <footer #f><Home/></footer> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_template_a_probe_made_first_keeps_its_addresses_elsewhere() {
    let Some((v, _)) = in_chrome(
        "tplc2",
        TPLC2,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.querySelector("#p .lvl"), 4000);
  await tick(200);
  d.getElementById("foot").click();
  await tick(200);
  const hrefs = [...d.querySelectorAll("a.home")].map((a) => a.getAttribute("href") + "|" + a.querySelector("img").getAttribute("src"));
  return { lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, hrefs };
"##,
    ) else {
        return;
    };
    assert_eq!(v["hrefs"], serde_json::json!(["/home|/pic?logo", "/home|/pic?logo"]), "{}", v);
}

/// A `let` whose code writes state, worked out by a probe (writes dropped): not kept, so the real
/// draw works it out again and its writes happen. Before the fix the probe's value was kept, and
/// the write never happened.
const MEMOW: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state hits = 0
state seen_last = ""
fn mark(x: any) -> any {
    hits = hits + 1
    seen_last = str(x)
    return x
}
fn nothing() -> any = null
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { if tick == 3 { t = "hard" }
        tick += 1 })
    let lvl = mark(t)
    <div #p><Level level={lvl}/></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button><i #hits>{hits}</i><u #last>{seen_last}</u>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_let_worked_out_by_a_probe_is_worked_out_again_with_its_writes() {
    let Some((v, _)) = in_chrome(
        "memow",
        MEMOW,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.querySelector("#p .lvl"), 4000);
  await tick(300);
  return { lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, hits: d.getElementById("hits").textContent, last: d.getElementById("last").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["lvl"], &v["last"]), (&serde_json::json!("hard"), &serde_json::json!("hard")), "{}", v);
}

/// `page_updated()` in markup a probe runs again brings the page's other updates about as they are:
/// a component shown then is made, with its link. Before the fix they ran as the probe's, and the
/// component was not made.
const PU: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
fn settle(x: any) -> any {
    page_updated()
    return x
}
state pulse = 0
state mirror = 0
fn copy(x: int) -> int {
    mirror = x
    return x
}
component Child() {
    <a class="child" href="/home">home</a>
}
component P() {
    state t = nothing()
    <p #p><Level level={settle(pulse >= 0 ? t : t)}/></p>
}
component Late() {
    <div #late>{mirror % 2 == 1 ? <Child/> : <i>even</i>}</div>
}
component Box() {
    state on = false
    state late = false
    <div>
        <b>{copy(pulse)}</b>
        <button #on onclick={() => { on = true }}>on</button>
        <button #late-on onclick={() => { late = true }}>late</button>
        <button #pulse onclick={() => { pulse += 1 }}>pulse</button>
        {on ? <P/> : null}
        {late ? <Late/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn page_updated_in_a_probes_markup_updates_the_page_as_it_is() {
    let Some((v, _)) = in_chrome(
        "pu",
        PU,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(100);
  d.getElementById("late-on").click(); await tick(100);
  d.getElementById("pulse").click(); await tick(150);
  const a = d.querySelector("#late a.child");
  return { child: !!a, href: a && a.getAttribute("href"), late: d.getElementById("late").innerHTML };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["child"], &v["href"]), (&serde_json::json!(true), &serde_json::json!("/home")), "{}", v);
}

// ---- Q-1381: a server fn that redirects ---------------------------------------------------------

const NO_REDIRECT: &str = "a server fn cannot redirect: return a value and navigate";

const HIKES: &str = r##"
server fn log_hike(name: str) {
    redirect("/hikes/7")
}
server fn feed() {
    return redirect("/feed")
}
component Feed(id: str) {
    state f = live(feed)
    <p id={id}>{str(f)}</p>
}
component Form() {
    state r = "not saved"
    state bad = false
    state more = false
    state early = log_hike("on the server")
    <div>
        <button #save onclick={() => { let x = log_hike("Lantern Ridge"); bad = is_err(x); r = str(x) }}>Save</button>
        <button #more onclick={() => { more = true }}>More</button>
        <p #out>{r}</p>
        <p #bad>{str(bad)}</p>
        <p #early>{str(early)} {str(is_err(early))}</p>
        <Feed id="served"/>
        {more ? <Feed id="feed"/> : null}
    </div>
}
route GET /hikes/7 = "hike 7"
route GET /*rest = <html><body><Form/></body></html>
"##;

/// The report's case: the call's value is an error value that says why, the page stays where it
/// is, nothing is followed (no `GET /hikes/7`), and the log says so once for two calls; a live
/// loader that redirects, of a component made in the browser, gives the same; and so do both as
/// the server renders the page, which is taken up as it is (it was a 500: "state `early` of
/// <Form> is a response"). Before the fix: `err("internal error")`, a 303, the GET.
#[test]
fn a_server_fn_that_redirects_gives_the_page_an_error_value_that_says_so() {
    let Some((v, log)) = in_chrome(
        "hike",
        HIKES,
        r##"
  const [w, d] = await open("/");
  d.getElementById("save").click();
  await wait(() => d.getElementById("out").textContent !== "not saved");
  d.getElementById("save").click();
  await tick(300);
  d.getElementById("more").click();
  await wait(() => d.getElementById("feed").textContent.startsWith("err"));
  const QW = w[Symbol.for("quillo")];
  return { path: w.location.pathname, out: d.getElementById("out").textContent, bad: d.getElementById("bad").textContent, feed: d.getElementById("feed").textContent, early: d.getElementById("early").textContent, served: d.getElementById("served").textContent, remade: QW.remade || 0 };
"##,
    ) else {
        return;
    };
    let said = format!("err(\"{}\")", NO_REDIRECT);
    assert_eq!(v["path"], "/");
    assert_eq!(v["out"], said.as_str(), "{}\n{}", v, log);
    assert_eq!(v["bad"], "true");
    assert_eq!(v["feed"], said.as_str(), "{}", v);
    assert_eq!(v["early"], format!("{} true", said).as_str(), "{}", v);
    assert_eq!(v["served"], said.as_str(), "{}", v);
    assert_eq!(v["remade"], 0, "{}", v);
    assert_eq!(v["errors"], serde_json::json!([]), "{}", v);
    assert!(log.contains("GET / -> 200"), "{}", log);
    assert_eq!(log.matches("quillo: state `early` of <Form> is given a redirect to /hikes/7 by the server fn it calls, which a page's call does not follow: the state is given an error value; return a value, and `navigate` in the page").count(), 1, "{}", log);
    assert_eq!(log.matches("state `f` of <Feed> is given a redirect to /feed").count(), 1, "{}", log);
    assert_eq!(calls_in(&log, "log_hike"), 2, "{}", log);
    assert_eq!(log.matches("server fn `log_hike` redirects to /hikes/7, which a page's call does not follow").count(), 1, "{}", log);
    assert!(!log.contains("GET /hikes/7"), "{}", log);
}

/// Each status `redirect` takes (301, 302, 303, 307, 308), returned, left as the last value or
/// given by `reply`, and a live loader's: a 200 with the error value; the statuses either side of
/// them (300, 304) and a `respond(200, ...)` answered as they were; a route's redirect is one; the
/// log says it once for each function, with the address (one with an `ü`, which `redirect`
/// encodes). A state the server renders is given the error value for a redirect, said once, and
/// still fails the page for another response.
#[test]
fn each_redirect_from_a_server_fn_is_an_error_value_and_other_answers_are_as_they_were() {
    let src = r##"
server fn last() {
    redirect("/hikes/7")
}
server fn moved() {
    return redirect("/wanderungen/über", 307)
}
server fn perm(n: int) {
    return redirect("/p", n)
}
server fn replied() {
    reply 303 redirect("/r")
}
server fn choice() {
    return respond(300, "pick one")
}
server fn same() {
    return respond(304, "")
}
server fn fine() {
    return respond(200, "fine")
}
server fn seven() -> int = 7
server fn guard() {
    reply 303 redirect("/login")
}
server fn go(n: int) {
    return redirect(`/p/${n}`)
}
component G() {
    state x = guard()
    <p>{str(x)}</p>
}
component DA() {
    state x = defer(go(1))
    <p>{str(x)}</p>
}
component DB() {
    state x = defer(go(2))
    <p>{str(x)}</p>
}
route GET /g = <html><body><G/></body></html>
route GET /d = <html><body><DA/><DB/></body></html>
route GET /login = "login"
route GET /p/:n(int) = "p"
component Late(n: int) {
    state x = perm(n)
    <p>{str(is_err(x))} {str(x)}</p>
}
component Early() {
    state x = choice()
    <p>{str(x)}</p>
}
route GET /go = redirect("/hikes/7")
route GET /late/:n(int) = <html><body><Late n={n}/></body></html>
route GET /early = <html><body><Early/></body></html>
"##;
    let s = served("statuses", src);
    let want = format!("{{\"err\":\"{}\"}}", NO_REDIRECT);
    for (f, args) in [("last", "[]"), ("moved", "[]"), ("perm", "[301]"), ("perm", "[302]"), ("perm", "[303]"), ("perm", "[307]"), ("perm", "[308]"), ("replied", "[]")] {
        let a = s.call(f, args, &[]);
        assert_eq!((a.status, a.body.as_str(), a.header("location")), (200, want.as_str(), None), "{} {}\n{}", f, args, s.log());
    }
    let a = s.call("last", "[]", &[("x-quillo-live", "1")]);
    assert_eq!((a.status, a.body.as_str()), (200, want.as_str()), "live\n{}", s.log());
    let a = s.call("choice", "[]", &[]);
    assert_eq!((a.status, a.body.as_str()), (300, "pick one"));
    assert_eq!(s.call("same", "[]", &[]).status, 304);
    let a = s.call("fine", "[]", &[]);
    assert_eq!((a.status, a.body.as_str()), (200, "fine"));
    let a = s.call("seven", "[]", &[]);
    assert_eq!((a.status, a.body.as_str()), (200, "{\"ok\":7}"));
    let a = support::send(s.port, "GET", "/go", &[], "");
    assert_eq!((a.status, a.header("location")), (303, Some("/hikes/7")));
    // As the server renders a component, a state given a redirect is the error value, said once;
    // given another answer, the page fails as before.
    for n in [301, 308, 308] {
        let a = support::send(s.port, "GET", &format!("/late/{}", n), &[], "");
        assert_eq!(a.status, 200, "{}\n{}", a.body, s.log());
        assert!(a.body.contains(&format!("<p>true err(&quot;{}&quot;)</p>", NO_REDIRECT)), "{}", a.body);
    }
    assert_eq!(support::send(s.port, "GET", "/early", &[], "").status, 500);
    let log = s.log();
    assert_eq!(log.matches("state `x` of <Late> is given a redirect to /p by the server fn it calls").count(), 1, "{}", log);
    assert!(log.contains("state `x` of <Early> is a response, which cannot be sent to the browser"), "{}", log);
    assert_eq!(log.matches("server fn `perm` redirects to /p,").count(), 1, "{}", log);
    assert_eq!(log.matches("server fn `last` redirects").count(), 1, "{}", log);
    assert!(log.contains("server fn `moved` redirects to /wanderungen/%C3%BCber, which a page's call does not follow: the call is given an error value; return a value, and `navigate` in the page"), "{}", log);
    assert!(log.contains("server fn `replied` redirects to /r,"), "{}", log);
    assert!(!log.contains("`choice` redirects") && !log.contains("`same` redirects") && !log.contains("`fine` redirects"), "{}", log);
    // A `reply` in a server fn a state calls as the server renders the page ends the page's
    // request, as in a route (a guard on a first load); called by a page, it is the error value
    // (the coordinator's decision of round 2).
    let a = support::send(s.port, "GET", "/g", &[], "");
    assert_eq!((a.status, a.header("location")), (303, Some("/login")), "{}", a.body);
    let a = s.call("guard", "[]", &[]);
    assert_eq!((a.status, a.body.as_str()), (200, want.as_str()));
    // Deferred states, each said under its own component (it was `<?>`, and the second unsaid).
    let a = support::send(s.port, "GET", "/d", &[], "");
    assert_eq!(a.status, 200, "{}", a.body);
    assert_eq!(a.body.matches(&format!("err(&quot;{}&quot;)", NO_REDIRECT)).count(), 2, "{}", a.body);
    let log = s.log();
    assert!(log.contains("state `x` of <DA> is given a redirect to /p/1 by") && log.contains("state `x` of <DB> is given a redirect to /p/2 by"), "{}", log);
    assert!(!log.contains("<?>"), "{}", log);
}


/// Under --watch, a redirect said once is said again by the program saved anew. Before the fix the
/// keys of what was said outlived the reload.
#[test]
fn a_redirect_said_once_is_said_again_after_a_reload() {
    let src = "server fn go() {\n    return redirect(\"/p\")\n}\nroute GET /p = \"p\"\n";
    let s = served_with("said", src, &["--watch"]);
    let want = format!("{{\"err\":\"{}\"}}", NO_REDIRECT);
    assert_eq!(s.call("go", "[]", &[]).body, want);
    assert_eq!(s.call("go", "[]", &[]).body, want);
    std::fs::write(s.dir.0.join("main.qwe"), format!("{}// saved again\n", src)).unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    while !s.log().contains("quillo: reloaded") {
        assert!(Instant::now() < end, "no reload:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(s.call("go", "[]", &[]).body, want);
    assert_eq!(s.log().matches("server fn `go` redirects to /p,").count(), 2, "{}", s.log());
}

// ---- the seventh review of slice C ----------------------------------------------------------

/// `let`s that each read the one before twice, read by a draw that fails as its state ticks: each
/// worked out once a probe. Before the fix each was worked out again at each read in a probe, 2^n of
/// them: 393,213 runs of the 17 `let`s here for 3 ticks. Since the tenth review a probe skips `js`
/// calls, so the count is the real draws' (17); the probes' own count is the node core test's
/// (t_exp in the slice's notes).
const EXP: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
fn bump(x: int) -> int {
    js("bump")
    return x
}
component P() {
    state tick = 0
    every(50, () => { if tick < 3 {
        tick += 1
    } })
    state t = nothing()
    let a0 = bump(tick % 2)
    let a1 = bump(a0 + a0)
    let a2 = bump(a1 + a1)
    let a3 = bump(a2 + a2)
    let a4 = bump(a3 + a3)
    let a5 = bump(a4 + a4)
    let a6 = bump(a5 + a5)
    let a7 = bump(a6 + a6)
    let a8 = bump(a7 + a7)
    let a9 = bump(a8 + a8)
    let a10 = bump(a9 + a9)
    let a11 = bump(a10 + a10)
    let a12 = bump(a11 + a11)
    let a13 = bump(a12 + a12)
    let a14 = bump(a13 + a13)
    let a15 = bump(a14 + a14)
    let a16 = bump(a15 + a15)
    <div #p><b>{a16}</b><Level level={t}/></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_works_out_each_let_once() {
    let Some((v, _)) = in_chrome(
        "exp",
        EXP,
        r##"
  const [w, d] = await open("/");
  w.bump = () => { w.__n = (w.__n | 0) + 1; return null; };
  d.getElementById("on").click();
  await tick(1000);
  return { runs: w.__n | 0, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    // 17 at the first draw, and 17 at each of its 3 probes.
    let runs = v["runs"].as_i64().unwrap_or(-1);
    assert!((17..=17 * 8).contains(&runs), "{}", v);
}

/// A `let` that writes a state and reads it back, read by a failed draw's probe and by a component
/// beside it in the same update: the component is given the `let`'s real value. Before the fix it
/// was given the probe's failure (the probe's write dropped), and showed nothing.
const PIN: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state seen = "a"
fn remember(x: str) -> str {
    seen = x
    return seen
}
fn same(a: str, b: str) -> str {
    if a != b {
        return upper(nothing())
    }
    return a
}
fn pick(a: str, b: any) -> any {
    return a == "" ? a : b
}
component Kid(v: str) {
    <i #kid>{v}</i>
}
component P() {
    state t = "a"
    state u = nothing()
    let m = same(remember(t), t)
    <div #p>
        <button #go onclick={() => { t = "b" }}>go</button>
        <span #pl>{u == null ? <Level level={pick(m, u)}/> : null}</span>
        <Kid v={m}/>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probes_failure_is_not_given_to_a_real_reader() {
    let Some((v, _)) = in_chrome(
        "pin",
        PIN,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(200);
  const k0 = d.getElementById("kid") && d.getElementById("kid").textContent;
  d.getElementById("go").click(); await tick(300);
  return { k0, kid: d.getElementById("kid") && d.getElementById("kid").textContent, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["k0"], &v["kid"], &v["errs"]), (&serde_json::json!("a"), &serde_json::json!("b"), &serde_json::json!(1)), "{}", v);
}

/// Markup that writes a state and reads it back (`remember`): its draw comes back once what it
/// needs comes, the probe reading back what it wrote, which is put back as it ends. Before the fix
/// the probe's write was dropped, each probe failed, and the draw waited for good.
const PW2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state seen = nothing()
fn remember(x: any) -> any {
    if x != null {
        seen = x
    }
    return seen
}
fn unused_old(x: any) -> any {
    seen = x
    return seen
}
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { if tick == 3 { t = "hard" }
        tick += 1 })
    <div #p><Level level={remember(t)}/></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_draw_whose_markup_writes_and_reads_back_comes_back() {
    let Some((v, _)) = in_chrome(
        "pw2",
        PW2,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.querySelector("#p .lvl"), 4000);
  return { lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["lvl"], &v["errs"]), (&serde_json::json!("hard"), &serde_json::json!(1)), "{}", v);
}

/// A template a probe makes first loads nothing: its image is not asked for. Before the fix it was
/// made in the page's document, and its image loaded.
const TPLIMG: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
route GET /pic = "x"
component P() {
    state t = nothing()
    state tick = 0
    every(100, () => { tick += 1 })
    <div #p>{tick >= 2 ? <a href="/home"><img alt="" src="/pic?never"/></a> : null}<Level level={t}/></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_template_a_probe_makes_first_loads_nothing() {
    let Some((v, log)) = in_chrome(
        "tplimg",
        TPLIMG,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(800);
  return { p: !!d.getElementById("p"), errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!(v["errs"], serde_json::json!(1), "{}", v);
    assert!(!log.contains("GET /pic?never"), "{}", log);
}

/// A `let` that guards its read of a `let` that fails (`??`) runs, and gives what it guards with.
/// Before the fix the failure was passed on to it: the component showed nothing, and it was said.
const GUARD: &str = r##"fn nothing() -> any = null
state pick = { level: "start" }
component Q() {
    let lvl = upper(pick.level)
    let shown = (lvl + "!") ?? "none"
    <div #q><b>{shown}</b></div>
}
component Box() {
    state on2 = false
    <div>
        <button #clear onclick={() => { pick = nothing() }}>clear</button>
        <button #set onclick={() => { pick = { level: "back" } }}>set</button>
        <button #on2 onclick={() => { on2 = true }}>on2</button>
        <Q/>
        <section #s2>{on2 ? <Q/> : null}</section>
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_let_that_guards_its_read_of_a_failed_let_runs() {
    let Some((v, _)) = in_chrome(
        "guard",
        GUARD,
        r##"
  const [w, d] = await open("/");
  const s = () => d.querySelector("#q").textContent;
  const out = [s()];
  d.getElementById("clear").click(); await tick(150); out.push(s() + ":e" + w.__quilloErrors.length);
  d.getElementById("set").click(); await tick(150); out.push(s());
  return { out };
"##,
    ) else {
        return;
    };
    assert_eq!(v["out"], serde_json::json!(["START!", "none:e0", "BACK!"]), "{}", v);
}

// ---- the eighth review of slice C -----------------------------------------------------------

/// Markup that sets a state on the third tick, its draw failing until a `let` that reads that state
/// gives the view what it needs: the probe stops at the write, and the draw made for real writes it
/// and is drawn. Before the fix the probe's write, made for the probe alone, staled nothing, and the
/// draw waited for good.
const N3: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state pg = null
fn fill(k: int) -> str {
    if k >= 3 {
        pg = "hard"
    }
    return ""
}
component P() {
    state k = 0
    every(100, () => { k += 1 })
    let lv = pg
    <div #p><i>{fill(k)}</i><Level level={lv}/></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_write_read_back_through_a_let_draws() {
    let Some((v, _)) = in_chrome(
        "n3",
        N3,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await wait(() => d.querySelector("#p .lvl"), 4000);
  return { lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!(v["lvl"], serde_json::json!("hard"), "{}", v);
}

/// Markup that sets a page state back as it fails, the state changed meanwhile by a handler: what
/// the `let`s built on that state show follows the state as it is. Before the fix a probe worked
/// them out from its own write, which it then put back, and they showed "a12" with the state "b"
/// for good.
const N4: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = "a"
fn pin() -> str {
    pg = "a"
    return ""
}
component P() {
    state lv = nothing()
    state k = 0
    state show = false
    state on2 = true
    let m1 = pg + "1"
    let c = m1 + "2"
    <div>
        <button #show onclick={() => { show = true }}>s</button>
        <button #go onclick={() => { pg = "b"
            k += 1 }}>go</button>
        {show ? <b #c>{c}</b> : null}<em #pg>{pg}</em>
        <p>{on2 ? <span>{pin()}<i>{c}</i><Level level={lv}/></span> : null}</p>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn lets_follow_a_state_markup_writes_as_it_fails() {
    let Some((v, _)) = in_chrome(
        "n4",
        N4,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(200);
  d.getElementById("show").click(); await tick(200);
  const before = d.getElementById("c").textContent;
  d.getElementById("go").click(); await tick(400);
  return { before, c: d.getElementById("c").textContent, pg: d.getElementById("pg").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!(v["before"], serde_json::json!("a12"), "{}", v);
    assert_eq!(v["c"].as_str().unwrap_or(""), format!("{}12", v["pg"].as_str().unwrap_or("?")), "{}", v);
}

/// Markup that sets a state a server fn loads, on the third tick, as it fails: the answer lands, as
/// on 0.8.11. Before the fix a probe's write took the state's claim from the call and wrote
/// nothing: the answer was dropped, and the state stayed empty. Changed for the thirteenth review of
/// slice C, which decided that a write a probe skips makes the draw real only when the probe reads
/// the state back after it, or something other than a place that shows it reads it: this test said
/// the set ("probe") was made by the draw made for real, at a wake where only the place showing the
/// state read it, which made rl's components load at each wake.
const N2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
server fn slow() -> str {
    sleep(800)
    return "loaded"
}
fn nothing() -> any = null
component P() {
    state seen = slow()
    state lv = nothing()
    state k = 0
    state on2 = true
    every(100, () => { k += 1 })
    let mark = (n: int) => {
        if n == 3 {
            seen = "probe"
        }
        return ""
    }
    <div #p><b #seen>{seen}</b><u>{k}</u>{on2 ? <span>{mark(k)}<Level level={lv}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_write_in_failing_markup_is_made_for_real() {
    let Some((v, _)) = in_chrome(
        "n2",
        N2,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(1800);
  return { seen: d.getElementById("seen") && d.getElementById("seen").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!(v["seen"], serde_json::json!("loaded"), "{}", v);
}

// ---- the tenth review of slice C ------------------------------------------------------------

/// Markup that writes a state and then reads a page state that is null, as a place and as a whole
/// draw: once the page state is set, both draw. Before the fix a probe stopped at the write, never
/// read the page state, and nothing woke the draw: both stayed empty for good.
const WBM: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state marks = 0
state pg = nothing()
fn mark() -> str {
    marks = 1
    return ""
}
component P() {
    state on2 = true
    <div><p #p>{on2 ? <span>{mark()}<Level level={pg}/></span> : null}</p></div>
}
component Q() {
    <div #q>{mark()}<Level level={pg}/></div>
}
component Box() {
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? [<P/>, <Q/>] : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_failure_on_a_page_state_read_after_a_write_recovers() {
    let Some((v, _)) = in_chrome(
        "wbm",
        WBM,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(200);
  const errs = w.__quilloErrors.length;
  d.getElementById("set").click(); await tick(400);
  return { errs, p: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, q: d.querySelector("#q .lvl") && d.querySelector("#q .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["errs"], &v["p"], &v["q"]), (&serde_json::json!(2), &serde_json::json!("hard"), &serde_json::json!("hard")), "{}", v);
}

/// A `js(...)` call in failing markup, whose callback (a frame's) sets the component's state: each
/// change wakes the failed draw, which is made for real, and its call asks for the next frame. Changed
/// for the twelfth review of slice C, which decided that a call of the page's ends a probe and the
/// draw is made for real, its answer not known (a skipped call's null failed a view the real draw
/// passed), and that this loop is the program's own, bounded by frames: the test said the probes
/// made no call, as they then skipped it with null. It now says the count follows the frames, not a
/// loop of its own, that the draw comes back once `t` is set, and that the loop then ends (the place
/// that calls reads nothing that changes).
const JST: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
component P() {
    state n = 0
    state t = nothing()
    state on2 = true
    <div>
        <i #n>{n}</i>
        <button #set onclick={() => { t = "hard" }}>set</button>
        <p #p>{on2 ? <span><b>{js("requestAnimationFrame", () => { n += 1 })}</b><Level level={t}/></span> : null}</p>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_makes_no_js_call() {
    let Some((v, _)) = in_chrome(
        "jst",
        JST,
        r##"
  const [w, d] = await open("/");
  let frames = 0; const count = () => { frames++; w.requestAnimationFrame(count); }; w.requestAnimationFrame(count);
  d.getElementById("on").click(); await tick(1000);
  const n1 = +d.getElementById("n").textContent, f1 = frames;
  d.getElementById("set").click(); await tick(500);
  const n2 = +d.getElementById("n").textContent;
  await tick(300);
  return { n1, f1, n2, n3: +d.getElementById("n").textContent, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    let n = |k: &str| v[k].as_i64().unwrap_or(-1);
    // At most one a frame (and the first draw's), as the program's own loop goes.
    assert!(n("n1") >= 1 && n("n1") <= n("f1") + 2, "{}", v);
    assert_eq!(v["lvl"], serde_json::json!("hard"), "{}", v);
    assert_eq!(n("n2"), n("n3"), "{}", v);
}

/// `navigate` in failing markup, its component's state ticking: the real draw's alone, not each
/// probe's. (A probe's `navigate` changed nothing before the fix either, for its runs are not the
/// latest; it now waits for the real draw, as anything that acts outside the page does.)
const NAV2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
component P() {
    state k = 0
    state t = nothing()
    every(100, () => { k += 1 })
    <p #p><b>{navigate("/n?k=" + str(k))}</b><Level level={t}/></p>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_does_not_navigate() {
    let Some((v, _)) = in_chrome(
        "nav2",
        NAV2,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(1000);
  return { loc: w.location.pathname + w.location.search };
"##,
    ) else {
        return;
    };
    assert_eq!(v["loc"], serde_json::json!("/n?k=0"), "{}", v);
}

/// `page_head`, `js_new` and `listen` in failing markup, its component's state ticking: made by the
/// real draws alone, a probe making none of them (it only reads); the draw once it can be made sets
/// the page's title and draws.
const ACTS: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
component P() {
    state n = 0
    state k = 0
    state t = nothing()
    state on2 = true
    every(100, () => { k += 1 })
    <div>
        <button #set onclick={() => { t = "hard" }}>set</button>
        <p #p>{on2 ? <span>{page_head({ title: "P " + str(k) })}{str(js_new("Date") != null)}{listen("/feed", (m: any) => { n += 1 })}<Level level={t}/></span> : null}</p>
    </div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><head><title>start</title></head><body><Box/></body></html>
"##;

#[test]
fn a_probe_acts_on_nothing_outside_the_page() {
    let Some((v, _)) = in_chrome(
        "acts",
        ACTS,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(700);
  const during = d.title;
  d.getElementById("set").click(); await tick(300);
  return { during, after: d.title, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["during"], &v["lvl"], &v["errs"]), (&serde_json::json!("start"), &serde_json::json!("hard"), &serde_json::json!(1)), "{}", v);
    assert!(v["after"].as_str().unwrap_or("").starts_with("P "), "{}", v);
}

/// A `let` that writes a state, worked out again by a probe (what it reads was changed by the draw's
/// own markup), before markup that reads a page state that is null: once the page state is set, the
/// component draws. Before the fix the probe stopped at the `let`'s write, never read the page
/// state, and the draw waited for good.
const LETW: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state seen = 0
state marks = 0
state pg = nothing()
fn note(v: int) -> int {
    marks = v
    return v
}
fn touch() -> str {
    seen = 1
    return ""
}
component P() {
    let x = note(seen)
    <div #p><b>{x}</b>{touch()}<Level level={pg}/></div>
}
component Box() {
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_write_in_a_let_does_not_hide_a_later_read() {
    let Some((v, _)) = in_chrome(
        "letw",
        LETW,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(200);
  const errs = w.__quilloErrors.length;
  d.getElementById("set").click(); await tick(400);
  return { errs, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, b: d.querySelector("#p b") && d.querySelector("#p b").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["errs"], &v["lvl"], &v["b"]), (&serde_json::json!(1), &serde_json::json!("hard"), &serde_json::json!("1")), "{}", v);
}

// ---- the eleventh review of slice C ---------------------------------------------------------

/// A failed draw whose markup writes a state nothing in it reads, before a page state that is null,
/// a page state ticking that the draw reads past the write: the component ahead of the failure loads
/// its state once for the failed draw and once for the draw that works, not at each tick. Before the
/// fix a probe that skipped a write always passed, the draw was made for real at each tick, and the
/// component loaded again each time: 12 loads.
const RL: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
view Clock(t: int) = <u>{t}</u>
fn nothing() -> any = null
state pg = nothing()
state clock = 0
state seen = 0
server fn load_n() -> int {
    return 7
}
fn touch() -> str {
    seen = 1
    return ""
}
component Loader() {
    state d = load_n()
    <em>{d}</em>
}
component P() {
    <div #p><Loader/>{touch()}<Clock t={clock}/><Level level={pg}/></div>
}
component Box() {
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #tick onclick={() => { clock = clock + 1 }}>tick</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_skipped_write_nothing_reads_does_not_redraw_at_each_wake() {
    let Some((v, log)) = in_chrome(
        "rl",
        RL,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(400);
  for (let i = 0; i < 10; i++) { d.getElementById("tick").click(); await tick(120); }
  const errs = w.__quilloErrors.length;
  d.getElementById("set").click(); await tick(400);
  return { errs, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, u: d.querySelector("#p u") && d.querySelector("#p u").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["errs"], &v["lvl"], &v["u"]), (&serde_json::json!(1), &serde_json::json!("hard"), &serde_json::json!("10")), "{}", v);
    assert!(log.matches("load_n").count() <= 2, "{}", log);
}

/// `notify`, `run_after` and `channel.open` in failing markup, its component's state changing: the
/// failed draw's and the working draw's, none of the probes'. Before the fix each probe sent a
/// notification: 9 where the draws made 2.
const ACTS2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state fired = 0
fn ping(k: int) -> str {
    notify("drawn {k}")
    return ""
}
fn tick() -> str {
    run_after(0, () => { fired = fired + 1 })
    return ""
}
fn chan() -> str {
    channel.open("room", (m) => { print(m) })
    return ""
}
component P() {
    state k = 0
    <div #p><button #k onclick={() => { k = k + 1 }}>k</button>{k >= 0 ? <span>{ping(k)}{tick()}{chan()}<Level level={pg}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #f>{fired}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_sends_no_notification() {
    let Some((v, _)) = in_chrome(
        "acts2",
        ACTS2,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(300);
  const t0 = d.querySelectorAll(".q-toast").length;
  for (let i = 0; i < 5; i++) { d.getElementById("k").click(); await tick(150); }
  const t1 = d.querySelectorAll(".q-toast").length;
  d.getElementById("set").click(); await tick(400);
  return { t0, t1, t2: d.querySelectorAll(".q-toast").length, f2: d.getElementById("f").textContent, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["t0"], &v["t1"], &v["t2"], &v["lvl"]), (&serde_json::json!(1), &serde_json::json!(1), &serde_json::json!(2), &serde_json::json!("hard")), "{}", v);
}

/// A method call and a field set on a page object the program holds (an element), in failing markup:
/// the real draws' alone, none of the probes'. Before the eleventh review's fix each probe appended
/// too: 9. Changed for the twelfth review, which decided that a call of the page's ends a probe and
/// the draw is made for real, as its answer is not known (a skipped call's null failed a view the
/// real draw passed): the draw is made at each change of the component's state (5) and once the page
/// state is set, one append each, as 0.8.11 made them.
const FM: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state box = nothing()
fn mark(b: any, k: int) -> str {
    if b != null {
        b.append("x")
        b.title = "t{k}"
    }
    return ""
}
component P() {
    state k = 0
    <div #p><button #k onclick={() => { k = k + 1 }}>k</button>{k >= 0 ? <span>{mark(box, k)}<Level level={pg}/></span> : null}</div>
}
component Box() {
    state on = false
    <div>
        <i #log @mount={(el) => { box = el }}></i>
        <button #on onclick={() => { on = true }}>on</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_calls_no_method_on_a_page_object() {
    let Some((v, _)) = in_chrome(
        "fm",
        FM,
        r##"
  const [w, d] = await open("/");
  await tick(200);
  d.getElementById("on").click(); await tick(300);
  const a0 = d.getElementById("log").textContent.length;
  for (let i = 0; i < 5; i++) { d.getElementById("k").click(); await tick(150); }
  const a1 = d.getElementById("log").textContent.length;
  d.getElementById("set").click(); await tick(400);
  return { a0, a1, a2: d.getElementById("log").textContent.length, title: d.getElementById("log").title, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["a0"], &v["a1"], &v["a2"], &v["lvl"]), (&serde_json::json!(1), &serde_json::json!(6), &serde_json::json!(7), &serde_json::json!("hard")), "{}", v);
}

// ---- the twelfth review of slice C ----------------------------------------------------------

/// A view given what a page object's method answers (`box.getAttribute`, `box` set by an `@mount`),
/// as a place and as a component's view, failing while `box` is null: drawn once the element is
/// there. Before the fix a probe gave the call null for an answer, the view failed on it, and the
/// draw waited for good, its failure said again.
const FC: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state box = nothing()
fn label(b: any) -> any {
    if b == null {
        return nothing()
    }
    return b.getAttribute("data-l")
}
component P() {
    state on2 = true
    <div><p #p>{on2 ? <span><Level level={label(box)}/></span> : null}</p></div>
}
component Q() {
    <div #q><Level level={label(box)}/></div>
}
component Box() {
    state on = false
    state ready = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #ready onclick={() => { ready = true }}>ready</button>
        {on ? [<P/>, <Q/>] : null}
        {ready ? <i #src data-l="hard" @mount={(el) => { box = el }}></i> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_page_objects_answer_reaches_a_view_once_it_is_there() {
    let Some((v, _)) = in_chrome(
        "fc",
        FC,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(200);
  const errs = w.__quilloErrors.length;
  d.getElementById("ready").click(); await tick(400);
  return { errs, errs2: w.__quilloErrors.length, p: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, q: d.querySelector("#q .lvl") && d.querySelector("#q .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["errs"], &v["errs2"], &v["p"], &v["q"]), (&serde_json::json!(2), &serde_json::json!(2), &serde_json::json!("hard"), &serde_json::json!("hard")), "{}", v);
}

/// Failing markup that writes a page state (`note(k)`) which only another component's statement
/// reads, that statement filling what the failure waits on: drawn once it is filled. Before the fix
/// the probe passed only when it read a state whose write it skipped, or what a `let` worked out from
/// one: the statement was not seen, the write never made, and the draw waited for good (0.8.11 drew
/// it).
const STM: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state q = 0
state info = nothing()
fn note(k: int) -> str {
    q = k
    return ""
}
component P() {
    state k = 0
    <div><button #k onclick={() => { k += 1 }}>k</button><p #p>{k >= 0 ? <span>{note(k)}<Level level={info}/></span> : null}</p></div>
}
component R() {
    if q > 1 {
        info = "hard"
    }
    <i #r>r</i>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button><R/>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_skipped_write_a_statement_reads_is_made() {
    let Some((v, _)) = in_chrome(
        "stm",
        STM,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(200);
  for (let i = 0; i < 2; i++) { d.getElementById("k").click(); await tick(200); }
  await tick(200);
  return { lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["lvl"], &v["errs"]), (&serde_json::json!("hard"), &serde_json::json!(1)), "{}", v);
}

/// `room.send` in failing markup, its component's state changing: sent by the real draws alone (the
/// failed one and the one that works), not by each probe. Before the fix each probe sent too: 6 by
/// the fifth change.
const CHS: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
let sent = 0
channel.on("room", (msg, peer) => {
    sent += 1
    null
})
state lv = nothing()
component P() {
    state k = 0
    let room = channel.open("room", (m) => { null })
    <div><button #k onclick={() => { k += 1 }}>k</button><p #p>{k >= 0 ? <span>{str(room.send(k))}<Level level={lv}/></span> : null}</p></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { lv = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /said = sent
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_probe_sends_nothing_on_a_channel() {
    let Some((v, _)) = in_chrome(
        "chs",
        CHS,
        r##"
  const [w, d] = await open("/");
  const said = async () => +(await (await fetch("/said")).text());
  d.getElementById("on").click(); await tick(500);
  const s0 = await said();
  for (let i = 0; i < 5; i++) { d.getElementById("k").click(); await tick(150); }
  await tick(300);
  const s1 = await said();
  d.getElementById("set").click(); await tick(500);
  return { s0, s1, s2: await said(), lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["s0"], &v["s1"], &v["s2"], &v["lvl"]), (&serde_json::json!(1), &serde_json::json!(1), &serde_json::json!(2), &serde_json::json!("hard")), "{}", v);
}

// ---- the thirteenth review of slice C -------------------------------------------------------

/// rl with a place elsewhere that shows the state the failing markup writes (`<i>{seen}</i>`): the
/// component ahead of the failure loads its state for the failed draw and for the one that works,
/// not at each tick. Before the fix any reader of the state made the skipped write count, and the
/// draw was made for real at each tick: 12 loads.
const RLR: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
view Clock(t: int) = <u>{t}</u>
fn nothing() -> any = null
state pg = nothing()
state clock = 0
state seen = 0
server fn load_n() -> int {
    return 7
}
fn touch() -> str {
    seen = 1
    return ""
}
component Loader() {
    state d = load_n()
    <em>{d}</em>
}
component P() {
    <div #p><Loader/>{touch()}<Clock t={clock}/><Level level={pg}/></div>
}
component Box() {
    state on = false
    <div>
        <i #seen>{seen}</i>
        <button #on onclick={() => { on = true }}>on</button>
        <button #tick onclick={() => { clock = clock + 1 }}>tick</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

/// rl whose write reads its own state (`seen += 1`): loaded twice, not at each tick. Before the fix
/// the probe's own read (before the write) made the write count: 12 loads.
const RLS: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
view Clock(t: int) = <u>{t}</u>
fn nothing() -> any = null
state pg = nothing()
state clock = 0
state seen = 0
server fn load_n() -> int {
    return 7
}
fn touch() -> str {
    seen += 1
    return ""
}
component Loader() {
    state d = load_n()
    <em>{d}</em>
}
component P() {
    <div #p><Loader/>{touch()}<Clock t={clock}/><Level level={pg}/></div>
}
component Box() {
    state on = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #tick onclick={() => { clock = clock + 1 }}>tick</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

/// rl+(a) whose markup writes a new value at each tick (`seen = clock`) and reads, after the write, a
/// `let` the write does not reach: loaded twice, not at each tick.
const RLM: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
view Clock(t: int) = <u>{t}</u>
fn nothing() -> any = null
state pg = nothing()
state clock = 0
state seen = 0
server fn load_n() -> int {
    return 7
}
fn touch() -> str {
    seen = clock
    return ""
}
component Loader() {
    state d = load_n()
    <em>{d}</em>
}
component P() {
    let shown = clock + 0
    <div #p><Loader/>{touch()}<Clock t={shown}/><Level level={pg}/></div>
}
component Box() {
    state on = false
    <div>
        <i #seen>{seen}</i>
        <button #on onclick={() => { on = true }}>on</button>
        <button #tick onclick={() => { clock = clock + 1 }}>tick</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_skipped_write_shown_elsewhere_or_read_before_does_not_redraw_at_each_wake() {
    for (name, src) in [("rlr", RLR), ("rls", RLS), ("rlm", RLM)] {
        let Some((v, log)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(400);
  for (let i = 0; i < 10; i++) { d.getElementById("tick").click(); await tick(120); }
  const errs = w.__quilloErrors.length;
  d.getElementById("set").click(); await tick(400);
  return { errs, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, u: d.querySelector("#p u") && d.querySelector("#p u").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!((&v["errs"], &v["lvl"], &v["u"]), (&serde_json::json!(1), &serde_json::json!("hard"), &serde_json::json!("10")), "{}: {}", name, v);
        assert!(log.matches("load_n").count() <= 2, "{}: {}", name, log);
    }
}

/// rl with the clock ticking by `every` (10 a second): loaded twice in 2 s, not at each tick. Before
/// the fix about 10 loads a second.
const RLT: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
view Clock(t: int) = <u>{t}</u>
fn nothing() -> any = null
state pg = nothing()
state clock = 0
state seen = 0
server fn load_n() -> int {
    return 7
}
fn touch() -> str {
    seen = 1
    return ""
}
component Loader() {
    state d = load_n()
    <em>{d}</em>
}
component P() {
    <div #p><Loader/>{touch()}<Clock t={clock}/><Level level={pg}/></div>
}
component Box() {
    state on = false
    every(100, () => { clock += 1 })
    <div>
        <i #seen>{seen}</i>
        <button #on onclick={() => { on = true }}>on</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_skipped_write_does_not_redraw_at_each_tick_of_a_timer() {
    let Some((v, log)) = in_chrome(
        "rlt",
        RLT,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(2000);
  const errs = w.__quilloErrors.length;
  d.getElementById("set").click(); await tick(400);
  return { errs, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["errs"], &v["lvl"]), (&serde_json::json!(1), &serde_json::json!("hard")), "{}", v);
    assert!(log.matches("load_n").count() <= 2, "{}", log);
}

/// A `js` call in failing markup whose frame callback writes a page state the component does not
/// read: the draw is not made again at each frame, so the count stays; the draw comes back once the
/// page state it reads is set. Before the fix the stopped probe waited on every page state, and the
/// count went round at the rate of frames: 121 in 2 s (0.8.11: 1).
const PSL: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
component P() {
    state k = 0
    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Level level={pg}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_page_state_the_failed_code_does_not_read_does_not_wake_it() {
    let Some((v, _)) = in_chrome(
        "psl",
        PSL,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(1500);
  const h1 = +d.getElementById("h").textContent;
  await tick(500);
  const h2 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(400);
  return { h1, h2, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    let n = |k: &str| v[k].as_i64().unwrap_or(-1);
    assert!(n("h1") <= 3 && n("h2") == n("h1"), "{}", v);
    assert_eq!(v["lvl"], serde_json::json!("hard"), "{}", v);
}

/// fm with a page state ticking by `every` that the component does not read: the page object's
/// method is called by the failed draw and by the one that works, not at each tick. Before the fix
/// 21 calls in 2 s.
const FMC: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state box = nothing()
state clock = 0
fn mark(b: any) -> str {
    if b != null {
        b.append("x")
    }
    return ""
}
component P() {
    <div #p><span>{mark(box)}<Level level={pg}/></span></div>
}
component Box() {
    state on = false
    every(100, () => { clock += 1 })
    <div>
        <i #log @mount={(el) => { box = el }}></i>
        <button #on onclick={() => { on = true }}>on</button>
        <button #set onclick={() => { pg = "hard" }}>set</button>
        {on ? <P/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_ticking_page_state_the_failed_code_does_not_read_makes_no_draw() {
    let Some((v, _)) = in_chrome(
        "fmc",
        FMC,
        r##"
  const [w, d] = await open("/");
  await tick(200);
  d.getElementById("on").click(); await tick(2000);
  const a1 = d.getElementById("log").textContent.length;
  d.getElementById("set").click(); await tick(400);
  return { a1, a2: d.getElementById("log").textContent.length, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["a1"], &v["a2"], &v["lvl"]), (&serde_json::json!(1), &serde_json::json!(2), &serde_json::json!("hard")), "{}", v);
}

// ---- the fourteenth review of slice C -------------------------------------------------------

/// A failed draw whose markup makes a `js` call and then reads a page state through a helper `fn`
/// (`getpg()`), through a view (`<LvPg/>`), or reads it while a handler of its own writes it: drawn
/// once the page state is set, and a frame callback writing a page state the draw does not read
/// does not wake it. Before the fix the page states a stopped probe waited on were found in the
/// component's own compiled code: the helper's and the view's read was missed, and a state the
/// code also wrote (in a handler) was left out, so the draw waited for good.
const FACTS_H: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
fn getpg() -> any {
    return pg
}
view LvPg() = <Level level={pg}/>

component P() {
    state k = 0
    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Level level={getpg()}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const FACTS_V: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
fn getpg() -> any {
    return pg
}
view LvPg() = <Level level={pg}/>

component P() {
    state k = 0
    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><LvPg/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const FACTS_RW: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
fn getpg() -> any {
    return pg
}
view LvPg() = <Level level={pg}/>

component P() {
    state k = 0
    <div #p><button #reset onclick={() => { pg = nothing() }}>reset</button>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Level level={pg}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_stopped_probe_waits_on_what_its_helpers_views_and_handlers_read() {
    for (name, src) in [("facts-h", FACTS_H), ("facts-v", FACTS_V), ("facts-rw", FACTS_RW)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(800);
  const h1 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(600);
  return { h1, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!(v["lvl"], serde_json::json!("hard"), "{}: {}", name, v);
        assert!(v["h1"].as_i64().unwrap_or(99) <= 3, "{}: {}", name, v);
    }
}

/// loopw3 with a place whose text holds `W.put(`: not taken for a write, so the count stays at the
/// first draw's. Before the fix a scan of the place's compiled code took the string for one, and the
/// count went up at each tick (52).
const FACTS_LS: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state hits = 0
fn seen(x: any) -> any {
    hits = hits + 1
    return x
}
component Box() {
    state t = null
    state on = false
    state k = 0
    every(100, () => { k += 1 })
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <i #hits>{hits}</i><u #k>{k}</u><s>{"W.put(" + str(hits)}</s>
        {on ? <p #p><Level level={seen(t)}/></p> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_string_in_a_place_is_not_a_write() {
    let Some((v, _)) = in_chrome(
        "facts-ls",
        FACTS_LS,
        r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(500); const h1 = +d.getElementById("hits").textContent, k1 = +d.getElementById("k").textContent; await tick(2000);
  return { h1, k1, hits: +d.getElementById("hits").textContent, k: +d.getElementById("k").textContent };
"##,
    ) else {
        return;
    };
    let n = |k: &str| v[k].as_i64().unwrap_or(-1);
    assert!(n("k") > n("k1"), "{}", v);
    assert_eq!(n("hits"), n("h1"), "{}", v);
}

/// A view argument that reads a state, writes it and reads it back, with few states read before
/// and with many: the same either way, drawn once it passes. Before the fix "after the write" turned
/// on `link` keeping a state read again among the first few once: with few read before, the read
/// back was not seen, and the draw waited for good (tries 2, nothing shown).
const TRIES_FEW: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state tries = 0
state a1 = 1
state a2 = 1
state a3 = 1
state a4 = 1
state a5 = 1
state a6 = 1
state a7 = 1
state a8 = 1
state a9 = 1
fn check(x: any) -> any {
    
    tries = tries + 1
    if tries < 3 {
        return null
    }
    return "ok"
}
component Box() {
    state on = false
    state k = 0
    every(100, () => { k += 1 })
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <i #tries>{tries}</i><u #k>{k}</u>
        {on ? <p #p><Level level={check(k)}/></p> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const TRIES_MANY: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
state tries = 0
state a1 = 1
state a2 = 1
state a3 = 1
state a4 = 1
state a5 = 1
state a6 = 1
state a7 = 1
state a8 = 1
state a9 = 1
fn check(x: any) -> any {
    let _pad = a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9
    tries = tries + 1
    if tries < 3 {
        return null
    }
    return "ok"
}
component Box() {
    state on = false
    state k = 0
    every(100, () => { k += 1 })
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <i #tries>{tries}</i><u #k>{k}</u>
        {on ? <p #p><Level level={check(k)}/></p> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_read_back_is_seen_whatever_was_read_before() {
    for (name, src) in [("tries-few", TRIES_FEW), ("tries-many", TRIES_MANY)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click();
  await tick(1500);
  return { tries: +d.getElementById("tries").textContent, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!(v["lvl"], serde_json::json!("ok"), "{}: {}", name, v);
    }
}

/// The code only a failure runs is a file of its own: a page that never fails does not load it, and
/// two draws that fail load it once, both drawn once what they wait on comes (the user's decision,
/// the thirteenth review of slice C).
const LAZY1: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state qg = nothing()
component P() {
    <div #p><Level level={pg}/></div>
}
component Q() {
    <div #q><Level level={qg}/></div>
}
component Box() {
    state on = false
    state on2 = false
    <div>
        <button #on onclick={() => { on = true }}>on</button>
        <button #on2 onclick={() => { on2 = true }}>on2</button>
        <button #set onclick={() => { pg = "hard"
            qg = "easy" }}>set</button>
        {on ? <P/> : null}
        {on2 ? <Q/> : null}
    </div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn the_failure_code_loads_once_on_the_first_failure() {
    let Some((v, log)) = in_chrome(
        "lazy1",
        LAZY1,
        r##"
  const [w, d] = await open("/");
  await tick(300);
  const before = [...w.document.scripts].filter((s) => /\/_quillo\/r\/f[0-9a-f]{16}\.js$/.test(s.src)).length;
  d.getElementById("on").click(); await tick(400);
  d.getElementById("on2").click(); await tick(400);
  const errs = w.__quilloErrors.length;
  d.getElementById("set").click(); await tick(400);
  return { before, errs, p: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent, q: d.querySelector("#q .lvl") && d.querySelector("#q .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["before"], &v["errs"], &v["p"], &v["q"]), (&serde_json::json!(0), &serde_json::json!(2), &serde_json::json!("hard"), &serde_json::json!("easy")), "{}", v);
    let rec: Vec<&str> = log.lines().filter(|l| l.contains("/_quillo/r/f")).collect();
    assert_eq!(rec.len(), 1, "{}", log);
}

/// Under Quillo's worker (an `offline` route), the code only a failure runs is kept as the page
/// settles, before any failure, so that a failure offline is handled too.
const LAZYOFF: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
component P() {
    <div #p><Level level={pg}/></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route offline GET / = <html><body><Box/></body></html>
route GET /ping = "pong"
"##;

#[test]
fn the_failure_code_is_kept_by_the_worker() {
    let Some((v, _)) = in_chrome(
        "lazyoff",
        LAZYOFF,
        r##"
  let [w, d] = await open("/");
  const sw = w.navigator.serviceWorker;
  await sw.ready;
  for (let i = 0; i < 300 && !sw.controller; i++) await tick(20);
  const f = document.querySelector("iframe");
  f.contentWindow.location.reload();
  await new Promise((r) => f.addEventListener("load", r, { once: true }));
  w = f.contentWindow; d = f.contentDocument;
  await wait(() => w[Symbol.for("quillo")] && w[Symbol.for("quillo")].booted);
  const src = [...d.scripts].map((s) => s.src).find((s) => /\/_quillo\/r\/[0-9a-f]{16}\.js$/.test(s));
  const rt = await (await fetch(src)).text();
  const at = /RecAt = "([^"]+)"/.exec(rt)[1];
  let kept = false;
  for (let i = 0; i < 100 && !kept; i++) { await tick(50); kept = !!(await w.caches.match(at)); }
  d.getElementById("on").click(); await tick(300);
  d.getElementById("set").click(); await tick(400);
  return { controlled: !!w.navigator.serviceWorker.controller, kept, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["controlled"], &v["kept"], &v["lvl"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!("hard")), "{}", v);
}

// ---- the fourteenth review of slice C (round 17) --------------------------------------------

/// A failed draw whose markup makes a `js` call and then reads a page state in a lambda that runs
/// as it draws (`[1].map((i) => ...)`), through a `let` lambda (`let g = () => pg`), or through an
/// inner `fn` of the component: drawn once the page state is set. Before the fix the compiler's
/// facts left out what lambdas and inner `fn`s read, and the stopped probe waited for good.
const F_MAP: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0

component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Level level={pg}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_LETLAM: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0

component P() {
    state k = 0
    let g = () => pg
    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Level level={g()}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_INNERFN: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0

component P() {
    state k = 0
    fn g() -> any {
        return pg
    }
    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Level level={g()}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn what_lambdas_and_inner_fns_read_is_waited_on() {
    for (name, src) in [("f-map", F_MAP), ("f-letlam", F_LETLAM), ("f-innerfn", F_INNERFN)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(800);
  const h1 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(800);
  return { h1, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!(v["lvl"], serde_json::json!("hard"), "{}: {}", name, v);
        assert!(v["h1"].as_i64().unwrap_or(99) <= 3, "{}: {}", name, v);
    }
}

/// A `for` whose list fails as it is first made (`upper(t)` of a null state, a component's or the
/// page's): drawn once the list can be worked out. Before the fix (and on 0.8.11) its next run put
/// its rows where its first make's nodes had been, long taken, and nothing showed.
const V_FORFIRST: &str = r##"fn nothing() -> any = null
component P() {
    state t = nothing()
    <div><button #set onclick={() => { t = "hard" }}>set</button><div #p>{for x in [upper(t)] { <b>{x}</b> }}</div><i #t>{t}</i></div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const V_FORL: &str = r##"fn nothing() -> any = null
state pg = nothing()

component P() {
    <div #p>{for x in [upper(pg)] { <b>{x}</b> }}</div>
}
component Box() {
    state on = false
    <div><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_for_whose_first_list_fails_is_drawn_once_it_works() {
    for (name, src) in [("v-forfirst", V_FORFIRST), ("v-forl", V_FORL)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(600);
  const p0 = d.getElementById("p") && d.getElementById("p").innerHTML;
  d.getElementById("set").click(); await tick(800);
  return { p0, p: d.getElementById("p") && d.getElementById("p").innerHTML, errs: w.__quilloErrors.length };
"##,
        ) else {
            return;
        };
        assert_eq!((&v["p0"], &v["p"], &v["errs"]), (&serde_json::json!(""), &serde_json::json!("<b>HARD</b>"), &serde_json::json!(1)), "{}: {}", name, v);
    }
}

/// The session that installs Quillo's worker keeps the code only a failure runs (once the worker
/// takes the page), and with the server stopped a failure is handled: the card drawn once its state
/// comes. Before the fix the page checked for a worker once, as it loaded, before there was one, and
/// the file was never kept: offline, the failure waited for good.
const PWA_INSTALL: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>

component Card(t: map?) {
    <section #card><Level level={t.level}/></section>
}

component Lab() {
    state t = null
    state card = false
    <div>
        <button #bad onclick={() => { t = { level: 5 } }}>bad</button>
        <button #good onclick={() => { t = { level: "easy" } }}>good</button>
        <button #card-on onclick={() => { card = !card }}>card</button>
        {card ? <Card t={t}/> : null}
    </div>
}

route offline GET /lab = <html><body><Lab/></body></html>
route GET /ping = "pong"
"##;

#[test]
fn a_failure_offline_in_the_session_that_installs_the_worker_is_handled() {
    let Some((v, _)) = in_chrome(
        "pwa-install",
        PWA_INSTALL,
        r##"
  const [w, d] = await open("/lab");
  const ctlAtBoot = !!w.navigator.serviceWorker.controller;
  await wait(() => w.navigator.serviceWorker.controller, 10000);
  let kept = false;
  for (let i = 0; i < 100 && !kept; i++) {
    await tick(50);
    for (const n of await caches.keys()) for (const r of await (await caches.open(n)).keys()) if (/\/_quillo\/r\/f[0-9a-f]{16}\.js$/.test(new URL(r.url).pathname)) kept = true;
  }
  await fetch("@STOP@");
  const down = await (async () => { for (let i = 0; i < 100; i++) { try { await fetch("/ping", { cache: "no-store" }); } catch (_) { return true; } await tick(50); } return false; })();
  d.querySelector("#card-on").click();
  await tick(300);
  d.querySelector("#good").click();
  const ok = await wait(() => d.querySelector(".lvl") && d.querySelector(".lvl").textContent === "easy", 8000);
  return { ctlAtBoot, kept, down, ok };
"##,
    ) else {
        return;
    };
    // (Whether a worker had the page as it loaded depends on the browser the tests run in: the
    // coverage runs' had one from before.)
    assert_eq!((&v["kept"], &v["down"], &v["ok"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(true)), "{}", v);
}

/// A failure code file this server does not have (a page from before the program changed) is not
/// found, never this program's code against that page's runtime.
#[test]
fn an_unknown_failure_code_file_is_not_found() {
    let s = served("rec404", "component C() {\n    <p>c</p>\n}\nroute GET / = <html><body><C/></body></html>\n");
    let a = support::send(s.port, "GET", "/_quillo/r/f0123456789abcdef.js", &[], "");
    assert_eq!(a.status, 404, "{}", a.body);
}

/// The failure code's file not had (no network, no worker's copy): tried again, and at once when
/// the browser is back online, and the failure is then handled (the fourteenth review of slice C).
#[test]
fn the_failure_code_is_tried_again_when_back_online() {
    let Some((v, log)) = in_chrome(
        "rec-online",
        LAZY1,
        r##"
  const [w, d] = await open("/");
  const add = w.document.head.appendChild.bind(w.document.head), get = w.fetch;
  let broken = true;
  w.document.head.appendChild = (el) => { if (broken && el.src && /\/_quillo\/r\/f/.test(el.src)) el.src = "http://127.0.0.1:1/missing.js"; return add(el); };
  w.fetch = (u, o) => broken && o && o.method === "HEAD" ? Promise.reject(new TypeError("offline")) : get(u, o);
  d.getElementById("on").click(); await tick(1500);
  const before = d.querySelector("#p .lvl");
  broken = false;
  w.dispatchEvent(new w.Event("online"));
  await tick(500);
  d.getElementById("set").click(); await tick(500);
  return { before: !!before, p: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["before"], &v["p"]), (&serde_json::json!(false), &serde_json::json!("hard")), "{}", v);
    assert!(log.lines().filter(|l| l.contains("/_quillo/r/f")).count() >= 1, "{}", log);
}

/// The failure code's file not found (a page from before the program changed): the page loads
/// again, once, never a second time for the same file (the fourteenth review of slice C).
#[test]
fn a_stale_failure_code_file_loads_the_page_again_once() {
    let Some((v, _)) = in_chrome(
        "rec-stale",
        LAZY1,
        r##"
  let [w, d] = await open("/");
  const f = document.querySelector("iframe");
  let loads = 0;
  f.addEventListener("load", () => { loads++; });
  const breakIt = (w) => {
    const add = w.document.head.appendChild.bind(w.document.head), get = w.fetch;
    w.document.head.appendChild = (el) => { if (el.src && /\/_quillo\/r\/f/.test(el.src)) el.src = "http://127.0.0.1:1/missing.js"; return add(el); };
    w.fetch = (u, o) => o && o.method === "HEAD" ? Promise.resolve(new w.Response(null, { status: 404 })) : get(u, o);
  };
  breakIt(w);
  d.getElementById("on").click();
  await wait(() => loads >= 1, 3000);
  await wait(() => f.contentWindow[Symbol.for("quillo")] && f.contentWindow[Symbol.for("quillo")].booted);
  w = f.contentWindow; d = f.contentDocument;
  breakIt(w);
  d.getElementById("on").click();
  await tick(1500);
  return { loads, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["loads"], &v["errs"]), (&serde_json::json!(1), &serde_json::json!(1)), "{}", v);
}

/// The worker's install keeps the code only a failure runs (the runtime file names it), before the
/// page asks for it: with the page's own fetch of it blocked, the first visit, with the server
/// stopped, still handles a failure (the fourteenth review of slice C).
#[test]
fn the_workers_install_keeps_the_failure_code() {
    let Some((v, _)) = in_chrome(
        "pwa-install-keep",
        PWA_INSTALL,
        r##"
  const [w, d] = await open("/lab");
  const get = w.fetch;
  w.fetch = (u, o) => /\/_quillo\/r\/f[0-9a-f]{16}\.js/.test(String(u)) ? Promise.reject(new TypeError("blocked")) : get(u, o);
  await wait(() => w.navigator.serviceWorker.controller, 10000);
  let kept = false;
  for (let i = 0; i < 100 && !kept; i++) {
    await tick(50);
    for (const n of await caches.keys()) for (const r of await (await caches.open(n)).keys()) if (/\/_quillo\/r\/f[0-9a-f]{16}\.js$/.test(new URL(r.url).pathname)) kept = true;
  }
  await fetch("@STOP@");
  const down = await (async () => { for (let i = 0; i < 100; i++) { try { await fetch("/ping", { cache: "no-store" }); } catch (_) { return true; } await tick(50); } return false; })();
  d.querySelector("#card-on").click();
  await tick(300);
  d.querySelector("#good").click();
  const ok = await wait(() => d.querySelector(".lvl") && d.querySelector(".lvl").textContent === "easy", 8000);
  return { kept, down, ok };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["kept"], &v["down"], &v["ok"]), (&serde_json::json!(true), &serde_json::json!(true), &serde_json::json!(true)), "{}", v);
}

// ---- the fifteenth review of slice C (round 18) ---------------------------------------------

/// A page state read through the program's own `fn on(f)` (a builtin's name), through a view's
/// render prop named `only`, and through two functions that call each other, one of them read by
/// another component first: drawn once it is set. Before the fix the facts went by name (`on` was
/// taken for the builtin, a prop named `on...`/`only` for a handler) and a cycle's function was given
/// part of its group's result, and the stopped probe waited for good.
const F_THEN2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
fn on(f: any) -> any {
    return f()
}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Level level={on(() => pg)}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_ONLY2: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
view Each(only: any) = <Level level={only()}/>
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Each only={() => pg}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_CYCLE3: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
fn a(n: int) -> any {
    if n > 0 {
        return b(n - 1)
    }
    return pg
}
fn b(n: int) -> any {
    return a(n)
}
component X() {
    <i>{hits > 100 ? a(5) : ""}</i>
}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Level level={b(1)}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><X/><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn the_facts_go_by_what_names_are_not_their_names() {
    for (name, src) in [("f-then2", F_THEN2), ("f-only2", F_ONLY2), ("f-cycle3", F_CYCLE3)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(800);
  const h1 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(800);
  return { h1, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!(v["lvl"], serde_json::json!("hard"), "{}: {}", name, v);
        assert!(v["h1"].as_i64().unwrap_or(99) <= 3, "{}: {}", name, v);
    }
}

/// A page whose failure code file is not found (a page from before the program changed): not
/// loaded again where its person has typed something, nor for a file it was already loaded again
/// for in this tab, whichever of several (two versions of the program behind one address).
const R404: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>

component Card(t: map?) {
    <section #card><Level level={t.level}/></section>
}

component Lab() {
    state t = null
    state card = false
    <div>
        <input #name/>
        <button #good onclick={() => { t = { level: "easy" } }}>good</button>
        <button #card-on onclick={() => { card = !card }}>card</button>
        {card ? <Card t={t}/> : null}
    </div>
}

route GET /lab = <html><body><Lab/></body></html>
"##;

#[test]
fn a_stale_failure_file_does_not_reload_over_typing_or_twice() {
    let Some((v, _)) = in_chrome(
        "r404",
        R404,
        r##"
  let [w, d] = await open("/lab");
  const f = document.querySelector("iframe");
  let loads = 0;
  f.addEventListener("load", () => { loads++; });
  const rig = (w, d) => {
    const add = d.head.appendChild.bind(d.head), get = w.fetch.bind(w);
    d.head.appendChild = (el) => { if (el.src && /\/_quillo\/r\/f/.test(el.src)) el.src = el.src.replace(/\/f[0-9a-f]{16}\.js/, "/f0000000000000000.js"); return add(el); };
    w.fetch = (u, o) => o && o.method === "HEAD" && String(u).includes("/_quillo/r/f") ? get(String(u).replace(/\/f[0-9a-f]{16}\.js/, "/f0000000000000000.js"), o) : get(u, o);
  };
  rig(w, d);
  d.getElementById("name").value = "typed by hand";
  d.querySelector("#card-on").click();
  await tick(1500);
  const typed = { loads, name: d.getElementById("name").value };
  // Another tab state: this file's and another version's already reloaded for.
  const rt = await (await fetch([...d.scripts].map((s) => s.src).find((s) => /\/_quillo\/r\/[0-9a-f]{16}\.js$/.test(s)))).text();
  const at = /RecAt = "([^"]+)"/.exec(rt)[1];
  w.sessionStorage.setItem("quillo:rec", JSON.stringify(["/_quillo/r/f1111111111111111.js", at]));
  d.getElementById("name").value = "";
  d.querySelector("#card-on").click(); await tick(200);
  d.querySelector("#card-on").click(); await tick(1500);
  return { typed, loads, errs: w.__quilloErrors.length };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["typed"]["loads"], &v["typed"]["name"], &v["loads"]), (&serde_json::json!(0), &serde_json::json!("typed by hand"), &serde_json::json!(0)), "{}", v);
}

/// The failure code file tried again and again with no network: one `online` listener for all the
/// tries. Before the fix each try added one.
#[test]
fn one_online_listener_for_all_the_tries() {
    let Some((v, _)) = in_chrome(
        "rec-online1",
        LAZY1,
        r##"
  const [w, d] = await open("/");
  const add = w.document.head.appendChild.bind(w.document.head), get = w.fetch;
  w.document.head.appendChild = (el) => { if (el.src && /\/_quillo\/r\/f/.test(el.src)) el.src = "http://127.0.0.1:1/missing.js"; return add(el); };
  w.fetch = (u, o) => o && o.method === "HEAD" ? Promise.reject(new TypeError("offline")) : get(u, o);
  const listen = w.addEventListener.bind(w);
  // The listeners, not the calls: each try adds the one named `recOnline`, which the browser keeps
  // once (the review's size cut, the sixteenth review of slice C; FEEDBACK88.md, Q-1380; branch
  // feedback86-89). Before the fifteenth review's fix each try added a listener of its own.
  const fns = new Set();
  w.addEventListener = (t, f, o) => { if (t === "online") fns.add(f); return listen(t, f, o); };
  d.getElementById("on").click(); await tick(4500);
  return { online: fns.size };
"##,
    ) else {
        return;
    };
    assert_eq!(v["online"], serde_json::json!(1), "{}", v);
}

/// A render prop handed on rather than called (to another view, kept in a `let`, given to a function,
/// mapped), a named handler, and a program's own `.then` and `channel`: the first four's reads are
/// the draw's, the named handler's are not, and the last two's lambdas run as the draw does (the
/// sixteenth review of slice C; FEEDBACK88.md, Q-1380; branch feedback86-89). Before the fix the
/// first four and the last two never drew their card once `pg` was set, and `P` took the named
/// handler's read and write (`{ r: ["other", "pg"], w: 1 }`).
const F_TWO: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
view Inner(only: any) = <Level level={only()}/>
view Each(only: any) = <Inner only={only}/>
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Each only={() => pg}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_STORE: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
fn call(g: any) -> any {
    return g()
}
state pg = nothing()
state hits = 0
view Each(only: any) = <Level level={call(only)}/>
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Each only={() => pg}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_MAPR: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
view Each(only: any) = <span>{[1].map(only)}</span>
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Each only={(i) => <Level level={pg}/>}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_NAMED: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
state other = 0
state picked = 0
component Pick(onPick: any) {
    let done = () => { onPick(1) }

    <button #pk @click={done}>pick</button>
}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Pick onPick={(n) => { picked = n + other }}/><Level level={pg}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_THENOBJ: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
let box = {then: (f) => f()}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Level level={box.then(() => pg)}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const F_CHAN: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
let channel = {open: (f) => f()}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Level level={channel.open(() => pg)}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_prop_handed_on_is_drawn_with_and_a_named_handler_is_not() {
    for (name, src) in [("f-two", F_TWO), ("f-store", F_STORE), ("f-mapr", F_MAPR), ("f-named", F_NAMED), ("f-thenobj", F_THENOBJ), ("f-chan", F_CHAN)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  const texts = await Promise.all([...d.scripts].filter((s) => s.src).map(async (s) => (await fetch(s.src)).text()));
  const m = /W\.component\("P",[\s\S]*?\{ r: (\[[^\]]*\]), w: ([01]) \}\)/.exec(texts.join("\n") + d.documentElement.outerHTML);
  d.getElementById("on").click(); await tick(800);
  const h1 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(800);
  return { h1, facts: m && m[1] + " " + m[2], lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!(v["lvl"], serde_json::json!("hard"), "{}: {}", name, v);
        assert!(v["h1"].as_i64().unwrap_or(99) <= 3, "{}: {}", name, v);
        if name == "f-named" {
            assert_eq!(v["facts"], serde_json::json!("[\"pg\"] 0"), "{}: {}", name, v);
        }
    }
}

/// A stale failure file: not loaded again over a changed `<select>`, a field typed in an open shadow
/// root or in a frame of this origin; loaded again where none was changed (a `<select>` with no
/// option chosen by its markup, a shadow root and a frame left alone); the sixteenth review of slice
/// C (FEEDBACK88.md, Q-1380; branch feedback86-89). Before the fix the first three loaded again.
const R404_MORE: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>

component Card(t: map?) {
    <section #card><Level level={t.level}/></section>
}

component Lab() {
    state t = null
    state card = false
    <div>
        <input #name/><select #pick><option>a</option><option>b</option></select><div #host></div>
        <button #good onclick={() => { t = { level: "easy" } }}>good</button>
        <button #card-on onclick={() => { card = !card }}>card</button>
        {card ? <Card t={t}/> : null}
    </div>
}

route GET /lab = <html><body><Lab/></body></html>
"##;

#[test]
fn a_stale_failure_file_does_not_reload_over_a_select_a_shadow_root_or_a_frame() {
    for (case, change) in [
        ("select", r#"d.getElementById("pick").value = "b";"#),
        ("shadow", r#"sr.querySelector("input").value = "typed in shadow";"#),
        ("frame", r#"fr.contentDocument.querySelector("textarea").value = "typed in frame";"#),
        ("none", ""),
    ] {
        let Some((v, _)) = in_chrome(
            &format!("r404-{}", case),
            R404_MORE,
            &r##"
  const [w, d] = await open("/lab");
  const f = document.querySelector("iframe");
  let loads = 0;
  f.addEventListener("load", () => { loads++; });
  const add = d.head.appendChild.bind(d.head), get = w.fetch.bind(w);
  d.head.appendChild = (el) => { if (el.src && /\/_quillo\/r\/f/.test(el.src)) el.src = el.src.replace(/\/f[0-9a-f]{16}\.js/, "/f0000000000000000.js"); return add(el); };
  w.fetch = (u, o) => o && o.method === "HEAD" && String(u).includes("/_quillo/r/f") ? get(String(u).replace(/\/f[0-9a-f]{16}\.js/, "/f0000000000000000.js"), o) : get(u, o);
  const sr = d.getElementById("host").attachShadow({ mode: "open" });
  sr.appendChild(d.createElement("input"));
  const fr = d.createElement("iframe");
  d.getElementById("host").appendChild(fr);
  await tick(100);
  fr.contentDocument.body.appendChild(fr.contentDocument.createElement("textarea"));
  @CHANGE@
  d.querySelector("#card-on").click();
  await wait(() => loads > 0, 3000);
  return { loads };
"##
            .replace("@CHANGE@", change),
        ) else {
            return;
        };
        assert_eq!(v["loads"], serde_json::json!(if case == "none" { 1 } else { 0 }), "{}: {}", case, v);
    }
}

/// Two components that read the same long list of page states (through a cycle of functions): the
/// second refers to the first's list, written once (`A.$f.r`), and a failed draw of it is drawn
/// once a state on that list is set (the sixteenth review of slice C; FEEDBACK88.md, Q-1380; branch
/// feedback86-89). Before the fix each wrote the whole list.
const SHARED_FACTS: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
state s0 = 0
state s1 = 0
state s2 = 0
state s3 = 0
state s4 = 0
state s5 = 0
state s6 = 0
state s7 = 0
state s8 = 0
state s9 = 0
fn g0(d: int) -> any {
    if d > 0 {
        return g1(d - 1)
    }
    return s0 >= 0 ? pg : pg
}
fn g1(d: int) -> any {
    if d > 0 {
        return g2(d - 1)
    }
    return s1 >= 0 ? pg : pg
}
fn g2(d: int) -> any {
    if d > 0 {
        return g3(d - 1)
    }
    return s2 >= 0 ? pg : pg
}
fn g3(d: int) -> any {
    if d > 0 {
        return g4(d - 1)
    }
    return s3 >= 0 ? pg : pg
}
fn g4(d: int) -> any {
    if d > 0 {
        return g5(d - 1)
    }
    return s4 >= 0 ? pg : pg
}
fn g5(d: int) -> any {
    if d > 0 {
        return g6(d - 1)
    }
    return s5 >= 0 ? pg : pg
}
fn g6(d: int) -> any {
    if d > 0 {
        return g7(d - 1)
    }
    return s6 >= 0 ? pg : pg
}
fn g7(d: int) -> any {
    if d > 0 {
        return g8(d - 1)
    }
    return s7 >= 0 ? pg : pg
}
fn g8(d: int) -> any {
    if d > 0 {
        return g9(d - 1)
    }
    return s8 >= 0 ? pg : pg
}
fn g9(d: int) -> any {
    if d > 0 {
        return g0(d - 1)
    }
    return s9 >= 0 ? pg : pg
}
component A() {
    <i #a>{g0(0) == null ? "none" : "some"}</i>
}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b><Level level={g5(3)}/></span> : null}</div>
}
component Box() {
    state on = false
    <div><A/><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn components_that_read_the_same_long_list_share_it() {
    let Some((v, _)) = in_chrome(
        "shared-facts",
        SHARED_FACTS,
        r##"
  const [w, d] = await open("/");
  const texts = (await Promise.all([...d.scripts].filter((s) => s.src).map(async (s) => (await fetch(s.src)).text()))).join("\n");
  const refs = (texts.match(/\{ r: G\["[AP]"\]\.\$f\.r, w: 0 \}/g) || []).length;
  const lists = (texts.match(/\{ r: \[[^\]]*"s9"\], w: 0 \}/g) || []).length;
  d.getElementById("on").click(); await tick(800);
  const h1 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(800);
  return { refs, lists, h1, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["refs"], &v["lists"], &v["lvl"]), (&serde_json::json!(1), &serde_json::json!(1), &serde_json::json!("hard")), "{}", v);
    assert!(v["h1"].as_i64().unwrap_or(99) <= 3, "{}", v);
}

/// A component first compiled as not waiting and then again as waiting (it is drawn by a function
/// that calls a `server fn`): its facts are its own list, not a reference to itself left from the
/// first try, which stopped the page from starting (the seventeenth review of slice C;
/// FEEDBACK88.md, Q-1380; branch feedback86-89). Before the fix `A` was `{ r: G["A"].$f.r }`.
const G_ASYNC: &str = r##"state s1 = 0
state s2 = 0
state s3 = 0
state s4 = 0
state s5 = 0
state s6 = 0
state s7 = 0
state s8 = 0
state s9 = 0
server fn getit() -> int = 1
fn helper() -> int {
    let x = <A/>
    let n = getit()
    return n
}
component Page() {
    state on = false
    state v = 0
    <div><button #b onclick={() => { on = true
        v = helper() }}>b</button><i #n>{v}</i>{on ? <A/> : null}</div>
}
component A() {
    <p #a>{s1 + s2 + s3 + s4 + s5 + s6 + s7 + s8 + s9}</p>
}
route GET / = <html><body><Page/></body></html>
"##;

#[test]
fn a_component_compiled_again_keeps_its_own_facts() {
    let Some((v, _)) = in_chrome(
        "g-async",
        G_ASYNC,
        r##"
  const [w, d] = await open("/");
  d.getElementById("b").click(); await tick(800);
  return { a: d.getElementById("a") && d.getElementById("a").textContent, n: d.getElementById("n") && d.getElementById("n").textContent, booted: !!w[Symbol.for("quillo")].booted };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["booted"], &v["a"], &v["n"], &v["errors"]), (&serde_json::json!(true), &serde_json::json!("0"), &serde_json::json!("1"), &serde_json::json!([])), "{}", v);
}

/// A local or a parameter named as a builtin that runs its callback later (`every`) is the local:
/// the lambda given to it runs as the draw does, and the render prop it calls is the draw's (the
/// seventeenth review of slice C; FEEDBACK88.md, Q-1380; branch feedback86-89). Before the fix the
/// card was never drawn once `pg` was set.
const S_LETV: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
view Inner(only: any) {
    let every = (n: int, f: any) => f()
    <Level level={every(1, () => only())}/>
}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Inner only={() => pg}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;
const S_PAR: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>
fn nothing() -> any = null
state pg = nothing()
state hits = 0
view Inner(only: any) {
    <Level level={[(n: int, f: any) => f()].map((every) => every(1, () => only()))[0]}/>
}
component P() {
    state k = 0

    <div #p>{k >= 0 ? <span><b>{js("requestAnimationFrame", () => { hits += 1 })}</b>{[1].map((i) => <Inner only={() => pg}/>)}</span> : null}</div>
}
component Box() {
    state on = false
    <div><i #h>{hits}</i><button #on onclick={() => { on = true }}>on</button><button #set onclick={() => { pg = "hard" }}>set</button>{on ? <P/> : null}</div>
}
route GET /*rest = <html><body><Box/></body></html>
"##;

#[test]
fn a_local_named_as_a_builtin_is_the_local() {
    for (name, src) in [("s-letv", S_LETV), ("s-par", S_PAR)] {
        let Some((v, _)) = in_chrome(
            name,
            src,
            r##"
  const [w, d] = await open("/");
  d.getElementById("on").click(); await tick(800);
  const h1 = +d.getElementById("h").textContent;
  d.getElementById("set").click(); await tick(800);
  return { h1, lvl: d.querySelector("#p .lvl") && d.querySelector("#p .lvl").textContent };
"##,
        ) else {
            return;
        };
        assert_eq!(v["lvl"], serde_json::json!("hard"), "{}: {}", name, v);
        assert!(v["h1"].as_i64().unwrap_or(99) <= 3, "{}: {}", name, v);
    }
}

/// A stale failure file over fields no one touched: a range with no value (it shows its middle)
/// and an empty `<select>` load the page again; a range moved does not (the seventeenth review of
/// slice C; FEEDBACK88.md, Q-1380; branch feedback86-89). Before the fix both untouched ones were
/// taken for typed and the page stayed.
const R404_RANGE: &str = r##"view Level(level: str) = <span class="lvl">{level}</span>

component Card(t: map?) {
    <section #card><Level level={t.level}/></section>
}

component Lab() {
    state t = null
    state card = false
    <div>
        <input #name type="range"/><select #empty></select>
        <button #good onclick={() => { t = { level: "easy" } }}>good</button>
        <button #card-on onclick={() => { card = !card }}>card</button>
        {card ? <Card t={t}/> : null}
    </div>
}

route GET /lab = <html><body><Lab/></body></html>
"##;

#[test]
fn a_stale_failure_file_reloads_over_an_untouched_range_or_empty_select() {
    for (case, change) in [("untouched", ""), ("moved", r#"d.getElementById("name").value = "80";"#)] {
        let Some((v, _)) = in_chrome(
            &format!("r404-range-{}", case),
            R404_RANGE,
            &r##"
  const [w, d] = await open("/lab");
  const f = document.querySelector("iframe");
  let loads = 0;
  f.addEventListener("load", () => { loads++; });
  const add = d.head.appendChild.bind(d.head), get = w.fetch.bind(w);
  d.head.appendChild = (el) => { if (el.src && /\/_quillo\/r\/f/.test(el.src)) el.src = el.src.replace(/\/f[0-9a-f]{16}\.js/, "/f0000000000000000.js"); return add(el); };
  w.fetch = (u, o) => o && o.method === "HEAD" && String(u).includes("/_quillo/r/f") ? get(String(u).replace(/\/f[0-9a-f]{16}\.js/, "/f0000000000000000.js"), o) : get(u, o);
  @CHANGE@
  d.querySelector("#card-on").click();
  await wait(() => loads > 0, 3000);
  return { loads };
"##
            .replace("@CHANGE@", change),
        ) else {
            return;
        };
        assert_eq!(v["loads"], serde_json::json!(if case == "untouched" { 1 } else { 0 }), "{}: {}", case, v);
    }
}

/// A program whose string reads like a reference to another component's facts (`{ r: zz.$f.r, w: `):
/// a debug build serves it, as the check of those references goes by what was written, not by the
/// text (the eighteenth review of slice C; FEEDBACK88.md, Q-1380; branch feedback86-89). Before the
/// fix the debug build panicked before it listened.
const DBG_STR: &str = r##"state pg = 0
component A() {
    <p #a>{"{ r: zz.$f.r, w: " + str(pg)}</p>
}
route GET / = <html><body><A/></body></html>
"##;

#[test]
fn a_string_like_a_facts_reference_is_only_a_string() {
    let Some((v, _)) = in_chrome(
        "dbg-str",
        DBG_STR,
        r##"
  const [w, d] = await open("/");
  return { a: d.getElementById("a") && d.getElementById("a").textContent };
"##,
    ) else {
        return;
    };
    assert_eq!((&v["a"], &v["errors"]), (&serde_json::json!("{ r: zz.$f.r, w: 0"), &serde_json::json!([])), "{}", v);
}
