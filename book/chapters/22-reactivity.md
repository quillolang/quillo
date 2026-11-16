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
        <input value={coupon} placeholder="Coupon"/>
        <p class={off > 0 && "saved"}>Total {total - off}</p>
        <button onclick={() => { lines = lines | filter((l) => l.qty > 0) }}>Tidy</button>
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
| an assignment alone in the body (`count = len(items)`, `n += 1`) | a step as it starts | never: it runs once, and `quillo check` warns when it reads a prop or another state (write `let`) |
| `{expr}` in markup, an attribute's or a class's `{...}` | a place on the page | when what it read changed; text that comes out the same is not written |
| `cond ? <a/> : <b/>`, `if`, `match`, `cond && <x/>` in markup | a branch | made again only when another branch is chosen |
| `for x in xs => <row/>` | a row for each item | rows are kept by `key` (else by place) and given their new item; a `key` that reads other state makes its rows again when that changes |
| `<Child prop={x}/>` | props passed live | the child's own reads of `prop` follow `x`; neither component runs again |
| `key={k}` on an element or component outside a loop | a part made afresh | when `k` changes |
| `@event={...}`, `@mount`, a function given to a library | a handler | when it is called; what it reads is read then, not followed |

"Something that changes" is: a `state`, a prop, a `let` that is itself derived, a `for`
row's item, a `live(...)` state, `url()`, and a call of the component's own functions or
the program's (taken as reading what changes, since they may), however it is written:
`x | show` and `show(x / 2)` follow what `show` reads, as `show(x)` does. Builtins on constant
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
        <button onclick={() => { running = !running }}>{running ? "Stop" : "Start"}</button>
        <button onclick={() => { speed = speed / 2 }}>Faster</button>
        <span>{ticks}</span>
    </div>
}
```

A handler may start one too, which then lasts as long as its component, after the
handler's waits too, or set the state that the statement reads. For once, later,
`run_after(ms, f)`. There is no `setTimeout`; a debounce or a throttle is an `every` under
a state flag that its tick turns off.

## Whose a timer is

A timer or a stream (`every`, `run_after`, `listen`) belongs to a part of the page, and
stops when that part leaves it. Which part depends on the code that starts it
(FEEDBACK61.md, Q-901):

- **An event handler's** belongs to the handler's component, before and after the
  handler's waits. It outlives the row or the `if` branch that held the button, so a
  row's Delete can still clear its notice once the row has gone (below). A stopwatch
  whose Start button sits in a branch keeps ticking after the branch hides; to end it
  with the branch, make the stopwatch a component of its own.
- **A part's own code** gives it to that part, its row or branch, or the component: a
  statement (`if running { every(...) }`), an `@mount`, a timer's tick, a stream's message.
- **A parent's function that a child calls** (a modal's Save, a countdown's end, an
  `on_done()` that a child's `if` calls) is the parent's code, so what it starts
  outlives the child.
- **A plain `fn`** gives it to what called it: from a handler, even after a wait, the
  handler's component; from an `@mount`, a callback or a component's own code, that part.
  A parent's function that a child uses as its handler (`@click={on_save}`, `on_save`
  being a prop) is the code of the component that wrote the child's tag: the parent, or,
  when the function was passed on through another component (`<Modal>` handing it to its
  `<Button>`), that one. When the child calls it from its own code
  (`@click={() => { on_save() }}`), or keeps it in a variable, a list or a map, it is the
  child's. A plain `fn`'s timer in it ends with that component.
- **Markup is the code of the component that wrote it**, wherever it is shown: passed as
  `children` or another prop, given back by a function prop, or made in a handler and
  kept in state. Its `@mount`, and what that starts, end with the part showing it; its
  handlers' timers are the writer's. Markup still shown after its writer has gone (a
  toast a page put in the app's state) works on, its handlers' timers given to the part
  showing it.
- **A function that a child calls while it makes a row or a branch** (a render prop,
  `row={(x) => ...}`) is part of that row or branch.
- **A function handed to the page** (a library's callback, a listener) runs as the code
  that handed it over.

A part that has gone starts nothing: what its code asks for after a wait it did not
outlast is not started.

```quillo
component Inbox() {
    state mails = [{ id: 1, subject: "Hello" }, { id: 2, subject: "Lunch?" }]
    state notice = ""
    <div>
        {for m in mails => <p key={m.id}>{m.subject} <button onclick={() => {
            mails = mails | filter((x) => x.id != m.id)
            notice = `Deleted ${m.subject}`
            run_after(2000, () => { notice = "" })
        }}>Delete</button></p>}
        <p>{notice}</p>
    </div>
}
```

Delete takes the row away and shows the notice; the `run_after` is `Inbox`'s, not the
row's, so the notice goes two seconds later.

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
- A key whose code fails (`key={100 / x.n}` on an item whose `n` is 0) is said in the
  console, once while it goes on failing, and its row is given a key of its own: that
  row is made again at each change, and the rest of the list keeps up.

## Branches

`if`, `? :`, `match` and `cond && <markup>` choose a branch, and make it again only
when another is chosen. While one stays, what is inside it follows its values like
anything else: an input in it keeps its focus and what was typed. A branch that goes
takes everything it made with it: its state of nested components, its timers. Keep what
must outlive a branch in the component's own `state`. The branch chosen next is made
anew, its fields too, even one written the same in both (the password of a sign-in form
and of a sign-up form): it has neither the focus nor what was typed. To keep one field
through a switch, write it once and switch what differs
(`placeholder={joining ? "New password" : "Password"}`). `key={k}` on an element or a
component makes it afresh whenever `k` changes, which is how a page for a new product
starts clean (`<Product key={id} id={id}/>`).

## Between components

- Props are passed live: a child that reads `label` follows the parent's value, in its
  markup, its `let`s and its statements, without either component's code running again.
  A function given as a prop (`on_change`) carries changes back up.
- `state x = prop` reads the prop once: state is the component's own. To follow the
  parent, read the prop (or a `let` of it); to start over when it changes, give the
  child a `key` that changes with it.
- A component that has left the page (its branch gone, its `key` changed) keeps the
  props it last read: the function its `@mount` returned, and a handler that runs late,
  see its own values, not the parent's new ones.
- A child that changes its parent's state as it starts (a callback it calls in its body)
  has the server render the parent once more, so the page shows one value and its island
  carries it; at most once, and never inside another component's second render.
- A view used as a tag runs once with its props live, as a component does; a plain
  function used as a tag (`<Row item={x}/>` where `Row` is neither) is called again as a
  whole when what it is given changes.
- There is no context: state lives in components and goes down as props. What the whole
  page shares is a [page state](#page-state) (below): components placed separately in a
  page follow it and set it alike.

## What is not followed

- **The page's own objects.** `el.value`, `el.scrollTop`, what `js(...)` gives: read
  them in a handler and put what matters in state (`onscroll={(e) => { top = e.scrollTop }}`).
- **Time.** `now()` is read once; a clock is a state that `every` sets.
- **The server.** A `state x = load()` is the answer the page was made with. To follow
  the server, `state x = live(load)` (see [Live data](#live-data)); its arguments are
  followed too (`live(load_thread, id)` loads again when `id` changes).
- **Global variables.** A component uses the program's constants as they were when the
  program loaded; a global the server changes cannot be read in a component at all
  (pass it as a prop, or load it with `live`). A page state is not one: it is the page's.

## Page state

`state` at the top of a program, outside any component, is the page's: every component on
the page that reads it follows it, and any of them may set it, though they load apart.

```quillo
state cart = []

component Add(item: map) {
    <button onclick={() => { cart = cart + [item] }}>Add</button>
}

component Badge() {
    <span>{len(cart)}</span>
}

route GET / {
    cart = load_cart()              // where this page starts
    <html><body>
        <header><Badge/></header>
        <main><Add item={{ id: 1 }}/></main>
    </body></html>
}
```

- **One for each page**, for each visitor: it starts again at every page load (a link
  followed is one), and no visitor sees another's. A top-level `let` is the server's, one
  for everyone, which is why a component may not set one.
- **On the server**, each request for a page runs the initializers in the order written
  (one may read another), before its route; the route may set it, and the page is made
  with it. They run with the request there, so an initializer may read the visitor's
  `session()` and `url()` (`state who = session().user ?? "guest"`); the page is then the
  visitor's, `private` (see [What a route returns](#what-a-route-returns)). Views and
  components read it as any name. A view that read one is not cached, since it is that
  page's.
- **In the browser**, the page starts from the value its render left, which it carries; a
  component that shows one starts with the page, so it follows a change made elsewhere
  before anyone reaches for it (one that only sets it waits, as any component does).
- **Not where no page is made**: a `server fn` reading one is an error (`quillo check`
  says so; pass what it needs as an argument), and so is a job, a timer or a channel's
  handler reading one when it runs. A request to Quillo's own addresses (a `server fn`'s
  call, live data) has none.
- `state cart` and a `let` or `fn` named `cart` cannot both be; two `state cart` neither.

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
| `useRef` | `bind:this` | template ref | `onmount={(el) => { box = el }}` |
| `value` with `onChange` | `bind:value` | `v-model` | `value={x}`: two-way by itself when `x` is state |
| `useContext` | `setContext` | `provide` | none: props |
| a store (Zustand, Redux) | `$state` in a module | a Pinia store | `state cart = []` at the top: the page's |
| `key` | `{#each ... (key)}` | `:key` | `key={...}` |
| `React.memo`, `useCallback` | | | not needed: nothing renders again |
| a list of dependencies | | | none: what is read is followed |

The rules of hooks have no counterpart: state is found by its name, not by the order of
calls, and a statement that starts something may sit inside an `if` (that is how it is
stopped).

On [quillolang.org's Reactivity page](https://quillolang.org/docs/reactivity/), live
demos show each of these running beside its code.


## What a save keeps

Under [`--watch`](#options-of-quillo-run), a save is put into the open pages with as little lost
as can be *(since 0.8.5: every save reloaded every page, and the server's data started again)*:

- **Nothing the page uses changed** (a route it never calls, a server fn's body): the page stays
  as it is.
- **A component's code changed**: it is swapped into the page in place, no reload. Each state
  keeps its value while the new code starts it as the same kind of value (a number stays a
  number, a list a list; a state that starts as `null` takes any); one whose kind changed starts
  again from its new initializer, and the browser's console says so. A changed initializer of
  the same kind keeps the value. The components inside it keep theirs, by component and order
  (`<Row/><Row/>` with a third added before them shifts them, as when a page is taken up). What
  its elements held that no state does is put back where the same kind of element is in the same
  place: the focus and its selection, the scroll of a box and of the page, what was typed in a
  field nothing follows, a box ticked, an open `<details>` or `<dialog>`, an `<audio>` or
  `<video>`'s time *(since 0.8.6: they went with the swap)*. Its effects, `@mount` and timers run
  again, as its code is new.
- **The page's own HTML changed**, outside its components (a heading, a class, the title, a
  node added at the end of an element): the page takes the new HTML in place, its components as
  they are; one whose props the page changed is made again with them, keeping its states. The
  page's stylesheets follow.
- **What cannot be put in place** (another element where one was, a script of the program's that
  differs, the runtime itself): the page reloads, and keeps what it can: each component's states
  by the same rule, a field a person typed into that nothing follows, an open `<details>`, the
  scroll and the focus.
- **On the server**, each top-level `let` the program changes somewhere (its data) keeps its value
  while the new version starts it as the same kind of value; one whose kind changed, or that holds
  a function or a handle, starts again. A `let` the program never changes is code, and follows
  the edit. A `:memory:` database starts again empty. The reload's line says what was kept:
  `quillo: reloaded main.qwe (3 routes); kept notes, count; started again: cart (its type changed:
  list to map)`. To start the data again without a restart, type `r` and Enter where the server
  runs *(since 0.8.6)*: the program loads again with nothing carried (`; its data started again (r)`). That is
  read only when the server's input is a terminal, and the start line says so.
- A save that does not load is said over the page, which keeps showing the version before it.

`--watch=reload` (or `<html data-reload>` on a page) reloads at every save, keeping nothing.
`--reload`, for production, carries nothing either.
