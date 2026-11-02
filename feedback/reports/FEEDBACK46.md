# Feedback from an app on the starter: Rust glue, `quillo build`'s paths, style maps, binding, `children`, and docs that run ahead of 0.6.0

| | |
|---|---|
| App | An app built from the starter (`quillolang/starter`, tag `v0.6.0`) by another AI session, which sent seven points. This report checks each one against the code and against builds of each version. The app itself was not seen: each repro below is written from its description |
| Quillo | Three builds: `v0.6.0` (991e96c, a `git archive` release build in the session's scratch folder); main `f2481d2` (the same way; the app's author compared against main `11d980d`, and between the two only `window` names and `db.exec_many` changed, nothing in these parts); and the runtime-coverage branch's working tree (`54dd1bd`) for the server-side style map |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, `quillo run`, `quillo build` and `curl` on probe programs; headless Chrome driven by [tests/browser/run.js](../../tests/browser/run.js) with a probe script per page (typing by `document.execCommand("insertText")`, which fires `input` as the DevTools protocol's `Input.insertText` does). Read: [src/native.rs](../../src/native.rs) (`glue`, `walk_program`, and `build_all` at v0.6.0), [src/interp.rs](../../src/interp.rs) (`render_attrs`, `render_attrs_fast`, `write_attr`), [src/client.rs](../../src/client.rs) (the implied `bind:`), [src/client/runtime.js](../../src/client/runtime.js) (`setProp`, `styleText`), USERGUIDE.md (Rust, two-way binding, `children`), the starter's README.md and AGENTS.md, quillolang.org/llms.txt |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section, and the browser probe is at the end. Numbers start at Q-530, after FEEDBACK45.md's Q-529.

Of the seven points: one is a bug in both 0.6.0 and main (the Rust glue, Q-530). Three are 0.6.0 bugs already fixed on main (`quillo build`'s paths, `children`, style maps in the browser). Checking the style-map point turned up a case Q-429's fix missed, on main (Q-531). The binding point does not reproduce on main as described. Its likely cause is that 0.6.0 has no implied binding at all, while main's guide describes it, and that leads into the docs point (Q-533). Main has silent one-way cases nearby (Q-532). The import point is by design. It has a form that works on both versions (a smaller note).

## Fixed on main, still in 0.6.0

Each one reproduces on the `v0.6.0` build exactly as reported and is gone on main. All three fixes landed after the tag (2026-10-29), so anyone on the starter meets them until the next release.

| ID | Where it bit | v0.6.0 | main |
|---|---|---|---|
| Q-154 | `quillo build main.qwe` where `ui/page.qwe` says `use ui/theme.qwe`: `quillo: cannot read ui/ui/theme.qwe: No such file or directory`, exit 1. `quillo check` and `quillo run` read the same program. v0.6.0's `build_all` joins each `use` to the folder of the file that writes it (`todo.push(dir.join(p))`). `run` counts from the program's folder | as reported | `main.qwe uses no .qwer files: nothing to build`, exit 0 (`walk_program`, [native.rs:1351](../../src/native.rs#L1351): "A `use` counts from its program's folder, in every file") |
| Q-345 | `view V(title, children = null)` used as `<V title="a"/>`: `children == null ? <b>icon</b> : children` renders an empty `<div>` on the server. `children` is an empty `html` (`type_of` says `html`, `len` 0), never null, so the default is never used. `view V2(title, children)` with no default does the same, so the default is not what matters; a named parameter (`lead = null`) works because nothing fills it | `<div class="v"></div>` | `<div class="v"><b>icon</b></div>` |
| Q-429 | `` style={{ left: `${x}px`, top: `${y}px` }} `` in a view shown by a component after a click: the attribute is `{"left":"149px","top":"160px"}`. On 0.6.0 the server writes JSON too | JSON on both sides | `left: 149px; top: 160px` in the browser; on the server see Q-531 |

Repros (`quillo run`, then `curl /`; `quillo build main.qwe` for the first):

```quillo
// main.qwe
use ui/page.qwe
route GET / = <html><body><Page/></body></html>

// ui/page.qwe
use ui/theme.qwe
view Page() = <main style={"color: " + accent}>hi</main>

// ui/theme.qwe
let accent = "red"
```

```quillo
view V(title, children = null) = <div class="v">{children == null ? <b>icon</b> : children}</div>
view V2(title, children) = <div class="v2">{children == null ? <b>icon</b> : children}</div>
view W(title, lead = null) = <div class="w">{lead == null ? <b>icon</b> : lead}</div>
route GET / = <html><body><V title="a"/><V title="c">kid</V><V2 title="e"/><W title="b"/></body></html>
```

## New issues

### Q-530. A `.qwer` parameter whose type a `use` names does not build: the glue writes the type outside the file's module

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo build`: a Rust error in a file the program never wrote | reported; reproduced on v0.6.0 and main |

```rust
// auth.qwer
use serde_json::Value;

pub fn role(auth: Value) -> String {
    auth.get("role").and_then(|r| r.as_str()).unwrap_or("none").to_string()
}
```

```quillo
use "auth.qwer"
print(role({ role: "admin" }))
```

```
quillo: building auth.qwer
quillo: cannot build auth.qwer:
lib.rs:246:25: error[E0425]: cannot find type `Value` in this scope: not found in this scope
```

`quillo check` says `ok`. The same happens with `use std::collections::HashMap;` and `pub fn total(m: HashMap<String, i64>)`. The guide names both types as ones that cross ("`HashMap<String, T>` and `BTreeMap<String, T>` are maps", "`serde_json::Value` is anything"), and both are normally imported with `use`. Written in full (`serde_json::Value`, `std::collections::HashMap<String, i64>`), each builds and runs (`3`, `admin`).

The cause is `glue` ([native.rs:691](../../src/native.rs#L691), the same line on main). It writes `let a0: {type} = __quillo::arg(...)` with the parameter's type text as written. That line is in the generated `lib.rs`, outside `mod user`, where the file's own `use` lines do not reach. The error then names `lib.rs:246`, a line of a file the developer never sees and cannot fix. The guide does mention a nearby failure ("Another crate's type named `Value` passes the check and fails the build"), but not this one.

**Suggestion:** let Rust infer the argument's type from the call rather than copying it. For a parameter taken by value, `let a0 = __quillo::arg(&mut a, 0, ...)?;` followed by `user::role(a0)` fixes `T` from `role`'s signature. A `&T` parameter works the same way with `&a0`. Only `&str` (owned as `String`) and `&[T]` (as `Vec<_>`, passed as `a0.as_slice()`) need a type written, and neither needs the file's names. This is not built or tried here. Failing that, the build error could say "write `Value` in full in a `pub fn`'s signature (`serde_json::Value`): the file's `use` lines do not reach it", and `quillo check` could warn of a signature type that a `use` in the file names.

### Q-531. On the server, a style map on an element that has `class=`, a spread, or is a `<textarea>` is written as JSON

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `curl`: the attribute | found checking Q-429 for this report; main and the working tree |

```quillo
route GET / = <html><body>
    <div id="a" style={{ color: "red" }}>x</div>
    <div class="b" style={{ color: "red" }}>x</div>
    <div style={{ color: "red" }} class="d">x</div>
</body></html>
```

```html
<div id="a" style="color: red">x</div>
<div class="b" style="{&quot;color&quot;:&quot;red&quot;}">x</div>
<div class="d" style="{&quot;color&quot;:&quot;red&quot;}">x</div>
```

Q-429's fix is in `write_attr` ([interp.rs:4336](../../src/interp.rs#L4336)), which only `render_attrs_fast` calls. An element with a spread, a `class=` attribute, or a `<textarea>` goes through `render_attrs` instead ([interp.rs:4084](../../src/interp.rs#L4084)). There, any map is written by `json_text` ([interp.rs:4207](../../src/interp.rs#L4207)). Q-429's test ([tests/feedback35.rs](../../tests/feedback35.rs), `style_takes_a_map_of_properties`) puts `id="a"` on its element, not a class, so it passes. In the browser, the runtime's `setProp` turns a map into properties whatever else the element has. Outside a component, nothing runs again, so the page keeps the JSON (the probe's `serverDot` on main: `{"left":"1px","top":"2px"}`). The app's author found server-rendered maps "fine", most likely on elements without a `class=`.

**Suggestion:** apply `style_text` in `render_attrs` too, before the `Value::List(_) | Value::Map(_)` arm. One way is to move the `(Value::Map(m), "style")` step into a helper that both paths call. Then extend the test with `class=`, a spread holding `class`, and a `<textarea>`.

### Q-532. `value={x}` that stays one way passes `check`, and the input and the state part silently

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the browser: the state stays `""` while the input shows the typed text | reported (two cases); the reported shapes do not fail on main; these do |

The report: an `<input value={summary}>` with an `@keydown` handler, and plain inputs in dialog components created after a click, kept `""` in the state while the input showed the typed text. `bind:value` fixed both. On main, none of the following fail. Each was typed into in Chrome, with the state shown beside it: `@keydown` with a lambda; `@keydown` that sets the state on Enter; `@keydown` with a named handler and `type`/`placeholder`; `@keydown` with `@blur` on a state named `value`; a `<textarea>` with `@keydown`; an input inside a `<form @submit>`; a component shown by `{open && <Dialog/>}` after a click; the same inside `<dialog open>`; a component whose state starts from a prop; a row component in a `for`. On v0.6.0 every one of them keeps `""`. 0.6.0 has no implied binding: it came with FEEDBACK40-43 (`client.rs`, "branch feedback40-43"). The starter pins 0.6.0, while main's guide and llms.txt describe the binding. That makes 0.6.0 the most likely explanation of what the app saw (see Q-533).

Three shapes do fail silently on main:

```quillo
view Field(id, v) = <label>{id}<input id={id} value={v}/></label>
component A() {
    state t = ""
    <div><Field id="a" v={t}/><p>{t}</p></div>              // a view's parameter
}
component B() {
    state f = { name: "" }
    <div><input id="b" value={f.name}/><p>{f.name}</p></div> // a field of a state
}
component D(t) {
    <div><input id="d" value={t}/><p>{t}</p></div>          // a component's parameter
}
```

Typing `hello` into each leaves the input showing `hello` and the state `""`. `quillo check` says `ok`. The guide does list these ("the value is not a state (a `let`, a prop, an expression): an input showing a `let` keeps what is typed and sets nothing"; `bind:` takes "not a global or a map's field"). So this is by the rules. But the result is the very thing the implied binding was made to end: an input that shows one thing while the program holds another, with nothing said. The first two shapes come easily when a form is split into a `Field` view or kept as one record.

**Suggestion:** have `quillo check` warn of `value=`, `checked=` or `open=` on an element a person changes, when the value is a parameter or a field, and nothing on the element (`@input`, `@change`, `bind:`, `readonly`) takes what is typed. For example: `` typing here changes nothing: `v` is `Field`'s parameter; give the input `@input={...}`, or `readonly` ``. If a case the app hit still fails on main, its exact markup is needed (see Questions).

### Q-533. The docs the starter points at describe main, not 0.6.0, and main calls itself 0.6.0 too

| Kind | Detected by | Attempts |
|---|---|---|
| docs | errors on the pinned release for features the docs give | reported; each feature run on v0.6.0 and main |

The starter pins `v0.6.0`. Its AGENTS.md sends an assistant to https://quillolang.org/llms.txt "before writing any", and its README's "Next" links the user guide at `blob/main`. Both describe main. Checked on the v0.6.0 build:

| Named in main's docs | v0.6.0 | main |
|---|---|---|
| `platform()` | `` unknown name `platform` `` | a map |
| `filled(x)` (llms.txt lines 70 and 304, live) | `` unknown name `filled` (did you mean `filter`?) `` | works |
| `run_after` | `` unknown name `run_after` `` | works |
| `use "r.rs"` (Rust as `.rs`) | `` ./r.rs:1: error: expected end of statement, found `fn` `` | works |
| `--no-build` | `` unknown option `--no-build` `` | works |
| `import x.qwe use y` | `` expected end of statement, found `lib` `` | works |
| `<input value={state}/>` two-way by itself | one way, silently (Q-532) | two-way |
| `children` null when none are given | an empty `html` (Q-345) | null |
| `quillo app` | **present** (needs `--features app`, as on main) | present |

So the report is right except for `quillo app`, which 0.6.0 has. Three things make it worse:

- Main's binary reports `quillo 0.6.0 (f2481d2, 2026-11-03)`, and `Cargo.toml` says `0.6.0`. The error note "this is quillo 0.6.0 ...; if the code was written for a newer Quillo, that would explain it" therefore reads the same on the release and on main. It cannot tell an assistant which one it has.
- The guide has no version marks. CHANGELOG.md does know: its `## Unreleased` section lists what came after 0.6.0. A reader of the guide cannot see that.
- The starter's README warns that "`main` may use what only the next release has", but about the starter's own `main`, not Quillo's docs.

**Suggestion:** in order of cost:

1. Point the starter at the tag's docs: the guide at `blob/v0.6.0/USERGUIDE.md`, and an llms.txt per release (`quillolang.org/v0.6.0/llms.txt`, written by `book/llms.py` from the tag) in AGENTS.md. The starter's tag moves with each release anyway.
2. Make main's version `0.7.0-dev`, so that `--version` and the error note tell the two apart.
3. Add a "since 0.7" mark to each guide and llms.txt entry that the changelog's `Unreleased` section names. A mark stops being needed once the release that brings it is the oldest supported one.

## Smaller notes

- **Imports across versions.** On 0.6.0, `import lib/m.qwe use price` does not parse, and `use price from lib/m.qwe` is an error ("names are taken from a `.qwer` file"). On main, the first is the form and the second warns `` write this as `import lib/m.qwe use price` ``. Whole-file `use lib/m.qwe` works on both, with no warning on main. So a program that must run on both has a form. It only cannot pick names. Nothing in the core needs to change. The guide's import section could say that `import` is since 0.7 and that on 0.6 the form is `use lib/m.qwe`, which Q-533's marks would cover. Note too that on 0.6.0 the `import` line fails with "expected end of statement, found `lib`". The version note after it is the only hint.

## Questions for you

1. Q-154, Q-345 and Q-429 are fixed on main but are in the release the starter pins. Q-530 and Q-531 are small fixes. Is that worth a 0.6.1 (those five, with the starter's tag moved), or do they wait for 0.7.0?
2. For Q-533: an llms.txt and guide per release, "since" marks in one guide, or both?
3. For Q-532: could the app's author send the markup of the two inputs that failed (the commit summary and one dialog), and say which quillo ran them? Neither shape fails on main in the forms tried here.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `for r in v0.6.0 main; do git show $r:src/client/runtime.js \| grep -c styleText; done` in zsh | to see which versions have Q-429's fix | zsh read `$r:s` as a substitution modifier, so every version showed 0, and main looked as if it had lost the fix; `${r}` gave the real counts (main and 11d980d have it) | 3 commands, and nearly a wrong finding |
| `@click={() => shown = true}` | the usual arrow | does not parse (FEEDBACK45 Q-518); wrote `() => { shown = true }` | 1 run |
| `state t: str = ""` | a typed state, as a `let` takes | `` expected end of statement, found `t` ``: `state` takes no type | 1 run |
| `fn key(e) { ... }` inside a component | a local handler | the error was the `state` line above, not this one; replaced by a `let` lambda while looking | 1 run |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `serde_json::Value` and `std::collections::HashMap<String, i64>` written in full in a `.qwer` signature (main) | builds; `3`, `admin` |
| `quillo build` on main, `use` inside a subfolder's file | counts from the program's folder |
| `children == null` with no body, with and without a default (main) | null both ways; `<V>kid</V>` gives `kid`; `str(children)` warned of by `check` |
| a style map in the browser on an element with a class, shown after a click (main) | `left: 149px; top: 160px` |
| implied binding on main: `@keydown` (lambda, named, sets the state), `@blur`, a `<textarea>`, a `<form @submit>`, `{open && <C/>}` after a click, `<dialog open>`, state from a prop, a component per `for` row | input and state agree in each |
| whole-file `use lib/m.qwe` | the same on v0.6.0 and main, no warning |

## Scripts

**Browser probe** (`node tests/browser/run.js <chrome> http://127.0.0.1:<port>/ <profile> 30000 probe.js`, with `quillo run main.qwe --port <port>`). Two pages were run, one with `Dot`, `Sum` and the dialogs, one with Q-532's three shapes and the variants in the list above. In each, every input has an `id` and a `<p id="<id>s">{state}</p>` beside it. The script below merges the two scripts (the ids differ per page). It types into each input in turn and records `[input.value, shown state]`:

```js
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const type = (el, text) => { el.focus(); document.execCommand("insertText", false, text); };
  try {
    for (let i = 0; i < 200 && !(window[Symbol.for("quillo")] || {}).booted; i++) await tick(30);
    await tick(200);
    out.serverDot = document.querySelectorAll(".dot")[0].getAttribute("style");
    $("#go").click(); await tick(200);
    out.clientDot = $("#pos .dot") && $("#pos .dot").getAttribute("style");
    for (const [i, s] of [["a", "as"], ["b", "bs"], ["d", "dhs"], ["k", "ks"]]) {
      type($("#" + i), "hello"); await tick(200);
      out[i] = [$("#" + i).value, $("#" + s).textContent];
    }
  } catch (e) { out.exception = String(e); }
  out.errors = (window.__quilloErrors || []).map(String);
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
```

0.6.0 has no `window[Symbol.for("quillo")].booted`, so there the loop waits its full 6 seconds before typing. The page had booted by then.

Results, the style part (`Dot` is `<span class="dot" style={{ left: ..., top: ... }}>`):

| | v0.6.0 | main |
|---|---|---|
| `serverDot` (outside a component) | `{"left":"1px","top":"2px"}` | `{"left":"1px","top":"2px"}` (Q-531) |
| `clientDot` (after the click) | `{"left":"149px","top":"160px"}` | `left: 149px; top: 160px` |
