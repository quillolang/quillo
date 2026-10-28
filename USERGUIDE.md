# Quillo user guide

Everything Quillo does, and how: the language, components, routes, the standard
library, databases, services, and running it. For what Quillo is and why, see the
[README](README.md).

Quillo is a single binary, built with Cargo:

```sh
cargo build --release
./target/release/quillo run app/main.qwe --watch        # the sample app: http://127.0.0.1:8080
./target/release/quillo check app.qwe                    # find mistakes without running
./target/release/quillo build app.qwe                    # build its Rust (.qwer files) beforehand
./target/release/quillo repl                             # try code line by line
./target/release/quillo lsp                              # language server (for editors)
./target/release/quillo dap                              # debugger (for editors)
cargo test
```

`quillo run` checks the program, executes its top level and, if it declared any routes,
serves them:

| Option | |
|---|---|
| `--port N`, `--host H` | where to listen (default `$PORT` or 8080, on 127.0.0.1) |
| `--watch`, `-w` | reload when the program or a `use`d file changes, and refresh open pages |
| `--reload` | reload when files change, as in production: no errors shown on pages, no page refreshes |
| `--instances N` | run the program N times over, one event loop each, to use N cores (see Concurrency) |
| `--workers N` | requests that can wait at once (for a database, the network), each on a thread (default 64) |
| `--no-check` | skip the static checks |
| `--split` | run each `service` as a process of its own (see [Services](#services)) |
| `--service-url NAME=URL` | where service NAME runs, when not here (also `QUILLO_SERVICE_NAME`); several addresses, comma-separated, are taken in turn |
| `--base PATH` | the path the program is served under (`/shop`): its links and redirects carry it |
| `--as-service` | serve the program as a service: its `server fn`s answer other services' signed calls, not pages (see [Services](#services)) |
| `--service-name NAME` | the name other programs call this service by, when it is not the program's file name; calls signed for another service are refused |
| `--no-view-cache` | render views every time (see [Cached views](#cached-views)) |
| `--no-jobs` | run none of the program's scheduled jobs on this server |
| `--no-prefetch` | pages do not prefetch the links pointed at (see [Prefetching](#prefetching)) |
| `--register URL`, `--service S` | join a limer pool, and share live updates with its other servers; needs `LIMER_ADMIN_SECRET` (see [Concurrency](#concurrency-and-reloading)) |
| `--max-body N` | largest request body to accept, in bytes (default 10 MiB; larger ones get 413) |
| `--wt-port N` | the UDP port pages connect to over WebTransport (default: the HTTP port's number; see [Channels](#channels)) |
| `--tls-cert F`, `--tls-key F` | the certificate WebTransport presents, instead of one of its own |
| `--no-wt` | no WebTransport: pages' channels and streams use a WebSocket |
| `--log F` | the request log: `text` (the default), `json` (one object per line) or `off` (see [Logs and metrics](#logs-and-metrics)) |
| `--metrics` | answer `GET /_quillo/metrics` in Prometheus's text format |

### Desktop apps

`quillo app` runs a program in a native window instead of a browser tab: the
platform's own web view (WebKit on macOS and Linux, WebView2 on Windows) shows
its pages, and closing the window ends the program. It is built only on request:

```sh
cargo build --release --features app      # Linux also needs libwebkit2gtk-4.1-dev
./target/release/quillo app app/main.qwe --watch
```

It takes the options of `run` except `--host`, `--register` and `--service`, and
`--open PATH`, the page the window opens on (`--open /admin`; `/` by default). The
program is served on 127.0.0.1, at a port the system picks, to the window alone:
the window's first request carries a token made for this launch and trades it
for a cookie, and requests without one (other programs, pages of other sites)
get 403. Links to other sites open in the system's browser.

Editor support: [quillolang/editors](https://github.com/quillolang/editors) has a VS Code extension (highlighting,
diagnostics as you type, hover, go to definition, completion, outline, snippets, and a
debugger: see [Debugging](#debugging)). Other editors can run `quillo lsp` and `quillo dap`
directly.

Examples, in [quillolang/examples](https://github.com/quillolang/examples) (cloned as `../quillo-examples` in the commands here): [hello.qwe](https://github.com/quillolang/examples/blob/main/hello.qwe) (routes), [orders.qwe](https://github.com/quillolang/examples/blob/main/orders.qwe)
(payload-based routing and middleware), [todo.qwe](https://github.com/quillolang/examples/blob/main/todo.qwe) (server-rendered
app with a JSON API), [notes.qwe](https://github.com/quillolang/examples/blob/main/notes.qwe) (SQLite and a live component),
[scroll.qwe](https://github.com/quillolang/examples/blob/main/scroll.qwe) (infinite scroll over 100,000 rows, rendering only a window),
[compose.qwe](https://github.com/quillolang/examples/blob/main/compose.qwe) (views and components built from each other),
[layout.qwe](https://github.com/quillolang/examples/blob/main/layout.qwe) (a page described as JSON, with a live editor),
[spa.qwe](https://github.com/quillolang/examples/blob/main/spa.qwe) (a single-page app: client-side navigation, deep links, 404s),
[life.qwe](https://github.com/quillolang/examples/blob/main/life.qwe) (Conway's Game of Life, run by a timer in the browser),
[storm.qwe](https://github.com/quillolang/examples/blob/main/storm.qwe) (a dashboard under a storm of random changes, tested for leaks),
[services](https://github.com/quillolang/examples/tree/main/services) (a shop that bills through a billing service, in one process or
split apart), and [index.qwe](https://github.com/quillolang/examples/blob/main/index.qwe), which mounts all of them in one server.

A whole application: [crm](https://github.com/quillolang/apps/tree/main/crm) in [quillolang/apps](https://github.com/quillolang/apps) is a multi-tenant CRM for merchants, bloggers
and news outlets, with a shop, campaigns, a flow designer, and plugins whose rules run in
the browser and on the server.

## Language tour

### Values

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

A name right before a template calls it with the text: `` shout`hi ${name}` `` is
`` shout(`hi ${name}`) ``. The builtins `css` and `sql` return the text as it is, and
tell editors to highlight it as CSS or SQL.

Operators are as in JavaScript, with these meanings:

| Operator | Means |
|---|---|
| `+ - * %` | as in JavaScript |
| `/` | exact division: `7 / 2` is `3.5`, and a whole result stays an integer (`6 / 2` is `3`); `floor(7 / 2)` is integer division |
| `== !=` | deep equality, like comparing JSON |
| `< <= > >=` | comparison |
| `!`, `&&`, `\|\|` | not, and, or; `a && b` and `a \|\| b` give back one of their operands, so `{loading && <Spinner/>}` works |
| `??` | the right side when the left is `null` or an error value |
| `cond ? a : b` | `a` if `cond` is truthy, else `b` |

| Falsy | Truthy |
|---|---|
| `null`, `false` and error values | everything else, `0` and `""` included |

Because `""` is truthy, `name || "anonymous"` keeps an empty name: write
`name ?? "anonymous"` to replace only null, or `name != "" ? name : "anonymous"`
(`quillo check` points these out).

### Coming from JavaScript

Most expressions read as they would in JavaScript. The differences:

- `let` declares a variable (there is no `const` or `var`); values are immutable, so
  `push`, `set` and `+` return new lists and maps. Spread works in literals as in
  JavaScript: `{ ...m, name: v }`, `[...xs, 3]`.
- Named functions are `fn name(params) = expr` or `fn name(params) { ... }`; anonymous
  ones are arrows. Blocks give back their last expression, so `return` is only for
  leaving early.
- `==` compares values deeply (`[1, 2] == [1, 2]`); there is no `===`.
- There are no semicolons to forget: a line break ends a statement unless the next
  line continues it (`|`, `.method`, `&&`, `||`, `??`, `else`, `*`, `==`, or `- b` and
  `+ b` with a space). A line starting `-b` is a new statement; `quillo check` warns
  when one does nothing.
- Only `null`, `false` and error values are falsy: `0` and `""` are truthy. Code
  written by JavaScript habit then runs without failing but means something else:
  `name || "anonymous"` keeps an empty name, and `count && <Badge/>` shows the badge
  for 0. Where a value is known to be a string or a number, the checker warns about
  `||`, `&&`, `!`, `if` and `? :` on it and suggests the explicit form
  (`name != "" ? name : "anonymous"`, `count != 0 && <Badge/>`).
- `match`, `|`, patterns, `route`, `reply`, `state` and `with` effects have no
  JavaScript equivalent.
- There is no `break` or `continue`: `return` leaves the function, from a loop too,
  and from a `for … =>` that makes a list; to skip items, `filter` first. `return` is
  also an expression, like `reply`: `let m = find(xs) ?? return null`, `"a" => return 1`.
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

### Variables, functions, views

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
`quillo check` checks them before the program runs: `int float dec num str bool list map
html fn response any`. Unions are written `int | str`; add `?` or `| null` to also allow null
(`str?`, `int | null`). `list<int>` says what a list holds, and `{ title: str, tags:
list<str>, note: str? }` a map's fields; `quillo check` holds the program to them (a
call with a map missing `title`, a list of strings for `list<int>`), and at run time
they are `list` and `map`.

```
fn titles(posts: list<{ id: int, title: str }>) = posts | map((p) => p.title)
```

### Records

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
  which keeps it apart from constants such as `MAX` or `DAY`. A record may name itself
  (`type Node = { value: int, kids: list<Node> = [] }`).
- Strings in a type (`"admin" | "member"`) are checked when the program runs; `quillo
  check` takes them as `str`.

### Pipelines and method calls

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

### Control flow

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

### Patterns

The same patterns work in `match`, `let`, `for`, and route declarations:

| Pattern | Matches |
|---|---|
| `_`, `name` | anything (`name` binds it) |
| `42`, `"order"`, `true`, `null` | that literal |
| `int`, `str`, `list`, ... | any value of that type (`dec` also takes an int, numeric text or a JSON number, made a decimal) |
| `"order" \| "refund"` | either alternative |
| `int as n` | binds what matched |
| `[first, second, ..rest]` | lists |
| `{ type: "user", name, customer.id: id, limit: int = 20 }` | maps (extra keys are allowed); `a.b` looks inside nested maps; `= default` fills a missing field |
| `User`, `User as u` | what `User(v)` makes of it (see [Records](#records)) |

In a map pattern, `key: int` (a bare type) also binds `key`.

```quillo
match event {
    { type: "click", x: int, y: int } => `click at ${x},${y}`
    { type: "key", key: "Enter" | "Return" } => "enter"
    int as n if n < 0 => "negative"
    _ => "other"
}
```

### Errors

`err(x)` makes an error value. Builtins such as `int("abc")` and `json.parse`
return errors instead of crashing. `?` after an expression returns the error from
the current function, and `??` falls back when the left side is null or an error.
Field access on null gives null (so there is no need for `?.`):

```quillo
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err(`bad age: ${s}`)
    }
fn next_year(s) = parse_age(s)? + 1         // err("bad age: x") for "x"
let age = int(request.query.age) ?? 18
```

`! T` in a signature declares what the function's errors contain. It is checked: a
function declared `! str` that returns `err(42)` is a runtime error, and `quillo check`
reports it, and a `?` that would pass on the wrong kind of error, before the program runs.

Mistakes in the code itself (calling a non-function, adding a map to an int, a
failed type check, recursion more than 10,000 calls deep) are runtime errors. In a
route they become a 500 response, and the server logs them.

### Effects

`with` declares what a function may do besides computing a value:

```quillo
fn load_user(id: int) with db = db.one("select * from users where id = ?", id)
fn fetch_avatar(url: str) with http = http.get(url).body
fn profile(id: int) with db, http = { user: load_user(id), avatar: fetch_avatar(...) }
```

The effects are `db` (with `idempotent`), `http`, `fs` (files), `env` (environment variables), `time` (`now`,
`sleep`), `random` (with `crypto.token`) and `net` (`udp.send`, `udp.reply`). Inside a `with` function, a builtin that needs another effect,
or a function declared with more effects, is an error, both at runtime and in
`quillo check` (which also warns when an undeclared helper may use one). The limit holds
for everything the function calls, including the lambdas it passes to `map`.
Functions without `with` may do anything.

### HTML literals

HTML is a value, not a string. It is written like JSX: text goes in as it is,
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
  for a brace.
- `.class` and `#id` shorthand, with `{}` interpolation in the name.
- `{name}` is short for `name={name}`, and `{...map}` spreads a map's keys as attributes
  (or as a view's props): `<a {...link}>`, `<Card {...node.props}/>`. Attributes written
  out win over spread ones, whatever the case of the key (`ACTION` is `action`); classes
  from both are kept. On a view or component, spread
  keys that are not parameters are ignored, so data with extra fields can be passed
  straight through; a misspelled attribute written out is still an error.
- Attributes: `"text"` (plain, as in JSX) or `{expr}`, for example ``href={`/todos/${id}`}``.
  `true` renders a bare attribute; `false`/`null` leaves it out.
- Event handler attributes (`onclick`, any `on...`) and `srcdoc` take only `"text"`
  written in the program: the browser decodes their value and runs it, so escaping
  cannot keep data out of the code. `onmouseover={...}` is an error when the program
  loads (and in `quillo check`); handle events with `@mouseover={...}`. An `<iframe>`
  with a written `sandbox` attribute that does not say `allow-same-origin` may take
  `srcdoc={...}`: its page cannot reach the site. A view's or component's attributes
  are its parameters, so `on_change={...}` there is fine.
- `null`/`false` children render nothing; list children render each item, so `{for ... => <li/>}` works.
- Void tags (`<br>`, `<img ...>`, `<input ...>`) need no closing tag. Any tag can self-close with `/>`.
- `<html>` gets `<!doctype html>`.
- `<style>{style}</style>` takes a style sheet as it is (no escaping, so `a > b` works).
  Write it as `` css`...` ``: a template string (braces are plain text, `${...}` fills in
  values) that editors highlight as CSS. `` sql`...` `` does the same for queries.
- `<script>{x}</script>` writes `x` as JSON, a JavaScript value (`<` written `\u003c`,
  so it cannot end the script): `<script>const user = {user};</script>`. HTML in it
  (`raw(code)`) goes in as it is, except that `</` becomes `<\/` and `<!--` becomes
  `<\!--`, which mean the same inside JavaScript strings and cannot end the element
  early, so `raw(json.stringify(x))` there is safe too. A component's `<script>` (made
  in the browser, which runs it) takes only text written in the program. Script built
  as text outside a `<script>` element (`raw("<script>" + ... )`) is not looked at.
- `raw(s)` marks trusted HTML.
- A capitalized tag calls a view: attributes fill parameters by name, and the element's body is passed as `children`.
- Comments: in markup, a line starting with `//` is a comment, and so is `{/* ... */}`,
  as in JSX; in code, `//` to the end of the line and `/* ... */` anywhere. `//` after
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

## Components

A `component` is a view with state that also runs in the browser:

```quillo
component Counter(start: int = 0) {
    state count = start                  // state: set once; assigning it updates what shows it
    let double = count * 2               // recomputed on every render
    <div>
        <span>{count} (x2 = {double})</span>
        <button @click={() => { count += 1 }}>+</button>
    </div>
}

route GET / = <html><body><Counter start={5}/></body></html>
```

- The server renders it like a view, inside a `<quillo-island>` that carries its props
  and state. The page loads its bundle automatically, and the browser resumes the
  component from that state, on the server's own elements (see below). The island is
  `display: contents`, so it does not change the layout, but it is still an element:
  a child selector such as `.posts > li` does not reach the component's root element
  (write `.posts li`, or give the component's root the class). A component whose root
  is a table's or a select's part (`tr`, `td`, `th`, `thead`, `tbody`, `tfoot`,
  `caption`, `colgroup`, `option`, `optgroup`) has no island element, since the browser
  would move it out of the table: its root element carries the island (`data-qi`), so a
  row component is a row. `raw(...)` in a component is likewise in a
  `<quillo-raw style="display: contents">`, which a child selector does not see through.
- `state count = start` declares state, set once (`~count = start` is the short form).
  After that it is just `count`, and assigning to it (`count += 1`) updates what reads
  it, and nothing else. A component's code runs once. A `let` that reads state or props
  (`let double = count * 2`) is worked out again when they change. So is a statement
  that reads them (`if running { every(...) }`). Each place in the markup that shows a
  value follows only that value: changing one row's text changes that text. Assigning
  the value a state already has does nothing.
- `for` in markup keeps a row for each item: by its `key={...}`, else by its place. The
  key is the row's element's, written after `=>` or as the value of a block
  (`for p in xs { let n = p.name; <li key={n}>{n}</li> }`, whose statements the key may use).
  `key` is the list's, not passed to the view or component, so neither may take a
  parameter named `key` (a load-time error); an attribute written twice on one element
  is an error too.
  A row whose item changed is updated where it shows what changed, and moved rows keep
  their DOM (and focus). `if`, `? :`, `match` and `cond && <markup>` in markup make a
  branch again only when another one is chosen: an input in one keeps its focus and
  what was typed while the page updates. Nested components keep their own state.
- `@event={fn}` handles DOM events (`@click`, `@input`, `@scroll`, `@submit`, ...).
  The function may take the event `e`:
  - `e.value` and `e.checked` of the input, `e.key` of a key press, `e.type`;
  - `e.form`: the fields of the form, as a map;
  - `e.target`: the element itself;
  - any other field of the browser's event (`e.clientX`, `e.deltaY`, `e.shiftKey`,
    ...), then of the element (`e.scrollTop`, `e.scrollHeight`, `e.clientHeight`,
    `e.dataset`, ...);
  - its methods: `e.preventDefault()` keeps the browser from doing what it would (an
    arrow key moving the cursor, a pasted image going into a textarea). It must come
    before anything the handler waits for (a `server fn`), since by then the browser
    has gone on.

  Setting a field sets the element's, when the event has no field of that name:
  `e.value = ""` empties the input (a file picker too, so that picking the same file
  again fires `change`). `@submit` does not reload the page.
- `bind:value={x}` and `bind:checked={x}` keep an input and a variable in step.
- `every(ms, f)` in a component's code calls `f` every `ms` milliseconds, in the browser,
  for as long as the code that called it stands: `if running { every(100, step) }`
  runs a game while `running` is true, starts again with the new speed when that changes,
  and stops when `running` is false or the component leaves the page. On the server it
  does nothing.
  `listen(url, f)` follows a stream the same way (see [Streaming](#streaming)).
- Component code, and the functions, views and constants it uses, is compiled to
  JavaScript. Code that cannot run in a browser is an error when the program loads
  (and in `quillo check`): `request`, `db`, `env` (but for variables named `PUBLIC_...`),
  files, `reply`, or a global the server changes (pass it as a prop instead). `http.*`
  runs in the browser, from event handlers (see [What the browser can reach](#what-the-browser-can-reach)).
- Each page loads a bundle of its own, `/_quillo/b/<hash>.js`: the components it was
  rendered with, what they use, and the parts of the runtime they reach, minified (a
  small component is about 25 KB, 8 KB gzipped). Its address changes with its
  content, so the browser keeps it for good; a page with no components loads no
  script.
- And only when it is needed: the server's HTML shows at once, and the bundle comes
  when someone reaches for a component (hovers, touches or focuses it). What they
  typed and clicked before it came is kept and done then. A component that runs by
  itself (`every`, `live`, `js`, `@mount` in its code or markup) or that is the page (in
  `<body>` itself, an app) loads as soon as the page is idle. `/_quillo/quillo.js` has
  every component, for pages that load it themselves. Text responses are gzipped for
  browsers that accept it.
- Components are resumed, not made again: the browser runs a component's code once,
  from the state the server sent, and takes the server's elements as its own (checking that
  they are the same) instead of replacing them, so images do not reload, a scrolled
  list stays where it is and what someone typed stays typed. Each component is resumed
  when it is first reached, the others when they are needed. An element whose tag or
  text differs (a script changed the page) is made again, for that component only.
- `ask(message, { title, ok, cancel, tone })` asks in a dialog on the page and waits for
  the answer (`true` or `false`) — a handler goes on after it; `notify(message, { title,
  tone, seconds })` shows a note that goes by itself. Both are markup with `q-` classes
  in a host the page lays out (`<body data-notices="dialog-center toasts-top-right">`)
  and styles: keyboard, focus and screen readers are taken care of.
- In the browser, numbers are JavaScript numbers, so a whole float such as `2.0` prints
  as `2`. While the server renders a component it writes numbers the same way (`8`,
  `1e+21`), so the page it sends is the one the browser would make and is taken up, not
  made again. Arithmetic gives the same results on both sides. Elsewhere the server
  writes `2.0`. With `--watch`, a component the browser had to make again says which,
  and what differed, in the console.

### Composition

Views and components nest like HTML: a capitalized tag renders another view or
component, its attributes are the props, and what is between the tags arrives as the
`children` parameter.

```quillo
view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>

component Counter(label: str, start: int = 0, on_change = null) {
    state n = start
    <p>{label}: {n} <button @click={() => { n += 1; on_change && on_change(n) }}>+</button></p>
}

component Panel(header, children) {          // `header` is a named slot: any HTML
    state open = true
    <div>
        <div @click={() => { open = !open }}>{header}</div>
        {open && children}
    </div>
}

component Dashboard() {
    state total = 0
    <Card title="Counters">
        <Counter label="Apples" on_change={(v) => { total = v }}/>   // child to parent
        <Counter label="Pears" start={3}/>
        <Panel header={<b>Details</b>}><p>Last change: {total}</p></Panel>
    </Card>
}
```

- **Down:** props. **Up:** pass a function (`on_change`) and call it.
- Props are passed live: a child shows its parent's new value without either running
  again. Each component keeps its own state; give components in a list a `key={...}`
  so each keeps its own (a new key makes it afresh).
- What reads a prop follows it, as what reads state does: a place in the markup that
  shows it (`{label}`, `class={...}`), a `let` (`let doubled = count * 2`), a statement
  (`if active { every(1000, tick) }` starts and stops with it), `live(load, id)` (loads
  again when `id` changes), and a prop passed on to a child. A handler reads its value
  when it runs.
- `state x = prop` reads the prop once, when the component starts: state is the
  component's own. To follow the parent, read the prop (or a `let` of it); to start over
  when it changes, give the component a `key` that changes with it.
- A plain function used as a tag (`<Row item={x}/>` where `Row` is neither a view nor a
  component) is called again as a whole when what it is given changes.
- Views are plain functions: they have no state and add nothing to the browser bundle
  unless a component uses them.
- A component placed directly in a page gets its props from the server, so they must
  be data (no functions), and they stay as the page was made: nothing above it changes
  them. Inside another component, props can be anything (callbacks, HTML, other values),
  and follow that component's state.

### JavaScript libraries

A component can hand an element to a library the page loads (a chart, a map, an editor)
and call into it:

```quillo
component Sales(points: list) {
    state chart = null
    <canvas @mount={(el) => {
        chart = js_new("Chart", el, { type: "line", data: points })
        () => chart.destroy()                   // when the canvas leaves the page
    }}></canvas>
}

route GET / = <html><head><script src="/assets/chart.js"></script></head>
                    <body><Sales points={load_points()}/></body></html>
```

- `@mount={(el) => ...}` runs once the element is on the page, once in its life. A
  function it returns runs when the element leaves the page, or its component does.
- `js("path.to.fn", args...)` calls a function of the page's scripts, found by its dotted
  name from the page's globals (`js("L.map", el)`, `js("analytics.track", "signup")`),
  and gives its result; `js_new("Chart", ...)` makes an object of a class. The name is
  written in the code: one from data could reach anything on the page, so a variable is
  an error when the program loads, as are `eval`, `Function` and the like.
- Objects from the page (elements, a library's objects) have their properties and
  methods as fields: `el.value`, `chart.update()`; and their fields can be set:
  `el.value = ""`, `el.scrollTop += 100` (only theirs: a map's fields are made anew,
  with `set` or `{ ...m, name: v }`). `innerHTML`, `outerHTML`, `srcdoc` and `on...` are
  not set, and nothing is set to a `javascript:` URL. What runs text as code or HTML is
  refused: `eval`, `Function` and its kin, `setTimeout` and `setInterval`,
  `insertAdjacentHTML`, `document.write`, `DOMParser`, `setAttribute("on...")`, script
  elements, nodes of another document, `javascript:` URLs (in lists and maps too) given
  to the page's functions, and what steps around these checks (`Reflect.set`,
  `Object.assign`, `Function.prototype.call`). Markup in the component does all of that
  safely.
- These checks are a guard-rail against mistakes, not a security boundary. Code that
  calls into the page can do what the page's scripts can, and a program set on it can
  find ways through (a library function that writes HTML, a function of the page
  reached inside data the page gave back, a `blob:` page opened in a frame). Keep data
  out of what `js` calls, as you would in JavaScript.
- A Promise a page's function gives (`navigator.mediaDevices.getUserMedia`, `fetch`) is
  waited for where code can wait, as a `server fn` call is: in an event handler, in
  `@mount`, and in a `state` initializer: `let cam = js("navigator.mediaDevices.getUserMedia",
  { video: true })` is the stream, or an error value if it was refused. Elsewhere (a
  function `map` calls, a statement that runs again), and with `.then`, `.catch` or
  `.finally` after it, it stays a Promise: `js("load").then((x) => { data = x })`.
  `type_of` calls one `promise`.
- A name the page does not have (a library's `<script>` left out) gives an error value,
  `js: L.map is not defined on this page`, so a component can say so instead of stopping.
- On the server, `@mount` does nothing and `js` gives null.

### Single-page apps

`url()` gives `{ path, query, hash }`: of the request on the server, and of the address
bar in the browser, where what reads it follows it when it changes. Once a
component has called it, clicks on same-site links inside components change the page
without reloading; Back and Forward work. `navigate("/path")` does the same from code;
on a page where no component has called `url()`, it loads that page as a link would.
(Links with a modifier key, `target`, `download` or `data-reload` work as usual.) A link
or `navigate("#spot")` to a spot on the same page goes there as the browser does:
scrolled into view, and CSS `:target` set. `navigate("/page#spot")` goes to the spot
once the new page shows it, and stays at the top if it never does.

```quillo
component App() {
    state cart = []                               // survives moving between pages
    let path = url().path
    <div>
        <nav><a href="/">Home</a> <a href="/products">Products</a> Cart: {len(cart)}</nav>
        {match split(path, "/") | filter((s) => s != "") {
            [] => <Home/>
            ["products"] => <ProductList/>                      // loads with state items = list_products()
            ["products", id] => <ProductPage key={id} id={int(id)} on_add={(p) => { cart = cart + [p] }}/>
            _ => <NotFound {path}/>
        }}
    </div>
}

route GET /*rest = <html><body><App/></body></html>       // every path renders on the server
```

Pages load their data with `server fn`s in `state` initializers: on the server while
rendering, in the browser when you navigate to them. Give a page component a `key` from
its URL (`key={id}`) so a new product starts with fresh state. See
[spa.qwe](https://github.com/quillolang/examples/blob/main/spa.qwe), which also answers 404 on the server for unknown paths.

### Live data

`live(load, ...args)` makes component state follow the server: `load` (a `server fn`)
runs again, and what shows the state follows it, whenever something it read changes, for
anyone, not only when this page acts.

```quillo
server fn load_notes() = db.query("select * from notes order by id")

server fn add_note(text: str) = db.exec("insert into notes (text) values (?)", text)

component Notes(initial: list) {
    state notes = live(load_notes)  // every open page sees new notes
    ...
}
```

- What a loader *reads* is recorded: the tables in its queries (`from notes`,
  `join users`) and the global variables it uses (`todos`). What any code *writes* is
  announced: `insert`/`update`/`delete` through `db.*` and assignments to globals.
  This works the same with SQLite, PostgreSQL and MySQL, and with data kept in variables.
- Inside `db.transaction`, the announcement waits for the commit, and is dropped
  on rollback.
- `publish("topic")` announces anything else, and `watch("topic")` inside a loader
  subscribes to it.
- What a loader reads includes what the functions it calls read: a `me()` that looks up
  the `sessions` table makes every loader calling it run again when anyone signs in.
  `unwatched(() => me())` calls it without following what it reads. Keep what changes
  often but is not shown (view counts, last-seen times) in a table of its own, so
  writing it wakes no page.
- A write to a table wakes every loader that read the table, whatever rows it read:
  a reply in one thread runs `load_posts` again on every open thread page. To follow
  one thread only, read the table `unwatched` and watch a topic of its own, which the
  code that writes publishes:

  ```quillo
  server fn load_posts(thread: int) {
      watch(`thread:${thread}`)
      unwatched(() => db.query("select * from posts where thread = ?", thread))
  }

  server fn reply_to(thread: int, text: str) {
      db.exec("insert into posts (thread, text) values (?, ?)", thread, text)
      publish(`thread:${thread}`)
  }
  ```
- A loader runs again as a call from the page, with the page's cookies, so `session()`
  inside it is the viewer's session. So what should happen when new data arrives for a
  viewer (marking posts or messages read) is done in the loader: a component's code
  cannot call a `server fn` when state changes, but the loader is that call. A page
  whose loader writes is not prefetched (see [Prefetching](#prefetching)); it is made
  when the link is followed.
- The server renders the first value; the page keeps one WebSocket open
  (`/_quillo/live`, served by its own thread), down which the server streams each
  change to its topics, and calls the loader again. A WebSocket, because browsers
  allow only six HTTP/1.1 connections to a host across all tabs: requests left
  waiting would hold them, and server calls would queue behind them. Where a
  WebSocket cannot open, the page asks the same URL as a plain request instead.
- A server keeps at most 4096 such connections open (live data, channels, `--watch`
  reloads), and at most 256 for one client: an IPv4 address, or an IPv6 /64 network.
  Past that, a page is answered `503` and tries again later. Behind a proxy that does
  not pass the client's own connection through, every visitor is the proxy's address,
  so put the proxy's own per-client limits in front (Quillo does not read
  `X-Forwarded-For`, which a client can write).
- Only changes made through Quillo are seen: a row changed by another program
  (`psql`, a script) does not wake pages.

#### Several servers

To run an app on several Quillo servers behind [limer](https://limer.sh), let
them join a `discover` pool with a `broadcast` path; changes made through one
server then wake pages on all of them, with no message broker:

```
app.example.com {
    reverse_proxy {
        discover shop {
            allow 10.0.0.0/8
            broadcast /_quillo/announce
        }
    }
}
```

```sh
export LIMER_ADMIN_SECRET=...        # limer's admin secret
quillo run shop.qwe --port 8081 --register http://10.0.0.1:2019   # on each server
```

- Each server registers (and keeps registering, every 5 seconds; limer drops
  one that stops for 15) and sends the topics it changes to limer's
  `/broadcast`, which passes them to every member. The pool's name is the
  file's (`shop`), or `--service`.
- limer's admin secret is required: `--register` does not start without
  `LIMER_ADMIN_SECRET`. Each announcement is signed (HMAC-SHA256, with a key made
  from the secret) together with the time and a random nonce; one that is not
  signed so, more than a minute old, or seen before is refused. A server takes
  only the topics it already knows (changed there, or shown on its pages), at most
  256 a message.
- What is shared is the database: tables (`db:` topics) and `publish` topics
  are announced. Data kept in variables belongs to each server and is not.
- Sequence numbers are per server, and each carries its server's id. A page
  whose live connection lands on a different server (after a restart, or with
  round-robin balancing) loads its live data again once, rather than missing a
  change. `lb_policy sticky_cookie` keeps a browser on one server and avoids
  even that.
- Announcements are delivered once, without retries: a server that cannot be
  reached when one is sent misses it, and its pages show that change only when
  they next load the data (the next change to it, or a reload).
- Sessions need the same `QUILLO_SECRET` on each server.

### Server functions

`server fn` makes a function callable from the browser:

```quillo
server fn add_note(text: str) -> list ! str with db {
    if trim(text) == "" { return err("empty note") }
    db.exec("insert into notes (text) values (?)", text)?
    db.query("select * from notes")
}

component Notes(initial: list) {
    state notes = initial
    state draft = ""
    state problem = null
    <form @submit={() => {
        let r = add_note(draft)                    // a request to the server
        if is_err(r) { problem = r.error } else { notes = r }
    }}>
        <input bind:value={draft}/>
    </form>
}
```

In component code the call is awaited automatically. A call that fails (the
connection drops, the server errors) returns an error value, like `http.*` on the
server, so `let more = load_more() ?? []` carries on and can try again later. It works in event handlers and
in `state` initializers (`state todos = load_todos()`: the server runs it while rendering,
so the page arrives with the data). Calling one while rendering is an error.

There is no `async` or `await` to write: on the server a call simply waits (and other
requests run meanwhile); in the browser the compiler awaits it, and every function that
calls it. Calls run one after another. To run several at once, give them to
`parallel`, which returns their results in order:

```quillo
let [user, orders] = parallel(() => load_user(id), () => load_orders(id))
```

In the browser that is `Promise.all`. In a request on the server each function runs as a
coroutine of its own on the event loop (outside a request, on a thread of its own), so
their waits (database, `http.*`, `sleep`) overlap. Their computing does not: Quillo code
runs one piece at a time, so `parallel` shortens waiting, not arithmetic. Inside a
transaction they run one after another, since the others' SQL would not be part of it. A
failure stops the whole call; error values are results like any other.

Each call is a `POST /_quillo/rpc/<name>` with a JSON array of arguments, checked against
the parameter types all the way in (a `list<Role>`'s items, a record's fields; a 400
otherwise), and records made as `User(v)` makes them (other fields left out); the answer is `{"ok": value}` or `{"err": payload}`.
Only `server fn`s can be called, only with `content-type: application/json`, and only
from the program's own pages: a request a browser marks as coming from another site
(`Sec-Fetch-Site`, or an `Origin` naming another host) gets a 403. Inside one, `request`
is the current request. `reply` in a
server fn becomes the HTTP response, so the usual middleware works:

```quillo
server fn delete_all() = request | auth.required | (_) => { ... }
```

From hand-written JavaScript: `await quillo.server.add_note("text")`.

A call that must not happen twice (a payment, a transfer) is wrapped in
[`idempotent`](#idempotent-requests): the page then sends a key with each call, and
sends the call again when its answer does not come back, and the server runs it once.

#### What the browser can reach

Components and server code share a file, but not what they can touch:

- A component cannot use `db`, `request`, `env`, files, Rust (`.qwer`) functions,
  or a global the server changes: that is an error when the program loads, and in the
  editor. Top-level values that come from them (`let KEY = env("STRIPE_KEY")`,
  `let CONFIG = load_config()`) are not written into the browser's bundle either, which
  anyone can download: a component that uses one is an error, and asks a `server fn`.
- An environment variable named `PUBLIC_...` is for the browser: `env("PUBLIC_MAP_TOKEN")`,
  with the name written out, works in a component and in a top-level value it uses, and
  is written into the bundle as it was when the program loaded. That suits the keys
  services hand out for pages to use (a map's token, a search-only key, an error
  reporter's address); a secret keeps its own name, and stays on the server.
- `http.get`, `http.post` and the rest work in a component's event handlers, `@mount`
  and the functions they call: the request goes from the page, with the browser's rules
  rather than the server's. The other site must allow this one (CORS: it answers with
  `access-control-allow-origin`), or the call is an error value; the page's cookies are
  not sent to it; and it reaches what the visitor's machine can, such as a service on
  their own computer or network, not what the server can. Nothing goes through the
  server, which suits a site that limits each visitor, one that answers by where the
  visitor is, a visitor's own token kept in the page, and large files:

  ```quillo
  server fn upload_url(name: str) -> str {
      let me = session().user ?? reply 401 "Log in first"
      presign_put(me, name)                              // your bucket's signed URL
  }

  component Upload() {
      state done = false
      <input type="file" @change={(e) => {
          let f = e.target.files[0]
          done = http.put(upload_url(f.name), f).ok      // the file, as it is, to the bucket
      }}/>
  }
  ```

  A file from an input is sent as it is, with its own type. While the server renders a
  component and runs its `state` initializers, nothing goes out: an `http.*` call there
  is an error (when the program loads where it can be seen, as the page renders where
  it cannot), since it would come from the server's network. Ask a `server fn` for what
  the first render needs. Answers from another site show only the headers it allows.
- A `server fn` is the way in, and it is a public address. Anyone can send
  `POST /_quillo/rpc/<name>` with any arguments, from a script or the browser's console,
  signed in or not; refusing other sites' pages stops other sites, not people. So each
  one checks, as a route does, who is asking and whether they may touch *this* record,
  and gives its parameters types (one without a type takes any JSON):

  ```quillo
  server fn delete_note(id: int) {
      let me = session().user ?? reply 401 "Log in first"
      db.exec("delete from notes where id = ? and owner = ?", id, me)
  }
  ```

- What a `server fn` returns, and the props a page gives a component, reach the browser
  in full: `db.one("select * from users where id = ?", id)` sends `password_hash` too.
  Select the columns the page shows.
- A `server fn` that calls another site for the browser passes on what it is given: with
  a URL it takes from its arguments, anyone could have it fetch any address the server
  reaches. Write the host in the code (`` `https://api.example.com/users/${id}` ``), or
  pass `{ public_only: true }` (see [HTTP client](#http-client)); `quillo check` warns
  when neither is so.

## Routes

```quillo
route METHOD /path/:param/:typed(int)/*rest [? { query pattern }] [{ request pattern }] = expr
route METHOD /path { ... block ... }
```

- **Methods:** `GET POST PUT PATCH DELETE OPTIONS HEAD ANY`. A `HEAD` request is served by the `GET` route without a body.
- **Path params** become variables. `:id(int)` only matches what parses as an int (also `float`, `num`, `str`). `*rest` (or `*name`) takes the rest of the path.
- **Query pattern:** `? { q: str, page: int = 1 }`. Query values are text, so `int`, `float` and `bool` patterns convert them.
- **Request pattern:** matched against the `request` map. Fields: `json`, `form`, `files`, `headers`, `cookies`, `query`, `params`, `method`, `path`, `body`, `ip`. JSON is matched strictly: the string `"7"` is not an int. Form fields, headers and cookies are converted like query values. `{ json: User }` takes the body as a [record](#records).
- **Reserved:** paths under `/_quillo/` belong to Quillo (the browser bundle, server fn calls, `--watch`).
- **Order:** routes are tried in the order they are declared, and the first whose method, path and patterns all match handles the request. A later route can be the fallback.

If no route matches, the response is 404 for an unknown path, 405 for a known
path with the wrong method, and 400 when a route matched the path and method but
none of its patterns matched.

### Uploads

A form with `enctype="multipart/form-data"` sends files. Its text fields are in
`request.form`, as for any form, and each file is in `request.files` under its field's
name: `{ field, filename, type, size }` (a field with several files, from `multiple`,
is a list). The bytes stay with the request until saved:

```quillo
route POST /avatar {
    let f = request.files.photo
    if f.size > 2_000_000 { reply 413 "Photos up to 2 MB" }
    save_upload(f, `uploads/${uuid()}.png`)?          // into the program's folder
    redirect("/profile")
}
```

- `save_upload(file, path)` writes the file under the program's folder (making folders
  as needed; a path that leads outside is refused) and gives the path back, or an error
  value. `read_upload(file)` gives a text file's contents.
- `filename` is what the browser sent, without any folders; name saved files yourself
  (as above) rather than trusting it.
- The whole request counts against `--max-body` (10 MiB by default): raise it for larger
  files.

From a component, a file goes to a `server fn` as an argument: pass it the input's file,
and the function gets the same `{ field, filename, type, size }` record a form's upload
is, for `save_upload`:

```quillo
server fn add_photo(file: map) = save_upload(file, `photos/${uuid()}.jpg`)

component Photo() {
    state saved = null
    <input type="file" accept="image/*" @change={(e) => { saved = add_photo(e.target.files[0]) }}/>
}
```

Such a call is sent as multipart with an `X-Quillo-Rpc` header, which pages of other
sites cannot add without asking first; a form another site posts is refused.

### Checking code from a program

`check_program("main.qwe", { "plugins/x.qwe": text })` gives what `quillo check` says of
a program, as `{ file, line, severity, message }` maps, with some files' text given in
place of what is on disk: a tool that writes Quillo code can check it before writing
it. The code checked may come from users, so it reads only files inside the program's
folder: a `use`, `mount` or `service` path with `..` or starting with `/` is reported
as an error, not read. `delete_file(path)` removes a file or folder inside the
program's folder.

### Scripts from users

`run_script(source, vars)` runs code a program's users write (a rule on a form's field,
who may see a record) on the server, and gives its last expression's value, or an error
value saying what went wrong:

```quillo
run_script("value > 3 ? null : `at least 4, got ${value}`", { value: 5 })   // null
```

- The script sees the values in `vars` and the builtins that only compute (`len`,
  `upper`, `map`, `filter`, ...). Method calls are the same: `"x".upper` works,
  `"HOME".env` and `"f".read_file` do not, nor the program's functions.
- It cannot define functions (it can write `(x) => ...`), routes, state or markup,
  `use` files or reply.
- It has no effects of its own: a builtin such as `env` passed in `vars` still cannot
  run. A function of the program passed in `vars` runs as the program's, with what the
  program may do, so it is the program's way to let a script ask for more
  (`{ taken: (code) => db.one(...) != null }`).
- It stops after 20,000 steps (calls and loop rounds). A value it makes may be up to
  1 MB (strings, and lists and maps counted with what they hold), and each KB it makes
  costs a step: doubling a string in a loop stops there, not when the memory runs out.
- Like any code, it may nest at most 256 levels deep.

### Images

```quillo
image.info("photos/a.jpg")              // { width, height, format, alpha }, or an error
image.convert("photos/a.jpg", "cache/a-800.jpg", {
    crop: { x: 100, y: 0, w: 1200, h: 675 },   // in the image's pixels
    width: 800,                                 // shrunk to it, never enlarged
    format: "jpeg", quality: 82,                // jpeg, png or webp (lossless)
})
```

JPEG, PNG, WebP and GIF are read. `image.info` reads only the header, so it is a cheap
check that an upload is an image at all. Decoding is limited to 12,000 pixels a side
and 512 MiB, so a small file claiming to be enormous is refused before anything is
allocated; both run off the event loop. At most four conversions run at once (fewer on
a machine with fewer cores), so many uploads together cannot each take 512 MiB; the rest
wait their turn, and past 64 waiting `image.convert` gives an error value at once. Paths
stay inside the program's folder.

### Cookies and sessions

```quillo
route POST /login { form: { email: str, password: str } } {
    let user = db.one("select * from users where email = ?", email)
    if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }
    set_session({ user: user.id })
    redirect("/")
}

route GET /me {
    let id = session().user ?? reply 401 "Log in first"
    db.one("select name from users where id = ?", id)
}

route POST /logout { end_session(); redirect("/") }
```

- `set_session(map, { max_age })` replaces the session (30 days unless given),
  `session()` gives it (`{}` when there is none) and `end_session()` forgets it.
- A session is a cookie signed with HMAC-SHA256: the browser can read it but not
  change it, so keep secrets out of it. Cookies hold about 4 KB; keep an id in
  the session and the rest in the database. The signature covers its expiry, its tenant
  and the program's mount point, so it is no session past its time, for another
  tenant, or in another mounted program.
- `end_session()` removes the cookie from the browser, but a copy someone kept stays
  valid until it expires. To end sessions on the server (log out everywhere, a stolen
  laptop), keep an id in the session and check it against the database.
- The session cookie is `__Host-quillo_session` when it is `Secure` and for the whole
  site (as in production): browsers take a cookie with that prefix only from the very
  host, over HTTPS, so a sibling subdomain (another tenant's site) cannot plant one.
  Elsewhere (`--watch`, a mounted program) it is `quillo_session`. A request with two
  session cookies that are both valid here has no session: one of them was planted.
- Set `QUILLO_SECRET` to a long random string (`crypto.token()` makes one) and use the
  same one on every server. Without it, a random key is made at start: sessions end
  when the server stops. The server warns when it starts if the secret is shorter than
  32 bytes, as it does for `QUILLO_SERVICE_SECRET` and `LIMER_ADMIN_SECRET`: a short one
  can be guessed offline from one signed cookie.
- `set_cookie(name, value, { max_age, path, domain, http_only, secure, same_site })` and
  `delete_cookie(name, { path, domain })` add `Set-Cookie` to the response, whatever
  the route returns. Cookies are `HttpOnly`, `SameSite=Lax`, `Path=/` and `Secure` by
  default. `Secure` is left out with `--watch` and in desktop apps, which are served
  over plain HTTP on this machine; in production, serve the site over HTTPS (a proxy
  such as limer in front). `request.cookies` has what the browser sent, and
  `cookie(name)` one of them (or null), in any function the request runs, as
  `session()` is: a helper that reads the visitor's time zone from a cookie need not be
  handed `request` by every caller.

### What a route returns

| Value | Response |
|---|---|
| html | 200 `text/html` |
| str | 200 `text/plain` |
| map, list, number, bool | 200 JSON |
| null | 204 |
| error | 500 `{"error": ...}` |
| `respond(status, body, headers)` | exactly that |
| `redirect(url, status = 303)` | a redirect |
| `file(path, { cache })` | a file under the program's folder (see [Static files](#static-files)) |
| `stream(f, { type, status, headers })` | sent as `f` makes it (see [Streaming](#streaming)) |
| `events(f, { status, headers })` | Server-Sent Events, as `f` sends them |

`reply status [body]` ends the request right away, from anywhere: inside a
helper, a pipeline stage or a `??` fallback. This is how middleware works:

```quillo
let auth = {
    required: (req) =>
        req.headers.authorization == `Bearer ${env("TOKEN")}`
            ? req
            : reply 401 { error: "unauthorized" },
}

route GET /me = request | auth.required | (r) => { ip: r.ip }

route GET /users/:id(int) {
    let user = users | find((u) => u.id == id) ?? reply 404 { error: `no user ${id}` }
    user
}
```

### Idempotent requests

A client that sent a payment and got no answer (a timeout, a dropped connection) cannot
tell whether it was made. `idempotent(f)` lets it send the request again: a request
with an `Idempotency-Key` header runs once, and the same request again with the same
key gets the first answer, without running again. It works as Stripe's API does
([idempotent requests](https://docs.stripe.com/api/idempotent_requests); the header is
the IETF's [draft](https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/)):

```quillo
route POST /payments { json: { to: str, amount: dec } } = idempotent(() => {
    let id = pay(to, amount)?
    respond(201, { id })
})

server fn transfer(from: str, to: str, amount: str) =
    idempotent({ isolation: "serializable", retries: 5 }, () => move(from, to, amount))
```

```sh
curl localhost:8080/payments -H "Idempotency-Key: $(uuidgen)" --json '{"to": "bob", "amount": "12.50"}'
```

- The client makes up the key (a UUID; 1 to 255 visible characters), and sends the
  same one when it sends the request again. A request with the key and another method,
  path, query or body is refused with 422. One that comes while the first is still
  running gets 409, and should be sent again a moment later. Neither runs.
- The answer kept is the status, the headers (not `Set-Cookie`) and the body; a request
  again gets it with `Idempotent-Replayed: true`. Answers are kept 24 hours, then the key
  is new again: `{ max_age: "7d" }` (seconds, or a duration as for cookies) keeps them
  longer. Expired ones are deleted as requests come.
- Keys belong to the tenant, the mount point and the user: `session().user`, or the
  `user` option for a request that names its caller otherwise (`{ user:
  token.owner }`). The same key from another user is another request, and cannot read
  this one's answer. Without a user (no session), keys are the clients' own random
  values, shared by everyone who knows one.
- Without the header, `f` simply runs; `{ required: true }` answers 400 instead.
  Outside a request (a script, a job), `f` runs.
- `idempotent` answers the request with what `f` gives, as the route or server fn would
  have: it ends the request, so make it the last thing. What comes before it (an
  `auth.required`, a pattern, a 400 for a bad form) runs every time and is not kept,
  as Stripe keeps nothing for a request that failed validation.
- A server fn whose body calls it gets a key from the page with each call. The page sends the
  call again, with its key, when the connection fails, or the answer is 409 or 502 to
  504: up to three more times, a quarter, half and a whole second later.

**What is guaranteed.** The answers are kept in `_quillo_idempotency`, in the program's
database (the tenant's, with a database per tenant), and `f` runs in a transaction on
that database, which `idempotent` begins (so it takes `isolation` and `retries`, and is
called outside `db.transaction`). The answer is written in that transaction, before it
commits: the work and its answer are kept together, or neither is.

- Before running, the key is claimed in a statement of its own, so that another request
  with it sees it running (409).
- When `f` returns, its answer is written and the transaction commits; then the answer
  is sent. When `f` fails, returns an error value or replies early, the transaction
  rolls back, the claim is given up, and the answer is sent but not kept: nothing
  happened, so a retry runs again. To keep a refusal ("not enough money"), return it:
  `respond(422, { error })` commits (what `f` wrote with it too) and is kept. A server
  fn's error value is not kept: the page's retry asks again.
- If the server stops (or loses the database) part way, the database rolls the
  transaction back, and the claim is left. A minute after it was made, the next request
  with the key takes it over and runs. If the first was only slow, and is still running
  then, at most one of the two commits: the claim is taken over only while it is not
  answered, and an answer is written only by the claim's holder, in its transaction; the
  other rolls back and gets the first's answer, or 409.
- On SQLite a transaction holds the database, so a second request with the key that
  comes while the first runs waits for it, and gets its answer. On PostgreSQL and MySQL
  it gets 409.
- Work outside that database is not in the transaction: `http.post` to a payment
  provider, another `db.connect`, files. For a handler that is mostly that,
  `{ transaction: false }` runs `f` without one, and keeps every answer, failures too
  (as Stripe does), since part of the work may have happened. A server that stops there
  leaves the key running: it is not known whether the work happened, so requests with
  the key get 409 until it expires. Give the provider the same key, if it takes one,
  so that it too makes the payment once.

### Prefetching

Every page Quillo serves asks for the pages it links to before they are clicked: a
link of the same site pointed at (for 65 ms), pressed, touched or reached with Tab. By
the time the click lands, the page is there. Browsers with speculation rules (Chrome,
Edge) keep the page for the navigation themselves; others get it in the background and
keep it in their cache for 30 seconds, which the server allows for this.

- **Prefetching changes nothing.** A route asked for ahead of a click may read (the
  database, files, other servers) but not write: `db.exec` or SQL that writes, assigning
  a global variable, cookies and sessions, files, `publish`, `http.post` and the like,
  and calls to services stop it, and it is answered "not prefetched" (503, which the
  browser drops, and which is not logged). The page is then made, as usual, when the
  link is followed. So a `GET` that does something (a
  logout link, a counter) is never done by pointing at it.
- **What is left alone:** links to other sites, with `target` or `download`, to the
  same page (`#section`), those a page's components follow themselves (see
  [Single-page apps](#single-page-apps)), anything inside an element with
  `data-no-prefetch`, and every link on a connection saving data.
- A route that sets its own `cache-control` keeps it. `quillo run --no-prefetch` turns
  prefetching off.

### Streaming

A route can send its answer as it makes it: a large export row by row, a job's
progress, a model's reply word by word. The client has the head at once, and each
`send` as it is made.

```quillo
route GET /orders.csv = stream((send) => {
    send("id,total\n")
    for o in db.query("select id, total from orders") { send(`${o.id},${o.total}\n`) }
}, { type: "text/csv", headers: { "content-disposition": "attachment" } })

route GET /jobs/:id/progress = events((send) => {
    for step in range(10) {
        work(id, step)
        send({ done: step + 1, of: 10 }, "progress")   // a value, and an event name
    }
})

// A model's answer, passed on as it arrives.
route POST /ask = events((send) => http.stream("https://api.example.com/v1/chat", send, {
    body: { prompt: request.json.prompt, stream: true },
    headers: { authorization: `Bearer ${env("API_KEY")}` },
}))
```

- `stream(f)` sends what `send` is given: text as it is, other values as a line of
  JSON (newline-delimited JSON, with `type: "application/x-ndjson"`). `events(f)`
  sends Server-Sent Events: each value as JSON in an event's data, with a name if
  `send` is given one; `send(null)` is a comment, which keeps a quiet connection
  open.
- The rest of the server carries on meanwhile. A `send` waits only when the client
  reads slower than the program writes, and then it waits as a database call does,
  holding up no other request. Proxies are asked not to hold the answer back.
- When the client goes away (or the server stops), the next `send` ends the
  function, so a stream that runs until someone leaves stops when they do. An
  error in the function after the answer began cuts the connection, so the client
  knows the answer is incomplete.
- `http.stream(url, f, { method, body, headers })` reads another server's answer as
  it arrives: `f` gets each event (its data, parsed as JSON when it is, and its
  name), each line of newline-delimited JSON, or each piece of text, and may stop
  it by returning `false`. Given `send`, what arrives goes straight on. It gives
  `{ status, ok, headers }` once the answer has ended (with the `body` when it is
  not a success), or an error value when it could not be had (also when one event or
  line goes past 16 MiB without ending). The map also takes
  `redirects` and `public_only` (see [HTTP client](#http-client)).
- In a page, `receive(url, f, { method, body, headers })` in a handler sends the
  request (a file from an input as `body` goes as it is) and calls `f` with what the
  answer streams, until it ends; `listen(url,
  f)` in a component's code follows a stream for as long as the code that called it
  stands (and opens it again if the connection drops):

```quillo
component Chat() {
    state answer = ""
    <div>
        <button @click={() => receive("/ask", (part) => { answer = answer + part.text }, { body: { prompt: "hi" } })}>Ask</button>
        <p>{answer}</p>
    </div>
}

component Progress(job: int) {
    state done = 0
    listen(`/jobs/${job}/progress`, (p) => { done = p.done })
    <progress value={done} max="10" />
}
```

Each open `listen` holds one of the six connections a browser keeps to a site over
HTTP/1.1, across its tabs. That suits a job's progress or a reply being written; for
data every page follows, `live` (see [Live data](#live-data)) shares one WebSocket.
Outside the server (a program's own requests in tests), the function runs first and
the whole answer is sent at once.

### UDP

A program can take UDP packets on a port, answer them, and send its own: readings from
sensors, syslog, a game's moves, metrics.

```quillo
let readings = []

udp.listen(9000, (p) => {
    readings = readings + [json.parse(p.text)]   // pages with live(...) on it follow
    udp.reply(p, "ok")                           // sent back from port 9000
})

udp.send("10.0.0.5", 514, "<14>app started")
```

- `udp.listen(port, f, { host, queue })` is written at the top of a program. The port is
  taken when the program runs, so one that is in use is an error at once. `host` is the
  address to listen on (every one, `"0.0.0.0"`, unless given).
- `f` gets each packet: `{ text, bytes, from: { host, port }, port, at }`. `text` is the
  packet as text, or `null` when it is not UTF-8; `bytes` is its bytes as a list of
  numbers; `at` is when it came, in milliseconds since 1970.
- The handler runs like a request: waiting on a database or `http.*` holds up no one, and
  it sees the program's globals, so a page following them with `live` sees each packet.
  Packets reach it in the order they came, one at a time. An error in it is logged and
  the next packet goes on.
- Packets wait in a queue while the handler is busy (10,000 unless `queue` says
  otherwise). A full queue drops its oldest packet, as UDP would; with `--metrics`,
  `quillo_udp_received_total` and `quillo_udp_dropped_total` count them. Nothing slows
  a sender down, so where packets come in bursts faster than the handler takes them,
  give `queue` room for the largest burst.
- `udp.send(host, port, data, { from })` sends one packet; `udp.reply(packet, data)` sends
  back to where a packet came from, from the port it came to (as most protocols expect).
  `data` is text, a list of byte values, or any other value as JSON; a packet holds up to
  65,507 bytes. Both return `null`, or an error value when the host has no address or the
  system refuses the packet. They need the `net` effect.
- With `--reload` and `--watch`, a port the program still listens on stays open, and its
  packets go to the new version's handler; a port it stopped listening on is closed.
- One program on a machine listens on a port: run with `--instances`, the instances share
  the socket. A program mounted in another (`mount /x = "x.qwe"`) gets its own ports' packets.

### Channels

A channel carries messages between the server and its pages as they happen: moves in a
game, cursors, readings from a device, a chat. Pages send to the program, which answers
one page, all of them, or none.

```quillo
channel.on("game", (msg, peer) => {
    // Everyone else in the channel sees the move.
    channel.broadcast("game", { id: peer.id, pos: msg.pos }, { except: peer })
})

component Board() {
    state others = {}
    let game = channel.open("game", (m) => { others = set(others, m.id, m.pos) })
    <canvas @pointermove={(e) => game.send({ pos: [e.clientX, e.clientY] })}/>
}
```

- In a component, `channel.open(name, f)` opens a channel for as long as the code that
  called it stands (the component leaving the page closes it): `f` gets each message the
  server sends on it, and `.send(value)` sends one to the server. On the server, while
  the page is rendered, it opens nothing.
- On the server, `channel.on(name, f, { join, leave })` is written at the top of a
  program. `f(msg, peer)` gets each message from a page; `peer` is `{ id, session }`, and
  `session()` in `f` is the page's session too (as it was when the page connected), as
  is its tenant. Messages reach `f` in the order they came, one at a time, as UDP
  packets do (see [UDP](#udp)): like a request, with waits holding up no one.
- `join(peer)` decides whether a page may join: anyone who can open a page can ask, so a
  channel whose messages are not for everyone has one (`(peer) => peer.session.user !=
  null`). `leave(peer)` hears that a page left, or closed. A channel no `channel.on`
  names may be joined, to hear what the program broadcasts on it; what pages send on it
  goes nowhere.
- `channel.broadcast(name, value, { except, reliable })` sends to every page in the
  channel and gives how many; `channel.send(name, peer, value, { reliable })` sends to
  one, and gives whether it was there; `channel.peers(name)` gives the ids of the pages
  in it. They need the `net` effect, and wait for nothing: a page that cannot keep up
  misses messages rather than holding up the program.

How messages travel:

- A page opens one connection for all its channels. Where the browser has WebTransport
  (HTTP/3, over UDP), a message that fits in a datagram (about 1,100 bytes as JSON) goes
  as one: it arrives as soon as it can, may come out of order, and may be lost, as a
  game's moves or a sensor's readings can afford to be. Larger messages, and those sent
  with `{ reliable: true }` (from either side: `game.send(v, { reliable: true })`), go on
  a stream: all of them, in order.
- Where WebTransport cannot be had (the browser lacks it, UDP is blocked, the page is
  served from a name its certificate does not cover), the page uses a WebSocket, which
  carries every message reliably. The connection is opened again when it drops, and the
  channels joined again.
- The server listens for WebTransport on the UDP port with the HTTP port's number
  (`--wt-port` for another), from when the first page asks to connect. It makes a
  certificate of its own, which browsers take by its hash, and a new one before it ends
  (browsers take such certificates for two weeks at most); `--tls-cert` and `--tls-key`
  give it a real one. Behind a proxy that passes HTTP only, open the UDP port to the
  server itself, or pages use the WebSocket. `--no-wt` turns WebTransport off.
- Broadcasts reach the pages connected to this server. With `--metrics`,
  `quillo_channel_pages` counts them and `quillo_channel_dropped_total` the messages
  dropped because a queue was full.

UDP and channels together pass a device's packets on to pages as they come; see
[telemetry.qwe](https://github.com/quillolang/examples/blob/main/telemetry.qwe).

### Media

A program can take a camera's or an encoder's stream as RTP, or a page's camera, and
play it in its pages, a fraction of a second behind:

```quillo
media.rtp("cam", { video: 5004, audio: 5006 })

route GET / = <html><body><quillo-media stream="cam" controls muted/></body></html>
```

```sh
ffmpeg -f avfoundation -framerate 30 -video_size 1280x720 -i "0:0" \
  -map 0:v -c:v libx264 -tune zerolatency -profile:v baseline -pix_fmt yuv420p -g 60 \
  -f rtp rtp://127.0.0.1:5004 -map 0:a -c:a libopus -f rtp rtp://127.0.0.1:5006
```

- `media.rtp(name, { video, audio, host })` makes a stream: at the top of a program, or
  while it runs (a route, a channel's handler: a stream for each room), once for a name
  (asked again, it is the same stream). Video is
  H.264 and audio Opus, each on a port of its own (`{ port: 5004, codec: "h264" }` says
  so in full). The packets never reach Quillo code: the server puts them back in order,
  makes frames of them (it reads single NAL units, STAP-A and FU-A), and passes each on
  to every page watching, without decoding or encoding anything.
- `<quillo-media stream="cam">` in any page plays the stream: the page loads a small
  script for it (4.5 KB compressed), which decodes with the browser's own decoders
  (WebCodecs) and draws on a canvas. `controls` adds a button for sound, and `muted`
  starts without it (browsers play sound only after a click on the page). Its
  `data-state` is `connecting`, `waiting`, `playing`, `ended` or `unsupported` (no
  WebCodecs), for styling.
- Frames travel on the page's channel connection: over WebTransport, a stream per
  track; else over the WebSocket. The server keeps the video since the last keyframe, so
  a page that starts watching shows a picture at once. A page that falls behind skips
  to the next keyframe, and so does a stream that lost a packet of video.
- A connection that drops is opened again (after half a second, then longer, up to 15
  seconds) and the page goes on watching, as a page that starts does. With no frames for 3
  seconds (the camera stopped, the connection is down), `data-state` is `waiting`.
- `media.streams()` gives each stream's viewers and, per track, `frames`, `fps`, `kbps`
  and `lost` packets.
- **Pages send too.** `media.on(name, { publish })` lets pages send stream `name` (or
  every stream a name ending in `*` starts: `"call:*"`), and
  `<quillo-media publish="call:42">` sends the page's camera and microphone to it, for
  every `<quillo-media stream="call:42">` to play. The browser encodes them with WebCodecs
  (H.264 baseline and Opus, what the relay passes on) and sends the frames on its channel
  connection; the server passes them on as it does RTP, decoding nothing. `publish(peer)`
  says which pages may, as a channel's `join` does (with the page's `session()`); without
  it, any page may. A video call is a stream per person:

  ```quillo
  media.on("call:*", { publish: (peer) => session().user != null })

  component Call(room: str, me: str, others: list) {
      <div>
          <quillo-media publish={`call:${room}:${me}`}/>
          {for p in others => <quillo-media key={p} stream={`call:${room}:${p}`}/>}
      </div>
  }
  ```

  `no-video` or `no-audio` leaves one out. Given a MediaStream as its `source` (a
  canvas's, the screen's: `el.source = js("navigator.mediaDevices.getDisplayMedia")` in
  `@mount`), it sends that instead of the camera. Its `data-state` is `starting`,
  `sending`, `refused` (no camera, or `publish` said no), `ended` or `unsupported`, and
  `el.mediaStats` counts what it encoded and sent. Each person's picture goes once to the
  server, which sends it on to everyone watching (what video services call an SFU); there
  is no TURN relay for networks that block UDP, where the WebSocket carries it.
- `media.close(name)` ends a stream: it stops taking RTP, lets its ports go (for another
  stream to take), and tells its pages, whose `data-state` turns `ended`. It gives true if
  there was such a stream.
- Audio and video are each played as they come, so they are in step within about a
  tenth of a second. The camera's encoder decides the picture: keyframes every second
  or two (`-g 60` at 30 fps) let pages that start watching (or fall behind) catch up
  quickly; `-tune zerolatency` keeps the encoder from holding frames back.

### Static files

```quillo
route GET /assets/*path = file("public/" + path)
route GET /build/*path = file("dist/" + path, { cache: "1y" })   // names that change with content
```

- Each file goes out with an `ETag` and `Last-Modified`. A browser asking with a
  current copy (`If-None-Match`, `If-Modified-Since`) gets a `304` with no body.
- `Cache-Control` is `no-cache` (keep it, but ask each time) unless `cache` is given:
  seconds, or `"10m"`, `"12h"`, `"30d"`, `"1y"`, which allows keeping it that long
  without asking (`immutable`). Use it for files whose name changes with their content.
- `Range: bytes=…` gets a `206` with only those bytes (audio and video seeking); a range
  past the end gets a `416`.
- Files up to 1 MiB are kept in memory (64 MiB in all) and re-read when they change on
  disk. Larger ones are sent from disk as they are written, on a thread of their own
  with a 64 KiB buffer, so a video costs no more memory than a page. At most 256 such
  sends run at once, 8 for one client (an IPv4 address, or an IPv6 /64 network; past
  that, a `503` with `Retry-After`). A reader slower than 16 KiB/s is let go after 20
  seconds, and any send stops after an hour: a browser or player goes on from there
  with a `Range` request. Behind a proxy every visitor shares the proxy's address and so
  its 8 places; limit clients at the proxy instead. The gzipped copy of a text file is
  kept too, rather than compressed again for each request.
- Paths with `..`, or starting at `/`, get a `404`.

### Translations

```quillo
translations("locales")          // locales/en.json, locales/fr.json, locales/pt-BR.json

view Header(user) = <header>
    <a href="/">{t("nav.home")}</a>
    <span>{t("greeting", { name: user.name })}</span>
    <span>{t("cart.items", { count: len(user.cart) })}</span>
</header>
```

```json
{ "nav": { "home": "Accueil" },
  "greeting": "Bonjour, {name} !",
  "cart": { "items": { "one": "{count} article", "other": "{count} articles" } } }
```

- A request's locale is what `locale_from((request) => ...)` gives (a query parameter,
  a user's setting), else its `locale` cookie, else the best match in `Accept-Language`
  (`fr-CA` is served by `fr`, `pt` by `pt-BR`), else the default:
  `translations(dir, { default: "en" })`, or `en` when there is one. `locale()` gives it.
- Nested keys are written with dots. A key missing in a locale comes from the default
  locale; a key missing there shows as itself.
- `{name}` is replaced by that value. An entry of plural forms (`zero one two few many
  other`) picks the form for `count` by the language's rules (English, French, Russian,
  Polish, Czech, Arabic, Hebrew, Japanese and others).
- Translations are text, escaped in pages like any other; `raw(t(...))` for markup you
  wrote in them. In that form the values put in (`{name}`) are still escaped, on the
  server and in the browser: `raw(t("welcome", { name: user.name }))` keeps the
  translation's `<b>` and shows the name as text. (Only that form: `raw` of a variable
  holding `t(...)`'s text makes all of it HTML.)
- `with_locale("de", () => ...)` runs code in another locale (an email in the
  recipient's language).
- `translations({ en: {...}, fr: {...} })` takes the text directly instead of a folder.
- Components call `t` and `locale` too: a page with components carries its locale's
  text for the browser. Cached views are kept once per locale.

### Mounting programs

`mount` serves another `.qwe` program under a path prefix:

```quillo
mount /notes = "notes.qwe"
mount /shop = "shop/app.qwe"
route GET / = <a href="/notes">Notes</a>
```

The mounted program runs unchanged, with its own globals and its own database, and
sees paths without the prefix (`url().path` is `/products`, not `/shop/products`).
Its root-relative links and form actions (`href`, `src`, `action`), its redirects, its
browser bundle, its `server fn` calls, live data and page changes are all moved under the
prefix. Changes to mounted files are picked up by `--watch`. See
[index.qwe](https://github.com/quillolang/examples/blob/main/index.qwe), which mounts every example.

### Services

A part of an app can run in the same process or as a server of its own, without its
code changing. `service` names a program whose `server fn`s the others call:

```quillo
// shop.qwe
service billing = "billing.qwe"

route POST /buy {
    let inv = billing.invoice(request.form.name, 1200)?    // billing's `server fn invoice`
    <p>Invoice #{inv.id}</p>
}

// app.qwe: the app, made of both
service /shop = "shop.qwe"          // named after the file; its routes under /shop
service /billing = "billing.qwe"
```

- **In one process** (the default) a service is loaded once and shared by every program
  in the process that names it: one copy of its variables and its database. A call
  goes straight to it.
- **Split**, a service runs elsewhere: `--service-url billing=http://10.0.0.5:8081` (or
  `QUILLO_SERVICE_BILLING`) sends its calls there, `POST /_quillo/service/invoice`,
  signed with HMAC-SHA256 under `QUILLO_SERVICE_SECRET` (the same on every service).
  The signature covers the time (a call more than five minutes old is refused), the
  service it is for, the function, the tenant, the arguments and the call's id. A
  service takes only calls for itself: its name is the program's file name
  (`billing.qwe` is `billing`), or `--service-name billing` when the file is named
  otherwise. It remembers each call's id for longer than a signature lasts, so a call
  sent again, by a retry or by someone who saw it on the network, is answered as it
  was and not run again. That memory is each process's own: when a service runs at
  several addresses, use HTTPS (or a private network) between services so calls cannot
  be seen and sent to another of them. The tenant goes with the call. Only `server fn`s are offered. Its routes, under its prefix, are passed on to it,
  but not Quillo's own addresses there (`/billing/_quillo/rpc/...`, `.../service/...`,
  `.../job/...`). Run alone, a service is a program served as one:
  `quillo run billing.qwe --base /billing --as-service` (`--base`: the path it is
  served under, which its links and redirects carry; `--as-service`: see below).
- `quillo run app.qwe --split` runs each service the app declares (and those they
  declare) as a process of its own, on a port of its own, its log lines marked with its
  name, all signed with one secret (32 random bytes from the system, unless
  `QUILLO_SERVICE_SECRET` is set); they stop with it. A service whose process ends
  while the app runs is started again, the same way, after a second (twice as long each
  time it ends again soon, at most 30 s), with a line in the log.
- Either way, what crosses is data, as JSON: arguments and results are copied (a
  function arrives as `null`; a decimal stays a decimal, its scale and all), so a
  program behaves the same whichever way it runs. Optional arguments left out are not sent, so the service's own defaults apply. A
  call that cannot be made (the service not answering) is an error value: `?` passes
  it on, `??` replaces it.
- **Live data across services.** A page's `live` loader that gets its data from a service
  follows only what it reads itself: the service's tables and variables are the
  service's, and their changes wake no page of the caller. So such a loader `watch`es a
  topic, and the code that changes the data publishes it: the service itself
  (`publish("account:7")` in its `server fn`), or the caller after the call. A topic a
  service publishes while answering a call reaches the caller's pages in one process and
  split alike: it comes back with the answer, and the caller announces it to its own
  pages, through every service in between. `quillo check` warns about a loader that calls
  a service and watches nothing.

  ```quillo
  // accounts.qwe, the service
  server fn deposit(id: int, cents: int) {
      db.exec("update accounts set cents = cents + ? where id = ?", cents, id)
      publish(`account:${id}`)
  }

  // the app
  server fn load_balance(id: int) {
      watch(`account:${id}`)
      accounts.balance(id)
  }
  ```
- `quillo check` checks each call against the service's `server fn`s (their names,
  arguments and types; it may fail, so the result may be an error), the browser cannot
  call a service, and a call needs the `http` effect. Services may use one another.
- A service's `server fn`s are for other services: pages cannot call them. A service
  does not answer `/_quillo/rpc/...` (in one process, under its prefix; split, `--split`
  starts it with `--as-service`), only signed calls at `/_quillo/service/...`, whose
  arguments are checked against the parameter types all the way in, as pages' calls are.
- A program with `server fn`s and no routes is served too: a service may be only that.

When a service runs elsewhere, its calls are made to hold up:

- **Several addresses.** `--service-url billing=http://10.0.0.5:8081,http://10.0.0.6:8081`
  (or the same in `QUILLO_SERVICE_BILLING`): the same service on several servers, called
  in turn.
- **Tried again.** A call that got no connection, or a server answering 502, 503 or 504,
  is tried at the next address, a little later each time. A call sent but not answered in
  time is tried again at the same address, with the same call id: the service remembers
  the calls it answered (for eleven minutes) and gives the same answer, so the function
  runs once however often the call is sent. A call still running there is waited for.
- **Left alone when failing.** An address that fails several calls in a row is not
  called for a while (its circuit is open), then tried with one call; the others take its
  calls meanwhile. When every address is left alone, a call fails at once: an error
  value saying so.
- **Set where it is declared**, after the file: how long a try may take, how often a
  call is tried again, after how many failures in a row an address is left alone, and
  for how long (twice as long each time after):

  ```quillo
  service billing = "billing.qwe" { timeout: "2s", retries: 2, failures: 5, cooldown: "5s" }
  ```

  Those are the defaults, except `timeout` (10 s). Durations are `"500ms"`, `"2s"`,
  `"1m"`, or milliseconds. A try that runs out says so: `service billing: took more
  than 2 s (its timeout)`. They apply when a service runs elsewhere (`--split`, or at an
  address); in one process a call is a function's, which runs to its end.
- **A chain gives up together.** Each call carries the time its caller still waits for
  it, and the calls the service makes in turn wait no longer than that (their own
  `timeout` when it is shorter). So when the first link gives up, the ones below stop
  waiting too instead of finishing work nobody waits for; a call with no time left is
  not made. What a service is already doing runs to its end.
- **One id for a request, everywhere.** Each request has an id: the `x-request-id` it came
  with (a proxy's, a load balancer's), or one made for it. Its answer carries it, the
  JSON log shows it (`"rid"`), the code sees it (`request.id`), and every call to a
  service passes it on, so a request is followed across services. With `--log json`,
  each call to a service is a line of its own: the service, the function, the address,
  the status, how long it took, the tries and the id.
- **Made later.** `billing.later.invoice(name, 1200)` does not wait: the call is kept, its
  id is the result, and it is made in the background, tried every second, then a little
  later each time (up to five minutes), until it gets through. Since the service
  remembers call ids, it runs once. The calls are kept with Quillo's own tables
  (`_quillo_later`: see [Quillo's own tables](#quillos-own-tables)), so they outlive a
  restart, and servers sharing that database make each once; a program with no database
  keeps them in `quillo-system.db` next to it. Checked as the call itself is (`quillo check`).

See [services](https://github.com/quillolang/examples/tree/main/services): a shop that bills through a billing service.

## Standard library

- **General:** `print str int float dec len type_of`
- **Strings:** `upper lower trim split join contains starts_with ends_with replace slice repeat index_of`
- **Regular expressions:** `re.test(text, pattern)` (true or false), `re.find` (the first
  match, or null), `re.find_all` (every match), `re.replace(text, pattern, with)` (every
  match; `with` is text where `$1`, `$<name>`, `$0` and `$$` stand for the groups, the
  match and `$`, or a function given the match) and `re.split`. A match is
  `{ text, start, end, groups, named }`, positions counting characters as `slice` does.
  Flags come last: `"i"` (any case), `"m"` (`^` and `$` at each line) and `"s"` (`.`
  matches line breaks too). They give the same answers on the server and in the
  browser, so a pattern takes only what both read alike: characters, `.`, `[...]`,
  `\d \w \s \b` (ASCII digits and word characters, as in JavaScript) and their capitals,
  `^ $`, `(...)`, `(?:...)`, `(?<name>...)`, `|`, and `* + ? {n} {n,} {n,m}` (lazy with
  `?`). Lookaround, backreferences and `\p{...}` are errors, found by `quillo check`
  when the pattern is written in the call. In a string, a backslash is written twice:
  `"\\d+"`.
- **Translations:** `t(key, vars)`, `locale()`, `translations`, `locale_from` and
  `with_locale` (see [Translations](#translations)).
- **Markdown:** `markdown(text)` gives HTML (CommonMark with tables, strikethrough and task
  lists). HTML inside the text is shown as text and `javascript:` links are blocked, so
  it is safe for what visitors write; `markdown(text, { html: true })` keeps the HTML,
  for text you wrote, and `markdown(text, { inline: true })` keeps only what is inside a
  line (bold, italic, code, links) for a headline or a caption: no paragraph around it, and
  no headings or lists (their lines, broken by `<br>`). It gives an `html` value, shown
  as it is in markup; `str(markdown(text))` is its text, to store. In a component the
  server makes it (a request to `/_quillo/markdown`, so the page shows the same HTML,
  highlighting included), awaited like a `server fn`: call it in an event handler or a
  `state` initializer, and keep the text in state:
  `@input={(e) => { preview = str(markdown(e.value)) }}` and `<div>{raw(preview)}</div>`.
- **Lists:** `map filter find any all reduce sum min max sort sort_by reverse first last push range enumerate slice unique flatten zip take drop count group_by join_on index_of`
- **Maps:** `keys values get has set remove merge`
- **Math:** `abs round floor ceil sqrt pow`, and the three modules under
  [Numbers and data](#numbers-and-data). For money, [Decimals](#decimals) and
  [Money](#money); `format_number(x, locale, digits)` writes a number as a locale does.
- **Records:** `type User = { ... }` makes `User(fields)`, which builds and checks one
  (see [Records](#records)).
- **Errors:** `err is_err unwrap`
- **Web:** `raw escape url_encode env now file redirect respond json.parse json.stringify
  set_cookie delete_cookie session set_session end_session`. `url_encode("a b")` is
  `a%20b`, and `url_encode({ q, page: 2 })` is a query string (`q=...&page=2`, leaving
  out keys whose value is null); in the browser too.
- **Streaming:** `stream(f)` and `events(f)` answer as they go, `http.stream` reads
  another server's answer as it arrives, and `listen` and `receive` read streams in a
  page (see [Streaming](#streaming)).
- **Database:** `db.open db.exec db.query db.one db.transaction db.migrate db.connect
  db.tenants db.tenant_exists db.shards db.atomic db.system` (see [Database](#database)).
- **Crypto:** `crypto.sha256(text)` and `crypto.hmac(key, text)` (hex), `crypto.token(bytes
  = 32)` (random, URL-safe), `crypto.hash_password(pw)` (Argon2id, salted) and
  `crypto.verify_password(pw, hash)` (false for a wrong password and for anything that
  is not such a hash), `crypto.equal(a, b)` (in constant time, for comparing secrets).
  Password hashing takes tens of milliseconds and 19 MiB on purpose, runs off the event
  loop, and at most one per core at a time (others wait their turn), so a flood of
  logins slows logins down rather than taking the server's memory.
- **Effects:** `sleep(ms)`, `random()`, `random_int(lo, hi)`, `uuid()`, `read_file(path)`,
  `write_file(path, text)` (paths stay inside the program's folder). `uuid()` takes its
  bytes from the operating system, so it can serve as a session id or token; `random()`
  and `random_int()` are fast but predictable, and must not.

Safety built in:

- Text in HTML and attribute values is escaped. Keys of a `{...map}` spread on an element
  become attributes only if they are plain names that run nothing: `onclick` (in any
  case, and as `bind:onclick`), `srcdoc`, names with `:` (but `xlink:href`), `@` or
  quotes or `>` are dropped, and a key cannot replace an attribute written out, in
  another case either. Event handler attributes and `srcdoc` take only text written in
  the program (see [HTML literals](#html-literals)). `javascript:` URLs in `href`, `src`
  and the like render as `about:blank#blocked`, whatever the value's type: the text the
  attribute would say is checked, so `href={markdown(u, { inline: true })}` too. So do
  the values an SVG `<set>` or `<animate>` gives another attribute (`to`, `from`, `by`,
  each of `values`), which may be an `href`.
- CSS takes data as it is written, by design: `style={...}` is escaped like any
  attribute and `<style>{x}</style>` has its `</` rewritten, so neither can end the
  element, but what they say is CSS. CSS runs no script in today's browsers; data put
  there can still restyle the page (cover it with an overlay,
  hide or move what is on it) and load URLs (`background: url(https://elsewhere/?...)`,
  `@import`), which tells another site that the page was viewed and can carry what the
  style sheet says. Put only values you chose into styles (a colour from a list, a
  number), or check what users give against what you allow. A Content-Security-Policy
  header that keeps loading on the site (`img-src 'self'; font-src 'self'`, sent with
  `respond` or by a proxy in front) stops the beacons that go through `url()`.
- Code nested more than 256 levels deep (brackets, operators, elements, ...) is refused
  when it is parsed, so source from anywhere cannot use up the stack.
- `respond` and `redirect` refuse header values with line breaks, which would let text
  from a request add headers of its own.
- A failing request answers `internal error`; the message, with file and line, goes to
  the terminal (and to the page too with `--watch`).
- A `json:` pattern matches only a body sent as JSON (or with no content type), not a
  `text/plain` one, which a page on another site could send.
- A page on another site cannot post a form here: a `POST`, `PUT`, `PATCH` or `DELETE`
  with a form, multipart or plain-text body that the browser marks as cross-site
  (`Sec-Fetch-Site`, or an `Origin` naming another host) gets a 403. JSON from another
  site needs the browser to ask first, which nothing here answers, and programs that
  call the site (webhooks, scripts) send no `Origin`, so both work as before. For a
  path that must take such posts, such as a payment provider's return or a single
  sign-on callback, say so at the top of the program: `allow_cross_site("/sso")`.
  Nor can such a page open the WebSocket on which pages hear of live changes
  (`/_quillo/live`), which the browser would otherwise let it do.
- Globals read with `env`, `read_file`, the database or a service are never written
  into the browser bundle, nor are those made by a function that reads them
  (`let CONFIG = load_config()`); a component that uses one is a load-time error. Only
  variables named `PUBLIC_...`, read with the name written out, are.
- `http.*` in a component goes from the visitor's browser, never from the server, so it
  cannot reach the server's own network: while the server renders a component, a call
  from its code is an error before anything connects.
- `quillo check` warns about an `http.*` call on the server whose host comes from a
  value, which could let a visitor reach the server's own network (localhost, a private
  network, a cloud's metadata service), unless its options say `public_only` (see
  [HTTP client](#http-client)).
- `quillo check` warns about SQL put together from values (`${x}`, `+`, or
  `join([...])`) that reaches `db.query`, `db.one` or `db.exec`, or the same on a
  connection from `db.connect`, whether passed directly, piped (`` `...${x}` |
  db.query ``) or made first in a variable (`let q = ...`, `q += ...`); pass the values
  as `?` parameters instead. Names cannot be parameters: put a table's or a column's
  name in with `sql_name(name)` (or a list of them; quoted `"name"`, or `` `name` `` when
  the program's database is MySQL), and named parameters with
  `sql_param(name)` (or a list: `sql_param(["title", "body"])` is `:title, :body`) —
  both take only letters, digits and `_`, whatever their argument is made of
  (`` sql_name(`obj_${id}`) ``). SQL made of them, text as written (also kept in a
  variable), choices between those (`if`, `match`), joins of them with `+`, and calls of
  the program's own functions that only return such text
  (`` fn table_of(id: int) = sql_name(`obj_${id}`) ``) is not warned about:
  `let order = match sort { "new" => "created desc", _ => "score desc" }` and then
  `"select * from posts order by " + order`. A run-time set of columns, from a map `row`:
  `` `insert into t (${sql_name(keys(row))}) values (${sql_param(keys(row))})` `` with
  `row` as the parameters, or `` `update t set (${sql_name(cols)}) = (${sql_param(cols)})` ``.
  It is a warning, not a proof: SQL built in a list made with `map` is not followed.
- Inside `db.transaction` on SQLite, `sleep` and `http` calls hold up other requests
  instead of letting them write into the open transaction.

Values are immutable: `push`, `set` and `+` return new lists and maps. A map's keys are
text: `set(m, 5, x)` keys it `"5"`, and `m[5]`, `get(m, 5)` and `has(m, 5)` read that.
`set(xs, i, v)` gives a list with item `i` replaced (from the end when negative). A map
or list built in a loop with `m = set(m, k, v)` (or `push`, `merge`, `remove`), or as
`reduce`'s total with `(m, x) => set(m, ...)`, is changed in place, since nothing else
holds it: building one of 20,000 takes milliseconds. Where something else still holds
it (the loop reads `m` in the same call, say), each step copies it. `flatten` takes one
level off: `flatten(flatten(xs))` for two.

### Files of a program

`use "lib/forum.qwe"` brings another file into the program:

- Every file `use`d, and every file those `use`, adds its names to one set of names for
  the whole program. The order of `use` lines does not matter, and names reach every
  file: `ui/editor.qwe` can call a function from `lib/forum.qwe` that only `main.qwe`
  `use`s.
- So a file makes sense only in its program: `quillo check main.qwe` checks it with
  the rest, and so does the editor (a file is checked as part of the program whose main
  file, in its folder or one above, `use`s it). Go to definition finds where a name
  comes from.
- A name is defined once in a program: a second `fn helper()` in another file is an
  error that names both places, since it would replace the first everywhere. `let x`
  twice in one file gives the variable again. A function with a builtin's name is a
  warning (it replaces the builtin for the whole program); a variable may take one
  (`let count = 0`), a type's name included (`let num = 5`, `for str in names`): in
  `let`, `for` and parameters a name binds, and only in `match` does `int` test a type.
- Paths are from the main file's folder, not from the file that writes them:
  `lib/b.qwe` writes `use "lib/c.qwe"`.
- A file `use`d twice, or by several files, runs once.
- A file that cannot be parsed is reported first, since what follows may be its doing,
  and the names it defines are not also reported as unknown where they are used.

### Decimals

A `dec` is an exact decimal number, for money and whatever else is counted in cents:
`12.50m`, `5m`, `-0.01m` (the `m` is C#'s). A float is a binary fraction, so `0.1 + 0.2`
is `0.30000000000000004`; decimals are exact, so `0.1m + 0.2m == 0.3m`.

```quillo
let price = 19.99m
let total = price * 3 + 4.50m          // 64.47m
let vat = round(total * 0.21m, 2)      // 13.54m (half to even)
let each = total / 3                   // 21.49m
dec("12.50")  dec(42)  dec(0.1)        // 12.50m, 42m, 0.1m
```

- A decimal keeps its digits: `12.50m` shows as "12.50", and `1.10m * 3` is `3.30m`.
  Equality and order do not see zeros at the end (`12.5m == 12.50m`), and neither do
  map keys (`set`, `get`, `group_by`).
- It holds 28 or 29 significant digits: a 96-bit whole number and up to 28 decimals, as
  C#'s `decimal`. `+`, `-` and `*` are exact; an answer with more decimals than fit keeps
  its whole part and rounds the rest half to even, and a whole part too big is an error
  (`decimal overflow`), as with ints. `/` is exact when the answer has 28 decimals or
  fewer, else rounded half to even to 28 or 29 digits (`1m / 3` is
  `0.3333333333333333333333333333m`). Dividing by zero is an error, and `%` is the
  remainder as for ints.
- With an int, a decimal stays exact and gives a decimal. With a float it is an error,
  never a silent float: `1.5m + 0.5` says to convert one with `dec(x)` or `float(x)`.
  `==` compares a decimal and a float through the float's shortest text (`0.1m == 0.1`).
- `dec(x)` makes one from text (exactly: `dec("abc")` is an error value, as `int("abc")`
  is), an int, or a float through its shortest text (`dec(0.1)` is `0.1m`, not
  `0.1000000000000000055…`). `int(d)` keeps the whole part, `float(d)` gives the
  nearest float, `str(d)` its digits.
- `round(d, digits = 0, mode = "half_even")` gives exactly that many decimals
  (`round(12.5m, 2)` is `12.50m`; negative digits round to tens, hundreds, ...). The
  modes: `half_even` (banker's rounding, the default), `half_up` (a half away from
  zero, as taught at school), `half_down`, `up` (away from zero), `down` (toward zero),
  `ceiling` and `floor`. `floor`, `ceil` and `abs` give decimals; `sum`, `min`, `max`
  and `sort` take lists of them, and a sum stays exact.
- `math`, `stats` and `vec` work on floats: a decimal there is an error that says to
  use `float(x)`.
- `json.stringify`, and a route answering a map, write a decimal as a string
  (`"12.50"`), so nothing that reads it rounds it. `json.parse` makes no decimals:
  numbers stay numbers.
- In a pattern or a record's field, `dec` takes a decimal, an int, numeric text
  (`"12.50"`, from a query, a form or JSON) or a JSON number (through its shortest
  text), and gives a decimal: `route GET /quote ? { amount: dec }`, `type Line = {
  price: dec }`, `/pay/:amount(dec)`. A `server fn`'s `dec` parameter takes the same
  from a page. In an annotation (`fn f(x: dec)`) it takes only a decimal.
- In the browser decimals are the same, and compiled components give the server's
  answers: arithmetic, comparison, rounding, display. They cross between the server
  and the page (props, state, server fns' arguments and answers) as decimals. Ints too
  large for a JavaScript number (past 2^53) reach the page whole, as BigInts, and show
  as on the server. The browser cannot tell `2.0` from `2`, so there a whole float
  added to a decimal counts as an int.
- `quillo check` knows them: `12.5m + 1` is a `dec`, `12.5m + 0.5` an error, and `round`,
  `abs`, `floor`, `ceil`, `sum`, `min` and `max` keep the type they are given. `num` is
  still `int | float`.
- [Compiled numbers](#compiled-numbers) leave decimals to the interpreter.

**In databases.** PostgreSQL's `numeric` and MySQL's `decimal` come back as decimals with
their scale (`numeric(10, 2)` gives `19.99m`, and `12.50` stays `12.50m`), and decimals
are sent exactly. A value without a scale (as `sum` of ints gives) is an int, and one
past 28 decimals or 29 digits is its exact text. In MySQL a decimal parameter arrives
as text, which MySQL turns exactly into a `decimal` column, but into a double inside an
expression: write `cast(? as decimal(12, 2))` there.

SQLite has no decimals. A decimal is stored as its text, and a column declared
`decimal(12, 2)`, `numeric(12, 2)` or `money` is read back as decimals (a bare `numeric`,
SQLite's name for any number, gives numbers as before). But such a column has
NUMERIC affinity: SQLite turns "0.10" into the real 0.1 (and rounds past 15 digits), so
the scale is lost and large amounts are not exact. For exact amounts, declare the
column `decimal_text` (TEXT affinity: SQLite keeps the digits as written, and Quillo
still reads them as decimals), or keep whole minor units (`amount_cents integer`; see
`money.to_minor`). SQL's own sums and ordering do not work on text: add up and sort in
Quillo, or keep minor units.

### Money

`money` keeps amounts of a currency: a map `{ amount: dec, currency: "USD" }`, the
amount always with the currency's minor units (ISO 4217: JPY 0, USD 2, BHD 3). It is a
plain map, so JSON, the database and the browser take it as it is, and `money` checks
it each time it is given one. On the server and in the browser alike:

```quillo
let price = money.of(19.99m, "USD")             // { amount: 19.99m, currency: "USD" }
money.of(form.amount, "EUR") ?? reply 400       // text from a form; an error value if bad
money.of("12.345", "USD")                       // err: more decimals than USD has
money.of("12.345", "USD", "half_up")            // 12.35
money.from_minor(1250, "USD")                   // 12.50, from cents
money.to_minor(price)                           // 1999
money.add(a, b)  money.sub(a, b)  money.neg(a)  money.compare(a, b)   // -1, 0, 1
money.mul(price, 3)  money.mul(price, 0.075m, "half_up")              // rounded to cents
money.allocate(money.of(100, "USD"), [1, 1, 1])  // 33.34, 33.33, 33.33
money.format(price, "de-DE")                    // "19,99 $"
```

- `money.of(amount, currency, mode)` takes a decimal, an int or numeric text (a float
  is an error: `dec(x)` it first). An amount with more decimals than the currency has,
  bad text and an unknown currency give error values, as `int("x")` does; with a
  rounding mode (those of `round`), the amount is rounded instead. Codes are ISO 4217's
  active ones (`money.minor_units("JPY")` is 0); codes without minor units, such as XAU,
  are not money.
- `add`, `sub` and `compare` take two amounts of one currency: two currencies are an
  error, never a sum. `mul(m, factor, mode = "half_even")` multiplies by a decimal or an
  int and rounds to the currency; `round(m, mode)` rounds an amount made by hand.
  `zero(currency)`, `is_zero(m)`.
- `money.allocate(m, ratios)` splits an amount by ratios (ints or decimals; a number n
  for n equal parts) without losing or making a minor unit: each share gets its part,
  rounded down, and the units left over go one each to the first shares (Martin
  Fowler's allocation). The shares always add up to the amount.
- `money.format(m, locale)` writes it as CLDR has it for the locale, the same as
  `Intl.NumberFormat` in a browser: `$1,234.50` (en-US), `1.234,50 €` (de-DE),
  `1 234,50 €` (fr-FR, a narrow no-break space between thousands), `￥1,235` (ja-JP),
  `₹12,34,567.00` (en-IN). Without a locale it uses the program's (`locale()`, with
  [Translations](#translations)), else en-US. Covered: en-US, en-GB, en-CA, en-AU,
  en-NZ, en-IE, en-IN, en-SG, en-ZA, de-DE, de-AT, de-CH, fr-FR, fr-BE, fr-CA, fr-CH,
  it-IT, it-CH, es-ES, es-MX, es-AR, pt-BR, pt-PT, nl-NL, nl-BE, sv-SE, nb-NO, da-DK,
  fi-FI, pl-PL, cs-CZ, hu-HU, ro-RO, el-GR, tr-TR, ru-RU, uk-UA, ja-JP, zh-CN, zh-TW,
  zh-HK, ko-KR, hi-IN, id-ID, th-TH, vi-VN, ms-MY and fil-PH. Another locale is written
  as the first of its language (`de-LU` as de-DE, `fr` as fr-FR), else as en-US. The
  common currencies have their symbols (`$`, `€`, `£`, `¥`, `₹`, `CA$`, ...); others
  are written with their code (`XOF 1,235`).
- `format_number(x, locale, digits)` writes any number the same way: `format_number(
  1234567.5, "en-IN", 2)` is `"12,34,567.50"`.

To hold the program to the shape, name it: `type Money = { amount: dec, currency: str }`
makes `Money(request.json)` check what a client sends, and `quillo check` knows what the
`money` functions give.

### Numbers and data

`math` is Python's math module, `stats` its statistics module with what a data frame's
`describe` adds, and `vec` numpy's operations on lists of numbers. They are on the server
and in the browser alike, so a component can chart what a route computed:

```quillo
let totals = vec.column(orders, "total")          // [12.5, 30, 7.5, ...]
stats.describe(totals)        // { count, sum, mean, std, min, q1, median, q3, max }
stats.quantiles(totals, 10)   // deciles, as Python computes them
stats.correlation(ads, sales) // Pearson; "ranked" for Spearman
stats.linear_regression(ads, sales)   // { slope, intercept }
stats.histogram(totals, 20)   // { counts, edges }, as numpy's
vec.rolling_mean(daily, 7)    // a week's moving average
math.comb(52, 5)  math.log(8, 2)  math.fsum(xs)  math.isclose(a, b)  math.pi
```

- **`math`:** `sqrt cbrt exp exp2 expm1 log(x, base) log2 log10 log1p pow`, `sin cos tan
  asin acos atan atan2` and their hyperbolic forms, `degrees radians hypot dist`, `floor
  ceil trunc` (ints) `fabs copysign fmod remainder modf frexp ldexp`, `factorial gcd lcm
  comb perm isqrt` (ints), `isclose isfinite isinf isnan`, `prod fsum sumprod`, `erf erfc
  gamma lgamma`, and `pi e tau inf nan`. What Python raises is an error here too: a math
  domain error (`math.sqrt(-1)`, `math.log(0)`), a range error (`math.exp(1000)`), an int
  past 64 bits (`math.factorial(21)`).
- **`stats`:** `mean fmean(data, weights) geometric_mean harmonic_mean`, `median
  median_low median_high mode multimode`, `pvariance variance pstdev stdev` (each with an
  optional known mean), `quantiles(data, n = 4, method = "exclusive")` (n up to 10,000), `covariance
  correlation(x, y, method = "linear") linear_regression(x, y, proportional = false)`,
  `normal_pdf normal_cdf normal_inv` (x, mu = 0, sigma = 1); and, beyond Python's,
  `percentile(data, p)` (p from 0 to 100, or a list of them; numpy's linear rule),
  `describe`, `histogram(data, bins = 10)` (or the edges as a list), `zscore`,
  `normalize` (to 0..1) and `rank` (ties share their average rank). Statistics of no
  data, or of too little, is an error, as in Python.
- **`vec`:** `add sub mul div mod` (two lists of one length, or a list and a number; ints stay
  ints, as with the operators), `pow dot norm(xs, p = 2)`, `cumsum cumprod diff`,
  `argmin argmax argsort(xs, descending = false) clip(xs, lo, hi)`, `abs sqrt exp log
  round(xs, digits)`, `linspace(start, stop, n = 50) arange(start, stop, step) zeros ones
  full(n, x)`, `rolling_mean rolling_sum(xs, window)` (one per full window),
  `matmul(a, b)` (rows times rows, or times a list), `transpose`, and `column(rows, key)`
  (a field of each map, or an item of each list).

Answers are Python's: sums are compensated and rounded once, so `stats.mean`,
`stats.variance` and `math.fsum` give Python's exact results (its statistics module
uses fractions; this gets the same float without them), and the rest agree to the last
digit or one or two places from it. None of it runs in the interpreter: a list is read
once into floats, and a long one (over 128K items) is worked on by all the cores, in
chunks of 32K whatever the machine, so the answer does not depend on how many cores
it has. A median, a percentile or a quantile sorts only the part of the list where it
falls (cut points from a sample split the list into ranges of about equal size), and
`sort` and `vec.argsort` sort those ranges side by side. On a million floats, against
Python 3.9 (no numpy) on the same 10-core machine:

| | Quillo | Python |
|---|---:|---:|
| `stats.mean` | 0.35 ms | 214 ms |
| `stats.stdev` | 0.65 ms | 674 ms |
| `stats.median` | 1.3 ms | 44 ms |
| `stats.quantiles(xs, 100)` | 2.0 ms | 45 ms |
| `stats.correlation` | 1.4 ms | 246 ms (3.10's algorithm) |
| `stats.linear_regression` | 1.3 ms | 182 ms (3.10's algorithm) |
| `stats.describe` | 1.7 ms | 764 ms (by hand) |
| `stats.histogram(xs, 50)` | 0.6 ms | 126 ms (a loop) |
| `math.fsum` | 1.6 ms | 20 ms |
| `vec.dot` | 0.9 ms | 11 ms (`sum(map(mul, x, y))`) |
| `vec.add` | 1.9 ms | 20 ms (`list(map(add, x, y))`) |
| `vec.rolling_mean(xs, 30)` | 2.0 ms | 69 ms (a running sum) |
| `vec.argsort` | 5.4 ms | 75 ms (`sorted(range(n), key=...)`) |
| `sort` | 4.6 ms | 46 ms |

Making a list one item at a time is the interpreter's work, though: `map(range(n), (i)
=> ...)` over a million takes about 1.4 times Python's list comprehension. Built with
`vec` instead (`vec.div(vec.mod(vec.mul(vec.arange(n), k), m), 1000.0)`), the same list
takes a fifteenth of the comprehension's time.

### Dates and times

There is no date type: a date is its ISO 8601 text, `"2026-10-27"`, and a date-time is
RFC 3339 text with its offset, `"2026-10-27T14:30:00Z"` or `"2026-10-27T15:30:00+01:00"`.
Text like that sorts and compares in time order (date-times when they have the same
offset: keep them in UTC), and goes through JSON, databases, forms (`<input type="date">`)
and the browser as it is. The `date` module works on it, on the server and in the browser
alike, with the same answers:

```quillo
date.today()                          // "2026-10-27", in UTC; date.today("Europe/Paris")
date.now()                            // "2026-10-27T14:30:00Z", to the second
date.add_months("2026-01-31", 1)      // "2026-02-28": the end of the month, not March
date.to_zone("2026-10-27T14:30:00Z", "Europe/Paris")       // "2026-10-27T15:30:00+01:00"
date.at("2026-10-27", "17:00", "America/New_York")         // "2026-10-27T17:00:00-04:00"
date.days_between("2026-11-05", "2026-11-14")              // 9: the nights of a stay
date.format("2026-10-27", { locale: "de-DE", style: "long" })  // "27. Oktober 2026"
date.parse("27/10/2026", "DD/MM/YYYY")                     // "2026-10-27", or an error value
```

- **Making and reading:** `date.of(y, m, d)`, `date.valid(s)` (a date or a date-time),
  `date.parts(d)` (`{ year, month, day, weekday }`, weekday 1 for Monday to 7 for Sunday;
  a date-time adds `hour minute second offset`), `date.weekday(d)`, `date.is_leap_year(year
  or d)`, `date.days_in_month(d)` or `(year, month)`.
- **Arithmetic:** `date.add_days(d, n)`, `date.add_months(d, n)` and `date.add_years(d, n)`
  (a day past the month's end is its last day: `2024-02-29` plus a year is `2025-02-28`),
  `date.days_between(a, b)` (`b - a`, in days), `date.months_between(a, b)` (whole months:
  `2026-01-31` to `2026-02-28` is one), `date.start_of_month(d)`, `date.end_of_month(d)`.
  These take dates; `date.date_in(t, tz)` gives a date-time's date.
- **Time zones:** `date.to_zone(t, tz)` is the same instant at the zone's offset then
  (daylight saving included); `date.date_in(t, tz)` its calendar date there;
  `date.at(d, "HH:MM", tz)` the instant a local time names (a time skipped when clocks go
  forward is moved forward by the gap, 02:30 becoming 03:30; a time that happens twice
  when they go back is the first). A zone is an IANA name (`"Europe/Paris"`), `"UTC"`, or
  an offset (`"+05:30"`); left out, it is UTC. An offset of zero is written `Z`. The server
  reads the system's time zone database, or a copy built into `quillo` where there is none
  (Windows, small containers); the browser asks its own (`Intl`).
- **Text:** `date.format(d, pattern)` with `YYYY YY`, `MM M` (month number, padded or
  not), `MMMM MMM` (its name), `DD D`, `dddd ddd` (the weekday's name), `HH H` and `hh h`
  (24 or 12 hours), `mm ss A` (AM/PM) and `Z` (the offset); anything else, and text in
  `[brackets]`, is kept as it is. A date-time shows its time as written: `date.to_zone` it
  first to show another zone's. `date.format(d, { locale, style })` gives a locale's
  `"short"`, `"medium"` (the default), `"long"` or `"full"` date, and `{ locale, pattern }`
  the pattern with that language's names. The locales are English (US, and the British
  style for `en-GB`, `en-AU`, `en-IE`, `en-NZ`, `en-IN`, `en-ZA`), German, French, Spanish,
  Italian, Dutch and Portuguese, as the server and the browser both have them (CLDR's
  forms); any other gives English. `date.parse(s)` reads a date or a date-time, with `T` or
  a space and an offset (`+01`, `+0100`, `+01:00` or `Z`), and gives it back as above;
  `date.parse(s, pattern)` reads by a pattern (English names; `YY` is 2000 to 2099; with a
  time and no `Z`, UTC). Either gives an error value when the text is not a date.

A malformed date (`"2026-02-30"`, `"27/10/2026"`, a number) is an error in every function
but `date.parse` and `date.valid`. Years are 1 to 9999. `date.today` and `date.now` read
the clock, so they are the `time` effect, and scripts run with `run_script` have the rest
of `date` but not them. A program's own variable called `date` hides the module, as with
the others.

Business days, holidays and the day counts of interest are a program's own: a calendar
is a list of holidays and the arithmetic above.

### Database

`db` talks to SQLite (built in), PostgreSQL or MySQL/MariaDB, with the same functions.
SQL problems are error values, so `?` and `??` work on them. `db` is a library, not
syntax: other stores (Redis, MongoDB, ...) come in through Rust (see
[Other stores](#other-stores)).

```quillo
db.open("app.db")                                   // SQLite, next to the program; ":memory:" also works
db.open("postgres://user:pass@host:5432/app")       // PostgreSQL
db.open("mysql://user:pass@host:3306/app")          // MySQL or MariaDB (also mariadb://)
db.exec("create table if not exists todos (id serial primary key, text text)")
db.exec("insert into todos (text) values (?)", text)       // { changes, last_id }
db.query("select * from todos where id > ?", 10)           // a list of maps
db.one("select * from todos where id = :id", { id })       // a map, or null; keys the SQL does not use are ignored
db.one("insert into todos (text) values (?) returning id", text)
db.transaction(() => { ... })                              // all or nothing; nests
db.transaction({ isolation: "serializable", retries: 3 }, () => { ... })
```

Without `db.open`, the database is `$QUILLO_DB` (a path or a URL), or else
SQLite in memory. A transaction rolls back if its function fails, returns an error
value or replies early. A SQLite path is from the program's folder, not the one Quillo
runs in, and `quillo run` says which file it opened when it starts
(`quillo: database /srv/app/app.db`).

**What failed.** `db.error_kind(e)` says what kind of failure a database's error value
is, the same for SQLite, PostgreSQL and MySQL, whose words for it differ: `"unique"` (a key
or unique column has the value already), `"foreign_key"`, `"not_null"`, `"check"`,
`"conflict"` (another transaction: the database busy, a deadlock, a serialization
failure), or null for any other failure and for anything else. It needs no effect.

```quillo
let r = db.exec("insert into seats (show, seat) values (?, ?)", show, seat)
if db.error_kind(r) == "unique" { return err("Someone has just taken that seat") }
r?
```

**Isolation and retries.** `db.transaction(options, f)` takes the isolation level and
how many times to run `f` again when the database gives up on it because of another
transaction:

```quillo
fn withdraw(account: int, cents: int) with db =
    db.transaction({ isolation: "serializable", retries: 5 }, () => {
        let balance = db.one("select balance from accounts where id = ?", account).balance
        if balance < cents { return err("not enough money") }
        db.exec("update accounts set balance = balance - ? where id = ?", cents, account)?
    })
```

- `isolation` is `"serializable"`, `"repeatable_read"` or `"read_committed"`; left out,
  it is the database's default (read committed on PostgreSQL, repeatable read on MySQL).
  PostgreSQL begins with `begin isolation level ...`, MySQL runs `set transaction
  isolation level ...` before `begin`. SQLite lets one transaction write at a time, so
  its transactions are serializable whatever is asked; with options it begins `begin
  immediate`, taking the write lock at once (another process holding it is then a
  conflict, tried again, rather than a failure half way).
- `retries` (0 to 20; 0 by default) runs the whole function again after a serialization
  failure or a deadlock: PostgreSQL's SQLSTATE 40001 and 40P01, MySQL's 1213 (deadlock),
  1205 (lock wait timeout), 1020 (a row changed since the snapshot, MariaDB) and 1412 (a
  table made since the transaction began), SQLite's busy database. Each try waits a little longer
  than the last (about 10 ms, doubling up to 320 ms, give or take half, so the two that
  collided do not collide again). Any other failure ends the transaction at once. Out of
  tries, the transaction gives the database's error value.
- After such a failure the transaction is over, whatever the function does with the
  error value: PostgreSQL refuses its next statements, and MySQL has already rolled it
  back (statements after it would otherwise run outside any transaction). So they fail
  at once, and a function that carries on anyway is not committed, but tried again.
- A transaction inside another is a savepoint, and cannot change the isolation: asking
  is an error. Only the outermost one tries again; give it the options.
- The function may run more than once, so it should do nothing that cannot be taken
  back: no `http.*`, `udp.send` or `channel.send` in it (send them after the
  transaction; `quillo check` warns), and global variables it changes are not rolled
  back. What it does in the database is.
- Under serializable isolation the database finds the conflicts itself: two
  withdrawals that read the same balance cannot both commit. Under read committed (or
  for a hot row, to wait instead of failing and trying again), lock what is read before
  writing it: `select balance from accounts where id = ? for update` (PostgreSQL,
  MySQL; SQLite has no `for update`, and needs none). A balance kept as a running total
  is updated in place: `update accounts set balance = balance - ? where id = ? and
  balance >= ?`, checking `changes`.

**Migrations.** `db.migrate("migrations")` (the folder is optional; that is its default)
brings the schema up to date from SQL files applied in name order: `001_todos.sql`,
`002_tags.sql`, and so on. Each file runs once, in its own transaction, and is recorded
in a table `_quillo_migrations` with its SHA-256. A file that fails is rolled back and
stops the program from starting, with the file's name and the database's message; a
recorded file whose text changed later gets a warning, not a second run. Servers that
start together take turns (an advisory lock on Postgres, an immediate transaction on
SQLite), so each file still runs once. Call it right after `db.open`:

```quillo
db.open(env("DATABASE_URL"))
db.migrate()
```

**SQLite** runs inside the server process, with foreign keys on and WAL mode.

**PostgreSQL:**

- Placeholders can be `?`, `:name` (with a map) or Postgres's own `$1`; the first two
  are translated, skipping quotes, comments and `$$` strings. Queries are prepared, so
  values never become SQL text.
- Values convert both ways: numbers, text, booleans, `json`/`jsonb` as maps and lists,
  arrays as lists, `numeric` as [decimals](#decimals), and `uuid`, `date`, `timestamp`/`timestamptz`
  (ISO 8601, in UTC) and `interval` as strings. For other column types the value is sent
  as text and Postgres converts it.
- `last_id` is `null`: use `insert ... returning id` with `db.one`.
- Connections come from a pool (8 by default; `?pool_size=N` in the URL). A query
  releases the interpreter lock while it waits, so other requests keep running.
  A transaction keeps one connection until it ends, so other requests never see its
  changes early.
- In the server, other queries share a few connections, pipelined: a request's queries
  (its `parallel` calls' too) go to the same one, so the database reads them together.
  While those connections wait on a slow query, new requests use others, up to
  `pool_size`.
- TLS is required for a server on another machine, with its certificate and name
  checked; on this machine (`localhost`, `127.0.0.1`, `::1`, a Unix socket) it is used
  when the server offers it. The URL can say otherwise, as libpq's do: see
  [TLS to the database](#tls-to-the-database) below. Before, TLS was only preferred
  everywhere: a URL to another machine whose server has no TLS now needs
  `?sslmode=disable` to connect.

**MySQL and MariaDB** (the `mysql` feature, built in by default; `cargo build
--no-default-features` leaves it out):

- Placeholders are `?`, or `:name` with a map (translated, skipping quotes, backticks
  and comments). Statements are prepared, so values never become SQL text.
- Values convert both ways: numbers, text, `tinyint(1)` as booleans, `decimal` as
  [decimals](#decimals), MySQL's `json` as maps and lists (MariaDB's JSON is text: `json.parse` it),
  and dates and times as ISO text. `last_id` is the new row's `auto_increment` id.
- Connections come from a pool, and a query releases the interpreter lock while it
  waits, as with Postgres. A transaction keeps one connection until it ends; nested
  ones are savepoints.
- `db.migrate()` takes turns through a named lock (`get_lock`). A statement that
  changes the schema commits at once in MySQL, so a migration that fails part way
  keeps what it did before the failure: put one change in each file.
- A database per tenant: `mysql://host/app_{tenant}`, made when the program makes the
  tenant, and listed by `db.tenants()`.
- TLS is required for a server on another machine, with its certificate and name
  checked; on this machine (`localhost`, `127.0.0.1`, `::1`, `?socket=...`) it is not
  used unless the URL asks for it. See [TLS to the database](#tls-to-the-database)
  below. Before, TLS was off unless `require_ssl=true`.

#### TLS to the database

Some hosted databases sign their certificates with a certificate authority (CA) of
their own: Amazon RDS and Aurora, Google Cloud SQL (the instance's `server-ca.pem`), and
a company's servers with its private CA. The public CAs that are trusted by default do not know those, so the
connection fails with `invalid peer certificate: UnknownIssuer` until the URL names
the CA's certificates (a PEM file, as the service publishes it):

```quillo
// Amazon RDS and Aurora: their CAs' bundle, saved where the program runs, from
// https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem
db.open("postgres://app@mydb.abc123.eu-west-1.rds.amazonaws.com:5432/app?sslrootcert=/srv/app/global-bundle.pem")
db.open("mysql://app@mydb.abc123.eu-west-1.rds.amazonaws.com:3306/app?ssl_ca=/srv/app/global-bundle.pem")
// Or in $DATABASE_URL, with the password: postgres://app:...@...rds.amazonaws.com/app?sslrootcert=...
```

PostgreSQL reads the options libpq does:

| In the URL | TLS | The server's certificate |
|---|---|---|
| `sslmode=verify-full` (the default for another machine) | always | signed by a trusted CA, for the host's name |
| `sslmode=verify-ca` | always | signed by a CA in `sslrootcert`, for any name |
| `sslmode=require` | always | not checked, unless `sslrootcert` is given (then as `verify-ca`) |
| `sslmode=prefer` (the default on this machine) | when the server offers it | as with `require` |
| `sslmode=disable` | never | |

- `sslrootcert=/path/ca.pem`: the CAs to trust, in PEM, instead of the public ones.
  `sslrootcert=system` trusts the operating system's CAs (with `verify-full` only).
- `sslcert=/path/client.crt` and `sslkey=/path/client.key`: a certificate and its key
  (PEM), for a server that asks for one (`hostssl ... cert` in `pg_hba.conf`).
- `require` keeps the connection from being read, but not someone in the middle from
  posing as the server, who then has the queries and the data. Use it only where the
  network is yours; `verify-ca` with the CA's certificate costs no more. `prefer` is
  weaker still: someone in the middle can say the server has no TLS.
- `verify-ca` without `sslrootcert` is refused: any certificate from a public CA, for
  anyone's site, would pass. Where the server is reached at an address its certificate
  does not name, `verify-full` with the name in `host` and the address in `hostaddr`
  (`postgres://db.internal/app?hostaddr=10.0.0.5`) checks the name too.

MySQL and MariaDB take the MySQL driver's options:

- `ssl_ca=/path/ca.pem`: the CAs to trust instead of the public ones (with
  `built_in_roots=true`, as well as them). It asks for TLS on this machine too.
- `ssl_server_name=db.example.com`: the name the certificate is checked for, where the
  host connected to is an address or a tunnel. `verify_identity=false` is refused: the
  driver checks the name regardless.
- `verify_ca=false`: the certificate is not checked at all, which has the risk of
  Postgres's `require` above.
- `ssl_cert=/path/client.crt` and `ssl_key=/path/client.key`: a client certificate
  (`REQUIRE X509`).
- `require_ssl=true` asks for TLS on this machine; `require_ssl=false` turns it off.

### Several databases

`db` is the program's database. `db.connect(target)` opens another, with the same
functions, and its own transactions, migrations and live updates:

```quillo
db.open("app.db")
let analytics = db.connect(env("ANALYTICS_URL"))           // any kind: SQLite, Postgres, MySQL
analytics.migrate("migrations/analytics")
analytics.query("select day, sum(total) from sales group by day")
db.transaction(() => {
    db.exec("insert into orders ...")
    analytics.transaction(() => analytics.exec("insert into sales ..."))   // its own
})
```

- Call its functions on it, `analytics.query(...)` (or `sql | analytics.one`): that is
  how they know which database. Connect at the top of the program; the same target
  again is the same connection, not a new pool.
- A transaction is its own database's: one inside another on a different database
  commits or rolls back on its own.
- Live data tells them apart: a page showing `analytics`' `orders` does not refresh
  when `db`'s `orders` changes. Its topics carry a label, not the URL.
- A connection never reaches the browser (its URL may hold a password).

Mounted programs and services have their own `db` too, so one Quillo can run programs
on different databases side by side.

**Read replicas.** A PostgreSQL or MySQL database's replicas take its reads:

```quillo
db.open(env("DATABASE_URL"), { replicas: [env("REPLICA_1"), env("REPLICA_2")] })
```

- Reads (`select`, and `with` that writes nothing) go to the replicas in turn. Writes,
  reads that lock (`for update`, `for share`), anything in a transaction, and every
  read of a request after it wrote go to the primary: a request reads what it wrote.
- A replica that fails is left out for ten seconds; its read goes to the primary.
- Another request may still read from a replica that has not caught up with a write
  made just before. Reads that cannot wait for that belong in a transaction.
- `db.connect(url, { replicas: [...] })` too.

**Shards.** Data split over several databases by a key (a customer, an account):

```quillo
let users = db.shards([env("SHARD_1"), env("SHARD_2"), env("SHARD_3")])
users.migrate()                                           // each one
users.on(id).exec("insert into users (id, name) values (?, ?)", id, name)
users.on(id).one("select * from users where id = ?", id)   // the shard for the key
users.all("select count(*) as n from users")               // every shard's rows, together
```

- `.on(key)` is a connection, as `db.connect` gives (its own transactions too). Keys
  are spread by jump consistent hashing of their text, the same on every server: a
  shard added at the end takes its share of keys from each of the others and moves
  no other key (those are the rows to copy over).
- Shards may be any kind of database, and mixed.

**Joining across databases.** One SQL query cannot join tables of two databases;
`join_on` puts their rows together in the program:

```quillo
let orders = db.query("select * from orders where day = ?", today)
let people = accounts.query("select * from users where id = any(?)", orders | map((o) => o.user_id))
join_on(orders, people, "user_id", "id")      // each order with `user`: its user, or null
```

`join_on(rows, others, key, other_key = key, as)` gives each row its first match (same
value, same type) under `as`, which is `key` without `_id` (`user`), else `joined`;
`| filter((o) => o.user != null)` keeps those with one.

**One transaction over several databases.** `db.atomic` keeps all of it, or none:

```quillo
db.atomic([db, ledger], () => {
    db.exec("update accounts set balance = balance - ? where id = ?", n, from)
    ledger.exec("insert into entries (account, amount) values (?, ?)", from, -n)
})
```

- The list names the databases that take part: `db`, connections from `db.connect`, a
  shard (`users.on(id)`), or shards whole (`users`: each of them). A database the
  function uses that is not in the list is not part of it: its writes stay when the
  rest rolls back.
- It commits in two phases: each PostgreSQL and MySQL database prepares its part
  (prepared transactions, XA), the decision to commit is recorded with Quillo's own
  tables, a SQLite database (one at most) commits, and the prepared parts commit. A part
  that cannot prepare (a deferred constraint, a database gone) rolls every part back,
  as does a function that fails, returns an error value or replies early.
- A server that stops part way leaves prepared parts, which hold their locks:
  recovery finishes them a minute later (from any server of the program), committing
  those decided and rolling back the others. The first to record a decision for a
  transaction, it or recovery, decides it.
- PostgreSQL needs `max_prepared_transactions` set above 0 (it is 0 by default).
- `db.transaction` inside it nests as usual; `db.atomic` cannot begin inside a
  transaction on one of its databases.

### Quillo's own tables

Scheduled jobs (`_quillo_jobs`: each run once, whichever server it is due on), calls
made later (`_quillo_later`) and the decisions of `db.atomic` (`_quillo_atomic`) are
kept in the program's database. When there is a database per tenant they cannot be,
and go to the system database instead: `db.system(target)` (a database every server of
the program shares, for several servers), `$QUILLO_SYSTEM_DB`, or else a SQLite file
`quillo-system.db` next to the program. `db.system` also keeps them apart from the
program's own tables when there is one database. A program with no database at all
keeps its calls made later in that file too, so they outlive a restart (its jobs need
no record: one server runs them).

The answers of [idempotent requests](#idempotent-requests) (`_quillo_idempotency`) are
the exception: they are kept in the program's database, the tenant's when there is one
per tenant, so they commit in the same transaction as the work they answer for.

### Many tenants

One program can serve many tenants (customers, teams, sites), each with its own data:

```quillo
tenant_from("example.com")          // acme.example.com is tenant "acme"
db.open("data/{tenant}.db")         // a SQLite file per tenant
db.migrate()                        // run for each tenant's database when it is made

route POST /signup { form: { name: str } } {
    with_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))   // makes acme.db
    redirect(`https://${name}.example.com/`)
}
route GET / = <h1>Welcome, {tenant()}</h1>
```

- `tenant_from("example.com")` takes the part of the host before the domain;
  `tenant_from((request) => ...)` takes what the function gives for the request (a
  header, a path segment, a lookup), a name or null. `tenant()` is the current one, or
  null. A tenant's name is lowercase letters, digits, `-` and `_` (at most 63); anything
  else is no tenant, so a name is always safe in a file name or a database name.
- A function as the target says where each tenant's database is, so tenants can be on
  different kinds of database. It is asked once for each tenant, when first used; null
  means there is no such tenant. It cannot use the database it picks for (keep the list
  in another):

  ```quillo
  let registry = db.connect("registry.db")
  db.open((tenant) => registry.one("select url from tenants where name = ?", tenant)?.url)
  ```
- A `db.open` target with `{tenant}` in it is a database per tenant: a SQLite file each,
  a PostgreSQL schema each in one database with `?schema=t_{tenant}`, or a database each
  with `postgres://host/app_{tenant}` (made by you beforehand). Give schemas a prefix:
  `public`, `information_schema` and `pg_...` are refused, and a prefix keeps a tenant
  named like another schema of the database (`auth`, say) out of it.
- A tenant's database (file or schema) is made, and migrated, the first time code runs
  `with_tenant(name, ...)` for it, as when signing the tenant up. A request's host can
  name any tenant, so requests only open databases that exist: `db` calls for a made-up
  `anything.example.com` give an error, and make no file or schema.
- Each open tenant has its own connections, so give PostgreSQL a small `pool_size`. At
  most 1,024 SQLite tenants' databases, and 16 PostgreSQL ones (each keeping up to four
  connections), are open at once; others are opened again when needed. Code that runs
  for no tenant, such as a request to the bare domain, gets an error from `db`.
- Live updates stay within a tenant: a change to one tenant's table wakes only that
  tenant's pages.
- `with_tenant("acme", () => ...)` runs code for a tenant: to sign it up, or outside a
  request, in a script or a scheduled job.
- What stays shared: global variables, one copy for the whole process, so keep a
  tenant's data in its database (or in maps keyed by `tenant()`). Views that call
  `tenant()` or read the database are never served from the view cache. Cookies
  belong to the host they were set on, so each subdomain has its own.

### HTTP client

```quillo
let r = http.get("https://api.example.com/users/1")        // { status, ok, headers, body, json }
http.post(url, { name: "Ada" })                            // maps and lists are sent as JSON
http.request("PUT", url, body, { authorization: `Bearer ${token}` })
```

Every HTTP status is a response; failing to connect is an error value.
`http.stream(url, f, opts)` reads an answer as it arrives (see [Streaming](#streaming)).
Text is sent as `text/plain`, HTML as `text/html`, other values as JSON, unless the
headers give a `content-type`.

Options come after the headers (in `http.stream`, in its map):

```quillo
http.get(url, {}, { redirects: 0 })                 // a redirect is the answer, not followed
http.post(url, body, {}, { public_only: true })     // only addresses on the public internet
http.get(`https://api.example.com/rates`, { authorization: key }, { cache: 60 })
http.get(url, {}, { timeout: 5 })                   // an error value after 5 seconds
```

- `redirects`: how many to follow, from 0 to 20 (5 unless given).
- `public_only: true` connects only to the public internet: not this machine, a private
  network, or a cloud's metadata service (`169.254.169.254`), however the address is
  written, checked on the address the name gives as it connects (and again for each
  redirect). Use it whenever someone else may choose the URL. `quillo check` warns about
  a call whose host comes from a value (a parameter, `request`, text put together before
  the host ends) unless its options say `public_only`, either way: `false` says the call
  is meant to reach inside.
- `cache: seconds` (a GET's, up to a day) keeps a successful answer and gives it again to
  the same call, the same URL, headers and options, from any request, until then. Calls
  to a site that limits how often it is asked, or that charges for each, then go out once
  for everyone: `cache: 60` on the rates above asks once a minute however many pages
  show them. Failures are not kept. At most 64 MiB of answers are, those that end soonest
  going first.
- `timeout: seconds` (more than 0, up to 300; `0.5` too) is how long the whole call may
  take: then it gives an error value (`... took more than 5 s (its timeout)`). Unless given,
  a call has 10 seconds to connect and 60 in all; `http.stream`, 300 seconds of silence
  between pieces. A page that waits on a slow service waits that long.

Another option is an error, in `quillo check` when the options are written in the call,
and when it runs: `{ timout: 5 }` is not ignored.

In a component, the same calls go from the page (see [What the browser can reach](#what-the-browser-can-reach)):
`redirects: 0` makes a redirect an error value there, `timeout` is the same, and
`public_only` and `cache` are the server's. A service that needs the visitor's address, called from the server, can be
given `request.ip`.

## Checking

`quillo check app.qwe` finds mistakes without running the program, and `quillo run`
does the same first. Types are sets of possible types, and code without annotations
is `any`, so only definite mistakes are reported, and annotations make the checks
stricter. It reports unknown names (with suggestions), names used before they are
defined, wrong argument counts and types (for builtins too), operators that can never
work, division by zero, return and error types, effects, view and component props,
component code that cannot run in a browser, route mistakes, and, as warnings, unused
variables (name a variable `_x` to keep it) and calls like `last(xs)` where a variable of
yours named `last` hides the builtin. When a call to a function of your own named like a
builtin (`notify`, `first`) has the wrong arguments, the error says so: this `notify` is
the program's own, from line 1, which hides the builtin `notify`.

After a syntax error, `quillo check`, `quillo run` and the language server say which
quillo they are (`note: this is quillo 0.5.0 (f1695a3, 2026-10-27), built 2026-10-27 at
...`): code written for a newer Quillo fails to parse in an older one, such as an old
build left on the `PATH`. `quillo --version` names the commit too: `QUILLO_COMMIT` when
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

## Concurrency and reloading

One thread runs an event loop (kqueue on macOS, epoll on Linux) over every connection,
reads and parses requests, runs the program on them and writes the answers. Each request
runs as a coroutine with its own stack. A handler runs on its own from start to end,
except while it waits: a database query, `http.*` or `sleep` pauses it, the waiting is
done elsewhere (on a pool thread, or for PostgreSQL on connections all requests share,
with queries pipelined), and the loop answers other requests meanwhile. Code is written
as if it simply waited; there is no `async` or `await`. Updating shared state such as
`todos = todos | push(t)` is safe without locks, since only one handler runs at a time.

A request that only computes never leaves the loop's thread, so plain pages and JSON
cost a few microseconds each. To use more cores, `--instances N` runs the program N times
over in one process, each with an event loop of its own; the first takes the connections
and hands them out in turn. Like N processes, each instance has its own variables, while
the database is shared, and changes to it (and `publish`) reach live pages on every
instance. SQLite in memory (`:memory:`) is one database per instance, so use a file or
PostgreSQL to share data between them.

What holds up the loop is code that computes for long without waiting: a loop over a
million rows keeps every other request on its instance waiting until it ends. Quillo does
not break into it, since that would end the promise above (a handler runs on its own
between waits). Images, `crypto.hash_password`, files and Rust (`.qwer`) functions run off
the loop, and sorting and `stats`, `vec` and `math` on large lists are spread over the
cores. So put long computing in Rust (see [Rust](#rust)), or run more instances.

How long a wait may last:

- `--workers N` (64 by default) requests can wait at once, each on a pool thread; more
  wait for a thread, while requests that only compute go on being answered. PostgreSQL
  queries take no thread.
- `http.*`: 10 s to connect, 60 s in all (300 s between the parts of a stream). A call to
  a `service` elsewhere: 10 s, or its `timeout` (see [Services](#services)).
- A PostgreSQL connection: when all of the pool's are taken, a request waits up to 30 s
  for one, then gets an error.
- A query has no limit from Quillo: give the database one (on PostgreSQL,
  `alter role app set statement_timeout = '5s'`).
- Inside a SQLite transaction a wait holds up every request, since another request's SQL
  would land in the transaction: keep `http.*` and `sleep` out of them.

With `--watch`, a change to the program or a `use`d file reloads it (starting its
state afresh) and refreshes open pages. `--reload` reloads it the same way without
`--watch`'s development aids (errors on pages, refreshing them), for production
programs that change their own files, such as a plugin editor. A version that fails to load or check does not
replace the one that works; the errors are printed.

What a reload keeps, and what it starts afresh:

- **Requests in progress** finish on the version they started on: its code, its
  variables, its services in this process and its database connections (a query or a
  transaction in progress goes on, and commits or rolls back as it would have). Requests
  that come after are answered by the new version. While the new version loads, Quillo
  code waits (what waits on a database goes on); no request is dropped.
- **Sessions** are signed cookies or rows in the database, not server memory: they stay
  valid. The signing key is `QUILLO_SECRET`; without it a key made at start, which a
  reload keeps and a restart does not (everyone is signed out), so set it in production.
- **Global variables** start afresh: keep in the database what must last.
- The old version's database connections close once no request uses it (within a second
  or so of the last one). Calls made later and transactions over several databases left
  part way are looked after by whichever version is current; a version that is the
  first to use services or `db.atomic` starts what does that.

### Stopping and health

The first Ctrl-C or `SIGTERM` stops the server gracefully: it takes no new connections,
closes idle ones, answers the requests in progress (for up to 10 seconds) and exits with
status 0. A server that joined a pool with `--register` leaves it first, so limer stops
sending it requests. A second Ctrl-C stops it at once.

`GET /_quillo/health` answers `200 ok` while the server is ready for requests, and `503`
while it is stopping or its program failed to load: give it to a load balancer's health
check (limer's `health_uri`).

### Scheduled jobs

```quillo
schedule("5m", clean_sessions)                  // every 5 minutes, on the clock
schedule("0 3 * * *", nightly_report)           // cron: 03:00 UTC
schedule("@daily", () => db.exec("delete from tokens where expires < ?", now()))
```

- The first argument is a duration (seconds, or `"30s"`, `"5m"`, `"1h"`, `"1d"`), counted
  from 1970 in UTC, so that every server agrees and `"5m"` runs at :00, :05, ...; or a cron line: `minute hour
  day month weekday` in UTC, with `*`, `1-5`, `*/15` and lists, or `@hourly`, `@daily`,
  `@weekly`, `@monthly`, `@yearly`.
- A job runs on the server like a request: waiting for the database lets requests run
  meanwhile, its errors are printed, and it is logged (`POST /_quillo/job/clean_sessions
  -> 200`). A job still running when it is due again is skipped that time. Stopping the
  server lets running jobs finish, and starts no more.
- Jobs are named after their function (`{ name: "..." }` for a lambda, or to run one
  function twice); nobody outside the server can start one.
- Several servers on one database (PostgreSQL, or SQLite shared by processes on one
  machine) run each job once per due time: the first to record it in `_quillo_jobs`
  runs it. Without a database, or with one per tenant, each server runs its jobs;
  `--no-jobs` turns them off on a server. For a database per tenant, loop over your
  tenants inside the job with `with_tenant(name, () => ...)`.

### The REPL

`quillo repl` (or `quillo` alone) runs code as you type it, and prints each expression's
value:

```
quillo> let xs = [1, 2, 3]
quillo> xs | map((x) => x * 10)
[10, 20, 30]
quillo> fn double(n) {
....>   return n * 2
....> }
quillo> double(21)
42
quillo> db.open("app.db")
quillo> db.query("select * from notes")
[{ id: 1, text: "first" }]
```

- What a line defines (`let`, `fn`, assignments, `use "lib.qwe"`) stays for the lines
  after it. A statement left open (a `{` not yet closed) goes on to the next line, shown
  by `....>`.
- It runs from the folder it was started in: `use`, `db.open` and `read_file` paths are
  from there. `db.open` (or `db.connect`) gives it a database, as in a program.
- An error stops the rest of its line; what came before it stays. The static checks do
  not run here (`quillo check` runs them on a file), and routes declared here are not
  served. Ctrl-D exits.

### Debugging

`quillo dap` is a debugger for editors: it speaks the Debug Adapter Protocol on stdin and
stdout, and runs the program as `quillo run` would. In VS Code, with the extension, press F5
in a `.qwe` file (or *Quillo: Debug this file*, or the ▶ menu). A `launch.json` entry
gives the options:

```json
{
  "type": "quillo",
  "request": "launch",
  "name": "Debug the app",
  "program": "${workspaceFolder}/main.qwe",
  "args": ["--watch", "--port", "8080"],
  "stopOnEntry": false
}
```

- **Breakpoints** go on statements, in any file the program loads (`use`d files, mounted
  programs). One on a line with nothing to run (a blank line, `fn name(...) {`) moves to the
  next statement in the same function or route; outside any, it is not set.
- **Conditional breakpoints** stop when their expression is true; a **hit count** stops
  from that hit on (`3`), or at `== 3`, `> 3`, every `% 3`. **Logpoints** print their message to the
  debug console, with each `{expression}` in it worked out, and do not stop.
- **Runtime errors** (the mistakes that make a route answer 500) stop where they happen, with
  the error and the variables as they were. *Runtime errors*, in the breakpoints list, turns this off.
- **Stepping**: over, into and out of calls. A frame is a function, a view, a component, a
  route (`GET /notes/:id`) or a file's top level (`<main>`).
- **Variables**: the frame's locals, the program's globals (without its functions and the
  builtins), and the request being answered. Lists and maps open; long lists come in pages.
- **The debug console** runs code in the stopped frame: expressions, and assignments
  (`count = 0`) too. Hovering a name shows its value; hovering does not call functions.
- The program's output (`print`, the request log, errors) shows in the debug console.

When the program stops, all of it stops: no other request runs Quillo code until it goes on
(waits already under way, for a database or the network, finish meanwhile). Pausing a server
that is waiting for requests stops the next one to run.

While debugging, views are not cached (`--no-view-cache`), so a breakpoint in one stops each
time it renders, and functions are not compiled to machine code. `.qwer` functions run as they
are, without stops. It runs on macOS and Linux; not yet on Windows.

#### In the browser

A component runs in the browser too, after its first render: its event handlers and updates.
With `"browser": true` in the launch configuration (or `"edge"`), VS Code opens the page in
Chrome (or Edge) once the server listens, with its JavaScript debugger, in the same session:
a breakpoint on a line of a `.qwe` file stops on the server and in the browser, wherever that
line runs. `"url"` names the page to open (by default, the address the server listens on).

This works because in development (`--watch`, or under the debugger) a page's bundle is a
small loader for files of their own: the runtime (`quillo.js`, as it is written), and each
component, view and function at `/_quillo/src/<hash>/<file>.qwe/<Name>.js`, with a source map
from its lines to the `.qwe` file's. The browser's developer tools show them by those names,
with the `.qwe` source beside them, and a variable a statement declares carries its own name
(`count`, which the compiled code calls `$count_1`); a component's state is a signal, its
value in `.v`. Values are JavaScript's: the debug console takes JavaScript, not Quillo. In
production a page's bundle is one minified file, as before.

#### Other editors

Any editor with a Debug Adapter Protocol client (Neovim's nvim-dap, Helix, Emacs's
dape, Zed) can use `quillo dap` as an adapter it starts itself: the command `quillo`,
the argument `dap`, messages on stdin and stdout. A `launch` request takes:

| Argument | |
|---|---|
| `program` | the `.qwe` file to run (required) |
| `args` | its `quillo run` options (`["--port", "8080"]`) |
| `cwd` | the folder to run in |
| `stopOnEntry` | stop at the first statement |
| `noDebug` | run without stopping at breakpoints |

`attach` is not supported yet: the debugger starts the program itself. `browser` and
`url` are the VS Code extension's; elsewhere, open the page in a browser yourself.

### Logs and metrics

Each request is logged on stderr as `GET /path -> 200 (0.4ms)`. With `--log json` the
line is a JSON object instead, for log collectors: `time` (RFC 3339, UTC, with
milliseconds), `method`, `path`, `status`, `ms`, `ip`, `tenant` when the request had
one, and `rid` (the request's id, also sent back as `x-request-id`). `--log off` turns
the request log off; errors are still printed. A desktop window's launch token never
appears in it.

With `--metrics`, `GET /_quillo/metrics` answers in Prometheus's text format: requests
answered by status class (`quillo_requests_total`), a histogram of how long they took
(`quillo_request_duration_seconds`), requests in progress, open connections and uptime.
It is off by default, since anyone who can reach the server could read it.

## Cached views

A view's HTML depends on its arguments and on the global variables it reads. When a
view is called again with the same arguments, and none of those globals has been
assigned since, Quillo reuses the HTML it rendered last time instead of running the
view again. A page made of views is then served about as fast as a static file (the
50-row benchmark page goes from about 26,000 to 84,000 pages a second), and is still
up to date the moment something it shows changes.

- Arguments are compared by value for numbers and text, and by identity for lists and
  maps: values never change, so the same list is the same data. `<List items={products}/>`
  is reused until `products` is assigned; `<List items={[1, 2]}/>` builds a new list
  each time and is rendered each time.
- A view that reads the clock, randomness, the database, files, the environment, the
  network or the request (`url()`), prints, writes, or renders a component, runs every
  time. Database reads are not cached because a row changed outside Quillo would go
  unnoticed.
- Views inside views pass on what they read, so a cached page follows the global its
  innermost view uses.
- `quillo run --no-view-cache` turns it off (to measure, or to rule it out).

## Compiled numbers

A top-level function that only does arithmetic on ints, floats and bools is compiled to
machine code (with [Cranelift](https://cranelift.dev)) the second time it is called, for
the types of the arguments it gets: `let`, assignment, `if`, `&&`, `||`, `return`,
`for` over `range(...)` or `a..b`, `while`, and calls of other such functions, itself
included.
It then runs as it would have, only faster: `fib(32)` takes 11 ms (Node 11, Bun 21),
and a loop of ten million steps 29 ms (Node and Bun take about 280 ms for the same
loop, on doubles).

- The answers are the interpreter's: ints are 64-bit and overflow is an error, `%` by
  zero is an error, comparing NaN is an error. Where the interpreter would fail, the
  machine code gives up and the call is run again by the interpreter, which fails as
  always (the function changed nothing, so running it twice is running it once).
- What it does not take runs in the interpreter as before: strings, lists, maps,
  decimals, builtins, globals that are not functions, functions inside functions, a variable that
  changes type, and `/` of two ints, whose answer is an int or a float depending on the
  numbers (`6 / 3` is 2, `7 / 2` is 3.5).
- A function it calls that is assigned something else is not called by the old code.
- Not with a fuel limit (scripts given a budget of steps), and not in builds without
  the `jit` feature (`cargo build --no-default-features --features mysql`).

## Rust

A `.qwer` file is Rust, and a program `use`s it as it uses a `.qwe` file: its top-level
`pub fn`s are the program's functions. Here is `imaging.qwer`:

```rust
//! [dependencies]
//! image = { version = "0.25", default-features = false, features = ["jpeg"] }

/// How bright a picture is, from 0 to 255.
pub fn brightness(path: &str) -> Result<f64, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_luma8();
    Ok(img.pixels().map(|p| p.0[0] as f64).sum::<f64>() / img.len() as f64)
}
```

```quillo
use "imaging.qwer"

route GET /photos/:name = { brightness: brightness(`photos/${name}.jpg`)? }
```

- Values cross as Quillo has them: `i64` and the other integers are `int`; `f64` and
  `f32` are `float`; `bool`; `String`, `&str` and `char` are `str`; `Vec<T>` and `&[T]`
  are lists; `HashMap<String, T>` and `BTreeMap<String, T>` are maps; `Option<T>` is the
  value or `null`; `serde_json::Value` is anything. A function returning
  `Result<T, E>` gives `T`, or an error value with `E`'s message, so `?` and `??` work
  on it. `()` is `null`.
- The rest of the file is ordinary Rust: private functions, structs, `impl`s, modules,
  `use`. A `pub fn` that is generic, `async`, takes `&mut`, or has a type that cannot
  cross is not exported, and `quillo check` says why.
- The `//! [dependencies]` lines are the file's crates, as in `Cargo.toml`. They must
  be the file's first lines: below any other line, even a comment, they are not read.
  `serde_json` is always there.
- `quillo check` reads the functions' types from the Rust, without building anything:
  calls are checked, and editors show them on hover. They run on the server only;
  components call them through a `server fn`.

How it runs:

- `use` builds the file with `cargo` (in a crate of its own in Quillo's cache,
  `~/.cache/quillo/native`, or `QUILLO_NATIVE_CACHE`; the crates' builds are shared), and
  keeps the library in `.quillo/native/` next to the file, named by a hash of the file's
  content: the next start loads it without building. A build takes seconds the first
  time, more with large crates.
- `quillo build app.qwe` builds every `.qwer` file a program uses. Deploy `.quillo/native/`
  with the program, and the server needs no Rust: it loads what was built for that
  content (on the same kind of machine).
- A call runs off the event loop, without the lock Quillo code holds: a slow function
  holds up no one, and calls from several requests run on several cores at once. The
  values cross as JSON, which suits most calls; a list of millions of numbers takes a
  while to cross.
- A panic is an error in the request that called the function, like any other; the
  server goes on. Arguments of the wrong type are refused before the function runs.
- With `--watch` and `--reload`, a `.qwer` file that changes is built again and the
  program reloaded with it. A build that fails prints the compiler's errors, and the
  program goes on as it was. Requests in progress finish with the version they started
  with: libraries stay loaded.
- Rust can do anything the server can, so `.qwer` files are the developers', like the
  rest of the program: never let a program's users write them (they write sandboxed
  Quillo with `run_script`).

See [rust.qwe](https://github.com/quillolang/examples/blob/main/rust.qwe) and [rust.qwer](https://github.com/quillolang/examples/blob/main/rust.qwer).

### Other stores

`db` is for SQL databases. For another kind (Redis, MongoDB, a graph database), use its
crate in a `.qwer` file: its functions are the driver, and the program calls them like
any other. A count of visits in Redis:

```rust
//! [dependencies]
//! redis = "0.27"

use std::sync::OnceLock;

/// The client, made on the first call (`REDIS_URL`, or Redis on this machine).
fn client() -> Result<&'static redis::Client, String> {
    static C: OnceLock<redis::Client> = OnceLock::new();
    if let Some(c) = C.get() {
        return Ok(c);
    }
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let c = redis::Client::open(url).map_err(|e| e.to_string())?;
    Ok(C.get_or_init(|| c))
}

/// The value at `key`, or null.
pub fn cache_get(key: &str) -> Result<Option<String>, String> {
    let mut con = client()?.get_connection().map_err(|e| e.to_string())?;
    redis::cmd("GET").arg(key).query(&mut con).map_err(|e| e.to_string())
}

/// Adds one to the number at `key`, and gives the new number.
pub fn cache_incr(key: &str) -> Result<i64, String> {
    let mut con = client()?.get_connection().map_err(|e| e.to_string())?;
    redis::cmd("INCR").arg(key).query(&mut con).map_err(|e| e.to_string())
}
```

```quillo
use "redis.qwer"

server fn visits(page: str) {
    watch(`visits:${page}`)                  // live pages showing this follow the topic
    int(cache_get(`visits:${page}`)? ?? "0")
}

server fn visit(page: str) {
    let n = cache_incr(`visits:${page}`)?
    publish(`visits:${page}`)                // and refresh when it changes
    n
}

component Visits(page: str) {
    state n = live(visits, page)
    <button @click={() => visit(page)}>{n} visits</button>
}
```

- Values cross as JSON, so a connection cannot be a Quillo value: the Rust keeps it in a
  `static`, and the library stays loaded between calls. (This one connects on each call;
  a pool, such as the crate's `r2d2` feature, keeps connections open.)
- Calls run off the event loop, so a slow store holds up no one, and a store that is
  down gives an error value.
- What `db` does by itself is the program's to do here: live pages follow what a loader
  `watch`es and a write `publish`es, as above, and writes are not part of
  `db.transaction`.

## Not built yet

- `match` exhaustiveness in the checker
- compiling components to WebAssembly (they compile to JavaScript)
- a formatter

## Layout

```
src/main.rs        CLI: run, app, check, repl, lsp
src/lib.rs         the crate: what the binary and the tests use

The language
src/lexer.rs       tokens; newline handling
src/parser.rs      recursive descent: statements, expressions, patterns, HTML, routes, components
src/ast.rs         the syntax tree, types, effects
src/interp.rs      evaluator, pattern matching, HTML and component rendering, effects
src/jit.rs         numeric functions compiled to machine code (Cranelift)
src/value.rs       values, maps, scopes
src/error.rs       errors with a file and line
src/record.rs      records: type User = { ... }, made, checked, in patterns
src/check.rs       the static checker (quillo check, quillo run, the language server)
src/script.rs      run_script: code a program's users write, sandboxed
src/native.rs      Rust in a program: .qwer files read, built, loaded and called

The standard library
src/builtins.rs    the builtins every program can call
src/numeric.rs     math, stats and vec: Python's math and statistics, numpy's operations
src/decimal.rs     dec: exact decimals, for money
src/money.rs       money: amounts in a currency, its minor units and formats
src/par.rs         a pool of threads for long lists, in chunks of a fixed size
src/crypto.rs      crypto: hashes, signatures, tokens, passwords
src/imaging.rs     image: what an image file is, and new files made from it
src/highlight.rs   highlight(code, lang): source code as HTML
src/i18n.rs        translations
src/fetch.rs       http: the HTTP client, its cache, and http.stream

Databases
src/db.rs          db: SQLite, connections, tenants, transactions, and the shared API
src/pg.rs          PostgreSQL (pool, pipelining, placeholders, types)
src/my.rs          MySQL and MariaDB (the mysql feature)
src/migrate.rs     db.migrate: schema files applied once each
src/txn.rs         db.transaction's isolation levels, and trying again after a conflict
src/sqlinfo.rs     which tables a statement reads and writes, for live updates

Serving
src/httpd.rs       the event loop: connections, requests as coroutines, waiting off the loop
src/server.rs      request matching, server fns, responses, --watch
src/stream.rs      answers sent as they are made: stream, events, send
src/service.rs     services: in this process or elsewhere, signed calls, --split
src/session.rs     cookies and sessions
src/idempotency.rs idempotent(f): requests with an Idempotency-Key run once
src/upload.rs      files sent with a form
src/statics.rs     files served with file()
src/site.rs        what a program sets about its site: tenants, cross-site posts
src/jobs.rs        jobs run on a schedule
src/viewcache.rs   reusing views' HTML while their inputs are unchanged
src/ws.rs          WebSockets, for live updates, --watch reloads and channels
src/udp.rs         UDP: udp.listen, udp.send, udp.reply
src/wt.rs          channels between the server and its pages, over WebTransport
src/media.rs       RTP from cameras and encoders, passed on to <quillo-media>
src/cluster.rs     several servers behind limer: --register, broadcasts
src/metrics.rs     what GET /_quillo/metrics reports
src/shared.rs      the interpreter lock and other state shared between threads
src/alloc.rs       the memory allocator: the system's on event loops, mimalloc elsewhere
src/stack.rs       room on the stack for deep recursion
src/desktop.rs     quillo app: the program in a native window

The browser and editors
src/client.rs      compiles components to JavaScript
src/client/runtime.js  the browser runtime: values, standard library, signals and markup
src/island.rs      when a page loads its components' code
src/lsp.rs         the language server

app/               a sample app to start from (main.qwe, lib/, ui/, migrations/, public/)
tests/             the language, routes, records, effects, the checker, numbers, streams,
                   UDP, Rust (.qwer), services, sessions, uploads, tenants, several
                   databases, the compiler
                   (in Node), the browser (headless Chrome), PostgreSQL and MariaDB
                   (throwaway servers), and the language server;
                   tests/programs/ holds the programs they run
```
