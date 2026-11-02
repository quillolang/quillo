# Feedback from reviewing what Quillo lacks in HTTP, and whether it can serve plain HTML and other text

| | |
|---|---|
| App | None. Part 1 covers the HTTP Quillo speaks and what it lacks. As a server: HTTP/1.0 and 1.1, methods, statuses, content codings, conditional and range requests, slow clients, Server-Sent Events and the WebSocket handshake. As a client (`http.*`): redirects, repeated headers, 1xx answers, proxies, certificates and HTTP/2. Part 2 asks whether Quillo can serve or render plain HTML and other text, not only `.qwe`: running an `.html` or `.md` file, a folder of files behind `file()`, plain HTML pasted into a literal, and XML, Markdown, CSV and other types |
| Quillo | 0.6.0 (23146e4, 2026-11-02), built from `git archive 23146e4` in a scratch folder, so its `--version` names no commit. The checkout is on another session's branch, `feedback27-35-fixes`, with uncommitted changes to src/server.rs, src/upload.rs, src/builtins.rs and five other files (fixes for Q-330, Q-331, Q-361, Q-365 and Q-373). None of the findings below is among them |
| Model | Claude (Opus 5.5) |
| Tested with | curl 8.7.1; Python sockets for what curl will not send (HTTP/1.0, pipelining, `OPTIONS *`, a body sent slowly, WebSocket handshakes); a Python server standing in for other sites (two `Set-Cookie`s, a `103`, `307` and `308`, `br`, event ids); `openssl s_server` with a self-signed certificate; 20 HTML constructs, each pasted alone into a route; reading [src/httpd.rs](../../src/httpd.rs), [src/server.rs](../../src/server.rs), [src/fetch.rs](../../src/fetch.rs), [src/stream.rs](../../src/stream.rs), [src/ws.rs](../../src/ws.rs), [src/builtins.rs](../../src/builtins.rs) (`file`, `mime`, `respond`), ureq 2.12.1's agent and redirect code, and USERGUIDE.md |

No Quillo source was changed: the user asked for no code change yet. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched, as another session is working in the checkout; each repro is in its section. Numbers start at Q-432, after FEEDBACK35.md's Q-431. Line numbers are 23146e4's.

What worked well: the HTTP/1.1 Quillo speaks is strict where it matters, as FEEDBACK31 found. Keep-alive and pipelining answer in order. A chunked body with a trailer is read. An HTTP/2 preface gets a clean 400, and an `Upgrade: h2c` is ignored, as it should be, with a 1.1 answer. A 304 goes out with no body. `Date` is formatted once a second. A file's gzipped copy is kept by its `ETag`. `Range` and `If-Range` work on files. A stream asks proxies not to buffer it (`x-accel-buffering: no`). `HEAD` on a stream does not run its function. The client decodes gzip, follows 301, 302 and 303 as browsers do, and joins an event's `data:` lines. As for other text, `respond(200, text, { "content-type": ... })` serves RSS, CSV or any text correctly. `file()` gives 31 extensions their type with `charset=utf-8` on text. Pasted HTML takes entities, bare attributes, SVG's `viewBox`, custom elements, `<template>` and `//` in text as written.

Fix Q-432 first: every upload that takes more than 30 seconds to arrive is reset without an answer and without a log line, so a phone on a weak link cannot send a photo. Then Q-433 and Q-447, which put wrong bytes on the wire: a `respond(103, ...)` leaves the client waiting, or gives it the next request's answer, and a range of an `.html` file is not the bytes its `Content-Range` names. Then Q-448: pasted HTML whose comments show on the page, and whose `{ ... }` runs as code.

## The answer

### What HTTP Quillo speaks

| | Server | Client (`http.*`) |
|---|---|---|
| HTTP/1.0 | taken, but answered with 1.1's framing (Q-434) | |
| HTTP/1.1 | yes: keep-alive, pipelining, chunked both ways, `100 Continue` | yes (ureq 2) |
| HTTP/2 | no; a proxy in front speaks it to browsers | no (Q-443) |
| HTTP/3 | WebTransport only, for channels and media; never pages | no |
| TLS | no: a proxy in front; `--tls-cert` is for WebTransport | yes, with ureq's bundled roots only (Q-442) |
| WebSocket | on Quillo's own paths; no route can take one | none |
| Server-Sent Events | `events`: data and an event name; no `id`, no `retry` (Q-444) | `http.stream`: data and name; `id` and `retry` dropped |
| Compression | gzip of answers (q-values not read, Q-439); no brotli or zstd; compressed request bodies not decoded (Q-438) | gzip decoded; `br` and `deflate` handed over as raw bytes (Q-438) |
| Conditional requests | files: `ETag`, `If-Modified-Since` (only an equal date, Q-441), `If-Range`; routes: none | none (`cache` is Quillo's own) |
| Ranges | files: one range; several ranges get the whole file | |
| Methods | any token reaches the routes; no `Allow`, no automatic `OPTIONS` (Q-440) | any method |
| 1xx | `100 Continue`; a `103` cannot be sent, and `respond(1xx)` breaks the connection (Q-433) | 1xx not skipped: a `103` costs 60 s and fails (Q-436) |
| Behind a proxy | `X-Forwarded-For` from `--trust-proxy` addresses; `X-Forwarded-Proto` for the cookie warning; no `Forwarded` (RFC 7239), no PROXY protocol | `HTTP_PROXY` and `HTTPS_PROXY` ignored (Q-442) |

### Whether Quillo can render plain HTML, or any text that is not `.qwe`

| Way | Today | Issue |
|---|---|---|
| `quillo run page.html` (or `notes.md`) | no: parsed as Quillo. `expected an expression, found <`; for `.md`, `unexpected character #` and a note that the code may be for a newer Quillo | Q-450 |
| `quillo run site/`, a folder with `index.html` and no `main.qwe` | no: `no program: site has no main.qwe`, exit 2 | Q-450 |
| One route over a folder, `route GET /*path = file("site/" + path)` | yes, for 31 extensions. `/` and `/docs/` are 404, as nothing reads `index.html`, and `/about` does not find `about.html`. Every `.html` file up to 1 MiB gets Quillo's prefetch script spliced in | Q-450, Q-447 |
| `raw(read_file("page.html"))` | yes, as a Quillo page, with Quillo's scripts | |
| Plain HTML pasted into a literal | not as written: 9 of 20 constructs are refused, and 2 come out wrong without a word | Q-448, Q-449 |
| Markdown | `markdown(read_file("notes.md"))` gives an HTML fragment (no `<head>`, no doctype); a `.md` file is served as `text/plain` | Q-451 |
| XML (sitemap, RSS), CSV, iCalendar, any text | yes, with `respond(200, text, { "content-type": ... })`. An XML literal parses, but goes out as `text/html` | Q-451 |
| A type `file()` does not know (`.vtt`, `.ics`, `.yaml`, `.m3u8`, `.jsonld`) | `application/octet-stream` with `nosniff`. On this build `file(p, { type })` passes `quillo check` and is ignored (Q-331); `feedback27-35-fixes` adds `type` | Q-451 |

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-393 (FEEDBACK31) | `HEAD` says `content-length: 0` for a route (`/big`, 5,000 bytes) and for a file (`/files/big.txt`, 5,001 bytes) alike. `GET http://127.0.0.1/hello HTTP/1.1` (the absolute form, which RFC 9112 §3.2.2 says a server must accept) is a 404 | 1 |
| Q-411 (FEEDBACK33) | `OPTIONS /hello` is a 405 with no `Allow` and no CORS headers, as documented; see Q-440 for what it does to health checks | 1 |
| Q-331 (FEEDBACK27) | `file("site/" + p, { type: "text/vtt" })`: `quillo check` says ok, and the answer is `application/octet-stream`. The option is not read on 23146e4; the other session's branch adds it | 1 |
| Q-391 (FEEDBACK31) | With no TLS in Quillo, `Strict-Transport-Security` can only come from the proxy, as no program can add a header to every page | 0 (reading) |

## New issues, part 1: HTTP

### Q-432. A request whose body takes more than 30 seconds to arrive is reset, with no answer and no log line: a 10 MiB upload needs an uplink of 2.8 Mbit/s

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a body sent slowly | 1 |

The 30 seconds of `SLOW` ([src/httpd.rs:39](../../src/httpd.rs#L39)) count from the first byte of a request to its last ([src/httpd.rs:869](../../src/httpd.rs#L869)), not from one byte to the next. A body that keeps arriving at a steady rate is cut off all the same. A 700 KB body at 20 KB/s, a weak phone link, was reset after 31.1 s with 620,000 bytes in:

```sh
# main.qwe: route POST /echo = { len: len(request.body) }
python3 - <<'EOF'
import socket, time
s = socket.create_connection(("127.0.0.1", 4811))
s.sendall(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 700000\r\n\r\n")
for _ in range(35): s.sendall(b"a" * 20000); time.sleep(1)
EOF
# BrokenPipeError after 31.1 s; the read gives ConnectionResetError. No 408, nothing in the log
```

At the default `--max-body` of 10 MiB, a file arrives in time only on an uplink of 2.8 Mbit/s or more. The guide's upload section says to "raise [`--max-body`] for larger files", which makes it worse. It says nothing of the time, and "a request still arriving after 30 s is dropped" is in another section. The log shows nothing, so the site's owner never learns that uploads fail; the visitor sees a network error. A proxy that buffers bodies (nginx does by default) hides it. A proxy that passes a body on as it comes does not.

**Suggestion:** time a stall, not a request: drop a connection that sends nothing for 30 s, as nginx's `client_body_timeout` does between two reads, or ask for a minimum rate. When one is dropped, answer `408` if the head has been read, and log it (`POST /echo -> 408 (body still arriving after 30 s)`). The upload section should say what limits the time.

### Q-433. `respond` takes a 1xx status, which goes out as an interim answer: the client waits for ever, and on a pipelined connection it takes the next request's answer for this one's

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl, then a pipelined pair of requests | 1 |

`respond` takes 100 to 599 ([src/builtins.rs:1265](../../src/builtins.rs#L1265)). A 1xx is not a final answer: the client reads it as "more to come" and waits for the real one, which never comes. curl gave up at its own `--max-time` for each of 100, 101, 103, 150 and 199. When a second request follows on the connection, its answer is taken as the first one's. The second request's own answer then never comes:

```quillo
route GET /hints = respond(103, "", { link: "</app.css>; rel=preload; as=style" })
route GET /hello = "hi"
```

```text
GET /hints, then GET /hello, on one connection:
HTTP/1.1 103            <- the client waits on
HTTP/1.1 200 OK ... hi  <- taken as /hints's answer; /hello's never comes
```

A `101` tells the client that the connection now speaks another protocol, when it does not. The log says `GET /hints -> 103 (0.0ms)` as for any answer. A proxy that keeps connections to Quillo open waits out its own timeout, and a client that pipelines is answered out of turn. `103` is the one a program would try on purpose, for Early Hints, and nothing says it cannot be sent that way.

**Suggestion:** refuse 100 to 199 in `respond` and `reply` (a runtime error, and `quillo check` for a written status). Early Hints, if wanted, is the server's job: the links of `preload` and `preconnect` libraries are known when the program loads, so a page's `103` could carry them before the route runs.

### Q-434. An HTTP/1.0 client is answered with HTTP/1.1's framing: streams and deferred pages come chunked, and `Expect: 100-continue` gets a `100 Continue`, which a 1.0 client takes for the answer

| Kind | Detected by | Attempts |
|---|---|---|
| silent | raw HTTP/1.0 requests | 1 |

The head of a streamed answer always says `transfer-encoding: chunked` ([src/httpd.rs:1750](../../src/httpd.rs#L1750)). RFC 9112 §6.1 says a server must not send `Transfer-Encoding` unless the request is HTTP/1.1 or later, as a 1.0 client cannot decode it. `100 Continue` is sent to any request that asks ([src/httpd.rs:1207](../../src/httpd.rs#L1207)). RFC 9110 §10.1.1 says to ignore that expectation in an HTTP/1.0 request, since 1.0 has no interim answers:

```sh
printf 'GET /s HTTP/1.0\r\n\r\n' | nc 127.0.0.1 4811
# HTTP/1.1 200 OK ... transfer-encoding: chunked ... 4 / one / 4 / two / 0
printf 'POST /hello HTTP/1.0\r\nExpect: 100-continue\r\nContent-Length: 5\r\n\r\n' | nc 127.0.0.1 4811
# HTTP/1.1 100 Continue   (a 1.0 client stops here: this is its answer)
```

The same holds for `events`, and, by reading [src/server.rs](../../src/server.rs), for a page with waiting components (`defer`), which streams. A 1.0 client sees the chunk sizes in the body. HTTP/1.0 is still sent by ApacheBench (`ab`), by HAProxy's `option httpchk` unless told otherwise, and by some monitors.

**Suggestion:** for a 1.0 request, send a stream with no length and close the connection at its end, as 1.0 does, and send no `100`.

### Q-435. The client keeps only the first of a repeated header: a second `Set-Cookie`, a second `Link`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a server that sends two of each | 1 |

`r.headers` is made from each name's first value ([src/fetch.rs:251](../../src/fetch.rs#L251)): `resp.header(&n)` in ureq gives the first. A sign-in to another service that sets a session cookie and a CSRF cookie gives the program one of them:

```quillo
let r = http.get("http://127.0.0.1:4812/cookies")
// the server sent Set-Cookie: a=1, Set-Cookie: b=2, Link: </x>; rel=a, Link: </y>; rel=b
print(r.headers["set-cookie"], r.headers.link)   // a=1 </x>; rel=a
```

The same loses a second `Link` (pagination in GitHub's API is one `Link` header, but other APIs send several), `WWW-Authenticate` and `Vary`. `http.stream` builds its headers the same way.

**Suggestion:** join a repeated header with `, `, as RFC 9110 §5.3 allows, and give `set-cookie`, which cannot be joined, as a list (as `fetch`'s `getSetCookie()` does), or as `r.cookies`.

### Q-436. A call to a server that sends `103 Early Hints` before its answer waits its whole time limit, 60 s, and fails with "timed out reading response"

| Kind | Detected by | Attempts |
|---|---|---|
| loud, late | a server that sends a 103 | 1 |

RFC 9110 §15.2 says a client must be able to read one or more 1xx answers before the final one. ureq 2 takes the `103` as the answer and waits for its body, which has none:

```quillo
let r = http.get("http://127.0.0.1:4812/early")   // 103 with a Link, then 200 "after hints"
print(error(r))   // http: timed out reading response   (after 60.02 s)
```

With `{ timeout: 3 }` it fails after 3 s instead. The answer never arrives either way. Node's `res.writeEarlyHints()` and Go's `net/http` send 103s, so an API built on them is out of reach. The error does not mention the 103.

**Suggestion:** skip 1xx answers (other than `101`, which the client never asks for) until the final one, in the client Quillo uses or in a wrapper around it.

### Q-437. The client drops `Authorization` on every redirect, the same host's too, and does not follow a `307` or `308` after a POST, PUT, PATCH or DELETE

| Kind | Detected by | Attempts |
|---|---|---|
| silent | redirects from a test server | 1 |

ureq's default is `RedirectAuthHeaders::Never`. An API that redirects `/v1/items` to `/v1/items/` (Django's `APPEND_SLASH` does) answers the program 401, though the redirect stays on its own host. Browsers' `fetch` drops `Authorization` only when the redirect goes to another origin. A 307 or 308 is followed only for GET, HEAD, OPTIONS and TRACE (ureq's `unit.rs`), so for a write the redirect is the answer:

```quillo
let u = "http://127.0.0.1:4812"
http.get(`${u}/same-host`, { authorization: "Bearer k" }).status   // 401: /needs-auth got no Authorization
http.post(`${u}/r307`, { a: 1 }).status                            // 307, ok false, empty body
http.put(`${u}/r308`, "x").status                                  // 308
```

The guide says `redirects` is "how many to follow" (5 unless given). It says nothing about which ones, or what is sent again. An API that has moved with a 308, which is what 308 is for, fails each write while its reads work.

**Suggestion:** keep `Authorization` and cookies on a redirect to the same origin (ureq has `RedirectAuthHeaders::SameHost`). Follow a 307 or 308 with the same method and body, which the client still holds. Write both rules in the guide.

### Q-438. A compressed body is taken as its compressed bytes: a request sent with `Content-Encoding: gzip` reaches the route so, with `request.json` null, and a `br` or `deflate` answer reaches the client so

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl; a test server | 1 |

```sh
echo '{"a":1}' | gzip -c | curl -s -X POST -H 'Content-Type: application/json' \
  -H 'Content-Encoding: gzip' --data-binary @- http://127.0.0.1:4811/echo
# {"body":"\u001f�\b\u0000...","json":null,"len":28}
```

RFC 9110 §8.4 asks a server that cannot decode a coding to answer `415`. Here the route runs on the bytes. Senders that gzip what they post include OpenTelemetry's OTLP/HTTP exporters and several error and log collectors. The other direction: a server that answers `br` or `deflate` (unasked, as ureq says `Accept-Encoding: gzip`) gives the program `r.body` as raw bytes, `�hello` and `x��H���...`, with the `content-encoding` header left in `r.headers` and no word.

**Suggestion:** on the server, decode `gzip` (it is already linked for answers, `flate2`) and refuse other codings with 415. In the client, decode `deflate`, and make an unknown coding an error value that names it.

### Q-439. `Accept-Encoding: gzip;q=0` gets gzip, and `GZIP` does not

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | curl with eight `Accept-Encoding` values | 1 |

`accepts_gzip` ([src/server.rs:2130](../../src/server.rs#L2130)) asks whether an item starts with `gzip`. `q=0` means "not this one" (RFC 9110 §12.5.3), and codings are case-insensitive:

| `Accept-Encoding` | Answer |
|---|---|
| `gzip`, `gzip;q=0`, `deflate, gzip;q=0` | gzip |
| `GZIP`, `x-gzip`, `*` | not compressed |

**Suggestion:** read the list as RFC 9110 says: names in any case, `q=0` refuses, and `*` stands for gzip when gzip is not named.

### Q-440. A 405 has no `Allow`, an `OPTIONS` request is a 405 or a 404 unless the program declares one, `OPTIONS *` is a 404, and an unknown method is a 405 rather than a 501

| Kind | Detected by | Attempts |
|---|---|---|
| loud | raw requests | 1 |

RFC 9110 §15.5.6 says a 405 must carry `Allow`, naming the methods the path takes. Quillo knows them, as it has just matched the path ([src/server.rs:600](../../src/server.rs#L600)), and sends none:

| Request | Quillo | RFC 9110 |
|---|---|---|
| `DELETE /hello` (GET and POST routes) | 405, no `Allow` | 405 with `Allow: GET, HEAD, POST` |
| `OPTIONS /hello` | 405 | 200 or 204 with `Allow` (§9.3.7) |
| `OPTIONS * HTTP/1.1` | 404 | the server's options |
| `FROB /hello` | 405 | 501 for a method it does not know (§9.1) |

HAProxy's `option httpchk` sends `OPTIONS / HTTP/1.0` unless told otherwise, so a server pointed at Quillo with the defaults marks it down: a 405 (or 404) here, and a `100 Continue` problem besides (Q-434). It works when `/_quillo/health` is named, but nothing says to.

**Suggestion:** send `Allow` with every 405. Answer `OPTIONS` on a known path, when no route takes it, with 204 and `Allow`, and `OPTIONS *` the same. Give 501 to a method no route names anywhere.

### Q-441. Caches revalidate less than they could: a later `If-Modified-Since` gets the whole file, a route's own `ETag` never gets a 304, and `/_quillo/media.js` has no validator

| Kind | Detected by | Attempts |
|---|---|---|
| perf | curl with conditional headers | 1 |

A file is a 304 only when `If-Modified-Since` equals its `Last-Modified` ([src/builtins.rs:1057](../../src/builtins.rs#L1057)). RFC 9110 §13.1.3 asks for a 304 whenever the file has not changed since the date given, which includes a later date and RFC 850's form of it:

| Request on a file | Quillo | RFC 9110 |
|---|---|---|
| `If-Modified-Since:` its `Last-Modified` | 304 | 304 |
| `If-Modified-Since:` now | 200, the whole file | 304 |
| `If-Modified-Since: Monday, 02-Nov-26 23:59:59 GMT` | 200 | 304 |
| `If-Match: "nope"`, `If-Unmodified-Since:` 2001 | 200 | 412 (§13.1.1, §13.1.4), on a GET as on a write |

A route that sets its own `etag` gets a 200 to a matching `If-None-Match`: only `file()` and `quillo.js` compare. A program can do it by hand, reading the header and answering `respond(304, "", {})`, but the guide shows no such thing. `/_quillo/media.js` (39 KB) is `no-cache` with no `ETag`, so each page that plays a stream fetches it whole. `quillo.js` beside it has an `ETag`.

**Suggestion:** compare dates as RFC 9110 says. Give a route's answer that carries an `etag` a 304 when `If-None-Match` matches it. Give `media.js` an `ETag`, or a hashed address as the bundles have.

### Q-442. The client ignores `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY`, trusts only its bundled certificate authorities, and cannot present a certificate: a server behind a company proxy, or one calling an internal service, cannot use `http.*`

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a listener as the proxy; `openssl s_server` | 2 |

ureq is built without its `proxy-from-env` feature, and Quillo's agent ([src/fetch.rs:205](../../src/fetch.rs#L205)) sets no proxy. With all four proxy variables pointing to a listener, the call went straight out (`Dns Failed`), and the listener saw nothing. Certificates are checked against ureq's bundled roots (webpki-roots): not the system's store, not `SSL_CERT_FILE`, and there is no option for a CA file. The database has one (`sslrootcert`, and `sslrootcert=system`), so an internal CA works for PostgreSQL and not for HTTP. No option sends a client certificate either, which some bank and government APIs require (mutual TLS).

**Suggestion:** read the proxy variables (ureq's `try_proxy_from_env(true)`), add `ca` (a PEM file, or `"system"`) and `cert`/`key` to the options, and list them under "Possible or not".

### Q-443. The client speaks HTTP/1.1 only, so services that take only HTTP/2 cannot be called: Apple's push notification service, and gRPC

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading ([src/fetch.rs](../../src/fetch.rs), ureq 2's features); not run against Apple | 0 |

ureq 2 has no HTTP/2. Apple's documentation for APNs asks for HTTP/2 and TLS 1.2 or later. gRPC needs HTTP/2 and trailers. A phone app made with Quillo's mobile branch can therefore not have its server push to iPhones, unless another program does it (FCM's v1 API takes HTTP/1.1). Neither the guide's "Possible or not" nor the phone apps guide says so.

**Suggestion:** say so in "Possible or not" now. A client with HTTP/2 (ureq 3 has none either; `reqwest` or `hyper` with `h2` would) is the fix, and it would also bring Q-435 to Q-437 in line with browsers.

### Q-444. Server-Sent Events carry no `id` and no `retry`, and `listen` reconnects without `Last-Event-ID`, so a dropped stream starts over

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading [src/stream.rs:152](../../src/stream.rs#L152) and the runtime; a test server | 1 |

`send(value, event)` writes `event:` and `data:` and nothing else. The browser's `listen` reads the stream with `fetch` and opens it again after a drop, without the last id, which it never had. A progress stream or a model's answer that loses its connection is sent again from the start, or not at all. The program could read `request.headers["last-event-id"]`, but it cannot write an id, so nothing would ever send one. `http.stream` reading another server's events drops `id:` and `retry:` too (the test server's `id: 7`, `retry: 1000` are lost). Nor does a quiet stream send anything by itself: a proxy that cuts idle connections (nginx after 60 s) ends it unless the program calls `send(null)` itself, as the guide says.

**Suggestion:** `send(value, { event, id, retry })`. `listen` sends `Last-Event-ID` when it reconnects, and `http.stream` gives each event's `id`.

### Q-445. The WebSocket handshake accepts any version, method and HTTP version, and never offers compression

| Kind | Detected by | Attempts |
|---|---|---|
| small | raw handshakes on `/_quillo/live` | 1 |

`Socket::accept` ([src/ws.rs:28](../../src/ws.rs#L28)) checks only that `Sec-WebSocket-Key` is there. `Sec-WebSocket-Version: 8`, a `POST`, and an `HTTP/1.0` request were each answered `101`. RFC 6455 §4.2.1 asks for a GET over HTTP/1.1 and version 13, and §4.2.2 for a `426` naming 13 otherwise. Only Quillo's own pages connect, so this is untidy rather than open. `permessage-deflate` (RFC 7692), which every browser offers, is never accepted, so live data's JSON goes uncompressed over the socket. A long poll's answer is gzipped.

**Suggestion:** check the method, the version and the `Connection` and `Upgrade` tokens, and answer 426 or 400. Weigh `permessage-deflate` against its memory for 4096 pages (it can be offered without context takeover).

### Q-446. Smaller departures from HTTP/1.1

| Kind | Detected by | Attempts |
|---|---|---|
| small | raw requests | 1 |

| Case | Quillo | Expected |
|---|---|---|
| A URL of 70,000 bytes | 431 "request headers too large" | 414 URI Too Long (RFC 9110 §15.5.15) |
| A head that never ends | closed after 30 s, nothing sent | a 408 first (RFC 9110 §15.5.9: "should") |
| `respond(205, "body")` | the body is sent | no content in a 205 (§15.3.6: "must not") |
| `respond(103 \| 205 \| 418 \| 451, ...)` | an empty reason phrase, `HTTP/1.1 418 ` | allowed; `reason()` knows 30 statuses |
| `Expect: foo` | ignored, the request answered | may be 417 |
| `100 Continue` for a path that is 404 | sent, and the body read before the 404 | allowed; the 404 could go first, saving the upload |

**Suggestion:** 414 for a long request line, 408 before closing, no body on a 205. The rest are a matter of choice.

## New issues, part 2: plain HTML and other text

### Q-447. `file()` splices Quillo's prefetch script into every `.html` file up to 1 MiB, so a range of it is not the bytes its `Content-Range` names, and the file's `ETag` goes with a body that is not the file

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl on a folder of plain files | 2 |

`Interp::handle` adds the prefetch script to any `text/html` answer with a `</body>`, whatever made it ([src/server.rs:393](../../src/server.rs#L393)). That includes a file `file()` serves. The status is not checked, so a `206` gets it too:

```quillo
route GET /*path = file("site/" + path)
```

```sh
curl -s -D - -H 'Range: bytes=702-731' http://127.0.0.1:4821/index.html   # the file is 732 bytes
# HTTP/1.1 206 Partial Content
# content-range: bytes 702-731/732
# content-length: 1349        <- 30 bytes asked for, 1,349 sent: the script is inside the range
```

A download resumed with `Range` (curl `-C -`, a download manager) writes the script into the middle of the file. A 22-byte `about.html` goes out as 1,341 bytes under the file's own `ETag` and `Last-Modified`. When Quillo's script changes in a new release, a browser's copy still revalidates as `304`, with the old script in it. A page whose own Content Security Policy forbids inline scripts gets a console error for a script it never wrote. A file over 1 MiB, sent from disk, is left alone, so whether a file is changed depends on its size. Under `--watch`, the development script is added in the same way.

**Suggestion:** leave what `file()` sends as it is on disk, and add the script only to pages a route renders. If the prefetching is wanted on static pages, make it an option (`file(p, { prefetch: true })`), and never on a 206.

### Q-448. Plain HTML pasted into a literal is changed without a word: `<!-- comments -->` show on the page, and any `{ ... }` in text runs as code, so `{ return "x" }` in a `<pre>` replaces the whole page

| Kind | Detected by | Attempts |
|---|---|---|
| silent | 20 HTML constructs, each pasted into a route | 2 |

```quillo
route GET /a = <div><!-- note --><p>a</p></div>
route GET /b = <main><h1>Docs</h1><pre>x { return "hidden" } y</pre><p>after</p></main>
```

```text
GET /a -> <div>&lt;!-- note --&gt;<p>a</p></div>        the comment is text on the page
GET /b -> hidden   (content-type: text/plain)          the page is gone
```

`quillo check` says ok to both. In markup, `{...}` is an expression, which the guide says. A block's `return` returns from the route, so text that happens to hold `{ return ... }` ends the request with that value. `<pre>f() { return 1 }</pre>` answers `1` as JSON. Code samples are what a `<pre>` holds, so a docs page pasted from HTML is where this shows. A sample with a colon in its braces (`{ a: 1 }`) is refused, which is loud (Q-449). The guide gives markup comments as `//` lines and `{/* */}`, and says nothing of `<!--`.

**Suggestion:** take `<!-- ... -->` as a comment (dropped, or kept as an HTML comment), and have `quillo check` warn of a `return`, `break` or `continue` in an expression child of markup. Everywhere else in markup, braces would stay code.

### Q-449. Plain HTML pasted into a literal is refused for 9 of 20 constructs, and the guide has no word on bringing HTML in

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | 20 HTML constructs, each pasted into a route | 1 |

A realistic page (doctype, a `<style>`, a `<script>`, single-quoted and unquoted attributes, unclosed `<p>` and `<li>`) fails at its first `{` in the `<style>`. Each construct alone:

| HTML as written | Quillo |
|---|---|
| `<!DOCTYPE html>` | `expected an expression, found <`; `<html>` gets one by itself, as the guide says |
| `<style>body { color: red }</style>` | `expected } , found :`; must be `` <style>{css`...`}</style> `` |
| `<p class='a b'>` | `attribute class needs a "string" or {expression} value` |
| `<h1 id=top>` | the same |
| `<div><p>one<p>two</div>` | `` `<p>` is never closed: add `</p>` before `</div>` `` (HTML closes it by itself) |
| `<ul><li>one<li>two</ul>` | the same for `<li>` |
| `<script>if (1 < 2) { console.log(1) }</script>` | `unknown name console`: the script is read as Quillo |
| `<DIV CLASS="a">` | `unknown name DIV`: taken as a view (HTML's tags are case-insensitive) |
| `<p>costs $5 and ${"{"}</p>` | a warning: `the $ before this { is text` |
| entities, bare attributes, `onclick="..."`, `//` in text, SVG's `viewBox`, `<my-widget>`, `<template>`, an XML tree | taken as written |

The errors are clear and each names its fix, so this is not a trap. The cost is a page's worth of edits, and the guide has no section saying what changes from HTML. `quillo fix` does not convert any of it.

**Suggestion:** a short "From HTML" list in the guide: the rows above and their fix. Take `'...'` and unquoted attribute values, as HTML does. Then either `quillo fix` for the mechanical ones (quotes, closing tags, `<style>` into `` css`...` ``), or a way to include an HTML file as it is (`html_file("page.html")`, which is `raw(read_file(...))` with a check of where the file is).

### Q-450. `quillo run page.html` reads HTML as a Quillo program, a folder of plain files is "no program", and nothing serves `index.html` for `/` or `/docs/`

| Kind | Detected by | Attempts |
|---|---|---|
| docs, limitation | the command line, and curl on a folder | 1 |

```sh
quillo run page.html     # page.html:1: error: expected an expression, found `<`
quillo run notes.md      # notes.md:1: error: unexpected character `#`
                         # note: ... if the code was written for a newer Quillo, that would explain it
quillo run site          # quillo: no program: site has no main.qwe; name a .qwe file   (exit 2)
```

Every file is parsed as Quillo, whatever its extension. The message for `.md` points at the wrong cause. Neither says that `.html` is not a program, or how to serve one. The way to serve a folder is one route, `route GET /*path = file("site/" + path)`. Through it, `/`, `/docs` and `/docs/` are 404, as `index.html` is never looked for, and `/about` does not find `about.html`. Every static host does both. The guide's FAQ, "serve a site as static files?", answers the other way round: exporting a Quillo site as files.

**Suggestion:** for a file whose extension is not `.qwe`, say what it is and how to serve it (`page.html is HTML, not a Quillo program: serve it with file(), see Static files`). Give `file()` a folder form, `files("site", { index: "index.html", extensions: ["html"] })`, that does the index and clean URLs. A `quillo serve DIR` would make Quillo usable as a static server too.

### Q-451. Text types `file()` does not know go out as `application/octet-stream` with `nosniff`, an XML literal goes out as `text/html`, and `markdown()` gives a fragment

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | curl on a folder of plain files | 1 |

`mime` ([src/builtins.rs:967](../../src/builtins.rs#L967)) knows 31 extensions. Among those it does not, many are text a site serves:

| File | Sent as | Its type |
|---|---|---|
| `subs.vtt` (subtitles for `<track>`) | `application/octet-stream` | `text/vtt`, which the HTML standard has a `<track>` check for (not tried in a browser) |
| `cal.ics` | `application/octet-stream` | `text/calendar` |
| `live.m3u8`, `.ts` (HLS) | `application/octet-stream` | `application/vnd.apple.mpegurl`, `video/mp2t` |
| `data.yaml`, `.toml` | `application/octet-stream` | `application/yaml`, `application/toml` |
| `x.jsonld`, `.rss`, `.atom` | `application/octet-stream` | `application/ld+json`, `application/rss+xml`, `application/atom+xml` |
| `.mov`, `.m4a`, `.aac`, `.flac`, `.opus`, `.bmp` | `application/octet-stream` | their own |
| `notes.md` | `text/plain; charset=utf-8` | `text/markdown; charset=utf-8` (or rendered) |

With `nosniff` on every answer, a browser takes the type as given and does not guess, so a feed reader or a calendar app that is handed the octet-stream downloads it as a file. A sitemap written as a literal (`<urlset xmlns=...>`) is valid XML, and goes out as `text/html; charset=utf-8` unless wrapped in `respond`. `markdown(read_file("notes.md"))` as a route's answer is `<h1>...</h1>...` with no doctype, `<head>` or `<title>`, which a browser shows in quirks mode.

**Suggestion:** add the types above. Make `{ type }` work (done on the other session's branch, for Q-331). Have a route whose value is markup with an XML namespace at its root, or that starts `<?xml`, answer `application/xml`. A `markdown_page(text, { title })` (or a guide example with a view around it) would answer Markdown as a full page.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `"x" * 5000` for a large body | Python's habit | `quillo check`: `cannot multiply str and int`; `repeat("x", 5000)` is the builtin | 1 check |
| Used `./target/release/quillo` from the checkout | it was built today | it holds another session's uncommitted `feedback27-35-fixes`, so `file(p, { type })` would have worked there and not on the commit; built 23146e4 from `git archive` instead | 1 build |
| `curl -o b.out` in a loop over statuses | the quick way | an answer with no body (304) leaves the file as the last one wrote it, so a 304 "had a body"; `nc` showed none | 1 rerun |
| The test server's `do_HEAD` writes a body | one handler for all methods | the pooled connection was out of step, and the next call (OPTIONS) failed: it looked like Quillo's error | 1 rerun |
| `pkill -f 'run pre.qwe'` to stop a probe | habit | refused, rightly: other sessions run `quillo` too; only the PID started is stopped | 1 |
| Pasted the whole plain page into a literal | the question was whether it works | the first error stopped it, which said nothing of the rest; each construct alone gave the table in Q-449 | 1 file of 20 |
| `{ a: 1 }` and `{ y = 2 }` inside `<pre>` in one probe file | testing braces in text | each is a parse error, so the file never ran and hid the silent case (`{ return ... }`); split apart | 2 runs |

## Limitations

Missing by design, as the guide says, and what stands in for each:

- **TLS, HTTP/2 and HTTP/3 for pages.** A proxy in front (limer, Caddy, nginx) terminates TLS and speaks HTTP/2 and 3 to browsers. Quillo keeps live data on one WebSocket a page because HTTP/1.1 allows browsers six connections to a host, which a proxy speaking HTTP/2 would lift for pages as well. There is no ACME (Let's Encrypt) in Quillo, and HSTS can only be the proxy's (Q-391).
- **A WebSocket route of one's own, CORS preflights, rate limits**: not built in (`route OPTIONS` and `respond` for CORS; Q-411).
- **Bytes**: bodies are text, `file()` is the way bytes go out, and a binary body cannot be read as it came.
- **A request body as it arrives**: it is read whole, up to `--max-body`, before the route runs (and then Q-432 limits the time).
- **Brotli and zstd**: only gzip, for answers.
- **Several ranges in one request**: the whole file, documented.
- **Repeated request headers**: the last is kept (documented), where RFC 9110 §5.3 would join them. `Accept: text/html` and `Accept: application/json` sent apart read as `application/json` alone.
- **`Forwarded` (RFC 7239) and the PROXY protocol**: only `X-Forwarded-For` (from `--trust-proxy`) and `X-Forwarded-Proto`.
- **Trailers**: a request's are read and dropped; an answer cannot send any (gRPC would need them).

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `PRI * HTTP/2.0` (HTTP/2 with prior knowledge) | 400, and the connection closed |
| `Upgrade: h2c` with `HTTP2-Settings` | ignored, answered over HTTP/1.1 as RFC 9113 allows |
| A chunked request body with a trailer | read; the body is `abc` |
| `CONNECT 127.0.0.1:443`, `TRACE /hello` | 404, 405: no tunnel, no echo |
| `respond(304, "body")` | no body, no length |
| `HEAD` on a `stream` | the head only; the function is not run |
| A route's `vary: Cookie`, gzipped | `vary: Cookie` and `vary: accept-encoding`, two lines, which caches combine |
| Two `set_cookie`s in one request | two `Set-Cookie` headers |
| `Range` with `Accept-Encoding: gzip` on a file | 206, not gzipped, the range of the file's bytes |
| `HEAD` of a file's type | `application/octet-stream` for `.bin`, `text/html; charset=utf-8` for `.html` |
| The client: `http.request("get", ...)` | sent as `GET` |
| The client: 301, 302 and 303 after a POST | followed as a GET without the body, as browsers do |
| The client: `{ timeout: 3 }` against a stalled `103` | an error value after 3.00 s that names the timeout |
| The client: an SSE event with two `data:` lines | one piece, `"two\nlines"` |
| The client: `HEAD` | status 200, no body read |
| An XML tree, entities, bare attributes, SVG, custom elements and `<template>` in a literal | written as given (`&nbsp;` as the character itself) |
| `respond(200, read_file("feed.xml"), { "content-type": "application/rss+xml; charset=utf-8" })` | sent as given |

## Not tested

HTTP/2 and HTTP/3 behind a real proxy (Caddy, nginx, limer) with these requests, which is where browsers meet Quillo. nginx's default `proxy_http_version 1.0` against a stream (Q-434; nginx itself may read the chunked answer). HAProxy's `httpchk` (Q-440, from its documentation). Apple's push service (Q-443, from its documentation). A `<track>` given `application/octet-stream` in a browser (Q-451). Linux. A WebSocket kept open through a proxy's idle timeout. HTTP/1.1 pipelining from a browser, which none does by default.
