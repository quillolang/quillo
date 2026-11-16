# Feedback from designing auto-CDN: public files and bundles reach visitors from a CDN, with no change to the program

| | |
|---|---|
| App | None: a design study. The question: what would it take for a Quillo site to have its files and bundles served from a CDN with nothing for the developer to do, and is a site safe behind a CDN today |
| Quillo | `0.7.0`, the release build of branch feedback55's working tree (it reports `e6f198f`, 2026-11-07) |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | A small program behind three caches on this Mac: nginx 1.31.8 keeping to the standard rules, nginx set up as a model of Cloudflare's Cache Everything (it ignores `Vary`, and keeps a 200 for an hour), and Limer 0.1.0 with `cache { ttl 5m }`. `curl` and Python for the requests, headless Chrome (`feedback/tools/drive.mjs`) for the content policy. The documentation of Cloudflare, Bunny, CloudFront and Fastly, and their published address lists. The scripts are at the end |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section. Numbers start at Q-903, after FEEDBACK55.md's Q-875. Under the rule of 2026-11-05, nothing here is built until main is in sync, every test (CI too) is green and the docs are aligned.

**No real CDN was used.** There is no account and nothing was sent to one. The Cache Everything model stands for what Cloudflare's documents say, and a real run should confirm it before anything here is called proven. A local rig also cannot show distance, so this report has no number for how much faster a far visitor gets a page.

What worked well: Quillo is most of the way there for its own files. Bundles, the runtime, sheets and translations have addresses named by their content and say `public, max-age=31536000, immutable`, and a standard cache kept them. A bundle's name is the same after a restart. A page made from the session says `private`, and none of the caches gave it to anyone else. The session cookie is `__Host-quillo_session`, which a browser never sends to another host, so a `cdn.` host is cookie-free with no work. A live request is held 25 seconds, far inside Cloudflare's 125. WebTransport takes its host from `location.hostname`, so a page never shows the origin's address. And `--trust-proxy` reads `X-Forwarded-For` from the right, as a chain of proxies must be read.

## The answer

**Auto-CDN should pull, not push, and it should be one setting with no change to the program.** Quillo has no build step, and `quillo run` is the deploy, so there is no moment to upload anything. A CDN that pulls needs no upload. Addresses named by their content need no purge. Quillo already knows the two things a CDN needs to be told: which answers are public, and what each file holds.

What the developer writes:

```sh
# .env, or --cdn on the command line. The program does not change.
QUILLO_CDN=https://cdn.example.com          # any CDN that pulls from this server
QUILLO_CDN=bunny://$BUNNY_KEY@shop          # the same, and Quillo sets the CDN up (Part D)
```

| Part | The developer writes | Quillo does |
|---|---|---|
| A. Every answer says who may keep it | nothing, on every server | a personal answer is `private` whatever else was said; pages say what they are; a CDN in front is safe |
| B. Public files named by their content | nothing, on every server | finds the routes that serve public files; writes `/_quillo/a/<hash>/static/css/style.css` for `/static/css/style.css`; the answer is `immutable` |
| C. The CDN's host | `QUILLO_CDN=https://...` | writes those addresses on that host; adds CORS, `integrity`, `preconnect` and the host in the page's content policy; falls back to its own address when the CDN fails |
| D. Drivers | a token | checks or creates the pull zone and its rules, trusts the CDN's addresses, keeps them current, hides each provider's quirks |
| E. Public pages at the edge (later) | `route edge GET /pricing = ...` | tags the page with the tables it read, and purges those tags when something writes them |

Parts A and B help every Quillo site, with or without a CDN, and Part A is needed today by anyone who has already put one in front.

Nine things stand in the way. Each is an issue below:

1. **A signed-in file reaches a visitor with no session** (Q-903): `cache: "1y"` or a route's own `public` beats the session read, and only `Vary: Cookie` is left, which Cloudflare ignores.
2. **The guide's own content policy stops every component** (Q-904): `default-src 'self'` refuses Quillo's inline loader, and nothing says so.
3. **A page made from `locale()`, `request.ip` or a header is not marked** (Q-905): a French page went to an English visitor through Limer's `cache { ttl 5m }`.
4. **A public file cannot be both kept and current** (Q-906): `no-cache` reached the origin 100 times of 100, and `cache: "1y"` kept the old file.
5. **Nothing can put a file on another host** (Q-907): `--base` is a path only.
6. **Bundles are gzipped again for every request** (Q-908): the runtime takes 1.97 ms instead of 0.10.
7. **A bundle is known only to the run that rendered its page** (Q-909): 111,756 bytes instead of 678.
8. **`--trust-proxy` wants the CDN's addresses by hand** (Q-910): 22 networks for Cloudflare, 585 addresses for Bunny, and they change.
9. **A page behind a CDN waits for WebTransport** (Q-911): up to 1.5 seconds on each page that opens a channel.

## Two ways to sit behind a CDN

| | Asset host (`cdn.example.com`) | Whole site (the site's DNS goes to the CDN) |
|---|---|---|
| What passes through the CDN | hashed files and bundles only | every request |
| Can a page leak | no: pages never pass through it | yes, if a rule keeps HTML or an answer says the wrong thing |
| Live data, channels, `server fn` calls | straight to the origin, as today | through the CDN: WebSockets and 25-second polls work, WebTransport does not (Q-911) |
| Needs a change in the page | yes: Parts B and C | no: Part A is enough |
| Pages at the edge (Part E) | not possible | possible |

**Recommendation: the asset host is what `QUILLO_CDN` does.** It is the shape where a mistake cannot show one visitor's page to another, which is why it should be the automatic one. The whole-site way stays open to anyone who sets it up, and Part A is what makes it safe.

## Part A: every answer says who may keep it

Today a route's own header wins, and pages with nothing to say send nothing, so a proxy's default decides. The rule should be that Quillo says, for each answer:

| Answer | `Cache-Control` | Change |
|---|---|---|
| Hashed address, hash right | `public, max-age=31536000, immutable` | none |
| Hashed address, hash wrong (a page from before a deploy) | `no-cache` | none: this already keeps a CDN from storing the wrong thing for good |
| A public file at its own address | `no-cache`, with its `ETag` | none |
| A page or file that read the visitor | `private`, whatever the route or `cache` said | new (Q-903), and more reads count (Q-905) |
| A page that read nothing of the visitor | `no-cache` | new: today nothing, so a proxy's default TTL applies |
| A page with a form key | `no-store`, always | new: today only when it said `public` or `max-age` (`src/server.rs:473`, read and not run), so a proxy's default TTL can keep the key that stops a payment being made twice |
| `/_quillo/rpc`, `live`, `wt`, `events`, `ready`, `health`, `metrics` | `no-store` | check each |

**`private` is the defence, not `Vary`.** Cloudflare uses `Vary` only for `accept-encoding` unless a rule says otherwise, and Bunny does not follow it at all and builds its key from its own settings. `Vary: Cookie` stays for caches that read it, but nothing should depend on it.

One provider needs more than a truthful answer. CloudFront's managed `CachingOptimized` policy has a minimum TTL above zero, and AWS says a minimum above zero keeps content even under `no-cache`, `no-store` and `private`. Its key also leaves out cookies. So that policy on a whole site would keep signed-in pages. That is a reason for the asset host, and for Part D setting CloudFront up rather than a person.

## Part B: public files named by their content

### B.1 Which routes count

Quillo finds them when it loads, from the routes as written:

```quillo
route GET /static/*path = files("public", path)                  // yes
route GET /logo.png = file("public/logo.png")                    // yes
route GET /build/*path = files("dist", path, { cache: "1y" })    // yes
routes /docs | signed_in { route GET /*path = files("docs", path) }   // no: middleware
route GET /mine/*path { files(folder_of(session()), path) }      // no: not a literal folder
```

A route counts only when its whole body is `files` or `file` with a literal folder or path, and it has no middleware, no group pattern and no `with`. **Anything else stays where it is.** A file behind a sign-in never gets a hashed address and never goes to the CDN. That has to be a test of its own.

### B.2 The address

`/static/css/style.css` is written `/_quillo/a/<hash>/static/css/style.css`.

- **The hash is in the path, not the query.** CloudFront's `CachingOptimized` leaves query strings out of the key, so `?v=abc` would serve the old file after a change. The path also keeps the extension, which Cloudflare needs: it decides by extension, not by type.
- **The door goes through the same `files()`.** `/_quillo/a/<hash>/<path>` finds the route for `<path>`, then runs what the route runs. `.env`, `.git/`, the program's own files and Q-701's checks must hold there with no second copy of them. If the door ever had its own file reading, it would be a way around FEEDBACK51's fixes.
- **Hash right: `immutable`. Hash wrong: the current file with `no-cache`**, as a bundle does today. A page from before a deploy still gets a file.
- **One hash per file, not one per folder.** A deploy that changes one image then costs visitors one image, and uploads into `public/` do not rename everything else.
- **What is hashed:** SHA-256 of the bytes, taken once and kept by path, length, time and identity, as `statics::FILES` keys its copies. The address uses its first 16 hex digits, and `integrity` the whole. Files over 8 MiB keep their own address: nobody should wait at a page render for a video to be read.

### B.3 Where addresses are written

`--base` already has the place: `write_url_attr` and `write_attr` (`src/interp.rs:5321`, `5483`, `5499`) handle `href`, `src`, `action`, `formaction` and `poster`. The same place takes a public file's address. `srcset` joins them.

- **Literal addresses get the hash.** Dynamic ones (`src={"/static/" + name}`) get only the host, on the server and in the browser alike, so they stay correct and are asked about at the edge.
- **In the browser**, a component that shows `<img src="/static/a.png"/>` must write what the server wrote, or `Inst.adopt` makes the element again. The page carries a small map of the literals its bundle can show, beside the hand-off. It goes in the page and not the bundle, so a changed image does not change every bundle's name.
- **In a stylesheet**, `url(...)` and `@import` are rewritten when the door serves it, and the stylesheet's hash is taken after that, so a changed font renames the sheet that names it. A relative address that was not rewritten still works: the door serves the current file, with `no-cache`.
- **The view cache** keeps HTML by arguments and globals (`## Cached views` in the guide), so an address in it would go stale. A generation number for the table of hashes counts as a global such a view read.
- **Not rewritten:** a `fetch("/static/data.json")` in the page's own script, inline `style=`, `<meta>`. They go on working at their own address.
- **Off under `--watch`**, where a stylesheet is swapped by its path, and in `quillo serve`, which serves files as they are.

The cost is a map lookup for each address on a page, and a check that the file has not changed, at most once a second for each. That is on the render path, so it must be measured with `bench/vs-python` before it lands.

## Part C: `QUILLO_CDN=https://cdn.example.com`

The setting is `--cdn URL`, or `QUILLO_CDN`, as `QUILLO_DB` is for the database. It takes an `https` address, and is ignored under `--watch` and in desktop and phone apps.

- **Addresses.** The seven places in `src/server.rs` that write `{base}/_quillo/...` (`503`, `534`, `537`, `539`, `549`, `551`, `578`) go through one function, and Part B's addresses use it too.
- **Calls stay home.** The runtime reaches the server through `BASE`, against the page's own address, so `rpc`, `live` and channels do not move. `src/client/runtime.js` has no worker and no `document.currentScript`, so nothing there depends on where the script came from. `import()` of a `client` module does need CORS, which the next point gives it.
- **CORS.** Door answers say `Access-Control-Allow-Origin: *`. They are public by definition, and fonts and modules need it. `Timing-Allow-Origin: *` too.
- **`integrity`.** A script or stylesheet from the CDN carries `integrity` and `crossorigin="anonymous"`, so a CDN that is broken into cannot run code on the page. The loader sets `t.integrity` and `t.crossOrigin`. `hint_links` (`src/client.rs:231`) must then say `crossorigin` on its preloads too: its comment says why a preload that differs is fetched twice. Images have no `integrity`, so a broken CDN can still change a picture.
- **`preconnect`.** One `<link rel="preconnect" href="..." crossorigin>`, as `hint_links` writes for `client` libraries.
- **The content policy.** When the program sends one, the host is added to `script-src`, `style-src`, `img-src`, `font-src` and `media-src`, or to `default-src` if that is all there is. The program's policy keeps everything else. This needs Q-904 first.
- **When the CDN fails.** Today the loader's `t.onerror=fail` gives up, so a CDN outage would leave components dead while the site is up. Instead the loader tries the same file at the page's own address, in order, and only then gives up. A stylesheet gets the same from the loader after load. A page with no loader has no fallback, so at start and every minute Quillo fetches one hashed file through the CDN, and while that fails it writes its own addresses. The check runs from the server, not from the visitor, so it is only half the answer, and the loader's retry is the other half.
- **`Host`.** A pull zone may send its own `Host`. The door does not read it, but `src/sites.rs` chooses a site by it, so a server with several sites needs each site's own setting and the CDN set to pass the site's host on. A driver does that. For a hand-made zone, the guide says so.

## Part D: drivers

This is the user's principle from 2026-11-05 again: per-provider differences are the driver's job, and programs never choose per provider. The same `.env` line with another scheme moves a site to another CDN.

| | Cloudflare | Bunny | CloudFront | Fastly |
|---|---|---|---|---|
| Keeps by default | by extension (`js`, `css`, `svg`, `woff2`, images, `pdf`...), no HTML or JSON | anything with cache headers, never `text/html`, `application/json`, `application/xml` | what its cache policy says | not checked |
| `Vary: Cookie` | ignored | ignored: its own key settings | its cache key decides | not checked |
| The trap | a Cache Everything rule, and `pdf` or `png` routes with no rule at all (Q-903) | an Override Cache Time rule | a minimum TTL above zero keeps `private` | not checked |
| Its own control header | `CDN-Cache-Control` (RFC 9213) | not checked | not checked | `Surrogate-Control` |
| Purge by tag | `Cache-Tag`, on all plans since 2025-04-01 | `CDN-Tag`, on all accounts | none found | `Surrogate-Key` |
| Purge limit | 5 a minute on Free, 5 a second on Pro, 100 tags a call | per account, not checked | not checked | not checked |
| Addresses | 22 networks | 585 addresses | 216 IPv4 networks, 49 origin-facing | 24 networks |
| What an asset host needs | the domain's nameservers on Cloudflare, unless on Business or Enterprise | nothing: a pull zone is `name.b-cdn.net` | not checked | not checked |

What a driver does:

1. **At start,** checks the zone exists and reaches this server, and creates it if the token may. A driver changes only a zone it created. For any other zone it says in the log what is missing.
2. **Rules:** keep `/_quillo/a/*` and the other hashed paths, whatever their extension (Cloudflare leaves out `mjs`, `wasm`, `json` and `map` by default); keep nothing else; pass the site's `Host`.
3. **Addresses:** fetches the provider's list, trusts it as `--trust-proxy` would, and refreshes it daily (Q-910).
4. **Purge,** for Part E only. Hashed addresses never need it.

Whether a driver is linked in or downloaded follows the answer to the same question for database drivers, which FEEDBACK43 left open. Two drivers are enough to prove the shape: Bunny first, as a pull zone is only an address, then Cloudflare.

## Part E: public pages at the edge (later)

This is the part no CDN can do alone. The hard problem with cached pages is knowing when they are wrong, and Quillo already knows: `live` records the tables a loader reads and announces what writes them, in any of the three databases.

```quillo
route edge GET /pricing with db = <html><body><Plans plans={db.query("select * from plans")}/></body></html>
```

- The run records what it read, as a loader's does. The answer says `Cache-Tag: q-plans`, `CDN-Cache-Control: max-age=300, stale-while-revalidate=60` and `Cache-Control: no-cache`, so the edge keeps it and the browser asks.
- A write to `plans` calls the driver's `purge`, gathered per tag. On Cloudflare's Free plan that means at most one call every 12 seconds, so a busy table gets a floor, and past it Quillo stops marking that table's pages as kept.
- **It is sound only if every read of the visitor is marked.** An answer whose run read nothing of the visitor cannot depend on the visitor. That is why it waits for Q-905, and why `edge` is a tag to start with and not the default. A tagged route that does read the visitor says `private`, logs it once, and `quillo check` warns.
- **Not seen:** what triggers, stored procedures and other programs write, the same gaps live data has. `max-age` is the limit on how wrong a page can be.
- **A page with live data repairs itself:** it arrives with an old `since`, and its first poll brings it up to date. I read this in the code and did not run it through a cache.
- It needs the whole-site shape, so it does not work with Part C's asset host alone.

## Part F: what not to build

- **Push at deploy.** There is no deploy step to hang it on, and pull with hashed names has nothing left to solve. Sites with no server at all are a different need: the guide's "fetch every page and upload them".
- **`quillo edge`, our own edge nodes.** Keeping files is the easy part. Sending each visitor to the nearest node takes anycast or geographic DNS, which a binary cannot give. Limer already has `cache`, `encode gzip zstd br` and automatic HTTPS, so anyone who wants their own edge can run Limer on a few servers and name it in `QUILLO_CDN`, once problems 1 to 3 of its `FEEDBACK1.md` are fixed. Until then its `cache` is not safe in front of Quillo.
- **A shared runtime at `cdn.quillolang.org`.** Each program's runtime is shaken down to what it uses (the two programs here got `6c265dc8` and `48c876c3`), and browsers keep their cache per site, so one site's copy never serves another.

## Seen again

- FEEDBACK35's design says a page with `script-src 'self'` "keeps working". The bundle does, but the inline loader that fetches it does not (Q-904).
- Q-391 and Q-669 made personal pages `private`, but only when nothing else had set `Cache-Control` (Q-903), and only for sessions, cookies and credentials (Q-905).
- The guide says of `cache`: "Use it for files whose name changes with their content" (`USERGUIDE.md:4494`). That is good advice, and Quillo gives no way to follow it (Q-906).

## New issues

### Q-903. A signed-in file reaches a visitor with no session: `cache` or a route's own `public` beats the session read

| Page | Who asked first | Who asked next | Standard cache | `Vary` ignored |
|---|---|---|---|---|
| `/files/kept.pdf`, `cache: "1y"` after a session check | Ana, signed in | no cookie | 401 | **200, Ana's report** |
| `/kept`, `respond(...)` with `public, max-age=60` | Ana | Bob | "Hello bob" | **"Hello ana"** |
| `/files/plain.pdf`, session check, no `cache` | Ana | no cookie | 401 | 401 |
| `/me` | Ana | Bob | "Hello bob" | "Hello bob" |

```quillo
route GET /files/kept.pdf {
    let u = session().user ?? reply 401 "Log in first"
    file("public/report.pdf", { cache: "1y" })
}
```

```sh
curl -s -D - -o /dev/null -H "Cookie: $C" -H 'X-Forwarded-Proto: https' http://127.0.0.1:4831/files/kept.pdf
# cache-control: public, max-age=31536000, immutable
# vary: Cookie
```

**This case needs no Cache Everything rule.** `pdf` and `png` are extensions Cloudflare keeps by default, it honours `public`, and it ignores `Vary`. So by the documents, a plain proxied site is exposed by this route. The model shows the result, and a real Cloudflare zone has not been tried.

`site_headers_into` adds `private` only when no rule set a `cache-control` (`src/server.rs:1064`), and a route's or `file()`'s own header is left as it is. There is a precedent for overriding: a page with a form key has its `public` replaced by `no-store` (`src/server.rs:473`).

**Fix:** the same treatment. When `visitor_read` or a cookie was set, and the answer says `public`, `s-maxage` or `max-age` without `private`, it becomes `private`, with the time kept for the browser. The log says so once, and `quillo check` warns where it can see both in one route. Applies to `respond`, `file`, `files` and `site_headers`.

### Q-904. The guide's own content policy stops every component, with no word

```quillo
site_headers("/csp", { "content-security-policy": "default-src 'self'" })
component Counter() {
    state n = 0
    <button id="c" @click={() => n = n + 1}>clicked {n}</button>
}
route GET /csp/page = <html><head><title>csp</title></head><body><Counter/></body></html>
```

In Chrome, after a click:

| Policy | The button says | Scripts on the page |
|---|---|---|
| none | `other 2` (the same component shape on `/other`) | `t/`, `r/`, `b/` files and two inline |
| `default-src 'self'` | `clicked 0` | two inline, both refused: no bundle is ever fetched |

`Executing inline script violates ... default-src 'self'`, twice, and `Applying inline style` for `<quillo-island style="display:contents">`. The page looks right, and nothing works. The server logs nothing and `quillo check` says nothing. `default-src 'self'` is the guide's own example, at `USERGUIDE.md:3744`.

**Fix:** when an answer carries a policy that would refuse them, Quillo adds a `'sha256-...'` for each inline script it wrote, to `script-src` (made from `default-src` when there is none). A hash and not a nonce: the page's text decides it, so a kept page stays valid, which a nonce would not. `display:contents` moves from a `style=` attribute to a rule that needs no permission. A test runs a component under `default-src 'self'` in Chrome.

### Q-905. A page made from `locale()`, `request.ip` or another header is not marked: a French page goes to an English visitor

| Page | Says | First visitor | Second visitor, Limer `cache { ttl 5m }` | Second visitor, Cache Everything model |
|---|---|---|---|---|
| `/lang`, `t("hello")` | nothing | `Bonjour` (`Accept-Language: fr`) | **`Bonjour`** (`en`) | **`Bonjour`** |
| `/agent`, `request.headers["user-agent"]` | nothing | `agent-A` | **`agent-A`** | **`agent-A`** |
| `/ip`, `request.ip` | nothing | `203.0.113.5` | kept, the address not told apart here | **`203.0.113.5`** |

The standard cache keeps none of them, as they say nothing and have no validator. But Limer's `ttl` and Cloudflare's rule both fill that silence, and Limer needs nothing beyond its guide's example.

`locale()` reads the `locale` cookie and `Accept-Language` itself (`src/i18n.rs:337-340`), past `cookie()`, so Q-669's mark never fires. `credential` (`src/interp.rs:731`) names only `cookies`, `cookie` and `authorization`.

**Fix:** live data solved this already. `read_request` (`src/interp.rs:219`, set at `1952`) is what `server.rs:1484` uses to decide a run is the viewer's own, and the guide lists it: `session()`, cookies, headers, the locale. The page side should use the same knowledge, with one refinement: the path, the query and `Host` are in every cache's key, so they do not count. `locale()` also says `Vary: Accept-Language` for caches that read it. And with Part A, a page that read nothing says `no-cache` instead of nothing.

### Q-906. A public file cannot be both kept and current

100 requests each through the standard cache and Limer:

| Address | Says | Reached the origin, nginx | Reached the origin, Limer |
|---|---|---|---|
| `/static/css/style.css` | `no-cache` | **100 of 100, whole files** | 2 (but see Limer, below) |
| `/static/img/logo.svg` | `no-cache` | **100 of 100, whole files** | 2 |
| `/forever/css/style.css`, `cache: "1y"` | `immutable` | 0, warm from an earlier run | 1 |
| `/_quillo/b/<hash>.js` | `immutable` | 0, warm from an earlier run | 1 |
| `/_quillo/r/<hash>.js` | `immutable` | 0, warm from an earlier run | 1 |

**Limer's `/_quillo/r/` row counts hits, not good answers.** That is the one answer here over 1 KB, so Quillo gzipped it, and Limer serves a kept gzip answer with no `Content-Encoding` (its `FEEDBACK1.md`, problem 1): those bodies were unreadable. Its `2` for `no-cache` is its problem 5.

nginx does not store `no-cache`. Cloudflare stores it and asks every time, so that would be 100 `304`s rather than 100 files: less traffic, the same trip to the origin, and a far visitor waits for it.

The other way is worse. With `cache: "1y"`, after `style.css` changed, the cache still had version one and the origin had version two, and every browser would keep the old one for a year, with no purge that reaches a browser. So a site's own CSS, scripts, images and fonts get nothing from a CDN today, while Quillo's own bundles get everything.

**Fix:** Part B.

### Q-907. Nothing can put a file on another host

`quillo run main.qwe --base https://cdn.example.com` answers `quillo: --base is a path such as /shop`, which is right for `--base`. There is no other way: bundles, sheets and translations are written from `self.base`, and a developer cannot reach those addresses at all, unlike their own `<link>`.

**Fix:** Part C.

### Q-908. Bundles, the runtime, sheets and translations are gzipped again for every request

The sample app from `quillo new`, 300 requests on one connection, median:

| Answer | Sent with gzip | With gzip | Without |
|---|---|---|---|
| `/_quillo/r/<hash>.js`, 137 KB | 38,508 bytes | **1.97 ms** | 0.10 ms |
| `/_quillo/b/<hash>.js`, 4.5 KB | 1,478 bytes | 0.08 ms | 0.06 ms |

`compress` keeps a gzipped copy only for an answer with an `ETag` (`src/server.rs:2961`), and these have none, though `/_quillo/quillo.js` has one. They are `immutable`, so a returning visitor costs nothing, but every first visit pays, on the request's thread. A CDN hides this. A site with none does not. `s/` and `t/` take the same path, and were not measured.

**Fix:** answer the four with `ETag: "<hash>"`. Their gzip is then kept by what is there, and a revalidation becomes a `304` for free. A test: a second request costs about what a plain one does.

### Q-909. A bundle is known only to the run that rendered its page

A bundle is made when a page first needs it (`bundle_for_page`), and kept in that process. Asked for before then, the answer is the whole program's bundle:

| After a restart, before the page is rendered | Bytes | Says |
|---|---|---|
| `/_quillo/b/58dc710968437ede.js` | **111,756** | `no-cache` |
| the same, after the page is rendered once | 678 | `immutable` |

The name came out the same after the restart, so the right file could have been made. `--instances` has the same gap, as each instance keeps its own: on a fresh `--instances 4`, 5 of 40 visits got the whole bundle (visits 1, 2, 4, 6 and 7), then none. Nothing is wrong on the page, and the `no-cache` keeps a CDN from storing the wrong thing, so it is waste rather than a fault: 165 times the bytes here. But it hits a CDN's first fetch after every deploy, and a kept page (Part E) would meet it at any time.

**Fix:** the address names its roots, `/_quillo/b/<hash>/Counter,Other.js`, so any instance can make the bundle, check that its hash matches, and answer it. In the path, as CloudFront may drop a query. Unknown names are refused before any work, one build runs at a time, and a set too long for an address falls back to `quillo.js`.

### Q-910. `--trust-proxy` wants the CDN's addresses by hand

Today's counts: Cloudflare 15 IPv4 and 7 IPv6 networks, CloudFront 216 IPv4 networks and 49 origin-facing, Fastly 21 and 3, Bunny 585 addresses. All are published, and all change. `--trust-proxy` takes a list on the command line, and a list that goes stale fails quietly:

- an address missing from it makes those visitors one client, sharing 8 large-file places (the guide's Static files) and 512 connections (`src/httpd.rs:1753`), and `request.ip` is the CDN's;
- an address left in after the provider gives it up lets someone else forge `X-Forwarded-For`.

**Fix:** `--trust-proxy cloudflare` (and `bunny`, `cloudfront`, `fastly`): a list built into the binary, refreshed from the provider each day, with the last good list kept if that fails. Part D's drivers use the same thing. Detecting a CDN from headers such as `CF-Ray` is not safe: anyone can send them.

### Q-911. A page behind a CDN waits for WebTransport

*Read from the code, not measured: a local rig cannot drop UDP as a CDN does.*

`chanConnect` tries WebTransport first when `/_quillo/wt-ticket` gives a port (`src/wt.rs:1156`), at `location.hostname` (`src/client/runtime.js:5027`). When the whole site is behind a CDN, that host is the CDN, which has nothing on that UDP port, so the attempt runs out its 1,500 ms before falling back to a WebSocket. `Chan.wt` is a page's own, so each full page load with a channel pays it again. `--no-wt` avoids it, if the developer knows.

**Fix:** the ticket request came through the proxy, so the server knows. An answer to a request from a trusted proxy leaves out `port`. With an asset host nothing changes, as the page's host is still the origin.

## Proposed order of work

Nothing starts until main is in sync, every test (CI too) is green and the docs are aligned. Each slice has tests and edge tests, and the docs follow it: the guide, the book, the website and `--help`.

| Slice | What | Issues | Note |
|---|---|---|---|
| 0 | Personal answers, in full | Q-903, Q-905 | sites behind a CDN need this now, so it goes first |
| 1 | The content policy | Q-904 | a Chrome test; needed by Part C, and a bug without it |
| 2 | Keep the gzip | Q-908 | small |
| 3 | Bundles any instance can answer | Q-909 | |
| 4 | Public routes, hashes, the door | Q-906 | the `files()` security tests run again through the door |
| 5 | Addresses in pages, components and stylesheets | Q-906 | `bench/vs-python` before and after; `runtime.js` stays at 99.50% |
| 6 | `QUILLO_CDN`: host, CORS, `integrity`, policy, fallback | Q-907 | test the fallback with the CDN stopped, in Chrome |
| 7 | `--trust-proxy` by name, WebTransport | Q-910, Q-911 | |
| 8 | quillolang.org on a real CDN | | the first real proof: this report's model is replaced by the real thing |
| 9 | Drivers: Bunny, then Cloudflare | | after FEEDBACK43's linked-in or downloaded answer |
| 10 | Pages at the edge | | Part E; only after a review of what Q-905 marks |

## Open questions for the user

None of these holds anything up today: nothing is built until the sync rule allows it. Each says what was measured and what comes from documents or reading the code.

### 1. What does "auto" stop at?

**A person should make the pull zone once for now, and Quillo should take that step over only after a real site has run this way.** It happens once per site, at setup. With `QUILLO_CDN=https://...` someone makes a zone that points at the server. With a driver, Quillo makes it from a token. Nothing here ran against a provider: this comes from their documents.

| | What changes with a driver |
|---|---|
| **Functionality** | Quillo makes and keeps the zone, its rules and the address list. Parts A to C work on any CDN without one |
| **User** | No change for visitors |
| **Requests** | No change per request; a few provider calls at start, and one address refresh a day |
| **Security** | A driver holds a token that can change the zone, so a leaked `.env` could point the site's files elsewhere. Without a driver no such token exists |

What today's way has that a driver would lose: no CDN credential exists anywhere in a Quillo site.

**My recommendation:** build through Part C, then decide on Part D with what quillolang.org teaches. The trade-off is that "auto" has one manual step at first.

**Question:** is one manual setup step acceptable for the first version? If no, the Bunny driver moves ahead of slice 8.

### 2. Is the asset host what `QUILLO_CDN` does?

**`QUILLO_CDN` should move only hashed files to the CDN, and leave pages on the origin.** It happens on every page view: the HTML comes from the origin and its files from the CDN. Measured: Q-903 and Q-905 both need the answer to pass through the cache. Not measured: distance, so the speed rows below are reasoning.

| | Asset host, compared with the whole site behind the CDN |
|---|---|
| **Functionality** | Needs Parts B and C; Part E is not possible. The whole site needs no page change, and Part E is possible |
| **User** | Files come from nearby, and the HTML still travels the whole way. With the whole site, encryption starts nearer, and kept pages would too |
| **Requests** | One more host to connect to, softened by `preconnect`; calls and live data do not change. With the whole site every request takes one more hop, and WebTransport is gone (Q-911) |
| **Security** | A page cannot leak, as it never passes through. With the whole site, a wrong header or rule can show one visitor's page to another |

What the whole-site way has that this loses: one host, with no CORS, `integrity` or content policy work.

**My recommendation:** the asset host, because a mistake cannot show one visitor's page to another. The trade-off is that the page's first byte is no faster.

**Question:** should `QUILLO_CDN` mean the asset host? If no, Part C shrinks to almost nothing, and Q-905 and Q-911 become the gate for everything.

### 3. Q-903: may Quillo overrule a route that says `public` after reading the session?

**Yes: an answer that read the visitor should be `private`, whatever the route said.** It happens when a route reads the session and then says `public`, by its own header or with `cache: "1y"` on a file. Measured on the Cache Everything model: a visitor with no cookie got Ana's PDF, and Bob got "Hello ana". Cloudflare itself was not tried.

| | What changes |
|---|---|
| **Functionality** | `public` is not obeyed after a session read. The time stays, for the browser alone. Logged once, and `quillo check` warns |
| **User** | No visible change: a signed-in visitor's own browser keeps the file as long as before |
| **Requests** | Such answers reach the origin once per visitor instead of once per edge |
| **Security** | The leak closes, and `Vary: Cookie` stops being the only thing in the way |

What today's way has that this would lose: on a cache that honours `Vary: Cookie` (the standard one here, where Bob got his own page), a copy per cookie saved origin requests.

**My recommendation:** overrule it, as Quillo already does for a form key. The trade-off is that a program counting on a copy per cookie loses it.

**Question:** may Quillo overrule the route? If no, it stays a `quillo check` warning, and the leak stays possible while the site runs.

### 4. Is Part E wanted, and as a tag?

**Yes, but last, and as `route edge` rather than by default.** It would apply to a public page that many people read and few things change: pricing, docs, a blog. This is design only and nothing was run; `live`'s tracking of reads and writes comes from the guide and the code.

| | What changes |
|---|---|
| **Functionality** | A new tag. A tagged page is kept at the edge, and purged when a table it read is written |
| **User** | Far visitors get the page quickly. It can be wrong for up to `max-age` if something Quillo cannot see wrote the table |
| **Requests** | Far fewer renders at the origin. A write can cost a purge call, gathered per tag: at most one every 12 seconds on Cloudflare's Free plan |
| **Security** | Safe only if every read of the visitor is marked. One missed read, of Q-905's kind, shows a visitor's page to others |

What today's way has that this would lose: every page is current when it is made.

**My recommendation:** build it after everything else, as a tag. The trade-off is one word on each route, so this part is not fully automatic.

**Question:** do you want Part E? If no, it comes out of the report, and the work ends at the drivers.

### 5. Is a Quillo-run CDN a goal?

**Not for now: this report assumes the CDNs are other companies'.** It matters if the real aim was a hosted service or edge nodes of our own. This is reasoning, not measurement: keeping files is the easy part, and sending each visitor to the nearest node needs anycast or geographic DNS, which a binary cannot give.

| | What a Quillo-run CDN would change |
|---|---|
| **Functionality** | Parts A to C stay the same; the driver design changes |
| **User** | Fewer locations than an established CDN, at least at first, so far visitors gain less |
| **Requests** | No change |
| **Security** | Whoever runs it carries customers' traffic and certificates |

What it would have that this report's way lacks: a CDN with no account at another company.

**My recommendation:** not a goal now. Limer can cover anyone who wants edges of their own, once its cache is fixed. The trade-off is that every site needs an account with a provider.

**Question:** is a hosted Quillo CDN a goal? If yes, Parts A to C still come first, and I write up the driver design for it.

### 6. Which CDN for quillolang.org?

**Bunny, for the first real test.** It happens once, at slice 8, when the site goes on a real CDN. The site runs on one VPS behind Limer. From the documents: a Bunny pull zone is an address, `name.b-cdn.net`, with no DNS change. Cloudflare's CNAME setup is Business and Enterprise only, so on Free or Pro the whole domain's nameservers move to Cloudflare. Prices were not compared.

| | What changes with Bunny |
|---|---|
| **Functionality** | The site's files move to the asset host; pages do not change. Undone by removing one line from `site.env` |
| **User** | Files come from nearby; nothing else changes |
| **Requests** | The VPS serves each file about once per edge instead of once per visitor |
| **Security** | Another company serves the site's scripts, held to `integrity`. Images are not covered |

What today's way has that this would lose: one provider to depend on instead of two. The fallback covers scripts and stylesheets at once, and images only after the server's next check.

**My recommendation:** Bunny for the proof, as it needs no DNS change and undoes in one line. The trade-off is that it does not test Cloudflare, so Q-903's real behaviour there needs a separate small test on a spare domain.

**Question:** is Bunny fine for quillolang.org's first run? If no, the site's nameservers move to Cloudflare first, and the report's slice 8 waits for that.

## Smaller notes

- **Limer** has its own report now, `FEEDBACK1.md` in `~/dev/limer`, with nine problems in its cache. The one that matters here: it keeps a gzipped answer and serves it again with no `Content-Encoding`, so a second visitor gets garbage. `ttl` keeping pages is its problem 4, and whether quillolang.org's Limerfile uses `cache` is still worth checking.
- **Brotli.** The sample's runtime: gzip 38,798 bytes, brotli 5 37,050, brotli 11 33,455 (181 ms, once), zstd 19 35,432. That saves about 5 KB on a first visit. Only `brotli-decompressor` is in the tree, so it would be a new dependency, and a CDN does its own compression on what it receives plain. Not now.
- **A cookie set on every request** by a middleware around `/static` (`/m/css/style.css` here) stops Cloudflare keeping those files, and Part B correctly leaves such routes out. The guide should say to put such middleware on pages only.
- **Early Hints.** Quillo sends no `Link` header. With Early Hints on, Cloudflare turns those into a `103` by itself. Worth trying once addresses are hashed. Its documents warn a `103` can reach signed-out visitors, which is harmless for public addresses only.
- **Compression dictionaries** (RFC 9842) would make a new runtime cost only what changed, but browser support is Chromium only so far. Later, if ever.
- **`--split` and `--as-service`** were not looked at. Services answer calls, not pages, so nothing here should reach them.

## Where the AI struggled

- My first leak test printed the last 40 characters of each page, which on a page with a component is script, so it proved nothing. The table above comes from the rerun that reads the page's text.
- Python's header map keeps one `Vary`, so `/me` looked as if it had lost `Vary: Cookie`. It sends two lines, and `curl -D -` showed both. It would have been a false issue.
- `--instances 2` first showed the whole bundle 30 times of 30, which looked like per-page bundles never working there. It was my request order: connections alternate between instances, so the page always went to one and the bundle to the other. A random order gave Q-909's real number.
- I remembered Cloudflare's read timeout as 100 seconds. Its page now says 125.
- I counted Limer's hits in Q-906 without reading the bodies, and called them kept. Looking closer for Limer's own report showed the runtime row's hits were unreadable gzip.
- `HEAD` moved to `a743bdc` during this study, from another session. That commit adds FEEDBACK53.md only, and the binary tested reports `e6f198f`.

## Edge cases checked that behave well

- `/me`, made from the session, was never given to another visitor by any of the three caches, `Vary` ignored or not.
- A session-checked file with no `cache` says `no-cache`, so a visitor with no cookie got 401 from both nginx caches.
- A wrong-hash bundle says `no-cache`, so a page from before a restart does not leave the wrong file at the edge for a year.
- A bundle's name is the same after a restart.
- `/_quillo/*` and `/static/` carry no `Set-Cookie` when the program sets none, so nothing stops them being kept.
- `--base https://...` is refused with a clear line.
- A page's WebTransport host is the page's own, so a CDN does not expose the origin's address that way.
- A long poll is held 25 seconds, against Cloudflare's 125-second read timeout.

## Scripts

The program (`main.qwe`), with `public/css/style.css`, `public/img/logo.svg`, `public/js/app.js`, `public/report.pdf`, and `locales/en.json` and `fr.json` holding `hello`:

```quillo
translations("locales", { default: "en" })
site_headers("/csp", { "content-security-policy": "default-src 'self'" })

route GET /static/*path = files("public", path)
route GET /forever/*path = files("public", path, { cache: "1y" })

component Counter() {
    state n = 0
    <button id="c" @click={() => n = n + 1}>clicked {n}</button>
}

component Other() {
    state n = 0
    <button id="o" @click={() => n = n + 2}>other {n}</button>
}

fn visit(r) {
    set_cookie("seen", "1")
    r
}

routes /m | visit {
    route GET /*path = files("public", path)
}

route GET / =
    <html><head><link rel="stylesheet" href="/static/css/style.css"/></head>
    <body><h1>Home</h1><img src="/static/img/logo.svg"/><Counter/></body></html>
route GET /other = <html><body><Other/></body></html>
route GET /csp/page = <html><head><title>csp</title></head><body><Counter/></body></html>
route GET /signin/:name {
    set_session({ user: name })
    redirect("/me")
}
route GET /me = <html><body>Hello {session().user ?? "nobody"}</body></html>
route GET /ip = <html><body>Your address is {request.ip}</body></html>
route GET /lang = <html lang={locale()}><body>{t("hello")}</body></html>
route GET /agent = <html><body>{request.headers["user-agent"]}</body></html>
route GET /kept {
    respond(200, `Hello ${session().user ?? "nobody"}`, { "cache-control": "public, max-age=60" })
}
route GET /files/kept.pdf {
    let u = session().user ?? reply 401 "Log in first"
    file("public/report.pdf", { cache: "1y" })
}
route GET /files/plain.pdf {
    let u = session().user ?? reply 401 "Log in first"
    file("public/report.pdf")
}
```

Run with `quillo run main.qwe --port 4831 --trust-proxy 127.0.0.1`. The caches (`nginx -p DIR -c nginx.conf -g 'daemon off;'`), inside `http { }`, where `X-Test-IP` lets one machine be two visitors:

```nginx
proxy_cache_path c1 keys_zone=rfc:1m;
proxy_cache_path c2 keys_zone=everything:1m;
upstream q { server 127.0.0.1:4831; keepalive 8; }
server {                                  # the standard rules
  listen 127.0.0.1:4832;
  location / {
    proxy_pass http://q; proxy_http_version 1.1; proxy_set_header Connection "";
    proxy_set_header Host $host; proxy_set_header X-Forwarded-Proto https;
    proxy_set_header X-Forwarded-For $http_x_test_ip;
    proxy_cache rfc; proxy_cache_revalidate on;
    add_header X-Cache $upstream_cache_status always;
  }
}
server {                                  # the model of Cache Everything
  listen 127.0.0.1:4833;
  location / {
    # the same five lines as above, then:
    proxy_cache everything; proxy_cache_revalidate on; proxy_cache_valid 200 1h;
    proxy_ignore_headers Vary;
    add_header X-Cache $upstream_cache_status always;
  }
}
```

Limer (`limer run -c limer.conf -w 1`):

```
{
    cache_capacity 16m
}
http://127.0.0.1:4834 {
    cache {
        ttl 5m
    }
    reverse_proxy 127.0.0.1:4831
}
```

Two visitors, here on `/lang` (`/ip` and `/agent` the same way, `/kept` with `Cookie` in place of the language):

```sh
curl -s -H 'X-Test-IP: 203.0.113.5'  -H 'Accept-Language: fr' http://127.0.0.1:4833/lang | grep -o 'Bonjour\|Hello'
curl -s -H 'X-Test-IP: 198.51.100.9' -H 'Accept-Language: en' http://127.0.0.1:4833/lang | grep -o 'Bonjour\|Hello'   # Bonjour
```

A session cookie for the signed-in rows, then the file with and without it:

```sh
C=$(curl -s -D - -o /dev/null -H 'X-Forwarded-Proto: https' http://127.0.0.1:4831/signin/ana | grep -i '^set-cookie' | sed 's/^[^:]*: //; s/;.*//' | tr -d '\r')
curl -s -o /dev/null -H "Cookie: $C" http://127.0.0.1:4833/files/kept.pdf
curl -s http://127.0.0.1:4833/files/kept.pdf        # %PDF-1.4 secret report of ana
curl -s http://127.0.0.1:4832/files/kept.pdf        # Log in first
```

Counting what reached the origin: its request log, `grep -c '^GET /static/css/style.css -> ' origin.log`, before and after 100 requests through the cache. Q-904 in Chrome, with `node feedback/tools/drive.mjs csp.mjs`:

```js
export default async ({ page, sleep }) => {
  await page.go("http://127.0.0.1:4851/csp/page");
  await sleep(900);
  const c = await page.center("#c");
  await page.click(c.x, c.y);
  await sleep(900);
  return { text: await page.ev(`return document.querySelector("#c").textContent`), problems: page.problems };
};
```

Sources: Cloudflare's [default cache behavior](https://developers.cloudflare.com/cache/concepts/default-cache-behavior/), [origin cache control](https://developers.cloudflare.com/cache/concepts/cache-control/), [uncached responses](https://developers.cloudflare.com/cache/troubleshooting/investigating-uncached-responses/), [CDN-Cache-Control](https://developers.cloudflare.com/cache/concepts/cdn-cache-control/), [purge by tag](https://developers.cloudflare.com/cache/how-to/purge-cache/purge-by-tags/), [purge limits](https://developers.cloudflare.com/cache/how-to/purge-cache/), [all purge methods on all plans](https://developers.cloudflare.com/changelog/post/2025-04-01-purge-for-all/), [connection limits](https://developers.cloudflare.com/fundamentals/reference/connection-limits/), [CNAME setup](https://developers.cloudflare.com/dns/zone-setups/partial-setup/) and [Early Hints](https://developers.cloudflare.com/cache/advanced-configuration/early-hints/); Bunny's [quickstart](https://bunny.net/docs/cdn/quickstart), [Smart Cache](https://bunny.net/docs/cdn/smart-cache), [Vary Cache](https://bunny.net/docs/cdn/vary-cache) and [purge](https://bunny.net/docs/cdn/purge-cache); AWS's [managed cache policies](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/using-managed-cache-policies.html) and [CachePolicyConfig](https://docs.aws.amazon.com/cloudfront/latest/APIReference/API_CachePolicyConfig.html); Fastly's [cache control headers](https://www.fastly.com/documentation/guides/full-site-delivery/caching/about-cache-control-headers); RFC 9213 (Targeted HTTP Cache Control) and [RFC 9842](https://www.rfc-editor.org/info/rfc9842). Address lists, 2026-11-07: <https://www.cloudflare.com/ips-v4>, <https://www.cloudflare.com/ips-v6>, <https://ip-ranges.amazonaws.com/ip-ranges.json>, <https://api.fastly.com/public-ip-list>, <https://bunnycdn.com/api/system/edgeserverlist>.
