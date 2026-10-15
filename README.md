# Wisp

Wisp is a small web programming language. Routes, request patterns, pipelines and
HTML are part of the syntax, so a web app reads like a list of what it answers:

```wisp
route GET /hello/:name = "Hello, {name}!"

route POST /orders { json: { type: "order" | "refund", customer.id: int as id } } {
    reply 201 { accepted: true, customer: id }
}

route GET /feed ? { page: int = 1 } =
    request |> auth.required |> load_feed(page) |> render_feed
```

Components run in the browser too, with state, and call the server directly:

```wisp
server fn add_note(text: str) -> list ! str with db { ... }

component Notes(initial: list) {
    ~notes = initial
    ~draft = ""
    <form @submit={fn() { notes = add_note(draft) ?? notes; draft = "" }}>
        <input bind:value={draft}/>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
    </form>
}
```

Wisp is a tree-walking interpreter in Rust with a built-in HTTP server, a static
checker, a compiler from components to JavaScript, an embedded SQLite database and
a language server for editors.

```sh
cargo build --release
./target/release/wisp run examples/notes.wisp --watch   # http://127.0.0.1:8080
./target/release/wisp check app.wisp                    # find mistakes without running
./target/release/wisp repl
./target/release/wisp lsp                               # language server (for editors)
cargo test
```

`wisp run` checks the program, executes its top level and, if it declared any routes,
serves them:

| Option | |
|---|---|
| `--port N`, `--host H` | where to listen (default `$PORT` or 8080, on 127.0.0.1) |
| `--watch`, `-w` | reload when the program or a `use`d file changes, and refresh open pages |
| `--workers N` | request threads (default 8) |
| `--no-check` | skip the static checks |

Editor support: [editors/vscode](editors/vscode) is a VS Code extension (highlighting,
diagnostics as you type, hover, go to definition, completion, outline, snippets).
Other editors can run `wisp lsp` directly.

Examples: [hello.wisp](examples/hello.wisp) (routes), [orders.wisp](examples/orders.wisp)
(payload-based routing and middleware), [todo.wisp](examples/todo.wisp) (server-rendered
app with a JSON API), [notes.wisp](examples/notes.wisp) (SQLite and a live component).

## Language tour

### Values

`nil`, `true`/`false`, `42`, `3.14`, `"strings with {interpolation}"`,
lists `[1, 2, 3]`, maps `{ name: "Ada", "content-type": "text/plain" }`,
functions, HTML and error values. A map entry `{ id }` is short for `{ id: id }`.

In strings, `{expr}` interpolates. Write `\{` for a literal brace (for example in
JSON text: `"\{\"a\": 1\}"`).

### Variables, functions, views

```wisp
let todos = []
todos = todos |> push("x")        // reassign with =, += or -=

fn add(a: int, b: int = 10) -> int = a + b
fn area(w, h) {                   // block body: the last expression is the result
    let a = w * h
    a
}
let double = fn(x) = x * 2         // anonymous function

view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>
```

Type annotations are checked when the function is called and when it returns, and
`wisp check` checks them before the program runs: `int float num str bool list map html
fn response any`. Unions are written `int | str`; add `?` or `| nil` to also allow nil
(`str?`, `int | nil`).

### Pipelines and method calls

`a |> f(b)` is `f(a, b)`. `x.f(y)` calls `f(x, y)` when `x` has no field `f`,
and `x.len` is `len(x)`:

```wisp
orders
    |> filter(fn(o) = o.total > 100)
    |> sort_by(fn(o) = o.total)
    |> map(fn(o) = o.id)

[3, 1, 2].sort().first()
```

A line starting with `|>`, `??`, `.method`, `else`, `and` or `or` continues the
previous line.

### Control flow

```wisp
if n > 0 then "pos" else "neg"
if n > 0 { "pos" } else if n == 0 { "zero" } else { "neg" }
for x in [1, 2, 3] => x * x                 // a list: [1, 4, 9]
for [key, value] in { a: 1 } => "{key}={value}"
match value { ... }
return x
```

### Patterns

The same patterns work in `match`, `let`, `for`, and route declarations:

| Pattern | Matches |
|---|---|
| `_`, `name` | anything (`name` binds it) |
| `42`, `"order"`, `true`, `nil` | that literal |
| `int`, `str`, `list`, ... | any value of that type |
| `"order" \| "refund"` | either alternative |
| `int as n` | binds what matched |
| `[first, second, ..rest]` | lists |
| `{ type: "user", name, customer.id: id, limit: int = 20 }` | maps (extra keys are allowed); `a.b` looks inside nested maps; `= default` fills a missing field |

In a map pattern, `key: int` (a bare type) also binds `key`.

```wisp
match event {
    { type: "click", x: int, y: int } => "click at {x},{y}"
    { type: "key", key: "Enter" | "Return" } => "enter"
    int as n if n < 0 => "negative"
    _ => "other"
}
```

### Errors

`err(x)` makes an error value. Builtins such as `int("abc")` and `json.parse`
return errors instead of crashing. `?` after an expression returns the error from
the current function, and `??` falls back when the left side is nil or an error.
Field access on nil gives nil:

```wisp
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err("bad age: {s}")
    }
fn next_year(s) = parse_age(s)? + 1         // err("bad age: x") for "x"
let age = int(request.query.age) ?? 18
```

`! T` in a signature declares what the function's errors contain. It is checked: a
function declared `! str` that returns `err(42)` is a runtime error, and `wisp check`
reports it, and a `?` that would pass on the wrong kind of error, before the program runs.

Mistakes in the code itself (calling a non-function, adding a map to an int, a
failed type check, recursion more than 10,000 calls deep) are runtime errors. In a
route they become a 500 response, and the server logs them.

### Effects

`with` declares what a function may do besides computing a value:

```wisp
fn load_user(id: int) with db = db.one("select * from users where id = ?", id)
fn fetch_avatar(url: str) with http = http.get(url).body
fn profile(id: int) with db, http = { user: load_user(id), avatar: fetch_avatar(...) }
```

The effects are `db`, `http`, `fs` (files), `env` (environment variables), `time` (`now`,
`sleep`) and `random`. Inside a `with` function, a builtin that needs another effect,
or a function declared with more effects, is an error, both at runtime and in
`wisp check` (which also warns when an undeclared helper may use one). The limit holds
for everything the function calls, including the lambdas it passes to `map`.
Functions without `with` may do anything.

### HTML literals

HTML is a value, not a string. Text children are `"quoted"`, expressions go in
`{...}`, and everything is escaped unless it is already HTML:

```wisp
<li.todo #todo-{t.id} class={if t.done then "done"} data-id={t.id}>
    {if t.done then <s>{t.text}</s> else t.text}
    <button disabled={not editable}>"Edit"</button>
</li>
```

- `.class` and `#id` shorthand, with interpolation.
- Attributes: `"text {x}"` or `{expr}`. `true` renders a bare attribute; `false`/`nil` leaves it out.
- `nil`/`false` children render nothing; list children render each item, so `{for ... => <li/>}` works.
- Void tags (`<br>`, `<img ...>`, `<input ...>`) need no closing tag. Any tag can self-close with `/>`.
- `<html>` gets `<!doctype html>`.
- `raw(s)` marks trusted HTML.
- A capitalized tag calls a view: attributes fill parameters by name, and the element's body is passed as `children`.

## Components

A `component` is a view with state that also runs in the browser:

```wisp
component Counter(start: int = 0) {
    ~count = start                       // state: set once, kept between renders
    let double = count * 2               // recomputed on every render
    <div>
        <span>"{count} (x2 = {double})"</span>
        <button @click={fn() { count += 1 }}>"+"</button>
    </div>
}

route GET / = <html><body><Counter start={5}/></body></html>
```

- The server renders it like a view, inside a `<wisp-island>` that carries its props
  and state. The page loads `/_wisp/wisp.js` automatically, and the browser takes
  over from that state.
- Assigning to a `~` variable re-renders the component. Lists keep their DOM (and
  focus) when items have `key={...}`. Nested components keep their own state.
- `@event={fn}` handles DOM events (`@click`, `@input`, `@submit`, ...). The function
  may take the event: `e.value`, `e.checked`, `e.key`, and `e.form` (the form's fields
  as a map). `@submit` does not reload the page.
- `bind:value={x}` and `bind:checked={x}` keep an input and a variable in step.
- Component code, and the functions, views and constants it uses, is compiled to
  JavaScript. Code that cannot run in a browser is an error when the program loads
  (and in `wisp check`): `request`, `db`, `http`, `env`, files, `reply`, or a global
  the server changes (pass it as a prop instead).
- In the browser, numbers are JavaScript numbers, so `2.0` prints as `2` and `7.0 / 2`
  is `3`.

### Server functions

`server fn` makes a function callable from the browser:

```wisp
server fn add_note(text: str) -> list ! str with db {
    if trim(text) == "" { return err("empty note") }
    db.exec("insert into notes (text) values (?)", text)?
    db.query("select * from notes")
}

component Notes(initial: list) {
    ~notes = initial
    ~draft = ""
    ~problem = nil
    <form @submit={fn() {
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

Each call is a `POST /_wisp/rpc/<name>` with a JSON array of arguments, checked against
the parameter types (a 400 otherwise); the answer is `{"ok": value}` or `{"err": payload}`.
Only `server fn`s can be called, only with `content-type: application/json` (so a form
on another site cannot), and inside one `request` is the current request. `reply` in a
server fn becomes the HTTP response, so the usual middleware works:

```wisp
server fn delete_all() = request |> auth.required |> fn(_) { ... }
```

From hand-written JavaScript: `await wisp.server.add_note("text")`.

## Routes

```wisp
route METHOD /path/:param/:typed(int)/*rest [? { query pattern }] [{ request pattern }] = expr
route METHOD /path { ... block ... }
```

- **Methods:** `GET POST PUT PATCH DELETE OPTIONS HEAD ANY`. A `HEAD` request is served by the `GET` route without a body.
- **Path params** become variables. `:id(int)` only matches what parses as an int (also `float`, `num`, `str`). `*rest` (or `*name`) takes the rest of the path.
- **Query pattern:** `? { q: str, page: int = 1 }`. Query values are text, so `int`, `float` and `bool` patterns convert them.
- **Request pattern:** matched against the `request` map. Fields: `json`, `form`, `headers`, `cookies`, `query`, `params`, `method`, `path`, `body`, `ip`. JSON is matched strictly: the string `"7"` is not an int. Form fields, headers and cookies are converted like query values.
- **Reserved:** paths under `/_wisp/` belong to Wisp (the browser bundle, server fn calls, `--watch`).
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
| nil | 204 |
| error | 500 `{"error": ...}` |
| `respond(status, body, headers)` | exactly that |
| `redirect(url, status = 303)` | a redirect |
| `file(path)` | a file under the program's folder (paths with `..` or `/` are refused) |

`reply status [body]` ends the request right away, from anywhere: inside a
helper, a pipeline stage or a `??` fallback. This is how middleware works:

```wisp
let auth = {
    required: fn(req) =
        if req.headers.authorization == "Bearer {env("TOKEN")}" then req
        else reply 401 { error: "unauthorized" }
}

route GET /me = request |> auth.required |> fn(r) = { ip: r.ip }

route GET /users/:id(int) {
    let user = users |> find(fn(u) = u.id == id) ?? reply 404 { error: "no user {id}" }
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
`use "lib.wisp"` runs another file once, into the global scope.

### Database

`db` is an embedded SQLite database. SQL problems are error values, so `?` and `??`
work on them.

```wisp
db.open("app.db")                                   // next to the program; ":memory:" also works
db.exec("create table if not exists todos (id integer primary key, text text)")
db.exec("insert into todos (text) values (?)", text)       // { changes, last_id }
db.query("select * from todos where id > ?", 10)           // a list of maps
db.one("select * from todos where id = :id", { id })       // a map, or nil
db.transaction(fn() { ... })                               // all or nothing
```

Without `db.open`, the database is `$WISP_DB`, or else in memory. A transaction rolls
back if its function fails, returns an error value or replies early.

### HTTP client

```wisp
let r = http.get("https://api.example.com/users/1")        // { status, ok, headers, body, json }
http.post(url, { name: "Ada" })                            // maps and lists are sent as JSON
http.request("PUT", url, body, { authorization: "Bearer {token}" })
```

Every HTTP status is a response; failing to connect is an error value.

## Checking

`wisp check app.wisp` finds mistakes without running the program, and `wisp run`
does the same first. Types are sets of possible types, and code without annotations
is `any`, so only definite mistakes are reported, and annotations make the checks
stricter. It reports unknown names (with suggestions), names used before they are
defined, wrong argument counts and types (for builtins too), operators that can never
work, division by zero, return and error types, effects, view and component props,
component code that cannot run in a browser, route mistakes, and unused variables
(a warning; name a variable `_x` to keep it).

## Concurrency and reloading

Requests are served by a pool of threads. Wisp code runs under one lock, so a handler
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
src/db.rs         db: SQLite
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
                  (headless Chrome), server and language server tests
```
