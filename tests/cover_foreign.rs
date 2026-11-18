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
            // A `<div>` left open in a `<foreignObject>`, or its tag unended: not known; closed, HTML.
            ("/open", format!("<svg><foreignObject><div><script>{CODE}</script></foreignObject></svg>")),
            ("/closed", format!("<svg><foreignObject><div></div><script>{HTML}</script></foreignObject></svg>")),
            ("/unended", format!("<svg><foreignObject><div<script>{CODE}</script></foreignObject></svg>")),
            // A script that is never ended: its text to the end, in a section.
            ("/no-end-tag", "<svg><script><![CDATA[a</b><b>x</b>&quot;]]]]><![CDATA[>]]></svg>".to_string()),
            // `<font>` alone is SVG's; given `color`, it ends SVG; unended, what follows is not known.
            ("/font-bare", format!("<svg><font>x</font><script>{CODE}</script></svg>")),
            ("/font-color", format!("<svg><font color=\"red\">x</font><script>{HTML}</script></svg>")),
            ("/font-unended", format!("<svg><font x<script>{CODE}</script></svg>")),
            ("/font-quote", format!("<svg><font face=\"x<script>{CODE}</script></svg>")),
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
