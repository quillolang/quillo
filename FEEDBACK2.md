# Feedback from building a project manager in Quillo

Notes from [pm/](pm), "Lanes", a project manager of about 3,000 lines: accounts,
projects with owners, editors and viewers, a live Kanban board (drag and drop between
columns and swimlanes, WIP limits, filters, Alt+←/→ to move a card), tasks with
checklists, comments, labels, points, days and dependencies, a Gantt timeline whose
bars are dragged to reschedule, milestones, and reports drawn as SVG from the board's
history: cumulative flow, burndown, throughput, cycle time with percentiles, workload,
and a Monte Carlo forecast. Every page was driven in headless Chrome with real mouse
and key events. Everything below was checked on **quillo 0.5.0 (896e758)**, most
severe first. Each item has a repro of a few lines.

## Status on fe10f8c (branch feedback-3)

Checked with `node feedback/run.mjs`, the full test suite, and pm/ and cms/ in headless
Chrome on every page. Beyond each repro, each fix was pushed further (below).

| Item | Status | Checked beyond the repro |
|---|---|---|
| 1. Table rows twice | fixed | rows, cells (`<td>`) and `<option>`s as components stay in their table and select; toggling, adding and removing rows works; the server marks the root (`data-qi`) |
| 2. `let` with type names | fixed | `let int`, `let html`, `for str in` bind; `match x { int => … }` still tests the type; annotations unaffected. **New: Q-035**, a top-level `let int` in one file takes `int()` from every file, with no warning where it is defined |
| 3. Whole floats | fixed | 0.1 + 0.2, 1/3, 2.0, 1e21, -0.0, 12.50m, 2^60, 1e-7 and an attribute (`data-v={2.0}`) are the same text on both sides; no component made again |
| 3. Mismatch reported | partly | with `--watch`, a top-level component made again is named with what differed; one inside another component is not ("quillo: the server wrote nothing, the browser `<div>`") |
| 4. `key` as a parameter | fixed | a load-time error that says why and what to name it instead |
| 5. File names in run-time errors | fixed | |
| 6. Spread | fixed | maps and lists, on both sides; a later key wins as in JavaScript; `{ ...null }` is `{}`. **New: Q-036**, its errors name internals (`__merge`, "cannot add list and null") |
| 7. `set` in a loop | fixed | 20,000 sets: 992 ms → 8 ms. Nothing in place shows through another name: aliases, snapshots pushed in a loop, closures, a `reduce`'s start value, a value passed to a function or held inside another map, all unchanged; the same in the browser |
| 8. Int map keys | fixed | `m[5]`, `get`, `has` on both sides; `group_by` keys documented as text |
| 9. `return` as an expression | fixed | in match arms, after `??`, and in component code; the guide now says `return` in `for … =>` leaves the function |
| 10. `cookie(name)` | fixed | in a helper a route calls |
| 10. `set` on lists | fixed | negative index from the end; out of range is a run-time error |
| 10. `flatten`, `<quillo-raw>`, the database's path | fixed | in the guide; `quillo: database /…/pm.db` at start |
| Limitations: dates, live payloads | not addressed | |

What worked well: SVG made by components keeps its namespace through every update, so
charts and the timeline are plain markup with no library. HTML5 drag and drop works with
`e.dataTransfer`, as do pointer capture (`e.target.setPointerCapture`) and a document
listener added in `@mount`. `live` with `watch`, `unwatched` and `publish` kept the
board, the open task, the timeline and the reports in step across tabs. Keyed rows kept
their DOM and their focus through live reloads. A 3,000-card board resumes in about 450
ms. `stats.percentile`, `stats.describe` and `stats.histogram` covered the reports.
`server fn` refused bad argument types and counts with a 400 that names the argument.
`quillo check` caught a variable named `last` that hid the builtin.

## 1. Bug: a component that is a table row shows each row twice

A component is wrapped in `<quillo-island>`. Inside `<tbody>` (or `<tr>`, `<select>`)
the HTML parser does not allow an unknown element, so it moves the island out in front
of the table and leaves the `<tr>` inside. The browser cannot take the page up (nothing
matches) and makes the component again, in the island, outside the table. Result: every
row twice. The server's copy is inside the table and dead (its buttons do nothing). The
live copy is above the table, out of its layout. Nothing is printed in the console.

```quillo
component Line(name: str) {
    state on = false
    <tr><td>{name}</td><td><button @click={() => { on = !on }}>{on ? "on" : "off"}</button></td></tr>
}
route GET / = <html><body><table><tbody><Line name="a"/><Line name="b"/></tbody></table></body></html>
```

After load: `__quilloRemade` is 2, and the body is `<quillo-island><tr>a…</tr></quillo-island>
<quillo-island><tr>b…</tr></quillo-island><table><tbody><tr>a…</tr><tr>b…</tr></tbody></table>`.

A settings table whose rows keep what is typed until saved needs exactly this. The app
uses a grid of `<div role="row">` instead.

**Suggestion:** where an island cannot stand (inside `table`, `thead`, `tbody`, `tfoot`,
`tr`, `select`, `optgroup`, `colgroup`), mark the component on its root element
(`<tr data-q-island=…>`) rather than wrapping it. Until then, `quillo check` should report
a component whose root is `tr`, `td`, `th`, `tbody`, `option` or the like, and the guide
should say so beside "The island is `display: contents`".

## 2. Bug: `let` and `for` silently do not bind names like `int`, `str`, `num`, `list`

A variable named after a type is accepted, and then reading it gives the builtin, or
nothing. `quillo check` reports no problem in any of these:

| Program | Prints |
|---|---|
| `let int = 5` then `print(int)` | `<builtin int>` |
| `let str = "a"` then `print(str)` | `<builtin str>` |
| `let x = 1`, `let int = x + 1`, `print([int, x])` | `[<builtin int>, 1]` |
| `for str in ["a"] { print(str) }` | `<builtin str>` |
| `let float = 2.5` then `float * 2` | `cannot multiply fn and int` |
| `let num = 5` then `num + 1` | `unknown name num (did you mean sum?)` |
| `let list = [1]` then `print(list)` | `unknown name list (did you mean last?)` |
| `fn f(num) = num * 2`, `f(3)` | `6` (a parameter works) |
| `let { num } = { num: 3 }`, `print(num)` | `3` (destructuring works) |

`num` is an ordinary word in this domain (a task's number). The app hit it twice
(`let num = db.one(...).num` in two files). The `unknown name num` error at the *use*,
with the `let` one line above, is confusing. The `int`/`str` cases are worse: the
program runs and computes with a builtin function where a value was meant.

The guide says the opposite: "a variable may take one (`let count = 0`)". That is true
for `count` and not for the type names.

**Suggestion:** let `let`/`for` bind these names as they bind any other (parameters and
destructuring already do), or make it an error at the `let`: "`num` is a type name and
cannot be a variable".

## 3. A whole float prints differently on the server and in the browser, and the page is made again

The server writes `round(8.04, 1)` as `8.0`; the browser writes `8`. Any component that
shows such a number does not match the server's HTML when it is taken up, so it is made
again from scratch. That costs a flash, the work, and the "resumed, not made again"
promise. Nothing tells you. At the top level the console is silent; for a nested
component it prints `quillo: text`, with no component, no text and no hint.

```quillo
component Score(v: float) {
    state n = 0
    <p>{round(v, 1)} days <button @click={() => { n += 1 }}>{n}</button></p>
}
route GET / = <html><body><Score v={8.04}/></body></html>
```

The server sends `<p>8.0 days`; the browser shows `8 days`; `__quilloRemade` is 1.

The reports page did this with any whole percentile ("95% within 8.0d"). Finding it
took a DevTools breakpoint inside `claimText` in the runtime. [USERGUIDE.md:501](USERGUIDE.md#L501)
says a whole float prints as `2` in the browser, but not that this undoes resuming.

**Fix:** print a float in HTML text and attributes the same way on both sides. The
server could print whole floats as JavaScript does (`8`, and `y="216"` not `"216.0"`),
or the browser could print them as the server does. Also, when resuming fails, report
it in development (`--watch`): the component, and the two texts
(`Score: the server wrote "8.0 days", the browser "8 days"`).

The app's workaround is a `num1(x)` that writes every displayed decimal itself.

## 4. A parameter named `key` collides with a list's `key`, and nothing says so

```quillo
view Row(key: str, label: str) = <li>{key}: {label}</li>
component Keyed() {
    state xs = [{ id: 1, name: "a" }]
    <ul>{for x in xs => <Row key={x.id} label={x.name}/>}</ul>
}
```

`quillo check`: ok. Rendering: `argument key of Row must be str, got int 1`, a 500. The
list's `key={...}` is passed to the view as its `key` parameter. In the app a card view
had both a project key and a row key (`<TaskCard key={t.id} {key}/>`), so both
`key` attributes were on one element, and that was not reported either.

**Suggestion:** reserve `key` as a parameter name (an error at `view Row(key: …)`), and
report an attribute written twice on one element.

## 5. Runtime errors in a program of several files name a line but not the file

```
quillo: error: line 177: argument `key` of `TaskCard` must be str, got int 1
quillo: error in server fn `load_reports`: line 29: list has no field `day`
```

The program has 15 files, and most of them have a line 177. `quillo check` gives
`./ui/board.qwe:177:`, but the run-time errors (in the terminal, in the page with
`--watch`, and in a server fn's log line) do not. In the repro for item 4, the two
errors are `line 5` (in `main.qwe`) and `line 3` (in `lib/rows.qwe`), side by side.

**Fix:** write `ui/board.qwe:177:` in run-time errors, as `quillo check` does.

## 6. `{ ...m, name: v }` is in the guide, but there is no spread in maps or lists

[USERGUIDE.md:585](USERGUIDE.md#L585): "a map's fields are made anew, with `set` or
`{ ...m, name: v }`". Neither that nor `[...xs, 3]` parses:

```
t.qwe:2: error: expected an expression, found `..`
note: this is quillo 0.5.0 (896e758, 2026-10-28), built 2026-10-27 at …; code written for a newer Quillo fails to parse like this
```

The note sends you off to look for an old binary, when the build is the newest there
is. `merge(t, { label_ids })` does the job (the app uses it in eight places).

**Suggestion:** support spread in map and list literals (JSX attributes already take
`{...map}`), or change the guide to `merge`. Show the version note only when the
failing token is one a newer Quillo knows, or word it "if this code is meant for a newer
Quillo…".

## 7. A map grown with `set` costs the square of its size

Values cannot be changed, so `set` copies the map. Growing one in a loop, the obvious way
to index rows by id, is quadratic:

| | Time |
|---|---|
| `reduce(range(20000), {}, (m, i) => set(m, str(i), true))` | 992 ms |
| 2,000 × `contains` on a 20,000-item list | 14 ms |
| 2,000 × `has` on the map | 0 ms |

The reports replayed the board's history this way (`place = set(place, task_id,
column)` for each move). With 3,000 tasks and 6,000 moves, `load_reports` took 387 ms,
and it runs again for every open page on every change. Rewritten with `group_by` (native,
one pass) and per-day deltas, it gives the same numbers in 50 ms.

**Suggestion:** persistent maps (a HAMT) so `set` is O(log n). Failing that, a
`to_map(list, key_fn)` or `from_entries` builtin, and a line in the guide beside
"values are immutable" that says what `set` in a loop costs.

## 8. Map keys are always strings, but indexing by an int is an error

| Expression | Gives |
|---|---|
| `set({}, 5, "five")` | `{ "5": "five" }` |
| `get(m, 5)`, `has(m, 5)` | `"five"`, `true` |
| `m[5]` | `error: cannot index map with int` |
| `group_by([{ a: 1 }], (x) => x.a)` | `{ "1": [...] }`: the int key is now text |

Each is reasonable alone (JSON keys are text). Together, `m[5]` is the one form refused,
after `set(m, 5, …)` was accepted. The app writes `by_id[str(id)]` everywhere.

**Suggestion:** let `m[5]` read key `"5"` as `get` does, and say in the guide (Maps)
that keys are text and numbers are converted.

## 9. `return` leaves the function from inside `for … =>`, and is not an expression

- [USERGUIDE.md:154](USERGUIDE.md#L154): "There is no `break` or `continue`: `return`
  leaves a loop early." Inside a list comprehension it leaves the whole function:

  ```quillo
  fn f() {
      let xs = for x in [1, 2, 3] => { if x == 2 { return "left f" }; x * 10 }
      xs
  }
  f()     // "left f"
  ```

  Skipping an item takes a `filter` first. The timeline's dependency arrows needed one.
- `return` is not allowed where an expression is expected: `"column_id" => return
  move_task(…)` in a `match` arm, and `let m = re.find(…) ?? return null`, are both
  `unexpected keyword return`. A block works: `=> { return … }`. `reply` is allowed
  there (`?? reply 404`), and that's the form people will copy.

**Suggestion:** allow `return` as an expression, like `reply`. In the guide: "`return`
leaves the function, from a loop too; to skip items, `filter` first."

## 10. Smaller things met on the way

- **`request` only exists in routes and server fns**, but `session()` works in any
  function. A helper that reads a cookie (the viewer's time zone) has to take `request`
  as an argument from every caller. A `cookie(name)` builtin, like `session()`, would
  fix it.
- **No way to change one item of a list.** `set(xs, 1, v)` is refused (`argument 1 of
  set must be map`). Swapping two columns' order is
  `for [i, x] in enumerate(xs) => i == a ? xs[b] : i == b ? xs[a] : x`.
- **`flatten` is one level**, and the checker cannot see that a list of lists of maps was
  passed where maps were expected. The error came at run time (`list has no field
  day`, without a file; see 5).
- **`db.open(env("PM_DB", "pm.db"))` is resolved from the program's folder**, not the
  working directory. The guide says "next to the program". But a test run pointed at a
  copy (`PM_DB=pm.db`, from the copy's folder) silently opened the real database and
  wrote to it. Printing the resolved path at start (`quillo: database
  /…/pm/pm.db`) would have caught it.
- **`raw(...)` in a component is wrapped in `<quillo-raw style="display: contents">`.**
  The guide does not mention it. CSS such as `.md > p:first-child` stops matching.

## Limitations a project manager runs into

### Days and time zones

The date module went in 0.5.0 (205d7cd: "Finance support is exact decimals and money;
the rest is the program's"). A project manager has days everywhere: start and due days,
milestones, "overdue", weeks for throughput, a calendar for the timeline.
[pm/lib/dates.qwe](pm/lib/dates.qwe) is 100 lines of Hinnant's `days_from_civil` and
`civil_from_days` with `floor` for every integer division, plus parsing, weekdays, week
starts and labels. It is the same on both sides, which matters: the timeline drags in
the browser and saves on the server.

There are no time zones. The server knows only UTC, so "today" is UTC's: at UTC+8,
"overdue" and the burndown's last day are a day behind until 08:00. The app's
[pm/public/js/pm.js](pm/public/js/pm.js) sets a `tz` cookie from
`getTimezoneOffset()`, and every route and server fn passes `request` to `today(req)`.
The first page a browser ever loads is made before the cookie exists, in UTC.

**Suggestion:** a small set of day functions (`date.parse`, `date.format`,
`date.add_days`, `date.weekday`), the same on server and browser, without full time-zone
support. Also the browser's offset on each server-fn call (`request.tz`), so programs do
not need their own cookie.

### Live data sends the whole value again

On every change, each open page calls its loader again and gets its whole answer. At
3,000 cards that is 992 KB (44 KB gzipped) per change per open board, and the loader runs
once per open page (31 ms each on SQLite). It is fine at this size, and the topic
design (`watch("project:<id>")`) keeps it to the project's own pages. A team of 50 with
the board open would make the server run `load_board` 50 times for each card dragged.
A loader that could answer "what changed since version n", or one run shared between
pages with the same arguments and viewer role, would be the next step.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Dragging a card onto another card, a column, or another lane | lands before that card, at the column's end, or with its assignee/priority changed |
| Moving a card into Done while what it waits for is open | refused with "WEB-45 is blocked by WEB-44", and the card goes back |
| A dependency that closes a loop (A waits for B, B for A, or longer) | refused (recursive CTE) |
| A due day before the start day; `2026-02-30` | refused with a message |
| A viewer's or outsider's server-fn calls (`move_task`, `edit_task`, `load_board`) | `{"err": ...}`; a viewer may still comment |
| A server fn called with `"1"` for an int, too many or too few arguments | 400, naming the argument |
| A project page for someone outside it | the same 404 as for no project |
| Markdown with `<img onerror>` in a comment or a description's preview | shown as text |
| A spreadsheet formula in a task's title (`=HYPERLINK(...)`) | the CSV export writes `'=` |
| `tz` cookie of 99999 | ignored, UTC |
| A project with no tasks, milestones or history | every page and chart renders, with a sentence where data is missing |
| Focus on a card while others' changes arrive | kept; keyed cards keep their DOM |
| Alt+→ on a focused card | the card moves a column, focus stays on it |
| Two tabs on one board | each sees the other's drags, edits and new cards without reloading |
| Every page | resumed, not remade (`__quilloRemade` 0) once items 1 and 3 are worked around |
