# Concurrency and reloading

One thread runs an event loop (kqueue on macOS, epoll on Linux) over every connection,
reads and parses requests, runs the program on them and writes the answers. Each request
runs as a coroutine with its own stack. A handler runs on its own from start to end,
except while it waits: a database query, `http.*` or `sleep` pauses it, the waiting is
done elsewhere (on a pool thread, or for PostgreSQL on connections all requests share,
with queries pipelined), and the loop answers other requests meanwhile. Code is written
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
left half done, and a line in the log. Time spent waiting does not count. Compiled code has
nowhere to stop, so under `--max-compute` functions are not compiled. `quillo check` warns on
a `while` that cannot end.

How long a wait may last:

- [`--workers N`](#options-of-quillo-run) (64 by default) requests can wait at once, each on a pool thread; more
  wait for a thread, while requests that only compute go on being answered. PostgreSQL
  queries take no thread. A thread the system will not start (an account's process
  limit, which on Linux counts threads) is said once in the log, `quillo: cannot start a
  thread for waiting work (...; the account's process limit?); requests wait for the N
  threads there are`; with none at all, it tries again a few times within a tenth of a
  second (another process may have ended), at most once a second. At
  start, a server whose `ulimit -u` is below what it may use says so: `quillo: the process
  limit (ulimit -u) is 30, and this server may use 73 threads: requests may wait for a
  thread; raise it, or give --workers fewer`.
- `http.*`: 10 s to connect, 60 s in all (300 s between the parts of a stream). A call to
  a `service` elsewhere: 5 s to connect, 10 s for a try, or its `timeout` (see
  [Services](#services)).
- A PostgreSQL connection: when all of the pool's are taken, a request waits up to 30 s
  for one, then gets an error.
- A query has no limit from Quillo: give the database one (on PostgreSQL,
  `alter role app set statement_timeout = '5s'`).
- Inside a SQLite transaction a wait holds up every request, since another request's SQL
  would land in the transaction: keep `http.*`, `sleep` and Rust (`.qwer`) calls out of
  them. A SQLite query itself always runs under the lock, on the loop's thread.

What a server is made of, for sizing a machine: the event loop's thread (a 16 MiB
stack; each request's coroutine reserves as much, and grows as it needs), up to
`--workers` plus one waiting threads per instance (so `--instances 4 --workers 64` may
reach 260 threads), one thread for PostgreSQL's pipelined connections (half the
instances, at least two, with `--instances`), a pool of one less than the cores (at most
31) for long lists, up to four for images, one for scheduled jobs, one for calls made
later, one for [`--watch`](#options-of-quillo-run), and one for each channel and [`--with`](#options-of-quillo-run) command. Memory is not
limited by anything configurable: the allocator is mimalloc on the loops and the waiting
threads, the system's elsewhere.

Each page with live data is a thread with 256 KiB of stack reserved, of which it touches
little, and two file descriptors, a socket and a connection. Quillo takes all the
descriptors the system allows when it starts, and says so in the log if that is too few
for 4096 pages; `ulimit -n`, or `LimitNOFILE=` under systemd, raises what is allowed.

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
  a stop (requests in progress finish, up to 10 s) and a start, during which the port
  answers nothing. For no gap, run two servers behind limer ([`--register`](#options-of-quillo-run); `/_quillo/health`
  says 503 as soon as one starts stopping) and restart them one at a time. Two programs
  cannot share a port: `mount` or `service /prefix` them in one.

## Stopping and health

The first Ctrl-C or `SIGTERM` stops the server gracefully: it takes no new connections,
closes idle ones, answers the requests in progress (for up to 10 seconds) and exits with
status 0. A server that joined a pool with [`--register`](#options-of-quillo-run) (see [Several
servers](#several-servers)) leaves it first, so limer stops
sending it requests. A second Ctrl-C stops it at once. It stops the same when its stderr
goes into a pipe that has closed (a `tee` or a log shipper that ended): Quillo's own
messages are then dropped, and never keep it from stopping.

`GET /_quillo/health` answers `200` (`ok`) while the server is ready for requests, and
`503` (`unavailable`) while it is stopping or its program failed to load: give it to a
load balancer's health check (limer's `health_uri`). `GET /_quillo/ready` also says
whether what the program needs is there: `{ "ready": true, "checks": { "database":
"ready", "service billing": "ready", "ledger": "not ready" } }`, 200 or 503, its
database (`select 1`), each service it declares (see [Services](#services)) and the
program's own checks, `ready("ledger", () => ...)` at the top level, a function that
gives true. For an orchestrator's readiness probe. Neither request is logged or counted.

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
stops the commands run with [`--with`](#options-of-quillo-run), and a stream's `send` fails once the server is
stopping, so a long stream ends.

## Scheduled jobs

```quillo
schedule("5m", clean_sessions)                  // every 5 minutes, on the clock
schedule("0 3 * * *", nightly_report)           // cron: 03:00 UTC
schedule("@daily", () => db.exec("delete from tokens where expires < ?", now()))
```

A job is a function of the program, so it takes `with` as any function does, and what it
may do is held to that each time it runs:

```quillo
fn nightly_report() with db, time {
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

fn nightly_report() {
    let day = str(date.today())
    let r = db.one("select count(*) as n, coalesce(sum(total), 0) as total from orders where at > ?", now() - 86_400_000)
    db.exec("insert into reports (day, orders, total) values (?, ?, ?) on conflict (day) do update set orders = excluded.orders, total = excluded.total", day, r.n, r.total)
}

schedule("0 3 * * *", nightly_report)

route POST /orders { form: { total: int } } = db.exec("insert into orders (total, at) values (?, ?)", total, now())
route GET /reports = <ul>{for r in db.query("select * from reports order by day desc") => <li>{r.day}: {r.orders} orders, {r.total}</li>}</ul>
```

Each run is a line in the log, `POST /_quillo/job/nightly_report?at=1793631366 -> 200
(3.5ms)`, and `/reports` shows `2026-11-02: 1 orders, 40` after it. To try a job without
waiting for 03:00, give it `"10s"` while you work on it.

- The first argument is a duration (seconds, or `"30s"`, `"5m"`, `"1h"`, `"1d"`,
  `"2w"`, `"1y"`), counted from 1970 in UTC, so that every server agrees and `"5m"` runs
  at :00, :05, ...; or a cron line: `minute hour day month weekday` in UTC, with `*`,
  `1-5`, `*/15` and lists, or `@hourly`, `@daily`, `@weekly`, `@monthly`, `@yearly`. A
  job first seen (at start, or after a reload) waits for its next time; it never runs at
  once. The clock is looked at once a second.
- A job runs on the server like a request: waiting for the database lets requests run
  meanwhile, its errors are printed, and it is logged (`POST
  /_quillo/job/clean_sessions?at=1761600000 -> 200`, the time it was due for). A job
  still running when it is due again is skipped that time. Stopping the server lets
  running jobs finish, and starts no more.
- Jobs are named after their function (`{ name: "..." }` for a lambda, else
  `job-<line>`, or to run one function twice): letters, digits, `_`, `-` and `.`, at
  most 64, and no two alike. Nobody outside the server can start one.
- A job that runs for ever (a loop with `sleep` in it) is told to end when the server
  stops, and when [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`) loads the program again: its `sleep` ends the job, so a
  new version never runs beside the old one (a tick's too; a request's `sleep` is waited
  out, since requests in progress are answered). A job still running when it is due again
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
fn snapshot() = { x: ball.x }

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
  `every-<line>`), and no two alike. It runs like a job, one at a time: a tick still
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
- It runs from the folder it was started in: `use`, `db.open` and `read_file` paths are
  from there. `db.open` (or `db.connect`) gives it a database, as in a program. It does
  not read a `.env`.
- An error stops the rest of its line; what came before it stays. The static checks do
  not run here (`quillo check` runs them on a file), and routes declared here are not
  served. Ctrl-D exits. It has no line editing or history of its own: `rlwrap quillo
  repl` adds them.
- From a pipe (`quillo repl < steps.qwe`) it prints only the answers: no greeting and
  no prompts. Input that ends inside a statement is an error naming the line it began
  on, and a statement that failed makes the exit status 1. `quillo repl -i` keeps the
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
times seventeen everyday programs in both, process start included, and Quillo is ahead in all of
them. Against Python 3.9 on a 10-core Mac, the middle of three runs each:

| | Python | Quillo |
|---|---:|---:|
| `fib(35)` | 1.11 s | 0.05 s |
| a float loop of 50 million | 1.71 s | 0.04 s |
| a `for` summing 5 million | 0.14 s | 0.10 s |
| `filter`, `map`, `sum` over 3 million | 0.09 s | 0.07 s |
| a million `xs = xs \| push(i)` | 0.04 s | 0.01 s |
| a map of lists, 100,000 pushes | 0.02 s | 0.02 s |
| a grid of 1,000 by 1,000, 100,000 writes | 0.02 s | 0.01 s |
| a field read a million times | 0.03 s | 0.02 s |
| 300,000 JSON rows grouped | 0.17 s | 0.14 s |
| word frequency over 1.2 million words | 0.22 s | 0.20 s |
| a sieve to 2 million | 0.09 s | 0.07 s |
| `s += "x"` a million times | 0.04 s | 0.04 s |
| `s[i]` over 300,000 characters | 0.02 s | 0.02 s |

The closest are a `while` over a text and a map of lists, a little under Python's time;
loops over numbers and lists of them run in machine code (see [Compiled numbers](#compiled-numbers)).

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
production a page's bundle is one minified file, as before. In both, the runtime keeps its
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
| `set`, `target`, `name`, `value` | sets a state; under `--watch` only |

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
query string. Started with [`--log json`](#options-of-quillo-run), `quillo run main.qwe --log json`, the line is a
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
starts, by `quillo run`, `quillo fcgi` and `quillo check`:

1. `/etc/quillo/policy.toml`, the host's, whenever it is there (one that is there and
   cannot be read stops the program);
2. each [`--policy FILE`](#options-of-quillo-run), in order;
3. `quillo.policy` beside the program's main file.

Each file only narrows what the ones before it allow: a list is a condition every file
must meet, so a later file cannot give back what an earlier one took away. A file that
says something a policy does not say, or a word that is not an effect, stops the program
with exit 2, naming the file and the line (`quillo.policy:2: `disk` is not an effect`).
What one refuses names the file that refused it.

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
| `[plugins] allow` | the plugins a program may `use`, by file name: `"qr"` at any version, `"csv@1"` or `"csv@1.4.2"` by the file's `//! version:` line. A plugin not listed is refused, by `quillo check` too, before anything is built. It names plugins; it does not prove their code |
| `[effects] allow` | the effects anything in the program may use, as a `with` on its top level, routes, jobs and functions alike; a Rust file with no `//! effects:` line is refused anywhere under it |
| `[http] hosts` | the hosts `http.*` may reach: `api.stripe.com`, `*.mapbox.com` (its subdomains, not itself), `*`. Each redirect is judged before it is followed |
| `[http] private` | `false`: `http.*` reaches nothing on this machine or its networks (loopback, private, link-local and the cloud's metadata at `169.254.169.254`, carrier-grade NAT, IPv6's unique-local), as the name resolves when it connects |
| `[env] names` | the variables `env()` may read; any other reads as not set, so `env("HOME") ?? "none"` gives `"none"` |
| `[limits] workers`, `max_compute`, `max_body` | the most [`--workers`](#options-of-quillo-run), [`--max-compute`](#options-of-quillo-run) and [`--max-body`](#options-of-quillo-run) may be: a smaller value given on the command line stays (`"10s"`, `"500ms"`, `"2m"`; `"10MB"`, `"512KB"` or bytes) |

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

- `quillo check` and `quillo run` always read the host's file and `quillo.policy`;
  `--policy` adds one, and `check` names the files only when one is given.
- [`--split`](#options-of-quillo-run) passes the `--policy` files and `quillo.policy` to its services; a service
  reads the host's file itself.
- What a policy holds to is Quillo code and what it loads. A plugin is Rust in the
  server's process, which a policy cannot watch: it can only choose which plugins load.
  `private = false` covers `http.*` only, not `db.connect` or UDP.
- A policy binds the `quillo` that reads it. On a shared server, a user who can upload a
  binary of their own can run it without one: the boundary between accounts is the
  system's (Unix users, CloudLinux's CageFS and LVE limits).
- Not built yet: a catalog whose entries are signed, a policy that allows catalog
  plugins only, and a sandbox from the system (`--sandbox`, seccomp and Landlock) or for
  plugins (WebAssembly).

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

**The binary.** The Linux releases from the next tag on are linked against glibc 2.17,
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
rather than built again.

**A policy.** The host's `/etc/quillo/policy.toml` says what any program may do there;
`quillo.policy` beside the program narrows it for the site (see [The
policy](#the-policy)). `quillo check --policy policy.toml main.qwe`, run before
uploading, says what would be refused.

**Threads and `ulimit -u`.** A Quillo server may use [`--workers`](#options-of-quillo-run) + 9 threads (73 with the
default 64), and on Linux the account's process limit counts them, beside PHP, cron and
SSH. Give `--workers 8` or so (a host's policy can cap it with `[limits] workers`). At
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
