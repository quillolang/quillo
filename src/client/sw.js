// Quillo's service worker, for a program with routes tagged `offline` (FEEDBACK67.md, B.2 and its
// reviews; branch pwa). Served at <base>/_quillo/sw.js, its scope the program's base. Its text
// names the build, the program and its offline routes (V): a deploy installs a new worker, which
// drops what the one before kept once it has kept the new pages (or tried five times); until then
// the old copies serve. Pages are kept on install as prefetches, which Quillo answers without
// changing anything, or refuses.
// - <base>/_quillo/{b,r,s,t,i}/<hash> hold what their hash names: from the cache first, kept for
//   good; only an answer the server marks immutable (one for a hash it no longer has is not).
// - No other /_quillo/ address is touched: rpc, live, events, ready, src/, quillo.js, this file.
// - A navigation goes to the network first. A 200 the server marks as an offline route's
//   (`x-quillo-offline`) is kept, at most 100 pages, the oldest dropped first; an answer without
//   the mark drops the copy kept of that address. When the network fails, or a proxy answers 502,
//   503 or 504, the kept copy is given, marked `data-kept` with when it was kept, else an offline
//   page. The marks go in an <html> start tag put before all of the page but its doctype and
//   comments, which the browser merges the page's own <html> into: nothing of the page is read or
//   changed.
// - A session's page (`x-quillo-offline: private`) is kept apart, by the session's name
//   (`x-quillo-session`), at most 50 a session. Given back, it is hidden behind the gate, which
//   shows it only to the session that browser holds now (the `quillo_tag` cookie) and otherwise
//   drops that session's pages and loads the page again. A session's pages are dropped when an
//   answer, or the page's runtime, names another, and a session dropped is never kept again.
// - Every other request goes to the network as if there were no worker.
const BASE = @BASE@, V = @V@, PAGES = @PAGES@, OFFLINE = @OFFLINE@, GATE = @GATE@, GATE_HASH = @GATE_HASH@;
const MINE = "quillo:" + BASE + ":", NAME = MINE + V, AWAY = NAME + ":", GONE = MINE + "gone";
const STATE = location.origin + BASE + "/_quillo/sw-state", DROPPED = location.origin + BASE + "/_quillo/sw-dropped";
const NAMED = BASE.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "/_quillo/[brsti]/[0-9a-f]+\\.(?:js|css|png)";
const ONE = new RegExp("^" + NAMED + "$"), ALL = new RegExp(NAMED, "g");
const lasting = (res) => res.ok && /immutable/.test(res.headers.get("cache-control") || "");
const offline = () => new Response(OFFLINE, { status: 503, headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" } });
// This version's pages kept on install: those done (kept, or answered so that they are not to
// be), how many tries, when the last was, and whether it is done (null: not read yet).
let st = null;

self.addEventListener("install", (e) => {
  self.skipWaiting();
  e.waitUntil(ahead());
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
  // A prefetch, which Quillo refuses with a 503 when it would change something: that says nothing
  // of the page.
  if (/prefetch/i.test(r.headers.get("sec-purpose") || "") || res.headers.get("x-quillo-prefetch") === "refused") return res;
  // An app down behind a proxy.
  if (res.status >= 502 && res.status <= 504) {
    const k = await kept(r.url);
    if (k) return k;
  }
  const who = res.headers.get("x-quillo-session"), mark = res.status === 200 && res.headers.get("x-quillo-offline");
  // A copy only of a page to keep: a download or an event stream is not held twice. Not kept now
  // (a 404, a redirect, a page that holds a form key now): the copy goes, after the session's
  // pages that go, whose caches it would open again.
  const said = who !== null || mark ? seen(mark ? res.clone() : null, r.url, who, mark) : Promise.resolve();
  e.waitUntil(mark ? said : said.then(() => forget(r.url)));
  e.waitUntil(settle());
  return res;
}

// What was kept of an address, in this version's caches and the old ones'; a cache dropped
// meanwhile is not made again.
async function forget(url) {
  for (const c of await mine()) if (await caches.has(c.k)) await (await caches.open(c.k)).delete(url);
}

async function named(e) {
  for (const c of await mine()) {
    if (c.who) continue;
    // By name, which makes no cache: one dropped meanwhile is not made again, empty.
    const hit = await caches.match(e.request, { cacheName: c.k, ignoreVary: true });
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
  // Written first, then taken from this version's other caches: never missing between.
  const name = who === null ? NAME : AWAY + who, c = await caches.open(name);
  await c.put(url, copy);
  for (const o of await mine()) if (o.v === V && o.k !== name && (await caches.has(o.k))) await (await caches.open(o.k)).delete(url);
  // The oldest pages first, past the bound; not the files they name.
  const pages = (await c.keys()).filter((k) => k.url !== STATE && !ONE.test(new URL(k.url).pathname));
  for (const k of pages.slice(0, Math.max(0, pages.length - (who === null ? 100 : 50)))) await c.delete(k);
}

async function kept(url) {
  const g = await dropped();
  for (const c of await mine()) {
    if (c.who && g.has(c.who)) continue;
    const hit = await caches.match(url, { cacheName: c.k, ignoreVary: true });
    if (!hit) continue;
    // A session's page whose kept policy would stop the gate is not given back.
    if (c.who && !gated(hit.headers)) continue;
    const html = await hit.text(), at = start(html);
    // A session's page: hidden, the gate first, before anything of theirs is read.
    const tag = "<html" + (c.who ? ' hidden data-kept-session="' + c.who + '"' : "") + ' data-kept="' + (hit.headers.get("x-quillo-kept") || "") + '">' + (c.who ? "<head><script>" + GATE + "</script>" : "");
    const h = new Headers(hit.headers);
    h.delete("content-length");
    return new Response(html.slice(0, at) + tag + html.slice(at), { status: 200, headers: h });
  }
  return null;
}

// Where a page's elements start: past a byte-order mark, white space, its doctype and comments,
// read as the browser's tokenizer reads them (the HTML standard's comment states: `<!-->` and
// `<!--->` end at once, `--!>` ends one as `-->` does, `<!...>` and `<?...>` are comments to the
// first `>`). What comes there is the browser's, as written: a conditional comment, `<html>` in a
// script's text. Not found in the first 4 KB, or a comment never closed: the very start, after
// the mark, where the gate still comes first (FEEDBACK67.md, B.2, third review; branch pwa).
function start(html) {
  const first = html.charCodeAt(0) === 0xfeff ? 1 : 0;
  let i = first;
  for (;;) {
    while (i < html.length && "\t\n\f\r ".includes(html[i])) i++;
    if (i > 4096) return first;
    if (html.startsWith("<!--", i)) {
      if (html[i + 4] === ">") { i += 5; continue; }
      if (html.startsWith("->", i + 4)) { i += 6; continue; }
      const a = html.indexOf("-->", i + 4), b = html.indexOf("--!>", i + 4);
      if (a < 0 && b < 0) return first;
      i = a >= 0 && (b < 0 || a < b) ? a + 3 : b + 4;
    } else if (html.startsWith("<!", i) || html.startsWith("<?", i)) {
      const end = html.indexOf(">", i + 2);
      if (end < 0) return first;
      i = end + 1;
    } else return i;
  }
}

// Whether the gate can run under a kept page's policy: no `sandbox` that refuses scripts, and
// where scripts are decided, inline ones let in or the gate's hash named. A page that cannot run
// it would show without it (its own sheet may undo `hidden`), so it is not given back.
function gated(h) {
  const policy = h.get("content-security-policy");
  if (!policy) return true;
  return policy.split(",").every((p) => {
    const ds = p.split(";").map((d) => d.trim().split(/\s+/)).filter((d) => d[0]);
    const named = (n) => ds.find((d) => d[0].toLowerCase() === n);
    const sandbox = named("sandbox");
    if (sandbox && !sandbox.slice(1).some((t) => t.toLowerCase() === "allow-scripts")) return false;
    const src = named("script-src-elem") || named("script-src") || named("default-src");
    if (!src) return true;
    const t = src.slice(1), low = t.map((x) => x.toLowerCase());
    if (t.includes(GATE_HASH)) return true;
    return low.includes("'unsafe-inline'") && !low.some((x) => /^'(nonce-|sha256-|sha384-|sha512-|strict-dynamic)/.test(x));
  });
}

async function state() {
  if (!st) {
    const hit = await (await caches.open(NAME)).match(STATE);
    st = hit ? await hit.json() : { done: [], tries: 0, at: 0, ready: false };
  }
  return st;
}

async function isReady() {
  return (await state()).ready;
}

async function dropOld() {
  for (const c of await mine()) if (c.v !== V) await caches.delete(c.k);
}

// This version's pages not all kept yet (a page did not come as the worker installed): tried again
// on a navigation that reached the network, 10 s after the first try, then 30 s, 90 s and 270 s;
// after five, the version is as kept as it gets, and the old copies go.
async function settle() {
  const s = await state();
  if (s.ready || Date.now() - s.at < 10000 * 3 ** (s.tries - 1)) return;
  if (await ahead()) await dropOld();
}

// Each offline route with no parameter, and the files its page names, so that a page never
// opened opens offline with its components: asked for as a prefetch (`x-quillo-prefetch`), which
// Quillo answers changing nothing, or refuses with a 503 that says so, as for a route that would
// write, or set a cookie. A page that did not come (no network, a proxy's 502, 503 or 504) is
// asked for again on the next try; one refused, redirected (here or off the site), or answered
// otherwise is done with for this version.
async function ahead() {
  const s = await state(), c = await caches.open(NAME);
  if (s.ready) return true;
  s.tries++;
  s.at = Date.now();
  // One at a time: a prefetch waits for anyone else's request (Quillo answers it 503 meanwhile,
  // which is tried again), the worker's own included.
  const todo = PAGES.filter((p) => !s.done.includes(p)), got = [];
  for (const p of todo) got.push(await (async () => {
    try {
      const res = await fetch(p, { cache: "no-store", redirect: "manual", headers: { "x-quillo-prefetch": "1" } });
      if (res.type === "opaqueredirect" || res.headers.get("x-quillo-prefetch") === "refused") return true;
      if (res.status >= 502 && res.status <= 504) return false;
      const mark = res.headers.get("x-quillo-offline");
      if (res.status !== 200 || !mark) return true;
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
  })());
  todo.forEach((p, i) => { if (got[i]) s.done.push(p); });
  s.ready = s.done.length >= PAGES.length || s.tries >= 5;
  await c.put(STATE, new Response(JSON.stringify(s), { headers: { "content-type": "application/json" } }));
  return s.ready;
}
