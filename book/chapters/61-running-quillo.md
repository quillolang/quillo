# Running quillo

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

This guide is for Quillo 0.8.3, and [CHANGELOG.md](CHANGELOG.md) says what it changed.
Features marked *(since 0.7)* came in 0.7.0. On an older release, read that release's guide:
https://github.com/quillolang/quillo/blob/v0.7.0/USERGUIDE.md or
https://github.com/quillolang/quillo/blob/v0.6.0/USERGUIDE.md.

## Options of `quillo run`

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
| `--no-prefetch` | when links must not be fetched before they are clicked (to measure, or a site whose `GET`s are counted): turn prefetching off (see [Prefetching](#routes-prefetching)) |
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

## Other commands

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

## Settings and the environment

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
| `QUILLO_SYSTEM_DB` | where Quillo's own tables go with a database per tenant (see [Quillo's own tables](#standard-library-quillos-own-tables)) |
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

## Desktop apps

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
