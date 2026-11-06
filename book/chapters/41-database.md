# Database

`db` is built in: SQLite inside the server, or PostgreSQL, MySQL and MariaDB over the
network, all with the same functions. Values go in as parameters, never as SQL text, and
what goes wrong comes back as an error value, so `?` and `??` handle a failed query as
they handle anything else. This chapter goes through it in the order an app needs it; the
details are also in [Standard library](#standard-library-database).

`db` is a library for SQL databases, not syntax. Another kind of store (Redis, MongoDB, a
graph database) comes in through its Rust crate: see [Other stores](#other-stores).

## Opening a database

```quillo
db.open("app.db")                                   // SQLite, a file next to the program
db.open(":memory:")                                 // SQLite, in memory
db.open("postgres://user:pass@host:5432/app")       // PostgreSQL
db.open("mysql://user:pass@host:3306/app")          // MySQL or MariaDB (also mariadb://)
db.open(env("DATABASE_URL"))                        // whatever the environment says
```

Call it once, at the top of the program. Without it, the database is `$QUILLO_DB` (a
path or a URL), or else SQLite in memory, which is handy for trying things out and for
tests.

When the database cannot be reached, `db.open` gives an error value, as every `db`
function does, and `db` stays what it was: SQLite in memory, empty. At the top of a
program nothing looks at that value, so pass it to `unwrap`, which stops the program
with the reason instead of letting it start on the wrong database:

```quillo
db.open(env("DATABASE_URL")) | unwrap
```

## Queries

Three functions do the work:

| Call | Gives |
|---|---|
| `db.query(sql, ...values)` | every row, as a list of maps |
| `db.one(sql, ...values)` | the first row as a map, or `null` |
| `db.exec(sql, ...values)` | `{ changes, last_id }` for statements that change the database |

```quillo
db.exec("create table if not exists todos (
    id integer primary key,
    text text not null,
    done int not null default 0
)")

let r = db.exec("insert into todos (text) values (?)", "Write the docs")      // { changes: 1, last_id: 1 }
let open = db.query("select id, text from todos where done = ? order by id", 0)
let todo = db.one("select * from todos where id = :id", { id: r.last_id })
let missing = db.one("select * from todos where id = ?", 999)               // null
```

Values are given after the SQL, one for each `?`, or as a map for `:name`
placeholders. PostgreSQL also takes its own `$1`, `$2`. Every `:name` needs a value, but
keys the SQL does not use are ignored, so one map can serve a query put together from
optional parts; given to SQL with no names it is no value, and given for a lone `?` it
is that value, stored as JSON. Placeholders given no values are an error. SQLite also
reads `@name`, `$name` and `#name`, and `?1` as the first value; on MySQL `@name` is a
variable of the session, so a map with that name is refused: write `:name`. Rows are
maps from column names to values, so `todo.text` reads a column, and a list of rows
goes straight into `for` in a page or out as JSON.

`changes` and `last_id` mean the same on every database. `changes` is the rows the
statement matched: an `update` that sets a value a row already has counts that row
(MySQL's own count, of rows changed, is `?client_found_rows=false` in its URL), an
upsert's or a `replace`'s row counts once, and a read or a schema statement counts 0.
`last_id` is the id of the one row an `insert` made, on SQLite and MySQL, and `null`
after anything else: an `insert` of several rows, an upsert that updated, an `update`.
PostgreSQL has no such id, so it is always `null` there: write `insert ... returning id`
with `db.one`.

Five things that surprise people:

- **A row is a map**, so two columns with one name (`select a.id, b.id`) leave only the
  last: name them apart with `as`.
- **`db.exec` with values runs one statement.** On SQLite, what follows the first `;` is
  ignored; without values, the whole text runs (a seed file), split where the database
  would split it, so a trigger's or function's body (`begin ... end`, `$$ ... $$`) is
  one statement. A statement that gives rows (`insert ... returning id`) runs in
  `db.exec` too; its rows are `db.one`'s and `db.query`'s.
- **`db.query` gives every row at once**, in memory: page through large tables with
  `limit` and the last row's key.
- **Values are separate arguments**, `db.exec(sql, a, b)`, not a list as Python's
  `sqlite3` takes them: `db.exec("insert ... values (?)", [x])` stores the list as its
  JSON text. `quillo check` says so when the SQL is written out, but not where a list is
  PostgreSQL's array: a `?` in `any(...)` or `all(...)`, or cast to an array type
  (`?::int[]`).
- **A value is what it is on every database.** NaN and the infinities are refused,
  naming them, since SQLite keeps NaN as null and MySQL refuses both; `-0.0` is stored
  and read as `0.0`. An error value given as a value is not stored but given back as the
  error it was, so `int("4O")` never becomes a row. SQL with no statement in it (`""`,
  only comments) is an error value.

A query that fails (a syntax error, a constraint, the server unreachable) is an error
value with the database's message:

```quillo
fn add(text: str) -> int with db = db.exec("insert into todos (text) values (?)", text)?.last_id

route POST /todos { form: { text: str } } with db {
    let id = add(text) ?? reply 422 "Could not add that"
    redirect(`/todos/${id}`)
}
```

`db.error_kind(e)` says what kind of failure it was, the same on every database:
`"unique"`, `"foreign_key"`, `"not_null"`, `"check"`, `"conflict"` (another
transaction), or `null`:

```quillo
let r = db.exec("insert into seats (show, seat) values (?, ?)", show, seat)
if db.error_kind(r) == "unique" { return err("Someone has just taken that seat") }
```

On PostgreSQL, the error's text ends in its SQLSTATE, as ` [23505]`, and `db.error_kind`
reads that code first. So it gives the same word on a server whose messages are in
another language.

`` sql`...` `` writes a query over several lines and tells editors to highlight it as
SQL. It is a template string like any other, so it should hold no `${...}` values:
those go in as parameters.

```quillo
fn recent(limit: int) -> list with db = db.query(sql`
    select id, text
    from todos
    order by id desc
    limit ?`, limit)
```

## Safe SQL

Statements are prepared and values are sent apart from the SQL, so a value can never
change what a query does. `quillo check` warns where SQL is put together from values
(`${x}` or `+`): pass them as `?` parameters instead.

Names cannot be parameters. When a table or a column comes from a choice (a column to
sort by, say), put it in with `sql_name`, which takes only letters and digits of any
script (with the marks written on a letter) and `_`, at most 63 bytes, so a name is the
same name on every database, or a list of such names:

```quillo
fn sorted(by: "text" | "id") -> list with db = db.query(`select id, text from todos order by ${sql_name(by)}`)
```

SQL built only from text as written, `sql_name` and `sql_param`, and choices between
those, is not warned about, nor is a `map` over such pieces joined with `join`, which
`quillo check` follows through every value its function returns. So an update of the
columns the program chose passes:

```quillo
fn save(id: int, data: map) with db {
    let cols = ["title", "body"]                       // chosen here, never from the form
    let set = cols | map((c) => `${sql_name(c)} = ${sql_param(c)}`) | join(", ")
    db.exec(`update notes set ${set} where id = :id`, { ...data, id })
}
```

while `` map((c) => `${c} = 1`) `` warns. Only the builtins count: a `sql_name` the
program or the function defines for itself is followed as any other value.

`sql_name` quotes for the program's database: `"title"`, or `` `title` `` on MySQL. For
SQL sent to another database, quote with that connection's own:
`analytics.sql_name(col)`. MySQL reads `"title"` as text, so a name quoted for SQLite
gives the word `title` in every row there.

## Rows as records

A `type` names the shape of a row, and an annotation checks rows against it as they
come back. A `User(row)` call makes one, keeping only its fields, which is the way to
turn a row into what the rest of the program expects:

```quillo
type Todo = { id: int, text: str }

fn todos() -> list<Todo> with db = db.query("select id, text from todos order by id")
```

See [Records](#records) for what a record checks and how it reports
what is wrong.

## Transactions

`db.transaction(f)` runs `f` and commits what it did, all or nothing:

```quillo
fn transfer(from: int, to: int, amount: int) -> map with db =
    db.transaction(() => {
        db.exec("update accounts set balance = balance - ? where id = ?", amount, from)?
        db.exec("update accounts set balance = balance + ? where id = ?", amount, to)?
        { moved: amount }
    })
```

- It rolls back if the function fails, returns an error value (as `?` does here) or
  replies early. Otherwise it commits, and gives what the function gave.
- Transactions nest, as savepoints: one inside another that fails undoes only its own
  part, and the outer one goes on.
- `db.transaction({ isolation, retries }, f)` takes those two options; another is an
  error, when it runs and in `quillo check` (`` `db.transaction` has no option
  `isolaton` (did you mean `isolation`?) ``).
- On MySQL and MariaDB a statement that commits by itself (`truncate`, `create table`,
  `alter`, `drop`) is refused inside a transaction and in `db.exec_many`, before it
  runs, so what ran before it can still be rolled back: run it before or after.
  PostgreSQL and SQLite roll their schema statements back, and take them in a
  transaction.
- On PostgreSQL and MySQL a transaction keeps one connection until it ends, so other
  requests never see its changes early. On SQLite, `sleep` and `http` calls inside a
  transaction hold up other requests rather than let them write into it.
- `parallel` inside a transaction runs its functions one after another, since queries
  on other threads would not be part of it.
- Live pages hear of its changes to tables when it commits, and not at all if it rolls
  back. A global variable it changes keeps its new value either way (variables are not
  rolled back), so pages showing it hear of it when the outermost transaction ends,
  whatever its outcome.

## Migrations

`db.migrate()` brings the schema up to date from SQL files in a folder, `migrations`
unless another is given, applied in name order:

```
migrations/
    001_todos.sql
    002_tags.sql
    003_todo_due_date.sql
```

```quillo
db.open(env("DATABASE_URL"))
db.migrate()
```

- Each file runs once, in its own transaction, and is recorded in the table
  `_quillo_migrations` with its SHA-256. Only `.sql` files directly in the folder count,
  sorted by name as text: pad the numbers (`002`, not `2`), since `10_x.sql` sorts
  before `2_x.sql`. A file renamed is a new file, and runs again.
- A file that fails is rolled back and stops the program from starting, with the file's
  name and the database's message.
- A recorded file whose text changed afterwards gets a warning, not a second run: add a
  new file to change the schema again.
- Servers that start together take turns (an advisory lock on PostgreSQL, a named lock
  on MySQL, an immediate transaction on SQLite), so each file still runs once.
- MySQL commits a schema change at once, so a file that fails part way keeps what it did
  before the failure (a migration may hold one, which `db.transaction` refuses). Put one
  change in each file.

## Live data {#database-live-data}

A page can follow the database. `live(load)` in a component runs a `server fn` again
whenever a table it read changes, through Quillo, for anyone:

```quillo
server fn load_todos() -> list = db.query("select id, text from todos order by id")
server fn add_todo(text: str) -> map = db.exec("insert into todos (text) values (?)", text)

component Todos() {
    state todos = live(load_todos)      // every open page sees new todos
    state draft = ""
    <div>
        <ul>{for t in todos => <li key={t.id}>{t.text}</li>}</ul>
        <form onsubmit={() => { add_todo(draft); draft = "" }}>
            <input value={draft} placeholder="Add a todo"/>
        </form>
    </div>
}

route GET / = <html><body><Todos/></body></html>
```

The form calls `add_todo` and does not touch `todos` itself: the insert announces the
table, and `live` runs `load_todos` again, on this page and on every other page showing
the list.

The tables a loader reads are found in its SQL (`from`, `join`), and every `insert`,
`update` and `delete` through `db` announces the tables it writes. This works the same on
SQLite, PostgreSQL and MySQL. Changes made by other programs (`psql`, a script) are not
seen, nor what a trigger or a stored procedure writes, nor writes to the tables behind a
view a loader reads: `publish` a topic for those.

A write wakes every loader that read the table, whatever rows it read: a reply in one
thread runs `load_posts` again on every open thread page. To follow one thread only,
read the table inside `unwatched` and watch a topic of your own, which the writer
publishes:

```quillo
server fn load_posts(thread: int) -> list {
    watch(`thread:${thread}`)
    unwatched(() => db.query("select * from posts where thread = ?", thread))
}

server fn reply_to(thread: int, text: str) {
    db.exec("insert into posts (thread, text) values (?, ?)", thread, text)
    publish(`thread:${thread}`)
}
```

A topic is any text of 1 to 256 bytes, commas included; `watch` and `publish` stop on an
empty or a longer one. One that ends in `:*` is a
wildcard: `publish("thread:*")` wakes every page watching a `thread:` topic. So a topic
built from what a visitor typed must not end in `:*`; one built from an int, as here,
cannot.

What a loader's helpers read counts too: a `me()` that looks up `sessions` makes every
loader calling it run again when anyone signs in, so call it as `unwatched(() => me())`.
Keep what changes often but is not shown (view counts, last-seen times) in a table of
its own. See [Live data](#live-data), and
[Several servers](#several-servers) for apps on more than one server.

## Several databases

`db` is the program's database; `db.connect(target)` opens another, of any kind, with
the same functions and its own transactions, migrations and live updates:

```quillo
db.open("app.db")
let analytics = db.connect(env("ANALYTICS_URL"))
analytics.migrate("migrations/analytics")

let sales = analytics.query("select day, sum(total) as total from sales group by day")
```

- Call its functions on it (`analytics.query(...)`): that is how they know which
  database. Connect at the top of the program; the same target again is the same
  connection, not a new pool. A SQLite file is the same by any spelling:
  `"analytics.db"` and `"./analytics.db"` are one connection.
- Its calls need the `db` effect, as `db`'s do: `analytics.query(...)` in a function
  declared `with http` is an error.
- A transaction belongs to its database: one inside another on a different database
  commits or rolls back on its own.
- Live updates tell the databases apart, and a connection never reaches the browser
  (its URL may hold a password). A server's live topics leave out the user, the password
  and the options, so servers on one database agree on them, during a password's
  rotation too.
- `db.open` called again while the program runs replaces its database, and open pages
  showing the old one's rows are told, every tenant's.
- Mounted programs and services have their own `db`, so one Quillo can serve programs
  on different databases side by side.

## Read replicas

A PostgreSQL or MySQL database's replicas can take its reads:

```quillo
db.open(env("DATABASE_URL"), { replicas: [env("REPLICA_1"), env("REPLICA_2")] }) | unwrap
```

- Reads (`select`, and `with` that writes nothing) go to the replicas in turn. Only pure
  reads do: a `select` that calls a function that may write (`nextval`, `setval`, or one
  made in the database with `create function`) goes to the primary; built-in functions
  that only compute (`lower`) do not. A replica's read-only refusal sends that
  statement to the primary without taking the replica out of use.
- Writes, reads that lock (`for update`, `for share`), everything in a transaction, and
  every read a request makes after it wrote go to the primary: a request always reads
  what it wrote.
- A replica that fails is left out for ten seconds, and its reads go to the primary
  meanwhile. Every replica must be reachable when the program opens the database:
  otherwise `db.open` gives an error value, which `unwrap` turns into a stop. The error
  names the replica without its user and password *(since 0.7: the URL went to the log
  whole)*.
- Another request may still read from a replica that has not caught up with a write made
  just before. Reads that cannot wait for that belong in a transaction.
- `db.connect(url, { replicas: [...] })` takes them too. Replicas are for one database
  given by its URL: not SQLite, and not a database per tenant.

## Shards

When one database is not enough, `db.shards` splits data over several by a key, such
as a customer or an account:

```quillo
let users = db.shards([env("SHARD_1"), env("SHARD_2"), env("SHARD_3")])
users.migrate()                                            // each shard

users.on(id).exec("insert into users (id, name) values (?, ?)", id, name)
users.on(id).one("select * from users where id = ?", id)  // the shard for the key

let total = users.all("select count(*) as n from users") | map((r) => r.n) | sum
```

- `.on(key)` is the shard for a key: a connection, as `db.connect` gives, with its own
  transactions.
- `.all(sql, ...values)` asks every shard and gives their rows together, in shard order.
  An aggregate comes back once per shard (`count(*)` above gives a row from each), so
  combine them in the program.
- `.migrate()` brings every shard up to date, and `.count` is how many there are.
  `query`, `one`, `exec` and `transaction` on the shards themselves give an error that
  says to use `.on` or `.all`: a query has to know where to go.
- Keys are placed by jump consistent hashing of their text, the same on every server.
  A shard added at the end of the list takes its share of keys from each of the others
  and moves no other key; those are the rows to copy over.
- Shards may be any kind of database, mixed, but each is one database: no `{tenant}`.

## Joining across databases

One SQL query cannot join tables in two databases. `join_on` puts their rows together in
the program:

```quillo
let orders = db.query("select * from orders where day = ?", today)
let people = accounts.query("select * from users where id = any(?)", orders | map((o) => o.user_id))

join_on(orders, people, "user_id", "id")    // each order with `user`: its user, or null
    | filter((o) => o.user != null)          // only orders whose user was found
```

`join_on(rows, others, key, other_key = key, as)` gives each row its first match in
`others` (the same value, of the same type) under `as`. Unless given, `as` is `key`
without `_id` (`user` here), else `joined`.

## One transaction over several databases

`db.transaction` is all or nothing on one database. `db.atomic` is all or nothing over
several:

```quillo
fn pay(from: int, n: int) -> int =
    db.atomic([db, ledger], () => {
        db.exec("update accounts set balance = balance - ? where id = ?", n, from)?
        ledger.exec("insert into entries (account, amount) values (?, ?)", from, -n)?
        n
    })
```

- It commits in two phases: each PostgreSQL and MySQL database prepares its part
  (prepared transactions, XA), the decision is recorded in Quillo's own tables, then
  every part commits. A part that cannot prepare rolls every part back, as does a
  function that fails, returns an error value or replies early.
- At most one SQLite database can take part, since SQLite cannot prepare a
  transaction: it commits last. Naming two is an error, not an error value.
- A server that stops part way leaves prepared parts holding their locks. Recovery
  finishes them a minute later, from any server of the program: it commits those
  decided and rolls back the rest.
- PostgreSQL needs `max_prepared_transactions` above 0, and it is 0 by default.
- `db.transaction` inside it nests as usual; `db.atomic` cannot begin inside a
  transaction on one of its databases.

## Quillo's own tables

Quillo keeps a few tables of its own in the program's database: scheduled jobs
(`_quillo_jobs`, each run once, whichever server it is due on), the decisions of
`db.atomic` (`_quillo_atomic`) and the migrations applied (`_quillo_migrations`).

With a database per tenant there is no one program database to keep them in, so the
first two go to a system database: `db.system(target)` (one every server of the program
shares, when there are several), else `$QUILLO_SYSTEM_DB`, else a SQLite file
`quillo-system.db` next to the program. `db.system` also keeps them out of the program's
own tables when there is one database.

Calls made later (`_quillo_later`, see
[Microservices](#calls-made-later)) stay with the work wherever the
others go, in the program's database or the tenant's, so they commit or roll back with it.
With a database per tenant, the system database only notes which tenants have some.

## A database per tenant

One program can serve many tenants (customers, teams, sites), each with a database of
its own:

```quillo
tenant_from("example.com")          // acme.example.com is tenant "acme"
db.open("data/{tenant}.db")         // a SQLite file per tenant
db.migrate()                        // run for each tenant's database when it is made

route POST /signup { form: { name: str } } with db {
    let made = new_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))   // makes acme.db
    if is_err(made) { reply 409 "That name is taken" }
    redirect(`https://${name}.example.com/`)
}
```

| Target | One per tenant |
|---|---|
| `data/{tenant}.db` | SQLite file |
| `postgres://host/app?schema=t_{tenant}` | PostgreSQL schema, in one database |
| `postgres://host/app_{tenant}` | PostgreSQL database, made by you beforehand |
| `mysql://host/app_{tenant}` | MySQL or MariaDB database |
| a function of the tenant's name | whatever it gives, or `null` for no such tenant |

- `new_tenant(name, f)` makes a tenant's database, migrates it and runs `f` for it, or
  gives an error when that tenant exists, in one step on the database, so a second
  signup for a name cannot land in a customer's data. `with_tenant(name, f)` runs code for
  a tenant, making its database if there is none: for scripts and jobs.
- Requests only open databases that exist, and no file or schema is made for a made-up
  host. Where a route needs its database, it answers 404, unless the route answers for
  itself; a route that needs no database answers as usual, so `www.example.com` can be a
  page of its own.
- `db.tenant_exists(name)` says whether a tenant has a database, without making one.
  `db.tenants()` lists the tenants whose databases exist, for a SQLite, PostgreSQL schema
  or MySQL template; where a function says where tenants are, keep the list yourself.
- Live updates stay within a tenant: a page follows only its own tenant's topics and
  those of no tenant *(since 0.7)*. Channels and media streams are each tenant's own too
  (see [Real time](#realtime)), unless `tenants: "shared"`. Global variables are
  shared by every tenant, so keep a tenant's data in its database.

[Many tenants](#many-tenants) has the rest: tenant names, schema
prefixes, and how many databases stay open at once.

## Engines

### SQLite

Built in, nothing to install: the database runs inside the server process, in a file
next to the program or in memory, with foreign keys on and WAL mode. It is SQLite 3.46,
with JSON and FTS5 full-text search, and without math functions (`sin`, `radians`).
Booleans are stored as `0` and `1`, and a column declared `boolean` reads them back as
`true` and `false`. Queries run on the server's own thread: a long one holds up the
other requests of its instance. Several processes may share the file: readers do not
wait, writers take turns, for up to 5 seconds before a write fails as a conflict.

### PostgreSQL

- Placeholders: `?`, `:name` with a map, or `$1`. The first two are translated, skipping
  quotes (`U&"..."` names too), comments (nested, as the server reads them) and `$$`
  strings; `?|`, `?&` and `@?` are operators.
- Values convert both ways: numbers, text, booleans, `json` and `jsonb` as maps and
  lists, arrays as lists, `numeric` as [decimals](#money) with their scale (an int
  when it has none), `money` as a decimal with its `lc_monetary` currency's decimals (no
  currency comes with it: `numeric(p, s)` and a currency column say more), `xid` and
  `xid8` as ints, `bit(n)` and `varbit` as their digits (`"00000101"`, kept at their
  length); `inet`, `cidr`, `macaddr`, `uuid`, `date`, `time`, `timetz`, `timestamptz`
  (ISO 8601, in UTC), `timestamp` (as stored, no zone), `interval` (PostgreSQL's own
  text) and `pg_lsn` as text. A BC date is ISO 8601's, `-0043-03-15` for 44 BC, and a
  `real` reads as the shortest float that prints the same (`0.1`). A type with no reader
  of its own (ranges, `hstore`, composites, `tsvector`) is its bytes in hex (`\x...`):
  cast it in the query (`r::text`). An array whose lower bound is not 1
  (`'[0:2]={1,2,3}'`) comes back as its text.
- JSON is read cell by cell: a whole number past an int is a decimal, past a decimal a
  float, so two large ids stay two; a value that cannot be read is an error value in
  its own cell, and the other cells read. A text given as a `json` or `jsonb` column's
  value is the JSON it holds, as MySQL's JSON column takes it: `json.stringify(m)`
  stores an object, not a string.
- Parameters are typed by the column, and those of other types are sent as text for
  PostgreSQL to convert. A float past `real`'s or `double precision`'s range is
  refused, and a value where nothing in the SQL says its type (`select greatest(?, ?)`)
  is sent as an `int8`, a `float8` or text, by what it is, so `greatest(10, 9)` is `10`
  as on the others.
- `last_id` is `null`: use `insert ... returning id` with `db.one`. `db.one` fetches the
  whole answer and keeps the first row, so write `limit 1`.
- Connections come from a pool, 8 unless the URL says `?pool_size=N`, and queries outside
  transactions share a second set, pipelined: plan for about twice `pool_size` per
  server. A query lets other requests run while it waits.
- TLS is required for a server on another machine, with its certificate and name
  checked (`sslmode=verify-full`); on this machine it is used when the server offers it.
  A hosted database with a CA of its own (Amazon RDS, Cloud SQL) names it with
  `?sslrootcert=/path/ca.pem`. See [TLS to the database](#tls-to-the-database).

### MySQL and MariaDB

- Placeholders: `?`, or `:name` with a map (translated, skipping quotes, backticks and
  comments; `/*! */` and MariaDB's `/*M! */`, which the server runs, are read as SQL).
- Values convert both ways: numbers, text, `tinyint(1)` as booleans when it holds 0 or 1
  (another value is an int), `decimal` as decimals, MySQL's `json` as maps and lists
  (MariaDB's JSON is text: `json.parse` it), `BIT(n)` as its digits (`"00000101"`),
  which it takes back, dates and times as text (`2026-10-27 14:30:00`). An unsigned
  value or an id past 2^63 is a decimal (`9223372036854775808m`), and a `FLOAT` reads
  as the shortest float that prints the same. `last_id` is the new row's
  `auto_increment` id when one row was inserted, else `null`.
- `db.open` of a database that is not there is an error (`Unknown database 'shop'`), as
  on PostgreSQL, so a typo in a URL makes no empty database; a tenant's is made by the
  driver. A host may be an IPv6 address, `mysql://app@[::1]:3306/app`.
- A `||` joins text, as on the other two: on MariaDB, whose `||` is `or` by default, the
  driver sets `PIPES_AS_CONCAT` for SQL that has one, migrations and scripts too.
- A statement larger than the server's `max_allowed_packet`, its values included, is
  refused, saying so and how big it is.
- Connections come from the driver's pool, and a query lets other requests run while it
  waits. `pool_size` is PostgreSQL's: in a MySQL URL it stops the database from opening.
- TLS is required for a server on another machine, as for PostgreSQL; `?ssl_ca=` names a
  CA of its own.
- Built in by default as the `mysql` feature; `cargo build --no-default-features`
  leaves it out.

## Reference

| Function | Does |
|---|---|
| `db.open(target)` | Opens the program's database: a path, `:memory:`, a URL, a template with `{tenant}`, or a function of the tenant |
| `db.connect(target)` | Opens another database, with these same functions |
| `db.query(sql, ...values)` | Every row, as a list of maps |
| `db.one(sql, ...values)` | The first row, or `null` |
| `db.exec(sql, ...values)` | `{ changes, last_id }` |
| `db.error_kind(e)` | `"unique"`, `"foreign_key"`, `"not_null"`, `"check"`, `"conflict"` or `null` |
| `db.transaction(f)` | Runs `f`; commits, or rolls back on failure, error value or early reply |
| `db.migrate(folder = "migrations")` | Applies the SQL files not yet applied, in name order |
| `db.open(url, { replicas })` | Sends reads to replicas (also `db.connect`) |
| `db.shards([target, ...])` | Data split by key: `.on(key)`, `.all(sql, ...)`, `.migrate()`, `.count` |
| `db.atomic([db, ...], f)` | Runs `f` as one transaction over several databases |
| `db.system(target)` | Where Quillo keeps its own tables |
| `db.tenants()` | The tenants that have a database |
| `db.tenant_exists(name)` | Whether a tenant has a database, without making one |
| `sql_name(name)` | A table or column name, safe to put into SQL |
| `conn.sql_name(name)`, `db.sql_name(name)` | The same, quoted for that database |
| `sql_param(name)` | A `:name` placeholder, safe to put into SQL |
| `join_on(rows, others, key, other_key, as)` | Each row with its first match from another list of rows |

A function declared with effects may use the database only if `db` is among them: one
declared `with http` cannot query, on `db` or on a connection, and `quillo check` says so
before the program runs.
Functions without `with` may do anything.

## What it does not do

- **Other databases.** SQLite, PostgreSQL, MySQL and MariaDB only; any other text given
  to `db.open` is taken for a SQLite file. Redis, MongoDB, SQL Server and the rest come
  in through [Rust](#other-stores).
- **An ORM or a query builder.** SQL with parameters; [records](#records)
  check the shape of what comes back.
- **Bytes.** There is no bytes value: a BLOB comes back as text, `bytea` as hex. Keep
  files on disk, or base64 text. `files()` and `quillo serve` never send a SQLite file,
  whatever its name; `file("app.db")` sends one on purpose.
- **Streaming, `COPY`, `LISTEN`/`NOTIFY`.** Page with `limit`; insert many rows with
  `values (?, ?), (?, ?)`; use `publish` for notifications.
- **Per-query time limits, read-only connections, encryption at rest.** These are the
  database server's settings (`statement_timeout`, a read-only role, the disk's
  encryption); SQLite has none of them here.

The [reference](#standard-library-database) has the rest: parameters and their
traps, type conversions in full, pools and connections, tenants' schemas.
