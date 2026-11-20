# Feedback from making comparison videos in Quillo: six stacks, side by side

| | |
|---|---|
| App | The apps of six long videos and 19 Shorts in `~/dev/quillo-videos` (`videos/166`–`171`, `shorts/64`–`82`; published at [github.com/quillolang/video-apps](https://github.com/quillolang/video-apps)): each a small Quillo app (12–38 lines, one `main.qwe` and a migration) beside the same app in another stack, run and measured side by side. Against Spring Boot (an API token), Express (live updates by server-sent events), Hono's JSX (escaping and layouts), Fresh 2.3.3 (forms), Dioxus 0.7.10 (typed routes), Go 1.27.4's html/template (escaping in five places), and in the Shorts React, Vue, Svelte, Angular, Solid, Next.js, Axum, Leptos, Yew, Sycamore and Dioxus. |
| Quillo | `quillo 0.8.6 (238dac2, 2026-11-16)`, each app's own from its `get-quillo.sh` (the videos are pinned to it). Every repro below was also run on `quillo 0.8.8 (68da426, 2026-11-19)`, this repository's `target/release/quillo`, and all eight again on its rebuild `0.8.8 (fac998b, 2026-11-20)`: all still open. |
| Model | Claude Opus 5.5 (`claude-opus-5-5`) |
| Found by | This session (Q-1250–Q-1253, Q-1256), and quillo-videos-36 while making the Advanced and Database courses (Q-1254, Q-1255, Q-1257, Q-1258, Q-1259), each run again for this report on 0.8.6 and 0.8.8. |
| Tested with | `quillo check` on every app; Playwright's Chromium, headless (real clicks and hovers, `alert`s counted, computed styles measured); curl; `feedback/run.mjs` for the repros. Scale: a handful of rows; these are small apps, compared for behavior, not load. |

What worked well: next to six other stacks, Quillo's defaults held up where the others needed a line or a
library. Text and attributes in a view were escaped byte for byte as Hono's JSX and Go's html/template do them.
A `javascript:` link was blocked in any case, with a leading space or a tab inside, where Hono's JSX kept it and
clicking ran the code. A value in a `<script>` came out as a JavaScript string with `<` as `<`, as Go's does.
A form posted from another site's page was refused (403) with a message that says why, where Fresh's starter took
it until `app.use(csrf())` was added. A typed route refused `/trails/02` and `/trails/+2` and answered real 404s,
where Dioxus's web build answers 200 for every address. `live()` kept every open page current, a page opened late
and a page across a server restart included, where Express needed a `Set` of responses, `flushHeaders()`,
`req.on('close')` and, behind `compression()`, `res.flush()`. The one place a comparison found Quillo behind is
Q-1250: a style.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-544 | A typed form pattern that doesn't match answers a bare `400 bad request: no route pattern matches this request`; the reason is in the log (and in the answer only under `--watch`, as the fix chose). For a form a person fills, videos 154 and 169 take every field as `str` and check them in one function, to give a message by each field. | 1 (designed around) |
| Q-542 | The route's `int` is strict; `int()` is not (the fix kept it). Video 169 checks its form's guests with `int()`, so `" 5"` was taken as 5 where a field typed `int` would refuse it. Written up for the guide as Q-1253. | 0 (found by probing) |

## New issues

### Q-1250. A string in `style=` is not escaped as CSS: a visitor's value adds declarations and can cover the page

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | browser (a computed style), found by comparing with Go's html/template | 1 |

Video 171 puts each of five note fields in a different place: the text between tags, the name in `title`, the link
in `href`, the color in a style, and the latest name in a script. With the color written the way a JSX habit and
the guide's "`{expr}` interpolates and escapes" suggest, `style={`color: ${n.color}`}`, a color of
`red; position: fixed; inset: 0; background: red` went into the page as written: the attribute was escaped for HTML
(there is nothing to escape), not for CSS. In Chromium that note covered the whole 800x600 window in red. Go's
html/template, given the same template, wrote `style="color: ZgotmplZ"`. A style map, `style={{ color: c }}`,
escapes the same value (`red\3b  position: fixed\3b …`, Q-668's fix), and `quillo check` says nothing about either.

```quillo
route GET / ? { c: str = "green" } = <html><body>
    <p style={`color: ${c}`}>a note</p>
</body></html>
```

`GET /?c=red%3B%20position%3A%20fixed%3B%20inset%3A%200%3B%20background%3A%20red` answers
`<p style="color: red; position: fixed; inset: 0; background: red">a note</p>`, on 0.8.6 and on 0.8.8
(`repros/Q-1250-a-string-in-style-is-not-escaped-as-css.qwe`).

Cost: nothing, until the comparison: the board looked right with ordinary colors, and every other place a value
went was handled. The app's fix (in the video) is a list of four colors and a 400 for anything else.

| Impact | |
|---|---|
| **Functionality** | Any declaration a visitor types becomes part of the page's style: layout, position, size, colors. |
| **User** | One visitor's note can cover the page for everyone, or lay a look-alike over real content (a fake notice or form). |
| **Requests** | No change on the server. A declaration such as `background: url(https://other.example/p.png)` would make each visitor's browser fetch another site (not tried). |
| **Security** | Defacement and overlays; no script runs (browsers don't run code from CSS). The fix the guide already has, a style map, isn't the obvious way to write it. |

**Suggestion:** escape what is interpolated into a `style` attribute's template string as Q-668 does for a style
map's values (`{ } ; \`), so `color: ${c}` stays one value; or have `quillo check` warn on an interpolated string in
`style=` and point to a style map. In the guide's "Answers and escaping", say that a style string is escaped for HTML
only.

### Q-1258. The warning that a group doesn't guard its pages' server fns sees only the route's own body

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | `quillo check` (no warning), found by quillo-videos-36 in the Advanced course's Help Desk; checked again for this report | 1 |

`quillo check` warns when a guarded group's page calls a server fn, since "a group guards its routes, not the
server fns their pages call: anyone can call it at POST /_quillo/rpc/…". It does when the route writes the component in
its own body. It doesn't when the route calls a helper that writes it, which is how a page with a layout is often
written. In Help Desk, `route GET / with db, random = team_home()` (a `fn` whose markup holds `<TicketList/>`, which
does `live(load_tickets)`) gave no warning; written as `= <Page title="Desk"><TicketList/></Page>`, it warned. The
same for `Replies` and `load_replies` under `route GET /t/:id(int)`.

```quillo
fn agent(r) = session().name != null ? r : reply 303 redirect("/")
server fn load_tickets() -> list = ["printer jam"]
component TicketList() {
    state ts = load_tickets()
    <ul>{for t in ts ?? [] => <li>{t}</li>}</ul>
}
fn team_home() = <html><body><TicketList/></body></html>
routes /desk | agent {
    route GET / = team_home()
}
```

`quillo check`: `main.qwe: ok (1 route, 3 functions, 1 component)`, no warning, on 0.8.6 and fac998b
(`repros/Q-1258-group-guard-warning-misses-helpers/`). Run, signed out: `GET /desk` is a 303, and
`POST /_quillo/rpc/load_tickets` with `[]` answers `{"ok":["printer jam"]}` (200). With `<TicketList/>` in the route's
body instead, the same check warns (``main.qwe:3: warning: `load_tickets` is called by `GET /desk`'s page, which `agent`
guards, …``).

| Impact | |
|---|---|
| **Functionality** | No change: the guide says each server fn checks its caller, and this one doesn't. |
| **User** | A developer who trusts the warning, and gets none, thinks the group protects the page's data. |
| **Requests** | Anyone can call the server fn at `/_quillo/rpc/<name>`, signed in or not. |
| **Security** | The guarded page's data is readable by anyone who calls its server fn (here, every ticket). |

**Suggestion:** follow the route's calls into the `fn`s (and views) it uses when finding the components its page
writes, as `quillo check` already does for the route's own body.

### Q-1254. With a database per tenant, `/_quillo/ready` says the database is "down", so the app is never ready

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading `/_quillo/ready`'s answer (found by quillo-videos-36 while making the Advanced course's Help Desk; checked again for this report) | 1 |

The guide: `/_quillo/ready` checks "its database (`select 1`), each service it declares … and the program's own
checks", 200 or 503, "for an orchestrator's readiness probe". With `db.open("data/{tenant}.db")` and
`tenant_from(...)`, a tenant's pages work, and ready answers
`{"ready":false,"checks":{"database":"down"}}` with a 503: there is no one database to run `select 1` on. The
course's Help Desk, with two services, gives `{"ready":false,"checks":{"database":"down","service mailer":{"ready":true,…},…}}`;
a one-database app gives `{"ready":true,"checks":{"database":"ready"}}`.

```quillo
tenant_from("localhost")
db.open("data/{tenant}.db")
route POST /teams = new_tenant("acme", () => "made")
route GET / with db = `tenant ${tenant()}: ${len(db.query("select 1 as one"))} row`
```

After `POST /teams` (made, 200) and `GET /` for `acme.localhost` (`tenant acme: 1 row`, 200), `/_quillo/ready` with
`QUILLO_HEALTH_TOKEN` answers `{"ready":false,"checks":{"database":"down"}}` (503), on 0.8.6 and 0.8.8
(`repros/Q-1254-ready-says-a-tenant-database-is-down/`).

| Impact | |
|---|---|
| **Functionality** | The readiness check is false for every multi-tenant app, whatever its tenants' databases do. |
| **User** | An orchestrator (a Kubernetes readiness probe, a load balancer that reads it) never sends the app traffic; the operator sees "database: down" for a database that is fine. |
| **Requests** | Every ready probe is a 503. |
| **Security** | No change. |

**Suggestion:** for a `{tenant}` target, check what a tenant needs: that the folder (or the server, for
PostgreSQL schemas) is there and writable, or the tenants already open, and say `"database": "ready (per tenant)"`;
and say in the guide what ready checks for tenants. There is no workaround in the program: a check of its own,
`ready("tenants", () => true)`, is listed beside the database's, and the answer stays
`{"ready":false,"checks":{"database":"down","tenants":"ready"}}` (503), on 0.8.6 and 0.8.8.

### Q-1255. `/_quillo/ready` is logged and counted in `quillo_requests_total`; the guide says it is neither

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `--metrics` (found by quillo-videos-36 while making the Database course; checked again for this report) | 1 |

The guide's "Stopping and health": `/_quillo/health` and `/_quillo/ready`, "Neither request is logged or counted."
With `--metrics`, two `GET /_quillo/ready` and nothing else make `quillo_requests_total{status="2xx"} 2`, and the log
has a line for each. `/_quillo/health` (the script waits on it, several times) is not counted. A 503 from ready, as in
Q-1254, counts as a `5xx`.

```quillo
db.open("app.db")
route GET / = "hi"
```

`quillo run main.qwe --metrics`, then `curl /_quillo/ready` twice: `quillo_requests_total{status="2xx"} 2` and 2 log
lines, on 0.8.6 and 0.8.8 (`repros/Q-1255-ready-is-logged-and-counted/`).

| Impact | |
|---|---|
| **Functionality** | Request counts include the orchestrator's probes, one every few seconds per instance. |
| **User** | A dashboard's request rate is the probes' on a quiet app, and with Q-1254 an error-rate alert fires on probes alone. |
| **Requests** | No change on the wire. |
| **Security** | No change. |

**Suggestion:** leave `/_quillo/ready` out of the request counter and the log, as `/_quillo/health` is (or give it
a counter of its own), as the guide says.

### Q-1256. `site_headers({ "server": null })` does not take away `server: Quillo`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl, while comparing with helmet (which takes away Express's `X-Powered-By`) | 1 |

The guide: `site_headers(path, headers)`, "null takes one of Quillo's away", and "Every answer carries
`server: Quillo` (unless the route sets `server`)". With `site_headers({ "server": null, "x-frame-options": null })`,
`x-frame-options` is gone from the page but `server: Quillo` stays. A route's own `respond(200, "hi", { "server": null })`
does take it away, so it can be done one route at a time, not for the site.

```quillo
site_headers({ "server": null, "x-frame-options": null })
route GET / = <p>hi</p>
route GET /own = respond(200, "hi", { "server": null })
```

`/` answers with `server: Quillo` (and no `x-frame-options`); `/own` with no `server`, on 0.8.6 and 0.8.8
(`repros/Q-1256-site-headers-cannot-take-away-server/`).

| Impact | |
|---|---|
| **Functionality** | One of Quillo's headers can't be set or taken away for the whole site, unlike the others. |
| **User** | A developer told by a scanner to hide what the server runs can't do it in the one place meant for it. |
| **Requests** | Every answer says `server: Quillo`. |
| **Security** | Small: it tells a scanner what to try. helmet takes Express's `X-Powered-By` away by default. |

**Suggestion:** let a `site_headers` rule take away or replace `server` as it does Quillo's other headers; or say in
the guide that only a route can.

### Q-1259. In an XML answer, `<link>` is written as HTML's empty element: an Atom feed isn't XML, and an RSS channel can't be written

| Kind | Detected by | Attempts |
|---|---|---|
| silent | an XML parser on the answer, and `quillo check` (found by quillo-videos-36 making the Advanced course's lesson 18; checked again for this report) | 1 |

The guide's Values table says `<rss>` and `<feed xmlns>` are served with their XML types, which is what happens. But
`<link>` is written as HTML writes it, with no end. In an Atom feed, `<link href="…"/>` comes out as
`<link href="…">` before `</feed>`, so the answer isn't XML (Python's ElementTree: "mismatched tag"); a feed reader
refuses it. An SVG's `<image href="x.png"/>` comes out as `<image …></image>`, which is fine, so Quillo knows the
answer is XML there. An RSS 2.0 channel needs `<link>` with the site's address as its text, and
`<link>http://example.com/</link>` fails `quillo check` with ``<link>` has no end tag: remove `</link>``. The way
out is `{raw("<link>…</link>")}`. The course's lesson uses a sitemap instead of a feed because of it.

```quillo
route GET /atom.xml = <feed xmlns="http://www.w3.org/2005/Atom"><title>Help</title><link href="http://example.com/help"/></feed>
```

The answer is `application/atom+xml`, and its body ends `<link href="http://example.com/help"></feed>`: "mismatched tag:
line 1, column 100", on 0.8.6 and fac998b (`repros/Q-1259-link-in-xml-is-html-void.qwe`). And
`route GET /r = <rss version="2.0"><channel><title>Help</title><link>http://example.com/</link></channel></rss>` fails
check with ``main.qwe:2: error: `<link>` has no end tag: remove `</link>` ``, on both.

| Impact | |
|---|---|
| **Functionality** | Neither feed format can be written in markup: Atom comes out broken, RSS doesn't pass check. |
| **User** | A feed reader shows nothing, or an error, for the Atom feed; a developer has to fall back to `raw`. |
| **Requests** | No change. |
| **Security** | Small: `raw` means writing the feed's text unescaped by hand, the thing markup was there to avoid. |

**Suggestion:** inside an XML root (`rss`, `feed`, `urlset`, `svg`, anything with `xmlns`, or after `<?xml`), write
`<link>` and every other HTML void name as an ordinary XML element (`<link …/>` when empty, holding text when given
some), and let `quillo check` accept `<link>text</link>` there.

### Q-1257. With two routes on one path, a 400 names the first route's miss, however far off that route was

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the log (and the answer under `--watch`), found by quillo-videos-36 in the Advanced course; checked again for this report | 1 |

Since Q-544 the 400 "is logged with the route that came closest and its first miss". With a form route and a JSON
route on the same path, in that order, a JSON post that leaves out `message` is logged as
`` `form.name` is missing, and must be a str ``: the form route's first miss, though the request was JSON and the JSON
route missed only `message`. A JSON post with `"updates": "yes"` gets the same line. With the two routes swapped, the
JSON posts are told `json.message` and `json.updates` rightly, and a form post missing `updates` is told
`` `json` is missing, and must be what its pattern takes ``. So the route named is the first in the file, not the
closest. quillo-videos-36 saw the same under `--watch`, in the answer as well as the log.

```quillo
route POST /contact { form: { name: str, email: str, message: str, updates: bool } } = "form"
route POST /contact { json: { name: str, email: str, message: str, updates: bool = false } } = "json"
```

`curl -H 'content-type: application/json' -d '{"name":"Bo","email":"bo@example.com"}'` and the same with
`"message":"hi","updates":"yes"`: both 400, both logged `` POST /contact (main.qwe:8): `form.name` is missing, and must
be a str ``, on 0.8.6 and on this repository's build (fac998b) (`repros/Q-1257-closest-route-is-always-the-first/`).

| Impact | |
|---|---|
| **Functionality** | No change: both requests are rightly refused. |
| **User** | The developer is sent to the wrong route and the wrong field; under `--watch`, so is whoever reads the answer. |
| **Requests** | No change. |
| **Security** | No change. |

**Suggestion:** measure "closest" across the routes on the path: a route whose body kind (`form`, `json`) matches the
request's `content-type` before one that doesn't, then the one that matched the most fields.

### Q-1252. A file sent in a field the form pattern names `str` is "missing" in the log, and a default takes its place without a word

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run-time log, while comparing with Fresh | 1 |

Video 169 posts a file as the sign-up's `name` (multipart, the way `<input type="file" name="name">` sends it),
because Fresh's guide reads a field with `form.get("name")?.toString()`, which turns a file into the text
`"[object File]"`, and the Fresh app saved a hiker called `[object File]`. Quillo never makes a file into text,
which is right. But the file is only in `request.files`, so to the form pattern the field is missing: with a
default (`name: str = ""`), the route runs with the default and logs nothing; without one, the answer is the bare
400 (Q-544) and the log says ``400: POST /b (main.qwe:10): `form.name` is missing, and must be a str``, which sends
whoever reads it looking for a field the form forgot to send.

```quillo
route POST /a { form: { name: str = "(none)" } } = `name is ${name}`
route POST /b { form: { name: str } } = `name is ${name}`
```

`curl -F name=@hello.txt` to `/a` answers `name is (none)` (200), to `/b` the bare 400, on 0.8.6 and on 0.8.8
(`repros/Q-1252-a-file-in-a-str-field-is-missing/`, with `steps.sh`). The file is at
`request.files.name` (`{"field":"name","filename":"hello.txt","type":"text/plain","size":6,…}`).

Cost: one look at the log; the video's app was fine (its default `""` gave "Your name, please").

| Impact | |
|---|---|
| **Functionality** | A file part is invisible to a form pattern; with a default, the route can't tell "not sent" from "sent as a file". |
| **User** | A developer debugging an upload form reads "missing" for a field that was sent. Visitors: no change. |
| **Requests** | No change. |
| **Security** | No change: a file never becomes text, unlike `toString()` in Fresh's guide. |

**Suggestion:** when the field is in `request.files`, say so: ``form.name` is a file (hello.txt), not a str; it is
in request.files.name``, in the log and under `--watch`, also when a default was used.

### Q-1253. `int()` takes `" 5"`, `"5 "`, `"+2"` and `"02"`; the guide says "text of digits", and a route's `int` refuses all four

| Kind | Detected by | Attempts |
|---|---|---|
| docs | probing, while comparing with Fresh's `Number()` | 0 |

The guide: "`int(x)`: A number cut toward zero, `true` as 1, or text of digits; `"1.0"` and a number too large for
an int are error values." On 0.8.6 and 0.8.8, `int()` also takes spaces at either end, a leading `+`, and leading
zeros. Since Q-542, a route's `int` takes "one written form" and refuses them. The guide's advice for a form a person
fills (take fields as `str`, check them in one function) therefore accepts what the typed route refuses: in video
169, `guests=" 5"` became 5 guests; in video 154, a fallback route that checked with `int()` found nothing wrong.

```quillo
for v in ["5", " 5", "5 ", "+2", "02", "1e1"] {
    print(`"${v}" -> ${is_err(int(v)) ? "error" : int(v)}`)
}
```

prints `5`, `5`, `5`, `2`, `2`, `error` (`repros/Q-1253-int-takes-spaces-and-signs.qwe`). `1e1`, `0x0A` and `12.0`
are error values, which the video credits against JavaScript's `Number()` (10, 10, 12).

| Impact | |
|---|---|
| **Functionality** | Two answers to "is this a whole number": the pattern's and `int()`'s. |
| **User** | No change for visitors; a developer's checks are looser than the guide says. |
| **Requests** | No change. |
| **Security** | No change. |

**Suggestion:** say exactly what `int()` takes in the guide (and `float()`), or give it the pattern's strict form
(an option, or a function like `parse_int`), so a check by hand can match the route's.

### Q-1251. `quillo check` passes a link to an address no route answers

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | browser (a 404 on click), found by comparing with Dioxus | 0 |

Video 170 gives each side the same typo in its list's links. In Dioxus, a link names the route
(`Link { to: Route::Trail { id } }` for `Route::TrailPage`), and the build fails:
``error[E0599]: no variant named `Trail` found for enum `Route` ``. In Quillo a link is a string, and
`href={`/trail/${t.id}`}` beside `route GET /trails/:id(int)` passes `quillo check`; the link is a 404, found only
when someone clicks it.

```quillo
route GET / = <ul>{for id in [1, 2] => <li><a href={`/trail/${id}`}>trail {id}</a></li>}</ul>
route GET /trails/:id(int) = `trail ${id}`
```

`quillo check` prints `ok (2 routes, 0 functions, 0 components)` on 0.8.6 and 0.8.8
(`repros/Q-1251-check-passes-a-link-to-no-route.qwe`).

| Impact | |
|---|---|
| **Functionality** | No change: the program does what it says. |
| **User** | A visitor gets a 404 from a link on the site. |
| **Requests** | One 404 per click. |
| **Security** | No change. |

**Suggestion:** in `quillo check`, for an `href`, `redirect()` or `navigate()` whose value is a literal, or a
template string with a literal start, beginning with `/`: warn when no route can match it (`/trail/` against `/`,
`/trails/:id(int)` and the program's file routes), with "did you mean /trails/…?".

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `style={`color: ${n.color}`}` | JSX habit; the guide says `{expr}` interpolates and escapes | escaped for HTML only: Q-1250, found only by comparing with Go | the finding |
| `{ form: { name: str = "", guests: str = "" } }` | wanted a missing field to get a message, not a bare 400 | worked, though the guide shows a default only in a query pattern (`? { page: int = 1 }`) | 0 |
| `<script>const latest = {latest};</script>`, `latest` possibly `""` | the guide: `<script>{x}</script>` writes `x` as JSON; Q-682 said `""` used to write nothing | `""` is written as `""` on 0.8.6: fixed | 1 probe |
| `has(bad, "name") && <strong> {bad.name}</strong>` | copied from an earlier video's form | worked | 0 |
| `contains(COLORS, color)` | the guide's table (`x in xs` is `contains(xs, x)`) | worked | 0 |
| A scraping regex `/<li[^>]*>/` in a test script (not Quillo) | quick | matched Fresh's `<link>` tags; `/<li(?:\s[^>]*)?>/` | 1 run |
| The narration said "browsers won't open a `data:` link" before it was tried | assumed | tried in Chromium only ("Not allowed to navigate top frame to data URL"), and the video says "Chrome" | 1 probe |

## Limitations

- A link is a string, so a link to no route is found when clicked (Q-1251). The apps rely on running every page in a
  browser.
- A style string isn't CSS-escaped (Q-1250): the video's app takes a color from a list.

## Edge cases checked that behave well

On 0.8.6, and in the repro run on 0.8.8 where marked.

| Case | Result |
|---|---|
| `<script>alert('text')</script>` as text, `<img src=x onerror="…">` as text, `x" onmouseover="…` in `title` | escaped; byte for byte as Hono 4.13.19's JSX; as Go's html/template but for `&quot;` against `&#34;` |
| `href` of `javascript:alert(1)`, `JavaScript:…`, ` javascript:…` (a leading space), `java\tscript:…`, `vbscript:…` | `about:blank#blocked`, also on 0.8.8; Hono kept all five, and a click on `javascript:alert('link')` ran it |
| `href` of `data:text/html,<script>alert(1)</script>` | kept, escaped (also on 0.8.8); Chromium refuses to open it from a link |
| `</script><script>alert('script')</script>` and `""` as a value in a `<script>` | a JavaScript string with `<` as `<`; `""` as `""` (also on 0.8.8) |
| A style map's value with `;` | `\3b` (Q-668), also on 0.8.8 |
| A form posted as a page on another site opens (`127.0.0.1:9000` to `localhost`, in Chromium) | 403, "this site does not take form posts from other sites" (also on 0.8.8 by curl); Fresh 2.3.3's starter took it until `app.use(csrf())` |
| A file posted as a `str` field | never becomes text (Fresh's guide made it `"[object File]"`), but see Q-1252 |
| `int("1e1")`, `int("0x0A")`, `int("12.0")` | error values, also on 0.8.8 (JavaScript's `Number()` gives 10, 10, 12) |
| `route GET /trails/:id(int)` with `02`, `+2`, `abc`, `0`, `999`, `4294967296` | 404 each, the right page for each (no such page, no such trail) |
| `live(conditions)` with two tabs, a third opened late, and a server restart under an open tab | every open tab updated with no reload; the late tab current; updates went on after the restart |
| A typed form pattern with defaults (`str = ""`) and a field left out | the default |
| `site_headers({ "content-security-policy": "default-src 'self'" })` on a page with a component (video 174) | sent with `script-src 'self' 'sha256-…' 'sha256-…'` and a `style-src` hash for Quillo's own scripts: the component's button worked, where helmet 8.3.0's `script-src 'self'` refused Express's inline script |
| A page on another site framing a Quillo page (an iframe from `127.0.0.1:9000`) | refused (`x-frame-options: SAMEORIGIN` by default); Express 5.3.0 as it comes was framed |
