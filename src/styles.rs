//! Components' and views' own CSS (FEEDBACK8.md, Q-067 and Q-068; branch
//! feedback8-fixes).
//!
//! `style css`...`` in a component's or view's body is its CSS. The parser
//! gives each element the body writes an attribute of the component's own,
//! `data-q-<id>` (`ast::style_id`), so the server's HTML and the browser's
//! carry it alike, and here its rules are made to need it: `.title` is
//! `.title[data-q-3f9a0c12]`, so two components that style `.title` style
//! only their own. `:global(...)` keeps a selector, or the part of one, as
//! it is.
//!
//! A page is sent the CSS of the components and views on it, and of those
//! its bundle can show, as a sheet of its own, `/_quillo/s/<hash>.css`,
//! which the browser keeps for good (its address changes with it).

use crate::ast::{style_id, style_of, Expr};
use crate::error::QuilloError;
use crate::interp::{Ctrl, Interp};
use crate::value::{scope_vars, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Sheets kept, one per set of styles pages were made of.
const KEPT: usize = 256;

#[derive(Default)]
pub struct Styles {
    /// The scoped CSS of each component and view with a style, by its id.
    by_id: BTreeMap<String, String>,
    sheets: HashMap<String, Sheet>,
    tick: u64,
}

struct Sheet {
    hash: String,
    css: Arc<String>,
    used: u64,
}

impl Styles {
    /// Every component's and view's style, taken when the program loads.
    pub fn collect(it: &mut Interp) -> Result<Styles, QuilloError> {
        let mut by_id = BTreeMap::new();
        let env = it.globals.clone();
        for (_, v) in scope_vars(&it.globals) {
            let Value::Func(f) = v else { continue };
            let Expr::Block(stmts) = &f.def.body else { continue };
            let id = style_id(f.def.file.as_deref(), &f.def.name);
            for e in stmts.iter().filter_map(style_of) {
                let css = match it.eval(e, &env) {
                    Ok(Value::Str(s)) => s.to_string(),
                    Ok(other) => {
                        return Err(QuilloError::new(
                            format!("`style` in `{}` takes CSS text (css`...`), got {}", f.def.name, other.type_name()),
                            f.def.line,
                        ))
                    }
                    Err(Ctrl::Error(e)) => return Err(e),
                    Err(_) => return Err(QuilloError::new(format!("`style` in `{}` must be CSS text", f.def.name), f.def.line)),
                };
                let scoped: &mut String = by_id.entry(id.clone()).or_default();
                scoped.push_str(&scope(&css, &id));
            }
        }
        Ok(Styles { by_id, ..Default::default() })
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn ids(&self) -> Vec<String> {
        self.by_id.keys().cloned().collect()
    }

    /// The sheet of these styles (ids without one are left out): its hash,
    /// or none when none of them has a style.
    pub fn sheet_for(&mut self, ids: &[String]) -> Option<String> {
        let mut ids: Vec<&String> = ids.iter().filter(|i| self.by_id.contains_key(*i)).collect();
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            return None;
        }
        let key = ids.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        self.tick += 1;
        if let Some(s) = self.sheets.get_mut(&key) {
            s.used = self.tick;
            return Some(s.hash.clone());
        }
        let css: String = ids.iter().map(|i| self.by_id[*i].as_str()).collect();
        let hash = crate::client::hash_text(&css);
        self.sheets.insert(key, Sheet { hash: hash.clone(), css: Arc::new(css), used: self.tick });
        while self.sheets.len() > KEPT {
            let Some(old) = self.sheets.iter().min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone()) else { break };
            self.sheets.remove(&old);
        }
        Some(hash)
    }

    pub fn by_hash(&self, hash: &str) -> Option<Arc<String>> {
        self.sheets.values().find(|s| s.hash == hash).map(|s| s.css.clone())
    }

    /// Every style: for a page that names a sheet no longer kept.
    pub fn all(&self) -> String {
        self.by_id.values().map(|s| s.as_str()).collect()
    }
}

/// The ids of the styles whose elements are in `html` (`data-q-<id>`).
pub fn ids_in(html: &[u8]) -> Vec<String> {
    const MARK: &[u8] = b"data-q-";
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(at) = html[i..].windows(MARK.len()).position(|w| w == MARK) {
        let start = i + at + MARK.len();
        let id: Vec<u8> = html[start..].iter().take(8).copied().collect();
        if id.len() == 8 && id.iter().all(u8::is_ascii_hexdigit) {
            let id = String::from_utf8(id).unwrap_or_default();
            if !out.contains(&id) {
                out.push(id);
            }
        }
        i = start;
    }
    out
}

/// `css` with each rule's selectors made to need `[data-q-<id>]`, in the
/// blocks of `@media`, `@supports`, `@container` and `@layer` too;
/// `@keyframes`, `@font-face` and the like as they are.
pub fn scope(css: &str, id: &str) -> String {
    let attr = format!("[data-q-{}]", id);
    let src = without_comments(css);
    let mut out = String::new();
    scope_block(&src, &attr, &mut out);
    out
}

fn scope_block(src: &str, attr: &str, out: &mut String) {
    let mut i = 0;
    while i < src.len() {
        let (end, found) = scan_to(src, i, &['{', ';']);
        let prelude = src[i..end].trim();
        if found != Some('{') {
            if !prelude.is_empty() {
                out.push_str(prelude);
                out.push_str(";\n");
            }
            i = end + 1;
            continue;
        }
        let close = closing(src, end);
        let inner = &src[end + 1..close.min(src.len())];
        if let Some(at) = prelude.strip_prefix('@') {
            let name: String = at.chars().take_while(|c| c.is_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
            out.push_str(prelude);
            if matches!(name.as_str(), "media" | "supports" | "container" | "layer" | "scope" | "document") {
                out.push_str(" {\n");
                scope_block(inner, attr, out);
                out.push_str("}\n");
            } else {
                out.push_str(" {");
                out.push_str(inner);
                out.push_str("}\n");
            }
        } else {
            let sels: Vec<String> = split_top(prelude, ',').iter().map(|s| scope_selector(s.trim(), attr)).collect();
            out.push_str(&sels.join(", "));
            out.push_str(" {");
            out.push_str(inner);
            out.push_str("}\n");
        }
        i = close + 1;
    }
}

/// One selector scoped: its last compound needs the attribute, before any
/// pseudo-class or pseudo-element (`.a:hover` is `.a[data-q-…]:hover`).
/// `:global(x)` is `x`, and what follows it is not scoped.
fn scope_selector(sel: &str, attr: &str) -> String {
    if sel.is_empty() {
        return String::new();
    }
    if let Some(g) = sel.find(":global(") {
        let before = sel[..g].trim_end();
        let gap = &sel[before.len()..g];
        let rest = unwrap_globals(&sel[g..]);
        return if before.is_empty() { rest } else { format!("{}{}{}", scope_last(before, attr), gap, rest) };
    }
    scope_last(sel, attr)
}

fn unwrap_globals(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(g) = rest.find(":global(") {
        out.push_str(&rest[..g]);
        let open = g + ":global".len();
        let close = closing_paren(rest, open);
        out.push_str(&rest[open + 1..close.min(rest.len())]);
        rest = if close < rest.len() { &rest[close + 1..] } else { "" };
    }
    out.push_str(rest);
    out
}

fn scope_last(sel: &str, attr: &str) -> String {
    // Where the last compound starts: after the last combinator outside
    // brackets, parentheses and quotes.
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (k, c) in sel.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ' ' | '\t' | '\n' | '>' | '+' | '~') if depth == 0 => start = k + c.len_utf8(),
            _ => {}
        }
    }
    let compound = &sel[start..];
    // Before its first pseudo-class or pseudo-element.
    let (mut depth, mut quote, mut at) = (0i32, None, compound.len());
    for (k, c) in compound.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ':') if depth == 0 => {
                at = k;
                break;
            }
            _ => {}
        }
    }
    format!("{}{}{}{}", &sel[..start], &compound[..at], attr, &compound[at..])
}

fn without_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = rest[at + 2..].find("*/").map_or("", |e| &rest[at + 2 + e + 2..]);
    }
    out.push_str(rest);
    out
}

/// The first of `stops` at depth 0 from `i` (outside quotes, parentheses
/// and brackets), or the end.
fn scan_to(src: &str, i: usize, stops: &[char]) -> (usize, Option<char>) {
    let (mut depth, mut quote) = (0i32, None);
    for (k, c) in src[i..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if depth == 0 && stops.contains(&c) => return (i + k, Some(c)),
            _ => {}
        }
    }
    (src.len(), None)
}

/// The `}` that closes the `{` at `open`.
fn closing(src: &str, open: usize) -> usize {
    let (mut depth, mut quote) = (0i32, None);
    for (k, c) in src[open..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth -= 1;
                if depth == 0 {
                    return open + k;
                }
            }
            _ => {}
        }
    }
    src.len()
}

fn closing_paren(src: &str, open: usize) -> usize {
    let mut depth = 0;
    for (k, c) in src[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return open + k;
                }
            }
            _ => {}
        }
    }
    src.len()
}

fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let (end, found) = scan_to(s, i, &[sep]);
        out.push(&s[i..end]);
        if found.is_none() {
            break;
        }
        i = end + 1;
    }
    out
}

/// The classes Quillo sets itself, which a program styles without
/// writing (FEEDBACK8.md, Q-066): `ask`'s and `notify`'s (`q-`, and the
/// places `data-notices` names, `dialog-...` and `toasts-...`, which a
/// sheet styles for whichever a page picks) and `highlight`'s (`hl-`).
pub fn set_by_quillo(class: &str) -> bool {
    ["q-", "hl-", "dialog-", "toasts-"].iter().any(|p| class.starts_with(p))
}

/// The classes `css`'s selectors style, each with the lines before it; not
/// those in `:global(...)`, which are the page's (FEEDBACK8.md, Q-065;
/// branch feedback8-fixes).
pub fn classes_in(css: &str) -> Vec<(String, u32)> {
    let src = blank_comments(css);
    let mut at = Vec::new();
    classes_in_block(&src, 0, &mut at);
    at.into_iter().map(|(name, k)| (name, src[..k].matches('\n').count() as u32)).collect()
}

fn classes_in_block(src: &str, base: usize, out: &mut Vec<(String, usize)>) {
    let mut i = 0;
    while i < src.len() {
        let (end, found) = scan_to(src, i, &['{', ';']);
        if found != Some('{') {
            i = end + 1;
            continue;
        }
        let close = closing(src, end);
        let prelude = &src[i..end];
        let inner = &src[end + 1..close.min(src.len())];
        if let Some(at) = prelude.trim_start().strip_prefix('@') {
            let name: String = at.chars().take_while(|c| c.is_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
            if matches!(name.as_str(), "media" | "supports" | "container" | "layer" | "scope" | "document") {
                classes_in_block(inner, base + end + 1, out);
            }
        } else {
            let mut globals: Vec<(usize, usize)> = Vec::new();
            let mut from = 0;
            while let Some(g) = prelude[from..].find(":global(") {
                let open = from + g + ":global".len();
                let close = closing_paren(prelude, open);
                globals.push((open, close));
                from = close.min(prelude.len());
            }
            let (mut depth, mut quote) = (0i32, None);
            for (k, c) in prelude.char_indices() {
                match (quote, c) {
                    (Some(q), _) if c == q => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(c),
                    (None, '[') => depth += 1,
                    (None, ']') => depth -= 1,
                    (None, '.') if depth == 0 && !globals.iter().any(|(a, z)| k > *a && k < *z) => {
                        let name: String = prelude[k + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
                        let starts = name.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_' || c == '-');
                        // `.sm\:p-2`: a name with escapes, which this does not read.
                        let escaped = prelude[k + 1 + name.len()..].starts_with('\\');
                        if starts && !escaped {
                            out.push((name, base + i + k));
                        }
                    }
                    _ => {}
                }
            }
        }
        i = close + 1;
    }
}

/// The starts of classes a sheet says scripts outside the program set,
/// in a comment: `/* set elsewhere: leaflet-, mapbox- */`.
pub fn set_elsewhere(css: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        let end = rest[at..].find("*/").map_or(rest.len(), |e| at + e);
        let comment = &rest[at + 2..end];
        if let Some(list) = comment.trim().strip_prefix("set elsewhere:") {
            out.extend(list.split(',').map(|s| s.trim().trim_start_matches('.').to_string()).filter(|s| !s.is_empty()));
        }
        rest = &rest[(end + 2).min(rest.len())..];
    }
    out
}

/// Comments as spaces, their lines kept.
fn blank_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        let end = rest[at..].find("*/").map_or(rest.len(), |e| at + e + 2);
        out.extend(rest[at..end].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_need_the_components_attribute() {
        let s = |css: &str| scope(css, "ab12cd34").split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(s(".title { color: red }"), ".title[data-q-ab12cd34] { color: red }");
        assert_eq!(s(".a .b, .c > .d:hover { x: 1 }"), ".a .b[data-q-ab12cd34], .c > .d[data-q-ab12cd34]:hover { x: 1 }");
        assert_eq!(s("li:nth-child(2n+1)::before { content: \"a, b {\" }"), "li[data-q-ab12cd34]:nth-child(2n+1)::before { content: \"a, b {\" }");
        assert_eq!(s(":hover { x: 1 }"), "[data-q-ab12cd34]:hover { x: 1 }");
        assert_eq!(s("@media (max-width: 600px) { .a { x: 1 } } /* note */"), "@media (max-width: 600px) { .a[data-q-ab12cd34] { x: 1 } }");
        assert_eq!(s("@keyframes spin { from { a: 0 } to { a: 1 } }"), "@keyframes spin { from { a: 0 } to { a: 1 } }");
        assert_eq!(s(":global(body.dark) .a { x: 1 }"), "body.dark .a { x: 1 }");
        assert_eq!(s(".card :global(.leaflet-pane) { x: 1 }"), ".card[data-q-ab12cd34] .leaflet-pane { x: 1 }");
        assert_eq!(s("@import url(\"x.css\");"), "@import url(\"x.css\");");
    }

    #[test]
    fn the_classes_a_sheet_styles() {
        let css = ".a, .b:hover { x: 1 }\n/* .c */\n@media (x) {\n  .d .e[title=\".f\"] { y: 1.5em }\n}\n:global(.g) .h { }";
        assert_eq!(classes_in(css), vec![("a".into(), 0), ("b".into(), 0), ("d".into(), 3), ("e".into(), 3), ("h".into(), 5)]);
        assert_eq!(set_elsewhere("/* set elsewhere: leaflet-, .mapbox */ .x {}"), vec!["leaflet-", "mapbox"]);
    }

    #[test]
    fn a_page_says_which_styles_it_shows() {
        assert_eq!(ids_in(b"<p data-q-ab12cd34 class=x><b data-q-ab12cd34 data-q-00ff00ff>"), vec!["ab12cd34", "00ff00ff"]);
    }
}
