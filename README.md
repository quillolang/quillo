<p align="center">
  <img src=".github/quillo-logo.png" width="96" height="96" alt="Quillo logo">
</p>

<h1 align="center">Quillo</h1>

<p align="center">
  <strong>A web language where routes, HTML, components and databases are the syntax.</strong>
</p>

<p align="center">
  <a href="https://quillolang.org">Website</a> ·
  <a href="https://quillolang.org/docs/">Documentation</a> ·
  <a href="https://github.com/quillolang/examples">Examples</a> ·
  <a href="https://github.com/quillolang/editors">VS Code extension</a> ·
  <a href="https://discord.gg/XRpTQ4uTq">Discord</a> ·
  <a href="https://www.reddit.com/r/quillo/">Reddit</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license"></a>
  <img src="https://img.shields.io/badge/version-0.5.0-informational.svg" alt="Version 0.5.0">
  <img src="https://img.shields.io/badge/written_in-Rust-orange.svg" alt="Written in Rust">
</p>

---

One file holds the server and the browser: what it answers, the pages it renders, the
components that run in the page, and the database they use. Quillo reads like
JavaScript, checks your program before it runs, and serves it faster than Node and Bun.

```quillo
db.open("notes.db")
db.exec("create table if not exists notes (id integer primary key, text text)")

server fn add_note(text: str) -> list with db {
    db.exec("insert into notes (text) values (?)", text)
    db.query("select * from notes order by id")
}

component Notes(initial: list) {
    state notes = initial
    state draft = ""
    <form @submit={() => { notes = add_note(draft); draft = "" }}>
        <input bind:value={draft}/>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
    </form>
}

route GET / = <html><body><Notes initial={db.query("select * from notes")}/></body></html>
```

That is a whole app. The page is rendered on the server; in the browser, the component
takes over the server's HTML as it is (nothing is drawn twice), and calling `add_note`
calls the server: no API to write, no `fetch`, no `async`/`await`.

## Contents

- [Features](#features)
- [Performance](#performance)
- [Getting started](#getting-started)
- [Repository layout](#repository-layout)
- [Documentation](#documentation)
- [Community](#community)
- [Contributing](#contributing)
- [License](#license)

## Features

### The web is the syntax

Routes match paths, query strings and JSON bodies by pattern, with types; requests flow
through pipelines; HTML is a value.

```quillo
route POST /orders { json: { type: "order" | "refund", customer.id: int as id } } {
    reply 201 { accepted: true, customer: id }
}

route GET /feed ? { page: int = 1 } = request | auth.required | load_feed(page) | render_feed
```

### Fine-grained components

A component's code runs once; its state is signals, and each place in the page that
shows a value follows that value alone. Rendered on the server, resumed in the browser
on the server's own HTML. A page loads only what it uses, when someone reaches for it:
8 KB compressed for a simple component, 13 KB for the benchmark's table.

### Databases, all of them, at once

SQLite built in, PostgreSQL and MySQL with the same functions; migrations; pages that
follow the data live as it changes, for everyone. Then, when it grows: a database per
tenant, read replicas, shards, and one transaction over several databases.

```quillo
db.open(env("DATABASE_URL"), { replicas: [env("REPLICA")] })
let users = db.shards([env("SHARD_1"), env("SHARD_2")])
let ledger = db.connect(env("LEDGER_URL"))

fn pay(id, n) = db.atomic([users.on(id), ledger], () => {
    users.on(id).exec("update users set balance = balance - ? where id = ?", n, id)
    ledger.exec("insert into entries (user_id, amount) values (?, ?)", id, -n)
})
```

### Services that hold up

Part of an app can run in the same process or as a server of its own without its code
changing. Calls between them are retried without running twice, spread over several
addresses, kept away from one that is failing, traced by one request id, or made later
in the background until they get through.

```quillo
service billing = "billing.qwe" { timeout: "2s", retries: 2 }

route POST /buy = billing.invoice(request.form.name, 1200)?
route POST /remind = billing.later.send_reminder(request.form.name)
```

### Rust when you want it

A `.qwer` file is Rust: `use "imaging.qwer"` builds it, and its `pub fn`s are the
program's functions, checked against their Rust types, running off the event loop on
every core, built again when they change.

```rust
// imaging.qwer
pub fn brightness(path: &str) -> Result<f64, String> { /* any crate you like */ }
```

### Checked before it runs

`quillo check` knows types, records, effects, routes, components and services, and finds
mistakes without running anything. The language server brings that, with completion and
go-to-definition, to your editor; `quillo dap` adds breakpoints and stepping.

### Real time, over UDP too

A program takes UDP packets on a port, and pages get messages as they happen over
WebTransport, as datagrams that wait for nothing (or reliably, when they must). A
camera's RTP stream plays in any page with `<quillo-media>`, a fraction of a second
behind.

```quillo
udp.listen(9000, (p) => channel.broadcast("readings", json.parse(p.text)))
media.rtp("cam", { video: 5004, audio: 5006 })

component Gauge() {
    state last = null
    channel.open("readings", (r) => { last = r })
    <p>{last.v} <quillo-media stream="cam"/></p>
}
```

### Everything else, in the box

Links prefetched as you point at them (and never doing anything by it), sessions,
uploads, images, translations, streaming, scheduled jobs, metrics, a JavaScript bridge
for browser libraries, multi-tenant hosting, graceful reloads that let requests in
progress finish, and desktop apps in a native window (`quillo app`).

## Performance

**Components.** On js-framework-benchmark's operations, against plain JavaScript (1.00,
the fastest):

| Quillo | Solid | Svelte 5 | Vue 3 | Preact | React 19 |
|---:|---:|---:|---:|---:|---:|
| **1.07** | 1.04 | 1.15 | 1.78 | 1.94 | 2.08 |

**Server.** One process each, requests a second:

| | Quillo | Bun 1.4 | Node 24 |
|---|---:|---:|---:|
| text | **262k** | 225k | 157k |
| JSON | **258k** | 228k | 155k |
| fortunes (sort, escape, render HTML) | **180k** | 169k | 89k |
| arithmetic loop | **26.4k** | 23.4k | 24.4k |

Waiting (on a database, another server, `sleep`) never blocks other requests, and code
is written as if it simply waited.

**Numbers.** A function doing arithmetic is compiled to machine code with Cranelift the
second time it is called, for the types it gets. `fib(32)` takes 11 ms (as in Node;
Bun 21), and a ten-million-step loop 29 ms (Node and Bun: about 280), with the
interpreter's exact answers and errors.

[quillolang/benchmarks](https://github.com/quillolang/benchmarks) has how the benchmarks run, and every number.

## Getting started

Quillo builds with a stable Rust toolchain, 1.94 or later ([rustup](https://rustup.rs)),
and a C compiler ([what to install](https://quillolang.org/docs/install/) on macOS, Linux and
Windows). `scripts/doctor.sh` checks for both, says what to install for anything
missing, and lists what the tests use (Node, Chrome, PostgreSQL, MariaDB):

```sh
git clone https://github.com/quillolang/quillo.git
cd quillo
sh scripts/doctor.sh            # add --build to also build and run the sample app
cargo build --release
```

On Windows, in PowerShell, the check is `scripts\doctor.ps1`:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\doctor.ps1 -Build
```

Run the sample app in [`app/`](app), then check it without running it:

```sh
./target/release/quillo run app/main.qwe --watch   # http://127.0.0.1:8080
./target/release/quillo check app/main.qwe
```

`app/` is a small to-do app laid out the way larger programs are; edit it while it runs
to start your own:

```
app/
  main.qwe              the program: its database, the files it uses, its routes
  lib/todos.qwe         the Todo record, queries and server functions
  ui/page.qwe           the frame every page uses
  ui/todos.qwe          a component, rendered on the server and run in the browser
  migrations/           SQL, run once, in order
  public/               served under /static/: css/, js/ (called from the component), icons/
```
 More programs, each showing one part of
Quillo, are in [quillolang/examples](https://github.com/quillolang/examples).

`cargo install --path .` puts `quillo` on your `PATH` instead.

For your editor, install the [VS Code extension](https://github.com/quillolang/editors): highlighting,
diagnostics, completion, go to definition and debugging.

## Repository layout

| Path | What it holds |
|---|---|
| [`src/`](src) | The `quillo` binary: interpreter, number compiler, HTTP server, checker, component compiler, database drivers, language server and debug adapter |
| [`app/`](app) | A sample app to start from: several files, migrations, CSS, JavaScript and an icon |
| [`scripts/`](scripts) | `doctor.sh` (macOS, Linux) and `doctor.ps1` (Windows): check that a machine can build and run Quillo |
| [`tests/`](tests) | Integration and browser tests; [`tests/programs`](tests/programs) holds the programs they run |

Everything else has a repository of its own:

| Repository | What it holds |
|---|---|
| [quillolang/examples](https://github.com/quillolang/examples) | Small programs, each showing one part of Quillo: routes, a todo app, live notes, a single-page app, infinite scroll, a virtual list of a million rows, the Game of Life, UDP telemetry, a camera stream, Rust in a program, services |
| [quillolang/apps](https://github.com/quillolang/apps) | Whole applications: a multi-tenant CRM with a shop, campaigns, a page designer and plugins |
| [quillolang/editors](https://github.com/quillolang/editors) | The VS Code extension |
| [quillolang/website](https://github.com/quillolang/website) | [quillolang.org](https://quillolang.org), written in Quillo, with the logo and banners |
| [quillolang/benchmarks](https://github.com/quillolang/benchmarks) | The browser and server benchmarks |

## Documentation

The [user guide](USERGUIDE.md), also at [quillolang.org/docs](https://quillolang.org/docs/),
has all of it: [the language](USERGUIDE.md#language-tour),
[components](USERGUIDE.md#components), [routes](USERGUIDE.md#routes),
[the standard library](USERGUIDE.md#standard-library) and
[databases](USERGUIDE.md#database), [services](USERGUIDE.md#services),
[checking](USERGUIDE.md#checking), and
[running in production](USERGUIDE.md#concurrency-and-reloading). The site also has
[Installing](https://quillolang.org/docs/install/) and
[Hosting](https://quillolang.org/docs/hosting/): a proxy for HTTPS, systemd, Docker,
the database, and a checklist before going live.

## Community

- **[Discord](https://discord.gg/XRpTQ4uTq)**: questions, help, and what people are building
- **[r/quillo](https://www.reddit.com/r/quillo/)**: news, show and tell, longer discussions
- **[GitHub issues](https://github.com/quillolang/quillo/issues)**: bugs and feature requests

## Contributing

Quillo is young, and the language may still change before 1.0. Issues and pull
requests are welcome. Before sending a change:

```sh
cargo test
for f in app/*.qwe tests/programs/*.qwe; do ./target/release/quillo check "$f"; done
```

## License

Quillo is released under the [MIT License](LICENSE).
