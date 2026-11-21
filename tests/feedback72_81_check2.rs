//! FEEDBACK72.md, FEEDBACK73.md and FEEDBACK79.md: what `quillo check` says of markup, links and
//! habits from other languages (slice C2; branch feedback72-81).

mod support;

use quillo::check::{check_file_opts, check_source, Diagnostic, Options, Severity};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
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
        let d = temp_dir(&format!("feedback72-81-check2-{}", name));
        for (f, text) in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        App(d.canonicalize().unwrap())
    }

    /// Each diagnostic of `quillo check file` (style sheets read, as the command reads them),
    /// with its file's name.
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

// ---- Q-1251: a link to an address no route answers ---------------------------------------------

// (FEEDBACK79.md, Q-1251; branch feedback72-81)
#[test]
fn q1251_a_link_no_route_answers_is_said_with_the_route_near_it() {
    says(
        "route GET / = <ul>{for id in [1, 2] => <li><a href={`/trail/${id}`}>trail {id}</a></li>}</ul>\nroute GET /trails/:id(int) = `trail ${id}`\n",
        &["1 warning: no route answers `/trail/…`, so this link is a 404 (did you mean `/trails/…`?)"],
    );
}

// (FEEDBACK79.md, Q-1251; branch feedback72-81)
#[test]
fn q1251_what_a_route_answers_and_what_is_not_the_programs_is_left_alone() {
    let src = r##"let url = "/nowhere"
let base = "/nowhere"
route GET / = <nav>
    <a href="/">home</a>
    <a href="/about">about</a>
    <a href="/about/">about, with a slash</a>
    <a href="/about?x=1#top">about, a query and a hash</a>
    <a href={`/search?q=${url}`}>a query from a value</a>
    <a href="/trails/12">a trail</a>
    <a href="/trails/-3">a trail below zero</a>
    <a href={`/trails/${7}`}>a trail from a value</a>
    <a href={"/trails/" + str(7)}>a trail joined</a>
    <a href="https://example.com/x">away</a>
    <a href="//cdn.example/x">away, no scheme</a>
    <a href="mailto:a@b.c">mail</a>
    <a href="#top">top</a>
    <a href="about">relative</a>
    <a href="">empty</a>
    <a href="/_quillo/metrics">Quillo's own</a>
    <a href={url}>a value</a>
    <a href={`${base}/x`}>a value first</a>
    <a href="/@ada">a profile</a>
    <link rel="stylesheet" href="/caf%C3%A9"/>
</nav>
route GET /about = "about"
route GET /search ? { q: str = "" } = q
route GET /trails/:id(int) = `trail ${id}`
route GET /café = "café"
route GET /@:name = name
"##;
    says(src, &[]);
}

// (FEEDBACK79.md, Q-1251; branch feedback72-81)
#[test]
fn q1251_what_no_route_takes() {
    let src = r#"route GET / = <nav>
    <a href="/trails/abc">not a number</a>
    <a href={"/trail/" + str(1)}>joined</a>
    <a href="/ada">no @</a>
    <a href="/@">only the @</a>
    <a href="/about/more">deeper</a>
</nav>
route GET /about = "about"
route GET /trails/:id(int) = `trail ${id}`
route GET /@:name = name
"#;
    says(
        src,
        &[
            "2 warning: no route answers `/trails/abc`, so this link is a 404 (did you mean `/trails/:id(int)`?)",
            "3 warning: no route answers `/trail/…`, so this link is a 404 (did you mean `/trails/…`?)",
            "4 warning: no route answers `/ada`, so this link is a 404 (did you mean `/@ada`?)",
            "5 warning: no route answers `/@`, so this link is a 404",
            "6 warning: no route answers `/about/more`, so this link is a 404",
        ],
    );
}

// The suggestion: an edit for an address of up to seven characters, none past it; unicode counted
// by characters. (FEEDBACK79.md, Q-1251; branch feedback72-81)
#[test]
fn q1251_did_you_mean_its_limit_and_one_either_side() {
    let src = "route GET / = <nav><a href=\"/abcut\">a</a><a href=\"/abxyt\">b</a><a href=\"/cafe\">c</a><a href=\"/about-us\">d</a></nav>\nroute GET /about = \"about\"\nroute GET /café = \"café\"\nroute GET /about-them = \"them\"\n";
    let got = diags(src);
    assert_eq!(
        got,
        [
            "1 warning: no route answers `/abcut`, so this link is a 404 (did you mean `/about`?)",
            "1 warning: no route answers `/abxyt`, so this link is a 404",
            "1 warning: no route answers `/cafe`, so this link is a 404 (did you mean `/café`?)",
            "1 warning: no route answers `/about-us`, so this link is a 404",
        ],
    );
    // Eight characters and more: two edits.
    says(
        "route GET / = <a href=\"/about-thim\">a</a>\nroute GET /about-them = \"them\"\n",
        &["1 warning: no route answers `/about-thim`, so this link is a 404 (did you mean `/about-them`?)"],
    );
}

// (FEEDBACK79.md, Q-1251; branch feedback72-81)
#[test]
fn q1251_redirect_navigate_and_a_route_of_another_method() {
    let src = r#"route POST /notes = redirect("/notes")
route GET /old = redirect("/gone?from=old", 301)
route GET /fine = redirect("/old")
component Go() {
    <button onclick={() => { navigate("/lost") }}>go</button>
}
route GET / = <html><body><Go/><a href="/notes">notes</a></body></html>
"#;
    says(
        src,
        &[
            "1 warning: only `POST /notes` answers `/notes`, and a redirect is followed with GET, so it gets a 405",
            "2 warning: no route answers `/gone`, so this redirect ends at a 404",
            "5 warning: no route answers `/lost`, so this `navigate` ends at a 404",
            "7 warning: only `POST /notes` answers `/notes`, and a link asks with GET, so it gets a 405",
        ],
    );
    // A 307 or a 308 is followed with the request's own method (found in review).
    let src = "route POST /old = redirect(\"/api/orders\", 308)\nroute PUT /was = redirect(\"/api/orders\", 307)\nroute POST /api/orders = \"ok\"\nroute POST /lost = redirect(\"/nowhere\", 308)\nroute POST /then = redirect(\"/api/orders\", 303)\n";
    says(
        src,
        &[
            "4 warning: no route answers `/nowhere`, so this redirect ends at a 404",
            "5 warning: only `POST /api/orders` answers `/api/orders`, and a redirect is followed with GET, so it gets a 405",
        ],
    );
}

// A view's or component's `href` is a prop, which may be anything; an element's is a link.
// (FEEDBACK79.md, Q-1251, found in review; branch feedback72-81)
#[test]
fn q1251_a_props_href_is_no_link() {
    let src = r#"view NavLink(href, label) = <a href={href}>{label}</a>
component Pagination(href: str) {
    state page = 1
    <nav><a href={href}>{page}</a></nav>
}
route GET / = <html><body><NavLink href="/inbox" label="Inbox"/><Pagination href="/posts?page=2"/><a href="/inbox">inbox</a></body></html>
"#;
    says(src, &["6 warning: no route answers `/inbox`, so this link is a 404"]);
}

// Many routes and links, and many dead ones: each link is tried against the routes of its first
// piece, and the suggestions have a budget; what is said stays the same.
// (FEEDBACK79.md, Q-1251, found in review; branch feedback72-81)
#[test]
fn q1251_many_routes_and_links() {
    let mut src = String::new();
    for i in 0..600 {
        src.push_str(&format!("route GET /r{}/:id(int) = id\n", i));
    }
    let links: String = (0..1800).map(|i| format!("<a href=\"/r{}/{}\">a</a>", i % 600, i)).collect();
    let dead: String = (0..600).map(|i| format!("<a href=\"/x{}/{}\">a</a>", i, i)).collect();
    src.push_str(&format!("route GET / = <nav>{}</nav>\nroute GET /near = <a href=\"/r7x/1\">a</a>\nroute GET /dead = <nav>{}</nav>\n", links, dead));
    let d = diags(&src);
    assert_eq!(d.len(), 601, "{:?}", &d[..d.len().min(3)]);
    assert_eq!(d[0], "602 warning: no route answers `/r7x/1`, so this link is a 404 (did you mean `/r7/1`?)");
    assert_eq!(d.iter().filter(|m| m.starts_with("603 warning: no route answers `/x")).count(), 600, "{:?}", &d[..d.len().min(3)]);
}

// A route that takes the rest, a program's `redirect` of its own, a file of views, and a mounted
// program's prefix. (FEEDBACK79.md, Q-1251; branch feedback72-81)
#[test]
fn q1251_what_takes_any_address_and_programs_with_none() {
    says("route GET /x = <a href=\"/anything/at/all\">a</a>\nroute GET /*path = files(\"public\", path)\n", &[]);
    says("route GET /assets/*path = files(\"public\", path)\nroute GET / = <a href=\"/assets/app.css\">a</a>\n", &[]);
    says("route GET /x = <a href=\"/about\">a</a>\nroute GET /:page = page\n", &[]);
    says("route GET / {\n    let redirect = (to) => to\n    redirect(\"/nowhere\")\n}\n", &[]);
    says("view Nav() = <a href=\"/nowhere\">nowhere</a>\n", &[]);
    // A program with no `/` is part of a site whose home is elsewhere, or a fragment.
    says("route GET /out = redirect(\"/\")\nroute GET /x = <a href=\"/?from=x\">home</a>\n", &[]);
    let app = App::new(
        "mount",
        &[
            ("main.qwe", "mount /admin = \"admin.qwe\"\nroute GET / = <nav><a href=\"/admin/users\">users</a><a href=\"/admins\">near</a></nav>\n"),
            ("admin.qwe", "route GET /users = <a href=\"/\">home</a>\n"),
        ],
    );
    // The mounted program's own link to the main program's `/` is not said: it is checked inside.
    assert_eq!(app.check("main.qwe"), ["main.qwe:2 warning: no route answers `/admins`, so this link is a 404"]);
}

// ---- Q-1012 and Q-1022: the sheets under public/, and what its pages write ---------------------

// (FEEDBACK72.md, Q-1012; branch feedback72-81)
#[test]
fn q1012_a_script_beside_an_app_is_not_told_of_the_apps_classes() {
    let app = App::new(
        "q1012",
        &[
            ("main.qwe", "route GET / = <html><head><link rel=\"stylesheet\" href=\"/style.css\"/></head><body><p.card>hi</p></body></html>\nroute GET /*path = files(\"public\", path)\n"),
            ("tool.qwe", "print(len(read_file(\"public/style.css\") ?? \"\"))\n"),
            ("api.qwe", "route GET /n = { n: 1 }\n"),
            ("public/style.css", ".card { color: red; }\n.gone { color: blue; }\n"),
        ],
    );
    assert_eq!(app.check("main.qwe"), ["public/style.css:2 warning: `.gone` is styled here, but nothing in the program writes it"]);
    assert_eq!(app.check("tool.qwe"), Vec::<String>::new());
    // Routes with no markup, component or file to send serve no page.
    assert_eq!(app.check("api.qwe"), Vec::<String>::new());
    // A file of views checked alone still sees the sheets' rules: only "styled here, but nothing
    // writes it" waits for a program that serves pages (found in review).
    let app = App::new(
        "q1012-views",
        &[("views.qwe", "css(\".cart { a: b }\")\nview V() = <p class=\"cart\"><b.card>x</b></p>\n"), ("public/app.css", ".card { color: red; }\n.lonely { }\n")],
    );
    assert_eq!(app.check("views.qwe"), Vec::<String>::new());
    let opts = Options { classes: true, ..Default::default() };
    let hints: Vec<String> = check_file_opts(&app.0.join("views.qwe"), HashMap::new(), opts).diagnostics.iter().map(shown).collect();
    assert_eq!(hints, Vec::<String>::new());
}

// (FEEDBACK73.md, Q-1022; branch feedback72-81)
#[test]
fn q1022_the_classes_public_pages_and_served_folders_write_are_written() {
    let app = App::new(
        "q1022",
        &[
            ("main.qwe", "route GET /site/*path = files(\"./site/\", path)\nroute GET /*path = files(\"public\", path)\n"),
            (
                "public/index.html",
                "<link rel=\"stylesheet\" href=\"style.css\"><h1 class=\"hero  big\">Hi</h1><p class='single'>a</p><p class=bare>b</p><p data-class=\"nope\">c</p><p class=\"café\">d</p>\n<script>document.body.classList.add(\"from-script\")</script>\n",
            ),
            ("site/deep/page.htm", "<div class=\"served\">x</div>\n"),
            ("site/node_modules/x/page.html", "<div class=\"hidden-dep\">x</div>\n"),
            ("public/style.css", ".hero { color: tomato; }\n.big { }\n.single { }\n.bare { }\n.nope { }\n.café { }\n.from-script { }\n.served { }\n.hidden-dep { }\n.card { padding: 1em; }\n"),
        ],
    );
    assert_eq!(
        app.check("main.qwe"),
        [
            "public/style.css:5 warning: `.nope` is styled here, but nothing in the program writes it",
            "public/style.css:9 warning: `.hidden-dep` is styled here, but nothing in the program writes it",
            "public/style.css:10 warning: `.card` is styled here, but nothing in the program writes it",
        ]
    );
}

// A folder that is a link is followed only inside the program's folder, once: a link out of it
// is not read, and links to `.` make no loop, under public/ and a folder `files()` serves alike.
// (FEEDBACK73.md, Q-1022, found in review; branch feedback72-81)
#[cfg(unix)]
#[test]
fn q1022_links_to_folders_stay_inside_the_program() {
    use std::os::unix::fs::symlink;
    let outside = App::new("q1022-outside", &[("page.html", "<p class=\"outside\">x</p>\n")]);
    let app = App::new(
        "q1022-links",
        &[
            ("main.qwe", "route GET /site/*path = files(\"site\", path)\nroute GET /loops/*path = files(\"loops\", path)\nroute GET /*path = files(\"public\", path)\n"),
            ("loops/page.html", "<p class=\"inside\">x</p>\n"),
            ("public/pub.html", "<p class=\"pub\">x</p>\n"),
            ("public/style.css", ".outside { }\n.inside { }\n.pub { }\n"),
        ],
    );
    symlink(&outside.0, app.0.join("site")).unwrap();
    symlink(&outside.0, app.0.join("public/ext")).unwrap();
    for i in 0..5 {
        symlink(".", app.0.join(format!("loops/s{}", i))).unwrap();
    }
    assert_eq!(app.check("main.qwe"), ["public/style.css:1 warning: `.outside` is styled here, but nothing in the program writes it"]);
}

// ---- Q-1016: `pages // 2` ------------------------------------------------------------------------

// Right after a name, a whole number or a name the program has. The test once had
// `floor(pages / 2) // 2.5` warn: after `)` a comment says what the line gives, as the guide's
// do (decided in review). (FEEDBACK72.md, Q-1016; branch feedback72-81)
#[test]
fn q1016_a_comment_holding_a_number_or_a_name_after_a_name() {
    let src = "let pages = 9\nlet half = pages // 2\nlet n = 3\nlet third = pages // n\nlet tenth = pages//10\nlet box = { count: 9 }\nlet c = box.count // 3\nprint(half, third, tenth, c, `${pages // 4}`)\n";
    let w = |line: u32, word: &str| format!("{} warning: `//` starts a comment here, so `// {}` is left out: whole-number division is `floor(a / b)`", line, word);
    says(src, &[&w(2, "2"), &w(4, "n"), &w(5, "10"), &w(7, "3"), &w(8, "4")]);
}

// What a line gives, written after it: after `)`, `]`, a string or a number, a decimal, and the
// value of a name given a whole number. (FEEDBACK72.md, Q-1016; branch feedback72-81)
#[test]
fn q1016_comments_that_say_a_result_are_left_alone() {
    let src = r#"fn f(x) = x * 2
let xs = [1, 2] // 2
let hour = 60 * 60 // 3600
let pages = 7
let total = pages // 7
let price = 19.99
let cost = price // 19.99
let size = len(xs) // xs
let name = "Ada" // Ada
print(f(2)) // 4
print(xs, hour, total, cost, size, name)
"#;
    says(src, &[]);
}

// (FEEDBACK72.md, Q-1016; branch feedback72-81)
#[test]
fn q1016_comments_of_words_and_comments_of_their_own_are_left_alone() {
    let src = "// 2\nlet n = 7 // pages in the book\nlet timeout = 30 // seconds\nlet m = 2 // TODO\nlet é = 1 // été\n/// 2\nfn f(x) = x\nprint(n, timeout, m, é, f(1)) ///\n    // n\nlet k = n // seconds\nlet j = n // TODO\nprint(k, j)\n";
    says(src, &[]);
}

// A parser's panic inside the note-taking leaves the thread as it was: nothing more is noted.
// (FEEDBACK72.md, Q-1016, found in review; branch feedback72-81)
#[test]
fn q1016_a_panic_while_noting_stops_the_noting() {
    let r = std::panic::catch_unwind(|| quillo::lexer::noting_value_comments(|| -> () { panic!("a parser's panic") }));
    assert!(r.is_err());
    // Outside the noting a comment is read as any other, and the next noting starts empty.
    let mut p = quillo::parser::Parser::new("let pages = 9\nlet half = pages // 2\n");
    assert!(p.parse_program().is_ok());
    let (_, noted) = quillo::lexer::noting_value_comments(|| quillo::parser::Parser::new("let a = 1\n").parse_program().is_ok());
    assert!(noted.is_empty(), "{:?}", noted);
    let (_, noted) = quillo::lexer::noting_value_comments(|| quillo::parser::Parser::new("let a = 1\nlet b = a // 2\n").parse_program().is_ok());
    assert_eq!(noted, [(2, "2".to_string(), "a".to_string())]);
}

// ---- Q-1017: CSS built from values ------------------------------------------------------------------

const STYLE_BAD: &str = "`<style>` with CSS built from values: a value containing `}` can add rules (CSS injection); write `` css`...` ``, which writes each `${...}` as one CSS value";
const STYLE_ATTR_BAD: &str = "`style` built from values with `+`: a value containing `;` can add declarations (CSS injection); write a style map, `style={{ color: c }}`, whose values are written as CSS values";

// (FEEDBACK72.md, Q-1017; branch feedback72-81)
#[test]
fn q1017_a_style_or_a_style_attribute_built_from_a_value() {
    let src = "route GET /b ? { c: str = \"red\" } = <html><body><style>{`p { color: ${c} }`}</style><p>hi</p></body></html>\nroute GET /c ? { c: str = \"red\" } = <html><body><style>{\"p { color: \" + c + \" }\"}</style><p style={\"color: \" + c}>hi</p></body></html>\n";
    says(src, &[&format!("1 warning: {}", STYLE_BAD), &format!("2 warning: {}", STYLE_BAD), &format!("2 warning: {}", STYLE_ATTR_BAD)]);
}

// (FEEDBACK72.md, Q-1017; branch feedback72-81)
#[test]
fn q1017_what_holds_no_visitors_text_is_left_alone() {
    let src = r#"let theme = css`body { color: navy }`
let kit = theme
let accent = "tomato"
let gap = "4px"
let page_css = kit + css`p { margin: 0 }`
route GET /a ? { c: str = "red", n: int = 2 } = <html><head>
    <style>{css`p { color: ${c} }`}</style>
    <style>{`${theme} ${page_css} a { color: ${accent} }`}</style>
    <style>{kit + page_css}</style>
    <style>{`p { margin: ${n}px }`}</style>
    <style>{"p { color: red }"}</style>
</head><body><p style={`color: ${c}`}>a</p><p style={{ color: c }}>b</p><p style={"margin: " + gap}>c</p>
    <p style={"width: " + str(n) + "%"}>d</p><p style={"width: " + format_number(n) + "px"}>e</p><p style={"width: " + n + "px"}>f</p>
</body></html>
"#;
    says(src, &[]);
    // Text from a value, or of no known type, still is.
    let src = "fn w(x) = x\nroute GET /b ? { c: str = \"1\" } = <html><body><p style={\"width: \" + str(c)}>a</p><p style={\"width: \" + w(c)}>b</p></body></html>\n";
    says(src, &[&format!("2 warning: {}", STYLE_ATTR_BAD)]);
}

// A long chain of `+`, as deep as the parser takes (256 levels), read without recursion.
// (FEEDBACK72.md, Q-1017; branch feedback72-81)
#[test]
fn q1017_a_long_chain() {
    let chain: Vec<String> = (0..250).map(|i| format!("\"p{} {{}}\"", i)).collect();
    let src = format!("route GET / ? {{ c: str = \"red\" }} = <html><body><style>{{{} + c}}</style></body></html>\n", chain.join(" + "));
    assert_eq!(about(&src, "CSS injection"), [format!("1 warning: {}", STYLE_BAD)]);
}

// ---- Q-1019: what the keyboard cannot reach ------------------------------------------------------

// (FEEDBACK72.md, Q-1019; branch feedback72-81)
#[test]
fn q1019_a_click_on_what_no_key_reaches_and_an_image_with_no_alt() {
    let src = r#"component Signup() {
    state agreed = false
    state sent = false
    <section>
        <span class={{ box: true, on: agreed }} onclick={() => { agreed = !agreed }}></span> I agree
        <div class="next" onclick={() => { sent = agreed }}>Join</div>
        <img src="/logo.png"/>
        <li onclick={() => { sent = true }}>a</li>
        <h2 @click={() => { sent = true }}>b</h2>
        {sent && <p>Welcome!</p>}
    </section>
}
route GET / = <html><body><Signup/></body></html>
"#;
    says(
        src,
        &[
            "5 warning: a `<span>` with onclick can't be reached with the keyboard: use a `<button>`",
            "6 warning: a `<div>` with onclick can't be reached with the keyboard: use a `<button>`",
            "7 warning: this `<img>` has no `alt`: `alt=\"\"` if it is decoration, or words for what it shows",
            "8 warning: a `<li>` with onclick can't be reached with the keyboard: use a `<button>`",
            "9 warning: an `<h2>` with onclick can't be reached with the keyboard: use a `<button>`",
        ],
    );
}

// (FEEDBACK72.md, Q-1019; branch feedback72-81)
#[test]
fn q1019_what_a_keyboard_or_a_reader_reaches_is_left_alone() {
    let src = r#"component Box() {
    state n = 0
    state shade = null
    <section>
        <button onclick={() => { n += 1 }}>a</button>
        <a href="/" onclick={() => { n += 1 }}>b</a>
        <div role="button" tabindex="0" onclick={() => { n += 1 }}>c</div>
        <div tabindex="0" onclick={() => { n += 1 }}>d</div>
        <div role="presentation" onclick={() => { n += 1 }}>e</div>
        <div onmount={(el) => { shade = el }} onclick={(e) => { if e.target == shade { n = 0 } }}>f</div>
        <ul onclick={(e) => { n = len(e.currentTarget.children) }}><li>g</li></ul>
        <div aria-hidden="true" onclick={() => { n += 1 }}>h</div>
        <div onkeydown={(e) => { n += 1 }} onclick={() => { n += 1 }}>i</div>
        <div contenteditable="true" onclick={() => { n += 1 }}>j</div>
        <label onclick={() => { n += 1 }}>k</label>
        <img src="/a.png" alt=""/>
        <img src="/b.png" alt="A kite"/>
        <img src="/c.png" aria-hidden="true"/>
        <img src="/d.png" {...{ alt: "spread" }}/>
        <div onkeydown={(e) => { if e.key == "Escape" { n = 0 } }}><div class="shade" onclick={() => { n = 0 }}></div></div>
        <table tabindex="0"><tr onclick={() => { n += 1 }}><td>row</td></tr></table>
        <div role="listbox">{n > 0 && <span onclick={() => { n = 1 }}>opt</span>}</div>
        <p>{n}</p>
    </section>
}
route GET / = <html><body><Box/></body></html>
"#;
    says(src, &[]);
}

// (FEEDBACK72.md, Q-1019; branch feedback72-81)
#[test]
fn q1019_a_sheet_that_takes_the_focus_ring_away() {
    let page = "route GET / = <html><head><link rel=\"stylesheet\" href=\"/s.css\"/></head><body><button.b>a</button></body></html>\nroute GET /*path = files(\"public\", path)\n";
    let ring = "warning: `outline: none` on `:focus` takes the keyboard's ring away everywhere: style `:focus-visible` instead";
    let app = App::new("q1019-ring", &[("main.qwe", page), ("public/s.css", ".b { color: red }\n/* :focus { outline: none } */\n\nbutton:focus,\n  a:focus { outline: none; }\n:focus-within { outline: 0 }\n@media (min-width: 1px) { .b:focus { OUTLINE: 0 !important } }\n")]);
    assert_eq!(app.check("main.qwe"), [format!("public/s.css:4 {}", ring), format!("public/s.css:7 {}", ring)]);
    // A `:focus-visible` rule anywhere: the ring is shown where the keyboard is.
    let app = App::new("q1019-visible", &[("main.qwe", page), ("public/s.css", ".b { color: red }\n.b:focus { outline: none }\n"), ("public/t.css", ".b:focus-visible { outline: 2px solid }\n")]);
    assert_eq!(app.check("main.qwe"), Vec::<String>::new());
    // And in the program's own sheet, at its line.
    let src = "let s = css`\n.b { color: red }\n.b:focus { outline: 0px }\n`\nroute GET / = <html><head><style>{s}</style></head><body><button.b>a</button></body></html>\n";
    let app = App::new("q1019-own", &[("main.qwe", src)]);
    assert_eq!(app.check("main.qwe"), [format!("main.qwe:3 {}", ring)]);
    // A ring of the rule's own in place of the outline (found in review); `box-shadow: none` is
    // none.
    let sheet = ".b { color: red }\nbutton:focus { outline: none; box-shadow: 0 0 0 3px gold }\na:focus { outline: 0; border: 2px solid }\ninput:focus { outline: none; outline-offset: 2px }\nselect:focus { outline: none; box-shadow: none }\n";
    let app = App::new("q1019-replaced", &[("main.qwe", page), ("public/s.css", sheet)]);
    assert_eq!(app.check("main.qwe"), [format!("public/s.css:5 {}", ring)]);
    // A script kept beside the app is not told of the app's sheets.
    let app = App::new("q1019-script", &[("tool.qwe", "print(1)\n"), ("public/s.css", ".b:focus { outline: none }\n")]);
    assert_eq!(app.check("tool.qwe"), Vec::<String>::new());
}

// ---- Q-1031 and Q-1032: Vue's and Svelte's habits ------------------------------------------------

// (FEEDBACK72.md, Q-1031; branch feedback72-81)
#[test]
fn q1031_modifiers_v_model_and_v_on() {
    let src = r#"component Order() {
    state n = 0
    state name = ""
    state ok = false
    <div>
        <button @click.prevent={() => { n += 1 }}>a</button>
        <button @click.stop.prevent={() => { n += 1 }}>b</button>
        <button @click.once={() => { n += 1 }}>c</button>
        <input v-model={name}/>
        <input type="checkbox" v-model={ok}/>
        <input v-model={n + 1}/>
        <button v-on:click.prevent={() => { n += 1 }}>d</button>
        <form @submit.prevent={() => { n += 1 }}><button>e</button></form>
        <button @click={() => { n += 1 }}>{n}{name}{ok}</button>
        <input v-model="msg"/>
        <button v-on:click="count++">petite-vue's</button>
    </div>
}
route GET / = <html><body><Order/></body></html>
"#;
    says(
        src,
        &[
            "6 warning: `@click.prevent` never fires: there are no modifiers; write `onclick={(e) => { e.preventDefault(); ... }}`",
            "7 warning: `@click.stop.prevent` never fires: there are no modifiers; write `onclick={(e) => { e.stopPropagation(); e.preventDefault(); ... }}`",
            "8 warning: `@click.once` never fires: there are no modifiers; write `onclick={(e) => { ... }}`",
            "9 warning: `v-model` is Vue's: write `value={name}`, which is two-way by itself",
            "10 warning: `v-model` is Vue's: write `checked={ok}`, which is two-way by itself",
            "11 warning: `v-model` is Vue's: write `value={...}`, which is two-way by itself",
            "12 warning: `v-on:click.prevent` is Vue's: write `onclick={...}`",
            "13 warning: `@submit.prevent` never fires: there are no modifiers, and `onsubmit` already keeps the page from reloading: write `onsubmit={...}`",
        ],
    );
    // Alpine's `@click.prevent="..."`, text: refused, with one message (found in review).
    says(
        "component A() {\n    state open = false\n    <button @click.prevent=\"open = true\">{open}</button>\n}\nroute GET / = <html><body><A/></body></html>\n",
        &["3 `@click.prevent` never fires: there are no modifiers; write `onclick={(e) => { e.preventDefault(); ... }}`"],
    );
}

// (FEEDBACK72.md, Q-1032 and Q-1031; branch feedback72-81)
#[test]
fn q1032_svelte_and_a_modifier_on_on_name() {
    let c = |attr: &str| format!("component Pot() {{\n    state n = 0\n    <form><button {}={{() => {{ n += 1 }}}}>{{n}}</button></form>\n}}\nroute GET / = <html><body><Pot/></body></html>\n", attr);
    says(&c("on:click"), &["3 `on:click` is Svelte's: write `onclick={...}`"]);
    says(&c("on:mouseenter"), &["3 `on:mouseenter` is Svelte's: write `onmouseenter={...}`"]);
    says(&c("onclick.prevent"), &["3 `onclick.prevent` never fires: there are no modifiers; write `onclick={(e) => { e.preventDefault(); ... }}`"]);
    says(&c("onsubmit.prevent"), &["3 `onsubmit.prevent` never fires: there are no modifiers, and `onsubmit` already keeps the page from reloading: write `onsubmit={...}`"]);
    // HTML's own, written as text, stays as it was.
    says("route GET / = <html><body><button onclick=\"history.back()\" on:click=\"x\">a</button></body></html>\n", &[]);
}

// ---- Q-1020: a handler in markup the server alone makes -------------------------------------------

// (FEEDBACK73.md, Q-1020; branch feedback72-81)
#[test]
fn q1020_a_view_placed_in_a_page() {
    let src = "state likes = 0\n\nview LikeView() =\n    <button @click={() => { likes += 1 }}>{likes}</button>\n\ncomponent LikeButton() {\n    <button @click={() => { likes += 1 }}>{likes}</button>\n}\n\nroute GET / = <html><body><LikeView/><LikeButton/></body></html>\n";
    says(src, &["4 warning: `onclick` in view LikeView never runs: LikeView is placed in a page (t.qwe:10), not in a component. Make it a component, or place it inside one"]);
}

// Placed through another view, a function that makes the page, or as what a view is given; not
// as a component's child. (FEEDBACK73.md, Q-1020; branch feedback72-81)
#[test]
fn q1020_a_view_in_a_component_and_in_a_page_and_what_reaches_it() {
    let src = r#"state n = 0
view Inner() = <span @mouseenter={() => { n += 1 }}>i</span>
view Outer() = <div><Inner/></div>
view Both() = <button @click={() => { n += 1 }}>b</button>
fn layout(body) = <html><body>{body}</body></html>
component Card() {
    <div><Both/></div>
}
component Shell(children) {
    <main>{children}</main>
}
view Hidden() = <button @click={() => { n += 1 }}>h</button>
view Frame(children) = <section>{children}</section>
view Given() = <button @click={() => { n += 1 }}>g</button>
route GET / = layout(<div><Card/><Shell><Hidden/></Shell><Outer/><Both/><Frame><Given/></Frame></div>)
"#;
    says(
        src,
        &[
            "2 warning: `onmouseenter` in view Inner never runs: Inner is placed in a page (t.qwe:3), not in a component. Make it a component, or place it inside one",
            "4 warning: `onclick` in view Both never runs: Both is placed in a page (t.qwe:15), not in a component. Make it a component, or place it inside one",
            "14 warning: `onclick` in view Given never runs: Given is placed in a page (t.qwe:15), not in a component. Make it a component, or place it inside one",
        ],
    );
}

// A route's own markup, and a plain function's a page places outside any component, lose their
// handlers too (decided in review); a function's that only hands on its parameter, or one a
// component calls, does not. (FEEDBACK73.md, Q-1020; branch feedback72-81)
#[test]
fn q1020_a_routes_own_markup_and_a_function_it_calls() {
    let src = r#"state n = 0
fn layout(body) = <html><body>{body}<p><button onclick={() => { n += 1 }}>more</button></p></body></html>
fn action(label, go) = <button onclick={go}>{label}</button>
fn counter() = <button onclick={() => { n += 1 }}>{n}</button>
component Box() {
    <div>{counter()}</div>
}
route GET / = layout(<div>{action("a", null)}<Box/></div>)
route GET /own = <html><body><button onclick={() => { n += 1 }} ondblclick={() => { n = 0 }}>{n}</button></body></html>
"#;
    says(
        src,
        &[
            "2 warning: `onclick` in `layout` never runs: `layout` makes markup for the page of `GET /` (t.qwe:8), on the server, and only a component's handlers reach the browser: put it in a component",
            "9 warning: `onclick` here never runs: the page of `GET /own` is made on the server, and only a component's handlers reach the browser: put this markup in a component",
            "9 warning: `ondblclick` here never runs: the page of `GET /own` is made on the server, and only a component's handlers reach the browser: put this markup in a component",
        ],
    );
}

// The page's markup as deep as the parser takes it, read on a thread with less stack than a
// request's. (FEEDBACK73.md, Q-1020; branch feedback72-81)
#[test]
fn q1020_deep_markup_in_a_page_checks_on_a_small_stack() {
    let t = std::thread::Builder::new()
        .stack_size(512 << 10)
        .spawn(|| {
            for n in [1, 60, 120, 250, 255, 256, 257] {
                let src = format!(
                    "state k = 0\nview V() = <button @click={{() => {{ k += 1 }}}}>v</button>\nroute GET / = {}<V/>{}\nroute GET /p = {{{}1{}}}\n",
                    "<b>".repeat(n),
                    "</b>".repeat(n),
                    "(".repeat(n),
                    ")".repeat(n)
                );
                let d = diags(&src);
                let view = "2 warning: `onclick` in view V never runs: V is placed in a page (t.qwe:3), not in a component. Make it a component, or place it inside one";
                assert!(d.iter().any(|m| m == view) || d.iter().any(|m| m.contains("nests too deeply")), "{}: {:?}", n, d);
            }
        })
        .unwrap();
    t.join().unwrap();
}

// A view's handler that hands the event to what it was given loses nothing where it is given
// nothing. (FEEDBACK73.md, Q-1020; branch feedback72-81)
#[test]
fn q1020_a_handler_that_only_hands_on_the_event() {
    let src = r#"view Button(label, on_click = null) = <button @click={() => { on_click && on_click() }}>{label}</button>
view Field(on_input = null) = <input @input={on_input}/>
view Field2(on_input = null) = <input @input={(e) => { if on_input != null { let v = e.target.value
    on_input(v) } }}/>
route GET / = <html><body><Button label="a"/><Field/><Field2/></body></html>
"#;
    says(src, &[]);
}

// ---- Q-1015: what a component draws again as it wakes ----------------------------------------------

// (FEEDBACK72.md, Q-1015; branch feedback72-81)
#[test]
fn q1015_a_draw_in_a_components_let_or_markup() {
    let src = r#"fn new_ticket() -> int {
    return random_int(1000, 9999)
}
fn ticket_of(x) = new_ticket() + x
view Receipt() = <small>{uuid()}</small>
component Raffle(name: str) {
    let ticket = ticket_of(0)
    let roll = random()
    let base = random()
    let code = base * 2
    state prize = ""
    <section>
        <p>{name}: <b>#{ticket}</b> {roll} {code} {uuid()}</p>
        <Receipt/>
        <button onclick={() => { prize = "a kite" }}>Draw a prize</button>
        {prize != "" && <p>{prize}</p>}
    </section>
}
route GET / = <html><body><Raffle name="Ada"/></body></html>
"#;
    says(
        src,
        &[
            "7 warning: `ticket` is drawn again in the browser when <Raffle> wakes (`ticket_of` calls `random_int`), so what the page shows changes under the visitor: `state ticket = ...` draws once, on the server, and the browser keeps it",
            "8 warning: `roll` is drawn again in the browser when <Raffle> wakes, so what the page shows changes under the visitor: `state roll = ...` draws once, on the server, and the browser keeps it",
            "9 warning: `base` is drawn again in the browser when <Raffle> wakes, so what the page shows changes under the visitor: `state base = ...` draws once, on the server, and the browser keeps it",
            "13 warning: `uuid()` here is drawn again in the browser when <Raffle> wakes, so what the page shows changes under the visitor: keep it in a state, `state x = uuid()`, which draws once, on the server, and the browser keeps it",
            "14 warning: <Receipt> here calls `uuid`, drawn again in the browser when <Raffle> wakes, so what the page shows changes under the visitor: give it the value from a state, `state x = uuid()`, which draws once, on the server, and the browser keeps it",
        ],
    );
}

// (FEEDBACK72.md, Q-1015; branch feedback72-81)
#[test]
fn q1015_a_state_a_handler_a_page_and_the_clock_are_left_alone() {
    let src = r#"fn new_ticket() -> int {
    return random_int(1000, 9999)
}
fn again(n) = n > 0 ? again(n - 1) : 0
component Raffle() {
    state ticket = new_ticket()
    state code = uuid()
    let shown = ticket + again(3)
    let _pick = () => random()
    let at = now()
    let seed = random()
    let _unused = uuid()
    let salt = random_int(1, 9)
    let mixed = salt * 2
    <section>
        <p>#{shown} {code} {at}</p>
        <button onclick={() => { ticket = random_int(1, 9) + seed + mixed }}>again</button>
        <form method="post" action="/pay" idempotent><button>pay</button></form>
    </section>
}
route GET / = <html><body><Raffle/><p>{uuid()} {random()}</p></body></html>
route POST /pay = "paid"
"#;
    says(src, &[]);
}

// What the warning advises keeps the server's value as the component wakes, in Chrome: the same
// ticket before and after the browser takes the component up. (FEEDBACK72.md, Q-1015; branch
// feedback72-81)
#[test]
fn q1015_the_advised_state_keeps_the_servers_draw_in_chrome() {
    let Some(chrome) = chrome() else { return };
    let src = "fn new_ticket() -> int {\n    return random_int(1000, 9999)\n}\ncomponent Raffle() {\n    state ticket = new_ticket()\n    state prize = \"\"\n    <section.raffle>\n        <p>ticket <b>#{ticket}</b></p>\n        <button onclick={() => { prize = \"a kite\" }}>Draw</button>\n        {prize != \"\" && <p>{prize}</p>}\n    </section>\n}\nroute GET / = <html><body><Raffle/></body></html>\n";
    assert_eq!(diags(src), Vec::<String>::new());
    let app = App::new("q1015-chrome", &[("main.qwe", src), ("probe.js", PROBE)]);
    let got = run_page(&chrome, &app.0.join("main.qwe"), &app.0.join("probe.js"));
    assert_eq!(got["woke"], serde_json::json!(true), "{}", got);
    assert_eq!(got["remade"], serde_json::json!(0), "{}", got);
    assert!(got["before"].as_str().is_some_and(|t| t.starts_with('#') && t.len() == 5), "{}", got);
    assert_eq!(got["before"], got["after"], "{}", got);
}

/// The ticket as the server sent it, then once the raffle has been pointed at, which wakes it, and
/// how the runtime took it up.
const PROBE: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const out = {};
  try {
    const q = () => window[Symbol.for("quillo")] || {};
    for (let i = 0; i < 400 && !q().booted; i++) await sleep(25);
    const ticket = () => document.querySelector(".raffle b").textContent;
    out.before = ticket();
    document.querySelector(".raffle p").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
    for (let i = 0; i < 400 && !(q().resumed || q().remade); i++) await sleep(25);
    await sleep(200);
    out.after = ticket();
    out.woke = (q().resumed || 0) + (q().remade || 0) > 0;
    out.remade = q().remade || 0;
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
"#;

fn chrome() -> Option<PathBuf> {
    let node = Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output().map(|o| o.status.success()).unwrap_or(false);
    let c = std::env::var("QUILLO_CHROME").ok().map(PathBuf::from).or_else(|| {
        ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
    });
    let c = c.filter(|_| node);
    if c.is_none() {
        eprintln!("skipping: needs Chrome and Node 22 or later");
    }
    c
}

/// `quillo run app` on a port the system picks, and what `script` recorded in the page, as
/// tests/browser/run.js drives headless Chrome.
fn run_page(chrome: &Path, app: &Path, script: &Path) -> serde_json::Value {
    let mut server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo")).arg("run").arg(app).args(["--port", "0"]).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap(),
    );
    // Generous: CI machines can be many times slower than a laptop.
    let port = support::port_in_stderr(&mut server.0, &app.display().to_string(), Instant::now() + Duration::from_secs(120));
    let profile = std::env::temp_dir().join(format!("quillo-chrome-f7281c2-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        .arg("120000")
        .arg(script)
        .output()
        .expect("node");
    drop(server);
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

// ---- Q-1024: `live`'s arguments, one by one ------------------------------------------------------------

// (FEEDBACK73.md, Q-1024; branch feedback72-81)
#[test]
fn q1024_live_is_shown_taking_the_rest_one_by_one() {
    says("component C() {\n  state c = live()\n  <p>{c}</p>\n}", &["2 missing argument `load` in call to `live`: live(load, ...args)"]);
    // Others as they were.
    says("print(js())", &["1 missing argument `name` in call to `js`: js(name, args)"]);
}
