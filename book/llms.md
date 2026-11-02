# Quillo

> Quillo is a programming language for web apps. One `.qwe` file can hold the server's
> routes, the HTML (written like JSX), components that run in the browser, and SQL on a
> built-in SQLite. There is no package file, no build step and no JavaScript to write.
> `quillo run` serves the program; `quillo check` checks it without running it.

This file is for AI coding assistants. Quillo is new, so what you know from other
languages will lead you wrong in a few places, listed below. Rules for working on a
Quillo program:

- After every edit, run `quillo check` (in the program's folder, or `quillo check
  main.qwe`). It finds syntax errors, wrong types, misspelled fields, missing `import`s,
  JavaScript habits that mean something else here, and markup the browser would break.
  Fix what it says before you go on.
- `quillo fix` writes the `import` lines a program's files need.
- `quillo run --watch` serves at http://127.0.0.1:8080 and reloads on each save.
- Do not invent builtins. The list is at the end of this file and in
  https://quillolang.org/docs/standard-library/. When unsure, `quillo repl` answers
  quickly: `echo 'split("a,b", ",")' | quillo repl`.
- Do not add `package.json`, a bundler or a framework. A page's components need nothing
  but the program.

## Files

| File | Is |
|---|---|
| `main.qwe` | the program: what it opens, the files it uses, its routes |
| `lib/*.qwe`, `ui/*.qwe` | its parts, by convention: helpers and server fns; views and components |
| `migrations/NNN_name.sql` | the database's tables, run once each, in order, by `db.migrate("migrations")` |
| `public/` | files served as they are (with a route: `route GET /assets/*path = file("public/" + path)`) |
| `*.qwer`, `*.rs` | Rust a program `use`s (`quillo install <name>` fetches a `.qwer` plugin from quillolang.org/plugins) |

`quillo new my-app` makes a program to start from (`--minimal` for a page and a
component).

## The language in one screen

```quillo
let n = 42                       // let declares; = += -= reassign. No const, var, *=, ++
let price = 12.50m               // a decimal: exact, for money
let name = "Ada"                 // "..." and '...' are plain text; only `...` interpolates
let hello = `Hello, ${name}`
let xs = [1, 2, 3]
let user = { name, role: "admin" }   // { name } is { name: name }

fn add(a: int, b: int = 10) -> int = a + b
fn area(w, h) {
    let a = w * h
    a                            // a block gives its last expression; return only to leave early
}
let double = (x) => x * 2

let big = xs | filter((x) => x > 1) | map(double)    // a | f(b) is f(a, b)
let first_big = xs.filter((x) => x > 1).first()      // x.f(y) is f(x, y) too
let label = n > 0 ? "positive" : "not positive"
let squares = for x in xs => x * x                   // for gives a list
let kind = match user {
    { role: "admin" } => "staff"
    { role: str as r } => r
    _ => "nobody"
}
```

- Values are immutable: `push`, `set` and `+` return new lists and maps, so write `xs =
  xs | push(4)` and `m = set(m, "k", v)`, or spread: `{ ...user, name: "Grace" }`.
- `==` compares deeply and converts nothing: `1 == "1"` is false, `[1, 2] == [1, 2]` is
  true.
- Only `null`, `false` and error values are falsy. **`0` and `""` are truthy**, so
  `name || "anonymous"` keeps an empty name: write `filled(name) ?? "anonymous"`, and
  `count != 0 && <Badge/>`, not `count && <Badge/>`.
- `/` is exact: `7 / 2` is `3.5`, `6 / 2` is `3`; `floor(7 / 2)` divides whole numbers.
  Ints are 64-bit and overflow is an error, never a wrap.
- A field of `null` is `null`, so `user.address.city` needs no `?.` (there is none).
- `a..b` is the ints from `a` up to, not including, `b`: `for i in 0..10 => i`.
- Comments are `//` and `/* */`. A line break ends a statement; a line starting with
  `|`, `.method`, `&&`, `||`, `??`, `?`, `:` or `else` continues the one before.

## What Quillo does not have, and what to write

| Not in Quillo | Write |
|---|---|
| `const`, `var` | `let` |
| `function f() {}`, `def`, `func` | `fn f(x) = ...` or `fn f(x) { ... }` |
| `class`, interfaces, enums | `type User = { name: str, role: "admin" \| "member" = "member" }`, and functions |
| `try`/`catch`, `throw`, `raise` | errors are values: `return err("why")`, `f()?` passes one on, `f() ?? fallback` replaces it |
| `x in xs`, `k in m` | `contains(xs, x)`, `has(m, k)` |
| `break`, `continue` | `return` to leave; `filter` first to skip items |
| `1 < x < 3` | `1 < x && x < 3` |
| `x?.y` | `x.y` (a field of null is null) |
| `===`, `**`, `++`, `*=`, `/=`, bitwise `& \| ^ << >>` | `==`, `pow(a, b)`, `x += 1`, `x = x * 2`, `bit.*` (`\|` is the pipe) |
| `and`, `or`, `not`, `if c then a else b` | `&&`, `\|\|`, `!`, `c ? a : b` or `if c { a } else { b }` |
| `/regex/` literals | strings to `re.*`: `re.find_all(s, "@(\\w+)")` |
| `yield`, generators | return a list |
| `<>...</>` fragments | one element around the others |
| `className`, `onClick`, `onclick={...}` | `class`, `@click={...}` |
| `m.name = v` | `m = { ...m, name: v }` (inside a component it sets an element's property) |

Builtins are free functions that take the list, map or string first:

| JavaScript or Python | Quillo |
|---|---|
| `xs.map(f)`, `xs.filter(f)`, `xs.find(f)` | `map(xs, f)`, `filter(xs, f)`, `find(xs, f)` (`null` when none) |
| `xs.reduce(f, init)` | `reduce(xs, init, f)`: the start value first |
| `Math.max(a, b)`, `max(a, b)` | `max([a, b])`: `max`, `min`, `sum` take one list |
| `s.length`, `len(s)` | `len(s)`, counting characters |
| `s.indexOf(x)` | `index_of(s, x)`: `null`, not `-1` |
| `parseInt(s)`, `int(s)` | `int(s)`: an error value for text that is no number; `int(s) ?? 0` |
| `s.replaceAll(a, b)` | `replace(s, a, b)` |
| `xs.sort((a, b) => a.n - b.n)`, `sorted(xs, key=...)` | `sort_by(xs, (x) => x.n)` |
| `x.toFixed(2)`, `round(x, 2)` | `round(x, 2)` |
| `JSON.parse`, `JSON.stringify` | `json.parse`, `json.stringify` |
| `encodeURIComponent(s)` | `url_encode(s)` |
| `console.log`, `print` | `print(x)` |

## Types and records

Annotations are optional, checked when a function is called and by `quillo check`:
`int float dec num str date bool list map html fn response any`, `list<int>`, `{ title:
str, note: str? }`, unions `int | str`, `str?` for "or null". `-> T` is what a function
gives, `! E` what its errors hold.

```quillo
type User = { name: str, email: str, role: "admin" | "member" = "member", tags: list<str> = [] }

let ada = User({ name: "Ada", email: "ada@example.com" })   // checked, defaults filled
let bad = User({ name: "Ada" })     // err("User: `email` is missing"); quillo check reports it
```

`User(v)` drops fields the record does not have, so what a client sends cannot add
`is_admin`. A type's name is `User` or `OrderLine` (not `ID`, not `user`).

## Errors

```quillo
fn parse_age(s: str) -> int ! str = match int(s) {
    int as n if n >= 0 => n
    _ => err(`bad age: ${s}`)
}
fn next_year(s: str) = parse_age(s)? + 1     // ? returns the error from this function
let age = int("x") ?? 18                     // ?? replaces null or an error
```

`is_err(r)` tests, `r.error` reads the message. An error a statement drops is logged; an
error from `db.exec` should be passed on with `?`, or a transaction may keep half its
work. Inside a lambda, `?` leaves the lambda only: use a `for` loop to stop at the first
failure.

## Program structure

```quillo
// main.qwe
db.open("app.db")
db.migrate("migrations")

use "lib/notes.qwe"          // runs the file once and takes all its names
use "ui/page.qwe"

route GET / = <Page title="Notes"><NoteList/></Page>
```

```quillo
// ui/note_list.qwe: takes only what it calls
import lib/notes.qwe use load_notes, add_note
```

- Paths count from the main file's folder, in every file: `use "lib/x.qwe"` in
  `ui/a.qwe`, never `"../lib/x.qwe"`.
- A file that calls a name from another file takes it: `import path use name`, or the
  whole file with `use "path"`. `quillo fix` writes these lines for you.
- Start each file with a comment saying what it holds.

## Routes

```quillo
route GET /hello = "Hello"                                   // text
route GET /api/todos = db.query("select * from todos")       // a list or map is JSON
route GET /todos/:id(int) {
    let t = db.one("select * from todos where id = ?", id)
    t ?? respond(404, "no such todo")
}
route GET /search ? { q: str, page: int = 1 } = search(q, page)       // query pattern
route POST /todos { form: { text: str } } {
    db.exec("insert into todos (text) values (?)", text)?
    redirect("/")
}
route POST /api/users { json: User } = respond(201, save(json))      // body made a User
route GET /assets/*path = file("public/" + path)
```

- A route gives: HTML (200 html), a string (200 text), a map, list, number or bool
  (200 JSON), `null` (204), `respond(status, body, headers)`, `redirect(url)`,
  `file(path)`, `stream(...)` or `events(...)`.
- `request` has `path`, `query`, `form`, `json`, `body`, `headers`, `cookies`, `files`,
  `ip`. Prefer patterns in the route line to reading `request` by hand: a route whose
  pattern does not match lets the next route try.
- `reply 401 "Log in"` ends the request from anywhere, a helper too.
- Sessions: `set_session({ user })`, `session().user`, `end_session()`. Cookies:
  `set_cookie`, `delete_cookie`.

## HTML, views and components

```quillo
view Card(title: str, children) =
    <section.card><h2>{title}</h2>{children}</section>

component Counter(start: int = 0) {
    state count = start              // state: assigning it updates the page
    let double = count * 2           // recomputed when count changes
    <div>
        <span>{count} (x2 = {double})</span>
        <button @click={() => { count += 1 }}>+</button>
    </div>
}

route GET / = <html><body><Card title="Hi"><Counter start={5}/></Card></body></html>
```

- HTML is a value, written like JSX: `{expr}` interpolates and escapes; `raw(s)` is
  trusted HTML; `<li.done #item-{id}>` is shorthand for class and id; `class={ok &&
  "ok"}` leaves the attribute out when false or null; `{for t in todos => <li
  key={t.id}>{t.text}</li>}` renders a list.
- A `view` is made on the server only. A `component` is made on the server and resumes in
  the browser; nothing else on the page needs JavaScript.
- Events are `@click`, `@submit`, `@input`, `@change`: a lambda. Inputs bind with
  `bind:value={draft}` (and `bind:checked`).
- `style css`...`` at the start of a component's body is CSS for its elements only.
- A `server fn` is a function the browser calls over HTTP. In a component it is awaited
  for you and returns an error value if it fails:

```quillo
server fn add_task(text: str) -> list ! str with db {
    if trim(text) == "" { return err("a task needs some text") }
    db.exec("insert into tasks (text) values (?)", text)?
    db.query("select * from tasks order by id")
}

server fn list_tasks() -> list with db = db.query("select * from tasks order by id")

component Tasks() {
    state items = live(list_tasks)     // runs again when the tasks table changes, on every page
    state draft = ""
    state problem = null
    <form @submit={() => {
        let r = add_task(draft)
        if is_err(r) { problem = r.error } else { draft = "" }
    }}>
        <input bind:value={draft} aria-label="New task"/>
        {problem && <p>{problem}</p>}
        <ul>{for t in items ?? [] => <li key={t.id}>{t.text}</li>}</ul>
    </form>
}
```

- `state x = load()` with a `server fn` loads on the server while the page is made, so
  the page arrives with its data.
- A statement in a component body that reads state runs again when that state changes;
  what it starts (`every(ms, f)`, a listener) stops when it no longer runs. There is no
  `useEffect`, no dependency list and no `useMemo`: `let` is already derived.

## Database

```quillo
db.open("app.db")                    // SQLite, built in; postgres://... works too
db.migrate("migrations")             // migrations/001_init.sql, 002_..., each run once

let rows = db.query("select id, text from todos where done = ? order by id", 0)   // list of maps
let one = db.one("select * from todos where id = :id", { id: 7 })                 // a map or null
let r = db.exec("insert into todos (text) values (?)", "Write docs")              // { changes, last_id }
db.transaction(() => {
    db.exec("update accounts set cents = cents - ? where id = ?", 500, 1)?
    db.exec("update accounts set cents = cents + ? where id = ?", 500, 2)?
    { moved: 500 }                   // an error above rolls it all back
})
```

- Always pass values as `?` or `:name` placeholders; never build SQL with `+` or `${}`.
- A row is a map: `select a.id, b.id` leaves one `id`, so name columns apart with `as`.
- `with db` on a function says it uses the database; a function with `with` may do only
  what it declares (`db`, `http`, `fs`, `env`, `time`, `random`, `net`).

## Before you finish

- `quillo check` passes with no errors, and its warnings are read.
- Every error a statement can give is passed on (`?`), replaced (`??`) or taken (`let _ =`).
- `0` and `""` were not used as false.
- No builtin was guessed: each is in the list below.
