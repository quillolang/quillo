# Feedback from planning page-wide state: one `state` at the top of a program, shared by every component on the page

| | |
|---|---|
| App | None: a plan for the user's requirement of 2026-11-15: "lets also implement global states" |
| Quillo | This repo's `target/release/quillo`, `quillo 0.8.4 (5249ff0, 2026-11-15)`, on branch `hmr`, which is on `feedback66`. Neither branch is on main yet: CI on `feedback66` was still running while this was written |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check` and `quillo run` on probe programs. A read-only map of the code the feature touches: the parser, checker, server render, browser compile and runtime |

No Quillo source was changed for this report, and neither were `feedback/INDEX.md` or `feedback/repros/`. One exception: the parser change of slice 1 (a top-level `state` parses) is in the working tree, uncommitted, on branch `global-state`; the probes below that need it say so. Numbers start at Q-961, after FEEDBACK67.md's Q-960.

**What was decided, and by whom.**
- The user, 2026-11-15, choosing among four: the value lives **for one page load, for each visitor**. Not per tab, not on the server, and not kept across pages, since a link click is a full page load here. Keeping it across pages may come later, as an opt-in.
- My defaults below (the syntax, where it may be read, what is an error). They were told to the user, who did not object; Part E lists them so each can be turned down.

## The answer

Two components placed separately on a page share nothing today. The guide says so ("There is no context and no global store in the browser", USERGUIDE.md, "Between components"), and the React table's `useContext` row says "none: props".

A cart badge in the header and an Add button in a product list must therefore sit inside one component that holds the cart and passes it down. Otherwise each must go through the server (`live` plus a `server fn`), which costs a request for each change and a live connection for each page.

The plan is a top-level `state`:

```quillo
state cart = []

component Add(item: map) {
    <button onclick={() => { cart = cart + [item] }}>Add</button>
}

component Badge() {
    <span>{len(cart)}</span>
}

route GET / = <html><body>
    <header><Badge/></header>
    <main><Add item={{ id: 1 }}/></main>
</body></html>
```

- Every component that reads `cart` follows it, and any of them may set it, across islands that load separately.
- The server makes the page with it: the initializer runs once for that page, as its request starts, and a route may set it before its markup. The page carries the value the render ended with.
- It is one visitor's page's, never another's. That is unlike a top-level `let`, which is the server's, one for everyone.
- It starts again on every page load.

## Part A: what happens today, measured

Each probe was run with `quillo check` and `quillo run` on 0.8.4 (5249ff0).

1. **`state cart = []` at the top** is a parse error: `expected end of statement, found cart`. The parser reads a `state` only inside a component's body (`component_body`, src/parser.rs:1658). At the top, `state` is an ordinary identifier.
2. **A top-level `let cart = []` that a component sets**:
   - `quillo check` refuses it: "<Add> runs in the browser, but `cart` is a server variable that changes; pass it as a prop or read it with a `server fn`". The same comes with the parser change of slice 1, since the checker does not know a page state yet.
   - Rightly so: that `let` is one value for every visitor.
3. **One parent component that holds the cart** works. But the badge and the list must then be one island: the whole page between them hydrates as one, and the header and the list cannot be placed apart.
4. **Through the server** (`state cart = live(load_cart)` in each, and a `server fn` to add) works. It costs a request for each change, a live connection for each open page, and a place to keep each visitor's cart.

## Part B: what the language would own

### B.1 Writing it

- `state name = expr` at the top of a file; not `~name` there, where `~` is the operator. A name may not be both a page state and a top-level `let` or `fn`. It is named across files as other top-level names are (`use`, `private`).
- **It may be read and set** in a component's body and handlers, in a view, in a route, and in plain functions those call.
- **Not in a `server fn`, a channel's handler, a job or a timer**: they run for no page. `quillo check` says so where it can see it (a `server fn`'s body); anywhere else it is a runtime error: "`cart` is a page's state, and this runs for no page: pass it as an argument".

### B.2 On the server

- **Per request.** The request's own map (a field of `Interp`, cleared with `session` and `islands` at src/server.rs:425-440). A page state is never put in the shared globals: no `assign_global`, no `publish("var:...")`, no `note_global_read`, so no visitor sees another's and no live loader wakes on it.
- **Its initializer runs once for each page, at the start of the request**, in the order written (one may read another): a route that sets it gives the page that value. *(Changed while building: on the first read, as first planned, the interpreter's own fast paths, which read a name without it, would have seen no value. A request to Quillo's own addresses, a `server fn`'s call or live data, runs none.)*
- **What the page carries.** The values the render ended with, at the end of the page: `<script type="application/json" data-quillo-g>{"cart": []}</script>`. Only those that something read or set, and none when the page has no component.
- **Views.** A view that reads one is not cached (`mark_impure`, src/interp.rs:2102). The view cache keeps HTML by its arguments and the globals it read, so it would give one visitor's cart to the next.
- **Deferred parts and `render_again`** render after the page on the same interpreter: the values last until the answer ends.
- **Mounted programs** keep their own page states.
- **The JIT** (src/jit.rs, src/jitloop.rs) never treats one as a constant global.

### B.3 In the browser

- A read compiles to the page's signal, `W.gs("cart")`, which resolves as a component's state does (`.v`, `W.put`, `W.to`, `bind:value`). A write sets it, and everything that read it follows.
- The runtime makes each signal once: from the page's `data-quillo-g` value, else from its initializer, which is compiled into the bundle. The initializer is used for a page the server made without it, such as a component first shown in the browser.
- **A component whose render read a page state loads as soon as the page is idle** (`data-e`). It follows the first change without being touched. A component that only sets it (the Add button) still waits to be reached for.
  - Why not lazily? Nothing wakes an island whose code has not loaded (the code map found none), and one woken after the change would meet HTML made with the old value.
  - Waking such islands just before the first change is possible, and would keep the badge's code off the page until then. Left for later (Part E, question 3).

### B.4 Tools

- **Devtools** (runtime.js `TOOLS`, `toolsAsk`): page states listed beside the components, each change said as `"global"`, and settable under `--watch` as states are.
- **HMR** (FEEDBACK54.md): a page state is kept across a swap while its type is the same, as the user decided for states on 2026-11-04.
- **Editors**: the grammars must take `state` at the top of a file.

## Part C: what not to build

- **Keeping it across pages** (`sessionStorage`, or the session on the server). Decided against for now. It could be an opt-in later (`state cart = [] keep "tab"`), designed separately.
- **A context** (a value given to the components below one). A page state is for the whole page. Something narrower is props, as now.
- **Reading it in a `server fn` by sending it along with each call.** The page would send every page state with every call, and the server fn would read values the visitor can change. An argument says what it uses.

## Seen again

None.

## New issues

### Q-961. Two components placed apart on a page cannot share a value without a parent around both or a trip to the server

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the user's requirement; probes 1 to 4 above | 4 |

The probes are in Part A.

| Impact | |
|---|---|
| **Functionality** | A badge and a list, a filter and a table, or a theme switch and the page cannot be separate islands that agree |
| **User** | The developer writes one component around the page (all of it hydrating as one), or a `live` loader and a `server fn` for what is the page's own business |
| **Requests** | Through the server: a request for each change and a live connection for each page |
| **Security** | No change; through the server, each visitor's value must be kept apart by the program |

**Suggestion:** Part B.

### Q-962. The error for a top-level `let` set in a component points only to props and `server fn`s

| Kind | Detected by | Attempts |
|---|---|---|
| loud (a message) | probe 2 | 1 |

"`cart` is a server variable that changes; pass it as a prop or read it with a `server fn`" is right about the `let`. With page states, what the developer most often wanted is `state cart = []`.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | The fix offered is the long way round |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** add "or, if each page should have its own, `state cart = ...` at the top" when the `let` is only set in the browser.

### Q-963. A view's cache and a live loader's run would leak a request's own value (design risk, not a bug today)

| Kind | Detected by | Attempts |
|---|---|---|
| design risk | the code map: src/viewcache.rs, src/livecache.rs | 0 |

The view cache keys a view's HTML by its arguments and the `var:` globals it read, and a live loader runs again in later requests. Neither knows of a value that is one request's. Today nothing is one request's except `session()` and `request`, which already mark a run impure.

| Impact | |
|---|---|
| **Functionality** | With page states as they would be without B.2's rules: one visitor's cart in another's page |
| **User** | Wrong pages, now and then |
| **Requests** | No change |
| **Security** | One visitor's data shown to another |

**Suggestion:** a read of a page state marks the run as `session()` does (B.2). A test gives two visitors two carts through a cached view.

## Proposed order of work

1. **Server.** The parser (in the tree), the checker (`Kind::PageState`, Q-962's message), the request's map, views not cached, the `server fn` error, and the page's `data-quillo-g`. Tests:
   - two visitors with two carts, through a view that is cached;
   - a route that sets the starting value;
   - a `server fn` refused, by `quillo check` and at run time;
   - a deferred part that reads it;
   - a mounted program's own.
2. **Browser.** `W.gs`, the runtime's signals from the page or the initializer, and eager islands. Chrome tests:
   - Add in one island, Badge in another, at once;
   - a writer reached for first;
   - a component first shown in the browser;
   - `bind:value` on one;
   - two pages in two tabs, apart.
3. **Speed.** A page without page states costs nothing more: Fortunes and the component page of bench/cyc.py, before and after, with the numbers in the commit.
4. **Devtools, guide, book, editors, CHANGELOG.** The guide's lines that say there is none (USERGUIDE.md 1661, 1974, 3454, 3466), and the React/Svelte/Vue table (`useContext`, `$state` in a module, a Pinia store).
5. **HMR** keeps page states (with FEEDBACK54.md's swap).

## Open questions for the user

All three decided by the user on 2026-11-15, each as recommended: `state cart = []` at the top; a `server fn` reading one is an error; components that read one load at idle.

### 1. Syntax: `state cart = []` at the top, or a keyword of its own?

| | `state` at the top (recommended) | `page state cart = []` |
|---|---|---|
| **Functionality** | The same | The same |
| **User** | One word for a value that the page changes, in a component or above them | Says outright that it is the page's; one more word |
| **Requests** | No change | No change |
| **Security** | No change | No change |

### 2. A `server fn` reading one: an error, or the initializer's value?

| | An error (recommended) | The initializer's value |
|---|---|---|
| **Functionality** | The code says what it needs, as an argument | Runs, with a value that is not the page's |
| **User** | Told at `quillo check` | A cart that is always empty on the server, with no word why |
| **Requests** | No change | No change |
| **Security** | No change | No change |

### 3. Components that read one: load at idle, or wake them just before the first change?

| | At idle (recommended, first) | Woken before the first change |
|---|---|---|
| **Functionality** | Follows every change | The same |
| **User** | No difference seen | No difference seen |
| **Requests** | Their code loads at idle on pages that use page states, as for `listen` | Their code loads at the first change, the change waiting for it (a few ms) |
| **Security** | No change | No change |

## Smaller notes

- The checker's `Kind` needs a new kind. The browser compiler's `Resolved::State` already does everything a page state needs, given the signal's expression.
- `W.gs(name)` is a map lookup on each read. If a page shows many, each component can take the signal once at its start.

## Where the AI struggled

- Nothing yet: this is a plan. Mapping the code took one read-only agent, which found the shared globals, the view cache and the lazy islands as the three places a naive version breaks.

## Edge cases checked that behave well

- A top-level `let` set in a component is refused today, so no program can be relying on that `let` being per visitor.
- A top-level `state` was a parse error, so the new syntax breaks no program that parsed.
