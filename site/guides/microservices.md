# Microservices

In Quillo a microservice is a `service`: a program whose `server fn`s other programs call
as if they were their own functions. The same code runs in one process or split across
servers, so an app can start as one program and be split when there is a reason to,
without its code changing. This page puts together what [Routes](/docs/routes/#services)
says about services; [examples/services](/docs/examples/services/) is a shop that bills
through a billing service.

### Declaring a service

`service` names a program and gives it a name to call it by. Its `server fn`s are then
functions of that name:

```quillo
// shop.qwe
service billing = "billing.qwe"

route POST /buy {
    let inv = billing.invoice(request.form.name, 1200)?    // billing's `server fn invoice`
    <p>Invoice #{inv.id}</p>
}
```

The app is made of its services. With a path, a service's routes are served under it too,
and it is named after its file:

```quillo
// app.qwe
service /shop = "shop.qwe"
service /billing = "billing.qwe"
```

A program with `server fn`s and no routes is served as well: a service may be only that.
Services may use one another.

### One process or many

- **In one process**, the default, a service is loaded once and shared by every program
  in the process that names it: one copy of its variables and of its database. A call
  goes straight to it, with nothing on the network.
- **Split**, a service runs on a server of its own, and its calls go there as signed
  HTTP requests. Run alone, a service is an ordinary program; `--base` is the path it is
  served under, which its links and redirects carry.
- **`--split`** runs each service the app declares (and those they declare) as a
  process of its own, each on its own port, its log lines marked with its name, all
  signed with one secret. They stop with the app. It is the quickest way to see an app
  as it would run split, on one machine.

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
and every caller needs the same `QUILLO_SERVICE_SECRET`; without it, a call to a service
that runs elsewhere is an error value saying so, not an unsigned request.

### What crosses the wire

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

### Checked before it runs

`quillo check` checks each call against the service's `server fn`s: their names, the
number and types of their arguments, and that the result may be an error. A call needs
the `http` effect, so a function declared `with db` cannot quietly reach another
server. The browser cannot call a service: components call the program's own
`server fn`s, which may call services.

```quillo
fn checkout(name: str) with http = billing.invoice(name, 1200)
```

### Calls that hold up

When a service runs elsewhere, its calls are made to survive slow and failing servers.

#### Several addresses

The same service can run on several servers, called in turn:

```sh
quillo run app.qwe --service-url billing=http://10.0.0.5:8081,http://10.0.0.6:8081
```

#### Retries that run once

A call that got no connection, or a server answering 502, 503 or 504, is tried at the
next address, a little later each time. A call sent but not answered in time is tried
again at the same address, with the same call id. The service remembers the calls it
answered for ten minutes and gives the same answer again, so the function runs once
however often the call is sent. A call still running there is waited for.

#### Failing servers left alone

An address that fails several calls in a row is left alone for a while (its circuit is
open), then tried with one call; the other addresses take its calls meanwhile. Each time
it fails again it is left alone twice as long, up to sixteen times the cooldown. When
every address is left alone, a call fails at once with an error value saying so, instead
of waiting on servers that are down.

#### Options

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

Durations are `"500ms"`, `"2s"`, `"1m"`, or a number of milliseconds.

### Following a request across services

Each request has an id: the `x-request-id` it came with (from a proxy or a load balancer),
or one made for it. Its answer carries it, the code sees it as `request.id`, and every
call to a service passes it on, so one request can be followed through every service it
reached.

With `--log json`, the request log shows the id as `"rid"`, and each call to a service is
a line of its own: the service, the function, the address, the status, how long it took,
the tries and the id.

### Calls made later

Some calls need not hold up the request: a receipt to send, a report to make.
`.later` keeps the call and makes it in the background:

```quillo
route POST /orders/:id(int)/paid {
    billing.later.invoice(`order ${id}`, 1200)    // the call's id, at once
    redirect(`/orders/${id}`)
}
```

The call is tried every second, then a little later each time (up to five minutes apart),
until it gets through; since the service remembers call ids, it runs once. Calls kept for
later are stored in the program's database when it has one (the `_quillo_later` table),
so they outlive a restart, and servers sharing the database make each call once; without
a database, in memory. `quillo check` checks them as it checks the call itself.

### Security

- A call is signed with HMAC-SHA256 under `QUILLO_SERVICE_SECRET` and the time; a call
  more than five minutes old is refused. The tenant goes with it.
- Only `server fn`s are offered at `/_quillo/service/<name>`: a service's other functions
  and its globals cannot be reached from outside.
- A service's routes, under its prefix, are passed on to it, and are public as any route
  is: guard them as you would in one program.

### The example

[examples/services](/docs/examples/services/) is a shop (`shop.qwe`) that bills through
a billing service (`billing.qwe`), put together by `app.qwe`. The shop declares billing
with a two-second timeout and one retry. Run it in one process with
`quillo run examples/services/app.qwe`, or split with `--split`.
