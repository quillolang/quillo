# Database

`db` is built in: SQLite inside the server, or PostgreSQL, MySQL and MariaDB over the
network, all with the same functions. Values go in as parameters, never as SQL text, and
what goes wrong comes back as an error value, so `?` and `??` handle a failed query as
they handle anything else. This page goes through it in the order an app needs it; the
details are also in [Standard library](/docs/standard-library/#database).

### Opening a database

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

### Queries

Three functions do the work:

| Call | Gives |
|---|---|
| `db.query(sql, ...values)` | every row, as a list of maps |
| `db.one(sql, ...values)` | the first row as a map, or `null` |
| `db.exec(sql, ...values)` | `{ changes, last_id }` for statements that return no rows |

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
placeholders. PostgreSQL also takes its own `$1`, `$2`. Rows are maps from column
names to values, so `todo.text` reads a column, and a list of rows goes straight into
`for` in a page or out as JSON.

A query that fails (a syntax error, a constraint, the server unreachable) is an error
value with the database's message:

```quillo
fn add(text: str) with db = db.exec("insert into todos (text) values (?)", text)?.last_id

route POST /todos { form: { text: str } } {
    let id = add(text) ?? reply 422 "Could not add that"
    redirect(`/todos/${id}`)
}
```

`` sql`...` `` writes a query over several lines and tells editors to highlight it as
SQL. It is a template string like any other, so it should hold no `${...}` values:
those go in as parameters.

```quillo
fn recent(limit: int) with db = db.query(sql`
    select id, text
    from todos
    order by id desc
    limit ?`, limit)
```

### Safe SQL

Statements are prepared and values are sent apart from the SQL, so a value can never
change what a query does. `quillo check` warns where SQL is put together from values
(`${x}` or `+`): pass them as `?` parameters instead.

Names cannot be parameters. When a table or a column comes from a choice (a column to
sort by, say), put it in with `sql_name`, which takes only letters, digits and `_`, or
a list of such names:

```quillo
fn sorted(by: "text" | "id") with db = db.query(`select id, text from todos order by ${sql_name(by)}`)
```

SQL built only from text as written, `sql_name` and `sql_param`, and choices between
those, is not warned about.

### Rows as records

A `type` names the shape of a row, and an annotation checks rows against it as they
come back. A `User(row)` call makes one, keeping only its fields, which is the way to
turn a row into what the rest of the program expects:

```quillo
type Todo = { id: int, text: str }

fn todos() -> list<Todo> with db = db.query("select id, text from todos order by id")
```

See [Records](/docs/language-tour/#records) for what a record checks and how it reports
what is wrong.

### Transactions

`db.transaction(f)` runs `f` and commits what it did, all or nothing:

```quillo
fn transfer(from: int, to: int, amount: int) with db =
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
- On PostgreSQL and MySQL a transaction keeps one connection until it ends, so other
  requests never see its changes early. On SQLite, `sleep` and `http` calls inside a
  transaction hold up other requests rather than let them write into it.
- `parallel` inside a transaction runs its functions one after another, since queries
  on other threads would not be part of it.
- Live pages hear of its changes when it commits, and not at all if it rolls back.

### Migrations

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
  `_quillo_migrations` with its SHA-256.
- A file that fails is rolled back and stops the program from starting, with the file's
  name and the database's message.
- A recorded file whose text changed afterwards gets a warning, not a second run: add a
  new file to change the schema again.
- Servers that start together take turns (an advisory lock on PostgreSQL, a named lock
  on MySQL, an immediate transaction on SQLite), so each file still runs once.
- MySQL commits a schema change at once, inside a transaction or not: a file that fails
  part way keeps what it did before the failure. Put one change in each file.

### Live data

A page can follow the database. `live(load)` in a component runs a `server fn` again
whenever a table it read changes, through Quillo, for anyone:

```quillo
server fn load_todos() = db.query("select id, text from todos order by id")
server fn add_todo(text: str) = db.exec("insert into todos (text) values (?)", text)

component Todos() {
    state todos = live(load_todos)      // every open page sees new todos
    <ul>{for t in todos => <li key={t.id}>{t.text}</li>}</ul>
}
```

The tables a loader reads are found in its SQL (`from`, `join`), and every `insert`,
`update` and `delete` through `db` announces the tables it writes. This works the same on
SQLite, PostgreSQL and MySQL. Changes made by other programs (`psql`, a script) are not
seen. See [Live data](/docs/components/#live-data), and
[Several servers](/docs/components/#several-servers) for apps on more than one server.

### Several databases

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
  connection, not a new pool.
- A transaction belongs to its database: one inside another on a different database
  commits or rolls back on its own.
- Live updates tell the databases apart, and a connection never reaches the browser
  (its URL may hold a password).
- Mounted programs and services have their own `db`, so one Quillo can serve programs
  on different databases side by side.

### Read replicas

A PostgreSQL or MySQL database's replicas can take its reads:

```quillo
db.open(env("DATABASE_URL"), { replicas: [env("REPLICA_1"), env("REPLICA_2")] }) | unwrap
```

- Reads (`select`, and `with` that writes nothing) go to the replicas in turn.
- Writes, reads that lock (`for update`, `for share`), everything in a transaction, and
  every read a request makes after it wrote go to the primary: a request always reads
  what it wrote.
- A replica that fails is left out for ten seconds, and its reads go to the primary
  meanwhile. Every replica must be reachable when the program opens the database:
  otherwise `db.open` gives an error value, which `unwrap` turns into a stop.
- Another request may still read from a replica that has not caught up with a write made
  just before. Reads that cannot wait for that belong in a transaction.
- `db.connect(url, { replicas: [...] })` takes them too. Replicas are for one database
  given by its URL: not SQLite, and not a database per tenant.

### Shards

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

### Joining across databases

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

### One transaction over several databases

`db.transaction` is all or nothing on one database. `db.atomic` is all or nothing over
several:

```quillo
fn pay(from: int, n: int) =
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

### Quillo's own tables

Quillo keeps a few tables of its own in the program's database: scheduled jobs
(`_quillo_jobs`, each run once, whichever server it is due on), calls made later
(`_quillo_later`, see [Microservices](/docs/microservices/#calls-made-later)), the
decisions of `db.atomic` (`_quillo_atomic`) and the migrations applied
(`_quillo_migrations`).

With a database per tenant there is no one program database to keep them in, so the
first three go to a system database: `db.system(target)` (one every server of the program
shares, when there are several), else `$QUILLO_SYSTEM_DB`, else a SQLite file
`quillo-system.db` next to the program. `db.system` also keeps them out of the program's
own tables when there is one database.

### A database per tenant

One program can serve many tenants (customers, teams, sites), each with a database of
its own:

```quillo
tenant_from("example.com")          // acme.example.com is tenant "acme"
db.open("data/{tenant}.db")         // a SQLite file per tenant
db.migrate()                        // run for each tenant's database when it is made

route POST /signup { form: { name: str } } {
    with_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))   // makes acme.db
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

- A tenant's database is made, and migrated, the first time `with_tenant(name, ...)`
  runs for it, as when it signs up. Requests only open databases that exist: a made-up
  host gets an error from `db`, and no file or schema is made for it.
- `db.tenant_exists(name)` says whether a tenant has a database, without making one.
  `db.tenants()` lists the tenants whose databases exist, for a SQLite, PostgreSQL schema
  or MySQL template; where a function says where tenants are, keep the list yourself.
- Live updates stay within a tenant. Global variables are shared by every tenant, so
  keep a tenant's data in its database.

[Many tenants](/docs/standard-library/#many-tenants) has the rest: tenant names, schema
prefixes, and how many databases stay open at once.

### Engines

#### SQLite

Built in, nothing to install: the database runs inside the server process, in a file
next to the program or in memory, with foreign keys on and WAL mode. Booleans are
stored as `0` and `1`.

#### PostgreSQL

- Placeholders: `?`, `:name` with a map, or `$1`. The first two are translated, skipping
  quotes, comments and `$$` strings.
- Values convert both ways: numbers, text, booleans, `json` and `jsonb` as maps and
  lists, arrays as lists, `numeric` as numbers; `uuid`, `date`, `timestamp` and
  `timestamptz` (ISO 8601, in UTC) and `interval` as text. Other types are sent as text
  for PostgreSQL to convert.
- `last_id` is `null`: use `insert ... returning id` with `db.one`.
- Connections come from a pool, 8 unless the URL says `?pool_size=N`. A query lets other
  requests run while it waits, and a request's queries share a connection, pipelined.
- TLS is used when the server offers it, and `?sslmode=require` insists, with
  certificates checked as hosted PostgreSQL services expect.

#### MySQL and MariaDB

- Placeholders: `?`, or `:name` with a map (translated, skipping quotes, backticks and
  comments).
- Values convert both ways: numbers, text, `tinyint(1)` as booleans, `decimal` as
  numbers, MySQL's `json` as maps and lists (MariaDB's JSON is text: `json.parse` it),
  dates and times as ISO text. `last_id` is the new row's `auto_increment` id.
- Connections come from a pool, and a query lets other requests run while it waits.
- Built in by default as the `mysql` feature; `cargo build --no-default-features`
  leaves it out.

### Reference

| Function | Does |
|---|---|
| `db.open(target)` | Opens the program's database: a path, `:memory:`, a URL, a template with `{tenant}`, or a function of the tenant |
| `db.connect(target)` | Opens another database, with these same functions |
| `db.query(sql, ...values)` | Every row, as a list of maps |
| `db.one(sql, ...values)` | The first row, or `null` |
| `db.exec(sql, ...values)` | `{ changes, last_id }` |
| `db.transaction(f)` | Runs `f`; commits, or rolls back on failure, error value or early reply |
| `db.migrate(folder = "migrations")` | Applies the SQL files not yet applied, in name order |
| `db.open(url, { replicas })` | Sends reads to replicas (also `db.connect`) |
| `db.shards([target, ...])` | Data split by key: `.on(key)`, `.all(sql, ...)`, `.migrate()`, `.count` |
| `db.atomic([db, ...], f)` | Runs `f` as one transaction over several databases |
| `db.system(target)` | Where Quillo keeps its own tables |
| `db.tenants()` | The tenants that have a database |
| `db.tenant_exists(name)` | Whether a tenant has a database, without making one |
| `sql_name(name)` | A table or column name, safe to put into SQL |
| `sql_param(name)` | A `:name` placeholder, safe to put into SQL |
| `join_on(rows, others, key, other_key, as)` | Each row with its first match from another list of rows |

A function declared with effects may use the database only if `db` is among them: one
declared `with http` cannot query, and `quillo check` says so before the program runs.
Functions without `with` may do anything.
