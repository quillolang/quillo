# Feedback from probing `quillo check`: syntax errors, argument counts, scope, typos, reserved words and stray characters

| | |
|---|---|
| App | None: about 450 probe programs of 1 to 6 lines, each written to make `quillo check` meet one mistake. Covered: invalid syntax, too many and too few arguments, out-of-scope names and variables that hold functions, typos (keywords, names, types, fields, module functions, attributes), `param: type` written swapped or in another language's form, `switch`, `where`, `any`, reserved words as names, extra characters anywhere (doubled, stray, invisible, lookalike), patterns, routes, imports, and the command line |
| Quillo | 0.6.0, release build of the runtime-coverage branch's working tree (it reports `54dd1bd`, built 2026-11-02). None of the branch's uncommitted work touches the parser or the checker's rules probed here |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` on each probe, then `quillo run` on the ones `check` passed, to see what really happens; `curl` against `quillo run --port` for the routes and the markup. No browser run. Read: [src/check.rs](../../src/check.rs) (`suggest`, `edit_distance`), USERGUIDE.md (Language tour, Checking, Files of a program, Routes, HTML literals) |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section. Numbers start at Q-503, after FEEDBACK44.md's Q-502.

What works well: argument counts and types for the program's own named functions, views, components and `server fn`s, called directly, through `|`, as a method or from a component's handler (`` `add` takes 2 arguments, got 3 ``, `` missing argument `b` in call to `add` ``, `` <Card> has no parameter `extra` ``). Names used outside their function, block, `for`, `match` arm or route are found. Known map shapes catch field typos (`a.nmae`, did you mean `name`?), and records too. Invisible characters are named by their code (U+200B, U+00A0, U+FEFF, U+0000). Many JavaScript habits get a message of their own: `**`, chained comparisons, `<>` fragments, `@` outside markup, `nil`, `fn` for an anonymous function, `s.length`, named arguments, a spread in a call, `{{ x }}` in markup, a handler given text, unknown HTTP methods, duplicate path parameters, unknown effects, and `--strict-nul` (did you mean `--strict-null`?). Closing tags are matched with the line of the opening one.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-008 | The "if the code was written for a newer Quillo, that would explain it" note still follows plain mistakes that no Quillo will ever accept: `const x = 1`, `var x = 1`, `fun add(a) = a`, `from lib/m.qwe import price`, and `let x = 1 @ 2`, whose own message says "there are no decorators". The guide says the note follows "an unknown word", so this is by design, but these words are known: they are other languages' (see Q-520) | probes |

## New issues

### Q-503. An unclosed `/*` swallows the rest of the file, and `quillo check` says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`: the route was missing | probe |

```quillo
let a = 1
/* the totals
let b = 2
print(a + b)
route GET /x = "x"
```

`quillo check` answers `ok (0 routes, 0 functions, 0 components)`, and `quillo run` prints nothing and exits 0. A route, a function or a whole page after the `/*` disappears without an error. An unterminated string or template is an error (`unexpected end of input: unterminated string`), so this should be one too.

**Suggestion:** at end of input inside a block comment, report `` `/*` on line 2 is never closed: add `*/` ``, pointing at the line where it opened.

### Q-504. A mistyped type in a pattern binds anything (lower case) or never matches (capitalised), and `check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`: the wrong arm answered | probe |

```quillo
let x = match "hello" {
    intt => "number"          // a name, so it binds "hello"
    str => "text"
}
print(x)                      // number

fn f(x) = match x {
    Strr => "s"               // no type Strr: never matches
    _ => "other"
}
print(f("a"))                 // other

route GET /d ? { page: itn } = `page ${itn}`     // binds `itn`, checks nothing: /d?page=abc answers "page abc"
route POST /a { json: Usr } = json               // no type Usr: no request ever matches
```

`check` passes all four. In a pattern, a lower-case word is a binding, so a typo of `int`, `str`, `list` or `map` turns a type test into a catch-all, and the arms after it can never run. The unused binding (`intt`) gets no "unused variable" warning either. A capitalised name with no `type` is a type test that never matches. The guide says so ("`match x { Foo => ... }` with no type `Foo` never matches"), but `check` already knows every type in the program.

**Suggestion:** an error for a capitalised pattern name that no `type` declares, with "did you mean `Str`/`User`?". A warning for a binding pattern within edit distance 2 of a type name (`intt`, `itn`, `strr`, `lsit`), and for any bound name that is never used in its arm. A warning for an arm that comes after a catch-all (see Q-527).

### Q-505. A list or map function whose result is thrown away does nothing, and `check` does not say so

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | probe |

```quillo
let xs = [1]
xs.push(2)              // JavaScript habit
push(xs, 2)
xs | push(2)
sort(xs)
let m = { a: 1 }
set(m, "b", 2)
print(xs, m)            // [1] { a: 1 }

component C() {
    state items = []
    <button @click={() => { items.push(1) }}>add</button>     // items stays []
}

let i = 0
while i < 3 {
    i + 1               // meant i += 1: never ends
}
```

Values are immutable, so each of these makes a new list or map and drops it. `check` passes them all. The `while` hangs: `check` warns about `while true { }` ("this `while` never ends") but not about this loop, whose condition reads only `i`, which nothing in the body assigns. Expressions that do nothing pass the same way: `x == 2` alone on a line (an assignment meant), and `n + 1` or `n == 1` in a handler.

**Suggestion:** a warning when the value of `push`, `set`, `sort`, `sort_by`, `filter`, `map`, `merge`, `drop`, `take`, `reverse` (any builtin that returns a new value and has no effect) is the whole statement: `` `xs.push(2)` makes a new list and drops it: write `xs = xs.push(2)` ``. The same for `==`, `+` and other operators as a statement. For `while`, warn when no variable in the condition is assigned in the body.

### Q-506. `route GET /a/{id}` is the path `/a/` and a request pattern `{ id }`: `check` ok, every request 404

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `curl` | probe |

```quillo
route GET /a/{id} = `got ${id}`
```

`check` says ok (1 route). `curl /a/5` and `curl /a/%7Bid%7D` are both 404. Braces for a parameter are the form of OpenAPI, Express 5 docs, ASP.NET and FastAPI, so this mistake will be common. The `{` right after a `/` with no space reads as the request pattern.

**Suggestion:** an error for `{` straight after a `/` in a route path: `` path parameters are written `:id`, as `/a/:id` ``.

### Q-507. `import ... use price as print` replaces the builtin `print` everywhere, without a warning

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | probe |

```quillo
// lib/m.qwe: fn price(x) = x * 2
import lib/m.qwe use price as print
print("hello")          // lib/m.qwe:1: cannot multiply str and int
```

`check` says ok. `fn print()` gets "a function named like a builtin" as a warning, and a top-level `let count` gets one too, but a name made with `as` gets nothing. With `print(1)` the program prints nothing and exits 0.

**Suggestion:** the same warning as for `fn print`, at the `as`.

### Q-508. A name declared twice in one place is accepted: parameters, local functions, record fields, states, effects

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | probe |

```quillo
fn f(a, a) = a                         // f(1, 2) is 2
let g = (a, a) => a
fn h(a: int, a: str) = a
fn outer() {
    fn inner() = 1
    fn inner() = 2                     // the second wins: outer() is 2
    inner()
}
type User = { name: str, name: int }
component C() {
    state n = 0
    state n = 1
    <p>{n}</p>
}
fn k() with db, db = 1
```

All pass `check`. At the top level a second `fn` is an error that names both places (Q-004). Inside a function it silently replaces the first. A repeated parameter takes the last argument. JavaScript's strict mode, Python and TypeScript refuse a repeated parameter.

**Suggestion:** errors for a repeated parameter, a repeated record field and a second `fn` or `state` of one name in one body (the top-level message, "defined twice, at ... and ..."). A warning for a repeated effect.

### Q-509. Misspelt attributes and events in markup pass `check` and do nothing on the page

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `curl` of the page | probe |

```quillo
view V() = <div className="box" clas="a"><a hreff="/x">x</a><p @clik={() => 1}>y</p></div>
route GET / = <V/>
```

`check` says ok. The page is `<div className="box" clas="a"><a hreff="/x">x</a><p>y</p></div>`: no class, no link, and the handler is silently dropped. The guide says `class`, not `className`. Also passed: `<card/>` when the program has `view Card` (an unknown HTML element), and `style={{ colr: "red" }}`.

**Suggestion:** `check` knows the HTML elements (it has a list for Q-449) and can know their attributes and the DOM events. Warn on an attribute or `@event` that no element has, with did-you-mean (`clas` → `class`, `hreff` → `href`, `@clik` → `@click`). Warn on `className` (write `class`), and on a lower-case tag that matches a view's name.

### Q-510. `"Hello ${name}"` in a plain string is not interpolated, and `check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | probe |

```quillo
let name = "Ada"
print("Hello ${name}")     // Hello ${name}
```

Only template strings interpolate. In markup text, `check` already asks about `${name}` ("write `{name}`"). In a string with `"` or `'` it does not, though Kotlin, Dart, Groovy, PHP and the shell all interpolate there.

**Suggestion:** a warning when a plain string holds `${` followed by a name in scope and `}`: `` this string is not a template: write `Hello ${name}` with backquotes, or `\${` to keep the text ``.

### Q-511. A typo in `request`'s fields is null, and `check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `curl` | probe |

```quillo
route GET /e = request.qurey.name ?? "none"     // /e?name=x answers "none"
```

`request` has a fixed set of fields (`method`, `path`, `query`, `json`, `form`, `headers` and the rest), so it can be a known shape the way a map literal is. Today it is open, so `qurey`, `heders` and `jsno` read as null.

**Suggestion:** give `request` (and `session()`, an upload, a `http.*` response) a closed shape in the checker: `` `request` has no field `qurey` (did you mean `query`?) ``.

### Q-512. `list<int>` accepts `[1, "a"]`, in `check` and when the program runs

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | probe |

```quillo
fn f(a: list<int>) = a
print(f(["a"]))         // check: must be list<int>, got list<str>
print(f([1, "a"]))      // check ok; runs, prints [1, "a"]
```

A list that is all of the wrong type is refused. A list that is partly wrong is not. The guide says `list<int>` is `list` when the program runs, so only `check` can catch it, and it should.

**Suggestion:** a list literal's item type is the union of its items' types. `list<int | str>` is not `list<int>`.

### Q-513. JavaScript habits that run and mean something else: `index_of(...) == -1`, `if f {` and `:id + 1`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`, `curl` | probe |

```quillo
print(index_of("abc", "x") == -1)     // false: index_of gives null, never -1

fn ready() = false
if ready { print("yes") }            // yes: a function is truthy (the call was forgotten)
print(ready == false)                // false

route GET /b/:id = id + 1            // /b/5 answers "51"
```

All pass `check`. The guide's table of differences from JavaScript lists `index_of`'s `null`, and Routes shows `:id(int)` for arithmetic. Nothing points at either where the mistake is written. TypeScript warns on `if ready` when `ready` is a function ("This condition will always return true since this function is always defined").

**Suggestion:** warnings for `index_of(...) == -1` (or `!= -1`) (`` `index_of` gives null when it is not there ``); for `if`, `&&`, `||`, `!` and `? :` on a value known to be a function (did you mean `ready()`?); for `==` between a function and anything but a function; and for `+` between an untyped path parameter and a number (`` `id` is text: declare `:id(int)` to add to it ``).

### Q-514. A function that reads a top-level `let` defined after the call passes `check` and fails at run time

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo run` | probe |

```quillo
fn f() = later
print(f())              // quillo: main.qwe:1: unknown name `later`, in f, called at main.qwe:2
let later = 1

let h = () => hlate
print(h())              // the same, from a lambda
let hlate = 1
```

`check` says ok for both. For a direct read, `print(later)` before the `let`, it says `` `later` is used before it is defined (line 3) ``. The run fails with "unknown name", which reads as a typo.

**Suggestion:** when top-level code calls a function, check what the function reads (as the checker does for effects) against the globals defined so far. Report it as "`f` reads `later`, which is defined at line 3, after this call on line 2". The run-time message should say the same, not "unknown name".

### Q-515. Argument counts are not checked for lambdas: in a `let`, given to `map`/`filter`/`reduce`/`sort_by`, in a map, or returned

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo run` | probe |

```quillo
let f = (a, b) => a + b
print(f(1))                                   // run: missing argument `b` in call to `<fn>`
print(f(1, 2, 3))                             // run: `<fn>` takes 2 arguments, got 3
print(map([1, 2], (a, b) => a + b))          // run: missing argument `b` in call to `<fn>`
print(reduce([1, 2], 0, (x) => x))           // run: `<fn>` takes 1 argument, got 2
print(filter([1], () => true))               // run: `<fn>` takes 0 arguments, got 1
print(sort_by([1], (a, b) => a))             // JavaScript's sort comparator, run: missing argument `b`
let m = { go: (x) => x * 2 }
print(m.go(2, 3))                             // run: `<fn>` takes 1 argument, got 2
fn make() = (x) => x + 1
print(make()(1, 2))
```

`check` passes every line, though it holds a lambda's parameter count, and for `map` and the like knows how many arguments the builtin passes. `fn` and local `fn` are checked. A `let` of a lambda is the form the guide teaches next to `fn`. The run-time messages name `<fn>` and "in a function", not `f`.

**Suggestion:** give a `let`-bound lambda the call checks of a `fn`, and check a lambda given to a builtin against the number of arguments that builtin passes (`map` 1, `reduce` 2, `sort_by` 1). Say `` `sort_by` passes one item, not two: `sort_by(xs, (x) => x.n)` `` for the comparator habit. At run time, name a lambda by the `let` or field it was given to.

### Q-516. A `let` or `for` pattern that can never match, and a `match` that can never match, pass `check`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo run` | probe |

```quillo
let true = 1            // run: `let` pattern does not match 1
let null = 1            // the same
let 1 = 2
let [a, b] = [1]
let { name } = 5
for null in [1] => 1
let x = match 1 { "a" => 1 }      // run: no `match` arm matches 1
let y = match 1 { }               // an empty match
```

All pass `check`. In each the value is a literal, so the checker knows the pattern fails. `let true = 1` is someone trying to name a variable `true`. It should get "`true` is a keyword" like `let if = 1` does, not a pattern failure at run time. The guide says that a `match` missing a case is not found, which is fair for a value that comes from outside, but not for these.

**Suggestion:** errors where the checker knows both sides: a literal pattern against a literal of another value or type, a list pattern against a list of a known other length, a map pattern against a non-map, an empty `match`. For `let true`, `let false` and `let null`, the keyword message.

### Q-517. "Did you mean" changes from run to run, misses swapped letters, and is absent for module functions, fields and types

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` run several times | probe |

```quillo
let x = lne("a")
```

Six runs of `quillo check` on this file gave six different answers: did you mean `any`? `int`? `int`? `len`? `env`? `live`? `pirnt(1)` gets `first` or `int`, never `print`. `suggest` ([src/check.rs:2304](../../src/check.rs)) keeps the first candidate at the lowest distance, and the globals are a `HashMap`, so ties come out in random order. `edit_distance` is plain Levenshtein, so a swapped pair (`pirnt`, `lne`, `retrun`) costs 2, the same as unrelated words.

Elsewhere the suggestion is absent or far off:

| Written | Said |
|---|---|
| `json.prase("1")`, `math.sqr(4)`, `db.qurey(...)` | `` `json` has no function `prase` `` (no `parse`) |
| `xs.lenght` | `list has no field lenght` (no `len`) |
| `s.toUpperCase()` | `str has no method toUpperCase` (no `upper`) |
| `a: integer`, `a: string`, `a: ayn` | `unknown type`, then the whole list, with no `int`, `str`, `any` |
| `None`, `True`, `undefined` | did you mean `money`? `re`? (nothing) |
| `<Div>x</Div>` | did you mean `env`? (Q-449 handles `<DIV>` only) |
| `console.log`, `JSON.parse`, `parseInt` | `unknown name` (no `print`, `json.parse`, `int`) |

**Suggestion:** Damerau-Levenshtein (a swap is one edit). Break ties by a stable order: same first letter, then builtins before the program's names, then alphabetical. Use the same suggester for module functions, fields, methods and types. Add a table of other languages' names: `None`/`undefined`/`nil` → `null`, `True` → `true`, `console.log` → `print`, `JSON.parse` → `json.parse`, `parseInt` → `int`, `toUpperCase` → `upper`, `integer` → `int`, `string` → `str`, `boolean` → `bool`. Test that a suggestion is the same in 100 runs.

### Q-518. An arrow whose body assigns does not parse: `() => n += 1`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

```quillo
let n = 0
let f = () => n += 1            // error: expected end of statement, found `+=`
let g = () => n = 5             // error: expected end of statement, found `=`

component C() {
    state n = 0
    <button @click={() => n = n + 1}>{n}</button>     // error: expected `}`, found `=`
    <button @click={n += 1}>{n}</button>              // error: expected `}`, found `+=`
}
```

The guide says a lambda "may assign" the variables around it (`count += 1` inside a handler). In JavaScript and in React code, `onClick={() => setN(n + 1)}` and `() => n += 1` are the commonest handlers there are. Only the block form, `() => { n += 1 }`, parses, and the error does not point to it.

**Suggestion:** parse an assignment as an arrow's body. Failing that, say `` an arrow's body is an expression: write `() => { n += 1 }` ``. For `@click={n += 1}`, say `` a handler is a function: `@click={() => { n += 1 }}` ``.

### Q-519. `param: type` swapped or written another language's way gets a parse error with no hint, or passes and fails at the first call

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check`, `quillo run` | probe |

| Written | `quillo check` says |
|---|---|
| `fn f(int: a)` (swapped) | unknown type `a` (known types: ...) |
| `type User = { str: name }` (swapped) | unknown type `name` (known types: ...) |
| `fn f(a int)` (Go) | expected `)`, found `int` |
| `fn f(int a)` (C, Java) | expected `)`, found `a` |
| `fn f(a: Int)`, `a: Any`, `a: String` | ok; at run time: `` argument `a` of `f` must be Int, got int 1 `` |
| `fn f(a: int[])`, `a: list[int]` | expected `)`, found `[` |
| `fn f(a?: int)`, `a: ?int` | expected `)`, found `?`; expected a type, found `?` |
| `fn f(a: int): int`, `fn f(a: int) => int` | expected `=` or `{` to start the body of `f` |
| `let x: int = 1` | expected `=`, found `:` |
| `state n: int = 0` | expected end of statement, found `n` |
| `let f = (a: string) => a`, `(a: itn) => a` | expected `)`, found `:` (`fn f(a: string)` says "unknown type") |
| `let f = (int: a) => a` | expected `)`, found `:` |

The arrow rows are the worst: a lower-case type the parser does not know makes it give up on the arrow, so the error is about punctuation, not about the type. The capitalised rows pass because an unknown capitalised name is taken for a record that may come later. The guide says that is not reported, but `Int`, `String`, `Bool` and `Any` are the builtin names in Kotlin, Swift, Scala, Java and Haskell.

**Suggestion:** `` `int: a` has the name and type swapped: write `a: int` `` when the "type" is a name and the "name" is a type. `` types follow the name after `:`: `a: int` `` for the Go and C forms. `` `list<int>` `` for `int[]` and `list[int]`, `` `a: int?` `` for `a?:` and `?int`, `` `-> int` for a `fn` `` for `): int`. `let` and `state` take no type: `let x = 1`, or `int(x)`. An arrow with an unknown type should say "unknown type" as `fn` does. `Int`, `String`, `Bool`, `Any`, `Float`, `List` and `Map` with no `type` of that name: did you mean `int`?

### Q-520. Other languages' keywords and typos of Quillo's get "expected end of statement, found `x`"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

The error names the token after the mistake, not the mistake, and the Q-008 note follows several of them.

| Written | Said | Wanted |
|---|---|---|
| `const x = 1`, `var x = 1` | expected end of statement, found `x` (+ newer-Quillo note) | `there is no const: use let` |
| `fun add(a) = a` | found `add` (+ note) | the message `func`, `def` and `function` already get: `` a function is `fn name(x) = ...` `` |
| `lett x`, `retrun x`, `mtach x {}`, `esle {}` | found `x`, found `esle` | did you mean `let`, `return`, `match`, `else`? |
| `if ... elif ...` | found `elif` | `else if` |
| `switch x { 1 => ... }`, `switch (1) { case 1: ... }` | found `x`, found `{` | `match x { 1 => ... }` |
| `match x { case 1 => ... }`, `1: "a"`, `1 -> "a"` | expected `=>` after the match pattern | `no case: write 1 => "a"` |
| `n where n > 0 => ...` (Rust, Haskell) | expected `=>`, found `where` | `a guard is if: n if n > 0 =>` |
| `xs \| where((x) => x > 1)` | unknown name `where` | did you mean `filter`? |
| `import { price } from "lib/m.qwe"`, `import price from ...` | found `{`, found `price` | `import lib/m.qwe use price` |
| `from lib/m.qwe import price` | found `lib` (+ note) | the same |
| `use lib/m.qwe use price` | found `use` | `import lib/m.qwe use price` |
| `let x = 1 === 1` | expected an expression, found `=` | `there is no ===: == compares deeply` (as `**` has) |
| `a++`, `n *= 2` | expected an expression, found `+` / `=` | `a += 1`; `n = n * 2` |
| `1 & 2` | unexpected character `&` | `&& is and; bit.and for bits` |
| `.5`, `0b101` | found `.`, found `b101` | `0.5`; no binary: `0x5` or `5` |
| `x ? 1` (no `:`) | expected end of statement, found `1` | `? needs : and a value for false` |
| `typeof x` | expected `)` after arguments | `type_of(x)` |
| `if x = 1 { }` | expected `{` after the `if` condition | `= assigns; compare with ==` |
| `if x == 1: print(1)` | expected `{` after the `if` condition | (this one is good enough) |
| `type user = {...}` | expected end of statement, found `user` | `a type's name starts with a capital: User` |
| `print(“hello”)` | unexpected character `“` | `a curly quote: write "` |
| `5 – 2`, `5 − 2` | unexpected character `–` | `an en dash / a minus sign: write -` |
| `print（1）` | unexpected character `（` | `a full-width parenthesis: write (` |
| `let x = 1 \` then `+ 2` | unexpected character `\` | `lines join by themselves: end the line with +` |

**Suggestion:** a small table in the parser from these words and characters to their message, tried before "expected end of statement". For a word that is not a keyword, try did-you-mean against the keywords when the next token makes the line a statement (`lett x =`, `retrun x`). The Q-008 note should not follow a word from the table.

### Q-521. Two elements side by side in a `view` give "comparisons do not chain"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

```quillo
view V() = <p>a</p><p>b</p>
```

`error: comparisons do not chain: `a < x && x < b``. The guide says that of two elements side by side "the first goes nowhere and `quillo check` warns: wrap them in one element", and in a component's body it does. After `=`, the second `<` is read as less-than.

**Suggestion:** where a `<` follows a closed element, say `` two elements side by side: wrap them in one, like <div> ``.

### Q-522. Only the first syntax error of a file is reported

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

```quillo
let a = (1
let b = 2 +
let c = [1 2]
print(undefined_one)
```

One error, `expected `)`, found `let``, for four mistakes. Each fix means another run. For an AI writing a whole file at once, this is one round trip per mistake.

**Suggestion:** recover at the next line that starts a statement at the top level (`let`, `fn`, `view`, `component`, `route`, `type`, `use`, `import`), and report up to some number of syntax errors per file. Keep reporting the first one first (Q-033).

### Q-523. Import mistakes: reported after their effects, with an absolute path, twice, or with a wrong fix

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

With `lib/m.qwe` defining `fn price`, `private fn secret` and `let RATE`:

| Written | Said |
|---|---|
| `import lib/m.qwe use pricee` | first `i01.qwe:2: unknown name pricee (did you mean price?)`, then `/private/tmp/.../i01.qwe:1: lib/m.qwe has no public name pricee`: two errors for one mistake, line 2 before line 1, the second with an absolute path |
| `import lib/m.qwe use secret` | the same pair, both saying `secret` is private |
| `import lib/mm.qwe use price` | `cannot read ./lib/mm.qwe`, then `unknown name price (did you mean print?)`: the cascade Q-033 removed for parse errors; no "did you mean lib/m.qwe?" |
| `import lib/m.qwe price` | `` import takes names from a file, and says which: import lib/m.qwe price use ... ``: the fix shown is wrong, it should be `import lib/m.qwe use price` |
| `import lib/m.qwe use *, price` | `` expected end of statement, found `,`: there is one `let` to a line ``: there is no `let` here |
| `import lib/m.qwe use price,` then `print(price(1))` | `expected end of statement, found (`: the next line's `print` was taken as a name |
| `print(price(1))` then `use lib/m.qwe` | `` `price` is used before it is defined (line 1) ``: line 1 of lib/m.qwe, without the file's name |
| `import lib/m.qwe use price as p` then `price(1)` | ok, no warning, though `price` is "a name the file did not take" (the guide's warning) |
| `import lib/m.qwe use price` twice on two lines, `use lib/m.qwe` twice | ok, no warning |

**Suggestion:** report the import line's error alone and first, with the path as the program writes it, and drop the "unknown name" it causes. Suggest a file near the missing one. Fix the hint for a missing `use`. Do not end a `use` list at a comma before a line break. Name the file in "defined (line N)". Warn on a renamed name used by its old name, and on a repeated `use`/`import`.

### Q-524. Out-of-scope names get "unknown name", a far-off suggestion and a false "unused variable"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

```quillo
fn f() {
    print(tmp)          // unknown name `tmp` (did you mean `map`?)
    let tmp = 1         // warning: unused variable `tmp`
}

if true {
    let x = 1           // warning: unused variable `x`
}
print(x)                // unknown name `x`

let а = 1               // Cyrillic а
print(a)                // unknown name `a`
```

At the top level, the same mistake gets `` `tmp` is used before it is defined (line 3) ``, which is right. Inside a function (also from a local lambda, `let g = () => later` before `let later`) it is "unknown name" with a suggestion that has nothing to do with it, and the variable is called unused though it was used. A name declared in a block and read after it gets the same pair. A lookalike letter gets no hint at all, though `check` names invisible characters.

**Suggestion:** when the unknown name is declared later in the same function, say "used before it is defined (line 3)" as the top level does. When it is declared in a block that has ended, say `` `x` is declared inside the `if` on line 2 and is not seen after it: declare it before the `if` ``. Do not count either as unused. When an unknown name differs from a defined one only by lookalike letters (Cyrillic, Greek), say so with the code point.

### Q-525. Builtins' parameters are called `arg1`, `arg2` and `arg3` in errors, and other argument messages

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

| Written | Said | Better |
|---|---|---|
| `map([1, 2])` | missing argument `arg2` in call to `map` | missing the function: `map(xs, (x) => ...)` |
| `reduce(xs, (a, x) => a + x)` | missing argument `arg3` in call to `reduce` | `reduce` takes the start value before the function: `reduce(xs, 0, f)` |
| `replace("aa", "a")` | missing argument `arg3` | what to put in its place |
| `max(1, 2)`, `sum(1, 2, 3)` | two errors each: takes 1 argument; argument 1 must be list | one: `max takes one list: max([1, 2])` |
| `f(a: 1)` | there are no named arguments: pass a map, as `f({ c: 9 })` | `f(1)`, or `f({ a: 1 })` with the names written |
| `let n = 1` then `n -= "a"` | cannot subtract any and str | cannot subtract str from int |
| `let xs = [1]` then `xs += 2` | ok (run: cannot add list and int) | the error `[1] + 2` gets |
| `str()` | ok (null) | `str takes 1 argument` |

The builtins' docs ([src/builtin_docs.rs](../../src/builtin_docs.rs)) have the parameter names already (`xs, f` for `map`, `xs, start, f` for `reduce`), so the errors can use them too.

**Suggestion:** take the parameter names from builtin_docs. Join a count error and a type error on one call into one. Use the written names in the named-argument hint. Keep the left side's known type in `-=` and `+=`.

### Q-526. `fn` is reserved, but `let fn = 1` is accepted

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

```quillo
let fn = 1              // ok
print(fn)               // line 2: anonymous functions are written `(x) => x * 2` ...
```

Every other reserved word gets `` `let` is a keyword, so it cannot be a name here `` at the `let`. `fn` passes the `let`, and the error comes where it is used, as a hint about lambdas. `type if = int` gets "expected end of statement, found `if`" rather than the keyword message.

**Suggestion:** the keyword message for `let fn` and for `type` followed by a keyword.

### Q-527. Code that can never run is not pointed out

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

```quillo
fn f() {
    return 1
    let dead = 2        // only "unused variable `dead`"
}
let g = () => { return 1; 2 }
let x = match 1 {
    _ => "a"
    1 => "b"            // after a catch-all
}
let y = match 1 {
    1 => "a"
    1 => "b"            // the same arm twice
    _ => "c"
}
```

All pass, `f` with a warning that names the wrong problem.

**Suggestion:** `` this never runs: the `return` on line 2 leaves first ``, and `` this arm never matches: `_` on line 2 takes everything `` (and the same for a repeated literal arm).

### Q-528. The command line: `quillo check main` without `.qwe`, `--policy` with no file, "1 functions"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

- `quillo check a01`, with `a01.qwe` there: `a01: error: cannot read file: No such file or directory`. A `use` path without an extension is a `.qwe` file, so the command line could take one too, or at least say "did you mean a01.qwe?".
- `quillo check main.qwe --policy` (the file forgotten): `` unknown option `--policy` for `check` ``, though it is an option of `check`. It should be `--policy needs a file`.
- The summary says `1 routes`, `1 functions`, `1 components`.
- `quillo check main.qwe --strict-null --strict-null` is accepted; harmless.

**Suggestion:** try `<name>.qwe` when `<name>` does not exist. Say that the option's value is missing. Use the singular for 1.

### Q-529. The lexer takes forms the guide does not give, and refuses others unevenly

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | probe |

| Written | Result |
|---|---|
| `{ a: 1,, b: 2 }` | ok, though `[1, 2,, 3]` and `fn f(a,, b)` are errors |
| `1__000`, `1_` | ok: 1000 and 1 (JavaScript and Rust refuse a doubled or trailing `_`) |
| `--1` | ok: 1 (JavaScript's decrement habit) |
| `08`, `010` | ok: 8 and 10 (the guide says no octal; `010` is 8 in old JavaScript's sloppy mode) |
| `let x = 1.` then `print(x)` on the next line | read as `1.print(x)`: `` `x` is used before it is defined (line 1) `` |
| `let x = 1\rprint(x)` (a lone CR, old Mac line ends) | expected end of statement, found `print` |
| `let x = 1;;`, `fn f() {;}` | ok |

**Suggestion:** an error for an empty item in a map as in a list; for a doubled, leading or trailing `_` in a number; for a leading `0` before more digits (`08`: write `8`); a warning for `--x`. For `1.` at the end of a line, say `` a number does not end with `.`: write `1` or `1.0` ``. Read a lone CR as a line break, as CRLF is.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| A harness that wrote each case as one file and ran `check` on it | to keep each probe to one mistake | probes that mixed two mistakes (`route ... /c/:id(itn)` with others in one file) stopped at the first parse error and hid the rest (Q-522); split into one file each | 2 runs |
| `kill $P` after `quillo run` in the background | to stop the one server I started | when `check` failed, `run` had already exited, so the kill failed; harmless | 0 |
| Took `x01`'s "expected end of statement, found `+=`" for a typo of mine | it reads like one | it was Q-518: the guide's own form does not parse | 1 probe |

## Limitations

None new. The guide's list of what `check` does not find (an unknown type in an annotation, `==` between values that can never be equal, a `match` that misses a case) is the source of Q-504, Q-513 and Q-516. Each asks only for the cases where the checker already has both sides.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `add(1, 2, 3)`, `add(1)`, `add()` with a default after | `` takes 2 arguments, got 3 ``; `` missing argument `b` `` |
| a required parameter after a default, `fn f(a, b = 1, c)` | the call is checked: `` missing argument `c` `` |
| local `fn g(b)` called with 2 | `` `g` takes 1 argument, got 2 `` |
| `<Card/>` missing a prop, `<Card extra="y"/>`, `<C m={2}/>` | `<Card> needs title`, `<Card> has no parameter extra` |
| `server fn save(n: int)` called from a handler with 0, 2 or a str | each reported |
| a name from another function, a block, a `for`, a `match` arm, a route's path or query | unknown name |
| `x2 = 5` with no `let` | `` cannot assign to `x2`: it was never declared (use `let x2 = …`) `` |
| `let x = x + 1` at the top | used before it is defined |
| `let fact = (n) => ... fact(n - 1)` inside a function | `` `fact` does not exist until its `let` ends: a function that calls itself is a `fn` `` |
| `fn f(n)` and `let f = 3` | defined twice |
| `let count = 0` then `count([1, 2])` | warning at the `let`, error at the call |
| `print(inner)` from a closure | unknown name |
| `n(1)` with `n` an int, `s()` with a str, `handler()` with null | cannot call ...: it is int / str / null |
| `fn true()`, `fn f(if)`, `component match()`, `use ... as if`, every reserved word in `let` but `fn`, `true`, `false`, `null` | the keyword message |
| `let then`, `and`, `or`, `not`; `let nil` | refused with a hint; `use null instead of nil` |
| `with dbb`, `with gpu`, `with` alone | expected an effect, with the list |
| `fn f() with time = db.query(...)`; `g()` that uses db from `with time` | error; warning |
| `route GTE`, `route get`, `route GET a`, `/a/:id/:id`, `/a/:`, `:id(itn)` | each has its own message |
| `<div><p>x</div>`, `</dvi>`, `class` twice, `<>`, `{`/`}` in text | each has its own message, with lines |
| `@click="n += 1"`, `@click={inc()}` | a handler needs a function, got str / null |
| `{{ "x" }}` in markup | warning: one pair of braces |
| U+200B, U+00A0, U+FEFF in the middle, U+0000 | named, "which cannot be seen: delete it" |
| `"\q"`, `"\u00"`, `"\u{110000}"`, `${}` | each has its own message |
| `9223372036854775808`, `0x`, `0xG1` | too large; expected hex digits |
| `1 ** 2`, `1 < 2 < 3`, `1 @ 2` | there is no `**`; comparisons do not chain; no decorators |
| `s.length`, `f(a: 1)`, `f(...[1])` | `len(x)`; no named arguments; spread goes in a list |
| `func add(a)`, `def add(a)`, `function add(a)` | `` a function is `fn name(x) = ...` `` |
| `"5" * 2`, `true + 1`, `null + 1`, `[1] + 1`, `1 / 0`, `1 % 0`, `"a" < 1`, `-"a"` | each refused |
| `len(5)`, `upper(5)`, `split(5, ",")`, `round(1.2, "2")`, `1..1.5`, `"a".."z"` | each refused with the types |
| `!5`, `n \|\| 5` with `n` an int | the JavaScript-truthiness warnings |
| `while true { }` | warning: never ends |
| `quillo check` with no file, a folder with no main.qwe, two files, `--strict-nul`, `-x`, `--watch` | each refused with a message; `--strict-nul` with did-you-mean |
