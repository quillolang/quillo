# Installable apps, offline pages and notifications {#pwa}

A Quillo program can be installed from the browser as an app, keep its pages on the device
for when there is no network, and tell people's devices something with the page closed.
Each comes from what the program says (a file, a word on a route, a call), with no
JavaScript, service worker or key of its own.

## Installable apps

A program with an `app.toml` beside its main file can be installed from the browser, as an
app with a window and an icon of its own (Chrome's and Edge's Install, Safari's Add to Dock
and Add to Home Screen, Android's Add to home screen). Quillo writes what browsers ask for:
the manifest, the icons at each size, and the tags in each page's head; the program writes
no JavaScript, no service worker and no plugin. It is the file the phone builds read
(`mobile/README.md`), so the app on a phone and the site installed from a browser have one
name and one icon:

```toml
# app.toml, beside main.qwe: the phone builds read it too
name = "Corner Shop"                  # 1 to 30 characters
icon = "icon.png"                     # PNG, JPEG or WebP, 512 × 512 or more
short_name = "Shop"                   # under the icon, where room is short
theme_color = "#2e7d32"               # the title bar, and <meta name="theme-color">
background_color = "#ffffff"          # the splash screen, and behind the icons
display = "standalone"                # or fullscreen, minimal-ui, browser
start_url = "/"                       # where the app opens
description = "Groceries in ten minutes."
```

- **`name`**: 1 to 30 characters, as the phone builds take it. Empty or left out, the app is
  called Quillo.
- **`icon`**: a PNG, JPEG or WebP file in the program's folder (Quillo's own icon when left
  out). Quillo makes four icons from it: 192 and 512 pixels as the image is, its clear parts
  kept (one that is not square is centred on a clear square); a maskable 512, the image in
  the middle 80% on `background_color`, so that Android's round and squircle masks do not
  cut it; and Apple's 180, the whole square on the background with no transparency, as iOS
  shows none. Under 512 pixels it is enlarged, as Chrome offers to install only an app with
  a 512 icon; past 4096 pixels a side it is decoded whole, which takes memory (1024 × 1024
  is plenty). `quillo check` warns of both.
- **`short_name`**: the name under the icon on a home screen. Left out, it is the name cut
  at a word to at most 12 characters (`Quillo Corner Shop` gives `Quillo`), never in the
  middle of one: the name whole when it fits, or when its first word is longer.
- **`theme_color`** and **`background_color`**: CSS colours, `#rgb`, `#rgba`, `#rrggbb`,
  `#rrggbbaa` or a name in any case (`teal`, `Navy`). The background is white when left out.
- **`display`**: `standalone` (the default: a window of its own), `fullscreen`,
  `minimal-ui`, or `browser`, which opens the app in a tab, so that browsers do not offer to
  install it (`quillo check` warns).
- **`start_url`**: where the app opens, a path of the program's starting with `/`
  (`/app?from=home`); the program's base when left out.
- **`description`**: a line about the app, for the install dialogs that show one.
- **`id`** is the phone apps' bundle id (`org.example.shop`), and the manifest never takes
  it: the manifest's `id` is the app's scope, the program's base (`/`, or `/shop/` under
  [`--base /shop`](#options-of-quillo-run)), so that a `start_url` changed later is still
  the same app, not a second one installed beside it. `version`, `build`, `main`, `site` and
  `env.NAME` are the phone builds' too, and the site leaves them alone (`mobile/README.md`).

The file is flat TOML, read by Quillo as the phone builds read it: one `key = value` a
line, strings in `"..."` (TOML's escapes read, a `#` inside no comment) or `'...'`, bare
numbers and words, `#` comments. A key given twice is read at its first line, and a
`[table]` line is skipped, as the phone builds do (both are warned of).

What Quillo serves, once the program has an `app.toml`:

| Address | What it is |
|---|---|
| `/manifest.webmanifest` | the manifest: `name`, `short_name`, `description`, `start_url`, `scope`, `id`, `display`, the colours and the three icons, as `application/manifest+json`, revalidated (`no-cache` and an `ETag`) |
| `/_quillo/i/<hash>.png` | the 192, 512 and maskable icons, named by what they are made of (the image and the background), kept for a year |
| `/apple-touch-icon.png` | Apple's icon, which Safari also asks for by this name when no page links it; revalidated |

and three tags at the end of each page's `<head>`, 151 bytes with a theme colour:

```html
<link rel="manifest" href="/manifest.webmanifest"><meta name="theme-color" content="#2e7d32"><link rel="apple-touch-icon" href="/apple-touch-icon.png">
```

- **The icons** are made at the first request for one, the four from one decode, off the
  event loop, and kept: a start never waits for them. Under `--base` every address carries
  the prefix, as `start_url`, `scope` and `id` do. Under
  [`--instances`](#options-of-quillo-run), every instance serves the same manifest and
  icons.
- **The tags** go where the browser takes them: before `</head>`, or after `<html>` on a
  page with no head, which the browser then starts with them; a `HEAD`'s length counts them.
  None go on a part of a page (an answer with no `<html>`), on a page `file()` or `files()`
  sends as written, or on a page that links a manifest of its own. A page with a theme colour
  or an Apple icon of its own keeps it, and gets the other tags.
- **A route of the program's own wins.** A route at the very path (`route GET
  /manifest.webmanifest = ...`) answers there, and Quillo adds no tags; one at
  `/apple-touch-icon.png` is the Apple icon, and pages get no Apple link. A route that takes
  any path (`/*rest`, `/:page`, `files("public", path)`) is tried first, and wins only with
  a real answer: a `2xx` that is not HTML (a `public/manifest.webmanifest` of the program's,
  served by `files()`) or a `3xx`. Anything else is Quillo's, so a last `route ANY /*rest`
  for a 404 page, a front end's `route GET /*path = file("dist/index.html")`, and a group
  whose guard answers the browser's request (sent with no cookies) with a `401` keep the app
  installable. Nothing in the manifest or the icons is anyone's, so no guard holds them back.
- **No service worker is needed to install**: Chrome stopped asking for one to install from
  its menu (in version 108 on Android, 112 on the desktop). For pages that open with no
  network, see [Pages that work offline](#pages-that-work-offline).
- **Without an `app.toml`, nothing changes**: no tags, and the three addresses are the
  program's (a 404 where no route takes them). What a request pays is one check.

`quillo check` reads `app.toml` with the program and says each mistake at its line:

```
app.toml:4: error: theme_color "green-ish" is not a CSS colour: #rgb, #rrggbb, #rrggbbaa or a name such as teal
app.toml:7: warning: `theme_colour` is read by no one: `theme_color`?
```

Errors stop `quillo check` and `quillo run`: a name over 30 characters, an icon that is not
there, is not a PNG, JPEG or WebP image or cannot be decoded, a colour that is none, a
`display` that is none, a `start_url` that is not a path of the program's (`app`,
`//elsewhere.example`), a string never closed, a line that is not `key = value`, and a
file that cannot be read (not UTF-8, a folder). What the phone builds take is a warning: an
empty name, a key with no value, a `[table]` line, a key given twice. So are an icon under
512 or past 4096 pixels a side, `display = "browser"`, and a key no one reads, with the one
it was likely meant to be. Run without the check (`--no-check`), a value that is wrong is
left out, and the start says what became of the icon (`quillo: app.toml: icon
/srv/shop/notes.png is not a PNG, JPEG or WebP image: Quillo's icon is used`).

Under [`--watch`](#options-of-quillo-run), `app.toml` is one of the program's files: made,
changed or taken away, the program loads again with it, and so it does when the icon
changes.

## Pages that work offline

A route tagged `offline` keeps its page on the device, so that it opens with no network:
on a train, in a basement, or with the server down. The tag comes after `route`, before
the method; after `routes`, it tags every `GET` route of the group and of the groups
inside it:

```quillo
server fn todays() -> list = [{ name: "Bread", price: 3 }, { name: "Milk", price: 2 }]

component Menu() {
    state items = todays()
    <ul>{for i in items => <li>{i.name}: {i.price}</li>}</ul>
}

route offline GET / = <html><head><title>Corner Shop</title></head><body><h1>Today</h1><Menu/></body></html>
route offline GET /orders/:id(int) = <html><body><h1>Order {id}</h1></body></html>

routes offline /help {
    route GET /returns = <html><body><h1>Returns</h1></body></html>
    route POST /question { form: { text: str } } = redirect("/help/returns")    // sent as ever, never kept
}
```

- **A `GET`, and only a `GET`**: a page is one. `route offline POST ...` is an error, and
  so is a `routes offline` group with no `GET` route, and the tag anywhere else (`route GET
  /menu offline`: `` `offline` comes after `route`, before the method ``). `offline` is
  still a name everywhere else (`let offline = true`).
- **What the page gets**: an offline route's page carries a script of 280 bytes that
  registers Quillo's service worker, `/_quillo/sw.js`, for the program's base. Under a
  `content-security-policy` its hash is named in the policy, as Quillo's other scripts'
  are. Other pages get no script, and a program with no offline route has no worker and
  no marks on its answers.
- **The network first.** Online, each page comes from the server, as with no worker, so
  what a visitor sees is never older than it has to be. Each `200` page of an offline route
  is kept as it comes, in place of the copy before. When the network fails, the worker gives
  the kept copy, marked with when it was kept, `<html
  data-kept="2026-11-17T08:30:00.000Z">`, so that a sheet can say the page may be old
  (the mark goes in an `<html>` tag of the worker's before the page, which the browser
  merges with the page's own; nothing of the page is changed):

  ```css
  html[data-kept] body::before { content: "Offline: this page may be out of date"; display: block; padding: 8px; background: #fff3cd; }
  ```

  A proxy in front that answers `502`, `503` or `504` for an app that is down counts as no
  network for a page that was kept; one not kept gets the proxy's answer.
- **The offline page**: a page not kept (a route not tagged, a page with a parameter never
  opened) gets a page saying there is no connection, with a button to try again, named with
  `app.toml`'s `name` or "You are offline" (a `503`, `<html data-quillo-offline>`).
- **Kept ahead**: as the worker installs, it keeps each offline route with no parameter in
  its path (`/` and `/help/returns` above, not `/orders/:id`), with the files its page names
  (the runtime, the bundles, the sheets), so that a page never opened opens offline with its
  components working. A page with a parameter is kept once it is opened. The worker asks for
  them one at a time as a prefetch, which Quillo answers changing nothing (see
  [Prefetching](#routes-prefetching)): a route that would write, set a cookie or call a service is
  refused, and kept only once someone opens it; a redirect is skipped. A page that did not
  come (no network, a `502` to `504`) is asked for again on later navigations, 10 s, 30 s,
  90 s and 270 s after each try, five tries in all.
- **The rest goes to the network** as if there were no worker: `server fn` calls, forms
  sent, `fetch`. A `server fn` called offline gives its error value (`` server fn `save`
  could not be reached: ... ``), and the handler says what it likes. Only Quillo's files named by
  their content come from the worker's cache. Live data comes over a WebSocket, which a
  worker never sees, and the runtime opens it again at once when the browser says it is back
  online (see [Live data](#live-data)).
- **A new version**: the worker's text names the build, the program's files, its base and
  its offline routes, so a deploy that changes any of them installs a new worker at the
  browser's next check (on a navigation, and at least once a day). The new one keeps its
  pages, then drops the old copies; until all are kept (or five tries are done), the old
  copies serve. Every instance, and every server of one deploy, serves the same worker.
- **Bounds**: at most 100 pages of no one's, and 50 of each session, the oldest dropped
  first. An answer that is not a page to keep drops the copy of its address (a `404` now, a
  route no longer tagged); a download or an event stream is never copied.

**A visitor's own pages.** A page that reads who the visitor is (their orders) is kept
too, for that visitor's session alone:

- **Only through `session()`**: a page that read the visitor any other way (a cookie, a
  header, their address, there or in a function or middleware it calls) is not kept, and
  `quillo check` says where (`` `route offline GET /a` reads who the visitor is other than
  through `session()` (a cookie, a header, the request's own fields of theirs): its page is
  not kept for offline ``). Nor are a page that holds a form key, which a kept copy would
  send again (see [Idempotent requests](#idempotent-requests)), an answer that set or ended
  the session (a goodbye page would show the last person under the next one's name), a
  page with waiting parts (`defer`: they are made after the page's headers, so what they
  read is not known), and a page whose `cache-control` the program took away
  (`cache-control: null`), which a shared cache could give anyone (see [What a route
  returns](#what-a-route-returns)).
- **Kept per session**: the server names each session by a digest of its hidden id and of
  all it holds (16 characters, in `x-quillo-session`), and the browser holds that name in a
  cookie, `quillo_tag`, that the page can read (no `HttpOnly`), with the session cookie's
  path, `SameSite=Lax`, `Secure` and life; it comes and goes with the session cookie. A
  program with no offline route sets no such cookie.
- **Shown to that session only**: a session's kept page is given back hidden, with a short
  script first in its head, the gate, which shows it when the browser's `quillo_tag` names
  that session, and otherwise shows the offline page and drops that session's pages. The
  page's `content-security-policy` names the gate by its hash; a kept page whose policy
  would stop it anyway (a `sandbox` without `allow-scripts`) is not given back at all.
- **Any change to the session drops them**: a sign-out, a sign-in as another person, a
  sign-in in two steps, a cart written into the session. They are kept again, under the new
  name, as they are opened. The worker learns of it from the answer that changed the
  session (a page, the redirect after a form, a `server fn`'s), from the runtime on every
  page with components, and from the register script on offline pages without them.
- **A session the server no longer takes** (past its time, or signed with a
  `QUILLO_SECRET` since changed) is no one, as a request with no session cookie at all is:
  the next page drops every session's kept pages. A sign-out the worker could not see (a
  redirect to another site) is caught so, at the next page of the site. A session dropped
  is never kept again, even by an answer of its that comes late.

**Where there is no worker.** Under [`--watch`](#options-of-quillo-run) no page registers
it, and `/_quillo/sw.js` is a worker that takes itself away with what Quillo's worker kept,
so a browser that installed it in a run without `--watch` gets each change at its next
check (for a program that uses `notification`, it is the worker that shows notifications
and keeps nothing). The phone apps and `quillo app` have none: service workers do not run
on their schemes, and the program is on the device. A program that drops its last offline
route serves the worker that takes itself away too, so browsers that installed the one
before lose it and its copies (by the standard, a `404` there would leave the old worker
answering from its cache). An offline route in a mounted program is not kept, and `quillo
check` says so: put it in the main program.

### A service worker of your own

A program may serve a worker of its own instead (a game whose assets come from the cache
first, say), as a file a route serves and its pages register. Two workers cannot share a
scope, so such a program tags no route `offline`. Quillo's own addresses, under
`/_quillo/`, are of two kinds:

| Path | What a worker may do with it |
|---|---|
| `b/<hash>.js` (a page's bundle), `r/<hash>.js` (the runtime), `s/<hash>.css` (components' sheets), `t/<hash>.js` (translations), `i/<hash>.png` (an app's icons) | keep it for good, from the cache first: each is named by what it holds, and goes out `immutable` |
| `rpc/`, `live`, `events`, `ready`, `prefetch-used`, `src/` (under `--watch`), `quillo.js`, `sw.js`, `push`, and any other | never keep it: always to the network. `quillo.js` has no hash in its name, so a kept copy would outlive an upgrade of Quillo |

- **Pages from the network first**, the cache only when it fails. A worker that answers
  pages from its cache first shows the page as it was kept, with nothing to say so, and under
  `--watch` a page it answers cannot be refreshed: it reloads once, then stops and says so in
  the page's error overlay and the console.
- **Keep only the pages that are everyone's.** The Cache API keeps what it is given,
  whatever its `Cache-Control` says, `private` and `no-store` included. Quillo sends
  `cache-control: private` on every page made from who the visitor is (see [What a route
  returns](#what-a-route-returns)), so a worker reads that header itself before keeping a
  page. One that keeps a person's pages drops them when the person signs out (the sign-out's
  handler tells it, with `postMessage`), or the next person on the device reads them.
  `Clear-Site-Data: "storage"` would clear them too, but by the standard it also unregisters
  the worker, and with it the device's push subscription.
- **`x-quillo-offline` and `x-quillo-session`** are what Quillo's worker reads, sent only by
  a program with offline routes: `x-quillo-offline: 1` on an offline route's page of no
  one's, and `private` on one to keep for a session; `x-quillo-session` on a page of a
  session's and on an answer that set or ended a session, with the session's name, or `-`
  (no one) on a page whose request came with no session cookie, or one the server no longer
  takes. A program's own headers of these names are taken off its answers: only Quillo sets
  them.

```js
// public/sw.js, served by `route GET /sw.js = file("public/sw.js")` and registered by
// <script>navigator.serviceWorker.register("/sw.js")</script> on the program's pages
const CACHE = "shop-v1";
const NAMED = /^\/_quillo\/[brsti]\/[0-9a-f]+\.(js|css|png)$/;
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));
self.addEventListener("fetch", (e) => {
  const r = e.request, u = new URL(r.url);
  if (r.method !== "GET" || u.origin !== location.origin) return;
  const keep = (res) => { const copy = res.clone(); caches.open(CACHE).then((c) => c.put(r, copy)); };
  if (NAMED.test(u.pathname)) {
    // Named by what it holds: the cache first.
    e.respondWith(caches.match(r).then((hit) => hit || fetch(r).then((res) => (res.ok && keep(res), res))));
  } else if (r.mode === "navigate" && !u.pathname.startsWith("/_quillo/")) {
    // A page: the network first, kept unless it is someone's.
    e.respondWith(fetch(r).then((res) => {
      if (res.ok && !/private|no-store/.test(res.headers.get("cache-control") || "")) keep(res);
      return res;
    }).catch(() => caches.match(r).then((hit) => hit || new Response("<h1>Offline</h1>", { headers: { "content-type": "text/html" } }))));
  }
});
```

## Notifications

`notification` tells a person's devices something while the page is closed, or the browser
too (an order shipped, a reply, a reminder), as the system's notification, over Web Push.
It is not `notify`, which shows a note on a page that is open (see
[Components](#components)). The browser asks once, from a click, and the server sends when
it likes:

```quillo
db.open("shop.db")
db.exec("create table if not exists orders (id integer primary key, user_id text not null, shipped int not null default 0)")

fn staff(r) = session().role == "staff" ? r : respond(403, "staff only")

routes /staff | staff {
    route POST /orders/:id(int)/ship {
        let o = db.one("update orders set shipped = 1 where id = ? returning user_id", id)
        notification.send(o.user_id, { title: "Shipped", body: `Order ${id} is on its way`, url: `/orders/${id}` })
    }
}

component Alerts() {
    state on = notification.state()            // "granted", "denied", "default" or "unsupported"
    <button disabled={on == "granted"} @click={() => { on = notification.ask() }}>Tell me when it ships</button>
}

route GET /orders = <html><body><h1>Your orders</h1><Alerts/></body></html>
```

In the browser:

- **`notification.ask(options)`**, in an event handler, asks the person, registers Quillo's
  service worker if it is not there, subscribes this device with the server's key, and
  sends the subscription to the server, which binds it to `session().user`. It gives
  `"granted"`; `"denied"` or `"default"` when the person said no or closed the prompt;
  `"unsupported"` where the page has no push (below), and when the browser gives no answer
  in 20 s; or an error value with the server's reason (`notification.ask: no one to send to:
  the visitor is not signed in (session().user), ...`), the subscription it made undone.
  Asked again, it keeps the subscription it has, and makes a new one for a key that changed.
- **Ask first in the handler.** Browsers show the prompt only close to the click (Safari
  only within it), so `ask` comes before anything that waits, and `quillo check` warns of
  one after a `server fn` call, `http.*` or another call that waits in the same `@` handler
  (`` `notification.ask` comes after `save` waits in this handler: browsers show the
  permission prompt only close to the click, so ask first, then wait ``).
- **`notification.state()`** gives the same words without asking (`"default"` while this
  device has no subscription), in a handler or to set a state as the page starts (`state on =
  notification.state()`, `null` in the server's render and read in the browser).
  **`notification.stop()`** ends this device's subscription and gives `"default"` (`"denied"`
  where the person blocked notifications). The three are the browser's: `quillo check`
  refuses them in a route, a `server fn`, a tick, a job, the top level and a component's body
  (`state` there only to set a state).
- **Someone not signed in as `session().user`** (a guest's order, a program that names its
  people another way) is named by the server: `notification.ask({ user: ticket })`, where
  `ticket` is made in a route or a `server fn` with `notification.user(x)`. It is `x` sealed,
  good for 30 days, for this program and for the session it was made in, and it ends with
  that session: it works only from that session, and a request with no session makes none
  (an error value; a sign-in route's own `set_session` counts). A plain value is refused, as
  any page could then name someone else and be sent their notifications:

  ```quillo
  component Track(ticket: str) {
      state on = notification.state()
      <button disabled={on == "granted"} @click={() => { on = notification.ask({ user: ticket }) }}>Tell me when it ships</button>
  }

  route GET /thanks {
      let order = session().order                  // set at checkout, for a guest who is no user
      <html><body><h1>Order {order} is placed</h1><Track ticket={notification.user(`order-${order}`)}/></body></html>
  }

  fn staff(r) = session().role == "staff" ? r : respond(403, "staff only")

  routes /staff | staff {
      route POST /orders/:id(int)/ship = notification.send(`order-${id}`, { title: "Shipped", url: "/thanks" })
  }
  ```

On the server:

- **`notification.send(user, message)`** sends to each device `user` turned on in this
  program (and this tenant), and gives `{ sent, gone }`: how many push services took it, and
  how many said the device's subscription had ended (a `404` or `410`), which is then
  dropped. Another failure keeps the subscription and is said in the log. It works in a
  route, a `server fn`, a job and a tick, and is refused in a component, which runs in the
  browser too. It waits as `http.post` does, the server going on meanwhile, and reaches a
  person's devices at once: 8 at a time over kept connections, each given 10 s and all of them
  30 s, past which the rest are said in the log as not sent. Inside `db.transaction` it is an
  error value, as it would hold every other request while it waits: send after the
  transaction (`quillo check` warns where it sees one inside). A `GET` that sends is refused
  as a prefetch (see [Prefetching](#routes-prefetching)).
- **The user** is what `session().user` held when the device subscribed (or the `x` of
  `notification.user(x)`), compared as JSON: the int `7` and the text `"7"` are two people,
  so send what the session holds.
- **The message**: `title` (required, the line the device shows first), `body`, `url` (a
  path on this site, `"/orders/7"`, opened by a click; `/` when left out; under `--base` it
  carries the prefix), `tag` (a later message with the same tag takes its place on the
  device), `badge` (a whole number on the app's icon, where the system shows one; `0` takes
  it away), `silent` (`true`: no sound or vibration), `ttl` (how long a push service keeps
  it for a device that is away: seconds or a duration such as `"1h"`; a day when left out,
  four weeks at most) and `urgency` (`"very-low"`, `"low"`, `"normal"`, the default, or
  `"high"`). Another key, a missing title, a `url` that is not a path here (`https://...`,
  `//...`), or a value of the wrong kind is a mistake that stops the route; `quillo check`
  says the first two where the message is written out in the call.
- **One push message holds 3,800 bytes** as sent (one encrypted record of 4,096): a longer
  one is an error value, and nothing is sent (`notification.send: the message is 3801 bytes
  as sent, more than the 3800 one push message holds: shorten its body`).
- **To many people**, send in a loop in a job, not in a visitor's request, which would wait
  for every send:

  ```quillo
  db.open("shop.db")
  db.exec("create table if not exists customers (id integer primary key, name text not null)")
  db.exec("create table if not exists news (id integer primary key, text text not null, sent int not null default 0)")

  fn send_news() {
      for n in db.query("select id, text from news where sent = 0") {
          for c in db.query("select id from customers") {
              notification.send(c.id, { title: "News from the shop", body: n.text, url: "/news" })
          }
          db.exec("update news set sent = 1 where id = ?", n.id)
      }
  }

  schedule("1m", send_news)

  route POST /news { form: { text: str } } {
      db.exec("insert into news (text) values (?)", text)
      redirect("/news")
  }
  ```
- **A sign-out unbinds**: `end_session()` unbinds every device subscribed in that session,
  and so does any change of person by `set_session`, to or from no `user` included, so the
  next person on a shared device gets nothing meant for the one before. A browser holding a
  subscription checks it with the server: a page with components as it starts, and Quillo's
  worker on a page with none, at most once a minute. One bound to someone else than the
  session's person (after a cookie that simply ran out, say) is unbound there and then, and
  `notification.state()` says `"default"`, so the next person turns on their own. A ticket
  stays its person's: after a change of person on its session it is refused.
- **Limits**: a person has at most 20 devices in a program (and a tenant), the oldest
  replaced by a new one, and a session may turn devices on or off 30 times a minute (past
  that, a `429`). `stop` removes only this visitor's own.
  When they cannot be unbound (the database is down), the sign-out fails rather than leave
  them bound.
- **The key**: the push key (VAPID, P-256) is derived from the session key, `QUILLO_SECRET`,
  so every instance, and every server that shares it, signs alike. Changing `QUILLO_SECRET`
  ends every subscription: push services refuse the old ones, and a page's next `ask` makes
  a new one. Without `QUILLO_SECRET`, the key is made at start and a restart ends them all,
  which Quillo says as it starts (not under `--watch`, whose key is kept in `.quillo/`).
- **Where they are kept**: `_quillo_push`, where Quillo keeps its own tables (see [Quillo's
  own tables](#standard-library-quillos-own-tables)), made at the first subscription: each device's endpoint
  and keys, who it is for, the session and the site it was made on, and when a send last
  reached it. A program that never names `notification` has none of it: no table, no
  `/_quillo/push`, no worker for it. A variable named `notification` hides the module, and
  `quillo check` says so.
- **Push services are https on port 443**, on the public internet: a subscription to anything
  else (a private address, plain http, another port or scheme) is refused when the page sends
  it, and an address is checked again as it resolves when a message is sent; a redirect is not
  followed. `QUILLO_PUSH_LOCAL=1` lets plain http to this machine through, for tests that play
  the push service; `--watch` does not. The site a notification opens is the one the page was
  on (its `Host`), never what the page says.
- **`/_quillo/push`** is where pages send subscriptions: `GET` gives the server's key, `POST`
  binds one, `DELETE` unbinds one. It takes JSON from the program's own pages only (another
  site's is a `403`), as a `server fn` does.

What the device shows: the message goes as Declarative Web Push's JSON, which Safari shows
by itself; elsewhere Quillo's worker shows it, with its title, body, tag, `silent` and badge.
A click closes it and brings a window of the app to its `url`: one already there, another
moved there, or a new one; never another site's address. A program with offline routes has
one worker for both; one without gets a worker that shows notifications and keeps nothing.

Where there is no push, `ask`, `state` and `stop` say `"unsupported"`:

- **An iPhone or iPad** gives push only to a web app added to the Home Screen; in a Safari tab
  there is none.
- **The phone apps and `quillo app`**, for now: a phone app shows local notifications with
  `js("quilloNative.notify", ...)`, through the app's bridge. Push through Apple's and
  Google's services is to come.

`ask` on a page of a mounted program says `"unsupported"` too, as the mounted program has
no `/_quillo/push` of its own. A page whose scope already has a service worker of the
program's own gets an error value from `ask`, rather than have Quillo's replace it.
