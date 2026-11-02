# Appendix A: Example programs {#examples}

Each program in **github.com/quillolang/examples** shows one part of Quillo, whole.
Clone them beside Quillo's own folder, as the chapters do, and run one:

```sh
git clone https://github.com/quillolang/examples.git quillo-examples
quillo run quillo-examples/notes.qwe --watch          # http://127.0.0.1:8080
```

`index.qwe` mounts all of them in one server, each under its own path, behind tabs.

| Program | What it shows |
|---|---|
| `hello.qwe` | Routes: path parameters, typed query patterns and JSON answers in fifteen lines |
| `orders.qwe` | Payload routing: one path, several handlers, chosen by what is inside the JSON body; middleware |
| `todo.qwe` | A server-rendered app with forms, redirects and a JSON API next to it |
| `notes.qwe` | SQLite on the server, a live component in the browser, in sync across tabs |
| `compose.qwe` | Views and components built from each other: props down, callbacks up, slots |
| `layout.qwe` | A page described as JSON, rendered by one view, with a live editor beside it |
| `spa.qwe` | A single-page app: client-side navigation, deep links, 404s, a cart that survives pages and live stock |
| `scroll.qwe` | Infinite scroll: 100,000 rows, loaded 200 at a time, with only the visible ones in the page |
| `virtual.qwe` | A virtual list: a million rows of ten columns, twenty on screen; drag the scrollbar anywhere |
| `life.qwe` | Conway's Game of Life: a board in a component's state, stepped by a timer that runs in the browser |
| `storm.qwe` | A dashboard under a storm of random changes, tested for leaks |
| `telemetry.qwe`, `sensors.qwe` | Sensors send readings over UDP; every page sees them as they come, over a channel |
| `media.qwe` | A camera's RTP stream, played in the page a fraction of a second behind |
| `rust.qwe`, `rust.qwer` | Primes and Collatz chains in a `.qwer` file, called from a component through server functions |
| `services/` | A shop that bills through a billing service, in one process or split apart (`app.qwe`, `shop.qwe`, `billing.qwe`) |
| `index.qwe` | Every example mounted under its own path in one server |

## The sample app

Quillo's own repository has a small to-do app in `app/`, laid out the way larger
programs are (see [Structuring a program](#structure)). `quillo new hello` writes the same
eight files into a folder of your own (`quillo new hello --minimal` writes a page, a
component and a function both sides use instead); edit them while they run to start your
own:

```
app/
  main.qwe              the program: its database, the files it uses, its routes
  lib/todos.qwe         the Todo record, queries and server functions
  ui/page.qwe           the frame every page uses
  ui/todos.qwe          a component, rendered on the server and run in the browser
  migrations/           SQL, run once, in order
  public/               served under /static/: css/, js/ (called from the component), icons/
```
