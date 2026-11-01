# Feedback from writing everyday Python programs in Quillo

| | |
|---|---|
| App | None: can Quillo match Python in code? Twelve small programs timed side by side (six of them written out in both languages and compared for size), and about 60 probes of what Python code leans on |
| Quillo | 0.6.0 (d702589, 2026-11-01), `target/release/quillo` rebuilt at the head of `feedback26-fixes` |
| Model | Claude (Opus 5.5) |
| Tested with | Python 3.9.6 (`/usr/bin/python3`, the only one on this machine) on the same Mac. Wall-clock times, process start included. Tables give the middle of three runs for words, sales, sieve and fib, one run for Quillo's other rows. Data: 1.2 million words of 5,000 kinds, 300,000 JSON rows |

The answer: **in length yes, in speed on numbers far ahead, in ordinary loops one and a half to
seven times behind, and in reach no.** Python's language is larger, and some of the gap is Quillo's own choice.
Four copying problems make it look much worse than that: code written the way the guide teaches ran
over 100 times slower than the same code written another way.

Fix Q-350 first, as a failed script says nothing and exits 0. Q-352 is next and gains the most: the
guide's own form of `set` and `push` misses the path that makes them cheap.

Writing this report changes no Quillo source, and there are no repros in `feedback/repros/` or rows
in `feedback/INDEX.md`. The repros are inline, and ran on the version above. Numbers start at Q-350:
FEEDBACK27 ends at Q-333, and FEEDBACK28 is named for the Mobile findings and not yet written.

## Length

Non-blank lines and bytes, each program as a person would write it:

| Program | Python | Quillo | Which way |
|---|---:|---:|---|
| Word frequency, top 3 | 6 lines, 182 B | 4 lines, 247 B | Python has `Counter` |
| Group sales and total | 8 lines, 268 B | 4 lines, 292 B | Quillo, `group_by` and pipes |
| Sieve of primes | 10 lines, 237 B | 16 lines, 327 B | Python: a stepped `range`, `[True] * n`, `flags[j] = False` |
| `fib` | 3 lines, 79 B | 2 lines, 64 B | Quillo |
| Sum with a parse error | 9 lines, 194 B | 9 lines, 189 B | Even: `?` against `try` |
| An account with a rule | 12 lines, 318 B | 9 lines, 284 B | Quillo: a record against a class |
| **All six** | **48 lines, 1,278 B** | **44 lines, 1,403 B** | Even |

All six gave the same answers in both, but for the order of tied words and the wording of the
parse error.

## Speed

| Program | Python | Quillo as first written | Quillo rewritten |
|---|---:|---:|---:|
| `fib(35)` | 1.18 s | **0.05 s** | |
| 50 million float divisions in a loop | 1.62 s | **0.05 s** | |
| Group 300,000 JSON rows | 0.12 s | 0.18 s | |
| Sum a list of 5 million | 0.19 s | 0.39 s | |
| `filter`, `map`, `sum` of 3 million | 0.11 s | 0.29 s | |
| A map of lists, 100,000 appends | 0.05 s | 0.20 s | |
| Grid of 1,000 × 1,000, 100,000 writes | 0.04 s | 0.28 s | |
| One field of a row, a million times | 0.09 s | 0.59 s | |
| Word frequency | 0.16 s | 30 s (Q-352, Q-353) | 0.42 s |
| Sieve to 2 million | 0.13 s | over 100 s, stopped (Q-352) | 1.22 s |
| `s += "x"`, a million | 0.07 s | 15.9 s (Q-354) | 0.19 s with `push` and `join` |
| `s[i]` over 300,000 characters | 0.02 s | 3.37 s (Q-355) | none yet for `s[i]`; `for ch in s` takes 0.40 s over 3 million |

Grouping JSON used 323 MB where Python used 144 MB. Python 3.11 and later are faster than the 3.9
measured here, so the gaps in ordinary loops are if anything larger; no such Python was to hand.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-018, Q-314 | Both are fixed, but only for `x = set(x, ...)` in a function: see Q-352 and Q-353 | 3 |
| Q-313 | Fixed in the browser; the server has the same shape: see Q-355 | 1 |
| Q-332 | `fn f() = err("nope")`, then `f()` as a statement in a script: nothing printed, exit 0 | 1 |
| Q-333 | `lower(read_file("../text.txt"))` says `` `lower` expects a str as argument 1, got error ``, not that the path left the program's folder | 1 |

## New issues

### Q-350. A `?` at the top level of a script stops it with no message and exit 0

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a script's exit status, by hand | 2 |

```quillo
let n = int("z")?
print("after")
```

`quillo run` prints nothing and exits 0. The script has failed and nothing says so, and `quillo run
tool.qwe && deploy` goes on to deploy. Python prints the error and exits 1. Inside a function, `?`
hands the error to the caller; at the top there is no caller, and `?` acts as a top-level `return`
does. A real crash is right: `print(9223372036854775807 + 1)` prints the overflow and exits 1. A
program with routes was not tried.

**Suggestion:** an error reaching the top of the main file by `?` is printed with its line, and the
exit status is 1, as for any stop.

### Q-351. `pow` of ints turns into a float past 2^63 and prints wrong digits as if exact

| Kind | Detected by | Attempts |
|---|---|---|
| silent, documented | comparing with Python | 2 |

```quillo
print(pow(2, 200))     // 1606938044258990300000000000000000000000000000000000000000000
print(pow(3, 40))      // 12157665459056929000; it is 12157665459056928801
print(pow(2, 53) + 1 == pow(2, 53))    // false
print(type_of(pow(2, 63)))             // float
print(pow(2, 1024))    // inf
```

The guide says it, at one row: "an int while it fits, else a float". But `*`, `+` and
`math.factorial` stop with `integer overflow` at the same size, and the guide's line is "overflow is
an error". Since no exponent is written, a float of this size reads as an exact integer with no sign
that its last digits were made up.

**Suggestion:** `pow` of two ints overflows as `*` does, and `math.pow` stays the float. It is a
change of behaviour, so it is a decision. At the least, `quillo check` could warn where it sees
literal ints that do not fit.

### Q-352. `x = x | set(...)` and `x = x.push(...)` copy, and that is the form the guide teaches

| Kind | Detected by | Attempts |
|---|---|---|
| perf, a docs error | a timeout, then timing each form | 4 |

The fix for Q-018 and Q-314 looks for one shape of code. `in_place_call` in `src/interp.rs` takes
only `Expr::Call` with a name and the variable as first argument, so the pipe and the method, which
the guide says mean the same call, copy the whole list each time.

```quillo
fn f(n) {
    let xs = []
    for i in 0..n { xs = xs | push(i) }
    len(xs)
}
print(f(100000))
```

| 100,000 appends, in a function | Time |
|---|---:|
| `xs = push(xs, i)` | 0.04 s |
| `xs = xs \| push(i)` | 13.25 s |
| `xs = xs.push(i)` | 13.74 s |
| `xs = [...xs, i]` | 13.50 s |
| `xs += [i]` | 7.14 s |
| `xs = xs + [i]` | 6.80 s |

`set` on a list of `n` the same way:

| `n` | `xs = set(xs, j, 1)` | `xs = xs \| set(j, 1)` |
|---:|---:|---:|
| 20,000 | 0.03 s | 0.62 s |
| 40,000 | 0.03 s | 2.41 s |
| 80,000 | 0.04 s | 9.69 s |

A map is no different: `m = m | set(k, v)` over 300,000 rounds with 20,000 keys takes 29.9 s, and
`m = set(m, k, v)` 0.15 s.

The guide's first example of updating a list is `todos = todos | push("x")`, and its table says
`push`, `set` and `+` are cheap when nothing else holds the old value. For `|`, `.method` and `+`
that is not so. Counting assignments of this kind in `.qwe` files:

| | `x = x \| set(...)` and friends | `x = set(x, ...)` and friends |
|---|---:|---:|
| `app/` | 6 | 34 |
| `~/dev/quillo-website` | 6 | 38 |
| `~/dev/quillo-components` | 2,172 | 1,076 |

The kit's are mostly browser code, which goes through the compiled JavaScript and was not timed
here. The check that lambdas use, `reduce(xs, {}, (m, x) => set(m, x.id, x))`, is the same function,
so from reading it a lambda written `m | set(x.id, x)` will miss too.

Python's `xs = xs + [i]` and `[*xs, i]` are also quadratic, so those two rows are fair. Its `+=`
is not, and neither is any way of writing `append`.

**Suggestion:** have `in_place_call` take `Expr::Pipe` and a method call on the variable as it takes
the plain call, and `+=` on a list. Add both to the Q-314 repro, which has only the plain form.

### Q-353. A variable at the top level of a file copies on every update

| Kind | Detected by | Attempts |
|---|---|---|
| perf, undocumented | a rewrite that changed nothing, then moving the loop | 3 |

`in_place_call` skips globals, "which other requests read". In a script, though, the top level is
where most of the code is, and nothing else reads it. The word count stayed at 30 s after changing
to the direct `set`, and took 0.42 s once the same loop was in a function:

```quillo
let counts = {}
for w in re.split(lower(read_file("text.txt")), "\\W+") {
    counts = set(counts, w, get(counts, w, 0) + 1)   // 30 s; in a `fn`, 0.42 s
}
```

Nothing tells the reader. The guide mentions neither this nor the need for a function.

**Suggestion:** treat globals as locals in a program with no routes, jobs or components, where no
other request exists. Otherwise, say it in the guide, and have `quillo check` warn on such a loop.

### Q-354. `s += "x"` in a loop is quadratic

| Kind | Detected by | Attempts |
|---|---|---|
| perf | timing probes | 1 |

```quillo
fn f(n) {
    let s = ""
    for i in 0..n { s += "x" }
    len(s)
}
print(f(1000000))
```

| `n` | Time |
|---:|---:|
| 250,000 | 0.93 s |
| 500,000 | 3.46 s |
| 1,000,000 | 15.69 s |

Python takes 0.07 s for a million, as it extends a string nothing else holds. In Quillo, `push` and
`join` take 0.19 s. Building text piece by piece is a common shape for reports and generated files.

**Suggestion:** the same idea as `in_place`, for `+=` on a local string held once. Until then, a
line in the guide pointing at `push` and `join`.

### Q-355. `s[i]` on the server walks the string from the start

| Kind | Detected by | Attempts |
|---|---|---|
| perf | timing probes | 1 |

`eval_index` runs `s.chars().count()` and `s.chars().nth(n)` for each read, so a loop over a text is
quadratic, even when every character is ASCII. It is Q-313 on the other side; d702589 fixed the
browser.

```quillo
fn f(s) {
    let c = 0
    for i in 0..len(s) { if s[i] == "x" { c += 1 } }
    c
}
print(f(repeat("x", 400000)))
```

| Length | Time |
|---:|---:|
| 100,000 | 0.40 s |
| 200,000 | 1.56 s |
| 400,000 | 6.07 s |

Python takes 0.02 s for 300,000. `for ch in s` is fine, at 0.40 s for 3 million, but a parser needs
to look ahead. A `while i < len(s)` adds a count of the characters each round as well.

**Suggestion:** when the string is all ASCII, index its bytes directly; it costs one scan, which can
be kept with the string.

### Q-356. `for i in 0..n` stops at ten million, though it makes no list

| Kind | Detected by | Attempts |
|---|---|---|
| loud, late | a probe of the limit | 2 |

```quillo
fn f(n) {
    let s = 0
    for i in 0..n { s += i }
    s
}
print(f(10))
print(f(10))
print(f(10))
print(f(20000000))    // quillo: range is too large
```

The guide says `for` runs over a range "not made into a list first", so the limit protects nothing
here, and it holds in compiled code too. The same loop with `while` runs and gives
199999990000000. It fails only once the data grows, in a program that worked on every test: a
function that passes at 10,000,000 stops at 10,000,001.

**Suggestion:** keep the limit where `range` makes a list, and lift it for `for` over `a..b` and
`range(...)`.

### Q-357. A script cannot be a command-line tool

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | writing a small tool | 3 |

d702589 gave scripts `args()`, but what comes in cannot be used:

```quillo
print(read_file(args()[0]))
```

`quillo run tool.qwe -- /etc/shells` gives `err("read_file: path must stay inside the program's
folder: /etc/shells")`. There is no option to widen that. And:

| Python | Quillo |
|---|---|
| `open(path)`, anywhere the user may read | Refused outside the program's folder; `write_file` too |
| `sys.stdin`, `input()` | `/dev/stdin` is refused, and there is no `input` |
| `sys.exit(3)` | Nothing: top-level `return 3` exits 0, and only a crash exits 1 |
| `os.listdir`, `glob` | Nothing built in; plugins `glob` and `file_tree` exist and were not tried |
| `subprocess.run` | Nothing built in |

The folder rule is right for a server answering strangers. A script its owner starts from a shell
is another case, and `print` plus nothing else leaves it a batch job that cannot be piped.

**Suggestion, the user's to decide:** a program with no routes may use any path the user can, or
`quillo run --allow-path /dir`; and `stdin()` and `exit(n)`, kept from a `server fn`.

### Q-358. Lists order in `sort` and `max`, but `<` refuses them

| Kind | Detected by | Attempts |
|---|---|---|
| loud | a probe | 1 |

`sort([[1, 3], [1, 2]])`, `sort_by` and `max([[1, 3], [1, 2]])` all work since d702589.
`[1, 2] < [1, 3]` says `cannot compare list with list`.

**Suggestion:** let `<`, `>`, `<=` and `>=` use the order `sort` already has.

### Q-359. Python's and JavaScript's habits get messages that do not say what to write

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | the parser | 1 each |

Q-324 did this for `<>` and others. These remain:

| Written | Message today | What it could say |
|---|---|---|
| `2 in xs` | `` expected `)` after arguments, found `in` `` | `contains(xs, 2)` |
| `1 < x < 3` | `` expected `)` after arguments, found `<` `` | `1 < x && x < 3` |
| `x ** 2` | ``expected an expression, found `*` `` | `pow(x, 2)` |
| `xs[1:3]` | ``expected `]`, found `:` `` | `slice(xs, 1, 3)` |
| `xs[::2]` | ``expected an expression, found `:` `` | no step: `filter` over `enumerate` |
| `for x in xs if c => x` | ``expected `=>` or `{` after the `for` list`` | `filter` first |
| `f(1, c: 9)` | ``expected `)` after arguments, found `:` `` | no named arguments: pass a map |
| `fn f(...xs)` | ``expected a parameter name, found `..` `` | take a list |
| `f(...xs)` | ``expected an expression, found `..` `` | spread works in literals only |
| `[a, b] = [b, a + b]` | `only variables, and the fields of the page's objects...` | `let [c, d] = ...`, then assign |
| `{ ...m, [k]: 1 }` | ``expected a map key, found `[` `` | what `{ [k]: 1 }` already says: `set(m, k, v)` |
| `try { }` | ``expected end of statement, found `{` `` | errors are values: `?` and `??` |
| `class A { }` | ``expected end of statement, found `A` `` | `type` and functions |
| `yield 1` | ``expected end of statement, found `1` `` | no generators: return a list |
| `@cache` | ``unexpected character `@` `` | no decorators |

`break` is the model for all of them: `Quillo has no break: return leaves a loop ... and find stops
at the first match`. The `{ ...m, [k]: 1 }` row is a miss in Q-324's own fix, which sees only a
computed key in first place.

### Q-360. Two rows of "Possible or not" are behind the code

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading | 1 |

- "there are no `& | ^ << >>`... (use `pow`, `math.*`)" leaves out `bit.and`, `bit.or`, `bit.xor`,
  `bit.not`, `bit.shl` and `bit.shr`, which work.
- "read CSV, XML, YAML, base64? Not built in: `split` and `re`, or a Rust crate" and "do data
  science?" do not mention the catalog, which has `csv`, `xml`, `yaml`, `toml`, `gzip`, `tar` and
  `bignum`. Those were seen in the folder, not run. `bignum` works on text, as
  `bignum_calc("2", "^", "200")`. No `base64` by that name turned up.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Timed `target/release/quillo` | Assumed it was current | It was f7e06a8, one commit behind; `--version` showed it and I saw that only after a full round. Rebuilt, and every number here is d702589's | 1 round |
| `counts = counts \| set(...)` at the top level | The guide's own form | 30 s. Changed to direct `set`, still 30 s, which looked like a wrong diagnosis until the loop went in a function (Q-352, Q-353) | 3 runs |
| A sieve with `flags \| set(j, false)` | The same | Over 100 s, twice | 220 s |
| `fn count(words)` | Python habit | `quillo run` warned that it hides the builtin: a good warning | 1 run |
| `read_file("../text.txt")` | A probe in a subfolder | Refused, and `lower` reported only that it got an error (Q-333) | 1 run |
| `${PIPESTATUS[0]}` for exit codes | Bash habit | zsh gave blanks, so the first round of exit codes was empty and Q-350 was almost missed | 1 run |
| `fib.qwe` printing twice | To see the compiled call | Doubled the first figure; cut to one call for the table | 1 run |

## Limitations

All documented, and all reasons Quillo does not match Python's reach today. Some are choices.

| Python | Quillo | Way around |
|---|---|---|
| Classes, inheritance, `dataclass` | `type` records and functions | Fits data; no polymorphism or methods with state |
| Exceptions, `try`, `finally`, `with` | Error values, `?`, `??` | Nothing runs on the way out, so a cleanup must be written at each exit |
| `yield`, lazy iterators | None | Lists, and `0..n`; an endless or very large stream cannot be walked |
| `set`, `tuple` | None | `unique`, `contains`, and a map with text keys |
| Mutation in place | None | Reassign; deep updates copy each level (`g = set(g, y, set(g[y], x, v))`) |
| `break`, `continue` | None | `return`, `find`, `filter` first |
| Named and variable arguments, `f(*xs)` | None | A map or a list |
| Bignums, `Fraction` | 64-bit ints, `dec` | The `bignum` plugin, on text |
| Comprehensions with `if` | None | `filter`, then `map` |
| Slices with a step, `in`, chained comparison, `**`, `//`, `"ab" * 3` | None | `slice`, `contains`, `&&`, `pow`, `floor(a / b)`, `repeat` |
| `f"{x:>8.2f}"` | `format_number(x, "en", 2)`, no padding | `repeat`, by hand |
| `bytes` | None | Files only |
| `subprocess`, sockets, threads | `udp.*`, `http.*`, `parallel`, `--instances` | `.qwer` for the rest |
| numpy arrays, pandas, matplotlib | `vec`, `stats`, on lists | No data frames or plots |
| PyPI | 627 plugins, and any Rust crate in a `.qwer` | The largest gap, and no language change closes it |

These read the same in both, run without complaint, and give another answer:

| Code | Python | Quillo |
|---|---|---|
| `-7 % 3` | 2 | -1 |
| `round(2.5)` | 2 | 3 |
| `round(0.125, 2)` | 0.12 | 0.13 |
| `7 // 2` in `let q = 7 // 2` | 3 | 7, the rest being a comment; `quillo check` is content |
| `xs[5]` past the end | `IndexError` | `null` |
| `m[1]` after `set(m, 1.0, "f")` | `"f"` | `null`: the key is the text `"1.0"` |
| `set(m, 1, a)`, then `set(m, "1", b)` | two keys | one key |
| `if 0`, `if ""`, `if []` | false | true |
| `"a  b".split()` | `["a", "b"]` | `split("a  b")` is `["a", "", "b"]` |
| `true == 1` | true | false |

A person porting code by eye will hit these. A converter would have to handle each of them.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Float text | `0.1 + 0.2` and `1 / 3` print as Python's, and `sum([0.1, 0.2, 0.3])` matches Python 3.9's |
| Int overflow | `9223372036854775807 + 1` stops with `integer overflow` and exit 1; a literal too large is a parse error |
| 9,000 calls deep | Works; Python's default stops it with `RecursionError` |
| A closure that counts, a local `fn` that calls itself | Work |
| `-1` as an index on a list and a string, and `zip` and `enumerate` taken apart in a `for` | Work |
| `"héllo".upper()`, a multi-line template string, `json.stringify(v, 2)` | Work |
| A failed `int("x")` | An error value, with a clear message |
| `break`, and a name close to a builtin | The messages say what to do (`` did you mean `int`? ``) |

## Not tested

Browser code (none of the timings are the compiled JavaScript), a program with routes for Q-350,
Python 3.11 or later, the catalog's plugins, and anything needing more than one core.
