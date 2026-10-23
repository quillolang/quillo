//! `markdown`: Markdown as HTML, safe for what visitors write.

fn md(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.display().to_string(),
        Err(e) => panic!("{}", e),
    }
}

#[test]
fn markdown_becomes_html() {
    assert_eq!(md(r##"markdown("# Hi\n\nSome *text* and `code`.")"##), "<h1>Hi</h1>\n<p>Some <em>text</em> and <code>code</code>.</p>\n");
    assert_eq!(md(r#"type_of(markdown("x"))"#), "html");
    let t = md(r#"markdown("| a | b |\n|---|---|\n| 1 | ~~2~~ |\n\n- [x] done")"#);
    assert!(t.contains("<table>") && t.contains("<del>2</del>") && t.contains("checked=\"\""), "{}", t);
    // In a page it is markup, not escaped text.
    assert_eq!(md(r#"str(<div>{markdown("**b**")}</div>)"#), "<div><p><strong>b</strong></p>\n</div>");
}

#[test]
fn what_visitors_write_cannot_run_scripts() {
    let t = md(r#"markdown("<script>alert(1)</script>\n\nhi <img src=x onerror=alert(1)>")"#);
    assert!(!t.contains("<script") && !t.contains("<img"), "{}", t);
    assert!(t.contains("&lt;script&gt;"), "{}", t);
    let t = md(r#"markdown("[a](javascript:alert(1)) [b]( JaVaScRiPt:x) ![c](javascript:y) [d](/ok) <javascript:z>")"#);
    assert_eq!(t.matches("\"about:blank#blocked\"").count(), 4, "{}", t);
    assert!(!t.to_lowercase().contains("=\"javascript") && !t.to_lowercase().contains("=\" javascript"), "{}", t);
    assert!(t.contains("href=\"/ok\""), "{}", t);
    // Text you wrote yourself may keep its HTML.
    assert_eq!(md(r#"markdown("<b>x</b>", { html: true })"#), "<p><b>x</b></p>\n");
}
