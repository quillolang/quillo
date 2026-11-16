# Feedback from reviewing a proposal: `return` for every function's result, and no one-line `fn f() = expr`

| | |
|---|---|
| App | None: a design review of a proposal, for readability. A function gives its result with `return`, not by its last expression, and the expression form `fn f(x) = expr` goes away, so that every function is a block |
| Quillo | `quillo 0.7.0-dev (98b5b75, 2026-11-03)`, a release build of main. The checkout also held another session's uncommitted edits (`src/ast.rs`, `builtins.rs`, `check.rs`, `describe.rs`, `httpd.rs`, `interp.rs`, `parser.rs`, `server.rs`, `tests/routes.rs`); none of the parts read here is among them. Line numbers are the working tree's |
| Model | Claude (Opus 5.5) |
| Tested with | Probe programs P1 to P4 with `quillo check` and `quillo run` (servers on 127.0.0.1 only), headless Chrome ([feedback/tools/drive.mjs](../tools/drive.mjs)) for a component, `/usr/bin/time` against Python 3.9.6, and a new counter, [feedback/tools/fn-forms.py](../tools/fn-forms.py), over every Quillo repository on this machine. Read: [src/parser.rs](../../src/parser.rs) (`parse_fn_rest`, `parse_arrow_body`, `return`), [src/interp.rs](../../src/interp.rs) (`Ctrl::Return`, `eval_block`), [src/jit.rs](../../src/jit.rs) (`shape_ok`, `func`, `block`), [src/lexer.rs](../../src/lexer.rs) (`Token`), USERGUIDE.md ("Coming from JavaScript", "Variables, functions, views"), the VS Code snippets and the tree-sitter grammar |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. The one new file besides this report is the counter. Numbers start at Q-677, after FEEDBACK51.md's Q-676.

## The answer

The proposal can be done, and the runtime is mostly ready for it. `return` already works in every place a function can be written: `fn`, `server fn`, `view`, components, arrows and routes, on the server and in the browser (P2, P3). The checker also already reports a missing `return` in a function that declares its result type (P4).

Four things stand in the way, in this order:

1. **`return` turns the JIT off** (Q-677). A function that uses `return` is never compiled to machine code, and neither is any function that calls it. 5 million calls take 0.64 s with `return` and 0.27 s with `= expr`, against 0.33 s in Python. If every function needed `return`, Quillo would be slower than Python everywhere the JIT helps today. It has to be fixed first: Quillo is meant to beat Python.
2. **Inside a block that gives a value, `return` means something else.** In a `for … =>` body, in an `if` used as a value, or in a `match` arm, `return` leaves the whole function, and `quillo check` says nothing (Q-678; Q-021 documented this for `for` only). Once `return` is required in functions, people and models will also write it in these blocks. The rule has to say where a bare value is still right, and the checker has to catch the mistake.
3. **Changing the meaning silently would break every program.** In the current code, 6,796 block functions end in a bare value, and only 2 end in `return`. If a block without `return` gave null tomorrow, all of them would return null without an error. The change has to go through `quillo check` first: a warning, then an error. It must never become a silent null.
4. **The migration is about 18,500 edits in about 3,400 files**, so it has to be done by a tool. `quillo fix` can only add `import`s today, and tokens carry a line but no column (Q-681). A rewriter needs source positions first.

Two more checks help with or without the proposal: a function that returns a value on some paths and falls off the end on others (Q-679), and code after a `return` (Q-680).

What the proposal gains, beyond reading: `(x) => { x }` is a block today, not a map (the guide warns about it). Under the new rule it becomes an error that points to `return { x }`, so that trap goes away. Two security findings in FEEDBACK51 were functions that returned null without anyone seeing it, by an `if` with no `else` (Q-651, Q-652). A function whose result has to be written out makes that path visible.

## Part A: what the rule would say

The proposal needs a precise rule. This is the one this review recommends. The open choices are in [Questions for you](#questions-for-you).

| # | Rule | Today |
|---|---|---|
| R1 | A named function has a block body: `fn`, `server fn`, `private`/`public fn` and `view`. `fn f(x) = e` is a parse error with a fix: ``write the body as a block: `fn f(x) { return e }` `` | `= expr` or `{ ... }` |
| R2 | In a function's block (a named function, a view, a component, a block arrow), the result is what `return` gives. A function that reaches its end gives null, as in JavaScript and Python | the last expression is the result |
| R3 | A block whose last statement is a value that does nothing (a name, a literal, an operator, markup, `{ x }`) is an error: ``this value goes nowhere: `return a + b` ``. A last call (`db.exec(...)`, `print(x)`) is kept for its effect and its value is dropped | the value is the result |
| R4 | Arrows written inline keep their expression form: `xs \| map((p) => p.id)`, `@click={() => n += 1}`. A block arrow follows R2 | same |
| R5 | `if`, `match`, `for … =>` and plain `{ }` blocks used as values still give their last expression: they are not functions. A `return` inside one whose value is used is an error (Q-678) | `return` leaves the function, silently |
| R6 | Routes and components give their page with `return`: `route GET / { return <main>...</main> }`, `component C() { state n = 0; return <button>...</button> }`. `reply` is unchanged | the last expression is the page |
| R7 | A program's top level, a `use`d file's top level, and `run_script` keep their last value: they are programs, not functions | same |
| R8 | `?` stays as the early `return` of an error | same |

Before and after, from the guide:

```quillo
// today
fn add(a: int, b: int = 10) -> int = a + b
view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>
fn role(name: str) = (r) => session().role == name ? r : reply 403 "no"

// under R1 to R6
fn add(a: int, b: int = 10) -> int {
    return a + b
}
view Card(title: str, children) {
    return <section.card><h2>{title}</h2>{children}</section>
}
fn role(name: str) {
    return (r) => session().role == name ? r : reply 403 "no"
}
```

R3's split between a last call and a last value matters. `fn save(n) { db.exec("...", n) }` is a common shape for functions run for their effect, and making them write `return db.exec(...)` would add noise, not clarity. The cost is that a caller that used the call's result now gets null. So the migration tool has to add `return` to every last expression, calls too, before R3 drops anything. Then no existing program changes meaning. New code is written under the rule.

## Part B: what exists today

| Probe | Result |
|---|---|
| P1: `xs \| map((x) => { return x * 2 })` | `[2, 4, 6]`: `return` leaves the arrow only |
| P1: `let ys = for x in xs => { return x * 2 }; return ys` | gives `2`: the first round's `return` leaves the function. No warning (Q-678) |
| P1: `let l = if n == 1 { return "one" } else { "many" }` | `label(1)` gives `"one"`, not `"1 one"`. No warning (Q-678) |
| P1: a `match` arm `0 => { return "zero" }` in a `let` | leaves the function. No warning (Q-678) |
| P1: `fn none2(c) { if c { 1 } }`, untyped | null for `false`. No warning (Q-679) |
| P1: `(x) => { x }` against `(x) => { return { x } }` | `5` against `{ x: 5 }` |
| P2: `view Card(t) { return <section>...</section> }`, `return` in a component, `route GET /r { return <main>...</main> }`, an early `return <p>early</p>` in a route | all served as expected, 200 |
| P3: a component with an early `return <p>`, a block arrow with `return`, and a helper `fn` with `return`, clicked 4 times in Chrome | updates on each click, then switches to the early branch. `__quilloErrors` and `__quilloRemade` null, no mismatches |
| P4: `fn some_paths(c: bool) -> int { if c { return 1 } }` and `fn ends_let() -> int { let x = 1 }` | check errors: `` `some_paths` must return int, but this returns null `` |
| P4: `return 1` followed by `print("never")` | no warning (Q-680) |

## Part C: how much code it touches

From [feedback/tools/fn-forms.py](../tools/fn-forms.py), a heuristic scanner rather than the parser: strings, templates and comments are masked, and brackets and markup tags are counted. Spot checks agree with `grep`. For example, no top-level function in `app/` ends in `return`, and 393 end in an indented bare line. Take the numbers as estimates. `quillo-website` includes its 628 plugins.

| Form | app/ | book/ | repros | quillo-website | starter | quillo-components | quillo-examples | devtools | total |
|---|---|---|---|---|---|---|---|---|---|
| `fn … = expr` | 373 | 53 | 61 | 438 | 15 | 2,629 | 26 | 2 | **3,597** |
| … on one line | 228 | 42 | 59 | 231 | 6 | 1,487 | 20 | 1 | 2,074 |
| `view … = expr` | 133 | 7 | 7 | 114 | 12 | 1,825 | 15 | 1 | **2,114** |
| `fn { }` ending in a bare value | 478 | 21 | 39 | 380 | 11 | 5,851 | 14 | 2 | **6,796** |
| `fn { }` ending in `if`/`match` (a value) | 19 | 2 | 0 | 17 | 0 | 240 | 0 | 0 | 278 |
| `fn { }` ending in a loop | 10 | 0 | 4 | 14 | 0 | 96 | 0 | 0 | 124 |
| `view { }` ending in a value or markup | 48 | 0 | 2 | 76 | 0 | 875 | 0 | 0 | 1,001 |
| components ending in markup or a value | 142 | 49 | 71 | 168 | 5 | 2,213 | 26 | 9 | **2,683** |
| routes ending in a value or markup | 103 | 48 | 53 | 1,695 | 4 | 2 | 11 | 0 | **1,916** |
| arrows, expression body (kept under R4) | 625 | 42 | 12 | 2,076 | 6 | 11,603 | 38 | 0 | 14,402 |
| arrows, block body (R2 applies) | 276 | 30 | 15 | 608 | 3 | 6,845 | 17 | 3 | 7,797 |
| top-level `let f = (..) =>` | 0 | 5 | 0 | 10 | 0 | 8 | 0 | 0 | 23 |
| `return`, all uses | 680 | 17 | 6 | 302 | 6 | 6,376 | 8 | 1 | 7,396 |
| files read | 177 | 21 | 231 | 1,149 | 18 | 1,760 | 21 | 5 | 3,382 |

Not in the table:

- USERGUIDE.md: 38 definitions with `= expr` at the start of a line, and more inline (`` `server fn charge() = ...` ``). Its "Coming from JavaScript" section says the opposite of R2: "Blocks give back their last expression, so `return` is only for leaving early" ([USERGUIDE.md:411-413](../../USERGUIDE.md)).
- `tests/*.rs`: about 1,085 lines with `fn … = ` inside embedded programs. `src/new.rs`: 1, in what `quillo new` writes.
- The editors: the VS Code snippets `fn`, `private fn` and `view` are written `= ...` (quillo-editors/vscode/snippets/quillo.json, shared by JetBrains). The tree-sitter grammar's `function_definition` accepts both forms, and Zed and Helix pin that grammar.
- quillolang.org's `llms.txt` and `llms-full.txt`, which teach models the language.
- The block-arrow count does not say how many of them end in a value. Most are handlers ending in an assignment, which need no change.

## Seen again

- FEEDBACK2.md, Q-021: `return` inside `for … =>` leaves the function. The fix was to the guide only. Under this proposal it is the most likely mistake, and it needs a check (Q-678).
- FEEDBACK51.md, Q-651 and Q-652: a middleware and an `accept_calls` `check` that returned null by an `if` with no `else`, which let callers through. That is the case Q-679 warns about.

## New issues

### Q-677. Any `return` keeps a function, and every function calling it, out of the JIT: 2.4 times slower, and slower than Python

The three forms of the same function, called 5 million times from a `while` loop (`g` below, the same in each). Each was run three times, and the times agreed within 0.01 s:

```quillo
fn f(x) = x % 7 + 1                  // expr: 0.27 s
fn f(x) {
    x % 7 + 1                        // implicit: 0.27 s
}
fn f(x) {
    return x % 7 + 1                 // ret: 0.64 s
}

fn g(n) {
    let i = 0
    let t = 0
    while i < n {
        t += f(i)
        i += 1
    }
    return t
}
print(g(5000000))
```

| Form of `f` | JIT on | `QUILLO_JIT=0` | Python 3.9 |
|---|---|---|---|
| `= x % 7 + 1` | 0.27 s | 0.45 s | 0.33 s (`return x % 7 + 1`) |
| `{ x % 7 + 1 }` | 0.27 s | 0.45 s | |
| `{ return x % 7 + 1 }` | **0.64 s** | 0.50 s | |
| `{ if x % 2 == 0 { return 1 }; return x % 7 + 1 }` | **0.76 s** | 0.62 s | 0.35 s |
| `= x % 2 == 0 ? 1 : x % 7 + 1` | 0.28 s | 0.54 s | |

With `return`, Quillo is about twice as slow as Python. It is also slower than with the JIT turned off, so the time goes to failed attempts to compile as well as to the interpreter. `g` itself ends in `return t` in every row, yet the first two rows are fast. The slow part is calling a function that uses `return` from compiled code. A function on text (`len(`${x}`) + 1`, never compiled) takes 0.14 s in both forms, so the interpreter's `Ctrl::Return` costs nothing that can be measured.

The cause for a last `return` is in `func` ([src/jit.rs:353-358](../../src/jit.rs#L353-L358)). The body's block gives no value of its own when its last statement is `return e`, because `last` stays `None` ([src/jit.rs:466](../../src/jit.rs#L466)). Then `match (got, ret)` answers `(None, _) => None`, even though `ret` holds the type that `return` gave. The function is taken as uncompilable, and so is its caller. The early-return row ends in a bare value, so it has a second cause, not traced here. Most likely the `if` statement holding a `return` (`stmt_expr`), or `type_of` of a block that ends in `return`, which reads only a last `Stmt::Expr` ([src/jit.rs:878-890](../../src/jit.rs#L878-L890)).

Expected: `(None, Some(t)) => Some(t)` when the block ended in `return`. An early `return` inside an `if` compiled as `leave` already is. Tests are needed for both shapes, and for a recursive function with an early `return` (`fib`). `bench/vs-python` needs a case with `return`. Kind: perf. It blocks the proposal, since under R2 every function would hit it.

### Q-678. `return` inside a block used as a value leaves the function: a `for … =>` body, `let x = if …`, a `match` arm. `quillo check` is silent

```quillo
fn doubled_for(xs) {
    let ys = for x in xs => { return x * 2 }      // gives 2, not [2, 4, 6]
    return ys
}
fn label(n) {
    let l = if n == 1 { return "one" } else { "many" }
    return `${n} ${l}`                            // label(1) is "one", not "1 one"
}
```

`quillo check p1.qwe` reports only an unused variable, elsewhere. Each case is defined behaviour: `return` leaves the function, as the guide says since Q-021. But none of them is ever what the writer meant when the block's value is used. Once `return` is how a function gives its result, writing it in these blocks becomes a habit.

Expected: an error when `return` appears directly in a block whose value is used (the body of a `for … =>` that is kept, a branch of an `if` or a `match` arm in a `let`, an argument or an operand). The message names both meanings: ``this `return` leaves `label`; the `if` gives its last expression: write `"one"` ``. A `return` in a `for` or `if` statement whose value is not used stays legal. That is the early exit, as in P3's component. Kind: silent.

### Q-679. A function that returns a value on one path and falls off the end on another gives null with no warning, unless it declares its type

```quillo
fn both(c) {
    if c { return 1 }
}                              // both(false) is null; check: ok
fn none2(c) { if c { 1 } }     // the same through an `if` with no `else`
```

With `-> int` the checker catches both (P4). Most functions in the apps declare no result type. Expected: a warning on an untyped function where some paths `return` a value and the end is reachable: ``` `both` returns a value on line 2 but gives null when it reaches its end ```. Under R2 the second form is an R3 error. This is the shape behind FEEDBACK51's Q-651 and Q-652. Kind: silent.

### Q-680. No warning for code after `return`

```quillo
fn after() {
    return 1
    print("never")             // check: ok
}
```

Expected: ``unreachable: `return` on line 2 always leaves `after` ``. The same goes for code after `reply`, and after a `return` in both branches of an `if`. The more `return` is written in the middle of blocks, the more this matters. Kind: silent (dead code).

### Q-681. No tool can rewrite a function's body: tokens carry a line but no column, and `quillo fix` only writes `import`s

`Token` is `{ tok, line }` ([src/lexer.rs:118](../../src/lexer.rs#L118)). The lexer knows its `pos`, but no token keeps it, and the AST keeps lines only. `quillo fix` adds the `import`s a program needs, as whole lines at the top of a file. Rewriting `= expr` into a block, or putting `return` before a last expression that starts in the middle of a line (`if c { a } else { b }`, `match` arms, markup), needs byte positions.

Expected: tokens carry their start and end offsets (two `u32`). Expressions carry them where a rewrite needs them (a function's `=`, the start of a block's last statement). Then `quillo fix --return <file|dir>` rewrites R1 and R2. It keeps comments and indentation, and changes nothing else. Run it twice and the second run changes nothing. Its test is the repositories in Part C: every rewritten program gives the same `quillo check` result, the same HTML for each route, and the same test results as before. Kind: limitation.

## Proposed order of work

1. **Q-677, Q-678, Q-679, Q-680 now**, with or without the proposal. Each needs tests and edge tests. Q-677 also needs a `bench/vs-python` case, and Rust coverage held at 99%.
2. **Positions and the rewriter** (Q-681). Measure lexing and parsing before and after: positions must not slow the start of a 1,824-component program (1.64 s in quillo-components' bench).
3. **One release of warnings.** `quillo check` warns on R1 and R3 and offers `quillo fix --return`. Nothing changes meaning. In the same change: the guide's "Coming from JavaScript" and "Variables, functions, views", the book (run `assemble.py`, fix the hand-written chapters), `quillo new`'s template, the snippets in every editor (VS Code, then JetBrains at its next build), the tree-sitter grammar (and the commit Zed and Helix pin), `llms.txt` and `llms-full.txt`, and the language server's hover examples.
4. **Run the rewriter over every repository**, one commit each: app/ and the book, quillo-website with its plugins (prebuilt plugins such as docsearch rebuilt, and the deploy that needs), quillo-components, quillo-examples, quillo-devtools, the starter (a new tag for that release), and the programs embedded in `tests/*.rs`. Leave `feedback/repros/` as written: they record the syntax of their day, and `run.mjs --update` on the whole list would rewrite hand-kept statuses. Repros that use `= expr` get "old syntax" in their status by hand, once R1 is an error.
5. **The next release: errors.** R1 and R3 become errors with the fix in the message. A program that has not been migrated fails `quillo check` with a line per function. It never silently gives null.

## Questions for you

1. **Views.** Should `view Card() = <…>` also become a block with `return` (recommended: a view is a function, and React's components are written that way), or should views keep `= <markup>` as the one exception?
2. **Arrows.** Should inline arrows keep their expression form (`map((p) => p.id)`, recommended, as in JavaScript)? And should a top-level `let f = (x) => …` be warned to use `fn` (23 today), so that it does not become the way around R1?
3. **Routes and components.** Should they give their page with `return` (R6, recommended, for one rule everywhere), or keep the last markup as the page, since the page is what they are for?
4. **A last call with no `return`.** Should its value be dropped, as in R3 and JavaScript (recommended)? Or should every function that gives anything say `return`, with a last call an error too? The second is stricter, but makes every effect-only function end in `return db.exec(...)` or `return null`.
5. **Which release.** The 0.7 scope is already set (FEEDBACK40 to 43, the apps and the website aligned, coverage). Recommended: Q-677 to Q-680 in 0.7, and the warnings (step 3) and the rewriter in 0.8. The errors would then come in 0.9.

## Smaller notes

- `return` with no value (`return`) gives null, and the parser takes one only when the next token cannot start an expression ([src/parser.rs:1183-1186](../../src/parser.rs#L1183-L1186)). With `return` on every function, `return` followed by a line starting with `(` or `[` joins that line as its value. The guide's line-break rules say so, but a check for `return` at the end of a line, followed by a line indented the same, would catch it.
- `fn f() = { … }` is accepted today. It is a block through `parse_arrow_body`, or a map if its only content is a name. R1's message should cover it: ``drop the `=`: `fn f() { … }` ``.
- `eval_block`'s fast path for a one-statement block leaves `Stmt::Return` out, so `{ return x }` goes through `block_in`. The text-function timing under Q-677 shows no measurable cost. It is worth a second look if Q-677's fix moves the numbers.
- The browser compiler emits JavaScript's own `return` ([src/client.rs:2930](../../src/client.rs#L2930)), so R2 costs nothing in the browser.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `@click={n += 1}` in a component | Svelte's and Vue's habit | `quillo check`: `an assignment is not a value: a handler is a function, @click={() => { n += ... }}`. Clear, with the fix in it | 1 run |
| The counter run on `USERGUIDE.md` as a folder | `os.walk` on a file | 0 files; the guide was counted with `grep` instead, and the tool now takes files too | 1 run |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `return` in a block arrow given to `map` | leaves the arrow only: `[2, 4, 6]` |
| `return` in a `view`, in a component, and as a route's page, on the server | served as written, 200 |
| An early `return <p>` in a route | the early page for `?x=1`, the late one otherwise |
| A component with an early `return` in Chrome, clicked into the other branch | switched on the third click, no errors, nothing remade |
| `(x) => { return { x } }` | `{ x: 5 }`: `return` settles the block-or-map question |
| A typed function that misses a `return` on one path, or ends in `let` | check error, naming the function and the type |
| A function on text with `return`, interpreted | 0.14 s for 1 million calls, the same as `= expr` |

## Scripts

The probes, as run (P1 to P4). The benchmark files are in the table under Q-677.

```quillo
// P1: return in arrows, `for … =>`, `if` and `match` values, null endings, `{ x }`
fn doubled(xs) {
    return xs | map((x) => { return x * 2 })
}
fn doubled_for(xs) {
    let ys = for x in xs => { return x * 2 }
    return ys
}
fn label(n) {
    let l = if n == 1 { return "one" } else { "many" }
    return `${n} ${l}`
}
fn kind(x) {
    let k = match x {
        0 => { return "zero" }
        _ => "other"
    }
    return `kind ${k}`
}
fn none1() {
    let x = 1
}
fn none2(c) {
    if c { 1 }
}
let same = (x) => { x }
let made = (x) => { return { x } }
fn run(f) = f()
fn outer() {
    let v = run(() => { return 7 })
    return v + 1
}
print(doubled([1, 2, 3]))      // [2, 4, 6]
print(doubled_for([1, 2, 3]))  // 2
print(label(1))                // one
print(label(2))                // 2 many
print(kind(0))                 // zero
print(kind(5))                 // kind other
print(none1())                 // null
print(none2(false))            // null
print(same(5))                 // 5
print(made(5))                 // { x: 5 }
print(outer())                 // 8
```

```quillo
// P2: return in views, components and routes (curl)
view Card(title) {
    return <section><h2>{title}</h2></section>
}
view Plain(title) = <section><h2>{title}</h2></section>
component Counter() {
    state n = 0
    return <button @click={() => { n += 1 }}>clicked {n}</button>
}
route GET / {
    <main><Card title="a"/><Plain title="b"/><Counter/></main>
}
route GET /r {
    return <main><p>from return</p></main>
}
route GET /early {
    if request.query.x == "1" { return <p>early</p> }
    <p>late</p>
}
```

```quillo
// P3: a component with return, in Chrome (clicked 4 times)
fn twice(n) {
    return n * 2
}
component Counter() {
    state n = 0
    let shown = (k) => { return `k=${twice(k)}` }
    if n > 2 { return <p id="done">done {n}</p> }
    return <button id="b" @click={() => { n += 1 }}>{shown(n)}</button>
}
route GET / {
    return <main><Counter/></main>
}
```

```quillo
// P4: what the checker sees
fn after() {
    return 1
    print("never")
}
fn some_paths(c: bool) -> int {
    if c { return 1 }
}
fn ends_let() -> int {
    let x = 1
}
fn both(c: bool) {
    if c { return 1 }
    "two"
}
```

Counts: `python3 -I feedback/tools/fn-forms.py app book feedback/repros ../quillo-website ../quillo-starter ../quillo-components ../quillo-examples ../quillo-devtools`.

## Found while fixing (branch feedback52, 2026-11-13)

Found by the four reviews of the fixes for Q-677 to Q-680. All three are on main too. Q-945 is fixed on branch feedback52 in c10c0f1, as Q-677 made it reach every runaway recursion in a function with `return`. Q-944 and Q-946 are not: each fix tried for Q-944 brought a new slowdown or missed a case, so the branch keeps main's behaviour.

### Q-944. Compiled code keeps looping over the builtin `range` after a `let` makes `range` something else

| Kind | Detected by | Attempts |
|---|---|---|
| bug | reviews of 6530e15 and f6598ef | 3 |

A function that loops over `range(..)` is compiled with the builtin's loop. When a `let` later makes `range` something else, the compiled code still runs the builtin's loop: a `let range` at the top level or in an enclosing function, or a caller compiled after its helper.

```quillo
fn g(n) { let s = 0; for i in range(n) { s += i }; s }
fn f(n) { g(n) + 1 }
print(f(4)); print(f(4))
let range = (n) => [100, 200]
print(f(4))   // compiled: 7; QUILLO_JIT=0: 301
```

`quillo check` says nothing about this program. It refuses `range = ...`, an assignment to a builtin, but that too is not followed under `--no-check`. On branch feedback52, a parameter, local or `for` variable named `range` is no longer read as the builtin (f6598ef), as that is decided when compiling.

Tried and dropped:
- A dep on `range` checked at each call (6530e15) took functions off the quick call path: mapping one over 1M items took 2.7 times the instructions.
- A hook on `let` (f6598ef) missed new bindings in an enclosing scope and callers compiled later, and cost re-`let` loops 0.35% to 0.56%.

What it likely needs: deciding when compiling which lexical scopes can still bind `range`, through the enclosing function's body or a flag the parser sets, with callers inheriting what their compiled callees read.

| Impact | |
|---|---|
| **Functionality** | A wrong value, silently, in a program that shadows `range` after a function using it has run twice |
| **User** | Rare: shadowing a builtin, after the fact; a `range` parameter (common, "a date range") is fixed |
| **Requests** | No change |
| **Security** | No change |

### Q-945. A recursion that goes past the call limit is up to 30 times slower compiled than interpreted

| Kind | Detected by | Attempts |
|---|---|---|
| speed | review of 01767d5 | 4 |

Compiled code gives up at the call limit and the interpreter takes over that call; each level it interprets calls back into machine code, which runs to the limit again. A 10,000-deep recursion that ends in the "too deep" error took 1.35 s compiled against 0.06 s interpreted. Programs that stay under the limit are not affected.

Tried and dropped (c928896, 6530e15, f6598ef):
- A floor for the thread was taken back by a helper called at the same depth.
- A floor for each function was taken back by a self-call at the same depth.
- A fixed floor with a countdown kept normal calls interpreted for 40,000 calls after the error, 2.3 times slower.
- Each was also slower than the interpreter with hundreds of self-calls a level.

Fixed in c10c0f1: a floor fixed at the depth of the outermost call that went too deep, taken off by any call at that depth or above, behind one load at the top of every call that also does what `enabled()` did, so normal calls pay nothing. Every shape the reviews found, and `fact(-1)`, fail in at most 1.11 times the interpreter's instructions.

| Impact | |
|---|---|
| **Functionality** | No change: the same error, the same values |
| **User** | A runaway recursion takes seconds, not milliseconds, to fail |
| **Requests** | A request that recurses too deep holds its thread longer before its 500 |
| **Security** | A visitor who can make a handler recurse without bound costs the server more time a request |

### Q-946. A loop at a script's top level that calls a function is slower than Python

| Kind | Detected by | Attempts |
|---|---|---|
| speed | slice A's measurements (01767d5) | 0 |

`jitloop` compiles no top-level loop that calls a function, so the loop runs in the interpreter. The function itself is compiled, but each call goes through the interpreter's call path. The report's 5M-call loop, written at a script's top level, took 0.77 s to 0.93 s in every function form, against Python's 0.64 s to 0.73 s; inside a function, it took 0.02 s. bench/vs-python measures loops inside functions, so it does not show this.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A first script, a loop at the top level as in Python, runs slower than Python |
| **Requests** | No change: handlers are functions |
| **Security** | No change |

### Q-947. A compiled frame can be larger than the 512 bytes the call limit assumes, and the process dies of a stack overflow

| Kind | Detected by | Attempts |
|---|---|---|
| crash | review 6 of 3aec2ec (on main too) | 0 |

`run` lets machine code go `left / 512` calls deep, taking 512 bytes as the most a compiled frame needs. A recursive function with many float `let`s still needed after its own call has bigger frames. With 56 such lets, `print(parallel(() => big(9000, 1.5), () => 1))` aborts with "stack overflow" on main, where `QUILLO_JIT=0` prints `[336.0006, 1]`. With 34, a `big(2000, 1.5)` that ends, in a route's `parallel` child, kills the server with SIGBUS. With 30, a runaway `big(-1, 1.5)` aborts main; c10c0f1's floor saves that one. 3aec2ec's rerun sizes its stack the same way (`limit * 512`). Review 7 found the quick path for a repeated call (`LAST`) still takes 512 bytes, with no reserve: `h(a)`, calling nothing and keeping 10,000 floats live, called at each level of a 9,000-deep `walk`, aborts on main and on 38feed9 where `QUILLO_JIT=0` prints `269959500.0`.

| Impact | |
|---|---|
| **Functionality** | A correct program dies where the interpreter gives its value |
| **User** | The process ends with "stack overflow" or SIGBUS, no Quillo error, no line |
| **Requests** | One request reaching such a function kills the server and every request on it |
| **Security** | A visitor who can choose the depth (a number in the path) can take the server down |
