//! Islands load their code when it is needed. Most components only react
//! to someone — a click, typing — so the browser downloads the page's
//! bundle when someone reaches for one (hovers, touches, focuses it). A
//! component that runs by itself must start with the page: this says which.

use crate::ast::*;

/// Elements the browser's HTML parser keeps only inside a table or a
/// select, out of which it moves any other element, `<quillo-island>`
/// included: a component whose root is one of these is its own island.
const TABLE_PARTS: &[&str] = &["tr", "td", "th", "thead", "tbody", "tfoot", "caption", "colgroup", "option", "optgroup"];

/// Where a component's island data goes in its own root element (just
/// after `<tr`), when its HTML is one element of `TABLE_PARTS`.
pub fn own_root(html: &str) -> Option<usize> {
    own_root_in(html, false)
}

/// The same, for a component rendered inside `<svg>` when `svg`: there its root, whatever it is
/// (`rect`, `g`, `a`, an `svg`), is its own island, as SVG draws only elements of its own, not a
/// `<quillo-island>`, as the browser makes it (FEEDBACK55.md, Q-763; branch feedback55).
pub fn own_root_in(html: &str, svg: bool) -> Option<usize> {
    let starts = |at: &str, tag: &str| at.strip_prefix(tag).is_some_and(|r| r.starts_with([' ', '>', '/', '\t', '\n']));
    let name = html.strip_prefix('<')?;
    let name = &name[..name.find([' ', '>', '/', '\t', '\n'])?];
    let tag = if svg { Some(name).filter(|n| n.starts_with(|c: char| c.is_ascii_alphabetic()))? } else { *TABLE_PARTS.iter().find(|t| **t == name)? };
    // One element, not several: its own end tag is the last thing.
    let mut depth = 0usize;
    for (i, _) in html.match_indices('<') {
        let at = &html[i + 1..];
        if let Some(r) = at.strip_prefix('/') {
            if starts(r, tag) {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return (html[i..].find('>')? + i + 1 == html.len()).then_some(tag.len() + 1);
                }
            }
        } else if starts(at, tag) {
            depth += 1;
        }
    }
    None
}

/// Whether `html`, a component's whole HTML, is nothing but parts of a table or a select (rows,
/// cells, options, ...) and space, or nothing at all: where such a component stands, inside a
/// table or a select, the browser's parser moves a `<quillo-island>` out, so when it is not one
/// element (`own_root`) it is marked by comments instead (FEEDBACK61.md, Q-892 and Q-936; branch
/// feedback61). Comments a component inside it was marked with may come between.
pub fn parts_only(html: &str) -> bool {
    let b = html.as_bytes();
    let (mut i, mut depth) = (0usize, 0usize);
    while i < b.len() {
        if b[i] != b'<' {
            if depth == 0 && !b[i].is_ascii_whitespace() {
                return false;
            }
            i += 1;
            continue;
        }
        let at = &html[i + 1..];
        if let Some(c) = at.strip_prefix("!--") {
            let Some(end) = c.find("-->") else { return false };
            i += 4 + end + 3;
            continue;
        }
        let close = at.starts_with('/');
        let name_at = if close { &at[1..] } else { at };
        let n = name_at.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(name_at.len());
        let name = name_at[..n].to_ascii_lowercase();
        let Some(end) = at.find('>') else { return false };
        let self_closing = at[..end].ends_with('/');
        if close {
            depth = match depth.checked_sub(1) {
                Some(d) => d,
                None => return false,
            };
        } else {
            if depth == 0 && name != "col" && !TABLE_PARTS.contains(&name.as_str()) {
                return false;
            }
            if !self_closing && !VOID.contains(&name.as_str()) {
                depth += 1;
            }
        }
        i += 1 + end + 1;
    }
    depth == 0
}

/// Elements with no end tag.
const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

/// A component's own root element from just after its tag name (see `own_root`), written to `out`
/// without the attributes of an island its markup gave it (`data-c={x}`, `data-e`; a spread's are
/// dropped already): the server's, written before them, are the island's, and one the server did
/// not write (`data-l`, `data-e`) was taken as the island's (FEEDBACK55.md, Q-700; branch
/// feedback55).
pub fn root_without_island_attrs(rest: &str, out: &mut String) {
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i] != b'>' {
        if !b[i].is_ascii_whitespace() {
            let s = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                i += 1;
            }
            out.push_str(&rest[s..i]);
            continue;
        }
        // ` name`, ` name="value"`: the value as written here, escaped, so its `"` ends it.
        let start = i;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let named = &rest[name..i];
        if b.get(i) == Some(&b'=') {
            i += 1;
            if b.get(i) == Some(&b'"') {
                i += 1 + rest[i + 1..].find('"').map_or(rest.len() - i - 1, |n| n + 1);
            } else {
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
            }
        }
        if named.is_empty() || !crate::interp::island_attr(&named.to_ascii_lowercase()) {
            out.push_str(&rest[start..i]);
        }
    }
    out.push_str(&rest[i..]);
}

/// Builtins that, called while a component renders, make it run without
/// anyone touching it: timers, live data, the page's own JavaScript.
const BY_ITSELF: &[&str] = &["every", "live", "listen", "js", "js_new"];

/// Whether a component's body (not its event handlers, which only run when
/// someone acts) starts timers or live data, runs JavaScript, or has an
/// element with `@mount`.
pub fn runs_by_itself(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) | Expr::Ident(..) => false,
        // Handlers and callbacks run when called, from something that woke
        // the component up already.
        Expr::Lambda(_) => false,
        Expr::Call(f, args, _) => {
            matches!(&**f, Expr::Ident(n, _) if BY_ITSELF.contains(&n.as_str())) || runs_by_itself(f) || args.iter().any(runs_by_itself)
        }
        Expr::Template(parts) => parts.iter().any(|p| matches!(p, TPart::Expr(e) if runs_by_itself(e))),
        Expr::List(es) => es.iter().any(runs_by_itself),
        Expr::Map(kvs) => kvs.iter().any(|(_, e)| runs_by_itself(e)),
        Expr::Unary(_, a, _) | Expr::Try(a) | Expr::Field(a, _, _) => runs_by_itself(a),
        Expr::Binary(_, a, b, _) | Expr::Coalesce(a, b) | Expr::Pipe(a, b, _) | Expr::Index(a, b, _) => runs_by_itself(a) || runs_by_itself(b),
        Expr::If(c, a, b) => runs_by_itself(c) || runs_by_itself(a) || b.as_deref().is_some_and(runs_by_itself),
        Expr::Match(x, arms, _) => runs_by_itself(x) || arms.iter().any(|a| a.guard.as_ref().is_some_and(runs_by_itself) || runs_by_itself(&a.body)),
        Expr::For(_, it, body, _) | Expr::While(it, body, _) => runs_by_itself(it) || runs_by_itself(body),
        Expr::Block(stmts) => stmts.iter().any(stmt_runs_by_itself),
        Expr::Reply(..) => false,
        Expr::Html(el, _) => el_runs_by_itself(el),
    }
}

fn stmt_runs_by_itself(s: &Stmt) -> bool {
    match s {
        Stmt::Let(_, e, _) | Stmt::Assign(_, _, e, _) | Stmt::Expr(e) | Stmt::State(_, e, _) | Stmt::Return(Some(e), _) => runs_by_itself(e),
        _ => false,
    }
}

fn el_runs_by_itself(el: &HtmlEl) -> bool {
    el.attrs.iter().any(|(name, v)| {
        name == "@mount" || (!name.starts_with('@') && matches!(v, AttrVal::Expr(e) | AttrVal::Spread(e) if runs_by_itself(e)))
    }) || el.id.as_ref().is_some_and(runs_by_itself)
        || el.classes.iter().any(runs_by_itself)
        || el.children.iter().any(|c| match c {
            HtmlChild::El(e) => el_runs_by_itself(e),
            HtmlChild::Expr(e) => runs_by_itself(e),
        })
}

#[cfg(test)]
mod tests {
    use super::own_root;

    #[test]
    fn parts_of_a_table_or_a_select_alone_are_marked_by_comments() {
        use super::parts_only;
        for yes in ["", "  \n", "<tr><td>a</td></tr><tr><td>b</td></tr>", "<option value=\"1\">1</option><option>2</option>", "<td>a</td>", "<!--q data-c=\"X\"--><tr><td>x</td></tr><!--/q--><tr></tr>", "<col><col span=\"2\">", "<tr><td><div>in <b>a</b></div></td></tr>", "<td><br></td>"] {
            assert!(parts_only(yes), "{:?}", yes);
        }
        for no in ["text", "<div></div>", "<tr></tr>text", "<tr></tr><div></div>", "<tr>", "</tr>", "<!--", "<tr><td>a</td></tr> b", "<span>a</span><tr></tr>"] {
            assert!(!parts_only(no), "{:?}", no);
        }
    }

    #[test]
    fn a_table_part_is_its_own_island_when_it_is_one_element() {
        assert_eq!(own_root("<tr><td>a</td></tr>"), Some(3));
        assert_eq!(own_root("<tr class=\"x\"><td><table><tr><td>in</td></tr></table></td></tr>"), Some(3));
        assert_eq!(own_root("<option value=\"1\">one</option>"), Some(7));
        assert_eq!(own_root("<thead><tr><th>h</th></tr></thead>"), Some(6));
        assert_eq!(own_root("<tr><td>a</td></tr><tr><td>b</td></tr>"), None);
        assert_eq!(own_root("<div><tr></tr></div>"), None);
        assert_eq!(own_root("<track>"), None);
        assert_eq!(own_root("text"), None);
    }
}
