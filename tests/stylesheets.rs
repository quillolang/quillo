//! Components' `style`: what it takes, the command `style_with` names, the
//! sheets kept for pages, and how selectors are scoped.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::styles::{ids_in, scope, set_elsewhere};
use quillo::Interp;

fn load(dir: &std::path::Path, src: &str) -> Result<Interp, String> {
    let mut it = Interp::new(dir);
    it.load_source(src, "app.qwe").map_err(|e| e.to_string())?;
    Ok(it)
}

fn app(src: &str) -> Interp {
    load(&std::env::temp_dir(), src).unwrap_or_else(|e| panic!("{}\n{}", e, src))
}

fn get(it: &mut Interp, url: &str) -> HttpResponse {
    let h = vec![("Host".to_string(), "x".to_string())];
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() })
}

#[track_caller]
fn refused(src: &str, wanted: &str) {
    match load(&std::env::temp_dir(), src) {
        Ok(_) => panic!("expected an error\n{}", src),
        Err(e) => assert!(e.contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e),
    }
}

/// `css` scoped, on one line.
fn scoped(css: &str) -> String {
    scope(css, "ab12cd34").split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_style_must_be_css_text() {
    refused("let n = 5\ncomponent Card() {\n style n\n <p>x</p>\n}\n", "`style` in `Card` takes CSS text (css`...`), got int");
    refused("let l = [1]\nview Card() {\n style l\n <p>x</p>\n}\n", "`style` in `Card` takes CSS text (css`...`), got list");
    refused("component Card() {\n style missing_name\n <p>x</p>\n}\n", "unknown name `missing_name`");
    refused("component Card() {\n style reply 200 \"x\"\n <p>x</p>\n}\n", "`style` in `Card` must be CSS text");
}

const TWO: &str = r#"
component Card() {
    style css`.t { @apply bold; }`
    <p.t>card</p>
}
view Panel() {
    style css`.u { color: red }`
    <p.u>panel</p>
}
route GET / = <html><body><Panel/></body></html>
"#;

#[test]
fn style_with_runs_the_command_on_every_style() {
    let src = format!("style_with(\"sed 's/@apply bold;/font-weight: 700;/'\")\n{}", TWO);
    let it = app(&src);
    let all = it.styles.lock().unwrap().all();
    assert!(all.contains("font-weight: 700;") && !all.contains("@apply"), "{}", all);
    assert!(all.contains("color: red"), "{}", all);
    // Loaded again with the same CSS, what was made before is used, and is the same.
    let again = app(&src).styles.lock().unwrap().all();
    assert_eq!(all, again);
}

#[test]
fn style_with_says_when_its_command_fails() {
    let src = format!("style_with(\"echo nothing here >&2; exit 1\")\n{}", TWO);
    let e = load(&std::env::temp_dir(), &src).err().expect("the load fails");
    assert!(e.contains("style_with `echo nothing here >&2; exit 1` failed: nothing here"), "{}", e);
}

#[test]
fn style_with_has_nothing_to_do_without_a_style() {
    // A command that would fail, had it been run.
    let it = app("style_with(\"exit 1\")\nroute GET / = \"ok\"\n");
    assert!(it.styles.lock().unwrap().is_empty());
    assert!(it.styles.lock().unwrap().ids().is_empty());
}

#[test]
fn a_page_with_styles_gets_their_sheet() {
    let mut it = app(TWO);
    assert!(!it.styles.lock().unwrap().is_empty());
    let page = get(&mut it, "/").text();
    assert!(page.contains("/_quillo/s/"), "{}", page);
    let at = page.find("/_quillo/s/").unwrap();
    let url: String = page[at..].chars().take_while(|c| *c != '"').collect();
    let sheet = get(&mut it, &url);
    assert_eq!(sheet.status, 200);
    assert!(sheet.text().contains("color: red"), "{}", sheet.text());
}

#[test]
fn the_same_styles_share_a_sheet_and_old_sheets_go() {
    let parts: String = (0..10).map(|i| format!("component C{i}() {{\n style css`.c{i} {{ order: {i} }}`\n <p.c{i}>x</p>\n}}\n")).collect();
    let it = app(&parts);
    let mut styles = it.styles.lock().unwrap();
    let ids = styles.ids();
    assert_eq!(ids.len(), 10);
    let first = styles.sheet_for(&ids[..2]).unwrap();
    assert_eq!(styles.sheet_for(&ids[..2]), Some(first.clone()), "the same set");
    let backwards: Vec<String> = ids[..2].iter().rev().cloned().collect();
    assert_eq!(styles.sheet_for(&backwards), Some(first.clone()), "in any order");
    assert_eq!(styles.sheet_for(&["no-such-id".to_string()]), None);
    assert!(styles.by_hash(&first).is_some());
    // 300 other sets, past the 256 kept: the oldest goes, the last stays.
    let mut last = None;
    for set in 1..=300usize {
        let chosen: Vec<String> = ids.iter().enumerate().filter(|(i, _)| set >> i & 1 == 1).map(|(_, s)| s.clone()).collect();
        if chosen.len() == 2 && chosen[..2] == ids[..2] {
            continue;
        }
        last = styles.sheet_for(&chosen);
    }
    assert!(styles.by_hash(&first).is_none(), "the sheet not used for longest is gone");
    assert!(styles.by_hash(&last.unwrap()).is_some());
    assert!(styles.all().contains("order: 9"), "every style is still there for a page with an old sheet");
}

#[test]
fn selectors_with_quotes_and_brackets() {
    assert_eq!(scoped("a[title=\"x y\"] { c: 1 }"), "a[title=\"x y\"][data-q-ab12cd34] { c: 1 }");
    assert_eq!(scoped("a[title='p > q'] b { c: 1 }"), "a[title='p > q'] b[data-q-ab12cd34] { c: 1 }");
    assert_eq!(scoped("a[title=\"p:q\"]:hover { c: 1 }"), "a[title=\"p:q\"][data-q-ab12cd34]:hover { c: 1 }");
    assert_eq!(scoped("a[href]:not(.x:y) { c: 1 }"), "a[href][data-q-ab12cd34]:not(.x:y) { c: 1 }");
}

#[test]
fn an_empty_selector_stays_empty() {
    assert_eq!(scoped(".a, { c: 1 }"), ".a[data-q-ab12cd34], { c: 1 }");
}

#[test]
fn a_rule_left_open_is_closed() {
    assert_eq!(scoped(".a { c: 1"), ".a[data-q-ab12cd34] { c: 1}");
}

#[test]
fn global_with_parentheses_inside_it() {
    assert_eq!(scoped(":global(.a:not(.b)) .c { x: 1 }"), ".a:not(.b) .c { x: 1 }");
    // The parenthesis never closes: what is written is kept.
    assert_eq!(scoped(":global(a] { x: 1 }"), "a] { x: 1 }");
}

#[test]
fn only_a_pages_own_style_marks_count() {
    assert_eq!(ids_in(b"<p data-q-xyz data-q-12 data-q-ab12cd34>"), vec!["ab12cd34"]);
    assert!(ids_in(b"data-q-").is_empty());
}

#[test]
fn a_comment_that_says_nothing_of_classes_is_passed_over() {
    assert_eq!(set_elsewhere("/* a note */ .x {} /* set elsewhere: map- */"), vec!["map-"]);
    assert!(set_elsewhere("/* a note").is_empty());
    assert!(set_elsewhere("/* set elsewhere: , */").is_empty());
}
