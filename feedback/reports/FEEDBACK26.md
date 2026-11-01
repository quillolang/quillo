# Feedback from writing 100 more plugins in Quillo

| | |
|---|---|
| App | None: 100 new `.qwer` plugins with their examples, in `~/dev/quillo-website/plugins`, written in four batches of 25 |
| Quillo | 0.6.0 (5f7f76c, 2026-11-01), a pinned copy of `target/release/quillo`, so the work on `feedback24-fixes` is not in it |
| Model | Claude (Opus 5.5) |
| Tested with | `plugins/test.mjs` for each plugin, against stand-ins, and for `sftp` and `ssh_exec` this Mac's own OpenSSH. The issues below were each reproduced by hand on the version above, with `curl`, in a program of their own |

All four batches have finished, and each passed all 25 of its plugins. This report is complete: the
100 plugins after these report in [FEEDBACK27.md](FEEDBACK27.md).

Writing this report changed no Quillo source. The fixes are on `feedback26-fixes`, in another
session, and this report says nothing about their state. The repros here are inline, and all were
run as printed on the version above. Fix Q-197 first: it can send passwords to visitors.

## Seen again

None so far.

## New issues

### Q-197. A hidden error's text reaches a visitor through a template string or `str()`

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | plugin example, then `curl` | 3 |

An error made by a builtin or a plugin is hidden: the guide says it reads `internal error`
"wherever it would reach a visitor: as a route's value, inside a map or list, in markup, or from a
`server fn`". That holds while the error stays a value. Once it is turned into a string, the mark
is gone and the whole text goes out, with a 200, and `quillo check` says ok.

```quillo
// hid.qwer
//! Fails on purpose.
//! version: 1.0.0

/// Fails with a secret in the text.
pub fn hid_fail() -> Result<i64, String> {
    Err("postgres://admin:hunter2@db.internal/app refused".to_string())
}
```

```quillo
// hid.qwe
use "hid.qwer"

fn r(path: str) = read_file(path)

route GET /json {
    let x = hid_fail()
    { x: x }
}

route GET /text {
    let x = hid_fail()
    `value: ${x}`
}

route GET /str {
    let x = hid_fail()
    { line: str(x) }
}

route GET /markup {
    let x = hid_fail()
    <p>{`${x}`}</p>
}

route GET /builtin {
    let x = r("/nope/secret-path.txt")
    `value: ${x}`
}
```

| Route | Answer (200 each) | |
|---|---|---|
| `/json` | `{"x":{"error":"internal error"}}` | right |
| `/text` | `value: err("postgres://admin:hunter2@db.internal/app refused")` | **leak** |
| `/str` | `{"line":"err(\"postgres://admin:hunter2@db.internal/app refused\")"}` | **leak** |
| `/markup` | `<p>err(&quot;postgres://admin:hunter2@db.internal/app refused&quot;)</p>` | **leak** |
| `/builtin` | `value: err("read_file: path must stay inside the program's folder: /nope/secret-path.txt")` | **leak** |

`<p>{x}</p>` is right, and shows `err("internal error")`, so the same value is safe in markup and
not in a template string inside that markup. This is the case the mark was made for: `Value::err`
hides by default so that a new builtin error cannot leak, and `repr_into` in `src/value.rs` prints
the text of `Value::Err` without looking at the mark.

A program that writes `` `Saved: ${result}` `` for a `result` it did not check is an ordinary
program. It looks fine until the database fails, and then the visitor gets what the log was meant
to keep.

`x.error` also gives the text, and a program can send that on: `{ line: x.error }` answers with
the same address. The guide says code on the server reads an error, naming `error(r)`, so that is
a choice the program made, not this bug. It is listed because the guide does not say that what
was read is no longer protected.

**Suggestion:** when an error is turned into a string, write `err("internal error")` if it is not
the program's own, as `for_visitor` does. Code on the server that wants the text has `error(r)`.
This was not tried. Check what it does to the log, `print` and `--watch`, which should keep the
text.

### Q-198. `?` in a lambda leaves the lambda, and the route answers 200 with errors in its list

| Kind | Detected by | Attempts |
|---|---|---|
| silent | plugin example, then `curl` | 2 |

A model writing `map(xs, (i) => f(i)?)` in a route means "stop at the first failure". What happens
is that `?` returns from the lambda, as the guide's "the current function" allows. The list holds
the errors, the route goes on, and it answers 200.

```quillo
// with hid.qwer from Q-197
route GET /lambda {
    let g = map([1, 2], (i) => hid_fail()?)
    { g: g, after: "ran" }
}

route GET /plain {
    let v = hid_fail()?
    { v: v, after: "ran" }
}
```

| Route | Answer | |
|---|---|---|
| `/plain` | 500 `{"error":"internal error"}` | stopped, as meant |
| `/lambda` | 200 `{"g":[{"error":"internal error"},{"error":"internal error"}],"after":"ran"}` | went on |

The `?` does nothing there: `(i) => hid_fail()` gives the same answer. The text stays hidden, so
this is not Q-197. The harm is a list holding errors, passed along to whatever the route does
next, and a 200 for a request that failed.

`filter` is worse, as an error from the predicate counts as false and the rows simply go:

```quillo
// with hid.qwer from Q-197
route GET /filter {
    let g = filter([1, 2], (i) => hid_fail()? > 0)
    { g: g, after: "ran" }
}

route GET /reduce {
    let g = reduce([1, 2], 0, (a, i) => hid_fail()?)
    { g: g, after: "ran" }
}
```

| Route | Answer (200 each) |
|---|---|
| `/filter` | `{"g":[],"after":"ran"}`: an empty list, with no sign that anything failed |
| `/reduce` | `{"g":{"error":"internal error"},"after":"ran"}`: the error as the result |

A failed permission check in a `filter` looks like a user with nothing to see. `quillo check` says
ok on all of these, and the guide says nothing on `?` in a lambda.

Two of the four batches wrote this without being told of the other, the second as
`[1, 2] | map((n) => json.parse("{")?)`: the shape a model reaches for.

**Suggestion:** have `filter` treat an error from its predicate as an error, not as false. Have
`quillo check` warn on a `?` that ends a lambda, since it cannot change the value, and suggest a
`for` loop. Say so in the guide beside `?`.

### Q-200. A repeated query parameter or form field keeps only its last value, and asking for a list refuses every request

| Kind | Detected by | Attempts |
|---|---|---|
| silent | plugin example, then `curl` | 3 |

A group of checkboxes with one name sends `tag=a&tag=b`, as does a filter link like
`?tag=a&tag=b`. Quillo keeps the last value and drops the others without a word. The obvious fix,
a list in the route's pattern, passes `quillo check` and then matches nothing at all.

```quillo
route GET /x {
    { query: request.query }
}

route GET /many ? { a: [str] } {
    { query: request.query }
}

route POST /form {
    { form: request.form, body: request.body }
}

route POST /list { form: { tag: [str] } } {
    { form: request.form }
}
```

| Request | Answer |
|---|---|
| `GET /x?a=1&a=2` | 200 `{"query":{"a":"2"}}`: `1` is gone |
| `GET /many?a=1&a=2` | 400 `bad request: no route pattern matches this request` |
| `GET /many?a=1` | 400, the same |
| `POST /form`, `tag=a&tag=b` | 200 `{"form":{"tag":"b"},"body":"tag=a&tag=b"}` |
| `POST /list`, `tag=a&tag=b` | 400, the same |

A form can be got back by parsing `request.body` by hand. A query cannot: the fields of `request`
are `method path query params headers cookies body json form files ip id`, and none holds the
query string, so the first value of `?a=1&a=2` is out of reach. A plugin that must check a signed
URL, or a form with a group of checkboxes, cannot see what was sent. The guide does say this
happens (`query` and `form` keep the last of a key given twice: no lists), so the loss is
documented, though silent at run time. It says nothing of `[str]` in a pattern.

**Suggestion:** have `[str]` in a pattern take all the values of a name, so `? { a: [str] }` gives
`["1", "2"]`, and one value a list of one. Until then have `quillo check` refuse `[str]` there,
since it can never match. Add `request.query_string`, the query as it came.

### Q-199. `.error` on a value that did not fail is null for a map and a 500 for a str or int

| Kind | Detected by | Attempts |
|---|---|---|
| silent | plugin example, then `curl` | 3 |

Asking a value whether it failed with `.error` gives three answers, by its type. When `quillo
check` knows the type it refuses all of these, which is right. When the value comes from
`json.parse`, a plugin or a service, it says ok, and this happens at run time:

```quillo
fn pick(k: str) = json.parse(k)

route GET /map {
    let r = pick("{\"k\":1}")
    { v: r.error }
}

route GET /text {
    let s = pick("\"text\"")
    { v: s.error }
}

route GET /whole {
    let n = pick("5")
    { v: n.error }
}
```

| Route | Answer |
|---|---|
| `/map` | 200 `{"v":null}` |
| `/text` | 500 `internal error`; the log says `str has no field error` |
| `/whole` | 500 `internal error`; the log says `int has no field error` |

The 500 on a value that is not an error was known (it is in `plugins/README.md`'s list, never
filed). What is new is that a map takes the other path. A plugin call giving a map on success
answers null, so `if r.error { ... }` works, and the same line 500s when the same call gives a
string. The test that passed says nothing about the next plugin.

An error is the same kind of trap the other way round: `.error` reads it, and any other field is a
500, `error has no field path`, which `quillo check` passes when the value comes from a function
using `?`. So `r.path` on a call that can fail stops the route with the real reason only in the
log, and a program has to ask `is_err(r)` before each read.

**Suggestion:** `is_err(r)` and `error(r)` are the way to ask, and work on any value. Say so where
the guide shows `.error`, and have `quillo check` suggest them on `.error` when the type is not
known.

### Q-201. Two refusals do not point at what works

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 each |

Both are stopped before the program runs, so nothing goes wrong, but neither message helps.

```quillo
let k = "a"
let m = { [k]: 1 }
```

`expected end of statement, found :`, with no sign that a map's key cannot be computed. The way is
`set({}, k, 1)`.

```quillo
route GET /s {
    session.x = 1
    { ok: true }
}
```

`cannot set x: only the page's objects (an element, an event) have fields that are set, in a
component`. This is in a route, and nothing here is about a page. The way is `set_session`.

**Suggestion:** hint at `set(m, k, v)` for `[` as a map's key, as `use null instead of nil` does
for its case, and at `set_session` when the name being set is `session`.

### Q-202. A program cannot take arguments from the command line

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | plugin example, then `quillo run` | 3 |

Quillo runs scripts as well as servers, but a script cannot be given anything to work on. Every
word after the file is taken as an option of `quillo run`, and no builtin reads the rest.

```quillo
// t.qwe
print(args())
```

| Command | Answer |
|---|---|
| `quillo check t.qwe` | ``unknown name `args` (did you mean `abs`?)`` |
| `quillo run t.qwe -- a b` | ``unknown option `--` `` |
| `quillo run t.qwe a b` | ``unknown option `a` `` |
| `quillo t.qwe a b` | ``unknown option `a` `` |

The `cli_args` plugin, for flags and subcommands, has nothing to read, so it falls back on a
variable: `CLI_ARGS="--port 80" quillo run tool.qwe`. A value with a space in it cannot be passed
that way without a quoting rule of its own.

**Suggestion:** take what follows `--` as the program's, and give it to the program as a list:
`args()`, or `cli.args`. `--` is what the other runners use, and an option of `quillo run` keeps
working before it.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Q-197's repro with `err("negative")` | Taken from the batch's report | The text is shown, which is right, as the program wrote it: no bug. It needs an error from a builtin or a plugin | 1 run |
| `let x = f()  { x: x }` on one line, from the batch's one-line repro | Short | `expected end of statement, found {`: statements need a line of their own | 1 check |
| `let view = 1` | An ordinary name for a page's value | `view` is a keyword, so it cannot be a name here. The guide lists it, and the message is clear: not an issue | 1 check |
| `let nil = f()` | An ordinary name for an empty value | `use null instead of nil`. The guide lists `nil` as refused with a hint: not an issue | 1 check |
| Q-199's repro with `{ k: 1 }` returned by a `fn` | Taken from the batch's report | `quillo check` knows the type and refuses it: right, and no bug. It needs a value of unknown type | 1 check |

## Limitations

- **`write_file` stays inside the program's folder.** The guide says so, and `write_file("/tmp/x",
  "y")` is refused. An example cannot make a temporary file for a plugin that takes local paths,
  so the examples write to a git-ignored `data/` beside them.
- **A plugin runs no Quillo code.** The `kubernetes` plugin refuses a kubeconfig whose user is an
  `exec` command, and reports it rather than running a program it did not choose.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A hidden error as a route's value, in a map, or in `<p>{x}</p>` | `internal error`, with the text in the log and a request id |
| The program's own `err("negative")` in a template string or `str()` | Shown, as the guide says |
| `let v = hid_fail()?` in a route | 500 `internal error`, the rest of the route not run |
