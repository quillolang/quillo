# Feedback from reviewing PWAs as part of the language: installable apps, offline pages and notifications

| | |
|---|---|
| App | None: a review of the user's requirement of 2026-11-14: "pwa should be part of the language including nofication" |
| Quillo | `quillo 0.8.2 (b33c2be, 2026-11-14)`, the release on the PATH. The checkout is on branch speed-fortunes (`b1427f3`), whose changes (page size, speed) do not touch what this report probes |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | A probe app: the catalog's `pwa_manifest` and `web_push` plugins, with a service worker written by hand. Run with `quillo run`, `--watch` and `--instances 2`, and driven over the DevTools protocol in headless Chrome (Chrome's own manifest and installability checks, the worker's own target). Node 22 played a browser and a push service: it made a subscription's keys, ran a local sink, and decrypted what Quillo sent by RFC 8291 with Node's crypto. Also `quillo check`. Read: `src/server.rs`, `src/client/runtime.js`, `mobile/ios/Quillo/App.swift`, `mobile/android/.../Bridge.kt`, `mobile/app-config.sh`, the guide and the 628-plugin catalog. Browser support comes from WebKit's and Chrome's own posts, searched on 2026-11-14. The probe app and scripts are in `.git/wip/feedback67/` |

No Quillo source was changed, and neither were `feedback/INDEX.md` or `feedback/repros/`. Each repro is in its own section. Numbers start at Q-956, after FEEDBACK66.md's Q-954 and FEEDBACK65.md's follow-up Q-955. Under the rule of 2026-11-05, nothing here is built until main is in sync, CI is green and the docs are aligned. While this was written, CI on main was still running for `ed486e0`. The user's next fix after 0.8.2 is Q-954.

**How the requirement is read.** "Part of the language" is read as what `live` already is for live data: a program says what it wants, and writes no JavaScript, no service worker, no keys and no plugin choice. A PWA is three things here:

- **Installable:** a manifest, icons and head tags.
- **Works offline:** a service worker.
- **Notifications:** permission, a push subscription, a server that sends, and a worker that shows the message.

"Notification" is read as a system notification that reaches a person who is not on the page. It is not the in-page note that `notify` already shows.

What worked well: the protocol work was already done in the catalog.

- `pwa_manifest` made 14 icon files and a manifest that Chrome's own checks found installable.
- `web_push`'s message decrypted exactly with an independent implementation, and its VAPID signature verified.
- `js()` waited for `navigator.serviceWorker.register`, for `.ready` (a property that is a Promise) and for `Notification.requestPermission` in a handler, with no glue code.
- Live data comes over a WebSocket, which a service worker never sees, so `live()` stayed right even under the worst worker.
- The runtime, the bundles and the sheets are named by their content (`/_quillo/r/<hash>.js`, `/_quillo/b/<hash>.js`), so a worker can safely keep them for ever.

## The answer

**Yes, make it part of the language. The hard part of a PWA is the service worker, and only Quillo knows its own URLs, versions, sessions and instances. The protocol work already works as plugins (measured). What no plugin can do is the worker, the browser side of asking and subscribing, and tying a subscription to a user and to sign-out. Build three things, each only for a program that asks:**

1. **Installable, from `app.toml`,** the file the phone builds already read.
2. **A service worker that Quillo writes,** with pages kept for offline by a route tag.
3. **A `notification` module:** `notification.ask()` in a handler and `notification.send(user, { title, body, url })` on the server, over Web Push. The key comes from `QUILLO_SECRET`, and subscriptions are kept in `quillo-system.db`.

```toml
# app.toml, beside main.qwe: the same file mobile/ios/build.sh and mobile/android/build.sh read
name = "Corner Shop"
icon = "icon.png"
short_name = "Shop"
theme_color = "#2e7d32"
```

```quillo
route GET /menu offline = <html><body><Menu/></body></html>       // kept for offline, network first

server fn ship(order: int) -> map with db {
    let o = db.one("update orders set shipped = 1 where id = ? returning user_id", order)
    notification.send(o.user_id, { title: "Shipped", body: `Order ${order} is on its way`, url: `/orders/${order}` })
}

component Alerts() {
    state on = notification.state()            // "granted", "denied", "default" or "unsupported"
    <button disabled={on == "granted"} onclick={() => { on = notification.ask() }}>Tell me when it ships</button>
}
```

| | A. Today: two plugins and a worker by hand | B. Quillo writes the worker and the browser side, and sends (recommended) | C. B, but sending stays in the `web_push` plugin |
|---|---|---|---|
| What the program writes | `pwa_icons`, `pwa_manifest` and `pwa_head_tags` calls, `raw(...)` in every page's head, a worker in JavaScript, a subscribe handler, a table, a send loop, and a key in `.env` (the probe: 66 lines of Quillo and 33 of JavaScript) | `app.toml`, `offline` on routes, `ask()` and `send()` | As B, plus a `use "web_push.qwer"` |
| What a model writes first | The tutorial worker, which loops under `--watch` (Q-957) and shows old pages (Q-958). `notify(...)` on the server, which passes the checker and fails every call (Q-959) | No worker. `notify` on the server gets a message that names `notification.send` | As B |
| Instances and restarts | The sample makes a key at start, so each instance has its own (Q-956) and a restart loses them all | One key from `QUILLO_SECRET`, the same wherever sessions work | As B, if the key is handed to the plugin |
| Sign-out | The subscription stays with the old user unless the program unbinds it. Pages a worker kept stay on the device | `end_session()` unbinds the browser's subscription and drops kept private pages | As B |
| Sending to many | One new connection per message (Q-960), inside the request | Kept connections, one round trip a message | One connection per message unless the plugin is fixed |
| Builds | Two `.qwer` plugins, so cargo at first run (the sample was up in 15 s here, build included). Not available in phone builds, and `--no-build` needs `quillo build` first | None | One plugin |
| Bytes on a page | 423 bytes of head tags as the plugin writes them, on every page | 157 bytes (manifest, theme colour, Apple icon), only with an `app.toml`. About 100 more (estimated) on a page tagged `offline` | As B |

B against today, in the four impacts:

| Impact | Quillo owns installing, the worker and notifications |
|---|---|
| **Functionality** | A program with an `app.toml` can be installed. Routes tagged `offline` open with the server unreachable, showing their last copy marked as old. `notification.send` reaches a person's devices with the page closed. No change for a program that uses none of it |
| **User** | Visitors get an install option, a page or an offline page in place of the browser's error, and notifications they asked for, from one prompt opened by a click. Developers write no JavaScript, worker or key, and `--watch` stays right |
| **Requests** | 157 bytes of head tags per page with an `app.toml`, and about 100 bytes of script per page tagged `offline`. The worker script is fetched when browsers check for updates (on navigations, and at least daily). The manifest and icons are fetched once, on install. One POST per subscription. Outbound: one request per device per notification. A page the worker answers costs the server nothing |
| **Security** | New stored data: one push subscription per device, bound to a user and unbound at `end_session()`. Kept private pages stay on the device until sign-out. The VAPID key comes from `QUILLO_SECRET`, so rotating the secret ends every subscription. The worker sees all of the site's same-origin requests, so it is Quillo's own code, versioned with the release. Payloads are encrypted end to end: the push service sees only size and timing |

What today's way gives that B would lose:

- A program chooses its own caching, for example cache-first for a game's assets. B keeps this with a worker of the program's own, named in `app.toml` (Part C).
- Core does not grow.
- A fix to the encryption is a plugin release, not a Quillo release.

**My recommendation:** B. The worker is where every measured failure was (Q-957, Q-958), and it depends on facts only Quillo has: which `/_quillo/` paths are named by content, the build's version, which pages are private, when a session ends, and `--watch`. Sending belongs with it, because the key, the subscription table and dropping ended subscriptions are core's. The crypto is already linked: `ring` 0.17 (Cargo.toml:41) has ECDH P-256, HKDF-SHA256, AES-128-GCM and ECDSA P-256. The trade-offs:

- Core grows, by an estimated 800 to 1,000 lines with tests (an estimate, not counted).
- An encryption fix needs a release.
- Firefox gives no install (Windows "taskbar tabs" only, in Labs).
- iPhones get push only for a site added to the Home Screen.

## Part A: what works today, measured

| Check | Result |
|---|---|
| `pwa_manifest.qwe`, the catalog's own sample | Up in 15 s, the plugin's build included. Its own check: installable, with one warning (no screenshots). 11 manifest icons, 14 files, 148 KB |
| Chrome's own checks on the probe (`Page.getAppManifest`, `Page.getInstallabilityErrors`) | No manifest errors and no installability errors, before any worker and after. Chrome stopped requiring a worker for installing from the menu in 108 (Android) and 112 (desktop), and its post of that change says the automatic prompt still wanted a `fetch` handler then |
| A worker registered from a handler with `js()` | `register`, `.ready` and `Notification.requestPermission` were waited for. The handler then called the `server fn` after them (server log) |
| `pushManager.subscribe` in headless Chrome | No answer in 30 s, from Quillo's handler and from plain JavaScript alike: headless Chrome has no push service. Not Quillo's problem. Delivery was checked as below instead |
| `web_push`'s message, decrypted in Node by RFC 8291 | Exact round trip of "Grüße: your order 1042 has shipped ✓", with delimiter 2, record size 4096, `aes128gcm`, `TTL: 60` and `Urgency: normal` |
| Its VAPID header, checked by RFC 8292 | The ES256 signature verifies against `k`, and `k` is the server's public key. `aud` is the endpoint's origin, and `exp` is 12 h ahead |
| 501 subscriptions in one `/send` | 1,442 ms on loopback over plain HTTP: 2.88 ms each, with 501 connections (Q-960) |
| `--instances 2`, 40 calls of `vapid_key` | Two keys, 20 calls each (Q-956) |
| `live()` under a cache-first worker, with a note added on the server | The open page showed it: the WebSocket is outside the worker |
| The tutorial worker, then the server stopped | Home showed 5 notes while the server had 6: the page as it was at install (Q-958). A page never opened: Chrome's "This site can't be reached" |
| A network-first worker (`sw-careful.js`), then the server stopped | Home showed the last copy seen, 7 of 7. A page never opened: the worker's own offline page |
| A `server fn` called with the server stopped | Nothing showed on the page, with either worker. The call returns the error value "server fn `add_note` could not be reached: ..." (`runtime.js:5762`), and the probe's handler dropped it |
| `--watch`, the tutorial worker, one save | 263 reloads in 4 s (314 in the minimal repro), and the old page still shown (Q-957) |
| `--watch`, the network-first worker, one save | One reload, and the change shown |
| `notify(...)` in a route or a `server fn` | `quillo check` says ok, and each call is a 500 (Q-959) |

The phone apps already show local notifications: `quilloNative.notify` (`App.swift:444`, `Bridge.kt:171`). They cannot receive pushes: App.swift never calls `registerForRemoteNotifications`, and the Android app has no Firebase (read). Service workers do not run there (USERGUIDE.md:358). The program runs on the device, so offline is already true there.

## Part B: what the language would own

### B.1 Installable, from `app.toml`

- **Keys:** `name` and `icon`, shared with the phone builds, plus `short_name`, `theme_color`, `background_color`, `display` (default `standalone`), `start_url` (default the base) and `description`. The phone builds read only the keys they know, by an anchored `sed` (mobile/app-config.sh:17), so the web keys do not disturb them.
- **What Quillo serves:** `/manifest.webmanifest`, and the icons made once from `icon` with `image.convert`, which already resizes (guide §Images). The plugin's maskable padding moves into core. Both are under `--base`: `start_url`, `scope` and `id` carry the prefix.
- **Head tags:** `<link rel="manifest">`, `<meta name="theme-color">` and `<link rel="apple-touch-icon">`, 157 bytes, on pages that have a `<head>`, as Quillo already adds its scripts. That drops to about 100 bytes if Quillo also answers `/apple-touch-icon.png`, which Safari asks for anyway. A page or program with a manifest of its own gets nothing added.
- **Checks:** `quillo check` reads `app.toml` with the plugin's rules (`pwa_check`): no 512 px icon, `display: browser`, a missing `id`. That gives errors before running, not a missing Install button.
- **Speed:** a program with no `app.toml` gets no change, in bytes or in time. The page benchmarks (Fortunes, the site) are run old against new anyway.

### B.2 The service worker

- **When:** only when a route is tagged `offline` or the program uses `notification`. It is served at `/_quillo/sw.js` with `Service-Worker-Allowed: <base>/` and `cache-control: no-cache`. Its text carries the build's version, so a deploy installs the new worker and that worker drops the old caches.
- **Registering:** a script of about 100 bytes on pages tagged `offline`, and the runtime when `notification.ask()` runs. Never in the phone shells (`platform()` says so).
- **The rules, as `sw-careful.js` measured them:**
  - `/_quillo/{b,r,s,t}/<hash>` come from the cache first.
  - No other `/_quillo/` path is touched: `rpc`, `live`, `events`, `ready`, `prefetch-used`, `src/` under `--watch`, and the unhashed `quillo.js`.
  - A navigation to an `offline` route goes to the network first, and a 200 answer is kept.
  - Any other navigation goes to the network, and gets an offline page made from `app.toml`'s name when the network fails.
- **Kept pages say so:** the runtime marks a page that came from the cache (`<html data-kept="2026-11-14T10:00Z">`), so a sheet can say it is old. This is what Q-958's tutorial worker could not do.
- **Private pages:** an `offline` route that reads the session (a person's orders) is kept, because the tag says so. It is dropped when the session ends: the answer that ends it carries a header the worker acts on. This cannot be `Clear-Site-Data: "storage"`, which also unregisters the worker and so ends its push subscription (from the spec, not tried). Quillo already sends `Clear-Site-Data: "prefetchCache", "prerenderCache"` on answers that write or change the session (server.rs:535). The Cache API keeps whatever it is given, `Cache-Control: private` or not, so the worker reads Quillo's mark itself (Q-886).
- **Pages kept in advance:** on install, the worker fetches each `offline` route that has no parameters. Each page's bundle is fetched from the page, because a bundle is known only once its page is rendered (Q-909).
- **`--watch`:** `/_quillo/sw.js` is a worker that unregisters itself and drops its caches. Q-957's fix in the dev script covers workers the program wrote itself.
- **Coming back online:** the runtime listens for `online` and opens its socket at once, without waiting out the backoff (smaller notes).

### B.3 Notifications

- **In the browser:**
  - `notification.ask(options)` runs in a handler and answers `"granted"`, `"denied"`, `"default"` or `"unsupported"`. It registers the worker if needed, asks, subscribes with the server's key, and posts the subscription to a `/_quillo/` endpoint. The subscription is bound to `session().user`, or to `{ user: x }` (rule of 2026-11-07).
  - Browsers allow the permission prompt only close to the click (WebKit asks for it in the gesture). So `quillo check` warns of an `ask()` that comes after a wait in the same handler.
  - `notification.state()` reads the permission and the subscription without asking. `notification.stop()` unsubscribes.
  - In a Safari tab on an iPhone, where push needs the Home Screen, `state()` is `"unsupported"`.
- **On the server:**
  - `notification.send(user, { title, body, url, tag, badge, silent, ttl, urgency })` works in a route, a `server fn`, a job or an `every`, and is refused in a component.
  - It sends to each of that user's subscriptions, drops those whose push service answers 404 or 410, and gives `{ sent, gone }`. A payload over 3,800 bytes is an error value (one 4,096-byte record).
  - One person has a few devices, so `send` waits as `http.post` does. A message to everyone is a loop in a job.
- **The payload:** Declarative Web Push JSON (`"web_push": 8030`, a `notification` with `title`, `body`, `navigate` and `tag`, and `app_badge`). `navigate` is absolute, built from the origin the subscription was made on, which is kept with it.
  - iOS and iPadOS 18.4 (Home Screen apps) and Safari 18.5 on macOS show it without running the worker.
  - Elsewhere, Quillo's worker shows it, because Chrome and Firefox have not shipped declarative push.
  - On a click, the worker focuses an open window of the app and moves it to the URL, or opens one.
- **The key:** a P-256 key derived from the session key by HKDF with a fixed label, so it is the same on every instance and every server that shares `QUILLO_SECRET`, as sessions must be (USERGUIDE.md:2697). If a program uses `notification` with no `QUILLO_SECRET`, Quillo warns at start: a restart would end every subscription without a word to anyone.
- **The table:** `_quillo_push` in `quillo-system.db`, beside `_quillo_mcp_tokens`. It holds the endpoint (key), `p256dh`, `auth`, the user, the session it was made in, the origin, when it was made and the last success. `end_session()` unbinds that session's subscriptions, so the next person on a shared device gets nothing meant for the last one.
- **Sending:** one kept connection per push service host (Google, Mozilla, Apple, Microsoft), off the event loop. The plugin's 128 lines are the reference, and the Node decrypt probe is the test.
- **Speed:** sending is new code, benchmarked on its own. Nothing changes for a program that does not use it.

### B.4 Phone and desktop apps

The same calls would work in the phone shells. `ask()` would ask the system and register for APNs (iOS) or FCM (Android). The shell would hand the device token to the program, and `send` would choose APNs or FCM for those rows. The catalog's `apns` and `fcm` plugins already hold that protocol work. This needs an Apple push key and a Firebase project for each app (new `app.toml` keys), and store uploads are not set up yet. So it comes after the web. `quillo app` on the desktop was not checked.

## Part C: what not to build

- **Worker code written in Quillo** (a `worker` block compiled to JavaScript). Quillo's worker is fixed code. A program with special needs names its own (`worker = "public/sw.js"` in `app.toml`), and Quillo's worker loads it with `importScripts` after its own handlers. Two registrations on one scope replace each other, so the two cannot be separate.
- **Writes queued while offline, for now.** A `server fn` called offline gives the error it gives today. A queue for `idempotent` calls (guide §Idempotent requests), whose `Idempotency-Key` makes a replay safe, is the natural next step. Background Sync is Chromium only.
- **An install-button API** (`beforeinstallprompt` is Chromium only), Periodic Sync, or store packaging of the web app (TWA). The phone shells are Quillo's way into the stores.
- **Notification topics for people who are not signed in,** in the first slice.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-909 (FEEDBACK59.md) | A bundle is known only to the run that rendered its page. A worker cannot be told in advance every bundle an offline page needs: it keeps each one when the page is fetched (B.2) | 0 (read) |
| Q-886 (FEEDBACK55.md, fixed) | Quillo marks pages that read the session `private`. The Cache API ignores `Cache-Control`, so a worker keeps them anyway unless it reads Quillo's mark itself (B.2) | 0 (read) |

## New issues

### Q-956. A VAPID key made at start is different on each instance: under `--instances 2` half the pushes are signed with a key the subscription was not made with

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `--instances 2`, 40 calls of the key | 1 |

The catalog's sample (`web_push.qwe:9`) makes the key when no `VAPID_PRIVATE_KEY` is set:

```quillo
let VAPID = env("VAPID_PRIVATE_KEY", null) ?? webpush_vapid_keys().private_key
server fn vapid_key() -> str = webpush_public_key(VAPID)?
```

Its comment warns that subscriptions "then last until the next restart". It does not say that `--instances N` runs the program N times, "each with its own variables" (USERGUIDE.md:65). With two instances, 40 calls gave two keys, 20 each. A browser subscribes with whichever key its page got. Then every `/send` that lands on the other instance signs with the wrong key, and push services refuse it (RFC 8292, restricted subscriptions: 401 or 403). The plugin answers that as `ok: false, gone: false`, so the subscription is kept and never works. quillolang.org itself runs with `--instances 4`.

| Impact | |
|---|---|
| **Functionality** | About 1 in N notifications arrives with N instances, and none after a restart |
| **User** | Visitors who turned notifications on get some or none. Developers see `status: 403` only if they log the results, and nothing in testing on one instance |
| **Requests** | Every refused send is still a request to the push service |
| **Security** | No change: a wrong key is refused |

**Suggestion:** in the plugin's sample, refuse to start without `VAPID_PRIVATE_KEY`, and say how to make one (`webpush_vapid_keys()` once). In core, derive the key from the session key (B.3), so there is no second secret to share.

### Q-957. Under `--watch`, a page controlled by a cache-first service worker reloads itself without end and never shows the change

| Kind | Detected by | Attempts |
|---|---|---|
| silent, perf | headless Chrome, counting load events after a save | 2 |

```quillo
route GET /sw.js = file("sw.js")
route GET / = <html><body><h1>One</h1><script>navigator.serviceWorker.register("/sw.js")</script></body></html>
```

```js
// sw.js: the tutorial worker, cut down
self.addEventListener("install", (e) => e.waitUntil(caches.open("v1").then((c) => c.add("/"))));
self.addEventListener("activate", (e) => e.waitUntil(clients.claim()));
self.addEventListener("fetch", (e) => e.respondWith(caches.match(e.request).then((hit) => hit || fetch(e.request))));
```

`quillo run --watch loop.qwe`. Open `/`, reload once so the worker controls the page, then change `One` to `Two` and save. In 4 s the page loaded 314 times and still said `One`. The server logged four `GET /` in the whole run: the loop never reaches it. A reload that bypasses the cache shows `Two`.

The dev script (server.rs:3295) holds the version its page was rendered from, and calls `location.reload()` when `/_quillo/events` reports another. The worker answers the reload with the cached page, which still holds the old version, and so the page reloads for ever. The fuller probe (`main.qwe` with the tutorial worker) did the same: 263 reloads in 4 s. With a network-first worker, a save gave one reload and showed the change.

| Impact | |
|---|---|
| **Functionality** | `--watch` never shows a change on a controlled page |
| **User** | The developer sees the old page flicker, at a full core, with no message. Every edit seems to do nothing until a hard reload or unregistering the worker in DevTools |
| **Requests** | None reach the server: the loop is in the browser |
| **Security** | No change |

**Suggestion:** in the dev script, when the page reloaded for a version and came back with the same old one (kept in `sessionStorage` across the reload), stop. Then tell the developer in the console and in the page's error overlay: "this page came from a service worker's cache (`navigator.serviceWorker.controller`), so `--watch` cannot refresh it. Unregister it, or have it go to the network first for pages". Quillo's own worker (B.2) unregisters itself under `--watch`. Test: the repro above, with the worker, gives one reload and a message.

### Q-958. Nothing tells a worker which of Quillo's URLs it may keep, and a kept page carries no sign that it is old

| Kind | Detected by | Attempts |
|---|---|---|
| docs, limitation (silent to the visitor) | the probe with its server stopped | 2 |

The tutorial worker, the one a model writes for "make it work offline", kept `/`, `/other`, the manifest, `/_quillo/b/<hash>.js`, `/_quillo/r/<hash>.js` and an icon. With the server stopped, home showed 5 notes while the server had 6: the page as it was at install. Nothing on the page said it was old. The guide never mentions service workers except to say they do not run in the phone shells. It does not say:

- which `/_quillo/` paths are named by their content and may be kept for ever (`b`, `r`, `s`, `t`);
- which must always go to the network (`rpc`, `live`, `events`, `ready`, `prefetch-used`, `src/` under `--watch`, and the unhashed `quillo.js`, which a cache-first worker would keep across a Quillo upgrade: read, not tried).

| Impact | |
|---|---|
| **Functionality** | Offline pages show old data as if current. A worker that keeps everything also keeps what Quillo needs fresh |
| **User** | Visitors read old prices or orders with no warning. Developers find the rules by reading runtime.js |
| **Requests** | No change |
| **Security** | A worker that keeps every page keeps signed-in pages after sign-out: the Cache API ignores `private` (from the spec, not tried here) |

**Suggestion:** a guide section, "A service worker of your own", with the table of `/_quillo/` paths, network first for pages, and dropping kept pages at sign-out. `sw-careful.js` is its example. Then B.2, which marks a kept page as kept.

### Q-959. `notify` in a route or a `server fn` passes `quillo check` and fails every call with a 500

| Kind | Detected by | Attempts |
|---|---|---|
| loud at run time, silent to the checker | `quillo check`, then a request | 1 |

```quillo
route POST /orders/:id/ship ? { user: str } {
    notify(`Order ${id} has shipped`, { title: "Shop", to: user })
    { ok: true }
}
```

`quillo check`: `ok (1 route, 0 functions, 0 components)`, and the same with the call inside a `server fn`. A `POST /orders/7/ship?user=ana` gets `500 internal error`. The log says: "`ask` and `notify` speak to someone in the browser: call them from a component (an event handler)". The guide says so too (USERGUIDE.md:1847). Asked for "notify the customer when the order ships", this is the first thing a model writes, because the builtin has the name of the job. The message names no way to reach someone who is not on the page.

| Impact | |
|---|---|
| **Functionality** | The route fails every time. Whether a write made before the `notify` stays was not checked |
| **User** | The visitor gets an error page. The developer learns at the first request, not at check time |
| **Requests** | A 500 per call |
| **Security** | No change |

**Suggestion:** have the checker refuse `ask` and `notify` written directly in a route body, a `server fn` or a job, where it knows the side. Have the message add: "to reach someone who is not on the page, use `notification.send`", or the `web_push` plugin until that exists. It should also say that `publish` updates pages that are open. Test: both forms refused at check time, with the message.

### Q-960. Sending a notification to many people opens a new connection for each message, inside the request

| Kind | Detected by | Attempts |
|---|---|---|
| perf | 501 subscriptions on a local sink | 1 |

`webpush_send` calls `ureq::post(endpoint)` (`web_push.qwer:114`), which builds a new agent and connection each time. One `/send` to 501 subscriptions took 1,442 ms on loopback over plain HTTP, with 501 connections. A real push service adds a TCP and TLS handshake to each send. At an estimated 50 to 100 ms each from a server (not measured), 10,000 people take 8 to 17 minutes, and the sample does it inside a route, which the visitor waits on.

| Impact | |
|---|---|
| **Functionality** | Broadcasts take minutes, and a route that sends times out for its caller |
| **User** | Late notifications. The developer who copies the sample has a route that hangs |
| **Requests** | One TLS handshake per message to the push service |
| **Security** | No change |

**Suggestion:** in the plugin, one `ureq::Agent` kept in a `OnceLock`, which reuses connections per host. In the sample, send from a job (`later`), not the request. In core, B.3's kept connections. Benchmark: 501 sends before and after, counting connections.

## Proposed order of work

All of this comes after main is in sync and after Q-954. Each item has its tests and edge tests in `tests/feedback67.rs`, coverage stays at 99%, and the guide and the book change in the same slice.

1. **Q-959 and Q-957:** small core changes, a check rule and the dev script.
2. **Plugins (website repo):** Q-956's sample, Q-960's agent, and `short_name` cut at a word (smaller notes). Each changed catalog `try` line says why.
3. **Docs for today:** Q-958's section, and an "Installable apps and notifications" section built on the plugins. Useful even if B waits.
4. **B.1** from `app.toml`.
5. **B.2,** the worker and the `offline` tag. A new tag is grammar work in every editor: the `fn mcp` tag took 28 files in quillo-editors (FEEDBACK63.md).
6. **B.3,** `notification`, browser then server, with the Node decrypt probe as a test.
7. **B.4,** the phone shells, after store set-up.

## Open questions for the user

### 1. Where is the app described?

**`app.toml` or a call in the program?** This decides where `name`, `icon` and the colours are written, for the installable site and the phone app alike.

| Impact | `app.toml` | `app({ name, icon, ... })` at the top of the program |
|---|---|---|
| **Functionality** | One name and icon for the phone app and the site | Checked as code, and can use `env` |
| **User** | One file, already in the guide for phones | One more top-level builtin, as `site_headers` is |
| **Requests** | No change | No change |
| **Security** | No change | No change |

What the call would give that the file lacks: expressions. Flat TOML has none.

**My recommendation:** `app.toml`, one source for both. **Question:** `app.toml`? If not, I use `app({...})` with the same keys, and the phone builds keep reading their own file.

### 2. Which pages are kept for offline?

**Only routes tagged `offline`, or every public page a visitor has opened?** This decides what a visitor sees with no network.

| Impact | Tagged routes only | Every public page opened |
|---|---|---|
| **Functionality** | The program chooses. The rest get the offline page | Whatever the visitor has seen works offline |
| **User** | One word per route | Nothing to write, but a page may show old data the program never meant to be read offline |
| **Requests** | Precaching only the tagged routes | No change in requests. The device keeps more |
| **Security** | Private pages only when tagged, dropped at sign-out | Public pages only, so no change |

What everything-kept would give: offline with no thought at all.

**My recommendation:** tagged only. It is explicit, and it keeps a worker away from programs that never asked for one. **Question:** a tag per route? If not, I keep every public page network first, and drop the tag from B.2 (and so the editor work).

### 3. The module's name

**`notification` (with `.ask`, `.state`, `.stop`, `.send`), knowing that `notify` stays the in-page note?** `push` is taken (`todos | push("x")`), and so is `notify`.

| Impact | `notification` |
|---|---|
| **Functionality** | A new module. A variable named `notification` would hide it, which `quillo check` reports since Q-069's fix (none of `app/` has one) |
| **User** | Two names close together: `notify` (on the page) and `notification` (to someone's device). Q-959's message links them |
| **Requests** | No change |
| **Security** | No change |

**My recommendation:** `notification`. **Question:** that name? If not, give me the one you want, and nothing else in the plan changes.

### 4. Sending in core, or the plugin under it?

**Should `notification.send` do Web Push itself, or call the `web_push` plugin?** This decides whether a program builds Rust.

| Impact | In core (ring) | The plugin |
|---|---|---|
| **Functionality** | Works in phone builds and with `--no-build` | Needs cargo, or `quillo build` |
| **User** | Nothing to install. A fix is a Quillo release | 15 s at first run here. A fix is a plugin release |
| **Requests** | Kept connections from the start | As the plugin does it (Q-960) |
| **Security** | The key never leaves core | The key passes to the plugin as text |

**My recommendation:** core. The primitives are already linked, and the plugin stays for programs that want raw Web Push. **Question:** core? If not, `send` calls the plugin and needs it in the program's folder.

### 5. Phone apps in the first release of this?

**Should remote push in the iOS and Android shells come with the web version, or after?** The shells have local notifications only.

| Impact | Web first, phones after |
|---|---|
| **Functionality** | Phone apps keep `quilloNative.notify` (local) until then |
| **User** | One API later for both. Developers of phone apps wait |
| **Requests** | No change |
| **Security** | No change until then |

**My recommendation:** after, since it needs an Apple push key, a Firebase project and store set-up. **Question:** web first? If not, I add APNs and FCM in slice 6, with `app.toml` keys for them.

## Smaller notes

- `pwa_manifest` cuts the default `short_name` at 12 characters, mid-word ("Quillo Corner Shop" gives "Quillo Corne", `pwa_manifest.qwer:39`), and the catalog's own `try` line expects exactly that. Cut at a word, or warn and keep the name.
- A handler waiting on a page Promise that never settles waits for ever, with no sign. Headless Chrome's `pushManager.subscribe` never answered, and the button stayed "idle" with nothing in `__quilloErrors`. JavaScript's `await` does the same. `notification.ask()` should have its own time limit and answer `"unsupported"`.
- The runtime has no `online` listener (read: none in runtime.js), so a socket that dropped comes back on its own backoff, up to every 30 s (guide). Not measured: CDP's offline emulation keeps an open WebSocket open, and the page showed a change 111 ms after the network came back.
- The plugin's sample builds its page as a string with `respond(200, "<!doctype html>...")`. A Quillo literal with `{raw(HEAD)}` in `<head>` works (the probe), and would suit the guide better.
- Firefox installs nothing outside Windows "taskbar tabs", which 2025 reports place in Labs from Firefox 142 (Mozilla's docs describe them; no 2026 status found). It takes Web Push.

## Where the AI struggled

| What was tried | What happened | Whose mistake |
|---|---|---|
| The tutorial worker, cache first for every GET, as the first offline attempt | Q-957's loop under `--watch`, and Q-958's old pages | the AI's, and the guide's silence |
| Checking delivery with `pushManager.subscribe` in headless Chrome | No answer in 30 s: headless has no push service. Replaced by Node playing the browser and the push service | the tool's |
| `Network.emulateNetworkConditions` for "offline" | It does not reach the page's worker: the "offline" run with the network-first worker still reached the server (`GET /other` and `GET /` in the log). Redone by stopping the server | the AI's |
| `?` after `webpush_send` inside a `map` lambda | `quillo check` warned that `?` there changes nothing. Right: kept for the probe, and worth knowing for B.3's loop | the AI's |
| `echo ====` between commands | zsh expanded `=word` and the command failed, as in FEEDBACK63 | the AI's |

## Edge cases checked that behave well

- Chrome finds the plugin's manifest and icons installable with no worker at all.
- `js("navigator.serviceWorker.ready")`, a property holding a Promise, is waited for like a call.
- `live()` stays right under a cache-first worker: the WebSocket is outside it, and a stale page caught up with the server's rows while online.
- The encryption round trip keeps non-ASCII text exactly. The record has the one delimiter byte 2 and no padding mistakes.
- The VAPID `exp` is 12 h ahead, inside RFC 8292's 24 h.
- `files("data/icons", name)` served the icons Chrome asked for (`icon-144.png`, `favicon-32.png`), and the manifest went out as `application/manifest+json`.
- The phone builds' `app.toml` reader ignores keys it does not know: `short_name` does not match its anchored `name` key.

## Scripts

Everything is in `.git/wip/feedback67/` (the scratchpad does not survive a reboot). Each was run with `quillo` 0.8.2 from the PATH, on ports checked free first, and each server was stopped by its own PID.

| File | What it does |
|---|---|
| `pwa/main.qwe`, `pwa/public/sw.js`, `pwa/public/sw-careful.js` | The probe app (copy the two plugins and `pwa_manifest_sample.png` beside it from `~/dev/quillo-website/plugins`). `SW=public/sw-careful.js` serves the network-first worker |
| `probe-pwa.mjs <base> install,subscribe,push,live,offline` | Chrome's manifest and installability checks, the worker, `live()` under it |
| `probe-sub.mjs <base> headless <key>` | `pushManager.subscribe` from plain JavaScript |
| `probe-push-decrypt.mjs <base> <sink port> [n]` | Node as the browser and the push service: decrypt (RFC 8291), check VAPID (RFC 8292), time a fan-out of n |
| `probe-offline.mjs <base> <pid>` | Pages before and after stopping that server |
| `probe-watch.mjs`, `probe-watch-min.mjs <base> <file>` | Load events after a save under `--watch` |
| `probe-reconnect.mjs <base> <seconds>` | A live change after a spell offline |
| `loop/` | Q-957's minimal repro |
| `notify/n.qwe`, `notify/n2.qwe` | Q-959 |

```sh
# Q-956: two instances, two keys
PORT=8474 quillo run --instances 2 main.qwe &
for i in $(seq 1 40); do curl -s -X POST -H 'content-type: application/json' -d '[]' http://127.0.0.1:8474/_quillo/rpc/vapid_key; echo; done | sort | uniq -c
```
