# Feedback from reviewing what browser developer tools for Quillo need, and building them (quillolang/devtools)

| | |
|---|---|
| App | None in Quillo: the work is a browser extension, [quillolang/devtools](https://github.com/quillolang/devtools) (local `~/dev/quillo-devtools`, not pushed yet): a **Quillo** panel in Chrome's and Edge's developer tools. The question: what can such a tool read from a Quillo page today, what does it have to work around, and what should Quillo's runtime offer it |
| Quillo | 0.6.0, release build of 487c518's source (it reports `5a919f0`, the commit it was built on before 487c518 was made). main is now ab9e600, the merge of 487c518, with the same `src/client/`, `src/client.rs` and `src/server.rs`. The extension's tests also pass on the 0.6.0 release (`991e96c`) |
| Model | Claude (Opus 5.5) |
| Tested with | Headless Chrome 156 over the protocol (`feedback/tools/chrome.mjs` for the probes, the extension's own `tests/browser/cdp.mjs`, which loads the extension through `Extensions.loadUnpacked`), the starter's `spa` sample plus a probe page, the extension's fixture app (two `Counter`s each holding an `Inner`, a `live` state, a decimal in a map, a server fn that answers an error value, an island the browser makes, a channel), and a page whose own scripts have names the runtime uses. Read: [src/client/runtime.js](../../src/client/runtime.js) (`Inst`, `island`, `resume`, `report`, `boot`, `rpcRaw`, `liveLoop`, `chanConnect`), [src/client.rs](../../src/client.rs) (`runtime_file`, `wrap`, the development bundle), [src/server.rs](../../src/server.rs) (`/_quillo/dev`, `/_quillo/events`), USERGUIDE.md (Components, Debugging, Channels, Stopping and health) |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section and the probe scripts are at the end. Numbers start at Q-481, after FEEDBACK41.md's Q-480.

What worked well: much of what a developer wants to see already crosses the wire in a readable form. A server fn call is a `POST /_quillo/rpc/<name>` with a JSON array, its answer `{"ok": ...}` or `{"err": ...}` with the topics it read and an `x-request-id` that matches the server's log line. A live state loading again is the same call, marked `x-quillo-live`. The live socket's messages are plain JSON (`{"seq":2,"server":"aa7154e3","changed":["db:tasks"]}`). The server's HTML says which component each island is, with its props and the state it was rendered with. Under `--watch`, the Sources panel already shows each `.qwe` file through source maps, `/_quillo/health/queries` times every statement, and `/_quillo/dev` gives the program's version. Reading the runtime's own objects through the debugger protocol took 30 ms for four components, and setting a state there re-rendered the page at once.

## The answer

A developer-tools panel for Quillo can be built today, and one now exists: [quillolang/devtools](https://github.com/quillolang/devtools), version 0.2.0, with five tabs (Components, Calls, Live, Errors, Server) and a pane beside the Elements panel. Version 0.2.0 adds where each prop comes from (`value ← <Parent>.a`, `label ← <Parent>'s prop title ← <Grand>.g`) and a component's states and lets with what each reads. It is tested in Chrome against both builds above, in production and under `--watch`. 56 tests cover every line of the extension (Part B).

It works **from outside** Quillo, because the runtime offers tools nothing to hold:

1. **A component's state as it is now cannot be read except through the browser's debugger** (Q-483). Each island's `data-s` is the state the server rendered, and the runtime never writes it again: after one click the page said `clicks: 4` and `data-s` still said `n: 3`. The extension's **Current state** attaches `chrome.debugger`, finds the runtime's closure through a function the runtime leaves on `window`, and asks V8 for every object made by the runtime's private class `Inst`. That works in production and under `--watch`, on 0.6.0 and on main. But it depends on names Quillo never promised (`Inst`, `.sigs`, `.def.$name`, `Live.subs`, `__quillo_resume`), shows the browser's "debugging this browser" bar, and cannot be done in Firefox at all.
2. **Errors reach a tool as a bare message, and only through a global the tool must make first** (Q-484). Under `--watch` the console line has the `.qwe` file, line and calls. `window.__quilloErrors` gets the message alone. A component made again because the server's HTML differed is counted, and only the console says why.
3. **Channels over WebTransport are invisible to every tool** (Q-485). The browser's own protocol reports that the session opened, not what it carries.
4. **Under `--watch`, 236 of the runtime's names are globals of the page** (Q-481). A page's own `function report()` is replaced by Quillo's. A script loaded later with its own `function run()` silently stops every component not yet taken up. Both happen under `--watch` only: production keeps them in a closure.

The fix for 1 to 3 is one small interface in the runtime, off until a tool asks for it, carried by DOM events rather than globals (Part C). It would also replace the `__quillo*` counters on `window`, which the guide's rule against JavaScript on `window` already argues against (Q-486). Part A lists what a page shows a tool today. Part B describes the extension. Part C is the interface. The issues follow, then an order of work.

## Part A: what a Quillo page shows a tool today

| Channel | What it carries | Production | `--watch` | Reachable by |
|---|---|---|---|---|
| `<quillo-island data-c data-p data-s data-l data-v data-e>`, `[data-qi]`, `data-qd` | Each component on the page: name, props, the state the server rendered, live states (function, arguments, topics), whether it runs by itself, deferred | yes | yes | any script, any extension |
| `el.__q` on an island | Whether the browser has taken it up (an expando property) | yes | yes | the page's own world only |
| `window.__quilloResumed`, `__quilloRemade`, `__quillo_booted` | Counts of components taken up and made again; whether the runtime has started | yes | yes | the page's world |
| `window.__quilloErrors` | Each error's message, if the page made the list before the error | if made | if made | the page's world |
| `window.quillo` | `{ server, components, rpc }`: call a server fn from a script (documented) | when there are server fns | same | the page's world |
| `window.__quillo_rt` | The shared runtime's exports: `B`, `C` (the components' definitions), `G`, `W`, `rpc`, `boot`, and whatever names the program uses (`add`, `match`, `link`...) | when pages share one runtime file | no | the page's world |
| The runtime's closure | Every component (`Inst`), its signals (`.sigs`), the live subscriptions (`Live.subs`), channels (`Chan`), the calls in flight (`Sent`) | through the debugger protocol only | same, and its functions are globals (Q-481) | `chrome.debugger`, a DevTools protocol client |
| `POST /_quillo/rpc/<name>` | Arguments, answer, `topics`, `seq`, `x-request-id`; `x-quillo-live`, `x-quillo-had`, `idempotency-key` on the request | yes | yes | the Network panel; `chrome.devtools.network` |
| WebSocket `/_quillo/live` | What the page follows (`?t=`), each change (`{seq, server, changed}`) | yes | yes | the Network panel; a script that wraps `WebSocket` before the runtime starts |
| `/_quillo/wt` | Channels: over WebTransport by default, a WebSocket with `--no-wt` or when WebTransport fails | WebSocket only | same | WebTransport: nothing (Q-485) |
| WebSocket `/_quillo/events` | The program's version; a change reloads the page, a stylesheet is swapped | no | yes | as for `/_quillo/live` |
| `/_quillo/src/<hash>/<file>.qwe/<Name>.js` and source maps | The program's own code, by file, mapped to `.qwe` lines | no | yes | the Sources panel |
| `GET /_quillo/health`, `/_quillo/ready` | Up, and what the program needs | yes | yes | anyone |
| `GET /_quillo/health/queries` | Each statement's count and times | `{"enabled": false}` unless switched on | on | anyone (on 0.6.0: 404) |
| `GET /_quillo/describe` | Routes, and server fns (`x-quillo-server-fns`, a map of name to signature) | with `--describe` | with `--describe` | anyone |
| `GET /_quillo/dev` | The program's version number | 404 | yes | anyone |

What nobody can read from outside: the state of a component the browser has taken up, the props of a component inside another (written as `data-p="{}"`; the outer one passes them), anything about a component the browser made itself (no `data-*` but `data-c`), which component an error happened in, why a component was made again, which live states a change made load again (only the calls that follow), and channel messages over WebTransport.

## Part B: the extension

[quillolang/devtools](https://github.com/quillolang/devtools), `extension/`, Manifest V3, no dependencies. Chrome and Edge 111 and later.

| Tab | How it reads it | Limits |
|---|---|---|
| **Components** | `inspectedWindow.eval` of a function that lists the islands and their nesting each second, and the picked island's `data-p`, `data-s`, `data-l`. A box over the island on hover; *Show in Elements* uses DevTools' `inspect()` | The server's state, labelled as such, unless **Current state** is on |
| **Current state** | `chrome.debugger`: `window.__quillo_resume` (or `quillo.rpc`, or a function of `__quillo_rt`, on 0.6.0) → `[[Scopes]]` → `Inst`, `Live`, `toJSON`, `untagDeep` → `Runtime.queryObjects(Inst.prototype)`; values written with the runtime's own `toJSON`; ✎ sets a state with `untagDeep` and `Sig.set` | Private names (Q-483); the browser's bar; Chrome only |
| **Calls** | `chrome.devtools.network`: each `/_quillo/rpc/` request, its arguments (the `args` part when files are sent), its answer (value, error value, patch, failure), flags for live loads, idempotent calls and resends | Only while the developer tools are open |
| **Live** | A content script in the page's world, before its scripts, wraps `WebSocket` for `/_quillo/live`, `/_quillo/wt` and `/_quillo/events` only, and passes each message to a second content script by a DOM event with a JSON string | Channels over WebTransport are not heard (Q-485) |
| **Errors** | The same script makes `window.__quilloErrors` on a Quillo page, as a list whose `push` also says so | The message alone (Q-484) |
| **Server** | `fetch` from the panel: health, ready, the statement times sorted by total time, `/_quillo/dev`, `/_quillo/describe` | — |
| **Where props come from; states and lets** (0.2.0, with Current state on) | The compiled component's own text (`String(inst.def)`): `const $a_2 = $i.sig("a", ...)` is a state, `W.live($i, "items", ...)` a live state, `const $x = W.memo(...)` a let worked out again, `let $x; $x = ...` a let; only statements at the component's own level count (not a handler's). Each child prop is a getter in the child's `props` (`get "value"() { return $a_2.v; }`) or a fixed value; its text names what it reads, followed through the parents' props. A let's value is read from a closure that keeps it (`[[Scopes]]` of a memo's function or a child prop's getter), or shown as written | Compiled code's shape (Q-483); a loop's item (`$row_10`) and a view's parameter have no name left; a let the browser did not keep has no value |
| Elements pane | `setExpression` with the selected element: the island it is in, its outer ones, props, state, live | The server's state |

Tests: 32 unit tests in Node and 24 in headless Chrome, which load the real extension through the protocol's `Extensions.loadUnpacked` (Chrome 137 and later take no `--load-extension`). The browser tests run the fixture app in production (with `--describe` and `--no-wt`) and under `--watch`. They open the panel as a page of the extension's, with `chrome.devtools` stood in for and the real `chrome.debugger`, and check each tab with a screenshot. V8 coverage from Node and from Chrome, merged: 1,201 of 1,201 lines of `extension/`. It is green on both Quillo builds (and on main's 54dd1bd, which another session has moved on since 487c518).

## Part C: an interface for tools in the runtime

What the extension does by debugger and by wrapping `WebSocket` should come from the runtime itself, in a form that holds across releases. A design, following the rule that a program's JavaScript does not go on `window`:

**Off until asked, at no cost when off.** A tool asks by setting an attribute before the runtime starts: its content script, at `document_start`, writes `<html data-quillo-devtools="1">`. `boot()` reads that attribute once, and does nothing else when it is absent. Measure the absent case with `bench/` to show it costs nothing measurable.

**Messages as DOM events, with JSON text.** The runtime dispatches `quillo:devtools` on `document`, its `detail` a JSON string, so that it crosses into an extension's isolated world. A tool asks with `quillo:devtools-ask` (`{ id, op, ... }`) and gets an answer with the same `id`. No global; a page's own script can listen too, which is no more than it can do today.

| Message | When | Carries |
|---|---|---|
| `hello` | the runtime starts | `protocol` (1), Quillo's version, `dev` (`--watch`), `base` (the mount) |
| `component` | one is taken up or made | an `id`, its name, the `id` of the one it is in, its props and state as wire JSON (`$dec`, `$int`, `$quillo_err`); for each prop, what it reads in the component around it (`{ state: "a" }`, `{ prop: "title" }`, `{ let: "double" }`, fixed, a loop's item, a view's parameter by its name); its states and lets by their names in the source, each with its kind and what a derived one reads. The compiler knows all of this: a small table per component, written into the bundle only for tools |
| `final` | a component taken up that nothing can change (no handler, live state or timer), whose code the browser is about to let go | `id` |
| `state` | a state is set | `id`, the state's name, the value |
| `gone` | a component leaves the page | `id` |
| `error` | `report()` | the message, the `.qwe` place and calls under `--watch` (what `placed` already works out), the component's `id` |
| `remade` | a component is made again | its name, what the server wrote and what the browser makes (what `wanted` already holds under `--watch`) |
| `call` | a server fn answers | name, arguments, how long it took, ok, error value or failure, `live`, `idempotent`, resent |
| `live` | a change comes | `seq`, the topics, the live states it loads again (component `id` and name) |
| `channel` | a message goes or comes, over either transport | in or out, the channel, the value; joined, refused, left |

| Ask | Answer |
|---|---|
| `list` | every component now, as `component` messages |
| `set` `{ id, name, value }` | sets the state; under `--watch` only, or with an option that allows it in production |
| `where` `{ id }` | marks the component's element with `data-quillo-devtools-id` for the tool to find, and removes the mark at the next `where` |

The extension would use these when `hello` comes and fall back to what it does now otherwise. The debugger would only be needed on releases without the interface.

Whether production pages answer at all is the user's decision. A page can already be read by anyone with the debugger, so reading state in production adds little. Setting it there should stay off unless switched on.

## Seen again

None.

## New issues

### Q-481. Under `--watch` the runtime's 236 functions are the page's globals, so a page's script and Quillo replace each other's

| Kind | Detected by | Attempts |
|---|---|---|
| silent, development only | the extension's state reader not finding `toJSON` under `--watch`, then a probe | 2 |

In production the runtime is one closure (`(function () { ... })()`), and a page's bundle gets what it uses through `window.__quillo_rt`. Under `--watch` the development bundle loads `quillo.js` as a plain script, and the program's files as more plain scripts beside it ([client.rs, the development bundle](../../src/client.rs)). A plain script's top-level `function` declarations become properties of `window`. A fresh window compared with the page's after the runtime started:

| | Own properties of `window` that a fresh window lacks |
|---|---|
| production | 8: `__quillo_pf`, `__quillo_q`, `__quillo_redo`, `__quillo_rt`, `quillo`, `__quillo_resume`, `__quilloResumed`, `__quillo_booted` |
| `--watch` | 236: those, and `add`, `eq`, `report`, `resume`, `boot`, `run`, `rpc`, `toJSON`, `match`, `field`, `index`, `link`, `wire`, `put`, `memo`, `effect`, `flush`, `island`, `decAdd`, ... |

None of them replaces one of the browser's own functions. But a page's own scripts share those names, and whoever is loaded last wins:

```quillo
// main.qwe
route GET /assets/*path = file("public/" + path)

component Tally() {
    state n = 0
    <button.tally @click={() => { n = n + 1 }}>tally {n}</button>
}

route GET / =
    <html><head><title>clash</title><script src="/assets/own.js"></script></head><body>
        <Tally/>
        <div.later><Tally/></div>
        <button.widget onclick="const s = document.createElement('script'); s.src = '/assets/widget.js'; document.head.append(s); this.textContent = 'widget loaded'">load widget</button>
    </body></html>
```

```js
// public/own.js: the page's own helper
function report(x) { return "the page's own report"; }
// public/widget.js: a script that comes later (a chat widget, analytics)
function run() {}
```

The probe reaches the first `Tally`, so the runtime starts, then calls `report("x")`. It then loads the widget and clicks the second `Tally` twice:

| | `report("x")` gives | second `Tally` after two clicks | console |
|---|---|---|---|
| production | `the page's own report` | `tally 2` | nothing |
| `--watch` | `undefined`: Quillo's `report` ran instead | `tally 0`: `resume` called the widget's `run`, and the component never started | `quillo: x` |

Nothing says a name was taken. The program behaves differently under `--watch` than in production, which is the one thing `--watch` must not do: a developer chasing the second case would see a dead component only while developing, and a clean one in production.

**Suggestion:** Give the development runtime the same boundary as production. Two ways, both keeping each file's own script and source map:

- Load the development bundle as ES modules: `quillo.js` exports what the program uses, and each `<file>.qwe/<Name>.js` imports it. A module's top level is its own, so nothing reaches `window`, which is also what the rule against JavaScript on `window` asks for. The source maps stay as they are.
- Or wrap `quillo.js` in a function as production does, and have each program file take its names from a parameter, as `wrap` already writes them (`(function ($rt) { const { ... } = $rt; ... })(...)`). Each source map moves by the one line the wrapper adds.

Either way, a test: the probe above, under `--watch`, gives `the page's own report` and `tally 2`.

### Q-482. `sleep(0.2)` sleeps 0.2 ms, and nothing says so

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the extension's Calls tab showing 0.8 ms for a call meant to take 0.2 s | 1 |

`sleep` takes milliseconds. Python's `time.sleep` takes seconds, and a model (this one) or a person coming from it writes `sleep(0.2)` for a fifth of a second:

```quillo
server fn a() -> str {
    sleep(0.2)
    "a"
}
server fn b() -> str {
    sleep(200)
    "b"
}
```

`quillo check`: `main.qwe: ok (1 routes, 2 functions, 0 components)`. Calling each: `a` answers in 19 ms, `b` in 216 ms.

**Suggestion:** `quillo check` warns when `sleep`'s argument is a number literal below 1: "`sleep` takes milliseconds: `sleep(0.2)` waits 0.2 ms; for 0.2 seconds write `sleep(200)`". A fraction of a millisecond is not a wait anyone means.

### Q-483. A tool cannot read a component's state as it is now: only the debugger, through names Quillo does not promise, can

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | building the Components tab | 3 |

The runtime keeps components in its closure: `Inst` objects, each with `.sigs`, the signals of its states. A component's element gets an expando `__q = true` and nothing that leads to its `Inst` ([runtime.js, `island` and `resume`](../../src/client/runtime.js)). The page's attributes do not follow the state:

```quillo
component Counter(start: int = 0, label: str = "count") {
    state n = start
    <button.inc @click={() => { n = n + 1 }}>{label}: {n}</button>
}
// <Counter start={3} label="clicks"/>
```

After one click, the button says `clicks: 4`, and the island's `data-s` still begins `{"n":3,`. A live state's new rows are not in `data-s` either: after `add_task` from another client, the page showed 3 tasks and `data-s` held 2. A tool reading the page shows the server's first render as if it were the component's state, with nothing to tell it otherwise.

What does work is the debugger protocol: the closure of a function the runtime puts on `window` (`[[Scopes]]` of `__quillo_resume`, or of `quillo.rpc` on 0.6.0, which has no `__quillo_resume`), then `Runtime.queryObjects` on `Inst.prototype`:

```
33 ms [{"name":"Counter","props":{"start":"3","label":"clicks"},"state":{"n":"4", ...}},
       {"name":"Inner","props":{"value":"4"},"state":{"clicks":"0"}}, ...]
after set: clicks: 100
```

The extension does this. It needs the `debugger` permission, shows the browser's "started debugging this browser" bar while it is on, cannot run in Firefox (no `chrome.debugger`), and leans on `Inst`, `.def.$name`, `.props`, `.sigs`, `Sig.v`, `Sig.set`, `.el`, `.dead`, `Live.subs`, `toJSON` and `untagDeep`. A rename in the runtime would break it. The extension checks for `Inst` and says it needs updating rather than showing something wrong, but it cannot tell a changed meaning. Under `--watch` the runtime's functions are found as globals (Q-481), not in a closure, so it looks in both.

The extension's 0.2.0 goes further on the same footing: where each prop comes from and what each let reads are read from the compiled function's text (`$i.sig("a", ...)`, `W.memo(...)`, a child prop's `get "value"() { return $a_2.v; }`), so a change in how Quillo compiles components breaks that too. Two things are not in the compiled code at all: the name of a loop's item (`for k in xs` becomes `$row_10`) and what a view passes on (its `$p.x` is the view's, not the component's). A let used only while the component was made is not kept by the browser, so its value cannot be read, only its text.

**Suggestion:** Part C's interface: `component` (with each prop's source and the states and lets, which the compiler knows by their source names) and `state` messages, `list` and `set`. Until then, if the runtime keeps its private names, a comment at `Inst` and `Sig` saying a tool reads them would at least warn whoever renames them.

### Q-484. An error reaches a tool as a bare message, through a list the tool must make, and why a component was made again reaches only the console

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | building the Errors tab | 1 |

`report()` ([runtime.js](../../src/client/runtime.js)) logs `quillo: ui/probe.qwe:25: cannot add null and int` with the calls under `--watch` (Q-173's fix), and pushes `String(e.message)` onto `window.__quilloErrors`, if the page made it:

```
console:            quillo: ui/probe.qwe:25: cannot add null and int  QuilloError: ... at ... Counter.js:22:12
__quilloErrors:     "cannot add null and int"
```

So a tool gets the message without the place the console has, nor the component it happened in. It must also create a global before the runtime starts, which is what the extension's content script does on Quillo's pages. A component made again is counted in `__quilloRemade`, and under `--watch` the reason (`where it wrote <p>, the browser makes <div>`) is a `console.warn` only. A tool cannot read the console without the debugger.

**Suggestion:** Part C's `error` message, with what `placed()` works out and the component; `remade` with what `wanted` holds. Then `__quilloErrors` and the counters can go (Q-486), once `feedback/tools/chrome.mjs` and the tests that read them listen for the events instead.

### Q-485. Channel messages over WebTransport cannot be seen by any tool

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the extension's channel test | 1 |

Chrome has WebTransport, so a page's channels go over it by default. On the fixture's chat page, a message sent and broadcast back:

| | `__quilloChannelVia` | what a script that wraps `WebSocket` hears | what the protocol's `Network` domain reports |
|---|---|---|---|
| default | `webtransport` | nothing | `webTransportCreated`, `webTransportConnectionEstablished`; no message |
| `--no-wt` | `websocket` | `["j","room"]` out, `["j","room",true]` in, `["m","room",{"text":"hello"}]` out, `["m","room",{"from":"d17510b104ab","text":"hello"}]` in | `webSocketFrameSent` twice, `webSocketFrameReceived` twice |

So a developer debugging a game or a chat sees nothing of its messages, in the Network panel or anywhere else, unless they know to run with `--no-wt`, which the guide does not suggest for this.

**Suggestion:** Part C's `channel` message, from `chanSend` and `chanGot`, over either transport. Until then, a line in the guide's Channels section: "to see channel messages in the browser's Network panel, run with `--no-wt`".

### Q-486. Quillo puts up to 14 names on `window`, one of them the runtime's internals

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, design | the inventory for Part A | 0 (read) |

| Name | Set by | For |
|---|---|---|
| `quillo` | the bundle, when there are server fns | documented: `quillo.server.<fn>()` from a script |
| `__quillo_rt` | the shared runtime file | the page bundles' runtime: `B`, `C`, `G`, `W`, `rpc`, `boot` and the names the program uses |
| `__quillo_resume`, `__quillo_swap` | the runtime, the deferred-state script | a deferred island swapped in |
| `__quillo_booted`, `__quillo_q`, `__quillo_redo`, `__quillo_at` | the loader in server.rs and `boot` | events held until the bundle comes |
| `__quillo_pf`, `__quillo_nav` | the prefetching script, the router | prefetch once; links inside islands are the router's |
| `__quilloResumed`, `__quilloRemade` | the runtime | counts, for tests |
| `__quilloChannelVia` | the runtime | which transport channels use |
| `__quillo_i18n` | the page, when the program has translations | the locale's messages and plural rules |
| `__quilloErrors` | the page (read by the runtime) | errors, for tests |

The guide's own rule for browser-library features (`use ... with module`: exports kept in a private registry, not on `window`) does not hold for the runtime itself. `__quillo_rt` hands any script on the page `boot`, `rpc`, the component definitions (`C`) and `W`, which every compiled line calls into. A bundle loaded after a script changed `__quillo_rt.W` would run with the change. Each name is also one a page's own script could take by accident.

**Suggestion:** Keep `quillo`, which is documented. Move the counters and the error list to Part C's events (Q-484). Keep the loader's handoff (`__quillo_q`, `__quillo_redo`, `__quillo_at`, `__quillo_booted`) in one object under a `Symbol.for("quillo")` key, and the same for `__quillo_rt`, or load bundles as modules (Q-481's first way), so that a module, not `window`, hands bundles the runtime.

### Q-487. The guide calls `data-s` the component's state; it is the state the server rendered, and an inner island's `data-p` is empty

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading the page while building the Components tab | 1 |

USERGUIDE.md (Components, under how a page resumes): "A component is an element `<quillo-island data-c="Name" data-p="{props}" data-s="{state}">`". Read as documentation of the page, that says `data-s` holds the state, and a tool or a test that reads it gets the first render (Q-483's repro). Two more cases are not said:

- an island inside another is written `data-p="{}"`: its outer one passes the props when it runs (in the probe, `<Inner value={n}/>` inside `<Counter>`);
- a component made in the browser (`{extra && <Inner value={99}/>}`, after a click) has `data-c` only.

**Suggestion:** "`data-s`, the state the server rendered it with (not written again as the state changes); `data-p`, its props, empty for a component inside another, whose outer one passes them; a component made in the browser carries `data-c` alone".

## Proposed order of work

1. **Q-481**, the development runtime in its own scope. It is a real difference between `--watch` and production, and it is in the way of Part C, whose messages should come from one runtime shape.
2. **Q-482**, the `sleep` warning, and **Q-487**'s and **Q-485**'s lines in the guide. Small.
3. **Part C**, behind the attribute, with `bench/` showing no cost when it is absent: `hello`, `component`, `state`, `gone`, `list` first (what Q-483 needs), then `error` and `remade` (Q-484), `call`, `live`, `channel` (Q-485), then `set`.
4. **The extension uses the interface** when it is there, and keeps the debugger path for 0.6.0. It goes to the Chrome Web Store and Edge Add-ons then, not before: a store listing for a tool that needs `debugger` for its main feature is a harder review.
5. **Q-486**: the counters and the error list removed once the tests listen for events; the loader's names in one object.

Steps 1 and 2 need no design decisions. Step 3 needs Part C agreed, and the production question answered.

## Smaller notes

- `/_quillo/describe` is off under `--watch` unless `--describe` is given. Under `--watch` it could be on, from this machine, as `/_quillo/health/queries` is: the Server tab would list routes and server fns with no flag.
- `x-quillo-server-fns` in the describe document is a map of name to signature; the guide's mention reads as if it were a list. The panel first iterated it as a list and threw.
- A page gives no Quillo version (the `server: Quillo` header has none, which is right for production). Part C's `hello` would.
- `/_quillo/health/queries` answers 404 on 0.6.0. The panel says "Not answered (404)". Fine, but a tool for several releases needs `hello`'s version to know what to ask for.
- Chrome 137 and later ignore `--load-extension` in branded builds (checked on 156: the content script did not run). Tests that load an extension must use `--remote-debugging-pipe` with `--enable-unsafe-extension-debugging` and `Extensions.loadUnpacked`. Worth knowing for `feedback/tools/` if it ever loads one.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Read `data-s` as the component's state | The guide calls it `{state}` | Showed `n: 3` while the page said 4 (Q-483, Q-487) | 1 probe |
| `sleep(0.2)` in the fixture's slow server fn | Python's seconds | The call took 0.8 ms; the Calls tab's time looked like a bug in the extension (Q-482) | 1 run |
| An inline `<script>function report(x) { return ... }</script>` in markup | JavaScript habit | The braces are Quillo's: `quillo check` warned "`{{ ... }}` here is a block" and the page broke; moved the scripts to files | 1 check |
| `public/own.js`, expecting it served | Other frameworks serve `public/` | 404 and a refused script; `route GET /assets/*path = file("public/" + path)` | 1 run |
| `x-quillo-server-fns` iterated as a list | The guide names it beside the paths | `for...of` threw on a map, and the Server tab stopped after its heading | 1 test |
| The state reader started from `window.__quillo_resume` only | It is on main | 0.6.0 has no `__quillo_resume`: every state test timed out there; added `quillo.rpc` and `__quillo_rt` | 1 test run |
| Looked for the runtime's functions in its closure under `--watch` | They are there in production | Under `--watch` they are globals; found Q-481 | 1 test, 2 probes |
| `rm -f app.db*` in a zsh `&&` chain, in a fresh folder | bash habit | zsh stopped the chain on "no matches found", and the `--watch` server never started | 1 run |
| `page.click(...(await page.center(sel)))` | Assumed an array | `center` gives `{x, y}` | 1 run |
| `--load-extension` for the browser tests | It used to work | Removed in Chrome 137; `Extensions.loadUnpacked` over the pipe | 1 probe |
| Read a component's `let`s from every `let $x_N;` in its compiled text | They looked alike | Counter listed `l`, the `let` of its click handler; only the component's own level (inside its function's braces, depth 1) counts | 1 run |
| Took a child prop's `$p.x` as its parent component's prop | Components pass props that way | A `view` between has a `$p` of its own: `<Leaf n={x + 1}/>` inside `view Wrap(x)` came out as "Grand's prop x, not given". A prop the component does not declare is now said to be made inside its markup | 1 run |
| `<i.leaf>{n}</i.leaf>` | Closed the tag as it was opened | `expected \`>\` to close </i`: a closing tag is the name alone | 1 check |
| Expected 6 islands on the family page | Counted the components I placed | The second `<Parent/>` brings its own `<Child>`: 7 | 1 test |
| Expected the runtime to have an object for every island taken up | They all ran | `Pattern`, with no handler, was collected: nothing can change it, so nothing keeps it. Gave it a handler for the test, and added `Still` to test the panel's words for the case | 2 probes |
| Coverage taken at the end of the run | Usual | A reload discarded the old document's scripts, and two extension pages share one process, so starting coverage in the second reset the first's: `page.js` showed 45%, `devtools.js` 0% until coverage was taken before each navigation and the devtools page closed first | 3 runs |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A server fn's answer that is an error value, in the Network panel | `{"err":"no way"}`, status 200; the page's `?? "refused"` took it |
| A live state after a change from another client | one `{"seq":2,"changed":["db:tasks"]}` on the socket, then one `POST /_quillo/rpc/list_tasks` with `x-quillo-live: 1` per component following it |
| A WebSocket wrapped by a content script before the runtime starts | the runtime's live updates, reloads and channels work as before (every browser test runs with it) |
| A page with no component | no runtime and no error list; under `--watch` its reload socket is still heard |
| The state reader on a production bundle | the runtime's names are kept by the minifier; 4 components read in about 30 ms |
| Setting a state through `Sig.set` from the protocol | the button showed `clicks: 100` within 50 ms; a decimal set as `{"$dec":"1.10"}` read back as `1.10` |
| `Runtime.queryObjects(Inst.prototype)` after a component the browser made left the page (`extra` on, then off) | the four components on the page, and not the one that left |
| A component nothing in the browser can change (`component Still() { state s = 1; <b>{s}</b> }`: no handler, no live state, no timer) | taken up, then its `Inst` is collected (nothing reaches it), and the page keeps what it showed: right, and no memory kept for it. A tool sees an island with no component behind it; the extension says the browser let its code go |
| Props through two levels (`<Grand>` state `g` → `<Parent title={g}>` → `<Child label={title}>`), a sum of two states, a derived let, a fixed value, a default, a loop's item, a view's value | each read from the compiled getters as `<Parent>.a`, `<Parent>'s prop title ← <Grand>.g`, `<Parent>.a, <Parent>.b`, `<Parent>.double`, fixed, default, a loop's item, made inside the markup; the same on 0.6.0 and main, in production and under `--watch` |
| Under `--watch`, the Sources panel | each component at `/_quillo/src/<hash>/ui/probe.qwe/Counter.js`, mapped to the `.qwe` file |
| `/_quillo/health/queries` under `--watch` | on, from this machine, each statement's count and times |
| The extension's tests on the 0.6.0 release | 22 of 22 browser tests pass, through `quillo.rpc`'s closure and with the query table not drawn |

## Scripts

Every probe used the release build copied into the session's scratch folder (`$Q`), on its own ports, and stopped only the PIDs it started.

Globals and `data-s` after a click (production on 18742, `--watch` on 18743, the starter's `spa` sample plus the probe page):

```js
import { launch, sleep } from "~/dev/quillo-app1/feedback/tools/chrome.mjs";
const page = await (await launch()).page();
await page.go(`http://127.0.0.1:${port}/probe`);
await page.booted();
const own = await page.ev(`const f = document.createElement("iframe"); document.body.append(f);
  const base = new Set(Object.getOwnPropertyNames(f.contentWindow)); f.remove();
  return Object.getOwnPropertyNames(window).filter((k) => !base.has(k))`);
const c = await page.center("button.inc"); await page.click(c.x, c.y);
const s = await page.ev(`return [document.querySelector("quillo-island").dataset.s.slice(0, 7), document.querySelector("button.inc").textContent]`);
// production: own.length 8; --watch: 236. s: ['{"n":3,', 'clicks: 4'] in both.
```

State through the protocol:

```js
const fn = (await page.raw("Runtime.evaluate", { expression: "window.__quillo_resume" })).result.result.objectId;
const scopes = (await page.raw("Runtime.getProperties", { objectId: fn, ownProperties: false })).result.internalProperties.find((p) => p.name === "[[Scopes]]");
const closure = (await page.raw("Runtime.getProperties", { objectId: scopes.value.objectId, ownProperties: true })).result.result[0].value.objectId;
const inst = (await page.raw("Runtime.getProperties", { objectId: closure, ownProperties: true })).result.result.find((v) => v.name === "Inst").value.objectId;
const proto = (await page.raw("Runtime.callFunctionOn", { objectId: inst, functionDeclaration: "function () { return this.prototype }" })).result.result.objectId;
const all = (await page.raw("Runtime.queryObjects", { prototypeObjectId: proto })).result.objects.objectId;
await page.raw("Runtime.callFunctionOn", { objectId: all, returnByValue: true, functionDeclaration:
  "function () { return this.map((i) => ({ name: i.def.$name, state: Object.fromEntries(Object.entries(i.sigs).map(([k, s]) => [k, s.v])) })) }" });
```

Q-481 (the clash app above, `$Q run main.qwe --port 18744` and `--port 18745 --watch`):

```js
await page.go(`http://127.0.0.1:${port}/`);
await page.ev(`document.querySelector(".tally").dispatchEvent(new PointerEvent("pointerover", { bubbles: true }))`);
await page.booted();
const reportGives = await page.ev(`return String(report("x"))`);
{ const c = await page.center("button.widget"); await page.click(c.x, c.y); }
{ const c = await page.center(".later button.tally"); await page.click(c.x, c.y); await page.click(c.x, c.y); }
const tally = await page.ev(`return document.querySelector(".later .tally").textContent`);
```

Q-482:

```sh
$Q check main.qwe
$Q run main.qwe --port 18748 &
for f in a b; do curl -s -o /dev/null -w "$f: %{time_total}s\n" -X POST -H 'content-type: application/json' -d '[]' http://127.0.0.1:18748/_quillo/rpc/$f; done
```

Q-485 (the extension's fixture, `/chat`, with and without `--no-wt`): the extension's test "a channel's messages are heard over a WebSocket, and not over WebTransport", and for the protocol:

```js
for (const m of ["webTransportCreated", "webTransportConnectionEstablished", "webTransportClosed", "webSocketFrameSent", "webSocketFrameReceived"]) p.on("Network." + m, () => seen[m] = (seen[m] || 0) + 1);
await p.send("Network.enable");
await p.go(app.url + "/chat");
await p.evaluate("document.querySelector('button.say').click()");
// seen: { webTransportCreated: 1, webTransportConnectionEstablished: 1 }, and the page shows "hello"
```

The extension's own tests: `npm test`, and `QUILLO_BIN=$Q npm run test:browser` (or `npm run coverage`) in `~/dev/quillo-devtools`.
