# Feedback from reviewing a proposal: database drivers as plugins, named databases in `with`, and switching databases under live pages

| | |
|---|---|
| App | None: a design review of a proposal. PostgreSQL and MySQL/MariaDB leave Quillo's core and become Rust or `.qwer` plugins. Databases get aliases (names), and the aliases can be named in `with` when a program uses several databases. Every driver keeps live updates, shared runs and sharing across servers. A program can switch a database without changing its components or what the page shows |
| Quillo | 0.6.0, release build of the runtime-coverage branch's working tree (it reports `75aad21`). `src/db.rs`, `pg.rs`, `my.rs`, `migrate.rs`, `sqlinfo.rs`, `livecache.rs`, `idempotency.rs` and `ast.rs` are the same as main (`ab9e600`). `native.rs`, `check.rs` and `builtins.rs` carry the branch's uncommitted FEEDBACK36 to 39 work, none of it in the parts read here |
| Model | Claude (Opus 5.5) |
| Tested with | Probes in the session's scratch folder: SQLite files, a scratch PostgreSQL 17 cluster (`initdb`, TCP only, port 54391), a scratch MariaDB 12.3 (`mariadb-install-db`, port 33391), two `.qwer` files built by `use`, Node 24's `WebSocket` on `/_quillo/live`, and Python 3's `sqlite3` as the baseline. Read: [src/db.rs](../../src/db.rs) (`Backend`, `Pinned`, `DbSlot`, `open`, `db_connect`, `announce_as`, `kind_of`, the `_quillo_*` upserts), [src/pg.rs](../../src/pg.rs) (`describe`), [src/my.rs](../../src/my.rs), [src/migrate.rs](../../src/migrate.rs), [src/native.rs](../../src/native.rs) (`glue`, `use_file`, `call`), [src/ast.rs](../../src/ast.rs) (`EFFECTS`, `builtin_effect`), [src/parser.rs](../../src/parser.rs) (`with`), [src/livecache.rs](../../src/livecache.rs), [src/idempotency.rs](../../src/idempotency.rs), [src/builtins.rs](../../src/builtins.rs) (`sql_name`), Cargo.toml, USERGUIDE.md (Effects, Live data, Database, Several databases, Quillo's own tables, Plugins from the catalog, Other stores) |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section, and the probe programs are at the end. Numbers start at Q-488, after FEEDBACK42.md's Q-487.

What works well today, and should be kept: live data does not depend on the driver. A statement's tables become topics in core (`crate::sqlinfo::tables_of`, `announce_as`), so SQLite, PostgreSQL and MySQL update pages the same way. The program's `db` uses topics with no driver in them (`db:notes`), so a component following `db:notes` already works whichever database is behind it. Replicas, shards, tenants, transactions held per request, the shared runs and the patches are all core code above three drivers. A connection held in a global is followed as `var:<name>`, so pointing the global at another database refreshes every page at once (probe P4). The proposal can build on all of this.

## The answer

The proposal is sound, and most of what it asks for already sits above the drivers. Four things stand in the way, in this order:

1. **The drivers are not separate yet, even in the source.** Dispatch is a closed enum (`Backend::{Sqlite, Postgres, Mysql}`, 62 references in `db.rs`). Each database's own SQL also lives outside its driver: the upserts of Quillo's own tables (`db.rs:1340`, `db.rs:1441`, `idempotency.rs:466`), the migration locks (`migrate.rs`), name quoting (`builtins.rs:2603`) and the English words `error_kind` looks for (`db.rs:341`). PostgreSQL cannot be left out of a build at all (Q-495). Step one is a driver interface in core with the three drivers behind it, and no change in behaviour.
2. **Today's plugin interface cannot carry a driver** (Q-497). A `.qwer` call is a JSON text in and a JSON text out, with no handle (a pool or a transaction's connection), no way back into Quillo, and its own copy of tokio. Decimals cross as text. 100,000 rows cost 87 to 102 ms through it. The built-in SQLite path takes 14 to 16 ms, and Python's `sqlite3` 50 ms. A driver plugin needs an interface of its own: typed rows, handles, an ABI version, and libraries built ahead of time for each platform.
3. **A database has no name the program and the checker share** (Q-496). A connection is a map in a variable, its topics are named by a hash of its URL (Q-491), and `with` knows one `db` for all databases (Q-488). Aliases fix all three if they are declarations: `database analytics = env("ANALYTICS_URL")`. Their topics then become `db@analytics:orders`, and `with db.analytics` limits a function to that database.
4. **Switching the database changes what components get, and pages are not told** (Q-490, Q-494). After `db.open` runs again, open pages keep the old database's rows until the next write, and for 2 s new live calls are answered from the old database. The same row also reads differently on the three databases: `"2026-11-02 15:00:00"` against `"2026-11-02T15:00:00"`, `9.9m` against `9.90m`, a JSON column as text against a map. "Without impacting the UI" needs one form for each value on every driver, and a switch that announces itself.

Six bugs turned up along the way. They are worth fixing before the redesign, whatever is decided about it:

- Q-488: `quillo check` misses a connection used in a `with` that lacks `db`.
- Q-489: a `.qwer` function needs no effect, so it runs inside any `with`.
- Q-490: `db.open` at run time leaves live pages on the old database.
- Q-491: one SQLite file reached by two spellings gets two sets of topics.
- Q-492: `sql_name` quotes for the program's database, so a MySQL connection beside SQLite gets every row wrong without an error.
- Q-493: `db.error_kind` is null on a PostgreSQL that answers in German.

Part A shows where the code stands. Part B is the driver interface. Part C covers aliases and `with`, Part D switching. The issues follow, then an order of work and the questions only you can answer.

## Part A: where each part of "a database" lives today

| Concern | Where | Driver-independent? |
|---|---|---|
| Choosing the driver from the target | `slot_for`, `is_server_url`, `is_mysql_url` (`db.rs`) | No: a fixed list of URL schemes; anything else is a SQLite path |
| Pool, checkout, a transaction's connection | `Backend::checkout`, `Pinned`, `TxConn` (`db.rs`), `pg::Pool`, `my::Pool` | The bookkeeping, yes; the pools are each driver's |
| PostgreSQL's pipelined shared connections per request | `pg.rs`, the async runtime (`httpd.rs:268`) | No: PostgreSQL only, on Quillo's tokio |
| Placeholders `?` and `:name` | `pg.rs`, `my.rs` (each translates) | Each driver |
| Values both ways | `from_sql` (`db.rs`), `pg.rs`, `my.rs` | Each driver, with different results (Q-494) |
| Error kinds | `kind_of` (`db.rs:341`): English words and MySQL's codes in the text | No (Q-493) |
| Conflicts and retries | `watch_conflicts` / `conflicted` per driver; `txn.rs` | Yes: by SQLSTATE and error number, not words |
| Isolation syntax | `transaction` (`db.rs`) | No: `begin isolation level` against `set transaction` against `begin immediate` |
| Migration lock | `migrate.rs`: `begin immediate`, `pg_advisory_lock(7171717171)`, `get_lock('quillo_migrations', 600)` | No |
| Quillo's own tables (`_quillo_jobs`, `_quillo_later_tenants`, `_quillo_idempotency`) | `db.rs:1340`, `db.rs:1441`, `idempotency.rs:466` | No: `on conflict` / `on duplicate key update` / `insert ignore`, picked by `is_mysql` |
| Quoting names | `sql_name` (`builtins.rs:2603`) | No, and it asks the wrong database (Q-492) |
| Tenants' databases | `TenantDbs` (`db.rs`); `create database` per driver | The cache, yes; making one, no |
| Live topics: reads and writes | `sqlinfo::tables_of`, `announce_as` | **Yes** |
| Shared runs, patches | `livecache.rs` | **Yes** |
| Sharing across servers | limer (`--register`), topics as text | **Yes**, while every server names the topic alike (Q-491) |
| Replicas, shards | `Replicas`, `only_reads`, `shard_of` (`db.rs`) | **Yes**, over any `Backend` |
| Effects | `EFFECTS` (`ast.rs:117`), one `db` bit in a `u8` | One `db` for every database (Q-488, Q-496) |
| Building it in | `mysql` Cargo feature; PostgreSQL always (`postgres`, `tokio-postgres`, `tokio-postgres-rustls`) | Q-495 |

What this table says for the proposal: the rows marked **Yes** are what "all drivers must support live update, sharing" depends on, and they are already core code. A driver plugin should get no say over them. It sees statements and returns rows; core reads the statement's tables, announces, shares and routes. A driver then cannot drop live updates, because it never handles them.

## Part B: a driver interface, then driver plugins

### B1. The interface (in core, no behaviour change)

A trait in a small crate of its own (`quillo-driver`), which core and every driver depend on:

```rust
pub trait Driver: Send + Sync {
    /// The URL schemes it opens: ["postgres", "postgresql"].
    fn schemes(&self) -> &'static [&'static str];
    fn open(&self, target: &Target, opts: &OpenOptions) -> Result<Box<dyn Pool>, DbError>;
    fn dialect(&self) -> &Dialect;
}

pub trait Pool: Send + Sync {
    fn checkout(&self, wait: Duration) -> Result<Box<dyn Conn>, DbError>;
    fn checkout_nested(&self) -> Result<Box<dyn Conn>, DbError>;
    fn create_tenant(&self, name: &str) -> Result<(), DbError>;   // create database / schema
    fn tenants(&self) -> Result<Vec<String>, DbError>;
}

pub trait Conn: Send {
    fn query(&mut self, sql: &str, params: &[Param], limit: Option<usize>) -> Result<Rows, DbError>;
    fn exec(&mut self, sql: &str, params: &[Param]) -> Result<Exec, DbError>;   // { changes, last_id }
    fn batch(&mut self, sql: &str) -> Result<(), DbError>;
    fn begin(&mut self, iso: Option<Isolation>) -> Result<(), DbError>;
    fn savepoint(&mut self, n: usize) -> Result<(), DbError>;   // and release / rollback to
    fn commit(&mut self) -> Result<(), DbError>;
    fn rollback(&mut self) -> Result<(), DbError>;
    fn migration_lock(&mut self) -> Result<(), DbError>;          // and unlock
}

pub struct DbError { pub message: String, pub kind: Option<ErrorKind>, pub retry: bool }
pub enum ErrorKind { Unique, ForeignKey, NotNull, Check, Conflict }

pub struct Dialect {
    pub quote: fn(&str) -> String,              // "x" or `x`
    pub upsert: fn(&Upsert) -> String,          // for _quillo_* tables
    pub insert_ignore: fn(&str, &[&str]) -> String,
    pub returning: bool,
}
```

- `DbError.kind` and `retry` come from the database's code (SQLSTATE, error number, SQLite's extended code), never from its words. `error_kind` and the retries then read the same field (fixes Q-493 by construction).
- `Rows` is columnar and typed (`Int`, `Float`, `Dec { digits, scale }`, `Text`, `Bool`, `Json`, `Timestamp { tz }`, `Date`, `Time`, `Null`). Core turns it into Quillo values in one place, so every driver gives the same forms (Part D, Q-494).
- Placeholder translation moves into core, which already skips quotes and comments for two drivers: a driver declares `$n` or `?`.
- PostgreSQL's pipelining keeps working only while that driver runs on Quillo's tokio. It must stay a crate linked into Quillo; a library loaded with `dlopen` would have its own runtime (B2).
- Non-SQL stores (Redis, MongoDB) get an optional `fn tables_of(&self, statement: &str) -> Option<ReadsWrites>`, so their reads and writes become topics too. Without it, core's SQL reader is used.

This step changes no behaviour. SQLite, PostgreSQL and MySQL become three implementations, each in its own crate in the workspace (`drivers/sqlite`, `drivers/pg`, `drivers/mysql`). Cargo features `pg` and `mysql` choose what is built in. The dialect SQL scattered through `db.rs`, `migrate.rs`, `idempotency.rs` and `builtins.rs` moves into the `Dialect`s. The existing tests (`tests/postgres.rs`, `tests/mysql.rs`, the edge files) become the contract every driver passes.

### B2. Plugins: "detached" in the source first, loaded at run time only if measured

"Detached" can mean two things, and they cost very differently:

| | Separate crates, linked in (recommended first) | Libraries loaded at run time (`quillo install pg`) |
|---|---|---|
| Release binary | Has SQLite, PG, MySQL, as today; smaller builds by feature (the phone's already leaves MySQL out) | SQLite only; PG and MySQL downloaded |
| Speed | As today (14 to 16 ms for 100,000 SQLite rows; PG pipelined) | Depends on the ABI: through today's JSON one, 87 to 102 ms, slower than Python's 50 ms (Q-497) |
| PG pipelining | Kept (Quillo's tokio) | Lost unless the ABI passes futures, which Rust has no stable form for |
| On a server | Nothing to build | A library per OS and CPU, matched to Quillo's ABI version, from the catalog; no cargo on the server |
| Third-party drivers (SQL Server, Oracle, DuckDB, ClickHouse) | A pull request to Quillo, or a fork | Yes: the reason to do it |
| Risk | None new | A driver's crash takes the process (no panic guard around a whole pool), an ABI mismatch, unsigned code with the database's password |

The recommendation: do B1 and ship it linked in. Offer runtime driver plugins only for drivers Quillo does not ship (SQL Server, Oracle, DuckDB), through an ABI made for them:

- **ABI v2 for drivers**, apart from today's function ABI: `quillo_driver_abi() -> u32`, a vtable of C functions over opaque handles (pool, connection), rows as one buffer (column types, then values packed, decimals as digits and scale), errors as `{code, kind, retry, message}`. Values are not JSON.
- **Blocking calls on Quillo's pool threads**, the interpreter lock released, as `.qwer` calls already are. No async across the boundary.
- **Prebuilt, hashed, versioned.** The catalog has one library per target (`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, ...) with its SHA-256, as `.qwer` files have now. `quillo install` refuses an ABI it does not speak. The deploy notes already require prebuilding one plugin (docsearch) for each release; drivers would be the same, for every program that uses one.
- **A `.qwer` driver** for someone writing their own: a header `//! driver: duckdb` and an `impl quillo_driver::Driver`, built by `use` as today. The glue then exports the vtable instead of `quillo_native_call`.
- **The bar before any of this ships**: `bench/vs-python` with a driver plugin at least as fast as the built-in driver it would replace, on 1, 100 and 100,000 rows. Coverage of the new code at 99% (the README's number), and each driver passing the shared contract tests.

## Part C: aliases, and naming them in `with`

### C1. Declaring an alias

A declaration, not a call, so the parser, the checker, the language server and `with` all know the names before the program runs:

```quillo
database main      = env("DATABASE_URL")                          // `db` is this one
database analytics = env("ANALYTICS_URL"), { replicas: [env("ANALYTICS_REPLICA")] }
database archive   = "archive.db"

server fn orders() with db.main = db.query("select * from orders")
server fn sales() with db.analytics = analytics.query("select day, sum(total) from sales group by day")
server fn both() with db = { orders: orders(), sales: sales() }   // `with db`: every database
```

- `db` is the alias named `main`, or the only one, or today's `$QUILLO_DB` / SQLite in memory when there is none. `db.open(...)` keeps working as "set `main`", so no program breaks.
- An alias is a name of the file, like a function, so `private database`, `import "data.qwe" use analytics` and mounts follow the module rules (Q-154 to Q-156). Each `.env` or deployment gives its own URL, and the program is the same everywhere.
- `db.connect(url)` stays for databases only known at run time (a tenant's registry). Its topics keep the hashed label, now said to be the fallback.
- An alias never reaches the browser, like a connection today. A component that names one is an error in `quillo check`, as `db.*` in a component already is.

### C2. Topics by alias

`db@analytics:orders`, with `db:orders` kept for `main` (no page following `db:` changes). The name is chosen in the program, is the same on every server, holds no secret, and does not change when the URL does. That fixes Q-491 for aliases: another spelling of the path, a rotated password, a new host or a different driver all keep the topics. Pages on two servers sharing live data through limer keep following each other's writes during a rolling change.

### C3. `with db.<alias>`

- The parser takes `db` or `db.<name>` after `with`. An unknown name is an error, with the nearest alias suggested (as `missing_export` does).
- The effect mask cannot stay a `u8` with one `db` bit (`ast.rs:118`). It becomes the seven effect bits plus a set of alias numbers, a `u64` for up to 64 aliases. The rule stays as it is: narrowed on the way in, never widened.
- The check applies to every way of reaching a database: `db.*` (the `main` alias), `analytics.*`, a connection passed as a parameter, and `db.transaction` on either. Today `quillo check` already misses the second and third ways (Q-488).
- A `.qwer` function declares its effects in its header (`//! effects: db.analytics, http`), and one that declares none needs none. Today it needs none, whatever it does (Q-489). A driver plugin's functions are `db.<its alias>` by construction.
- `quillo check` warns, as now for helpers, when a `with db.main` function calls one that reaches `analytics`.

## Part D: switching a database without the components knowing

What "switch" covers: a new URL for an alias (failover, a move to a bigger server, a password rotation), a new driver for it (SQLite in development, PostgreSQL in production; MySQL to PostgreSQL in a migration), or, for a tenant, another database.

### D1. The switch itself

`db.switch("analytics", url)` in the program, or a changed `ANALYTICS_URL` taken up by `--reload`. Either does the same thing in order:

1. Opens the new database, and runs its migrations if the alias has any. A failure leaves the old one in place and gives an error value: no half switch, unlike today's `db.open` (the guide's "a failed `db.open` ... runs the app on an empty database").
2. Sends new statements to the new database. Transactions already open finish on the old one; `TxConn` holds its `Arc` already.
3. Drops the alias's shared runs and patches in `livecache` (Q-490: today they answer from the old database for 2 s).
4. Announces `db@analytics:*`, a topic every `db@analytics:<table>` follower hears. Every page showing that database asks again, on every server, through limer as any topic does.
5. Closes the old pool when its last transaction ends.

Components need no change: they follow the alias's topics, and those keep their names.

### D2. The same values from every driver

The switch is only invisible to the page if a row reads the same on every driver. Today it does not. Probe P8 wrote one row (`timestamp`, `boolean`, `decimal(10,2)`, `json` given a map) to each database and read it back:

| Column | SQLite | PostgreSQL 17 | MariaDB 12.3 | Proposed, on every driver |
|---|---|---|---|---|
| `at timestamp` | `"2026-11-02 15:00:00"` | `"2026-11-02T15:00:00"` | `"2026-11-02 15:00:00"` | `"2026-11-02T15:00:00"` (ISO 8601; `Z` when the type has a zone) |
| `paid boolean` | `true` | `true` | `true` | `true` |
| `price decimal(10,2)` | `9.9m` | `9.90m` | `9.90m` | `9.90m`: the declared scale, on SQLite too |
| `meta json` | `"{\"tags\":[\"a\"]}"` (text) | `{ tags: ["a"] }` | `"{\"tags\":[\"a\"]}"` (text) | `{ tags: ["a"] }`: by the declared type on SQLite; on MariaDB where the server says the column is JSON |
| `exec(...).last_id` (id given) | `1` | `null` | `0` | One rule: the generated id, or `null` |

The same insert written the SQLite way (`json.stringify(meta)`) stores a JSON *string* on PostgreSQL (`json_typeof` gives `string`). A program moved from SQLite to PostgreSQL then keeps running and reads back text where it expected a map. Passing the map works on all three; the guide should say so.

The common forms are a contract of the driver interface (B1's typed `Rows`), checked by the shared tests. They change what programs see today, so they belong in 0.7 with the change listed. `database x = ..., { raw: true }` keeps today's forms for a program that depends on them.

### D3. What a switch cannot hide

- SQL that only one database speaks (`returning` on old MySQL, `ilike`, `$1`, backticks). Aliases do not translate SQL. `quillo check` could warn about a statement the alias's driver does not take, when the driver is known.
- Data: a switch is to a database that already has it. Copying between them is a separate tool (`quillo db copy analytics new_url`), if wanted.

## Seen again

- FEEDBACK5.md, Q-053 (`db.error_kind`): the kinds were made to be the same on every database, and are, on English-speaking servers (Q-493).
- The guide's "A failed `db.open` does not stop the program" describes the start; at run time the same call leaves live pages behind (Q-490).

## New issues

### Q-488. `quillo check` lets a connection's functions run in a `with` that lacks `db`; the request then fails with a 500

`db.query` inside `with http` is caught by `quillo check`. The same query through a connection (`a.query`), or through a connection passed as a parameter, passes the check, and the route then answers `500 internal error`:

```quillo
fn direct() with http = db.query("select 1 as x")     // check: error, as it should be
let a = db.connect("b.db")
fn via() with http = a.query("select 1 as x")         // check: nothing
fn passed(c) with http = c.query("select 1 as x")     // check: nothing
```

```
$ quillo check p1b.qwe
p1b.qwe:1: error: `db.query` needs the db effect, but `direct` is declared `with http`
quillo: 1 error
$ curl localhost:4391/p1      # route GET /p1 = via_conn()
internal error
quillo: error: GET /p1 (rid d8fd0276afb3851e): main.qwe:13: `db.query` needs the db effect, but `via_conn` is declared `with http`
```

At run time the connection's functions are `Value::Builtin("db.query", ...)`, so `builtin_effect` sees them. The checker resolves `x.f` to an effect only when `x` is the name `db` (`check.rs:2945`). Expected: a global bound to `db.connect(...)` is known as a connection, and a call on an unknown receiver that names a `db` function (`.query`, `.exec`, `.one`, `.transaction`) in a function with a declared `with` is at least a warning. With aliases (Part C) it becomes exact. Kind: loud (fails in production, not in the check).

### Q-489. A `.qwer` function needs no effect: it runs inside any `with`

```rust
// store.qwer
pub fn store_put(v: &str) -> Result<bool, String> {
    std::fs::write("store.txt", v).map_err(|e| e.to_string())?;
    Ok(true)
}
```

```quillo
use "store.qwer"
fn pure_time() with time = store_put("written from a `with time` function")
print(pure_time())            // true
print(read_file("store.txt")) // written from a `with time` function
```

`quillo check` says `ok`, and the run writes the file. A `.qwer` function's builtin name is `native:<lib>:<fn>`, for which `builtin_effect` (`ast.rs:133`) returns 0. The guide's own Redis example ("Other stores") is a database reached this way, so `with db` says nothing about it. Expected: a header line `//! effects: db, http`, read by `native::parse` with the signatures. A function in a file without that line keeps working outside `with` and is refused inside one (or allowed with a warning for one release). Kind: silent.

### Q-490. `db.open` at run time: open pages keep the old database's rows, and new live calls get them for 2 s

```quillo
db.open("a.db")   // notes: "from a"
server fn load_notes() = db.query("select text from notes")
route GET /switch = { db.open("c.db"); /* create, insert "from c" */ "switched" }
route GET /switch_quiet = { db.open("a.db"); "back to a" }
```

With a socket on `/_quillo/live?t=db:notes`:

```
+317ms live: {"seq":6,...,"changed":["db:notes"]}     // /switch wrote to c.db, so pages hear it
GET /switch: switched
load_notes: [{"text":"from c"}]
GET /switch_quiet: back to a                           // no message on the socket
load_notes now: [{"text":"from c"}]                    // a live call 1.5 s later: still c's rows
```

A plain call then gives `from a`, and a live call does too after `SHARED_FOR` (2 s). `open` (`db.rs:1064`) only replaces the slot: it announces nothing and leaves `livecache`'s shared runs for `db:` in place. Pages showing the program's database never ask again until something writes to each table they read. Contrast probe P4: pointing a global connection at another database (`store = db.connect("c.db")`) announces `var:store`, and the page refreshes at once. Expected: `db.open` after the start, and any switch (Part D), drops the shared runs and announces every `db:` topic. Kind: silent.

### Q-491. A connection's topics are named by a hash of its target text: two spellings of one file are two databases to live pages

```quillo
let b1 = db.connect("b.db")
let b2 = db.connect("./b.db")
server fn load_b1() = b1.query("select text from notes")
server fn load_b2() = b2.query("select text from notes")
```

```
load_b1 topics: ["db@55c8e71313e4:notes","var:b1"]
load_b2 topics: ["db@ba8f63819b87:notes","var:b2"]
```

One file, two connections (two `Mutex<Connection>`s), two sets of topics. A write through `b1` does not refresh a page reading through `b2`. The label is `sha256(target)[..12]` (`db.rs:1087`), so the same happens to:

- a URL whose password changes: during a rolling rotation, servers on the old and new URL name the same table differently, and pages on one stop following writes on the other (not run: read in `db_connect`);
- a host spelled two ways (`db.internal` / `10.0.0.5`);
- a function target, labelled `fn<n>` by the order connections were made, which differs between servers that connect in a different order.

Expected: SQLite paths made absolute before the label is taken (as `open` already does for the startup line), and credentials and query options left out of a URL's label. Aliases (C2) name the topics outright. Kind: silent.

### Q-492. `sql_name` quotes for the program's database, not the connection the SQL is for: on MySQL beside SQLite, every row has the column's name as its value

```quillo
db.open("app.db")
let s = db.connect("mysql://root@127.0.0.1:33391/shop")
// items: (1, 'Lamp'), (2, 'Desk')
let col = "title"
print(sql_name(col))                                            // "title"
print(s.query(`select ${sql_name(col)} from items order by id`))
// [{ title: "title" }, { title: "title" }]
```

MySQL reads `"title"` as a string literal. The answer has the right shape and the wrong values, with no error. The same quoting in `order by` silently does not sort, and in `where` compares with a constant. `sql_name` asks `program_db_is_mysql` (`builtins.rs:2603`), which looks at `db` only. The other direction works by luck: SQLite takes backticks. Expected: `s.sql_name(col)` on a connection (and `analytics.sql_name` on an alias). A bare `sql_name` that cannot tell which database the text is for, in a program with a MySQL database besides a SQLite or PostgreSQL one, gets a `quillo check` warning. Kind: silent.

### Q-493. `db.error_kind` reads PostgreSQL's English words: on a server whose messages are in another language it is null

With `alter role quillo set lc_messages = 'de_DE.UTF-8'` (as an install made with a German locale has by default):

```
err("db: doppelter Schlüsselwert verletzt Unique-Constraint »seats_pkey« (Schlüssel »(k)=(1)« existiert bereits.)")
error_kind: null
```

`kind_of` (`db.rs:341`) looks for "violates unique constraint" and friends. `pg::describe` keeps the message and its detail but not the SQLSTATE (`23505`), which `tokio_postgres` has (`db.code()`). Retries are not affected: they read the code (`pg.rs:594`). MySQL's kinds also hold, as its codes are in the text (`(1062)`). Expected: the PostgreSQL error carries its SQLSTATE, and `error_kind` maps `23505`, `23503`, `23502`, `23514`, `40001`, `40P01` and `55P03`. With the driver interface this is `DbError.kind` (B1). Kind: silent.

### Q-494. The same row reads differently from SQLite, PostgreSQL and MySQL, so moving a program's database changes what its pages show

See D2's table: a `timestamp` is `"... 15:00:00"` on SQLite and MariaDB and `"...T15:00:00"` on PostgreSQL. A `decimal(10,2)` holding 9.90 is `9.9m` on SQLite. A `json` column is text on SQLite and MariaDB and a map on PostgreSQL. `last_id` is `1`, `null` or `0`. Separately, `json.stringify(meta)` into a `json` column, the natural way on SQLite, stores a JSON string on PostgreSQL (`json_typeof` = `string`). Most of this is in the guide, database by database. Nothing checks it, and a component that formats `at` or shows `price` shows something else after the move. Expected: one form for each type on every driver (D2), from typed rows (B1), in 0.7; until then the guide shows the table in one place. Kind: limitation.

### Q-495. Drivers cannot be added, and PostgreSQL cannot be left out: dispatch is a closed enum and each database's SQL is spread through core

`Backend` and `Pinned` (`db.rs:63`, `db.rs:95`) list the three drivers, `#[cfg(feature = "mysql")]` on every arm. PostgreSQL's crates (`postgres`, `tokio-postgres`, `tokio-postgres-rustls`) are not optional. MySQL's feature leaves out 41 of 353 crates (`cargo tree`: 353 with the default features, 312 without `mysql`). Each database's own SQL sits outside its driver: `on duplicate key update` / `on conflict` for `_quillo_jobs` and `_quillo_later_tenants` (`db.rs:1340`, `db.rs:1441`), `insert ignore` / `on conflict do nothing` for `_quillo_idempotency` (`idempotency.rs:466`), three migration locks (`migrate.rs:50-190`), quoting (`builtins.rs:2603`) and error words (`db.rs:341`). A fourth database means changing all of them. Expected: B1. Kind: limitation (design).

### Q-496. A database has no name the program, the checker and `with` share

A second database is a value: `let analytics = db.connect(url)`. The checker cannot tell it is one (Q-488), its live topics are named by its URL's hash (Q-491), `with` has one `db` for all of them (`EFFECTS`, `ast.rs:117`, one bit), and nothing outside the program (a `.env` key, `quillo describe`, the health page) can name it. Expected: `database <name> = <target>` (C1), topics by name (C2), `with db.<name>` (C3). Kind: limitation (design).

### Q-497. The `.qwer` interface cannot carry a database driver: JSON per call, no handles, and slower than Python for rows

A `.qwer` call encodes its arguments as JSON, calls `quillo_native_call`, and decodes a JSON answer (`native.rs:637`, `native.rs:1082`). For a driver that means:

- no handle that outlives a call (a pool, the connection a transaction holds), except a `static` in the library, as the guide's Redis example keeps;
- decimals cross as text (`Value::Dec` → `J::String`, `value.rs:380`), so the type is lost unless the driver tags it;
- no way back into Quillo during a call (no announcing; core must do it from the statement, which is the better design anyway);
- the library's own copy of tokio, so PostgreSQL's pipelining on Quillo's runtime cannot be reached.

The cost of moving rows across it, best of 5, 100,000 rows of `{id, name, total}` (probe P7):

| Path | ms |
|---|---|
| `db.query` on SQLite (built in) | 14, 16 |
| Python 3 `sqlite3`, rows as dicts | 50 |
| A `.qwer` function returning the rows as JSON (rows made in Rust, then JSON both ways) | 87, 102 |

So a driver behind today's interface would break the rule that Quillo beats Python. Expected: a driver ABI of its own (B2) and the benchmark as the bar. Kind: limitation (design).

## Proposed order of work

1. **The six bugs, now, on any branch** (each with tests and edge tests; none needs the redesign): Q-493 (SQLSTATE into PostgreSQL's errors, `error_kind` by code), Q-492 (`conn.sql_name`; a warning for the bare one), Q-490 (`db.open` after the start announces `db:*` and drops the shared runs), Q-491 (absolute SQLite paths; labels without credentials), Q-488 (connections in the checker), Q-489 (`//! effects:` in `.qwer` headers).
2. **B1: the driver interface, linked in.** Three crates behind one trait, with each database's SQL moved into its `Dialect` and the existing database tests as the shared contract. No change a program can see. Rust coverage held at 99%.
3. **C: aliases.** `database x = ...`, topics by alias, `with db.x`, the language server and `quillo check` taught the names, the guide's "Several databases" rewritten around them. `db.connect` kept.
4. **D: switching and common values.** `db.switch` / reload of an alias's URL, and the value forms of D2 (0.7, listed as a change, `{ raw: true }` for the old ones).
5. **B2: runtime driver plugins**, for drivers Quillo does not ship, if wanted: driver ABI v2, prebuilt per platform in the catalog, the `vs-python` benchmark as the bar. PostgreSQL and MySQL move out of the release binary only if they pass that bar there.

## Questions for you

1. **Linked in, or downloaded?** Should the release binary keep PostgreSQL and MySQL built in, as separate crates (recommended), or ship SQLite only and fetch the others with `quillo install`? The second needs B2's ABI and prebuilt libraries for every platform, and gives up PostgreSQL's pipelining unless that driver stays linked in.
2. **How aliases are written.** A declaration (`database analytics = env(...)`, recommended: the checker and `with` can see it), or a call (`db.alias("analytics", url)`), which looks like today's API but which `with` cannot check before the program runs.
3. **The value forms of D2.** Make them the default in 0.7, a change programs will notice, or opt-in per alias?
4. **Switching at run time.** A program call (`db.switch`), a reload of the alias's environment variable only, or both?

## Smaller notes

- `db.connect(target, { replicas: [...] })` with a target already connected returns the first connection and drops the new options without a word (`db_connect` finds it by label before reading the options). Read, not run.
- The guide's "Other stores" says writes through Rust are not part of `db.transaction`. With driver plugins that stays true only for non-`Driver` plugins, and the guide should say which is which.
- MariaDB's JSON columns come back as text (documented). The D2 proposal needs the server's word that a column is JSON, which MariaDB gives only for some versions and clients. If it cannot, `{ json: ["meta"] }` per alias, or a column-name rule, is the fallback. Not tested here.
- A tenant's database made by a function target (`db.open((t) => ...)`) may be any driver. With aliases, `database tenants = (t) => ...` keeps that, and its topics are `db@tenants:<table>` within the tenant, as now.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `rm -f *.db*` in a fresh folder | bash habit | zsh stopped the chain on "no matches found" (as in FEEDBACK42) | 1 run |
| `var store = db.connect(...)` | JavaScript habit | `expected end of statement, found store`: a global is a `let` that is assigned | 1 run |
| `decimal("9.90")`, then `9.90d` | Other languages' names | `unknown name decimal`, then a parse error: the literal is `9.90m` | 2 runs |
| Timing 10,000 rows with `now()` | Expected sub-millisecond time | `now()` is whole milliseconds: 1 ms and 6 ms meant little; reran with 100,000 rows | 1 run |
| PostgreSQL's socket in the scratch folder | `pg_ctl` default | The path is over macOS's 103-byte limit for Unix sockets: started with `-k ''`, TCP only | 1 run |
| `set lc_messages` in the same `psql -c` as the statements | Expected one session | The output in German showed it worked, but the table was missing from that run; used `alter role` for Quillo's connections | 1 run |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A connection's function in a `with http` function, at run time | refused with the effect named: `` `db.query` needs the db effect, but `via_conn` is declared `with http` `` |
| A global connection pointed at another database | `var:store` announced; the page's next answer has the new database's rows and topics (`db@ca9b0f0a1256:notes`) |
| Live topics of the program's `db` across a switch from SQLite to SQLite | stay `db:notes`: components following them need no change |
| A write after the switch | announces `db:notes` on the new database at once (probe P3, `/switch`) |
| Transactions' retries on a German-speaking PostgreSQL | read by SQLSTATE, not words (`pg.rs:594`): unaffected by Q-493 |
| A MySQL connection beside a SQLite `db` | opens, creates a database, queries, through `mysql://` with no TLS on this machine |
| `boolean` columns | `true` on all three databases |

## Scripts

Every probe used the release build (`Q=~/dev/quillo-app1/target/release/quillo`) in the session's scratch folder, on ports 4391 and 4392 (Quillo), 54391 (PostgreSQL) and 33391 (MariaDB). Each probe stopped only the PIDs it started.

**P1 to P3** (`main.qwe`): effects through a connection, two spellings of one file, the program's database switched under a live socket.

```quillo
db.open("a.db")
db.exec("create table if not exists notes (id integer primary key, text text)")
db.exec("delete from notes")
db.exec("insert into notes (text) values ('from a')")
let b1 = db.connect("b.db")
let b2 = db.connect("./b.db")
b1.exec("create table if not exists notes (id integer primary key, text text)")
b1.exec("delete from notes")
b1.exec("insert into notes (text) values ('from b')")
fn via_conn() with http = b1.query("select text from notes")
route GET /p1 = via_conn()
server fn load_b1() = b1.query("select text from notes")
server fn load_b2() = b2.query("select text from notes")
server fn load_notes() = db.query("select text from notes")
route GET /switch = {
    db.open("c.db")
    db.exec("create table if not exists notes (id integer primary key, text text)")
    db.exec("delete from notes")
    let _ = db.exec("insert into notes (text) values ('from c')")
    "switched"
}
route GET /switch_quiet = {
    db.open("a.db")
    "back to a"
}
component Notes() {
    state notes = live(load_notes)
    <ul>{notes | map((n) => <li>{n.text}</li>)}</ul>
}
route GET / = <html><body><Notes /></body></html>
```

```js
// ws.mjs: a page's live socket, while the database is switched
const ws = new WebSocket("ws://localhost:4391/_quillo/live?t=" + encodeURIComponent("db:notes"));
const t0 = Date.now();
ws.onmessage = (e) => console.log(`+${Date.now() - t0}ms live:`, e.data);
ws.onopen = async () => {
  const rpc = async () => (await (await fetch("http://localhost:4391/_quillo/rpc/load_notes", { method: "POST", headers: { "content-type": "application/json", "x-quillo-live": "1" }, body: "[]" })).json()).ok;
  await new Promise(r => setTimeout(r, 300));
  console.log("GET /switch:", await (await fetch("http://localhost:4391/switch")).text());
  await new Promise(r => setTimeout(r, 500));
  console.log("load_notes:", JSON.stringify(await rpc()));
  console.log("GET /switch_quiet:", await (await fetch("http://localhost:4391/switch_quiet")).text());
  await new Promise(r => setTimeout(r, 1500));
  console.log("load_notes now:", JSON.stringify(await rpc()));
  ws.close();
};
```

**P1b** (`p1b.qwe`, `quillo check` only): the three functions of Q-488.

**P4** (`p4.qwe`): a global connection pointed elsewhere.

```quillo
let store = db.connect("b.db")
server fn load() = store.query("select text from notes")
route GET /flip = {
    store = db.connect("c.db")
    "flipped"
}
```

**P5** (`p5.qwe`): Q-493, against `postgres://quillo@127.0.0.1:54391/postgres` after `alter role quillo set lc_messages='de_DE.UTF-8'`.

```quillo
db.open("postgres://quillo@127.0.0.1:54391/postgres")
let _ = db.exec("create table if not exists seats (k int primary key)")
let _ = db.exec("delete from seats")
let _ = db.exec("insert into seats values (1)")
let r = db.exec("insert into seats values (1)")
print(r)
print("error_kind:", db.error_kind(r))
```

**P6** (`store.qwer`, `p6.qwe`): Q-489, as in its section.

**P7** (`rows.qwer`, `p7.qwe`): Q-497's timings; Python's baseline built the same 100,000 rows with a recursive `with` and read them with `cursor.description` into dicts.

```rust
/// `n` rows shaped as a query's: id, name, total.
pub fn fake_rows(n: i64) -> serde_json::Value {
    serde_json::Value::Array((0..n).map(|i| serde_json::json!({ "id": i, "name": format!("customer {}", i), "total": i as f64 * 1.5 })).collect())
}
```

```quillo
use "rows.qwer"
db.open(":memory:")
db.exec("create table c (id integer primary key, name text, total real)")
db.exec("with recursive s(i) as (select 0 union all select i + 1 from s where i < 99999) insert into c select i, 'customer ' || i, i * 1.5 from s")
let _ = fake_rows(10)
fn best(f) = range(5) | map((_) => { let t = now(); let r = f(); [now() - t, r.len] }) | map((p) => p[0]) | min
print("sqlite db.query, 100,000 rows:", best(() => db.query("select * from c")), "ms")
print(".qwer JSON,      100,000 rows:", best(() => fake_rows(100000)), "ms")
```

**P8** (`p8.qwe`): D2's table, run against `sw.db`, the PostgreSQL cluster and MariaDB in turn.

```quillo
fn probe(target: str) {
    db.open(target)
    let _ = db.exec("drop table if exists ev")
    let _ = db.exec("create table ev (id integer primary key, at timestamp, paid boolean, price decimal(10,2), meta json)")
    let r = db.exec("insert into ev (id, at, paid, price, meta) values (?, ?, ?, ?, ?)", 1, "2026-11-02 15:00:00", true, 9.90m, { tags: ["a"] })
    print(target, "exec:", r)
    print(target, "row: ", db.one("select * from ev where id = 1"))
}
probe("sw.db")
probe("postgres://quillo@127.0.0.1:54391/postgres")
probe("mysql://root@127.0.0.1:33391/shop")
```

**P9** (`p9.qwe`): Q-492, as in its section.
