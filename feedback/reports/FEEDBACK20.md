# Quillo for a SaaS, and web, desktop and phone from one program

| | |
|---|---|
| App | None: two questions, whether one codebase can start as `web.qwe`, `desktop.qwe` and `mobile.qwe`, and how well Quillo suits a SaaS, answered with small programs |
| Quillo | 0.6.0, the source of 0db133d on branch `feedback19-fixes`: the build predates that commit, so `quillo --version` and the index say 9e8c2fb. It was made after the last edit to `src/`, and src/httpd.rs and src/db.rs are the same in both |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` and `quillo run` on macOS, `curl` and Python clients setting `Host`, a throwaway PostgreSQL 17, 40 PostgreSQL tenants and 1,100 SQLite tenants. Not run: `quillo app`, a phone build, MySQL, or anything on Linux |

What worked well: tenancy is in the runtime, not in a library, and it held under hostile
input. Every `Host` header tried landed on the right tenant or on none, a session cookie
did not cross tenants, a made-up subdomain made no database, and bad tenant names were
refused before any file was touched. 1,100 SQLite tenants were made in 1.0 s. A transaction
held open while 39 other tenants pushed its database past the limit of open ones committed
whole. Three entry files over one set of routes and one database took 3 lines each.

The answer to both questions:

| Question | Answer |
|---|---|
| `web.qwe`, `desktop.qwe`, `mobile.qwe` over one codebase? | Yes on the web and the desktop, as a program is any `.qwe` file. A phone app starts from `main.qwe` only (Q-169), and a one-line `main.qwe` that says `use mobile.qwe` gets past that. The code is shared, not the data: each phone has its own SQLite |
| Is Quillo suited to a SaaS? | Its design is. It is not ready for customers who can affect each other: one request can stop a server of any size (Q-162, Q-163), and a database error goes to the visitor with its SQL (Q-161) |

Fix first, in this order: Q-161 (an information leak that needs no privileges), Q-162, Q-163,
Q-164, then Q-108 and Q-143. The rest can wait for a release.

## What was fixed

Branch `feedback20-fixes`: Q-161 to Q-168, and from the earlier lists Q-108, Q-110, Q-117, Q-122,
Q-134, Q-143, Q-159 and Q-160. Two choices differ from the suggestions below:

- **Q-167 answers 404 only where a route needs the database.** A check before every route would
  also stop `www.example.com`, which names a tenant with no database and may be a page of its own.
- **Q-168 keeps connections per tenant.** One pool with `search_path` set on checkout would be
  faster, but connections keep prepared statements by their SQL, so a statement prepared for one
  tenant could run against another's tables. `?open_tenants=N` is the option suggested beside it.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-108 | A failed commit stays open: run again here, still open on 9e8c2fb. A signup or a billing write in a transaction is what it would hit | 1 |
| Q-143 | A migration that fails at its commit stays open: read in the index, not run again. Tenants migrate one at a time, on first use, so it can stay hidden for a long time | 0 |
| Q-159 | Read in the index, not run again. It is narrower than its title: `tests/cover_data.rs` shows a panic when the first query opens `$QUILLO_DB` while another request calls `db.open("t/{tenant}.db")`. A program that opens its database at the top level does not take that path | 0 |

## New issues

### Q-161. A route that gives a database's error sends the visitor its SQL

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a request for a tenant that does not exist, then a bad query | 1 |

A route whose last value is an error answers `500 {"error": ...}`, and the guide's table
under "What a route returns" says so. What it does not say is that `db.*` errors are such
values, so their text goes to whoever made the request: the SQL, the table, the column.
This is on a plain `quillo run`, not under `--watch`. `?` does the same. A runtime error
beside it says only `internal error`, so the two look alike in code and differ on the wire.

The operator gets less than the visitor: the log has `GET /users -> 500 (0.1ms)` and no text.

```quillo
db.open(":memory:")
db.exec("create table if not exists users (id integer primary key, email text)")

route GET /users = db.query("select nope from users")
```

| Route | Answer |
|---|---|
| `db.query("select nope from users")` | `{"error":"db: no such column: nope in select nope from users at offset 7"}` |
| `db.exec` breaking a unique column | `{"error":"db: UNIQUE constraint failed: users.email"}` |
| the same query, with `?` | the same text |
| the same query, then `rows[0].email` | `internal error` |

**Suggestion:** tell the two kinds of error apart. `err(x)` written by the program is meant to
be read and stays as it is. An error that comes out of a builtin answers `internal error`,
and its text goes to the log with the request's id. Say so in the guide, and in SECURITY.md.

### Q-162. The first instance accepts for all of them, and connections go out in turn whatever is busy

| Kind | Detected by | Attempts |
|---|---|---|
| silent | timing a ping beside a slow request, then reading src/httpd.rs | 3 |

`--instances N` is the guide's answer to long computing ("put it in Rust, or run more
instances"). It helps less than that says, for two reasons:

- The listener is registered on the first instance's loop alone (src/httpd.rs:430). While
  that instance computes, nothing accepts, so every new connection waits, however many
  instances there are.
- Each connection goes to the next instance in turn (src/httpd.rs:647), busy or not, and
  stays there. So a stuck instance past the first still takes 1 in N of the new connections.

With `--instances 4` and `/forever` sent after a number of other connections, then 8 pings
with a 1 s limit:

| Connections before it | Instance it lands on | Pings answered |
|---|---|---|
| 0 | the first | 0 of 8 |
| 1 | second | 6 of 8 |
| 2 | third | 6 of 8 |
| 3 | fourth | 6 of 8 |
| 4 | the first | 0 of 8 |

So 1 request in N can stop the whole server, and a request that merely takes a while does
the same for as long as it takes: at `--instances 2`, a ping beside a 3.8 s loop waited 3.7 s
with an idle instance beside it. On one instance that is expected; on two it is not.

```quillo
route GET /ping = "pong"
route GET /forever {
    let i = 0
    while true { i = i + 1 }
    "never"
}
```

```sh
quillo run main.qwe --port 8080 --instances 2 &
curl 127.0.0.1:8080/forever &      # the first connection: the first instance
curl -m 1 127.0.0.1:8080/ping      # never answered, nor is any after it
```

Measured on macOS. Nothing in this code differs by system, but it was not run on Linux.

**Suggestion:** accept on a thread that runs no program, so accepting never waits for
computing. Then skip an instance that has not come round its loop for a while, which the
watchdog in Q-163 would already know. The guide's advice needs a note until then.

### Q-163. Nothing limits how long a request computes

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a loop that never ends, and reading the guide | 1 |

The guide says so ("A time limit on a route: none"). For a SaaS it is the largest gap, since
it makes one customer's request every customer's outage. A `while` that never ends holds its
instance until the process is killed, with no line in the log. `quillo check` passes it. With
Q-162, that is the whole server.

```quillo
route GET /forever {
    let i = 0
    while true { i = i + 1 }
    "never"
}
```

**Suggestion:** the plan already made, in its order: a watchdog that logs the request and
line of an instance stuck for about 5 s, then `--max-compute <duration>`, checked where
`run_script`'s fuel is and ending the request as a 500 at a statement boundary, then a
`quillo check` warning on a `while` that cannot end. No preemption: it would break what the
guide promises about shared state. The repro expects `--max-compute 1s` to answer 500.

### Q-164. A request that computes outlives SIGTERM

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a `kill` in a probe that did not end it | 2 |

The guide says the first SIGTERM answers the requests in progress "for up to 10 seconds" and
exits with status 0. With a request computing, it was still running 25 s later. A second
SIGTERM stopped it at once. Q-081 fixed this for a job, not for a request.

The cost is on a deploy: a service manager sends one signal and waits out its own timeout,
and under `--instances` with the first instance stuck, new requests have already stopped.

**Suggestion:** a thread of its own counts the 10 seconds and exits, as nothing a stuck loop
holds matters by then. Log the request it gave up on.

### Q-165. The guide's signup writes into a tenant that already exists

| Kind | Detected by | Attempts |
|---|---|---|
| docs | running the sample twice with one name | 1 |

`with_tenant(name, ...)` makes a tenant or enters it, and the sample under "Many tenants"
calls it with a name from a form, with nothing in between. A second signup for `acme` is
told `made acme` and adds a row to acme's database. The sample writes a harmless row, but it
is the pattern people will copy, and the code in that function runs as the tenant.

```quillo
route POST /signup { form: { name: str } } {
    with_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))
    redirect(`https://${name}.example.com/`)
}
```

`acme holds: [{"plan":"free"},{"plan":"free"}]`

The names `www`, `public` and the like were accepted too, which is the program's to refuse,
and the guide could say so.

**Suggestion:** check `db.tenant_exists(name)` in the sample and `reply 409`. That leaves a
race between two signups, so a call that makes a tenant and fails when it exists would be
better than a check beside `with_tenant`.

### Q-166. A global a route writes crosses tenants, without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent | writing one by hand, on the guide's warning | 1 |

```quillo
tenant_from("example.com")

let last_seen = "nobody"

route GET /who = tenant()
route GET /peek {
    let was = last_seen
    last_seen = str(tenant())
    was
}
```

`acme.example.com/peek` gives `nobody`, then `beta.example.com/peek` gives `acme`. The guide
says that globals are shared, at the end of "Many tenants". It is correct, and it is the one
way found here for one customer's data to reach another with no mistake in the SQL.
`quillo check` passes it. A cache in a global is the usual way to write this by accident.

**Suggestion:** in a program with `tenant_from`, warn on assigning a global from a route or
a function: one copy for all tenants, keep it in the database or a map keyed by `tenant()`.

### Q-167. A made-up subdomain is a 500 that names `with_tenant`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `ghost.example.com` | 1 |

The database is safe: nothing is made. But the visitor gets a 500 where a site with no such
customer would say 404, and the text is meant for the program's author:

`{"error":"db: there is no tenant \"ghost\": with_tenant(\"ghost\", ...) makes its database"}`

So every scanner trying subdomains counts as a server error, and the status says which names
are customers. A route that does not touch the database answers 200: `tenant()` gives
`ghost`, as nothing says it is unknown. This shares a cause with Q-161.

**Suggestion:** with a `{tenant}` target, a request for a tenant with no database answers 404
before the route runs, unless the program asks otherwise for a page of its own.

### Q-168. Past 16 tenants at once, each PostgreSQL request connects again

| Kind | Detected by | Attempts |
|---|---|---|
| perf | round-robin requests over 40 tenants | 2 |

The guide gives the limits. What it does not give is what they cost, and a SaaS runs past 16
active customers early. `OPEN_PG_TENANTS` is a constant (src/db.rs:300), and it applies to
schemas in one database too, where connections could be shared. Requests one after another,
median of two rounds after a first:

| Tenants in turn | PostgreSQL schemas | | Tenants in turn | SQLite files |
|---|---|---|---|---|
| 16 | 0.2 ms | | 1,000 | 0.06 ms (worst 0.1) |
| 17 | 3.0 ms | | 1,100 | 0.41 ms (worst 45.7) |
| 40 | 3.0 ms (worst 8.9) | | | |

That is 15 times over one more tenant, on loopback and without TLS; to a managed database
each is a TLS connection. Nothing failed, and connections stayed bounded (33 at `pool_size=2`).

```quillo
tenant_from("example.com")
db.open("postgres://q@127.0.0.1:18533/app?schema=t_{tenant}&pool_size=2")
db.migrate()
route POST /make/:name {
    with_tenant(name, () => db.exec("insert into t (v) values ('seed')"))
    "made"
}
route GET /read = db.query("select count(*) as n from t")
```

The two limits pull apart. SQLite takes the most tenants but ties them to one server, as
several servers share a database; PostgreSQL scales out and keeps 16. No repro file: it needs
a PostgreSQL, and the program above is all of it.

**Suggestion:** for `?schema=`, one pool for the program, with `search_path` set on checkout,
so that connections do not grow with tenants. For the rest, an option in place of the constant.

### Q-169. A phone app can only start from `main.qwe`

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the launchers and `mobile/app-config.sh` | 1 |

`quillo run` and `quillo app` take any file. `app.toml` has no key for the entry, Android
passes the name as a literal (mobile/android/.../MainActivity.kt:235), and iOS reads it from
an Info.plist that no build changes (mobile/ios/Quillo/Info.plist:18).

A program made of `web.qwe`, `desktop.qwe` and `mobile.qwe` needs a `main.qwe` saying `use
mobile.qwe`, which works: `main.qwe` served the mobile layout under `quillo run`. No phone
build was made to confirm it.

A folder for each platform does not get past this. `apps/web.qwe` may say `use ../lib/api.qwe`,
but a phone build bundles only the program's folder and skips subfolders with a `main.qwe`.

**Suggestion:** `main = "mobile.qwe"` in `app.toml`, read by `app-config.sh`, written into the
plist on iOS and passed to `Quillo.start` on Android. `src/mobile.rs` takes the name already.

### Q-170. `${name}` in markup prints a stray `$`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | looking at a probe's output | 1 |

```quillo
let name = "web"

route GET / = <p>${name}</p>
```

The page says `$web`, and `quillo check` says ok. Markup takes `{name}` and templates take
`${name}`, so the `$` is plain text. Anyone coming from a template string writes this, and it
was the first thing the model got wrong here.

**Suggestion:** warn on a `$` right before `{` in markup. A price is written `${price}` on
purpose, so a warning, with `\$` or `{"$"}` to say it is meant, and not an error.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `<p>${name}</p>` | a template string's habit | `$web` on the page, no warning (Q-170) | 1 run |
| `db.open(url, { pool_size: 2 })` | the guide's prose says `pool_size` | `quillo check` passed, `quillo run` refused it: it is `?pool_size=2` in the URL | 1 run |
| `session().user = "ada"` | a map, as in JavaScript | `quillo check` refused it with a clear message; `set_session` is the way | 1 check |
| Said a stuck request costs "1 in N" | measured after a health check had taken the first turn | a repro with no connection before it hung 10 of 10, which led to src/httpd.rs and Q-162 | 2 probes |
| Said Q-159 panics "when tenant databases open at the same time" | read the title and not the test | too broad: see Seen again | a correction |
| Expected evicting a tenant to break a transaction | guessed from "closed first" | it does not: the transaction committed whole | 1 probe |
| Expected `crypto.verify_password` to hold the loop | Argon2id is slow on purpose | a ping beside 8 logins took 0.5 ms; it may not be on the loop at all | 1 probe |

## Limitations

For a SaaS, by how soon each is met. All are read from the guide or the catalog; none is a
bug, and `rate_limit` was removed on purpose.

- **Rate limiting:** none. The guide's way is a table of your own. Behind a proxy `request.ip`
  is the proxy, so a count per visitor reads `x-forwarded-for` by hand.
- **No audit log:** the guide does not mention one. A program writes its own.
- **Removing or exporting a tenant:** `db.tenants` and `db.tenant_exists` are all there is, so
  a customer who leaves is a file or a schema dropped by hand. There is no per-tenant backup
  either; `vacuum into` inside `with_tenant` looks like the way, and was not tried.
- **Enterprise sign-on:** the catalog has `oauth_pkce`, `totp` and `jwt`, and no SAML plugin.
- **Billing:** the Stripe plugin and its sample cover one-off checkout, a PaymentIntent and the
  webhook. Neither mentions subscriptions, which is what a SaaS bills by.
- **SQLite at rest:** no encryption; the disk's, or a server database.
- **Upgrades:** a new binary is a stop and a start, so no gap takes two servers behind limer.
- **Telling devices apart:** `platform()` describes where the program runs, not the visitor. On
  the web it says `server`, so a phone's browser and a desktop's get the same answer. That is
  the page's CSS, or the `user-agent`.
- **One entry for all three:** branching on `platform().runtime` passes `quillo check`, and only
  its web branch was run. `quillo app desktop.qwe` was not run either.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `Host: ACME.example.com` | tenant `acme` |
| `Host: acme.example.com:8080` | tenant `acme` |
| `Host: a.b.example.com` | no tenant |
| `Host: acme.example.com.evil.com` | no tenant |
| `Host: evil.com` | no tenant |
| `Host: ..%2f.example.com`, `acme%2f..example.com` | no tenant |
| Tenant named `../evil`, `a/b`, `ACME` | refused, no file made; the visitor gets `internal error` and the log says why |
| Request for an unknown tenant | no file or schema made |
| Session cookie from `acme` sent to `beta` | `ada on acme`, `nobody on beta` |
| Session cookie's name | `__Host-` prefix, so a sibling subdomain cannot plant one |
| Transaction open 1.5 s while 39 other tenants passed the limit twice | committed whole, no errors beside it |
| 40 PostgreSQL tenants at `pool_size=2` | no failures, 33 connections |
| 1,100 SQLite tenants made through a route | 1.0 s, 49 MB |
| Three entry files over one set of routes | each serves its own layout and the shared routes |
| Three entry files over one `db.open("shared.db")` | one database, as paths count from the entry's folder |
| A second SIGTERM with a request stuck | stops at once |
| 8 logins at once on one instance | a ping beside them took 0.5 ms |
