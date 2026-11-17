// A small emulation of a browser around Quillo's served worker: CacheStorage, a cookie jar,
// navigations (redirects opaque, as a browser's navigation fetch with redirect: manual gives),
// and a switch for the network. Not Chrome: enough to run the worker's own logic. From the second
// review of FEEDBACK67.md's B.2 (branch pwa), with the worker's own fetches' headers and
// `redirect: "manual"` taken as a browser takes them.
import vm from "node:vm";

export async function makeBrowser(origin) {
  let src = await (await fetch(origin + "/_quillo/sw.js")).text();
  // A clock the test sets (`browser.time`), for the worker's waits; none, the real one.
  let time = null;
  const FakeDate = class extends Date { static now() { return time ?? Date.now(); } };
  const jar = new Map();
  let online = true;
  const store = new Map(); // name -> Map(url -> {body, status, headers})
  const toUrl = (r) => new URL(typeof r === "string" ? r : r.url, origin).href;
  const cacheOf = (m) => ({
    async put(r, res) {
      const body = await res.arrayBuffer();
      m.set(toUrl(r), { body, status: res.status, headers: [...res.headers] });
    },
    async match(r) {
      const v = m.get(toUrl(r));
      return v ? new Response(v.body.slice(0), { status: v.status, headers: v.headers }) : undefined;
    },
    async delete(r) { return m.delete(toUrl(r)); },
    async keys() { return [...m.keys()].map((u) => ({ url: u })); },
  });
  const caches = {
    async keys() { return [...store.keys()]; },
    async open(n) { if (!store.has(n)) store.set(n, new Map()); return cacheOf(store.get(n)); },
    async has(n) { return store.has(n); },
    // In one cache by its name (`cacheName`), or in each; never makes one.
    async match(r, opts = {}) {
      for (const [n, m] of store) if (!opts.cacheName || opts.cacheName === n) { const hit = await cacheOf(m).match(r); if (hit) return hit; }
      return undefined;
    },
    async delete(n) { return store.delete(n); },
  };
  const cookieHeader = () => [...jar].map(([k, v]) => k + "=" + v).join("; ");
  const takeCookies = (res) => {
    for (const c of res.headers.getSetCookie()) {
      const [pair, ...attrs] = c.split(";");
      const i = pair.indexOf("="), k = pair.slice(0, i).trim(), v = pair.slice(i + 1).trim();
      if (attrs.some((a) => /max-age=0/i.test(a.trim())) || v === "") jar.delete(k); else jar.set(k, v);
    }
  };
  async function net(r, init = {}) {
    if (!online) throw new TypeError("Failed to fetch");
    const url = toUrl(r), method = (typeof r === "object" && r.method) || init.method || "GET";
    const nav = typeof r === "object" && r.mode === "navigate";
    const manual = nav || init.redirect === "manual";
    const own = typeof r === "object" && r.headers ? Object.fromEntries(new Headers(r.headers)) : {};
    const headers = { cookie: cookieHeader(), ...own, ...(init.headers || {}) };
    // The worker's own prefetch of a page answered as a busy Quillo answers it, the first
    // `browser.busy.times` times (`browser.busy = { path, times }`); each is counted in `asked`.
    if (browser.busy && url.endsWith(browser.busy.path) && headers["x-quillo-prefetch"] === "1") {
      browser.busy.asked = (browser.busy.asked || 0) + 1;
      if (browser.busy.times > 0) {
        browser.busy.times--;
        return new Response(null, { status: 503, headers: { "cache-control": "no-store", ...(browser.busy.plain ? {} : { "x-quillo-prefetch": "busy" }) } });
      }
    }
    // A request sent now whose answer comes later (`browser.hold`).
    const res = await fetch(url, { method, headers, body: r.body, redirect: "manual" });
    if (browser.hold && url.endsWith(browser.hold.path)) await browser.hold.gate;
    takeCookies(res);
    if (res.status >= 300 && res.status < 400) {
      if (manual) {
        // A browser hands the worker an opaque redirect: no status, no headers.
        const o = new Response(null, { status: 200 });
        Object.defineProperty(o, "status", { value: 0 });
        Object.defineProperty(o, "headers", { value: new Headers() });
        Object.defineProperty(o, "type", { value: "opaqueredirect" });
        o.location = new URL(res.headers.get("location"), url).href;
        return o;
      }
      return net(new URL(res.headers.get("location"), url).href, init);
    }
    return res;
  }
  const handlers = {};
  const self = {
    addEventListener: (k, f) => { handlers[k] = f; },
    skipWaiting() {},
    registration: { navigationPreload: null, unregister() {} },
    clients: { claim: async () => {} },
  };
  const start = () => vm.runInContext(src, vm.createContext({ self, caches, fetch: net, location: { origin, href: origin + "/_quillo/sw.js" }, Response, Headers, URL, Promise, console, Date: FakeDate, Set, RegExp, setTimeout }));
  start();
  const run = async (kind, e) => {
    const waits = [];
    let answer = null;
    e.waitUntil = (p) => waits.push(p);
    e.respondWith = (p) => { answer = p; };
    handlers[kind](e);
    let res = answer ? await answer : null;
    for (let i = 0; i < waits.length; i++) await waits[i];
    return res;
  };
  const browser = {
    jar, store, caches,
    set online(v) { online = v; },
    async install() { await run("install", {}); await run("activate", {}); },
    set time(v) { time = v; },
    // The browser's check for a new worker: the one served now, installed over this one's caches.
    async update() { src = await (await fetch(origin + "/_quillo/sw.js")).text(); start(); await this.install(); },
    // A navigation, its redirects followed as the browser does (each a navigation of its own).
    async go(path, method = "GET", body, extra = {}) {
      let url = new URL(path, origin).href;
      for (let i = 0; i < 5; i++) {
        // Its headers a `Headers`, as a browser's request has them.
        const req = { url, method, mode: "navigate", body, headers: new Headers({ ...(body ? { "content-type": "application/x-www-form-urlencoded" } : {}), ...extra }) };
        const res = await run("fetch", { request: req, preloadResponse: Promise.resolve(undefined) });
        if (res && res.type === "opaqueredirect") { url = res.location; method = "GET"; body = undefined; continue; }
        if (!res) return { status: "not handled" };
        const text = await res.text();
        const ks = (/data-kept-session="([^"]*)"/.exec(text) || [])[1] || null;
        // The gate, as src/client/sw-gate.js runs it (when it runs): the jar's quillo_tag against the mark.
        if (ks !== null && browser.gate !== false) {
          const tag = jar.has("quillo_tag") ? jar.get("quillo_tag") : "-";
          if (tag !== ks) { await run("message", { data: { quillo: "drop", who: ks }, ports: [] }); browser.gated = (browser.gated || 0) + 1; continue; }
        }
        return { status: res.status, h1: (/<h1>(.*?)<\/h1>/.exec(text) || [])[1] || null, kept: /data-kept/.test(text), session: ks, offlinePage: /data-quillo-offline/.test(text) };
      }
    },
    async message(data) { await run("message", { data }); },
    async dump() {
      const out = {};
      for (const [n, m] of store) out[n.replace(/:[0-9a-f]{16}/, ":V")] = [...m.keys()].map((u) => new URL(u).pathname);
      return out;
    },
  };
  return browser;
}
