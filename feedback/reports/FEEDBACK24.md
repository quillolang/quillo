# How `live()` holds up with large records and 1000 active users

| | |
|---|---|
| App | None: one question, how `live()` performs on large records with 1000 active users at a time, answered by reading the code and the guide. Q-196 came from a second question, how Quillo makes a form idempotent |
| Quillo | 0.6.0 (57370bc, 2026-10-31), branch `feedback23-fixes`, with that branch's uncommitted work. It does not touch `src/server.rs`, `src/shared.rs`, `src/livecache.rs` or `src/client/runtime.js`, and its change to `src/builtins.rs` does not touch `live` |
| Model | Claude (Opus 5.5) |
| Tested with | **Nothing about `live()` was run.** No load test, no server, no browser. Everything on it comes from reading the code and `USERGUIDE.md`. Not checked: whether limer passes the client's address through (its source was not read). Q-196 alone was run, on 5f7f76c, with `curl` and headless Chrome on SQLite |

**Read this first.** There are no latencies here because none were measured. Counts of runs, wakes
and bytes follow from the code. Figures marked *est.* are estimates, or worked examples whose
inputs are assumed. The one measured number is older: FEEDBACK6 (Q-028) put 3,000 rows at 90,877
bytes, about 30 bytes a row, and a patch for one renamed row at 201 bytes.

What works well: the design is sound for the case it was built for. Many pages show the same data
and it changes now and then. The loader runs once, each page gets a patch of a few hundred bytes,
and a page with nothing to do costs a parked thread and a ping every 20 s. A page never has two
refreshes in flight, so a burst slows pages down rather than piling up without end. A restart with
unchanged data costs little too: the hash still matches, so a page is answered `{"$=":1}`.

The answer to the question: **it depends almost entirely on whether the loader is shared.**

| What the loader is | Loader runs per change | To each page | Verdict at 1000 pages |
|---|---|---|---|
| Same data for everyone, no `session()`, same arguments, SQLite | 1 | Patch, ~200 bytes for one row | Fine, as long as a run does not stall the loop |
| The same, on PostgreSQL | 1 or many (Q-191) | Patch | Fine at few writes; degrades as writes rise |
| Reads `session()`, cookies or the locale, or takes a user id as an argument | **1000** | Patch, while its value is kept | **Does not scale.** Time for all pages to catch up ≈ 1000 × run time ÷ instances |
| Personal and large (over ~64 KiB each) | **1000** | **Often the whole value**, gzipped per page (Q-193) | **Worst case** |
| Any of these, behind a proxy on one address | As above | As above | **Only 256 pages get updates at all** (Q-189) |

Worked example (*est.*): a personal loader taking 5 ms, 1000 pages, 4 instances, and a database
that keeps up. One change takes 1.25 s to reach every page, and the instances are busy throughout.
At 33 writes a second to that table (each of 1000 users writing every 30 s), the server is
permanently saturated, and other requests share that queue. Pages stay correct but fall behind.

So 1000 users is comfortable for a shared loader with paged rows, and is not comfortable for a
personal loader on a busy table without topics of its own. The size of the table does not matter
by itself. What matters is the size of the value a loader returns and how often a run happens.

Nothing here is fixed: no Quillo source was changed, and there are no repros in
`feedback/repros/` or rows in `feedback/INDEX.md`. Fix in this order: Q-189, Q-191, Q-190, Q-195,
Q-192, Q-193, Q-194. Q-196 is about forms, not `live()`. It is small and it can charge a visitor
twice, so it can go first.

## Seen again

| Issue | Where it stands |
|---|---|
| Q-028 (whole value sent, one run per page) | Fixed for shared loaders. It remains for personal loaders (Q-190) and for values that fall out of the 64 MiB (Q-193) |
| Q-084 (fields of a global followed apart) | Holds for which pages refresh. Every waiting thread is still woken (Q-192) |

## What a change costs

One write to a table that 1000 pages follow:

1. `publish_here` raises the sequence number and calls `notify_all` ([shared.rs:239](../../src/shared.rs#L239)).
2. All 1000 waiting threads wake, whichever topics they follow, and take the one `live` mutex in turn.
3. Each page following the topic gets a WebSocket frame and POSTs to `/_quillo/rpc/<loader>`.
4. All 1000 calls go through the event loop. Each parses its arguments, builds the request and hashes
   the viewer.
5. Runs are shared where the rules allow; a loader that cannot share runs each time.
6. A new value goes to JSON, to text and to a hash, on the loop, for each run that happens.
7. A diff against the previous value is made once per pair of versions, under the `live_cache` mutex.
8. The answer goes out. A body of 1 KiB or more is gzipped for that page ([server.rs:2206](../../src/server.rs#L2206)).

What is cheap: a waiting page. Each has a 256 KiB stack reserved, of which little is touched, so
1000 pages reserve 250 MiB of address space and use far less. The limit of 4096 pages is not the
problem at 1000. Step 2 and step 5 are, and the limit of 256 per client below.

What "large" costs, by shape:

| Shape | Cost | Bytes per change, 1000 pages |
|---|---|---|
| A million-row table, a loader with `limit 50` | The query only | Patches; small |
| 10,000 rows returned, ~300 KB (*est.*, at 30 B a row) | Each run costs the JSON and the hash on the loop | ~200 KB of patches for one row changed, against ~300 MB whole |
| 100,000 rows returned, ~3 MB (*est.*) | Each run stalls the instance, and each page load runs it too (Q-193) | The same patches, but a large first page |
| One row with a 1 MB text column, edited | Text is replaced whole, as a patch does not go inside a string | ~1 GB, a 1 MB field to each page |
| A value over 16 MiB | Never kept, so never patched | The whole value to each page, every time |

## New issues

### Q-189. Behind a proxy, only 256 pages get live updates, and the rest go stale with no sign

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading | 0 |

`Waiter::take` allows 256 waiting pages per client ([server.rs:1013](../../src/server.rs#L1013)).
The client is the TCP peer ([httpd.rs:841](../../src/httpd.rs#L841)), because Quillo rightly does
not read `X-Forwarded-For`. Behind nginx, Caddy or any proxy that opens its own connections, every
visitor is one address, so page 257 gets a 503. The same goes for an office or a carrier NAT, at a
smaller scale.

The guide states the limit and the proxy case, but not what it does to a page, and its advice,
"put the proxy's own per-client limits in front", brings no page back. There is no way to raise
the limit or name a trusted proxy.

What the page does ([runtime.js:3879-3893](../../src/client/runtime.js#L3879-L3893)): the socket
fails to open, the `fetch` fallback gets the 503, `.json()` throws on its plain text, and the loop
waits 1 s and tries again. `retry-after: 5` is ignored, nothing reaches `report()`, and `waiting`
does not change. So with 1000 users, 744 pages show data frozen at the first render, and each
makes two requests a second, forever. Nothing tells the visitor or the operator: there is no
metric for it (Q-195), and no log line for a refusal.

**Suggestion:** a flag naming the proxies to trust (`--trust-proxy 10.0.0.0/8`), reading the client
from `X-Forwarded-For` only on connections from those addresses, for this limit and for
`req.ip`. Also a flag for the per-client limit, and a warning in the log the first time it is hit.
On the page, respect `retry-after`, add jitter, and report once that live updates have stopped.

### Q-190. A personal loader runs once per page, for every change, with nothing to slow it down

| Kind | Detected by | Attempts |
|---|---|---|
| perf | reading | 0 |

The sharing rules are documented, and they are right: a run that read the request cannot be given
to another viewer. The gap is what follows. Nothing limits the rate. The topic is the whole table,
so a write wakes all 1000 pages, each re-runs its own query, and most find nothing new for them.
`SHARED_FOR` is a window for pages to share a run and not a floor, because any change to a topic
ends it. There is no way to say "at most twice a second".

The page's rule of one load at a time is what stops this running away. It turns overload into lag,
which is good, but the instance stays busy and other requests wait behind the refreshes.

A trap makes this easy to hit. The guide recommends `unwatched(() => me())`, and that stops the
table being followed, but `unwatched` only touches `reads`
([builtins.rs:1760](../../src/builtins.rs#L1760)). `read_request` stays set
([interp.rs:1161](../../src/interp.rs#L1161)), so the run is still per viewer. Nothing tells the
programmer a loader has gone from 1 run to 1000: `quillo check` and the log are silent.

**Suggestion:** gather changes in the waiting thread: after a wake, wait a short window (say 100 ms,
with jitter) and send one frame for all of it, so a burst of writes makes one refresh. That is a
few lines in `serve_live` and helps every loader. Then an option on the loader for a minimum gap
between refreshes. Have `quillo check` note a `live` loader that reads `session()` from a table it
follows, pointing at the `watch` and `publish` pattern in the guide. Until then, that pattern is
the answer for personal data, and it works today.

### Q-191. A run is not shared if anything was written anywhere while it ran, and identical runs start together

| Kind | Detected by | Attempts |
|---|---|---|
| perf | reading | 0 |

Two gaps, both on PostgreSQL, where a query gives up the lock. SQLite runs a query under the lock
([db.rs:26](../../src/db.rs#L26)), so a run usually finishes before the next call starts, and
neither applies unless the loader gives up the lock some other way.

**First: the check is global.** A run is kept only if `self.shared.live_seq() == seq`
([server.rs:954](../../src/server.rs#L954)). The comment says this is to skip a run that wrote, but
the number is the whole server's. A write to any table during the run counts, and so does any
global assignment by any request, and any change from another server. With a 50 ms loader and 20
writes a second on anything, most runs are not kept (*est.*), and the 1000 pages run it one after
another: one run becomes up to 1000, on the busiest servers, where sharing matters most.

**Second: nothing marks a run as under way.** `cache.shared` is only asked before a run. While the
first call waits on PostgreSQL, the loop takes the next call, which also misses and starts the same
query. Up to `--workers` + 1 run together on each instance (the limit on waiting requests,
[httpd.rs:555](../../src/httpd.rs#L555)), so 65 by default, or 260 with `--instances 4`. Those are
65 copies of the query, at the moment the database is already busy.

**Suggestion:** replace the check with `self.own_seqs.is_empty() && self.shared.unchanged_since(&topics, seq)`,
which is what the rule means: this run wrote nothing, and nothing it read has changed. Then one run
per key at a time: a call that finds its key under way parks, as a request waiting on the database
does, and takes the answer. Do the first change first: it is one line, and it matters more.

### Q-192. Every change wakes every waiting thread, even for pages that do not follow it

| Kind | Detected by | Attempts |
|---|---|---|
| perf | reading | 0 |

`live_changed` is one `Condvar` for the whole server. Each publish wakes all 1000 threads, which
take the `live` mutex in turn and look through their topics, and nearly all go back to sleep.
Pages do not refresh, so this does not show up as traffic, only as CPU.

It is worse than it looks in two ways. Any assignment to a global publishes, with no check that
anything follows it ([interp.rs:1558](../../src/interp.rs#L1558)), so `hits = hits + 1` in a
middleware wakes 1000 threads per request. And the event loop needs that same mutex, for
`live_seq` and `unchanged_since` on every live call, so it waits behind the threads it just woke.
At 100 writes a second that is 100,000 wakes a second, a core or more (*est.*), doing nothing.

**Suggestion:** one thread for all live sockets, holding a map from topic to the pages following
it: a change looks up its topics and writes to those sockets alone. That removes the wakes, and
with them the 1000 threads, and most of the cost of `MAX_WAITERS`. A smaller step: skip the publish
for a global no one has `track`ed.

### Q-193. Large values: a page load shares nothing, patches run out, and each full answer is gzipped per page

| Kind | Detected by | Attempts |
|---|---|---|
| perf | reading | 0 |

Five separate costs, each small alone. All of them grow with the value:

1. **The first render never shares.** `live()` on the server calls the loader directly
   ([builtins.rs:1656](../../src/builtins.rs#L1656)), and `cache.shared` is only asked on a refresh.
   So 1000 arrivals are 1000 full runs, each with its JSON, its text, its hash, and the value
   written into the page's HTML. Refreshes get all of the optimisation and page loads get none.
2. **The 64 MiB runs out for personal values, in the worst way.** 1000 viewers at 100 KB each is
   100 MB, so values are dropped, and a page whose value was dropped gets the whole thing. Pages
   refreshing in the same order is the classic bad case for least-recently-used: nearly every
   value can be gone by the time it is asked for, so patches stop for all of them rather than
   for 36% (*est.*).
3. **The 64 MiB is not 64 MiB.** The budget counts JSON text ([livecache.rs:120](../../src/livecache.rs#L120)),
   but what is kept is parsed `serde_json::Value`, several times larger (*est.*). `patches` keeps up
   to 256 more values and is not counted at all.
4. **A full answer is gzipped once per page.** `compress` keeps a gzipped copy only for files
   ([server.rs:1961](../../src/server.rs#L1961)), so a run shared by 1000 pages is compressed 1000
   times at the default level.
5. **A large patch is written out per page.** `patches` holds the parsed patch, and `p.to_string()`
   runs for each page, under the `live_cache` mutex ([server.rs:966](../../src/server.rs#L966)).
   It is thrown away if it turns out longer than the text. After a bulk update, that is once per
   page for nothing.

Separately, a patch does not go inside a string, and the guide does not say so: one character
changed in a 1 MB text field sends 1 MB to each page.

**Suggestion:** have the first render ask `cache.shared` as a refresh does. Keep each answer's
gzipped bytes beside its text, and keep patches as text, measured once. Count real size in the
budget and include `patches`. For a loader over a few MiB, have `quillo check` or the log say so.
Say in the guide that a loader should return what the page shows, not the table.

### Q-194. After a restart every page reloads within a second, and a page that left holds its place for up to 40 s

| Kind | Detected by | Attempts |
|---|---|---|
| perf | reading | 0 |

After a restart every page reconnects within a second: the wait is a fixed 1 s, no jitter
([runtime.js:3890](../../src/client/runtime.js#L3890)), and a socket that had opened reconnects at
once. The new server has a new `id`, so all topics count as changed and all 1000 pages call their
loaders together. A shared loader takes this well, as the hash gives `$=`. A personal loader makes
it the worst moment of the day, with 1000 runs straight after a start, while the empty value store
means any changed value goes out whole.

A waiting thread does not read its socket, so it learns a page has left only when a write fails
([ws.rs:57](../../src/ws.rs#L57)). That is up to 20 s for the next ping, and the first write after
a close usually succeeds, so up to about 40 s (*est.*). Each reload holds an extra place meanwhile,
which counts against the 256 of Q-189.

**Suggestion:** jitter of a second or so, growing with failures, on the page. A waiter that notices a
close at once comes for free with the single thread of Q-192.

### Q-195. Two things an operator needs at 1000 pages are missing: enough file descriptors, and any sign of live

| Kind | Detected by | Attempts |
|---|---|---|
| hang, missing | reading | 0 |

**File descriptors.** 1000 pages need at least 2000 descriptors: a WebSocket and an HTTP connection
each, in the browser's normal use, and more through a proxy that keeps its own. Quillo does not
raise its soft limit (no `rlimit` in `src/`), which is commonly 1024 on Linux, and the guide's
section on sizing a machine does not mention it. Past the limit `accept` fails, the error is logged
and the loop returns ([httpd.rs:818](../../src/httpd.rs#L818)), to try again on the next connection.
New visitors hang in the backlog while existing ones carry on, which looks like a slow site and
would appear around page 500 (*est.*).

**Metrics.** `/metrics` has nothing about live data: not pages waiting, refused places, loader runs,
shared runs, or patches against whole values. `quillo_channel_pages` and `quillo_prefetches_total`
exist, so the pattern is there. Every issue above is invisible in production without this.

**Suggestion:** raise the soft limit to the hard limit at start, as Go's runtime does, and log the
number if it is low for `MAX_WAITERS`. Add `quillo_live_pages`, `quillo_live_refused_total`,
`quillo_live_runs_total{outcome="ran|shared"}`, `quillo_live_answers_total{kind="same|patch|whole"}`
and `quillo_live_wakes_total`. Add a line to the guide's sizing section.

### Q-196. A form sent twice runs twice, and making it safe is left to the developer

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a browser test | 1 |

`idempotent` protects a call whose answer was lost. It does not protect a form. The page makes a
key for each call ([runtime.js:3765](../../src/client/runtime.js#L3765)), so two clicks are two
calls with two keys, and the server sees two requests. Nothing is refused or logged, and a visitor
can pay twice. The guide reads as if this were covered: "A call that must not happen twice (a
payment, a transfer) is wrapped in `idempotent`" ([USERGUIDE.md:1497](../../USERGUIDE.md#L1497)).

Measured with a handler that takes 300 ms, on SQLite. The `@submit` forms were clicked twice in
headless Chrome, by two `click()`s in a row from a script. The plain forms were sent twice with
`curl`:

| Form | Requests | Work ran |
|---|---|---|
| `@submit` calling a `server fn` that uses `idempotent` | 2 | **2** |
| The same, with `idempotent` in a helper the `server fn` calls | 2 | **2** |
| Plain `<form method="post">` to a route that uses `idempotent` | 2 | **2** |
| The same with `{ required: true }` | 1 | 0, refused with 400 |
| `@submit` with `http.post`, the key in `state` | 2 | 1 |
| The same, with the code below | 1 | 1 |

The helper fails because `used_by` looks only at the function's own body
([idempotency.rs:537](../../src/idempotency.rs#L537)): the bundle has `W.server("pay_helper")`
with no mark, so the page sends no key and never retries. A plain form cannot set a header, so
`{ required: true }` makes a route unusable from one.

The only safe form today is written by hand, around a route rather than a `server fn`:

```quillo
state key = uuid()
state busy = false
state problem = null
<form @submit={() => {
    if busy { return }
    busy = true
    let r = http.post("/payments", { amount }, { "idempotency-key": key })
    busy = false
    if is_err(r) { problem = r.error }                   // keep the key, so trying again is safe
    else if r.ok { key = uuid() }                        // done: the next payment gets a new key
    else if r.status != 409 { problem = `failed (${r.status})`; key = uuid() }
}}>
```

A developer should not have to write this. Each line is a rule of the protocol, and a slip fails
silently: a new key on a lost answer runs the work twice, and a key kept after a finished answer
gets the old answer or a 422. It also gives up what a `server fn` brings: typed arguments and the
page's retries.

**Suggestion:** have Quillo keep the key, so a form is only `<form @submit={() => { pay(amount) }}>`.

1. In `rpcRaw`, tie the key to what is sent and not to the call: a map on the page from the function
   and its arguments to a key. While a call runs, an identical one shares its answer and sends
   nothing, so two clicks are one request, and this covers a `@click` too. A call whose tries ran
   out keeps its key, so sending it again is safe. Any answer removes it, so the next payment has a
   new key. Other values are another entry, so editing the form cannot cause a 422. The one thing
   given up is sending two identical calls at once on purpose, which a function marked `idempotent`
   has already said it does not want.
2. For plain forms, `<form method="post" idempotent>`: the server adds a hidden field with a key, and
   `idempotent` reads it where there is no header, so `{ required: true }` works there too. A cached
   page must not share that key between visitors.
3. Have `used_by` follow the functions a `server fn` calls.
4. Say in the guide what `idempotent` covers, at both places it is explained.

`quillo check` is left out of this on purpose: no warning and no check, as the fix belongs in the
page and the server.

## What to do in an app today

None of this needs a change to Quillo, and it gets a 1000-user app to the good rows of the first table:

1. **Return what the page shows.** `limit`, or `live(load_page, page)`. Rows of a table are free; rows
   in the value are not.
2. **Keep loaders free of the viewer.** Have the loader return shared data, and filter on the page
   where that is safe. `unwatched` does not make a loader shared (Q-190): reading `session()` at all
   makes it per viewer.
3. **Give each personal loader a topic of its own:** `watch("user:7")` and an `unwatched` query, as
   in the guide, published by whoever writes. This is the biggest gain for personal data: one
   user's change refreshes one user's pages, not 1000.
4. **Give every row an `id`.** Without one, a diff has to match rows by content.
5. **Move what changes often to its own table,** as the guide says, and do not write to a global in
   a request, since that wakes every thread (Q-192).
6. **Run `--instances N`.** Each instance has its own loop and cache, which divides the lag.
7. **Before going live:** check `ulimit -n` on the server, and whether the proxy in front keeps the
   client's address. If it does not, Q-189 applies and live data will silently stop at 256 pages.

## Smaller things

- `Live.versions` is never trimmed ([shared.rs:236](../../src/shared.rs#L236)), so a topic per user or
  per thread, which is the recommended pattern, keeps an entry for each for the life of the process.
  The guide's advice leads there. Small per entry, and it grows without end.
- `share` walks every key on each call once there are over 256 keys live
  ([livecache.rs:99](../../src/livecache.rs#L99)), which is normal with a loader taking 1000
  different arguments.

## Where the AI struggled

The first plan was a load test, and it would have hit Q-189 at once: all clients on one machine are
one address, so the test could not have passed 256 pages without a change to the limit or a set
of source addresses. Anyone load-testing this has to work around that first, or the result reads
as a capacity limit when it is a policy.

The deploy storm looked serious until `patch` turned out to check `from == to` before looking the
value up, which is why a restart with unchanged data stays cheap. Reading alone can overstate
things like this, so the *est.* figures deserve doubt.

## Limitations

- **Nothing about `live()` was measured.** The issues are real in the code, but their sizes are not
  known. No capacity figure should be quoted from this report.
- Q-196 was run on SQLite only, with clicks from a script rather than a mouse. On PostgreSQL and
  MySQL the guide says a second request gets 409 where SQLite waits; that was not run.
- Q-192's cost in particular depends on the OS and is a guess; it could be half or twice that.
- Q-191's limit of 65 concurrent runs comes from the limit on waiting requests. Whether PostgreSQL's
  pipelined connections are held to it exactly was not traced.
- MariaDB and `db.connect` were not traced, only SQLite and PostgreSQL. `--register` was read but
  not followed through limer. Browser cost (applying a patch, rendering a large list) was not
  looked at, and Linux was not considered beyond the descriptor limit.
- To measure this, a test needs: 1000 WebSockets from many source addresses, on a separate machine;
  shared, per-viewer and per-argument loaders; 50, 10,000 and 100,000 rows; 1, 10 and 100 writes a
  second; SQLite and PostgreSQL; with and without `--instances`. Report the time from a write to the
  last page updated, along with CPU and memory.

## Edge cases checked that behave well

From reading, not from running:

| Case | Result |
|---|---|
| A page with a refresh in flight is told of another change | It waits, and asks once more only if the answer is older. No pile-up |
| Two tabs of one viewer | They share a run, even when the run read the session |
| A restart with unchanged data | The hash matches and the page gets `$=`, without the value being stored |
| A change arrives while a loader runs | `seq` is taken before the run, so the page hears of the change and asks again. No update is lost |
| A hash the server no longer has | The whole value, not an error |
| One client trying to take every place | Stopped at 256; other clients are unaffected. This is the same limit as Q-189, working as meant |
| Many writes during one refresh | The page gets the latest value, not each one |
