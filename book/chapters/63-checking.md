# Checking

`quillo check app.qwe` finds mistakes without running the program, and `quillo run`
does the same first. Types are sets of possible types, and code without annotations
is `any`, so only definite mistakes are reported, and annotations make the checks
stricter. Its types are sets of twelve kinds (`null bool int float dec str list map fn
html response error`), lists with what they hold and maps with their fields. It reports
unknown names (with suggestions), names used before they are
defined, names defined twice, names a file did not take, wrong argument counts and types (for builtins too), operators that can never
work, division by a written zero, return and error types, effects, view and component props,
component code that cannot run in a browser, route mistakes, and calls like `last(xs)`
where a variable of yours named `last`, known not to be a function, hides the builtin.
When a call to a function of your own named like a builtin (`notify`, `first`) has the
wrong arguments, the error says so: this `notify` is the program's own, from line 1,
which hides the builtin `notify`.

Its warnings: an unused variable (name it `_x` to keep it); a function named like a
builtin, or a variable named like a module; a line starting `-x` that does nothing;
`||`, `&&`, `!`, `if` or `? :` on a value known to be a string or a number; SQL put
together from values, and an `http.*` host from a value (see [Standard
library](#standard-library)); a `/` result given to an `int` parameter; a helper with no
`with` that may use an effect its `with` caller lacks; `http.*` or `net` inside a
transaction that retries; a live loader fed only by a service; a class a sheet styles
that nothing writes; a `.qwer` function that cannot be exported; `sleep` given a number
written between 0 and 1 (`sleep` takes milliseconds: `` `sleep(0.2)` waits 0.2 ms; for 0.2
seconds write `sleep(200)` ``); `c.query(...)` on an untyped parameter in a `with`
function without `db`; a service whose file is not here. Warnings never make
it fail.

`quillo check --policy FILE` checks the program as it would run under that policy (and
the host's and the program's own, which it always reads): it names the files first
(`quillo: policy host.toml + quillo.policy (effects: db, http, time; no builds; no
private addresses)`), then what each would refuse, as errors: a builtin needing an
effect the policy does not allow (`` `udp.listen` needs the net effect, which host.toml
does not allow ``), a plugin it does not list, a Rust file with no `//! effects:` line
under a policy that narrows effects. See [The policy](#the-policy).

What it does not find, so a reader is not surprised: an unknown type name in an
annotation (`u: Usr`, when no `type Usr` exists; it fails at the first call), division
by a variable that is zero, `==` between values that can never be equal, a `match`
that misses a case (it fails when the value comes), and component code that reaches the
database through a helper between them (`quillo run` refuses that before it serves).

`quillo check` writes its findings to stderr as `file:line: error: ...` and `file:line:
warning: ...`; with errors it ends `quillo: N errors` and exits 1; else it writes
`main.qwe: ok (12 routes, 40 functions, 9 components, in 18 files)` to stdout (with the
warnings counted) and exits 0. `--strict-null` (below) is its one option.

After a syntax error that a newer Quillo might accept (an unknown character or escape,
an unexpected keyword, an unknown word), `quillo check`, `quillo run` and the language
server say which quillo they are (`note: this is quillo 0.6.0 (f1695a3, 2026-10-29),
built 2026-10-27 at ...`): code written for a newer Quillo fails to parse in an older
one, such as an old build left on the `PATH`. `quillo --version` names the commit too: `QUILLO_COMMIT` when
the build sets it (as for a source archive, which has no git), else the checkout's.
`scripts/doctor.sh` and `doctor.ps1` point out a `quillo` on the `PATH` older than the
checkout's own build.

It also knows what lists hold and which fields maps have, from what made them: a list
or map literal, an annotation, `split`, `range`, `merge`, `set`. A lambda given to
`map`, `filter`, `find`, `sort_by`, `reduce` and the like takes what the list holds, and
`for x in xs` too, so `[1, 2] | map((x) => x.name)` is an error; `map` gives a list of
what its function returns, `first` an item or null. A map whose fields are known has no
others: `{ title: "Hi" }.titel` is an error (`did you mean title?`), since a missing key
would read as null and nothing would fail. Maps that come from elsewhere (the database,
JSON, a parameter without an annotation) and `{}` are open: any field may be there.

`quillo check --strict-null app.qwe` also warns where a value may be null and null does
not work: an argument `str | null` for a `str` parameter, `count + 1` with `count: int |
null`, a `-> str` function that may return null. Checks narrow the type, as in
TypeScript: after `if x == null { return ... }`, in `x != null ? f(x) : ""`, `x != null &&
f(x)`, `if x { ... }`, and in the arms of a `match` after `null =>`. As in TypeScript,
`xs[i]` is an item, not "or null" past the end. A parameter with a default (`page: int =
1`) is null only if its default may be. Editors get the same warnings with the
language server's `initializationOptions: { "strictNull": true }`.
