# Feedback from writing a second 100 plugins in Quillo

| | |
|---|---|
| App | None: another 100 `.qwer` plugins with their examples, in `~/dev/quillo-website/plugins`, written in four batches of 25 |
| Quillo | 0.6.0 (5f7f76c, 2026-11-01), a pinned copy of `target/release/quillo`, so neither `feedback24-fixes` nor `feedback26-fixes` is in it |
| Model | Claude (Opus 5.5) |
| Tested with | `plugins/test.mjs` for each plugin, against stand-ins and made-up samples, with real tools as a check where there were any (`git check-ignore`, `nginx -t`, `openssl`, `ssh-keygen`, `xmllint`, ffprobe). The issues below were each reproduced by hand on the version above, with `curl`, in a program of their own |

All four batches have finished, and each passed all 25 of its plugins. This report is complete.

Fix Q-332 first: a failed write answers 200, logs nothing, and can commit half a transaction.

Writing this report changes no Quillo source, and there are no repros in `feedback/repros/` or rows
in `feedback/INDEX.md`. The repros are inline. They ran on the version above with their plugin's `use` line, so each needs
that line. Numbers start at Q-326, as FEEDBACK25 went up to Q-325.

## Seen again

None: the batches were told FEEDBACK26's list, and wrote around it.

## New issues

### Q-332. An error from a statement whose value is not used is lost, even a failed database write

| Kind | Detected by | Attempts |
|---|---|---|
| silent, data | plugin example, then `curl` | 5 |

Errors in Quillo are values, so a call used as a statement can fail and nobody hears. The route goes
on, it answers 200, `quillo check` says ok, and **the log has no line at all**. Every kind of call
does it: a program's function, `write_file`, `db.exec`, `db.transaction`.

```quillo
fn half(n) {
    if n % 2 != 0 { return err("odd") }
    n / 2
}

route GET /fn {
    half(3)
    { ok: true }
}

route GET /write {
    write_file("/tmp/x", "x")
    { ok: true }
}

route GET /db {
    db.exec("insert into no_such_table values (1)")
    { ok: true }
}
```

All three answer 200 `{"ok":true}`. `/write` writes no file, as the path is outside the program's
folder, and `/db` inserts nothing. `half(3)?` is a 500, as meant.

The worst of it is a transaction, as the program's work goes on after the failure and is committed:

```quillo
db.exec("create table if not exists accounts (id integer primary key, balance int)")
db.exec("delete from accounts")
db.exec("insert into accounts values (1, 100), (2, 0)")

route GET /move {
    db.transaction(() => {
        db.exec("update accounts set balance = balance - 40 where id = 1")
        db.exec("update no_such_table set balance = balance + 40 where id = 2")
        { moved: 40 }
    })
    { ok: true }
}

route GET /checked {
    db.transaction(() => {
        db.exec("update accounts set balance = balance - 40 where id = 1")?
        db.exec("update no_such_table set balance = balance + 40 where id = 2")?
        { moved: 40 }
    })
    { ok: true }
}

route GET /balances {
    db.query("select id, balance from accounts order by id")
}
```

| Request | Answer | Balances after |
|---|---|---|
| start | | 100, 0 |
| `/move` | 200 `{"ok":true}` | **60, 0: 40 is gone** |
| `/checked` | 200 `{"ok":true}` | 60, 0: no change, as it rolled back |

`/move` debits one account, fails to credit the other, commits, and tells the visitor it went well.
There is no line in the log. This is what the code looks like when the writer has not yet thought
about failure, which is the code that most needs telling.

`/checked` shows that being careful is not enough. The `?` works, and the transaction rolls back
correctly, but it gives the error back as its value, and that value is a statement's, so it is
dropped too. The visitor is told `ok` for money that did not move. It needs a `?` after the closing
`})` as well.

The guide says "SQL problems are error values, so `?` and `??` work on them", but not that a failure
with neither is lost.

**Suggestion:** three steps, the first alone worth having.

1. At run time, log any error that a statement drops, with its place and request id. It changes no
   program's behaviour, and `/move` stops being invisible.
2. Have `quillo check` warn on a call used as a statement when it can give an error: a builtin
   documented to, or a function that has `err(` or `?`. Allow `let _ = f()` to say it was meant, as
   Rust's `#[must_use]` does.
3. In a transaction, a dropped error should roll back and give the error back, as there is no case
   where committing past it is what was wanted.

### Q-326. A plugin's function hides a builtin without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent | plugin example, then `curl` | 2 |

The guide says "A function with a builtin's name is a warning (it replaces the builtin for the
whole program)". That holds for a function in a `.qwe` file. One from a plugin replaces the builtin
just the same, and nothing is said.

```rust
// v.qwer
//! Named like a builtin.
//! version: 1.0.0

/// Named like a builtin.
pub fn len(s: &str) -> i64 {
    s.len() as i64 * 100
}
```

```quillo
// v.qwe
use "v.qwer"

route GET /len {
    { v: len("abc") }
}
```

`quillo check` says ok, with no warning, and `/len` answers `{"v":300}`.

The same function in the program itself is warned about: ``warning: `len` hides the builtin `len`:
the program's own is used everywhere, so pick another name``.

A plugin is the likeliest place for this, as its author is thinking of the crate's names and not of
Quillo's builtins, and its user cannot see the function at all. Every `len` in the program changes,
in files that have nothing to do with the plugin.

**Suggestion:** give the same warning when a plugin's `pub fn` has a builtin's name, naming the
`.qwer` file and line.

### Q-327. A `u64` inside a `Value` turns into a float, where the same number alone is an error

| Kind | Detected by | Attempts |
|---|---|---|
| silent | plugin example, then `curl` | 2 |

The guide says "a `u64` or `usize` answer past `i64`'s range is a runtime error", and it is. Inside
a `serde_json::Value`, which most plugins return, the number becomes a float and loses its last
digits, and nothing says so.

```rust
//! A u64 past i64.
//! version: 1.0.0
//!
//! [dependencies]
//! serde_json = "1"

use serde_json::{json, Value};

/// Inside a value.
pub fn big() -> Value {
    json!({ "id": 18446744073709551615u64 })
}

/// On its own.
pub fn big_direct() -> u64 {
    18446744073709551615u64
}
```

```quillo
route GET /big {
    let b = big()
    { text: str(b.id), raw: b }
}

route GET /direct {
    { v: big_direct() }
}
```

| Route | Answer |
|---|---|
| `/direct` | 500; the log says `18446744073709551615 is too large for Quillo's int` |
| `/big` | 200 `{"text":"18446744073709552000","raw":{"id":1.8446744073709552e+19}}` |

Two batches met this without being told of the other. An id from an API or a hash read this way is a different number that looks right, and a lookup with
it finds nothing or something else. `json.parse` documents the same thing for ints past 64 bits;
this is for a plugin's value. Only this number was tried.

**Suggestion:** make it the same runtime error as `/direct`, naming the field. A plugin that wants
such a number as text can then say so.

### Q-331. `file()` ignores options it does not know

| Kind | Detected by | Attempts |
|---|---|---|
| silent | plugin example, then `curl` | 2 |

```quillo
route GET /f {
    file("p.log", { headers: { "content-encoding": "gzip" }, nonsense: 1 })
}
```

`quillo check` says ok, and `/f` answers 200 with `content-type: application/octet-stream` and no
`content-encoding`. The guide lists `{ cache }` and does not offer `headers`, so the header not
going out is right. What is wrong is that nothing says so: `headers` and `nonsense` go the same way
a mistyped `cache` would, leaving a file with no caching and no word about it.

**Suggestion:** have `quillo check` and the run refuse an option `file()` does not know, naming the
ones it does.

### Q-328. Two refusals at a plugin's edge read badly

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run-time error | 1 each |

Both are stopped and logged, so nothing goes wrong, but neither says what to do.

```rust
//! Edges.
//! version: 1.0.0
//!
//! [dependencies]
//! serde_json = "1"

/// Takes a small int.
pub fn small(n: i32) -> i64 {
    n as i64
}

/// Takes any value.
pub fn deep(v: serde_json::Value) -> i64 {
    v.to_string().len() as i64
}
```

```quillo
route GET /small {
    { v: small(5000000000) }
}

route GET /fine {
    { v: deep(reduce(range(0, 100), 1, (acc, i) => { a: acc })) }
}

route GET /deep {
    { v: deep(reduce(range(0, 200), 1, (acc, i) => { a: acc })) }
}
```

`/small` logs `` `small`: argument `n` must be 5000000000 does not fit in i32 ``: two sentences
joined. `/fine` works, at 100 levels. `/deep`, at 200, logs `recursion limit exceeded at line 1
column 632`. That is serde_json's limit, read out with a line and column of a text the program
never wrote, and it names neither the argument nor the limit.

**Suggestion:** `` `small`: argument `n` does not fit in i32: 5000000000 ``, and `` `deep`:
argument `v` is nested too deeply (the limit is about 128 levels) ``.

### Q-329. A plugin's answer nested too deeply is logged as a panic

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run-time error | 2 |

This is the other way across from Q-328's `/deep`, and what it says is not true.

```rust
//! Plugin edges.
//! version: 1.0.0
//!
//! [dependencies]
//! serde_json = "1"

use serde_json::json;

/// A value nested n deep.
pub fn pr_deep(n: i64) -> serde_json::Value {
    let mut v = json!(1);
    for _ in 0..n {
        v = json!([v]);
    }
    v
}
```

```quillo
route GET /deep ? { n: int } {
    let v = pr_deep(n)
    { ok: type_of(v) }
}
```

`/deep?n=126` answers `{"ok":"list"}`. `/deep?n=127` is a 500, logged as `` `pr_deep` (w.qwer)
panicked: it gave back nothing ``.

Nothing panicked, and the plugin gave back a good value. `src/native.rs` reads the answer with
`from_slice(&out).unwrap_or(Null)`, so serde_json's recursion limit is thrown away, and with no
panic to report it prints this. A plugin's author is sent looking for a crash that did not happen.

**Suggestion:** keep the error from `from_slice` and report it, as Q-328 asks for arguments:
`` `pr_deep` gave back a value nested too deeply ``.

### Q-330. Stopping while a plugin call runs is reported as a bug in Quillo

| Kind | Detected by | Attempts |
|---|---|---|
| loud | reading the log | 2 |

Q-081 was this for a job that never ends. The same line comes from a request in a plugin call.

```rust
//! Plugin edges.
//! version: 1.0.0

/// Sleeps.
pub fn pr_slow(ms: i64) -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    ms
}
```

```quillo
route GET /slow ? { ms: int } {
    { s: pr_slow(ms) }
}
```

Ask for `/slow?ms=14000`, and send SIGTERM two seconds in. The log says:

```
quillo: stopping once the requests in progress are answered (again to stop now)
quillo: internal error (a bug in Quillo) while handling GET /slow?ms=14000
```

The visitor's connection closes with no answer, which is fair for a request that takes longer than
a stop allows. But it is no bug, and the line asks the reader to think so. Anyone reading a log
after a deploy sees a bug in Quillo for each slow request cut off by the restart.

**Suggestion:** have this path say what happened, as for jobs: `quillo: stopped with GET
/slow?ms=14000 still running`. The line is in `src/server.rs`.

### Q-333. An error handed to a builtin loses the reason it failed

| Kind | Detected by | Attempts |
|---|---|---|
| loud | start-up error | 2 |

```quillo
let DATA = json.parse(read_file("does-not-exist.json"))?

route GET / = { data: DATA }
```

`quillo check` says ok. `quillo run` stops with `` `json.parse` expects a str as argument 1, got
error ``, and the file is never named, though the error it was given says it. In a route it is the
same line in the log, and a 500; `int(read_file("does-not-exist.txt"))` logs `cannot convert error
to int`, and hands on a new error value with a 200.

The reader looks at `json.parse` and at a `str`, when what happened is a missing file one call in.

**Suggestion:** when a builtin refuses an argument that is an error, say what that error was:
`` `json.parse` argument 1 is an error: read_file: cannot read does-not-exist.json ``. Better still,
hand the error on as it is, as `?` would.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `pub fn deep(v: Value)` in a repro | The file has `use serde_json::Value` | `cannot find type Value`: parameter types are written without the plugin's `use` lines, a known issue in `plugins/README.md` | 1 build |
| `echo == b` in a test script | Habit from bash | zsh reads `=` as a command to look up, and the rest of the script did not run | 1 run |
| `db.execute(...)` in a repro | Other libraries' name | `` `db` has no function `execute` ``: the check caught it, it is `db.exec` | 1 check |
| `size(request.body)` in a repro | Thought it was a builtin | `unknown name size`: the check caught it | 1 check |
| Filing `request.body` and repeated headers as issues | The batch did, having looked in `INDEX.md` and not found them | The guide states both: documented, so they are under Limitations here | 1 read |

## Limitations

- **A header sent twice keeps the last.** The guide says so: `-H 'x-a: one' -H 'x-a: two'` gives
  `"two"`, and only the last `Cookie` header is read. HTTP allows the two to be joined with a
  comma, and FEEDBACK26's Q-200 is the same question for queries and forms. Worth deciding along
  with it.
- **`request.body` is text.** The guide says so: bytes that are not UTF-8 are replaced, as there is
  no bytes value. `printf '\xff\xfea' | curl --data-binary @-` gives `"��a"`. A plugin checking a
  signature over a body that is not UTF-8 cannot see what was signed.

- **A gzipped file cannot be served as gzipped.** `file()` has no option for it (Q-331), and a
  `file()` value cannot be a `respond` body. The `mbtiles` plugin unpacks each tile, as tiles in
  those files are stored gzipped.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A value 100 levels deep passed to a plugin, and one 126 deep back from it | Works |
| A program's own `fn len` | Warned about by `quillo check`, as the guide says |
| A direct `u64` past `i64` from a plugin | A runtime error, as the guide says |
| An `i32` parameter given too large a number | Refused and logged, not cut down |
