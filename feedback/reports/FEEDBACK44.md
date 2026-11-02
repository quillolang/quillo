# Feedback from probing what one transaction lets another see: SQLite's one connection, a hand-written `begin`, and Postgres sessions that outlive their request

| | |
|---|---|
| App | None: probe programs aimed at what crosses from one request's transaction into another's. They cover SQLite's single connection while a transaction waits, transactions inside transactions of other requests, `begin` and `commit` written by hand, session settings and temporary tables on PostgreSQL's pooled and shared connections, the same on MariaDB, `last_id`, a transaction stopped by `--max-compute`, and the code paths that pick a connection (replicas, `exec_many`, `idempotent`) |
| Quillo | 0.7.0-dev, release build of the runtime-coverage branch's working tree (it reports `54dd1bd`; the binary was built at 07:28 on 2026-11-03, after the last source change). The branch's uncommitted work in `db.rs`, `httpd.rs`, `pg.rs` and `interp.rs` does not touch the functions named below (`Interp::unlocked`, `await_elsewhere`, `shared_lanes`, `checkout_for_tx`, `attempt`, `Pool::give_back`, `Lanes`): they are as committed in `54dd1bd` |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` and `quillo run` with `curl`, in the session's scratch folder, against a throwaway PostgreSQL 17 (port 55444) and MariaDB 12.3 (port 55445) started there (not the PostgreSQL already running on 5432), and `sqlite3` and `psql` reading the databases from outside. No browser run. Read: [src/db.rs](../../src/db.rs) (`attempt`, `caught`, `checkout_for_tx`, `pooled_run`, `shared_lanes`, `exec_on`, `select`, `reader`, `exec_many`), [src/interp.rs](../../src/interp.rs) (`unlocked`, `parallel`), [src/httpd.rs](../../src/httpd.rs) (`wait_elsewhere`, `await_elsewhere`), [src/txn.rs](../../src/txn.rs), [src/pg.rs](../../src/pg.rs) (`Pool`, `give_back`, `Lanes`), [src/my.rs](../../src/my.rs), [src/crypto.rs](../../src/crypto.rs) (the hashing wait), [src/idempotency.rs](../../src/idempotency.rs), [src/jobs.rs](../../src/jobs.rs), USERGUIDE.md (Databases, Isolation and retries, Live data, Idempotency) |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section, and the scripts are at the end. FEEDBACK47.md (routes and groups) was read first for its shape and its issues: none of them is about the database, so nothing here repeats it. An earlier FEEDBACK44 is kept as `SKIP_FEEDBACK44.md`, with Q-498 to Q-502. Numbers here start at Q-579, after FEEDBACK48.md's Q-578 (FEEDBACK48 left Q-547 to Q-559 for FEEDBACK47).

What the guide promises holds for rows, in most places: a Postgres transaction keeps its connection, a rolled-back transaction announces nothing, replicas are never read inside a transaction, and `sleep` or `http` inside a SQLite transaction hold the other requests up. The three issues below are where it does not hold. `quillo check` says `ok` to every program in them.

| ID | In short | Kind |
|---|---|---|
| Q-579 | Inside a SQLite transaction, a query to a Postgres database lets other requests run on SQLite's one connection: they read what it has not committed, and their writes, their own committed transactions included, are rolled back with it | silent, data, security |
| Q-580 | A `db.exec("begin")` whose `commit` is never reached leaves the shared connection in a transaction: on SQLite every later write by every request goes into it and is lost at stop; on PostgreSQL one request in four; on MySQL `begin` does nothing at all | silent, data |
| Q-581 | PostgreSQL session state crosses requests: a `set`, a `set_config(..., false)` or a temporary table made in one request is there in the next one on that connection, in a transaction (the pool) and outside one (the shared lanes) | silent, security |

## New issues

### Q-579. Inside a SQLite transaction, a query to a Postgres database lets other requests run on SQLite's one connection

| Kind | Detected by | Attempts |
|---|---|---|
| silent, data, security | a second request reading during the first one's transaction; the table read after the first rolled back; `sqlite3` from outside | 3 programs, and `sleep` in place of the query as a contrast |

```quillo
db.open("app.db")
db.exec("create table if not exists t (id integer primary key, v text)")
let pg = db.connect("postgres://quillo@127.0.0.1:55444/app")

route POST /a = db.transaction(() => {
    db.exec("insert into t (v) values ('A: not committed')")
    pg.query("select pg_sleep(1)")              // a stock check, a report: any Postgres query
    err("A gives up")
})
route GET /b = json.stringify(db.query("select v from t"))
route POST /c = {
    db.exec("insert into t (v) values ('C: written outside any transaction')")
    "C ok"
}
```

```
$ curl -s -X POST localhost:47401/a &          # A starts, and waits on Postgres
$ curl -s localhost:47401/b                    # 0.3 s later
[{"v":"A: not committed"}]                     200   (A's row, never committed)
$ curl -s -X POST localhost:47401/c
C ok                                           200
{"error":"A gives up"}                         500   (A ends and rolls back)
$ curl -s localhost:47401/b
[]                                             200   (C's row is gone too)
```

B read a row that was never committed. C wrote outside any transaction, was answered 200, and its row was rolled back with A's. A transaction of the second request's own fares no better:

```quillo
route POST /b = db.transaction(() => {
    db.exec("insert into orders (who) values ('B')")?
    "B committed"
})
route POST /d = db.transaction({ isolation: "serializable" }, () => {
    db.exec("insert into orders (who) values ('D')")?
    "D committed"
})
```

```
POST /b      B committed      200    (while A waits)
POST /d      internal error   500    log: a transaction inside another cannot change its isolation: give it to the outermost one
POST /a      out of stock     500
GET /orders  []                      (B's committed order is gone)
```

B's transaction began while SQLite was already inside A's savepoint, so it became a savepoint inside A's ([db.rs:1994](../../src/db.rs#L1994): `outermost` is read from the connection, which every request shares). Its `release` committed nothing. D was refused because of a transaction it knows nothing about. Two requests' savepoints are both named `quillo_tx`, so when they end in another order, one `release` ends the other's.

With `sleep(1000)` in place of the Postgres query, B waits 0.7 s, sees nothing of A's, and C's row stays. That is the guard the guide describes ("Inside `db.transaction` on SQLite, `sleep` and `http` calls hold up other requests instead of letting them write into the open transaction"). It lives in `Interp::unlocked` ([interp.rs:1481-1487](../../src/interp.rs#L1481-L1487)): while `sqlite_tx > 0`, the wait is done in place. Password hashing repeats the same check by hand ([crypto.rs:198](../../src/crypto.rs#L198)). But a Postgres query outside a Postgres transaction does not go through `unlocked`. In the server it goes to the shared lanes, through `httpd::await_elsewhere` ([db.rs:1287-1291](../../src/db.rs#L1287-L1291) for `exec`, [db.rs:1908-1909](../../src/db.rs#L1908-L1909) for `query` and `one`), and `await_elsewhere` pauses the request's coroutine with no look at `sqlite_tx` ([httpd.rs:529](../../src/httpd.rs#L529)). The server then runs other requests, on the same SQLite connection. The same call is made while a Postgres transaction waits for a free connection ([db.rs:666](../../src/db.rs#L666)). That path was read, not run.

The two databases need not be the program's: `db` on Postgres and an `analytics` on SQLite are the same case the other way round. Jobs and ticks reach the server as requests ([jobs.rs:285](../../src/jobs.rs#L285)), so a job's writes land in the open transaction too. A live page that loads while A waits is shown A's rows, and when A rolls back nothing is announced (rightly, for A), so it keeps showing them until the next change. That last point follows from the code and B's read. It was not run in a browser.

Who writes this: a program whose main database is SQLite and which reads a second one (a shop's stock, a company's Postgres) inside an order's transaction. The guide's own "Several databases" example puts an `analytics.transaction` inside a `db.transaction`.

**Suggestion:** have `shared_lanes` give `None` while `it.sqlite_tx > 0`, so the query goes through `pooled_run` and `unlocked`, which waits in place. Do the same for the pause in `checkout_for_tx`. Better, put the check in `await_elsewhere` itself (it would take the count, or read a thread-local set with `sqlite_tx`), so that the next caller cannot miss it as these two did. The guide's line should then name database calls on other connections beside `sleep` and `http`. A test: the repro above, with `/b` answering only after `/a` has ended, and `[]` from both reads.

### Q-580. A `db.exec("begin")` whose `commit` is never reached leaves the shared connection in a transaction

| Kind | Detected by | Attempts |
|---|---|---|
| silent, data | later writes read back, read from another process, and read after a restart | SQLite, PostgreSQL, MariaDB |

```quillo
db.open("app.db")
db.exec("create table if not exists t (id integer primary key, v text)")

// written the way Python's sqlite3 code is: begin ... commit by hand
route POST /import = {
    db.exec("begin")
    db.exec("insert into t (v) values ('import: row 1')")?
    db.exec("insert into missing (v) values ('import: row 2')")?    // fails: the route ends here
    db.exec("commit")
    "imported"
}
route POST /c = {
    db.exec("insert into t (v) values ('C: a later order')")?
    "C ok"
}
route GET /b = json.stringify(db.query("select v from t"))
```

```
$ quillo check main.qwe
main.qwe: ok (3 routes, 0 functions, 0 components)
POST /import    internal error                                    500
POST /c         C ok                                              200
GET /b          [{"v":"import: row 1"},{"v":"C: a later order"}]  200
$ sqlite3 app.db "select v from t"                                (nothing)
# the server is stopped
$ sqlite3 app.db "select v from t"                                (nothing)
```

The import's `begin` was never ended, and SQLite has one connection for every request. So every write after it, by anyone, went into that transaction. The program reads its own writes back and answers 200, while no other process sees them. Everything is lost when the server stops. Nothing is logged after the import's own error. Only `db.transaction` raises `sqlite_tx`, so Q-579's guard does not apply either: a `sleep` between the hand-written `begin` and `commit` lets every other request in.

On PostgreSQL the `begin` goes to one of the shared lanes, and the lane stays `idle in transaction` (`pg_stat_activity`). The route was a validation that left early (`if n > 10 { reply 400 "too many rows" }`) before its `commit`:

```
POST /import/99    too many rows    400
POST /c/1 ... /c/8 C ok             200 each
$ psql -c "select string_agg(v, ', ' order by id) from t"
C1, C2, C3, C5, C6, C7                      (C4 and C8, on the lane with the open transaction, are lost)
```

On MariaDB each statement takes a pooled connection of its own, which is reset when it goes back, so `begin` does nothing. The 400 left `import: row 1` committed: the code reads as a transaction and is not one.

So the same five lines lose other requests' data on SQLite, lose one request in four on PostgreSQL, and are quietly not atomic on MySQL. The guide never says not to write `begin` (it says so only for migration files). A developer coming from Python's `sqlite3`, PHP's PDO or Go's `database/sql` without `Tx` may well write it, and an AI translating such code will.

**Suggestion:** refuse transaction control in `db.exec`, `db.query` and `db.one`. A statement that begins with `begin`, `start transaction`, `commit`, `end`, `rollback`, `savepoint` or `release` is an error: "`db.exec(\"begin\")`: write `db.transaction(() => ...)`, which commits, rolls back when something fails, and keeps other requests out of it". A seed file run whole (`db.exec(read_file("seed.sql"))`, as `sqlite3 .dump` writes them with `BEGIN TRANSACTION; ... COMMIT;`) can stay allowed when it leaves the connection as it found it. On SQLite that is `is_autocommit()` after the call: when it is not, roll back and give an error value. As a safety net for SQLite, at the end of each request, a connection left out of autocommit with no `db.transaction` open is rolled back, and the log says which request left it so. `quillo check` can warn on the literal text.

### Q-581. PostgreSQL session state crosses requests: `set`, `set_config(..., false)` and temporary tables outlive their request

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | another request's `current_setting(...)` and temporary table read back | a pool of 1, the default pool, the shared lanes, and the same on MariaDB |

```quillo
db.open("postgres://quillo@127.0.0.1:55444/app")

// alice's request: a setting for row-level security, and a scratch table for her cart
route POST /alice = db.transaction(() => {
    db.exec("set app.user_id = 'alice'")
    db.exec("create temp table if not exists cart (item text)")
    db.exec("insert into cart values ('alice: engagement ring')")
    "alice ok"
})
// bob's request, later: a transaction of his own
route GET /bob = db.transaction(() => {
    db.exec("create temp table if not exists cart (item text)")
    json.stringify({
        user: db.one("select current_setting('app.user_id', true) as u").u,
        cart: db.query("select item from cart"),
    })
})
```

```
POST /alice    alice ok                                                   200
GET /bob       {"user":"alice","cart":[{"item":"alice: engagement ring"}]} 200
```

That was with `?pool_size=1`. With the default pool of 8, bob read alice's cart on 3 requests out of 3: the pool hands out the connection given back last ([pg.rs:139-142](../../src/pg.rs#L139-L142)). `give_back` puts a connection back as it is, and resets nothing ([pg.rs:208](../../src/pg.rs#L208)).

Outside a transaction it is worse, since the lanes are shared by requests running at the same time ([pg.rs:670](../../src/pg.rs#L670)):

```quillo
route POST /as/:who = {
    db.one("select set_config('app.user_id', ?, false)", who)     // row-level security's usual first line
    "set"
}
route GET /whoami = db.one("select current_setting('app.user_id', true) as u, pg_backend_pid() as pid") | json.stringify
```

```
POST /as/alice    set
GET /whoami       {"u":null,"pid":48871}
GET /whoami       {"u":null,"pid":48873}
GET /whoami       {"u":null,"pid":48866}
GET /whoami       {"u":"alice","pid":48869}      (the lane alice's request used)
GET /whoami       {"u":null,"pid":48871}
GET /whoami       {"u":null,"pid":48873}
```

One request in four runs as alice. Under load, two requests that each set their own user before reading can interleave on one lane, and each read as the other.

This matters most for row-level security. Its common pattern is a setting per request (`set_config('app.user_id', ...)` or `set role`) and policies that read it with `current_setting`. The setting leaks to whoever gets the connection next, and the policies then show them alice's rows. A `set search_path` for a schema per tenant leaks in the same way. So do `set statement_timeout`, a session's advisory locks (`pg_advisory_lock`), `listen`, and a cursor declared `with hold`.

The guide says "A transaction keeps one connection until it ends, so other requests never see its changes early". That is true of rows and of `set local`, `set_config(..., true)` and `on commit drop`, which all stay inside their transaction (tried, see the end). It is not true of the session. MariaDB does not have the problem: the same program there gave bob `{"user":null,"cart":[]}` three times, because mysql_async resets each connection as it goes back to its pool (`reset_connection`, on by default in 0.37).

**Suggestion:** two parts.

- After a transaction, reset the connection before it goes back. `discard all` would also drop Quillo's prepared statements, so either clear the connection's statement cache with it, or send the parts that keep them: `reset all; discard temp; select pg_advisory_unlock_all(); unlisten *`. Sent in one batch with the `commit` it costs no extra round trip (not measured). Or send it only after a transaction that ran a statement that is not `select`/`insert`/`update`/`delete`/`with`, as `only_reads` already reads the first word.
- On the shared lanes, refuse what changes the session: `set` without `local`, `set_config(..., false)`, `create temp`, `listen`, `pg_advisory_lock`, `declare ... with hold`. The message: "outside `db.transaction` this connection is shared by every request: use `set local` (or `set_config(..., true)`) inside `db.transaction`".

The guide's PostgreSQL section could show row-level security the way that works: `db.transaction(() => { db.one("select set_config('app.user_id', ?, true)", id); ... })`.

## Smaller notes

- **SQLite's `last_id` can be another request's row, in another table.** One request's upsert that took the update path (`insert ... on conflict (id) do update ...`) answered `{"changes":1,"last_id":2}`, where 2 was another visitor's new id in `users`. The guide does say that after an `update` it is "an old one", but not that the old one is anyone's. Make it `null` when the statement inserted no row (`last_insert_rowid()` read before and after, under the same lock).
- **Q-579's guard is written twice and missed twice.** `unlocked` and the hashing in `crypto.rs` check `sqlite_tx`. `await_elsewhere`'s two database callers do not. A test that runs a SQLite transaction around each waiting builtin (Postgres, MySQL, `http`, `sleep`, hashing, `service` calls, `udp`) and checks that a second request waits would keep the next one honest.
- **`set local` leaves a trace.** After a transaction that ran `set local app.user_id = 'alice'`, the next transaction on the connection reads `current_setting('app.user_id', true)` as `""`, not `null`. That is PostgreSQL's way (the setting stays defined, empty), not a leak. But a policy written as `current_setting(...) is null` misreads it. The row-level security example in the guide could use `coalesce(nullif(current_setting('app.user_id', true), ''), ...)`.

## Questions for you

1. Q-579: the simple fix waits for the Postgres query in place, which holds every request of the instance meanwhile, as `http` inside a SQLite transaction does now. Is that acceptable, or should a SQLite transaction stop depending on the interpreter lock (a connection of its own per transaction, `begin immediate`, with WAL readers on others)?
2. Q-580: should `begin`/`commit` in `db.exec` be refused outright, or only warned about by `check`, with the end-of-request rollback as the safety net?
3. Q-581: reset every connection after every transaction (simplest, costs a little), or only after statements that change the session? And should a session-changing statement on the shared lanes be an error or a warning?

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `initdb` and `pg_ctl ... -k <scratch folder>`, and `mariadbd --socket=<scratch folder>/s` | keep everything in the session's folder | both refused: a Unix socket path is limited to 103 bytes, and the scratch path is 108; the sockets went to a short folder under `/tmp`, the data stayed | 2 runs |
| `set app.user = 'alice'` | the obvious name | `db: syntax error at or near "user" [42601]`, as `user` is reserved; the visitor saw `internal error`, the log the SQL error; renamed `app.user_id` | 1 run |
| taking `unlocked` for the only way a request lets go of the lock | its comment explains the SQLite case, and every `http`/`sleep` path goes through it | Q-579 was found by listing every `yielder.suspend(...)` in `httpd.rs` and then every caller of `await_elsewhere`, not by reading `unlocked` | reading only |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `sleep(1000)` inside a SQLite transaction, with a reader and a writer meanwhile | both wait (0.7 s), the reader sees nothing uncommitted, the writer's row stays |
| a SQLite transaction stopped by `--max-compute 500ms` (a `while true` after an insert) | 500; its insert rolled back; the next request's write committed, and `sqlite3` saw it |
| `set local`, `set_config('app.tenant', 'acme', true)`, `create temp table ... on commit drop` in a Postgres transaction, read by the next transaction on the same connection (`pool_size=1`) | none of the three is there |
| MariaDB: `set @user_id`, `create temporary table` in one request's transaction, read in three later ones | `{"user":null,"cart":[]}` each time |
| reads to a replica inside a transaction (read, not run) | `reader` gives none while the request holds a transaction on that database ([db.rs:255-262](../../src/db.rs#L255-L262)) |
| `db.exec_many` outside a transaction (read) | through `txn::run`, so a transaction of its own on its own connection, not a shared lane |
| `parallel(...)` inside any transaction (read) | its functions run one after another on the request's own connection ([interp.rs:1565-1573](../../src/interp.rs#L1565-L1573)) |
| idempotency keys (read) | the stored answer's id hashes the tenant, the mount point, the user and the request's method, path, query and body, so another user's key and body give another id ([idempotency.rs:204](../../src/idempotency.rs#L204)) |
| a Postgres error in a request (the reserved-word case above) | the visitor gets `internal error`, the SQL and SQLSTATE go to the log |

## Scripts

Each probe is a folder with a `main.qwe`, served by `run.sh` on its own port and stopped by its own PID (never `pkill`):

```bash
#!/bin/bash
# run.sh prog.qwe port: start quillo on the port, wait until it answers
Q=/Users/restiguay/dev/quillo-app1/target/release/quillo
dir=$(dirname "$1"); f=$(basename "$1")
cd "$dir"
"$Q" run "$f" --port "$2" > "$f.log" 2>&1 &
echo $! > "$f.pid"
for i in $(seq 1 80); do curl -s -o /dev/null "127.0.0.1:$2/_quillo/ready" && break; sleep 0.1; done
```

```bash
#!/bin/bash
# stop.sh prog.qwe...: stop what run.sh started
for p in "$@"; do kill "$(cat "$p.pid")" 2>/dev/null; done
```

The databases, as `tests/support/mod.rs` starts its own, with the sockets in a short folder (`$K`, made by `mktemp -d /tmp/f44s.XXXX`):

```bash
initdb -D pg -U quillo --auth=trust -E UTF8 --no-locale
pg_ctl -D pg -o "-p 55444 -k $K -c listen_addresses=127.0.0.1 -c fsync=off -c max_connections=80" -l pg/log -w start
psql postgres://quillo@127.0.0.1:55444/postgres -c "create database app"

mariadb-install-db --datadir=my/data --auth-root-authentication-method=normal --skip-test-db
mariadbd --no-defaults --datadir=my/data --port=55445 --bind-address=127.0.0.1 \
  --socket=$K/my.sock --pid-file=my/pid --log-error=my/err.log --skip-grant-tables &
mariadb -h127.0.0.1 -P55445 -uroot -e "create database app"
```

The overlapping requests (Q-579) were a background `curl` for the transaction, then the others 0.3 s later:

```bash
(curl -s -X POST 127.0.0.1:47401/a -w ' %{http_code}\n' > a.out &); sleep 0.3
curl -s 127.0.0.1:47401/b; curl -s -X POST 127.0.0.1:47401/c; sleep 1.2; cat a.out; curl -s 127.0.0.1:47401/b
```
