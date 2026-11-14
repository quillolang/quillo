# Possible or not

The questions people ask before choosing a language, or once they are a week into a
program, answered in a line each, with where the rest is. "No" here means the code
does not do it today; the way around it follows.

## The language

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

## Pages and the browser

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

## Serving

| Can I... | |
|---|---|
| serve HTTPS or HTTP/2 from Quillo itself? | No: HTTP/1.1 on plain TCP; a proxy in front (limer, Caddy, nginx) does TLS. See [What a route cannot do](#what-a-route-cannot-do) |
| write middleware? | Yes, as a pipeline stage that `reply`s, or for many routes at once in a `routes` group (prefix, middleware, request pattern, nested; see [Route groups](#routes-route-groups)); there is no global hook, and no custom 500 page |
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

## Real time

| Can I... | |
|---|---|
| have pages follow the database without writing a socket? | Yes: `live(load)`, over one WebSocket a page. See [Live data](#live-data) |
| see changes made by other programs (`psql`, a cron script, a trigger)? | No: only writes made through Quillo, and `publish`. Have the other program call a route that publishes |
| push to one user? | Yes: a topic of theirs (`watch` and `publish`), or `channel.send` to their page. See [Channels](#channels) |
| use UDP, WebTransport datagrams, live video? | Yes: `udp.*`, channels, `media.*`. See [UDP](#udp), [Media](#media) |
| connect pages to each other (WebRTC)? | No: everything goes through the server, which relays video as an SFU, with no TURN |
| record a stream? | No: have the encoder write a copy |
| relay channels and streams across servers? | No: live data is shared through limer ([`--register`](#options-of-quillo-run)); channels and media are each server's |

## Data

| Can I... | |
|---|---|
| use SQLite, PostgreSQL, MySQL, MariaDB? | Yes, with the same functions, several at once. See [Database](#standard-library-database) |
| use SQL Server, Oracle, Redis, MongoDB? | Through a Rust crate in a `.qwer` file. See [Other stores](#other-stores) |
| use an ORM or a query builder? | No: SQL with parameters, rows as maps, records to check their shape |
| stream a large result, or `COPY`, `LISTEN`/`NOTIFY`? | No: page with `limit`; many-row `insert`s; `publish` for notifications |
| make a transaction over several databases? | Yes: `db.atomic`, in two phases. Across services: no. See [Several databases](#standard-library-several-databases) |
| give each customer a database? | Yes: tenants. See [Many tenants](#many-tenants) |
| encrypt SQLite at rest? | No: the disk's encryption, or a server database |
| do exact money arithmetic? | Yes: decimals and `money`. See [Decimals](#decimals) |
| do data science? | Python's `math` and `statistics` and numpy's array operations, on every core; no data frames or plotting. See [Numbers and data](#numbers-and-data) |
| read CSV, XML, YAML, TOML? | Not built in: the catalog's `csv`, `xml`, `yaml` and `toml` plugins, or `split` and `re`. Base64: the `encoding` plugin. See [Plugins](#plugins-from-the-catalog) |
| know the visitor's time zone on the server? | Not by itself: the page tells it with a cookie. See [Dates and times](#dates-and-times) |

## Services and scale

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

## Extending and tooling

| Can I... | |
|---|---|
| write part of a program in Rust? | Yes: `.rs` or `.qwer` files, any crate, called like functions. See [Rust](#rust) |
| call C libraries? | Through a Rust `-sys` crate |
| embed Quillo in a Rust program? | It is a library crate, but not published, and its API may change. `quillo::embed::Program` runs a program with no socket, handed each request, as the apps do (on Unix only for now) |
| debug with breakpoints? | Yes, in VS Code or any DAP client, on macOS and Linux, server and browser alike. See [Debugging](#debugging) |
| get editor support? | Yes: `quillo lsp`, and the VS Code extension |
| run untrusted users' code? | Yes, sandboxed: `run_script`. Never as `.qwer`. See [Scripts from users](#scripts-from-users) |
| get structured logs and metrics? | JSON request lines ([`--log json`](#options-of-quillo-run)) and Prometheus metrics ([`--metrics`](#options-of-quillo-run)); errors are text lines. See [Logs and metrics](#logs-and-metrics) |
| configure with a file? | Only `.env`, and a policy that takes away; everything else is an option. See the options in [Running quillo](#running-quillo) |
| limit what a program may do from outside it? | Yes, with a policy: plugins, effects, hosts, variables, limits. See [The policy](#the-policy) |
| sandbox a program with the system's help (seccomp, Landlock)? | Not yet: a policy holds Quillo code, not a plugin's Rust |
