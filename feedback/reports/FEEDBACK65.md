# Feedback from running qwertime.com in production: a lambda in a `let` that leaks its scope, connections closed after a long answer, and no base64

| | |
|---|---|
| App | qwertime.com, outside this repo (`~/dev/qwertime.com`). It is a social network for mascots: PostgreSQL 16, two `quillo run` servers behind limer (a C reverse proxy), live updates through `events()`, an MCP door with 56 tools, and store webhooks. About 12,800 lines in 72 `.qwe` files, with 442 tests and 99.3% coverage. It has been live since 2026-11-10 |
| Quillo | Production runs `quillo 0.8.1 (89a91c2, 2026-11-09)`. Each issue below was checked again on this repo's `target/release/quillo`, `quillo 0.8.1 (01767d5, 2026-11-13)` on branch `feedback52`, on 2026-11-13 |
| Model | Claude (Opus 5.5, `claude-opus-5-5`), which built the app and ran the load tests |
| Tested with | the app's own tests (`quillo test`, plus Node HTTP tests). A load test of 1,000 members online at once, against a copy of the site on a test server (never production), with one vCPU and 512 MB a server. `footprint` on macOS, and raw TCP clients for the connection reuse |

No Quillo source was changed, and neither were `feedback/INDEX.md` or `feedback/repros/`. Each repro is in its own section. Numbers start at Q-941, after FEEDBACK61.md's Q-940.

What worked well: the hot paths take little code. A member's points, a feed page and a live stream each needed one module that keeps memory first, with the database behind it. The MCP door serves 56 tools whose read-only hints come from effect analysis, and no client needed a hand-written schema. Going from one server to two behind limer needed no change to the program. Webhooks from four e-commerce platforms were verified with `crypto.hmac` and `crypto.equal`, apart from the base64 of Q-943.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-928 | Each of 14 MCP tools had the name of a library function it called (`add_store`, `link_product`, `send_item`...). Because a tool's name is its function's name, the library functions were renamed instead (`stores_of`, `claim_with_code`, `answer_item`...) so that clients keep the tool names | 2 |

## New issues

### Q-941. A lambda kept in a `let` keeps its whole scope for ever, and leaks it on every call

| Kind | Detected by | Attempts |
|---|---|---|
| perf (memory), silent | a load test, then bisecting by kind of request | 3 load runs |

Under load, each server's memory grew by about 3.5 MB a second and never levelled off, until the kernel killed it. That was a cgroup OOM at 512 MB, four times in five minutes, and requests failed while the pool had one server or none. It was the same on Linux and on macOS, with and without limer, cluster mode or the view cache (`--no-view-cache`), over a Unix socket or TCP to PostgreSQL, and with every mimalloc purge setting. The leaked memory was still in use, not freed memory that the allocator kept. (On macOS, `ps` RSS hides it because of the memory compressor; `footprint` shows it.)

The cause: a lambda kept in a local (`let f = (x) => ...`) keeps the scope it was made in, and the scope keeps the lambda. Neither is freed when the call returns, so every value in that scope leaks each time the code runs, such as a page's rows or an upload's bytes. The lambda does not have to use them.

Expected: a scope and its lambdas are freed when nothing outside refers to them. That already happens for an inline lambda: `filter((r) => ...)` leaks nothing.

```quillo
route GET /plain = {
    let big = range(20000) | map((i) => `row number ${i} with some text`)
    `${len(big)}`
}
route GET /stored = {
    let big = range(20000) | map((i) => `row number ${i} with some text`)
    let keep = (r) => len(r) > 3
    `${len(big | filter(keep))}`
}
route GET /stored-unused = {
    let big = range(20000) | map((i) => `row number ${i} with some text`)
    let one = (x) => x + 1
    `${len(big)} ${one(1)}`
}
```

To reproduce: `quillo run repro.qwe`, send requests to each route 8 at a time, and read memory with `footprint -p <pid>` (macOS):

| route | 89a91c2, 1,000 requests | 16dfab4, 1,000 requests | 01767d5, 300 requests |
|---|---|---|---|
| /plain | 8.8 MB -> 10 MB | 5.2 MB -> 6.5 MB | 7.5 MB -> 7.2 MB |
| /stored | 25 MB -> 2,339 MB (1,500 requests) | 6.5 MB -> 1,557 MB | 7.2 MB -> 471 MB |
| /stored-unused | 2,339 MB -> 4,660 MB (1,500 requests) | 1,557 MB -> 3,108 MB | not run |

Each request leaks about 1.5 MB, the whole list, every time. `/stored-unused` shows that the lambda leaks the scope even when it reads nothing from it.

What it cost: in qwertime, eight such lambdas leaked about 120 KB on every feed page, and the uploaded photo on every upload. Among them were a post card's `small`, the live stream's `seen` and `wanted`, a cached profile's `mine`, and two upload routes' `at`. Finding them took three load runs and a bisection by kind of request. The workaround was to make each one a top-level `fn` (qwertime commit 259c545), and to add a check to its tests that refuses `let x = (...) =>`.

- Programs: any program that keeps a lambda in a local on a request's path leaks memory in proportion to its traffic, and nothing says so. `quillo check` gives no warning.
- Language: no change.
- Docs: nothing says to avoid it. USERGUIDE.md line 536 shows `let double = (x) => x * 2` as the ordinary way to name a lambda.
- Speed: memory only, as measured above; the outage came from the OOM kills.

Suggestion: the scope and the lambda form a reference cycle. Values never change in Quillo, so a lambda could keep copies of the variables it reads (its free variables) instead of the whole scope. A lambda that reads itself (a recursive `let f`) could then refer to itself without a strong reference. Until then, `quillo check` could warn about a lambda kept in a `let` on a request's path.

### Q-942. A connection whose answer took longer than a minute is closed right after it, without `connection: close`

| Kind | Detected by | Attempts |
|---|---|---|
| reliability, silent | a load test through limer, then a single raw TCP client | 2 |

When a streamed answer (an `events()` stream, a long download) takes longer than `IDLE` (60 s), the server closes the connection within a second of the answer's end, and sends no `connection: close`. The answer ends as a kept-alive one (chunked, its last chunk sent). So the client, or a proxy's pool, keeps the connection to use again, and its next request on it is cut off: EOF, or a reset if the request was already sent.

Through limer, each such reuse is counted as a failure against the server, and the request is tried again on the other server. qwertime's live streams last 2 minutes. When 1,000 streams opened together end together, three failures in a row take a server out of the pool for 10 s, and both servers went out. Every page was refused for 10 s every two minutes, 3% of requests over 5 minutes.

Expected: a connection is idle from the end of its last answer, as nginx's `keepalive_timeout` counts it. Or, when the server will not keep the connection, the answer says `connection: close`.

Why, in `src/httpd.rs` at 01767d5:
- `c.active` is stamped when a request starts being served (line 1369).
- When the answer is done, the slot goes back to `Slot::Idle(c)` (line 1611) without a new stamp.
- `sweep`, once a second, then sees `now - c.active > IDLE` (line 1287) and closes the connection.

So the connection looks idle for as long as its answer took. Lines 1611 and 1369 are unchanged from 16dfab4.

Repro:

```quillo
route GET /stream = events((send) => {
    for i in range(0, 22) { send({ i }); sleep(3000) }
})
```

On one TCP connection: read the whole stream (66 s), wait 2 s, then send `GET /robots.txt` on the same connection. This was checked with qwertime's own `/live` on 89a91c2 (the same shape: 22 updates, 3 s apart). The client saw "stream ended after 63.1 s ... connection closed by the server without an answer". A 3-second stream is answered, and so is a second request sent at once, before the next sweep.

What it cost: two load runs to see the pattern (outages every two minutes, in step with the streams), then reading `src/httpd.rs`. Worked around in qwertime in two ways. The streams end at a random 35 to 40 updates so they do not end together. And limer's passive ejection is turned off (`max_fails 0`), keeping the active health check every 2 s.

- Programs: any program with answers longer than a minute behind a proxy that pools connections (nginx `keepalive`, HAProxy, limer, a CDN) gets failures counted against it, and nothing in its log says so. A browser retries quietly. A client that pools connections, such as Node's `fetch` keep-alive agent, gets `ECONNRESET` or `socket hang up`.
- Language: no change.
- Docs: no change; nothing says how long a connection is kept.
- Speed: no change.

Suggestion: stamp `c.active = self.now` when an answer is finished and the connection goes back to idle (line 1611, and wherever a streamed answer ends). Idleness then counts from the end of the last answer.

### Q-943. There is no base64, so checking a Shopify or WooCommerce webhook takes a hand-written encoder

| Kind | Detected by | Attempts |
|---|---|---|
| missing | building store webhooks | 1 |

Shopify (`X-Shopify-Hmac-Sha256`) and WooCommerce (`X-WC-Webhook-Signature`) sign a webhook with HMAC-SHA256 in base64. `crypto.hmac` gives hex only, and nothing in the standard library converts hex or text to base64 or back. USERGUIDE.md says so ("Nothing decodes base64", "No base64 yet"), and `crypto.token` gives base64url but takes no input. So the program carries its own encoder:

```quillo
let HEX = "0123456789abcdef"
let B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

fn hex_to_base64(hex: str) -> str {
    let bytes = range(0, floor(len(hex) / 2)) | map((i) => (index_of(HEX, slice(hex, i * 2, i * 2 + 1)) ?? 0) * 16 + (index_of(HEX, slice(hex, i * 2 + 1, i * 2 + 2)) ?? 0))
    join(range(0, floor((len(bytes) + 2) / 3)) | map((g) => b64_group(bytes, g * 3)), "")
}
fn b64_group(bytes: list<int>, at: int) -> str {
    let left = len(bytes) - at
    let n = bytes[at] * 65536 + (left > 1 ? bytes[at + 1] : 0) * 256 + (left > 2 ? bytes[at + 2] : 0)
    `${b64_char(floor(n / 262144) % 64)}${b64_char(floor(n / 4096) % 64)}${left > 1 ? b64_char(floor(n / 64) % 64) : "="}${left > 2 ? b64_char(n % 64) : "="}`
}
fn b64_char(k: int) -> str = slice(B64, k, k + 1)
```

Its first form kept `b64_char` as a lambda in a `let`, which Q-941 would have leaked on every webhook.

- Programs: webhooks signed in base64 (Shopify, WooCommerce), a Basic auth header, a JWT's parts and a data URL all need base64. Webhooks signed in hex (Lemon Squeezy, Stripe, GitHub, Slack) are fine as they are.
- Language: none; this is the standard library.
- Docs: the guide's crypto line could say how to get a base64 HMAC.
- Speed: the hand-written encoder is fast enough for a webhook (44 characters).

Suggestion: `crypto.hmac(key, text, { encoding: "base64" })`, plus `base64.encode(text)` and `base64.decode(text)` (and their url forms), since `src/ws.rs` already has a `base64` function.

## Found while fixing (branch feedback65, 2026-11-13)

### Q-948. A pipelined request's `100 Continue` is written before the answer to the request ahead of it

| Kind | Detected by | Attempts |
|---|---|---|
| protocol | slice S1's follow-up (Q-942) | 0 |

When a request with `Expect: 100-continue` arrives pipelined behind another, its head partly or wholly read while the first is still being answered, the server writes its `100 Continue` at once, before the first request's answer. A client reads that interim response as belonging to the first request. Found while timing partial requests behind a long answer; on 0.8.1 too.

| Impact | |
|---|---|
| **Functionality** | Answers on a pipelined connection come out of order |
| **User** | A client that pipelines and sends `Expect` (rare: curl and most libraries do neither together) misreads the first answer |
| **Requests** | Only pipelined requests with `Expect: 100-continue`; a proxy in front (Limer, nginx) does not pipeline to the server |
| **Security** | No change: no data from one request reaches another |

### Q-949. A pipelined request whose last bytes come while an earlier answer is made can sit unread until its 408

| Kind | Detected by | Attempts |
|---|---|---|
| reliability | the second review of b55b9f2 and 86d6946 (review-S1-2.md); on 0.8.1 too | 0 |

When the requests ahead of it went into `pending`, the readable edge for a pipelined request's last bytes arrives while the slot is Busy and becomes `again`. `Loop::serve` takes the `pending` branch and starts the next batch without reading, and when that batch ends `finish`'s `more` is false (`receiving.is_some()`); no new edge comes. `GET /stream/2/15`, `GET /big/300000`, `GET /stream/2/15`, then `GET /after` with its final `\r\n` 10 ms later: three 200s, then a 408 at 30.1 s (6 of 6). With an `Expect` request last, its 100 never comes.

| Impact | |
|---|---|
| **Functionality** | A whole request is never answered; the client gets a 408 after 30 s |
| **User** | A pipelining client sees a random 30 s stall and an error |
| **Requests** | Only pipelined requests (Limer and nginx do not pipeline to the server) |
| **Security** | No change |

### Q-955. A still-held scope next to a list or record of lambdas pays a scan each time it ends

| Kind | Detected by | Attempts |
|---|---|---|
| speed | the review of Q-941's S2c round (review-S2c.md); follow-up to 7538584 | 0 |

Q-941's fix (7538584) frees a kept lambda's scope when nothing outside holds it. When the scope also holds a list or map, and its lambda is still held when the scope ends, each end walks the containers once (up to 128 items). Against the parent's build: a kept lambda beside a list of 3 or 100 items +11% / +16%, a record `{ go, pad: range(120) }` returning `acts.go` +21%, and with every result kept, mk +21%, a list +31%, a record +33%. The base leaked all of these (75 MB per 40 requests). The user accepted the cost for 7538584 (2026-11-14) with this filed to bring it down, for example by remembering a scope's verdict across calls of the same function.

| Impact | |
|---|---|
| **Functionality** | No change: the scopes are freed |
| **User** | Code keeping lambdas beside lists or in records runs 11-36% slower per call than 0.8.1 (which leaked) |
| **Requests** | A route of this shape pays per call; server CPU on plain routes unchanged |
| **Security** | No change |
