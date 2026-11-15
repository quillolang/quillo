// FEEDBACK68.md: the page's state, `state cart = []` at the top of a program, shared by every
// component on the page, one value for each page made (branch global-state).

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn dir(name: &str, src: &str) -> PathBuf {
    let d = support::temp_dir(&format!("f68-{}", name));
    std::fs::write(d.join("main.qwe"), src).unwrap();
    d
}

fn check(d: &Path) -> (bool, String) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_quillo")).arg("check").arg(d.join("main.qwe")).output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
}

fn get(port: u16, path: &str) -> String {
    support::send(port, "GET", path, &[], "").body
}

// Each request for a page has its own, from its initializer, in order; a route sets it before
// its markup; a top-level `let` stays the server's, one for all.
#[test]
fn each_page_has_its_own_and_a_route_sets_where_it_starts() {
    let d = dir("own", "state visits = 0\nstate cart = []\nstate label = \"items: \" + str(len(cart))\nlet total = 0\nview Badge() = <span id=\"b\">{len(cart)}</span>\nroute GET /a {\n    visits += 1\n    total += 1\n    `${visits} ${total}`\n}\nroute GET /:n(int) {\n    cart = range(0, n)\n    <html><body><Badge/><p>{label}</p></body></html>\n}\n");
    assert!(check(&d).0, "{}", check(&d).1);
    let s = support::serve(&d.join("main.qwe"), &[]);
    assert_eq!((get(s.port, "/a"), get(s.port, "/a")), ("1 1".to_string(), "1 2".to_string()));
    // A cached view that reads it: each page its own (Q-963).
    for n in [3, 5, 3, 0] {
        let page = get(s.port, &format!("/{}", n));
        assert!(page.contains(&format!("<span id=\"b\">{}</span>", n)), "{}: {}", n, page);
        assert!(page.contains("<p>items: 0</p>"), "an initializer runs before the route: {}", page);
    }
}

// A `server fn` runs for no page: `quillo check` refuses it, and so does a run that skipped the
// check; a timer that reads one says so too. Two of one name, or one and a `let`, are refused.
#[test]
fn what_runs_for_no_page_is_told_so() {
    let src = "state cart = [1]\nserver fn how_many() = len(cart)\nroute GET / = \"hi\"\n";
    let d = dir("server-fn", src);
    let (ok, said) = check(&d);
    assert!(!ok && said.contains("`cart` is the page's state, and server fn `how_many` runs for no page: pass it as an argument"), "{}", said);
    let s = support::serve_with(&d.join("main.qwe"), &["--no-check"]);
    let a = support::send(s.port, "POST", "/_quillo/rpc/how_many", &[("content-type", "application/json"), ("origin", "http://localhost")], "[]");
    assert_eq!(a.status, 500, "{}", a.body);
    drop(s);

    for (src, line) in [("state n = 0\nstate n = 1\nroute GET / = str(n)\n", 2), ("let m = 2\nstate m = 3\nroute GET / = str(m)\n", 2), ("state m = 3\nfn m() = 1\nroute GET / = str(m)\n", 2)] {
        let (ok, said) = check(&dir("twice", src));
        assert!(!ok && said.contains(&format!("main.qwe:{}: error:", line)) && said.contains("is defined twice"), "{}: {}", src, said);
    }

    // At run time, outside a request for a page: a timer.
    let d = dir("timer", "state cart = [1]\nlet seen = \"\"\nfn tick() {\n    seen = str(len(cart))\n}\nevery(20, tick)\nroute GET / = seen\n");
    let s = support::serve_with(&d.join("main.qwe"), &["--no-check"]);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(get(s.port, "/"), "", "the timer read no page's cart");
}

// The page carries its states as JSON in a string, where `</script>` cannot end the script; a
// program with none carries nothing more.
#[test]
fn the_page_carries_its_states_and_a_program_with_none_nothing() {
    let d = dir("carried", "state note = \"</script><b>x</b><!--\"\ncomponent C() = <p id=\"p\">{note}</p>\nroute GET / = <html><body><div><C/></div></body></html>\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = get(s.port, "/");
    let at = page.find(".g=").unwrap_or_else(|| panic!("{}", page));
    let script = &page[at..at + page[at..].find("</script>").unwrap()];
    assert!(!script.contains("</") && !script.contains("<!--"), "{}", script);
    assert!(page.contains("\" data-e"), "a component that read one starts with the page: {}", page);
    drop(s);
    let d = dir("none", "component C() {\n    state n = 0\n    <p>{n}</p>\n}\nroute GET / = <html><body><div><C/></div></body></html>\n");
    let s = support::serve(&d.join("main.qwe"), &[]);
    let page = get(s.port, "/");
    assert!(!page.contains(".g=") && !page.contains("\" data-e"), "{}", page);
}

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

fn in_chrome(chrome: &Path, url: &str, script: &str) -> serde_json::Value {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let d = support::temp_dir(&format!("f68-chrome-{}", script.len()));
    std::fs::write(d.join("t.js"), script).unwrap();
    let out = std::process::Command::new("node").arg(root.join("browser/run.js")).arg(chrome).arg(url).arg(d.join("profile")).arg("30000").arg(d.join("t.js")).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

const SHOP: &str = r#"state cart = []
state shown = false

component Add(item: int) {
    <button id={`add${item}`} onclick={() => { cart = cart + [item] }}>Add {item}</button>
}

component Badge() {
    <span id="badge">{len(cart)}</span>
}

component Later() {
    <div>
        <button id="show" onclick={() => { shown = true }}>show</button>
        {shown && <p id="late">{len(cart)} in the cart</p>}
    </div>
}

route GET / {
    cart = [7]
    <html><body><header><Badge/></header><main><Add item={1}/><Add item={2}/></main><aside><Later/></aside></body></html>
}
"#;

const SHOP_SCRIPT: &str = r#"(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (id) => document.getElementById(id);
  const first = $("badge").textContent;
  for (let i = 0; i < 300 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
  $("add1").click();
  for (let i = 0; i < 100 && $("badge").textContent === first; i++) await tick(30);
  const after1 = $("badge").textContent;
  $("add2").click();
  for (let i = 0; i < 100 && $("badge").textContent === after1; i++) await tick(30);
  $("show").click();
  for (let i = 0; i < 100 && !$("late"); i++) await tick(30);
  const QW = window[Symbol.for("quillo")];
  document.body.dataset.result = JSON.stringify({ first, after1, after2: $("badge").textContent, late: ($("late") || {}).textContent, eager: document.querySelector("header quillo-island").hasAttribute("data-e"), addEager: document.querySelector("main quillo-island").hasAttribute("data-e"), remade: QW.remade ?? 0, errors: window.__quilloErrors });
})();"#;

// Q-961: two components placed apart share the cart: the server's start (7) shows, an Add that
// waited for its click sets it, the Badge that started with the page follows, and a part made
// in the browser later reads it. In development (each file of its own) too.
#[test]
fn components_placed_apart_share_the_pages_state() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: no Chrome");
        return;
    };
    for opts in [&[][..], &["--watch"][..]] {
        let d = dir(&format!("shop{}", opts.len()), SHOP);
        let s = support::serve_with(&d.join("main.qwe"), opts);
        let r = in_chrome(&chrome, &format!("http://127.0.0.1:{}/", s.port), SHOP_SCRIPT);
        assert_eq!(
            r,
            serde_json::json!({ "first": "1", "after1": "2", "after2": "3", "late": "3 in the cart", "eager": true, "addEager": false, "remade": 0, "errors": [] }),
            "{:?}",
            opts
        );
    }
}

#[allow(dead_code)]
fn until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}
