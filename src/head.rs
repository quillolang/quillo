//! `page_head({ title, description, image, canonical })`: what a page's code says goes in the
//! document's head (FEEDBACK84.md, Q-1320, Q-1321; FEEDBACK81.md, Q-1275; branch feedback82-85).
//! Decided (user, 2026-11-22): a builtin, Nuxt's way, not a `<head>` block.
//!
//! On the server each call while a page is made is kept here, by key; once the page is made,
//! the document's head gets the winners: for each key its tags in the head are taken away and
//! the call's written where the browser ends the head (`scan`). Which call wins a key: the innermost. A call in a view's
//! or component's body is inside what called it, and one in what is given to a view as its
//! `children` is inside that view; a view or component that takes `children` (a layout) is
//! outside what calls it, so a page that wraps itself in a layout keeps its own title. Of two at
//! the same depth, the one inside more components, as the browser has it; then the later. Null says nothing, which leaves the key to an outer call or the
//! document. The browser's side is `B.page_head` in src/client/runtime.js.

use crate::interp::escape;

/// The keys `page_head` takes, in the order their tags are written.
pub const KEYS: [&str; 4] = ["title", "description", "image", "canonical"];

/// What the page's calls said, by key (`KEYS`' order): the value and its rank (depth, the
/// components around the call, the call).
#[derive(Default, Debug, Clone)]
pub struct Head {
    said: [Option<(i32, usize, u32, String)>; 4],
    calls: u32,
}

impl Head {
    /// A call's `key` at `depth` inside `comps` components: kept where it ranks as high as the one
    /// kept or higher, so of two the same the later (review of slice H: the browser's rule).
    pub fn say(&mut self, key: usize, depth: i32, comps: usize, value: String) {
        let call = self.calls;
        let slot = &mut self.said[key];
        if slot.as_ref().is_none_or(|(d, n, _, _)| (depth, comps) >= (*d, *n)) {
            *slot = Some((depth, comps, call, value));
        }
    }

    /// One call more, for the order of those at one depth.
    pub fn called(&mut self) {
        self.calls += 1;
    }

    /// Whether this says a key otherwise than `sent`, the head the page went out with.
    pub fn differs(&self, sent: Option<&Head>) -> bool {
        KEYS.iter().any(|k| self.get(k).is_some_and(|v| sent.and_then(|s| s.get(k)) != Some(v)))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        let i = KEYS.iter().position(|k| *k == key)?;
        self.said[i].as_ref().map(|(_, _, _, v)| v.as_str())
    }

    /// The tags the winners make, each key's in one place.
    fn tags(&self) -> String {
        let mut out = String::new();
        let attr = |s: &str| escape(s);
        if let Some(t) = self.get("title") {
            out.push_str(&format!("<title>{}</title><meta property=\"og:title\" content=\"{}\">", escape(t), attr(t)));
        }
        if let Some(d) = self.get("description") {
            out.push_str(&format!("<meta name=\"description\" content=\"{}\"><meta property=\"og:description\" content=\"{}\">", attr(d), attr(d)));
        }
        if let Some(i) = self.get("image") {
            out.push_str(&format!("<meta property=\"og:image\" content=\"{}\">", attr(i)));
        }
        if let Some(c) = self.get("canonical") {
            out.push_str(&format!("<link rel=\"canonical\" href=\"{}\"><meta property=\"og:url\" content=\"{}\">", attr(c), attr(c)));
        }
        out
    }

    /// Whether `tag` (one tag's text, `<meta ...>`, its name `name` as written) is one of a key
    /// this head says.
    fn replaces(&self, name: &[u8], tag: &[u8]) -> bool {
        let said = |k: &str| self.get(k).is_some();
        if name.eq_ignore_ascii_case(b"title") {
            said("title")
        } else if name.eq_ignore_ascii_case(b"meta") {
            // Compared as written, in any case, with nothing allocated (FEEDBACK84.md, Q-1320,
            // found by the review of 1a4bb9b; branch feedback82-85: each meta of a head was a
            // String or two).
            let Some(n) = attr_of(tag, b"name").or_else(|| attr_of(tag, b"property")) else { return false };
            let is = |w: &[u8]| n.eq_ignore_ascii_case(w);
            if is(b"og:title") {
                said("title")
            } else if is(b"description") || is(b"og:description") {
                said("description")
            } else if is(b"og:image") {
                said("image")
            } else if is(b"og:url") {
                said("canonical")
            } else {
                false
            }
        } else if name.eq_ignore_ascii_case(b"link") {
            said("canonical") && attr_of(tag, b"rel").is_some_and(|r| r.split(|&c| is_space(c)).any(|w| w.eq_ignore_ascii_case(b"canonical")))
        } else {
            false
        }
    }
}

/// The value of attribute `name` in one tag's text, its quotes taken off (entities left as they
/// are: the names compared are plain words); of two of one name, the first, as the browser keeps.
fn attr_of<'a>(tag: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    // Past the tag's name.
    let mut i = 1;
    while i < tag.len() && !is_space(tag[i]) && tag[i] != b'>' && tag[i] != b'/' {
        i += 1;
    }
    while i < tag.len() {
        while i < tag.len() && (is_space(tag[i]) || tag[i] == b'/') {
            i += 1;
        }
        if i >= tag.len() || tag[i] == b'>' {
            return None;
        }
        let from = i;
        while i < tag.len() && !is_space(tag[i]) && !matches!(tag[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let key = &tag[from..i];
        while i < tag.len() && is_space(tag[i]) {
            i += 1;
        }
        let mut value: &[u8] = b"";
        if tag.get(i) == Some(&b'=') {
            i += 1;
            while i < tag.len() && is_space(tag[i]) {
                i += 1;
            }
            match tag.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let end = tag[i + 1..].iter().position(|&c| c == q).map_or(tag.len(), |e| i + 1 + e);
                    value = &tag[i + 1..end];
                    i = end + 1;
                }
                _ => {
                    let from = i;
                    while i < tag.len() && !is_space(tag[i]) && tag[i] != b'>' {
                        i += 1;
                    }
                    value = &tag[from..i];
                }
            }
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(value);
        }
    }
    None
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// The page `body` with `head`'s tags in its head, in place of those it had of the same keys. A
/// page that is no document (no doctype, `<html>`, `<head>`, `<body>`, `<frameset>` or `</head>`: a
/// part of one, as an `hx-get` asks for) is left as it is.
pub fn apply(body: &mut Vec<u8>, head: &Head) {
    let tags = head.tags();
    if tags.is_empty() {
        return;
    }
    // Read once, as the browser reads it (`scan`): where the head ends, and the head's own tags of
    // the keys said, to be taken away: a `<title>` with its text and end tag, a `<meta>` or
    // `<link>` whole.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let Some((end, first)) = scan(body, true, |lt, name, close, end| {
        if head.replaces(name, &body[lt..close]) {
            cuts.push((lt, if name.eq_ignore_ascii_case(b"title") { end } else { close }));
        }
    }) else {
        return;
    };
    // A head the page leaves out the browser starts at its first element: where that is cut, the
    // tags take its place, so what follows it (spaces, a comment, a doctype, which the browser
    // passes over in a head) stays in the head (FEEDBACK84.md, Q-1320, found by the review of
    // 1a4bb9b; branch feedback82-85).
    let at = first.filter(|&p| cuts.first().is_some_and(|c| c.0 == p)).unwrap_or(end);
    // The page made again in one pass, what is kept copied once and the tags put in at `at`, which
    // no cut holds (FEEDBACK84.md, Q-1320, found by the review of 1a4bb9b; branch feedback82-85:
    // each cut was a drain of the page's rest, a head of many of them quadratic).
    let mut out = Vec::with_capacity(body.len() + tags.len());
    let mut kept = 0;
    let mut put = false;
    for &(from, to) in &cuts {
        if !put && at <= from {
            out.extend_from_slice(&body[kept..at]);
            out.extend_from_slice(tags.as_bytes());
            kept = at;
            put = true;
        }
        out.extend_from_slice(&body[kept..from]);
        kept = to;
    }
    if !put {
        out.extend_from_slice(&body[kept..at]);
        out.extend_from_slice(tags.as_bytes());
        kept = at;
    }
    out.extend_from_slice(&body[kept..]);
    *body = out;
}

/// Where `page_head`'s tags go in the page: where the browser's tree builder ends its head
/// (FEEDBACK84.md, Q-1320, found by the review of 1a4bb9b; branch feedback82-85: all up to
/// `</head>` was taken as the head, so a title of an `<svg>` there, or of a template's content, was
/// cut, and the tags went to the body when text or an svg had ended the head before). Read as the
/// browser reads it: by the HTML standard's tokenizer (a tag to the `>` that ends it, quotes and
/// all; a comment to its `-->` or `--!>`; a title's text and a style's and the other raw text
/// elements' to their end tag; a script's by its script data states, so `<!-- <script>` in one
/// holds its first `</script>`), and by the tree builder's modes up to the body (before the head,
/// in it, after it). The head ends at the first thing in it that is not of a head: text that is
/// not spaces, an element that is not a title, meta, link, base, script, style, noscript,
/// noframes or template, or `</body>`, `</html>` or `</br>`; or at `</head>`, after which a title,
/// meta, link, script, style or template still goes in the head, up to anything else. A
/// template's content is passed over whole, to the `</template>` that closes it (`walk`). The tags
/// go before what ends the head, or before a tag, comment or element that runs on to the page's
/// end (which the browser drops, or takes whole; a title that does is the head's, cut to the end),
/// or before the head's first `<noscript>`, where both readers have the head still open: a
/// noscript is text to a browser with scripting on, but a crawler (a search engine's, a social
/// card's) reads with it off, and there what it holds is tags, a pixel's `<img>` ends the head
/// (FEEDBACK84.md, Q-1320, found by the review of a6988b4; branch feedback82-85). Nothing after
/// what ends the head is cut. With it, where a head the page leaves out (no `<head>`) starts: at
/// its first element, a title, meta, script, .... None for a page that is no document: no doctype,
/// `<html>`, `<head>`, `<body>`, `<frameset>` or `</head>` in it, outside a template.
///
/// No full parser: the head is read as the browser reads it, a template's content and the rest
/// (for whether the page is a document) by `walk`, which follows foreign content by name, not the
/// body's implied end tags, nor the older `<select>` rules (which drop a `<title>` or `<svg>` in a
/// select); and the page is read as UTF-8, with no `<meta charset>` reread. A known limit, not
/// fixed (found by the review of a6988b4): `walk` follows 64 open elements, without the browser's
/// implied end tags past the commonest or its adoption agency, so in a template many elements left
/// open (a run of `<button>`s, which the browser ends one by the next, or of `<b>`s) before an
/// `<svg>` leave the svg not followed, and a `<![CDATA[` in it that holds `</template>` closes the
/// template there, early.
///
/// `each` is told of each element of the head's own (not a template's), as (where it starts, its
/// name as written, just past its start tag, just past its element: its end tag's `>`, its own
/// for a meta, link or base, the `</template>`'s for a template, the page's end for one that runs
/// to it); and, with `noscripts`, of the titles, metas and links in a noscript of the head as
/// scripting off reads them (`hidden`), none of them a script (`head_scripts` has no use for them).
fn scan(body: &[u8], noscripts: bool, mut each: impl FnMut(usize, &[u8], usize, usize)) -> Option<(usize, Option<usize>)> {
    #[derive(PartialEq, Clone, Copy)]
    enum Mode {
        Before,
        In,
        After,
    }
    let mut mode = Mode::Before;
    let mut doc = false;
    let mut head_end = None;
    let mut implied = None;
    let mut noscript = None;
    // A byte order mark is the decoder's, no text.
    let mut i = if body.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    let stop = loop {
        let lt = memchr::memchr(b'<', &body[i..]).map_or(body.len(), |p| i + p);
        if let Some(t) = not_space(body, i, lt) {
            break t;
        }
        if lt == body.len() {
            break lt;
        }
        // A tag, comment or doctype that never ends.
        let Some((tok, mut next)) = token(body, lt) else { break lt };
        match tok {
            // A `<` that starts nothing is text.
            Tok::Text => break lt,
            Tok::Skip { doctype } => doc |= doctype,
            Tok::End(name) => {
                if name.eq_ignore_ascii_case(b"head") {
                    doc = true;
                    if mode != Mode::After {
                        head_end = Some(lt);
                        mode = Mode::After;
                    }
                } else if is_one(name, &[b"body", b"html", b"br"]) {
                    break lt;
                }
                // Any other end tag the tree builder passes over.
            }
            Tok::Start(name, _) => {
                if name.eq_ignore_ascii_case(b"html") {
                    doc = true;
                } else if name.eq_ignore_ascii_case(b"head") {
                    doc = true;
                    if mode == Mode::Before {
                        mode = Mode::In;
                    }
                } else if let Some(kind) = head_kind(name).filter(|_| mode != Mode::After || !name.eq_ignore_ascii_case(b"noscript")) {
                    if mode == Mode::Before {
                        mode = Mode::In;
                        implied = Some(lt);
                    }
                    let end = match kind {
                        Kind::Void => Some(next),
                        // A noscript's text is tags to a browser with scripting off: its titles,
                        // metas and links are told (`hidden`), and the tags go before the first
                        // (FEEDBACK84.md, Q-1320, found by the review of a6988b4; branch
                        // feedback82-85: they went after it, where a crawler, which reads with
                        // scripting off, has ended the head at a pixel's `<img>` in it).
                        Kind::Text if name.eq_ignore_ascii_case(b"noscript") => {
                            let to = text_end(body, next, name);
                            noscript = noscript.or(Some(lt));
                            if noscripts {
                                hidden(body, next, to.unwrap_or(body.len()), &mut each);
                            }
                            to.and_then(|e| tag_close(body, e)).map(|(c, _)| c)
                        }
                        Kind::Text => text_end(body, next, name).and_then(|e| tag_close(body, e)).map(|(c, _)| c),
                        Kind::Script => script_end(body, next).and_then(|e| tag_close(body, e)).map(|(c, _)| c),
                        Kind::Template => walk(body, next, false, true),
                    };
                    // Its text runs to the page's end: the head's, to there.
                    let Some(end) = end else {
                        each(lt, name, next, body.len());
                        break lt;
                    };
                    each(lt, name, next, end);
                    next = end;
                } else {
                    doc |= is_one(name, &[b"body", b"frameset"]);
                    break lt;
                }
            }
        }
        i = next;
    };
    if !doc && walk(body, stop, true, true).is_none() {
        return None;
    }
    // The head's first noscript is before its `</head>` and what ends it.
    Some((noscript.or(head_end).unwrap_or(stop), implied))
}

/// The titles, metas and links in a `<noscript>` of the head, its text from `from` to `to` (its
/// `</noscript`, or the page's end), as a browser with scripting off reads them, told to `each` as
/// `scan` tells the head's (FEEDBACK84.md, Q-1320, found by the review of a6988b4; branch
/// feedback82-85). With scripting on the browser reads that text as text, no element, so taking
/// one away changes nothing there; with it off they are the head's (a meta or link in the
/// noscript, in the head; a title, script, template or base closes the noscript and is the
/// head's, and what follows), and left, a crawler would read two titles, descriptions or
/// canonicals, the noscript's first. Read as the tree builder reads it there, by its in-head
/// noscript, in-head and after-head modes: spaces, comments, a style's and a noframes' text, a
/// script, a template whole (`walk`, scripting off), `<head>`, `<html>` and the end tags those
/// modes pass over are passed over; `</head>` once the noscript is closed is after the head, where
/// a title, meta or link is still the head's. Anything else ends the head there (text, an `<img>`,
/// an `<svg>`, `</br>`, once closed `</body>` or `</html>`, after `</head>` a `<noscript>`): what
/// follows, the body's, is left. So is all from one that runs past `to` (`<title></noscript>`:
/// with scripting on the noscript ends in its text, so it is not cut, and a crawler reads two); a
/// title that runs to the page's end, in a noscript that does, is the head's to there. A title
/// that closes the noscript is left where taking it away would have what follows read otherwise
/// (`moves`).
fn hidden(body: &[u8], from: usize, to: usize, each: &mut impl FnMut(usize, &[u8], usize, usize)) {
    #[derive(PartialEq, Clone, Copy)]
    enum M {
        /// In the noscript; closed, in the head; after `</head>`.
        Noscript,
        Head,
        After,
    }
    let mut m = M::Noscript;
    let mut i = from;
    loop {
        let lt = memchr::memchr(b'<', &body[i..to]).map_or(to, |p| i + p);
        if lt == to || not_space(body, i, lt).is_some() || !holds(body, lt, to) {
            return;
        }
        let Some((tok, mut next)) = token(body, lt).filter(|&(_, n)| n <= to) else { return };
        match tok {
            Tok::Text => return,
            Tok::Skip { .. } => {}
            Tok::End(name) if name.eq_ignore_ascii_case(b"br") || (m != M::Noscript && is_one(name, &[b"body", b"html"])) => return,
            Tok::End(name) if m == M::Head && name.eq_ignore_ascii_case(b"head") => m = M::After,
            Tok::End(_) => {}
            Tok::Start(name, _) if is_one(name, &[b"meta", b"link"]) => each(lt, name, next, next),
            Tok::Start(name, _) if is_one(name, &[b"basefont", b"bgsound", b"head", b"html"]) => {}
            // Another passed over in one, one more once it closed, the body's after `</head>`.
            Tok::Start(name, _) if name.eq_ignore_ascii_case(b"noscript") => {
                if m == M::After {
                    return;
                }
                m = M::Noscript;
            }
            Tok::Start(name, _) => {
                let title = name.eq_ignore_ascii_case(b"title");
                let raw = is_one(name, &[b"style", b"noframes"]);
                let end = if title || raw {
                    text_end(body, next, name).and_then(|e| tag_close(body, e)).map(|(c, _)| c)
                } else if name.eq_ignore_ascii_case(b"script") {
                    script_end(body, next).and_then(|e| tag_close(body, e)).map(|(c, _)| c)
                } else if name.eq_ignore_ascii_case(b"template") {
                    walk(body, next, false, false)
                } else {
                    Some(next)
                };
                let Some(end) = end.filter(|&e| e <= to).or_else(|| (title && to == body.len()).then_some(to)) else { return };
                if title && (m != M::Noscript || !moves(body, end, to)) {
                    each(lt, name, next, end);
                }
                if m == M::Noscript && !raw {
                    m = M::Head;
                }
                next = end;
            }
        }
        i = next;
    }
}

/// Whether the tag at `lt` (before `to`) is of an element a head holds, read by its name only, so
/// an `<img>`'s, its long `src`, is not read on: any other ends the head. What is not a start tag
/// is read on.
fn holds(body: &[u8], lt: usize, to: usize) -> bool {
    if !body.get(lt + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
        return true;
    }
    let s = &body[lt + 1..to];
    let name = s.iter().position(|&c| is_space(c) || c == b'/' || c == b'>').map_or(s, |p| &s[..p]);
    is_one(name, &HEAD_HOLDS)
}

/// Whether taking away the title that closes a head's noscript, with scripting off, has what
/// follows it in the noscript's text (`from` to `to`) read otherwise (`hidden`; FEEDBACK84.md,
/// Q-1320, found by the review of a6988b4; branch feedback82-85). With the title the head goes on
/// closed; without it in the noscript, which passes over `</head>`, `</body>`, `</html>` and
/// `<noscript>`. The two readings meet again at what ends the head in both (text, an element no
/// head holds, `</br>`), at a script, template, base or title it keeps that closes the noscript in
/// turn, or at its `</noscript>` (or the page's end); not where one of those four comes first
/// (`</body>` or `</html>` would end the head, `<noscript>` start one more), where something runs
/// past `</noscript` (in the noscript, it holds that), or where `</head>` did (after the head
/// with the title, in it without, past the noscript; a page's end is the end of both).
fn moves(body: &[u8], from: usize, to: usize) -> bool {
    let mut after = false;
    let mut i = from;
    loop {
        let lt = memchr::memchr(b'<', &body[i..to]).map_or(to, |p| i + p);
        if not_space(body, i, lt).is_some() {
            return false;
        }
        if lt == to {
            return after && to < body.len();
        }
        if !holds(body, lt, to) {
            return false;
        }
        let Some((tok, next)) = token(body, lt) else { return false };
        let end = match tok {
            Tok::Text => return false,
            Tok::End(name) if name.eq_ignore_ascii_case(b"br") => return false,
            Tok::End(name) if is_one(name, &[b"body", b"html"]) => return true,
            Tok::End(name) => {
                after |= name.eq_ignore_ascii_case(b"head");
                next
            }
            Tok::Start(name, _) if name.eq_ignore_ascii_case(b"noscript") => return true,
            Tok::Start(name, _) if is_one(name, &[b"script", b"template", b"base"]) => return after,
            Tok::Start(name, _) if is_one(name, &[b"title", b"style", b"noframes"]) => {
                let title = name.eq_ignore_ascii_case(b"title");
                match text_end(body, next, name).and_then(|e| tag_close(body, e)) {
                    // One `hidden` takes away too.
                    Some((e, _)) if e <= to => e,
                    // A title it keeps closes the noscript.
                    Some(_) if title => return after,
                    None if title && to < body.len() => return after,
                    Some(_) => return true,
                    None => return false,
                }
            }
            Tok::Skip { .. } | Tok::Start(..) => next,
        };
        if end > to {
            return true;
        }
        i = end;
    }
}

/// The elements a noscript of the head holds, as scripting off reads it (`hidden`), that leave the
/// head open.
const HEAD_HOLDS: [&[u8]; 13] = [b"meta", b"link", b"basefont", b"bgsound", b"head", b"html", b"noscript", b"title", b"style", b"noframes", b"script", b"template", b"base"];

/// How the head's elements end: a meta, link or base with its tag; a title's, style's,
/// noscript's and noframes' text at their end tag; a script's by its states; a template's
/// content by `walk`.
enum Kind {
    Void,
    Text,
    Script,
    Template,
}

fn head_kind(name: &[u8]) -> Option<Kind> {
    if is_one(name, &[b"meta", b"link", b"base", b"basefont", b"bgsound"]) {
        Some(Kind::Void)
    } else if is_one(name, &[b"title", b"style", b"noscript", b"noframes"]) {
        Some(Kind::Text)
    } else if name.eq_ignore_ascii_case(b"script") {
        Some(Kind::Script)
    } else if name.eq_ignore_ascii_case(b"template") {
        Some(Kind::Template)
    } else {
        None
    }
}

fn is_one(name: &[u8], of: &[&[u8]]) -> bool {
    of.iter().any(|w| name.eq_ignore_ascii_case(w))
}

/// Where the text from `i` to `to` has a character that is not a space, the first, as the tree
/// builder reads a head's text: a reference to a space (`&#32;`, `&#x9;`, `&Tab;`, `&NewLine;`)
/// is one.
fn not_space(body: &[u8], mut i: usize, to: usize) -> Option<usize> {
    while i < to {
        match body[i] {
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => i += 1,
            b'&' => match space_ref(&body[i..to]) {
                Some(n) => i += n,
                None => return Some(i),
            },
            _ => return Some(i),
        }
    }
    None
}

/// The length of the character reference at the start of `s` (an `&`) where it is one to a space,
/// as the tokenizer reads it: `&Tab;` and `&NewLine;`, and a number (`&#32`, `&#x0020;`, the `;`
/// left out or not) that is a space's: tab, line feed, form feed, carriage return or space.
fn space_ref(s: &[u8]) -> Option<usize> {
    for (name, n) in [(&b"&Tab;"[..], 5), (&b"&NewLine;"[..], 9)] {
        if s.starts_with(name) {
            return Some(n);
        }
    }
    if s.get(1) != Some(&b'#') {
        return None;
    }
    let hex = matches!(s.get(2), Some(b'x' | b'X'));
    let radix = if hex { 16 } else { 10 };
    let from = 2 + usize::from(hex);
    let mut k = from;
    let mut v: u32 = 0;
    while let Some(d) = s.get(k).and_then(|&c| (c as char).to_digit(radix)) {
        v = v.saturating_mul(radix).saturating_add(d);
        k += 1;
    }
    if k == from {
        return None;
    }
    k += usize::from(s.get(k) == Some(&b';'));
    matches!(v, 9 | 10 | 12 | 13 | 32).then_some(k)
}

/// What is open, as `walk` reads a template's content or the page's rest: what changes how what
/// follows is read.
#[derive(Clone, Copy)]
enum Open<'a> {
    Template,
    /// An element of the page's own (HTML), its name as written.
    Html(&'a [u8]),
    /// An SVG or MathML one (foreign content, where a `<title>`, `<style>` or `<script>` holds
    /// tags and `<![CDATA[` is text): its name as written, whether SVG, and where it reads a start
    /// tag as the page's own (`Point`).
    Foreign { name: &'a [u8], svg: bool, point: Point },
}

/// What a foreign element reads as the page's own: at an HTML integration point (SVG's
/// foreignObject, desc and title, MathML's annotation-xml for HTML) every start tag; at a MathML
/// text integration point (mi, mo, mn, ms, mtext) all but mglyph and malignmark; else none.
#[derive(Clone, Copy, PartialEq)]
enum Point {
    No,
    Html,
    Text,
}

/// The page's own elements whose start tag ends foreign content (`<font>` with a color, face or
/// size too).
const BREAK_OUT: [&[u8]; 44] = [
    b"b", b"big", b"blockquote", b"body", b"br", b"center", b"code", b"dd", b"div", b"dl", b"dt", b"em", b"embed", b"h1", b"h2", b"h3", b"h4", b"h5", b"h6", b"head", b"hr", b"i", b"img",
    b"li", b"listing", b"menu", b"meta", b"nobr", b"ol", b"p", b"pre", b"ruby", b"s", b"small", b"span", b"strong", b"strike", b"sub", b"sup", b"table", b"tt", b"u", b"ul", b"var",
];

/// The elements whose text is no tags, to their end tag: a title's and a textarea's, a style's
/// and the other raw text elements' (noscript's as the browser reads it, with scripts on).
const RAW: [&[u8]; 8] = [b"title", b"textarea", b"style", b"xmp", b"iframe", b"noembed", b"noframes", b"noscript"];

/// The elements with no end, which nothing is inside.
const VOID: [&[u8]; 19] = [
    b"area", b"base", b"basefont", b"bgsound", b"br", b"col", b"embed", b"frame", b"hr", b"image", b"img", b"input", b"keygen", b"link", b"meta", b"param", b"source", b"track", b"wbr",
];

fn foreign<'a>(name: &'a [u8], tag: &[u8], svg: bool) -> Open<'a> {
    let point = if svg {
        if is_one(name, &[b"foreignobject", b"desc", b"title"]) { Point::Html } else { Point::No }
    } else if is_one(name, &[b"mi", b"mo", b"mn", b"ms", b"mtext"]) {
        Point::Text
    } else if name.eq_ignore_ascii_case(b"annotation-xml")
        && attr_of(tag, b"encoding").is_some_and(|e| e.eq_ignore_ascii_case(b"text/html") || e.eq_ignore_ascii_case(b"application/xhtml+xml"))
    {
        Point::Html
    } else {
        Point::No
    };
    Open::Foreign { name, svg, point }
}

/// A template's content, from `from` (just past `<template ...>`), read as the tree builder reads
/// it there (FEEDBACK84.md, Q-1320, found by the review of 1a4bb9b; branch feedback82-85): just
/// past the `</template>` that closes it, a template in it closed first; None where it runs to the
/// page's end. A script's, a title's and the raw text elements' text holds no tags, nor a comment
/// or, in SVG or MathML, a `<![CDATA[`; there a `<title>`, `<style>` or `<script>` is the
/// foreign element's own and holds tags, up to an integration point (where a `<![CDATA[` is a
/// comment) or one of the page's own elements that breaks out of it, and a `</template>` past the
/// foreign elements closes the template. With `signal`, the page's rest from `from`, read alike: where a doctype, `<html>`,
/// `<head>`, `<body>`, `<frameset>` or `</head>` is outside a template, which makes it a document;
/// None where none is. Past `DEEP` open elements no more are followed (templates are), so a page
/// of many that never end, and as many end tags that close none, is still read in one pass; the
/// commonest elements left open, a list item, a paragraph, a cell, end as the body ends them
/// (`ends_before`). A noscript's text is raw text with `scripting`, tags without it (a template in a
/// head's noscript, as a crawler reads it: `hidden`; FEEDBACK84.md, Q-1320, found by the review
/// of a6988b4; branch feedback82-85).
fn walk(body: &[u8], from: usize, signal: bool, scripting: bool) -> Option<usize> {
    const DEEP: usize = 64;
    let mut open: Vec<Open> = if signal { Vec::new() } else { vec![Open::Template] };
    let mut templates = open.len();
    let mut i = from;
    while let Some(lt) = memchr::memchr(b'<', &body[i..]).map(|p| i + p) {
        let top = open.last().copied();
        // A CDATA section in foreign content, not at an integration point (as parse5 reads it).
        if matches!(top, Some(Open::Foreign { point: Point::No, .. })) && body[lt..].starts_with(b"<![CDATA[") {
            i = lt + 9 + memchr::memmem::find(&body[lt + 9..], b"]]>")? + 3;
            continue;
        }
        let (tok, next) = token(body, lt)?;
        i = next;
        match tok {
            Tok::Text => {}
            Tok::Skip { doctype } => {
                if signal && doctype && templates == 0 {
                    return Some(lt);
                }
            }
            Tok::Start(name, closed) => {
                if signal && is_one(name, &[b"html", b"head", b"body", b"frameset"]) && templates == 0 {
                    return Some(lt);
                }
                let tag = &body[lt..next];
                let mut own = match top {
                    Some(Open::Foreign { name: n, point, .. }) => {
                        point == Point::Html
                            || (point == Point::Text && !is_one(name, &[b"mglyph", b"malignmark"]))
                            || (n.eq_ignore_ascii_case(b"annotation-xml") && name.eq_ignore_ascii_case(b"svg"))
                    }
                    _ => true,
                };
                if !own && (is_one(name, &BREAK_OUT) || (name.eq_ignore_ascii_case(b"font") && [&b"color"[..], b"face", b"size"].iter().any(|a| attr_of(tag, a).is_some()))) {
                    while let Some(Open::Foreign { point: Point::No, .. }) = open.last() {
                        open.pop();
                    }
                    own = true;
                }
                let room = open.len() < DEEP;
                if !own {
                    if !closed && room {
                        open.push(foreign(name, tag, matches!(top, Some(Open::Foreign { svg: true, .. }))));
                    }
                } else if is_one(name, &[b"svg", b"math"]) {
                    if !closed && room {
                        open.push(foreign(name, tag, name.eq_ignore_ascii_case(b"svg")));
                    }
                } else if name.eq_ignore_ascii_case(b"template") {
                    open.push(Open::Template);
                    templates += 1;
                } else if name.eq_ignore_ascii_case(b"script") {
                    i = tag_close(body, script_end(body, next)?)?.0;
                } else if is_one(name, &RAW) && (scripting || !name.eq_ignore_ascii_case(b"noscript")) {
                    i = tag_close(body, text_end(body, next, name)?)?.0;
                } else if name.eq_ignore_ascii_case(b"plaintext") {
                    return None;
                } else if !is_one(name, &VOID) {
                    ends_before(&mut open, name);
                    if open.len() < DEEP {
                        open.push(Open::Html(name));
                    }
                }
            }
            Tok::End(name) => {
                if signal && name.eq_ignore_ascii_case(b"head") && templates == 0 {
                    return Some(lt);
                }
                // In foreign content an end tag closes the innermost foreign element of its name,
                // unless one of the page's own comes first, which reads it as its own; `</br>` and
                // `</p>` break out of it.
                let mut own = true;
                if let Some(Open::Foreign { .. }) = top {
                    if is_one(name, &[b"br", b"p"]) {
                        while let Some(Open::Foreign { point: Point::No, .. }) = open.last() {
                            open.pop();
                        }
                    } else {
                        own = false;
                        for k in (0..open.len()).rev() {
                            match open[k] {
                                Open::Foreign { name: n, .. } if n.eq_ignore_ascii_case(name) => {
                                    open.truncate(k);
                                    break;
                                }
                                Open::Foreign { .. } => {}
                                _ => {
                                    own = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                if own {
                    // The page's own closes the innermost of its name in the innermost template;
                    // `</template>` that template.
                    let template = name.eq_ignore_ascii_case(b"template");
                    let k = open.iter().rposition(|o| match o {
                        Open::Template => true,
                        Open::Html(n) => !template && n.eq_ignore_ascii_case(name),
                        Open::Foreign { .. } => false,
                    });
                    if let Some(k) = k.filter(|&k| template || matches!(open[k], Open::Html(_))) {
                        open.truncate(k);
                        templates -= usize::from(template);
                        if open.is_empty() && !signal {
                            return Some(next);
                        }
                    }
                }
            }
        }
    }
    None
}

/// The commonest of the body's implied end tags, read at the top of what is open: a list item ends
/// a list item or a paragraph before it, a term or definition one of those, an option an option,
/// a cell a cell, a row a cell and a row, and a block (`<div>`, `<ul>`, `<h1>`, ...) a paragraph.
fn ends_before(open: &mut Vec<Open>, name: &[u8]) {
    let ends: &[&[u8]] = if name.eq_ignore_ascii_case(b"li") {
        &[b"li", b"p"]
    } else if is_one(name, &[b"dt", b"dd"]) {
        &[b"dt", b"dd", b"p"]
    } else if name.eq_ignore_ascii_case(b"option") {
        &[b"option"]
    } else if name.eq_ignore_ascii_case(b"optgroup") {
        &[b"option", b"optgroup"]
    } else if is_one(name, &[b"td", b"th"]) {
        &[b"td", b"th"]
    } else if name.eq_ignore_ascii_case(b"tr") {
        &[b"td", b"th", b"tr"]
    } else if is_one(name, &BLOCKS) {
        &[b"p"]
    } else {
        return;
    };
    while let Some(Open::Html(n)) = open.last() {
        if !is_one(n, ends) {
            break;
        }
        open.pop();
    }
}

/// The elements whose start tag ends a paragraph open before them.
const BLOCKS: [&[u8]; 32] = [
    b"address", b"article", b"aside", b"blockquote", b"center", b"details", b"dialog", b"dir", b"div", b"dl", b"fieldset", b"figcaption", b"figure", b"footer", b"form", b"h1", b"h2",
    b"h3", b"h4", b"h5", b"h6", b"header", b"hgroup", b"listing", b"main", b"menu", b"nav", b"ol", b"p", b"pre", b"section", b"ul",
];

/// What a `<` in the page's text starts.
enum Tok<'a> {
    /// A `<` that starts nothing (`a < b`, `<3`): text.
    Text,
    /// A comment, a doctype, or another `<!...>`, `<?...>` or `</ ...>`, passed over whole.
    Skip { doctype: bool },
    /// A start tag, its name as written and whether it ends `/>`; an end tag, its name.
    Start(&'a [u8], bool),
    End(&'a [u8]),
}

/// What starts at `lt` (a `<`), and just past it; None for a tag, comment or doctype that never
/// ends.
fn token(body: &[u8], lt: usize) -> Option<(Tok<'_>, usize)> {
    let s = &body[lt..];
    // A doctype, and a `<!x>`, `<?x>` or `</ x>` the browser reads as a comment, end at their
    // first `>`, even one in quotes (`<!doctype html "a>b">` ends at `a>`), as the browser ends one.
    let to_gt = || memchr::memchr(b'>', s).map(|p| lt + p + 1);
    let name = |from: usize| &s[from..s[from..].iter().position(|&c| is_space(c) || c == b'/' || c == b'>').map_or(s.len(), |p| from + p)];
    match s.get(1).copied() {
        Some(b'!') if s[2..].starts_with(b"--") => Some((Tok::Skip { doctype: false }, comment_end(body, lt)?)),
        Some(b'!') => Some((Tok::Skip { doctype: s.len() >= 9 && s[2..9].eq_ignore_ascii_case(b"doctype") }, to_gt()?)),
        Some(b'?') => Some((Tok::Skip { doctype: false }, to_gt()?)),
        Some(b'/') => match s.get(2).copied() {
            Some(c) if c.is_ascii_alphabetic() => Some((Tok::End(name(2)), tag_close(body, lt)?.0)),
            Some(b'>') => Some((Tok::Skip { doctype: false }, lt + 3)),
            Some(_) => Some((Tok::Skip { doctype: false }, to_gt()?)),
            None => Some((Tok::Text, lt + 1)),
        },
        Some(c) if c.is_ascii_alphabetic() => {
            let (end, closed) = tag_close(body, lt)?;
            Some((Tok::Start(name(1), closed), end))
        }
        _ => Some((Tok::Text, lt + 1)),
    }
}

/// Just past the comment at `lt` (`<!--`), as the browser ends one: at its first `-->` or
/// `--!>`, where `<!-->` and `<!--->` are whole; None where it never ends.
fn comment_end(body: &[u8], lt: usize) -> Option<usize> {
    let s = &body[lt..];
    if s.get(4) == Some(&b'>') {
        return Some(lt + 5);
    }
    if s.get(4..6) == Some(b"->") {
        return Some(lt + 6);
    }
    let mut k = 4;
    while let Some(p) = memchr::memchr(b'>', &s[k..]) {
        let g = k + p;
        if (g >= 6 && &s[g - 2..g] == b"--") || (g >= 7 && &s[g - 3..g] == b"--!") {
            return Some(lt + g + 1);
        }
        k = g + 1;
    }
    None
}

/// Whether `body` at `at` says `name` (in any case) whole: a space, `/` or `>` after it, as the
/// tokenizer reads an end tag's name before it takes it for the element's own.
fn name_at(body: &[u8], at: usize, name: &[u8]) -> bool {
    body.get(at..at + name.len()).is_some_and(|w| w.eq_ignore_ascii_case(name)) && body.get(at + name.len()).is_some_and(|&c| is_space(c) || c == b'/' || c == b'>')
}

/// Where the text of the element `name`, from `from`, ends: its end tag's `<`, `</name` whole
/// and in any case (`</scripts>` is text in a script); None where it runs to the page's end.
fn text_end(body: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let mut i = from;
    while let Some(lt) = memchr::memchr(b'<', &body[i..]).map(|p| i + p) {
        if body.get(lt + 1) == Some(&b'/') && name_at(body, lt + 2, name) {
            return Some(lt);
        }
        i = lt + 1;
    }
    None
}

/// Where a script's text, from `from` (just past `<script ...>`), ends: the `<` of the `</script`
/// that ends it, read by the tokenizer's script data states (the HTML standard's 13.2.5.4 on;
/// FEEDBACK84.md, Q-1320, found by the review of 1a4bb9b; branch feedback82-85: the first
/// `</script` was taken). After `<!--` the text is escaped, and `-->` ends that; escaped, a
/// `<script` (and a space, `/` or `>`) makes it double escaped, where `</script` ends nothing but
/// the double escape, back to escaped. None where it runs to the page's end.
fn script_end(body: &[u8], from: usize) -> Option<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Data,
        Escaped,
        Dash,
        DashDash,
        Double,
        DoubleDash,
        DoubleDashDash,
    }
    let ends = |lt: usize| body.get(lt + 1) == Some(&b'/') && name_at(body, lt + 2, b"script");
    // Past a run of letters from `k`, and whether they say `script` and a space, `/` or `>`
    // follows: `script` whole.
    let word = |k: usize| {
        let e = body[k..].iter().position(|c| !c.is_ascii_alphabetic()).map_or(body.len(), |p| k + p);
        (e, name_at(body, k, b"script") && e == k + 6)
    };
    let mut s = S::Data;
    let mut i = from;
    loop {
        // Where the next `<` is that the state reads, and the state it is read in.
        let lt = match s {
            S::Data => {
                let lt = i + memchr::memchr(b'<', &body[i..])?;
                if ends(lt) {
                    return Some(lt);
                }
                if body[lt + 1..].starts_with(b"!--") {
                    s = S::DashDash;
                    i = lt + 4;
                } else {
                    i = lt + 1;
                }
                continue;
            }
            S::Escaped | S::Double => {
                let p = i + memchr::memchr2(b'-', b'<', &body[i..])?;
                if body[p] == b'-' {
                    s = if s == S::Escaped { S::Dash } else { S::DoubleDash };
                    i = p + 1;
                    continue;
                }
                p
            }
            S::Dash | S::DashDash | S::DoubleDash | S::DoubleDashDash => {
                let double = matches!(s, S::DoubleDash | S::DoubleDashDash);
                match *body.get(i)? {
                    b'-' => s = if double { S::DoubleDashDash } else { S::DashDash },
                    b'<' => {
                        s = if double { S::Double } else { S::Escaped };
                        continue;
                    }
                    b'>' if matches!(s, S::DashDash | S::DoubleDashDash) => s = S::Data,
                    _ => s = if double { S::Double } else { S::Escaped },
                }
                i += 1;
                continue;
            }
        };
        // A `<` read escaped: `</script` ends the script, `<script` starts the double escape. Read
        // double escaped: `</script` ends that. Anything else is text.
        if s == S::Escaped {
            if ends(lt) {
                return Some(lt);
            }
            if body.get(lt + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
                let (e, script) = word(lt + 1);
                (s, i) = if script { (S::Double, e + 1) } else { (S::Escaped, e) };
            } else {
                i = lt + 1;
            }
        } else if body.get(lt + 1) == Some(&b'/') {
            let (e, script) = word(lt + 2);
            let then = body.get(e).is_some_and(|&c| is_space(c) || c == b'/' || c == b'>');
            (s, i) = if script { (S::Escaped, e + 1) } else { (S::Double, e + usize::from(then)) };
        } else {
            i = lt + 1;
        }
    }
}

/// The `<script>` elements the page's head was written with, each whole, in their order, which a
/// classic script's meaning depends on (review of slice H:
/// what a page that moves in place compares, rather than the live head, where a library or an
/// analytics script adds its own). Those the browser puts in the head (`scan`: a template's are
/// none, FEEDBACK84.md, Q-1320, found by the review of 1a4bb9b; branch feedback82-85); none of a
/// page that is no document.
pub fn head_scripts(body: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let doc = scan(body, false, |lt, name, _, end| {
        if name.eq_ignore_ascii_case(b"script") {
            out.push(String::from_utf8_lossy(&body[lt..end]).into_owned());
        }
    });
    if doc.is_none() {
        out.clear();
    }
    out
}

/// Just past the `>` that ends the tag at `lt` (a start or end tag), and whether it ends `/>` (a
/// foreign element's whole): read by the tokenizer's states, so a quote opens a value only where a
/// value starts, after an attribute's name and `=` (FEEDBACK84.md, Q-1320, found by the coverage
/// tests; branch feedback82-85). `<a b="x>y">` ends at its last `>`; `<a b'c>`, `<a b=c"d>`,
/// `<a ="x>` (a name that starts with `=`) and `<a b==">` (a value that does) at their first, as
/// the browser ends them. `<a b=c/>` does not end `/>`: its `/` is the value's.
fn tag_close(body: &[u8], lt: usize) -> Option<(usize, bool)> {
    #[derive(Clone, Copy)]
    enum S {
        /// The tag's name; an unquoted value.
        Name,
        Unquoted,
        /// Before an attribute's name (after a space, a `/`, a quoted value).
        Before,
        /// In an attribute's name, after it.
        Attr,
        After,
        /// After its `=`.
        Value,
    }
    let mut s = S::Name;
    let mut slash = false;
    let mut k = lt + 1 + usize::from(body.get(lt + 1) == Some(&b'/'));
    while k < body.len() {
        let c = body[k];
        if c == b'>' {
            return Some((k + 1, slash));
        }
        let space = is_space(c);
        slash = c == b'/' && matches!(s, S::Name | S::Before | S::Attr | S::After);
        s = match s {
            S::Name | S::Unquoted if space => S::Before,
            S::Name if c == b'/' => S::Before,
            S::Name | S::Unquoted => s,
            S::Before if space || c == b'/' => S::Before,
            S::Before => S::Attr,
            S::Attr | S::After if c == b'=' => S::Value,
            S::Attr | S::After if c == b'/' => S::Before,
            S::Attr | S::After if space => S::After,
            S::Attr | S::After => S::Attr,
            S::Value if space => S::Value,
            S::Value if c == b'"' || c == b'\'' => {
                k += 1 + memchr::memchr(c, &body[k + 1..])?;
                S::Before
            }
            S::Value => S::Unquoted,
        };
        k += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(pairs: &[(&str, &str)]) -> Head {
        let mut h = Head::default();
        for (k, v) in pairs {
            h.say(KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
            h.called();
        }
        h
    }

    fn applied(page: &str, h: &Head) -> String {
        let mut b = page.as_bytes().to_vec();
        apply(&mut b, h);
        String::from_utf8(b).unwrap()
    }

    #[test]
    fn the_heads_tags_of_a_key_said_are_replaced() {
        let h = head(&[("title", "New <b>"), ("description", "He said \"hi\"")]);
        let out = applied(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Old</title><meta name=\"description\" content=\"old\"><meta name=\"viewport\" content=\"x\"></head><body><svg><title>shape</title></svg></body></html>",
            &h,
        );
        assert_eq!(
            out,
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"x\"><title>New &lt;b&gt;</title><meta property=\"og:title\" content=\"New &lt;b&gt;\"><meta name=\"description\" content=\"He said &quot;hi&quot;\"><meta property=\"og:description\" content=\"He said &quot;hi&quot;\"></head><body><svg><title>shape</title></svg></body></html>"
        );
    }

    #[test]
    fn a_page_without_a_head_gets_one_before_its_body_and_a_part_none() {
        let h = head(&[("title", "T")]);
        assert_eq!(applied("<html><body>x</body></html>", &h), "<html><title>T</title><meta property=\"og:title\" content=\"T\"><body>x</body></html>");
        assert_eq!(applied("<!doctype html><p>x</p>", &h), "<!doctype html><title>T</title><meta property=\"og:title\" content=\"T\"><p>x</p>");
        assert_eq!(applied("<li>part</li>", &h), "<li>part</li>");
        assert_eq!(applied("", &h), "");
    }

    #[test]
    fn a_script_in_the_head_is_not_read_as_tags() {
        let h = head(&[("title", "T")]);
        let page = "<head><script>let s = \"<title>no</title>\"</script><!-- <title>c</title> --><TITLE>Old</TITLE></head>";
        assert_eq!(applied(page, &h), "<head><script>let s = \"<title>no</title>\"</script><!-- <title>c</title> --><title>T</title><meta property=\"og:title\" content=\"T\"></head>");
    }

    #[test]
    fn canonical_and_image_replace_theirs_only() {
        let h = head(&[("canonical", "https://x.example/a"), ("image", "https://x.example/i.png")]);
        let page = "<head><link rel=\"icon\" href=\"/i\"><link rel='canonical' href='/old'><meta property=\"og:url\" content=\"o\"><meta property=og:image content=o><title>Keep</title></head>";
        assert_eq!(
            applied(page, &h),
            "<head><link rel=\"icon\" href=\"/i\"><title>Keep</title><meta property=\"og:image\" content=\"https://x.example/i.png\"><link rel=\"canonical\" href=\"https://x.example/a\"><meta property=\"og:url\" content=\"https://x.example/a\"></head>"
        );
    }

    #[test]
    fn the_innermost_wins_and_of_one_depth_the_later() {
        let mut h = Head::default();
        h.say(0, 1, 0, "inner".into());
        h.called();
        h.say(0, 0, 3, "layout".into());
        h.called();
        assert_eq!(h.get("title"), Some("inner"));
        h.say(0, 1, 0, "later".into());
        assert_eq!(h.get("title"), Some("later"));
        // At one depth, the one inside more components; then the later.
        h.say(0, 1, 2, "in a component".into());
        h.say(0, 1, 1, "fewer".into());
        assert_eq!(h.get("title"), Some("in a component"));
        assert_eq!(h.get("description"), None);
        assert_eq!(applied("<head></head>", &Head::default()), "<head></head>");
    }

    #[test]
    fn a_head_or_body_said_in_a_comment_or_a_script_is_no_tag() {
        let h = head(&[("title", "T")]);
        let page = "<html><head><!-- </head><body> --><script>let s = \"</head><body>\"</script><title>x</title></head><body>b</body></html>";
        assert_eq!(
            applied(page, &h),
            "<html><head><!-- </head><body> --><script>let s = \"</head><body>\"</script><title>T</title><meta property=\"og:title\" content=\"T\"></head><body>b</body></html>"
        );
        // No head: before the page's own <body>, not one in a script.
        let page = "<html><script>\"<body>\"</script><body>b</body></html>";
        assert_eq!(applied(page, &h), "<html><script>\"<body>\"</script><title>T</title><meta property=\"og:title\" content=\"T\"><body>b</body></html>");
    }

    #[test]
    fn the_heads_scripts_are_read_in_their_order() {
        let a = head_scripts(b"<html><head><script src=\"/b.js\"></script><title>x</title><SCRIPT>let a = 1</SCRIPT></head><body><script>body</script></body></html>");
        assert_eq!(a, ["<script src=\"/b.js\"></script>", "<SCRIPT>let a = 1</SCRIPT>"]);
        let b = head_scripts(b"<head><SCRIPT>let a = 1</SCRIPT><script src=\"/b.js\"></script></head>");
        assert_ne!(a, b);
        assert!(head_scripts(b"<p>no head</p>").is_empty());
        assert!(head_scripts(b"").is_empty());
    }

    #[test]
    fn unicode_values_are_written_whole() {
        let h = head(&[("title", "Café · 東京")]);
        assert_eq!(applied("<head><title>x</title></head>", &h), "<head><title>Café · 東京</title><meta property=\"og:title\" content=\"Café · 東京\"></head>");
    }
}
