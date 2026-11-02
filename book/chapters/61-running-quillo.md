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

`quillo run` checks the program, executes its top level and, if it declared routes or
`server fn`s, serves them; a program with neither ends when its top level has run
(under `--watch` it is served all the same). The program is the first argument that is
not an option, wherever it stands (`quillo run --watch main.qwe` works); left out, or
given a folder, it is that folder's `main.qwe`, so `quillo run` in a program's folder
runs it. `quillo app.qwe` alone is `quillo run app.qwe`. An option's value follows it,
or `=` (`--port 8081`, `--port=8081`); `-w` is the one short option (`-p` is not
`--port`); `-h` prints the usage; a mistyped option is told the nearest one there is:

| Option | |
|---|---|
| `--port N`, `--host H` | where to listen (default `$PORT` or 8080, on 127.0.0.1); the port is 0 to 65535, 0 for any free one (`platform().server.port` says which), and `--host localhost` is 127.0.0.1 |
| `--listen unix:PATH` | listen on a Unix socket instead of a TCP port, for a web server on the same machine: made with mode 0600, removed when the server stops, and at most 100 bytes long (a short path relative to the folder `quillo` runs in). A `$PORT` that holds an absolute path (`/home/u/app.sock`, or `unix:/home/u/app.sock`) is taken the same way. Not with `--split`, `--instances`, `--register` or `--join` (see [Quillo on a shared server](#quillo-on-a-shared-server-cpanel)) |
| `--socket-mode 660` | the socket's permissions, in octal; the owner must keep read and write (`600`, the default, or `660` for a web server in the owner's group) |
| `--policy FILE` | narrow what the program may do, after `/etc/quillo/policy.toml` and before the `quillo.policy` beside it; may be given more than once (see [The policy](#the-policy)) |
| `--no-build` | never build a Rust file here: load only libraries `quillo build` made, as `.quillo/plugins.lock` names them (see [Rust](#rust)) |
| `--watch`, `-w` | reload when the program or a `use`d file changes (a symbolic link pointed elsewhere, to a file or a folder, counts, within a second), and refresh open pages; a file the program served with `file()` that changes refreshes them too, and a stylesheet is swapped in them as they are, keeping what they show (for one another tool writes, such as Tailwind's) |
| `--reload` | reload when files change, as in production: no errors shown on pages, no page refreshes |
| `--max-compute 2s` | end a request that computes this long without letting go, as a 500 (see Concurrency) |
| `--trust-proxy 10.0.0.0/8` | believe `X-Forwarded-For` from these addresses (see [Live data](#live-data)) |
| `--live-per-client N` | pages one client may have waiting for live data (default 256) |
| `--instances N` | run the program N times over, one event loop each, to use N cores (see Concurrency); not with `--watch`, `--register` or `--split`; more than the machine's cores is warned of |
| `--workers N` | requests that can wait at once (for a database, the network), each on a thread (default 64) |
| `--no-check` | skip the static checks |
| `-- a b` | after the program, what follows is the program's: `args()` gives `["a", "b"]`, for a script; before it, `--` ends the options (`quillo run -- a.qwe x`). A stray word elsewhere is told it goes after `--` |
| `--allow-path DIR` | `read_file` and `write_file` may use `DIR` too, as a script used as a tool needs (see [Scripts as tools](#scripts-as-tools)); any part of the program may then, routes too |
| `--split` | run each `service` as a process of its own (see [Services](#services)) |
| `--service-url NAME=URL` | where service NAME runs, when not here (also `QUILLO_SERVICE_<NAME>`, the name in capitals); several addresses, comma-separated, are taken in turn |
| `--base PATH` | the path the program is served under (`/shop`): its links and redirects carry it |
| `--as-service` | serve the program as a service: its `server fn`s answer other services' signed calls, not pages (see [Services](#services)) |
| `--service-name NAME` | the name other programs call this service by, when it is not the program's file name; calls signed for another service are refused |
| `--no-view-cache` | render views every time (see [Cached views](#cached-views)) |
| `--no-jobs` | run none of the program's scheduled jobs on this server |
| `--no-prefetch` | pages do not prefetch the links pointed at (see [Prefetching](#routes-prefetching)) |
| `--plain-http` | the site is served over plain HTTP on purpose, on a network you trust (a factory's kiosks at `http://10.0.0.5:8080`): cookies are not `Secure`, which browsers would drop there (see [Cookies and sessions](#cookies-and-sessions)) |
| `--register URL`, `--service S` | join a limer pool, and share live updates with its other servers; needs `LIMER_ADMIN_SECRET` (see [Several servers](#several-servers)) |
| `--max-body N` | largest request body to accept, in bytes (default 10 MiB; larger ones get 413) |
| `--wt-port N` | the UDP port pages connect to over WebTransport (default: the HTTP port's number; see [Channels](#channels)) |
| `--tls-cert F`, `--tls-key F` | the certificate WebTransport presents, instead of one of its own; the files must exist |
| `--no-wt` | no WebTransport: pages' channels and streams use a WebSocket |
| `--log F` | the request log: `text` (the default), `json` (one object per line) or `off` (see [Logs and metrics](#logs-and-metrics)) |
| `--metrics` | answer `GET /_quillo/metrics` in Prometheus's text format |
| `--query-stats` | time database queries for `GET /_quillo/health/queries`: the last 100, overall and per statement (on under `--watch`; see [Stopping and health](#stopping-and-health)) |
| `--scale NAME=N` | with `--split`: run service NAME as N copies (1 to 64), called in turn (see [Services](#services)) |
| `--join URL` | with `--as-service` and a `--port`: join the app at URL as one more copy of this service while it runs, signed with `QUILLO_SERVICE_KEY` when it is set (see [Copies that join](#copies-that-join)) |
| `--join-as URL` | with `--join`: the address the app calls this copy at (`https://billing-3.internal:8443`); it must lead to where the copy joined from |
| `--no-join`, `--allow-join A,B` | take no copies that join, or only copies of these services |
| `--describe` | answer `GET /_quillo/describe` with the program's API, as `quillo docs` writes it |
| `--env-file F` | the settings file to read (a path from the folder `quillo` runs in; it must exist), instead of the `.env` beside the program (which need not) |
| `--with CMD` | run a command beside the program (through `sh -c`, in its folder), its lines marked with its name, stopped with it however it ends, a Ctrl-C or `SIGTERM` while it loads, a panic and a `SIGKILL` too (more than once for several): `--with "npx tailwindcss -i src/app.css -o public/app.css --watch"` |

`quillo new <folder>` writes a program to start from. By default it is the to-do app, the
eight files of `app/`: a database with a migration, a component, files served.
`--minimal` writes three: `main.qwe`, `lib/greet.qwe` and `ui/page.qwe`, a page, a
component and a function both sides use. Both get a `.gitignore` (`*.db`, `*.db-wal`,
`*.db-shm`, `.env`, `.env.*`, `.quillo/`), and `--binary` copies this `quillo` into
`<folder>/.quillo/bin/quillo`. It refuses a folder that is not empty (an empty one, or one
holding only `.git`, is used) and a name Windows refuses (`con`, `aux`, `nul`, `prn`,
`com1` to `com9`, `lpt1` to `lpt9`, any of `<>:"|?*\`, a trailing dot or space, a line
break). It writes into a folder beside the target and moves it in place at the end, so a
failed `new` leaves nothing half made, and it never writes an empty folder. It warns when
the folder is inside another program, and prints the next step:

```sh
quillo new hello --minimal
cd hello && quillo run --watch
```

In VS Code, *Quillo: New program* (extension 0.14.0) asks for the to-do app or the minimal
one and for a folder, runs `quillo new` and opens the folder.

The other commands: `quillo check` (below, and [Checking](#checking)), `quillo docs`
(the API as OpenAPI 3, to stdout; see [Services](#services)), `quillo build` (the Rust
files; see [Rust](#rust)), `quillo fix` (the `import` lines a program's files need, and 0.6's lines turned around; see
[Files of a program](#files-of-a-program)), `quillo serve <folder>` (a folder of files as
they are, as a static host serves it; see [Static files](#static-files)), `quillo classes
<file.qwe>` (the classes the program's markup writes, one a line; see [HTML
literals](#html-literals)), `quillo fcgi <file.qwe>` (the program behind FastCGI, for
`mod_fcgid` or nginx's `fastcgi_pass`; see [Quillo on a shared
server](#quillo-on-a-shared-server-cpanel)), `quillo repl`, `quillo lsp`, `quillo dap`,
`quillo version` (or `--version`, `-V`), which names the commit: from a source archive
it reads the `COMMIT` file `git archive` writes, else says `(commit unknown)`. `quillo
<file.qwe>` is `quillo run <file.qwe>`, options and all, whatever the case of its
extension (`quillo MAIN.QWE`). A misspelt command is told the nearest (``unknown command
`chek` (did you mean `check`?)``), and a file of another extension given with no command is
told how to run it (`to run the file: quillo run tool`). `quillo run` given a file that is
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
a policy does not say). An option that takes a value refuses another option
in its place: `--service-name --watch` says `needs a value, and --watch is an option`. A stopped server exits 0 (see [Stopping and
health](#stopping-and-health)).

The `.env` beside the program is read into its environment as it starts: `KEY=value`
lines (`export` allowed, `#` for comments, and ` #` ends a bare value; values in `"..."`
take `\n`, `\t`, `\"` and `\\`, and `'...'` is as written; a quoted value goes on to its
closing quote across lines, as a PEM key does; a byte-order mark at the start is skipped;
a key is letters, digits and `_`, and a line that is not `KEY=value` is skipped and said
at the start: `quillo: .env:3: not KEY=value, skipped`), for `env("API_KEY")` and for Quillo's own
settings (`PORT`, `QUILLO_SECRET`). A variable the environment already has wins, so a
`.env` left on a server cannot override what production sets. It is read once: a
`--reload` does not read it again. A service reads the `.env` beside its own file as
well, in one process and under `--split` alike: its values come before the app's file's
(the app's reach it too), and after the environment's. Keep `.env` out of version
control.

Everything Quillo reads from the environment:

| Variable | |
|---|---|
| `PORT` | the port, when `--port` is not given; an absolute path is a Unix socket to listen on, as `--listen unix:` |
| `QUILLO_SECRET` | signs sessions (see [Cookies and sessions](#cookies-and-sessions)); without it, a key made at start |
| `QUILLO_QUERY_STATS` | `1`: query stats on from the start, as `--query-stats` |
| `QUILLO_HEALTH_TOKEN` | lets a request with `Authorization: Bearer <it>` see each statement's query stats and switch them on or off; 16 characters at least |
| `QUILLO_DB` | the database when the program does not call `db.open`; without it, SQLite in memory, gone at restart |
| `QUILLO_SYSTEM_DB` | where Quillo's own tables go with a database per tenant (see [Quillo's own tables](#standard-library-quillos-own-tables)) |
| `QUILLO_SERVICE_SECRET`, `QUILLO_SERVICE_<NAME>` | signs calls between services (several, comma-separated: the first signs, any is taken); where service NAME runs (see [Services](#services)) |
| `QUILLO_SERVICE_SECRET_PREVIOUS` | a secret still taken while `QUILLO_SERVICE_SECRET` is changed (see [Authentication between services](#authentication-between-services)) |
| `QUILLO_SERVICE_KEY` | a service's own key: a copy joins with it, and it proves the copy is that service (see [Copies that join](#copies-that-join)) |
| `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` | the proxy `http.*` calls go through, and the hosts that do not (see [HTTP client](#http-client)) |
| `LIMER_ADMIN_SECRET` | for `--register` (see [Several servers](#several-servers)) |
| `QUILLO_NATIVE_CACHE`, `CARGO` | where `.qwer` files are built, and with what (see [Rust](#rust)) |
| `PUBLIC_...` | any such variable may be read in a component (see [What the browser can reach](#what-the-browser-can-reach)) |
| `QUILLO_CHROME`, `QUILLO_TEST_PG_BIN`, `QUILLO_TEST_MYSQL_BIN`, `QUILLO_TEST_LOG`, `LIMER` | `cargo test`: where Chrome, PostgreSQL, MariaDB and limer are; show the test servers' output |

The one other configuration is the policy, which only takes away: `/etc/quillo/policy.toml`,
`--policy FILE` and `quillo.policy` beside the program (see [The policy](#the-policy)).
`HOST` is not read (`--host` is), and `RUST_LOG`
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

The release downloads are built without it. It takes the options of `run` except
`--host`, `--register` and `--service`, reads the `.env` beside the program (or
`--env-file`) as `run` does, keeps `--with` commands going until the window closes, and
takes `--open PATH`, the page the window opens on
(`--open /admin`; `/` by default). The program runs in the window's process with no
HTTP server: nothing listens, and nothing else on the machine can reach it. The window
asks for its pages through a scheme of its own, `quillo://app/` (`https://quillo.localhost/`
on Windows), and the program answers each request as a server would, so routes,
sessions, `server fn`s, uploads and live data work as in a browser (Phone apps, below,
says what differs). The origin is the same at every launch, so what a page keeps in
`localStorage` and IndexedDB is there at the next one. The web view
keeps no cookies for the scheme, so the program's are kept for it in
`.quillo/app-cookies.json` beside the program, and its session key in
`.quillo/.quillo-secret` unless `QUILLO_SECRET` is set: someone signed in stays signed in
from one launch to the next. `--port` has no effect. Links to other sites open in the
system's browser. Only macOS has run this way; Windows and Linux take the same code
through wry, not yet run.

Editor support: [quillolang/editors](https://github.com/quillolang/editors) has a VS Code extension (highlighting,
diagnostics as you type, quick fixes, hover, go to definition, completion, outline, snippets,
end tags added as you type a start tag's `>` or `</`, and a debugger: see [Debugging](#debugging)).
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
variables in scope at the cursor first. It runs the `quillo` it finds (its `quillo.path`
setting, the workspace's `target/release`, then the `PATH`), so stale errors on code the
build accepts come from an old binary: `quillo --version` names the commit, and
`scripts/doctor.sh` points out an old one. Other editors can run `quillo lsp` (hover,
go to definition (of names, and of classes to their CSS rules), references and rename
of classes (also from the CSS files under `public/`, when the client sends them), document symbols,
completion after `.`, `<`, `</` and `@` (and of classes after `<tag.` and in `class="`), diagnostics on
open, change and save, quick fixes as code actions, and `quillo/closeTag`: at a position
just after a start tag's `>` or after `</`, the text to insert there, or null; `initializationOptions: { "strictNull": true }` for the extra
warnings) and `quillo dap` directly.
