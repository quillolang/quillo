# Language tour

## Values

| Kind | Written |
|---|---|
| Null and booleans | `null`, `true`, `false` |
| Numbers | `42`, `3.14`, `1e-9`, `6.02e23` |
| Decimals | `12.50m`, `5m`: exact, for money (see [Decimals](#decimals)) |
| Strings | `"strings"` or `'strings'`: plain text, so JSON is easy to write: `'{"a": 1}'`. Escapes: `\n \t \r \0 \\ \" \' \` \$ \{ \}`, and `\u00e9` or `\u{1F600}` for any character |
| Template strings | `` `Hello, ${name}` ``: the only strings that interpolate; they can span lines |
| Lists | `[1, 2, 3]` |
| Maps | `{ name: "Ada", "content-type": "text/plain" }`; `{ id }` is short for `{ id: id }` |
| Others | functions, HTML and error values |

| Expression | Gives |
|---|---|
| `xs[0]` | the first item; `first(xs)` says the same in words |
| `xs[-1]` | the last item (`last(xs)`): negative indexes count from the end |
| `xs[99]`, a missing map key, a field of `null` | `null` |
| `"héllo"[1]` | `"é"`: strings index by character |
| `"abc".foo`, `5.foo`, `xs[1.5]` | runtime errors: only maps have fields, and lists index by int |

A name right before a template calls it with the text: `` shout`hi ${name}` `` is
`` shout(`hi ${name}`) ``. The builtins `css` and `sql` return the text as it is, and
tell editors to highlight it as CSS or SQL.

Operators are as in JavaScript, with these meanings:

| Operator | Means |
|---|---|
| `+` | numbers add; a string with anything is text (`"n=" + 1` is `"n=1"`, `"a" + null` is `"anull"`); two lists join; two maps merge, the right one's values winning; `true + 1` and `null + 1` are errors |
| `- * %` | as in JavaScript on numbers; nothing else is converted (`"5" * 2` is an error) |
| `/` | exact division: `7 / 2` is `3.5`, and a whole result stays an integer (`6 / 2` is `3`); `floor(7 / 2)` is integer division |
| `== !=` | deep equality, like comparing JSON: `1 == 1.0` and `0.1m == 0.1`, but `1 == "1"` is false (nothing is converted); functions are equal only to themselves |
| `< <= > >=` | comparison of two numbers (ints, floats and decimals mix; not a decimal and a float), or two strings by code point; a string and a number, booleans, or NaN is an error |
| `!`, `&&`, `\|\|` | not, and, or; `a && b` and `a \|\| b` give back one of their operands, so `{loading && <Spinner/>}` works |
| `??` | the right side when the left is `null` or an error value |
| `cond ? a : b` | `a` if `cond` is truthy, else `b` |
| `a..b` | the ints from `a` up to, not including, `b` (both ints; at most ten million) |

| Falsy | Truthy |
|---|---|
| `null`, `false` and error values | everything else, `0` and `""` included |

Because `""` is truthy, `name || "anonymous"` keeps an empty name: write
`filled(name) ?? "anonymous"` to replace a blank or a missing name, or
`name ?? "anonymous"` to replace only null (`quillo check` points these out).

Ints are 64-bit: `+`, `-`, `*` and unary `-` past their range are a runtime error
(`integer overflow`), never a wrap. An int divided by int 0, or `%` 0, is an error;
a float divided by zero is `inf` or `NaN`, written so, and ordering either is an error.
There is no `**`, no bitwise `& | ^ << >>`, no `++`, no `*=` or `/=` (only `=`, `+=`,
`-=`), no `?.`, no `===`, no `in`; `pow(a, b)` and `math.*` do the arithmetic, and
[`bit.*`](#numbers-and-data) the bits (`|` is the pipe).
Comparisons do not chain: `1 < x < 3` is a syntax error.

From loosest to tightest: `? :`, `??`, `|` (a pipe), `||`, `&&`, comparisons, `..`,
`+ -`, `* / %`, unary `-` and `!`, then calls, `.field`, `[index]` and `?`. So a pipe
binds looser than everything but `??` and `? :`: `xs | find(f) != null` is `xs |
(find(f) != null)`, and `(xs | find(f)) != null` is what was meant; `xs | len ?? 0` is
`(xs | len) ?? 0`. `??` and pipes group to the left.

## Coming from JavaScript

Most expressions read as they would in JavaScript. The differences:

- `let` declares a variable (there is no `const` or `var`); values are immutable, so
  `push`, `set` and `+` return new lists and maps, and `m.name = v` is a runtime error
  outside a component (there it sets an element's property; see [JavaScript
  libraries](#javascript-libraries)). Spread works in literals as in JavaScript:
  `{ ...m, name: v }`, `[...xs, 3]`; `...null` spreads nothing.
- Named functions are `fn name(params) = expr` or `fn name(params) { ... }`; anonymous
  ones are arrows, which may declare a return type too (`(x: int): int => x * 2`).
  Blocks give back their last expression, so `return` is only for leaving early (at the
  top level of a `use`d file it leaves that file). Of two elements side by side, the
  first goes nowhere and `quillo check` warns: wrap them in one element. A call
  gives every parameter without a default, and no more: `f(1, 2)` for a one-parameter
  `f` is an error, as is a missing one. A default is worked out at each call, in order,
  and may use the parameters before it (`fn page(n, size = n * 10)`).
- `==` compares values deeply (`[1, 2] == [1, 2]`); there is no `===`.
- There are no semicolons to forget (`;` does separate statements on one line): a line
  break ends a statement unless the next line continues it (`|`, `.method`, `&&`,
  `||`, `??`, `?`, `:`, `else`, `*`, `%`, `==`, `!=`, or `- b` and `+ b` with a
  space), or the line before ends with something that needs more (`+`, `=`, `,`), or
  the break is inside `(...)` or `[...]` (not `{...}`). A line starting `-b` is a new
  statement; `quillo check` warns when one does nothing. A `/* */` comment over several
  lines ends a statement as a line break does.
- Names may use any letter (`let café = 1`). Reserved, so not a name anywhere: `let fn
  view route use return if else match for in as reply component while true false null`
  (and the old `then and or not nil`, refused with a hint). `with type state service
  mount server style` and the type names (`int`, `str`, ...) are words only where they
  mean something, so `let state = 1` and `let int = 5` are allowed (see [Files of a
  program](#files-of-a-program) for what hiding a builtin costs).
- Numbers are written `42`, `1_000_000`, `3.14`, `2.5e-3`, `6E+23`, `0xFF`,
  `0xFFFF_0000`: no octal or binary, no `.5`. An int past 64 bits is a parse error
  (`0x8000000000000000` too: a hex literal is an int, and does not wrap to a negative
  one); a float past `f64` is `inf`.
- Only `null`, `false` and error values are falsy: `0` and `""` are truthy. Code
  written by JavaScript habit then runs without failing but means something else:
  `name || "anonymous"` keeps an empty name, and `count && <Badge/>` shows the badge
  for 0. Where a value is known to be a string or a number, the checker warns about
  `||`, `&&`, `!`, `if` and `? :` on it and suggests the explicit form
  (`name != "" ? name : "anonymous"`, `count != 0 && <Badge/>`). It warns too where only
  the fallback says so, as with what comes from a request or a row: `request.query.name
  || "anonymous"`, `row.n || 10`. `filled(x) ?? "anonymous"` replaces `null` and `""`;
  `x || ""`, `x || 0` and `x || []` are left alone, as they mean the same in both
  languages.
- `match`, `|`, patterns, `route`, `reply`, `state` and `with` effects have no
  JavaScript equivalent.
- There is no `break` or `continue` (each is an ordinary name, and an error that says
  this where nothing defines it): `return` leaves the function, from a loop too, and
  from a `for … =>` that makes a list; to skip items, `filter` first. `return` is
  also an expression, like `reply`: `let m = find(xs) ?? return null`, `"a" => return 1`.
- Arguments are worked out eagerly, left to right, before the call; only `&&`, `||`,
  `??`, `? :`, `if` and `match` skip what they do not need. A lambda sees the variables
  around it as they are now, not as they were when it was made, and may assign them
  (`count += 1` inside a handler changes the outer `count`). A block that declares a
  name makes a scope: `let x` inside `{ }` hides the outer `x` until the block ends.
  `fn` inside a function is a local closure. There is no tail-call optimisation: 10,000
  calls deep is the limit, in a tail position or not.
- There are no classes, interfaces, enums (a `"a" | "b"` type, checked when the program
  runs, is the nearest), generics (beyond `list<T>` and map shapes), decorators or
  exceptions. A file sees its own names and what it takes with `use`, without a prefix:
  `import lib/auth use login`, then `login()`, not `auth.login()` (see [Files of a
  program](#files-of-a-program)). `mount` and `service` are the boundaries.
- Regular expressions are strings given to `re.*` (`re.find_all(text, "@(\\w+)")`), not
  `/.../` literals, and take only the syntax both sides read alike (see the standard
  library).

The built-in functions are free functions taking the list, map or string first (so
`xs | map(f)` reads left to right), and some differ from JavaScript's methods:

| JavaScript | Quillo |
|---|---|
| `xs.map(f)`, `xs.filter(f)`, `xs.find(f)` | `map(xs, f)`, `filter(xs, f)`, `find(xs, f)` (`null` when nothing matches) |
| `xs.reduce(f, init)` | `reduce(xs, init, f)`: the start value comes before the function |
| `Math.max(a, b)` | `max([a, b])`: `max`, `min` and `sum` take one list; `max([])` is `null` |
| `s.replaceAll(a, b)` | `replace(s, a, b)` replaces every occurrence |
| `s.indexOf(x)` | `index_of(s, x)`: `null`, not `-1`, when it is not there |
| `parseInt(s)` | `int(s)`: an error value, not `NaN`, for text that is not a number |
| `s.length` | `len(s)` counts characters: `len("é")` and `len("😀")` are 1 |
| `xs.sort((a, b) => a.n - b.n)` | `sort_by(xs, (x) => x.n)`; `sort(xs)` sorts numbers as numbers |
| `x.toFixed(2)` | `round(x, 2)` gives a number |
| `s.match(/re/g)`, `s.replace(/re/g, ...)` | `re.find_all(s, "re")`, `re.replace(s, "re", ...)`; `re.test`, `re.find`, `re.split` |
| `encodeURIComponent(s)` | `url_encode(s)`, which also encodes `!'()*`; `url_encode(map)` gives a query string |
- Text in HTML follows JSX, but `class` (not `className`) and `@click` (not `onClick`).

## Variables, functions, views

```quillo
let todos = []
todos = todos | push("x")        // reassign with =, += or -=

fn add(a: int, b: int = 10) -> int = a + b
fn area(w, h) {                   // block body: the last expression is the result
    let a = w * h
    a
}
let double = (x) => x * 2          // arrow functions, as in JavaScript
let triple = x => x * 3
let point = (x: int, y: int = 0) => { x, y }      // `{ x, y }` here is a map,
let same = (x) => { x }                           // but `{ x }` alone is a block

view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>
```

Type annotations are checked when the function is called and when it returns, and
`quillo check` checks them before the program runs: `int float dec num str date bool list map
html fn response any` (`num` is `int | float`, not `dec`). Unions are written `int | str`; add `?` or `| null` to also allow null
(`str?`, `int | null`). `list<int>` says what a list holds, and `{ title: str, tags:
list<str>, note: str? }` a map's fields; `quillo check` holds the program to them (a
call with a map missing `title`, a list of strings for `list<int>`), and at run time
they are `list` and `map`. A capitalised name in an annotation is a record's (below); one
that no `type` declares is not reported by `quillo check`, and fails at the first call.
A function with a capital first letter (`fn Card()`) is taken for a view.

```
fn titles(posts: list<{ id: int, title: str }>) = posts | map((p) => p.title)
```

## Records

`type` names a shape, so it can be used by name, checked when the program runs, and
made from what a client sends:

```quillo
type Address = { city: str, zip: str? }
type User = {
    name: str,
    email: str,
    role: "admin" | "member" = "member",    // one of these strings; a default
    address: Address?,
    tags: list<str> = [],
}
type Id = int | str                         // another name for a type

let ada = User({ name: "Ada", email: "ada@example.com" })
// { name: "Ada", email: "ada@example.com", role: "member", tags: [] }

fn greet(u: User) = `Hi ${u.name}`

route POST /users { json: User } = save(json)    // the body, made a User; else no match
route POST /users = respond(422, "not a user")

match v {
    User as u => u.role
    _ => "not a user"
}
```

- A record is a map: JSON, the browser and every map function see it as one. What its
  name adds is checking.
- `User(fields)` makes one: each field is checked, records in it are made the same way
  (and lists of them, item by item), missing fields get their defaults (made afresh
  for each record), and fields the record does not have are left out, so what a client
  sends cannot add any (`is_staff: true` in a body does not get through). It gives an
  error value saying what is wrong, as `int("x")` does: `User({ name: "Ada" })` is
  `err("User: `email` is missing")`, and a deeper mistake names its place
  (`` `address.city` is missing ``, `` `tags[1]` must be str, got int 2 ``). So
  `User(request.json)?` passes it on, and `?? reply 400` answers it.
- In an annotation, a record is checked field by field when the function is called or
  returns: `greet({ name: "Ada" })` fails with `` argument `u` of `greet` must be User,
  got map { name: "Ada" } (`email` is missing) ``. A field with a default must be
  there too; `User(...)` is what puts it in. A `server fn`'s arguments from the browser
  (or another service) are made as `User(v)` makes them: checked the same way, with
  defaults put in and fields the record does not have left out, so a page cannot add
  `is_admin` to the profile it saves.
- In a pattern, a record matches what `User(v)` would make, and binds that: in a route,
  `{ json: User }` takes the body as a User, and `{ form: Signup }` a form, its text
  converted to the numbers and booleans the record asks for, as query values are.
- `quillo check` holds code to the fields: `u.nmae` (did you mean `name`?), a field
  `User` does not have in `User({ ... })`, a missing field or one of the wrong type,
  and a default that is not of its field's type.
- A type is named like `User` or `OrderLine` (a capital letter first, and small ones),
  which keeps it apart from constants such as `MAX` or `DAY`; `type ID = int` is
  refused. A record may name itself (`type Node = { value: int, kids: list<Node> = [] }`).
  `type`, like `service` and `mount`, is written at the top level. A field that is
  there but `null` counts as missing: its default fills it, or it is reported.
- `date` is text that is a calendar date (`YYYY-MM-DD`, a real day): `type Leave = {
  start: date, end: date }` refuses `"2026-02-30"` and a date-time, in a record, an
  annotation (`fn next(d: date)`), a pattern (`match x { date => ... }`, a query's `?
  { from: date }`) and a `server fn`'s arguments from a page, so the `date` module's
  functions, which fail on anything else, are safe on what got through. It is a `str` to
  `quillo check` and to JSON (OpenAPI's `format: date`); as a variable's name, `date`
  still hides the module.
- Strings in a type (`"admin" | "member"`) are checked when the program runs; `quillo
  check` takes them as `str`.

## Pipelines and method calls

`a | f(b)` is `f(a, b)`. `x.f(y)` calls `f(x, y)` when `x` has no field `f`,
and `x.len` is `len(x)`:

```quillo
orders
    | filter((o) => o.total > 100)
    | sort_by((o) => o.total)
    | map((o) => o.id)

orders.filter(o => o.paid).map(o => o.id)
[3, 1, 2].sort().first()
```

A line starting with `|`, `??`, `&&`, `||`, `?`, `:`, `.method` or `else` continues
the previous line.

## Control flow

```quillo
n > 0 ? "pos" : "neg"
if n > 0 { "pos" } else if n == 0 { "zero" } else { "neg" }   // also a value
for x in [1, 2, 3] => x * x                 // a list: [1, 4, 9]
for [key, value] in { a: 1 } => `${key}=${value}`
while i < n { i += 1 }                      // null; `return` leaves it early
match value { ... }
return x
```

`while` runs its block for as long as the condition holds (anything but `null`, `false`
or an error, as with `if`). Numeric functions with `while` loops are compiled to machine
code like those with `for` (see [Compiled numbers](#compiled-numbers)).

Everything is an expression: `if` without `else` is `null` when the condition fails,
`while` is `null`, `for` is the list of its body's values (with a block body too:
`for i in 0..3 { i * i }` is `[0, 1, 4]`), and `match` is its arm's. A body in braces
ends at its `}`, after `=>` too, so what follows takes the list: `for n in ns => { ...
} | flatten()` flattens the list of every round's values. `for` runs over a
list, a map (as `[key, value]` pairs), a string (character by character), a range (not
made into a list first) or `null` (no rounds); anything else is an error. A `match`
that no arm takes is a runtime error, and so is a `let` pattern that does not match.

## Patterns

The same patterns work in `match`, `let`, `for`, and route declarations:

| Pattern | Matches |
|---|---|
| `_`, `name` | anything (`name` binds it) |
| `42`, `"order"`, `true`, `null` | that literal |
| `int`, `str`, `list`, ... | any value of that type (`dec` also takes an int, numeric text or a JSON number, made a decimal; `date` takes text that is a date, `"2026-11-20"`, not a date-time) |
| `"order" \| "refund"` | either alternative |
| `int as n` | binds what matched |
| `[first, second, ..rest]` | lists |
| `{ type: "user", name, customer.id: id, limit: int = 20 }` | maps (extra keys are allowed); `a.b` looks inside nested maps; `= default` fills a missing field |
| `User`, `User as u` | what `User(v)` makes of it (see [Records](#records)) |

In a map pattern, `key: int` (a bare type) also binds `key`, and `= default` fills a
field that is `null` as well as one that is missing. A name written like a type
(`Item`, `Max`: a capital letter, then a small one) is always a type test, never a
binding: `let Max = 1` fails, and `match x { Foo => ... }` with no type `Foo` never
matches. In `"a" | "b"` alternatives, a name bound in one alternative only is unbound
when the other matched.

```quillo
match event {
    { type: "click", x: int, y: int } => `click at ${x},${y}`
    { type: "key", key: "Enter" | "Return" } => "enter"
    int as n if n < 0 => "negative"
    _ => "other"
}
```

## Errors

`err(x)` makes an error value. Builtins such as `int("abc")` and `json.parse`
return errors instead of crashing. `?` after an expression returns the error from
the current function, and `??` falls back when the left side is null or an error, or
could not be worked out: `1m / 0m ?? 0m`, `date.of(y, m, d) ?? today`,
`re.test(s, typed) ?? false`. Without `??`, such a stop ends the request with a 500, so
a total that overflows cannot go on as a value and reach the database unseen; with it,
you have said what to do. `is_err(r)` asks whether a value failed and `error(r)` (or
`r.error`, `r.message`) what it says, and on any other value that is null. Field access
on null gives null (so there is no need for `?.`):

```quillo
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err(`bad age: ${s}`)
    }
fn next_year(s) = parse_age(s)? + 1         // err("bad age: x") for "x"
let age = int(request.query.age) ?? 18
```

At the top of a program, or of a file it uses, an error that `?` passes on has no one to
take it: the program stops, saying what it was, with status 1, and a server does not start
half made (FEEDBACK29.md, Q-350).

An error value a statement gives and nothing takes, as a call alone on a line whose value
is an error, is not lost: the log says where it was dropped, where it was made and the
request it was in, and the program goes on.

```
main.qwe:10: an error no statement took, made at main.qwe:6: odd (pass it on with ?, give a value with ??, or take it with let _ =)
```

In a script it goes to stderr, and the script goes on. `f()?` passes it on, `f() ?? x`
gives a value in its place, and `let _ = f()` takes it and says it is not wanted. Inside
`db.transaction(() => ...)`, a dropped error rolls the transaction back (see
[Database](#standard-library-database)). `quillo check` warns when a call of one of the program's own
functions that can fail is dropped: one that declares `! T`, or whose body calls `err(`
or uses `?`. A builtin's dropped error (`db.exec(...)` alone on a line) is not warned of;
it is logged when it happens (FEEDBACK27.md, Q-332, Q-333).

A builtin given an error where it wants something else says what the error was:

```
json.parse expects a str as argument 1, got error (it failed before this call, main.qwe:3: read_file: does-not-exist.json: No such file or directory)
```

`int(r)` of an error says `cannot convert error to int (...)` with the same reason.

`?` leaves the function it is written in, and in a lambda that is the lambda:
`map(xs, (x) => f(x)?)` gives a list with the errors in it and goes on, and `quillo
check` says so. To stop at the first failure, use a `for` loop, where `?` leaves the
function around it. `filter`, `find`, `any`, `all` and `count` give back an error their
function gives, since it has said neither yes nor no, so `filter(xs, f)?` stops there;
`map` keeps each result, an error too. In a component's body, `?` and `return` are ways
out of it, so a body with one is made again whole on each change, which loses the focus
of an input; `x?.y` is such a `?`, and `quillo check` says so. A field of null is null, so
write `x.y`.

`! T` in a signature declares what the function's errors contain. It is checked: a
function declared `! str` that returns `err(42)` is a runtime error, and `quillo check`
reports it, and a `?` that would pass on the wrong kind of error, before the program runs.

Mistakes in the code itself (calling a non-function, adding a map to an int, a
failed type check, recursion more than 10,000 calls deep) are runtime errors. In a
route they become a 500 response, and the server logs them.

A runtime error says where it happened and how it got there, in the terminal, the log and,
under `--watch`, on the page. A visitor sees only `internal error`:

```
quillo: price.qwe:2: division by zero
  in share, called at main.qwe:4
  in split_bill, called at main.qwe:7
```

A deep recursion keeps its first and last five calls. Under `--log json` the calls are a
`calls` field beside `error`. The chain is built as the error leaves each call, so calls
that succeed pay nothing, and a function in machine code gives the same chain, as what
fails there runs again in the interpreter. An error value is not a failure yet and has no chain, but
the log says where one was made when it ends a request: `main.qwe:8: db: no such column:
nope`. A program does not see that place: two errors with the same contents are equal
wherever they were made.

In a page under `--watch`, the browser's console says the same, from the error's stack:
the `.qwe` file and line, and the calls. In production it says the message alone.

## Effects

`with` declares what a function may do besides computing a value:

```quillo
fn load_user(id: int) with db = db.one("select * from users where id = ?", id)
fn fetch_avatar(url: str) with http = http.get(url).body
fn profile(id: int) with db, http = { user: load_user(id), avatar: fetch_avatar(...) }
```

The effects, and the builtins behind each:

| Effect | Builtins |
|---|---|
| `db` | every `db.*` but `db.error_kind` and `db.sql_name`, and the same on a connection from `db.connect` (`analytics.query`); `idempotent`, `later.status` |
| `http` | `http.*`, and calls to services |
| `fs` | `file`, `files`, `read_file`, `write_file`, `delete_file`, `save_upload`, `check_program`, `image.info`, `image.convert` |
| `env` | `env`, `args`, `stdin`, `read_line`, `exit` |
| `time` | `now`, `sleep`, `date.today`, `date.now` |
| `random` | `random`, `random_int`, `uuid`, `crypto.token`, `crypto.hash_password` (its salt) |
| `net` | `udp.listen`, `udp.send`, `udp.reply`, `media.rtp`, `channel.send`, `channel.broadcast` |

Inside a `with` function, a builtin that needs another effect,
or a function declared with more effects, is an error, both at runtime and in
`quillo check` (which also warns when an undeclared helper may use one). The limit holds
for everything the function calls, including the lambdas it passes to `map`: the
effects allowed are narrowed on the way in, and nothing called can widen them.
Functions without `with` may do anything.

- **A connection's calls** need `db` as `db`'s do: `analytics.query(...)` in a `with
  http` function is an error. On a parameter with no type, `c.query(...)` (or `.one`,
  `.exec`, `.transaction`) is a warning, since `c` may be a connection: `` `c.query` reads
  like a database call, which needs the db effect ``.
- **A Rust function** (a `.qwer` or `.rs` file) has the effects its file declares in
  its header, `//! effects: db, http` (or `//! effects: none`). A file without that line
  may do anything, so its functions run outside any `with` as before, and are refused
  inside one, at run time and by `quillo check`: `` `hi` is Rust whose file does not say
  what it does, so it may do anything and is refused in `greet` ``. An editor's hover
  shows `(effects: not declared)` for them. Quillo cannot see inside the Rust: the line
  is the file's word, not a proof (see [Rust](#rust)).
- **The policy** sets a `with` on the whole program from outside it: its `[effects]
  allow` is what every route, job and function may use at most, and a Rust file with no
  `//! effects:` line is refused anywhere under it (see [The policy](#the-policy)).

```quillo noexec
use "store.qwer"            // //! effects: fs
use "hi.qwer"               // no effects line

fn keep(text: str) with fs = store_put(text)        // allowed: store.qwer says fs
fn stamp() with time = store_put(str(now()))        // error: store_put needs fs
fn greet() with time = hi("Ana")                    // error: hi.qwer says nothing
print(hi("Ana"))                                    // outside any `with`: runs
```

## HTML literals

HTML is a value, not a string. It is written like JSX, with no `<>` fragments: text goes in as it is,
expressions go in `{...}`, and everything is escaped unless it is already HTML:

```quillo
<li.todo #todo-{t.id} class={t.done && "done"} data-id={t.id}>
    {t.done ? <s>{t.text}</s> : t.text}
    <button disabled={!editable}>Edit</button>
    Added {t.age} days ago &mdash; {t.owner}
</li>
```

- Text follows JSX's rules: each line is trimmed, blank lines are dropped and the rest
  are joined with a space. Entities such as `&amp;` and `&mdash;` work; write `{"{"}`
  for a brace. A `$` is text too, so `${name}` shows a `$` before the value, and
  `quillo check` asks: write `{name}`, or `{"$"}{name}` for a price. Braces in text are
  code, so a `return` in them leaves the function: `<pre>f() { return 1 }</pre>` answers
  `1`, not the page, and `quillo check` warns (`` `{ return ... }` in <pre> leaves the
  function, and its value is the answer, not the page ``).
- `.class` and `#id` shorthand, with `{}` interpolation in the name. A class may hold
  `:` before a letter or digit and `/` before a digit, as Tailwind's do:
  `<em.hover:underline>`, `<div.md:flex.w-1/2>`.
- `{name}` is short for `name={name}`, and `{...map}` spreads a map's keys as attributes
  (or as a view's props): `<a {...link}>`, `<Card {...node.props}/>`. Attributes written
  out win over spread ones, whatever the case of the key (`ACTION` is `action`); classes
  from both are kept. On a view or component, spread
  keys that are not parameters are ignored, so data with extra fields can be passed
  straight through; a misspelled attribute written out is still an error.
- Attributes: `"text"` (plain, as in JSX) or `{expr}`, for example
  ``href={`/todos/${id}`}``. HTML's other two ways are taken too, as text with no `{...}`
  in them: `'single quotes'` and an unquoted value, `<p class='a b' id=top>`, which go
  out double-quoted. `true` renders a bare attribute; `false`/`null` leaves it
  out. A name may hold `-`, `_`, `:`, `@` and `.` (`data-id`, `bind:value`, `@click`),
  the `{name}` shorthand takes a bare name, and an attribute written twice is an error.
- Event handler attributes (`onclick`, any `on...`) and `srcdoc` take only `"text"`
  written in the program: the browser decodes their value and runs it, so escaping
  cannot keep data out of the code. `onmouseover={...}` is an error when the program
  loads (and in `quillo check`); handle events with `@mouseover={...}`. An `<iframe>`
  with a written `sandbox` attribute that does not say `allow-same-origin` may take
  `srcdoc={...}`: its page cannot reach the site. A view's or component's attributes
  are its parameters, so `on_change={...}` there is fine.
- `null`/`false` children render nothing; list children render each item, so `{for ... => <li/>}` works.
  A map as a child is shown as Quillo writes it, which is seldom meant: `quillo check`
  warns on `<p>{{ name }}</p>` (`` shows the map `{ name: ... }`: to show `name`, write
  `{name}` ``), and on any map or `{{ ... }}` block as an element's child. A map as an
  attribute (`Prop={{ k: v }}`) is as before.
- Void tags (`<br>`, `<img ...>`, `<input ...>`) need no closing tag. Any tag can self-close with `/>`.
- `<html>` gets `<!doctype html>`. A `<!DOCTYPE html>` written before `<html>`, as an HTML
  file has it, is passed over; before another element it is an error (`` `<!DOCTYPE html>`
  goes before an <html> element, which gets one by itself ``).
- `<style>{style}</style>` takes a style sheet as it is (no escaping, so `a > b` works).
  Write it as `` css`...` ``: a template string (braces are plain text, `${...}` fills in
  values) that editors highlight as CSS. `` sql`...` `` does the same for queries.
- A component's or view's own CSS is `style` at the start of its body, before its markup:

  ```quillo
  component Card() {
      style css`
        .title { color: darkred }
        .title:hover { text-decoration: underline }
      `
      state open = false
      <div.card><h2.title>{...}</h2></div>
  }
  ```

  It styles only the elements the component writes, so another component's `.title`, or
  the page's, is left alone. Each element written in its body (in helpers written there
  too, and in markup it passes to other components) carries an attribute of its own,
  `data-q-<id>`, and each rule is made to need it: `.title` is `.title[data-q-3f9a0c12]`,
  the same on the server and in the browser. `:global(...)` keeps a selector, or the start
  of one, for the page: `:global(body.dark) .title`, `.map :global(.leaflet-pane)`.
  `@media`, `@supports`, `@container` and `@layer` are scoped inside; `@keyframes` and
  `@font-face` are kept as they are. The style is the same for every use of the component,
  so it cannot read its parameters (set a class or `style=` on an element for that).
- A `style` at a file's top level styles the components that file declares, and nothing
  else: not the markup of its routes, views or top-level `fn`s (even one its component
  calls), not other files' components, not the page. It is scoped as a component's own
  style is, with an attribute of the file's (from its path, the same on the server and in
  the browser) on every element its components write, and sent once for the file rather
  than once for each component that shares it:

  ```quillo
  // lib/cards.qwe
  style css`.title { color: darkred }`
  component Card(t) {
      style css`.title { color: navy }`        // a component's own rule wins over the file's
      <div.card><h2.title>{t}</h2></div>
  }
  component Badge(t) { <span.title>{t}</span> }   // darkred
  ```

  It comes before the file's first component (``a file's `style` comes before its
  components``), and the file must declare one (``a file's `style` styles the components
  declared in it, and this file has none``). Several are joined in order. Its rules come
  before the components' own, with the same specificity, so a component's own rule wins.
  `import cards use Card` takes Card with its file's style. `style global` at the top
  level does the same with the selectors as written, not scoped.
- `style_with("npx postcss")`, at the top level, passes every component's and view's
  `style` through a command that reads CSS and writes CSS, as the program loads and before
  the styles are scoped: PostCSS with Tailwind's plugin (and its config beside the
  program) makes `@apply`, `theme()` and nesting work in a component's own CSS. All the
  styles go through it at once; what it made is kept while the CSS is the same, and a
  command that fails stops the program loading, with what it said. `style_with(f)` takes
  a function instead, Quillo's or a Rust file's (see [Rust](#rust)): it is called once
  with all the styles, each after a `/*! quillo-style <id> */` mark, and gives them back
  with the marks. One that fails, or gives something other than text, stops the program
  as a command does (``style_with `engine` failed: it gave int, not CSS text``):

  ```quillo
  fn engine(css) { replace(css, "@apply m-2;", "margin: .5rem;") }
  style_with(engine)
  ```
- With Tailwind's CLI for the page's own sheet: `quillo run main.qwe --watch --with
  "npx tailwindcss -i src/app.css -o public/app.css --watch"`, the sheet served with
  `file()`. Tailwind finds classes written out in strings and attributes (`class="px-3"`,
  `ok ? "text-green-700" : "text-red-700"`); a class made from parts (`` `bg-${color}` ``)
  is found by neither it nor Quillo. Under `--watch`, each time it writes the sheet the
  open pages take it as they are. Tailwind's scanner reads the shorthand too
  (`<div.flex.p-4.md:flex-row>`), but a fraction right before `/>` is read with the
  slash (`<i.w-1/3/>`): put a space before `/>`. `classes()` gives the classes the
  program's markup writes, sorted, for a CSS engine called from the program, and `quillo
  classes app.qwe` prints them one a line, for a tool beside it (Tailwind's `@source
  inline(...)`, or a safelist file). Both read the source: the `.class` shorthand,
  `class="..."`, `class:name`, and the whole words in the text of `class={...}`; a class
  made from parts (`` `bg-${color}` ``) is in neither. For
  production, build the sheet once (the same command without `--watch`) as a deploy
  step, and serve it under a name that changes with its content, with `file(..., {
  cache: "1y" })`: Quillo does not do that step. A CSS engine the program calls (a Rust
  crate in a `.rs` file) can make the sheet as the program starts, with `build "/app.css"
  = ...` (see [Static files](#static-files)).
- `style global card_css` is the component's CSS with its selectors as written, not made to
  need its attribute: for a library whose classes are its own already (the UI kit's are all
  `ui-...`), whose rules may reach into what the caller passes in (`.ui-card-body >
  :first-child`). It is still sent only to pages that show the component, and companions
  that share one stylesheet (`Tabs` and `Tab`, each `style global tabs_css`) send it once,
  also from a view whose body is only another component's tag (FEEDBACK28.md, Q-345).
- A page is sent the styles of the components and views on it, and of those its bundle can
  show, as a sheet of its own in its `<head>`, `/_quillo/s/<hash>.css`, which the browser
  keeps for good. A `<style>` the page writes is for what is not a component's.
- `quillo check` warns of a class a sheet styles (`` css`...` ``, a `style`, or a `.css`
  file under `public/`) that nothing in the program writes: "`.never-used` is styled here,
  but nothing in the program writes it". Text anywhere in the program counts, and so do
  its services' and the scripts under `public/`. A template's start counts for every class
  that begins with it (`` `card-${size}` `` writes any `card-...`), and so do the classes
  Quillo sets (below). For classes a library's script sets, name their starts in a comment
  in the sheet: `/* set elsewhere: leaflet- */`. A file that does not parse counts as
  writing every class its text holds, so its one error is not followed by a warning for
  each of them.
- `<script>{x}</script>` writes `x` as JSON, a JavaScript value (`<` written `\u003c`,
  so it cannot end the script): `<script>const user = {user};</script>`. HTML in it
  (`raw(code)`) goes in as it is, except that `</` becomes `<\/` and `<!--` becomes
  `<\!--`, which mean the same inside JavaScript strings and cannot end the element
  early, so `raw(json.stringify(x))` there is safe too. The script's own text, and a
  string written in it (`<script>{"if (a < b) go('x')"}</script>`), go in the same way:
  as written, quotes included, with only `</` and `<!--` escaped. A `{` in a script's text
  starts a Quillo value, so JavaScript with braces is a parse error that says what to
  do: ``a `{` in a <script> starts a Quillo value: write the code as a string,
  <script>{raw(CODE)}</script>, or put it in a file of its own``. A component's `<script>` (made
  in the browser, which runs it) takes only text written in the program. Script built
  as text outside a `<script>` element (`raw("<script>" + ... )`) is not looked at.
- `raw(s)` marks trusted HTML.
- A capitalized tag calls a view: attributes fill parameters by name, and the element's body is passed as `children`.
  An HTML tag written in capitals (`<DIV>`) is taken for a view, and the error says to
  write it in lowercase, `<div>`.
- Comments: in markup, a line starting with `//` is a comment, and so are `{/* ... */}`,
  as in JSX, and `<!-- ... -->`, as in HTML; none of them reaches the page. A `<!--` with
  no `-->` is an error. In code, `//` to the end of the line and `/* ... */` anywhere. `//` after
  other text on a line is text (`https://...`), and `{"//"}` writes the characters.
- `<textarea>{text}</textarea>`: the box shows `text`, and shows it again whenever
  `text` changes, even after someone has typed in it; while `text` stays the same,
  what was typed stays (a starting text from a prop). `<textarea value={text}>` holds
  the box to `text` instead, as with any input: typing that the state does not take is
  undone. Write one or the other, not both.
- The browser re-parses what the server sends, and some markup does not survive that:
  it ends a `<p>` before a `<div>`, `<ul>`, `<section>` or other block, ends a link
  before an `<a>` inside it, and drops a `<form>` inside a `<form>`. The page is then
  not what was written, and its components are made again in the browser. `quillo check`
  reports these as errors, a view or component counting as its outer element
  (`<p><Presence/></p>` when `Presence` is a `<div>`); one that may be either
  (`link ? <a> : <span>`) is let through.

### From HTML

HTML pasted into a literal mostly stays as it is: entities, bare attributes,
`onclick="..."`, SVG's `viewBox`, custom elements, `<template>`, `<!-- comments -->`,
`'single-quoted'` and unquoted attribute values, and a `<!DOCTYPE html>` before `<html>`.
What to change:

| HTML as written | In Quillo |
|---|---|
| `<p>one<p>two`, `<li>one<li>two`, closed by what follows | close each one: the error says where (`` `<p>` is never closed: add `</p>` before `</div>` on line 1 ``) |
| `<style>body { color: red }</style>` | `` <style>{css`body { color: red }`}</style> ``: braces in markup are code |
| `<script>if (a < b) { go() }</script>` | the code in a string, `<script>{raw(CODE)}</script>`, or in a file of its own |
| `<DIV CLASS="a">` | lowercase, `<div class="a">`: a capital starts a view's name |
| `{` and `}` in text, as a code sample in a `<pre>` has them | `{"{"}` and `{"}"}`, or the sample in a string, `<pre>{SAMPLE}</pre>` |
| a `$` before a `{` in text | `{"$"}` |

To send an HTML file unchanged, serve it with `file()` or `files()`, or a whole folder with
`quillo serve` (see [Static files](#static-files)); `raw(read_file("page.html"))` puts it
in a page of the program's, with Quillo's scripts.
