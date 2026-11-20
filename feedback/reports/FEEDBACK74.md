# Feedback from studying state machines and a history for undo and redo in Quillo

| | |
|---|---|
| App | None: a study done at the user's request, 2026-11-17: "study the benefit of statemachine and historian(undo/redo) feature to quillo" |
| Quillo | The released `quillo 0.8.6 (238dac2, 2026-11-16)` on the PATH. This checkout (branch `security`, `c616da3`) was read for the guide and the apps but not built, since it holds another session's uncommitted work |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check` and `quillo run`. Headless Chrome through `feedback/tools/chrome.mjs`: real clicks and keys, `document.execCommand("undo")` for the browser's own undo, and the heap read over the DevTools protocol after a forced collection. curl with 20 pairs of requests sent at once, on 1 and on 4 instances. SQLite timings in a script. Read: the guide (Reactivity, Server functions' "The latest call wins", Records, Patterns, Sagas, Live data), the apps' status changes (`app/*/lib`), the 628-plugin catalog (`crdt`, `json_patch`, `event_log`, `diff`), the `yrs` crate behind `crdt`, and the DevTools panel's repo. The probes are in `.git/wip/feedback74/` |

Numbers Q-1040 to Q-1047, from the block Q-1040 to Q-1049. That block comes after Q-1030 to Q-1039, the last one agreed. This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed, and the repros are written out below. Under the rule of 2026-11-05, nothing here is built until main is released and in sync. FEEDBACK71's batch comes first.

**How the request is read.**

- **A state machine** is a value that is always in one of a known set of states. It moves between them only along moves declared beforehand, and its state decides what may happen next. Quillo programs have two kinds:
  - In a component: a load (idle, loading, done, failed), a form being sent, the steps of a wizard, a media player.
  - A row in the database: a loan, a leave request, an order. Its moves are writes by different people, often at the same moment.
- **A historian** is read, as the parenthesis says, as the history of values that undo and redo walk through:
  - In the page: undo and redo of what a person edits.
  - On the server: each row's past versions, who made each one and when, and putting one back.

  In plant software, "historian" also names a store of sensor readings over time. That is not studied here. Neither is a statechart's "history state", which remembers the last inner state of a nested one.

What worked well:

- **Snapshots are cheap.** Values never change in place, so a step of history is the old value, kept, and the items that did not change are shared rather than copied. 100 kept steps of a 10,000-row list cost 4.1 MB of heap. 200 edits took 26 ms (0.13 ms each).
- **Activities come for free.** "What a statement starts, it stops" already gives a state its own timers. `if phase == "loading" { every(100, ...) }` ticked only while loading and stopped at Cancel, at 200 ms.
- **The latest call guards a load.** "The latest call wins" already protects a load, as long as the phase and its data are one state. Cancel stayed cancelled, and of two loads only the second landed (Part B.1, loader D).
- **Unions of records work.** `type Load = Idle | Loading | Done | Failed` is checked as the program runs, and `match` takes each member by name.
- **History works across islands.** A history held in page state worked across two islands: the Undo button in `<header>`, the editing in `<main>`.
- **The careful moves never doubled.** The coop app reads and moves each loan inside one serializable transaction. The guide's saga moves with a compare-and-set (`where id = ? and state = ? and step = ?`, USERGUIDE.md:6734). Written that way, a move was never made twice in the probe.

## The answer

**Yes to both, but mostly not as new syntax. Quillo already does the hard half of each. What is missing is where the measured failures were:**

- **The checker** takes a set of states as `str`, so typos and missing states pass.
- **On the server,** nothing makes a move a compare-and-set, and the shape people write moved 20 of 20 rows twice.
- **On the server,** a history of rows is written by hand at every write.

**In the page, the guide should teach one state per machine, and a small `undo` module would save every editor from writing its own.**

| | Today | Proposed | Measured |
|---|---|---|---|
| **A machine in a component** | `state`, `match` and statements that start and stop with it. One state holding the phase and its data is right. Two states are not (Q-1041) | The checker knows sets and unions (Q-1042 to Q-1045). The guide teaches one state | After Cancel: two states showed "done", or "idle" with 3 rows. One state showed "idle" with 0 |
| **A machine in the database** | Each move written by hand. 15 of the apps' 42 status updates compare the old state in the update itself | `moves` on the type, and `db.move` as one compare-and-set (Q-1040) | 20 of 20 rows moved twice with a wait between check and write. 4 of 20 on 4 instances with no wait. 0 with a compare |
| **Undo in the page** | Four one-line functions written by hand, plus the keys | An `undo` module of pure functions, on both sides (Q-1047) | 100 steps of 10,000 rows: 4.1 MB. One step per letter typed |
| **History of rows** | An `audit()` call per write (33 in the apps), or a trigger, which live pages do not see | `db.history(table)`, `db.versions`, `db.restore` and `db.forget` (Q-1046) | A trigger costs 0.65 to 1.0 µs more per row updated |

The four impacts of all four parts:

| Impact | The checker, `db.move`, `db.history` and `undo` |
|---|---|
| **Functionality** | A misspelled state, a missing arm and a field of another state are found before running. A move the machine does not have is refused with the state the row is in, and so is one that someone else made first. It is never made twice. A row's past versions can be listed and put back. Pages get undo and redo with one step per action, and typing joined into one step. No change for a program that uses none of it |
| **User** | Visitors get an Undo that works like a desktop app's, and never "approved" and "rejected" for one request. Developers get the moves in one place instead of a check in every function, and errors from `quillo check` instead of a wrong page |
| **Requests** | None for the checker or the page's undo. `db.move` is one `update` where today's shape is a `select` and then an `update`, so one round trip fewer. A refused move costs one more `select`, to say why. `db.history` writes one more row per row changed, in the same statement: 0.65 to 1.0 µs on SQLite. No change to a program without them |
| **Security** | A move is where who may do what is checked, and today a race lets both of two moves through. A compare-and-set cannot be raced. A row's history keeps what was deleted or edited away, so it needs `db.forget` for erasure requests, and it belongs to the tenant. The page's undo keeps past values in memory until the page goes, so a card number typed and then cleared is still there: password and card fields stay out of it |

What today's way gives that the proposal would lose:

- **Plain SQL.** A move written as SQL shows exactly what runs, and may set any columns. `db.move` keeps the second: its last argument is the other columns to set.
- **No new word.** `moves` would be a new word at the top level, so every editor's grammar changes ([[editors-day-one]]).
- **Freedom.** An undo written by hand can join steps its own way, or keep only some fields. The module keeps both: it is plain functions over a value.

**My recommendation, in this order:**

1. **The checker learns sets of strings and unions of records** (Q-1042 to Q-1045). It costs nothing as a program runs, and every machine, in the page or the database, gains from it.
2. **`moves` on a set type, and `db.move`** (Q-1040). This fixes the one data bug measured.
3. **The guide's pattern and the `undo` module** (Q-1041, Q-1047). Small: the module is about 15 functions, written once for both sides.
4. **`db.history`** (Q-1046). The largest, since each of the three database families needs its own triggers.

No statechart language (nested and parallel states, a visual editor, XState's actors): B.1 shows that Quillo's components already have nearly all of it.

## Part A: what works today, measured

| Check | Result |
|---|---|
| `r.status == "aproved"`, with `status: Leave` and `type Leave = "pending" \| "approved" \| ...` | `quillo check`: ok. Runs as `false` (Q-1042) |
| `fn next(s: Leave) -> Leave` returning `"aproved"` | `quillo check`: ok. At run time: `` `next` must return Leave, but returned str "aproved" `` |
| A `match` over a `Leave` with no `"cancelled"` arm | `quillo check`: ok. Fails when a cancelled one comes (Q-1044) |
| `len(s.rows)` with `s: Load`, on an `Idle` | `quillo check`: ok. Gives 0 (Q-1043) |
| `show({ phase: "done" })`, a Done without its `rows` | Refused at the call: `` `phase` must be "idle", got str "done" ``, naming the first member only (Q-1045) |
| `type Load = { phase: "idle" } \| { phase: "done", rows: list }` | Parse error, `expected end of statement, found \|`: each record in a union must be named first |
| Loader A (two states), Cancel 250 ms into an 800 ms load | "idle", then "done" with 3 rows when the answer came (Q-1041) |
| Loader B (two states, the phase decided by the answer), the same | "idle" stayed, but 3 rows came in |
| Loader C (a ticket by hand), the same | "idle", 0 rows |
| Loader D (one state: `s = { phase: "done", rows: load(3) }`), the same | "idle", 0 rows |
| Two loads 300 ms apart | A showed "done" 900 ms in, with the second still out. B, C and D said "loading" until the second answer |
| `if phase == "loading" { every(100, ...) }` | Ticked to 200 and stopped at Cancel. With two loads, it stopped at the first answer (500) |
| Undo written by hand: 3 Adds, Undo, Redo | 3, 2, 3 items. "3 back", "2 back, 1 forward", "3 back" |
| Typing "abc" into a field that is part of the document | 3 steps, one per letter |
| Cmd+Z with the focus in that field | The container's `onkeydown` took it and went back one step: the field shows "ab" |
| A field bound to state, "hello" typed, then the browser's undo and redo | "hell" and the state followed, then "hello" again |
| The same field after Upper (`name = upper(name)`), then the browser's undo | Stays "HELLO": a value set from code clears the browser's undo for that field (Q-1047) |
| After Clear (`name = ""`), then the browser's undo | Stays empty: "hello" is gone |
| 200 edits of a 10,000-row list, 100 steps kept | 26.2 ms in all. Heap 2.2 MB before and 6.3 MB after, both after a forced collection: 41 KB a step, about 4 bytes per row per step |
| One history in page state: Undo in `<header>`, Add in `<main>` | "1,2,3", then after two Undos "1", then after Redo "1,2". Both islands followed |
| Approve and reject of one leave sent at once, read-check-write with `sleep(50)` between, 20 rows | 20 of 20 moved twice. All 40 answered 200 (Q-1040) |
| The same with `where id = ? and status = 'pending'` and a check of `changes` | 0 moved twice. 20 answered 200 and 20 answered 409 |
| Read-check-write with no wait, 1 instance | 0 of 20 moved twice |
| Read-check-write with no wait, `--instances 4` | 4 of 20 moved twice |
| 20,000 updates on SQLite, then the same with a trigger writing the old row as JSON to a history table | 23 to 33 ms, then 42 to 46 ms, over three rounds. 60,000 history rows took 9.4 MB with the rows they came from |
| The catalog | `json_patch` (diffs to undo with), `event_log` (event streams with snapshots), `diff` (text) and `crdt` (Yjs through `yrs` 0.28). None keeps a history for a program. `yrs` has an `UndoManager` with tracked origins and a capture timeout (`yrs-0.28.0/src/undo.rs`), but the `crdt` plugin does not expose it |
| The DevTools panel (`~/dev/quillo-devtools`) | Shows state as it is now. No history of sets |

## Part B: state machines

### B.1 In a component

A statechart's parts, and what a Quillo component already has for each:

| A statechart has | Quillo today |
|---|---|
| States | A set of strings, or a union of records when a state carries data |
| Events and the moves they make | Handlers that set the state |
| Guards | An `if` in the handler |
| Entry and exit actions, activities | A statement that reads the state: `if s.phase == "loading" { every(...) }` starts on entering and stops on leaving |
| Delayed moves (XState's `after: 5000`) | `if s.phase == "loading" { run_after(5000, () => { s = { phase: "failed", why: "timeout" } }) }`, stopped if the state left first |
| Invoked services | A `server fn` call. "The latest call wins" drops a late answer when the state was set since |
| Nested and parallel states | A nested `match`, and separate states |
| An illegal move refused | None: every set is made (`moves` in B.2 would work here too) |
| A picture of the machine | None (`quillo docs` could draw one from `moves`) |

So a component needs no new machinery. It needs the checker to know the states (Q-1042 to Q-1045), and one rule in the guide: **a machine is one state.** The four loaders show why:

```quillo
server fn load(n: int) -> list with time {
    sleep(800)
    for i in 0..n => `row ${i}`
}

// A: the shape most code has. Cancel, then the late answer: "done", 3 rows.
component LoaderA() {
    state phase = "idle"
    state rows = []
    <div>
        <button onclick={() => { phase = "loading"; rows = load(3); phase = "done" }}>Load</button>
        <button onclick={() => { phase = "idle" }}>Cancel</button>
        <p>{phase}: {len(rows)}</p>
    </div>
}

// D: one state for the phase and its data. Cancel, then the late answer: "idle", 0 rows.
component LoaderD() {
    state s = { phase: "idle" }
    <div>
        <button onclick={() => { s = { phase: "loading" }; s = { phase: "done", rows: load(3) } }}>Load</button>
        <button onclick={() => { s = { phase: "idle" } }}>Cancel</button>
        <p>{s.phase}: {len(s.rows ?? [])}</p>
    </div>
}
```

In A, `phase = "done"` is a plain set, which "the latest call wins" always makes. B decides the phase by the answer (`if is_err(r) { ... } else { rows = r; phase = "done" }`), so the phase gives way. But `rows` was not set by Cancel, so it still takes the answer: "idle", with 3 rows. In D the answer and the phase are one set, which gives way as a whole. C does what a machine library would, with a ticket, in 5 more lines.

The guide nudges towards A: "there is no loading flag either: set a state of your own around the call" (USERGUIDE.md:3248).

### B.2 In the database

The apps have 42 status updates in 15 files of 7 apps (bank, clinic, cms, coop, forum, shifts, stays). 15 of them compare the old state in the update itself (counted line by line, so an update split over lines may be missed). The others check a row they read first, some inside a serializable transaction, as the coop's loans do. I did not audit those one by one. Each move repeats the same five parts:

1. Read the row.
2. Check its state, and who may move it.
3. Write the new state and the columns that go with it.
4. Write an audit row.
5. `publish` its topics.

The coop's `review_loan` is 20 lines (`app/coop/lib/loans.qwe:126`), one of 7 status updates in that file.

Only steps 1 to 3 need the machine, and they are where the race is. A sketch of the proposal (the syntax is my default, to be decided):

```quillo
type Leave = "pending" | "approved" | "rejected" | "cancelled"

moves Leave {
    "pending" -> "approved" | "rejected" | "cancelled"
    "approved" -> "cancelled"
}

server fn decide_leave(id: int, approve: bool, reason: str) ! str with db, time {
    let u = acting()?
    let l = db.move("leaves.status", id, approve ? "approved" : "rejected", { decided_by: u.id, decided_at: now(), reason })?
    publish(leave_topic(l.user_id))
    true
}
```

- **`db.move(table.column, id, to, others)`** knows from `moves` which states may move to `to` (here only `pending`). It is one statement: `update leaves set status = ?, decided_by = ?, ... where id = ? and status in ('pending')`, giving the row (`returning *`, or a `select` in the same transaction on MySQL).
- **When no row changes,** it reads the row once to say why, as an error value: `` `leaves` 7 is approved: a Leave moves from approved only to cancelled ``, or `` no `leaves` 7 ``. The same works with `db.connect` handles (`conn.move`) and with tenants.
- **The checker** refuses a literal move the machine does not have (`db.move("leaves.status", id, "pending")` when nothing moves to `pending`), and a `moves` naming a state the type lacks.
- **`moves(Leave, "pending")`** gives `["approved", "rejected", "cancelled"]`, in the page as on the server: which buttons to show. The same list can tell an AI client which of `fn mcp`'s tools apply to a record now, and `quillo docs` can draw the machine.
- **In a component,** a set of a state whose type has `moves` could be checked the same way: a set that is not a move is dropped, and the console says so. That would fix loader A without a ticket. I am less sure of it than of `db.move`, since it changes what a plain set means. It is left out of the recommendation.

## Part C: history

### C.1 Undo and redo in the page

Written by hand, undo is short, because values do not change in place:

```quillo
fn start(v) -> map = { past: [], now: v, next: [] }
fn put(h: map, v) -> map = v == h.now ? h : { past: slice(h.past + [h.now], -100), now: v, next: [] }
fn back(h: map) -> map = len(h.past) == 0 ? h : { past: take(h.past, len(h.past) - 1), now: last(h.past), next: [h.now] + h.next }
fn forth(h: map) -> map = len(h.next) == 0 ? h : { past: h.past + [h.now], now: first(h.next), next: drop(h.next, 1) }
```

What the probe showed is missing around it:

- **Typing is one step per letter.** Joining a word into one step needs a time and the field it came from.
- **Ctrl+Z and Cmd+Z are taken from fields.** A key handler on the container gets the field's keys too, and went back a step of its own while the focus was in the field. It must call `preventDefault` to keep the browser's undo from running as well, so the field's own undo does not run (read from the handler, not seen in headless Chrome).
- **A field set from code drops out of the browser's undo.** After `name = upper(name)` or `name = ""`, the browser's undo does nothing (Q-1047). Only a history of the program's own can bring "hello" back.
- **Page-wide keys need more code.** They need `onkeydown` on an element with `tabindex`, or a listener through `js()` in an `@mount`.
- **Undo with other people editing.** When others edit the same document, a snapshot undo puts back their changes too. Undoing only one's own needs operations (Yjs's `UndoManager`), which the `crdt` plugin could expose. That is a plugin change, not core.

A sketch of the `undo` module. Its functions are pure, so it works in components and on the server alike:

```quillo
component Drawing() {
    state h = undo.start({ shapes: [], title: "" }, { keep: 100 })
    let doc = h.now
    undo.keys(h, (next) => { h = next })       // Ctrl/Cmd+Z, Shift+Ctrl/Cmd+Z, Ctrl+Y on the page; a field's own undo is left alone
    <div>
        <input value={doc.title} oninput={(e) => { h = undo.put(h, merge(doc, { title: e.value }), { join: "title", within: 1000 }) }}/>
        <button disabled={!undo.can_back(h)} onclick={() => { h = undo.back(h) }}>Undo</button>
        <button disabled={!undo.can_forward(h)} onclick={() => { h = undo.forward(h) }}>Redo</button>
    </div>
}
```

- **A history is a value** (`{ past, now, next, keep, joined }`), so it is kept by HMR like any state, and goes in a page state to be shared by islands.
- **`join` and `within`** make one step of the edits to one field while they come less than a second apart.
- **`undo.keys`** is the only part that touches the page, and it stops with its component, as `listen` does.
- **Where it lives:** a builtin module rather than a file in the UI kit, since the kit is copied into each app and a model reading `llms.txt` finds builtins. As a builtin, it needs its entries in the browser's list, `script.rs`'s SAFE list and `builtin_docs.rs`.
- **The name `undo`,** not `history`: `history.back()` is the browser's navigation to anyone coming from JavaScript.

A runtime-level history (`state doc = v keep 100`, one step for each handler run, the DevTools panel walking it) would do more by itself. It is left for later, as it needs the runtime to know which sets are a person's and which are an answer's.

### C.2 History of rows

The apps keep history by hand: 33 `audit(...)` and `insert into activity` writes, beside the moves they record. A trigger could do it in the database instead, but has two gaps:

- **Live pages do not see what a trigger writes** (USERGUIDE.md:2975), so a page listing a row's history does not follow it.
- **A trigger does not know who the visitor is.**

A sketch:

```quillo
db.open("app.db")
db.history("articles")                         // every row's versions kept, with who and when

route GET /articles/:id/history with db =
    <ul>{for v in db.versions("articles", int(id)) => <li>{v.at} {v.by} {v.op}: {v.row.title}</li>}</ul>

server fn put_back(id: int, version: int) ! map with db = db.restore("articles", id, version)
```

- **The triggers.** Quillo installs them, one set per database family, written by the driver ([[database-layer-removes-decisions]]). Each writes the old row as JSON into `_quillo_history`, in the program's database or the tenant's, since it must commit with the work, as `_quillo_idempotency` does.
- **`by` is `session().user`.** It reaches the trigger as a value set for the connection:
  - PostgreSQL: `set_config(..., true)`;
  - MySQL: a session variable;
  - SQLite: a function Quillo registers on the connection.

  Writes made from `psql` or a cron script are kept too, with no `by`.
- **Live pages follow.** A loader that calls `db.versions("articles", ...)` is recorded as reading `articles`, so its pages follow every write to the table without seeing the trigger.
- **`db.restore`** is a write like any other: it is checked by the program's own guards, kept as a version itself, and seen by live pages. A row deleted since is put back under its id.
- **`db.forget("articles", id)`** drops a row's versions, for erasure requests. `{ keep: "90d" }` drops the old ones.
- **The cost** is only for tables named, about 0.65 to 1.0 µs more per row changed on SQLite (measured above).

## New issues

### Q-1040. A state's move written as read, check and write is made twice when two people move one row at once

| Kind | Detected by | Attempts |
|---|---|---|
| silent | scale test (20 pairs sent at once, 1 and 4 instances) | 1 |

A supervisor approves a leave request while another rejects it. Both requests read "pending", both pass the check, and both write. Both answer 200, both notifications go out, and the row ends with whichever wrote last. With any wait between the check and the write (a call to a service, `http.*`, `sleep`), one instance is enough: 20 of 20 rows moved twice. With no wait, 4 instances moved 4 of 20 twice. Written as a compare-and-set, none was. The guide shows the compare-and-set inside the saga (USERGUIDE.md:6734) and for balances (USERGUIDE.md:8060), but nowhere for a status.

```quillo
db.open(":memory:")
db.exec("create table leaves (id integer primary key, status text not null)")
db.exec("create table sent (leave_id int, what text)")
for i in 1..21 { db.exec("insert into leaves (id, status) values (?, 'pending')", i) }

route POST /naive/:id/:to with db, time {
    let l = db.one("select status from leaves where id = ?", int(id))
    if l.status != "pending" { reply 409 `It was ${l.status} already` }
    sleep(50)                                              // a check that asks a service
    db.exec("update leaves set status = ? where id = ?", to, int(id))
    db.exec("insert into sent (leave_id, what) values (?, ?)", int(id), to)
    `moved to ${to}`
}

route GET /sent = db.query("select leave_id, group_concat(what) as whats from sent group by leave_id having count(*) > 1")
```

```sh
for i in $(seq 1 20); do
  curl -s -X POST localhost:8080/naive/$i/approved & curl -s -X POST localhost:8080/naive/$i/rejected &
done; wait
curl -s localhost:8080/sent     # 20 rows: [{"leave_id":1,"whats":"rejected,approved"}, ...]
```

| Impact | |
|---|---|
| **Functionality** | One request is both approved and rejected. Everything done for each move happens twice (mails, payments, stock), and the row keeps the last write |
| **User** | The visitor gets two answers that contradict each other. The developer sees nothing in a test with one person |
| **Requests** | No change: the same requests, both answered 200 |
| **Security** | A move is where permissions are checked, and a race lets both of two contradicting moves through: for example, an order both refunded and shipped |

**Suggestion:** `moves` on a set type and `db.move` (Part B.2). Until then, put the compare-and-set for a status in the guide's Database section: `update ... where id = ? and status = ?`, then check `changes`.

### Q-1041. With the phase and its data in two states, a load cancelled or started again shows the old answer

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (headless Chrome) | 1 |

Loaders A and B in Part B.1:

- **A, after Cancel:** shows "done" when the late answer comes.
- **B, after Cancel:** stays "idle", but takes the 3 rows.
- **A, two loads 300 ms apart:** says "done" while the second load is still out, and its timer stops early (500 ms counted for an 800 ms wait).

The one-state loader D was right in every case. This is not a fault in "the latest call wins", which does as the guide says. It is the class of bug a state machine removes, and the guide's own advice leads to it: "set a state of your own around the call" (USERGUIDE.md:3248).

| Impact | |
|---|---|
| **Functionality** | A cancelled load completes anyway, and a page shows data the person turned away from |
| **User** | "Done" after Cancel. A search box shows results for the text typed before the last one |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:**

- **The guide:** in Server functions and Reactivity, say that a machine is one state, with loader D as the example. Change "set a state of your own around the call" to show `s = { phase: "loading" }` and then `s = { phase: "done", rows: load() }`.
- **`quillo check` could warn** when a handler sets a state before a wait and sets it again with a plain value after the wait. Say that the set after the wait is made even when another run set it since, and that it should come from the answer.

### Q-1042. `quillo check` takes a set of strings as `str`, so a misspelled state compares false and a state that is none passes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` (passes), run time | 1 |

The guide says so (USERGUIDE.md:1174: "`quillo check` takes them as `str`"). The checker still knows the set at each place below, and the set is the nearest thing to an enum Quillo has (USERGUIDE.md:10001).

```quillo
type Leave = "pending" | "approved" | "rejected" | "cancelled"
type Row = { id: int, status: Leave }

fn is_done(r: Row) -> bool = r.status == "aproved"            // always false
fn next(s: Leave) -> Leave = s == "pending" ? "aproved" : s    // fails only when it runs

print(is_done(Row({ id: 1, status: "approved" })))     // false
print(next("pending"))                                 // `next` must return Leave, but returned str "aproved"
```

`quillo check`: `ok (0 routes, 3 functions, 0 components)`.

| Impact | |
|---|---|
| **Functionality** | A misspelled state is a branch that never runs, or a value refused at run time, far from the line that is wrong |
| **User** | Developers find it by a wrong page, not by `quillo check` |
| **Requests** | No change |
| **Security** | A permission check written `role == "admn"` is always false. Here that refuses (safe), but `!=` grants |

**Suggestion:** the checker keeps a set of strings as its own type wherever a value is annotated with it (an argument, a return, a record's field, a `state` initialised from a typed function). It then refuses:

- comparing such a value with a literal outside the set, with ``"aproved" is not a Leave: did you mean "approved"?``;
- returning or passing a literal outside the set.

A value from a database row stays `str` unless a record types it. Make this an error from the start, since a literal outside the set is never right.

### Q-1043. A field that only some members of a union have is read without a word: `len(s.rows)` on an Idle is 0

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` (passes), run time | 1 |

```quillo
type Idle = { phase: "idle" }
type Done = { phase: "done", rows: list<str> }
type Load = Idle | Done

fn rows_of(s: Load) -> int = len(s.rows)     // `rows` is only in Done
print(rows_of({ phase: "idle" }))            // 0
```

| Impact | |
|---|---|
| **Functionality** | Code written for one state runs on the others with `null`, and `len`, `??` and `for` hide it |
| **User** | A page shows "0 rows" in a state that has no rows at all |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** for a value typed as a union, `quillo check` warns on a field that not every member has. Inside `match s { Done as d => ... }` and under `if s.phase == "done"`, the field is known. The warning names the members that have it: ```rows` is only in Done: match `s` first``.

### Q-1044. A `match` over a set of states or a union may miss one, and fails only when that one comes

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run time | 1 |

The guide lists it as not built yet (USERGUIDE.md:10009, 10118), and INDEX.md has no entry for it. With sets and unions as types (Q-1042), the checker can prove the cover for these two cases: every string of the set, or every member of the union, has an arm or there is a `_`.

```quillo
type Leave = "pending" | "approved" | "rejected" | "cancelled"

fn label(s: Leave) -> str = match s {
    "pending" => "Waiting"
    "approved" => "Approved"
    "rejected" => "Rejected"
}
print(label("cancelled"))    // no `match` arm matches "cancelled", at run time
```

| Impact | |
|---|---|
| **Functionality** | A state added to a type later (`"withdrawn"`) breaks every `match` that does not know it, one at a time, as each is reached |
| **User** | A 500 on the first request in the new state |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** exhaustiveness for a `match` whose subject is typed as a set or a union: `` `match` misses "cancelled" of Leave ``. Other `match`es stay as they are.

### Q-1045. A value that is none of a union's members is refused with the first member's reason

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run time | 1 |

```quillo
type Idle = { phase: "idle" }
type Done = { phase: "done", rows: list<str> }
type Load = Idle | Done
fn show(s: Load) -> str = s.phase
show({ phase: "done" })
// argument `s` of `show` must be Load, got map { phase: "done" } (`phase` must be "idle", got str "done")
```

The value is a Done without its `rows`. The message sends the reader to `phase`, which is right.

Writing the union inline, `type Load = { phase: "idle" } | { phase: "done", rows: list }`, is a parse error (`expected end of statement, found |`) that does not say each record must be named first. TypeScript writes them inline.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | Developers fix the wrong field first. The inline form costs a check and a guess |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:**

- When one member's literal fields match (`phase: "done"`), give that member's reason: ``as Done, `rows` is missing``. Name all members only when none matches.
- The parse error for `{ ... } |` in a `type` says: `a record in a union is named first: type Idle = { ... }, then type Load = Idle | ...`.

### Q-1046. A history of rows is an `audit()` call at every write, or a trigger that live pages do not see and that does not know the visitor

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the apps and the guide, a SQLite timing | 1 |

The apps have 33 hand-written `audit(...)` and `insert into activity` writes. The pm app's task history and the cms app's article history are lines of who did what. Neither keeps the old values, so neither can put a version back. Part C.2 has the sketch and its cost.

| Impact | |
|---|---|
| **Functionality** | Each table's history and "put it back" written once, by Quillo, for every database family |
| **User** | Visitors: "History" and "Restore" on records. Developers: no `audit()` to forget at the next write |
| **Requests** | One more row per row changed, written in the same statement: 0.65 to 1.0 µs on SQLite. Nothing for tables not named |
| **Security** | It keeps what was deleted, so `db.forget` and `keep` are part of it, not later. Versions belong to the tenant. `db.restore` is a write, checked by the program's own guards |

**Suggestion:** `db.history(table, { keep })`, `db.versions`, `db.restore` and `db.forget`, as in Part C.2.

### Q-1047. A field set from code drops out of the browser's undo, and the page has nothing to undo with instead

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | browser (headless Chrome) | 1 |

A field bound to state follows the browser's own undo while the person types ("hello" to "hell", and back). After the program sets it (`name = upper(name)`, a Clear button, a value loaded from the server), the browser has no undo for that field, so "hello" cannot come back. That is how browsers treat a value set by script, not Quillo's doing. But it means an app with Clear, Reset, Format or a template has no Undo unless it writes one:

- Four functions (C.1).
- A key handler that leaves fields alone.
- Joining a word typed into one step, since the probe's history took one step per letter.

```quillo
component Field() {
    state name = ""
    <div>
        <input value={name}/>
        <button onclick={() => { name = "" }}>Clear</button>
    </div>
}
// Type "hello", click Clear, focus the field, Cmd+Z (or document.execCommand("undo")): it stays empty.
```

| Impact | |
|---|---|
| **Functionality** | With `undo`, any state can be stepped back and forward, Clear included |
| **User** | Visitors: Ctrl+Z and Cmd+Z work on the page as in a desktop app. Developers: one line per action instead of a history of their own |
| **Requests** | None: the history is in the page. A history that is saved costs the saves the program makes |
| **Security** | Past values stay in the page's memory until it goes. A password or card field kept in a history keeps the number after Clear, so the guide says to keep those out |

**Suggestion:** the `undo` module (Part C.1): `undo.start`, `undo.put` with `join` and `within`, `undo.back`, `undo.forward`, `undo.can_back`, `undo.can_forward` and `undo.keys`.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `take(h.past + [h.now], -100)` to keep the last 100 | Python's `xs[-100:]` | An empty history, silently. The guide says a negative count is 0 for `take`, unlike `slice` (USERGUIDE.md:6992) | 1 browser run |
| `type Load = { phase: "idle" } \| { phase: "done", rows: list }` | TypeScript's unions | `expected end of statement, found \|` (Q-1045) | 1 check |
| `phase = "loading"; rows = load(3); phase = "done"` | The guide's "set a state of your own around the call" | "done" after Cancel (Q-1041) | Written on purpose for the probe. A model would write it first |
| `db.close()` between two timing runs | habit | `` `db` has no function `close` ``: clear, and one database with two tables did instead | 1 check |

## Limitations

- **Records in a union are named first.** There is no inline `{ ... } | { ... }`.
- **Page-wide keys need code.** A key for the whole page needs `onkeydown` on an element with `tabindex`, or a listener through `js()` in an `@mount`. That last way was not tried here.
- **The `crdt` plugin has no undo.** It does not expose `yrs`'s `UndoManager`, so an editor shared through it cannot undo only one person's changes.
- **The DevTools panel has no history.** It shows each state's value now, not the sets that led to it.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| One state holding the phase and its data, cancelled | "idle", 0 rows: the late answer gave way as a whole |
| One state, two loads 300 ms apart | "loading" until the second answer, then its rows |
| A timer under `if phase == "loading"` | Stopped at Cancel, and when the phase left "loading" |
| A history in page state, read and set by two islands | Both followed every Undo and Redo |
| 100 kept steps of a 10,000-row list | 4.1 MB, and 0.13 ms per edit: the rows that did not change were shared |
| A union member missing a field, passed to a typed argument | Refused at the call, the field named (though under the wrong member, Q-1045) |
| A typed function returning a string outside its set | Refused as it returned, with the value |
| The browser's own undo and redo while typing in a bound field | The state followed both |
| Read, check and write with no wait, on one instance | Never moved twice |
| A compare-and-set move, 20 pairs at once | Never moved twice. The loser answered 409 |

## Questions for the user

### 1. Should moves be declared on the type, with `db.move` making each one a compare-and-set?

**Yes, after the checker work. The race in Q-1040 happens whenever a wait sits between the check and the write (20 of 20), or on several instances (4 of 20).**

| Impact | `moves` and `db.move` |
|---|---|
| **Functionality** | A move outside the machine, or one made by someone else first, is refused with the state the row is in. `moves(T, s)` lists the buttons to show. The machine is in one place |
| **User** | Developers: one line per move instead of a read, a check and a write. Visitors: one answer per action |
| **Requests** | One `update` instead of a `select` and an `update`. A refused move costs one `select` more |
| **Security** | The state check cannot be raced. Who-may checks stay the program's |

What is lost: a new word at the top level (`moves`) and its grammar in every editor. A move is no longer plain SQL to read, though its other columns still are.

**My recommendation:** build it after Q-1042 to Q-1045, with the syntax above as a start.

**The question:** shall I plan `moves` and `db.move` as above? If not, I write the compare-and-set pattern for a status into the guide's Database section instead.

### 2. Should a history of rows be part of Quillo (`db.history`), or stay a pattern of triggers and `audit()` calls?

**Part of Quillo, since only Quillo can stamp who made each change, keep live pages following it and make the triggers for each database family. 33 hand-written audit writes in the apps show the need. A trigger costs 0.65 to 1.0 µs more per row changed.**

| Impact | `db.history` |
|---|---|
| **Functionality** | Versions, who and when, and putting one back, for any table named |
| **User** | Visitors get History and Restore on records. Developers stop writing `audit()` |
| **Requests** | One more row per row changed, in the same statement. Nothing for tables not named |
| **Security** | It keeps deleted data, so `db.forget` and `keep` ship with it. Versions are the tenant's |

What is lost: a history table shaped by the program itself, with columns of its own. `db.versions` gives the row as JSON instead.

**My recommendation:** build it last of the four, since it is the largest (three families of triggers, with tests for each).

**The question:** shall `db.history` go in the plan? If not, I add a "History of rows" section to the guide, with a trigger, an `audit()` helper and a `publish` for live pages.

### 3. Should undo in the page be a builtin `undo` module of pure functions, or a pattern in the guide only?

**A builtin module. Written by hand, undo is four lines of logic, but the parts around it are what people get wrong: joining typed letters into one step, Ctrl+Z leaving fields alone, and the limit. The browser cannot undo a field set from code (Q-1047).**

| Impact | The `undo` module |
|---|---|
| **Functionality** | Undo and redo of any state, with steps joined per field and a limit |
| **User** | Visitors: Ctrl+Z and Cmd+Z work. Developers: one call per action |
| **Requests** | None |
| **Security** | Past values stay in memory: the guide says to keep password and card fields out |

What is lost: core grows, by an estimated 150 lines with tests (an estimate, not counted). A hand-written history may still do as it likes.

**My recommendation:** the module, now. A runtime-level `state x = v keep 100`, walked by the DevTools panel too, can come later if apps ask for it.

**The question:** shall I plan the `undo` module? If not, the four functions and a key handler go in the guide as a pattern.
