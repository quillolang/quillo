# Feedback from reviewing a proposal: the page's JavaScript written as `js.name`, every name declared by a `client` line

| | |
|---|---|
| App | None: a design review of a proposal. `js("path.to.fn", args...)`, `js_new`, `js_has` and a `use` library's bare name (`Chart.new(...)`) give way to one form: `js.path.to.fn(args...)`. Each root it starts from is declared at the top of the file by a `client` line. The browser's own names are declared too (`client use localStorage, navigator`), so the editor can suggest exactly the names the file declares after `js.` |
| Quillo | 0.6.0, release build of the runtime-coverage branch's working tree (it reports `54dd1bd`). `src/lexer.rs`, `src/client.rs`, `src/lsp.rs` and `src/client/runtime.js` carry the branch's uncommitted work. None of it is in the parts read here except `B.js`, which is the same as on main |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` on probe programs, and `quillo lsp` driven by a Python 3 script over JSON-RPC (completion requests), in the session's scratch folder. No browser run. Read: [src/lexer.rs](../../src/lexer.rs) (`next`, `lex_template`), [src/client.rs](../../src/client.rs) (`js_path_problem`, `lib_path`, `lib_call`, `js_name`, the browser's builtins list), [src/check.rs](../../src/check.rs) (the builtins' types, the page-`let` warning, the literal-name check), [src/island.rs](../../src/island.rs) (`BY_ITSELF`), [src/lsp.rs](../../src/lsp.rs) (`completion`), [src/builtins.rs](../../src/builtins.rs), [src/builtin_docs.rs](../../src/builtin_docs.rs), [src/client/runtime.js](../../src/client/runtime.js) (`jsPath`, `B.js`, `B.js_value`, `B.js_new`, `B.js_has`), USERGUIDE.md (Browser state, JavaScript libraries) |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section, and the probe programs are at the end. Numbers start at Q-498, after FEEDBACK43.md's Q-497.

Decided in the discussion that led to this report:

- Calls into JavaScript carry a visible marker, `js.`. Bare names are not used: a developer must not mistake JavaScript for Quillo.
- Declarations live outside the component, at the top of the file, the way a file's `style` does. There is no import inside a component.
- Completion after `js.` is fed by the file's `client` lines.

What works well today, and should be kept: a library named with `use` is already written as a path, not a string. `client "chart.js" use Chart` makes `Chart.new(el, cfg)` a `js_new` call, `L.map(el)` a `js` call, and `Chart.defaults` a read of the value (`lib_path` and `lib_call`, `client.rs:3556-3590`). It goes through the same checks: `js_name` for the shape of the path, and `js_path_problem` for `eval`, `Function`, `Reflect.set` and the rest. A name the page lacks gives an error value, not a crash. A Promise is waited for where code can wait. The proposal reuses all of it. The runtime does not change.

## The answer

```quillo
client use localStorage, navigator                  // the browser's own names
client /assets/chart.js use Chart                   // a library: loaded, and named

component Theme() {
    state dark = false
    <button @click={() => {
        dark = !dark
        js.localStorage.setItem("theme", dark ? "dark" : "light")
    }}>{dark ? "Dark" : "Light"}</button>
}

component Sales(points: list) {
    state chart = null
    <canvas @mount={(el) => {
        chart = js.Chart.new(el, { type: "line", data: points })
        () => chart.destroy()
    }}></canvas>
}
```

1. **`js.` marks every line that crosses into JavaScript.** JavaScript and Quillo follow different rules (Part B). A call that reads like Quillo but is JavaScript is where those rules surprise. Today a `use` library's `L.map(el)` already reads exactly like a Quillo call. The marker is visible in a review, a diff or a terminal, with no editor needed. A plain highlighter (tree-sitter, Sublime, Vim) can colour it without resolving anything.
2. **The path is not a string.** `js.a.b(x)` cannot hold a variable, so the "write the name out" rule disappears (Q-498). Libraries and the browser's globals are written the same way (Q-499). `js.Date.now` is the function and `js.Date.now()` its result (Q-500).
3. **Each root is declared at the top of the file.** `client <script> use Name` declares what it does today. `client use localStorage, navigator`, with no script, declares names the browser already has. `js.x` with an undeclared `x` is an error, with the line to add. The language server can offer that line as a quick fix. The top of a file then lists every JavaScript name the file touches, which `quillo check`, policy.toml (Q-478) and a reviewer can all read.
4. **Completion follows the declarations.** After `js.`, the editor offers the file's declared names and nothing else. After `js.localStorage.`, it offers that API's members. After `js.Chart.`, it offers what a local `.mjs` exports. Today it offers none of these (Q-502).
5. **`js("...")`, `js_new`, `js_has` and bare library names warn, then become errors.** `quillo fix` rewrites them, since every path is a literal by rule.
6. **Fix Q-501 first.** `quillo check`'s warning for a page value in a component's `let` looks for calls named `js`/`js_new`. It already misses the `use` library form, so `js.` built on that path would miss it too.

Part A lists every place that knows `js` today. Part B covers why a marker is needed and the forms considered. Part C is the design, Part D the migration. The issues follow, then an order of work and the questions only you can answer.

## Part A: where `js` is known today

| Concern | Where | Keyed on |
|---|---|---|
| Lexing | `lexer.rs:331`: identifiers start with a letter or `_` | `js` is an ordinary identifier |
| Types for the checker | `check.rs:539` (`js_has`), `check.rs:656-657` (`js`, `js_new`) | the name |
| Name written out, its shape, refused paths | `check.rs:3690-3700`; `client.rs:3619` (`js_name`); `client.rs:102` (`js_path_problem`) | `Ident("js" \| "js_new")`, not shadowed by a local |
| "A `let` set from the page" warning | `check.rs:2562` | `Expr::Call` of `Ident("js" \| "js_new")` only (Q-501) |
| A component that runs by itself (island) | `island.rs:38` (`BY_ITSELF`) | the names `js`, `js_new` |
| Builtins the browser has | `client.rs:46-48` | the names |
| Library form: `Chart.new(...)`, `L.map(...)`, `Chart.defaults` | `client.rs:3556` (`lib_path`), `client.rs:3575` (`lib_call`) | a root named by `use` |
| Server: null | `builtins.rs:135-136`, `builtins.rs:17` (`js_has`) | the names |
| The runtime | `runtime.js:1778` (`jsPath`), `:1808` (`B.js`), `:1820` (`B.js_value`), `:1825` (`B.js_new`), `:1280` (`B.js_has`) | a path string, from the compiler |
| Completion | `lsp.rs:1446`: after `x.`, only a builtin module's functions (`db.`, `udp.`); otherwise every name | no `client` names at all (Q-502) |
| Hover | `builtin_docs.rs:66, 146, 147` | the names |
| Guide, book, editors' grammars | USERGUIDE.md, book/, ~/dev/quillo-editors | the text `js(` |

`js.a.b(x)` compiles to `B.js("a.b", x)`, `js.a.b` to `B.js_value("a.b")` and `js.A.new(x)` to `B.js_new("A", x)`. All of these exist. Everything that changes is in the parser, the compiler, the checker and the language server.

## Part B: why a marker, and the forms considered

What changes when a value comes from the page. A developer who cannot tell the two apart meets these as bugs:

| JavaScript (the page) | Quillo |
|---|---|
| Null on the server, so a component's first render differs from the browser's | The same on both sides |
| A Promise, waited for in handlers, `@mount`, `state` and the component's `let` functions; elsewhere it stays a Promise | Plain values |
| `selectionStart` and string offsets count UTF-16 units | `len`, `slice`, `s[i]` count characters |
| An object's fields can be set (`el.value = ""`) | A map is made anew (`{ ...m, x: 1 }`) |
| Throws; a missing name is an error value | Error values |
| Its checks are a guard-rail, not a security boundary | Checked by the language |

The forms considered:

| | `js("a.b", x)` (today) | `$("a.b", x)` | `$.a.b(x)` | bare `a.b(x)` from `client use a` | **`js.a.b(x)`** |
|---|---|---|---|---|---|
| JavaScript visible at the call | yes | yes | yes | **no** | yes |
| Path is a string | yes | yes | no | no | no |
| One form for libraries and globals | no | no | yes | yes | yes |
| Function as a value (`Date.now`) | no (Q-500) | no | yes | yes | yes |
| Easy to grep and look up | yes | no | no | n/a | yes |
| Reads as jQuery | no | yes | somewhat | no | no |
| Completion from the declarations | no | no | possible | possible | yes |

`js.` keeps today's word, so the move from `js("localStorage.setItem", k)` to `js.localStorage.setItem(k)` is one a reader recognizes and `quillo fix` can make. `js.localStorage...` is free today: it fails with `fn has no field \`localStorage\`` (probe P5).

## Part C: the design

### C1. Declarations

- `client <script> use Name, Other as Alias` (and `module`, `preload`, `preconnect`) is unchanged. It loads the script where a component that uses it is on the page, and declares the names.
- `client use name, other.path as alias` with no script declares names the browser already has. It loads nothing. A dotted name declares its last part (`client use navigator.clipboard` declares `clipboard`), or the alias.
- `window`, `globalThis`, `self`, `top`, `parent` and `frames` are refused as roots. Each holds every other name, so declaring one would make the list say nothing. The error says what to declare instead: `innerWidth`, `matchMedia`, `scrollTo` are globals themselves. `document` is allowed. The refused paths (`document.write`, `eval`, `setTimeout` and the rest) are still refused wherever they appear.
- Declarations are file-level, like every other import. A file that uses `js.Chart` declares `Chart`, and another file declares it again. The page still loads the script once. There is no declaration inside a component (decided).
- A name declared and never used warns, as an unused `import` does.
- A declared name must not clash with another import in the file. It never clashes with Quillo's own names, since it is reached only through `js.`.

### C2. Use

| Written | Compiles to | Note |
|---|---|---|
| `js.localStorage.setItem(k, v)` | `B.js("localStorage.setItem", k, v)` | `this` is `localStorage`, as now (`jsPath`'s `owner`) |
| `js.innerWidth` | `B.js_value("innerWidth")` | a read, never a call |
| `js.Date.now()` / `js.Date.now` | `B.js("Date.now")` / `B.js_value("Date.now")` | a call / the function itself (Q-500) |
| `js.Chart.new(el, cfg)` | `B.js_new("Chart", el, cfg)` | `.new`, as for libraries today |
| `js.document.querySelector("#x").focus()` | `B.js(...)`, then a method of the page's object | as `js(...).focus()` works now |
| `js.Chart` with `client module ... use Chart` | the module's `Chart`, ahead of the page's | the same precedence as now (`jsPath`: `MODS` first) |
| `chart.update()`, `el.value = ""` | unchanged | an object the page gave is used as now: the marker is on the way in, not on every method |

- The parser makes `js.` the root of a path (`Expr::Page`, or `lib_path` given `js` as a root), never a field of the builtin.
- `js` alone, `js[...]` and `js.x` with `x` undeclared are errors that say what to write. During the warning period, `js(...)` and `js_new(...)` keep working.
- `js` becomes a reserved word once the old forms are errors. Until then, a local named `js` warns. Today it shadows the builtin (`!self.is_local(n)`), and with `js.` it would silently mean the local's field.
- On the server every `js.` path gives null, and `@mount` does nothing, as now.
- Missing names keep the runtime's error value, `` `js.L.map` is not defined on this page (did you mean ...?) ``, with the name as written.

### C3. The checker

- `js.` paths go to `js_name` and `js_path_problem`, as `js("...")` does now. What `quillo run` refuses, `quillo check` refuses.
- The page-`let` warning and `BY_ITSELF` recognize `js.` paths (Q-501).
- `js_has` gives way to `is_err(js.navigator.vibrate)`. The runtime already returns an error value for a missing name, so this needs no new form (Question 2).
- The old forms warn: `` `js("a.b", x)` is now written `js.a.b(x)`, with `client use a` at the top ``, and `` `Chart.new(...)` is now written `js.Chart.new(...)` ``. They become errors one release later, as with modules (Q-154 to Q-156).

### C4. The editor

- **Completion after `js.`:** the file's declared names, each with its source as detail (`from /assets/chart.js`, `the browser's`). Only those names: an undeclared one is not offered.
- **Completion after `client use `:** the browser's global names from a table, without the refused roots or refused paths.
- **Completion after `js.<global>.`:** that API's members from the same table (`getItem`, `setItem`, `removeItem`, `clear`, `key`, `length` for `localStorage`). After `js.<library>.`: a local `.mjs`'s `export`s, read the way `quillo check` reads the file. Nothing for an address or a UMD script, unless a `.d.ts` sits beside it (later).
- **Quick fix:** for `js.x` with `x` undeclared, add `client use x` to the file's `client` lines.
- **Hover on `js.`:** "the page's JavaScript: runs in the browser, null on the server", and the line that declares the name.
- **Grammars:** tree-sitter, VS Code, Zed, Neovim, Helix, Sublime, Emacs, Vim and JetBrains colour `js.` and the path after it as one foreign path. No resolution is needed.

The table of the browser's names is the one piece of new data. It can be a small curated list kept in Quillo (the common globals and their members), or generated at build time from TypeScript's `lib.dom.d.ts` or MDN's browser-compat-data (Question 3).

## Part D: migration

Counted with `grep -E '\bjs(_new|_has)?\('`. The counts include `js_new` and `js_has`. Bare library calls (`Chart.new`, `L.map`) also change and are not counted here.

| Where | Files | Calls |
|---|---|---|
| this repo's `.qwe`/`.qwer` (tests/browser, app/) | 38 | 161 |
| tests/*.rs (inline programs) | | 63 lines |
| ~/dev/quillo-website (pages, plugins) | 7 | 17 |
| ~/dev/quillo-starter | 2 | 2 |
| ~/dev/quillo-devtools | 0 | 0 |
| USERGUIDE.md, book/ (feedback/ excluded) | | part of 195 |

The kit components on the smart-components branch were not counted. The feedback reports are history and keep `js(`. The guide and the book change in the same commit as the compiler, under the rule to keep the book in sync. The guide's "JavaScript libraries" and "Browser state" sections get one rule: declare with `client`, call with `js.`.

## New issues

### Q-498. `js` takes the page's name as a string, so "write the name out" is a rule to check rather than a form that cannot be wrong

`js(k, "1")` with `k` a variable is refused when the program loads (FEEDBACK18, Q-100), and `quillo check` says so (probe P4):

```
q3.qwe:3: error: `js` needs the name of what it calls written out, like js("Chart", ...)
```

This works, but the error exists only because the form allows what it forbids. The guide has to teach the rule for `js` and for `js_new`, and the checker has to guard against a local named `js` (`check.rs:3693`). `js.a.b(x)` cannot hold a variable. Kind: limitation. Fixed by Part C.

### Q-499. A page library has two spellings: `L.map(el)` when named with `use`, `js("L.map", el)` when loaded by a `<script>` tag

The same global on the page is written two ways, depending on how the page loaded it. Moving a library from a `<script>` tag to `use` (to get module loading, `preload`, or the lock) means rewriting every call. The guide documents both forms in "JavaScript libraries", one after the other. The `use` form is also the one that reads exactly like Quillo. Kind: limitation. Fixed by Part C: `js.L.map(el)` in both cases.

### Q-500. `js("x")` with no arguments calls `x` if it is a function, so a page function cannot be had as a value; the guide says "no arguments reads a value"

`B.js` (`runtime.js:1808`) reads the path, and calls it if it is a function, whatever the number of arguments:

```js
if (typeof v !== "function") { ...; return v === undefined ? null : v; }
...
return fromPage(Reflect.apply(v, owner, guardCall(...)));
```

So `js("Date.now")` is a number. There is no way to hand `alert` or a library's function on to another function as a value, except through a `use` library's name, whose fields read through `B.js_value`. The guide's "`js("window.innerWidth")` (no arguments reads a value)" (USERGUIDE.md, Browser state) is true only for values that are not functions. Until Part C, the guide should say "no arguments reads a value, or calls a function". Kind: docs, limitation. Read, not run in a browser.

### Q-501. `quillo check` does not warn of a page value in a component's `let` when it comes through a `use` library's name

The warning that a `let` set from the page is null while the server renders works for `js(...)`. It is silent for the same value read or called through a library's name (probe P3):

```quillo
client "chart.js" use Chart
component A() {
  let c = js("Chart.defaults.color")    // warning, as it should be
  <p>{c}</p>
}
```

```
q.qwe:4: warning: `c` is a value from the page shown in markup: the server has null for it, so the browser makes <A> again on each load. Write `state c = ...`, which is set once the component is on the page
```

With `let c = Chart.defaults.color` or `let c = Chart.make()`, the same program checks with no warning (`q.qwe: ok (1 routes, 0 functions, 1 components)`). The component is still made again in the browser on each load. The cause is `check.rs:2562`, which matches only `Expr::Call` of `Ident("js" | "js_new")`, not a field path rooted at a `use` name. `island.rs:38` (`BY_ITSELF`) lists the same two names, so a component whose body calls `Chart.make()` is probably not seen as running by itself either (read, not run). Expected: both recognize what `lib_path` resolves, and later `js.` paths. Kind: silent.

### Q-502. The editor knows no `client` names: completion never offers `Chart`, and after `Chart.` it offers every name in the program

The guide says of `client ... use` names: "The names are known to `quillo check` and the editor." `quillo check` knows them. Completion does not (probe P6, `quillo lsp` over JSON-RPC):

```
client "chart.js" use Chart
component A() {
  <p @mount={(el) => { Chart. }}>x</p>
}
```

| Cursor | Items | `Chart` among them | First items |
|---|---|---|---|
| after `Chart.` | 198 | no | `print`, `error`, `run_after`, `page_updated`, `js_has`, `cursor_index`, `args`, `stdin`, ... |
| after `Chart` | 198 | no | the same |
| after `Ch` | 198 | no | the same |

`completion` (`lsp.rs:1446`) handles `x.` only when `x` is a builtin module (`db.`, `udp.`). Otherwise it falls through to the whole list, which is wrong after a dot. The file's `client` names are not in `doc.analysis.symbols`. Expected now: `Chart` is offered with its source as detail, and after a dot that is not a module's, nothing is offered rather than everything. With Part C: C4. Kind: limitation, docs.

## Proposed order of work

1. **Q-501 and Q-502's fall-through, now, on any branch**, with tests and edge tests:
   - Q-501: a field read, a call and `.new` through a `use` name in a `let` shown in markup; in a lambda (no warning); with the name shadowed by a parameter (no warning); `BY_ITSELF` the same way.
   - Q-502: `client` names in completion; nothing offered after an unknown `x.`.
2. **Q-500's guide sentence**, now.
3. **`client use` with no script, and `js.` paths** (C1 to C3) as one slice:
   - the parser, `lib_path` given `js` as a root, the refused roots, the checker, the error messages;
   - browser tests through `tests/browser`;
   - edge tests: `js` alone, `js[`, `js.x` undeclared, a local named `js`, `js` in strings and templates, `js.eval`, `js.window`, `js.setTimeout`, a missing name, a Promise in a handler and after `.then`, `js.X.new`, a module's name ahead of the page's, a `js.` read in a `state` initializer and in a `let` (Q-501), an unused declaration.

   runtime.js coverage should hold at 99.50% by itself, since the runtime does not change. Rust coverage is held as usual.
4. **The editor (C4)**: completion from declarations, the browser's names table, `.mjs` exports, the quick fix, hover, and the grammars in quillo-editors.
5. **The old forms warn**, `quillo fix` rewrites them, and this repo, the website, the starter, the guide and the book are rewritten.
6. **The old forms become errors, and `js` is reserved**, one release later.

## Questions for you

1. **Undeclared browser globals.** Must `js.localStorage` be declared by `client use localStorage` (recommended: completion and the file's top then show exactly what the file uses), or may the browser's common globals be used undeclared, with the declaration needed only for libraries?
2. **`js_has`.** Replace it with `is_err(js.navigator.vibrate)` (recommended, nothing new), or with an optional declaration, `client use navigator.vibrate?`, that gives null when the page lacks it?
3. **The browser's names for completion.** A small curated table kept in Quillo (recommended to start), or one generated at build time from TypeScript's `lib.dom.d.ts` or MDN's browser-compat-data (complete, large, and a build step to keep up)?
4. **Version.** Does this go in 0.7.0 with FEEDBACK40 to 43, or in 0.8?

## Smaller notes

- An object the page gave (`chart`, `el`) is used without `js.`: `chart.update()`, `el.value = ""`. The marker is on the way in. A value's origin is still visible in its name's `let` or `state`, and hover can say "from the page" on it.
- A `js` key in a map literal (`{ js: 1 }`) and a field named `js` on a map (`m.js`) are unaffected: only `js.` at the start of a path, as an expression, is the page. `client.rs` already special-cases `x.js` as not a call of `js`.
- The private runtime registry for ES modules (the rule against putting names on `window`) is not affected. `js.` exists only in Quillo source and compiles to `B.js*`.
- jQuery users write `client /assets/jquery.js use jQuery`, then `js.jQuery("#x").hide()`.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `grep -rn 'js("' --include=*.qwe .` | bash habit | zsh expanded `*.qwe` and stopped on "no matches found"; quoted the glob (as in FEEDBACK42 and 43) | 1 run |
| `component A { ... }` | other frameworks | `expected (, found {`: a component takes `()` even with no props | 1 run |
| `route GET / = render(A())` | other frameworks | `unknown name render`, and `A` is a component: write `<A/>` inside a page's markup | 1 run |
| Completion at column 28 for "after `Chart.`" | counted the line by eye | it was after `Chart`, before the dot; reran at 29, 28 and 25 | 1 run |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `js.localStorage...` today | `fn has no field \`localStorage\``: free, no program means anything by it |
| `client use localStorage` today | `expected end of statement, found \`use\``: free |
| `client ... use` inside a component | `` `use` must be at the top level ``: stays so (decided) |
| `js(k, ...)` with a variable `k` | refused by `quillo check`, with the fix in the message |
| `js("Chart.defaults.color")` in a `let` shown in markup | warned, with `state` suggested |

## Scripts

Every probe used the release build (`~/dev/quillo-app1/target/release/quillo`) in the session's scratch folder. None started a server. The language server was started by the probe and stopped with `shutdown`.

**P1** (`p.qwe`, `quillo check`): `$` today, for the alternatives in Part B. `<form>` was `$.window.innerWidth`, then `$("window.innerWidth")`. Both gave `unexpected character \`$\``.

```quillo
component A() {
  state w = <form>
  <p>{w}</p>
}
```

**P2** (`s1.qwe`, `s2.qwe`, `quillo check`): `client use localStorage` at the top, and `client "chart.js" use Chart` inside a component.

**P3** (`chart.js`, `q.qwe`, `q2.qwe`, `quillo check`): Q-501. `chart.js` is `globalThis.Chart = { defaults: { color: "red" }, make: () => 1 };`. `<form>` was `js("Chart.defaults.color")`, then `Chart.defaults.color`, then `Chart.make()`.

```quillo
client "chart.js" use Chart
component A() {
  let c = <form>
  <p>{c}</p>
}
route GET / = <html><body><A/></body></html>
```

**P4** (`q3.qwe`, `quillo check`): Q-498's error.

```quillo
component A() {
  let k = "eval"
  <button @click={() => js(k, "1")}>x</button>
}
route GET / = <html><body><A/></body></html>
```

**P5** (`j1.qwe`, `quillo check`): `js.` today.

```quillo
component A() {
  <button @click={() => js.localStorage.setItem("t", "d")}>x</button>
}
route GET / = <html><body><A/></body></html>
```

**P6** (`lsp_probe.py`, `python3 -I lsp_probe.py <scratch>`): Q-502. It writes `c.qwe` (the program in Q-502's section), starts `quillo lsp`, sends `initialize`, `initialized` and `didOpen`, then a `textDocument/completion` at line 2, columns 29, 28 and 25. It prints each answer's count, whether `Chart` is in it, and the first twelve labels.

```python
send({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file://"+d,"capabilities":{}}}); recv()
send({"jsonrpc":"2.0","method":"initialized","params":{}})
send({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"quillo","version":1,"text":src}}})
for line, col in [(2, 29), (2, 28), (2, 25)]:
    send({"jsonrpc":"2.0","id":10+line+col,"method":"textDocument/completion","params":{"textDocument":{"uri":uri},"position":{"line":line,"character":col}}})
```
