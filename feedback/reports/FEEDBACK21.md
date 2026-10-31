# What a Quillo error tells you, and who it tells

| | |
|---|---|
| App | None: one question, how Quillo prints an error stack, answered with small programs |
| Quillo | 0.6.0 (c59890e), with the uncommitted work of branch `feedback20-fixes`: the build is newer than every file in `src/`, so `quillo --version` names a commit that has less in it than was tested |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo run` and `quillo check` on macOS, `curl`, 60 requests at once from Python, headless Chrome with and without `--watch`. Not run: `quillo dap` (what is said of it comes from the guide and `src/debug.rs`), `--split`, `--instances`, a channel handler, a panic in a `.qwer` plugin, or anything on Linux |

What worked well: the place an error gives is right. A failure in the middle of a call over
five lines names the line of the argument that failed, not the line of the call. A wrong
argument names the line of the call, not the function's first line. Recursion past the limit
ends with one clean line. Without `--watch`, a runtime error gives the visitor nothing about
the program, and the log keeps it all.

The answer to the question: **Quillo does not print a stack.** It prints one place, and how
much of that depends on where the error happened and who is reading:

| Where it failed | The log | The visitor |
|---|---|---|
| A script | `quillo: lib.qwe:2: division by zero`, exit 1 | |
| A route, runtime error | `quillo: error: lib.qwe:2: division by zero` | `internal error` |
| The same under `--watch` | the same | `internal error: lib.qwe:2: division by zero` |
| A `server fn` | ``quillo: error in server fn `sv`: price.qwe:2: division by zero`` | ``{"error":"internal error in `sv`"}`` |
| A service | ``quillo: service ledger: error in `spend`: ledger.qwe:1: division by zero`` | ``{"error":"service ledger: internal error in `spend`"}`` |
| A job | `quillo: error: price.qwe:2: division by zero`, and the job's name on the request line after it | |
| A route, `db.*` error value | `quillo: error: db: no such column: nope in select nope from t at offset 7` (no place) | `{"error":"internal error"}` |
| A route, any other error value | nothing but `GET /age -> 500` | the error's whole text (Q-171) |
| The browser | | in the console, the message alone (Q-173) |

Nothing here is fixed: no code was changed for this report, as asked. Fix first, in this order:
Q-171 (an information leak that needs no privileges, and the same hole Q-161 closed for
databases), then Q-174 with Q-175 (the two together are what makes an error in production
findable), Q-173, Q-172.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-161 | Fixed for what it names: a `db:` value is hidden and logged. `db::hidden` looks for database errors only, so the rest of the error values still go out. That part is new, as Q-171 | 1 |

## New issues

### Q-171. A route that gives an `http.*` or file error sends the visitor its text, and the log gets none of it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | looking at what Q-161's fix did not cover | 1 |

Q-161 again, for every error value that is not a database's. On a plain `quillo run`, not only
under `--watch`, a route whose last value is an error answers with that error's text. For an
`http.*` call that is the address it called, with its query string; for a file, the path. The
commonest way to reach this is not an attack: a service behind the program goes down, and every
visitor is shown the internal address and any key in it.

```quillo
route GET /billing = http.get("http://127.0.0.1:1/internal/billing?key=abc")
route GET /keys = read_file("/srv/app/secrets/keys.json")
```

| Route | The visitor gets |
|---|---|
| `/billing` | `{"error":"http: http://127.0.0.1:1/internal/billing?key=abc: Connection Failed: Connect error: Connection refused (os error 61)"}` |
| `/keys` | `{"error":"read_file: path must stay inside the program's folder: /srv/app/secrets/keys.json"}` |

The operator gets less than the visitor: the log has `GET /billing -> 500 (0.2ms)` and no text.
`?` does the same: a helper that passes on `int("x")` answers
`{"error":"not an int: \"x\""}` with nothing logged. A `server fn` goes through the same
`db::for_visitor` (read in `src/server.rs`, not run), so what it hands the browser is likely the
same.

**Suggestion:** a value from `err(...)` in the program stays the program's to show, as the guide
says. An error made by a builtin is treated as Q-161 treats a database's: `internal error` for
the visitor, outside `--watch`, and its text in the log. Both kinds get a log line, so no 500 is
ever without a reason. This needs a mark on the value, as the text alone cannot tell them apart.

### Q-172. Of two elements side by side, only the last reaches the page, and `quillo check` says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a browser test that could not find its button | 2 |

A body gives its last expression, so this follows from a documented rule. But markup is what
people write next to markup: in HTML both elements stand, and in JSX the same lines are an error.
Here the first one is gone without a word, with any handler on it. This cost a probe here: the button was not on
the page, `quillo check` said ok, and the driver only said it was null.

```quillo
component Pair() {
    <p>one</p>
    <p>two</p>
}
```

`<Pair/>` gives `<p>two</p>`. A `view` and a `fn` do the same, and `quillo check` passes all three.

**Suggestion:** `quillo check` warns on markup whose value goes nowhere, as a statement that
cannot do anything: "this element is not on the page: only the last one in a body is; wrap them
in one element". Numbers on lines of their own (`1` then `2`) pass as well, and the same warning
would cover them.

### Q-173. A runtime error in the browser prints its message alone

| Kind | Detected by | Attempts |
|---|---|---|
| loud | headless Chrome's console | 4 |

The same failure names its file and line on the server, and in the browser names neither:

```quillo
fn inner(a, b) = a / b
fn share(a) = inner(a, 0)

component Pay() {
    state n = 7
    <button id="go" @click={() => { n = share(n) }}>pay {n}</button>
}
```

| Where | What it prints |
|---|---|
| Server | `quillo: error: s.qwe:1: division by zero` |
| Browser, with and without `--watch` | `quillo: division by zero` |

`report` in `src/client/runtime.js` logs `e.message` and drops the rest. Under `--watch` this
throws away what is already there: a page's code comes as a file per function, with a source map
back to the `.qwe` lines, and a `.qwe` breakpoint works in it. The error's own stack should point
at those lines.

**Suggestion:** in development, `console.error("quillo:", e)`, so the browser prints the stack
through the source maps. That is one line, not yet tried here. In production, keep the message.
The compiled code could carry the file and line of a failing statement as the server's does,
which costs bytes in every bundle, so measure that first.

### Q-174. A runtime error names the line that failed, and not how it got there

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the question itself, then `src/error.rs` | 1 |

`QuilloError` holds a message, a line and a file. Nothing is added as it passes up through the
calls, so a helper with many callers gives the same line for all of them:

```quillo
import price.qwe use share

fn checkout(n) = share(100, n)
fn split_bill(n) = share(200, n)

print(checkout(2))
print(split_bill(0))
```

```
quillo: price.qwe:2: division by zero
```

Which caller, and which of a thousand calls, is left to the reader. An error value is worse off:
it has no place at all, so a `db.*` failure logs its SQL and neither file nor line, and `?` hands
it up without a mark. The guide's FAQ says exceptions do not exist, and does not say that errors
have no stack, which someone coming from any common language will look for first.

The interpreter does know the calls, but only for the debugger: `src/debug.rs` keeps frames while
`debug::on()` holds, and `src/interp.rs` skips them on an ordinary run.

**Suggestion:** a chain built on the way out, so a call that succeeds pays nothing. Each call the
error leaves adds its function's name and the line of the call, and a run that recursed 10,000
times keeps the first and last few:

```
quillo: price.qwe:2: division by zero
  in share, called at main.qwe:4
  in split_bill, called at main.qwe:7
```

It goes to the log, and to the page only under `--watch`. An error value gets the file and line
it was made at, which is a small cost even where `??` takes it straight away. Hot functions run
in `src/jit.rs`: the chain has to come out the same from there, and that was not looked at.

### Q-175. A server's error line does not say which request it belongs to

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | 60 requests at once | 2 |

The visitor gets an `x-request-id`, and `--log json` records it as `rid`, but the line with the
error has neither that id nor the route. So someone who reports an id cannot be matched to an
error:

```
quillo: error: main.qwe:8: division by zero
{"time":"...","method":"GET","path":"/checkout","status":500,"ms":0.08,"ip":"127.0.0.1","rid":"1cf61931ea88ba90"}
```

Under `--log json` that first line is plain text among objects, so a log collector cannot read it
by field. Reading the two lines as a pair works with one request at a time and not otherwise,
since errors are printed at once and request lines are buffered for each turn of the loop. With
30 requests to `/checkout` and 30 to `/split` at once, all failing in the same helper, 31 of the
60 error lines were followed by a request line at all, and the lines themselves were identical.

**Suggestion:** the error line carries the method, the path and the id:
``quillo: error: GET /checkout (rid 1cf6...): main.qwe:8: division by zero``. Under `--log json`
it is an object too, or better, an `error` field on the request's own line, which is then one
record with everything in it. The chain from Q-174 would go there too.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `fn b(n int) int { ... }` | Go's shape, from memory, not from the guide | ``expected `)`, found `int` `` | 1 run |
| `get "/" { ... }` for a route | a guess at the syntax | `expected end of statement, found a string`; the server had not started, which showed only in the log | 1 run |
| `route GET /x with http = ...` | the effects are written so on a `fn` | ``expected `=` or `{` to start the route body``, plain and right | 1 run |
| `xs.map(fn(x) { ... })` | JavaScript and Go habit | the message says what to write: `(x) => x * 2` | 1 run |
| `10 / 0` to get a runtime error | | `quillo check` found it first, so it never reached the run. A divisor from a variable did | 1 run |
| `n.missing` on a `state n = 7` | wanted a failing call | a check error, so no server started, and the probe read that as a missing button | 1 run |
| Two buttons side by side in a component | HTML habit | the first was gone (Q-172), which took two more runs to see, as the driver's error said only `null` | 2 runs |
| `page()`, `page.click("#go")`, `return` in `page.ev` | guessed at `feedback/tools/` without reading it: `page` is a tab, `click` takes x and y, `ev` takes an expression | a click that never landed, read at first as a browser that reports nothing | 3 runs |
| `wait %2 %3 %4` in a shell loop around a server | my own mistake, nothing to do with Quillo | waited on the server itself until the 120 s limit | 2 minutes |
| "Quillo has no stacks, and that is all" | answered the question from one program | the same probe on each kind of failure found Q-171, a leak, which the first answer had missed | 8 probes |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A failure inside a call written over five lines | names the argument's line, `n4.qwe:5` |
| Adding a map in a sum over several lines | names the line of the `+` |
| A wrong type through a function passed as a value | names the call's line, with what it got: ``argument `a` of `f` must be int, got str "x"`` |
| Recursion past 10,000 calls | ``too much recursion (more than 10000 nested calls) in `deep` `` |
| An error at the top level of a used file | names that file (as `./used.qwe:1`, where a function's error says `lib.qwe:2`, a small difference only) |
| A runtime error in a route, without `--watch` | `internal error`, and no file or line reaches the visitor |
| An error in a service | the log names the service, the function, the file and the line; the visitor gets the first two |
| An error in a `server fn` | the same: the log has the place, and the visitor has the name |
| A failing job | answered 500 and logged, and the next due run goes ahead |
