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
| Q-001 | A link to a spot on the page scrolls to the top once a component has read `url()` | silent | 9abef4a, [BUGREPORT1](reports/BUGREPORT1.md) | forum | fixed (ae6a6e5) | [repro](repros/Q-001-hash-link-after-url.qwe) |
| Q-002 | A refused prefetch is answered 204, which Chrome takes as the page: links to routes that write do nothing | silent | f1695a3, [FEEDBACK §1](reports/FEEDBACK.md) | forum | fixed (ae6a6e5) | [repro](repros/Q-002-refused-prefetch-is-2xx.qwe) |
| Q-003 | Each refused prefetch is logged as an error | loud | f1695a3, [FEEDBACK §1](reports/FEEDBACK.md) | forum | fixed (ae6a6e5) | [repro](repros/Q-003-refused-prefetch-logged-as-error.qwe) |
| Q-004 | A second definition of a name silently replaces the first | silent | f1695a3, [FEEDBACK §2](reports/FEEDBACK.md) | forum | fixed (ae6a6e5) | [repro](repros/Q-004-name-defined-twice) |
| Q-005 | A function named like a builtin replaces it everywhere, without a warning | silent | f1695a3, [FEEDBACK §2](reports/FEEDBACK.md) | forum, cms | fixed (ae6a6e5) | [repro](repros/Q-005-fn-hides-builtin.qwe) |
| Q-006 | `use` underdocumented; the language server checked each file alone (red `unknown name` everywhere) | docs | f1695a3, [FEEDBACK §3](reports/FEEDBACK.md) | forum, stays | fixed in the guide ("Files of a program"); the editor still checks each file alone (ae6a6e5) | – |
| Q-007 | `quillo check` counts only the main file | loud | f1695a3, [FEEDBACK §4](reports/FEEDBACK.md) | forum | fixed (ae6a6e5) | [repro](repros/Q-007-check-counts-main-file) |
| Q-008 | The "code written for a newer Quillo" note follows every parse error, typos too | loud | f1695a3, [FEEDBACK §5](reports/FEEDBACK.md) | forum, pm, cms ×2, bank, stays | open (ae6a6e5) | [repro](repros/Q-008-version-note-on-every-parse-error.qwe) |
| Q-009 | A component cannot set a page object's field (`el.value = ...`) | limitation | f1695a3, [FEEDBACK §6](reports/FEEDBACK.md) | forum | fixed (ae6a6e5) | [repro](repros/Q-009-set-element-property.qwe) |
| Q-010 | Guide leaves out `e.preventDefault()`, and writing in a loader when live data arrives | docs | f1695a3, [FEEDBACK §7](reports/FEEDBACK.md) | forum | fixed in the guide (Components; Live data) | – |
| Q-011 | A component that is a table row (or `<option>`) shows twice: dead rows in the table, live ones above it | silent | 896e758, [FEEDBACK2 §1](reports/FEEDBACK2.md) | pm, cms (avoided) | fixed (ae6a6e5) | [repro](repros/Q-011-table-row-component-twice.qwe) |
| Q-012 | `let`/`for` silently do not bind names like `int`, `str`, `num`, `list` | silent | 896e758, [FEEDBACK2 §2](reports/FEEDBACK2.md) | pm ×2, cms | fixed (ae6a6e5) | [repro](repros/Q-012-let-type-name.qwe) |
| Q-013 | A whole float is `8.0` on the server, `8` in the browser: the component is made again on load | silent | 896e758, [FEEDBACK2 §3](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-013-whole-float-text-differs.qwe) |
| Q-014 | A component made again on load is reported as a bare `quillo: text`, or not at all | loud | 896e758, [FEEDBACK2 §3](reports/FEEDBACK2.md) | pm | open (ae6a6e5) | [repro](repros/Q-014-mismatch-not-reported.qwe) |
| Q-015 | A parameter named `key` collides with a list's `key`; `quillo check` says nothing | loud | 896e758, [FEEDBACK2 §4](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-015-key-parameter.qwe) |
| Q-016 | Run-time errors name a line but not the file | loud | 896e758, [FEEDBACK2 §5](reports/FEEDBACK2.md) | pm ×2 | fixed (ae6a6e5) | [repro](repros/Q-016-runtime-error-no-file) |
| Q-017 | `{ ...m, k: v }` (shown in the guide) and `[...xs]` do not parse | docs | 896e758, [FEEDBACK2 §6](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-017-spread-in-literals.qwe) |
| Q-018 | A map grown with `set` in a loop costs the square of its size (20,000 sets: 1 s) | perf | 896e758, [FEEDBACK2 §7](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-018-set-in-a-loop-is-quadratic.qwe) |
| Q-019 | `set(m, 5, x)` works, `m[5]` is an error; `group_by` turns int keys into text | loud | 896e758, [FEEDBACK2 §8](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-019-map-int-index.qwe) |
| Q-020 | `return` is not an expression (match arm, after `??`), though `reply` is | loud | 896e758, [FEEDBACK2 §9](reports/FEEDBACK2.md) | pm ×2, cms | fixed (ae6a6e5) | [repro](repros/Q-020-return-not-an-expression.qwe) |
| Q-021 | `return` inside `for … =>` leaves the function; the guide says it "leaves a loop early" | docs | 896e758, [FEEDBACK2 §9](reports/FEEDBACK2.md) | pm | fixed in the guide (fe10f8c) | – |
| Q-022 | `request` cannot be read in a helper, though `session()` can | limitation | 896e758, [FEEDBACK2 §10](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-022-request-only-in-routes.qwe) |
| Q-023 | No way to change one item of a list (`set` takes only maps) | limitation | 896e758, [FEEDBACK2 §10](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-023-no-list-item-update.qwe) |
| Q-024 | `flatten` is one level, and the checker cannot follow nested list shapes | docs | 896e758, [FEEDBACK2 §10](reports/FEEDBACK2.md) | pm | fixed in the guide (fe10f8c) | – |
| Q-025 | `db.open` with a relative path opens it in the program's folder, and does not say where | silent | 896e758, [FEEDBACK2 §10](reports/FEEDBACK2.md) | pm | fixed (ae6a6e5) | [repro](repros/Q-025-database-path-not-shown.qwe) |
| Q-026 | `raw(...)` in a component is wrapped in `<quillo-raw>`, which the guide does not mention | docs | 896e758, [FEEDBACK2 §10](reports/FEEDBACK2.md) | pm | fixed in the guide (fe10f8c) | – |
| Q-027 | No date module since 0.5.0, and no time zones: "today" is UTC's | limitation | 896e758, [FEEDBACK2](reports/FEEDBACK2.md) (Limitations) | pm, cms, bank, stays | open | – |
| Q-028 | Live data sends the loader's whole value again, and runs the loader once per open page | perf | 896e758, [FEEDBACK2](reports/FEEDBACK2.md) (Limitations) | pm, bank, stays | open | – |
| Q-029 | SQL from `sql_name(…)` warned about as injection when kept in a variable (made from a template) or returned by a helper; `sql_param(list)` undocumented | loud | 896e758, [FEEDBACK3 §Q-029](reports/FEEDBACK3.md) | cms ×14 | fixed (ae6a6e5) | [repro](repros/Q-029-sql-name-in-variable-warned.qwe) |
| Q-030 | A `<textarea>` whose text is its children stops following state once someone types | silent | 896e758, [FEEDBACK3 §Q-030](reports/FEEDBACK3.md) | cms (probe) | fixed (ae6a6e5) | [repro](repros/Q-030-textarea-children-stop-following.qwe) |
| Q-031 | No comments inside markup: `// note` there is page text; no `{/* */}`, no block comments at all | silent | 896e758, [FEEDBACK3 §Q-031](reports/FEEDBACK3.md) | cms | fixed (ae6a6e5) | [repro](repros/Q-031-no-comments-in-markup.qwe) |
| Q-032 | Markup the HTML parser restructures (a `<div>` in a `<p>`) is sent as written; the component is made again on load | silent | 896e758, [FEEDBACK3 §Q-032](reports/FEEDBACK3.md) | cms | fixed (ae6a6e5) | [repro](repros/Q-032-invalid-nesting-remade.qwe) |
| Q-033 | A parse error in a used file is reported after the "unknown name" errors it causes elsewhere | loud | 896e758, [FEEDBACK3 §Q-033](reports/FEEDBACK3.md) | cms | fixed (ae6a6e5) | [repro](repros/Q-033-parse-error-reported-after-its-effects) |
| Q-034 | A keyword as a state's name (`state reply`) gives "expected end of statement", not "is a keyword" | loud | 896e758, [FEEDBACK3 §Q-034](reports/FEEDBACK3.md) | cms | fixed (ae6a6e5) | [repro](repros/Q-034-keyword-as-state-name.qwe) |
| Q-035 | A top-level `let int` in one file takes the builtin `int()` from every file; no warning where it is defined, and the error where it is called does not say where | loud | fe10f8c, review of Q-012's fix | – | open (ae6a6e5) | [repro](repros/Q-035-top-level-let-hides-builtin) |
| Q-036 | Spread's errors name its internals: `__merge` for a list spread into a map, "cannot add list and null" for `[...null]` | loud | fe10f8c, review of Q-017's fix | – | open (ae6a6e5) | [repro](repros/Q-036-spread-errors-leak-internals.qwe) |
| Q-037 | A builtin Quillo removed (`rate_limit`, `date.*`, in 205d7cd) is reported as `unknown name`, and `date` as "did you mean `take`?": nothing says it was removed or what to do instead | loud | 1cf047c, [FEEDBACK §8](reports/FEEDBACK.md) | forum | open (ae6a6e5) | [repro](repros/Q-037-removed-builtin-unknown-name.qwe) |
| Q-038 | `publish()` in a service reaches the app's pages in one process, and silently nobody once `--split` | silent | 5d2e21e, [FEEDBACK4 §Q-038](reports/FEEDBACK4.md) | bank | open (ae6a6e5) | [repro](repros/Q-038-publish-in-a-service-lost-when-split) |
| Q-039 | A live loader that gets its data from a service follows only the service's name: the page never updates, without a word | silent | 5d2e21e, [FEEDBACK4 §Q-039](reports/FEEDBACK4.md) | bank | open (ae6a6e5) | [repro](repros/Q-039-live-loader-calling-a-service-follows-nothing) |
| Q-040 | A `service` declared before a `use` of a file the service also uses: the program does not start (the browser code lacks the file's functions); `quillo check` passes | loud | 5d2e21e, [FEEDBACK4 §Q-040](reports/FEEDBACK4.md) | bank | open (ae6a6e5) | [repro](repros/Q-040-service-before-use-breaks-browser-code) |
| Q-041 | `cond && <markup>` makes its elements again whenever anything it reads changes, so inputs in it lose focus and what was typed (`? :` and `if` keep them) | silent | 5d2e21e, [FEEDBACK4 §Q-041](reports/FEEDBACK4.md) | bank | open (ae6a6e5) | [repro](repros/Q-041-and-in-markup-remakes-its-elements.qwe) |
| Q-042 | `--split` does not start again a service that died: the app stays without it until the whole app is restarted | limitation | 5d2e21e, [FEEDBACK4 §Q-042](reports/FEEDBACK4.md) | bank | open | – |
| Q-043 | A program with a service logs `POST /_quillo/later -> 200` about once a second, idle | loud | 5d2e21e, [FEEDBACK4 §Q-043](reports/FEEDBACK4.md) | bank | open (ae6a6e5) | [repro](repros/Q-043-later-poll-logged-every-second) |
| Q-044 | `quillo check` lets `n / 4` (always a float) through where a builtin takes an int (`range`): an error only at run time | loud | 5d2e21e, [FEEDBACK4 §Q-044](reports/FEEDBACK4.md) | bank | open (ae6a6e5) | [repro](repros/Q-044-int-divided-passed-as-int.qwe) |
| Q-045 | A top-level `let x = 0.12m` is a dec on the server and the text "0.12" in browser code (also in a top-level list or map): `money.mul` fails there, only the console says so | silent | ae6a6e5, [FEEDBACK5 §Q-045](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-045-top-level-dec-is-text-in-the-browser.qwe) |
| Q-046 | A component that runs by itself (`@mount`, `live`, `every`) inside another component does not start when the page is idle: only once someone touches the outer one | silent | ae6a6e5, [FEEDBACK5 §Q-046](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-046-mount-in-a-nested-component-waits-for-a-touch.qwe) |
| Q-047 | Under `--watch`, markup kept in a top-level `let` compiles to `59return W.t(...)`: a SyntaxError, and every component showing it is dead (the UI kit's `<Rating>`) | loud | ae6a6e5, [FEEDBACK5 §Q-047](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-047-markup-in-a-top-level-let-breaks-watch-mode.qwe) |
| Q-048 | A range input whose value the browser corrects (snapped to its step) is taken on resume for typing: its `@input` runs with nobody touching it | silent | ae6a6e5, [FEEDBACK5 §Q-048](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-048-a-value-the-browser-corrects-is-replayed-as-typing.qwe) |
| Q-049 | `http.get` ignores option keys it does not know (`timeout`), and cannot give up sooner than its own 10 s to connect and 60 s in all, which the guide does not give | limitation | ae6a6e5, [FEEDBACK5 §Q-049](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-049-http-options-unknown-key-ignored.qwe) |
| Q-050 | A Quillo function handed to a JavaScript library throws when called with more arguments than it declares (Leaflet's `on("click", () => ...)`) | silent | ae6a6e5, [FEEDBACK5 §Q-050](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-050-js-calling-back-with-extra-arguments-throws.qwe) |
| Q-051 | `xs \| find(f) != null` is `xs \| (find(f) != null)`: three errors about `find`'s arguments ("cannot call `this function`"), none about precedence | loud | open (ae6a6e5) | stays | open (ae6a6e5) | [repro](repros/Q-051-pipe-binds-looser-than-comparison.qwe) |
| Q-052 | An address `public_only` refuses is reported as "Dns Failed: resolve dns name '127.0.0.1:9'" | loud | ae6a6e5, [FEEDBACK5 §Q-052](reports/FEEDBACK5.md) | stays | open (ae6a6e5) | [repro](repros/Q-052-refused-private-address-said-as-dns-failure.qwe) |
| Q-053 | A database error is only its text: a unique key's failure is told from an outage by matching words that differ by database | limitation | ae6a6e5, [FEEDBACK5 §Q-053](reports/FEEDBACK5.md) | stays | open | [repro](repros/Q-053-database-error-is-only-text.qwe) |
| Q-054 | `js(...)` of a library the page does not have stops the handler with a console error: no error value to check | loud | e045b8b, [FEEDBACK5 §Q-054](reports/FEEDBACK5.md) | stays (Quillo UI's MapView) | open (e045b8b) | [repro](repros/Q-054-js-of-a-missing-library-stops-the-handler.qwe) |
