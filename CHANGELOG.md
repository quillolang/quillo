# Changelog

## Unreleased

### Additions

- **`platform()`**: where the program runs, as a map: the operating system and its version,
  the device and its hardware model, simulator or not, the processor, how the program runs
  (a phone app, `quillo app`, a server), and in a phone app its screen, memory, locale and
  the app's version. Server-side, as `env` is.
- **Phone apps, in progress** (branch `mobile`): a program inside an iOS app, served to the
  app's web view by the runtime linked into it; built and run in the iOS simulator by
  `mobile/ios/build.sh`. See mobile/README.md.

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

### Security and reliability

- Fuzzing of parsing, checking, requests, JSON, decimals and dates, and what it found.
- Security review fixes: chunked bodies and slow readers, the script sandbox, nesting
  limits, spread attributes, HTTP options, secrets kept out of bundles.
- Audit logs on PostgreSQL and MySQL, sessions that can be ended, two-step sign-in,
  rate limits, security headers and CSP.
- Fixes from FEEDBACK6 to FEEDBACK13: the issues found by building apps in Quillo,
  each with a runnable repro in `feedback/`.
