# Feedback from making a "same four mistakes in six frameworks" video with the Groceries app

| | |
|---|---|
| App | The Groceries app of `~/dev/quillo-videos/videos/208-reactivity-models/app/main.qwe`: a 59-line list (50 items from `items.qwe`) with the states `items`, `query` and `draft`, `let`s for what is shown, open and the total, and a `Row` component per item. Found while making video 210, "the same four mistakes in six frameworks" (QUEUE 198), by quillo-videos-10: each mistake was made on purpose in the Quillo copy to see what check and the page say |
| Quillo | `quillo 0.8.10 (61997ab, 2026-11-22)`, the `quillo` on the PATH. Every repro below was run on that binary |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; headless Chrome (`feedback/tools/chrome.mjs`) clicking the page; `node feedback/run.mjs Q-1360 Q-1361` |

Numbers Q-1360 and Q-1361, from the block Q-1360 to Q-1369.

## New issues

### Q-1360. A dropped `map` or `filter` is never warned of, while a dropped `push` or `set` is

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (making a video) | 1 |

In the Groceries component, the assignment left out of `toggle`:

```quillo
fn toggle(id) {
    items | map((i) => i.id == id ? { ...i, done: !i.done } : i)
}
```

`quillo check main.qwe` says `ok (2 routes, 1 function, 2 components, in 2 files)` with no warning, and ticking a box does nothing: "40 left · $555.52" stays, and the box stays ticked. The same mistake with `push` is warned of: `` `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)` ``.

This is what is left of Q-505 and Q-1267. Q-505's fix leaves `map` out on purpose (its test keeps `xs.map((v) => print(v))`, `map` for its function, unwarned), and Q-1267's fix (e15e5ab, in 0.8.10) brought the warning to a component's fn for `push`. On 0.8.10, `map` and `filter` are not warned of anywhere: in a component's fn as `xs | map(f)` or `map(xs, f)`, as the last statement or not; in an inline handler (`onclick={() => { xs | map((x) => x * 2) }}`); in a top-level fn; or at the top level. A dropped `set` (`m | set("a", 2)`, `set(m, "a", 2)`) in a component's fn is warned of.

```quillo
component G() {
    state xs = [1, 2, 3]
    fn m1() { xs | map((x) => x * 2) }      // not warned
    fn m2() { map(xs, (x) => x * 2) }       // not warned
    fn f1() { xs | filter((x) => x > 1) }   // not warned
    fn p1() { xs | push(4) }                // warned
    <div>
        <button id="m1" onclick={m1}>m1</button>
        <button id="m2" onclick={m2}>m2</button>
        <button id="f1" onclick={f1}>f1</button>
        <button id="p1" onclick={p1}>p1</button>
        <p id="out">{str(xs)}</p>
    </div>
}
```

Saved as [repros/Q-1360-dropped-map-in-a-fn.qwe](../repros/Q-1360-dropped-map-in-a-fn.qwe). `quillo check`:

```
Q-1360-dropped-map-in-a-fn.qwe:13: warning: `push` gives a new value and changes nothing, and it is dropped here: write `xs = xs | push(...)`
Q-1360-dropped-map-in-a-fn.qwe: ok (1 route, 0 functions, 1 component), 1 warning
```

In the browser, `[1, 2, 3]` stays after each of m1, m2, f1 and p1. With `xs = xs | map(...)` the same button gives `[2, 4, 6]`.

| Impact | |
|---|---|
| **Functionality** | The update is lost: the state keeps its old value |
| **User** | A click that does nothing, with no error in check, the page or the console |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** extend Q-505's warning to `map` and `filter` when the function given to them has no effect: its body calls nothing that is not a pure builtin and assigns nothing (`(i) => i.id == id ? {...} : i`, `(x) => x * 2`). `xs.map((v) => print(v))` stays unwarned. A dropped `filter` could be warned of always, as a filter's function run for its effect is rare. The message as for `push`: `` `map` gives a new list and changes nothing, and it is dropped here: write `items = items | map(...)` ``.

### Q-1361. A `state` whose initializer reads another state, or a `let` that does, never follows it, and check says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (making a video) | 1 |

In the Groceries component, `let total` written as a state:

```quillo
let open = items | filter((i) => !i.done)
state total = open | map((i) => i.cents * i.qty) | sum()
```

`quillo check` says `ok (2 routes, 1 function, 2 components, in 2 files)`. Ticking an item changes "N left" (`len(open)`, a let) but not the total: "40 left · $555.52" became "41 left · $555.52".

The user guide says `state count = start` is "set once", and the guide for 0.8.10 says "`let` is already derived". Check already warns of the same mistake written as an assignment in the body (Q-764): `b = a * 2` gives `` `b = ...` runs once, as <C> starts: it does not follow `a` when that changes; for a value that follows it, write `let b = ...` in place of the state ``. Written as the initializer, it is not warned of.

```quillo
component C(start: int) {
    state a = 1
    state b = a * 2      // reads a state: set once, not warned
    let c = a * 2
    state d = c + 1      // reads a let that reads a state: set once, not warned
    state s = start      // a prop, read once as documented: fine
    <div>
        <button id="go" onclick={() => { a += 1 }}>+1</button>
        <p id="out">a={a} b={b} c={c} d={d} s={s}</p>
    </div>
}
```

Saved as [repros/Q-1361-state-initializer-reads-state.qwe](../repros/Q-1361-state-initializer-reads-state.qwe). `quillo check`:

```
Q-1361-state-initializer-reads-state.qwe: ok (1 route, 0 functions, 1 component)
```

In the browser: `a=1 b=2 c=2 d=3 s=7`, then after +1 `a=2 b=2 c=4 d=3 s=7`, then `a=3 b=2 c=6 d=3 s=7`.

| Impact | |
|---|---|
| **Functionality** | A value meant to follow another one is frozen at its first value |
| **User** | A total or count that disagrees with the rest of the page, with nothing said |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** the warning `run_once_assignments` gives, for a `state` whose initializer reads another state of the component, or a `let` that reads one: `` `state total = ...` is set once, as <Groceries> starts: it does not follow `open` when that changes; for a value that follows it, write `let total = ...` ``. Not for a prop alone (`state s = start` is documented as read once) and not for a call that loads (`state x = load()`, `live(...)`).

## Where the AI struggled

Both mistakes were made on purpose for the video, so there were no wrong turns; the cost is that the video had to say that Quillo's check misses them as React's and the others' tools do.
