# Microservices

In Quillo a microservice is a `service`: a program whose `server fn`s other programs call
as if they were their own functions. The same code runs in one process or split across
servers, so an app can start as one program and be split when there is a reason to,
without its code changing. This page puts together what [Routes](#services)
says about services; [examples/services](https://github.com/quillolang/examples/tree/main/services) is a shop that bills
through a billing service.

## Declaring a service

`service` names a program and gives it a name to call it by. Its `server fn`s are then
functions of that name:

```quillo
// shop.qwe
service billing = "billing.qwe"

route GET / =
    <form method="post" action="/buy">
        <input name="name" placeholder="Your name"/>
        <button>Buy for $12.00</button>
    </form>

route POST /buy {
    let inv = billing.invoice(request.form.name, 1200)?    // billing's `server fn invoice`
    <p>Invoice #{inv.id}</p>
}
```

The other side is a program with the `server fn`, in a file beside it:

```quillo
// billing.qwe
let invoices = []

server fn invoice(name: str, cents: int) -> map ! str {
    if trim(name) == "" { return err("An invoice is for someone") }
    let inv = { id: len(invoices) + 1, name, cents }
    invoices = invoices + [inv]
    inv
}
```

`quillo run shop.qwe` serves the form; sending it with a name gives `Invoice #1`, then
`#2`, the same with `quillo run shop.qwe --split`, where billing runs as a process of its
own. A blank name
is billing's error value, which `?` makes the answer: a 500 with `{"error":"An invoice
is for someone"}`. To answer with a status of your own, use `??` (see [What crosses the
wire](#what-crosses-the-wire)).

The app is made of its services. With a path, a service's routes are served under it too,
and it is named after its file:

```quillo
// app.qwe
service /shop = "shop.qwe"
service /billing = "billing.qwe"
```

A program with `server fn`s and no routes is served as well: a service may be only that.
Services may use one another.

## One process or many

How the app runs is said when it is started, with options of `quillo run` after the
program's file: `quillo run app.qwe --split`, `quillo run billing.qwe --port 8081
--as-service`. Every word starting with `--` on this page is one of them; [the
options](#running-quillo) lists them all.

- **In one process**, the default, a service is loaded once and shared by every program
  in the process that names it: one copy of its variables and of its database. A call
  goes straight to it, with nothing on the network.
- **Split**, a service runs on a server of its own, and its calls go there as signed
  HTTP requests. Run alone, a service is an ordinary program; `--base`
  (`quillo run billing.qwe --base /billing`) is the path it is served under, which its links and redirects carry.
- **`--split`** runs each service the app declares (and those they declare) as a
  process of its own, each on its own port, its log lines marked with its name, all
  signed with one secret. They stop with the app. `--describe`, `--no-build` and the
  `--policy` files, given to the app (`quillo run app.qwe --split --no-build --policy
  prod.toml`), are given to each of them too. It is the quickest
  way to see an app as it would run split, on one machine.
- **Checked once.** Before `--split` starts any service, the app checks the whole tree,
  its own files and every service's, and says each warning once. An error in a service
  stops it there, before anything waits. A service that ends as it starts is said at
  once: `service billing stopped at start (exit status: 1; see its lines above)`.
- **`quillo run app.qwe --split --watch`** watches the services' files too: a function added to
  `billing.qwe` reloads billing's process and the app, and the app calls it at once.

```sh
# Everything in one process
quillo run app.qwe

# Each service as a process of its own, on this machine
quillo run app.qwe --split

# Split across servers: billing on 10.0.0.5, the app elsewhere
QUILLO_SERVICE_SECRET=... quillo run billing.qwe --port 8081 --base /billing
QUILLO_SERVICE_SECRET=... quillo run app.qwe --service-url billing=http://10.0.0.5:8081
```

`--service-url billing=...` can also be given as `QUILLO_SERVICE_BILLING`. Every service
and every caller needs the same `QUILLO_SERVICE_SECRET`, unless the call proves who makes
it another way (a key of the caller's own, a token, a certificate: see [Authentication
between services](#microservices-authentication-between-services)); without either, a call to a
service that runs elsewhere is an error value saying so, not an unsigned request.

## What crosses the wire

What crosses is data, as JSON, whichever way the service runs: arguments and results are
copied, and a function arrives as `null`. A program therefore behaves the same in one
process and split. Optional arguments left out are not sent, so the service's own
defaults apply.

A call that cannot be made (the service is not answering, every address is failing) is
an error value, like any other: `?` passes it on and `??` replaces it.

```quillo
route POST /buy {
    let inv = billing.invoice(request.form.name, 1200) ?? reply 503 "Billing is busy; try again in a moment"
    <p>Invoice #{inv.id}</p>
}
```

## The payload

A call sends its arguments and gets a value back. What goes each way, exactly:

- **Arguments** go as a JSON array, in the order of the parameters. An optional argument
  left out is not sent, so the service's own default applies, whichever version of the
  service answers.
- **A record** (a `type`) goes as the map of its fields. The service's parameter type
  checks it on arrival, as it checks a page's call, and fills the defaults the type
  declares: `billing.invoice({ customer: "Zed", cents: 5 })` arrives with `note: ""`
  when `Order` says so. The same record type is declared in both files, since each is a
  program of its own; a `use`d file can hold it for both.
- **Numbers** keep their kind: an int stays an int, a float a float (`2.0` is not
  turned into `2`), a decimal a decimal with its scale (`12.50`). Strings, booleans,
  lists, maps and `null` go as they are.
- **A function cannot go.** Across the network it arrives as `null`. Send what the
  function would have computed, or a name the service understands.
- **The result** is the function's value, or its error: `?` and `??` work on it as on
  any call. A wrong argument (a `str` for an `int`) is refused by the service before the
  function runs, with the same message `quillo check` would have given.

```quillo
// billing.qwe
type Order = { customer: str, cents: int, note: str = "" }

server fn invoice(o: Order) -> map ! str {
    if trim(o.customer) == "" { return err("An invoice is for someone") }
    ...
}

// shop.qwe, calling it
type Order = { customer: str, cents: int, note: str = "" }

route POST /buy { json: Order } {
    let inv = billing.invoice(Order(request.json)) ?? reply 400 "Billing refused the order"
    { invoice: inv }
}
```

What this is on the wire, for a balancer's rules, a log, or a service written in
another language: `POST <service>/_quillo/service/<fn>` with the arguments as the JSON
body and these headers:

| Header | Carries |
|---|---|
| `x-quillo-service` | `v=3,kid=<caller>,t=<unix seconds>,sig=<hex>`: the signature's version, the caller's key id (empty under the shared secret), the time and the signature |
| `x-quillo-to` | the service's name, as the caller knows it |
| `x-quillo-call` | the call's id, up to 64 letters and digits, remembered by the service |
| `x-quillo-caller` | the calling program's name |
| `x-quillo-deadline` | milliseconds the caller still waits |
| `x-quillo-contract` | the function's contract as the caller's copy declares it |
| `x-quillo-tenant` | the tenant, when there is one |
| `x-request-id` | the request's id, passed down the chain |
| `authorization` | `Bearer <token>`, when the caller proves itself with a token |
| `x-quillo-forward-<name>` | a header of the visitor's request the caller passes on (`forward`) |

The signature is HMAC-SHA256, under `QUILLO_SERVICE_SECRET` or the caller's own key, of
the text `quillo-service-3`, the time, the service, the function, the tenant, the call
id, the key id, the caller, the deadline and the contract, each followed by a line
break, then the body. The time must be within five minutes. Services still take
version 2 (`t=...,sig=...`, over `quillo-service-2`, the time, the service, the
function, the tenant and the call id), which does not sign the caller, the deadline or
the contract; a caller of this version sends version 3, which an older service refuses,
so upgrade the services before their callers. The answer comes back signed with the
key the call was signed with: `x-quillo-answer: sig=<hex>`, HMAC-SHA256 of
`quillo-answer-3`, the call id and the status, each on a line, then the body.
The answer is `{"ok": value}` or `{"err": payload}`, with `"published": [...]` when the
call published topics. Otherwise: 400 for an argument of the wrong type, 401 for a bad
or old signature or a call meant for another service, 404 for no such function, 409
when the contracts differ (the answer says both), 503 while the same call is still
running. A call sent twice with the same id gets the same answer and runs once while
the service stays up. Only an answer of 200 is remembered.

A copy that [joins](#microservices-copies-that-join) the app does so at `POST <app>/_quillo/join`,
with `{ "service": "billing", "port": 8081, "leave": false }` as the body (and `"url"`,
from the copy's `--join-as`; see [Copies that join](#microservices-copies-that-join)), signed as a service call is, for the function `_join`, with a nonce
in `x-quillo-call`, which is taken once: with the service's own key (`kid` its name)
when the app has a key for that service in `accept_calls`, else under
`QUILLO_SERVICE_SECRET`. The answer is 200 with `{ "service", "at" }`, the address the
copy is recorded at; otherwise 400 for a body without a port, an address that is not
known, or a `url` that does not lead to where the copy joined from, 401 for a bad or old
signature, a key of another service's, a join under the shared secret where the app
wants the service's key, or no nonce, 403 when the app takes no copies (started with `--no-join`,
or an `--allow-join` that does not name the service) or has neither `QUILLO_SERVICE_SECRET` nor keys in `accept_calls`, 404 when the app declares no such
service, 405 for a method other than POST, and 409 for a nonce sent before. Only copies call it, so a proxy in
front of the app need not pass it from outside.

## A chain of calls

Services may use one another: the app calls the shop, the shop calls billing, billing
posts to a ledger. Each link is an ordinary call, so the chain is written as three
functions calling each other:

```quillo
// shop.qwe
service billing = "billing.qwe" { timeout: "2s", retries: 1 }

route POST /buy { json: Order } {
    let inv = billing.invoice(Order(request.json))?
    { invoice: inv }
}

// billing.qwe
service ledger = "ledger.qwe"

server fn invoice(o: Order) -> map ! str {
    if trim(o.customer) == "" { return err("An invoice is for someone") }
    let inv = { id: len(invoices) + 1, customer: trim(o.customer), cents: o.cents, note: o.note }
    invoices = invoices + [inv]
    ledger.post(`inv-${inv.id}`, inv.cents)?        // the ledger's error is the invoice's
    inv
}

// ledger.qwe
let posted = {}

server fn post(ref: str, cents: int) -> int ! str {
    if cents <= 0 { return err("nothing to post") }
    if has(posted, ref) { return posted[ref] }      // a step done already is not done twice
    posted = set(posted, ref, cents)
    cents
}
```

What holds across the links:

- **Errors come back up.** `ledger.post` refusing `cents: 0` makes `billing.invoice`
  an error (`?` passes it on), which the shop's `?` turns into its answer: the caller
  three links up sees `nothing to post`. Replace it where it is yours to handle
  (`?? reply 400 ...`) and pass it on where it is not.
- **Each link commits by itself.** There is no transaction across services: billing's
  invoice is written whether or not the ledger takes the posting. So each step takes a
  reference the caller makes up (`inv-7`) and keeps what it did, as `post` does above:
  a request sent again goes on from where it stopped instead of posting twice.
- **The chain gives up together.** Each call carries the time its caller still waits
  (`x-quillo-deadline`), and the calls made below wait no longer than that, or their own
  `timeout` when shorter. When the shop's two seconds are up, billing's call to the
  ledger stops waiting too, and a call with no time left is not made. What a service is
  already doing runs to its end.
- **One id all the way down.** The request's `x-request-id` (or the one made for it)
  goes with every call. With `quillo run app.qwe --log json` the caller logs each call as a line of its
  own, beside the request's:

```json
{"time":"2026-10-29T03:30:43.727Z","service":"ledger","fn":"total","url":"http://127.0.0.1:56782","status":200,"ms":0.18,"tries":1,"rid":"req-44"}
{"time":"2026-10-29T03:30:43.727Z","method":"GET","path":"/","status":200,"ms":0.26,"ip":"127.0.0.1","rid":"req-44"}
```

- **A call runs once.** A retry sends the same call id, and a service remembers the ids
  it answered, so a link that was answered but not heard is not run again; the chain
  below it is not run again either. A service that dies in the middle of one is the
  exception: see [Calls that hold up](#calls-that-hold-up).
- **Only `server fn`s are reachable.** A service's routes, globals and plain functions
  are its own. What the next link needs, it gets as an argument.
- **In one process it is all function calls**, with nothing on the network, no
  timeouts and no retries; the same code, split, gets all of the above.

## Querying the API: `quillo docs` and `--describe`

A program's API is written from its source, as OpenAPI 3, so a gateway, a client
generator or a reviewer can read what a service takes without running it:

```sh
quillo docs app.qwe                 # to stdout
quillo run app.qwe --describe       # the same at GET /_quillo/describe
```

Each route is a path with its method; a query pattern becomes `parameters`, a `json:`
pattern a `requestBody` schema, and `x-quillo-at` says where in the source the route is:

```json
"/payments": {
  "post": {
    "requestBody": {
      "required": true,
      "content": { "application/json": { "schema": {
        "type": "object",
        "properties": { "to": { "type": "string" }, "amount": { "type": "string", "format": "decimal" } },
        "required": ["to", "amount"]
      } } }
    },
    "x-quillo-at": "app.qwe:6"
  }
}
```

A record type in a pattern or a parameter is named rather than expanded
(`"schema": { "x-quillo-type": "Order" }`). Each `server fn` is under
`x-quillo-server-fns` with its contract, its parameters and what it returns or fails
with:

```json
"invoice": {
  "contract": "invoice(o: Order) -> map ! str",
  "params": [{ "name": "o", "schema": { "x-quillo-type": "Order" }, "required": true }],
  "returns": { "type": "object" },
  "error": { "type": "string" }
}
```

Run on the app, the document holds the routes of the services served under their
prefixes (`/shop/buy`, `/billing`), and the `server fn`s of the app itself. A prefixed
service's `server fn`s are not in it: they are called by other services, signed, not
at `/billing/_quillo/rpc`. `quillo docs` and `--describe` give the same document. A
service is described by itself at `/billing/_quillo/describe`, or by running
`quillo docs` on its own file; a service declared without a path, only called, is
described that way only.

### What the document holds

- **Its title** is the program file's name (`main` for `main.qwe`), however the program
  was started. Under `--base /shop` it says where it is served:
  `"servers": [{ "url": "/shop" }]`.
- **Several routes on one method and path shape are one operation.** Their bodies are a
  `oneOf`, each literal field an `enum`; their query parameters are merged, required
  only where every route requires them; the path template is named as the first route
  names it (`/users/{id}`), its parameter a `oneOf` of the routes' types; and
  `x-quillo-at` is a list of the places in the source.
- **A rest parameter** (`/files/*path`) is `/files/{path}`, with
  `"x-quillo-rest": true` and the description "the rest of the path, slashes
  included". An OpenAPI template matches one segment, so a gateway that reads the
  document routes the rest itself.
- **Each operation has one response**, 200, with no content schema.
- **Under `--split`** the app's document still holds the prefixed services' routes,
  read from their source, and `/billing/_quillo/describe` answers the service's own
  document, its paths without the prefix.

```json
"/users/{id}": {
  "get": {
    "parameters": [{ "name": "id", "in": "path", "required": true,
                     "schema": { "oneOf": [{ "type": "integer" }, { "type": "string" }] } }],
    "responses": { "200": { "description": "the route's answer" } },
    "x-quillo-at": ["app.qwe:4", "app.qwe:8"]
  }
}
```

`/_quillo/describe`, when it is on, is public: it lists every route, those no page
links to as well, each `server fn`'s signature, the file and line of every route, and
the version of Quillo. Keep it private as `/_quillo/metrics` is, with a rule at the
proxy (see [Hosting](#health-logs-and-metrics)), or do not turn it on in
production.

The `contract` line is what a call carries in `x-quillo-contract`, and what a service
checks it against. A service deployed apart whose own differs refuses the call with
409 and says both sides:

```
service ledger: 409 `total` here is `total() -> int`; the caller's copy of this service says `total() -> str`
```

So a caller's `.qwe` copy of a service is its client stub: keep the `server fn`
signatures in it in step with the service, and `quillo check` plus the contract check
catch the rest. As the app loads, it reads only the copy's `server fn` headers, so a
mistake in one of its routes or bodies does not stop an app run with `quillo run app.qwe --no-check`.

The app may also go without the copy. With no file and an address (`--service-url` or
`QUILLO_SERVICE_<NAME>`), it asks the running service for its functions as it starts.
That is a call like any other, named `_contracts`, signed and tried at each address in
turn. The answer gives each function's header and contract, and only those this caller
may call. A service that cannot be reached then stops the app with `billing.qwe is not
here, and service billing did not give its functions`. `quillo check` warns instead:

```
main.qwe:1: warning: service billing: billing.qwe is not here, so its calls are not checked; it must run elsewhere (QUILLO_SERVICE_BILLING or --service-url)
```

```sh
# on 10.0.0.5, with billing.qwe
QUILLO_SERVICE_SECRET=... quillo run billing.qwe --port 8081 --as-service
# elsewhere, with main.qwe and no billing.qwe
QUILLO_SERVICE_SECRET=... quillo run main.qwe --service-url billing=http://10.0.0.5:8081
```

For a service written in another language, the copy is what `quillo check` reads, or
the server answers `_contracts` with `{"ok": [{"header": "server fn add(a: int, b: int)
-> int = null", "contract": "add(a: int, b: int) -> int"}]}`. Either way it must answer
`/_quillo/service/<fn>` as [The payload](#the-payload) describes, and `GET
/_quillo/ready`.

## Checked before it runs {#microservices-checked-before-it-runs}

`quillo check` checks each call against the service's `server fn`s: their names, the
number and types of their arguments, and that the result may be an error. A service
whose file is not here is warned of, and its calls are taken as written. A call needs
the `http` effect, so a function declared `with db` cannot quietly reach another
server. The browser cannot call a service: components call the program's own
`server fn`s, which may call services.

```quillo
fn checkout(name: str) with http = billing.invoice(name, 1200)
```

## Calls that hold up

When a service runs elsewhere, its calls are made to survive slow and failing servers.

### Several addresses

The same service can run on several servers, called in turn:

```sh
quillo run app.qwe --service-url billing=http://10.0.0.5:8081,http://10.0.0.6:8081
```

### Copies that join {#microservices-copies-that-join}

A copy of a service can join an app while it runs:

```sh
QUILLO_SERVICE_SECRET=... quillo run billing.qwe --as-service --port 8081 --join http://10.0.0.2:8080
```

It says it is there every 5 seconds, and that it is leaving when it stops; the app drops
a copy it has not heard from in 15 seconds. It is recorded at the address it connects
from, with its `--port`, called over `http://`; `--join-as
https://billing-3.internal:8443` gives the address the app calls it at instead, which
must lead to where it joined from.

The copy signs its joins with `QUILLO_SERVICE_KEY`, the service's own key, when it is
set, and with `QUILLO_SERVICE_SECRET` otherwise. An app whose `accept_calls` has a key
for `billing` takes copies of billing only when they are signed with that key, so a
service that holds the shared secret cannot join as another one and take its calls:

```quillo
// app.qwe
accept_calls { keys: { billing: env("BILLING_KEY") } }
service billing = "billing.qwe"
```

```sh
QUILLO_SERVICE_KEY=$BILLING_KEY quillo run billing.qwe --as-service --port 8081 \
    --join http://10.0.0.2:8080 --join-as https://billing-3.internal:8443
```

An app that never wants copies runs with `--no-join`; `--allow-join billing,risk` takes
copies of those services only.

- **When the service runs in the app's process**, the copies that joined take every
  call, in turn, and the app runs a call itself only when no copy answers it. A
  program in one process takes on copies as it gets busy, with no line of either
  changed.
- **When the app was given addresses** (`--service-url` or `QUILLO_SERVICE_<NAME>`),
  the given addresses and the joined copies are all taken in turn.

Each copy has a circuit of its own. With several app servers, a copy joins each.

Join the app's own address, not its public one: through a proxy the app sees the
proxy's address, unless it runs with `--trust-proxy` naming it (`quillo run app.qwe --trust-proxy
10.0.0.1`). A copy behind NAT is
recorded at an address the app cannot reach, so such a copy belongs behind a balancer
instead.

### Discovery by registration

Services are found by registering, in two ways: a copy joins the app (`--join`, above),
and any server joins limer's pool (`quillo run app.qwe --register URL`, with
`LIMER_ADMIN_SECRET`). A caller
does not look a service up by name, so a service the app does not run itself needs
`--service-url` or `QUILLO_SERVICE_<NAME>`. `QUILLO_SERVICE_SECRET` and the keys are
set on every machine by hand.

### Retries that run once

A call that got no connection, or a server answering 502, 503 or 504, is tried at the
next address, a little later each time. A call sent but not answered in time is tried
again at the same address, with the same call id. The service remembers the calls it
answered for eleven minutes (in its database too, `_quillo_calls`, when it has one, so
copies sharing it remember together) and gives the same answer again, so while the
service stays up the function runs once however often the call is sent. A call still
running there is waited for.

Across a crash a call runs at least once. A service stopped in the middle of a function
cannot say how far it got, so the call is refused as still running for eleven minutes,
as long as a signature lasts, and then runs again, though its first run may have
committed. A function that must not do its work twice keeps its own record: a reference
the caller makes up, or `idempotent`, which needs no header in a service: its key is the
call's own id, `request.call`, the same for each try, and it commits with the work.

### Failing servers left alone

An address that fails several calls in a row is left alone for a while (its circuit is
open), then tried with one call; the other addresses take its calls meanwhile. Each time
it fails again it is left alone twice as long, up to sixteen times the cooldown. When
every address is left alone, a call fails at once with an error value saying so, instead
of waiting on servers that are down.

### Options

Set where the service is declared, after the file:

```quillo
service billing = "billing.qwe" { timeout: "2s", retries: 1, failures: 5, cooldown: "5s" }
```

| Option | Default | What it sets |
|---|---|---|
| `timeout` | `"10s"` | How long one try may take |
| `retries` | `2` | How many times a call is tried again, 0 to 10 |
| `failures` | `5` | Failures in a row after which an address is left alone |
| `cooldown` | `"5s"` | How long it is left alone the first time |

Durations are `"500ms"`, `"2s"`, `"1m"` (`ms`, `s` and `m` only), or a number of
milliseconds. They apply when a service runs elsewhere; in one process a call is a
function's, which runs to its end.

## Following a request across services

Each request has an id: the `x-request-id` it came with (from a proxy or a load balancer),
or one made for it. Its answer carries it, the code sees it as `request.id`, and every
call to a service passes it on, so one request can be followed through every service it
reached.

With `--log json`, the request log shows the id as `"rid"`, and each call to a service is
a line of its own: the service, the function, the address, the status, how long it took,
the tries and the id.

## Calls made later

Some calls need not hold up the request: a receipt to send, a report to make.
`.later` keeps the call and makes it in the background:

```quillo
route POST /orders/:id(int)/paid {
    billing.later.invoice(`order ${id}`, 1200)    // the call's id, at once
    redirect(`/orders/${id}`)
}
```

The call is tried every second, then a little later each time (up to five minutes apart),
until it gets through; since the service remembers call ids, it runs once while it stays
up. `quillo check` checks it as it checks the call itself.

**It goes with the transaction.** A call is kept in the database the work is in, the
program's or the tenant's (the `_quillo_later` table), so inside `db.transaction` it
commits or rolls back with the work, on every database. It outlives a restart, and servers
sharing the database make each call once. Without a database, it goes in a
`quillo-system.db` file next to the program.

**What finishes it.** A value, or an error value the function gave. Any other answer does
not: a 401, 404 or 409 from a deploy gone wrong, or a 500, is tried again for about a day,
and a service that does not answer at all is waited for however long it takes. A 400,
arguments the service refuses, cannot work and is given up on at once. A call given up on
stays in `_quillo_later` with `tries` -1 and why in `error`, is counted in
`quillo_later_total` with `quillo run app.qwe --metrics`, and goes again with `update _quillo_later set at =
0, tries = 0 where tries < 0`.

**Asking after one.** [`later.status(id)`](https://quillolang.org/docs/reference/later.status/) says whether the
call is waiting, done, with what the function gave, or failed.

**Who delivers them.** The same thread for each program in the process, the app and the
services it runs, however the program runs. It takes 50 calls a go from each database,
each second, and the next go at once after a full one. A call that has to wait does not
hold up the next, so order is not kept.

## Sagas {#microservices-sagas}

A saga is an operation spread over services that share no transaction. Each service commits
its own part, and if a later part cannot happen, the earlier parts are undone by
compensating steps you write. Quillo has no `saga` word: one is a table, a job that moves
each row a step at a time, and ordinary calls. [Routes](#sagas) has
a working one. Five rules carry it:

1. **The reference is the key.** The caller makes it up, and a step keyed on it changes
   nothing the second time.
2. **A "no" is an answer, not an error.** A step gives `{ ok: false, why }`. An error value
   means no answer came and the step may have run.
3. **An undo works on what never happened, and leaves a mark.** A refund with no charge
   writes `refunded`, so a charge that arrives late is refused.
4. **A step with no answer is undone too.** It may have run, and rule 3 makes that safe.
5. **An undo never gives up.** It waits for the service, so a saga long in `undoing` is what
   to alert on.

## What it does not do {#microservices-what-it-does-not-do}

- **No transaction across services.** Each service's `db.transaction` is its own, so a
  chain of calls commits step by step. Give each step a reference the caller makes up,
  and keep what was done, so a request sent again goes on from where it stopped instead
  of doing a step twice. To undo the steps before one that fails, see [Sagas](#microservices-sagas).
- **Variables are each copy's.** Copies of a service (`quillo run app.qwe --split --scale
  billing=3`, `--join`, several addresses) share its database, not its global variables.
- **A service's own publishing.** A topic a service publishes while answering a call
  reaches the caller's pages; one it publishes on its own (a job, a call made later
  being delivered) reaches only the service's pages when it runs apart.

[Services](#services) in the reference has the rest: `--scale`, `--join`,
`/_quillo/ready`, live data across services, and copies behind a balancer.

## Authentication between services {#microservices-authentication-between-services}

A call signed with `QUILLO_SERVICE_SECRET` proves that the caller is one of the system's
services, and nothing more: every service that holds the secret can sign as any other.
To say which service calls, and what it may call, each side says how.

The caller says how it proves who it is, where it declares the service:

```quillo
service billing = "billing.qwe"                                                  // auth: "quillo", the shared secret
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY"), as: "shop" } }   // a key of its own, known as shop
service billing = "billing.qwe" { auth: { bearer: () => env("BILLING_TOKEN") } } // any token the program makes
service billing = "billing.qwe" { auth: { oauth: {                               // OAuth 2.0 client credentials
    token_url: env("IDP_TOKEN_URL"), client_id: "shop", client_secret: env("SHOP_CLIENT_SECRET"),
    scope: "billing", audience: "billing",
} } }
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY"), as: "shop", mtls: { cert: "shop.crt", key: "shop.key", ca: "ca.pem" } } }
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY"), as: "shop" }, forward: ["authorization"] }
```

- `{ key, as }` signs each call with the caller's own key, under the name `as` (the
  callee's `keys` names it).
- `{ bearer: f }` calls `f` for each call and sends what it gives as `Authorization:
  Bearer ...`.
- `{ oauth: { token_url, client_id, client_secret, scope, audience, method } }` fetches
  a token from the identity provider's token endpoint (RFC 6749's client credentials),
  keeps it until 30 seconds before it expires, and fetches it again once when the
  service answers 401. `method` is `"post"` (the client's id and secret in the form, the
  default) or `"basic"` (in an `Authorization: Basic` header).
- `{ mtls: { cert, key, ca } }` presents a client certificate on the connection, beside
  one of the others; `ca` is the certificate authorities to trust for the service's own.
- `forward: ["authorization"]` passes those headers of the visitor's request along, so
  the service can see who the visitor is.

`quillo check` warns of a `key` or `client_secret` written in the source, where every
copy of the program has it: read it with `env(...)`.

The service says what it takes, at its top level, and who may call each function:

```quillo
// billing.qwe
accept_calls {
    keys: { shop: [env("SHOP_KEY"), env("SHOP_KEY_OLD")], notify: env("NOTIFY_KEY") },
    shared: false,                                                    // no calls under the shared secret
    jwt: { issuer: env("IDP"), audience: "billing", jwks: env("IDP") + "/.well-known/jwks.json" },
    client_cert: { header: "x-forwarded-client-cert" },              // from a proxy --trust-proxy names
    check: (caller) => caller.name != "mallory",                     // anything else, in Quillo
}

server fn refund(account: int, cents: int) for ["shop"] {
    { refunded: cents, by: request.caller.name }
}

server fn whoami() = { caller: request.caller, user: request.user, forwarded: request.forwarded }
```

- `keys` gives each caller's key by its name; a list takes any of them while one is
  rotated.
- `jwt` takes bearer tokens: their signature against the provider's keys (`jwks`, a URL;
  RS256, ES256 and EdDSA) or a `secret` (HS256), with `exp` required and `iss` and `aud`
  checked. `audience` is required.
- `client_cert` takes the identity of a certificate a proxy checked, as Envoy and
  Istio pass it on: Quillo does not end TLS itself. The header is believed only on a
  request from an address `--trust-proxy` names.
- `check` runs before the arguments are read. `false` refuses the call with 403, a map
  becomes the caller, and `reply` answers the call.
- `for ["shop"]` takes calls from those callers only; others get 403, before their
  arguments are read.

`request.caller` is who called: `{ name, by }`, with `by` one of `"quillo"`, `"key"`,
`"jwt"` and `"mtls"`; a token adds its `scopes` and `claims`, a certificate its
`subject` and `uri`. `request.forwarded` holds the visitor's headers the caller passed
on, and `request.user` the claims of the visitor's token there, when `jwt`'s rules take
it. `quillo docs` writes each function's callers as `x-quillo-callers`, for a gateway.

**Rotating a secret.** `QUILLO_SERVICE_SECRET=new,old` (or `QUILLO_SERVICE_SECRET` and
`QUILLO_SERVICE_SECRET_PREVIOUS`) signs with the first and takes any. Set both on every
machine, one at a time, then drop the old one. A caller's key is rotated the same way,
with a list in the service's `keys`.

**Signed answers.** A service signs its answer with the key the call was signed with. A
caller that signs with a key of its own refuses an answer without that signature, so a
server that only says it is billing cannot answer for it.

**Callers that are not Quillo services.** A gateway, or a service in another language,
can sign Quillo's calls (the wire is above), send a token `jwt` takes, or call routes.
For routes, check the token in a route's middleware with the catalog's `jwt.qwer`
(2.0.0: `jwt_check` with `audience`, `issuer` and `require`, `exp` required, and
`jwt_verify_jwks` for an identity provider's keys); `oidc.qwer` 1.1.0 adds
`oidc_client_credentials`, to fetch a token for calling out, and `oidc_verify_access`.
Routes give up what `service` brings (checked calls, contracts, retries run once,
deadlines, `.later`), so prefer `accept_calls` where the caller can send a token.

## Security

- A call is signed with HMAC-SHA256 under `QUILLO_SERVICE_SECRET` or the caller's own
  key, together with the time, the service it is for, the function, the tenant, the
  call's id, the caller, the deadline, the contract and its arguments; a call more than
  five minutes old, or signed for another service, is refused. The tenant goes with it.
  The answer is signed back.
- The shared secret is all or nothing: every service that holds it can call any
  function of any other, as any caller, in any tenant. Give each caller a key of its own
  and name who may call what with `for [...]`; see [Authentication between
  services](#microservices-authentication-between-services).
- Only `server fn`s are offered at `/_quillo/service/<name>`: a service's other functions
  and its globals cannot be reached from outside.
- A service's routes, under its prefix, are passed on to it, and are public as any route
  is: guard them as you would in one program.
- `/_quillo/describe`, when on, is public and lists every route, each `server fn`'s
  signature, the file and line of every route, and the version: keep it private at the
  proxy, as `/_quillo/metrics` is, or leave `--describe` off in production.
- `/_quillo/join` takes only signed joins, each nonce once: with the service's own key
  where the app has one for it, else under `QUILLO_SERVICE_SECRET`. `--no-join` turns it
  off.

## The example

[examples/services](https://github.com/quillolang/examples/tree/main/services) is a shop (`shop.qwe`) that bills through
a billing service (`billing.qwe`), put together by `app.qwe`. The shop declares billing
with a two-second timeout and one retry. Run it in one process with
`quillo run services/app.qwe` (in a clone of [quillolang/examples](https://github.com/quillolang/examples)), or split with `--split`.
