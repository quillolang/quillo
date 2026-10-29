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
}

/// A page whose waiting components follow it: its HTML up to `</body>`, what comes
/// after, and the components.
pub(crate) struct Page {
    pub first: Vec<u8>,
    pub tail: Vec<u8>,
    pub jobs: Vec<Deferred>,
}

/// Swaps a waiting island for the one that came: defined once, before the first part.
pub const SWAP: &str = concat!(
    "<script>window.__quillo_swap=function(i){var d=document,t=d.querySelector('template[data-qt=\"'+i+'\"]'),",
    "o=d.querySelector('[data-qd=\"'+i+'\"]');if(!t)return;if(o){var n=t.content.firstElementChild;o.replaceWith(t.content);",
    "if(n&&window.__quillo_resume&&(n.hasAttribute('data-e')||n.hasAttribute('data-l')))window.__quillo_resume(n)}t.remove()}</script>"
);

/// A waiting component's part: its island, and the line that swaps it in.
pub fn part(id: usize, island: &str) -> String {
    format!("<template data-qt=\"{}\">{}</template><script>__quillo_swap(\"{}\")</script>", id, island, id)
}

/// The page split at its last `</body>` (or its end), the swap script at the end of
/// the first part.
pub(crate) fn split(body: Vec<u8>, jobs: Vec<Deferred>) -> Page {
    let at = body.windows(7).rposition(|w| w.eq_ignore_ascii_case(b"</body>")).unwrap_or(body.len());
    let mut first = body[..at].to_vec();
    first.extend_from_slice(SWAP.as_bytes());
    Page { first, tail: body[at..].to_vec(), jobs }
}

/// Each waiting component rendered again: its part, or nothing when rendering it
/// failed (the error is logged and the placeholder stays).
pub(crate) fn resolve(it: &mut crate::interp::Interp, d: &Deferred) -> String {
    match it.render_deferred(d) {
        Ok(html) => part(d.id, &html),
        Err(crate::interp::Ctrl::Error(e)) => {
            eprintln!("quillo: error while rendering the deferred part of <{}>: {}", d.tag, e);
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// The page whole: the first part, every waiting component's part, the rest.
pub(crate) fn whole(it: &mut crate::interp::Interp, page: Page) -> Vec<u8> {
    let mut out = page.first;
    for d in &page.jobs {
        out.extend_from_slice(resolve(it, d).as_bytes());
    }
    out.extend_from_slice(&page.tail);
    out
}
