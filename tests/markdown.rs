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

#[test]
fn code_that_names_its_language_is_highlighted_and_stays_escaped() {
    let t = md(r#"markdown("```js\nlet a = \"<script>\" // hi\n```\n\nafter")"#);
    assert_eq!(
        t,
        "<pre class=\"hl\"><code class=\"language-js\"><span class=\"hl-k\">let</span> a = <span class=\"hl-s\">&quot;&lt;script&gt;&quot;</span> <span class=\"hl-c\">// hi</span>\n</code></pre>\n<p>after</p>\n"
    );
    // A block that names no language is as CommonMark has it.
    assert_eq!(md(r#"markdown("```\n<b>\n```")"#), "<pre><code>&lt;b&gt;\n</code></pre>\n");
    assert_eq!(md(r#"type_of(highlight("x", "qwe"))"#), "html");
}

#[test]
fn inline_markdown_keeps_the_line_and_drops_the_blocks() {
    assert_eq!(md(r#"markdown("A **bold** line, a [link](/x)", { inline: true })"#), r#"A <strong>bold</strong> line, a <a href="/x">link</a>"#);
    // No headings, lists or paragraphs: their lines, broken; items keep their marks, as text.
    assert_eq!(md(r##"markdown("# Not a heading\n\n- nor a list", { inline: true })"##), "Not a heading<br>• nor a list");
    assert_eq!(md(r#"markdown("1. One\n2. Two", { inline: true })"#), "1. One<br>2. Two");
    // As safe as the rest.
    assert_eq!(md(r#"markdown("<b>x</b> [y](javascript:alert(1))", { inline: true })"#), r#"&lt;b&gt;x&lt;/b&gt; <a href="about:blank#blocked">y</a>"#);
}
