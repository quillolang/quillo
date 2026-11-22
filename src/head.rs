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

    /// Whether `tag` (one tag's text, `<meta ...>`) is one of a key this head says.
    fn replaces(&self, name: &str, tag: &[u8]) -> bool {
        let said = |k: &str| self.get(k).is_some();
        match name {
            "title" => said("title"),
            "meta" => {
                let n = attr_of(tag, b"name").or_else(|| attr_of(tag, b"property")).map(|v| v.to_ascii_lowercase());
                match n.as_deref() {
                    Some("og:title") => said("title"),
                    Some("description" | "og:description") => said("description"),
                    Some("og:image") => said("image"),
                    Some("og:url") => said("canonical"),
                    _ => false,
                }
            }
            "link" => said("canonical") && attr_of(tag, b"rel").is_some_and(|r| r.split_ascii_whitespace().any(|w| w.eq_ignore_ascii_case("canonical"))),
            _ => false,
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

/// Where the tag named `name` (`</head`, `<body`) first starts, any case, its name whole.
fn tag_at(body: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let (&first, _) = name.split_first()?;
    memchr::memchr_iter(first, &body[from..]).map(|i| from + i).find(|&i| {
        body.len() - i > name.len() && body[i..i + name.len()].eq_ignore_ascii_case(name) && matches!(body[i + name.len()], b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
    })
}

/// The page `body` with `head`'s tags in its head, in place of those it had of the same keys. A
/// page that is no document (no `<head>`, `<body>`, `<html>` or doctype: a part of one, as an
/// `hx-get` asks for) is left as it is.
pub fn apply(body: &mut Vec<u8>, head: &Head) {
    let tags = head.tags();
    if tags.is_empty() {
        return;
    }
    // Read as tags, past comments and scripts, whose text may say `</head>` (review of slice H).
    let (head_end, body_at) = head_and_body(body);
    // Where the new tags go: before `</head>`; else before `<body>`, which a head left out ends;
    // else after `<html ...>` or the doctype.
    let after = |at: usize, body: &[u8]| body[at..].iter().position(|&c| c == b'>').map(|e| at + e + 1);
    let at = match (head_end, body_at) {
        (Some(h), _) => Some(h),
        (None, Some(b)) => Some(b),
        (None, None) => tag_at(body, 0, b"<html").and_then(|h| after(h, body)).or_else(|| {
            let d = body.windows(9).position(|w| w.eq_ignore_ascii_case(b"<!doctype"))?;
            after(d, body)
        }),
    };
    let Some(at) = at else { return };
    // The head's own tags of the keys said, taken away: those before `at`, past any `<script>`,
    // `<style>` or comment, whose text is not tags.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while let Some(lt) = memchr::memchr(b'<', &body[i..at]).map(|p| i + p) {
        let rest = &body[lt..at];
        if rest.starts_with(b"<!--") {
            i = find(body, lt + 4, at, b"-->").map_or(at, |e| e + 3);
            continue;
        }
        let name_len = rest[1..].iter().position(|&c| !c.is_ascii_alphanumeric()).map_or(rest.len() - 1, |n| n);
        if name_len == 0 {
            i = lt + 1;
            continue;
        }
        let name = String::from_utf8_lossy(&rest[1..1 + name_len]).to_ascii_lowercase();
        let close = match tag_close(body, lt, at) {
            Some(c) => c,
            None => break,
        };
        match name.as_str() {
            "script" | "style" | "textarea" | "noscript" | "template" => {
                let end = find_ci(body, close, at, format!("</{}", name).as_bytes()).and_then(|e| tag_close(body, e, at)).unwrap_or(at);
                i = end;
            }
            "title" => {
                let end = find_ci(body, close, at, b"</title").and_then(|e| tag_close(body, e, at)).unwrap_or(close);
                if head.replaces("title", &body[lt..close]) {
                    cuts.push((lt, end));
                }
                i = end;
            }
            n => {
                if head.replaces(n, &body[lt..close]) {
                    cuts.push((lt, close));
                }
                i = close;
            }
        }
    }
    let mut at = at;
    for &(from, to) in cuts.iter().rev() {
        body.drain(from..to);
        at -= to - from;
    }
    body.splice(at..at, tags.bytes());
}

/// Where the page's own `</head>` and `<body>` tags start, read tag by tag: a comment, and the
/// text of a `<script>`, `<style>`, `<textarea>`, `<title>`, `<noscript>` or `<template>`, are
/// passed over whole.
fn head_and_body(body: &[u8]) -> (Option<usize>, Option<usize>) {
    let (mut head, mut i) = (None, 0);
    let end = body.len();
    while let Some(lt) = memchr::memchr(b'<', &body[i..]).map(|p| i + p) {
        let rest = &body[lt..];
        if rest.starts_with(b"<!--") {
            i = find(body, lt + 4, end, b"-->").map_or(end, |e| e + 3);
            continue;
        }
        let close = rest.get(1) == Some(&b'/');
        let from = lt + 1 + close as usize;
        let n = body[from..].iter().position(|&c| !c.is_ascii_alphanumeric()).unwrap_or(end - from);
        let name = body[from..from + n].to_ascii_lowercase();
        if n == 0 {
            i = lt + 1;
            continue;
        }
        if close && name == b"head" && head.is_none() {
            head = Some(lt);
        }
        if !close && name == b"body" {
            return (head, Some(lt));
        }
        let Some(after) = tag_close(body, lt, end) else { return (head, None) };
        i = after;
        if !close && matches!(&name[..], b"script" | b"style" | b"textarea" | b"title" | b"noscript" | b"template") {
            let mut closing = b"</".to_vec();
            closing.extend_from_slice(&name);
            i = find_ci(body, after, end, &closing).unwrap_or(end);
        }
    }
    (head, None)
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

/// Just past the `>` that ends the tag at `lt`, quotes in it skipped.
fn tag_close(body: &[u8], lt: usize, limit: usize) -> Option<usize> {
    // A quote opens a value only after `=` (and spaces): `<a b'c>` ends at its `>`.
    let (mut q, mut eq) = (None, false);
    for (k, &c) in body[lt..limit].iter().enumerate() {
        match (q, c) {
            (Some(x), _) if c == x => q = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') if eq => q = Some(c),
            (None, b'>') => return Some(lt + k + 1),
            (None, b'=') => {
                eq = true;
                continue;
            }
            (None, _) if is_space(c) => continue,
            _ => {}
        }
        eq = false;
    }
    None
}

fn find(body: &[u8], from: usize, limit: usize, needle: &[u8]) -> Option<usize> {
    body[from..limit].windows(needle.len()).position(|w| w == needle).map(|p| from + p)
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
