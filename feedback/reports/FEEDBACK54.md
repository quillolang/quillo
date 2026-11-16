# Feedback from studying hot module replacement for `--watch`: keeping the developer's data on the page

| | |
|---|---|
| App | None: a design study. The question: what does a save under `--watch` cost the developer today (what they typed, opened, scrolled to, signed in as, saved), and what would real hot module replacement (HMR) take in Quillo's runtime and server |
| Quillo | `0.7.0-dev`, the release build of 4ed1155's source (it reports `424e1a8`, the commit before; 4ed1155 changed `money.rs` and `runtime.js`'s money code only, nothing on the reload path), branch feedback50-fixes |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | Headless Chrome over the protocol (`feedback/tools/chrome.mjs`, `drive.mjs`): a probe page with a component holding three states, a bound field, a field and a form that are not bound, a `<details>`, a tall page scrolled down, a server fn writing to a top-level `let`, and a session counter. The page was set up by real clicks and typing, then `main.qwe` was edited on disk four ways and the page read back after each. Plus `curl` for the compiled files, the session across a restart, and an in-memory database. The scripts are at the end |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section. Numbers start at Q-685, after FEEDBACK53.md's Q-684.

What worked well: the pieces HMR needs are mostly there. Each component compiles to a file of its own (`/_quillo/src/<hash>/main.qwe/Counter.js`) that defines it with one assignment, `G["Counter"] = W.component(...)`, and asks for each state **by name**: `$i.sig("count", () => 0)`. `Inst.made` already starts a state from a map of values by name (`s0`, what the server rendered), and `takeHydration` already hands inner components their states in order, by name. That is the whole mechanism of carrying state across a swap. The events socket (`/_quillo/events`) already sends more than "reload": a stylesheet change is a `css:` message, and the page swaps the `<link>` and keeps everything (FEEDBACK11, Q-078). A save that does not load leaves the previous version serving, so the page goes on working. Sessions survive a reload, as the process does.

## The answer

Real HMR can be built on what Quillo has, without changing production: everything below is `// dev` lines in the runtime and `--watch`-only code in the server.

Today **every save reloads every open page** (Q-685), whatever changed, even a route the page never calls. The measured cost of one save, on the probe page:

| What the developer had | After a save |
|---|---|
| A component's state (`count` 3, `open` true) | reset (0, false) |
| Text typed in a bound field (`bind:value`) | gone |
| Text typed in a field nothing binds, and in a plain `<form>` outside any component | gone (the browser did not restore it) |
| A `<details>` opened | closed |
| Focus in a field | lost |
| Scrolled to 1200 px | 1111 px (the browser's own restore, before the page had its full height) |
| A note saved through a `server fn` into a top-level `let` | gone on the server (Q-687) |
| Rows in a `db.open(":memory:")` database | gone (Q-687) |
| The session (signed in) | kept across a reload; gone at every restart of `quillo run --watch` (Q-689) |
| A save with a syntax error | nothing on the page; it goes on running the old version (Q-686) |

Four things stand in the way of keeping it all:

1. **The page is told only "a new version"** (`dev_script`: `if (t !== v) location.reload()`): not what changed, nor whether it failed.
2. **A change cannot be narrowed to what changed** (Q-688). Template blocks are numbered across the whole program (`$b0`, `$b1`, ...), so adding one block to `First` renumbers `Second`'s (`W.t($b1` became `W.t($b2`). And one hash names every file of the bundle, so all of them get new URLs and the browser's cache cannot help.
3. **The runtime keeps no list of the components on the page** except when a devtools extension asks for one (`TOOLS.all`), so nothing could find the instances of `Counter` to swap.
4. **The server starts its data again at each reload** (Q-687): the new version runs its top level from scratch, in a new interpreter.

Part A is the design. Part B is the message the page gets. Part C is what the server keeps. The issues follow, then an order of work and the open questions.

## Part A: swapping on the page

The rule, as React's Fast Refresh and Flutter's hot reload have it: **the code is new, the state is old**. A component whose code changed is made again from its new code, given the values its states had, by name. What no new code touches is not touched at all.

### A.1 What a save becomes on the page

For each open page, after a save that loaded:

1. **Nothing the page uses changed** (a route it never calls, a server fn's body, a file it does not `use`): nothing happens on the page. The probe's second edit (to `/visit`) reloaded the page today.
2. **Only client definitions changed** (a component, a view, a function compiled for the browser): the page loads the changed files and swaps them (A.2).
3. **What the server renders outside components changed** (the route's markup, a component's props written there, `<head>`): the page fetches its own URL again and morphs the document onto it (A.3), then swaps as in 2. The probe's first edit (`label="Version A"` to `"Version B"` in the route) is this case.
4. **What cannot be swapped** (A.5): the page reloads, keeping what the browser alone would lose (A.6).

The server can tell 1 and 2 apart from 3: it knows which definitions' compiled code changed (once the hashes are stable, Q-688). Only the page can tell 3: whether the HTML it would get differs. So in case 3 the page asks, and compares.

### A.2 Swapping a component

In development, the runtime keeps each live instance by its definition's name (a `Map<name, Set<Inst>>`, filled in `island` and `resume`, emptied in `dispose`). A swap of `Counter`:

1. Load the new `Counter.js` (a `<script>` with the new file's URL). Its `G["Counter"] = ...` replaces the definition; `C` too.
2. For each live instance of `Counter`, outermost first (an inner one is made again with its outer one):
   - Take its states' values, `{count: 3, draft: "hello", open: true}`, and those of the components inside it, in order, by name: the same queue `resume` builds from `data-s` (`hydrating`).
   - Take what the DOM holds that no state does (A.4).
   - Dispose of the instance (its effects' cleanups, `every`, live subscriptions end), empty its island, and run the new definition in it, with the old values as `s0`. `Inst.made` takes a state from `s0` when it has the name, exactly as it does when a page is resumed.
   - Put back what A.4 took.
3. A view or a function compiled for the browser has no instance. A change to one swaps the components that call it. The compiler knows who refers to whom (`G["name"]` in their code), so the server sends the list.

Which states keep their value (decided by the user, 2026-11-04: a value survives the latest change, and keeps its value, while its type is still the same):

| The state after the edit | Its value |
|---|---|
| Same name, same type | kept |
| Same name, same type, initializer changed (`state count = 0` to `= 10`) | kept (the new initializer applies from the next reload or a new instance) |
| Same name, type changed (`state count = 0` to `= ""`, or `count: int` to `count: str`) | started again from the new initializer, and said in the console |
| Renamed, or new | the initializer |
| Removed | dropped |

The type is the state's declared one (`state count: int = 0`), or, where none is written, the one `quillo check` infers from its initializer (`0` is an `int`). The compiler passes it as a fourth argument to `$i.sig` in development only, and the runtime keeps the old value when it fits that type. An initializer whose type cannot be told (`state problem = null`) keeps any value.

Inner components keep their states by name and order, as resuming does: `<Counter/><Counter/>` with a third one added *before* the others shifts them. A `key` prop, where one is given, keys them instead. This is Fast Refresh's rule for hooks, and it should be in the guide.

What runs again: a swapped component's `@mount` and its effects, as its code is new. A `live` state keeps its value until its loader answers again. `every` starts again at its interval.

### A.3 A page whose server HTML changed

The page fetches its own URL (`GET`, marked `x-quillo-hot: 1` so the log can tell it from a visit) and parses it. If the body is the same as the one it has, apart from the dev script's version and the bundle's URL, nothing more is done. Otherwise it morphs `<body>` onto the new one, as `morph` already does for a list's rows:

- Outside islands: attributes, text, and the elements themselves, matched by position and tag; an input's `value`, `checked` and `open` are left as the person set them unless the server's own value changed.
- An island present in both (same place, same component): kept, with its new `data-p` given to it as props. A top island's props are plain values from JSON, so taking new ones means making it again, as in A.2, with its states kept. An island whose props did not change is not touched.
- An island only in the new page is resumed; one only in the old page is disposed.
- `<head>`: `<title>` and `<meta>` updated; a stylesheet added or removed; a `<script>` that differs means a reload (A.5).

A page reached by a `POST` (the answer to a form) is not fetched again. Its components are swapped as in A.2, and the part the server rendered stays until the next navigation.

### A.4 What the DOM holds that no state does

Taken before a swap, put back after, matched by the element's path from the island (tag and index at each level), and given up quietly where the path no longer leads to the same kind of element:

- the focused element, its selection (`selectionStart`, `selectionEnd`, `selectionDirection`) and its scroll;
- the scroll offset of each scrolled element, and the window's;
- the value of each field that no `bind:` follows and the person changed (`asParsed` already tells such a field apart);
- `<details open>`, and a `<dialog>` that is open (`showModal` again, not the attribute);
- a `<video>` or `<audio>`'s time and whether it plays.

### A.5 When the page reloads all the same

- The runtime changed (`quillo.js` differs: a new binary, or a change in what the runtime is given).
- A `client`, `preload` or `module` library was added, removed or changed. A library's globals cannot be taken back.
- A page script outside components, or `<head>`'s scripts, changed.
- A swap threw. The page says so in the console with the error and reloads. A swap left half done is worse than a reload.
- `QW.hot = false` from the page's own script (or a `data-reload` on `<html>`): an author who wants every save to reload.

The console says why each time: `quillo: reloaded: the runtime changed`.

### A.6 A reload that keeps what it can

Where the page must reload, the runtime first writes to `sessionStorage`, under the page's URL: the components' states by island and name, the fields and `<details>` of A.4, and the scroll. The next load of that URL under `--watch` reads it once and deletes it. Each island resumes with its old states as `s0` in place of `data-s`, and the rest is put back once the page has booted. This is also what the browser failed to do on the probe (the plain field and the plain form came back empty). It costs nothing in production, where the code is not there.

## Part B: what the page is told

Today's messages are `N` (a new version: reload) and `css:N:[paths]`. Proposed, on the same socket and the same `?v=&c=` plain-request fallback:

```
hot:{"v":7,"runtime":"9f2c...","defs":{"Counter":"main.qwe/Counter.3a81c0.js","Badge":"ui/badge.qwe/Badge.77e1d2.js"},"gone":["Old"],"uses":{"fmt":["Counter","Badge"]},"server":true}
err:{"v":7,"errors":[{"file":"main.qwe","line":21,"message":"expected an expression, found `/`"}]}
ok:{"v":8}
```

- `hot` lists every client definition the new version has whose compiled code changed, with its new file's URL. `server: true` says something the server renders changed, so the page should compare (A.3). `runtime` is the runtime file's hash: a page with another one reloads.
- `err` comes when a save does not load (Q-686). The page shows the error over itself (A shadow root on an element of its own, no name on `window`, closed by Escape or by the next `ok`/`hot`) and goes on running the version it has, with its state.
- A page that connects with an old `v` gets one `hot` with every change since, or a plain `N` (reload) when the server no longer has them (it keeps the last few versions' maps, as it keeps bundles).
- `N` stays, for an older page and for `--reload`.

A page whose bundle does not include a changed definition (it never shows `Badge`) skips it, unless the morph of A.3 brings one in. Then the runtime loads it as client-side navigation does today.

## Part C: what the server keeps across a reload

Under `--watch`, after the new version loads, each top-level `let` and `var` that survived the change takes the value the old version's had, by default (decided by the user, 2026-11-04, with the rule for states in A.2):

- Survived means the same name and the same type: the declared one, or the one inferred from its initializer. A changed initializer of the same type keeps the value (`let notes = []` to `let notes = ["first"]` keeps the notes added). A changed type starts it from its new initializer, as today.
- A value holding a function, a database handle, a channel, a job or anything else of the old version's, or a record whose type's definition changed (its type is no longer the same), is not carried. It starts again, and the log says which.
- The log says what was kept, in one line: `quillo: reloaded main.qwe (2 routes); kept notes (1 item), started; reset cart (its type changed)`.
- `--reload` (production) does not do this, unless the user decides otherwise (open question 3).

An in-memory database is the hard case. Keeping the connection keeps the rows, but the top level runs again: `db.exec("create table t (x int)")` then fails with "table t already exists" and the reload is refused. Two ways: keep a `:memory:` database only when the program creates its tables with `db.migrate` (which knows what it has run), or keep none and say so in the reload line: `reset: the :memory: database (use a file to keep rows across saves)`. The second is simple and honest. Open question 4.

Top-level effects run again as they do today: `every`, `schedule`, `db.migrate` (already a no-op when nothing is new), `print`. That is right: they are code, and the code is new.

## Seen again

None of the known issues bit. Q-078 (a stylesheet another tool writes) is what this study builds on: its `css:` message is the model for `hot:`.

## New issues

### Q-685. Every save under `--watch` reloads every open page, and the developer loses what they typed, opened and set

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | headless Chrome, the probe page read back after each edit | 1 |

```quillo
component Counter(label: str) {
    state count = 0
    state draft = ""
    state open = false
    <div.counter>
        <h2>{label}</h2>
        <button.inc @click={() => count = count + 1}>+1</button>
        <span.count>{count}</span>
        <input.draft bind:value={draft}/>
        <button.toggle @click={() => open = !open}>toggle</button>
        {open && <p.panel>OPEN</p>}
        <input.plain placeholder="not bound"/>
    </div>
}

route GET / =
    <html><body>
        <Counter label="Version A"/>
        <form method="post" action="/x"><input.formfield name="f"/></form>
        <details.det><summary>more</summary>hidden</details>
        <div style="height:3000px">tall</div>
    </body></html>

route GET /visit = 1
```

Under `quillo run main.qwe --watch`: click `+1` three times, type in each field, open the panel and the `<details>`, scroll down. Then change `"Version A"` to `"Version B"` and save. The page reloads: `count` is 0, every field is empty, the panel and `<details>` are closed, the focus is gone, and the scroll is 1111 px, not 1200. Then change `route GET /visit = 1` to `= 2`, a route the page never calls, and save. The page reloads again, with the same losses.

The page's dev script compares one number, `if (t !== v) location.reload()` (`dev_script` in `src/server.rs`). Nothing else is sent.

**Suggestion:** Parts A and B. In the order of work below, the reload for a change the page does not use (step 4) is the cheapest win, and A.6 keeps the fields even where nothing else is done yet.

### Q-686. A save that does not load says nothing on the page

| Kind | Detected by | Attempts |
|---|---|---|
| silent | headless Chrome after a broken save | 1 |

Save the probe with `<span.echo {{{` in it. The terminal says `main.qwe:21: error: expected an expression, found '/'` and `quillo: still serving the previous version`. The page shows nothing and goes on running the old code (its state was kept: the `window` mark set before the save was still there). Someone looking at the browser, not the terminal, keeps clicking and wonders why their change does not show. The guide says `--watch` will "show errors on the page", which today is true only for a program that has never loaded (`Loaded::Failed`), or for a runtime error in a route.

`App::reload` does wake the pages' sockets (`notify_all`), but the version is unchanged, so they send nothing.

**Suggestion:** the `err:` message of Part B and its overlay, with the state kept underneath, so fixing the typo takes the page back to where it was. Say in the guide that a failed save keeps the page as it was.

### Q-687. A reload starts the server's data again: top-level `let`s and `:memory:` databases are emptied, and nothing says so

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the probe's server fn, read before and after a save; `curl` on an in-memory database | 2 |

```quillo
let notes = []

server fn add_note(text: str) -> list {
    notes = notes + [text]
    return notes
}

server fn server_notes() -> list = notes
```

Call `add_note("hello")` from the page, then edit any line of the program. `server_notes()` was `["hello"]` and is `[]`. With `db.open(":memory:")` and `db.exec("create table t (x int)")` at the top, two inserts give 2 rows, and after a save the table has 0. The reload line says `quillo: reloaded main.qwe (2 routes)`, and nothing about the data.

A developer filling a page with test data through the app itself (a cart, a draft, a game's lobby kept in a `let`) loses it at every save. The data is not the code, and nothing in the edit asked for it to start again.

**Suggestion:** Part C.

### Q-688. One component's edit changes every later component's compiled file, and every file's URL

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, perf | `curl` of `Second.js` before and after an edit to `First` | 2 (the first edit added no block) |

```quillo
component First() {
    state a = 0
    <p.first>{a}</p>
}
component Second() {
    state b = 1
    <div.second><b>{b}</b></div>
}
route GET / = <html><body><First/><Second/></body></html>
```

`Second.js` has `W.t($b1, ...)`. Change `First`'s markup to `<p.first><i>{a}</i> {a > 0 && <b>y</b>}</p>` and save. `Second` did not change, yet `Second.js` now has `W.t($b2, ...)`: the blocks are named by their place in the program (`format!("$b{}", self.blocks.len())`), and they live in `quillo.js`'s prelude, so the runtime's file changes too. And the hash in `/_quillo/src/<hash>/` covers every file of the bundle, so after any save every file has a new URL. On the probe, each save that loaded fetched all four files again under a new hash; on a page of the 491-component UI kit, it is every component file the page uses.

So the server cannot say which definitions changed by comparing their code, and the page cannot keep what did not change.

**Suggestion:** name a block by a hash of its template (`$b_3fa9c1`, the same template giving the same name in any program), keep a definition's blocks in its own file rather than the runtime's prelude, and give each file a hash of its own in its URL (`main.qwe/Counter.3a81c0.js`). This also lets the browser cache what did not change across reloads today, before any HMR.

### Q-689. A restart under `--watch` signs the developer out

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `curl` with a cookie jar across two starts of the same program | 1 |

With no `QUILLO_SECRET`, each start makes a random signing key (`quillo: QUILLO_SECRET is not set; sessions use a random key and end when the server stops`). A session counter read `1 2`, then after a restart `1 2` again: the cookie from the first run no longer checks. A reload keeps the session (same process). But a developer restarts often: a new Quillo binary, `.env` changed (it is read once), a change of options, a crash. Each time they sign in again, on every tab.

`quillo app` already keeps its key in `.quillo/.quillo-secret` beside the program, and `quillo new`'s `.gitignore` already has `.quillo/`.

**Suggestion:** under `--watch`, with no `QUILLO_SECRET`, keep the key in `.quillo/.quillo-secret` as `quillo app` does, and say so once: `quillo: sessions signed with .quillo/.quillo-secret (development only; set QUILLO_SECRET in production)`. Without `--watch`, keep today's random key, so a server never quietly uses a file it was not told about.

## Proposed order of work

Each step is useful by itself and testable by itself. Every slice gets Rust tests in `tests/feedback54.rs` and browser tests, and `runtime.js` keeps its 99.50%.

1. **Q-688: stable block names and per-file hashes.** A compiler change with no behaviour change. Test: a bundle's unchanged definitions keep their file text and URL across an edit to another.
2. **Q-686: `err:` and the overlay.** Server: the failed reload's diagnostics, kept until the next good one. Runtime: the overlay in a shadow root. Test: a broken save shows the overlay, the state is kept, and a fixing save clears it.
3. **Q-689: the dev session key file.** Small and separate.
4. **Q-685 step 1: no reload for a change the page does not use.** Server: compare each client definition's hash between versions. `hot:` with nothing in `defs` and `server: true`: the page fetches itself, compares (A.3's first step) and stops there when equal. Test: an edit to an unused route leaves a component's state as it was.
5. **A.6: the reload that keeps.** States, fields, `<details>` and scroll through `sessionStorage`. Test: the probe's table, every row kept after a forced reload.
6. **A.2: swapping components.** The instance registry, the swap, the states by name with the initializer hash, A.4's DOM state, the list of who calls which view or function. Tests: a markup edit keeps `count`; a changed initializer of the same type keeps the value; a type change resets that state only, with a console line; a state initialized to `null` keeps any value; an inner component keeps its state; `@mount` runs again and its cleanup ran; a swap that throws reloads.
7. **A.3: the morph of the server's HTML** with props given to kept islands. Test: the probe's `"Version A"` to `"Version B"` edit keeps every row of the table.
8. **Part C: the server's data kept.** Test: `notes` kept across an unrelated edit and across a change to its initializer; reset when its type changes; a `let` holding a function or a handle not carried; the log line.
9. **The guide:** `--watch`'s row in the options table, a short "What a save keeps" section (the state table of A.2, the order rule for inner components, what reloads), and the overlay.
10. **quillolang/devtools:** after a swap the extension hears `gone` and `component` for each instance made again. A `swapped` event with the old and new ids would let it keep its selection.

## Open questions for the user

1. **Server data (Part C) under `--watch`:** decided (2026-11-04): kept by default, no marking, while the `let` survives the change with the same type. Still open: a way to ask for a fresh start without restarting (typing `r` and Enter in the terminal).
2. **A state whose initializer changed:** decided (2026-11-04): it keeps its value while its type is the same, as Flutter does; only a changed type starts it again.
3. **`--reload` in production:** carry the server's data across a deploy too? I recommend not yet: production data belongs in a database, and a silent carry across a deploy is harder to reason about.
4. **`:memory:` databases:** reset and say so (recommended), or keep them when the program uses `db.migrate`?
5. **Opting out of HMR:** is `QW.hot = false` or `<html data-reload>` enough, or should there be a `--watch=reload` option?

## Smaller notes

- The reload's scroll restore landed at 1111 px of 1200: the browser restores before the page has its full height. A.6 restores after boot, which fixes it.
- `boot.js` sets `window.quillo = { server, components, rpc }` under `--watch`, a name on `window` that the guide's rule and Q-486 argue against. The HMR registry should go in the runtime's closure, as Part C of FEEDBACK42 did for tools, not on `window`.
- Whether a component's `style` block can be swapped like a stylesheet (Q-078's `css:` path, by its `style_id`) was not probed. Today it reloads the page with everything else. It belongs with step 6.

## Where the AI struggled

| What was tried | What happened | Whose mistake |
|---|---|---|
| `session.visits` and `session.visits = n` in the probe | `quillo check`: "fn has no field `visits`" and "cannot set `visits` on `session`: use `set_session({ visits: ... })`". The second message gave the fix; the first could say that `session` is a function, `session().visits` | the AI's, with a message that helped |
| Clicking `+1` after the page had reloaded scrolled down | The click landed off screen, so the next counts looked wrong. The driver now scrolls each target into view first | the AI's (the driver) |
| Adding nested elements to `First` to show the block numbering | No new block: nested static markup is part of the same template. A conditional (`{a > 0 && <b>y</b>}`) makes one | the AI's |
| Reading the compiled files with the hash from an earlier run's log | `not found`: each run has its own bundles, and the hash is in the page's loader (`/_quillo/b/<hash>.js`), not in the page | the AI's |

## Edge cases checked that behave well

- A broken save keeps the old version serving, and the open page keeps its state and keeps working.
- A session survives a reload under `--watch`.
- `quillo check`'s message for setting a field on `session` names the function to use.
- Each component's compiled file defines it in one assignment and names its states, so a swap needs no change to how components are written.

## Scripts

The probe (`main.qwe`), run with `quillo run main.qwe --watch --port 8732`:

```quillo
// HMR probe: what a --watch reload keeps.
let notes = []
let started = now()

server fn add_note(text: str) -> list {
    notes = notes + [text]
    return notes
}

server fn server_notes() -> list = notes

component Counter(label: str) {
    state count = 0
    state draft = ""
    state open = false
    <div.counter>
        <h2>{label}</h2>
        <button.inc @click={() => count = count + 1}>+1</button>
        <span.count>{count}</span>
        <input.draft bind:value={draft}/>
        <span.echo>{draft}</span>
        <button.toggle @click={() => open = !open}>toggle</button>
        {open && <p.panel>OPEN</p>}
        <input.plain placeholder="not bound"/>
        <button.save @click={() => add_note(draft)}>save</button>
    </div>
}

route GET / =
    <html><head><title>probe</title></head><body>
        <Counter label="Version A"/>
        <form method="post" action="/x"><input.formfield name="f"/></form>
        <details.det><summary>more</summary>hidden</details>
        <div style="height:3000px">tall</div>
        <p.notes>{len(notes)} notes since {started}</p>
        <p.sess>{session().visits ?? 0}</p>
    </body></html>

route GET /visit = {
    let n = (session().visits ?? 0) + 1
    set_session({ visits: n })
    return n
}
```

The driver, `DIR=<the probe's folder> URL0=http://127.0.0.1:8732/ node feedback/tools/drive.mjs drive.mjs`:

```js
import { readFileSync, writeFileSync } from "node:fs";
const DIR = process.env.DIR, URL0 = process.env.URL0;
const edit = (from, to) => { const p = DIR + "/main.qwe"; writeFileSync(p, readFileSync(p, "utf8").replace(from, to)); };
export default async ({ page, sleep }) => {
  const out = {};
  await page.go(URL0);
  await page.booted();
  await page.ev(`window.__mark = 1; return 1`);
  const clickSel = async (sel) => {
    await page.ev(`document.querySelector(${JSON.stringify(sel)}).scrollIntoView({block:"center"}); return 1`);
    await sleep(50);
    const c = await page.center(sel); await page.click(c.x, c.y); await sleep(80);
  };
  await clickSel(".inc"); await clickSel(".inc"); await clickSel(".inc");
  await clickSel(".draft"); await page.type("hello");
  await clickSel(".toggle");
  await clickSel(".save");
  await clickSel(".plain"); await page.type("typed");
  await clickSel(".formfield"); await page.type("form text");
  await page.ev(`document.querySelector(".det").open = true; await fetch("/visit"); await fetch("/visit"); return 1`);
  await page.ev(`window.scrollTo(0, 1200); return 1`);
  await sleep(200);
  const snap = () => page.ev(`return {
    mark: window.__mark ?? null, count: document.querySelector(".count")?.textContent,
    draft: document.querySelector(".draft")?.value, plain: document.querySelector(".plain")?.value,
    form: document.querySelector(".formfield")?.value, panel: !!document.querySelector(".panel"),
    details: document.querySelector(".det")?.open, scrollY: Math.round(scrollY),
    focus: document.activeElement?.className, label: document.querySelector("h2")?.textContent,
    server_notes: (await (await fetch("/_quillo/rpc/server_notes", { method: "POST",
      headers: { "content-type": "application/json" }, body: "[]" })).text()),
    sess: document.querySelector(".sess")?.textContent }`);
  out.before = await snap();
  edit('label="Version A"', 'label="Version B"');            // the route's markup
  await sleep(2500); out.after_markup_edit = await snap();
  edit('return n\n', 'return n + 0\n');                      // a route the page does not use
  await sleep(2500); out.after_unrelated_route_edit = await snap();
  await page.ev(`window.__mark = 4; return 1`);
  edit('<span.echo>', '<span.echo {{{');                     // a save that does not load
  await sleep(2500); out.after_broken_edit = await snap();
  edit('<span.echo {{{', '<span.echo>');
  await sleep(2500); out.after_fix = await snap();
  return out;
};
```

The session across a restart:

```sh
for run in 1 2; do
  quillo run main.qwe --watch --port 8737 & P=$!; sleep 1.2
  echo "run $run: $(curl -s -b jar -c jar http://127.0.0.1:8737/visit) $(curl -s -b jar -c jar http://127.0.0.1:8737/visit)"
  kill $P; wait $P
done
# run 1: 1 2
# run 2: 1 2
```
