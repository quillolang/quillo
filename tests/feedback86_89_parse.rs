//! FEEDBACK87.md's Q-1371 (branch feedback86-89): an attribute's `{` left open, its `}` left out
//! after a lambda's block (`onclick={() => { n += 1 }>`), so the `>` that ends the tag was read
//! as greater than and the element's text as code, is said to be the `{` not closed, with its
//! line and column, where the error named that text (`unexpected character `♥``, `expected an
//! expression, found `+``). By what the parser read, as the coordinator decided in round 3: (b) a
//! handler's block, then a `>`, `>=` or `/` read as an operator, says where the `}` goes, and the
//! fix puts it there when the handler is the whole value; (a) the parser wanting the value's `}`
//! where it ended names that `{`; (c) anything else is as it was. With Q-008 seen again: no note
//! that the code may be for a newer Quillo after a character none takes in code, as `♥`. The
//! program the report gives is run by this build's `quillo` under a deadline; the others are
//! checked in this process.

mod support;

use quillo::check::{check_source, Severity};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The first error `quillo check` gives `src`, with its line, and whether the note that the code
/// may be for a newer Quillo follows it (`syntax_errors`, which the note is printed for).
fn first_error(src: &str) -> (String, bool) {
    let r = check_source(src, "main.qwe", Path::new("."));
    let d = r.diagnostics.iter().find(|d| d.severity == Severity::Error).unwrap_or_else(|| panic!("no error\n--- source ---\n{}", src));
    (format!("{} {}", d.line, d.message), !r.syntax_errors.is_empty())
}

/// `src`'s first error is `want` (its line, then its message), with no note of a newer Quillo.
fn says(src: &str, want: &str) {
    let (got, note) = first_error(src);
    assert_eq!(got, want, "\n--- source ---\n{}", src);
    assert!(!note, "a newer Quillo's note after {:?}\n--- source ---\n{}", got, src);
}

/// `src`'s first error is `want`, with the note a newer Quillo gets after it, `note`.
fn said(src: &str, want: &str, note: bool) {
    let (got, n) = first_error(src);
    assert_eq!((got.as_str(), n), (want, note), "\n--- source ---\n{}", src);
}

/// `src` is checked with no error.
fn clean(src: &str) {
    let r = check_source(src, "main.qwe", Path::new("."));
    let errors: Vec<String> = r.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| format!("{} {}", d.line, d.message)).collect();
    assert!(errors.is_empty(), "{:?}\n--- source ---\n{}", errors, src);
}

/// A component `Like` whose body is `line`, on its line 3, after `state n = 0`; the page is it.
fn like(line: &str) -> String {
    format!("component Like() {{\n    state n = 0\n    {}\n}}\nroute GET / = <Like/>\n", line)
}

/// The message for an attribute `name`'s `{` at `col` on line 3, the tag's `>` on that line (b).
fn open_brace(name: &str, col: u32, tag: &str) -> String {
    format!(
        "3 `{}={{` on line 3, column {} is never closed: add `}}` before the tag's `>`; without it, that `>` is read as greater than and the text of <{}> as code",
        name, col, tag
    )
}

/// The message for an attribute `name`'s `{` on `line`, column `col`, whose `}` the parser wanted
/// where it found `found` (a).
fn wanted(line: u32, name: &str, col: u32, found: &str) -> String {
    format!("{0} `{1}={{` on line {0}, column {2}: expected `}}` to close it, found {3}", line, name, col, found)
}

/// `src`'s first error is `want`, as it was before Q-1371 (c), with the note a newer Quillo gets
/// after it, `note`, as Q-008 now has it.
fn as_it_was(src: &str, want: &str, note: bool) {
    let (got, n) = first_error(src);
    assert_eq!((got.as_str(), n), (want, note), "\n--- source ---\n{}", src);
    let Err(e) = quillo::eval_str(src) else { panic!("an error\n{}", src) };
    assert!(e.fix.as_ref().map_or(true, |f| f.title != "Add }"), "{}", src);
}

/// A page whose component `Like` takes `on_like`, given on line 6 as `<Like ...` + `rest`.
fn page(rest: &str) -> String {
    format!("component Like(on_like) {{\n    <button onclick={{on_like}}>♥</button>\n}}\ncomponent Page() {{\n    state n = 0\n    <Like {}\n}}\nroute GET / = <Page/>\n", rest)
}

/// The message for attribute `name`'s `{` on `line`, column `col`, before a `/>` on its line (b).
fn open_before_slash(line: u32, name: &str, col: u32) -> String {
    format!("{0} `{1}={{` on line {0}, column {2} is never closed: add `}}` before the tag's `/>`; without it, that `/` is read as division and the `>` as greater than", line, name, col)
}

/// `src`'s fix, (line, column), and that the program with it passes `quillo check`.
fn fixed_at(src: &str) -> (u32, u32) {
    let Err(e) = quillo::eval_str(src) else { panic!("an error\n{}", src) };
    let f = e.fix.unwrap_or_else(|| panic!("no fix: {}\n{}", e.msg, src));
    assert_eq!((f.title.as_str(), f.len, f.text.as_str()), ("Add }", 0, "}"));
    let mut lines: Vec<String> = src.lines().map(String::from).collect();
    let l = &mut lines[f.line as usize - 1];
    let at = l.char_indices().nth(f.col as usize).map_or(l.len(), |(b, _)| b);
    l.insert_str(at, &f.text);
    clean(&lines.join("\n"));
    (f.line, f.col)
}

/// `src`'s error has no fix.
fn no_fix(src: &str) {
    let Err(e) = quillo::eval_str(src) else { panic!("an error\n{}", src) };
    assert!(e.fix.is_none(), "{:?}\n{}", e.fix.map(|f| (f.title, f.line, f.col)), src);
}

// ---- Q-1371: the report's program, as `quillo check` prints it ------------------------------

#[test]
fn the_reports_program_is_told_its_open_brace_with_no_version_note() {
    let src = "component Like() {\n    state n = 0\n    <button onclick={() => { n += 1 }>♥ {n}</button>\n}\nroute GET / = <Like/>\n";
    let dir = support::temp_dir(&format!("f8689-parse-repro-{}", std::process::id()));
    std::fs::write(dir.join("main.qwe"), src).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo")).args(["check", "main.qwe"]).current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // A guard against a hang only, far past what a slow debug build takes.
    let deadline = Instant::now() + Duration::from_secs(300);
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            let _ = c.kill();
            let _ = c.wait();
            let _ = std::fs::remove_dir_all(&dir);
            panic!("quillo check never ended");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (mut out, mut err) = (String::new(), String::new());
    c.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    c.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(status.code(), Some(1), "{}{}", out, err);
    assert_eq!(
        err,
        "main.qwe:3: error: `onclick={` on line 3, column 21 is never closed: add `}` before the tag's `>`; without it, that `>` is read as greater than and the text of <button> as code\nquillo: 1 error\n"
    );
}

// ---- Q-1371 (b): a handler's block, then the tag's `>` read as an operator ----------------------

#[test]
fn an_open_brace_after_a_handlers_block_is_named_and_fixed_whatever_text_follows() {
    // Text that does not read as code after the tag's `>`, which the parser stops in: the report's
    // two, an empty text, a text beginning with `=` (`}>=`, found in review), and a `>` after the
    // first character; and the element's end tag without its own (`</div>`, said after the fix).
    for text in ["♥ {n}", "+ {n}", "", "=", "♥ a > b"] {
        let src = like(&format!("<button onclick={{() => {{ n += 1 }}>{}</button>", text));
        says(&src, &open_brace("onclick", 21, "button"));
        assert_eq!(fixed_at(&src), (3, 37), "{}", text);
    }
    says(&like("<button onclick={() => { n += 1 }>♥ {n}</div>"), &open_brace("onclick", 21, "button"));
    // `@click`, an end tag in capitals, a comment in the block with a quote and a brace in it.
    says(&like("<button @click={() => { n += 1 }>♥ {n}</button>"), &open_brace("@click", 20, "button"));
    says(&like("<button onclick={() => { n += 1 }>♥ {n}</BUTTON>"), &open_brace("onclick", 21, "button"));
    says(&like("<button onclick={() => { n += 1 /* don't } */ }>♥ {n}</button>"), &open_brace("onclick", 21, "button"));
    // Text that reads as code (a name, a markup element, `{...}`), the comparison then complete:
    // as the parser then finds it, the `}` wanted (a) or the old message (c) (round 4, the
    // coordinator's decision: a block compared is a program's own once its right side is read).
    as_it_was(&like("<button onclick={() => { n += 1 }>Like</button>"), "3 expected an expression, found `/`", false);
    as_it_was(&like("<button onclick={() => { n += 1 }>= {n}</button>"), "3 expected an expression, found `/`", false);
    said(&like("<button onclick={() => { n += 1 }>Like it</button>"), &wanted(3, "onclick", 21, "`it`"), true);
    said(&like("<button onclick={() => { n += 1 }><b>♥</b> {n}</button>"), &wanted(3, "onclick", 21, "`{`"), false);
    said(&like("<button onclick={() => { n += 1 }>a > b</button>"), &wanted(3, "onclick", 21, "`<`"), false);
    said(&like("<button onclick={() => { n += 1 }>a { b</button>"), &wanted(3, "onclick", 21, "`{`"), false);
    // A component's prop, an element inside others, a string attribute before.
    let src = "component Like(on_like) {\n    <button onclick={on_like}>♥</button>\n}\ncomponent Page() {\n    state n = 0\n    <Like on_like={() => { n += 1 }>♥ {n}</Like>\n}\nroute GET / = <Page/>\n";
    says(src, "6 `on_like={` on line 6, column 19 is never closed: add `}` before the tag's `>`; without it, that `>` is read as greater than and the text of <Like> as code");
    says(&like("<div><span><button onclick={() => { n += 1 }>♥ {n}</button></span></div>"), &open_brace("onclick", 32, "button"));
    says(&like("<button title=\"like\" onclick={() => { n += 1 }>♥ {n}</button>"), &open_brace("onclick", 34, "button"));
    says(&like("<div><p title={n}>a</p><button onclick={() => { n += 1 }>♥ {n}</button></div>"), &open_brace("onclick", 44, "button"));
    // Characters before it outside ASCII count one column each.
    says(&like("<button title=\"héllo ♥\" onclick={() => { n += 1 }>♥ {n}</button>"), &open_brace("onclick", 37, "button"));
}

#[test]
fn the_tags_line_is_named_where_the_error_is_on_another() {
    // The `{` on line 4, the tag's `>` on line 7, where the parser stopped: the error stays on
    // its line, as the errors after it are looked for from there (found in review).
    let src = "component Like() {\n    state n = 0\n    <button\n        onclick={() => {\n            n += 1\n        }\n    >♥ {n}</button>\n}\nroute GET / = <Like/>\n";
    says(src, "7 `onclick={` on line 4, column 17 is never closed: add `}` before the tag's `>` on line 7; without it, that `>` is read as greater than and the text of <button> as code");
    assert_eq!(fixed_at(src), (7, 4));
    // The `{` and the `>` on line 1, the parser stopping on line 2: the `>`'s line said too
    // (found in the second review).
    let src = "route GET / = <button onclick={() => { print(1) }>\nlet me\nknow</button>\n";
    says(src, "2 `onclick={` on line 1, column 31 is never closed: add `}` before the tag's `>` on line 1; without it, that `>` is read as greater than and the text of <button> as code");
    assert_eq!(fixed_at(src), (1, 49));
    // And the errors after it are looked for from that line: a text line starting with a keyword
    // is not taken for a statement of its own (`type` reads as a name, so the `}` is wanted at
    // `here`, round 4).
    let src = "route GET / = <button onclick={() => { print(1) }>\ntype here\n</button>\nroute GET /b = <p>{1 +* 2}</p>\n";
    let r = check_source(src, "main.qwe", Path::new("."));
    let errors: Vec<String> = r.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| format!("{} {}", d.line, d.message)).collect();
    assert_eq!(
        errors,
        [
            "2 `onclick={` on line 1, column 31: expected `}` to close it, found `here`",
            "4 expected an expression, found `*`",
        ]
    );
}

#[test]
fn the_fix_is_made_only_where_the_handler_is_the_whole_value() {
    // The report's program, its fix applied, passes check.
    let src = like("<button onclick={() => { n += 1 }>♥ {n}</button>");
    let Err(e) = quillo::eval_str(&src) else { panic!("an error") };
    let f = e.fix.expect("a fix");
    assert_eq!((f.title.as_str(), f.line, f.col, f.len, f.text.as_str()), ("Add }", 3, 37, 0, "}"));
    assert_eq!(fixed_at(&src), (3, 37));
    // With spaces before the handler, and its parameters.
    assert_eq!(fixed_at(&like("<button onclick={ (e) => { n += 1 }>♥</button>")), (3, 39));
    // A handler inside a call or a list, and one written `e => {...}` (whose start the parser does
    // not keep): no handler that is the whole value, so as it was (round 5, the coordinator's
    // decision).
    for line in [
        "<button onclick={f(() => { n += 1 }>♥ {n}</button>",
        "<button onclick={[() => { n += 1 }>♥ {n}</button>",
        "<button onclick={e => { n += 1 }>♥</button>",
    ] {
        as_it_was(&like(line), "3 unexpected character `♥`", false);
    }
    // An element in the text read before the parser stops (`</buton>` in it): as it was, with its
    // own fix (round 4: the stop is past the comparison's first token).
    as_it_was(&like("<button onclick={() => { n += 1 }><b>♥</buton> {n}</button>"), "3 `</buton>` does not close `<b>` from line 3: expected `</b>`", false);
}

// ---- Q-1371 (b): a tag that closes itself (the coordinator's decision, 2026-11-25) ---------------

#[test]
fn an_open_brace_before_a_tags_own_end_is_named_and_fixed_there() {
    let src = page("on_like={() => { n += 1 }/>");
    says(&src, &open_before_slash(6, "on_like", 19));
    assert_eq!(fixed_at(&src), (6, 35));
    // Over several lines: the `/>`'s line said.
    let src = page("on_like={() => {\n        n += 1\n    }\n    />");
    says(
        &src,
        "9 `on_like={` on line 6, column 19 is never closed: add `}` before the tag's `/>` on line 9; without it, that `/` is read as division and the `>` as greater than",
    );
    assert_eq!(fixed_at(&src), (9, 4));
    // A `/>` in an element's text after a name: the tag's comparison reads `a` before it stops,
    // which a block compared then divided (`{ x * 2 } > 5/>`) would too: as it was (round 4).
    as_it_was(&like("<p onclick={() => { n += 1 }>a/>b</p>"), "3 expected an expression, found `>`", false);
    // What is right stays right: a division in a value, the prop closed.
    clean(&like("<input value={n/2}/>"));
    clean(&page("on_like={() => { n += 1 }}/>"));
}

// ---- Q-1371 (a): the parser wanting the value's `}` ------------------------------------------------

#[test]
fn a_brace_wanted_where_the_value_ended_is_said_of_its_brace_with_no_fix() {
    // The `}` left out before the next attribute, with or without a `>` of the value's own, in a
    // self-closing tag and in a prop's markup, and the second review's `=` texts. The note a newer
    // Quillo gets after a word found stays, as without the `{` named (round 4, found in review).
    for (line, name, col, found, note) in [
        ("<button onclick={() => { n += 1 } title=\"like\">♥ {n}</button>", "onclick", 21, "`title`", true),
        ("<button disabled={n > 9 onclick={() => { n += 1 }}>+</button>", "disabled", 22, "`onclick`", true),
        ("<button disabled={n > 9 onclick={() => { n += 1 }}>=</button>", "disabled", 22, "`onclick`", true),
        ("<button class={n > 0 ? \"on\" : \"off\" title=\"equals\">= {n}</button>", "class", 19, "`title`", true),
        ("<p hidden={n > 0 title=\"x\">shown</p>", "hidden", 15, "`title`", true),
        ("<p class={n > 0 ? \"on\" : \"off\" id=\"x\">shown</p>", "class", 14, "`id`", true),
        ("<button onclick={() => n > 0 && print(n) title=\"x\">Go</button>", "onclick", 21, "`title`", true),
        ("<p hidden={n > 0 data-x=\"1\">shown</p>", "hidden", 15, "`data`", true),
        ("<p hidden={n > 0 disabled>shown</p>", "hidden", 15, "`disabled`", true),
        ("<p hidden={n > 0 and n > 1>shown</p>", "hidden", 15, "`and`", true),
        ("<input hidden={n > 0 value=\"x\"/>", "hidden", 19, "`value`", true),
        // The tag's `>` where the `}` was wanted, `.class` shorthand before it, and a `>=`.
        ("<p hidden={n > 0>shown</p>", "hidden", 15, "`>`", false),
        ("<p hidden={n > 0 .on>t</p>", "hidden", 15, "`>`", false),
        ("<button disabled={n > 9>= 10</button>", "disabled", 22, "`>=`", false),
        // A `}` coming later: what the parser wanted, no word on whether one comes.
        ("<p title={n > 1 c}>x</p>", "title", 14, "`c`", true),
        ("<p title={n > 0 ? \"</p>\" : \"b\" c}>t</p>", "title", 14, "`c`", true),
    ] {
        let src = like(line);
        said(&src, &wanted(3, name, col, found), note);
        no_fix(&src);
    }
    let card = |v: &str| format!("component Card(icon) {{\n    <p>{{icon}}</p>\n}}\nroute GET / = <Card icon={{{}\n", v);
    for (v, found) in [("<b>hi</b> title=\"x\">t</Card>", "`title`"), ("<img src=\"x\"> body</Card>", "`body`"), ("<Card icon={1}>x</Card> y}>t</Card>", "`y`")] {
        said(&card(v), &wanted(4, "icon", 26, found), true);
        no_fix(&card(v));
    }
    said(&page("on_like={() => { n += 1 } title=\"x\"/>"), &wanted(6, "on_like", 19, "`title`"), true);
    said(&page("on_like={a b}/>"), &wanted(6, "on_like", 19, "`b`"), true);
    // At the end of the input, which the REPL reads on after: still said so.
    says("route GET / = <p onclick={() => { print(1) }>hi\n", "2 `onclick={` on line 1, column 26: expected `}` to close it, found end of input");
}

// ---- Q-1371 (c): everything else, as it was ----------------------------------------------------

#[test]
fn other_errors_in_and_after_an_attribute_are_as_they_were() {
    // A comparison, then the tag's `>` and a text beginning with `=` or `=>` (found in the second
    // review), and in a self-closing tag; a `)` or `]` left out, the `{` closed.
    as_it_was(&like("<button class={n > 0 ? \"on\" : \"off\">=</button>"), "3 expected an expression, found `<`", false);
    as_it_was(&like("<p class={n > 0 ? \"a\" : \"b\">=> next</p>"), "3 expected an expression, found `>`", false);
    as_it_was(&like("<input hidden={n > 0/>"), "3 expected an expression, found `>`", false);
    as_it_was(&like("<input value={n/>"), "3 expected an expression, found `>`", false);
    as_it_was(&like("<p hidden={f(n) > g(0}>x</p>"), "3 expected `)` after arguments, found `}`", false);
    as_it_was(&like("<p hidden={xs[0] > ys[1}>x</p>"), "3 expected `]`, found `}`", false);
    as_it_was(&like("<input hidden={f(n) > g(0}/>"), "3 expected `)` after arguments, found `}`", false);
    as_it_was(&like("<button onclick={() => { n += 1 } && go(n}>Go</button>"), "3 expected `)` after arguments, found `}`", false);
    // `#id` shorthand after a comparison (the second review's), markup in a prop before `/>`.
    as_it_was(&like("<p hidden={n > 0 #main>t</p>"), "3 unexpected character `#`", true);
    let card = |v: &str| format!("component Card(icon) {{\n    <p>{{icon}}</p>\n}}\nroute GET / = <Card icon={{{}/>\n", v);
    as_it_was(&card("<b>hi</b>"), "4 expected an expression, found `>`", false);
    as_it_was(&card("<br/>"), "4 expected an expression, found `>`", false);
    // A value that is no handler: a string, a map, the `{` closed with a missing operand.
    as_it_was(&like("<p style={{color: \"red\"}>♥ {n}</p>"), "3 unexpected character `♥`", false);
    as_it_was(&like("<button title={\"like\">♥ {n}</button>"), "3 unexpected character `♥`", false);
    as_it_was(&like("<button disabled={n > 3 && }>Go</button>"), "3 expected an expression, found `}`", false);
    // A mistake in the handler before its block's end; two braces open; a map's comma left out.
    as_it_was(&like("<button onclick={() => { n +* 1 }>♥ {n}</button>"), "3 expected an expression, found `*`", false);
    as_it_was(&like("<button onclick={() => { n += 1 >♥ {n}</button>"), "3 unexpected character `♥`", false);
    as_it_was(&like("<p title={f({a: 1 b: 2}) title2=\"x\">♥</p>"), "3 expected `,` or `}` in map, found `b`", true);
    as_it_was(&page("on_like={() => { n +* 1 }/>"), "6 expected an expression, found `*`", false);
    // The end of the input inside the text: as it was, for the REPL, which reads on after it.
    as_it_was(&like("<button onclick={() => { n += 1 }>Don't</button>"), "3 unexpected end of input: unterminated string", false);
    // A child's `{` left open: not an attribute's. A handler whose block holds a name alone, read
    // as the function and not its body's block: as it was.
    as_it_was(&like("<p>{n > 0 ? \"a\" : \"b\"</p>"), "3 expected an expression, found `/`", false);
    as_it_was(&like("<button onclick={() => { go }>♥</button>"), "3 unexpected character `♥`", false);
    // What is right stays right: comparisons, `=>`, `>=`, a `>` in a string or a template, markup
    // in an attribute, and a lambda's block closed.
    clean(&like(
        "<div><button onclick={() => { n += 1 }}>♥ {n}</button><p hidden={n > 3} title={n >= 1 ? \"a\" : \"b\"}>{n}</p><p title={\"a>b\"} data-y={`x>${n}`}>ok</p></div>",
    ));
    clean("fn f(x: int) -> int = x\ncomponent Card(icon) {\n    <p>{icon}</p>\n}\nroute GET / = <Card icon={<b>hi</b>}/>\n");
}

// ---- Q-1371, round 4 (the third review's findings) ------------------------------------------

#[test]
fn a_handler_holding_a_block_of_its_own_is_told_but_not_fixed() {
    // An `if`, a loop, a map, a lambda, a `match` or markup with a `{` in the handler: the `}` left
    // out may be that one's (`{ if n > 0 { n -= 1 n += 2 }>`), and a `}` at the tag's `>` would make
    // another program, which passes check. The message, but no fix.
    let a01 = "component Like() {\n    state n = 0\n    <button onclick={() => {\n        if n > 0 {\n            n -= 1\n        n += 2\n    }}>♥</button>\n}\nroute GET / = <Like/>\n";
    says(a01, "7 `onclick={` on line 3, column 21 is never closed: add `}` before the tag's `>` on line 7; without it, that `>` is read as greater than and the text of <button> as code");
    no_fix(a01);
    let a02 = "component Like() {\n    state n = 0\n    state xs = [1, 2]\n    <button onclick={() => {\n        for x in xs {\n            n += x\n        n += 1\n    }}>♥</button>\n}\nroute GET / = <Like/>\n";
    says(a02, "8 `onclick={` on line 4, column 21 is never closed: add `}` before the tag's `>` on line 8; without it, that `>` is read as greater than and the text of <button> as code");
    no_fix(a02);
    for line in [
        "<button onclick={() => { if n > 0 { n -= 1; n += 2 }}>♥</button>",
        "<button onclick={() => { let z = { a: n } }>♥</button>",
        "<button onclick={() => { [1].each((x) => { n += x }) }>♥</button>",
        "<button onclick={() => { match n { 0 => go(1) _ => go(2) } }>♥</button>",
        "<button onclick={() => { show(<b title={n}>x</b>) }>♥</button>",
        "<button onclick={() => { if n > 0 { n = 0 } }>=</button>",
    ] {
        says(&like(line), &open_brace("onclick", 21, "button"));
        no_fix(&like(line));
    }
    // The map's own `}` left out, the handler's then closing it: as it was.
    as_it_was(&like("<button onclick={() => { let z = { a: n }>♥</button>"), "3 unexpected character `♥`", false);
    // A template's `${` is no block (one left open is the lexer's error, before any `>`), nor is a
    // default parameter's arrow the handler's block: the fix is made.
    assert_eq!(fixed_at(&like("<button onclick={() => { print(`x>${n}`) }>♥</button>")), (3, 46));
    assert_eq!(fixed_at(&like("<button onclick={(f = () => { 1 }) => { n += f() }>♥</button>")), (3, 54));
}

#[test]
fn a_block_compared_in_a_value_is_the_programs_own_once_its_right_side_is_read() {
    // `(v) => { v * 2 } > 5` is a comparison a program may write (`[1, 5, 9].filter(...)`): once
    // its right side is read, its `>` is no tag's end. The `{` left open after it is then said as
    // the parser found it, with no fix.
    let b01 = "component List(keep, children) {\n    <ul>{children}</ul>\n}\nroute GET / = <List keep={(v) => { v * 2 } > 5>items</List>\n";
    said(b01, &wanted(4, "keep", 26, "`<`"), false);
    no_fix(b01);
    said(&like("<ul data-keep={(v) => { v * 2 } > 5>items</ul>"), &wanted(3, "data-keep", 19, "`<`"), false);
    no_fix(&like("<ul data-keep={(v) => { v * 2 } > 5>items</ul>"));
    // A `)` left out after one: as it was.
    as_it_was(&like("<button onclick={() => { n = [1, 2].filter((v) => { v * 2 } > 3.len() }}>♥</button>"), "3 expected `)` after arguments, found `}`", false);
    // One in the element's text on the next line, read after the tag's `>`: the tag's comparison
    // has read a whole value (`{...}`) before the parser stops, or completes; either is as it was,
    // on the line where the parser stopped, and gives no fix (round 4: the note is the tag's only
    // where its right side stops at its first token).
    as_it_was(
        "component Like() {\n    state n = 0\n    <button onclick={() => { n += 1 }>\n        {[1].filter((x) => { x * 2 } > 1)} + ♥\n    </button>\n}\nroute GET / = <Like/>\n",
        "4 unexpected character `♥`",
        false,
    );
    as_it_was(
        "component Like() {\n    state n = 0\n    <button onclick={() => { n += 1 }>\n        {[1].filter((x) => { x * 2 } > 1)}\n    </button>\n}\nroute GET / = <Like/>\n",
        "5 expected an expression, found `/`",
        false,
    );
    // A block compared, its value's `}` left out before a tag's `/>` (`> 5/>`), or the block's own
    // `}` left out (`{ x * 2 > 5}>`): as it was, as a fix would make another program (round 4; a
    // block whose value is the point is compared by a program as written, round 5).
    let card = |v: &str| format!("component Card(keep) {{\n    <p>x</p>\n}}\nroute GET / = <Card keep={{{}\n", v);
    as_it_was(&card("(x) => { x * 2 } > 5/>"), "4 expected an expression, found `>`", false);
    as_it_was(&card("(x) => { x * 2 > 5}>=</Card>"), "4 expected an expression, found `<`", false);
    // A ternary's look ahead that read a block compared leaves no note behind.
    said(&like("<p title={x ? () => { a; b } > 1 y}>t</p>"), &wanted(3, "title", 14, "`=>`"), false);
}

#[test]
fn a_brace_wanted_keeps_the_newer_quillo_note_of_what_was_found() {
    // `as` is no word Quillo has: the note follows, as it did without the `{` named.
    said(&like("<p title={n as str}>t</p>"), &wanted(3, "title", 14, "`as`"), true);
    assert!(quillo::error::may_be_newer_syntax("`title={` on line 3, column 14: expected `}` to close it, found `as`"));
    assert!(!quillo::error::may_be_newer_syntax("`title={` on line 3, column 14: expected `}` to close it, found `<`"));
    assert!(!quillo::error::may_be_newer_syntax("`title={` on line 3, column 14: expected `}` to close it, found end of input"));
}

// ---- Q-1371, round 5 (the fourth review's findings) -------------------------------------------

/// A page whose component `List` takes `keep`, given on line 4 as `keep={` + `value`.
fn list(value: &str) -> String {
    format!("component List(keep, children) {{\n    <ul>{{children}}</ul>\n}}\nroute GET / = <List keep={{{}}}>items</List>\n", value)
}

#[test]
fn a_block_compared_while_its_right_side_is_typed_is_as_it_was() {
    // A block whose value is the point, compared with its right side left out, cut, or written
    // another language's way: the error is the one the program has, said as before, with no fix.
    for (value, want, note) in [
        ("(p) => { p.n } > ", "4 expected an expression, found `}`", false),
        ("(p) => { p.n } > )", "4 expected an expression, found `)`", false),
        ("(p) => { p.n } > $5", "4 unexpected character `$`", true),
        ("(p) => { p.n } > == 5", "4 expected an expression, found `==`", false),
        ("(p) => { p.n } > “5”", "4 unexpected character `“`, a curly quote: write `\"`", false),
        ("(p) => { p.n } > .5", "4 a number starts with a digit: `0.5`, not `.5`", false),
    ] {
        as_it_was(&list(value), want, note);
        no_fix(&list(value));
    }
    // A handler's `}` left out with a closer next (`}> }` while it is typed): no tag's `>` there.
    as_it_was(&like("<button onclick={() => { n += 1 }> }</button>"), "3 expected an expression, found `}`", false);
    // In a handler, a lambda's block compared with its right side left out: the `{` around is
    // closed, and the error is as it was.
    as_it_was(&like("<button onclick={() => { n = xs.filter((v) => { v * 2 } > ).len() }}>Go</button>"), "3 expected an expression, found `)`", false);
}

#[test]
fn an_element_whose_text_holds_its_end_tag_without_the_lt_gets_no_end_tag_fix() {
    // `<b>hi/b>`: its end tag with the `<` left out, which `</b>` at the end would leave as text.
    let Err(e) = quillo::eval_str("route GET / = <b>hi/b>\n") else { panic!("an error") };
    assert_eq!((e.msg.as_str(), e.fix.is_none()), ("`<b>` is never closed: add `</b>` at the end of the file", true));
    let Err(e) = quillo::eval_str("route GET / = <p><b>hi/b> there</p>\n") else { panic!("an error") };
    assert_eq!((e.msg.as_str(), e.fix.is_none()), ("`<b>` is never closed: add `</b>` before `</p>` on line 1", true));
    // Nor the change of another element's end tag into its own (found by the same fuzzer).
    let Err(e) = quillo::eval_str("route GET / = <b>hi/b> x</i>\n") else { panic!("an error") };
    assert_eq!((e.msg.as_str(), e.fix.is_none()), ("`</i>` does not close `<b>` from line 1: expected `</b>`", true));
    // (An end tag that is an element's name, `</i>`, is no longer renamed: round 7, the start tag
    // may be the one mistyped. One that is no element's is.)
    let Err(e) = quillo::eval_str("route GET / = <b>hi x</xb>\n") else { panic!("an error") };
    assert_eq!(e.fix.map(|f| f.title), Some("Change to </b>".to_string()));
    // Without it, and with a `/b>` that has its `<` (an inner element's end): the fix stays.
    let Err(e) = quillo::eval_str("route GET / = <b>hi\n") else { panic!("an error") };
    assert_eq!(e.fix.map(|f| f.text), Some("</b>\n".to_string()));
    let Err(e) = quillo::eval_str("route GET / = <b>a<i>x</i> <b>y</b>\n") else { panic!("an error") };
    assert!(e.fix.is_some(), "{}", e.msg);
}

#[test]
fn a_quick_fix_in_a_file_of_lone_carriage_returns_goes_at_its_column() {
    use std::io::{BufRead, BufReader, Write};
    // An old Mac file: lines ended by `\r` alone, which the parser and an editor both count.
    let text = "component Like() {\r    state n = 0\r    <button onclick={() => { n += 1 }>♥ {n}</button>\r}\rroute GET / = <Like/>\r";
    let dir = support::temp_dir(&format!("f8689-parse-cr-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("lsp").current_dir(&dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel::<serde_json::Value>();
    let reader = std::thread::spawn(move || loop {
        let mut len = 0;
        loop {
            let mut line = String::new();
            if out.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0; len];
        if std::io::Read::read_exact(&mut out, &mut buf).is_err() || tx.send(serde_json::from_slice(&buf).unwrap()).is_err() {
            return;
        }
    });
    let mut stdin = child.stdin.take().unwrap();
    let mut send = |m: serde_json::Value| {
        let body = m.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        stdin.flush().unwrap();
    };
    // Each answer within a deadline, far past a slow debug build's; past it the server is stopped.
    let deadline = Instant::now() + Duration::from_secs(300);
    let wait_for = |want: &dyn Fn(&serde_json::Value) -> bool| -> serde_json::Value {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(m) if want(&m) => return m,
                Ok(_) => {}
                Err(_) => panic!("quillo lsp did not answer in time"),
            }
        }
    };
    let uri = format!("file://{}/main.qwe", dir.display());
    send(serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}));
    wait_for(&|m| m["id"] == 1);
    send(serde_json::json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
    send(serde_json::json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "languageId": "quillo", "version": 1, "text": text}}}));
    let diags = wait_for(&|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri.as_str());
    let d = diags["params"]["diagnostics"][0].clone();
    assert_eq!(d["range"]["start"]["line"], 2, "{}", d);
    send(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction", "params": {"textDocument": {"uri": uri}, "range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 0}}, "context": {"diagnostics": [d]}}}));
    let actions = wait_for(&|m| m["id"] == 2);
    let edit = actions["result"][0]["edit"]["changes"][uri.as_str()][0].clone();
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!((edit["range"]["start"]["line"].clone(), edit["range"]["start"]["character"].clone(), edit["newText"].clone()), (serde_json::json!(2), serde_json::json!(37), serde_json::json!("}")), "{}", actions);
}

#[test]
fn an_end_tag_fix_is_offered_only_where_the_parser_takes_it() {
    // An end tag with its `/` left out, `<b>hi<b>`: a second `</b>` would not mend it (the
    // coordinator's decision of round 5, three groups the fourth review's fuzzer found).
    let Err(e) = quillo::eval_str("route GET / = <div title={<b>hi<b>}>x</div>\n") else { panic!("an error") };
    assert_eq!((e.msg.as_str(), e.fix.is_none()), ("`<b>` is never closed: add `</b>` before the `}` on line 1 (write {\"}\"} for the character)", true));
    // An end tag with its name left out, `</>`, and the value's `}` too: the change would leave the
    // `{` open.
    let Err(e) = quillo::eval_str("route GET / = <div hidden={<b>hi</> title=\"x\">t</div>\n") else { panic!("an error") };
    assert_eq!((e.msg.as_str(), e.fix.is_none()), ("`</>` does not close `<b>` from line 1: expected `</b>`", true));
    // A block's `{` left out, so that its `}` reads as the element's end: an end tag there would
    // not mend it.
    let src = "component Card(on, children) {\n    <p>{children}</p>\n}\ncomponent Like() {\n    <Card\n        on={(x) =>  x * 2 } > 5}\n    >t</Card>\n}\nroute GET / = <Like/>\n";
    let Err(e) = quillo::eval_str(src) else { panic!("an error") };
    assert_eq!((e.line, e.msg.as_str(), e.fix.is_none()), (5, "`<Card>` is never closed: add `</Card>` before the `}` on line 6 (write {\"}\"} for the character)", true));
    // A self-closing tag ended early by a `>` (a block's `{` and the value's `}` left out): its `/>`
    // is left in the element's text, where an end tag added after would make another program.
    let src = "component Like() {\n    <div data-y={(p) =>  p.n } > 5 title={() => { go() }}/>\n}\nfn go() = 1\nroute GET / = <Like/>\n";
    let Err(e) = quillo::eval_str(src) else { panic!("an error") };
    assert!(e.fix.is_none(), "{} {:?}", e.msg, e.fix.map(|f| f.text));
    // Where the parser takes it, the fix stays: an element inside another left open, and the name
    // left out with the value's `}` in place.
    let Err(e) = quillo::eval_str("route GET / = <i><b>x</i>\n") else { panic!("an error") };
    assert_eq!(e.fix.map(|f| (f.title, f.text)), Some(("Add </b>".to_string(), "</b>".to_string())));
    let Err(e) = quillo::eval_str("route GET / = <div hidden={<b>hi</>}>t</div>\n") else { panic!("an error") };
    assert_eq!(e.fix.map(|f| f.title), Some("Change to </b>".to_string()));
}

#[test]
fn a_file_that_parses_judges_no_fix_and_its_parse_stays_linear() {
    // A look ahead (a ternary's, after `x? <y`) whose error is thrown away judged that error's
    // end-tag fix by parsing the file again each time: a valid file's parse went quadratic (round 7,
    // found in the sixth review). The shape the review timed, at a size and at twice it.
    let src = |n: usize| (0..n).map(|k| format!("fn f{k}(x, y, w) {{\n    let ok = x? <y\n    let _z = w > 1\n    print(\"</q>\")\n    return ok\n}}\n")).collect::<String>();
    let (small, large) = (src(1000), src(2000));
    let before = quillo::parser::trial_parses();
    quillo::parser::Parser::new(&small).parse_program().expect("it parses");
    assert_eq!(quillo::parser::trial_parses(), before, "a file that parses judged a fix");
    let time = |s: &str| {
        (0..3)
            .map(|_| {
                let t = Instant::now();
                quillo::parser::Parser::new(s).parse_program().expect("it parses");
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let (a, b) = (time(&small), time(&large));
    assert!(b < a * 3 + Duration::from_millis(5), "twice the file took {:?} against {:?}", b, a);
}

#[test]
fn an_end_tag_fix_is_kept_only_where_the_file_then_parses_and_in_its_own_line_ending() {
    // After the fix, the file still failed (on the element's own line, which the oracle let by),
    // and a second fix would put text inside the wrong element (round 7, found in the sixth review).
    for src in [
        "route GET / = <ul>\n    <em>\n        <i>a</i>\n    <em>b</ul>\n",
        "route GET / = <div>\n    <b>\n        1}\n    </b>\n</div>\n",
    ] {
        let Err(e) = quillo::eval_str(src) else { panic!("an error\n{}", src) };
        assert!(e.fix.is_none(), "{} {:?}\n{}", e.msg, e.fix.map(|f| (f.line, f.col, f.text)), src);
    }
    // A start tag whose name was mistyped (`<pan>`, `<ba>`: a letter or its `>` left out) is not
    // renamed into by its end tag; a mistyped end tag still is.
    for src in ["route GET / = <p><pan>x</span></p>\n", "route GET / = <p><ba</b></p>\n", "route GET / = <div><u>x</ul></div>\n", "route GET / = <ul><li>a</lia b</li></ul>\n"] {
        let Err(e) = quillo::eval_str(src) else { panic!("an error\n{}", src) };
        assert!(e.fix.as_ref().map_or(true, |f| !f.title.starts_with("Change to")), "{} {:?}", e.msg, e.fix.map(|f| f.title));
    }
    let Err(e) = quillo::eval_str("route GET / = <p><button>x</buton></p>\n") else { panic!("an error") };
    assert_eq!(e.fix.map(|f| f.title), Some("Change to </button>".to_string()));
    // In a file of `\r\n` or a lone `\r`, an end tag added on a line of its own ends as the file's do.
    for eol in ["\r\n", "\r"] {
        let src = format!("route GET / = <div>{eol}  <p>{eol}</div>{eol}");
        let Err(e) = quillo::eval_str(&src) else { panic!("an error") };
        assert_eq!(e.fix.map(|f| f.text), Some(format!("  </p>{eol}")));
    }
}

#[test]
fn an_end_tag_is_renamed_only_from_an_elements_name_to_none() {
    // A component's name, an SVG or MathML name mistyped, an older HTML name: either tag may be the
    // one mistyped, so no rename (round 8, found in the seventh review).
    for src in [
        "route GET / = <my-crad>hi</my-card>\n",
        "route GET / = <my-card>hi</my-crad>\n",
        "route GET / = <svg><stopp offset=\"0\"></stop></svg>\n",
        "route GET / = <math><mrw>x</mrow></math>\n",
        "route GET / = <cnter>x</center>\n",
        "route GET / = <svg><radialGradient id=\"g\"></radialGradiant></svg>\n",
    ] {
        let Err(e) = quillo::eval_str(src) else { panic!("an error\n{}", src) };
        let rename = e.fix.as_ref().filter(|f| f.title.starts_with("Change to")).map(|f| f.title.clone());
        let want = src.contains("radialGradient").then(|| "Change to </radialGradient>".to_string());
        assert_eq!(rename, want, "{}\n{}", e.msg, src);
    }
    for (src, title) in [("route GET / = <p><button>x</buton></p>\n", "Change to </button>"), ("route GET / = <center>x</cnter>\n", "Change to </center>")] {
        let Err(e) = quillo::eval_str(src) else { panic!("an error") };
        assert_eq!(e.fix.map(|f| f.title), Some(title.to_string()), "{}", src);
    }
}

#[test]
fn markup_inside_a_templates_part_gets_no_fix() {
    // The part is parsed on its own text, so a fix's column was the part's and no parse of the
    // file judged it: none is offered there (round 8, found in the seventh review).
    for src in [
        "route GET / = `x ${<b>hi} y`\n",
        "route GET / = <div>{`a ${<i>b</p>}`}</div>\n",
        "route GET / = `x ${<b>hi</zz>} y`\n",
        "route GET / = `x ${<p>a</br></p>} ${<b>hi</br>} y`\n",
    ] {
        no_fix(src);
    }
}

#[test]
fn an_end_tag_removed_only_where_the_file_then_parses() {
    // Removing `</br>` from `<b>hi</br>` leaves `<b>` open (round 8, found in the seventh review).
    for src in ["route GET / = <b>hi</br>\n", "route GET / = <i>x</img>\n"] {
        no_fix(src);
    }
    let Err(e) = quillo::eval_str("route GET / = <p>a</br></p>\n") else { panic!("an error") };
    assert_eq!(e.fix.map(|f| (f.title, f.col, f.len)), Some(("Remove </br>".to_string(), 18, 5)));
}

#[test]
fn an_added_end_tag_line_ends_as_its_own_line_does_in_a_file_of_mixed_endings() {
    // The first line's ending was used for all (round 8, found in the seventh review).
    for (first, rest) in [("\n", "\r\n"), ("\r\n", "\n"), ("\n", "\r")] {
        let src = format!("route GET / = <div>{first}  <p>{rest}</div>{rest}");
        let Err(e) = quillo::eval_str(&src) else { panic!("an error") };
        assert_eq!(e.fix.map(|f| f.text), Some(format!("  </p>{rest}")), "{:?}", src);
    }
}

#[test]
fn an_attributes_brace_in_a_templates_part_is_named_at_its_column_in_the_file() {
    // Its column was counted from the part's start where the `{` is on the part's first line
    // (round 9, found in the eighth review). One line, the part over two lines, a part on a later
    // line, in markup's value, in a template's part; in `\n` and `\r\n` files.
    let cases = [
        ("route GET / = `xxxxx ${<p title={a b}>hi</p>} y`\n", 1, 33),
        ("route GET / = `xx ${\n  <p title={a b}>hi</p>} y`\n", 2, 12),
        ("route GET / = `xx ${f(1,\n  <p title={a b}>hi</p>)} y`\n", 2, 12),
        ("fn f() {\n    return 1\n}\nroute GET / = `ab ${<p title={a b}>hi</p>}`\n", 4, 30),
        ("route GET / = <div>{`a ${<p title={a b}>x</p>}`}</div>\n", 1, 35),
        ("route GET / = `a ${`b ${<p title={a b}>x</p>}`}`\n", 1, 34),
    ];
    for (src, line, col) in cases {
        for src in [src.to_string(), src.replace('\n', "\r\n")] {
            let (got, _) = first_error(&src);
            let want = format!("{line} `title={{` on line {line}, column {col}: expected `}}` to close it, ");
            assert!(got.starts_with(&want), "{:?}\nwanted {:?}\n{:?}", got, want, src);
        }
    }
}

#[test]
fn an_attributes_brace_in_templates_inside_values_and_templates_is_named_at_its_column() {
    // A template in an attribute's value in a template's part: the value's `{` set the place the
    // part's template looked for in the words, so the column stayed the part's (round 10, found
    // in the ninth review). Nested three deep, in values and in parts; characters of two, three
    // and four bytes and a tab before the part; in `\n`, `\r\n` and `\r` files.
    let cases = [
        ("route GET / = `zz ${<div title={`a ${<p title={a b}>x</p>}`}>y</div>}`\n", 1, 47),
        ("route GET / = `zz ${<div>{`a ${<p title={a b}>x</p>}`}</div>}`\n", 1, 41),
        ("route GET / = `a ${`b ${`c ${<p title={a b}>x</p>}`}`}`\n", 1, 39),
        ("route GET / = `a ${<i t={`b ${<u t={`c ${<p title={a b}>x</p>}`}>y</u>}`}>z</i>}`\n", 1, 51),
        ("route GET / = `a ${<i t={`b\n ${<u t={`c ${<p title={a b}>x</p>}`}>y</u>}`}>z</i>}`\n", 2, 24),
        ("route GET / = `\u{e9}\u{e9}\u{20ac}\u{1F600} ${<p title={a b}>hi</p>}`\n", 1, 32),
        ("route GET / = `\t ${<p title={a b}>hi</p>}`\n", 1, 29),
    ];
    for (src, line, col) in cases {
        for src in [src.to_string(), src.replace('\n', "\r\n"), src.replace('\n', "\r")] {
            let (got, _) = first_error(&src);
            let want = format!("{line} `title={{` on line {line}, column {col}: expected `}}` to close it, ");
            assert!(got.starts_with(&want), "{:?}\nwanted {:?}\n{:?}", got, want, src);
        }
    }
}

#[test]
fn an_attributes_brace_in_a_script_in_a_templates_part_is_named_at_its_column() {
    // The note a <script>'s `{` adds made a new error without the place, which the part's
    // template then did not move (round 11, found in the tenth review).
    let b = "<p title={a b}>q</p>";
    let cases = [
        (format!("route GET / = `xxxx ${{<script>{{{b}}}</script>}}`\n"), 1, 41),
        (format!("route GET / = `a ${{<div t={{`b ${{<script>{{{b}}}</script>}}`}}>z</div>}}`\n"), 1, 51),
        (format!("route GET / = `a ${{`b ${{<script>\n{{{b}}}</script>}}`}}`\n"), 2, 11),
    ];
    for (src, line, col) in cases {
        for src in [src.clone(), src.replace('\n', "\r\n"), src.replace('\n', "\r")] {
            let (got, _) = first_error(&src);
            let want = format!("{line} `title={{` on line {line}, column {col}: expected `}}` to close it, found `b` (a `{{` in a <script> starts a Quillo value");
            assert!(got.starts_with(&want), "{:?}\nwanted {:?}\n{:?}", got, want, src);
        }
    }
}

#[test]
fn a_templates_part_on_line_one_keeps_a_byte_order_mark_or_hash_bang_as_its_text() {
    // Its parser took them away as a file's, so a column after them was one short and
    // `${U+FEFF 1}` was `1` (round 11, found in the tenth review). Now as on any other line: an
    // unexpected character.
    let bom = "unexpected character U+FEFF (a byte-order mark), which cannot be seen: delete it";
    for src in ["route GET / = `${\u{FEFF}<p title={a b}>x</p>}`\n", "route GET / = `a ${\u{FEFF}`b ${<p title={a b}>x</p>}`}`\n", "route GET / = `${\u{FEFF}1}`\n"] {
        assert_eq!(first_error(src).0, format!("1 {bom}"), "{:?}", src);
        assert_eq!(first_error(&src.replacen("route", "x()\nroute", 1)).0, format!("2 {bom}"), "{:?}", src);
    }
    for src in ["route GET / = `${#!x\n<p title={a b}>x</p>}`\n", "route GET / = `${#!x\n1}`\n"] {
        assert_eq!(first_error(src).0, "1 unexpected character `#`", "{:?}", src);
    }
    // A file's own byte-order mark is still taken away.
    let (got, _) = first_error("\u{FEFF}route GET / = `${<p title={a b}>x</p>}`\n");
    assert!(got.starts_with("1 `title={` on line 1, column 27:"), "{:?}", got);
}

#[test]
fn a_shebang_line_ends_at_a_lone_carriage_return() {
    // The whole file was read as its `#!` line: "ok (0 routes)" (round 9, found in the eighth
    // review; as 0.8.11 had it).
    for eol in ["\r", "\r\n", "\n"] {
        let src = format!("#!/usr/bin/env quillo{eol}let x = 3 ♥ 4{eol}");
        says(&src, "2 unexpected character `♥`");
    }
}

// ---- Q-008 seen again: the note after a character no Quillo takes in code -------------------

#[test]
fn a_character_outside_ascii_has_no_newer_quillo_note() {
    // Its message as it was, with no note: a symbol, an arrow, a currency, an emoji (four bytes),
    // a digit of another script.
    for c in ['♥', '→', '€', '😀', '٣'] {
        says(&format!("let x = 3 {} 4\n", c), &format!("1 unexpected character `{}`", c));
        assert!(!quillo::error::may_be_newer_syntax(&format!("unexpected character `{}`", c)));
    }
    // In a tag's text read as code, with two `{` left open.
    let (got, note) = first_error(&like("<button onclick={() => { n += 1 >♥ {n}</button>"));
    assert_eq!((got.as_str(), note), ("3 unexpected character `♥`", false));
    // One in ASCII that Quillo does not take yet may still be a newer Quillo's; a letter outside
    // ASCII is a name's.
    for c in ['#', '$', '^'] {
        let (got, note) = first_error(&format!("let x = 3 {} 4\n", c));
        assert_eq!((got, note), (format!("1 unexpected character `{}`", c), true));
    }
    clean("let é = 1\nprint(é)\n");
}

