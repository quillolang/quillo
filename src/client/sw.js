// Quillo's service worker, for a program with routes tagged `offline` (FEEDBACK67.md, B.2 and its
// review; branch pwa). Served at <base>/_quillo/sw.js, its scope the program's base. Its text
// names the build, the program and its offline routes (V): a deploy installs a new worker, which
// drops what the one before kept once it has kept the new pages; until then the old copies serve.
// - <base>/_quillo/{b,r,s,t,i}/<hash> hold what their hash names: from the cache first, kept for
//   good; only an answer the server marks immutable (one for a hash it no longer has is not).
// - No other /_quillo/ address is touched: rpc, live, events, ready, src/, quillo.js, this file.
// - A navigation goes to the network first. A 200 the server marks as an offline route's
//   (`x-quillo-offline`) is kept, at most 100 pages, the oldest dropped first; when the network
//   fails, or a proxy answers 502, 503 or 504, the kept copy is given, its <html> marked
//   `data-kept` with when it was kept, else an offline page.
// - A session's page (`x-quillo-offline: private`) is kept apart, by the session's name
//   (`x-quillo-session`), at most 50 a session. Given back, it is hidden behind the gate, which
//   shows it only to the session that browser holds now (the `quillo_tag` cookie) and otherwise
//   drops that session's pages and loads the page again. A session's pages are dropped when an
//   answer, or the page's runtime, names another, and a session dropped is never kept again.
// - Every other request goes to the network as if there were no worker.
const BASE = @BASE@, V = @V@, PAGES = @PAGES@, OFFLINE = @OFFLINE@, GATE = @GATE@;
const MINE = "quillo:" + BASE + ":", NAME = MINE + V, AWAY = NAME + ":", GONE = MINE + "gone";
const READY = location.origin + BASE + "/_quillo/sw-ready", DROPPED = location.origin + BASE + "/_quillo/sw-dropped";
const NAMED = BASE.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "/_quillo/[brsti]/[0-9a-f]+\\.(?:js|css|png)";
const ONE = new RegExp("^" + NAMED + "$"), ALL = new RegExp(NAMED, "g");
const lasting = (res) => res.ok && /immutable/.test(res.headers.get("cache-control") || "");
const offline = () => new Response(OFFLINE, { status: 503, headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" } });
// Whether this version's pages are all kept (null: not looked yet), and when that was last tried.
let ready = null, tried = 0;

self.addEventListener("install", (e) => {
  self.skipWaiting();
  e.waitUntil(ahead().then((ok) => { ready = ok; tried = Date.now(); }));
});

self.addEventListener("activate", (e) => e.waitUntil((async () => {
  if (await isReady()) await dropOld();
  if (self.registration.navigationPreload) await self.registration.navigationPreload.enable();
  await self.clients.claim();
})()));

self.addEventListener("fetch", (e) => {
  const r = e.request, u = new URL(r.url);
  if (u.origin !== location.origin || (u.pathname !== BASE && !u.pathname.startsWith(BASE + "/"))) return;
  if (u.pathname.startsWith(BASE + "/_quillo/")) {
    if (r.method === "GET" && !u.search && ONE.test(u.pathname)) e.respondWith(named(e));
    return;
  }
  if (r.mode !== "navigate") return;
  // A form sent by a navigation is only read for the session it leaves, as a sign-out answered
  // with a page says so there.
  e.respondWith(r.method === "GET" ? page(e) : fetch(r).then((res) => (e.waitUntil(seen(null, null, res.headers.get("x-quillo-session"), null)), res)));
});

// The page's runtime says which session the browser holds; the gate, a session to drop.
self.addEventListener("message", (e) => {
  const d = e.data || {};
  if (typeof d.who !== "string") return;
  if (d.quillo === "session") e.waitUntil(current(d.who, true));
  else if (d.quillo === "drop") e.waitUntil(drop(d.who).then(() => e.ports[0] && e.ports[0].postMessage(1)));
});

async function page(e) {
  const r = e.request;
  let res;
  try {
    res = (await e.preloadResponse) || (await fetch(r));
  } catch (_) {
    return (await kept(r.url)) || offline();
  }
  // An app down behind a proxy; not a prefetch, which Quillo refuses with a 503.
  if (res.status >= 502 && res.status <= 504 && !/prefetch/i.test(r.headers.get("sec-purpose") || "")) {
    const k = await kept(r.url);
    if (k) return k;
  }
  const who = res.headers.get("x-quillo-session"), mark = res.status === 200 && res.headers.get("x-quillo-offline");
  // A copy only of a page to keep: a download or an event stream is not held twice.
  if (who !== null || mark) e.waitUntil(seen(mark ? res.clone() : null, r.url, who, mark));
  if (ready !== true) e.waitUntil(settle());
  return res;
}

async function named(e) {
  for (const c of await mine()) {
    if (c.who) continue;
    const hit = await (await caches.open(c.k)).match(e.request, { ignoreVary: true });
    if (hit) return hit;
  }
  const res = await fetch(e.request);
  if (lasting(res)) {
    const copy = res.clone();
    e.waitUntil(caches.open(NAME).then((c) => c.put(e.request, copy)));
  }
  return res;
}

// This worker's caches, this version's first: each one's name, version and session ("" for the
// pages of no one's and the files).
async function mine() {
  const out = [];
  for (const k of await caches.keys()) {
    if (!k.startsWith(MINE) || k === GONE) continue;
    const [v, who = ""] = k.slice(MINE.length).split(":");
    out.push({ k, v, who });
  }
  return out.sort((a, b) => (b.v === V) - (a.v === V));
}

// The sessions dropped, kept across the worker's restarts: a late answer of one is not kept.
let gone = null;
async function dropped() {
  if (!gone) {
    const hit = await (await caches.open(GONE)).match(DROPPED);
    gone = new Set(hit ? await hit.json() : []);
  }
  return gone;
}
async function save(g) {
  const last = [...g].slice(-200);
  await (await caches.open(GONE)).put(DROPPED, new Response(JSON.stringify(last), { headers: { "content-type": "application/json" } }));
}

// The browser holds session `who` ("-": none): the pages of every other are dropped. Said by the
// page (`sure`), it is the session even if it was dropped before; said by an answer, one dropped
// stays so, and the answer is not taken.
async function current(who, sure) {
  const g = await dropped();
  let changed = false;
  if (g.has(who)) {
    if (!sure) return false;
    g.delete(who);
    changed = true;
  }
  for (const c of await mine()) {
    if (!c.who || c.who === who) continue;
    await caches.delete(c.k);
    if (c.who !== "-") { g.add(c.who); changed = true; }
  }
  if (changed) await save(g);
  return true;
}

async function drop(who) {
  const g = await dropped();
  for (const c of await mine()) if (c.who === who) await caches.delete(c.k);
  if (who !== "-") { g.add(who); await save(g); }
}

// What an answer says: the session it leaves, and a page to keep.
async function seen(res, url, who, mark) {
  if (who !== null && !(await current(who, false))) return;
  if (!res || (mark === "private" && !who)) return;
  await keep(url, res, mark === "private" ? who : null);
}

async function keep(url, res, who) {
  const h = new Headers(res.headers);
  h.delete("clear-site-data");
  h.set("x-quillo-kept", new Date().toISOString());
  const copy = new Response(await res.blob(), { status: 200, statusText: res.statusText, headers: h });
  for (const c of await mine()) if (c.v === V) await (await caches.open(c.k)).delete(url);
  const c = await caches.open(who === null ? NAME : AWAY + who);
  await c.put(url, copy);
  // The oldest pages first, past the bound; not the files they name.
  const pages = (await c.keys()).filter((k) => k.url !== READY && !ONE.test(new URL(k.url).pathname));
  for (const k of pages.slice(0, Math.max(0, pages.length - (who === null ? 100 : 50)))) await c.delete(k);
}

async function kept(url) {
  const g = await dropped();
  for (const c of await mine()) {
    if (c.who && g.has(c.who)) continue;
    const hit = await (await caches.open(c.k)).match(url, { ignoreVary: true });
    if (!hit) continue;
    const mark = ' data-kept="' + (hit.headers.get("x-quillo-kept") || "") + '"' + (c.who ? ' data-kept-session="' + c.who + '" hidden' : "");
    let html = await hit.text(), at;
    const m = /<html(?=[\s/>])/i.exec(html);
    if (m) {
      at = m.index + 5;
      html = html.slice(0, at) + mark + html.slice(at);
    } else {
      const d = /^\s*<!doctype[^>]*>/i.exec(html), i = d ? d[0].length : 0;
      html = html.slice(0, i) + "<html" + mark + ">" + html.slice(i);
      at = i + 5;
    }
    // A session's page: the gate first in its head, before anything of theirs is read.
    if (c.who) {
      const end = html.indexOf(">", at) + 1, head = /<head(?=[\s/>])[^>]*>/i.exec(html.slice(end));
      const i = head ? end + head.index + head[0].length : end;
      html = html.slice(0, i) + "<script>" + GATE + "</script>" + html.slice(i);
    }
    const h = new Headers(hit.headers);
    h.delete("content-length");
    return new Response(html, { status: 200, headers: h });
  }
  return null;
}

async function isReady() {
  if (ready === null) ready = !!(await (await caches.open(NAME)).match(READY));
  return ready;
}

async function dropOld() {
  for (const c of await mine()) if (c.v !== V) await caches.delete(c.k);
}

// This version's pages not all kept yet (a page failed as the worker installed): tried again on a
// navigation that reached the network, at most every 10 s; the old copies go once they are.
async function settle() {
  if ((await isReady()) || Date.now() - tried < 10000) return;
  tried = Date.now();
  if (await ahead()) {
    ready = true;
    await dropOld();
  }
}

// Each offline route with no parameter, and the files its page names, so that a page never
// opened opens offline with its components. Whether all were answered: a page that did not come
// (no network, or a proxy's 502, 503 or 504) is tried again later; one answered otherwise is as
// the server said.
async function ahead() {
  const c = await caches.open(NAME);
  const got = await Promise.all(PAGES.map(async (p) => {
    try {
      const res = await fetch(p, { cache: "no-store" });
      if (res.status >= 502 && res.status <= 504) return false;
      const mark = res.headers.get("x-quillo-offline");
      if (res.status !== 200 || res.redirected || !mark) return true;
      const html = await res.clone().text();
      await seen(res, new URL(p, location.href).href, res.headers.get("x-quillo-session"), mark);
      await Promise.all([...new Set(html.match(ALL) || [])].map(async (f) => {
        if (await c.match(f)) return;
        const file = await fetch(f);
        if (lasting(file)) await c.put(f, file);
      }));
      return true;
    } catch (_) {
      return false;
    }
  }));
  const ok = got.every(Boolean);
  if (ok) await c.put(READY, new Response("1"));
  return ok;
}
