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
Also: a parameter, a record's field, or a local `fn` or `state` named twice; a pattern's
type the program has not (`Usr`); a type that names itself with nothing between (`type Ab
= Ab | int`, see [Records](#records)) *(since 0.8.9)*; a `let` or a `match` that cannot match what it is given
(`let [a, b] = [1]`, `match x { }`); a lambda called with the wrong number of arguments,
by its `let` name or given to `map`, `filter`, `reduce`, `sort_by` and the like; a list
whose item is not what `list<T>` takes (`[1, "a"]` for `list<int>`); a field `request`
does not have (`request.qurey`); top-level code calling a function that reads a
global defined further down; a name bound twice in one pattern (`[x, x]`), a name two
tested fields bind used (`{ user.id: int, order.id: int } => id`), and a route binding
one name by two of its path, query and request patterns; and `every` and `schedule` as
the start reads them, their written-out arguments (`every(0, f)`, a cron field `60`,
`@reboot`) and a job or tick named twice where both calls surely run (see [Scheduled
jobs](#scheduled-jobs)).
When a call to a function of your own named like a builtin (`notify`, `first`) has the
wrong arguments, the error says so: this `notify` is the program's own, from line 1,
which hides the builtin `notify`.

A program source (`.qwe`, `.qwer`) is read only as a regular file of at most 16 MiB, and one
program's sources come to at most 32 MiB together for `quillo check`, the language server and
`quillo docs`; other files (`app.toml`, `quillo.policy`, `public/` style sheets and scripts)
are read up to 64 MiB, so a cloned repository's link to `/dev/zero` or a FIFO is refused
(`cannot read ...: it is not a regular file`) rather than read for ever *(since 0.8.7: a link
to `/dev/zero` ate memory)*. The program or
`--policy` named on the command line may be a pipe or device under `/dev` or `/proc`
(`quillo run <(...)`, `quillo run /dev/stdin`): it is read once *(since 0.8.7:
`quillo run <(...)` printed nothing)*. `.env` and `--env-file` are
read as they come.

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
function without `db`; a service whose file is not here; a name in a pattern that takes
any value and goes unused, or reads like a type (`intt =>`); an arm that never runs
after one that takes any value, or with a value an arm before it has; `xs.push(2)`,
`sort(xs)` or `i + 1` alone, which make a value and drop it; a `return` that leaves the
function from a value that is kept (a block, a `for` body, an `if` or `match` every way
of which leaves); code after a `return` or a `reply`, which never runs; a function
that returns a value on one path and gives null at its end (see [Coming from
JavaScript](#coming-from-javascript)); a `while` whose condition
nothing in it changes; `"Hello ${name}"` in a plain string; `index_of(...) == -1`, a
function used as a condition (`if ready {`), and a path's untyped parameter added to a
number; a misspelt attribute or event (`clas`, `@clik`), `className`, and `<card>` for a
view `Card`; a name imported as a builtin's; a key written twice in a map
(`{ a: 1, b: 2, a: 3 }`, `{ a, "a": 2 }`: the last value is the one kept; a spread
before it is no second key); a function written in a route's answer, which cannot be
JSON (`` this route's answer holds a function at `f` ``), the program's own or one the
route's block makes (`fn helper() = 2`, `let f = (x) => x`; a call of it is a value); a job or tick named twice where
both calls may run; a `GET` route that writes and reads who the visitor is (see `writes`
under [Routes](#routes)); a `media.on` rule *(since 0.8.9)* for a name ending in `*` whose
`publish` or `watch` never reads which stream is asked for (see [Media](#media)); a page
state *(since 0.8.9)* that a component reads and that comes from `env`, `read_file` or a
`select *` row (see [Page state](#page-state)); a stream's `send` *(since 0.8.9)* inside
`db.transaction` or `db.atomic`, which on SQLite holds every other request while the client
reads (see [Concurrency](#concurrency-and-reloading)). Warnings never make it fail.

It also warns of what a page does:

- A written `href`, `redirect` or `navigate` (or a template's written start) to a path no
  route, mount, service, file route or file under `public/` or `files()` answers, with
  the nearest one: ``no route answers `/abuot`, so this link is a 404 (did you mean
  `/about`?)``; and a redirect to a path only another method answers, as a redirect is
  followed with `GET` (a 307 or 308 keeps the method, and is not warned of). The nearest
  route is measured on the address the browser asks for: `/docs/../abuot` is `/abuot`, as
  is `/%61buot`.
- Markup a keyboard can't reach: a click on an element with no role, `tabindex` or key
  handler around it (`` a `<div>` with onclick can't be reached with the keyboard: use a
  `<button>` ``), an `<img>` with no `alt` (`alt=""` for decoration), and `:focus {
  outline: none }` with no `:focus-visible` and no other ring for that selector.
- CSS built from a value in a `<style>` or in a `style=` made with `+`: write css`...`,
  or a style map, whose values are written as CSS values.
- A group that guards its routes but not the server fns their pages call, followed through
  helpers, views and components; and an effect that what a page runs while it is made (a
  component's `state` values, its markup, the helpers they call) needs and the route's
  `with` lacks: an error where it always runs, a warning where it runs under a condition,
  on the left of `??`, or in `state x = defer(...)`.
- A new value or an error dropped at the end of an `if`, `else`, `match`, `for` or `while`
  branch, or by a handler named by reference (`onclick={go}`, judged by what `go` gives).
  A `server fn`'s error dropped in a handler is not warned of: the run logs it.
- A component's plain `let` that a handler changes and its markup shows (`` `shown` is
  changed in a handler but isn't a state, so the page won't show it: write `state shown =
  0` ``).
- A value that can fail shown in markup (see [Errors](#errors)), an event attribute that
  never runs (see [Composition](#composition)), `random`, `random_int` or `uuid` drawn in
  a component's body and shown (see [Components](#components)), `pages // 2` (see
  [Values](#values)), and Vue's and Svelte's spellings (see [Coming from other
  frameworks](#coming-from-other-frameworks)).
- A sheet under `public/` has its unused classes warned of only in a program that serves
  pages, and classes written in `public/**/*.html` and in the `.html` files and scripts of
  `files()` folders count as written.
- A component inside `<head>`, an error *(since 0.8.11)*: call `page_head` from it (see [The
  page's head](#the-pages-head)); and `{json.stringify(x)}` as the whole of an
  `application/ld+json` script, a warning: it is written as a JSON string, so give the map,
  `{x}`.

And of what the program's files reach *(since 0.8.11)* (see [What a program
reaches](#what-a-program-reaches)):

- A file taken for some of its names (`import lib/geo.qwe use lookup`) that also adds a route
  or a tool, or reaches out as the program starts, a warning: ``lib/geo.qwe is taken for
  lookup, and also adds GET /x``.
- A Rust file whose `//! effects:` line leaves out what its code plainly calls, a warning
  that names the call and its line (``tools.rs says `effects: fs`, and calls
  std::process::Command::new (line 5): a reading of its text, not a proof``).
- An `http.*` call whose written host the policy's `[http] hosts` refuses, an error at the
  call, where it was an error value at run time.
- Once the program has a `quillo.confirmed`, each thing a file reaches that it does not
  confirm, an error at its line (``reaches example.com: not confirmed for this file``), and
  a Rust file that is not the one confirmed by its SHA-256.
- Under a provider's policy, a program with no `quillo.confirmed`, or one that leaves
  something out: a warning in 0.8.11, which says the release it stops in, and an error from
  0.8.12.
- A host that comes only from `env()` (and text written in the program, and their trims) is
  warned of as the operator's setting, not a visitor's (``the host `http.get` goes to comes
  from `API_HOST`, the operator's setting: say { public_only: false } ...``).

A type grows as the code is read (a list of maps of lists, made again on each line), so
the check keeps each to 1,000,000 parts and 64 levels, and takes what grows past them as
unknown, which says less but never wrongly: a program whose types double on each line is
checked at once, and a data table written out keeps its fields known *(since 0.8.9: such a
program took the check's memory)*. A check of code from a program's users, by
`check_program`, keeps tighter limits and a time (see [Checking code from a
program](#checking-code-from-a-program)).

[`quillo check --policy FILE`](#other-commands) (`quillo check main.qwe --policy host.toml`)
checks the program as it would run under that policy (and
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
warnings counted) and exits 0. Its two options, given after the program as `quillo run`'s are,
are [`--strict-null`](#other-commands) (below) and `--policy FILE` (above).

A `"""` or `'''`, as Python and Kotlin write text over several lines, is told what Quillo
writes *(since 0.8.11)*: ``tq.qwe:1: error: `"""` is not a string here: text over several lines
is a backtick string, `line one ...` (`${` interpolates)``.

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
