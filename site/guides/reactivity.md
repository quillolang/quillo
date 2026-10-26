# Reactivity

A component's code runs once. Its `state` variables are signals: assigning one updates
the places that read it, and nothing else. There is no virtual DOM and no re-render: a
`let` that reads state is worked out again, a statement that reads it runs again, and
each spot in the markup that shows a value changes on its own.

The demos on this page are Quillo components, running in your browser beside their code.

> **Watch it happen.** Right-click a demo and choose **Inspect** (or press F12, ⌥⌘I on a
> Mac) to open the Elements panel, then use the demo's buttons. The panel briefly
> highlights each node as it changes: only the numbers and words that changed light up,
> never the whole demo. (The first click in a demo can light up a few more, once: that is
> the browser taking over the HTML the server sent.)

### State and what follows it

```quillo
state count = 0
let double = count * 2
```

`state` declares a value the component keeps; it is set once, when the component is made.
`let double = count * 2` follows it: when `count` changes, `double` is worked out again.
In the markup, `{count}` and `{double}` are the only places that change.

{{demo counter}}

- **Only what reads a value follows it.** The sentence that reads no state is never
  touched, and neither are the buttons.
- **Assigning the value a state already has does nothing.** "Assign the same value" sets
  `count` to itself: nothing lights up in the inspector.
- **Work things out with `let`, rather than keeping copies in state.** A `let` is always
  in step with what it reads; a second `state` would have to be kept in step by hand.

### Statements that read state

A statement in the component's code runs once, unless it reads state: then it runs again
when what it read changes. That is how timers start and stop:

{{demo timer}}

- `if running { every(...) }` reads `running` and `speed`. Start sets `running`, so the
  statement runs and the timer starts. Changing the speed runs it again, and the timer
  starts over at the new speed. Stop makes the `if` false, and the timer is gone.
- A timer, a stream (`listen`) or anything else such a statement starts lasts as long as
  the code that started it stands, and stops when the component leaves the page.

### Values are replaced, not changed

Lists and maps are values: `push`, `set`, `merge` and `+` give new ones and leave the old
alone. A state changes only when it is assigned, so changing a list means assigning the
new one:

```quillo
items = items + [{ id: 4, text: "Dates" }]                 // add
items = items | filter((i) => i.id != 2)                   // remove
items = items | map((i) => i.id == 2 ? merge(i, { text: "BREAD" }) : i)   // change one
```

`push(items, x)` alone builds a new list and drops it: `items` is unchanged, and nothing
on the page moves.

### Lists: a row for each item

`for` in markup keeps a row for each item: by its `key`, else by its place in the list.
Type a note into a row, then reverse the list:

{{demo rows}}

- **Rows move, they are not made again.** Reversing moves the three rows, and each takes
  what you typed in it along. In the inspector, the list's children are reordered and
  nothing inside them changes.
- **A changed item changes only where it shows what changed.** "Change Bread" changes one
  piece of text.
- **Adding puts one new row in**, and leaves the others alone.
- **Give rows a `key` from the data** (an id), so a row stays with its item. Without one,
  rows are kept by place: after a reverse the first row shows the last item, and what was
  typed stays where it was typed.

### Branches

`if`, `? :` and `match` in markup make a branch only when another one is chosen. While
the same branch stays, what is inside it follows its values like anything else:

{{demo branch}}

- Sign in, then change the name: only the name changes, and what you typed in the box
  stays.
- Sign out and in again: the branch is made afresh, so the box is empty. A branch keeps
  nothing once another has taken its place; keep what must survive in the component's
  `state`.

### Components: props are passed live

A child gets its props live: when the parent's value changes, the child shows it, without
either component running its code again. Each component keeps its own state:

{{demo props}}

- "Parent: add one" changes `n`: both badges show it, and each keeps its own click count.
- A `key` names a component: give it a new key and it is made afresh, with fresh state.
  "New key for the second" resets the second badge's clicks and leaves the first alone.
  It is how a page for a new product, say, starts clean (`key={id}`).

### Other values that change

State is not the only thing a component can follow. These change on their own, and what
reads them follows them the same way:

| Read | Follows |
|---|---|
| `url()` | the address bar: path, query and hash ([Single-page apps](/docs/components/#single-page-apps)) |
| `live(load)` | the server's data, when a table or variable it read changes, for anyone ([Live data](/docs/components/#live-data)) |
| `bind:value={x}` | an input, kept in step with a variable both ways |
| `every(ms, f)`, `listen(url, f)` | time and streams, for as long as the code that called them stands |

### From the server to the browser

The server renders a component to HTML with the state it starts with, so the page shows
at once. When someone reaches for the component, the browser runs its code once, from
that same state, and takes the server's elements as its own instead of drawing them
again. From then on, everything on this page applies. See
[Components](/docs/components/) for when each component's code loads.

### In short

- `state` for what the component keeps, `let` for what follows from it.
- Assign to change: `items = ...`, never a change in place.
- Statements that read state run again; use that for timers and streams.
- Key list rows by their data.
- What a branch holds lasts as long as the branch; what must last, keep in `state`.
