# Feedback from building 200 Mobile components in Quillo

| | |
|---|---|
| App | None: 200 new components for the Quillo UI kit, in a new `Mobile` group, for phones and tablets (180 for phones, adapting to tablets, and 20 for tablets), on branch `smart-components` of `~/dev/quillo-components`. 877 components in all. Twenty batches of ten |
| Quillo | 0.6.0 (5f7f76c, 2026-11-01), a pinned copy of `target/release/quillo`. It predates `feedback26-fixes`, so a few things below may already be fixed there, and they say so |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, and headless Chrome with real touch input at 390 by 844 and 820 by 1180, or 820 by 1180 and 1180 by 820 for the tablet batches. `--watch` for components remade on load, by hand |

**Status of the components:** all 877 pass on both 0.6.0 binaries, d702589 and 378e64b, the one live since 2026-11-01, at kit b03290b: every test, by slices of about 110 components, once on each binary, 2,154 runs. Seven failed: `water_tracker` on both, a demo and test written for 2026-11-01 that began a new day, rightly, from the 2nd (fixed); and `bottom_sheet`, `markup_overlay` and `workout_timer` once each under load, each then passing three times alone on each binary. The whole gallery, 877 sections and 1,921 islands, loads and wakes with no error on both. Moving the browser's clock 45 days and a year ahead found one more demo with a date in it, `countdown`, which counted to 2027-01-01 (fixed). The kit's benchmark: 378e64b starts 744 components in 1.1 s and 372 MB, d702589 in 1.4 s and 502 MB.

**What this does not show.** Nothing here ran on Safari's engine, in WKWebView, in Android's WebView, on a
simulator, on an emulator or on a real device. Everything is Chrome's phone emulation. There was no real
on-screen keyboard (`visualViewport` was a stand-in), no real safe areas in most tests, no real `quilloNative`
(a stand-in in every test), no real pen, no screen reader, and no real rotation (a change of metrics, with
no `orientationchange`). The biggest single risk: stopping an Apple Pencil from scrolling depends on code that
has never run, as a CDP pen sends no touch events. Findings about the apps, marked **read, not run**, come
from reading `App.swift`, `Bridge.kt` and `MainActivity.kt`.

**How far to trust this:** each batch ran its own repros. Those with a repro below were run a second time, in one
session, on the version above, with real touch where it matters. The exceptions are named. Timings are from a
shared machine.

**Being fixed while this was written.** Another session read a draft of this report and began fixing it on branch
`feedback29-fixes`. At 12:30 on 2026-11-01 its working tree (6ec94b4 plus uncommitted changes, built as
`target/release/quillo`) behaved like this on the repros below. It is work in progress, so take this table as a
snapshot, not as a statement of what is fixed:

| On that build | Issues |
|---|---|
| Behaves as expected | Q-334 (`==` on elements), Q-337 (`style` and `class` keep what a handler set), Q-338 (`muted`, `<details open>`), Q-340 (`preventDefault` after `js()`), Q-342 (dead rows), Q-343 (capture kept), the `@load` part of Q-345, Q-347's `"decimal"` (now a warning). Q-339 and Q-346 have changes in `src/mobile.rs` and the two shells that were read, not run |
| Unchanged | Q-341 (`js("navigator.vibrate")` still calls it), Q-345's first, second and fourth parts, Q-347's `xs.push(3)`, `index_of(...) < 0` and `js("setTimeout", ...)`, which still passes check and stops `run`. Q-348 moved from check to `run`, with the same false message |
| Not settled | Q-335: in one probe the callback's button kept its first text, which may mean it now waits and the probe read too early |

Every issue below is written as it stood on 5f7f76c, where it was found.

**Numbering:** Q-334 to Q-349, the range left between FEEDBACK27 and FEEDBACK29, so related findings are grouped to
fit. None is in `feedback/INDEX.md`, and they have no files in `feedback/repros/`: the repros are in this report.

What worked well: gestures. `@pointerdown`, `@pointermove`, `@pointerup` and `@pointercancel`, `setPointerCapture`,
`e.timeStamp`, `e.pointerType`, `touch-action`, `@touchmove` that can cancel a scroll, `ResizeObserver` and
`IntersectionObserver` through `js_new`, `visualViewport` listeners, `<dialog>` with `@close`, `every` under a state
as a long-press timer: every batch called these the smoothest part of the language, and most worked first time.
Keyed rows kept focus and typed text while lists re-sorted under them. A sensor reading through a Quillo handler
cost about 45 microseconds. No component was remade on load in any batch's final check. A barcode reader working on
camera frames runs in Quillo at usable speed.

The theme of this report is **the seam between Quillo and the page**. Inside Quillo the language is careful. Where it
meets a page object, a JavaScript function, a Promise or a hand-set attribute, the rules change without a sign: what
waits, what compares, what is kept. A second theme is the phone apps themselves, whose shells drop links, differ from
each other, and lack calls that a real phone app needs.

## What the fixes broke: the kit on 378e64b

The fixes for this report and FEEDBACK29 were merged to `main` as 378e64b. Run on it, the kit's 877
components failed in about 120 places, 40 of them components that were live and passing on d702589. Some
were kit code that leaned on old bugs, such as `context_menu` finding the pressed item with `==`, which
only worked because any two plain elements were equal. The rest are below. All 877 now pass on both
binaries, as kit `87110b8` works round each one, but a program outside the kit meets them as written.

These start at Q-415, as Q-334 to Q-349 were used up and other reports reached Q-414. Each was found by one
or more of the eight agents fixing the kit, and the four marked so were run again here on both binaries.
Repros are inline; none is in `feedback/INDEX.md` or `feedback/repros/` yet.

### Q-415. A handler pauses where nothing waits, and the page updates in the middle of it

| Kind | Detected by | Attempts |
|---|---|---|
| silent, a regression from Q-336 and Q-340 | kit tests, in all eight slices; run again here | many |

```quillo
component P() {
    state a = 0
    let note = () => null
    <button id="m" @keydown={(e) => {
        let was = a
        let step = match e.key {
            "x" => 1
            _ => 0
        }
        a = was + step
        note()
    }}>{a}</button>
}
```

Two keydowns sent one after the other give `2` on d702589 and `1` on 378e64b. Nothing in the handler
waits, but 378e64b compiles it `async`, as it calls a function, and then each `match`, `if` block and `for`
in it becomes `await (async () => ...)()`, which always yields. `finish_wait` in `src/client.rs` emits
`(W.waits(t = call) ? await W.settle(t) : t)` through `await_if(true, ...)`, which makes a function
`async` though its await is conditional, and every call of an `async` component function is awaited.
Every `x.m(...)` counts, `date.valid`, `date.parts` and `re.replace` included.

At that pause the page updates. So `e.target` can already be off the page (`null has no method
querySelector`), `e.currentTarget` is null, a second event sent at once reads state the first has not set,
and a pointer move or lift can run before the press that came first. For an event sent by a script,
`preventDefault` after the pause comes after `dispatchEvent` has returned. Real key presses are not
affected: with real arrow keys the page did not scroll on either binary, run here, as the browser runs
what is pending before acting on a real event.

It cost the most: about 30 components, from context menus and key grids to a brush chart and a swipe deck.
Workaround: find elements and keep `e.currentTarget` before any state changes, and move logic a handler calls
into top-level `fn`s, which do not wait on either binary.

**Suggestion:** await only a value that is a Promise, and do not make a block or a function `async` because a
call in it might return one.

### Q-416. A component's own function given to `map`, `filter` or `reduce` is refused, and the page stops updating there

| Kind | Detected by | Attempts |
|---|---|---|
| silent while rendering, loud in a handler | kit tests; run again here | 1 to 2 each |

```quillo
fn twice(x: int) -> int = x * 2
component Sums() {
    state out = "none"
    let double = (x: int) => twice(x)
    <div>
        <p>{join([1, 2, 3] | map(double), ",")}</p>
        <button id="go" @click={() => { out = join([1, 2, 3] | map((x) => double(x)), ",") }}>{out}</button>
    </div>
}
```

On d702589 the click shows `2,4,6`. On 378e64b it stays `none`, and the console says
`` `map` cannot call a function that waits, on the server or the page (`js` in it waits) `` though there is no
`js` in it. `double` only calls a top-level `fn`. The same goes for one that calls `money.of` or reads a field of
what a call gave. `quillo check` says ok. When this happens while the page renders, as in `realtime_chart`,
that part of the page silently stops updating. Eleven kit components were moved to top-level `fn`s for it.

### Q-417. A state that starts from a module's method is null at first in a component the page makes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `task_row`'s Undo; run again here | 1 |

```quillo
fn shout(s: str) -> str = upper(s)
component Child(today: str = "") {
    state day = date.valid(today, "date") ? today : "none"
    <p.child>{shout(day)}</p>
}
component Parent() {
    state show = false
    <div><button.go @click={() => { show = true }}>go</button>{show && <Child today="2026-11-01"/>}</div>
}
```

After the click, d702589 shows `2026-11-01`; 378e64b shows nothing, and logs
`` argument `s` of `shout` must be str, got null ``. The initializer is compiled `async`, by Q-415's rule, so the
state is null until it settles. A component the server renders hides this. 20 kit files start a state from
`date.`, `re.`, `money.`, `math.`, `stats.` or `vec.`; only `task_row` showed it in a test.

### Q-418. A replayed `@load` has no image, and the real one comes as well

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `image_crop`, which then broke the whole gallery page; run again here | 1 |

An `<img @load={...}>` the server rendered, already loaded when the component starts, logs on d702589
`load tag=IMG w=600`, once. On 378e64b it logs `load tag=null w=null`, then `load tag=IMG w=600`: the replay that
Q-345's fix added is a `new Event` that is never dispatched, so its `target` is not the image, and the browser's
own `load` still arrives. A handler reading the image's size gets null and runs twice.

### Q-419. A prop's Promise of nothing and a prop that gives nothing are the same

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `pull_to_refresh` waited for ever | 1 |

As a prop's call is now waited for (Q-335), `cb={() => js("Promise.resolve")}` and `cb={() => { later = 1 }}` both
give the caller `null`; d702589 gave `promise` and `null`. A component that told "the answer is coming, call `done`
later" from "this was the answer" can no longer, and waited for ever.

### Q-420. A page object's method takes the name of the component's own function

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `voice_recorder` stuck at "starting" | 1 |

```quillo
component P() {
    state out = "idle"
    state rec = null
    let resume = () => { if rec != null { rec.resume() } }
    let start = () => {
        let r = js("Promise.resolve", js_new("Promise", (done) => {
            let ctx = js_new("AudioContext")
            ctx.resume()
            done(true)
        }))
        out = is_err(r) ? "err" : "ok"
    }
    <button @click={start}>{out}</button>
}
```

`ctx.resume()` falls back to the component's own `resume`, which is `async` on 378e64b, so the function handed to
the Promise becomes `async` too. When `js_new("AudioContext")` throws, that no longer rejects the Promise: it
never settles, and the handler hangs with an uncaught error. d702589 gives `err`. Workaround: no component function
named like a method the code calls. Not run again here.

### Q-421. Five changes in what code means, with no warning

| Kind | Detected by | Attempts |
|---|---|---|
| silent or loud | kit tests and `quillo check` | 1 each |

Each is a fix or a choice that can stand, but code written for 0.6 breaks without a word. The changelog gives
the first, second and last as fixes, not as something a program must change; the third and fourth are not in it:

| Was | Now | Broke |
|---|---|---|
| A component given no children has empty HTML | `null`, so `str(children) != ""` is true | five components lost their default text |
| `js_new("Promise", ...)` is waited for: FEEDBACK25 and this report give it as the way to catch a throw | it stays a Promise, so a throw is an unhandled rejection | 131 places in the kit, now `js("Promise.resolve", js_new("Promise", ...))` |
| A child's `@mount` runs before its parent's | a parent's can run first | `large_title_bar` observed a null element |
| `session` is a name like any other | `quillo check` takes it for the request's session: `cannot set metadata on session` | `now_playing`, and the whole gallery would not start |
| `==` on page elements is true for any two plain ones | identity, as Q-334 asked | kit code that leaned on the old answer |

**Suggestion:** a line for each in the changelog under "what to change in your code", and a check warning for
`str(children)`.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-302 | The brief warned of it, and it was avoided | 0 |
| Q-303 | Avoided; tests now cover the keys `constructor`, `toString` and `__proto__` | 0 |
| Q-305 | Avoided: no `?` in any component body | 0 |
| Q-306 | A wider form: `{cond && f(x)}` rebuilds `f`'s elements whenever anything `cond` reads changes, though the branch stays. The state is not even passed in. `{cond && <div>{f(x)}</div>}` keeps them. It is what "keeps each tab's state" hangs on | 2 |
| Q-316 | Two `let` functions in a component cannot call each other either, as a `let` is not seen ahead. A menu needs this shape: closing removes the listener that closes | 1 |
| Q-317 | SlotFinder's limit again. The guide now describes `date.valid_zone`, which this binary refuses (`date has no function valid_zone`), so OrderStatusCard takes an offset in minutes | 1 |
| Q-318 | A throw from a page's function stops the handler: `new Notification()` on Android's Chrome, `localStorage` when blocked or full, `js_new("Intl.DateTimeFormat", "bad locale!")`. INDEX says `??` takes a stop on `feedback26-fixes`, which this binary lacks, so try this on a newer build | several batches |
| Q-320 | `sort_by` with a list as the key: a 500 here, in three batches; the guide on the working branch says it works, so this is the binary. Also `min([])` through an untyped parameter, `re.find` used as text, and `re.find_all` read as strings, which the guide's table of equivalents suggests at line 336 | 1 each |
| Q-323 | Worse on a phone: `NotAllowedError` (a person said no: show Settings) cannot be told from `NotFoundError` (no camera), as `e.name` stops the handler. Five components have the same workaround. My first version asked for the camera twice, to read the name | 1 |
| Q-324 | `for x, i in xs`, `</ol.a>`, `10 ** 2`, `n // 2` (a comment, so the error names the next line), `chr(104)`, and `<style>.a{font:15px}</style>` (`` expected `}`, found `:` ``, with no word about style) | 1 each |
| Q-324, one more | A backtick in a CSS comment inside `` css`...` `` ends the template; the error, `` expected end of statement, found `tick` ``, names the word after it | 1 |
| Q-325 | Every check error in this work pointed at a line of `quillo-ui.qwe` | many |
| Q-014 | `quillo: element` and `quillo: text` in a plain run name no component | 5 reductions in one batch |

## New issues

### Q-334. `==` calls any two page elements equal

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a real finger, twice independently | about 5 runs, a log of pointer events and the compiled bundle |

```quillo
component A() {
    state said = "not run"
    <div @mount={(el) => {
        let a = el.querySelector("#a")
        let b = el.querySelector("#b")
        said = `${a == b} ${a != b} ${index_of([a, b], b)} ${a.isSameNode(b)}`
    }}><i id="a">a</i><i id="b">b</i><span>{said}</span></div>
}
```

It prints `true false 0 false`. `eq` in `runtime.js` falls to its map case, as any object that is not a list, error,
Html, decimal or event counts as a map, and `Object.keys` of an element is `[]`: two empty maps.

**The title is not quite the rule, and the exact rule is worse.** Elements compare by the own fields Quillo leaves on them:

| Compared | `==` |
|---|---|
| Two plain `<i>` | `true` |
| Two links | `true` |
| Two elements with dynamic `class={...}` | `true` |
| A plain element and one with `@click` | `false` |
| Two buttons with different `@click`s | `false` |

So a test on buttons passes, and the same code fails silently on plain links and on what a page or a library made.
It is not only elements: two `MediaStream`s compare equal too, so StoryCapture's late `ended` from the old camera shut
the new one. Four
batches saw `==` "work" in one place and fail in another before this was pinned down. EdgeDrawer's Tab wrapped to the
start from a plain link in the middle, and SearchHeader's arrow keys jumped to the last plain link, both past a passing
first test.

Any component comparing `e.target`, `document.activeElement` or a node in a list is wrong without a word.
SplitView's `kids[n] == p` gave `[0, 1, 2]`, so each press said the first pane was in use, which moved the panes under
the finger, so the click landed on another row. KeyboardBar's Previous stayed off on the second field. SheetSelect's
arrow keys had the same bug with a passing test, since one ArrowDown from the first row looks right. RotationLayout
passed every test with the bug in it, because Chrome's scroll anchoring did the work its code was failing to do: it
would have failed on Safari alone.

Workaround: `a.isSameNode(b)`.

**Suggestion:** two foreign objects compare by identity. The existing kit has not been searched for this.

### Q-335. A function given as a prop is not waited for, so a wrong answer looks right

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probes, and PinPad's own design, in seven batches | 1 to 3 |

```quillo
server fn fetched(x: str) -> str = x + "!"
component Child(cb) {
    state out = "start"
    <button @click={() => { let r = cb("a"); out = `${type_of(r)} ok=${r.ok}` }}>{out}</button>
}
component Top() { <Child cb={(x) => fetched(x)}/> }
```

It shows `promise ok=null`. A `server fn` called directly in a handler waits; the same call through a prop does not,
and a field of a Promise reads as null, not as an error. This is the kit's normal shape. Code written as
`if on_pay(order) == false { failed }` shows "Paid" for a refused payment. PinPad's first version did `ok == true`, so
every right code would have counted as wrong and locked the owner out, and its test passed because the demo answered
on the spot. Near Q-102, which covered a component's own `let` functions.

It goes further: an inline function in a prop and the same code in a named `let` differ. Measured with a 300 ms wait:
inline with `js_new("Promise")`, 1 ms, type `promise`; inline with a `server fn`, 4 ms, type `int`; a named `let`, 302 ms,
waited for. The same words wait or do not by whether the function has a name. Two demos passed with no wait in them.

Workaround: `js("Promise.resolve", cb(...))`, or `if type_of(a) == "promise" { a.then(ok, fail) } else { ok(a) }`, and
test with both a Promise and a plain answer.

**Suggestion:** one rule: a Promise is waited for wherever code can wait.

### Q-336. What waits and what does not has no single rule

| Kind | Detected by | Attempts |
|---|---|---|
| silent | first runs and probes, in five batches | 1 to 3 each |

| Written | What happens | Workaround |
|---|---|---|
| `x.method()` giving a Promise, such as `img.decode()`, `audio.play()` | Not waited for, though `js()` is. `is_err` is false, and a refusal is an uncaught error | `js("Promise.resolve", x.method())` |
| `js_new("Promise", ...)` kept to use later | Waited for, so the handler stops on that line until it settles, with nothing in the console | Start the work where it is awaited |
| `js()` in a top-level `fn` | Not waited for, where a component's `let` function waits, so helpers cannot be shared | Local functions, repeated in each component |
| Two `done`s, a timer's and a waited one | The timer's first `done` is ignored while the body waits, so a timeout does nothing: `plain value late 603`, not 200 ms. It looks right | `a.then(done, ...)` in place of `done(js("Promise.resolve", a))` |
| `.then((x) => ...)` on a Promise of `undefined`: `play()`, `close()`, `clipboard.writeText`, most Web promises | `missing argument x in call to <fn>`, and the success path is skipped. In DataSaverToggle the code after it never ran, so `on_change` was skipped | A default: `(x = null) =>` |
| A `let` lambda calling `js()`, given to `map` or `filter` | `` `filter` cannot call a function that waits on the server ``, though no server is involved, and check says nothing | `for` |

The two-`done` result was not run a second time; the rest of the table was run by at least two batches.

### Q-337. `style={...}` and `class={...}` wipe what a handler set, and a drag jumps home

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone | a probe, then real bugs in eight batches | 1 |

```quillo
component Box() {
    state n = 0
    state m = 0
    state on = false
    state el = null
    <div>
        <div class={on ? "on" : "off"} style={`--a: ${n % 1}`} @mount={(e) => { el = e }}>{m}</div>
        <button @click={() => { el.style.transform = "translateX(5px)"; el.classList.add("hand"); m += 1 }}>a</button>
        <button @click={() => { n += 1 }}>b</button>
        <button @click={() => { on = !on }}>c</button>
    </div>
}
```

| After | `style` | `class` |
|---|---|---|
| `a`, a state neither reads | `--a: 0; transform: translateX(5px);`, kept | `off hand`, kept |
| `b`, what `style` reads, the text coming out the same | `--a: 0`, **wiped** | |
| `c`, what `class` reads | | `on`, **wiped** |

So the attribute is written whole whenever what its expression reads changes. `style` writes even when the text is the
same; `class` compares first. A prop counts, and a prop can follow a parent's state. Moving an element by
`el.style.transform` in the handler is the right way to keep a drag smooth, and building `style` from state on the
same element is the kit's habit, so together a drag jumps home when that one state changes mid-gesture. `quillo check`
says nothing. The guide's "each place follows only that value" reads as though the same text would change nothing.

It was a real bug in WidgetGrid, ScrubBar (a media `timeupdate` wiped the drag: the thumb went from 0.75 to 0.589),
TableFloorPlan, VoiceNoteList, CalendarSheet's class, and more. **A test can pass with this bug:** one first test
changed a state mid-drag that the style did not read, and passed with the bug in place. It has to change what the
attribute reads. A hand-set `hidden` is the same from the other side: it is written only when what it reads changes.

Workaround: no `style={...}` on what a handler moves; the computed value goes on a parent, and a hand-set class goes on
an element whose `class` is static.

**Suggestion:** `style` compares its text first, as `class` does. Better, set only the properties the markup names.

### Q-338. `muted={x}` and `<details open={x}>` do not hold what they look like they hold

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe, and a test that a folded section stays folded | 1 each |

```quillo
component P() {
    state quiet = true
    <div><video id="v" muted={quiet} playsinline></video><button @click={() => { quiet = !quiet }}>x</button></div>
}
```

After the click, the `muted` attribute has gone and `v.muted` is still `true`. The icon follows the state, and the video
stays silent: an unmute button written the obvious way never gives sound, and a test reading the attribute passes.
`muted` the attribute is only the default. The guide says `value`, `checked` and `selected` are held to their state.

```quillo
{for r in rows => <details key={r.t} open={r.t == "A"}><summary>{r.t}</summary>{r.n}</details>}
```

A person closes it, the row's item changes, and it opens again, undoing what they did, though `open={...}` gave the same
value as before.

Workaround: `muted` written plain and `v.muted = ...` in handlers; the folded state held by hand with `@toggle`.

### Q-339. A phone app gets a new origin at every launch, so `localStorage` is silently empty

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone only | reading `src/mobile.rs`, then two ports in one Chrome profile | 1 |

`src/mobile.rs` serves on `"127.0.0.1:0"`, a port the system picks, and I confirmed that line. A page's origin includes
its port, and `localStorage`, IndexedDB and the Cache API go by origin. Cookies do not go by port. One program, one
Chrome profile, two ports as two launches:

```
launch on port 9116: localStorage=null cookie=
launch on port 9117: localStorage=null cookie=seen=1
```

Something a page writes to last until the next launch is gone, with no error. The guide and `mobile/README.md` do not
mention it. **This was shown on desktop Chrome with two ports, not seen on a phone**, and script-written cookies may
be capped at 7 days in WKWebView. It reaches every component in the kit that remembers in `localStorage`, among them
`announcement_bar`, `breaking_news`, `coachmark`, `spotlight_tour` and `autosave_form`: in a phone app their "stays
closed" lasts until the app is closed.

It was a real hole in PinPad: its lock-out after wrong tries lived in `localStorage`, so closing and opening the app
escaped it. GestureTutorial came back at every launch, and NotifyPrompt asked again each time.

Workaround: keep what must last through a `server fn` and SQLite. Batch 30 tested this: after a fresh Chrome profile on a
kept SQLite file, the queue, the draft and the upload list all came back, though a file cannot be kept that way.

**Suggestion:** a fixed port per app, kept in the app's data folder, or a custom scheme with a stable origin.

### Q-340. Any `js()` in a handler makes the rest of it late, even a plain read

| Kind | Detected by | Attempts |
|---|---|---|
| silent | EdgeDrawer's Tab test, a fuzz page and a reduced-motion test | 1 to see it, then the bundle to know why |

```quillo
component Late() {
    <a href="#after" @click={(e) => {
        let t = js("document.title")
        e.preventDefault()
    }}>x</a>
}
```

The link is followed: `location.hash` is `#after`. `document.title` is a plain read and nothing is waited for, yet `js()`
compiles to `await W.settle(B.js(...))`, so everything after it runs in a microtask, after the browser has acted. The
guide's warning names "a `server fn`", so nobody would think of `js("getComputedStyle", el)`.

The same seam shows two more ways. State set in `@mount` before a `js()` runs what follows with the state half set,
which broke `open={true}` at load in five components, and no demo opened at load, so a fuzz page caught it. And a child's
`@mount` runs before a parent's waiting `@mount` goes on: ShortVideoFeed autoplayed under "reduce motion" for that reason.

Workaround: `preventDefault` first, or keep `win = js("window")` in state and call `win.x`, which does not wait.

**Suggestion:** do not await `js()` unless it gave a Promise.

### Q-341. A page's feature cannot be looked for: `js("a.b")` calls it

| Kind | Detected by | Attempts |
|---|---|---|
| silent or console only | the first probe, in eight batches | 1 |

The guide says `js("window.innerWidth")` "(no arguments reads a value)". For a function it calls it, and a class throws.
Asking whether a feature exists is the first thing a phone component does.

| Written | What happens |
|---|---|
| `js("navigator.vibrate")` | Called: `1 argument required`, and the handler stops. In AlphaRailList everything after that line was lost, far from its symptom |
| `js("Notification")`, `js("window.MediaRecorder")` | `Failed to construct...`, and the handler stops |
| `js("window.quilloNative")` when absent | An error value, so `!= null` is always true, and a plain browser reads as being in the app |
| `js("window").quilloNative` | null, as wanted |
| `js("window").DeviceOrientationEvent.requestPermission` | `` fn has no field `requestPermission` ``, and the handler stops |
| `js("window").Array.from(x)` | `` fn has no method `from` `` |
| `js("window").Object.keys(x)` | Runs Quillo's own `keys` on the constructor |

So a component learns whether it needs a tap for motion only by making the call the tap is for. `quillo check` sees none
of this. Telling "not here" from "refused" means matching the words of an error, and a failure of that test is silent.

Workaround: read it as a field, `js("navigator").vibrate != null`.

**Suggestion:** `js_has("name")`, or a field of a JavaScript function reading as a field of any object.

### Q-342. A row written as `a ? <x/> : <y/>` stays on the page if its list empties after a branch flips

| Kind | Detected by | Attempts |
|---|---|---|
| silent | DragBetweenPanes, then dumping each row's record | several |

```quillo
<ul>{for x in items => x.on ? <li key={x.id}>on</li> : <li key={x.id}>off</li>}</ul>
```

| Steps, from three rows | This form | `<li key={x.id}>{x.on ? ... : ...}</li>` |
|---|---|---|
| Empty the list | gone | gone |
| Flip the rows, then empty the list | **3 dead rows stay** | gone |
| Flip the rows, then shrink to one | right | right |
| Flip, flip back, then empty the list | **3 dead rows stay** | gone |

One arm as text does the same, and `x.on && <li/>` and two text arms are fine. The first report said such a row can never
be removed; the exact condition is narrower, as the table shows. From reading `runtime.js`, not confirmed by changing it:
a `key` inside a `? :` is not the row's key, so the row goes by its place; when it flips, the row's `nodes` keep the
first element; `clearRows` takes that as the first node, finds it has no parent, and removes nothing. The guide says the
key is the row's element's.

36 rows in the kit have this shape, most over static data. It has not been shown to bite any of them but the one where
it was found. Rows that can really empty after a flip need looking at: `json_viewer`, `terminal_output`, `story_viewer`.

Workaround: one element for each row, with the branch inside it.

### Q-343. A keyed row holding pointer capture loses it when the list moves that row

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone | asking what else a state change does under a finger | 2 |

```quillo
{for r in order => <li key={r}
    @pointerdown={(e) => { e.target.setPointerCapture(e.pointerId); log = log + ["down " + r] }}
    @pointermove={(e) => { if !moved && e.clientY > 40 { moved = true; order = ["b", "c", "a"] } else { log = log + ["move " + r] } }}
    @lostpointercapture={(_e) => { log = log + ["lost " + r] }}
    @pointerup={(_e) => { log = log + ["up " + r] }}>{r}</li>}
```

With real touch: `down a,move a,move a,reordered,lost a,move b,move c,move c,move c,up c`. The capture goes at the
reorder, and the rest of the gesture lands on other rows. The guide says moved rows keep their DOM and focus. It depends on
which row Quillo chooses to move: with `["b", "a", "c"]` the batch's run kept the capture, so a test in one direction
passes and the other fails. The cause, a move being a removal and an insert, is inferred.

Workaround: capture on a stable parent, and commit the order at release.

**Suggestion:** `moveBefore` where the browser has it.

### Q-344. A cursor counts UTF-16 units and `slice` counts characters, and nothing converts

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading what was about to be built | 1 |

With `😀 @an rest` and the cursor after `an`: `len` is 10, `selectionStart` is 6, and `slice(t, 0, 6)` is `😀 @an ` with
the space, one past the cursor. They agree until an emoji comes first, and on a phone an emoji keyboard is one tap away.

**The kit's existing `MentionInput` breaks this way:** `hi @an rest` offers 1 person, `😀 @an rest` offers 0, and nothing is
logged. This was run by batch 19 and not a second time. `markdown_editor`, `code_editor`, `masked_input`, `currency_input`
and `iban_input` also read `selectionStart` and have not been run for it.

Workaround: `js_new("String", text).slice(...)` and `.length`, which count as the field does.

### Q-345. Four ways a component loses part of itself, with no word

| Kind | Detected by | Attempts |
|---|---|---|
| silent | tests that looked like other bugs | 3 to 5 reductions each |

```quillo
view V() {                         // 1. the page has no sheet, and #p stays black
    style global v_css
    <Inner/>
}
component VC(children) {           // 2. the aside stays empty, for good
    state c = null
    <aside>{children == null && (c != null ? <div>open</div> : <p>none</p>)}</aside>
}
component E() { <div><VC/></div> }
<img src="..." @load={() => { loaded = true }}/>   // 3. never runs if it finished before the component started
let make = Dead
route GET / = <html><body>{make()}</body></html>    // 4. no island, and clicks do nothing
```

1. `style global` in a view whose body is only a component tag sends no CSS. Wrapping the tag in an element fixes it.
2. A nested component given no children has empty html on the server and null in the browser. The console says only
   `quillo: element`, twice, naming nothing, and that place stays empty. Workaround: a flag prop.
3. `@load` and `@error` on a server-rendered `<img>` never run if it finished first. StoryViewer's progress bar stood at
   0 for ever. The guide says what is replayed, and that covers input. Workaround: `el.complete` in `@mount`.
4. A component called as a function renders dead, and check says ok. Workaround: `view XDemo() = <XDemoLive/>`.

All four were run a second time and behave as written here.

### Q-346. The phone apps drop links silently, and answer differently from each other

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone only, **read, not run** | reading `App.swift`, `MainActivity.kt` and `Bridge.kt` | 1 |

**Only `http` and `https` leave the app.** `tel:`, `sms:`, `mailto:`, `itms-apps:` and `market:` are cancelled with no
message (`mobile/ios/Quillo/App.swift:186-192`, `MainActivity.kt:92-98`), and no `quilloNative` call dials. A contact list's
Call button does nothing, where any browser on the same phone would dial. ContactList hands the number to the share sheet
and says so on screen. ForceUpdate and RatePrompt accept only https.

**`quilloNative.share` answers `{ shared, to }` on iOS and plain `true` on Android**, and a field of a bool stops the
handler. That half was run: `quillo: bool has no field shared`. So `if r.shared != false` works on iOS and stops the handler
on Android, and tests with an iOS-shaped stand-in pass. Android cannot tell a cancelled share from a sent one.

**The two shells give a page different safe areas and keyboards.** iOS: top inset 0, the home bar at the bottom, and the
keyboard shrinks the visual viewport only. Android: all insets 0, and the keyboard shrinks the whole window. Neither adds
`viewport-fit=cover`.

**Android cannot give brightness back:** `screenBrightness` is kept in 0 to 1, and "follow the system" is -1, so the window
stays fixed until the app restarts.

**Suggestion:** open those schemes with the system; one answer shape, written in `mobile/README.md`.

### Q-347. `quillo check` passes what fails at once or does nothing

| Kind | Detected by | Attempts |
|---|---|---|
| loud, late, or silent | failed runs | 1 each |

| Written | `quillo check` | Then |
|---|---|---|
| `js("setTimeout", () => { ... }, 50)` | ok | `quillo run` refuses to start: "it runs text as code", of a call given a function. One demo line stopped the whole gallery |
| `js("Reflect.set", ...)` | ok | The same |
| `js("...getCurrentPosition.bind")` | ok | Starts, then stops the handler |
| `xs.push(3)` | ok | Nothing: it is `push(xs, 3)` with its answer dropped, on both sides. It reads as the JavaScript call |
| `i < 0` after `index_of` | ok | `cannot compare null with int`: a 500, or a stopped handler. A miss gives null, and -1 is what anyone from JavaScript writes |
| `type_of(x) == "decimal"` | ok | Never true: the name is `dec`. Seven guards had it, no test caught it, and a decimal fare would have become 0 |
| `g.a = 2`, where `g` came through a state | ok | Stops the handler. A plain object from the page gets the same refusal, whose advice, `m = set(m, ...)`, cannot work on an object the page owns |
| `anim.onfinish = fn` | ok | "it runs text as code or HTML", though the value is a function. `addEventListener("finish", ...)` works, and the message could say so |
| `px.length` on a list from `js("Array.from", px)` | ok | `list has no field length`, in the console once per frame, so a camera loop stops silently for the person holding the phone |

`xs.push(3)`, `index_of`, `"decimal"` and `setTimeout` were run a second time. `.bind`, `Reflect.set` and `.length` were not.

**Suggestion:** have `quillo check` run the browser compile; warn on a builtin's answer left unused, and on a type name
that does not exist.

### Q-348. Assigning a local named like a global gives a false error

| Kind | Detected by | Attempts |
|---|---|---|
| loud, false | a probe of GroupedList with `let rows = ...` | 1 |

```quillo
let rows = [1]
fn other(rows) {
    rows = rows + [1]
    rows
}
component P() { <b>{len(rows)}</b> }
```

`` error: <P> runs in the browser, but `rows` is a server variable that changes ``. Nothing changes the global: `other`
assigns its own parameter. A component's `state rows` does the same. The kit has state called `rows` in several files, so
a program with `use quillo-ui.qwe` cannot have a top-level `let rows` that a component reads, and is told something untrue
about its own code. Any common word can do this, and the line shown has nothing to do with the cause.

Workaround: another name.

### Q-349. Nothing says "after the page has updated", and nothing is a one-shot timer

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | tests, in six batches | 1 each |

A statement runs before the page is updated, and there is no way to ask for after. `depth = 2; root.querySelector("h2").focus()`
does nothing, as the column is still hidden. `peek = false; button.focus()` fails on an `inert` page. `scrollTop = 572` on 12
rows sets 484, so a wheel settles there and a value quietly changes: 13:05 became 11:05. All silent, and every panel that
opens needs this. A cursor, a focus and a scroll are the three things a phone UI sets right after a change.

Workarounds, each worth saying plainly: `js("Promise.resolve")` with nothing to resolve, which works and reads like a mistake;
a one-shot `every(30, ...)` under a state; holding the target and setting it until it holds.

`setTimeout` is refused and `quilloNative.wait` is only in the apps, so a one-off is `if press { every(280, () => { press = false; ... }) }`,
which works well and takes thought, or `js("AbortSignal.timeout", ms).addEventListener("abort", ...)`, which nobody should have
to find.

**Suggestion:** `after(ms, fn)` and `tick()`, or both idioms in the guide.

## Where the AI struggled

| What was written first | Why | What happened | Cost |
|---|---|---|---|
| "Move by `el.style.transform`", in this work's own brief | The usual advice | It collides with `style={...}` (Q-337), and it was found when ten agents were already building to it | A correction to nine running agents |
| "Any state change wipes it", in that correction | One probe | Six batches measured it: only what the attribute reads. Each was right to say so | 0, and a better rule |
| The same corrected rule, as "a test that changes state mid-drag" | It sounds sufficient | One test passed with the bug in place | 1 rewrite of a test |
| "Dead rows can never be removed" | The first report's words | The coordinator's own warning, that stale text could stay in `match_highlight`, was wrong: two tests and a table of cases found the exact condition, and ten planned edits were dropped | 3 probes |
| `quillo check ui/slug.qwe` | Natural | The false error of Q-325, avoided this time by the brief | 0 |
| Expected values worked out in the head | Speed | Again the largest single cause of failed runs, in most batches | Many short runs |
| Passing tests over real defects | Tests read the attribute, or a first row | NoteEditor's Done off-screen, as `tap()` scrolls to its target; TaskRow's strike past the words; `[hidden]` losing to `display: grid`. Screenshots found them | Each 1 run, once seen |
| Default radio names | Worked alone | Seen again from FEEDBACK25 | 0 |
| A second tab during a full run | Wanting screenshots | A hidden tab never answers a touch: one run hung for 3.5 hours | 3.5 hours |
| Speed from the last two pointer events | Simple | Noisy under load: a rewrite in three components. `Input.dispatchTouchEvent`'s `timestamp` fixes a test's speed exactly | 1 rewrite |
| A file not written | Its own running list said done | NoteEditor was missed until an `ls` failed | 1 |
| Ports, and `pkill` | Early probes | Two batches strayed, and one ran `pkill -P 0` against the brief, with no effect seen | none lasting |

## The second pass

The first ten batches finished on a harness that was then fixed six times, and before most of the traps above were
known. Each was sent back to rerun on the fixed harness and audit its own code against the new traps. **All ten had
passed every test, and the second pass found more than fifty real bugs in them**, in nine of every ten components in
one batch. Nearly all were on paths no test had reached:

- **At a real tablet width, for the first time:** SnapCarousel's last picture could not be reached, SheetSelect's panel
  stretched to 820 px, wheels and forms stretched across the screen, and a dozen layouts were stretched phones. The old
  harness's 450 px stage could not show any of it.
- **Q-334:** EdgeDrawer's Tab, SearchHeader's arrows, SwipeRow's keyboard, StoryCapture's camera.
- **Q-337:** BottomSheet, AdaptiveDialog, PeekSheet, SwipeDeck, PullToRefresh, WelcomeCarousel, ProfileScreen, SwipeReply,
  CommentSheet and CheckoutSheet jumped under the finger. A first test of SwipeDeck passed on the old code, and had to be
  rewritten to change what the attribute reads.
- **A throw that stops a handler (Q-318):** VideoCapture, VoiceRecorder and StoryCapture hung when the recorder was refused,
  TorchToggle stuck on "starting", QrScanner stopped, FilterSheet's Show and SnapCarousel's start stopped on blocked
  storage. `is_err(js_new(...))` can never run when the constructor throws.
- **A press landing on what replaced the button:** SelectionMode's long press pressed Delete on the toolbar that appeared,
  SpiritLevel's zero, LiveLocation's Reset, PayCode, StoryCapture's "Use this", all under a finger already down.
- **iOS paths that Chrome never takes:** PermissionChecklist's Allow on motion left every other Allow dead.
- **Q-339:** PinPad's lock-out, GestureTutorial, NotifyPrompt.
- **Components that could not be closed:** StoryViewer and CommentSheet with `open` held true and no `on_close`.

The batches marked plainly which fixes had a test shown failing on the old code and which did not; most had. The lesson for
the study is the one in the title of this section: a batch of passing tests was too kind to the components, and the test
harness, not the model's care, set the ceiling.

## The harness

`tests/browser.mjs` gained real touch, phone and tablet sizes, and automatic checks, and each of these came out of use:

| Found | By | Fixed |
|---|---|---|
| The callback id lost precision, so a touch never answered | a throwaway test, before launching | yes |
| A port in use tested the wrong program and printed `all 1 passed` | batch 17 | yes: `test.sh` stops, and a slug not in the gallery fails |
| Load errors were thrown away | batch 17 | yes, for phone and tablet tests |
| A tablet's stage was 450 px, so no wide layout ever ran | batches 11 to 20 | yes: `?only=` fills the screen |
| A pinch left the tab's touch dead for later tests | batches 13 and 15 | yes: each test has its own tab. The cause is not known |
| A plain `return` skipped the automatic checks | batch 21 | yes: the test runs in its own function |
| A hung test hung the run for ever | batch 29 | yes: 180 s a test, and page dialogs are answered |
| `Page.bringToFront`, added as a safeguard for a hidden tab, broke three existing tests that had always passed | the merged run, read first as timing | yes: removed. All three failed every time on the branch and passed on `main`, and a bisect found it. Why it breaks them is not known |

Still missing: an on-screen keyboard, Safari's engine, a real pen, a frozen page, a real `orientationchange`, a real offline
reload, a screenshot per page. Worth adding: a `flick` that sets `timestamp`, an `elementFromPoint` check in `tap`, and sizes
and overflow checked during a gesture, not only at the end. `tests/classes.mjs` checks whole classes and not prefixes:
`ui-mini-` clashed with MiniCart's, unseen.

## Limitations of the phone apps

All **read, not run**. None can be worked around from a program.

| Missing | Effect |
|---|---|
| Permission state | `quilloNative` cannot say whether location or notifications are allowed without asking, and cannot ask about notifications without sending one |
| Background audio | No `UIBackgroundModes` or `AVAudioSession` on iOS, no foreground service on Android, so sound should stop when the app leaves the screen. MiniPlayer, NowPlaying, PodcastQueue and SleepTimerSheet are in-app only |
| Status bar | A web view reads no `theme-color`, so StatusTint does nothing in an app. Pick dark, and the bar stays as the shell drew it, with Android's dark icons fixed |
| Biometrics | No `authenticate`, so ConfirmWithPin is a PIN alone |
| Hiding a screen | No `FLAG_SECURE` or cover, so a card number on screen goes into the app switcher |
| Sharing a file | Text only: a receipt or a QR code cannot reach the share sheet |
| Location watch | LiveLocation polls |
| Review sheet | Neither `SKStoreReviewController` nor Play's in-app review, which Apple's guideline 5.6.1 expects |
| Notifications | No way to cancel one, so a paused workout would still buzz. No icon badge |
| A held back button | A hard update cannot be fully hard on Android |
| First paint | iOS's web view stays opaque and white until it paints, and Android sets no background, so a dark app flashes white before a splash can help |
| System text size | No call, so Android's cannot be read |
| A plain page offline | A page reloaded offline needs a service worker, which a component cannot install |

Elsewhere: no `url_decode`, no way to make a map from pairs, a route cannot join parts into one file as there is no bytes
value, and a `request` read in a view is refused while a `server fn` that view calls gets the real request, which the guide does
not say.

## Phone findings, not Quillo's

These cost the most time, and would cost anyone the same.

- **`touch-action` stops at a scroller.** `pan-y` on a panel does not reach into an `overflow: auto` child, so a sideways drag
  gets `pointercancel` after a few moves. It belongs on both the root and the scroller. Found in three batches.
- **`pan-y` needs help.** A pointer event cannot keep the browser from scrolling. Only a cancelled `touchmove` can, and
  `@touchmove` in markup is not passive in Chrome, which the guide does not mention. The pointer handler decides, and the touch
  handler obeys.
- **A held finger presses what replaces its button.** Lifting it fires `pointerup` and `click` on the new element, with no
  `pointerdown` ever on it. In StoryCapture it pressed "Use this" unseen, the worst thing one batch found.
- **Acting on `pointerdown`, then going away,** sends the ending click to whatever took your place: a PIN's last digit pressed
  a key on the next pad.
- **A click after a long press or a captured drag** lands on what is there by then. Swallow clicks for a moment.
- **Capture only after the axis lock,** so a tap on a button inside a swiped row stays a click. Capture on the element with the
  handlers, or later moves pass them by.
- **`lostpointercapture` bubbles,** so moving capture from a button to its parent fires it on the parent at the start of a drag.
- **Touch adjustment** moves a press onto a nearby field, which then pans. A touch at x=12 beside a textarea at x=16 goes to
  the textarea.
- **Chrome's scroll anchoring** moves a list as it is laid out anew, and can hide a bug that only Safari will show.
- **`overscroll-behavior: contain` on an `overflow: hidden` box** stops the page scrolling through it.
- **`display: none` forgets `scrollTop`.** A class's `display` beats `[hidden]`. A container query cannot style the container.
- **Moves arrive unevenly,** 1 ms or 22 px apart. Sample speed over 32 ms or more, and from where the slop ends.
- **A modal `<dialog>` ignores a transformed frame,** as the top layer leaves it.

## The kit

Existing components, found in passing. None is fixed here unless it says so.

- **`MentionInput` offers nobody** once an emoji comes before the cursor (Q-344). Five more read `selectionStart`.
- **The `localStorage` components** forget in a phone app (Q-339): `announcement_bar`, `breaking_news`, `coachmark`,
  `spotlight_tour`, `autosave_form` and others.
- **36 rows have the `? :` shape of Q-342.** On 5f7f76c, `json_viewer`, `terminal_output` and `story_viewer` are the ones
  over data that can empty after a flip.
- **The README said** "use `field` for a label's `for`", which is a view's prop and not a `<label>`'s attribute; the gallery's
  own search label was tied to nothing that way. Both fixed on this branch.
- **Fixed on this branch,** found in the merge: `scan_count` made a `BarcodeDetector` with no guard against a throw;
  `action_sheet`, `quick_settings_grid`, `approval_queue`, `mini_player` and `log_explorer` compared elements with `==`;
  `approval_queue`, `bill_reminders` and `branching_survey` named a function after a builtin. 22 gallery warnings remain where a
  local name shadows a builtin and is called as the local on purpose, in eleven new files and three older ones.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `@pointer*` and `setPointerCapture` under real touch, on an element that is not the target, from `@pointermove` | As the guide says, first time, in every batch |
| `@pointercancel` from a real `touchCancel`, and from the browser taking a scroll | Works, and tells you a finger became a scroll |
| `el.style.transform` from a handler or a plain `fn` | Works, and costs no render |
| State written on every move with no reader | Costs nothing visible |
| Turning the viewport | State, focus and scroll kept |
| Rows keyed and moved while typing | Focus and text kept |
| `popstate` from the browser's real Back, after `history.pushState` through `js` | Works |
| `<style>{q}</style>` with `q` holding `</style><img src=x>` | `<\/style>`: the page cannot be broken out of |
| `@mount` that waits, then returns a clean-up | The clean-up runs |
| A real tap before the bundle has come | Replayed once, in Chrome |
| A text place written by hand, then a state change | The next change still shows |
| A promise inside `every` | Waited for, though the guide does not list it |
| Three `client` lines for one script | The program starts and its maps work |
| `Network.emulateNetworkConditions`, `Emulation.setSafeAreaInsetsOverride`, `Target.createTarget` to hide a page | All real, and useful in tests |
