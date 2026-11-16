# Feedback from designing a dev-only database tool: migration SQL made on a web page, with the new table structure and the table's data shown

| | |
|---|---|
| App | None: a design study. The question: should a module that exists only in development give a web page that manages migration SQL, creating and altering tables and columns, and what would it take |
| Quillo | `0.8.0 (c293883, 2026-11-07)`, the release build. `src/` last changed in 701aa69, so it is the same as main's 5562c7a. The exception is the last two probes (plugins, `quillo install`): another session rebuilt `target/release/quillo` during them, so `probe7.sh` ran on `0.8.0 (5562c7a)` with its uncommitted FEEDBACK61 changes to `check`, `builtin_docs`, `runtime.js` and `styles`, and `probe6.sh` may have too. Neither probe depends on those files, but neither was repeated on the clean build |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check`; `quillo run` with and without `--watch`; `curl`; SQLite as bundled with rusqlite 0.32; PostgreSQL 17.11 and MariaDB 12.3.3 on private servers made for this report on their own ports and stopped after it (the shared servers on 5432 and 3306 were not touched). Six catalog plugins, built with cargo: `glob`, `file_tree`, `sql_parse`, `sql_format`, `diff`, `git_cli`. `quillo install` from the live catalog. No browser: no page was built |

No Quillo source was changed, and no tool was built: the user asked for this report and no code. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section. Numbers start at Q-915, after the last ID in the repo, Q-902. The probe programs are in `.git/wip/feedback62/` on this Mac.

**Revised the same day.** The first version called listing a folder a gap in core without having run a plugin, and the user pointed at the catalog. Running them changed the conclusion: the sections on plugins, Q-921, the order of work and question 5 were rewritten, and most of the core work moved after the first version.

**This report was asked for while CI was red.** Main's head, 5562c7a, failed CI run 42384904061, and FEEDBACK61 is unfixed. A new run on that head was in progress when this was written. Under the rule of 2026-11-05, nothing here is built until main is in sync, every test (CI too) is green and the docs are aligned.

The request, as it arrived in four messages:

| Said | Read as |
|---|---|
| A new `/dev/db.qwem` module plugin, only available in dev mode, a web application to manage migration SQL visually, including creating and altering tables and columns | A page for schema changes that never exists in production |
| It can be added as `.qwe`, which can reside in the repo | A plain Quillo file in the app's repository, so no new file type is needed. If "the repo" meant Quillo's, see [question 5](#5-how-does-the-tool-reach-an-apps-repo) |
| It will only manage migration SQL for now | The only thing it writes is migration files |
| It can show what the new table structure is, and display table data as well | It reads: the structure a change gives, and rows |

What worked well: most of this tool can already be written. `db.migrate()` run from a second program applied a new file, recorded it, and the running app saw the table at once. `reply` inside `db.transaction` rolls back, so a page can run a migration, read the new structure and rows, and leave nothing behind, on SQLite and PostgreSQL alike. Five of the twelve changes tried are the same SQL on all three databases. `conn.sql_name` quotes for its own connection (`"order"`, `` `order` ``). `--with` starts a tool beside the app and stops it with the app. The plugin catalog already holds most of what the tool needs, and `quillo install --example` already delivers a plugin with a program. A migration that fails on a reload keeps the previous version serving. MariaDB's refusal of a schema change in a transaction (Q-728) says why, in plain words. The gates built in FEEDBACK51 and before held: another site's form gets a 403, a rebinding `Host` a 421.

## The answer

**Worth building, as a `.qwe` program of its own that uses catalog plugins, not as a module the app takes, and not as `.qwem`.** A tool the app never loads cannot reach production, which no other form tried can promise: `use dev/db.qwe` answered and wrote a file with no `--watch` at all, and a program has no way to tell whether `--watch` is on (Q-920). A separate program also stays up when a broken migration stops the app from starting, which is when it is needed most.

| | |
|---|---|
| Writes | Draft `.sql` files in `migrations/drafts/`, and a finished file into `migrations/`. Nothing else |
| Applies with | `db.migrate()`, the one path production uses. The tool never runs a schema change of its own, so nothing can exist in development that production will not get |
| Reads | The structure now, the structure a draft gives, and rows, 100 at a time |
| Runs as | Its own program on its own port, on 127.0.0.1, under `--watch` |
| Costs the app | Nothing: no route, no `use`, no line in `main.qwe` |

**One finding matters today, whether or not the tool is built.** SQLite's own documented way to change a column, put in a migration, deleted every row of another table while `db.migrate` reported success (Q-915). Anyone changing a column on SQLite reaches for that procedure, so it comes first.

**A first version needs one small change in core.** Built from `.qwe` and catalog plugins, it can already list the migrations and give each its state, show structure and rows, check a draft, preview it on all three databases, and apply it. Each of those was measured below. The one addition is `platform().dev` (Q-920), so that it can refuse to run outside development.

What is left for later, or for a decision:

1. **Six of the twelve changes differ by database,** and SQLite has no statement at all for four of them (Part C). The first version leaves those to SQL typed by hand. Under the rule that differences are the driver's job, forms for them get their SQL from the driver.
2. **The tool's reads branch by database for now.** Structure takes a different query on each, and preview a different method. That is tolerable inside one tool. `db.schema()` (Q-922) and a preview call move them into the drivers later, where any program gains them.
3. **One migrations folder speaks one dialect.** `quillo new`'s own first migration applies on all three databases and then fails at the first insert on two of them (Q-916). A tool on a SQLite development database would write files that do this, and no plugin can check for it (Part B). That needs a decision ([question 2](#2-which-database-does-it-write-sql-for)).

## Part A: where it lives

Each form was run, except the `.qwem` and `dev use` rows, which do not exist.

| Form | Sees the app's database | In production | When a migration stops the app at start | Verdict |
|---|---|---|---|---|
| `use dev/db.qwe` in `main.qwe` | Yes, the same connection | **Yes.** Without `--watch` it answered 200, and a POST wrote `002_prod.sql` into `migrations/` | Gone: 500, "The program did not load" | No |
| The same behind `routes /dev/db \| dev_only`, an `env` guard | Yes | Hidden (404) unless the variable is set, which has nothing to do with `--watch` | Gone | No |
| `mount /dev/db = "dev/tool.qwe"` | **No.** A mount has its own database: it listed no tables | Yes, like `use` | Not tried | No |
| `dev/db.qwem`, a new file type | As `use` | As `use` | As `use` | No: it adds cost and solves nothing |
| **Its own program, `quillo run dev.qwe --watch --port 4700`** | Yes: it opens the same target | **No. Nothing starts it; a deployed copy is an unused file** | **Stays up** | **Yes** |
| A new form, `dev use dev/db.qwe`, read only under `--watch` | Yes | No | Gone | The fallback ([question 1](#1-does-the-tool-run-as-its-own-program)) |

The files:

```
main.qwe            // not changed
dev.qwe             // one line: use dev/db.qwe
dev/db.qwe          // the tool
dev/glob.qwer ...   // the plugins it uses (Part B)
migrations/
    drafts/         // the tool's drafts; db.migrate reads only files directly in the folder
```

`dev.qwe` is there because paths stay inside the program's folder. Started as `dev/db.qwe`, the tool's folder is `dev/` and `write_file("../migrations/x.sql")` is refused. Started through `dev.qwe` at the top, `write_file("migrations/...")` works, and `db.migrate()` found the folder (both measured). Installed from the catalog, the tool would sit beside `main.qwe` and need no launcher ([question 5](#5-how-does-the-tool-reach-an-apps-repo)).

Two path rules bit here. A `use` inside `dev/db.qwe` is read from the program's folder, so it says `use "dev/glob.qwer"`. And a path given to a plugin is read from the folder the process started in, not the program's: started from `/`, `glob_files("migrations", ...)` answered `migrations: not a folder`. So the tool is started from the app's folder.

Starting both together:

```sh
quillo run main.qwe --watch --with "quillo run dev.qwe --watch --port 4700"
```

This was measured in two halves: `--with` started a tool and stopped it with the app, and `quillo run dev.qwe --watch` wrote and applied a file. The whole line was not run. A shorter spelling can come later.

**The tool itself needs `--watch`,** for three things that exist only there: the `Host` gate (421 for a rebinding name), the database's own words (outside `--watch` every failure in the matrix below read `internal error`), and reloading. Until Q-920, it cannot refuse to run without it.

What "only in dev mode" then rests on:

| Promise | Kept by | State |
|---|---|---|
| The app never serves it | No line in the app | Holds by construction |
| Production never starts it | Nothing runs `dev.qwe` | Holds by construction |
| Only this machine reaches it | 127.0.0.1 by default, and a check of `request.ip` in the tool | To build (Q-918) |
| Only under `--watch` | `platform().dev` | Needs Q-920 |
| Never applied to a shared database | Apply refused unless the target is a file or a local address | To build (Part E) |

**The target.** The tool must open what the app opens. `db.open(env("DATABASE_URL"))` works by itself. The sample's literal `db.open("app.db")` cannot be learned from outside, so the tool reads `DATABASE_URL`, falling back to `app.db`, and the page shows which it found. `db.connect` databases, tenants and shards are left for later: version one is one database and one folder.

## Part B: what the page does

| View | Shows | Needs |
|---|---|---|
| Migrations | Every file, as applied, waiting, changed since it ran, missing, or out of order | `glob`, `crypto.sha256`, `_quillo_migrations`: works today |
| Tables | Each table's columns, keys, indexes and foreign keys | A query per database now, `db.schema()` later (Q-922). Columns were measured; indexes and foreign keys were not tried |
| Change | Forms: create a table; add, rename or drop a column; add an index. The SQL is shown and can be edited before saving | Part C |
| Preview | The structure before and after, side by side, and rows as they would read | Part D |
| Data | A table's rows, paged, read only | Part E |

The steps of a change:

1. A form, or SQL typed by hand, makes a draft: `migrations/drafts/007_add_sku_to_items.sql`.
2. Preview shows the new structure and rows. Nothing is kept.
3. Save moves the draft into `migrations/`, and Apply calls `db.migrate()`.
4. The file goes into git like any other.

**Drafts have to stay out of `migrations/` itself.** A file there is applied the next time any `.qwe` is saved, since `db.migrate()` runs again on each reload. A waiting file would run before anyone previewed it. Only files directly in the folder count, so a subfolder is safe (`files()` in `src/migrate.rs` does not descend).

The rules that follow from how migrations already work:

- Only drafts can be edited. A file in `migrations/` is never changed by the tool, since a recorded file whose text changes is only warned about.
- The number is the highest there plus one, padded, and the name comes from the change.
- One change per file, as the guide asks because of MySQL. A MariaDB file that fails part way keeps what ran: `half_one` stayed, unrecorded, so the corrected file then meets a table it left (measured, and the error says so).
- There is no undo, because there are no down migrations. Preview is what makes that bearable.

### What the plugins already do

All run on a folder with one migration in each state:

| From | In the tool | Measured |
|---|---|---|
| `glob` | `glob_files("migrations", ["*.sql"], [])`: the migrations, sorted | The five files, and not `drafts/` |
| `file_tree` | `file_tree("migrations", 1, false)`: the same with sizes and dates | Yes; `drafts` shows as a folder |
| `crypto.sha256` (a builtin) | `crypto.sha256(read_file(path))` against `_quillo_migrations.sha256` | The hashes match what `db.migrate` recorded |
| `sql_parse` | `sql_tables`: what a file touches, and each statement's kind, so a `drop` can be flagged | `alter_table` on `items`, `drop` on `tags` |
| `sql_parse` | `sql_valid`: a typing mistake caught before saving | `create tabel` refused in all three dialects. Not a dialect check: see below |
| `sql_format` | `sql_pretty`: readable generated SQL | Yes, keeping the case it was given |
| `diff` | `unified_diff`: the structure before and after | Yes |
| `git_cli` | `git_cli_status`: migrations not yet committed | Modified, deleted and untracked files under `migrations/`, drafts too |

The five states take a page of plain Quillo, and all five came out right:

```quillo
fn states() {
    let files = glob_files("migrations", ["*.sql"], [])
    let rows = db.query("select name, sha256 from _quillo_migrations")
    let out = []
    let last = ""
    for r in rows {
        if r.name > last { last = r.name }
    }
    for f in files {
        let found = null
        for r in rows {
            if r.name == f { found = r }
        }
        let state = "waiting"
        if found != null {
            state = crypto.sha256(read_file("migrations/" + f)) == found.sha256 ? "applied" : "changed"
        } else {
            if f < last { state = "out_of_order" }
        }
        out = out + [{ name: f, state: state }]
    }
    for r in rows {
        let there = false
        for f in files {
            if f == r.name { there = true }
        }
        if !there { out = out + [{ name: r.name, state: "missing" }] }
    }
    out
}
```

```
001_items.sql changed      002_tags.sql applied      004_four.sql out_of_order
005_five.sql applied       006_six.sql waiting       003_gone.sql missing
```

**`sql_valid` cannot say a file suits another database.** It is too lenient for that, as the real answers from Part C show:

| SQL | `sql_valid`: SQLite, PostgreSQL, MySQL | The real databases |
|---|---|---|
| `alter table t alter column b type varchar(50)` | no, yes, no | Agrees |
| `alter table t modify column b varchar(50)` | **yes, yes,** yes | No, no, yes |
| `alter table t alter column b set default 5` | **yes,** yes, yes | No, yes, yes |
| `alter table t add constraint t_fk foreign key (id) references o (id)` | **yes,** yes, yes | No, yes, yes |
| `create tabel x (id integer)` | no, no, no | Agrees (a typing mistake) |

So it guards against typing mistakes only. A dialect check still takes that database.

## Part C: the changes, on each database

A table `t (id integer primary key, a varchar(50), b integer)` with two rows, one with `b` null; each change ran on a fresh table, under `--watch` so that the real reasons show.

| Change | SQLite | PostgreSQL 17.11 | MariaDB 12.3.3 |
|---|---|---|---|
| Add a nullable column | same SQL | same SQL | same SQL |
| Add `not null` with a default | same SQL | same SQL | same SQL |
| Rename a column | same SQL | same SQL | same SQL |
| Drop a column | same SQL | same SQL | same SQL |
| Rename a table | same SQL | same SQL | same SQL |
| Add `not null`, no default, rows there | Refused | Refused | **Accepted: rows get `''` and `0`** (Q-916) |
| Drop an indexed column | **Refused**: `error in index t_a` | Accepted | Accepted |
| Change a column's type | **No statement** | `alter column b type ...` | `modify column b ...` |
| Set `not null`, a null row there | **No statement** | Refused, `contains null values` | Refused through `modify`, `Data truncated` |
| Set a default | **No statement** | `alter column b set default` | Same as PostgreSQL |
| Add a foreign key | **No statement** | `add constraint ...` | Same as PostgreSQL |
| A unique index over duplicates | Refused | Refused | Refused |

So the first five can be forms at once, and the last row is the same SQL everywhere, with a refusal for the page to show. The six between differ, and on SQLite the four with no statement need a rebuild: make a new table, copy, drop the old one, rename. That is Q-915's procedure. For a plain type change, adding a column, copying, dropping and renaming kept every row in the test, with no table dropped, but it moves the column to the end and cannot change a constraint.

**Where the SQL comes from.** The five shared changes need no choice. For the others, a `.qwe` generator would hold three branches per change, which is a program choosing per database, the thing the database layer exists to remove. So the first version leaves them to SQL typed by hand, and forms for them come with the driver, as a builtin that takes a description of the change. That also keeps the door open for the portable forms that the design report planned on 2026-11-05 is to propose.

## Part D: showing the new structure

| Database | How | Measured |
|---|---|---|
| SQLite | Run the draft in `db.transaction`, read, and let `reply` roll it back | Yes: the new column and its rows were seen, and the column was gone afterwards. `vacuum into` a copy also worked |
| PostgreSQL | The same | Yes: a new column and a type change, both gone afterwards |
| MariaDB | Refused inside a transaction (Q-728). Replay `migrations/` plus the draft into a scratch database | Structure yes, as `root`. Rows, and a user without `create database`: not tried |

```quillo
route GET /preview {
    db.transaction(() => {
        db.exec(read_file("migrations/drafts/007.sql"))
        reply 200 {
            after: db.query("select name from pragma_table_info('items')"),
            rows: db.query("select id, sku from items limit 3"),
        }
    })
}
```

The first version can do this in `.qwe`, as above. **Later it belongs in one call,** `db.migrate(folder, { preview: true })`, giving the files it would run, the structure before and after, and any failure as a value, with nothing kept. Each driver then does what suits it, and the three-way split leaves the tool. It also solves Q-923 and Q-925 for the tool.

Two costs to accept. A preview holds SQLite's write lock, and on PostgreSQL an `alter table` blocks the app's queries on that table until the rollback (from the databases' documentation, not measured). Both are short on a development database. And `vacuum into` puts a full copy of the data in the project folder, so rollback is the better default.

From memory, not checked for this report: Prisma replays migrations into a shadow database, as MariaDB would need here; Django's SQLite editor refuses to run with foreign keys on inside a transaction; Alembic has a batch mode for SQLite's rebuild. Every tool of this kind has had to solve Part C's rebuild and Q-915.

## Part E: table data

**Read only by construction, not by a setting.** The tool has no SQL box and no route that writes a row. Its only read is `select * from <name> limit ? offset ?`, with the name through `conn.sql_name` and an order by the primary key when there is one.

A database-level read-only mode was tried and is not needed for this, but it does not exist across the board: `pragma query_only = on` refused a write on SQLite, `set transaction read only` refused one on PostgreSQL (25006), and on MariaDB it failed with ERROR 1568 inside `db.transaction`, **so nothing stopped the update after it**. If a SQL box ever comes, it needs a driver option first.

What the grid has to handle, measured on a row with each kind of value:

| Value | SQLite | PostgreSQL | MariaDB | The grid |
|---|---|---|---|---|
| `decimal(12,2)`, 9.90 | `"9.9"` | `"9.90"` | `"9.90"` | Shows what it is given (Q-494) |
| `timestamp` | `2026-11-08 10:00:00` | `2026-11-08T10:00:00` | `2026-11-08 10:00:00` | Same (Q-494) |
| `json` | text | a map | text | Same (Q-494) |
| Blob | **broken text** | `"\\x00ff"` | **broken text** | Its size only, until Q-924 |
| 9007199254740993 | exact | exact | exact | Exact in the JSON; through a `server fn` by the rule of 2026-11-05 (not measured here) |

Also: long text is cut in the grid, and columns named like `password`, `token` or `secret` are hidden until asked for. Rows are real data even in development.

**A `.env` can name a shared database.** The page shows the target at the top, and Apply is refused unless the target is a file or a local address. The rest of the page works, since it only reads.

The page itself should keep clear of two open bugs from FEEDBACK61: a grid row keeps one root element (Q-889), and it does not use `live()`, since a tool in another process is not heard by live updates anyway. It reloads after Apply.

## Part F: requests

Against a route that writes a file, under `--watch`, with the headers another site's page would send:

| Request | Answer | For the tool |
|---|---|---|
| A form post from another site | 403 | Safe |
| `text/plain` from another site | 403 | Safe |
| JSON from another site | 200 if it arrives. `OPTIONS` gets a 204 with no `access-control-*`, so a browser does not send it (by the fetch rules; no browser was used) | Safe in a browser |
| A `server fn`, as `text/plain` | 415 | Safe |
| **A `GET` that writes, as an `<img>` would** | **200, and the file was written** | Never write on a `GET` |
| `Host: evil.example` | 421 | Safe under `--watch` only |
| **`Host: 192.168.1.50:8080`** | **200** | Q-918 |

So every write is a `server fn`, and no token is needed on loopback. The tool builds a file's name itself and never takes it from the request, so a request cannot choose what gets written.

## Part G: what not to build

- Editing rows, a SQL box, or seeds. Out of scope for now, and each brings Part E's read-only question back.
- Down migrations. They would be a second system beside the SHA record.
- `use`, `mount` or `routes` forms of the tool, for the reasons in Part A.
- `.qwem`. `use` adds `.qwe` to it (`cannot read dev/db.qwem.qwe`), and 27 files across `quillo-editors` name `.qwe`, besides the parser, modules, `check` and the LSP.
- Tenants, shards and `db.connect` databases in version one.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-494 | One row reads three ways, so a grid shows three forms of a value (Part E) | 1 |
| Q-663 | Its gate held for a name, and let any address through (Q-918) | 1 |
| Q-728 | Working as decided, and the reason preview cannot be one method on all three databases | 1 |
| Q-889, Q-899 | Not hit, as no page was built; they limit how the grid is written | 0 |

## New issues

### Q-915. A table rebuilt in a SQLite migration deletes or blanks rows of other tables, and `db.migrate` says it worked

| Kind | Detected by | Attempts |
|---|---|---|
| silent, data loss | Running SQLite's documented steps as a migration | 1 |

| Impact | |
|---|---|
| Functionality | A migration that reports success removes or blanks rows in a table it never names. It cannot be run again or undone |
| User | A developer who follows SQLite's own page sees nothing wrong. The loss shows when someone notices the missing rows, perhaps in production |
| Requests | No change |
| Security | No new exposure. It is a loss of data and of links between rows |

```sql
-- 001.sql
create table authors (id integer primary key, name text);
create table books (id integer primary key, author_id integer references authors(id) on delete cascade, title text);
insert into authors (id, name) values (1, 'Ana'), (2, 'Bo');
insert into books (id, author_id, title) values (1, 1, 'A'), (2, 1, 'B'), (3, 2, 'C');

-- 002.sql
pragma foreign_keys = off;
create table authors_new (id integer primary key, name varchar(80) not null default '');
insert into authors_new (id, name) select id, coalesce(name, '') from authors;
drop table authors;
alter table authors_new rename to authors;
pragma foreign_keys = on;
```

| `books.author_id`, and how 002 is written | After 002 | `db.migrate` |
|---|---|---|
| `on delete cascade`, as above | **All 3 books gone** | `["001.sql", "002.sql"]` |
| `on delete set null`, as above | **All 3 `author_id`s null** | `["001.sql", "002.sql"]` |
| No action, as above | Unchanged | Stops: `FOREIGN KEY constraint failed` |
| `on delete cascade`, `pragma defer_foreign_keys = on` instead | **All 3 books gone** | `["001.sql", "002.sql"]` |
| No action, `pragma defer_foreign_keys = on` instead | Unchanged | Stops: `FOREIGN KEY constraint failed` |
| `on delete cascade`, add, copy, drop and rename a column, no table dropped | All 3 kept | `["001.sql", "002.sql"]` |

Why: `src/migrate.rs` runs each file inside `begin immediate`, SQLite ignores `pragma foreign_keys` inside a transaction, and `src/db.rs:658` turns foreign keys on for every connection. So `drop table` does its implicit delete with them on, and the actions fire. `pragma foreign_keys` read 1 afterwards. Nothing in the guide warns of it.

**Suggestion:** do what the file says. When a file sets `pragma foreign_keys = off` (found by the tokenizer in `src/sql.rs`), `migrate` turns them off before `begin immediate`, runs `pragma foreign_key_check` before the commit, rolls back on any violation, and turns them on after. Only files that say so change, and today that line does nothing. See [question 3](#3-q-903-does-a-migration-do-what-pragma-foreign_keys--off-says).

### Q-916. A migration written for one database applies on another, then fails at the first insert

| Kind | Detected by | Attempts |
|---|---|---|
| silent, then loud | Running `quillo new`'s own migration on all three | 1 |

| Impact | |
|---|---|
| Functionality | `db.migrate` succeeds and the table cannot take a row without an id. MariaDB also accepts a `not null` column the others refuse, and invents its values |
| User | A program made on SQLite and deployed on PostgreSQL fails at its first write, not at its migration. This breaks the promise that the choice of database is not the developer's |
| Requests | The first request that inserts gets a 500 |
| Security | No change |

```sql
create table todos (id integer primary key, text text not null, done integer not null default 0);
```

| Database | `db.migrate` | `insert into todos (text) values (?)` |
|---|---|---|
| SQLite | applied | `{ changes: 1, last_id: 1 }` |
| PostgreSQL | applied | `null value in column "id"` |
| MariaDB | applied | `Field 'id' doesn't have a default value` |

And adding `c varchar(50) not null` to a table with rows: SQLite and PostgreSQL refuse. MariaDB accepts it, and with `n integer not null` added the same way the row reads `{ c: "", n: 0 }`.

**Suggestion:** the portable forms planned on 2026-11-05 are the real answer, a portable `id` first. For the tool, see [question 2](#2-which-database-does-it-write-sql-for). The MariaDB fill belongs to the driver under the rule of Q-736.

### Q-917. Nothing says a migration ran out of order, shares a number, or went missing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | Reading `_quillo_migrations` in the order applied | 1 |

| Impact | |
|---|---|
| Functionality | Two databases built from one folder can have run its files in different orders, and differ if those files touch the same table |
| User | Two branches that each add a migration merge with no word. A deleted file leaves no trace in the log |
| Requests | No change |
| Security | No change |

Under `--watch`, adding `006_six.sql`, then `005_five.sql`, then `007_a.sql` and `007_b.sql`, then deleting the applied `002_x.sql`:

```
applied order: 001_init, 002_x, 006_six, 005_five, 007_a, 007_b
log:           "applied migration ..." for each; no word on the order, the number or the deletion
```

A fresh database would run 005 before 006. Only a changed file gets a warning.

**Suggestion:** warn when a waiting file sorts before the last applied one, and when a recorded file is gone, as a changed file is warned of. They stay warnings, since a merge that lands out of order is often fine. The tool works out the same states itself (Part B), so this is for everyone who does not run it.

### Q-918. Under `--watch`, any address passes the `Host` check, and `--host 0.0.0.0 --watch` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | `curl` with a `Host` header; reading `local_host` | 1 |

| Impact | |
|---|---|
| Functionality | No change to what the server does |
| User | A developer who opens the server to test from a phone is not told that everyone on the network can now reach it, with errors shown in full |
| Requests | A request naming any IP address is answered: 200 for `Host: 192.168.1.50:8080`, against 421 for `evil.example` |
| Security | Anyone on the same network reaches the program's routes and server fns while it runs under `--watch`. A tool inside the app would add schema changes to that. Q-663's fix covers names, as it was meant to |

`local_host` (`src/server.rs`) takes every name that parses as an address. The log for `--host 0.0.0.0 --watch` says only `listening on http://0.0.0.0:...` and `watching for changes`.

**Suggestion:** a line at start under `--watch` on a non-loopback address. It should stay allowed, since testing from a phone is a good reason. The tool itself keeps to 127.0.0.1 and checks `request.ip`.

### Q-919. A new migration file does not reach a running `--watch` program

| Kind | Detected by | Attempts |
|---|---|---|
| silent | Adding a file and asking for the tables | 1 |

| Impact | |
|---|---|
| Functionality | The program runs on an old schema until a `.qwe` file changes |
| User | After a `git pull` that brings a migration, pages fail on a missing column, with no sign of the reason |
| Requests | Requests that need the new schema fail until a reload |
| Security | No change |

`002_x.sql` written, 2.5 s later: no table `x`. After `touch main.qwe`: table `x`. `watched_files` holds the program's files, and a `.sql` is not one.

**Suggestion:** under `--watch` only, also watch the folder given to `db.migrate` (`site.migrations` already holds it). Not under `--reload`. This is safe with the tool only because its drafts stay in a subfolder (Part B).

### Q-920. A program cannot tell it runs under `--watch`, so a file it takes is in production

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, security | Comparing `platform()` with and without `--watch` | 1 |

| Impact | |
|---|---|
| Functionality | No form makes a route exist only in development. `if` around a route is refused (`routes must be declared at the top level`) |
| User | A developer who writes `use dev/db.qwe` has a tool that looks dev-only and is not |
| Requests | Its routes answer in production |
| Security | With no sign in, a POST to such a route wrote a `.sql` file into `migrations/` on a server without `--watch`. It would run at the next start |

`platform()` gave byte-identical output in both modes.

**Suggestion:** `platform().dev`, true under `--watch`. A small change, and the tool uses it to refuse to run outside development. See [question 1](#1-does-the-tool-run-as-its-own-program) for the larger form.

### Q-921. Listing a folder takes a plugin, so the tool needs Rust on the machine, or a built copy

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | Running `glob` with and without cargo on the `PATH` | 2 |

| Impact | |
|---|---|
| Functionality | `glob` and `file_tree` list a folder well. Core has no listing of its own, so a program that needs one is more than a `.qwe` file |
| User | A developer without Rust gets a clear refusal. With it, the first build of six plugins took 18 s here, with their crates already downloaded, and start-up with the build kept took 2.4 s |
| Requests | No change |
| Security | No change. A general listing builtin would be a new way to see a program's files, which this avoids |

The first version of this report titled this "a program cannot list a folder" and asked for a builtin, while naming these two plugins and not running them. That was wrong: they do the job.

Without cargo:

```
quillo: ./glob.qwer:1: cannot build ./glob.qwer: cargo did not run (No such file or directory (os error 2)). Install Rust (https://rustup.rs), or build it where Rust is with `quillo build` and bring .quillo/native along
```

That advice works: a copy with `dev/.quillo/native/` (8.6 MB for the six) ran and answered with no cargo on the `PATH`.

**Suggestion:** no builtin. Use the plugins, and treat the Rust build as the tool's one real cost. If Rust on a developer's machine is too much to ask, the catalog can carry built libraries for the common platforms, as the site's `docsearch` is prebuilt for each release.

### Q-922. A program cannot ask for its tables, or which database it has

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check`: `` `db` has no `kind` `` | 1 |

| Impact | |
|---|---|
| Functionality | Showing a table's structure takes a different query on each database, and the program cannot choose one |
| User | The answers differ too: `notnull: 0` against `is_nullable: "YES"`; `INTEGER`, `integer`, `int`; a default of `null` against the text `"NULL"` |
| Requests | No change |
| Security | No change |

SQLite needs `pragma_table_info`, PostgreSQL `information_schema` with `table_schema = 'public'`, MariaDB the same with `database()`, and indexes and foreign keys differ again.

The tool can get by without it: it opened the target, so the text of the target tells it the kind, and the three column queries above all worked. So this does not block a first version.

**Suggestion:** `db.schema()`: `{ tables: [{ name, columns: [{ name, type, null, default, primary }], indexes, foreign_keys }] }`, the same shape from every driver, through the driver interface of FEEDBACK43. Not `db.kind`, which would invite programs to branch. `quillo check` could use the same model for the tables-and-columns checks discussed on 2026-11-05.

### Q-923. `quillo check` says a failed `db.migrate` gives an error value, but it stops the request

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | A route with `is_err` after `.migrate()` | 1 |

| Impact | |
|---|---|
| Functionality | A program cannot handle a failed migration: the line after it never runs |
| User | `check` accepts `?` and `is_err` on the result, which do nothing. A tool's Apply gets a 500, whose text holds the reason under `--watch` (measured), so the page can show that |
| Requests | A 500, reading `internal error` outside `--watch` |
| Security | No change |

```quillo
route GET /apply {
    let m = db.migrate("mig_fail")      // a 500 here
    is_err(m) ? "failed" : "ok"         // never reached
}
```

`src/check.rs:880` gives `list | error(str)`, and `migrate` in `src/db.rs:1768` uses `fail`.

**Suggestion:** correct the signature to `list`, since stopping is right at start (Q-602). The preview form returns failure as a value (Part D).

### Q-924. A blob reaches JSON as broken text

| Kind | Detected by | Attempts |
|---|---|---|
| silent | A route answering a row | 1 |

| Impact | |
|---|---|
| Functionality | Bytes are lost in any JSON answer. `x'00ff10'` reads `"\u0000�\u0010"` on SQLite and MariaDB, and `"\\x00ff"` on PostgreSQL |
| User | A program that answers rows with a blob sends text that cannot be turned back into the bytes |
| Requests | The body of any such answer |
| Security | No change |

**Suggestion:** one form for bytes from every driver, as part of Q-494. Until then the grid shows a blob's size.

### Q-925. A scratch database cannot be made twice

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | A second `db.connect(":memory:")` | 1 |

| Impact | |
|---|---|
| Functionality | The second `db.connect(":memory:")` is the first connection, with its tables, as the guide says, and the guide lists no way to close one. So a replay from empty works once per process |
| User | Each `vacuum into` copy needs a new name and keeps its connection until the program stops |
| Requests | No change |
| Security | Copies of real data pile up in the project folder if a page does this by hand |

**Suggestion:** nothing of its own if the preview moves into the driver (Part D). Otherwise a `.close()`.

### Q-926. `quillo check` says nothing of a `write_file` whose error is dropped

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `ls` after a route said `{ ok: true }` | 1 |

| Impact | |
|---|---|
| Functionality | A refused write goes unnoticed, and the code after it runs |
| User | It cost this report a wrong reading: a route said ok and no file existed |
| Requests | A 200 for a write that did not happen |
| Security | No change |

```quillo
route POST /w {
    write_file("../migrations/x.sql", "x")     // refused: outside the program's folder
    { ok: true }                               // answered; check says "ok (1 route...)"
}
```

**Suggestion:** a `check` warning when a call that can give an error value stands as a statement. The checker knows these signatures. The cost is noise in programs that mean to ignore one, so measure it on `app/` first.

## Proposed order of work

Nothing starts before the rule of 2026-11-05 is met. Each step carries tests, edge tests and the book, as the standing rules ask.

| Step | What | Where | Gives |
|---|---|---|---|
| 1 | Q-915 | `src/migrate.rs` | Safe rebuilds, tool or no tool |
| 2 | `platform().dev` (Q-920) | Core, small | The tool can refuse to run outside development |
| 3 | **Tool, first version:** all five views, drafts, Apply, and forms for the five shared changes | `.qwe` and catalog plugins | All that was asked, except forms for changes that differ |
| 4 | Q-917, Q-918, Q-919, Q-923 | Core: warnings, a watched folder, a signature | Better migrations for everyone, with no tool |
| 5 | `db.schema()` (Q-922), `db.migrate(folder, { preview: true })` | Each driver | The tool's per-database reads and previews move into the drivers, for any program |
| 6 | Changes that differ, from the driver; SQLite's rebuild | Each driver, then forms | All of Part C |

Steps 1 to 3 are the smallest thing worth having. Q-916's portable forms, Q-924 and Q-926 stand apart from this order.

## Open questions for the user

### 1. Does the tool run as its own program?

**The tool should be a separate program, which gives up being a module the app takes.** This comes up the moment a developer writes `use dev/db.qwe`: measured, it answered and wrote a file with no `--watch`, and it died with the app when a migration broke at start. As a separate program, nothing in production ever loads it.

| | Own program (recommended) | `dev use dev/db.qwe`, a new form |
|---|---|---|
| **Functionality** | Works today. Stays up when a migration stops the app | New syntax in the parser, `check` and the editors. Gone when the app fails to load |
| **User** | One more port, and the target comes from `DATABASE_URL` | `/dev/db` in the app, with no setting |
| **Requests** | Its own port; the app's routes do not change | Its routes join the app's under `--watch`, and can clash with a program's `/dev` |
| **Security** | Nothing to reach in production. A mistake in a production command cannot expose it | Depends on one check of `--watch` being right forever |

What the separate program loses: the in-app address, and the app's `db.connect` databases, which it cannot see.

**My recommendation:** its own program, with `dev.qwe` and `dev/db.qwe`. The trade-off is one more thing to start, which `--with` covers and a shorter spelling can cover later.

Do you accept it as a separate program rather than a module? If not, I will specify the `dev use` form instead, with the "gone when the app fails to load" case handled.

### 2. Which database does it write SQL for?

**The tool should write for the database it is connected to, and say so, until portable forms exist.** This comes up for anyone on SQLite in development and PostgreSQL in production: measured, `id integer primary key` applied on all three and failed at the first insert on two.

| | Write for the connected database (recommended) | Also ask for a production target | Wait for portable forms |
|---|---|---|---|
| **Functionality** | Correct for that database. The file names its dialect in a comment | Correct for the target, but previewed on another database, which proves little, and `sql_valid` cannot check it (Part B) | Right everywhere, and needs a design report first |
| **User** | A warning when the production database differs, if it is told | One more setting | No tool until then |
| **Requests** | No change | No change | No change |
| **Security** | No change | No change | No change |

**My recommendation:** the connected database now, and a portable `id` as the first form in the report planned on 2026-11-05. The trade-off is that a SQLite-to-PostgreSQL project still needs care until then, which is no worse than today.

Is it acceptable that version one writes one dialect? If not, the tool waits for that design report, and I will move it ahead of step 3.

### 3. Q-915: does a migration do what `pragma foreign_keys = off` says?

**Yes, it should, because today that line does nothing and rows are lost.** This happens in any SQLite migration that rebuilds a table other tables point at: measured, three books deleted with `db.migrate` reporting success.

| | Honour the pragma (recommended) | Refuse the pragma with an error |
|---|---|---|
| **Functionality** | SQLite's documented procedure works, with `foreign_key_check` before the commit | No loss from that pragma, but rebuilds stay impossible, and a file without it still loses rows |
| **User** | Their file does what it says | A clear message, and no way to do the job |
| **Requests** | Requests wait on the migration as they do today | No change |
| **Security** | No change | No change |

Nothing good is lost: the line has no effect today. A file that does not say the pragma keeps today's behaviour, so a warning for `drop table` of a table others reference is worth adding beside it.

**My recommendation:** honour it, and add the warning. The trade-off is one special case in `migrate.rs`, on SQLite only.

May I plan it this way? If not, I will plan the refusal, and the tool will not offer changes that need a rebuild on SQLite.

### 4. Does the tool apply migrations, or only write them?

**The tool should have an Apply button, calling `db.migrate()` and nothing else.** "Only manage migration SQL" could mean it stops at the file. This comes up after every change: without Apply, the developer saves a `.qwe` to make the app migrate.

| | Apply button (recommended) | Writes files only |
|---|---|---|
| **Functionality** | Same path as production. Measured from a separate program: applied, recorded, seen by the app | Applied at the next reload of the app |
| **User** | One step, with the failure shown in the page, from the 500's text under `--watch` (Q-923) | A second step elsewhere, with the failure only in the app's log |
| **Requests** | One `server fn` | None |
| **Security** | Needs the local-target check of Part E | Cannot change any database, which is the strongest position |

**My recommendation:** the button, refused unless the target is local. The trade-off is that the tool can change a database, though only through a file that then goes into git.

Do you want Apply in the tool? If not, it stays a writer and reader, and Part E's target check goes away.

### 5. How does the tool reach an app's repo?

**As a plugin like the other 628: a `.qwer` with the tool as its example program, which `quillo install` already delivers.** This comes up as soon as someone wants it in a repo. Measured from the live catalog, `quillo install glob --example --dir i` wrote `glob.qwer` and `glob.qwe`, and a plugin's `files` entries come too (`install_extras` in `src/install.rs`). So `quillo install db_tool --example` needs no change to `install`, and it is the "module plugin" of the request, in the form plugins already have.

| | Catalog plugin with its example (recommended) | In `quillo new`'s template | Kept in Quillo's repo, run by the binary |
|---|---|---|---|
| **Functionality** | Versions and `--update` exist. `db_tool.qwe` lands beside `main.qwe`, so it writes `migrations/` with no launcher | Written at `quillo new` only; existing apps get nothing | Always matches the binary, but a `.qwer` still needs building |
| **User** | One command, and the file is theirs to read and change. Needs Rust or a built copy (Q-921) | Every new app carries it, wanted or not | No file to keep up to date, and none to change |
| **Requests** | No change | No change | No change |
| **Security** | The `.qwer` is checked against the catalog's hash. The example and `files` are written as downloaded, which is worth closing if the tool ships this way | No change | Nothing added to the app's repo |

What it loses: the name `dev/db`. Extras land flat in the folder, so the tool is `db_tool.qwe` beside `main.qwe`. Keeping `dev/db.qwe` means placing it by hand, with Part A's `dev.qwe` launcher.

**My recommendation:** the catalog, tested by `plugins/test.mjs` like the others, with one `db_tool.qwer` naming the crates it needs, so that there is one build and not six. The trade-off is repeating a few lines of `glob`, `sql_parse` and `diff`.

Is the catalog the right home, with the tool beside `main.qwe`? If not, I will plan on `dev/db.qwe` placed by hand, with its launcher.

## Smaller notes

- `use /dev/db.qwe` reads the machine's `/dev` and says `cannot read /dev/db.qwe`. So the file is `dev/db.qwe`, with no leading slash. `mount` takes any extension: a `.qwem` file checked ok there.
- `sql_name` takes only letters, digits and `_`, so a table named otherwise (made by hand, in quotes) is listed but its rows cannot be shown.
- A program on `db.open(":memory:")` has nothing a second program can open.
- `db.migrate`'s failure on MariaDB says in its own words that earlier changes stay, which the page can show as it is.
- A quick search found no table or grid component in the website's `.qwe` files. The kit branches were not searched, so check them before writing the grid.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `write_file("../migrations/...")`, then `{ ok: true }`, in `dev/studio.qwe` | `db.open("../app.db")` had worked, so I assumed paths behaved alike | The write was refused, my route dropped the error, and the answer said ok. `ls` showed it. My mistake, and Q-926 | 1 wrong reading |
| A bad migration would take an in-app tool down | It stops the program, says the guide | On a reload the previous version serves; only a fresh start fails. The claim had to narrow | 1 more probe |
| `db.kind` in a batch of probes | It seemed a natural name | One `check` error stopped the whole run. It became Q-922 | 1 rerun |
| The matrix without `--watch` | Forgot that errors are marked | Every failure read `internal error`, so no row could be trusted | 1 rerun |
| A foreign key to an empty table | Careless setup | It failed for the data, not for the statement | 1 rerun |
| `to_str(x)`, `{ [k]: v }` | Habits from other languages | Unknown name; the parse error pointed at `set(m, k, v)` | 2 reruns |
| Called listing a folder a gap in core, naming `glob.qwer` and `file_tree.qwer` and not running them | Looked for a builtin first, and assumed a Rust build ruled plugins out | Wrong: the user pointed at the catalog, and the plugins did it. Three more of my core additions turned out not to be needed for a first version | A rewrite of the plan |
| `use "glob.qwer"` inside `dev/db.qwe` | Expected a path from the file | Read from the program's folder: six errors | 1 rerun |
| `local_path("migrations")` to make a full path | Guessed from the name | It checks a site address, and gave null | 1 probe |

## Edge cases checked that behave well

- A failed migration on a reload: "still serving the previous version", and it recovered once the file was gone.
- A file that fails rolls back whole on PostgreSQL.
- `--with` stops the tool with the app.
- `db.migrate` from a second process: recorded once, not run again by the app.
- `conn.sql_name` quotes per connection, and refuses a backtick.
- A changed applied file gets its warning and does not run again.
- MariaDB's refusal of `alter table` in a transaction names the reason and what to do.
- `.migrate()` into a scratch MariaDB database applied the folder.
- `quillo check` read six plugins' signatures without building them.
- The refusal with no Rust says what to do, and doing it works.

## Scripts

All in `.git/wip/feedback62/`: `probe1.sh` (placement, gating), `probe2.sh` and `probe3.sh` (`--watch`, rebuilds, read side, requests), `probe4.sh` (the matrix, on private servers), `probe5.sh` (rollback, half-run files, the launcher), `probe6.sh` and `probe7.sh` (the plugins, no Rust, `quillo install`), `matrix.qwe`, `read-side.qwe`. Each checks its ports first and stops only what it started. The SQL of the matrix, on `t (id integer primary key, a varchar(50), b integer)`:

```sql
alter table t add column c varchar(50);
alter table t add column c varchar(50) not null;
alter table t add column c varchar(50) not null default 'n';
alter table t rename column a to a2;
alter table t drop column a;
create index t_a on t (a); alter table t drop column a;
alter table t rename to t2;
alter table t alter column b type varchar(50);
alter table t modify column b varchar(50);
alter table t alter column b set not null;
alter table t modify column b integer not null;
alter table t alter column b set default 5;
create unique index t_u on t (a);
alter table t add constraint t_fk foreign key (id) references o (id);
```
