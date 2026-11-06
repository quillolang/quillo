# Money and payments {#money}

An app that moves money must get every cent right and do each payment once, however
often it is sent. Quillo has the pieces for that: **decimals** and **`money`** for
amounts, **`idempotent`** for requests that are sent again, and **transactions** that
are tried again when they collide. The rest (a record of who did what, a payment
provider's webhooks) is the program's own code, and short.

This chapter builds a small ledger with them: accounts, transfers between them, deposits
from Stripe, and a statement. The details of each piece are in the reference, linked
from each part.

## Amounts: decimals

A float is a binary fraction, so `0.1 + 0.2` is `0.30000000000000004`. A `dec` is an
exact decimal, written with an `m` after it (as in C#):

```quillo
0.1 + 0.2                                // 0.30000000000000004
0.1m + 0.2m                              // 0.3
let price = 19.99m
let total = price * 3 + 4.50m            // 64.47
let vat = round(total * 0.21m, 2)        // 13.54, half to even
dec("12.50")                             // from a form or JSON: 12.50
```

A decimal keeps its digits (`12.50m` shows as "12.50"), `+`, `-` and `*` are exact,
and `round` takes the modes banks use: `half_even` (the default), `half_up`, `down`,
`floor` and the rest. Mixing a decimal with a float is an error, never a silent float:
`1.5m + 0.5` says to convert one with `dec(x)` or `float(x)`, and `quillo check` finds it
before the program runs. The browser gives the same answers: a whole float is a float
there too, so `1.5m + 2.0` is the same error in a component, and `money.of(2.0, "USD")`
is refused on both sides, as an amount never goes through a float. A route's pattern takes `amount: dec` from a query, a form or
JSON (`"12.50"` or `12.5`), and JSON answers write decimals as strings, so nothing that
reads them rounds them. See [Decimals](#decimals).

## Amounts of a currency: money

`money` keeps an amount with its currency, as a plain map, `{ amount: 19.99m, currency:
"USD" }`, always with the currency's minor units (JPY none, USD two, BHD three):

```quillo
money.of(19.99m, "USD")                          // { amount: 19.99m, currency: "USD" }
money.of("12.345", "USD")                        // an error value: USD has two decimals
money.of("12.345", "USD", "half_up")             // 12.35
money.mul(money.of(19.99m, "USD"), 0.075m, "half_up")   // 1.50, rounded to cents
money.of(10, "USD") + money.of(2.50m, "USD")     // 12.50: two amounts of one currency
money.allocate(money.of(100, "USD"), [1, 1, 1])  // 33.34, 33.33 and 33.33
money.parse("1.234,50 €", "EUR", "de-DE")        // 1234.50, as a German visitor writes it
money.format(money.of("1234.5", "EUR"), "de-DE") // "1.234,50 €"
money.to_minor(money.of(19.99m, "USD"))          // 1999
money.from_minor(1250, "USD")                    // 12.50
```

`+` and `-` add and subtract two amounts of one currency, as `money.add` and `money.sub`
do; two currencies are an error value, never a sum. Every `money` function holds an
amount to the currency's decimals: `money.round(m, mode)` makes money of one with more.
`allocate` splits an amount without losing or making a cent: the shares always add up to
it, and the cents left go to the shares closest to their next one. `format` writes an
amount as CLDR has it for 122 locales, as `Intl.NumberFormat` writes it in a browser, and
`parse` reads one back as the locale writes it: `money.of("1.000", "EUR")` is one euro,
while a German visitor meant a thousand. All of it works in components too, with the same
answers. See [Money](#standard-library-money).

Money a client sends is checked by its type: `{ json: { price: money } }` in a route's
pattern, or `price: money` in a record, takes `{ "amount": "19.99", "currency": "USD" }`
held to `money.of`'s rules, and answers 400 to `"19.999"`, to `"1e3"` and to a currency
that is not one.

## Keeping it in the database

SQLite has no decimal type, so the ledger keeps whole cents in integers, which SQL can
add up exactly. (A `money` or `decimal(12, 2)` column there keeps 15 digits, and the log
says so when the table is made.) PostgreSQL's `numeric` and MySQL's `decimal` come back as decimals
instead, with their scale.

```sql
-- migrations/001_ledger.sql
create table accounts (
    id integer primary key,
    owner text not null,
    currency text not null,
    balance_minor integer not null default 0    -- cents, for USD
);
create table transfers (
    id integer primary key,
    from_id integer, to_id integer not null,
    amount_minor integer not null,
    currency text not null,
    at text not null default (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    stripe_event text unique                    -- a webhook's event, once
);
create table history (
    id integer primary key,
    at text not null default (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    actor text, action text not null, details text not null
);
```

```quillo
db.open(env("DATABASE_URL") ?? "ledger.db") | unwrap
db.migrate()

fn me() = session().user ?? reply 401 "Sign in first"
fn balance(a) -> map = money.from_minor(a.balance_minor, a.currency)

// Who did what: a row of `history`, in the transaction of the change it records.
fn record(action: str, details) -> map with db =
    db.exec("insert into history (actor, action, details) values (?, ?, ?)",
        session().user, action, json.stringify(details))
```

## A transfer, once

A client that sent a transfer and got no answer (a timeout, a dropped connection)
cannot tell whether it was made. With `idempotent`, it sends the same request again with
the same `Idempotency-Key` header, and gets the first answer, without the transfer
running again, as with Stripe's API:

```quillo
route POST /transfers { json: { from: int, to: int, amount: dec } } with db {
    let user = me()
    idempotent({ isolation: "serializable", retries: 5 }, () => {
        let src = db.one("select * from accounts where id = ? and owner = ?", from, user)
            ?? reply 404 "No such account"
        let dst = db.one("select * from accounts where id = ?", to) ?? reply 404 "No such account"
        if dst.currency != src.currency { reply 422 "The accounts hold different currencies" }
        let amt = money.of(amount, src.currency) ?? reply 422 `Not an amount of ${src.currency}`
        let cents = money.to_minor(amt)
        if cents <= 0 { reply 422 "The amount must be more than zero" }
        if cents > src.balance_minor {
            return respond(422, { error: "Not enough money", balance: balance(src) })
        }
        db.exec("update accounts set balance_minor = balance_minor - ? where id = ?", cents, from)?
        db.exec("update accounts set balance_minor = balance_minor + ? where id = ?", cents, to)?
        db.exec("insert into transfers (from_id, to_id, amount_minor, currency) values (?, ?, ?, ?)",
            from, to, cents, src.currency)?
        let id = db.one("select last_insert_rowid() as id").id
        record("transfer", { id, from, to, amount: amt })?
        respond(201, { id, balance: money.sub(balance(src), amt) })
    })
}
```

```sh
curl localhost:8080/transfers -b cookies.txt -H "Idempotency-Key: $(uuidgen)" \
    --json '{"from": 1, "to": 2, "amount": "12.50"}'
# 201 {"id":1,"balance":{"amount":"87.50","currency":"USD"}}
# the same again: 201, the same body, and Idempotent-Replayed: true
```

`cookies.txt` holds the session of a signed-in user, from a sign-in like the one in
[Headers, cookies and sessions](#headers-cookies-sessions) (`curl -c cookies.txt`
on its `POST /login`); without it the answer is `401 Sign in first`. Asking for more
than the balance gives `422 {"error":"Not enough money","balance":{"amount":"87.50","currency":"USD"}}`.

- `idempotent` runs the function in a transaction and writes the answer in it, so the
  transfer, its `history` row and its answer are kept together, or none of them is. If
  the server stops part way, the database rolls the transaction back, and a minute later
  the next request with the key runs it.
- The same key with another body is refused with 422; one sent while the first is still
  running gets 409. Keys belong to the signed-in user: another user's key is another
  request.
- A `reply` rolls back and keeps nothing, so a retry runs again: right for a request
  that was wrong. `respond(422, ...)` is an answer like any other: "Not enough money" is
  kept, and the same request again gets it, even if money has come in since.
- Each `?` turns a failed statement into an error value, which rolls everything back.

A `server fn` can use `idempotent` too, and the page then sends a key with each call by
itself, and sends the call again when the connection fails. See
[Idempotent requests](#idempotent-requests).

## Transactions that collide

Two transfers out of one account at once must not both spend the same balance.
`{ isolation: "serializable" }` has the database find such conflicts: one of the two
fails, and `retries: 5` runs it again, from the start, on the balance the other left.
`db.transaction` takes the same options when there is no request to keep:

```quillo
fn withdraw(account: int, cents: int) -> map with db =
    db.transaction({ isolation: "serializable", retries: 5 }, () => {
        let balance = db.one("select balance_minor from accounts where id = ?", account).balance_minor
        if balance < cents { return err("not enough money") }
        db.exec("update accounts set balance_minor = balance_minor - ? where id = ?", cents, account)
    })
```

A function that may run more than once should do nothing that cannot be taken back:
send emails and call other servers after the transaction (`quillo check` warns about
`http.*` inside one). SQLite lets one transaction write at a time, so there every
transaction is serializable already. See [Database](#standard-library-database).

## Deposits: a signed webhook

Stripe says that a card payment went through by posting an event, signed with a secret
the two share: the `Stripe-Signature` header is `t=<time>,v1=<hex>`, an HMAC-SHA256 of
the time and the body. `crypto.hmac` makes it again, and `crypto.equal` compares it in
constant time:

```quillo
fn stripe_signed(body: str, header: str?, secret: str) -> bool {
    let fields = split(header ?? "", ",") | map((f) => split(f, "="))
    let t = int(find(fields, (f) => f[0] == "t")?[1] ?? "")
    if is_err(t) || abs(now() / 1000 - t) > 300 { return false }   // five minutes
    let want = crypto.hmac(secret, `${t}.${body}`)
    any(fields, (f) => f[0] == "v1" && crypto.equal(f[1] ?? "", want))
}

route POST /hooks/stripe with db, env, time {
    if !stripe_signed(request.body, request.headers["stripe-signature"], env("STRIPE_WEBHOOK_SECRET")) {
        reply 400 "Bad signature"
    }
    let event = request.json
    if event.type == "payment_intent.succeeded" {
        let pi = event.data.object
        let to = int(pi.metadata.account)?
        let amt = money.from_minor(pi.amount, upper(pi.currency))
        db.transaction(() => {
            // Stripe may send an event more than once: keep it once.
            if db.one("select id from transfers where stripe_event = ?", event.id) == null {
                db.exec("insert into transfers (to_id, amount_minor, currency, stripe_event) values (?, ?, ?, ?)",
                    to, money.to_minor(amt), amt.currency, event.id)?
                db.exec("update accounts set balance_minor = balance_minor + ? where id = ? and currency = ?",
                    money.to_minor(amt), to, amt.currency)?
                record("deposit", { to, amount: amt, stripe_event: event.id })?
            }
        })
    }
    { received: true }
}
```

`request.body` is the body as it was sent, which the signature is of. A signature older
than five minutes is refused, so an event recorded and sent again later does nothing,
and any of the `v1` signatures may match (Stripe sends two while a secret is changed).

To try it without Stripe, sign an event as Stripe would, with the same secret in
`STRIPE_WEBHOOK_SECRET`:

```sh
body='{"id":"evt_1","type":"payment_intent.succeeded","data":{"object":{"amount":2500,"currency":"usd","metadata":{"account":"2"}}}}'
t=$(date +%s)
sig=$(printf '%s.%s' "$t" "$body" | openssl dgst -sha256 -hmac "$STRIPE_WEBHOOK_SECRET" -hex | awk '{print $NF}')
curl localhost:8080/hooks/stripe -H "Stripe-Signature: t=$t,v1=$sig" --json "$body"
# {"received":true}, and account 2 has 25.00 more; sent again, the same answer and nothing added
curl localhost:8080/hooks/stripe -H "Stripe-Signature: t=$t,v1=00" --json "$body"
# 400 Bad signature
```

## The statement

`money.format` writes amounts in the reader's language, on the server and in
components alike:

```quillo
view Statement(a, rows: list, locale: str) =
    <section>
        <h1>{money.format(balance(a), locale)}</h1>
        <table>{for t in rows =>
            <tr>
                <td>{slice(t.at, 0, 10)}</td>
                <td>{t.to_id == a.id ? "In" : "Out"}</td>
                <td>{money.format(money.from_minor(t.amount_minor, t.currency), locale)}</td>
            </tr>}</table>
    </section>

route GET /accounts/:id(int) { query: { locale: str = "en-US" } } with db {
    let a = db.one("select * from accounts where id = ? and owner = ?", id, me()) ?? reply 404 "No such account"
    let rows = db.query("select * from transfers where from_id = ? or to_id = ? order by id desc limit 50", id, id)
    <html><body><Statement a={a} rows={rows} locale={locale}/></body></html>
}
```

With `?locale=de-DE`, a balance shows as "112,50 $". `format_number(1234567.5, "en-IN",
2)` writes any number the same way: "12,34,567.50". For interest, a decimal rate keeps
it exact: `money.mul(money.of(10000, "USD"), 0.045m * dec(181) / 360)` is 226.25. A loan's
monthly payment needs a power, which a decimal takes too:

```quillo
fn payment(principal: dec, yearly: dec, months: int) -> dec {
    let r = yearly / 12
    let f = pow(1m + r, months)
    principal * r * f / (f - 1)
}
money.of(payment(200000m, 0.05m, 360), "USD", "half_up")   // 1073.64 a month
```
