# Feedback from building 100 smart components in Quillo

| | |
|---|---|
| App | None: 100 new `component`s for the Quillo UI kit, each doing real work in the browser (importing CSV, spreadsheets, cron, settling debts, critical path, statistics), on branch `smart-components` of `~/dev/quillo-components`. 677 components in all. About 300 new files (`ui/`, `demos/`, `tests/`), written in ten batches of ten |
| Quillo | 0.6.0 (5f7f76c, 2026-11-01), a pinned copy of `target/release/quillo`, so the later work on `feedback24-fixes` is not in it |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, headless Chrome through the kit's `scripts/test.sh`, `--watch` for components remade on load, widths of 300 to 375 px |

**Status of the components:** all 100 pass in headless Chrome, and with every `ui/` file loaded as one
program none uses a name it did not take. The full run of the kit passes 671 of 677. The other six are
old failures, none in a new component (see [The kit](#the-kit)). Nothing has been tried on a phone.

**Not fully delivered, from the manifest:** CitationBuilder warns of Title Case and does not convert a
title to APA's sentence case, and reads "Ursula K. Le Guin" as family name "Guin" (it shows how it read
the name, and "Le Guin, Ursula K." is right). SlotFinder and TimezoneOverlap refuse real time zones off a
list of about 120 (Q-317). Keyboard tests in the first ten batches use synthetic key events in several
places, and most components were looked at in only one of the two themes, or not looked at.

**How far to trust this:** each batch ran its own repros. The issues below were then reproduced a
second time, in one session, on the version above, with real mouse and key events where it matters.
The exceptions are named in the issue. All timings are from one run on a machine that was busy
with other work, so read them as shapes rather than as measurements.

**Numbering:** these start at Q-300 on purpose. FEEDBACK26 (100 plugins) was being written at the
same time and is taking numbers from Q-197, so sequential ones would collide. None of these is in
`feedback/INDEX.md` yet, and they have no files in `feedback/repros/`: the repros are in this report.

What worked well: top-level pure functions compile to the browser unchanged, so logic could be
written and tried on the server in seconds. In one batch, nine of the ten components worked in the
browser the first time their logic worked on the server. `dec` and `money.*` were exact and agreed on both sides
in every batch that compared them (74 of 75 probes in one, the odd one being a JSON error's wording).
They are fast too: 20,000 decimal multiply-and-rounds took 14 ms in Chrome, so a month-by-month loan
simulation can run on each keystroke. `date.*` got daylight saving and +05:45 offsets right the first
time. No component was remade on load in the nine batches that checked, which matters given how
many server and browser differences are listed below. Most failing tests, in every batch, were the
model's own arithmetic in the test, and not Quillo.

The theme of this report is that **the server and the browser are two implementations, and the guide
says they are one.** Nine of the issues below (Q-300 to Q-303, Q-308, Q-309, Q-313 to Q-315) are a
difference between them. The second theme is
that bad input cannot be handled: there is no `try`, and `??` catches only some errors, so "bad input
never throws" means proving each operation safe by hand. Guards of this kind are a large share of
the code in these 100 components.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-013 family | `json.stringify(json.parse("1.0"))` is `1.0` on the server and `1` in the browser, so a whole float is formatted by hand in several components (see Q-309) | 1 |
| Q-014 | Marked fixed, but under plain `quillo run` the console still says only `quillo: text` or `quillo: extra`, naming neither the component nor the difference. `--watch` gives more, so this may be by design | 1 |
| Q-018 kin | The same quadratic shape as maps, now in lists and strings (Q-313, Q-314) | 1 |
| Q-041 kin | A function in markup given a state still remakes its input (Q-306) | 1 |
| Q-044, the other way round | A false warning on `/` inside `money.of` and `money.mul` (Q-321) | 3 batches |
| Q-051 kin | The `!=` form was fixed; `a && xs \| f()` was not (Q-319) | 1 |
| Q-103 family | `type_of(json.parse("2.0"))` is `float` on the server and `int` in the browser | 0 |

## New issues

### Q-300. In the browser, an int computed past 2^53 loses units, with no error

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe before building on minor units | 1 |

The guide says arithmetic gives the same results on both sides, and that ints past 2^53 reach the page
as BigInts. That holds for a literal. A value computed by `*` from smaller ints is a double.

```quillo
fn sum_it() {
    let c = 4000000000000000 * 3
    `${c + 1}`
}
```

Server `12000000000000001`, browser `12000000000000000`. `d % 10`, `dec(d)` and
`money.to_minor(money.from_minor(d, "USD"))` then carry the wrong value on. `quillo check` says ok.
This is the most serious issue here because it is money: amounts in cents reach it at about 90
trillion, which an unguarded sum of large lines can do.

Workaround: every amount is capped so that minor units stay under 2^53, with a message past that.

**Suggestion:** a checked multiply that moves to BigInt past 2^53, as a literal does, or an error value.

### Q-301. `json.parse` rounds large ints in the browser, and `int()` past 64 bits gives a different number

| Kind | Detected by | Attempts |
|---|---|---|
| silent | server against browser probe | 1 |

| | Server | Browser |
|---|---|---|
| `json.parse("9007199254740993")` | `9007199254740993` | `9007199254740992` |
| `int("99999999999999999999")` | `err("not an int: ...")` | `100000000000000000000` |

An id in a JSON document comes out wrong without a word, which is why JwtInspector could not simply
use `json.parse`: a plain port would have shown the wrong user id. JsonFormatter and JwtInspector
share a JSON parser of their own, built on `re.find_all`. SchemaInferrer and JsonQuery each have a
scanner of about 90 lines, for Q-323. Q-111 fixed the float case of `int`, not text.

**Suggestion:** a BigInt-preserving parse in the browser, and `int()` refusing text past 64 bits there too.

### Q-302. A map's integer-like keys move to the front in the browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | server against browser probe, in two batches | 1 |

```quillo
keys({ "10": 1, "2": 2, b: 3, "1": 4 })
group_by([1, 2, 3], (x) => x % 2)
```

| | Server | Browser |
|---|---|---|
| `keys` | `["10", "2", "b", "1"]` | `["1", "2", "10", "b"]` |
| `group_by` | `{ "1": [1, 3], "0": [2] }` | `{ "0": [2], "1": [1, 3] }` |

This is JavaScript's object order. `group_by` turns int keys into text, so it is the ordinary path, not
a rare one. `values` and `json.stringify` follow it. With `--watch`, a page showing such a map says
`<P> was made again, not taken up from the server's HTML`. Q-121 already treats key order as meaningful.

Workaround: sort by a first-seen index, or keep a separate list of ids. There is none for showing a
document in its own order, so SchemaInferrer and JsonQuery keep the limit.

**Suggestion:** keep maps in insertion order on both sides, for instance as a `Map` in the runtime.

### Q-303. In the browser, reading a map by index throws if the map holds the key `constructor`

| Kind | Detected by | Attempts |
|---|---|---|
| silent on the page, a console line | a probe looking for this class of bug | 3 |

```quillo
fn read(m, k) = m[k]
fn word_count(words) {
    let m = {}
    for w in words { m = set(m, w, get(m, w, 0) + 1) }
    m
}
// in a component: read(word_count(["constructor", "a"]), "constructor")
```

Server `1`. In the browser the handler stops, and the console says
`` quillo: `constructor` is not used from Quillo code: it runs text as code or HTML ``. So any word
count breaks on a text with that word in it, and only on the visitor's machine.

From reading `src/client/runtime.js`: `foreign()` tests `v.constructor !== Object`, and a map with its
own `constructor` key fails that, so a program's own map is taken for a page object. `get`, `has`,
`keys`, `set`, `group_by` and `for [k, v] in m` are fine.

Workaround: `get(m, k, 0)`, never `m[k]`, on maps keyed by data.

**Suggestion:** test the prototype, `Object.getPrototypeOf(v) === Object.prototype`, and not a field the
data can set.

### Q-304. After a wait in a handler, `e.value` reads the state again, and the field empties

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test that found the field empty | 3 |

```quillo
component X() {
    state text = ""
    let go = () => { js_new("Date") }
    <input value={text} @input={(e) => { go(); text = e.value }}/>
}
```

Typing "hello" leaves the box empty and `text` at `""`, with no error and no console line. Without
`go()` it works. `js_new("Date")` is not even a promise: a `let` function that touches `js` is
awaited since Q-102's fix, and at that first wait the input is set back to the state. That cause is
inferred from behaviour; the compiled code was not read. The guide says `e.preventDefault()` must
come before a wait, but not that `e.value` changes across one.

Workaround: read `e.value` into a `let`, and assign the state before anything that waits.

**Suggestion:** take the event's value once when the handler starts.

### Q-305. `x?.y` is accepted, and quietly rebuilds the whole component on every change

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test holding elements that had been replaced | about 4, the most of any issue here |

The guide says there is no `?.`, but it parses as `(x?).y`, and `quillo check` says nothing. It reads
as JavaScript's optional chaining and gives the right value. But `?` counts as a way out of the body,
and a body that can leave early is made again whole whenever what it read changes (`src/client.rs`,
about line 2362). The guide does not mention that mode at all.

```quillo
component C() {
    state typed = "1"
    let shown = typed == "" ? null : { text: typed + "!" }
    <div>
        <input id="in" value={typed} @input={(e) => { typed = e.value }}/>
        <p>{shown?.text ?? "none"}</p>
    </div>
}
```

Typing two characters with real key events gives `12`: the first takes the focus away, so the second
goes nowhere. With `shown.text` in place of `shown?.text`, typing the same two gives `123` and the
focus stays. A test with a button shows no problem, which is why the first repro failed; it takes an
input. Near FEEDBACK26's Q-198, where `?` in a lambda also leaves somewhere a model does not expect,
but this is a separate fault.

Workaround: `shown.text`, as a field of null is null, and no `?` in a component's body.

**Suggestion:** `quillo check` warns on `?.`, and the guide says what `?` and `return` do to a body.

### Q-306. A function in markup given a state remakes its input, and focus is lost

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading the model's own code | 1 |

```quillo
component P() {
    state on = false
    let tick = (v: bool) => <label><input id="box" type="checkbox" checked={v} @change={(e) => { on = e.checked }}/>x</label>
    <div>{tick(on)}</div>
}
```

After a click the checkbox is a new element and focus is on `<body>`, so a keyboard user tabs back from
the top after each box. Q-041 fixed this for `&&` and `if`. The guide states it only for a plain
function used as a tag.

Workaround: a `view` with live props.

### Q-307. `take` and `drop` ignore a negative count without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test asserting exact text | 1 |

```quillo
take([1, 2, 3, 4], -2)        // []
drop([1, 2, 3, 4], -1)        // [1, 2, 3, 4]
slice([1, 2, 3, 4], -2, 4)    // [3, 4]
```

The same on both sides, and `quillo check` says ok. `slice` and `xs[-1]` count from the end, so a
model expects these to. HabitTracker's "Streaks:" line came out empty; without an exact assertion that
reads as a habit with no history. The guide's table of edges gives `slice`'s negatives and does not
list `take` or `drop`.

**Suggestion:** count from the end as `slice` does, or give an error.

### Q-308. `re` capture groups in a loop differ between server and browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe of nine patterns | 1 |

| | Server | Browser |
|---|---|---|
| `(?:(a)\|b)*` on `ab`, first match's groups | `["a"]` | `[null]` |
| `(a*)*` on `b`, first match's groups | `[""]` | `[null]` |

The match text agrees; only the groups differ. JavaScript clears a group at each round of a loop, and
the `regex` crate does not. The guide says `re` gives the same answers on both sides.

Workaround: RegexTester copies the browser. A server-side `re` still differs.

### Q-309. Other conversions differ between server and browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | server against browser diffs of 38 to 120 builtins, in several batches | 1 |

| | Server | Browser |
|---|---|---|
| `str(1e21)` | `1000000000000000000000` | `1e+21` |
| `str(0.0000001)` | `0.0000001` | `1e-7` |
| `dec(1e30)` | an error value | a `dec` |
| `float("NaN")`, `float("inf")` | `NaN`, `inf` | `err("not a number: ...")` |
| `json.stringify(json.parse("1.0"))` | `1.0` | `1` |
| `math.erfc(1)` | `0.15729920705028516` | `0.157299207050285` |

The last replaces the server's text after waking, which here printed nothing to the console on a
small page and `quillo: text` on a larger one. The guide says `str` never uses exponent form.
`json.parse`'s error text differs too, by side, and has a line and column only on the server (Q-323).

Workaround: format through a rounding helper, and check text with a regular expression before `float`
or `dec`.

### Q-310. `x.n` also reads a local named `n`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | cutting a component down until one `let` made the difference | part of Q-311's 14 |

```quillo
fn twice(n) = n * 2
component P() {
    state a = 3
    let n = a > 0 ? 1 : 2        // never used; rename it and the error below goes away
    let rows = [{ id: "x", found: a > 1 ? { n: a } : null }]
    <div>
        <button @click={() => { a = 1 }}>b</button>
        <ul>{for f in rows => <li key={f.id}>{f.found != null && <small>{twice(f.found.n)}</small>}</li>}</ul>
    </div>
}
```

It compiles to `W.fieldp(W.fieldp($row.v, "found", null), "n", $n_2.v)`: the fallback argument
(`self.fallback(name)`, `src/client.rs` about line 2965) reads the local, so the place depends on it.
A field read should depend on the row only. With `let n` a click gives
`quillo: cannot multiply null and int`; with `let m`, in an otherwise identical program, nothing.

### Q-311. A place inside a row's `cond && <markup>` can run before the branch goes, and throws

| Kind | Detected by | Attempts |
|---|---|---|
| console only | the harness saw a page error while every assertion passed | about 14 probes, most failing to reproduce it because of Q-310 |

```quillo
component P() {
    state a = 3
    let rows = [{ id: "x", found: a > 1 ? { n: a } : null }]
    <div>
        <button @click={() => { a = 1 }}>b</button>
        <ul>{for f in rows => <li key={f.id}>{f.found != null && <b>{f.found.n * a}</b>}</li>}</ul>
    </div>
}
```

`found` is null, so the branch goes and `<b>` should never be worked out, yet a click gives
`quillo: cannot multiply null and int`. The page ends up right, and in production the message has no
line. From reading `flush` in `runtime.js`, not confirmed by changing it: the inner place reads `a`
directly and is queued in the first round, while the row's new item reaches the outer condition only
when the list's effect runs. Effects are ordered by id only within a round.

Workaround: work the text out when the row is made, so markup reads only a field.

### Q-312. A `\r\n` in an attribute reads differently from the server's HTML and after an update

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | GroupSummary's test | 1 |

`value={s}` with `"a\r\nb"` reads as 3 characters from the server's HTML, as the parser folds it, and a
value set by state keeps 4. Writing `&#13;` would keep it.

Workaround: send LF in a hidden field.

### Q-313. `s[i]` copies the whole string in the browser, so a loop over a text is quadratic

| Kind | Detected by | Attempts |
|---|---|---|
| perf, silent until the input is long | timing probes, in three batches | 1, and one hung run |

The guide's table shows `"héllo"[1]` with no warning, and a `for` over a text's characters by index is
what anyone writes first. It works on the small text of a demo. `index()` in `runtime.js` does
`Array.from(o)` for each `s[i]`. A probe at 200,000 characters had not finished after 120 s.

| | Server | Browser |
|---|---|---|
| `s[i]`, 20,000 reads | 54 ms | 873 ms |
| the same over `split(s, "")` | 7 ms | 1 ms |

**It is already in the kit.** `ui/csv_table.qwe` parses with `text[i]`. Parsing a CSV of about 160,000
characters took 29.5 s in Chrome and 1.2 s on the server. See [The kit](#the-kit).

Workaround: `split(text, "")` once, and index the list.

**Suggestion:** an ASCII fast path in `index()`, or an index cached per string.

### Q-314. List `set` is quadratic on both sides and `push` in the browser, though the guide says in place

| Kind | Detected by | Attempts |
|---|---|---|
| perf, a docs error | a failed time limit, then timing probes | 2 |

The guide says a list built in a loop with `push` or `set` "is changed in place".

| | Server | Browser |
|---|---|---|
| `push`, 100,000 | 57 ms | 1,576 ms |
| `set` on a list, 50,000 | 4,051 ms | 675 ms |
| `for i in 0..n => i`, 100,000 | 2 ms | 1 ms |

So the server's `push` is in place and its list `set` is not, and the browser's are neither. `set` on
a map is fine, since Q-018.

Workaround: build with `for`, or gather chunks and `flatten` them. It made an edit distance 2.5 times
faster.

### Q-315. A pattern instant on the server can hang the browser, and nothing can stop it

| Kind | Detected by | Attempts |
|---|---|---|
| perf, silent | timing hostile input in the browser | 2 rounds on one component |

The answers agree (but see Q-308); the costs do not. The server's engine is linear and the browser's
backtracks, so the difference has no limit, and neither `quillo check` nor the server's render shows it.

| | Server | Browser |
|---|---|---|
| A sentence splitter, `[.!?]+["']*(?:\s+\|$)`, on 20,000 dots | 0 ms | 509 ms, growing with the square |
| `(a+)+$` on 25 a's and a `b` | 0 ms | 286 ms, doubling with each `a` |

At full size, one link pattern in SeoAudit took 46.6 s on a single keystroke. A visitor's own pattern is
safe on the server and hangs a tab in a component, with no way to stop it. This is why RegexTester
has a step-limited regular expression engine of its own, 858 lines of Quillo.

Workaround: bounded counts (`{1,8}`), no two overlapping runs in a row, and a linear pattern's matches
walked by hand in place of `[\s\S]*?`.

**Suggestion:** a line in the guide at least. Better, `quillo check` warns of nested or adjacent
overlapping runs.

### Q-316. A local `fn` that calls itself passes `quillo check`, then the program will not start

| Kind | Detected by | Attempts |
|---|---|---|
| loud, late | the gallery did not start, in three batches | 1 to 2 |

```quillo
fn outer() {
    fn f(n) = n == 0 ? 0 : 1 + f(n - 1)
    f(3)
}
component P() {
    state o = 0
    <button @click={() => { o = outer() }}>{o}</button>
}
route GET / = <html><body><P/></body></html>
```

`quillo check` says ok. `quillo run` stops with `` `outer` (compiled for the browser): unknown name `f` ``.
On the server alone the same code prints 3, and the guide says `fn` inside a function is a local closure.
One bad function takes the whole program down, and only when a component reaches it.

Workaround: a top-level `fn` with its state passed in, or `let f = null; f = (i) => { ... }`.

**Suggestion:** compile a local `fn` so it can see itself. Failing that, have `quillo check` run the
browser compile, so the error comes before the server starts.

### Q-317. An unknown time zone cannot be tested for or caught, and one bad prop is a 500

| Kind | Detected by | Attempts |
|---|---|---|
| loud, limitation | the first probe | 1 |

```quillo
print(date.to_zone("2026-10-27T14:30:00Z", "Mars/Base") ?? "fallback")
```

`` quillo: `date.to_zone`: unknown time zone "Mars/Base" ``, which `??` does not catch. `quillo check`
passes it even as a literal. In a route the page is a 500; in the browser the handler stops. A zone is
among the commonest values a program takes from a user, and `date.valid` has no equal for it.

The workaround is poor: SlotFinder and TimezoneOverlap accept only TimezonePicker's list of about 120
zones, so **a real zone off the list, such as `Asia/Ho_Chi_Minh`, is refused with a message.** The
browser could ask `Intl`, but the first render is the server's.

**Suggestion:** `date.valid_zone(z)`, and an error value here.

### Q-318. Nothing else can be guarded either: there is no `try`, and `??` catches only some errors

| Kind | Detected by | Attempts |
|---|---|---|
| loud, limitation | probing before building, in seven batches | design time, not attempts |

```quillo
print(int("x") ?? 0)                                     // 0: an error value
print(79228162514264337593543950335m * 10 ?? "caught")   // stops: decimal overflow
print(1m / 0m ?? "caught")                               // stops: division by zero
print(date.of(2026, 2, 30) ?? "caught")                  // stops: there is no date 2026-2-30
print(re.test("a", "(") ?? "caught")                     // stops: `(` without its `)`
print(stats.correlation([1, 2, 3], [1, 1, 1]) ?? 0)      // stops: an input is constant
print(money.allocate(money.of(0, "USD"), [0, 0]))        // stops: ratios must add up to more than 0
```

`quillo check` passes each. Each is a 500 on the server, and in the browser stops the handler, or a
component's `let`, leaving old figures on the page. The guide documents most of these, so they are not
wrong answers. But a calculator's numbers come from what people type, and `dec("x")` and `money.of`
already give an error value, so the rule is not one a model can guess. `money.allocate` is the odd one
out in its own module, and an empty or all-free cart reaches it naturally.

The cost is the largest in this report. ExpressionInput measures digits before each `*` and `+`.
NaturalDateInput overestimates month and year lengths so `add_months` cannot pass 9999. RuleForm takes
a dependency on an 858-line file to test a pattern from data. Every `<slug>_num` helper is here because
of this, and one review covered every multiplication in ten components.

`js_new("Promise", (done) => { done(js("localStorage.getItem", key)) })`, written directly in a
handler, turns a throw into an error value and puts nothing on `window`. That is the neatest trick
found, and worth a line in the guide. Inside a plain `fn` it stays a Promise and is not waited for.

**Suggestion:** one rule: anything a value can cause gives an error value. `re.valid` too.

### Q-319. `a && xs | f()` passes `quillo check`, and fails only when `a` is false

| Kind | Detected by | Attempts |
|---|---|---|
| loud, late | a test past 14 balances | 1 once seen |

```quillo
fn holds(xs: list, y) {
    if y != null && xs | any((x) => x == 1) { return "yes" }
    "no"
}
print(holds([1], 2))      // yes
print(holds([1], null))   // `any` expects a list as argument 1, got bool
```

The pipe binds looser than `&&`, as documented, so this is `(y != null && xs) | any(...)`. It works when
`a` is true, because `&&` gives `xs`, and that is the branch a developer tries first.

Workaround: parentheses.

**Suggestion:** `quillo check` warns on a pipe whose left side is `&&`, as for the `!=` form (Q-051).

### Q-320. Three type errors pass `quillo check` through an untyped parameter

| Kind | Detected by | Attempts |
|---|---|---|
| loud, late | failed runs | 1 each |

```quillo
fn f(d) -> str = str(round(d, 4, "half_up"))
print(f(0.1 * 3))                                  // stops: a rounding mode is for a dec only

sort_by(rows, (x) => [x.a, x.b])                   // stops: cannot compare list with list

index_of("a,b", re.find("a,b", ","))               // null, without a word
```

The first took the whole gallery down, and the test script said only "the gallery did not start". The
second is fine with one row and stops with two, and there is no multi-key sort. The third comes from
expecting `re.find` to give text, as the JavaScript table's `s.match` row suggests; it surfaced later as
"`slice` expects an int as argument 3, got null". This last repro was not run a second time.

### Q-321. `/` inside `money.of` or `money.mul` warns "must be an int", and the advice would lose the cents

| Kind | Detected by | Attempts |
|---|---|---|
| loud, false warning | `quillo check`, in three batches | 1 each |

```quillo
fn part(price, left: int, days: int) = money.of(price * left / days, "USD", "half_up")
fn off(d: map) = money.mul(money.of(10m, "USD"), d.value / 100, "half_up")
```

`` warning: argument 1 of `money.of` must be an int, and `/` gives one only when it divides evenly ``,
and the same for argument 2 of `money.mul`, though both take decimals and the answers are right. It
appears when the left of `/` is of unknown type. Its advice, `floor(...)` or `round(...)`, gives wrong
money. It is Q-044 the other way round.

Workaround: `dec(price) * left / days`.

### Q-322. `quillo check` refuses a return type on a `while true` that returns

| Kind | Detected by | Attempts |
|---|---|---|
| loud, false error | `quillo check`, in three batches | 1 |

```quillo
fn first_even(xs: list) -> int {
    let i = 0
    while true {
        if xs[i] % 2 == 0 { return xs[i] }
        i += 1
    }
}
```

`` `first_even` must return int, but this returns null ``. The function can only leave by the `return`.

Workaround: leave out the return type, or add an unreachable last line.

### Q-323. An error value's message cannot be read

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | writing a JSON field | 1 |

```quillo
let e = json.parse("[1,")
print(str(e))     // err("invalid JSON: EOF while parsing a value at line 1 column 3")
print(e.text)     // stops: error has no field `text`
```

`unwrap(e)` stops the program, a `match` arm of `err(x) =>` does not parse, and the guide has nothing on
it. Parsing `str(e)` is the only way. Chrome's text has no place in it anyway, so a component cannot
say where bad JSON fails, and showing the message would remake the component.

**Suggestion:** `e.message`, the same on both sides.

### Q-324. Syntax that is missing, with a message that does not say so

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | `quillo check` | 1 each |

| Written | Message | Workaround |
|---|---|---|
| `let walk = (n) => n > 0 ? walk(n - 1) : 0` | `` unknown name `walk` (did you mean `ask`?) `` under `check`, `` `all`? `` under `run`, and `Mark` in the kit | a top-level `fn` |
| `<>...</>` | `` expected an expression, found `<` `` | a wrapper element |
| `map(([a, b]) => a + b)` | `` expected `)` after arguments, found `=>` `` | `(p) => p[0] + p[1]` |
| `{ [k]: 1 }` | `` expected end of statement, found `:` `` | `set(m, k, v)` |
| `let a = 1, b = 2` | `` expected end of statement, found `,` `` | one `let` a line |

The guide says HTML is "written like JSX" and never mentions fragments either way.

### Q-325. A kit's file cannot be checked alone, and a gallery's errors point at a generated file

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | `quillo check` | 1 to 2, in most batches |

`quillo check ui/slug.qwe` gives a false `cannot read ui/ui/theme.qwe`, then `` unknown name `ui_cls` `` on
each use, as paths count from the program's folder and a file checked alone is its own program. Every
kit file does this. The guide does say so, and the mistake was in this work's brief, but the same
limit is why the gallery keeps 626 warnings: a library file cannot name a neighbour. Lines from
`quillo check gallery.qwe` also point into the generated `quillo-ui.qwe` (`quillo-ui.qwe:61926`), so
each has to be traced back by hand.

Workaround: a throwaway main file of `use ui/...` lines for every file, which checks all imports at once.

**Suggestion:** paths relative to the file that writes them, or a way to name the program's root.

## Where the AI struggled

| What was written first | Why | What happened | Cost |
|---|---|---|---|
| `shown?.text` | JavaScript habit, and it gave the right value | Q-305 | about 4 attempts, the most in a batch |
| A test built on elements held across a change | The harness keeps references | The elements had been replaced, which looked as if state had not changed | part of the same 4 |
| `quillo check ui/slug.qwe` in the brief | The coordinator's own mistake | A false error in every kit file, until the brief was corrected mid-run | 1 to 2 runs a batch |
| A shared SeoAudit pattern with neighbouring bounded runs | Each looked bounded | 46.6 s on a keystroke, and one test run past 150 s | 2 rounds, and a change to a splitter seven components share |
| A CashCount search through a million counts a piece | Direct, and maps are passed by value | 2.4 s on a keystroke | 1 rewrite: not Quillo's fault |
| A Newton step for a 12th root | Textbook | 1836 to the 11th overflowed a decimal at 1000% daily | 1 rewrite, to square and cube roots |
| `re.find` used as text | The JavaScript table suggests it | Q-320 | 1 run |
| Radio buttons with a fixed default `name` | Worked alone | Two on a page share a group, so one shows nothing checked. Tests passed, and only a screenshot showed it, in two batches | a fix in each, and new assertions |
| `const root` in a test | The harness lists only `$`, `$$`, `wait`, `type`, `key` | `Identifier 'root' has already been declared` | 1 run |
| Expected values worked out in the head | Speed | The largest single cause of failed runs, in every batch. One batch counts about 15, and the component was right each time | Five expected values in one batch were changed after seeing the output, so those are less independent |
| Running outside the assigned ports | Early probes | Two batches briefly used ports of another. No damage was found | none lasting |

## Limitations

- **No `try`, and no way to read an error's message.** See Q-318 and Q-323.
- **No `chr`, `ord`, `url_decode` or base64**, as the guide says. It cost more than it sounds: number to
  character goes through `json.parse` of a `\uXXXX` escape, character to number through the bytes of
  `url_encode`, and a UTF-8 decoder is written twice, in JwtInspector and UrlInspector.
- **No multi-key sort.** Chained stable `sort_by`, last key first.
- **`date.valid` refuses what databases write.** `date.valid("2026-01-01T10:00:00")` is false, and the
  same with a space. This matches the guide. Two components add the `T` and `Z` by regex.
- **Statistics are floats.** Square roots and normal quantiles have no exact decimal, so `AbTestCalc`,
  `SampleSizeCalc` and the like use floats, a deliberate exception to "rates in decimals".
- **`state` "must be data", yet `@mount={(el) => { box = el }}` holds an element.** The guide shows it, and
  it works. Small, but contradictory.
- **A plain string may span lines**, though the guide gives that only to template strings. Harmless.
- **A class used only as a test hook** sometimes gets "no style sheet has a rule for" in a small program
  and not in the gallery, for 2 of 7 classes in one case. Not explained. Batches moved hooks to `data-` attributes.

## The kit

Not Quillo, and on `smart-components` unless stated.

- **`CsvTable` hangs on a long file (existing).** 29.5 s for 8,000 rows in Chrome, from Q-313. Not fixed;
  the fix is `split(text, "")` once.
- **`.ui-sr` inside an unpositioned `overflow-x: auto` wrapper widens the page (existing pattern).**
  It is `position: absolute`, so it escapes. WorkloadView went from 360 to 459 px, and another reached
  1,413 px on a 1,280 window. The test harness measures nothing that catches this. The new components
  are fixed with `position: relative`. Measured on the gallery at 390 px, 12 existing components have
  it: `editable_table`, `kanban_board`, `org_chart`, `pricing_table`, `cross_rates`, `fx_positions`,
  `fx_rate_table`, `loan_committee_queue`, `payment_history`, `repayment_schedule`, `weather_strip` and
  `token_list`. Not fixed.
- **38 existing demos run past a 390 px screen, and the gallery page is 937 px wide there.** Among them
  are `bento_grid`, `columns`, `radio_cards` (871 px), `box_plot` and `flight_search`. Some of this is the
  demo and not the component, which has not been told apart. Tests at 1280 px cannot see it. Batch 04's
  unexplained 364 px page is probably the same thing. Not fixed.
- **The gallery's search label is tied to nothing (existing).** `<label.ui-sr field="g-q">` is sent as
  written, and `quillo check` says ok. The README's line "use `field` for a label's `for`" is about a
  view's prop, not a `<label>`, and reads as if it applied to both.
- **Two instances need a `name`.** PlanAdvisor, SizeAdvisor and CohortTable share a radio group on a page
  unless each is given one. Documented, and the demos do so.
- **Six existing tests fail every time:** `file_upload`, `inline_edit`, `spotlight_tour`,
  `editable_table`, `json_viewer` and `month_calendar`, on the unmodified kit and in all three runs here.
  They fail the same on 9e8c2fb with the kit at c856df3, so they predate this work. `file_upload`'s is a
  real message, `` `reduce` cannot call a function that waits on the server ``. `video_player` failed once
  under load and passed twice.
- **The gallery's demos cannot import.** The 577 existing components and the 100 new ones all take what
  they use with `import` lines, and with every `ui/` file loaded as one program none uses a name it did
  not take. `MapView` moved to a `client` line in this work. Only the demos are left, for Q-325's reason:
  629 warnings before, 730 now.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Daylight saving gaps and repeats, +05:45 offsets | Right the first time, the same on both sides |
| Decimals and money across 75 server and browser probes | One difference, a JSON error's wording |
| 20,000 decimal multiply-and-rounds in Chrome | 14 ms |
| A million interpreter steps of a `while` over maps in Chrome | about 30 ms, so an engine written in Quillo is practical |
| `re.find_all` as a tokenizer, emoji included | Positions in characters, fast at 200,000 characters |
| Unicode ranges and `\u{...}` in patterns | Agree on both sides |
| Keyed rows re-sorted or edited while typing | Focus and typed text kept, with no special care |
| `value` on a `<textarea>`, `<select>` with `selected`, a range's `@input` | As the guide says |
| `@input` reading a derived `let` right after assigning state | Gets the new value |
| `then`, `view` and `match` used as names, and `break` | A clear message each |
| Junk through every data prop of ten components | All answered 200, with no console errors on the four pages loaded |
| Resuming, across 90 components under `--watch` (the nine batches that checked) | None remade |
