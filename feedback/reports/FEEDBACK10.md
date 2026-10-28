# Feedback from scaling one service without changing its code

| | |
|---|---|
| App | None built: the user's aim for Quillo, "a service-driven architecture that can be a monolith, and at the same time reuse the same code to load-balance a heavily used service, without modifying the code", with "the instance added separately joining the main process to handle more throughput". Tested with an app calling a `risk` service that computes (~19 ms a call), kept with its harness in [tools/service-scale/](../tools/service-scale) |
| Quillo | 0.5.0 (1f86d58); the join was first seen on 9097981; Q-076 and Q-077 on af64ac8 |
| Model | Claude Opus 5.5 |
| Tested with | [tools/service-scale/run.sh](../tools/service-scale/run.sh): 200 calls 16 at a time (50 for the retries), each run recorded in a shared SQLite file with the caller's number, so a call run twice shows; limer.sh 0.1.0 (`~/dev/limer`) for the pool, its `discover` and `/registry` |

What worked well: the aim, as far as it reaches today. The same `app.qwe` and `risk.qwe` ran
four ways without a line changed:

| How `risk` ran | Calls a second | Answered by | Ran twice |
|---|---|---|---|
| in the app's process | 41 | one | 0 |
| one copy elsewhere (`--service-url`) | 49 | one | 0 |
| three copies, listed (`--service-url a,b,c`) | 123 | 67, 66, 67 | 0 |
| two copies behind limer, a third started 2 s into 900 calls | 94 | 331, 331, 238 | – |

Three copies did 2.5 times what one did. A copy started with
`quillo run risk.qwe --as-service --register http://lb:2019` joined limer's pool by itself
and took calls 47 ms after it started; stopped with SIGTERM, it said
``quillo: left `risk` at http://127.0.0.1:19290`` and was out of the pool within a second.
With the addresses listed, a call retried after its timeout went to the same copy, which was
still running it, and was answered once: of 50 calls, every 5th slower than its timeout, none
failed and none ran twice. The calls carry what a balancer needs: `x-quillo-call` (the call's
id) and `x-quillo-deadline`.

What it cost: joining needs a balancer between the app and the copies, and then two things
the app does for itself stop working: a retry goes to another copy, which runs the call again
(Q-073), and the app sees the pool as one address, so one bad member opens the circuit for
all (Q-074). And `--split` starts one copy of each service, never more (Q-075). Asked after,
whether services have health and documentation, also through a gateway: health is each
process's own, so an app answers 200 with its services down (Q-076), and nothing describes a
program's API (Q-077).

## Fixed, on branch feedback10-fixes

Each fix is commented in the code with its issue and the branch.

| ID | The fix |
|---|---|
| Q-073 | A service with a database keeps the calls it answered there too (`_quillo_calls`: the call's id, its status and its answer, forgotten after the eleven minutes the memory lasts): a call another copy answered is answered from there, one it is running is said to be (503, "still running", which the caller asks again). [run.sh](../tools/service-scale/run.sh) step 4, limer `round_robin`, every 5th call past its timeout: 0 failed, 50 runs of 50 calls, none twice (before: 4 failed, 59 runs, 7 twice). Without a database, the memory is the process's, as before (src/db.rs `call_claim`, src/service.rs `answer`) |
| Q-074 | `quillo run risk.qwe --as-service --join http://app:8080`: the copy joins the app, signed with `QUILLO_SERVICE_SECRET`, as a heartbeat every 5 s, and leaves on SIGTERM; the app takes its address from the connection and its `--port`, drops one not heard from in 15 s, and calls it with its own circuit. When `risk` runs in the app's process, the app calls itself and the copies in turn, and itself when no copy answers. Measured with the harness's app and `risk`: 52 calls a second alone, 148 with two copies joined (67, 66, 67), 99 after one left (50, 50) (src/service.rs `join_here`, `Joining`) |
| Q-075 | `quillo run app.qwe --split --scale risk=3`: three copies of `risk`, each on a port of its own, called in turn, started again when one ends, sharing its database: 123 calls a second, 66, 67 and 67. `--scale` without `--split`, or for a service the app does not declare, is refused with what to write |
| Q-076 | `GET /_quillo/ready`: 200 or 503, and as JSON the database (`select 1`), each service the program declares (one in its process asked in turn, down its own; one elsewhere asked at each address, the answer kept 2 s; joined copies too), and checks of the program's own, `ready("ledger", () => ...)`. Through the app, `/<prefix>/_quillo/ready` asks a service served under that prefix. Only states are said, not error texts. The bank in one process walks the whole chain (payments, through risk, to accounts); `--split`, each service at its address (src/ready.rs) |
| Q-077 | `quillo docs app.qwe` writes the program's API as OpenAPI 3 from its source (each route's path, query and payload patterns; each `server fn`'s contract in `x-quillo-server-fns`; the routes of services served under a prefix); `quillo run --describe` serves it at `/_quillo/describe`, off unless asked, as the suggestion has it (the repro now says `--describe`). Each call to a service elsewhere carries the function's contract as the caller's copy declares it, and a service deployed apart refuses one that differs, saying both: ``service risk: 409 `assess` here is `assess(n: int, size: int) -> int`; the caller's copy of this service says `assess(n: int) -> int` `` (src/describe.rs) |

Not done: several app servers each told of a copy (the copy joins each, or a pool tells
them); and copies on other machines joining through NAT, where the address an app sees is
not the copy's.

## Seen again

None.

## New issues

### Q-073. Behind a balancer, a retried call runs again on another copy

| Kind | Detected by | Attempts |
|---|---|---|
| silent | every run recorded with the caller's number, behind limer `round_robin` | 2 |

Every 5th call takes 600 ms; the service's `timeout` is 300 ms with 2 retries. The same 50
calls, three copies each time:

| Routed by | Failed | Runs for 50 calls | Calls run more than once |
|---|---|---|---|
| the app, `--service-url a,b,c` | 0 | 50 | 0 |
| limer, `lb_policy round_robin` | 4 (timeout; `503 ... (this call is still running)`) | 59 | 7 (some three times) |
| limer, `lb_policy hash header x-quillo-call` | 0 | 50 | 0 |

A copy remembers the calls it has answered, and waits for one it is still running, so a retry
sent to the same copy runs once (the guide: "the function runs once however often the call is
sent"). That memory is each process's own. Listed, the app sends a retry to the address it
tried; behind a balancer, that address is the balancer's, which sends it to whichever copy
is next: one that has not seen the call starts it again. The function's effects happen twice
(here a row; in the bank, a posting, saved only by its own `ref`), and nothing says so: the
caller sees an answer, or a timeout.

limer can keep a call on one copy (`lb_policy hash header x-quillo-call`), and then it is as
safe as the list. The guide does not say to: it mentions limer only for whole servers, and
"use HTTPS (or a private network) between services so calls cannot be seen and sent to
another of them", about replays, not retries.

No repro in `repros/`: it needs several processes and a balancer. `tools/service-scale/run.sh`
runs it (steps 4 and 5).

**Suggestion:** keep the answered calls where every copy sees them, the database the copies
share (a `_quillo_calls` table beside `_quillo_later`, its rows kept the eleven minutes the
memory lasts): a call another copy has answered is answered from there, one it is running is
waited for. Until then, a section in the guide on services behind a balancer, with
`lb_policy hash header x-quillo-call` for limer (and its like for others: `hash $http_x_quillo_call`
in nginx).

### Q-074. A new copy of a service cannot join the app; behind a balancer, the app's own balancing sees one address

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | adding a copy while the app runs; a pool with dead members | 2 |

A service's addresses are `--service-url` (or `QUILLO_SERVICE_RISK`), given when the app
starts. A copy started later is not called until the app is restarted with it in the list,
unless a balancer stands between them and the copy joins the balancer's pool. That works (the
fourth row of the first table), and then the app has one address for the service, so what it
does per address is done to the whole pool:

- **Circuits.** An address that fails several calls in a row is left alone for a while. With
  three dead members left in limer's pool (by the test script, which stopped limer before
  they could leave), 12 of 40 quick calls failed at once with `service risk: unavailable`:
  the failures of a few copies opened the app's one circuit, for the copies that were fine.
- **Taking turns, and retrying at the same address** (Q-073), are the balancer's to do.

The app already reads the addresses on every call, from a shared map
([src/service.rs:76](../../src/service.rs#L76) `urls_of`), so a copy could join it directly.

No repro in `repros/`: steps 3 and 6 of `tools/service-scale/run.sh` show the difference.

**Suggestion:** `quillo run risk.qwe --as-service --join http://app:8080`: the copy registers
with the app (signed with `QUILLO_SERVICE_SECRET`, as calls are), keeps its place with a
heartbeat and leaves on SIGTERM, as `--register` does with limer; the app adds it to `risk`'s
addresses, with its own circuit, and drops it when the heartbeat stops. With several app
servers, each copy joins each (or the pool tells them, as limer's broadcasts already do).

### Q-075. `--split` starts one copy of each service, never more

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the guide and `src/service.rs` (one `start_service` per service) | – |

`quillo run app.qwe --split` runs each service as a process of its own, one each. A service
that needs three is started by hand three times, and listed. On one machine, `--split` already
picks the ports, makes the secret, starts each again when it dies, and fills in
`--service-url`; it could do so for several.

No repro: there is no option to try.

**Suggestion:** `quillo run app.qwe --split --scale risk=3` (or `service risk = "risk.qwe"
{ copies: 3 }`), the copies sharing the service's database as copies must; their answered
calls shared too (Q-073).

### Q-076. Health is the process's own: an app answers 200 with its services down, and a gateway cannot see theirs

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the app's and each service's `/_quillo/health`, a service killed; the bank's `/health` | 1 |

Every Quillo server answers `GET /_quillo/health`: `200 ok` while it is ready for requests,
`503` while it stops or when its program failed to load. A copy run `--as-service` answers it
too, though it answers no other unsigned request. It says the process is up, and nothing of
what the program needs:

| Asked | Answer |
|---|---|
| the app's `/_quillo/health`, its `risk` up | `ok` [200] |
| the app's `/_quillo/health`, its `risk` killed | `ok` [200], while `/go` fails `service risk: unreachable` |
| `risk`'s own, on its port (`--split`) | `ok` [200] |
| `/risk/_quillo/health` through the app | 404: Quillo's own addresses under a service's prefix are not passed on |
| `/_quillo/metrics` | only with `--metrics` |

A balancer or gateway that checks the app keeps sending it traffic it cannot answer. What
orchestrators call readiness (are the things I need there?), as against liveness (am I
running?), a program writes for itself, and cannot add to Quillo's. The bank did:

```quillo
// For a load balancer: whether the services answer, and the ledger adds up.
route GET /health {
    let problems = accounts.problems()
    let up = { customers: !is_err(customers.how_many()), accounts: !is_err(problems),
               payments: !is_err(payments.held()), messages: !is_err(messages.unread(0)) }
    ...
}
```

It asks each service a real question (a count, a query) on every probe, and it checks four of
the five services the bank declares: `risk`, through which every transfer goes, is not in it.
A list kept by hand falls behind the program.

Behind a gateway it compounds: a service that calls services (payments calls risk, accounts
and messages) is ready only when they are, and the app in front is ready only when all of
them are; nothing walks that chain.

Repro: [Q-076-health-ignores-the-services/](../repros/Q-076-health-ignores-the-services)
(`risk` given at a port where nothing answers: `/_quillo/health` 200, `/_quillo/ready` 404).

**Suggestion:** keep `/_quillo/health` as liveness, and add `/_quillo/ready`: 200 or 503 with,
as JSON, each thing the program needs and its state: its database (a `select 1`), each
service it declares (that service's own `ready`, signed, with a short timeout, so the chain
is walked to its end, each hop once per a second or two, not per probe), and checks the
program adds (`ready("ledger", () => len(accounts.problems() ?? ["down"]) == 0)`). For a
gateway, Quillo's or another's: the app's `ready` names every service under it, so one probe
of the front door says which link is down; and a service's `ready` is reachable through the
app under its prefix (`/risk/_quillo/ready`) for a gateway that routes by path, signed or
allowed by address, as `/_quillo/metrics` is.

### Q-077. Nothing describes a program's API: no OpenAPI for its routes, no contract for its services

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the guide and Quillo's own addresses (`announce channel dev events health later live media metrics quillo udp wt`) | 1 |

`quillo check` knows a program's API in full: each route's method, path and payload
(`route POST /signup { form: { name: str, age: int } }`), and each `server fn`'s name,
parameters, defaults and types, against which it checks every call to a service. None of it
is served. There is no OpenAPI (or Swagger) document for the routes, and no description of a
service's functions for a caller deployed apart from it: two deploys that disagree on
`assess(n: int)` find out at the first call.

A gateway in front of services (an API gateway, or a Quillo app that passes `service /billing`
routes on) has no description to route, document or check by either: Kong, Tyk, AWS API Gateway
and their kin import OpenAPI.

Repro: [Q-077-no-api-description/](../repros/Q-077-no-api-description)
(a route with a typed form and a `server fn`; `/_quillo/describe` 404).

**Suggestion:** `quillo docs app.qwe` (a file) and `GET /_quillo/describe` (off unless asked,
like `--metrics`): OpenAPI 3 for the routes, from their payload patterns; and for each
`server fn`, its contract (name, parameters, types, result, error), with a hash of it. A caller
asks a service for its contract when it starts (or on its first call) and says plainly when
they differ: ``service risk at http://10.0.0.5:8081: `assess` takes (n: int, size: int), this
program calls it with (n: int, size: str)``. For a gateway: the app's document includes the
routes of the services it mounts under their prefixes, so the front door describes all that
is behind it.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| A service that waits (`sleep`) to make it slow | Slow is slow | A wait holds up no other call (64 can wait at once), so one copy would never saturate. A computation instead: `range(300000) \| map(...) \| sum`, 18.7 ms | 1 calibration |
| "Quillo never leaves limer's pool: `leave()` is never called" | `grep "\.leave()"` found only its definition | `main.rs:508` calls it (`m.leave()`, no dot before `leave` in my pattern); SIGTERM leaves within a second. The dead members were my script's: limer stopped at the same moment as the copies, so their leaving had nowhere to go, and limer brought them back from its `registry.json` | 1 test |
| The first round-robin numbers: 14 calls never run, 20 failed | Reported as they came | Three dead members, from the run before, were in the pool. Run again with the pool empty first: 4 failed, 7 run twice. The dead-member run is kept as Q-074's evidence, for what it shows about circuits | 1 run |
| `count(*)` over `group by n`, labelled "runs" | Haste | It counted calls, not runs: "36 runs of 36 calls" for what was 47 runs of 36 | 1 query |
| Deleting the runs file between steps with copies still running | A fresh file | The running copies kept writing to the file deleted; only the new copy's rows were in the new one | 1 run |

## Limitations

- Copies of a service share its data only through its database: SQLite for copies on one
  machine, PostgreSQL across machines. Variables are each copy's own, as with `--instances`.
- The secret (`QUILLO_SERVICE_SECRET`) is set on every machine by hand (Q-071).

## Edge cases checked that behave well

| Case | Result |
|---|---|
| The same `app.qwe` and `risk.qwe` in one process, one copy, three copies, behind limer | no change to either file |
| Three copies listed, 200 calls | 67, 66 and 67: in turn |
| A call slower than its timeout, the copies listed | retried at the same copy, which waits for the run in progress: answered once, none failed |
| `--as-service --register` | joins limer's pool by itself (`discover risk`), and takes calls 47 ms after starting |
| SIGTERM to a registered copy | ``quillo: left `risk` at http://127.0.0.1:19290``, out of the pool within a second |
| Each call's headers | `x-quillo-call`, `x-quillo-deadline`, `x-quillo-caller`, `x-quillo-to`: enough for a balancer to keep a call on one copy |
| limer `hash header x-quillo-call` | as safe as the list: 50 runs for 50 calls |
| `/_quillo/health` on a copy run `--as-service` | `ok` [200]: a balancer can check a copy that answers no other unsigned request |
