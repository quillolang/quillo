# Introduction


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


## The web is the syntax

Routes match paths, query strings and JSON bodies by pattern, with types; requests flow
through pipelines; HTML is a value.

```quillo
route POST /orders { json: { type: "order" | "refund", customer.id: int as id } } {
    reply 201 { accepted: true, customer: id }
}

route GET /feed ? { page: int = 1 } = request | auth.required | load_feed(page) | render_feed
```

## Fine-grained components

A component's code runs once; its state is signals, and each place in the page that
shows a value follows that value alone. Rendered on the server, resumed in the browser
on the server's own HTML. A page loads only what it uses, when someone reaches for it:
9 KB compressed for a simple component, 13 KB for the benchmark's table.

## Exact money

`0.1m + 0.2m == 0.3m`: decimals are a type of their own, exact from PostgreSQL's
`numeric` to the page, never mixed with floats by accident; `money` keeps amounts in
their currency's minor units, splits them without losing a cent, and writes them as
each locale does.

```quillo
let total = money.mul(money.of(19.99m, "USD"), 3)       // 59.97
money.allocate(total, [1, 1, 1]) | map((m) => money.format(m, "de-DE"))
// ["19,99 $", "19,99 $", "19,99 $"]
```

## Databases, all of them, at once

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

## Services that hold up

Part of an app can run in the same process or as a server of its own without its code
changing. Calls between them are retried without running twice, spread over several
addresses, kept away from one that is failing, traced by one request id, or made later
in the background until they get through. A program is described as OpenAPI for a
gateway (`quillo docs`), and a service is given more copies while it runs (`--join`).

```quillo
service billing = "billing.qwe" { timeout: "2s", retries: 2 }

route POST /buy = billing.invoice(request.form.name, 1200)?
route POST /remind = billing.later.send_reminder(request.form.name)
```

## Rust when you want it

A `.qwer` file is Rust: `use "imaging.qwer"` builds it, and its `pub fn`s are the
program's functions, checked against their Rust types, running off the event loop on
every core, built again when they change.

```rust
// imaging.qwer
pub fn brightness(path: &str) -> Result<f64, String> { /* any crate you like */ }
```

## Checked before it runs

`quillo check` knows types, records, effects, routes, components and services, and finds
mistakes without running anything. The language server brings that, with completion and
go-to-definition, to your editor; `quillo dap` adds breakpoints and stepping.

## Real time, over UDP too

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

## Everything else, in the box

Links prefetched as you point at them (and never doing anything by it), sessions,
uploads, images, translations, streaming, scheduled jobs, metrics, a JavaScript bridge
for browser libraries, multi-tenant hosting, graceful reloads that let requests in
progress finish, desktop apps in a native window (`quillo app`), and, in preview, the
same program inside an iOS app, served on the phone to the app's own web view (`mobile/`).

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
