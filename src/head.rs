//! `page_head({ title, description, image, canonical })`: what a page's code says goes in the
//! document's head (FEEDBACK84.md, Q-1320, Q-1321; FEEDBACK81.md, Q-1275; branch feedback82-85).
//! Decided (user, 2026-11-22): a builtin, Nuxt's way, not a `<head>` block.
//!
//! On the server each call while a page is made is kept here, by key; once the page is made,
//! the document's head gets the winners: for each key its tags in the head are taken away and
//! the call's written before `</head>`. Which call wins a key: the innermost. A call in a view's
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
            let n = attr_of(tag, b"name").or_else(|| attr_of(tag, b"property")).map(|v| v.to_ascii_lowercase());
            match n.as_deref() {
                Some("og:title") => said("title"),
                Some("description" | "og:description") => said("description"),
                Some("og:image") => said("image"),
                Some("og:url") => said("canonical"),
                _ => false,
            }
        } else if name.eq_ignore_ascii_case(b"link") {
            said("canonical") && attr_of(tag, b"rel").is_some_and(|r| r.split_ascii_whitespace().any(|w| w.eq_ignore_ascii_case("canonical")))
        } else {
            false
        }
    }
}

/// The value of attribute `name` in one tag's text, its quotes taken off (entities left as they
/// are: the names compared are plain words).
fn attr_of(tag: &[u8], name: &[u8]) -> Option<String> {
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
            return Some(String::from_utf8_lossy(value).into_owned());
        }
    }
    None
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

/// The page `body` with `head`'s tags in its head, in place of those it had of the same keys. A
/// page that is no document (no `<head>`, `<body>`, `<html>` or doctype: a part of one, as an
/// `hx-get` asks for) is left as it is.
pub fn apply(body: &mut Vec<u8>, head: &Head) {
    let tags = head.tags();
    if tags.is_empty() {
        return;
    }
    // Read once, as the browser reads it (`marks`): where the head ends, and the head's own tags
    // of the keys said, to be taken away: a `<title>` with its text and end tag, a `<meta>` or
    // `<link>` whole.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let m = marks(body, |lt, name, close, end| {
        if head.replaces(name, &body[lt..close]) {
            cuts.push((lt, if name.eq_ignore_ascii_case(b"title") { end } else { close }));
        }
    });
    // Where the new tags go: before `</head>`; else before `<body>`, which a head left out ends;
    // else, after the head's start, where the rest of the page is one tag, comment or text that
    // never ends (which the browser drops, or takes whole); else after `<html ...>` or the
    // doctype, each just past its own `>`, never inside it (FEEDBACK84.md, Q-1320, found by the
    // coverage tests; branch feedback82-85: `<html lang="a>b">` took the tags at its first `>`).
    let Some(mut at) = m.head_end.or(m.body_at).or(m.open_in_head).or(m.html_end).or(m.doctype_end) else { return };
    // Only those before it: a page with no head's end read on to its last tag.
    for &(from, to) in cuts.iter().rev() {
        if to <= at {
            body.drain(from..to);
            at -= to - from;
        }
    }
    body.splice(at..at, tags.bytes());
}

/// Where a page's own tags are, read as the browser's tokenizer reads them (the HTML standard's),
/// up to the page's first `</head>` or `<body ...>` (FEEDBACK84.md, Q-1320, found by the coverage
/// tests; branch feedback82-85).
#[derive(Default)]
struct Marks {
    /// Where `</head>` starts.
    head_end: Option<usize>,
    /// Where `<body ...>` starts.
    body_at: Option<usize>,
    /// Just past the first `<html ...>`, and the first doctype.
    html_end: Option<usize>,
    doctype_end: Option<usize>,
    /// Where, after `<head ...>`, a tag, comment or element's text starts that runs on to the
    /// page's end: what the browser reads of the head ends there.
    open_in_head: Option<usize>,
}

/// The elements whose text is no tags, up to their own end tag (`<title>`, `<textarea>`: text;
/// `<script>`, `<style>` and the others: raw text; `<template>`'s, which the browser holds apart,
/// passed over whole too).
const TEXT_ONLY: [&[u8]; 10] = [b"script", b"style", b"title", b"textarea", b"noscript", b"template", b"iframe", b"noembed", b"noframes", b"xmp"];

/// The page read thing by thing, as the browser reads it, up to its first `</head>` or `<body>`:
/// a comment, a doctype and any other `<!...>`, `<?...>` or `</ ...>` passed over whole; a tag to
/// the `>` that ends it, quotes and all; the text of the `TEXT_ONLY` elements to their end tag.
/// `start` is told of each start tag before them, as (where it starts, its name as written, just
/// past its `>`, just past its element: its end tag's `>` for a `TEXT_ONLY` one, else its own).
fn marks(body: &[u8], mut start: impl FnMut(usize, &[u8], usize, usize)) -> Marks {
    let mut m = Marks::default();
    let mut in_head = false;
    let mut i = 0;
    while let Some(lt) = memchr::memchr(b'<', &body[i..]).map(|p| i + p) {
        let Some((tok, next)) = token(body, lt) else {
            // A tag that never ends: the browser drops it and all after it; a comment or doctype
            // that never ends takes the rest.
            if in_head {
                m.open_in_head = Some(lt);
            }
            break;
        };
        i = next;
        match tok {
            Tok::Text => {}
            Tok::Skip { doctype } => {
                if doctype && m.doctype_end.is_none() {
                    m.doctype_end = Some(next);
                }
            }
            Tok::End(name) => {
                if name.eq_ignore_ascii_case(b"head") {
                    m.head_end = Some(lt);
                    break;
                }
            }
            Tok::Start(name) => {
                if name.eq_ignore_ascii_case(b"body") {
                    m.body_at = Some(lt);
                    break;
                }
                if name.eq_ignore_ascii_case(b"html") && m.html_end.is_none() {
                    m.html_end = Some(next);
                }
                in_head |= name.eq_ignore_ascii_case(b"head");
                if name.eq_ignore_ascii_case(b"plaintext") {
                    break;
                }
                let mut end = next;
                if TEXT_ONLY.iter().any(|t| name.eq_ignore_ascii_case(t)) {
                    // Its text runs to its end tag, or to the page's end.
                    let Some(e) = text_end(body, next, name) else {
                        if in_head {
                            m.open_in_head = Some(lt);
                        }
                        break;
                    };
                    end = tag_close(body, e, body.len()).unwrap_or(body.len());
                    i = e;
                }
                start(lt, name, next, end);
            }
        }
    }
    m
}

/// What a `<` in the page's text starts.
enum Tok<'a> {
    /// A `<` that starts nothing (`a < b`, `<3`): text.
    Text,
    /// A comment, a doctype, or another `<!...>`, `<?...>` or `</ ...>`, passed over whole.
    Skip { doctype: bool },
    /// A start tag, an end tag: its name as written.
    Start(&'a [u8]),
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
            Some(c) if c.is_ascii_alphabetic() => Some((Tok::End(name(2)), tag_close(body, lt, body.len())?)),
            Some(b'>') => Some((Tok::Skip { doctype: false }, lt + 3)),
            Some(_) => Some((Tok::Skip { doctype: false }, to_gt()?)),
            None => Some((Tok::Text, lt + 1)),
        },
        Some(c) if c.is_ascii_alphabetic() => Some((Tok::Start(name(1)), tag_close(body, lt, body.len())?)),
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

/// Where the text of the element `name`, from `from`, ends: its end tag's `<`, `</name` whole
/// and in any case (`</scripts>` is text in a script); None where it runs to the page's end.
fn text_end(body: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let mut i = from;
    while let Some(lt) = memchr::memchr(b'<', &body[i..]).map(|p| i + p) {
        let n = lt + 2;
        if body.get(lt + 1) == Some(&b'/')
            && body.get(n..n + name.len()).is_some_and(|w| w.eq_ignore_ascii_case(name))
            && body.get(n + name.len()).is_some_and(|&c| is_space(c) || c == b'/' || c == b'>')
        {
            return Some(lt);
        }
        i = lt + 1;
    }
    None
}

/// Where the page's own `</head>` and `<body>` tags start (`marks`).
fn head_and_body(body: &[u8]) -> (Option<usize>, Option<usize>) {
    let m = marks(body, |_, _, _, _| {});
    (m.head_end, m.body_at)
}

/// The `<script>` elements the page's head was written with, each whole, in their order, which a
/// classic script's meaning depends on (review of slice H:
/// what a page that moves in place compares, rather than the live head, where a library or an
/// analytics script adds its own).
pub fn head_scripts(body: &[u8]) -> Vec<String> {
    let (head, body_at) = head_and_body(body);
    let end = head.or(body_at).unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = find_ci(body, i, end, b"<script") {
        let stop = find_ci(body, at, end, b"</script").and_then(|e| tag_close(body, e, end)).unwrap_or(end);
        out.push(String::from_utf8_lossy(&body[at..stop]).into_owned());
        i = stop.max(at + 7);
    }
    out
}

/// Just past the `>` that ends the tag at `lt` (a start or end tag), before `limit`: read by the
/// tokenizer's states, so a quote opens a value only where a value starts, after an attribute's
/// name and `=` (FEEDBACK84.md, Q-1320, found by the coverage tests; branch feedback82-85).
/// `<a b="x>y">` ends at its last `>`; `<a b'c>`, `<a b=c"d>`, `<a ="x>` (a name that starts
/// with `=`) and `<a b==">` (a value that does) at their first, as the browser ends them.
fn tag_close(body: &[u8], lt: usize, limit: usize) -> Option<usize> {
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
    let mut k = lt + 1 + usize::from(body.get(lt + 1) == Some(&b'/'));
    while k < limit {
        let c = body[k];
        if c == b'>' {
            return Some(k + 1);
        }
        let space = is_space(c);
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
                k += 1 + memchr::memchr(c, &body[k + 1..limit])?;
                S::Before
            }
            S::Value => S::Unquoted,
        };
        k += 1;
    }
    None
}

fn find_ci(body: &[u8], from: usize, limit: usize, needle: &[u8]) -> Option<usize> {
    if from >= limit {
        return None;
    }
    body[from..limit].windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle)).map(|p| from + p)
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
