//! FEEDBACK48.md: markup written as JSX and HTML have it, the parser's and the checker's part
//! (Q-560 to Q-563, Q-568 to Q-577; branch main). What the writer and the browser do with
//! values is in tests/feedback48_render.rs.

mod support;

use quillo::check::{check_source, Severity};
use quillo::Interp;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn diags(src: &str) -> Vec<String> {
    check_source(src, "t.qwe", Path::new("."))
        .diagnostics
        .iter()
        .map(|d| format!("{}{} {}", d.line, if d.severity == Severity::Warning { " warning:" } else { "" }, d.message))
        .collect()
}

/// The diagnostics that say `part`.
fn said(src: &str, part: &str) -> Vec<String> {
    diags(src).into_iter().filter(|d| d.contains(part)).collect()
}

fn run(src: &str) -> String {
    Interp::new(".").load_source(src, "t.qwe").map(|v| v.display()).unwrap_or_else(|e| format!("error: {}", e))
}

/// The markup `m` as the server writes it.
fn page(m: &str) -> String {
    run(&format!("str({})", m))
}

#[test]
fn markup_is_written_as_text() {
    assert_eq!(page("<p>x</p>"), "<p>x</p>");
}

// Q-562: HTML's named entities, all of them, in text and in each form of attribute value.
#[test]
fn every_named_entity_is_decoded_in_text_and_attributes() {
    assert_eq!(page("<p>&rarr; &times; &euro; &lsquo;q&rsquo; &bull; &deg; &trade; &frac12;</p>"), "<p>→ × € ‘q’ • ° ™ ½</p>");
    assert_eq!(page(r#"<a href="/s?a=1&amp;b=2" title="Tom &amp; Jerry">x</a>"#), r#"<a href="/s?a=1&amp;b=2" title="Tom &amp; Jerry">x</a>"#);
    assert_eq!(page("<p title='a &lt; b' data-x=a&amp;b>y</p>"), r#"<p title="a &lt; b" data-x="a&amp;b">y</p>"#);
    // Numbers, decimal and hex in either case; the longest name; one of two characters.
    assert_eq!(page("<p>&#65;&#x42;&#X43;&#39;</p>"), "<p>ABC&#39;</p>");
    assert_eq!(page("<p>&CounterClockwiseContourIntegral;</p>"), "<p>\u{2233}</p>");
    assert_eq!(page("<p>&NotNestedGreaterGreater;</p>"), "<p>\u{2aa2}\u{338}</p>");
    // Edges: a name HTML has not, no `;`, `&;`, a lone `&` at the end, a number no
    // character has, and a name in another case.
    assert_eq!(page("<p>&nope; &amp &; a&</p>"), "<p>&amp;nope; &amp;amp &amp;; a&amp;</p>");
    assert_eq!(page("<p>&#xD800; &AMP; &Amp;</p>"), "<p>&amp;#xD800; &amp; &amp;Amp;</p>");
    // Only `&` starts one: text without any is as it was.
    assert_eq!(page("<p>a; b # c</p>"), "<p>a; b # c</p>");
}

// Q-563: an attribute's "..." is plain text, as in HTML and JSX.
#[test]
fn attribute_strings_are_plain_text() {
    assert_eq!(page(r#"<p title="C:\new\table">x</p>"#), r#"<p title="C:\new\table">x</p>"#);
    assert_eq!(page(r#"<input pattern="\d{3}"/>"#), r#"<input pattern="\d{3}">"#);
    assert_eq!(page(r#"<p title="a\\b">x</p>"#), r#"<p title="a\\b">x</p>"#);
    // A quote is `&quot;`, or the value a Quillo string.
    assert_eq!(page(r#"<p title="say &quot;hi&quot;">x</p>"#), r#"<p title="say &quot;hi&quot;">x</p>"#);
    assert_eq!(page(r#"<p title={"say \"hi\""}>x</p>"#), r#"<p title="say &quot;hi&quot;">x</p>"#);
    // `\"`, an escape before, now ends the text: refused, with what to write.
    let e = run(r#"print(<p title="say \"hi\"">x</p>)"#);
    assert!(e.contains("`\\\"` in an attribute: its text is plain, as in HTML") && e.contains("write `&quot;` for a quote"), "{}", e);
    // In a Quillo string an escape is still one, and `${x}` in an attribute is still told.
    assert_eq!(run(r#"str(<p title={"a\tb"}>x</p>)"#), "<p title=\"a\tb\">x</p>");
    assert_eq!(said("let n = 1\nprint(<p title=\"${n}\">x</p>)\n", "${n}").len(), 1, "{:?}", diags("let n = 1\nprint(<p title=\"${n}\">x</p>)\n"));
}

// Q-572: a <pre>, a <textarea> and a <listing> keep their text as written.
#[test]
fn pre_and_textarea_keep_their_lines_and_spaces() {
    let src = "str(<pre>\nfn main() =\n    print(\"hi\")\n// done\n</pre>)";
    assert_eq!(run(src), "<pre>fn main() =\n    print(&quot;hi&quot;)\n// done\n</pre>");
    assert_eq!(run("str(<textarea>\nline one\n  line two\n</textarea>)"), "<textarea>line one\n  line two\n</textarea>");
    assert_eq!(run("str(<listing>\n a  b\n</listing>)"), "<listing> a  b\n</listing>");
    // Inside an element in it too, with entities; the line break right after the start tag
    // only; CRLF, which the lexer reads as LF.
    assert_eq!(run("let x = \"X\"\nstr(<pre class=\"a\"><code>\n    {x}\n    tail &amp; more\n</code></pre>)"), "<pre class=\"a\"><code>X\n    tail &amp; more\n</code></pre>");
    // The writer puts back the line break the browser drops after the start tag, so the
    // page shows the one left: "\nsecond".
    assert_eq!(run("str(<pre>\n\nsecond</pre>)"), "<pre>\n\nsecond</pre>");
    assert_eq!(run("str(<pre>\r\nx\r\n</pre>)"), "<pre>x\n</pre>");
    // Spaces on one line between values are kept; spaces across lines between tags, as
    // in code laid out to read, are not (the kit's CodeBlock).
    assert_eq!(run("let x = \"X\"\nstr(<pre>  {x}  {x}</pre>)"), "<pre>  X  X</pre>");
    assert_eq!(run("str(<pre>\n    <span>a</span>\n    <span>b</span>\n</pre>)"), "<pre><span>a</span><span>b</span></pre>");
    // Elsewhere, JSX's rules as before: lines joined, a `//` line a comment.
    assert_eq!(run("str(<div>\n    a\n    // a comment\n    b\n</div>)"), "<div>a b</div>");
}

// Q-575: JSX's comments between attributes and `{// ...}`.
#[test]
fn comments_in_a_tag_and_line_comments_in_braces() {
    assert_eq!(page("<div {/* note */} class=\"a\">x</div>"), "<div class=\"a\">x</div>");
    assert_eq!(page("<div {// note\n} class=\"a\" {/* one */ /* two */}>x</div>"), "<div class=\"a\">x</div>");
    assert_eq!(page("<div>\n    {// a note\n    }\n    <p>a</p>\n    {/* one */ // two\n    }\n</div>"), "<div><p>a</p></div>");
    // Braces with nothing in them are no comment, and a comment then a value is the value.
    assert!(run("print(<div>{}</div>)").contains("expected an expression"), "{}", run("print(<div>{}</div>)"));
    assert_eq!(page("<div>{/* x */ 1}</div>"), "<div>1</div>");
    // A comment never closed is not taken for one.
    assert!(run("print(<div {/* x>y</div>)").contains("error"));
}

// Q-576: `<img></img>`, as JSX and XHTML write it.
#[test]
fn a_void_element_may_have_its_end_tag_right_after() {
    assert_eq!(page(r#"<div><img src="a.png"></img><br></br><input></INPUT><hr/></div>"#), r#"<div><img src="a.png"><br><input><hr></div>"#);
    // Anywhere else, removed by the fix, where the fix closed the element around it.
    let Err(e) = quillo::eval_str("<div><img src=\"a.png\"> </img></div>") else { panic!("an error") };
    assert_eq!(e.msg, "`<img>` has no end tag: remove `</img>`");
    let f = e.fix.expect("a fix");
    assert_eq!((f.title.as_str(), f.line, f.col, f.len, f.text.as_str()), ("Remove </img>", 1, 23, 6, ""));
    let Err(e) = quillo::eval_str("<p>a</br></p>") else { panic!("an error") };
    assert_eq!(e.msg, "`<br>` has no end tag: remove `</br>`");
}

// Q-568: `.name` and `#name` on a view are its `class` and `id`; no member tags.
#[test]
fn shorthand_on_a_view_is_its_class_and_id() {
    let views = "view Box(class = \"\", id = null, children = null) = <div class={`box ${class}`} id={id}>{children}</div>\n";
    assert_eq!(run(&format!("{}str(<Box.wide.tall #b>in</Box>)", views)), "<div class=\"box wide tall\" id=\"b\">in</div>");
    assert_eq!(run(&format!("{}let k = \"k\"\nstr(<Box.{{k}}-x/>)", views)), "<div class=\"box k-x\"></div>");
    assert_eq!(run(&format!("{}str(<Box/>)", views)), "<div class=\"box \"></div>");
    // A view without them is told so, by the attribute's name.
    let src = "view Card() = <div.card>card</div>\nprint(<Card.big #main/>)\n";
    assert_eq!(said(src, "has no parameter").len(), 2, "{:?}", diags(src));
    assert!(said(src, "`id`").iter().any(|d| d.contains("give it an `id` parameter")), "{:?}", diags(src));
    assert!(said(src, "`class`").iter().any(|d| d.contains("`.name` giving `class`")), "{:?}", diags(src));
    // Both ways at once is the attribute twice.
    assert!(run(&format!("{}print(<Box.a class=\"b\"/>)", views)).contains("attribute `class` is written twice"));
    // A capital after the dot is JSX's member tag.
    let e = run("view Card() = <div/>\nprint(<Card.Header/>)");
    assert!(e.contains("there are no member tags: `<Card.Header>` is <Card> with the class `Header`"), "{}", e);
    // On an element, the shorthand is as before.
    assert_eq!(page("<div.a.b #c>x</div>"), "<div id=\"c\" class=\"a b\">x</div>");
}

// Q-569: what is between a view's tags when it has no `children`.
#[test]
fn children_with_nowhere_to_go_are_an_error() {
    let v = "view Card(title) = <div>{title}</div>\n";
    assert_eq!(diags(&format!("{}print(<Card title=\"t\">body text</Card>)\n", v)), ["2 <Card> takes no children, so what is between its tags is lost: give it a `children` parameter, or write <Card .../>"]);
    // Spaces alone are none; nor is a self-closed tag; a spread may carry them.
    assert!(diags(&format!("{}print(<Card title=\"t\">\n  </Card>)\nprint(<Card title=\"t\"/>)\n", v)).is_empty());
    assert!(diags(&format!("{}let m = {{ title: \"t\" }}\nprint(<Card {{...m}}>x</Card>)\n", v)).is_empty());
    // With the parameter, they go to it.
    assert_eq!(run("view Card(title, children) = <div>{title}{children}</div>\nstr(<Card title=\"t\"><b>b</b></Card>)"), "<div>t<b>b</b></div>");
}

// Q-570: React's own attribute names, which reach the page and do nothing.
#[test]
fn reacts_own_attributes_are_told() {
    let src = r##"let s = "<b>x</b>"
print(<input defaultValue="hello"/>)
print(<input type="checkbox" defaultChecked/>)
print(<div dangerouslySetInnerHTML={{ __html: s }} ref={s}></div>)
print(<svg viewBox="0 0 10 10"><path strokeWidth={2} fillRule="evenodd" strokeLinecap="round" d="M0 0"/><use xlinkHref="#a"/></svg>)
"##;
    let d = diags(src);
    for want in [
        "2 warning: `defaultValue` is React's, and does nothing in a page: write `value`",
        "3 warning: `defaultChecked` is React's, and does nothing in a page: write `checked`",
        "4 warning: `dangerouslySetInnerHTML` is React's, and does nothing in a page: put {raw(html)} between the element's tags",
        "4 warning: `ref` is React's, and does nothing in a page: write `@mount={(el) => ...}`",
        "5 warning: `strokeWidth` is React's: SVG's attribute is `stroke-width`",
        "5 warning: `fillRule` is React's: SVG's attribute is `fill-rule`",
        "5 warning: `strokeLinecap` is React's: SVG's attribute is `stroke-linecap`",
        "5 warning: `xlinkHref` is React's: SVG's attribute is `href`",
    ] {
        assert!(d.iter().any(|x| x.starts_with(want)), "{} in {:?}", want, d);
    }
    assert_eq!(d.len(), 8, "{:?}", d);
    // SVG's own names, and a view's parameter named `ref`, are let be.
    assert!(diags("print(<svg><path stroke-width=\"2\" fill-rule=\"evenodd\" d=\"M0 0\"/></svg>)\n").is_empty());
    assert!(diags("view V(ref) = <p>{ref}</p>\nprint(<V ref=\"r\"/>)\n").is_empty());
}

// Q-571: a function where markup goes.
#[test]
fn a_function_as_a_child_is_told() {
    let w = "a function where markup goes shows nothing: call it, {render(x)}, or give it to a view as an attribute";
    assert_eq!(said("print(<div>{(x) => <p>{x}</p>}</div>)\n", w).len(), 1);
    assert_eq!(said("fn go() = 1\nprint(<button>{go}</button>)\n", w).len(), 1);
    // Called, it is its value; as an attribute, a render prop.
    assert!(said("fn go() = 1\nprint(<button>{go()}</button>)\n", w).is_empty());
    assert!(said("view L(render) = <ul>{render(1)}</ul>\nprint(<L render={(i) => <li>{i}</li>}/>)\n", w).is_empty());
}

// Q-573: text and an inline element on lines of their own.
#[test]
fn text_and_an_inline_element_on_separate_lines_are_told() {
    let src = "let name = \"Ann\"\nprint(<p>\n    Hello,\n    <b>{name}</b>\n    welcome back\n</p>)\n";
    assert_eq!(
        said(src, "lines of their own"),
        [
            "4 warning: \"Hello,\" and <b> are on lines of their own, so no space comes between them on the page: write {\" \"} between them, or put them on one line",
            "4 warning: <b> and \"welcome\" are on lines of their own, so no space comes between them on the page: write {\" \"} between them, or put them on one line",
        ]
    );
    // A view counts as inline; one whose element is a block does not.
    let views = "view Em(children) = <em>{children}</em>\nview List(children) = <ul>{children}</ul>\n";
    assert_eq!(said(&format!("{}print(<p>\n    see\n    <Em>this</Em>\n</p>)\n", views), "lines of their own").len(), 1);
    assert!(said(&format!("{}print(<div>\n    a list:\n    <List><li>x</li></List>\n</div>)\n", views), "lines of their own").is_empty());
    // Not told: punctuation after, an opening bracket before, one line, blocks, braces
    // between, and <pre>, where the line break is kept.
    for ok in [
        "print(<p>\n    <a href=\"/x\">this</a>.\n</p>)\n",
        "print(<p>\n    (\n    <b>x</b>\n</p>)\n",
        "print(<p>Hello, <b>Ann</b> welcome</p>)\n",
        "print(<div>\n    Title\n    <div>block</div>\n</div>)\n",
        "print(<ul>\n    <li>a</li>\n    <li>b</li>\n</ul>)\n",
        "print(<p>\n    <b>x</b>{\" \"}\n    welcome\n</p>)\n",
        "print(<pre>\n    text\n    <b>x</b>\n</pre>)\n",
    ] {
        assert!(said(ok, "lines of their own").is_empty(), "{}: {:?}", ok, diags(ok));
    }
}

// Q-574: `xs.map((x, i) => ...)`, and how to have the index.
#[test]
fn the_method_form_has_its_function_checked_and_says_how_to_index() {
    let w = "`map` passes its function one item, but this one takes 2: for the index, `for [i, x] in enumerate(xs) => ...`, which gives a list too";
    assert_eq!(diags("let xs = [\"a\"]\nprint(xs.map((x, i) => i))\n"), [format!("2 {}", w)]);
    assert_eq!(diags("let xs = [\"a\"]\nprint(map(xs, (x, i) => i))\n"), [format!("2 {}", w)]);
    assert_eq!(said("let xs = [1]\nprint(xs.filter((a, b, c) => a))\n", "`filter` passes its function one item, but this one takes 3").len(), 1);
    assert_eq!(said("let xs = [1]\nprint(xs.reduce(0, (x) => x))\n", "`reduce` passes the total so far and an item").len(), 1);
    // The forms that fit, and a default for the second, are fine; the index form works.
    assert!(diags("let xs = [1]\nprint(xs.map((x) => x))\nprint(xs.map((x, i = 0) => x))\nprint(xs.reduce(0, (t, x) => t + x))\n").is_empty());
    assert_eq!(run("let xs = [\"a\", \"b\"]\nfor [i, x] in enumerate(xs) => `${i}${x}`"), r#"["0a", "1b"]"#);
    // `for (x, i)`: what to write instead.
    let e = run("for (x, i) in [1] { print(x) }");
    assert!(e.contains("`for (x, i)`: a pattern takes no parentheses; for each item with its index, write `for [i, x] in enumerate(xs)`"), "{}", e);
}

// Q-577: React's forms, told how Quillo writes them.
#[test]
fn reacts_forms_say_how_quillo_writes_them() {
    let frag = "there are no `<>` fragments: wrap the elements in one, like <div>, or give a list";
    assert!(run("print(<React.Fragment><p>a</p></React.Fragment>)").contains(frag));
    assert_eq!(said("print(<Fragment><p>a</p></Fragment>)\n", frag).len(), 1);
    // A program's own `Fragment` is its view.
    assert!(diags("view Fragment(children) = <div>{children}</div>\nprint(<Fragment><p>a</p></Fragment>)\n").is_empty());
    let d = diags("let Tag = \"h1\"\nprint(Tag)\n");
    assert!(d[0].starts_with("1 `let Tag = ...` reads `Tag` as a type, as a capitalised name in a pattern is one, so it fails when it runs: name the variable in lower case, `let tag = ...`; and a tag's name is written out, as there are no dynamic tags"), "{:?}", d);
    assert!(diags("view V() {\n    let Kind = 1\n    <p>{Kind}</p>\n}\n")[0].contains("`let Kind = ...` reads `Kind` as a type"));
    // A record's name in a pattern is its type, as before; ALL CAPS is a name.
    assert!(diags("type Point = { x: int }\nlet p = Point({ x: 1 })\nlet ABC = 2\nprint(p, ABC)\n").is_empty());
    assert_eq!(diags("view Card(props) = <div>{props.title}</div>\nprint(<Card title=\"x\" body=\"y\"/>)\n"), ["2 a view's parameters are its props, each by name: write `view Card(title, body)`, not one `props`"]);
    assert!(diags("view Card(props) = <div>{props.title}</div>\nprint(<Card props={{ title: \"x\" }}/>)\n").is_empty());
    let e = run("view Card({ title, body }) = <div>{title}</div>");
    assert!(e.contains("a view's parameters are its props, each by name: write `view Card(title, body)`, without the braces"), "{}", e);
    let e = run("fn f({ a }) = a");
    assert!(e.contains("a parameter is a name, not a `{ ... }` pattern: write `f(a)`, or take the map and read its fields"), "{}", e);
    let e = run("let xs = [1]\nprint(<ul>{...xs}</ul>)");
    assert!(e.contains("`{...xs}` among children: a list shows each of its items where it stands, so write `{xs}`"), "{}", e);
    // `{...m}` in a tag spreads a map, as before.
    assert_eq!(run("let m = { title: \"t\" }\nstr(<p {...m}>x</p>)"), "<p title=\"t\">x</p>");
}

// Q-560 and Q-561, as the parser renames them; the browser hears them in the test below.
#[test]
fn reacts_event_names_are_the_doms() {
    // `onChange` on a field is `@input`, so with `@input` as well it is the same twice.
    let e = run("fn f(e) = 1\nprint(<input onChange={f} @input={f}/>)");
    assert!(e.contains("attribute `@input` is written twice in <input> (`onChange={...}` is `@input`)"), "{}", e);
    let e = run("fn f(e) = 1\nprint(<div onDoubleClick={f} @dblclick={f}/>)");
    assert!(e.contains("(`onDoubleClick={...}` is `@dblclick`)"), "{}", e);
    let e = run("fn f(e) = 1\nprint(<div onFocus={f} @focusin={f}/>)");
    assert!(e.contains("(`onFocus={...}` is `@focusin`)"), "{}", e);
    // On a <select>, and the DOM's own `@change`, `change` stays.
    assert!(!run("fn f(e) = 1\nprint(<select onChange={f} @input={f}></select>)").contains("error"));
    assert!(!run("fn f(e) = 1\nprint(<input @change={f} @input={f}/>)").contains("error"));
    // React's capture phase has no form here.
    let e = run("fn f(e) = 1\nprint(<div onClickCapture={f}>x</div>)");
    assert!(e.contains("`onClickCapture` listens before the element's own handlers, as React's `…Capture` does, which Quillo has no form for: write `onClick` (or `@click`)"), "{}", e);
    let e = run("fn f(e) = 1\nprint(<div onDoubleClickCapture={f}>x</div>)");
    assert!(e.contains("write `onDoubleClick` (or `@dblclick`)"), "{}", e);
    // The DOM's own events that end in "capture" are theirs.
    assert!(!run("fn f(e) = 1\nprint(<div onGotPointerCapture={f} onLostPointerCapture={f}>x</div>)").contains("error"));
    // `@mouseEnter`: compared as written.
    assert_eq!(
        diags("fn f(e) = 1\nprint(<div @mouseEnter={f} @keydown={f} @myEvent={f}>x</div>)\n"),
        ["2 warning: `@mouseEnter` never fires: the browser's event names are lower case, `@mouseenter` (or React's `onMouseEnter`)"]
    );
    // On a view, `onChange` is a parameter, as before.
    assert!(diags("view V(onChange) = <input @input={onChange}/>\nfn f(e) = 1\nprint(<V onChange={f}/>)\n").is_empty());
}

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .filter(|_| Command::new("node").args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"]).output().is_ok_and(|o| o.status.success()))
}

#[test]
fn reacts_event_names_fire_in_the_browser() {
    let Some(chrome) = chrome() else {
        eprintln!("skipping: needs Chrome and Node 22 or later");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser");
    let port = support::free_port();
    let _server = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(dir.join("f48-events.qwe"))
            .args(["--port", &port.to_string()])
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
    let profile = std::env::temp_dir().join(format!("quillo-chrome-f48-{}-{}", std::process::id(), port));
    let out = Command::new("node")
        .arg(dir.join("run.js"))
        .arg(&chrome)
        .arg(format!("http://127.0.0.1:{}/", port))
        .arg(&profile)
        .arg("60000")
        .arg(dir.join("f48-events.js"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&profile);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["exception"], serde_json::Value::Null, "{}", r);
    // Each keystroke, without leaving the field.
    assert_eq!(r["typed"], serde_json::json!(["a", "an", "ann"]), "{}", r);
    assert_eq!(r["focusedStill"], true, "{}", r);
    assert_eq!(r["note"], "hi", "{}", r);
    assert_eq!(r["log"], "dbl,focus,blur,pick y", "{}", r);
    assert_eq!(r["errors"], serde_json::json!([]), "{}", r);
}
