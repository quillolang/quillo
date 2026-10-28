# Feedback from building a newsroom CMS in Quillo

| | |
|---|---|
| App | [cms/](cms), "Newsroom": 3,274 lines in 20 files (17 of Quillo). Articles made of blocks move through a flow of steps, each controlled by a role (writer, reviewer, publisher); a step can need several approvals. Reviewers give feedback on single blocks, live, from several desks at once; the author answers; reviewers close it. Designers draw flows, and make objects (tables created at run time) with fields, a state machine, create forms, transaction forms (the only way a record changes state) and lists, then build desk pages from those blocks. |
| Quillo | 0.5.0 (896e758) |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; headless Chrome with three desks signed in as different people at once; `feedback/tools/mismatches.mjs` on 17 pages for three roles; hostile and simultaneous server-fn calls |

What worked well: live data carried the whole collaborative part. Each article has topics of
its own (`article:<id>`, `threads:<id>`, `presence:<id>`), so a presence heartbeat every 10
seconds wakes only the avatars, and a reply in one thread wakes only that article's desks.
A function passed as a prop that returns markup (each block's gutter) followed the parent's
state: when rex focused a block, his avatar appeared on it on wanda's screen. Tables made at
run time needed nothing special. Live data followed writes to `obj_<id>` through
`sql_name`, and `alter table … add column` / `drop column` worked with records in place.
`sql_name(list)` with `sql_param(list)` and a map makes the insert of a run-time set of
columns clean. A `db.transaction` with `where state = :from` let exactly one of three
simultaneous transactions on one record through. `value={…}` on a `<textarea>` is
written by the server as the textarea's text, so the page shows it before any script.
`quillo check` caught a function of mine named `has` hiding the builtin (Q-005's fix at
work), and `--watch` kept serving the last good version when an edit broke the program.

## Seen again

Known issues this app ran into (see [feedback/INDEX.md](feedback/INDEX.md)):

| ID | Where it bit | Attempts |
|---|---|---|
| Q-012 | `let html = match b.type {…}` in lib/blocks.qwe: `unknown name html`. A third type name after `num` and `int` | 1 |
| Q-008 | the "newer Quillo" note after `state reply = ""` and after `{/* note */}` in markup | 2 |
| Q-020 | `let u = db.one(…) ?? return null` was not possible; my first workaround (`?? return_null()`) was wrong code | 1 |
| Q-011 | avoided: the flow designer's transitions and the object designer's fields are rows of divs, not `<tr>` components | 0 |
| Q-027 | a "date" field needs day checks and labels: 30 more lines of calendar code in lib/text.qwe | – |
| Q-005 | fixed, and useful: `fn has(u, role)` was reported as hiding the builtin `has` | 1 |

## New issues

### Q-032. Markup the browser cannot keep as written (a `<div>` in a `<p>`) goes out as it is, and the component is made again

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `feedback/tools/mismatches.mjs` (a debugger breakpoint in the runtime) | 3 |

The article room, the app's main screen, was made again on every load (`__quilloRemade` 1,
for every article and every user). The cause was a `<p class="byline">` holding a
component (`<Presence>`) whose root is a `<div>`. The HTML parser closes a `<p>` before a
`<div>`, so the page's tree is not what the server wrote. Taking it up fails ("element: want
div, have null"), and the browser makes the component from scratch. The console said
`quillo: element`; nothing else did.

```quillo
component Byline(name: str) {
    state n = 0
    <p>By {name} <div class="presence"><button @click={() => { n += 1 }}>{n}</button></div></p>
}
route GET / = <html><body><Byline name="wanda"/></body></html>
```

The mistake was mine, but it is the same family as Q-011: what the server writes is HTML
that the parser restructures (a `<div>` in a `<p>`; a `<quillo-island>` in a `<tbody>`;
also `<a>` in `<a>`, `<form>` in `<form>`, `<li>` outside a list, `<p>` in a `<p>`). In
JSX the browser builds the tree itself, so a server-rendered framework meets this where
React does not. React warns about these ("validateDOMNesting").

**Suggestion:** `quillo check` knows the elements and where they sit, including a view's or
component's root: report a `<div>`, `<section>`, `<ul>` (any flow content) in a `<p>`,
`<a>` in `<a>`, and the table and select cases of Q-011, as errors. At least, when resuming
fails in `--watch`, print the component and both sides (see Q-014).

### Q-031. There is no way to comment inside markup; `// note` there is shown to readers

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a parse error, by luck | 2 |

```quillo
view Card() = <div>
    // a note for whoever edits this
    <p>x</p>
</div>
```

renders `<div>// a note for whoever edits this<p>x</p></div>`: the note is page text. My
note in ui/article.qwe happened to mention `<Presence>`, which opened a tag and broke the
parse, or it would have shipped. `{/* note */}` (JSX's way) and `{// note}` are parse
errors, and the language has no block comments at all (`1 /* c */ + 1` fails too). So a
comment about a piece of markup has to go above the whole expression, often far away.

**Suggestion:** treat a line in markup that starts with `//` as a comment (it is almost
never meant as text; `{"//"}` writes it), or support `{/* … */}`. Failing both, have
`quillo check` warn about text in markup that starts with `//`.

### Q-030. A `<textarea>` whose text is its children stops following state once someone types

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe before building | 1 |

```quillo
component Note() {
    state text = "first"
    <div>
        <textarea @input={(e) => { text = e.value }}>{text}</textarea>
        <button @click={() => { text = "reset" }}>Reset</button>
    </div>
}
```

After typing, Reset sets `text` but the box keeps showing what was typed: children are
HTML's default value, taken once. `value={text}` works fully (the server even writes it as
the textarea's text), but nothing says the children form is wrong. A block editor clears and
reloads boxes all the time; this would have shown stale text after every save conflict.

**Suggestion:** take a textarea's children as its `value` (both sides already know the
element), or have `quillo check` say "write `value={…}`".

### Q-029. SQL from `sql_name(…)` is warned about as injection when the name is in a variable or made by a helper

| Kind | Detected by | Attempts |
|---|---|---|
| loud (false) | `quillo check` | – |

All 14 of `quillo check cms/main.qwe`'s warnings are this, and all are false:

```quillo
fn table_of(id: int) -> str = sql_name(`obj_${id}`)
fn count_of(id: int) = db.one(`select count(*) as n from ${table_of(id)}`)    // warned

fn rows_of_object(id: int) {
    let table = sql_name(`obj_${id}`)
    db.query(`select * from ${table}`)                                          // warned
}

fn rows_of(name: str) {
    let table = sql_name(name)
    db.query(`select * from ${table}`)                                          // not warned
}
```

The guide says SQL made of `sql_name` "(also kept in a variable)" is not warned about. It
is, when `sql_name` is given a template string, or when a helper returns its result. A
program whose tables are made at run time names them everywhere, so the warnings bury any
real one. A developer learns to skip the list, or inlines `sql_name(`obj_${id}`)` in 14
places.

Also: `sql_param(list)` gives `:a, :b` and makes `insert into t (${sql_name(cols)}) values
(${sql_param(cols)})` with a map clean. That is the way to write a run-time set of columns,
and the guide does not mention that `sql_param` takes a list.

**Suggestion:** treat a variable bound to `sql_name(…)` (whatever its argument), and a
function whose every return is `sql_name(…)` (or that is annotated to return one, say
`-> sql`), as safe. Document `sql_param(list)` and the row-value update
`set (${sql_name(cols)}) = (${sql_param(cols)})`.

### Q-033. A parse error in a used file is reported after the errors it causes

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

```
main.qwe:82: error: unknown name `ArticleRoom` (did you mean `ArticleRow`?)
./ui/article.qwe:29: error: expected end of statement, found `reply`
```

The first line sends you to main.qwe, which is fine; the cause is the second. The suggestion
(`ArticleRow`) makes it worse: it suggests renaming a correct reference.

**Suggestion:** report parse errors first, and leave out "unknown name" for names that a
file which failed to parse may define.

### Q-034. A keyword used as a state's name gives "expected end of statement"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

`state reply = ""` gives `expected end of statement, found reply`, with the version note.
`fn reply()` now says `reply` is a keyword (from FEEDBACK.md); `state`, `let` and
parameters should say the same. A reply box is the obvious place for a variable named
`reply`.

## Where the AI struggled

What I wrote first, in order, including mistakes that were mine and not Quillo's:

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `fn has(u, role)` for "has this role" | the natural name | `quillo check`: hides the builtin `has`. Good | 1 check |
| renaming it with `sed 's/\bhas(u, /…/'` | shell habit | macOS sed has no `\b`: five calls stayed `has(u, "admin")`, which now called the builtin `has(map, key)` and returned false. `quillo check` passed: `u` is an open map | found by grep; admins would have been silently refused |
| `let html = match …` | the natural name | `unknown name html` (Q-012) | 1 check |
| `state reply = ""` | the natural name | a parse error, the version note, and an unrelated "unknown name" first (Q-033, Q-034) | 1 check |
| `?? return_null()` to leave early | `?? return` is not allowed (Q-020) | wrong code: `merge(null, …)` later; caught rereading | 0 runs |
| `<p.byline>` around `<Presence>` (a div) | JSX habit: React builds the tree, so nesting never mattered | the main screen made again on every load (Q-032) | 3 scans and a runtime breakpoint |
| `// why this is a div` inside markup | habit from code | a parse error, because the comment contained `<Presence>`; then `--watch` kept serving the old version, and one scan tested stale code | 2 runs |
| `<textarea>{draft.md}</textarea>` | HTML habit | caught by a probe before building (Q-030) | 1 probe |
| `table_of(id)` returning `sql_name(…)` | not repeating myself | 14 false warnings (Q-029) | – |
| test selector `.step-card:nth-child(5)` | CSS habit | each component is wrapped in a `<quillo-island>`, so every card is a first child | 1 run |
| a save conflict tested with two tabs of one browser | test habit | a background tab gets no `focusout`, so nothing saved; two browsers were needed (my harness, not Quillo) | 1 run |

What went right on the first try: tables made at run time with live data on them, presence
and three simultaneous desks, the flow engine, approvals and rounds, drag and drop of
steps, the transaction race. Probing first (run-time DDL with live data, `every` calling a
server fn, `@focusin`, textareas, `sql_name`/`sql_param` with lists) steered the design
away from two of the traps above before any of the app was written.

## Limitations

- **No date module** (Q-027): a "date" field type needs its own check (real days only) and
  labels; 30 lines in lib/text.qwe, a second copy of what pm/lib/dates.qwe has.
- **State is read from a prop once.** A block's editor keeps its draft in state, so another
  desk's save of the same block does not show in it. The save is refused (versions), and
  the draft is kept for copying. Giving the editor a `key` that changes with the block's
  version would throw away what is being typed. A way to know a prop changed under a
  state that came from it would help.
- **Presence is hand-made**: a table, a heartbeat every 10 seconds, and a topic. Someone who
  closes the tab stays on the others' screens until another desk's heartbeat finds them
  30 seconds quiet and clears them, so up to about 40 seconds.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A reviewer opens feedback on a block | the author's desk shows the count on that block and the thread, within a second |
| The author answers; then tries to close the thread (button hidden, so through the server fn) | the reply shows on the reviewer's desk; closing is refused: "Only reviewers and publishers…" |
| Approve while feedback is open | disabled, with "3 open threads first"; the server refuses too |
| Approving twice; the author approving their own article through the server fn | "you approved"; "Approve: for reviewers" |
| Two approvals needed at Review | the first leaves it at Review ("1 of 2"), the second moves it to Approval on every desk |
| Unpublish without a reason; with one | refused; back to Draft, round 2, approvals cleared, the public page 404 |
| Feedback on a published article | refused |
| Two desks save the same block | the first is saved, the second refused, its text kept in the box |
| Editing after submitting; editing someone else's draft; a form block in an article | all refused by the server |
| Unknown block type; unknown fields in a block's data | refused; dropped |
| Each wrong kind of field value (missing, not a choice, 2026-02-30, "12,50", no such person) | refused, naming the field |
| A create form's input with `state: 99` and `f_999` | ignored: the record starts at the flow's first step |
| A transaction by the wrong role; run twice; three at once | refused; "not at Requested any more"; exactly one runs, one history entry |
| Deleting a field that records hold; changing a field's type | the column is dropped, lists and forms follow; refused |
| Deleting a step something is at; reordering with a stale list; a transition to another flow's step | refused, each with a reason |
| Taking away one's own admin role | refused |
| Every page, as admin, designer and reviewer | resumed, not remade (`__quilloRemade` 0), after Q-032's fix |
