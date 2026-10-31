# How Quillo can run a saga across its services, and what stands in the way

| | |
|---|---|
| App | None: one question, how Quillo can implement the saga pattern in its microservices, answered with reading, small programs and one working saga of four services (156 lines) |
| Quillo | 0.6.0 (da6c1a3, 2026-10-31), branch `feedback22-fixes`, with that branch's uncommitted prefetching work, which landed and was rebuilt while these tests ran. It does not touch `src/service.rs`, `.later`, jobs, service calls or transactions, and line numbers here were checked after it |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, `curl`, `kill -9`, as servers of their own, under `--split` and in one process, on SQLite, PostgreSQL 17.11 and MariaDB 12.3.3. Not run: Linux, `--scale`, `db.system`, several orchestrators on one database, the saga itself on PostgreSQL or MariaDB, or anything under real traffic |

What worked well: a saga needs no new feature to work today. Four services and 82 lines of
orchestrator came through a refused payment, a refused delivery, a step run twice, a `kill -9` of
the orchestrator and a service down for over 20 s, and ended with the stock and the money right every
time. The same files ran in one process, under `--split` and as separate servers, without a line
changing. Calls that are checked against the service, a job that runs once per time across
servers, and a transaction on each side do most of the work.

The answer to the question: **run a saga today as an orchestrator: a table of sagas, a scheduled
job that moves each one a step at a time, and ordinary service calls.** Do not build it on
`service.later` yet. `.later` is the obvious tool, and it is a real transactional outbox on SQLite
and PostgreSQL, but it has six problems a saga would meet (Q-182 to Q-187). Five of them lose, strand
or invent work with no error. Q-182 matters beyond sagas: on MySQL and MariaDB, one `.later` call
inside `db.transaction` turns the rollback off for that transaction.

Nothing here is fixed: no Quillo source was changed. Fix in this order: Q-182, Q-183, Q-184,
Q-186, Q-185, Q-187, Q-188, and only then a `saga` builtin. The repros below are in this report
only: they are not yet in `feedback/repros/`, and `feedback/INDEX.md` has no rows for them.

## Seen again

None. `.later` appears in `INDEX.md` only as Q-043, its log noise, which is fixed.

## How a saga works

A saga is a business operation spread over services that share no transaction. Each service
commits its own part, and if a later part cannot happen, the earlier parts are undone by
compensating steps the programmer writes. An order reserves stock, charges a card and books a
parcel; if the parcel cannot be booked, the charge is refunded and the stock released.

The guide says this much at "Each service's transactions are its own": give each step a reference
and keep what was done. That is correct and it is all there is. It does not say how to undo.

| Style | How it runs | In Quillo today |
|---|---|---|
| Orchestration | One service holds the saga's state and tells each service what to do | Works, in one process and split. **Use this** |
| Choreography | Each service reacts to the last one's event | Services may call each other: `.later` both ways reached `paid` under `--split`, but stalled in one process (Q-183). There is no event bus, so each service names the next |

What a saga needs, and what Quillo has:

| Need | Quillo | State |
|---|---|---|
| Know where a saga is, across a crash | A table in the orchestrator's database | Works |
| Something that moves it on | `schedule("1s", ...)`, once per time across servers | Works |
| Call a step, and tell "no" from "no answer" | Service calls with timeouts, retries and a circuit | Works, by convention (below) |
| Run a step twice safely | A reference the caller makes up, as a key | Works, by hand. `idempotent` does not help here (Q-188) |
| Save state and send a message together | `.later` in `db.transaction` | Depends on the database (below) |
| Be told how a step ended | Nothing for `.later`: the result is logged | Missing (Q-184, Q-188) |
| Compensate | Written by hand | By design |

`.later` in a transaction that rolls back:

| Database | Call rolled back too? | Measured |
|---|---|---|
| SQLite, one database | Yes | `orders=1 later=0 sent=1` before and after |
| PostgreSQL | Yes | No orders, the call not delivered |
| MySQL, MariaDB | **No, and the transaction is lost** | Both inserts kept, the call delivered (Q-182) |
| A database per tenant | **No** | No orders, the call delivered (Q-185) |

## The saga that worked

`orders.qwe` is the whole orchestrator. `advance_all` runs each second, moves each saga one step
and writes where it got to before anything else happens, so a server that stops goes on from
there.

```quillo
// orders.qwe
db.open("orders.db")
db.exec("create table if not exists sagas (id text primary key, item text not null, cents int not null, address text not null, state text not null, step int not null, tries int not null, next_at bigint not null, why text)")

service inventory = "inventory.qwe" { timeout: "1s", retries: 1 }
service payments = "payments.qwe" { timeout: "1s", retries: 1 }
service shipping = "shipping.qwe" { timeout: "1s", retries: 1 }

let steps = 3
let give_up_after = 3

fn forward(s) = match s.step {
    0 => inventory.reserve(s.id, s.item)
    1 => payments.charge(s.id, s.cents)
    _ => shipping.book(s.id, s.address)
}

fn undo(s) = match s.step {
    0 => inventory.release(s.id)
    1 => payments.refund(s.id)
    _ => shipping.cancel(s.id)
}

fn wait(tries) = now() + min([1000 * pow(2, tries), 60000])

// Each move is written before the next call, so a server that stops goes on from here.
fn move(s, state, step, why) =
    db.exec("update sagas set state = ?, step = ?, tries = 0, next_at = 0, why = ? where id = ? and state = ? and step = ?", state, step, why, s.id, s.state, s.step)

fn try_again(s, why) =
    db.exec("update sagas set tries = ?, next_at = ?, why = ? where id = ?", s.tries + 1, wait(s.tries + 1), why, s.id)

fn advance(s) {
    if s.state == "running" {
        let r = forward(s)
        if is_err(r) {
            // No answer: it may have run. Try again, then undo this step too.
            if s.tries + 1 >= give_up_after {
                move(s, "undoing", s.step, str(r))
            } else {
                try_again(s, str(r))
            }
        } else if !r.ok {
            move(s, "undoing", s.step - 1, r.why)
        } else if s.step + 1 == steps {
            move(s, "done", s.step, null)
        } else {
            move(s, "running", s.step + 1, null)
        }
    } else if s.step < 0 {
        move(s, "undone", s.step, s.why)
    } else {
        let r = undo(s)
        if is_err(r) {
            // An undo is never given up on: it waits for the service.
            try_again(s, str(r))
        } else {
            move(s, "undoing", s.step - 1, s.why)
        }
    }
}

fn advance_all() {
    let due = db.query("select * from sagas where state in ('running', 'undoing') and next_at <= ? order by id limit 100", now())
    for s in due {
        advance(s)
    }
}

schedule("1s", advance_all)

route GET /buy/:id/:item/:cents/:address {
    db.exec("insert into sagas (id, item, cents, address, state, step, tries, next_at) values (?, ?, ?, ?, 'running', 0, 0, 0) on conflict (id) do nothing", id, item, int(cents), address)
    "taken"
}

route GET /saga/:id {
    let s = db.one("select state, step, tries, why from sagas where id = ?", id)
    `${s.state} step=${s.step} tries=${s.tries} why=${s.why}`
}

route GET /stock = `book=${inventory.left("book")}`
route GET /charge/:id = `charge=${payments.state_of(id)}`
```

A route that writes on `GET` is only there to drive the tests. `payments.qwe` shows the shape of
every service in it; `inventory.qwe` and `shipping.qwe` are the same (the `sleep` is the test of
a charge slower than the caller waits):

```quillo
// payments.qwe
db.open("payments.db")
db.exec("create table if not exists charges (ref text primary key, cents int not null, state text not null)")

server fn charge(ref: str, cents: int) -> map {
    // Slower than the caller waits, to show a charge that finishes after its refund.
    if cents == 777 {
        sleep(15000)
    }
    db.transaction(() => {
        let was = db.one("select state from charges where ref = ?", ref)
        if was != null {
            return { ok: was.state == "charged", why: was.state }
        }
        if cents > 100000 {
            return { ok: false, why: "declined" }
        }
        db.exec("insert into charges (ref, cents, state) values (?, ?, 'charged')", ref, cents)
        { ok: true }
    })
}

server fn refund(ref: str) -> map {
    db.exec("insert into charges (ref, cents, state) values (?, 0, 'refunded') on conflict (ref) do update set state = 'refunded'", ref)
    { ok: true }
}

server fn state_of(ref: str) = db.one("select state from charges where ref = ?", ref)?.state
```

Five rules carry it, and each one was needed by a test below:

1. **The reference is the key.** The caller makes it up, and a step keyed on it changes nothing
   the second time. Quillo's call id does not do this job: each try from the job is a new call
   with a new id.
2. **A "no" is an answer, not an error.** A step gives `{ ok: false, why }`. An error value means
   no answer came and the step may have run, which is a different decision. If a step uses
   `err` for its "no", both are error values and the caller has to look inside to tell them
   apart.
3. **An undo works on what never happened, and leaves a mark.** A refund with no charge writes
   `refunded`, so a charge that arrives late is refused. Service calls that time out keep running
   there ("What a service is already doing runs to its end"), so this order really happens.
4. **A step with no answer is undone too.** It may have run, and rule 3 makes that safe.
5. **An undo never gives up.** It waits for the service. If it cannot ever succeed, a person has
   to look, so `undoing` for a long time is what to alert on.

Two more belong in a real one, and this example leaves them out. Put the step that cannot be
undone last. And sagas do not isolate each other: between steps, other requests see the stock
reserved and the charge taken, so a state such as `held` must mean something to them.

### What happened to it

As separate servers, one signed process each:

| Case | Result |
|---|---|
| All steps succeed | `done`, within 5 s; stock 3 to 2, `charged` |
| Payment declined | `undone`, `why=declined`; stock back to 2, no charge |
| Delivery refused | `undone`; `refunded`, stock back to 2 |
| The whole saga run again from step 0, as if the moves were lost | `done`; stock unchanged, one charge |
| Orchestrator `kill -9` at step 1, then restarted | `done`; one charge |
| Payments down: 3 tries, then undoing, refund waits | Still `undoing` at 22 s; back up, `refunded`, then `undone` |
| Charge slower than the caller waits (15 s) | `undone` by 31 s: `refunded`, stock back to 3 |
| Refund first, then the charge, by hand | `charge={ ok: false, why: "refunded" }`, and the same again |

In one process and under `--split`, the first and third cases gave the same answers.

The slow charge shows two costs. Its timeouts opened the circuit to `payments`, so the refund
failed with `unavailable (every address failed lately)` and got through only at about 28 s,
right but late. And each of the job's three tries was a new call, so three charges ran for
15 s each at the service: only the reference made that one charge.

## New issues

### Q-182. On MySQL and MariaDB, `.later` inside `db.transaction` ends the transaction, and the rollback undoes nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading the runtime, then a test on MariaDB 12.3.3 | 2 |

`later_put` runs `create table if not exists _quillo_later` before every insert (`later_table`,
`src/db.rs:1353`). MySQL and MariaDB commit the open transaction before `create table`, and the
table being there already does not stop it. Everything before the call is committed, everything
after it commits on its own, and the transaction's rollback finds nothing to undo. Nothing is
logged, and the transaction returns the error as usual.

```quillo
db.open("mysql://root@127.0.0.1:53306/ctl") | unwrap
db.exec("create table if not exists orders (id bigint primary key)")
service mailer = "mailer.qwe"      // server fn send(who: str) = who

route GET /with-later {
    let r = db.transaction(() => {
        db.exec("insert into orders (id) values (?)", 2)
        mailer.later.send("x")
        err("no")
    })
    `with later: ${len(db.query("select id from orders"))} rows`
}
```

| Inside a transaction that fails | Rows left |
|---|---|
| Only the insert (control) | 0 |
| Insert, then the same `create table if not exists`, in the program | 1 |
| Insert, then `.later` | 1 |
| Insert, `.later`, insert | 2, and the call delivered |

PostgreSQL and SQLite are not affected. This costs more than a saga: a payment rolled back on
MariaDB stays paid because of a notification sent inside it.

**Suggestion:** make the table once, outside any transaction, when a program with services
starts, and never in `later_put`. A test on `db_mariadb.rs` should roll back a transaction with
`.later` in it. Any other table of Quillo's made on demand inside a program's transaction should
get the same look.

### Q-183. In one process, a `.later` made by a service is kept and never delivered

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test of services calling each other | 3 |

The call is kept by the program that makes it, in that program's database (`later_put`). Only the
app's own interpreter delivers (`src/server.rs:767`), and it reads only the app's tables. So a
call kept anywhere else stays there. The result is a good id and no error, the row stays at
`tries 0`, and nothing is logged. The same files under `--split` work, against the guide's "a
program behaves the same whichever way it runs".

```quillo
// main.qwe
db.open("main.db")
service svc = "svc.qwe"
route GET /go = svc.go()
route GET /seen = svc.seen()

// svc.qwe
db.open("svc.db")
service inner = "inner.qwe"
server fn go() = inner.later.hit("x")
server fn seen() = inner.hits()

// inner.qwe
let seen = []
server fn hit(x: str) {
    seen = [...seen, x]
    "ok"
}
server fn hits() = seen
```

| App | Service making the call | One process, after 8 s |
|---|---|---|
| Has a database | Has a database | Not delivered, row in `svc.db` |
| No database | Has a database | Not delivered, row in `svc.db` |
| Has a database | No database | Not delivered, row in `quillo-system.db` |
| No database | No database | Delivered: they share `quillo-system.db` |

The row was still there 30 s after a restart, with no line in either log. Under `--split` it was
delivered once. A call left by a one-process run was delivered as soon as the same folder ran
split, so the work is stranded, not lost.

`app/bank/services/payments/payments.qwe` has this shape: a service with its own database calling
`messages.later`, in an app that can run in one process. Its databases here have no
`_quillo_later` table, so this was not seen happening there, and the bank was not run.

**Suggestion:** deliver for each program in the process, as a service's jobs already run with the
app's as `<service>~<name>`.

### Q-184. A `.later` call is deleted on any answer, so a bad deploy loses it with one log line

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading `make_later`, then a test of each answer | 1 |

The guide says a call is tried "until it gets through". `make_later` counts any answer as through
(`src/service.rs:1298`), and `deliver` then deletes the row. Only no answer at all, or 502 to 504,
is tried again.

| What the service answered | `_quillo_later` afterwards |
|---|---|
| Down, then back | Kept, delivered within 15 s of its return |
| 401: its `QUILLO_SERVICE_SECRET` differs | Deleted |
| 404: the function was renamed | Deleted |
| 409: its contract differs, as in a rolling deploy | Deleted |
| 500: a runtime error in the step | Deleted |
| An error value the step returned | Deleted |

Each leaves one line, `quillo: mailer.send (made later, ...) gave an error: ...`, cut short and
at no special level. The last case is right as delivery and wrong for a saga, since nobody learns
the step said no. The first four are the failures a retry exists for: the fix for a bad deploy
is another deploy, by which time the calls are gone.

```sh
# the caller's mailer.qwe has send(who: str); the deployed one has send(who: str, urgent: bool)
quillo run mailer.qwe --port 4742 --as-service --service-name mailer   # the new version
quillo run front.qwe --port 4741 --service-url mailer=http://127.0.0.1:4742
curl localhost:4741/send/ana                                           # an id, then the row is gone
```

**Suggestion:** only a 200 finishes a call. Keep trying on 401, 404, 409 and 500, and past a limit
(a day, say) keep the row marked as failed with its `error`, not deleted, and count those in
`--metrics`. A 400 cannot succeed and can be marked at once. Say in the guide what a call does
when the service answers with an error.

### Q-185. With a database per tenant, a call made in a transaction that rolls back is still delivered

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test | 2 |

With one database per tenant, `_quillo_later` is in the system database, on another connection,
so it commits at once. The order is rolled back and the payment goes ahead anyway. The guide says
`_quillo_idempotency` commits with the work as "the exception", which implies this, but nothing
warns the person writing `.later` inside a transaction. `db.system` takes the same path in the
code and was not run.

```quillo
db.open("data/{tenant}.db")
service mailer = "mailer.qwe"

route GET /rollback/:who = with_tenant("acme", () => {
    db.exec("create table if not exists orders (id integer primary key, who text)")
    let r = db.transaction(() => {
        db.exec("insert into orders (who) values (?)", who)
        mailer.later.send(who)
        err("change of mind")
    })
    `tx: ${r}`
})
```

Result: `orders=0 sent=["bob"]`.

**Suggestion:** keep `_quillo_later` in the tenant's database, as `_quillo_idempotency` is, so it
commits with the work. Delivery then has to find tenants with calls waiting, which a small list
in the system database can tell it. Until then, say so in the guide and have `quillo check` warn
on `.later` inside `db.transaction` where there are tenants.

### Q-186. A service that dies during a call blocks that call, then runs it twice

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading `call_claim`, then `kill -9` during a step | 1 |

The call's row in `_quillo_calls` is written before the function runs and finished after it, in
transactions of their own. If the process dies between them, the row stays at status 0. After a
restart every try of that call gets `503 this call is still running`, however old the row is:
`call_claim` (`src/db.rs:1464`) never looks at its age. Only a sweep clears it, on 1 call in 256 to
that service. A service with nothing else to do, tried every 5 minutes, waits about 21 hours on
average.

Then the call runs again, though its first run may have committed. The guide's "the function runs
once however often the call is sent" holds while the service stays up.

```quillo
server fn post(who: str) -> str {
    db.exec("insert into entries (who) values (?)", who)
    sleep(4000)                  // kill -9 here
    "posted"
}
```

| Moment | `entries` | Call |
|---|---|---|
| Killed, then restarted | 1 | `503 still running`, again in 2, 4, 8 s |
| Row aged past 11 minutes by hand | 1 | Still 503 |
| The sweep's delete, done by hand | **2** | Ran again |

The aging and the sweep were done by hand, to avoid waiting; the rest is the running code. A step
in a `db.transaction` that dies rolls back and is safe to run again; one that committed is not.
The saga above survives this, by its reference, but only if the step has one.

**Suggestion:** treat a running claim older than 11 minutes as free, in `call_claim`, so a stuck
call ends. Change the guide to say a call runs at least once across a crash, so a step keeps its
own reference. Q-188 would close the gap properly.

### Q-187. `.later` delivers at most 50 calls a second per server

| Kind | Detected by | Attempts |
|---|---|---|
| perf | a test | 1 |

`deliver` takes 50 due calls (`src/service.rs:1273`) and waits for the next second, however quiet
the server. 300 calls took 6.4 s, in steps of 50:

```
t=0.0 left=300   t=1.1 left=250   t=2.2 left=200   t=3.2 left=150   t=4.3 left=100   t=5.4 left=50   t=6.4 left=0
```

It adds latency too: a choreographed saga waits up to a second per hop. A queue that grows past 50 a second never drains. The job above has the same one-second
pace, per saga step, and takes 100 sagas per tick.

**Suggestion:** after a full batch, ask again at once rather than waiting for the second.

### Q-188. A step cannot see its own call, and `.later` gives nothing back

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | tests | 1 |

Three gaps with one cause: the service call's identity stops at the door.

- `request` in a step is `{ id: "df3172b83ee40ffd" }`, the id of the whole request, the same for
  every step. The call id is not there, so a step cannot key on it.
- `idempotent` is what would make a step safe, committing the record with the work, which is what
  Q-186 needs. Inside a service it has no `Idempotency-Key`, so it runs the function unguarded
  and no `_quillo_idempotency` table is made. With `{ required: true }` the call fails, as
  `` service ledger: 500 `strict` replied with a page, not a value ``, which hides the reason.
- `.later` gives an id that nothing can be asked about. There is no status, no result and no
  callback.

**Suggestion:** give a step `request.call`, the same across retries of one call, and let
`idempotent` use it inside a service, so a step is one word: `server fn charge(...) =
idempotent(() => ...)`. Add `later.status(id)`, or an option naming a `server fn` that gets the
result.

## What to build, after the fixes

A `saga` builtin, not a keyword, in the manner of `schedule` and `idempotent`. This is a sketch,
and none of it exists:

```quillo
saga("order", [
    { run: (s) => inventory.reserve(s.id, s.item), undo: (s) => inventory.release(s.id) },
    { run: (s) => payments.charge(s.id, s.cents), undo: (s) => payments.refund(s.id) },
    { run: (s) => shipping.book(s.id, s.address), undo: (s) => shipping.cancel(s.id) },
])

route POST /orders { json: Order } = saga.start("order", order.id, order)
```

It would be the orchestrator above: `_quillo_sagas` next to Quillo's other tables, moved on by
the thread that already delivers `.later`, with the backoff, `saga.status(id)`, metrics for
sagas stuck undoing, and `quillo check` asking that a step give `{ ok }` and have an `undo`.

**Steps as data, not a long function.** The other design is durable execution: a function with
ordinary calls in it, which survives a restart by replaying. A request in Quillo is a coroutine
that dies with its process, so that would need every call recorded and replayed in the same
order, and code changed mid-saga handled. A list of steps saved as a number needs none of it, and
82 lines above show it is small. Add the replaying kind only if real programs outgrow the list.

The guide should get a section on sagas whether or not this is built, with the five rules above.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `fail("...")` for an error | Other languages | Unknown name; `check` suggested `file`, `ceil`, `all`, never `err` | 1 run |
| `"tx: {r}"` | Braces work in markup | Printed `{r}` as it stands: only backticks interpolate, and nothing warned | 1 run |
| `"orders={len(db.query("select ..."))}"` | The same | Parse error at `select` | 1 run |
| `min(a, b)` | Python and JavaScript | `quillo check`: takes 1 argument | 1 check |
| `m + 1` on a map literal, to force a 500 | Wanted a runtime error | `check` caught it and the program did not start; used `json.parse` to hide the type | 1 run |
| `db.open(url)` with no `\| unwrap`, as the guide warns against | My test servers had not started: their socket paths were over 103 characters | Ran on SQLite in memory; the only sign was `error has no field n`. Results discarded | 2 runs |
| The slow charge straight after the outage test | Same server | The circuit from the outage was still open, so the test showed nothing. Rerun on fresh servers | 1 run |
| `--split` in a folder that had been run in one process | Reused a folder | It delivered the stranded call, so the restart check looked clean. Rerun in fresh folders | 1 run |
| `.later` as the saga's transport | It is the obvious tool, and the guide says it is delivered until it gets through | Six issues; replaced with a job and ordinary calls | Most of this report |

## Limitations

- **No delay on `.later`:** a call cannot wait. A saga's timeouts and backoff are a column and a
  job, as above.
- **Order is not kept.** `later_due` orders by time, and a call that failed waits, so a later call
  to the same service can overtake it. From reading, not tested. Rule 3 covers it for undos.
- **Sagas and steps need both ends written:** nothing in Quillo checks that an undo exists.
- **A circuit does not know an undo matters:** a step's timeouts delay its own compensation.
- **A job per tenant is by hand:** with a database per tenant, the job has to walk the tenants.
  Not tested.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `.later` in a transaction that rolls back, SQLite and PostgreSQL | Not delivered, in one process and `--split` (SQLite) |
| `.later` in a transaction that commits | Delivered once, within about 2 s |
| Service down when the call is due | Kept, tried with backoff, delivered within 15 s of its return |
| Two services declaring each other | `quillo check` is clean, and it runs under `--split` |
| A service answering `err` to a plain call | `is_err` is true, `type_of` is `error`, the text intact |
| Orchestrator killed and restarted | Its job picks up at its next time |
| The same saga started twice | One row, `done`, one unit of stock taken |

## Sources

- [microservices.io: Saga](https://microservices.io/patterns/data/saga.html)
- [MySQL 8.4: statements that cause an implicit commit](https://dev.mysql.com/doc/refman/8.4/en/implicit-commit.html)
- [MariaDB: statements that cause an implicit commit](https://mariadb.com/kb/en/sql-statements-that-cause-an-implicit-commit/)
