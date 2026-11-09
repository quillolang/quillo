# Quillo user guide

Everything Quillo does, and how: the language, components, routes, the standard
library, databases, services, and running it. For what Quillo is and why, see the
[README](README.md). For what it does not do, and what to do instead, see [Possible or
not](#possible-or-not).

Quillo is a single binary, built with Cargo:

```sh
cargo build --release
./target/release/quillo new hello                        # a program to start from, in hello/
./target/release/quillo run app/main.qwe --watch        # the sample app: http://127.0.0.1:8080
./target/release/quillo check app.qwe                    # find mistakes without running
./target/release/quillo build app.qwe                    # build its Rust (.qwer files) beforehand
./target/release/quillo repl                             # try code line by line
./target/release/quillo lsp                              # language server (for editors)
./target/release/quillo dap                              # debugger (for editors)
cargo test
```

This guide is for Quillo 0.8.1, and [CHANGELOG.md](CHANGELOG.md) says what it changed.
Features marked *(since 0.7)* came in 0.7.0. On an older release, read that release's guide:
https://github.com/quillolang/quillo/blob/v0.7.0/USERGUIDE.md or
https://github.com/quillolang/quillo/blob/v0.6.0/USERGUIDE.md.

### Options of `quillo run`

`quillo run` checks the program, executes its top level and, if it declared routes,
`server fn`s or tools (`fn mcp`), serves them; a program with none of them ends when its top level has run
(under `--watch` it is served all the same). The program is the first argument that is
not an option, wherever it stands (`quillo run --watch main.qwe` works); left out, or
given a folder, it is that folder's `main.qwe`, so `quillo run` in a program's folder
runs it. `quillo app.qwe` alone is `quillo run app.qwe`.

Options are words starting with `--` that you add to the command line when you start the
program, and they hold for that run only; nothing is written to a file:

```sh
quillo run main.qwe --watch                          # while you work on it
quillo run main.qwe --port 8081 --max-body 50MB      # in production, on another port, larger uploads
```

An option's value follows it, or `=` (`--port 8081`, `--port=8081`); `-w` is the one
short option (`-p` is not `--port`); `-h` prints the usage; a mistyped option is told the
nearest one there is. `quillo app`, `quillo serve` and `quillo fcgi` take these options
too, but for a few each refuses ([below](#other-commands)). The options, and when to give them:

| Option | What it does, and when to give it |
|---|---|
| `--port N`, `--host H` | where to listen (default `$PORT` or 8080, on 127.0.0.1). Give `--port` when 8080 is taken or a host names the port; `--port 0` takes any free one (`platform().server.port` says which). Give `--host 0.0.0.0` to be reached from other machines without a proxy in front; `--host localhost` is 127.0.0.1 |
| `--listen unix:PATH` *(since 0.7)* | listen on a Unix socket instead of a TCP port, when a web server on the same machine passes requests on (on a shared server, so the neighbours cannot reach the port): made with mode 0600, removed when the server stops, and at most 100 bytes long (a short path relative to the folder `quillo` runs in). A `$PORT` that holds an absolute path (`/home/u/app.sock`, or `unix:/home/u/app.sock`) is taken the same way. Not with `--split`, `--instances`, `--register` or `--join` (see [Quillo on a shared server](#quillo-on-a-shared-server-cpanel)) |
| `--socket-mode 660` *(since 0.7)* | with `--listen`: the socket's permissions, in octal, when the web server reaches it through the owner's group; the owner must keep read and write (`600`, the default) |
| `--watch`, `-w` | while you work on the program: reload when it or a `use`d file changes (a symbolic link pointed elsewhere, to a file or a folder, counts, within a second), show errors on the page, and refresh open pages; a file the program served with `file()` that changes refreshes them too, and a stylesheet is swapped in them as they are, keeping what they show (for one another tool writes, such as Tailwind's). Started, or reloaded, while a file is broken or missing, it watches every file that load read or tried and the folders of those not there, so fixing or making one loads the program; each error is said once. Not in production |
| `--reload` | in production, for a program whose files change while it runs (a deploy that copies new files, a plugin editor): reload when files change, with no errors shown on pages and no page refreshes; with `--instances`, each instance loads the change, the first checking it, so one that does not check reaches none; `--split` passes it to its services |
| `--env-file F` | when the settings are not in the `.env` beside the program: a file kept outside the site's folder, or one file per place (`--env-file prod.env`); a path from the folder `quillo` runs in, which must exist |
| `--policy FILE` *(since 0.7)* | before a deploy, or on a server you share: narrow what the program may do, after `/etc/quillo/policy.toml` and before the `quillo.policy` beside it; may be given more than once (see [The policy](#the-policy)) |
| `--no-build` *(since 0.7)* | on a server with no Rust, or one that must never run cargo: load only libraries `quillo build` made, as `.quillo/plugins.lock` names them (see [Rust](#rust)) |
| `--no-check` | when the program is known to check (a deploy that ran `quillo check` already) and starting must be quick: skip the static checks |
| `--max-body N` | when visitors send larger files or bodies than 10MB: the largest request body to accept, a size such as `50MB` (`KB`, `MB` and `GB`, in any case, are 1024-based) or bytes; default 10MB; a larger body gets 413 (see [Uploads](#uploads)) |
| `--max-compute 2s` *(since 0.7)* | when a request that computes for long must not hold up the others: end one that computes this long without letting go, as a 500 (see [Concurrency](#concurrency-and-reloading)) |
| `--drain 30s` | when a stop must not wait for ever: cut off what is still running this long after the stop is asked for (500ms, 30s, 2m). Without it, a stop waits for every request in progress, a large file being sent too, however long it takes (see [Stopping and health](#stopping-and-health)); for `run` and `mcp` |
| `--max-tool-calls 60` | how many tool calls a minute each user of the MCP door may make (60 by default); one more is a `429` with `Retry-After` (see [Tools for AI clients](#tools-for-ai-clients)) |
| `--workers N` | requests that can wait at once (for a database, the network), each on a thread (default 64). Give fewer where an account limits its threads (shared hosting), more for many slow calls at once |
| `--instances N` | to use N cores on one machine: run the program N times over, one event loop each, each with its own variables (see [Concurrency](#concurrency-and-reloading)); not with `--watch`, `--register` or `--split`; more than the machine's cores is warned of |
| `--trust-proxy 10.0.0.0/8` *(since 0.7)* | when a proxy (limer, nginx, Apache) is in front: believe `X-Forwarded-For` from these addresses, so `request.ip` and the limits per client are the visitor's (see [Live data](#live-data)) |
| `--live-per-client N` *(since 0.7)* | when one address is many visitors (an office behind one address): the pages one client may have waiting for live data (default 256) |
| `--plain-http` | when the site is served over plain HTTP on purpose, on a network you trust (a factory's kiosks at `http://10.0.0.5:8080`): cookies are not `Secure`, which browsers would drop there (see [Cookies and sessions](#cookies-and-sessions)) |
| `--allow-path DIR` *(since 0.7)* | when a script used as a tool must read or write files outside its folder, or a deploy links a folder from elsewhere (`uploads -> /srv/shared/uploads`): `read_file` and `write_file` may use `DIR` too; any part of the program may then, routes too (see [Paths and links](#paths-and-links)) |
| `-- a b` *(since 0.7)* | for a script: after the program, what follows is the program's, and `args()` gives `["a", "b"]`; before it, `--` ends the options (`quillo run -- a.qwe x`). A stray word elsewhere is told it goes after `--` (see [Scripts as tools](#scripts-as-tools)) |
| `--with CMD` | when a tool must run beside the program while you work (Tailwind's watcher): run a command (through `sh -c`, in its folder), its lines marked with its name, stopped with it however it ends, a Ctrl-C or `SIGTERM` while it loads, a panic and a `SIGKILL` too (more than once for several): `--with "npx tailwindcss -i src/app.css -o public/app.css --watch"` |
| `--log F` | the request log on stderr: `text` (the default), `json` (one object per line) for a log collector, or `off` when another layer logs requests (see [Logs and metrics](#logs-and-metrics)) |
| `--metrics` | when Prometheus (or another scraper) should read the server: answer `GET /_quillo/metrics` (see [Logs and metrics](#logs-and-metrics)) |
| `--query-stats` *(since 0.7)* | to find slow queries on a running server: time database queries for `GET /_quillo/health/queries`, the last 100, overall and per statement (on under `--watch`; see [Stopping and health](#stopping-and-health)) |
| `--describe` | when a gateway imports the program's API: answer `GET /_quillo/describe` with it, as `quillo docs` writes it (see [The program's API](#the-programs-api-openapi)) |
| `--no-view-cache` | to measure, or to rule the cache out while looking for a bug: render views every time (see [Cached views](#cached-views)) |
| `--no-jobs` | on all but one of several servers that do not share a database, or on a copy that must not run them: run none of the program's scheduled jobs here (see [Scheduled jobs](#scheduled-jobs)) |
| `--no-prefetch` | when links must not be fetched before they are clicked (to measure, or a site whose `GET`s are counted): turn prefetching off (see [Prefetching](#prefetching)) |
| `--base PATH` | when a proxy serves the program under a path (`/shop`) and takes the path off before passing a request on: the program sees `/x` for `/shop/x`, and its links and redirects carry the prefix (`/y` is written `/shop/y`) |
| `--split` | to try the app as separate processes, as it will run deployed: run each `service` as a process of its own (see [Services](#services)); with `--port 0`, the services are told the port the app was given |
| `--scale NAME=N` | with `--split`, for a service that needs more than one process: run service NAME as N copies (1 to 64), called in turn (see [Services](#services)) |
| `--service-url NAME=URL` | when service NAME runs on another machine: where it is (also `QUILLO_SERVICE_<NAME>`, the name in capitals); several addresses, comma-separated, are taken in turn (see [Services](#services)) |
| `--as-service` | to run a service on a machine of its own: serve the program as a service, its `server fn`s answering other services' signed calls, not pages (see [Services](#services)) |
| `--service-name NAME` | with `--as-service`, when the file is not named as callers call the service: the name it answers to; calls signed for another service are refused |
| `--join URL` | with `--as-service` and a `--port`, to add a copy of a service while the app runs: join the app at URL, signed with `QUILLO_SERVICE_KEY` when it is set (see [Copies that join](#copies-that-join)) |
| `--join-as URL` *(since 0.7)* | with `--join`, when the app must call the copy at another address than the one it connects from (`https://billing-3.internal:8443`); it must lead to where the copy joined from |
| `--no-join`, `--allow-join A,B` *(since 0.7)* | on the app, to take no copies that join, or only copies of these services |
| `--register URL`, `--service S` | to run several servers behind limer: join its pool at its admin address (the pool named `S`, else the file's name), and share live updates with its other servers; needs `LIMER_ADMIN_SECRET` (see [Several servers](#several-servers)) |
| `--wt-port N` | when the UDP port of the HTTP port's number is taken or not open: the UDP port pages connect to over WebTransport (see [Channels](#channels)) |
| `--tls-cert F`, `--tls-key F` | when WebTransport should present a real certificate, instead of one of its own; the files must exist, go together and be a pair (`--tls-cert b.pem and --tls-key a.key are no pair: the key is not the certificate's`); given twice, the last wins |
| `--no-wt` | when UDP is blocked between pages and the server: no WebTransport, so pages' channels and streams use a WebSocket at once |

### Other commands

`quillo new <folder>` *(since 0.7)* writes a program to start from. By default it is the to-do app, the
eight files of `app/`: a database with a migration, a component, files served.
`--minimal` writes three: `main.qwe`, `lib/greet.qwe` and `ui/page.qwe`, a page, a
component and a function both sides use. Both get a `.gitignore` (`*.db`, `*.db-wal`,
`*.db-shm`, `.env`, `.env.*`, `.quillo/`), and `--binary` copies this `quillo` into
`<folder>/.quillo/bin/quillo`. It refuses a folder that is not empty (an empty one, or one
holding only `.git`, is used) and a name Windows refuses (`con`, `aux`, `nul`, `prn`,
`com1` to `com9`, `lpt1` to `lpt9`, any of `<>:"|?*\`, a trailing dot or space, a line
break). A new folder is written beside the target and moved in place at the end, so a
failed `new` leaves nothing half made, and it never writes an empty folder; an empty
folder that is there is built inside, so its parent need not be writable (a leftover of a
`new` whose process is gone is removed). It warns when
the folder is inside another program, and prints the next step:

```sh
quillo new hello --minimal
cd hello && quillo run --watch
```

In VS Code, *Quillo: New program* (extension 0.14.0) asks for the to-do app or the minimal
one and for a folder, runs `quillo new` and opens the folder.

The other commands: `quillo check` (below, and [Checking](#checking)), `quillo docs`
(the API as OpenAPI 3, to stdout; see [Services](#services)), `quillo build` (the Rust
files; see [Rust](#rust)), `quillo fix` *(since 0.7)* (the `import` lines a program's files need, and 0.6's lines turned around; see
[Files of a program](#files-of-a-program)), `quillo serve <folder>` *(since 0.7)* (a folder of files as
they are, as a static host serves it; see [Static files](#static-files)), `quillo classes
<file.qwe>` *(since 0.7)* (the classes the program's markup writes, one a line; see [HTML
literals](#html-literals)), `quillo fcgi <file.qwe>` *(since 0.7)* (the program behind FastCGI, for
`mod_fcgid` or nginx's `fastcgi_pass`; see [Quillo on a shared
server](#quillo-on-a-shared-server-cpanel)), `quillo repl`, `quillo lsp`, `quillo dap`,
`quillo version` (or `--version`, `-V`), which names the commit: from a source archive
it reads the `COMMIT` file `git archive` writes, else says `(commit unknown)`. `quillo
<file.qwe>` is `quillo run <file.qwe>`, options and all, whatever the case of its
extension (`quillo MAIN.QWE`). A misspelt command is told the nearest (``unknown command
`chek` (did you mean `check`?)``), and a file of another extension given with no command is
told how to run it (`to run the file: quillo run tool`). An option given before the
command is told where it goes, in one line: ``quillo: put the command first: `quillo run
--watch main.qwe` ``. `-h` and `--help` are read first, after any command (`quillo run -h`
with no program, `quillo lsp --help`), and print to stdout. `quillo run` given a file that is
not a program says what it is and how to serve it: ``page.html is HTML, not a Quillo
program: serve its folder as it is with `quillo serve .` ``, and the same for Markdown
(with `markdown_page`), Rust (`use "t.rs"`), and `.css`, `.js`, `.json`, `.txt`, `.xml`,
`.csv` and `.svg` files (`file()`); a folder with no `main.qwe` but an `index.html` is
told `quillo serve`. `check`, `docs`, `build` and `fix` take one program, and refuse a second and an option
they do not have (`check` has `--strict-null` and `--policy FILE`). `quillo check` exits 1 when there is an error (warnings do not
fail it), `quillo run` 1 when the program does not load or the port cannot be taken, and
either 2 for a usage mistake (an unknown option, an option without its value, no program, `--register` without its
secret, options that do not go together, a `--port` outside 0 to 65535, a `--tls-cert` or
`--tls-key` file that is not there, a policy file that cannot be read or says something
a policy does not say, a `.env` that cannot be read). An option that takes a value refuses another option
in its place: `--service-name --watch` says `needs a value, and --watch is an option`. A stopped server exits 0 (see [Stopping and
health](#stopping-and-health)).

Their options, given after the command as `quillo run`'s are:

| Command and option | What it does, and when to give it |
|---|---|
| `quillo check --strict-null` | to find where a value may be null and null does not work (see [Checking](#checking)); `check` only, not `run` |
| `quillo check --policy FILE` *(since 0.7)* | before a deploy to a server with a policy: check the program as it would run under it (see [The policy](#the-policy)) |
| `quillo new --minimal`, `--binary` | three files instead of the to-do app; a copy of this `quillo` in the new folder, for a machine that has none |
| `quillo app --open PATH` | the page the window opens on (`/` by default); `app` takes `run`'s options but `--host`, `--register`, `--service`, `--port` and `--listen`: its window asks the program directly, with no socket (see [Desktop apps](#desktop-apps)); nor `--drain`: it stops with its window |
| `quillo serve <folder>` | `run`'s options, such as `--port` (see [Static files](#static-files)) |
| `quillo fcgi <file.qwe>` | `run`'s options but `--split`, `--instances`, `--register` and `--join`; `--listen unix:PATH` or `--port` when no FastCGI manager hands it a socket (see [Quillo on a shared server](#quillo-on-a-shared-server-cpanel)) |
| `quillo install --example`, `--update`, `--list [word]`, `--force`, `--dir DIR`, `--registry URL` | a plugin's example program too; the plugins here the catalog has newer; what the catalog has; replace a file changed by hand; install elsewhere; another catalog (see [Plugins from the catalog](#plugins-from-the-catalog)) |
| `quillo repl -i` | keep the greeting and prompts when the input comes from a pipe (see [The REPL](#the-repl)) |
| `quillo mcp <file.qwe>` | serve the program's tools to an AI client over stdin and stdout (see [Tools for AI clients](#tools-for-ai-clients)); takes `--env-file`, `--policy`, `--allow-path`, `--no-build`, `--drain` and `-- args`, and refuses the options of serving |

`quillo build`, `fix` and `classes` take a program and no options; `quillo docs --mcp` prints what an AI client is told, `server/discover` on stdio and over HTTP and `tools/list`, as JSON (a program with no tools is refused).

### Settings and the environment

The `.env` beside the program is read into its environment as it starts: `KEY=value`
lines (`export` allowed, `#` for comments, and ` #` ends a bare value, so `KEY= # set me`
is `""` while `KEY=#fff` is `#fff`; values in `"..."`
take `\n`, `\t`, `\"` and `\\`, and `'...'` is as written; a quoted value goes on to its
closing quote across lines, as a PEM key does, and a quote never closed skips its own line
only; a byte-order mark at the start is skipped;
a key is letters, digits and `_`, and a line that is not `KEY=value`, or whose value holds
a NUL byte, is skipped and said
at the start: `quillo: .env:3: not KEY=value, skipped`), for `env("API_KEY")` and for Quillo's own
settings (`PORT`, `QUILLO_SECRET`). Only a `.env` that is not there (or a folder of that
name) is nothing: one that cannot be read, is not UTF-8, or is a link to nothing stops
the start with exit 2, saying where and the ways out (``quillo: cannot read .env: it is not
UTF-8 text: byte 0xE9 at line 1, column 9 (save it as UTF-8); to start without it, remove
it or give `--env-file /dev/null` ``), as a missing `--env-file` does, and a service's own
`.env` stops its load the same way. A variable the environment already has wins, so a
`.env` left on a server cannot override what production sets. It is read once: a program
started with [`--reload`](#options-of-quillo-run) does not read it again when its files change. Another file is
named when the program starts: `quillo run main.qwe --env-file /etc/shop/prod.env`. A service reads the `.env` beside its own file as
well, in one process and under [`--split`](#options-of-quillo-run) alike: its values come before the app's file's
(the app's reach it too), and after the environment's. Keep `.env` out of version
control.

Everything Quillo reads from the environment:

| Variable | |
|---|---|
| `PORT` | the port, when [`--port`](#options-of-quillo-run) is not given; an absolute path is a Unix socket to listen on, as [`--listen unix:`](#options-of-quillo-run) |
| `QUILLO_SECRET` | signs sessions (see [Cookies and sessions](#cookies-and-sessions)); without it, a key made at start |
| `QUILLO_QUERY_STATS` *(since 0.7)* | `1`: query stats on from the start, as [`--query-stats`](#options-of-quillo-run) |
| `QUILLO_HEALTH_TOKEN` *(since 0.7)* | lets a request with `Authorization: Bearer <it>` see each statement's query stats and switch them on or off; 16 characters at least |
| `QUILLO_DB` | the database when the program does not call `db.open`; without it, SQLite in memory, gone at restart |
| `QUILLO_SYSTEM_DB` | where Quillo's own tables go with a database per tenant (see [Quillo's own tables](#quillos-own-tables)) |
| `QUILLO_SERVICE_SECRET`, `QUILLO_SERVICE_<NAME>` | signs calls between services (several, comma-separated: the first signs, any is taken); where service NAME runs (see [Services](#services)) |
| `QUILLO_SERVICE_SECRET_PREVIOUS` *(since 0.7)* | a secret still taken while `QUILLO_SERVICE_SECRET` is changed (see [Authentication between services](#authentication-between-services)) |
| `QUILLO_SERVICE_KEY` *(since 0.7)* | a service's own key: a copy joins with it, and it proves the copy is that service (see [Copies that join](#copies-that-join)) |
| `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` *(since 0.7)* | the proxy `http.*` calls go through, and the hosts that do not (see [HTTP client](#http-client)) |
| `LIMER_ADMIN_SECRET` | for [`--register`](#options-of-quillo-run) (see [Several servers](#several-servers)) |
| `QUILLO_NATIVE_CACHE`, `CARGO` | where `.qwer` files are built, and with what (see [Rust](#rust)) |
| `PUBLIC_...` | any such variable may be read in a component (see [What the browser can reach](#what-the-browser-can-reach)) |
| `QUILLO_CHROME`, `QUILLO_TEST_PG_BIN`, `QUILLO_TEST_MYSQL_BIN`, `QUILLO_TEST_LOG`, `LIMER` | `cargo test`: where Chrome, PostgreSQL, MariaDB and limer are; show the test servers' output |

The one other configuration is the policy, which only takes away: `/etc/quillo/policy.toml`,
[`--policy FILE`](#options-of-quillo-run) and `quillo.policy` beside the program (see [The policy](#the-policy)).
`HOST` is not read ([`--host`](#options-of-quillo-run) is), and `RUST_LOG`
and `NO_COLOR` do nothing. The program's paths (`use`, `db.open`, `read_file`, `.env`,
`quillo.policy`, `quillo-system.db`, `.quillo/native`) count from the program file's folder, whatever
folder `quillo` was started in.

### Desktop apps

`quillo app` runs a program in a native window instead of a browser tab: the
platform's own web view (WebKit on macOS and Linux, WebView2 on Windows) shows
its pages, and closing the window ends the program. It is built only on request:

```sh
cargo build --release --features app      # Linux also needs libwebkit2gtk-4.1-dev and libgtk-3-dev
./target/release/quillo app app/main.qwe --watch
```

The release downloads are built without it. It takes the [options of `run`](#options-of-quillo-run) except
[`--host`](#options-of-quillo-run), [`--register`](#options-of-quillo-run) and [`--service`](#options-of-quillo-run), reads the `.env` beside the program (or
[`--env-file`](#options-of-quillo-run)) as `run` does, keeps [`--with`](#options-of-quillo-run) commands going until the window closes, and
takes [`--open PATH`](#other-commands), the page the window opens on
(`quillo app main.qwe --open /admin`; `/` by default). The program runs in the window's process with no
HTTP server: nothing listens, and nothing else on the machine can reach it. The window
asks for its pages through a scheme of its own, `quillo://app/` (`https://quillo.localhost/`
on Windows), and the program answers each request as a server would, so routes,
sessions, `server fn`s, uploads and live data work as in a browser (Phone apps, below,
says what differs). The origin is the same at every launch, so what a page keeps in
`localStorage` and IndexedDB is there at the next one. The web view
keeps no cookies for the scheme, so the program's are kept for it in
`.quillo/app-cookies.json` beside the program, and its session key in
`.quillo/.quillo-secret` unless `QUILLO_SECRET` is set: someone signed in stays signed in
from one launch to the next. [`--port`](#options-of-quillo-run) and [`--listen`](#options-of-quillo-run) are refused, as there is no socket to give them to. Links to other sites open in the
system's browser. Only macOS has run this way; Windows and Linux take the same code
through wry, not yet run.

Editor support: [quillolang/editors](https://github.com/quillolang/editors) has a VS Code extension (highlighting,
diagnostics as you type, quick fixes, hover, go to definition, completion, outline, snippets,
end tags added as you type a start tag's `>` or `</`, and a debugger: see [Debugging](#debugging)),
which Cursor, Windsurf and VSCodium take as its `.vsix` (or from Open VSX, once a release is
published there). The same repository has a plugin for the JetBrains IDEs and Android Studio,
an extension for Zed, and support for Neovim, Helix, Emacs, Sublime Text and Vim, with Eclipse
and Visual Studio set up by hand. All but Visual Studio run `quillo lsp`, and all but Visual
Studio, Sublime Text and Vim `quillo dap`. Zed, Neovim, Helix and Emacs highlight
with tree-sitter-quillo, a tree-sitter grammar in the same repository; documentation sites can
use its Shiki, highlight.js and Prism definitions.
An element left open is reported at its start tag (`` `<li>` is never closed: add `</li>` before
`</ul>` on line 5 ``), with a quick fix that adds the end tag there. A class in markup
(`<span.chip>`, `class="chip"`) goes to its rules with Go to Definition, in `css(...)`, a
component's `style` or the CSS files under `public/`, and hover shows them; those classes
are offered after `<span.` and in `class="`. Find All References lists a class's rules and
the markup that writes it, from either side, and Rename renames it in both (a class put
together, `card-${size}`, is left as it is). A class no sheet has a rule for is marked: a
warning when a rule's class is close to it (`` no style sheet has a rule for `.chiip` (did
you mean `chip`?) ``, which `quillo check` says too), else a faint hint, as markup often
carries classes for its own sake. Neither is told when the program has no sheets of its
own or links one from another site. On a `use` line, the path goes to its file and
completes to the program's files, and the names taken from a `.qwer` file go to their
Rust functions, show their signatures (under the name after `as`) and complete to the
file's functions. Inside a call, as `(` and `,` are typed and with Ctrl+Space, it shows the
parameters, the current one marked, and a line about the function: a builtin's own, a `.qwer`
function's `///` comment, or the `//` comment above your function. Completion offers the
variables in scope at the cursor first, and `mcp` after `fn `, `server fn ` and `service `;
hover on that tag says what a tool is, and the outline shows tools. It runs the `quillo` it finds (its `quillo.path`
setting, the workspace's `target/release`, then the `PATH`), so stale errors on code the
build accepts come from an old binary: `quillo --version` names the commit, and
`scripts/doctor.sh` points out an old one. Other editors can run `quillo lsp` (hover,
go to definition (of names, and of classes to their CSS rules), references and rename
of classes (also from the CSS files under `public/`, when the client sends them), document symbols,
completion after `.`, `<`, `</` and `@` (and of classes after `<tag.` and in `class="`), diagnostics on
open, change and save, quick fixes as code actions, and `quillo/closeTag`: at a position
just after a start tag's `>` or after `</`, the text to insert there, or null; `initializationOptions: { "strictNull": true }` for the extra
warnings) and `quillo dap` directly. The language server reads the editor's text, not the
file on disk, and says diagnostics under every URI a file is open by, so a project opened
through a link gets them; it reads headers in any case (`content-length`), answers
`-32002` to a request before `initialize`, `-32600` after `shutdown`, and `-32700` to a
body that is not JSON.

Examples, in [quillolang/examples](https://github.com/quillolang/examples) (cloned as `../quillo-examples` in the commands here): [hello.qwe](https://github.com/quillolang/examples/blob/main/hello.qwe) (routes), [orders.qwe](https://github.com/quillolang/examples/blob/main/orders.qwe)
(payload-based routing and middleware), [todo.qwe](https://github.com/quillolang/examples/blob/main/todo.qwe) (server-rendered
app with a JSON API), [notes.qwe](https://github.com/quillolang/examples/blob/main/notes.qwe) (SQLite and a live component),
[scroll.qwe](https://github.com/quillolang/examples/blob/main/scroll.qwe) (infinite scroll over 100,000 rows, rendering only a window),
[compose.qwe](https://github.com/quillolang/examples/blob/main/compose.qwe) (views and components built from each other),
[layout.qwe](https://github.com/quillolang/examples/blob/main/layout.qwe) (a page described as JSON, with a live editor),
[spa.qwe](https://github.com/quillolang/examples/blob/main/spa.qwe) (a single-page app: client-side navigation, deep links, 404s),
[life.qwe](https://github.com/quillolang/examples/blob/main/life.qwe) (Conway's Game of Life, run by a timer in the browser),
[storm.qwe](https://github.com/quillolang/examples/blob/main/storm.qwe) (a dashboard under a storm of random changes, tested for leaks),
[services](https://github.com/quillolang/examples/tree/main/services) (a shop that bills through a billing service, in one process or
split apart), and [index.qwe](https://github.com/quillolang/examples/blob/main/index.qwe), which mounts all of them in one server.

### Phone apps (preview)

*(since 0.7)* A program runs inside an iOS or Android app: the runtime is linked into it and runs the
program in the app's process, with no HTTP server, as `quillo app` does on a desktop, so it
works offline and keeps its SQLite database in the app's container. `mobile/ios/build.sh app
--run` builds the app around a program folder for the iOS simulator and opens it there, and
`mobile/android/build.sh app --run` a debug APK for an Android emulator or phone. The
app's name, id, version, build number, icon and first file (`main = "mobile.qwe"`, by
default main.qwe) come from an `app.toml` in the program's folder;
`--device` and `--archive` (iOS) and `--release` (Android, a signed APK and App Bundle)
make builds for the stores;
`mobile/README.md` says how, and what a phone build leaves out (compiled numbers,
WebTransport, `.qwer` plugins built at run time, anything that starts a process). The
app puts `window.quilloNative` on the program's pages for what a web view cannot reach:
`js("quilloNative.location")`, `.haptic`, `.battery`, `.notify`, `.keepAwake`,
`.brightness`, `.share`, `.openSettings`, `.wait`. Both apps answer `.share` as `{ shared,
to }` (on Android both `null`, as it cannot tell), on Android `.brightness(null)` gives the
screen back to the system (iOS does when the app leaves the screen), and a `tel:`, `sms:`, `mailto:` or maps link opens what the phone opens it with
(FEEDBACK28.md, Q-346). It has run on a real iPhone, served on 127.0.0.1 as it was then;
the scheme below has run on the iOS simulator and the Android emulator (pages, a server fn,
live data, a form with a file, and storage and a session kept from one launch to the next),
not yet on a real phone. Uploads to the stores are not
done yet, and HarmonyOS NEXT is planned (`mobile/harmony/PLAN.md`). `platform()` (see [Standard library](#standard-library)) tells a program
it is in a phone app, and on what. `mobile/examples/fieldbook` tries the phone's hardware
from a program.

Nothing listens. The web view gets its pages from the program through a scheme the app
answers: `quillo://app/` on iOS (and in `quillo app` on macOS and Linux),
`https://quillo.localhost/` on Android (and on Windows). So no other app on the phone, and
no page in its browser, can reach the program; iOS has no listening socket to take back
while the app is suspended; and the apps carry no exception for plain HTTP (iOS's
local-networking ATS key and Android's cleartext config are gone). The origin is the same
at every launch, so `localStorage` and IndexedDB keep what they had. An install made
before kept them under `http://127.0.0.1:<port>`, and they stay there: they are not copied
to the new origin (the program's database is the program's, and is not touched).

What a scheme cannot carry goes through the app's message bridge: on Android every request
with a body (its interceptor is given none), on WebKit a body that is a file, a `Blob` or a
`FormData` (it hands the scheme none of those, so the page turns them into bytes first), and
everywhere the connections live data and channels keep open. The shell's script, run before
every page, hands the runtime its way out when the runtime asks for it with an event, so
nothing is put on `window`. On Android, where every body goes through the bridge, it also
takes over the page's own `fetch`, so a script's own request with a body goes the same way:
the one global it changes. A form the scheme cannot carry (every post on
Android, a form with a file on WebKit) is sent through the bridge, and the page goes to its
answer, as after a post and a redirect. What the program sees is what a server sees, but:

- The cookie jar is the app's. The web view keeps no cookies for a custom scheme, so the
  program's are kept in `.quillo/cookies.json` in the app's data, with a session key
  beside them, and a session lasts from one launch to the next. It keeps cookies as a
  browser does, by name, path and domain (the longer path sent first), and one set with
  no `Path` at the folder of the request that set it. `document.cookie` is
  empty to the page's scripts.
- The app goes only to `http(s)` addresses and its own pages: a `location` of another
  scheme is not followed.
- `request.ip` is `"app"`.
- Jobs, ticks (`every` on the server) and calls made `later` reach the program with no
  connection to itself.
- A `stream` route's answer arrives whole, when it ends: the shells hand an answer over in
  one piece.
- Service workers do not run on a custom scheme. Quillo uses none; a program's own will
  not start.
- Safari's and Chrome's inspectors work as before, at `quillo://app/...` or
  `https://quillo.localhost/...`.

`mobile/README.md` lists the calls an app makes into the runtime (`quillo_open`,
`quillo_request`, `quillo_bridge` and the rest).

## Language tour

### Values

| Kind | Written |
|---|---|
| Null and booleans | `null`, `true`, `false` |
| Numbers | `42`, `3.14`, `1e-9`, `6.02e23` |
| Decimals | `12.50m`, `5m`: exact, for money (see [Decimals](#decimals)) |
| Strings | `"strings"` or `'strings'`: plain text, so JSON is easy to write: `'{"a": 1}'`. Escapes: `\n \t \r \0 \\ \" \' \` \$ \{ \}`, and `\u00e9` or `\u{1F600}` for any character |
| Template strings | `` `Hello, ${name}` ``: the only strings that interpolate; they can span lines |
| Lists | `[1, 2, 3]` |
| Maps | `{ name: "Ada", "content-type": "text/plain" }`; `{ id }` is short for `{ id: id }` |
| Others | functions, HTML and error values |

| Expression | Gives |
|---|---|
| `xs[0]` | the first item; `first(xs)` says the same in words |
| `xs[-1]` | the last item (`last(xs)`): negative indexes count from the end |
| `xs[99]`, a missing map key, a field of `null` | `null` |
| `"héllo"[1]` | `"é"`: strings index by character |
| `"abc".foo`, `5.foo`, `xs[1.5]` | runtime errors: only maps have fields, and lists index by int |

A name right before a template calls it with the text: `` shout`hi ${name}` `` is
`` shout(`hi ${name}`) ``. The builtins `css` and `sql` return the text as it is, and
tell editors to highlight it as CSS or SQL.

Operators are as in JavaScript, with these meanings:

| Operator | Means |
|---|---|
| `+` | numbers add; a string with anything is text (`"n=" + 1` is `"n=1"`, `"a" + null` is `"anull"`); two lists join; two maps merge, the right one's values winning; `true + 1` and `null + 1` are errors |
| `- * %` | as in JavaScript on numbers; nothing else is converted (`"5" * 2` is an error) |
| `/` | exact division: `7 / 2` is `3.5`, and a whole result stays an integer (`6 / 2` is `3`); `floor(7 / 2)` is integer division, done in ints past 2^53 too: `floor`, `ceil`, `round`, `math.floor`, `math.ceil` and `math.trunc` of an int divided by an int, written in the call (`floor(a / b)`, `a / b \| floor`, `(a / b).floor()`, `math.floor(a / b)`, a builtin held in a variable), give the exact int, as Python's `a // b` does; a quotient kept in a variable first (`let q = a / b`) is a float, and its digits past 2^53 are gone; `math` has no `round` (write `round(a / b)`), on both sides |
| `== !=` | deep equality, like comparing JSON: `1 == 1.0` and `0.1m == 0.1`, but `1 == "1"` is false (nothing is converted); an int and a float compare exactly, as in Python (`9007199254740993 == 9007199254740992.0` is false), in compiled code and in the browser too; functions are equal only to themselves |
| `< <= > >=` | comparison of two numbers (ints, floats and decimals mix, exactly; not a decimal and a float), or two strings by code point; a string and a number, booleans, or NaN is an error |
| `!`, `&&`, `\|\|` | not, and, or; `a && b` and `a \|\| b` give back one of their operands, so `{loading && <Spinner/>}` works |
| `??` | the right side when the left is `null` or an error value |
| `cond ? a : b` | `a` if `cond` is truthy, else `b` |
| `a..b` | the ints from `a` up to, not including, `b` (both ints; at most ten million) |

| Falsy | Truthy |
|---|---|
| `null`, `false` and error values | everything else, `0` and `""` included |

Because `""` is truthy, `name || "anonymous"` keeps an empty name: write
`filled(name) ?? "anonymous"` *(since 0.7)* to replace a blank or a missing name, or
`name ?? "anonymous"` to replace only null (`quillo check` points these out).

Ints are 64-bit: `+`, `-`, `*` and unary `-` past their range are a runtime error
(`integer overflow`), never a wrap. The smallest, `-9223372036854775808`, can be written,
in a pattern too; `-` comes after a call, so a method on it takes brackets,
`(-9223372036854775808).str()`, as `quillo check` says. An int divided by int 0, or `%` 0, is an error;
a float divided by zero is `inf` or `NaN`, written so, and ordering either is an error.
There is no `**`, no bitwise `& | ^ << >>`, no `++`, no `*=` or `/=` (only `=`, `+=`,
`-=`), no `?.`, no `===`, no `in`; `pow(a, b)` and `math.*` do the arithmetic, and
[`bit.*`](#numbers-and-data) the bits (`|` is the pipe).
Comparisons do not chain: `1 < x < 3` is a syntax error.

From loosest to tightest: `? :`, `??`, `|` (a pipe), `||`, `&&`, comparisons, `..`,
`+ -`, `* / %`, unary `-` and `!`, then calls, `.field`, `[index]` and `?`. So a pipe
binds looser than everything but `??` and `? :`: `xs | find(f) != null` is `xs |
(find(f) != null)`, and `(xs | find(f)) != null` is what was meant; `xs | len ?? 0` is
`(xs | len) ?? 0`. `??` and pipes group to the left.

### Coming from JavaScript

Most expressions read as they would in JavaScript. The differences:

- `let` declares a variable (there is no `const` or `var`); values are immutable, so
  `push`, `set` and `+` return new lists and maps, and `m.name = v` is a runtime error
  outside a component (there it sets an element's property; see [JavaScript
  libraries](#javascript-libraries)). Spread works in literals as in JavaScript:
  `{ ...m, name: v }`, `[...xs, 3]`; `...null` spreads nothing.
- Named functions are `fn name(params) = expr` or `fn name(params) { ... }`; anonymous
  ones are arrows, which may declare a return type too (`(x: int): int => x * 2`).
  Blocks give back their last expression, so `return` is only for leaving early (at the
  top level of a `use`d file it leaves that file). Of two elements side by side, the
  first goes nowhere and `quillo check` warns: wrap them in one element. A call
  gives every parameter without a default, and no more: `f(1, 2)` for a one-parameter
  `f` is an error, as is a missing one, and so is a builtin given more than it takes
  (`split(s, ",", 2)`). A default is worked out at each call, in order,
  and may use the parameters before it (`fn page(n, size = n * 10)`).
- `==` compares values deeply (`[1, 2] == [1, 2]`); there is no `===`.
- A map keeps its keys in the order they were set, where a JavaScript object lists keys
  like `"1"` first: `keys({ b: 1, "1": 2 })` is `["b", "1"]`, in the browser too.
- There are no semicolons to forget (`;` does separate statements on one line): a line
  break ends a statement unless the next line continues it (`|`, `.method`, `&&`,
  `||`, `??`, `?`, `:`, `else`, `*`, `%`, `==`, `!=`, or `- b` and `+ b` with a
  space), or the line before ends with something that needs more (`+`, `=`, `,`), or
  the break is inside `(...)` or `[...]` (not `{...}`). A line starting `-b` is a new
  statement; `quillo check` warns when one does nothing. A `/* */` comment over several
  lines ends a statement as a line break does.
- Names may use any letter (`let café = 1`). Reserved, so not a name anywhere: `let fn
  view route use return if else match for in as reply component while true false null`
  (and the old `then and or not nil`, refused with a hint). `with type state service
  mount server style` and the type names (`int`, `str`, ...) are words only where they
  mean something, so `let state = 1` and `let int = 5` are allowed (see [Files of a
  program](#files-of-a-program) for what hiding a builtin costs).
- Numbers are written `42`, `1_000_000`, `3.14`, `2.5e-3`, `6E+23`, `0xFF`,
  `0xFFFF_0000`: no octal or binary, no `.5`. A `_` goes between two digits (`1__000` and
  `1_` are errors), a number does not start with `0` before more digits (`08` and `010`
  are errors: write `8`, `10`), nor end with `.` (`1.`: write `1` or `1.0`), and there is
  no `--` (`--n` is an error: `n -= 1`, or `-(-n)`). An int past 64 bits is a parse error
  (`0x8000000000000000` too: a hex literal is an int, and does not wrap to a negative
  one); a float past `f64` is `inf`.
- Only `null`, `false` and error values are falsy: `0` and `""` are truthy. Code
  written by JavaScript habit then runs without failing but means something else:
  `name || "anonymous"` keeps an empty name, and `count && <Badge/>` shows the badge
  for 0. Where a value is known to be a string or a number, the checker warns about
  `||`, `&&`, `!`, `if` and `? :` on it and suggests the explicit form
  (`name != "" ? name : "anonymous"`, `count != 0 && <Badge/>`). It warns too where only
  the fallback says so, as with what comes from a request or a row: `request.query.name
  || "anonymous"`, `row.n || 10`. `filled(x) ?? "anonymous"` replaces `null` and `""`;
  `x || ""`, `x || 0` and `x || []` are left alone, as they mean the same in both
  languages.
- `match`, `|`, patterns, `route`, `reply`, `state` and `with` effects have no
  JavaScript equivalent.
- There is no `break` or `continue` (each is an ordinary name, and an error that says
  this where nothing defines it): `return` leaves the function, from a loop too, and
  from a `for … =>` that makes a list; to skip items, `filter` first. `return` is
  also an expression, like `reply`: `let m = find(xs) ?? return null`, `"a" => return 1`.
- Arguments are worked out eagerly, left to right, before the call; only `&&`, `||`,
  `??`, `? :`, `if` and `match` skip what they do not need. A lambda sees the variables
  around it as they are now, not as they were when it was made, and may assign them
  (`count += 1` inside a handler changes the outer `count`). An assignment may be an
  arrow's whole body, as in JavaScript: `() => count += 1` is `() => { count += 1 }`, and
  gives null. A handler is a function, so `@click={count += 1}` is an error that says so. A block that declares a
  name makes a scope: `let x` inside `{ }` hides the outer `x` until the block ends.
  `fn` inside a function is a local closure. There is no tail-call optimisation: 10,000
  calls deep is the limit, in a tail position or not.
- There are no classes, interfaces, enums (a `"a" | "b"` type, checked when the program
  runs, is the nearest), generics (beyond `list<T>` and map shapes), decorators or
  exceptions. A file sees its own names and what it takes with `use`, without a prefix:
  `import lib/auth use login`, then `login()`, not `auth.login()` (see [Files of a
  program](#files-of-a-program)). `mount` and `service` are the boundaries.
- Regular expressions are strings given to `re.*` (`re.find_all(text, "@(\\w+)")`), not
  `/.../` literals, and take only the syntax both sides read alike (see the standard
  library).

The built-in functions are free functions taking the list, map or string first (so
`xs | map(f)` reads left to right), and some differ from JavaScript's methods:

| JavaScript | Quillo |
|---|---|
| `xs.map(f)`, `xs.filter(f)`, `xs.find(f)` | `map(xs, f)`, `filter(xs, f)`, `find(xs, f)` (`null` when nothing matches) |
| `xs.reduce(f, init)` | `reduce(xs, init, f)`: the start value comes before the function |
| `Math.max(a, b)` | `max([a, b])`: `max`, `min` and `sum` take one list; `max([])` is `null` |
| `s.replaceAll(a, b)` | `replace(s, a, b)` replaces every occurrence |
| `s.indexOf(x)` | `index_of(s, x)`: `null`, not `-1`, when it is not there |
| `parseInt(s)` | `int(s)`: an error value, not `NaN`, for text that is not a number |
| `s.length` | `len(s)` counts characters: `len("é")` and `len("😀")` are 1 |
| `xs.sort((a, b) => a.n - b.n)` | `sort_by(xs, (x) => x.n)`; `sort(xs)` sorts numbers as numbers |
| `x.toFixed(2)` | `round(x, 2)` gives a number |
| `s.match(/re/g)`, `s.replace(/re/g, ...)` | `re.find_all(s, "re")`, `re.replace(s, "re", ...)`; `re.test`, `re.find`, `re.split` |
| `encodeURIComponent(s)` | `url_encode(s)`, which also encodes `!'()*`; `url_encode(map)` gives a query string |
- Text in HTML follows JSX, but `class` (not `className`), and a handler is `@click={...}`
  or `onclick={...}`, which on an element mean the same.

### Variables, functions, views

```quillo
let todos = []
todos = todos | push("x")        // reassign with =, += or -=

fn add(a: int, b: int = 10) -> int = a + b
fn area(w, h) {                   // block body: the last expression is the result
    let a = w * h
    a
}
let double = (x) => x * 2          // arrow functions, as in JavaScript
let triple = x => x * 3
let point = (x: int, y: int = 0) => { x, y }      // `{ x, y }` here is a map,
let same = (x) => { x }                           // but `{ x }` alone is a block

view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>
```

Type annotations are checked when the function is called and when it returns, and
`quillo check` checks them before the program runs: `int float dec num str date bool list map
html fn response any` (`num` is `int | float`, not `dec`). Unions are written `int | str`; add `?` or `| null` to also allow null
(`str?`, `int | null`). `list<int>` says what a list holds, and `{ title: str, tags:
list<str>, note: str? }` a map's fields; `quillo check` holds the program to them (a
call with a map missing `title`, a list of strings for `list<int>`), and at run time
they are `list` and `map`. A capitalised name in an annotation is a record's (below); one
that no `type` declares is not reported by `quillo check`, and fails at the first call.
A function with a capital first letter (`fn Card()`) is taken for a view. A list's item
type goes in `<>` in a map's or a record's fields too: `quillo check` says so for
`tags: list[str]?`, with the fix `` `tags: list<str>?` `` (FEEDBACK56.md, Q-861).

`///` lines right above a function, a `server fn` or a component are its description: the
editor shows them on hover, and the program keeps them, so `quillo docs` writes them as the
function's `description` ([The program's API](#the-programs-api-openapi)). A `//` line is a
comment only, `////` is not a description, a `//` line among `///` lines is skipped, and a
blank line ends one (FEEDBACK56.md, Q-854).

```quillo
/// Adds a note and gives back all notes.
server fn add_note(text: str) -> list with db {
    db.exec("insert into notes (text) values (?)", text)
    db.query("select text from notes")
}
```

```
fn titles(posts: list<{ id: int, title: str }>) = posts | map((p) => p.title)
```

### Records

`type` names a shape, so it can be used by name, checked when the program runs, and
made from what a client sends:

```quillo
type Address = { city: str, zip: str? }
type User = {
    name: str,
    email: str,
    role: "admin" | "member" = "member",    // one of these strings; a default
    address: Address?,
    tags: list<str> = [],
}
type Id = int | str                         // another name for a type

let ada = User({ name: "Ada", email: "ada@example.com" })
// { name: "Ada", email: "ada@example.com", role: "member", tags: [] }

fn greet(u: User) -> str = `Hi ${u.name}`

route POST /users { json: User } = save(json)    // the body, made a User; else no match
route POST /users = respond(422, "not a user")

match v {
    User as u => u.role
    _ => "not a user"
}
```

- A record is a map: JSON, the browser and every map function see it as one. What its
  name adds is checking.
- `User(fields)` makes one: each field is checked, records in it are made the same way
  (and lists of them, item by item), missing fields get their defaults (made afresh
  for each record), and fields the record does not have are left out, so what a client
  sends cannot add any (`is_staff: true` in a body does not get through). It gives an
  error value saying what is wrong, as `int("x")` does: `User({ name: "Ada" })` is
  `err("User: `email` is missing")`, and a deeper mistake names its place
  (`` `address.city` is missing ``, `` `tags[1]` must be str, got int 2 ``). So
  `User(request.json)?` passes it on, and `?? reply 400` answers it.
- In an annotation, a record is checked field by field when the function is called or
  returns: `greet({ name: "Ada" })` fails with `` argument `u` of `greet` must be User,
  got map { name: "Ada" } (`email` is missing) ``. A field with a default must be
  there too; `User(...)` is what puts it in. A `server fn`'s arguments from the browser
  (or another service) are made as `User(v)` makes them: checked the same way, with
  defaults put in and fields the record does not have left out, so a page cannot add
  `is_admin` to the profile it saves.
- In a pattern, a record matches what `User(v)` would make, and binds that: in a route,
  `{ json: User }` takes the body as a User, and `{ form: Signup }` a form, its text
  converted to the numbers and booleans the record asks for, as query values are.
- `quillo check` holds code to the fields: `u.nmae` (did you mean `name`?), a field
  `User` does not have in `User({ ... })`, a missing field or one of the wrong type,
  and a default that is not of its field's type.
- A type is named like `User` or `OrderLine` (a capital letter first, and small ones),
  which keeps it apart from constants such as `MAX` or `DAY`; `type ID = int` is
  refused. A record may name itself (`type Node = { value: int, kids: list<Node> = [] }`).
  `type`, like `service` and `mount`, is written at the top level. A field that is
  there but `null` counts as missing: its default fills it, or it is reported.
- `date` is text that is a calendar date (`YYYY-MM-DD`, a real day): `type Leave = {
  start: date, end: date }` refuses `"2026-02-30"` and a date-time, in a record, an
  annotation (`fn next(d: date)`), a pattern (`match x { date => ... }`, a query's `?
  { from: date }`) and a `server fn`'s arguments from a page, so the `date` module's
  functions, which fail on anything else, are safe on what got through. It is a `str` to
  `quillo check` and to JSON (OpenAPI's `format: date`); as a variable's name, `date`
  still hides the module.
- Strings in a type (`"admin" | "member"`) are checked when the program runs; `quillo
  check` takes them as `str`.

### Pipelines and method calls

`a | f(b)` is `f(a, b)`. `x.f(y)` calls `f(x, y)` when `x` has no field `f`,
and `x.len` is `len(x)`:

```quillo
orders
    | filter((o) => o.total > 100)
    | sort_by((o) => o.total)
    | map((o) => o.id)

orders.filter(o => o.paid).map(o => o.id)
[3, 1, 2].sort().first()
```

A line starting with `|`, `??`, `&&`, `||`, `?`, `:`, `.method` or `else` continues
the previous line.

### Control flow

```quillo
n > 0 ? "pos" : "neg"
if n > 0 { "pos" } else if n == 0 { "zero" } else { "neg" }   // also a value
for x in [1, 2, 3] => x * x                 // a list: [1, 4, 9]
for [key, value] in { a: 1 } => `${key}=${value}`
while i < n { i += 1 }                      // null; `return` leaves it early
match value { ... }
return x
```

`while` runs its block for as long as the condition holds (anything but `null`, `false`
or an error, as with `if`). Numeric functions with `while` loops are compiled to machine
code like those with `for` (see [Compiled numbers](#compiled-numbers)).

Everything is an expression: `if` without `else` is `null` when the condition fails,
`while` is `null`, `for` is the list of its body's values (with a block body too:
`for i in 0..3 { i * i }` is `[0, 1, 4]`), and `match` is its arm's. A body in braces
ends at its `}`, after `=>` too, so what follows takes the list: `for n in ns => { ...
} | flatten()` flattens the list of every round's values. `for` runs over a
list, a map (as `[key, value]` pairs), a string (character by character), a range (not
made into a list first) or `null` (no rounds); anything else is an error. A `match`
that no arm takes is a runtime error, and so is a `let` pattern that does not match.

### Patterns

The same patterns work in `match`, `let`, `for`, and route declarations:

| Pattern | Matches |
|---|---|
| `_`, `name` | anything (`name` binds it) |
| `42`, `"order"`, `true`, `null` | that literal |
| `int`, `str`, `list`, ... | any value of that type (`dec` also takes an int, numeric text or a JSON number, made a decimal; `date` takes text that is a date, `"2026-11-20"`, not a date-time) |
| `"order" \| "refund"` | either alternative |
| `int as n` | binds what matched |
| `[first, second, ..rest]` | lists |
| `{ type: "user", name, customer.id: id, limit: int = 20 }` | maps (extra keys are allowed); `a.b` looks inside nested maps; `= default` fills a missing field |
| `User`, `User as u` | what `User(v)` makes of it (see [Records](#records)) |

In a map pattern, `key: int` (a bare type) also binds `key`, and `= default` fills a
field that is `null` as well as one that is missing. A name written like a type
(`Item`, `Max`: a capital letter, then a small one) is always a type test, never a
binding: `let Max = 1` fails, and `match x { Foo => ... }` with no type `Foo` never
matches. In `"a" | "b"` alternatives, a name bound in one alternative only is unbound
when the other matched.

A name binds once in a pattern: `[x, x]`, `{ p: a, q: a }` and `let [y, y] = ...` are
errors, in `quillo check` and as the program runs (`` `x` is bound twice in this pattern:
to test that two parts are equal, use `[x, x2] if x == x2` ``), where they matched
anything and kept the last part. Each alternative of `|` binds its own. A tested field
binds its last name by itself, so `{ user.id: int, order.id: int }` may test two ids;
but using `id` there is an error, since two fields bind it: name the one you mean,
`user.id: int as user_id`. A route that binds one name by two of its path, its query
pattern and its request pattern (`/a/:id ? { id: str }`) is an error too.

```quillo
match event {
    { type: "click", x: int, y: int } => `click at ${x},${y}`
    { type: "key", key: "Enter" | "Return" } => "enter"
    int as n if n < 0 => "negative"
    _ => "other"
}
```

### Errors

`err(x)` makes an error value. Builtins such as `int("abc")` and `json.parse`
return errors instead of crashing. `?` after an expression returns the error from
the current function, and `??` falls back when the left side is null or an error, or
could not be worked out: `1m / 0m ?? 0m`, `date.of(y, m, d) ?? today`,
`re.test(s, typed) ?? false`, `v.foo ?? "none"` for a `v` that is text, `xs[i] ?? 0` for
an `i` that is not an int, in the browser as on the server. Without `??`, such a stop ends the request with a 500, so
a total that overflows cannot go on as a value and reach the database unseen; with it,
you have said what to do. `is_err(r)` asks whether a value failed and `error(r)` (or
`r.error`, `r.message`) what it says, and on any other value that is null. Field access
on null gives null (so there is no need for `?.`):

```quillo
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err(`bad age: ${s}`)
    }
fn next_year(s) = parse_age(s)? + 1         // err("bad age: x") for "x"
let age = int(request.query.age) ?? 18
```

At the top of a program, or of a file it uses, an error that `?` passes on has no one to
take it: the program stops, saying what it was, with status 1, and a server does not start
half made (FEEDBACK29.md, Q-350).

An error value a statement gives and nothing takes, as a call alone on a line whose value
is an error, is not lost: the log says where it was dropped, where it was made and the
request it was in, and the program goes on.

```
main.qwe:10: an error no statement took, made at main.qwe:6: odd (pass it on with ?, give a value with ??, or take it with let _ =)
```

In a script it goes to stderr, and the script goes on. `f()?` passes it on, `f() ?? x`
gives a value in its place, and `let _ = f()` takes it and says it is not wanted. Inside
`db.transaction(() => ...)`, a dropped error rolls the transaction back (see
[Database](#database)). `quillo check` warns when a call of one of the program's own
functions that can fail is dropped: one that declares `! T`, or whose body calls `err(`
or uses `?`. A builtin's dropped error (`db.exec(...)` alone on a line) is not warned of;
it is logged when it happens (FEEDBACK27.md, Q-332, Q-333).

A builtin given an error where it wants something else says what the error was:

```
json.parse expects a str as argument 1, got error (it failed before this call, main.qwe:3: read_file: does-not-exist.json: No such file or directory)
```

`int(r)` of an error says `cannot convert error to int (...)` with the same reason.

`?` leaves the function it is written in, and in a lambda that is the lambda:
`map(xs, (x) => f(x)?)` gives a list with the errors in it and goes on, and `quillo
check` says so. To stop at the first failure, use a `for` loop, where `?` leaves the
function around it. `filter`, `find`, `any`, `all` and `count` give back an error their
function gives, since it has said neither yes nor no, so `filter(xs, f)?` stops there;
`map` keeps each result, an error too. An error is never made into text where a value
is wanted: `get`, `has`, `set` and `remove` given one as the key, `group_by` and
`re.replace` given one by their function, and `url_encode` given one or a value holding
one at any depth (a list, a map in a list) give that error back, the first in order, where
its text became a key, a part of the text or of the URL. In a component's body, `?` and `return` are ways
out of it, so a body with one is made again whole on each change, which loses the focus
of an input; `x?.y` is such a `?`, and `quillo check` says so. A field of null is null, so
write `x.y`.

`! T` in a signature declares what the function's errors contain. It is checked: a
function declared `! str` that returns `err(42)` is a runtime error, and `quillo check`
reports it, and a `?` that would pass on the wrong kind of error, before the program runs.

Mistakes in the code itself (calling a non-function, adding a map to an int, a
failed type check, recursion more than 10,000 calls deep) are runtime errors. In a
route they become a 500 response, and the server logs them.

A runtime error says where it happened and how it got there, in the terminal, the log and,
under [`--watch`](#options-of-quillo-run) (the option you start a program with while working on it,
`quillo run main.qwe --watch`), on the page. A visitor sees only `internal error`:

```
quillo: price.qwe:2: division by zero
  in share, called at main.qwe:4
  in split_bill, called at main.qwe:7
```

A deep recursion keeps its first and last five calls. Under [`--log json`](#options-of-quillo-run) the calls are a
`calls` field beside `error`. The chain is built as the error leaves each call, so calls
that succeed pay nothing, and a function in machine code gives the same chain, as what
fails there runs again in the interpreter. An error value is not a failure yet and has no chain, but
the log says where one was made when it ends a request: `main.qwe:8: db: no such column:
nope`. A program does not see that place: two errors with the same contents are equal
wherever they were made.

In a page under `--watch`, the browser's console says the same, from the error's stack:
the `.qwe` file and line, and the calls. In production it says the message alone.
The browser hides a builtin's error in production as the server does (see [What a route
returns](#what-a-route-returns)): shown as text, in a template, in markup or an attribute,
or written by `json.stringify`, it is `internal error`, so a component made again in the
browser shows no more than the server's page did; so are a builtin's errors in attribute
JSON and in channel messages. Errors the program wrote, a `server fn`'s errors (which the
server has judged already) and the page's own JavaScript's are shown as they are.

### Effects

`with` declares what a function may do besides computing a value:

```quillo
fn load_user(id: int) -> map? with db = db.one("select * from users where id = ?", id)
fn fetch_avatar(url: str) with http = http.get(url).body
fn profile(id: int) -> map with db, http = { user: load_user(id), avatar: fetch_avatar(...) }
```

The effects, and the builtins behind each:

| Effect | Builtins |
|---|---|
| `db` | every `db.*` but `db.error_kind` and `db.sql_name`, and the same on a connection from `db.connect` (`analytics.query`); `idempotent`, `later.status` |
| `http` | `http.*`, and calls to services |
| `fs` | `file`, `files`, `read_file`, `write_file`, `delete_file`, `save_upload`, `check_program`, `image.info`, `image.convert` |
| `env` | `env`, `args`, `stdin`, `read_line`, `exit` |
| `time` | `now`, `sleep`, `date.today`, `date.now` |
| `random` | `random`, `random_int`, `uuid`, `crypto.token`, `crypto.hash_password` (its salt) |
| `net` | `udp.listen`, `udp.send`, `udp.reply`, `media.rtp`, `channel.send`, `channel.broadcast` |

Inside a `with` function, a builtin that needs another effect,
or a function declared with more effects, is an error, both at runtime and in
`quillo check` (which also warns when an undeclared helper may use one). The limit holds
for everything the function calls, including the lambdas it passes to `map`: the
effects allowed are narrowed on the way in, and nothing called can widen them.
Functions without `with` may do anything.

Routes and route groups take `with` too *(since 0.7)*, written before the body, after the patterns. It
holds for everything the route runs: its body, what it calls, and the middleware of its
groups. A group's `with` is what its routes may use at most; a route in it may narrow it,
and one that asks for more is an error:

```quillo
route GET /report/:day with db = db.query("select * from orders where day = ?", day)
route POST /notes ? { text: str } with db {
    db.exec("insert into notes (text) values (?)", text)
    redirect("/notes")
}

routes /admin | signed_in with db, time {
    route GET /users = list_users()                  // db and time, as the group allows
    route GET /status with time = str(now())         // narrower: no database here
}
```

A read-only page declared `with db` cannot post to another site by mistake, and a check
in its middleware is held to the same: `` `http.post` needs the http effect, but `route
GET /report/:day` is declared `with db` ``. A route without `with` may do anything, as
a function without it may. A job is a function given to `schedule`, so it takes `with`
as a function does (see [Scheduled jobs](#scheduled-jobs)).

- **A connection's calls** need `db` as `db`'s do: `analytics.query(...)` in a `with
  http` function is an error. On a parameter with no type, `c.query(...)` (or `.one`,
  `.exec`, `.transaction`) is a warning, since `c` may be a connection: `` `c.query` reads
  like a database call, which needs the db effect ``.
- **A Rust function** (a `.qwer` or `.rs` file) has the effects its file declares in
  its header, `//! effects: db, http` *(since 0.7)* (or `//! effects: none`). A file without that line
  may do anything, so its functions run outside any `with` as before, and are refused
  inside one, at run time and by `quillo check`: `` `hi` is Rust whose file does not say
  what it does, so it may do anything and is refused in `greet` ``. An editor's hover
  shows `(effects: not declared)` for them. Quillo cannot see inside the Rust: the line
  is the file's word, not a proof (see [Rust](#rust)).
- **The policy** sets a `with` on the whole program from outside it: its `[effects]
  allow` is what every route, job and function may use at most, and a Rust file with no
  `//! effects:` line is refused anywhere under it (see [The policy](#the-policy)).

```quillo noexec
use "store.qwer"            // //! effects: fs
use "hi.qwer"               // no effects line

fn keep(text: str) with fs = store_put(text)        // allowed: store.qwer says fs
fn stamp() with time = store_put(str(now()))        // error: store_put needs fs
fn greet() with time = hi("Ana")                    // error: hi.qwer says nothing
print(hi("Ana"))                                    // outside any `with`: runs
```

### HTML literals

HTML is a value, not a string. It is written like JSX, with no `<>` fragments: text goes in as it is,
expressions go in `{...}`, and everything is escaped unless it is already HTML:

```quillo
<li.todo #todo-{t.id} class={t.done && "done"} data-id={t.id}>
    {t.done ? <s>{t.text}</s> : t.text}
    <button disabled={!editable}>Edit</button>
    Added {t.age} days ago &mdash; {t.owner}
</li>
```

- Text follows JSX's rules: each line is trimmed, blank lines are dropped and the rest
  are joined with a space. So text and an element on lines of their own touch, as in JSX
  (`Hello,` then `<b>{name}</b>` on the next line is "Hello,Ann"), and `quillo check` says
  so for an inline element or a view *(since 0.7)*: write `{" "}` at the end of the line.
  In a `<pre>`, a `<textarea>` and a `<listing>`, text keeps its lines and spaces as HTML
  has them, and a `//` line in it is text *(since 0.7)*; only the line break right after the
  start tag is dropped, and spaces alone across lines, between its tags, as code laid out
  to read has them. Every named entity HTML has (`&amp;`, `&mdash;`, `&rarr;`, `&times;`,
  `&rsquo;`) and numeric ones (`&#8594;`, `&#x2192;`) work, in text and in attribute values
  *(since 0.7: ten names, and none in attributes)*; a name HTML has not stays as written. Write `{"{"}`
  for a brace. A `$` is text too, so `${name}` shows a `$` before the value, and
  `quillo check` asks: write `{name}`, or `{"$"}{name}` for a price. Braces in text are
  code, so a `return` in them leaves the function: `<pre>f() { return 1 }</pre>` answers
  `1`, not the page, and `quillo check` warns (`` `{ return ... }` in <pre> leaves the
  function, and its value is the answer, not the page ``).
- `.class` and `#id` shorthand, with `{}` interpolation in the name. A class may hold
  `:` before a letter or digit and `/` before a digit, as Tailwind's do:
  `<em.hover:underline>`, `<div.md:flex.w-1/2>`. On a view or component's tag they are its
  `class` and `id` parameters *(since 0.7: they were dropped)*, so `<Card.big #main/>` needs
  a `Card(class, id)`, and one without is told so. There are no member tags:
  `<Card.Header/>` is an error that says to import `Header` by its own name.
- `{name}` is short for `name={name}`, and `{...map}` spreads a map's keys as attributes
  (or as a view's props): `<a {...link}>`, `<Card {...node.props}/>`. On an element they
  follow the rules of the same keys written out *(since 0.7)*: a key `onClick` whose value
  is a function is a handler, as `onClick={f}` is (`"@click"` too), `className` is `class`
  and `htmlFor` is `for`, and booleans and style numbers are written as below. Attributes written
  out win over spread ones, whatever the case of the key (`ACTION` is `action`); classes
  from both are kept. On a view or component, spread
  keys that are not parameters are ignored, so data with extra fields can be passed
  straight through; a misspelled attribute written out is still an error.
  A spread never writes an attribute Quillo's runtime or loader reads: `data-qi`,
  `data-qd`, `data-qt`, `data-c`, `data-p`, `data-s`, `data-l`, `data-v`, `data-e`, and
  names starting `data-q-` or `data-quillo-`, in any case, are left out, on the server and
  in the browser, so a visitor's JSON spread on an element cannot make it a component of
  the visitor's choosing. The marks of an island are the server's alone: `data-qi`,
  `data-qd` and `data-qt` written out, and a `<quillo-island>` tag, are errors when the
  program loads and in `quillo check` (`` `data-qi` in <div> is Quillo's own attribute ``).
  The other names written out are the program's, but on a component whose root is its own
  island (a table's row, below), where the island's attributes are the server's.
- Attributes: `"text"` (plain, as in JSX) or `{expr}`, for example
  ``href={`/todos/${id}`}``. `"text"` is plain as HTML's is: a backslash is a backslash
  (`title="C:\new"`, `pattern="\d{3}"`) and entities are decoded (`href="?a=1&amp;b=2"`)
  *(since 0.7: `\n` and `\t` were escapes)*. So a `\"` would end it: it is an error that
  says to write `&quot;`, or the value as a Quillo string, `title={"say \"hi\""}`.
  HTML's other two ways are taken too, as text with no `{...}`
  in them: `'single quotes'` and an unquoted value, `<p class='a b' id=top>`, which go
  out double-quoted. On HTML's boolean attributes (`disabled`, `hidden`, `checked`...)
  `true` renders a bare attribute and `false`/`null` leaves it out. On `aria-*`, `data-*`,
  `draggable`, `spellcheck` and `contenteditable`, whose value is the text, `true` and
  `false` are written `"true"` and `"false"`, and `translate` gets `"yes"`/`"no"`, as React
  writes them *(since 0.7: `aria-hidden={true}` was bare, which hides nothing, and
  `aria-expanded={false}` was left out)*; written bare, `<div aria-hidden>`, it is `"true"`.
  `null` leaves any attribute out. A name may hold `-`, `_`, `:`, `@` and `.` (`data-id`, `bind:value`, `@click`),
  the `{name}` shorthand takes a bare name, and an attribute written twice is an error.
- An event attribute given an expression, `onclick={() => ...}` or `onClick={save}`, is a
  handler *(since 0.7)*, the same as `@click={...}` (both on one element is an error).
  Where React's name means another DOM event, it is that one: `onChange` on an `<input>`
  or a `<textarea>` is `@input`, each keystroke (the DOM's `change` waits until the field
  loses focus; `@change` is still that); `onDoubleClick` is `@dblclick`; `onFocus` and
  `onBlur` are `@focusin` and `@focusout`, which a wrapper around fields hears.
  `onClickCapture` and React's other `…Capture` names are an error, as there is no capture
  phase here. The DOM's names are lower case and compared as written, so `quillo check`
  warns of `@mouseEnter`, which never fires. Given text,
  `onclick="..."`, it is HTML's own and runs in the browser as written, so it takes only
  `"text"` written in the program: the browser decodes the value and runs it, and escaping
  cannot keep data out of code. Text made from values, ``onmouseover={`go('${u}')`}``, is
  an error when the program loads (and in `quillo check`), and a handler given text
  (`onclick={u}` with `u` a str) is an error in `quillo check` and does nothing in the page.
  `srcdoc` takes only text too. An `<iframe>`
  with a written `sandbox` attribute that does not say `allow-same-origin` may take
  `srcdoc={...}`: its page cannot reach the site. A view's or component's attributes
  are its parameters, so `on_change={...}` there is fine.
- `null`, `false` and `true` children render nothing, as in React *(since 0.7: `true` showed
  "true")*, so `{admin && " (admin)"}` is the way to show something for a flag; list children
  render each item, so `{for ... => <li/>}` works. A function where markup goes renders
  nothing too, on the server and in the browser.
  `{...xs}` among children is an error that says to write `{xs}`. A function where markup
  goes is a mistake, `{onClick}` for `{label}`, or React's function as children: `quillo
  check` warns (call it, `{render(x)}`, or give it to a view as an attribute).
  A map as a child is shown as Quillo writes it, which is seldom meant: `quillo check`
  warns on `<p>{{ name }}</p>` (`` shows the map `{ name: ... }`: to show `name`, write
  `{name}` ``), and on any map or `{{ ... }}` block as an element's child. A map as an
  attribute (`Prop={{ k: v }}`) is as before.
- Void tags (`<br>`, `<img ...>`, `<input ...>`) need no closing tag. Any tag can self-close with `/>`.
  An end tag right after one, `<img src="a.png"></img>`, as JSX and XHTML write it, is
  passed over *(since 0.7)*; one anywhere else is an error whose fix removes it.
- `<html>` gets `<!doctype html>`. A `<!DOCTYPE html>` written before `<html>`, as an HTML
  file has it, is passed over; before another element it is an error (`` `<!DOCTYPE html>`
  goes before an <html> element, which gets one by itself ``).
- `<style>{style}</style>` takes a style sheet as it is (no escaping, so `a > b` works),
  but for each `<`, written as CSS's own `\3c ` (the same character to CSS), so the text
  cannot end the element, nor start one inside `<svg>` or `<math>`, where a browser reads
  a `<style>`'s text as markup *(since 0.7)*. A `<script>`'s values are JSON with `&` as
  `\u0026` for the same reason.
  Write it as `` css`...` ``: a template string (braces are plain text, `${...}` fills in
  values) that editors highlight as CSS. Each `${...}` is a CSS value: its `{`, `}`, `;`
  and `\` are written as CSS's escapes, so `` css`p { color: ${c} }` `` with
  `c = "red} input[value^=a]{background:url(//evil)}"` adds no rule *(since 0.7)*, and
  so is its `&`, as `\26 `, since inside `<svg>` and `<math>` a browser reads a
  `<style>`'s `&#125` as a `}`.
  `` sql`...` `` does the same for queries.
- A component's or view's own CSS is `style` at the start of its body, before its markup
  (from the start, with what the page is sent: [Styling a component](#styling-a-component)):

  ```quillo
  component Card() {
      style css`
        .title { color: darkred }
        .title:hover { text-decoration: underline }
      `
      state open = false
      <div.card><h2.title>{...}</h2></div>
  }
  ```

  It styles only the elements the component writes, so another component's `.title`, or
  the page's, is left alone. Each element written in its body (in helpers written there
  too, and in markup it passes to other components) carries an attribute of its own,
  `data-q-<id>`, and each rule is made to need it: `.title` is `.title[data-q-3f9a0c12]`,
  the same on the server and in the browser. `:global(...)` keeps a selector, or the start
  of one, for the page: `:global(body.dark) .title`, `.map :global(.leaflet-pane)`.
  `@media`, `@supports`, `@container` and `@layer` are scoped inside; `@keyframes` and
  `@font-face` are kept as they are. The style is the same for every use of the component,
  so it cannot read its parameters (set a class or `style=` on an element for that).
- A `style` at a file's top level styles the components that file declares, and nothing
  else: not the markup of its routes, views or top-level `fn`s (even one its component
  calls), not other files' components, not the page. It is scoped as a component's own
  style is, with an attribute of the file's (from its path, the same on the server and in
  the browser) on every element its components write, and sent once for the file rather
  than once for each component that shares it:

  ```quillo
  // lib/cards.qwe
  style css`.title { color: darkred }`
  component Card(t) {
      style css`.title { color: navy }`        // a component's own rule wins over the file's
      <div.card><h2.title>{t}</h2></div>
  }
  component Badge(t) { <span.title>{t}</span> }   // darkred
  ```

  It comes before the file's first component (``a file's `style` comes before its
  components``), and the file must declare one (``a file's `style` styles the components
  declared in it, and this file has none``). Several are joined in order. Its rules come
  before the components' own, with the same specificity, so a component's own rule wins.
  `import cards use Card` takes Card with its file's style. `style global` at the top
  level does the same with the selectors as written, not scoped.
- `style_with("npx postcss")`, at the top level, passes every component's and view's
  `style` through a command that reads CSS and writes CSS, as the program loads and before
  the styles are scoped: PostCSS with Tailwind's plugin (and its config beside the
  program) makes `@apply`, `theme()` and nesting work in a component's own CSS. All the
  styles go through it at once; what it made is kept while the CSS is the same, and a
  command that fails stops the program loading, with what it said. `style_with(f)` takes
  a function instead, Quillo's or a Rust file's (see [Rust](#rust)): it is called once
  with all the styles, each after a `/*! quillo-style <id> */` mark, and gives them back
  with the marks. One that fails, or gives something other than text, stops the program
  as a command does (``style_with `engine` failed: it gave int, not CSS text``):

  ```quillo
  fn engine(css) -> str { replace(css, "@apply m-2;", "margin: .5rem;") }
  style_with(engine)
  ```
- With Tailwind's CLI for the page's own sheet: `quillo run main.qwe --watch --with
  "npx tailwindcss -i src/app.css -o public/app.css --watch"`, the sheet served with
  `file()`. Tailwind finds classes written out in strings and attributes (`class="px-3"`,
  `ok ? "text-green-700" : "text-red-700"`); a class made from parts (`` `bg-${color}` ``)
  is found by neither it nor Quillo. Under [`--watch`](#options-of-quillo-run), each time it writes the sheet the
  open pages take it as they are. Tailwind's scanner reads the shorthand too
  (`<div.flex.p-4.md:flex-row>`), but a fraction right before `/>` is read with the
  slash (`<i.w-1/3/>`): put a space before `/>`. `classes()` *(since 0.7)* gives the classes the
  program's markup writes, sorted, for a CSS engine called from the program, and `quillo
  classes app.qwe` prints them one a line, for a tool beside it (Tailwind's `@source
  inline(...)`, or a safelist file). Both read the source: the `.class` shorthand,
  `class="..."`, `class:name`, and the whole words in the text of `class={...}`; a class
  made from parts (`` `bg-${color}` ``) is in neither. For
  production, build the sheet once (the same command without `--watch`) as a deploy
  step, and serve it under a name that changes with its content, with `file(..., {
  cache: "1y" })`: Quillo does not do that step. A CSS engine the program calls (a Rust
  crate in a `.rs` file) can make the sheet as the program starts, with `build "/app.css"
  = ...` (see [Static files](#static-files)).
- `style global card_css` is the component's CSS with its selectors as written, not made to
  need its attribute: for a library whose classes are its own already (the UI kit's are all
  `ui-...`), whose rules may reach into what the caller passes in (`.ui-card-body >
  :first-child`). It is still sent only to pages that show the component, and companions
  that share one stylesheet (`Tabs` and `Tab`, each `style global tabs_css`) send it once,
  also from a view whose body is only another component's tag (FEEDBACK28.md, Q-345).
- A page is sent the styles of the components and views on it, and of those its bundle can
  show, as a sheet of its own in its `<head>`, `/_quillo/s/<hash>.css`, which the browser
  keeps for good. A `<style>` the page writes is for what is not a component's.
- `quillo check` warns of a class a sheet styles (`` css`...` ``, a `style`, or a `.css`
  file under `public/`) that nothing in the program writes: "`.never-used` is styled here,
  but nothing in the program writes it". Text anywhere in the program counts, and so do
  its services' and the scripts under `public/`. A template's start counts for every class
  that begins with it (`` `card-${size}` `` writes any `card-...`), and so do the classes
  Quillo sets (below). For classes a library's script sets, name their starts in a comment
  in the sheet: `/* set elsewhere: leaflet- */`. A file that does not parse counts as
  writing every class its text holds, so its one error is not followed by a warning for
  each of them.
- `<script>{x}</script>` writes `x` as JSON, a JavaScript value (`<` written `\u003c`,
  so it cannot end the script): `<script>const user = {user};</script>`. `null` and
  `false` are `null` and `false` there, and a string written in the braces, `{"hello"}`,
  is `"hello"` *(since 0.7: they wrote nothing, or the text bare, and the script did not
  parse)*. What puts something in or not, `{dev && raw(x)}` or an `if` with no `else`,
  writes nothing when it gives `false` or `null`. HTML in it
  (`raw(code)`) goes in as it is, except that `</` becomes `<\/` and `<!--` becomes
  `<\!--`, which mean the same inside JavaScript strings and cannot end the element
  early, so `raw(json.stringify(x))` there is safe too. The script's own text goes in the
  same way: as written, quotes included, with only `</` and `<!--` escaped; code kept in
  a string goes in as code with `raw`, `<script>{raw("if (a < b) go('x')")}</script>`. A `{` in a script's text
  starts a Quillo value, so JavaScript with braces is a parse error that says what to
  do: ``a `{` in a <script> starts a Quillo value: write the code as a string,
  <script>{raw(CODE)}</script>, or put it in a file of its own``. A component's `<script>` (made
  in the browser, which runs it) takes only text written in the program. Script built
  as text outside a `<script>` element (`raw("<script>" + ... )`) is not looked at.
- `raw(s)` marks trusted HTML. HTML a visitor wrote, even cleaned by a sanitizer, must not
  keep the attributes Quillo's islands are made of: `data-qi`, `data-qd`, `data-qt`,
  `data-c`, `data-p`, `data-s`, `data-l`, `data-v` and `data-e`, nor `<quillo-island>`
  elements. The page takes such an element up
  as one of its components, with the props and state it says, and a `children` text in its
  `data-p` as HTML (FEEDBACK51.md, Q-667). DOMPurify keeps `data-*` unless told not to
  (`ALLOW_DATA_ATTR: false`).
- A capitalized tag calls a view: attributes fill parameters by name, and the element's body is passed as `children`.
  A view with no `children` parameter given a body is an error *(since 0.7: the body was
  dropped)*, as an attribute it has no parameter for is; spaces alone are no body.
  An HTML tag written in capitals (`<DIV>`) is taken for a view, and the error says to
  write it in lowercase, `<div>`.
- Comments: in markup, a line starting with `//` is a comment, and so are `{/* ... */}`
  and `{// ...` with its `}` on a later line, as in JSX, and `<!-- ... -->`, as in HTML;
  none of them reaches the page. Between a tag's attributes, a `//` line and `{/* ... */}`
  are comments too. A `<!--` with
  no `-->` is an error. In code, `//` to the end of the line and `/* ... */` anywhere; a
  `/*` never closed is an error at its line, not the rest of the file left out. `//` after
  other text on a line is text (`https://...`), and `{"//"}` writes the characters.
- `<textarea>{text}</textarea>`: the box shows `text`, and shows it again whenever
  `text` changes, even after someone has typed in it; while `text` stays the same,
  what was typed stays (a starting text from a prop). `<textarea value={text}>` holds
  the box to `text` instead, as with any input: typing that the state does not take is
  undone. Write one or the other, not both.
- The browser re-parses what the server sends, and some markup does not survive that:
  it ends a `<p>` before a `<div>`, `<ul>`, `<section>` or other block, ends a link
  before an `<a>` inside it, and drops a `<form>` inside a `<form>`. The page is then
  not what was written, and its components are made again in the browser. `quillo check`
  reports these as errors, a view or component counting as its outer element
  (`<p><Presence/></p>` when `Presence` is a `<div>`); one that may be either
  (`link ? <a> : <span>`) is let through.

#### From HTML

HTML pasted into a literal mostly stays as it is: entities, bare attributes,
`onclick="..."`, SVG's `viewBox`, custom elements, `<template>`, `<!-- comments -->`,
`'single-quoted'` and unquoted attribute values, and a `<!DOCTYPE html>` before `<html>`.
A `<template>`'s markup is its content (`tpl.content` in the page's JavaScript), from the
server and made in the browser alike. A form's reset brings the fields a component made in
the browser back to their first values, as it does the server's. What to change:

| HTML as written | In Quillo |
|---|---|
| `<p>one<p>two`, `<li>one<li>two`, closed by what follows | close each one: the error says where (`` `<p>` is never closed: add `</p>` before `</div>` on line 1 ``) |
| `<style>body { color: red }</style>` | `` <style>{css`body { color: red }`}</style> ``: braces in markup are code |
| `<script>if (a < b) { go() }</script>` | the code in a string, `<script>{raw(CODE)}</script>`, or in a file of its own |
| `<DIV CLASS="a">` | lowercase, `<div class="a">`: a capital starts a view's name |
| `{` and `}` in text, as a code sample in a `<pre>` has them | `{"{"}` and `{"}"}`, or the sample in a string, `<pre>{SAMPLE}</pre>` |
| a `$` before a `{` in text | `{"$"}` |
| text and an inline element on lines of their own, `Hello,` then `<b>Ann</b>` | `{" "}` at the end of the line, as HTML's space between them is none here (`quillo check` says where) |

#### From JSX

Markup is written like JSX, and React's habits mostly work: `className` is told,
`onClick={f}` is a handler, `{cond && <p/>}`, `xs.map(x => <li/>)`, `style={{ fontSize: 12 }}`,
`{/* comments */}`, `<img></img>`. What to change, where `quillo check` or the error says it too:

| JSX as written | In Quillo |
|---|---|
| `className="a"`, `htmlFor="n"` | `class="a"`, `for="n"` |
| `<>...</>`, `<Fragment>`, `<React.Fragment>` | one element around them, like `<div>`, or a list |
| `function Card(props)`, `props.title`; `function Card({ title })` | `view Card(title)`: a view's props are its parameters, by name |
| `<Card.Header/>`, a member tag | `<Header/>`, imported by its own name |
| `const Tag = "h1"; <Tag/>`, a dynamic tag | a `match` choosing between written tags, or a view per tag |
| `{children}` without declaring it | a `children` parameter: a body given to a view without one is an error |
| `xs.map((x, i) => ...)` | `for [i, x] in enumerate(xs) => ...`, as `map` passes one item |
| `{...xs}` among children | `{xs}`: a list shows each item |
| `ref={el}` | `@mount={(el) => ...}`, given the element |
| `dangerouslySetInnerHTML={{ __html: s }}` | `{raw(s)}` between the tags |
| `defaultValue`, `defaultChecked` | `value`, `checked`: a field with no state keeps what is typed |
| `onClickCapture` | `onClick` (or `@click`): there is no capture phase |
| SVG's `strokeWidth`, `fillRule` | SVG's own `stroke-width`, `fill-rule` |
| `{cond && " (admin)"}` with `0` or `""` | `0` and `""` are truthy in Quillo: compare, `{n > 0 && ...}` |
| `<input value={x} onChange={(e) => setX(e.target.value)}/>` | works as is; or `value={x}` alone, which follows typing by itself |

To send an HTML file unchanged, serve it with `file()` or `files()`, or a whole folder with
`quillo serve` (see [Static files](#static-files)); `raw(read_file("page.html"))` puts it
in a page of the program's, with Quillo's scripts.

## Components

A `component` is a view with state that also runs in the browser:

```quillo
component Counter(start: int = 0) {
    state count = start                  // state: set once; assigning it updates what shows it
    let double = count * 2               // recomputed on every render
    <div>
        <span>{count} (x2 = {double})</span>
        <button onclick={() => { count += 1 }}>+</button>
    </div>
}

route GET / = <html><body><Counter start={5}/></body></html>
```

- The server renders it like a view, inside a `<quillo-island>` that carries its props
  and state. The page loads its bundle automatically, and the browser resumes the
  component from that state, on the server's own elements (see below). The island is
  `display: contents`, so it does not change the layout, but it is still an element:
  a child selector such as `.posts > li` does not reach the component's root element
  (write `.posts li`, or give the component's root the class), and `:first-child` or
  `:nth-child` count the islands, so each component in a list is a "first child" of its
  own island. A component whose root
  is a table's or a select's part (`tr`, `td`, `th`, `thead`, `tbody`, `tfoot`,
  `caption`, `colgroup`, `option`, `optgroup`) has no island element, since the browser
  would move it out of the table: its root element carries the island (`data-qi`), so a
  row component is a row; the island's attributes on it are the server's, and those the
  markup writes of their names are left out. Inside an `<svg>`, which draws no
  `<quillo-island>`, a component whose root is one element (`<rect>`, `<g>`, `<a>`, an
  inner `<svg>`) is its own island the same way, from the server and when the browser
  makes it, a deferred one too; outside an `<svg>`, and in a `<foreignObject>`, it is in
  its island as before. `raw(...)` in a component is likewise in a
  `<quillo-raw style="display: contents">`, which a child selector does not see through.

  ```quillo
  component Row(name: str, qty: int) {
      <tr><td>{name}</td><td>{qty}</td></tr>
  }

  view Stock() = <table><tbody>
      <Row name="lamp" qty={2}/>
      <Row name="desk" qty={1}/>
  </tbody></table>
  ```

  Each row is `<tr data-qi="" data-c="Row" ...>` in the page, with no island around it,
  so `tbody > tr` finds both.
- `state count = start` declares state, set once (`~count = start` is the short form).
  After that it is just `count`, and assigning to it (`count += 1`) updates what reads
  it, and nothing else. A component's code runs once. A `let` that reads state or props
  (`let double = count * 2`) is worked out again when they change. So is a statement
  that reads them (`if running { every(...) }`). An assignment written alone in the body,
  `count = len(items)`, runs once, as the component starts, and does not follow what it
  reads: `quillo check` warns of one that reads a prop or another state (`` `count = ...`
  runs once, as <T> starts: it does not follow `items` when that changes; for a value that
  follows it, write `let count = ...` in place of the state ``), so a value worked out
  from others is a `let`. Each place in the
  markup that shows a value follows only that value: changing one row's text changes that
  text. Assigning the value a state already has does nothing.

  ```quillo
  component Total(price: int) {
      ~qty = 1                                  // the same as `state qty = 1`
      let total = price * qty                   // worked out again when qty changes
      <p>{qty} x {price} = {total} <button onclick={() => { qty += 1 }}>+</button></p>
  }
  ```

  `<Total price={3}/>` shows `1 x 3 = 3`, and `2 x 3 = 6` after a click.
- `for` in markup keeps a row for each item: by its `key={...}`, else by its place. The
  key is the row's element's, written after `=>` or as the value of a block
  (`for p in xs { let n = p.name; <li key={n}>{n}</li> }`, whose statements the key may use).
  `key` is the list's, not passed to the view or component, so neither may take a
  parameter named `key` (a load-time error); an attribute written twice on one element
  is an error too.
  A row that is a choice (`? :`, `if`/`else`, `match`) of elements that each have a `key`
  is keyed by it; with one branch that has none, the rows are kept by place.
  A row whose item changed is updated where it shows what changed, and moved rows keep
  their DOM, focus and the selection in a field (in browsers without
  `Element.moveBefore`, Safari, the focus and selection are put back after the move).
  `if`, `? :`, `match` and `cond && <markup>` in markup make a
  branch again only when another one is chosen: an input in one keeps its focus and
  what was typed while the page updates. Nested components keep their own state.

  ```quillo
  component Todos() {
      state items = [{ id: 1, text: "milk" }, { id: 2, text: "bread" }]
      <div>
          <ul>{for it in items => <li key={it.id}><input/> {it.text}</li>}</ul>
          <button onclick={() => { items = reverse(items) }}>Reverse</button>
      </div>
  }
  ```

  Type in the first row's input and press Reverse: "bread" comes first, and what you
  typed moves with "milk", as its row was moved, not written again.
- `@event={fn}` handles DOM events (`@click`, `@input`, `@scroll`, `@submit`, ...), on
  HTML elements only (on a view or component tag it is an error). `onclick={fn}` is the same
  handler, written as HTML and JSX write it, and the form this guide's examples use
  (`onsubmit`, `oninput`, `onmount`, ...); on a view or component, `on_change={...}` or
  `onChange={...}` is a parameter like any other. There are no
  modifiers: `@click.prevent` is taken as an event named `click.prevent`, which never
  fires; call `e.preventDefault()`. Event names are not checked, so a misspelt one
  (`@clik`) is silent. Clicks are handled once, at the document, innermost first, so
  `e.stopPropagation()` stops outer handlers, and a library that stops a click before
  it reaches the document hides it from Quillo; other events listen on their element.
  The function may take the event `e` (it gets it only if it declares a parameter):
  - `e.value` and `e.checked` of the input (`null` and `false` when it has none), `e.key`
    of a key press, `e.type`;
  - `e.form`: the fields of the form, as a map (a file field gives the file's name; a
    name given twice, the last), with the name and value of the button that sent it,
    as the browser sends them;
  - `e.target`: the element itself;
  - any other field of the browser's event (`e.clientX`, `e.deltaY`, `e.shiftKey`,
    ...), then of the element (`e.scrollTop`, `e.scrollHeight`, `e.clientHeight`,
    `e.dataset`, ...);
  - its methods: `e.preventDefault()` keeps the browser from doing what it would (an
    arrow key moving the cursor, a pasted image going into a textarea). It must come
    before anything the handler waits for (a `server fn`), since by then the browser
    has gone on.

  ```quillo
  component Search() {
      state sent = ""
      <form onsubmit={(e) => { sent = e.form.q }}>
          <input name="q" onkeydown={(e) => { if e.key == "Escape" { e.value = "" } }}/>
          <button>Go</button>
          <p>{sent}</p>
      </form>
  }
  ```

  Submitting shows what was typed and the page stays; Escape empties the input.

  Setting a field sets the element's, when the event has no field of that name:
  `e.value = ""` empties the input (a file picker too, so that picking the same file
  again fires `change`). `@submit` does not reload the page. HTML5 drag and drop
  (`e.dataTransfer`), pointer capture (`e.target.setPointerCapture(e.pointerId)`) and
  SVG markup work as they do in the browser.
- What a person changes in the page is two-way by itself *(since 0.7)*: `<input value={name}/>`, where
  `name` is the component's state and nothing on the input sets it, keeps the input and
  `name` in step, typing included. The same for each thing a person can change:

  | Element | Attribute | Changed by the person on |
  |---|---|---|
  | `<input>` (text, number, date, range, color, ...), `<textarea>`, `<select>` | `value` | `input` |
  | `<input type="checkbox">` | `checked`, `indeterminate` | `change` |
  | `<details>` | `open` | `toggle` |
  | `<dialog>` | `open` | `close` |
  | `<video>`, `<audio>` | `muted`, `volume` | `volumechange` |
  | `<video>`, `<audio>` | `currentTime` | `timeupdate` |
  | `<video>`, `<audio>` | `playbackRate` | `ratechange` |

  ```quillo
  component Settings() {
      state name = ""
      state notify = true
      state more = false
      state loud = 0.8
      <form>
          <input value={name}/>
          <input type="checkbox" checked={notify}/>
          <details open={more}><summary>More</summary>...</details>
          <audio src="/bell.mp3" volume={loud}></audio>
          <p>{name}, {notify ? "notified" : "quiet"}, volume {loud}</p>
      </form>
  }
  ```

  A number or range input gives a number back (`null` when empty, or only spaces), so
  `qty + 1` after typing 3 is 4, and text being typed that already reads as the state's
  number is left as typed (`1.0`, `2.50`, `1e3` are not rewritten to `1`, `2.5`, `1000`).
  A text input, `<textarea>` or `<select>` bound to a state that holds a number gives a
  number when the text is that number's own form, and text otherwise (`"02"` stays text),
  `null` when empty; bound to a text state it gives text. A range's value is set again
  when its `min`, `max` or `step` change, so a value set with a new `max` is kept, and one
  past the new limits is brought into them, as the browser does. A player's `volume` is
  kept between 0 and 1 and its `playbackRate`
  between 1/16 and 16, whatever the state says, and a place the player has just reported
  is not sought back to while it plays.

  It stays one way, the attribute held to the value, when:
  - the element has a handler of its own on an event that says it changed (`@input` or
    `@change` for a value, `@change` or `@click` for a box, `@toggle` or `@close` for
    `open`, the player's own events): a transform, `value={code} @input={(e) => { code =
    upper(e.target.value) }}`, or a check before the value is taken;
  - it is `readonly` (but not `readonly={false}`);
  - the value is not a state (a `let`, a prop, an expression): an input showing a `let`
    keeps what is typed and sets nothing;
  - what the element is cannot be known when the program is compiled: a `type={...}`
    decided as it runs, or a `{...spread}` (which may hold a handler);
  - it is a radio (unticking one fires no event, so each cannot follow a state of its
    own: give the group one state and `checked={size == "s"}`), or a `<select multiple>`,
    whose `value={list}` marks each option the list names, on the server and in the
    browser.

  A `<select value={x}>` is written on the server as `selected` on the first option `x`
  names (by its `value`, else by its text, spaces collapsed), or none, which shows the
  first; the select itself gets no `value` attribute, which HTML does not have. In the
  browser the value is set once its options are in, and again when they change, so
  options made from a list that arrives later still show the one `x` names; what a person
  picked is kept while the page updates, unless options are added or taken away.

  `quillo check` warns of an input whose value is a parameter (a view's, or a component's
  prop) or a state's field (`value={form.name}`) when nothing on it takes what is typed: no
  handler, no `bind:`, no `name=` for a form sent to a route, not `readonly` or `disabled`.
  The input would show the typed text while the program kept what it had: "typing here
  changes nothing: `v` is `Field`'s parameter, which `value=` only shows". Pass the state's
  own input down, give the field a state of its own, or take what changes with `@input`.

  `value` on a checkbox, radio, file or button input is not what a person changes, and
  stays an attribute.
- `bind:x={y}` says the same in so many words, for any attribute in the table, and also
  binds a local variable, and works with a handler of its own: `bind:value` with `@input`
  is one listener, the binding first, so the handler reads the state already set:
  `<input bind:value={amount} @input={refresh}/>`. `y` is the component's state or a
  local, not a global or a map's field (give each input its own state, and make the map
  with a `let`). On a custom element (`<my-field>`), `bind:value` and `bind:checked` listen
  to `input` and `change`. Any other `bind:` is an error that names those there are.
- An input with `value={x}` (or `checked`, `selected`, and a `<video>`'s or `<audio>`'s
  `muted`) that is one way is held to `x`: after each handler it is set back to the state,
  so typing the state does not take is undone. A bare `<input>` keeps what is typed, read
  through `e.value` or `e.form`.

  ```quillo
  component Pin() {
      state pin = ""
      <input value={pin} oninput={(e) => { if len(e.value) <= 4 { pin = e.value } }}/>
  }
  ```

  The handler takes four characters; a fifth is typed, not taken, and undone, so the
  input still shows `1234`.
- `style={...}` takes text or a map of properties *(since 0.7)*: `style={{ color: tone, font_size:
  "2rem", marginTop: "3px", "--gap": "4px" }}`, where `_` and camelCase are written as
  `-` (`font-size`, `margin-top`), a custom property is kept as it is, and a `null` or
  `false` value is left out; the same on the server and in the browser. A number is in
  pixels, as React has it, except `0` and React's unitless properties (`opacity`,
  `z-index`, `line-height`, `flex`, `font-weight`, `order`...) and custom properties:
  `{ fontSize: 12, opacity: 0.5, "--gap": 4 }` is `font-size: 12px; opacity: 0.5; --gap: 4`
  *(since 0.7: `12` was written bare, and the browser dropped it)*. `ms` starts a prefix as
  `Webkit` and `Moz` do: `msFlex` is `-ms-flex`. A value's `{`, `}`, `;` and `\` are
  written as CSS's escapes (`\7b `, `\7d `, `\3b `, `\5c `), so a value from data is one value
  and adds no declaration: `{ color: "red;background:url(//evil)" }` is one colour the
  browser does not take. A key is a property's name (letters, digits, `-` and `_`) or a
  custom property's; another key is left out *(since 0.7, FEEDBACK51.md, Q-668)*.
- `class={...}` takes text, a map (the keys whose values are true) or a list (its texts,
  lists inside it flattened, `null`, `false`, `true` and `""` skipped, a map in it giving
  its true keys), as `clsx` does: `class={{ active: on, off: !on }}`, `class={["card", big
  && "big"]}` *(since 0.7: they were written as Quillo text)*. Quillo's truthiness counts,
  so `{ d: 0 }` gives `d`. Nothing left gives no `class` at all. After an update the
  element has exactly the new classes.
  `style={...}` and `class={...}` change only what they name when what they read
  changes: a `transform` or a class a handler set by hand stays, so a drag does not jump
  home. `<details open={x}>` and `<dialog open={x}>` follow `x` when `x` changes, and
  leave what a person opened or closed alone otherwise (FEEDBACK28.md, Q-337, Q-338).

  ```quillo
  component Badge(tone: str, big: bool) {
      <p style={{ color: tone, font_size: "2rem", margin_top: big ? "1rem" : null, "--gap": "4px" }}>New</p>
  }
  ```

  `<Badge tone="teal" big={false}/>` writes `style="color: teal; font-size: 2rem; --gap:
  4px"`: `margin-top` is left out.
- `==` on two of the page's objects (elements, events) asks whether they are the same one
  (Q-334).

  ```quillo
  component Modal() {
      state open = true
      state backdrop = null
      <div class="backdrop" onmount={(el) => { backdrop = el }}
           onclick={(e) => { if e.target == backdrop { open = false } }}>
          {open && <div class="box">Click outside to close</div>}
      </div>
  }
  ```

  A click on the box reaches the backdrop's handler too, with the box as `e.target`, so
  only a click on the backdrop itself closes it. (`e.currentTarget` is the document, as
  clicks are handled there.)
- Assigning state updates the page a moment later (once the handler's synchronous code
  is done, or at its first wait), all changes at once. A state is changed when it gets a
  different value: assigning the same list again does nothing, and assigning an equal
  but new list updates what reads it. Code that sets what it reads goes round in a loop,
  and is stopped after 1,000 rounds with a message in the console. See
  [Reactivity](#reactivity) for the whole model.

  ```quillo
  component Pair() {
      state a = 0
      state b = 0
      <p onclick={() => { a += 1; b = a * 10 }}>{a} and {b}</p>
  }
  ```

  A click shows `1 and 10` at once; the page never shows `1 and 0` in between.
- `every(ms, f)` in a component's code calls `f` every `ms` milliseconds, in the browser,
  for as long as the code that called it stands: `if running { every(100, step) }`
  runs a game while `running` is true, starts again with the new speed when that changes,
  and stops when `running` is false or the component leaves the page. On the server it
  does nothing. It is called from the component's code (or a `let` or `if` there), or
  from one of its event handlers or its `@mount`, where it runs until the component
  leaves the page (started after a handler's first wait, it belongs to no one, and is an
  error). A tab in the background is slowed by the browser to once a second at most.
  A tick that finds that timer's last run still waiting does nothing, so a poll slower
  than its timer runs one call at a time and shows each answer; a run with no answer
  after 30 seconds (or two gaps, if that is longer) is given up on, and the next tick
  starts anew. A clock's tick should not share an `every` with a call that waits: give
  it its own.

  ```quillo
  component Stopwatch() {
      state running = false
      state tenths = 0
      if running { every(100, () => { tenths += 1 }) }
      <p>{tenths / 10} s <button onclick={() => { running = !running }}>{running ? "Stop" : "Start"}</button></p>
  }
  ```

  Start counts in tenths of a second; Stop ends the `every`, as the `if` that called it no
  longer stands.
- `run_after(ms, f)` *(since 0.7)* calls `f` once, `ms` milliseconds on, unless its component has gone
  first: from a handler, `@mount` or the component's code alike. There is no
  `setTimeout`; a debounce or a throttle is an `every` under a state flag that the tick
  turns off. `page_updated()` *(since 0.7)* in a handler waits until the page shows what the handler
  has set so far, to read or focus what that made appear (FEEDBACK28.md, Q-349).
  `listen(url, f)` follows a stream the same way (see [Streaming](#streaming)), and opens
  it again when the connection drops (after a second, doubling up to 30 s), not when the
  stream ends.

  ```quillo
  component Save() {
      state saved = false
      <p>
          <button onclick={() => { saved = true; run_after(2000, () => { saved = false }) }}>Save</button>
          {saved && <span>Saved</span>}
      </p>
  }

  component Names() {
      state names = ["Ana"]
      <div>
          {for n in names => <input class="name" value={n}/>}
          <button onclick={() => {
              names = names + [""]
              page_updated()                    // the new input is in the page now
              js("document.querySelector", ".name:last-of-type").focus()
          }}>Add</button>
      </div>
  }
  ```

  "Saved" shows for two seconds. Add puts a new input in the page and focuses it, which it
  could not do before `page_updated()`, as the input was not there yet.
- Component code, and the functions, views and constants it uses, is compiled to
  JavaScript. Code that cannot run in a browser is an error when the program loads
  (under [`--watch`](#options-of-quillo-run), the option for working on a program, `quillo run main.qwe --watch`: in the log right after the load, and in the browser's console of
  a page that needs the component; and in `quillo check`, for what the component calls
  directly): `request`, `db`, `env`
  (but for variables named `PUBLIC_...`), files, `reply`, or a global the server changes
  (pass it as a prop instead). `http.*` runs in the browser, from event handlers (see
  [What the browser can reach](#what-the-browser-can-reach)). Any name is fine in
  component code, JavaScript's own words included (`new`, `class`, `this`, `window`): the
  compiler renames them.

  ```quillo
  component Orders() {
      state rows = db.query("select * from orders")
      <p>{len(rows)} orders</p>
  }
  ```

  ```
  main.qwe:2: error: `db.query` is not available in the browser, where <Orders> also runs; use a `server fn`
  ```

  Ask the server for them instead: `server fn orders() -> list with db = db.query("select * from
  orders")`, then `state rows = orders()`.
- What the browser has of the standard library: text, lists, maps, numbers, errors,
  `raw`, `escape`, `url_encode`, `now`, `random`, `random_int`, `uuid`, `format_number`,
  `markdown` (made by the server), `highlight`, `t` and `locale`, `json`, `re`, `math`,
  `stats`, `vec`, `date`, `money`, `http.*`, and what only a page has: `url`,
  `navigate`, `every`, `listen`, `receive`, `ask`, `notify`, `js`, `js_new`,
  `channel.open`. `print` writes to the console. Not there, and an error when the
  program loads: `env` (but `PUBLIC_`), `sleep`, files, `redirect`, `respond`,
  sessions and cookies, `crypto`, `image`, `db`, `publish` and `watch`, tenants,
  uploads, `schedule`, `stream`, `events`, `http.stream`, `join_on`, services.

  ```quillo
  component Receipt(total: float, at: str) {
      <p>{format_number(total)} on {at}, ref {uuid()}</p>
  }
  ```

  `<Receipt total={1234.5} at="2026-11-03"/>` shows `1,234.5 on 2026-11-03, ref` and a
  uuid, on the server and in the browser alike. `env("PUBLIC_SHOP")` may join them;
  `env("DB_URL")` may not.
- There is nothing built in for `document`, `window`, `localStorage`, the clipboard,
  focus or scrolling: reach them through the page, `js("localStorage.getItem", k)`,
  `js("navigator.clipboard.writeText", s)`, `js("window.innerWidth")` (no arguments
  reads a value), and `el.focus()` on an element (see [JavaScript
  libraries](#javascript-libraries)).

  ```quillo
  component Theme() {
      state dark = false
      <button onclick={() => {
          dark = !dark
          js("localStorage.setItem", "theme", dark ? "dark" : "light")
      }}>{dark ? "Dark" : "Light"}</button>
  }
  ```

  `js("document.querySelector", "#name").focus()` focuses an input the same way.
- Each page loads two scripts, minified, whose addresses change with their content, so
  the browser keeps them for good:
  - `/_quillo/r/<hash>.js`, the runtime: the parts of it the program's components
    reach (about 35 KB, 11 KB gzipped, for a page with one counter). Every page of the program
    loads the same one, so the browser downloads it once.
  - `/_quillo/b/<hash>.js`, the page's own code: the components it was rendered with,
    every component they can show, and what they use. Pages with the same components
    share it, whichever of them each showed.

  A page with no components loads no script. The server keeps the bundles of the last
  256 sets of components pages were made of (`quillo_bundles` under [`--metrics`](#options-of-quillo-run)); a
  page that names one no longer kept is sent the whole program's. There is no splitting
  by route and no lazy component: an `App` whose `match` shows thirty pages carries the
  thirty in its bundle. Under `--watch` a page loads one unminified bundle instead,
  which loads a file per component and function (see [Debugging](#in-the-browser)).

  ```sh
  curl -s localhost:8080/ | grep -o '/_quillo/[rb]/[^"]*'
  # /_quillo/r/f68b2dc51ad5ad52.js
  # /_quillo/b/0767fa4dd4c7b224.js
  ```
- And only when it is needed: the server's HTML shows at once, and the bundle comes
  when someone reaches for a component (points at, touches or focuses it, or presses a
  key in it). What they typed before it came is kept, and each click, double click,
  right or middle click, press and release is done again then as it was made: on the
  same element, at the same point, with the same buttons, keys and click count (a
  `@pointerdown` may still capture a pointer whose finger was lifted meanwhile). A
  click waits for the code, so a checkbox ticks and a link is followed then; should the
  code fail to load, they are done as the page alone does them. (The idea comes from
  Flickr, Google's JSAction, Angular's preboot and event replay, Qwik and React: see
  Acknowledgements in the [README](README.md#acknowledgements).) A
  component that runs by itself (`every`, `live`, `listen`, `js`, `js_new`, `@mount` in
  its code or markup, or in a component nested inside it) or that is the page (in
  `<body>` itself, an app) loads as soon as the page is idle. `/_quillo/quillo.js` has
  every component, for pages that load it themselves. Text responses are gzipped for
  browsers that accept it.

  Of the components above, `Total` and `Todos` wait for a person to reach them;
  `Stopwatch` (an `every` in its code) and `Modal` (an `@mount`) load as soon as the page
  is idle.
- A component is an element `<quillo-island data-c="Name" data-p="{props}"
  data-s="{state}">`, with `data-l` and `data-v` for live state and `data-e` when it runs
  by itself. `data-s` is the state the server rendered it with, and is not written again
  as the state changes: after a click that makes `count` 6, it still says 5, so a test
  or a tool reads the page, not `data-s`. A component inside another is written with
  `data-p="{}"`, as the one around it passes its props when it runs; one made in the
  browser (`{open && <Detail/>}` after a click) carries `data-c` alone (FEEDBACK42.md,
  Q-487). Values cross as JSON, with these marked so they come back as they were: a
  decimal (`{"$dec": "12.50"}`), an int past 2^53 (`{"$int": "..."}`, a BigInt in the
  browser), an error value (`{"$quillo_err": ...}`); a whole float is written `2.0`, and
  read back as a float. NaN and infinities become `null`; dates are their text, as
  everywhere. A map whose one key starts with `$` is written `{"$": [key, value]}` and
  read back as that map, so a key a visitor chose (`set({}, k, v)`) cannot pass for one
  of these marks in another visitor's page or on the server, both ways: in a `server
  fn`'s arguments and answer, island state and props, live data, services and sessions.
  Nor can any text become a number in the JSON the browser writes (`?q=%00int:5` reached
  a `server fn` as the int 5).

  `<Counter start={5}/>` from the top of this section is sent as:

  ```html
  <quillo-island style="display:contents" data-c="Counter" data-p="{&quot;start&quot;:5}"
      data-s="{&quot;count&quot;:5}"><div>...</div></quillo-island>
  ```
- A `state` that still holds its very prop (`state items = initial`) is written in the
  page as a reference to the prop, not as a second copy of it.

  ```quillo
  component List(initial: list<str>) {
      state items = initial
      <ul>{for i in items => <li>{i}</li>}</ul>
  }
  ```

  `<List initial={["a", "b"]}/>` carries `data-p="{"initial":["a","b"]}"` and
  `data-s="{"items":{"$quillo_prop":"initial"}}"`: the list once.
- Components are resumed, not made again: the browser runs a component's code once,
  from the state the server sent, and takes the server's elements as its own (checking that
  they are the same) instead of replacing them, so images do not reload, a scrolled
  list stays where it is and what someone typed stays typed. Each component is resumed
  when it is first reached, the others when they are needed. An element whose tag or
  text differs (a script changed the page) is made again, for that component only. An
  attribute the markup sets that differs (changed, removed or added) is the browser's
  value: under `--watch` it is said (`where it wrote title="changed", the browser makes
  title="title"`) and the component is made again, and in production the value is put
  in place, so a browser extension that rewrites styles makes no component again.
  Classes a script adds, attributes the markup does not set, and a field's `value`,
  `checked` or `selected` (what a person typed or picked) are not compared. A CR in text
  is written `&#13;` and a NUL U+FFFD, on both sides, so a note typed in a `<textarea>`
  and posted with CRLF is taken up as written.
  `state` initializers do not run again in the browser: the server's values win. There
  is no virtual DOM: each state is a signal, tied to the text, attributes and classes
  that show it, and a list's rows are moved by key.

  ```quillo
  component Roll() {
      state seed = random_int(1, 1000000)       // drawn once, on the server
      <p>{seed}</p>
  }
  ```

  The browser shows the number the server drew, and keeps it: it does not draw again.
- A JavaScript error in a handler, `@mount`, a statement or an initializer is written
  to the console (`quillo: ...`), and pushed onto `window.__quilloErrors` if the page
  made that list; the component goes on. There is no error overlay on the page.

  ```quillo
  component Broken() {
      state n = 0
      <button onclick={() => { js("JSON.parse", "{"); n += 1 }}>{n}</button>
  }

  route GET / = <html>
      <head><script>window.__quilloErrors = []</script></head>
      <body><Broken/></body>
  </html>
  ```

  Each click stops at the bad JSON (so `n` stays 0), writes the error to the console, and
  adds it to `window.__quilloErrors`, where a test can read it; the button goes on
  answering.
- `ask(message, { title, ok, cancel, tone })` (a tool's question to the person at an AI
  client is `about(question, choices, default)`, refused in a component: see [Tools for AI
  clients](#tools-for-ai-clients)) asks in a dialog on the page and waits for
  the answer (`true` or `false`) — a handler goes on after it, and its buttons, unless
  `ok` and `cancel` are given, are in the page's locale (see
  [Translations](#translations)); `notify(message, { title,
  tone, seconds })` shows a note that goes by itself. Both are markup with `q-` classes
  in a host the page lays out (`<body data-notices="dialog-center toasts-top-right">`)
  and styles: keyboard, focus and screen readers are taken care of (Escape, the
  backdrop and Cancel answer `false`; `tone: "danger"` starts on Cancel; a note goes
  after 4 seconds unless `seconds` says otherwise). On the server both are an error:
  they speak to someone in the browser.

  ```quillo
  component Remove(id: int) {
      state gone = false
      <button onclick={() => {
          if ask("Delete this order?", { title: "Delete", ok: "Delete", tone: "danger" }) {
              gone = true
              notify("Deleted", { tone: "success" })
          }
      }}>{gone ? "Deleted" : "Delete"}</button>
  }
  ```

  The dialog starts on Cancel, as its tone is `danger`; Delete answers `true`, and the
  note goes after 4 seconds.
- The classes and attributes Quillo sets itself, for a sheet to style. No `.qwe` file
  writes them, so a tool that drops CSS it finds no use for (PurgeCSS and the like) must
  be told to keep them:

  | Set by | Classes and attributes |
  |---|---|
  | `ask` | `q-notices` (the host), `q-backdrop`, `q-dialog`, `q-title`, `q-message`, `q-actions`, `q-cancel`, `q-ok`; `q-open` while shown |
  | `notify` | `q-notices`, `q-toasts`, `q-toast`, `q-title`, `q-message`, `q-close`; `q-open` while shown |
  | `ask`, `notify` with `tone` | `q-` and the tone, on the dialog or toast: `q-danger`, `q-success` |
  | the host's places | the page's `data-notices` on `<body>`, else `dialog-center toasts-top-right`, as classes of `q-notices` (any `dialog-...` and `toasts-...`) |
  | `highlight(code, lang)` | `hl-k` keywords, `hl-s` strings, `hl-c` comments, `hl-n` numbers and constants, `hl-f` functions called, `hl-t` types and tags, `hl-a` attributes, properties and keys |
  | `<quillo-media>` | `data-state`: `connecting`, `waiting`, `playing`, `ended`, `unsupported`; one that sends: `starting`, `sending`, `refused`, `ended`, `unsupported` |
  | a component's `style` | `data-q-<id>` on the elements it writes |

  ```css
  .q-backdrop { background: rgb(0 0 0 / 40%); }
  .q-dialog { border-radius: 8px; padding: 1.5rem; }
  .q-dialog.q-danger .q-ok { background: crimson; color: white; }
  .q-toast.q-success { border-left: 4px solid seagreen; }
  ```
- In the browser a value is what it is on the server, or an error, never another value.
  A whole float keeps its type: `type_of(2.0)` is `float`, `2.0 / 0` is `inf`, an `int`
  parameter, an index or `range` refuses `2.0` and a `float` parameter refuses `2`, a
  record's `float` field widens an int, and a field bound to a float state gives floats.
  Ints past 2^53 are exact (BigInts) in arithmetic, comparisons, `math`, `stats`, `vec`,
  ranges and every builtin that takes an int, and past 64 bits they are the server's
  `integer overflow` (`math.factorial(20)` is exact, `math.factorial(21)` overflows on
  both sides); an int is never `-0`. Numbers cross between the server and the page as
  their JSON text, so a float stays a float both ways (state, props, a `server fn`'s
  arguments and answers); in a browser whose `JSON.parse` gives no number's text (Safari
  before 18.4, Firefox before 135), one pass over the text marks them first. The page's
  own JavaScript (`js(...)`, a library) gets plain numbers. Text in a component writes a
  whole float as `2`, on both sides, so the page the server sends is the one the browser
  would make and is taken up, not made again; elsewhere the server writes `2.0`. Text has
  no exponent; `json.stringify` writes `2.0` on both sides, and turns to an exponent where
  JavaScript does (`1e+21`, `1e-7`). `math.*` may differ in the last digits, as each side
  has its own library.

  ```quillo
  component Price() {
      let whole = 2.0
      <p>{whole} {7 / 2}</p>
  }
  ```

  Both sides show `2 3.5`.
- A map keeps its keys in the order they were set, on the server and in the browser,
  keys that look like whole numbers too (a JavaScript object puts those first, in number
  order): `keys`, `values`, `for [k, v] in m`, `str`, `json.stringify`, `url_encode`,
  `merge`, spreads, `+`, `group_by` and records read them alike on both sides, so a
  map of ids, years or prices shows in one order, and a signature or a cache key made
  from `json.stringify` is the same. JSON the browser reads (`json.parse`, an
  `http` answer's `json`, a `server fn`'s answer, island state and props, live data,
  channels, streams) keeps its keys in the order written. With `--watch`, a component the
  browser had to make again says which, and what differed, in the console.

  ```quillo
  component Keys() {
      let m = { b: 1, "10": 2, a: 3 }
      <p>{join(keys(m), " ")} / {join(sort(keys(m)), " ")}</p>
  }
  ```

  Both sides write `b 10 a / 10 a b`.
- A top-level constant holding an error value (`let bad = int("x")`) is an error value
  in a component too, `is_err` true, its text hidden as the server hides a builtin's;
  it was a map there.

### Composition

Views and components nest like HTML: a capitalized tag renders another view or
component, its attributes are the props, and what is between the tags arrives as the
`children` parameter, or `null` for a tag with nothing between *(since 0.7)* (`<Card title="x"/>`),
on the server as in the browser, so `children == null` tells the two apart (FEEDBACK28.md,
Q-345), and `quillo check` warns on `str(children)`, which is `"null"` then. A component is used by its tag: called as a function (`let make = Dead`, then
`make()`, or given to `map`) it would show and never answer, so it is an error, and `quillo
check` says so.

```quillo
view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>

component Counter(label: str, start: int = 0, on_change = null) {
    state n = start
    <p>{label}: {n} <button onclick={() => { n += 1; on_change && on_change(n) }}>+</button></p>
}

component Panel(header, children) {          // `header` is a named slot: any HTML
    state open = true
    <div>
        <div onclick={() => { open = !open }}>{header}</div>
        {open && children}
    </div>
}

component Dashboard() {
    state total = 0
    <Card title="Counters">
        <Counter label="Apples" on_change={(v) => { total = v }}/>   // child to parent
        <Counter label="Pears" start={3}/>
        <Panel header={<b>Details</b>}><p>Last change: {total}</p></Panel>
    </Card>
}
```

- **Down:** props. **Up:** pass a function (`on_change`) and call it. A call of a
  function given as a prop is waited for, so what it gives is what the child gets: a
  callback that only sets state (`cb={() => { x = 1 }}`) gives `null`, and so does one
  that ends in a page's Promise (`cb={() => js("Promise.resolve")}`) once it is done. A
  child that must know whether its caller finished has the caller give `true` when done,
  or calls a `done` function it is given. There is no
  context or global store: state in the outermost component, passed down, is the store.
  Components placed separately in a page share nothing in the browser; they share
  through the server (`live`, channels).
- Props are passed live: a child shows its parent's new value without either running
  again. Each component keeps its own state; give components in a list a `key={...}`
  so each keeps its own (a new key makes it afresh). A key is text or a number: a list
  or a map as a key never matches, and remakes its row each time.
- A view or component takes only the parameters it declares: there is no rest
  parameter, so `id`, `data-*` or a class meant for its root element is passed as a
  parameter it declares (`<Button id="x">` is an error unless `Button` has `id`).
- A component may show itself (a tree of comments), as long as it stops somewhere.
- There are no portals and no refs: `onmount={(el) => { box = el }}` with `state box =
  null` keeps an element, and `js("document.body.appendChild", el)` moves one.
- What reads a prop follows it, as what reads state does: a place in the markup that
  shows it (`{label}`, `class={...}`), a `let` (`let doubled = count * 2`), a statement
  (`if active { every(1000, tick) }` starts and stops with it), `live(load, id)` (loads
  again when `id` changes), and a prop passed on to a child. A handler reads its value
  when it runs.
- `state x = prop` reads the prop once, when the component starts: state is the
  component's own. To follow the parent, read the prop (or a `let` of it); to start over
  when it changes, give the component a `key` that changes with it. There is no hook
  that says the prop changed under a state copied from it: the `key` starts afresh, and
  loses what was typed.
- A function called in markup with a state, `{tick(on)}`, is called again at each change.
  When its markup comes out the same shape and nothing in it follows state itself, the
  page's elements are kept and take the new attributes, text and handlers, so an input in
  it keeps its focus and what was typed. A function whose markup changes shape is made
  anew, and its input loses the focus: there, use a `view`, whose props are live.
- A plain function used as a tag (`<Row item={x}/>` where `Row` is neither a view nor a
  component) is called again as a whole when what it is given changes.
- Views are plain functions: they have no state and add nothing to the browser bundle
  unless a component uses them.
- A component placed directly in a page gets its props from the server, so they must
  be data (no functions, no HTML values), and they stay as the page was made: nothing
  above it changes them. Inside another component, props can be anything (callbacks,
  HTML, other values), and follow that component's state. A component's `state` must be
  data wherever it is. `quillo check` does not see a prop or state that is not: the page
  fails as it renders (`prop header of <D> is html, which cannot be sent to the
  browser`). `children` placed from the server travels as its HTML.

### Styling a component

A component or a view carries its own CSS. You write it with the component, and Quillo
sends it to the browser in a stylesheet of its own: nothing is written into the page's
HTML for it. The components on [quillolang.org/components](https://quillolang.org/components/)
all work this way.

**Write it at the start of the body**, before the markup:

```quillo
component Card(title: str) {
    style css`
      .title { color: darkred }
      .title:hover { text-decoration: underline }
    `
    state open = false
    <div.card @click={() => { open = !open }}><h2.title>{title}</h2>{open && <p>More about {title}</p>}</div>
}

route GET / = <html><head><title>Cards</title></head><body><Card title="Hello"/></body></html>
```

The page has a link to the sheet in its `<head>`, and each element the component writes
carries the component's attribute:

```html
<link rel="stylesheet" href="/_quillo/s/a1ce371646f1b209.css">
...
<h2 class="title" data-q-1df36318="true">Hello</h2>
```

`/_quillo/s/a1ce371646f1b209.css` holds the component's rules, made to need that
attribute, so they style the card's `.title` and no other:

```css
.title[data-q-1df36318] { color: darkred }
.title[data-q-1df36318]:hover { text-decoration: underline }
```

**Or keep the CSS in a file of its own**, beside the program, and give `style` its text:

```quillo
let card_css = read_file("card.css")     // read once, as the program starts

component Card(title: str) {
    style card_css
    <div.card><h2.title>{title}</h2></div>
}
```

Any text will do: a `.css` file read with `read_file`, or a `` css`...` `` kept in a
variable. It is scoped and sent the same way.

**Scoped, or as written.** `style` scopes the rules to the component (above). `style
global` sends them with their selectors as written, for a library whose class names are
its own already. That is what the downloadable components do: each one is a `.qwe` file
with its CSS in a variable and `style global` in its body, so copying the file brings the
styles along.

```quillo
// ui/collapsible.qwe, from the kit (shortened)
let collapsible_css = css`
.ui-collapsible { min-width: 0; color: var(--ui-fg); font: 15px/1.5 var(--ui-font); }
.ui-collapsible > summary { display: flex; align-items: center; gap: 8px; cursor: pointer; }
`

view Collapsible(title: str, children) {
    style global collapsible_css
    <details class="ui-collapsible"><summary>{title}</summary><div>{children}</div></details>
}
```

The kit's `ui_css` (its colours, fonts and the `--ui-...` variables the rules use) is the
one stylesheet a page itself puts in: `<style>{ui_css}</style>` once, in the `<head>`.

**What reaches the browser:**
- A page is sent the styles of the components and views on it, and of those they can
  show later in the browser (a `<Note>` that appears after a click), joined into one
  sheet, `/_quillo/s/<hash>.css`. A page that shows none gets no sheet.
- Its name comes from what is in it: two pages with the same components share one sheet,
  and the browser keeps it for good (`Cache-Control: public, max-age=31536000,
  immutable`). When the CSS changes, so does the name.
- The same attribute and rules on the server and in the browser, so a component the
  browser draws again keeps its style.

**What a style cannot do.** It is the same for every use of the component, so it cannot
read the component's parameters or state. For a style that changes with them, set a class
(`<div class={{ open: open }}>`) or a `style={...}` on the element. A page's own
`<style>`, or a `.css` file under `public/` linked from the page, is for what belongs to
no component: the page's layout, its fonts.

More: `:global(...)`, scoped `@media` and the other rules, a `style` for all of a file's
components, `style_with` for PostCSS or Tailwind, and what `quillo check` says of a class
nothing writes are in [HTML literals](#html-literals).

### JavaScript libraries

A component can hand an element to a library (a chart, a map, an editor) and call into
it. Name the library on a line that starts with `client` *(since 0.7)*, then call it as JavaScript does:

```quillo
client "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js" use Chart
client /assets/leaflet.js use L
client "https://cdn.jsdelivr.net/npm/canvas-confetti@1.9.3/dist/confetti.browser.min.js"
    use confetti

component Sales(points: list) {
    state chart = null
    <canvas onmount={(el) => {
        chart = Chart.new(el, { type: "line", data: points })    // new Chart(el, ...)
        () => chart.destroy()                   // when the canvas leaves the page
    }}></canvas>
    <button onclick={() => confetti({ particleCount: 120 })}>Celebrate</button>
}

component Where() {
    <div class="map" onmount={(el) => { L.map(el).setView([14.6, 121.0], 12) }}></div>
}

route GET / = <html><body><Sales points={load_points()}/></body></html>
```

- `client <script> use Name`: `client` says this runs in the browser, and the path says
  where it comes from: an address (a CDN) or a file of your own, ending in `.js` or
  `.mjs`. `Name` is what the script defines on the page (`Chart`, `L`, `confetti`,
  `dayjs`): the name the library's own documentation uses, in its case. Several from one
  script: `client /assets/leaflet.js use L, LeafletDraw`; another name for one:
  `client "..." use Chart as ChartJS`. Quotes are optional, as for every path; put an
  address in them, so editors do not read its `//` as a comment.
- A long address may leave its `use` to the next line, as `confetti` does above, so the
  names stay at the left. `quillo fix` breaks a line this way when it would be long.
- `client`, `preload`, `preconnect` and `module` are still names everywhere else:
  `client / 2` is a division. It is the `use` after a file that makes the line an import.
- Each file that calls a library says so, as for any name: another file that calls
  `Chart` writes the same line. The page still loads the script once. A file taken whole
  does not pass its browser names on: one used through it is warned of, with the
  `client` line to write. One name from two scripts (or a script and a module's export)
  is an error, in `quillo check` and as the program starts, since the page would have
  only the last: take one `as` another name.
- `Name.new(args)` makes an object of a class (`new Chart(el, config)`); `Name(args)`
  calls a function (`confetti()`); `Name.fn(args)` calls one of its functions
  (`L.map(el)`, `dayjs.extend(...)`); `Name.field` reads a value (`Chart.version`).
  What they give is the library's object, whose methods and fields are used as below
  (`chart.update()`, `map.setView(...)`).
- The page loads the script only where a component that uses the library is on it (or
  one it can come to show), once, in the order the program names its libraries, before
  the components' own code. A page without such a component loads nothing more. No
  `<script>` tag is written; one you write still works, as below.
- A path (`/assets/leaflet.js`, `assets/leaflet.js`) is from the program's own address,
  under its base path; serve it as any other file (`public/`, or a route with `file()`).
  An address (`https://...`, `http://...`, `//...`, in any case) is loaded as written:
  pin its version. Either may carry a query or a fragment (`/assets/app.js?v=2`). Any
  other kind of address (`ftp:`, `file:`, `data:`) is refused, and so is one with a user
  name or password in it: browsers do not load those, and the page would show it.
- A page loads a library late: when the browser is idle, or at the first touch of a
  component that only answers clicks. To get ready for it sooner, start the line with
  `preload` or `preconnect` in place of `client` *(since 0.7)*:

  ```quillo
  preload "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js" use Chart
  preconnect "https://unpkg.com/leaflet@1.9.4/dist/leaflet.js" use L
  ```

  `preload` has the page's `<head>` fetch the script (`<link rel="preload"
  as="script">`), which suits a library the page uses as soon as it opens, such as a
  chart in its `@mount`. `preconnect` only opens the connection to the script's
  host (`<link rel="preconnect">`), which costs little and suits one a click may need;
  it takes an address, since a path is on the page's own connection. Take one or the
  other: a preload opens the connection itself. Each goes only in
  pages that can load the library, as the script does. A preload is fetched whether or
  not the library ends up used, so keep it for one or two scripts a page needs at once.
- The names are known to `quillo check` and the editor. They are the browser's: on the
  server, while a component renders, `Chart.version` and `Chart.new(...)` give null, as
  `js(...)` does, so call them in `@mount` and event handlers.
- Mistakes say what to do: `Chart(el)` for a class stops with "Chart is a class: make
  one with Chart.new(...)"; a name in the wrong case gives an error value,
  `js_new: chart is not defined on this page (did you mean Chart?)`; so does a script
  that did not load.
- Without `module`, `client` takes a library that sets a global (a "UMD" or "browser"
  build: `.umd.js`, `.min.js`, `.browser.js`). An ES module is below; an npm package
  needs a CDN that serves it as one.

An ES module (`import`/`export`) gives names by exporting them, and `module` after the
first word takes them. Nothing goes on `window`: what the module exports is kept in Quillo's runtime, where
the page's scripts cannot reach or replace it, and is called as any library is:

```quillo
client module "https://esm.sh/chart.js@4.4.1" use Chart, registerables
preload module "https://esm.sh/chart.js@4.4.1" use Chart as ChartJS
client module "https://esm.sh/dayjs@1.11.13" use default as dayjs
client module "https://esm.sh/three@0.170.0" use * as THREE
client /assets/helper.mjs use helper

component Scene() {
    <canvas onmount={(el) => {
        let scene = THREE.Scene.new()
        scene.name = dayjs().format("YYYY")
    }}></canvas>
}
```

- Each name is an export, and `as` gives it another name. `default as Name` takes the
  default export, and `* as Name` the whole module (`THREE.Scene.new()`,
  `Name.default(...)`).
- A file ending in `.mjs` is a module without saying so. A `.js` file or an address may be
  either, so `module` says which.
- With `preload`, the page has `<link rel="modulepreload">`, and with `preconnect` a
  connection made for CORS. A module at another address needs its server to send
  `access-control-allow-origin`, as the public CDNs do, and its own imports must be
  addresses too: a CDN such as esm.sh or jsDelivr's `/+esm` rewrites an npm package's.
- A page's components start once its modules are in, so a slow module holds them back,
  and what is done on the page meanwhile is done when they start.
- `quillo check` cannot read a module at an address, so a wrong name is known in the
  browser: an error value where it is used, `gone: /x.js has no export \`gone\``, or
  `/x.js did not load: ...`, and the rest of the page runs. Calling a class without `.new`
  and the rest say what they do for any library.
- `js("Chart")` finds the module's `Chart` too, ahead of a `Chart` on the page. Anything
  else `js` and `js_new` find from the page's globals.

Without `use`, a page's own `<script>` tags work too, and `js` and `js_new` reach what
they define by name:

```quillo
component Sales(points: list) {
    state chart = null
    <canvas onmount={(el) => { chart = js_new("Chart", el, { type: "line", data: points }) }}></canvas>
}

route GET / = <html><head><script src="/assets/chart.js"></script></head>
                    <body><Sales points={load_points()}/></body></html>
```

- `@mount={(el) => ...}` runs once the element is on the page, once in its life (on a
  server's element too, when the component is resumed). A function it returns runs when
  the element leaves the page, or its component does. That is the only lifecycle hook:
  what runs when state changes is a statement that reads it.
- `autofocus` on markup made later (a dialog's field, a row added) is focused once it is
  on the page, the first of several, unless someone is typing in another field; the
  browser itself focuses only what the page had as it loaded.
- A Quillo function handed to the page's JavaScript (an event listener, a callback) is
  called with as many arguments as it declares, so `marker.on("click", () => ...)`
  works, and `off` finds what `on` was given.
- `js("path.to.fn", args...)` calls a function of the page's scripts, found by its dotted
  name from the page's globals (`js("L.map", el)`, `js("analytics.track", "signup")`),
  and gives its result; `js_new("Chart", ...)` makes an object of a class. A library's
  name from `use` is the same call written as JavaScript reads, with the same checks.
  The name is written in the code: one from data could reach anything on the page, so a variable is
  an error when the program loads, as are `eval`, `Function` and the like.
- A map reaches the page's JavaScript as the object it is, but one holding a key that is
  an array index (`"0"` to `"4294967294"`) out of JavaScript's order: that one reaches it
  as a plain copy, its keys in JavaScript's order, so `URLSearchParams`, `Headers` and
  deep-equality libraries take it, and what page code changes in that copy does not
  show in Quillo. What a program's function called by page code returns is handed over
  the same way.
- Objects from the page (elements, a library's objects) have their properties and
  methods as fields: `el.value`, `chart.update()`; and their fields can be set:
  `el.value = ""`, `el.scrollTop += 100` (only theirs: a map's fields are made anew,
  with `set` or `{ ...m, name: v }`). A plain `fn` may set them too, on a parameter of
  no declared type that a component hands it (`fn paint(ctx) { ctx.fillStyle = "red" }`,
  called as `paint(el.getContext("2d"))`), so drawing code is shared by components;
  run on the server, it is an error there. `innerHTML`, `outerHTML`, `srcdoc` and `on...` are
  not set, and nothing is set to a `javascript:` URL. What runs text as code or HTML is
  refused: `eval`, `Function` and its kin, `setTimeout` and `setInterval`,
  `insertAdjacentHTML`, `document.write`, `DOMParser`, `setAttribute("on...")`, script
  elements, nodes of another document, `javascript:` URLs (in lists and maps too) given
  to the page's functions, and what steps around these checks (`Reflect.set`,
  `Object.assign`, `Function.prototype.call`). Markup in the component does all of that
  safely.
- These checks are a guard-rail against mistakes, not a security boundary. Code that
  calls into the page can do what the page's scripts can, and a program set on it can
  find ways through (a library function that writes HTML, a function of the page
  reached inside data the page gave back, a `blob:` page opened in a frame). Keep data
  out of what `js` calls, as you would in JavaScript.
- A Promise a page's function gives (`navigator.mediaDevices.getUserMedia`, `fetch`) is
  waited for where code can wait, as a `server fn` call is: in an event handler, in
  `@mount`, in a `state` initializer, and in the component's `let` functions that its
  markup does not call (`let locate = () => { ... }`, then `onclick={locate}`): `let cam =
  js("navigator.mediaDevices.getUserMedia", { video: true })` is the stream, or an
  error value if it was refused. The same holds for a method of what the page gave
  (`img.decode()`, `audio.play()`) and for a function given as a prop, so there is one
  rule: a call of the page's that gives a Promise is waited for where code can wait.
  Elsewhere (a function `map` calls, a function the markup calls, a statement that runs
  again, a top-level `fn`), with `.then`, `.catch` or `.finally` after it, and for what
  `js_new` makes (`js_new("Promise", ...)`, kept to be used later), it stays a Promise:
  `js("load").then((x) => { data = x })`. `type_of` calls one `promise`. A function of the
  program's that the page calls with `undefined`, as `.then` does for a Promise of
  nothing, is given `null` (FEEDBACK28.md, Q-336). To wait for what `js_new` makes, and
  catch its failure, wrap it: `js("Promise.resolve", js_new("Promise", ...))`.
- A handler pauses only where something waits. A block in it (`match`, `if`, `for`) is
  made to wait only when something inside it waits; a call of a module's function
  (`date.`, `re.`, `money.`, `math.`, `stats.`, `vec.`) is never waited for; and a call of
  a function compiled here (the component's own, or a top-level `fn`) that is known to
  give no Promise is not waited for, so it can be given to `map`, `filter` and `reduce`.
  A state started from a module's function is set at once, in a component made later
  too. A component's own function named like a method of the page's objects is not
  waited for outside waiting code, so a Promise's executor that throws rejects it.
- A `let` set from the page (`let w = js("window.innerWidth")`, `js_new(...)`) is null
  while the server renders, so a component that shows it in its markup is made again in
  the browser; `quillo check` warns of it: write `state w = js(...)`, which runs again
  in the browser (below).
- `js_has("navigator.vibrate")` *(since 0.7)* says whether the page has a feature without calling it,
  as `js(...)` would; `false` on the server (Q-341). A field's `selectionStart` counts
  UTF-16 units, as JavaScript does, and `slice`, `len` and `s[i]` count characters:
  `cursor_index(text, el.selectionStart)` *(since 0.7)* turns the one into the other (Q-344).
- A `state` initializer that reads the page (`state wide = js("window.innerWidth") > 800`)
  gives null while the server renders the component, and runs again in the browser
  once the component is on the page, which sets the state from what it gives. An
  image's `@load` that came before the code is sent to the image once the code runs
  (its `e.target` is the image), and runs once. A frame or a player with `src={...}` is
  asked for once, unless it has a listener other than a click: that one starts over when
  the code runs, so that it hears its `@load` (FEEDBACK61.md, Q-893).
- A name the page does not have (a library's `<script>` left out) gives an error value,
  `js: L.map is not defined on this page`, so a component can say so instead of stopping.
- On the server, `@mount` does nothing and `js` gives null.
- `js` names what it calls in the code (`js("Chart", ...)`, not `js(name, ...)`), which
  `quillo check` says as loading the program does.
- There are no npm packages, `import`s or TypeScript: a library is a script the page
  loads (named with `use`, or a `<script>` tag), and component code reaches it by name. `js("fetch", url)` works, but `http.*`
  is the way.

### Single-page apps

`url()` gives `{ path, query, hash }`: of the request on the server, and of the address
bar in the browser, where what reads it follows it when it changes. A browser never
sends the hash, so on the server it is `""`, and a component that shows something by
the hash is made again in the browser. A query key given twice keeps the last. Once a
component has called it, clicks on same-site links inside components change the page
without reloading; Back and Forward work. `navigate("/path")` does the same from code;
on a page where no component has called `url()`, it loads that page as a link would.
(Links with a modifier key, `target`, `download` or `data-reload` work as usual.) A link
or `navigate("#spot")` to a spot on the same page goes there as the browser does:
scrolled into view, and CSS `:target` set. `navigate("/page#spot")` goes to the spot
once the new page shows it, and stays at the top if it never does. A `javascript:` or
`vbscript:` URL, however it is written (`  JaVaScRiPt:`), is an error, in the browser
and on the server, as it is for `redirect`: `navigate(next)` after a sign-in, with
`?next=javascript:...`, would have run as the page *(since 0.7)*.

```quillo
component App() {
    state cart = []                               // survives moving between pages
    let path = url().path
    <div>
        <nav><a href="/">Home</a> <a href="/products">Products</a> Cart: {len(cart)}</nav>
        {match split(path, "/") | filter((s) => s != "") {
            [] => <Home/>
            ["products"] => <ProductList/>                      // loads with state items = list_products()
            ["products", id] => <ProductPage key={id} id={int(id)} on_add={(p) => { cart = cart + [p] }}/>
            _ => <NotFound {path}/>
        }}
    </div>
}

route GET /*rest = <html><body><App/></body></html>       // every path renders on the server
```

And the pages it shows, with the `server fn`s that give them their data:

```quillo
let products = [{ id: 1, name: "Kettle", price: 30 }, { id: 2, name: "Teapot", price: 25 }]

server fn list_products() -> list = products | map((p) => { id: p.id, name: p.name })
server fn load_product(id: int) -> map? = products | find((p) => p.id == id)

view Home() = <h1>Welcome</h1>
view NotFound(path: str) = <h1>No page at {path}</h1>

component ProductList() {
    state items = list_products()
    {items == null
        ? <p>Loading…</p>
        : <ul>{for p in items => <li key={p.id}><a href={`/products/${p.id}`}>{p.name}</a></li>}</ul>}
}

component ProductPage(id: int, on_add) {
    state product = load_product(id)
    {product == null
        ? <p>Loading…</p>
        : <div>
              <h1>{product.name}</h1>
              <button onclick={() => on_add(product)}>Add to cart, {product.price}</button>
          </div>}
}
```

Opened at `/products/2`, the server sends the Teapot's page whole. Clicking Products, then
Kettle, changes the address and the page without loading one, each page showing
`Loading…` until its call answers; Add to cart makes the cart 1, and Back keeps it.

Pages load their data with `server fn`s in `state` initializers: on the server while
rendering, in the browser when you navigate to them, where the state is `null` until
the answer comes (show something for `null`). Give a page component a `key` from
its URL (`key={id}`) so a new product starts with fresh state. `navigate` scrolls to
the top; links outside the program's mount, and under `/_quillo/`, load as usual. A
full page load starts every state afresh: keep what must survive one on the server, or
in `localStorage` through `js`. See
[spa.qwe](https://github.com/quillolang/examples/blob/main/spa.qwe), which also answers 404 on the server for unknown paths.

### Live data

`live(load, ...args)` makes component state follow the server: `load` (a `server fn`)
runs again, and what shows the state follows it, whenever something it read changes, for
anyone, not only when this page acts.

```quillo
db.open("notes.db")
db.exec("create table if not exists notes (id integer primary key, text text not null)")

server fn load_notes() -> list = db.query("select id, text from notes order by id")

server fn add_note(text: str) -> map = db.exec("insert into notes (text) values (?)", text)

component Notes() {
    state notes = live(load_notes)  // every open page sees new notes
    state draft = ""
    <div>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
        <form onsubmit={() => { add_note(draft); draft = "" }}>
            <input value={draft} placeholder="A note"/>
            <button>Add</button>
        </form>
    </div>
}

route GET / = <html><body><Notes/></body></html>
```

The form calls `add_note`, and nothing sets `notes` by hand: the insert writes `notes`, so
every page whose loader read it runs `load_notes` again, this one included, and the list
grows on each. A note added from anywhere else (another tab, another visitor, a
`POST /_quillo/rpc/add_note`, a route) shows the same way.

- `live` takes a `server fn` by its name (not a lambda, not a variable holding one),
  and in a component only as a `state` initializer: `state notes = live(load_notes)`.
  The checker refuses other forms. In a route or a view it calls the function once and
  follows nothing: live data is a component's.
- Its arguments are followed too: `live(load, id)` runs `load` again with the new `id`
  when the prop or state it came from changes, and an answer still on its way for the
  old one is dropped. They are sent as JSON, so they must be data. Until the new answer
  comes, the state keeps the old one: `waiting(grid)` is true meanwhile (and while a
  page the browser made waits for its first), so a grid titled December can say that it
  still shows November: `<table class={waiting(grid) ? "stale" : ""}>`. On the server it
  is false, since the server renders with the answer.
- What a loader *reads* is recorded: the tables in its queries (`from notes`,
  `join users`, in subqueries too; a schema's prefix is dropped, so `a.notes` and
  `b.notes` are one table) and the global variables it uses (`todos`; a global holding
  a function is not followed). What any code *writes* is announced: `insert`, `update`,
  `delete`, `truncate`, `create`, `alter` and `drop table` through `db.*`
  (`insert ... select` writes one table and reads another), and assignments to globals.
  This works the same with SQLite, PostgreSQL and MySQL, and with data kept in variables.
  Not seen: what a trigger, a stored procedure or a `copy` writes, writes to the tables
  behind a view a loader reads (the view's name is what is recorded), and MySQL's
  `delete t from t join u` form. Publish a topic for those.
- Topics have names: a table is `db:notes` (`db@55c8e71313e4:notes` on a connection from
  `db.connect`, by a label made from where the database is, the same for every spelling
  of it: see [Several databases](#several-databases)), a global `var:todos`, and `publish("thread:7")` what it
  says. With tenants, every topic but a global's belongs to the tenant, so a `publish`
  wakes only the same tenant's pages; in a mounted program they are the mount's.
  `watch` and `publish` take several topics at once; `unwatched(f)` calls `f` with
  nothing. A topic is 1 to 256 bytes of any characters, a comma too; an empty or longer
  one stops `watch` and `publish` with an error, where the page never heard of it.
- A topic ending in `:*` is a wildcard: `publish("room:*")` wakes every page following a
  topic under it (`room:7`, `room:lobby`), and `publish("db:*")` every table's. So a
  topic built from a visitor's text must not end in `:*`: write `` `room:${id}` `` with
  an `id` the program checked (an int), not one taken as it came.
- A loader that gives an error value makes the state that error value, sent whole; one
  that fails with a runtime error, or `reply`s a status that is not a success, leaves
  the page's state as it was (the console says so), and the next change tries again.
- A page may follow at most 256 topics; past that, the rest hear nothing. A list too
  long for an address goes after it, so long topics are heard too.
- Inside `db.transaction`, a table's announcement waits for the commit, and is dropped
  on rollback. A global's waits for the outermost transaction to end, whatever its
  outcome, since a global keeps its value through a rollback: pages are told it, where
  they kept showing the old one.
- `publish("topic")` announces anything else, and `watch("topic")` inside a loader
  subscribes to it.
- What a loader reads includes what the functions it calls read: a `me()` that looks up
  the `sessions` table makes every loader calling it run again when anyone signs in.
  `unwatched(() => me())` calls it without following what it reads, though what it read
  still makes the run the viewer's own. Keep what changes often but is not shown (view
  counts, last-seen times) in a table of its own, so writing it wakes no page.
- A field of a global map, read as `world.scores`, is a topic of its own
  (`var:world.scores`): a loader that shows it runs again when that field changes, not on
  every write to the map, even 20 times a second from a game's loop. Assigning the map
  announces each top-level field whose value changed (what `{ ...world, tick: t }` keeps
  is unchanged), and the map itself, for what reads it whole (`world`, `keys(world)`,
  `world.len`, a field it lacks). Deeper fields are not followed apart: `world.scores.ana`
  follows `scores`.
- A write to a table wakes every loader that read the table, whatever rows it read:
  a reply in one thread runs `load_posts` again on every open thread page. To follow
  one thread only, read the table `unwatched` and watch a topic of its own, which the
  code that writes publishes:

  ```quillo
  server fn load_posts(thread: int) -> list {
      watch(`thread:${thread}`)
      unwatched(() => db.query("select * from posts where thread = ?", thread))
  }

  server fn reply_to(thread: int, text: str) {
      db.exec("insert into posts (thread, text) values (?, ?)", thread, text)
      publish(`thread:${thread}`)
  }

  component Thread(id: int) {
      state posts = live(load_posts, id)
      state draft = ""
      <section>
          {for p in posts => <p key={p.id}>{p.text}</p>}
          <form onsubmit={() => { reply_to(id, draft); draft = "" }}>
              <textarea value={draft}></textarea>
              <button>Reply</button>
          </form>
      </section>
  }

  route GET /threads/:id(int) = <html><body><Thread id={id}/></body></html>
  ```

  A reply on `/threads/7` shows on every page open on thread 7; pages on thread 8 do not
  run their loader for it.
- A loader runs again as a call from the page, with the page's cookies, so `session()`
  inside it is the viewer's session. So what should happen when new data arrives for a
  viewer (marking posts or messages read) is done in the loader: a component's code
  cannot call a `server fn` when state changes, but the loader is that call. A page
  whose loader writes is not prefetched (see [Prefetching](#prefetching)); it is made
  when the link is followed. What a loader writes does not wake that loader again on its
  page, since its answer has it already (pages showing it elsewhere are woken), and a
  statement that changed no row (`update messages set seen = 1 where seen = 0`, once
  everything is seen) is not announced at all: a loader that marks what it shows as
  read runs once for each change, not for ever.
- The server renders the first value; the page keeps one WebSocket open
  (`/_quillo/live`, served by its own thread), down which the server streams each
  change to its topics (the names, with a sequence number; a ping every 20 s when
  nothing changed), and calls the loader again, so a page gets the latest value, not
  every change in between: changes made while a page was disconnected are caught up on
  when it comes back. A WebSocket, because browsers
  allow only six HTTP/1.1 connections to a host across all tabs: requests left
  waiting would hold them, and server calls would queue behind them. Where a
  WebSocket cannot open, the page asks the same URL as a plain request instead,
  answered at the first change or after 25 s, and asked again at once. A connection
  that drops is opened again after a second or so, spread out, and after a restart pages
  ask for their data over a second, so that they do not all come at once; when the
  server cannot be reached, the page waits longer each time, up to half a minute. A tab
  in the background keeps its connection and runs its loaders as a visible one does. A
  page on another site cannot open it (403).
- A server keeps at most 4096 such connections open (live data, [`--watch`](#options-of-quillo-run) reloads,
  channels over a WebSocket), and at most 256 for one client: an IPv4 address, or an
  IPv6 /64 network. Channels over WebTransport are not counted. Past that, a page is
  answered `503`, keeps what it shows, says so once in the console, and asks again when
  told. The server says so once in its log, and `quillo_live_refused_total` counts them.
- **Behind a proxy, name it.** Every visitor is then the proxy's address, so without
  this only 256 pages get updates. Start the program with [`--trust-proxy`](#options-of-quillo-run), naming the
  proxy, `quillo run main.qwe --trust-proxy 127.0.0.1` (addresses or networks,
  comma-separated): it believes `X-Forwarded-For` on connections from those: the client is
  the nearest address in it that is not one of them, for this limit, for `request.ip` and
  everywhere else. From any other address the header is ignored, as a client can write
  it. When one address really is many visitors, as an office is, [`--live-per-client 1000`](#options-of-quillo-run)
  raises the limit.
- Only changes made through Quillo are seen: a row changed by another program
  (`psql`, a script) does not wake pages.
- One change wakes the pages showing it, and no others, and they all ask at once. The
  first change goes to a page at once; what comes within a tenth of a second or so after
  it goes as one, so a burst of writes is a few refreshes, not one each.
  `live({ every: "2s" }, load)` asks for a gap of its own between a page's refreshes,
  as `500`, `"500ms"`, `"2s"` or `"1m"`, written here as it is.
- A loader's run is shared by the pages that ask, within two seconds, for the same
  function with the same arguments, `Host` and tenant while nothing it read has changed:
  fifty open boards run `load_board` once, and that goes for a page's first load too.
  Calls that come while a run is under way wait for it. A run that read the visitor
  (`session()`, cookies, `request`, the locale), wherever in the run it read it, or did
  anything but read (wrote, called a service or plugin, `crypto.token`, `uuid`, `random`,
  set a cookie, took a file, answered with `respond`), is shared with nobody, not even
  the visitor's other tabs, and nothing waits for it. Randomness made in SQL (`select
  hex(randomblob(8))`) is a read, so such a run is shared: make tokens with
  `crypto.token`. `nextval` or a writing function behind a view is not seen either: put
  it in the query.
- **What a thousand pages cost** depends on whether the loader is shared. A loader for
  everyone runs once a change however many pages show it. One that reads the viewer runs
  once for each page, on every change to what it read, so with many pages and a busy
  table give each viewer a topic of their own: `watch("user:7")` and an `unwatched` query,
  published by whoever writes. What is in the table does not matter, what the loader
  returns does: each run makes that value's JSON, so return what the page shows (`limit`,
  or `live(load_page, page)`), not the table. A loader giving over 4 MB is said in the
  log. `quillo_live_runs_total` tells runs made from runs shared.
- A page is sent what changed in the value, not the whole value again: the server keeps
  the values pages have (up to 64 MB with the patches, no value over 16 MB; past that,
  those few pages share go first), and answers a page with a patch to its own when the
  patch is the shorter. A patch does not go inside text, so one letter changed in a long
  field sends the field. Lists of rows are matched by their `id` (a number or text; the first of two
  alike counts), rows without one by place, then by content, and what is inside a row
  is patched the same way, so a card moved or renamed on a board of 3,000 costs that
  card. A map whose keys changed order, and a decimal, go whole. The patch applies to
  what the server last sent, so changes the page made to the state meanwhile (a card
  shown moved before the server says so) are replaced as before. An error value is
  never patched.

#### Several servers

To run an app on several Quillo servers behind [limer](https://limer.sh), let
them join a `discover` pool with a `broadcast` path; changes made through one
server then wake pages on all of them, with no message broker:

```
app.example.com {
    reverse_proxy {
        discover shop {
            allow 10.0.0.0/8
            broadcast /_quillo/announce
        }
    }
}
```

```sh
export LIMER_ADMIN_SECRET=...        # limer's admin secret
quillo run shop.qwe --port 8081 --register http://10.0.0.1:2019   # on each server
```

- Each server registers (and keeps registering, every 5 seconds; limer drops
  one that stops for 15) and sends the topics it changes to limer's
  `/broadcast`, which passes them to every member (`POST /_quillo/announce`, at
  most 64 KiB). The pool's name is the file's (`shop`), or [`--service`](#options-of-quillo-run). [`--register`](#options-of-quillo-run)
  needs a [`--port`](#options-of-quillo-run) written out, and cannot be combined with [`--instances`](#options-of-quillo-run) (nor can
  [`--watch`](#options-of-quillo-run)): run one instance per server, or several servers.
- limer's admin secret is required: `--register` does not start without
  `LIMER_ADMIN_SECRET`. Each announcement is signed (HMAC-SHA256, with a key made
  from the secret) together with the time and a random nonce; one that is not
  signed so, more than a minute old, or seen before is refused. A server takes
  only the topics it already knows (changed there, or shown on its pages), at most
  256 a message.
- What is shared is the database: tables (`db:` topics) and `publish` topics
  are announced. Data kept in variables belongs to each server and is not. Nor are
  channels and media streams: a broadcast reaches the pages on this server, and a
  camera's stream is watched through the server it comes to (see [Channels](#channels)).
- Sequence numbers are per server, and each carries its server's id. A page
  whose live connection lands on a different server (after a restart, or with
  round-robin balancing) loads its live data again once, rather than missing a
  change. `lb_policy sticky_cookie` keeps a browser on one server and avoids
  even that.
- Announcements are delivered once, without retries: a server that cannot be
  reached when one is sent misses it, and its pages show that change only when
  they next load the data (the next change to it, or a reload).
- Sessions need the same `QUILLO_SECRET` on each server.

### Server functions

`server fn` makes a function callable from the browser:

```quillo
server fn add_note(text: str) -> list ! str with db {
    if trim(text) == "" { return err("empty note") }
    db.exec("insert into notes (text) values (?)", text)?
    db.query("select * from notes")
}

component Notes(initial: list) {
    state notes = initial
    state draft = ""
    state problem = null
    <form onsubmit={() => {
        let r = add_note(draft)                    // a request to the server
        if is_err(r) { problem = r.error } else { notes = r; draft = ""; problem = null }
    }}>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
        <input value={draft}/>
        <button>Add</button>
        {problem && <p class="problem">{problem}</p>}
    </form>
}

route GET / with db = <html><body><Notes initial={db.query("select * from notes")}/></body></html>
```

The route gives the component the notes there are; pressing Add with the box empty shows
`empty note` under it, and with text the list shows the new note and the box empties.

In component code the call is awaited automatically. A call that fails (the
connection drops, the server errors) returns an error value, like `http.*` on the
server, so `let more = load_more() ?? []` carries on and can try again later. It works in event handlers and
in `state` initializers (`state todos = load_todos()`: the server runs it while rendering,
so the page arrives with the data). Calling one while rendering is an error.

The error value's text says what happened: ``server fn `x` could not be reached:
...`` when there was no answer, ``server fn `x` failed (403): ...`` when the server
refused it (400 with the argument that did not fit, 403 from another site, 404 for no
such function, 500 `internal error in x`, whose real message is on the server's
stderr), and ``server fn `x` answered something that is not its answer (text/html, 1234
characters)`` when a 2xx came back that the server did not send, or cut short: a sign-in page a
redirect led to, a proxy's page, an empty body. Quillo's server marks what it sends with
`x-quillo-answer`, so `respond` and `reply` still give `null` (a proxy that drops the mark turns those
into this error; ordinary answers go through). A `redirect` from the function itself is followed
by the browser, and gives this error too. A function that returns `err(v)` gives an error value with `v`. A `reply 401
"Log in first"` inside it gives only the status (``failed (401)``), since its body is
not JSON: return `err("Log in first")`, or `reply 401 { error: "Log in first" }`, for the
page to show the words. Nothing happens by itself on a 401 or a 500 (no redirect, no
retry, except for [idempotent](#idempotent-requests) calls); there is no loading flag
either: set a state of your own around the call.

**The latest call wins.** A handler can run again before its call has answered (a
letter typed in a search box, a second click), and answers come back in any order. An
older call's answer never replaces a newer value: every set of a state claims it (a
plain `shown = "x"`, a field bound with `bind:`, a set from a call). A set that carries an
answer the function waited for gives way when another run has set or claimed that state
since. It carries the answer if its value holds the call or a variable bound from it, or
if the answer decides it: it sits under an `if`, `match`, `while` or `for` on the answer
(or `&&`, `||` or `??` used as one), or after a guard that leaves on it (`return`, `?`). A
run is one handler, effect, timer or
frame callback, with the functions it calls; a handler fired by `el.click()` and each
`parallel` function are runs of their own. So
`oninput={(e) => { shown = search(e.value) }}` ends on the answer for what was typed
last, however late the first one comes.

Every other set is made: a plain one, like `saving = false` after the wait, a fold, and a
set keeping a state in step with one folded earlier in that same branch. A set that reads
the state it sets is a fold (`n += 1`, `busy -= 1`, `xs = xs + [x]`, `s = s | push(x)`,
`s = f(s)`), made on what the state holds then: three quick likes that each call `save()`
and then `likes += 1` count three, and two uploads that each do `busy += 1`, wait, and
`busy -= 1` end at 0.

In a fold with a wait in it, every part runs where it is written, and only the read of the
state moves after the waits: `n += count()` adds to what `n` holds when the answer comes,
and `n = n + check()? + charge(1)` makes no charge if `check()` stops. That is so for `+=`
and `-=`, and for `=` forms headed by the state (`n + f()`, a template, `...state` in a
list or map, `set` and `push`). Other shapes read the state first.

Only a set with an answer is ever dropped. Plain sets and folds are always made; from a run
that lost the state they take nothing, so the newest answer still lands. So clear a state
before the wait, not after it: in `let r = search(q); wipe(); shown = r`, an older run's
`wipe()` can blank what the newest answer showed. Write the set in the function that waited:
an answer handed to a helper as an argument counts as plain there.

In a function that waits, a hole of markup may wait too: `let el = <b>{load()}</b>`. It is
worked out first, before the markup is made and before the holes around it, once, and does
not follow what it read.

**Slow data: `defer`.** A `state` initializer runs while the server renders, so the
page waits for its slowest one. `defer` lets a slow one follow the page instead:

```quillo
component Product(id: int) {
    state product = load_product(id)                  // in the first bytes
    state reviews = defer(load_reviews(id))           // streamed when it is ready
    <div>
        <h1>{product.name}</h1>
        {reviews == null ? <p>Loading reviews…</p> : <ReviewList reviews={reviews}/>}
    </div>
}
```

- The page is sent at once with `reviews` null: what the component shows for null is
  there (a skeleton, "Loading…"). The rest of the answer comes when `load_reviews`
  does: the component rendered again, its other states as they were (`load_product`
  is not called twice) and `reviews` worked out, which a line of script swaps in. The
  answer is one response, so search engines read the whole page, reviews included.
- The browser takes the component up once its part has come, not before: until then
  it is HTML (links work, nothing else does). Its part comes as soon as it is ready,
  and before the page counts as loaded.
- Only a component placed in the page defers. Inside another component, and in the
  browser (a page made there after `navigate`), `defer(x)` is `x`: there a state that
  calls the server is null until it answers anyway. Components inside a deferring one
  render again with it, their initializers too.
- Several deferred components follow in the order they are on the page, each as soon
  as the ones before it are sent. A deferred part cannot set cookies or the session
  (the answer's head is gone): do that in the route or a server fn.
- Without a connection to stream to (a test calling `handle`), for a prefetch, or for
  a mounted program, the parts are worked out first and the page is sent whole. A
  runtime error in one is logged, and its placeholder stays.

For data a page should not wait for at all (another site's API that may take seconds),
render a placeholder and call the `server fn` from `@mount`.

There is no `async` or `await` to write: on the server a call simply waits (and other
requests run meanwhile); in the browser the compiler awaits it, and every function that
calls it. Calls run one after another. To run several at once, give them to
`parallel`, which returns their results in order:

```quillo
let [user, orders] = parallel(() => load_user(id), () => load_orders(id))
```

In the browser that is `Promise.all`. In a request on the server each function runs as a
coroutine of its own on the event loop (outside a request, on a thread of its own), so
their waits (database, `http.*`, `sleep`) overlap. Their computing does not: Quillo code
runs one piece at a time, so `parallel` shortens waiting, not arithmetic. Inside a
transaction they run one after another, since the others' SQL would not be part of it.
A function that fails stops the others at their next step (a call, a loop's round, a
builtin, a `sleep`, which ends at once), nested `parallel` too; what they did before it
stays done, and the call fails with the failure that happened. Error values are results
like any other and stop nothing. Inside the functions, `set_cookie`, `set_session`,
`end_session`, `session()`, `publish` and a replica's read-your-writes work as they do
outside, and what each sets goes into the answer in the functions' order, as if they had
run one after another; one that reads who the visitor is makes the answer private to them.

Each call is a `POST /_quillo/rpc/<name>` with a JSON array of arguments (or, when an
argument is a file, multipart: see [Uploads](#uploads)), checked against
the parameter types all the way in (a `list<Role>`'s items, a record's fields; a 400
otherwise), and records made as `User(v)` makes them (other fields left out); the answer is `{"ok": value}` or `{"err": payload}`.
Only `server fn`s can be called, only with `content-type: application/json`, and only
from the program's own pages: a request a browser marks as coming from another site
(`Sec-Fetch-Site`, or an `Origin` naming another host) gets a 403. Inside one, `request`
is the current request. `reply` in a
server fn becomes the HTTP response, so the usual middleware works:

```quillo
server fn delete_all() = request | auth.required | (_) => { ... }
```

From hand-written JavaScript: `await quillo.server.add_note("text")`, which resolves to
an object with `.error` rather than rejecting. `window.quillo` (`{ server, components,
rpc }`) is there once a program with `server fn`s has had its bundle loaded, which is
when a component is reached; a script that runs as the page loads includes `<script
src="/_quillo/quillo.js"></script>` first. A component cannot be mounted from a script:
components start from the server's HTML, or inside another component.

A call that must not happen twice (a payment, a transfer) is wrapped in
[`idempotent`](#idempotent-requests), in the `server fn` or a function it calls: the page
then sends a key with each call, and sends the call again when its answer does not come
back, and the server runs it once. Two clicks are one call: while a call runs, the same
call with the same arguments shares its answer and sends nothing, and a call that got no
answer keeps its key, so sending it again is safe. Any answer ends it, so the next
payment, even of the same amount, is a new one. A call with other arguments is another
call.

#### Tools for AI clients

`fn mcp name(...)` is a tool: a function an AI client (Claude, an editor's assistant) can call
through the program's MCP door, with its `///` as the description the model reads and its
types as the schema. `server fn mcp name(...)` is a server fn the program's pages call too. To
the program both are plain functions: a call from Quillo code is a plain call, with no checks
and no limits.

```quillo
/// The orders placed today, newest first.
fn mcp orders_today(limit: int = 20) -> list with db {
    db.query("select id, total from orders order by id desc limit ?", limit)
}
```

- The tag is `mcp` after `fn` (or `server fn`) and before the name, and `mcp` stays an
  ordinary name everywhere else: `fn mcp(x)` is a function named `mcp`, and `fn mcp mcp()` a
  tool named `mcp`.
- A tool is an address: `private fn mcp` is an error, as `private server fn` is. It is
  declared at the top level, and a `view` or a `component` is not a tool.
- Its name is the tool's: `A-Z a-z 0-9 _ - .`, 1 to 128 characters. A tool's name is the
  program's in every file, so two files cannot both declare `total`.
- A plain `fn mcp` is not a page's to call: `/_quillo/rpc/<name>` answers 404, as for any
  plain function.
- `quillo check` warns of a tool with no `///`, and of a parameter with no type (the model
  guesses). A tool whose body uses an effect it does not declare is warned too, the warning
  naming what it uses (`with time`): without a `with`, a client is told it may change anything
  and reach anywhere. A tool that uses none is fine as it is. A parameter that cannot come as JSON (`fn`, `html`, `response`) is an
  error, and so is an answer that can only be a page (`redirect`, `html`, `respond`): a tool
  answers with data. One that only may be a page is a warning.
- `service mcp /billing = "billing.qwe"` serves billing as `service /billing` does, and offers
  billing's own tools as the program's, named `billing.<name>` (at most 128 characters), after
  the program's own, with billing's schemas and hints. Both authors agree to it: billing says
  which functions are tools (`fn mcp`), the app whether they are offered here (`service mcp`).
  A plain `service`, a service's own services, and a service whose file is not here offer none.
  Their calls run in billing, in the same process or over the signed service call under
  `--split`; there `session()`, `tenant()` and `request.mcp` are the caller's (cookies and
  `set_session` set there go nowhere), and they count toward the caller's 60 a minute. Apart,
  a call of billing's tools and its answer are also signed with the app's key, which only
  billing and its app hold (`QUILLO_APP_KEY` where billing runs, `QUILLO_APP_KEY_BILLING` where
  the app runs, the same secret of at least 32 bytes, refused shorter; `--split` makes one of
  64 and sets both; `..._PREVIOUS` of each is accepted while keys change, never to sign). The
  signature covers the call, its caller, deadline and contract, so a call sent again is the
  same call, answered as before; only the app's door runs billing's tools, never a plain call
  from code, another service, a token or a certificate. Tool calls go only to copies of billing
  that proved its app's key when they joined (a copy with `QUILLO_APP_KEY` set signs its join
  with it; a wrong proof is a `401`). A `service
  mcp` whose auth signs nothing (`auth: { bearer }`, OAuth or mTLS without a key) is refused by
  `quillo check`. A tool's call that a joined copy did not answer is not run again here: it
  fails, as it may have run; and inside a tool's call that may ask, so does a plain call of a
  service's function that a copy did not answer, which counts as a change. A service that
  offers only tools is served under `--split`. Services must run this Quillo or later, so upgrade them before the
  app: an older one refuses a tool's call, and the app says so.
  The named form, `service mcp crm = "https://..."`, for calling another MCP server, is refused
  for now.
- The API document marks a `server fn mcp` with `x-quillo-mcp: true`.
- What a client is told: each tool's arguments and result as JSON Schema 2020-12, the dialect
  MCP requires (`str?` is `["string", "null"]`, a record once under `$defs` by a local `$ref`,
  `int | str` a list of types, `dec` and money's amount a text with a `pattern`). The
  arguments and the records they hold refuse other fields (`additionalProperties: false`); a
  result says no such thing, since the server does not check it. A typed result is always an
  object: one that is not (`-> int`, `-> list`) is held as `result`, in the schema and in what
  the client gets (`{ "result": 3 }`). A tool with no `->` type has no result schema, and its
  answer goes as text alone.
- Two hints. `readOnlyHint` is true when Quillo can see the whole of what the tool may run (no
  function held in a variable or a parameter, no `js`, `run_script` or native plugin), it
  writes nothing (no `db` write, no builtin that changes something, such as a session, a
  cookie, `publish` or `media.close`, no file, no top-level `let` it changes, none in the
  defaults of a record it takes, gives or makes, and no function a statement calls that its
  database does not ship, as `select sync()`) and its `with` has none of `fs`, `net` and
  `http`. `openWorldHint` is false when it reaches no other site: no `net`, no `db.connect` or
  `db.shards`, no such function, `http` only to the program's own services whose code is in the
  program and stays inside, and all of it seen. Otherwise each is MCP's cautious default. A
  client may skip asking the person before a read-only tool, so Quillo says so only when it is
  true.
- "Only reads" is worked out, with no word to write: every `db.query` and `db.one` the tool
  may reach has a statement written in the program that starts with `select`, only reads, and
  calls only the functions its database ships (aggregates, text, numbers, dates, JSON,
  conditionals). A `with ... insert`, `insert ... returning`, `select ... for update`, a call
  of any other function (`dblink`, `sync()`) or a statement built at run time counts as a
  write. A read-only tool that may reach the database (its `with` names `db`, or it has none
  and calls `db`) runs inside a read-only transaction, so a write Quillo did not see is
  refused by the database (`db.error_kind` gives `"read_only"`); one that reaches none runs
  with none. Its arguments are made inside that transaction and under its `with`, so a
  record's default that needs an effect the `with` lacks fails as the body would
  (FEEDBACK56.md, Q-858).

**`quillo mcp app.qwe`** serves the tools to a client on the same machine, which starts it:

```json
{ "mcpServers": { "notes": { "command": "quillo", "args": ["mcp", "/home/ana/notes/main.qwe"] } } }
```

It checks the program (one that does not check does not start), runs its top level
(`db.open`, tables), then answers MCP on stdin and stdout, one JSON message a line. Routes are
not served. Only MCP's messages reach stdout: `print` goes to stderr under `quillo mcp`, and on
Unix whatever else the program or a plugin writes to stdout goes there too. `stdin`, `read_line`
and `exit` are errors. It ends when stdin closes, and on SIGTERM: at once while it waits for a
message, and once the call in progress is answered while it answers one, however long that
takes, saying so every 5 s (`--drain 30s` cuts the call off after that long; a second SIGTERM
ends it at once). There are no tokens on stdio: the person who starts it is the caller.

Quillo speaks MCP's revision 2026-07-28 only. A client of the earlier revision, which opens with
`initialize` (Claude Code before 2.1.319, for one), is told the version is not supported. What a
client gets from a call:

- A typed result: `structuredContent` (held as `result` when it is not an object) and the same
  as JSON in a text block. An untyped one: the text block alone.
- `isError: true`, with text, for a tool's `err(v)`, an argument the door refuses (with the
  message `/_quillo/rpc` gives), a `reply` or a `respond` (with its text, or its status such as
  `404 Not Found`; a 2xx too, which `quillo check` warns of in a tool's code: a tool answers by
  returning its value), a page, and `internal error` when the tool failed (the real message on
  stderr).
- The protocol's own errors for an unknown tool, a malformed message and the wrong version.
- An answer past what the door takes in (10 MiB of JSON, or 128 levels deep) is an `isError`
  saying so, never a cut or empty answer; a tool that crashes answers `internal error` and the
  door goes on serving.

A policy's `max_compute` and `max_body` hold `quillo mcp` as they hold `quillo run`. On
Windows, `print` goes to stderr, but other writes to stdout (a `.qwer` plugin's own output) are
not moved there, and break the session; it stops on Ctrl-C, Ctrl-Break or the console
closing, as on SIGTERM elsewhere.

**`/_quillo/mcp`** is the same door over HTTP, served by `quillo run` when the program has a
tool (a program needs no route to be served: one of tools alone is served, its door and, under
`--watch`, its token; under `--base /shop`, at `/shop/_quillo/mcp`; a program run `--as-service` has none: its
tools are its app's). It takes `POST` only, one JSON answer a request, and checks each one as
MCP 2026-07-28 says (the version, the `MCP-Protocol-Version`, `Mcp-Method` and `Mcp-Name`
headers against the message). Who may call:

- **A signed-in session**: the program's session cookie, with a `user` in it. Another site's
  `Origin` or `Sec-Fetch-Site` is refused (`403`), and the door sends no CORS headers, so a page
  on another site cannot drive it from a visitor's browser.
- **A token**: `Authorization: Bearer ...`. `mcp_token()`, called in a route where someone is
  signed in, makes one tied to that session and gives it once, for them to paste into their
  AI client. It lasts as long as the session has left when the program gave the session a
  `max_age`, else 30 days (`mcp_token({ max_age: 7 * 24 * 3600 })` says otherwise; a longer
  one than 366 days is cut to 366; `mcp_token({ user })` for a program that names its caller another way). Quillo keeps
  only its signature under the session key (`QUILLO_SECRET`), in `_quillo_mcp_tokens` (or in
  memory for a program with no database: at most 100 a user and 10,000 in all, `mcp_token()`
  refused past either), and seals each row as it seals a cookie: a row
  changed in the database is no token. The first `mcp_token()` makes that table, and fails
  saying so where the database user may not create one. A database the program has that will
  not open is an error, never memory: `mcp_token`, `mcp_tokens` and `revoke_mcp_token` fail, a
  call with a token is a `503`, and `end_session()` fails while the door is open (the program
  has tools and is serving them).
  `mcp_tokens()` lists the tokens made in this session, or for its `user`, also
  after its cookie has gone or on a new sign-in, and `revoke_mcp_token(id)` ends one
  (`{ user }` reaches all of that user's, as a token a session made for someone else is reached);
  `end_session()` ends the tokens made in that session for its user, whether the door is open
  or not.
- Neither is a `401` with `WWW-Authenticate: Bearer`.

In a tool, `session()` and `tenant()` are the caller's: a token's are those of the session it
was made in, as they were then (a session is a signed cookie, kept nowhere else), so check
what can change, a role, in the database on each call. Cookies a tool sets go back to a
cookie caller and are dropped for a token's. `request.mcp` is `{ client, version }` from
what the client says of itself, and null on any other request; it is read where `request` is,
in a `server fn mcp` (a plain `fn mcp` has no `request`). Each user may make 60 tool
calls a minute, the cookie and their tokens together, per tenant (a caller with no `user`:
per session or token) ([`--max-tool-calls`](#options-of-quillo-run), and a policy's
`[limits] max_tool_calls` can lower it); one more is a `429` with `Retry-After`. Under
`--watch` on 127.0.0.1, `quillo run` prints a token for the door (one with no one signed in:
`session().user` is null in a call made with it), at
`http://127.0.0.1:PORT/_quillo/mcp` (without `--base`), and the line that adds it to Claude
Code, when it starts or once the first version that loads has tools (FEEDBACK56.md, Q-853,
Q-859, Q-860).

**Asking the person: `confirm` and `about`.** A tool can ask the person at the AI client
before it acts. `confirm(question)` gives their yes or no, and `about(question, choices,
default)` the key of the choice they picked (`choices` maps a key to the text they see, and
`default`, one of the keys or null, is always written):

```quillo
/// Publishes a post. It is public at once.
fn mcp publish_post(title: str, body: str) -> int with db {
    let who = about("Who can see it?", { public: "Everyone", draft: "Only me, as a draft" }, "draft")
    if !confirm(`Publish "${title}" now?`) { return err("not published: the user said no") }
    db.one("insert into posts (title, body, draft) values (?, ?, ?) returning id", title, body, who == "draft").id
}
```

- The call stops at the first question: the client is sent a form (MCP's `input_required`),
  asks the person, and calls the tool again with the answer. The function then runs again
  from the top, so the first question gives its answer and the next one is asked, up to 100
  questions or rounds in a call (a question with `now()`, `random()` or `uuid()` in its text is
  a new one each round, and ends the call there). The same text asked in two places in the code
  is two questions. A `confirm` is a box to tick: sent back unticked, it is a no.
- So a tool asks before it changes anything. `quillo check` refuses a tool that it sees change
  something before a question. As it runs, a question after any change (a database write, a
  file, a request out, a cookie or the session, the program's variables, another database, a
  plugin, a service) is refused and the call stops (the model is told `internal error`, the
  message is in the log); the change stands, so this is for code the checker cannot see whole.
  `??` does not turn these refusals into a value. A change on a way that leaves the function
  first (`if id == 0 { save(0); return false }`, or a `reply`) is no change before a question
  after that `if`; it is still its function's change for whoever calls it.
- A question the checker does not see a tool ask, as in a function given to a builtin (a
  `locale_from` or `tenant_from` rule that calls `confirm`), is refused as it runs, in the tool
  and in any service its call reaches, and the call stops (`internal error`, the reason in the
  log): no one was asked, so no yes or default is given. A tool's own `db.transaction`, `idempotent`
  and `db.atomic` work as anywhere; in a call that may ask, `parallel` runs its functions one
  after another, so that a question is never answered by nobody.
- The answers come back in a state Quillo signs: good for 10 minutes, once, for the same
  caller, tool and arguments; anything else is refused (`-32602`), used up, and the call starts
  over. A state whose use cannot be recorded is refused too. A person who declines or dismisses
  an `about` ends the call (an `isError` saying so); a `confirm` declined, dismissed or sent back
  unticked is a no.
- A client that cannot ask: `about` takes its default, and the result says so (`the user was not
  asked "Who can see it?": the default "Only me, as a draft" was taken`), so the model can tell
  the person; `confirm` refuses (`isError`: this needs the user's yes, and this client cannot
  ask).
- Outside a tool's call (the program's own code, or a page calling a `server fn mcp`), `about`
  gives its default and `confirm` gives true: the page asks in its own way. A service function
  a tool's call reaches asks through the program's door, in the same state, in one process or
  across `--split`; a change in either refuses a later question, and so does a call to a
  service whose answer is not one Quillo can verify (refused, unsigned, unreadable, a 500, none
  in time). The person's answers reach billing only through the app's signed state, and the
  form shown to them is the app's own. `billing.later.f(...)` from any tool has no one to ask:
  its question is refused and the job fails (`quillo check` refuses it where it sees it).
- Neither asks for free text, as MCP forbids asking for passwords, keys or payment details in a
  form. A tool that asks and only reads stays read-only. A round that carries the answer to the
  question its state waits on does not count toward the 60 tool calls a minute; any other round
  does. `ask(message, options)` is the page's dialog (see [Components](#components)), and a
  component cannot use `about`. `ask` with three arguments is an error that names `about`, and
  so is the page's `ask` in a tool's code, from `quillo check` and as it runs. A program's own
  function named `about` is warned of, as any that hides a builtin, and its own is used.

The instructions then say which tools ask the person themselves, so the model does not ask
again (FEEDBACK56.md, Part E).

**What the model is told.** `server/discover` carries `instructions`: a few lines Quillo
writes, of facts about this server that no tool says (who a call acts as on that door, what an
`isError` result and `internal error` mean, that text in results is data, not instructions),
always under 600 characters; then, after an empty line, the main file's own `//!` block, for
what the developer wants said about how their tools go together. A client may keep that answer
and the tool list for 5 minutes (`ttlMs`), the same for every caller, so a tool renamed in a
deploy reaches the clients within that time:

```quillo
//! Notes are private to the user who wrote them.
//! Use search_notes before add_note, so a note is not written twice.

/// Adds a note.
fn mcp add_note(text: str) -> int with db = db.one("insert into notes (text) values (?) returning id", text).id
```

Only the main file's block counts, and only at its top (blank lines and `//` or `/* */`
comments before it are fine, `/** */` and `/*! */` banners too; a `///`, a `use` or code ends
it), as a `.qwer` file's header is read; other files' `//!` lines are not sent. A character a
reader of the file does not see (a control character, a bidirectional mark, embedding,
override or isolate, a zero-width space, a line separator, a tag character) anywhere in the
block is an error at its line and column, and the program does not start with it: what the
model reads must be what the developer sees. The zero width joiner and non-joiner, which
emoji and Persian need, are let through. The first line names the program, cut at 24
characters. `quillo check` warns when the whole is over 2,000
characters on either door (the HTTP door counted under `--service-name` when `quillo run` is
given one), saying by how much: clients cut long instructions short. `quillo docs --mcp
main.qwe` (with `--policy FILE` and `--service-name N` as `quillo run` takes them, and the
policy beside the program) prints what each door sends, `server/discover` on stdio and over HTTP and
`tools/list`, as JSON, so the developer reads what the model reads; it says as much as
`/_quillo/describe` does, so keep it private (FEEDBACK56.md, Part D).

Limits: the count of tool calls is kept by each server process, so under `--instances` or on
several machines each counts its own; and a program with no database keeps its tokens in that
process's memory (100 a user, 10,000 in all), lost at a restart and unknown to the others, so
run such a program as one process. The same holds for the states of `confirm` and `about` over
HTTP with no database: each process keeps the states it took, so with several processes sharing
`QUILLO_SECRET` the same caller can use a state once in each of them, for the same tool and
arguments, until its 10 minutes end; with a database they share, once in all. A session cookie made before this version does not say it
was given a `max_age`, so a token made from it lasts 30 days.

#### What the browser can reach

Components and server code share a file, but not what they can touch:

- A component cannot use `db`, `request`, `env`, files, Rust (`.qwer`) functions,
  or a global the server changes: that is an error when the program loads, and in the
  editor. Top-level values that come from them (`let KEY = env("STRIPE_KEY")`,
  `let CONFIG = load_config()`) are not written into the browser's bundle either, which
  anyone can download: a component that uses one is an error, and asks a `server fn`.
- An environment variable named `PUBLIC_...` is for the browser: `env("PUBLIC_MAP_TOKEN")`,
  with the name written out, works in a component and in a top-level value it uses, and
  is written into the bundle as it was when the program loaded. That suits the keys
  services hand out for pages to use (a map's token, a search-only key, an error
  reporter's address); a secret keeps its own name, and stays on the server.

  ```sh
  # .env, beside main.qwe
  PUBLIC_MAP_TOKEN=pk.test123
  STRIPE_KEY=sk_live_xyz
  ```

  ```quillo
  component Map() {
      state token = env("PUBLIC_MAP_TOKEN")              // written into the bundle
      <div class="map" data-token={token}>Map, token {token}</div>
  }

  server fn charge() -> bool = env("STRIPE_KEY") != null         // a secret stays on the server
  ```

  The page and its bundle hold `pk.test123`, and nothing of `sk_live_xyz`. Reading
  `env("STRIPE_KEY")` in the component instead stops the program as it loads (and `quillo
  check`): `` `env` is not available in the browser, where <Map> also runs; use a `server
  fn` (or name the variable PUBLIC_..., which says the browser may see it) ``.
- `http.get`, `http.post` and the rest work in a component's event handlers, `@mount`
  and the functions they call: the request goes from the page, with the browser's rules
  rather than the server's. The other site must allow this one (CORS: it answers with
  `access-control-allow-origin`), or the call is an error value that says what it may be
  (`http: no answer from api.example.com: it may not allow this page (CORS), or be plain
  http, or be unreachable; a server fn can call it from the server`). To reach your own
  server from a page that is not its own, a phone app's, call it from a `server fn` (the
  program's `http.*`, or [`site`](#calling-another-quillo-site)); the page's cookies are
  not sent to it (a path of the program's own, `/api/x`, goes to its own server, with
  its cookies); and it reaches what the visitor's machine can, such as a service on
  their own computer or network, not what the server can. Only `redirects: 0` and
  `timeout` of the options mean something there. Nothing goes through the
  server, which suits a site that limits each visitor, one that answers by where the
  visitor is, a visitor's own token kept in the page, and large files:

  ```quillo
  server fn upload_url(name: str) -> str {
      let me = session().user ?? reply 401 "Log in first"
      presign_put(me, name)                              // your bucket's signed URL
  }

  component Upload() {
      state done = false
      <input type="file" onchange={(e) => {
          let f = e.target.files[0]
          done = http.put(upload_url(f.name), f).ok      // the file, as it is, to the bucket
      }}/>
  }
  ```

  A file from an input is sent as it is, with its own type. While the server renders a
  component and runs its `state` initializers, nothing goes out: an `http.*` call there
  is an error (when the program loads where it can be seen, as the page renders where
  it cannot), since it would come from the server's network. Ask a `server fn` for what
  the first render needs. Answers from another site show only the headers it allows.
- A `server fn` is the way in, and it is a public address. Anyone can send
  `POST /_quillo/rpc/<name>` with any arguments, from a script or the browser's console,
  signed in or not; refusing other sites' pages stops other sites, not people. So each
  one checks, as a route does, who is asking and whether they may touch *this* record,
  and gives its parameters types (one without a type takes any JSON):

  ```quillo
  server fn delete_note(id: int) -> map {
      let me = session().user ?? reply 401 "Log in first"
      db.exec("delete from notes where id = ? and owner = ?", id, me)
  }
  ```

- What a `server fn` returns, and the props a page gives a component, reach the browser
  in full: `db.one("select * from users where id = ?", id)` sends `password_hash` too.
  Select the columns the page shows.
- A `server fn` that calls another site for the browser passes on what it is given: with
  a URL it takes from its arguments, anyone could have it fetch any address the server
  reaches. Write the host in the code (`` `https://api.example.com/users/${id}` ``), or
  pass `{ public_only: true }` (see [HTTP client](#http-client)); `quillo check` warns
  when neither is so.

## Reactivity

A component's code runs once. What it declares with `state` is a signal; what reads a
signal (a `let`, a statement, a place in the markup) is recorded as it runs; and when the
signal is assigned a different value, those, and only those, are worked out again. The
component does not render again, nothing is compared against a virtual DOM, and there
are no dependency lists to keep: what code reads is what it follows. This is how
SolidJS and Svelte's runes work, not React.

```quillo
component Cart(items: list) {
    state lines = items                          // a signal
    state coupon = ""                            // another
    let total = lines | map((l) => l.price * l.qty) | sum()    // follows lines
    let off = coupon == "TEN" ? round(total * 0.1, 2) : 0     // follows coupon and total
    if total > 1000 { every(5000, () => { notify("Big order!") }) }   // starts and stops with it
    <div>
        <ul>{for l in lines => <li key={l.id}>{l.name} × {l.qty}</li>}</ul>
        <input value={coupon} placeholder="Coupon"/>
        <p class={off > 0 && "saved"}>Total {total - off}</p>
        <button onclick={() => { lines = lines | filter((l) => l.qty > 0) }}>Tidy</button>
    </div>
}
```

Typing in the coupon box sets `coupon`: `off` is worked out again, then the text of the
total and its class, and nothing else. `total` is not, since `lines` did not change; the
list is not touched. The page shows the new total once the keystroke's handler is done.

### What follows what

| Written in a component | Becomes | Is worked out again |
|---|---|---|
| `state x = v` (or `~x = v`) | a signal, set once | never: it changes when assigned |
| `let y = ...` that reads something that changes | a derived value | when something it read changed, and only once something reads it |
| `let y = ...` that reads nothing that changes | a constant | never |
| `let y = ...` whose name is assigned somewhere (`y = 5`) | a plain variable | never by itself |
| a statement that reads something that changes (`if running { every(...) }`) | an effect | after what it read changed |
| an assignment alone in the body (`count = len(items)`, `n += 1`) | a step as it starts | never: it runs once, and `quillo check` warns when it reads a prop or another state (write `let`) |
| `{expr}` in markup, an attribute's or a class's `{...}` | a place on the page | when what it read changed; text that comes out the same is not written |
| `cond ? <a/> : <b/>`, `if`, `match`, `cond && <x/>` in markup | a branch | made again only when another branch is chosen |
| `for x in xs => <row/>` | a row for each item | rows are kept by `key` (else by place) and given their new item |
| `<Child prop={x}/>` | props passed live | the child's own reads of `prop` follow `x`; neither component runs again |
| `key={k}` on an element or component outside a loop | a part made afresh | when `k` changes |
| `@event={...}`, `@mount`, a function given to a library | a handler | when it is called; what it reads is read then, not followed |

"Something that changes" is: a `state`, a prop, a `let` that is itself derived, a `for`
row's item, a `live(...)` state, `url()`, and a call of the component's own functions or
the program's (taken as reading what changes, since they may), however it is written:
`x | show` and `show(x / 2)` follow what `show` reads, as `show(x)` does. Builtins on constant
values are not: `let started = now()` is the time the component started, and stays so.
What is followed is what a run actually read: `let shown = open ? details : summary`
follows `open` and `details` while `open` is true, and `open` and `summary` while it is
false.

### When the page changes

- **A change is a different value.** Assigning a state the value it has (the same list,
  the same number) does nothing. Lists and maps are values, so a list built anew is a
  different value even when it holds the same items: everything that reads the state is
  worked out again (text that comes out the same is left alone). There is no way to
  change a list or map in place, so there is no change that goes unseen.
- **In a batch, a moment later.** Assigning marks what reads the state; the statements
  and places on the page run once the handler's own code is done (in a microtask), each
  once however many states it read changed, in the order they were made. An element read
  right after `n += 1` (`e.target.textContent`) still shows the old text; once the
  handler has waited for something (a `server fn`, `js(...)`), the page has caught up.
- **Derived values are lazy and consistent.** A `let` is worked out only when something
  reads it, and only if something it read really changed: a `let` that comes out the same
  as before stops the update there (`let even = n % 2 == 0` going from 2 to 4 wakes
  nothing that reads `even`). A statement never sees a `let` that is out of date.
- **No loops.** Code that sets what it reads would run for ever: after 1,000 rounds of
  updates the page stops and says so in the console (`updates go round in a loop:
  something sets what it reads`). A statement may set another state (`if query == "" {
  results = [] }`), as long as the two do not feed each other.
- **Errors stay where they are.** An error in a statement or a place on the page is
  written to the console (`quillo: ...`); the rest of the page goes on updating. A place
  whose expression fails is emptied, rather than left showing its last good value.

### What a statement starts, it stops

What runs again owns what it started: `every`, `listen`, `channel.open`, the statements
and branches inside it, and the functions `@mount` returned. Before it runs again, and
when its component, row or branch leaves the page, those are stopped. So timers and
streams are started by state rather than by hand:

```quillo
component Clock() {
    state running = false
    state speed = 1000
    state ticks = 0
    if running { every(speed, () => { ticks += 1 }) }    // stops when running is false,
    <div>                                                 // starts over when speed changes
        <button onclick={() => { running = !running }}>{running ? "Stop" : "Start"}</button>
        <button onclick={() => { speed = speed / 2 }}>Faster</button>
        <span>{ticks}</span>
    </div>
}
```

A handler may start one too, which then lasts as long as its component, or set the state
that the statement reads. For once, later, `run_after(ms, f)`. There is no `setTimeout`; a
debounce or a throttle is an `every` under a state flag that its tick turns off.

### Lists and rows

- `for` keeps a row for each item, matched by `key={...}` (text or a number), else by
  place. Rows that move are moved, as few as keep the rest in order: a swap moves two, a
  removal none; their DOM, focus and what was typed in them go with them.
- A row whose item is a different value is given it, and only the places in the row that
  read the item change. So when changing one item, keep the others the same values:
  `items | map((i) => i.id == id ? merge(i, { done: true }) : i)` changes one row; a
  list made of new maps for every item (from `json.parse`, or a server fn's answer) gives
  every row its new item, which is still cheap, since text that comes out the same is not
  written.
- `x == state` (or `!=`), with a state written by its name on one side and text, a
  number or a boolean on the other, is followed per value: in a thousand rows that each
  ask `r.id == selected`, choosing another row wakes two of them, not a thousand. Written
  through a function (`is_selected(r)`), every row asks again.
- A key that is a list or a map never matches the row it had, so the row is made again
  each time: key rows by an id.

### Branches

`if`, `? :`, `match` and `cond && <markup>` choose a branch, and make it again only
when another is chosen. While one stays, what is inside it follows its values like
anything else: an input in it keeps its focus and what was typed. A branch that goes
takes everything it made with it: its state of nested components, its timers. Keep what
must outlive a branch in the component's own `state`. `key={k}` on an element or a
component makes it afresh whenever `k` changes, which is how a page for a new product
starts clean (`<Product key={id} id={id}/>`).

### Between components

- Props are passed live: a child that reads `label` follows the parent's value, in its
  markup, its `let`s and its statements, without either component's code running again.
  A function given as a prop (`on_change`) carries changes back up.
- `state x = prop` reads the prop once: state is the component's own. To follow the
  parent, read the prop (or a `let` of it); to start over when it changes, give the
  child a `key` that changes with it.
- A component that has left the page (its branch gone, its `key` changed) keeps the
  props it last read: the function its `@mount` returned, and a handler that runs late,
  see its own values, not the parent's new ones.
- A child that changes its parent's state as it starts (a callback it calls in its body)
  has the server render the parent once more, so the page shows one value and its island
  carries it; at most once, and never inside another component's second render.
- A view used as a tag runs once with its props live, as a component does; a plain
  function used as a tag (`<Row item={x}/>` where `Row` is neither) is called again as a
  whole when what it is given changes.
- There is no context and no global store in the browser: state lives in components and
  goes down as props. Components placed separately in a page share nothing; they share
  through the server (below).

### What is not followed

- **The page's own objects.** `el.value`, `el.scrollTop`, what `js(...)` gives: read
  them in a handler and put what matters in state (`onscroll={(e) => { top = e.scrollTop }}`).
- **Time.** `now()` is read once; a clock is a state that `every` sets.
- **The server.** A `state x = load()` is the answer the page was made with. To follow
  the server, `state x = live(load)` (see [Live data](#live-data)); its arguments are
  followed too (`live(load_thread, id)` loads again when `id` changes).
- **Global variables.** A component uses the program's constants as they were when the
  program loaded; a global the server changes cannot be read in a component at all
  (pass it as a prop, or load it with `live`).

### On the server

A component's code runs once on the server too, to write the page's HTML with the state
it starts with; statements that start timers or streams do nothing there. The browser
then takes the server's elements as its own, from that same state, without running the
initializers again (see [Components](#components)). From then on, the browser is where
reactivity happens.

The server has two of its own, which follow the same idea of following what was read:

- **Live data.** A `live` loader records the tables and globals it read; a write to one,
  or a `publish` of a topic it `watch`ed, runs it again for every page that shows it, and
  the pages get the difference (see [Live data](#live-data)).
- **Cached views.** A view's HTML is reused while its arguments and the globals it read
  are unchanged (see [Cached views](#cached-views)).

### Coming from other frameworks

| React | Svelte 5 | Vue | Quillo |
|---|---|---|---|
| `useState` | `$state` | `ref` | `state x = v` |
| `useMemo` | `$derived` | `computed` | `let y = ...` reading state |
| `useEffect` (and its cleanup) | `$effect` | `watchEffect` | a statement reading state; what it starts stops with it |
| `useRef` | `bind:this` | template ref | `onmount={(el) => { box = el }}` |
| `value` with `onChange` | `bind:value` | `v-model` | `value={x}`: two-way by itself when `x` is state |
| `useContext` | `setContext` | `provide` | none: props |
| `key` | `{#each ... (key)}` | `:key` | `key={...}` |
| `React.memo`, `useCallback` | | | not needed: nothing renders again |
| a list of dependencies | | | none: what is read is followed |

The rules of hooks have no counterpart: state is found by its name, not by the order of
calls, and a statement that starts something may sit inside an `if` (that is how it is
stopped).

On [quillolang.org's Reactivity page](https://quillolang.org/docs/reactivity/), live
demos show each of these running beside its code.

## Routes

```quillo
route METHOD /path/:param/:typed(int)/*rest [? { query pattern }] [{ request pattern }] [with effects] = expr
route METHOD /path [with effects] { ... block ... }
```

- **Methods:** `GET POST PUT PATCH DELETE OPTIONS ANY`. `ANY` takes every method. A
  `HEAD` request is served by the `GET` route: the route runs (what it does, it does),
  and the body is dropped. A `route HEAD` is accepted but never matches. An `OPTIONS`
  request reaches a route declared `OPTIONS` (or `ANY`). On a path that has routes but
  none for `OPTIONS`, it is answered 204 with `Allow`, the methods the path takes (`GET,
  HEAD, OPTIONS, POST`), and so is `OPTIONS *`: a health check that sends it (HAProxy's
  `option httpchk`) finds the server up. Nothing answers CORS preflights by itself, and
  no `access-control-*` header is written for you.

  ```quillo
  route GET /items = "the list"
  route POST /items = "added"
  route ANY /echo = request.method
  ```

  ```sh
  curl -X DELETE -i localhost:8080/items    # 405, allow: GET, HEAD, OPTIONS, POST
  curl -I localhost:8080/items              # 200, content-length: 8, no body
  curl -X OPTIONS -i localhost:8080/items   # 204, allow: GET, HEAD, OPTIONS, POST
  curl -X PATCH localhost:8080/echo         # PATCH
  ```
- **Path params** become variables. `:id(int)` only matches what parses as an int (also
  `float`, `dec`, `num`, `str`, and `date`: `/day/:d(date)` takes `2026-11-20`, not
  `2026-02-30`). A typed parameter takes its type written one way *(since 0.7)*: an `int`
  is an optional `-` and digits with no leading `0` (`05`, `+5` and ` 5` are not one, so a
  page has one address), a `float`, `num` or `dec` also takes a fraction and an exponent and
  is finite (`NaN`, `inf` and `1e999` are not), and nothing is trimmed, a `str` included.
  The same holds for typed fields of a query or a form. `*rest` (or `*name`) takes the rest
  of the path, so it comes last: nothing can follow it, in the route or in its group's
  routes *(since 0.7: what followed was ignored)*; `*` alone names it `rest`. `/users/{id}`, as OpenAPI and Express write a parameter, is an
  error that says to write `:id`. The path is split at `/`, empty pieces dropped, so `/users`,
  `/users/` and `//users` are one path; each piece is percent-decoded after the split, so
  a `%2F` stays inside its piece. `request.path` is the path as it came, undecoded.
- **What a piece may hold:** letters and digits (any script: `/café`), `- . _ ~` and the
  other characters a URL's path takes as they are, `@ + ! $ &` *(since 0.7: `@` and `+`
  were refused)*: `/u/@me`, `/c++`. A `%` escape is read as the request's is, so
  `/a%20b` takes `/a%20b` *(since 0.7: it never matched)*. `/users/${id}` and `/@{name}`,
  a template's or OpenAPI's way, are an error that says to write `:id`. `= , ; ' ( ) *` are not, as the route line gives them
  meanings of its own, and `:` starts a parameter. A parameter may follow `@` or `~` in
  its piece, as profiles' addresses are written *(since 0.7)*: `route GET /@:name` takes
  `/@ann` with `name` `"ann"` (and `/%40ann`, the same piece encoded); `/@` alone has no
  name, so a later route takes it. A route before it that takes the same requests is said
  by `quillo check`, as for any other route.

  ```quillo
  route GET /users/:id(int) = `user ${id + 1}`     // id is an int: arithmetic works
  route GET /day/:d(date) = `the ${d}`
  route GET /files/*rest = rest
  ```

  ```sh
  curl localhost:8080/users/41              # user 42
  curl localhost:8080/users/abc             # 404: not an int, so no route matches
  curl localhost:8080/day/2026-02-30        # 404: not a date
  curl localhost:8080/files/docs/a.txt      # docs/a.txt
  ```
- **Query pattern:** `? { q: str, page: int = 1 }`. Query values are text, so `int`, `float` and `bool` patterns convert them. A blank value (`?page=`, as a search
  form sends a field left empty) is one not sent when the field has a default, so `page`
  is 1 *(since 0.7)*. `bool` takes `true`, `on`, `1` and `yes`, and `false`, `off`, `0` and
  `no`, in any case. A list (`{ tag: [str] }`) takes every `tag`, or every `tag[]` when
  no `tag` came, as PHP- and Rails-shaped forms name them. An unchecked checkbox sends
  nothing, so its field needs a default: `{ form: { agree: bool = false } }`.
  A name sent more than once keeps its last value in `request.query` and `request.form`;
  a list in the pattern takes them all, as a group of checkboxes sends: `? { tag: [str] }`
  gives `["a", "b"]` for `?tag=a&tag=b`, one value a list of one, and none an empty list.
  So does `{ form: { tag: [str] } }`, for a urlencoded form.

  ```quillo
  route GET /search ? { q: str, page: int = 1 } = `${q}, page ${page}`
  route GET /filter ? { tag: [str] } = join(tag, ", ")
  ```

  ```sh
  curl "localhost:8080/search?q=lamp"            # lamp, page 1
  curl "localhost:8080/search?q=lamp&page=2"     # lamp, page 2
  curl "localhost:8080/search?q=lamp&page=x"     # 400: page is not an int
  curl "localhost:8080/search"                   # 400: q is missing
  curl "localhost:8080/filter?tag=a&tag=b"       # a, b
  ```
- **Request pattern:** matched against the `request` map. Its fields, and all of them:
  `method`, `path`, `query`, `query_string` (the query as it came), `params`, `headers`,
  `cookies`, `body`, `json`, `form`, `files`, `ip`, `id`. The host is `request.headers.host`; the tenant and the locale
  are `tenant()` and `locale()`. JSON is matched strictly: the string `"7"` is not an
  int. Form fields, headers and cookies are converted like query values. `{ json: User }`
  takes the body as a [record](#records). A pattern on the host is a pattern on the
  headers: `{ headers: { host: "api.example.com" } }`.

  ```quillo
  route POST /api/items { json: { name: str, qty: int } } = `${qty} x ${name}`
  route GET /beta { headers: { "x-beta": "yes" } } = "the new page"
  route GET /beta = "the old page"                 // the same path, for everyone else
  ```

  ```sh
  curl -H "content-type: application/json" -d '{"name":"lamp","qty":2}' localhost:8080/api/items
  # 2 x lamp
  curl -H "content-type: application/json" -d '{"name":"lamp","qty":"2"}' localhost:8080/api/items
  # 400: "2" is text, not an int
  curl -H "x-beta: yes" localhost:8080/beta      # the new page
  curl localhost:8080/beta                       # the old page
  ```
- **What the request holds.** `headers` has its names in lower case, and a header sent
  twice keeps the last; only the last `Cookie` header is read, its values
  percent-decoded. `query` and `form` keep the last of a key given twice (`a=1&a=2` is
  `{ a: "2" }`): no lists, and `a[b]` is a key named so. `json` is the body parsed when
  the content type is `application/json` (or `...+json`), or when there is none and the
  body starts with `{` or `[`; otherwise, and for JSON that does not parse, it is
  `null`. `body` is the body as text: bytes that are not UTF-8 are replaced, so a binary
  body cannot be read as it was (there is no bytes value in the language), and for a
  multipart body it is empty. `id` is the request's id (see [Services](#services)). A
  JSON body that starts with a byte-order mark is read.

  ```quillo
  route POST /show = json.stringify({ method: request.method, path: request.path,
      query: request.query, form: request.form, agent: request.headers["user-agent"] })
  ```

  ```sh
  curl -A demo/1 -d "name=Ana&name=Bo" "localhost:8080/show?a=1"
  # {"method":"POST","path":"/show","query":{"a":"1"},"form":{"name":"Bo"},"agent":"demo/1"}
  ```

  `name` was sent twice and keeps the last; a list in a pattern (`{ form: { name: [str] }
  }`) takes both.
- **What the server refuses before a route.** Two `Host` headers, an HTTP/1.1 request
  without one, or one that is not a host and a port, get 400; a method not written in
  capitals, or one HTTP does not define (`FROB`), gets 501, unless a `route ANY` takes the
  path, as `ANY` takes every method. A request target in absolute
  form (`GET http://host/path`) takes its host from the target. A `HEAD` answer carries
  the `Content-Length` the `GET` would. A body sent with `Content-Encoding: gzip` reaches
  the route decoded, [`--max-body`](#options-of-quillo-run) (`quillo run main.qwe --max-body 50MB`; 10MB unless
  given) counting what it decodes to (more is 413); one in
  another coding gets 415, and one that is not the gzip it says, 400. An `Expect` other
  than `100-continue` gets 417. An HTTP/1.0 request gets no `100 Continue` (a 1.0 client
  would take it for the answer) and no chunks: a streamed answer goes with no length and
  ends with the connection, and an `Upgrade` is ignored.

  ```sh
  curl -X FROB localhost:8080/items         # 501 unknown method
  curl -X get localhost:8080/items          # 501 unknown method (methods are written in capitals)
  printf 'GET /items HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n' | nc localhost 8080
  # 400 Bad Request: two Host headers (curl would send only one)
  ```
- **Reserved:** paths under `/_quillo/` belong to Quillo and never reach a route (what is
  there is listed under [What a route cannot do](#what-a-route-cannot-do)). A route
  written there is never called: `route GET /_quillo/stats = ...` answers 404, so put
  your own under another path (`/stats`, `/admin/stats`). `quillo check` refuses one, and
  a `route HEAD` too (a HEAD is answered by the GET route), and warns of a route an
  earlier one with no pattern always takes first *(since 0.7)*.
- **Order:** routes are tried in the order they are declared, and the first whose method, path and patterns all match handles the request. A later route can be the fallback.
  Routes are declared at the top level of a file: `quillo check` refuses one inside a
  function or a block.

  ```quillo
  route GET /people/me = "you"                     // before :id, or "me" would not match
  route GET /people/:id(int) = `person ${id}`
  route ANY /*rest = respond(404, `Nothing at /${rest}`)   // last: everything else
  ```

  ```sh
  curl localhost:8080/people/me             # you
  curl localhost:8080/people/7              # person 7
  curl localhost:8080/nowhere               # 404: Nothing at /nowhere
  ```

  Put the specific before the general. With the catch-all last, no path answers 405 or
  400 any more: every request reaches it.
- **What it may do:** `with`, before the body, holds the route to what it says, as for a
  function (see [Effects](#effects)):

  ```quillo
  route GET /report/:day with db = db.query("select * from orders where day = ?", day)
  ```

  A call in it (or in a group's middleware) that needs another effect is refused by
  `quillo check` and when it runs. What the answer runs after the body is held too: the
  function given to `stream(f)` or `events(f)`, and a component's `defer(...)` parts.

If no route matches, the response is 404 for an unknown path, 405 for a known
path with the wrong method (with `Allow`, the methods it takes), and 400 when a route
matched the path and method but none of its patterns matched. The 400 is logged with the
route that came closest and its first miss (`` POST /form (main.qwe:6): `form.agree` is
missing, and must be a bool ``), and under `--watch` the answer says it too *(since 0.7)*. Before any route is tried, a `POST`, `PUT`, `PATCH` or
`DELETE` with a form, multipart or plain-text body (or none) that the browser marks as
coming from another site is refused with 403 (see `allow_cross_site` under
[Standard library](#standard-library)). A last `route ANY /*rest` is a 404 page of
your own; since `ANY` matches every method, no path then answers 405 or 400.

The statuses the server answers by itself:

| Status | When |
|---|---|
| 204 | `OPTIONS` on a path no `OPTIONS` route takes, and `OPTIONS *`, with `Allow` |
| 304 | a file's, or a route's own `etag`, that the browser already holds |
| 400 | a malformed request line, header or chunked body; a body that is not the gzip it says; a route matched the path and method but no pattern; a `server fn` call whose body is not a JSON array; a WebSocket handshake that is not a `GET` with `Upgrade` and `Connection` |
| 403 | a cross-site form post, `server fn` call or live connection; a phone app built the old way, on 127.0.0.1, whose launch token is missing |
| 404 | no route; an unknown `/_quillo/` path; a `file()` path refused |
| 405 | the path is known, the method is not (with `Allow`) |
| 408 | a request head not whole after 30 s, or a body of which nothing came for 30 s (logged: `POST /up HTTP/1.1 -> 408 (nothing of the body came for 30 s)`) |
| 409, 422 | an [idempotent request](#idempotent-requests) running, or sent again with another body |
| 412 | a file whose `If-Match` or `If-Unmodified-Since` it does not meet |
| 413 | a body larger than `--max-body`, sent or once decoded |
| 414 | a request line over 64 KiB |
| 415 | a body in a coding other than gzip; a `server fn` call without `content-type: application/json` |
| 416 | a `Range` past the end of a file |
| 417 | an `Expect` other than `100-continue` |
| 426 | a WebSocket handshake of a version other than 13 |
| 431 | a request head over 64 KiB, or more than 100 headers |
| 500 | a runtime error in the route (the body is `internal error`; the message is on stderr, and on the page under [`--watch`](#options-of-quillo-run)) |
| 501 | a `transfer-encoding` other than `chunked`; a method HTTP does not define |
| 503 | the server stopping or its program failed to load; a prefetch refused; too many files being sent or pages waiting (with `retry-after: 5`) |

### Route groups

Routes that share a start of their path, middleware or a request pattern are written once
for all of them, in a `routes` group, as Laravel's `Route::prefix()->middleware()->group()`:

```quillo
fn signed_in(r) = session().user != null ? r : reply 303 redirect(`/login?next=${url_encode(r.path)}`)
fn admins(r) = is_admin(session().user) ? r : respond(403, "admins only")

routes /admin | signed_in | admins {
    route GET / = <AdminHome/>                      // GET /admin
    route GET /users = list_users()                 // GET /admin/users
    route POST /users/:id(int)/ban { ban(id) }

    routes /teams/:team(int) {                      // groups nest: /admin/teams/7/members
        route GET /members = members(team)
    }
}

routes /api { headers: { host: "api.example.com" } } {    // Laravel's domain()
    route GET /me = me()
}

routes / | signed_in {                              // no prefix: middleware only
    route GET /settings = <Settings/>
}

routes /reports | signed_in with db {               // what its routes may do, at most
    route GET /daily = daily()
}
```

- **`with`**, after the middleware, is what every route in the group, and its middleware,
  may do. A route in it may narrow it (`route GET /status with time = ...`), a group
  inside it too, and asking for an effect it does not allow is an error (see
  [Effects](#effects)).

- **The path** goes before each route's own, parameters and types included
  (`:team(int)`), which the routes' bodies and patterns can use; a group's `/` alone
  adds nothing.
- **Middleware** (each after a `|`, as in a pipeline) is called with the request before
  the route's body, outer groups' first, in the order written. It lets the route go on by
  giving the request back (as `auth.required` does), or a copy of it (`merge(r, ...)`).
  **Null says the group's routes do not match**, as a request pattern that misses: the
  next route is tried, and with none the server answers as if they were not there (a 404,
  where no other route has the path). So a guard that ends with no value (`if root {
  return r }`) keeps out everyone else and tells them nothing, and a later route of the
  same path can answer them. Anything else stops it: a `reply` in it, or a response it
  gives back (`redirect(...)`, `respond(...)`), answers in the route's place; `false`
  answers 403; an error value answers as a route's error does; and any other value
  (`true`, text, another map such as `{ error: "no" }`) is a 500 that names the middleware
  *(since 0.7: these let the route run, null and maps too)*. A middleware that only logs
  ends with `r`. So a check is written `(r) => ok(r) ? r : reply 403 "..."`, not as a
  predicate, and `quillo check` says so when a middleware can only be a `bool` or text.
  What it gives does not reach the route: a route that needs the user reads it again
  (`session()`, `me()`). It is given the request as sent, whatever the route's patterns
  bind: a JSON field named `request` is the route's, not the middleware's *(since 0.7)*.
- **Before the server's own answers** *(since 0.7)*. A request under a group's path that
  no route of the group takes (another path, method or parameter type, a pattern it does
  not match, an `OPTIONS`) goes through the group's middleware first: only those it lets
  in learn what is there from a 404, a 400, a 405 and its `Allow`.
- **Middleware that takes an argument** is a function that makes one, called in the
  group's scope, where the group's own parameters are, already typed:

  ```quillo
  fn member_of(team: int) = (r) => my_teams() | contains(team) ? r : reply 403 "not your team"
  fn role(name: str) = (r) => session().role == name ? r : reply 403 "no"

  routes /teams/:team(int) | member_of(team) {
      route GET /board = board(team)
  }
  routes /billing | role("admin") { route GET / = <Billing/> }
  ```
- **A group guards its routes, not the `server fn`s their pages call.** Each server fn is
  its own `POST /_quillo/rpc/<name>`, which anyone can call: one a guarded page loads or
  acts through checks the caller itself (`admins(request)`, `me()`). `quillo check` warns
  of a server fn a guarded page calls that reads neither `request` nor `session`.
- **A request pattern** after the middleware is matched as well as each route's own
  (both must match), and binds the same way: a request it does not match goes on to the
  next route, as with a route's own pattern. The middleware comes first:
  `routes /api { headers: ... } | g { ... }` is an error, as `| g` there would be a
  pattern that takes any request.
- **A host** in a pattern is matched as it names a host: `headers.host`, and
  `request.host`, are lower case, without a trailing dot or the default port
  (`API.example.com:443` is `api.example.com`) *(since 0.7)*. `request.route_path` is the
  path as routes read it (`//admin/users` and `/%61dmin/users` are `/admin/users`), which
  `site_headers` and `allow_cross_site` match on too.
- A group holds only `route`s and `routes` groups, is written at the top level of a file,
  and keeps its routes in the order written, among the file's others. Outside that,
  `routes` is a name like any other (`routes / 2` divides). Routes from groups are
  routes: `quillo docs` lists them by their whole path.
- **Back where they were.** The login page sends the visitor on to `next` once signed in.
  `next` came in the address, so anyone can write a link whose `next` is another site:
  `local_path(next)` *(since 0.7)* gives `next` when it is an address on this site as a browser reads it
  (tabs and line breaks dropped, `\` read as `/`), else null. `//evil.example`,
  `/\evil.example`, `https://...` and `javascript:...` are null. It works on both sides.

  ```quillo
  route GET /login ? { next: str = "/" } =
      <form method="post" action={`/login?next=${url_encode(next)}`}>   // next goes on with the form
          <input type="email" name="email"/>
          <input type="password" name="password"/>
          <button>Log in</button>
      </form>

  route POST /login ? { next: str = "/" } { form: { email: str, password: str } } {
      sign_in(email, password)?
      redirect(local_path(next) ?? "/")
  }
  ```

  A visitor who is not signed in and opens `/admin/users` is sent to
  `/login?next=%2Fadmin%2Fusers` by `signed_in`; the form posts to that same address, and
  once signed in they are back on `/admin/users`. A link whose `next` is
  `//evil.example` lands on `/`.

### Uploads

A form with `enctype="multipart/form-data"` sends files. Its text fields are in
`request.form`, as for any form, and each file is in `request.files` under its field's
name: `{ field, filename, type, size }`. The shape follows the count: a field that sent
one file holds its record, and a field with several, from `multiple`, a list of them. A
pattern makes it one shape: `{ files: { docs: [map] } }` takes every file of that field
as a list, one file a list of one and none an empty list, as `[str]` does for form fields:

```quillo
route GET /docs =
    <form method="post" action="/docs" enctype="multipart/form-data">
        <input type="text" name="title"/>
        <input type="file" name="docs" accept=".pdf" multiple/>
        <button>Send</button>
    </form>

route POST /docs { files: { docs: [map] } } {
    for d in docs { save_upload(d, `uploads/${uuid()}.pdf`)? }
    redirect("/docs")
}
```

The whole round trip for one file: the page with its form, then the route the form posts
to. The bytes stay with the request until saved:

```quillo
route GET /avatar =
    <form method="post" action="/avatar" enctype="multipart/form-data">
        <label>Your photo <input type="file" name="photo" accept="image/*" required/></label>
        <button>Upload</button>
    </form>

route POST /avatar {
    let f = request.files.photo
    if f == null { reply 400 "Choose a photo" }
    if f.size > 2_000_000 { reply 413 "Photos up to 2 MB" }
    let path = save_upload(f, `uploads/${uuid()}.png`)?   // into the program's folder
    redirect(`/${path}`)
}

route GET /uploads/:name = file(`uploads/${name}`)
```

The form needs `method="post"` and `enctype="multipart/form-data"`: without the enctype,
the browser sends only the file's name. Each file input's `name` is its key in
`request.files`; `accept` and `required` are the browser's own checks, which a visitor
can skip, so the route checks again (`f == null`, `f.size`). A file chosen in a component
goes to a `server fn` instead, without a form (see `save_upload` in the reference).

- `save_upload(file, path)` writes the file under the program's folder (making folders
  as needed) and gives the path back; `read_upload(file)` gives a text file's contents.
  Both give an error value when the request has no files, or the record is not one of
  this request's; a path that leads outside the folder, by `..` or through a link
  (see [Paths and links](#paths-and-links)), is a runtime error. `save_upload` needs the
  `fs` effect.
- `save_upload`, `write_file`, `delete_file` and `image.convert` refuse the program's own
  files, and give an error value saying so: the `.qwe` and Rust (`.qwer`, `.rs`) files it
  loaded (or a folder holding one), `.env` and `.env.*`, the paths given to
  [`--env-file`](#options-of-quillo-run) and [`--tls-key`](#options-of-quillo-run) (while
  nothing is there too), the SQLite databases it has open with their `-wal`, `-shm` and
  `-journal` files (or a folder holding one), a tenant's database not made yet, and
  anything in `migrations/`. Names are compared as the file system may take them, on
  every system: in any case and Unicode form (`.ENV`, `Main.QWE`), and as Windows reads
  them (cut at `:`, trailing dots and spaces dropped), so `.ENV` is not written to be
  read as the settings at the next start.
- `filename` is what the browser sent, without any folders; name saved files yourself
  (as above) rather than trusting it. Save under a folder of the program's with a name the
  program makes (`uploads/${uuid()}.pdf`), and keep the visitor's name in the database.
  `quillo check` warns on `save_upload(f, f.filename)`, and on a path that is only a form
  value, then `/`, then the filename. The record also carries `start` and `end`, the
  file's place in the body.
- A file's name and a field's name decode the three escapes HTML's multipart encoding
  writes (`%22` for `"`, `%0D`, `%0A`) and nothing else, and a quoted name keeps a `;` in it.
  Clients other than browsers are read too: a `\"` in a quoted name (Go, curl) is a `"`,
  and `filename*=UTF-8''...` (RFC 5987, UTF-8 or ISO-8859-1) gives the name when no
  `filename` is there, a browser's plain `filename` coming first.
- A multipart body may hold up to 1,000 parts, fields and files together; one with more is
  answered 413 before the route runs. A text field given twice keeps the last. The body's
  content type is read in any
  case (`Multipart/Form-Data`, `Application/X-WWW-Form-Urlencoded`), as HTTP has it.
- A request body may be up to 10MB, the files and the form's fields together; a larger one
  is answered 413. For larger files, start the program with a higher limit, the option
  [`--max-body`](#options-of-quillo-run): `quillo run main.qwe --max-body 50MB`.
  A body may take as long as it keeps coming, a photo from a phone on a weak link too; one of
  which nothing comes for 30 s is answered 408, with a line in the log. A proxy in front
  has limits of its own (nginx's `client_body_timeout`). The request's head (its line and headers) has limits of its own, 64 KiB and 100
  headers, answered 431.

From a component, a file goes to a `server fn` as an argument: pass it the input's file,
and the function gets the same `{ field, filename, type, size }` record a form's upload
is, for `save_upload`:

```quillo
server fn add_photo(file: map) -> str = save_upload(file, `photos/${uuid()}.jpg`)

component Photo() {
    state saved = null
    <input type="file" accept="image/*" onchange={(e) => { saved = add_photo(e.target.files[0]) }}/>
}
```

A file anywhere in the arguments goes as a file, inside a list or a map too, and a
`FileList` (`e.target.files`) is a list of files, so `add_photos(e.target.files)` gets a
list of records. Such a call is sent as multipart with an `X-Quillo-Rpc` header, which
pages of other sites cannot add without asking first; a form another site posts is refused.

### Checking code from a program

`check_program("main.qwe", { "plugins/x.qwe": text })` gives what `quillo check` says of
a program, as `{ file, line, severity, message }` maps, with some files' text given in
place of what is on disk: a tool that writes Quillo code can check it before writing
it. The code checked may come from users, so it reads only files inside the program's
folder: a `use`, `mount` or `service` path with `..`, starting with `/`, or leading out
of the folder through a link is reported as an error (`path must stay inside the
program's folder: data/x.qwe`), not read. It reads only Quillo's files, `.qwe` (and the
signatures of `.qwer` and `.rs` files for a `use`), no path with a dot folder or file in
it, and a link judged by where it leads, so `use .env` or `use .git/config` reads
nothing; its messages never show a file's text. `delete_file(path)` removes a file or folder inside the
program's folder, but not a folder that holds a database the program has open.

### Scripts from users

`run_script(source, vars)` runs code a program's users write (a rule on a form's field,
who may see a record) on the server, and gives its last expression's value, or an error
value saying what went wrong:

```quillo
run_script("value > 3 ? null : `at least 4, got ${value}`", { value: 5 })   // null
```

- The script sees the values in `vars` and the builtins that only compute (`len`,
  `upper`, `map`, `filter`, ...). Method calls are the same: `"x".upper` works,
  `"HOME".env` and `"f".read_file` do not, nor the program's functions.
- It cannot define functions (it can write `(x) => ...`), routes, state or markup,
  `use` files or reply.
- It has no effects of its own: a builtin such as `env` passed in `vars` still cannot
  run. A function of the program passed in `vars` runs as the program's, with what the
  program may do, so it is the program's way to let a script ask for more
  (`{ taken: (code) => db.one(...) != null }`).
- It stops after 20,000 steps (calls and loop rounds). A value it makes may be up to
  1 MB (strings, and lists and maps counted with what they hold), and each KB it makes
  costs a step: doubling a string in a loop stops there, not when the memory runs out.
- Like any code, it may nest at most 256 levels deep.

### Images

```quillo
image.info("photos/a.jpg")              // { width, height, format, alpha }, or an error
image.convert("photos/a.jpg", "cache/a-800.jpg", {
    crop: { x: 100, y: 0, w: 1200, h: 675 },   // in the image's pixels
    width: 800,                                 // shrunk to it, never enlarged
    format: "jpeg", quality: 82,                // jpeg, png or webp (lossless)
})
```

`format` is what the target's name says unless given: `.jpg` or `.jpeg`, `.png`, `.webp`,
in any case *(since 0.7: it was JPEG whatever the name, and `out.png` held a JPEG without
its transparency)*. A name that says none of them (`.gif`, no extension), with no
`format`, is an error value, and nothing is written. A `format` given wins over the name.

A photo is read as it is shown: its EXIF orientation is applied, so `image.info`'s
`width` and `height` are those of the turned image, a `crop` is in its pixels, and the
copy is upright. JPEG has no transparency, so transparent pixels are laid on white in a
JPEG (they went black); there is no option for another colour. Each conversion writes to
a temporary file of its own and moves it in place, so two at once to one target both
finish, the last one's file kept.

JPEG, PNG, WebP and GIF are read. `image.info` reads only the header, so it is a cheap
check that an upload is an image at all. Decoding is limited to 12,000 pixels a side
and 512 MiB, so a small file claiming to be enormous is refused before anything is
allocated; both run off the event loop. At most four conversions run at once (fewer on
a machine with fewer cores), so many uploads together cannot each take 512 MiB; the rest
wait their turn, and past 64 waiting `image.convert` gives an error value at once. Paths
stay inside the program's folder.

An upload made into a picture for the page: the form, the route that keeps the file and
makes an 800-pixel copy, and the page that shows it:

```quillo
route GET / =
    <form method="post" action="/photos" enctype="multipart/form-data">
        <input type="file" name="photo" accept="image/*" required/>
        <button>Upload</button>
    </form>

route POST /photos {
    let f = request.files.photo ?? reply 400 "Choose a photo"
    let id = uuid()
    let original = save_upload(f, `uploads/${id}`)?
    let info = image.info(original)
    if is_err(info) { delete_file(original); reply 415 "That file is not a JPEG, PNG, WebP or GIF image" }
    image.convert(original, `thumbs/${id}.jpg`, { width: 800, format: "jpeg", quality: 82 })?
    redirect(`/photos/${id}`)
}

route GET /photos/:id = <figure>
    <img src={`/thumbs/${id}.jpg`} width="800"/>
    <figcaption>{image.info(`uploads/${id}`).width} pixels wide, shown at 800</figcaption>
</figure>

route GET /thumbs/:name = file(`thumbs/${name}`, { cache: "1y" })
```

A photo 1,600 pixels wide goes to its page, `1600 pixels wide, shown at 800`, with the
copy served as `image/jpeg`. A text file named `.jpg` is not kept, and the answer is
`That file is not a JPEG, PNG, WebP or GIF image` (415).

### Cookies and sessions

```quillo
route POST /login { form: { email: str, password: str } } with db {
    // stored the same way at signup: one Unicode form, trimmed, in lower case
    let user = db.one("select * from users where email = ?", lower(normalize(trim(email))))
    if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }
    set_session({ user: user.id })
    redirect("/")
}

route GET /me with db {
    let id = session().user ?? reply 401 "Log in first"
    db.one("select name from users where id = ?", id)
}

route POST /logout { end_session(); redirect("/") }
```

The pages that call them: a form that posts the email and password, and a logout button,
which is a form too, since `/logout` takes a `POST` (a link would be a `GET`, which a
prefetch could follow):

```quillo
route GET /login =
    <html><body>
        <form method="post" action="/login">
            <label>Email <input type="email" name="email" required/></label>
            <label>Password <input type="password" name="password" required/></label>
            <button>Log in</button>
        </form>
    </body></html>

route GET / = <html><body>
    {session().user == null
        ? <a href="/login">Log in</a>
        : <form method="post" action="/logout"><button>Log out</button></form>}
</body></html>
```

A wrong password, or an email no one has (`user` is null, so its `password_hash` is null,
which `verify_password` says is false), shows the plain text `Wrong email or password`
with status 401. The right one sets the cookie and goes to `/`, which now shows the
button; `/me` then gives `{"name":"Ana"}`, and after logging out `Log in first` (401).

- `set_session(map, { max_age })` replaces the session (30 days unless given),
  `session()` gives it (`{}` when there is none) and `end_session()` forgets it.
  `max_age` is seconds or a duration (`"12h"`, as for cookies below). A session keeps
  decimals, and ints past 2^53, as they are, so `session().total + 1m` adds.
- A session ends when its time is up, however often the visitor comes back: `session()`
  reads the cookie and never sends it again. For a session that lasts while it is used,
  call `set_session` again on the requests that count as use. Once a day is enough, and
  keeps the cookie from being sent with every answer:

```quillo
fn touch() {
    let s = session()
    let day = 24 * 60 * 60 * 1000                      // now() is milliseconds
    if s.user != null && now() - (s.at ?? 0) > day {
        set_session({ ...s, at: now() }, { max_age: "30d" })
    }
}
```
- A session is a cookie signed with HMAC-SHA256: the browser can read it but not
  change it, so keep secrets out of it. Cookies hold about 4 KB; keep an id in
  the session and the rest in the database. The signature covers its expiry, its tenant
  and the program's mount point, so it is no session past its time, for another
  tenant, or in another mounted program.
- Two requests of one visitor that set the session at once each start from the cookie
  the browser sent, and the browser keeps the last cookie it is given: what the other
  added is lost. Twenty adds to a cart kept in the session, sent at once, each
  `set_session({ ...s, cart: (s.cart ?? []) + [item] })`, kept one item. Keep what must
  not be lost (a cart, a draft) in a table, keyed by an id in the session.
- `end_session()` removes the cookie from the browser, and ends the session's MCP tokens
  (see [Tools for AI clients](#tools-for-ai-clients)), but a copy of the cookie someone kept stays
  valid until it expires. To end sessions on the server (log out everywhere, a stolen
  laptop), keep an id in the session and check it against the database.
- The session cookie is `__Host-quillo_session` when it is `Secure` and for the whole
  site (as in production): browsers take a cookie with that prefix only from the very
  host, over HTTPS, so a sibling subdomain (another tenant's site) cannot plant one.
  Elsewhere ([`--watch`](#options-of-quillo-run), `quillo run main.qwe --watch` while working on a program; a
  mounted program) it is `quillo_session`. A request with two
  session cookies that are both valid here has no session: one of them was planted.
- Set `QUILLO_SECRET` to a long random string (`crypto.token()` makes one) and use the
  same one on every server. Without it, a random key is made at start: sessions end
  when the server stops. The server warns when it starts if the secret is shorter than
  32 bytes, as it does for `QUILLO_SERVICE_SECRET` and `LIMER_ADMIN_SECRET`: a short one
  can be guessed offline from one signed cookie.
- `set_cookie(name, value, { max_age, path, domain, http_only, secure, same_site })` and
  `delete_cookie(name, { path, domain, http_only, secure, same_site })` add `Set-Cookie`
  to the response, whatever the route returns. Cookies are `HttpOnly`, `SameSite=Lax`,
  `Path=/` and `Secure` by default. `max_age` is seconds or a duration (`"30d"`: `s`, `m`,
  `h`, `d`, `w`, `y`); `path` starts with `/` and holds no space, `;`, control character
  or letter past ASCII (percent-encode them); `domain` takes letters, digits, `.` and `-`;
  `same_site` is `"Lax"`, `"Strict"` or `"None"`, and `"None"` makes the cookie `Secure`
  whatever was asked, since browsers take it no other way. Values are percent-encoded.
  In a mounted program, or under [`--base`](#options-of-quillo-run), the default `Path`
  is the prefix.
- An option `set_cookie` does not take is an error, when the program runs and in `quillo
  check` for one written out, and Express's and the header's spellings are told the name
  here: `` `set_cookie` has no option `sameSite`: write it `same_site` `` (`maxAge`,
  `Max-Age`, `httpOnly` alike); `expires` is told that `max_age` is how long it lasts.
  `delete_cookie` takes `max_age` only as 0.
- A cookie replaces another set in the same request only at the same name, path and
  domain (a domain in any case, with or without its leading `.`), as browsers keep them:
  `delete_cookie("theme", { path: "/old" })` and then `set_cookie("theme", "dark")` send
  both. A cookie of more than 3,900 bytes as sent (its value percent-encoded) is an
  error, as browsers drop it with its attributes.
- A name starting `__Host-` or `__Secure-` (in any case, as browsers read it) keeps its
  prefix's rules, or browsers drop the cookie: it is `Secure`, under `--watch` and
  `--plain-http` too (browsers take it from `localhost`), and a `__Host-` one is at
  `Path=/` with no `Domain`. Asking otherwise (`secure: false`, a `domain`, a `path` other
  than `/` for `__Host-`) is an error: `` a `__Host-` cookie has no domain ``.

  ```quillo
  route POST /cart/:id { set_cookie("__Host-cart", id, { max_age: "7d" }); "kept" }
  // Set-Cookie: __Host-cart=...; Path=/; Max-Age=604800; HttpOnly; Secure; SameSite=Lax
  // set_cookie("theme", "dark", { sameSite: "Strict" }) is an error: write it `same_site`
  ```
  `Secure` is left out with `--watch` and in desktop apps, which are served
  over plain HTTP on this machine; in production, serve the site over HTTPS (a proxy
  such as limer in front). A browser drops a `Secure` cookie that comes over plain HTTP
  from anywhere but `localhost`, so a site reached as `http://10.0.0.5:8080` cannot keep
  anyone signed in: the server says so once, naming the cookie, the first time it sends
  such a cookie to a request that came straight from another machine (not through a proxy
  here, and without `x-forwarded-proto: https`). On a network you trust, start the program with
  [`--plain-http`](#options-of-quillo-run), `quillo run main.qwe --plain-http`, which leaves `Secure` out, as
  `--watch` does. `delete_cookie` is `Secure` only when the request
  came over HTTPS (or a proxy's `X-Forwarded-Proto: https` says so), when the name needs it
  (`__Host-`, `__Secure-`), or when `secure: true` is given, so a cookie set with `secure:
  false` can be removed over plain HTTP. `request.cookies` has what the browser sent, and
  `cookie(name)` one of them (or null), in any function the request runs, as
  `session()` is: a helper that reads the visitor's time zone from a cookie need not be
  handed `request` by every caller. Outside a request (a job, a script), `session()` is
  `{}` and `cookie(name)` is null.
- `set_session` takes only `max_age`; `set_session(null)` ends the session too. The
  cookie's value is the session as JSON, with its expiry, base64url-encoded and signed,
  so a session over about 3,900 bytes is a runtime error, and so is one nested deeper
  than 128 levels, as deep as JSON is read back.

### What a route returns

| Value | Response |
|---|---|
| html | 200 `text/html`; XML when its root names a namespace (`<urlset xmlns="...">`, a sitemap) or it starts `<?xml`: `application/xml`, `application/rss+xml` for `<rss>`, `application/atom+xml` for `<feed xmlns="...">`, and `image/svg+xml` for `<svg xmlns="...">` |
| str | 200 `text/plain` |
| map, list, number, bool | 200 JSON; one holding a function is a 500, its log line naming where (`the answer's value cannot be written as JSON: `f` is a function, ...`), and `quillo check` warns of a function written in the answer |
| null | 204 |
| error | 500 `{"error": ...}` |
| `respond(status, body, headers)` | exactly that |
| `redirect(url, status = 303)` | a redirect |
| `file(path, { cache })` | a file under the program's folder (see [Static files](#static-files)) |
| `files(folder, path, options)` *(since 0.7)* | a folder of files, as a static host serves it (see [Static files](#static-files)) |
| `markdown_page(text, { title, lang, html })` *(since 0.7)* | Markdown as a whole page: a doctype, a `<head>` with the title (the option's, else the first level-one heading's text, `#` or underlined with `=`), and the HTML |
| `stream(f, { type, status, headers })` | sent as `f` makes it (see [Streaming](#streaming)) |
| `events(f, { status, headers })` | Server-Sent Events, as `f` sends them |
| a runtime error | 500 `text/plain`, `internal error` |

- `redirect(...)` and `respond(...)` make an answer; they do not leave. A route answers with
  its last value, or what `return` or `reply` gives, so `if no_user { redirect("/login") }`
  with more after it goes on and shows the page. To leave, write
  `reply 303 redirect("/login")`; `quillo check` warns of one nothing takes.
- An error the program wrote, `err("That name is taken")`, is sent as it is. One a builtin
  made says `internal error` instead, wherever it would reach a visitor: as a route's
  value, inside a map or list, in markup, or from a `server fn`, and when it is turned
  into text: a template string, `str`, `join`, `json.stringify`. A database's has the SQL
  and the tables, an `http` call's the address with its key, a file's its path, so that
  text goes to the log with the request. `print` shows it too, as [`--watch`](#options-of-quillo-run)
  (`quillo run main.qwe --watch`) and a script
  do, and code on the server still reads it: `db.error_kind(r)`, `is_err(r)`, `error(r)`.
  What is read that way is plain text, no longer protected: `{ line: error(r) }` sends
  it. To show a visitor something, say it:
  `err("Billing is down, try again soon")`. A service's own `err` reaches its caller's
  visitors in the same way.
- A failed request's line in the log names it, with the id in its `x-request-id`:
  `quillo: error: GET /checkout (rid 1cf61931): main.qwe:8: division by zero`. Under
  [`--log json`](#options-of-quillo-run) it is an object, with `level`, `request` and `error`.
- `respond(status, body, headers)` takes a status from 200 to 599. A 1xx is an interim
  answer, after which the client waits for the real one, so it is refused, when the
  program runs and in `quillo check` for a written one (`` `respond` status must be
  200-599, got 103: a 1xx is an interim answer, and the client would wait for the real
  one ``); `reply` takes 200 to 599 too. A 205 goes with no body, whatever is given, and
  each status has its reason phrase (`418 I'm a teapot`). A route's answer that carries
  an `etag` is a 304 to a `GET` or `HEAD` whose `If-None-Match` holds it, as files are:
  `` respond(200, v, { etag: `"${version}"` }) ``. Header names are written in lower case, and a name or value with a control character (a line break) is
  a runtime error. `content-length`, `transfer-encoding`, `connection` and `date` are
  the server's, whatever is given.
- A page whose route, group middleware or server fn reads `request.headers`,
  `request.cookies` or `request.ip`, or passes `request` on, is `private`, whatever its
  status, and a translated page varies by `Accept-Language` and `Cookie`. Middleware that
  cannot be read, a factory's too, makes its pages private; it is read once.
- A header's value is text, or a number. `null` means no such header: none is sent, not
  even the one Quillo would send itself, as in `site_headers`, so `{ "cache-control": null }`
  leaves out the `private` a page with a session gets, and `{ server: null }` the
  `server: Quillo`; a header the route does not name keeps Quillo's. A list is one header
  per item for `set-cookie` alone (`{ "set-cookie": ["a=1", "b=2"] }` sends two, `[]`
  none), and is an error for any other header, as a map is (`` `respond` header `x-list`
  is a str, got list: only `set-cookie` takes a list ``). `stream` and `events` take their
  `headers` the same way. A response's `.headers` gives `set-cookie` sent several times as
  a list.
- `redirect(url, status)` takes 301, 302, 303 (the default), 307 and 308; another status
  is an error. `Location` is written as a URL holds it: what a URL cannot hold is
  percent-encoded (`/a b` is `/a%20b`, a `%XX` already there is kept), a non-ASCII host is
  written as IDNA (`bücher.de` is `xn--bcher-kva.de`), and a host IDNA cannot write, a tab
  or a line break, is an error. A `\` is read as browsers read it: in `http(s)`, `ws(s)`,
  `ftp`, `file` and relative URLs it is a `/` before the query, so a check that the URL
  starts with `https://good.com` holds for `https://good.com\@evil.com/`, which goes to
  good.com. A path of the site's own, starting with `/`, stays on the site: a `\` among its
  leading slashes is `%5C` (`/\evil.com` is `/%5Cevil.com`, so a check for `/` and not `//`
  holds), and its `.` and `..` segments (`%2e` too) are taken out before
  [`--base`](#options-of-quillo-run) is put in front, so `..` cannot climb out of it.
  A `javascript:`, `vbscript:` or `data:` location is refused, in any form browsers read
  (`JavaScript:`, a tab inside), by `redirect` and as a `location` header given to
  `respond`, `stream`, `events` or `site_headers`.
- `stream` and `events` refuse a status that has no body (204, 205, 304) and a 1xx, as
  their function would write one (`` `stream` cannot answer 204: that status has no body
  ``), and `events` refuses a `type` that is not text, as `stream` does.
- Every answer carries `server: Quillo` (unless the route sets `server`), `date`,
  `x-request-id` and `x-content-type-options: nosniff`. Every HTML page carries
  `x-frame-options: SAMEORIGIN`, unless the route sets one or a
  `content-security-policy` with `frame-ancestors`. A page made from who the visitor is
  (it called `session()` or `cookie()`, or set a cookie) carries `cache-control:
  private` and `vary: cookie`, unless the route set its own `cache-control`, so a shared
  cache never gives one visitor's page to another.
- `site_headers(map)` *(since 0.7)* at the top level adds headers to every answer that does not set
  them itself, and `site_headers("/admin", map)` to those under a path; a longer path's
  rule wins over a shorter one's. A `null` value takes away one Quillo sends. Names are
  checked, and values are one line; `content-length`, `transfer-encoding`, `connection`,
  `set-cookie` and `content-type` are refused, as a route sets them:

  ```quillo
  site_headers({ "content-security-policy": "default-src 'self'", "referrer-policy": "same-origin" })
  site_headers("/embed", { "x-frame-options": null, "content-security-policy": "frame-ancestors *" })
  ```
- Text answers are gzipped for browsers that accept it: a 200 with a body of 1 KiB or
  more whose type is `text/*` or names `json`, `javascript` or `svg`, with `vary:
  accept-encoding`, which the plain copy of such an answer, sent to a client that takes
  no gzip, carries too. `Accept-Encoding` is read as HTTP says: names in any case, `x-gzip`
  as `gzip`, `gzip;q=0` refusing it, and `*` standing for gzip when gzip is not named. Not when the route set `content-encoding`, not streams, not partial
  files (206). There is no brotli.
- A header cannot be added from a helper: what `respond`, `stream` and `events` are given
  is the answer's, and headers for every page are `site_headers`'. Cookies are the
  exception: `set_cookie` anywhere in the request joins it. `reply status respond(...)` keeps the `respond`'s headers and body with the new
  status.

`reply status [body]` ends the request right away, from anywhere: inside a
helper, a pipeline stage or a `??` fallback. This is how middleware works:

```quillo
let auth = {
    required: (req) =>
        req.headers.authorization == `Bearer ${env("TOKEN")}`
            ? req
            : reply 401 { error: "unauthorized" },
}

route GET /me = request | auth.required | (r) => { ip: r.ip }

route GET /users/:id(int) {
    let user = users | find((u) => u.id == id) ?? reply 404 { error: `no user ${id}` }
    user
}
```

### What a route cannot do

The questions that come up once an app is past its first pages, answered from the
server's code:

- **A hook before every route, or a global error handler:** the hook is a `routes /`
  group with middleware around the routes that need it (see [Route groups](#route-groups));
  for one route, a pipeline stage that `reply`s (above). There is no global error
  handler: a runtime error is always the plain `internal error` 500, and a 404 page of
  your own is a last `route ANY /*rest`.
- **`request` in a helper:** `request` is read only in a route or a `server fn`; a
  function they call gets it as an argument. `session()`, `cookie(name)`, `tenant()`
  and `locale()` work in any function the request runs.
- **A response header from a helper:** no; see [What a route returns](#what-a-route-returns).
- **HTTPS, HTTP/2, HTTP/3 for pages:** the server speaks HTTP/1.1 over plain TCP
  (keep-alive, pipelining, chunked bodies and `Expect: 100-continue` included), and
  HTTP/1.0 as 1.0 frames it; a proxy in front terminates TLS. The option [`--tls-cert`](#options-of-quillo-run) is for WebTransport only (see [Channels](#channels)).
- **A WebSocket route of your own:** no. Upgrades are taken only on Quillo's own paths
  (live data, channels, [`--watch`](#options-of-quillo-run)); a route gets a plain request. Pages hear from the
  server through `live`, `events`/`listen`, or channels.
- **Bytes:** there is no bytes value. A body arrives as text (above), a route answers
  text, HTML or JSON, and `stream` sends text or lines of JSON. Files are the way bytes
  go out: `file()` sends one as it is, so write what you made (a PDF, an image) under the
  program's folder and serve it from there. The body is read whole, up to [`--max-body`](#options-of-quillo-run),
  before the route runs: it cannot be read as it arrives.
- **A time limit on a route:** for computing, starting the program with [`--max-compute 2s`](#options-of-quillo-run) (`quillo run main.qwe --max-compute 2s`) ends a request that
  computes that long without letting go, as a 500; see
  [Concurrency](#concurrency-and-reloading). A wait is not limited: a route waiting on
  something that never comes holds its coroutine for good, and no one else. Also limited
  is the socket: a request head not whole after 30 s, or a body of which nothing has
  come for 30 s, is answered 408 and dropped (a body that keeps coming may take as long
  as it takes), an idle keep-alive connection is closed after 60 s, and a client that
  stops reading its answer is let go after 30 s.
- **Rate limiting:** none for routes (a `rate_limit` builtin was removed). Count what
  matters in a table of your own: wrong PINs per account, uploads per hour. What is
  limited by itself: 256 large files being sent at once (8 per client), 4096 pages
  waiting for live data (256 per client), answered 503 with `retry-after: 5`.
- **The visitor behind a proxy:** `request.ip` is the TCP peer, unless [`--trust-proxy`](#options-of-quillo-run)
  names it (see [Live data](#live-data)): `X-Forwarded-For` is read from no one else. An
  `x-request-id` a proxy sends is kept and echoed.
- **Matching the host or a subdomain in the route line:** no; match the `host` header
  in a request pattern, or use `tenant_from` (see [Many tenants](#many-tenants)).
- **Logs anywhere but stderr:** the request log and errors go to stderr, text or JSON
  (see [Logs and metrics](#logs-and-metrics)); a service manager or a container collects
  them. There is no log file or syslog option.
- **Quillo's own addresses.** Under `/_quillo/` the server answers: `health`, `ready`
  and, with [`--metrics`](#options-of-quillo-run), `metrics`; `live` (the live-data WebSocket, or a long poll where
  it cannot open); `wt` and `wt-ticket` (channels); `events`, `dev` and `src/...`
  (`--watch`); `quillo.js`, `media.js`, `b/<hash>.js`, `r/<hash>.js` and
  `s/<hash>.css` (what pages load); `rpc/<name>` (`server fn` calls) and `markdown`;
  `service/<name>` and `join` (services); `announce` ([`--register`](#options-of-quillo-run)); `describe`
  ([`--describe`](#options-of-quillo-run)); `posted/<id>` (in an app, a form's answer, shown once). `job/<name>`, `channel/<name>`, `udp/<port>` and `later` are the
  process's own, for its instances, and are 404 to anyone else. Anything else there is
  404. Under a mount or [`--base`](#options-of-quillo-run), `/shop/_quillo/...` serves the mounted program's
  pages' needs (its bundle, calls, live data) and its `ready`; `health` and `metrics`
  are the process's, at the root.

### Idempotent requests

A client that sent a payment and got no answer (a timeout, a dropped connection) cannot
tell whether it was made. `idempotent(f)` lets it send the request again: a request
with an `Idempotency-Key` header runs once, and the same request again with the same
key gets the first answer, without running again. It works as Stripe's API does
([idempotent requests](https://docs.stripe.com/api/idempotent_requests); the header is
the IETF's [draft](https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/)):

```quillo
route POST /payments { json: { to: str, amount: dec } } = idempotent(() => {
    let id = pay(to, amount)?
    respond(201, { id })
})

server fn transfer(from: str, to: str, amount: str) =
    idempotent({ isolation: "serializable", retries: 5 }, () => move(from, to, amount))
```

A client of the route sends the key in a header, and the same key again when it retries:

```sh
curl localhost:8080/payments -H "Idempotency-Key: $(uuidgen)" --json '{"to": "bob", "amount": "12.50"}'
```

A page calls the `server fn` as any other; it makes the key and sends it by itself, so
the component has nothing to add for it:

```quillo
component Pay() {
    state sent = null
    <div>
        <button onclick={() => { sent = transfer("ana", "bob", "12.50") }}>Send 12.50 to Bob</button>
        {is_err(sent) ? <p>Not sent: {error(sent)}</p> : sent && <p>Sent, transfer {sent.id}</p>}
    </div>
}
```

Sent twice with one key, the second answer is the first, with `Idempotent-Replayed: true`,
and `move` has run once:

```sh
curl localhost:8080/_quillo/rpc/transfer -H "Idempotency-Key: k-77" --json '["ana", "bob", "12.50"]'
# {"ok":{"id":1}}
curl -i localhost:8080/_quillo/rpc/transfer -H "Idempotency-Key: k-77" --json '["ana", "bob", "12.50"]'
# idempotent-replayed: true
# {"ok":{"id":1}}
```

- The client makes up the key (a UUID; 1 to 255 visible characters, else 400), and sends
  the same one when it sends the request again. A request with the key and another
  method, path, query or body is refused with 422. One that comes while the first is
  still running gets 409, and should be sent again a moment later. Neither runs.
- The answer kept is the status, the headers (not `Set-Cookie`) and the body; a request
  again gets it with `Idempotent-Replayed: true`. A streamed answer (`stream`, `events`)
  cannot be kept. Answers are kept 24 hours, then the key is new again: `{ max_age:
  "7d" }` (seconds, or a duration as for cookies, up to 366 days) keeps them longer.
  Expired ones are deleted as requests come. When the database that keeps them cannot
  be reached, the request is answered 503.
- A key runs at most once, whoever asks, within the tenant and the mount point. Who may
  see its answer is compared at the strongest level both sides have: the `user` option
  (a str or an int, as `{ user: token.owner }`), then the session (a hidden id its cookie
  carries, or an older cookie's `session().user`), then each of `Authorization`,
  `Proxy-Authorization`, `X-Api-Key`, `Api-Key`, `X-Auth-Token` and `X-Access-Token`,
  then nobody. The same gets the answer. Different, or no level in common, gets 409, with
  nothing run or shown, also after signing out or losing a cookie: the work is never done
  twice. A plain cookie is not seen. The `user` option gives each user a row of their
  own, so two accounts may use one key; clients sharing keys without it get the 409, so
  pass `{ user }` or use unique keys. A header key and a form field's key are two keys,
  and rows from before 0.8 have no owners.
- Without the header, `f` simply runs; `{ required: true }` answers 400 instead.
  Outside a request (a script, a job), `f` runs.
- **A plain form** cannot send a header, so `<form method="post" idempotent>` carries a key
  of its own, made as the page is rendered, in a hidden field (`_idempotency_key`) that
  `idempotent` reads where there is no header, `{ required: true }` included. The browser
  shows the answer to the last of two clicks, so that one waits for the first and gets
  its answer, not a 409, and the form edited and sent again is another request, not a
  422. A page with such a key is never kept by a cache.

  The whole round trip: the page with its form, and the route it posts to, which places
  the order once however many times the form is sent (a double click, the browser sending
  it again after an answer was lost):

  ```quillo
  db.open("shop.db")
  db.exec("create table if not exists orders (id integer primary key, item text not null, qty int not null)")

  route GET /order =
      <html><body>
          <form method="post" action="/order" idempotent>
              <input name="item" value="Coffee beans"/>
              <input type="number" name="qty" value="1"/>
              <button>Place the order</button>
          </form>
      </body></html>

  route POST /order { form: { item: str, qty: int } } with db = idempotent(() => {
      let o = db.one("insert into orders (item, qty) values (?, ?) returning id", item, qty)
      respond(201, <p>Order {o.id} placed: {qty} × {item}</p>)
  })
  ```

  The page the browser gets has the key in the form, a new one each time the page is made:

  ```html
  <form method="post" action="/order"><input type="hidden" name="_idempotency_key" value="5760b74f-41cb-4067-a450-986f06988301">...
  ```

  The form sent twice with that key is one order. The second answer is the first one,
  with `idempotent-replayed: true`, and the table has one row:

  ```sh
  curl -i localhost:8080/order -H "Origin: http://localhost:8080" -d item=Tea -d qty=2 -d _idempotency_key=5760b74f-...
  # HTTP/1.1 201 Created
  # <p>Order 1 placed: 2 × Tea</p>
  curl -i localhost:8080/order -H "Origin: http://localhost:8080" -d item=Tea -d qty=2 -d _idempotency_key=5760b74f-...
  # HTTP/1.1 201 Created
  # idempotent-replayed: true
  # <p>Order 1 placed: 2 × Tea</p>
  ```

  The visitor who clicked twice sees "Order 1 placed" once, as the page the second click
  brings. Loading `/order` again gives a new key, so a second order on purpose is a new
  order (2), and so is the form sent again with `qty` changed.
- In a function a service answers with, the key is the call's own id, which is the same
  for each try, so a step is one word: `server fn charge(ref: str, cents: int) =
  idempotent(() => ...)`. Its value is kept and given again, and what would refuse a
  request is a runtime error there, so the call is sent again. It holds across a crash,
  which a service's memory of its calls does not (see [Services](#services)).
- `idempotent` answers the request with what `f` gives, as the route or server fn would
  have: it ends the request, so make it the last thing. What comes before it (an
  `auth.required`, a pattern, a 400 for a bad form) runs every time and is not kept,
  as Stripe keeps nothing for a request that failed validation.
- A server fn whose body calls it gets a key from the page with each call. The page sends the
  call again, with its key, when the connection fails, or the answer is 409 or 502 to
  504: up to three more times, a quarter, half and a whole second later.

**What is guaranteed.** The answers are kept in `_quillo_idempotency`, in the program's
database (the tenant's, with a database per tenant), and `f` runs in a transaction on
that database, which `idempotent` begins (so it takes `isolation` and `retries`, and is
called outside `db.transaction`). The answer is written in that transaction, before it
commits: the work and its answer are kept together, or neither is.

- Before running, the key is claimed in a statement of its own, so that another request
  with it sees it running (409).
- When `f` returns, its answer is written and the transaction commits; then the answer
  is sent. When `f` fails, returns an error value or replies early, the transaction
  rolls back, the claim is given up, and the answer is sent but not kept: nothing
  happened, so a retry runs again. To keep a refusal ("not enough money"), return it:
  `respond(422, { error })` commits (what `f` wrote with it too) and is kept. A server
  fn's error value is not kept: the page's retry asks again.
- If the server stops (or loses the database) part way, the database rolls the
  transaction back, and the claim is left. A minute after it was made, the next request
  with the key, from the same visitor, takes it over and runs; without a transaction
  (`transaction: false`) every try gets 409 until the key expires, so keep `max_age`
  longer than such work. If the first was only slow, and is still running
  then, at most one of the two commits: the claim is taken over only while it is not
  answered, and an answer is written only by the claim's holder, in its transaction; the
  other rolls back and gets the first's answer, or 409.
- On SQLite a transaction holds the database, so a second request with the key that
  comes while the first runs waits for it, and gets its answer. On PostgreSQL and MySQL
  it gets 409.
- Work outside that database is not in the transaction: `http.post` to a payment
  provider, another `db.connect`, files. For a handler that is mostly that,
  `{ transaction: false }` runs `f` without one, and keeps every answer, failures too
  (as Stripe does), since part of the work may have happened. A server that stops there
  leaves the key running: it is not known whether the work happened, so requests with
  the key get 409 until it expires. Give the provider the same key, if it takes one,
  so that it too makes the payment once.
- A [service](#services)'s function that changes data is such work too: it commits in the
  service, not in this transaction. When its answer is lost (the service stopped part way,
  the network dropped it), the call fails, the transaction rolls back and gives the key up,
  and the client's retry with it calls the service again, under a new call id, so the
  service changes its data twice. `quillo check` warns where an `idempotent` run in its
  transaction reaches such a function, a site's too, directly or through the program's
  functions, and through a service that calls another: ``main.qwe:6: warning: `idempotent`
  reaches `billing.charge`, which may change data in another program, outside its
  transaction: ...``. Give it `{ transaction: false }`, or send an id the service's change
  is keyed by (`insert ... on conflict do nothing` on an order's id), so that a second run
  changes nothing. A call made later (`billing.later.charge(...)`) waits in this
  transaction's database, and is not warned of.

### Prefetching

Every page a route makes asks for the pages it links to before they are clicked: a
link of the same site pointed at (for 65 ms), pressed, touched or reached with Tab. By
the time the click lands, the page is there. Browsers with speculation rules (Chrome,
Edge) keep the page for the navigation themselves; others get it in the background and
keep it in their cache for 30 seconds, which the server allows for this, unless someone
has logged in or out.

- **Prefetching changes nothing.** A route asked for ahead of a click may read (the
  database, files, other servers) but not write: `db.exec` or SQL that writes, assigning
  a global variable, cookies and sessions, files, `publish`, `http.post` and the like,
  and calls to services stop it, and it is answered "not prefetched" (503, which the
  browser drops; the request log shows it as any 503, with no error line). The page is
  then made, as usual, when the link is followed. So a `GET` that does something (a
  logout link, a counter) is never done by pointing at it. A page prerendered runs none
  of its components, `server fn`s or live updates until someone opens it.
- **Links a page's components follow** are fetched for the state of the components on the
  page they lead to, so those start with their data and no `null` first (see
  [Single-page apps](#single-page-apps)). It is kept for 30 seconds, until a `server fn` is
  called, and only for components not already on the page; the others ask as usual.
- **Keeping up with changes:** a request that changes something, or logs someone in or
  out, answers with `Clear-Site-Data: "prefetchCache", "prerenderCache"`, so a page kept
  for a click is not shown out of date; Chrome obeys it from version 138. A `GET` does not
  send it, nor a `POST` that only reads. Elsewhere, a change that is not a login can be up
  to 30 seconds old on the click.
- **When the server is busy:** with anyone waiting, a prefetch is refused before it runs,
  so prefetches never go ahead of a visitor.
- **Seeing what it does:** the log marks them, `GET /docs -> 200 (0.4ms) prefetch`, and
  `"prefetch": true` under [`--log json`](#options-of-quillo-run). Started with [`--metrics`](#options-of-quillo-run)
  (`quillo run main.qwe --log json --metrics`), `quillo_prefetches_total` counts
  those `answered`, `refused`, and `used` by a click, as a page reports in Chrome and Edge.
- **What is left alone:** links to other sites, with `target` or `download`, to the
  same page (`#section`), anything inside an element with `data-no-prefetch`, and every
  link on a connection saving data.
- A route that sets its own `cache-control` keeps it. Starting the program with
  [`--no-prefetch`](#options-of-quillo-run), `quillo run main.qwe --no-prefetch`, turns prefetching off.
- **Pages sent as written:** a file `file()` or `files()` serves, and any answer with an
  `etag` or a range, goes out as its bytes are, so its `ETag` and `Content-Range` name what
  is sent: no prefetching script is added to it, nor [`--watch`](#options-of-quillo-run)'s.

### Streaming

A route can send its answer as it makes it: a large export row by row, a job's
progress, a model's reply word by word. The client has the head at once, and each
`send` as it is made.

```quillo
route GET /orders.csv with db = stream((send) => {
    send("id,total\n")
    for o in db.query("select id, total from orders") { send(`${o.id},${o.total}\n`) }
}, { type: "text/csv", headers: { "content-disposition": "attachment" } })

route GET /jobs/:id/progress = events((send) => {
    for step in range(10) {
        work(id, step)
        send({ done: step + 1, of: 10 }, "progress")   // a value, and an event name
    }
})

// A model's answer, passed on as it arrives.
route POST /ask with http, env = events((send) => http.stream("https://api.example.com/v1/chat", send, {
    body: { prompt: request.json.prompt, stream: true },
    headers: { authorization: `Bearer ${env("API_KEY")}` },
}))
```

- `stream(f)` sends what `send` is given: text as it is, other values as a line of
  JSON (newline-delimited JSON, with `type: "application/x-ndjson"`). `events(f)`
  sends Server-Sent Events: each value as JSON in an event's data, with a name if
  `send` is given one; `send(null)` is a comment, which keeps a quiet connection
  open. `send(value, { event, id, retry })` names the event, gives it an id, which the
  browser sends back as `Last-Event-ID` when it connects again, and says how many
  milliseconds a client waits before it does; `send(null, { id: "8" })` sends an id
  alone. Another option, or a `retry` that is not a whole number, ends the stream with
  an error. A program that goes on from the last id reads it in
  `request.headers["last-event-id"]`:

  ```quillo
  route GET /feed = events((send) => {
      let from = int(request.headers["last-event-id"] ?? "0") ?? 0
      for n in range(from + 1, from + 100) {
          send({ n }, { event: "tick", id: str(n), retry: 2000 })
          sleep(1000)
      }
  })
  ```
- `{ status, headers }` are those of [`respond`](#what-a-route-returns): a status with no
  body (204, 205, 304) or a 1xx is refused, so no function runs after a head that has no
  body to send, and a header's `null` is no such header.
- An HTTP/1.0 client, which reads no chunks, gets a stream with no length, ended by
  closing the connection.
- The rest of the server carries on meanwhile. A `send` waits only when the client
  reads slower than the program writes, and then it waits as a database call does,
  holding up no other request. Proxies are asked not to hold the answer back.
- When the client goes away (or the server stops), the next `send` ends the
  function, so a stream that runs until someone leaves stops when they do. An
  error in the function after the answer began cuts the connection, so the client
  knows the answer is incomplete.
- `http.stream(url, f, { method, body, headers })` reads another server's answer as
  it arrives: `f` gets each event (its data, parsed as JSON when it is, its name, its
  id and its `retry`: `(data, name, id, retry)`, the id being the last one the stream
  gave, as a browser keeps it), each line of newline-delimited JSON, or each piece of
  text, and may stop it by returning `false`. Given `send`, what arrives goes straight
  on. It gives `{ status, ok, headers, cookies }` once the answer has ended (with the
  `body` when it is not a success), or an error value when it could not be had (also
  when one event or line goes past 16 MiB without ending). The map also takes
  `redirects`, `public_only`, `ca`, `cert` and `key` (see [HTTP client](#http-client)).
- In a page, `receive(url, f, { method, body, headers })` in a handler sends the
  request (a file from an input as `body` goes as it is) and calls `f` with what the
  answer streams, until it ends; `listen(url,
  f)` in a component's code follows a stream for as long as the code that called it
  stands, and opens it again if the connection drops, sending the last event's id as
  `Last-Event-ID` and waiting the `retry` the server gave, if it gave one:

```quillo
component Chat() {
    state answer = ""
    <div>
        <button onclick={() => receive("/ask", (part) => { answer = answer + part.text }, { body: { prompt: "hi" } })}>Ask</button>
        <p>{answer}</p>
    </div>
}

component Progress(job: int) {
    state done = 0
    listen(`/jobs/${job}/progress`, (p) => { done = p.done })
    <progress value={done} max="10" />
}
```

Each open `listen` holds one of the six connections a browser keeps to a site over
HTTP/1.1, across its tabs. That suits a job's progress or a reply being written; for
data every page follows, `live` (see [Live data](#live-data)) shares one WebSocket.
Outside the server (a program's own requests in tests), the function runs first and
the whole answer is sent at once.

### UDP

A program can take UDP packets on a port, answer them, and send its own: readings from
sensors, syslog, a game's moves, metrics.

```quillo
let readings = []

udp.listen(9000, (p) => {
    readings = readings + [json.parse(p.text)]   // pages with live(...) on it follow
    udp.reply(p, "ok")                           // sent back from port 9000
})

udp.send("10.0.0.5", 514, "<14>app started")
```

A page that shows the last readings as they come, and a sensor's packet sent from a
terminal (`nc` stands in for the device):

```quillo
server fn latest() -> list = readings | drop(len(readings) - 10)     // the last ten

component Readings() {
    state rs = live(latest)
    <ul>{for r in rs => <li>{r.sensor}: {r.c} °C</li>}</ul>
}

route GET / = <html><body><Readings/></body></html>
```

```sh
echo '{"sensor": "hall", "c": 21.5}' | nc -u -w1 127.0.0.1 9000     # prints ok, the reply
```

Each packet adds a line, `hall: 21.5 °C`, to every page open on `/`, without a reload.

- `udp.listen(port, f, { host, queue })` is written at the top of a program. The port is
  taken when the program runs, so one that is in use is an error at once. `host` is the
  address to listen on: every IPv4 address, `"0.0.0.0"`, unless given; `"::"` for IPv6
  as well. There is no multicast, and packets are plain: no DTLS.
- `f` gets each packet: `{ text, bytes, from: { host, port }, port, at }`. `text` is the
  packet as text, or `null` when it is not UTF-8; `bytes` is its bytes as a list of
  numbers; `at` is when it came, in milliseconds since 1970. A handler written with no
  parameter is called with none.
- The handler runs like a request: waiting on a database or `http.*` holds up no one, and
  it sees the program's globals, so a page following them with `live` sees each packet.
  Packets reach it in the order they came, one at a time. An error in it is logged and
  the next packet goes on.
- Packets wait in a queue while the handler is busy (10,000 unless `queue` says
  otherwise, and 64 MiB at most). A full queue drops its oldest packet, as UDP would,
  except that a sender holding more than half of it while others wait has its new packet
  dropped, so one sender cannot push out the rest *(since 0.7)*. A batch the server could
  not take for five seconds is dropped with a line in the log; on a server started with
  [`--metrics`](#options-of-quillo-run) (`quillo run main.qwe --metrics`),
  `quillo_udp_received_total` and `quillo_udp_dropped_total` count them. Nothing slows
  a sender down, so where packets come in bursts faster than the handler takes them,
  give `queue` room for the largest burst.
- `udp.send(host, port, data, { from })` sends one packet; `udp.reply(packet, data)` sends
  back to where a packet came from, from the port it came to (as most protocols expect).
  `data` is text, a list of byte values, or any other value as JSON; a packet holds up to
  65,507 bytes. Both return `null`, or an error value when the host has no address or the
  system refuses the packet; more than 65,507 bytes, `null` data, or a `from` port the
  program does not listen on is a runtime error. A host name with both kinds of address
  is sent to over IPv4. They need the `net` effect.
- With [`--reload`](#options-of-quillo-run) and [`--watch`](#options-of-quillo-run), a port the program still listens on stays open, and its
  packets go to the new version's handler; a port it stopped listening on is closed
  (after 3 s, in case the next version takes it back).
- One program on a machine listens on a port: run with [`--instances`](#options-of-quillo-run), the instances share
  the socket. A program mounted in another (`mount /x = "x.qwe"`) gets its own ports' packets.

### Channels

A channel carries messages between the server and its pages as they happen: moves in a
game, cursors, readings from a device, a chat. Pages send to the program, which answers
one page, all of them, or none.

```quillo
channel.on("game", (msg, peer) => {
    // Everyone else in the channel sees the move.
    channel.broadcast("game", { id: peer.id, pos: msg.pos }, { except: peer })
})

component Board() {
    state others = {}
    let game = channel.open("game", (m) => { others = set(others, m.id, m.pos) })
    <canvas onpointermove={(e) => game.send({ pos: [e.clientX, e.clientY] })}/>
}
```

- In a component, `channel.open(name, f)` opens a channel for as long as the code that
  called it stands (the component leaving the page closes it): `f` gets each message the
  server sends on it, and `.send(value)` sends one to the server. On the server, while
  the page is rendered, it opens nothing.
- On the server, `channel.on(name, f, { join, leave, tenants })` is written at the top of a
  program, once for a name. A name is letters, digits, `_`, `-`, `.` and `:`, at most
  64 characters. `f(msg, peer)` gets each message from a page; `peer` is `{ id,
  session }`, its `id` twelve hex characters made for the connection (a page that
  reconnects has a new one), and `session()` in `f` is the page's session too, as it
  was when the page connected: someone who signs in afterwards is seen so once the page
  reconnects. So is its tenant. Messages reach `f` in the order they came, one at a time,
  as UDP packets do (see [UDP](#udp)): like a request, with waits holding up no one. An
  error in `f` is logged and the next message goes on. Each of `f`, `join` and `leave`
  is called with as many arguments as it declares.
- `join(peer)` decides whether a page may join (anything but `null`, `false` or an error
  says yes): anyone who can open a page can ask, so a channel whose messages are not for
  everyone has one (`(peer) => peer.session.user != null`). `leave(peer)` hears that a
  page left, or closed (for each channel it was in; not when the server itself
  stops). A channel no `channel.on` names may be joined, to hear what the program
  broadcasts on it; what pages send on it goes nowhere. There is no list of who is
  present beyond `channel.peers` and what `join` and `leave` record. What `join`
  broadcasts on its channel, or sends to the page that is joining, reaches that page
  too, right after its yes: a roster broadcast as someone enters is the first thing
  they hear. So does what is broadcast between the yes and the page hearing it. A page
  asks to join a room once at a time, and each `join` that said yes gets exactly one
  `leave`, also when the page left or closed while its `join` was still running, so a
  count kept by the two (`online += 1`, `online -= 1`) stays right.
- **Each tenant has its own room** of a channel (see [Many tenants](#many-tenants)): pages on
  `acme.example.com` and `evil.example.com` joining `lobby` are in two rooms *(since 0.7:
  one room for all)*. What code run for a page or a request broadcasts, sends or lists
  (`channel.peers`) is its tenant's room's; with no tenant, the bare domain's, the room of
  no tenant. Code run for no one, a job, a tick or the top level, reaches every tenant's
  room, and `with_tenant(name, f)` one tenant's. `{ tenants: "shared" }` makes one room
  for every tenant. Under `--split`, a service's channel is per tenant whatever it says.
  Pages may ask of 4,096 channel names in all; past that a page asking to join is told no.

  A chat for people who are signed in, with both sides:

  ```quillo
  channel.on("lobby", (msg, peer) => {
      channel.broadcast("lobby", { from: peer.session.user, text: msg.text })
  }, {
      join: (peer) => {
          if peer.session.user == null { return false }      // signed-in people only
          channel.broadcast("lobby", { from: peer.session.user, text: "joined" })
          true
      },
      leave: (peer) => channel.broadcast("lobby", { from: peer.session.user, text: "left" }),
  })

  component Lobby() {
      state lines = []
      state draft = ""
      let room = channel.open("lobby", (m) => { lines = lines + [`${m.from}: ${m.text}`] })
      <div>
          <ul>{for l in lines => <li>{l}</li>}</ul>
          <form onsubmit={() => { room.send({ text: draft }); draft = "" }}>
              <input value={draft}/>
              <button>Send</button>
          </form>
      </div>
  }

  route GET / = <html><body><Lobby/></body></html>
  ```

  Ana, signed in, sees `ana: joined` first, then each line anyone sends, her own too
  (`ana: hi all`), since the broadcast has no `except`. A visitor with no session is
  refused at `join`: the list stays empty, and what they send goes nowhere.
- `channel.broadcast(name, value, { except, reliable })` sends to every page in the
  channel and gives how many took it; `channel.send(name, peer, value, { reliable })`
  sends to one (the peer, or its id, in a room the code reaches), and gives whether it was there;
  `channel.peers(name)` gives the ids of the pages in it. They need the `net` effect,
  and wait for nothing: a page that cannot keep up misses messages rather than holding
  up the program. A message is a value, as JSON, up to 1 MiB (bytes go as a list of
  numbers; there is no binary message). Each page has room for 4096 messages on their
  way to it: when it is full, a plain message is dropped and counted, and a `reliable`
  one closes the connection, so the page comes back and joins again rather than miss
  it. A channel keeps 10,000 messages from pages waiting for its handler; past that the
  oldest goes. Nothing is kept for a page while it is away.
- In the page, messages sent before the join is answered, or while the connection is
  down, wait (up to 256 a channel) and go once it is joined; sent on a channel that
  refused the page, they are dropped. `.send` gives `null`.

How messages travel:

- A page opens one connection for all its channels. Where the browser has WebTransport
  (HTTP/3, over UDP), a message that fits in a datagram (about 1,100 bytes as JSON) goes
  as one: it arrives as soon as it can, may come out of order, and may be lost, as a
  game's moves or a sensor's readings can afford to be. Larger messages, and those sent
  with `{ reliable: true }` (from either side: `game.send(v, { reliable: true })`), go on
  a stream: all of them, in order.
- Where WebTransport cannot be had (the browser lacks it, UDP is blocked, the page is
  served from a name its certificate does not cover, or it is not ready within 1.5 s),
  the page uses a WebSocket for the rest of its life, which carries messages in order,
  none lost while the page keeps up, even in a burst of thousands (`reliable` changes
  nothing there but past the queue's limit: one 4,096 messages behind loses a plain
  message and is closed for a `reliable` one, as above). The connection is opened
  again when it drops (after half a second, then longer, up to 15 s), the channels
  joined again, and what was queued sent. A page that also shows live data has two
  connections, `/_quillo/live` and `/_quillo/wt`. In a phone app or `quillo app`, which
  have no socket, both go through the app's bridge instead.
- The server listens for WebTransport on the UDP port with the HTTP port's number
  (start the program with [`--wt-port`](#options-of-quillo-run) for another, `quillo run main.qwe --wt-port 4433`), from when the first page asks to connect, on every address
  of the machine unless [`--host`](#options-of-quillo-run) is a loopback address. It makes a certificate of its
  own, good for 13 days and replaced after 11, which browsers take by its hash (they
  take such certificates for two weeks at most); [`--tls-cert`](#options-of-quillo-run) and [`--tls-key`](#options-of-quillo-run), given
  together, give it a real one. A WebTransport connection is kept alive every 5 s and
  dropped after 20 s of silence. Behind a proxy that passes HTTP only, open the UDP
  port to the server itself, or pages use the WebSocket. [`--no-wt`](#options-of-quillo-run) turns WebTransport
  off, as does a build without the `realtime` feature. The browser's Network panel shows
  a WebTransport session open but none of its messages, so to watch a channel's messages
  there, run with `--no-wt`: each one is then a WebSocket frame (`["m","room",{...}]`)
  (FEEDBACK42.md, Q-485).
- Broadcasts reach the pages connected to this server (its [`--instances`](#options-of-quillo-run) share them;
  servers behind limer do not relay them, see [Several servers](#several-servers)).
  With [`--metrics`](#options-of-quillo-run), `quillo_channel_pages` counts them and `quillo_channel_dropped_total`
  the messages dropped because a queue was full. The server keeps channels in memory:
  after a restart, pages reconnect and join again, with new ids, and queued messages are
  what each page kept.
- There is no WebSocket or WebRTC of your own: pages talk to the server, never to each
  other directly, and a channel is the way a page's messages reach the program.

UDP and channels together pass a device's packets on to pages as they come; see
[telemetry.qwe](https://github.com/quillolang/examples/blob/main/telemetry.qwe).

### Media

A program can take a camera's or an encoder's stream as RTP, or a page's camera, and
play it in its pages, a fraction of a second behind:

```quillo
media.rtp("cam", { video: 5004, audio: 5006 })
media.on("cam", { watch: (peer) => peer.session.user != null })

route GET / = <html><body><quillo-media stream="cam" controls muted/></body></html>
```

```sh
ffmpeg -f avfoundation -framerate 30 -video_size 1280x720 -i "0:0" \
  -map 0:v -c:v libx264 -tune zerolatency -profile:v baseline -pix_fmt yuv420p -g 60 \
  -f rtp rtp://127.0.0.1:5004 -map 0:a -c:a libopus -f rtp rtp://127.0.0.1:5006
```

- `media.rtp(name, { video, audio, host })` makes a stream: at the top of a program, or
  while it runs (a route, a channel's handler: a stream for each room), once for a name
  (asked again, it is the same stream; names are as channels'). Video is
  H.264 and audio Opus, the only codecs taken, each on a port of its own (`{ port: 5004,
  codec: "h264" }` says so in full). The packets never reach Quillo code: the server puts
  them back in order (waiting 50 ms, or 256 packets, for a late one), makes frames of
  them (it reads single NAL units, STAP-A and FU-A), and passes each on to every page
  watching, without decoding or encoding anything. Any sender that reaches the port is
  taken (the payload type and SSRC are not checked), and there is no RTCP and no SRTP:
  keep the ports to the network the camera is on. The server keeps at most 600 frames
  since the last keyframe, and 32 MiB of them (256 MiB for all streams: past that a stream
  keeps none, and a page that starts watching it waits for its next keyframe).
- **Who may watch** is `media.on(name, { watch })`'s `watch(peer)`, asked as a channel's
  `join` is, with the page's session. A stream no `watch` names is watched by no page
  *(since 0.7: any page could watch any stream)*.
- `<quillo-media stream="cam">` in any page plays the stream: the page loads a small
  script for it (4.5 KB compressed, `/_quillo/media.js`, put into any page that writes
  the element), which decodes with the browser's own decoders (WebCodecs) and draws on a
  canvas, about 120 ms behind the frames it has. `controls` adds a button for sound, and
  `muted` starts without it (browsers play sound only after a click on the page; a muted
  player does not decode the sound at all). Its `data-state` is `connecting`,
  `waiting`, `playing`, `ended` or `unsupported` (no WebCodecs), for styling, and
  `el.mediaStats` counts what it decoded.
- Frames travel on the page's channel connection: over WebTransport, a stream per
  track; else over the WebSocket. The server keeps the video since the last keyframe, so
  a page that starts watching shows a picture at once. A page that falls behind skips
  to the next keyframe, and so does a stream that lost a packet of video.
- A connection that drops is opened again (after half a second, then longer, up to 15
  seconds) and the page goes on watching, as a page that starts does. With no frames for 3
  seconds (the camera stopped, the connection is down), `data-state` is `waiting`.
- `media.streams()` gives a list, by name: `{ name, viewers, video: { frames, fps,
  kbps, lost }, audio: { ... } }`, the viewers being this server's pages, and `fps` and
  `kbps` 0 after 3 s without frames.
- **Pages send too.** `media.on(name, { publish, watch, tenants })` lets pages send stream `name` (or
  every stream a name ending in `*` starts: `"call:*"`), and
  `<quillo-media publish="call:42">` sends the page's camera and microphone to it, for
  every `<quillo-media stream="call:42">` to play. The browser encodes them with WebCodecs
  (H.264 baseline at 1280x720, 30 frames a second and 1.5 Mbit/s with a keyframe every
  two seconds, and Opus at 64 kbit/s, what the relay passes on) and sends the frames on
  its channel connection; the server passes them on as it does RTP, decoding nothing.
  Sound needs Chromium's capture API (`MediaStreamTrackProcessor`): a browser without it
  sends the picture alone. `publish(peer)` says which pages may, as a channel's `join`
  does (with the page's `session()`); without it, no page may *(since 0.7: any page
  could)*. A video call is a stream per person:

  ```quillo
  media.on("call:*", { publish: (peer) => session().user != null, watch: (peer) => session().user != null })

  component Call(room: str, me: str, others: list) {
      <div>
          <quillo-media publish={`call:${room}:${me}`}/>
          {for p in others => <quillo-media key={p} stream={`call:${room}:${p}`}/>}
      </div>
  }
  ```

  `no-video` or `no-audio` leaves one out. Given a MediaStream as its `source` (a
  canvas's, the screen's: `el.source = js("navigator.mediaDevices.getDisplayMedia")` in
  `@mount`), it sends that instead of the camera. Its `data-state` is `starting`,
  `sending`, `refused` (no camera, or `publish` said no), `ended` or `unsupported`, and
  `el.mediaStats` counts what it encoded and sent. Each person's picture goes once to the
  server, which sends it on to everyone watching (what video services call an SFU); there
  is no TURN relay for networks that block UDP, where the WebSocket carries it.
- **Each tenant has its own streams** of a name, as it has its own rooms of a channel
  (see [Channels](#channels)): acme's `call:42` is not evil's, and a tenant's page that
  watches a name it has no stream of watches the program's own RTP stream of that name.
  `tenants: "shared"` makes one stream for all. `media.streams()` lists the tenant's and
  the program's; `media.close(name)` ends the tenant's, and from a job, a tick or the top
  level every tenant's of that name. Pages make at most 1,024 streams, 256 a tenant, and a
  page watches at most 32 at once; a stream no page sends or watches any more goes.
- `media.close(name)` ends a stream: it stops taking RTP, lets its ports go (for another
  stream to take; it waits up to a second for that), and tells its pages, whose
  `data-state` turns `ended`, and stays so until the page reconnects, even if a stream
  of that name is made again. It gives true if there was such a stream.
- Streams are not recorded: frames go to the pages watching, and nowhere else. Write
  a recording with the encoder (ffmpeg's `-f tee`) beside what it sends. A stream lives
  on the server its packets come to: pages on another server behind a balancer do not
  see it.
- Audio and video are each played as they come, so they are in step within about a
  tenth of a second. The camera's encoder decides the picture: keyframes every second
  or two (`-g 60` at 30 fps) let pages that start watching (or fall behind) catch up
  quickly; `-tune zerolatency` keeps the encoder from holding frames back.

### Static files

```quillo
route GET /assets/*path = files("public", path)
route GET /build/*path = files("dist", path, { cache: "1y" })   // names that change with content
route GET /report = file("exports/report.pdf")
```

- **A path from the visitor goes to `files(folder, path)`** (below), which does not serve
  `.env`, `.git/` or the program's own files. `file()` serves the one file it is given,
  whatever its name: `file("public/" + path)` gives `/assets/.env` and `/assets/main.qwe`
  away *(the guide's example until 0.7)*, and `quillo check` warns about a `file()` whose
  path is made from text (FEEDBACK51.md, Q-657).

- `file(path, options)` takes three options and refuses any other, naming those it takes:
  `cache` (below), `download`, a name, which sends `Content-Disposition: attachment` with
  that name (written so a name in any script arrives whole), and `type`, the content type
  in place of the one the extension gives: `file("exports/r.csv", { download: "rapport
  d'été.csv", type: "text/csv; charset=utf-8" })`. An empty `type` or `download` is an
  error that says so, and so is a `type` with a line break or another control character.
- Each file goes out with an `ETag` (a weak one, from the file's size, time, inode and
  change time) and `Last-Modified`, and as it is on disk: nothing is added to an HTML
  file, so a range of it is the bytes its `Content-Range` names. Conditionals are as
  RFC 9110 has them. A browser asking with a current copy (`If-None-Match`, or an
  `If-Modified-Since` the file has not changed since, in any of HTTP's three date forms)
  gets a `304` with no body, to a `GET` or a `HEAD`; another method whose `If-None-Match`
  matches gets a `412`. `If-Match` that does not name the file's `ETag`, or an
  `If-Unmodified-Since` it has changed since, gets a `412`, on a `GET` as on a write. The
  file's own weak tag, with its `W/` or without, passes `If-Match` and `If-Range`, as
  nginx has it, so a download resumed or a video sought gets its part. A file replaced by
  one of the same size and modified time (`cp -p`, `touch -t`, a build that sets every
  file's time) is a new file, in memory and to the browser.
- The type comes from the extension: `html htm css js mjs json map webmanifest xml xsl
  txt csv svg png jpg jpeg gif webp avif ico mp4 m4v webm mp3 ogg oga wav wasm woff2
  woff ttf otf pdf zip`, and `md` (`text/markdown`), `vtt` (subtitles for `<track>`),
  `ics`, `rss`, `atom`, `jsonld`, `geojson`, `yaml`, `yml`, `toml`, `m3u8`, `mpd`, `ts`,
  `m4s` (HLS and DASH), `srt`, `tsv`, `bmp`, `tif`, `tiff`, `heic`, `jxl`, `mov`, `m4a`,
  `aac`, `flac` and `opus`, text ones with `charset=utf-8`; anything else is
  `application/octet-stream`, which `nosniff` keeps a browser from guessing at (give
  `type` then). `file()` is also the only
  way to send bytes as they are: a route's other answers are text, HTML or JSON, so a
  file made in the program (a PDF, an image from `image.convert`) is written under the
  program's folder and served from there.
- `Cache-Control` is `no-cache` (keep it, but ask each time) unless `cache` is given:
  seconds, or `"10m"`, `"12h"`, `"30d"`, `"1y"`, which allows keeping it that long
  without asking (`immutable`). Use it for files whose name changes with their content.
- `Range: bytes=…` gets a `206` with only those bytes (audio and video seeking; `If-Range`
  is honoured); the unit is read in any case (`Bytes=0-1`), the numbers are digits only
  (`bytes=+2-4` is no range, and gets the whole file), an end past the file is the
  file's end, a range that starts past the end gets a `416`, and several ranges at once
  the whole file.
- Files up to 1 MiB are kept in memory (64 MiB in all) and re-read when they change on
  disk, or are replaced. A file changed in the last moment (50 ms, or 2 s where the file
  system keeps whole seconds) is read but not kept, as git does, so one being written is
  never kept half written. A file rewritten in place while it is read is served as it was
  read (a short range says what it sent), and only one replaced at every try is
  answered `503` with `Retry-After`; a busy file is never a 404. Larger ones are sent
  from disk as they are written, on a thread of their own
  with a 64 KiB buffer, so a video costs no more memory than a page, and read a few at a
  time, so many cold files at once do not fill the memory. At most 256 such
  sends run at once, 8 for one client (an IPv4 address, or an IPv6 /64 network; past
  that, a `503` with `Retry-After`); a `HEAD`, and a range of 1 MiB or less, take no place
  among them. A reader slower than 16 KiB/s is let go after 20
  seconds, and any send stops after an hour: a browser or player goes on from there
  with a `Range` request. Behind a proxy every visitor shares the proxy's address and so
  its 8 places; limit clients at the proxy instead. The gzipped copy of a text file up
  to 1 MiB is kept too (32 MiB of them in all), rather than compressed again for each
  request. Its plain copy says `Vary: accept-encoding` as the gzipped one does, so a
  shared cache keeps both, and a `HEAD` answers as a `GET` would, gzip included.
- Paths with `..`, or starting at `/`, get a `404`.

A folder of files, as a static host serves it, is `files(folder, path, options)`:

```quillo
route GET /*path = files("site", path)
route GET /docs/*p = files("manual", p, { index: "home.htm", extensions: ["htm"], not_found: null, cache: "1h" })
```

- `/` and `/docs/` are their folder's `index.html`, `/about` finds `about.html`, and a
  folder named without its `/` (`/docs`) is redirected to it (301), so its pages'
  relative links work (`//docs` goes to `/docs/`, never to another host). A miss is the folder's `404.html`, sent with status 404, when it
  has one; else the plain 404. The page of a miss is `no-cache`, with `cache` or without
  (a file asked for before it was deployed is not missing in caches for a year), and is
  sent whole whatever `Range` or conditional the request had. A route's URLs are the
  program's: `files()` serves what it is asked for, `/build/a.txt/` as `a.txt`, so a
  site whose URLs end in `/` keeps them.
- Files whose names start with a dot (`.env`, `.git/`) are not served, but `.well-known/`
  is; nor are the program's own (`.qwe`, `.qwer`, `.rs`), however the name is written for
  the file system: `lib.QWE` on macOS and Windows, `lib.qwe.` and `lib.qwe::$DATA` on
  Windows *(since 0.7)*, and a Windows short name (`ENV~1`) is refused. Each file goes as `file()`
  sends it, its `ETag`, ranges and type included. A path that leads out of the program's
  folder through a link is a miss, whether what it leads to is there or not (see [Paths
  and links](#paths-and-links)). `files` and `file` need the `fs` effect.
- A file is judged by where it leads, not by the name asked for: the index, extension and
  `404.html` pages too. A link inside the folder to an ordinary file in it is served; a
  link to `.env` or `main.qwe` under a harmless name (`settings.txt -> ../.env`) is not.
  Links inside the served folder are followed only through the dot folders a build or a
  package manager makes: `.pnpm`, `.vitepress/dist`, `.output/public`, `.next/static`,
  `.svelte-kit/output/client`, `.nuxt/dist/client`, `.vercel/output/static`, `.well-known`
  at the top, and `.pnpm`, `.deno`, `.bun` and `.store` directly under `node_modules`.
  Other dot folders (`.git`, `.ssh`, `.quillo`, `.vscode`, `.docker`, `.config`, `.cache`
  and the tools' server parts) stay refused.
- The program's own files are known by their device and inode as well as by name: the
  `.env` and `.env.*` (a service's too), the [`--env-file`](#options-of-quillo-run) and
  [`--tls-key`](#options-of-quillo-run) (kept by the path given, so a key renewed through
  a link, as certbot does, stays refused), the `.qwe`, `.qwer` and `.rs` files loaded, and
  the databases open. So a hard link to one, or one saved anew, is refused too.
- No SQLite database is served by `files()` or `quillo serve`, whatever its name:
  each is known by its content (the header, a `-wal`, `-shm` or `-journal` beside it), the
  program's, a tenant's, another program's, or one no one has open. `file("backup.db")`
  sends one named on purpose, with ranges.
- Only regular files are served (not a FIFO, a device or a folder), and the file sent is
  the one judged: a file swapped for a link to a protected one between the look and the
  open is a miss, and one saved anew is served as it is now. A file kept back is the
  miss a file not there is, with the folder's `404.html`.
- The options: `index` (a file name, `index.html` unless given), `extensions` (those a
  clean URL may leave out, `["html"]` unless given), `not_found` (the page for a miss,
  `404.html` unless given; `null` for none) and `cache`, as `file()`'s. Another option is
  a runtime error that lists them.

`quillo serve site` serves a folder that way with no program at all: `index.html`, clean
URLs and `404.html`, on [`--port`](#options-of-quillo-run) (8080 unless given: `quillo serve site --port 3000`) and with
the other [options of `run`](#options-of-quillo-run), read as `run` reads them. Its URLs are
its own, so a file asked for with a `/` after it (`/a.txt/`, `/about/`) is moved to its path
without one (301, `cache-control: max-age=3600`), only when it would be served there.
A Markdown file goes out as `text/markdown`; to show one as a page,
`markdown_page(read_file("notes.md"))` gives a whole page (a doctype, a `<head>` with
`<title>`, the text of the first level-one heading unless `title` is given, and `lang` and `html` as
options; `html: true` keeps HTML written in the text, as `markdown` does).

A file the program makes as it starts, a stylesheet compiled by an engine in Rust, is
declared with `build` at the top level:

```quillo
use "tailwind.rs"

build "/app.css" = tailwind(css`@import "tailwindcss";`, classes())
```

- It is made once the port is open, beside the requests, where work at the top level
  (`let APP = tailwind(...)`) holds the port shut until it ends. A request for it that
  comes first waits for it, and is not refused. The log says when it is done: `quillo:
  built /app.css in 1.5 s`.
- It is served at its path with the type its extension names (text; markup as HTML;
  another value as JSON; a `respond(...)` as it is), an `ETag` and `cache-control:
  no-cache`, so a browser's copy is checked rather than fetched again.
- One that fails at the first start stops the program with exit 1 (`quillo: build /app.css
  failed: no engine`). After a reload ([`--watch`](#options-of-quillo-run), [`--reload`](#options-of-quillo-run)) it is made again, and while
  that fails the last good one is served.
- The path starts with `/`, and holds no `?` and no trailing `/` (`` `build` names the path
  it is served at ``). Elsewhere `build` is an ordinary name.
- Ordinary top-level code still runs before the port opens, as a route may need what it
  sets: only what is declared as a `build` moves off that path. `quillo build` does not
  make it ahead of time.

### Translations

```quillo
translations("locales")          // locales/en.json, locales/fr.json, locales/pt-BR.json

view Header(user) = <header>
    <a href="/">{t("nav.home")}</a>
    <span>{t("greeting", { name: user.name })}</span>
    <span>{t("cart.items", { count: len(user.cart) })}</span>
</header>
```

```json
{ "nav": { "home": "Accueil" },
  "greeting": "Bonjour, {name} !",
  "cart": { "items": { "one": "{count} article", "other": "{count} articles" } } }
```

- The folder holds one JSON object per locale, `<dir>/<locale>.json`, the locale named
  as `en`, `pt-BR` or `zh-Hant`. A file named with `_` (`fr_FR.json`, as other tools
  write them) is read as `fr-FR`; two files for one locale are an error. A byte-order mark
  at a file's start, as Windows editors save one, is skipped.
- A request's locale is what `locale_from((request) => ...)` gives (a query parameter,
  a user's setting), else its `locale` cookie, else the best match in `Accept-Language`
  (`fr-CA` is served by `fr`, `pt` by `pt-BR`; `q` weights count, `Q=` too, `*` does not), else
  the default: `translations(dir, { default: "en" })`, or `en` when there is one, or
  else the first locale by name. `locale()` gives it. A choice of language a visitor makes
  is that cookie, set by a route the page's form posts to:

  ```quillo
  translations({ en: { hi: "Hello", lang: "Language" }, fr: { hi: "Bonjour", lang: "Langue" } })

  route GET / = <html lang={locale()}><body>
      <h1>{t("hi")}</h1>
      <form method="post" action="/lang">
          <select name="to">
              <option value="en" selected={locale() == "en"}>English</option>
              <option value="fr" selected={locale() == "fr"}>Français</option>
          </select>
          <button>{t("lang")}</button>
      </form>
  </body></html>

  route POST /lang { form: { to: str } } {
      if !contains(["en", "fr"], to) { reply 400 "No such language" }
      set_cookie("locale", to, { max_age: "1y" })     // read before Accept-Language
      redirect("/")
  }
  ```

  A browser that asks for `fr-CA` sees `Bonjour` at once; one that asks for nothing sees
  `Hello` until Français is chosen, and `Bonjour` from then on, with `<html lang="fr">`.
- The best match goes by script before language, with CLDR's likely scripts: `zh-TW`,
  `zh-HK`, `zh-MO` and `zh-Hant-*` get `zh-Hant`, and `zh-CN`, `zh-SG` and plain `zh`
  get `zh-Hans`; `sr-Latn-RS` gets `sr-Latn`, and `sr` gets `sr-Cyrl`. A locale in
  another script than the one asked is not taken over one that matches. A region takes
  CLDR's parent where there is one: `pt-AO` gets `pt-PT`, a Latin American `es-MX` gets
  `es-419`, and English outside the US gets `en-GB`.
- Nested keys are written with dots. A key missing in a locale comes from the default
  locale; a key missing there shows as itself.
- `{name}` is replaced by that value. An entry of plural forms (`zero one two few many
  other`) picks the form for `count` by the language's rules, CLDR 47's, which are built
  in, the same on the server and in the browser, for about 110 languages: English and the
  languages that count as it, French, Brazilian Portuguese and Hindi (where 0 is
  singular), European Portuguese (`pt-PT`, where only 1 is), Russian, Ukrainian and
  Belarusian, Serbian, Croatian and Bosnian, Polish, Czech and Slovak, Romanian, Lithuanian,
  Latvian, Slovenian, Icelandic, Macedonian, Danish, Sinhala, Filipino and Tagalog (`one`
  but for a last digit of 4, 6 or 9), Irish, Scottish Gaelic, Welsh, Maltese, Arabic,
  Hebrew, Lower and Upper Sorbian, Sami, and `other` alone for Japanese, Chinese, Korean,
  Thai, Vietnamese, Indonesian, Malay and their kin; old codes (`iw`, `jw`, `tl`) too.
  French, Spanish, Italian, Catalan and Portuguese take `many` for exact millions
  (`1000000`). A `zero` form, where one is written, is used for a 0 shown with no point
  (`0`, not `0.0`) in every language.
- `count` may be a number, or numeric text (`"1"`, as a query or a form gives it). The form
  is CLDR's for the number as `{count}` shows it, read from its digits (CLDR's operands:
  the whole part, and the fraction's digits with and without their trailing zeros), so a
  fraction is not one rule for all: `1.5` and `0.5` are `one` in French, `"1.0"` is `one`
  in Spanish and `other` in English ("1.0 items"), `2.0` is `two` in Arabic, and
  Slovenian's fractions are `few`. A decimal keeps its digits (`1.0m` is "1.0 items"). A
  whole float shows as a route writes it, `1.0`, so it takes `other` in English there,
  and as a component writes it, `1`, so it takes `one` there, on both sides alike (a
  float past 2^53 by the digits it shows too). `"1e3"` is 1000; `"0x1"`, `"Infinity"`
  and a full-width digit are not numbers, on either side, and take `other`.
- As it loads, a form the language never picks is warned of: ``translations (en): `a` has
  a `few` form, which en never picks (it has one, other)``.
- Translations are text, escaped in pages like any other; `raw(t(...))` for markup you
  wrote in them. In that form the values put in (`{name}`) are still escaped, on the
  server and in the browser: `raw(t("welcome", { name: user.name }))` keeps the
  translation's `<b>` and shows the name as text. (Only that form: `raw` of a variable
  holding `t(...)`'s text makes all of it HTML.)
- `with_locale("de", () => ...)` runs code in another locale (an email in the
  recipient's language).
- `translations({ en: {...}, fr: {...} })` takes the text directly instead of a folder.
- Components call `t` and `locale` too. A page with components loads its locale's text
  from `/_quillo/t/<hash>.js`, which the browser keeps, rather than carrying it in each
  page. It holds only the keys the components' code can ask for: each `t("literal")`,
  every key that starts with a template's literal start (`` t(`sec.${x}`) `` sends
  `sec.*`), and the `quillo.*` keys. A key worked out any other way (`t(k)`) sends all
  of them. Cached views are kept once per locale.
- `ask()`'s buttons follow the page's locale: the program's own `quillo.ok` and
  `quillo.cancel` translations when it has them, else Quillo's own for about thirty
  languages (Spanish Aceptar and Cancelar, French, German, Japanese, Chinese, Arabic,
  ...), else English.

### Mounting programs

`mount` serves another `.qwe` program under a path prefix:

```quillo
mount /notes = "notes.qwe"
mount /shop = "shop/app.qwe"
route GET / = <a href="/notes">Notes</a>
```

The mounted program runs unchanged, with its own globals and its own database, and
sees paths without the prefix (`url().path` is `/products`, not `/shop/products`).
Its root-relative links and form actions (`href`, `src`, `action`), its redirects, its
browser bundle, its `server fn` calls, live data and page changes are all moved under the
prefix. Its cookies are set for the prefix (`Path=/shop`), its session cookie is
`quillo_session` and is signed for that mount, so the two programs' sessions never pass
for each other. Its file paths count from its own folder; the services it declares are
shared with the program that mounts it. `/_quillo/health` and `/_quillo/metrics` are
the process's, at the root only. `mount`, like `service`, is written at the top level.
Changes to mounted files are picked up by [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`). See
[index.qwe](https://github.com/quillolang/examples/blob/main/index.qwe), which mounts every example.

### Services

A part of an app can run in the same process or as a server of its own, without its
code changing. `service` names a program whose `server fn`s the others call:

```quillo
// billing.qwe: the service
db.open("billing.db")
db.exec("create table if not exists invoices (id integer primary key, name text not null, cents int not null)")

server fn invoice(name: str, cents: int) -> map =
    db.one("insert into invoices (name, cents) values (?, ?) returning id", name, cents)

// shop.qwe
service billing = "billing.qwe"

route GET / =
    <form method="post" action="/buy">            // /shop/buy once served under /shop
        <input name="name" placeholder="Your name" required/>
        <button>Buy, 12.00</button>
    </form>

route POST /buy with http {
    let inv = billing.invoice(request.form.name, 1200)?    // billing's `server fn invoice`
    <p>Invoice #{inv.id}</p>
}

// app.qwe: the app, made of both
service /shop = "shop.qwe"          // named after the file; its routes under /shop
service /billing = "billing.qwe"
```

`quillo run app.qwe` serves the form at `/shop/`; sent, it shows `Invoice #1`, the row
billing wrote in its own database. The shop's own paths are written as the shop sees
them: `action="/buy"` reaches the page as `/shop/buy`, as the shop's links and redirects
do (`action="/shop/buy"` would become `/shop/shop/buy`). `quillo run app.qwe --split`
answers the same, with billing in a process of its own.

- A service's file is found as a `use`d file is: from the main file's folder (beside the
  real file when the main file is a link), and its own `use` paths with the same rules,
  a path without an extension being the `.qwe`, `.qwer` or `.rs` there. A Rust file a
  service's file uses is the service's: the service builds it, and the app passes over
  it when it reads the service's functions.
- **In one process** (the default) a service is loaded once and shared by every program
  in the process that names it: one copy of its variables and its database. A call
  goes straight to it.
- **Split**, a service runs elsewhere: the app started with the option
  [`--service-url`](#options-of-quillo-run), `quillo run app.qwe --service-url billing=http://10.0.0.5:8081` (or
  `QUILLO_SERVICE_BILLING`) sends its calls there, `POST /_quillo/service/invoice`,
  signed with HMAC-SHA256 under `QUILLO_SERVICE_SECRET` (the same on every service), or
  proven another way (see [Authentication between
  services](#authentication-between-services)). The signature covers the time (a call
  more than five minutes old is refused), the service it is for, the function, the
  tenant, the arguments, the call's id, the caller's name, its deadline and its
  contract, and the answer comes back signed with the same key. A
  service takes only calls for itself: its name is the program's file name
  (`billing.qwe` is `billing`), or [`--service-name billing`](#options-of-quillo-run) when the file is named
  otherwise. It remembers each call's id for longer than a signature lasts, so a call
  sent again, by a retry or by someone who saw it on the network, is answered as it
  was and not run again. A service with a database keeps that memory there too
  (`_quillo_calls`), so copies sharing it answer a call one of them answered, or say it
  is still running, whichever copy a balancer sends it to. Use HTTPS (or a private
  network) between services so calls cannot be seen. The tenant goes with the call. Only `server fn`s are offered. Its routes, under its prefix, are passed on to it,
  but not Quillo's own addresses there (`/billing/_quillo/rpc/...`, `.../service/...`,
  `.../job/...`). Run alone, a service is a program served as one:
  `quillo run billing.qwe --base /billing --as-service` ([`--base`](#options-of-quillo-run): the path it is
  served under, which its links and redirects carry; [`--as-service`](#options-of-quillo-run): see below).
- `quillo run app.qwe --split` runs each service the app declares (and those they
  declare) as a process of its own, on 127.0.0.1 at a port of its own (a free one; the
  same again when it is restarted), its log lines marked with its name, all signed with
  one secret (32 random bytes from the system, unless `QUILLO_SERVICE_SECRET` is set,
  so a copy started by hand elsewhere cannot join a split app unless the secret is
  set); they stop with it, after it: once the app has answered its requests in progress,
  which may still call them, each is told to stop (the pipe the app holds on its input
  closes) and finishes what it is doing as a first SIGTERM has it (a call, a call made
  later, a job), under the app's [`--drain`](#options-of-quillo-run), and the app waits for
  it. They run in a process group of their own, so a Ctrl-C in the terminal reaches the app
  alone; a service signalled itself while its app runs (systemd stops a unit by signalling
  each of its processes) says `stopping when the app stops` and serves the app until then.
  A second Ctrl-C ends them with the app, at once. When the app's process ends any other
  way (killed with SIGKILL, a crash), each stops as a first SIGTERM stops it, as the pipe
  on its input closes, so none is left serving with its port taken. A service started on its own, by hand or by systemd, serves on. Before any service starts, the app
  checks the whole tree once, its own files and every service's, and says each warning
  once; an error anywhere stops it there, before anything waits. Each is then started as
  `quillo run <file> --port P --as-service --service-name NAME`, with `--base`, [`--watch`](#options-of-quillo-run),
  [`--no-check`](#options-of-quillo-run), [`--no-build`](#options-of-quillo-run), [`--describe`](#options-of-quillo-run) and the [`--policy`](#options-of-quillo-run) files passed down, and
  [`--log`](#options-of-quillo-run), [`--workers`](#options-of-quillo-run), [`--max-body`](#options-of-quillo-run), [`--metrics`](#options-of-quillo-run), [`--no-jobs`](#options-of-quillo-run), [`--env-file`](#options-of-quillo-run) and
  `--service-url` not; it skips its own first check, which the app has made (a reload
  under `--watch` checks again). A service that is not up within 20 s stops the app with
  exit 1, and one whose process ends while starting is said at once: `service billing
  stopped at start (exit status: 1; see its lines above)`. Under `--watch` the app
  watches its services' files too, as in one process: a function added to `billing.qwe`
  reloads billing's process and the app, and the app calls it at once. A service whose process ends while the app runs is
  started again, the same way, after a second (twice as long each time it ends again
  soon, at most 30 s), with a line in the log. [`--scale risk=3`](#options-of-quillo-run) runs `risk` as three
  copies (up to 64), called in turn; they share its database, so give it one that
  several processes can open (SQLite on one machine, PostgreSQL across machines). Copies
  share only that: each has its own global variables, as [`--instances`](#options-of-quillo-run) do. A service's
  channels work apart too: pages connect to the app's process, which asks the services
  which channels their `channel.on` took, hands what pages send on one to its service
  (in order, a batch at a time, with the page's session), and passes on what the
  service broadcasts, sends or asks of `channel.peers` to its pages; all of it signed
  with `QUILLO_SERVICE_SECRET`, and none of it logged. Each crossing is a call on this
  machine (well under a millisecond): a game service ticking 20 times a second reaches
  its players 20 times a second. With `--scale`, channels go to the service's first copy
  only, so one copy holds a game's world.
- Either way, what crosses is data, as JSON: arguments and results are copied (a
  function arrives as `null`; a decimal stays a decimal, its scale and all), so a
  program behaves the same whichever way it runs. Optional arguments left out are not sent, so the service's own defaults apply. A
  call that cannot be made (the service not answering) is an error value: `?` passes
  it on, `??` replaces it.
- **Live data across services.** A page's `live` loader that gets its data from a service
  follows only what it reads itself: the service's tables and variables are the
  service's, and their changes wake no page of the caller. So such a loader `watch`es a
  topic, and the code that changes the data publishes it: the service itself
  (`publish("account:7")` in its `server fn`), or the caller after the call. A topic a
  service publishes while answering a call reaches the caller's pages in one process and
  split alike: it comes back with the answer, and the caller announces it to its own
  pages, through every service in between. What a service publishes on its own, not
  while answering a call (a call made later being delivered, a scheduled job, a channel
  handler), reaches the service's own pages: split apart, the app's pages see that
  change when they next load the data. `quillo check` warns about a loader that calls
  a service and watches nothing.

  ```quillo
  // accounts.qwe, the service
  server fn deposit(id: int, cents: int) {
      db.exec("update accounts set cents = cents + ? where id = ?", cents, id)
      publish(`account:${id}`)
  }

  // the app
  server fn load_balance(id: int) {
      watch(`account:${id}`)
      accounts.balance(id)
  }
  ```
- `quillo check` checks each call against the service's `server fn`s (their names,
  arguments and types; it may fail, so the result may be an error), the browser cannot
  call a service, and a call needs the `http` effect. Services may use one another. A
  service's name is a name of the program that declares it, so `service notify = ...`
  (or a file `notify.qwe`) hides the builtin `notify` there, with a warning; inside the
  service, a `server fn count` hides `count` the same way.
- **The service's file**, beside the app, is how the app knows what `billing.add` takes.
  As the app loads, it reads only the file's `server fn` headers (and those of the files
  it `use`s), so a mistake in one of its routes or bodies does not stop an app run with
  `--no-check` (`quillo check`, and [`--split`](#options-of-quillo-run), still check the file whole). A service
  that runs elsewhere need not be here at all: with no file and an address
  (`--service-url` or `QUILLO_SERVICE_BILLING`), the app asks the running service for its
  functions as it starts, as a call like any other (signed, each address in turn), and
  takes each one's header and contract from the answer. Only the functions this caller
  may call are given (a `for [...]` keeps the rest). A service that cannot be reached
  then stops the app: `billing.qwe is not here, and service billing did not give its
  functions: ...`. `quillo check` warns instead of failing: `service billing: billing.qwe
  is not here, so its calls are not checked; it must run elsewhere (QUILLO_SERVICE_BILLING
  or --service-url)`, and a call to it still needs the `http` effect.

  ```sh
  # on 10.0.0.5, with billing.qwe
  quillo run billing.qwe --port 8081 --as-service
  # elsewhere, with main.qwe and no billing.qwe
  quillo run main.qwe --service-url billing=http://10.0.0.5:8081
  ```
- Each service's transactions are its own: there is no transaction across services, and
  `db.atomic` spans databases of one program, not calls. A chain of calls commits step
  by step, so give each step a reference the caller makes up and keep what was done,
  so that a request sent again goes on from where it stopped rather than doing it
  twice. To undo the steps before one that fails, see [Sagas](#sagas).
- A service's `server fn`s are for other services: pages cannot call them. A service
  does not answer `/_quillo/rpc/...` (in one process, under its prefix; split, `--split`
  starts it with `--as-service`), only signed calls at `/_quillo/service/...`, whose
  arguments are checked against the parameter types all the way in, as pages' calls are.
- A program with `server fn`s and no routes is served too: a service may be only that.

When a service runs elsewhere, its calls are made to hold up:

- **Several addresses.** `--service-url billing=http://10.0.0.5:8081,http://10.0.0.6:8081`
  (or the same in `QUILLO_SERVICE_BILLING`): the same service on several servers, called
  in turn.
- **Copies that join** the app while it runs: see [Copies that join](#copies-that-join).
- **Ready.** `GET /_quillo/ready` answers 200 when what the program needs is there, 503
  when not, with each part as JSON: its database (`select 1`), each service it declares
  (one in its process asked in turn, down its own services; one elsewhere asked at each
  address, the answer kept two seconds), and checks of its own,
  `ready("ledger", () => len(accounts.problems() ?? ["down"]) == 0)`. Through the app,
  `/billing/_quillo/ready` asks a service served under `/billing`. `/_quillo/health` stays
  the process's own: up or not.
- **Contracts.** Each call to a service elsewhere carries its function's contract as the
  caller's copy of the service declares it (`assess(n: int, size?: int) -> map`); a
  service deployed apart whose own differs refuses the call and says both:
  ``service risk: 409 `assess` here is `assess(n: int, size: int) -> int`; the caller's
  copy of this service says `assess(n: int) -> int` ``.
- **Described** as OpenAPI, for a gateway: see [The program's API
  (OpenAPI)](#the-programs-api-openapi).
- **Tried again.** A call that got no connection, or a server answering 503, is tried at
  the next address, a little later each time (100 ms, doubling, with some jitter). A 502
  or 504, which a proxy gives after the copy behind it may have run the call, sends it to
  the next address only when the function is safe to run twice, as one that uses
  [`idempotent`](#idempotent-requests) (in it, or in a function it calls, as the caller's
  copy of the service's file says) is; any other call gives the error *(since 0.7: every
  call was tried again, and ran on both copies)*. A call sent but not answered in time is tried again at the same address, with
  the same call id: the service remembers the calls it answered (for eleven minutes, up
  to 100,000 of them in memory, and in `_quillo_calls` when it has a database) and gives
  the same answer, so while the service stays up the function runs once however often
  the call is sent. A call still running there is waited for (the service says `503 this
  call is still running`, and the caller asks again). Only an answer of 200 is
  remembered, so a service that refused a call may take it after the next deploy.
- **Across a crash, at least once.** A service stopped in the middle of a function cannot
  say how far it got. The call is refused as still running for eleven minutes, as long as
  a signature lasts, and then runs again, though its first run may have committed. So a
  function that must not do its work twice keeps its own record: a reference the caller
  makes up as a key, or [`idempotent`](#idempotent-requests), which commits a record with
  the work.
- **Left alone when failing.** An address that fails several calls in a row is not
  called for a while (its circuit is open), then tried with one call; the others take its
  calls meanwhile. Each time it fails again it is left alone twice as long, up to
  sixteen times the cooldown. When every address is left alone, a call fails at once:
  an error value saying so.
- **Set where it is declared**, after the file: how long a try may take, how often a
  call is tried again, after how many failures in a row an address is left alone, and
  for how long (twice as long each time after):

  ```quillo
  service billing = "billing.qwe" { timeout: "2s", retries: 2, failures: 5, cooldown: "5s" }
  ```

  Those are the defaults, except `timeout` (10 s). Durations are `"500ms"`, `"2s"`,
  `"1m"` (`ms`, `s` and `m` only), or a number of milliseconds; `timeout` is more than
  0, `retries` 0 to 10, `failures` 1 or more. A duration past a century counts as a
  century; one no duration can hold (past about 1.8e19 s, infinite, NaN or negative) is
  refused as the program loads (`` service work: `timeout` is a duration ``), as `"1.5h"`
  is. A try that runs out says so: `service
  billing: took more than 2 s (its timeout)`. They apply when a service runs elsewhere
  (`--split`, or at an address); in one process a call is a function's, which runs to
  its end.
- **A chain gives up together.** Each call carries the time its caller still waits for
  it, and the calls the service makes in turn wait no longer than that (their own
  `timeout` when it is shorter). So when the first link gives up, the ones below stop
  waiting too instead of finishing work nobody waits for; a call with no time left is
  not made. What a service is already doing runs to its end.
- **One id for a request, everywhere.** Each request has an id: the `x-request-id` it came
  with (a proxy's, a load balancer's), or one made for it. Its answer carries it, the
  JSON log shows it (`"rid"`), the code sees it (`request.id`), and every call to a
  service passes it on, so a request is followed across services. With `--log json`,
  each call to a service is a line of its own: the service, the function, the address,
  the status, how long it took, the tries and the id (a chain that failed altogether
  logs status 0). In a function a service answers with, `request.call` is the call's own
  id, the same for each try of it, and for a call made later the id `.later` gave.
- **On the wire**, for a balancer's rules, a log, or a service written in another
  language: a call is `POST <service>/_quillo/service/<fn>` with the arguments as a
  JSON array, and the headers `x-quillo-service: v=3,kid=<key's name>,t=<unix
  seconds>,sig=<hex>` (`kid` only for a key of the caller's own), `x-quillo-to` (the
  service's name), `x-quillo-call` (the call's id, up to 64 letters and digits),
  `x-quillo-caller`, `x-quillo-deadline` (milliseconds the caller still waits),
  `x-quillo-contract`, `x-quillo-tenant` and `x-request-id`, and `authorization` for a
  token and `x-quillo-forward-<name>` for each header `forward` passes on. The signature
  is HMAC-SHA256 under the key (`QUILLO_SERVICE_SECRET`'s first, or the caller's own) of
  `quillo-service-3`, the time, the service, the function, the tenant, the call id, the
  `kid`, the caller, the deadline and the contract, each on a line, then the body; the
  time must be within five minutes. Version 2 (`t=...,sig=...` over `quillo-service-2`,
  the time, the service, the function, the tenant and the call id) is still taken, with
  the caller's name then not known. The answer is `{"ok": value}` or `{"err": payload}`
  (with `"published": [...]`, the topics to announce), signed with the call's key in
  `x-quillo-answer: sig=<hex>`, HMAC-SHA256 of `quillo-answer-3`, the call id and the
  status, each on a line, then the body. A 200 without it is refused, under the shared
  secret as with a key of the caller's own *(since 0.7: under the secret it was taken)*:
  what answered at the service's address may not be the service. Under `bearer`, OAuth and
  mTLS nothing signs the answer, which is the service's as far as TLS says it is. It is 401 for a call that proves no caller (a
  bad or old signature, another service's, a key or token not taken), 403 for a caller
  `for` or `check` refuses, 404 for no such function, 409 for a contract that differs,
  503 while the call still runs. A server in another language that answers so,
  remembers call ids, and answers `GET /_quillo/ready`, is a service; its caller keeps a
  `.qwe` file with the `server fn` signatures for `quillo check` and the contract, or the
  service answers the call `_contracts` (`POST <service>/_quillo/service/_contracts`,
  signed as any call, no arguments) with `{"ok": [{"header": "server fn add(a: int, b:
  int) -> int = null", "contract": "add(a: int, b: int) -> int"}, ...]}`, as a Quillo
  service does for an app without its file. The routes under a service's prefix are passed on as they come, but not
  Quillo's own addresses there. A copy [joins](#copies-that-join) at `POST
  <app>/_quillo/join`, with `{ "service": "billing", "port": 8081, "leave": false }`
  (and `"url"` under [`--join-as`](#options-of-quillo-run)), signed as a call is to the service `_join` and the
  function `billing` (with its `QUILLO_SERVICE_KEY` as `kid=billing`, or under
  `QUILLO_SERVICE_SECRET`; a nonce in `x-quillo-call`, taken once). The answer is 200 `{
  service, at }`, the address the copy is recorded at, or 400 (no port, an address not
  known, a `url` that does not lead to where the copy joined from), 401 (a bad or old
  signature, no nonce, the shared secret where the app has the service's key, another
  service's key), 403 (the app has neither `QUILLO_SERVICE_SECRET` nor keys, or
  [`--no-join`](#options-of-quillo-run) and [`--allow-join`](#options-of-quillo-run) refuse it), 404 (no such service), 405 (not a POST) or
  409 (a nonce sent before). Only copies call it, so a proxy in front of the app need not
  pass it from outside.
- **Made later.** `billing.later.invoice(name, 1200)` does not wait: the call is kept, its
  id is the result, and it is made in the background, tried every second, then a little
  later each time (up to five minutes), until it gets through. Since the service
  remembers call ids, it runs once while it stays up. Checked as the call itself is
  (`quillo check`).
  - **It goes with the transaction.** The call is kept in the database the work is in,
    the program's or the tenant's (`_quillo_later`: see [Quillo's own
    tables](#quillos-own-tables)), so inside `db.transaction` it commits or rolls back
    with the work, on every database. It outlives a restart, and servers sharing that
    database make each once; a program with no database keeps its calls in
    `quillo-system.db` next to it.
  - **What finishes it.** A value, or an error value the function gave, finishes a call;
    its answer is kept for a day. Any other answer does not: a 401, 404 or 409 from a
    deploy gone wrong, or a 500, is tried again for about a day, and a service that does
    not answer at all is waited for however long it takes. A 400, arguments the service
    refuses, cannot work and is given up on at once. A call given up on stays in
    `_quillo_later` with `tries` -1 and why in `error`, is counted by `--metrics`, and goes
    again with `update _quillo_later set at = 0, tries = 0 where tries < 0`.
  - **Asking after one.** `later.status(id)` *(since 0.7)* gives `{ state: "waiting", tries, error }`,
    `{ state: "done", result }`, `{ state: "failed", error }`, or `null` if nothing is
    known of it: a call done over a day ago, kept in memory and done, or made in a
    transaction not committed yet. `result` is what the function gave, an error value
    if it said no. `error` is an error value, as it can name addresses, so a visitor
    is not shown it.
  - **Who delivers them.** The thread that also finishes `db.atomic`'s work, for each
    program in the process: the app and the services it runs, however the program runs.
    `--no-jobs` does not stop it. It takes 50 calls a go from each database, each second,
    and the next go at once after a full one. A call that has to wait does not hold up
    the next, so order is not kept.

See [services](https://github.com/quillolang/examples/tree/main/services): a shop that bills through a billing service.

#### Authentication between services

A call signed with `QUILLO_SERVICE_SECRET` proves one thing: the caller holds the secret.
Every service holding it can sign as any other, call any function of any service, and name
any tenant, so the most exposed service in a system holds the key to all the others. A
caller can prove more than that, and a service can say whom it takes calls from *(since 0.7)*.

The caller says how it proves who it is, where it declares the service:

```quillo
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY"), as: "shop" } }
```

| `auth` | The call |
|---|---|
| `"quillo"` (the default) | signed with the shared secret |
| `{ key, as }` | signed with a key of the caller's own, which the service knows it by: `as` is the name (the program's service name unless given). A key of `notify`'s cannot sign as `shop` |
| `{ bearer: f }` | `Authorization: Bearer <token>`, the token from `f` (a function, Quillo's or a Rust file's) at each call, or text |
| `{ oauth: { token_url, client_id, client_secret, scope, audience, method } }` | a token fetched by OAuth 2.0's client credentials grant (`method`: `"post"`, the client's secret in the form, or `"basic"`, in `Authorization`), kept until 30 s before it expires, and fetched again once when the service answers 401 |
| `{ mtls: { cert, key, ca } }` | a client certificate on the connection (PEM files; `ca` a file or `"system"`), alone or beside one of the others |

A call that proves itself with a token (`bearer`, `oauth`) is not also signed with the shared
secret, so the service checks the token, not the secret.

```quillo
service billing = "billing.qwe" { auth: { oauth: {
    token_url: env("IDP_TOKEN_URL"), client_id: "shop", client_secret: env("SHOP_CLIENT_SECRET"),
    scope: "billing", audience: "billing",
} }, forward: ["authorization"] }
```

`forward: ["authorization"]` passes the visitor's own headers of the request the call is
made in along with it; the service reads them in `request.forwarded`. A call signed with a key
or the secret signs them too, so they cannot be changed on the way; they are still what the
visitor sent. `quillo check` warns of a `key` or `client_secret`
written in the source, where every copy of the program has it (``service billing: its
`key` is written in the source, ... read it with env("...")``), and of a misspelt option
(`auht`: did you mean `auth`?). A wrong value stops the program as it loads, saying what
is wrong (`` service billing: `oauth` needs `client_id` ``).

The service says what it takes, once, at its top level:

```quillo
// billing.qwe
accept_calls {
    keys: { shop: [env("SHOP_KEY"), env("SHOP_KEY_OLD")], orders: env("ORDERS_KEY") },
    jwt: { issuer: env("IDP"), audience: "billing", jwks: env("IDP") + "/.well-known/jwks.json" },
    client_cert: { header: "x-forwarded-client-cert" },
    check: (caller) => caller.name != "mallory",
}

server fn refund(account: int, cents: int) for ["shop"] {
    // request.caller: { name: "shop", by: "key" }
    { refunded: cents }
}
```

- `keys`: a key for each caller, by name; a list takes any of its keys, while one is
  changed.
- `shared: false`: calls signed with the shared secret are refused (`this service takes
  no calls signed with the shared secret`). Without it they are taken as before.
- `jwt: { issuer, audience, jwks, secret }`: a bearer token, an OAuth 2.0 access token,
  checked against the identity provider's keys at `jwks` (a URL, kept for ten minutes,
  fetched again at most once a minute for a key it does not have: RS256, ES256, EdDSA)
  or against `secret` (HS256). One fetch at a time, for three seconds at most; one that
  fails is not tried again for a minute, the keys fetched before are used meanwhile, and
  the URL and what went wrong go to the log, while the caller is told only `the identity
  provider's keys could not be fetched` *(since 0.7)*. `audience` is required, so a token meant for another
  service is not taken here; `exp` is required too, and `nbf`, `iss` and `aud` are
  checked, with a minute's leeway. A token that fails is a 401 that says why (`the
  bearer token: the token is not for billing (aud)`, `... has expired`, `... has no exp:
  it would be good for ever`, `an unsigned token`).
- `client_cert: { header }`: the identity in the header a proxy that ended mutual TLS
  sets, as Envoy's `x-forwarded-client-cert` (`Subject="CN=shop,O=Acme";URI=spiffe://...`):
  the SPIFFE id's last part, else the subject's CN. Quillo ends no TLS itself, so the
  header is believed only from an address the option [`--trust-proxy`](#options-of-quillo-run) names
  (`quillo run billing.qwe --as-service --trust-proxy 10.0.0.2`); from anyone else it is
  a 401.
- `check: (caller) => ...`, run with `request.caller` before the call's arguments are
  read: `false` refuses it with 403 (and an error value, with its text), a map takes the
  caller's place, and `reply` answers the call there.
- An `accept_calls` with nothing in it is an error, and so is an option it does not have.

`server fn refund(...) for ["shop"]` takes calls from those callers only, checked before
the arguments are read: another gets 403 (`` `refund` takes calls from shop only; this is
`notify` ``). Pages cannot call it at `/_quillo/rpc/refund`, which answers 404, and their
script has no `server.refund` *(since 0.7: anyone could)*. `quillo check` refuses `for []`, `for [shop]` (a caller is named in plain
text, `"shop"`) and `for "shop"`, and `quillo docs` puts each function's callers in
`x-quillo-callers`, for a gateway to enforce too.

What the function sees, where a service runs elsewhere (in one process a call is a
function's call: `request.caller` is null, and `for` is not checked):

- `request.caller`: `{ name, by }`, `by` being `"quillo"` (the shared secret; `name` as
  the caller signed it), `"key"`, `"jwt"` (with `scopes`, from `scope` or `scp`, and the
  token's `claims`; `name` its `client_id`, `azp` or `sub`) or `"mtls"` (with `subject`,
  and `uri` for a SPIFFE id). What `check` gave, when it gave a map.
- `request.user`: the claims of the visitor's token, when the caller forwarded its
  `authorization` and `accept_calls`'s `jwt` takes it; else null.
- `request.forwarded`: the headers forwarded, by name.

A call proven several ways is taken by the first that holds, in this order: a key of the
caller's own, the shared secret, a token, a certificate. A caller with `QUILLO_SERVICE_SECRET`
set signs every call with it, a bearer one too, so a service that is to check tokens says
`shared: false`.

Answers are signed back with the key the call was signed with, and a caller that signs
with a key of its own takes no answer without it (``service billing: the answer from
http://... is not signed by the service``), so a copy that only says it is the service
cannot answer for it. Signatures are now version 3, which also covers the caller's name,
the deadline and the contract; services still take version 2, so upgrade the services
before their callers.

The shared secret is changed without stopping anything: `QUILLO_SERVICE_SECRET=new,old`
(or `QUILLO_SERVICE_SECRET=new` and `QUILLO_SERVICE_SECRET_PREVIOUS=old`) signs with the
first and takes any. Set both on every machine, then drop the old one. `keys` take a list
for the same.

For a caller that is not a Quillo service (a gateway, a program in another language with
an OAuth token), `accept_calls { jwt }` takes its token at `/_quillo/service/<fn>` with
`x-quillo-to` and `x-quillo-call`. Or the service offers routes, a JSON API, checked as
middleware with the catalog's `jwt.qwer` (2.0.0: `jwt_check(token, secret, { audience,
issuer, require })`, `exp` required unless told otherwise, and `jwt_verify_jwks` for an
identity provider's keys) or `oidc.qwer` (1.1.0: `oidc_client_credentials` fetches a
token, `oidc_verify_access` checks one), and called with `http.*`. That gives up what
`service` brings: `quillo check` of each call, contracts, retries run once, deadlines,
`.later` and published topics.

```quillo
use "jwt.qwer"

route POST /api/refund {
    let token = replace(request.headers.authorization ?? "", "Bearer ", "")
    let claims = jwt_check(token, env("JWT_KEY"), { audience: "orders", issuer: "idp" })
    if is_err(claims) { reply 401 { error: error(claims) } }
    { refunded: request.json.cents, for: claims.sub }
}
```

#### Copies that join

`quillo run billing.qwe --as-service --port 8081 --join http://app:8080` starts one more
copy of `billing` that joins the app while it runs: it says so, signed, every 5 s, and that
it is leaving when stopped (the app drops one it has not heard from in 15 s). Its address
is the one it connects from, with its [`--port`](#options-of-quillo-run), called over `http://`; `--join-as
https://billing-3.internal:8443` gives the address it is called at instead, which must
lead to where it joined from.

- A copy signs with `QUILLO_SERVICE_KEY`, billing's own key, when it is set (`kid` is
  its service's name), else with `QUILLO_SERVICE_SECRET`. An app whose `accept_calls`
  has a key for `billing` takes copies of `billing` only so: the shared secret is not
  enough (``this app takes copies of `billing` signed with its key (QUILLO_SERVICE_KEY on
  the copy)``), and a key of another service's never is. Without such a key, the secret
  is, as before: anyone with it can join as any service the app declares, and take its
  calls.
- [`--no-join`](#options-of-quillo-run) takes no copies, and [`--allow-join billing,risk`](#options-of-quillo-run) only those (403 to the
  others).

- When `billing` runs in the app's process, the copies that joined take every call, in
  turn, and the app runs a call itself only when no copy answers it: a monolith takes on
  copies as it gets busy, without a line of either changed.
- When the app was given addresses ([`--service-url`](#options-of-quillo-run), `QUILLO_SERVICE_<NAME>`), the given
  addresses and the joined copies are all taken in turn.
- Each copy has a circuit of its own. With several app servers, a copy joins each.
- Join the app's own address, not its public one: through a proxy the app sees the
  proxy's address, unless it runs with [`--trust-proxy`](#options-of-quillo-run) naming it. A copy behind NAT is
  recorded at an address the app cannot reach, so put such a copy behind a balancer
  instead: limer's `discover` pool with [`--register`](#options-of-quillo-run), keeping a call on one copy
  (`lb_policy hash header x-quillo-call`) or giving the copies a database to share, so a
  call sent again is not run again.

Services are found by registration: a copy joins the app ([`--join`](#options-of-quillo-run)), and any server joins
limer's pool (`--register`). A caller does not look a service up by name, so a service the
app does not run itself needs `--service-url` or `QUILLO_SERVICE_<NAME>`, and
`QUILLO_SERVICE_SECRET` is set on every machine by hand.

#### The program's API (OpenAPI)

`quillo docs app.qwe` writes the program's API as OpenAPI 3, from its source (`quillo docs
--mcp app.qwe` prints what AI clients are told instead, see [Tools for AI
clients](#tools-for-ai-clients)): each
route's path, query and payload patterns, and each `server fn`'s contract
(`x-quillo-server-fns`, with `x-quillo-callers` for one declared `for [...]`), with the routes of the services it serves under their prefixes.
Started with the option [`--describe`](#options-of-quillo-run), `quillo run app.qwe --describe`, the server serves
the same at `GET /_quillo/describe`, for a gateway to import. A prefixed service's `server fn`s are not in the app's document: they are called by
other services, signed, not at `/billing/_quillo/rpc`. Under [`--split`](#options-of-quillo-run) the app's document
still holds the services' routes, read from their source, and `--describe` is passed down,
so `/billing/_quillo/describe` answers the service's own document, its paths without the
prefix.

What the document holds:

- Its title is the program file's name (`main`), both ways. Under [`--base /shop`](#options-of-quillo-run) it has
  `"servers": [{ "url": "/shop" }]`.
- Several routes on one method and path shape are one operation: their bodies a `oneOf`
  (each literal field an `enum`), their query parameters merged (required only where every
  route requires them), the path template named as the first route names it (`/users/{id}`,
  its parameter a `oneOf` of the routes' types), and `x-quillo-at` a list of the places
  when there are several.
- A rest parameter (`/files/*path`) is `/files/{path}`, with `"x-quillo-rest": true` and
  the description "the rest of the path, slashes included": an OpenAPI template matches one
  segment, so a gateway routes the rest itself.
- Each operation has one response, 200, with no content schema.
- A named type (`type Note = { ... }`, or an alias such as `type Id = int | str`) is
  written once under `components/schemas` and used with a local `$ref`; a mount's or a
  service's is named `shop.Item`, with its own name as `title`. Only the types the document
  uses are there. A record's fields carry `required` (those without `?` and without a
  default, which a call is refused without), a union of texts is an `enum`, and a written
  default is its `default`. A union that one value could fit twice (two records,
  `int | float`) is an `anyOf`, others a `oneOf`. A nullable or defaulted `$ref` is wrapped in
  `allOf`, since OpenAPI 3.0 ignores what stands beside a `$ref` (FEEDBACK56.md, Q-855).
- A map type's fields are `required` the same way, in what a call sends. A result's or an
  error's map type lists none, since the server does not check those fields of what a function
  gives back; a record does, as it is checked (FEEDBACK56.md, Q-856).
- A mount's or a service's routes and `server fn`s include those of a file it shares with the
  program (a `use`d models file), as the running server has them.
- A `server fn`'s entry has its `///` as `description`, and says what it may do: its
  `contract` ends with its `with` (`add_note(text: str) -> list ! str with db`) and
  `x-quillo-effects` lists the same, in Quillo's order. Services compare contracts without the
  `with`, so one built against an older document still matches (FEEDBACK56.md, Q-854, Q-857).

`/_quillo/describe`, when on, is public: it lists every route, those no page links to as
well, each `server fn`'s signature, the file and line of every route, and the version of
Quillo. Keep it private as `/_quillo/metrics` is, with a rule at the proxy, or do not turn
it on in production.

#### Calling another Quillo site

*(since 0.7)* `site shop = "https://shop.example.com"` names a deployed Quillo site, and `shop.notes(a,
b)` calls its `server fn notes` as the site's own pages do: `POST
<url>/_quillo/rpc/notes` with the arguments as a JSON array. It gives the answer's value,
the site's error as an error value, or an error value when the site cannot be reached.
Decimals and big ints cross as they do for pages. The address is any expression:

```quillo
// shop.qwe, deployed at https://shop.example.com
server fn notes(n: int) -> list = range(1, n + 1) | map((i) => { id: i, text: `note ${i}` })

// main.qwe, elsewhere
site shop = env("SHOP_URL", "https://shop.example.com")

server fn my_notes() = shop.notes(10)

route GET / with http = <ul>{for n in my_notes() ?? [] => <li>{n.text}</li>}</ul>
```

`SHOP_URL=http://127.0.0.1:8081 quillo run main.qwe` with the shop on that port lists
`note 1` to `note 10`; with the shop down, `my_notes()` is an error value and the list is
empty.

- In an app of one person (a phone app, `quillo app`, or with `QUILLO_SITE_COOKIES=1`),
  the cookies the site sets are kept in `.quillo/sites/<name>.json` in the program's
  folder, so whoever signed in stays signed in. On a server they are not kept (a log line
  says so, once), as one visitor's session would be every visitor's.
- It runs on the server side only: `quillo check` refuses `shop.x()` in a component
  ("call it in a server fn"). `site` stays an ordinary name elsewhere (`let site = ...`).
- A call is a request to the site: it needs the `http` effect, and `quillo check` says so
  of a function `with time` that calls `shop.notes` (`` `shop.notes` needs the http
  effect ``). Only what a `site` declaration made is a site *(since 0.7)*: a map with the
  same keys, from JSON or a script, is a map, so a visitor cannot point a call elsewhere.
  The name is ASCII letters, digits and `_`.
- `/_quillo/rpc/<fn>` is a stable wire for programs: a JSON array of arguments in,
  `{"ok": value}` or `{"err": error}` out. A program needs no `Origin` header; another
  site's `Origin` is refused.
- A phone app runs its services inside the app. To use a server's, call the server with
  `site`, not `service`.

#### Sagas

A saga is a business operation spread over services that share no transaction. Each
service commits its own part, and if a later part cannot happen, the earlier parts are
undone by compensating steps you write: an order reserves stock, charges a card and books
a parcel, and if the parcel cannot be booked, the charge is refunded and the stock
released.

Quillo has no `saga` word. One is a table, a job that moves each row a step at a time, and
ordinary calls, and it needs nothing else. The state is written before anything else
happens, so a server that stops goes on from there:

```quillo
db.open("orders.db")
db.exec("create table if not exists sagas (id text primary key, cents int not null, state text not null, step int not null, tries int not null, next_at bigint not null, why text)")

service inventory = "inventory.qwe" { timeout: "1s", retries: 1 }
service payments = "payments.qwe" { timeout: "1s", retries: 1 }

let steps = 2

fn forward(s) = match s.step {
    0 => inventory.reserve(s.id)
    _ => payments.charge(s.id, s.cents)
}

fn undo(s) = match s.step {
    0 => inventory.release(s.id)
    _ => payments.refund(s.id)
}

fn move(s, state, step, why) -> map =
    db.exec("update sagas set state = ?, step = ?, tries = 0, next_at = 0, why = ? where id = ? and state = ? and step = ?", state, step, why, s.id, s.state, s.step)

fn try_again(s, why) -> map =
    db.exec("update sagas set tries = ?, next_at = ?, why = ? where id = ?", s.tries + 1, now() + min([1000 * pow(2, s.tries + 1), 60000]), why, s.id)

fn advance(s) {
    if s.state == "running" {
        let r = forward(s)
        if is_err(r) {
            // No answer: it may have run. Try again, then undo this step too.
            if s.tries + 1 >= 3 {
                move(s, "undoing", s.step, str(r))
            } else {
                try_again(s, str(r))
            }
        } else if !r.ok {
            move(s, "undoing", s.step - 1, r.why)
        } else if s.step + 1 == steps {
            move(s, "done", s.step, null)
        } else {
            move(s, "running", s.step + 1, null)
        }
    } else if s.step < 0 {
        move(s, "undone", s.step, s.why)
    } else {
        let r = undo(s)
        if is_err(r) {
            // An undo is never given up on: it waits for the service.
            try_again(s, str(r))
        } else {
            move(s, "undoing", s.step - 1, s.why)
        }
    }
}

fn advance_all() {
    for s in db.query("select * from sagas where state in ('running', 'undoing') and next_at <= ? order by id limit 100", now()) {
        advance(s)
    }
}

schedule("1s", advance_all)
```

And a step, the same in every service:

```quillo
server fn charge(ref: str, cents: int) -> map {
    db.transaction(() => {
        let was = db.one("select state from charges where ref = ?", ref)
        if was != null {
            return { ok: was.state == "charged", why: was.state }
        }
        if cents > 100000 {
            return { ok: false, why: "declined" }
        }
        db.exec("insert into charges (ref, cents, state) values (?, ?, 'charged')", ref, cents)
        { ok: true }
    })
}

server fn refund(ref: str) -> map {
    db.exec("insert into charges (ref, cents, state) values (?, 0, 'refunded') on conflict (ref) do update set state = 'refunded'", ref)
    { ok: true }
}
```

What starts a saga is a row: the order's route writes it and answers at once, and the
job does the rest. The visitor follows it on a page that reads the row:

```quillo
route GET /order =
    <form method="post" action="/order">
        <input type="number" name="cents" value="2500"/>
        <button>Order</button>
    </form>

route POST /order { form: { cents: int } } with db, random {
    let id = uuid()
    db.exec("insert into sagas (id, cents, state, step, tries, next_at) values (?, ?, 'running', 0, 0, 0)", id, cents)
    redirect(`/order/${id}`)
}

route GET /order/:id with db {
    let s = db.one("select state, why from sagas where id = ?", id) ?? reply 404 "No such order"
    match s.state {
        "done" => <p>Your order is placed.</p>
        "undone" => <p>Your order could not be placed ({s.why}); nothing was charged.</p>
        _ => <p>Placing your order… <a href={`/order/${id}`}>See how it is going</a></p>
    }
}
```

An order of 2,500 shows "Placing your order…", then, a couple of seconds later, "Your
order is placed." One of 200,000 is declined at the charge: the stock is released, and
the page says `Your order could not be placed (declined); nothing was charged.`

Five rules carry it, and leaving out any one breaks it:

1. **The reference is the key.** The caller makes it up, and a step keyed on it changes
   nothing the second time. A job's tries are new calls with new ids, so `request.call`
   and `idempotent` do not do this job here, though they do for one call sent again.
2. **A "no" is an answer, not an error.** A step gives `{ ok: false, why }`. An error value
   means no answer came and the step may have run, which is a different decision.
3. **An undo works on what never happened, and leaves a mark.** A refund with no charge
   writes `refunded`, so a charge that arrives late is refused. A call that times out
   keeps running at the service, so this order really happens.
4. **A step with no answer is undone too.** It may have run, and rule 3 makes that safe.
5. **An undo never gives up.** It waits for the service. If it can never work, a person has
   to look, so a saga long in `undoing` is what to alert on.

Put the step that cannot be undone last. And sagas do not isolate each other: between
steps, other requests see the stock reserved and the charge taken, so a state such as
`held` must mean something to them.

`.later` can carry a step too, committing the move with the call: `later.status(id)` tells
how it ended, and a `server fn` with `idempotent` is safe across a crash. A job as above
stays the simpler choice when the next move depends on the answer.

## Standard library

- **General:** `print str int float dec len type_of`
- **Strings:** `upper lower trim split join contains starts_with ends_with replace slice repeat index_of normalize`
  (`upper` and `lower` know all of Unicode: `"straße"` is `"STRASSE"`; `trim` takes off
  Unicode's White_Space (U+0085 too, not U+FEFF); `replace` replaces every occurrence of
  plain text, not a pattern, and an empty pattern puts the text before each character and
  at the end (`replace("abc", "", "-")` is `-a-b-c-`); text orders by code point; each the
  same in the browser; regular expressions are `re`,
  below). `normalize(s, form)` *(since 0.7)* gives one Unicode form of the text, `"NFC"` unless
  `"NFD"`, `"NFKC"` or `"NFKD"` is given, on both sides: an `é` typed as one character and
  one typed as `e` and an accent are two texts until normalized. Compare emails and user
  names normalized, and user names in `"NFKC"`, which also makes `ﬁ` `fi` and a
  full-width `Ａ` an `A`: `lower(normalize(trim(name), "NFKC"))`.
- **Regular expressions:** `re.test(text, pattern)` (true or false), `re.find` (the first
  match, or null), `re.find_all` (every match), `re.replace(text, pattern, with)` (every
  match; `with` is text where `$1`, `$<name>`, `$0` and `$$` stand for the groups, the
  match and `$`, or a function given the match) and `re.split`. A match is
  `{ text, start, end, groups, named }`, positions counting characters as `slice` does.
  Flags come last: `"i"` (any case), `"m"` (`^` and `$` at each line, a line ending at `\n` or `\r`, `\r\n` as one end, on both sides) and `"s"` (`.`
  matches line breaks too). `re.valid(p, flags)` *(since 0.7)* asks whether a pattern can be run, for
  one from data. They give the same answers on the server and in the browser, but for
  groups in a loop (`(?:(a)|b)*` on `ab`, where the server keeps the group's last match and
  the browser clears `(a)` on each round: the two engines cannot be made the same, and
  `quillo check` warns of a capturing group inside a repeated one; capture the whole
  repetition, `((?:a|b)*)`), and not at the same cost: the server's engine takes the same time on any text, and the
  browser's backtracks, so `(a+)+$` can hang a tab with no way to stop it, which `quillo
  check` warns of. Bound the counts (`{1,8}`), or run a visitor's own pattern in a `server
  fn`. So a pattern takes only what both read alike: characters, `.`, `[...]`,
  `\d \w \s \b` (ASCII digits and word characters, as in JavaScript) and their capitals,
  `^ $`, `(...)`, `(?:...)`, `(?<name>...)`, `|`, and `* + ? {n} {n,} {n,m}` (lazy with
  `?`). Lookaround, backreferences and `\p{...}` are errors, found by `quillo check`
  when the pattern is written in the call, and so is escaping a character that needs
  none (`\\,`), a flag given twice, and a count over 1,000 (`{1001}`). In a string, a
  backslash is written twice: `"\\d+"`.
- **Translations:** `t(key, vars)`, `locale()`, `translations`, `locale_from` and
  `with_locale` (see [Translations](#translations)).
- **Markdown:** `markdown(text)` gives HTML (CommonMark with tables, strikethrough and task
  lists). HTML inside the text is shown as text and `javascript:` links are blocked, so
  it is safe for what visitors write; `markdown(text, { html: true })` keeps the HTML,
  for text you wrote, and `markdown(text, { inline: true })` keeps only what is inside a
  line (bold, italic, code, links) for a headline or a caption: no paragraph around it, and
  no headings or lists (their lines, broken by `<br>`). It gives an `html` value, shown
  as it is in markup; `str(markdown(text))` is its text, to store. In a component the
  server makes it (a request to `/_quillo/markdown`, so the page shows the same HTML,
  highlighting included), awaited like a `server fn`: call it in an event handler or a
  `state` initializer, and keep the text in state:
  `oninput={(e) => { preview = str(markdown(e.value)) }}` and `<div>{raw(preview)}</div>`.
  `/_quillo/markdown` takes at most 1 MB of text (a larger one is answered 413, and
  `markdown` in the component gives an error value), and one that computes past
  [`--max-compute`](#options-of-quillo-run) is a 500.
  `markdown_page(text, { title, lang, html })` is the same as a whole page, for a route:
  a doctype, a `<head>` with its `<title>` (the option's, else the text's first level-one
  heading, `# Title` or a line underlined with `=`, as the text it shows, without its
  marks; a `#` line in a code block or in HTML is no heading), and the HTML in its `<body>`. Markdown takes time in proportion to the text,
  whatever marks it holds (a megabyte of `*a_` is 0.03 s), and `markdown`, `markdown_page`
  and `highlight` end at `--max-compute` as a loop does. A fence's language, for
  highlighting, is its first word, to a space or a comma (```` ```rust,ignore ```` is Rust).
- **Lists:** `map filter find any all reduce sum min max sort sort_by reverse first last push range enumerate slice unique flatten zip take drop count group_by join_on index_of`
- **Maps:** `keys values get has set remove merge` (`get(m, k, default)`; `merge` takes
  two maps; there is no `entries`: `for [k, v] in m`). A map keeps the order its keys
  were set, number-like keys too, in the browser as on the server.
- **Math:** `abs round floor ceil sqrt pow`, and the three modules under
  [Numbers and data](#numbers-and-data). For money, [Decimals](#decimals) and
  [Money](#money); `format_number(x, locale, digits)` writes a number as a locale does.
- **Records:** `type User = { ... }` makes `User(fields)`, which builds and checks one
  (see [Records](#records)).
- **Errors:** `err is_err unwrap`
- **Web:** `raw escape url_encode local_path env now file files markdown_page redirect respond json.parse json.stringify
  cookie set_cookie delete_cookie session set_session end_session site_headers`
  (`local_path` under [Route groups](#route-groups), `site_headers` under [What a route
  returns](#what-a-route-returns)). `url_encode("a b")` is
  `a%20b`, and `url_encode({ q, page: 2 })` is a query string (`q=...&page=2`, leaving
  out keys whose value is null); in the browser too. `env(name, default)` gives the
  default when the variable is not set.
- **Program and pages:** `highlight(code, lang)` (Quillo, JavaScript and TypeScript,
  Python, Rust, shell, SQL, CSS, HTML and XML, JSON; anything else as escaped text, no
  word coloured), the same on the server and in the browser, where `str(highlight(...))`
  is the markup too. It reads each line once, so it takes time in proportion to the text,
  and colours strings as each language reads them: Python's triple-quoted and Rust's
  strings run over lines, Rust's raw strings `r#"..."#` have no escapes, a Rust
  character such as `'"'` is a string and a lifetime `'a` is not, a JavaScript regex
  literal is one token, a shell's `it's` or a `#` inside a word starts nothing, and a
  range's `..` ends the number before it (`0..10` is two numbers); a word is Unicode
  letters and digits on both sides.
  The browser knows only its own languages: `"constructor"` and `"__proto__"` are escaped
  text,
  `check_program`, `run_script`, `delete_file`, `save_upload`, `read_upload`,
  `schedule`, `idempotent`, `parallel`, `ready`, `style_with`, `classes`, `accept_calls`, `allow_cross_site`,
  `tenant`, `tenant_from`, `with_tenant`, `platform` (below), and in components `url`, `navigate`, `every`,
  `ask`, `notify`, `js`, `js_new`, `live`, `waiting`, `publish`, `watch`, `unwatched`: each is
  described where it is used.
- **Where the program runs:** `platform()` *(since 0.7)* gives a map: `os` (`"ios"`, `"android"`,
  `"macos"`, `"linux"`, `"windows"`), `os_name` and `os_version` (`"iOS"`, `"27.0"`), `kernel`,
  `device` (`"phone"`, `"tablet"`, `"desktop"` in `quillo app`, `"server"`), `model` (the
  hardware's identifier: `"iPhone19,2"`, `"Mac17,2"`), `arch`, `cpus`, `simulator`, `runtime`
  (`"mobile"`, `"app"` or `"server"`), `locale` (the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is set and not empty; null for `C` and `POSIX`), `time_zone`, `quillo` (this build's version), and in a
  phone app also `model_family` (`"iPhone"`), `memory` (bytes), `screen` (`{ width, height, scale }`, in
  points) and `app` (`{ id, name, version, build }`). What a system does not tell is `null`. It is read
  once: the screen and locale are as they were when the program started. It runs on the server, as
  `env` does: in a component, call it from a `server fn`. `server` is where the server listens,
  `{ host, port, url, base }`: the port it got when started with [`--port 0`](#options-of-quillo-run)
  (`quillo run main.qwe --port 0`), and a `url` reachable from this
  machine (`0.0.0.0` is written `127.0.0.1`), so a program that reads its own pages need not be
  told its address again. It is null in a script and while the program loads.

A builtin given more arguments than it takes stops and says what it takes: `` `split`
takes 2 arguments, got 3: split(s, sep) `` (and for `sort` given a comparator, to use
`sort_by`), in `quillo check` where the call is written out, as the program runs however
it is called (a method on an untyped value, `line.split(",", 2)`; a builtin held in a
variable or passed to another, `reduce(xs, 0, max)`), and in the browser. Nothing is
ignored: `uuid(1)`, `random(1)` and `merge(a, b, c)` are errors.

What each does at its edges, where the answer is not what another language would give:

| Call | Gives |
|---|---|
| `sort(xs)`, `sort_by(xs, f)` | ascending and stable; `f` may give a list to sort by several keys, `sort_by(rows, (r) => [r.a, r.b])`, as lists compare item by item, and so do `<`, `>`, `<=` and `>=` (`[1, 2] < [1, 3]`); no comparator and no descending flag (`reverse(sort(xs))`, or `vec.argsort(xs, true)`). Numbers (ints, floats and decimals together), strings (by code point: `"B"` before `"a"`) and lists of those order; `null`, booleans, maps, a string with a number, a decimal with a float, or NaN are an error, in the browser too (`vec.argsort` puts NaN last), and comparing `null` says that a miss of `index_of`, `find` or `get` is `null`, not -1 |
| `split(s, sep = " ")` | at each single `sep` (`"a  b"` is `["a", "", "b"]`); `split(s, "")` is the characters; no limit |
| `join(xs, sep = "")` | the items as text, with `sep` between |
| `filled(x)` | `x`, or `null` for `""`, so `filled(name) ?? "anonymous"` replaces a blank or missing name; `0`, `false`, `[]` and `" "` stay as they are (`filled(trim(x))` counts spaces as blank) |
| `slice(x, start, end)` | negative positions from the end; both kept within the list or string; `end` before `start` gives an empty one |
| `take(x, n)`, `drop(x, n)` | a negative `n` counts as 0, unlike `slice`: `take(xs, -2)` is empty, `drop(xs, -1)` all of it, so a count worked out by subtraction stays safe |
| `range(n)`, `range(a, b)`, `a..b`, `range(a, b, step)` | `b` left out, step 1, empty when `b <= a`; a negative step counts down; a list of at most ten million, while a `for` whose list no one keeps counts without one |
| `group_by(xs, f)` | a map from each key, as text, to the items with it, in order: `1` and `"1"` are one key; an error `f` gives is given back, not made a key |
| `unique(xs)` | the first of equal items, keeping exactly what `==` keeps (`unique([1, 1.0, 1m, "1"])` is `[1, "1"]`), in time linear in the list on both sides: 40,000 items take milliseconds, not seconds |
| `zip(a, b)` | pairs, as long as the shorter |
| `contains(x, y)` | in a string, `y` as text; in a list, an equal item; in a map, a key |
| `remove(list, x)` | every item equal to `x` gone; `remove(map, k)` the key |
| `set(xs, i, v)` | `i` past the end is an error |
| `round(x)` | a float rounds half away from zero (`round(-2.5)` is `-3`) to an int, up to 2^63 (`type_of(round(1e16))` is `int`; past it, and NaN and the infinities, the float), and an int is itself (exact past 2^53); `round(x, 2)` a float with at most 15 places, from its shortest decimal (`round(1.005, 2)` is `1.01`, as `format_number` writes it); fewer than no places round to tens, hundreds, ...: `round(1234.5678, -2)` is `1200.0`, from the float's shortest digits and exact at every place (`round(1e30, -29)` is `1e30`), and a result past the largest float is an error; an int to 0 places or more is itself (`round(5, 2)` is `5`, as in Python), and to fewer the nearest multiple, exactly, a half away from zero (`round(1250, -2)` is `1300`, where Python's is `1200`, half to even); `round(a / b)` of two ints is exact (see `/` under [Values](#values)); a decimal takes a mode (see [Decimals](#decimals)) |
| `int(x)` | a float or decimal cut toward zero (an error value when too large for an int); `true` is 1; text of digits only (`int("1.0")`, `int("1e3")` are error values) |
| `float(x)` | a number as a float; text as Rust reads a float (`"2.5"`, `" 7 "`, `"1e3"`, `".5"`, `"inf"`, `"nan"`), so `"0x10"`, `"0b11"`, `"1_000"` and `""` are error values, in the browser too |
| `floor(x)`, `ceil(x)` | a float to an int, up to 2^63 (`type_of(floor(1e16))` is `int`; past it the float); an int as it is, past 2^53 too, as `round(x)` gives it; `floor(a / b)` of two ints divides in ints, exactly (see `/` under [Values](#values)) |
| `str(x)` | a whole float as `2.0` (in a component's text, `2`, on both sides), never in exponent form (`1e16` is `10000000000000000`); a float's shortest digits, a tie taking the even digit as JavaScript does (`1916218016406818.3`, which is held as ...818.25, is written `1916218016406818.2`); markup as its HTML, in the browser too; a list or map as Quillo writes it (`{ a: 1, b: "x" }`, a key of letters, digits and `_` of any script, not starting with a digit, bare (`{ é: 1, 日本: 2 }`), another quoted (`{ "a b": 3, "1a": 4 }`), on both sides); `null` as `null` |
| `type_of(x)` | one of `null bool int float dec str list map fn html response error` (`promise` for a JavaScript Promise in the browser) |
| `len(x)` | a string's characters, a list's items, a map's keys; an `html` value's bytes |
| `min([])`, `max([])`, `first([])` | `null`; `sum([])` is `0`, and a sum of decimals stays exact; `sum` adds numbers only, and text stops it at its item (`` `sum` adds numbers; got the str "12.50" at item 1: use `float(...)` or `dec(...)` first ``), where it was joined after a `0` |
| `sqrt(-1)` | `NaN`, while `math.sqrt(-1)` is an error, as in Python |
| `pow(2, 10)` | an int, exact, or an overflow as `*` gives (`pow(2, 63)`, and `pow(2, 4294967296)`: a power of 2^32 or more is exact for a base of -1, 0 or 1, and an overflow for any other); a float on either side, or a negative power, gives a float; a decimal to an int's power, a decimal (see [Decimals](#decimals)) |
| `print(a, b)` | the values as text, a space between, on stdout (the console in a page) |
| `now()` | milliseconds since 1970, an int |
| `sleep(ms)` | no maximum; other requests run meanwhile, and a request's sleep of 5 ms or more holds no thread (a timer wakes it), so a hundred streams sleeping in their loops hold up nothing; in a scheduled job or a tick, ends it (status 503) when the server stops or the program is loaded again, so a loop that runs for ever stops; in a stream (`stream`, `events`), a stop ends it as its next `send` would, as a client gone (not in a transaction, which finishes); a request waits it out (it is answered at a stop, however long it takes unless [`--drain`](#options-of-quillo-run) gives a time, and finishes on its version at a reload) |
| `random_int(lo, hi)` | both ends included |
| `uuid()` | a version 4 UUID |
| `read_file(path)` | text: bytes that are not UTF-8 are replaced; a missing file is an error value |
| `write_file(path, v)` | `v` as text, folders made, the file replaced; `null` or an error value |
| `escape(s)` | `& < > " '` as entities |
| `json.parse(s)` | an error value for bad JSON, for a number past a float (`1e400`) and for half of a character (a lone `\ud83d`), on both sides; an int past 64 bits becomes a float; a key given twice keeps the last value; keys in the order written, on both sides; at most 128 levels deep, on both sides, past which it is `invalid JSON: it nests more than 128 deep` (`request.json` and an `http` answer's `json` are `null`, a stream's line is its text, and a database's JSON cell is that error); every other reader of JSON (sessions, channels, a `server fn`'s arguments, a service's body) takes the same depth |
| `json.stringify(v, pretty)` | indented by two spaces when `pretty` is true; a whole float as `2.0` (`-0.0` too), so it reads back as a float, and an exponent where JavaScript writes one (`1e+21`, `1e-7`), on both sides; markup as its HTML; a decimal as a string, NaN and infinities as `null`, an error value as `{"error": ...}`; a function is an error value naming where it is (`` json.stringify: `a.f` is a function, which cannot be written as JSON ``), where it was written as text; JSON past 256 MiB, or nested more than 10,000 deep, is an error value, counted before it is made, as lists that hold one part twice can double at each step *(since 0.7)*; a route's answer the same, a 500 with the reason in the log |

What is not in the standard library, and what to do instead:

- **Bytes, base64, hex.** There is no bytes value: files, request bodies and HTTP
  answers are read as text (bytes that are not UTF-8 replaced), and only `file()`,
  `save_upload` and `image.convert` move bytes as they are. Nothing decodes base64 or
  hex (`crypto.token` and the hashes only make them). Binary work goes in Rust (see
  [Rust](#rust)).
- **Formatting text.** No `sprintf` or padding: template strings, `repeat`, `slice`,
  `format_number`, `money.format` and `date.format`.
- **CSV, XML, YAML.** No parsers built in: `split` and `re` for simple CSV, or the
  catalog's plugins `csv`, `xml`, `yaml`, `toml`, `gzip`, `tar` and `bignum` (`quillo
  install csv`, see [Plugins](#plugins-from-the-catalog)). JSON, Markdown and regular expressions are
  built in.
- **`url_decode`.** Not there: query strings, forms and cookies arrive decoded.
- **A seed for `random`.** None: runs are not repeatable.
- **Other processes.** A program cannot run a command: `env`, files, and a command started
  beside it with the option [`--with`](#options-of-quillo-run) (`quillo run main.qwe --with "npx tailwindcss ..."`). A script reads its arguments with `args()` and its input with `stdin()` and
  `read_line()` (see [Scripts as tools](#scripts-as-tools)).
- **Hashing a value.** `crypto.sha256(json.stringify(v))`. Values never change, so
  nothing needs copying.
- **Building a long string.** `+` makes a new string each time: push the pieces to a
  list and `join` it.
- **Streaming:** `stream(f)` and `events(f)` answer as they go, `http.stream` reads
  another server's answer as it arrives, and `listen` and `receive` read streams in a
  page (see [Streaming](#streaming)).
- **Database:** `db.open db.exec db.query db.one db.transaction db.migrate db.connect
  db.tenants db.tenant_exists db.shards db.atomic db.system` (see [Database](#database)).
- **Crypto:** `crypto.sha256(text)` and `crypto.hmac(key, text)` (HMAC-SHA256, hex),
  `crypto.token(bytes = 32)` (1 to 1,024 random bytes, base64url: 43 characters by
  default), `crypto.hash_password(pw)` (Argon2id, salted, so it needs the `random`
  effect) and
  `crypto.verify_password(pw, hash)` (false for a wrong password and for anything that
  is not such a hash), `crypto.equal(a, b)` (in constant time, for comparing secrets).
  Password hashing takes tens of milliseconds and 19 MiB on purpose, runs off the event
  loop, and at most one per core at a time (others wait their turn), so a flood of
  logins slows logins down rather than taking the server's memory.
- **Effects:** `sleep(ms)`, `random()`, `random_int(lo, hi)`, `uuid()`, `read_file(path)`,
  `write_file(path, text)` (paths stay inside the program's folder, links followed: see
  [Paths and links](#paths-and-links)). `uuid()` takes its
  bytes from the operating system, so it can serve as a session id or token; `random()`
  and `random_int()` are fast but predictable, and must not.

Safety built in:

- Text in HTML and attribute values is escaped. Keys of a `{...map}` spread on an element
  become attributes only if they are plain names that run nothing: `onclick` (in any
  case, and as `bind:onclick`), `srcdoc`, names with `:` (but `xlink:href`), `@` or
  quotes or `>` are dropped, and so are the attributes Quillo's runtime reads (`data-qi`,
  `data-c`, `data-p`, `data-s` and the rest), so a spread cannot make a component; a key
  cannot replace an attribute written out, in another case either. Event handler attributes and `srcdoc` take only text written in
  the program (see [HTML literals](#html-literals)). `javascript:` URLs in `href`, `src`
  and the like render as `about:blank#blocked`, whatever the value's type: the text the
  attribute would say is checked, so `href={markdown(u, { inline: true })}` too. So do
  the values an SVG `<set>` or `<animate>` gives another attribute (`to`, `from`, `by`,
  each of `values`), which may be an `href`.
- CSS takes data as it is written, but for what would end it: `style={...}` is escaped
  like any attribute, a style map's values and css`...`'s `${...}` have `{ } ; \` written
  as CSS's escapes, and css`...`'s `&` as `\26 ` (so a value adds no rule or declaration,
  in `<svg>` and `<math>` too), and `<style>{x}</style>` has
  its `<` rewritten (`\3c `), so none can end the element; within a value, what it says is
  CSS. CSS runs no script in today's browsers; data put
  there can still restyle the page (cover it with an overlay,
  hide or move what is on it) and load URLs (`background: url(https://elsewhere/?...)`,
  `@import`), which tells another site that the page was viewed and can carry what the
  style sheet says. Put only values you chose into styles (a colour from a list, a
  number), or check what users give against what you allow. A Content-Security-Policy
  header that keeps loading on the site (`img-src 'self'; font-src 'self'`, sent with
  `respond` or by a proxy in front) stops the beacons that go through `url()`.
- Code nested more than 256 levels deep (brackets, operators, elements, ...) is refused
  when it is parsed, so source from anywhere cannot use up the stack.
- `respond` and `redirect` refuse header values with line breaks, which would let text
  from a request add headers of its own, and a location that runs code or carries a page
  (`javascript:`, `vbscript:`, `data:`), naming the one given (`` `redirect` is not given
  a vbscript: URL ``), as `stream`, `site_headers` and `navigate` do.
- A failing request answers `internal error`; the message, with file and line, goes to
  the terminal (and to the page too with [`--watch`](#options-of-quillo-run)). A builtin's
  error is `internal error` to a visitor in the browser too, in production.
- A `json:` pattern matches only a body sent as JSON (or with no content type), not a
  `text/plain` one, which a page on another site could send.
- A page on another site cannot post a form here: a `POST`, `PUT`, `PATCH` or `DELETE`
  with a form, multipart or plain-text body that the browser marks as cross-site
  (`Sec-Fetch-Site`, or an `Origin` naming another host) gets a 403. JSON from another
  site needs the browser to ask first, which nothing here answers, and programs that
  call the site (webhooks, scripts) send no `Origin`, so both work as before. For a
  path that must take such posts, such as a payment provider's return or a single
  sign-on callback, say so at the top of the program: `allow_cross_site("/sso")`.

  ```quillo
  allow_cross_site("/payment")        // the provider's page posts back here

  route POST /payment/return { form: { order: str, status: str } } {
      // anyone can post this: check with the provider before believing it
      redirect(`/orders/${order}`)
  }

  route POST /notes { form: { text: str } } = "saved"
  ```

  A form on `https://pay.example` posting to `/payment/return` reaches the route; the same
  page posting to `/notes` gets 403 and the text `this site does not take form posts from
  other sites`.

  Nor can such a page open the WebSocket on which pages hear of live changes
  (`/_quillo/live`), which the browser would otherwise let it do.
- Globals read with `env`, `read_file`, the database or a service are never written
  into the browser bundle, nor are those made by a function that reads them
  (`let CONFIG = load_config()`); a component that uses one is a load-time error. Only
  variables named `PUBLIC_...`, read with the name written out, are.
- `http.*` in a component goes from the visitor's browser, never from the server, so it
  cannot reach the server's own network: while the server renders a component, a call
  from its code is an error before anything connects.
- `quillo check` warns about an `http.*` call on the server whose host comes from a
  value, which could let a visitor reach the server's own network (localhost, a private
  network, a cloud's metadata service), unless its options say `public_only` (see
  [HTTP client](#http-client)).
- `quillo check` warns about SQL put together from values (`${x}`, `+`, or
  `join([...])`) that reaches `db.query`, `db.one` or `db.exec`, or the same on a
  connection from `db.connect`, whether passed directly, piped (`` `...${x}` |
  db.query ``) or made first in a variable (`let q = ...`, `q += ...`); pass the values
  as `?` parameters instead. Names cannot be parameters: put a table's or a column's
  name in with `sql_name(name)` (or a list of them; quoted `"name"`, or `` `name` `` when
  the program's database is MySQL; `conn.sql_name(name)` quotes for the connection it is
  called on, `db.sql_name(name)` for the program's), and named parameters with
  `sql_param(name)` (or a list: `sql_param(["title", "body"])` is `:title, :body`) —
  both take only letters and digits of any script (`sql_name("名前")`), the marks that go
  on a letter (Hindi's vowel signs, an accent written apart) and `_`, not starting with a
  digit, at most 63 bytes, so that two names are two on every database, whatever their
  argument is made of (`` sql_name(`obj_${id}`) ``). SQL made of them, text as written (also kept in a
  variable), choices between those (`if`, `match`), joins of them with `+`, and calls of
  the program's own functions that only return such text
  (`` fn table_of(id: int) = sql_name(`obj_${id}`) ``), and a `map` whose function gives
  only such text on every way it returns, joined with `join`, is not warned about:
  `let order = match sort { "new" => "created desc", _ => "score desc" }` and then
  `"select * from posts order by " + order`. A run-time set of columns, from a list the
  program wrote: `` `insert into t (${sql_name(cols)}) values (${sql_param(cols)})` `` with
  a map of those columns as the parameters, or for an update the pairs, which every
  database takes (a column list, `set (a, b) = (...)`, is not MySQL's, nor PostgreSQL's for
  one column): `` let set = cols | map((c) => `${sql_name(c)} = ${sql_param(c)}`) | join(", ") ``
  and then `` `update t set ${set} where id = :id` ``. Only the builtins are trusted:
  a `sql_name`, `sql_param`, `map` or `join` the program defines, or a name the lambda
  binds again, is followed no further, and raw text in the lambda
  (`` keys(data) | map((c) => `${c} = 1`) ``) is warned of. Take the columns from
  the program, never from what the visitor sent: `keys(request.form)` names `is_admin`
  and `id` as well as `email`, which `sql_name` takes, as they are names, so a sign-up form
  with `is_admin=1` made an admin (FEEDBACK51.md, Q-662). A record type, `{ form: Signup }`,
  keeps its fields only.
  It is a warning, not a proof.
- Inside `db.transaction` on SQLite, `sleep`, `http` and queries to another database
  (PostgreSQL, MySQL) hold up other requests instead of letting them read and write
  inside the open transaction *(since 0.7: a query to another database let them in)*.
- **A transaction is `db.transaction`'s, not written by hand** *(since 0.7)*. `db.exec`,
  `db.query` and `db.one` refuse `begin`, `start transaction`, `commit`, `end`,
  `rollback`, `savepoint` and `release` alone: a `begin` whose `commit` was never reached
  (a `?` that left, a `reply 400`) left the shared connection in it, and every other
  request's writes with it. `db.transaction(() => ...)` commits, rolls back when something
  fails, and keeps other requests out. A script run whole (`db.exec(read_file("seed.sql"))`)
  may hold its own transaction, as `sqlite3 .dump` writes them, but one it leaves open is
  rolled back and is an error; a SQLite connection still in a transaction at the end of a
  request is rolled back too, and logged. `quillo check` says so of the literal text.

Values are immutable: `push`, `set` and `+` return new lists and maps. A map's keys are
text: `set(m, 5, x)` keys it `"5"`, and `m[5]`, `get(m, 5)` and `has(m, 5)` read that.
`set(xs, i, v)` gives a list with item `i` replaced (from the end when negative). A map
or list built in a loop with `m = set(m, k, v)` (or `push`, `merge`, `remove`), or as
`reduce`'s total with `(m, x) => set(m, ...)`, is changed in place, since nothing else
holds it: building one of 20,000 takes milliseconds. It is the same however it is
written, `m = m | set(k, v)`, `m = m.set(k, v)`, `xs += [x]` or `xs = xs + ys`, at a
script's top level too, and for what is inside: `m = set(m, k, get(m, k, []) | push(x))`
(a map of lists) and `g = set(g, y, set(g[y], x, v))` (a grid) change the inner list
where it is, with a key made by `str`, `lower`, `trim` or `int` too, as those give the same
written twice. `s += t` adds to the text where it is. Where something else still holds it
(a second name for it, say), each step copies it, so that name keeps what it had. In the
browser a list's `push` and `set` always copy, so build a long list there with `for`, or
`flatten` chunks. `flatten` takes one
level off: `flatten(flatten(xs))` for two.

### Paths and links

`read_file`, `write_file`, `delete_file`, `file`, `files`, `save_upload` and
`check_program` take a path from the program's folder, and stay in it. A path with `..`
or starting with `/` is refused, unless the program is started with [`--allow-path`](#options-of-quillo-run)
naming the folder it leads to (`quillo run main.qwe --allow-path /srv/shared/uploads`). A
path is judged where it really leads, with its links followed:

- A link inside the folder that leads inside it works as any path: `alias -> real`.
- A link that leads out of the folder is refused, as `..` is. A cloned repository or an
  unpacked archive can bring one (`data -> /home/other/...`), and on a shared server the
  folder it leads to may be another account's. The first one is said in the log, once:
  `quillo: data/s.txt leads out of the program's folder through a link, to
  /srv/outside/s.txt, and is refused; --allow-path FOLDER lets the program reach a
  folder outside`.
- A link that leads nowhere is refused too: writing through it would make its target,
  wherever that is.
- A program run through a link (a deploy's `current -> releases/42`) is inside the
  folder the link leads to, so its own files work as before. A main file that is itself
  a link (`app.qwe -> /srv/releases/42/main.qwe`) takes its `use`, `import`, `mount` and
  `service` files from beside the real file (a `./` path from the real file's folder),
  while its policy, `.env`, data, Quillo's own tables and messages stay at the link's
  folder, with the path as it was given.
- `--allow-path DIR` opens a folder outside on purpose (a path from the folder `quillo`
  runs in, taken as it really is): a deploy's `uploads -> /srv/shared/uploads` needs
  `--allow-path /srv/shared/uploads`.

What is refused is what any refused path is: an error value from `read_file`,
`write_file` and `delete_file` (`read_file: path must stay inside the program's folder:
data/s.txt`), a 404 from `file` and `files` (the same whether the folder outside is
there or not), and a runtime error from `save_upload`.

```quillo noexec
// alias -> real (inside), data -> ../outside, dangling -> ../outside/gone.txt
print(read_file("alias/in.txt"))                 // inner
print(read_file("data/s.txt") ?? "refused")      // refused
print(write_file("dangling", "x") | is_err)      // true
```

Only the path's parts are looked at, and only a path with a link in it is resolved, so a
static file costs what it did. A link changed between the check and the open is not
caught: Quillo does not use Linux's `openat2`.

### Files of a program

`use lib/forum.qwe` brings another file into the program, and its names into this file.
`import` *(since 0.7)* takes some of them. A file sees its own names, the builtins, and what it takes:

```quillo
// ui/thread.qwe
use lib/forum.qwe                         // every public name of the file
import lib/forum.qwe use *                // the same
import lib/forum.qwe use latest, Feed     // only these
import lib/forum.qwe use latest as newest // this one, called `newest` in this file
```

The file comes first, and the first word says what kind of thing it is, for three reasons:

1. **Mental model.** The line reads in the order things happen: what kind of code this is,
   where it is, then which of its names you want. `import` is Quillo or Rust on the
   server; `client`, `preload` and `preconnect` are the browser, and the word says how
   the page gets the script. One glance at the start of a line tells you which side of
   the program you are on, with nothing to read from the end of a long address.
2. **Editor.** Once the file is written the editor knows its names, so it can offer them
   as you type, and hide those you already took. With the names first, as `import { a }
   from "x"` has them, there is nothing to offer until the path is there. The word is
   `import` rather than `from` for the same reason: `from` starts lines of SQL, and
   programs use it as a name.
3. **Readability.** Lines line up by where things come from, so a file's sources read
   down the left. An address can be long, and the names no longer sit after it at the far
   right: `use` may take the next line.

0.6's `use latest, Feed from lib/forum.qwe` still works, with a warning; `quillo fix`
turns it around. A later release removes it.


- A `use` is for the file that writes it, and is not passed on: when `lib/a.qwe` `use`s
  `lib/b.qwe`, a file that `use`s `lib/a.qwe` sees `a`'s names and not `b`'s. The order
  of `use` lines does not matter, and files may `use` each other.
- **For now a name the file did not take still works, with a warning** *(since 0.7)*: "`price` comes
  from lib/money.qwe, which this file does not take: add `import lib/money.qwe use
  price`". `quillo fix main.qwe` writes those lines, in every file of the program
  (run it once for each program, a service's or a mounted one's too), after a byte-order
  mark and with the file's own line ends (CRLF in a CRLF file). A later release
  makes this an error.
- Two files may define one name: a `helper` in `lib/a.qwe` and a `helper` in `lib/b.qwe`
  are two functions, and each `fn`, `view`, `component` and top-level `let` works this
  way. A file's own name comes first, and then a name it took by name. A file that
  `use`s both files whole and calls `helper` is told to choose ("`helper` comes from
  lib/a.qwe and lib/b.qwe: take the one you mean: import lib/a.qwe use helper"). Taking a
  name the file defines, or one name from two places, is an error: `as` gives one another
  name.
- Three things stay the program's. A `server fn`'s name is its address, so it is defined
  once in a program, and a plain function of that name in another file gives way to it.
  A `type` and a `service` are defined once too, and every file sees them without a
  `use`.
- So a file makes sense only in its program: `quillo check main.qwe` checks it with
  the rest, and so does the editor (a file is checked as part of the program whose main
  file `use`s it; below). Go to definition finds where a name comes from.
- Within a file a name is defined once: a second `fn helper()`, `view` or `component` is
  an error that names both places. `let x` twice in one file gives the variable again.
  Two routes alike are not reported: the first declared
  answers. A function with a builtin's name is a warning (it replaces the builtin for
  the whole program); a variable may take one (`let count = 0`), a type's name included
  (`let num = 5`, `for str in names`): in `let`, `for` and parameters a name binds, and
  only in `match` does `int` test a type. But a *top-level* `let` named like a builtin
  (`let last = ...`, `let int = ...`) hides it in every file of the program: a warning
  at the `let`, and an error at each call that meant the builtin. The same goes for the
  modules: a variable called `db`, `http`, `json`, `re`, `math`, `bit`, `stats`, `vec`,
  `date`, `money`, `crypto`, `image`, `udp`, `channel` or `media` hides the module
  (`let json = request.json`, `let date = row.created`, a parameter `re`), so name such
  values otherwise (`body`, `created`).
- Paths are from the main file's folder, not from the file that writes them:
  `lib/b.qwe` writes `use "lib/c.qwe"`. A path that starts `./` or `../` is from the
  folder of the file that writes it, so a library's file can name its neighbour
  (`ui/slug.qwe` writes `import ./theme.qwe use cls`) and be checked alone, `quillo check
  ui/slug.qwe`. A path outside the folder works (`use "../shared/auth.qwe"` in the main
  file); there are no globs, and a `.qwer` file is `use`d the same way (see
  [Rust](#rust)) (FEEDBACK25.md, Q-325).
- A path may be written without quotes: `use lib/auth.qwe`, `use ../shared/auth.qwe`,
  and `use lib\auth.qwe`, its backslash read as `/`. Without an extension it is the
  `.qwe`, `.qwer` or `.rs` file there (`use lib/auth`), and an error when two are
  (`` `helpers` could be helpers.qwe or helpers.rs: write the one you mean with its
  extension ``); written with its extension, in any case (`MAIN.QWE`, `x.RS`), it is that
  file only, so `import lib.qwer use hi` with only `lib.qwe` there is an error. A file
  that is not there is one error at its import line, for every form, with a file near it
  (`cannot read lib/helpers.rs: No such file or directory (did you mean
  lib/helpers.qwe?)`), and its names are not said again as unknown. `import lib/auth.qwe
  use *` says the same as `use lib/auth.qwe`: every public name.
- A file `use`d twice, or by several files, runs once. Within one file, a second `use`
  of a file, or a name taken twice from it, is a warning: one is enough.
- A name taken `as` another is called that in the file: after `import lib/m.qwe use
  price as cost`, writing `price` is a warning ("this file takes `price` as `cost`"),
  and still works, as a name not taken does. Take both to have both: `use price as cost,
  price`. A type taken `as` another, `import lib/geo.qwe use Point as Pt`, is `Pt` in
  annotations, patterns and type definitions too; `as` cannot take the name of one of
  the program's types (`import lib.qwe use one as Point` is told `` `Point` is a type of
  the program: take `one` from lib.qwe under another name ``), and a name no type can be written as (`P`, one letter) is warned of
  with one that can (`Pt`).
- A `return` at the top level of a file ends the file there: nothing after it is
  defined, and `quillo check` says so at the first line after it (``this never runs: the
  `return` at line 2 leaves the file first, so nothing below it is defined``), as the
  program does when it runs.
- A `use` list may go on over lines, each line after a `,`. A `,` at the end of the
  list is an error: the next line is the next statement, not a name to take.
- **A file's text.** A file is UTF-8; one that is not is refused with the line and column
  of the first bad byte, by `run`, `check`, `build` and `classes` alike. A byte-order mark at the start is skipped, and so is a first line
  starting `#!`, in the main file and in a `use`d one, so a script with
  `#!/usr/bin/env quillo` runs as `./tool.qwe`. A CRLF is a line break, so a string over
  two lines is the same text from a Windows checkout (`\r` written as `\r` stays). A
  character that cannot be seen is named by its code: `unexpected character U+00A0 (a
  no-break space), which cannot be seen: delete it`.
- What runs in the browser, an address or a `.js` or `.mjs` file, starts with `client`:
  `client "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js" use Chart`
  (see [JavaScript libraries](#javascript-libraries)). `preload` in its place fetches the
  script from the page's `<head>`, and `preconnect` only connects to its host. `client
  module "..." use Chart` takes an export of an ES module, kept in the runtime and not on
  `window`; `use default as Name` and `use * as Name` take the default export and the
  whole module. A `.mjs` file is a module without saying so.
- **`private` and `public`.** `private` before a `fn`, `view`, `component` or top-level
  `let` keeps the name to its file: the file uses it as any name, other files do not
  see it, and two files may each have a private `helper` without clashing. `public`
  says the default, for a file that wants to say what it offers:

  ```quillo
  // lib/forum.qwe
  private let PAGE = 20
  private fn page_of(posts: list, n: int) -> list = posts | drop((n - 1) * PAGE) | take(PAGE)
  private view Post(p: map) = <article><h2>{p.title}</h2></article>

  public fn latest(n: int = 1) -> list = page_of(db.query("select * from posts order by id desc"), n)
  public view Feed(n: int = 1) = <div>{for p in latest(n) => <Post p={p}/>}</div>
  ```

  Another file calling `page_of` is told: "`page_of` is private to lib/forum.qwe". Only
  at a file's top level, and not for a `server fn` (an address on the server anyone can
  call; keep the work in a private `fn` it calls) or a `type`. A view's or component's
  parameter cannot share a private name of its file, as props are passed by name, nor a
  name two files define.
  Elsewhere `private` and `public` are ordinary names (`{ private: true }`). A private
  function given to `schedule` or `every` keeps its job to itself, under a name of its
  own (`tick__pv` and eight hex digits in the log), made from the file's path from the
  main file's folder, so copies of a program started from different folders (`quillo
  run current/main.qwe`, a service under `--split`) share the job.
- An editor finds the program a file belongs to by looking, in the file's folder and up
  to three above it (stopping at a folder with `.git` or `Cargo.toml`), for the first
  `.qwe` file by name that `use`s it, directly or through others. Keep a program's files
  within three folders of its main file, and one program per folder: two main files
  side by side that `use` one file have it checked as part of the first by name.
- A file that cannot be parsed is reported first, since what follows may be its doing,
  and the names it defines are not also reported as unknown where they are used.
- `x.helper()` finds a function by its plain name, so it does not reach a name two files
  define: write `helper(x)`.

### Decimals

A `dec` is an exact decimal number, for money and whatever else is counted in cents:
`12.50m`, `5m`, `-0.01m` (the `m` is C#'s). A float is a binary fraction, so `0.1 + 0.2`
is `0.30000000000000004`; decimals are exact, so `0.1m + 0.2m == 0.3m`.

```quillo
let price = 19.99m
let total = price * 3 + 4.50m          // 64.47m
let vat = round(total * 0.21m, 2)      // 13.54m (half to even)
let each = total / 3                   // 21.49m
dec("12.50")  dec(42)  dec(0.1)        // 12.50m, 42m, 0.1m
```

- A decimal keeps its digits: `12.50m` shows as "12.50", and `1.10m * 3` is `3.30m`.
  Equality and order do not see zeros at the end (`12.5m == 12.50m`), and neither do
  map keys (`set`, `get`, `group_by`).
- It holds 28 or 29 significant digits: a 96-bit whole number and up to 28 decimals, as
  C#'s `decimal`. `+`, `-` and `*` are exact; an answer with more decimals than fit keeps
  its whole part and rounds the rest half to even, and a whole part too big is an error
  (`decimal overflow`), as with ints. `/` is exact when the answer has 28 decimals or
  fewer, else rounded half to even to 28 or 29 digits (`1m / 3` is
  `0.3333333333333333333333333333m`). Dividing by zero is an error, and `%` is the
  remainder as for ints.
- With an int, a decimal stays exact and gives a decimal. With a float it is an error,
  never a silent float: `1.5m + 0.5` says to convert one with `dec(x)` or `float(x)`.
  `==` compares a decimal and a float through the float's shortest text (`0.1m == 0.1`).
- `dec(x)` makes one from text (exactly: `dec("abc")` is an error value, as `int("abc")`
  is; an exponent is read, so `dec("1e3")` is `1000m`), an int, or a float through its
  shortest text (`dec(0.1)` is `0.1m`, not
  `0.1000000000000000055…`). `int(d)` keeps the whole part, `float(d)` gives the
  nearest float, `str(d)` its digits.
- `round(d, digits = 0, mode = "half_even")` gives exactly that many decimals
  (`round(12.5m, 2)` is `12.50m`; negative digits round to tens, hundreds, ...). The
  modes: `half_even` (banker's rounding, the default), `half_up` (a half away from
  zero, as taught at school), `half_down`, `up` (away from zero), `down` (toward zero),
  `ceiling` and `floor`. `floor`, `ceil` and `abs` give decimals; `sum`, `min`, `max`
  and `sort` take lists of them, and a sum stays exact.
- `pow(d, n)` raises a decimal to an int's power, by squaring: exact while the answer
  fits, else each product rounded as `*` rounds, and a negative power is one over the
  positive one *(since 0.7)*. Interest and loan payments need no floats:
  `pow(1m + 0.05m / 12, 360)` is `4.4677443140061322124280701433m`, right to 25 digits. A
  power that is not an int is an error: `float(x)` and `math.pow` for that.
- `math`, `stats` and `vec` work on floats: a decimal there is an error that says to
  use `float(x)`.
- `json.stringify`, and a route answering a map, write a decimal as a string
  (`"12.50"`), so nothing that reads it rounds it. `json.parse` makes no decimals:
  numbers stay numbers.
- In a pattern or a record's field, `dec` takes a decimal, an int, numeric text
  (`"12.50"`, from a query, a form or JSON) or a JSON number (through its shortest
  text), and gives a decimal: `route GET /quote ? { amount: dec }`, `type Line = {
  price: dec }`, `/pay/:amount(dec)`. A `server fn`'s `dec` parameter takes the same
  from a page. In an annotation (`fn f(x: dec)`) it takes only a decimal.
- In the browser decimals are the same, and compiled components give the server's
  answers: arithmetic, comparison, rounding, display. They cross between the server
  and the page (props, state, server fns' arguments and answers) as decimals. Ints too
  large for a JavaScript number (past 2^53) reach the page whole, as BigInts, and show
  as on the server. A whole float is a float there too, so adding one to a decimal is an
  error on both sides, and `money.of(2.0, "USD")` is refused in both: an amount never goes
  through a float.
- `quillo check` knows them: `12.5m + 1` is a `dec`, `12.5m + 0.5` an error, and `round`,
  `abs`, `floor`, `ceil`, `pow`, `sum`, `min` and `max` keep the type they are given. `num`
  is still `int | float`.

**Rounding.** Each place that rounds says how. A tie, the 5 of `0.125` to two places:

| Where | A tie | `0.125` | `1.005` |
|---|---|---|---|
| `money.*`, and `round(d, n)` of a decimal | to even, unless a mode says otherwise | 0.12 | 1.00 |
| `format_number(x, locale, n)` | away from zero, from the shortest decimal, as `Intl.NumberFormat`; or `mode` | 0.13 | 1.01 |
| `round(x, n)` of a float | away from zero, from the float's shortest decimal *(since 0.7: `1.005` was `1.0`)* | 0.13 | 1.01 |
| `money.format`, where a currency shows fewer decimals than it has (IRR) | away from zero, as `Intl.NumberFormat` | | |
| PostgreSQL `numeric(p, s)` and MySQL `decimal` when a value has more decimals | away from zero | 0.13 | 1.01 |

So write a line of an invoice as money works it out: `format_number(x, locale, 2,
"half_even")`, or `money.format` of the line's money, never `format_number` of the
amount with its default, which may show a cent more than the total adds up.
- [Compiled numbers](#compiled-numbers) leave decimals to the interpreter.

**In databases.** PostgreSQL's `numeric` and MySQL's `decimal` come back as decimals with
their scale (`numeric(10, 2)` gives `19.99m`, and `12.50` stays `12.50m`), and decimals
are sent exactly. A value without a scale (as `sum` of ints gives) is an int, and one
past 28 decimals or 29 digits is its exact text. In MySQL a decimal parameter arrives
as text, which MySQL turns exactly into a `decimal` column, but into a double inside an
expression: write `cast(? as decimal(12, 2))` there.

SQLite has no decimals. A decimal is stored as its text, and a column declared
`decimal(12, 2)`, `numeric(12, 2)` or `money` is read back as decimals (a bare `numeric`,
SQLite's name for any number, gives numbers as before). But such a column has
NUMERIC affinity: SQLite turns "0.10" into the real 0.1 (and rounds past 15 digits), so
the scale is lost and large amounts are not exact. For exact amounts, declare the
column `decimal_text` (TEXT affinity: SQLite keeps the digits as written, and Quillo
still reads them as decimals), or keep whole minor units (`amount_cents integer`; see
`money.to_minor`). SQL's own sums and ordering do not work on text: `max` of a
`decimal_text` column is the largest text (`"9.99"` before `"100.00"`), as text; add up
and sort in Quillo, or keep minor units. The first time SQLite is given a `create table`
or `alter table` with a `money`, `decimal`, `dec(p, s)` or `numeric(p, s)` column, the log
says which column, once *(since 0.7)*.

PostgreSQL's own `money` type keeps a count of the smallest unit of its `lc_monetary`'s
currency. Quillo reads it with that currency's decimals, asked of the connection the first
time a result has one: `500` written reads back as `500m` under a yen `lc_monetary`, and
`500.00m` under a dollar one *(since 0.7: it was always read as cents, so ￥500 came back
as `5.00m`)*. It carries no currency: `numeric(p, s)` and a currency column say more.

A list given as a query's parameters, `db.exec("insert ... values (?)", [x])` as Python's
`sqlite3` takes them, is one parameter, stored as the list's JSON text. Parameters are
separate arguments: `db.exec(sql, a, b)`. `quillo check` says so when the SQL is written
out and has a `?` *(since 0.7)*, but not where a list is PostgreSQL's array: a `?` in
`any(...)` or `all(...)`, or cast to an array type (`?::int[]`, `?::jsonb[]`).

### Money

`money` keeps amounts of a currency: a map `{ amount: dec, currency: "USD" }`, the
amount always with the currency's minor units (ISO 4217: JPY 0, USD 2, BHD 3). It is a
plain map, so JSON, the database and the browser take it as it is, and `money` checks
it each time it is given one. On the server and in the browser alike:

```quillo
let price = money.of(19.99m, "USD")             // { amount: 19.99m, currency: "USD" }
money.of(form.amount, "EUR") ?? reply 400       // text from a form; an error value if bad
money.parse("1.234,50 €", "EUR", "de-DE")       // 1234.50, as a German visitor writes it
money.of("12.345", "USD")                       // err: more decimals than USD has
money.of("12.345", "USD", "half_up")            // 12.35
money.from_minor(1250, "USD")                   // 12.50, from cents
money.to_minor(price)                           // 1999
price + tax  price - discount                   // amounts of one currency, added
money.add(a, b)  money.sub(a, b)  money.neg(a)  money.compare(a, b)   // -1, 0, 1
money.mul(price, 3)  money.mul(price, 0.075m, "half_up")              // rounded to cents
money.allocate(money.of(100, "USD"), [1, 1, 1])  // 33.34, 33.33, 33.33
money.format(price, "de-DE")                    // "19,99 $"
```

- `money.of(amount, currency, mode)` takes a decimal, an int or numeric text (a float
  is a runtime error, as is a currency that is not text: `dec(x)` it first). Text is an
  amount as a program writes one: digits, a sign and a point, not `"1e3"` and not
  `"1,234.50"`. An amount with more decimals than the currency has, bad text, an amount
  whose minor units do not fit an int (so that `money.to_minor` always can) and an
  unknown currency give error values, as `int("x")` does; with a rounding mode (those of
  `round`), the amount is rounded instead. Codes are ISO 4217's
  (`money.minor_units("JPY")` is 0), and withdrawn ones still read, so last year's
  invoice in leva is money: BGN, HRK, ANG, SLL, ZWL and the others withdrawn since 2007
  *(since 0.7)*. Codes without minor units, such as XAU, and crypto currencies are not
  money.
- `money.parse(text, currency, locale, mode)` reads an amount as the locale writes one,
  for a form *(since 0.7)*: its decimal mark and group mark, Indian grouping in en-IN, a
  minus, and the currency's symbol or code before or after it, as `money.format` writes
  them. Groups must be whole (`"1.00"` is not an amount in de-DE), and a mark of the
  other kind is refused (`"1,5"` is not an amount in en-US). `money.of` reads text as a
  program writes it, so a German visitor's `"1.000"`, one thousand, was one euro there.
  Without a locale, the program's (`locale()`), else en-US. What is wrong is an error
  value, as from `money.of`.
- `a + b` and `a - b` on two money values add and subtract the amounts, as `money.add` and
  `money.sub` do, with `+=` and `-=` too; two currencies give an error value *(since 0.7:
  `+` merged the two maps, so a cart's total was its last line)*. Only maps of exactly
  `amount` and `currency` are money here: other maps still merge. `money.add`, `sub` and
  `compare` take two amounts of one currency, and two currencies are a runtime error.
  `mul(m, factor, mode = "half_even")` multiplies by a decimal or an int and rounds to the
  currency; `round(m, mode)` rounds an amount made by hand. `zero(currency)`,
  `is_zero(m)`.
- Every `money` function reads a map it is given as `money.of` reads an amount: the code
  trimmed and in any case, text without an exponent, no more decimals than the currency
  has, minor units that fit an int. `money.round(m, mode)` alone takes more decimals, and
  makes money of them. Anything else is a runtime error that says why *(since 0.7: some
  functions rounded $1.005 and others took it as it was, so `compare` said it was more
  than $1.00 while `format` wrote $1.00)*.
- An amount past what `money.of` takes (minor units that do not fit an int) is an error
  value from `+`, `-`, `money.add`, `money.sub`, `money.mul`, `money.neg` and
  `money.round` *(since 0.7: they made it, and only `money.to_minor` refused it)*. A bad
  argument (a float, a map that is not money, two currencies to `money.add`) is a runtime
  error, a mistake in the program. `quillo check` types their results as money or an
  error.
- `money.allocate(m, ratios)` splits an amount by ratios (ints or decimals; a number n
  for n equal parts) without losing or making a minor unit: each share gets its part,
  rounded down, and the units left over go one each to the shares closest to their next
  unit, the largest remainders, a tie to the first *(since 0.7: to the first shares, so a
  1:99 split of $0.10 gave the 1% share $0.01)*. The shares always add up to the amount,
  and a ratio of 0 gets nothing.
- `money.format(m, locale)` writes it as CLDR has it for the locale, as
  `Intl.NumberFormat` writes it in a browser: `$1,234.50` (en-US), `1.234,50 €` (de-DE),
  `1 234,50 €` (fr-FR, a narrow no-break space between thousands), `￥1,235` (ja-JP),
  `₹12,34,567.00` (en-IN), `F CFA 1,234` (XOF in en-US), `IRR 1,235` (CLDR shows no
  decimals of the rial; the amount keeps them). Without a locale it uses the program's
  (`locale()`, with [Translations](#translations)), else en-US. 122 locales are covered,
  written from ICU 77's data by `scripts/money-tables.mjs`, and every currency's symbol in
  each *(since 0.7: 48 locales and 16 symbols)*. A name is matched without regard to case,
  with `_` for `-` and its extensions (`-u-...`) left out; Chinese by its script
  (`zh-Hant` as zh-TW); Spanish of Latin America as es-419 and Portuguese of Africa and
  Asia as pt-PT, as CLDR's parent locales are; a language alone as its first locale (`fr`
  as fr-FR, `es` as es-ES). Any other is written as the first of its language, else as
  en-US, and the log says so once (the browser's console too). Not covered: languages
  whose digits are not 0-9 (Arabic in Egypt, Persian, Bengali, Burmese, Nepali, Marathi),
  right-to-left ones (Hebrew, Arabic in the Emirates and Morocco), Urdu and en-150.
- `format_number(x, locale, digits, mode)` writes any number the same way: `format_number(
  1234567.5, "en-IN", 2)` is `"12,34,567.50"`. `digits` is kept within 0 to 28; a float
  too large for a decimal (past about 7.9e28), NaN or an infinity is written plainly,
  without grouping. It rounds a tie away from zero, from the number's shortest decimal, as
  `Intl.NumberFormat` does, on both sides: `2.5` to 0 digits is `3`, and `1.005` to 2 is
  `1.01`; `mode` rounds as `round` does instead (`"half_even"`, as money) *(since 0.7)*.
  `digits` may be `{ min, max }` (`max` is `max(min, 3)` when left out), for as many digits
  as the number needs within them: `format_number(10, "en-US", { max: 3 })` is `10`,
  `8.875` stays `8.875`, and `{ min: 2, max: 3 }` writes `8.8` as `8.80`. With no
  `digits`, every digit of the number is written (`Intl` stops at three), on both sides.
  A negative number rounded to zero keeps its minus, as `Intl` writes it: `-0.001` to 2
  digits is `-0.00`, and `-0.0` is `-0`. A whole float past 2^53 is written by its
  decimal digits in the browser too, from Quillo or from the page's JavaScript.
- There are no exchange rates: an amount stays in its currency, and converting is the
  program's arithmetic with a rate it chose.

To hold what a client sends to money's rules, name the type `money`: in a pattern, `route
POST /pay { json: { price: money } }`, a record's field, `type Line = { item: str, price:
money }`, or an annotation, `fn total(m: money)`. In a pattern or a field it takes a map
of an `amount` (a decimal, an int, numeric text or a JSON number) and a `currency`, holds
it to `money.of`'s rules with no rounding mode, and gives the money; a request that fails
gets a 400, and the log (or the page, under `--watch`) says which rule, without the value
sent: "`json.price` has more decimals than JPY has (0)". In an annotation it takes money
as the `money` functions take it. A record written out, `type Money = { amount: dec,
currency: str }`, checks only the shape *(since 0.7: that was the way, and $10.004 went
through to be cut to $10.00, `"1e3"` was 1,000, and BTC a 500)*.

### Numbers and data

`math` is Python's math module, `stats` its statistics module with what a data frame's
`describe` adds, and `vec` numpy's operations on lists of numbers. They are on the server
and in the browser alike, so a component can chart what a route computed:

```quillo
let totals = vec.column(orders, "total")          // [12.5, 30, 7.5, ...]
stats.describe(totals)        // { count, sum, mean, std, min, q1, median, q3, max }
stats.quantiles(totals, 10)   // deciles, as Python computes them
stats.correlation(ads, sales) // Pearson; "ranked" for Spearman
stats.linear_regression(ads, sales)   // { slope, intercept }
stats.histogram(totals, 20)   // { counts, edges }, as numpy's
vec.rolling_mean(daily, 7)    // a week's moving average
math.comb(52, 5)  math.log(8, 2)  math.fsum(xs)  math.isclose(a, b)  math.pi
```

- **`math`:** `sqrt cbrt exp exp2 expm1 log(x, base) log2 log10 log1p pow`, `sin cos tan
  asin acos atan atan2` and their hyperbolic forms, `degrees radians hypot dist`, `floor
  ceil trunc` (ints, up to 2^63; of an int divided by an int, written in the call, the
  exact int, as `floor` gives it) `fabs copysign fmod remainder modf frexp ldexp`, `factorial gcd lcm
  comb perm isqrt` (ints), `isclose isfinite isinf isnan`, `prod fsum sumprod`, `erf erfc
  gamma lgamma`, and `pi e tau inf nan`. What Python raises is an error here too: a math
  domain error (`math.sqrt(-1)`, `math.log(0)`), a range error (`math.exp(1000)`), an int
  past 64 bits (`math.factorial(21)`).
- **`bit`:** *(since 0.7)* `and(a, b) or(a, b) xor(a, b) not(a) shl(a, n) shr(a, n)`, on the 64 bits of
  an int, with the same answers in the browser (where JavaScript's own `&` and `<<` keep
  32). `shl` drops what goes past the top, unlike `*`, which refuses; `shr` keeps the
  sign, and `bit.and` masks it off: `bit.and(bit.shr(x, 8), 0xFF)`. A shift outside 0 to
  63 is an error. A variable called `bit` hides the module, as with `math`.
- **`stats`:** `mean fmean(data, weights) geometric_mean harmonic_mean`, `median
  median_low median_high mode multimode`, `pvariance variance pstdev stdev` (each with an
  optional known mean), `quantiles(data, n = 4, method = "exclusive")` (n up to 10,000), `covariance
  correlation(x, y, method = "linear") linear_regression(x, y, proportional = false)`,
  `normal_pdf normal_cdf normal_inv` (x, mu = 0, sigma = 1); and, beyond Python's,
  `percentile(data, p)` (p from 0 to 100, or a list of them; numpy's linear rule),
  `describe`, `histogram(data, bins = 10)` (or the edges as a list), `zscore`,
  `normalize` (to 0..1) and `rank` (ties share their average rank, a float; others are
  ints). What puts data in order or counts them (the medians, `quantiles`, `percentile`,
  `describe`, `rank`, `histogram` and its edges) refuses NaN and infinities, naming which
  and where, as `sort` does *(since 0.7: `median([3, NaN, 1])` was 3.0, and a histogram
  with `inf` counted nothing)*; a mean or a spread has them as Python's does. Statistics of no data, or of too little, is an error, as in Python, except
  `describe([])`, which is `{ count: 0, sum: 0 }` with the rest `null`, and `zscore` and
  `normalize` of a constant list, which give zeros. `mode` and `multimode` take any
  values, text too. Python's `median_grouped` and the rest of `NormalDist` are not
  there.
- **`vec`:** `add sub mul div mod` (two lists of one length, or a list and a number; ints stay
  ints, as with the operators), `pow dot norm(xs, p = 2)`, `cumsum cumprod diff`,
  `argmin argmax argsort(xs, descending = false) clip(xs, lo, hi)` (either bound may be
  `null`), `abs sqrt exp log round(xs, digits)`, `linspace(start, stop, n = 50, endpoint
  = true) arange(start, stop, step) zeros ones full(n, x)` (at most ten million items),
  `rolling_mean rolling_sum(xs, window)` (one per full window),
  `matmul(a, b)` (rows times rows, or times a list), `transpose`, and `column(rows, key)`
  (a field of each map, or an item of each list).

Answers are Python's: sums are compensated and rounded once, so `stats.mean`,
`stats.variance` and `math.fsum` give Python's exact results (its statistics module
uses fractions; this gets the same float without them), and the rest agree to the last
digit or one or two places from it. None of it runs in the interpreter: a list is read
once into floats, and a long one (over 128K items) is worked on by all the cores, in
chunks of 32K whatever the machine, so the answer does not depend on how many cores
it has. A median, a percentile or a quantile sorts only the part of the list where it
falls (cut points from a sample split the list into ranges of about equal size), and
`vec.argsort`, and `sort` of a list of 64 or more items that are all ints or all floats
(no NaN), sort those ranges side by side; any other list sorts on one core. There is
no matrix or data-frame type: rows are lists of maps, `vec.column` takes a field of
each, and `vec.matmul` and `vec.transpose` work on lists of lists. On a million floats, against
Python 3.9 (no numpy) on the same 10-core machine:

| | Quillo | Python |
|---|---:|---:|
| `stats.mean` | 0.35 ms | 214 ms |
| `stats.stdev` | 0.65 ms | 674 ms |
| `stats.median` | 1.3 ms | 44 ms |
| `stats.quantiles(xs, 100)` | 2.0 ms | 45 ms |
| `stats.correlation` | 1.4 ms | 246 ms (3.10's algorithm) |
| `stats.linear_regression` | 1.3 ms | 182 ms (3.10's algorithm) |
| `stats.describe` | 1.7 ms | 764 ms (by hand) |
| `stats.histogram(xs, 50)` | 0.6 ms | 126 ms (a loop) |
| `math.fsum` | 1.6 ms | 20 ms |
| `vec.dot` | 0.9 ms | 11 ms (`sum(map(mul, x, y))`) |
| `vec.add` | 1.9 ms | 20 ms (`list(map(add, x, y))`) |
| `vec.rolling_mean(xs, 30)` | 2.0 ms | 69 ms (a running sum) |
| `vec.argsort` | 5.4 ms | 75 ms (`sorted(range(n), key=...)`) |
| `sort` | 4.6 ms | 46 ms |

Making a list one item at a time is the interpreter's work, though: `map(range(n), (i)
=> ...)` over a million takes about 1.4 times Python's list comprehension. Built with
`vec` instead (`vec.div(vec.mod(vec.mul(vec.arange(n), k), m), 1000.0)`), the same list
takes a fifteenth of the comprehension's time.

### Dates and times

There is no date type: a date is its ISO 8601 text, `"2026-10-27"`, and a date-time is
RFC 3339 text with its offset, `"2026-10-27T14:30:00Z"` or `"2026-10-27T15:30:00+01:00"`.
Dates sort and compare in time order as text. Date-times do only when they are written
the same way, with the same offset and the same digits: `"…14:30:00.000Z"`, as
JavaScript's `toISOString()` writes it, sorts before `"…14:30:00Z"`, though they are
one instant. To order or compare date-times, use `date.compare(a, b)` or `date.ms`.
Both go through JSON, databases, forms and the browser as they are. The `date` module works on it, on the server and in the browser
alike, with the same answers:

```quillo
date.today()                          // "2026-10-27", in UTC; date.today("Europe/Paris")
date.now()                            // "2026-10-27T14:30:00Z", to the second
date.add_months("2026-01-31", 1)      // "2026-02-28": the end of the month, not March
date.to_zone("2026-10-27T14:30:00Z", "Europe/Paris")       // "2026-10-27T15:30:00+01:00"
date.at("2026-10-27", "17:00", "America/New_York")         // "2026-10-27T17:00:00-04:00"
date.at("2026-10-27T17:00", "America/New_York")            // the same, from <input type="datetime-local">
date.compare("2026-10-27T14:30:00.000Z", "2026-10-27T15:30:00+01:00")   // 0: one instant
date.days_between("2026-11-05", "2026-11-14")              // 9: the nights of a stay
date.format("2026-10-27", { locale: "de-DE", style: "long" })  // "27. Oktober 2026"
date.parse("27/10/2026", "DD/MM/YYYY")                     // "2026-10-27", or an error value
```

- **Making and reading:** `date.of(y, m, d)`, `date.valid(s)` (a date or a date-time;
  `date.valid(s, "date")` only a date and `date.valid(s, "date-time")` only a date-time:
  check text from a page with the first before the functions below, which take only
  dates and fail on a date-time with a runtime error, not an error value; or give the
  field or parameter the type `date`, see [Records](#records)),
  `date.parts(d)` (`{ year, month, day, weekday }`, weekday 1 for Monday to 7 for Sunday;
  a date-time adds `hour minute second offset`), `date.weekday(d)`, `date.is_leap_year(year
  or d)`, `date.days_in_month(d)` or `(year, month)`.
- **Arithmetic:** `date.add_days(d, n)`, `date.add_months(d, n)` and `date.add_years(d, n)`
  (a day past the month's end is its last day: `2024-02-29` plus a year is `2025-02-28`),
  `date.days_between(a, b)` (`b - a`, in days), `date.months_between(a, b)` (whole months:
  `2026-01-31` to `2026-02-28` is one), `date.start_of_month(d)`, `date.end_of_month(d)`.
  These take dates; `date.date_in(t, tz)` gives a date-time's date.
- **Milliseconds:** `date.ms(t)` is a date-time (or a date, at its midnight in UTC) as
  milliseconds since 1970, what `now()` gives, and `date.from_ms(ms, tz)` a time kept that
  way as a date-time (to the second): `date.from_ms(now(), "Europe/Lisbon")`.
- **Comparing:** `date.compare(a, b)` *(since 0.7)* is -1, 0 or 1 by the instants, fractions of a
  second included, a date counting as its midnight in UTC: the safe way to order or
  compare date-times written differently, `sort_by(xs, (x) => date.ms(x.at))` the way
  to sort them.
- **Time zones:** `date.valid_zone(z)` *(since 0.7)* asks whether a zone exists, for one a person typed;
  a zone it does not know stops the call, so `date.to_zone(t, z) ?? t` guards it too.
  `date.to_zone(t, tz)` is the same instant at the zone's offset then
  (daylight saving included), a fraction of a second that is all zeros dropped (`.000Z`
  becomes `Z`); `date.date_in(t, tz)` its calendar date there;
  `date.at(d, "HH:MM", tz, options)` the instant a local time names, and
  `date.at("2026-10-27T14:30", tz, options)` the same from one text (seconds may follow,
  and a space may stand for the `T`), as `<input type="datetime-local">` gives it. When
  clocks go back, a time that happens twice is the first, or `{ repeated: "later" }` the
  second; when they go forward, a skipped time is moved on by the gap (02:30 becoming
  03:30), or `{ skipped: "earlier" }` moved back, or `{ skipped: "error" }` an error
  value: `date.at: 2026-03-08 02:30 is skipped in that zone, as its clocks move
  forward`. A zone is a name of the IANA database, a zone or a link, in any case
  (`"Europe/Paris"`, `"US/Pacific"`, `"EST"`), `"UTC"`, or an offset as `date.parse` reads
  one (`"+05:30"`, `"+0530"`, `"+05"`, up to `+23:59`); left out, it is UTC. An offset of
  zero is written `Z`. The server and the browser take the same zones and refuse the same
  names, as an unknown time zone: `posixrules`, `Factory`, `Etc/Unknown`, `localtime`,
  `posix/...` and `right/...` (copies some systems keep), names the database dropped
  (`US/Pacific-New`, `Canada/East-Saskatchewan`, `SystemV/...`), Java's three-letter ids
  (`PST`), and any other name with a sign (`+5`, `+0560`, a U+2212 minus). The server
  reads the system's time zone database, and a copy built into `quillo` where there is none
  (Windows, small containers) and for a name the system's leaves out (Ubuntu 24.04 moved
  `EST`, `US/Pacific`, `Asia/Calcutta` and `EST5EDT` to a package of their own,
  `tzdata-legacy`), so a program takes the same zones on every machine; the browser asks
  its own (`Intl`).
- **Text:** `date.format(d, pattern)` with `YYYY YY`, `MM M` (month number, padded or
  not), `MMMM MMM` (its name), `DD D`, `dddd ddd` (the weekday's name), `HH H` and `hh h`
  (24 or 12 hours), `mm ss A` (AM/PM) and `Z` (the offset); anything else, and text in
  `[brackets]`, is kept as it is. A date-time shows its time as written: `date.to_zone` it
  first to show another zone's. `date.format(d, { locale, style })` gives a locale's
  `"short"`, `"medium"` (the default), `"long"` or `"full"` date, and `{ locale, pattern }`
  the pattern with that language's names. The locales are English, German, French,
  Spanish, Italian, Dutch and Portuguese, and the regions whose styles CLDR gives their
  own: `en-GB`, `en-CA` (`2026-09-05` short), `en-IE`, `en-AU`, `en-NZ`, `en-IN`,
  `en-ZA`, `de-AT` (`Jänner`, `Jän.`), `fr-CA`, `fr-CH`, `es-MX`, `it-CH`, `nl-BE` and
  `pt-PT`, the same on the server and in the browser (CLDR's forms, names in a pattern
  too); another region takes its language's (`de-CH` is German's), and any other
  language gives English. `date.parse(s)` reads a date or a date-time, with `T` or
  a space and an offset (`+01`, `+0100`, `+01:00` or `Z`), and gives it back as above;
  a date-time with no offset is an error value that says to read it with `date.at(v,
  zone)`, as it names no instant until a zone is given; an HTTP date (`Tue, 27 Oct 2026
  14:30:00 GMT`) is read as UTC;
  `date.parse(s, pattern)` reads by a pattern (English names; `YY` is 2000 to 2099; with a
  time and no `Z`, UTC; its `Z` reads `Z`, `+01:00` or `+0100`, not `+01`). Either gives
  an error value when the text is not a date. There is no token for fractions of a
  second, though `date.parse` reads them and `to_zone` keeps them.

A malformed date (`"2026-02-30"`, `"27/10/2026"`, a number) is an error in every function
but `date.parse` and `date.valid`. Years are 1 to 9999. `date.today` and `date.now` read
the clock, so they are the `time` effect, and scripts run with `run_script` have the rest
of `date` but not them. A program's own variable called `date` hides the module, as with
the others.

What a form's date and time inputs send, and what reads it:

| Input | Sends | Read it with |
|---|---|---|
| `<input type="date">` | `2026-10-27` | as it is: it is a date |
| `<input type="time">` | `14:30` | `date.at(day, v, zone)`'s second argument |
| `<input type="datetime-local">` | `2026-10-27T14:30` | `date.at(v, zone)` |
| `<input type="month">` | `2026-10` | no `date` function reads it: split it, `date.of(y, m, 1)` |
| `<input type="week">` | `2026-W44` | no `date` function reads it |

None of them carries a zone: the program says which (a user's setting, the `tz` cookie
below).

Business days, holidays and the day counts of interest are a program's own: a calendar
is a list of holidays and the arithmetic above.

The server does not know a visitor's time zone: `date.today()` is UTC's. A page tells it
with a cookie, set from `Intl.DateTimeFormat().resolvedOptions().timeZone` through
`js`, which a helper reads with `cookie("tz")`; the first page a browser asks for is made
before the cookie exists.

```quillo
fn zone() -> str = cookie("tz") ?? "UTC"     // in any function the request runs

server fn set_zone(zone: str) =
    date.valid_zone(zone) ? set_cookie("tz", zone, { max_age: "1y" }) : null

component SendZone(known) {            // once in the layout: sends the zone when it differs
    <span hidden onmount={() => {
        let z = js("Intl.DateTimeFormat", []).resolvedOptions().timeZone
        if z != known { set_zone(z) }
    }}></span>
}

route GET / = <html><body>
    <SendZone known={cookie("tz")}/>
    <p>Today, where you are: {date.today(zone())} ({zone()})</p>
    <form method="post" action="/meet">
        <input type="datetime-local" name="at"/>
        <button>Book</button>
    </form>
</body></html>

route POST /meet { form: { at: str } } = `Booked for ${date.at(at, zone())}`
```

In Manila the first page says `(UTC)`; from the next one on, `Today, where you are:
2026-11-02 (Asia/Manila)`, and 09:30 booked in the form is `Booked for
2026-11-20T09:30:00+08:00`. `set_zone` checks the zone, since anyone can call it with any
text.

### Database

`db` talks to SQLite (built in), PostgreSQL or MySQL/MariaDB, with the same functions.
SQL problems are error values, so `?` and `??` work on them. `db` is a library, not
syntax: other stores (Redis, MongoDB, ...) come in through Rust (see
[Other stores](#other-stores)).

```quillo
db.open("app.db")                                   // SQLite, next to the program; ":memory:" also works
db.open("postgres://user:pass@host:5432/app")       // PostgreSQL
db.open("mysql://user:pass@host:3306/app")          // MySQL or MariaDB (also mariadb://)
db.exec("create table if not exists todos (id serial primary key, text text)")
db.exec("insert into todos (text) values (?)", text)       // { changes, last_id }
db.exec_many("insert into todos (text) values (?)", [["a"], ["b"]])   // { changes: 2 }, all or none
db.query("select * from todos where id > ?", 10)           // a list of maps
db.one("select * from todos where id = :id", { id })       // a map, or null; keys the SQL does not use are ignored
db.one("insert into todos (text) values (?) returning id", text)
db.transaction(() => { ... })                              // all or nothing; nests
db.transaction({ isolation: "serializable", retries: 3 }, () => { ... })
```

Without `db.open`, the database is `$QUILLO_DB` (a path or a URL), or else
SQLite in memory. A transaction rolls back if its function fails, returns an error
value or replies early, and also when a statement in it gives an error that nothing
takes: `db.exec(...)` alone on a line that fails rolls the whole transaction back, and
the transaction gives that error as its value, so half the work is never committed.
A transaction SQLite ends by itself (a full disk, an I/O error), rolling back all it held,
is over: each statement after it in the function is an error, and the transaction gives
``the database ended this transaction after an error, and rolled back all it held``
*(since 0.7: what ran after it was committed statement by statement)*.
An inner transaction whose error is dropped rolls the outer one back too; `let _ =
db.transaction(...)` for the inner one says its failure is not wanted, and the outer one
goes on. `quillo check` warns when a `db.transaction(...)` call's value is dropped
(FEEDBACK27.md, Q-332). A SQLite path is from the program's folder, not the one Quillo
runs in, and `quillo run` says which file it opened when it starts
(`quillo: database /srv/app/app.db`). Only `postgres://`, `postgresql://`, `mysql://` and
`mariadb://` are servers: any other text, `sqlserver://...` included, is taken for a
SQLite file's path. SQL Server, Oracle and the rest are reached through Rust (see
[Other stores](#other-stores)).

**Many rows at once: `db.exec_many(sql, rows)`.** *(since 0.7)* It runs the statement for each row, a
list of its `?` values, and gives `{ changes }`, the rows changed in all. It is all or
none: it runs in a transaction of its own, or as a savepoint of the one it is in, so a row
that fails (a duplicate key) leaves the table as it was, and gives the error. An empty
list changes nothing. On PostgreSQL and MySQL an `insert into t (...) values (?, ...)`
with nothing after its values goes as inserts of up to 1,000 rows each, as Python's
`executemany` sends them, not a round trip a row; any other statement (an `update`, an
insert with `on conflict`) runs a row at a time, in the one call. For one row's
`last_id`, use `db.exec`.

```quillo
let rows = for p in imported => [p.sku, p.name, p.price]
let r = db.exec_many("insert into products (sku, name, price) values (?, ?, ?)", rows)
if is_err(r) { reply 400 `Not imported: ${r.error}` }
print(`${r.changes} products`)
```

**A `db.open` that fails as the program starts, its error taken by nothing, stops it**
*(since 0.7)*, with the reason: ``db.open: error connecting to server: Connection refused:
the program's database cannot be opened, so the program does not start``. That is
`db.open(...)` alone on a line, or given back by a function called so. It ran on SQLite in
memory before, answered writes, and lost them at the next start; a wrong password, a server
down or a damaged file did it. An error the program takes (`let r = db.open(...)`, `??`,
`| is_err`) is a value, as is one from a `db.open` later, which leaves the database as it
was.

**Opening another database while it runs.** `db.open` after the start replaces the
program's database. Open pages showing its rows are told at once, every tenant's (the
database is the program's, whichever request opened it), and ask again; a live call is
not answered from what was read in the old one.

**What the calls give.** `db.query` gives every row, all of it in memory (there is no
cursor: page with `limit` and keyset or `offset`); `db.one` the first row or `null` (on
PostgreSQL it fetches the whole answer and keeps one: write `limit 1`); `db.exec`
`{ changes, last_id }`. A row is a map from column names, so two columns of one name
(`select a.id, b.id`) leave the last: name them apart with `as`. SQL `NULL` is `null`.
The two numbers mean the same on every database. `changes` is the rows the statement
matched: an `update` that sets a value a row already has counts that row (MySQL's own
count, of rows changed, is `?client_found_rows=false` in its URL), an upsert's or a
`replace`'s row counts once, a read and a schema statement count 0, and a script's is its
last statement's. `last_id` is the id of the one row an `insert` made (on SQLite and
MySQL), and `null` after anything else: an `insert` of several rows, an upsert that
updated, an `update`. PostgreSQL has no such id, so it is always `null` there: write
`insert ... returning id` with `db.one`, or `db.query` for several rows. SQL with no
statement in it (`""`, only spaces or comments) is an error value, on all three, from
`db.exec`, `db.query` and `db.one` alike.

**One statement, or several.** A query (`db.query`, `db.one`), and `db.exec` given values,
run one statement: SQL with more after its first `;` is an error, on all three databases
(SQLite had run the first and ignored the rest until 0.8.1). Without values the whole
text runs, statement by statement (a seed file: `db.exec(read_file("seed.sql"))`), and
`changes` is the last one's. The text is split where the database would split it: a
function's, procedure's or trigger's body (`$$ ... $$`, `begin ... end`), a `do` block and
`begin atomic` are one statement with the rest, never a transaction begun by hand.
`db.exec` of a statement that gives rows (`insert ... returning id`) runs it, on SQLite
too; its rows are `db.one`'s and `db.query`'s.

**Parameters.** `?` takes values in order, as many as there are `?` (a different count
is an error, and so are placeholders given no values, on every database), or `:name`
takes them from one map (a name needs a value; other keys are ignored). One map is the
values by name only where the SQL has names: given to SQL with none, it is no value, so
a query put together from optional parts can always pass the same map; given for a lone
`?`, it is that value (a map stored as JSON). SQLite also reads `@name`, `$name` and
`#name`, and numbered ones (`?1`, `:1`), taking `name` from the map; on MySQL `@name` is a
variable of the session, so a map with that name is refused (`` `@x` is a variable of
MySQL's session (null unless set), not one of the values: write `:x` ``). `?NNN` is the
NNN-th value everywhere. PostgreSQL's `$1` turns the `?` translation off for that
statement, so `$1` and `?` do not mix; a `?` right after a word that is not SQL's
(`interval ?`, `date ?`) is taken for jsonb's operator and left alone: write
`?::interval`. PostgreSQL's `?|`, `?&` and `@?` are operators, `?||` is a value then
`||`, a `:name` inside brackets is a value unless it is a slice's (`a[1:n]`), and block
comments nest, as the server reads them. At most 32,766 values in a statement on SQLite,
65,535 on the others.

A value is what it is on every database: an error value given as a parameter is not
stored, but given back as the error it was (`value 2 cannot be stored: got error (it
failed before this call, ...)`), so `int("4O")` never becomes a row; a function is
refused. NaN and the infinities are refused, naming them (`value 1 is NaN, which cannot
be stored`), since SQLite keeps NaN as null and MySQL refuses both; `-0.0` is stored and
read as `0.0`, as SQLite and MySQL cannot keep it. A `||` joins text on all three: on
MariaDB, whose `||` is `or` by default, the driver sets `PIPES_AS_CONCAT` for SQL that has
one, migrations and scripts too.

**Bytes and other types.** There is no bytes value: a SQLite BLOB or a MySQL BLOB
comes back as text (bytes that are not UTF-8 replaced), and PostgreSQL's `bytea` as
`\x...` hex. Store base64 or hex text, or files beside the database.

**What failed.** `db.error_kind(e)` says what kind of failure a database's error value
is, the same for SQLite, PostgreSQL and MySQL, whose words for it differ: `"unique"` (a key
or unique column has the value already), `"foreign_key"`, `"not_null"`, `"check"`,
`"conflict"` (another transaction: the database busy, a deadlock, a serialization
failure), `"connection"` (the server ended the connection, or it could not be made:
`57P01` and class `08`) *(since 0.7)*, `"read_only"` (a write the database refused because
the transaction only reads, as a tool's that only reads is: `25006`, SQLite's "attempt to
write a readonly database", MySQL's 1792; FEEDBACK56.md, Q-858), or null for any other
failure and for anything else.
A transaction whose first statement finds that the server ended its pooled connection
while it sat idle, as after a restart or a failover, is begun again on another one: nothing
had run. It needs no effect.
Every database failure is an error value whose text starts `db: `, with the database's
own message (PostgreSQL's `detail` in brackets, then its SQLSTATE in square brackets:
`db: duplicate key value violates unique constraint "seats_pkey" (Key (k)=(1) already
exists.) [23505]`). `db.error_kind` reads PostgreSQL's code first (`23505`, `23503`,
`23502`, `23514`, and `40001`, `40P01` and `55P03` for a conflict), so it gives the same
word on a server whose messages are in German. MySQL's codes are in its text already.

```quillo
let r = db.exec("insert into seats (show, seat) values (?, ?)", show, seat)
if db.error_kind(r) == "unique" { return err("Someone has just taken that seat") }
r?
```

**Isolation and retries.** `db.transaction(options, f)` takes the isolation level and
how many times to run `f` again when the database gives up on it because of another
transaction:

```quillo
fn withdraw(account: int, cents: int) -> map with db =
    db.transaction({ isolation: "serializable", retries: 5 }, () => {
        let balance = db.one("select balance from accounts where id = ?", account).balance
        if balance < cents { return err("not enough money") }
        db.exec("update accounts set balance = balance - ? where id = ?", cents, account)?
    })
```

- Those are its two options; another is refused, when it runs and in `quillo check`
  (`` `db.transaction` has no option `isolaton` (did you mean `isolation`?) ``), where a
  misspelt key ran the transaction as if none were given.
- `isolation` is `"serializable"`, `"repeatable_read"` or `"read_committed"`; left out,
  it is the database's default (read committed on PostgreSQL, repeatable read on MySQL).
  PostgreSQL begins with `begin isolation level ...`, MySQL runs `set transaction
  isolation level ...` before `begin`. SQLite lets one transaction write at a time, so
  its transactions are serializable whatever is asked; with options it begins `begin
  immediate`, taking the write lock at once (another process holding it is then a
  conflict, tried again, rather than a failure half way). Without options a SQLite
  transaction is a savepoint that takes the lock when it first writes, and a busy
  database is not tried again.
- On PostgreSQL and MySQL a transaction waits up to 30 s for a free connection, then
  fails (`no PostgreSQL connection free after 30 s`).
- `retries` (0 to 20; 0 by default) runs the whole function again after a serialization
  failure or a deadlock: PostgreSQL's SQLSTATE 40001 and 40P01, MySQL's 1213 (deadlock),
  1205 (lock wait timeout), 1020 (a row changed since the snapshot, MariaDB) and 1412 (a
  table made since the transaction began), SQLite's busy database. Each try waits a little longer
  than the last (about 10 ms, doubling up to 320 ms, give or take half, so the two that
  collided do not collide again). Any other failure ends the transaction at once. Out of
  tries, the transaction gives the database's error value.
- After such a failure the transaction is over, whatever the function does with the
  error value: PostgreSQL refuses its next statements, and MySQL has already rolled it
  back (statements after it would otherwise run outside any transaction). So they fail
  at once, and a function that carries on anyway is not committed, but tried again.
- A transaction inside another is a savepoint, and cannot change the isolation: asking
  is an error. Only the outermost one tries again; give it the options.
- The function may run more than once, so it should do nothing that cannot be taken
  back: no `http.*`, `udp.send` or `channel.send` in it (send them after the
  transaction; `quillo check` warns), and global variables it changes are not rolled
  back. What it does in the database is.
- Under serializable isolation the database finds the conflicts itself: two
  withdrawals that read the same balance cannot both commit. Under read committed (or
  for a hot row, to wait instead of failing and trying again), lock what is read before
  writing it: `select balance from accounts where id = ? for update` (PostgreSQL,
  MySQL; SQLite has no `for update`, and needs none). A balance kept as a running total
  is updated in place: `update accounts set balance = balance - ? where id = ? and
  balance >= ?`, checking `changes`.

**Migrations.** `db.migrate("migrations")` (the folder is optional; that is its default)
brings the schema up to date from SQL files applied in name order: `001_todos.sql`,
`002_tags.sql`, and so on. Each file runs once, in its own transaction, and is recorded
in a table `_quillo_migrations` with its SHA-256. A file that fails is rolled back and
stops the program from starting, with the file's name and the database's message; a
recorded file whose text changed later gets a warning, not a second run. Servers that
start together take turns (an advisory lock on Postgres, an immediate transaction on
SQLite), so each file still runs once. Only files ending `.sql` directly in the folder
count (no subfolders); they sort by name as text, so `10_x.sql` comes before `2_x.sql`:
pad the numbers. The name is the key: a renamed file runs again. A file holds no
`begin` or `commit` of its own. The folder must exist: a missing one is an error that
says why it may be gone from a clone (`there is no such folder (git keeps no empty
folder: put the first migration in it, or a .gitkeep, which is not read)`). The table is `_quillo_migrations
(name, sha256, applied_at)`. Call it right after `db.open`:

```quillo
db.open(env("DATABASE_URL"))
db.migrate()
```

**SQLite** runs inside the server process, with foreign keys on and WAL mode, and waits
up to 5 s for a lock before a write is `"conflict"`. It is SQLite 3.46 with JSON, FTS5
full-text search and R*Tree, and without math functions (`sin`, `acos`, `radians`):
compute those in Quillo. Values: a column declared `bool` or `boolean` gives `true` and
`false`, other 0s and 1s are ints; `decimal(...)` and friends give decimals (see
[Decimals](#decimals)); booleans are stored as 0 and 1, lists and maps as JSON text;
dates are what text you store. There is one connection per database per process, and its
queries run under the interpreter's lock, on the loop's thread: a SQLite query holds up
the other requests of its instance while it runs. Several processes (a program started
with [`--instances`](#options-of-quillo-run) or [`--split`](#options-of-quillo-run), `quillo run main.qwe --instances 4`; another program) may share a file: readers do not wait, writers take turns.
There is no SQLCipher (no encryption at rest), and a file is opened by path, not as a
`file:` URI (so no read-only mode). `files()` and `quillo serve` never send a SQLite
file, whatever its name or folder (see [Static files](#static-files)); `file("app.db")`
sends one on purpose. Back it up with `db.exec("vacuum into
'backup.db'")` or `sqlite3 app.db ".backup ..."`, not by copying the file while it runs.

**PostgreSQL:**

- Placeholders can be `?`, `:name` (with a map) or Postgres's own `$1`; the first two
  are translated, skipping quotes (`U&"..."` names too), comments and `$$` strings, as
  the server reads them. Queries are prepared, so values never become SQL text.
- Values convert both ways: numbers, text, booleans, `json`/`jsonb` as maps and lists,
  arrays as lists (up to six dimensions), `numeric` as [decimals](#decimals) (an int
  when it has no scale), `money` as a decimal (with its `lc_monetary`'s decimals), `xid`
  and `xid8` as ints, `bit(n)` and `varbit` as their digits (`"00000101"`, kept at their
  length), and `inet`, `cidr`, `macaddr`, `uuid`, `date`, `time`, `timetz`,
  `timestamptz` (ISO 8601, in UTC, with `Z`), `timestamp` (as stored, with no zone),
  `interval` (PostgreSQL's own text, `1 mon 2 days 03:00:00`) and `pg_lsn` as strings,
  as PostgreSQL writes them. A BC date is ISO 8601's, `-0043-03-15` for 44 BC (`0000` is
  1 BC), and is written back as the same date. A `real` reads as the shortest float that
  prints the same (`0.1`, not `0.10000000149011612`). A type with no reader of its own
  (ranges, `hstore`, composites, `tsvector`, `regtype`) is its bytes in hex (`\x...`),
  the same way in every row: cast it in the query (`r::text`). An array whose lower
  bound is not 1 (`'[0:2]={1,2,3}'`) comes back as its text, which written back keeps
  its bounds, so a column's value may be a list in one row and text in another.
- JSON is read cell by cell by Quillo's own reader: a whole number past an int is a
  [decimal](#decimals), past a decimal a float, so two large ids stay two; a value that
  cannot be read (a number past a float, a nesting deeper than 128 levels) is an error value
  in its own cell (`db: this JSON could not be read: ...`), and the other cells and rows
  read; a lone surrogate is U+FFFD. A whole decimal inside a value sent is written as a
  JSON number, so it goes back as it came.
- Parameters are typed by the column: a `bool` takes only a boolean (not 1), the integer
  types take ints (or whole numbers and numeric text, range-checked: an int past the
  column's range is the server's own error, and compared with the column it matches no row), and other types are sent as text and cast.
  A list sent to an array column is an array, and a list inside a `jsonb[]`'s list is one
  JSON value. A text given as a `json` or `jsonb` column's value (an insert's values,
  `set doc = ?`) is the JSON it holds, as MySQL's JSON column takes it:
  `json.stringify(m)` stores an object, not a string; given to a function, it is a JSON
  string, as MySQL's `json_set` takes it. A float past `real`'s or `double precision`'s
  range is refused (`1e39 is out of range for type real`), where it was stored as
  Infinity or 0. A value where nothing in the SQL says its type (`select greatest(?,
  ?)`, `select ? as v`) is declared `int4`, `int8` or `float8` by what it is (text as
  text), so `greatest(10, 9)` is `10` and `select ? as v` with 5 is `5`, as on the others; when PostgreSQL refuses those types, only
  the values that refusal is about (a function's arguments, an operator's two sides) go as
  text, so `select upper(?) as u, ? as v` with 12345 and 5 gives `v` as `5`, as SQLite and
  MariaDB do; a statement is tried at most six times.
- `last_id` is `null`: use `insert ... returning id` with `db.one`.
- After a schema statement (`alter table`, `create or replace view`, a `drop`), the
  statements kept on what it names are prepared again, on every connection; one whose
  plan another server changed is prepared again and tried once.
- `?schema=` in the URL is added to `?options=`, not in its place.
- Connections come from a pool (8 by default; `?pool_size=N` in the URL) for
  transactions and migrations. A query releases the interpreter lock while it waits,
  so other requests keep running. A transaction keeps one connection until it ends, so
  other requests never see its changes early.
- **The session is the transaction's** *(since 0.7)*. A pooled connection goes from one
  request to another, so what a statement changes beyond its transaction would reach the
  next visitor: `set` (without `local`), `set_config(..., false)`, a `create temp` table,
  `listen`, a session's advisory lock, a cursor `with hold`, `set session authorization`.
  Every transaction's connection (`db.transaction`, `db.atomic` and migrations alike) is
  reset as it ends, before it goes back (`set session authorization default`, `reset
  all`, `close all`, `discard temp`, advisory locks let go, `unlisten *`; Quillo's
  prepared statements kept), or closed if the reset fails. Outside one, where the
  connection is shared by every request, such a statement is refused, in any form and
  place: `set_config` with values, inside `values (...)` or an `update`, and whatever its
  case or quoting, unless its third argument is `true`; a `pg_temp` table; and a `begin`
  anywhere in a script. A `do` block, and a call of a function of the database's own
  (which may change the session where Quillo cannot read it), runs outside a transaction
  on a connection of its own, reset after it. `prepare` is refused everywhere: pass values as
  parameters (`?`), which Quillo prepares itself. Inside a SQLite transaction a PostgreSQL
  query takes a free connection at once or is an error, as waiting would hold every request. Row-level
  security's setting per request is therefore written inside the transaction, as local:

  ```quillo
  db.transaction(() => {
      db.one("select set_config('app.user_id', ?, true)", me().id)   // true: this transaction's
      db.query("select * from orders")                                 // what its policies allow
  })
  ```

  A policy that reads it can take `coalesce(nullif(current_setting('app.user_id', true), ''), ...)`:
  after a `set local`, PostgreSQL reads the setting back as `""` in the connection's next
  transaction, not null.
- In the server, other queries share a second set of connections, pipelined (four,
  more while some wait on slow queries, up to `pool_size`): a request's queries (its
  `parallel` calls' too, outside a transaction) go to the same one, so the database
  reads them together. Size `max_connections` for twice `pool_size` per server and
  instance, and a few more for Quillo's own tables. Idle connections stay open; a
  connection that drops is opened again on the next query, and the query that met the
  drop fails.
- Prepared statements are kept, 256 per connection. There is no `COPY`, `LISTEN` or
  `NOTIFY`: bulk inserts are many-row `values (?, ?), (?, ?)` or `unnest(?::int[],
  ?::text[])` with lists. A Unix socket is `?host=/var/run/postgresql`. A query's time
  limit, or a read-only session, is the server's: `?options=-c%20statement_timeout%3D5000`
  in the URL, or `alter role`.
- Special characters in the user or password are percent-encoded in the URL (`%40` for
  `@`).
- TLS is required for a server on another machine, with its certificate and name
  checked; on this machine (`localhost`, `127.0.0.1`, `::1`, a Unix socket) it is used
  when the server offers it. The URL can say otherwise, as libpq's do: see
  [TLS to the database](#tls-to-the-database) below. Before, TLS was only preferred
  everywhere: a URL to another machine whose server has no TLS now needs
  `?sslmode=disable` to connect.

**MySQL and MariaDB** (the `mysql` feature, built in by default; `cargo build
--no-default-features` leaves it out):

- Placeholders are `?`, or `:name` with a map (translated, skipping quotes, backticks
  and comments: `#` lines, and `/*! */` and MariaDB's `/*M! */`, which the server runs,
  read as SQL). Statements are prepared, so values never become SQL text.
- Values convert both ways: numbers, text, `tinyint(1)` as booleans when it holds 0 or 1
  (another value is an int), `decimal` as [decimals](#decimals), MySQL's `json` as maps
  and lists (MariaDB's JSON is text: `json.parse` it), `BIT(n)` as its digits
  (`"00000101"`, as PostgreSQL's `bit`), which it takes back, and dates and times as
  text (`2026-10-27 14:30:00`, with a space and no zone; `TIME` as `HH:MM:SS`, hours past
  24 too). A `FLOAT` reads as the shortest float that prints the same, and a value a
  `FLOAT` column would keep as 0 (`1e-46`) is refused as out of its range. An unsigned
  value or an id past 2^63 is a [decimal](#decimals) (`9223372036854775808m`), which finds
  its own row. `last_id` is the new row's `auto_increment` id (see above for several
  rows).
- `db.open` of a database that is not there is an error (`Unknown database 'shop'`), as
  on PostgreSQL, so a typo in a URL makes no empty database; a tenant's is made by the
  driver (below). A host may be an IPv6 address, `mysql://app@[::1]:3306/app`.
- A statement larger than the server's `max_allowed_packet`, its values included, is
  refused, saying so and how big it is, where the connection broke.
- Inside `db.transaction` and `db.exec_many`, a statement that commits by itself
  (`truncate`, `create table`, `alter`, `drop`, ...) is refused before it runs, so what
  ran before it can still be rolled back: `` `db.exec("truncate table rates")` inside a
  transaction: MySQL and MariaDB commit the transaction around this statement by itself
  ... run it before or after the transaction ``. One that cannot be seen beforehand (a
  procedure's, `execute immediate`, a `begin not atomic` block) ends the transaction with
  the same error, read from the server's in-transaction flag. PostgreSQL and SQLite roll
  their schema statements back, and take them in a transaction.
- Connections come from the driver's pool (10 to 100), and a query releases the
  interpreter lock while it waits, as with Postgres. `pool_size` is PostgreSQL's: in a
  MySQL URL it stops the database from opening; the driver's own `pool_min` and
  `pool_max` set the pool. A transaction keeps one connection until it ends; nested
  ones are savepoints. A Unix socket is `?socket=/path`.
- `db.migrate()` takes turns through a named lock (`get_lock`). A migration may change
  the schema, but such a statement commits at once in MySQL (outside the refusal above),
  so a migration that fails part way keeps what it did before the failure: put one
  change in each file.
- A database per tenant: `mysql://host/app_{tenant}`, made when the program makes the
  tenant, and listed by `db.tenants()`.
- TLS is required for a server on another machine, with its certificate and name
  checked; on this machine (`localhost`, `127.0.0.1`, `::1`, `?socket=...`) it is not
  used unless the URL asks for it. See [TLS to the database](#tls-to-the-database)
  below. Before, TLS was off unless `require_ssl=true`.

#### TLS to the database

Some hosted databases sign their certificates with a certificate authority (CA) of
their own: Amazon RDS and Aurora, Google Cloud SQL (the instance's `server-ca.pem`), and
a company's servers with its private CA. The public CAs that are trusted by default do not know those, so the
connection fails with `invalid peer certificate: UnknownIssuer` until the URL names
the CA's certificates (a PEM file, as the service publishes it):

```quillo
// Amazon RDS and Aurora: their CAs' bundle, saved where the program runs, from
// https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem
db.open("postgres://app@mydb.abc123.eu-west-1.rds.amazonaws.com:5432/app?sslrootcert=/srv/app/global-bundle.pem")
db.open("mysql://app@mydb.abc123.eu-west-1.rds.amazonaws.com:3306/app?ssl_ca=/srv/app/global-bundle.pem")
// Or in $DATABASE_URL, with the password: postgres://app:...@...rds.amazonaws.com/app?sslrootcert=...
```

PostgreSQL reads the options libpq does:

| In the URL | TLS | The server's certificate |
|---|---|---|
| `sslmode=verify-full` (the default for another machine) | always | signed by a trusted CA, for the host's name |
| `sslmode=verify-ca` | always | signed by a CA in `sslrootcert`, for any name |
| `sslmode=require` | always | not checked, unless `sslrootcert` is given (then as `verify-ca`) |
| `sslmode=prefer` (the default on this machine) | when the server offers it | as with `require` |
| `sslmode=disable` | never | |

- `sslrootcert=/path/ca.pem`: the CAs to trust, in PEM, instead of the public ones.
  `sslrootcert=system` trusts the operating system's CAs (with `verify-full` only).
- `sslcert=/path/client.crt` and `sslkey=/path/client.key`: a certificate and its key
  (PEM), for a server that asks for one (`hostssl ... cert` in `pg_hba.conf`).
- `require` keeps the connection from being read, but not someone in the middle from
  posing as the server, who then has the queries and the data. Use it only where the
  network is yours; `verify-ca` with the CA's certificate costs no more. `prefer` is
  weaker still: someone in the middle can say the server has no TLS.
- `verify-ca` without `sslrootcert` is refused: any certificate from a public CA, for
  anyone's site, would pass. Where the server is reached at an address its certificate
  does not name, `verify-full` with the name in `host` and the address in `hostaddr`
  (`postgres://db.internal/app?hostaddr=10.0.0.5`) checks the name too.

MySQL and MariaDB take the MySQL driver's options:

- `ssl_ca=/path/ca.pem`: the CAs to trust instead of the public ones (with
  `built_in_roots=true`, as well as them). It asks for TLS on this machine too.
- `ssl_server_name=db.example.com`: the name the certificate is checked for, where the
  host connected to is an address or a tunnel. `verify_identity=false` alone is
  refused, since the driver checks the name regardless; with `verify_ca=false` too,
  nothing is checked.
- `verify_ca=false`: the certificate is not checked at all, which has the risk of
  Postgres's `require` above.
- `ssl_cert=/path/client.crt` and `ssl_key=/path/client.key`: a client certificate
  (`REQUIRE X509`).
- `require_ssl=true` asks for TLS on this machine; `require_ssl=false` turns it off.

### Several databases

`db` is the program's database. `db.connect(target)` opens another, with the same
functions, and its own transactions, migrations and live updates:

```quillo
db.open("app.db")
let analytics = db.connect(env("ANALYTICS_URL"))           // any kind: SQLite, Postgres, MySQL
analytics.migrate("migrations/analytics")
analytics.query("select day, sum(total) from sales group by day")
db.transaction(() => {
    db.exec("insert into orders ...")
    analytics.transaction(() => analytics.exec("insert into sales ..."))   // its own
})
```

- Call its functions on it, `analytics.query(...)` (or `sql | analytics.one`): that is
  how they know which database. A connection has `exec`, `query`, `one`, `transaction`,
  `migrate`, `tenants`, `tenant_exists` and `sql_name`; `error_kind`, `connect`, `system`, `shards`
  and `atomic` are `db`'s. Connect at the top of the program; the same target
  again is the same connection, not a new pool. A SQLite file is the same file by any
  spelling: `db.connect("./analytics.db")`, `"sub/../analytics.db"` and the full path
  are one connection.
- Its calls need the `db` effect, as `db`'s do (see [Effects](#effects)).
- A transaction is its own database's: one inside another on a different database
  commits or rolls back on its own.
- Live data tells them apart: a page showing `analytics`' `orders` does not refresh
  when `db`'s `orders` changes. Its topics carry a label, not the URL: a SQLite file's
  real path (from the program's folder when it is inside it), or a server's scheme,
  host, port and database, without the user, the password or the options. So two
  servers on one database agree on its topics, during a password's rotation too, and a
  page reading through one connection hears a write made through another. (Another
  user or other options on a server URL is another pool, with the same topics.)
- **Names for this database.** `analytics.sql_name(col)` quotes a name as that database
  reads it, `` `col` `` on MySQL, `"col"` elsewhere; `db.sql_name(col)` *(since 0.7)* does the same for
  the program's database. Plain `sql_name` quotes for the program's database only, so a
  program with MySQL beside SQLite or PostgreSQL uses the connection's: MySQL reads
  `"title"` as text, and `select "title" from items` gives the word `title` in every row.
  None of them needs an effect, and `quillo check` takes SQL built with them as safe.

```quillo
db.open("app.db")
let analytics = db.connect("analytics.db")
print(analytics.sql_name("total"), db.sql_name(["day", "total"]))
print(db.connect("./analytics.db") == analytics)
```

```
"total" "day", "total"
true
```
- A connection never reaches the browser (its URL may hold a password).

Mounted programs and services have their own `db` too, so one Quillo can run programs
on different databases side by side.

**Read replicas.** A PostgreSQL or MySQL database's replicas take its reads:

```quillo
db.open(env("DATABASE_URL"), { replicas: [env("REPLICA_1"), env("REPLICA_2")] })
```

- Reads (`select`, and `with` that writes nothing) go to the replicas in turn. Writes,
  reads that lock (`for update`, `for share`), anything in a transaction, and every
  read of a request after it wrote go to the primary: a request reads what it wrote.
  Only pure reads go to a replica: a `select` that calls a function that may write
  (`nextval`, `setval`, or one made in the database with `create function`, on
  PostgreSQL and MySQL alike) goes to the primary; built-in functions that only compute
  (`lower`) do not. A replica's read-only refusal sends that statement to the primary
  without taking the replica out of use.
- Every replica must open when the database does, or `db.open` fails. A replica that
  fails later is left out for ten seconds; its read goes to the primary. Replicas are
  for a database named by its URL: not SQLite, not a database per tenant.
- Another request may still read from a replica that has not caught up with a write
  made just before. Reads that cannot wait for that belong in a transaction.
- `db.connect(url, { replicas: [...] })` too.

**Shards.** Data split over several databases by a key (a customer, an account):

```quillo
let users = db.shards([env("SHARD_1"), env("SHARD_2"), env("SHARD_3")])
users.migrate()                                           // each one
users.on(id).exec("insert into users (id, name) values (?, ?)", id, name)
users.on(id).one("select * from users where id = ?", id)   // the shard for the key
users.all("select count(*) as n from users")               // every shard's rows, together
```

- `.on(key)` is a connection, as `db.connect` gives (its own transactions too). Keys
  are spread by jump consistent hashing of their text (so `1` and `"1"` are one key),
  the same on every server: a shard added at the end takes its share of keys from each
  of the others and moves no other key (those are the rows to copy over).
- `.all(sql, ...)` asks the shards one after another, and stops at the first that
  fails; an aggregate comes back once per shard. `.count` is how many there are.
  `query`, `one`, `exec` and `transaction` on the shards themselves are an error.
- Shards may be any kind of database, and mixed.

**Joining across databases.** One SQL query cannot join tables of two databases;
`join_on` puts their rows together in the program:

```quillo
let orders = db.query("select * from orders where day = ?", today)
let people = accounts.query("select * from users where id = any(?)", orders | map((o) => o.user_id))
join_on(orders, people, "user_id", "id")      // each order with `user`: its user, or null
```

`join_on(rows, others, key, other_key = key, as)` gives each row its first match (same
value, same type) under `as`, which is `key` without `_id` (`user`), else `joined`;
`| filter((o) => o.user != null)` keeps those with one.

**One transaction over several databases.** `db.atomic` keeps all of it, or none:

```quillo
db.atomic([db, ledger], () => {
    db.exec("update accounts set balance = balance - ? where id = ?", n, from)
    ledger.exec("insert into entries (account, amount) values (?, ?)", from, -n)
})
```

- The list names the databases that take part: `db`, connections from `db.connect`, a
  shard (`users.on(id)`), or shards whole (`users`: each of them). A database the
  function uses that is not in the list is not part of it: its writes stay when the
  rest rolls back.
- It commits in two phases: each PostgreSQL and MySQL database prepares its part
  (prepared transactions, XA), the decision to commit is recorded with Quillo's own
  tables, a SQLite database (one at most) commits, and the prepared parts commit. A part
  that cannot prepare (a deferred constraint, a database gone) rolls every part back,
  as does a function that fails, returns an error value or replies early.
- A server that stops part way leaves prepared parts, which hold their locks:
  recovery finishes them a minute or so later (it looks every 30 s for parts over a
  minute old, from any server of the program), committing those decided and rolling
  back the others. The first to record a decision for a transaction, it or recovery,
  decides it. Recovery looks at the program's database and its connections, not at a
  database per tenant. At most one SQLite database may take part (naming two is a
  runtime error).
- PostgreSQL needs `max_prepared_transactions` set above 0 (it is 0 by default).
- A SQLite database taking part begins last, once every server database's part has begun;
  from then until the end no other request runs, as in a SQLite `db.transaction`, so no other
  request's write lands in it (FEEDBACK56.md, found in review).
- `db.transaction` inside it nests as usual; `db.atomic` cannot begin inside a
  transaction on one of its databases.

### Quillo's own tables

Scheduled jobs (`_quillo_jobs`: each run once, whichever server it is due on) and the
decisions of `db.atomic` (`_quillo_atomic`) are kept in the program's database. When
there is a database per tenant they cannot be, and go to the system database instead:
`db.system(target)` (a database every server of the program shares, for several servers),
`$QUILLO_SYSTEM_DB`, or else a SQLite file `quillo-system.db` next to the program.
`db.system` also keeps them apart from the program's own tables when there is one
database.

What must commit with the work is kept with the work, in the program's database, or the
tenant's when there is one per tenant, whatever `db.system` says: the answers of
[idempotent requests](#idempotent-requests) (`_quillo_idempotency`) and calls made later
(`_quillo_later`, with the answers of those finished in `_quillo_later_done`). A call made
with no database to go with goes to the system database, so that it outlives a restart;
a program with no database at all has `quillo-system.db` for that (its jobs need no
record: one server runs them). With a database per tenant, the system database notes
which tenants have calls (`_quillo_later_tenants`), as nothing else says where to look.

The migrations applied (`_quillo_migrations`) are in the program's database. The calls a
service answered (`_quillo_calls`) go with the jobs. `quillo-system.db` is made only when
something must be kept in it (calls made later, `db.atomic`'s decisions);
`$QUILLO_SYSTEM_DB` may be a URL.

### Many tenants

One program can serve many tenants (customers, teams, sites), each with its own data:

```quillo
tenant_from("example.com")          // acme.example.com is tenant "acme"
db.open("data/{tenant}.db")         // a SQLite file per tenant
db.migrate()                        // run for each tenant's database when it is made

route GET /signup =                 // at www.example.com, which needs no database
    <form method="post" action="/signup">
        <input name="name" pattern="[a-z0-9][a-z0-9_-]*" placeholder="acme" required/>.example.com
        <button>Make my site</button>
    </form>

route POST /signup { form: { name: str } } with db {
    let made = new_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))   // makes acme.db
    if is_err(made) { reply 409 "That name is taken" }
    redirect(`https://${name}.example.com/`)
}
route GET / = <h1>Welcome, {tenant()}</h1>
```

Sent `acme`, the form makes `data/acme.db` (with `migrations/`'s tables) and goes to
`https://acme.example.com/`, which says `Welcome, acme`. Sent `acme` again, it shows
`That name is taken`, with status 409.

- `tenant_from("example.com")` takes the part of the host before the domain;
  `tenant_from((request) => ...)` takes what the function gives for the request (a
  header, a path segment, a lookup), a name or null. `tenant()` is the current one, or
  null. A tenant's name is lowercase letters, digits, `-` and `_` (at most 63), starting
  with a letter or a digit; anything else is no tenant, so a name is always safe in a
  file name or a database name.
- A function as the target says where each tenant's database is, so tenants can be on
  different kinds of database. It is asked once for each tenant, when first used; null
  means there is no such tenant. It cannot use the database it picks for (keep the list
  in another):

  ```quillo
  let registry = db.connect("registry.db")
  db.open((tenant) => registry.one("select url from tenants where name = ?", tenant)?.url)
  ```
- A `db.open` target with `{tenant}` in it is a database per tenant: a SQLite file each,
  a PostgreSQL schema each in one database with `?schema=t_{tenant}`, or a database each
  with `postgres://host/app_{tenant}` (made by you beforehand), or a MySQL database each
  with `mysql://host/app_{tenant}` (made by Quillo). Give schemas a prefix:
  `public`, `information_schema` and `pg_...` are refused, and a prefix keeps a tenant
  named like another schema of the database (`auth`, say) out of it. A tenant's
  connections search its schema alone, so what is in `public` (an extension's functions)
  is named with it: `public.gen_random_uuid()`. `?schema=app` without `{tenant}` keeps
  one program's tables in a schema of its own.
- `db.tenants()` lists the SQLite files, the PostgreSQL schemas or the MySQL databases
  there are; for `postgres://host/app_{tenant}`, and for a function, it is an error:
  keep the list yourself.
- `new_tenant(name, f)` *(since 0.7)* is for signing up: it makes the tenant's database, migrates it and
  runs `f` for it, or gives an error when that tenant exists, so a second signup for one
  name cannot land in a customer's data. It is one step on the database, so of two signups
  at once one makes it. Names such as `www` and `admin` are the program's to refuse.
- A request's host can name any tenant, so requests only open databases that exist, and
  make no file or schema. A request for a made-up `anything.example.com` that needs its
  database answers 404, unless the route answers for itself; one that needs no database
  answers as usual, so `www.example.com` can be a page of its own.
- Each open tenant has its own connections (a pool and its shared ones, as above), so
  give PostgreSQL a small `pool_size`. At most 1,024 SQLite tenants' databases, and 16
  PostgreSQL or MySQL ones, are open at once, the least recently used closed first;
  others are opened again when needed, their migrations run on each opening, and past
  the limit each request connects again: 3 ms against 0.2 ms on one machine, more over
  TLS. `?open_tenants=64` in the URL keeps that many open, at `pool_size` connections
  each: leave room in the server's `max_connections`. Connections are never shared
  between tenants, as they keep statements prepared against one schema. Code that runs
  for no tenant, such as a request to the bare domain, gets an error from `db`.
- Live updates stay within a tenant: a change to one tenant's table wakes only that
  tenant's pages, even when the tenants share one database, and a write made for no
  tenant (a job, a script) wakes none. A page follows only its own tenant's topics and
  those of no tenant: one naming `acme@db:orders` from evil's host is not told of acme's
  orders *(since 0.7)*. Topics are not checked further: within a tenant, any page may
  follow any topic it can name.
- Channels and media streams are each tenant's own, unless `tenants: "shared"` (see
  [Channels](#channels) and [Media](#media)).
- `with_tenant("acme", () => ...)` runs code for a tenant, making its database if there is
  none: outside a request, in a script or a scheduled job.
- What stays shared: global variables, one copy for the whole process, so keep a
  tenant's data in its database (or in maps keyed by `tenant()`). `quillo check` warns on a
  function or route that writes a global in a program with `tenant_from`. Views that call
  `tenant()` or read the database are never served from the view cache. Cookies
  belong to the host they were set on, so each subdomain has its own.

### HTTP client

```quillo
let r = http.get("https://api.example.com/users/1")        // { status, ok, headers, body, json }
http.post(url, { name: "Ada" })                            // maps and lists are sent as JSON
http.request("PUT", url, body, { authorization: `Bearer ${token}` })
```

Every HTTP status is a response; failing to connect is an error value, and so is a URL
that is not `http://` or `https://`. The functions: `http.get(url, headers, options)`,
`http.delete(url, headers, options)`, `http.post(url, body, headers, options)`,
`http.put` and `http.patch` as `post`, and `http.request(method, url, body, headers,
options)`. The answer is `{ status, ok, headers, body, json, cookies }`: `ok` for a 2xx,
header names in lower case, a header sent more than once joined with `, ` (two `Link`s are
one `</x>; rel=a, </y>; rel=b`), `set-cookie`'s lines joined by a line break, as they
cannot be by a comma, and also given one by one in `cookies`, a list; `body` as text
(bytes that are not UTF-8 replaced), `json` the body parsed, or `null`. A body longer than
64 MiB is an error value that says to read it with `http.stream` *(since 0.7: it was cut
there, and cut JSON was `null`)*.
Interim answers (`100 Continue`, `103 Early Hints`) are passed over to the final one. A
body in `gzip`, `deflate` or `br` is decoded; another coding is an error value that names
it (`the answer is in a coding Quillo cannot read: content-encoding zstd`). Requests say `User-Agent: quillo/<version>` unless
the headers give another; some APIs (OpenStreetMap's Nominatim) refuse that one, so
name your program. `http.stream(url, f, opts)` reads an answer as it arrives (see [Streaming](#streaming)).
Text is sent as `text/plain`, HTML as `text/html`, other values (numbers and decimals
too) as JSON, unless the headers give a `content-type`.

Options come after the headers (in `http.stream`, in its map):

```quillo
http.get(url, {}, { redirects: 0 })                 // a redirect is the answer, not followed
http.post(url, body, {}, { public_only: true })     // only addresses on the public internet
http.get(`https://api.example.com/rates`, { authorization: key }, { cache: 60 })
http.get(url, {}, { timeout: 5 })                   // an error value after 5 seconds
```

- `redirects`: how many to follow, from 0 to 20 (5 unless given). A 301, 302 or 303 after
  a `POST` is followed as a `GET` without the body, as browsers do; a 307 or 308 after a
  write is followed with the same method and body. `Authorization` goes on to the same
  host, and `Authorization` and `Cookie` are not sent on to another origin.
- `public_only: true` connects only to the public internet: not this machine, a private
  network, or a cloud's metadata service (`169.254.169.254`), however the address is
  written, checked on the address the name gives as it connects (and again for each
  redirect). Use it whenever someone else may choose the URL. `quillo check` warns about
  a call whose host comes from a value (a parameter, `request`, text put together before
  the host ends) unless its options say `public_only`, either way: `false` says the call
  is meant to reach inside.
- Under a policy (see [The policy](#the-policy)), `[http] hosts` lists the hosts any
  call may reach, and `private = false` keeps every call off this machine and its
  networks, as `public_only` does, whatever the call's options. Each redirect is judged
  too, before it is followed. A call refused is an error value: `http: example.com is not
  among the hosts quillo.policy allows: api.stripe.com, *.mapbox.com`.
- `cache: seconds` (a GET's, up to a day) keeps a successful answer and gives it again to
  the same call, the same URL, headers and options, from any request, until then. Calls
  to a site that limits how often it is asked, or that charges for each, then go out once
  for everyone: `cache: 60` on the rates above asks once a minute however many pages
  show them. Failures are not kept, nor answers over 16 MiB. At most 64 MiB of answers
  are, those that end soonest going first. On a method other than GET, and in
  `http.stream`, `cache` is an error.
- `timeout: seconds` (more than 0, up to 300; `0.5` too) is how long the whole call may
  take: then it gives an error value (`... took more than 5 s (its timeout)`). Unless given,
  a call has 10 seconds to connect and 60 in all; `http.stream`, 300 seconds of silence
  between pieces. A page that waits on a slow service waits that long.

- `ca`: the certificate authorities to trust instead of the usual public ones, a PEM file
  (named from the program's folder) or `"system"`, the machine's own store: for an
  internal service under a company's CA.
- `cert` and `key`: a client certificate and its key, PEM files, presented to the server,
  as some banks' and governments' APIs ask (mutual TLS). One without the other is an error
  (`` `cert` and `key` go together ``), and so is a file with no certificate in it.

```quillo
http.get("https://ledger.internal/balance", {}, { ca: "certs/internal-ca.pem" })
http.post("https://api.bank.example/payments", order, {}, { cert: "certs/client.pem", key: "certs/client.key" })
```

Another option is an error, in `quillo check` when the options are written in the call,
and when it runs: `{ timout: 5 }` is not ignored.

Calls go through the proxy `HTTPS_PROXY` names for `https://` and `HTTP_PROXY` for
`http://` (in either case, `ALL_PROXY` for both), unless `NO_PROXY` names the host (a
name, `.example.com` or `example.com` for it and those under it, an address, or `*`).
This machine (`localhost`, a name under `.localhost`, a loopback address) is never reached
through one. With a proxy, the proxy looks the name up, so `public_only` checks the host
before the call is made.

In a component, the same calls go from the page (see [What the browser can reach](#what-the-browser-can-reach)):
`redirects: 0` makes a redirect an error value there, `timeout` is the same, and
`public_only` and `cache` are the server's. A service that needs the visitor's address, called from the server, can be
given `request.ip`.

## Checking

`quillo check app.qwe` finds mistakes without running the program, and `quillo run`
does the same first. Types are sets of possible types, and code without annotations
is `any`, so only definite mistakes are reported, and annotations make the checks
stricter. Its types are sets of twelve kinds (`null bool int float dec str list map fn
html response error`), lists with what they hold and maps with their fields. It reports
unknown names (with suggestions), names used before they are
defined, names defined twice, names a file did not take, wrong argument counts and types (for builtins too), operators that can never
work, division by a written zero, return and error types, effects, view and component props,
component code that cannot run in a browser, route mistakes, and calls like `last(xs)`
where a variable of yours named `last`, known not to be a function, hides the builtin.
Also: a parameter, a record's field, or a local `fn` or `state` named twice; a pattern's
type the program has not (`Usr`); a `let` or a `match` that cannot match what it is given
(`let [a, b] = [1]`, `match x { }`); a lambda called with the wrong number of arguments,
by its `let` name or given to `map`, `filter`, `reduce`, `sort_by` and the like; a list
whose item is not what `list<T>` takes (`[1, "a"]` for `list<int>`); a field `request`
does not have (`request.qurey`); top-level code calling a function that reads a
global defined further down; a name bound twice in one pattern (`[x, x]`), a name two
tested fields bind used (`{ user.id: int, order.id: int } => id`), and a route binding
one name by two of its path, query and request patterns; and `every` and `schedule` as
the start reads them, their written-out arguments (`every(0, f)`, a cron field `60`,
`@reboot`) and a job or tick named twice where both calls surely run (see [Scheduled
jobs](#scheduled-jobs)).
When a call to a function of your own named like a builtin (`notify`, `first`) has the
wrong arguments, the error says so: this `notify` is the program's own, from line 1,
which hides the builtin `notify`.

Its warnings: an unused variable (name it `_x` to keep it); a function named like a
builtin, or a variable named like a module; a line starting `-x` that does nothing;
`||`, `&&`, `!`, `if` or `? :` on a value known to be a string or a number; SQL put
together from values, and an `http.*` host from a value (see [Standard
library](#standard-library)); a `/` result given to an `int` parameter; a helper with no
`with` that may use an effect its `with` caller lacks; `http.*` or `net` inside a
transaction that retries; a live loader fed only by a service; a class a sheet styles
that nothing writes; a `.qwer` function that cannot be exported; `sleep` given a number
written between 0 and 1 (`sleep` takes milliseconds: `` `sleep(0.2)` waits 0.2 ms; for 0.2
seconds write `sleep(200)` ``); `c.query(...)` on an untyped parameter in a `with`
function without `db`; a service whose file is not here; a name in a pattern that takes
any value and goes unused, or reads like a type (`intt =>`); an arm that never runs
after one that takes any value, or with a value an arm before it has; `xs.push(2)`,
`sort(xs)` or `i + 1` alone, which make a value and drop it; a `while` whose condition
nothing in it changes; `"Hello ${name}"` in a plain string; `index_of(...) == -1`, a
function used as a condition (`if ready {`), and a path's untyped parameter added to a
number; a misspelt attribute or event (`clas`, `@clik`), `className`, and `<card>` for a
view `Card`; a name imported as a builtin's; a key written twice in a map
(`{ a: 1, b: 2, a: 3 }`, `{ a, "a": 2 }`: the last value is the one kept; a spread
before it is no second key); a function written in a route's answer, which cannot be
JSON (`` this route's answer holds a function at `f` ``), the program's own or one the
route's block makes (`fn helper() = 2`, `let f = (x) => x`; a call of it is a value); a job or tick named twice where
both calls may run. Warnings never make
it fail.

[`quillo check --policy FILE`](#other-commands) (`quillo check main.qwe --policy host.toml`)
checks the program as it would run under that policy (and
the host's and the program's own, which it always reads): it names the files first
(`quillo: policy host.toml + quillo.policy (effects: db, http, time; no builds; no
private addresses)`), then what each would refuse, as errors: a builtin needing an
effect the policy does not allow (`` `udp.listen` needs the net effect, which host.toml
does not allow ``), a plugin it does not list, a Rust file with no `//! effects:` line
under a policy that narrows effects. See [The policy](#the-policy).

What it does not find, so a reader is not surprised: an unknown type name in an
annotation (`u: Usr`, when no `type Usr` exists; it fails at the first call), division
by a variable that is zero, `==` between values that can never be equal, a `match`
that misses a case (it fails when the value comes), and component code that reaches the
database through a helper between them (`quillo run` refuses that before it serves).

`quillo check` writes its findings to stderr as `file:line: error: ...` and `file:line:
warning: ...`; with errors it ends `quillo: N errors` and exits 1; else it writes
`main.qwe: ok (12 routes, 40 functions, 9 components, in 18 files)` to stdout (with the
warnings counted) and exits 0. Its two options, given after the program as `quillo run`'s are,
are [`--strict-null`](#other-commands) (below) and `--policy FILE` (above).

After a syntax error that a newer Quillo might accept (an unknown character or escape,
an unexpected keyword, an unknown word), `quillo check`, `quillo run` and the language
server say which quillo they are (`note: this is quillo 0.6.0 (f1695a3, 2026-10-29),
built 2026-10-27 at ...`): code written for a newer Quillo fails to parse in an older
one, such as an old build left on the `PATH`. `quillo --version` names the commit too: `QUILLO_COMMIT` when
the build sets it (as for a source archive, which has no git), else the checkout's.
`scripts/doctor.sh` and `doctor.ps1` point out a `quillo` on the `PATH` older than the
checkout's own build.

It also knows what lists hold and which fields maps have, from what made them: a list
or map literal, an annotation, `split`, `range`, `merge`, `set`. A lambda given to
`map`, `filter`, `find`, `sort_by`, `reduce` and the like takes what the list holds, and
`for x in xs` too, so `[1, 2] | map((x) => x.name)` is an error; `map` gives a list of
what its function returns, `first` an item or null. A map whose fields are known has no
others: `{ title: "Hi" }.titel` is an error (`did you mean title?`), since a missing key
would read as null and nothing would fail. Maps that come from elsewhere (the database,
JSON, a parameter without an annotation) and `{}` are open: any field may be there.

`quillo check --strict-null app.qwe` also warns where a value may be null and null does
not work: an argument `str | null` for a `str` parameter, `count + 1` with `count: int |
null`, a `-> str` function that may return null. Checks narrow the type, as in
TypeScript: after `if x == null { return ... }`, in `x != null ? f(x) : ""`, `x != null &&
f(x)`, `if x { ... }`, and in the arms of a `match` after `null =>`. As in TypeScript,
`xs[i]` is an item, not "or null" past the end. A parameter with a default (`page: int =
1`) is null only if its default may be. Editors get the same warnings with the
language server's `initializationOptions: { "strictNull": true }`.

## Concurrency and reloading

One thread runs an event loop (kqueue on macOS, epoll on Linux) over every connection,
reads and parses requests, runs the program on them and writes the answers. Each request
runs as a coroutine with its own stack. A handler runs on its own from start to end,
except while it waits: a database query, `http.*` or `sleep` pauses it, the waiting is
done elsewhere (on a pool thread, or for PostgreSQL on connections all requests share,
with queries pipelined, and a `sleep` of 5 ms or more on a timer, holding no thread),
and the loop answers other requests meanwhile. Code is written
as if it simply waited; there is no `async` or `await`. Updating shared state such as
`todos = todos | push(t)` is safe without locks, since only one handler runs at a time.

A request that only computes never leaves the loop's thread, so plain pages and JSON
cost a few microseconds each. To use more cores, start the program with
[`--instances N`](#options-of-quillo-run), `quillo run main.qwe --instances 4`, which runs the program N times
over in one process, each with an event loop of its own; the first takes the connections
and hands them out in turn. Like N processes, each instance has its own variables, while
the database is shared, and changes to it (and `publish`) reach live pages on every
instance. SQLite in memory (`:memory:`) is one database per instance, so use a file or
PostgreSQL to share data between them.

What holds up the loop is code that computes for long without waiting: a loop over a
million rows keeps every other request on its instance waiting until it ends. Quillo does
not break into it, since that would end the promise above (a handler runs on its own
between waits). Images, `crypto.hash_password`, files and Rust (`.qwer`) functions run off
the loop, and sorting and `stats`, `vec` and `math` on large lists are spread over the
cores. So put long computing in Rust (see [Rust](#rust)), or run more instances: with
several, connections are taken on a thread of their own and go to an instance that is not
computing, so one stuck instance holds up only the connections already on it.

An instance computing for 5 s is logged, with its request. Started with
[`--max-compute 2s`](#options-of-quillo-run) (`quillo run main.qwe --max-compute 2s`), the server ends a request
that computes that long without letting go: a 500, between two statements, so nothing is
left half done, and a line in the log. `highlight`, `markdown` and `markdown_page` end at
it too, inside a long text, and their end is not an error value a `??` takes, as a loop's
is not. Time spent waiting does not count. Compiled code has
nowhere to stop, so under `--max-compute` functions are not compiled. `quillo check` warns on
a `while` that cannot end.

How long a wait may last:

- [`--workers N`](#options-of-quillo-run) (64 by default) requests can wait at once, each on a pool thread; more
  wait for a thread, while requests that only compute go on being answered. PostgreSQL
  queries take no thread. A thread the system will not start (an account's process
  limit, which on Linux counts threads) is said once in the log, `quillo: cannot start a
  thread for waiting work (...; the account's process limit?); requests wait for the N
  threads there are`; with none at all, it tries again a few times within a tenth of a
  second (another process may have ended), at most once a second. At
  start, a server whose `ulimit -u` is below what it may use says so: `quillo: the process
  limit (ulimit -u) is 30, and this server may use 73 threads: requests may wait for a
  thread; raise it, or give --workers fewer`.
- `http.*`: 10 s to connect, 60 s in all (300 s between the parts of a stream). A call to
  a `service` elsewhere: 5 s to connect, 10 s for a try, or its `timeout` (see
  [Services](#services)).
- A PostgreSQL connection: when all of the pool's are taken, a request waits up to 30 s
  for one, then gets an error.
- A query has no limit from Quillo: give the database one (on PostgreSQL,
  `alter role app set statement_timeout = '5s'`).
- Inside a SQLite transaction a wait holds up every request, since another request's SQL
  would land in the transaction: keep `http.*`, `sleep` and Rust (`.qwer`) calls out of
  them. A SQLite query itself always runs under the lock, on the loop's thread.

What a server is made of, for sizing a machine: the event loop's thread (a 16 MiB
stack; each request's coroutine reserves as much, and grows as it needs), up to
`--workers` plus one waiting threads per instance (so `--instances 4 --workers 64` may
reach 260 threads), one thread for PostgreSQL's pipelined connections (half the
instances, at least two, with `--instances`), a pool of one less than the cores (at most
31) for long lists, up to four for images, one for scheduled jobs, one for calls made
later, one for [`--watch`](#options-of-quillo-run), and one for each channel and [`--with`](#options-of-quillo-run) command. Memory is not
limited by anything configurable: the allocator is mimalloc on the loops and the waiting
threads, the system's elsewhere.

Each page with live data is a thread with 256 KiB of stack reserved, of which it touches
little, and two file descriptors, a socket and a connection. Quillo takes all the
descriptors the system allows when it starts, and says so in the log if that is too few
for 4096 pages; `ulimit -n`, or `LimitNOFILE=` under systemd, raises what is allowed.

With [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`), a change to the program or a `use`d file reloads it (starting its
state afresh) and refreshes open pages. [`--reload`](#options-of-quillo-run) reloads it the same way without
`--watch`'s development aids (errors on pages, refreshing them), for production
programs that change their own files, such as a plugin editor. Both watch the main file
and every file the program loaded (`use`d, mounted, a service's; under `--watch`, files
served with `file()` too): on macOS and Linux the system tells of a change as it happens
(kqueue, inotify), and a save, in place or as an editor does it, a new file renamed over
the old, is seen within a few milliseconds; elsewhere, or when the system cannot watch a
file (too many open, a network mount that tells nothing), the files' modification times
are looked at every 50 ms, and said so once in the log. No folder is watched, so a new
file is noticed once something loads it. A version that fails to load or check does not
replace the one that works; the errors are printed.
A reload parses only the files that changed (the others' parses are kept) and, under
`--watch`, compiles the program for the browser on a thread of its own once the new
version answers, so a save shows in open pages about 120 ms later for a program of a
few hundred components (the site quillolang.org, measured), plus the time to parse the
file that changed.

What a reload keeps, and what it starts afresh:

- **Requests in progress** finish on the version they started on: its code, its
  variables, its services in this process and its database connections (a query or a
  transaction in progress goes on, and commits or rolls back as it would have). Requests
  that come after are answered by the new version. While the new version loads, Quillo
  code waits (what waits on a database goes on); no request is dropped.
- **Sessions** are signed cookies or rows in the database, not server memory: they stay
  valid. The signing key is `QUILLO_SECRET`; without it a key made at start, which a
  reload keeps and a restart does not (everyone is signed out), so set it in production.
- **Global variables** start afresh: keep in the database what must last.
- The old version's database connections close once no request uses it (within a second
  or so of the last one). Calls made later and transactions over several databases left
  part way are looked after by whichever version is current; a version that is the
  first to use services or `db.atomic` starts what does that.
- What outlives a restart: the database (a file or server; without `db.open` or
  `QUILLO_DB` it is in memory and gone), `quillo-system.db` beside the program, the
  `.quillo/native/` builds, and sessions signed with `QUILLO_SECRET`. Globals, the
  memory of calls a service answered, the live cache and channels start empty.
- There is no handing a port from one process to the next: a new `quillo` binary means
  a stop (requests in progress finish, however long they take unless [`--drain`](#options-of-quillo-run) gives a
  time) and a start, during which the port
  answers nothing. For no gap, run two servers behind limer ([`--register`](#options-of-quillo-run); one that starts
  stopping leaves the pool, and its port refuses connections, which a health check sees as
  down) and restart them one at a time. Two programs
  cannot share a port: `mount` or `service /prefix` them in one. A new binary whose Rust
  glue changed builds the program's Rust files again: run `quillo build` with it as part
  of the deploy, before the restart (see [Rust](#rust)). While an old and a new copy
  share one database, a job whose name changed between them may run under both for one
  slot.

### Stopping and health

The first Ctrl-C or `SIGTERM` (`kill <pid>`) stops the server gracefully: it takes no new
connections (its port refuses them at once, even while a request holds the server
computing), closes idle ones, answers every request in progress, however long it takes, so
that a transaction under way commits whole, finishes sending the large files being sent
(a download), and exits with status 0. An answer given
meanwhile closes its connection and says so (`connection: close`), so the client sends its
next request elsewhere. Every 5 s it says what it still waits for:

```
quillo: stopping once the requests in progress are answered (again to stop now)
quillo: still waiting for GET /pay (rid 01HF3K...), 1 file being sent (again to stop now)
```

[`--drain 30s`](#options-of-quillo-run) gives the stop a time: what still runs then is cut off, its
transaction rolled back, and said: `quillo: stopped with GET /pay still running`, or, for a
request that holds its instance then (computing, or waiting inside a SQLite transaction),
`quillo: stopping with requests in progress: GET /pay (rid 01HF3K...)`. Without
it, a request that never ends holds the stop until a second Ctrl-C, or until whatever runs
the server gives up on its own: systemd sends SIGKILL after 90 s (`TimeoutStopSec`), Docker
after 10 s (`docker stop -t`), Kubernetes after 30 s (`terminationGracePeriodSeconds`), so
give `--drain` a little less than that time to have the cut said. A server that joined a
pool with [`--register`](#options-of-quillo-run) (see [Several servers](#several-servers)), or a copy that joined its app with
[`--join`](#options-of-quillo-run), leaves it first, so it is sent no more requests. A second Ctrl-C stops it at once. It stops the same when its stderr
goes into a pipe that has closed (a `tee` or a log shipper that ended): Quillo's own
messages are then dropped, and never keep it from stopping.

`GET /_quillo/health` answers `200` (`ok`) while the server is ready for requests, and
`503` (`unavailable`) while its program failed to load, or, asked on a connection open
before a stop, while it is stopping (a new connection is refused then): give it to a load
balancer's health check (limer's `health_uri`). `GET /_quillo/ready` also says
whether what the program needs is there: `{ "ready": true, "checks": { "database":
"ready", "service billing": "ready", "ledger": "not ready" } }`, 200 or 503, its
database (`select 1`), each service it declares (see [Services](#services)) and the
program's own checks, `ready("ledger", () => ...)` at the top level, a function that
gives true. For an orchestrator's readiness probe. Neither request is logged or counted.

`GET /_quillo/health/queries` says how long database queries take: the minimum,
maximum and average of the last 100, with how many ran and failed in all:

```
{ "enabled": true, "window": 100,
  "queries": { "count": 100, "min_ms": 0.21, "max_ms": 48.7, "avg_ms": 3.4, "total": 5120, "errors": 2 },
  "statements": [ { "sql": "select * from orders where customer = ?", "count": 100, "min_ms": 1.9, "max_ms": 48.7, "avg_ms": 12.3, "total": 840, "errors": 0 }, ... ] }
```

- **Off until switched on**, so a server pays nothing for it: on under [`--watch`](#options-of-quillo-run), when
  the server is started with [`--query-stats`](#options-of-quillo-run) or `QUILLO_QUERY_STATS=1`, or while it runs with `POST
  /_quillo/health/queries?on=1` (`?on=0` switches it off; either way what was counted is
  forgotten). Off, the address answers `{ "enabled": false }`.
- **`statements`**, each statement's figures, slowest on average first (at most 100 of
  them, its SQL as written with its `?`s, spaces squeezed), are shown only to a request
  with `Authorization: Bearer` and `QUILLO_HEALTH_TOKEN`, which switching on and off
  needs too: they show the program's SQL, and a server behind a proxy sees every
  visitor coming from this machine. Under `--watch`, requests from this machine may.
- `db.exec`, `db.query` and `db.one` are counted, a replica's answers too, failed ones
  as errors; Quillo's own tables (`_quillo_jobs` and the like) are not. The last 500
  statements are kept; past that, the one used longest ago makes room.

On a server already running, with the token in its environment:

```sh
export QUILLO_HEALTH_TOKEN=0123456789abcdef-token        # 16 characters at least
quillo run main.qwe                                      # or with --query-stats, on from the start
curl -X POST 'localhost:8080/_quillo/health/queries?on=1' -H "Authorization: Bearer $QUILLO_HEALTH_TOKEN"
curl localhost:8080/_quillo/health/queries -H "Authorization: Bearer $QUILLO_HEALTH_TOKEN"   # with statements
curl localhost:8080/_quillo/health/queries                                                    # the totals only
```

Without the switch, the address says how to turn it on: `"how": "off: run with
--query-stats or QUILLO_QUERY_STATS=1, or POST /_quillo/health/queries?on=1 with the health
token"`.

A second Ctrl-C, or a second `SIGTERM`, exits at once with status 130. Stopping also
stops the commands run with [`--with`](#options-of-quillo-run), and a stream's `send` fails, and its `sleep`
ends, once the server is stopping (but in a transaction, which finishes), so a long stream ends
as when its client goes away. Pages waiting for live data stay connected while the stop
waits, are let go as the server exits, and connect to the next server. Scheduled jobs and calls
made later start no more: one running finishes (its `sleep` ends it), and calls made later
not yet run wait in the database for the next start.

### Scheduled jobs

```quillo
schedule("5m", clean_sessions)                  // every 5 minutes, on the clock
schedule("0 3 * * *", nightly_report)           // cron: 03:00 UTC
schedule("@daily", () => db.exec("delete from tokens where expires < ?", now()))
```

A job is a function of the program, so it takes `with` as any function does, and what it
may do is held to that each time it runs:

```quillo
fn nightly_report() -> map with db, time {
    db.exec("insert into reports (day, total) select date(?), sum(total) from orders", now())
}
schedule("0 3 * * *", nightly_report)
```

What it does is seen where it writes. Here the
nightly report the routes add to, and the page that shows its rows:

```quillo
db.open("app.db")
db.exec("create table if not exists orders (id integer primary key, total int, at bigint)")
db.exec("create table if not exists reports (day text primary key, orders int, total int)")

fn nightly_report() -> map {
    let day = str(date.today())
    let r = db.one("select count(*) as n, coalesce(sum(total), 0) as total from orders where at > ?", now() - 86_400_000)
    db.exec("insert into reports (day, orders, total) values (?, ?, ?) on conflict (day) do update set orders = excluded.orders, total = excluded.total", day, r.n, r.total)
}

schedule("0 3 * * *", nightly_report)

route POST /orders { form: { total: int } } with db, time = db.exec("insert into orders (total, at) values (?, ?)", total, now())
route GET /reports with db = <ul>{for r in db.query("select * from reports order by day desc") => <li>{r.day}: {r.orders} orders, {r.total}</li>}</ul>
```

Each run is a line in the log, `POST /_quillo/job/nightly_report?at=1793631366 -> 200
(3.5ms)`, and `/reports` shows `2026-11-02: 1 orders, 40` after it. To try a job without
waiting for 03:00, give it `"10s"` while you work on it.

- The first argument is a duration (seconds, or `"30s"`, `"5m"`, `"1h"`, `"1d"`,
  `"2w"`, `"1y"`), counted from 1970 in UTC, so that every server agrees and `"5m"` runs
  at :00, :05, ...; or a cron line: `minute hour day month weekday` in UTC, with `*`,
  `1-5`, `*/15` and lists, its fields apart by spaces or tabs, or `@hourly`, `@daily`,
  `@weekly`, `@monthly`, `@yearly`. A job first seen (at start, or after a reload) waits
  for its next time, counted from when the server first looks: started at 04:01,
  `"2 * * * *"` runs at 04:02, and a nightly `"0 3 * * *"` of a server deployed each day
  at 02:00 runs at 03:00 that day; it never runs at once. The clock is looked at once a
  second.
- `schedule` and `every` add a job or a tick as the program starts, at its top level
  (or in a function the top level calls): called once it has started, in a route, a
  `server fn`, a job or a tick, they stop with an error (a 500 in a route), where they
  answered as if added and the job never ran. `quillo check` reads their written-out
  arguments as the start does (`every(0, f)`, a cron field `60`, `*/0`, `5-1`, `MON`,
  `@reboot`, six fields, a bad `{ name }` are errors there too).
- A job runs on the server like a request: waiting for the database lets requests run
  meanwhile, its errors are printed, and it is logged (`POST
  /_quillo/job/clean_sessions?at=1761600000 -> 200`, the time it was due for). A job
  still running when it is due again is skipped that time. Stopping the server lets
  running jobs finish, and starts no more.
- Jobs are named after their function (`{ name: "..." }` for a lambda, else
  `job-<line>`, or to run one function twice): letters, digits, `_`, `-` and `.`, at
  most 64, and no two alike. The start refuses a second job of a name when it is added;
  `quillo check` says so as an error where both calls surely run (twice at the top level,
  or a top-level function that adds it called twice), and warns where both may (two
  `if`s, `&&`, `||`, `??`, a loop); the arms of one `if`/`else` or `match` are
  alternatives, and say nothing. Nobody outside the server can start one.
- A job that runs for ever (a loop with `sleep` in it) is told to end when the server
  stops, and when [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`) loads the program again: its `sleep` ends the job, so a
  new version never runs beside the old one (a tick's too, and at a stop a stream's; a
  request's `sleep` is waited out, since requests in progress are answered). A job still running when it is due again
  is said once in the log, not each time.
- A service's jobs run with the app's when the service shares its process
  (`POST /_quillo/job/game~end_round` in the log), and in the service's own process
  when the app is started with [`--split`](#options-of-quillo-run) (`quillo run app.qwe --split`).
- Several servers on one database (PostgreSQL, or SQLite shared by processes on one
  machine) run each job once per due time: the first to record it in `_quillo_jobs`
  runs it. Without a database, or with one per tenant, each server runs its jobs;
  starting a server with [`--no-jobs`](#options-of-quillo-run) (`quillo run main.qwe --no-jobs`) turns them
  off there. For a database per tenant, loop over your
  tenants inside the job with `with_tenant(name, () => ...)`.

### Ticks: `every` on the server

```quillo
let ball = { x: 0, dx: 4 }

fn step() {
    let dx = ball.x + ball.dx > 300 || ball.x + ball.dx < 0 ? -ball.dx : ball.dx
    ball = { x: ball.x + dx, dx }
}
fn snapshot() -> map = { x: ball.x }

fn tick() { step(); channel.broadcast("room", snapshot()) }
every(50, tick)                                 // 20 times a second, on the clock

component Court() {                             // the other side: each page in "room"
    state x = 0
    channel.open("room", (s) => { x = s.x })
    <svg width="320" height="40"><circle cx={x + 10} cy="20" r="8"/></svg>
}

route GET / = <html><body><Court/></body></html>
```

Every page open on `/` shows the same ball, moved by the server 20 times a second; a page
opened later picks it up where it is.

- `every(ms, f)` at the top of a program runs `f` on the server every `ms`
  milliseconds, counted from 1970 so that each tick falls on a multiple of `ms`: the
  loop of a game, a sensor read, a short poll. In a component it is the page's clock
  instead (see [Components](#components)).
- A tick is named after its function (`{ name: "..." }` for a lambda, else
  `every-<line>`), and no two alike, as jobs are (above); `every` on the server is
  only for the top level, as `schedule` is. It runs like a job, one at a time: a tick still
  running when the next is due skips that one, so a slow step slows the loop and never
  doubles it. Ticks are not logged or counted in [`--metrics`](#options-of-quillo-run) (`quillo run main.qwe --metrics`), are not recorded in
  `_quillo_jobs` (each server runs its own), and stop with the server and on a reload.
- Under a second is what `every` is for; for minutes and hours, `schedule`.

### The REPL

`quillo repl` (or `quillo` alone) runs code as you type it, and prints each expression's
value:

```
quillo> let xs = [1, 2, 3]
quillo> xs | map((x) => x * 10)
[10, 20, 30]
quillo> fn double(n) {
....>   return n * 2
....> }
quillo> double(21)
42
quillo> db.open("app.db")
quillo> db.query("select * from notes")
[{ id: 1, text: "first" }]
```

- What a line defines (`let`, `fn`, assignments, `use "lib.qwe"`) stays for the lines
  after it. A statement left open (a `{` not yet closed) goes on to the next line, shown
  by `....>`.
- It runs from the folder it was started in: `use`, `db.open` and `read_file` paths are
  from there. `db.open` (or `db.connect`) gives it a database, as in a program. It does
  not read a `.env`.
- An error stops the rest of its line; what came before it stays. The static checks do
  not run here (`quillo check` runs them on a file), and routes declared here are not
  served. Ctrl-D exits. It has no line editing or history of its own: `rlwrap quillo
  repl` adds them.
- From a pipe (`quillo repl < steps.qwe`) it prints only the answers: no greeting and
  no prompts. Input that ends inside a statement is an error naming the line it began
  on, and a statement that failed makes the exit status 1. A line that is not UTF-8 is
  said by its number (`line 3 is not UTF-8 text (byte 0xC3 at column 1)`) and skipped,
  the rest runs, and the exit status is 1. `quillo repl -i` keeps the
  greeting and the prompts from a pipe too, as Python's `-i` does.

### Scripts as tools

A program with no routes is a script: `quillo run tool.qwe` runs it from the top and ends.
What follows `--` is its arguments, `args()`; `stdin()` is all that is piped in, and
`read_line()` the next line, `null` at the end; `exit(n)` ends it with status `n` *(since 0.7)*. A script
that ends by itself ends with 0, and one that fails (a stop, or an error a `?` at the top
passes on) with 1, saying what it was. Files are the program's folder's, as for a server;
[`--allow-path DIR`](#options-of-quillo-run), given when the script is started
(`quillo run tool.qwe --allow-path ../data -- x`), names another folder that `read_file` and `write_file` may use, by its
own path or through `..`, where a link leads being what counts (FEEDBACK29.md, Q-357);
without it, a link out of the folder is refused too (see [Paths and
links](#paths-and-links)). It is named, never assumed: the same program may serve strangers later, and in a program that
serves, `stdin`, `read_line` and `exit` are errors. A script whose output pipe closes
(`quillo run tool.qwe | head -1`) ends quietly, with status 141, as other tools do; a
server whose stdout closes goes on answering, and what `print` writes is dropped.

```quillo noexec
// cat orders.csv | quillo run total.qwe -- EUR
let currency = args()[0] ?? "USD"
let total = 0m
let line = read_line()
while line != null {
    let parts = split(line, ",")
    total += dec(parts[2]) ?? 0m
    line = read_line()
}
print(`${total} ${currency}`)
exit(total > 0m ? 0 : 3)
```

Written the way each is usually written, a script is faster than Python's: `bench/vs-python/run.py`
times eighteen everyday programs in both, process start included, and Quillo is ahead in all of
them. Against Python 3.9 on a 10-core Mac, the best of 21 runs each:

| | Python | Quillo |
|---|---:|---:|
| `fib(35)` | 1.11 s | 0.05 s |
| a float loop of 50 million | 1.71 s | 0.04 s |
| a `for` summing 5 million | 0.14 s | 0.10 s |
| `filter`, `map`, `sum` over 3 million | 0.09 s | 0.07 s |
| a million `xs = xs \| push(i)` | 0.04 s | 0.01 s |
| a map of lists, 100,000 pushes | 0.02 s | 0.02 s |
| a grid of 1,000 by 1,000, 100,000 writes | 0.02 s | 0.01 s |
| a field read a million times | 0.03 s | 0.02 s |
| 300,000 JSON rows grouped | 0.17 s | 0.14 s |
| word frequency over 1.2 million words | 0.21 s | 0.15 s |
| a million counts by an int key | 0.08 s | 0.03 s |
| a sieve to 2 million | 0.09 s | 0.07 s |
| `s += "x"` a million times | 0.04 s | 0.01 s |
| `s[i]` over 300,000 characters | 0.02 s | 0.01 s |
| a `while` over 200,000 characters | 0.02 s | 0.01 s |

The closest are a map of lists (0.85 of Python's time), 300,000 JSON rows grouped (0.81) and
the sieve (0.79); loops over numbers, lists of them, texts and counts run in machine code (see
[Compiled numbers](#compiled-numbers)).

### Debugging

`quillo dap` is a debugger for editors: it speaks the Debug Adapter Protocol on stdin and
stdout, and runs the program as `quillo run` would. In VS Code, with the extension, press F5
in a `.qwe` file (or *Quillo: Debug this file*, or the ▶ menu). A `launch.json` entry
gives the options:

```json
{
  "type": "quillo",
  "request": "launch",
  "name": "Debug the app",
  "program": "${workspaceFolder}/main.qwe",
  "args": ["--watch", "--port", "8080"],
  "stopOnEntry": false
}
```

- **Breakpoints** go on statements, in any file the program loads (`use`d files, mounted
  programs). One on a line with nothing to run (a blank line, `fn name(...) {`) moves to the
  next statement in the same function or route; outside any, it is not set.
- **Conditional breakpoints** stop when their expression is true; a **hit count** stops
  from that hit on (`3`), or at `== 3`, `> 3`, every `% 3`. **Logpoints** print their message to the
  debug console, with each `{expression}` in it worked out, and do not stop.
- **Runtime errors** (the mistakes that make a route answer 500) stop where they happen, with
  the error and the variables as they were. *Runtime errors*, in the breakpoints list, turns this off.
- **Stepping**: over, into and out of calls. A frame is a function, a view, a component, a
  route (`GET /notes/:id`) or a file's top level (`<main>`).
- **Variables**: the frame's locals, the program's globals (without its functions and the
  builtins), and the request being answered. Lists and maps open; long lists come in pages.
- **The debug console** runs code in the stopped frame: expressions, and assignments
  (`count = 0`) too. Hovering a name shows its value; hovering does not call functions.
- The program's output (`print`, the request log, errors) shows in the debug console.

When the program stops, all of it stops: no other request runs Quillo code until it goes on
(waits already under way, for a database or the network, finish meanwhile). Pausing a server
that is waiting for requests stops the next one to run.

While debugging, views are not cached (as with the option [`--no-view-cache`](#options-of-quillo-run)), so a breakpoint in one stops each
time it renders, and functions are not compiled to machine code. `.qwer` functions run as they
are, without stops. It runs on macOS and Linux; not yet on Windows.

#### In the browser

A component runs in the browser too, after its first render: its event handlers and updates.
With `"browser": true` in the launch configuration (or `"edge"`), VS Code opens the page in
Chrome (or Edge) once the server listens, with its JavaScript debugger, in the same session:
a breakpoint on a line of a `.qwe` file stops on the server and in the browser, wherever that
line runs. `"url"` names the page to open (by default, the address the server listens on).

This works because in development ([`--watch`](#options-of-quillo-run), or under the debugger) a page's bundle is a
small loader for files of their own: the runtime (`quillo.js`, as it is written), and each
component, view and function at `/_quillo/src/<hash>/<file>.qwe/<Name>.js`, with a source map
from its lines to the `.qwe` file's. The browser's developer tools show them by those names,
with the `.qwe` source beside them, and a variable a statement declares carries its own name
(`count`, which the compiled code calls `$count_1`); a component's state is a signal, its
value in `.v`. Values are JavaScript's: the debug console takes JavaScript, not Quillo. In
production a page's bundle is one minified file, as before. In both, the runtime keeps its
names to itself: none of its functions is the page's, so a page's own `function report()`
or a widget's `function run()` and Quillo's leave each other alone (FEEDBACK42.md, Q-481).

#### For developer tools

*(since 0.7)* A browser extension or a test can follow what the runtime does, without the debugger. It
asks by setting `data-quillo-devtools="1"` on `<html>` before the runtime starts (a content
script at `document_start`, or the page itself); a page that does not ask does nothing
more. The runtime then says what happens as `quillo:devtools` events on `document`, each
`detail` a JSON string, with values written as the wire writes them (`{"$dec": "12.50"}`,
`{"$int": "..."}`, `{"$quillo_err": ...}`):

```js
document.documentElement.dataset.quilloDevtools = "1";
document.addEventListener("quillo:devtools", (e) => console.log(JSON.parse(e.detail)));
// {"type":"component","id":1,"name":"Panel","parent":null,"props":{},"state":{"count":0}}
// {"type":"state","id":1,"name":"count","value":1}
```

| `type` | When | Carries |
|---|---|---|
| `hello` | the runtime starts | `protocol` (1), `version`, `dev` (under `--watch`), `base` |
| `component` | one is taken up or made in the page | `id`, `name`, `parent` (the id of the one it is in), `props`, `state` |
| `state` | a state is set to another value | `id`, `name`, `value` |
| `gone` | a component leaves the page | `id` |
| `error` | an error is reported | `message`, `place` (the `.qwe` file and line, under `--watch`), `id` when known |
| `remade` | a component is made again, not taken up | `name`; under `--watch`, `wrote` (what the page had) and `makes` |
| `call` | a server fn answers | `name`, `args`, `ms`, `ok`, `failed` (it did not reach the server), `value`, `live`, `idempotent` |
| `live` | a live change comes | `seq`, `topics`, `loads` (the components whose live states load again) |
| `channel` | a message goes or comes, over WebTransport or a WebSocket | `dir` (`in`, `out`), `channel`, `value`; or `joined` (true, false) |

A tool asks with a `quillo:devtools-ask` event, its `detail` a JSON string
`{ "id": ..., "op": ... }`, and is answered `{"type": "answer", "id": ..., "ok": ...}`, with
`error` when not:

| `op` | |
|---|---|
| `list` | each component now, as a `component` message |
| `where`, `target` | marks the component's element with `data-quillo-devtools-id` (the mark moves with the next `where`) |
| `set`, `target`, `name`, `value` | sets a state; under `--watch` only. The JSON is read as the server reads it: `2.0` is a float, an int past 2^53 exact, a map's keys in the order written |

Reading is open in production, as the page can be read with the browser's debugger anyway;
setting is not (FEEDBACK42.md, Part C).

Quillo's only names on `window` are `quillo` (the server fns, above) and `__quilloErrors`
when a page made it. What its own scripts hand each other is under one key,
`window[Symbol.for("quillo")]`: not an interface, but a test may wait on its `booted`
before driving a page (Q-486).

#### Other editors

Any editor with a Debug Adapter Protocol client (Neovim's nvim-dap, Helix, Emacs's
dape, Zed) can use `quillo dap` as an adapter it starts itself: the command `quillo`,
the argument `dap`, messages on stdin and stdout. A `launch` request takes:

| Argument | |
|---|---|
| `program` | the `.qwe` file to run (required) |
| `args` | its `quillo run` options (`["--port", "8080"]`) |
| `cwd` | the folder to run in |
| `stopOnEntry` | stop at the first statement |
| `noDebug` | run without stopping at breakpoints |

`attach` is not supported yet: the debugger starts the program itself (with
[`--no-view-cache`](#options-of-quillo-run), unless `noDebug`). `browser` and `url` are the VS Code extension's;
elsewhere, open the page in a browser yourself. What the adapter offers: breakpoints
with conditions and hit counts, logpoints, stepping, evaluation for hovers, the
`errors` exception filter (on by default), and terminate.

### Logs and metrics

Each request is logged on stderr as `GET /path?query -> 200 (0.4ms)`: the path with its
query string. Started with [`--log json`](#options-of-quillo-run), `quillo run main.qwe --log json`, the line is a
JSON object instead, for log collectors:
`time` (RFC 3339, UTC, with milliseconds), `method`, `path` (with the query string),
`status`, `ms`, `ip` (the TCP peer), `tenant` when the request had one, and `rid` (the
request's id, also sent back as `x-request-id`):

```
{"time":"2026-11-02T14:59:16.713Z","method":"GET","path":"/orders/1","status":200,"ms":0.07,"ip":"127.0.0.1","rid":"e60956f5b6f86d27"}
```

`--log off` turns the request log off;
errors are still printed. Lines are buffered and written each turn of the event loop,
or at 32 KiB. Not logged: `/_quillo/health`, `/_quillo/metrics`, Chrome's
`/.well-known/appspecific/com.chrome.devtools.json`, UDP packets, channel messages. The
launch token of a phone app built the old way, on 127.0.0.1, never appears in it.

An error's text can hold what a visitor sent, so in a text line its control characters and
line breaks are written as escapes (`\n`, `\x1b`): one request is one line, and no one can
write a line of their own into the log or colour a terminal. A malformed request line is
written escaped too, and under `--log json` it is an object of its own, `{"time",
"status", "error", "line"}`. Quillo's own notices (`listening on`, warnings, `stopping`)
are text lines on stderr in both modes.

Started with [`--metrics`](#options-of-quillo-run), `GET /_quillo/metrics` answers in Prometheus's text format:

```sh
quillo run main.qwe --metrics --log json
curl -s localhost:8080/_quillo/metrics | grep quillo_requests_total
# quillo_requests_total{status="2xx"} 2
```

What it reports:

| Metric | |
|---|---|
| `quillo_requests_total` | requests answered, by status class |
| `quillo_request_duration_seconds` | a histogram (1 ms, 2.5, 5, 10, 25, 50, 100, 250 ms, 1 s, 5 s) |
| `quillo_requests_in_progress`, `quillo_connections` | now |
| `quillo_prefetches_total{result}` | pages asked for ahead of a click: `answered`, `refused`, `used` |
| `quillo_later_total{result}` | tries of calls made later: `delivered`, `again`, `failed` (given up on) |
| `quillo_live_pages`, `quillo_live_refused_total` | pages waiting for live data, and those refused a place |
| `quillo_live_wakes_total` | pages woken by a change to what they follow |
| `quillo_live_runs_total{outcome}` | loaders asked for: `ran`, or `shared` a run another page made |
| `quillo_live_answers_total{kind}` | answers: `same`, a `patch`, or the `whole` value |
| `quillo_uptime_seconds` | since the server started |
| `quillo_bundles` | sets of components whose bundle is kept (see [Components](#components)) |
| `quillo_udp_received_total`, `quillo_udp_dropped_total` | packets (see [UDP](#udp)) |
| `quillo_channel_pages`, `quillo_channel_dropped_total` | pages on channels, messages dropped (see [Channels](#channels)) |

It is off by default, since anyone who can reach the server could read it.

### The policy

`with` limits what a function may do, from inside the program. A policy limits what the
whole program may do, from outside it *(since 0.7)*: the host's, for every program on a server, and
the site owner's, for one deploy. Policies are small TOML files, read as the program
starts, by `quillo run`, `quillo fcgi` and `quillo check`:

1. `/etc/quillo/policy.toml`, the host's, whenever it is there (one that is there and
   cannot be read stops the program);
2. each [`--policy FILE`](#options-of-quillo-run), in order;
3. `quillo.policy` beside the program's main file.

Each file only narrows what the ones before it allow: a list is a condition every file
must meet, so a later file cannot give back what an earlier one took away. A file that
says something a policy does not say, or a word that is not an effect, stops the program
with exit 2, naming the file and the line (`quillo.policy:2: `disk` is not an effect`).
What one refuses names the file that refused it.

```toml
# /etc/quillo/policy.toml: what any program on this server may do
[plugins]
build = false                     # never run cargo: prebuilt libraries only
allow = ["csv@1", "qr", "markdown"]

[effects]
allow = ["db", "http", "fs", "time", "random", "env"]   # no net

[http]
private = false                   # no loopback, private or link-local addresses

[env]
names = ["QUILLO_SECRET", "DATABASE_URL", "APP_NAME"]

[limits]
workers = 8
max_compute = "10s"
max_body = "10MB"
```

```toml
# quillo.policy, beside main.qwe: this program narrows it further
[http]
hosts = ["api.stripe.com", "*.mapbox.com"]
```

| Section | |
|---|---|
| `[plugins] build` | `false`: never build a Rust file, as [`--no-build`](#options-of-quillo-run) (see [Rust](#rust)) |
| `[plugins] allow` | the plugins a program may `use`, by file name: `"qr"` at any version, `"csv@1"` or `"csv@1.4.2"` by the file's `//! version:` line. A plugin not listed is refused, by `quillo check` too, before anything is built. It names plugins; it does not prove their code |
| `[effects] allow` | the effects anything in the program may use, as a `with` on its top level, routes, jobs and functions alike; a Rust file with no `//! effects:` line is refused anywhere under it |
| `[http] hosts` | the hosts `http.*` may reach: `api.stripe.com`, `*.mapbox.com` (its subdomains, not itself), `*`. Each redirect is judged before it is followed |
| `[http] private` | `false`: `http.*` reaches nothing on this machine or its networks (loopback, private, link-local and the cloud's metadata at `169.254.169.254`, carrier-grade NAT, IPv6's unique-local), as the name resolves when it connects |
| `[env] names` | the variables `env()` may read; any other reads as not set, so `env("HOME") ?? "none"` gives `"none"` |
| `[limits] workers`, `max_compute`, `max_body` | the most [`--workers`](#options-of-quillo-run), [`--max-compute`](#options-of-quillo-run) and [`--max-body`](#options-of-quillo-run) may be: a smaller value given on the command line stays (`"10s"`, `"500ms"`, `"2m"`; `"10MB"`, `"512KB"` or bytes). Each takes what the option takes: 0 is refused (`p.toml:2: `max_body` is a size larger than 0`), as the option refuses it |
| `[limits] max_tool_calls` | the most tool calls a minute each user of the MCP door may make: it can lower `--max-tool-calls`, not raise it |

Under the two files above, with `APP_NAME=Shop`:

```quillo noexec
route GET / = `${env("APP_NAME") ?? "shop"} on ${env("HOME") ?? "no HOME"}`   // Shop on no HOME
route GET /charge = http.get("https://api.stripe.com/v1/charges").status       // reaches Stripe
route GET /other = str(http.get("https://example.com/").error)
// http: example.com is not among the hosts quillo.policy allows: api.stripe.com, *.mapbox.com
```

`quillo check --policy host.toml main.qwe` says, before a deploy, what the program would
be refused, as errors (see [Checking](#checking)):

```
quillo: policy host.toml + quillo.policy (effects: db, http, fs, env, time, random; no builds; no private addresses)
main.qwe:6: error: `udp.listen` needs the net effect, which host.toml does not allow
```

- `quillo check` and `quillo run` always read the host's file and `quillo.policy`;
  `--policy` adds one, and `check` names the files only when one is given. A byte-order
  mark at a file's start is skipped. The start line lists the limits that hold the
  program: `quillo: policy p.toml (workers 8; max_compute 10s; max_body 10485760 bytes)`.
- A request the policy's `max_compute` ends is a 500, as one `--max-compute` ends,
  wherever it is ended (a loop, a call, `highlight`, `markdown`), and the log names
  whichever is the tighter: `this request computed for longer than the policy p.toml
  allows (max_compute)`, or `... than --max-compute allows`. No `??` takes that stop
  (`highlight(x) ?? x` too). A page (a Markdown preview) is told `the server's policy`,
  with no path of the server's.
- [`--split`](#options-of-quillo-run) passes the `--policy` files and `quillo.policy` to its services; a service
  reads the host's file itself.
- What a policy holds to is Quillo code and what it loads. A plugin is Rust in the
  server's process, which a policy cannot watch: it can only choose which plugins load.
  `private = false` covers `http.*` only, not `db.connect` or UDP.
- A policy binds the `quillo` that reads it. On a shared server, a user who can upload a
  binary of their own can run it without one: the boundary between accounts is the
  system's (Unix users, CloudLinux's CageFS and LVE limits).
- Not built yet: a catalog whose entries are signed, a policy that allows catalog
  plugins only, and a sandbox from the system (`--sandbox`, seccomp and Landlock) or for
  plugins (WebAssembly).

### Quillo on a shared server (cPanel)

*(since 0.7)* A shared hosting account (cPanel, often on CloudLinux, with Apache or LiteSpeed in
front) gives a folder, a web server the host runs, and no root. It differs from a server
of one's own in three ways that matter to Quillo:

- **127.0.0.1 is everyone's.** Every account on the machine can connect to a TCP port
  there, past the web server, so `quillo run --port 8080` is open to the neighbours. A
  Unix socket in a folder of the account's own, or FastCGI, is not.
- **Nothing keeps a process of yours running.** There is no systemd. Processes started by
  hand or by cron are often killed. The web server starts FastCGI programs itself, keeps
  them up while requests come, and stops them when idle.
- **The account has limits.** No Rust, a disk and inode quota, and CloudLinux's LVE
  limits: processes (`NPROC`, which counts threads, for the whole account), and requests
  at once (`EP`; over it, visitors get a 508 page from the host, not from Quillo).

**The binary.** The Linux releases from 0.7.0 on are linked against glibc 2.17,
so they start on CentOS 7, CloudLinux 7 and 8, AlmaLinux 8 and newer (0.6.0's need glibc
2.35). They are glibc builds, not static ones, as a plugin is a library they load.

**FastCGI with `mod_fcgid`**, where the account has it (it is in cPanel's EasyApache 4).
`quillo fcgi main.qwe` answers FastCGI on the socket the web server gives it on file
descriptor 0, and Apache starts it, keeps it up and stops it. In the site's folder:

```sh
#!/bin/sh
# app.fcgi, made executable with chmod 755
cd /home/u/app && exec /home/u/bin/quillo fcgi main.qwe --no-build --workers 8
```

```
# .htaccess
Options +ExecCGI
AddHandler fcgid-script .fcgi
RewriteEngine On
RewriteCond %{REQUEST_FILENAME} !-f
RewriteRule ^ app.fcgi [QSA,L]
```

`.htaccess` and `app.fcgi` go in `public_html` (or the domain's folder); the program
itself, its `.env` and its database stay outside it, in `/home/u/app`, where Apache does
not serve them.

- Each request is the request the visitor made: its path (`REQUEST_URI`), headers and
  body, and `request.ip` is Apache's `REMOTE_ADDR`, with no [`--trust-proxy`](#options-of-quillo-run). The server
  does the rest as for any visitor: routes, sessions, limits, the log (on Apache's error
  log).
- Its jobs and its own calls reach it through a socket only it can open, in a private
  folder under the system's temporary one.
- What FastCGI cannot carry: WebSockets. Live data falls back to plain requests, each
  held until a change or 25 s, so live pages still follow changes; channels over a
  WebSocket do not work, and WebTransport needs a UDP port of its own. A `stream` or `events` answer
  arrives whole unless the host sets `mod_fcgid`'s output buffer to 0
  (`FcgidOutputBufferSize 0`).
- `mod_fcgid` may start several processes, each with its own global variables, as
  [`--instances`](#options-of-quillo-run) would; keep what must be shared in the database. `quillo fcgi` refuses
  [`--split`](#options-of-quillo-run), `--instances`, [`--register`](#options-of-quillo-run) and [`--join`](#options-of-quillo-run).
- A request Apache abandons while its answer streams (FastCGI's `ABORT`) ends when the
  connection closes.
- Run from a terminal, it says what it needs: `file descriptor 0 is not a socket:
  quillo fcgi is started by a FastCGI process manager (mod_fcgid), or given --listen
  unix:PATH or --port`. With [`--listen unix:f.sock`](#options-of-quillo-run) (or [`--port 9000`](#options-of-quillo-run), on 127.0.0.1) it
  makes its own socket instead, for nginx's `fastcgi_pass unix:/srv/app/f.sock` with
  `include fastcgi_params` on a server of one's own.

**A Unix socket behind `mod_proxy`**, where the host sets it up (a VPS with WHM, or a
host that offers Quillo): `quillo run main.qwe --listen unix:app.sock --trust-proxy
127.0.0.1`, kept running by the host, and Apache's

```
ProxyPass "/" "unix:/home/u/app/app.sock|http://localhost/" upgrade=websocket
ProxyPassReverse "/" "http://localhost/"
```

(Apache 2.4.47 or later for `upgrade=websocket`; `ProxyPass` is the host's
configuration, not `.htaccess`'s). The socket is made with mode 0600, for the account's
own processes: give [`--socket-mode 660`](#options-of-quillo-run) when the web server reaches it through its
group. A socket left by a server that did not stop cleanly is taken over; one another
server answers on is not (`a server is listening there`). Every visitor through the
socket is 127.0.0.1 to the program, which the server says at start until
`--trust-proxy 127.0.0.1` is given; with it, `request.ip` is the visitor's, from the
`X-Forwarded-For` Apache adds. Jobs reach the program through the socket.

**Plugins, prebuilt.** The account has no Rust, so build the program's `.qwer` files
elsewhere with `quillo build main.qwe`, on Linux on the same processor, and on a system
no newer than the server's (a plugin linked against a newer glibc does not load on an
older one: build in a CentOS 7 or AlmaLinux 8 container). Upload `.quillo/` with the
program: `.quillo/native/` and `.quillo/plugins.lock`. Run with [`--no-build`](#options-of-quillo-run), so that
nothing is ever built on the server, and a library that is not the one built is refused
rather than built again. A new `quillo` whose glue changed (the libraries' names carry its
version) loads none of the old libraries: run `quillo build main.qwe` with it as part of
the deploy that brings it, and upload `.quillo/` again; under `--no-build` the server says
which file was `built by an older Quillo`.

**A policy.** The host's `/etc/quillo/policy.toml` says what any program may do there;
`quillo.policy` beside the program narrows it for the site (see [The
policy](#the-policy)). `quillo check --policy policy.toml main.qwe`, run before
uploading, says what would be refused.

**Threads and `ulimit -u`.** A Quillo server may use [`--workers`](#options-of-quillo-run) + 9 threads (73 with the
default 64), and on Linux the account's process limit counts them, beside PHP, cron and
SSH. Give `--workers 8` or so (a host's policy can cap it with `[limits] workers`). At
start a server says when `ulimit -u` is below what it may use, and a thread the system
refuses is said once (see [Concurrency and reloading](#concurrency-and-reloading)).

**The rest of the account.**

- **Settings:** the `.env` beside the program, or [`--env-file`](#options-of-quillo-run) for one outside the
  public folder. A FastCGI program gets the web server's environment, so keep secrets
  there and not in `app.fcgi`.
- **Jobs:** a FastCGI process stopped when idle runs no `schedule` or `every`. Run them
  from cPanel's Cron Jobs page instead, as a script: `cd /home/u/app && quillo run
  jobs.qwe`.
- **The database:** SQLite in a folder outside `public_html`, which one process at a
  time writes; or the host's MySQL, `db.open(env("DATABASE_URL"))`.
- **Paths:** a link out of the program's folder is refused (see [Paths and
  links](#paths-and-links)), so a link to another account's files leads nowhere.
- **Not built:** Passenger (cPanel's Application Manager starts Node.js, Python and Ruby
  apps only), and a WHM plugin that installs Quillo for every account.

## Cached views

A view's HTML depends on its arguments and on the global variables it reads. When a
view is called again with the same arguments, and none of those globals has been
assigned since, Quillo reuses the HTML it rendered last time instead of running the
view again. A page made of views is then served about as fast as a static file (the
50-row benchmark page goes from about 26,000 to 84,000 pages a second), and is still
up to date the moment something it shows changes.

- Arguments are compared by value for numbers and text, and by identity for lists and
  maps: values never change, so the same list is the same data. `<List items={products}/>`
  is reused until `products` is assigned; `<List items={[1, 2]}/>` builds a new list
  each time and is rendered each time.
- A view that reads the clock, randomness, the database, files, the environment, the
  network or the request (`url()`), prints, writes, or renders a component, runs every
  time. Database reads are not cached because a row changed outside Quillo would go
  unnoticed.
- Views inside views pass on what they read, so a cached page follows the global its
  innermost view uses. Only views declared at the top level are cached, and at most 32
  renders of each are kept (the 32 most recent sets of arguments).
- Starting the program with [`--no-view-cache`](#options-of-quillo-run), `quillo run main.qwe --no-view-cache`,
  turns it off (to measure, or to rule it out).

## Compiled numbers

A top-level function that only does arithmetic on ints, floats and bools is compiled to
machine code (with [Cranelift](https://cranelift.dev)) the second time it is called, or the
first if it has a loop of its own, for the types of the arguments it gets: `let`,
assignment, `if`, `&&`, `||`, `return`, `for` over `range(...)` or `a..b`, `while`, and
calls of other such functions, itself included. So is a lambda of that kind given a
number, as `map` and `filter` give one: `xs | filter((x) => x % 3 == 0)`.
It then runs as it would have, only faster: `fib(32)` takes 11 ms (Node 11, Bun 21),
and a loop of ten million steps 29 ms (Node and Bun take about 280 ms for the same
loop, on doubles).

- The answers are the interpreter's: ints are 64-bit and overflow is an error, `%` of
  ints by zero is an error, ordering NaN (`<`, `>`) is an error while `NaN == NaN` is
  false, and the 10,000-call limit holds. Where the interpreter would fail, the
  machine code gives up and the call is run again by the interpreter, which fails as
  always (the function changed nothing, so running it twice is running it once).
- What it does not take runs in the interpreter as before: strings, lists, maps,
  decimals, builtins, globals that are not functions, functions inside functions, a variable that
  changes type, and `/` of two ints, whose answer is an int or a float depending on the
  numbers (`6 / 3` is 2, `7 / 2` is 3.5).
- A function it calls that is assigned something else is not called by the old code.
- Not with a fuel limit (scripts given a budget of steps), and not in builds without
  the `jit` feature (`cargo build --no-default-features --features mysql`).
  `QUILLO_JIT=0` runs everything in the interpreter, to compare the two.

A loop goes to machine code by itself, wherever it is: in a function that does other
things too, or at a script's top. Its first 256 rounds run in the interpreter; a loop that
has gone that far is compiled for the types its variables have then, and goes on from the
round it is at. Its body may use ints, floats and bools, lists of them and lists of such
lists held in variables: `xs[i]`, `g[y][x]`, `len(xs)`, and `xs = xs | set(i, v)`, `g =
set(g, y, set(g[y], x, v))` and `xs = xs | push(v)` in any of their forms; texts: `s[i]`
(a character, as the interpreter gives it), `let c = s[i]`, `len(s)`, a character compared
with a quoted text, another character or `null`, and `s += x` or `s = s + x`; counting into a
map by an int key, `m = m | set(k, get(m, k, d) + e)` (also `-`, `*` and `set(m, ...)`, with a
number for `d`); `let`, assignment, `if`, `for` over a range and `while`. A list read in a
loop costs about 4 ns a round, written about 5, where the interpreter takes 45 (FEEDBACK29.md);
a text read by index about 1. A loop that appends to a text and reads it by index in the
same loop is not compiled.

- What it does is what the interpreter does: an error is the interpreter's own, made from
  the same values (an overflow, a `%` by zero, a `set` past the end, a step of 0).
- An item of another type than the code was made for, or a read past the end, gives the
  round back to the interpreter, with its numbers as the round began. A list written
  cannot be put back, so a loop that reads after writing in the same round is not
  compiled, and runs as before.
- A list it writes is taken out of its variable while it runs, and put back after; one
  only read is held. What else holds the list keeps what it had.
- Not in a server for a global the loop assigns (what follows it must be told), not under
  the debugger, with [`--max-compute`](#options-of-quillo-run), in a script with a budget, or in a cached view.

## Rust

A `.qwer` or `.rs` *(since 0.7)* file is Rust, and a program `use`s it as it uses a `.qwe` file: its
top-level `pub fn`s are the program's functions. The two extensions are the same to
Quillo: `.rs` is the one editors, `rustfmt` and clippy know, and `.qwer` is the catalog's
and `quillo install`'s. A `.rs` file is the program's only when a `use` or `import` names
it (`use "imaging.rs"`, `import imaging.rs use brightness`), so a crate's own `src/*.rs`
beside the program is left alone. Here is `imaging.qwer`:

```rust
//! effects: fs
//! [dependencies]
//! image = { version = "0.25", default-features = false, features = ["jpeg"] }

/// How bright a picture is, from 0 to 255.
pub fn brightness(path: &str) -> Result<f64, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_luma8();
    Ok(img.pixels().map(|p| p.0[0] as f64).sum::<f64>() / img.len() as f64)
}
```

```quillo
use "imaging.qwer"

route GET /photos/:name = { brightness: brightness(`photos/${name}.jpg`)? }
```

- Values cross as Quillo has them: `i64` and the other integers up to 64 bits are `int`
  (a `u64` or `usize` answer past `i64`'s range is a runtime error; `i128` cannot
  cross); `f64` and `f32` are `float`; `bool`; `String`, `&str` and `char` are `str` (a
  `char` argument must be one character); `Vec<T>`, `VecDeque<T>` and `&[T]` are lists;
  `HashMap<String, T>` and `BTreeMap<String, T>` are maps, nested too; `Option<T>` is the
  value or `null`; `serde_json::Value` is anything. A function returning
  `Result<T, E>` gives `T`, or an error value with `E`'s message, so `?` and `??` work
  on it. `()` is `null`. References (`&String`, `&Vec<T>`) are fine as parameters.
  A `u64` past `i64`'s range inside a returned `serde_json::Value` is a runtime error
  that names where it is (`.id`, `[1].n`), not a float that has lost digits, and one
  returned alone names its function. NaN and the infinities cross to an `f64` and back
  as they are, at any depth, a map of one key too (they arrived as `null`); a `serde_json::Value` argument that holds one is
  refused by the argument's name, as JSON has none. An error value given to a Rust
  function, at the top or inside a list or map, is given back, and the function does not
  run.
- An argument that does not fit is refused with the parameter's name: `` `small`:
  argument `n` does not fit in i32: 5000000000 ``. Values cross at most 126 levels deep:
  an argument nested deeper is refused (`` `deep`: argument `v` is nested too deeply: a
  plugin takes values at most 126 levels deep ``), and an answer nested deeper is an
  error that says so (`` `pr_deep` (w.qwer) gave back a value nested too deeply ``).
- What does not cross: structs (even with `#[derive(Deserialize)]`: take a
  `serde_json::Value` and `serde_json::from_value` it inside), tuples, sets, `Box`,
  `Rc`, `Arc`, `PathBuf`, `Cow`. A decimal arrives as its text, so an `f64` parameter
  refuses one. Bytes are a list of numbers each way (a 10 MB file is about 40 MB of
  JSON): pass a file's path, or base64 text, instead.
- The rest of the file is ordinary Rust: private functions, structs, `impl`s, modules,
  `use`. A `pub fn` that is generic (a written lifetime counts: `fn f<'a>`), `async`,
  takes `&mut`, or has a type that cannot cross, as a fixed-size array cannot, is not
  exported, and `quillo check` says why. So is one with a reference inside another type,
  `Option<&str>`, `Vec<&str>` or `&[&str]`, which the glue cannot build: the message
  names the owned type to take (`Option<String>`, `Vec<String>`). `&str` and `-> &'static
  str` are fine, as are paths (`std::string::String`, `std::collections::HashMap`) and
  comments in a signature. A raw name, `pub fn r#type`, is the function `type`, and the
  functions after it are read; a function named like a Quillo keyword (`view`, `match`,
  `reply`) is not exported. A `#[cfg(...)]` on a `pub fn` is kept: called where it was
  left out of the build, it says the function is not in this build. Another crate's type named `Value` passes the
  check and fails the build. A signature may name what the file's own `use` lines bring
  in: after `use serde_json::Value;` and `use std::collections::HashMap;`, `pub fn
  role(auth: Value)` and `pub fn total(m: HashMap<String, i64>)` build as written in
  full do.
- The `//! [dependencies]` lines are the file's crates, as in `Cargo.toml`: any crate
  from crates.io or git. They come first, after other `//!` lines if any; the first line
  that is not `//!` ends them. Only that table is read (no `[features]` table, no
  `[patch]`, no `build.rs` of the file's own; a crate's own build scripts do run).
  `serde_json` is always there. C libraries come through `-sys` crates. The `//!` lines
  may follow `//` comments, blank lines, block comments and `#![...]` attributes at the
  top of the file.
- **`//! effects: fs, http`**, a header line, says what the file's functions do, in the
  words of `with` (`db`, `http`, `fs`, `env`, `time`, `random`, `net`; `db.analytics` is
  `db`), or `//! effects: none`. It is what lets a `with` function call them: a function
  declared `with fs` may call `brightness`, one declared `with time` may not. A file
  without the line may do anything: its functions are refused inside any `with`, and
  anywhere under a policy that narrows effects (see [Effects](#effects)). A word that is
  not an effect is said by `quillo check` at its line. Quillo cannot see what Rust does,
  so the line is the author's claim, read as a promise: write what the code really
  reaches, and review it in a plugin from elsewhere.
- **Taking names.** `import redis.qwer use cache_get, cache_set` takes only those
  functions; `as` gives one another name, `import redis.qwer use cache_get as redis_get`,
  and it is not there under its own. A name the file does not export is an error that
  names the nearest it does. `import redis.qwer use *`, or `use redis.qwer`, takes them
  all. Each file takes what it calls, as with a `.qwe` file (see [Files of a
  program](#files-of-a-program)).
- A Rust file's names are the file's that takes them, as a `.qwe` file's are: two Rust
  files exporting one name are two functions, and a file that takes both is told to
  choose, naming the files (`` comes from a.rs and b.rs: ... import a.rs use who ``);
  one function taken by several files is one function. A name from a Rust file taken
  whole gives way to the file's own definitions and to names it takes by name. The fix a
  warning suggests (`import x.qwer use len as x_len`) parses.
- A `pub fn` named like a builtin (`len`, `get`) hides the builtin in the files that
  take it, and `quillo check` warns of it at the `.qwer` file's line, with how to take it
  under another name: `import x.qwer use len as x_len` (FEEDBACK27.md, Q-326).
- `quillo check` reads the functions' types from the Rust, without building anything:
  calls are checked, and editors show them on hover. The language server also runs
  `cargo check` on each save of a `.qwer` file, and shows Rust's errors in the editor, in
  a folder the editor says it trusts: `cargo check` runs the dependencies' build scripts
  and macros, so a cloned repository's `.qwer` could run code as it was opened *(since
  0.7)*. The VS Code extension says so from its Workspace Trust; another editor gives the
  server `initializationOptions: { "trusted": true }` for folders of your own.
  They run on the server only (in services and mounted programs too); components call
  them through a `server fn`.
- A Rust function cannot call back into Quillo: no `db`, `publish` or `env()` (it reads
  `std::env` itself). It may start threads, keep a runtime of its own in a `static` and
  `block_on` it, and hold a connection pool in a `static`.

How it runs:

- The Rust files the program uses that are not built yet are built before its first
  statement runs, all in one cargo call over a workspace, so cargo builds them side by
  side (`quillo: building ./a.rs, ./b.qwer together`); a file that fails there is built
  again alone by its `use`, which says why.
- `use` builds the file with `cargo` (`$CARGO`, or the one on the `PATH`), in a crate of
  its own in Quillo's cache: `QUILLO_NATIVE_CACHE`, else `$XDG_CACHE_HOME/quillo/native`,
  else `~/.cache/quillo/native`, else the system's temporary folder. The crates' builds
  are shared there, and nothing old is removed (it grows: clear it now and then). The
  library is kept in `.quillo/native/` next to the file, named by the file's whole name
  (so `x.rs` and `x.qwer` side by side keep one each), the glue's version, and a hash of
  the file's text, the glue and the processor: the next start loads it without building.
  An older Quillo's libraries, and their lock lines, are kept, so going back to it still
  runs.
  A build takes seconds the first time, more with large crates, and needs crates.io
  (or cargo's own offline mode).
- `quillo build app.qwe` builds every Rust file the program and its mounted programs
  use (not those of its services: build each service's file too), and prints where each
  library went; it also takes a `.qwer` or `.rs` file itself. Deploy `.quillo/native/` with the
  program, and the server needs no Rust: it loads what was built for that content, on
  the same processor and system (there is no cross-compiling: build on Linux for
  Linux), by the same Quillo glue. A Quillo whose glue changed builds again, and needs
  cargo then: run `quillo build` once as part of the deploy that brings it.
- **`.quillo/plugins.lock`** *(since 0.7)*, beside `.quillo/native/`, holds each library's file name
  and SHA-256, a line each, written by every build: `quillo build`, a `use` that built,
  [`--watch`](#options-of-quillo-run). Deploy it with `.quillo/native/`. A library is loaded only when it is the one
  built: one whose hash differs, or that has no line, is built again where cargo may run
  (`quillo: libhi-687dd0b24f29e5cf.dylib is not the library that was built (its SHA-256
  differs from .quillo/plugins.lock's): building it again`), and refused where it may
  not. A folder built before there were locks has none: its libraries load as before, with
  a warning once (`.quillo/plugins.lock is not there, so the libraries in .quillo/native
  are loaded unchecked, as before; quillo build writes it`). A build writes the lock, but
  a library already there is not built again, so remove `.quillo/native/` and run
  `quillo build` to get one. Whoever can write both the library and the lock can still
  run code in the server.
- **[`--no-build`](#options-of-quillo-run)** never runs cargo, so no crate's build script runs on the server: a
  library is loaded only as the lock names it, and a file not built is an error
  (`./hi.qwer is not built, and --no-build says not to build it here: build it with
  quillo build and bring .quillo along`); one built only by an older Quillo is said to
  be (``./hi.qwer was built by an older Quillo, whose libraries this one cannot load, and
  --no-build says not to build it here: run `quillo build` once where Rust is, and bring
  .quillo along``). Under it, a folder with no
  lock loads nothing.
  [`--split`](#options-of-quillo-run) passes it to its services, and a policy's `[plugins] build = false` is the
  same (see [The policy](#the-policy)).
- **A policy's `[plugins] allow`** names the plugins a program may `use`: `"csv"` any
  version, `"csv@1"` or `"csv@1.4.2"` by the file's `//! version:` line (`1` takes
  `1.4.2`, not `10.0`). A plugin it does not name is refused before anything is built:
  `` the plugin `pdf` 2.0.1 is not in the plugins /etc/quillo/policy.toml allows: csv@1, qr ``.
  It names plugins by their file's name and header; it does not prove their code.
- A call runs off the event loop, without the lock Quillo code holds, on the threads
  that also wait for databases ([`--workers`](#options-of-quillo-run)): a slow function holds up no one, and
  calls from several requests run on several cores at once; many long calls at once
  can keep database waits waiting. Inside a SQLite transaction a call holds the lock,
  and every request waits. Nothing times a call out or cancels it. The values cross as
  JSON, which suits most calls; a list of millions of numbers takes a while to cross.
- A panic is a runtime error in the request that called the function (not an error
  value: `??` does not catch it), and the server goes on. `std::process::exit`, an
  abort, or a stack overflow ends the whole server. Arguments of the wrong type are
  refused before the function runs. A stop (`SIGTERM`) waits for a request in a call; one
  that [`--drain`](#options-of-quillo-run)'s time cuts off is logged as a stop, `quillo: stopped with GET
  /slow?ms=14000 still running`, not as a bug in Quillo.
- When the program is started with [`--watch`](#options-of-quillo-run) or [`--reload`](#options-of-quillo-run)
  (`quillo run main.qwe --watch`), a Rust file that changes is built again and the
  program reloaded with it. A build that fails prints the compiler's errors, and the
  program goes on as it was. Requests in progress finish with the version they started
  with: libraries stay loaded, the old ones for the life of the process, with their
  `static`s (an old pool stays open).
- Rust can do anything the server can, so Rust files are the developers', like the
  rest of the program: never let a program's users write them (they write sandboxed
  Quillo with `run_script`).

See [rust.qwe](https://github.com/quillolang/examples/blob/main/rust.qwe) and [rust.qwer](https://github.com/quillolang/examples/blob/main/rust.qwer).

### Plugins from the catalog

[quillolang.org/plugins](https://quillolang.org/plugins/) has `.qwer` files ready to use,
each with a version, an example, and a test run before it is published. `quillo install`
puts them beside the program (its [options](#other-commands) are below):

```
quillo install csv qr jwt          # csv.qwer, qr.qwer and jwt.qwer, into this folder
quillo install csv --example       # and csv.qwe, its example program
quillo install --update            # the plugins here that the catalog has newer
quillo install --list pdf          # what the catalog has, matching "pdf"
```

- It reads the catalog (`https://quillolang.org/plugins/index.json`; [`--registry`](#other-commands) or
  `QUILLO_REGISTRY` for another), downloads each file, and writes it only when its
  SHA-256 and the `//! version:` line of its header match the catalog's. A plugin
  that fails leaves the others going, and the exit status is 1.
- A file already there is left alone when it is the same, replaced when the catalog's
  version is newer, and kept when it is newer or the same version changed by hand:
  [`--force`](#other-commands) replaces it. [`--dir DIR`](#other-commands) installs elsewhere. `name@1.2.0` asks for that
  version, and fails when the catalog has another (it keeps only the newest).
- Then `use csv.qwer`, or only what you need, `import csv use csv_parse, csv_write`: the
  first run builds it with cargo, as any `.qwer` file.
- A catalog plugin is called inside a `with` function only once its header has a
  `//! effects:` line, as any Rust file (see [Rust](#rust)). The catalog's files do not
  have it yet: add it to your copy after reading what the plugin does.
- `install` checks a file against the catalog it came from, which a changed server
  could change too: the catalog's entries are not signed yet. A host can still list the
  plugins and versions it takes, in a policy's `[plugins] allow`.

### Other stores

`db` is for SQL databases. For another kind (Redis, MongoDB, a graph database), use its
crate in a `.qwer` file: its functions are the driver, and the program calls them like
any other. A count of visits in Redis:

```rust
//! effects: db, env
//! [dependencies]
//! redis = "0.27"

use std::sync::OnceLock;

/// The client, made on the first call (`REDIS_URL`, or Redis on this machine).
fn client() -> Result<&'static redis::Client, String> {
    static C: OnceLock<redis::Client> = OnceLock::new();
    if let Some(c) = C.get() {
        return Ok(c);
    }
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let c = redis::Client::open(url).map_err(|e| e.to_string())?;
    Ok(C.get_or_init(|| c))
}

/// The value at `key`, or null.
pub fn cache_get(key: &str) -> Result<Option<String>, String> {
    let mut con = client()?.get_connection().map_err(|e| e.to_string())?;
    redis::cmd("GET").arg(key).query(&mut con).map_err(|e| e.to_string())
}

/// Adds one to the number at `key`, and gives the new number.
pub fn cache_incr(key: &str) -> Result<i64, String> {
    let mut con = client()?.get_connection().map_err(|e| e.to_string())?;
    redis::cmd("INCR").arg(key).query(&mut con).map_err(|e| e.to_string())
}
```

```quillo
use "redis.qwer"

server fn visits(page: str) -> int {
    watch(`visits:${page}`)                  // live pages showing this follow the topic
    int(cache_get(`visits:${page}`)? ?? "0")
}

server fn visit(page: str) {
    let n = cache_incr(`visits:${page}`)?
    publish(`visits:${page}`)                // and refresh when it changes
    n
}

component Visits(page: str) {
    state n = live(visits, page)
    <button onclick={() => visit(page)}>{n} visits</button>
}
```

- Values cross as JSON, so a connection cannot be a Quillo value: the Rust keeps it in a
  `static`, and the library stays loaded between calls. (This one connects on each call;
  a pool, such as the crate's `r2d2` feature, keeps connections open.)
- Calls run off the event loop, so a slow store holds up no one, and a store that is
  down gives an error value.
- What `db` does by itself is the program's to do here: live pages follow what a loader
  `watch`es and a write `publish`es, as above, and writes are not part of
  `db.transaction`.
- `//! effects: db, env` makes `cache_get` and `cache_incr` the `db` effect (and `env`,
  for `REDIS_URL`), so a function declared `with db, env` may call them and one declared
  `with http` may not.

## Possible or not

The questions people ask before choosing a language, or once they are a week into a
program, answered in a line each, with where the rest is. "No" here means the code
does not do it today; the way around it follows.

### The language

| Can I... | |
|---|---|
| use classes, interfaces, generics or enums? | No. Records (`type User = { ... }`) are checked maps; `"a" \| "b"` is the nearest enum; `list<T>` and map shapes are the generics. See [Records](#records) |
| declare constants? | No `const`: a `let` that nothing assigns. Values themselves never change |
| mutate a list or map in place? | No: `push`, `set`, `+` give new ones, cheaply when nothing else holds the old one. See [Standard library](#standard-library) |
| throw and catch exceptions? | No: failures are error values, `?` passes them on, `??` replaces them. A runtime error ends the request with a 500. See [Errors](#errors) |
| write `async`/`await`, threads or locks? | No need: waits suspend the request and the server goes on; `parallel` overlaps waits. Long computing holds its instance: put it in Rust or run more instances. See [Concurrency](#concurrency-and-reloading) |
| use namespaces or modules? | A file sees its own names, the builtins and what it takes with `use`, and two files may define one name. `private` keeps a name to its file, `mount` and `service` are the boundaries, and there are no prefixes (`auth.login()`). See [Files of a program](#files-of-a-program) |
| recurse deeply? | Up to 10,000 calls; there is no tail-call optimisation |
| rely on integer overflow wrapping, or bitwise operators? | No wrapping: overflow is an error. There are no `& \| ^ << >>` or `**`: `bit.and`, `bit.or`, `bit.xor`, `bit.not`, `bit.shl`, `bit.shr`, and `pow` |
| have the checker prove a `match` covers every case? | Not yet: a value no arm takes fails when it comes |
| format code automatically? | Not yet: there is no formatter |
| use Unicode names? | Yes: `let café = 1` |

### Pages and the browser

| Can I... | |
|---|---|
| send the page before its slow parts, as React's Suspense streams them? | Yes: `state x = defer(load())` streams that component when its data comes, in the same answer. See [Server functions](#server-functions) |
| render on the server and have it interactive in the browser? | Yes, that is what a `component` is: rendered on the server, resumed in the browser without being drawn again. See [Components](#components) |
| render a component only in the browser, or mount one from a script? | No: components start from the server's HTML, or inside another component |
| use npm packages, TypeScript, a bundler? | No: component code is Quillo, compiled to JavaScript by `quillo`; a library is a `<script>` reached with `js(...)`. See [JavaScript libraries](#javascript-libraries) |
| use React-style context, portals, refs, lifecycle hooks? | No context or portals; `@mount` is the one hook (its returned function is the cleanup) and gives the element. See [Composition](#composition) |
| split the bundle by route or load a component lazily? | No: a page's bundle has every component it can show; it loads when someone reaches for it |
| use `setTimeout`, debounce or throttle? | No `setTimeout`: `run_after(ms, f)` for once, and `every(ms, f)` under a state flag for a debounce. See [Components](#components) |
| use `localStorage`, the clipboard, `window`? | Through the page: `js("localStorage.getItem", k)`. Nothing is built in for them |
| use event modifiers like `@click.prevent`? | No: `e.preventDefault()` in the handler |
| scope CSS to a component, or use Tailwind? | Yes: `style` in a component is scoped, a file's `style` styles its components, [`--with`](#options-of-quillo-run) and `style_with` run Tailwind and PostCSS, and `style_with(f)` and `build` run an engine of the program's own, fed by `classes()`. See [HTML literals](#html-literals) |
| paste plain HTML into a page? | Mostly: comments, quotes of either kind, unquoted values and the doctype are taken; unclosed `<p>` and `<li>`, `<style>` and `<script>` braces need changing. See [From HTML](#from-html) |
| translate the interface? | Yes, with plural rules, on both sides. See [Translations](#translations) |
| run the app in a desktop window? | Yes, with a build that has the `app` feature (the release downloads do not). See [Desktop apps](#desktop-apps) |
| compile components to WebAssembly? | Not yet: they compile to JavaScript |

### Serving

| Can I... | |
|---|---|
| serve HTTPS or HTTP/2 from Quillo itself? | No: HTTP/1.1 on plain TCP; a proxy in front (limer, Caddy, nginx) does TLS. See [What a route cannot do](#what-a-route-cannot-do) |
| write middleware? | Yes, as a pipeline stage that `reply`s, or for many routes at once in a `routes` group (prefix, middleware, request pattern, nested; see [Route groups](#route-groups)); there is no global hook, and no custom 500 page |
| answer CORS preflights? | Not by itself: declare `route OPTIONS` and set the headers with `respond` (with no such route, `OPTIONS` gets 204 and `Allow`, no CORS headers) |
| serve a folder of HTML and other files as they are? | Yes: `quillo serve site`, or `files("site", path)` in a route, with `index.html`, clean URLs and `404.html`. See [Static files](#static-files) |
| open WebSockets of my own? | No: `live`, `events` with `listen`, and channels are the ways to push. See [Channels](#channels) |
| receive or send binary data? | Only as files: `file()`, uploads, `image.*`. There is no bytes value; bodies are text |
| stream a response? | Yes: `stream`, `events`. See [Streaming](#streaming) |
| limit how often a client calls? | Not built in: count in a table of your own. See [What a route cannot do](#what-a-route-cannot-do) |
| put a time limit on a route? | No: only sockets time out |
| see the visitor's address behind a proxy? | Name it with [`--trust-proxy`](#options-of-quillo-run), and `request.ip` is the visitor's. Otherwise read `x-forwarded-for` from `request.headers` yourself; Quillo never trusts it |
| run several programs on one port? | Yes, in one process: `mount` or `service /prefix`. See [Mounting programs](#mounting-programs) |
| listen on a Unix socket, or run behind FastCGI? | Yes: [`--listen unix:PATH`](#options-of-quillo-run), and `quillo fcgi` for `mod_fcgid` or nginx's `fastcgi_pass`. See [Quillo on a shared server](#quillo-on-a-shared-server-cpanel) |
| run on shared hosting (cPanel)? | Where the account has `mod_fcgid`, with `quillo fcgi` and plugins built beforehand; not through Passenger. See [Quillo on a shared server](#quillo-on-a-shared-server-cpanel) |
| serve a site as static files? | Yes, when its components call nothing on the server: fetch every page and the bundles it loads, and upload them |
| make requests run once when retried? | Yes: `idempotent`. See [Idempotent requests](#idempotent-requests) |

### Real time

| Can I... | |
|---|---|
| have pages follow the database without writing a socket? | Yes: `live(load)`, over one WebSocket a page. See [Live data](#live-data) |
| see changes made by other programs (`psql`, a cron script, a trigger)? | No: only writes made through Quillo, and `publish`. Have the other program call a route that publishes |
| push to one user? | Yes: a topic of theirs (`watch` and `publish`), or `channel.send` to their page. See [Channels](#channels) |
| use UDP, WebTransport datagrams, live video? | Yes: `udp.*`, channels, `media.*`. See [UDP](#udp), [Media](#media) |
| connect pages to each other (WebRTC)? | No: everything goes through the server, which relays video as an SFU, with no TURN |
| record a stream? | No: have the encoder write a copy |
| relay channels and streams across servers? | No: live data is shared through limer ([`--register`](#options-of-quillo-run)); channels and media are each server's |

### Data

| Can I... | |
|---|---|
| use SQLite, PostgreSQL, MySQL, MariaDB? | Yes, with the same functions, several at once. See [Database](#database) |
| use SQL Server, Oracle, Redis, MongoDB? | Through a Rust crate in a `.qwer` file. See [Other stores](#other-stores) |
| use an ORM or a query builder? | No: SQL with parameters, rows as maps, records to check their shape |
| stream a large result, or `COPY`, `LISTEN`/`NOTIFY`? | No: page with `limit`; many-row `insert`s; `publish` for notifications |
| make a transaction over several databases? | Yes: `db.atomic`, in two phases. Across services: no. See [Several databases](#several-databases) |
| give each customer a database? | Yes: tenants. See [Many tenants](#many-tenants) |
| encrypt SQLite at rest? | No: the disk's encryption, or a server database |
| do exact money arithmetic? | Yes: decimals and `money`. See [Decimals](#decimals) |
| do data science? | Python's `math` and `statistics` and numpy's array operations, on every core; no data frames or plotting. See [Numbers and data](#numbers-and-data) |
| read CSV, XML, YAML, TOML? | Not built in: the catalog's `csv`, `xml`, `yaml` and `toml` plugins, or `split` and `re`. No base64 yet. See [Plugins](#plugins-from-the-catalog) |
| know the visitor's time zone on the server? | Not by itself: the page tells it with a cookie. See [Dates and times](#dates-and-times) |

### Services and scale

| Can I... | |
|---|---|
| split an app into services without changing its code? | Yes: `service`, in one process or [`--split`](#options-of-quillo-run). See [Services](#services) |
| write a service in another language? | Yes, if it answers Quillo's signed calls; the caller keeps a `.qwe` file of its signatures, or the service answers `_contracts`. See [Services](#services) |
| deploy an app without its services' code? | Yes: with an address and no file, the app asks the running service for its functions. See [Services](#services) |
| authenticate services with keys of their own, OAuth 2.0 tokens, JWTs or mutual TLS? | Yes: `auth` where a service is declared, `accept_calls` in it, `for [...]` on a `server fn`. See [Authentication between services](#authentication-between-services) |
| call an API that speaks only HTTP/2, as Apple's push service (APNs) or gRPC? | No: `http.*` speaks HTTP/1.1 only. Use another program for those calls, or an API that takes HTTP/1.1 (Firebase Cloud Messaging's v1 API does) |
| call out through a company's proxy, trust an internal CA, or present a client certificate? | Yes: `HTTPS_PROXY` and `NO_PROXY` are read, and `http.*` takes `ca`, `cert` and `key`. See [HTTP client](#http-client) |
| use every core? | Yes: [`--instances N`](#options-of-quillo-run), each with its own variables. See [Concurrency](#concurrency-and-reloading) |
| run several servers? | Yes, behind limer, sharing a database; live data follows. See [Several servers](#several-servers) |
| describe my API for a gateway? | Yes: `quillo docs`, [`--describe`](#options-of-quillo-run). See [The program's API (OpenAPI)](#the-programs-api-openapi) |
| add a copy of a service while it runs? | Yes: [`--join`](#options-of-quillo-run), or behind limer with [`--register`](#options-of-quillo-run). See [Copies that join](#copies-that-join) |
| deploy without dropping a request? | With [`--reload`](#options-of-quillo-run) for the program's files; a new binary is a stop and a start, so run two servers for no gap |
| keep state in memory across restarts? | No: globals start afresh; keep it in the database |

### Extending and tooling

| Can I... | |
|---|---|
| write part of a program in Rust? | Yes: `.rs` or `.qwer` files, any crate, called like functions. See [Rust](#rust) |
| call C libraries? | Through a Rust `-sys` crate |
| embed Quillo in a Rust program? | It is a library crate, but not published, and its API may change. `quillo::embed::Program` runs a program with no socket, handed each request, as the apps do (on Unix only for now) |
| debug with breakpoints? | Yes, in VS Code or any DAP client, on macOS and Linux, server and browser alike. See [Debugging](#debugging) |
| get editor support? | Yes: `quillo lsp`, and the VS Code extension |
| run untrusted users' code? | Yes, sandboxed: `run_script`. Never as `.qwer`. See [Scripts from users](#scripts-from-users) |
| get structured logs and metrics? | JSON request lines ([`--log json`](#options-of-quillo-run)) and Prometheus metrics ([`--metrics`](#options-of-quillo-run)); errors are text lines. See [Logs and metrics](#logs-and-metrics) |
| configure with a file? | Only `.env`, and a policy that takes away; everything else is an option. See the options at the start of this guide |
| limit what a program may do from outside it? | Yes, with a policy: plugins, effects, hosts, variables, limits. See [The policy](#the-policy) |
| sandbox a program with the system's help (seccomp, Landlock)? | Not yet: a policy holds Quillo code, not a plugin's Rust |

## Not built yet

- `match` exhaustiveness in the checker
- compiling components to WebAssembly (they compile to JavaScript)
- a formatter
- a file's names under a prefix (`use lib/auth as auth`, then `auth.login()`), where `.`
  in markup is already a class
- splitting a page's bundle by route, and components loaded lazily
- a caller finding a service by name through limer's pool, and [`--join`](#options-of-quillo-run) for copies behind NAT
- `{{ ... }}` blocks of JavaScript in a component (a design only)
- in the browser, a map's keys that look like integers keep their order of insertion:
  there `keys({ "10": 1, "2": 2 })` is `["2", "10"]`, as JavaScript orders them
- Linux builds linked with musl, or static: not planned, since a static binary cannot
  load plugins. The releases from 0.7.0 on are linked against glibc 2.17 instead
- a catalog whose entries are signed, its plugins prebuilt for Linux, and `//! effects:`
  on each of them; a policy that allows catalog plugins only (`local = false`)
- `quillo run --sandbox` (seccomp and Landlock on Linux), and plugins built to
  WebAssembly and run sandboxed
- a policy's `private = false` for `db.connect` and UDP (it covers `http.*` only)
- Passenger (cPanel's Application Manager) starting a Quillo program
- phone apps in the stores: uploads to the App Store and Google Play, HarmonyOS NEXT,
  `.qwer` plugins linked into the app, and work while the app is in the background
- `quillo app` without a socket run on Windows and Linux (the code is wry's on all three;
  only macOS has run), and a `stream` route's answer sent in parts to an app's web view
- `quillo build` making a program's `build` declarations ahead of time
- an HTTP/2 client, for APNs and gRPC

## Layout

```
src/main.rs        CLI: run, app, new, check, fix, docs, build, fcgi, repl, lsp, dap, version
src/new.rs         quillo new: a program to start from
src/lib.rs         the crate: what the binary and the tests use
build.rs           stamps the commit into `quillo --version`

The language
src/lexer.rs       tokens; newline handling
src/parser.rs      recursive descent: statements, expressions, patterns, HTML, routes, components
src/ast.rs         the syntax tree, types, effects
src/interp.rs      evaluator, pattern matching, HTML and component rendering, effects
src/jit.rs         numeric functions compiled to machine code (Cranelift)
src/value.rs       values, maps, scopes
src/error.rs       errors with a file and line
src/record.rs      records: type User = { ... }, made, checked, in patterns
src/check.rs       the static checker (quillo check, quillo run, the language server)
src/modules.rs     what each file of a program sees and takes, and `quillo fix`
src/script.rs      run_script: code a program's users write, sandboxed
src/native.rs      Rust in a program: .qwer files read, built, loaded and called;
                   .quillo/plugins.lock and --no-build
src/policy.rs      the policy: what whoever runs Quillo lets a program do

The standard library
src/builtins.rs    the builtins every program can call
src/numeric.rs     math, stats and vec: Python's math and statistics, numpy's operations
src/decimal.rs     dec: exact decimals, for money
src/money.rs       money: amounts in a currency, its minor units and formats
src/dates.rs       date: calendar dates, date-times, time zones
src/re.rs          re: regular expressions, the same on the server and in the browser
src/par.rs         a pool of threads for long lists, in chunks of a fixed size
src/crypto.rs      crypto: hashes, signatures, tokens, passwords
src/imaging.rs     image: what an image file is, and new files made from it
src/highlight.rs   highlight(code, lang): source code as HTML
src/i18n.rs        translations
src/fetch.rs       http: the HTTP client, its cache, and http.stream

Databases
src/db.rs          db: SQLite, connections, tenants, transactions, and the shared API
src/pg.rs          PostgreSQL (pool, pipelining, placeholders, types)
src/my.rs          MySQL and MariaDB (the mysql feature)
src/migrate.rs     db.migrate: schema files applied once each
src/txn.rs         db.transaction's isolation levels, and trying again after a conflict
src/sqlinfo.rs     which tables a statement reads and writes, for live updates

Serving
src/httpd.rs       the event loop: connections, requests as coroutines, waiting off the loop
src/server.rs      request matching, server fns, responses, --watch
src/stream.rs      answers sent as they are made: stream, events, send
src/fcgi.rs        quillo fcgi: FastCGI requests made HTTP requests for the server
src/service.rs     services: in this process or elsewhere, signed calls, retries and
                   circuits, calls made later, --split, --scale, --join
src/ready.rs       GET /_quillo/ready: what a program needs
src/describe.rs    quillo docs and --describe: the API as OpenAPI
src/dotenv.rs      .env files
src/livecache.rs   live data: runs shared between pages, and patches
src/session.rs     cookies and sessions
src/idempotency.rs idempotent(f): requests with an Idempotency-Key run once
src/upload.rs      files sent with a form
src/statics.rs     files served with file()
src/site.rs        what a program sets about its site: tenants, cross-site posts, site_headers
src/sites.rs       site name = URL: calling another Quillo site's server fns
src/jobs.rs        jobs run on a schedule
src/viewcache.rs   reusing views' HTML while their inputs are unchanged
src/ws.rs          WebSockets, for live updates, --watch reloads and channels
src/udp.rs         UDP: udp.listen, udp.send, udp.reply
src/wt.rs          channels between the server and its pages, over WebTransport (the
                   realtime feature) or a WebSocket
src/media.rs       RTP from cameras and encoders, and pages' own cameras, passed on
                   to <quillo-media>
src/cluster.rs     several servers behind limer: --register, broadcasts
src/metrics.rs     what GET /_quillo/metrics reports, and the request log's shape
src/shared.rs      the interpreter lock and other state shared between threads
src/alloc.rs       the memory allocator: mimalloc on event loops and waiting threads,
                   the system's elsewhere
src/stack.rs       room on the stack for deep recursion
src/desktop.rs     quillo app: the program in a native window (the app feature)
src/mobile.rs      a program inside a phone app: copied into the app, served to its web view
src/platform.rs    platform(): where the program runs

The browser and editors
src/client.rs      compiles components to JavaScript
src/client/runtime.js  the browser runtime: values, standard library, signals and markup
src/island.rs      when a page loads its components' code
src/styles.rs      components' and views' own CSS, scoped
src/lsp.rs         the language server
src/debug.rs       quillo dap: the debugger

app/               a sample app to start from (main.qwe, lib/, ui/, migrations/, public/)
tests/             one file per part: the language, routes, records, effects, the
                   checker, numbers, decimals, money, dates, crypto, images, Markdown,
                   translations, migrations, scripts, streams, UDP, Rust (.qwer),
                   services, sessions, uploads, tenants, idempotency, isolation, static
                   files, cached views, compiled numbers, security, several databases,
                   the debugger and the language server; tests/programs/ holds the
                   programs they run, tests/browser/ the pages and Node scripts,
                   tests/support/ free ports and throwaway database servers
scripts/           doctor.sh and doctor.ps1: what a machine needs, checked
deny.toml          cargo-deny's rules for dependencies (run in CI)
```

What `cargo test` needs: Rust and a C compiler; Node (22 or later; 24 for the money
tests, which compare with its CLDR data) for the compiler's tests; Chrome or Chromium
(`QUILLO_CHROME`) for the browser's; PostgreSQL's `initdb` and `pg_ctl`
(`QUILLO_TEST_PG_BIN`) and MariaDB's `mariadbd` (`QUILLO_TEST_MYSQL_BIN`), which it runs
as throwaway servers; a `limer` binary (`LIMER`, or `../limer/limer`) for the several
servers tests; and the network, for the Rust tests' first build of a crate. A test whose
tool is missing says so and is skipped, except the Rust ones. `QUILLO_TEST_LOG=1` shows
the test servers' output.

`quillo` is also a library crate (`quillo::eval_str`, the interpreter, the server, the
checker), used by the binary and the tests; it is not published to crates.io, and its
Rust API makes no promise of staying the same.
