# Feedback from making "Show It Only When", a video on conditional rendering in ten frameworks

| | |
|---|---|
| App | `~/dev/quillo-videos/videos/239-reactive-conditions/app/`: one page of five small boxes (a details toggle, a three-way `match`, an unread badge, a log-in/sign-up switch, a counter that hides), 60 lines, one file; the same page in React, Vue, Svelte, Solid, Reflex, NiceGUI, Streamlit, Leptos and Dioxus in `compare/`. Made by quillo-videos-88 for QUEUE 227 |
| Quillo | `quillo 0.8.11 (3602f18, 2026-11-25)`, from the release; first made on 0.8.10 (61997ab); Q-1400's repro also on the repo's `target/release/quillo`, 0.8.11 (ff565c6) |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`, `quillo run`, Playwright (Chromium) playing each box (`compare/run.mjs`), `node feedback/run.mjs Q-1400` |

What worked well: every condition was written where the markup is (`{open && ...}`, `? :`, `match` in markup) and followed its state with no wrapper, no `move ||`, no `.into_any()` and no `rx.cond`. A branch is made anew when another is chosen, so the typed email did not leak from "Your email" into "Work email" (React and Reflex carry it over). `hidden={!shown}` kept a hidden counter's count, as React needs `<Activity>` for. The documented trap of a `state` assigned on its own line (`label = open ? ...`) is warned of by check with the fix.

Numbers Q-1400 to Q-1401, from the block Q-1400 to Q-1409.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-1361 | `state label = open ? "Hide details" : "Show details"` in a component: the label never follows `open`, and check is silent on 0.8.11 (the assignment form is warned of) | 1 |

## New issues

### Q-1400. `n && <markup>` loses check's "0 counts as true" warning once a handler assigns `n`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a browser run (making a video), then `quillo check` | 1 |

The guide says check warns about `count && <Badge/>` where the value is known to be a number, and it does while the state is never assigned: `state unread = 0` then `{unread && <span>...}` gets "`unread` counts as true even when it is 0". Add the button that every real badge has, `onclick={() => { unread += 1 }}` (or `unread = 0`), and the warning is gone: check says ok, and the page shows "0 new" at 0.

```quillo
component Inbox() {
    state unread = 0
    <div>
        <button onclick={() => { unread += 1 }}>+1 unread</button>
        <p>Inbox{unread && <span>{unread} new</span>}</p>    // check: ok; the page: "Inbox0 new"
    </div>
}
route GET / = <html><body><Inbox/></body></html>
```

Repro: `repros/Q-1400-zero-and-badge-after-assignment.qwe`, open on 61997ab, 3602f18 and ff565c6.

| Impact | |
|---|---|
| **Functionality** | A badge, a "0 items" note or an empty-state message shows when it shouldn't; the warning meant for this case is missing in the case that occurs |
| **User** | Sees "0 new" |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** keep a state's type from its initial value and from every assignment (all ints here), and warn on `&&`, `||`, `!`, `if` and `? :` as for a never-assigned one.

### Q-1401. A `let` assigned on a later line still follows the state it reads (the guide says it doesn't)

| Kind | Detected by | Attempts |
|---|---|---|
| docs | a browser run (making a video) | 1 |

The guide's table: "`let y = ...` whose name is assigned somewhere (`y = 5`): a plain variable, never by itself", and "an assignment alone in the body runs once". Written so:

```quillo
component A() {
    state open = false
    let label = "Show details"
    label = open ? "Hide details" : "Show details"
    <div><button onclick={() => { open = !open }}>{label}</button></div>
}
```

check says ok, and in the browser the button follows `open` ("Hide details" after a click, "Show details" after the next). With `state label = "Show details"` in its place, the label stays and check warns. Either the behavior or the table is off; a reader of the table expects a stale label.

| Impact | |
|---|---|
| **Functionality** | None seen: the page is right |
| **User** | No change |
| **Requests** | No change |
| **Security** | No change; the guide's model of `let` disagrees with what runs |

**Suggestion:** say in the table what an assignment to a `let` in the body does (it follows, as here), or make it run once with the same warning as for a `state`.
