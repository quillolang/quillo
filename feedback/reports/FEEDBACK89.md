# Feedback from making "Services", a video on splitting a Quillo app into services

| | |
|---|---|
| App | `~/dev/quillo-videos/videos/222-arch-services/app/`: a shop (`main.qwe`, two routes and a summary) that bills through `billing.qwe` (`service billing = "billing.qwe" { timeout: "2s", retries: 1 }`, two `server fn`s over SQLite). Made for video 222 of the "Quillo system architecture" series (QUEUE 210) by quillo-videos-40: run in one process, `--split`, `--split --scale billing=3`, and on two ports by hand with `--service-url` |
| Quillo | `quillo 0.8.10 (61997ab, 2026-11-22)`, from the release; the repro also on the repo's `target/release/quillo`, `0.8.10 (64df6c0, 2026-11-24)` |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`, `quillo run` (`--split`, `--scale`, `--as-service`, `--service-url`), `quillo docs`, curl, a Node proxy printing each call, Playwright (Chromium), `node feedback/run.mjs Q-1390` |

What worked well: the same code ran in one process and split, with the same pages. A misspelled service function (`billing.invoce`: "service `billing` has no server fn `invoce` (did you mean `invoice`?)") and a wrong argument type ("argument `cents` of `billing.invoice` must be int, got str") are caught by check, and a `fn ... with db` calling the service is refused for the missing http effect. On the wire: an unsigned call is a 401, a call with its body changed a 401, the same call sent again gives the same answer without running twice, and a service's server fn is a 404 at `/_quillo/rpc/` ("a service's server fns are called by other services, not by pages"). The circuit opened after five failures and calls then failed in under a millisecond; with billing hanging past the 2 s timeout, the retry with the same call id did not run the invoice twice. Measured (Apple M5, busy, median of 3): a buy in one process 0.20 ms, split 0.45 ms.

Numbers Q-1390 to Q-1394, from the block Q-1390 to Q-1399. Q-1391 is from the next video, 224 "Bill Once" (`~/dev/quillo-videos/videos/224-idempotent-services`: the same shop with `idempotent` in billing and in the shop's buy route), made the same day.

## New issues

### Q-1390. A field read straight off a call that may give an error value passes `quillo check`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check`, then a request (making a video) | 1 |

A function declared `-> map ! str` (or a service's `server fn`, called as `billing.invoice(...)`) may give an error value. Written with neither `?` nor `??`, its result's fields are read as if it were the map, and check says ok. When the error comes, the route is a 500: `main.qwe:20: error has no field `id`` in the log, `internal error` to the visitor.

```quillo
fn invoice(name: str) -> map ! str = if name == "" { err("An invoice is for someone") } else { { id: 1, name } }
route GET / {
    let inv = invoice("")
    <p>Invoice #{inv.id}</p>     // check: ok; the request: 500
}
```

Seen with a service: the shop's `let inv = billing.invoice(request.form.name, 1200)` with its `?? reply 503 ...` line deleted passes check; a blank name (billing's `err("An invoice is for someone")`) or billing being down gives the visitor `internal error`. Across services this is the common case: any call can fail on the network, so `?`/`??` is never optional there.

| Impact | |
|---|---|
| **Functionality** | The error's own message is lost; the route ends in a 500 |
| **User** | "internal error" instead of the app's answer for a failed call |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** treat reading a field (or calling a method) on a `T ! E` value without `?`, `??`, `is_err` or a `match` as an error, with the fix in the message ("`invoice` may give an error: add `?` to pass it on, or `?? value` to replace it"). At least a warning for service calls, which can always fail.

### Q-1391. With `{ transaction: false }`, a failure the route answered early is kept, so the same form stays "busy" after the service is back

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a crash test (making a video): billing killed mid-call, then the same form sent again | 1 |

`quillo check` asks for `{ transaction: false }` when `idempotent` wraps a call to a service (the warning names it), and the guide says that mode "keeps every answer, failures too". With the usual handling of a failed call, `?? reply 503 Page(<p>Billing is busy...</p>)`, that answer is kept for the key. Once billing is back, the same form (the browser's Back, then Buy again, or the browser resending it) gets the kept 503, `idempotent-replayed: true`, in 0.2 ms, every time until the key expires. Only a new page load (a new key) gets through.

Measured on 61997ab (compare/measure.mjs in the video's folder): billing killed 0.8 s into a buy: `503 Billing is busy` (938 ms); billing started again; the same key: `503 ... (replayed)`, 0.2 ms; a new key: `200 Invoice #1`.

The guide's own example says a form keeps its key so that sending it again after a lost answer is safe; here sending it again can never succeed. In the default mode (with a transaction), an early reply is not kept and a retry runs again, which is what a visitor expects, but that mode is the one check warns against around a service call.

| Impact | |
|---|---|
| **Functionality** | A failure that happened before any work was done (the call was never answered, or got connection refused) is treated as final for the key |
| **User** | "Billing is busy" forever on that form; the visitor has to reload the page, which they have no reason to know |
| **Requests** | None |
| **Security** | None |

**Suggestion:** in `{ transaction: false }`, keep a 5xx the handler replied with early only when the call it replaced may have run (a timeout, no answer), and not when the call was refused before it was sent (connection refused, an open circuit); or add an option such as `keep: "success"` and mention it in check's warning. At least, say in the warning that a kept failure replays until the key expires, so the failure page should offer a fresh form.

### Q-1392. The "idempotent reaches another program" warning stays after the change is keyed as it says

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` on a three-link chain (shop → billing → ledger), answering "can idempotent be chained?" | 1 |

`billing.invoice` runs `idempotent(() => { insert the invoice; ledger.post(ref, cents)?; ... })`. Check warns: `` `idempotent` reaches `ledger.post`, which may change data in another program ... or send an id the other program's change is keyed by, so that a second run changes nothing ``. After doing the second of those (the ledger's `postings.ref` is `unique` and `post` is `insert ... on conflict (ref) do nothing`, then a select), the warning is the same, and there is no way to say so. The fix was verified: billing killed (SIGKILL) after the ledger posted, then a new buy: before the fix 2 postings for `inv-1`, after it 1.

Files: [repros/Q-1392-chain](../repros/Q-1392-chain) (main.qwe, billing.qwe, ledger.qwe; the ledger already keyed by `ref`). `quillo check main.qwe` gives the warning at `billing.qwe:6`.

| Impact | |
|---|---|
| **Functionality** | None at run time |
| **User** | A warning that can't be cleared teaches people to ignore check's warnings, including the useful first one |
| **Requests** | None |
| **Security** | None |

**Suggestion:** let the call say it: a marker on the declaration (`service ledger = "ledger.qwe" { keyed: ["post"] }`) or on `idempotent` (`{ keyed: true }`), or read the service's `server fn` for an `idempotent` or a unique insert and stay quiet then; and say in the warning how to silence it once done.

### Q-1393. A warning in a service file that two programs of the app declare is printed twice by `quillo check`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

`main.qwe` declares `service billing` and `service ledger`; `billing.qwe` declares `service ledger` too. A warning in `ledger.qwe` (`` `count` hides the builtin `count` ``) is printed twice, and counted twice ("3 warnings" for two). The docs say the whole tree is checked once and each warning said once (for `--split`).

```
./billing.qwe:2: warning: `count` hides the builtin `count`: ...
./ledger.qwe:1: warning: `count` hides the builtin `count`: ...
./ledger.qwe:1: warning: `count` hides the builtin `count`: ...
main.qwe: ok (1 route, 0 functions, 0 components), 3 warnings
```

Files: [repros/Q-1393-dup](../repros/Q-1393-dup) (three one-line programs).

| Impact | |
|---|---|
| **Functionality** | None |
| **User** | Noise, and a count that doesn't match the warnings |
| **Requests** | None |
| **Security** | None |

**Suggestion:** check each service file once per run, as `--split` already does.

### Q-1394. `idempotent` doesn't carry down a chain: calls made inside a run get new call ids when it runs again

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a crash test on shop → billing → ledger, each with `idempotent` (answering "can idempotent be chained?") | 1 |

Each link of a chain can use `idempotent`, and a retried call keeps its call id, so each link runs once per call. But a call made inside an `idempotent` run gets a fresh call id each time the run starts. When billing is killed after `ledger.post` committed and before billing's own transaction committed, billing's invoice rolls back, and the next run (a new buy) calls the ledger under a new id. The ledger's `idempotent`, keyed by call id, posts again: measured on 61997ab, invoices 0 / postings 1 after the crash, then invoices 1 / postings 2, both `inv-1`.

Check warns at billing (`idempotent` reaches `ledger.post`, Q-1392), and the workaround works (the ledger keyed by a reference, `unique` plus `on conflict do nothing`: 1 posting). But every service in a chain then has to carry its own reference scheme, and a reference built from the middle link's own data (here `inv-${inv.id}`) only works while the rolled-back id is reused, so the caller's reference has to be threaded through by hand.

Files: [repros/Q-1394-chain](../repros/Q-1394-chain) (main.qwe, billing.qwe, ledger.qwe, README.md with the steps).

| Impact | |
|---|---|
| **Functionality** | A mid-chain crash can apply a downstream change twice, though every link uses `idempotent` |
| **User** | Double postings, charges or emails in a chain the author believed idempotent |
| **Requests** | None |
| **Security** | None |

**Suggestion:** derive the call id of a call made inside an `idempotent` run from that run's key and the call's place in it (for example HMAC(key, service, function, n-th call)), so that running the same key again sends the same call ids down the chain and each link's own `idempotent` answers from its record. A caller's retry after a lost answer would then hit the downstream records too. Document the chain case either way, with the reference pattern.

**Proposed design: chained retries (the user, 2026-11-24).** Retries should chain as keys do: a retry at the top carries
its identity all the way down, every link keeps the answer it gave, and the caller gets one answer, once.

- The top call's id (or the route's `Idempotency-Key`) is the chain's root. Each call a link makes while answering it
  gets an id derived from the root and its place in the run, so a retry of the root sends the same ids down every link.
- A link that already answered a derived id gives its kept answer without running again (the ledger: "posted, id 1"),
  so a link that rolled back (billing, killed mid-run) runs again and finds every downstream step already done, with
  the same answers it got the first time, and commits once.
- Answers and tries are kept per link in its own database (`_quillo_calls`/`_quillo_idempotency`), so it holds across
  a crash at any link, not only while the link stays up.
- Each link retries its own downstream calls (timeouts, 502 to 504) under the same derived id; the caller sees only the
  final answer, or one error, never a half-done chain answered twice.
- `x-quillo-call` already crosses the wire; a `x-quillo-root` (signed, like the rest) would let a service in another
  language take part.
- With this, check's warning (Q-1392) could go quiet for calls to Quillo services, since a second run changes nothing;
  it stays for `http.*` to third parties that don't take a key.
