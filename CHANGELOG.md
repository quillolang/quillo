# Changelog

## Unreleased

### What to change in your code

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
