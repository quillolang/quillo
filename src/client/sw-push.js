// Quillo's service worker for a program that uses `notification` and has no route tagged
// `offline`, and for any that does under --watch (FEEDBACK67.md, B.3; branch pwa): it shows the
// program's notifications, with sw.js's part for them after this, and keeps nothing. It answers no
// request, so every one goes to the network as if there were no worker (on a navigation it only
// looks at the browser's subscription, in that part), and it drops what a worker of Quillo's kept
// for this program before.
const BASE = @BASE@, MINE = "quillo:" + BASE + ":";
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil((async () => {
  for (const k of await caches.keys()) if (k.startsWith(MINE)) await caches.delete(k);
  await self.clients.claim();
})()));
