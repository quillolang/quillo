# Feedback from probing money and the other finance types: `money`, `dec`, rounding, formatting, and amounts in SQLite and PostgreSQL

| | |
|---|---|
| App | None: about 70 probe programs of 1 to 25 lines, one transfer route driven by `curl`, one page run in headless Chrome (25 money calls worked out by the server and by the browser, then compared), a grid of 18,144 `money.format` calls compared with Node's `Intl.NumberFormat` (49 locales and 3 locales outside the table, 36 currencies, 9 amounts), all 165 currencies in 6 locales against `Intl`, and SQLite and a local PostgreSQL 17 (a scratch database, dropped afterwards). Covered: `money.*`, `dec` and its arithmetic, `round`, `format_number`, `type Money = {...}` records, JSON in and out, `db.exec`/`db.query` with `decimal`, `decimal_text`, `money` and `numeric` columns |
| Quillo | `quillo 0.7.0-dev (a0a0ac0, 2026-11-03)`, a release build of main. While this report was written, another session had uncommitted edits in the same checkout (`USERGUIDE.md`, `src/desktop.rs`, `tests/cli_app.rs`, and a new FEEDBACK49.md); none touch money, decimals, the databases or `runtime.js` |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` and `quillo run` on each probe; `quillo run --port` with `curl` for the route; [feedback/tools/drive.mjs](../tools/drive.mjs) for the page; Node 24.11 (ICU 77.1) for `Intl`; `psql` for PostgreSQL. Read: [src/money.rs](../../src/money.rs), [src/decimal.rs](../../src/decimal.rs), the money part of [src/client/runtime.js](../../src/client/runtime.js) (around line 2148), `col_kind`/`from_sql` in [src/db.rs](../../src/db.rs), `other_type` in [src/pg.rs](../../src/pg.rs), `round` in [src/builtins.rs](../../src/builtins.rs), and USERGUIDE.md's Decimals and Money sections |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section. FEEDBACK49.md (the test suite's failure side) was being written at the same time and had reached Q-609: numbers here start at Q-620, to leave it room.

`money.of` is careful: it refuses floats, exponents, bad text, unknown codes and too many decimals, and gives error values where the input comes from a visitor. The rough edges are around it. A money value is a plain map, and every way of getting one other than `money.of` skips those checks. That includes the `type Money` record the guide recommends, JSON, a hand-made map and the database. Then some functions round the amount and others do not. The worst cases: `+` on two money values merges the maps and gives the right one (Q-620), and the guide's way of checking client money lets through what later becomes a 500 or a silently changed amount (Q-621). A PostgreSQL `money` column can also read back 100 times smaller (Q-625). Nine of the fourteen issues are silent.

## Summary

| ID | Problem | Kind |
|---|---|---|
| Q-620 | `a + b` on two money values merges the maps: a cart's total is its last line, and `usd + eur` is the euros. `check` says ok | silent |
| Q-621 | `type Money = { amount: dec, currency: str }`, the guide's way to check what a client sends, checks the shape only: `10.004` and `"1e3"` go through and get changed, and `BTC`, `"usd "` or an amount too large become a 500 at the first `money.*` call | silent / loud |
| Q-622 | Given a map with more decimals than its currency has, `add`, `sub`, `mul`, `to_minor`, `format` and `allocate` round it, but `compare`, `is_zero`, `neg` and `==` do not | silent |
| Q-623 | Q-096's checks are `money.of`'s only: an amount `"1e3"` in a map is 1,000, a code with a space is refused only from a map, and `add`/`mul` make amounts that `to_minor` fails on | silent / loud |
| Q-624 | `money.of` reads amounts only as en-US writes them: a German visitor's "1.000" (one thousand) is €1.00, and `money.format`'s output cannot be read back | silent |
| Q-625 | A PostgreSQL `money` column is always read with 2 decimals: under a yen `lc_monetary`, ¥1,234 reads as `12.34m`, and `500m` written reads back as `5.00m` | silent, data |
| Q-626 | One amount, four roundings: `money.*` half to even, `format_number` half away from the shortest decimal, `round(float, n)` half away from the binary product, PostgreSQL half away. An invoice line shows 0.13 and adds up as 0.12 | silent |
| Q-627 | `money.format` is not "the same as `Intl.NumberFormat`": 13 currencies get decimals `Intl` does not show, XOF/XAF/XPF/XCD/XCG have no symbol, and a locale outside the 48 is silently another country's (es-CO as es-ES, zh-Hant-TW as zh-CN, ar-EG as en-US) | silent / docs |
| Q-628 | In SQLite every column type for money has a trap that only the guide mentions: `money` and `decimal(12,2)` lose cents past 15 digits, and `decimal_text` sorts, `max`es and `sum`s as text or floats | silent |
| Q-629 | `db.exec(sql, [x])`, Python's way of passing parameters, stores the list as JSON text (`["9.99"]`) when the statement has one `?`. `check` says ok | silent |
| Q-630 | Money from data with a withdrawn or unlisted code (BGN since 2026-01-01, HRK, BTC, USDT) is a runtime error in every `money.*` function, not an error value | loud |
| Q-631 | `money.allocate` gives the leftover units to the first shares, not the largest remainders: a 1:99 split of $0.10 gives the 1% share 10% | silent, small |
| Q-632 | There is no exact power of a decimal: compound interest and loan payments go through floats (`pow` and `math.pow` refuse a `dec`) | limitation |
| Q-633 | The guide says `money.mul` gives an error value for a bad argument; it is a runtime error. `check` lets `"half-up"` and `"XYZ"` literals through | docs / loud |

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-096 | Its fix holds at `money.of` only: see Q-623 | 1 |
| Q-369 | Its fix made `format_number` round ties away from zero "as `Intl` and `round` do" (money.rs:242), but `round(1.005, 2)` is `1.0` and `format_number(1.005, "en-US", 2)` is `"1.01"`: see Q-626 | 1 |

## New issues

### Q-620. `a + b` on two money values merges the maps: a cart's total is its last line

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time output | 1 |

`+` on two maps merges them, the right one's values winning (the guide's operator table). A money value is a map, so `total + price` is `price`:

```quillo
fn checkout(cart: list) = {
  let total = money.zero("USD")
  for item in cart {
    total = total + money.mul(item.price, item.qty)
  }
  total
}
print(checkout([{ price: money.of(10, "USD"), qty: 2 }, { price: money.of(5, "USD"), qty: 1 }]))
```

`check`: ok. Prints `{ amount: 5.00m, currency: "USD" }`, not 25.00. `money.of(5, "USD") + money.of(2, "EUR")` is `{ amount: 2.00m, currency: "EUR" }`: the same `+` that `money.add` refuses across currencies turns dollars into euros. The browser does the same (runtime.js:691).

Every other operator is refused by `check`, so `+` is the only one that seems to work: `a - b` ("cannot subtract map | error and map | error"), `a * 3`, `a < b` and `a + 1m` are all errors. `+` is the first thing a model writes for a sum. Only the docs' list of `money.add`, `money.sub`, ... says otherwise.

**Suggestion:** `check` already types `money.*` results as money (it says "this map has no field `error` (it has amount, currency)"). Make `+` on two values it knows are money an error: "money + money merges the maps: use money.add(a, b)". At run time, `+` of two maps that both have exactly `amount` and `currency` could fail the same way, which costs one key check per map merge.

### Q-621. `type Money = { amount: dec, currency: str }` checks the shape only, and what it lets through becomes a 500 or a changed amount

| Kind | Detected by | Attempts |
|---|---|---|
| silent / loud | `curl` | 1 |

The guide ends its Money section with: "To hold the program to the shape, name it: `type Money = { amount: dec, currency: str }` makes `Money(request.json)` check what a client sends". A transfer route written that way:

```quillo
type Money = { amount: dec, currency: str }
type Transfer = { to: str, amount: Money }
route POST /transfer { json: Transfer } = {
  let fee = money.of(0.25m, "USD")
  let total = money.add(json.amount, fee)
  { total, cents: money.to_minor(total) }
}
```

`check`: ok. What the route answers:

| `amount` sent | Answer |
|---|---|
| `{"amount": "10.00", "currency": "USD"}` | 200, total 10.25 |
| `{"amount": "10.004", "currency": "USD"}` | 200, total 10.25: the 0.004 was taken and dropped |
| `{"amount": "1e3", "currency": "USD"}` | 200, total 1000.25: `dec` reads exponents, `money.of` does not |
| `{"amount": "10", "currency": "BTC"}` | 500, "internal error"; the log: "`money.add` expects money ... unknown currency" |
| `{"amount": "10", "currency": "usd "}` | 500, the same (`money.of(1, " usd ")` takes it, see Q-623) |
| `{"amount": "92233720368547758.07", "currency": "USD"}` | 500, "integer overflow" at `money.to_minor` |
| `{"amount": "-10", "currency": "USD"}` | 200, total -9.75: nothing says a transfer is positive, which is the program's job, but the guide's example suggests the type is the check |

The record checks that `amount` is decimal-like and `currency` is text. It does not run `money.of`'s rules: minor units, known code, no exponent, fits an int. The 500s carry the reason only in the log, so a client cannot tell what it did wrong.

**Suggestion:** give patterns and records a `money` type (`{ amount: money }`, `route ... ? { price: money }`) that runs `money.of`'s rules on `{ amount, currency }` and fails the request with a 400 saying which rule, as other pattern types do. Until then, the guide's paragraph should say that the record is a shape check, and show `money.of(json.amount.amount, json.amount.currency) ?? reply 400`.

### Q-622. A map with more decimals than its currency is rounded by some `money` functions and not by others

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time output, then reading money.rs | 1 |

`money(args, ...)` (money.rs:296), which every function but `of`, `from_minor` and `zero` uses to read its argument, takes the amount as it is. Each function then rounds it or does not:

```quillo
let raw = { amount: 1.005m, currency: "USD" }      // from JSON, a record, a form, the database
print(money.compare(raw, money.of(1, "USD")))       // 1: more than $1.00
print(money.format(raw, "en-US"))                   // $1.00
print(money.to_minor(raw))                          // 100
print(money.add(raw, money.zero("USD")))            // 1.00m
print(money.neg(raw))                               // -1.005m, still three decimals
print(money.is_zero({ amount: 0.001m, currency: "USD" }))   // false, though it shows as $0.00 and is 0 cents
```

Rounded (half to even): `add`, `sub`, `mul`, `round`, `to_minor`, `allocate`, `format`. Not rounded: `compare`, `is_zero`, `neg`, and `==` on the maps. So a check and the arithmetic after it see different amounts. `if money.compare(amount, money.zero("USD")) > 0` passes $0.004, which is then $0.00 everywhere it is added or written. The browser behaves the same (runtime.js:2259).

**Suggestion:** read a map's amount the way `money.of` reads one without a mode: more decimals than the currency has is an error. A runtime error is enough here, since the map was made by the program; data from outside comes through Q-621's `money` type or `money.of`. Rounding it in `money()` would at least make all functions agree.

### Q-623. Q-096's checks are `money.of`'s only

| Kind | Detected by | Attempts |
|---|---|---|
| silent / loud | run-time output | 1 |

Q-096 made `money.of` refuse `"1e3"` and amounts whose minor units do not fit an int, "so that `money.to_minor` always can" (the guide). `money.of` also trims and upper-cases the code. None of that holds once a value is made another way:

```quillo
print(money.add({ amount: "1e3", currency: "USD" }, money.zero("USD")))   // 1000.00m: text amounts in maps read exponents
print(money.of(5, " usd "))                                                 // 5.00 USD
print(money.add({ amount: 5, currency: "usd " }, money.zero("USD")))       // runtime error: unknown currency
let big = money.of("92233720368547758.07", "USD")                           // fits
let twice = money.add(big, big)                                             // not an error value
print(money.to_minor(twice))                                                // runtime error: "integer overflow"
```

`money.mul(big, 1000000)` and a running sum do the same. 92 quadrillion dollars is not a real amount, but the limit is lower for CLF and UYW (4 decimals: 9.2e14), and a wrong factor in `money.mul` reaches it in any currency. When it does, "integer overflow" does not say which money or which call.

**Suggestion:** one reader for both paths: `money()` uses `money.of`'s text rules (no exponent, trimmed code), and `add`, `sub`, `mul` and `allocate` give an error value (or a runtime error naming the amount) when the result's minor units no longer fit an int. Then the guide's "always can" is true.

### Q-624. A German visitor's "1.000" is €1.00

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time output | 1 |

`money.of` reads text as en-US writes it. It refuses `"1,234.50"`, `"12,50"` and `"1.000,00"` with error values, as it should. But `"1.000"`, a thousand in de-DE, fr-CH, nl, es-ES, it, pt-BR, id and others, has three decimals that are all zeros. Dropping them is exact, so the "more decimals than EUR has" check passes:

```quillo
print(money.of("1.000", "EUR"))     // { amount: 1.00m, currency: "EUR" }
print(money.of("2.500", "EUR"))     // 2.50
let m = money.of("1234.5", "EUR")
print(money.of(money.format(m, "de-DE"), "EUR"))   // err: not an amount: "1.234,50 €"
```

The guide's own example is `money.of(form.amount, "EUR") ?? reply 400`. With a German form, one thousand euros becomes one euro and no error is raised. `money.format` writes amounts in 48 locales, but nothing reads them back in those locales.

**Suggestion:** a `money.parse(text, currency, locale)` that uses the `LOCALES` table money.rs already has, with the program's `locale()` when none is given. It would read the group and decimal marks and the symbol, and refuse text that fits neither reading. The guide's form example should use it, or say that `money.of` takes en-US text only.

### Q-625. A PostgreSQL `money` column is read as cents whatever the database's currency is

| Kind | Detected by | Attempts |
|---|---|---|
| silent, data | reading pg.rs, then PostgreSQL 17 | 1 |

`other_type` (pg.rs:1383) reads PostgreSQL's binary `money` as an int64 with 2 decimals. PostgreSQL keeps it in the smallest unit of `lc_monetary`'s currency, which has 0 decimals for yen, 3 for dinars:

```quillo
// the database: alter database app set lc_monetary = 'ja_JP.UTF-8'
db.open("postgres://u@127.0.0.1/app")
db.exec("create table t (m money)")
db.exec("insert into t (m) values ($1)", 500m)
print(db.query("select m, m::text as t from t"))   // [{ m: 5.00m, t: "￥500" }]
```

Rows written before the change read the same way: `psql` shows ￥1,234 and Quillo `12.34m`. A value written and read back is 100 times smaller, and nothing fails. The value also has no currency, so it cannot be told apart from dollars.

**Suggestion:** read the connection's `lc_monetary` once (`show lc_monetary`, or the frac digits from `to_char(1::money, ...)`) and scale by it. Alternatively, refuse `money` columns with a message saying to use `numeric(p, s)`, as PostgreSQL's own docs advise. The guide's "In databases" paragraph does not mention `money` for PostgreSQL.

### Q-626. One amount, four roundings

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time output, comparing | 2 |

Each place that rounds money uses its own rule:

| Where | Rule | `0.125` to 2 places | `1.005` to 2 places |
|---|---|---|---|
| `money.mul`, `add`, `sub`, `round`, `to_minor`, `format` (no mode given) | half to even | 0.12 | 1.00 |
| `round(dec, 2)` | half to even | 0.12 | 1.00 |
| `format_number(x, loc, 2)` | half away from zero, from the shortest decimal (Q-369) | 0.13 | 1.01 |
| `round(float, 2)` | half away from zero, of `x * 100` in binary | 0.13 | **1.0** |
| PostgreSQL `numeric(12, 2)` on insert | half away from zero | 0.13 | 1.01 |
| `Intl.NumberFormat` | half expand | 0.13 | 1.01 |

An invoice written the obvious way shows one figure and adds another:

```quillo
let line = money.mul(money.of(2.50m, "USD"), 0.05m)    // 0.12 (half to even)
print(format_number(2.50m * 0.05m, "en-US", 2))        // "0.13": what the line shows
```

Half to even is a sound default for money, and the guide says so. But `format_number`, the only way to write a plain decimal with a locale's marks, rounds the other way. `round(float, 2)` disagrees with both, and with the comment at money.rs:242 that says `round` rounds as `format_number` does: `round(1.005, 2)` is `1.0` and `round(2.675, 2)` is `2.68`, since `2.675 * 100` happens to round up in binary. A decimal sent to a PostgreSQL `numeric(12, 2)` column with more places is stored half away from zero, so the stored row differs from what `money` worked out.

**Suggestion:** give `format_number` a mode, as `round` has, or let it take money's half-to-even for decimals. Make `round(float, n)` go through the shortest decimal as `format_number` does, so `round(1.005, 2)` is 1.01. Add the table above to the Decimals section, with a line on the database's rounding.

### Q-627. `money.format` is not "the same as `Intl.NumberFormat`"

| Kind | Detected by | Attempts |
|---|---|---|
| silent / docs | comparing 18,144 and 990 calls with Node's `Intl` (ICU 77.1, the version LOCALES names) | 1 |

Within the 48 covered locales, 36 currencies and 9 amounts (15,552 calls, each amount rounded first so that only the writing is compared), 414 differ, all from two gaps. All 165 currencies in en-US, de-DE, ja-JP, fr-FR, en-IN and pt-BR (990 calls) show more:

- **Decimals.** 13 currencies have ISO minor units that CLDR does not display: AFN, ALL, IQD, IRR, KPW, LAK, LBP, MGA, MMK, RSD, SOS, SYP, YER. `Intl` writes `IRR 1,234`; Quillo writes `IRR 1,234.00`, and `IQD 1,234.000`. ISO's units are right for storing, but the guide says the format is the browser's.
- **Symbols missing in every locale.** XOF (`F CFA`) and XAF (`FCFA`), the currencies of 14 countries, plus XPF (`CFPF`, `FCFP` in French), XCD (`EC$`) and XCG (`Cg.`): Quillo writes the code.
- **Symbols missing in French.** `$CL`, `$CO`, `$BM`, `$BN`, `$BZ`, `$FJ`, `£FK`, `£GI`, `$NA`, `$SB`, `$SR`, `$TT`, `$UY`, `$WS`.
- **Locales outside the table** follow the guide's rule (the first locale of the language, else en-US), and the result is another country's format with no warning:

| Locale | Quillo | `Intl` |
|---|---|---|
| es-CO, $1,234.50 | `1234,50 US$` (es-ES) | `US$ 1.234,50` |
| zh-Hant-TW, NT$1,234.50 | `NT$1,234.50` (zh-CN) | `$1,234.50` |
| ar-EG, EGP 1,234.50 | `EGP 1,234.50` (en-US) | `‏١٬٢٣٤٫٥٠ ج.م.‏` |
| sr-Latn-RS | en-US | Serbian |

Latin America's Spanish (es-CO, es-CL, es-PE, es-VE, es-UY, es-419) all take Spain's format: no group in four digits, and the symbol after.

**Suggestion:** add the five symbols to `SYMBOLS` and the French ones to the fr rows, and use CLDR's display digits in `format_amount` while `minor_units` keeps ISO's. Map a script subtag (`zh-Hant-*`) to zh-TW, and add es-419 as the fallback for `es-` outside Spain. For a locale with no row, log once that it is written as another locale, and change the guide's "the same as `Intl.NumberFormat`" to "as `Intl.NumberFormat` does for these locales".

### Q-628. In SQLite every column type for money has a trap that only the guide mentions

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time output | 2 |

```quillo
db.exec("create table t (price money, p2 decimal(12,2), p3 decimal_text)")
db.exec("insert into t (price, p2, p3) values (?, ?, ?)", 12345678901234567.89m, 0.10m, 9.99m)
db.exec("insert into t (p3) values (?)", 100.00m)
print(db.query("select price, p2 from t")[0])         // { price: 12345678901234568m, p2: 0.1m }
print(db.query("select max(p3) as m from t")[0])      // { m: "9.99" }: text, and not the largest
print(db.query("select p3 from t order by p3"))       // 100.00 before 9.99
```

The Decimals section explains each of these: NUMERIC affinity, use `decimal_text`, and do sums and ordering in Quillo. But `money` and `decimal(12,2)` are the names a model writes first, and nothing at `check` or at run time says the cents can go. 15 significant digits is IDR 10 trillion with cents, or USD 10 trillion. `max(p3)` also loses the column's declared type, so it comes back as `str`, and `sum(p3)` as a float. A money map put in a text column comes back as its JSON text, which no `money` function reads.

**Suggestion:** have `check` warn on a `create table` for SQLite with a `money`, `decimal(` or `numeric(` column: "SQLite stores this as a float past 15 digits: use decimal_text, or the minor units in an integer column". Reading an aggregate over a `decimal_text` column (`max`, `min`, `sum`) could also keep the column's kind when the result is a bare column name.

### Q-629. `db.exec(sql, [x])` stores the list as JSON text

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time output | 1 |

Found while writing Q-628's repro. Python's `sqlite3` takes parameters as a tuple or list, and that is what I wrote first:

```quillo
db.exec("create table t (price decimal_text)")
db.exec("insert into t (price) values (?)", [9.99m])
print(db.query("select price from t"))     // [{ price: "[\"9.99\"]" }]
```

`check`: ok. Quillo's parameters are separate arguments, and a list parameter is stored as JSON, so the one `?` gets the whole list. With five `?`s and one list, the error is clear ("the query has 5 parameters, got 1"), so only the most common case, one parameter, is silent.

**Suggestion:** `check` can warn when the only parameter is a list literal and the SQL text has one `?`: "parameters are separate arguments: `db.exec(sql, a, b)`; a list is stored as JSON". At run time, the same warning once per call site.

### Q-630. Money with a withdrawn or unlisted code is a runtime error in every `money` function

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run-time output | 1 |

`currency()` (money.rs:284) gives an error value for an unknown code, "it may come from data". But `money()`, which reads a money map, fails the program for the same code (money.rs:303). Codes that leave the table therefore break stored rows: BGN since Bulgaria joined the euro on 2026-01-01, and HRK. So do codes a shop may hold that ISO does not list, such as crypto currencies (BTC, USDT), whose 8 decimals are not money here either:

```quillo
let old = { amount: 120.00m, currency: "BGN" }     // last year's invoice, from the database
print(money.format(old, "bg-BG"))                    // runtime error: unknown currency
```

Every page that shows a 2025 Bulgarian invoice is a 500, and listing the invoices fails on the first such row.

**Suggestion:** keep withdrawn codes readable: a second table of withdrawn currencies with their units, accepted by `money()` and `format`, refused by `money.of` for new amounts, or allowed with a note. And give `money()` an error value for an unknown code, as `currency()` does, so a list can skip a bad row.

### Q-631. `money.allocate` gives the leftover units to the first shares

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | run-time output | 1 |

```quillo
print(money.allocate(money.of(0.10m, "USD"), [1, 99]))    // 0.01 and 0.09
```

The 1% share's exact part is $0.001. Rounded down, the shares are 0.00 and 0.09, and the one cent left goes to the first share with a ratio, not to the share closest to its next cent (0.099). So a 1% platform fee on a $0.10 sale takes 10%. This is Fowler's method, as the guide says, and the shares add up. But the guide also says "each share gets its part", and for skewed ratios the first share gets far more than its part.

**Suggestion:** hand the leftover units out by largest remainder (Hamilton's method), with ties to the first share. That keeps today's answers for equal ratios (33.34, 33.33, 33.33) and gives 0.00 / 0.10 here. If Fowler's order matters to someone, offer it as a mode.

### Q-632. There is no exact power of a decimal

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` | 1 |

```quillo
let monthly = 0.05m / 12
let factor = pow(1m + monthly, 360)    // error: argument 1 of `pow` must be num, got dec
```

`**` is refused with "`pow(x, 2)`", `pow` and `math.pow` refuse a `dec`, and `stats.mean` does too. A loan's payment, compound interest and an APR all need `(1 + r)^n`. The ways out are `dec(math.pow(float(x), n))`, which is inexact and exactly what decimals are for avoiding, or a loop of `n` multiplications, which rounds at 28 digits on each step.

**Suggestion:** `pow(dec, int)` by repeated squaring on the 96-bit mantissa, with the result rounded half to even to 28 digits once, as `/` is. A `mean` that keeps decimals exact (`sum(xs) / len(xs)`) is a one-liner, but `stats.mean` could take decimals too.

### Q-633. The guide says `money.mul` gives error values, and `check` lets bad literals through

| Kind | Detected by | Attempts |
|---|---|---|
| docs / loud | run-time output | 1 |

The guide: "`quillo check` takes every `money.*` result as money, so `is_err(r) ? r.error : ...` after `money.mul` is refused by it, though `money.mul` does give an error value for a bad argument: test with `type_of(r) == "error"` there." `money.mul` never gives an error value: a string or float factor is a runtime error (money.rs:434). So the advice to test with `type_of` protects against nothing, and `check`'s typing is right.

`check` also lets literals that can only fail through: `money.of(1, "USD", "half-up")` (a runtime error, "unknown rounding"), `money.of(1, "XYZ")` (an error value). It also types `money.of(...).amount` as anything, so `money.of(1, "USD").amount + 0.5` passes `check` and fails at run time, where `12.5m + 0.5` is caught.

**Suggestion:** fix the sentence: `money.mul` and the other functions fail on a bad argument, and only `of`, `from_minor`, `zero` and `minor_units` give error values. In `check`, test a literal mode against `Rounding::NAMES` and a literal code against `CURRENCIES`, and give a money map's `amount` the type `dec`.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `print("display {a.amount}")` | Quillo's markup interpolates `{}`, so I assumed strings did too | printed the braces as they are; only `` `${...}` `` interpolates | 1 run |
| `"{money.of(19.9, "USD").amount}"` | the same, with quotes inside | parse error "expected `)` after arguments, found `USD`" | 1 check |
| `try { ... } catch e { ... }` | JavaScript habit | a clear message: "there is no `try`: errors are values" | 1 check |
| `db.exec(sql, [a, b, c, d, e])` | Python's `sqlite3` | with five `?`s, a clear error; with one, a silent JSON row (Q-629) | 2 runs |
| `reduce(xs, f, init)` | JavaScript's order | `check`: "argument 3 of `reduce` must be fn" | 1 check |
| `total = total + money.mul(...)` | `+` adds; the money docs list `money.add` but nothing warns | a wrong total, `check` ok (Q-620) | found by reading the output |
| `sum(map(lines, (l) => l.total))` on money | `sum` works on decimals | "cannot add int and map": correct, but it does not say `money.add` | 1 run |
| `money.of(1.5, "USD")` | a float literal | `check`: "must be int \| dec \| str, got float" | 1 check |
| an error value of `money.of` put straight into a component's list | wanted to show it | the page was a 500 "not an amount", as an error no statement took is; `type_of(x) == "error"` before showing it fixed it | 1 run |

## Limitations

- No `money.sum`, `money.min`, `money.max`, `money.abs` or `money.is_negative`. A sum is `reduce(xs, money.zero(c), (a, x) => money.add(a, x))`, and `sum` of money says "cannot add int and map".
- No locale-aware reading of amounts (Q-624), no accounting format (`(1,234.50)` for negatives), no compact format (`$1.2K`), and no percentages: `format_number(x * 100, loc, 1) + "%"` puts the sign where en-US does, not where fr-FR (`12,5 %`) or tr-TR (`%12,5`) do.
- No cash rounding: CHF to 0.05, and the cash rules of SEK, DKK and others, are the program's arithmetic. `money.round` only goes to the currency's units, so it takes `money.of(round(m.amount * 20) / 20, "CHF")`.
- No exchange rates, by design (the guide says so), and no currency in PostgreSQL's `money` (Q-625).
- Codes without minor units (XAU, XDR) and crypto currencies are not money (Q-630).
- `dec("1e-29")` and text with more than 28 decimals are error values ("not a decimal"), not rounded, unlike arithmetic, which rounds past the 28th decimal. An exchange-rate API that sends 30 decimals needs the text cut first.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Server and browser, 25 money calls (raw maps, allocation, formats in 6 locales, `+` merge, `1m / 3`, exponents, codes with spaces, BHD's 3 units, `==` with an int amount) | identical in all 25, no console errors, no remade components |
| `money.of` text: `"+5"`, `".5"`, `"5."`, `"0012.50"` | 5.00, 0.50, 5.00, 12.50 |
| `money.of` text: `""`, `" "`, `"-"`, `"--5"`, `"- 5"`, `"5-"`, `"1.2.3"`, `"1_000"`, `"0x10"`, `"NaN"`, `"Infinity"`, `"$12.50"`, `"12.50 USD"`, Arabic and full-width digits | all error values ("not an amount") |
| `money.of("1e3")`, `money.of("12.5E1")` | error values (Q-096's fix) |
| `money.of(0.5m, "JPY")`, then with `"half_up"` | error value, then 1 |
| `money.add`/`money.compare` of USD and EUR | runtime error: "convert one currency to the other first" |
| `a - b`, `a * 3`, `a < b`, `a + 1m` on money | refused by `check` |
| a float to `money.of` or `money.mul` | refused by `check`; at run time, "convert it with dec(x)" |
| `money.allocate` of -0.05 by 3, by `[0.5m, 0.25m, 0.25m]`, by `[1, 0, 1]` | -0.02, -0.02, -0.01; 5.00, 2.50, 2.50; 5.00, 0.00, 5.00: always the whole amount |
| `money.neg(money.zero("USD"))`, `money.of("-0.00")`, formatting -0.001 | 0.00, never `-0.00` (`Intl` writes `-$0.00`) |
| `json.stringify` then `json.parse` of money, then `money.add` | 39.80: text amounts are read |
| PostgreSQL `numeric(12,2)` and bare `numeric` | `12.30m` keeps its scale; `n / 3` is `4.1000000000000000m`; `sum(n) over ()` stays a decimal |
| `money.format` in the 48 covered locales, 34 of 36 common currencies | the same as `Intl`, including de-CH's `’`, fr-FR's narrow no-break space, en-IN's lakhs, nl-NL's `€ -1.234,50` and de-AT's grouping |
| `0.1m + 0.2m == 0.3m`, `12.5m == 12.50m`, `1.10m * 3` | true, true, `3.30m` |
| `dec + float`, `dec < float`, `sum([1m, 0.5])`, `min([1m, 0.5])` | refused with "convert one with dec(x) or float(x)" |
| `1m / 0`, `dec(1e300)` | "division by zero"; an error value "too large" |
