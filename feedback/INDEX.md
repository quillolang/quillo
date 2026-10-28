# Known issues

Every edge case found by building apps in Quillo, once each. Read this before
writing a new report: an issue already here gets "seen again" (add the app to
**Seen**), not a new entry. A new one gets the next ID, a row here, and a repro in
[repros/](repros) when it can be run.

`node feedback/run.mjs --update` runs every repro against the current build and
rewrites **Status**. Rows without a repro are kept up to date by hand.

**Kind** is how the problem shows itself, which is what it costs whoever writes the code:

| Kind | Means |
|---|---|
| silent | wrong output, no error: the costliest, found only by testing |
| loud | an error, but a misleading one, or at the wrong time (run time, not `quillo check`) |
| perf | right, but too slow at a size an app reaches |
| docs | the guide says something wrong, or leaves out something people need |
| limitation | missing by design or not yet built: costs a workaround |

| ID | Issue | Kind | Found | Seen | Status | Repro |
|---|---|---|---|---|---|---|
| Q-001 | A link to a spot on the page scrolls to the top once a component has read `url()` | silent | 9abef4a, [BUGREPORT1](../BUGREPORT1.md) | forum | fixed (fe10f8c) | [repro](repros/Q-001-hash-link-after-url.qwe) |
| Q-002 | A refused prefetch is answered 204, which Chrome takes as the page: links to routes that write do nothing | silent | f1695a3, [FEEDBACK §1](../FEEDBACK.md) | forum | fixed (fe10f8c) | [repro](repros/Q-002-refused-prefetch-is-2xx.qwe) |
| Q-003 | Each refused prefetch is logged as an error | loud | f1695a3, [FEEDBACK §1](../FEEDBACK.md) | forum | fixed (fe10f8c) | [repro](repros/Q-003-refused-prefetch-logged-as-error.qwe) |
| Q-004 | A second definition of a name silently replaces the first | silent | f1695a3, [FEEDBACK §2](../FEEDBACK.md) | forum | fixed (fe10f8c) | [repro](repros/Q-004-name-defined-twice) |
| Q-005 | A function named like a builtin replaces it everywhere, without a warning | silent | f1695a3, [FEEDBACK §2](../FEEDBACK.md) | forum, cms | fixed (fe10f8c) | [repro](repros/Q-005-fn-hides-builtin.qwe) |
| Q-006 | `use` underdocumented; the language server checked each file alone (red `unknown name` everywhere) | docs | f1695a3, [FEEDBACK §3](../FEEDBACK.md) | forum | fixed in the guide ("Files of a program"); editor not rechecked | – |
| Q-007 | `quillo check` counts only the main file | loud | f1695a3, [FEEDBACK §4](../FEEDBACK.md) | forum | fixed (fe10f8c) | [repro](repros/Q-007-check-counts-main-file) |
| Q-008 | The "code written for a newer Quillo" note follows every parse error, typos too | loud | f1695a3, [FEEDBACK §5](../FEEDBACK.md) | forum, pm, cms ×2 | open (fe10f8c) | [repro](repros/Q-008-version-note-on-every-parse-error.qwe) |
| Q-009 | A component cannot set a page object's field (`el.value = ...`) | limitation | f1695a3, [FEEDBACK §6](../FEEDBACK.md) | forum | fixed (fe10f8c) | [repro](repros/Q-009-set-element-property.qwe) |
| Q-010 | Guide leaves out `e.preventDefault()`, and writing in a loader when live data arrives | docs | f1695a3, [FEEDBACK §7](../FEEDBACK.md) | forum | fixed in the guide (Components; Live data) | – |
| Q-011 | A component that is a table row (or `<option>`) shows twice: dead rows in the table, live ones above it | silent | 896e758, [FEEDBACK2 §1](../FEEDBACK2.md) | pm, cms (avoided) | fixed (fe10f8c) | [repro](repros/Q-011-table-row-component-twice.qwe) |
| Q-012 | `let`/`for` silently do not bind names like `int`, `str`, `num`, `list` | silent | 896e758, [FEEDBACK2 §2](../FEEDBACK2.md) | pm ×2, cms | fixed (fe10f8c) | [repro](repros/Q-012-let-type-name.qwe) |
| Q-013 | A whole float is `8.0` on the server, `8` in the browser: the component is made again on load | silent | 896e758, [FEEDBACK2 §3](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-013-whole-float-text-differs.qwe) |
| Q-014 | A component made again on load is reported as a bare `quillo: text`, or not at all | loud | 896e758, [FEEDBACK2 §3](../FEEDBACK2.md) | pm | open (fe10f8c) | [repro](repros/Q-014-mismatch-not-reported.qwe) |
| Q-015 | A parameter named `key` collides with a list's `key`; `quillo check` says nothing | loud | 896e758, [FEEDBACK2 §4](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-015-key-parameter.qwe) |
| Q-016 | Run-time errors name a line but not the file | loud | 896e758, [FEEDBACK2 §5](../FEEDBACK2.md) | pm ×2 | fixed (fe10f8c) | [repro](repros/Q-016-runtime-error-no-file) |
| Q-017 | `{ ...m, k: v }` (shown in the guide) and `[...xs]` do not parse | docs | 896e758, [FEEDBACK2 §6](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-017-spread-in-literals.qwe) |
| Q-018 | A map grown with `set` in a loop costs the square of its size (20,000 sets: 1 s) | perf | 896e758, [FEEDBACK2 §7](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-018-set-in-a-loop-is-quadratic.qwe) |
| Q-019 | `set(m, 5, x)` works, `m[5]` is an error; `group_by` turns int keys into text | loud | 896e758, [FEEDBACK2 §8](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-019-map-int-index.qwe) |
| Q-020 | `return` is not an expression (match arm, after `??`), though `reply` is | loud | 896e758, [FEEDBACK2 §9](../FEEDBACK2.md) | pm ×2, cms | fixed (fe10f8c) | [repro](repros/Q-020-return-not-an-expression.qwe) |
| Q-021 | `return` inside `for … =>` leaves the function; the guide says it "leaves a loop early" | docs | 896e758, [FEEDBACK2 §9](../FEEDBACK2.md) | pm | fixed in the guide (fe10f8c) | – |
| Q-022 | `request` cannot be read in a helper, though `session()` can | limitation | 896e758, [FEEDBACK2 §10](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-022-request-only-in-routes.qwe) |
| Q-023 | No way to change one item of a list (`set` takes only maps) | limitation | 896e758, [FEEDBACK2 §10](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-023-no-list-item-update.qwe) |
| Q-024 | `flatten` is one level, and the checker cannot follow nested list shapes | docs | 896e758, [FEEDBACK2 §10](../FEEDBACK2.md) | pm | fixed in the guide (fe10f8c) | – |
| Q-025 | `db.open` with a relative path opens it in the program's folder, and does not say where | silent | 896e758, [FEEDBACK2 §10](../FEEDBACK2.md) | pm | fixed (fe10f8c) | [repro](repros/Q-025-database-path-not-shown.qwe) |
| Q-026 | `raw(...)` in a component is wrapped in `<quillo-raw>`, which the guide does not mention | docs | 896e758, [FEEDBACK2 §10](../FEEDBACK2.md) | pm | fixed in the guide (fe10f8c) | – |
| Q-027 | No date module since 0.5.0, and no time zones: "today" is UTC's | limitation | 896e758, [FEEDBACK2](../FEEDBACK2.md) (Limitations) | pm, cms | open | – |
| Q-028 | Live data sends the loader's whole value again, and runs the loader once per open page | perf | 896e758, [FEEDBACK2](../FEEDBACK2.md) (Limitations) | pm | open | – |
| Q-029 | SQL from `sql_name(…)` warned about as injection when kept in a variable (made from a template) or returned by a helper; `sql_param(list)` undocumented | loud | 896e758, [FEEDBACK3 §Q-029](../FEEDBACK3.md) | cms ×14 | open (fe10f8c) | [repro](repros/Q-029-sql-name-in-variable-warned.qwe) |
| Q-030 | A `<textarea>` whose text is its children stops following state once someone types | silent | 896e758, [FEEDBACK3 §Q-030](../FEEDBACK3.md) | cms (probe) | open (fe10f8c) | [repro](repros/Q-030-textarea-children-stop-following.qwe) |
| Q-031 | No comments inside markup: `// note` there is page text; no `{/* */}`, no block comments at all | silent | 896e758, [FEEDBACK3 §Q-031](../FEEDBACK3.md) | cms | open (fe10f8c) | [repro](repros/Q-031-no-comments-in-markup.qwe) |
| Q-032 | Markup the HTML parser restructures (a `<div>` in a `<p>`) is sent as written; the component is made again on load | silent | 896e758, [FEEDBACK3 §Q-032](../FEEDBACK3.md) | cms | open (fe10f8c) | [repro](repros/Q-032-invalid-nesting-remade.qwe) |
| Q-033 | A parse error in a used file is reported after the "unknown name" errors it causes elsewhere | loud | 896e758, [FEEDBACK3 §Q-033](../FEEDBACK3.md) | cms | open (fe10f8c) | [repro](repros/Q-033-parse-error-reported-after-its-effects) |
| Q-034 | A keyword as a state's name (`state reply`) gives "expected end of statement", not "is a keyword" | loud | 896e758, [FEEDBACK3 §Q-034](../FEEDBACK3.md) | cms | open (fe10f8c) | [repro](repros/Q-034-keyword-as-state-name.qwe) |
| Q-035 | A top-level `let int` in one file takes the builtin `int()` from every file; no warning where it is defined, and the error where it is called does not say where | loud | fe10f8c, review of Q-012's fix | – | open (fe10f8c) | [repro](repros/Q-035-top-level-let-hides-builtin) |
| Q-036 | Spread's errors name its internals: `__merge` for a list spread into a map, "cannot add list and null" for `[...null]` | loud | fe10f8c, review of Q-017's fix | – | open (fe10f8c) | [repro](repros/Q-036-spread-errors-leak-internals.qwe) |
