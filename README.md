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

This is version 0.1: a tree-walking interpreter in Rust with a built-in HTTP server.

```sh
cargo build --release
./target/release/wisp run examples/todo.wisp      # http://127.0.0.1:8080
./target/release/wisp repl
./target/release/wisp check app.wisp              # parse only
cargo test
```

`wisp run` executes the file's top level. If it declared any routes, it starts
serving them (`--port N`, `--host H`, or `$PORT`).

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

Type annotations are checked at runtime when the function is called and when it
returns: `int float num str bool list map html fn response any`. Add `?` to also
allow nil (`str?`).

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

Mistakes in the code itself (calling a non-function, adding a map to an int, a
failed type check) are runtime errors. In a route they become a 500 response,
and the server logs them.

`! ErrorType` in a signature is accepted but not checked yet.

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

## Routes

```wisp
route METHOD /path/:param/:typed(int)/*rest [? { query pattern }] [{ request pattern }] = expr
route METHOD /path { ... block ... }
```

- **Methods:** `GET POST PUT PATCH DELETE OPTIONS HEAD ANY`. A `HEAD` request is served by the `GET` route without a body.
- **Path params** become variables. `:id(int)` only matches what parses as an int (also `float`, `num`, `str`). `*rest` (or `*name`) takes the rest of the path.
- **Query pattern:** `? { q: str, page: int = 1 }`. Query values are text, so `int`, `float` and `bool` patterns convert them.
- **Request pattern:** matched against the `request` map. Fields: `json`, `form`, `headers`, `cookies`, `query`, `params`, `method`, `path`, `body`, `ip`. JSON is matched strictly: the string `"7"` is not an int. Form fields, headers and cookies are converted like query values.
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
- **Strings:** `upper lower trim split join contains starts_with ends_with replace slice`
- **Lists:** `map filter find any all reduce sum min max sort sort_by reverse first last push range enumerate slice`
- **Maps:** `keys values get has set remove merge`
- **Errors:** `err is_err unwrap`
- **Web:** `raw escape env now file redirect respond json.parse json.stringify`

Values are immutable: `push`, `set` and `+` return new lists and maps.
`use "lib.wisp"` runs another file once, into the global scope.

## Not built yet

These are from the original design and planned next:

- `~reactive` state and components compiled to the browser (JS/WASM)
- `server fn` calls from client code as automatic RPC
- `with db, http` effect declarations, and checking `! ErrorType`
- static type checking (types are checked at runtime today)
- concurrency (one request at a time), file watching and hot reload

## Layout

```
src/lexer.rs     tokens; newline handling
src/parser.rs    recursive descent: statements, expressions, patterns, HTML, routes
src/interp.rs    evaluator, pattern matching, HTML rendering
src/builtins.rs  standard library
src/server.rs    request matching, response encoding, HTTP server (tiny_http)
src/main.rs      CLI: run, check, repl
examples/        hello.wisp, orders.wisp, todo.wisp
tests/           language and routing tests
```
