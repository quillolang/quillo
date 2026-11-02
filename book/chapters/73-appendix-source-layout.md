# Appendix C: How Quillo is built {#layout}

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
