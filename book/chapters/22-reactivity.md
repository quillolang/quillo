# Reactivity

A component's code runs once. What it declares with `state` is a signal; what reads a
signal (a `let`, a statement, a place in the markup) is recorded as it runs; and when the
signal is assigned a different value, those, and only those, are worked out again. The
component does not render again, nothing is compared against a virtual DOM, and there
are no dependency lists to keep: what code reads is what it follows. This is how
SolidJS and Svelte's runes work, not React.

```quillo
component Cart(items: list) {
    state lines = items                          // a signal
    state coupon = ""                            // another
    let total = lines | map((l) => l.price * l.qty) | sum()    // follows lines
    let off = coupon == "TEN" ? round(total * 0.1, 2) : 0     // follows coupon and total
    if total > 1000 { every(5000, () => { notify("Big order!") }) }   // starts and stops with it
    <div>
        <ul>{for l in lines => <li key={l.id}>{l.name} × {l.qty}</li>}</ul>
        <input bind:value={coupon} placeholder="Coupon"/>
        <p class={off > 0 && "saved"}>Total {total - off}</p>
        <button @click={() => { lines = lines | filter((l) => l.qty > 0) }}>Tidy</button>
    </div>
}
```

Typing in the coupon box sets `coupon`: `off` is worked out again, then the text of the
total and its class, and nothing else. `total` is not, since `lines` did not change; the
list is not touched. The page shows the new total once the keystroke's handler is done.

## What follows what

| Written in a component | Becomes | Is worked out again |
|---|---|---|
| `state x = v` (or `~x = v`) | a signal, set once | never: it changes when assigned |
| `let y = ...` that reads something that changes | a derived value | when something it read changed, and only once something reads it |
| `let y = ...` that reads nothing that changes | a constant | never |
| `let y = ...` whose name is assigned somewhere (`y = 5`) | a plain variable | never by itself |
| a statement that reads something that changes (`if running { every(...) }`) | an effect | after what it read changed |
| `{expr}` in markup, an attribute's or a class's `{...}` | a place on the page | when what it read changed; text that comes out the same is not written |
| `cond ? <a/> : <b/>`, `if`, `match`, `cond && <x/>` in markup | a branch | made again only when another branch is chosen |
| `for x in xs => <row/>` | a row for each item | rows are kept by `key` (else by place) and given their new item |
| `<Child prop={x}/>` | props passed live | the child's own reads of `prop` follow `x`; neither component runs again |
| `key={k}` on an element or component outside a loop | a part made afresh | when `k` changes |
| `@event={...}`, `@mount`, a function given to a library | a handler | when it is called; what it reads is read then, not followed |

"Something that changes" is: a `state`, a prop, a `let` that is itself derived, a `for`
row's item, a `live(...)` state, `url()`, and a call of the component's own functions or
the program's (taken as reading what changes, since they may). Builtins on constant
values are not: `let started = now()` is the time the component started, and stays so.
What is followed is what a run actually read: `let shown = open ? details : summary`
follows `open` and `details` while `open` is true, and `open` and `summary` while it is
false.

## When the page changes

- **A change is a different value.** Assigning a state the value it has (the same list,
  the same number) does nothing. Lists and maps are values, so a list built anew is a
  different value even when it holds the same items: everything that reads the state is
  worked out again (text that comes out the same is left alone). There is no way to
  change a list or map in place, so there is no change that goes unseen.
- **In a batch, a moment later.** Assigning marks what reads the state; the statements
  and places on the page run once the handler's own code is done (in a microtask), each
  once however many states it read changed, in the order they were made. An element read
  right after `n += 1` (`e.target.textContent`) still shows the old text; once the
  handler has waited for something (a `server fn`, `js(...)`), the page has caught up.
- **Derived values are lazy and consistent.** A `let` is worked out only when something
  reads it, and only if something it read really changed: a `let` that comes out the same
  as before stops the update there (`let even = n % 2 == 0` going from 2 to 4 wakes
  nothing that reads `even`). A statement never sees a `let` that is out of date.
- **No loops.** Code that sets what it reads would run for ever: after 1,000 rounds of
  updates the page stops and says so in the console (`updates go round in a loop:
  something sets what it reads`). A statement may set another state (`if query == "" {
  results = [] }`), as long as the two do not feed each other.
- **Errors stay where they are.** An error in a statement or a place on the page is
  written to the console (`quillo: ...`); the rest of the page goes on updating. A place
  whose expression fails is emptied, rather than left showing its last good value.

## What a statement starts, it stops

What runs again owns what it started: `every`, `listen`, `channel.open`, the statements
and branches inside it, and the functions `@mount` returned. Before it runs again, and
when its component, row or branch leaves the page, those are stopped. So timers and
streams are started by state rather than by hand:

```quillo
component Clock() {
    state running = false
    state speed = 1000
    state ticks = 0
    if running { every(speed, () => { ticks += 1 }) }    // stops when running is false,
    <div>                                                 // starts over when speed changes
        <button @click={() => { running = !running }}>{running ? "Stop" : "Start"}</button>
        <button @click={() => { speed = speed / 2 }}>Faster</button>
        <span>{ticks}</span>
    </div>
}
```

A handler may start one too, which then lasts as long as its component, or set the state
that the statement reads. For once, later, `run_after(ms, f)`. There is no `setTimeout`; a
debounce or a throttle is an `every` under a state flag that its tick turns off.

## Lists and rows

- `for` keeps a row for each item, matched by `key={...}` (text or a number), else by
  place. Rows that move are moved, as few as keep the rest in order: a swap moves two, a
  removal none; their DOM, focus and what was typed in them go with them.
- A row whose item is a different value is given it, and only the places in the row that
  read the item change. So when changing one item, keep the others the same values:
  `items | map((i) => i.id == id ? merge(i, { done: true }) : i)` changes one row; a
  list made of new maps for every item (from `json.parse`, or a server fn's answer) gives
  every row its new item, which is still cheap, since text that comes out the same is not
  written.
- `x == state` (or `!=`), with a state written by its name on one side and text, a
  number or a boolean on the other, is followed per value: in a thousand rows that each
  ask `r.id == selected`, choosing another row wakes two of them, not a thousand. Written
  through a function (`is_selected(r)`), every row asks again.
- A key that is a list or a map never matches the row it had, so the row is made again
  each time: key rows by an id.

## Branches

`if`, `? :`, `match` and `cond && <markup>` choose a branch, and make it again only
when another is chosen. While one stays, what is inside it follows its values like
anything else: an input in it keeps its focus and what was typed. A branch that goes
takes everything it made with it: its state of nested components, its timers. Keep what
must outlive a branch in the component's own `state`. `key={k}` on an element or a
component makes it afresh whenever `k` changes, which is how a page for a new product
starts clean (`<Product key={id} id={id}/>`).

## Between components

- Props are passed live: a child that reads `label` follows the parent's value, in its
  markup, its `let`s and its statements, without either component's code running again.
  A function given as a prop (`on_change`) carries changes back up.
- `state x = prop` reads the prop once: state is the component's own. To follow the
  parent, read the prop (or a `let` of it); to start over when it changes, give the
  child a `key` that changes with it.
- A view used as a tag runs once with its props live, as a component does; a plain
  function used as a tag (`<Row item={x}/>` where `Row` is neither) is called again as a
  whole when what it is given changes.
- There is no context and no global store in the browser: state lives in components and
  goes down as props. Components placed separately in a page share nothing; they share
  through the server (below).

## What is not followed

- **The page's own objects.** `el.value`, `el.scrollTop`, what `js(...)` gives: read
  them in a handler and put what matters in state (`@scroll={(e) => { top = e.scrollTop }}`).
- **Time.** `now()` is read once; a clock is a state that `every` sets.
- **The server.** A `state x = load()` is the answer the page was made with. To follow
  the server, `state x = live(load)` (see [Live data](#live-data)); its arguments are
  followed too (`live(load_thread, id)` loads again when `id` changes).
- **Global variables.** A component uses the program's constants as they were when the
  program loaded; a global the server changes cannot be read in a component at all
  (pass it as a prop, or load it with `live`).

## On the server

A component's code runs once on the server too, to write the page's HTML with the state
it starts with; statements that start timers or streams do nothing there. The browser
then takes the server's elements as its own, from that same state, without running the
initializers again (see [Components](#components)). From then on, the browser is where
reactivity happens.

The server has two of its own, which follow the same idea of following what was read:

- **Live data.** A `live` loader records the tables and globals it read; a write to one,
  or a `publish` of a topic it `watch`ed, runs it again for every page that shows it, and
  the pages get the difference (see [Live data](#live-data)).
- **Cached views.** A view's HTML is reused while its arguments and the globals it read
  are unchanged (see [Cached views](#cached-views)).

## Coming from other frameworks

| React | Svelte 5 | Vue | Quillo |
|---|---|---|---|
| `useState` | `$state` | `ref` | `state x = v` |
| `useMemo` | `$derived` | `computed` | `let y = ...` reading state |
| `useEffect` (and its cleanup) | `$effect` | `watchEffect` | a statement reading state; what it starts stops with it |
| `useRef` | `bind:this` | template ref | `@mount={(el) => { box = el }}` |
| `useContext` | `setContext` | `provide` | none: props |
| `key` | `{#each ... (key)}` | `:key` | `key={...}` |
| `React.memo`, `useCallback` | | | not needed: nothing renders again |
| a list of dependencies | | | none: what is read is followed |

The rules of hooks have no counterpart: state is found by its name, not by the order of
calls, and a statement that starts something may sit inside an `if` (that is how it is
stopped).

On [quillolang.org's Reactivity page](https://quillolang.org/docs/reactivity/), live
demos show each of these running beside its code.
