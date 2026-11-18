//! Where the browser's parser is, HTML, SVG or MathML, as the server writes a page (src/foreign.rs):
//! the cautious turns of 0.8.7's walk that no test took. A `<script>` the parser reads as SVG's or
//! MathML's has an `html` value (`raw(...)`) in it put in a CDATA section; where the server cannot
//! be sure where the parser is (`unsure`), it writes the form that is safe in both. Each page here
//! is checked by the HTML the server sends: the section, or none where HTML is read (FEEDBACK71.md,
//! Q-969; coverage of 0.8.7; branch coverage-087).

mod support;

use quillo::server::HttpRequest;
use quillo::Interp;

/// A visitor's markup: a tag, an end tag, a character reference and the end of a CDATA section.
const Q: &str = "<b>x</b>&quot;]]>";
/// `Q` in a script the parser reads as markup: in a CDATA section, its `]]>` split between two.
const CODE: &str = r#"<![CDATA[<b>x<\/b>&quot;]]]]><![CDATA[>]]>"#;
/// `Q` in a script the parser reads as HTML: no section.
const HTML: &str = r#"<b>x<\/b>&quot;]]>"#;
/// `Q` written by the program in raw markup (no `</` turned into `<\/`), then put in a section.
const RAW_CODE: &str = "<![CDATA[<b>x</b>&quot;]]]]><![CDATA[>]]>";

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// The program, loaded once `quillo check` has nothing against it (no error, no warning).
fn app(src: &str) -> Interp {
    let a = quillo::check::check_source(src, "main.qwe", std::path::Path::new("."));
    let said: Vec<String> = a.diagnostics.iter().map(|d| format!("{}: {}", d.line, d.message)).collect();
    assert!(said.is_empty(), "quillo check: {:?}", said);
    let mut it = Interp::new(".");
    if let Err(e) = it.load_source(src, "main.qwe") {
        panic!("{}: {}", e.line, e.msg);
    }
    it
}

/// What a page's body holds before the scripts Quillo adds, for `path` with `q` set to `Q`.
fn page(it: &mut Interp, path: &str) -> String {
    let sep = if path.contains('?') { "&" } else { "?" };
    let url = format!("{}{}q={}", path, sep, enc(Q));
    let r = it.handle(&HttpRequest { method: "GET".into(), url: url.clone(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() });
    let text = r.text();
    assert_eq!(r.status, 200, "{}: {}", url, text);
    let from = text.find("<body>").map_or(0, |i| i + 6);
    let to = text[from..].find("<script>(()=>").or_else(|| text[from..].find("</body>")).map_or(text.len(), |i| from + i);
    text[from..to].to_string()
}

/// Each path's body, twice: a page's fixed parts and a view's answer are kept after the first.
fn pages(it: &mut Interp, cases: &[(&str, String)]) {
    for round in 0..2 {
        for (path, want) in cases {
            assert_eq!(&page(it, path), want, "{} (round {})", path, round + 1);
        }
    }
}

// ---- tags whose attributes a walk cannot see ---------------------------------------------------

const ASK: &str = r##"view Font(children, s: str) = <svg><font size={s}>{children}</font></svg>
view FontAfter(children, s: str) = <svg><font size={s}>x</font>{children}</svg>
view MathFont(children, s: str) = <math><font size={s}>{children}</font></math>
view Ann(children, e: str) = <math><annotation-xml encoding={e}>{children}</annotation-xml></math>
route GET /font ? { q: str = "" } = <html><body><Font s="3"><script>{raw(q)}</script></Font></body></html>
route GET /font-after ? { q: str = "" } = <html><body><FontAfter s="3"><script>{raw(q)}</script></FontAfter></body></html>
route GET /math-font ? { q: str = "" } = <html><body><MathFont s="3"><script>{raw(q)}</script></MathFont></body></html>
route GET /ann ? { q: str = "", e: str = "" } = <html><body><Ann e={e}><script>{raw(q)}</script></Ann></body></html>
route GET /font-here ? { q: str = "", s: str = "3" } = <html><body><svg><font size={s}><script>{raw(q)}</script></font></svg></body></html>
route GET /ann-here ? { q: str = "", e: str = "" } = <html><body><math><annotation-xml encoding={e}><script>{raw(q)}</script></annotation-xml></math></body></html>
"##;

/// A view's `<font size={s}>` inside `<svg>` or `<math>`, and its `<annotation-xml encoding={e}>`,
/// are known only once written: the walk that places the view's children cannot tell, so their
/// script is written as it is safe in both, with the CDATA section, whatever the value; written
/// in the page itself, the renderer reads the attribute and writes HTML's form where it is HTML
/// (src/foreign.rs `enter`'s FontAsk and AnnotationAsk arms, `unsure`, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_font_or_annotation_a_walk_cannot_read_has_the_childrens_script_safe_in_both() {
    let mut it = app(ASK);
    pages(
        &mut it,
        &[
            ("/font", format!("<svg><font size=\"3\"><script>{CODE}</script></font></svg>")),
            ("/font-after", format!("<svg><font size=\"3\">x</font><script>{CODE}</script></svg>")),
            ("/math-font", format!("<math><font size=\"3\"><script>{CODE}</script></font></math>")),
            ("/ann?e=text/html", format!("<math><annotation-xml encoding=\"text/html\"><script>{CODE}</script></annotation-xml></math>")),
            ("/ann?e=text/xml", format!("<math><annotation-xml encoding=\"text/xml\"><script>{CODE}</script></annotation-xml></math>")),
            ("/ann", format!("<math><annotation-xml encoding=\"\"><script>{CODE}</script></annotation-xml></math>")),
            // In the page itself: a `<font>` given a size ends SVG, an HTML `encoding` is HTML's.
            ("/font-here", format!("<svg><font size=\"3\"><script>{HTML}</script></font></svg>")),
            ("/ann-here?e=TEXT/HTML", format!("<math><annotation-xml encoding=\"TEXT/HTML\"><script>{HTML}</script></annotation-xml></math>")),
            ("/ann-here?e=text/xml", format!("<math><annotation-xml encoding=\"text/xml\"><script>{CODE}</script></annotation-xml></math>")),
        ],
    );
}

// ---- raw markup placed inside <svg> ------------------------------------------------------------

const RAW: &str = r##"route GET /lt ? { q: str = "" } = <html><body><svg>{raw("<g>1 < 2 </ 3 <!x></g>")}<script>{raw(q)}</script></svg></body></html>
route GET /long ? { q: str = "" } = <html><body><svg>{raw("<a-very-long-element><script>" + q + "</script></a-very-long-element>")}</svg></body></html>
route GET /sixteen ? { q: str = "" } = <html><body><svg>{raw("<abcdefghijklmnop><script>" + q + "</script></abcdefghijklmnop>")}</svg></body></html>
route GET /seventeen ? { q: str = "" } = <html><body><svg>{raw("<abcdefghijklmnopq><script>" + q + "</script></abcdefghijklmnopq>")}</svg></body></html>
route GET /point-like ? { q: str = "" } = <html><body><svg>{raw("<foreignObjectXYZW><script>" + q + "</script></foreignObjectXYZW>")}</svg></body></html>
route GET /point ? { q: str = "" } = <html><body><svg>{raw("<FOREIGNOBJECT><script>" + q + "</script></FOREIGNOBJECT>")}</svg></body></html>
route GET /unicode ? { q: str = "" } = <html><body><svg>{raw("<é> ✓ <script>" + q + "</script>")}</svg></body></html>
route GET /empty ? { q: str = "" } = <html><body><svg>{raw("")}<script>{raw(q)}</script></svg></body></html>
route GET /void ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><input></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /self-closed ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><svg/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /open ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<div>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /closed ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<div></div>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /unended ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<div")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /no-end-tag ? { q: str = "" } = <html><body><svg>{raw("<script>a</b>" + q)}</svg></body></html>
route GET /font-bare ? { q: str = "" } = <html><body><svg>{raw("<font>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /font-color ? { q: str = "" } = <html><body><svg>{raw("<font color=\"red\">x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /font-unended ? { q: str = "" } = <html><body><svg>{raw("<font x")}<script>{raw(q)}</script></svg></body></html>
route GET /font-quote ? { q: str = "" } = <html><body><svg>{raw("<font face=\"x")}<script>{raw(q)}</script></svg></body></html>
route GET /font-spread ? { q: str = "", a: str = "{}" } = <html><body><svg><font {...(json.parse(a) ?? {})}>x</font><script>{raw(q)}</script></svg></body></html>
"##;

/// Markup a program made and placed inside `<svg>` is read as the parser reads it: a `<` that
/// starts no tag (`< 2`, `<é>`), a tag name longer than any that matters (16 letters, 17, and
/// one that starts as `foreignObject`), none, a void element and a self-closed SVG
/// one (no end tag follows), an element left open or a tag left unended (what follows is not
/// known: the safe form), a script with no end tag, and a `<font>` whose written attributes say
/// whether it ends SVG (src/foreign.rs `written`, `tag_end`, `find_end_tag`, `kind_written`,
/// `written_attr`, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn raw_markup_in_svg_is_read_as_the_parser_reads_it() {
    let mut it = app(RAW);
    let lt = "<g>1 < 2 </ 3 <!x></g>";
    pages(
        &mut it,
        &[
            // No tag in `< 2`, `</ 3` or `<!x>`: still in SVG after it.
            ("/lt", format!("<svg>{lt}<script>{CODE}</script></svg>")),
            // An element of any other name is SVG's there, its script markup.
            ("/long", format!("<svg><a-very-long-element><script>{RAW_CODE}</script></a-very-long-element></svg>")),
            ("/sixteen", format!("<svg><abcdefghijklmnop><script>{RAW_CODE}</script></abcdefghijklmnop></svg>")),
            ("/seventeen", format!("<svg><abcdefghijklmnopq><script>{RAW_CODE}</script></abcdefghijklmnopq></svg>")),
            // A name that starts as an integration point's is another element; the name in any
            // case is the integration point, where HTML is read.
            ("/point-like", format!("<svg><foreignObjectXYZW><script>{RAW_CODE}</script></foreignObjectXYZW></svg>")),
            ("/point", format!("<svg><FOREIGNOBJECT><script>{Q}</script></FOREIGNOBJECT></svg>")),
            // `<é>` starts no tag (a tag's name starts with an ASCII letter); nothing written.
            ("/unicode", format!("<svg><é> ✓ <script>{RAW_CODE}</script></svg>")),
            ("/empty", format!("<svg><script>{CODE}</script></svg>")),
            // `<input>` and `<svg/>` take no end tag: `</foreignObject>` closes the integration
            // point, and the script after it is SVG's.
            ("/void", format!("<svg><foreignObject><input></foreignObject><script>{RAW_CODE}</script></svg>")),
            ("/self-closed", format!("<svg><foreignObject><svg/></foreignObject><script>{RAW_CODE}</script></svg>")),
            // A `<div>` left open in a `<foreignObject>`: not known; closed, HTML. Its tag unended
            // is ended (`>`), else the browser reads the renderer's next markup into it (changed
            // with Q-1116: it was left unended).
            ("/open", format!("<svg><foreignObject><div><script>{CODE}</script></foreignObject></svg>")),
            ("/closed", format!("<svg><foreignObject><div></div><script>{HTML}</script></foreignObject></svg>")),
            ("/unended", format!("<svg><foreignObject><div><script>{CODE}</script></foreignObject></svg>")),
            // A script that is never ended: its text to the end, in a section, and its end tag
            // written (changed with Q-1116: it was left open).
            ("/no-end-tag", "<svg><script><![CDATA[a</b><b>x</b>&quot;]]]]><![CDATA[>]]></script></svg>".to_string()),
            // `<font>` alone is SVG's; given `color`, it ends SVG; unended, what follows is not known.
            ("/font-bare", format!("<svg><font>x</font><script>{CODE}</script></svg>")),
            ("/font-color", format!("<svg><font color=\"red\">x</font><script>{HTML}</script></svg>")),
            // Unended, the tag is ended (its quote too; changed with Q-1116), and what is in it is
            // not known.
            ("/font-unended", format!("<svg><font x><script>{CODE}</script></svg>")),
            ("/font-quote", format!("<svg><font face=\"x\"><script>{CODE}</script></svg>")),
            // A spread on a `<font>`: what it wrote says.
            ("/font-spread", format!("<svg><font>x</font><script>{CODE}</script></svg>")),
            ("/font-spread?a=%7B%22color%22%3A%22red%22%7D", format!("<svg><font color=\"red\">x</font><script>{HTML}</script></svg>")),
            ("/font-spread?a=%7B%22SIZE%22%3A1%7D", format!("<svg><font SIZE=\"1\">x</font><script>{HTML}</script></svg>")),
            ("/font-spread?a=%7B%22sizes%22%3A1%7D", format!("<svg><font sizes=\"1\">x</font><script>{CODE}</script></svg>")),
        ],
    );
}

// ---- where a view puts its children ------------------------------------------------------------

const DEEP: &str = r##"view Deep(children) = <svg><foreignObject><svg>{children}{children}</svg></foreignObject></svg>
view DeepOnce(children) = <svg><foreignObject><svg>{children}</svg></foreignObject></svg>
route GET /deep ? { q: str = "" } = <html><body><Deep><p><script>{raw(q)}</script></p></Deep></body></html>
route GET /deep-once ? { q: str = "" } = <html><body><DeepOnce><p><script>{raw(q)}</script></p></DeepOnce></body></html>
"##;

/// Children written in an `<svg>` that started inside a `<foreignObject>`: a `<p>` among them may
/// leave the parser in SVG where the walk would go on as in HTML, so what follows them there is
/// not known, and children written twice are rendered as safe in both; written once, the `<p>`
/// ends SVG and their script is HTML's (src/foreign.rs `after_unseen`, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn children_after_a_run_started_in_an_integration_point_are_not_known() {
    let mut it = app(DEEP);
    let p = format!("<p><script>{CODE}</script></p>");
    pages(
        &mut it,
        &[
            ("/deep", format!("<svg><foreignObject><svg>{p}{p}</svg></foreignObject></svg>")),
            ("/deep-once", format!("<svg><foreignObject><svg><p><script>{HTML}</script></p></svg></foreignObject></svg>")),
        ],
    );
}

/// `n` functions, each calling the next, the last giving `""`: `{prefix}1()` starts the chain.
fn chain(prefix: &str, n: usize) -> String {
    let mut s = String::new();
    for i in 1..n {
        s += &format!("fn {prefix}{i}() = {prefix}{}()\n", i + 1);
    }
    s + &format!("fn {prefix}{n}() = \"\"\n")
}

fn reach_src() -> String {
    chain("c", 9)
        + &chain("d", 8)
        + r##"fn ident(x) = x
fn plain() = "x"
fn drawn() = <svg></svg>
fn rec(n: int) = n > 0 ? rec(n - 1) : ""
view Rec(children) {
    let f = (x) => x
    <div>{f(children)}{rec(3)}</div>
}
view Builtin(children) {
    let f = (x) => x
    <div>{f(children)}{upper("a")}</div>
}
view Named(children) {
    let f = (x) => x
    <div>{f(children)}{plain()}</div>
}
view NamedSvg(children) {
    let f = (x) => x
    <div>{f(children)}{drawn()}</div>
}
view Unknown(children) {
    let g = ident
    <div>{g(children)}</div>
}
view Nine(children) {
    let f = (x) => x
    <div>{f(children)}{c1()}</div>
}
view Eight(children) {
    let f = (x) => x
    <div>{f(children)}{d1()}</div>
}
route GET /rec ? { q: str = "" } = <html><body><Rec><script>{raw(q)}</script></Rec></body></html>
route GET /builtin ? { q: str = "" } = <html><body><Builtin><script>{raw(q)}</script></Builtin></body></html>
route GET /named ? { q: str = "" } = <html><body><Named><script>{raw(q)}</script></Named></body></html>
route GET /named-svg ? { q: str = "" } = <html><body><NamedSvg><script>{raw(q)}</script></NamedSvg></body></html>
route GET /unknown ? { q: str = "" } = <html><body><Unknown><script>{raw(q)}</script></Unknown></body></html>
route GET /nine ? { q: str = "" } = <html><body><Nine><script>{raw(q)}</script></Nine></body></html>
route GET /eight ? { q: str = "" } = <html><body><Eight><script>{raw(q)}</script></Eight></body></html>
"##
}

/// A view the walk cannot follow (its children through a lambda), called in HTML, has them as
/// HTML when nothing it reaches can start an `<svg>` or `<math>`: a function that calls itself is
/// looked at once, a builtin starts none, a named function is looked into (one drawing an `<svg>`
/// keeps the safe form), a name bound to a function in the body is not known, and a chain of
/// functions is followed eight deep, not nine (src/foreign.rs `may_foreign`, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_view_the_walk_cannot_follow_keeps_html_only_where_no_svg_is_reached() {
    let mut it = app(&reach_src());
    pages(
        &mut it,
        &[
            ("/rec", format!("<div><script>{HTML}</script></div>")),
            ("/builtin", format!("<div><script>{HTML}</script>A</div>")),
            ("/named", format!("<div><script>{HTML}</script>x</div>")),
            ("/named-svg", format!("<div><script>{CODE}</script><svg></svg></div>")),
            ("/unknown", format!("<div><script>{CODE}</script></div>")),
            ("/eight", format!("<div><script>{HTML}</script></div>")),
            ("/nine", format!("<div><script>{CODE}</script></div>")),
        ],
    );
}

/// Functions `{name}1` to `{name}7`, each but the last calling the next `fan` times (with two
/// empty `<g>`s after), the last putting `c` in a `<g>`: `fan` 4 makes more steps than a walk
/// takes, `fan` 2 far fewer.
fn fan(name: &str, fan: usize) -> String {
    let mut s = String::new();
    for i in 1..7 {
        let calls: String = (0..fan).map(|_| format!("{{{name}{}(c)}}", i + 1)).collect();
        s += &format!("fn {name}{i}(c) = <g>{calls}<g></g><g></g></g>\n");
    }
    s + &format!("fn {name}7(c) = <g>{{c}}</g>\n")
}

/// A view whose children the walk follows through more calls than its budget of steps allows
/// (four calls a level, seven levels) is not sure where they go, and renders them as safe in
/// both: their `<p>` does not end SVG for it. Through two calls a level the walk is sure: the
/// `<p>` ends SVG, and the script is HTML's (src/foreign.rs `Walk::tick`, `placed` and `el` past
/// the budget, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_walk_past_its_budget_of_steps_is_not_sure() {
    let src = fan("w", 4)
        + &fan("v", 2)
        + r##"view Wide(children, deep: bool) = <svg>{deep ? w1(children) : children}</svg>
view Narrow(children, deep: bool) = <svg>{deep ? v1(children) : children}</svg>
route GET /wide ? { q: str = "" } = <html><body><Wide deep={false}><p><script>{raw(q)}</script></p></Wide></body></html>
route GET /narrow ? { q: str = "" } = <html><body><Narrow deep={false}><p><script>{raw(q)}</script></p></Narrow></body></html>
"##;
    let mut it = app(&src);
    pages(
        &mut it,
        &[("/wide", format!("<svg><p><script>{CODE}</script></p></svg>")), ("/narrow", format!("<svg><p><script>{HTML}</script></p></svg>"))],
    );
}

const PASSED: &str = r##"view Box(children) = <svg>{children}</svg>
view Chart(label) = <svg>{label}</svg>
view Card(label) = <div>{label}<svg></svg></div>
view InG(children) = <Box><g title={"t"} .c-{"k"} #i-{"d"}><text>{children}</text></g></Box>
view InB(children) = <Box><b title={"t"}>{children}</b></Box>
view Pass(children) = <div><Chart label={children}/></div>
view PassCard(children) = <div><Card label={children}/></div>
view PassSpread(children) = <div><Chart {...{ label: children }}/></div>
view PassText(children) = <div>{children}<Chart label={"x"}/></div>
route GET /in-g ? { q: str = "" } = <html><body><InG><script>{raw(q)}</script></InG></body></html>
route GET /in-b ? { q: str = "" } = <html><body><InB><script>{raw(q)}</script></InB></body></html>
route GET /pass ? { q: str = "" } = <html><body><Pass><script>{raw(q)}</script></Pass></body></html>
route GET /pass-card ? { q: str = "" } = <html><body><PassCard><script>{raw(q)}</script></PassCard></body></html>
route GET /pass-spread ? { q: str = "" } = <html><body><PassSpread><script>{raw(q)}</script></PassSpread></body></html>
route GET /pass-text ? { q: str = "" } = <html><body><PassText><script>{raw(q)}</script></PassText></body></html>
"##;

/// Children handed on to another view, in an element given to it (with an id, a class and an
/// attribute of its own) or in an attribute: they go where that view puts them, SVG in a `<g>`,
/// HTML in a `<b>` (which ends SVG) or a `<div>`; through a spread the walk cannot tell, the safe
/// form (src/foreign.rs `el_mentions`, `view_el`'s attributes and their walk, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn children_handed_to_another_view_go_where_it_puts_them() {
    let mut it = app(PASSED);
    pages(
        &mut it,
        &[
            ("/in-g", format!("<svg><g id=\"i-d\" class=\"c-k\" title=\"t\"><text><script>{CODE}</script></text></g></svg>")),
            ("/in-b", format!("<svg><b title=\"t\"><script>{HTML}</script></b></svg>")),
            ("/pass", format!("<div><svg><script>{CODE}</script></svg></div>")),
            ("/pass-card", format!("<div><div><script>{HTML}</script><svg></svg></div></div>")),
            ("/pass-spread", format!("<div><svg><script>{CODE}</script></svg></div>")),
            ("/pass-text", format!("<div><script>{HTML}</script><svg>x</svg></div>")),
        ],
    );
}

// Each view below puts its children in HTML and has an `<svg>` of its own, so a walk that is not
// sure keeps the safe form (the section); one that is sure writes HTML's.
const VALUES: &str = r##"fn ident(x) = x
fn maker() = (a) => <span>{a}</span>
let one = maker()
view Compared(children) = <div>{children}<svg></svg>{len(children) > 3}</div>
view SetLen(children) {
    let n = 0
    n = len(children)
    <div>{children}<svg></svg></div>
}
view SetCompared(children) {
    let b = false
    b = len(children) > 3
    <div>{children}<svg></svg></div>
}
view SetAdded(children) {
    let s = ""
    s = "a" + "b"
    <div>{children}<svg></svg></div>
}
view SetChildren(children) {
    let x = ""
    x = children
    <div>{children}<svg></svg></div>
}
view Kept(children) {
    let e = is_err(children)
    <div>{children}<svg></svg>{if e { "err" } else { "ok" }}</div>
}
view KeptPlaced(children) {
    let t = str(children ?? "")
    <div>{if false { t } else { "" }}{children}<svg></svg></div>
}
view TypeOf(children) = <div>{type_of(children)}{children}<svg></svg></div>
view Indexed(children) = <div>{[children][0]}<svg></svg></div>
view Field(children) {
    let m = { a: 1 }
    <div>{m.a}{children}<svg></svg></div>
}
view MapCall(children) {
    let m = { f: ident }
    <div>{m.f(children)}<svg></svg></div>
}
view Method(children) {
    let m = { a: children }
    <div>{children}{if false { m.a(1) } else { "" }}<svg></svg></div>
}
view Lambda(children) {
    let f = () => children
    <div>{f()}<svg></svg></div>
}
view Matched(children) = match children {
    x => <div>{x}<svg></svg></div>
}
view Guarded(children, k: str) = match k {
    s if s == "a" => <div>{children}<svg></svg></div>
    _ => <div>{children}<svg></svg></div>
}
view ForChildren(children) = <div>{for c in [children] => c}<svg></svg></div>
view ForRows(children) = <div>{for i in [1, 2] => <span>{i}</span>}{children}<svg></svg></div>
view ForBreaks(children) = <svg>{for i in [1] => <p>x</p>}<g>{children}</g></svg>
view Arity(children) = <div>{children}{if false { one(children, children) } else { "" }}<svg></svg></div>
view Painted(children, ctx = null) {
    if ctx != null {
        ctx.fill = children
    }
    <div>{children}<svg></svg></div>
}
view Inner(children) {
    fn inner() = children
    <div>{children}<svg></svg>{inner() == null}</div>
}
view InnerPlain(children) {
    fn inner() = "x"
    <div>{children}<svg></svg>{inner()}</div>
}
view Loop(children) {
    let i = 0
    while i < 2 {
        i += 1
    }
    <div>{children}<svg></svg>{i}</div>
}
view Bare(children, k: bool) {
    if k {
        return
    }
    <div>{children}<svg></svg></div>
}
view Spread(children) {
    let [a, b] = [children, 1]
    <div>{a}<svg></svg>{b}</div>
}
view SpreadPlain(children) {
    let [a, b] = [1, 2]
    <div>{children}<svg></svg>{a + b}</div>
}
view ScriptKept(children) {
    let s = <script>{"var a = 1;"}</script>
    <div>{children}{s}<svg></svg></div>
}
view TwoReturns(children, k: bool) {
    if k {
        return <svg>{children}</svg>
    }
    return <svg><g>{children}</g></svg>
}
view TwoPlaces(children, k: bool) {
    if k {
        return <svg>{children}</svg>
    }
    return <div>{children}</div>
}
"##;

/// `VALUES` with a route for each view (`/{name}` renders `<Name>` with a script as its children,
/// `/{name}-p` with the script in a `<p>`) and the holding chains of `holds`.
fn values_src() -> String {
    let mut s = VALUES.to_string();
    for n in [8, 9] {
        s += &format!("view Holds{n}(children) {{\n    let a1 = <span>{{children}}</span>\n");
        for i in 2..=n {
            s += &format!("    let a{i} = <span>{{a{}}}</span>\n", i - 1);
        }
        s += &format!("    <div>{{a{n}}}<svg></svg></div>\n}}\n");
    }
    for v in VALUES.lines().filter_map(|l| l.strip_prefix("view ")).map(|l| l.split('(').next().unwrap()).chain(["Holds8", "Holds9"]) {
        let args = match v {
            "Guarded" => " k=\"a\"",
            "Bare" | "TwoReturns" | "TwoPlaces" => " k={false}",
            _ => "",
        };
        let path = v.to_ascii_lowercase();
        s += &format!("route GET /{path} ? {{ q: str = \"\" }} = <html><body><{v}{args}><script>{{raw(q)}}</script></{v}></body></html>\n");
        s += &format!("route GET /{path}-p ? {{ q: str = \"\" }} = <html><body><{v}{args}><p><script>{{raw(q)}}</script></p></{v}></body></html>\n");
    }
    s
}

/// What a view does with its children before it writes them: a value made of them that holds no
/// markup (a length, a comparison, `type_of`, a builtin's answer kept and not written, a field of
/// another value, a sum, a loop over other rows, a guard, a destructured list of others, two
/// `return`s that agree, a bare `return`) leaves the walk sure, and their script is HTML's; one
/// that may hold them and is not followed (an index, a method, a lambda, a match or loop over
/// them, a call given more of them than it takes, an assignment, a field set, a nested `fn`, a
/// destructuring `let`, a holding chain nine deep, a loop whose rows end SVG, `return`s that
/// differ) makes it unsure: the section (src/foreign.rs `Walk::carries`, `text_builtin`, `callee`,
/// `bind_pattern`, `placed`, `call`, `el`, `block`, `bind_let`, `ident_of`, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn what_a_view_makes_of_its_children_decides_whether_the_walk_is_sure() {
    let mut it = app(&values_src());
    let sure = format!("<script>{HTML}</script>");
    let unsure = format!("<script>{CODE}</script>");
    let div = |s: &str, more: &str| format!("<div>{s}<svg></svg>{more}</div>");
    let spans = |n: usize, s: &str| format!("<div>{}{s}{}<svg></svg></div>", "<span>".repeat(n), "</span>".repeat(n));
    pages(
        &mut it,
        &[
            // Sure: HTML's form.
            ("/compared", div(&sure, "")),
            ("/setlen", div(&sure, "")),
            ("/setcompared", div(&sure, "")),
            ("/setadded", div(&sure, "")),
            ("/kept", div(&sure, "ok")),
            ("/typeof", format!("<div>html{sure}<svg></svg></div>")),
            ("/field", format!("<div>1{sure}<svg></svg></div>")),
            ("/guarded", div(&sure, "")),
            ("/forrows", format!("<div><span>1</span><span>2</span>{sure}<svg></svg></div>")),
            ("/innerplain", div(&sure, "x")),
            ("/loop", div(&sure, "2")),
            ("/bare", div(&sure, "")),
            ("/spreadplain", div(&sure, "3")),
            ("/scriptkept", format!("<div>{sure}<script>\"var a = 1;\"</script><svg></svg></div>")),
            ("/holds8", spans(8, &sure)),
            // Two returns that agree: SVG, where a `<p>` ends it, and the script is HTML's.
            ("/tworeturns-p", format!("<svg><g><p>{sure}</p></g></svg>")),
            // Not sure: the section.
            ("/setchildren", div(&unsure, "")),
            ("/keptplaced", div(&unsure, "")),
            ("/indexed", div(&unsure, "")),
            ("/mapcall", div(&unsure, "")),
            ("/method", div(&unsure, "")),
            ("/lambda", div(&unsure, "")),
            ("/matched", div(&unsure, "")),
            ("/forchildren", div(&unsure, "")),
            ("/arity", div(&unsure, "")),
            ("/painted", div(&unsure, "")),
            ("/inner", div(&unsure, "")),
            ("/spread", div(&unsure, "1")),
            ("/holds9", spans(9, &unsure)),
            ("/forbreaks", format!("<svg><p>x</p><g>{unsure}</g></svg>")),
            ("/twoplaces", format!("<div>{unsure}</div>")),
        ],
    );
}

// ---- a fixed element where the parser's place is not known -------------------------------------

const FIXED: &str = r##"view Pick(children, k: bool) = k ? <svg>{children}</svg> : <div>{children}</div>
route GET /pick ? { q: str = "", k: str = "" } = <html><body><Pick k={k == "1"}><p>fixed text</p><script>{raw(q)}</script></Pick></body></html>
route GET /left-open ? { q: str = "" } = <html><body><svg>{raw("<g>")}<p>fixed text</p><script>{raw(q)}</script></svg></body></html>
route GET /sum ? { q: str = "" } = <html><body><svg><g>{raw(q) + raw("<i>")}</g><text>{"a" + q}</text><script>{raw(q)}</script></svg></body></html>
"##;

/// An element whose HTML never changes, which ends foreign content (`<p>`), written where the
/// parser's place is not known (children of a view whose arms differ, after markup left open):
/// its HTML is made once and kept, the same on each render, and the script after it keeps the
/// safe form; a sum of values inside `<svg>` is written as it is, its markup read again
/// (src/interp.rs `render_el_ns`'s kept HTML, `render_value_ns`, FEEDBACK71.md Q-969; coverage of 0.8.7; branch coverage-087).
#[test]
fn a_fixed_element_where_the_place_is_not_known_is_made_once() {
    let mut it = app(FIXED);
    let p = "<p>fixed text</p>";
    pages(
        &mut it,
        &[
            ("/pick?k=1", format!("<svg>{p}<script>{CODE}</script></svg>")),
            ("/pick?k=0", format!("<div>{p}<script>{CODE}</script></div>")),
            ("/left-open", format!("<svg><g>{p}<script>{CODE}</script></svg>")),
            ("/sum", format!("<svg><g>{Q}<i></g><text>a&lt;b&gt;x&lt;/b&gt;&amp;quot;]]&gt;</text><script>{CODE}</script></svg>")),
        ],
    );
}

// ---- Q-1100, Q-1111, Q-1112: start tags, read as the browser reads them ------------------------

const SELF_CLOSED: &str = r##"route GET /div ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><div/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /div-upper ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><DIV/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /unknown ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><x-y/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /unicode ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><x-é/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /br ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><br/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /br-upper ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><BR/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /img ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><img src=\"/x.png\"/></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /then-closed ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><div/></foreignObject></div></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /left-in-point ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<div/>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /circle ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><circle r=\"1\"/></svg><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /circle-spaced ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><circle r=\"1\" /></svg><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /svg-closed ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg width=\"10\"/><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /svg-bare ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg hidden/><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /svg-unquoted ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg width=10/><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /svg-unicode ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg aria-label=✓/><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /svg-spaced-slash ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg / ><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /math-closed ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<math/><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /svg-script ? { q: str = "" } = <html><body><svg>{raw("<script/><foreignObject><script>" + q + "</script></foreignObject>")}</svg></body></html>
route GET /html-script ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<script/>" + q + "</script>")}</foreignObject></svg></body></html>
"##;

/// Q-1100's repro: a `<div/>` in markup placed in a `<foreignObject>` is open, as the browser
/// keeps it (an HTML element's self-closing flag is passed over), so the `</foreignObject>` after
/// it ends nothing and the script after that is HTML's, written without a CDATA section; `<DIV/>`,
/// an unknown `<x-y/>` and `<x-é/>` alike. Void elements (`<br/>`, `<BR/>`, `<img/>`) and SVG's
/// and MathML's elements (`<circle/>`, `<svg width="10"/>`, `<svg hidden/>`, `<circle r="1" />`,
/// `<math/>`, an SVG `<script/>`) are closed by it; a `/` that is an unquoted value's
/// (`<svg width=10/>`, `<svg aria-label=✓/>`) or that a space parts from `>` (`<svg / >`) is no
/// flag, and the `<svg>` stays open: its script is SVG's. A `</div>` then a real
/// `</foreignObject>` leave the integration point (src/foreign.rs `written`, `self_closing`,
/// `next_attr`; FEEDBACK71.md Q-969, coverage of 0.8.7, Q-1100, Q-1112; branch coverage-087).
#[test]
fn a_self_closed_html_element_stays_open_as_the_browser_keeps_it() {
    let mut it = app(SELF_CLOSED);
    pages(
        &mut it,
        &[
            // HTML elements: open, and `</foreignObject>` ends nothing: the script is HTML's.
            ("/div", format!("<svg><foreignObject><div/></foreignObject><script>{Q}</script></svg>")),
            ("/div-upper", format!("<svg><foreignObject><DIV/></foreignObject><script>{Q}</script></svg>")),
            ("/unknown", format!("<svg><foreignObject><x-y/></foreignObject><script>{Q}</script></svg>")),
            ("/unicode", format!("<svg><foreignObject><x-é/></foreignObject><script>{Q}</script></svg>")),
            // Void elements: no end tag, so `</foreignObject>` ends the integration point.
            ("/br", format!("<svg><foreignObject><br/></foreignObject><script>{RAW_CODE}</script></svg>")),
            ("/br-upper", format!("<svg><foreignObject><BR/></foreignObject><script>{RAW_CODE}</script></svg>")),
            ("/img", format!("<svg><foreignObject><img src=\"/x.png\"/></foreignObject><script>{RAW_CODE}</script></svg>")),
            // `</div>` closes the `<div>`, then `</foreignObject>` the integration point.
            ("/then-closed", format!("<svg><foreignObject><div/></foreignObject></div></foreignObject><script>{RAW_CODE}</script></svg>")),
            // Left open at the end of the markup, as `raw("<div>")` is: what follows is not known.
            ("/left-in-point", format!("<svg><foreignObject><div/><script>{CODE}</script></foreignObject></svg>")),
            // SVG's and MathML's elements: closed by the flag.
            ("/circle", format!("<svg><foreignObject><svg><circle r=\"1\"/></svg><script>{Q}</script></foreignObject></svg>")),
            ("/circle-spaced", format!("<svg><foreignObject><svg><circle r=\"1\" /></svg><script>{Q}</script></foreignObject></svg>")),
            ("/svg-closed", format!("<svg><foreignObject><svg width=\"10\"/><script>{Q}</script></foreignObject></svg>")),
            ("/svg-bare", format!("<svg><foreignObject><svg hidden/><script>{Q}</script></foreignObject></svg>")),
            ("/math-closed", format!("<svg><foreignObject><math/><script>{Q}</script></foreignObject></svg>")),
            // No flag: the `<svg>` is open, and the script in it is SVG's.
            ("/svg-unquoted", format!("<svg><foreignObject><svg width=10/><script>{RAW_CODE}</script></foreignObject></svg>")),
            ("/svg-unicode", format!("<svg><foreignObject><svg aria-label=✓/><script>{RAW_CODE}</script></foreignObject></svg>")),
            ("/svg-spaced-slash", format!("<svg><foreignObject><svg / ><script>{RAW_CODE}</script></foreignObject></svg>")),
            // An SVG `<script/>` holds no text: the `<foreignObject>` after it is read, and its
            // script is HTML's. An HTML `<script/>`'s text runs to `</script>`, as before.
            ("/svg-script", format!("<svg><script/><foreignObject><script>{Q}</script></foreignObject></svg>")),
            ("/html-script", format!("<svg><foreignObject><script/>{Q}</script></foreignObject></svg>")),
        ],
    );
}

const ATTRS: &str = r##"route GET /size ? { q: str = "" } = <html><body><svg>{raw("<font size=1>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /second ? { q: str = "" } = <html><body><svg>{raw("<font a=x color=red>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /spaced ? { q: str = "" } = <html><body><svg>{raw("<font size = \"1\">x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /single ? { q: str = "" } = <html><body><svg>{raw("<font a='x y' face='f'>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /bare ? { q: str = "" } = <html><body><svg>{raw("<font\tcolor>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /no-value ? { q: str = "" } = <html><body><svg>{raw("<font size=>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /swallowed ? { q: str = "" } = <html><body><svg>{raw("<font a= size>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /sizes ? { q: str = "" } = <html><body><svg>{raw("<font sizes=1 =size>x</font>")}<script>{raw(q)}</script></svg></body></html>
route GET /ann ? { q: str = "" } = <html><body><math>{raw("<annotation-xml definitionURL=x encoding=text/html><script>" + q + "</script></annotation-xml>")}</math></body></html>
route GET /ann-single ? { q: str = "" } = <html><body><math>{raw("<annotation-xml encoding='TEXT/HTML'><script>" + q + "</script></annotation-xml>")}</math></body></html>
route GET /ann-slash ? { q: str = "" } = <html><body><math>{raw("<annotation-xml encoding=text/html/><script>" + q + "</script></annotation-xml>")}</math></body></html>
route GET /ann-first ? { q: str = "" } = <html><body><math>{raw("<annotation-xml encoding=x encoding=text/html><script>" + q + "</script></annotation-xml>")}</math></body></html>
route GET /quote-in-value ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<g a=x'y><svg>'><script>" + q + "</script></svg></g>")}</foreignObject></svg></body></html>
route GET /quote-in-name ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<g a\"b><svg>\"><script>" + q + "</script></svg></g>")}</foreignObject></svg></body></html>
route GET /quoted-gt ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<g title=\"a>b\"><svg><script>" + q + "</script></svg></g>")}</foreignObject></svg></body></html>
route GET /end-tag-quote ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg></svg a='>'><script>" + q + "</script>")}</foreignObject></svg></body></html>
"##;

/// A start tag in markup the program made, read as the browser reads its attributes: unquoted
/// values, spaces around `=` (`a= size` is `a` given `size`), single quotes, a tab, a name with
/// no value or with `=` and none, a name that starts with `=`; the first of two with one name
/// counts; a quote is one only where a value starts (`<g a=x'y>` ends at its `>`).
/// `raw("<font size=1>")` inside `<svg>` was a request that never ended (`written_attr` read the
/// same `=` again and again); now `size` is seen and the `<font>` ends SVG. Each request has a
/// deadline (src/foreign.rs `written_attr`, `next_attr`, `kind_written`, `tag_end`;
/// FEEDBACK71.md Q-969, coverage of 0.8.7, Q-1111, Q-1112; branch coverage-087).
#[test]
fn a_start_tag_made_by_the_program_is_read_as_the_browser_reads_it_and_never_hangs() {
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let mut it = app(ATTRS);
        let font = |raw: &str, script: &str| format!("<svg>{raw}x</font><script>{script}</script></svg>");
        pages(
            &mut it,
            &[
                // `color`, `face` or `size` given: the `<font>` ends SVG, and the script is HTML's.
                ("/size", font("<font size=1>", HTML)),
                ("/second", font("<font a=x color=red>", HTML)),
                ("/spaced", font("<font size = \"1\">", HTML)),
                ("/single", font("<font a='x y' face='f'>", HTML)),
                ("/bare", font("<font\tcolor>", HTML)),
                ("/no-value", font("<font size=>", HTML)),
                // `sizes` and `=size` are other names, and after `a= ` the `size` is `a`'s value:
                // SVG's `<font>`.
                ("/sizes", font("<font sizes=1 =size>", CODE)),
                ("/swallowed", font("<font a= size>", CODE)),
                // An HTML `encoding`, however quoted, after another attribute: HTML's script.
                ("/ann", format!("<math><annotation-xml definitionURL=x encoding=text/html><script>{Q}</script></annotation-xml></math>")),
                ("/ann-single", format!("<math><annotation-xml encoding='TEXT/HTML'><script>{Q}</script></annotation-xml></math>")),
                // `text/html/` is the value (no flag), and the first `encoding` counts: MathML's.
                ("/ann-slash", format!("<math><annotation-xml encoding=text/html/><script>{RAW_CODE}</script></annotation-xml></math>")),
                ("/ann-first", format!("<math><annotation-xml encoding=x encoding=text/html><script>{RAW_CODE}</script></annotation-xml></math>")),
                // A quote in an unquoted value or in a name is no quote: the tag ends at its `>`,
                // and the `<svg>` after it holds the script, SVG's. A quoted value's `>`, in a
                // start tag or an end tag, ends nothing.
                ("/quote-in-value", format!("<svg><foreignObject><g a=x'y><svg>'><script>{RAW_CODE}</script></svg></g></foreignObject></svg>")),
                ("/quote-in-name", format!("<svg><foreignObject><g a\"b><svg>\"><script>{RAW_CODE}</script></svg></g></foreignObject></svg>")),
                ("/quoted-gt", format!("<svg><foreignObject><g title=\"a>b\"><svg><script>{RAW_CODE}</script></svg></g></foreignObject></svg>")),
                ("/end-tag-quote", format!("<svg><foreignObject><svg></svg a='>'><script>{Q}</script></foreignObject></svg>")),
            ],
        );
        let _ = tx.send(());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(()) => t.join().unwrap(),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => std::panic::resume_unwind(t.join().unwrap_err()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("a page was not answered within 120 s: a start tag was read again and again"),
    }
}

// ---- Q-1113: comments, bogus comments and CDATA sections, passed over as the tokenizer does ----

const PASSED_OVER: &str = r##"route GET /comment-empty ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!--><svg>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /comment-dash ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!---><svg>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /comment-bang ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!-- x --!><svg>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /comment-dashes ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!-- a -- <svg> b -->")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /comment-closed ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!-- <svg> --->")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /comment-open ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!-- x")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /bogus ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><!x </svg>><script>" + q + "</script></svg>")}</foreignObject></svg></body></html>
route GET /doctype ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><!DOCTYPE x </svg>><script>" + q + "</script></svg>")}</foreignObject></svg></body></html>
route GET /question ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><?x </svg>><script>" + q + "</script></svg>")}</foreignObject></svg></body></html>
route GET /end-bogus ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg></ x </svg>><script>" + q + "</script></svg>")}</foreignObject></svg></body></html>
route GET /end-empty ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg></></svg><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /bogus-open ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<!x")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /cdata-svg ? { q: str = "" } = <html><body><svg>{raw("<![CDATA[><foreignObject>]]><script>" + q + "</script>")}</svg></body></html>
route GET /cdata-point ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<![CDATA[><svg>]]><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /cdata-div ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<div><![CDATA[><svg>]]><script>" + q + "</script></svg></div>")}</foreignObject></svg></body></html>
route GET /cdata-open ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<![CDATA[ x")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /cdata-unsure ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><p></p><![CDATA[><svg><script>" + q + "</script>]]>")}</foreignObject></svg></body></html>
"##;

/// Markup in a `<foreignObject>` that holds no tag to the browser is passed over as its tokenizer
/// passes it over. A comment ends at once for `<!-->` and `<!--->`, and at the first `-->` or
/// `--!>` (not at `-- b`): the `<svg>` after it is open, so the script after the markup is
/// written as safe in both (Q-1113's repro, where the comment ran to the end and the script was
/// HTML's). One left open is ended. `<!x ...>`, `<!DOCTYPE ...>`, `<?x ...>` and
/// `</ x ...>` end at the first `>`, so an `</svg>` in them closes nothing and the script after
/// is SVG's; `</>` is dropped. `<![CDATA[` is a section to `]]>` in SVG, a bogus comment to the
/// first `>` in a `<div>`, and either right in a `<foreignObject>` or where the place is not
/// known: the shorter way, what follows not known where the two differ by markup
/// (src/foreign.rs `passed_over`, `written`; FEEDBACK71.md Q-969, coverage of 0.8.7, Q-1113,
/// Q-1116; branch coverage-087).
#[test]
fn comments_and_cdata_sections_are_passed_over_as_the_browser_passes_them() {
    let mut it = app(PASSED_OVER);
    let fo = |inner: &str| format!("<svg><foreignObject>{inner}</foreignObject></svg>");
    pages(
        &mut it,
        &[
            // A comment that ends at once, or at `--!>`: the `<svg>` after it is open.
            ("/comment-empty", fo(&format!("<!--><svg><script>{CODE}</script>"))),
            ("/comment-dash", fo(&format!("<!---><svg><script>{CODE}</script>"))),
            ("/comment-bang", fo(&format!("<!-- x --!><svg><script>{CODE}</script>"))),
            // `--` alone, or before `->`, ends nothing: the `<svg>` is in the comment.
            ("/comment-dashes", fo(&format!("<!-- a -- <svg> b --><script>{HTML}</script>"))),
            ("/comment-closed", fo(&format!("<!-- <svg> ---><script>{HTML}</script>"))),
            // Left open, it is ended (changed with Q-1116: what followed was not known).
            ("/comment-open", fo(&format!("<!-- x--><script>{HTML}</script>"))),
            // Bogus comments end at the first `>`: the `</svg>` in them closes nothing.
            ("/bogus", fo(&format!("<svg><!x </svg>><script>{RAW_CODE}</script></svg>"))),
            ("/doctype", fo(&format!("<svg><!DOCTYPE x </svg>><script>{RAW_CODE}</script></svg>"))),
            ("/question", fo(&format!("<svg><?x </svg>><script>{RAW_CODE}</script></svg>"))),
            ("/end-bogus", fo(&format!("<svg></ x </svg>><script>{RAW_CODE}</script></svg>"))),
            ("/end-empty", fo(&format!("<svg></></svg><script>{Q}</script>"))),
            ("/bogus-open", fo(&format!("<!x><script>{HTML}</script>"))),
            // A CDATA section in SVG; a bogus comment in a `<div>`. Right in a `<foreignObject>`,
            // browsers read the bogus comment (an `<svg>` after its `>`) where WHATWG HTML reads a
            // section: with markup between the two ends, what follows is not known, and the
            // script is written as safe in both (changed with Q-1113's review: the section was
            // read, the script written as HTML's, and Chrome and parse5 read it as SVG's). Left
            // open, it is ended with `]]>`, which ends either.
            ("/cdata-svg", format!("<svg><![CDATA[><foreignObject>]]><script>{RAW_CODE}</script></svg>")),
            ("/cdata-point", fo(&format!("<![CDATA[><svg>]]><script>{RAW_CODE}</script>"))),
            ("/cdata-div", fo(&format!("<div><![CDATA[><svg>]]><script>{RAW_CODE}</script></svg></div>"))),
            ("/cdata-open", fo(&format!("<![CDATA[ x]]><script>{HTML}</script>"))),
            ("/cdata-unsure", fo(&format!("<svg><p></p><![CDATA[><svg><script>{RAW_CODE}</script>]]>"))),
        ],
    );
}

// ---- the review against parse5: CDATA at integration points, the body's void and ignored tags,
// ---- encodings with character references, end tags, markup that stops inside a tag -----------

const REVIEWED: &str = r##"route GET /cdata-mi ? { q: str = "" } = <html><body><math><mi>{raw("<![CDATA[><svg>]]><script>" + q + "</script>")}</mi></math></body></html>
route GET /cdata-desc ? { q: str = "" } = <html><body><svg><desc>{raw("<![CDATA[><math>]]><script>" + q + "</script>")}</desc></svg></body></html>
route GET /cdata-ann ? { q: str = "" } = <html><body><math><annotation-xml encoding="text/html">{raw("<![CDATA[><svg>]]><script>" + q + "</script>")}</annotation-xml></math></body></html>
route GET /cdata-renderer ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<![CDATA[><svg>]]>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /cdata-text ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<![CDATA[>x]]>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /tag ? { q: str = "", t: str = "" } = <html><body><svg>{raw("<foreignObject>" + t + "</foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /ann-ref ? { q: str = "" } = <html><body><math>{raw("<annotation-xml encoding='text&#47;html'><script>" + q + "</script></annotation-xml>")}</math></body></html>
route GET /ann-ref-cdata ? { q: str = "" } = <html><body><math>{raw("<annotation-xml encoding='text&#47;html'><x><![CDATA[ > </x></annotation-xml><script>" + q + "</script> ]]>")}</math></body></html>
route GET /ann-plain ? { q: str = "" } = <html><body><math>{raw("<annotation-xml encoding='text/html'><script>" + q + "</script></annotation-xml>")}</math></body></html>
route GET /ann-renderer ? { q: str = "", e: str = "" } = <html><body><math><annotation-xml encoding={e}><script>{raw(q)}</script></annotation-xml></math></body></html>
route GET /endtag-other ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><svg></x><script>" + q + "</script>")}</svg></body></html>
route GET /endtag-outer ? { q: str = "" } = <html><body><svg><foreignObject>{raw("</foreignObject>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /endtag-walk ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><x-y></foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /endtag-rule ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><div><span></div><script>" + q + "</script></foreignObject>")}</svg></body></html>
route GET /scriptx ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><script></scriptx></script><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /escaped ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<script><!--<script></script><script>" + q + "</script>")}</foreignObject></svg></body></html>
route GET /input-open ? { q: str = "" } = <html><body><svg>{raw("<input")}<foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /end-open ? { q: str = "" } = <html><body><svg>{raw("</")}<foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /end-x-open ? { q: str = "" } = <html><body><svg>{raw("</x")}<foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /quote-open ? { q: str = "" } = <html><body><svg>{raw("<g a=\"x")}<foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /single-open ? { q: str = "" } = <html><body><svg>{raw("<g a='x")}<foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /textarea-open ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<textarea>")}<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /script-tag-open ? { q: str = "" } = <html><body><svg>{raw("<script")}<foreignObject><script>{raw(q)}</script></foreignObject></svg></body></html>
"##;

/// The inputs a review found against parse5 (a spec-exact parser standing in for the browser),
/// each now written as the browser reads it, or as safe in both:
/// - `<![CDATA[` at an integration point (`<mi>`, `<desc>`, an HTML `annotation-xml`, a
///   `<foreignObject>` before the renderer's own script) is a bogus comment to browsers: the
///   `<svg>` after its `>` holds the script, written in a section (Q-1113's review);
/// - an HTML tag the body treats as void or passes over (`<param>`, `<keygen>`, `<image>`,
///   `<basefont>`, `<bgsound>`, `<frame>`, `<head>`, `<html>`, `<body>`, `<col>`), with its slash
///   or not, leaves nothing open, so `</foreignObject>` ends the integration point and the script
///   after is SVG's; after a table's parts, `<select>`, `<template>` or `<frameset>` the parser is
///   not followed; a `<span/>` is open, and `</foreignObject>` in it ends nothing (Q-1100);
/// - an `encoding` with a character reference (`text&#47;html`, HTML's to the browser) is not
///   known (Q-1117);
/// - an end tag that names no element open here, or another than the last, and `</scriptx>`, do
///   not end what they do not name; an HTML script with `<!--<script>` is not followed; markup
///   that stops inside a tag, `</`, a quoted value or a `<textarea>` is ended, else the browser
///   reads the renderer's next tag into it (Q-1116)
/// (src/foreign.rs `written`, `end_tag`, `passed_over`, `tag_end`, `find_end_tag`,
/// `kind_written`; FEEDBACK71.md Q-969, coverage of 0.8.7, Q-1100, Q-1113, Q-1116, Q-1117; branch
/// coverage-087).
#[test]
fn the_inputs_a_review_found_against_parse5_are_read_as_the_browser_reads_them() {
    let mut it = app(REVIEWED);
    let fo = |inner: &str| format!("<svg><foreignObject>{inner}</foreignObject></svg>");
    let mut cases: Vec<(String, String)> = vec![
        ("/cdata-mi".into(), format!("<math><mi><![CDATA[><svg>]]><script>{RAW_CODE}</script></mi></math>")),
        ("/cdata-desc".into(), format!("<svg><desc><![CDATA[><math>]]><script>{RAW_CODE}</script></desc></svg>")),
        ("/cdata-ann".into(), format!("<math><annotation-xml encoding=\"text/html\"><![CDATA[><svg>]]><script>{RAW_CODE}</script></annotation-xml></math>")),
        ("/cdata-renderer".into(), fo(&format!("<![CDATA[><svg>]]><script>{CODE}</script>"))),
        // No markup between the two ends: either way the same, and the script is HTML's.
        ("/cdata-text".into(), fo(&format!("<![CDATA[>x]]><script>{HTML}</script>"))),
        ("/ann-ref".into(), format!("<math><annotation-xml encoding='text&#47;html'><script>{RAW_CODE}</script></annotation-xml></math>")),
        ("/ann-ref-cdata".into(), format!("<math><annotation-xml encoding='text&#47;html'><x><![CDATA[ > </x></annotation-xml><script>{RAW_CODE}</script> ]]></math>")),
        ("/ann-plain".into(), format!("<math><annotation-xml encoding='text/html'><script>{Q}</script></annotation-xml></math>")),
        // Written by the renderer, `&` is `&amp;`: the browser reads `text&#47;html` as it is,
        // no HTML encoding, and the section is right.
        ("/ann-renderer?e=text%26%2347%3Bhtml".into(), format!("<math><annotation-xml encoding=\"text&amp;#47;html\"><script>{CODE}</script></annotation-xml></math>")),
        ("/endtag-other".into(), format!("<svg><foreignObject><svg></x><script>{RAW_CODE}</script></svg>")),
        ("/endtag-outer".into(), fo(&format!("</foreignObject><script>{CODE}</script>"))),
        ("/endtag-walk".into(), format!("<svg><foreignObject><x-y></foreignObject><script>{Q}</script></svg>")),
        ("/endtag-rule".into(), format!("<svg><foreignObject><div><span></div><script>{RAW_CODE}</script></foreignObject></svg>")),
        ("/scriptx".into(), fo(&format!("<svg><script><![CDATA[</scriptx>]]></script><script>{RAW_CODE}</script>"))),
        ("/escaped".into(), fo(&format!("<script><!--<script></script><script>{RAW_CODE}</script>"))),
        ("/input-open".into(), format!("<svg><input><foreignObject><script>{HTML}</script></foreignObject></svg>")),
        ("/end-open".into(), format!("<svg></><foreignObject><script>{HTML}</script></foreignObject></svg>")),
        ("/end-x-open".into(), format!("<svg></x><foreignObject><script>{CODE}</script></foreignObject></svg>")),
        ("/quote-open".into(), format!("<svg><g a=\"x\"><foreignObject><script>{CODE}</script></foreignObject></svg>")),
        ("/single-open".into(), format!("<svg><g a='x'><foreignObject><script>{CODE}</script></foreignObject></svg>")),
        ("/textarea-open".into(), fo(&format!("<textarea></textarea><script>{HTML}</script>"))),
        ("/script-tag-open".into(), format!("<svg><script></script><foreignObject><script>{HTML}</script></foreignObject></svg>")),
    ];
    // Leaving nothing open, or after which the parser is not followed: the script after
    // `</foreignObject>` is SVG's, in a section, with the slash or without.
    for t in [
        "param", "keygen", "image", "basefont", "bgsound", "frame", "head", "html", "body", "col", "td", "th", "tr", "tbody", "thead", "tfoot", "caption",
        "colgroup", "table", "select", "template", "frameset",
    ] {
        for slash in ["", "/"] {
            let tag = format!("<{t}{slash}>");
            cases.push((format!("/tag?t={}", enc(&tag)), format!("<svg><foreignObject>{tag}</foreignObject><script>{RAW_CODE}</script></svg>")));
        }
    }
    // Open, as the browser keeps them: `</foreignObject>` ends nothing, and the script is HTML's.
    for tag in ["<span/>", "<div/>", "<x-y/>", "<b/>"] {
        cases.push((format!("/tag?t={}", enc(tag)), format!("<svg><foreignObject>{tag}</foreignObject><script>{Q}</script></svg>")));
    }
    let cases: Vec<(&str, String)> = cases.iter().map(|(p, w)| (p.as_str(), w.clone())).collect();
    pages(&mut it, &cases);
}

// ---- the second review: elements a start tag closes, a script's end in its own CDATA, a trailing
// ---- `<`, and markup split over values -----------------------------------------------------------

const IMPLIED: &str = r##"route GET /tag ? { q: str = "", t: str = "" } = <html><body><svg>{raw("<foreignObject>" + t + "</foreignObject><script>" + q + "</script>")}</svg></body></html>
route GET /form-outer ? { q: str = "" } = <html><body><form><svg><foreignObject>{raw("<form></foreignObject><script>" + q + "</script>")}</foreignObject></svg></form></body></html>
route GET /cdata-end ? { q: str = "" } = <html><body><svg>{raw("<svg><script><![CDATA[ var s = \"</script><img src=x onerror=1>\"; ]]></script></svg>")}</svg></body></html>
route GET /cdata-end-fo ? { q: str = "" } = <html><body><svg>{raw("<foreignObject><svg><script><![CDATA[ x=\"</script><img src=x onerror=1>\" ]]></script></svg></foreignObject>")}</svg></body></html>
route GET /comment-end ? { q: str = "" } = <html><body><svg>{raw("<script><!-- </script><img src=x onerror=1> --></script>")}</svg></body></html>
route GET /attr-end ? { q: str = "" } = <html><body><svg>{raw("<script><g a='</script><img src=x onerror=1>'></g></script>")}</svg></body></html>
route GET /cdata-end-unsure ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<svg><p></p><script><![CDATA[ \"</script><img src=x onerror=1>\" ]]></script>")}</foreignObject></svg></body></html>
route GET /style-cdata ? { q: str = "" } = <html><body><svg>{raw("<style><![CDATA[ a{} </style><img src=x onerror=1> ]]></style>")}</svg></body></html>
route GET /lt-text ? { q: str = "" } = <html><body><svg><foreignObject>{raw("<")}hello<script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /lt-tag ? { q: str = "" } = <html><body><svg><foreignObject>{raw("x <")}<b>bold</b><script>{raw(q)}</script></foreignObject></svg></body></html>
route GET /split-comment ? { q: str = "" } = <html><body><svg>{raw("<!--")}<text>hidden</text>{raw("-->")}<script>{raw(q)}</script></svg></body></html>
route GET /split-attr ? { u: str = "/x" } = <html><body><svg>{raw("<a href='")}{u}{raw("'>link</a>")}</svg></body></html>
route GET /split-script ? { d: str = "1" } = <html><body><svg>{raw("<script>var d = ")}{d}{raw(";</script>")}</svg></body></html>
route GET /complete = <html><body><svg>{raw("<g a='1'><!-- c --><text>t</text></g><style>a{}</style><script><![CDATA[ x < 1 ]]></script>")}</svg></body></html>
"##;

/// The second review's inputs against parse5:
/// - a start tag that closes an element by itself (a `<p>` in a `<p>` or before a `<div>`, an
///   `<li>` in an `<li>`, `<dd>`/`<dt>`, `<h1>`/`<h2>`, `<button>`, `<a>`, `<nobr>`, `<option>`,
///   `<rt>` after `<rb>`) leaves the browser's elements open other than these: an end tag that goes
///   down past one of them is not followed, and the script after `</foreignObject>` is written in
///   a section (the browser had ended the integration point); a `<form>` (which a page's own
///   form makes the browser pass over) is not followed either; written closed, the same markup
///   is followed as it is (Q-1116);
/// - an SVG script's end is past the CDATA sections, comments and tags in it, so its own
///   `"</script><img ...>"` stays text in it, the markup unchanged; where it is not known whether
///   the script is SVG's or HTML's, its inner `</script` is written `<\/script` so that both end it
///   at the same place (Q-1116);
/// - a `<` that ends a value is written `&lt;`, so the renderer's next text does not make it a tag
///   (Q-1116);
/// - markup split over values is ended where each value stops (a comment, a quoted attribute, a
///   script), so what the second value holds is text; complete markup is written as it is (Q-1116)
/// (src/foreign.rs `written`, `end_tag`, `foreign_end`, `cdata_text`, `escape_end_tags`;
/// FEEDBACK71.md Q-969, coverage of 0.8.7, Q-1116; branch coverage-087).
#[test]
fn the_second_reviews_inputs_are_read_as_the_browser_reads_them() {
    let mut it = app(IMPLIED);
    let mut cases: Vec<(String, String)> = Vec::new();
    for t in [
        "<p>one<p>two</p>",
        "<p>one<div>two</div>",
        "<li>a<li>b</li>",
        "<dd>a<dt>b</dt>",
        "<h1>a<h2>b</h2>",
        "<button>a<button>b</button>",
        "<a>a<a>b</a>",
        "<nobr>a<nobr>b</nobr>",
        "<option>a<option>b</option>",
        "<ruby><rb>a<rt>b</rt></ruby>",
        "<b><p>x</b></p>",
        "<form><form></form>",
        // Each closed by its own end tag: followed, and the script after is SVG's all the same.
        "<p>one</p><p>two</p>",
        "<li>a</li><li>b</li>",
    ] {
        cases.push((format!("/tag?t={}", enc(t)), format!("<svg><foreignObject>{t}</foreignObject><script>{RAW_CODE}</script></svg>")));
    }
    cases.extend([
        ("/form-outer".to_string(), format!("<form><svg><foreignObject><form></foreignObject><script>{RAW_CODE}</script></foreignObject></svg></form>")),
        ("/cdata-end".into(), "<svg><svg><script><![CDATA[ var s = \"</script><img src=x onerror=1>\"; ]]></script></svg></svg>".into()),
        ("/cdata-end-fo".into(), "<svg><foreignObject><svg><script><![CDATA[ x=\"</script><img src=x onerror=1>\" ]]></script></svg></foreignObject></svg>".into()),
        // A comment in it ends nothing, and is put in a section, as markup in an SVG script is.
        ("/comment-end".into(), "<svg><script><![CDATA[<!-- </script><img src=x onerror=1> -->]]></script></svg>".into()),
        ("/attr-end".into(), "<svg><script><![CDATA[<g a='</script><img src=x onerror=1>'></g>]]></script></svg>".into()),
        ("/cdata-end-unsure".into(), "<svg><foreignObject><svg><p></p><script><![CDATA[ \"<\\/script><img src=x onerror=1>\" ]]></script></foreignObject></svg>".into()),
        ("/style-cdata".into(), "<svg><style><![CDATA[ a{} </style><img src=x onerror=1> ]]></style></svg>".into()),
        ("/lt-text".into(), format!("<svg><foreignObject>&lt;hello<script>{HTML}</script></foreignObject></svg>")),
        ("/lt-tag".into(), format!("<svg><foreignObject>x &lt;<b>bold</b><script>{HTML}</script></foreignObject></svg>")),
        // Split over values: each value's markup ended where it stops (new in round 4; a decision
        // for the user), the second value's then text.
        ("/split-comment".into(), format!("<svg><!----><text>hidden</text>--><script>{CODE}</script></svg>")),
        ("/split-attr?u=%2Fx".into(), "<svg><a href=''>/x'>link</a></svg>".into()),
        ("/split-script?d=1".into(), "<svg><script>var d = </script>1;</script></svg>".into()),
        ("/complete".into(), "<svg><g a='1'><!-- c --><text>t</text></g><style>a{}</style><script><![CDATA[ x < 1 ]]></script></svg>".into()),
    ]);
    let cases: Vec<(&str, String)> = cases.iter().map(|(p, w)| (p.as_str(), w.clone())).collect();
    pages(&mut it, &cases);
}

// ---- a place lost for the rest of the page, and the next request -------------------------------

const PAGES: &str = r##"fn boom(z: int) = 10 / z
view Bad(z: int) = <g>{boom(z)}</g>
component Bar(h: int) {
    state tall = false
    <rect class="bar" width="10" height={tall ? h * 2 : h} @click={() => { tall = true }}/>
}
route GET /reopen ? { q: str = "" } = <html><body><svg><g>{raw("</svg><svg><svg>")}</g></svg><script>{raw(q)}</script></body></html>
route GET /reopen-later ? { q: str = "" } = <html><body><svg><g>{raw("</svg><svg><svg>")}</g></svg><p>after</p><div><script>{raw(q)}</script></div></body></html>
route GET /boom-child ? { z: int = 0 } = <html><body><svg><g>{boom(z)}</g></svg></body></html>
route GET /boom-attr ? { z: int = 0 } = <html><body><svg><g x={boom(z)}></g></svg></body></html>
route GET /boom-view ? { z: int = 0 } = <html><body><svg><Bad z={z}/></svg></body></html>
route GET /boom-raw ? { z: int = 0 } = <html><body><svg>{raw(str(boom(z)))}</svg></body></html>
route GET /boom-script ? { z: int = 0 } = <html><body><svg><script>{raw(str(boom(z)))}</script></svg></body></html>
route GET /boom-point ? { z: int = 0 } = <html><body><svg><foreignObject>{raw("<div>")}{boom(z)}</foreignObject></svg></body></html>
route GET /boom-mi ? { z: int = 0 } = <html><body><math><mi><svg><g>{boom(z)}</g></svg></mi></math></body></html>
route GET /plain ? { q: str = "" } = <html><head><title>p</title></head><body><p>x</p><script>{raw(q)}</script><svg><g><text>t</text></g></svg><script>{raw(q)}</script></body></html>
route GET /island ? { q: str = "" } = <html><body><svg><Bar h={4}/></svg><script>{raw(q)}</script></body></html>
"##;

/// A raw value that closes the renderer's `<svg>` and opens its own (`</svg><svg><svg>`) leaves
/// the browser in SVG past the renderer's own `</svg>`: the place is lost for the rest of the page
/// (`foreign::LOST`, which `leave` keeps), and a script after it, in the page's HTML, is written
/// as safe in both (in 0.8.7 as HTML's, which the browser read as SVG's: the parse5 review's
/// `reopen`) (src/foreign.rs `LOST`, `leave`, `written`; FEEDBACK71.md Q-969, coverage of 0.8.7,
/// Q-1116; branch coverage-087).
#[test]
fn a_place_lost_in_a_page_stays_lost_to_its_end() {
    let mut it = app(PAGES);
    pages(
        &mut it,
        &[
            ("/reopen", format!("<svg><g></svg><svg><svg></g></svg><script>{CODE}</script>")),
            ("/reopen-later", format!("<svg><g></svg><svg><svg></g></svg><p>after</p><div><script>{CODE}</script></div>")),
        ],
    );
}

/// The parser's place starts at HTML for each request (`Interp::page_start`): after a page that
/// lost it, or one that failed inside `<svg>`, `<math>` or a `<foreignObject>` (a child, an
/// attribute, a view, a raw value, a script's value), the same worker's next page is written as a
/// fresh server writes it, its scripts HTML's where HTML is read and a component in `<svg>` in its
/// SVG place (src/interp.rs `page_start`; FEEDBACK71.md Q-969, coverage of 0.8.7, Q-1116; branch
/// coverage-087).
#[test]
fn the_next_request_on_a_worker_starts_where_a_fresh_server_does() {
    let fresh = |path: &str| page(&mut app(PAGES), path);
    let (plain, island) = (fresh("/plain"), fresh("/island"));
    assert_eq!(plain, format!("<p>x</p><script>{HTML}</script><svg><g><text>t</text></g></svg><script>{HTML}</script>"));
    assert!(island.starts_with("<svg><rect data-qi data-c=\"Bar\""), "{}", island);
    assert!(island.ends_with(&format!("</svg><script>{HTML}</script>")), "{}", island);
    let mut it = app(PAGES);
    let failing = |it: &mut Interp, path: &str| {
        let r = it.handle(&HttpRequest { method: "GET".into(), url: path.into(), headers: vec![("Host".into(), "localhost".into())], body: Vec::new(), remote: "127.0.0.1".into() });
        assert_eq!(r.status, 500, "{}: {}", path, r.text());
    };
    for before in ["/reopen", "/reopen-later", "/boom-child", "/boom-attr", "/boom-view", "/boom-raw", "/boom-script", "/boom-point", "/boom-mi"] {
        for _ in 0..2 {
            if before.starts_with("/boom") {
                failing(&mut it, before);
            } else {
                page(&mut it, before);
            }
            assert_eq!(page(&mut it, "/plain"), plain, "/plain after {}", before);
            assert_eq!(page(&mut it, "/island"), island, "/island after {}", before);
        }
    }
}
