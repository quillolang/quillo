# Changelog

## Unreleased (0.8.11)

Found studying how a provider's policy holds on shared and edge hosting (FEEDBACK82.md, Q-1280
to Q-1286), a scan of what components and plugins reach, confirmed once per project
(FEEDBACK83.md, Q-1300 to Q-1308), a routing system for single-page apps and each page's head
(FEEDBACK84.md, Q-1320 to Q-1330, with FEEDBACK81.md's Q-1275), and moving a 3D animation
studio onto Quillo (FEEDBACK85.md, Q-1340 to Q-1348). The user decided the forms on
2026-11-21 and 2026-11-22: `page_head`, not a `<head>` block; routes moved in place as an
opt-in; `[run]`, `[fs] paths` and `[env] set`; `QUILLO_POLICY_PATH` with `include` and
`require_path`; a broken layer stops; every file scanned, held, unconfirmed refused, the catalog
included, a complete `quillo.confirmed` asked by a provider's policy (not by `--policy`, revised
2026-11-23), a warning in this release
and a stop from 0.8.12; under a `quillo.confirmed`, a name not taken an error (2026-11-24);
`save:` on the HTTP calls; per-function effects lines that only narrow;
`quillo mcp --serve`. Q-1343 is answered in the guide (the catalog's `cli_run` and
`ffmpeg_cli`).

### Security

- **What a program's files reach is confirmed once, and held.** A component file taken from
  another project ran whole, with the program's rights: its top level at the start (one posted
  `.env` to another server) and its routes, and `quillo check` said ok. `quillo confirm` reads
  every file of the program (`use`, `mount` and `service` followed) and writes
  `quillo.confirmed`: by file, the hosts its `http.*`, `site`, UDP and database calls reach
  (`http`), the paths it reads (`files`) and writes (`writes`), the UDP ports others reach
  (`listens`), its pages' scripts with their `sha384` hashes, what its markup loads and its
  pages call, the routes, `server fn`s and tools a file taken for some of its names adds, and
  each Rust file by SHA-256. `quillo run`, `mcp`, `app`, `build` and `--split`'s services (by
  the app's file, never the environment) are held to it: an unconfirmed host or path is an
  error value at the call, said once; a taken file's unconfirmed route, `server fn` or tool is
  not served; an unconfirmed script is left out of the page; a Rust file not confirmed at its
  hash does not load, and the start stops. Paths are judged where they really are, and hosts as
  the `url` crate and each database driver read them (`http://127.0.0.1:9\@good.example/`,
  `postgres://good/app@evil`). Three reviews' attacks each have a test (FEEDBACK83.md, Q-1300,
  Q-1301).
- **The hold, as the coverage tests and later reviews tried to get round it** (Q-1300,
  Q-1301): a path is judged where the system takes it, each link followed before the `..`
  after it, and a `..` after a name that is not there is refused (a link and a `..` wrote
  outside the confirmed folder, and made a tenant's database there); a builtin handed on as
  a value (`map(xs, read_file)`, `let get = http.get`) is held to the file that names it,
  not to the code that calls it; SQLite's `attach` and `vacuum into` are held to the running
  file's `writes`, counted from the process's folder (a file confirmed for nothing read
  another app's database and copied its users out), with `file:` names, names the SQL works
  out and `temp_store_directory` refused; a taken file's `mount` and `service` lines are
  listed and held as its routes are (its `mount /admin` replaced the main file's); `quillo
  mcp`'s stdio door, alone or beside `--serve`, offers only confirmed tools; `quillo.confirmed`
  takes only the escapes `quillo confirm` writes (`\u{+2e}` confirmed `.env` unseen), its
  temporary file is never written through a link, and one that is a link is said at start.
- **A taken file's calls for the whole site are held** (Q-1301, found in review): a kit taken
  for `Badge` called `allow_cross_site("/")` and `site_headers`, and a cross-site `POST` to the
  main file's routes got through, its frame and CORS headers changed. A new key `site`,
  listed as `site-wide`, holds a taken file's `allow_cross_site`, `site_headers`,
  `tenant_from`, `moves_in_place`, `accept_calls`, `style_with`, `channel`, `media`, `ready`,
  `locale_from` and `translations`, its `db.open` and `db.system`, which replaced every
  file's database, and `db.migrate()` on it: not applied unless confirmed. `pragma
  lock_proxy_file` is refused under a
  hold; a migration's files are read, and its statements held, as the calling file's;
  `quillo fix`, `check` and `confirm` say what an import hands a file of another's reach
  (`kit/k.qwe takes main.qwe's reader (read_file, as main.qwe reaches it)`).
- **A taken file sets nothing for the visitor unless confirmed** (Q-1301, found in review): a
  kit's view called `set_session({ user: { role: "admin" } })` and every visitor became an
  admin. A new key `session`, listed as `visitor's`, holds a taken file's `set_session`,
  `end_session`, `set_cookie NAME`, `delete_cookie NAME`, `mcp_token` and
  `revoke_mcp_token`, and the headers of its `reply`, which ended the main file's request
  with any cookie, CORS or `location`: unconfirmed, its status and body go out without them.
- **Push and Quillo's own tables are held** (Q-1300, found in review): a taken file wrote its
  own endpoint into `_quillo_push`, and `notification.send` carried the users table there.
  `notification.send` asks the hold for each push service's host, for the file that sends
  (`quillo confirm` lists the browsers' push hosts for it), and under a hold a taken file's
  SQL never touches Quillo's own `_quillo_*` tables, however it names them.
- **A taken file's SQL that runs SQL from text is refused** on PostgreSQL and MySQL/MariaDB,
  under a `quillo.confirmed` (the user's decision of 2026-11-24; Q-1300, found by the
  coverage tests): `DO`, `PREPARE`/`EXECUTE`, `EXECUTE IMMEDIATE`, `CALL`, `CREATE
  FUNCTION`/`PROCEDURE`/`TRIGGER`/`RULE`/`EVENT`, the `*_to_xml` functions and `regclass`
  from text reached `_quillo_*` by names built at run time (`U&"_quillo..."`). The statement
  is read as the database reads it, so a word in a string, comment or quoted name is not
  refused. A connection is held to the file that opened it: a connection's map a taken file
  built by hand reached a helper's database. A tenant's migrations run as the code that set
  them, and a migration file is judged as the text that runs, at the path the system says it
  opened. The reading fails closed (found in review): what it cannot read for certain is
  refused (`/*!`, a string left open, a compound statement), `EXECUTE`, `CALL` and `PREPARE`
  anywhere, a `CREATE` of anything but a table, index, view or sequence, SQL reaching the
  server's files (`COPY`, `INTO OUTFILE`, `pg_read_file`), and the catalogs holding other
  tables' values or SQL text (`pg_stats`, `pg_prepared_statements`, `performance_schema`). A
  `--` comment ends at `\r` too, also for the main file's transaction check (`select 1 --\r;
  begin` left a transaction open on a shared connection). A taken file's statement begins
  only with `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `WITH`, `VALUES`, `TABLE`, `REPLACE`,
  `EXPLAIN`, a listed `CREATE` or `ALTER`, `DROP` of a table, index, view or sequence, or
  `TRUNCATE` (`GRANT ... ON ALL TABLES IN SCHEMA`, `DROP SCHEMA ... CASCADE` and `INSTALL
  SONAME` reached them); `SET` of any kind is refused, as `SET NAMES gbk` let a character
  take the backslash after it, and so is a non-ASCII byte just before a `\` or a backtick; on
  MariaDB only InnoDB, Aria, MyISAM and MEMORY tables, without file or connection options; on
  PostgreSQL any `pg_` name; a function that takes a table (from `pg_proc`: `nextval`,
  `setval`, `has_table_privilege`, `row_security_active`, the `brin_*` ones and the rest) only
  with the name written as a string, not a column, a number (an oid) or a parameter; on
  MariaDB a `[` outside strings, which `sql_mode=MSSQL` reads as a quoted name. A taken
  file's statement is judged as its server is sent it too (a value's place written `$1`
  after a `$` opened a dollar quote the reading never saw), the `mysql` and `sys` schemas
  and the catalogs by every spelling (quoted, and the letters outside ASCII MariaDB takes for
  ASCII ones), and PostgreSQL's own extensions' functions that reach a table by text by
  name (`xpath_table`, pageinspect's, `pgstattuple`, amcheck's, pg_surgery's); on SQLite a
  taken file runs no `ANALYZE`, makes no trigger and reads no statistics (`sqlite_stat4`
  held Quillo's own rows' values). A column named `engine` is no table option, MySQL's
  `explain (select ...)` runs, and the refusals say what they refuse (found by the coverage
  tests of 3117dea). A MySQL table's engine is judged where the server takes a table's
  options, not by the word before it (`alter table after engine = csv` changed a table to
  CSV), and on SQLite a taken file makes no virtual table over `dbstat` (found by the review
  of a65644e); the SQL a kit ordinarily writes runs under a hold on SQLite, PostgreSQL and
  MariaDB, as a test of 151 statements shows. On SQLite a taken file's pragma that sets (`=
  v`, `(v)`, quoted, `main.`) is refused and its reads run: a kit left the main file's
  connection with its journal off and `query_only` on; Quillo's read-only transactions and
  its foreign-keys turn in migrations set theirs as its own. A taken file makes no SQLite temporary table, view or
  index (a kit's `create temp table kits` stood for the main file's `kits`, and a visitor's
  insert landed in it), its SQLite migrations begin or end no transaction and set no
  `foreign_keys` (a kit's migration left them off for the main file), and its PostgreSQL SQL
  names no `reg*` type (`'{kits}'::_regclass` too); `merge`, `refresh materialized view`,
  `setval('s', ?)` and a column named `call` run (found by the live review of 726b006). A
  taken file's SQLite migration is held where SQLite runs it: no transaction statement of its
  own runs (`select :a(x');commit;pragma foreign_keys=off;begin;...` hid a commit from the
  reading: parameters are read as SQLite reads them now), and Quillo's own transaction and
  foreign-keys switch are its own SQL; on PostgreSQL the functions that make a `reg*` value
  are refused by name and a table's argument is counted past `[]` and `{}` (found by the
  live review of b3a2998).
- **Under a `quillo.confirmed`, each file uses only the names it takes** (the user's decision
  of 2026-11-24): a name another file defines, used without taking it, is an error in
  `quillo check`, and the start refuses it with `--no-check` too, as the value under it
  carried what that file was confirmed for; a record made from another file's `type` and a
  page state holding a function are refused alike. Without one it stays a warning. `quillo
  fix` writes those imports there too, and says what they leave for `quillo confirm`.
- **A link that leads nowhere, inside a folder `--allow-path` names**, made a file outside
  every folder named, with or without a hold; it is refused, as one in the program's folder
  is (found by the coverage tests).
- **What a terminal acts on is shown escaped**: `quillo check`, `quillo fix`, a run's and a
  reload's errors, the MCP door, `quillo confirm`'s listing and the hold's messages write a
  file's name and the program's text with control characters, DEL, C1, U+2028/9 and the bidi
  marks as `\u{1b}` and the like (a path holding `ESC[1A ESC[2K` erased a line just before
  the question).
- **A confirmed plugin loads only a library this machine built.** A `.quillo/native` a clone or
  an archive brought along no longer loads behind a confirmed source: what was built is
  recorded outside any project (`~/.cache/quillo/built.txt`, 0600, the user's alone), and a
  library not recorded is built apart, from the bytes confirmed, once per change. Only a
  provider's layers may pin a prebuilt library by its hash (FEEDBACK83.md, Q-1300).
- **A plugin can be pinned by its hash.** `[plugins] allow = ["qr@sha256:<hex>"]`: by the
  library's SHA-256 only that library loads; by the source's, the plugin is built here from the
  very bytes hashed, in a fresh 0700 folder outside the program's reach, with a cleared
  environment and `PATH`'s cargo, and loaded from a private copy whose hash is checked.
  `[plugins] catalog` pins a whole catalog by its index. A name alone let any file called
  `qr.qwer` load (FEEDBACK82.md, Q-1285).
- **A developer's `.env` no longer steers the host's policy or Quillo's builds.** A `.env`
  could set `QUILLO_POLICY_PATH`, which a `--split` service then read as a host's layer, and
  `LD_PRELOAD`, `CARGO`, `RUSTFLAGS`, `HOME` or `PATH`, which reached every process started
  from the program and a pinned build. `QUILLO_POLICY_PATH` is never taken from a `.env`, and
  under a host's layer a `.env` that sets any of those names stops the start (FEEDBACK82.md,
  Q-1281, found in review). `QUILLO_ENV_FROM_FILE`, Quillo's own note to the services
  `--split` starts, is believed only there, never for a `QUILLO_POLICY*` name, and is given
  to no `--with` command: set in the environment, it dropped the provider's
  `QUILLO_POLICY_PATH` layer and let a `.env` replace the variables it named. A name the
  host's `[env] set` forces is never a `.env`'s, a service's own included (Q-1280, Q-1281,
  found by the coverage tests).
- **Moves in place never cross what a page load would.** A page swapped in place keeps the
  current page's scripts and headers, so a move is a full load when the two differ in the
  program or mount, its version, the head's scripts, the content security policy, frame,
  permission or referrer headers, or the visitor (an HMAC under `QUILLO_SECRET`); a move's
  fetch is read-only, so a `GET` that writes runs once, and is never cached (FEEDBACK84.md,
  Q-1323, found in review).
- **A save is a write**: an `http.*` call with `save:` is never made by a prefetch, a hover's
  guess or a move in place (a `Sec-Purpose: prefetch` request wrote the file in review), and a
  saved file keeps its mode, never setuid, setgid or sticky (FEEDBACK85.md, Q-1342).

### Changed

What a program may notice:

- **An `[effects] allow` that lists the seven effects of 0.8.10 now refuses a Rust file with no
  `//! effects:` line.** There is an eighth, `run` (starting another program), so such a list
  no longer allows every effect, and a file that says nothing may do anything. Add `run` to the
  list, or an effects line to the file (FEEDBACK83.md, Q-1308).
- **`db.migrate(dir)`'s folder is judged as a `read_file` path**, with or without a
  `quillo.confirmed`: inside the program's folder or one `--allow-path` names, links followed,
  and each `.sql` file in it too. It read and ran SQL from any folder, so migrations kept
  outside the program (`db.migrate("../shared/migrations")`, or a link out of the folder) now
  stop the start until it is given `--allow-path ../shared` (Q-1300, found in review).
- **In `quillo.confirmed`, `files` is read-only**: a path written or deleted (`write_file`,
  `delete_file`, `save_upload`, `save:`, a SQLite file and what its SQL attaches) is confirmed
  under `writes`, and is read too, with no `files` entry. The folders of `db.migrate` and
  `translations` are `files` entries.
- **New builtins `page_head`, `page_url` and `moves_in_place`**: a program's own function of
  that name still wins, with a warning, `` `page_url` hides the builtin `page_url` `` (rename
  it). quillolang.org's own `page_url` is one.
- **A policy layer that is there but broken stops the program**, exit 2, naming it: a
  `quillo.policy` that was a link to nothing, a folder or unreadable was skipped, and the
  program ran with no policy (FEEDBACK82.md, Q-1283).
- **On Windows the host's file is `%ProgramData%\quillo\policy.toml`**, found through the
  system's known folder: it was `\etc\quillo\policy.toml` on whichever drive the program ran
  from (Q-1282).
- **`quillo app` runs under the policy**, `--policy` too, and a phone app under its
  `quillo.policy`: neither read one, and `--policy` was taken and ignored (Q-1284).
- **`files()` serves a link into a folder `--allow-path` names**, as `file()` does; elsewhere
  it stays a 404, now with `file()`'s log line (FEEDBACK85.md, Q-1340).
- **Under a provider's policy, a program with no `quillo.confirmed`, or one that leaves
  something out, is warned of at start and in `quillo check`**, naming 0.8.12, from which it
  will not start (FEEDBACK83.md, decisions 5 and 6). Only a provider's layers ask (the host's
  file, what it includes, `policy.d`, `QUILLO_POLICY_PATH`); a file given with `--policy` is
  the developer's, as `quillo.policy` is, and does not (the user's revision of 2026-11-23).
- **A program with `quillo.confirmed`**: `quillo check` exits 1 on what it leaves out, and
  `quillo build` builds no Rust file it does not have at its hash.
- **Moves in place are opt-in**: without `moves_in_place()`, links, `url()` and `navigate`
  work as in 0.8.10, but for `navigate`'s new options, Back's scroll and the focus after a
  move (below).
- **Speed**: against 0.8.10 built the same way, whole-process instructions, best of 3 to 9.
  `quillo check` of quillolang.org's site takes 6.2% fewer: check asks the main file's real
  path once, the parser looks at a token without copying it, and the table of what a hold
  stands in for (4,096 addresses every process wrote into place as it started) is made in a
  held run only. A program starts with what it did (102.0 M to its first answer). vs-python's
  `int histogram` takes 9.3% fewer and `word frequency` 2.2% fewer: a counted map's two ints
  are added as `eval` adds them. `group json` takes 0.6% more, as the map library's table
  code is compiled apart from its callers since the code grew; the other 23 cases are within
  0.6%. A request costs what it did (in process, `Interp::handle`, best of 8): text -0.4%,
  json +0.2%, fortunes 0.0%, db -0.1%, compute -0.1%, a page 0.0%, a stream -0.4% (median); no
  per-request code runs without a `quillo.confirmed`, a policy or `moves_in_place()`. With a
  `quillo.confirmed`: `check` +2.9% and a start +3.2% (3.2 M: what the files reach, read and
  judged), a page +1.5 k a request (+3%, its `content-security-policy-report-only`), a JSON
  answer as without. `page_head` costs a page 12 k over the same tags written by hand.
  Runtime: a counter page +79 B (+32 B gzipped); a page with `url()` or `navigate` +3,862 B
  (+1,170 B gzipped) (Q-1300).

### Added

- **`quillo confirm [main.qwe] [PATH...]`**, with `--yes`, `--list` and `--effects FILE...`,
  and the file `quillo.confirmed` (see Security). A terminal is asked (`y`, `f` file by file,
  `n`), as is `quillo run` at one; elsewhere `confirm` exits 1, and CI runs `quillo check`,
  which fails on what is not confirmed. Held pages carry `integrity` on confirmed scripts and a
  `content-security-policy-report-only` of what was confirmed, where the program sets none
  (FEEDBACK83.md, Q-1300 to Q-1304).
- **`quillo install` keeps what it checked**: the SHA-256 and effects line go into the
  project's `quillo.confirmed` when it has one, and a copy whose only change is an added
  effects line still updates (Q-1306, Q-1307).
- **The effect `run`**, for a Rust file that starts other programs; effects are 16 bits now
  (Q-1308).
- **A provider's policy holds `quillo run`** (FEEDBACK82.md, Q-1280): `[run]` off switches
  (`with`, `watch`, `describe`, `plain_http`, `split`, `join`), forced values (`check = true`,
  `log`, `base`, `listen = "unix"`, `trust_proxy`, which `--trust-proxy` may only narrow) and
  ceilings (`instances`, `max_connections`, `connections_per_client`, `live_per_client`,
  `drain`, `socket_mode`; a connection a policy's ceiling closes names its file); `[fs]
  paths`, the ceiling of `--allow-path`; `[env] set`, from the host's layers only, ahead of
  any `.env` and the shell.
- **More places for the host's layers** (Q-1281): the anchor's `include = [...]`,
  `policy.d/*.toml` beside it (`.TOML` too), `QUILLO_POLICY_PATH` (a list), then `--policy` and
  `quillo.policy`; `require_path = true` stops a start without the variable; a provider's build
  can compile its anchor in (`QUILLO_HOST_POLICY`).
- **`quillo policy [main.qwe] [--policy FILE]`**: each layer, the places with none, and each
  value in force with its file; exit 1 when a layer asked for is missing, 2 when one is broken
  (Q-1286).
- **`page_head({ title, description, image, canonical })`** writes the page's `<title>`,
  description, `og:` tags and canonical link from any view or component, the innermost call
  winning, on the server and, following state, in the browser (FEEDBACK84.md, Q-1320, Q-1321;
  FEEDBACK81.md, Q-1275). The page is read as the browser reads it, checked against parse5:
  the tags go where it puts the head, before the text or element that ends it early, and an
  `<svg>`'s, a `<template>`'s or a script's `<title>` stays. They go before the head's first
  `<noscript>`, and its title, description or canonical is cut, so a crawler without scripts
  reads exactly one of each. A tag the page has is known by its `name` or its `property`, with
  its character references read as the browser reads them (`name="&#100;escription"` is the
  description; found in review).
- **`page_url()` and `page_url(path)`**: the page's public address, with its scheme (https only
  through a trusted proxy), host and `--base` prefix (Q-1328).
- **`moves_in_place()`**: same-site links and `navigate` move between the program's routes
  without a page load; page states, and a component on both pages with the same props, keep
  their state; the head is diffed and the body swapped (Q-1323, Q-1327; decision Q3).
- **`navigate(path, { replace, scroll })`**; Back and Forward give the scroll back; after a
  move to another path, focus goes to `<main>` or the first `<h1>` and the title is said; a link
  pressed while its page is on the way waits for it (Q-1324 to Q-1326, Q-1330).
- **`save:` and `max_bytes:` on `http.get`, `delete`, `post`, `put`, `patch` and
  `request`**: a 2xx body streamed to a file (64 KiB at a time, renamed over the target once
  whole, 64 MiB at most unless given), giving `{ status, ok, headers, file, bytes }`; the path
  held as `write_file`'s; needs `fs` as well as `http`; `save: null` is no option, on
  `http.stream` too (FEEDBACK85.md, Q-1342).
- **A plugin function's own `/// effects:` line** narrows its file's for that function; a
  wider one is an error at check and at load (Q-1344).
- **`quillo mcp main.qwe --serve [--port N]`**: the program's routes served beside the stdio
  door, in one process, held to the policy as `quillo run` is; a tool's call runs as a request
  does, so one that waits (`sleep`, `http.get`) holds up no route (Q-1347).

### Fixed

- **A folder is one folder whatever name it is given** (Q-1285, Q-1340, Q-1300): on macOS
  `/System/Volumes/Data/private/x` is `/private/x`. A pinned plugin was built in a temporary
  folder the program could write when `--allow-path`, `[fs] paths` or the program's folder
  named it by the other name, and under `--allow-path /`. `--allow-path` by the long name
  refused the paths it allows; `delete_file` by the other name deleted the program's folder or
  its database's; `db.migrate()` of a program started by the long name failed (0.8.10 applied
  it). A pinned source that builds another file in (`global_asm!`'s `.include`, `include!`)
  is refused without a `quillo.confirmed` too. `delete_file` of a folder judges what is in it
  by the files themselves: a folder holding the program's files or a database it has open is
  refused whatever name it is given (`/System/Volumes/Data`, `/.nofollow/...`, a bind
  mount), and a folder in it that cannot be read stops it before anything is deleted (it
  deleted what it could). `/.nofollow/x` names `/x`. The program's `.quillo` folders (built
  plugins, `plugins.lock`, the session key) are its own files: `write_file`, `delete_file`,
  `save_upload` and the save options refuse them by any name, as they refuse `main.qwe`
  (found by the review of b7f7c26; an upload path a visitor steers could plant a library for
  the next start), and by where they really lead (found by the review of 0284f7c): a
  `.quillo` that is a link (`.quillo -> qstore`, or one out of the folder into an
  `--allow-path` folder, as an upload's name reached), a link inside one, a hard link or a
  bind mount of one; a link in it to the program's folder or above is no folder of Quillo's.
  Deleting a folder that holds a link to a folder above the program says so; on macOS
  `.quıllo` (dotless ı) is another name, as APFS compares names. Then simpler and wider (found by the
  review of 9da2632: past its 4,096-entry walk the guard let everything through): a path
  with a `.quillo` part (any ASCII case on macOS and Windows), or one under where the
  program's own `.quillo` folders lead (a link, a link to what is not there yet), is refused
  to `write_file`, `delete_file`, uploads, saves and `db.open`; a `.quillo` that leads to the
  program's folder or above stops the start; Quillo reads its libraries, `plugins.lock`, the
  session key and cookie jars through no link below `.quillo` and only from a file with one
  name. Writing onto a folder fails as the system says; only `delete_file` judges what a
  folder holds. Deleting a folder no longer removes a link the program's code,
  `--env-file` or its database is used through, works on trees of any depth, and judges a
  FIFO beside a journal's name without waiting on it; `files()` no longer hangs on one.
- **`read_file`, `write_file` and `delete_file`'s refusal of a path outside the folder** ends
  with the way out: ``; `--allow-path ..` (on `quillo run` or `quillo mcp`) lets the program
  read it`` (Q-1341). Refused for a link, it names the link and where it leads, with the
  folder to allow, or that it leads nowhere: it suggested the `--allow-path` already given.
- **`quillo fix` into a closed pipe** (`quillo fix main.qwe | head -1`) exits 141 without a
  word, as a script's `print` does: it panicked after writing its files.
- **`files()` refused a folder reached through a link without a word**, whatever
  `--allow-path` said (Q-1340).
- **The MCP door's refusal of a request without `_meta`** names both keys and what they hold,
  to copy (Q-1346).
- **A `--checked` given by hand skipped a policy's forced check**, and `quillo build` and the
  editor's `cargo check` built under `build = false` (found in review of Q-1280).
- **Back to a page a form's `POST` answered** is the browser's own, and posts nothing again;
  a link to a page whose key differs loads it (found in review of Q-1323).

### `quillo check`

- **A component inside `<head>`** is an error, with `page_head` as the way (Q-1322);
  `{json.stringify(x)}` as a JSON-LD script's whole content is warned of (Q-1329).
- **A written host the policy's `[http] hosts` refuses** is an error at the call, where it was
  an error value with no log line at run time (FEEDBACK83.md, Q-1302).
- **A file taken for some of its names that adds routes or tools, or reaches out at start**, is
  warned of (Q-1301); **a Rust file whose effects line leaves out what its code plainly calls**
  (`std::process::Command` without `run`) is warned of, a reading, not a proof, that follows
  `use ... as` names, `use std::{fs::*}` and `r#fs`, a type in brackets (`<(T)>::`) and an
  `impl`'s type, `::` and `!` however spaced, and
  `include_str!` and the like as `fs`; what it cannot follow (a path a macro builds, a
  foreign block, `asm!`, a `use` nested past 64) reads as every effect but `db` (Q-1305).
  Under a `quillo.confirmed` a Rust file that builds another into it (`include!`,
  `include_str!`, `include_bytes!` however written or named, assembly by any name, whose
  `.include` and `.incbin` read a file, `#[link]` and `#[debugger_visualizer]`, `#[path]`, a
  `mod x;`, a `$m!`, a `path`, `git` or `registry` dependency, a crate that embeds files such
  as `rust-embed` or its macro crate)
  is refused, as that file is outside its SHA-256; without one, warned of. The file is read
  by its tokens, from where rustc starts (after a BOM and a `#!` line), and its dependency
  lines as TOML reads them. Under a hold the build itself is judged by what rustc read: a
  plugin whose dep-info lists any file but its source and Quillo's glue does not load and is
  not kept (a dependency's macro that includes a file, `doc_comment::doctest!`); the bytes
  built are checked against the confirmed SHA-256 (found in review). The build is judged by
  its LLVM IR too (no assembly) and by its linker's own list (only the build's, the
  toolchain's and the system's files), and a source pin is not built where the program can
  reach CARGO_HOME, RUSTUP_HOME, its `cargo`, a `PATH` folder or the folder Quillo runs in
  (Q-1285; found by the review of e181c7e: assembly whose `!` a macro hands apart, a doc
  comment's `#` given to `[link(...)]`). `dotenvy_macro` is refused with the crates that
  build files in, and `<T>::` after a keyword, `<T<>>`, an impl's const generics and a
  macro's `impl X for $t` read as `T`. The linker's command line is judged as given and its list
  of files resolved through every link (a path that cannot be resolved refuses the build), the
  library built or pinned by its own SHA-256 loads only the system's libraries (its install
  names and DT_NEEDED), and the reach check runs both ways, the toolchain trusted for its
  `lib/rustlib` only (found by the review of a86f326: a framework named by a path past 1024
  bytes, lld's list written with `..` removed by text, a library loaded by its install name). A crate
  that reaches the system or loads code itself (`libc`, `nix`, `rustix`, `libloading`,
  `dlopen`, any `-sys` crate, and those that map, run or load code or call the system's
  frameworks: `memmap2`, `region`, `duct`, `mlua`, `pyo3`, `jni`, `core-foundation`,
  `objc2` and the like) reads as every effect but `db` (`libc::dlopen` read as `none`), and
  so does a foreign block a macro writes;
  a held or pinned library is read once and loaded from a private copy of the bytes judged,
  ELF libraries are read where their loader reads them (exact C-runtime sonames, the dynamic
  table at its address), and an unpinned plugin is not judged where another is pinned (found
  by the review of 0a3aeb9). A held or pinned library is copied once a process, shared by instances and
  reloads (each loaded a copy of its own, its state reset), in a folder the program cannot
  reach; musl's sonames are the C runtime's; an ELF library with overlapping segments is
  refused and its names are read through the mapping (found by the review of ce4f15d).
- **What `quillo.confirmed` does not confirm** is an error at its line, and so is a name used
  from another file and not taken, under one (see Security).
- **`"""` and `'''`** say that text over several lines is a backtick string (FEEDBACK85.md,
  Q-1345).
- **A host that comes only from `env()`** (with text written in the program and their trims) is
  warned of as the operator's setting, not as one a visitor can choose (Q-1348).
- **A `save:` in a `GET` route that reads who the visitor is** gets `route writes`' warning
  (Q-965), and `save` with `cache`, on `http.stream` or in a component is an error.
- **A `GET` route that writes and reads no one, not declared `route writes`**, is warned of
  (FEEDBACK70.md, Q-965, decided 2026-11-23): a hover's prefetch, or a move in place, ran it
  as far as its first write, with nothing said. Lasting writes only: the database, files
  (`write_file`, `delete_file`, `save_upload`, `save:`), another server and a service; the
  visitor's session or cookies, a program variable and a message (`publish`, a channel's,
  UDP) are not said. Three or more routes through one helper's write are said once, at it.
  quillolang.org's site has one: `GET /newsletter/confirm`.

### The guide

- **New sections**: What a program reaches, What a provider holds a program to, The page's
  head, The page's address, Routes that move in place; and each change above in the options,
  commands, effects, policy, Rust, catalog, HTTP client, paths and checking sections, the
  book's chapters and the short guide.
- **Running other programs** (ffmpeg, a converter, a Node script) goes through the catalog's
  `cli_run` and `ffmpeg_cli`, which say `run` among their effects (FEEDBACK85.md, Q-1343).

## 0.8.10 (2026-11-22)

Found making the videos and courses about Quillo: thirty small apps for videos (FEEDBACK72.md,
Q-1010 to Q-1019 and Q-1030 to Q-1032), thirty YouTube Shorts (FEEDBACK73.md, Q-1020 to
Q-1026; Q-1025 is Q-1270, found first), the comparison videos against six other stacks
(FEEDBACK79.md, Q-1250 to Q-1259), the courses (FEEDBACK80.md, Q-1260 to Q-1269) and the React
Quirks videos (FEEDBACK81.md, Q-1270 to Q-1272); by the review of Q-1250's trusted CSS
(Q-1274); and by this release's CI (a race in the offline worker, a follow-up to FEEDBACK67.md's
B.2). Q-1014, Q-1026, Q-1253 and Q-1263 are answered in the guide (and an editor's hover),
Q-1024 and Q-1264 in the guide and the code.

### Security

- **A visitor's value in a style is one CSS value.** `` style={`color: ${n.color}`} `` wrote a
  visitor's `red; position: fixed; inset: 0; background: red` as it came, and that note
  covered the whole page in red; Go's html/template, given the same template, writes
  `ZgotmplZ`. Each value interpolated into a `style` template is escaped as one CSS value (its
  `{ } ; \` as CSS's escapes, its `&` as `\26 `), wherever it stands in the expression: in a
  `? :`, `if`, `match`, `??`, `&&` or `||`, through `+`, `trim`, `join`, a list, a pipe, a
  block's `let`s, a function declared in the block, a lambda's defaults, a spread map's `style`
  key and a component's or view's `style` prop; the text the program writes stays as written,
  on the server and in the browser alike. With `c` as `red; background: blue`,
  `` style={`color: ${c}; padding: 4px`} `` is written
  `color: red\3b  background: blue; padding: 4px`, a colour the browser drops: a value can
  take away the declarations after it, never add one. An error's contents are escaped while
  it stays an error, so `??` still chooses. A template made in a `let` or a helper and given
  whole, `style={s}`, is the program's own text, as before (FEEDBACK79.md, Q-1250).
- **Only the program makes trusted CSS.** What `` css`...` `` makes (see Added) is written as
  it is in a style, so nothing from outside is taken for it: a request's data, the arguments
  a page sends a `server fn` (JSON and multipart), channel and MCP messages, the session,
  split services, other sites, the database, files, `env` and `run_script` (what it is given
  and what it gives back) are all plain text. In the browser, an answer the program wrote
  itself (`respond`, `reply`) carries none, nested marks included: a page takes the mark only
  from Quillo's own answers, by the `x-quillo-marked` header, unredirected and from the page's
  origin; a program's `respond`, `stream` or `events` drops that header in any spelling, and
  `site_headers` refuses it at load; a Quillo answer passed on by a proxy works as on 0.8.9,
  with no mark. A channel message pushed to pages stays text (decided). Each forgery two
  security reviews tried has its test (FEEDBACK79.md, Q-1250, the user's choice: "make it
  secure with trusted css").
- **A script's functions run only while their script does.** In 0.8.9 a function a
  `run_script` script made could be kept and called once the script had ended: a kept closure
  or a `parallel` branch ran 100,000 steps and more past the script's limits, one run by a
  helper's own nested script, or rendered as a tag by a helper, ran with the program's
  effects, and a script's code could be compiled into a page's JavaScript. While its script
  runs, such a function runs as the script's (a helper of the program's may call it back),
  with no effects and its steps and time counted against the script's; once the script has
  ended, a call anywhere (kept in a variable, a list, a map or an error, from a tick, as a tag,
  through `map` or `parallel`) gives the error value `a script's function runs only while its
  script does`. What it gives back to the program's code is plain text (no trusted CSS), a
  helper gets plain arguments, `parallel` inside a script runs in order under its budget,
  concurrent requests on one thread keep their scripts apart, and a program whose global
  holds a script's function that a component uses stops at start (Q-1274, found reviewing
  Q-1250's trusted CSS).
- **A sign-out stays a sign-out to the offline worker.** A session message a page sent before
  its visitor signed out could reach Quillo's worker after the sign-out had dropped that
  person's kept pages, and took them off the dropped list, so a late answer of theirs could be
  kept again, for the next person on the device to see offline (a test caught it 10 times in
  25 runs under load on 0.8.9). Where the worker has the Cookie Store API (Chrome), the cookie
  says whose session it is; without one, the page's word is taken as before (a follow-up to
  FEEDBACK67.md's B.2, found by this release's CI; in 0.8.9 too).
- **`crypto.verify_password` takes a real hash's time for any hash it cannot verify**: a
  `$scrypt$` hash and a broken `$argon2id$` one were answered in 0 ms. Any it cannot verify (an
  unknown or malformed PHC string, other text, a 1 MB value) does the work a null hash does,
  10 to 11 ms, as a real hash takes (FEEDBACK80.md, Q-1264).

### Changed

What a program may notice:

- **Lowercase `onchange` is the DOM's `change`**, which comes when the field is left, and
  `onfocus` and `onblur` are `focus` and `blur`: `onchange` on a field ran at each keystroke,
  and `onfocus` and `onblur` as `focusin` and `focusout`. For each keystroke write `oninput`
  (or React's `onChange`, still `input`) (Q-1270, Q-1025).
- **An error value shown in markup shows nothing**, where it printed `err("...")`, and the log
  says so; give it a fallback (`??`) or show its words with `error(r)`. A template string still
  writes `err(...)` (Q-1272).
- **A value in a `style` template is one CSS value**: one holding `;` or braces no longer adds
  declarations. Compose declarations with a style map or `` css`...` ``; a component adds its
  caller's to its own with `` style={`padding: 8px; ${style}`} ``. Text made in a `let` that
  holds declarations (`` let look = `color: ${c}; font-size: 20px` ``) and given to such a
  component is one value there, which the browser drops: make it with `` css`...` `` (Q-1250).
- **`` css`...` `` makes a value of its own**: `type_of` gives `str`, and `==`, `json`, printing
  and every builtin read its text, but `+` or a builtin gives plain text. The `x-quillo-marked`
  header is Quillo's: a program's own is dropped, and `site_headers` refuses it (Q-1250).
- **Voids inside an XML root are written as XML's**: inside `rss`, `feed`, `urlset`,
  `sitemapindex`, `entry`, `item`, `url` or `sitemap`, or an outermost element with a
  non-XHTML `xmlns`, `<link>` holds text and an empty one is `<link .../>`. HTML that uses
  custom elements named `item`, `entry`, `url` or `sitemap`, or an outermost element with a
  non-XHTML `xmlns:*`, has `<br/>`-style voids inside them (the DOM the same). A helper whose
  root is `<link/>` does not know it is in a feed: write the `<link>` in the entry's own
  literal (Q-1259).
- **A program with nothing to serve over HTTP runs without a port when its port is taken** and
  nothing asked for one, saying so; give `--port` to reach `/_quillo/health` and the metrics
  (Q-1011).
- **A clean stop closes each WebTransport session** (`server stopping`, at most 500 ms), so
  pages connect again at once, **and each SQLite database**, so its `-wal` and `-shm` go
  (Q-1265, Q-1260).
- **WebTransport also listens on `[::1]`** when HTTP is on 127.0.0.1 (Q-1266).
- **`/_quillo/ready` is neither logged nor in `quillo_requests_total`**, and an etag's 304 is
  logged and counted as a `3xx`, where it was a 200 and a `2xx` (Q-1255, Q-1262).
- **`/_quillo/ready` with a database per tenant can answer 200**, the database `"ready (per
  tenant)"` (Q-1254).
- **`site_headers`' `"server"`** takes away `server: Quillo` (null) or replaces it (Q-1256).
- **The 400 of several routes on one path names the closest route's miss**, not the first
  written's (Q-1257), and a file sent in a text field is named as a file (Q-1252).
- **A script's function called after its script has ended gives an error value**: test what
  one gives with `is_err`, as `if !deny(x)` reads the error as false (Q-1274).
- **`run_script` says what it leaves out**: `db`, `now`, `random`, `range` and `vec` are `not
  available in a script`, where they were an `unknown name` (Q-1013).
- **The REPL prints `null`** for a bare name, field, index or literal that is null (Q-1023).
- **`date.format` warns** of a locale it has no names for, and still writes English (Q-1021).
- **`quillo new` writes `onclick`, `onsubmit`, `oninput` and `value={x}`**, and so do the hover
  and `quillo check`'s hints (Q-1030).
- **New `quillo check` warnings**: a guarded group's page that calls a `server fn` through a
  helper (Q-1258); an effect a page needs that its route does not declare, an error where it
  always runs (Q-1261); a new value or an error dropped at the end of a branch (Q-1018,
  Q-1267); a plain `let` a handler changes (Q-1268); a value that can fail shown in markup
  (Q-1272); a link no route answers (Q-1251); `pages // 2` (Q-1016); CSS built with `+` or a
  plain template (Q-1017); a click a keyboard can't reach, an `<img>` with no `alt` and a
  focus ring taken away (Q-1019); Vue's and Svelte's spellings (Q-1031, Q-1032); an event
  attribute that never runs (Q-1020); a random value drawn in a component's body (Q-1015).
  Fewer: a program that serves no pages is not told of its folder's sheets (Q-1012), and the
  classes of the HTML it serves count (Q-1022).
- **Speed**: against 0.8.9 built the same way, whole-process instructions, best of 5 to 9.
  `quillo check` of quillolang.org's site takes 7.6% fewer (cycles 5.5%): the editor's hover
  text and references are made only for `quillo lsp`, and `quillo run`'s own check gains the
  same. A program starts with 4.9% fewer (`print(0)`): runtime.js is cut into its declarations
  with a quarter of the work. `map` and `filter` given a lambda take 3.6% fewer, vs-python's
  "filter map sum" 4.0%: a lambda's shortcuts are asked once. A request costs what it did: text
  +0.5%, json +0.2%, fortunes -0.8%, db -0.6%, compute 0.0% (one process; six, -0.1% to -0.8%).
  Plain markup -0.32%, a style's `${v}` +0.51%, a value with `;` +2.33% (the escape's own work);
  trusted CSS cost -0.41% to +0.51% (a template `style` prop, its path) against the build before
  it. 50,000 `run_script` calls take 61 ms where 0.8.9 took 203; a call inside a script costs
  2.6% more (its run's check) (Q-1250, Q-1013, Q-1274).

### Added

- **Trusted CSS, `` css`...` ``.** Its `${...}` values are escaped where it is written, and a
  style writes the rest as written wherever the value reaches one: through a name, a `let`, a
  prop, a spread, a list or a map, a function's answer, an island's props and state, a `live`
  or `server fn` answer, and a hot reload; a style map takes it as written too
  (`` style={{ color: css`red; font-weight: bold` }} ``). A template or text written straight in
  a component's or view's `style` prop, or in a spread literal's `style` key, is trusted too,
  its values escaped there. So a component adds its caller's declarations to its own with
  `` <div style={`padding: 8px; ${style}`}> ``: given
  `` <Card style={css`color: ${c}; font-size: 20px`}/> ``, with `c` as `red; background:
  blue`, it writes `padding: 8px; color: red\3b  background: blue; font-size: 20px`, `c` one
  value and the caller's `font-size` kept. Everywhere else it is the `str` it says, on the
  server and in the browser, and a count given to `t()` as one is a count on both sides
  (FEEDBACK79.md, Q-1250, the user's choice).
- **`run_script` has `math`, `stats`, `re`, `bit`, `dec` and `format_number`, and spreads**
  (`[...xs, 4]`, `{ ...m }`, refused past 1 MB): `` run_script("format_number(stats.mean(xs),
  \"de-DE\", 2)", { xs: [1, 2, 4] }) `` gives `"2,33"`. A script's `re` is held to its 1 MB
  while a replacement is built (one match times many `$0` asked for 1.9 GB), and its matches
  count by what each costs, groups included; `stats.histogram` takes a whole float's bins as
  an int's. The world is built once a thread and copied for each script, so 50,000 calls take
  61 ms where 0.8.9 took 203 (FEEDBACK72.md, Q-1013).
- **`check::Options::references` and `Options::plain()`**, for a program that embeds
  Quillo's checker: a check that leaves out what only an editor reads (each name's uses and
  their hover text, the scopes' names), as `quillo check`, `quillo run`, `quillo mcp`, `quillo fix`
  and `check_program` now do. `Options::default()` still gathers them.

### Fixed

- **A component that listens only to scrolls loads on a scroll.** Its code came when someone
  pointed at, touched, focused or typed in it, so a box scrolled by a script, a link into it
  or the browser putting a place back never ran its `onscroll`, and the page kept what the
  server rendered. A scroll loads it too, and that scroll is given to it once it has loaded
  (FEEDBACK72.md, Q-1010).
- **A form's reset sets the states its fields show.** A reset emptied a field bound with
  `value={name}`, but the state kept the old text, which the page went on showing beside the
  empty field; after a reset each two-way bound field's state is set from the field, and a
  prevented reset changes nothing (FEEDBACK80.md, Q-1269).
- **Lowercase `onchange` waits until the field is left**, as in HTML: on a text field it ran at
  each keystroke, as React's `onChange` does. Only React's camel-case names are renamed
  (`onChange` is `input`, `onFocus` `focusin`, `onDoubleClick` `dblclick`), so `onchange`,
  `onfocus` and `onblur` are the DOM's own events, as the guide's "compared as written" says
  (FEEDBACK73.md, Q-1025; FEEDBACK81.md, Q-1270).
- **An error value in markup no longer prints its source.** A failing `server fn`'s value,
  shown as it was, put `err("503 Service Unavailable")` into the page. As a child, an
  attribute, a class or a class list's item, a style map's declaration or a `<style>`'s text
  it shows nothing, on the server and in the browser alike, and the log says so once a request
  (`` an error no statement took, made at main.qwe:1, shown in <News> as nothing: 503 Service
  Unavailable ``); `<select value={err}>` chooses nothing (FEEDBACK81.md, Q-1272).
- **`state` inside a block says where it goes**: `if show { state extra = "extra" }` was a parse
  error that did not say why. Inside an `if`, `for`, `while`, a function or a block it says
  `` `state` goes at the top of a component's body, not inside an `if`: declare `state extra =
  ""` there and set it in the `if` ``, the first value of the kind given (FEEDBACK81.md,
  Q-1271).
- **A feed's `<link>` is XML's.** In an Atom feed `<link href="..."/>` was written as HTML's
  `<link href="...">`, so the answer was no XML and a feed reader refused it, and an RSS
  channel's `<link>https://example.com/</link>` failed `quillo check`. Inside an XML root a
  void name holds text and an empty one is written `<link .../>`; HTML pages are written byte
  for byte as before (FEEDBACK79.md, Q-1259).
- **A clean stop closes each SQLite database.** A stop left each database's `-wal` and `-shm`,
  and the main file without what the WAL held, so a copy of the file alone had lost tables, and
  a backup copied over it was undone at the next start without a word. After the drain each
  database (but one in a transaction) is closed without waiting on SQLite's busy timeout, so
  its `-wal` and `-shm` go and a restore copied over the file is what the next start opens;
  they stay while another process (Litestream, a `sqlite3` shell, a sibling instance) has the
  file open, and that process's writes are not held up (a stop 0.01 s) (FEEDBACK80.md,
  Q-1260).
- **A clean stop closes each WebTransport session**, with `server stopping` (500 ms at most,
  nothing when there are none), so its pages notice at once and connect to the next server:
  after a restart, a page that only listened heard nothing for 10 to 20 s, until the
  keep-alive gave up, and what was broadcast meanwhile was lost to it (FEEDBACK80.md, Q-1265).
- **WebTransport works at `http://localhost`**: with HTTP on 127.0.0.1, WebTransport listens on
  `[::1]` too, where Chrome dials `localhost` over QUIC; it was refused there, so a page in
  development took the WebSocket for good (FEEDBACK80.md, Q-1266).
- **`/_quillo/ready` with a database per tenant**: it said `"database": "down"`, 503, so the
  app was never ready. The database is `"ready (per tenant)"` when a tenant can be opened: the
  tenants' folder can be written, or their server answers (`select 1` on the tenant used last,
  else a TCP or Unix-socket connect, PostgreSQL's socket folder and MySQL's `?socket=`
  included; MySQL by a real login, so a probe never counts toward its `max_connect_errors`),
  with no database opened or made to ask. A probe has 2 s, DNS included, one runs at a time,
  and its answer is kept 30 s when up and 2 s when down (FEEDBACK79.md, Q-1254).
- **`/_quillo/ready` is neither logged nor counted**, as the guide says: two probes made two log
  lines and `quillo_requests_total{status="2xx"} 2`. Quillo's own answer (`/_quillo/ready`, or
  a mount's prefix and it) is left out; a route of the program's own at a path ending so is
  still logged and counted (FEEDBACK79.md, Q-1255).
- **`site_headers({ "server": null })` takes away `server: Quillo`**, which stayed; a text
  replaces it, and a route's own value wins (FEEDBACK79.md, Q-1256).
- **An etag's 304 is logged and counted as a 304**: the log said 200 and `--metrics` counted a
  `2xx` (FEEDBACK80.md, Q-1262).
- **The 400 of several routes on one path names the closest route's miss.** It named the first
  route's, however far off: a JSON post that left out `message` was told `` `form.name` is
  missing ``. The closest is one whose body is of the kind the request's content-type says,
  then the one that missed later (its body, after its query and its group's pattern), then the
  one with the most fields sent that fit (a field left to its default counts as none), and on a
  tie the first written (FEEDBACK79.md, Q-1257).
- **A file sent in a field the pattern takes as text is named as a file**: `` `form.photo` is a
  file (cat.png), not a str; it is in request.files.photo ``, in the 400 and the log, where it
  was "missing"; when the field has a default, the route runs with it, as before, and the log
  says the same, `(the route ran with its default)` (FEEDBACK79.md, Q-1252).
- **A program with nothing to serve over HTTP starts beside the app it feeds.** A program of
  `every`, `udp.listen` or jobs alone listened on 8080 and stopped when that was taken. It
  still listens on the default port, or `$PORT`, when it is free, where orchestrators probe
  its health and metrics, now saying so (`nothing to serve over HTTP; health and metrics
  listening on ...`); when that port is taken and nothing asked for one (`--port`, `$PORT`,
  `--host`, `--listen`, `--metrics`, `--watch`, `--split` and the like), it runs without one,
  saying so, on a private Unix socket it removes when it stops (FEEDBACK72.md, Q-1011).
- **`date.format` with a locale it has no names for warns**, on the server and in the browser,
  as `format_number` and `money.format` do, and still writes English: `ja-JP`, `ru-RU` and
  `xx-YY` gave English in silence (FEEDBACK73.md, Q-1021).
- **The REPL prints `null`** for a bare literal, name, field or index whose value is null, so
  `p.age` no longer looks like a line that did nothing; a call, `if`, `match` or block that
  gives null prints nothing, as before (FEEDBACK73.md, Q-1023).
- **`quillo new`, the hover and the hints write events as the guide does.** `quillo new`'s
  programs, the hover over `component` and `quillo check`'s hints taught `@click`, `@submit`,
  `bind:value` and `@change`; they write `onclick`, `onsubmit`, `oninput`, `onchange` (a
  checkbox's) and `value={x}`, and a handler written as text is told it needs a function
  (`` `onmouseover` needs a function, `onmouseover={() => ...}` ``) (FEEDBACK72.md, Q-1030).
- **`run_script` keeps nothing after its run**: a lambda a script set to a name of its world
  kept about 10 KB a call for good (40,000 calls: 377 MB, now 14 MB). The script's copy of its
  world is emptied after each run, and a name a script sets never reaches the next
  (FEEDBACK72.md, Q-1013).

### `quillo check`

- **The warning that a group guards its routes, not their pages' `server fn`s, follows the
  page**: into the functions, views and components the route calls, and a top-level `let page
  = () => ...`, however deep. It saw only the route's own body, so `route GET / with db =
  team_home()`, whose `<TicketList/>` calls `live(load_tickets)`, got no warning, and anyone
  could call that `server fn` (FEEDBACK79.md, Q-1258).
- **An effect a page needs is asked of its route**: what the server runs while it makes the
  page (a component's `state` values, its markup, components, plain helpers and builtins such
  as `now()`; not a handler or a lambda, which runs in the browser) is checked against the
  route's `with`. It is an error where the function that needs the effect declares it (or is
  a builtin) and always runs, a warning where it runs only under an `if`, `match`, `&&`, `??`
  or a loop, on the left of `??`, or for `state v = defer(...)`, said once a cause, `as are N
  other routes`. A component's `state x = load()` whose call needs `time`, in a route `with
  db`, passed the check and was a 500 (FEEDBACK80.md, Q-1261).
- **A new value or an error dropped at the end of a branch is warned of**: `push` and the other
  list and map functions that give a new value, or an error, at the end of an `if`, `else`,
  `match`, `for` or `while` branch whose value nothing takes, in a component's `fn` or a
  top-level one, as `push(xs, v)` and `xs | push(v)` too, where only an inline handler's
  `xs.push(v)` was. A handler named by reference (`onclick={go}`) is judged by what `go` gives
  (its `! E` or its result's type), and an inline one is told `?? ...`, `is_err` or `let _ =`,
  not a `?` that changes nothing; a `server fn`'s error dropped in a handler is not warned of,
  and a `?` inside a nested lambda no longer makes its function one that can fail, which gave
  0.8.9 wrong warnings (FEEDBACK72.md, Q-1018; FEEDBACK80.md, Q-1267).
- **A plain `let` that a handler changes and the markup shows**, which the page never
  updates, gets `` isn't a state, so the page won't show it: write `state x = ...` ``, also
  when it is changed through a function the component names, a timer, a channel's callback or
  an `on...` callback given to a child (FEEDBACK80.md, Q-1268).
- **A value that can fail, shown in markup** with no `??`, `?`, `match` or `is_err` test, is
  warned of: one from a function that declares `! E`, uses `?` or `err(...)`, or a `let` or
  `state` set from one, builtin conversions left out (FEEDBACK81.md, Q-1272).
- **A link to an address no route answers**: a literal `href`, `redirect` or `navigate`, or a
  template's literal start (`/todos/${id}` standing for one piece more), to a path no route,
  mount, service, file route or file of `public/` or `files()` answers is warned of, with "did
  you mean"; where only another method answers, it says 405 (307 and 308 keep the method).
  `/` alone, other sites' URLs, `<base href>` and a component's `href` prop are left out, and
  `.` and `..` pieces are read as browsers read them. With the class checks below, the
  website's check costs 0.09% more, 3,000 routes with 9,000 links 1.9% to 2.6% (FEEDBACK79.md,
  Q-1251).
- **Classes, for a script and for the files a program serves**: the unused-class warnings of
  `public/`'s sheets come only for a program that serves pages (a script kept beside an app was
  told of every class the app writes), and the classes written in the HTML under `public/` and
  in the `files()` folders' HTML and scripts (16 MB a check at most) count, a folder link
  followed wherever it points, each real folder once (FEEDBACK72.md, Q-1012; FEEDBACK73.md,
  Q-1022).
- **`pages // 2`**, Python's whole-number division, after a name that holds a number, gets
  `` `//` starts a comment here, so `// 2` is left out: whole-number division is `floor(a /
  b)` ``; a comment that shows a result (`// 59.97`) gets nothing (FEEDBACK72.md, Q-1016).
- **CSS built from a value with `+` or a plain template**, in a `<style>` or a `style=`, asks for
  `` css`...` ``, numbers and their `str` and `format_number` being safe: SQL built so was
  warned of, and this passed (FEEDBACK72.md, Q-1017).
- **Markup a keyboard can't reach**: a click on an element with no role, `tabindex` or key
  handler (not one that holds a button, a link, a field, a `summary` or a `[tabindex]`, nor one
  whose handler takes the event, as delegation does), an `<img>` with no `alt`, and
  `:focus { outline: none }` on `:focus`, `*:focus` or an element's type with no
  `:focus-visible` and no rule that draws a ring again (FEEDBACK72.md, Q-1019).
- **Vue's and Svelte's habits**: `@click.prevent`, `v-model={x}`, `v-on:` and `on:click`, which
  passed in silence and did nothing, or (`on:click`) were told `takes only text written here`,
  are each told the spelling to use (FEEDBACK72.md, Q-1031, Q-1032).
- **An event attribute that never runs**: in a view, a route's own markup or a plain function's
  markup that a page places outside any component, where the run drops it (FEEDBACK73.md,
  Q-1020).
- **`random`, `random_int` or `uuid` drawn in a component's body and shown** is told that it is
  drawn again when the component wakes, the browser's value taking the server's place; `state
  x = ...` keeps the server's (FEEDBACK72.md, Q-1015).
- **`live` with its argument missing** shows `live(load, ...args)` (FEEDBACK73.md, Q-1024).
- **What it costs**: the website's check took 9.284G instructions before the checks of Q-1258,
  Q-1261, Q-1267, Q-1268 and Q-1272, 9.273G after; 150 guarded routes 3.3% more.

### The guide

- **The guide and the short guide say each change above**, and the examples the new warnings
  found are fixed (headers as buttons, an `<img>`'s `alt`, `app/coop`'s dropped error and
  transaction).
- **Several routes' bodies on one path** are a `oneOf`, or an `anyOf` when one body could fit
  two (a catch-all), as `quillo docs` gives them (FEEDBACK72.md, Q-1014).
- **The short guide** (`llms.txt`) points to `llms-full.txt`, and has `while`, `quillo serve`,
  `live(load, ...args)`, and that `ask` and `notify` are plain markup with `q-` classes for the
  page to style; an editor's hover for `live` and `int` says the same (FEEDBACK73.md, Q-1024;
  FEEDBACK79.md, Q-1253).
- **A page's runtime as measured**: 139,198 B, 38,621 B gzipped, for one counter on 0.8.9, with
  comments, blank lines and indentation taken out but not minified, where the guide said about
  35 KB, 11 KB and minified; the README's 9 and 13 KB are 39 and 45 KB with the runtime
  (FEEDBACK73.md, Q-1026).
- **What `int()` and `float()` take**: white space at either end, a leading `+` and zeros,
  looser than a route's `int`, with the pattern's rule for a check by hand; `int()` stays as
  Q-542 decided (FEEDBACK79.md, Q-1253).
- **A path inside SQL counts from the process's folder**: `vacuum into 'backups/b.db'` in a
  program started from elsewhere fails; `pragma database_list` gives a path that works from
  anywhere (FEEDBACK80.md, Q-1263).
- **Sign-ins that take one time**: `verify_password` takes as long for a null hash, a null
  check before it is what answers a stranger at once, and the guide verifies first and then
  decides, in place of a stand-in hash (FEEDBACK80.md, Q-1264).

## 0.8.9 (2026-11-20)

Found by the security audit of 0.8.7 (FEEDBACK75.md, Q-1050 to Q-1095, and Q-1120 and Q-1121
found in review), by the study of authentication (FEEDBACK78.md, Q-1158, Q-1159, Q-1168,
Q-1169 and Q-1170, and its A.13 and A.14), and by the tests written for the lines of 0.8.9 no
test ran (Q-1097, Q-1125 to Q-1128, Q-1171 to Q-1177 and Q-1180).

### Security

- **A wildcard media stream's rules are told which stream a page asks for.** With
  `media.on("call:*", ...)`, a page let in for one stream could watch every stream under it and
  send its own video into anyone's, the guide's video call included, as `publish` and `watch`
  were called with the peer alone. They are called as `(peer, name)`, `name` being the stream
  asked for (`call:42:ana`), and a channel's `join` is given the channel's name the same way;
  a rule written with one parameter is called as before. `quillo check` warns of a `*` rule
  whose `publish` or `watch` never reads the name, and the guide's call checks it (FEEDBACK75.md,
  Q-1050, the user's choice).
- **`media.rtp` listens on this machine unless told otherwise, and takes one sender at a
  time.** It listened on every interface and took any sender's packets, so anyone who reached
  the UDP port put their video between the camera's frames, for every viewer. `host` gives
  another address (`"0.0.0.0"`: all of them, as before); the first sender, by its address and
  SSRC, is taken until it has sent nothing for 2 s, and another's packets are dropped meanwhile
  (FEEDBACK75.md, Q-1066).
- **A visitor's JSON is read within `--max-memory`.** A `server fn`'s arguments, `request.json`,
  a form's JSON, `/_quillo/markdown`'s text and a page's channel messages over 1 KiB are read
  with each text, list and map counted at its real size as it is made, so a request whose
  values pass the limit ends with the memory 500. One call of about 10 KB of gzip made about
  1 GB before its arguments were checked; under any limit a 3 KB body made 136 MB, as JSON's
  count of values had a floor of 1,000,000 and an empty list or map counted as one. With no
  limit, and for JSON Quillo trusts (a split service's answers, replays of idempotent
  requests, stored channel values), nothing changes. A message to the MCP door over HTTP of
  more than 1 KiB is read the same way; stdio's are read as before (FEEDBACK75.md, Q-1052,
  Q-1070).
- **The markdown door reckons what it will make before it makes it**: `/_quillo/markdown`
  made 160 to 220 MB from a 1.2 KB request (1 MB of `[`, gzipped), past `--max-memory`. What the
  parser's tree, the highlighter and the HTML will take is worked out from the text, and past
  the limit the request is the memory 500 before anything is parsed (FEEDBACK75.md, Q-1069).
- **`/_quillo/join` refuses a body over 4 KB**, with 413, before reading it: its JSON was read
  whole before the signature could be checked, and 1 KB of gzip made 100 MB (FEEDBACK75.md,
  Q-1052).
- **Live runs kept for sharing have a bound.** They were kept by their arguments' bytes, and
  256 calls of about 10 KB kept 2.5 GB for good. A run is kept by a keyed digest of its call,
  the runs kept weigh 64 MB at most, the oldest let go first, and a run heavier than that alone
  is not kept (FEEDBACK75.md, Q-1057).
- **`data-q-` words in a page cost nothing**: one comment of `data-q-XXXXXXXX` words made each
  view of its page compute for seconds to minutes, past `--max-compute`. A page's marks are
  looked up once among the program's own styles' (80,000 of them: 6.07 s to 2 ms)
  (FEEDBACK75.md, Q-1054).
- **What pages' messages hold while they wait for a handler is counted in bytes.** A channel
  kept 10,000 messages from pages, up to 1 MiB each, so one visitor on one socket could make
  the server hold gigabytes. Messages waiting take at most 4 MiB a page, an eighth of the total
  a client, a quarter a channel, and 256 MiB in all, or `--max-memory` if smaller; a page past
  its share waits, its socket unread, instead of losing messages; a message sent as a
  WebTransport datagram is dropped past it, as one past a full queue was (FEEDBACK75.md,
  Q-1056).
- **A channel message begun must keep coming.** A socket that never finished a message held up
  to 1 MiB for good, on 4,096 sockets, outside `--max-memory`. A message in parts, or over 4
  KiB, is read in 8 KiB pieces counted as they come, against 64 MiB for all such messages (or
  `--max-memory`, if smaller) and an eighth of it for one client, and must be whole within 10
  s, and a second more for each 16 KiB that came; past either, the connection ends. A whole
  message in one frame of 4 KiB or less is read as before (FEEDBACK75.md, Q-1073).
- **WebTransport sessions are held as WebSockets are.** A session counts among the 4,096
  connections the server keeps (256 a client), where it was not counted; it opens at most 8
  streams of frames and 2 both ways; and it may have 1 MiB a stream, and 4 MiB in all, sent
  before the server reads it. One session could hold about 100 MiB in unfinished frames
  (FEEDBACK75.md, Q-1074).
- **A program's session reaches its own channels only.** A page's channel ticket names the
  program it came from, and another mounted program's handlers see no session for it: a
  session a visitor could set freely in one program was taken by another's `join` and handlers
  (FEEDBACK75.md, Q-1061).
- **Under `--split`, a channel no program claims is not joined while a service is silent.**
  While a service started again (a crash, a deploy), anyone could join its channels as no
  one's, without its `join`, and kept hearing them once it was back (FEEDBACK75.md, Q-1065).
- **A mounted program's session cookie has a name of its own**: `__Host-shop.quillo_session`
  for `mount /shop`, at `Path=/`, where it was `quillo_session` at `Path=/shop`, which a sibling
  subdomain could plant with a session of its own, so the visitor was signed in as the
  attacker, or out. Under `--base`, and over plain HTTP (`--watch`, `--plain-http`, `quillo
  app`), it is `quillo_session` with the program's path, as before (FEEDBACK75.md, Q-1077).
- **UDP fairness counts packets too.** A sender of empty packets filled a port's queue, pushed
  everyone else's out and was never held back, its bytes being few. A sender holding more than
  half of a full queue's packets, or of its bytes, has its new packet dropped, and IPv6 senders
  count by their /64 (FEEDBACK75.md, Q-1094).
- **`check_program` stays within its budget.** A 699-byte program whose types doubled on each
  line took the server's memory until it was killed. A confined check (`check_program`) keeps a
  type to 10,000 parts and 16 levels, taking what grows past them as unknown, and past 2 s, or
  about a million parts kept in all, gives one error, `the code is too complex to check`.
  `quillo check` and `quillo run` take a type past 1,000,000 parts or 64 levels as unknown and
  say what 0.8.8 said (FEEDBACK75.md, Q-1053); the 2 s holds for a function of 100,000 `let`s
  too, which a check and a load read as fast as 0.8.8 did (Q-1127). A confined check quotes no `.rs` file's text
  (Q-1095).
- **`run_script` no longer holds its instance.** A 143-byte script held an instance for 13 s,
  as a step was one call however much it read. A builtin or a comparison costs a step for each
  KB it reads, as each KB made did, and a script stops after 100 ms unless `run_script(source,
  vars, { timeout })` says otherwise, from 1 ms to 60 s (FEEDBACK75.md, Q-1055, the user's
  choice). A script's method names are no longer kept for good (each request leaked its
  source's size), and a name in its errors is cut at 100 characters (Q-1076).
- **A prefetch sends no packet and no message.** Pointing at a link (65 ms), or the offline
  worker installing, ran a `GET`'s `udp.send`, `udp.reply`, `channel.send`,
  `channel.broadcast`, `http.request` with a method that writes, or `http.stream` with one or a
  `body`, as the visitor. Each is refused in a prefetch, as `http.post` was (FEEDBACK75.md,
  Q-1062).
- **`quillo.policy` is never served**: `quillo serve DIR` and `files(".", path)` served the
  folder's policy file, which names the secrets' variables, inner hosts and limits. A
  `quillo.policy`, and the policy file the program runs under by any name, a hard link to it
  too, are kept back as `.env` is (FEEDBACK75.md, Q-1088).
- **A translation key from the request is shown as text.** `raw(t(key))` wrote a key missing
  from every locale as HTML, so `` raw(t(`flash.${msg}`)) `` put a visitor's `<script>` into the
  page. Such a key is written escaped, on the server and in the browser (FEEDBACK75.md,
  Q-1059).
- **A URL put into a translation's markup is checked as markup's are.** `raw(t("site", {
  url }))` wrote a `javascript:` link into `<a href="{url}">` as it was. An attribute that takes
  a URL is `about:blank#blocked` where what is written there, once the values are put in and the
  translation's own character references read, makes a `javascript:` or `vbscript:` URL; an
  attribute written without quotes that holds a value is quoted, so the value cannot end it.
  The same value as text (`<i>{title}</i>`) is shown as written; the browser does the same
  (FEEDBACK75.md, Q-1083). A text that holds a value may hold plain HTML only: `translations`
  refuses one with a comment, `<!...>`, `<?...>`, `<script>`, `<style>`, `<title>`,
  `<textarea>`, `<iframe>`, `<template>`, `<svg>`, `<math>` and the like, whose reading let a
  value through in ways four rounds of tests kept finding (Q-1176, the reviews of 0.8.9's
  coverage). In plain HTML a value makes no tag after a `<` that starts none (`Must be
  <{max}`), no character reference after a `&`, and no attribute in a name; a character
  reference is read whatever its leading zeros (Q-1177); an SVG animation's values are checked
  as in templates, and an `on...` attribute or `srcdoc` holding a value is written empty; on
  the server and in the page alike.
- **A PostgreSQL session change by other words is caught.** `update pg_settings set setting =
  ...` changed the shared connection's session for every later visitor; it is refused outside
  a transaction, as `set` is. `query_to_xml` and its kin (`cursor_to_xml`, `table_to_xml`,
  `schema_to_xml`, `database_to_xml`), `ts_stat` and `ts_rewrite`, which run SQL Quillo cannot
  read, run on a connection of their own, reset after them, as a `do` block does. A view, a
  rule or a trigger that changes the session is still not seen (FEEDBACK75.md, Q-1060).
- **A user's own Okta or Entra ID token is not a service's.** `accept_calls { jwt }` named an
  Okta token's caller by its `sub`, the user's login, which then matched itself, so every
  user's token for the audience passed as a service's own. Okta's `cid` and Entra ID v1's
  `appid` name the client, after `client_id` and `azp`, so such a token is a user's and is
  refused (FEEDBACK75.md, Q-1079).
- **A record from a request is checked to its end.** A recursive record was checked 64 levels
  deep, and below that wrong types, values outside a set of strings and extra fields reached a
  `server fn`'s parameters and route patterns. A request's value is checked to its end, up to
  512 levels (each type named on the way counting one), and refused past them (`is nested more
  than 512 deep`); a union is tried once for each value, so a deep valid one is made in linear
  time, and a `server fn`'s arguments, and an MCP tool's, are checked once: a tool's call
  with a valid tree 22 levels deep took 1.4 s, as the door checked its arguments a second time
  in exponential time, and 62 levels would have held a worker for good. What the program builds
  itself is checked as before. `quillo check` refuses a type that names itself with nothing
  between (`type Ab = Ab | int`) (FEEDBACK75.md, Q-1080).
- **An offline page's gate fails closed under a sandbox.** Under `content-security-policy:
  sandbox allow-scripts` the gate could not read the session's cookie and stopped, and the page
  showed, its scripts run, to the next person on the device. A kept page whose policy is a
  `sandbox` without `allow-same-origin`, as without `allow-scripts`, is not given back
  (FEEDBACK75.md, Q-1078).
- **A sign-out that redirects to another site drops the kept pages.** They stayed in Cache
  Storage, readable on the device, until a page of the site was opened online. Where the
  browser gives a worker the Cookie Store API (Chromium's), Quillo's worker follows
  `quillo_tag` itself, and reads it after a redirect, so the session's pages go at once
  (FEEDBACK75.md, Q-1082).
- **A page carries only the page states its components read.** Every top-level `state` went,
  whole, into the HTML of each page that had a component, so `state me = load_user()` sent the
  user's password hash and TOTP secret, and a state set from `env` went to every visitor,
  though no component read them. A page now carries the states its components' code reads (the
  views they show included), and `quillo check` warns of one a component reads that comes from
  `env`, `read_file` or a `select *` row. The error for a `server fn` that reaches a page state
  through a helper says whether the helper reads it or only sets it (``it sets it through
  `remember` ``) (FEEDBACK75.md, Q-1058, the user's choice).
- **The bundle keeps more of the server's values out.** Globals made from `args()`,
  `read_line`, `stdin`, `run_script`, `crypto`, `uuid`, a site's call or a plugin's function
  (as a method too) went into the public JavaScript bundle when a component read them; they are
  kept out as `env`'s are, a plugin whose file says `//! effects: none` aside. A name a
  parameter, a lambda, a `let` or a loop binds is no longer taken for the builtin of that name,
  nor is a top-level `let` or `fn` of the program's (`fn uuid() = "fixed"`), which is kept out
  only when its own value is the server's (FEEDBACK75.md, Q-1063).
- **Under `--split`, a service sees the visitor's address.** Every visitor reached a service
  as the app's `127.0.0.1`, so a `request.ip` check that refused them in one process let them
  through, and the limits by client put everyone in one place. The app signs the address it
  took (with its own `--trust-proxy`) into each request it passes on, and only a service it
  started takes it, from this machine; one a client sends is dropped (FEEDBACK75.md, Q-1064).
- **`stream` and `events` hide a builtin's error**, as routes, `server fn`s and channels do: a
  database's error or an internal URL with its key went to the visitor (FEEDBACK75.md,
  Q-1068).
- **A split service's answer is not held whole in the app.** It was read into the app's
  memory, up to 256 MB a request, outside `--max-memory`. One over 1 MiB (or over
  `--max-memory`, where that is less: Q-1175), or with no length,
  goes through to the visitor as it comes (5 s to connect, 60 s between its bytes), so a
  service's `stream` arrives as it is sent; a shorter one is read within 256 MiB or
  `--max-memory`, else the visitor gets 502 (FEEDBACK75.md, Q-1071).
- **The app's session cookie stays with the app**: every visitor header went to a split
  service, the root site's `__Host-quillo_session` included, at whatever address
  `--service-url` names. The app's own session cookies are left out of the `Cookie` line
  (FEEDBACK75.md, Q-1081).
- **A `server fn` tells a caller no internals.** A record's default that failed as an argument
  was made gave any caller its file, line and error in a 400, outside `--watch`; it is the
  function's 500, the details in the log. `/_quillo/rpc/<name>` of a `for [...]` function named
  the services it takes calls from; it is the 404 of a name that is no `server fn`
  (FEEDBACK75.md, Q-1086).
- **A program's `site_headers` reach what it mounts.** Its CSP, HSTS and `Referrer-Policy`
  never reached `/shop/*` or a service under a prefix. They are added below the mounted
  program's own rules, and its page, or a split service's (sent the app's policy, signed, with
  each request), is written with the policy so that Quillo's scripts are let through
  (FEEDBACK75.md, Q-1087).
- **The request log hides secrets in a URL.** A password, a sign-in or reset link's token,
  OAuth's `code` and `state` and SAML's messages in a query string went to the log as they
  were, kept for months. A value whose name says a secret (`token`, `tokens`, `code`,
  `password`, `key`, `state`, `sig`, `authorization`, session ids such as `session`, `sid`,
  `JSESSIONID` and `PHPSESSID`, and names ending so, `access_token`, `apiKey`) is written
  `[hidden]`, in the text log and the JSON one (FEEDBACK78.md, Q-1158).
- **A single regex search no longer holds an instance.** One `re.test` or `re.find` of a
  visitor's pattern (`[ab]*a[ab]{900}c` on 200 KB) held an instance for seconds to minutes,
  past `--max-compute`, as Q-976's limit covered only the calls that search many times. A
  search whose work would pass a quarter of a second runs on Quillo's own counted engine,
  which stops at 0.1 s plus 0.15 µs a byte, within `--max-compute`, and before its work takes
  256 MiB or `--max-memory`, giving the error value `the pattern is too costly for this text`.
  Ordinary patterns give the answers 0.8.8 gave, long texts faster (FEEDBACK75.md, Q-1075). A
  pattern of literals alone (`cat|dog`, a list of 5,000 words) is searched as 0.8.8 searched
  it, on any text and as fast, and a pattern too big to compile is `the pattern is too big`
  whatever text it first meets, `re.valid` false (Q-1125, Q-1126).
- **A PNG's colour profile is read to 4 MiB**: a 146 KB PNG made `image.info` allocate 264 MiB,
  and a 486 KB one 512 MiB, outside every image and memory limit. A larger profile is an error
  value, and headers are read eight at a time (FEEDBACK75.md, Q-1051).
- **`http.*` answers share 256 MiB.** Each call could take its 64 MiB, and each gzip answer
  doubled its buffer past that, so a 200 KB gzip answer held about 200 MB a call and 64 calls
  asked for 13 GB. Answers are read in pieces, never copied whole, and those being read at once
  take 256 MiB at most past each one's first MiB; a call past it is an error value
  (FEEDBACK75.md, Q-1072).
- **A program's headers cannot frame its request.** `http.*` sent a program's
  `content-length` against the body it sent, so one that passed a visitor's headers on let
  them smuggle a second request to the upstream. `content-length`, `transfer-encoding`,
  `connection`, `keep-alive`, `te`, `trailer` and `upgrade` are left out of a program's
  headers, as `fetch` leaves them out (FEEDBACK75.md, Q-1089).
- **A gzip request body decodes to 100 times its size at most**, plus 64 KB: 10 KB on the wire
  carried 10 MB to the route and every limit after it. More is 413 (FEEDBACK75.md, Q-1090).
- **A body trickled in cannot hold a graceful stop**: with no `--drain`, a body arriving a
  little over 1 KiB a second held the stop for as long as it kept coming. A request still
  arriving 30 s after the stop is answered 503 and closed (FEEDBACK75.md, Q-1091).
- **`stats.mode` and `stats.multimode` count with a keyed hash**: crafted numbers fell in one
  bucket, and 3 MB of JSON held an instance 3.5 s, past `--max-compute` (216 ms to 15 ms on
  3 MB) (FEEDBACK75.md, Q-1092).
- **`public_only` refuses more of this machine's addresses**: `::ffff:0:a.b.c.d`
  (IPv4-translated) is judged by its IPv4 address, as is ISATAP's `::5efe:a.b.c.d`, and the rest
  of `0::/8` is not public; `[http] private = false` refuses the same (FEEDBACK75.md, Q-1093).
- **A slow reader inside a SQLite transaction holds the instance a second at most.** A
  stream's `send` inside `db.transaction` waited on the client's socket while the transaction
  held the instance's lock, so one client that read nothing stopped every other request for up
  to 30 s at each `send` (`/ping` waited 34.9 s), and a few kept it stopped. What the client has
  not read is kept, up to 4 MiB (less under `--max-memory`); past that the transaction waits a
  second in all, then `send` fails as for a client gone and the transaction rolls back. A
  client that keeps up gets the whole answer (a 20 MB export) and the transaction commits.
  `quillo check` warns of `send` inside `db.transaction` or `db.atomic` (FEEDBACK75.md,
  Q-1067).
- **A service `--split` starts gets a visitor's header that is not ASCII, and its errors
  repeat no header.** One byte past ASCII in any header (a cookie a page's script set to
  `ünï`, `X-Note: café`) gave every page of the service a 502 whose text repeated the header,
  the service's HttpOnly session cookie with it, for the page's script to read. The app
  passes such headers on, signed, and the service reads them as sent; the 502 says only what
  failed (`Connection refused`). A visitor's own `X-Forwarded-For`, `-Host` and `-Prefix` reach
  no service: it sees the app's (FEEDBACK75.md follow-up, Q-1174; in 0.8.8 too).
- **A record a `server fn` takes inside a map shape is made as a parameter of its type.**
  `server fn f(x: { owner: Account })`, a list of such shapes, or a record behind eight or more
  types naming types, was checked but not made: fields `Account` does not have (`is_admin`)
  reached the function, and a left-out field with a default was refused. They are left out
  and the defaults put in, as for `f(a: Account)` and a route's `{ json: ... }`; MCP tools and
  service calls alike (FEEDBACK75.md follow-up, Q-1171; in 0.8.8 too).

### Added

- **Who a proxy says the visitor is.** `--client-cert-header NAME` gives `request.client_cert`,
  `{ name, subject, uri }`, the client certificate a proxy in front checked, from its header
  (Envoy's `x-forwarded-client-cert`, or with `--client-cert-subject` a subject alone, as
  nginx's `$ssl_client_s_dn`), read as `accept_calls { client_cert }` reads it; `--user-header
  NAME` gives `request.remote_user`, the user a proxy signed in (Kerberos by SPNEGO: Apache's
  mod_auth_gssapi, IIS). Each needs `--trust-proxy`, and the program does not start without it;
  each is believed only on a request from a `--trust-proxy` address, and is null for a missing
  header, several elements, several lines or (the user's header) a comma-joined value, the log saying why once. A
  service `--split` starts gets both from its app, signed. Where neither option is given,
  `request` has neither field. Each option is given once (Q-1128) (FEEDBACK78.md, A.13 and
  A.14, the user's choice of a small reading in core; the options' names are on the list for
  the user).

### Fixed

- **MySQL and MariaDB keep their connections while the program is busy.** The driver closed
  every connection handed back past ten idle ones, so 64 requests at once opened 31,563
  connections in 5 s, left 16,506 sockets waiting to close and failed 14,082 of 93k requests.
  Idle connections now stay a minute unless the URL gives `inactive_connection_ttl`; the same
  load opens 169, with no failures (FEEDBACK78.md, Q-1169).
- **`quillo check` and a `GET` that writes**: a write through `http.stream` with a writing
  `method` or a `body`, `http.request` whose method a `let` holds, `udp.send`, `udp.reply`,
  `channel.send` or `channel.broadcast`, SQL a `let` holds, and a function a `let` holds, are
  warned of as `db.exec` was; their forms that write nothing are not (FEEDBACK75.md, Q-1084).
- **`quillo check` and SQL built in a helper**: SQL built from values is warned of where a
  function of the program's builds it (written before or after its caller), where it goes
  through `str`, `trim`, `lower`, `upper` or `replace`, a map's field or a list's item, `s = s +
  n` or a loop that builds an `IN` list, and where it is returned from inside an `if`. Only
  what is built is said: a map's other field, a helper given only safe text, and `sql_name` are
  not (FEEDBACK75.md, Q-1085).
- **"Did you mean" at once**: a name is compared with others within a band, and the search
  stops early, so 3,000 unknown names among 3,000 functions of 67 letters check in about a
  second, where they took 82 s, with the same suggestions (FEEDBACK75.md, Q-1120, found in
  review).
- **Transactions on one row at once all end**, on PostgreSQL and MySQL. Each statement of such a
  transaction waits on one of the server's threads, and with every thread waiting on a row's
  lock the transaction holding it had none for its commit: PostgreSQL waited for good, and a
  graceful stop with it, and MariaDB 50 s and then error 1205. A statement inside a transaction
  goes before other waits and may take a thread past `--workers`, and a waiting job wakes a
  thread every time, so 100 requests on one row all finish (FEEDBACK78.md, Q-1168).
- **A MySQL transaction waits for a connection without a thread**, for 30 s at most, as
  PostgreSQL's does, then gets `` no MySQL connection free after 30 s: all the pool's are in use
  (`?pool_max=N` in the URL allows more) ``; it waited on a thread with no limit, taking the
  threads other requests needed (FEEDBACK78.md, Q-1170).
- **`raw(t(...))` in a component keeps its text in the browser**: its key was left out of the
  translations the page loads, and the page showed the key once its components took over
  (FEEDBACK75.md, Q-1121, found in review).
- **A passkey's credential reaches a `server fn`**: an object of the browser's made of getters,
  with a `toJSON` (a `PublicKeyCredential`, a push subscription, a position), went as `{}`; it
  goes as its `toJSON` gives (FEEDBACK78.md, Q-1159).
- **A channel loses no message under `--max-memory` or `--max-body`.** Messages waiting for a
  slow handler went to it in batches of up to 1 MiB, which a smaller limit refused whole: of
  1,040 messages at `--max-memory 1MB`, 16 were handled and the rest lost, uncounted. Batches
  stay within both limits; a message too large to hand over within them, or for the page's own
  read, is dropped and counted in `quillo_channel_dropped_total`; the same for a service
  `--split` starts (FEEDBACK75.md follow-up, Q-1172; in 0.8.8 too).
- **A service `--split` starts answers after a quiet spell**: the first request passed on to
  it after about two minutes idle was a 502 (`Connection reset by peer`), as the app used a
  connection the service had closed (FEEDBACK75.md follow-up, Q-1097; in 0.8.8 too).
- **A whole number is taken for a float however the float is reached**: behind eight or more
  types naming types, or in a map shape's field (`x: { p: float }` given `{"p": 1}`), it was
  refused where `x: float` took it; a `dec` in a shape (`{ a: dec }` given `"1.50"`) was
  refused, and `money` in one stayed a plain map (FEEDBACK75.md follow-up, Q-1180; in 0.8.8
  too).

### Changed

What a program may notice:

- **`media.on`'s `publish` and `watch`, and `channel.on`'s `join`, may take the name as a
  second parameter**: `(peer, name) =>`. A rule for a name ending in `*` should check it: the
  guide's video call does (Q-1050).
- **`media.rtp` listens on 127.0.0.1**: a camera or an encoder on another machine needs `host`
  (`media.rtp("cam", { video: 5004, host: "0.0.0.0" })`, best a private address), and a second
  sender is taken only after the first has been quiet for 2 s (Q-1066).
- **`run_script(source, vars, { timeout })`** stops after 100 ms unless `timeout` says (a number
  of milliseconds, or `"250ms"`, `"2s"`; 1 ms to 60 s; another option, or one out of range, is
  an error), with the error value `the script took longer than its 100 ms`. A script that
  walks large values near 20,000 steps may stop sooner, each KB read costing a step (Q-1055).
- **`check_program` past its caps** gives the one error `the code is too complex to check:
  ...` in place of all else (Q-1053).
- **A mounted program's session cookie is `__Host-<mount>.quillo_session`** over HTTPS without
  `--base`, so a person signed in to a mounted program before the upgrade signs in again
  (Q-1077).
- **A request's value nested past 512 levels is refused**, and a type that names itself with
  nothing between is an error (Q-1080).
- **New `quillo check` warnings**: a `GET` route that writes in the ways above (Q-1084), SQL
  built in a helper or through text functions (Q-1085), and a `*` media rule that never reads
  the stream's name (Q-1050).
- **A page that sends faster than a channel's handler takes its messages** is held back, its
  socket left unread, rather than having the oldest messages dropped (Q-1056).
- **`/_quillo/wt-ticket` is 404** for a program that surely uses no channel or media (none of
  `channel`, `media` or `js` in it, nothing mounted, no service, no plugin), where anyone could
  connect and send what the server reads (Q-1052).
- **`quillo.policy` is a miss** for `files()` and `quillo serve`, as `.env` is (Q-1088).
- **`update pg_settings` outside a transaction is refused** on PostgreSQL, and `query_to_xml`
  and its kin run on a connection of their own (Q-1060).
- **`raw(t(key))` of a key no locale has is escaped text**, and a URL attribute of a
  translation's markup that a value makes a `javascript:` one is `about:blank#blocked`
  (Q-1059, Q-1083).
- **A regex on a multi-megabyte text may give the error value `the pattern is too costly for
  this text`** where its search would take long, past 0.1 s and 0.15 µs a byte (Q-1075).
- **A split service is asked for its answers uncompressed** (`Accept-Encoding: identity`),
  and the app compresses what it sends on (Q-1071).
- **The request log writes `[hidden]`** for a query value whose name says a secret (Q-1158).
- **A page carries only the states its components read**: a state only the route's own HTML
  shows stays on the server, and the DevTools panel sees only the states sent (Q-1058).
- **The bundle guard refuses more sources**: a component that reads a global made from
  `args()`, `read_line`, `stdin`, `run_script`, `crypto`, `uuid`, a site's call or a plugin's
  function is a load-time error, as one made from `env` is (Q-1063).
- **A wrong caller of a `for [...]` `server fn` at `/_quillo/rpc/<name>` gets `no server fn
  ...`**, the 404 of an unknown name (Q-1086).
- **A record's default that fails as a `server fn`'s argument is made is a 500**, `internal
  error in x`, where it was a 400 with the error's text (Q-1086).
- **A mounted program's answers get its parent's `site_headers`**, below its own rules, and a
  split service's pages the app's policy (Q-1087).
- **A MySQL transaction with no connection free errors after 30 s** (Q-1170).
- **A stream's `send` inside a SQLite transaction fails after a second** when its client has
  not read past 4 MiB, and the transaction rolls back; `quillo check` warns of the `send`
  (Q-1067).
- **`http.*` drops a program's `content-length`, `transfer-encoding`, `connection`,
  `keep-alive`, `te`, `trailer` and `upgrade` headers**; `host` is sent as given (Q-1089).
- **A `server fn`'s record inside a map shape, or behind eight or more types naming types, has
  only its own fields and its defaults** (Q-1171); a whole number where such a float goes comes
  as a float, and `n: Num | int` (with `type Num = float`) or `n: float | Cnt` (with `type Cnt =
  int`) given `1` keeps the int, where 0.8.8 gave `1.0`: an int stays an int where the union
  names `int`, itself or through types naming it (Q-1180).
- **A service sees the app's `X-Forwarded-For`, `-Host` and `-Prefix` alone**, never the
  visitor's own, and a service run apart reads a visitor's headers that are not ASCII from the
  app's signed `x-quillo-headers` with `QUILLO_SERVICE_SECRET` (Q-1174).
- **A translation text that holds a value and a comment, `<script>`, `<style>`, `<title>`,
  `<textarea>`, `<svg>`, `<math>` or the like is refused at start**; put that markup in the
  component around `t(...)` (Q-1176).
- **`quillo_channel_dropped_total` counts messages too large to hand over** under `--max-memory`
  or `--max-body`, which were lost uncounted (Q-1172).
- **Loops that `let` a value or change a list or a map are as fast as 0.8.8's or faster**: the
  release's benchmark found a `let` in a loop 17 instructions dearer each time and a loop counting
  into a map 3% slower, as the compiler had stopped inlining two helpers; now against 0.8.8 a
  sieve of 2,000,000 takes 65.4 ms where it took 71.0, counting into a map 25.3 ms (25.7), a map
  of lists 22.0 ms (22.5), and growing a list a million times 14.2 ms (14.4) (the 0.8.9
  benchmark).

## 0.8.8 (2026-11-19)

Found by the tests written for 0.8.7's coverage (Q-1100 to Q-1117).

### Security

- **Scripts in SVG and MathML: markup made as text is read as the browser reads it.** To tell
  whether a `<script>` is HTML's or SVG's, Quillo read `raw(...)` markup otherwise than the
  browser, and a script the browser reads as SVG could get HTML's form, with no CDATA, so an
  `html` value in it became elements: `<svg width=10/>` was taken as closed (the `/` is the
  value's), and a quote inside an unquoted value or a name as the start of a quoted one
  (Q-1112); a comment ended only at `-->`, where the browser also ends it at `--!>`, and at
  once for `<!-->` and `<!--->`, so an `<svg>` opened after one was taken for comment
  (Q-1113). `<!x>`, `<?x>` and `</ x>` end at their first `>`, and `<![CDATA[` is a section
  in SVG and MathML and a comment in HTML. `/>` closes only void elements and SVG's and
  MathML's, so `<div/>` in a `<foreignObject>` stays open and a script after it is written
  for HTML, without the CDATA section that kept it from running (Q-1100) (coverage of 0.8.7).
- **`raw("<font size=1>")` inside `<svg>` hung its request**: an unquoted attribute value, or
  spaces around `=`, had the server read the same place again and again, and the request
  held its thread for good. Attributes are read as the browser's tokenizer reads them, each
  step moving on (coverage of 0.8.7, Q-1111).
- **Scripts in SVG and MathML: where the browser is after an end tag, or after markup that
  stops partway.** An end tag ended the element opened last, not the one it names; an
  element a tag closes by itself (`<p>`, `<li>`, `<a>`, `<form>`, `<option>` and the like)
  was taken as still open; a script inside `<svg>` was taken to end at a `</script>` inside
  its own CDATA section; the page's own `</svg>` brought Quillo back to HTML where raw markup
  had left it unable to tell; and markup that stopped partway (`raw("<input")`,
  `raw("<!-- x")`, a last `<`) had the browser read on into the page's next markup. In each,
  a script the browser reads as SVG could be written for HTML, and an `html` value in it
  became elements. End tags end the element they name; where Quillo cannot tell, every later
  script on the page is written in the form safe in both readings; an SVG script ends where
  the browser ends it; markup inside `<svg>` or `<math>` that stops partway has its end
  written (`>`, `-->`, `</script>`, `&lt;`). A program that splits one comment, tag or
  script over two `raw` values now sees it ended after the first, and the server log says
  so once for each line. `annotation-xml` with `encoding="text&#47;html"` is no longer taken
  for MathML (coverage of 0.8.7, Q-1116, Q-1117). Each worker's page starts at HTML.
- **What Quillo writes after such a page is read as HTML.** Where raw markup left an `<svg>`
  open, the browser read what came after the page inside it: a deferred part's `<template>`
  became an SVG element and its script SVG's, as did the next part after one whose own raw
  markup lost the place, and the page states' script made `<img` an element and `&quot;` a
  quote that ends its string. Deferred parts on such a page are written in the form safe in
  both readings; an empty `<span></span>`, which ends SVG and MathML, goes before the page's
  `</body>` and after a part that lost the place; and the page states' script has its `<`
  and `&` as `\u003c` and `\u0026`, as the translations' script has. Raw markup that leaves an
  `<svg>` or `<math>` open, a start tag that closes an element by itself (a second `<li>`, a
  `<p>` after a `<p>`), and `</mi>` or `</mo>` with an HTML element open inside now make
  Quillo unsure where the browser is, and a view whose markup did so is not kept in the view
  cache (coverage of 0.8.7, Q-1116).
- **Pages with SVG render faster**: reading markup inside `<svg>` takes 39-49% less time,
  as tag names are matched at once where 0.8.7 compared them with a list of 44 on every
  tag. A page of 5,000 `<rect>`s answers in 0.90 ms where it took 1.04 ms, nested SVG in
  0.75 ms where it took 1.11; plain HTML pages are unchanged (coverage of 0.8.7).
- **`--max-memory` holds a site's answer.** `shop.notes()` read the answer to 64 MiB
  whatever the limit, and made its value whole: a 698 KB answer became 349,001 values (11 MB)
  under `--max-memory 1MB`. The answer is read no further than the limit and made into values
  held to it, and past it the request ends with a 500 that no `??` takes, as `http`'s does
  (it was an error value). Without a limit, an answer with more values than JSON of its length
  may have (1,000,000, or one for each 8 bytes of an answer over 8 MB) is an error value, where
  `http` leaves its `json` `null` (coverage of 0.8.7, Q-1105, Q-1109).
- **`json.stringify` of a very deep value stopped the server inside `parallel`.** A list
  20,000 deep, given to it in a `parallel` call's function (a smaller stack), overflowed the
  stack in the look for hidden errors before the JSON was written; that look goes on to a new
  piece of stack as the writer does, and the depth's error value comes back, as outside
  `parallel` (coverage of 0.8.7, Q-1110). In a debug build (`cargo run`, CI) 150 levels did,
  before the writer's first look at the stack; release builds are unchanged there (Q-1106).

### Fixed

- **`notification.send` and `/_quillo/push` at `--max-memory`**: subscriptions read past the
  limit end the request with a 500 that says it, as any other value does; a send gave an error
  value with the limit's words, which `??` took, and the request answered 200, and
  `/_quillo/push` answered "internal error" (coverage of 0.8.7, Q-1101).
- **A function in an `http` body is an error value**, as in `json.stringify`, and nothing is
  sent (``http: the body: `[0]` is a function, which cannot be written as JSON``): it went out
  as its text (`["<builtin len>",1]`). The body's JSON is held to `--max-memory` and to
  `json.stringify`'s limits, in `http.stream` too (coverage of 0.8.7, Q-1107).
- **`quillo check` and offline groups' middleware**: middleware Quillo cannot follow (a field of
  a map, a builtin, a call that makes the function, an index), whose pages the server never
  keeps, is warned of, as is a lambda a top-level `let` holds that reads the request past the
  session; the check said nothing of them (coverage of 0.8.7, Q-1102).
- **`quillo check` and a `GET` that writes**: a request pattern's `|` alternatives, and one
  under `as`, are looked through, so `{ query: q } | { cookies: c }` reads the visitor; it read
  no one, and the warning did not come (coverage of 0.8.7, Q-1104).
- **A group's slips said as such**: `routes offline/help { ... }` says a space goes between
  the tag and the path, and `routes admin-tools { ... }` that the path starts with `/`; both
  said "`routes` is no keyword: did you mean `route`?" (coverage of 0.8.7, Q-1103).

## 0.8.7 (2026-11-18)

- **Installable apps**: a program with an `app.toml` beside it can be installed from the
  browser, with nothing else written. Quillo serves `/manifest.webmanifest`, icons made from
  `icon` (192, 512, a maskable 512 and Apple's 180 at `/apple-touch-icon.png`) and three tags in
  each page's head (151 bytes); the file is the one the phone builds read, with `short_name`
  (cut at a word), `theme_color`, `background_color`, `display`, `start_url` and `description`
  added. The manifest's `id` is the scope, never the phone's bundle id. A route of the
  program's at the very path wins, and one that takes any path wins only with a real answer
  (a `2xx` that is not HTML, or a `3xx`), so a 404 page, a front end's `index.html` and a
  guard's `401` keep the app installable. `quillo check` says the file's mistakes at their
  lines (errors, and warnings for what the phone builds take), and `--watch` loads the
  program again when it or its icon changes. A program without `app.toml` is served as before,
  byte for byte (FEEDBACK67.md, B.1).
- **Pages that work offline**: `route offline GET /x` and `routes offline /x { ... }` keep a
  page on the device, with a service worker Quillo writes (`/_quillo/sw.js`): pages from the
  network first, the copy marked `data-kept` (or an offline page named from `app.toml`) when
  the network fails or a proxy answers `502` to `504`, routes without parameters kept as the
  worker installs (a page Quillo is too busy to prefetch asked again within 5 s), a deploy's
  new worker keeping the old copies until it has its own. A page
  that reads the visitor through `session()` alone is kept for that session only, behind a
  gate that checks the `quillo_tag` cookie, and dropped when the session changes, ends or is
  no longer taken; one that reads them another way, holds a form key, has waiting parts or
  changed the session is not kept (`quillo check` says where a route reads them another
  way). No worker under `--watch`, in the phone apps or in `quillo app`; a program without
  offline routes pays nothing (FEEDBACK67.md, B.2; Q-958's rules for a worker of the
  program's own are in the guide).
- **Notifications**: `notification.ask()` in a handler turns on a device's notifications
  (`"granted"`, `"denied"`, `"default"` or `"unsupported"`), `notification.state()` reads it
  without asking, and `notification.stop()` turns it off; `notification.send(user, { title,
  body, url, tag, badge, silent, ttl, urgency })` reaches that person's devices with the page
  closed, over Web Push sent by Quillo itself (RFC 8291 and 8292), and gives `{ sent, gone }`.
  Subscriptions are bound to `session().user`, or to a sealed `notification.user(x)`, kept in
  `_quillo_push`, and unbound by `end_session()`, any change of person, and a page that finds
  its device bound to someone else. At most 20 devices a person; sends go 8 at a time within
  30 s, over kept connections (Q-960); push services are https on port 443. The push key comes
  from `QUILLO_SECRET`, the same on every instance (Q-956). The phone apps answer
  `"unsupported"` for now (FEEDBACK67.md, B.3).
- **A `GET` that writes**: `quillo check` warns of a `GET` (or `ANY`) route that surely writes (as the
  prefetch guard reads writes: `db.exec` and SQL written to write, a global assigned, cookies
  and sessions, files, `publish`, outbound writes, services, `notification.send`) and reads who
  the visitor is (the request, `session()`, `cookie()`, there, in what it calls, a component it
  shows or its middleware): a link on another site opens it with the visitor's session. `route writes GET /x`
  and `routes writes /x { ... }` say the write is meant; three or more routes through one write
  are said once, at it; `offline` with `writes` is warned of. The sample apps' meant writes are
  tagged (FEEDBACK70.md, Q-965).
- **`Origin`'s scheme**: a write with an `Origin` and no `Sec-Fetch-Site` whose scheme differs
  from a `--trust-proxy` proxy's `X-Forwarded-Proto` (the last item of the last header) is
  refused as another site's, and the log says so once (FEEDBACK70.md, Q-966). Quillo sends no
  `Strict-Transport-Security` itself, as it serves plain HTTP: the proxy does, or
  `site_headers` (the guide says how).
- **Service callers**: a call signed with the shared secret cannot claim a name
  `accept_calls { keys }` holds (401), and where `accept_calls` names callers by keys,
  certificates or tokens it names no caller at all: `for` refuses it and `_contracts` lists
  only what has no `for` (FEEDBACK71.md, Q-970). A caller proven by a SPIFFE id is named by
  the whole id, as Istio names principals; a `for` written with its last part is refused with
  the id to write (Q-971). The forwarded-certificate header is read as Envoy writes it, one
  element unless `client_cert: { chain: true }`, its lines joined, held to Envoy's keys, order
  and quoting with nothing trimmed, and only a SPIFFE id, checked against the spec, names a
  caller by its URI; `client_cert: { subject: true }` reads a header that is a subject alone
  (nginx's `$ssl_client_s_dn`), as the form is never guessed (Q-988, Q-1000). A subject's CN
  is its CN attribute, read in RFC 2253's form with its escapes decoded; the slash form, two
  CNs, a CN with `:` or a subject that does not parse name no caller (Q-989).
- **Outgoing calls**: a method that is not an HTTP token is an error value before anything
  connects (Q-972). `site` calls go as `http`'s do: the host's policy, the environment's
  proxy, same-origin redirects only (a `file:` one was a 500), one 30 s deadline, the site's
  cookies along the way (Q-973). `cache` keeps answers apart by every option and a GET's body
  (one client certificate's answer went to calls with another), and `timeout` covers a call's
  redirects (Q-974).
- **Large values and slow patterns**: `--max-memory 512MB` (and a policy's `max_memory`) ends a
  request that would make one value bigger than that, alone, with a 500, its size compared
  before the value is made; unset, nothing is checked (FEEDBACK71.md, Q-977). Each thread keeps
  32 MB of compiled regexes at most, the oldest going first (a visitor's pattern held 11 MB a
  request, Q-975); `find_all`, `replace` and `split` stop at `--max-compute`, and a pattern that rereads
  the text is an error value (300 KB had held a worker 11 s, Q-976). `--split` passes
  `--max-compute` on to its services (Q-1001).
- **Scripts in SVG and MathML, and spreads**: `raw(...)` in a `<script>` the browser reads as
  SVG or MathML goes in CDATA, following the parser's own namespace (a view's children where it
  puts them, markup made apart and placed there later too), where visitor markup got out
  (FEEDBACK71.md, Q-969); a spread `name` or `id` that `document` would take as its own property
  is dropped on the elements that give it one (Q-979); `<sCript>` and `<sTyle>` in another case
  are refused where the program is read, as the browser ran them as scripts that Quillo wrote as
  any element (Q-990); the browser's `local_path` refuses C1 controls, as the server does (Q-968).
- **Local files and tools**: a failing key table in `idempotent` answers 503 without the
  database's words, which go to the log (FEEDBACK71.md, Q-978); a hidden character in a tool's
  `///` or schema strings, or in a tools program's file name, is an error, and joiners pass only
  between visible characters (Q-980); `quillo install` writes each file through a temporary file
  of its own and refuses links (Q-981); program sources are read as regular files of at most
  16 MiB each and 32 MiB a program, command-line pipes under `/dev` read once (`quillo run <(...)`
  now runs; Q-982); the plugin cache's folders above it are checked as OpenSSH checks a home, each
  of its folders is made 700 and checked itself, links and other owners are refused, folders others
  could write in are tightened and rebuilt (never under `QUILLO_NATIVE_CACHE`, which is refused
  instead), and cargo is found first, then run in `/` (Q-983).
- **The request's scheme**: `X-Forwarded-Proto` is believed only from a `--trust-proxy` proxy, for
  `delete_cookie`'s `Secure` and a push subscription's origin as for the form gate; where it would
  have counted, an unnamed sender is said once; `--split` services take the scheme from their app,
  signed, and believe no visitor's header (FEEDBACK70.md, Q-986). Under `quillo fcgi` the web
  server's `HTTPS` (else `REQUEST_SCHEME`) is the request's scheme and a visitor's own header is
  dropped, so the gate, deletions and push origins follow it (Q-987).
- `http.*`'s `public_only` no longer takes 64:ff9b:1::/48 (RFC 8215's local-use NAT64) as the
  public internet (found in FEEDBACK67's review).
- `ask` and `notify` where the code surely runs on the server (a route, a `server fn`, a tick,
  a job, the top level, a page state's initializer) are refused by `quillo check`, with what
  reaches someone who is not on the page: `notification.send`, and `publish` for open pages.
  They passed the check and failed every call with a 500 (FEEDBACK67.md, Q-959).
- Under `--watch`, a page a service worker answers from its cache reloads once, then stops and
  says why, in the page's error overlay and the console: it reloaded without end (305 times in
  4 s under `--watch=reload`), or never showed the change (FEEDBACK67.md, Q-957).
- A page state's initializer that reads `session()` reads the visitor's: it ran before the
  request was there, found no session, and every `session()` after it in that request gave
  `{}` (FEEDBACK68.md, Q-985).
- A pattern's miss names a type of the program's with its article: `not an Outer`, still `a
  User` (it said `a Outer`; FEEDBACK58.md, Q-874).
- Editors (quillolang/editors, released on its own): `offline` after `route` and `routes` is
  highlighted as a keyword in every editor's grammar, as `quillo lsp` already completes and
  explains it.
- **Security: redirects through dot segments.** `redirect` wrote a path of the site's own whose
  `.` and `..` hid a second `/` (`/.//evil.example`, `/%2e//evil.example`,
  `/a/..//evil.example`) as `Location: //evil.example`, which a browser takes to that host,
  through the guide's own `redirect(local_path(next) ?? "/")`. It now writes one `/` first
  (`/evil.example`, on the site), and `local_path` is null for such a path, on the server and in
  the browser (FEEDBACK71.md, Q-968).

## 0.8.6 (2026-11-16)

- The page `files()` sends for a miss (the folder's `404.html`, or its `not_found`) goes out as
  written, as the guide had it: it was given the prefetching script, and `--watch`'s, for having
  lost its `ETag` (FEEDBACK69.md, Q-964).
- The guide's "A front end built elsewhere (React, Vite)": a built single-page app's `dist/`
  served beside the program, deep links answered `200` with its page, its hashed files kept a
  year, and its calls to the program on its own origin.
- Under `--watch`, a component swapped in place at a save keeps what its elements held that no
  state does: the focus and its selection, a box's scroll and the page's, what was typed in a
  field nothing follows, a box ticked, an open `<details>` or `<dialog>`, an `<audio>` or
  `<video>`'s time; they went with the swap (FEEDBACK54.md, A.4).
- Under `--watch`, `r` and Enter typed where the server runs loads the program again with its data
  started again, nothing carried, without a restart (FEEDBACK54.md, Part C).
- A SQLite migration that rebuilds a table others point at, with `pragma foreign_keys = off` as
  SQLite's own steps write it, keeps their rows: the pragma did nothing inside the file's
  transaction, so dropping the old table ran the children's `on delete` actions, deleting or
  blanking their rows while `db.migrate` said it worked. Foreign keys are now off for that
  file, checked before it commits (a row left pointing at nothing rolls it back), and on
  again after. A file without the pragma that drops such a table gets a warning
  (FEEDBACK62.md, Q-915).
- An answer made from who the visitor is stays `private`, whatever its route said: `public`
  (or `s-maxage`, a bare `max-age`) from `respond`, `file`'s `cache` or a `site_headers` rule is
  sent as `private`, its time kept for the browser, and so is any `cache-control` that says
  neither `private` nor `no-store`; the fields a CDN reads first (`CDN-Cache-Control`, `Surrogate-Control`, `X-Accel-Expires`) are
  left out. A cache in front that ignores `Vary` gave one signed-in visitor's file to the next.
  The log says so once for each path, and `quillo check` warns where a route writes it
  (FEEDBACK59.md, Q-903, the user's choice).
- A page under a `content-security-policy` that refuses inline code (the guide's own
  `default-src 'self'`) runs its components. Quillo's own scripts are named by their hashes in
  the policy, and the islands' `display:contents` is a hashed `<style>` rule; its links are
  prefetched with `fetch`, as Chrome's speculation rules pass no policy that names a hash. The
  page looked right while nothing on it worked, and nothing said why (FEEDBACK59.md, Q-904).
- `platform().dev`: true under `--watch` alone (FEEDBACK62.md, Q-920). Under `--watch`, the
  folders `db.migrate` reads are watched, so a migration that arrives is applied, and one that
  stopped the first load is tried again once fixed; a new file with no statement yet waits for
  one, as an empty file applied was recorded and its SQL never ran (Q-919). Started
  on an address beyond this machine, `--watch` says that anyone on the network can reach the
  program (Q-918). `quillo check` takes `db.migrate` as giving a list, as a failure stops what
  runs it (Q-923).
- A field limited to a set of strings (`type Service = "oil_change" | "tire_rotation"`) takes its
  values from a form and a query, as from JSON: it refused every one with `the request does not
  match its pattern`. A miss names the field and the type. A set written out in the pattern
  binds no name, and `quillo check` now says so where the name is used (FEEDBACK58.md, Q-874).
- A `server fn mcp` that reads `request` (`request.mcp`, a stage `request | auth.required |
  ...`) works under `quillo mcp`. It passed `quillo check` and worked over HTTP, and every call
  over stdio failed with `internal error` (FEEDBACK63.md, Q-927).
- The guide names SQLite 3.54, which Quillo has had since 0.8.1.

## 0.8.5 (2026-11-16)

- **Page state**: `state cart = []` at the top of a program is the page's, shared by every
  component on it. Components placed apart read and set it alike, one value for each page load,
  for each visitor. The server makes the page with it (a route may set where it starts), and the
  page carries it; a component that shows one starts with the page. A `server fn` reading one is
  an error. Programs without one pay nothing measurable (FEEDBACK68.md, Q-961 to Q-963).

- Under `--watch`, a save that does not load is said over the open pages, which keep what they
  hold, until a save loads; it said nothing, and the pages showed the version before it as if
  the save had worked (FEEDBACK54.md, Q-686).
- Under `--watch`, a component whose code changed is swapped into the open pages in place, no
  reload: its states (and those of the components inside it) keep their values while their kind
  is the same. A change to the page's own HTML outside its components (text, attributes, the
  title, a node added at the end) is put in place without a reload. A page that must still reload
  keeps its states, the fields typed into, open `<details>`, the scroll and the focus. On the
  server, a top-level `let` the program changes keeps its value across a save while its kind is
  the same, and the reload's line says what was kept; a `:memory:` database starts again, said
  too (FEEDBACK54.md, A.2, A.3, A.6, Part C, Q-687).
- Under `--watch`, a change a page does not use (a route it never calls) no longer refreshes it:
  the page asks for itself again and stays when its HTML and code are as they were.
  `--watch=reload`, or `<html data-reload>` on a page, refreshes at every change, as before
  (FEEDBACK54.md, Q-685).
- Under `--watch`, with no `QUILLO_SECRET`, sessions are signed with a key kept in
  `.quillo/.quillo-secret`, so a restart no longer signs you out (FEEDBACK54.md, Q-689).
- In development, a component's file and its address stay the same when another component is
  edited, so the browser keeps what did not change (FEEDBACK54.md, Q-688).

- A component's callback may answer on the channel it is the callback of: `let room =
  channel.open(name, (m) => room.send(...))` passed `quillo check`, and the server then did not
  start ("unknown name `room`", from the component's compile for the browser). A function in a
  `let`'s initializer may use the name it is given, as on the server (FEEDBACK66.md, Q-950).
- A component with `channel.open` loads as soon as the page is idle, as one with `listen` does,
  and hears what the server sends before anyone touches the page; it waited for someone to reach
  for it, unless it was the page itself (FEEDBACK66.md, Q-953).
- `quillo check` no longer says a channel's handle is "a value from the page shown in markup"
  when a handler sends on it, or when its callback calls `js(...)`, with a fix that did not
  compile; what does show a page's value is still warned of (FEEDBACK66.md, Q-952).
- `quillo dap` no longer verifies a breakpoint on a line only the browser runs (a component's
  handler, a channel's or a timer's callback), which never stopped: it says so, and a coverage
  tool on the debugger can leave such lines out (FEEDBACK66.md, Q-951).

## 0.8.4 (2026-11-15)

- A page on a channel no longer takes three threads on the server, nor a page with live data one,
  each with memory of its own: their sockets are read and written on one thread that every such
  page shares. 1,000 pages on a channel cost 12 MB, where they cost 562 MB and kept 350 MB after
  they closed; 1,000 with live data 8.5 MB, where they cost 240 MB and kept 206 MB; nothing is kept
  now. A burst to three pages on a channel costs half the CPU it did, one to 1,000 pages a
  hundredth, and an answer to a page comes 5% sooner; a change followed by 1,000 pages with live
  data costs 71% less. A change told to one page at a time, among many that are quiet, costs
  about 6,000 cycles more (2 µs), the wake of the one shared thread (FEEDBACK66.md, Q-954).
- The server counts that thread in the threads it says it may use when `ulimit -u` is low:
  `--workers` + 10, 74 with the default 64.

## 0.8.3 (2026-11-15)

- A page with nothing to prefetch (no link, no `<script>`, no component) no longer carries
  the prefetch script: it is sent as the route made it, 1,334 bytes smaller. TechEmpower's
  Fortunes page is now 1,226 bytes, as Bun's and Node's are (it was 2,560). Pages with a
  link are unchanged. With `--metrics`, such a page still reports being opened from a
  prefetch.
- Faster pages: HTML escaping looks for the characters it changes 16 bytes at a time (NEON on
  64-bit ARM, SSE2 on x86-64), short pieces of a page and of the answer's head are copied
  without a call to the C library, a record's field is found without one, and the page's checks
  for links, scripts and a stream's player are one pass. TechEmpower's Fortunes takes 7.8% fewer
  CPU cycles a request (24,259 against 26,318 for 0.8.2's code with the page as small; Bun
  27,380), from SQLite 8.5% fewer, JSON 5.5%, text 2.6%. A page whose only script is written `<Script>` now gets prefetching too.

## 0.8.2 (2026-11-14)

- `quillo check` warns when a call of the program's own function that changes nothing is
  dropped, in a handler or a function's code: `onclick={() => down(val)}` did nothing and nothing
  said so. It says what to write (`val = down(val)`), and for a `server fn` that its arguments
  are copies; the fix is an assignment only where what it gives fits the variable. A call is left
  alone when the function assigns one of the program's variables or one it did not declare, sets a
  page object's field, calls a method on what may be a page object or a map of functions, prints,
  waits (`sleep`, `page_updated`), schedules (`run_after`, `every`), publishes, reaches the
  database, the network or files, hands a function it was given to a builtin that calls it, can
  give an error (that warning says so), or calls what the checker cannot read; and at the top
  level of a program (FEEDBACK64.md, Q-931).
- `set(xs, [0], v)` and `set(xs, "a", v)` on a list are refused by `quillo check`: a list's index
  is an int. They had passed, and failed every time they ran. On what may be null, a key is taken,
  as `set` makes a map of null (Q-932).
- `xs[0] = 5`, `xs[0] += 1`, `m["k"] = 5` and `m.k = 5` are refused with what to write:
  `xs = set(xs, 0, 5)`, `m = { ...m, k: 5 }`, and for a page object `el.value = x` (Q-934).
- Under `--watch`, a `server fn` that fails says why in its 500, with its file, line and the calls
  it came through, so the page's error value reads
  ``server fn `down` failed (500): main.qwe:9: division by zero``, and
  the browser's console says it too; a visitor still sees only `internal error in down` (Q-933).
- llms.txt says what a top-level `let` is: one value for every visitor, reset at a restart and
  on each save under `--watch`, one per instance (Q-935).
- Requests are answered with less work: what each request is (for the log and a stop's
  progress line) is kept by its event loop and made without `format!`, not in one map for all
  of them behind one lock; a path or query with nothing to decode is not decoded byte by byte;
  a route's path already in its form is not split and joined again; and a page is searched for
  a `<quillo-media>` player forwards, which memchr does with SIMD, not back from the end. Nothing
  a page or the log says changed.

### From a site in production: memory and connections (FEEDBACK65.md)

From qwertime.com, live on 0.8.1 behind two servers and Limer. Fixed (0.8.1 too):

- **A lambda kept in a variable no longer keeps its scope for ever.** `let f = (x) => ...` (or a
  nested `fn`, or a record or list of them) held the scope it was made in, and the scope held
  it, so neither was freed: every request leaked all the scope held, a page's rows or an
  upload's bytes, until the server was killed for memory (qwertime: four times in five
  minutes). The scope is freed when the block, call, loop step, `match` arm, view, component,
  route or script ends, or once what held the lambda lets it go. Lambdas still share and assign
  the variables around them. A function ending in `return`, a view or component body, a loop
  or call left by an error, a match binding and a route parameter leaked the same way. Freeing
  costs a function that keeps a lambda a few hundred instructions a call (Q-941).
- **A kept-alive connection is idle from the end of its last answer**, as nginx counts it. A
  connection whose answer took longer than 60 s (an `events()` stream, a long download) was
  closed within a second of its end, without `connection: close`, so a proxy's next request on
  it was cut off: through Limer, failures against the server and, with many streams ending
  together, both servers out of the pool every two minutes. A streamed answer now says
  `connection: close` whenever the connection will not be kept, a program's own included, and
  a request that had partly come behind a long answer is timed from that answer's end, not
  given a 408 right after it (Q-942).
- A pipelined request's `100 Continue` comes after the answers to the requests ahead of it, not
  before them (Q-948).
- A pipelined request whose last bytes came while large answers ahead of it were made is read
  and answered, not left for a 408 after 30 s (Q-949).
- A webhook signed in base64 (Shopify, WooCommerce) can be checked: the catalog's `encoding`
  plugin 1.1.0 turns hex into base64 and back byte for byte, so
  `hex_to_base64(crypto.hmac(key, request.body))` is the signature; the `webhooks` plugin
  checks the whole webhook. No core change (Q-943).

### Pages that change a lot, and live data (FEEDBACK61.md)

From a three-pane console, a one-page tracker with live data and two visitors, and lists
four deep, on Chromium, Firefox and WebKit: Q-889 to Q-902 and Q-936 to Q-940 (Q-893 and
Q-898 came in 0.8.1).

Lists and rows:

- **A row whose root changes after it is made moves as it is now**: a `? :` or `if` at its
  root, a view or function with a branch at its root, a table-row component whose root is a
  branch. The row kept the elements it was made with, so its next move brought an old
  element back with its old handlers, lost real rows, or stopped the update. A branch that
  showed a list takes all its rows with it, those added later too (Q-889).
- A row of several elements keeps its scroll when it moves, and its frames do not load
  again; in Safari, which cannot move a frame, they still do (Q-894).
- A `for` row's `key` that reads other state (`` key={`${ver}:${x.id}`} ``) makes its rows
  again when that state changes; it waited for the list's next change (Q-895).
- A row whose other branch shows nothing (`if` with no `else`, `null`, `false`, `true`,
  `{ }`, `&&`) is keyed by the branch that has a key, so an `if` that filters a list keeps
  each row's typing and focus through a reorder. It was kept by place, and made again on
  every reorder (Q-897).
- A row's `key` whose code fails is said in the console, once while it goes on failing, and
  the row is given a key of its own, so the list shows and keeps up; the error stopped the
  whole list.
- A `run_after` that has fired leaves nothing behind: a poll that starts the next one as it
  ends kept a little more of the page's memory at every tick (10 polls of 400 ticks: 464 KB).

What a person typed:

- **A function's markup made again keeps what a person typed, ticked, picked or opened.**
  `value`, `checked`, `selected`, `muted`, `indeterminate` and `open` are written only when
  the markup writes them and says something new. The focus stayed and the text went
  (Q-890).
- **A field rewritten as it is typed keeps its caret** in the text that did not change:
  capitals, a refused key, a card or phone mask, a trim. It went to the end, so the next
  letters landed after it (`AXYZB` became `AXBYZ`) (Q-891).
- A field is not written while a person composes in it with an input method; the program's
  value goes in when the composition ends, and a field the program did not set keeps what
  was composed. Rewriting it broke the composition (on main too, Q-939).
- No handler runs while Quillo takes elements off the page, so in Chrome a field that its
  Enter handler hides no longer saves a second time on its `blur`, as Firefox and Safari
  did not; a dropped card's `dragend`, which comes after, still runs (Q-896).

Tables, selects and SVG taken up from the server's page:

- **A component of table or select parts that is not one element** (several rows, a list,
  a branch, nothing) is marked with comments, which the HTML parser leaves in the table,
  and taken up as written: it was written as a `<quillo-island>` that the parser moved out
  of the table, so the browser made the whole page again as it loaded, erasing what was
  typed before the code came. Placed straight in a route's page, it is still made again
  unless its root is one element (Q-892, Q-936).
- **Rows written straight into a `<table>`** with no `<tbody>` are taken up, with a `<thead>`
  before them or a `<tfoot>` after, bare `<col>`s, and a component that wraps a row
  component; the page keeps the `<tbody>` the parser made. Their component was made again
  on every load (on main too, Q-940). A list whose items mix `<tbody>` or `<thead>` items with
  `<tr>` rows is still made again.
- In the browser, a component made inside a table part, a `<select>` or an `<svg>` has no
  `<quillo-island>`: a branched SVG component is drawn, where it was not; a `<select>`
  keeps its value when an option component is made anew; and a table-row component whose
  root is a branch is a row, as on the server's page (Q-937, Q-938).

Timers and streams, and whose they are:

- **A timer or stream that an event handler starts is its component's**, after the
  handler's waits too, so it outlives the row or branch that held the button (the user's
  decision). After a wait, `every` refused to start (`` `every` works in a component ``) and
  `run_after` outlived its component; `listen` refused in any handler; a parent's function
  that a child calls and then closes (a modal's Save) lost its timer with the child (Q-901).
- One that a part's own code starts (a statement, an `@mount`) ends with that part, its row
  or branch, as before, and so does what a timer's tick or a stream's message starts, which
  outlived the component. Markup is its writer's code wherever it is shown (passed as
  `children` or another prop, given back by a function prop, kept in state), so its
  handlers' timers are the writer's; markup still shown after its writer has gone gives
  them to the part showing it. A function handed to the page (a library's callback, a
  listener) runs as the code that handed it over (Q-901).

Live data:

- **A `live` load that fails is tried again** after a second, then two, doubling up to 30 s,
  until one comes back, and the page keeps its data meanwhile. It had given up until the
  next change, which a quiet table may never make. Only a failed connection, a 408, a 429
  and a 5xx are tried again, never before their `Retry-After`; another 4xx, a page that is
  not the server's answer (a sign-in page) and the loader's own failure (its error, or a
  run past `--max-compute`) are said and not tried again. The tries leave `waiting()` false,
  so a page that shows a spinner while waiting keeps its data; a load for new arguments
  stays waiting until it comes (Q-899).
- Quillo's own answer that a `server fn` failed is marked `x-quillo-answer: failed`, so a
  program's `reply 500 {...}` or `respond(500, ...)` is tried again and the loader's own
  error is not (Q-899).
- **The server answers a page's socket close at once**, with the page's code. It read
  nothing a waiting page sent, so a close waited for the next 5 s check, and Firefox then
  held new sockets back for 10 to 45 s and more: a visitor moving between pages at an
  ordinary pace heard no change meanwhile (Q-900).
- A page keeps its socket as its topics change (moving between the pages of a single-page
  app) and sends the new topics down it; it closed it and opened another for each page. A
  server that does not take topics this way, an older one during a deploy or one on
  Windows, gets a new socket as before (Q-900).
- A socket that has not opened within 3 s goes on trying while plain requests carry the
  changes, and takes over when it opens, with no change missed or shown twice. With no
  socket to be had, sockets are tried again later each time, up to every 30 s; the plain
  requests back off, wait for a `Retry-After`, and stop at a 4xx other than 408 and 429; a
  page that falls behind says so once in the console, at the third failure, or at once when
  it is refused (Q-900).
- A write tells many waiting pages sooner, and idle pages cost less: 1,000 pages on one topic
  are all told within 9 to 14 ms, where they took 16 to 31, and 1,000 idle pages use 0.12 s
  of the server's CPU a minute, where they used 0.50 (Q-900).

Navigation:

- **A `navigate` after a wait is dropped if the page moved meanwhile**, by a link, Back or
  Forward, or a run that started later (the user's decision): a save that answered after
  the person had gone to another page moved them anyway. It still goes when only the
  component that asked has gone, as a modal does whose Save closes it. On a page that never
  reads `url()`, a link or form the browser follows counts as moving, so a late `navigate`
  no longer cancels the page the person asked for (Q-902).

### `return` (FEEDBACK52.md)

Added, as warnings of `quillo check` (no program runs differently):

- **A `return` that leaves the function from a value that is kept.** In a block or a `for`
  body it is always said: `let ys = for x in xs { return x }` leaves in the first round and
  makes no list. In an `if` or a `match` it is said when every way leaves: ``every way of
  this `if` leaves `pick`, so the value it is used for is never given: to give it, write
  `1`; to leave, write the `if` as a statement``. A guard is not said:
  `let r = if b == 0 { return 0 } else { a / b }`, `?? return null`, `"a" => return 1`,
  nor an `if` or `match` in a way of one whose every way leaves: only the outermost
  decides (Q-678).
- **A function that returns a value on one path and gives null at its end**:
  `` `sign` returns a value on line 3 but gives null when it reaches its end: end it with
  a value too, or `null` if null is meant ``. So is a last `if` with no `else` whose way
  is a value, and a last `for` whose rounds give nothing, which gives a list of nulls.
  `else { null }` or a last `null` says null is meant. Not said: a function with a return
  type, a view, a component, an arrow, a function handed to other code (a middleware, a
  callback), a markup helper, and a last `for` whose rounds end in a call. `quillo check
  lib.qwe` alone may warn of a middleware that only its importer hands to `routes`: check
  from the main file (Q-679).
- **Code that never runs**, after a `return`, a `reply`, or an `if` or `match` whose every
  way leaves: `` this never runs: the `return` on line 2 always leaves `g` ``. After a
  `return` at the end of a line, which takes no value from the next one, it says why:
  `` a `return` at the end of a line gives null, so the one on line 2 leaves `f` before
  this `` (Q-680).
- Over 5,139 programs (app/, the website and its plugins, quillo-components, the examples,
  the repros, the guide's and llms.md's blocks, the tests' programs), the only new
  warnings are four true ones, in tests that put code after a `return` on purpose.
  `quillo check` takes 0.4% to 0.5% more instructions on the largest programs.

Fixed, in compiled numbers:

- **A function that gives its result with `return` is compiled**, at its end, early, or in
  both arms of an `if` (`c ? x : return -1` too), and so is every function calling it. A
  block ending in `return` had no type, so the function and its callers ran in the
  interpreter: 5 million calls of a function ending in `return`, or with an early one,
  took 1.1 to 1.4 s, and now 0.02 s (Python: 0.64 to 0.73 s). `bench/vs-python` has a
  `return` and an early-`return` case (Q-677).
- A block in a numeric function that declares one of the function's names with another
  type (`let y = { let x = 2.5; ... }` where the function's `x` is an int) stopped the
  process with a Cranelift panic (0.8.1 too).
- A recursive numeric function could stop the process the same way: what was planned
  under a guess of its type that did not come out was kept (0.8.1 too).
- After a compile refused part way, the next compile stopped the process ("anonymous
  symbol must be defined locally"): two functions calling each other and a helper compiled
  before them were enough (0.8.1 too).
- A recursion past the 10,000-call limit took up to 80 times the interpreter's time, as
  the machine code was run again at each level (1.35 s where the interpreter takes
  0.06 s; with 1,000 calls of itself a level, 114 s): the calls below the one that gave
  up run in the interpreter now, in about its time (0.93 s), a runaway recursion in a
  function with `return` such as `fact(-1)` too (Q-945).
- A recursive numeric function keeping many values across its own call could stop the
  program ("stack overflow"), or a server (SIGBUS), when it went deep on a small stack,
  such as in a `parallel` child: with 57 floats kept, `parallel(() => big(9000, 1.5), ...)`
  crashed where the interpreter gives its value. Compiled code now counts each function's
  own frame, not 512 bytes a call (Q-947, 0.8.1 too). A compiled function keeping
  thousands of values across a call it makes, called at every level of a deep recursion,
  could stop the program the same way: with 9,000 floats kept it crashed where the
  interpreter gives its value (0.8.1 too).
- A function calling one already compiled on its own was not compiled (0.8.1 too).
- Compiled code looped over the builtin `range` where `range` was something else: a
  parameter or a variable named `range` (`fn g(range, n)`), or a function compiled in one
  scope and run in another where `range` is shadowed. `range` is now read from the
  function's scope when it is compiled. A `let` that makes `range` something else after
  that is not followed yet (Q-944, as in 0.8.1).

## 0.8.1 (2026-11-09)

- SQLite 3.54.1 (rusqlite 0.41, libsqlite3-sys 0.39.1), for RUSTSEC-2026-0459 (CVE-2026-119375):
  SQLite before 3.54.0 could give uninitialized page-cache memory for a database opened with a
  `-wal` file of another page size, which only a database someone less trusted could write
  exposes. It was 3.46 (libsqlite3-sys 0.30.1).
- Pages are served faster: a page's server side searched the finished HTML byte by byte, put the
  prefetch script and the bundle's tags in a byte at a time (the script made anew for each page),
  and made a system call for each request's id; small lists and maps were freed on another
  thread, mimalloc's slow way. memchr's search, a move and a copy, the script made once, ids from
  a pool of the system's random bytes kept by each thread, and what holds fewer than 256 items
  (two levels down) freed in place: about twice the requests a second on the Fortunes and SQLite
  pages of the server benchmark, ahead of Node and Bun with six processes. Nothing a page says
  changed.
- SQL of two statements given to `db.query` or `db.one`, or to `db.exec` with values, is an
  error on SQLite, as on PostgreSQL and MySQL: SQLite's had run the first and dropped the rest
  without a word, a write lost. `db.exec` of statements without values runs them all, as before.
- A file the program serves whose first bytes begin as a SQLite header could, written in place
  while it is read, is not sent from the start unless those bytes read the same twice: a read
  could give them torn, a database's header begun with the old file after it, which no check of
  the whole header saw, and a part of 5 bytes sent `SQLit` (a 503 to come back to for a large
  file, as when it is replaced; found by CI). Any other file written in place is served as it is.
- A stop (`kill <pid>`, SIGTERM, Ctrl-C) waits for every request and transaction in progress,
  however long it takes, a large file being sent too, saying every 5 s what it still waits for;
  `--drain 30s` gives it a time, after which what still runs is cut off (`quillo run` and
  `quillo mcp`; `quillo app`, which stops with its window, refuses it).
  Before, 10 s, then cut off. The port refuses new connections at once, even while a request
  holds the server computing (it had queued them, and reset them later); an answer given while
  stopping closes its connection and says so (`connection: close`), where the client's next
  request on it had been reset; a stream's `sleep` ends at a stop as its `send` does, unless it is
in a transaction. Services
  `--split` started are no longer killed when the app has answered its own requests: each
  finishes what it is doing (a call, a call made later, a job) under the app's `--drain`, and
  the app waits for it. They run in a process group of their own, so a Ctrl-C in the terminal
  reaches the app alone, and one signalled while its app runs (systemd signals every process of
  a unit) serves the app until it stops: they had stopped taking calls while the app answered
  requests that needed them. A second stop ends them with the app, at once (the user's graceful
  stop, 2026-11-09).
- `quillo check` warns where an `idempotent` run in its transaction reaches a service's or a
  site's function that may change data, directly, through the program's functions, or through a
  service that calls another: that change is outside the transaction, so when its answer is lost
  the key is given up and the client's retry makes the change again, under a new call id. The
  warning names `{ transaction: false }` and keying the change by an id the request sends
  (the user's decision, 2026-11-09: lost answers between services are lived with, and said
  where they matter).
- After a panic part way through `db.atomic` on MySQL or MariaDB, a part prepared on the
  connection the panic closed is ended at once on another, as the server allows once it has
  seen the close; before, the server's "unknown XID" was taken as done and the part waited a
  minute for recovery with its rows held (FEEDBACK56.md, found under load).
- A panic inside a transaction rolls it back and frees the connection on every database; on
  SQLite it had left the savepoint open, so later writes went uncommitted. `db.atomic`'s too:
  its parts end as decided, committed where the decision stands, rolled back otherwise. A panic
  in a PostgreSQL call no longer loses the connection's place in the pool, and a panic as a
  transaction begins leaves nothing open (FEEDBACK56.md, found in review).
- Another request's SQLite write no longer lands in `db.atomic`'s SQLite part while the atomic
  waits for a server database's connection: SQLite's part begins last. Before, the other
  request could roll the atomic's savepoint back, and the atomic's own SQLite write was then
  committed although it ended in an error (FEEDBACK56.md, found in review).
- `fn mcp name(...)` and `server fn mcp name(...)` declare tools for AI clients, and
  `service mcp /path = "file.qwe"` a service whose tools are the program's. `quillo check` holds
  them to MCP's names and to data answers, and warns of a tool with no `///`, an untyped
  parameter, or an effect its body uses that its `with` does not say; highlighting, hover, completion and the outline know the tag.
  `mcp` stays an ordinary name everywhere else (FEEDBACK56.md, Q-853).
- `service mcp /billing = "billing.qwe"` offers billing's own tools as `billing.<name>`, run in
  billing in one process or over the signed call under `--split` (signed with the app's key too,
  so only the app's door runs them), with the caller's session and tenant; a service function's `confirm` and `about` ask the person through the program's door
  (FEEDBACK56.md, Part F).
- `confirm(question)` and `about(question, choices, default)` let a tool ask the person at the AI
  client before it acts: the call stops, the client asks, and the call runs again from the top
  with the answer, in a state Quillo signs (10 minutes, once, same caller, tool and arguments).
  `quillo check` refuses a change before a question (a change on a way that returns or replies
  first is none); as it runs, a question after any change is refused and the call stops (the
  change stands), and so is a question the checker does not see the tool ask, as in a
  `locale_from` rule, in the tool or a service it calls. Outside a tool's call, `about` gives its
  default and `confirm` true. `ask(message, options)` stays the page's dialog; `ask` with three
  arguments is an error naming `about` (FEEDBACK56.md, Part E; `about` is the user's choice).
- A client may keep `server/discover`'s and `tools/list`'s answers for 5 minutes (`ttlMs`), the
  same for every caller, so a tool a deploy renames reaches it soon. `quillo check` warns of a
  `reply` of a 2xx status in a tool's code, which the AI client gets as an error, as any `reply`
  there: a tool answers by returning its value (the pre-release defaults).
- `server/discover` carries `instructions`: a few lines Quillo writes (under 600 characters),
  then the main file's top `//!` block, in which a character a reader does not see (a
  bidirectional override, a zero-width space) is an error; `quillo check` warns past 2,000
  characters, and `quillo docs --mcp app.qwe` (`--policy`, `--service-name`) prints exactly
  what each door sends (FEEDBACK56.md, Part D).
- A `.qwer` file's `//!` header after a `/*! ... */` banner, or after `/* a */ /* b` on one line,
  is read as Rust reads it: the banner no longer ends it, and an open comment hides what is in
  it (FEEDBACK56.md, found in review).
- `/_quillo/mcp` serves a program's tools over HTTP from `quillo run`, to a signed-in session or
  a token (`mcp_token()`, `mcp_tokens()`, `revoke_mcp_token(id)`; `end_session()` ends them),
  never to a page on another site; 60 tool calls a minute per user (`--max-tool-calls`,
  `[limits] max_tool_calls`); `request.mcp` says the client (FEEDBACK56.md, Q-859, Q-860).
- `quillo mcp app.qwe` serves a program's tools to an AI client over stdin and stdout, in MCP's
  revision 2026-07-28: `server/discover`, `tools/list` and `tools/call`, with only MCP's
  messages on stdout (`print` goes to stderr there). A client of the earlier revision is told
  the version is not supported (FEEDBACK56.md, C.1).
- A tool's arguments and result are written as JSON Schema 2020-12, with `readOnlyHint` and
  `openWorldHint` said only when Quillo can see they are true. "Only reads" is worked out
  from the program and enforced: such a tool that may reach the database runs in a read-only transaction on SQLite,
  PostgreSQL and MySQL, and `db.error_kind` gives `"read_only"` for a write it refused. A
  result that is not an object is held as `result`, as MCP's earlier clients require
  (FEEDBACK56.md, Q-858).
- A function's `///` comment is kept by the program: `quillo docs` and `/_quillo/describe` write it
  as the `server fn`'s description, and MCP tools will use it (FEEDBACK56.md, Q-854).
- The API document writes each named type once under `components/schemas` and refers to it
  with `$ref`, where it gave a bare name a validator takes as anything; records and map types
  say which fields are `required` (FEEDBACK56.md, Q-855, Q-856).
- A `server fn`'s entry says what it may do: its `with` ends its `contract`, and
  `x-quillo-effects` lists the same (FEEDBACK56.md, Q-857).
- `list[str]` in a map's or a record's field gives the fix, `list<str>`, as a parameter's does
  (FEEDBACK56.md, Q-861).
- `quillo check` no longer calls a class unused when `class={{ active: on }}`, a map in a list or
  choice inside `class={...}`, or `class:active={on}` writes it, and `classes()` and
  `quillo classes` now list a map's keys, so a CSS engine makes rules for them. A map held in a
  name is still not read (FEEDBACK61.md, Q-898).
- A server fn answered with a 2xx that its server did not send, or that was cut short, gives an
  error in the browser: a sign-in page a redirect led to, a proxy's page, an empty body. It gave
  `null`, as if the call had worked. Server fn answers now carry `x-quillo-answer`, so `respond` and
  `reply` go on giving `null`. A `redirect` from the function itself, which the browser follows,
  gives the same error (FEEDBACK49.md, Q-598).
- A frame, picture or player with `src={...}` in a component is asked for once when the page
  loads, not twice: taking up the server's page no longer writes the same text again. A frame or
  player with a listener other than a click is still started over, since the load it waits for
  may already have gone by (FEEDBACK61.md, Q-893).
- A component of a file with a top-level `style` gets that style when it is first shown in the
  browser, from a file nothing on the server's page came from. It had no CSS at all from the
  file (FEEDBACK37.md, Q-457).

## 0.8.0 (2026-11-07)

**Upgrade from 0.7.0: see Security below.** One visitor's data could reach another in 0.7.0,
in twelve ways, one of them a worker reading a finished request after it was freed. A key of
`idempotent` now runs at most once, whoever asks. Not in it, moved on: FEEDBACK43's database driver interface (Q-494 to Q-497), the
apps planned for 0.8 (the live dashboard and the others), and the studies FEEDBACK52, 54 and
56 to 59.

The fixes of FEEDBACK55.md's sweep, all of it: what a program writes into markup, the
files `files()` serves, HTTP's answers and cookies, highlighting and Markdown, the three
databases, components' fields, the server and the browser giving one answer, values that
are exact or an error, what runs at the same time, a program's files and Rust modules,
plurals, dates, numbers and images, and the command line: Q-700 to Q-852, and Q-875, found
while fixing (a map's order in the browser).

### Security

- **One viewer's `live` answer could reach another, for up to two seconds, in 0.7.0 and
  before: upgrade.** A `live` run is shared between viewers unless its code read the request.
  `session()` gave its kept copy without saying so, so the second loader of a page, or a
  loader after a route that read the session, went to the next viewer: their page showed the
  first viewer's data. `t()` and `locale()` did the same with the language. Both mark the run
  as the viewer's now; a loader reading nothing of the viewer's stays shared (Q-876).
- **`quillo check` warns of a `redirect` or `respond` nothing takes.** In
  `if no_user { redirect("/login") }` with more after it, the redirect is made and left, and
  the route goes on: the visitor is shown the page. Write `reply 303 redirect("/login")`.
  Look for this in your code: two of the sample apps had it (Q-877).
- **Eleven more ways one visitor's data reached another are closed** (an audit after
  Q-876; in 0.7.0 and most in 0.6.0):
  - a view's cache kept a service's answer until restart, so one visitor saw another's
    `Authorization`, and one tenant another's data (Q-878);
  - a tenant told by a function, or named with `with_tenant`, was not in a run's key, so a
    tenant saw another's rows (Q-879);
  - `request` in a parameter, pattern or lambda default went unseen, and the run was
    shared (Q-880);
  - visitors were told apart by five fields, so `X-Api-Key`, `?token=` or `X-Tenant`
    reached the next visitor from the same address (Q-881);
  - mounted programs shared runs (Q-882);
  - **a worker kept a finished request**, memory-unsafely: a job or a channel handler read
    the last visitor's query, and a stream could go to the wrong visitor (Q-883);
  - kept values answered to a hash anyone could work out, so a visitor could guess another's
    short secret offline (Q-884);
  - a kept run lost its marks, so a page lost `Cache-Control: private` (Q-885);
  - a page showing the session with a status of 400 or more, one read through
    `get(request, ...)` or a pattern, and a translated page had no `private` or `Vary`
    (Q-886);
  - an idempotency key was shared by every session without `.user` (Q-887);
  - a run with `crypto.token`, `random` or a write was shared, so two visitors got one
    token and a second write was skipped (Q-888).

### What to change in your code

- **A run that read the visitor is shared with nobody**, not even their other tabs, and one
  that did anything but read is not shared either. A loader for everyone is one run, as
  before, now per tenant too. Loaders calling a service, a site or a plugin stop sharing.
  Builtins are classified: one not named is neither kept nor shared (Q-881, Q-888).
- **An idempotency key runs at most once, whoever asks**, the user's decision: a double
  charge is the worst outcome. With nothing in common the answer is 409, nothing run or
  shown, also after signing out or losing a cookie. **Two visitors with one key used to each
  run; the second now gets 409**: pass `{ user }` (a str or an int; a record is refused) or
  use unique keys (Q-887).
- **Pages reading `request.headers`, `request.cookies` or `request.ip`, or passing `request`
  on, are private**, whatever their status, and so are those under middleware that reads the
  visitor or cannot be read (Q-886).

Databases (each driver hides its database's quirks, so a value means the same on all three;
the user's decision, Q-736):

- **NaN and the infinities are refused as values** on every database, naming them (`value 1
  is NaN, which cannot be stored`): SQLite stored NaN as null, PostgreSQL kept them. **`-0.0`
  is stored and read as `0.0`** everywhere (Q-735).
- **`last_id` is the one inserted row's id, else `null`**: after an insert of several rows
  (SQLite gave the last, MySQL the first), an upsert that updated, an `update` (SQLite gave an
  old one) or a read. It is `null` on PostgreSQL as before: `returning id` there (Q-753).
- **`changes` is the rows matched on MySQL too**, as on SQLite and PostgreSQL: an `update`
  that sets the value a row has counts it, and an upsert's or a `replace`'s row counts once
  (`?client_found_rows=false` gives MySQL's own count). `changes` is 0 for a read and a
  schema statement, and a script's last statement's (Q-739, Q-753).
- **SQL with no statement (`""`, spaces, a comment) is an error value**, from `db.exec`,
  `db.query` and `db.one` (Q-753).
- **An error value given as a parameter is given back**, not stored as its JSON text, and a
  function is refused (Q-727). **Placeholders given no values are refused** on SQLite, which
  stored nulls (Q-729). **A map given to SQL with no named placeholder is no value**, and a
  map for a lone `?` is that value, on all three (Q-747, Q-748).
- **On MySQL, `@name` with a map holding `name` is refused**: it is the session's variable
  (null); write `:name`. SQLite's `@name`, `$name` and `#name` take the map's plain `name`
  (Q-752).
- **MySQL and MariaDB: a statement that commits by itself (`truncate`, `create table`,
  `alter`, ...) is refused inside `db.transaction` and `db.exec_many`**, and one that could
  not be seen beforehand ends the transaction with an error naming it, where everything
  before it was committed (Q-728, the user's decision: MySQL only). Migrations take them as
  before.
- **MySQL and MariaDB: `db.open` of a database that is not there is an error**, as on
  PostgreSQL; it made an empty one. A tenant's is still made, by the driver (Q-736).
- **MySQL and MariaDB values:** `BIT(n)` reads as its digits (`"00000101"`) and takes them
  back (Q-741); ids and unsigned values past 2^63 are decimals, not negative or floats
  (Q-740, Q-756); `tinyint(1)` is a bool only for 0 and 1 (Q-756); `FLOAT` reads as the
  shortest float that prints the same, and a value it would keep as 0 is refused (Q-744,
  Q-734); a statement past `max_allowed_packet` is refused saying so (Q-751); `||` joins
  text on MariaDB, in migrations and scripts too, where it was `or`.
- **PostgreSQL values:** whole numbers in JSON past an int are decimals, and a JSON value that
  cannot be read is an error in its own cell, not a failed query (Q-732); a str written as a
  `json`/`jsonb` column's value is the JSON it holds, so `json.stringify(m)` stores an object
  (Q-733); a float past `real`'s or `double precision`'s range is refused (Q-734); `real`
  reads as the shortest float (`0.1`) (Q-744); BC dates read as ISO 8601 (`-0043-03-15`) and
  go back in (Q-743); a type with no reader is hex in every row, and `timetz`, `pg_lsn`,
  `xid`, `xid8` read as PostgreSQL writes them (Q-742); `bit(n)` keeps its length (Q-730); an
  array not starting at 1 comes back as its text, keeping its bounds (Q-756); a value with no
  type in the SQL is declared `int4`, `int8` or `float8`, so `select ? as v` with 5 is `5`,
  not `"5"`, and `greatest(10, 9)` is 10 (Q-731).
- **PostgreSQL: `set_config(..., false)` is refused outside a transaction in every form**
  (with values, inside `values (...)`, in an `update`, any case or quoting), and so are
  `pg_temp` tables; `do` blocks and calls of the database's own functions run on a connection
  of their own, reset after; every transaction's connection is reset as it ends (Q-708).
- **`db.transaction` refuses an option it does not have**, at run time and in `quillo check`
  (`` has no option `isolaton` (did you mean `isolation`?) ``); a misspelt key ran with none
  (Q-738).
- **The guide's column-list update, `` set (${sql_name(cols)}) = (${sql_param(cols)}) ``,
  fails on MySQL and on PostgreSQL with one column**: write the pairs, `a = :a, b = :b`,
  built from the columns (the guide shows how) (Q-754).

Files and HTTP:

- **`files()` and `quillo serve` serve no SQLite database, whatever its name** (by its
  header, `-wal`, `-shm` or `-journal`): the program's, a tenant's, another's. `file("x.db")`
  serves one on purpose, with ranges (Q-702, the user's decision).
- **A link inside `files()`' folder is followed only through build-output and package-store
  dot folders** (`.pnpm`, `.vitepress/dist`, `.output/public`, `.next/static`,
  `.svelte-kit/output/client`, `.nuxt/dist/client`, `.vercel/output/static`, `.well-known`,
  and `node_modules`' `.pnpm`, `.deno`, `.bun`, `.store`); a file is judged by where it leads,
  and the program's own files (`.env`, `--env-file`, `--tls-key`, loaded sources, open
  databases) by device and inode too, so a hard link to one is a miss (Q-701).
- **`write_file`, `save_upload`, `delete_file` and `image.convert` refuse the program's own
  files in any case and Unicode form, and as Windows reads names**, on every system (`.ENV`
  was written, then read as the settings) (Q-704); **`delete_file` refuses a folder that holds
  an open database** (Q-705); **`check_program` reads only `.qwe` files** (and `.qwer`/`.rs`
  signatures), no dot paths, and its messages show no file's text (Q-703).
- **A header's `null` in `respond`, `stream` and `events` is no such header**, Quillo's own
  default included (`{ "cache-control": null }` drops a session page's `private`, `{ server:
  null }` the `server` header); it was sent as the text `null`. A list is one header per item
  for `set-cookie` only, and an error for another header, as a map is (Q-710).
- **`stream` and `events` refuse 204, 205, 304 and 1xx**, which corrupted the next answer on
  the connection (Q-709).
- **`redirect` takes 301, 302, 303, 307 and 308** (it took 300 to 399, 304 too), and writes
  `Location` percent-encoded, a non-ASCII host as IDNA, a `\` as browsers read it, a site
  path's `\` among its leading slashes as `%5C` and its dot segments taken out before
  `--base`; `javascript:`, `vbscript:` and `data:` locations are refused in `redirect`,
  `respond`, `stream`, `events` and `site_headers` (Q-719).
- **`set_cookie` refuses an option it does not take** (it ignored them), naming the
  snake_case one for `sameSite`, `maxAge`, `Max-Age`, `httpOnly`, also in `quillo check`;
  `delete_cookie` takes `path`, `domain`, `http_only`, `secure`, `same_site`, and `max_age` 0
  only (Q-712). **`__Host-` and `__Secure-` cookies keep their prefix's rules**, `Secure`
  under `--watch` too, and asking otherwise is an error (Q-713). **A cookie replaces another
  only at the same name, path and domain** (Q-714). **A cookie over 3,900 bytes as sent is
  refused** (Q-726).
- **The `Cookie` header keeps the first cookie of a name** (the browser's longer path),
  trimmed, one pair of quotes taken off; it kept the last (Q-718).
- **A multipart body of more than 1,000 parts is a 413**; those past the 1,000th were dropped
  with a 200 (Q-716).
- **A non-GET request whose `If-None-Match` matches a file is a 412**, not a 304, and a range
  unit is read in any case, its numbers digits only (Q-723).
- **`quillo serve` moves a file asked for with a `/` after it to its path** (301,
  `max-age=3600`); `files()` in a route serves what it is asked, as before (Q-721, the
  user's decision).
- **`/_quillo/markdown` takes at most 1 MB** (413), and `highlight`, `markdown` and
  `markdown_page` end at `--max-compute` as a loop does (Q-706).

Markup and components:

- **A spread never writes `data-qi`, `data-qd`, `data-qt`, `data-c`, `data-p`, `data-s`,
  `data-l`, `data-v`, `data-e`, `data-q-*` or `data-quillo-*`**, in any case: they are left
  out. Written out, `data-qi`, `data-qd`, `data-qt` and `<quillo-island>` are errors
  (Q-700).
- **In production the browser hides a builtin's error as the server does** (text,
  templates, markup, attributes, `json.stringify`: `internal error`), and so do attribute JSON
  on the server and channel messages (Q-773).
- **`<select value={x}>` is written as `selected` on the option `x` names**, not as a
  `value` attribute on the select; `<select multiple value={list}>` marks each (Q-758, Q-761).
- **A text input or select bound to a number state gives a number** when the text is that
  number's own form (`"02"` stays text), `null` when empty; it gave text (Q-760). Blank text
  in a number field is `null` (Q-757).
- **An assignment alone in a component's body runs once**, as before, and `quillo check` now
  warns when it reads a prop or another state: write `let` (Q-764).
- **`highlight` of a language it does not know is only escaped**, with no colours (Q-834).

The server and the browser give one answer (the user's rule: money, and every number,
accurate; Q-775, Q-776):

- **In the browser a whole float is a float**: `2.0 / 0` is `inf` (it stopped with "division
  by zero"), `type_of(2.0)` is `float`, `match` takes the `float` arm, an `int` parameter,
  an index, `slice`, `range`, `repeat`, `take` and `round`'s digits refuse `2.0`, and a
  `float` parameter refuses `2`; a record's `float` field widens an int. Code that relied on
  the browser taking `2.0` for `2` now stops there as it does on the server (Q-775).
- **Ints past 2^53 are exact in the browser**, and past 64 bits are the server's `integer
  overflow` where they were a wrong number: `3037000500 * 3037000500`,
  `math.factorial(21)`, `math.comb(67, 33)` (Q-776). **`floor`, `ceil` and `round` of an int
  give the int**, on the server too (it went through a float past 2^53).
- **`money.of` refuses a whole float in the browser too**, as on the server: an amount
  never goes through a float (Q-775).
- **Text orders by code point in the browser**, `trim` takes Unicode's White_Space (U+0085,
  not U+FEFF), and `replace(s, "", x)` puts `x` before each character and at the end, as on
  the server (Q-778). **NaN cannot be ordered there either** (`sort`, `min`, `max`, `<`,
  `stats.median`); `vec.argsort` puts it last (Q-780).
- **`json.stringify` writes a whole float `2.0` and turns to an exponent where JavaScript
  does** (`1e+21`, `1e-7`; the server did from 1e16 and below 1e-5), on both sides, and the
  server's float text takes JavaScript's digit on a tie (`1916218016406818.2`) (Q-777,
  Q-781). **`json.parse` refuses `1e400` and lone surrogates** in the browser too, and
  **`float("0x10")`**, `"0b11"` and `"0o17"` are refused there (Q-782).
- **A CR in text is written `&#13;` and a NUL U+FFFD**, on both sides, `escape()` too
  (Q-772).
- **Time zones are the IANA database's names on both sides**: `posixrules`, `Factory`,
  `Etc/Unknown`, `localtime`, `posix/...`, `right/...`, `US/Pacific-New`,
  `Canada/East-Saskatchewan`, `SystemV/...` and Java's three-letter ids (`PST`) are unknown
  zones on both (Q-782); **`+0530` and `+05` are zones on both**, and any other name with a
  sign (`+5`, a U+2212 minus) on neither (Q-829).

A program's files and Rust (Q-807 to Q-822):

- **Every Rust plugin library is built again once** after this update: the glue is version 2,
  and libraries are named by the whole file name and the glue version (`x.rs` and `x.qwer`
  deleted each other's). A deploy with `--no-build` is told `` built by an older Quillo ...
  run `quillo build` once ``: run `quillo build` with the new binary as part of the deploy,
  and bring `.quillo/` along. An older Quillo's libraries and lock lines stay, so going back
  still runs (going back and forward again builds once more) (Q-812, Q-809).
- **A private name's tag counts from the main file's folder**, not the folder `quillo` was
  started in, and `--split` services take the app's (Q-807). The tags, and so the names of
  private `schedule` and `every` jobs, change once for a program started from another folder
  (`quillo run current/main.qwe`), for files outside the program's folder, and for split
  services; while an old and a new copy share a database during a rolling deploy, a renamed
  job may run under both names for one slot: stop the old copy first, or accept it.
- **A path written with its extension is that file only**: `import lib.qwer use hi` with only
  `lib.qwe`, or `use helpers.rs` with only `helpers.qwe`, is a missing file, said once at the
  import line with the near file; **a path without its extension that two files match is an
  error** (`` `helpers` could be helpers.qwe or helpers.rs: write the one you mean with its
  extension ``) (Q-819).
- **A main file reached through a link takes its `use`, `import`, `mount` and `service`
  files from beside the real file**; its policy, `.env`, data and messages stay at the link's
  folder (Q-817). A service's file follows the same rules, and a Rust file it uses is built by
  the service, not read as Quillo by the app.
- **One browser name from two scripts is an error**, in check and as the program starts
  (the page had only the last): take one `as` another name (Q-808).
- **NaN and the infinities reach a Rust `f64` as they are** (they were `null`), and a
  `serde_json::Value` argument holding one is refused; **an error value given to a Rust
  function is given back and the function does not run** (it arrived as a map) (Q-809,
  Q-810).
- **Rust signatures with a reference inside another type (`Option<&str>`, `Vec<&str>`,
  `&[&str]`) or a lifetime are not exported**, with the owned type to take, where the glue
  failed to build the whole file (Q-813). **Rust names are the file's that takes them**:
  two Rust files may export one name, and a file taking both is told to choose (Q-816).
- **`use Point as Pt` makes `Pt` a type, and `as` cannot take a type's name** (Q-821).
- **`quillo check` says code after a top-level `return` never runs**, and does not define
  what follows it, as the program does (Q-818).

Plurals, dates, numbers and images (Q-823 to Q-833):

- **A count's plural form is CLDR 47's for the number as it is shown**, by its operands, on
  both sides: `1.5` and `0.5` are `one` in French (they were `other`), `"1.0"` is `one` in
  Spanish, `2.0` `two` in Arabic, Slovenian's fractions `few`, Filipino and Tagalog count by
  their last digit. A whole float count takes the form of what `{count}` shows: `1.0` in a
  route ("1.0 items" in English), `1` in a component ("1 item"), on both sides; a written
  `zero` form is used only for a 0 shown with no point (Q-823, Q-832).
- **`date.format`'s styles for `en-CA`, `en-IE`, `en-AU`, `en-IN`, `en-ZA`, `de-AT`, `fr-CA`,
  `fr-CH`, `es-MX`, `it-CH`, `nl-BE` and `pt-PT` are CLDR's**, where they were the language's
  or Britain's: `en-CA`'s short date is `2026-09-05`, `de-AT`'s January `Jänner` (Q-828).
- **`format_number` with no digits keeps every digit** in the browser too, a large whole
  float is written by its decimal digits there, and a negative number rounded to zero keeps
  its minus (`-0.00`, `-0`), as `Intl` writes it (Q-827).
- **`image.convert` and `image.info` follow EXIF orientation**: `width`, `height` and a
  `crop` are the turned image's (Q-825). **Transparent pixels are white in a JPEG**, where
  they were black (Q-826).
- **A service `timeout` or `cooldown` past a century is a century, and one no duration
  holds is refused** as the program loads, where it panicked (Q-830).
- **`markdown_page`'s title is the first level-one heading**, setext too, as its text; a `#`
  line in a code block is none (Q-831).

A value is exact or an error, the same on the server, in compiled code and in the browser
(Q-783 to Q-795):

- **`sum` of text stops** at the item, saying what to write: `` `sum` adds numbers; got the
  str "12.50" at item 1: use `float(...)` or `dec(...)` first ``. It joined the text after a
  `0` (`"012.505.00"`), and amounts read from a form or a CSV are text (Q-783).
- **A builtin given more arguments than it takes stops**, saying what it takes: `` `split`
  takes 2 arguments, got 3: split(s, sep) ``, in `quillo check` where the call is written
  out, as the program runs however it is reached (a method on an untyped value, a builtin
  in a variable or given to another, `reduce(xs, 0, max)`), and in the browser. The extra
  ones were dropped: `xs.sort((a, b) => b - a)` sorted up (it now says to use `sort_by`),
  `line.split(",", 2)` split at every comma, and `uuid(1)`, `random(1)` and `merge(a, b, c)`
  ran as if they were not there (Q-786).
- **A name bound twice in one pattern is an error**, in `quillo check` and as the program
  runs: `[x, x]`, `{ p: a, q: a }`, `let [y, y] = ...` matched anything, keeping the last
  part; test two parts with a guard, `[x, x2] if x == x2`. A tested field binds its last
  name, so `{ user.id: int, order.id: int }` still tests two ids, but using `id` there is an
  error naming both fields (name one with `as`), and so is a route binding one name by two
  of its path, query and request patterns (Q-785).
- **An int and a float compare exactly**, as in Python, in the interpreter, both JITs and
  the browser: `9007199254740993 == 9007199254740992.0` is false, and `sort` puts
  `9007199254740992.0` before `9007199254740993`; they went through a float (Q-787).
- **`floor(a / b)` of two ints is the exact int** past 2^53, and so are `ceil`, `round`,
  `math.floor`, `math.ceil` and `math.trunc` of it, however the builtin is reached (called,
  piped, as a method, from a module or a variable): `floor(9007199254740995 / 2)` is
  `4503599627370497` (it was ...498, a float's digits). A quotient kept in a variable first
  (`let q = a / b`) is a float, as before. **A float `floor`, `ceil` or `round` makes whole is
  an int up to 2^63**: `type_of(round(1e16))` is `int`, where it was `float` from 9e15
  (Q-788).
- **`round` takes negative digits for floats and ints, and an int rounded to places is
  itself**: `round(1234.5678, -2)` is `1200.0` (it was `1235.0`, the digits taken as 0),
  `round(1255, -1)` is `1260` (it was `1255.0`), `round(2, 2)` is `2` (it was `2.0`), and
  `round(9007199254740993, 2)` keeps every digit. Ints to negative digits round a half away
  from zero, where Python rounds it to even (`round(1250, -2)` is `1300`). A float to negative
  digits works on its shortest digits, exact at every place (`round(1e30, -29)` is `1e30`;
  past 28 digits it went through floats and gave `999999999999999900000000000000`), and a
  result past the largest float is an error, as in Python (it gave the float back) (Q-789).
- **`pow` with a power of 2^32 or more is exact for a base of -1, 0 or 1, and an `integer
  overflow` for any other**: `pow(-1, 9007199254740993)` was `1.0`, `pow(2, 4294967296)`
  `inf` (Q-790).
- **`unique` keeps exactly what `==` keeps**, so `unique([9007199254740993,
  9007199254740992.0])` has both (Q-791).
- **`json.stringify` of a function is an error value naming where it is** (`` `a.f` is a
  function, which cannot be written as JSON ``), where it wrote `"<fn <fn>>"`; **a route
  answering a value that holds a function is a 500**, its log line naming the path, where
  it was a 200 with `"<fn f>"` in it, and `quillo check` warns of a function written in a
  route's answer (Q-792).
- **An error is given back, not made into text**: by `get`, `has`, `set` and `remove` given
  one as the key, by `group_by` and `re.replace` given one by their function, and by
  `url_encode` given one or a value holding one at any depth (the first in order); its text
  became a key, part of the text or of the URL (Q-784).
- **`sql_name` and `sql_param` take letters and digits of any script and the marks on
  them, at most 63 bytes on every database**: a name of 64 bytes is refused, as PostgreSQL
  keeps 63, and `sql_name("名前")` is `"名前"`. **`quillo check` follows `map` and `join`
  over them**, through every return of the lambda, so the guide's update from pairs
  (`` cols | map((c) => `${sql_name(c)} = ${sql_param(c)}`) | join(", ") ``) gives no
  warning, while raw text in the lambda still does; only the builtins are trusted, not a
  name the program or the lambda binds again.

What runs at the same time comes out as if done one after another (Q-796 to Q-806):

- **One failing `parallel` function stops the others** at their next step (a call, a
  loop's round, a builtin, a `sleep`), nested `parallel` too; what they did before stays,
  and the call fails with the failure that happened. They ran to their ends, their writes
  kept, and the first error was raised after (Q-804). Error values stop nothing.
- **`set_cookie`, `set_session`, `end_session`, `session()`, `publish` and read-your-writes
  work inside `parallel`'s functions**, merged into the answer in the functions' order;
  reading the visitor there makes the answer private. They were dropped: a logout in a
  `parallel` function left the visitor signed in (Q-796).
- **A global set inside a transaction is announced when the outermost transaction ends,
  whatever its outcome**, since the global keeps its value: pages kept the old one after a
  rollback. A table's announcement still goes with the rollback (Q-797).
- **A cron job's first time after a start runs**: counted from when the server first looks,
  so started at 04:01, `"2 * * * *"` runs at 04:02 (it first ran at 05:02), and a nightly
  job of a server deployed each day before its hour runs (Q-798). Cron fields may be apart
  by tabs.
- **`every` and `schedule` called once the program has started stop with an error** (a 500
  in a route): they answered and the job never ran. `quillo check` reads their written-out
  arguments as the start does, and **refuses one name twice only where both calls surely
  run** (twice at the top level, or a top-level function adding it called twice); where
  both may run (two `if`s, `&&`, `||`, `??`, a loop) it warns, and the start refuses the
  second add that happens (Q-803).
- **A state set after a wait gives way to a newer set of it** (the user's decision, "the
  latest call only"): every set of a state claims it (plain, `bind:`, from a call), and a
  set carrying an answer the function waited for (in its value, or decided by it: under an
  `if`, `match`, `while` or `for` on the answer, or after a guard that leaves on it) gives
  way when another run (a handler, effect, timer or frame, with the functions it calls) has
  set or claimed that state since, so a search box ends on the answer for what was typed
  last. Plain sets after a wait are made and still claim, and a function's own helpers are
  its run, so their waits take nothing from it; a plain set from a run that lost the state
  is made and takes nothing, so the newest answer still lands. A set that reads the state it
  sets is a fold (`n += 1`, `xs = xs + [x]`, `s = f(s)`), always made. In one with a wait, every
  part runs where it is written and only the read of the state moves after the waits, so
  `n += count()` three times in one tick counts three (Q-800).
- **`every` in the browser waits for its last run**: a tick that finds it still waiting
  does nothing, so a poll slower than its timer shows each answer, where every answer was
  dropped for a newer call and calls piled up on a slow server. A run with no answer after
  30 seconds (or two gaps, if longer) is given up on (Q-800).
- **A hole of markup in a function that waits may wait** (`let el = <b>{load()}</b>`): it
  emitted `await` in a function that could not hold it, so the page's script did not parse,
  `quillo check` said ok and nothing reported it. It is worked out once, before the markup
  is made (Q-800's review).
- **A topic is 1 to 256 bytes**, any characters, a comma too; an empty or longer one stops
  `watch` and `publish` with an error, where its page never heard of it. A trailing `:*` is
  a wildcard, as before, now in the guide: a topic built from a visitor's text must not end
  in `:*` (Q-801).

Maps in the browser, and JSON (Q-875):

- **A map keeps the order its keys were set in the browser too**, keys like `"1"` included
  (JavaScript lists those first): `keys`, `values`, `for`, `str`, `json.stringify`,
  `url_encode`, `merge`, spreads, `+`, `group_by` and records read `{ b: 1, "1": 2 }` with
  `b` first, as the server does, and JSON the browser reads (`json.parse`, `http`'s `json`,
  `server fn` answers, island state and props, live data, channels, streams) keeps the order
  written. Code that counted on such keys coming first in a component sorts them.
- **`js(...)`: a map holding a key that is an array index out of JavaScript's order reaches
  page code as a plain copy**, in JavaScript's order, and what page code changes in it does
  not show in Quillo; a map without such keys is handed over as before. What a program's
  function called by page code returns is converted the same way.
- **JSON nests 128 levels on both sides**, for every reader: `json.parse` past it is
  `invalid JSON: it nests more than 128 deep` (the browser took some 3,000 and then
  stopped), `request.json` and an `http` answer's `json` are `null`, a stream's line is its
  text, and a database's JSON cell is that error (it took 1,000); sessions, channels, a
  `server fn`'s arguments and a service's body read the same depth, and `set_session`
  refuses a value nested deeper.
- **A top-level constant holding an error value is an error value in the browser** (`is_err`
  true, a builtin's text hidden), where it was a map.

Services and time zones:

- **A service `--split` started stops when its app ends, whatever way** (a SIGKILL, a crash,
  the app's own exit after a second Ctrl-C), as a first SIGTERM stops it: the app holds a
  pipe on the service's input, which closes then. A killed app's services served on with
  their ports taken. A service started on its own (by hand, by systemd) serves on.
- **A time zone the system's database leaves out is taken from Quillo's own copy**: Ubuntu
  24.04 moved `EST`, `US/Pacific`, `Asia/Calcutta` and `EST5EDT` to `tzdata-legacy`, and a
  program using one stopped there while it ran on a Mac and in the browser.

The command line:

- **A `.env` that cannot be read, is not UTF-8 or is a link to nothing stops the start**
  (exit 2), saying where and the ways out; it was skipped without a word. A service's own
  likewise (Q-835). **`KEY= # note` is `""`** (Q-838); **an unclosed quote skips its line
  only** (Q-839); **a NUL in a value skips the line** (Q-840).
- **A policy's limits of 0 are refused**, as the options refuse them: `max_body = 0` answered
  every POST 413 (Q-849).
- **`--tls-cert` and `--tls-key`: the last given wins, and they must be a pair** (Q-852).
- **An option before the command is an error that says where the command goes**:
  `quillo --watch main.qwe` is told `` put the command first: `quillo run --watch main.qwe` ``
  (Q-851).
- **The REPL fed from a pipe skips a line that is not UTF-8 and exits 1**; it stopped there
  with 0 (Q-848).
- **A request the policy's `max_compute` ends is the policy's wherever it is ended**, a loop
  or a call too, and **no `??` takes that stop** (`highlight(x) ?? y` under a policy took it)
  (Q-849).

### Fixed

- `--watch` and `--reload` load a file saved right after the server starts. The watcher read
  the files' times when its own thread first ran, so a save made before then was taken for
  the program as it was and never loaded; it now reads them before the server says it is
  listening (found by CI in `no_check_reloads_what_loads`).
- Found while covering FEEDBACK55's new lines: a Rust plugin's map of one key whose value is
  NaN or an infinity came back as `{ k: { "\u0000f64": "NaN" } }` (Q-809); on PostgreSQL,
  `select upper(?) as u, ? as v` gave `v` as text, as every value went as text when one
  function refused its type: only the values a refusal is about do now, as SQLite and
  MariaDB give (Q-731); `check` said nothing of a function the route's block makes written
  in its answer, which answered 500 (Q-792); `math.round(7 / 2)` gave 4 on the server and an
  error in the browser: `math` has no `round`, and both refuse it (Q-788).

From FEEDBACK55.md:

- **Markup (Q-700, Q-707, Q-711, Q-773):** visitor JSON spread on an element cannot make it a
  component, and a component whose root is a table's part keeps only the server's island
  attributes, which an update cannot set; a `css` template's `&` is `\26 `, so `<svg><style>`
  and `<math><style>` gain no rule; `highlight` in the browser takes `"constructor"` and
  `"__proto__"` as unknown languages; the debugger's bundle hides errors as the server does.
- **`files()` (Q-701, Q-702):** the file opened is the file judged (regular files only, its
  real path from the system, a file swapped for a link a miss); `--tls-key` and every `.env`
  stay refused after a renewal through a link or a save; on Windows `~1` short names are
  refused. Small files are judged and kept off the interpreter lock, so a 304 costs what it
  did before (342k in 4 s on a 42 KB css); a large one's judgement is kept by its identity, so
  its 304 is a stat (58.6 to 11.1 µs). A file changed in the last 50 ms is not kept (git's
  racily-clean rule), one dated ahead of the clock is; a file rewritten in place is served as
  read, and only one replaced at every try answers 503 with `Retry-After`, never a 404. Files
  over 64 KiB are read a few at a time on the async runtime, holding no thread or lock:
  128 cold 1 MiB files peak at 110 MB, not 465.
- **HTTP (Q-715, Q-717, Q-720, Q-722 to Q-725):** `files()`' 404 page is `no-cache`, and sent
  whole whatever the request's ranges and conditionals; a form's content type is read in any
  case; filenames with `\"` (Go, curl) and RFC 5987's `filename*` are read; a range's end
  past the file is clamped, and the file's own weak tag passes `If-Range` and `If-Match`, as
  nginx has it, so resumes and seeks get 206; the plain copy of a compressible answer carries
  `Vary: accept-encoding`; a HEAD, or a range of 1 MiB or less, takes no place among the
  large sends, and a small HEAD answers as GET would, gzip included; `file()`'s type errors
  say what is wrong; a page with `{ etag: null }` keeps its component bundle and scripts; an
  app's cookie jar keeps cookies by name, path and domain, one with no `Path` at the
  request's folder, and an app goes only to `http(s)` or its own pages.
- **Highlighting and Markdown (Q-706, Q-834):** the highlighter reads each line once on both
  sides (64,000 `/*` took 5.88 s, now about 2 ms); Markdown is pulldown-cmark 0.13 (0.12 was
  quadratic on mixed emphasis: a megabyte of `*a_` took 17 s, now 0.03 s). Python's
  triple-quoted and Rust's strings run over lines, Rust's raw strings have no escapes, Rust
  characters such as `'"'` are strings and lifetimes are not, a JavaScript regex is one token,
  a shell's `it's` and a `#` inside a word start nothing, a range's `..` ends its number; a
  fence's language is its first word to a space or comma (`rust,ignore`); words are Unicode
  letters and digits on both sides; `str(highlight(...))` in the browser is the markup.
- **Databases (Q-708, Q-727 to Q-756):** one SQL tokenizer per dialect (`src/sql.rs`) is read
  by every part that reads SQL: function, procedure and trigger bodies, `do $$` and `begin
  atomic` are one statement, not a transaction begun by hand (Q-746); PostgreSQL's `?||`,
  `@?`, `?1`, `[:name]` and nested comments, and MySQL's `/*! */` and MariaDB's `/*M! */`, are
  read as the servers read them (Q-749); `?schema=` keeps `?options=` (Q-737); a kept
  statement whose table changed is prepared again, once, on every connection (Q-745); a
  replica gets only pure reads, and its read-only refusal goes to the primary without taking
  it out of use (Q-750); MySQL hosts may be IPv6 addresses (Q-756); a `jsonb[]` takes lists
  (Q-756); `quillo check` gives no list warning for PostgreSQL arrays (Q-755); SQLite's own
  `last_insert_rowid()` is untouched, and `db.exec` of a statement returning rows runs.
- **Components (Q-757 to Q-771):** a number field keeps `1.0`, `2.50` and `1e3` as typed; a
  select shows the option its value names once its options are in, and keeps a person's
  pick unless options are added or taken away; a range's value is set again when its limits
  change; rows chosen by `? :`, `if` or `match` with a key on each branch are keyed; a
  component that has left keeps the props it last read; inside `<svg>` a component's
  one-element root is its own island, on both sides; focus and selection come back when a
  keyed row moves without `moveBefore` (Safari); `e.form` names the button that sent it; a
  form reset brings back browser-made fields' first values; `<template>` keeps its content;
  a child that sets its parent's state as it starts has the server render the parent once
  more, so the page shows one value; `autofocus` on markup made later is focused.
- **The command line, the policy and the language server (Q-836, Q-837, Q-841 to Q-852):**
  `--watch` started or reloaded while a file is broken or missing watches every file that
  load named and the folders of those not there; errors are said once, and "1 route";
  `--reload` works with `--instances` and passes to `--split`'s services; `--split --port 0`
  tells the services the app's real port; `quillo fix` keeps a byte-order mark first and the
  file's CRLF; `quillo install` refuses an option as a value; `quillo serve` reads its
  options as `run` does; `quillo new` builds inside an empty folder whose parent it cannot
  write; `-h` and `--help` are read first for every command, on stdout; suggestions name every
  command; `quillo help` lists `--env-file`; `classes` and `build` say where a file is not
  UTF-8; a policy file's byte-order mark is skipped, its limits are listed at the start, and
  a request it ends names it in the log (`the policy p.toml allows (max_compute)`) and "the
  server's policy" to pages; the language server gives diagnostics to projects opened through
  links, reads headers in any case, and answers -32002, -32600 and -32700 as the protocol
  asks.
- **The server and the browser (Q-772, Q-774, Q-777 to Q-782):** an attribute is compared
  as a component is taken up: under `--watch` a difference is said (`where it wrote
  title="x", the browser makes title="y"`) and the component made again, and in production
  the browser's value is put in place, so an extension rewriting styles makes nothing again;
  numbers cross as their JSON text in every browser (where `JSON.parse` gives no number's
  text, Safari before 18.4 and Firefox before 135, one pass marks them first), so a float in
  a state, a prop or a `server fn`'s answer stays a float both ways; markup made into text
  is its HTML in the browser (`str`, `+`, `${}`, `json.stringify`, `len`, `==`), as the
  server writes it; `??` guards a stop in a field or an index on its left in the browser;
  a map's keys of letters in any script are bare in its text; with `m`, a line ends at
  `\n` or `\r` (`\r\n` one end), not U+2028; `dec(float)` takes the server's digits on a
  tie, so money from a float agrees.
- **A program's files and Rust (Q-811, Q-814, Q-815, Q-820, Q-822):** `r#type` is a name,
  and the functions after it are read; the `//!` header may follow comments, blank lines and
  `#![...]`; `std::` paths, `-> &'static str` and comments in a signature are read; a `pub
  fn`'s `#[cfg(...)]` is kept, and a call of one left out says so; extensions are read in
  any case (`MAIN.QWE`, `x.RS`), by the language server too; a missing file is one error at
  its import line for every form, with a near file of any kind, and no cascade of unknown
  names; a browser name used through a file taken whole is warned of; messages: no `!`
  marker in list items, a `u64` too large to return names its function, a repeated `use`
  in a used file at the program's path, "chain" for 65 files, `client "javascript:..."`
  neither a script nor a module, Quillo's keywords (`view`, `match`) not exported from
  Rust, and the suggested fixes parse.
- **Plurals, dates and images (Q-824, Q-833):** two `image.convert` calls to one target at
  once each write a temporary file of their own and both finish; `Accept-Language`'s `q`
  weight is read in any case (`Q=0.1`); a locale file may start with a byte-order mark.
- **Values (Q-791, Q-793 to Q-795):** `unique` is hashed, linear for any list on both
  sides, deep lists and tiny floats beside decimals too (20,000 ints took 1.38 s; 8,000
  lists 34 deep 4,812 to 3 ms); `math.trunc`, `math.floor` and `math.ceil` reach 2^63
  (`math.trunc(9.2e18)` was an overflow); `quillo check` warns of a map key written twice
  (`{ a: 1, b: 2, a: 3 }`, `{ a, "a": 2 }`); `-9223372036854775808` can be written, in a
  pattern too, `x - 9223372036854775808` says how, and `-9223372036854775808.f()` says to
  write `(-9223372036854775808).f()`. A refused URL names the scheme it was given
  (`` `redirect` is not given a vbscript: URL ``), in `redirect`, `respond`, `stream`,
  `site_headers` and `navigate`, where each said `javascript:`. In a component, `x | f`
  and `f(x / y)` follow what `f` reads, as `f(x)` does.
- **At the same time (Q-799, Q-802, Q-805, Q-806):** a page joins a channel's room once at
  a time, and each `join` that said yes gets exactly one `leave`, also when the page left
  or closed while its `join` ran (10 pages closing during a slow `join` left `online` at
  10, 20, 30); a request's `sleep` of 5 ms or more waits on a timer and holds no thread
  (with 65 streams sleeping in their loops, a plain request took 0.5 to 0.9 s); over a
  WebSocket a page reading at full speed gets every message of a burst, in order (one got
  13,378 of 20,000), and the guide says the queue's rules hold once a page is 4,096
  behind; the guide says that two requests of one visitor setting the session at once
  lose one's change (the last cookie wins), and to keep such data in a table.
- **Maps and the wire (Q-875's review):** the browser marked big ints and whole floats in
  the JSON it writes with fixed prefixes any text could hold, so `?q=%00int:5` reached a
  `server fn` as the int 5, and `a"\0int:5` broke the whole body; they go through
  `JSON.rawJSON`, or marks made from the page's random tag. A map whose one key starts
  with `$` crosses every wire both ways as `{"$": [key, value]}` (`server fn` arguments
  and answers, island state and props, live data, services, sessions, `idempotent`
  answers), so a key a visitor chose can no longer make an error value, an int, a decimal,
  a file or a prop in another visitor's page or on the server. A state the developer
  tools set reads its JSON as the server does (`2.0` a float, ints past 2^53 exact, keys in
  the order written), and their messages list a map's keys in the order set.
- **CI on Linux:** pulldown-cmark 0.13.6 (RUSTSEC-2026-0433: 0.13.5 parsed nested image
  wikilinks in quadratic time; Quillo does not turn wikilinks on).

### Changed

- Loops over a text are compiled (`s[i]`, `len(s)`, a character compared, `s += x`): `s += "x"`
  a million times, `s[i]` and a `while` over a text take about a quarter of Python's time,
  where they took about as long. Counting into a map by an int key (`m = m | set(k, get(m, k,
  0) + 1)`) is compiled too, and looks the key up once in the interpreter for any key: a
  million counts took 66 times Python's time (each round copied the map) and take a third of
  it. `==` and `+=` on a text no longer allocate. Every case of `bench/vs-python` is ahead of
  Python.
- A `set`, `push`, `merge` or `remove` that fails, caught by `??`, keeps the variable's value
  (`ys = set(ys, 1.5, 0)` left `ys` null).

- Markdown is rendered by pulldown-cmark 0.13; the 545 Markdown files of the projects render
  byte for byte as before (Q-706).
- Speed: highlighting ordinary code is as fast or faster (6 KB: Quillo 36 µs, Rust 56, JS 47,
  Python 40); databases within noise of before and ahead of Python on all three (SQLite 100k
  inserts in 0.96 of Python's time); rendering 200 bound selects 1-3% slower on the server
  (the `selected` marks); runtime.js 1.3 KB larger gzipped.
- Tests: a server a test starts takes a port the system picks (`--port 0`) and reads it from
  its "listening on" line, so another process cannot take it between the pick and the bind.
  A test's error hiding is the thread's, not the whole process's.
- The browser's numbers (Q-775): an int `while` loop 263 to 165 ms, `fib(30)` 12 to 9 ms, a
  float loop 85 to 81 ms (Node); a counter page's runtime 24.4 to 26.4 KB gzipped, as the
  JSON fallback for older browsers ships with every page. Plurals read the digits of their
  count: 100,000 `t()` calls with a float count take 32 ms on the server (26 before; with an
  int, 24), Python's equivalent 54 to 73 ms.
- Exact values (Q-783 to Q-795): int and float loops and builtin calls as before or faster;
  a compiled loop comparing ints with floats about 12% slower, as a float past 2^53 or NaN
  takes a cold path, still far ahead of Python.
- What runs at the same time (Q-796 to Q-806): a page's channel writer sends up to 64
  messages in one write, and in a burst waits at most 50 µs for more, in steps of 10 µs:
  bursts to pages 30 to 60 times faster at a tenth of the CPU (64 pages x 2,000 messages:
  1,454 to 112 ms), an echo's round trip 0.058 ms; a broadcast's text is made once and
  shared by its pages. A request's sleep holds no thread (100 sleeping streams: 25 to
  13,122 requests a second beside them); a sleep under 5 ms still waits on a thread, as a
  timer's are whole milliseconds. A page's topics too many or too long for its address go
  after it (`tm=1`), read 4 KiB at a time and held to what 256 topics take (a socket closes
  and a POST gets 413 past it). Parallel calls and plain routes as before.
- Maps in the browser (Q-875): field reads, literals, keys and `for` as before, and maps
  keyed by ids set in rising order too; a map holding an array-index key out of
  JavaScript's order keeps its key list beside it; `json.parse` of maps whose digit keys are
  out of JavaScript's order 1.38 times as long (their keys read from the text); a component
  showing 1,000 maps again 8.71 to 8.97 ms; the served runtime 2.7 KB larger gzipped.
- Tests: absolute time bounds on work are ten times as long in a debug build, which CI
  runs (the ratios that tell linear from quadratic stay); a browser test's script goes
  to node from a file (it passed Linux's 128 KiB argument limit), and deep JSON is read on
  a request's stack in a debug build.

## 0.7.0 (2026-11-05)

Strict modules, the security review's fixes, money, shared hosting, services the app does not
hold, the failure side, markup as JSX writes it, and the rest below. Not in it, moved to 0.8:
FEEDBACK43's database driver interface (Q-494 to Q-497, a design study) and the apps planned
for 0.7 (the live dashboard and the others, see the 0.7 plan).

### What to change in your code

From the security review (FEEDBACK51.md, Q-640 to Q-676):

- **Channels and media streams are each tenant's own.** Pages on `acme.example.com` and
  `evil.example.com` joining `lobby` are in two rooms, and what code run for a page or a
  request broadcasts, sends or lists reaches its tenant's room; a page with no tenant has
  the rooms of no tenant. Jobs, ticks and the top level reach every tenant's.
  `channel.on(name, f, { tenants: "shared" })` and `media.on(name, { tenants: "shared" })`
  make one for all (Q-644, Q-642).
- **A stream is watched only as `media.on(name, { watch })` says, and sent only as its
  `publish` says**: with no `watch`, no page watches it (RTP cameras included: add
  `media.on("cam", { watch: (peer) => ... })`); with no `publish`, no page sends it
  (Q-642, Q-643). Pages make at most 1,024 streams, 256 a tenant, and watch 32 each; a
  stream keeps 32 MiB of video, all of them 256 MiB.
- **A middleware that gives a map other than the request is a 500** naming it: `{ error:
  "bad api key" }` let the route run. **One that gives null says its routes do not match**:
  `if root { return r }` let everyone else in; now the next route is tried, and with none it
  is a 404. `quillo check` refuses a middleware that can only give null (Q-651).
- **An `accept_calls` `check` accepts only with `true` or a caller map**; null (an `if` with
  no `else`) refused nobody (Q-652). **A visitor's token is not a service caller's**: a JWT
  whose `sub` is a user, not the client, is refused (Q-665). **A remembered service answer
  is the same caller's, function's, tenant's and arguments'** (Q-664).
- **`server fn f() for [...]` takes no calls from pages** at `/_quillo/rpc/f`, which
  answers 404, and is not in the page's script (Q-641).
- **Only what a `site` declaration made is a site**, and calling one needs the `http`
  effect, which `quillo check` says (Q-640).
- **`navigate` and `redirect` refuse `javascript:` URLs** (Q-645).
- **In `<style>`, each `<` of a value is `\3c `** (the same character to CSS), and a
  `<script>`'s JSON has `&` as `\u0026`, so neither can make markup inside `<svg>` or
  `<math>` (Q-646). **A css`...`'s `${}` and a style map's values have `{ } ; \` as CSS
  escapes, and a style map's keys are property names** (Q-668).
- **Quillo follows each redirect of `http.*` itself**: to another origin (scheme, host,
  port) only `accept`, `accept-language`, `content-type` and `user-agent` go on;
  `public_only` is checked at each hop, through a proxy too; `redirects: N` follows N;
  a `file:` redirect is the answer, not a crash (Q-649, Q-650).
- **`run_script` uses Quillo's builtins**, never the program's functions of their names, and
  gives back no function (Q-648, Q-670).
- **`files()` hides `lib.QWE` and the like** on macOS and Windows, and its folder redirect
  stays on the host (Q-656, Q-671). The template and the guide serve static files with
  `files("public", path)`; `quillo check` warns about `file("public/" + path)` (Q-657).
- **`image.convert` will not write the program's own files** (Q-658), and converts at most
  40 megapixels, and what would take more than about 640 MiB (a resize works in 16 bytes a
  pixel); conversions at once take 1 GiB at most (Q-674).
- **`quillo install --example --force` keeps the program's files**, and takes extras only as
  plain file names on the registry (Q-673).
- **`/_quillo/ready` says each check only with the health token** (`QUILLO_HEALTH_TOKEN`);
  others get `{"ready": ...}` (Q-655).
- **Under `--watch` the server answers only `localhost`, addresses and `--allowed-hosts`**,
  with 421 for other names (DNS rebinding, Q-663).
- **New server options**: `--max-connections` (default: what the file descriptors allow,
  less 4,608) and `--connections-per-client` (512; this machine and `--trust-proxy`'s
  proxies are not held to it). A request body must keep coming at 1 KiB/s after its first
  30 s (Q-672).
- **A page that reads `request.cookies`, or the cookie or authorization header, is private**
  (`Cache-Control: private`), as one made with `session()` is (Q-669).
- **JSON is read with at most a value per 8 bytes (or a million)**, and `==`, `str`, JSON
  and sorting go 10,000 levels deep at most (Q-660, Q-661).
- **A new SQLite file is its owner's alone** (0600) (Q-675). **A replica's error names it
  without its password** (Q-659).
- **`quillo lsp` compiles a `.qwer` only when it is saved, in a folder the editor says it
  trusts** (`initializationOptions.trusted`; the VS Code extension sends it) (Q-653).

- **A group's middleware lets the route go on only by giving the request back** (or a copy
  of it, `merge(r, ...)`). Null says the group's routes do not match, as a pattern that
  misses: the next route is tried, and with none it is a 404. `false` now answers 403, an
  error value answers as a route's error, and `true`, text or another map is a 500 naming the
  middleware; all of them let the route run before. A middleware that only logs ends with
  `r` (FEEDBACK51.md, Q-651). A predicate middleware
  (`fn is_admin(r) = ...`) is written `(r) => is_admin(r) ? r : reply 403 "..."`;
  `quillo check` says so (FEEDBACK47.md, Q-534).
- **Typed path, query and form values take their type written one way**: `05`, `+5`,
  ` 5`, `NaN`, `inf` and `1e999` no longer match `int` or `float`, and a `str` parameter
  is no longer trimmed (Q-542). `int(s)` and `float(s)` read as before.
- **`db.exec("begin")` and the like are refused**: write `db.transaction(() => ...)`. A
  `begin` whose `commit` was never reached left the shared connection in a transaction, and
  every other request's writes in it. A script run whole may still hold its own (FEEDBACK44.md,
  Q-580).
- **On PostgreSQL, `set` (without `local`), `set_config(..., false)` and `create temp` are
  refused outside `db.transaction`**: the connection is shared by every request, so another
  visitor read them. Inside one they are reset when it ends (Q-581).
- **`*rest` comes last**: `/f/*rest/more`, and a route inside `routes /docs/*path`, are
  errors (what followed was ignored); so are `route HEAD` and a route under `/_quillo/`,
  which were never called (Q-540, Q-541).
- **An attribute's `"text"` is plain, as in HTML**: `\n`, `\t` and `\\` in it are the
  characters as written, and `\"` is an error. Write `&quot;` for a quote, or the value as
  a Quillo string, `title={"say \"hi\""}` (FEEDBACK48.md, Q-563).
- **`onChange={...}` on an `<input>` or `<textarea>` runs on each keystroke**, as React's
  does, where it waited until the field lost focus; `onFocus` and `onBlur` are `focusin`
  and `focusout`, so a wrapper hears its fields. `@change` and `@focus` are as before (Q-560,
  Q-561).
- **`.class` and `#id` on a view's tag are its `class` and `id`**, which were dropped: a
  view without those parameters is told so. A body given to a view with no `children`
  parameter is an error, where it was dropped (Q-568, Q-569).
- **`{true}` as a child shows nothing**, as `{false}` and `{null}` do, as in React; it
  showed "true". **`aria-*`, `data-*`, `draggable`, `spellcheck` and `contenteditable`
  write `true`/`false` as text**, where `true` was a bare name and `false` left them out.
  **A number in a `style` map is in pixels** but for unitless properties. **`class` takes
  a map or a list**, and a spread's `onClick` and `className` work as written-out ones
  (Q-564 to Q-567).
- **Text in a `<pre>`, `<textarea>` and `<listing>` keeps its lines and spaces**, where it
  was joined into one line, and a `//` line in it is text, not a comment (Q-572).
- **`image.convert` writes the format its target's name says** when no `format` is given,
  where it wrote JPEG; a target named `.gif` or with no extension is an error value
  (FEEDBACK53.md, Q-684). **In a `<script>`, `{null}`, `{false}` and `{"text"}` are JSON**
  (Q-682).
- **A `db.open` that fails as the program starts stops it when nothing takes its error**
  (alone on a line), where it ran on SQLite in memory and lost what it took at the next
  start. `let r = db.open(...)` takes it, to go on (FEEDBACK49.md, Q-602).
- **A service call answered 502 or 504 goes to another copy only if its function uses
  `idempotent`**; any other call gives the error, as the copy behind a proxy may have run
  it (Q-595). **An unsigned 200 from a service is refused under the shared secret** too: a
  service in another language signs its answers, as the guide's protocol says (Q-596).
- **`http.get` and the rest give an error value for an answer past 64 MiB**, where they cut
  it there (Q-599). **The medians, `quantiles`, `percentile`, `describe`, `rank` and
  `histogram` refuse NaN and infinities**, and `money.add`, `sub` and `mul` give an error
  value past `money.of`'s range (Q-605). **`json.stringify` gives
  an error value past 256 MiB or 10,000 levels**, and an answer that would, a 500 (Q-606).

- **An input with `value={state}` and nothing else setting it is two-way now**: typing
  changes the state. Before, typing was undone at once. To show a state without letting
  it change, add `readonly`. The same holds for a checkbox's `checked`, `open`, and a player's `muted`,
  `volume`, `currentTime` and `playbackRate`.
- **`bind:value` on a number or range input gives a number** (`null` when empty), where it
  gave text: drop an `int(...)` around it.
- **`onclick={...}` on an element is a handler**, where it was an error: nothing to change,
  but `onclick={text}` is now refused by `quillo check` as a handler that is not a function.

- **`children` given none is `null`,** on both sides: test `children != null`. `quillo
  check` warns on `str(children)`.
- **`js_new("Promise", ...)` stays a Promise.** To wait for it and catch its failure, wrap
  it: `js("Promise.resolve", js_new("Promise", ...))`.
- **A parent's `@mount` can run before a child's.** Do not count on the order.
- **`session` is the request's session to `quillo check`**, as it is when the program
  runs, so the check may now say more about code that uses it.
- **`==` on two of the page's elements is identity**, not a comparison of their fields.
- **An error a statement drops is logged**, and inside `db.transaction` rolls it back: a
  transaction that used to commit half its work now gives the error. Write `let _ = f()`
  where a failure is truly not wanted.
- **`file()` refuses an option it does not know**, and `date.parse` a date-time with no
  offset: read it with `date.at(v, zone)`.
- **`respond` and `reply` refuse a 1xx status**: it was an interim answer the client waited
  past. `quillo check` says so for a written one.
- **`file()` sends HTML files as they are**, without the prefetching script: a static page
  that relied on it prefetches no more (pages a route makes still do).
- **Signatures between services are version 3**, and answers are signed back. Services
  take version 2 too: deploy the services before their callers.
- **`OPTIONS` on a path is a 204 with `Allow`**, where it was a 405 or a 404, and a method
  HTTP does not define is a 501.
- **`http.*` reads `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY`.** A server with
  them set in its environment now calls out through the proxy.
- **A phone app or `quillo app` has a new origin**, `quillo://app` (or
  `https://quillo.localhost` on Android and Windows): what an installed app's pages kept
  in `localStorage` and IndexedDB stays with the old `http://127.0.0.1:<port>` and is not
  copied. Its database is not touched.
- **A `.rs` file a program `use`s is Rust**, where it was read as Quillo.
- **PostgreSQL's error messages end in their SQLSTATE**, as ` [23505]`. Code that compares
  an error's whole text, or shows it, sees the code too; `db.error_kind` is the way to
  tell failures apart.
- **A Rust file needs `//! effects: ...` to be called inside a `with` function** (or
  `//! effects: none`). Without it, its functions still run outside any `with`, and are
  refused inside one, at run time and by `quillo check`. Add the line to your `.qwer` and
  `.rs` files, and to the catalog plugins you call that way.
- **A link that leads out of the program's folder is refused**, by `read_file`,
  `write_file`, `delete_file`, `file`, `files`, `save_upload` and `check_program`, and so
  is a link that leads nowhere. A deploy's `uploads -> /srv/shared/uploads` needs
  `--allow-path /srv/shared/uploads`.
- **`.quillo/plugins.lock` is checked when it is there.** Deploy it with
  `.quillo/native/`: a library that does not match it is built again, or refused under
  `--no-build`. A folder with no lock loads as before, with a warning; to get a lock,
  remove `.quillo/native/` and run `quillo build`.
- **A connection's calls need the `db` effect**: `analytics.query(...)` in a function
  declared `with http` is now an error. Add `db` to its `with`.
- **`udp.listen` (and `media.rtp`) need the `net` effect**, and `save_upload` and `files`
  the `fs` effect: add them to the `with` of the functions that call them.

- **`a + b` on two money values adds them**, and `a - b` subtracts: `+` merged the two
  maps, so `total + line` was the line. Only maps of exactly `amount` and `currency`; two
  currencies give an error value (FEEDBACK50.md, Q-620).
- **Every `money` function holds a money map to `money.of`'s rules**: more decimals than
  the currency has, `"1e3"` as text, and minor units past an int are runtime errors, where
  some functions rounded and others did not. `money.round(m, mode)` makes money of an
  amount with more decimals (Q-622, Q-623).
- **`money.allocate` gives the units left to the largest remainders**, not the first
  shares: $0.10 split 1:99 is 0.00 and 0.10, not 0.01 and 0.09 (Q-631).
- **`round(x, n)` of a float rounds its shortest decimal**: `round(1.005, 2)` is `1.01`, as
  `format_number` writes it, where it was `1.0` (Q-626).
- **`money.format` writes what `Intl.NumberFormat` writes**: XOF, XAF, XPF, XCD and XCG have
  symbols, IRR and 12 other currencies show no decimals, the French locales have `$CL` and
  the like, and es-CO, zh-Hant-TW and 70-odd other locales are their own, not another
  country's (Q-627).
- **A PostgreSQL `money` column reads with its `lc_monetary`'s decimals**: under a yen
  `lc_monetary`, ￥500 is `500m`, where it was `5.00m` (Q-625).
- **`money` is a type**: in a pattern, `{ price: money }` checks money where it bound a
  variable named `money` (Q-621).

### Money (FEEDBACK50.md)

- **A `money` type** for patterns, record fields and annotations: `route POST /pay { json: {
  price: money } }` takes `{ amount, currency }` held to `money.of`'s rules and gives the
  money; a request that fails is a 400 whose log line says which rule. `type Money = {
  amount: dec, currency: str }` checked the shape only (Q-621).
- **`money.parse(text, currency, locale)`** reads an amount as the locale writes it,
  `"1.234,50 €"` in de-DE; `money.of` reads text as a program writes it, so a German
  `"1.000"` was €1.00 (Q-624).
- **`pow(d, n)`** raises a decimal to an int's power, for interest and loan payments, which
  went through floats (Q-632).
- **`format_number(x, locale, digits, mode)`** rounds as `mode` says, `"half_even"` as money
  does (Q-626).
- **Withdrawn currencies** (BGN, HRK and 22 others since 2007) still read, so old invoices
  are money (Q-630).
- **122 locales** for `money.format` and `format_number`, every currency's symbol in each,
  written from ICU 77 by `scripts/money-tables.mjs`; a locale written as another is said in
  the log once (Q-627).
- **Warnings**: SQLite's log says once which `money`, `decimal` or `numeric(p, s)` column
  keeps 15 digits (Q-628), and `quillo check` says when a list is given as a query's
  parameters, stored as its JSON text (Q-629).
- **In the browser too**: money's range check of FEEDBACK49's Q-605, and text trimmed and
  quoted as the server does.

### What a person changes is two-way, and `onclick={...}` is a handler

- **`value={x}` is two-way when `x` is the component's state** and nothing else on the
  input sets it: typing changes `x`, as `bind:value={x}` did. The same for every attribute
  a person changes in the page: `checked` and `indeterminate` on a checkbox, `value` on a
  `<textarea>` and `<select>`, `open` on `<details>` and `<dialog>`, and `muted`, `volume`,
  `currentTime` and `playbackRate` on `<video>` and `<audio>`. It stays one way with a
  handler of its own on an event saying it changed (`@input` or `@change` for a value),
  with `readonly`, on a radio or a `<select multiple>`, with a `type={...}` or a spread, or
  when the value is not a state. A number or range input gives a number back, with
  `bind:value` too (it gave text); a player's volume and speed are kept in range.
- **`bind:` takes each of those**, where it took `value` and `checked` only; two bindings
  on one event (`muted` and `volume`) are one listener. The runtime sets `indeterminate`,
  `volume`, `currentTime` and `playbackRate` as the properties they are.
- **`onclick={() => ...}` on an element is `@click={...}`**, in any case (`onClick`).
  `onclick="..."` stays HTML's own, text made from values stays refused, and on a view or
  component `on_change={...}` stays a parameter.

### Routes take `with`

- **`route GET /report/:day with db = ...`** and **`routes /admin | signed_in with db {
  ... }`**: what the route's body, what it calls and its groups' middleware may do, held
  in `quillo check` and as it runs, as a `with` function is. A group's `with` is what its
  routes may use at most; a route or a group inside may narrow it, and asking for more is
  an error. A route without `with` may do anything, as before. A stream's function
  (`stream(f)`, `events(f)`) and a component's `defer(...)` parts run under the route's
  `with` too, though they run after its body; a group at the root takes it
  (`routes / with db { ... }`). When a policy takes away an effect the `with` names, the
  error says it was the policy.

### The command line

- **`--max-body` takes a size as the policy writes one**: `--max-body 50MB`, `512KB`,
  `2GB` (1024-based), in any case, or plain bytes as before.

### Editors

- **A new `import` line offers its file's names at once**: on `import lib/order.qwe use `,
  `quillo lsp` lists the file's public functions before a letter is typed, from the file
  itself (as the editor has it) when the program does not take it yet. They came only once
  a name was begun.

### Shared servers: a socket, FastCGI and a policy (FEEDBACK41.md)

- **`--listen unix:PATH`** listens on a Unix socket instead of a TCP port, made with mode
  0600 (`--socket-mode 660` for the web server's group) and removed when the server
  stops; a socket left by a server that did not stop cleanly is taken over, one another
  server answers on is not. `$PORT` may hold an absolute path. A path is at most 100
  bytes. Not with `--split`, `--instances`, `--register` or `--join`. Jobs reach the
  program through the socket, and the server says to add `--trust-proxy 127.0.0.1` until
  it is given (Q-479).
- **`quillo fcgi main.qwe`** answers FastCGI: on the socket `mod_fcgid` gives it on file
  descriptor 0, or its own with `--listen unix:PATH` or `--port`, for nginx's
  `fastcgi_pass`. Each request goes to the server as an HTTP request, `request.ip` from
  `REMOTE_ADDR`, and its answer goes back as it is written (Q-479).
- **Links are followed and judged** by the file builtins: one leading out of the folder,
  or nowhere, is refused, said once in the log with `--allow-path FOLDER`; `files()`
  gives the same 404 whether a folder outside is there or not, and `check_program` reads
  nothing through such a link (Q-475).
- **Effects:** `save_upload` and `files` are `fs`; `udp.listen` and `media.rtp` are `net`.
  A Rust file declares `//! effects: db, http` (or `none`), checked by `quillo check` and at
  run time; without it, it is refused inside a `with`, and an editor's hover says
  `(effects: not declared)` (Q-476).
- **`.quillo/plugins.lock`** holds each built library's SHA-256, written by every build,
  `quillo build` included; a library that does not match is built again, or refused under
  **`--no-build`**, which never builds (Q-477).
- **A policy** (Q-478): `/etc/quillo/policy.toml`, then each `--policy FILE`, then
  `quillo.policy` beside the program, each only narrowing. `[plugins] build` and `allow`
  (`name` or `name@version`, by `//! version:`), `[effects] allow`, `[http] private` and
  `hosts` (each redirect judged too), `[env] names`, and `[limits] workers`,
  `max_compute` and `max_body`. `quillo check --policy FILE` says what would be refused.
  Not done: a signed catalog, `local = false`, `--sandbox` and WebAssembly plugins;
  `private = false` covers `http.*`, not `db.connect` or UDP.
- **Threads:** a wait thread the system will not start is said once, and starting one is
  tried again when there is none; a `ulimit -u` below what the server may use is said at
  start (Q-480).
- **The Linux release binaries are linked against glibc 2.17** with zig, so they start on
  CentOS 7, CloudLinux 7 and 8 and AlmaLinux 8, and still load plugins. The release
  workflow checks the binary in a CentOS 7 container. It takes effect with the next tag
  (Q-424).
- The guide has "The policy" and "Quillo on a shared server (cPanel)", and the website a
  guide for shared hosting.

### Services the app does not hold (FEEDBACK40.md)

- **A service's file that does not parse is read for its `server fn` headers**, so a
  mistake in a route or a body of the service no longer stops an app run with
  `--no-check` (Q-471).
- **With no file and an address**, the app asks the running service for its functions as
  it starts, as a signed call (`_contracts`), each address in turn; only those the caller
  may call are given. `quillo check` warns that the file is not here, and a call to it
  still needs `http` (Q-471).
- **`--split --watch`** reloads the app when a service's files change, so a function added
  to a service is called at once (Q-472).
- **`--split` checks the whole tree once** before starting any service, each warning said
  once; the services skip their first check (Q-473).
- **A service that ends at start is said at once**, `service billing stopped at start
  (exit status: 1; see its lines above)`, not after 20 s (Q-474).

### Databases and plugins (FEEDBACK43.md)

- **`db.exec_many(sql, rows)`**: a statement for each row, all of them or none, `{ changes
  }`. On PostgreSQL and MySQL an insert goes as up to 1,000 rows a statement, as Python's
  `executemany` sends them: 20,000 inserts on PostgreSQL and MariaDB were 1.7 and 2 times
  Python's time as a loop of `db.exec`, and are now ahead of it.

- **A connection's calls need the `db` effect** in `quillo check`, as at run time;
  `c.query(...)` on an untyped parameter in a `with` function without `db` is a warning
  (Q-488).
- **A Rust function's effects are its file's `//! effects:` line** (Q-489, with Q-476).
- **`db.open` replacing a database is announced** to live pages, every tenant's, and live
  calls are not answered from the old one (Q-490).
- **One database, one name:** a SQLite file by any spelling is one connection and one set
  of live topics; a server URL's topics leave out the user, the password and the
  options (Q-491).
- **`conn.sql_name(x)` and `db.sql_name(x)`** quote a name for that database: backticks on
  MySQL (Q-492).
- **PostgreSQL errors carry their SQLSTATE** (` [23505]`), and `db.error_kind` reads it
  first, so it works whatever language the server speaks (Q-493).

### What `quillo check` and the parser say (FEEDBACK45.md)

- **An unclosed `/*` is an error** at the line it opens; the rest of the file, routes and
  functions included, was silently the comment (Q-503).
- **`route GET /a/{id}` says to write `:id`**: it was the path `/a/` and a request pattern,
  and answered every request 404 (Q-506).
- **`"Hello ${name}"` is pointed out** when `name` is the program's: a plain string is not a
  template (Q-510). `\${` keeps the text.
- **A name imported as a builtin's (`use price as print`) is warned of**, as `fn print` is
  (Q-507).
- **`let fn`, `let true`, `let null` and `type if`** get the keyword message (Q-526, Q-516).
- **An assignment may be an arrow's body**: `() => count += 1` is `() => { count += 1 }`;
  `@click={count += 1}` says a handler is a function (Q-518).
- **Two elements side by side in a view** say so, not "comparisons do not chain" (Q-521).
- **Numbers**: `1__000`, `1_`, `08`, `010` and `1.` are errors that say how to write them,
  there is no `--` (`--n` was `n`), a map takes one `,` between entries, and a lone CR is
  a line break, in a string too, as Python's universal newlines read one (Q-529). What
  changes: a program with one of these no longer loads.
- **The command line**: `quillo check main` finds `main.qwe`, `--policy` without its file
  says so, and the summary says `1 route` (Q-528).
- **New errors in `quillo check`**, each a program that fails or misbehaves as it runs: a
  parameter, a field, a local `fn` or `state` named twice (Q-508); a pattern's type that
  does not exist (Q-504); a `let` or `match` that cannot match (Q-516); a lambda's argument
  count, by its `let` name or given to `map`, `reduce` and the like (Q-515); `list<int>`
  given `[1, "a"]` (Q-512); `request.qurey` (Q-511); a call from top-level code to what
  reads a global defined later (Q-514). What changes: a program with one of these no longer
  starts until it is fixed (the kit's `nav_search` had one: its own `set` hid the builtin).
- **New warnings**: a dropped `push`/`sort`/`i + 1` and a `while` nothing in it ends (Q-505);
  arms that never run (Q-527); `intt =>` (Q-504); `index_of(...) == -1`, `if ready {`, `:id + 1`
  (Q-513); misspelt attributes and events, `className`, `<card>` for a view (Q-509).
- **"Did you mean" is the same every run** and counts two letters swapped as one edit
  (`pirnt` → `print`, `lne` → `len`); it knows other languages' names (`None`, `undefined`,
  `console.log`, `parseInt`, `toUpperCase`) and is given for module functions
  (`json.prase`), methods, `xs.lenght` and types (`integer`, `string`, `Int`) (Q-517).
- **Types written another language's way** say how Quillo writes them: `int: a` swapped,
  `a int`, `int a`, `int[]`, `list[int]`, `a?: int`, `?int`, `): int`, `let x: int`, `state
  n: int`, and in an arrow's parameters the type's own error (Q-519).
- **Other languages' words say what Quillo writes**: `===`, `!==`, `++`, `*=`, `&`, `.5`,
  `0b101`, curly quotes, dashes and full-width characters, `const`, `var`, `fun`, `elif`,
  `switch`, `case`, `typeof`, `if x = 1`, `x ? 1` with no `:`, `type user =`, `import x
  from` and `import { x } from`, each with the Quillo way; a slip of a keyword (`retrun`)
  says which (Q-520).
- **Several syntax errors in one run**: after the first, `check` goes on from the next
  statement and says up to nine more (Q-522).
- **Import mistakes are said once, at the import line**, by the path the program wrote:
  a name the file does not have, with "did you mean", a private one, a missing file with
  the file nearest its name, and none of the "unknown name" errors they caused.
  `import lib/m.qwe price` shows `import lib/m.qwe use price`; `use *, price` says `*` is
  alone; "used before it is defined" names another file's line. New warnings: a name used
  by its old name after `as`, and a `use` repeated. What changes: a `use` list that ends
  with `,` at a line's end is an error, where it took the next line's first word (Q-523).
- **Names out of their scope**: in a function, a name read before its `let` is "used
  before it is defined (line 3)", as at the top level; a block's `let` read after the
  block says which block, and how to have it after; neither is called unused. A name
  with a lookalike letter (Cyrillic `а` for `a`) says which, by its code point (Q-524).
- **Builtins' arguments by their names**: `missing argument `f`, a function, in call to
  `map`: map(xs, f)`, not `arg2`; `reduce(xs, f)` says the start value comes first;
  `max(1, 2)` is one error, "takes one list"; `f(a: 1)` shows `f({ a: ... })`; `n -= "a"`
  after `let n = 1` says int, not any, and `xs += 2` on a list is an error, as `[1] + 2`
  is. What changes: `str()` with nothing to make text of is an error; it gave null (Q-525).

### An app on the starter (FEEDBACK46.md)

- **A `.qwer` function's signature may name what the file's `use` lines bring in**:
  `use serde_json::Value;` then `pub fn role(auth: Value)`, or a `HashMap<String, i64>`
  after `use std::collections::HashMap;`. The glue copied each type's text into a file
  where those lines did not reach, and the build failed with an error in `lib.rs`, a file
  the program never wrote (Q-530).
- **A style map is properties on every element on the server**: on one with `class=`, a
  spread, or a `<textarea>`, `style={{ color: "red" }}` was written as JSON (Q-531).
- **`quillo check` warns of an input whose value nothing takes**: `value={v}` where `v` is a
  view's parameter or a component's prop, `value={form.name}`, and a checkbox's `checked`,
  with no handler, `bind:`, `name=`, `readonly` or `disabled` on it. The input showed the
  typed text while the program kept what it had (Q-532).

### What one request's transaction lets another see (FEEDBACK44.md)

- **Inside a SQLite transaction, a query to PostgreSQL or MySQL waits in place**: it paused
  the request, and others ran on SQLite's one connection inside the transaction, reading its
  rows before it committed and losing their own writes when it rolled back (Q-579).
- **Transactions are `db.transaction`'s**: `begin`, `commit` and the rest alone are refused,
  a script that leaves one open is rolled back, and so is a SQLite connection still in one
  at the end of a request, which the log says (Q-580).
- **A PostgreSQL connection's session is reset before another request gets it**, after a
  transaction that changed it; outside a transaction such a change is refused (Q-581).

### Routes and groups (FEEDBACK47.md)

- **A guarded group says nothing before its middleware**: a request under its path that
  no route of it takes goes through the middleware before the server's 400, 404, 405, 501
  or `OPTIONS`, which told a stranger its paths, methods and parameter types (Q-539).
- **Middleware gets the request as sent**: a JSON field, query field or path parameter
  named `request` replaced it, so a body could carry the headers it checked (Q-535).
- **`routes /api { headers: ... } | g { ... }` is an error**: `| g` was a second pattern
  that took any request, so neither the pattern nor `g` refused anything (Q-536).
- **`site_headers` and `allow_cross_site` match the path as routes read it**:
  `//admin/users` and `/%61dmin/users` got the admin page without its headers (Q-537).
  `request.route_path` gives that path, and `request.host` the host as it names one: lower
  case, no trailing dot or default port, as `headers.host` is now too (Q-545).
- **Query and form values**: a blank field with a default takes the default, `bool` reads
  any case, and a list takes `tag[]` when no `tag` came (Q-543).
- **A 400 says why**: the route that came closest and its first miss, in the log always and
  under `--watch` in the answer, with a hint for an unchecked checkbox (Q-544).
- **`quillo check`**: a server fn a guarded page calls that checks nothing itself (Q-538), a
  route an earlier one always takes first (Q-541), and a message of its own for each
  mistake on the route line: `{ ok: true }` meant as an answer, a pattern after `with` or
  before `?`, a method or no `/` after `routes`, a character a path cannot hold, a
  parameter named `if` or `1`, and a group with nothing in it (Q-546).

### A long-read site on Quillo (FEEDBACK53.md)

- A route's path takes `@`, `+`, `!`, `$` and `&` in a piece, and a parameter after `@` or
  `~`: `route GET /@:name` takes `/@ann`, as profiles' addresses are written; `quillo check`
  and `quillo docs` know it (`/@{name}`) (Q-683).
- `<script>{x}</script>` writes `null`, `false` and a string written in the braces as
  JSON, where they wrote nothing or the text bare and the page's script did not parse
  (Q-682).
- `image.convert` writes the format its target's name says unless `format` is given, where
  it wrote JPEG to `out.png`; a name that says none is an error value (Q-684).
- `\s`, `\d`, `\w` and the other escapes of a regular expression in a string say to write
  `\\s`, without the note about a newer Quillo (Q-008). SQL written in `` sql`...` `` and
  then added to (`BASE + "where id = ?"`) is no longer taken for SQL built from values
  (Q-029).

- A review of these fixes found more, fixed: `/users/${id}` and `/@{name}` are an error that
  says to write `:id`, where `$` was taken as text and `{id}` as a request pattern; `routes
  /@:name { ... }` parses at the top level; a `%` escape in a fixed piece is read as the
  request's is (`/a%20b` never matched); `/~:id(int)` is not said to be hidden by
  `/:id(int)`; OpenAPI has one path for one shape, whatever the method; `{dev && raw(x)}` and
  an `if` with no `else` in a script write nothing for `false` and `null`; a component's
  `<script>{"..."}</script>` is told why it is refused, by `quillo check` too; a program's own
  `sql` is not the tag; two formats of one image made at once do not share a file.

### The failure side (FEEDBACK49.md)

- A page could stop a channel's thread with one WebSocket frame whose length wrapped the
  size check, and stay counted and ignored; control frames longer than 125 bytes, or in
  parts, are refused too (Q-593). A page that stops reading no longer costs a thread for
  each reliable message, and is let go when a frame to it takes more than 30 s (Q-594).
- JWKS: a failed fetch is not tried again for a minute, the old keys are used meanwhile,
  one fetch at a time for 3 s at most, and the URL goes to the log, not to the caller
  (Q-597). UDP: 64 MiB a port, a share for each sender, a lost batch logged; H.264: 16 MiB
  a frame (Q-604).
- A transaction SQLite ends by itself (a full disk) makes each later statement an error,
  where they committed one by one, and `idempotent` stored a success (Q-591). A PostgreSQL
  transaction whose idle connection the server ended is begun on another, and
  `db.error_kind` says `"connection"` (Q-601).
- A request followed at once by the client's end is answered and the connection closed,
  where it stayed open (Q-600).
- A review of these fixes found seven more, fixed: a page that neither reads nor closes is
  let go with its thread and socket, and the live updates' socket has the same deadline;
  `json.stringify` and a JSON answer are held to their limits as they are written, with no
  tree made first (3.9 GB and 2.8 s were 298 MB and 0.19 s); a transaction after a
  PostgreSQL restart gets past every ended connection in a pool of eight; an error made later
  in the memory of a `db.open` error taken is not taken for it; a 504 from a copy that joined
  is not run here as well; `money.neg` of the least amount is an error value; a client's end
  after a full read buffer is not lost; only the name `idempotent` makes a function safe to
  run twice.
- Still open: a derived value that throws in the browser (Q-590), a server fn's answer
  that is not one (Q-598), dropping a value nested hundreds of thousands deep (Q-592, but
  for JSON), SQLite's lock wait holding others (Q-603), the compute budget inside long
  builtins (Q-606), and the fuzz targets (Q-608). An ignored test of a bug since fixed runs again (Q-609).

### Markup as JSX and HTML write it (FEEDBACK48.md)

- Every named entity HTML has (`&rarr;`, `&times;`, `&rsquo;`), in text and in each kind of
  attribute value, where ten were known and attributes had none: `href="?a=1&amp;b=2"`
  sent `amp;b` (Q-562). An attribute's `"text"` is plain, so `pattern="\d{3}"` loads (Q-563).
- React's event names that are other DOM events: `onChange` on a field is `@input`,
  `onDoubleClick` is `@dblclick`, `onFocus`/`onBlur` are `@focusin`/`@focusout`; `…Capture`
  is an error; `quillo check` warns of `@mouseEnter`, as event names are compared as written
  (Q-560, Q-561).
- `<pre>`, `<textarea>` and `<listing>` keep their text as written (Q-572). `{/* ... */}`
  between attributes and `{// ...}` are comments (Q-575). `<img></img>` is taken, and a
  stray `</img>` gets a fix that removes it, where the fix closed the element around it
  (Q-576).
- `.class` and `#id` on a view are its `class` and `id`; `<Card.Header/>` and
  `<React.Fragment>` say what to write (Q-568, Q-577). A body for a view with no `children`
  is an error (Q-569).
- `quillo check` warns of React's own attributes (`defaultValue`, `defaultChecked`, `ref`,
  `dangerouslySetInnerHTML`, SVG's `strokeWidth` and the like) (Q-570), a function where
  markup goes (Q-571), text and an inline element on lines of their own, which touch on the
  page (Q-573; the site and the kit had 26), and `xs.map((x, i) => ...)`, as `map(xs, f)`
  was checked (Q-574). `view Card(props)`, `view Card({ title })`, `let Tag = "h1"`,
  `<Fragment>`, `{...xs}` among children and `for (x, i) in xs` each say how Quillo writes
  them (Q-577).
- Values in markup as React writes them, on the server and in the browser after an update:
  `{true}` shows nothing, ARIA's and `data-*`'s booleans are `"true"`/`"false"`, a style
  number is in pixels but for React's unitless properties (and `msFlex` is `-ms-flex`),
  `class` takes a map or a list as `clsx` does, and a spread follows the written-out rules
  (Q-564 to Q-567). A function as a child shows nothing, where it was `<fn <fn>>`; in the
  browser it was called, so `{inc}` ran the handler as the page rendered (Q-571). A `<pre>`
  whose text starts with a line break gets the one the browser drops back (Q-572).
- A review of the rendering found three more, fixed: a spread's `onChange` beside
  `bind:value` or `bind:checked` took the binding's place, so it stopped binding (now the
  spread's handler runs after the binding's); a function given to an attribute was written
  `<fn <fn>>` and called in the browser as the page loaded (now nothing, on both sides); a
  spread's `CLASS` was a class to the server only.
- The guide has a "From JSX" table beside "From HTML" (Q-578).

### Found measuring coverage

- **`quillo app --port` and `--listen` say they mean nothing there**: the window asks the
  program directly, with no socket, so a port was taken and ignored without a word. Both now
  stop it with a message, as `--host` does.
- **Under `quillo fcgi`, an answer whose head is over 1 MB is a 502** that says so. It was cut
  at 1 MB, and the rest of the head, its `content-length` and all, went out as the body.

### Developer tools and the browser (FEEDBACK42.md)

- **`quillo check` warns of `sleep(0.2)`**: `sleep` takes milliseconds, and a number written
  between 0 and 1 is a fraction of one (Q-482).
- **Under `--watch` the runtime keeps its names to itself**, as in production: they were
  the page's globals, so a page's own `function report()` was replaced by Quillo's, and a
  script loaded later with a `function run()` stopped every component not yet taken up,
  under `--watch` only (Q-481).
- **An interface for developer tools**: a page with `<html data-quillo-devtools="1">` says
  what happens as `quillo:devtools` events (components, states, errors, components made
  again, server fn calls, live changes, channel messages over either transport), and
  answers `list`, `where` and, under `--watch`, `set`. A page that does not ask does
  nothing more, measured on a keyed list of 1,000 rows (Part C; Q-483, Q-484, Q-485).
- **A component inside another whose text a script changed is made again**: it was left
  empty, with a bare `quillo: text` in the console.
- **Quillo puts no names of its own on `window`** but `quillo` and a page's
  `__quilloErrors`: what its loader, prefetching, translations, deferred parts and the
  runtime hand each other is under one key, `window[Symbol.for("quillo")]` (`booted`,
  `resumed`, `remade`, `via`, ...), where `__quillo_booted`, `__quilloRemade` and the like
  were names each. A test or a tool that read them reads the key, or listens to the
  developer-tools events; Quillo DevTools reads both (Q-486).
- The guide says what `data-s` and `data-p` hold (Q-487), and to run with `--no-wt` to see
  a channel's messages in the Network panel (Q-485).

### HTTP the server speaks (FEEDBACK36.md)

- **A body is dropped only when nothing of it comes for 30 s**, with a 408 and a log line
  (`POST /up HTTP/1.1 -> 408 (nothing of the body came for 30 s)`); a slow upload that
  keeps coming is taken. A head not whole in 30 s is a 408 too (Q-432, Q-446).
- **`respond` and `reply` take 200 to 599**: a 1xx is refused, at run time and by `quillo
  check` (Q-433).
- **HTTP/1.0 is answered as 1.0:** a stream with no length, ended with the connection, no
  `100 Continue`, no upgrade (Q-434).
- **Methods:** every 405 has `Allow`; `OPTIONS` on a path, and `OPTIONS *`, get 204 with
  `Allow`, so HAProxy's `option httpchk` finds the server up; an unknown method is 501
  (Q-440).
- **Content codings:** `Accept-Encoding` is read with q-values and in any case
  (`gzip;q=0` refuses, `*` and `x-gzip` count) (Q-439); a gzip request body is decoded, with
  `--max-body` counting what it decodes to, and another coding is 415 (Q-438).
- **Conditional requests:** `If-Modified-Since` is compared as a date, in HTTP's three
  forms; `If-Match` and `If-Unmodified-Since` give 412; a route's own `etag` gets a 304;
  `/_quillo/media.js` has an `ETag` (Q-441).
- **Smaller ones:** 414 for a long request line, no body on a 205, reason phrases for the
  statuses that had none (`418 I'm a teapot`), 417 for an `Expect` other than `100-continue`
  (Q-446). WebSocket handshakes are checked: 400 for a wrong method or headers, 426 for a
  version other than 13 (Q-445).
- **`send(value, { event, id, retry })`** in `events`, and `listen` sends `Last-Event-ID`
  when it connects again and waits the server's `retry` (Q-444).

### The HTTP client (FEEDBACK36.md)

- **Repeated headers are joined with `, `**; `Set-Cookie`s are joined by a line break and
  given one by one in `r.cookies`, on `http.stream`'s result too (Q-435).
- **Interim answers are passed over**: a `103 Early Hints` no longer costs the whole time
  limit (Q-436).
- **Redirects:** `Authorization` goes on to the same host; a 307 or 308 after a write is
  followed with its method and body; to another origin, `Authorization` and `Cookie` are
  dropped (Q-437).
- **`deflate` and `br` answers are decoded**, and another coding is an error value that
  names it (Q-438).
- **Proxies and certificates:** `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` (never
  for this machine; `public_only` checks the host before the proxy), and the options `ca`
  (a PEM file or `"system"`), `cert` and `key` (Q-442).
- **`http.stream`'s function gets `(data, name, id, retry)`** for each event (Q-444).
- HTTP/2 stays out of reach of `http.*`: Apple's push service and gRPC cannot be called.
  The guide says so in Possible or not (Q-443).

### Plain HTML, static files and other text (FEEDBACK36.md)

- **`files("site", path, { index, extensions, not_found, cache })`** serves a folder as a
  static host does: `index.html`, `/about` finding `about.html`, `/docs` redirected to
  `/docs/`, `404.html` for a miss; dotfiles (but `.well-known/`), `.qwe` and Rust files are
  not served. **`quillo serve FOLDER`** does it with no program (Q-450).
- **`quillo run page.html`** (or `.md`, `.rs`, `.css`, and others) says what the file is and
  how to serve it; a folder with an `index.html` and no `main.qwe` is told `quillo serve`
  (Q-450).
- **`file()` sends a file as it is**: an answer with an `ETag` or a range gets no prefetching
  or `--watch` script (Q-447).
- **Markup takes HTML as written:** `<!-- comments -->` are dropped, attribute values may
  be single-quoted or unquoted, and `<!DOCTYPE html>` before `<html>` is passed over (Q-448,
  Q-449). `quillo check` warns of `{ return ... }` in markup, which left the function, and
  hints lowercase for `<DIV>`. The guide has a "From HTML" list (Q-449).
- **New file types:** `vtt`, `ics`, `rss`, `atom`, `jsonld`, `geojson`, `yaml`, `toml`,
  `m3u8`, `mpd`, `ts`, `m4s`, `srt`, `tsv`, `md` as `text/markdown`, `bmp`, `tiff`, `heic`,
  `jxl`, `mov`, `m4a`, `aac`, `flac`, `opus` and more (Q-451).
- **Markup whose root names a namespace, or that starts `<?xml`, goes out as XML:**
  `application/xml`, `application/rss+xml` for `<rss>`, `application/atom+xml` for
  `<feed>`, `image/svg+xml` for `<svg xmlns=...>` (Q-451).
- **`markdown_page(text, { title, lang, html })`**: Markdown as a whole page, with a doctype,
  a `<head>` and a title (Q-451).

### Rust beside the program, and styles (FEEDBACK37.md)

- **A server whose stderr pipe has closed still stops** on `SIGTERM` or Ctrl-C: Quillo's
  own messages no longer panic (Q-452).
- **`.rs` files are Rust as `.qwer` files are:** `use "tw.rs"`, `import tw.rs use f`,
  `quillo build`, `--watch` and the language server. `.qwer` stays for the catalog and
  `quillo install`. A crate's own `src/*.rs`, which nothing uses, is left alone (Q-453).
- **Rust files not built yet are built in one cargo call** before the program runs, side by
  side: `quillo: building ./a.rs, ./b.qwer together` (Q-454).
- **`build "/app.css" = expr`** at the top level is made once the port is open, a request
  for it waiting; it is served with its extension's type and an `ETag`. A failure at the
  first start stops the program; after a reload the last good one is kept. Not done:
  `quillo build` making it ahead of time (Q-454).
- **`style_with(f)` takes a function**, Quillo's or Rust's, given all the marked styles
  (Q-455).
- **`classes()` and `quillo classes app.qwe`** list the classes the program's markup writes,
  for a CSS engine or a safelist; classes made from parts are not in it (Q-456).
- **A `style` at a file's top level styles the components that file declares**, and
  nothing else, sent once for the file; a component's own rules win over it. `style
  global` at the top level likewise, unscoped (Q-457).

### Authentication between services (FEEDBACK38.md)

- **A caller proves who it is with `auth`** where it declares the service: `"quillo"` (the
  shared secret, the default), `{ key, as }`, `{ bearer }`, `{ oauth: { token_url,
  client_id, client_secret, scope, audience, method } }` (kept until 30 s before it
  expires, fetched again once on a 401), and `{ mtls: { cert, key, ca } }`;
  `forward: ["authorization"]` passes the visitor's headers along (Q-458, Q-460, Q-462).
- **A service says what it takes with `accept_calls { keys, shared, jwt, client_cert,
  check }`**: keys by caller, the shared secret or not, JWT access tokens against a JWKS or
  a secret (`audience` required, `exp` required), a client certificate from a proxy
  `--trust-proxy` names, and a check of its own (Q-458, Q-460, Q-462).
- **`server fn refund(...) for ["shop"]`** takes calls from those callers only, before the
  arguments are read, and `quillo docs` lists them in `x-quillo-callers` (Q-458).
- **`request.caller`** (`{ name, by, scopes?, claims?, subject?, uri? }`), **`request.user`**
  (the forwarded token, verified) and **`request.forwarded`** in a function a service
  answers with (Q-458, Q-460).
- **Version 3 signatures** cover the caller, the deadline and the contract, and answers are
  signed back: a caller with a key of its own refuses an unsigned answer (Q-458, Q-459).
- **`QUILLO_SERVICE_SECRET=new,old`** or **`QUILLO_SERVICE_SECRET_PREVIOUS`**: signed with
  the first, any taken, so the secret changes with no downtime (Q-461).
- **Joining:** a copy signs with `QUILLO_SERVICE_KEY`, and an app with a key for that
  service takes copies only so; `--join-as URL`, `--no-join`, `--allow-join a,b` (Q-459).
- `quillo check` warns of a `key` or `client_secret` written in the source. The guide has
  "Authentication between services", the routes way with `jwt.qwer` included (Q-464). In
  the catalog, `jwt.qwer` 2.0.0 (`jwt_check` with `audience`, `issuer` and `require`,
  `jwt_verify_jwks`, `exp` required) and `oidc.qwer` 1.1.0 (`oidc_client_credentials`,
  `oidc_verify_access`) (Q-463).

### Apps with no socket (FEEDBACK39.md)

- **A program runs inside an app with no HTTP server** (`src/embed.rs`): `quillo app` and
  the iOS and Android shells hand it each request, through the `quillo://app` scheme (iOS,
  macOS, Linux) or `https://quillo.localhost` (Android, Windows) (Q-465).
- **The origin stays the same from one launch to the next**, and the page's `localStorage`
  and IndexedDB with it (Q-466).
- **No socket:** iOS has none to take back from a suspended app, no other app on the phone
  can reach the program, and the ATS exception and Android's cleartext config are gone
  (Q-467, Q-469).
- **What a scheme cannot carry goes through the app's bridge**: bodies on Android, files and
  `FormData` on WebKit, and the connections of live data and channels (Q-470). Forms with
  files (WebKit) and every post (Android) go through it, and the page goes to their answer.
- **The app keeps the cookie jar** and a session key, so a session lasts a relaunch;
  `request.ip` is `"app"`; jobs, ticks and calls made later reach the program with no
  connection to itself (Q-468).
- **New calls into the runtime:** `quillo_open`, `quillo_request`, `quillo_bridge`,
  `quillo_bridge_next`, `quillo_shell_js`, `quillo_suspend`, `quillo_resume` and the
  `quillo_free_*` calls, and on Android `Quillo.open`, `request`, `bridge`, `bridgeNext`,
  `shellJs` and `suspend`. `quillo_start` is kept for apps built before (mobile/README.md).
- Only macOS has run `quillo app` this way; Windows and Linux use the same wry code, and
  HarmonyOS is not built. A `stream` route's answer arrives whole in these shells.

### Errors that are dropped, and plugins (FEEDBACK27.md)

- **An error no statement takes is written to the log**, with where it was dropped, where it
  was made and the request: `main.qwe:10: an error no statement took, made at main.qwe:6:
  odd (pass it on with ?, give a value with ??, or take it with let _ =)`. A script writes
  it to stderr and goes on (Q-332).
- **Inside `db.transaction`, a dropped error rolls the transaction back**, and the
  transaction gives that error, nested ones too, so a failed `db.exec` no longer answers
  200 with half the work committed. `let _ = db.transaction(...)` for an inner one keeps
  the outer one going (Q-332).
- **`quillo check` warns of a dropped `db.transaction(...)`**, and of a dropped call of the
  program's own function that can fail (`! T`, `err(` or `?` in its body). A builtin's
  dropped error is logged when it happens (Q-333).
- **A builtin given an error says what it was:** `` `json.parse` expects a str as argument
  1, got error (it failed before this call, main.qwe:3: read_file: ...) ``, and
  `int(...)` the same (Q-333).
- **Plugins:** a `pub fn` named like a builtin is warned of at the `.qwer` file's line, with
  `import x.qwer use len as x_len` (Q-326); a `u64` past `i64`'s range inside a
  `serde_json::Value` is an error naming where it is (`.id`, `[1].n`), not a float
  (Q-327); an argument that does not fit (`` `small`: argument `n` does not fit in i32 ``)
  or is nested past 126 levels says so, and an answer nested too deep is no longer a
  false "panicked" (Q-328, Q-329); a `SIGTERM` during a call logs `stopped with GET
  /slow still running`, not a bug in Quillo (Q-330).
- **`file(path, options)` refuses options it does not know**, naming `cache`, `download`
  (a name, sent as `Content-Disposition: attachment`, safe for any script) and `type`
  (Q-331).

### Uploads, translations, dates (FEEDBACK30.md)

- **`save_upload`, `write_file` and `delete_file` refuse the program's own files:** its
  `.qwe` and `.qwer` files (and a folder holding one), `.env` and `.env.*`, the open SQLite
  database with its `-wal`, `-shm` and `-journal`, and `migrations/`. `quillo check` warns
  on `save_upload(f, f.filename)` (Q-361).
- **`{ files: { docs: [map] } }`** takes every file of a field as a list, one a list of one
  and none an empty list (Q-365). A file's and a field's name decode only `%22`, `%0D` and
  `%0A`, and a quoted name keeps its `;` (Q-373). From a page, a file anywhere in a `server
  fn`'s arguments goes as a file, and a `FileList` is a list of files (Q-364).
- **Translations:** `fr_FR.json` is read as `fr-FR`, and two files for one locale are an
  error. A request's locale is matched by script first, with CLDR's likely scripts (`zh-TW`
  gets `zh-Hant`, `zh-CN` and `zh` get `zh-Hans`, `sr` gets `sr-Cyrl`) and CLDR's parent
  regions (`pt-AO` gets `pt-PT`, `es-MX` gets `es-419`, English outside the US `en-GB`)
  (Q-362, Q-363).
- **Plural forms are CLDR's on both sides:** a written `zero` form for 0 in every language;
  Romanian, Lithuanian, Latvian, Slovenian, Icelandic, Macedonian, Irish, Welsh and
  Maltese added; `many` for exact millions in French, Spanish, Italian, Catalan and
  Portuguese; `pt-PT` singular for 1 only; fractions take each language's form, and
  `1.0m` or `"1.5"` count as fractions; a `count` given as text (`"1"`) is taken at its
  number. A form a language never picks is warned of as the translations load (Q-366,
  Q-367).
- **A page's translations are a file the browser keeps**, `/_quillo/t/<hash>.js`, holding
  only the keys its components can ask for (Q-372), and `ask()`'s buttons are in the
  page's locale: the program's `quillo.ok` and `quillo.cancel`, else about thirty built in
  (Q-377).
- **Dates:** `date.compare(a, b)` orders date-times by their instants, however they are
  written; `date.to_zone` drops an all-zero fraction (`.000Z` is `Z`); `date.at` takes `{
  repeated: "earlier" | "later" }` and `{ skipped: "later" | "earlier" | "error" }`, and
  reads `<input type="datetime-local">`'s `2026-10-27T14:30` as one text; `date.parse`
  refuses a date-time with no offset, saying to use `date.at`, and reads an HTTP date as
  UTC (Q-368, Q-375, Q-376).
- **`format_number` rounds a tie away from zero**, from the number's shortest decimal, as
  `Intl.NumberFormat` does (2.5 is 3, 1.005 is 1.01), and `digits` may be `{ min, max }`
  (Q-369).
- **A session keeps decimals and ints past 2^53** as they are, so `session().total + 1m`
  adds (Q-374).
- **In the browser, a place whose expression fails is emptied**, not left at its last good
  value (Q-370), and a `state` that still holds its prop is written in the page once, as a
  reference to the prop (Q-371).

### Components and pages (FEEDBACK28.md, FEEDBACK35.md)

- **`{{ ... }}` blocks of JavaScript in a component** are designed, not built (Q-427).
- **A function in markup given a state keeps its elements** when its markup has the same
  shape and follows no state itself, so an input in it keeps its focus and what was typed
  (FEEDBACK25.md, Q-306).
- **`<script>`'s own text is written as it is**, with only `</` and `<!--` escaped; quotes
  no longer become `&quot;`. A `{` in a script's text says how to write the code instead
  (Q-428).
- **`style={{ color: tone, font_size: "2rem", "--gap": "4px" }}`:** a map of properties, `_`
  and camelCase written as `-`, custom properties kept, `null` and `false` left out, on
  both sides (Q-429).
- **`quillo check` warns on `<p>{{ name }}</p>`**, which shows a map, and on a component's
  `let` set from the page and shown in markup, which makes the browser remake it: write
  `state` (Q-430, Q-431).
- **Handlers pause only where something waits:** a block that waits for nothing, a module's
  function (`date.`, `re.`, `money.`, ...) and a function compiled here that gives no
  Promise are not waited for, so they can go to `map`, `filter` and `reduce`; a Promise
  executor that throws rejects; a replayed `@load` on a server-rendered image runs once,
  on the image (Q-415 to Q-418, Q-420).
- **A prop's call is waited for** (since 0.6), so a callback that only sets state gives the
  child `null`: the guide says how a child learns that its caller finished (Q-419).

### Command line and files (FEEDBACK31.md, FEEDBACK34.md)

- **`quillo new <folder>`** writes a program to start from: the to-do app, or with
  `--minimal` a page, a component and a function both sides use, with a `.gitignore`;
  `--binary` copies `quillo` into `.quillo/bin/`. It refuses a folder that is not empty
  and names Windows refuses, and never leaves a folder half written (Q-422).
- **Logs cannot be forged:** line breaks and control characters in an error's text are
  written as `\n` and `\x1b`, a malformed request line is escaped, and under `--log json`
  is an object of its own. Quillo's notices stay text lines on stderr (Q-378).
- **A script piped into `head`** ends quietly with status 141; a server whose stdout closes
  goes on (Q-379).
- **`.env`:** a byte-order mark is skipped, a quoted value may span lines (PEM keys), and a
  line that is not `KEY=value` is said at the start (Q-380).
- **Options:** a value option followed by another option is refused, `--port` is 0 to
  65535 (0 for any free port), `--tls-cert` and `--tls-key` must exist, all exit 2; `--`
  ends the options before the program; `--instances` past the cores is warned of; a
  misspelt command is told the nearest; `quillo MAIN.QWE` runs (Q-381, Q-389);
  `--host localhost` is 127.0.0.1 (Q-384).
- **`--with` commands stop on every way out**, a Ctrl-C or `SIGTERM` while loading, a
  panic and a `SIGKILL` too (Q-382). **`--watch` and `--reload` follow a symbolic link**
  pointed elsewhere (Q-383).
- **Source files:** a byte-order mark and a `#!` line are skipped, so `./tool.qwe` runs; a
  CRLF is a line break; a file that is not UTF-8 is refused at its first bad byte; an
  invisible character is named by its code. A JSON body with a byte-order mark is read
  (Q-385 to Q-387).
- **The REPL from a pipe** prints only the answers, and a failure is exit 1 (Q-388).
- **`quillo --version` from a source archive** reads `COMMIT`, else says `(commit unknown)`.
- **`db.migrate` on a missing folder** says git keeps no empty folder (Q-423). The Linux
  builds need glibc 2.34, which the README now says (Q-424).
- **`delete_cookie` is `Secure` only over HTTPS**, for a `__Host-` or `__Secure-` name, or
  with `secure: true`, so a cookie can be removed over plain HTTP (Q-425).
- **`platform().server`** is `{ host, port, url, base }`, where the server listens, so a
  program that reads its own pages need not be told its address (Q-426).
- **`./` and `../` in `use` and `import`** count from the file that writes them, so a
  library's file names its neighbour and is checked alone (FEEDBACK25.md, Q-325).
- **`quillo check` warns on a capturing group inside a repeated one** (`(?:(a)|b)*`),
  where the server's and the browser's engines differ (Q-308).

### The web server (FEEDBACK31.md)

- **`local_path(next)`** gives `next` when it is an address on this site as a browser reads
  it, else null, for `redirect(local_path(next) ?? "/")` after a login (Q-390).
- **Pages made from a session or a cookie** get `Cache-Control: private` and `Vary:
  Cookie`, and every HTML page `X-Frame-Options: SAMEORIGIN`, unless the route sets its own
  (Q-391).
- **`site_headers(map)` and `site_headers("/admin", map)`** set headers for every answer
  that does not set them, a `null` taking one of Quillo's away (Q-391).
- **`file()` sees a file replaced by one of the same size and time** (`cp -p`, `touch -t`),
  as its ETag and memory copy include the inode and change time (Q-392).
- **HTTP framing:** two `Host` headers, an HTTP/1.1 request without one, or a bad one are
  400; an absolute-form target gives the host; a method not in capitals is 501; a `HEAD`
  carries the `GET`'s `Content-Length` (Q-393).
- **`normalize(s, form)`**, one Unicode form, on both sides (Q-394).

### Services and the API document (FEEDBACK32.md)

- **`--describe` matches `quillo docs`:** a prefixed service's server fns are no longer
  listed; under `--split` the services' routes stay and `--describe` is passed down, so
  `/billing/_quillo/describe` answers the service's own document (Q-395, Q-397).
- **Several routes on one method and path are one operation**, with a `oneOf` of bodies and
  merged query parameters; a rest parameter is `/files/{path}` with `x-quillo-rest`; the
  title is the program's name, and `--base` gives `servers` (Q-396, Q-398, Q-405).
- **Joined copies take every call in turn**, the app running a call itself only when no
  copy answers; with given addresses, the addresses and the copies are taken in turn
  (Q-399).
- The guide says how services are found (by registration), documents `/_quillo/join`, and
  warns that `/_quillo/describe` is public (Q-400 to Q-405).

### Phone apps (FEEDBACK33.md)

- **`env.NAME = "value"` in `app.toml`** is what `env("NAME")` gives on the phone, and is
  public. What goes into an app is one filter for both systems: no `.env`, dotfiles,
  databases, `uploads/` or a program in a subfolder, and the build says what it left out;
  Android keeps folders starting with `_` (Q-406 to Q-408).
- **An `<iframe>` of another site stays in the page**, with no bridge, and **`site =
  "https://app.example.com"`** in `app.toml` keeps that origin's pages in the web view with
  `window.quilloNative` (Q-409, Q-410).
- **`site shop = "https://..."`** calls a deployed Quillo site's server fns as
  `shop.notes(a, b)`, keeping the site's cookies in an app of one person; server side
  only. `/_quillo/rpc/<fn>` is documented as a wire for programs (Q-412, Q-413).
- **A page's failed `http.*` to another site** says it may be CORS, plain http or
  unreachable, and that a server fn can call it (Q-411).
- iOS asks before reaching a computer on the local network (`NSLocalNetworkUsageDescription`),
  and the guide says the runtime's `http.*` keeps its own certificate roots and may reach
  plain http (Q-414).

### Faster than Python, and a script as a tool (FEEDBACK29.md)

- **Everyday programs run faster than Python's**, in all seventeen that `bench/vs-python/run.py`
  times, from three on 6ec94b4: `s += "x"` a million times went from 19.7 s to 0.04 s, word
  frequency over 1.2 million words from 34 s to 0.20 s, a million pushes from over 40 s to
  0.01 s, a sieve from over 40 s to 0.07 s, a float loop from 6.3 s to 0.04 s. Variables are
  found by a pointer, not by their text; a function with a loop of its own, and a lambda given a
  number, go to machine code on the first call; JSON is read straight into values; and mimalloc
  allocates on every thread.
- **A loop goes to machine code by itself**, in any function or at a script's top, once it has
  run 256 rounds, when it works on numbers and lists of them: a list read costs about 4 ns a
  round where it took 45. An error is the interpreter's own, and an item of another type gives
  the round back to the interpreter. `QUILLO_JIT=0` runs everything in the interpreter.
- **A statement's value no one takes is not made:** a `for` in an `if`'s branch built the list
  of what each round gave, and threw it away.
- **An update is made where it is, however it is written:** `m = m | set(k, v)`, `m = m.set(k,
  v)`, `xs += [x]` and `xs = xs + ys`, at a script's top level too, and what is inside, as in
  `m = set(m, k, get(m, k, []) | push(x))` and `g = set(g, y, set(g[y], x, v))`, a key made with
  `str`, `lower` and the like too, which in a `reduce` of 600,000 went from minutes to 0.1 s. `s += t` adds
  to the text where it is, and `s[i]` reads it in constant time. A second name for the value
  keeps what it had (Q-352 to Q-355).
- **A loop whose list no one keeps makes none**, at the top of a script, in another loop's
  body, anywhere, so it goes past ten million (Q-356). `range(a, b, step)` counts by a step, down
  too.
- **A script can be a command-line tool:** `stdin()` reads what is piped in, `read_line()` a
  line at a time, `exit(n)` ends with a status, and `quillo run --allow-path DIR` lets
  `read_file` and `write_file` use a folder outside the program's. None of the three builtins
  works in a program that serves (Q-357).
- **An error that `?` passes on at the top stops the program**, saying what it was, with status 1,
  from a file it uses too, and a server does not start half made (Q-350).
- **`pow` of ints is exact, or an overflow as `*` gives:** `pow(2, 63)` was a float printed with
  digits that looked exact. A float on either side, or a negative power, still gives a float
  (Q-351). This changes what programs that relied on the float get.
- **Lists compare with `<`, `>`, `<=` and `>=`**, item by item as `sort` has them, and `quillo check`
  takes it (Q-358).
- **Habits from Python and JavaScript are told what to write:** `2 in xs`, `1 < x < 3`, `x ** 2`,
  `xs[1:3]`, `f(c: 9)`, `f(...xs)`, `try`, `class`, `yield`, `throw`, `def` and more (Q-359). The
  guide names the `bit.*` functions and the catalog's `csv`, `xml`, `yaml` and `toml` (Q-360).

### Where Quillo meets the page (FEEDBACK28.md)

- **One rule for what waits:** a call of the page's that gives a Promise is waited for in a handler,
  `@mount` and a state's initializer, `js(...)` as before and now also a method of what the page gave
  (`img.decode()`, `audio.play()`) and a function given as a prop. `.then`, `.catch` and `.finally`
  keep the promise, and so does `js_new`. A plain read through `js` no longer makes the rest of a
  handler late, so `e.preventDefault()` after it works. A function the page calls with `undefined`,
  as `.then` does, is given `null` (Q-335, Q-336, Q-340).
- **`style={...}` and `class={...}` change only what they name**, so what a handler set by hand stays
  and a drag no longer jumps home; `muted` is held to its state, and `<details open>` leaves what a
  person opened alone (Q-337, Q-338). `==` on two of the page's objects asks whether they are the
  same one (Q-334). A keyed row that moves keeps its pointer capture where the browser can move it
  so (Q-343). A row written `a ? <x/> : <y/>` goes when its list empties (Q-342).
- **`run_after(ms, f)`** runs `f` once, unless its component has gone first, **`page_updated()`** waits
  in a handler until the page shows what it set, and `every` may be started from a handler or
  `@mount`, ending with its component (Q-349). **`js_has(name)`** looks for a feature without
  calling it (Q-341), and **`cursor_index(s, n)`** turns a field's cursor into a character's place
  (Q-344).
- **A component keeps all of itself:** a view's `style global` reaches the page when its body is
  only another component, `children` is `null` on both sides when none are given, an image's
  `load` that came first is not lost, and a component called as a function is an error, which
  `quillo check` says too (Q-345).
- **`quillo check` says what fails at once:** what `quillo run` refuses of `js(...)`, a `.bind` that
  will stop a handler, a builtin's answer dropped (`xs.push(3)`), a type name `type_of` never gives,
  and messages say `len(x)` for `.length`, that a miss is `null` and not -1, and to use
  `addEventListener` for `onfinish = f` (Q-347). A parameter or a local named like a global is no
  longer taken for the global, by the check or the browser's compile (Q-348).
- **The phone apps** keep their port from one launch to the next, so `localStorage` keeps what it
  had, open `tel:`, `sms:`, `mailto:` and maps links with the system, and answer `share` alike;
  on Android `brightness(null)` gives the screen back to the system (Q-339, Q-346). Built for both,
  not yet run on a phone.

### An error's text stays off the page

- **An error Quillo made no longer reaches a visitor by being turned into text.** A template
  string, `str`, `join` or `json.stringify` of one, with a database's address or a file's path
  in it, went out with a 200. On a server it now says `internal error` wherever it becomes
  text, and the text goes to the log with the request. `print`, `--watch` and scripts keep it.
- **`filter`, `find`, `any`, `all` and `count` give back an error their function gives.** A failed
  permission check in a `filter` looked like nothing to see. `quillo check` warns on a `?` that
  ends a lambda, where it changes nothing.

### The server and the browser agree

- **Ints stay exact past 2^53 in the browser**, as `*`, `+` and `-` go on with big ints, so an
  amount in cents keeps its units. So do `json.parse` of a large id and `int` of long text.
- **Neither side writes an exponent**, `dec` of a huge float is an error on both, and `float("nan")`
  and `float("inf")` work on both.
- **A map holding the key `constructor` is a map**, not one of the page's objects, so a word count
  no longer stops on that word.
- **A `\r` in an attribute** reads the same from the server's HTML and after an update.
- **`s[i]` in a loop is no longer quadratic in the browser.** Neither is a list's `set` on the
  server: 50,000 went from 3.6 s to 9 ms.

### Guarding against bad input

- **`??` also takes a stop on its left:** `1m / 0m ?? 0m`, `date.of(y, m, d) ?? today`,
  `re.test(s, typed) ?? false`. Without it a stop still ends the request, so a total that
  overflows cannot go on as a value. `date.valid_zone` and `re.valid` ask first.
- **`error(r)`, `r.error` and `r.message`** are null on any value that did not fail, whatever it
  is, where a str or an int stopped the route.

### Components

- **An input keeps what was typed** past a wait in its handler: `e.value` is what it held when
  the event came.
- **`x.n` no longer depends on a local `n`,** and a place inside a row's `cond && <markup>` no longer
  runs after its branch has gone.
- **A local `fn` can call itself** in the browser, as on the server.

### Requests and scripts

- **A list in a pattern takes all of a name's values:** `? { tag: [str] }`, `{ form: { tag: [str] } }`,
  where it matched nothing. `request.query_string` is the query as it came.
- **`quillo run tool.qwe -- a b`** gives a script `args()`.
- **`sort_by` takes a list of keys,** as lists compare item by item.

### `quillo check`

- It warns on `x?.y` in a component, `a && xs | f()`, and a pattern that can hang a browser tab,
  and no longer on `/` in `money.of`, or on a `while true` that returns.
- Clearer messages for `<>`, `{ [k]: 1 }`, `let a = 1, b = 2`, `([a, b]) => ...`, a function calling
  itself in its own `let`, and `session.x = 1`.

### Live data at a thousand pages

Measured with real sockets, 250 pages on the build before and on this one, and 1,000 and
3,000 on this one; on one machine, so these show what changed, not what a server can hold.

- **Behind a proxy, every page gets updates.** `--trust-proxy` names the proxy, and only
  then is `X-Forwarded-For` believed, for `request.ip` and each client's limit of 256.
  `--live-per-client` changes that limit. A page refused says so, the log says so, and it
  waits as it is told, spread out, instead of asking twice a second for ever.
- **A loader waits for a run under way.** On PostgreSQL, 200 pages after 10 writes ran a
  50 ms query 736 times and took 14 s to catch up; now 11 times and 1.8 s. A write
  elsewhere no longer costs a run its sharing.
- **A burst of changes is a few refreshes:** 4,238 calls where there were 10,000, for 40
  writes at 20 a second to 250 pages, and the first change still goes at once.
  `live({ every: "2s" }, load)` sets a gap of its own.
- **A change wakes only the pages following it:** with pages waiting and 200 unrelated
  writes a second, the server's CPU fell from 2.81 s to 0.14 s.
- **A page load shares a run**, as a refresh does, and a whole answer many pages share is
  gzipped once. Values are kept as text, so the 64 MB is 64 MB, patches count towards
  it, and a store past its size keeps most of what fits rather than none.
- **A page that left gives its place back** in about 5 seconds, not up to 40.
- **`--metrics` shows live data:** `quillo_live_pages`, `_refused_total`, `_wakes_total`,
  `_runs_total` and `_answers_total`. Quillo takes all the file descriptors it may at the
  start, and says so if that is too few.

### Forms sent twice

- **Two clicks are one call** on a function marked `idempotent`, however deep it uses it,
  for `@submit` and `@click` alike, with no code of your own.
- **`<form method="post" idempotent>`** does the same with no script: the second click gets
  the first's answer, on SQLite, PostgreSQL and MariaDB, and `{ required: true }` works
  from a form.

### Calls made later, and sagas

- **`.later` goes with its transaction on every database.** On MySQL and MariaDB one call
  inside `db.transaction` used to end the transaction, so its rollback undid nothing. With a
  database per tenant or beside `db.system`, a call rolled back used to be delivered: it is
  now kept in the database the work is in.
- **A service's own `.later` is delivered in one process**, as it is under `--split`, and its
  `db.atomic` work is finished there too.
- **Only a finished call is finished.** A 401, 404, 409 or 500 no longer deletes a call: it
  is tried for about a day, then kept and marked, with `quillo_later_total` counting them. A
  400 is marked at once. A service remembers only answers of 200.
- **`later.status(id)`** says what became of a call: waiting, done with its result, or failed.
- **A long queue drains:** after a full go of 50 calls the next comes at once.
- **A service that dies during a call no longer blocks it for good:** after eleven minutes the
  call runs again. The guide now says what was always so: across a crash a call runs at
  least once.
- **`request.call`** is a call's own id in a service, the same for each try, and `idempotent`
  uses it as its key there, so a step is safe across a crash in one word.
- The guide has a section on [sagas](USERGUIDE.md#sagas).

### Prefetching

- **A page kept for a click no longer outlives a logout.** Only Quillo's own background
  fetch uses the browser's cache, and with `Vary: Cookie`.
- **A request that changes something lets kept pages go**, with `Clear-Site-Data`, in Chrome
  and Edge. A `POST` that reads and a `GET` that counts a visit do not.
- **A prerender runs nothing until it is shown**: no component, `server fn` or live update.
- **Prefetches never go ahead of a visitor:** with anyone waiting, one is refused first.
- **Links in components start with their data:** pointing at one fetches the state of the
  components on the page it leads to, so they show no `null` first.
- **The log marks a prefetch**, and `--metrics` counts those answered, refused and used.

### Databases and tenants

- **A visitor only reads errors the program wrote.** One a builtin made, from a database, an
  `http` call, a file or a conversion, says `internal error` as a route's value, inside a map
  or list, in markup or from a `server fn`, and its text, with the SQL, an address or a path,
  goes to the log. `err(...)` is sent as it is, also from a service, `--watch` shows
  everything, and code on the server still reads it (`db.error_kind`).
- **Every failed request has a log line**, naming the route and the `x-request-id`, and an
  object under `--log json`.
- **`new_tenant(name, f)`** makes a tenant or gives an error when it exists, in one step on
  the database. A second signup cannot land in a customer's data, and the guide's sample
  uses it.
- **A made-up subdomain is a 404** where the route needs its database, and a server that is
  down stays an error. A route that needs no database answers as usual.
- **`?open_tenants=N`** in a server database's URL keeps that many tenants open, past the 16
  at which each request connects again.
- **`quillo check` warns on a global written by a request** in a program with `tenant_from`.
- A commit that fails, in `db.transaction` or a migration, now rolls back, on all three
  databases. A request no longer panics when the default database opens beside a tenant's.
- PostgreSQL: `inet`, `cidr`, `macaddr`, `macaddr8`, `money` and `bit` are read as what they
  are, and types with no reader show as hex. A float past a bigint is refused, not stored as
  the largest bigint, and the smallest interval reads correctly. MariaDB's failed check and
  changed-row errors have a `db.error_kind`. Outside a request, a transaction waiting for a
  connection no longer stops the one holding it.

### One request cannot stop a server

- **`--max-compute 2s`** ends a request that computes that long without letting go, as a 500
  between statements. Waiting does not count.
- **An instance computing for 5 s is logged**, with its request.
- **With `--instances`, connections are taken on a thread of their own** and skip an
  instance that is computing, so one stuck instance holds up only the connections on it.
- **The first `SIGTERM` keeps its promise:** a request still computing after 10 s is named
  and the process exits with status 0.
- `quillo check` warns on a `while` that cannot end.

### Errors that say how they got there

- **A runtime error names the calls it came through**, in the terminal, the log and on the
  page under `--watch`: `in share, called at main.qwe:4`, a line each, and the first and
  last five of a deep recursion. It costs nothing until something fails. A visitor still
  sees `internal error`, and `--log json` has them as `calls`.
- **An error value knows where it was made:** the log line of a request it ended starts
  with the file and line, so a failed `db.*` no longer logs its SQL and nothing else.
  Programs see no difference.
- **In the browser, under `--watch`, a runtime error prints the `.qwe` file and line and
  its calls**, as the server does, with the stack behind it through the source maps. In
  production the console keeps the message alone, and bundles do not grow.

### Commands and start-up

- **`quillo check`, `docs`, `build` and `fix` refuse what they do not know**: a misspelt
  `--strict-null` no longer turns a check off without a word, and a second program is
  refused, not skipped under `ok`.
- **`quillo app` reads `.env` and `--env-file` and runs `--with`**, as `run` does.
- **`--with` commands stop however the program ends**, also when it does not load, has
  nothing to serve or cannot listen.
- **`quillo app.qwe --log=off`** works: a file alone is read as `quillo run` reads it.
- `--port` with nothing after it says it needs a value, not that it is unknown, and an
  option is no longer offered as its own correction. `--with ""` and `--open` under `run`
  say what is wrong.
- `quillo install --list` prints its `Other` heading.
- **Phone apps can start from any file:** `main = "mobile.qwe"` in `app.toml`, which the
  build refuses when it is not a `.qwe` file in the program's folder.

### Servers and files

- **A reload gets its turn on a busy server:** the event loop hands the interpreter lock to
  whoever waits, where it gave it back and took it again before the waiter woke, which held
  a `--reload` off for 15 s and more.
- **A ping on a channel's WebSocket gets a pong**, as RFC 6455 requires, which matters to
  clients and proxies other than browsers.
- **`image.convert` reports a full disk:** the last write was not checked, so a small image
  came out cut off and the program was told it worked. A failed rename no longer leaves a
  temporary file beside the destination.
- **A breakpoint inside a block in markup stops there:** `{for x in xs { print(x) }}` was
  marked as set and ran past it.
- Under `--watch`, a file served before the watcher has looked round is watched too.
- A `file()` cache duration too long to count is refused. `platform().locale` skips an empty
  `LC_ALL`, and reads `LC_MESSAGES` before `LANG`.

### Checks and corrections

- **`quillo check` refuses what a module does not have.** `json.title` and `db.nothing` read
  as null, and now say `` `json` has no `title` ``, with a hint when a route's shape bound
  the name. Under `--no-check` they are null as before.
- **`quillo check` warns on an element that goes nowhere.** Of two elements side by side in
  a body, only the last reaches the page; the first gets a warning, in a component, a view
  or a function.
- **`quillo check` warns on `${name}` in markup**, which shows a `$` before the value.
  `{"$"}{name}` says the `$` is meant.
- A service called through a helper counts as the `http` effect, a local function called as
  a method counts as used, a bare `return` is reported on its own line, and the strings of
  `class={if on { "a" } else { "b" }}` are read.
- **A plugin function with a fixed-size array** (`[u8; 4]`) was dropped in silence. It is now
  reported as not exported, and the functions after it stay.
- **`return` at the top level of a `use`d file leaves that file**, where it ended the whole
  program without a word.
- **A private name beside a typed field** (`private let q`, `route GET /q ? { q: str } = q`)
  no longer reads the variable: `q` is what the request sent.
- **`type Ids = list<int>` may end a line**, and a long chain of `reply` is refused as too
  deep, where it crashed the check.
- **Numbers.** `int(1.0e30)` is an error value, not the largest int, in the browser too.
  `math.fsum` is an error when the sum passes the floats (it gave 0.0) and for `inf` with
  `-inf`, as in Python. `stats.mean` no longer turns inf when only its sum overflows.
  `math.hypot`, `math.dist` and `vec.norm` give NaN for NaN. The smallest int `% -1` is 0,
  `math.lcm` is 0 with a zero anywhere, and `date.add_days` and `date.add_months` no longer
  panic on the smallest int in a debug build.
- **Dates.** A named zone keeps its offset in the last two days of 9999.
- **Server and browser agree on more.** `{true}` is the text true in the browser too, where
  it disappeared. `1e999`, and a top-level constant that is inf, are `inf` in the browser,
  where they were null, and an infinity reads `inf` there, not `Infinity`. A constant map
  with a `__proto__` key keeps it. Under `--no-check`, `{1 | save}` in a component is refused
  as `{save(1)}` is. A live value whose map only changed its keys' order reaches the page.

### Additions

- **A file sees its own names, the builtins and what it takes.** `use lib/forum` takes a
  file's public names, `import lib/forum use latest, Feed` only those, and
  `import lib/forum use latest as newest` under another name. What a file takes is for that
  file, and is not passed on. Two files may each define a `helper`: a file's own comes
  first, and a file that `use`s both whole is told to choose. A `server fn`, a `type` and a
  `service` are still defined once in a program, and types and services are seen everywhere.
  **For now a name a file did not take still works, with a warning**, so a program can move
  a file at a time; a later release makes it an error.
- **The file comes first on a line that takes names.** `import lib/forum use latest` for
  Quillo and Rust files; `client`, `preload` and `preconnect` for what runs in the browser,
  from a CDN or a file of your own: `client /assets/leaflet.js use L`,
  `preload "https://..." use Chart`, `client module "https://esm.sh/three" use * as THREE`.
  A long address may leave its `use` to the next line. Three reasons: the **mental model**
  (the line reads as what kind of code, where, then which names, and its first word says
  which side of the program it is on), the **editor** (it knows the file before the names,
  so it offers them as they are typed), and **readability** (sources line up on the left,
  and names no longer sit after a long address). These words stay names everywhere else.
  0.6's `use a from lib/x` and `use Chart with preload from "..."` still work, with a
  warning; a later release removes them.
- **`quillo fix main.qwe`** writes the `import` lines a program's files need, and turns 0.6's
  lines around. Run it once for each program, a service's or a mounted one's too. A second
  run has nothing to do.
- **`bit` and hex literals.** `bit.and`, `bit.or`, `bit.xor`, `bit.not`, `bit.shl` and
  `bit.shr` work on all 64 bits of an int, with the same answers in the browser, and ints
  may be written `0xFF` or `0xFFFF_0000`. They are functions, as `|` is the pipe. A hex
  literal past 63 bits is an error, as any int that does not fit is.
- **`filled(x)`** gives `x`, or null for `""`: `filled(name) ?? "anonymous"` replaces a
  blank or a missing name, which is what `||` does for text in JavaScript.
- **`quillo check` warns about `x || "text"` and `x || 10` where `x`'s type is not known**,
  as with `request.query.name`, `request.json.title` and a row's fields. These are where a
  blank field or a count of 0 is kept without a word. A warning may appear on a program
  that had none; `filled(x) ?? ...` or `x ?? ...` says which is meant. `x || ""`, `x || 0`
  and `x || []` are left alone.
- **`break` and `continue` say what to write instead**, rather than only that the name is
  unknown. They are still ordinary names.
- **ES modules by name.** `use Chart, registerables with module from "..."` takes a
  module's exports, `use default as dayjs with module from "..."` its default export,
  and `use * as THREE with module from "..."` all of it. They are called as any library
  is (`Chart.new(el, config)`, `THREE.Scene.new()`), and nothing goes on `window`: the
  exports are kept in Quillo's runtime. A page's components start once its modules are
  in, and a `.mjs` file is a module without saying so. A wrong name or a module that does
  not load is an error value where it is used.
- **`use Chart with preload from ...` and `with preconnect`.** A browser library can say
  how a page gets ready for it: `with preload` fetches the script from the page's
  `<head>`, `with preconnect` only opens the connection to its host. Only pages that can
  load the library carry the `<link>`, so nothing is written by hand.
- **`platform()`**: where the program runs, as a map: the operating system and its version,
  the device and its hardware model, simulator or not, the processor, how the program runs
  (a phone app, `quillo app`, a server), and in a phone app its screen, memory, locale and
  the app's version. Server-side, as `env` is.
- **Phone apps, in preview**: a program inside an iOS or Android app, served to the app's
  web view by the runtime linked into it, with `window.quilloNative` for the phone's
  location, haptics, battery, notifications, screen and share sheet; built and run in the
  iOS simulator by `mobile/ios/build.sh` and on an Android emulator or phone by
  `mobile/android/build.sh`. See mobile/README.md; HarmonyOS NEXT is planned there.
  Each program names its app in an `app.toml` (name, id, version, build, icon); the iOS
  script builds for devices and makes an .ipa (signed when given a certificate and
  profile, with the privacy manifest and icon App Store Connect wants), and the Android
  script makes a signed APK and App Bundle targeting Android 16, as Google Play requires.

### Added

- **Browser libraries by name.** `use Chart from "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js"`
  (or a path, `use L from /assets/leaflet.js`) names what a script defines on the page,
  and component code calls it as JavaScript reads: `Chart.new(el, config)`, `L.map(el)`,
  `confetti()`, `Chart.version`. The page loads the script only where a component using
  it is, before the components' code; no `<script>` tag to write. The names are known to
  the checker and the editor; on the server they give null. Calling a class without
  `.new` says how, and a name in the wrong case says which one the page has (`did you
  mean Chart?`), for `js(...)` too. `js` and `js_new` stay, for a page's own scripts.
- **`defer`: slow parts of a page streamed after it.** `state reviews =
  defer(load_reviews(id))` in a component placed in the page: the page is sent at once
  with the state null (its placeholder shown), and the component follows in the same
  answer once the data comes, its other states kept, swapped in by a line of script
  before the browser takes it up. Search engines read the whole page. Inside another
  component, in the browser, for a prefetch or without a server to stream to, `defer(x)`
  is `x` and the page is whole.
- **Query stats at `/_quillo/health/queries`:** the minimum, maximum and average time
  of the last 100 database queries, overall for anyone and per statement with the health
  token (`QUILLO_HEALTH_TOKEN`). Off until switched on: `--watch`, `--query-stats`,
  `QUILLO_QUERY_STATS=1`, or `POST /_quillo/health/queries?on=1` with the token while
  the server runs.

### Fixes

- **`quillo build` and a service's program followed a `use` from the file's own folder**,
  though paths count from the program's folder, so a `use` in `lib/a.qwe` looked in
  `lib/lib/`.
- **A state read from the page** (`state wide = js("window.innerWidth") > 800`) was null for
  good on a page the server rendered: the initializer now runs again in the browser once
  the component is on the page (FEEDBACK18.md, Q-105).
- **A component's `let` functions wait for `js(...)`** as its handlers do, when its markup
  does not call them: `let locate = () => { let p = js("quilloNative.location") ... }` with
  `@click={locate}` gets the place, not a Promise (Q-102).
- **A whole number is a float where data is made into a type**: a `server fn`'s arguments
  from the browser, `Type(json)`, a record's fields and a `list<float>`. JavaScript and
  JSON have one kind of number, so 5.0 came as 5 and a `float` parameter answered 400
  (Q-103).
- **`quillo check`**: `r.error` after `is_err(r)` on a `server fn` called from a handler,
  which fails as a request can, is no longer refused (Q-104); `js(name, ...)` with the
  name in a variable is refused, as loading the program refuses it (Q-100).
- **A load error in a component compiled for the browser** names the component's own file,
  not main.qwe with another file's line (Q-101).
- **Clicks before a page's code comes** are done as they were made. A component's code
  loads when someone reaches for it; a click, a double click, a right or middle click,
  a press (`@pointerdown`, `@mousedown`) and its release made meanwhile were reduced to
  a bare click (no position, no Shift, no click count) or lost. Each is now kept in
  order and made again once the code runs, with its position, buttons, keys, click
  count and pointer type; a canvas gets the point clicked. If the code never comes, a
  kept click or form is done as plain HTML (a link is followed, a form sent).

- **`--watch` loads a program with a large file quickly again.** A file's text went
  into the source map of every function compiled from it; it goes into the first
  one's only, and the others name the file. A program using a 4 MB component kit
  of 1,600 definitions loaded in 7 s under `--watch` and in 0.5 s now, with a
  fifth of the memory (FEEDBACK15, Q-097).
- **A reload under `--watch` is quicker:** the program is compiled for the browser
  once, not twice; files that did not change are not parsed again; whether the program
  uses `db.atomic` is noted as it loads instead of searched for in a printout of it;
  in development the whole program's bundle is made on a thread once the program is
  up (a component that cannot run in the browser is then reported in the log right
  after the load); and the watcher looks every 50 ms. The documentation site reloads in 0.15 s, from 0.5 s
  (Q-098).
- **The watcher is told of changes by the system** on macOS (kqueue) and Linux (inotify)
  instead of looking every 50 ms, so a save, in place or renamed over the file, reloads
  within a few milliseconds of it and an idle server does nothing; the poll stays as the
  fallback and elsewhere (Q-099).
- **`bind:value` with `@input`** (and `bind:checked` with `@change`) on one input: both
  run, the binding first, so the handler reads the state as typed. Before, the one
  written second took the other's place (FEEDBACK14, Q-094).
- **A `for` body in braces ends at its `}`** after `=>` too: `for x in xs => { ... } |
  flatten()` flattens the list, not each round's block (Q-095).
- **`money.of`** refuses text with an exponent (`"1e3"`) and an amount whose minor units
  do not fit an int, as error values: `money.to_minor` of what it gives no longer fails
  at run time (Q-096).

## 0.6.0 (2026-10-29)

### Language

- **`private` and `public`.** `private fn`, `view`, `component` and top-level `let`
  keep a name to its file, so two files may each have a private `helper`; `public`
  says the default and is optional. Another file's use is told the name is private,
  and editors offer it only in its own file. Nothing changes for code that does not
  use them.
- **`use` takes names from plugins.** `use cache_get as redis_get, cache_set from
  redis.qwer` takes only those functions, under the names given; `use * from` takes
  them all, as `use "file"` always did. Two plugins exporting one name are now
  warned of, with the `as` that keeps both.
- **Paths without quotes.** `use lib/auth.qwe`, `use lib/auth` (a `.qwe` file, or a
  `.qwer` one when names are taken) and `use lib\auth` (read as `/`). Quoted paths
  work as before.
- **Route groups.** `routes /admin | auth.required { request pattern } { route ... }`:
  a prefix, middleware and a request pattern for every route inside, nested.
- **`every` at the top of a program** is a tick on the server, on the clock, for game
  loops and polls; a job or tick that never ends stops with the server and on a reload.
- **A component's own CSS:** `style` in its body, scoped to its elements, sent with
  it; `style global` for rules that are not; keyed `for` blocks.
- **Exact decimals and money** from the database to the page, in each currency's
  minor units; idempotent requests and transactions that retry after a conflict.

### Tools

- **`quillo install <plugin>...`**: Rust plugins from quillolang.org/plugins (237 of
  them), several at once, each written only when its SHA-256 and version match the
  catalog; `--example`, `--update`, `--list`.
- **Services:** `--split` runs each service as a process, `--scale` runs copies,
  `--join` adds one to a running app; a service's jobs and channels work in one process
  or split.
- **`--with CMD`** runs a command beside the program (Tailwind's watcher), and
  `style_with` passes components' CSS through PostCSS; `--watch` swaps a changed
  stylesheet in open pages.
- **Editors:** a missing or mistyped end tag is told at its element with a fix;
  classes in markup go to their CSS rules and are completed.
- **`--plain-http`**, and a warning for `Secure` cookies over plain HTTP.
- **`quillo docs` and `--describe`:** the program's API as OpenAPI 3, its routes and its
  server fns' contracts; a service deployed apart refuses, with 409, a call whose
  contract differs; `/_quillo/ready` and `ready()`.

### Security and reliability

- Fuzzing of parsing, checking, requests, JSON, decimals and dates, and what it found.
- Security review fixes: chunked bodies and slow readers, the script sandbox, nesting
  limits, spread attributes, HTTP options, secrets kept out of bundles.
- Audit logs on PostgreSQL and MySQL, sessions that can be ended, two-step sign-in,
  rate limits, security headers and CSP.
- Fixes from FEEDBACK6 to FEEDBACK13: the issues found by building apps in Quillo,
  each with a runnable repro in `feedback/`.
