# Feedback from checking service discovery and the OpenAPI document against the docs

| | |
|---|---|
| App | None. The question was whether service discovery and the OpenAPI document, both added in 0.6.0 (FEEDBACK10.md, Q-074 and Q-077), are in the docs. Each claim the docs make about them was checked against the code and run, and each place a reader could look was searched |
| Quillo | 0.6.0 (378e64b, 2026-11-01), branch `feedback29-fixes`, `./target/release/quillo` built from it; website 9406d96; the live quillolang.org, read on 2026-11-01 |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo docs` and `quillo run --describe` on one app (a route group, a file taken with `import`, a prefixed service, a service that is only called, a mount), in one process, under `--split` and under `--base`; a copy of a service joined with `--join`, 10 calls counted; curl; reading [src/describe.rs](../../src/describe.rs), [src/service.rs](../../src/service.rs), [src/cluster.rs](../../src/cluster.rs), [src/server.rs](../../src/server.rs) and [tests/describe_api.rs](../../tests/describe_api.rs); grep over USERGUIDE.md, README.md, CHANGELOG.md, SECURITY.md, book/chapters, the website's guides and the live pages |

What worked well: both features exist and mostly do what the guide says. `quillo docs` reads a program without running it. It lists a route group's routes by their whole path, follows a file taken with `import`, and puts mounts and prefixed services under their prefixes. It names a record type (`x-quillo-type`) and gives `dec` as a decimal string, as the microservices guide shows. A mount's described call answers 200. A copy started with `--join` showed up in the app's log at once and took 5 of the next 10 calls. Every number the guide gives for joining and for limer's pool matches the code.

So the answer to "not in the documents" is that both are in, but hard to find. In places the docs also say the opposite of what the code does:

| Document (site page, book chapter) | OpenAPI: `quillo docs`, `--describe` | Discovery: `--join`, `--register` |
|---|---|---|
| `quillo --help` | both listed | both listed, with `--service-url` and `--scale` |
| USERGUIDE.md, option and environment tables (/docs/, ch. 61) | a row and a line | rows; `--register`'s link points to the wrong section (Q-401) |
| USERGUIDE.md › Routes › Services (/docs/routes/#services, ch. 31) | one bullet, "**Described.**" | one bullet, "**Copies that join.**"; "Services are not found through limer's pool" |
| USERGUIDE.md › Components › Live data › Several servers (/docs/components/, ch. 21) | | `--register` and limer's `discover` pool |
| USERGUIDE.md › Possible or not (/docs/possible-or-not/, ch. 13) | no row | no row |
| USERGUIDE.md › Not built yet (/docs/not-built-yet/, ch. 72) | | lists "discovering services through limer's pool" |
| quillo-website guides/microservices.md (/docs/microservices/, ch. 52) | a section, "Querying the API" | "**No discovery.**" |
| quillo-website guides/hosting.md (/docs/hosting/, ch. 54) | nothing; only metrics are kept private (Q-403) | `--register`, not `--join` |
| README.md (GitHub, ch. 11) | nothing | nothing |
| CHANGELOG.md | nothing, though 0.6.0 shipped it | `--join` in 0.6.0's Tools |
| SECURITY.md | nothing | nothing |

The book's text is the guide's word for word (the Services bullets were compared), so every docs issue below is in the book too. On a local machine the site's `content/USERGUIDE.md` links to `~/dev/wisp` at 1cf047c, ten hours before `describe.rs` was written. That is only link.sh's default: each deploy copies quillo-app1's guide into the release, and the live pages have the sections.

Fix Q-395 first. Two documents that should be the same differ, and the one served live advertises a call that can only fail. Q-396 and Q-397 are next: each loses routes without a word.

The repros are inline, each run on 378e64b. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched. The index has no rows yet for FEEDBACK30 and FEEDBACK31 (Q-361 to Q-394), so these rows follow theirs when all three are added. Numbers start at Q-395.

## Seen again

None. Two fixed issues are where these features came from:

| ID | What it was | Here |
|---|---|---|
| Q-074 | a new copy of a service could not join the app | fixed by `--join`; Q-399 and Q-404 are about how the docs describe it |
| Q-077 | nothing described a program's API | fixed by `quillo docs` and `--describe`; Q-395 to Q-398 are about what the document gets wrong |

## New issues

### Q-395. `--describe` lists a service's server fns with a call address that answers 404, so it differs from `quillo docs`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | comparing `quillo docs` with `/_quillo/describe` on one app | 1 |

The guide says `--describe` "serves the same" as `quillo docs`. For a service served under a prefix, it does not:

- `quillo docs` leaves the service's server fns out, on purpose: [tests/describe_api.rs:165](../../tests/describe_api.rs) says "A service's functions stay its own: only a mount's are served here".
- `--describe` puts them in, keyed `"/billing invoice"`, with `"call": "POST /billing/_quillo/rpc/invoice"`.

That address refuses, as the guide says it should: a service does not answer `/_quillo/rpc` under its prefix. A gateway or client generator that imports the document gets an operation that can never work. The microservices guide documents the `--describe` behaviour: "their `server fn`s keyed by prefix and name (`"/billing invoice"`)". `/billing/_quillo/describe` gives the service's own document, and its `invoice` says `POST /_quillo/rpc/invoice`, which is also a 404 at the app's root.

```quillo
// main.qwe
service /billing = "billing.qwe"
route GET / = "shop"
```

```quillo
// billing.qwe
route POST /buy = "bought"
server fn invoice(name: str, cents: int) -> map = { name, cents }
```

```sh
quillo docs main.qwe              # paths /billing/buy and /; x-quillo-server-fns empty
quillo run main.qwe --describe &
curl -s 127.0.0.1:8080/_quillo/describe
# "/billing invoice": { "call": "POST /billing/_quillo/rpc/invoice", ... }
curl -s -X POST -H 'content-type: application/json' -d '["ann",100]' 127.0.0.1:8080/billing/_quillo/rpc/invoice
# 404 {"error":"no server fn `invoice` (a service's server fns are called by other services, not by pages)"}
```

**Suggestion:** in `collect_interp`, take server fns only from mounts, as `collect_file` does. The interpreter's `mounts` holds prefixed services too, so tell them apart by `declared_services`. Then fix the microservices guide's sentence. If a service's functions are worth describing, put them under a key of their own with the call that works: `POST /_quillo/service/<fn>`, signed, at the service's own address.

### Q-396. A path with several routes is described as its last route: the other bodies and queries are dropped, and two path parameters of one shape make the document invalid

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo docs` on payload and query routing | 1 |

Payload routing is a Quillo feature with an example of its own on the site: examples/orders, "One path, several handlers, chosen by what is inside the JSON body". `Parts::document` keeps one operation per method and path (`m.insert(method, op)`), so each route overwrites the one before it.

Both `/orders` routes below answer (`refund`, `new`), but the document has only the second: its body is `{ kind: "new", item }` and its `x-quillo-at` is `main.qwe:2`. `/search` has `tag` and not `q`. `/users/:id(int)` and `/users/:name` become `/users/{id}` and `/users/{name}`. OpenAPI 3 forbids that pair ("Templated paths with the same hierarchy but different templated names MUST NOT exist as they are identical"), so a validator refuses the whole document.

```quillo
route POST /orders { json: { kind: "refund", id: int } } = "refund"
route POST /orders { json: { kind: "new", item: str } } = "new"
route GET /search ? { q: str } = "by q"
route GET /search ? { tag: str } = "by tag"
route GET /users/:id(int) = id
route GET /users/:name = name
```

```sh
quillo docs main.qwe
# /orders post: kind { "type": "string", "enum": ["new"] }, x-quillo-at main.qwe:2
# /search get: parameters [tag]
# paths: /orders, /search, /users/{id}, /users/{name}
```

**Suggestion:** describe several bodies as `oneOf`, each with its `x-quillo-at`. The literal fields are already `enum`s, so they can serve as the discriminator. Merge several query patterns, a parameter being required only where every variant requires it. Use one template per path shape, named as the first route names it, with its schema a `oneOf` of the types. Add a test beside `the_root_is_a_slash_and_two_methods_share_a_path`: two routes, one method.

### Q-397. Under `--split` the app's document loses the routes it serves for its services, and `/billing/_quillo/describe` is a 404

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `--describe` with `--split` | 1 |

src/describe.rs says "the front door describes what is behind it". The guide says the document holds "the routes of the services it serves under their prefixes". In one process it does. Under `--split` the same app still serves `POST /billing/buy` through itself, but its document has no `/billing/buy` and no `/billing invoice`: the services are processes of their own and not in the interpreter's mounts.

Asking the service through the app, at `/billing/_quillo/describe`, gets a 404, because `--describe` is not passed to the services. The guide's list of options passed down, and of those not passed, leaves it out. `quillo docs main.qwe` reads the source, so it still has `/billing/buy`. The document now depends on how the app was started.

```sh
quillo run main.qwe --describe --split &                         # Q-395's main.qwe and billing.qwe
curl -s -X POST 127.0.0.1:8080/billing/buy                         # bought
curl -s 127.0.0.1:8080/_quillo/describe | grep -c /billing/buy     # 0
curl -s -o /dev/null -w '%{http_code}\n' 127.0.0.1:8080/billing/_quillo/describe   # 404
```

**Suggestion:** under `--split`, add the prefixed services' routes from their source, as `collect_file` already does. Alternatively, pass `--describe` down and merge each service's routes under its prefix. Either way, name `--describe` in the guide's list of what `--split` passes down.

### Q-398. The document's title depends on how the program was started, and nothing in it says the path it is served under

| Kind | Detected by | Attempts |
|---|---|---|
| silent | comparing the two documents' `info`, and `--base` | 1 |

`quillo docs main.qwe` titles the document `main`, after the file. `--describe` titles it after the program's folder (`api2`), and with nothing at all when the program is started as `quillo run main.qwe` from its own folder: `"info": { "title": "" }`. OpenAPI requires a title, and viewers show it as the API's name.

The document has no `servers`. Under `--base /shop`, where every link the program makes carries `/shop`, nothing in the document says so, and a gateway importing it routes `/users/{id}` at its root.

**Suggestion:** use one title both ways: the file's stem, or a title the program sets. Under `--base`, add `"servers": [{ "url": "/shop" }]`.

### Q-399. The guide says the app calls joined copies first; it takes them in turn with itself

| Kind | Detected by | Attempts |
|---|---|---|
| docs | counting which side answered 10 calls | 1 |

USERGUIDE.md › Services, "Copies that join", says: "The app calls the joined copies first, and falls back to its own when `billing` runs in its process (...), or to the addresses it was given". `make` in src/service.rs does otherwise:

- **The service runs in the app's process:** one call in N + 1 runs in the app, and the rest go to the N copies in turn. The app takes a call back when a copy misses it.
- **Addresses are given (`--service-url`):** the joined copies are added after the given addresses, and all of them are taken in turn.

With one copy joined, the copy answered 5 of 10 calls and the app answered the other 5. Someone who adds a copy to take load off a busy monolith expects all of the load to move.

```quillo
// main.qwe
service ledger = "ledger.qwe"
route GET /t = ledger.total()
```

```quillo
// ledger.qwe
server fn total() -> int = 42
```

```sh
export QUILLO_SERVICE_SECRET=0123456789abcdef0123456789abcdef
quillo run main.qwe --port 8080 &
quillo run ledger.qwe --as-service --port 8081 --join http://127.0.0.1:8080 > copy.log 2>&1 &
for i in $(seq 10); do curl -s 127.0.0.1:8080/t; done
grep -c service/total copy.log      # 5
```

**Suggestion:** say what the code does: "The app takes the joined copies in turn with itself (with one copy, half the calls stay in the app), and runs a call itself when a copy misses it; when it was given addresses, the copies come after them." If moving the load off the app is the aim, make the app skip itself while a healthy copy is in.

### Q-400. The docs say Quillo has no service discovery and list it as not built, while the code has it twice

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading the docs beside src/service.rs and src/cluster.rs | 1 |

Three places tell a reader looking for discovery that there is none:

- The microservices guide's limits open with "**No discovery.** Each caller is told where a service runs".
- The Services section says "Services are not found through limer's pool".
- Not built yet lists "discovering services through limer's pool".

What the code has is self-registration, which most systems call discovery:

| | Registers with | Every | Dropped after | When stopped | Code |
|---|---|---|---|---|---|
| a copy of a service, `--join URL` | the app, `POST /_quillo/join`, signed with `QUILLO_SERVICE_SECRET` | 5 s | 15 s | leaves | src/service.rs, `Joining`, `join_here` |
| any server, `--register URL` | limer's `discover` pool, through its admin API, with `LIMER_ADMIN_SECRET` | 5 s | 15 s (its TTL) | deregisters | src/cluster.rs |

What it does not have is a caller finding a service by name. An app still needs `--service-url` or `QUILLO_SERVICE_<NAME>` for a service it does not run itself, or must be the app the copies join.

**Suggestion:** replace "No discovery" with what is there and what is not: "**Discovery by registration.** A copy joins the app (`--join`), and any server joins limer's pool (`--register`). A caller does not look a service up by name, so a service the app does not run needs `--service-url`." Reword the Not-built-yet line to "a caller finding a service by name through limer's pool".

### Q-401. Neither feature has a heading or a page, and `--register`'s row links to a section that does not explain it

| Kind | Detected by | Attempts |
|---|---|---|
| docs | the site's navigation, and every heading in the guide, the website's guides and the book | 1 |

On the site, Services is a `###` inside the Routes page, and the API description is one bullet in it, "**Described.**", on a page of 196 KB. Discovery has no heading anywhere:

- `--join` is a bullet, "**Copies that join.**", in the same list.
- `--register` is "Several servers", under Components › Live data.

Of all the headings in USERGUIDE.md, the website's guides and the book, the only one that names either feature is the microservices guide's "Querying the API: `quillo docs` and `--describe`".

The option table's `--register` row says "see Concurrency", and Concurrency mentions `--register` only in passing, about draining and stopping. The environment table links to Several servers, correctly. "Services and scale" in Possible or not has no row for "describe my API for a gateway?" or "add a copy of a service while it runs?".

**Suggestion:**

- Under Services, add `#### Copies that join` and `#### The program's API (OpenAPI)`, made from the two bullets, which are long enough to need a heading.
- Link `--register`'s row to Several servers.
- Add two rows to Possible or not: "Yes: `quillo docs`, `--describe`" and "Yes: `--join`, or behind limer with `--register`".
- Run `assemble.py` again so the book has them too.

### Q-402. The changelog and the README name neither feature, though both shipped in 0.6.0

| Kind | Detected by | Attempts |
|---|---|---|
| docs | grep, and `git merge-base --is-ancestor` against v0.6.0 | 1 |

`quillo docs`, `--describe` and the contract a service checks (409) came in ab895a0. `/_quillo/ready` and `ready()` came in 5e50d17. Both commits are in v0.6.0. CHANGELOG.md's 0.6.0 lists `--split`, `--scale` and `--join` under Tools, and nothing on the API document, contracts or readiness.

README.md mentions none of OpenAPI, `describe`, `--join`, `--register` or `/_quillo/ready`. Its "Services that hold up" lists retries, several addresses, circuits, tracing and calls made later, but not that copies can join or that the API is described. The README is the GitHub page and the book's chapter 11, the first thing most readers see.

**Suggestion:** add a line to 0.6.0's Tools: "**`quillo docs` and `--describe`**: the program's API as OpenAPI 3, its routes and its server fns' contracts; a service deployed apart refuses, with 409, a call whose contract differs; `/_quillo/ready` and `ready()`." Add a clause to the README's services paragraph: "described as OpenAPI for a gateway (`quillo docs`), and given more copies while it runs (`--join`)".

### Q-403. Nothing says `/_quillo/describe` is public when it is on, and `/_quillo/join` is described nowhere

| Kind | Detected by | Attempts |
|---|---|---|
| docs, security | reading the hosting guide, SECURITY.md and src/service.rs | 1 |

Under `--describe`, anyone who can reach the port gets the document. It holds every route, including ones no page links to (`/admin/users/{id}`), each server fn's signature, the file and line of every route (`x-quillo-at`), and the binary's version and commit. The guide says the document is "for a gateway to import". The hosting guide's checklist keeps only `/_quillo/metrics` private, with the nginx and Caddy lines for it, and SECURITY.md does not name `/_quillo/describe` at all.

Every app that has services answers `/_quillo/join`. The guide names it once, in the list of Quillo's own addresses, but does not say:

- what it takes: a POST of `{ service, port, leave }`, signed as a service call is, with a nonce in `x-quillo-call`;
- what it answers: 200 `{ service, at }`, or 400, 401, 403, 404, 405 or 409;
- whether a proxy in front should pass it.

Someone writing a copy in another language, or writing a proxy's rules, has only the source to go on.

**Suggestion:** in the hosting guide, treat `/_quillo/describe` as metrics are treated: keep it private, or serve it only with `QUILLO_HEALTH_TOKEN`. Document `/_quillo/join` in the microservices guide's Payload section, beside `/_quillo/service/<fn>`, which it already documents for services written in other languages.

### Q-404. A copy that joins through the app's proxy is recorded at the proxy's address

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading `join_here` (src/service.rs) and `client_of` (src/httpd.rs:1593); not run, as it needs a proxy in front of the app | 0 |

`join_here` records a copy at the address its request came from, with the port the copy gives. That address is the TCP peer, unless the app runs with `--trust-proxy` naming the proxy.

So `--join https://app.example.com`, the address a person has at hand, goes through the app's proxy. The app records the proxy's address with the copy's port, which is not where the copy listens unless both run on one machine. Calls sent there fail, and the app runs them itself. The guide warns about a copy behind NAT, and its example joins the app's own address (`http://app:8080`), but it never says why.

**Suggestion:** add one sentence under "Copies that join": "Join the app's own address, not its public one: through a proxy the app sees the proxy, unless it runs with `--trust-proxy`." The app could also log a warning when a join comes from a proxy it does not trust.

### Q-405. Smaller gaps: what the document holds besides routes and contracts, and `QUILLO_SERVICE_NAME`

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading and running | 1 |

- Each server fn's `call` field is in no doc, and it is wrong for services (Q-395).
- Each operation has one response, `"200": { "description": "the route's answer" }`, with no content and no 400 or 404. The docs don't say so, so a reader doesn't know why a client generator makes untyped answers.
- A rest parameter, `/files/*path`, becomes `/files/{path}`, which an OpenAPI path template matches for one segment only, so a gateway does not route `/files/a/b`. The docs don't say so.
- `/<prefix>/_quillo/describe` answers the mounted program's or service's own document, with its paths without the prefix. The docs don't say so: the address list says what `/shop/_quillo/...` serves, but not this.
- `quillo --help` ([src/main.rs:67](../../src/main.rs)) and the option table ([USERGUIDE.md:44](../../USERGUIDE.md)) write "also `QUILLO_SERVICE_NAME`", next to `--service-name NAME`. It reads as a variable that sets the service's name. Nothing reads it: it means `QUILLO_SERVICE_<NAME>`, as the environment table writes.

**Suggestion:** a short "What the document holds" under the API section, and `QUILLO_SERVICE_<NAME>` in both places.

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| Took the site's guide for out of date: `content/USERGUIDE.md` links to `~/dev/wisp` at 1cf047c, before `describe.rs` | link.sh's default, and the 2,839-line guide behind the link | Deploys copy quillo-app1's guide into the release, and the live pages have the sections | 3 reads |
| Took `quillo docs` leaving out a service's server fns for the bug | the microservices guide says they are in the document | tests/describe_api.rs asserts it on purpose; `--describe` is the side that is wrong (Q-395) | 1 read |
| `echo ====` between commands | a bash habit | zsh reads `=word` as a command lookup, prints `==== not found` and stops | 2 commands |

## Limitations

- A caller cannot look a service up by name: it is given `--service-url`, or it is the app that copies join (Q-400 suggests how to word this).
- Not run here: `--register`, which needs limer (tests/cluster.rs runs it when `$LIMER` names one), and Q-404, which needs a proxy in front of the app.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Routes from a `routes /admin { ... }` group | listed by their whole path (`/admin/stats`, `/admin/users/{id}`), as the route-groups guide says |
| A file taken with `import lib/extra.qwe use helper` | its routes and its server fns are in both documents |
| A mount's server fn | `"/blog like"`, whose call, `POST /blog/_quillo/rpc/like`, answers 200 |
| A record type as a body (`{ json: Order }`), and `dec` | `{ "x-quillo-type": "Order" }` and `{ "type": "string", "format": "decimal" }`, as the microservices guide shows |
| A service declared without a path | its routes are not in the app's document, as the guide says (run `quillo docs` on its own file) |
| The guide's numbers for joining and limer | they match the code: a copy's heartbeat every 5 s and dropped after 15 s (src/service.rs `HEARTBEAT`, `JOINED_FOR`); limer every 5 s with a TTL of 15; an announcement of at most 256 topics, a minute old and 64 KiB; `--scale` from 1 to 64; `--join` needing `--as-service` and a `--port` |
| A copy joining | `quillo: service ledger: http://127.0.0.1:18934 joined` in the app's log, and it took calls at once |
| `--describe` under `--base /shop` | answered at `/_quillo/describe` (the proxy strips the prefix); `/shop/_quillo/describe` is 404, as the program serves no `/shop` path |
| The book | chapters 31 and 52 carry the guide's and the microservices guide's text word for word |
