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
        // The start tags that end foreign content (WHATWG HTML, 13.2.6.5), with `<font>` given
        // `color`, `face` or `size` (`kind_of`). A `match`, which looks at a name's length first,
        // where a list was gone through name by name for each tag (coverage of 0.8.7; branch
        // coverage-087).
        "b" | "big" | "blockquote" | "body" | "br" | "center" | "code" | "dd" | "div" | "dl" | "dt" | "em" | "embed" | "h1" | "h2" | "h3" | "h4" | "h5"
        | "h6" | "head" | "hr" | "i" | "img" | "li" | "listing" | "menu" | "meta" | "nobr" | "ol" | "p" | "pre" | "ruby" | "s" | "small" | "span"
        | "strong" | "strike" | "sub" | "sup" | "table" | "tt" | "u" | "ul" | "var" => Kind::Breakout,
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
            // A character reference in it (`text&#47;html`) is decoded by the browser, which this
            // does not do: not known (coverage of 0.8.7, Q-1117; branch coverage-087).
            Some(v) if v.contains('&') => Kind::AnnotationAsk,
            Some(v) if html_encoding(v) => Kind::AnnotationHtml,
            _ => Kind::Annotation,
        },
        Kind::FontAsk if FONT_ATTRS.iter().any(|n| written_attr(tag, n).is_some()) => Kind::Breakout,
        Kind::FontAsk => Kind::Other,
        k => k,
    }
}

/// The value of the first attribute `name` (ASCII case aside, as the parser reads names) in a
/// start tag the renderer wrote, or one in markup it did not write (`raw(...)`): `name="value"`,
/// `name='value'`, `name=value`, or `name` alone, which is `""`. Read as the browser reads it
/// (`next_attr`): an unquoted value, or spaces around `=`, had it read the same place again and
/// again, a request that never ended (`raw("<font size=1>")` inside `<svg>`; coverage of 0.8.7,
/// Q-1111; branch coverage-087).
fn written_attr<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let mut i = name_end(tag);
    while let Ok((here, value)) = next_attr(tag, &mut i) {
        if here.eq_ignore_ascii_case(name) {
            return Some(value);
        }
    }
    None
}

/// Where a start tag's name ends (`<font size=1>` at the space): past `<`, up to a space, `/` or
/// `>`, as the browser's tokenizer reads a tag name.
fn name_end(tag: &str) -> usize {
    tag.bytes().position(|c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>')).unwrap_or(tag.len())
}

/// The next attribute of a start tag from `i` (past its name, or the attribute before), read as
/// the browser's tokenizer reads it (WHATWG HTML, 13.2.5.32 to 13.2.5.40; character references
/// aside): its name and value, `i` moved past them; or, at the tag's end, whether it ended with
/// the self-closing flag. Each step reads at least one byte (coverage of 0.8.7, Q-1111; branch
/// coverage-087).
fn next_attr<'t>(tag: &'t str, i: &mut usize) -> Result<(&'t str, &'t str), bool> {
    let b = tag.as_bytes();
    // Before a name: spaces, and a `/` that `>` does not follow.
    loop {
        match b.get(*i) {
            None | Some(b'>') => return Err(false),
            Some(b'/') => {
                *i += 1;
                if b.get(*i) == Some(&b'>') {
                    return Err(true);
                }
            }
            Some(c) if c.is_ascii_whitespace() => *i += 1,
            Some(_) => break,
        }
    }
    // The name, its first byte whatever it is (a first `=` is part of it).
    let start = *i;
    *i += 1;
    while b.get(*i).is_some_and(|&c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'>' | b'=')) {
        *i += 1;
    }
    let name = &tag[start..*i];
    let mut j = *i;
    while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
        j += 1;
    }
    if b.get(j) != Some(&b'=') {
        return Ok((name, ""));
    }
    j += 1;
    while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
        j += 1;
    }
    let value = match b.get(j) {
        Some(&q @ (b'"' | b'\'')) => {
            let from = j + 1;
            let to = tag[from..].find(q as char).map_or(tag.len(), |n| from + n);
            *i = (to + 1).min(tag.len());
            &tag[from..to]
        }
        // `name=>`: no value, and `>` ends the tag.
        None | Some(b'>') => {
            *i = j;
            ""
        }
        // Unquoted: up to a space or `>`, a `/` in it.
        Some(_) => {
            let from = j;
            while b.get(j).is_some_and(|&c| !c.is_ascii_whitespace() && c != b'>') {
                j += 1;
            }
            *i = j;
            &tag[from..j]
        }
    };
    Ok((name, value))
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

/// Not known where, for the rest of the page: what `written` gives back where markup it read lost
/// the parser's place (an end tag that names no element open there, a table). The browser's
/// elements open are then not the renderer's, so leaving the renderer's own (`</svg>`) does not
/// bring the place back (`<svg><g>{raw("</svg><svg><svg>")}</g></svg>` then a script, still in
/// SVG to the browser): `leave` keeps it, and only it has `base` not known. The next request
/// starts at HTML again (`Interp::page_start`; coverage of 0.8.7, Q-1116; branch coverage-087).
pub const LOST: At = At { now: Ns::Unsure, base: Ns::Unsure, post: Some(Ns::Unsure) };

/// Where the parser is after an element: `outer` where it was opened, `inner` its contents as
/// `enter` gave them, `cur` where they ended.
#[inline]
pub fn leave(outer: At, inner: At, cur: At) -> At {
    if cur.base == Ns::Unsure {
        return LOST;
    }
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
/// found in review; branch security). Where it stops inside a tag, a comment, a CDATA section or
/// a script's text, that is ended in the html given back; and where the parser's place is not
/// followed (an end tag that names no element open here, a table), every script after is written
/// as it is safe in both, and the place after is not known (coverage of 0.8.7, Q-1116; branch
/// coverage-087).
pub fn written(at: At, html: &str) -> (At, Option<String>, Option<Ending>) {
    let b = html.as_bytes();
    // The elements open, each with where it was opened, where its contents are, and where its
    // name is in `html`, which an end tag must name (coverage of 0.8.7, Q-1116; branch
    // coverage-087).
    let mut open: Vec<(At, At, u32, u32)> = Vec::new();
    let mut cur = at;
    let mut fixed: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    // Where the parser is is no longer followed (an end tag that names no element open here, a
    // table, a `<select>`, ...): every script after is written as it is safe in both, and the
    // rest of the page is not known (`LOST`). Sticky, as `leave` can bring a place not known back
    // to one that is (coverage of 0.8.7, Q-1116; branch coverage-087).
    let mut lost = at.base == Ns::Unsure;
    // What the html needs at its end where it stops inside a tag, a comment, a CDATA section or
    // the text of a `<script>`: there the browser would read on into the renderer's next markup
    // (`raw("<input")` then `<foreignObject>`), which is no longer what the renderer takes it for
    // (coverage of 0.8.7, Q-1116; branch coverage-087).
    let mut closer = String::new();
    // A `<` that ends the html, written `&lt;`: the renderer's next text would make it a tag
    // (`raw("<")` then `hello`; coverage of 0.8.7, Q-1116; branch coverage-087).
    let mut last_lt = false;
    // What it stopped inside, the first such, for the line the server says (`Ending`).
    let mut ended: Option<Ending> = None;
    // The next `<`: most often right after the tag before.
    while let Some(n) = if b.get(i) == Some(&b'<') { Some(0) } else { memchr::memchr(b'<', b.get(i..).unwrap_or(&[])) } {
        i += n;
        // A start or end tag, the most of them, at one look; else what the tokenizer passes over.
        let end = b.get(i + 1) == Some(&b'/');
        let from = i + 1 + end as usize;
        if !b.get(from).is_some_and(|c| c.is_ascii_alphabetic()) {
            if let Some((past, close, unknown)) = passed_over(html, i, cur.now, lost) {
                i = past;
                lost |= unknown;
                if let Some(c) = close {
                    closer.push_str(c);
                    ended.get_or_insert(match c {
                        "-->" => Ending::Comment,
                        "]]>" => Ending::Cdata,
                        _ => Ending::Declaration,
                    });
                }
                continue;
            }
            if from >= b.len() {
                if end {
                    // `</` at the very end: the browser takes what follows for a bogus comment.
                    closer.push('>');
                    ended.get_or_insert(Ending::Tag);
                } else {
                    last_lt = true;
                    ended.get_or_insert(Ending::Lt);
                }
            }
            i += 1;
            continue;
        }
        let name_end = from + b[from..].iter().take_while(|c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'>')).count();
        let (tag_end, how) = tag_end(b, name_end);
        let flag = how & SELF_CLOSING != 0;
        let whole;
        let tag = if how & UNENDED != 0 {
            let c = if how & IN_DOUBLE != 0 {
                "\">"
            } else if how & IN_SINGLE != 0 {
                "'>"
            } else {
                ">"
            };
            closer.push_str(c);
            ended.get_or_insert(if c == ">" { Ending::Tag } else { Ending::Quoted(c) });
            whole = format!("{}{}", &html[i..], c);
            whole.as_str()
        } else {
            &html[i..tag_end]
        };
        i = tag_end;
        if end {
            // An end tag ends the element open that it names, the last one. In an HTML element,
            // one of no rule of its own is passed over where that element, or one below it before
            // the one it names, is of HTML's special ones (`</foreignObject>` in a `<div>`), as the
            // body's rules have it. Else, one that names another, or none open here, is followed
            // by the browser up the elements open, or ends one outside this html
            // (`raw("</foreignObject>")`), which is not followed (WHATWG HTML 13.2.6.4.7 "any other
            // end tag", 13.2.6.5; coverage of 0.8.7, Q-1116; branch coverage-087).
            if !lost {
                // The last element open, named so (written in the same case, the most often).
                let named = &b[from..name_end];
                if let Some(&(outer, inner, at, len)) = open.last() {
                    let top = &b[at as usize..(at + len) as usize];
                    if top == named || top.eq_ignore_ascii_case(named) {
                        open.pop();
                        cur = leave(outer, inner, cur);
                        continue;
                    }
                }
                match end_tag(&open, b, named) {
                    Some(0) => {}
                    Some(n) => {
                        for _ in 0..n {
                            let (outer, inner, _, _) = open.pop().expect("an element open");
                            cur = leave(outer, inner, cur);
                        }
                    }
                    None => lost = true,
                }
            }
            continue;
        }
        // Its name in lower case, on the stack: none that matters is longer.
        let mut low = [0u8; 16];
        let name = lower(&b[from..name_end], &mut low);
        let kind = match name {
            "annotation-xml" => kind_written(Kind::AnnotationAsk, tag),
            "font" => kind_written(Kind::FontAsk, tag),
            n => name_kind(n),
        };
        let inner = enter(cur, kind);
        // An HTML element (by the body's rules, after a tag that ends foreign content too).
        let html_el = inner.now == Ns::Html;
        // Tags after which the body's rules are not what this follows: tables, `<select>`,
        // `<template>`, `<frameset>`, `<plaintext>`, a `<form>` (which a page's own form makes the
        // browser pass over) (coverage of 0.8.7, Q-1116; branch coverage-087).
        if html_el || inner.now == Ns::Unsure {
            if html_lost(name) {
                lost = true;
                continue;
            }
            // An HTML start tag that closes an element open by itself (a `<p>` or a `<div>` in a
            // `<p>`, an `<li>` in an `<li>`, ...): the browser's elements open are then not these,
            // and a run of SVG opened after takes another place to go back to
            // (`<li>a<li>b</li><svg>...<p>` ended SVG where the first `<li>` was taken to be open)
            // (WHATWG HTML 13.2.6.4.7; coverage of 0.8.7, Q-1116; branch coverage-087).
            if html_el && !lost && closes_open(&open, b, name) {
                lost = true;
            }
        }
        // The self-closing flag (`<circle/>`) closes an SVG or MathML element, or one not known to
        // be HTML's; an HTML element's is passed over, so `<div/>` is open as `<div>` is, and a
        // `</foreignObject>` after it is no end of the integration point to the browser, which
        // `div` keeps it from (WHATWG HTML, 13.2.6.4.7 and 13.2.6.5). Taken as closed, the script
        // after that HTML was written for SVG (coverage of 0.8.7, Q-1100; branch coverage-087).
        // The flag is read as the tokenizer reads it: in `<svg width=10/>` the `/` is the
        // value's, and the `<svg>` stays open, where taking it closed had a script in it written
        // as HTML's (coverage of 0.8.7, Q-1112; branch coverage-087).
        let closed = inner.now != Ns::Html && flag;
        // A script's text, and in HTML the text of the elements whose contents are text, runs to
        // its end tag; an SVG or MathML `<script/>` has none. Where the parser is not followed,
        // a script's text is taken as text and the others' as markup, as is safe in both.
        let script = name == "script";
        let text = (script && (lost || !(closed && inner.now.foreign())))
            || (!lost && html_el && matches!(name, "style" | "textarea" | "title" | "xmp" | "iframe" | "noembed" | "noframes" | "noscript"));
        if text {
            // An SVG or MathML script's contents are markup to the browser: its end is past the
            // CDATA sections, comments and tags in it (`<![CDATA[ "</script>" ]]>`), where an HTML
            // one's is at the first `</script>`. Where markup in it runs past the end of the html
            // (a `<![CDATA[` in a value of a script made for HTML), that browser would never end
            // it: the first `</script>` ends it, what is open in it closed (`cdata_text`). Where it
            // is not known whether the script is SVG's or HTML's and the two differ, the text runs
            // to the later one, and each `</script` before it is written `<\/script` (the same in
            // a JavaScript string), so that both end there (coverage of 0.8.7, Q-1116; branch
            // coverage-087).
            let foreign = script && (lost || inner.script_is_foreign());
            let first = find_end_tag(&html[i..], name).map(|e| i + e);
            let found = match foreign {
                true => {
                    let (later, never) = svg_script_end(html, i, name, first);
                    if later.is_some() && later != first && (lost || !inner.now.foreign() || never) {
                        let to = later.unwrap_or(b.len());
                        let f = fixed.get_or_insert_with(String::new);
                        f.push_str(&html[copied..i]);
                        let body = escape_end_tags(&html[i..to]);
                        f.push_str(&cdata_text(&body).unwrap_or(body));
                        copied = to;
                        i = to;
                        open.push((cur, inner, from as u32, (name_end - from) as u32));
                        cur = inner;
                        continue;
                    }
                    later.or(first)
                }
                false => first,
            };
            let close = found.unwrap_or(b.len());
            if foreign {
                if let Some(t) = cdata_text(&html[i..close]) {
                    let f = fixed.get_or_insert_with(String::new);
                    f.push_str(&html[copied..i]);
                    f.push_str(&t);
                    copied = close;
                }
            } else if script && script_escapes(&html[i..close]) {
                // `<!--` then `<script>` in an HTML script: its `</script>` ends the inner one
                // only, which is not followed (WHATWG HTML 13.2.5.20 to 13.2.5.31; coverage of
                // 0.8.7, Q-1116; branch coverage-087).
                lost = true;
            }
            i = close;
            if found.is_none() {
                // Its end tag is written, else the browser takes the renderer's next markup for
                // its text.
                closer.push_str("</");
                closer.push_str(name);
                closer.push('>');
                ended.get_or_insert(Ending::Text(text_name(name)));
                continue;
            }
        } else if closed || (if html_el { html_no_push(name) } else { void_tag(name) }) {
            // No end tag follows: an element of HTML's the body makes none of or ends at once
            // (`<param>`, `<td>`, `<image>`, whatever its slash; coverage of 0.8.7, Q-1100;
            // branch coverage-087), or one written so in SVG (the renderer writes none for
            // `<input>`, in SVG too, where the parser keeps it open; it holds what follows as SVG
            // all the same).
            cur = leave(cur, inner, inner);
            continue;
        }
        open.push((cur, inner, from as u32, (name_end - from) as u32));
        cur = inner;
    }
    if last_lt || !closer.is_empty() {
        let f = fixed.get_or_insert_with(String::new);
        f.push_str(&html[copied..b.len() - last_lt as usize]);
        if last_lt {
            f.push_str("&lt;");
        }
        f.push_str(&closer);
    } else if let Some(f) = &mut fixed {
        f.push_str(&html[copied..]);
    }
    if lost {
        return (LOST, fixed, ended);
    }
    // An element left open (raw html that is not whole): what follows is in it, which is not
    // followed. An `<svg>` or `<math>` left open takes the renderer's own `</svg>` or `</math>`,
    // which then ends this one and not the renderer's, and the page goes on in SVG past the
    // renderer's run: lost to the page's end. Others the renderer's end tags close on their way,
    // or keep the browser in HTML where the renderer thinks it in SVG (coverage of 0.8.7, Q-1116;
    // branch coverage-087).
    if !open.is_empty() {
        if opens_run(&open, b) {
            return (LOST, fixed, ended);
        }
        cur = unsure(cur);
    }
    (cur, fixed, ended)
}

/// Whether an `<svg>` or a `<math>` that starts a run of SVG or MathML is among the elements open.
#[cold]
#[inline(never)]
fn opens_run(open: &[(At, At, u32, u32)], b: &[u8]) -> bool {
    open.iter().any(|&(_, inner, at, len)| matches!(inner.now, Ns::Svg | Ns::Math) && matches!(&b[at as usize..(at + len) as usize], [b's' | b'S', b'v' | b'V', b'g' | b'G'] | [b'm' | b'M', b'a' | b'A', b't' | b'T', b'h' | b'H']))
}

/// What raw markup stopped inside, whose end `written` wrote so that the browser does not read on
/// into the renderer's next markup: the server says so once for each line of the program that
/// wrote it (`Interp::ending_written`; coverage of 0.8.7, Q-1116; the user, 2026-11-19: the
/// endings written, a line in the log the first time).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    /// `<!--` with no `-->`.
    Comment,
    /// `<!x`, `<!DOCTYPE`, `<?x` or `</ x` with no `>`.
    Declaration,
    /// `<![CDATA[` with no `]]>`.
    Cdata,
    /// A start or end tag with no `>`.
    Tag,
    /// A tag stopped inside a quoted attribute value: the quote and `>` that end it.
    Quoted(&'static str),
    /// A `<script>`'s (or a `<textarea>`'s, ...) text with no end tag.
    Text(&'static str),
    /// A `<` at the very end.
    Lt,
}

impl Ending {
    /// The line the server says, after the program's file and line.
    pub fn words(self) -> String {
        let (inside, end, whole) = match self {
            Ending::Comment => ("a comment", "its end (-->)".to_string(), "the whole comment"),
            Ending::Declaration => ("a <!...> or <?...>", "its end (>)".to_string(), "the whole declaration"),
            Ending::Cdata => ("a CDATA section", "its end (]]>)".to_string(), "the whole section"),
            Ending::Tag => ("a tag", "its end (>)".to_string(), "the whole tag"),
            Ending::Quoted(c) => ("a quoted attribute value", format!("its end ({c})"), "the whole tag"),
            Ending::Text(t) => {
                return format!(
                    "raw markup inside <svg> or <math> stops inside a <{t}>'s text; Quillo wrote its end tag (</{t}>) so the page stays safe. Pass the whole <{t}> in one raw()."
                )
            }
            Ending::Lt => {
                return "raw markup inside <svg> or <math> ends with a lone <; Quillo wrote it as &lt; so the page stays safe. Write &lt; for a < that starts no tag.".to_string()
            }
        };
        format!("raw markup inside <svg> or <math> stops inside {inside}; Quillo wrote {end} so the page stays safe. Pass {whole} in one raw().")
    }
}

/// A text element's name, kept: those `written` reads to their end tag.
fn text_name(n: &str) -> &'static str {
    match n {
        "style" => "style",
        "textarea" => "textarea",
        "title" => "title",
        "xmp" => "xmp",
        "iframe" => "iframe",
        "noembed" => "noembed",
        "noframes" => "noframes",
        "noscript" => "noscript",
        _ => "script",
    }
}

/// `name` in lower case in `buf`, where it fits (none that matters is longer), else `""`.
#[inline]
fn lower<'b>(name: &[u8], buf: &'b mut [u8; 16]) -> &'b str {
    if name.len() > buf.len() {
        return "";
    }
    let buf = &mut buf[..name.len()];
    buf.copy_from_slice(name);
    buf.make_ascii_lowercase();
    std::str::from_utf8(buf).unwrap_or("")
}

/// What an end tag (its name `name` as written in `b`, `low` in lower case) does to the elements
/// open here, as the browser's tree builder does it: how many it closes, the last ones (`Some(0)`:
/// it is passed over), or `None` where that is not followed. The last one named so is closed; in
/// an HTML element, an end tag of no rule of its own goes down the elements open until one named
/// so (closed with those above it) or one of the special ones (passed over), as the body's "any
/// other end tag" does; one that may have been closed by a start tag (`<p>` in a `<p>`, an `<li>`
/// in an `<li>`) is not followed, as the browser's elements open are then not these (coverage of
/// 0.8.7, Q-1116; branch coverage-087).
#[cold]
#[inline(never)]
fn end_tag(open: &[(At, At, u32, u32)], b: &[u8], name: &[u8]) -> Option<usize> {
    let &(_, top_in, at, len) = open.last()?;
    let (at, len) = (at as usize, len as usize);
    if b[at..at + len].eq_ignore_ascii_case(name) {
        return Some(1);
    }
    let mut buf = [0u8; 16];
    if top_in.now != Ns::Html || html_end_rule(lower(name, &mut buf)) {
        return None;
    }
    for (k, &(_, inner, at, len)) in open.iter().enumerate().rev() {
        let (at, len) = (at as usize, len as usize);
        let mut buf = [0u8; 16];
        let n = lower(&b[at..at + len], &mut buf);
        let html = inner.now == Ns::Html;
        if html && implied_closes(n) {
            return None;
        }
        if html && b[at..at + len].eq_ignore_ascii_case(name) {
            return Some(open.len() - k);
        }
        let special = if html {
            html_special(n)
        } else {
            matches!(inner.now, Ns::Point | Ns::MathText) || (n == "annotation-xml" && inner.now == Ns::Annotation)
        };
        // An SVG or MathML integration point of that name (`</mi>` in an `<mi>` with a `<span>`
        // open in it): passed over by WHATWG HTML, Blink and Gecko, closed by parse5, which
        // matches names across languages; not followed, so that either way is safe (coverage of
        // 0.8.7, Q-1116; branch coverage-087).
        if special && !html && b[at..at + len].eq_ignore_ascii_case(name) {
            return None;
        }
        if special {
            return Some(0);
        }
        if inner.now == Ns::Unsure {
            return None;
        }
    }
    None
}

/// Whether the HTML start tag `name` closes by itself an element open here, as the body's rules
/// have it (WHATWG HTML 13.2.6.4.7): a `<p>` in button scope for the tags that close one, an
/// `<li>` for an `<li>`, a `<dd>` or `<dt>` for either, a heading for a heading just open, a
/// `<button>` for a button, `<a>` and `<nobr>` for their own, an `<option>` just open for an
/// `<option>` or `<optgroup>`, ruby's parts for theirs. Looked for among the HTML elements open
/// since the last SVG or MathML element (past an SVG run a tag that ends it pops first), one
/// whose scope may hold it found is enough (coverage of 0.8.7, Q-1116; branch coverage-087).
#[cold]
#[inline(never)]
fn closes_open(open: &[(At, At, u32, u32)], b: &[u8], name: &str) -> bool {
    let closes_p = matches!(
        name,
        "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure"
            | "footer" | "header" | "hgroup" | "main" | "menu" | "nav" | "ol" | "p" | "search" | "section" | "summary" | "ul" | "h1" | "h2" | "h3"
            | "h4" | "h5" | "h6" | "pre" | "listing" | "hr" | "xmp" | "li" | "dd" | "dt"
    );
    let own = |n: &str| match name {
        "li" => n == "li",
        "dd" | "dt" => matches!(n, "dd" | "dt"),
        "button" | "a" | "nobr" => n == name,
        "rb" | "rtc" | "rp" | "rt" => matches!(n, "rb" | "rtc" | "rp" | "rt"),
        _ => false,
    };
    let heading = |n: &str| matches!(n, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
    if !closes_p && !matches!(name, "li" | "dd" | "dt" | "button" | "a" | "nobr" | "rb" | "rtc" | "rp" | "rt" | "option" | "optgroup") {
        return false;
    }
    let mut top = true;
    for &(_, inner, at, len) in open.iter().rev() {
        match inner.now {
            // Popped by the browser before a tag that ends SVG or MathML is taken.
            Ns::Svg | Ns::Math | Ns::Annotation if top => continue,
            Ns::Html => {}
            _ => return false,
        }
        let mut buf = [0u8; 16];
        let n = lower(&b[at as usize..(at + len) as usize], &mut buf);
        if (closes_p && n == "p") || own(n) || (top && heading(name) && heading(n)) || (top && matches!(name, "option" | "optgroup") && n == "option") {
            return true;
        }
        top = false;
    }
    false
}

/// End tags with a rule of their own in a page's body (WHATWG HTML, 13.2.6.4.7).
fn html_end_rule(n: &str) -> bool {
    matches!(
        n,
        "a" | "address" | "applet" | "article" | "aside" | "b" | "big" | "blockquote" | "body" | "br" | "button" | "center" | "code" | "dd" | "details"
            | "dialog" | "dir" | "div" | "dl" | "dt" | "em" | "fieldset" | "figcaption" | "figure" | "font" | "footer" | "form" | "h1" | "h2" | "h3"
            | "h4" | "h5" | "h6" | "header" | "hgroup" | "html" | "i" | "li" | "listing" | "main" | "marquee" | "menu" | "nav" | "nobr" | "object"
            | "ol" | "p" | "pre" | "s" | "search" | "section" | "small" | "strike" | "strong" | "summary" | "template" | "tt" | "u" | "ul"
    )
}

/// HTML's special elements that an element can be left open in here (WHATWG HTML, 13.2.4.3).
fn html_special(n: &str) -> bool {
    matches!(
        n,
        "address" | "applet" | "article" | "aside" | "blockquote" | "button" | "center" | "dd" | "details" | "dir" | "div" | "dl" | "dt" | "fieldset"
            | "figcaption" | "figure" | "footer" | "form" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "hgroup" | "li" | "listing" | "main"
            | "marquee" | "menu" | "nav" | "object" | "ol" | "p" | "pre" | "search" | "section" | "summary" | "ul"
    )
}

/// HTML elements that a later start tag can close by itself (a `<p>` the next `<p>` or `<div>`
/// ends, an `<li>` the next `<li>`, ...; WHATWG HTML 13.2.6.4.7).
fn implied_closes(n: &str) -> bool {
    matches!(
        n,
        "p" | "li" | "dd" | "dt" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "button" | "a" | "nobr" | "form" | "option" | "optgroup" | "rb" | "rp" | "rt"
            | "rtc"
    )
}

/// HTML's void elements (`crate::parser::VOID_TAGS`), whose end tag the renderer never writes.
#[inline]
fn void_tag(n: &str) -> bool {
    matches!(n, "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr")
}

/// HTML's start tags that leave no element open in a page's body: void elements, those it treats
/// as void (`<param>`, `<keygen>`, `<image>`, an `<img>`, `<basefont>`, `<bgsound>`), and those it
/// passes over (`<frame>`, `<head>`, `<html>`, `<body>`), whatever their slash (WHATWG HTML,
/// 13.2.6.4.7; coverage of 0.8.7, Q-1100; branch coverage-087).
fn html_no_push(n: &str) -> bool {
    matches!(
        n,
        "area" | "base" | "basefont" | "bgsound" | "body" | "br" | "col" | "embed" | "frame" | "head" | "hr" | "html" | "image" | "img" | "input"
            | "keygen" | "link" | "meta" | "param" | "source" | "track" | "wbr"
    )
}

/// HTML's start tags after which this does not follow the parser: a table and its parts (which,
/// in a table's cell, end elements outside the integration point), `<select>`, `<template>`,
/// `<frameset>`, `<plaintext>`, `<form>` (coverage of 0.8.7, Q-1116; branch coverage-087).
fn html_lost(n: &str) -> bool {
    matches!(
        n,
        "caption" | "colgroup" | "form" | "frameset" | "plaintext" | "select" | "table" | "tbody" | "td" | "template" | "tfoot" | "th" | "thead" | "tr"
    )
}

/// Whether an HTML script's text has `<!--` and then `<script` and a space, `/` or `>`: the
/// tokenizer's double-escaped state, where `</script>` does not end the script.
fn script_escapes(t: &str) -> bool {
    let Some(k) = t.find("<!--") else { return false };
    let b = t[k..].as_bytes();
    b.windows(8).any(|w| w[..7].eq_ignore_ascii_case(b"<script") && (w[7].is_ascii_whitespace() || matches!(w[7], b'/' | b'>')))
}

/// Where an SVG or MathML script whose contents start at `i` ends, at its `</script>`, its contents
/// read as the markup they are there: CDATA sections, comments, bogus comments and other tags (an
/// attribute's `</script>`) passed over; `None` where it does not end in `html` (coverage of
/// 0.8.7, Q-1116; branch coverage-087).
fn foreign_end(html: &str, mut i: usize) -> Option<usize> {
    let b = html.as_bytes();
    while let Some(n) = b[i..].iter().position(|&c| c == b'<') {
        i += n;
        if let Some((past, close, _)) = passed_over(html, i, Ns::Svg, false) {
            if close.is_some() {
                return None;
            }
            i = past;
            continue;
        }
        let end = b.get(i + 1) == Some(&b'/');
        let from = i + 1 + end as usize;
        if !b.get(from).is_some_and(|c| c.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let name_end = from + b[from..].iter().take_while(|c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'>')).count();
        if end && b[from..name_end].eq_ignore_ascii_case(b"script") {
            return Some(i);
        }
        match tag_end(b, name_end) {
            (_, how) if how & UNENDED != 0 => return None,
            (past, _) => i = past,
        }
    }
    None
}

/// Where an SVG or MathML script whose contents start at `i` ends (`foreign_end`), and whether
/// that reading never ends it. A CDATA section or a comment it opens and never closes: SVG never
/// ends it, and the first `</script>`, inside that section, is no end of the script as its markup
/// was written (`<![CDATA[ "</script><img ...>";</script>`). Where a later `</script>` follows,
/// that one ends it, and those before are written `<\/script`; else the first (`first`) ends it,
/// as a script made for HTML ends (coverage of 0.8.7, Q-1116; branch coverage-087).
#[cold]
#[inline(never)]
fn svg_script_end(html: &str, i: usize, name: &str, first: Option<usize>) -> (Option<usize>, bool) {
    let later = foreign_end(html, i);
    if later.is_some() {
        return (later, false);
    }
    let Some(f) = first else { return (None, true) };
    let mut end = f;
    while open_construct(&html[i..end]) {
        match find_end_tag(&html[end + 2..], name) {
            Some(k) => end = end + 2 + k,
            None => break,
        }
    }
    ((end != f).then_some(end), true)
}

/// Whether `t` ends inside a CDATA section or a comment it opens.
#[cold]
#[inline(never)]
fn open_construct(t: &str) -> bool {
    let mut rest = t;
    loop {
        match (rest.find("<![CDATA["), rest.find("<!--")) {
            (Some(k), c) if c.is_none_or(|c| k < c) => match rest[k + 9..].find("]]>") {
                Some(e) => rest = &rest[k + 9 + e + 3..],
                None => return true,
            },
            (_, Some(c)) => match comment_end(rest, c + 4) {
                Some(e) => rest = &rest[e..],
                None => return true,
            },
            _ => return false,
        }
    }
}

/// `t` with each `</script` written `<\/script`, ASCII case aside.
fn escape_end_tags(t: &str) -> String {
    let mut out = String::with_capacity(t.len() + 8);
    let mut rest = t;
    while let Some(k) = rest.find("</") {
        out.push_str(&rest[..k]);
        let after = &rest[k + 2..];
        if after.len() >= 6 && after.as_bytes()[..6].eq_ignore_ascii_case(b"script") {
            out.push_str("<\\/");
        } else {
            out.push_str("</");
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// What the tokenizer passes over as no tag at `i` (a `<`), where the parser's current element is
/// `now`: past its end, what the html needs at its end where it runs to the end, and whether the
/// parser's place is no longer known after it; `None` for a tag, or a `<` that is text. A comment
/// ends at the first `-->` or `--!>`, or at once for `<!-->` and `<!--->`; `<!x>`,
/// `<!DOCTYPE ...>`, `<?x>` and `</ x>` (bogus comments) at the first `>`; `</>` is dropped.
/// `<![CDATA[` is a CDATA section, to `]]>`, in foreign content (an SVG or MathML element that is
/// no integration point), and a bogus comment in an HTML element; at an integration point (a
/// `<foreignObject>`, an `<mi>`) browsers read the bogus comment where WHATWG HTML reads a
/// section, so the bogus comment is read and, where the two differ by markup, the place is no
/// longer known (coverage of 0.8.7, Q-1113; branch coverage-087). Read otherwise, markup the
/// browser takes was passed over (`<!--><svg>` was a comment to the end) or markup it does not
/// take was read (an end tag in `<!x </svg>>`), and a script then in SVG was written as HTML's
/// (WHATWG HTML, 13.2.5.6 to 13.2.5.8, 13.2.5.41 to 13.2.5.52, 13.2.5.69; coverage of 0.8.7,
/// Q-1113; branch coverage-087).
#[cold]
#[inline(never)]
fn passed_over(html: &str, i: usize, now: Ns, lost: bool) -> Option<(usize, Option<&'static str>, bool)> {
    let rest = &html.as_bytes()[i..];
    match rest.get(1) {
        Some(b'!' | b'?') => {}
        Some(b'/') if rest.get(2).is_some_and(|c| !c.is_ascii_alphabetic()) => {}
        _ => return None,
    }
    let to_gt = |from: usize| html[from..].find('>').map_or((html.len(), Some(">"), false), |k| (from + k + 1, None, false));
    if rest.starts_with(b"<!--") {
        return Some(comment_end(html, i + 4).map_or((html.len(), Some("-->"), false), |e| (e, None, false)));
    }
    if rest.starts_with(b"<![CDATA[") {
        let section = html[i + 9..].find("]]>").map(|k| i + 9 + k + 3);
        if now.foreign() && !lost {
            return Some(section.map_or((html.len(), Some("]]>"), false), |e| (e, None, false)));
        }
        let (past, close, _) = to_gt(i + 9);
        if now == Ns::Html {
            return Some((past, close, false));
        }
        // Read either way: a `<` between the two ends is markup to one reading only, and a
        // section that runs to the end is closed, which ends either.
        let far = section.unwrap_or(html.len());
        let differs = past < far && html[past..far].contains('<');
        return Some((past, if section.is_none() { Some("]]>") } else { close }, differs));
    }
    match rest.get(1) {
        Some(b'/') if rest.get(2) == Some(&b'>') => Some((i + 3, None, false)),
        _ => Some(to_gt(i + 2)),
    }
}

/// Where a comment whose text starts at `from` (past `<!--`) ends, past it: at once for `<!-->` and
/// `<!--->`, else past the first `-->` or `--!>` (WHATWG HTML 13.2.5.43 to 13.2.5.52).
fn comment_end(html: &str, from: usize) -> Option<usize> {
    let after = &html[from..];
    if after.starts_with('>') {
        return Some(from + 1);
    }
    if after.starts_with("->") {
        return Some(from + 2);
    }
    match (after.find("-->"), after.find("--!>")) {
        (Some(a), Some(b)) if b < a => Some(from + b + 4),
        (Some(a), _) => Some(from + a + 3),
        (None, Some(b)) => Some(from + b + 4),
        (None, None) => None,
    }
}

/// Where a tag whose name ends at `from` ends, past its `>`, a quoted value's `>` not counted, as
/// the browser's tokenizer reads it (13.2.5.32 to 13.2.5.40): a quote is one only where a value
/// starts. Taken as one anywhere (`<g a=x'y><svg>'>`), the `<svg>` the browser opens was read as
/// part of the tag, and a script in it as HTML's (coverage of 0.8.7, Q-1112; branch coverage-087).
/// Where the html ends first, that it does (`UNENDED`) and in which quote (coverage of 0.8.7,
/// Q-1116; branch coverage-087). And whether it carries the self-closing flag, read in the same
/// pass: `<g/>`, `<g a="1"/>`, `<g a />`, not `<g a=1/>`, whose `/` is its value's, nor `<g / >`
/// (coverage of 0.8.7, Q-1112; branch coverage-087).
#[inline]
fn tag_end(b: &[u8], from: usize) -> (usize, u8) {
    let n = b.len();
    let mut k = from;
    // A `/` the tokenizer takes for the self-closing flag's was the byte before.
    let mut slash = 0;
    loop {
        // Before a name: spaces, a `/`, or the end.
        while k < n && b[k].is_ascii_whitespace() {
            k += 1;
            slash = 0;
        }
        if k >= n {
            return (n, UNENDED | slash);
        }
        match b[k] {
            b'>' => return (k + 1, slash),
            b'/' => {
                slash = SELF_CLOSING;
                k += 1;
                continue;
            }
            _ => slash = 0,
        }
        // A name, its first byte whatever it is.
        k += 1;
        while k < n && !matches!(b[k], b'/' | b'>' | b'=') && !b[k].is_ascii_whitespace() {
            k += 1;
        }
        // Its value, if a `=` follows.
        let mut j = k;
        while j < n && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= n || b[j] != b'=' {
            continue;
        }
        j += 1;
        while j < n && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= n {
            return (n, UNENDED);
        }
        match b[j] {
            q @ (b'"' | b'\'') => match b[j + 1..].iter().position(|&c| c == q) {
                Some(m) => k = j + 2 + m,
                None => return (n, UNENDED | if q == b'"' { IN_DOUBLE } else { IN_SINGLE }),
            },
            b'>' => return (j + 1, 0),
            _ => {
                k = j;
                while k < n && b[k] != b'>' && !b[k].is_ascii_whitespace() {
                    k += 1;
                }
            }
        }
    }
}

/// What `tag_end` says of a tag besides where it ends: it carries the self-closing flag; the html
/// ends before its `>`, in a value opened with `"` or `'`.
const SELF_CLOSING: u8 = 1;
const UNENDED: u8 = 2;
const IN_DOUBLE: u8 = 4;
const IN_SINGLE: u8 = 8;

/// Where the first `</name` (`name` in lower case) that ends the element is in `hay`, ASCII case
/// aside: followed by a space, `/` or `>`, as the tokenizer takes an end tag in a script's or a
/// text's (`</scriptx>` ends nothing; coverage of 0.8.7, Q-1116; branch coverage-087).
fn find_end_tag(hay: &str, name: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), name.as_bytes());
    let mut k = 0;
    while let Some(at) = h[k..].windows(2).position(|w| w == b"</") {
        let from = k + at + 2;
        if h.get(from..from + n.len()).is_some_and(|x| x.eq_ignore_ascii_case(n)) && h.get(from + n.len()).is_some_and(|&c| c.is_ascii_whitespace() || matches!(c, b'/' | b'>')) {
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
