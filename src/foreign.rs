//! Where the browser's parser is, HTML, SVG or MathML, as the server writes a page: what it takes
//! each element for, by WHATWG HTML's tree construction ("the rules for parsing tokens in foreign
//! content"). A `<script>` the parser reads as SVG's or MathML's has its text read as markup, so
//! an `html` value (`raw(...)`) in it goes in a CDATA section there, and nowhere else (see
//! `Interp::render_contents`). Only an element inside `<svg>` or `<math>`, or one that opens them,
//! asks: on a page without them the renderer looks at one flag (FEEDBACK71.md, Q-969; branch
//! security).

use crate::ast::{AttrVal, BinOp, Expr, FnDef, HtmlChild, HtmlEl, Lit, Pattern, Stmt};
use crate::value::{Closure, Env, Value};
use std::cell::Cell;
use std::sync::Arc;

/// What an element's tag is to the parser, worked out when it is parsed (`ElFacts::ns`). Its case
/// does not count, as it does not for the parser: `foreignobject` is SVG's `foreignObject`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// Any other tag: in foreign content it is an element of the same language.
    #[default]
    Other,
    /// `<svg>` and `<math>`: foreign content starts at them, from HTML.
    Svg,
    Math,
    /// SVG's `<foreignObject>`, `<desc>` and `<title>`: HTML again in them (HTML integration
    /// points), only inside SVG.
    SvgPoint,
    /// MathML's `<mi>`, `<mo>`, `<mn>`, `<ms>` and `<mtext>`: HTML again in them, but for
    /// `<mglyph>` and `<malignmark>` (MathML text integration points), only inside MathML.
    MathText,
    /// `<mglyph>` and `<malignmark>`, MathML's still directly inside a MathML text element.
    Glyph,
    /// MathML's `<annotation-xml>`: HTML again in it when its `encoding` is `text/html` or
    /// `application/xhtml+xml`; `AnnotationAsk` where that is known only once it is written.
    AnnotationHtml,
    Annotation,
    AnnotationAsk,
    /// A tag that ends foreign content where it stands (`<p>`, `<div>`, `<b>`, `<img>`, ... and a
    /// `<font>` with `color`, `face` or `size`); `FontAsk`, a `<font>` whose attributes are known
    /// only once written.
    Breakout,
    FontAsk,
}

/// The start tags that end foreign content (WHATWG HTML, 13.2.6.5), with `<font>` given `color`,
/// `face` or `size`.
const BREAKOUT: &[&str] = &[
    "b", "big", "blockquote", "body", "br", "center", "code", "dd", "div", "dl", "dt", "em", "embed", "h1", "h2", "h3", "h4", "h5", "h6", "head", "hr", "i",
    "img", "li", "listing", "menu", "meta", "nobr", "ol", "p", "pre", "ruby", "s", "small", "span", "strong", "strike", "sub", "sup", "table", "tt", "u",
    "ul", "var",
];

const FONT_ATTRS: [&str; 3] = ["color", "face", "size"];

/// What `tag` is to the parser (see `Kind`). A view's or a component's tag is none of these.
pub fn kind_of(tag: &str, attrs: &[(String, AttrVal)], component: bool) -> Kind {
    if component {
        return Kind::Other;
    }
    let t = tag.to_ascii_lowercase();
    let spread = attrs.iter().any(|(_, a)| matches!(a, AttrVal::Spread(_)));
    let attr = |name: &str| attrs.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, a)| a);
    match t.as_str() {
        "annotation-xml" if spread => Kind::AnnotationAsk,
        "annotation-xml" => match attr("encoding") {
            None | Some(AttrVal::Bool) => Kind::Annotation,
            Some(AttrVal::Expr(Expr::Lit(Lit::Str(s)))) if html_encoding(s) => Kind::AnnotationHtml,
            Some(AttrVal::Expr(Expr::Lit(Lit::Str(_)))) => Kind::Annotation,
            Some(_) => Kind::AnnotationAsk,
        },
        "font" if spread => Kind::FontAsk,
        "font" => {
            let given: Vec<&AttrVal> = FONT_ATTRS.iter().filter_map(|n| attr(n)).collect();
            if given.iter().any(|a| matches!(a, AttrVal::Bool | AttrVal::Expr(Expr::Lit(Lit::Str(_))))) {
                Kind::Breakout
            } else if given.is_empty() {
                Kind::Other
            } else {
                Kind::FontAsk
            }
        }
        t => name_kind(t),
    }
}

/// What a tag's name in lower case is to the parser, `annotation-xml` and `font` aside (see
/// `kind_of`, `kind_written`).
fn name_kind(t: &str) -> Kind {
    match t {
        "svg" => Kind::Svg,
        "math" => Kind::Math,
        "foreignobject" | "desc" | "title" => Kind::SvgPoint,
        "mi" | "mo" | "mn" | "ms" | "mtext" => Kind::MathText,
        "mglyph" | "malignmark" => Kind::Glyph,
        t if BREAKOUT.contains(&t) => Kind::Breakout,
        _ => Kind::Other,
    }
}

/// An `encoding` that makes an `<annotation-xml>` an HTML integration point: compared as the
/// parser compares it, ASCII case aside, nothing trimmed.
fn html_encoding(s: &str) -> bool {
    s.eq_ignore_ascii_case("text/html") || s.eq_ignore_ascii_case("application/xhtml+xml")
}

/// A `Kind` known only once its start tag is written (`<annotation-xml encoding={e}>`, a spread on
/// a `<font>`): what the written tag says. `tag` is the start tag as written, `<annotation-xml
/// encoding="text/html">`, its values escaped (neither encoding has a character escaping changes).
pub fn kind_written(kind: Kind, tag: &str) -> Kind {
    match kind {
        Kind::AnnotationAsk => match written_attr(tag, "encoding") {
            Some(v) if html_encoding(v) => Kind::AnnotationHtml,
            _ => Kind::Annotation,
        },
        Kind::FontAsk if FONT_ATTRS.iter().any(|n| written_attr(tag, n).is_some()) => Kind::Breakout,
        Kind::FontAsk => Kind::Other,
        k => k,
    }
}

/// The value of the first attribute `name` (ASCII case aside, as the parser reads names) in a
/// start tag the renderer wrote: `name="value"`, or `name` alone, which is `""`.
fn written_attr<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let b = tag.as_bytes();
    // Past `<` and the tag's name.
    let mut i = b.iter().position(|c| c.is_ascii_whitespace())?;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() || b[i] == b'>' {
            return None;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let here = &tag[start..i];
        let mut value = "";
        if b.get(i) == Some(&b'=') && b.get(i + 1) == Some(&b'"') {
            let from = i + 2;
            let to = tag[from..].find('"').map_or(tag.len(), |n| from + n);
            value = &tag[from..to];
            i = to + 1;
        }
        if here.eq_ignore_ascii_case(name) {
            return Some(value);
        }
    }
    None
}

/// What the parser's current element is, which decides what the next start tag makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ns {
    /// An HTML element (or the document).
    #[default]
    Html,
    /// An HTML integration point that is no HTML element: SVG's `foreignObject`, `desc` or
    /// `title`, an `annotation-xml` with an HTML encoding. Start tags make HTML elements there.
    Point,
    /// A MathML text integration point (`mi`, ...): start tags make HTML elements there, but
    /// `mglyph` and `malignmark`.
    MathText,
    /// An SVG element, a MathML element, an `annotation-xml` without an HTML encoding (where an
    /// `<svg>` is SVG's): foreign content, where a script's text is markup.
    Svg,
    Math,
    Annotation,
    /// Not known: the page is written as it is safe in either (see `At::script_is_foreign`).
    Unsure,
}

impl Ns {
    #[inline]
    pub fn foreign(self) -> bool {
        matches!(self, Ns::Svg | Ns::Math | Ns::Annotation)
    }
}

/// Where the renderer is, as the parser will be: the current element (`now`), and of the foreign
/// content it is in (a run of SVG or MathML elements started at an `<svg>` or `<math>`), what
/// that run started from (`base`) and, once a tag that ends foreign content has come (`<p>`),
/// where what follows in it goes (`post`): the element the parser went back to, HTML when that
/// was an HTML element, else not known (an end tag that comes later, `</g>`, may close an SVG
/// element outside it there; see `enter`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct At {
    pub now: Ns,
    base: Ns,
    post: Option<Ns>,
}

/// Not known where, nor where what follows goes.
pub const UNSURE: At = At { now: Ns::Unsure, base: Ns::Html, post: Some(Ns::Unsure) };

impl At {
    /// Whether a `<script>` whose contents are here has its text read as markup, or may: an
    /// `html` value in it goes in a CDATA section. The safe form where it is not known: there it
    /// is text in an SVG or MathML script; in an HTML script, `<![CDATA[` is text too, which
    /// JavaScript refuses, so the script stops with a syntax error and no markup gets out. Left
    /// out where the parser reads SVG, the visitor's `<img onerror>` is an element.
    #[inline]
    pub fn script_is_foreign(self) -> bool {
        matches!(self.now, Ns::Svg | Ns::Math | Ns::Unsure)
    }
}

/// Where an element's contents are, the element opened at `at` (its `Kind` as written; see
/// `kind_written`).
#[inline]
pub fn enter(at: At, kind: Kind) -> At {
    use Ns::*;
    match at.now {
        Unsure => at,
        Html | Point | MathText => {
            let to = match kind {
                Kind::Svg => Svg,
                Kind::Math => Math,
                Kind::Glyph if at.now == MathText => Math,
                _ => Html,
            };
            if to.foreign() {
                // A run of foreign content starts here.
                At { now: to, base: at.now, post: None }
            } else {
                At { now: Html, ..at }
            }
        }
        Svg => match kind {
            Kind::Breakout => broken(at),
            Kind::SvgPoint => At { now: Point, ..at },
            // A `<font>` whose attributes a walk cannot see (the renderer reads them first).
            Kind::FontAsk => unsure(at),
            _ => At { now: Svg, ..at },
        },
        Math | Annotation => match kind {
            Kind::Breakout => broken(at),
            Kind::MathText => At { now: MathText, ..at },
            Kind::AnnotationHtml => At { now: Point, ..at },
            Kind::Annotation => At { now: Annotation, ..at },
            Kind::Svg if at.now == Annotation => At { now: Svg, ..at },
            Kind::AnnotationAsk | Kind::FontAsk => unsure(at),
            _ => At { now: Math, ..at },
        },
    }
}

/// Not known, in the run `at` is in, nor what follows in it.
fn unsure(at: At) -> At {
    At { now: Ns::Unsure, base: at.base, post: Some(Ns::Unsure) }
}

/// A tag that ends foreign content, at `at`: the parser closes its run's elements up to the
/// element the run started in and puts this one there, an HTML element. What follows in the run
/// goes there too: HTML where that is an HTML element; else not known, as a later end tag can
/// close an element of an outer run and leave the parser in SVG (`<svg><g><foreignObject><svg><g>
/// <p></p></g>` has `</g>` close the outer `<g>`).
fn broken(at: At) -> At {
    let post = if at.base == Ns::Html { Ns::Html } else { Ns::Unsure };
    At { now: Ns::Html, base: at.base, post: Some(post) }
}

/// Where the parser is after an element: `outer` where it was opened, `inner` its contents as
/// `enter` gave them, `cur` where they ended.
#[inline]
pub fn leave(outer: At, inner: At, cur: At) -> At {
    if outer.now == Ns::Unsure {
        return outer;
    }
    if inner.now.foreign() && !outer.now.foreign() {
        // The run it started ends, and what was around it is back; but where its end was not
        // known and it stood in an integration point, nor is what follows in the run outside.
        if cur.post == Some(Ns::Unsure) && outer.now != Ns::Html {
            return At { now: Ns::Unsure, base: outer.base, post: Some(Ns::Unsure) };
        }
        return outer;
    }
    match cur.post {
        Some(p) if outer.now.foreign() => At { now: p, ..cur },
        _ => At { now: outer.now, ..cur },
    }
}

/// Html the renderer did not write where it stands (a value: `raw(...)`, a view's children,
/// markup made apart and placed here; or what a value, a view or a component wrote, looked at as
/// a whole), written at `at`, read as the parser will read it: where the parser is after it, and
/// that html again (`Some`) where a script in it is one the parser reads as SVG's or MathML's
/// and its text was not written for that. Such text was made for HTML (`let s =
/// <script>{raw(x)}</script>`, then `<svg>{s}</svg>`): its markup would be elements there, and
/// `&quot;` a quote. It goes in CDATA sections, the parts that are in none already, a `]]>` in
/// them split, so the parser takes it as the text it is in HTML; a section it opened and left
/// open is closed. Only inside `<svg>` or `<math>` is this asked (`Interp::render_unseen`). The
/// html is taken as whole, its elements closed as the renderer writes them (FEEDBACK71.md, Q-969,
/// found in review; branch security).
pub fn written(at: At, html: &str) -> (At, Option<String>) {
    let b = html.as_bytes();
    let mut open: Vec<(At, At)> = Vec::new();
    let mut cur = at;
    let mut fixed: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    while let Some(n) = b[i..].iter().position(|&c| c == b'<') {
        i += n;
        if b[i..].starts_with(b"<!--") {
            i = html[i + 4..].find("-->").map_or(b.len(), |e| i + 4 + e + 3);
            continue;
        }
        let end = b.get(i + 1) == Some(&b'/');
        let from = i + 1 + end as usize;
        if !b.get(from).is_some_and(|c| c.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let name_end = from + b[from..].iter().take_while(|c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'>')).count();
        let tag_end = tag_end(b, name_end);
        let tag = &html[i..tag_end];
        i = tag_end;
        if end {
            if let Some((outer, inner)) = open.pop() {
                cur = leave(outer, inner, cur);
            }
            continue;
        }
        // Its name in lower case, on the stack: none that matters is longer.
        let mut low = [0u8; 16];
        let len = name_end - from;
        let name = if len <= low.len() {
            low[..len].copy_from_slice(&b[from..name_end]);
            low[..len].make_ascii_lowercase();
            std::str::from_utf8(&low[..len]).unwrap_or("")
        } else {
            ""
        };
        let kind = match name {
            "annotation-xml" => kind_written(Kind::AnnotationAsk, tag),
            "font" => kind_written(Kind::FontAsk, tag),
            n => name_kind(n),
        };
        let inner = enter(cur, kind);
        // A script's text, and in HTML the text of the elements whose contents are text, runs to
        // its end tag.
        let text = name == "script"
            || (inner.now == Ns::Html && matches!(name, "style" | "textarea" | "title" | "xmp" | "iframe" | "noembed" | "noframes" | "noscript"));
        if text {
            let close = find_end_tag(&html[i..], name).map_or(b.len(), |e| i + e);
            if name == "script" && inner.script_is_foreign() {
                if let Some(t) = cdata_text(&html[i..close]) {
                    let f = fixed.get_or_insert_with(String::new);
                    f.push_str(&html[copied..i]);
                    f.push_str(&t);
                    copied = close;
                }
            }
            i = close;
        } else if tag.ends_with("/>") || crate::parser::VOID_TAGS.contains(&name) {
            // No end tag follows (the renderer writes none for `<input>`, in SVG too, where the
            // parser keeps it open; it holds what follows as SVG all the same).
            cur = leave(cur, inner, inner);
            continue;
        }
        open.push((cur, inner));
        cur = inner;
    }
    if let Some(f) = &mut fixed {
        f.push_str(&html[copied..]);
    }
    // An element left open (raw html that is not whole): what follows is in it, which is not
    // followed.
    if !open.is_empty() {
        cur = unsure(cur);
    }
    (cur, fixed)
}

/// Where a start tag whose name ends at `from` ends, past its `>`, a quoted value's `>` not
/// counted.
fn tag_end(b: &[u8], from: usize) -> usize {
    let mut quote = None;
    for (k, &c) in b.iter().enumerate().skip(from) {
        match (quote, c) {
            (None, b'"' | b'\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, b'>') => return k + 1,
            _ => {}
        }
    }
    b.len()
}

/// Where the first `</name` (`name` in lower case) is in `hay`, ASCII case aside.
fn find_end_tag(hay: &str, name: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), name.as_bytes());
    let mut k = 0;
    while let Some(at) = h[k..].windows(2).position(|w| w == b"</") {
        let from = k + at + 2;
        if h.get(from..from + n.len()).is_some_and(|x| x.eq_ignore_ascii_case(n)) {
            return Some(k + at);
        }
        k = from;
    }
    None
}

/// A script's text as the parser of SVG and MathML takes it as text: the parts outside CDATA
/// sections that it would read as markup (a `<` before a letter, `/`, `!` or `?`; a `&` before a
/// letter or `#`) in sections of their own, `]]>` split; a section left open closed. `None` where
/// it reads as text already.
fn cdata_text(t: &str) -> Option<String> {
    let markup = |s: &str| {
        let b = s.as_bytes();
        b.windows(2).any(|w| (w[0] == b'<' && (w[1].is_ascii_alphabetic() || matches!(w[1], b'/' | b'!' | b'?'))) || (w[0] == b'&' && (w[1].is_ascii_alphanumeric() || w[1] == b'#')))
    };
    let mut out = String::with_capacity(t.len() + 24);
    let mut changed = false;
    let mut rest = t;
    loop {
        let (plain, after) = match rest.find("<![CDATA[") {
            Some(k) => (&rest[..k], Some(&rest[k..])),
            None => (rest, None),
        };
        if markup(plain) {
            out.push_str("<![CDATA[");
            out.push_str(&plain.replace("]]>", "]]]]><![CDATA[>"));
            out.push_str("]]>");
            changed = true;
        } else {
            out.push_str(plain);
        }
        let Some(section) = after else { break };
        match section.find("]]>") {
            Some(k) => {
                out.push_str(&section[..k + 3]);
                rest = &section[k + 3..];
            }
            None => {
                out.push_str(section);
                out.push_str("]]>");
                changed = true;
                break;
            }
        }
    }
    changed.then_some(out)
}

/// What a walk takes for a value it cannot see written, at `at`: in a run that started in an
/// integration point, a `<p>` in it may leave the parser in SVG where the walk goes on as in
/// HTML (see `written`, `broken`), so what follows there is not known.
fn after_unseen(at: At) -> At {
    if at.now.foreign() && at.base != Ns::Html {
        UNSURE
    } else {
        at
    }
}

/// One `At` or, where two differ, `UNSURE`.
fn merge(a: At, b: At) -> At {
    if a == b {
        a
    } else {
        UNSURE
    }
}

/// `At` as one byte, for `ElFacts::kids_at`: 0 not asked yet, 1 nowhere, else the place.
pub fn encode(at: Option<At>) -> u8 {
    let ns = |n: Ns| n as u8;
    match at {
        None => 1,
        Some(a) => 2 + ns(a.now) * 24 + ns(a.base) * 3 + a.post.map_or(0, |p| if p == Ns::Html { 1 } else { 2 }),
    }
}

/// What `encode` wrote: `None` when not asked yet.
pub fn decode(b: u8) -> Option<Option<At>> {
    const ALL: [Ns; 7] = [Ns::Html, Ns::Point, Ns::MathText, Ns::Svg, Ns::Math, Ns::Annotation, Ns::Unsure];
    match b {
        0 => None,
        1 => Some(None),
        b => {
            let b = b - 2;
            let post = match b % 3 {
                0 => None,
                1 => Some(Ns::Html),
                _ => Some(Ns::Unsure),
            };
            Some(Some(At { now: ALL[(b / 24) as usize], base: ALL[((b % 24) / 3) as usize], post }))
        }
    }
}

/// `encode` of HTML, as a run starts it (`At::default()`).
pub const HTML_PLACE: u8 = 2;

/// What `ElFacts::kids_at` keeps: the view (its definition's address) and where it was called,
/// with the answer, in one word: `None` where the address does not fit in its 48 bits.
pub fn kept(def: usize, at: At, answer: Option<At>) -> Option<u64> {
    (def < 1 << 48).then(|| (def as u64) << 16 | (encode(Some(at)) as u64) << 8 | encode(answer) as u64)
}

/// The answer `kept` keeps for that view called there, if it is the one kept.
pub fn kept_answer(word: u64, def: usize, at: At) -> Option<Option<At>> {
    let key = kept(def, at, None)?;
    if word >> 8 == key >> 8 {
        decode(word as u8)
    } else {
        None
    }
}

// ---- where a view puts its children ---------------------------------------------------------
//
// A view's children are rendered before the view, into the html it is given as `children`. So
// they are rendered as where its body will put them, found by walking the body as the renderer
// would write it, through `let`s, branches, `return`s, `??`, loops, and the views and functions it
// hands them to. Where two places differ, or the walk cannot follow (a function it cannot see, a
// value kept in a list and taken out, a lambda), they are rendered at `UNSURE`.

/// How many views and functions deep a walk follows children, and how many steps it takes in all.
const DEPTH: u32 = 8;
const BUDGET: u32 = 20_000;
/// How many variables deep it follows one that holds another (`let t = <g>{s}</g>`).
const HOLDS: u32 = 8;

/// The builtins whose result holds nothing of what they are given: a number, a bool, a type's
/// name. Not `str`, whose text of a markup value `raw(...)` makes markup again.
const TEXT_BUILTINS: &[&str] = &["len", "type_of", "contains", "starts_with", "ends_with"];

/// Where the view `c` puts the children it is called with, called at `at`: `None` where its body
/// writes them nowhere in the page, `UNSURE` where the walk cannot tell.
pub fn children_at(c: &Closure, at: At) -> Option<At> {
    let budget = Cell::new(BUDGET);
    let out = Walk::run(c, &["children"], Some(at), &budget, 0);
    let found = if out.unsure { Some(UNSURE) } else { out.found };
    // Not known where, but called in HTML by a view that can reach no `<svg>` or `<math>`: in
    // HTML, wherever, as no foreign content can start around them. A page with neither had its
    // honest scripts in CDATA sections, which stopped them (FEEDBACK71.md, Q-969, found in
    // review; branch security).
    if found.is_some_and(|f| f.now == Ns::Unsure) && at.now == Ns::Html && !may_foreign(c, 0, &mut Vec::new()) {
        return Some(at);
    }
    found
}

/// Whether the body of `c`, or what it reaches (the views and functions it calls, lambdas written
/// in it), may start foreign content: an `<svg>` or a `<math>` element, or a view or function it
/// cannot name (a parameter, a variable), which might.
fn may_foreign(c: &Closure, depth: u32, seen: &mut Vec<*const FnDef>) -> bool {
    let def = Arc::as_ptr(&c.def);
    if seen.contains(&def) {
        return false;
    }
    if depth > DEPTH || crate::stack::remaining().is_some_and(|r| r < 256 << 10) {
        return true;
    }
    seen.push(def);
    let params: Vec<&str> = c.def.params.iter().map(|p| p.name.as_str()).collect();
    // The names its lambdas are kept in (`let f = (x) => ...`): their bodies are walked here.
    let mut lambdas: Vec<&str> = Vec::new();
    crate::check::walk_fn(&c.def, &mut |x| {
        if let Expr::Block(stmts) = x {
            for st in stmts {
                if let Stmt::Let(Pattern::Bind(n), Expr::Lambda(_), _) = st {
                    lambdas.push(n.as_str());
                }
            }
        }
    });
    let mut calls: Vec<String> = Vec::new();
    let mut found = false;
    crate::check::walk_fn(&c.def, &mut |x| match x {
        Expr::Html(el, _) => found |= els_foreign(el, &mut calls),
        Expr::Call(f, _, _) => {
            if let Expr::Ident(n, _) = &**f {
                calls.push(n.to_string());
            }
        }
        _ => {}
    });
    for name in calls {
        if found {
            break;
        }
        found = if params.contains(&name.as_str()) {
            true
        } else if lambdas.contains(&name.as_str()) {
            false
        } else {
            match crate::value::lookup(&c.env, name.as_str()) {
                Some(Value::Func(f)) => may_foreign(&f, depth + 1, seen),
                Some(Value::Builtin(..)) => false,
                _ => true,
            }
        };
    }
    found
}

/// Whether `el` or an element in it is an `<svg>` or a `<math>`; the views it calls go in `calls`.
fn els_foreign(el: &HtmlEl, calls: &mut Vec<String>) -> bool {
    if el.facts.ns_opens {
        return true;
    }
    if el.facts.component {
        calls.push(el.tag.clone());
    }
    el.children.iter().any(|c| matches!(c, HtmlChild::El(e) if els_foreign(e, calls)))
}

/// A walk's position: where on the page a value goes, `None` where it is made and not written
/// there (a condition, a statement's value).
type Pos = Option<At>;

fn merge_pos(a: Pos, b: Pos) -> Pos {
    match (a, b) {
        (Some(a), Some(b)) => Some(merge(a, b)),
        (a, b) => a.or(b),
    }
}

/// A variable, to a walk: one of the values it follows (a parameter), one that may hold them
/// (`let s = <svg>{children}</svg>`), or another.
#[derive(Clone, Copy)]
enum Bind<'a> {
    It,
    Holds(&'a Expr),
    Other,
}

struct Out {
    found: Option<At>,
    unsure: bool,
    after: Pos,
}

struct Walk<'a, 'b> {
    binds: Vec<(String, Bind<'a>)>,
    found: Option<At>,
    unsure: bool,
    /// Where a `return` puts its value: where the function's call is.
    fn_at: Pos,
    rets: Option<Pos>,
    env: &'a Env,
    budget: &'b Cell<u32>,
    depth: u32,
    holds: u32,
}

impl<'a, 'b> Walk<'a, 'b> {
    /// The body of `c`, called at `pos`, following its parameters `params`: where they go, and
    /// where the parser is after what the body writes.
    fn run(c: &'a Closure, params: &[&str], pos: Pos, budget: &'b Cell<u32>, depth: u32) -> Out {
        let def: &'a FnDef = &c.def;
        let mut w = Walk { binds: Vec::new(), found: None, unsure: false, fn_at: pos, rets: None, env: &c.env, budget, depth, holds: 0 };
        for p in &def.params {
            let b = if params.contains(&p.name.as_str()) { Bind::It } else { Bind::Other };
            w.binds.push((p.name.to_string(), b));
        }
        let end = w.placed(&def.body, pos);
        let after = match w.rets {
            Some(r) => merge_pos(end, r),
            None => end,
        };
        Out { found: w.found, unsure: w.unsure, after }
    }

    /// One more step, `n` of them: past the budget, or near the stack's end (each view and `let`
    /// it follows is a level deeper), the walk stops, not sure.
    fn tick(&mut self, n: u32) -> bool {
        let left = self.budget.get();
        if left < n || crate::stack::remaining().is_some_and(|r| r < 256 << 10) {
            self.unsure = true;
            self.budget.set(0);
            return false;
        }
        self.budget.set(left - n);
        true
    }

    fn bind(&self, n: &str) -> Option<Bind<'a>> {
        self.binds.iter().rev().find(|(m, _)| m == n).map(|(_, b)| *b)
    }

    fn record(&mut self, pos: Pos) {
        if let Some(at) = pos {
            let at = if at.now == Ns::Unsure { UNSURE } else { at };
            self.found = Some(match self.found {
                Some(f) => merge(f, at),
                None => at,
            });
        }
    }

    /// Whether `e` names a value followed, anywhere in it.
    fn mentions(&mut self, e: &Expr) -> bool {
        let mut hit = false;
        let mut n = 0u32;
        crate::check::walk_expr(e, &mut |x| {
            n += 1;
            if let Expr::Ident(name, _) = x {
                hit |= matches!(self.bind(name.as_str()), Some(Bind::It | Bind::Holds(_)));
            }
        });
        self.tick(n);
        hit
    }

    /// Whether an element names a value followed, in its attributes or anything in it.
    fn el_mentions(&mut self, el: &HtmlEl) -> bool {
        let mut hit = el.id.as_ref().is_some_and(|x| self.mentions(x)) || el.classes.iter().any(|x| self.mentions(x));
        for (_, a) in &el.attrs {
            if let AttrVal::Expr(x) | AttrVal::Spread(x) = a {
                hit |= self.mentions(x);
            }
        }
        for c in &el.children {
            hit |= match c {
                HtmlChild::El(e) => self.el_mentions(e),
                HtmlChild::Expr(x) => self.mentions(x),
            };
        }
        hit
    }

    /// Whether the value of `e` may hold a value followed, or its text: `children`, `children ??
    /// ""`, `[a, children]`, `<b>{children}</b>`, `str(children)` (which `raw` makes markup
    /// again); not `len(children)` or `children == ""`, a number or a bool.
    fn carries(&mut self, e: &Expr) -> bool {
        use BinOp::*;
        match e {
            Expr::Ident(n, _) => matches!(self.bind(n.as_str()), Some(Bind::It | Bind::Holds(_))),
            Expr::Lit(_) | Expr::Unary(..) | Expr::While(..) => false,
            Expr::Binary(Eq | Ne | Lt | Le | Gt | Ge | Sub | Mul | Div | Mod | Range, ..) => false,
            Expr::Call(f, _, _) if self.text_builtin(f) => false,
            _ => self.mentions(e),
        }
    }

    /// A call of a builtin whose result holds no markup (`TEXT_BUILTINS`), under its own name.
    fn text_builtin(&self, f: &Expr) -> bool {
        match f {
            Expr::Ident(n, _) => {
                TEXT_BUILTINS.contains(&n.as_str()) && self.bind(n.as_str()).is_none() && matches!(crate::value::lookup(self.env, n), Some(Value::Builtin(..)))
            }
            _ => false,
        }
    }

    /// The view or function a name calls, where the walk can follow it: not a local (a lambda).
    fn callee(&self, name: &str) -> Option<Arc<Closure>> {
        if self.bind(name).is_some() {
            return None;
        }
        match crate::value::lookup(self.env, name) {
            Some(Value::Func(c)) => Some(c),
            _ => None,
        }
    }

    fn bind_pattern(&mut self, p: &Pattern) {
        let mut names = Vec::new();
        crate::check::pattern_names(p, &mut names);
        for n in names {
            self.binds.push((n, Bind::Other));
        }
    }

    /// `e` evaluated, its value written at `pos` (or, at `None`, not written): where the parser
    /// is after it.
    fn placed(&mut self, e: &'a Expr, pos: Pos) -> Pos {
        if !self.tick(1) {
            return pos.map(|_| UNSURE);
        }
        match e {
            Expr::Lit(_) | Expr::Template(_) => pos,
            Expr::Ident(n, _) => match self.bind(n.as_str()) {
                Some(Bind::It) => {
                    self.record(pos);
                    pos.map(after_unseen)
                }
                Some(Bind::Holds(x)) if self.holds < HOLDS => {
                    // As where it is written now: what `x` made, made where this name is.
                    self.holds += 1;
                    let after = self.placed(x, pos);
                    self.holds -= 1;
                    after
                }
                Some(Bind::Holds(_)) => {
                    self.unsure = true;
                    pos.map(|_| UNSURE)
                }
                _ => pos.map(after_unseen),
            },
            Expr::Html(el, _) => self.el(el, pos),
            Expr::If(c, a, b) => {
                self.placed(c, None);
                let x = self.placed(a, pos);
                let y = match b {
                    Some(b) => self.placed(b, pos),
                    None => pos,
                };
                merge_pos(x, y)
            }
            Expr::Coalesce(a, b) | Expr::Binary(BinOp::And | BinOp::Or, a, b, _) => {
                let x = self.placed(a, pos);
                let y = self.placed(b, pos);
                merge_pos(x, y)
            }
            Expr::Match(s, arms, _) => {
                let binds_names = arms.iter().any(|a| {
                    let mut ns = Vec::new();
                    crate::check::pattern_names(&a.pat, &mut ns);
                    !ns.is_empty()
                });
                if binds_names && self.carries(s) {
                    self.unsure = true;
                }
                self.placed(s, None);
                let mut out: Option<Pos> = None;
                for arm in arms {
                    let n = self.binds.len();
                    self.bind_pattern(&arm.pat);
                    if let Some(g) = &arm.guard {
                        self.placed(g, None);
                    }
                    let x = self.placed(&arm.body, pos);
                    self.binds.truncate(n);
                    out = Some(match out {
                        Some(o) => merge_pos(o, x),
                        None => x,
                    });
                }
                out.unwrap_or(pos)
            }
            Expr::Block(stmts) => self.block(stmts, pos),
            Expr::For(p, it, body, _) => {
                if self.carries(it) {
                    self.unsure = true;
                }
                self.placed(it, None);
                let n = self.binds.len();
                self.bind_pattern(p);
                // A row, then the next one after it: as many as there are.
                let one = self.placed(body, pos);
                let two = if one != pos { self.placed(body, one) } else { one };
                self.binds.truncate(n);
                merge_pos(merge_pos(pos, one), two)
            }
            Expr::While(c, body, _) => {
                self.placed(c, None);
                self.placed(body, None);
                pos
            }
            Expr::List(items) => {
                let mut p = pos;
                for x in items {
                    p = self.placed(x, p);
                }
                p
            }
            Expr::Call(f, args, _) => self.call(f, args, pos),
            Expr::Lambda(_) => {
                // Called who knows where.
                if self.mentions(e) {
                    self.unsure = true;
                }
                pos
            }
            _ => {
                // `a.b`, `a[i]`, `-a`, `a < b`, `{k: v}`, `a | f()`, `x?`, `reply`: what holds a
                // followed value here is not followed further.
                if pos.is_some() && self.carries(e) {
                    self.unsure = true;
                }
                pos.map(after_unseen)
            }
        }
    }

    /// A call, its value written at `pos`.
    fn call(&mut self, f: &'a Expr, args: &'a [Expr], pos: Pos) -> Pos {
        // `children.trim()`: a method of a followed value, whose result is not followed.
        if !matches!(f, Expr::Ident(..)) && pos.is_some() && self.carries(f) {
            self.unsure = true;
        }
        let mut carrying = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if self.carries(a) {
                carrying.push(i);
            }
            self.placed(a, None);
        }
        if self.text_builtin(f) {
            return pos;
        }
        let callee = match f {
            Expr::Ident(n, _) => self.callee(n.as_str()),
            _ => None,
        };
        let Some(c) = callee.filter(|_| self.depth < DEPTH) else {
            // A builtin (`raw(x)`), a method, a lambda: its value is not followed, nor what it
            // makes of a followed value written in the page.
            if !carrying.is_empty() && pos.is_some() {
                self.unsure = true;
            }
            // Kept in a variable it is given, or passed on further: not followed either.
            if !carrying.is_empty() && !matches!(crate::value::lookup(self.env, ident_of(f)), Some(Value::Builtin(..))) {
                self.unsure = true;
            }
            return pos.map(after_unseen);
        };
        let params: Vec<&str> = carrying.iter().filter_map(|&i| c.def.params.get(i).map(|p| p.name.as_str())).collect();
        if params.len() != carrying.len() {
            self.unsure = true;
        }
        let out = Walk::run(&c, &params, pos, self.budget, self.depth + 1);
        self.unsure |= out.unsure;
        self.record(out.found);
        out.after
    }

    /// A view or component's element at `pos`: where what it holds of a followed value goes, its
    /// children (rendered as where it puts its own) and its attributes.
    fn view_el(&mut self, el: &'a HtmlEl, pos: Pos) -> Pos {
        let mut kids_carry = false;
        for ch in &el.children {
            kids_carry |= match ch {
                HtmlChild::El(e) => self.el_mentions(e),
                HtmlChild::Expr(x) => self.mentions(x),
            };
        }
        let mut attrs = Vec::new();
        for (n, a) in &el.attrs {
            match a {
                AttrVal::Expr(x) if self.carries(x) => attrs.push(n.as_str()),
                AttrVal::Spread(x) if self.carries(x) => self.unsure = true,
                _ => {}
            }
        }
        let callee = self.callee(&el.tag).filter(|_| self.depth < DEPTH);
        let Some(c) = callee else {
            if kids_carry || !attrs.is_empty() {
                self.unsure = true;
            }
            return pos.map(after_unseen);
        };
        let takes = c.def.params.iter().any(|p| p.name == "children");
        let own = Walk::run(&c, if takes { &["children"] } else { &[] }, pos, self.budget, self.depth + 1);
        if kids_carry && takes {
            let mut cur = if own.unsure { pos.map(|_| UNSURE) } else { own.found };
            for ch in &el.children {
                cur = self.child(ch, cur);
            }
        }
        if !attrs.is_empty() {
            let out = Walk::run(&c, &attrs, pos, self.budget, self.depth + 1);
            self.unsure |= out.unsure;
            self.record(out.found);
        }
        own.after
    }

    fn child(&mut self, c: &'a HtmlChild, pos: Pos) -> Pos {
        match c {
            HtmlChild::El(e) => self.el(e, pos),
            HtmlChild::Expr(x) => self.placed(x, pos),
        }
    }

    /// An element at `pos`, as the renderer writes it: its contents where `enter` says.
    fn el(&mut self, el: &'a HtmlEl, pos: Pos) -> Pos {
        if !self.tick(1) {
            return pos.map(|_| UNSURE);
        }
        if el.facts.component {
            return self.view_el(el, pos);
        }
        let inner = pos.map(|at| enter(at, el.facts.ns));
        // A script's or a style sheet's contents are text, whatever is put there.
        if !(el.tag.eq_ignore_ascii_case("script") || el.tag.eq_ignore_ascii_case("style")) {
            let mut cur = inner;
            for c in &el.children {
                cur = self.child(c, cur);
            }
            return match (pos, inner, cur) {
                (Some(at), Some(i), Some(c)) => Some(leave(at, i, c)),
                _ => None,
            };
        }
        match (pos, inner) {
            (Some(at), Some(i)) => Some(leave(at, i, i)),
            _ => None,
        }
    }

    /// A block's statements, its value written at `pos`.
    fn block(&mut self, stmts: &'a [Stmt], pos: Pos) -> Pos {
        let n = self.binds.len();
        let mut out = pos;
        for (i, s) in stmts.iter().enumerate() {
            let last = i + 1 == stmts.len();
            match s {
                Stmt::Expr(e) => {
                    let p = self.placed(e, if last { pos } else { None });
                    if last {
                        out = p;
                    }
                }
                Stmt::Return(e, _) => {
                    let a = match e {
                        Some(e) => self.placed(e, self.fn_at),
                        None => self.fn_at,
                    };
                    self.rets = Some(match self.rets {
                        Some(r) => merge_pos(r, a),
                        None => a,
                    });
                    // What follows it does not run.
                    break;
                }
                Stmt::Let(p, e, _) => self.bind_let(p, e),
                Stmt::State(name, e, _) => self.bind_let_name(name, e),
                Stmt::Assign(name, _, e, _) => {
                    if self.carries(e) || matches!(self.bind(name.as_str()), Some(Bind::It | Bind::Holds(_))) {
                        self.unsure = true;
                    }
                    self.placed(e, None);
                }
                Stmt::SetField(o, _, _, e, _) => {
                    if self.carries(e) {
                        self.unsure = true;
                    }
                    self.placed(o, None);
                    self.placed(e, None);
                }
                Stmt::Fn(d) => {
                    let mut hit = false;
                    crate::check::walk_fn(d, &mut |x| {
                        if let Expr::Ident(name, _) = x {
                            hit |= matches!(self.bind(name.as_str()), Some(Bind::It | Bind::Holds(_)));
                        }
                    });
                    if hit {
                        self.unsure = true;
                    }
                    self.binds.push((d.name.clone(), Bind::Other));
                }
                _ => {}
            }
        }
        self.binds.truncate(n);
        out
    }

    fn bind_let(&mut self, p: &'a Pattern, e: &'a Expr) {
        match p {
            Pattern::Bind(n) => self.bind_let_name(n.as_str(), e),
            _ => {
                if self.carries(e) {
                    self.unsure = true;
                }
                self.placed(e, None);
                self.bind_pattern(p);
            }
        }
    }

    fn bind_let_name(&mut self, name: &str, e: &'a Expr) {
        let holds = self.carries(e);
        // What it runs now (a function it calls may keep what it is given).
        self.placed(e, None);
        self.binds.push((name.to_string(), if holds { Bind::Holds(e) } else { Bind::Other }));
    }
}

/// The name a call is made by, or `""`.
fn ident_of(f: &Expr) -> &str {
    match f {
        Expr::Ident(n, _) => n.as_str(),
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_as_one_byte_is_the_same_place() {
        assert_eq!(encode(Some(At::default())), HTML_PLACE);
        let all = [Ns::Html, Ns::Point, Ns::MathText, Ns::Svg, Ns::Math, Ns::Annotation, Ns::Unsure];
        assert_eq!(decode(0), None);
        assert_eq!(decode(encode(None)), Some(None));
        for now in all {
            for base in all {
                for post in [None, Some(Ns::Html), Some(Ns::Unsure)] {
                    let at = At { now, base, post };
                    assert_eq!(decode(encode(Some(at))), Some(Some(at)));
                }
            }
        }
    }

    #[test]
    fn a_written_attribute_is_read_as_the_parser_reads_it() {
        assert_eq!(written_attr(r#"<annotation-xml ENCODING="Text/HTML">"#, "encoding"), Some("Text/HTML"));
        assert_eq!(written_attr(r#"<font a="x y" size>"#, "size"), Some(""));
        assert_eq!(written_attr(r#"<font sizes="1">"#, "size"), None);
        assert_eq!(written_attr("<font>", "size"), None);
        assert_eq!(kind_written(Kind::AnnotationAsk, r#"<annotation-xml encoding=" text/html">"#), Kind::Annotation);
        assert_eq!(kind_written(Kind::FontAsk, r#"<font face="x">"#), Kind::Breakout);
        assert_eq!(cdata_text("a < b && c"), None);
        assert_eq!(cdata_text("x = \"<b>]]>\";").as_deref(), Some("<![CDATA[x = \"<b>]]]]><![CDATA[>\";]]>"));
        assert_eq!(cdata_text("<![CDATA[<b>]]>&quot;").as_deref(), Some("<![CDATA[<b>]]><![CDATA[&quot;]]>"));
        assert_eq!(cdata_text("v = 1;<![CDATA[<img>").as_deref(), Some("v = 1;<![CDATA[<img>]]>"));
        assert_eq!(find_end_tag("a</b></ScRiPt>", "script"), Some(5));
    }
}
