// Quillo's service worker under --watch, and for a program with no route tagged `offline`: it
// takes itself away, with what Quillo's worker kept for this program, for a browser that
// registered that one in an earlier run, whose pages it would answer from its cache when the
// server is down (FEEDBACK67.md, B.2 and its review; branch pwa).
const MINE = "quillo:" + @BASE@ + ":";
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil((async () => {
  for (const k of await caches.keys()) if (k.startsWith(MINE)) await caches.delete(k);
  await self.registration.unregister();
})()));
