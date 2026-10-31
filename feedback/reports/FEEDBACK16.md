# Feedback from taking Quillo's Rust to 99% test coverage

| | |
|---|---|
| App | No app: new tests for every part of `src/`, written to reach the code no test reached. 81 new test files under `tests/`, 1,037 tests |
| Quillo | 0.6.0 (b28e689), with the uncommitted work of branch `library-hints` in the tree |
| Model | Claude (Opus 5.5), one session and nine agents, seven for the parts of the source and two for what was left |
| Tested with | `cargo llvm-cov` over `cargo test --release --all-features`, a real window for `quillo app`, headless Chrome, throwaway PostgreSQL and MariaDB servers, the repros under `feedback/run.mjs` |

What worked well: nearly everything a test reached did what the guide says. The 1,037 new tests turned up
43 issues, most of them at the far edge of a type's range or on a path nobody takes on a good
day. The standard library's error values held up well, `date.*` above all: 600 untested lines,
now tested, and one wrong answer in them, for the last two days of the year 9999. The parser's
messages were right almost everywhere they were asked to be.

The findings that matter most are the first five, and Q-143 and Q-144. Each gives a wrong answer
or a false success without a word.

## Seen again

None. INDEX.md was read first, and no finding here is a known issue. The connection reset on a
server's first request (in `flaky-tests-under-load`, not an INDEX row) showed in the new tests too.

## New issues

Each has a repro in [repros/](../repros), or, where it needs a database server, a window or a
private function, an ignored test that fails until it is fixed. Those tests carry
`#[ignore = "finding: ..."]`. To run them all: `cargo test --release --all-features --tests -- --ignored`.

### Q-108. A plain `db.transaction` whose commit fails stays open

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

When the commit of a `db.transaction` with no options fails (SQLite, a deferred foreign key), the
error comes back, but the transaction is still open: its row is still seen, and whatever runs
next runs inside it. The same call with options rolls back, so only the plain form has this.

```quillo
db.open(":memory:")
db.exec("create table p (id integer primary key)")
db.exec("create table k (p int references p (id) deferrable initially deferred)")

let r = db.transaction(() => db.exec("insert into k values (9)"))
print(r)                                    // the foreign key error, rightly
print(len(db.query("select * from k")))     // 1, where it should be 0
```

**Suggestion:** when `release quillo_tx` fails on the outermost transaction, roll it back too (src/db.rs).

### Q-109. A `return` at the top level of a used file ends the program that used it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

The program stops at the `use`, with no message and a clean exit. Everything after it, routes
included, is never declared.

```quillo
// early.qwe
print("in early")
return

// main.qwe
use early.qwe
print("after the use")      // never printed
```

**Suggestion:** let `use` catch the file's `return`, or refuse a top-level `return` in a used file.

### Q-110. PostgreSQL: a float too large for bigint is stored as the largest bigint

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

Binding `1e30` to a `bigint` column stores 9223372036854775807: `*f as i64` saturates, so a wrong
number reaches the table. Other values that don't fit are refused with "cannot send float".
Test: `db_postgres::a_float_too_large_for_a_bigint_is_refused`.

**Suggestion:** check the range before the cast (src/pg.rs), and refuse as for the others.

### Q-111. `int(1.0e30)` is silently 9223372036854775807

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`int(-1.0e30)` likewise gives the smallest int. A dec that large is an error value, and in the
browser `Math.trunc` gives 1e30, so server and browser disagree too.

```quillo
print(int(1.0e30))
```

**Suggestion:** a range check in `to_int`, giving the same error value as a dec.

### Q-112. A private name beside a typed field: the route reads the variable, not the request

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`{ q: str }` binds the key `q`, but the body's `q` is renamed to the private name, so it reads the
file's variable. `/q?q=hi` answers with the variable's value, which is also how a private value
could reach a client.

```quillo
private let q = "the variable"

route GET /q ? { q: str } = q
```

**Suggestion:** have `Renamer::pat` rename a field's implicit binding along with its uses.

### Q-113. `math.fsum` gives 0.0 when an exact sum overflows

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`math.fsum([1e308, 1e308])` and `fsum([1e308, 1e308, -1e308])` both give 0.0: an infinite partial
cancels against its own rounding error. `fsum([inf, -inf])` gives NaN. Python raises for all
three, and the guide says what Python raises is an error here too.

```quillo
print(math.fsum([1e308, 1e308]))
```

**Suggestion:** stop when a partial turns infinite, and report the overflow.

### Q-114. `stats.mean([1e308, 1e308])` gives inf

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

Its running sum overflows. Python gives 1e308.

```quillo
print(stats.mean([1e308, 1e308]))
```

**Suggestion:** a running mean, or `fsum` once Q-113 is fixed.

### Q-115. `math.hypot`, `math.dist` and `vec.norm` give 0.0 for all-NaN input

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`f64::max` skips NaN, so the `is_nan` check in `norm2` never holds.

```quillo
print(math.hypot(math.nan, math.nan))
print(math.dist([math.nan], [0]))
print(vec.norm([math.nan]))
```

**Suggestion:** check for NaN while taking the maximum, not after it.

### Q-116. In the last two days of 9999 a named zone's offset is taken as 0

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

Past the range jiff can convert, the offset quietly falls back to 0, so the time is written as UTC.

```quillo
print(date.to_zone("9999-12-31T12:00:00Z", "America/New_York"))   // ...12:00:00Z
```

**Suggestion:** an error value when the zone's offset cannot be found.

### Q-117. PostgreSQL: `inet`, `macaddr` and others come back as raw bytes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

Types with a binary form Quillo has no reader for reach the program as bytes, not text:
`select '192.168.0.1'::inet` gives junk. `macaddr` and `money` take the same path; `money` was
read in the code, not run. Test: `db_postgres::an_address_comes_back_as_its_text`.

**Suggestion:** read those types as text, or say that the type is not supported.

### Q-118. `{true}` is text on the server and nothing in the browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

A boolean child shows on a page the server made, and disappears once the browser renders it.

```quillo
view Flag(on) = <span #flag>{on}</span>

component Page() {
    state shown = false
    <div>
        <button #show @click={() => { shown = true }}>show</button>
        {shown && <Flag on={true}/>}
    </div>
}
```

**Suggestion:** make `put` in runtime.js write what the server writes.

### Q-119. A `__proto__` key is lost on its way to the browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

A constant map with plain values goes out as a JSON literal, and in a literal `__proto__` sets the
prototype. The key is gone, and `{ "__proto__": { admin: true } }` gives every lookup of
`m.admin` the value true in the browser, which is how prototype confusion starts.

```quillo
let m = { "__proto__": 1 }
```

**Suggestion:** write the key as `["__proto__"]`, or send such a map through `JSON.parse`.

### Q-120. `1e999` is inf on the server and null in the browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

The literal overflows to inf, and JSON has no inf, so the compiler writes null.

```quillo
component Page() {
    state v = "wait"
    <button @click={() => { v = `${1e999}` }}>{v}</button>
}
```

**Suggestion:** refuse the literal at load, as `int` refuses one too large, or write `Infinity`.

### Q-121. A map that only changed its keys' order is patched as unchanged

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`diff({a:1,b:2}, {b:2,a:1})` says nothing changed, because `old == new` ignores order, though a
page that loops over the map shows that order. Seen at the function; not driven in a browser.
Test: `server_parts::a_map_in_another_order_goes_whole`.

**Suggestion:** compare the order of keys in `diff`.

### Q-122. `db.error_kind` gives null for two MariaDB errors

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

A failed check (`CONSTRAINT ... failed`, 4025) and a snapshot conflict (1020) both give null. A
transaction already retries on 1020, so `kind_of` is behind `txn`. Tests:
`db_mariadb::a_failed_check_on_mariadb_is_a_check`, `db_mariadb::a_row_changed_since_the_snapshot_is_a_conflict`.

**Suggestion:** add both codes to `kind_of`.

### Q-123. `--with` commands outlive a program that does not start

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

The guide says a `--with` command stops with the program. When the program fails to load, or ends
with nothing to serve, `run` returns without `with::stop_all` and the command stays. By reading,
the same when it cannot listen.

```sh
quillo run bad.qwe --with 'exec sleep 30'   # quillo exits; sleep goes on
```

**Suggestion:** stop them on every path out of `run`.

### Q-124. `quillo app` reads no `.env`, and takes no notice of `--env-file` or `--with`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`run` has all three, and the usage says `app` takes `run`'s options. A program that works under
`run` finds `env("GREETING")` empty in a window. Test: `cli_app::with_windows::the_env_file_beside_the_program_is_read_as_for_run`.

**Suggestion:** share `run`'s start-up with `app`.

### Q-125. `quillo check` drops what it does not know

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`quillo check ok.qwe bad.qwe` checks only the first and says ok. `quillo check ok.qwe --strict-nul`
says ok too, so a misspelt option turns a check off without a word. `docs` and `build` do the same.

```sh
quillo check ok.qwe bad.qwe
```

**Suggestion:** refuse an unknown option, as `run` does, and check every file named or refuse the extra ones.

### Q-126. A service called from a helper is not seen as the http effect

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` | 1 |

`called_names` drops the service's name, so `with db` on the caller passes.

```quillo
service billing = "billing.qwe"

fn load() = billing.invoice()
fn f() with db = load()
```

**Suggestion:** keep the service's name in `called_names`. That also makes check.rs 2610-2611 reachable.

### Q-127. Under `--no-check`, a pipe into a server fn gets past the browser compiler

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 2 |

`quillo check` and a plain `quillo run` refuse `{1 | save}`, so this needs `--no-check`. There the
compiler refuses `{save(1)}` but passes the piped form, and the page gets a Promise.

```quillo
server fn save(x) = x

component C() = <p>{1 | save}</p>
```

**Suggestion:** treat a pipe's target as a call in the compiler's check.

### Q-128. The strings of `class={if on { "a" } else { "b" }}` are not read

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` | 1 |

A misspelt class passes here, and is caught in `class={on ? "ok" : "okk"}`.

```quillo
css(".ok { color: red }")

fn a(on) = <p class={if on { "ok" } else { "okk" }}>1</p>
```

**Suggestion:** read the blocks' last expressions as the ternary's branches are read.

### Q-129. `platform().locale` is null under an empty `LC_ALL`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

With `LC_ALL=` and `LANG=en_US.UTF-8` it gives null, where an empty `LC_ALL` counts as unset everywhere else.

```sh
LC_ALL= LANG=en_US.UTF-8 quillo run locale.qwe
```

**Suggestion:** skip empty values, in `LC_ALL`, `LC_MESSAGES` and `LANG` alike.

### Q-130. A cache duration too long to count is not refused

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`n * per` overflows, so `file("a.txt", { cache: "999999999999y" })` loads with a wrong duration.

```quillo
let f = file("a.txt", { cache: "999999999999y" })
```

**Suggestion:** `checked_mul`, and the message a bad duration already gets.

### Q-131. `quillo install --list` never prints its `Other` heading

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

The group starts as "", so plugins with no group run on under whatever came before, with no
heading. Test: `cli_install::plugins_with_no_group_are_listed_under_other`.

**Suggestion:** start with no group at all, an `Option`.

### Q-132. A ping on a channel's WebSocket gets no pong

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`over_websocket` drops it; RFC 6455 §5.5.2 requires a pong. Browsers don't send pings from a page,
so this reaches other clients and proxies. Test: `wt_edges::a_ping_on_a_channels_websocket_is_answered`.

**Suggestion:** answer opcode 9 with its payload.

### Q-133. A long chain of `reply` crashes the parser

| Kind | Detected by | Attempts |
|---|---|---|
| loud | test | 1 |

30,000 of `reply ` before a number escape the nesting limit, so `quillo check` dies on the stack
though 600 of them are refused rightly, with "the code nests too deeply": the check runs out of stack before it gets to say so.

```sh
{ printf 'fn f() = '; yes 'reply' | head -30000 | tr '\n' ' '; echo 200; } > chain.qwe
quillo check chain.qwe
```

**Suggestion:** count `reply` in the parser's nesting.

### Q-134. Outside a request, a transaction waiting for a connection holds the interpreter lock

| Kind | Detected by | Attempts |
|---|---|---|
| perf | test | 1 |

`checkout_for_tx` sleeps with the lock, so the one holding the connection cannot finish. With
`pool_size=1`, `parallel` of three transactions stalls 30 s, then fails. Test:
`db_postgres::transactions_wait_their_turn_outside_the_server_too`.

**Suggestion:** sleep through `it.unlocked` (src/db.rs:569).

### Q-135. Under sustained computing, a reload is held off

| Kind | Detected by | Attempts |
|---|---|---|
| perf | test | 1 |

With three clients looping on a computing route, a `--reload` was held off past 15 s in some runs,
and took 5.6 s in one. httpd.rs 556-557 does `gil.release(); gil.acquire()`, so the loop takes
the lock back before the waiter wakes. The cause is from reading the code. Test:
`http_edges::a_reload_gets_its_turn_on_a_busy_server`.

**Suggestion:** hand the lock over fairly at that point.

### Q-136. `type Ids = list<int>` at the end of a line does not parse

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

The newline after `>` is taken as a continuation: "expected end of statement, found `let`".

```quillo
type Ids = list<int>
let x = 1
```

**Suggestion:** end the type at the newline once its brackets close.

### Q-137. A local function called as a method is warned about as unused

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

```quillo
fn f() {
    let adder = (x) => x + 10
    1.adder()
}
print(f())
```

**Suggestion:** count a method call's name as a use of a local of that name.

### Q-138. A bare `return` that breaks the return type is reported a line early

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

```quillo
fn f() -> int {
    let x = 1
    print(x)
    return          // reported on the line above
}
```

**Suggestion:** give the statement its own line, not the previous token's.

### Q-139. `quillo app.qwe --log=off` is refused

| Kind | Detected by | Attempts |
|---|---|---|
| loud | test | 1 |

`quillo <file.qwe>` skips the handling `run` has, though the usage says they are the same.
`quillo run app.qwe --log=off` works.

**Suggestion:** send a bare file through the same parsing as `run`.

### Q-140. An option without its value is called unknown, and offered as its own correction

| Kind | Detected by | Attempts |
|---|---|---|
| loud | test | 1 |

```sh
$ quillo run a.qwe --port
quillo: unknown option `--port` (did you mean `--port`?)
```

**Suggestion:** say that `--port` needs a value.

### Q-141. `date.add_days` and `date.add_months` panic on the smallest int in a debug build

| Kind | Detected by | Attempts |
|---|---|---|
| loud | test | 1 |

`n.abs()` overflows. A release build gives the right error, "the date is out of range", so only a
debug build shows it. Test: `dates::the_smallest_int_is_out_of_range`, which passes on a release build.

**Suggestion:** `unsigned_abs`.

### Q-142. A plugin function with an array parameter is neither exported nor reported

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

The `;` of `[u8; 4]` ends the signature, so `pub fn f(x: [u8; 4]) -> i64 { 1 }` gives no export
and no problem: the function just isn't there. Test: `native_edges::an_array_parameter_is_exported_or_said_not_to_be`.

**Suggestion:** track brackets when finding the signature's end in `native::parse`.

### Q-143. A migration that fails at its commit stays open

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

A deferred foreign key that fails at commit leaves the file neither applied nor rolled back: its
tables stay, and it cannot be run again on that connection. The sibling of Q-108, in `migrate`.
Test: `migrate_failures::a_file_that_fails_at_its_commit_is_rolled_back`.

**Suggestion:** roll back when the commit fails.

### Q-144. `image.convert` reports success for an image cut off by a full disk

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`drop(w)` swallows the error of the last flush, so a small image on a full disk is written short
and the program is told it worked. Shown with `ulimit -f 1`. Test:
`image_failures::an_image_cut_off_by_a_full_disk_is_not_a_success`.

**Suggestion:** flush explicitly, and report what it says, before the rename.

### Q-145. `image.convert` leaves its temporary file when the rename fails

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

With a non-empty folder at the destination, the error comes back and `out.tmp<pid>` stays beside it.
Test: `image_failures::a_finished_image_that_has_nowhere_to_go_leaves_nothing_behind`.

**Suggestion:** remove the temporary file when the rename fails.

### Q-146. `(-9223372036854775807 - 1) % -1` is an overflow error

| Kind | Detected by | Attempts |
|---|---|---|
| loud | test | 1 |

The answer is 0, and only `checked_rem` makes this an error.

```quillo
print((-9223372036854775807 - 1) % -1)
```

**Suggestion:** treat a divisor of -1 as giving 0.

### Q-147. `math.lcm` with a zero depends on the order

| Kind | Detected by | Attempts |
|---|---|---|
| loud | test | 1 |

`math.lcm(0, MIN)` is 0, and `math.lcm(MIN, 0)` overflows.

```quillo
print(math.lcm(-9223372036854775807 - 1, 0))
```

**Suggestion:** answer 0 as soon as any argument is 0, before taking an absolute value.

### Q-148. A breakpoint inside a block in markup is verified but never stops

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

The debugger accepts a breakpoint on `print(x)` in `{for x in xs { ... }}`, marks it verified, and
runs past it. Test: `debug_corners::a_breakpoint_inside_a_block_in_markup_stops_there`.

**Suggestion:** either stop there, or don't verify it.

### Q-149. Under `--watch`, a file served before the watcher has started is never watched

| Kind | Detected by | Attempts |
|---|---|---|
| silent | test | 1 |

`WATCH_SERVED` is set on the watcher's thread, so a file served earlier is not recorded, and a
change to it does not refresh the page. It takes a slow start to see this, 20,000 `use`d files in
the test that found it, which is why a test of it failed only under load. Not reproduced outside that test.

**Suggestion:** set the flag before the server takes its first request (src/server.rs:1681).

### Q-160. A PostgreSQL connection given back as it dies goes back in the pool

| Kind | Detected by | Attempts |
|---|---|---|
| silent | CI, on Linux | 1 |

`give_back` keeps a connection unless `client.is_closed()` is true, and that is set a moment after
the query fails. So after the server ends a connection, the error can reach the caller first, the
dead connection is kept, and the next caller gets "Connection reset by peer" on a query that should
have had a fresh connection. On macOS the timing hid it; Ubuntu showed it on the first CI run.
Test: `db_postgres::a_connection_given_back_as_it_dies_is_not_handed_out_again`, which passed 8 of 8
on macOS: the failure is from Ubuntu, and the cause is from reading the code.

**Suggestion:** have `failed` note a connection the server ended (SQLSTATE 57P01, or any error with
no code), and let `give_back` drop it without waiting for `is_closed`.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `cargo llvm-cov` as it comes | the usual way | the tests stop servers with SIGKILL, so a server never writes its counts; caught by reading tests/support before running it | 1 read |
| `%c` in the profile path, with no more | it is what clang's docs say | "Counters section not page-aligned": 0.00%. rustc doesn't add the linker flags clang adds | 2 runs |
| Page-aligned the profile sections only | what clang does | the last page of counters is mapped from a file, which zeroed the start of `__data`: 8 test binaries crashed reading address 0, and a `urls` test failed for no real reason | 1 run, a crash log |
| Trusted a report after rebuilding with new flags | the numbers looked plausible | old binaries stayed in `release/deps` and were counted: parser.rs at 35%, really 92.5%. Noticed only because 35% couldn't be right | 1 full run |
| Took "each ignored test fails, so each finding is real" as proof | a failing test looks like one | 41 of 42 held, but Q-127's test skipped the check step, so the bug as first written does not happen in normal use; it is narrowed here to `--no-check` | caught by rerunning each finding as a program |
| `<script src="/first.js">` for `public/first.js` | other frameworks serve `public/` at the root | 404; Quillo needs a route, such as `route GET /assets/*path = file("public/" + path)` | 1 run |
| `file("a.txt", { cache: ... })` as a route's body in a probe | guessed the shape | a parse error; the agent's test had it right | 1 run |
| `Child::kill()` to stop a server under `--split` | as tests elsewhere stop plain servers | 23 services left running, found by counting processes; tests/services.rs had the answer, SIGINT first | 1 fix in 2 files |
| Took the total from `cargo llvm-cov` and the agents' counts as the same thing | both say "missed lines" | the agents counted each source line once, and the total counts each closure on its own: db.rs was 17 missed to them and 79 to the total. The first reads 99.1% well before the second reaches 99%, and hides every unrun `.map_err(\|e\| ...)` | 1 round of two more agents |
| Treated 201 unrun closures as missing tests | they were listed as code that never ran | all were `\|e\| e.into_inner()` on a lock, run only if another thread panics holding it. Written as `std::sync::PoisonError::into_inner` (199 lines, 29 files) there is nothing to run, and nothing changes | 1 read of the list |
| Called `Address already in use` flakiness and retried | the repo has a known first-request flake | a port is found free, then taken before the server binds it: a real race in the new tests' helpers, fixed by asking for another port | 3 full runs |
| Ran the new tests only on macOS | that is where the work was | Ubuntu failed a test on the first push, and the failure was a real race macOS timing hid (Q-160): the test was right | 1 CI run |
| "desktop.rs can reach 43.6% at most" | an agent took "needs a window" as the end of it | a real window, a stub `open` on the PATH and a page reporting back reached 92% | 1 test, gated by QUILLO_TEST_WINDOW |

## Limitations

What 99% does not cover: `fuzz/` (7 targets) and `mobile/quillo-mobile` have no tests; branch coverage needs nightly Rust;
Linux and Windows paths are not compiled on macOS, and platform.rs's 24 missed lines are those. The rest
of the 375 are OS failures that can't be provoked, races, and code with no caller.

- `--open '/x#y'` looks wrong by reading: desktop.rs:131 appends the launch token after the
  fragment, so the server would never get it. Not run.
- "did you mean" varies between runs on ties: `host` gives `has` or `last`.
- `math.degrees(1e308)` and `math.lgamma(-inf)` are errors here, and inf in Python.
  `stats.histogram([math.inf, 1], 2)` gives NaN edges.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `date.add_days` past either end of the calendar | "the date is out of range (years 1 to 9999)", in a release build even for the smallest int |
| `{save(1)}` and `{1 \| save}` in markup | refused by `quillo check` and by `quillo run` |
| `--as-service` on SIGTERM and SIGINT | stops at once |
| Malformed HTTP: bad chunk sizes, lengths that overflow, raw resets mid-answer | refused or dropped; the server goes on |
| Date formats and parsing, zones, month ends, leap years | as documented, across 600 lines of new coverage |
| A web view's new window and a navigation to another site | go to the system's browser; the program's own stay in the window |
