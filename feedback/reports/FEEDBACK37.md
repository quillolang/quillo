# Feedback from asking whether Quillo can take a CSS engine written in Rust: `.rs` files, building at load beside the program, and a file's `style` for its own components

| | |
|---|---|
| App | None. The question: if Tailwind, or another CSS engine, ships as a Rust crate, can a Quillo program use it with no Node? That needs two things. First, Rust files under their own extension, `.rs`. Second, the stylesheet built as the program loads and alongside it: not when `GET /app.css` first comes in, and not before the port opens. Third, a `style` block outside the components, at a file's top level, that styles only the components that file declares |
| Quillo | 0.6.0 (23146e4, 2026-11-02), built from `git archive 23146e4` in a scratch folder. The checkout is on another session's branch, `feedback27-35-fixes`, with uncommitted changes. Q-452 was also checked on that branch's binary (built 10:31 today, which carries Q-379's fix) |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` and `quillo run` on small programs; curl polled every 0.25 s across a start and a reload; an empty `QUILLO_NATIVE_CACHE` for cold builds; signals sent to the server's own PID; reading [src/native.rs](../../src/native.rs), [src/parser.rs](../../src/parser.rs) (`use` paths), [src/modules.rs](../../src/modules.rs), [src/styles.rs](../../src/styles.rs), [src/builtins.rs](../../src/builtins.rs) (`style_with`), [src/main.rs](../../src/main.rs) (`stop_on_signal_with`) and USERGUIDE.md (Rust, `parallel`, the HTML literal's `style`) |

No Quillo source was changed. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched, as another session is working in the checkout; each repro is in its own section. The user asked for this as FEEDBACK36.md, but that file already holds another session's report on HTTP (Q-432 to Q-451), so this is 37. Numbers start at Q-452. Line numbers are 23146e4's.

What worked well: a `.qwer` file is already Rust with its crates in a `//! [dependencies]` header. A CSS engine published as a crate would be a plugin of a few dozen lines, as `sass.qwer` (grass) is today. `quillo check` reads a `.qwer` file's types without building it. Once the libraries are built, a start is fast: 0.06 s to the first answer with two `.qwer` files. A reload is not held up by slow load-time work: the old version answered every request in under 1 ms while the new one spent 3 s at its top level. `parallel` at the top level overlaps two 1.5 s waits in 1,510 ms.

Fix Q-452 first: a server whose stderr goes into a pipe that closed cannot be stopped by a signal, only killed. Then Q-454, the user's main ask: on a first start nothing answers until every `.qwer` build and every piece of load-time work is done, one after another. Q-453 is the `.rs` extension itself. Q-457 is the file's `style`, which the user asked for after the rest.

## The answer

### Where a Rust Tailwind stands

Tailwind 4's engine, Oxide, is mostly TypeScript with Rust for the costly parts (the scanner, in the repository's `crates/oxide`), and it uses Lightning CSS, which is Rust, for parsing, nesting and prefixes. No official Tailwind crate on crates.io was found; its Rust parts ship inside the npm packages. A third-party crate, `ironframe`, describes itself as a Tailwind 4.1-compatible engine in Rust, and `lightningcss` is on crates.io. Either could be wrapped as a `.qwer` today. Not tried here: building either would take more disk space than this machine has free.

### What a program can do today, and what it would need

| Step | Today | Issue |
|---|---|---|
| Rust in a program | yes, as `.qwer`; `use "x.rs"` reads the file as Quillo code | Q-453 |
| Build the Rust before starting | `quillo build app.qwe`; otherwise `use` builds each file when its statement runs, one at a time | Q-454 |
| Compile the stylesheet at load, not on request | yes: `let APP = engine(...)` at the top level runs once, at load | |
| ... alongside the rest of the load, with the port open | no: the port opens after the whole top level; `parallel` overlaps work with other work, not with the port | Q-454 |
| ... during a reload | yes: the old version answers until the new one is ready | |
| Run components' `style` through the engine | only through a shell command (`style_with("npx postcss")`), not a function | Q-455 |
| Give the engine the classes the program writes | no: a tool reads the source as text | Q-456 |
| One `style` for all the components of a file | no: `style` outside a component is an error; a CSS value shared by `style X` in each component is copied once per component | Q-457 |

### The design asked for

```quillo
use "tailwind.rs"                                   // Rust under its own extension (Q-453)

build "/app.css" = tailwind(css`@import "tailwindcss";`, classes())   // at load, beside the rest (Q-454, Q-456)
style_with(tailwind_apply)                          // components' `style` through the same engine (Q-455)
```

```quillo
// lib/cards.qwe
style css`.title { color: darkred }`               // styles Card and Badge, and nothing else (Q-457)
component Card(t) { <div.card><h2.title>{t}</h2></div> }
component Badge(t) { <span.title>{t}</span> }
```

1. **Builds start at link time.** The linker already reads every file before the first statement runs (0.04 s for the 6,750 lines of app/stays). Every Rust file it finds starts building there, all in one cargo call (a workspace), so cargo's own parallel builds cover them all. Each `use` then waits only for its own library.
2. **A declared build runs beside the top level.** `build "/app.css" = expr` starts as soon as the functions it calls are loaded, off the lock that Quillo code holds. The port opens without waiting for it. A request for `/app.css` that arrives first waits for the build: it is not refused and gets no 404. Pages link the result under a name that holds its hash. `--watch` runs the build again when a file changes, before the open pages swap their sheet. `quillo build` runs it once and writes the result, so a production start begins with it done. If the build fails, the first start stops with the error, as `style_with` does; under `--watch`, the old sheet stays.
3. **Ordinary top-level code keeps holding the port.** A route may need the state that code sets. Only what is declared as a build moves off that path.
4. **A file's `style` is scoped to the file's components.** The rule, as the user gave it: only the components declared in that `.qwe` file are styled by it. See Q-457 for what follows from it.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-079 (FEEDBACK11) | Fixed with steps 1 and 2: `--with` and `style_with`. A CSS step written in Rust inside the program, which would be step 3, is Q-454 to Q-456 here | 0 (reading) |
| Q-379 (FEEDBACK31) | A closed pipe: its fix on `feedback27-35-fixes` covers `print` to stdout, not Quillo's own messages to stderr. Q-452 is what that leaves | 1 |

## New issues

### Q-452. A server whose stderr goes into a pipe that has closed cannot be stopped by SIGTERM or Ctrl-C, only killed (found on the way)

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the shell | 2 |

```sh
quillo run main.qwe --port 18441 2>&1 | head -1 &      # head exits after the "listening" line
kill -TERM <the server's pid>      # nothing
kill -TERM <the server's pid>      # nothing; "again to stop now" never happens
# still listening 14 s later; only SIGKILL ends it
```

The same happens on `feedback27-35-fixes`'s binary. The handler in `stop_on_signal_with` ([src/main.rs:753](../../src/main.rs)) first sets `again`, then writes `quillo: stopping once the requests in progress are answered` with `eprintln!`. On a closed stderr that write panics, and the panic ends the handler's thread before `httpd::stop()` and before the timer that ends a slow drain. Every later signal finds no handler, so the server never stops and never says so. In practice that is a server run through a log shipper or `tee` that stopped: systemd waits out `TimeoutStopSec` and then kills it, and a deploy script that waits for the port to close hangs.

**Suggestion:** Quillo's own messages should not panic: write them with `writeln!` to stderr and drop the error (one `note()` helper beside Q-379's `print_line`). In the handler, call `httpd::stop()` and start the timer before printing anything.

### Q-453. `use "x.rs"` reads a Rust file as Quillo code: "expected end of statement, found `fn`" and a note that the code may be for a newer Quillo. Nothing says Rust goes in `.qwer`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

```quillo
// tw.rs:  pub fn utilities(classes: Vec<String>) -> String { ... }
use "tw.rs"                          // ./tw.rs:2: error: expected end of statement, found `fn`
                                     // note: ... if the code was written for a newer Quillo, that would explain it
use tw.rs                            // error: cannot read ./tw.rs.qwe
import "tw.rs" use utilities         // the same as the first
use "tw.qwer"                        // the same file under .qwer: ok
```

The parser adds `.qwe` to any path that does not end in `.qwe`, `.qwer`, `.js` or `.mjs` ([src/parser.rs:466](../../src/parser.rs)), so a bare `tw.rs` becomes `tw.rs.qwe`. A quoted one is read as Quillo code. The note about a newer Quillo sends the reader the wrong way.

`.rs` is worth having for more than the error. Editors, GitHub, `rustfmt` and clippy know `.rs` and not `.qwer`. A `.qwer` file gets the Quillo extension's own grammar, plus `cargo check` on save from the language server, but no completion or go-to-definition in its crates' items. The `//! [dependencies]` header is a valid inner doc comment, so a `.qwer` file is already a valid `.rs` file.

**Suggestion:** take `.rs` as a second extension for `.qwer`. This touches `use` and `import` paths, `quillo build` (`build_all` checks for `"qwer"`, [src/native.rs:1040](../../src/native.rs)), the language server's `cargo check` on save, `--watch`, and the file stem in the cache. Keep `.qwer` for the catalog's 628 plugins and `quillo install`. A `.rs` file becomes a plugin only through a `use`, so a Cargo crate's `src/*.rs` next to the program is left alone. For editors, `quillo build` could write a `rust-project.json` that points at the crates Quillo makes in its cache, so rust-analyzer resolves the file's crates (not tried). Even without the alias, the error should say: "a Rust file is used as `.qwer`".

### Q-454. On a first start the port opens only after the whole top level has run: every `.qwer` build, one after another, then every piece of load-time work. Until then a connection is refused

| Kind | Detected by | Attempts |
|---|---|---|
| perf / limitation | curl polled across a start | 2 |

```quillo
// order/main.qwe: 3 s of work at load, standing in for compiling a stylesheet
sleep(3000)
let APP = ".p-4{padding:1rem}"
route GET /app.css = respond(200, APP, { "content-type": "text/css" })
// curl every 0.25 s: connection refused (000) until 3.07 s, then 200
```

```quillo
// two/main.qwe, with an empty QUILLO_NATIVE_CACHE
use "a.qwer"          // 0.0 s  quillo: building ./a.qwer
use "b.qwer"          // 3.3 s  quillo: building ./b.qwer
route GET / = a() + b()
                      // 4.2 s  quillo: 1 route listening
```

Each `use` builds its file when the statement runs, after the program is parsed and linked. `build` holds a lock so that only one build runs per process (`BUILDING`, [src/native.rs:817](../../src/native.rs)), and each file is a cargo call of its own. Two files with no crates took 3.3 s and 0.9 s, one after the other. A crate the size of grass or lightningcss takes far longer on its first build (not measured here). Once both libraries were built, the start took 0.06 s. Parsing is not the cost: `quillo check` of 6,750 lines takes 0.04 s.

The cost lands at a cold start: a deploy without `quillo build`, a new machine, or a cache that was cleared. Behind a proxy such as Limer, the refused connections reach visitors as 502s for as long as the build takes. `parallel` at the top level overlaps load-time tasks with each other (1,510 ms for two 1.5 s waits) but still holds the port. A reload is fine: see "Edge cases".

**Suggestion:** the design above. Start every Rust build at link time, in one cargo call over a workspace. Add a declared `build "/path" = expr` that runs off the lock beside the top level, with requests for it waiting rather than being refused, `--watch` rerunning it, and `quillo build` running it ahead of time. Leave ordinary top-level code where it is.

### Q-455. `style_with` takes only a shell command, so a CSS engine in a `.qwer` file cannot process components' `style`

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` | 1 |

```quillo
fn process(css) { replace(css, "@apply p-4;", "padding: 1rem;") }
style_with(process)          // error: argument 1 of `style_with` must be str, got fn
```

The refusal is clear, in `quillo check` and at run time alike. The cost is that `@apply` and `theme()` in a component's `style` need Node and PostCSS even when the engine is in the program as Rust. `made_with` ([src/styles.rs:134](../../src/styles.rs)) already marks each style with `/*! quillo-style <id> */`, sends them all through at once and caches the output by content. A function would fit in the same place.

**Suggestion:** `style_with(f)` with a function, Quillo or `.qwer`, called once with all the marked styles and expected to return them with the marks. The same cache and the same rule apply: a failure stops the load. Text stays a shell command.

### Q-456. Quillo knows every class the program writes, but cannot hand the list to a CSS tool: a scanner reads the source as text, and a Rust engine would have nothing to read

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the guide and the builtins | 0 |

The guide says Tailwind's scanner reads `<div.flex.p-4.md:flex-row>`, but takes `<i.w-1/3/>` with the slash, so a space is needed before `/>`. An engine called from the program needs the list from somewhere. The parser has it exactly: the `.class` shorthand and `class="..."` attributes written out, and the strings in `class={...}`. No builtin or command gives it out.

**Suggestion:** `classes()` at the top level, giving the classes the program's files write, as a list; and `quillo classes app.qwe` for a tool beside the program (Tailwind's `@source inline(...)` or a safelist file). As in the guide, classes made from parts (`` `bg-${color}` ``) are in neither. Say so where `classes()` is documented.

### Q-457. A `style` outside a component is an error, so the components of one file cannot share scoped CSS: the only way is a CSS value given to `style` in each component, which copies the rules once per component

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` | 3 |

```quillo
// lib/cards.qwe
style css`.title { color: darkred }`      // error: `style` belongs in the body of a component or a view
component Card(t) { <div.card><h2.title>{t}</h2></div> }
component Badge(t) { <span.title>{t}</span> }
```

`style global` at the top level is refused the same way. What works today is a `let` that each component names:

```quillo
let CARDS_CSS = css`
  .title { color: darkred }
  .title:hover { text-decoration: underline }
`
component Card(t) { style CARDS_CSS  <div.card><h2.title>{t}</h2></div> }
component Badge(t) { style CARDS_CSS  <span.title>{t}</span> }
// the page's sheet: the rules twice, once per component
//   .title[data-q-1839103e] { color: darkred }   .title[data-q-1839103e]:hover { ... }
//   .title[data-q-608dbc19] { color: darkred }   .title[data-q-608dbc19]:hover { ... }
```

The scoping is right: another file's `<h2.title>` and the page's own are left alone. But a file of 20 components that share 200 lines of CSS sends 4,000 lines, and a component that forgets its `style CARDS_CSS` line gets none of it without a word. A component can still add its own CSS (`style CARDS_CSS` and then `` style css`...` ``, or `` style CARDS_CSS + css`...` ``), and both are scoped.

**Suggestion,** following the user's rule that only the components inside the `.qwe` file get a file's style:

1. **What it styles.** A `style` at a file's top level styles the elements written by the components declared in that file, including markup they pass to components of other files, as a component's own style does today. Nothing else in the file gets it: not a route's markup, not a view's, and not a top-level `fn`'s, even when a component of the file calls that `fn` (such markup carries no mark today either: `<em class="title">`). Components of other files and the page are left alone, and so are children that another file's code passes into one of this file's components.
2. **The mark.** Each element of the file's components carries one more attribute, `data-q-f<id>`, with the id hashed from the file's path relative to the program's folder, so it is the same on the server, in the browser, and on every machine. Each rule is made to need it: `.title[data-q-f1a2b3c4]`. The rules go out once per file, not once per component.
3. **Order.** A file's style comes before its components' own. Both have the same specificity (a class plus an attribute), so a component's own rule wins over the file's.
4. **The rest is a component's style.** `:global(...)`, `@media`, `@supports`, `@container` and `@layer` are scoped inside; `@keyframes` and `@font-face` are kept as they are. It goes through `style_with` (Q-455). It is sent only to pages that show one of the file's components, in the same hashed sheet. Several top-level `style`s in a file are joined in order. `style global` at the top level does the same, unscoped, which replaces companions repeating `style global tabs_css`.
5. **Imports.** `import cards use Card` takes Card with the file's style: the style belongs to the file, not to a name.
6. **Errors.** In `quillo check` and at load: a top-level `style` in a file that declares no component ("a file's `style` styles its components, and this file has none"), and one after the first component, so the rules are written where they are read.

Edge tests for the build: the page's `.title`; another file's component inside one of this file's; one of this file's inside another file's; children passed in from another file; a top-level `fn`'s markup; a file `use`d twice; the same file mounted in two programs; `--watch` changing only the file's style; a server-rendered page compared with the browser's re-render (no component remade, `__quilloRemade` empty).

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| In the chat: "this means we need `.rs`" was taken as a need. The check showed `.qwer` is already Rust, so `.rs` is an alias, for editors | the user's framing | Q-453 written as an alias with an editor case, not as missing Rust support | 1 read |
| `now_ms()` to time the `parallel` probe | other languages' habit | `unknown name now_ms`; `now()` is in milliseconds | 1 run |
| Probe harness: `quillo run ... 2>&1 \| while read l; do ...; break; done` | to stop reading at "listening" | the server outlived the loop and ignored two SIGTERMs: that is how Q-452 was found | 1 SIGKILL of my own PID |
| Wrote the report as FEEDBACK36.md, as asked | the user named the number | the file holds another session's report; written as 37 instead | 1 `ls` |

## Limitations

- The engines themselves (`ironframe`, `lightningcss`) were not built as `.qwer` plugins: the disk is 96% full, and the first build of a crate that size is the cost Q-454 describes.
- Whether rust-analyzer resolves a `.rs` plugin's crates through a `rust-project.json` pointing at Quillo's cache was not tried.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `--reload` with 3 s of work at the new version's top level | every request during the reload answered by the old version in under 1 ms; the new version answered from 3.09 s |
| `parallel(() => { sleep(1500); ... }, () => { sleep(1500); ... })` at the top level | 1,510 ms in all, not 3,000 |
| A start with both `.qwer` libraries already in `.quillo/native/` | 0.06 s to the first answer, with no cargo run |
| `quillo check` of a program that uses a `.qwer` file | `ok (1 routes, 1 functions, 0 components, in 2 files)`, with nothing built |
| `quillo check` of app/stays (6,750 lines in its `.qwe` files) | 0.04 s |
| `style_with(a_function)` | refused by `quillo check` and at run time, naming the type it got |
| `let APP = ...` at the top level | runs once, at load: a request for `/app.css` does not compile anything |
| One `let` CSS value given to `style` in two components of a file | each scoped to its own component; another file's `.title` and the page's are not styled |
| `style SHARED` and `` style css`...` `` in one component, or `` style SHARED + css`...` `` | both accepted, both scoped to the component |

Sources for the Tailwind part: [Tailwind CSS 4.0 released with Rust-powered build (DevClass)](https://devclass.com/2025/01/24/tailwind-css-4-0-released-with-ground-up-rewrite-for-faster-rust-powered-build), [tailwindcss v4.1.15 source tree](https://upd.dev/tailwindlabs/tailwindcss/src/v4.1.15), [ironframe on docs.rs](https://docs.rs/crate/ironframe/latest/features).
