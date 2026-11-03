# Feedback from checking the other side of the test coverage: what fails, what never fails in a test, and what the 97.9% cannot see

| | |
|---|---|
| App | None: a review of the test suite's failure side. Covered: the coverage numbers themselves (lines, regions, branches, runtime.js's line rule), every error message in `src/`, and five areas read and probed for failures a test never makes: HTTP and network, databases and background work, services and the cluster, the browser runtime, the interpreter and JIT |
| Quillo | 0.7.0-dev, release build of the runtime-coverage branch's working tree (it reports `a0a0ac0`). Coverage: two full runs of `scripts/coverage.sh`, the one behind README.md's 97.91% (c8b50ae, 2026-11-02 18:17, lcov kept) and one on this working tree (2026-11-03 07:53 to 08:09, started by another session; its region data was exported with `cargo llvm-cov report --json`). No source file changed after 07:50 |
| Model | Claude (Opus 5.5), with five review agents, one per area |
| Tested with | lcov and llvm-cov's region export, read by scripts; `quillo run` with `curl`, raw sockets (Python), headless Chrome (tests/browser/run.js), Node running the real runtime.js, PostgreSQL 17 and MariaDB on this machine (throwaway databases only), SQLite with `pragma max_page_count` to fill the disk |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section. The probe scripts are in this session's scratch folder (listed at the end) and are not kept. FEEDBACK44 used Q-579 to Q-581. Numbers here start at Q-590, to leave room.

## The answer, in numbers

**The error side is tested, more than "happy path only".** Of the 702 lines in `src/` that hold an explicit error message (`fail(...)`, `Err("...")`, `error(...)`), 662 (94%) ran in the tests, and 727 of the 2,089 Rust tests (35%) are named for a failure. The failure areas everyone thinks of are tested hard: HTTP framing, constraint errors, deadlocks and retries, idempotency races, service retries and circuits, int and decimal overflow, dates, recursion, `--max-compute`. There is one gap. The fuzz targets in fuzz/ are not run anywhere (Q-608).

**But the number cannot see most of the failure side.** Line coverage counts a line as run when any part of it ran, so `let x = f()?;` is covered even when `f` never failed:

| Measure (run on a0a0ac0) | Value |
|---|---|
| Lines | 97.68% (1,167 of 50,280 not run; README's 97.91% is c8b50ae) |
| Regions | 96.15% (4,656 of 120,856 not run) |
| Branches | **not measured**: 0 in the report, since llvm-cov needs nightly's `-Zcoverage-options=branch` |
| Regions not run that are on a line counted as run | **2,748 of 4,615 (60%)**: what the 97.68% hides |
| `?` that pass an error up | **2,315; 1,474 (64%) never passed one up in any test** |
| runtime.js lines | 99.85% (4,619 of 4,626), but a line "ran" when its first character did |
| runtime.js lines that hold a branch the rule cannot see | 1,081 (23%): 27 of 48 `catch` bodies are on the `catch` line, all 15 `.catch(` are, and 229 of 273 failure statements share a line with code that runs anyway |

The 2,748 hidden regions, by what they are: 1,445 are a `?`'s failure, 160 are match arms on a line that ran, 119 are `else` and let-else, 106 are `unwrap_or_else`/`map_or` fallbacks, 63 are the right side of `&&`/`||`, and 39 are error values written inline. By file:

| File | `?` never taken | Hidden regions / all not run |
|---|---|---|
| parser.rs | 382 of 492 (78%) | 422 / 505 |
| client.rs | 131 of 184 (71%) | 163 / 192 |
| numeric.rs | 106 of 145 (73%) | 126 / 134 |
| builtins.rs | 103 of 133 (77%) | 191 / 314 |
| interp.rs | 97 of 182 (53%) | 162 / 292 |
| pg.rs | 45 of 92 (49%) | 58 / 72 |
| db.rs | 24 of 62 (39%) | 74 / 132 |
| fetch.rs | 22 of 39 (56%) | — |
| svcauth.rs | 22 of 32 (69%) | — |
| httpd.rs | 8 of 8 (100%) | 76 / 231 |
| embed.rs | 9 of 9 (100%) | — |
| ws.rs | 6 of 8 (75%) | — |

Three more things make the number look better than the suite is:

- **Both runs that produced a published number had failing tests.** c8b50ae's run had 5: `a_claim_taken_and_a_table_that_can_no_longer_be_read_is_a_503`, `a_connection_that_died_is_replaced`, `a_program_cannot_mark_an_answer_as_a_stream_or_a_file_itself` (http_edges.rs:289, a 500 expected, no answer at all), `the_same_key_with_another_request_is_refused` and `with_windows::a_port_already_taken_stops_it_before_the_window`. This morning's run had 5 others: `a_page_that_opens_no_stream_is_let_go`, `a_program_stops_at_its_top_level_too`, `a_refused_prefetch_is_not_logged_as_an_error`, `an_error_line_is_an_object_where_the_log_is_objects`, and the same window test. Each passes alone and in other logs, so they are flaky under the coverage run's load. But `coverage.sh` still writes the numbers, and lines a failed test ran count as tested. Almost every one of them is a failure-path test.
- **41 tests are ignored.** 36 need the public CDNs. The other 5 mark known bugs, and one of those is stale (Q-609).
- **Code for other systems is not counted at all.** 60 `#[cfg(windows/linux/...)]` sites in desktop.rs, httpd.rs, stack.rs and server.rs. CI runs them on Linux and Windows, but coverage is measured on macOS only.

## Summary

| ID | Problem | Kind |
|---|---|---|
| Q-590 | A derived value that throws while the page updates stops every component on the page from updating, with nothing reported | silent |
| Q-591 | SQLite out of space in a transaction: SQLite rolls it all back, Quillo goes on, and the rest autocommits; `idempotent` then replays a success | silent |
| Q-592 | A value nested a few hundred thousand deep crashes the whole server (stack overflow), with no log line | loud, crash |
| Q-593 | A WebSocket continuation frame with length `0xFFFF…FFFF` wraps the size check and panics the channel thread; the page stays counted and is ignored | silent, security |
| Q-594 | A page that joins a channel and stops reading leaks one OS thread per reliable broadcast (11 to 3,868 threads) | perf, security |
| Q-595 | A service call that reached copy 1 and then got a 504 from a proxy is sent to copy 2 and runs twice | silent |
| Q-596 | Under the shared secret, bearer, OAuth or mTLS, a service's answer with no signature is accepted | silent, security |
| Q-597 | JWKS has no negative cache and no stale fallback: every junk token fetches it, a silent IdP holds a worker 10 s, an outage refuses every caller | perf, security |
| Q-598 | A server fn answered 200 with HTML, an empty or a cut-short body gives `null` as a success in the browser | silent |
| Q-599 | `http.get` cuts a body at 64 MiB and says nothing; cut JSON becomes `json: null` | silent |
| Q-600 | A request followed at once by the client's FIN is answered, and then the connection stays open (6 of 12 tries) | loud |
| Q-601 | After a PostgreSQL restart, the first transaction on each idle pooled connection fails with "internal error" | loud |
| Q-602 | A database that cannot be opened at start (PostgreSQL down, a damaged SQLite file) runs the app on memory, answering writes 200 | silent |
| Q-603 | A wait for SQLite's write lock holds the interpreter: an unrelated `/ping` waits 3.3 s | perf |
| Q-604 | Memory a peer can grow without bound: UDP's queue counts packets not bytes (325 MB), H.264 frames with no marker (117 MB) | perf, security |
| Q-605 | Numbers silently wrong at the edges: `stats.histogram` with `inf`, `stats.median` with NaN, `money.add`/`mul` past what `money.of` takes | silent |
| Q-606 | Lists that share parts grow exponentially in `json.stringify` (3.5 GB in 2.9 s), and `--max-compute` cannot stop one builtin | perf |
| Q-607 | The coverage number cannot see failure branches, and is written from runs with failing tests | docs, limitation |
| Q-608 | The fuzz targets are not run anywhere, and none runs programs | limitation |
| Q-609 | An ignored "bug" test passes now, and the ignore messages name lines that moved | docs |

Then [Failure paths no test makes](#failure-paths-no-test-makes): paths that behaved well when probed or read, but that nothing keeps working.

## New issues

### Q-590. A derived value that throws while the page updates stops every component on the page from updating, with nothing reported

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (headless Chrome), then reading runtime.js | 1 |

One component's derived value divides by zero after a click. The error escapes as an uncaught "division by zero". It does not go through `report`, so `__quilloErrors` stays empty. Every other component on the page then stops updating: `Fine`'s button below counts no more clicks.

The cause is in [runtime.js](../../src/client/runtime.js). `flush()` runs `fresh(c)` for each queued effect with no `try` around it (around line 4572). `fresh` brings stale derived values up to date outside the effect's own `try`. A throw there leaves `flushing = true` (line 4583 is never reached), and a new flush is only scheduled `if (!flushing)` (line 4492). An exception in a handler is caught and reported (5423-5427; tested by cover-timers and edges30). An exception while re-rendering is not, and no test throws there.

```quillo
component Broken(n: int) {
    state zero = 0
    state go = false
    let x = go ? 1 / zero : 1
    <div><button id="break" @click={() => { go = true }}>break {x}</button></div>
}
component Fine() {
    state n = 0
    <div><button id="ok" @click={() => { n += 1 }}>ok {n}</button></div>
}
route GET / = <html><body><Broken n={1}/><Fine/></body></html>
```

Click `#break`, then `#ok`: "ok 0" stays.

**Suggestion:** a `try`/`finally` in `flush()` that resets `flushing`, and a catch per effect that reports through `report` and leaves the other effects running, as handlers do. Test: the page above in tests/browser, with the error expected in `__quilloErrors` and `#ok` counting.

### Q-591. SQLite out of space in a transaction: SQLite rolls it all back, Quillo goes on, and the rest autocommits; `idempotent` then replays a success

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probe with `pragma max_page_count`, reading db.rs | 2 |

On SQLITE_FULL (and probably SQLITE_IOERR), SQLite ends the whole transaction itself. In [db.rs](../../src/db.rs) `attempt` (1993-2030), the outermost transaction without options is `savepoint quillo_tx`, and nothing checks `is_autocommit()` after a statement fails. So every later statement in the function commits on its own, and the closing `release quillo_tx` fails with "no such savepoint".

```quillo
db.open("full2.db")
db.exec("create table t (i int, s text)")
db.exec("pragma max_page_count = 12")
let big = repeat("x", 500)
let r = db.transaction(() => {
    for i in range(0, 120) { db.exec("insert into t values (?, ?)", i, big) }
    "done"
})
print("tx", r)
print(db.one("select count(*) as c, min(i) as lo, max(i) as hi from t"))
```

Rows 0 to 80 are gone, rows 81 to 119 are committed, and `r` is "no such savepoint". The same body under `idempotent` (`pragma max_page_count = 16` after a first warm-up call) answered the first request 500. But 31 rows **and** the stored answer were committed, so the retry got `200 "all 120 rows written"` with `idempotent-replayed: true`.

**Suggestion:** after a statement fails inside a transaction, check `is_autocommit()`. If SQLite has ended the transaction, make every later statement of the function an error ("the transaction was ended by the database: …") and never commit the idempotency record. Test: `max_page_count` in tests/sqlite_failures.rs, for `db.transaction` and `idempotent`.

### Q-592. A value nested a few hundred thousand deep crashes the whole server (stack overflow), with no log line

| Kind | Detected by | Attempts |
|---|---|---|
| loud (a crash) | probe | 1 |

```quillo
let x = []
for i in range(0, 400000) {
    x = [x]
}
print("built")
```

The process aborts when the list is dropped: "thread '<unknown>' has overflowed its stack, fatal runtime error: stack overflow, aborting" (exit 134; seen again for this report). In a server, a route doing `x = [x]` 1,000,000 times, or `json.stringify` of a list nested 100,000 deep, kills the server with exit 138 and no log line, dropping every other request. `str(x)` and `==` survive 100,000. Sandboxed scripts (scripts.rs:76) and the Rust plugin bridge (feedback27.rs:359) cap depth, but ordinary programs do not. `json.parse` stops at serde's depth of 128, which no test pins, so a change of parser would bring the crash to request bodies.

**Suggestion:** drop deep values iteratively (a `Drop` that moves children onto a list), and give `json.stringify`, `str` and comparison a depth limit with an error value. Tests: the program above in a server, which must answer the next request; and a JSON body nested 10,000 deep.

### Q-593. A WebSocket continuation frame with length `0xFFFF…FFFF` wraps the size check and panics the channel thread; the page stays counted and is ignored

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | raw-socket probe, reading ws.rs | 1 |

[ws.rs](../../src/ws.rs) line 196: `if len + so_far > self.max as u64`. In a release build (no overflow checks), a 64-bit length of `u64::MAX` after one byte of a message wraps to 0 and passes. Then `vec![0u8; len as usize]` panics with "capacity overflow" in the `quillo-ch-ws` thread. The server lives on, but the page stays in `channel.peers` and everything it sends afterwards is ignored. The RFC's rules for control frames (FIN set, at most 125 bytes) are not enforced either.

Repro (`quillo run main.qwe --no-wt`, with `QUILLO_SECRET` set): a program with `channel.on("chat", ...)`. Connect to `/_quillo/wt?t=<ticket from POST /_quillo/wt-ticket>` as a WebSocket and send `["j","chat"]`. Then send a text frame without FIN holding `x`, followed by a continuation frame with FIN and the 64-bit length `0xFFFFFFFFFFFFFFFF`. `/peers` still says 1; a later `["m","chat","after"]` is not heard.

**Suggestion:** `len.checked_add(so_far)` (or compare `len > max - so_far`), control frame rules, and a test with that frame in tests/wt_edges.rs.

### Q-594. A page that joins a channel and stops reading leaks one OS thread per reliable broadcast

| Kind | Detected by | Attempts |
|---|---|---|
| perf, security | probe (thread count), reading wt.rs | 1 |

Once a page's 4,096-message queue is full, each reliable message to it runs `std::thread::spawn(move || { let _ = tx.blocking_send(Out::Close); })` ([wt.rs](../../src/wt.rs) around line 158). The writer that should empty the queue is stuck in `write_all` on a socket nobody reads, and a WebSocket write has no timeout ([ws.rs](../../src/ws.rs) around line 123). So the threads never end. One TCP connection with a small receive buffer that never reads, and 8,000 broadcasts of 10 KB, took the server from 11 to 3,868 threads. They were still there afterwards, and the page was never let go. wt_edges covers a page that "stops and goes" and one that "falls behind, then reads", not one that stops for good.

**Suggestion:** one "close" flag per peer instead of a thread per message, plus a write timeout on the WebSocket path. Test: the reader that never reads, with the thread count checked.

### Q-595. A service call that reached copy 1 and then got a 504 from a proxy is sent to copy 2 and runs twice

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probe (a proxy in Python in front of copy 1) | 1 |

[service.rs](../../src/service.rs) 1139-1146: a 502 to 504 marks that address as tried and the call goes to the next one. When the 504 comes from a proxy or load balancer *after* the copy behind it ran the call, the call runs again on the next copy. The log said "504 at … trying again", and both copies printed `RAN refund`. Only copies sharing one database are protected (`call_claim`). A retry at the same address is tested to run once (`a_call_with_no_answer_in_time_is_sent_again_and_runs_once`); this case is not.

```quillo
// main.qwe
service billing = "billing.qwe" { retries: 2, timeout: "3s" }
route GET /refund = billing.refund()
// billing.qwe
server fn refund() {
    print("RAN refund")
    { refunded: 500 }
}
```

The probe gave billing two addresses: a proxy that passes the call to copy 1 and then answers 504, and copy 2.

**Suggestion:** retry a 502 to 504 at another copy only for calls that are safe to repeat (no `server fn` side effect known, or `idempotent`). Otherwise give the error, as the guide's retries section implies. Test: the proxy above, in tests/services_edge.rs.

### Q-596. Under the shared secret, bearer, OAuth or mTLS, a service's answer with no signature is accepted

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | probe (an impostor answering for the service), reading service.rs | 1 |

[service.rs](../../src/service.rs) line 1156:

```rust
if good == Some(false) || (good.is_none() && !kid.is_empty() && status == 200) {
```

A missing signature is refused only when the call has a key of its own (`kid`, key mode). Under the shared secret `kid` is empty, so an unsigned `{"ok":{"refunded":0,"by":"an impostor"}}` from whatever answers at the service's address is taken as the result. Only a wrong signature is refused. With bearer, OAuth or mTLS, nothing about the answer is checked. feedback38's `an_answer_is_the_services_and_a_copy_joins_with_its_own_key` tests key mode only.

**Suggestion:** under the shared secret, refuse an answer with no signature too, as Q-459 does for keys. For bearer and OAuth, say in the guide that the answer is trusted as far as the transport is (TLS). Test: the impostor in tests/feedback38.rs, under the shared secret.

### Q-597. JWKS has no negative cache and no stale fallback

| Kind | Detected by | Attempts |
|---|---|---|
| perf, security | probe (a fake IdP that answers 500, or never answers) | 1 |

`accept_calls { jwt: { jwks: ... } }` ([svcauth.rs](../../src/svcauth.rs) around line 570):

- **No negative cache.** With the IdP answering 500, 5 calls made 5 fetches of the JWKS.
- **Each call waits on the IdP.** With the IdP never answering, each call took 10.01 s.
- **Anyone can trigger it.** A caller with a junk RS256 token and a new call id makes the server fetch from the IdP and holds a worker 10 s. This happens before any authentication, and the error text tells that caller the JWKS URL.
- **No stale fallback.** Once the cached keys are 10 minutes old, an IdP outage refuses every JWT caller.

Also never run in a test: the wrong-signature branch (svcauth.rs:507), RS256 and ES256 verification, an unreachable OAuth token endpoint (256), and any mTLS caller.

**Suggestion:**
- Remember a failed fetch for a short while.
- Keep the old keys when a refresh fails.
- Allow one fetch at a time, with a short timeout.
- Do not put the URL in the caller's error.

Tests: an IdP that answers 500, one that never answers, and tokens signed RS256 and ES256 with a wrong signature.

### Q-598. A server fn answered 200 with HTML, an empty or a cut-short body gives `null` as a success in the browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | Node running the real runtime.js against a stub server, reading runtime.js | 1 |

[runtime.js](../../src/client/runtime.js) around line 4139: `try { body = await r.json(); ... } catch (_) {}`, then `if (!r.ok)` handles errors. A 200 whose body does not parse leaves `body` null and gives `value = null` (4145) with no error. A 500 correctly gives `Err(failed (500))`, and a network failure gives `Err(could not be reached)`. Since `fetch` follows redirects, an expired session sent to a login page would very likely look the same (from reading). No browser test answers a server fn with a non-JSON 200.

**Suggestion:** a 2xx that does not parse is `failed("server fn `x` answered something that is not its answer (text/html, 1234 bytes)")`. Test: a proxy route that answers HTML for `/_quillo/rpc/*`.

### Q-599. `http.get` cuts a body at 64 MiB and says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probe (a 70 MiB answer; a gzip body of 100 MB unpacked) | 1 |

[fetch.rs](../../src/fetch.rs) around line 456: `take(MAX_BODY)`. Both answers gave a body of exactly 67,108,864 bytes, with `is_err` false. JSON cut that way is quietly `json: null`. The limit is in the guide (USERGUIDE.md around line 6190), but the answer carries no flag and no test pins it. `http.stream`'s non-2xx body is read the same way, with its error dropped (`let _ =`, around fetch.rs:990).

**Suggestion:** an answer longer than the limit is an error value ("the answer is more than 64 MiB; use http.stream"). Test: in tests/fetch_edges.rs.

### Q-600. A request followed at once by the client's FIN is answered, and then the connection stays open

| Kind | Detected by | Attempts |
|---|---|---|
| loud | raw-socket probe, 12 tries | 1 |

The client sends `GET / HTTP/1.1` with keep-alive and then `shutdown(SHUT_WR)` straight away. In 6 of 12 tries the answer came, but the connection was still open 14.5 s later, presumably until the 60 s idle limit. The read loop in [httpd.rs](../../src/httpd.rs) around 1139-1150 stops on a short read and waits for another event to see the end of input. With edge-triggered polling, a FIN that arrives with the data raises no new event. Clients that read until EOF (`nc -N`, simple health checks) hang. The nearest tests are `a_request_ending_exactly_where_a_read_does_*`; the `ended` path ran once in the whole suite.

**Suggestion:** after a short read, read once more to see the 0, or check `EPOLLRDHUP`/`EV_EOF` on the event. Test: the half-close, repeated 20 times.

### Q-601. After a PostgreSQL restart, the first transaction on each idle pooled connection fails

| Kind | Detected by | Attempts |
|---|---|---|
| loud | probe (throwaway database, server connections ended with `pg_terminate_backend`) | 1 |

`try_checkout` ([pg.rs](../../src/pg.rs) around line 140) hands out an idle connection without checking it, and connection errors are not retried. After a restart or failover, the first transaction, script or job on each pooled connection fails with 57P01, which the program sees as `err("internal error")`. Shared request connections recover; transactions do not. The known `Pool::give_back` race (pg.rs:209: a connection whose close is not yet noticed is kept) is the ignored `a_connection_given_back_as_it_dies_is_not_handed_out_again` (db_postgres.rs:320). `a_connection_that_died_is_replaced` passes only because it polls for 5 s, which is why it failed under the coverage run's load.

**Suggestion:** on a connection error before the first statement of a transaction has done anything, take another connection and start again. Name 57P01 and its kin as `"connection"` in `error_kind`. Test: kill the backends of an idle pool, then run a transaction.

### Q-602. A database that cannot be opened at start runs the app on memory

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probes (PostgreSQL unreachable; a SQLite file of random bytes) | 1 |

`open_backend` ([db.rs](../../src/db.rs) around line 452) falls back to an in-memory SQLite. The server logs one line, starts, and answers writes 200, and the data is gone at the next restart. The guide says so, but no test pins it, and nothing in the answers or `/_quillo/health` shows that the app is not on its database. postgres_failures covers a server that goes away *after* the first request.

**Suggestion:** refuse to start when the program's main database (`db.open` at top level, or `--db`) cannot be opened. Keep the memory fallback only behind an option (`--db-fallback memory`), or report it in readiness. Tests: both probes.

### Q-603. A wait for SQLite's write lock holds the interpreter

| Kind | Detected by | Attempts |
|---|---|---|
| perf | probe (another process holding the write lock) | 1 |

Statements run while holding the interpreter lock ([db.rs](../../src/db.rs) around line 1907). While another process held SQLite's write lock, an unrelated `route GET /ping = "pong"` waited 3.3 s. rusqlite's busy timeout of 5 s is the bound. The lock errors themselves are tested (`a_database_another_process_holds_is_a_conflict`); the stall is not.

**Suggestion:** release the interpreter while SQLite waits on a busy lock, as `sleep` and `http` already do inside a transaction (FEEDBACK44). Or make the busy wait a few short tries with the interpreter released in between.

### Q-604. Memory a peer can grow without bound

| Kind | Detected by | Attempts |
|---|---|---|
| perf, security | probes (resident memory measured) | 1 |

- **UDP's queue limit counts packets, not bytes.** 8,000 packets of 60 KB sent to a slow `udp.listen` handler took the process from 14 MB to 325 MB. About 655 MB per port is possible from any sender, and dropping the oldest packet lets one sender starve the others. After 51 failed tries, a batch is dropped with no log line (udp.rs:281, 301).
- **H.264 assembly ([media.rs](../../src/media.rs) 345-391) grows until a packet sets the marker.** Packets that never set it took the process from 13 MB to 117 MB, with 0 frames out. Media also accepts RTP from any source address (media.rs:506).

**Suggestion:** a byte limit on the UDP queue (per port, and per sender), a size limit on a frame being assembled, and optionally a source filter for `media.rtp`. Tests: both floods, with resident memory checked.

### Q-605. Numbers silently wrong at the edges

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probes | 1 |

```quillo
print(stats.histogram([1, 2, math.inf], 3))   // { counts: [0, 0, 0], edges: [NaN, inf, inf, inf] }
print(stats.median([3, math.nan, 1]))         // 3.0
print(max([1, math.nan]))                     // "`max` cannot compare float with int", which hides the NaN

print(money.of(9223372036854775807, "USD"))   // err("9223372036854775807.00 USD is too large an amount")
let big = money.mul(money.of(1, "USD"), 9223372036854775807)
print(big)                                    // { amount: 9223372036854775807.00m, currency: "USD" }
print(money.add(big, big))                    // { amount: 18446744073709551614.00m, currency: "USD" }
print(money.to_minor(big))                    // only here: "integer overflow"
```

±1e308 in `stats.histogram` does what `inf` does, and explicit edges holding NaN are accepted (`[1, NaN, 3]` gives `[3, 0]`). numpy refuses both. `MIN / -1` gives the float 9223372036854776000 while `MIN * -1` is an overflow error. That is JavaScript's rule, but no test pins it.

**Suggestion:**
- `stats.*`: refuse NaN and inf, as `sort` does, with a message that names them.
- `money.add`/`mul`: the same range check as `money.of`.
- Tests: these lines.

### Q-606. Lists that share parts grow exponentially in `json.stringify`

| Kind | Detected by | Attempts |
|---|---|---|
| perf | probe | 1 |

```quillo
let xs = [1]
for i in range(0, 24) { xs = [xs, xs] }
print(len(json.stringify(xs)))
```

That makes a 100 MB string, uses 3.5 GB of memory, and takes 2.9 s. 30 rounds would need hundreds of GB. `--max-compute` is checked at each loop step, so it cannot stop one builtin that runs that long. Sandboxed scripts have a 1 MB output cap (scripts.rs:66-75); ordinary programs have none. There is no memory limit for the process either: when an allocation fails, the process aborts.

**Suggestion:** a size limit on what `json.stringify`, `str` and the server's JSON writer produce (an error value past it, e.g. 256 MB), and a check of the compute budget inside long builtins.

### Q-607. The coverage number cannot see failure branches, and is written from runs with failing tests

| Kind | Detected by | Attempts |
|---|---|---|
| docs, limitation | reading the reports | — |

This is [The answer, in numbers](#the-answer-in-numbers) above. What the README's "97.91%" means is lines, in which a `?` that never failed counts as tested. 64% of the `?` never did.

**Suggestion:**
- `scripts/coverage.sh`:
  - Add region coverage beside lines in README.md and the /coverage/ page (llvm-cov already reports it).
  - Add branch coverage, from a nightly run with `-Zcoverage-options=branch`.
  - Refuse to write coverage.json when a test failed, or first run each failed test file again on its own.
- `rtcover.js`: count a line as run only when every V8 range that starts on it ran, so a one-line `catch` or `||` fallback that never ran shows as missed.
- A short list in scripts/ of the `?` that never failed in the main modules (httpd.rs, db.rs, pg.rs, fetch.rs, svcauth.rs, embed.rs), to work through.

### Q-608. The fuzz targets are not run anywhere, and none runs programs

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading fuzz/, CI, scripts/ | — |

fuzz/ has `parse`, `check`, `http`, `json`, `decimal` and `decimal_ops` (a6aae07, 2026-10-27). Nothing in .github/workflows or scripts/ runs them. There is no corpus or artifact here newer than that, and 205d7cd removed the dates target but left its hook (src/dates.rs:248). No target runs programs, so the interpreter, builtins, money, regex and the JIT are not fuzzed. Since 382 of parser.rs's 492 `?` never fail in a test, `parse` is exactly what would reach them.

**Suggestion:**
- A nightly CI job running each target for a few minutes, with the corpus cached.
- A target that runs small generated programs under `--max-compute`, comparing the JIT with the interpreter (the JIT's float `/`, `-`, `<=`, `>=` and negation never ran in a test, see below).

### Q-609. An ignored "bug" test passes now, and the ignore messages name lines that moved

| Kind | Detected by | Attempts |
|---|---|---|
| docs | running the ignored test by hand | 1 |

`a_changed_text_inside_a_component_makes_it_again` (tests/runtime_cover_dom.rs:294) is ignored as "a Mismatch in a dynamic text part is reported and emptied, not made again (runtime.js:4492)". Run by hand, it gives exactly the expected `{"clicks":"1","said":"as written","remade":1,"errors":[]}`. So it is fixed, but a regression would go unseen. The other ignore messages name lines that moved:

| Named | Now |
|---|---|
| 5202 | 5248 |
| 3777 and 3820 | 3809/3811 and 3854 |
| 4492 | 4537 |

`navigate()` to another origin still throws from `pushState` (now line 5248).

**Suggestion:** take the `#[ignore]` off that test, and name functions instead of line numbers in the other messages.

## Failure paths no test makes

Each of these behaved well when probed, or was read only. None is a known bug, but nothing keeps it working. Each line is a test to write.

**HTTP and network**

| Path | Where | Note |
|---|---|---|
| Slow head (slowloris): `GET / HTTP/1.1\r\nHo`, then a byte every 20 s | httpd.rs:1028, the 408 after 30 s | that arm ran 0 times |
| A client that reads one byte every 29 s | the stall timer resets on any byte | a buffered answer (up to 64 MB) or a pool thread is held without limit |
| `http.*` with an unknown issuer, a wrong host name, a `ca:` that works | fetch.rs:105-150 | every TLS test points at the closed port 127.0.0.1:1; probed: correct |
| `http.*` DNS failure, GET redirect loop, cut chunked or gzip body, a garbage status line | ureq | probed: each a clear error |
| FastCGI: bad version, two requests on one connection, body over `--max-body`, abort, unknown role, connection limit, web server gone mid-answer | fcgi.rs:85-382 | feedback41 tests the path that works; probed: correct |
| The acceptor with several instances | httpd.rs:244 | the descriptor-exhaustion tests use one |
| `%2F` and `%00` reach route parameters decoded: `/a/..%2F..` gives `../..` | server.rs | `files()` refuses it (probed); `read_file(x)` in a program would not |

**Databases and background work**

| Path | Where | Note |
|---|---|---|
| A statement timeout (`?options=-c statement_timeout=…` passes through) | `error_kind` gives null for 57014 | — |
| A connection killed in the middle of a transaction | db.rs:2033-2105 | probed: rolls back, the next transaction works |
| Two processes migrating at once | migrate.rs:56, 112, 154 | PG: the second waits (correct). SQLite: a migration over 5 s makes the second exit "database is locked". MySQL's `get_lock` result is unchecked (112). `pg_advisory_lock` has no timeout, and unlock errors are dropped (190) |
| A migration's commit failing on PG or MySQL | migrate.rs:135, 177, 178 | ran 0 times; SQLite's is tested |
| Migration state read with `.ok()` | migrate.rs:57-59 | any error reads as "not applied" |
| A Quillo panic inside a PG or SQLite transaction | server.rs:2515 drops the interpreter | the PG connection is dropped without `give_back`, so the pool loses a slot per panic; SQLite is left inside a savepoint (from reading) |
| A job that hangs on the database | jobs.rs:276 | no time limit; its schedule stops after one log line |
| The process killed while a job runs | claimed before it runs (server.rs:718) | at most once, untested |
| A damaged stored idempotent answer | idempotency.rs:503 | replays as an empty body |
| `call_done`'s update failing | db.rs:1872-1876 (`let _ =`) | the claim stays "running": 503 for 11 minutes, then the call runs again |

**Services, cluster, embed**

| Path | Where | Note |
|---|---|---|
| An unknown `kid`; an `accept_calls` check that returns an error value or fails | service.rs:1280, 1330-1335 | 0 runs |
| A service killed in the middle of a call | — | `--split` restarting a dead service is tested; mid-call is not |
| A JWT inside the 60 s leeway | svcauth.rs:515-522 | only tokens far past it are tested |
| A replayed channel relay message | service.rs:949 | no nonce is kept |
| More than 65,536 topics: `forget()` | shared.rs:226 | never ran |
| Cluster clock skew over 60 s | cluster.rs | silent 403s; lost announcements are never replayed (the test asserts they are dropped) |
| Native module in an endless loop, aborting, or cargo hanging | native.rs:1258, 1069 | no deadline; `BUILDING` held at 1047 |
| embed: 90 lines never ran (82.8%) | embed.rs | almost all failure and size paths: relay's 502 and chunked bodies, frames of 126 bytes or more both ways, close/ping/pong, the cookie jar read from disk, stop. SHELL_JS never runs in a test |
| embed: the 4 bugs of the deferred review | embed.rs:855/892 (BRIDGED keyed by ids that restart per page), 66/344 (a never-ending stream pins one of 4 workers), 819-839 (origin taken from the target URL), 791-804 (capture-phase submit before `@submit`) | none is tested; the code is unchanged since c8b50ae |
| embed: `request()` after `stop()` | embed.rs:157 | blocks for ever (from reading) |
| embed: a body cut short | embed.rs:467, 494 | passed on as complete; the jar's write and rename errors are dropped (654-655), and bad base64 becomes an empty body (815, 866) |

**Browser runtime**

| Path | Where | Note |
|---|---|---|
| `live()` data across a server restart: reconnect with `since=`, catch up, spread the refetches | 4224-4253 | only a 503 with retry-after is tested (live_refused.js) |
| A half-open live socket (laptop asleep, NAT) | — | no heartbeat or timeout |
| `listen()` resuming with last-event-id | 1230-1250 | only the server's format is tested (feedback36.rs:300) |
| `listen()` when the stream ends cleanly | 1240 | stops for good, unlike EventSource, so a graceful restart ends it; `retry: 0` reconnects with no wait |
| A slow `live()` answer arriving after a newer one | 4314, the guard | — |
| An idempotent server fn retried on 409/502-504/network | 4127-4137 | the double click is tested (once.js); the retries are not (the wait at 4136 ran in 1 of 791 records) |
| A new build while a page is open | — | a renamed server fn gives "failed (404)"; no build id |
| A channel's queue over 256 messages | 3476 | dropped silently |
| No `AbortSignal.timeout` | 1170 | — |

**Interpreter and JIT**

| Path | Where | Note |
|---|---|---|
| An error inside a compiled `while`; an interpreted `while` whose body has a `let` | interp.rs:2334-2338, 2351-2355 | count 0; probed: correct |
| JIT float `-`, `/`, `<=`, `>=`, negation, a block as a value | jitloop.rs:1027, 1054, 1072, 1074, 904, 965-974 | never generated in a test; probed: matches the interpreter |
| JIT: `/` by 0.0 and -0.0, NaN with `==`/`>=`, -0.0 | — | probed: matches |
| `len(g[y])` and `set1`/`set2` slow and error paths | jitloop.rs:500-560 | never ran |
| The "compiled loop failed where the interpreter would not" safety net | jitloop.rs:577 | never ran |
| A failed `realloc` in `append_text` (unsafe) | value.rs:1273 | — |

## What behaves well

These have failure tests that are strong (exact output, status and headers, and `errors: []` in the browser), not tests that only run the code:

- **HTTP/1.1 framing.** Content-Length against chunked, duplicate or signed lengths, chunk sizes that overflow, endless trailers, pipelining, HTTP/1.0, oversize heads and bodies, gzip bombs, clients that reset or never read, descriptor exhaustion.
- **Databases.**
  - Errors: constraint errors with their kinds on all three databases, deadlocks and serialization conflicts with retries, SQLite locked, a read-only file, migrations rolled back on failure.
  - Connections: pool exhaustion, shared connections stuck or dying.
  - Tenants that do not exist, idempotency races with 8 requests on 3 databases.
- **Services and the cluster.** A service that is down, slow, or answers garbage, 404 or 500. The deadline passed down a chain of calls, the circuit breaker, a retry at the same address running once, `.later` across restarts and two servers, keys rotating, JWT rejections (expired, nbf, issuer, audience, no exp), cluster announcements replayed or badly signed.
- **UDP and media.** UDP's queue overflow and resends. Media's bad RTP, reordering, duplicates, wraparound and lost fragments (media.rs at 100% of lines).
- **The core.**
  - Overflow and number edges: int overflow (also against the JIT), decimals' 7 rounding modes and overflow, money, dates (DST gaps and repeats, years 1 to 9999, unknown and path-like zones, leap seconds).
  - Limits: recursion through calls and `map`, `--max-compute` in loops, compiled loops and `each`, regex size, the 100 MB cap on `repeat`.
  - Panics: every `unwrap` read in the core has a check before it, and 26 probes with multibyte text found no panic.
- **The browser runtime.**
  - Failing code: exceptions in handlers, timers and `@mount`, hydration mismatches.
  - Missing browser features: no WebTransport, `moveBefore`, `randomUUID` or WebCodecs.
  - Connections: channels reconnecting with queued sends, media across a server restart.
  - Cleanup: listeners and timers removed with their component.

The cover-*.js scripts were written to raise coverage, but they compare the whole result exactly. They are not run-only.

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| Matched each error message's first 28 characters against tests/ | quick measure of "asserted" | 37% looked unasserted, svcauth.rs 93%; but feedback38.rs tests every JWT rejection with short fragments ("expired", "no exp") | replaced by the line counts from lcov |
| Counted tests with no `assert` in their body | quick measure of weak tests | 312 found; nearly all call helpers that assert (check.rs's diagnostics, the browser harness) | dropped |
| Counted error sites whose line ran (94%) as "tested" | line counts | inflated by let-else and `.ok_or(…)?` on one line; only 20 sites share a line, so the figure stands | 1 script |
| Planned a coverage run of my own | the profile data of the 97.91% run was gone | another session had just started one in this folder; waited for it and exported its regions instead | none |

## Order of work, and questions

1. **Bugs a peer can trigger:** Q-593 and Q-594 (one connection), Q-596, Q-597, Q-604.
2. **Silent data bugs:** Q-591, Q-595, Q-590, Q-598, Q-602.
3. **Q-592** (one request kills the server).
4. **Everything else, then the tests:** the other issues, then the "Failure paths no test makes" tables. Writing those tests is also what moves the 64% of `?`.
5. **Q-607 and Q-608,** so the number shows the failure side from then on.

Questions:

- **Q-602:** should a program whose database cannot be opened refuse to start? That changes documented behaviour.
- **Q-595:** should a 502-504 be retried at another copy only for `idempotent` calls, or for no call?
- **Q-607:** should README.md state region coverage (96.15%) beside lines, or instead of them? Should a run with a failed test write no number?

## Probe scripts

In /private/tmp/claude-501/-Users-restiguay-dev-quillo-app1/83c0252c-4dff-4fce-b38c-b0978e42215f/scratchpad/ (this session's scratch, not kept):

| Scripts | What they probe |
|---|---|
| regions.py, errsites.py, errmsgs.py, swallow.py, asserts.py | the numbers above; full.json is the region export of the a0a0ac0 run |
| rt/iso/app.qwe, rt/rpc_probe.js, rt/classify.js | Q-590, Q-598, runtime.js's line rule |
| db/full2.qwe, db/idfull.qwe, db/busy.qwe, db/pgsrv.qwe, db/pgdown.qwe, db/corruptsrv.qwe, db/smig/ | Q-591, Q-601 to Q-603, migrations |
| http/ws/probe.py, http/ws/stall.py, http/fetch/, http/fcgi/, http/tls/ | Q-593, Q-594, Q-599, Q-600, FastCGI, TLS |
| svc/probe1/ (proxy.py), svc/probe2/ (idp.py), svc/udp/ | Q-595 to Q-597, Q-604 |
| core/srv/main.qwe, core/*.qwe | Q-592, Q-605, Q-606, the JIT |
| v/deep.qwe | Q-592 |
