# A reader's critique of Quillo, checked claim by claim

| | |
|---|---|
| App | None: a critique of the language, "what developers would complain about", in four points |
| Quillo | 0.6.0 (b28e689, 2026-10-30) for the findings; fixed on branch `feedback19-fixes` (library-hints and main merged) |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` and `quillo run` on small programs; the fixes on the server, in Node with the browser's runtime, and in headless Chrome |

What held up: the critique's facts about what Quillo lacks are mostly right, and two of its
four conclusions are not. Each claim was tried before anything was changed.

| Claim | Verdict |
|---|---|
| `0` and `""` being truthy causes silent bugs | True, and worse than it said: see Q-150 and Q-151 |
| No `break`, `continue`, `try`/`catch` or bitwise operators, 10,000 calls deep | True but for `try`/`catch`, where `?`, `??` and error values cover what is expected. See Q-152 and Q-153 |
| No generics, classes or interfaces | True, and it costs little: the checker carries `list<User>` through `map`, `filter`, `first` and declared return types, and caught `.nmae` in each. Only an unannotated helper loses it: `fn head(xs) = xs[0]` |
| `use` puts everything in one namespace, a worry for microservices | The namespace is per program, and `service` and `mount` are the boundaries, so microservices are not affected. A collision is an error naming both files. A large single program does pay for it, by renaming |

## New issues

### Q-150. `x || "text"` is not warned about where `x`'s type is not known

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` on seven forms of the JavaScript habit | 1 |

The warning needed a known string or number, and what comes from a request or a row has no
known type. Of seven forms, it caught a typed parameter and `len(...)`. It missed an untyped
parameter, `request.query.name`, `request.json.title` and two fields of a row: the places a
blank value comes from. A blank field then stays blank, and nothing fails.

```quillo
route GET / = request.query.name || "anonymous"
```

Fix: the fallback says what the value was meant to be. A non-empty string or template, or a
number other than 0, makes `||` treat a value of no known type as that type or null. `""` and
`0` are left out, as both languages give the same answer, and so are lists, maps and names.
None of the 280 lines with `||` in `app/` has a string or number right after it.

Not done: `x ? x : "text"` is the same habit and is not warned about.

### Q-151. There is no short way to say "blank or missing"

| Kind | Detected by | Attempts |
|---|---|---|
| silent | running the guide's own suggestion | 1 |

`??` keeps `""`, and the guide's `name != "" ? name : "anonymous"` gives null for a missing
value, as `null != ""` holds. Testing both repeats the expression twice.

```quillo
let q = {}
let name = q.name
print(name != "" ? name : "anonymous")   // null
```

Fix: `filled(x)` gives `x`, or null for `""`, so it works with `??` and in a pipe. Only `""`:
it is what differs from JavaScript for text. `0` is a value, and `[]` is true in both
languages. The warning for a string that may be null now suggests it, and scripts
(`run_script`) may call it.

### Q-152. `break` and `continue` are only an "unknown name"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

The guide says Quillo has neither, but someone writing one by habit was told nothing of it.
Fix: the error says what to write, in the checker and at run time. They stay ordinary names,
so `let break = 1` still works.

### Q-153. No way to work on bits, and no hex literals

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the guide, and `quillo check` | 1 |

A language with UDP has binary formats to read, and had no way to mask a byte. Fix: `bit.and`,
`or`, `xor`, `not`, `shl` and `shr`, as functions since `|` is the pipe, and `0xFF`.

They work on 64 bits on both sides. JavaScript's own operators keep 32, and the page keeps an
int past 2^53 as a BigInt, so the browser's side goes through BigInt and comes back as the int
the page would have had. The bundle carries them only for a page that uses them, which a test
in Chrome covers.

Decisions: `shl` drops bits past the top rather than failing, as it is shifting, not `*`. `shr`
keeps the sign. A shift outside 0 to 63 is an error, since it would otherwise lose every bit or
differ by platform. A hex literal past 63 bits is an error, like any int that does not fit, so
`0xFFFFFFFFFFFFFFFF` is not -1.

### Q-154. A program has one set of names: a file sees everything, and two files cannot share one

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the critique's fourth point, then all 18 programs in `app/` and `mobile/` | 1 |

Every file saw every other file's names, so a file's `use` lines said little, and a second
`fn helper()` in any file was an error. `private` helped a file's own helpers, and nothing
helped two public names, which is what a kit of components and a set of plugins run into.

```quillo
// lib/a.qwe and lib/b.qwe each: fn helper() = ...
use helper as a_helper from lib/a.qwe
use helper as b_helper from lib/b.qwe
```

Fix: option 3 of the four put to the user, modules that are public by default. `src/modules.rs`
links a program once its files are parsed. A name two files define is renamed for each, as
`private` is, and only then, so a program with no clash runs as it did and the runtime keeps
one table. The checker holds each file to what it took. `quillo fix` writes the lines: 991
names in 131 files of `app/` and `mobile/examples/fieldbook`, 554 lines, and no warning of
this kind left.

Moving those files found two things. Assigning to another file's global never reached the
check, so `quillo fix` would have missed it. And `quillo build` and a service's program
followed a nested `use` from that file's folder, not the program's: `quillo build` stopped
on nine of the apps with `cannot read app/forum/lib/lib/auth.qwe`, and a service's program
stopped in `quillo check`. Both are fixed, and tested in `tests/modules.rs`.

Trying the language server by hand found a third. It matched names by the word, so `helper`
in a file that took `lib/b.qwe`'s went to `lib/a.qwe`'s with that signature, called private,
and an alias found nothing. The checker now tells it each name as the file wrote it, and a
name on a `use` line goes to the file it comes from. Tested in `tests/lsp.rs`.

Decided, and easy to change: a name not taken is a warning for now and still works. Making
it an error is one line, `self.warn` to `self.error` in `Checker::taken`.

Not done:

- A prefix, `use lib/auth as auth` with `auth.login()`. `<auth.Post/>` means a class in
  markup, and renaming a whole file's text is not aware of scopes.
- `x.helper()` does not reach a name two files define, as it looks the plain name up.
- A prop cannot share a name two files define; the error says so.
- `quillo fix` works on one program: it does not go on into a service or a mounted program.
- The kit, the website and the plugins are in other repositories and were not moved. They
  still run, with warnings.

### Q-155. Nothing tells a browser library from a file of the program

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the user, while Q-154 was being built | 1 |

`use Chart from "https://..."` and `use helper from lib/a.qwe` read alike, and only the end
of the path told them apart, in a line that now means something else for each.

Fix: `use client`, one marker for whatever runs in the browser, chosen by the user over one
word for a CDN and another for local assets, as the path shows which it is. It goes before
the names and works with `with preload`, `with preconnect`, `with module`, `default as` and
`* as`. `client` on a Quillo file is an error, and `client` stays a name, so
`use client from lib/a.qwe` still takes one. The old spelling works, with a warning, and
`quillo fix` rewrites it.

Q-156 replaced this spelling before it shipped: `use client` became a first word, `client`.

### Q-156. A `use` line names what it takes before the file it takes from

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the user, on the way to better suggestions in the editor | 1 |

`use latest, Feed from lib/forum.qwe` puts the names first, so the editor had nothing to offer
until the path was written and the cursor taken back. The rest of the line had the same order
problem: the words that say how a browser library arrives (`with preload`) sat after the names
and before a long address, and the names of a long address ended past column 100.

Fix: the file comes first, chosen by the user for three reasons.

1. **Mental model.** The line reads as what kind of code, where, then which names. Its first
   word says which side of the program it is on: `import` for Quillo and Rust, `client`,
   `preload` or `preconnect` for the browser, in one place and with none optional.
2. **Editor.** With the file known first, the editor offers its names as they are typed and
   leaves out those taken. `from` was the first choice of word and was dropped: it starts 112
   lines of SQL and is defined as a name 62 times in the repositories searched, where `import`
   appears in neither way.
3. **Readability.** Sources line up on the left, and a long address may leave its `use` to the
   next line, so the names stay readable. `quillo fix` breaks a line past 90 columns.

The four words are not reserved. A line is an import only if a file and then `use` follow, on
that line or the next, so `client / 2` and `import(x)` are what they were; so is a file called
`module`. `quillo fix` turned 423 lines around in `app/` and `mobile/examples`.

0.6's `use a from lib/x` and `use Chart with preload from "..."` still work, with a warning, as
they shipped; `use client` never did, and is gone.

Found while building it: after `use *` the lexer took the `*` for an operator and ate the line
end, and `with module` overwrote the flag that marks an old line, so `quillo fix` missed
`use * as THREE with module from ...`. Both have tests in `tests/imports.rs`.

### Found on the way to 100% coverage, open

Each has an ignored test, `#[ignore = "finding: ..."]`, that fails until it is fixed.

- **Q-157.** A service called in a helper does not count as the `http` effect: `called_names`
  keeps only `charge` for `billing.charge(1)`, so `fn safe() with db { charge() }` is not told
  it may use `http`. `tests/cover_core.rs`,
  `a_service_called_in_a_helper_counts_as_the_http_effect`.
- **Q-158.** A `pub fn` with `;` in its parameters is dropped without a word: for
  `pub fn first(a: [u8; 3]) -> i64` the signature is cut at the `;`, and the function is
  neither exported nor told. `tests/cover_core.rs`,
  `a_rust_signature_with_an_array_says_it_is_not_exported`.
- **Q-159.** A request panics when another opens a tenant's database while the default one
  opens. `backend()` opens `$QUILLO_DB` without holding the slot's lock, so `get_or_insert`
  returns the tenant slot and `unreachable!("set just above")` in `db.rs` answers 500.
  `tests/cover_data.rs`, `tenants_opened_while_the_default_database_opens`.
- **Q-160.** The smallest PostgreSQL interval overflows `us.abs()`: `select interval
  '-2562047788:00:00' - interval '54.775808 seconds'` gives wrong text in a release build,
  `--2562047788:00:-54.-775808`, with no error. A debug build panics, and the request sees "the
  database call failed unexpectedly". `months.abs()` and `days.abs()` read the same way at
  `i32::MIN`. `tests/cover_data.rs`, `the_smallest_interval_is_read_as_it_is`.

## Left as they are, on purpose

- **Truthiness.** `0` and `""` stay true: `count && <Badge/>` showing a badge for 0 is what a
  reader of the code expects, where JavaScript prints a `0`.
- **`try`/`catch`.** A mistake in the code stops: `deep(11000) ?? "caught"` ends the program, and
  in a route is a 500.
- **The depth limit.** `for` and `while` cover what deep recursion would.
- **Generics and classes.** Nothing an app has needed yet.
- **Private by default.** Modules are public by default (Q-154). Private by default was option
  4, and is a later choice.

## For the next run

The VS Code grammar in `quillo-editors` does not colour a hex literal as a number, and does not
know `import`, `client`, `preload` or `preconnect` as the start of a line. It is a separate
repository and was not changed.

A type holding an escaped quote (`fn f(x: "a\"b")`) gets no parameter hints: the signature is
written without the backslash, so it does not read to its end. Nothing fails, and
`tests/lsp.rs` pins what it does today. It has no number yet.
