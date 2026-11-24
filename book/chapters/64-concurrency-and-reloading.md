# Concurrency and reloading

One thread runs an event loop (kqueue on macOS, epoll on Linux) over every connection,
reads and parses requests, runs the program on them and writes the answers. Each request
runs as a coroutine with its own stack. A handler runs on its own from start to end,
except while it waits: a database query, `http.*` or `sleep` pauses it, the waiting is
done elsewhere (on a pool thread, or for PostgreSQL on connections all requests share,
with queries pipelined, and a `sleep` of 5 ms or more on a timer, holding no thread),
and the loop answers other requests meanwhile. Code is written
as if it simply waited; there is no `async` or `await`. Updating shared state such as
`todos = todos | push(t)` is safe without locks, since only one handler runs at a time.

A request that only computes never leaves the loop's thread, so plain pages and JSON
cost a few microseconds each. To use more cores, start the program with
[`--instances N`](#options-of-quillo-run), `quillo run main.qwe --instances 4`, which runs the program N times
over in one process, each with an event loop of its own; the first takes the connections
and hands them out in turn. Like N processes, each instance has its own variables, while
the database is shared, and changes to it (and `publish`) reach live pages on every
instance. SQLite in memory (`:memory:`) is one database per instance, so use a file or
PostgreSQL to share data between them.

What holds up the loop is code that computes for long without waiting: a loop over a
million rows keeps every other request on its instance waiting until it ends. Quillo does
not break into it, since that would end the promise above (a handler runs on its own
between waits). Images, `crypto.hash_password`, files and Rust (`.qwer`) functions run off
the loop, and sorting and `stats`, `vec` and `math` on large lists are spread over the
cores. So put long computing in Rust (see [Rust](#rust)), or run more instances: with
several, connections are taken on a thread of their own and go to an instance that is not
computing, so one stuck instance holds up only the connections already on it.

An instance computing for 5 s is logged, with its request. Started with
[`--max-compute 2s`](#options-of-quillo-run) (`quillo run main.qwe --max-compute 2s`), the server ends a request
that computes that long without letting go: a 500, between two statements, so nothing is
left half done, and a line in the log. `highlight`, `markdown` and `markdown_page` end at
it too, inside a long text, and their end is not an error value a `??` takes, as a loop's
is not. Time spent waiting does not count. Compiled code has
nowhere to stop, so under `--max-compute` functions are not compiled. `quillo check` warns on
a `while` that cannot end.

Started with [`--max-memory 512MB`](#options-of-quillo-run), no one value a request makes may be
bigger than that, and a request that would make one ends there with a 500 (`this request's
value would be bigger than --max-memory allows (N bytes)`) and a line in the log, while every
other request goes on. No `??` takes it, and it is said at the line that made the value:

- Every text a builtin or an answer writes stops as it would pass the limit: `+`, templates,
  `str`, `join`, `replace`, `re.replace`, `upper`, `lower`, `normalize`, `escape`,
  `url_encode`, `raw`, `markdown`, `json.stringify`, a route's JSON, an `http` request's JSON
  body, a page and its components (their attributes, `class` and `style` values, the text
  inside a `<script>` or a `<style>`, a `<select>`'s value, the props and state their islands
  carry), a `server fn`'s answer, a live part's too, a stream's piece. A deferred part that
  would pass it keeps its placeholder, as a part that fails does, and its stop is said in the
  log.
- Lists and maps are compared as they grow: `push`, `range`, `flatten`, spreads, `merge`,
  `set`, `group_by`, `json.parse`, a site's answer, a query's rows, a form's fields.
- A visitor's JSON is read with each text, list and map counted at its size as it is made:
  `request.json` and a form's JSON, a `server fn`'s arguments (the files and decimals in
  them too), `/_quillo/markdown`'s text (and the HTML it will make, worked out first), and a
  page's channel message over 1 KiB. So a small body that would make many empty lists stops
  at the limit *(since 0.8.9: JSON was counted in values, at least 1,000,000 whatever the
  limit, and a 3 KB body made 136 MB)*.
- Bytes read stop at the limit (`read_file`, `http`, a site's call, a request's body), and
  their text (U+FFFD for each byte that is not UTF-8) is held to it too.
- What pages' channel messages hold while they wait for a handler is at most 256 MiB, or the
  limit when it is smaller (see [Channels](#channels)).
- Not held to it: the sum of many values each under it (bounded by time, with
  `--max-compute`), copies no larger than what they copy (`slice`, `map`, `filter`, `sort`),
  error messages, sessions, the values channels keep, and the PostgreSQL driver's own rows.

Each instance and each `--split` service holds the same limit, and `--split` passes
`--max-compute` on to its services too *(since 0.8.7: they ran without it)*.

How long a wait may last:

- [`--workers N`](#options-of-quillo-run) (64 by default) requests can wait at once, each on a pool thread; more
  wait for a thread, while requests that only compute go on being answered. PostgreSQL
  queries take no thread, but a statement of a PostgreSQL or MySQL transaction does: it
  goes before the other waits, and takes a thread past `--workers` when all are taken, so
  the transaction that holds a row's lock always finishes, however many requests wait on
  that row *(since 0.8.9: with every thread waiting on one row's lock, the commit had none,
  and PostgreSQL waited for good, a graceful stop with it, and MariaDB 50 s and then error
  1205)*. A thread the system will not start (an account's process
  limit, which on Linux counts threads) is said once in the log, `quillo: cannot start a
  thread for waiting work (...; the account's process limit?); requests wait for the N
  threads there are`; with none at all, it tries again a few times within a tenth of a
  second (another process may have ended), at most once a second. At
  start, a server whose `ulimit -u` is below what it may use says so: `quillo: the process
  limit (ulimit -u) is 30, and this server may use 74 threads: requests may wait for a
  thread; raise it, or give --workers fewer`.
- `http.*`: 10 s to connect, 60 s in all (300 s between the parts of a stream). A call to
  a `service` elsewhere: 5 s to connect, 10 s for a try, or its `timeout` (see
  [Services](#services)).
- A PostgreSQL connection: when all of the pool's are taken, a request waits up to 30 s
  for one, then gets an error. A MySQL transaction waits for one of its pool's the same
  way, holding no thread meanwhile, then gets `` no MySQL connection free after 30 s: all
  the pool's are in use (`?pool_max=N` in the URL allows more) `` *(since 0.8.9: it waited
  on a thread with no limit, and took the threads other requests, and the transactions
  holding the connections, needed)*.
- A query has no limit from Quillo: give the database one (on PostgreSQL,
  `alter role app set statement_timeout = '5s'`).
- Inside a SQLite transaction a wait holds up every request, since another request's SQL
  would land in the transaction: keep `http.*`, `sleep` and Rust (`.qwer`) calls out of
  them. A SQLite query itself always runs under the lock, on the loop's thread.
- A stream's `send` inside a SQLite transaction does not wait for its client either. What
  the client has not read yet is kept, up to 4 MiB (less under
  [`--max-memory`](#options-of-quillo-run)); past that, the transaction waits a second in
  all for the client, then `send` fails as it does for a client gone, and the transaction
  rolls back. Read the rows inside the transaction and send them after it. `quillo check`
  warns of a `send` inside `db.transaction` or `db.atomic` *(since 0.8.9: a client that read
  nothing held every request of the instance for up to 30 s at each `send`)*:

  ```quillo
  route GET /export = stream((send) => {
      let rows = db.transaction(() => db.query("select * from orders"))
      for r in rows { send(json.stringify(r) + "\n") }
  })
  ```

What a server is made of, for sizing a machine: the event loop's thread (a 16 MiB
stack; each request's coroutine reserves as much, and grows as it needs), up to
`--workers` plus one waiting threads per instance (so `--instances 4 --workers 64` may
reach 260 threads), one thread for PostgreSQL's pipelined connections (half the
instances, at least two, with `--instances`), a pool of one less than the cores (at most
31) for long lists, up to four for images, one for scheduled jobs, one for calls made
later, one for [`--watch`](#options-of-quillo-run), one for the sockets of every page with live data or on a channel, and one for each channel and [`--with`](#options-of-quillo-run) command. Memory is not
limited by anything configurable: the allocator is mimalloc on the loops and the waiting
threads, the system's elsewhere.

A page with live data, or on a channel, takes no thread of its own: its socket is read and
written on the one thread all those pages share, and costs a few kilobytes and one file
descriptor while it is open (1,000 pages with live data, 8.5 MB in all; 1,000 on a channel,
12 MB), all given back when it closes. A page on a channel over WebTransport is the same. Quillo
takes all the descriptors the system allows when it starts, and says so in the log if that is too
few for 4096 pages; `ulimit -n`, or `LimitNOFILE=` under systemd, raises what is allowed.

With [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`), a change to the program or a `use`d file reloads it (starting its
state afresh) and refreshes open pages. [`--reload`](#options-of-quillo-run) reloads it the same way without
`--watch`'s development aids (errors on pages, refreshing them), for production
programs that change their own files, such as a plugin editor. Both watch the main file
and every file the program loaded (`use`d, mounted, a service's; under `--watch`, files
served with `file()` too): on macOS and Linux the system tells of a change as it happens
(kqueue, inotify), and a save, in place or as an editor does it, a new file renamed over
the old, is seen within a few milliseconds; elsewhere, or when the system cannot watch a
file (too many open, a network mount that tells nothing), the files' modification times
are looked at every 50 ms, and said so once in the log. No folder is watched, so a new
file is noticed once something loads it. A version that fails to load or check does not
replace the one that works; the errors are printed.
A reload parses only the files that changed (the others' parses are kept) and, under
`--watch`, compiles the program for the browser on a thread of its own once the new
version answers, so a save shows in open pages about 120 ms later for a program of a
few hundred components (the site quillolang.org, measured), plus the time to parse the
file that changed.

What a reload keeps, and what it starts afresh:

- **Requests in progress** finish on the version they started on: its code, its
  variables, its services in this process and its database connections (a query or a
  transaction in progress goes on, and commits or rolls back as it would have). Requests
  that come after are answered by the new version. While the new version loads, Quillo
  code waits (what waits on a database goes on); no request is dropped.
- **Sessions** are signed cookies or rows in the database, not server memory: they stay
  valid. The signing key is `QUILLO_SECRET`; without it a key made at start, which a
  reload keeps and a restart does not (everyone is signed out), so set it in production.
- **Global variables** start afresh: keep in the database what must last.
- The old version's database connections close once no request uses it (within a second
  or so of the last one). Calls made later and transactions over several databases left
  part way are looked after by whichever version is current; a version that is the
  first to use services or `db.atomic` starts what does that.
- What outlives a restart: the database (a file or server; without `db.open` or
  `QUILLO_DB` it is in memory and gone), `quillo-system.db` beside the program, the
  `.quillo/native/` builds, and sessions signed with `QUILLO_SECRET`. Globals, the
  memory of calls a service answered, the live cache and channels start empty.
- There is no handing a port from one process to the next: a new `quillo` binary means
  a stop (requests in progress finish, however long they take unless [`--drain`](#options-of-quillo-run) gives a
  time) and a start, during which the port
  answers nothing. For no gap, run two servers behind limer ([`--register`](#options-of-quillo-run); one that starts
  stopping leaves the pool, and its port refuses connections, which a health check sees as
  down) and restart them one at a time. Two programs
  cannot share a port: `mount` or `service /prefix` them in one. A new binary whose Rust
  glue changed builds the program's Rust files again: run `quillo build` with it as part
  of the deploy, before the restart (see [Rust](#rust)). While an old and a new copy
  share one database, a job whose name changed between them may run under both for one
  slot.

## Stopping and health

The first Ctrl-C or `SIGTERM` (`kill <pid>`) stops the server gracefully: it takes no new
connections (its port refuses them at once, even while a request holds the server
computing), closes idle ones, answers every request in progress, however long it takes, so
that a transaction under way commits whole, finishes sending the large files being sent
(a download), and exits with status 0. An answer given
meanwhile closes its connection and says so (`connection: close`), so the client sends its
next request elsewhere. A request still arriving 30 s after the stop (its head, or a body
not yet whole) has had no work done, and is answered 503 (`the server is stopping`) and its
connection closed, the log saying why *(since 0.8.9: a body trickled at a little over 1 KiB
a second held the stop for as long as it kept coming)*. Every 5 s it says what it still
waits for:

```
quillo: stopping once the requests in progress are answered (again to stop now)
quillo: still waiting for GET /pay (rid 01HF3K...), 1 file being sent (again to stop now)
```

[`--drain 30s`](#options-of-quillo-run) gives the stop a time: what still runs then is cut off, its
transaction rolled back, and said: `quillo: stopped with GET /pay still running`, or, for a
request that holds its instance then (computing, or waiting inside a SQLite transaction),
`quillo: stopping with requests in progress: GET /pay (rid 01HF3K...)`. Without
it, a request that never ends holds the stop until a second Ctrl-C, or until whatever runs
the server gives up on its own: systemd sends SIGKILL after 90 s (`TimeoutStopSec`), Docker
after 10 s (`docker stop -t`), Kubernetes after 30 s (`terminationGracePeriodSeconds`), so
give `--drain` a little less than that time to have the cut said. A server that joined a
pool with [`--register`](#options-of-quillo-run) (see [Several servers](#several-servers)), or a copy that joined its app with
[`--join`](#options-of-quillo-run), leaves it first, so it is sent no more requests. A second Ctrl-C stops it at once. It stops the same when its stderr
goes into a pipe that has closed (a `tee` or a log shipper that ended): Quillo's own
messages are then dropped, and never keep it from stopping.

`GET /_quillo/health` answers `200` (`ok`) while the server is ready for requests, and
`503` (`unavailable`) while its program failed to load, or, asked on a connection open
before a stop, while it is stopping (a new connection is refused then): give it to a load
balancer's health check (limer's `health_uri`). `GET /_quillo/ready` also says
whether what the program needs is there: `{ "ready": true, "checks": { "database":
"ready", "service billing": "ready", "ledger": "not ready" } }`, 200 or 503, its
database (`select 1`), each service it declares (see [Services](#services)) and the
program's own checks, `ready("ledger", () => ...)` at the top level, a function that
gives true. For an orchestrator's readiness probe. With a database per tenant (see [Many
tenants](#many-tenants)) the database is `"ready (per tenant)"` when a tenant can be
opened: the tenants' folder can be written, or their server answers (MySQL by a real
login), and no database is opened or made to ask. A program with no folder of its own (one
`quillo::eval_str` runs) makes its tenants' files in the working folder, so that is the
folder looked at. A server's URL the driver would refuse (`postgres://q@[::1/app_{tenant}`,
a port that is no number) is `"down"`, its reason written once to stderr (`quillo:
/_quillo/ready: the tenants' database is down: ...`). Neither request is logged or counted
(a route of the program's own at a path ending in `/_quillo/ready` is).

A program with nothing to serve over HTTP (only `every`, `udp.listen` or jobs) listens on
the default port, or `$PORT`, when it is free, for its health and metrics (`quillo: nothing
to serve over HTTP; health and metrics listening on ...`). When that port is taken and
nothing asked for one, it runs without a port, and says so (`quillo: 8080 is taken; this
program serves nothing over HTTP, so it runs without a port (give --port for
/_quillo/health)`): give it `--port` to reach `/_quillo/health` and the metrics. Its
ticks, packets and jobs then reach it through a socket in a folder of its own under the
temporary folder, which only the user running it may enter (0700, whatever the umask),
removed as it stops. A port
asked for (`--port`, `$PORT`, `--host`, `--listen`, `--metrics`) that is taken stops it, as
for any program.

`GET /_quillo/health/queries` says how long database queries take: the minimum,
maximum and average of the last 100, with how many ran and failed in all:

```
{ "enabled": true, "window": 100,
  "queries": { "count": 100, "min_ms": 0.21, "max_ms": 48.7, "avg_ms": 3.4, "total": 5120, "errors": 2 },
  "statements": [ { "sql": "select * from orders where customer = ?", "count": 100, "min_ms": 1.9, "max_ms": 48.7, "avg_ms": 12.3, "total": 840, "errors": 0 }, ... ] }
```

- **Off until switched on**, so a server pays nothing for it: on under [`--watch`](#options-of-quillo-run), when
  the server is started with [`--query-stats`](#options-of-quillo-run) or `QUILLO_QUERY_STATS=1`, or while it runs with `POST
  /_quillo/health/queries?on=1` (`?on=0` switches it off; either way what was counted is
  forgotten). Off, the address answers `{ "enabled": false }`.
- **`statements`**, each statement's figures, slowest on average first (at most 100 of
  them, its SQL as written with its `?`s, spaces squeezed), are shown only to a request
  with `Authorization: Bearer` and `QUILLO_HEALTH_TOKEN`, which switching on and off
  needs too: they show the program's SQL, and a server behind a proxy sees every
  visitor coming from this machine. Under `--watch`, requests from this machine may.
- `db.exec`, `db.query` and `db.one` are counted, a replica's answers too, failed ones
  as errors; Quillo's own tables (`_quillo_jobs` and the like) are not. The last 500
  statements are kept; past that, the one used longest ago makes room.

On a server already running, with the token in its environment:

```sh
export QUILLO_HEALTH_TOKEN=0123456789abcdef-token        # 16 characters at least
quillo run main.qwe                                      # or with --query-stats, on from the start
curl -X POST 'localhost:8080/_quillo/health/queries?on=1' -H "Authorization: Bearer $QUILLO_HEALTH_TOKEN"
curl localhost:8080/_quillo/health/queries -H "Authorization: Bearer $QUILLO_HEALTH_TOKEN"   # with statements
curl localhost:8080/_quillo/health/queries                                                    # the totals only
```

Without the switch, the address says how to turn it on: `"how": "off: run with
--query-stats or QUILLO_QUERY_STATS=1, or POST /_quillo/health/queries?on=1 with the health
token"`.

A second Ctrl-C, or a second `SIGTERM`, exits at once with status 130. Stopping also
stops the commands run with [`--with`](#options-of-quillo-run), and a stream's `send` fails, and its `sleep`
ends, once the server is stopping (but in a transaction, which finishes), so a long stream ends
as when its client goes away. Pages waiting for live data stay connected while the stop
waits, are let go as the server exits, and connect to the next server. Scheduled jobs and calls
made later start no more: one running finishes (its `sleep` ends it), and calls made later
not yet run wait in the database for the next start.

## Scheduled jobs

```quillo
schedule("5m", clean_sessions)                  // every 5 minutes, on the clock
schedule("0 3 * * *", nightly_report)           // cron: 03:00 UTC
schedule("@daily", () => db.exec("delete from tokens where expires < ?", now()))
```

A job is a function of the program, so it takes `with` as any function does, and what it
may do is held to that each time it runs:

```quillo
fn nightly_report() -> map with db, time {
    db.exec("insert into reports (day, total) select date(?), sum(total) from orders", now())
}
schedule("0 3 * * *", nightly_report)
```

What it does is seen where it writes. Here the
nightly report the routes add to, and the page that shows its rows:

```quillo
db.open("app.db")
db.exec("create table if not exists orders (id integer primary key, total int, at bigint)")
db.exec("create table if not exists reports (day text primary key, orders int, total int)")

fn nightly_report() -> map {
    let day = str(date.today())
    let r = db.one("select count(*) as n, coalesce(sum(total), 0) as total from orders where at > ?", now() - 86_400_000)
    db.exec("insert into reports (day, orders, total) values (?, ?, ?) on conflict (day) do update set orders = excluded.orders, total = excluded.total", day, r.n, r.total)
}

schedule("0 3 * * *", nightly_report)

route POST /orders { form: { total: int } } with db, time = db.exec("insert into orders (total, at) values (?, ?)", total, now())
route GET /reports with db = <ul>{for r in db.query("select * from reports order by day desc") => <li>{r.day}: {r.orders} orders, {r.total}</li>}</ul>
```

Each run is a line in the log, `POST /_quillo/job/nightly_report?at=1793631366 -> 200
(3.5ms)`, and `/reports` shows `2026-11-02: 1 orders, 40` after it. To try a job without
waiting for 03:00, give it `"10s"` while you work on it.

- The first argument is a duration (seconds, or `"30s"`, `"5m"`, `"1h"`, `"1d"`,
  `"2w"`, `"1y"`), counted from 1970 in UTC, so that every server agrees and `"5m"` runs
  at :00, :05, ...; or a cron line: `minute hour day month weekday` in UTC, with `*`,
  `1-5`, `*/15` and lists, its fields apart by spaces or tabs, or `@hourly`, `@daily`,
  `@weekly`, `@monthly`, `@yearly`. A job first seen (at start, or after a reload) waits
  for its next time, counted from when the server first looks: started at 04:01,
  `"2 * * * *"` runs at 04:02, and a nightly `"0 3 * * *"` of a server deployed each day
  at 02:00 runs at 03:00 that day; it never runs at once. The clock is looked at once a
  second.
- `schedule` and `every` add a job or a tick as the program starts, at its top level
  (or in a function the top level calls): called once it has started, in a route, a
  `server fn`, a job or a tick, they stop with an error (a 500 in a route), where they
  answered as if added and the job never ran. `quillo check` reads their written-out
  arguments as the start does (`every(0, f)`, a cron field `60`, `*/0`, `5-1`, `MON`,
  `@reboot`, six fields, a bad `{ name }` are errors there too).
- A job runs on the server like a request: waiting for the database lets requests run
  meanwhile, its errors are printed, and it is logged (`POST
  /_quillo/job/clean_sessions?at=1761600000 -> 200`, the time it was due for). A job
  still running when it is due again is skipped that time. Stopping the server lets
  running jobs finish, and starts no more.
- Jobs are named after their function (`{ name: "..." }` for a lambda, else
  `job-<line>`, or to run one function twice): letters, digits, `_`, `-` and `.`, at
  most 64, and no two alike. The start refuses a second job of a name when it is added;
  `quillo check` says so as an error where both calls surely run (twice at the top level,
  or a top-level function that adds it called twice), and warns where both may (two
  `if`s, `&&`, `||`, `??`, a loop); the arms of one `if`/`else` or `match` are
  alternatives, and say nothing. Nobody outside the server can start one.
- A job that runs for ever (a loop with `sleep` in it) is told to end when the server
  stops, and when [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`) loads the program again: its `sleep` ends the job, so a
  new version never runs beside the old one (a tick's too, and at a stop a stream's; a
  request's `sleep` is waited out, since requests in progress are answered). A job still running when it is due again
  is said once in the log, not each time.
- A service's jobs run with the app's when the service shares its process
  (`POST /_quillo/job/game~end_round` in the log), and in the service's own process
  when the app is started with [`--split`](#options-of-quillo-run) (`quillo run app.qwe --split`).
- Several servers on one database (PostgreSQL, or SQLite shared by processes on one
  machine) run each job once per due time: the first to record it in `_quillo_jobs`
  runs it. Without a database, or with one per tenant, each server runs its jobs;
  starting a server with [`--no-jobs`](#options-of-quillo-run) (`quillo run main.qwe --no-jobs`) turns them
  off there. For a database per tenant, loop over your
  tenants inside the job with `with_tenant(name, () => ...)`.

## Ticks: `every` on the server

```quillo
let ball = { x: 0, dx: 4 }

fn step() {
    let dx = ball.x + ball.dx > 300 || ball.x + ball.dx < 0 ? -ball.dx : ball.dx
    ball = { x: ball.x + dx, dx }
}
fn snapshot() -> map = { x: ball.x }

fn tick() { step(); channel.broadcast("room", snapshot()) }
every(50, tick)                                 // 20 times a second, on the clock

component Court() {                             // the other side: each page in "room"
    state x = 0
    channel.open("room", (s) => { x = s.x })
    <svg width="320" height="40"><circle cx={x + 10} cy="20" r="8"/></svg>
}

route GET / = <html><body><Court/></body></html>
```

Every page open on `/` shows the same ball, moved by the server 20 times a second; a page
opened later picks it up where it is.

- `every(ms, f)` at the top of a program runs `f` on the server every `ms`
  milliseconds, counted from 1970 so that each tick falls on a multiple of `ms`: the
  loop of a game, a sensor read, a short poll. In a component it is the page's clock
  instead (see [Components](#components)).
- A tick is named after its function (`{ name: "..." }` for a lambda, else
  `every-<line>`), and no two alike, as jobs are (above); `every` on the server is
  only for the top level, as `schedule` is. It runs like a job, one at a time: a tick still
  running when the next is due skips that one, so a slow step slows the loop and never
  doubles it. Ticks are not logged or counted in [`--metrics`](#options-of-quillo-run) (`quillo run main.qwe --metrics`), are not recorded in
  `_quillo_jobs` (each server runs its own), and stop with the server and on a reload.
- Under a second is what `every` is for; for minutes and hours, `schedule`.

## The REPL

`quillo repl` (or `quillo` alone) runs code as you type it, and prints each expression's
value:

```
quillo> let xs = [1, 2, 3]
quillo> xs | map((x) => x * 10)
[10, 20, 30]
quillo> fn double(n) {
....>   return n * 2
....> }
quillo> double(21)
42
quillo> db.open("app.db")
quillo> db.query("select * from notes")
[{ id: 1, text: "first" }]
```

- What a line defines (`let`, `fn`, assignments, `use "lib.qwe"`) stays for the lines
  after it. A statement left open (a `{` not yet closed) goes on to the next line, shown
  by `....>`.
- A null is printed where it is what you asked for: a name, a field or an index (`p.age`
  prints `null` when `p` has no `age`), or `null` itself. A call whose value is null prints
  nothing, so `db.open(...)` above, a `print` and a function of yours that gives nothing
  show only what they do; nor does an `if`, a `match` or a block whose value is null.
- It runs from the folder it was started in: `use`, `db.open` and `read_file` paths are
  from there. `db.open` (or `db.connect`) gives it a database, as in a program. It does
  not read a `.env`.
- An error stops the rest of its line; what came before it stays. The static checks do
  not run here (`quillo check` runs them on a file), and routes declared here are not
  served. Ctrl-D exits. It has no line editing or history of its own: `rlwrap quillo
  repl` adds them.
- From a pipe (`quillo repl < steps.qwe`) it prints only the answers: no greeting and
  no prompts. Input that ends inside a statement is an error naming the line it began
  on, and a statement that failed makes the exit status 1. A line that is not UTF-8 is
  said by its number (`line 3 is not UTF-8 text (byte 0xC3 at column 1)`) and skipped,
  the rest runs, and the exit status is 1. `quillo repl -i` keeps the
  greeting and the prompts from a pipe too, as Python's `-i` does.

## Scripts as tools

A program with no routes is a script: `quillo run tool.qwe` runs it from the top and ends.
What follows `--` is its arguments, `args()`; `stdin()` is all that is piped in, and
`read_line()` the next line, `null` at the end; `exit(n)` ends it with status `n` *(since 0.7)*. A script
that ends by itself ends with 0, and one that fails (a stop, or an error a `?` at the top
passes on) with 1, saying what it was. Files are the program's folder's, as for a server;
[`--allow-path DIR`](#options-of-quillo-run), given when the script is started
(`quillo run tool.qwe --allow-path ../data -- x`), names another folder that `read_file` and `write_file` may use, by its
own path or through `..`, where a link leads being what counts (FEEDBACK29.md, Q-357);
without it, a link out of the folder is refused too (see [Paths and
links](#paths-and-links)). It is named, never assumed: the same program may serve strangers later, and in a program that
serves, `stdin`, `read_line` and `exit` are errors. A script whose output pipe closes
(`quillo run tool.qwe | head -1`) ends quietly, with status 141, as other tools do; a
server whose stdout closes goes on answering, and what `print` writes is dropped.

```quillo noexec
// cat orders.csv | quillo run total.qwe -- EUR
let currency = args()[0] ?? "USD"
let total = 0m
let line = read_line()
while line != null {
    let parts = split(line, ",")
    total += dec(parts[2]) ?? 0m
    line = read_line()
}
print(`${total} ${currency}`)
exit(total > 0m ? 0 : 3)
```

Written the way each is usually written, a script is faster than Python's: `bench/vs-python/run.py`
times twenty everyday programs in both, process start included, and Quillo is ahead in all of
them. Against Python 3.9 on a 10-core Mac, the median of 3 runs each, on 0.8.6:

| | Python | Quillo |
|---|---:|---:|
| `fib(35)` | 1.09 s | 0.05 s |
| a float loop of 50 million | 1.70 s | 0.03 s |
| a `for` summing 5 million | 0.14 s | 0.09 s |
| `filter`, `map`, `sum` over 3 million | 0.09 s | 0.07 s |
| a million `xs = xs \| push(i)` | 0.04 s | 0.01 s |
| a map of lists, 100,000 pushes | 0.02 s | 0.02 s |
| a grid of 1,000 by 1,000, 100,000 writes | 0.02 s | 0.02 s |
| a field read a million times | 0.03 s | 0.02 s |
| 300,000 JSON rows grouped | 0.17 s | 0.12 s |
| word frequency over 1.2 million words | 0.21 s | 0.15 s |
| a million counts by an int key | 0.07 s | 0.03 s |
| a sieve to 2 million | 0.09 s | 0.08 s |
| `s += "x"` a million times | 0.04 s | 0.01 s |
| `s[i]` over 300,000 characters | 0.02 s | 0.01 s |
| a `while` over 200,000 characters | 0.02 s | 0.01 s |
| 5 million calls of a function ending in `return` | 0.31 s | 0.01 s |
| 5 million calls of a function with an early `return` | 0.37 s | 0.01 s |

The closest are a map of lists (0.98 of Python's time; 0.8.5's code gives 0.95 to 0.98 on the
same machine the same day), the sieve (0.91) and the grid's writes (0.81); loops over numbers, lists of
them, texts and counts run in machine code
(see [Compiled numbers](#compiled-numbers)).

## Debugging

`quillo dap` is a debugger for editors: it speaks the Debug Adapter Protocol on stdin and
stdout, and runs the program as `quillo run` would. In VS Code, with the extension, press F5
in a `.qwe` file (or *Quillo: Debug this file*, or the ▶ menu). A `launch.json` entry
gives the options:

```json
{
  "type": "quillo",
  "request": "launch",
  "name": "Debug the app",
  "program": "${workspaceFolder}/main.qwe",
  "args": ["--watch", "--port", "8080"],
  "stopOnEntry": false
}
```

- **Breakpoints** go on statements, in any file the program loads (`use`d files, mounted
  programs). One on a line with nothing to run (a blank line, `fn name(...) {`) moves to the
  next statement in the same function or route; outside any, it is not set.
- **Conditional breakpoints** stop when their expression is true; a **hit count** stops
  from that hit on (`3`), or at `== 3`, `> 3`, every `% 3`. **Logpoints** print their message to the
  debug console, with each `{expression}` in it worked out, and do not stop.
- **Runtime errors** (the mistakes that make a route answer 500) stop where they happen, with
  the error and the variables as they were. *Runtime errors*, in the breakpoints list, turns this off.
- **Stepping**: over, into and out of calls. A frame is a function, a view, a component, a
  route (`GET /notes/:id`) or a file's top level (`<main>`).
- **Variables**: the frame's locals, the program's globals (without its functions and the
  builtins), and the request being answered. Lists and maps open; long lists come in pages.
- **The debug console** runs code in the stopped frame: expressions, and assignments
  (`count = 0`) too. Hovering a name shows its value; hovering does not call functions.
- The program's output (`print`, the request log, errors) shows in the debug console.

When the program stops, all of it stops: no other request runs Quillo code until it goes on
(waits already under way, for a database or the network, finish meanwhile). Pausing a server
that is waiting for requests stops the next one to run.

While debugging, views are not cached (as with the option [`--no-view-cache`](#options-of-quillo-run)), so a breakpoint in one stops each
time it renders, and functions are not compiled to machine code. `.qwer` functions run as they
are, without stops. It runs on macOS and Linux; not yet on Windows.

### In the browser

A component runs in the browser too, after its first render: its event handlers and updates.
With `"browser": true` in the launch configuration (or `"edge"`), VS Code opens the page in
Chrome (or Edge) once the server listens, with its JavaScript debugger, in the same session:
a breakpoint on a line of a `.qwe` file stops on the server and in the browser, wherever that
line runs. `"url"` names the page to open (by default, the address the server listens on).

This works because in development ([`--watch`](#options-of-quillo-run), or under the debugger) a page's bundle is a
small loader for files of their own: the runtime (`quillo.js`, as it is written), and each
component, view and function at `/_quillo/src/<hash>/<file>.qwe/<Name>.js`, with a source map
from its lines to the `.qwe` file's. The browser's developer tools show them by those names,
with the `.qwe` source beside them, and a variable a statement declares carries its own name
(`count`, which the compiled code calls `$count_1`); a component's state is a signal, its
value in `.v`. Values are JavaScript's: the debug console takes JavaScript, not Quillo. In
production a page's bundle is one file, without comments or indentation, as before. In both, the runtime keeps its
names to itself: none of its functions is the page's, so a page's own `function report()`
or a widget's `function run()` and Quillo's leave each other alone (FEEDBACK42.md, Q-481).

### For developer tools

*(since 0.7)* A browser extension or a test can follow what the runtime does, without the debugger. It
asks by setting `data-quillo-devtools="1"` on `<html>` before the runtime starts (a content
script at `document_start`, or the page itself); a page that does not ask does nothing
more. The runtime then says what happens as `quillo:devtools` events on `document`, each
`detail` a JSON string, with values written as the wire writes them (`{"$dec": "12.50"}`,
`{"$int": "..."}`, `{"$quillo_err": ...}`):

```js
document.documentElement.dataset.quilloDevtools = "1";
document.addEventListener("quillo:devtools", (e) => console.log(JSON.parse(e.detail)));
// {"type":"component","id":1,"name":"Panel","parent":null,"props":{},"state":{"count":0}}
// {"type":"state","id":1,"name":"count","value":1}
```

| `type` | When | Carries |
|---|---|---|
| `hello` | the runtime starts | `protocol` (1), `version`, `dev` (under `--watch`), `base` |
| `component` | one is taken up or made in the page | `id`, `name`, `parent` (the id of the one it is in), `props`, `state` |
| `state` | a state is set to another value | `id`, `name`, `value` |
| `gone` | a component leaves the page | `id` |
| `error` | an error is reported | `message`, `place` (the `.qwe` file and line, under `--watch`), `id` when known |
| `remade` | a component is made again, not taken up | `name`; under `--watch`, `wrote` (what the page had) and `makes` |
| `call` | a server fn answers | `name`, `args`, `ms`, `ok`, `failed` (it did not reach the server), `value`, `live`, `idempotent` |
| `live` | a live change comes | `seq`, `topics`, `loads` (the components whose live states load again) |
| `channel` | a message goes or comes, over WebTransport or a WebSocket | `dir` (`in`, `out`), `channel`, `value`; or `joined` (true, false) |

A tool asks with a `quillo:devtools-ask` event, its `detail` a JSON string
`{ "id": ..., "op": ... }`, and is answered `{"type": "answer", "id": ..., "ok": ...}`, with
`error` when not:

| `op` | |
|---|---|
| `list` | each component now, as a `component` message |
| `where`, `target` | marks the component's element with `data-quillo-devtools-id` (the mark moves with the next `where`) |
| `set`, `target`, `name`, `value` | sets a state; under `--watch` only. The JSON is read as the server reads it: `2.0` is a float, an int past 2^53 exact, a map's keys in the order written |

Reading is open in production, as the page can be read with the browser's debugger anyway;
setting is not (FEEDBACK42.md, Part C).

Quillo's only names on `window` are `quillo` (the server fns, above) and `__quilloErrors`
when a page made it. What its own scripts hand each other is under one key,
`window[Symbol.for("quillo")]`: not an interface, but a test may wait on its `booted`
before driving a page (Q-486).

### Other editors

Any editor with a Debug Adapter Protocol client (Neovim's nvim-dap, Helix, Emacs's
dape, Zed) can use `quillo dap` as an adapter it starts itself: the command `quillo`,
the argument `dap`, messages on stdin and stdout. A `launch` request takes:

| Argument | |
|---|---|
| `program` | the `.qwe` file to run (required) |
| `args` | its `quillo run` options (`["--port", "8080"]`) |
| `cwd` | the folder to run in |
| `stopOnEntry` | stop at the first statement |
| `noDebug` | run without stopping at breakpoints |

`attach` is not supported yet: the debugger starts the program itself (with
[`--no-view-cache`](#options-of-quillo-run), unless `noDebug`). `browser` and `url` are the VS Code extension's;
elsewhere, open the page in a browser yourself. What the adapter offers: breakpoints
with conditions and hit counts, logpoints, stepping, evaluation for hovers, the
`errors` exception filter (on by default), and terminate.

## Logs and metrics

Each request is logged on stderr as `GET /path?query -> 200 (0.4ms)`: the path with its
query string. A query value whose name says a secret is written `[hidden]`, in the text
line and the JSON one: `/reset?token=[hidden]&page=2`. The names are `password`, `passwd`,
`pwd`, `pass`, `token`, `tokens`, `code`, `secret`, `key`, `otp`, `state`, `ticket`, `sig`,
`signature`, `credential`, `verifier`, `jwt`, `auth`, `authorization`, session ids
(`session`, `sessionid`, `sid`), and SAML's `SAMLRequest` and `SAMLResponse`, in any case
and as the last word of a longer name (`access_token`, `api-key`, `clientSecret`,
`user[password]`, `API_KEY`, `JSESSIONID`, `PHPSESSID`), and `password`, `token`, `secret`,
`signature`, `credential`, `verifier`, `jwt`, `authorization` and `session` anywhere in one
(`id_token_hint`, `session_id`) *(since 0.8.9: a sign-in or reset link's token, OAuth's
`code` and `state`, a session id and a password in a URL went to the log as they were)*. Started with [`--log json`](#options-of-quillo-run), `quillo run main.qwe --log json`, the line is a
JSON object instead, for log collectors:
`time` (RFC 3339, UTC, with milliseconds), `method`, `path` (with the query string),
`status`, `ms`, `ip` (the TCP peer), `tenant` when the request had one, and `rid` (the
request's id, also sent back as `x-request-id`):

```
{"time":"2026-11-02T14:59:16.713Z","method":"GET","path":"/orders/1","status":200,"ms":0.07,"ip":"127.0.0.1","rid":"e60956f5b6f86d27"}
```

`--log off` turns the request log off;
errors are still printed. Lines are buffered and written each turn of the event loop,
or at 32 KiB. Not logged: `/_quillo/health`, `/_quillo/metrics`, Chrome's
`/.well-known/appspecific/com.chrome.devtools.json`, UDP packets, channel messages. The
launch token of a phone app built the old way, on 127.0.0.1, never appears in it.

An error's text can hold what a visitor sent, so in a text line its control characters and
line breaks are written as escapes (`\n`, `\x1b`): one request is one line, and no one can
write a line of their own into the log or colour a terminal. A malformed request line is
written escaped too, and under `--log json` it is an object of its own, `{"time",
"status", "error", "line"}`. Quillo's own notices (`listening on`, warnings, `stopping`)
are text lines on stderr in both modes.

Started with [`--metrics`](#options-of-quillo-run), `GET /_quillo/metrics` answers in Prometheus's text format:

```sh
quillo run main.qwe --metrics --log json
curl -s localhost:8080/_quillo/metrics | grep quillo_requests_total
# quillo_requests_total{status="2xx"} 2
```

What it reports:

| Metric | |
|---|---|
| `quillo_requests_total` | requests answered, by status class |
| `quillo_request_duration_seconds` | a histogram (1 ms, 2.5, 5, 10, 25, 50, 100, 250 ms, 1 s, 5 s) |
| `quillo_requests_in_progress`, `quillo_connections` | now |
| `quillo_prefetches_total{result}` | pages asked for ahead of a click: `answered`, `refused`, `used` |
| `quillo_later_total{result}` | tries of calls made later: `delivered`, `again`, `failed` (given up on) |
| `quillo_live_pages`, `quillo_live_refused_total` | pages waiting for live data, and those refused a place |
| `quillo_live_wakes_total` | pages woken by a change to what they follow |
| `quillo_live_runs_total{outcome}` | loaders asked for: `ran`, or `shared` a run another page made |
| `quillo_live_answers_total{kind}` | answers: `same`, a `patch`, or the `whole` value |
| `quillo_uptime_seconds` | since the server started |
| `quillo_bundles` | sets of components whose bundle is kept (see [Components](#components)) |
| `quillo_udp_received_total`, `quillo_udp_dropped_total` | packets (see [UDP](#udp)) |
| `quillo_channel_pages`, `quillo_channel_dropped_total` | pages on channels, messages dropped (see [Channels](#channels)) |

It is off by default, since anyone who can reach the server could read it.

## The policy

`with` limits what a function may do, from inside the program. A policy limits what the
whole program may do, from outside it *(since 0.7)*: the host's, for every program on a server, and
the site owner's, for one deploy. Policies are small TOML files, read as the program
starts, by `quillo run`, `quillo fcgi`, `quillo mcp`, `quillo app` and `quillo check`. The
layers, in the order read:

1. `/etc/quillo/policy.toml`, the host's (on Windows `%ProgramData%\quillo\policy.toml`
   *(since 0.8.11: it was `\etc\quillo\policy.toml` on whichever drive the program ran
   from)*), whenever it is there;
2. the files it names in `include = [...]`, by their whole path *(since 0.8.11)*;
3. `policy.d/*.toml` beside it (`.TOML` in any case), in name order *(since 0.8.11)*: a
   provider's plans as files of their own;
4. each file `QUILLO_POLICY_PATH` names, a list as `PATH` is *(since 0.8.11)*, for a provider
   that starts each account's process with its own (set where the process starts: a
   `.env` never sets it);
5. each [`--policy FILE`](#options-of-quillo-run), in order;
6. `quillo.policy` beside the program's main file.

The first four are the provider's, the host's layers; the last two are the developer's.
Every file a layer names must be there: an `include` or `QUILLO_POLICY_PATH` naming a
file that is not stops the program. A layer that is there but cannot be read (a link to
nothing, a folder, a file without read permission) stops it too, with exit 2, naming it
*(since 0.8.11: a broken `quillo.policy` was skipped without a word)*. Only the host's file may
say `include` and `require_path = true`, which stops a start without `QUILLO_POLICY_PATH`
rather than run it without the provider's limits. A provider's own build of Quillo can
compile its anchor in (`QUILLO_HOST_POLICY` at build time) in place of `/etc`.

Each file only narrows what the ones before it allow: a list is a condition every file
must meet, so a later file cannot give back what an earlier one took away. A file that
says something a policy does not say, or a word that is not an effect, stops the program
with exit 2, naming the file and the line (`quillo.policy:2: `disk` is not an effect`).
What one refuses names the file that refused it. None of them is served: a file named
`quillo.policy`, and each policy file the program runs under, by its name or a hard link's,
is a miss for `files()` and `quillo serve` (see [Static files](#static-files)) *(since
0.8.9)*.

```toml
# /etc/quillo/policy.toml: what any program on this server may do
[plugins]
build = false                     # never run cargo: prebuilt libraries only
allow = ["csv@1", "qr", "markdown"]

[effects]
allow = ["db", "http", "fs", "time", "random", "env"]   # no net

[http]
private = false                   # no loopback, private or link-local addresses

[env]
names = ["QUILLO_SECRET", "DATABASE_URL", "APP_NAME"]

[limits]
workers = 8
max_compute = "10s"
max_body = "10MB"
```

```toml
# quillo.policy, beside main.qwe: this program narrows it further
[http]
hosts = ["api.stripe.com", "*.mapbox.com"]
```

| Section | |
|---|---|
| `[plugins] build` | `false`: never build a Rust file, as [`--no-build`](#options-of-quillo-run) (see [Rust](#rust)) |
| `[plugins] allow` | the plugins a program may `use`, by file name: `"qr"` at any version, `"csv@1"` or `"csv@1.4.2"` by the file's `//! version:` line. A plugin not listed is refused, by `quillo check` too, before anything is built. By name it does not prove their code; `"qr@sha256:<hex>"` *(since 0.8.11)* does (below) |
| `[plugins] catalog` *(since 0.8.11)* | `true`, or the path of a catalog's index (`plugins/index.json`, as `quillo install` reads it; `true` is `catalog.json` beside the policy file): every plugin of that catalog, each pinned by the SHA-256 the index gives for it, as an `allow` of them all |
| `[effects] allow` | the effects anything in the program may use, as a `with` on its top level, routes, jobs and functions alike; a Rust file with no `//! effects:` line is refused anywhere under it |
| `[http] hosts` | the hosts `http.*` and `site` calls may reach: `api.stripe.com`, `*.mapbox.com` (its subdomains, not itself), `*`. Each redirect is judged before it is followed |
| `[http] private` | `false`: `http.*` and `site` calls reach nothing on this machine or its networks (loopback, private, link-local and the cloud's metadata at `169.254.169.254`, carrier-grade NAT, IPv6's unique-local), as the name resolves when it connects |
| `[env] names` | the variables `env()` may read; any other reads as not set, so `env("HOME") ?? "none"` gives `"none"` |
| `[limits] workers`, `max_compute`, `max_body`, `max_memory` | the most [`--workers`](#options-of-quillo-run), [`--max-compute`](#options-of-quillo-run), [`--max-body`](#options-of-quillo-run) and [`--max-memory`](#options-of-quillo-run) may be (`max_memory` *(since 0.8.7)*: no one value a request makes may be bigger; its stop says `the policy p.toml allows (max_memory, N bytes)`): a smaller value given on the command line stays (`"10s"`, `"500ms"`, `"2m"`; `"10MB"`, `"512KB"` or bytes). Each takes what the option takes: 0 is refused (`p.toml:2: `max_body` is a size larger than 0`), as the option refuses it |
| `[limits] max_tool_calls` | the most tool calls a minute each user of the MCP door may make: it can lower `--max-tool-calls`, not raise it |
| `[run]` *(since 0.8.11)* | what `quillo run` may be started with (below) |
| `[fs] paths` *(since 0.8.11)* | the folders [`--allow-path`](#options-of-quillo-run) may name, or folders inside them; `[]` allows none. Each is judged where it really is, once, at the start |
| `[env] set` *(since 0.8.11)* | `set = { APP_ENV = "production" }`: what `env()` gives for those names, ahead of any `.env` (a service's own too) and the shell. The host's layers only (a `--policy` or `quillo.policy` that says it stops the start); never `QUILLO_POLICY*`, `LD_*` or `DYLD_*`; two layers that set one name differently stop the start, naming the name and not the value |

Under the two files above, with `APP_NAME=Shop`:

```quillo noexec
route GET / = `${env("APP_NAME") ?? "shop"} on ${env("HOME") ?? "no HOME"}`   // Shop on no HOME
route GET /charge = http.get("https://api.stripe.com/v1/charges").status       // reaches Stripe
route GET /other = str(http.get("https://example.com/").error)
// http: example.com is not among the hosts quillo.policy allows: api.stripe.com, *.mapbox.com
```

`quillo check --policy host.toml main.qwe` says, before a deploy, what the program would
be refused, as errors (see [Checking](#checking)):

```
quillo: policy host.toml + quillo.policy (effects: db, http, fs, env, time, random; no builds; no private addresses)
main.qwe:6: error: `udp.listen` needs the net effect, which host.toml does not allow
```

### What a provider holds a program to

*(since 0.8.11)* A provider that runs other people's programs (a host, a school, a company's
platform) writes down how they may be started, and nothing the developer controls (the
command line, `.env`, `quillo.policy`) widens it:

```toml
# a provider's plan: what each account's program may be started with
[run]
watch = false                     # no --watch or --reload in production
with = false                      # no --with commands
check = true                      # no --no-check
log = "json"                      # the request log as JSON, whatever --log says
trust_proxy = ["127.0.0.1"]       # the proxy in front; --trust-proxy may only narrow it
instances = 2                     # ceilings: the least of these and the option wins
max_connections = 500
drain = "30s"

[fs]
paths = ["/srv/shared"]           # --allow-path only inside these; [] allows none

[env]
set = { APP_ENV = "production" }  # what env("APP_ENV") gives, whatever .env says
```

`[run]` says three kinds of thing:

| Kind | Keys | |
|---|---|---|
| Off switches, `false` | `with` (`--with`), `watch` (`--watch`, `-w`, `--watch=reload`, `--reload`), `describe` (`--describe`), `plain_http` (`--plain-http`), `split` (`--split`, `--scale`, `--as-service`), `join` (`--join`, `--register`, `--allow-join`, `--join-as`, `--service`) | the option is refused with exit 2, naming the file: `quillo: --watch is turned off by the policy host.toml (run.watch = false)` |
| Forced values | `check = true`, `log` (`"text"`, `"json"` or `"off"`), `base` (`"/shop"`), `listen = "unix"`, `trust_proxy` (a list of networks) | `check` refuses `--no-check`, and a start under `--watch` whose check fails stops; `base` refuses `--split`, whose services each take their own; `listen = "unix"` takes only `--listen unix:PATH` or `quillo fcgi`, with WebTransport off; `trust_proxy` is what every layer allows, and `--trust-proxy` may only name networks inside it |
| Ceilings | `instances`, `max_connections`, `connections_per_client`, `live_per_client`, `drain` (`"30s"`), `socket_mode` (`"660"`) | the least of the layers' and the option's holds, as `[limits]`' do; `max_connections` replaces the default the file descriptors give. A connection closed at a policy's connection ceiling names its file in the log: `quillo: a connection is closed as it comes: 500 connections are open, the most the policy /etc/quillo/policy.toml allows (run.max_connections)` |

A plugin pinned by its hash *(since 0.8.11)*: `allow = ["qr@sha256:9f86d0..."]` lets `qr` load
only as that file. The hash may be the source's, the `.qwer` file's SHA-256 (what the catalog's
index gives): the plugin is then built on this machine from the very bytes hashed, in a new
folder of its own (0700) outside the program's reach (judged by the folders themselves,
whatever name `TMPDIR`, `--allow-path` or `[fs] paths` gives them; under `--allow-path /` no
folder is, and the build is refused), with a cleared environment and the
`cargo` on `PATH`, and loaded from a private copy whose hash is checked. Or it may be the built
library's own SHA-256 (as `.quillo/plugins.lock` lists it), and then only that prebuilt
library loads. Only the host's layers may pin a library: a pin in `quillo.policy` or a
`--policy` is the repository's, and would let the library it brought load. A developer who
controls their account's cargo home or toolchain can still steer a build from source, so
where that matters, pin the library and build it yourself.

`quillo policy main.qwe` shows what is in force, layer by layer: the files in the order read,
the places that had none, and each value with the file it came from (`env.set` by name, never
its value). It exits 1 when a layer that is asked for is not there (an `include`,
`QUILLO_POLICY_PATH`'s file, `require_path` without the variable) and 2 when one is there but
broken:

```
$ QUILLO_POLICY_PATH=/etc/plans/small.toml quillo policy main.qwe
the policy for main.qwe
layers, in the order read (each only narrows the ones before it):
  /etc/plans/small.toml (QUILLO_POLICY_PATH)
not there:
  /etc/quillo/policy.toml (the host's file)
  /etc/quillo/policy.d/ (policy.d)
  quillo.policy (the program's)
in force:
  env.set.APP_ENV (set) (/etc/plans/small.toml)
  fs.paths: in every one of [/srv/shared] (/etc/plans/small.toml)
  run.watch = false (--watch, -w, --watch=reload, --reload) (/etc/plans/small.toml)
  run.check = true (/etc/plans/small.toml)
  run.instances = 2 (/etc/plans/small.toml)
```

A provider's layer also asks for a complete `quillo.confirmed`, the list of what the
program's files reach (see [What a program reaches](#what-a-program-reaches)): the host's
file, what it includes, `policy.d` and `QUILLO_POLICY_PATH`. A file given with `--policy` does
not ask, as the program's own `quillo.policy` does not: both are the developer's, the rule a
library's pin follows too. In 0.8.11 a program with none, or one that leaves
something out, starts with a warning that names the release it will stop in (``the policy
host.toml asks for a complete quillo.confirmed, and quillo.confirmed leaves 2 things out:
from Quillo 0.8.12 it will not start so``); from 0.8.12, it does not start.

- `quillo check` and `quillo run` always read the host's file and `quillo.policy`;
  `--policy` adds one, and `check` names the files only when one is given. A byte-order
  mark at a file's start is skipped. The start line lists the limits that hold the
  program: `quillo: policy p.toml (workers 8; max_compute 10s; max_body 10485760 bytes)`.
- A request the policy's `max_compute` ends is a 500, as one `--max-compute` ends,
  wherever it is ended (a loop, a call, `highlight`, `markdown`), and the log names
  whichever is the tighter: `this request computed for longer than the policy p.toml
  allows (max_compute)`, or `... than --max-compute allows`. No `??` takes that stop
  (`highlight(x) ?? x` too). A page (a Markdown preview) is told `the server's policy`,
  with no path of the server's.
- [`--split`](#options-of-quillo-run) passes the `--policy` files and `quillo.policy` to its services; a service
  reads the host's file itself.
- What a policy holds to is Quillo code and what it loads. A plugin is Rust in the
  server's process, which a policy cannot watch: it can only choose which plugins load.
  `private = false` covers `http.*` only, not `db.connect` or UDP.
- A policy binds the `quillo` that reads it. On a shared server, a user who can upload a
  binary of their own can run it without one: the boundary between accounts is the
  system's (Unix users, CloudLinux's CageFS and LVE limits).
- `quillo repl` reads no policy.
- Not built yet: a catalog whose entries are signed (`[plugins] catalog` pins by the hashes
  of an index the provider keeps), and a sandbox from the system (`--sandbox`, seccomp and
  Landlock) or for plugins (WebAssembly).

## What a program reaches

*(since 0.8.11)* A component file taken from another project, or a plugin from elsewhere,
runs with the program's rights: it may post `.env` to another server as the program starts,
and `quillo check` would say ok. `quillo confirm` reads every file of the program (the main
file, and each file `use`, `import`, `mount` and `service` take, followed through), lists
what each one reaches, asks once, and writes the answer to `quillo.confirmed` beside
`main.qwe`. Commit it: the answer is the project's, and every clone, CI run and server reads
the same one. From then on the program is held to it.

```
$ quillo confirm
quillo: 2 files reach something not confirmed yet
  lib/geo.qwe
    reaches       nominatim.openstreetmap.org              line 2
    reads         uploads/logo.svg                         line 6

  main.qwe
    writes        notes/                                   line 4

  [y] confirm all   [f] file by file   [n] none: y
quillo: wrote ./quillo.confirmed (2 files confirmed)
```

```toml
# quillo.confirmed: what this program's files reach, as confirmed.
# Written by `quillo confirm`. Commit it: the answer is the project's.

["lib/geo.qwe"]
http = ["nominatim.openstreetmap.org"]
files = ["uploads/logo.svg"]

["main.qwe"]
writes = ["notes/"]
```

Each file has a block, named by its path from the main file's folder, and these lists:

| Key | What the file reaches |
|---|---|
| `http` | the hosts its `http.*` and `site` calls, its UDP sends, its databases (as their drivers read the address) and its `notification.send` pushes reach (for a file that calls it, `quillo confirm` lists the browsers' push hosts: `fcm.googleapis.com`, `updates.push.services.mozilla.com`, `web.push.apple.com`, `*.notify.windows.com`); `*.stripe.com`, written by hand, covers the subdomains, and `quillo confirm` keeps it |
| `files` | the paths its file builtins read (`read_file`, `file`, `files`, `image.info`, and the folders of `db.migrate` and `translations`); a folder ends with `/` and covers what is in it |
| `writes` | the paths it writes or deletes (`write_file`, `delete_file`, `save_upload`, `image.convert`'s, an `http.*` call's `save:`, a SQLite database's file and the files its SQL attaches); a path confirmed in `files` only is read, never written, and one under `writes` is read too, with no `files` entry |
| `listens` | the UDP ports it listens on at an address others reach (`udp 514`); this machine's own loopback needs none |
| `scripts` | the scripts its pages load (`client "https://..."`), each with the `sha384-...` integrity hash `quillo confirm` fetched it for |
| `loads`, `calls` | the hosts its markup loads from (`src`), and its pages call (`js("fetch", "https://...")`) |
| `routes` | for a file taken for some of its names (`import lib/x.qwe use f`), the routes, `server fn`s and tools (`fn mcp`) it adds beyond the names taken, and its `mount` and `service` lines (`mount /admin`, `service /jobs`, and a `service mcp` line's tools as `fn mcp billing.total`) |
| `site` | for a file taken for some of its names, its calls that set something for the whole site, listed as `site-wide`: `allow_cross_site /`, `site_headers /`, `tenant_from`, `moves_in_place`, `accept_calls`, `style_with`, `channel`, `media`, `ready`, `locale_from`, `translations`, `db.open` and `db.system`, which set every file's database, and `db.migrate()` on it (a connection's own `c.migrate()` is not); however written (in a function, piped, in an `if`), and as `name *` where the path or name is worked out as the program runs |
| `session` | for a file taken for some of its names, what it sets for the visitor, listed as `visitor's`: `set_session`, `end_session`, `set_cookie NAME`, `delete_cookie NAME`, `mcp_token`, `revoke_mcp_token`, and `reply`, whose headers it sends only when confirmed |
| `sha256`, `effects` | for a Rust file: its SHA-256, and what its `//! effects:` line said; Quillo cannot hold Rust to a list, only to the file |

What is held, once the file is there (`quillo run`, `quillo mcp`, `quillo app`, `quillo
build`, and a `--split` service, by its app's `quillo.confirmed`):

- A call to a host, or a file builtin on a path, its file was not confirmed for is an error
  value at the call, said once in the log. Paths are judged where the system takes them,
  each link followed before the `..` after it: `uploads/x` leading to `.env` is `.env`, and
  `data/media/../x.db`, with `data/media` a link to `shared/media`, is `shared/x.db`, not
  `data/x.db`. A `..` after a name that is not there is refused, and so is a link that leads
  nowhere; `quillo confirm` lists a path as it is written, `..` and all. Each redirect of a
  call is judged too.
- A builtin handed on as a value (`map(paths, read_file)`, `let get = http.get`, a module
  passed whole) is held to the file that names it, wherever it is called: a helper in
  another file neither lends it its own list nor takes it away.
- A file SQLite attaches, by `attach 'side.db' as s` or `vacuum into 'backups/b.db'`, is held
  as `db.open`'s file is, to the `writes` of the file whose code runs the statement; its path
  counts from the folder the process runs in, as SQLite opens it (see
  [Database](#standard-library-database)). A `file:` address, a name the SQL works out (`attach ? as x`),
  moving SQLite's temporary files (`pragma temp_store_directory`) and a lock file named
  elsewhere (`pragma lock_proxy_file`) are refused (``db: the database file:data/u.db
  attached by its address is refused for main.qwe (quillo confirm)``); `vacuum into ?`,
  given the path, is judged by it. A migration's SQL files are read as the file that calls
  `db.migrate` was confirmed to read, and its statements held as that file's, in a request
  too.
- A route, `server fn` or tool that a file taken for some of its names adds, and that was
  not confirmed, is not served (a 404; a tool is not offered, on `quillo mcp`'s stdio too),
  said at the start: `quillo: lib/geo.qwe: the route GET /x is not served: it is not
  confirmed for this file`. Its `mount` and `service` lines are held the same way, however
  they are written (in a `routes` group, an `if` or a function, the file from a variable): a
  mount not confirmed is not loaded (`quillo: kit/ui.qwe: mount /admin is not mounted: ...`),
  and a service is not served at its path, while its functions are called as before
  (`service /jobs is not served at /jobs (its calls are made as before)`). The main file's
  routes and a mounted program's are the program's own, and so is a `server fn` or tool the
  `use` names (`import lib/notes.qwe use load_notes`).
- A site-wide call of such a file that was not confirmed is not applied, said once: `quillo:
  kit/k.qwe: allow_cross_site / is not applied: it is not confirmed for this file`. A kit
  taken for `Badge` could otherwise let other sites post to every route, or drop the
  site's headers. Its `db.open` or `db.system`, which would replace every file's database,
  is an error value: ``db: db.open sets every file's database, and is not confirmed for
  kit/k.qwe (quillo confirm)``. The main file's calls, a file it takes whole and a mounted
  program's are the program's own.
- What such a file sets for the visitor (`session`) is held too: unconfirmed, `set_session`,
  `end_session`, `set_cookie` and `delete_cookie` are not applied, said once (a kit's view
  could make every visitor an admin, or sign everyone out), and `mcp_token` and
  `revoke_mcp_token` give an error value. Its `reply` still ends the request with its status
  and body, so a guard's `reply 401` stops it, but without its headers (a cookie, CORS for
  another site, a `location`): `quillo: kit/k.qwe: a reply's headers are not sent (its status
  and body are): it is not confirmed for this file`.
- `notification.send` is held as an `http` call, each push service's host for the file that
  sends. A taken file's SQL never touches Quillo's own tables (`_quillo_push`, jobs, tokens),
  however it names them: ``db: Quillo's own tables (_quillo_...) are refused for kit/k.qwe's SQL
  where a quillo.confirmed holds the run ...``.
- Each file uses only the names it takes. A name another file defines, used without taking
  it, is an error in `quillo check`, and the start refuses it, checked or not: the value
  under it carries what that file was confirmed for (main's `let reader = read_file`, used
  by a kit confirmed for nothing, read `.env` as main). Taken by name, it still does: the
  import is the grant, and `quillo fix`, `quillo check` (a warning) and `quillo confirm` say
  it (``kit/k.qwe takes main.qwe's reader (read_file, as main.qwe reaches it)``) for a
  top-level `let` holding a builtin that reaches something, a site-wide builtin, a module or
  a function value; a `fn` taken by name is plain reuse, not said. A record made from another
  file's `type` must be taken too, and a
  page state holds no function. Without a `quillo.confirmed`, a name not taken stays a
  warning (see [Files of a program](#files-of-a-program)). `quillo fix main.qwe` writes the
  imports, and says what they leave to confirm, as an import can make a file one taken for
  some of its names, whose routes are then held:

  ```
  $ quillo fix main.qwe
  ./kit/k.qwe: import lib/b.qwe use helper
  ./kit/k.qwe: import main.qwe use site
  quillo.confirmed: with these imports, 1 thing in 1 file is not confirmed: run `quillo confirm` to confirm it (until then `quillo check` stops on it, and a run refuses it):
    lib/b.qwe
      adds a route  GET /b                                   line 4
  ```

  A name in a program this one mounts or calls as a service is that program's to fix: `quillo
  fix` says so, and is run on it, then on this one again.
- A page script not confirmed is left out of the page; a confirmed one is loaded with its
  `integrity` attribute, so a changed copy on the CDN does not run.
- A held program's pages carry a `content-security-policy-report-only` of what its files
  were confirmed to load and call, when the program sets no policy of its own: the browser
  reports what goes beyond, and blocks nothing yet.
- A Rust file that is not confirmed at its SHA-256 does not load, and the start stops:
  `` tools.rs is not confirmed (its SHA-256 is ...); Rust cannot be held to a list, so it does
  not load (`quillo confirm` asks about it) ``. `quillo build` builds none that is not.
- A confirmed Rust file loads only a library this machine built from those bytes, never one
  a cloned repository or an unpacked `.quillo/native` brought along. What was built is
  recorded outside any project (`$XDG_CACHE_HOME/quillo/built.txt`, else
  `~/.cache/quillo/built.txt`, the user's alone, 0600): the first start after a change
  builds it apart, as a pinned build is built, and later starts load it as fast as before.
  `quillo build` records too, so a `quillo run --no-build` after it, on the same machine,
  loads what it built. With building off (`--no-build`, `build = false`) and no record, the
  library is refused, unless the host's policy pins it by its own SHA-256 (see [The
  policy](#the-policy)). A program run with `quillo.confirmed` and then without it, and back,
  builds again each time.
- What the program's text holds is shown escaped, in the listing, the start's question, every
  refusal and `quillo check`'s lines: a path with a terminal's control characters in it is
  written `notes/a\u{1b}[2Kb.txt`, in `quillo.confirmed` too. That file takes only the
  escapes `quillo confirm` writes (`\"`, `\\`, `\n`, `\t`, `\r`, and `\u{...}` for a character
  a terminal does not show): `\u{2e}` for a `.` is an error at its line.
- A `quillo.confirmed` that is a symbolic link is said at the start (`quillo:
  ./quillo.confirmed is a symbolic link, to /srv/shared/app.confirmed: the program is held to
  what that file says`), and `quillo confirm` writes nothing through one.

A program with no `quillo.confirmed` runs as before, and `quillo check` says only what it
always says plus the [warnings](#checking) about taken files and effects lines. Under a
provider's policy (the host's layers and `QUILLO_POLICY_PATH`, not `--policy`), which asks for
a complete one, it starts with a warning in 0.8.11 and does
not start from 0.8.12 (see [The policy](#the-policy)).

Asking, and who answers:

- `quillo confirm` asks at a terminal: `y` all, `f` file by file, `n` none. Where no one can
  answer (CI, systemd, a container) it exits 1 and says so. `quillo confirm --yes` confirms the
  list it printed, once you have read it; `quillo confirm lib/` takes only the files under
  `lib/`; `--list` prints everything each file reaches and writes nothing.
- `quillo run` at a terminal asks about what is not confirmed before it starts (`[y] confirm
  them [n] not now`); elsewhere it starts, and refuses each such thing as it is reached:
  ``quillo: 2 things in 1 file are not confirmed (quillo.confirmed): each is refused as it is
  reached; `quillo confirm` lists them``.
- **In CI, run `quillo check`, never `quillo confirm --yes`**: once there is a
  `quillo.confirmed`, `check` fails, exit 1, on each thing a change brought that is not in it
  (``lib/geo.qwe:8: error: reaches example.com: not confirmed for this file``), and `--yes`
  would confirm whatever the change brought.
- What the policy refuses stays refused: a confirmed host outside its `[http] hosts` is
  noted (`note: lib/geo.qwe: example.com stays refused: ...`). A list only narrows.
- An item the files no longer have leaves the file at the next `quillo confirm`; a file the
  program no longer takes leaves with it.
- `quillo install` *(since 0.8.11)* writes the SHA-256 it checked, and the plugin's effects
  line, into the project's `quillo.confirmed` when there is one, so a plugin changed after it
  was installed does not load (see [Plugins from the catalog](#plugins-from-the-catalog)).
- `quillo confirm --effects tools.rs` reads a Rust file for what it does (the standard
  library's file, network, process, clock and random calls, the crates that do them) and
  proposes its `//! effects:` line, saying where the file's own leaves something out. It
  follows the names the file gives them (`use std::fs as disk`, `use std::{fs::*}`, `r#fs`,
  `extern crate reqwest as web`, a dependency's `package = "reqwest"`, a type in brackets as
  in `<(std::net::TcpStream)>::connect`, the type an `impl` is for), however `::` and `!`
  are spaced, and reads `include_str!`, `include_bytes!` and `include!` as `fs`. What it
  cannot follow it reads as every effect but `db`, and says where: a path a macro builds
  from its argument, a foreign block (`extern "C" { ... }`, written out or by a macro), a
  crate that reaches the system or loads or runs code itself (`libc`, `nix`, `rustix`,
  `libloading`, `memmap2`, `region`, `duct`, `mlua`, `pyo3`, `jni`, `core-foundation`,
  `objc2`, any `-sys` crate and the like), `asm!`, a `use` nested more than 64 deep. A file that builds another into it is refused under a `quillo.confirmed`, by
  `quillo confirm`, `check`, `build`, the start and a reload, since the other file is outside
  its SHA-256; without one, it is a warning. That is `include!`, `include_str!` or
  `include_bytes!` however it is written or named, assembly (`asm!`, `global_asm!`,
  `naked_asm!`, by any name: its `.include` and `.incbin` read a file), an attribute that
  links a library (`#[link]`, or one a macro is given or builds) or reads a file
  (`#[debugger_visualizer]`), `#[path]`, a `mod x;` that is not `mod x
  { ... }` (one a macro writes too), a macro named by a macro's argument (`$m!`), a
  dependency by anything but its version (`path`, `git`, `registry`), and a crate whose
  macros build files in (`rust-embed`, `include_dir`, `askama`, `sqlx` and the like, and
  their macro crates). The
  file is read as Rust reads it, by its tokens: comments, every kind of string and Rust's
  whitespace, from where rustc starts (after a byte order mark and a `#!` first line that is
  not `#![...]`); a dependency line's texts as TOML reads them (`'x'` as written; a `\`
  escape or a text over lines is refused under a hold). Under a `quillo.confirmed` the build
  itself is then judged by what the compiler read: a plugin is built apart, and if rustc's own
  list of the files it read for the plugin (its dep-info) has any file but the plugin's
  source and Quillo's glue, it does not load, at a start, a load, a `--watch` reload or
  `quillo build`, and what was built is not kept (``./hi.qwer is held to its SHA-256, and
  building it read /work/secret.txt, which that hash does not cover``), and by what its
  assembler and linker read: the plugin's crate is built with its LLVM IR, and any assembly
  in it refuses it; the linker lists each file it reads, and one that is not the build's own,
  the Rust toolchain's or the system's refuses it (a `-sys` crate that links a library from
  elsewhere, as under `/opt/homebrew`, too). That catches what a reading of the text misses,
  as a dependency's macro that includes a file (`doc_comment::doctest!`) or a macro that
  hands assembly its `!` apart. Nor is a source pin built where the program can reach its
  cargo home, rustup's home, its `cargo` or `rustc`, a folder of its `PATH`, or the folder
  Quillo runs in, or inside any of them: the message names which, and the way out is to pin
  the built library. Its linker's command line is judged as given (search folders, libraries
  and files only in the build's own folder, the Rust toolchain's `lib/rustlib` or the system's
  library folders, no `..`, no rpath), then the files it read, each resolved through every
  link (one that cannot be resolved refuses the build); and the library built, or one pinned
  by its own SHA-256, loads only the system's libraries (macOS: `/usr/lib`,
  `/System/Library`; Linux: the C runtime's). A Linux linker without `--dependency-file`
  (binutils before 2.35) refuses the build: use lld. A pinned source's dependencies are not
  locked: each fresh build takes the newest release their version allows, and their build
  scripts and macros run as cargo runs them. A crate that reaches the system or loads code
  itself (`libc`, `nix`, `rustix`, `libloading`, `dlopen`, any `-sys` crate, and those
  listed above) reads as every effect but `db`; the list is no proof, as a crate it does not
  know can do what these do.
  A held or pinned plugin's library is read once, hashed, judged by what it loads (the C
  runtime's exact library names on Linux, `/usr/lib` and `/System` on macOS, no rpath, not
  named as a system library) and opened as a private copy of those bytes, one copy per
  library a process opens, which every instance and reload shares; the copy is made where
  the program cannot reach (on Linux a sealed memfd, else a private file; on macOS in the
  user's own temporary folder where `TMPDIR` is the program's), else the plugin does not
  load. No other plugin's library is judged. It is still a reading, not a proof: a crate can do what it does
  unseen (`libc::kill` reads as the `libc` crate, which the file's `crates` list confirms by
  name), and a proc macro that reads a file without telling rustc, or a dependency's own
  macro that writes assembly into a function inlined into the plugin (refused, as assembly,
  though it may be none of the plugin's), is not seen.

Not held yet: [`--watch`](#options-of-quillo-run) does not read `quillo.confirmed` again
when it changes (restart after `quillo confirm`); what a page loads and calls is only
reported, by the report-only policy; a database's address in upper case
(`POSTGRES://...`) or as a `conninfo` string is listed as a path, and refused when reached;
under [`--split`](#options-of-quillo-run), a taken file's `service` line that is not
confirmed still starts its service, on this machine's loopback only, and says it is served
under its path, which is then not served; a taken component's `page_head` sets the page's
head as any call does (a canonical link to another site); and without a `quillo.confirmed`,
`translations(dir)` reads its folder through links wherever they lead (`db.migrate`'s is
judged as `read_file`'s).

Three things a list per file does not cover, by design for now. A taken file shares the
program's data and secrets: the hold is about what it reaches outside, and what it changes
for the site and the visitor.

- **The program's open database is every file's.** A taken file's `db.query` and `db.exec`
  on the database the program opened are not held per file: a file confirmed for nothing
  can read and write the program's tables through `db`. What is held is opening one
  (`db.open`, `db.system`, a tenant's file), its migrations (`db.migrate()`), the files SQL
  attaches, and Quillo's own `_quillo_*` tables, which a taken file's SQL never touches.
  *(since 0.8.11)* So that what its SQL reaches can be read, a taken file's statement on
  PostgreSQL or MySQL/MariaDB that has the database run SQL, or name a table, from text is
  refused: `DO`, `PREPARE` and `EXECUTE` (`EXECUTE IMMEDIATE`), `CALL`, `CREATE FUNCTION`,
  `PROCEDURE`, `TRIGGER`, `RULE` or `EVENT`, the `*_to_xml` functions, the `reg*` types
  (`regclass`, `regtype`, `regproc`, ...) by name and the functions that make one
  (`regclassin`, `to_regclass`, `to_regtype`, ...), and a function that takes a table
  (`nextval`, `setval`, `has_table_privilege`, `row_security_active`, ...) given the table by
  anything but a plain string such as `'kit_seq'` (its other arguments may be anything; the
  table's is found counting past every bracket). A word inside a string, a comment or a quoted name
  is not one. A statement with values is read as written and as its server is sent it, each
  `?`, `?1` and `:name` written `$1` (PostgreSQL) or `?` (MySQL). The refusal says to run it from the main file's SQL, or a migration the main
  file runs; the main file's SQL, and a program without a `quillo.confirmed`, are not read
  this way. A function the program's own SQL made that runs SQL text can still be called
  from a taken file: it is the program's. So can one an extension the program installed
  adds, beyond `crosstab`, `connectby`, `ts_stat`, `ts_rewrite`, the `dblink` functions and
  the `*_to_xml` ones, and the functions of PostgreSQL's own extensions that run a query or
  reach a table by its name (xml2's `xpath_table`, pageinspect's, pgstattuple's,
  `pgrowlocks`, amcheck's, pg_surgery's), which are refused by their names, and so can a trigger the program
  made that a taken file's write sets off, and so can a view or materialized view the program
  made over one of Quillo's own tables on PostgreSQL or MySQL/MariaDB (on SQLite its read is
  refused). The reading fails closed: what it cannot read for
  certain is refused (MySQL's `/*!` comments, a `[` outside a string on MySQL/MariaDB, which
  `sql_mode=MSSQL` reads as a quoted name, a string left open, a statement inside an `IF`
  or a label), `EXECUTE`, `CALL` and `PREPARE` anywhere in the statement, a `CREATE` of
  anything but a table, an index, a view or a sequence, SQL that reaches the server's files
  (`COPY`, `INTO OUTFILE`, `pg_read_file`, `LOAD`), and the catalogs that hold other tables'
  values or SQL text (`pg_stats`, `pg_prepared_statements`, `performance_schema`'s
  statements, the `sys` and `mysql` schemas by any spelling (quoted, and the letters outside
  ASCII MariaDB takes for ASCII ones), the process list, `optimizer_trace`, and on PostgreSQL
  every `pg_...` name). On SQLite a taken file runs no `ANALYZE`, makes no trigger and no
  virtual table over `dbstat`, and reads neither the statistics (`sqlite_stat1` to
  `sqlite_stat4`) nor `dbstat` (a virtual table the main file made over `dbstat` is read by
  its own name: page sizes, not values). Its pragmas only read (`pragma table_info(t)`,
  `pragma journal_mode`): one given a value (`pragma journal_mode = delete`, `pragma
  foreign_keys(off)`) sets the shared connection or the file and is refused, as `SET` is on
  PostgreSQL and MySQL; Quillo's own turn of foreign keys around a migration that asks for
  it still happens. It makes no temporary table, view or index (`create temp ...`,
  `temp.x`), which would stand for the program's table of its name in every file's later
  statements, and its migrations begin or end no transaction, judged as SQLite runs each
  statement (its parameters read as SQLite reads them, `:a(...)` too); Quillo's own
  transaction and its foreign-keys switch around the file are its own. A taken file's statement begins with `SELECT`, `INSERT`, `UPDATE`,
  `DELETE`, `MERGE`, `WITH`, `VALUES`, `TABLE`, `REPLACE`, `REFRESH MATERIALIZED VIEW`,
  `EXPLAIN` of one of these (MySQL's in
  brackets too), `CREATE` or `ALTER` as above (a MySQL table's engine InnoDB, Aria, MyISAM
  or MEMORY, by name or string, wherever the server takes a table's options: after its name,
  after another option, at the start of an `alter` clause, in a partition's definition),
  `DROP` of a table,
  an index, a view or a sequence, or `TRUNCATE`; any other first word (`SET`, `GRANT`,
  `SHOW`, `INSTALL` and the rest) is refused, as is a character outside ASCII just before a
  `\` or a backtick. Tables of other databases the database user can read are not held.
- **A connection is its maker's.** A connection `db.connect` (or `db.shards`) opened is
  held to the file that opened it: a taken file that builds a connection's map by hand
  reaches nothing through it, and one that connects to the same database is held to its own
  list, as for any database it opens. One the main file hands a taken
  file, or a `c.query` read from it, runs as the main file's, as its functions do.
- **A function is its own file's.** A `fn` another file declares, taken by name (`import
  main.qwe use secret`), runs with that file's reach, a site-wide call inside it included:
  a kit confirmed for nothing that takes main's `secret()`, which reads `.env`, reads it.
  The import notes name only values that hold a builtin (`let reader = read_file`), not
  declared functions, so read what a taken file imports from the program's own.
- **The environment is every file's.** `env()` is not held per file for `.qwe` code: a
  taken file reads the program's environment, `QUILLO_SECRET` included, as it shares the
  program's open database.

## Quillo on a shared server (cPanel)

*(since 0.7)* A shared hosting account (cPanel, often on CloudLinux, with Apache or LiteSpeed in
front) gives a folder, a web server the host runs, and no root. It differs from a server
of one's own in three ways that matter to Quillo:

- **127.0.0.1 is everyone's.** Every account on the machine can connect to a TCP port
  there, past the web server, so `quillo run --port 8080` is open to the neighbours. A
  Unix socket in a folder of the account's own, or FastCGI, is not.
- **Nothing keeps a process of yours running.** There is no systemd. Processes started by
  hand or by cron are often killed. The web server starts FastCGI programs itself, keeps
  them up while requests come, and stops them when idle.
- **The account has limits.** No Rust, a disk and inode quota, and CloudLinux's LVE
  limits: processes (`NPROC`, which counts threads, for the whole account), and requests
  at once (`EP`; over it, visitors get a 508 page from the host, not from Quillo).

**The binary.** The Linux releases from 0.7.0 on are linked against glibc 2.17,
so they start on CentOS 7, CloudLinux 7 and 8, AlmaLinux 8 and newer (0.6.0's need glibc
2.35). They are glibc builds, not static ones, as a plugin is a library they load.

**FastCGI with `mod_fcgid`**, where the account has it (it is in cPanel's EasyApache 4).
`quillo fcgi main.qwe` answers FastCGI on the socket the web server gives it on file
descriptor 0, and Apache starts it, keeps it up and stops it. In the site's folder:

```sh
#!/bin/sh
# app.fcgi, made executable with chmod 755
cd /home/u/app && exec /home/u/bin/quillo fcgi main.qwe --no-build --workers 8
```

```
# .htaccess
Options +ExecCGI
AddHandler fcgid-script .fcgi
RewriteEngine On
RewriteCond %{REQUEST_FILENAME} !-f
RewriteRule ^ app.fcgi [QSA,L]
```

`.htaccess` and `app.fcgi` go in `public_html` (or the domain's folder); the program
itself, its `.env` and its database stay outside it, in `/home/u/app`, where Apache does
not serve them.

- Each request is the request the visitor made: its path (`REQUEST_URI`), headers and
  body, and `request.ip` is Apache's `REMOTE_ADDR`, with no [`--trust-proxy`](#options-of-quillo-run). The server
  does the rest as for any visitor: routes, sessions, limits, the log (on Apache's error
  log).
- The scheme is the web server's: `HTTPS=on` (or `1`) is https, another value http, else
  `REQUEST_SCHEME`; neither, unknown (hosts compared alone). It reaches the program as
  `X-Forwarded-Proto`, and a visitor's own is dropped. Behind another proxy that ends TLS, set
  `HTTPS` from it (Apache: `SetEnvIf X-Forwarded-Proto https HTTPS=on`) *(since 0.8.7)*. A write
  refused for the scheme alone is said once in the log, naming the parameter (`HTTPS` or
  `REQUEST_SCHEME`) and that line.
- Its jobs and its own calls reach it through a socket only it can open, in a private
  folder under the system's temporary one.
- What FastCGI cannot carry: WebSockets. Live data falls back to plain requests, each
  held until a change or 25 s, so live pages still follow changes; channels over a
  WebSocket do not work, and WebTransport needs a UDP port of its own. A `stream` or `events` answer
  arrives whole unless the host sets `mod_fcgid`'s output buffer to 0
  (`FcgidOutputBufferSize 0`).
- `mod_fcgid` may start several processes, each with its own global variables, as
  [`--instances`](#options-of-quillo-run) would; keep what must be shared in the database. `quillo fcgi` refuses
  [`--split`](#options-of-quillo-run), `--instances`, [`--register`](#options-of-quillo-run) and [`--join`](#options-of-quillo-run).
- A request Apache abandons while its answer streams (FastCGI's `ABORT`) ends when the
  connection closes.
- Run from a terminal, it says what it needs: `file descriptor 0 is not a socket:
  quillo fcgi is started by a FastCGI process manager (mod_fcgid), or given --listen
  unix:PATH or --port`. With [`--listen unix:f.sock`](#options-of-quillo-run) (or [`--port 9000`](#options-of-quillo-run), on 127.0.0.1) it
  makes its own socket instead, for nginx's `fastcgi_pass unix:/srv/app/f.sock` with
  `include fastcgi_params` on a server of one's own.

**A Unix socket behind `mod_proxy`**, where the host sets it up (a VPS with WHM, or a
host that offers Quillo): `quillo run main.qwe --listen unix:app.sock --trust-proxy
127.0.0.1`, kept running by the host, and Apache's

```
ProxyPass "/" "unix:/home/u/app/app.sock|http://localhost/" upgrade=websocket
ProxyPassReverse "/" "http://localhost/"
```

(Apache 2.4.47 or later for `upgrade=websocket`; `ProxyPass` is the host's
configuration, not `.htaccess`'s). The socket is made with mode 0600, for the account's
own processes: give [`--socket-mode 660`](#options-of-quillo-run) when the web server reaches it through its
group. A socket left by a server that did not stop cleanly is taken over; one another
server answers on is not (`a server is listening there`). Every visitor through the
socket is 127.0.0.1 to the program, which the server says at start until
`--trust-proxy 127.0.0.1` is given; with it, `request.ip` is the visitor's, from the
`X-Forwarded-For` Apache adds. Jobs reach the program through the socket.

**Plugins, prebuilt.** The account has no Rust, so build the program's `.qwer` files
elsewhere with `quillo build main.qwe`, on Linux on the same processor, and on a system
no newer than the server's (a plugin linked against a newer glibc does not load on an
older one: build in a CentOS 7 or AlmaLinux 8 container). Upload `.quillo/` with the
program: `.quillo/native/` and `.quillo/plugins.lock`. Run with [`--no-build`](#options-of-quillo-run), so that
nothing is ever built on the server, and a library that is not the one built is refused
rather than built again. A new `quillo` whose glue changed (the libraries' names carry its
version) loads none of the old libraries: run `quillo build main.qwe` with it as part of
the deploy that brings it, and upload `.quillo/` again; under `--no-build` the server says
which file was `built by an older Quillo`.

**A policy.** The host's `/etc/quillo/policy.toml` says what any program may do there;
`quillo.policy` beside the program narrows it for the site (see [The
policy](#the-policy)). `quillo check --policy policy.toml main.qwe`, run before
uploading, says what would be refused.

**Threads and `ulimit -u`.** A Quillo server may use [`--workers`](#options-of-quillo-run) + 10 threads (74 with the
default 64), one of them shared by every page with live data or on a channel (before 0.8.4,
each such page was a thread of its own, a channel's three), and on Linux the account's process
limit counts them, beside PHP, cron and SSH. Give `--workers 8` or so (a host's policy can cap it with `[limits] workers`). At
start a server says when `ulimit -u` is below what it may use, and a thread the system
refuses is said once (see [Concurrency and reloading](#concurrency-and-reloading)).

**The rest of the account.**

- **Settings:** the `.env` beside the program, or [`--env-file`](#options-of-quillo-run) for one outside the
  public folder. A FastCGI program gets the web server's environment, so keep secrets
  there and not in `app.fcgi`.
- **Jobs:** a FastCGI process stopped when idle runs no `schedule` or `every`. Run them
  from cPanel's Cron Jobs page instead, as a script: `cd /home/u/app && quillo run
  jobs.qwe`.
- **The database:** SQLite in a folder outside `public_html`, which one process at a
  time writes; or the host's MySQL, `db.open(env("DATABASE_URL"))`.
- **Paths:** a link out of the program's folder is refused (see [Paths and
  links](#paths-and-links)), so a link to another account's files leads nowhere.
- **Not built:** Passenger (cPanel's Application Manager starts Node.js, Python and Ruby
  apps only), and a WHM plugin that installs Quillo for every account.
