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
    request |> auth.required |> load_feed(page) |> render_feed
```

Components run in the browser too, with state, and call the server directly:

```quillo
server fn add_note(text: str) -> list ! str with db { ... }

component Notes(initial: list) {
    ~notes = initial
    ~draft = ""
    <form @submit={() => { notes = add_note(draft) ?? notes; draft = "" }}>
        <input bind:value={draft}/>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
    </form>
}
```

Quillo is a tree-walking interpreter in Rust with a built-in HTTP server, a static
checker, a compiler from components to JavaScript, an embedded SQLite database and
a language server for editors.

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
| `--workers N` | request threads (default 8) |
| `--no-check` | skip the static checks |

Editor support: [editors/vscode](editors/vscode) is a VS Code extension (highlighting,
diagnostics as you type, hover, go to definition, completion, outline, snippets).
Other editors can run `quillo lsp` directly.

Examples: [hello.qwe](examples/hello.qwe) (routes), [orders.qwe](examples/orders.qwe)
(payload-based routing and middleware), [todo.qwe](examples/todo.qwe) (server-rendered
app with a JSON API), [notes.qwe](examples/notes.qwe) (SQLite and a live component).

## Language tour

### Values

`null`, `true`/`false`, `42`, `3.14`, `"strings"` or `'strings'`, template strings
`` `Hello, ${name}` ``, lists `[1, 2, 3]`, maps `{ name: "Ada", "content-type": "text/plain" }`,
functions, HTML and error values. A map entry `{ id }` is short for `{ id: id }`.

Only template strings (backticks) interpolate; `"..."` and `'...'` are plain text, so
JSON is easy to write: `'{"a": 1}'`. Template strings can span lines.

Operators are as in JavaScript: `+ - * / %`, `== !=` (deep equality, like comparing
JSON), `< <= > >=`, `!`, `&&`, `||`, `??` and `cond ? a : b`. `null`, `false` and error
values are falsy; everything else (including `0` and `""`) is truthy. `a && b` and
`a || b` give back one of their operands, so `name || "anonymous"` and
`{loading && <Spinner/>}` work. Integer `/` gives an integer: `7 / 2` is `3`.

### Coming from JavaScript

Most expressions read as they would in JavaScript. The differences:

- `let` declares a variable (there is no `const` or `var`); values are immutable, so
  `push`, `set` and `+` return new lists and maps.
- Named functions are `fn name(params) = expr` or `fn name(params) { ... }`; anonymous
  ones are arrows. Blocks give back their last expression, so `return` is only for
  leaving early.
- `==` compares values deeply (`[1, 2] == [1, 2]`); there is no `===`.
- Only `null`, `false` and error values are falsy: `0` and `""` are truthy.
- `match`, `|>`, patterns, `route`, `reply`, `~state` and `with` effects have no
  JavaScript equivalent.
- Integers stay integers: `7 / 2` is `3` (write `7.0 / 2` for `3.5`).
- Text in HTML follows JSX, but `class` (not `className`) and `@click` (not `onClick`).

### Variables, functions, views

```quillo
let todos = []
todos = todos |> push("x")        // reassign with =, += or -=

fn add(a: int, b: int = 10) -> int = a + b
fn area(w, h) {                   // block body: the last expression is the result
    let a = w * h
    a
}
let double = (x) => x * 2          // arrow functions, as in JavaScript
let triple = x => x * 3
let point = (x: int, y: int = 0) => { x, y }      // `{ x, y }` here is a map

view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>
```

Type annotations are checked when the function is called and when it returns, and
`quillo check` checks them before the program runs: `int float num str bool list map html
fn response any`. Unions are written `int | str`; add `?` or `| null` to also allow null
(`str?`, `int | null`).

### Pipelines and method calls

`a |> f(b)` is `f(a, b)`. `x.f(y)` calls `f(x, y)` when `x` has no field `f`,
and `x.len` is `len(x)`:

```quillo
orders
    |> filter((o) => o.total > 100)
    |> sort_by((o) => o.total)
    |> map((o) => o.id)

orders.filter(o => o.paid).map(o => o.id)
[3, 1, 2].sort().first()
```

A line starting with `|>`, `??`, `&&`, `||`, `?`, `:`, `.method` or `else` continues
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
- Attributes: `"text"` (plain, as in JSX) or `{expr}`, for example ``href={`/todos/${id}`}``.
  `true` renders a bare attribute; `false`/`null` leaves it out.
- `null`/`false` children render nothing; list children render each item, so `{for ... => <li/>}` works.
- Void tags (`<br>`, `<img ...>`, `<input ...>`) need no closing tag. Any tag can self-close with `/>`.
- `<html>` gets `<!doctype html>`.
- `raw(s)` marks trusted HTML.
- A capitalized tag calls a view: attributes fill parameters by name, and the element's body is passed as `children`.

## Components

A `component` is a view with state that also runs in the browser:

```quillo
component Counter(start: int = 0) {
    ~count = start                       // state: set once, kept between renders
    let double = count * 2               // recomputed on every render
    <div>
        <span>{count} (x2 = {double})</span>
        <button @click={() => { count += 1 }}>+</button>
    </div>
}

route GET / = <html><body><Counter start={5}/></body></html>
```

- The server renders it like a view, inside a `<quillo-island>` that carries its props
  and state. The page loads `/_quillo/quillo.js` automatically, and the browser takes
  over from that state.
- Assigning to a `~` variable re-renders the component. Lists keep their DOM (and
  focus) when items have `key={...}`. Nested components keep their own state.
- `@event={fn}` handles DOM events (`@click`, `@input`, `@submit`, ...). The function
  may take the event: `e.value`, `e.checked`, `e.key`, and `e.form` (the form's fields
  as a map). `@submit` does not reload the page.
- `bind:value={x}` and `bind:checked={x}` keep an input and a variable in step.
- Component code, and the functions, views and constants it uses, is compiled to
  JavaScript. Code that cannot run in a browser is an error when the program loads
  (and in `quillo check`): `request`, `db`, `http`, `env`, files, `reply`, or a global
  the server changes (pass it as a prop instead).
- `/_quillo/quillo.js` holds only what the page's components use: their compiled code and
  the parts of the runtime it reaches, minified (a small component is about 11 KB,
  under 4 KB gzipped). Text responses are gzipped for browsers that accept it, and
  the bundle has an ETag, so an unchanged one costs a `304` on reload.
- In the browser, numbers are JavaScript numbers, so `2.0` prints as `2` and `7.0 / 2`
  is `3`.

### Server functions

`server fn` makes a function callable from the browser:

```quillo
server fn add_note(text: str) -> list ! str with db {
    if trim(text) == "" { return err("empty note") }
    db.exec("insert into notes (text) values (?)", text)?
    db.query("select * from notes")
}

component Notes(initial: list) {
    ~notes = initial
    ~draft = ""
    ~problem = null
    <form @submit={() => {
        let r = add_note(draft)                    // a request to the server
        if is_err(r) { problem = r.error } else { notes = r }
    }}>
        <input bind:value={draft}/>
    </form>
}
```

In component code the call is awaited automatically. It works in event handlers and
in `~state` initializers (`~todos = load_todos()`: the server runs it while rendering,
so the page arrives with the data). Calling one while rendering is an error.

Each call is a `POST /_quillo/rpc/<name>` with a JSON array of arguments, checked against
the parameter types (a 400 otherwise); the answer is `{"ok": value}` or `{"err": payload}`.
Only `server fn`s can be called, only with `content-type: application/json` (so a form
on another site cannot), and inside one `request` is the current request. `reply` in a
server fn becomes the HTTP response, so the usual middleware works:

```quillo
server fn delete_all() = request |> auth.required |> (_) => { ... }
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
- **Request pattern:** matched against the `request` map. Fields: `json`, `form`, `headers`, `cookies`, `query`, `params`, `method`, `path`, `body`, `ip`. JSON is matched strictly: the string `"7"` is not an int. Form fields, headers and cookies are converted like query values.
- **Reserved:** paths under `/_quillo/` belong to Quillo (the browser bundle, server fn calls, `--watch`).
- **Order:** routes are tried in the order they are declared, and the first whose method, path and patterns all match handles the request. A later route can be the fallback.

If no route matches, the response is 404 for an unknown path, 405 for a known
path with the wrong method, and 400 when a route matched the path and method but
none of its patterns matched.

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
| `file(path)` | a file under the program's folder (paths with `..` or `/` are refused) |

`reply status [body]` ends the request right away, from anywhere: inside a
helper, a pipeline stage or a `??` fallback. This is how middleware works:

```quillo
let auth = {
    required: (req) =>
        req.headers.authorization == `Bearer ${env("TOKEN")}`
            ? req
            : reply 401 { error: "unauthorized" },
}

route GET /me = request |> auth.required |> (r) => { ip: r.ip }

route GET /users/:id(int) {
    let user = users |> find((u) => u.id == id) ?? reply 404 { error: `no user ${id}` }
    user
}
```

## Standard library

- **General:** `print str int float len type_of`
- **Strings:** `upper lower trim split join contains starts_with ends_with replace slice repeat index_of`
- **Lists:** `map filter find any all reduce sum min max sort sort_by reverse first last push range enumerate slice unique flatten zip take drop count group_by index_of`
- **Maps:** `keys values get has set remove merge`
- **Math:** `abs round floor ceil sqrt pow`
- **Errors:** `err is_err unwrap`
- **Web:** `raw escape env now file redirect respond json.parse json.stringify`
- **Effects:** `sleep(ms)`, `random()`, `random_int(lo, hi)`, `uuid()`, `read_file(path)`,
  `write_file(path, text)` (paths stay inside the program's folder)

Values are immutable: `push`, `set` and `+` return new lists and maps.
`use "lib.qwe"` runs another file once, into the global scope.

### Database

`db` talks to SQLite (built in) or PostgreSQL, with the same functions. SQL problems
are error values, so `?` and `??` work on them.

```quillo
db.open("app.db")                                   // SQLite, next to the program; ":memory:" also works
db.open("postgres://user:pass@host:5432/app")       // PostgreSQL
db.exec("create table if not exists todos (id serial primary key, text text)")
db.exec("insert into todos (text) values (?)", text)       // { changes, last_id }
db.query("select * from todos where id > ?", 10)           // a list of maps
db.one("select * from todos where id = :id", { id })       // a map, or null
db.one("insert into todos (text) values (?) returning id", text)
db.transaction(() => { ... })                              // all or nothing; nests
```

Without `db.open`, the database is `$QUILLO_DB` (a path or a `postgres://` URL), or else
SQLite in memory. A transaction rolls back if its function fails, returns an error
value or replies early.

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
- TLS is used when the server offers it (`?sslmode=require` insists), with certificates
  checked against the usual public certificate authorities, as hosted Postgres services
  use.

### HTTP client

```quillo
let r = http.get("https://api.example.com/users/1")        // { status, ok, headers, body, json }
http.post(url, { name: "Ada" })                            // maps and lists are sent as JSON
http.request("PUT", url, body, { authorization: `Bearer ${token}` })
```

Every HTTP status is a response; failing to connect is an error value.

## Checking

`quillo check app.qwe` finds mistakes without running the program, and `quillo run`
does the same first. Types are sets of possible types, and code without annotations
is `any`, so only definite mistakes are reported, and annotations make the checks
stricter. It reports unknown names (with suggestions), names used before they are
defined, wrong argument counts and types (for builtins too), operators that can never
work, division by zero, return and error types, effects, view and component props,
component code that cannot run in a browser, route mistakes, and unused variables
(a warning; name a variable `_x` to keep it).

## Concurrency and reloading

Requests are served by a pool of threads. Quillo code runs under one lock, so a handler
runs on its own from start to end, except while it waits: `sleep`, `http.*` and
reading the request let other requests run. Updating shared state such as
`todos = todos |> push(t)` is therefore safe without locks.

With `--watch`, a change to the program or a `use`d file reloads it (starting its
state afresh) and refreshes open pages. A version that fails to load or check does not
replace the one that works; the errors are printed.

## Not built yet

- `match` exhaustiveness and record types in the checker
- compiling components to WebAssembly (they compile to JavaScript)
- a formatter

## Layout

```
src/lexer.rs      tokens; newline handling
src/parser.rs     recursive descent: statements, expressions, patterns, HTML, routes, components
src/interp.rs     evaluator, pattern matching, HTML and component rendering, effects
src/builtins.rs   standard library
src/db.rs         db: SQLite, and the shared API
src/pg.rs         db: PostgreSQL (pool, placeholders, types)
src/fetch.rs      http: the HTTP client
src/server.rs     request matching, server fns, responses, worker pool, --watch
src/shared.rs     the interpreter lock and other state shared between threads
src/check.rs      the static checker
src/client.rs     compiles components to JavaScript
src/client/runtime.js   the browser runtime: values, standard library, virtual DOM
src/lsp.rs        the language server
src/main.rs       CLI: run, check, repl, lsp
editors/vscode/   the VS Code extension
examples/         hello, orders, todo, notes
tests/            language, routing, effects, checker, compiler (in Node), browser
                  (headless Chrome), PostgreSQL (a throwaway server), server and
                  language server tests
```
