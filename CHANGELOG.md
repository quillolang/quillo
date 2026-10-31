# Changelog

## Unreleased

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

### Security and reliability

- Fuzzing of parsing, checking, requests, JSON, decimals and dates, and what it found.
- Security review fixes: chunked bodies and slow readers, the script sandbox, nesting
  limits, spread attributes, HTTP options, secrets kept out of bundles.
- Audit logs on PostgreSQL and MySQL, sessions that can be ended, two-step sign-in,
  rate limits, security headers and CSP.
- Fixes from FEEDBACK6 to FEEDBACK13: the issues found by building apps in Quillo,
  each with a runnable repro in `feedback/`.
