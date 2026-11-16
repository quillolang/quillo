# Feedback from testing complex layout and re-rendering: a three-pane console, a one-page tracker with live data and two visitors, lists four deep, on three browser engines

| | |
|---|---|
| App | None kept in the repo. First pass: a throwaway "Ops console", 108 lines, a three-pane CSS grid with a drag resizer, a board of component cards, a table, tabs, a filter and a drawer, plus 89 small probe components. Second pass: a one-page project tracker on SQLite with `live()`, two visitors and drag and drop; lists four deep at 3,000 units; a recursive tree; a window on 20,000 rows; 30 cases of what happens across a wait; scoped CSS and layout at desktop and phone width |
| Quillo | `0.8.0 (c293883, 2026-11-07)`, the release build. `src/` is the same from c293883 to main's 5562c7a (`git diff` is empty) |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check`; headless Chrome 156 (`feedback/tools/`); Playwright 1.63's Chromium 153, WebKit 26.6 and Firefox 155; the installed Firefox 159 over WebDriver BiDi; one browser at a time; seeded walks of over 17,000 steps; real mouse, key, touch and drag events; a `MutationObserver` on 5,000 rows |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section. Numbers start at Q-889, after the last ID in the repo, Q-888. There is no FEEDBACK60.md in the repo, so check these IDs against it if it exists elsewhere. The programs and drivers are in `.git/wip/feedback61/` on this Mac, the second pass in `hard/`. Sections below are in ID order; the table in [The answer](#the-answer) gives the priority.

**This report was asked for while CI was red.** The rule of 2026-11-05 says no next FEEDBACK until CI is green; the user asked for this one directly. Main's head, 5562c7a, failed CI run 42384904061 on `example_gallery_tabs_switch_without_reloading`: `server did not start` at `tests/browser.rs:63`. That is a start-up timeout, not a wrong page, and it does not look related to this report. It was not investigated.

What worked well: the reactive core is very good. In over 16,000 random steps with single-root rows, across three engines, navigation, deep lists and two visitors, the page only ever disagreed with its state or its database through the bugs below, and the layout never broke. The guide's claims about how little a change touches are exact: a swap moves two rows, a removal moves none, and 5,000 new maps with the same values write nothing. The worst failures below are at the edges of that core: a row's record of its own nodes, the step that takes up the server's HTML, and the writing of a value back into a field.

## The answer

**Three bugs matter most.** A list row whose root element changes corrupts the list on its next move (Q-889). A `live` page that misses one load stays on old data until something else changes (Q-899). And on Firefox, a visitor who moves between pages at an ordinary pace can stop hearing live updates for 10 to 45 seconds or more (Q-900). None of the three shows the visitor anything.

The first pass compared the same app with two shapes of row:

| Rows in the console | Steps | Steps where the page disagreed with its state | Console errors |
|---|---|---|---|
| One root element per row, 6 seeds | 2,400 | **0** | 0 |
| Root is a branch (Q-889), 3 seeds | 360 | **63**, in all 3 seeds | 15 |

The 63 counts every step after a list first broke, so they are not 63 separate failures. The first breaks came at steps 27, 55 and 84, which is the better measure of how quickly it fails.

The second pass, with single-root rows so that Q-889 could not hide anything else:

| Test | Chromium 153 | WebKit 26.6 | Firefox 155 |
|---|---|---|---|
| Tracker, instant answers, checked against the database, 600 steps each | clean | clean | clean |
| Tracker, answers delayed up to 150 ms, 800 steps each | clean | clean | **Q-900**, 756 steps, then 800 more in replays |
| Four-deep lists, recursive tree, window on 20,000 rows, 3,150 steps each | clean | clean | clean |
| 30 cases across a wait | 26 pass; Q-901, Q-902 | the same | the same |
| Stale-data cases (write during a run, shared runs, remounts, stress) | clean | clean | clean |
| One failed load | **Q-899** | **Q-899** | **Q-899** |
| Scoped CSS, layout at desktop and phone width | clean | clean | clean |
| Update after 24 page changes in 8 s | 35 ms | 35 ms | **over 45 s** |

| ID | Problem | Kind | Browsers | Priority |
|---|---|---|---|---|
| Q-889 | A row whose root changes brings back old rows, loses real ones, or stops the update | silent | all | fix first |
| Q-899 | One failed `live` load leaves the page on old data until something else changes | silent | all | high |
| Q-900 | The server never answers a socket's close, and Firefox then hears live updates 10 to 45+ s late | silent, perf | Firefox | high |
| Q-890 | A function's markup erases what a person typed, ticked, picked or opened | silent | all | high |
| Q-891 | Rewriting a field as it is typed sends the caret to the end and scrambles the text | silent | all | high |
| Q-901 | After a wait, a handler has lost its component: `every` refuses, `run_after` outlives it | silent | all | medium |
| Q-902 | A save that finishes after the person has left moves them anyway | silent | all | medium |
| Q-892 | Table parts with no single root rebuild the page on load, erasing early typing | silent | all | medium |
| Q-893 | `src={...}` in a hydrated component loads twice | silent, perf | all | medium |
| Q-894 | A row of more than one node loses its scroll, and its iframe reloads, when it moves; on WebKit, every moved row | silent | all | medium |
| Q-895 | A `for` row's `key` that reads other state is ignored until the list changes | silent | all | low |
| Q-896 | Enter saves, then the removed input's blur saves again | silent | Chromium | low |
| Q-897 | `if` as a filter in a `for` block loses typing and focus on a reorder | docs | all | low |
| Q-898 | `quillo check` calls a class written by a class map unused | loud | n/a | low |

## The scenario

The console is one component tree: `Console` holds the state and passes it through `Column` to `Card` (a component with its own state and branches), and through `Rows` to the table. Each step of the walk does one thing a person would do: flip a card's blocked state, move it to the next column, delete it, select it, write a note, sort, filter, add, change tab, or drag the resizer. For speed, the steps use the page's own `click()` and `input` events; only the resizer uses real mouse events. A step with nothing to act on is skipped but counted, so the step counts are slightly high.

The page prints its state in a hidden `<pre>`, and after every step the driver compares the page with it:

- **Content:** the rows' order, ids and text, blocked marks, column counts, the selected card, the navigation's counts, the drawer.
- **Layout:** the five panes touch, the left pane is as wide as the state says, the drawer ends at the window's edge, nothing scrolls sideways, every card sits inside its column.
- **Row state:** a note written in a card stays with that card through sorts. Cards that change column, or leave the page through a filter or tab, are dropped from this check, since they start afresh by design.

The resizer was checked apart from the walk, so that a drag that never started could not pass: it followed the pointer to 320 and 140, and stopped at both limits (900 gave 400, 20 gave 100), with the columns reflowing from 324 to 251 and 351 pixels.

### The second pass

The first pass tested the hardest features one at a time, so the second put them together, and ran each test on three engines with one driver.

- **The tracker** is one page app: `App` holds pinned cards through navigation, and shows a home page, three projects and card pages, each loading with `server fn`s and following `live()`. Each step does one of: follow a link, two links at once, Back, Forward, reload, open a card, toggle, delete, add, write a note, pin, comment, drag a card to a column or before another card with the real mouse, write straight to the server as someone else would, or act as a second visitor in another browser context. After every step the page is compared with **the database**, and the walk stops at the first difference, so one bug cannot hide another. Answers ran instantly, then delayed at random by up to 150 ms.
- **Lists four deep** (region, site, rack, unit), with a branch at each level, components with state at the bottom, up to 3,000 units. Rows reorder at every level, branches open and close, and state is tracked: a note must stay with its unit through moves, and go when its branch goes.
- **A recursive tree** of 364 components whose keyed children reverse and drop, each with its own state.
- **A window on 20,000 rows**, driven by the real wheel, jumps and selection.
- **Across a wait:** each rule the guide gives for `server fn` calls became a case, with delays set so that answers come in the order the case needs.
- **Failure:** offline, one failed load, a real restart of the server, 320 round trips while watching for leaks.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-342 | Same cause as Q-889. Its fix covers only a list that empties (`clearRows`); a move or an insert still reads the stale record | 1 |
| Q-306 | Its promise fails when the input holds no state (Q-890). The test at `tests/browser/edges30.qwe:176` binds both fields to state, so it never reaches the case | 1 |
| Q-032 | Same family as Q-892: markup the HTML parser moves, so the browser makes the component again | 1 |
| Q-189 | A page that stays behind has to be seen: the console line it added fired in the Chromium burst of Q-900, but Q-899 and Firefox's wait say nothing to the visitor | 1 |

## New issues

### Q-889. A row whose root changes after it is made corrupts its list on the next move

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading the runtime, then browser | 1 |

```quillo
component List() {
    state xs = [{ id: 1, done: false }, { id: 2, done: false }, { id: 3, done: false }]
    <div>
        <ul>{for x in xs => x.done ? <li key={x.id}>done {x.id}</li> : <li key={x.id}>todo {x.id}</li>}</ul>
        <p id="n">{len(xs)} items</p>
        <button id="mark" onclick={() => { xs = xs | map((i) => i.id == 2 ? merge(i, { done: true }) : i) }}>mark</button>
        <button id="rev" onclick={() => { xs = reverse(xs) }}>rev</button>
        <button id="ins" onclick={() => { xs = [xs[0], { id: 9, done: false }] + slice(xs, 1) }}>ins</button>
    </div>
}
route GET / = <html><body><List/></body></html>
```

| Steps | Should show | Shows | Error |
|---|---|---|---|
| mark, rev | todo 3, done 2, todo 1 | todo 3, **todo 2**, done 2, todo 1, under "3 items" | none |
| mark, ins | todo 1, todo 9, done 2, todo 3 | todo 1, done 2, todo 3, under "4 items": **the new row is missing** | console: `insertBefore` |
| A `live()` list: the server marks a row, then reverses | 3 rows | todo 3, **todo 2**, done 2, todo 1, with no click on the page | none |
| A component row in a table, root `t.done ? <tr/> : <tr/>`: mark, rev | todo 3, done 2, todo 1 | todo 3, **todo 2**, todo 1, done 2: ghost **and** wrong order | none |

Rows that are lists of their own fail worse, with `view Group(g) = {for x in g.xs => <li key={x}>{x}</li>}` as the row:

| Steps | Should show | Shows | Error |
|---|---|---|---|
| Groups [a, b] and [c]: empty the first, reverse | c | **nothing: `c` is gone**, and does not come back when the list changes again | console: `insertBefore` |
| Groups [a, b], [c], [d]: empty the first, reverse | d, c | **d**: `c` is gone | console: `insertBefore` |
| Groups [a, b], [c], [d, e]: grow the second, reverse, empty the first, reverse | c, c2, c3, d, e | **a, b**, c, c2, c3, d, e: deleted rows are back | none |

**The ghost is alive.** It keeps the old text, and clicking it runs the old branch's handler: a test with `hits += 1` on the todo branch and `hits += 100` on the done branch counted 103 for one click on each of the four rows.

Measured the same on Chrome 156, Chromium 153, WebKit 26.6, Firefox 155 and the installed Firefox 159, apart from the error's wording.

Every row whose root can change is affected: `? :` and `if`/`else` rows, a view or a plain function with a branch at its root, a view giving a list, a `for` as the root, and a table-row component. Single-element rows are not, and neither is a component in a list, whose island is the one stable element. Both the server's page and the browser's are affected; there is no hydration mismatch, since the first render is right.

**Why:** `newRow` (`src/client/runtime.js:7421`) copies the nodes a row was made with into `r.nodes`, once. A branch at the row's root keeps its own current nodes and replaces them later, but the copy still names the first element next to the branch's marker. `nodesOf` (`:6312`) walks both, so a moved row carries the old element back in. `firstOf` (`:7436`) returns the old element as the row's start, and `reconcile` (`:7479`) then uses that detached node as the place to insert before, which throws. The throw leaves `st.rows` out of step with the page, and moved rows left in a fragment are lost, so later updates build on a wrong record.

| Impact | |
|---|---|
| **Functionality** | Lists show rows that are gone, lose rows that exist, and can stop matching their state until the page reloads |
| **User** | Duplicates, missing rows and counts that disagree with the list, in the common "a finished item looks different, and the list sorts" pattern; nothing tells them why |
| **Requests** | None by itself. A control on a ghost still works, so it can send a request the page no longer offers (from reading; the handler running was measured) |
| **Security** | A ghost shows what the program replaced. If a `live` update took a row's details away, a reorder puts them back on that person's page. It reaches only what that page already held (from reading; ghosts under `live` were measured) |

**Fix:** a row should not hold a copy of what a part owns: keep the marker and let the part answer for its nodes, so `nodesOf` and `firstOf` always read the current ones. This suggestion was not built or tested. Keep the seeded walk as the test, since it found all of these shapes.

**Until then:** one stable element per row, with the branch inside it: `<li key={x.id}>{x.done ? "done" : "todo"} {x.id}</li>`. The console's flat table has that shape and had no failures.

### Q-890. A function's markup erases what a person typed, ticked, picked or opened

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 2 |

`USERGUIDE.md:1887` says that when a function's markup keeps its shape, "an input in it keeps its focus and what was typed". The focus stays and the text does not.

```quillo
fn form(n) = <div><input id="f"/><span>{n}</span></div>
component Fn() {
    state n = 0
    <div>{form(n)}<button id="inc" onclick={() => { n += 1 }}>inc</button></div>
}
route GET / = <html><body><Fn/></body></html>
```

Type "hello", press inc: the same element, still focused, now empty, on all the browsers tried, the installed Firefox 159 included. All five shapes tried gave the same result: an attribute, text, nested text, a handler, or a checkbox beside the input. In one function's markup:

| Field | Before an unrelated state changed | After |
|---|---|---|
| `<input type="checkbox">` | ticked | **not ticked** |
| `<details>` | open | **closed** |
| `<select>` | "b" | **"a"** |
| `<textarea>` | "note" | **empty** |

**Why:** `morph` (`src/client/runtime.js:6402`) copies `value`, `checked` and `selected` from the freshly made element, whose values are the defaults, and removes attributes the new one lacks, which closes a `<details>`. It cannot tell "the markup said this" from "nobody said anything".

| Impact | |
|---|---|
| **Functionality** | A field with no state in a function's markup is cleared by any change that calls the function again |
| **User** | Loses what they typed while the cursor stays in place, so it looks as if the text was never there |
| **Requests** | No change, but a form sent afterwards carries the emptied values |
| **Security** | No change |

**Fix:** carry a field's value over only when the new markup set one (`__want` already records this for `value`), and leave `open` alone unless it was written. Add a test with a field that has no state.

### Q-891. Rewriting a field as it is typed sends the caret to the end, and scrambles the text

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 2 |

```quillo
component Caret() {
    state code = "AB"
    state digits = "123"
    <div>
        <input id="code" value={code} oninput={(e) => { code = upper(e.value) }}/>
        <input id="digits" value={digits} oninput={(e) => { digits = re.replace(e.value, "[^0-9]", "") }}/>
    </div>
}
route GET / = <html><body><Caret/></body></html>
```

| Field | A person does | Should end as | Ends as |
|---|---|---|---|
| `code` | puts the caret after A, types x y z | `AXYZB` | **`AXBYZ`**, caret at the end |
| `digits` | puts the caret after 1, types z, then 9 | `1923` | **`1239`**: the refused z moved the caret, so the 9 went to the end |
| a plain `value={plain}` | puts the caret after a, types x y z | `axyzb` | `axyzb` (right) |

`AXBYZ` was measured on Chrome 156, Chromium 153, WebKit 26.6 and Firefox 155. Typing at the end of the field hides this, since the caret belongs there, so it is easy to miss. It appears when someone corrects the middle of a value.

**Why:** `setProp` writes the new text with `el.value = nv` (`src/client/runtime.js:7118`), and a browser puts the caret at the end whenever a field's value is set to something else. Q-766's `focusBack` restores the caret after a move, but nothing does after a write.

| Impact | |
|---|---|
| **Functionality** | Any field its handler rewrites (upper case, digits only, phone or card formats) takes text in the wrong order when edited in the middle |
| **User** | Their correction is scrambled, and they have to retype it; in an account or card number the wrong order can go unnoticed |
| **Requests** | No change, but a wrong value can be sent |
| **Security** | No change |

**Fix:** when the field has the focus, keep the caret's distance from the end of the text across the write. That handles upper-casing and a refused character alike. A format that inserts characters may want more; check that case when building it.

### Q-892. A table part with no single root rebuilds its page on load, and erases early typing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser, the `mismatches` hook | 2 |

The guide (`USERGUIDE.md:1213`) says a component whose root is a table's part has no island. That holds for one root element only. In any other case the server writes `<quillo-island>` inside `<tbody>`, the HTML parser moves it out of the table, and the browser cannot take up the page.

```quillo
component Rows(xs: list) { {for x in xs => <tr key={x}><td>{x}</td></tr>} }
component Page() { <div><input id="early"/><table><tbody><Rows xs={["a", "b"]}/></tbody></table></div> }
route GET / = <html><body><Page/></body></html>
```

| The component in `<tbody>` or `<tr>` | Page rebuilt on load | Typed before the bundle ran |
|---|---|---|
| `for` with 1 row | no | kept |
| `for` with **0** rows | **yes** | **erased** |
| `for` with **2** rows | **yes** | **erased** |
| `on ? <tr/> : null`, hidden | **yes** | **erased** |
| `if on { <tr/> }`, hidden | **yes** | not measured |
| `[<tr/>, <tr/>]` | **yes** | not measured |
| `on ? <td/> : null`, hidden | **yes** | not measured |

The parser moves the island out in Chromium, WebKit and Firefox alike; the rebuild itself was measured in Chrome. So the same program hydrates or not depending on how many rows the data has. The console says nothing; only the `mismatches` hook reports it. The rows still reach the table, so nothing looks wrong. A plain `<img>` in the rebuilt component was requested twice. In Chrome 156 a `for` of options in a `<select>` caused no rebuild, with 0 or 2 rows; other browsers were not tried.

| Impact | |
|---|---|
| **Functionality** | The page works after the rebuild, but hydration is lost, and whether that happens depends on the data |
| **User** | What they typed or changed before the bundle ran is erased, on a slow connection most of all |
| **Requests** | Images in the rebuilt component are requested again (measured with an uncached image) |
| **Security** | No change |

**Fix:** mark these components with comment nodes where an element cannot stand (in `tbody`, `tr`, `thead`, `tfoot`, `table`), since a comment survives the parser there. Until then, the guide should say that the rule holds for one root element, and `quillo check` could warn.

### Q-893. `src={...}` in a hydrated component loads twice

| Kind | Detected by | Attempts |
|---|---|---|
| silent, perf | the server's request log | 2 |

```quillo
component Embed() {
    state n = 0
    let page = "/inner/let"
    <div>
        <iframe src="/inner/lit"></iframe>
        <iframe src={page}></iframe>
        <iframe src={`/inner/tpl${n}`}></iframe>
        <button onclick={() => { n += 1 }}>{n}</button>
    </div>
}
```

| What | Requests on load |
|---|---|
| `src="..."` written as text | 1 |
| `<iframe src={...}>`, from a `let`, a state or a template | **2** |
| `<img src={...}>`, alone or in a row | **2** |
| `<video src={...}>` | **2** |
| `src={...}` in a component with no state, which is never taken up | 1 |
| `src={...}` made in the browser, in a branch | 1 |

Changing the value later costs one request, as it should. Iframes loaded twice on Chromium, WebKit and Firefox. Images did on Chromium and Firefox, and once on WebKit. `<video>` was measured in Chrome only.

**Why:** the last line of `setProp` (`src/client/runtime.js:7180`) writes when `old !== v`. `old` is `undefined` while taking up the server's HTML, so the same `src` is written again, which starts an iframe or a player over.

| Impact | |
|---|---|
| **Functionality** | Right in the end, but every embedded page starts twice |
| **User** | An embed can flash or restart; a video starts loading again |
| **Requests** | Two where one is enough, for every iframe, player and uncached image with a `src={...}`, on every load. The images measured had no cache headers; a cached one may not reach the network |
| **Security** | A one-time link in a frame's `src` is used up by the first load, so the second fails, and a GET with side effects runs twice (from reading, not measured) |

**Fix:** while claiming, do not write an attribute that already says the same, or start `old` from the page's value.

### Q-894. A row of more than one node loses its scroll, and its iframe reloads, when it moves

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

The guide says moved rows keep their DOM. Single-element rows move with `Element.moveBefore`, which keeps everything. A row of several nodes is taken out and put back.

| Row, after a reverse | Scroll position, set to 77 | An iframe in it |
|---|---|---|
| `<div key={x}>...</div>` | kept: 3:77, 2:77, 1:77 | not reloaded |
| `x > 0 ? <div key={x}>...</div> : <div key={x}>...</div>` | **lost: 3:0, 2:0**, 1:77 | **reloaded**: 3 loads became 5, for the two rows that moved |
| A view giving `[<dt/>, <dd/>]` | **lost** (55 became 0) | not measured |

The row that stays put keeps its scroll, so only moved rows show it. A branch-rooted row always counts as several nodes, because its marker is a node, even though the person sees one element.

**On WebKit it is every moved row.** It has no `moveBefore`, so single-element rows lose the same things:

| Single-element rows, after a reverse | Chromium 153 | Firefox 159 | WebKit 26.6 |
|---|---|---|---|
| Scroll, set to 77 | 3:77, 2:77, 1:77 | 3:77, 2:77, 1:77 | **3:0, 2:0**, 1:77 |
| Loads of 3 iframes | 3 | not measured (Firefox 155: 3) | **5** |

Playwright's Firefox 155 lost one of the two moved rows' scroll; the installed Firefox 159 kept both in three runs of three, so that was the older build.

**Why:** `reconcile` (`src/client/runtime.js:7542`) uses `moveNode` only when `ns.length === 1`; otherwise it goes through a fragment, which removes the nodes from the page. Focus is put back by `focusBack`; scroll and frames are not. Pointer capture (Q-343) should be lost the same way (from reading, not measured).

| Impact | |
|---|---|
| **Functionality** | The guide's "moved rows keep their DOM" fails for these rows |
| **User** | A scrolled panel inside a row jumps to the top, and an embedded page starts over, when the list sorts |
| **Requests** | One more load per moved row with an iframe (measured) |
| **Security** | No change |

**Fix:** call `moveNode` on each node in turn, in place of the fragment. Q-889's fix should come first, since it decides what a row's nodes are. For WebKit, put back each moved element's scroll, as `focusBack` does for the focus; a frame cannot be saved there.

### Q-895. A `for` row's `key` that reads other state is ignored until the list changes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading the runtime, then browser | 1 |

```quillo
component Ver() {
    state ver = 1
    state xs = [1, 2]
    <div>
        <ul>{for x in xs => <li key={`${ver}:${x}`}><input/>{x}</li>}</ul>
        <div key={ver}><input/></div>
        <button id="ver" onclick={() => { ver += 1 }}>new version</button>
        <button id="touch" onclick={() => { xs = xs + [] }}>touch</button>
    </div>
}
```

On all three engines, with all three inputs holding "old", pressing "new version" remakes the single element outside the loop, and leaves the rows: `["old", "old", ""]`. Pressing "touch", which has nothing to do with versions, then remakes the rows: `["", "", ""]`. So the reset happens at the wrong time, on some later change to the list.

The guide's table (`USERGUIDE.md:2853`) promises a remake when `k` changes for a `key` "outside a loop", and does not say what a loop's key does.

**Why:** `each` (`src/client/runtime.js:7347`) reads only the list as what it follows, and works out keys inside `untrack`.

| Impact | |
|---|---|
| **Functionality** | "Start every row over when this changes" does nothing at first, then happens later |
| **User** | Sees old rows after a reset, then loses what they typed at an unrelated moment |
| **Requests** | No change |
| **Security** | No change, unless a reset is relied on to clear a form between records |

**Fix:** work out keys inside the tracked part of the effect, so that a key follows what it reads, as it does outside a loop. If that is not wanted, `quillo check` should warn and the guide should say so.

### Q-896. Enter saves, then the removed input's blur saves again

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 2 |

```quillo
component Edit(save) {
    state editing = true
    {editing
        ? <input onkeydown={(e) => { if e.key == "Enter" { save("enter"); editing = false } }} onblur={() => { save("blur"); editing = false }}/>
        : <span>saved</span>}
}
```

On Chrome 156 and Chromium 153, pressing Enter calls `save` twice: the log reads `enter,blur`. They fire `blur` when a focused element is removed, and Quillo's listener is still on it as the branch goes. **WebKit and Firefox do not**: their log reads `enter`. So a program behaves differently by browser, in the usual shape of an inline edit, where both Enter and leaving the field save.

| Impact | |
|---|---|
| **Functionality** | The handler of a branch that is already going runs after the program has moved on |
| **User** | Usually nothing to see; a toast or an entry in a log appears twice |
| **Requests** | Two saves, when `save` calls a `server fn` (from reading; two calls of `save` were measured). Not idempotent unless the program makes it so |
| **Security** | No change |

**Fix:** a decision for the user: either stop an element's listeners before Quillo removes it, or leave the browser's behaviour and say so in the guide. Stopping them seems better: a program cannot want a handler from a branch it has just closed, and all browsers would then agree.

### Q-897. `if` as a filter in a `for` block loses typing and focus on a reorder

| Kind | Detected by | Attempts |
|---|---|---|
| docs | browser | 2 |

```quillo
<ul id="a">{for x in xs { if x.on { <li key={x.id}><input/>{x.id}</li> } }}</ul>
<ul id="b">{for x in xs | filter((i) => i.on) => <li key={x.id}><input/>{x.id}</li>}</ul>
```

Type in the first row of each, and reverse the list:

| Form | Rows after | Focus |
|---|---|---|
| `a`: `if` in the block | `2=`, `1=`: **typing lost** | **lost** (body) |
| `b`: `filter` | `2=`, `1=t`: kept | kept |

The same on all three engines.

`x.on ? <li key={x.id}/> : null` behaves as `a` does. This is what the guide says: with a branch that has no `key`, "the rows are kept by place" (`USERGUIDE.md:1267`). The `key` still written on the `<li>` then remakes the element when the place's item changes. So it is not a broken promise, but the row looks keyed, and `quillo check` says nothing. It does hold while items only change or hide: typing survived both.

| Impact | |
|---|---|
| **Functionality** | Every row is made again on a reorder, although a key is written |
| **User** | Loses typing and focus when the list sorts |
| **Requests** | No change |
| **Security** | No change |

**Fix:** `quillo check` could warn on a `key` inside such a row, and suggest `for x in xs | filter(...)`. Or an `if` with no `else` could count as keyed by its one branch, as a row that shows nothing needs no key.

### Q-898. `quillo check` calls a class written by a class map unused

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 2 |

```quillo
component Tab(active: bool) {
    style css`
      .tab { padding: 4px }
      .active { color: blue }
    `
    <p class={{ tab: true, active: active }}>x</p>
}
```

```
cls.qwe:3: warning: `.tab` is styled here, but nothing in the program writes it
cls.qwe:4: warning: `.active` is styled here, but nothing in the program writes it
```

Both classes are written by the map, which is the form the guide teaches (`USERGUIDE.md:1439`). The check (`src/check.rs`, the warning at line 7537) collects the words of the program's strings and templates, and a map's keys are neither. That also makes the warning come and go: in the console, `.card` was not flagged, apparently because a plain string said `"pick a card"`.

| Impact | |
|---|---|
| **Functionality** | No change to the program; the check gives a false warning |
| **User** | No change for visitors. A developer, or a model acting on the warning, may delete a rule that is in use |
| **Requests** | No change |
| **Security** | No change |

**Fix:** count the keys of a map written in `class={...}` as written. `written_classes` (`src/styles.rs:161`), which lists classes for a CSS engine, may need the same: its comment does not name map keys.

### Q-899. One failed `live` load leaves the page on old data until something else changes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | offline test, reading the runtime, then browser | 3 |

```quillo
db.open("stale.db")
db.exec("create table if not exists t (id integer primary key, v int not null)")
server fn load_v(id: int) -> list = db.query("select v from t where id = ?", id)
component V(id: int) {
    state r = live(load_v, id)
    <p>{r == null ? "none" : r[0].v}</p>
}
```

The socket stays up; only the call that fetches the new value fails, once. The test does that with Playwright's `route`, and waits 4 seconds:

| What went wrong | The page shows | The database has |
|---|---|---|
| One load fails to connect | 35 | 36 |
| One load is answered 503 | 37 | 38 |
| A second change comes while a load that then fails is on its way | 39 | 41 |

The result was the same on Chromium, WebKit and Firefox, and nothing asks again. The next change to the table brings the page up to date (measured). If there is no next change, it stays wrong until a reload.

That also fits the offline test. After going offline and back, Chromium caught up in 106 ms, while WebKit and Firefox had not in 30 seconds, though a later event brought them up to date. Most likely their offline switch left the socket open, so the wake arrived and its load failed; that part is inferred.

**Why:** in `liveRefresh`, `if (failed) { report(value.error); return; }` (`src/client/runtime.js:5860`) gives up. It also leaves `sub.wanted` unread, so a change heard during the load is forgotten. The guide does say "the next change tries again" (`USERGUIDE.md:2367`), so the first two rows behave as written. It is the rule that is wrong: it counts on a change that may never come. The guide promises catch-up only for a page that was disconnected, and here the page never was.

| Impact | |
|---|---|
| **Functionality** | A live page can show old data for as long as its table stays quiet, after one failed call |
| **User** | Sees an old price, status or count with nothing to say so; only the console knows. The quieter the table, the longer it lasts |
| **Requests** | None today. A retry adds a few, bounded by its backoff |
| **Security** | A value that was changed or withdrawn stays on that page until something else changes (from reading) |

**Fix:** keep the change as wanted and try again with a growing wait (1 s, 2 s, up to 30 s, as `liveLoop` does) until a load succeeds. `waiting()` could stay true meanwhile, so a page can say it is behind. The guide's line should change with it.

### Q-900. The server never answers a socket's close, and Firefox then hears live updates 10 to 45+ seconds late

| Kind | Detected by | Attempts |
|---|---|---|
| silent, perf | seeded walk, a timed log of every socket, then a plain client | 5 |

Measured on the tracker, whose home page and project pages follow different topics. After a run of page changes, the test writes to the server and times how long the page takes to show it:

| Page changes before the write | Chromium 153 | WebKit 26.6 | Firefox 155 | **Firefox 159, as installed** |
|---|---|---|---|---|
| 6, at 3 a second | 35 ms | 37 ms | 36 ms | 38 ms |
| 12 | 34 ms | 34 ms | 35 ms | 65 ms |
| 24 | 35 ms | 35 ms | **over 45 s** | **over 45 s** |
| 24 more | 32 ms | 35 ms | **11.8 s** | **11.8 s** |
| 24 more | 33 ms | 33 ms | 35 ms | 37 ms |

**Speed is not what sets it off.** At one page change a second, on the installed Firefox 159, four rounds of 24 gave 39 ms, **over 45 s**, 38 ms and **over 45 s**. So this is ordinary browsing, and about half the rounds at either speed.

The walk found it at its own pace: Firefox's seed 5 stalled at about the same point, steps 164 to 169, in four runs of four, with updates 19.8, 25.9 and 33.4 seconds late in the three that were timed. Chromium and WebKit did not, on the same seed. The page shows old data throughout, and the update does arrive in the end. Nothing is logged but the browser's own socket errors.

A timed log of every socket on that page shows what happens:

```
  7691  socket 24 created
 12903  19 older sockets finally close, within 1.5 s of each other, all with code 1006
 33563  socket 24 opens, 25.9 s after it was made, and delivers the change that was waiting
```

The server's part needs no browser:

```js
const ws = new WebSocket("ws://127.0.0.1:4631/_quillo/live?since=0&s=&t=db%3Acards");
// once open:
ws.close(1000);   // onclose: 4702, 3999, 3299, 2602 ms; code 1006, wasClean false, every time
```

**Why:**

1. Each page that follows other topics closes the live socket and opens a new one (`liveRestart`, `src/client/runtime.js:5745`).
2. The server notices a close only when it next looks, every 5 seconds (`LIVE_LOOK`, `src/server.rs:2042`; `gone`, `src/ws.rs:153`), and then drops the connection without answering. So every close ends as 1006, which a browser treats as a failed connection, and each one holds a thread and a waiter's place meanwhile.
3. Firefox then holds back the next connection. This step is inferred from the log and from what is known of Firefox's delay after failed sockets; it was not confirmed in Firefox's own code.
4. `liveSocket` (`:5798`) sets no limit on how long a socket may stay connecting, so neither the plain-request fallback nor a retry begins. The page is blind until Firefox lets the socket through.

Half-closed sockets count against `MAX_WAITERS_PER_CLIENT`, 256 (`src/server.rs:1905`). About 240 page changes in a few seconds most likely reached it on Chromium: the console said `live updates are not reaching this page`, and an update took 4.2 s. No person does that alone, but everyone behind one address shares the same 256 (from reading).

| Impact | |
|---|---|
| **Functionality** | Live data stops arriving on Firefox after page changes, for 10 to 45 seconds or more, then catches up |
| **User** | A Firefox visitor who moves around sees old data, with nothing to say so |
| **Requests** | A new socket for each page change, each holding a thread for up to 5 s after it is closed |
| **Security** | No new exposure. Threads held by closed sockets are bounded by the per-client limit |

**Fix,** in order of what each buys:

1. Answer a close at once, by reading the socket in place of looking every 5 seconds. This should remove the abnormal closes, and with them Firefox's delay. It also frees the thread.
2. Send a new set of topics down the open socket, which already takes topics as its first message when they are too long for the address. Page changes then open no sockets at all.
3. Give connecting a time limit, about 3 seconds, then fall back to the plain request.

### Q-901. After a wait, a handler has lost its component: `every` refuses, `run_after` outlives it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser, reading the runtime | 2 |

```quillo
server fn slow(ms: int, v: str) -> str { sleep(ms); v }
component Alive(on_tick) {
    <div>
        <button id="ev" onclick={() => { slow(30, "x"); every(20, () => { on_tick() }) }}>e</button>
        <button id="ra" onclick={() => { slow(30, "x"); run_after(150, () => { on_tick() }) }}>r</button>
    </div>
}
```

| In a handler | The guide says | Measured, the same on all three engines |
|---|---|---|
| `every` after a wait, component on the page | "lasts as long as its component" (`USERGUIDE.md:2910`) | **never runs**: the console says ``quillo: `every` works in a component`` |
| `run_after` after a wait, component leaves before it fires | calls `f` "unless its component has gone" (`USERGUIDE.md:1514`) | **fires** |
| `run_after` with no wait, component leaves first | the same | does not fire (right) |

A handler that starts a job and then polls it is the natural use, and it silently does nothing: the message names something the code is already doing.

**Why:** `Acting`, which names the handler's component, is set around the handler's first stretch and put back when it first waits (`src/client/runtime.js:7213`). What follows the wait runs with neither `Owner` nor `Acting`. `every` then fails (`:1779`), and `run_after` starts a timer that nothing owns (`:1745`), so nothing cancels it.

| Impact | |
|---|---|
| **Functionality** | Timers started after a `server fn` call fail or outlive their component, against the guide |
| **User** | Polling that should run does not, so a progress display stops; a late timer can act on a page they have left |
| **Requests** | Polling that never starts sends nothing; a stray `run_after` can send what it should not |
| **Security** | No change |

**Fix:** carry the component across the wait, as the handler's run already is for "the latest call wins". `listen`, which checks `Owner` alone (`:1705`), should be looked at with it.

### Q-902. A save that finishes after the person has left moves them anyway

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

```quillo
server fn slow(ms: int, v: str) -> str { sleep(ms); v }
component Saver() {
    <button id="save" onclick={() => { slow(200, "x"); navigate("/done") }}>save</button>
}
```

Press save, then follow a link elsewhere within the 200 ms. The person arrives at `/other`, and is then taken to `/done`, on all three engines. Nothing in the guide says either way. The rest of the runtime takes care of the same moment: a late answer does not write to a component that has gone (see the table at the end of this report).

| Impact | |
|---|---|
| **Functionality** | A component that has left the page still decides where the person goes |
| **User** | Pulled off the page they chose, and what they were doing there goes with it |
| **Requests** | One more page's data is loaded |
| **Security** | No change |

**Fix:** a decision for the user. `navigate` from a run whose component has gone could do nothing, which Q-901's fix makes easy to tell. The case against: a sign-out that must always end at `/login`. If those are rare, a way to ask for it would cover them; otherwise the guide should say that a `navigate` after a wait can come late.

## How much a change touches

5,000 rows, with a `MutationObserver` on the `<tbody>`. All the guide's claims hold. Times are left out: they were not a benchmark.

| Change | Page writes |
|---|---|
| Select a row, from none | 1 attribute |
| Select another row | 3 attributes, nothing else |
| One row's text | 1 text |
| Every row a new map with the same values | **0** |
| Swap two rows | 2 moves (4 entries) |
| Remove a row | 1 removal, no moves |
| Add a row in front | 1 insert |
| Reverse | 4,999 moves, the fewest possible |
| Clear | 5,002 entries, in one step |
| Fill with 5,000 | 5,000 inserts |

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `<>...</>` in a view | JSX habit | a clear error, which says to use a list | 1 check |
| `for x, i in xs` | other languages | `expected in`; the guide has `enumerate` | 1 check |
| `upper_case(...)` | guessed the name | unknown name | 1 check |
| `state upper` | a natural name | `check` warned that it hides the builtin; I ignored it, and the handler failed with `cannot call str` | 1 run |
| `min(a, b)` | other languages | a clear error: takes one list | 1 check |
| `<style>body { margin: 0 }</style>` in a route | HTML habit | `expected }, found :`, since `{` starts an expression | 1 check |
| `const [x, y] = page.center(...)` | my own driver | it gives `{ x, y }`, so the batch stopped | 1 run |
| Thought `if` in a `for` block lost typing on any change | one result in a mixed test | wrong: only on a reorder, and as documented (Q-897) | 1 run |
| Thought the double load had gone | a repro with no state, which is never taken up | my repro was wrong; it holds for hydrated components (Q-893) | 1 run |
| Expected deleted rows to return in the minimal repro of Q-889 | an earlier run's result | a different sequence lost a real row, which is worse than what I was looking for | 1 run |
| Stepped Back on the walk's first move | my own driver | left the app for `about:blank`, and my diff printer crashed | 1 run |
| The second visitor clicked a card a slow update was removing | my own driver | a 30 s wait that looked like a hang | 1 run |
| Counted a browser's errors on leaving a page as failures | my own filter | stopped three walks; the only real one had a different cause | 2 runs |
| `.stick` styled in a parent, on a child's element | forgot scoping | scoping was right; I took it for a layout bug | 1 run |
| No `<meta viewport>` in the phone test | forgot it | the phone layout never responded | 1 run |
| `data = slow(...)` in a statement | habit | `check` refuses it, with a clear message | 1 check |
| Took `last = hit(100)` showing 1 for a lost answer | assumed the server keeps order | it finished two equal sleeps last-first; the tagged call showed the right one won | 2 runs |
| Thought an answer's `seq` was taken after its read | a reading of the client alone | wrong: the server takes it first | 1 read |
| Thought a call joining a run in flight would lose a write | a reading of `live_turn` | wrong: `unchanged_since` guards it, and no round in four tests ended stale | 1 run |

## Limitations

- **Real Safari was not run.** It needs "Allow remote automation" in Safari's Developer settings, which is the user's to turn on. WebKit 26.6 stands in for it, and shares its engine, but not all of its behaviour.
- **Walks ran on Playwright's builds.** The installed Firefox 159 was used for Q-889, Q-890, Q-894 and Q-900 only. Edge was not run: it is Chromium.
- **The fixes are suggestions.** None was built or tested.
- **Phones were emulated:** a 390-pixel viewport with touch, on Chromium and WebKit, and the width alone on Firefox. No real device, and no real media files: request counts were read from the server's log.
- **Offline** is each engine's own switch, and they differ, so the offline row in Q-899 compares unlike things. The failed load in that section does not depend on it.
- **Not covered:** `--split` and services, several servers behind limer, tenants, translated pages, a real slow network, and memory on a phone.

Small things seen on the way, too small for an ID:

- Reloading or leaving a page with a call in flight logs ``server fn `x` could not be reached`` on Firefox and WebKit. Nothing shows on the page, but an error tracker would count it.
- Past 100 page changes in 10 seconds, WebKit refuses `history.pushState` with a `SecurityError` that reaches the page uncaught. Only a burst does that.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| 2,400 random steps through the console, single-root rows, 6 seeds | no disagreement with state, no layout failure, no console error |
| The three-pane grid while dragging the resizer | follows the pointer, stops at its limits, columns reflow |
| `{a && <i/>}{b && <i/>}{for ...}<i/>`, toggled in every order | always in written order |
| A parent clears what a child reads (`sel = null`, `m = {}`) | the child goes first; no read of a gone value |
| Duplicate keys | each row keeps its own input through a reverse |
| Keys `1` and `"1"` in one list | kept apart |
| The row's position from `enumerate`, after a drop and a reverse | text and handler both current |
| One `let` of markup shown in two places, `children` shown twice | both live; the second returns after a toggle |
| A field that refuses a character, or clamps a number (at the end of the text) | the page shows the state: 99 and 999 stay 10 |
| A place that fails, then is right again | recovers, and the rest of the page goes on |
| Style map losing a key, class map, `hidden`, `title` and `data-x` going to null | removed and restored |
| `<select>`'s option removed, then restored | the state's value is shown again |
| `<details>` opened by a person, then closed by state | closes |
| SVG made in the browser: a branch, a list, a component root | right namespace, and it measures |
| A diamond of `let`s, two assignments in one handler | one run, consistent values; assigning the same value runs nothing |
| A row reading state through a function, a `let` function, a pipe, a derived `let`, a block's `let`, an attribute, or an outer row's item | all follow |
| `match` with names, a guard, an input and a component in the branch | names stay live; the typed text and the child's state stay |
| `onmount` on the server's page, a branch, a row, a nested component, a keyed part | always on the page, with its real size |
| Text beside text with `""`, `null`, `0`, `false`, then changed both ways | takes up the server's text and updates right |
| `for` over `null`, then a list, then `null` | empty, rows, empty; no error |
| A tree of 400 components deep, opened one level at a time; closed at the top; reopened | right each time; nested state starts afresh, as documented |
| `every` in a row's branch | stops when the branch goes |
| `style css` in a component first shown in the browser | applied |
| A component with a branch at its root, in a `<ul>` | safe: its island is the stable element |
| **Second pass, on Chromium, WebKit and Firefox unless it says so** | |
| Tracker: links, two links at once, Back, Forward, reload, deep links, comments, pins, two visitors, writes straight to the server | matched the database after every step, but for Q-900 |
| Real drag and drop, to a column and before a card | 274 of 274 moved the card |
| Typing in a card while `live` renames and toggles it | text and focus kept, and typing carries on |
| Lists four deep, up to 3,000 units, 1,650 steps per engine | order, text, branches and each unit's state all right |
| A recursive tree whose keyed children reverse and drop, 1,200 steps per engine | each node keeps its state; a closed branch starts afresh |
| A window on 20,000 rows: wheel, jumps, both ends, selection, 300 steps per engine | the right rows, and the selection survives leaving and returning |
| The latest call wins: answers reversed, in order, equal delays, with a tagged call | the last call's answer shows, every time |
| Folds across a wait: `xs = xs + f()`, `n += f()`, `busy += 1` around a wait, an answer through a `let` | all land; the counter ends at 0 |
| Two like calls at once | both run on the server; no merging |
| `live` following a prop, answers reversed; `waiting` | shows the last id; true while waiting, then false |
| A component, a keyed part or a row leaves while its call waits | no write to it; a row back with its key starts afresh |
| A failed call beside a good one | each handled as itself |
| A person types while an answer is on its way | the typing wins, as the guide says |
| `live` in each row, while rows move, drop and return, during updates | each row ends right |
| A state set by hand at once, then `live` | ends as the server has it |
| `defer` landing after its page went | dropped; returning shows its own answer |
| The database changes during a run; two pages on one run; a component remade into a run; 60 rounds of stress | no page ended stale |
| A real restart of the server | caught up in 620 to 832 ms; the next change in about 105 ms |
| Leaving a page | its loader stops: 0 runs for the project left, 5 for the one on show |
| 320 round trips between pages, Chromium only | nodes level at 102; heap 2.71 to 3.06 MB; listeners rose and fell, with no steady growth |
| Scoped CSS: `>`, `+`, `:not`, `:is`, `:has`, `:nth-child`, `:first-child`, attributes, lists, `::before`, nesting, `@media`, `@supports`, `@container`, `@layer`, `@keyframes`, custom properties | all apply; a parent's rule does not reach a child's element |
| Layout through islands: grid, a span that grows, flex growth, sticky, absolute | as if the island were not there, at desktop and phone width, with no sideways scroll |
| Pointer capture with a mouse, on all three; real touch on Chromium | drags past the element, and ends |

## Found while fixing (branch feedback61, 2026-11-12/13)

Found by the reviews of the fixes, on main too unless said. Each is fixed on branch feedback61, in the commit named.

### Q-936. A table component of several rows rebuilds its page on load

| Kind | Detected by | Attempts |
|---|---|---|
| bug | review of 0851ec8 | 1 |

The server wrote a component of several `<tr>` as a `<quillo-island>` inside `<tbody>`, which the HTML parser moves out of the table, so the browser made the page again on load and lost early typing. Fixed in 16dfab4: such a component is marked with `<!--q data-c=...-->` comments the parser leaves in place.

| Impact | |
|---|---|
| **Functionality** | The whole page is made again as it loads |
| **User** | Loses what was typed before the code came |
| **Requests** | No change |
| **Security** | No change |

### Q-937. A branched SVG component made in the browser is not drawn

| Kind | Detected by | Attempts |
|---|---|---|
| bug | review of 0851ec8 | 1 |

It sat in a `<quillo-island>`, which SVG does not draw. Fixed in 16dfab4: inside `<svg>` and table and select parts, a component made in the browser has no wrapper.

| Impact | |
|---|---|
| **Functionality** | The shape is missing |
| **User** | Sees nothing where the drawing should be |
| **Requests** | No change |
| **Security** | No change |

### Q-938. A `<select>` shows another value when an option component is made anew

| Kind | Detected by | Attempts |
|---|---|---|
| bug | review of 0851ec8 | 1 |

Fixed in 16dfab4 (no wrapper among options), and for function markup in 81be797.

| Impact | |
|---|---|
| **Functionality** | The select shows an option that is not its value |
| **User** | Sees, and may submit, the wrong choice |
| **Requests** | A form may post the wrong value |
| **Security** | No change |

### Q-939. Rewriting a field while a person composes with an input method breaks the composition

| Kind | Detected by | Attempts |
|---|---|---|
| bug | review of 16dfab4 | 1 |

Main gave `AKBKAか` for `AかB`. Fixed in 81be797 and 19727f3: a field being composed in is not written, and the program's value goes in when the composition ends, only if a write was held. Checked in Chrome through DevTools; Firefox and WebKit only with page-made events.

| Impact | |
|---|---|
| **Functionality** | Composed text doubled or broken |
| **User** | Cannot type Japanese, Chinese or Korean into such a field |
| **Requests** | No change |
| **Security** | No change |

### Q-940. Rows written straight into a `<table>` make their component again on load

| Kind | Detected by | Attempts |
|---|---|---|
| bug | review of 16dfab4 | 1 |

The parser's own `<tbody>` is in no markup, so take-up failed. Fixed in 06cda99, 19727f3 and 61cc8b0: take-up steps into that `<tbody>` (and `<colgroup>`), with a `<thead>` before, a `<tfoot>` after and nested marks; a list mixing `<tbody>` items with rows is still made again, as on main. The served HTML is unchanged.

| Impact | |
|---|---|
| **Functionality** | The component is made again on every load |
| **User** | Loses what was typed before the code came |
| **Requests** | No change |
| **Security** | No change |
