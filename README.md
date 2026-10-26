# Quillo

Quillo is a small web programming language. Routes, request patterns, pipelines and
HTML are part of the syntax, so a web app reads like a list of what it answers. Its
expressions look like JavaScript: arrow functions, `&&` and `||`, `? :`, `null`,
template strings, and JSX-style HTML.

```quillo
route GET /hello/:name = `Hello, ${name}!`

route POST /orders { json: { type: "order" | "refund", customer.id: int as id } } {
    reply 201 { accepted: true, customer: id }
}

route GET /feed ? { page: int = 1 } =
    request | auth.required | load_feed(page) | render_feed
```

Components run in the browser too, with state, and call the server directly:

```quillo
server fn add_note(text: str) -> list ! str with db { ... }

component Notes(initial: list) {
    state notes = initial
    state draft = ""
    <form @submit={() => { notes = add_note(draft) ?? notes; draft = "" }}>
        <input bind:value={draft}/>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
    </form>
}
```

Quillo is a tree-walking interpreter in Rust with a built-in HTTP server, a static
checker, a compiler from components to JavaScript, an embedded SQLite database (and
PostgreSQL and MySQL drivers) and a language server for editors.

```sh
cargo build --release
./target/release/quillo run examples/notes.qwe --watch   # http://127.0.0.1:8080
./target/release/quillo check app.qwe                    # find mistakes without running
./target/release/quillo repl
./target/release/quillo lsp                              # language server (for editors)
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
| `--service-url NAME=URL` | where service NAME runs, when not here (also `QUILLO_SERVICE_NAME`) |
| `--base PATH` | the path the program is served under (`/shop`): its links and redirects carry it |
| `--no-view-cache` | render views every time (see [Cached views](#cached-views)) |
| `--no-jobs` | run none of the program's scheduled jobs on this server |
| `--register URL`, `--service S` | join a limer pool, and share live updates with its other servers (see [Concurrency](#concurrency-and-reloading)) |
| `--max-body N` | largest request body to accept, in bytes (default 10 MiB; larger ones get 413) |
| `--log F` | the request log: `text` (the default), `json` (one object per line) or `off` |
| `--metrics` | answer `GET /_quillo/metrics` in Prometheus's text format |

### Desktop apps

`quillo app` runs a program in a native window instead of a browser tab: the
platform's own web view (WebKit on macOS and Linux, WebView2 on Windows) shows
its pages, and closing the window ends the program. It is built only on request:

```sh
cargo build --release --features app      # Linux also needs libwebkit2gtk-4.1-dev
./target/release/quillo app examples/notes.qwe --watch
```

It takes the options of `run` except `--host`, `--register` and `--service`. The
program is served on 127.0.0.1, at a port the system picks, to the window alone:
the window's first request carries a token made for this launch and trades it
for a cookie, and requests without one (other programs, pages of other sites)
get 403. Links to other sites open in the system's browser.

Editor support: [editors/vscode](editors/vscode) is a VS Code extension (highlighting,
diagnostics as you type, hover, go to definition, completion, outline, snippets).
Other editors can run `quillo lsp` directly.

Examples: [hello.qwe](examples/hello.qwe) (routes), [orders.qwe](examples/orders.qwe)
(payload-based routing and middleware), [todo.qwe](examples/todo.qwe) (server-rendered
app with a JSON API), [notes.qwe](examples/notes.qwe) (SQLite and a live component),
[scroll.qwe](examples/scroll.qwe) (infinite scroll over 100,000 rows, rendering only a window),
[compose.qwe](examples/compose.qwe) (views and components built from each other),
[layout.qwe](examples/layout.qwe) (a page described as JSON, with a live editor),
[spa.qwe](examples/spa.qwe) (a single-page app: client-side navigation, deep links, 404s),
[life.qwe](examples/life.qwe) (Conway's Game of Life, run by a timer in the browser),
[storm.qwe](examples/storm.qwe) (a dashboard under a storm of random changes, tested for leaks),
[services](examples/services) (a shop that bills through a billing service, in one process or
split apart), and [index.qwe](examples/index.qwe), which mounts all of them in one server.

A whole application: [apps/crm](apps/crm) is a multi-tenant CRM for merchants, bloggers
and news outlets, with a shop, campaigns, a flow designer, and plugins whose rules run in
the browser and on the server.

## Language tour

### Values

`null`, `true`/`false`, `42`, `3.14`, `1e-9` and `6.02e23`, `"strings"` or `'strings'`, template strings
`` `Hello, ${name}` ``, lists `[1, 2, 3]`, maps `{ name: "Ada", "content-type": "text/plain" }`,
functions, HTML and error values. A map entry `{ id }` is short for `{ id: id }`.

`xs[0]` is the first item and `xs[-1]` the last (negative indexes count from the end;
`first(xs)` and `last(xs)` say the same in words). An index past the end, a missing map
key, or any field of `null` gives `null`. Strings index by character: `"héllo"[1]`
is `"é"`.

A name right before a template calls it with the text: `` shout`hi ${name}` `` is
`` shout(`hi ${name}`) ``. The builtins `css` and `sql` return the text as it is, and
tell editors to highlight it as CSS or SQL.

Only template strings (backticks) interpolate; `"..."` and `'...'` are plain text, so
JSON is easy to write: `'{"a": 1}'`. Template strings can span lines.

Operators are as in JavaScript: `+ - * / %`, `== !=` (deep equality, like comparing
JSON), `< <= > >=`, `!`, `&&`, `||`, `??` and `cond ? a : b`. `null`, `false` and error
values are falsy; everything else (including `0` and `""`) is truthy. `a && b` and
`a || b` give back one of their operands, so `{loading && <Spinner/>}` works. Because
`""` is truthy, `name || "anonymous"` keeps an empty name: write `name ?? "anonymous"` to
replace only null, or `name != "" ? name : "anonymous"` (`quillo check` points these out). `/` divides exactly, as in JavaScript: `7 / 2` is `3.5`,
and a whole result stays an integer (`6 / 2` is `3`); `floor(7 / 2)` is integer division.

### Coming from JavaScript

Most expressions read as they would in JavaScript. The differences:

- `let` declares a variable (there is no `const` or `var`); values are immutable, so
  `push`, `set` and `+` return new lists and maps.
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
`quillo check` checks them before the program runs: `int float num str bool list map html
fn response any`. Unions are written `int | str`; add `?` or `| null` to also allow null
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
  are checked the same way.
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
match value { ... }
return x
```

### Patterns

The same patterns work in `match`, `let`, `for`, and route declarations:

| Pattern | Matches |
|---|---|
| `_`, `name` | anything (`name` binds it) |
| `42`, `"order"`, `true`, `null` | that literal |
| `int`, `str`, `list`, ... | any value of that type |
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

The effects are `db`, `http`, `fs` (files), `env` (environment variables), `time` (`now`,
`sleep`) and `random`. Inside a `with` function, a builtin that needs another effect,
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
  out win over spread ones; classes from both are kept. On a view or component, spread
  keys that are not parameters are ignored, so data with extra fields can be passed
  straight through; a misspelled attribute written out is still an error.
- Attributes: `"text"` (plain, as in JSX) or `{expr}`, for example ``href={`/todos/${id}`}``.
  `true` renders a bare attribute; `false`/`null` leaves it out.
- `null`/`false` children render nothing; list children render each item, so `{for ... => <li/>}` works.
- Void tags (`<br>`, `<img ...>`, `<input ...>`) need no closing tag. Any tag can self-close with `/>`.
- `<html>` gets `<!doctype html>`.
- `<style>{style}</style>` takes a style sheet as it is (no escaping, so `a > b` works).
  Write it as `` css`...` ``: a template string (braces are plain text, `${...}` fills in
  values) that editors highlight as CSS. `` sql`...` `` does the same for queries.
- `raw(s)` marks trusted HTML.
- A capitalized tag calls a view: attributes fill parameters by name, and the element's body is passed as `children`.

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
  component from that state, on the server's own elements (see below).
- `state count = start` declares state, set once (`~count = start` is the short form).
  After that it is just `count`, and assigning to it (`count += 1`) updates what reads
  it, and nothing else. A component's code runs once. A `let` that reads state or props
  (`let double = count * 2`) is worked out again when they change. So is a statement
  that reads them (`if running { every(...) }`). Each place in the markup that shows a
  value follows only that value: changing one row's text changes that text. Assigning
  the value a state already has does nothing.
- `for` in markup keeps a row for each item: by its `key={...}`, else by its place.
  A row whose item changed is updated where it shows what changed, and moved rows keep
  their DOM (and focus). `if`, `? :` and `match` in markup make a branch again only
  when another one is chosen. Nested components keep their own state.
- `@event={fn}` handles DOM events (`@click`, `@input`, `@scroll`, `@submit`, ...).
  The function may take the event `e`:
  - `e.value` and `e.checked` of the input, `e.key` of a key press, `e.type`;
  - `e.form`: the fields of the form, as a map;
  - `e.target`: the element itself;
  - any other field of the browser's event (`e.clientX`, `e.deltaY`, `e.shiftKey`,
    ...), then of the element (`e.scrollTop`, `e.scrollHeight`, `e.clientHeight`,
    `e.dataset`, ...).

  `@submit` does not reload the page.
- `bind:value={x}` and `bind:checked={x}` keep an input and a variable in step.
- `every(ms, f)` in a component's code calls `f` every `ms` milliseconds, in the browser,
  for as long as the code that called it stands: `if running { every(100, step) }`
  runs a game while `running` is true, starts again with the new speed when that changes,
  and stops when `running` is false or the component leaves the page. On the server it
  does nothing.
  `listen(url, f)` follows a stream the same way (see [Streaming](#streaming)).
- Component code, and the functions, views and constants it uses, is compiled to
  JavaScript. Code that cannot run in a browser is an error when the program loads
  (and in `quillo check`): `request`, `db`, `http`, `env`, files, `reply`, or a global
  the server changes (pass it as a prop instead).
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
  as `2`. Arithmetic gives the same results on both sides.

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
- Views are plain functions: they have no state and add nothing to the browser bundle
  unless a component uses them.
- A component placed directly in a page gets its props from the server, so they must
  be data (no functions). Inside another component, props can be anything: callbacks,
  HTML, other values.

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
  methods as fields: `el.value`, `chart.update()`. What runs text as code or HTML is
  refused, however it is reached: `eval`, `Function`, `setTimeout` and `setInterval`,
  `insertAdjacentHTML`, `document.write`, `setAttribute("on...")`, `javascript:` URLs
  given to the page's functions. Markup in the component does all of that safely.
- On the server, `@mount` does nothing and `js` gives null.

### Single-page apps

`url()` gives `{ path, query, hash }`: of the request on the server, and of the address
bar in the browser, where what reads it follows it when it changes. Once a
component has called it, clicks on same-site links inside components change the page
without reloading; Back and Forward work. `navigate("/path")` does the same from code.
(Links with a modifier key, `target`, `download` or `data-reload` work as usual.)

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
[spa.qwe](examples/spa.qwe), which also answers 404 on the server for unknown paths.

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
- The server renders the first value; the page keeps one WebSocket open
  (`/_quillo/live`, served by its own thread), down which the server streams each
  change to its topics, and calls the loader again. A WebSocket, because browsers
  allow only six HTTP/1.1 connections to a host across all tabs: requests left
  waiting would hold them, and server calls would queue behind them. Where a
  WebSocket cannot open, the page asks the same URL as a plain request instead.
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

In the browser that is `Promise.all`. On the server each function runs on a thread of
its own, so their waits (database, `http.*`, `sleep`) overlap; inside a transaction they
run one after another, since other threads' SQL would not be part of it. A failure stops
the whole call; error values are results like any other.

Each call is a `POST /_quillo/rpc/<name>` with a JSON array of arguments, checked against
the parameter types (a 400 otherwise); the answer is `{"ok": value}` or `{"err": payload}`.
Only `server fn`s can be called, only with `content-type: application/json`, and only
from the program's own pages: a request a browser marks as coming from another site
(`Sec-Fetch-Site`, or an `Origin` naming another host) gets a 403. Inside one, `request`
is the current request. `reply` in a
server fn becomes the HTTP response, so the usual middleware works:

```quillo
server fn delete_all() = request | auth.required | (_) => { ... }
```

From hand-written JavaScript: `await quillo.server.add_note("text")`.

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
it. `delete_file(path)` removes a file or folder inside the program's folder.

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
allocated; both run off the event loop. Paths stay inside the program's folder.

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
  change it, so keep secrets out of it. Cookies hold about 4 KB; keep an id in the
  session and the rest in the database. The signature covers its expiry, its tenant
  and the program's mount point, so it is no session past its time, for another
  tenant, or in another mounted program.
- `end_session()` removes the cookie from the browser, but a copy someone kept stays
  valid until it expires. To end sessions on the server (log out everywhere, a stolen
  laptop), keep an id in the session and check it against the database.
- Set `QUILLO_SECRET` to a long random string (`crypto.token()` makes one) and use the
  same one on every server. Without it, a random key is made at start: sessions end
  when the server stops.
- `set_cookie(name, value, { max_age, path, domain, http_only, secure, same_site })` and
  `delete_cookie(name, { path, domain })` add `Set-Cookie` to the response, whatever
  the route returns. Cookies are `HttpOnly`, `SameSite=Lax`, `Path=/` and `Secure` by
  default. `Secure` is left out with `--watch` and in desktop apps, which are served
  over plain HTTP on this machine; in production, serve the site over HTTPS (a proxy
  such as limer in front). `request.cookies` has what the browser sent.

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
  not a success), or an error value when it could not be had.
- In a page, `receive(url, f, { method, body, headers })` in a handler sends the
  request and calls `f` with what the answer streams, until it ends; `listen(url,
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
  with a 64 KiB buffer, so a video costs no more memory than a page. The gzipped copy
  of a text file is kept too, rather than compressed again for each request.
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
  wrote in them.
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
[index.qwe](examples/index.qwe), which mounts every example.

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
  signed with HMAC-SHA256 under `QUILLO_SERVICE_SECRET` (the same on every service)
  and the time (a call more than five minutes old is refused); the tenant goes with
  it. Only `server fn`s are offered. Its routes, under its prefix, are passed on to it.
  Run alone, a service is an ordinary program: `quillo run billing.qwe --base /billing`
  (`--base`: the path it is served under, which its links and redirects carry).
- `quillo run app.qwe --split` runs each service the app declares (and those they
  declare) as a process of its own, on a port of its own, its log lines marked with its
  name, all signed with one secret; they stop with it.
- Either way, what crosses is data, as JSON: arguments and results are copied (a
  function arrives as `null`), so a program behaves the same whichever way it runs.
  Optional arguments left out are not sent, so the service's own defaults apply. A
  call that cannot be made (the service not answering) is an error value: `?` passes
  it on, `??` replaces it.
- `quillo check` checks each call against the service's `server fn`s (their names,
  arguments and types; it may fail, so the result may be an error), the browser cannot
  call a service, and a call needs the `http` effect. Services may use one another.
- A program with `server fn`s and no routes is served too: a service may be only that.

See [examples/services](examples/services): a shop that bills through a billing service.

## Standard library

- **General:** `print str int float len type_of`
- **Strings:** `upper lower trim split join contains starts_with ends_with replace slice repeat index_of`
- **Translations:** `t(key, vars)`, `locale()`, `translations`, `locale_from` and
  `with_locale` (see [Translations](#translations)).
- **Markdown:** `markdown(text)` gives HTML (CommonMark with tables, strikethrough and task
  lists). HTML inside the text is shown as text and `javascript:` links are blocked, so
  it is safe for what visitors write; `markdown(text, { html: true })` keeps the HTML,
  for text you wrote, and `markdown(text, { inline: true })` keeps only what is inside a
  line (bold, italic, code, links) for a headline or a caption: no paragraph around it, and
  no headings or lists (their lines, broken by `<br>`).
- **Lists:** `map filter find any all reduce sum min max sort sort_by reverse first last push range enumerate slice unique flatten zip take drop count group_by index_of`
- **Maps:** `keys values get has set remove merge`
- **Math:** `abs round floor ceil sqrt pow`, and the three modules under
  [Numbers and data](#numbers-and-data).
- **Records:** `type User = { ... }` makes `User(fields)`, which builds and checks one
  (see [Records](#records)).
- **Errors:** `err is_err unwrap`
- **Web:** `raw escape env now file redirect respond json.parse json.stringify set_cookie
  delete_cookie session set_session end_session`
- **Streaming:** `stream(f)` and `events(f)` answer as they go, `http.stream` reads
  another server's answer as it arrives, and `listen` and `receive` read streams in a
  page (see [Streaming](#streaming)).
- **Database:** `db.open db.exec db.query db.one db.transaction db.migrate db.connect
  db.tenants db.tenant_exists` (see [Database](#database)).
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
  become attributes only if they are plain names that run nothing: `onclick`, `srcdoc`
  and names with quotes or `>` are dropped. `javascript:` URLs in `href`, `src` and the
  like render as `about:blank#blocked`.
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
- Globals read with `env`, `read_file` or the database are never written into the
  browser bundle; a component that uses one is a load-time error.
- `quillo check` warns about SQL put together from values (`${x}` or `+`); pass them
  as `?` parameters instead. Names cannot be parameters: put a table's or a column's
  name in with `sql_name(name)` (or a list of them), and named parameters with
  `sql_param(name)` — both take only letters, digits and `_`, and SQL made of them,
  text as written and choices between those is not warned about.
- Inside `db.transaction` on SQLite, `sleep` and `http` calls hold up other requests
  instead of letting them write into the open transaction.

Values are immutable: `push`, `set` and `+` return new lists and maps.
`use "lib.qwe"` runs another file once, into the global scope.

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
  optional known mean), `quantiles(data, n = 4, method = "exclusive")`, `covariance
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

### Database

`db` talks to SQLite (built in), PostgreSQL or MySQL/MariaDB, with the same functions.
SQL problems are error values, so `?` and `??` work on them.

```quillo
db.open("app.db")                                   // SQLite, next to the program; ":memory:" also works
db.open("postgres://user:pass@host:5432/app")       // PostgreSQL
db.open("mysql://user:pass@host:3306/app")          // MySQL or MariaDB (also mariadb://)
db.exec("create table if not exists todos (id serial primary key, text text)")
db.exec("insert into todos (text) values (?)", text)       // { changes, last_id }
db.query("select * from todos where id > ?", 10)           // a list of maps
db.one("select * from todos where id = :id", { id })       // a map, or null
db.one("insert into todos (text) values (?) returning id", text)
db.transaction(() => { ... })                              // all or nothing; nests
```

Without `db.open`, the database is `$QUILLO_DB` (a path or a URL), or else
SQLite in memory. A transaction rolls back if its function fails, returns an error
value or replies early.

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
  arrays as lists, `numeric` as numbers, and `uuid`, `date`, `timestamp`/`timestamptz`
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
- TLS is used when the server offers it (`?sslmode=require` insists), with certificates
  checked against the usual public certificate authorities, as hosted Postgres services
  use.

**MySQL and MariaDB** (the `mysql` feature, built in by default; `cargo build
--no-default-features` leaves it out):

- Placeholders are `?`, or `:name` with a map (translated, skipping quotes, backticks
  and comments). Statements are prepared, so values never become SQL text.
- Values convert both ways: numbers, text, `tinyint(1)` as booleans, `decimal` as
  numbers, MySQL's `json` as maps and lists (MariaDB's JSON is text: `json.parse` it),
  and dates and times as ISO text. `last_id` is the new row's `auto_increment` id.
- Connections come from a pool, and a query releases the interpreter lock while it
  waits, as with Postgres. A transaction keeps one connection until it ends; nested
  ones are savepoints.
- `db.migrate()` takes turns through a named lock (`get_lock`). A statement that
  changes the schema commits at once in MySQL, so a migration that fails part way
  keeps what it did before the failure: put one change in each file.
- A database per tenant: `mysql://host/app_{tenant}`, made when the program makes the
  tenant, and listed by `db.tenants()`.

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

## Checking

`quillo check app.qwe` finds mistakes without running the program, and `quillo run`
does the same first. Types are sets of possible types, and code without annotations
is `any`, so only definite mistakes are reported, and annotations make the checks
stricter. It reports unknown names (with suggestions), names used before they are
defined, wrong argument counts and types (for builtins too), operators that can never
work, division by zero, return and error types, effects, view and component props,
component code that cannot run in a browser, route mistakes, and, as warnings, unused
variables (name a variable `_x` to keep it) and calls like `last(xs)` where a variable of
yours named `last` hides the builtin.

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
`xs[i]` is an item, not "or null" past the end. Editors get the same warnings with the
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

With `--watch`, a change to the program or a `use`d file reloads it (starting its
state afresh) and refreshes open pages. `--reload` reloads it the same way without
`--watch`'s development aids (errors on pages, refreshing them), for production
programs that change their own files, such as a plugin editor. A version that fails to load or check does not
replace the one that works; the errors are printed.

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

### Logs and metrics

Each request is logged on stderr as `GET /path -> 200 (0.4ms)`. With `--log json` the
line is a JSON object instead, for log collectors: `time` (ISO 8601, UTC), `method`,
`path`, `status`, `ms`, `ip`, and `tenant` when the request had one. `--log off` turns
the request log off; errors are still printed.

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
`for` over `range(...)` or `a..b`, and calls of other such functions, itself included.
It then runs as it would have, only faster: `fib(32)` takes 11 ms (Node 11, Bun 21),
and a loop of ten million steps 29 ms (Node and Bun take about 280 ms for the same
loop, on doubles).

- The answers are the interpreter's: ints are 64-bit and overflow is an error, `%` by
  zero is an error, comparing NaN is an error. Where the interpreter would fail, the
  machine code gives up and the call is run again by the interpreter, which fails as
  always (the function changed nothing, so running it twice is running it once).
- What it does not take runs in the interpreter as before: strings, lists, maps,
  builtins, globals that are not functions, functions inside functions, a variable that
  changes type, and `/` of two ints, whose answer is an int or a float depending on the
  numbers (`6 / 3` is 2, `7 / 2` is 3.5).
- A function it calls that is assigned something else is not called by the old code.
- Not with a fuel limit (scripts given a budget of steps), and not in builds without
  the `jit` feature (`cargo build --no-default-features --features mysql`).

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

The standard library
src/builtins.rs    the builtins every program can call
src/numeric.rs     math, stats and vec: Python's math and statistics, numpy's operations
src/par.rs         a pool of threads for long lists, in chunks of a fixed size
src/crypto.rs      crypto: hashes, signatures, tokens, passwords
src/imaging.rs     image: what an image file is, and new files made from it
src/highlight.rs   highlight(code, lang): source code as HTML
src/i18n.rs        translations
src/fetch.rs       http: the HTTP client, and http.stream

Databases
src/db.rs          db: SQLite, connections, tenants, transactions, and the shared API
src/pg.rs          PostgreSQL (pool, pipelining, placeholders, types)
src/my.rs          MySQL and MariaDB (the mysql feature)
src/migrate.rs     db.migrate: schema files applied once each
src/sqlinfo.rs     which tables a statement reads and writes, for live updates

Serving
src/httpd.rs       the event loop: connections, requests as coroutines, waiting off the loop
src/server.rs      request matching, server fns, responses, --watch
src/stream.rs      answers sent as they are made: stream, events, send
src/service.rs     services: in this process or elsewhere, signed calls, --split
src/session.rs     cookies and sessions
src/upload.rs      files sent with a form
src/statics.rs     files served with file()
src/site.rs        what a program sets about its site: tenants, cross-site posts
src/jobs.rs        jobs run on a schedule
src/viewcache.rs   reusing views' HTML while their inputs are unchanged
src/ws.rs          WebSockets, for live updates and --watch reloads
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
editors/vscode/    the VS Code extension

examples/          hello, orders, todo, notes, compose, layout, spa, scroll, life, storm,
                   services, and index (all of them in one server)
apps/crm/          a whole application: a multi-tenant CRM
site/              quillolang.org, written in Quillo; its docs are made from this README
tests/             the language, routes, records, effects, the checker, numbers, streams,
                   services, sessions, uploads, tenants, several databases, the compiler
                   (in Node), the browser (headless Chrome), PostgreSQL and MariaDB
                   (throwaway servers), the CRM, and the language server
```
