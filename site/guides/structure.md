# Structuring a program

A Quillo program is a main `.qwe` file and the files it `use`s. There is no package
file, no import list at the top of each file, and no build step. As a program grows, a
few rules decide how well your editor and `quillo check` keep up with it. They are all
on this page, and each one was tried before it was written down.

### One main file, and the files it uses

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
  public/             files served as they are
```

```quillo
// main.qwe
db.open("data/shop.db")
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
component Cart() {
    state orders = load_orders()
    let total = orders | map((o) => o.cents) | sum()
    <section>
        <ul>{for o in orders => <li key={o.id}>{o.item}: {price(o.cents)}</li>}</ul>
        <p>Total {price(total)}</p>
    </section>
}
```

- **`use` runs a file once, into the program's one global scope.** Its functions, views,
  components, types, routes and variables belong to the whole program: `ui/cart.qwe`
  calls `price` and `load_orders` without naming the files they are in. A file `use`d
  twice, or by two files, still runs once, and files that `use` each other are fine.
- **Paths count from the main file's folder, in every file.** A `use` in `ui/cart.qwe`
  is written `use "lib/money.qwe"`, not `"../lib/money.qwe"`. Paths outside the folder
  work too (`use "../shared/auth.qwe"`). There are no globs: list each file.
- **Put the `use` lines in the main file.** A part can `use` what it needs, but one list
  in one place shows the whole program at a glance, and its order says what comes first.
- **Start each file with a comment** saying what it holds. It is the first thing a
  reader sees, and the [CRM](#a-larger-example) does it in all 47 of its files.

### What the order decides

- **Functions, views and components are looked up when they are called**, so their order
  does not matter: a helper in `lib/` can call one defined in a file `use`d after it.
- **Top-level values are worked out as each file runs**, so a `let` that reads another
  file's variable must come after that file. `quillo check` says when one does not:

```
lib/early.qwe:1: error: `LATER` is used before it is defined (line 1)
```

The order that works: pure helpers and constants first, then data (server fns,
queries, rules), then views and pages, as in `main.qwe` above.

### Names are global

There are no namespaces inside a program. Two files that define the same name do not
fail: one of them wins, without a word.

| Defined twice | The one used |
|---|---|
| Function, view, component | The last one loaded |
| Variable (`let`) | The last one loaded |
| Route (same method, path) | The first one loaded |

`quillo check` does not warn about either yet, so name things after their part:
`order_total` and `cart_total` rather than two `total`s.

When a part needs names of its own, give it a boundary:

| Use | When | Its names |
|---|---|---|
| `use "lib/x.qwe"` | Parts of one program | Shared with the whole program |
| `mount /blog = "blog.qwe"` | Another program under a path | Its own globals and its own database ([Mounting programs](/docs/routes/#mounting-programs)) |
| `service billing = "billing.qwe"` | A part others call, here or on its own server | Called as `billing.invoice(...)` ([Microservices](/docs/microservices/)) |

### What the browser gets

A component is compiled to JavaScript with the functions it calls, from whichever files
they are in: `Cart` takes `price` along to the browser. Those functions must only
compute. A function that reaches for the database, `http`, `env` or files cannot be
compiled for the browser, and the program refuses to start, naming it:

```
quillo: `order_count` (compiled for the browser): `order_count` declares `with db`, which the browser cannot do; make it a `server fn`
```

`quillo check` does not catch this yet when a helper is between the component and the
database; `quillo run` does, before it serves anything.

So keep the helpers components use in files of their own (`lib/money.qwe` above, the
CRM's `lib/util.qwe` and `lib/rules.qwe`), apart from the files that hold `server fn`s
and queries. It is then clear at a glance what runs where, and rules written once check
forms in the browser and saves on the server
([Validation](/docs/validation/)).

### What the editor needs

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
  knows nothing of the rest and reports every name it borrows.

#### Types where files meet

A function other files call is the program's interface, so give it types. Hover then
shows what it takes and gives, and every call to it is checked:

```quillo
fn price(cents: int) -> str = ...       // hover: fn price(cents: int) -> str
```

```
ui/cart.qwe:7: error: argument `cents` of `price` must be int, got str
```

Without types, `fn price(cents) = ...` takes anything and the mistake waits for a page
to show it. See [Records](/docs/language-tour/#records) for the shapes of maps that
cross between files.

### Editors

**VS Code.** The Quillo extension
([editors/vscode](https://github.com/quillolang/quillo/tree/main/editors/vscode)) brings
diagnostics as you type, hover, Go to Definition across `use`d files, completion (your
names, builtins with their signatures, the members of `db.`, `http.`, `channel.` and the
other modules, effects after `with`), an outline, snippets, and a debugger (F5, with
breakpoints, stepping and the variables). *Run this file* starts it with `--watch`. It
finds `quillo` in the `quillo.path` setting, then in the workspace's `target/release` or
`target/debug`, then on your `PATH`.

```sh
cargo install --path .            # in the Quillo repository: puts quillo on your PATH
cd editors/vscode
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

### Other files in a program

| Folder | Holds | Used by |
|---|---|---|
| `lib/*.qwer` | Rust, `use`d like a `.qwe` file | [Rust](/docs/rust/) |
| `migrations/` | SQL files, run in order, once | `db.migrate("migrations")` ([Database](/docs/database/)) |
| `locales/` | `en.json`, `fr.json`, ... | `translations("locales")` |
| `public/` | Images, fonts, CSS | `route GET /assets/*path = file("public/" + path)` |

### A larger example

The [CRM](https://github.com/quillolang/quillo/tree/main/apps/crm) in the repository is 47
files in this shape, with every `use` in its `main.qwe`:

```
main.qwe            tenants, routes, the plugin list, the job that runs flows and mail
migrations/         each workspace's tables
lib/util.qwe        dates, money, CSV, names: pure, so both sides use them
lib/rules.qwe       validate(kind, record, ctx): every plugin's rules for a kind
lib/records.qwe     contacts, companies, deals, tasks, notes, the dashboard
lib/commerce.qwe    products, the price quote, checkout, orders
lib/flows.qwe       the flow engine: fire, walk, waits, steps, templates
plugins/*.qwe       core (always on) and the optional plugins
ui/*.qwe            the pages: public ones, the app, the flow and page designers
```

Its `main.qwe` `use`s the pure helpers and the rules first, then the plugins, then the
data, then the pages: the order this page recommends.
