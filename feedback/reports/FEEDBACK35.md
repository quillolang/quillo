# Feedback from a design study: JavaScript in a component, written `{{ ... }}`

| | |
|---|---|
| App | None. This is a study of how a component could hold JavaScript of its own, written between double braces, `{{ ... }}`, and run in the browser where the program writes it. It looks at what `{{` means in Quillo today, where JavaScript can be written today and at what cost, and how the compiler and runtime would take a block. The probes (11 programs, 3 to 20 lines each, and one run 38 times for the reserved words) were written in a scratch folder and run there |
| Quillo | 0.6.0 (378e64b, 2026-11-01), branch `feedback29-fixes`, `./target/release/quillo` |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, `quillo run` and curl; headless Chrome 156 on an Apple M5 (`feedback/tools/drive.mjs`, with `--mismatches`); a scan of every `{{` in the 3,512 `.qwe` files of quillo-app1, quillo-components, quillo-website (with its 627 plugins), quillo-install, quillo-feedback, quillo-examples, quillo-starter and quillo; reading [src/parser.rs](../../src/parser.rs) (`parse_html_children`, `parse_braced_expr`, `brace_is_map`), [src/client.rs](../../src/client.rs) (a compiled component, `Scan`, `minify_into`, `names_in`, `js_path_problem`), [src/client/runtime.js](../../src/client/runtime.js) (`fromPage`, `guardCall`, how values are kept), USERGUIDE.md's JavaScript libraries and Reactivity sections, and the VS Code grammar |

No core change was made: `{{ ... }}` does not exist yet, and this report is its design. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched; each finding's repro is in its section. Numbers start at Q-427, after FEEDBACK34.md's Q-426.

What worked well: the place for the feature is already free. Of the 3,512 files, none writes `{{` in an event handler, in `@mount`, at the start of a line or as an element's child. Every `{{` outside a string is an attribute's map (318) or one template's `${{`. Values need almost no conversion, because in the browser Quillo's values are already JavaScript's: numbers, strings, arrays, plain objects and `null`. The rules a block needs already exist for `js(...)`: null on the server, a Promise waited for where code can wait, an error value for what fails, and a `state` initializer run again in the browser. The bundle already has a scanner of JavaScript's strings, templates and regular expressions ([src/client.rs:1142](../../src/client.rs#L1142)), which is what finding a block's closing `}}` needs. The grammar already embeds CSS and SQL, so embedding JavaScript follows the same pattern. And `.qwer`, Rust in a program file, is the precedent for native code on the server side; `{{ }}` would be the browser's.

## The answer

`{{ ... }}` is JavaScript, compiled into the page's bundle as a function at the place the program writes it, and called there. Quillo's names reach it as arguments, and what it returns comes back as a Quillo value. It runs only in the browser.

```quillo
client module "https://esm.sh/canvas-confetti@1.9.3" use default as confetti

component Board(cells: list) {
    state count = 0
    state width = {{ return window.innerWidth }}          // null on the server, then the width

    {{ document.title = `${count} cells picked` }}        // runs again when count changes

    <div @mount={{
        const ro = new ResizeObserver(() => el.classList.toggle("narrow", el.clientWidth < 600))
        ro.observe(el)
        return () => ro.disconnect()                     // when the element leaves the page
    }}>
        <canvas @mount={{
            const ctx = el.getContext("2d")
            for (let i = 0; i < cells.length; i++) ctx.fillRect(i * 4, 0, 3, cells[i])
        }}></canvas>
        <button @click={{ e.currentTarget.animate([{ scale: 1.1 }, { scale: 1 }], 150) }}
                @dblclick={() => { count += 1; {{ confetti() }} }}>Pick</button>
        <p>{count} of {len(cells)}, {width}px</p>
    </div>
}
```

Where a block may go, and what it is there:

| Where | What it is | Today |
|---|---|---|
| `@event={{ ... }}` | the handler: a function body given `e`, the event, and `el`, the element it is on | refused: "`@click` needs a function, got null", so no program changes meaning |
| `@mount={{ ... }}` | `@mount`'s function, given `el`; a function it returns runs when the element leaves the page | refused, as above |
| browser-side code: a handler's or `@mount`'s Quillo body, a component's `let` function, `state x = {{ ... }}`, a `fn` a component calls | an expression: what the block returns, or null | a block in a block, or a map; not written in any of the 3,512 files |
| a statement in a component's body | a statement as any other: it runs once, then again when a state it reads changes, and a function it returns runs before the next run and when the component goes (an effect) | as above |
| an attribute, `Prop={{ k: v }}` | unchanged: a map in braces, as in JSX | 318 uses outside strings, all on component tags (Q-427) |
| `${{ ... }}` in a template | unchanged: `${` and a map | 1 use, [tests/browser/seam.qwe:14](../../tests/browser/seam.qwe#L14) |
| an element's child, `<p>{{ ... }}</p>` | refused, with what to write: `{x}` shows a value, and a value from the page goes through `state` | shows a map or a block's value (Q-430) |
| a `view`, a route, a `server fn` | refused when the program loads: "JavaScript runs in the browser: put it in a component" | as above |

`{{` is the two braces written together. `{ {` with a space keeps what it means today, so the rare Quillo block that starts with a block or a map can still be written, and `quillo fix` can rewrite one into that form.

The rules, each one `js(...)`'s where it has one:

- **Names come in as arguments, not as text.** Each Quillo name in scope that the block's code mentions (outside its strings and comments) is passed under its own name: locals, parameters, props, constants, functions, the current value of each state, and each library name from `client ... use`, including a module's, which the runtime keeps out of `window`. The code is the same text for every visitor. Data enters only as arguments, so a value never becomes code.
- **State is set by Quillo.** `count = 5` in a block would only change the block's own copy, so it is refused when the program loads: "`count` is state: set it in Quillo, `count = {{ return 5 }}`". The same goes for `count++`, `count += 1` and the other assignments.
- **On the server, a block does nothing, and its value is null.** A `state` set from a block is set again in the browser once the component is on the page, as `state s = js(...)` is today, with no mismatch. A `let` shown in markup is the case to avoid (Q-431).
- **A Promise is waited for where code can wait**: `let r = {{ return fetch("/x").then((r) => r.json()) }}`, or `await` inside the block, which makes it an `async` function.
- **What it throws is an error value**, `js: <message>` with the `.qwe` file and line, which `?` passes on, as for a name the page does not have.
- **The guard-rails stay with `js(...)`.** Inside a block, `eval`, `innerHTML` and `setTimeout` are JavaScript's own, since that is what native means. The guide already calls the checks "a guard-rail against mistakes, not a security boundary". What a block cannot do is put data into code: its text is fixed when the program loads.
- **Nothing goes on `window`.** A block is a function, so its `const`, `let`, `var` and `function` declarations stay its own. That keeps to the rule that JavaScript does not go on `window`: a module's names reach a block from the runtime's private registry, not from a global.

## What a program does today

Three ways reach the page's JavaScript from a component. None of them gives code written in the component access to its element and state.

| Way | What it costs |
|---|---|
| `<script>` with the code as its text | breaks: the text is escaped, so a quote is a SyntaxError, and a `{` is read as Quillo. The browser then remakes the component and runs its scripts a second time (Q-428) |
| `<script>{raw(CODE)}</script>`, the code in a Quillo string | works, and is what the programs do ([app/clinic/ui/page.qwe:21](../../app/clinic/ui/page.qwe#L21), and quillo-website's `components.qwe` and `sponsors.qwe`). But the code is a string, with no highlighting and no line numbers in an error. It runs at the page's global scope as the page is read, before the component resumes, and reaches neither its element nor its state |
| `js("path", ...)`, `js_new`, or a file of one's own, `client /assets/helper.mjs use helper` | works, and reaches the element through `@mount`. But the code lives in another file, and each call from Quillo goes through the path's lookup and `guardCall`'s checks |

The cost of a crossing, measured in Chrome with 200,000 calls in `@mount`, three page loads:

| Loop of 200,000 | Time |
|---|---|
| `a += js("Math.max", i, 1)` from Quillo | 45.3 to 48.4 ms |
| `b += (i > 1 ? i : 1)` in Quillo | 6.5 to 7.3 ms |
| `a += Math.max(i, 1)` in JavaScript | 2.3 ms |
| `w += js("window.innerWidth")` (20,000, times 10) | 46 to 52 ms |
| `w += window.innerWidth` in JavaScript | 22.6 ms |

A call through `js(...)` costs about 20 times a cheap function's own work. A loop inside `{{ }}` runs at JavaScript's speed, as the two JavaScript rows do, and pays for one crossing in all.

## Why `{{` is free where JavaScript goes

Every `{{` in the 3,512 files, after taking out `"..."` strings and `//` comments, by what stands before it:

| Before `{{` | Count | Example |
|---|---|---|
| an attribute, `name=` | 318 | `form={{}}`, `values={{ "2026-03-14": 4 }}`, `value={{ adults: guests }}` |
| `$`, in a template | 1 | `` `${{ x: 1 } == { x: 1 }}` `` |
| `@event=` or `bind:x=` | 0 | |
| the start of a line (a statement, or an element's child on its own line) | 0 | |
| `>`, an element's child | 0 | |
| `let x =`, `state x =` | 0 | |
| `(`, `,`, `[`, an argument or a list's item | 0 | |

Counted again without taking out strings, 480 attribute maps, every one on a capitalized tag: a component's prop. None is on an HTML element. Many more `{{` are in strings (Mustache templates in the mail and label plugins, MailMerge's `{{name}}`), and a string is not touched by any of this.

The attribute case cannot change. `prop={{ k: v }}` is JSX's way to pass an object, it is everywhere in what models have read, and it is how Quillo's programs pass a map. A block in an attribute would also be one more way to compute a value the page could not show until the browser runs, which `@mount` already does.

## How a block compiles

### Finding the end

At `{{`, the lexer reads JavaScript up to the `}}` that is not inside one of its strings, templates (with their `${...}`), regular expressions or comments, at a brace depth of zero:

| Block | Ends at |
|---|---|
| `{{ return {a: {b: 1}} }}` | the last `}}`: the inner `}}` closes the objects, from depth 2 to 0 |
| `{{ return {a: 1}}}` | the last two: `}` takes the depth to 0, then `}}` ends |
| `{{ s = "}}" }}` | the last `}}`: the first is in a string |
| `` {{ t = `${ {a: 1}.a }}` }} `` | the last `}}`: the first `}` is inside `${...}` |
| `{{ ok = /}}/.test(s) }}` | the last `}}`, if `/` is read as starting a regular expression after `=` |
| `{{ x }}}` | the first `}}`; the third `}` is markup's, with its usual message |

`Scan` in [src/client.rs:1142](../../src/client.rs#L1142) has these rules already, for the minifier, with a rule for a regular expression after a keyword (`after_keyword`). A `/` after `)`, `]`, `}`, a name or a number is a division, and any other `/` starts a regular expression. That is wrong only for a regular expression right after a block's `}`, which is rare and can be put in parentheses. A block that is never closed should say where it began and what may have opened a string: "the JavaScript from line 12 has no `}}`: is a quote, a backtick or a `/` inside it left open?"

### The function

Each block becomes a function in a scope of its own, ahead of the bundle's, where none of the runtime's names exist:

```js
const $J = (function () { "use strict"; return [
  /* ui/board.qwe:5 */ function (count) { document.title = `${count} cells picked` },
  /* ui/board.qwe:7 */ function (el) { const ro = new ResizeObserver(...); ... },
]; })();
(function ($rt) { ... W.js_block($J[0], [$count_1.v]) ... })(window.__quillo_rt);
```

- **Not in the component's scope.** A compiled component sees `G`, `W`, `B`, `boot`, `div` and its own `$count_1` and `$step_2` ([the bundle of a 10-line counter](#what-a-compiled-component-looks-like)). A block written there would find the runtime's `G` where it meant a page's `G`, and could change it.
- **Strict mode**, as the bundle is: `with`, octal literals and assigning an undeclared name are errors. The guide should say so.
- **`async`** when the block's code has `await` outside a nested function, so the value is a Promise, which is waited for.
- **Only in the bundle file**, `/_quillo/b/<hash>.js`, never inline and never through `new Function`. A page with `Content-Security-Policy: script-src 'self'` keeps working, with no `'unsafe-inline'` and no `'unsafe-eval'`. The block's text is part of the bundle's hash, so an edit makes a new address.

### The names it is given

- **The names it mentions.** A scan like `names_in` ([src/client.rs:1060](../../src/client.rs#L1060)) finds the names in the code, except names after a `.`, and is given only those in Quillo's scope. Unlike `names_in`, the scan must skip strings and comments. Each state passed is read at the call, and reading is what makes a statement follow a state, so a state named in a log message (`{{ console.log("count is low") }}`) would run the block on each change of `count`.
- **Not a JavaScript reserved word.** Of 38 reserved words and special names tried, Quillo takes 35 as a local (all but `in`, `let` and `NaN`): `new`, `class`, `this`, `default`, `function`, `arguments`, `eval`, `undefined` and the rest pass `quillo check` as locals. The scan sees `new` in `new Date()` and `this` in `this.x`. A parameter named `new` is a SyntaxError, and one named `arguments` or `eval` is a SyntaxError in strict mode. So these are never passed, and code cannot name them anyway.
- **Quillo's names hide the page's.** A Quillo `name`, `status`, `top`, `parent`, `origin`, `length` or `event` is what the block sees. That is what a reader of the component expects, but the guide should say so. `globalThis.name` reaches the page's.
- **`e` and `el` are the block's own** in `@event={{ }}` and `@mount={{ }}`. `quillo check` should warn when the component has a Quillo `e` or `el` of its own that the block would have meant.

### Values both ways

In the browser a Quillo value is mostly a JavaScript value already ([runtime.js:33](../../src/client/runtime.js#L33), [:303](../../src/client/runtime.js#L303)):

| Quillo | In a block | Back to Quillo |
|---|---|---|
| `int` | a number, or a `BigInt` past 2^53: `n + 1` then throws "Cannot mix BigInt and other types" | a whole number is an `int`, as `js(...)` gives |
| `float` | a number; `2.0` cannot be told from `2` | |
| `str`, `bool`, `null` | the same | `undefined` is `null` |
| `list`, a map | an array, a plain object: the same one Quillo holds | an array or a plain object is a list or a map |
| `dec` (money) | the runtime's `Dec`, an object with `m` and `s` | |
| an error value | the runtime's `Err` | a throw is an error value |
| `raw(...)` | the runtime's `Html` | |
| a function | callable, with its arity check; a call may give a Promise | a function is callable from Quillo, as one from `js(...)` |
| an element, a library's object | the same object | the same object |

Two of these need the guide's words. A list or map given to a block is the one Quillo holds, so a block that changes it (`items.push(x)`) changes the state without its signal knowing, and any other place that holds the same list. So the rule is to return a new value. Under `--watch`, a block could be given frozen copies, so a change throws where it is made. And a `Dec` and an `Err` are the runtime's own, so the guide should name what a block may read of them (`str(d)` in Quillo first, or `d.toString()` if it is given one), or a block should get money as its text.

### On the server, and when the page resumes

What `js(...)` does already:

| Written | On the server | In the browser |
|---|---|---|
| `state w = {{ return innerWidth }}`, shown | null | set once the component is on the page, with no mismatch (probed with `js(...)`) |
| `let w = {{ return innerWidth }}`, shown | null | a mismatch, and the whole component is made again (Q-431) |
| `{{ ... }}` as a statement | not run | run as the component resumes, then when what it reads changes |
| `@mount={{ ... }}`, `@event={{ ... }}` | nothing | as `@mount` and handlers are |

`quillo check` should say what to write for the second row: "a value from the page shown in markup: use `state`, or the component is made again in the browser".

### When the code is wrong

- **A syntax error stops the whole bundle.** The browser refuses a script with one SyntaxError in it whole, so a typo in one block would stop every component on every page that bundle serves. That is the largest risk in the design. There are two ways to meet it, and the first is the one I'd take:
  1. Parse each block when the program loads, so a typo is an error at `ui/board.qwe:7` from `quillo run` and `quillo check`, as Quillo's own are. That needs a JavaScript parser in the binary. `oxc_parser` is the fast one written in Rust. Its size in the 25.5 MB binary was not measured.
  2. Put the blocks in a file of their own beside the bundle, which fills `$J` in the runtime's private registry. A SyntaxError there stops only the blocks, and the runtime can say "the JavaScript in `{{ }}` did not load: SyntaxError at ui/board.qwe:7", with the components working otherwise.
- **An error while it runs** is an error value, with the block's `.qwe` file and line. Under `--watch`, each block's lines go into the source map ([src/client.rs:731](../../src/client.rs#L731)), so DevTools shows the `.qwe`.
- **The minifier** keeps a block's lines as written, except blank lines, indentation and whole-line `//` comments, and joins a line only after `;`, `{` or `,` when `Scan` is sure it is in code. That holds for JavaScript with no semicolons, since lines are not joined after anything else. It needs a test of its own: a block whose lines start with `(`, `[` and `` ` ``.

## What else changes

- **`quillo check`** refuses what the table above refuses, and warns as above. `js_path_problem`'s list ([src/client.rs:96](../../src/client.rs#L96)) does not apply inside a block. A warning when a block sets `innerHTML`, `outerHTML` or `document.write` from a name Quillo gave it would catch the one way data becomes markup.
- **`quillo fix`** leaves a block's text as written, and changes only its indentation with the line it is on.
- **The editors.** The VS Code grammar gets a `{{ ... }}` region embedding `source.js` (`contentName: meta.embedded.block.javascript`, beside the CSS and SQL ones, at line 1003 of quillo-editors' `vscode/syntaxes/quillo.tmLanguage.json`). It also needs `applyEndPatternLast`, so JavaScript's own `{...}` scopes close before the region's `}}` does, and an object literal does not end the region. JetBrains uses the same bundle. The language server gives a block no completions, but it knows its range, so hovering a Quillo name inside it can show the name.
- **`highlight`** ([src/highlight.rs](../../src/highlight.rs)), and the runtime's highlighter ([runtime.js:1550](../../src/client/runtime.js#L1550)), colour a `.qwe` file's blocks as JavaScript, so the website's code samples do.
- **The guide, the book and the website.** A section after JavaScript libraries, "JavaScript in a component", and an entry in "Coming from other frameworks" (React's `useEffect` with a ref is a `{{ }}` statement). The book follows, as it does for every change. The FAQ's "use npm packages, TypeScript, a bundler?" answer changes too.

## Tests it needs

A `tests/feedback35.rs` for the parser and `quillo check`, and a `tests/browser/native.*` in Chrome:

- every row of [Finding the end](#finding-the-end), plus a block of 10,000 lines, a block with a CRLF file, a block with `</script>` and `<!--` in a string, and one never closed, at the end of the file and before the next `component`;
- every row of the table of places: each refusal with its message, and `{ {` with a space, which keeps today's meaning;
- the names: a state named only in a string or comment, which does not make the block run again; `new` and `arguments` as Quillo locals beside `new Date()`; a Quillo `name` against `window.name`; `e` and `el`; a module's export (`use default as confetti`), with nothing new on `window`;
- the values: a `BigInt`, a `Dec`, an `Err`, a returned `undefined`, `NaN`, a `Map`, a function both ways, a Promise and `await`, a throw;
- state: `count = 5` refused, `count = {{ return 5 }}` set, and a statement block that runs again when its state changes and calls its cleanup first;
- the page: no mismatch for `state x = {{ }}`, `@mount`'s cleanup when the element goes, a page with `script-src 'self'`, a block in a component shown later, and two instances, each running its own;
- speed: the 200,000-call loop above, inside a block and through `js(...)`.

## Seen again

None.

## New issues

### Q-427. A component cannot hold JavaScript of its own that reaches its element and state

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the guide, probes | |

What JavaScript a component runs is in another file (`client /x.mjs use helper`, `js("path")`), or in a Quillo string (`<script>{raw(CODE)}</script>`), which runs at the page's global scope and reaches neither the element nor the state. Writing it in place, as a `<script>` element's text, breaks (Q-428). Each crossing through `js(...)` costs about 20 times the work of a cheap call ([the measurements](#what-a-program-does-today)).

**Suggestion:** `{{ ... }}`, as designed above. Build it in this order, each step usable alone:

1. `@event={{ }}` and `@mount={{ }}`. These have no meaning today, the lexer and the function are the same as for the rest, and they cover most of what programs do with a `<script>` or a `js(...)` today.
2. `{{ }}` as an expression and a statement in browser-side code.
3. The parse at load (or the separate file), the editors, the guide and the book.

### Q-428. A `<script>` element's own text is escaped as markup: a quote becomes `&quot;`, so the script is a SyntaxError, and in a component it runs twice

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (a console SyntaxError; `quillo check` says ok) | 2 |

The server writes a `<script>`'s literal text as any element's text, escaped, in a route's page, in a view and in a component. Inside `<script>` the browser does not decode entities, so the code it runs is `console.log(&quot;page&#39;s&quot;)`. In a component, the browser's text then differs from the server's. That is a mismatch, so the component is made again, and on the remake each of its `<script>`s runs a second time. A script with no quotes runs once per instance, as expected. And a `{` in the text is read as Quillo, so most JavaScript does not parse at all (`const o = {a: 1}` gives `expected }, found :`). The guide ([USERGUIDE.md:714](../../USERGUIDE.md#L714)) says what `{x}` and `raw(...)` do in a `<script>`, but not what its text does. The programs avoid all of this by writing `<script>{raw(CODE)}</script>`.

```quillo
view Shell() = <div><script>console.log("view's"); window.a = 1</script></div>
component C() {
    <div><script>console.log("comp's")</script></div>
}
route GET / = <html><head><script>console.log("page's")</script></head><body><Shell/><C/></body></html>
```

```
$ curl -s http://127.0.0.1:8742/ | grep -o '<script>console[^<]*</script>'
<script>console.log(&quot;page&#39;s&quot;)</script>
<script>console.log(&quot;view&#39;s&quot;); window.a = 1</script>
<script>console.log(&quot;comp&#39;s&quot;)</script>
```

In Chrome, a component with three scripts (no quotes; quotes; a `{"..."}` string with `'` in it) gave two `SyntaxError: Unexpected token '&'`, a text mismatch (`want: ... = 'ok'`, `have: ... = &#39;ok&#39;`), `__quilloRemade` 1, and the quote-free script's counter at 2.

**Suggestion:** a `<script>`'s text is raw text in HTML, so write it as it is, with only `</` and `<!--` changed, as `raw(...)` there already is. Then say what a literal `{` does in one: an error that names `{{ }}` (Q-427), or `{raw(...)}` until that exists.

### Q-429. `style={{ color: c }}`, JSX's habit, writes the map's JSON into the attribute, and the style is silently ignored

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

```quillo
component C() {
    state tone = "red"
    <div id="s" style={{ color: tone }} @click={() => { tone = "blue" }}>styled</div>
}
```

The server writes `style="{&quot;color&quot;:&quot;red&quot;}"`, and after the click the browser writes `{"color":"blue"}`. The computed colour stays `rgb(0, 0, 0)` on both sides. `quillo check` says ok. In React this is the way to write a style, so a model writes it without thinking.

**Suggestion:** take a map in `style`, as React does: `{ color: tone, font_size: "2rem" }`, with `_` or camelCase becoming `-`. It already changes only what it names (Q-337), so a map is that rule written per property. Otherwise, refuse a map there, in `quillo check` and when the program loads, saying what to write: ``style={`color: ${tone}`}``.

### Q-430. `<p>{{ name }}</p>`, the habit of Vue, Angular, Handlebars and Jinja, shows `{ name: "Ana" }`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl | 1 |

`{` opens a value and `{ name }` is the map `{ name: name }`, so the element shows the map. `{{}}` shows `{}`, and `{{ let y = 2 ... }}` shows the block's value. Nothing warns.

```quillo
component C() {
    let x = 5
    <div><p>{{ x }}</p><p>{{ a: 1 }}</p><p>{{}}</p></div>
}
```

The server writes `<p>{ x: 5 }</p><p>{ a: 1 }</p><p>{}</p>`.

**Suggestion:** with Q-427, `{{` as a child is refused, with "to show `x`, write `{x}`" when the block is only a name in scope. Without Q-427, `quillo check` should warn on a map written as an element's child.

### Q-431. `let w = js("window.innerWidth")` shown in a component's markup makes the browser make the whole component again, on every load

| Kind | Detected by | Attempts |
|---|---|---|
| perf, silent | browser (`--mismatches`) | 1 |

```quillo
component W() {
    let w = js("window.innerWidth")
    state s = js("window.innerWidth")
    <div><p id="l">let {w}</p><p id="s">state {s}</p></div>
}
```

The server writes `let ` and `state ` (both null). In the browser both show 1440, but the `let`'s text is a mismatch (`want: "1440", have: null`) and `__quilloRemade` is 1. The `state` is set after the component resumes, with no mismatch. The guide says what a `state` initializer that reads the page does, not what a `let` does.

**Suggestion:** treat a `let` whose value is from the page as a `state` initializer is treated: null on the server, and worked out again once the component is on the page, not as a mismatch. Failing that, `quillo check` should warn: "a value from the page shown in markup: use `state`". `{{ }}` follows whichever rule is chosen.

## What a compiled component looks like

For reference, the bundle of this 10-line counter, which is why a block cannot simply name a Quillo variable:

```quillo
fn double(x) = x * 2
component Counter(start: int, label: str = "Count") {
    state count = start
    let step = 2
    <div>
        <canvas @mount={(el) => { el.width = 100 }}></canvas>
        <button @click={() => { count += double(step) }}>{label}: {count}</button>
    </div>
}
```

```js
const { $mount, G, W, boot, div } = $rt;
G["Counter"] = W.component("Counter", ["start", "label"], function ($i, $p) {
  ... let $step_2; const $count_1 = $i.sig("count", () => { return $p.start; }); $step_2 = 2;
  return W.t($b0, [function ($el_3) { ... W.setf($el_3, "width", 100); ... },
    async function () { ... $count_1.set(W.add($count_1.v, ... G["double"]($step_2) ...)); ... },
    () => ($p.label), () => ($count_1.v)]); });
```

A local is renamed (`step` is `$step_2`), a state is a signal (`$count_1.v`, `.set(...)`), a prop is on `$p`, and a function is `G["double"]`. Code written as JavaScript cannot name any of them, so the compiler passes them in.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `pkill -f 'quillo run'` to stop each probe's server | the usual way to clean up | it matches every `quillo run` on the machine, and other sessions were testing at the same time. The six `quillo run` instances running before the probes were still up afterwards, so the sandbox seems to have limited it to this session's processes; one started and stopped in between cannot be ruled out. The user said not to stop any instance, and from then on each probe was stopped by its own PID | 4 commands |
| `@click={{ n += 1 }}` among the first probe's cases | to see what `{{` is in a handler | `quillo check` refused the file ("`@click` needs a function, got null"), so the other cases waited for a second run. It is also this report's best news | 1 check |
| `b += max(i, 1)` in the timing loop | Python's and JavaScript's `max` | `` `max` takes 1 argument, got 2 ``: Quillo's takes a list; a ternary instead | 1 check |
| The next number as Q-426 | FEEDBACK34.md's highest, read early in the study | another session added Q-426 to FEEDBACK34.md while the study ran; seen in its headings, so this report starts at Q-427 | 1 grep |
| One component with three `<script>`s, two of them with quotes | to see each case at once | `__seen` was 2, which could be the remake or two runs; a second probe, with no quotes and two instances, told them apart (once each, no remake) | 1 run |

## Limitations

- Nothing was built: no core change, as asked. The block's lexer, function, names and refusals are design. What is measured is today's Quillo.
- The binary-size cost of a JavaScript parser (`oxc_parser`) was not measured.
- The scan of `{{` is rough. It takes out `"..."` strings and `//` comments, but not a backtick template's text, so 2 of its hits were inside templates. It covers the eight local repositories, not programs elsewhere.
- The timings are from one machine (Apple M5) and one browser (Chrome 156, headless), with three loads each. Safari and Firefox were not run, and neither was a phone.
- The VS Code grammar's `applyEndPatternLast` with an embedded `source.js` was not tried.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `@click={{ n += 1 }}` | refused by `quillo check`: "`@click` needs a function, got null"; no program depends on it |
| `{{ x }}` and `{{}}` in markup | the shorthand map and the empty map, as the guide's map rule says (but see Q-430) |
| `Prop={{ k: v }}` on a component | a map, as the 318 uses expect |
| A component's `<script>` with no quotes, two instances | runs once per instance, not again when state changes |
| A component shown later, holding a `<script>` | its script runs once when it appears |
| `<script>{"... const o = {a: 1} ..."}</script>` in a component | runs in the browser; braces in a string are fine |
| `state s = js("window.innerWidth")` shown in markup | null on the server, 1440 in the browser, no mismatch |
| `js("Math.max", i, 1)` 200,000 times | the same total as Quillo's own loop (`a == b`), with no errors |
| Quillo names that are JavaScript reserved words (35 of 38 tried: `new`, `class`, `this`, ...) | accepted, and harmless today, since the compiler renames every local |
