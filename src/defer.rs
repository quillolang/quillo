//! `state x = defer(load())`: a component's slow state, streamed after the page
//! (branch defer).
//!
//! On the server, while the page renders, a deferred state starts as `null` and its
//! component's island is marked as waiting (`data-qd`). The page is sent at once, up to
//! `</body>`; then each waiting component is rendered again, its other states as they
//! were and its deferred ones worked out, and sent in the same answer as a `<template>`
//! with a line of script that swaps it in. The browser does not take up a waiting
//! island before then. What search engines read is the whole page, as before.
//!
//! Only a component placed in the page (not one inside another component) defers; any
//! other `defer(x)` is `x`. Without a connection to stream to (a test, `Interp::handle`
//! called directly) or for a prefetch, the parts are worked out before the page is sent,
//! so the answer is whole. In the browser (a page made there, after `navigate`),
//! `defer(x)` is `x`: a state whose initializer calls the server is null until it answers.

use crate::value::{Closure, Map, Value};
use std::sync::Arc;

/// A waiting component: what renders it again.
pub(crate) struct Deferred {
    pub id: usize,
    pub c: Arc<Closure>,
    pub args: Vec<Option<Value>>,
    pub tag: String,
    pub props: Map,
    /// Its states as the page had them, but the deferred ones, and the live data it follows.
    pub state: Map,
    pub live: Map,
    pub line: u32,
    /// Placed inside `<svg>` (see `island::own_root_in`; FEEDBACK55.md, Q-763; branch feedback55).
    pub svg: bool,
}

/// A page whose waiting components follow it: its HTML up to `</body>`, what comes
/// after, and the components.
pub(crate) struct Page {
    pub first: Vec<u8>,
    pub tail: Vec<u8>,
    pub jobs: Vec<Deferred>,
    /// The page's content policy refuses an island's `style` attribute: its parts' islands are
    /// written without it, the page's `<style>` giving the rule (FEEDBACK59.md, Q-904; branch
    /// cache-and-migrate).
    pub plain_islands: bool,
    /// It refuses inline scripts: each part swaps itself in by the one line `ONE_SWAP`, hashed
    /// once in the policy, where a line naming each part made the policy grow with every part
    /// (found in review).
    pub one_swap: bool,
}

/// Swaps a waiting island for the one that came: defined once, before the first part, in the
/// object the runtime shares with the page's scripts (FEEDBACK42.md, Q-486). The document read
/// through `Document.prototype`, past what the page names `querySelector` (`<form
/// name="querySelector">` left the part waiting for good; the second review of slice W1,
/// FEEDBACK51.md, Q-667; branch feedback86-89).
pub const SWAP: &str = concat!(
    "<script>(function(w){w.swap=function(i){var d=document,q=Document.prototype.querySelector,t=q.call(d,'template[data-qt=\"'+i+'\"]'),",
    "o=q.call(d,'[data-qd=\"'+i+'\"]');if(!t)return;if(o){var n=t.content.firstElementChild;o.replaceWith(t.content);",
    "if(n&&w.resume&&(n.hasAttribute('data-e')||n.hasAttribute('data-l')))w.resume(n)}t.remove()}})",
    "(window[Symbol.for('quillo')]||(window[Symbol.for('quillo')]={}))</script>"
);

/// A waiting component's part: its island, and the line that swaps it in.
pub fn part(id: usize, island: &str) -> String {
    format!("<template data-qt=\"{}\">{}</template><script>window[Symbol.for(\"quillo\")].swap(\"{}\")</script>", id, island, id)
}

/// The line that swaps in the part it follows, whichever it is: one text for every part. Its own
/// script through `Document.prototype`, as `SWAP` reads the document.
pub const ONE_SWAP: &str = "window[Symbol.for(\"quillo\")].swap(Object.getOwnPropertyDescriptor(Document.prototype,\"currentScript\").get.call(document).previousElementSibling.getAttribute(\"data-qt\"))";

/// `part`, swapped in by `ONE_SWAP`.
pub fn part_one_swap(id: usize, island: &str) -> String {
    format!("<template data-qt=\"{}\">{}</template><script>{}</script>", id, island, ONE_SWAP)
}

/// The page split at its last `</body>` (or its end), the swap script at the end of
/// the first part.
pub(crate) fn split(body: Vec<u8>, jobs: Vec<Deferred>) -> Page {
    let at = body.windows(7).rposition(|w| w.eq_ignore_ascii_case(b"</body>")).unwrap_or(body.len());
    let mut first = body[..at].to_vec();
    first.extend_from_slice(SWAP.as_bytes());
    Page { first, tail: body[at..].to_vec(), jobs, plain_islands: false, one_swap: false }
}

/// Each waiting component rendered again: its part, or nothing when rendering it
/// failed (the error is logged and the placeholder stays).
pub(crate) fn resolve(it: &mut crate::interp::Interp, d: &Deferred, page: (bool, bool)) -> String {
    let (plain_islands, one_swap) = page;
    // Under the route's `with`, as the rest of the page was (branch feedback40-43).
    let r = it.under_route(|it| it.render_deferred(d));
    // A part sent after the page's head has gone out: what its `page_head` says otherwise than the
    // head went out with reaches the head in the browser alone, as its component runs there; said
    // once in the log (FEEDBACK84.md, Q-1320,
    // review of slice H; branch feedback82-85).
    if let Some(late) = it.page_head.take() {
        if late.differs(it.head_sent.as_deref()) {
            late_head(&d.tag);
        }
    }
    match r {
        Ok(html) => {
            let mut made = if one_swap { part_one_swap(d.id, &html) } else { part(d.id, &html) };
            // A part rendered where the page's place is lost (its own raw markup lost it, or the
            // page's did): an empty `<span>` after it, past its swap line (which `ONE_SWAP` finds
            // as its template's next element), ends SVG and MathML where the browser is, so that
            // the next part's template and line are read as HTML (coverage of 0.8.7, Q-1116;
            // branch coverage-087).
            if it.place_lost() {
                made.push_str(crate::interp::BACK_TO_HTML);
            }
            if !plain_islands {
                return made;
            }
            let mut b = made.into_bytes();
            let _ = crate::server::plain_islands(&mut b);
            String::from_utf8(b).unwrap_or_default()
        }
        Err(crate::interp::Ctrl::Error(e)) => {
            eprintln!("quillo: error while rendering the deferred part of <{}>: {}", d.tag, e);
            String::new()
        }
        Err(_) => String::new(),
    }
}

#[cold]
fn late_head(tag: &str) {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        eprintln!("quillo: <{}> waits (defer), so its page_head reaches the page's head in the browser only, once it comes: a crawler that runs no script sees the head without it", tag);
    });
}

/// The page whole: the first part, every waiting component's part, the rest.
pub(crate) fn whole(it: &mut crate::interp::Interp, page: Page) -> Vec<u8> {
    let mut out = page.first;
    for d in &page.jobs {
        out.extend_from_slice(resolve(it, d, (page.plain_islands, page.one_swap)).as_bytes());
    }
    out.extend_from_slice(&page.tail);
    out
}
