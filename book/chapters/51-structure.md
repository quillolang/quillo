# Structuring a program {#structure}

A Quillo program is a main `.qwe` file and the files it `use`s. There is no package file
and no build step, and each file says what it takes from the others. As a program grows,
a few rules decide how well your editor and `quillo check` keep up with it. They are all
in this chapter, and each one was tried before it was written down.

## One main file, and the files it uses

A small shop, split the way larger programs are:

```
shop/
  main.qwe            the program: its database, its parts, its routes
  lib/
    money.qwe         pure helpers: pages and the server both use them
    orders.qwe        server fns and queries
  ui/
    kit.qwe           views every page uses: the frame, buttons, cards
    cart.qwe          components
  migrations/         001_orders.sql, ...
  data/               the database: make the folder, db.open does not
  public/             files served as they are
```

```quillo
// main.qwe
db.open("data/shop.db") | unwrap      // stops here if data/ is missing
db.migrate("migrations")

use "lib/money.qwe"
use "lib/orders.qwe"
use "ui/kit.qwe"
use "ui/cart.qwe"

route GET / = <Frame title="Shop"><Cart/></Frame>
```

```quillo
// lib/money.qwe: pure, so pages and the server both use it.
fn price(cents: int) -> str = `€${floor(cents / 100)}.${slice(str(100 + cents % 100), 1)}`

// lib/orders.qwe
server fn load_orders() -> list with db = db.query("select * from orders order by id")

// ui/kit.qwe
view Frame(title: str, children) =
    <html><head><meta charset="utf-8"/><title>{title}</title></head><body>{children}</body></html>

// ui/cart.qwe
import lib/money.qwe use price
import lib/orders.qwe use load_orders

component Cart() {
    state orders = load_orders()
    let total = orders | map((o) => o.cents) | sum()
    <section>
        <ul>{for o in orders => <li key={o.id}>{o.item}: {price(o.cents)}</li>}</ul>
        <p>Total {price(total)}</p>
    </section>
}
```

And the table the queries read, `migrations/001_orders.sql`, with two rows to see:

```sql
create table orders (id integer primary key, item text not null, cents integer not null);
insert into orders (item, cents) values ('Mug', 1250), ('Tea', 499);
```

`quillo run shop/main.qwe` then serves `/` with `Mug: €12.50`, `Tea: €4.99` and `Total
€17.49`. Without `| unwrap`, a missing `data/` folder would be one line in the log (`db:
unable to open database file`) and the program would go on with SQLite in memory,
losing its orders at each restart.

- **`use` and `import` run a file once, and give this file its names.** `ui/cart.qwe`
  calls `price` and `load_orders` because it took them, and `main.qwe` sees `Frame` and `Cart` because
  it `use`s those files whole. A file `use`d twice, or by two files, still runs once, and
  files that `use` each other are fine.
- **Paths count from the main file's folder, in every file.** A `use` in `ui/cart.qwe`
  is written `use "lib/money.qwe"`, not `"../lib/money.qwe"`. Paths outside the folder
  work too (`use "../shared/auth.qwe"`). There are no globs: list each file. `quillo
  run` with no file, or a folder, runs that folder's `main.qwe`.
- **The main file lists the parts, and each part takes what it calls.** One list in one
  place shows the whole program at a glance, and its order says what comes first. A
  part's own `use` lines say what it depends on.
- **Start each file with a comment** saying what it holds. It is the first thing a
  reader sees.

## What the order decides

- **Functions, views and components are looked up when they are called**, so their order
  does not matter: a helper in `lib/` can call one defined in a file `use`d after it.
- **Top-level values are worked out as each file runs**, so a `let` that reads another
  file's variable must come after that file. `quillo check` says when one does not:

```
lib/early.qwe:1: error: `LATER` is used before it is defined (line 1)
```

The order that works: pure helpers and constants first, then data (server fns,
queries, rules), then views and pages, as in `main.qwe` above.

## What a file sees

A file sees its own names, the builtins, and what it takes:

```quillo
use lib/money.qwe                             // every public name of the file
import lib/money.qwe use price, tax           // only these
import lib/money.qwe use price as shop_price  // this one, under another name here
```

The file comes first, and the first word says what kind of thing it is. There are three
reasons for that order:

1. **Mental model.** The line reads in the order things happen: what kind of code this
   is, where it is, then which of its names you want. `import` is Quillo or Rust on the
   server; `client`, `preload` and `preconnect` are the browser. A glance at the start of
   a line tells you which side of the program you are on.
2. **Editor.** Once the file is written, the editor knows its names and offers them as
   you type, leaving out those you took. With the names first, there is nothing to offer
   until the path is there.
3. **Readability.** Lines line up by where things come from, so a file's sources read
   down the left, and names no longer sit after a long address at the far right.

0.6's `use price from lib/money.qwe` still works, with a warning, and `quillo fix` turns it
around. A later release removes it.

What a file takes is for that file, and is not passed on: when `ui/cart.qwe` takes from
`lib/money.qwe`, a file that `use`s `ui/cart.qwe` does not get `price` with it.

For now a name a file did not take still works, with a warning, so a program can move a
file at a time. A later release makes it an error.

```
ui/cart.qwe:7: warning: `price` comes from lib/money.qwe, which this file does not take: add `import lib/money.qwe use price` (`quillo fix` writes it)
```

`quillo fix` writes those lines, in every file of the program. Run it once for each
program, a service's or a mounted one's too:

```sh
quillo fix main.qwe
```

Two files may define one name. A `total` in `lib/cart.qwe` and a `total` in
`lib/invoice.qwe` are two functions, and a file's own name comes first. A file that
`use`s both whole and calls `total` is told to choose:

```
main.qwe:3: error: `total` comes from lib/cart.qwe and lib/invoice.qwe: take the one you mean: import lib/cart.qwe use total
```

| Defined twice | What happens |
|---|---|
| Function, view or component, in one file | An error |
| Function, view, component or top-level `let`, in two files | Allowed: two names, and each file takes the one it means |
| `server fn`, `type`, `service`, in one file or two | An error, naming both places: a `server fn`'s name is its address, and types and services are the program's |
| `let x` twice in one file | Allowed: the second gives the variable again |
| Route (same method, path) | Not reported: the first one loaded answers |

A `type` and a `service` are seen in every file without a `use`. There are no prefixes
yet: a file writes `import lib/auth.qwe use login` and calls `login()`, not `auth.login()`.

A file's own helpers need no `use` at all: mark them `private`, and only that file sees
them.

```quillo
// lib/cart.qwe
private let TAX = 0.12
private fn total(lines: list) = sum(lines | map((l) => l.price * l.qty))

public fn cart_total(lines: list) = total(lines) * (1 + TAX)
```

`private` goes before a `fn`, `view`, `component` or top-level `let`, at a file's top
level; `public`, which says the default, is optional. Another file that calls `total`
is told so, and editors offer it only in its own file:

```
main.qwe:3: error: `total` is private to lib/cart.qwe: only that file sees it (make it `public` there, or call something public)
```

A `server fn` cannot be private (it is an address on the server anyone can call: keep the
work in a private `fn` it calls), and neither can a `type` yet. A top-level `let` named like a builtin (`let last = ...`) or a module (`let
json = request.json`, `let date = ...`) hides it in every file of the program: name such
values otherwise (`body`, `created`).

When a part needs names of its own, give it a boundary:

| Use | When | Its names |
|---|---|---|
| `use lib/x.qwe` | Parts of one program | Seen by the files that take them |
| `private fn`, `private let` | A file's own helpers | That file's only |
| `mount /blog = "blog.qwe"` | Another program under a path | Its own globals and its own database ([Mounting programs](#mounting-programs)) |
| `service billing = "billing.qwe"` | A part others call, here or on its own server | Called as `billing.invoice(...)` ([Microservices](#microservices)) |

## What the browser gets

A component is compiled to JavaScript with the functions it calls, from whichever files
they are in: `Cart` takes `price` along to the browser. Those functions must only
compute. A function that reaches for the database, `http`, `env` or files cannot be
compiled for the browser, and the program refuses to start, naming it:

```
quillo: `order_count` (compiled for the browser): `order_count` declares `with db`, which the browser cannot do; make it a `server fn`
```

`quillo check` does not catch this yet when a helper is between the component and the
database; `quillo run` does, before it serves anything.

A library that runs in the browser starts with `client`, whether it comes from a CDN or
from your own assets: the path says which. `preload` or `preconnect` in its place says
how the page gets ready for it, and a long address may leave its `use` to the next line.
Each file that calls it writes the line, and the page loads it once ([JavaScript
libraries](#javascript-libraries)):

```quillo
preload "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js"
    use Chart
client /assets/leaflet.js use L
```

So keep the helpers components use in files of their own (`lib/money.qwe` above), apart
from the files that hold `server fn`s and queries. It is then clear at a glance what runs where, and rules written once check
forms in the browser and saves on the server
([Validation](#validation)).

## What the editor needs

Open any file, and the language server finds the main file it belongs to: it looks in
the file's folder and up to three folders above it, stopping at the project's root (a
folder with `.git` or `Cargo.toml`), for a `.qwe` file that `use`s it, directly or
through other files. It then checks the whole program from there, including what is
unsaved in other open files. That is how `ui/cart.qwe`, opened alone, knows `price`:
there are no false "unknown name" errors, **Go to Definition** opens `lib/money.qwe`, and
hover shows the signature.

Three things keep that working:

- **Keep files within three folders of the main file.** `a/b/c/x.qwe` is found from
  `main.qwe`; `a/b/c/d/x.qwe` is too far, so the editor reports "unknown name" for
  everything it uses from the rest of the program, though the program runs.
- **One program per folder.** If `admin.qwe` and `main.qwe` sit side by side and both
  `use` a file, the editor checks that file as part of the first by name (`admin.qwe`),
  and anything only `main.qwe` defines shows as unknown there. Put the second program in
  a folder of its own, or `mount` it.
- **Check the main file.** `quillo check main.qwe` checks everything it `use`s, with each
  problem at its file and line. A part checked on its own, `quillo check ui/cart.qwe`,
  counts its paths from its own folder, so it does not find the rest of the program.

### Types where files meet

A function other files call is the program's interface, so give it types. Hover then
shows what it takes and gives, and every call to it is checked:

```quillo
fn price(cents: int) -> str = ...       // hover: fn price(cents: int) -> str
```

```
ui/cart.qwe:7: error: argument `cents` of `price` must be int, got str
```

Without types, `fn price(cents) = ...` takes anything and the mistake waits for a page
to show it. See [Records](#records) for the shapes of maps that
cross between files.

## Editors

**VS Code.** The Quillo extension
([quillolang/editors](https://github.com/quillolang/editors)) brings
diagnostics as you type, hover, Go to Definition across `use`d files, completion (your
names, builtins with their signatures, the members of `db.`, `http.`, `channel.` and the
other modules, effects after `with`), an outline, snippets, and a debugger (F5, with
breakpoints, stepping and the variables). *Run this file* starts it as `quillo run file.qwe --watch`, which reloads it
when a file changes (an option of `quillo run`; see [the options](#running-quillo)). It
finds `quillo` in the `quillo.path` setting, then in the workspace's `target/release` or
`target/debug`, then on your `PATH`. Errors on code that runs fine usually mean an old
`quillo` was found: `quillo --version` names its commit.

```sh
cargo install --git https://github.com/quillolang/quillo quillo   # puts quillo on your PATH
git clone https://github.com/quillolang/editors.git && cd editors/vscode
npm install && npm run package    # builds quillo-lang-<version>.vsix
code --install-extension quillo-lang-*.vsix
```

**Other editors** run `quillo lsp`, over stdin and stdout, for `*.qwe` files. In Neovim:

```lua
vim.filetype.add({ extension = { qwe = "quillo" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "quillo",
  callback = function() vim.lsp.start({ name = "quillo", cmd = { "quillo", "lsp" } }) end,
})
```

The extension's README has Helix's settings, and `quillo dap` for debuggers in other
editors.

**While it runs**, `quillo run main.qwe --watch` reloads the program when it or any file
it `use`s changes, and refreshes the pages you have open.

## Other files in a program

| Folder | Holds | Used by |
|---|---|---|
| `lib/*.qwer` | Rust, `use`d like a `.qwe` file | [Rust](#rust) |
| `migrations/` | SQL files, run in order, once | `db.migrate("migrations")` ([Database](#database)) |
| `locales/` | `en.json`, `fr.json`, ... | `translations("locales")` |
| `public/` | Images, fonts, CSS | `route GET /assets/*path = file("public/" + path)` |
