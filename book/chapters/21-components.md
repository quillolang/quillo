# Components

A `component` is a view with state that also runs in the browser:

```quillo
component Counter(start: int = 0) {
    state count = start                  // state: set once; assigning it updates what shows it
    let double = count * 2               // recomputed on every render
    <div>
        <span>{count} (x2 = {double})</span>
        <button onclick={() => { count += 1 }}>+</button>
    </div>
}

route GET / = <html><body><Counter start={5}/></body></html>
```

- The server renders it like a view, inside a `<quillo-island>` that carries its props
  and state. The page loads its bundle automatically, and the browser resumes the
  component from that state, on the server's own elements (see below). The island is
  `display: contents`, so it does not change the layout, but it is still an element:
  a child selector such as `.posts > li` does not reach the component's root element
  (write `.posts li`, or give the component's root the class), and `:first-child` or
  `:nth-child` count the islands, so each component in a list is a "first child" of its
  own island. A component whose root
  is a table's or a select's part (`tr`, `td`, `th`, `thead`, `tbody`, `tfoot`,
  `caption`, `colgroup`, `option`, `optgroup`) has no island element, since the browser
  would move it out of the table: its root element carries the island (`data-qi`), so a
  row component is a row; the island's attributes on it are the server's, and those the
  markup writes of their names are left out. Inside an `<svg>`, which draws no
  `<quillo-island>`, a component whose root is one element (`<rect>`, `<g>`, `<a>`, an
  inner `<svg>`) is its own island the same way, from the server and when the browser
  makes it, a deferred one too; outside an `<svg>`, and in a `<foreignObject>`, it is in
  its island as before. `raw(...)` in a component is likewise in a
  `<quillo-raw style="display: contents">`, which a child selector does not see through.

  ```quillo
  component Row(name: str, qty: int) {
      <tr><td>{name}</td><td>{qty}</td></tr>
  }

  view Stock() = <table><tbody>
      <Row name="lamp" qty={2}/>
      <Row name="desk" qty={1}/>
  </tbody></table>
  ```

  Each row is `<tr data-qi="" data-c="Row" ...>` in the page, with no island around it,
  so `tbody > tr` finds both.
- `state count = start` declares state, set once (`~count = start` is the short form).
  After that it is just `count`, and assigning to it (`count += 1`) updates what reads
  it, and nothing else. A component's code runs once. A `let` that reads state or props
  (`let double = count * 2`) is worked out again when they change. So is a statement
  that reads them (`if running { every(...) }`). An assignment written alone in the body,
  `count = len(items)`, runs once, as the component starts, and does not follow what it
  reads: `quillo check` warns of one that reads a prop or another state (`` `count = ...`
  runs once, as <T> starts: it does not follow `items` when that changes; for a value that
  follows it, write `let count = ...` in place of the state ``), so a value worked out
  from others is a `let`. Each place in the
  markup that shows a value follows only that value: changing one row's text changes that
  text. Assigning the value a state already has does nothing.

  ```quillo
  component Total(price: int) {
      ~qty = 1                                  // the same as `state qty = 1`
      let total = price * qty                   // worked out again when qty changes
      <p>{qty} x {price} = {total} <button onclick={() => { qty += 1 }}>+</button></p>
  }
  ```

  `<Total price={3}/>` shows `1 x 3 = 3`, and `2 x 3 = 6` after a click.
- `for` in markup keeps a row for each item: by its `key={...}`, else by its place. The
  key is the row's element's, written after `=>` or as the value of a block
  (`for p in xs { let n = p.name; <li key={n}>{n}</li> }`, whose statements the key may use).
  `key` is the list's, not passed to the view or component, so neither may take a
  parameter named `key` (a load-time error); an attribute written twice on one element
  is an error too.
  A row that is a choice (`? :`, `if`/`else`, `match`) of elements that each have a `key`
  is keyed by it; with one branch that has none, the rows are kept by place.
  A row whose item changed is updated where it shows what changed, and moved rows keep
  their DOM, focus and the selection in a field (in browsers without
  `Element.moveBefore`, Safari, the focus and selection are put back after the move).
  `if`, `? :`, `match` and `cond && <markup>` in markup make a
  branch again only when another one is chosen: an input in one keeps its focus and
  what was typed while the page updates. Nested components keep their own state.

  ```quillo
  component Todos() {
      state items = [{ id: 1, text: "milk" }, { id: 2, text: "bread" }]
      <div>
          <ul>{for it in items => <li key={it.id}><input/> {it.text}</li>}</ul>
          <button onclick={() => { items = reverse(items) }}>Reverse</button>
      </div>
  }
  ```

  Type in the first row's input and press Reverse: "bread" comes first, and what you
  typed moves with "milk", as its row was moved, not written again.
- `@event={fn}` handles DOM events (`@click`, `@input`, `@scroll`, `@submit`, ...), on
  HTML elements only (on a view or component tag it is an error). `onclick={fn}` is the same
  handler, written as HTML and JSX write it, and the form this guide's examples use
  (`onsubmit`, `oninput`, `onmount`, ...); on a view or component, `on_change={...}` or
  `onChange={...}` is a parameter like any other. There are no
  modifiers: `@click.prevent` is taken as an event named `click.prevent`, which never
  fires; call `e.preventDefault()`. Event names are not checked, so a misspelt one
  (`@clik`) is silent. Clicks are handled once, at the document, innermost first, so
  `e.stopPropagation()` stops outer handlers, and a library that stops a click before
  it reaches the document hides it from Quillo; other events listen on their element.
  The function may take the event `e` (it gets it only if it declares a parameter):
  - `e.value` and `e.checked` of the input (`null` and `false` when it has none), `e.key`
    of a key press, `e.type`;
  - `e.form`: the fields of the form, as a map (a file field gives the file's name; a
    name given twice, the last), with the name and value of the button that sent it,
    as the browser sends them;
  - `e.target`: the element itself;
  - any other field of the browser's event (`e.clientX`, `e.deltaY`, `e.shiftKey`,
    ...), then of the element (`e.scrollTop`, `e.scrollHeight`, `e.clientHeight`,
    `e.dataset`, ...);
  - its methods: `e.preventDefault()` keeps the browser from doing what it would (an
    arrow key moving the cursor, a pasted image going into a textarea). It must come
    before anything the handler waits for (a `server fn`), since by then the browser
    has gone on.

  ```quillo
  component Search() {
      state sent = ""
      <form onsubmit={(e) => { sent = e.form.q }}>
          <input name="q" onkeydown={(e) => { if e.key == "Escape" { e.value = "" } }}/>
          <button>Go</button>
          <p>{sent}</p>
      </form>
  }
  ```

  Submitting shows what was typed and the page stays; Escape empties the input.

  Setting a field sets the element's, when the event has no field of that name:
  `e.value = ""` empties the input (a file picker too, so that picking the same file
  again fires `change`). `@submit` does not reload the page. HTML5 drag and drop
  (`e.dataTransfer`), pointer capture (`e.target.setPointerCapture(e.pointerId)`) and
  SVG markup work as they do in the browser.
- What a person changes in the page is two-way by itself *(since 0.7)*: `<input value={name}/>`, where
  `name` is the component's state and nothing on the input sets it, keeps the input and
  `name` in step, typing included. The same for each thing a person can change:

  | Element | Attribute | Changed by the person on |
  |---|---|---|
  | `<input>` (text, number, date, range, color, ...), `<textarea>`, `<select>` | `value` | `input` |
  | `<input type="checkbox">` | `checked`, `indeterminate` | `change` |
  | `<details>` | `open` | `toggle` |
  | `<dialog>` | `open` | `close` |
  | `<video>`, `<audio>` | `muted`, `volume` | `volumechange` |
  | `<video>`, `<audio>` | `currentTime` | `timeupdate` |
  | `<video>`, `<audio>` | `playbackRate` | `ratechange` |

  ```quillo
  component Settings() {
      state name = ""
      state notify = true
      state more = false
      state loud = 0.8
      <form>
          <input value={name}/>
          <input type="checkbox" checked={notify}/>
          <details open={more}><summary>More</summary>...</details>
          <audio src="/bell.mp3" volume={loud}></audio>
          <p>{name}, {notify ? "notified" : "quiet"}, volume {loud}</p>
      </form>
  }
  ```

  A number or range input gives a number back (`null` when empty, or only spaces), so
  `qty + 1` after typing 3 is 4, and text being typed that already reads as the state's
  number is left as typed (`1.0`, `2.50`, `1e3` are not rewritten to `1`, `2.5`, `1000`).
  A text input, `<textarea>` or `<select>` bound to a state that holds a number gives a
  number when the text is that number's own form, and text otherwise (`"02"` stays text),
  `null` when empty; bound to a text state it gives text. A range's value is set again
  when its `min`, `max` or `step` change, so a value set with a new `max` is kept, and one
  past the new limits is brought into them, as the browser does. A player's `volume` is
  kept between 0 and 1 and its `playbackRate`
  between 1/16 and 16, whatever the state says, and a place the player has just reported
  is not sought back to while it plays.

  It stays one way, the attribute held to the value, when:
  - the element has a handler of its own on an event that says it changed (`@input` or
    `@change` for a value, `@change` or `@click` for a box, `@toggle` or `@close` for
    `open`, the player's own events): a transform, `value={code} @input={(e) => { code =
    upper(e.target.value) }}`, or a check before the value is taken;
  - it is `readonly` (but not `readonly={false}`);
  - the value is not a state (a `let`, a prop, an expression): an input showing a `let`
    keeps what is typed and sets nothing;
  - what the element is cannot be known when the program is compiled: a `type={...}`
    decided as it runs, or a `{...spread}` (which may hold a handler);
  - it is a radio (unticking one fires no event, so each cannot follow a state of its
    own: give the group one state and `checked={size == "s"}`), or a `<select multiple>`,
    whose `value={list}` marks each option the list names, on the server and in the
    browser.

  A `<select value={x}>` is written on the server as `selected` on the first option `x`
  names (by its `value`, else by its text, spaces collapsed), or none, which shows the
  first; the select itself gets no `value` attribute, which HTML does not have. In the
  browser the value is set once its options are in, and again when they change, so
  options made from a list that arrives later still show the one `x` names; what a person
  picked is kept while the page updates, unless options are added or taken away.

  `quillo check` warns of an input whose value is a parameter (a view's, or a component's
  prop) or a state's field (`value={form.name}`) when nothing on it takes what is typed: no
  handler, no `bind:`, no `name=` for a form sent to a route, not `readonly` or `disabled`.
  The input would show the typed text while the program kept what it had: "typing here
  changes nothing: `v` is `Field`'s parameter, which `value=` only shows". Pass the state's
  own input down, give the field a state of its own, or take what changes with `@input`.

  `value` on a checkbox, radio, file or button input is not what a person changes, and
  stays an attribute.
- `bind:x={y}` says the same in so many words, for any attribute in the table, and also
  binds a local variable, and works with a handler of its own: `bind:value` with `@input`
  is one listener, the binding first, so the handler reads the state already set:
  `<input bind:value={amount} @input={refresh}/>`. `y` is the component's state or a
  local, not a global or a map's field (give each input its own state, and make the map
  with a `let`). On a custom element (`<my-field>`), `bind:value` and `bind:checked` listen
  to `input` and `change`. Any other `bind:` is an error that names those there are.
- An input with `value={x}` (or `checked`, `selected`, and a `<video>`'s or `<audio>`'s
  `muted`) that is one way is held to `x`: after each handler it is set back to the state,
  so typing the state does not take is undone. A bare `<input>` keeps what is typed, read
  through `e.value` or `e.form`.

  ```quillo
  component Pin() {
      state pin = ""
      <input value={pin} oninput={(e) => { if len(e.value) <= 4 { pin = e.value } }}/>
  }
  ```

  The handler takes four characters; a fifth is typed, not taken, and undone, so the
  input still shows `1234`.
- `style={...}` takes text or a map of properties *(since 0.7)*: `style={{ color: tone, font_size:
  "2rem", marginTop: "3px", "--gap": "4px" }}`, where `_` and camelCase are written as
  `-` (`font-size`, `margin-top`), a custom property is kept as it is, and a `null` or
  `false` value is left out; the same on the server and in the browser. A number is in
  pixels, as React has it, except `0` and React's unitless properties (`opacity`,
  `z-index`, `line-height`, `flex`, `font-weight`, `order`...) and custom properties:
  `{ fontSize: 12, opacity: 0.5, "--gap": 4 }` is `font-size: 12px; opacity: 0.5; --gap: 4`
  *(since 0.7: `12` was written bare, and the browser dropped it)*. `ms` starts a prefix as
  `Webkit` and `Moz` do: `msFlex` is `-ms-flex`. A value's `{`, `}`, `;` and `\` are
  written as CSS's escapes (`\7b `, `\7d `, `\3b `, `\5c `), so a value from data is one value
  and adds no declaration: `{ color: "red;background:url(//evil)" }` is one colour the
  browser does not take. A key is a property's name (letters, digits, `-` and `_`) or a
  custom property's; another key is left out *(since 0.7, FEEDBACK51.md, Q-668)*.
- `class={...}` takes text, a map (the keys whose values are true) or a list (its texts,
  lists inside it flattened, `null`, `false`, `true` and `""` skipped, a map in it giving
  its true keys), as `clsx` does: `class={{ active: on, off: !on }}`, `class={["card", big
  && "big"]}` *(since 0.7: they were written as Quillo text)*. Quillo's truthiness counts,
  so `{ d: 0 }` gives `d`. Nothing left gives no `class` at all. After an update the
  element has exactly the new classes.
  `style={...}` and `class={...}` change only what they name when what they read
  changes: a `transform` or a class a handler set by hand stays, so a drag does not jump
  home. `<details open={x}>` and `<dialog open={x}>` follow `x` when `x` changes, and
  leave what a person opened or closed alone otherwise (FEEDBACK28.md, Q-337, Q-338).

  ```quillo
  component Badge(tone: str, big: bool) {
      <p style={{ color: tone, font_size: "2rem", margin_top: big ? "1rem" : null, "--gap": "4px" }}>New</p>
  }
  ```

  `<Badge tone="teal" big={false}/>` writes `style="color: teal; font-size: 2rem; --gap:
  4px"`: `margin-top` is left out.
- `==` on two of the page's objects (elements, events) asks whether they are the same one
  (Q-334).

  ```quillo
  component Modal() {
      state open = true
      state backdrop = null
      <div class="backdrop" onmount={(el) => { backdrop = el }}
           onclick={(e) => { if e.target == backdrop { open = false } }}>
          {open && <div class="box">Click outside to close</div>}
      </div>
  }
  ```

  A click on the box reaches the backdrop's handler too, with the box as `e.target`, so
  only a click on the backdrop itself closes it. (`e.currentTarget` is the document, as
  clicks are handled there.)
- Assigning state updates the page a moment later (once the handler's synchronous code
  is done, or at its first wait), all changes at once. A state is changed when it gets a
  different value: assigning the same list again does nothing, and assigning an equal
  but new list updates what reads it. Code that sets what it reads goes round in a loop,
  and is stopped after 1,000 rounds with a message in the console. See
  [Reactivity](#reactivity) for the whole model.

  ```quillo
  component Pair() {
      state a = 0
      state b = 0
      <p onclick={() => { a += 1; b = a * 10 }}>{a} and {b}</p>
  }
  ```

  A click shows `1 and 10` at once; the page never shows `1 and 0` in between.
- `every(ms, f)` in a component's code calls `f` every `ms` milliseconds, in the browser,
  for as long as the code that called it stands: `if running { every(100, step) }`
  runs a game while `running` is true, starts again with the new speed when that changes,
  and stops when `running` is false or the component leaves the page. On the server it
  does nothing. It is called from the component's code (or a `let` or `if` there), or
  from one of its event handlers or its `@mount`, where it runs until the component
  leaves the page (started after a handler's first wait, it belongs to no one, and is an
  error). A tab in the background is slowed by the browser to once a second at most.
  A tick that finds that timer's last run still waiting does nothing, so a poll slower
  than its timer runs one call at a time and shows each answer; a run with no answer
  after 30 seconds (or two gaps, if that is longer) is given up on, and the next tick
  starts anew. A clock's tick should not share an `every` with a call that waits: give
  it its own.

  ```quillo
  component Stopwatch() {
      state running = false
      state tenths = 0
      if running { every(100, () => { tenths += 1 }) }
      <p>{tenths / 10} s <button onclick={() => { running = !running }}>{running ? "Stop" : "Start"}</button></p>
  }
  ```

  Start counts in tenths of a second; Stop ends the `every`, as the `if` that called it no
  longer stands.
- `run_after(ms, f)` *(since 0.7)* calls `f` once, `ms` milliseconds on, unless its component has gone
  first: from a handler, `@mount` or the component's code alike. There is no
  `setTimeout`; a debounce or a throttle is an `every` under a state flag that the tick
  turns off. `page_updated()` *(since 0.7)* in a handler waits until the page shows what the handler
  has set so far, to read or focus what that made appear (FEEDBACK28.md, Q-349).
  `listen(url, f)` follows a stream the same way (see [Streaming](#streaming)), and opens
  it again when the connection drops (after a second, doubling up to 30 s), not when the
  stream ends.

  ```quillo
  component Save() {
      state saved = false
      <p>
          <button onclick={() => { saved = true; run_after(2000, () => { saved = false }) }}>Save</button>
          {saved && <span>Saved</span>}
      </p>
  }

  component Names() {
      state names = ["Ana"]
      <div>
          {for n in names => <input class="name" value={n}/>}
          <button onclick={() => {
              names = names + [""]
              page_updated()                    // the new input is in the page now
              js("document.querySelector", ".name:last-of-type").focus()
          }}>Add</button>
      </div>
  }
  ```

  "Saved" shows for two seconds. Add puts a new input in the page and focuses it, which it
  could not do before `page_updated()`, as the input was not there yet.
- Component code, and the functions, views and constants it uses, is compiled to
  JavaScript. Code that cannot run in a browser is an error when the program loads
  (under [`--watch`](#options-of-quillo-run), the option for working on a program, `quillo run main.qwe --watch`: in the log right after the load, and in the browser's console of
  a page that needs the component; and in `quillo check`, for what the component calls
  directly): `request`, `db`, `env`
  (but for variables named `PUBLIC_...`), files, `reply`, or a global the server changes
  (pass it as a prop instead). `http.*` runs in the browser, from event handlers (see
  [What the browser can reach](#what-the-browser-can-reach)). Any name is fine in
  component code, JavaScript's own words included (`new`, `class`, `this`, `window`): the
  compiler renames them.

  ```quillo
  component Orders() {
      state rows = db.query("select * from orders")
      <p>{len(rows)} orders</p>
  }
  ```

  ```
  main.qwe:2: error: `db.query` is not available in the browser, where <Orders> also runs; use a `server fn`
  ```

  Ask the server for them instead: `server fn orders() -> list with db = db.query("select * from
  orders")`, then `state rows = orders()`.
- What the browser has of the standard library: text, lists, maps, numbers, errors,
  `raw`, `escape`, `url_encode`, `now`, `random`, `random_int`, `uuid`, `format_number`,
  `markdown` (made by the server), `highlight`, `t` and `locale`, `json`, `re`, `math`,
  `stats`, `vec`, `date`, `money`, `http.*`, and what only a page has: `url`,
  `navigate`, `every`, `listen`, `receive`, `ask`, `notify`, `js`, `js_new`,
  `channel.open`. `print` writes to the console. Not there, and an error when the
  program loads: `env` (but `PUBLIC_`), `sleep`, files, `redirect`, `respond`,
  sessions and cookies, `crypto`, `image`, `db`, `publish` and `watch`, tenants,
  uploads, `schedule`, `stream`, `events`, `http.stream`, `join_on`, services.

  ```quillo
  component Receipt(total: float, at: str) {
      <p>{format_number(total)} on {at}, ref {uuid()}</p>
  }
  ```

  `<Receipt total={1234.5} at="2026-11-03"/>` shows `1,234.5 on 2026-11-03, ref` and a
  uuid, on the server and in the browser alike. `env("PUBLIC_SHOP")` may join them;
  `env("DB_URL")` may not.
- There is nothing built in for `document`, `window`, `localStorage`, the clipboard,
  focus or scrolling: reach them through the page, `js("localStorage.getItem", k)`,
  `js("navigator.clipboard.writeText", s)`, `js("window.innerWidth")` (no arguments
  reads a value), and `el.focus()` on an element (see [JavaScript
  libraries](#javascript-libraries)).

  ```quillo
  component Theme() {
      state dark = false
      <button onclick={() => {
          dark = !dark
          js("localStorage.setItem", "theme", dark ? "dark" : "light")
      }}>{dark ? "Dark" : "Light"}</button>
  }
  ```

  `js("document.querySelector", "#name").focus()` focuses an input the same way.
- Each page loads two scripts, minified, whose addresses change with their content, so
  the browser keeps them for good:
  - `/_quillo/r/<hash>.js`, the runtime: the parts of it the program's components
    reach (about 35 KB, 11 KB gzipped, for a page with one counter). Every page of the program
    loads the same one, so the browser downloads it once.
  - `/_quillo/b/<hash>.js`, the page's own code: the components it was rendered with,
    every component they can show, and what they use. Pages with the same components
    share it, whichever of them each showed.

  A page with no components loads no script. The server keeps the bundles of the last
  256 sets of components pages were made of (`quillo_bundles` under [`--metrics`](#options-of-quillo-run)); a
  page that names one no longer kept is sent the whole program's. There is no splitting
  by route and no lazy component: an `App` whose `match` shows thirty pages carries the
  thirty in its bundle. Under `--watch` a page loads one unminified bundle instead,
  which loads a file per component and function (see [Debugging](#in-the-browser)).

  ```sh
  curl -s localhost:8080/ | grep -o '/_quillo/[rb]/[^"]*'
  # /_quillo/r/f68b2dc51ad5ad52.js
  # /_quillo/b/0767fa4dd4c7b224.js
  ```
- And only when it is needed: the server's HTML shows at once, and the bundle comes
  when someone reaches for a component (points at, touches or focuses it, or presses a
  key in it). What they typed before it came is kept, and each click, double click,
  right or middle click, press and release is done again then as it was made: on the
  same element, at the same point, with the same buttons, keys and click count (a
  `@pointerdown` may still capture a pointer whose finger was lifted meanwhile). A
  click waits for the code, so a checkbox ticks and a link is followed then; should the
  code fail to load, they are done as the page alone does them. (The idea comes from
  Flickr, Google's JSAction, Angular's preboot and event replay, Qwik and React: see
  Acknowledgements in the [README](README.md#acknowledgements).) A
  component that runs by itself (`every`, `live`, `listen`, `js`, `js_new`, `@mount` in
  its code or markup, or in a component nested inside it) or that is the page (in
  `<body>` itself, an app) loads as soon as the page is idle. `/_quillo/quillo.js` has
  every component, for pages that load it themselves. Text responses are gzipped for
  browsers that accept it.

  Of the components above, `Total` and `Todos` wait for a person to reach them;
  `Stopwatch` (an `every` in its code) and `Modal` (an `@mount`) load as soon as the page
  is idle.
- A component is an element `<quillo-island data-c="Name" data-p="{props}"
  data-s="{state}">`, with `data-l` and `data-v` for live state and `data-e` when it runs
  by itself. `data-s` is the state the server rendered it with, and is not written again
  as the state changes: after a click that makes `count` 6, it still says 5, so a test
  or a tool reads the page, not `data-s`. A component inside another is written with
  `data-p="{}"`, as the one around it passes its props when it runs; one made in the
  browser (`{open && <Detail/>}` after a click) carries `data-c` alone (FEEDBACK42.md,
  Q-487). Values cross as JSON, with these marked so they come back as they were: a
  decimal (`{"$dec": "12.50"}`), an int past 2^53 (`{"$int": "..."}`, a BigInt in the
  browser), an error value (`{"$quillo_err": ...}`); a whole float is written `2.0`, and
  read back as a float. NaN and infinities become `null`; dates are their text, as
  everywhere. A map whose one key starts with `$` is written `{"$": [key, value]}` and
  read back as that map, so a key a visitor chose (`set({}, k, v)`) cannot pass for one
  of these marks in another visitor's page or on the server, both ways: in a `server
  fn`'s arguments and answer, island state and props, live data, services and sessions.
  Nor can any text become a number in the JSON the browser writes (`?q=%00int:5` reached
  a `server fn` as the int 5).

  `<Counter start={5}/>` from the top of this section is sent as:

  ```html
  <quillo-island style="display:contents" data-c="Counter" data-p="{&quot;start&quot;:5}"
      data-s="{&quot;count&quot;:5}"><div>...</div></quillo-island>
  ```
- A `state` that still holds its very prop (`state items = initial`) is written in the
  page as a reference to the prop, not as a second copy of it.

  ```quillo
  component List(initial: list<str>) {
      state items = initial
      <ul>{for i in items => <li>{i}</li>}</ul>
  }
  ```

  `<List initial={["a", "b"]}/>` carries `data-p="{"initial":["a","b"]}"` and
  `data-s="{"items":{"$quillo_prop":"initial"}}"`: the list once.
- Components are resumed, not made again: the browser runs a component's code once,
  from the state the server sent, and takes the server's elements as its own (checking that
  they are the same) instead of replacing them, so images do not reload, a scrolled
  list stays where it is and what someone typed stays typed. Each component is resumed
  when it is first reached, the others when they are needed. An element whose tag or
  text differs (a script changed the page) is made again, for that component only. An
  attribute the markup sets that differs (changed, removed or added) is the browser's
  value: under `--watch` it is said (`where it wrote title="changed", the browser makes
  title="title"`) and the component is made again, and in production the value is put
  in place, so a browser extension that rewrites styles makes no component again.
  Classes a script adds, attributes the markup does not set, and a field's `value`,
  `checked` or `selected` (what a person typed or picked) are not compared. A CR in text
  is written `&#13;` and a NUL U+FFFD, on both sides, so a note typed in a `<textarea>`
  and posted with CRLF is taken up as written.
  `state` initializers do not run again in the browser: the server's values win. There
  is no virtual DOM: each state is a signal, tied to the text, attributes and classes
  that show it, and a list's rows are moved by key.

  ```quillo
  component Roll() {
      state seed = random_int(1, 1000000)       // drawn once, on the server
      <p>{seed}</p>
  }
  ```

  The browser shows the number the server drew, and keeps it: it does not draw again.
- A JavaScript error in a handler, `@mount`, a statement or an initializer is written
  to the console (`quillo: ...`), and pushed onto `window.__quilloErrors` if the page
  made that list; the component goes on. There is no error overlay on the page.

  ```quillo
  component Broken() {
      state n = 0
      <button onclick={() => { js("JSON.parse", "{"); n += 1 }}>{n}</button>
  }

  route GET / = <html>
      <head><script>window.__quilloErrors = []</script></head>
      <body><Broken/></body>
  </html>
  ```

  Each click stops at the bad JSON (so `n` stays 0), writes the error to the console, and
  adds it to `window.__quilloErrors`, where a test can read it; the button goes on
  answering.
- `ask(message, { title, ok, cancel, tone })` asks in a dialog on the page and waits for
  the answer (`true` or `false`) — a handler goes on after it, and its buttons, unless
  `ok` and `cancel` are given, are in the page's locale (see
  [Translations](#translations)); `notify(message, { title,
  tone, seconds })` shows a note that goes by itself. Both are markup with `q-` classes
  in a host the page lays out (`<body data-notices="dialog-center toasts-top-right">`)
  and styles: keyboard, focus and screen readers are taken care of (Escape, the
  backdrop and Cancel answer `false`; `tone: "danger"` starts on Cancel; a note goes
  after 4 seconds unless `seconds` says otherwise). On the server both are an error:
  they speak to someone in the browser.

  ```quillo
  component Remove(id: int) {
      state gone = false
      <button onclick={() => {
          if ask("Delete this order?", { title: "Delete", ok: "Delete", tone: "danger" }) {
              gone = true
              notify("Deleted", { tone: "success" })
          }
      }}>{gone ? "Deleted" : "Delete"}</button>
  }
  ```

  The dialog starts on Cancel, as its tone is `danger`; Delete answers `true`, and the
  note goes after 4 seconds.
- The classes and attributes Quillo sets itself, for a sheet to style. No `.qwe` file
  writes them, so a tool that drops CSS it finds no use for (PurgeCSS and the like) must
  be told to keep them:

  | Set by | Classes and attributes |
  |---|---|
  | `ask` | `q-notices` (the host), `q-backdrop`, `q-dialog`, `q-title`, `q-message`, `q-actions`, `q-cancel`, `q-ok`; `q-open` while shown |
  | `notify` | `q-notices`, `q-toasts`, `q-toast`, `q-title`, `q-message`, `q-close`; `q-open` while shown |
  | `ask`, `notify` with `tone` | `q-` and the tone, on the dialog or toast: `q-danger`, `q-success` |
  | the host's places | the page's `data-notices` on `<body>`, else `dialog-center toasts-top-right`, as classes of `q-notices` (any `dialog-...` and `toasts-...`) |
  | `highlight(code, lang)` | `hl-k` keywords, `hl-s` strings, `hl-c` comments, `hl-n` numbers and constants, `hl-f` functions called, `hl-t` types and tags, `hl-a` attributes, properties and keys |
  | `<quillo-media>` | `data-state`: `connecting`, `waiting`, `playing`, `ended`, `unsupported`; one that sends: `starting`, `sending`, `refused`, `ended`, `unsupported` |
  | a component's `style` | `data-q-<id>` on the elements it writes |

  ```css
  .q-backdrop { background: rgb(0 0 0 / 40%); }
  .q-dialog { border-radius: 8px; padding: 1.5rem; }
  .q-dialog.q-danger .q-ok { background: crimson; color: white; }
  .q-toast.q-success { border-left: 4px solid seagreen; }
  ```
- In the browser a value is what it is on the server, or an error, never another value.
  A whole float keeps its type: `type_of(2.0)` is `float`, `2.0 / 0` is `inf`, an `int`
  parameter, an index or `range` refuses `2.0` and a `float` parameter refuses `2`, a
  record's `float` field widens an int, and a field bound to a float state gives floats.
  Ints past 2^53 are exact (BigInts) in arithmetic, comparisons, `math`, `stats`, `vec`,
  ranges and every builtin that takes an int, and past 64 bits they are the server's
  `integer overflow` (`math.factorial(20)` is exact, `math.factorial(21)` overflows on
  both sides); an int is never `-0`. Numbers cross between the server and the page as
  their JSON text, so a float stays a float both ways (state, props, a `server fn`'s
  arguments and answers); in a browser whose `JSON.parse` gives no number's text (Safari
  before 18.4, Firefox before 135), one pass over the text marks them first. The page's
  own JavaScript (`js(...)`, a library) gets plain numbers. Text in a component writes a
  whole float as `2`, on both sides, so the page the server sends is the one the browser
  would make and is taken up, not made again; elsewhere the server writes `2.0`. Text has
  no exponent; `json.stringify` writes `2.0` on both sides, and turns to an exponent where
  JavaScript does (`1e+21`, `1e-7`). `math.*` may differ in the last digits, as each side
  has its own library.

  ```quillo
  component Price() {
      let whole = 2.0
      <p>{whole} {7 / 2}</p>
  }
  ```

  Both sides show `2 3.5`.
- A map keeps its keys in the order they were set, on the server and in the browser,
  keys that look like whole numbers too (a JavaScript object puts those first, in number
  order): `keys`, `values`, `for [k, v] in m`, `str`, `json.stringify`, `url_encode`,
  `merge`, spreads, `+`, `group_by` and records read them alike on both sides, so a
  map of ids, years or prices shows in one order, and a signature or a cache key made
  from `json.stringify` is the same. JSON the browser reads (`json.parse`, an
  `http` answer's `json`, a `server fn`'s answer, island state and props, live data,
  channels, streams) keeps its keys in the order written. With `--watch`, a component the
  browser had to make again says which, and what differed, in the console.

  ```quillo
  component Keys() {
      let m = { b: 1, "10": 2, a: 3 }
      <p>{join(keys(m), " ")} / {join(sort(keys(m)), " ")}</p>
  }
  ```

  Both sides write `b 10 a / 10 a b`.
- A top-level constant holding an error value (`let bad = int("x")`) is an error value
  in a component too, `is_err` true, its text hidden as the server hides a builtin's;
  it was a map there.

## Composition

Views and components nest like HTML: a capitalized tag renders another view or
component, its attributes are the props, and what is between the tags arrives as the
`children` parameter, or `null` for a tag with nothing between *(since 0.7)* (`<Card title="x"/>`),
on the server as in the browser, so `children == null` tells the two apart (FEEDBACK28.md,
Q-345), and `quillo check` warns on `str(children)`, which is `"null"` then. A component is used by its tag: called as a function (`let make = Dead`, then
`make()`, or given to `map`) it would show and never answer, so it is an error, and `quillo
check` says so.

```quillo
view Card(title: str, children) = <section.card><h2>{title}</h2>{children}</section>

component Counter(label: str, start: int = 0, on_change = null) {
    state n = start
    <p>{label}: {n} <button onclick={() => { n += 1; on_change && on_change(n) }}>+</button></p>
}

component Panel(header, children) {          // `header` is a named slot: any HTML
    state open = true
    <div>
        <div onclick={() => { open = !open }}>{header}</div>
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

- **Down:** props. **Up:** pass a function (`on_change`) and call it. A call of a
  function given as a prop is waited for, so what it gives is what the child gets: a
  callback that only sets state (`cb={() => { x = 1 }}`) gives `null`, and so does one
  that ends in a page's Promise (`cb={() => js("Promise.resolve")}`) once it is done. A
  child that must know whether its caller finished has the caller give `true` when done,
  or calls a `done` function it is given. There is no
  context or global store: state in the outermost component, passed down, is the store.
  Components placed separately in a page share nothing in the browser; they share
  through the server (`live`, channels).
- Props are passed live: a child shows its parent's new value without either running
  again. Each component keeps its own state; give components in a list a `key={...}`
  so each keeps its own (a new key makes it afresh). A key is text or a number: a list
  or a map as a key never matches, and remakes its row each time.
- A view or component takes only the parameters it declares: there is no rest
  parameter, so `id`, `data-*` or a class meant for its root element is passed as a
  parameter it declares (`<Button id="x">` is an error unless `Button` has `id`).
- A component may show itself (a tree of comments), as long as it stops somewhere.
- There are no portals and no refs: `onmount={(el) => { box = el }}` with `state box =
  null` keeps an element, and `js("document.body.appendChild", el)` moves one.
- What reads a prop follows it, as what reads state does: a place in the markup that
  shows it (`{label}`, `class={...}`), a `let` (`let doubled = count * 2`), a statement
  (`if active { every(1000, tick) }` starts and stops with it), `live(load, id)` (loads
  again when `id` changes), and a prop passed on to a child. A handler reads its value
  when it runs.
- `state x = prop` reads the prop once, when the component starts: state is the
  component's own. To follow the parent, read the prop (or a `let` of it); to start over
  when it changes, give the component a `key` that changes with it. There is no hook
  that says the prop changed under a state copied from it: the `key` starts afresh, and
  loses what was typed.
- A function called in markup with a state, `{tick(on)}`, is called again at each change.
  When its markup comes out the same shape and nothing in it follows state itself, the
  page's elements are kept and take the new attributes, text and handlers, so an input in
  it keeps its focus and what was typed. A function whose markup changes shape is made
  anew, and its input loses the focus: there, use a `view`, whose props are live.
- A plain function used as a tag (`<Row item={x}/>` where `Row` is neither a view nor a
  component) is called again as a whole when what it is given changes.
- Views are plain functions: they have no state and add nothing to the browser bundle
  unless a component uses them.
- A component placed directly in a page gets its props from the server, so they must
  be data (no functions, no HTML values), and they stay as the page was made: nothing
  above it changes them. Inside another component, props can be anything (callbacks,
  HTML, other values), and follow that component's state. A component's `state` must be
  data wherever it is. `quillo check` does not see a prop or state that is not: the page
  fails as it renders (`prop header of <D> is html, which cannot be sent to the
  browser`). `children` placed from the server travels as its HTML.

## Styling a component

A component or a view carries its own CSS. You write it with the component, and Quillo
sends it to the browser in a stylesheet of its own: nothing is written into the page's
HTML for it. The components on [quillolang.org/components](https://quillolang.org/components/)
all work this way.

**Write it at the start of the body**, before the markup:

```quillo
component Card(title: str) {
    style css`
      .title { color: darkred }
      .title:hover { text-decoration: underline }
    `
    state open = false
    <div.card @click={() => { open = !open }}><h2.title>{title}</h2>{open && <p>More about {title}</p>}</div>
}

route GET / = <html><head><title>Cards</title></head><body><Card title="Hello"/></body></html>
```

The page has a link to the sheet in its `<head>`, and each element the component writes
carries the component's attribute:

```html
<link rel="stylesheet" href="/_quillo/s/a1ce371646f1b209.css">
...
<h2 class="title" data-q-1df36318="true">Hello</h2>
```

`/_quillo/s/a1ce371646f1b209.css` holds the component's rules, made to need that
attribute, so they style the card's `.title` and no other:

```css
.title[data-q-1df36318] { color: darkred }
.title[data-q-1df36318]:hover { text-decoration: underline }
```

**Or keep the CSS in a file of its own**, beside the program, and give `style` its text:

```quillo
let card_css = read_file("card.css")     // read once, as the program starts

component Card(title: str) {
    style card_css
    <div.card><h2.title>{title}</h2></div>
}
```

Any text will do: a `.css` file read with `read_file`, or a `` css`...` `` kept in a
variable. It is scoped and sent the same way.

**Scoped, or as written.** `style` scopes the rules to the component (above). `style
global` sends them with their selectors as written, for a library whose class names are
its own already. That is what the downloadable components do: each one is a `.qwe` file
with its CSS in a variable and `style global` in its body, so copying the file brings the
styles along.

```quillo
// ui/collapsible.qwe, from the kit (shortened)
let collapsible_css = css`
.ui-collapsible { min-width: 0; color: var(--ui-fg); font: 15px/1.5 var(--ui-font); }
.ui-collapsible > summary { display: flex; align-items: center; gap: 8px; cursor: pointer; }
`

view Collapsible(title: str, children) {
    style global collapsible_css
    <details class="ui-collapsible"><summary>{title}</summary><div>{children}</div></details>
}
```

The kit's `ui_css` (its colours, fonts and the `--ui-...` variables the rules use) is the
one stylesheet a page itself puts in: `<style>{ui_css}</style>` once, in the `<head>`.

**What reaches the browser:**
- A page is sent the styles of the components and views on it, and of those they can
  show later in the browser (a `<Note>` that appears after a click), joined into one
  sheet, `/_quillo/s/<hash>.css`. A page that shows none gets no sheet.
- Its name comes from what is in it: two pages with the same components share one sheet,
  and the browser keeps it for good (`Cache-Control: public, max-age=31536000,
  immutable`). When the CSS changes, so does the name.
- The same attribute and rules on the server and in the browser, so a component the
  browser draws again keeps its style.

**What a style cannot do.** It is the same for every use of the component, so it cannot
read the component's parameters or state. For a style that changes with them, set a class
(`<div class={{ open: open }}>`) or a `style={...}` on the element. A page's own
`<style>`, or a `.css` file under `public/` linked from the page, is for what belongs to
no component: the page's layout, its fonts.

More: `:global(...)`, scoped `@media` and the other rules, a `style` for all of a file's
components, `style_with` for PostCSS or Tailwind, and what `quillo check` says of a class
nothing writes are in [HTML literals](#html-literals).

## JavaScript libraries

A component can hand an element to a library (a chart, a map, an editor) and call into
it. Name the library on a line that starts with `client` *(since 0.7)*, then call it as JavaScript does:

```quillo
client "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js" use Chart
client /assets/leaflet.js use L
client "https://cdn.jsdelivr.net/npm/canvas-confetti@1.9.3/dist/confetti.browser.min.js"
    use confetti

component Sales(points: list) {
    state chart = null
    <canvas onmount={(el) => {
        chart = Chart.new(el, { type: "line", data: points })    // new Chart(el, ...)
        () => chart.destroy()                   // when the canvas leaves the page
    }}></canvas>
    <button onclick={() => confetti({ particleCount: 120 })}>Celebrate</button>
}

component Where() {
    <div class="map" onmount={(el) => { L.map(el).setView([14.6, 121.0], 12) }}></div>
}

route GET / = <html><body><Sales points={load_points()}/></body></html>
```

- `client <script> use Name`: `client` says this runs in the browser, and the path says
  where it comes from: an address (a CDN) or a file of your own, ending in `.js` or
  `.mjs`. `Name` is what the script defines on the page (`Chart`, `L`, `confetti`,
  `dayjs`): the name the library's own documentation uses, in its case. Several from one
  script: `client /assets/leaflet.js use L, LeafletDraw`; another name for one:
  `client "..." use Chart as ChartJS`. Quotes are optional, as for every path; put an
  address in them, so editors do not read its `//` as a comment.
- A long address may leave its `use` to the next line, as `confetti` does above, so the
  names stay at the left. `quillo fix` breaks a line this way when it would be long.
- `client`, `preload`, `preconnect` and `module` are still names everywhere else:
  `client / 2` is a division. It is the `use` after a file that makes the line an import.
- Each file that calls a library says so, as for any name: another file that calls
  `Chart` writes the same line. The page still loads the script once. A file taken whole
  does not pass its browser names on: one used through it is warned of, with the
  `client` line to write. One name from two scripts (or a script and a module's export)
  is an error, in `quillo check` and as the program starts, since the page would have
  only the last: take one `as` another name.
- `Name.new(args)` makes an object of a class (`new Chart(el, config)`); `Name(args)`
  calls a function (`confetti()`); `Name.fn(args)` calls one of its functions
  (`L.map(el)`, `dayjs.extend(...)`); `Name.field` reads a value (`Chart.version`).
  What they give is the library's object, whose methods and fields are used as below
  (`chart.update()`, `map.setView(...)`).
- The page loads the script only where a component that uses the library is on it (or
  one it can come to show), once, in the order the program names its libraries, before
  the components' own code. A page without such a component loads nothing more. No
  `<script>` tag is written; one you write still works, as below.
- A path (`/assets/leaflet.js`, `assets/leaflet.js`) is from the program's own address,
  under its base path; serve it as any other file (`public/`, or a route with `file()`).
  An address (`https://...`, `http://...`, `//...`, in any case) is loaded as written:
  pin its version. Either may carry a query or a fragment (`/assets/app.js?v=2`). Any
  other kind of address (`ftp:`, `file:`, `data:`) is refused, and so is one with a user
  name or password in it: browsers do not load those, and the page would show it.
- A page loads a library late: when the browser is idle, or at the first touch of a
  component that only answers clicks. To get ready for it sooner, start the line with
  `preload` or `preconnect` in place of `client` *(since 0.7)*:

  ```quillo
  preload "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js" use Chart
  preconnect "https://unpkg.com/leaflet@1.9.4/dist/leaflet.js" use L
  ```

  `preload` has the page's `<head>` fetch the script (`<link rel="preload"
  as="script">`), which suits a library the page uses as soon as it opens, such as a
  chart in its `@mount`. `preconnect` only opens the connection to the script's
  host (`<link rel="preconnect">`), which costs little and suits one a click may need;
  it takes an address, since a path is on the page's own connection. Take one or the
  other: a preload opens the connection itself. Each goes only in
  pages that can load the library, as the script does. A preload is fetched whether or
  not the library ends up used, so keep it for one or two scripts a page needs at once.
- The names are known to `quillo check` and the editor. They are the browser's: on the
  server, while a component renders, `Chart.version` and `Chart.new(...)` give null, as
  `js(...)` does, so call them in `@mount` and event handlers.
- Mistakes say what to do: `Chart(el)` for a class stops with "Chart is a class: make
  one with Chart.new(...)"; a name in the wrong case gives an error value,
  `js_new: chart is not defined on this page (did you mean Chart?)`; so does a script
  that did not load.
- Without `module`, `client` takes a library that sets a global (a "UMD" or "browser"
  build: `.umd.js`, `.min.js`, `.browser.js`). An ES module is below; an npm package
  needs a CDN that serves it as one.

An ES module (`import`/`export`) gives names by exporting them, and `module` after the
first word takes them. Nothing goes on `window`: what the module exports is kept in Quillo's runtime, where
the page's scripts cannot reach or replace it, and is called as any library is:

```quillo
client module "https://esm.sh/chart.js@4.4.1" use Chart, registerables
preload module "https://esm.sh/chart.js@4.4.1" use Chart as ChartJS
client module "https://esm.sh/dayjs@1.11.13" use default as dayjs
client module "https://esm.sh/three@0.170.0" use * as THREE
client /assets/helper.mjs use helper

component Scene() {
    <canvas onmount={(el) => {
        let scene = THREE.Scene.new()
        scene.name = dayjs().format("YYYY")
    }}></canvas>
}
```

- Each name is an export, and `as` gives it another name. `default as Name` takes the
  default export, and `* as Name` the whole module (`THREE.Scene.new()`,
  `Name.default(...)`).
- A file ending in `.mjs` is a module without saying so. A `.js` file or an address may be
  either, so `module` says which.
- With `preload`, the page has `<link rel="modulepreload">`, and with `preconnect` a
  connection made for CORS. A module at another address needs its server to send
  `access-control-allow-origin`, as the public CDNs do, and its own imports must be
  addresses too: a CDN such as esm.sh or jsDelivr's `/+esm` rewrites an npm package's.
- A page's components start once its modules are in, so a slow module holds them back,
  and what is done on the page meanwhile is done when they start.
- `quillo check` cannot read a module at an address, so a wrong name is known in the
  browser: an error value where it is used, `gone: /x.js has no export \`gone\``, or
  `/x.js did not load: ...`, and the rest of the page runs. Calling a class without `.new`
  and the rest say what they do for any library.
- `js("Chart")` finds the module's `Chart` too, ahead of a `Chart` on the page. Anything
  else `js` and `js_new` find from the page's globals.

Without `use`, a page's own `<script>` tags work too, and `js` and `js_new` reach what
they define by name:

```quillo
component Sales(points: list) {
    state chart = null
    <canvas onmount={(el) => { chart = js_new("Chart", el, { type: "line", data: points }) }}></canvas>
}

route GET / = <html><head><script src="/assets/chart.js"></script></head>
                    <body><Sales points={load_points()}/></body></html>
```

- `@mount={(el) => ...}` runs once the element is on the page, once in its life (on a
  server's element too, when the component is resumed). A function it returns runs when
  the element leaves the page, or its component does. That is the only lifecycle hook:
  what runs when state changes is a statement that reads it.
- `autofocus` on markup made later (a dialog's field, a row added) is focused once it is
  on the page, the first of several, unless someone is typing in another field; the
  browser itself focuses only what the page had as it loaded.
- A Quillo function handed to the page's JavaScript (an event listener, a callback) is
  called with as many arguments as it declares, so `marker.on("click", () => ...)`
  works, and `off` finds what `on` was given.
- `js("path.to.fn", args...)` calls a function of the page's scripts, found by its dotted
  name from the page's globals (`js("L.map", el)`, `js("analytics.track", "signup")`),
  and gives its result; `js_new("Chart", ...)` makes an object of a class. A library's
  name from `use` is the same call written as JavaScript reads, with the same checks.
  The name is written in the code: one from data could reach anything on the page, so a variable is
  an error when the program loads, as are `eval`, `Function` and the like.
- A map reaches the page's JavaScript as the object it is, but one holding a key that is
  an array index (`"0"` to `"4294967294"`) out of JavaScript's order: that one reaches it
  as a plain copy, its keys in JavaScript's order, so `URLSearchParams`, `Headers` and
  deep-equality libraries take it, and what page code changes in that copy does not
  show in Quillo. What a program's function called by page code returns is handed over
  the same way.
- Objects from the page (elements, a library's objects) have their properties and
  methods as fields: `el.value`, `chart.update()`; and their fields can be set:
  `el.value = ""`, `el.scrollTop += 100` (only theirs: a map's fields are made anew,
  with `set` or `{ ...m, name: v }`). A plain `fn` may set them too, on a parameter of
  no declared type that a component hands it (`fn paint(ctx) { ctx.fillStyle = "red" }`,
  called as `paint(el.getContext("2d"))`), so drawing code is shared by components;
  run on the server, it is an error there. `innerHTML`, `outerHTML`, `srcdoc` and `on...` are
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
  `@mount`, in a `state` initializer, and in the component's `let` functions that its
  markup does not call (`let locate = () => { ... }`, then `onclick={locate}`): `let cam =
  js("navigator.mediaDevices.getUserMedia", { video: true })` is the stream, or an
  error value if it was refused. The same holds for a method of what the page gave
  (`img.decode()`, `audio.play()`) and for a function given as a prop, so there is one
  rule: a call of the page's that gives a Promise is waited for where code can wait.
  Elsewhere (a function `map` calls, a function the markup calls, a statement that runs
  again, a top-level `fn`), with `.then`, `.catch` or `.finally` after it, and for what
  `js_new` makes (`js_new("Promise", ...)`, kept to be used later), it stays a Promise:
  `js("load").then((x) => { data = x })`. `type_of` calls one `promise`. A function of the
  program's that the page calls with `undefined`, as `.then` does for a Promise of
  nothing, is given `null` (FEEDBACK28.md, Q-336). To wait for what `js_new` makes, and
  catch its failure, wrap it: `js("Promise.resolve", js_new("Promise", ...))`.
- A handler pauses only where something waits. A block in it (`match`, `if`, `for`) is
  made to wait only when something inside it waits; a call of a module's function
  (`date.`, `re.`, `money.`, `math.`, `stats.`, `vec.`) is never waited for; and a call of
  a function compiled here (the component's own, or a top-level `fn`) that is known to
  give no Promise is not waited for, so it can be given to `map`, `filter` and `reduce`.
  A state started from a module's function is set at once, in a component made later
  too. A component's own function named like a method of the page's objects is not
  waited for outside waiting code, so a Promise's executor that throws rejects it.
- A `let` set from the page (`let w = js("window.innerWidth")`, `js_new(...)`) is null
  while the server renders, so a component that shows it in its markup is made again in
  the browser; `quillo check` warns of it: write `state w = js(...)`, which runs again
  in the browser (below).
- `js_has("navigator.vibrate")` *(since 0.7)* says whether the page has a feature without calling it,
  as `js(...)` would; `false` on the server (Q-341). A field's `selectionStart` counts
  UTF-16 units, as JavaScript does, and `slice`, `len` and `s[i]` count characters:
  `cursor_index(text, el.selectionStart)` *(since 0.7)* turns the one into the other (Q-344).
- A `state` initializer that reads the page (`state wide = js("window.innerWidth") > 800`)
  gives null while the server renders the component, and runs again in the browser
  once the component is on the page, which sets the state from what it gives. An
  image's `@load` that came before the code is sent to the image once the code runs
  (its `e.target` is the image), and runs once. A frame or a player with `src={...}` is
  asked for once, unless it has a listener other than a click: that one starts over when
  the code runs, so that it hears its `@load` (FEEDBACK61.md, Q-893).
- A name the page does not have (a library's `<script>` left out) gives an error value,
  `js: L.map is not defined on this page`, so a component can say so instead of stopping.
- On the server, `@mount` does nothing and `js` gives null.
- `js` names what it calls in the code (`js("Chart", ...)`, not `js(name, ...)`), which
  `quillo check` says as loading the program does.
- There are no npm packages, `import`s or TypeScript: a library is a script the page
  loads (named with `use`, or a `<script>` tag), and component code reaches it by name. `js("fetch", url)` works, but `http.*`
  is the way.

## Single-page apps

`url()` gives `{ path, query, hash }`: of the request on the server, and of the address
bar in the browser, where what reads it follows it when it changes. A browser never
sends the hash, so on the server it is `""`, and a component that shows something by
the hash is made again in the browser. A query key given twice keeps the last. Once a
component has called it, clicks on same-site links inside components change the page
without reloading; Back and Forward work. `navigate("/path")` does the same from code;
on a page where no component has called `url()`, it loads that page as a link would.
(Links with a modifier key, `target`, `download` or `data-reload` work as usual.) A link
or `navigate("#spot")` to a spot on the same page goes there as the browser does:
scrolled into view, and CSS `:target` set. `navigate("/page#spot")` goes to the spot
once the new page shows it, and stays at the top if it never does. A `javascript:` or
`vbscript:` URL, however it is written (`  JaVaScRiPt:`), is an error, in the browser
and on the server, as it is for `redirect`: `navigate(next)` after a sign-in, with
`?next=javascript:...`, would have run as the page *(since 0.7)*.

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

And the pages it shows, with the `server fn`s that give them their data:

```quillo
let products = [{ id: 1, name: "Kettle", price: 30 }, { id: 2, name: "Teapot", price: 25 }]

server fn list_products() -> list = products | map((p) => { id: p.id, name: p.name })
server fn load_product(id: int) -> map? = products | find((p) => p.id == id)

view Home() = <h1>Welcome</h1>
view NotFound(path: str) = <h1>No page at {path}</h1>

component ProductList() {
    state items = list_products()
    {items == null
        ? <p>Loading…</p>
        : <ul>{for p in items => <li key={p.id}><a href={`/products/${p.id}`}>{p.name}</a></li>}</ul>}
}

component ProductPage(id: int, on_add) {
    state product = load_product(id)
    {product == null
        ? <p>Loading…</p>
        : <div>
              <h1>{product.name}</h1>
              <button onclick={() => on_add(product)}>Add to cart, {product.price}</button>
          </div>}
}
```

Opened at `/products/2`, the server sends the Teapot's page whole. Clicking Products, then
Kettle, changes the address and the page without loading one, each page showing
`Loading…` until its call answers; Add to cart makes the cart 1, and Back keeps it.

Pages load their data with `server fn`s in `state` initializers: on the server while
rendering, in the browser when you navigate to them, where the state is `null` until
the answer comes (show something for `null`). Give a page component a `key` from
its URL (`key={id}`) so a new product starts with fresh state. `navigate` scrolls to
the top; links outside the program's mount, and under `/_quillo/`, load as usual. A
full page load starts every state afresh: keep what must survive one on the server, or
in `localStorage` through `js`. See
[spa.qwe](https://github.com/quillolang/examples/blob/main/spa.qwe), which also answers 404 on the server for unknown paths.

## Live data

`live(load, ...args)` makes component state follow the server: `load` (a `server fn`)
runs again, and what shows the state follows it, whenever something it read changes, for
anyone, not only when this page acts.

```quillo
db.open("notes.db")
db.exec("create table if not exists notes (id integer primary key, text text not null)")

server fn load_notes() -> list = db.query("select id, text from notes order by id")

server fn add_note(text: str) -> map = db.exec("insert into notes (text) values (?)", text)

component Notes() {
    state notes = live(load_notes)  // every open page sees new notes
    state draft = ""
    <div>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
        <form onsubmit={() => { add_note(draft); draft = "" }}>
            <input value={draft} placeholder="A note"/>
            <button>Add</button>
        </form>
    </div>
}

route GET / = <html><body><Notes/></body></html>
```

The form calls `add_note`, and nothing sets `notes` by hand: the insert writes `notes`, so
every page whose loader read it runs `load_notes` again, this one included, and the list
grows on each. A note added from anywhere else (another tab, another visitor, a
`POST /_quillo/rpc/add_note`, a route) shows the same way.

- `live` takes a `server fn` by its name (not a lambda, not a variable holding one),
  and in a component only as a `state` initializer: `state notes = live(load_notes)`.
  The checker refuses other forms. In a route or a view it calls the function once and
  follows nothing: live data is a component's.
- Its arguments are followed too: `live(load, id)` runs `load` again with the new `id`
  when the prop or state it came from changes, and an answer still on its way for the
  old one is dropped. They are sent as JSON, so they must be data. Until the new answer
  comes, the state keeps the old one: `waiting(grid)` is true meanwhile (and while a
  page the browser made waits for its first), so a grid titled December can say that it
  still shows November: `<table class={waiting(grid) ? "stale" : ""}>`. On the server it
  is false, since the server renders with the answer.
- What a loader *reads* is recorded: the tables in its queries (`from notes`,
  `join users`, in subqueries too; a schema's prefix is dropped, so `a.notes` and
  `b.notes` are one table) and the global variables it uses (`todos`; a global holding
  a function is not followed). What any code *writes* is announced: `insert`, `update`,
  `delete`, `truncate`, `create`, `alter` and `drop table` through `db.*`
  (`insert ... select` writes one table and reads another), and assignments to globals.
  This works the same with SQLite, PostgreSQL and MySQL, and with data kept in variables.
  Not seen: what a trigger, a stored procedure or a `copy` writes, writes to the tables
  behind a view a loader reads (the view's name is what is recorded), and MySQL's
  `delete t from t join u` form. Publish a topic for those.
- Topics have names: a table is `db:notes` (`db@55c8e71313e4:notes` on a connection from
  `db.connect`, by a label made from where the database is, the same for every spelling
  of it: see [Several databases](#standard-library-several-databases)), a global `var:todos`, and `publish("thread:7")` what it
  says. With tenants, every topic but a global's belongs to the tenant, so a `publish`
  wakes only the same tenant's pages; in a mounted program they are the mount's.
  `watch` and `publish` take several topics at once; `unwatched(f)` calls `f` with
  nothing. A topic is 1 to 256 bytes of any characters, a comma too; an empty or longer
  one stops `watch` and `publish` with an error, where the page never heard of it.
- A topic ending in `:*` is a wildcard: `publish("room:*")` wakes every page following a
  topic under it (`room:7`, `room:lobby`), and `publish("db:*")` every table's. So a
  topic built from a visitor's text must not end in `:*`: write `` `room:${id}` `` with
  an `id` the program checked (an int), not one taken as it came.
- A loader that gives an error value makes the state that error value, sent whole; one
  that fails with a runtime error, or `reply`s a status that is not a success, leaves
  the page's state as it was (the console says so), and the next change tries again.
- A page may follow at most 256 topics; past that, the rest hear nothing. A list too
  long for an address goes after it, so long topics are heard too.
- Inside `db.transaction`, a table's announcement waits for the commit, and is dropped
  on rollback. A global's waits for the outermost transaction to end, whatever its
  outcome, since a global keeps its value through a rollback: pages are told it, where
  they kept showing the old one.
- `publish("topic")` announces anything else, and `watch("topic")` inside a loader
  subscribes to it.
- What a loader reads includes what the functions it calls read: a `me()` that looks up
  the `sessions` table makes every loader calling it run again when anyone signs in.
  `unwatched(() => me())` calls it without following what it reads, though what it read
  still makes the run the viewer's own. Keep what changes often but is not shown (view
  counts, last-seen times) in a table of its own, so writing it wakes no page.
- A field of a global map, read as `world.scores`, is a topic of its own
  (`var:world.scores`): a loader that shows it runs again when that field changes, not on
  every write to the map, even 20 times a second from a game's loop. Assigning the map
  announces each top-level field whose value changed (what `{ ...world, tick: t }` keeps
  is unchanged), and the map itself, for what reads it whole (`world`, `keys(world)`,
  `world.len`, a field it lacks). Deeper fields are not followed apart: `world.scores.ana`
  follows `scores`.
- A write to a table wakes every loader that read the table, whatever rows it read:
  a reply in one thread runs `load_posts` again on every open thread page. To follow
  one thread only, read the table `unwatched` and watch a topic of its own, which the
  code that writes publishes:

  ```quillo
  server fn load_posts(thread: int) -> list {
      watch(`thread:${thread}`)
      unwatched(() => db.query("select * from posts where thread = ?", thread))
  }

  server fn reply_to(thread: int, text: str) {
      db.exec("insert into posts (thread, text) values (?, ?)", thread, text)
      publish(`thread:${thread}`)
  }

  component Thread(id: int) {
      state posts = live(load_posts, id)
      state draft = ""
      <section>
          {for p in posts => <p key={p.id}>{p.text}</p>}
          <form onsubmit={() => { reply_to(id, draft); draft = "" }}>
              <textarea value={draft}></textarea>
              <button>Reply</button>
          </form>
      </section>
  }

  route GET /threads/:id(int) = <html><body><Thread id={id}/></body></html>
  ```

  A reply on `/threads/7` shows on every page open on thread 7; pages on thread 8 do not
  run their loader for it.
- A loader runs again as a call from the page, with the page's cookies, so `session()`
  inside it is the viewer's session. So what should happen when new data arrives for a
  viewer (marking posts or messages read) is done in the loader: a component's code
  cannot call a `server fn` when state changes, but the loader is that call. A page
  whose loader writes is not prefetched (see [Prefetching](#routes-prefetching)); it is made
  when the link is followed. What a loader writes does not wake that loader again on its
  page, since its answer has it already (pages showing it elsewhere are woken), and a
  statement that changed no row (`update messages set seen = 1 where seen = 0`, once
  everything is seen) is not announced at all: a loader that marks what it shows as
  read runs once for each change, not for ever.
- The server renders the first value; the page keeps one WebSocket open
  (`/_quillo/live`, served by its own thread), down which the server streams each
  change to its topics (the names, with a sequence number; a ping every 20 s when
  nothing changed), and calls the loader again, so a page gets the latest value, not
  every change in between: changes made while a page was disconnected are caught up on
  when it comes back. A WebSocket, because browsers
  allow only six HTTP/1.1 connections to a host across all tabs: requests left
  waiting would hold them, and server calls would queue behind them. Where a
  WebSocket cannot open, the page asks the same URL as a plain request instead,
  answered at the first change or after 25 s, and asked again at once. A connection
  that drops is opened again after a second or so, spread out, and after a restart pages
  ask for their data over a second, so that they do not all come at once; when the
  server cannot be reached, the page waits longer each time, up to half a minute. A tab
  in the background keeps its connection and runs its loaders as a visible one does. A
  page on another site cannot open it (403).
- A server keeps at most 4096 such connections open (live data, [`--watch`](#options-of-quillo-run) reloads,
  channels over a WebSocket), and at most 256 for one client: an IPv4 address, or an
  IPv6 /64 network. Channels over WebTransport are not counted. Past that, a page is
  answered `503`, keeps what it shows, says so once in the console, and asks again when
  told. The server says so once in its log, and `quillo_live_refused_total` counts them.
- **Behind a proxy, name it.** Every visitor is then the proxy's address, so without
  this only 256 pages get updates. Start the program with [`--trust-proxy`](#options-of-quillo-run), naming the
  proxy, `quillo run main.qwe --trust-proxy 127.0.0.1` (addresses or networks,
  comma-separated): it believes `X-Forwarded-For` on connections from those: the client is
  the nearest address in it that is not one of them, for this limit, for `request.ip` and
  everywhere else. From any other address the header is ignored, as a client can write
  it. When one address really is many visitors, as an office is, [`--live-per-client 1000`](#options-of-quillo-run)
  raises the limit.
- Only changes made through Quillo are seen: a row changed by another program
  (`psql`, a script) does not wake pages.
- One change wakes the pages showing it, and no others, and they all ask at once. The
  first change goes to a page at once; what comes within a tenth of a second or so after
  it goes as one, so a burst of writes is a few refreshes, not one each.
  `live({ every: "2s" }, load)` asks for a gap of its own between a page's refreshes,
  as `500`, `"500ms"`, `"2s"` or `"1m"`, written here as it is.
- A loader's run is shared by the pages that ask, within two seconds, for the same
  function with the same arguments, `Host` and tenant while nothing it read has changed:
  fifty open boards run `load_board` once, and that goes for a page's first load too.
  Calls that come while a run is under way wait for it. A run that read the visitor
  (`session()`, cookies, `request`, the locale), wherever in the run it read it, or did
  anything but read (wrote, called a service or plugin, `crypto.token`, `uuid`, `random`,
  set a cookie, took a file, answered with `respond`), is shared with nobody, not even
  the visitor's other tabs, and nothing waits for it. Randomness made in SQL (`select
  hex(randomblob(8))`) is a read, so such a run is shared: make tokens with
  `crypto.token`. `nextval` or a writing function behind a view is not seen either: put
  it in the query.
- **What a thousand pages cost** depends on whether the loader is shared. A loader for
  everyone runs once a change however many pages show it. One that reads the viewer runs
  once for each page, on every change to what it read, so with many pages and a busy
  table give each viewer a topic of their own: `watch("user:7")` and an `unwatched` query,
  published by whoever writes. What is in the table does not matter, what the loader
  returns does: each run makes that value's JSON, so return what the page shows (`limit`,
  or `live(load_page, page)`), not the table. A loader giving over 4 MB is said in the
  log. `quillo_live_runs_total` tells runs made from runs shared.
- A page is sent what changed in the value, not the whole value again: the server keeps
  the values pages have (up to 64 MB with the patches, no value over 16 MB; past that,
  those few pages share go first), and answers a page with a patch to its own when the
  patch is the shorter. A patch does not go inside text, so one letter changed in a long
  field sends the field. Lists of rows are matched by their `id` (a number or text; the first of two
  alike counts), rows without one by place, then by content, and what is inside a row
  is patched the same way, so a card moved or renamed on a board of 3,000 costs that
  card. A map whose keys changed order, and a decimal, go whole. The patch applies to
  what the server last sent, so changes the page made to the state meanwhile (a card
  shown moved before the server says so) are replaced as before. An error value is
  never patched.

### Several servers

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
  `/broadcast`, which passes them to every member (`POST /_quillo/announce`, at
  most 64 KiB). The pool's name is the file's (`shop`), or [`--service`](#options-of-quillo-run). [`--register`](#options-of-quillo-run)
  needs a [`--port`](#options-of-quillo-run) written out, and cannot be combined with [`--instances`](#options-of-quillo-run) (nor can
  [`--watch`](#options-of-quillo-run)): run one instance per server, or several servers.
- limer's admin secret is required: `--register` does not start without
  `LIMER_ADMIN_SECRET`. Each announcement is signed (HMAC-SHA256, with a key made
  from the secret) together with the time and a random nonce; one that is not
  signed so, more than a minute old, or seen before is refused. A server takes
  only the topics it already knows (changed there, or shown on its pages), at most
  256 a message.
- What is shared is the database: tables (`db:` topics) and `publish` topics
  are announced. Data kept in variables belongs to each server and is not. Nor are
  channels and media streams: a broadcast reaches the pages on this server, and a
  camera's stream is watched through the server it comes to (see [Channels](#channels)).
- Sequence numbers are per server, and each carries its server's id. A page
  whose live connection lands on a different server (after a restart, or with
  round-robin balancing) loads its live data again once, rather than missing a
  change. `lb_policy sticky_cookie` keeps a browser on one server and avoids
  even that.
- Announcements are delivered once, without retries: a server that cannot be
  reached when one is sent misses it, and its pages show that change only when
  they next load the data (the next change to it, or a reload).
- Sessions need the same `QUILLO_SECRET` on each server.

## Server functions

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
    <form onsubmit={() => {
        let r = add_note(draft)                    // a request to the server
        if is_err(r) { problem = r.error } else { notes = r; draft = ""; problem = null }
    }}>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
        <input value={draft}/>
        <button>Add</button>
        {problem && <p class="problem">{problem}</p>}
    </form>
}

route GET / with db = <html><body><Notes initial={db.query("select * from notes")}/></body></html>
```

The route gives the component the notes there are; pressing Add with the box empty shows
`empty note` under it, and with text the list shows the new note and the box empties.

In component code the call is awaited automatically. A call that fails (the
connection drops, the server errors) returns an error value, like `http.*` on the
server, so `let more = load_more() ?? []` carries on and can try again later. It works in event handlers and
in `state` initializers (`state todos = load_todos()`: the server runs it while rendering,
so the page arrives with the data). Calling one while rendering is an error.

The error value's text says what happened: ``server fn `x` could not be reached:
...`` when there was no answer, ``server fn `x` failed (403): ...`` when the server
refused it (400 with the argument that did not fit, 403 from another site, 404 for no
such function, 500 `internal error in x`, whose real message is on the server's
stderr), and ``server fn `x` answered something that is not its answer (text/html, 1234
characters)`` when a 2xx came back that the server did not send, or cut short: a sign-in page a
redirect led to, a proxy's page, an empty body. Quillo's server marks what it sends with
`x-quillo-answer`, so `respond` and `reply` still give `null` (a proxy that drops the mark turns those
into this error; ordinary answers go through). A `redirect` from the function itself is followed
by the browser, and gives this error too. A function that returns `err(v)` gives an error value with `v`. A `reply 401
"Log in first"` inside it gives only the status (``failed (401)``), since its body is
not JSON: return `err("Log in first")`, or `reply 401 { error: "Log in first" }`, for the
page to show the words. Nothing happens by itself on a 401 or a 500 (no redirect, no
retry, except for [idempotent](#idempotent-requests) calls); there is no loading flag
either: set a state of your own around the call.

**The latest call wins.** A handler can run again before its call has answered (a
letter typed in a search box, a second click), and answers come back in any order. An
older call's answer never replaces a newer value: every set of a state claims it (a
plain `shown = "x"`, a field bound with `bind:`, a set from a call). A set that carries an
answer the function waited for gives way when another run has set or claimed that state
since. It carries the answer if its value holds the call or a variable bound from it, or
if the answer decides it: it sits under an `if`, `match`, `while` or `for` on the answer
(or `&&`, `||` or `??` used as one), or after a guard that leaves on it (`return`, `?`). A
run is one handler, effect, timer or
frame callback, with the functions it calls; a handler fired by `el.click()` and each
`parallel` function are runs of their own. So
`oninput={(e) => { shown = search(e.value) }}` ends on the answer for what was typed
last, however late the first one comes.

Every other set is made: a plain one, like `saving = false` after the wait, a fold, and a
set keeping a state in step with one folded earlier in that same branch. A set that reads
the state it sets is a fold (`n += 1`, `busy -= 1`, `xs = xs + [x]`, `s = s | push(x)`,
`s = f(s)`), made on what the state holds then: three quick likes that each call `save()`
and then `likes += 1` count three, and two uploads that each do `busy += 1`, wait, and
`busy -= 1` end at 0.

In a fold with a wait in it, every part runs where it is written, and only the read of the
state moves after the waits: `n += count()` adds to what `n` holds when the answer comes,
and `n = n + check()? + charge(1)` makes no charge if `check()` stops. That is so for `+=`
and `-=`, and for `=` forms headed by the state (`n + f()`, a template, `...state` in a
list or map, `set` and `push`). Other shapes read the state first.

Only a set with an answer is ever dropped. Plain sets and folds are always made; from a run
that lost the state they take nothing, so the newest answer still lands. So clear a state
before the wait, not after it: in `let r = search(q); wipe(); shown = r`, an older run's
`wipe()` can blank what the newest answer showed. Write the set in the function that waited:
an answer handed to a helper as an argument counts as plain there.

In a function that waits, a hole of markup may wait too: `let el = <b>{load()}</b>`. It is
worked out first, before the markup is made and before the holes around it, once, and does
not follow what it read.

**Slow data: `defer`.** A `state` initializer runs while the server renders, so the
page waits for its slowest one. `defer` lets a slow one follow the page instead:

```quillo
component Product(id: int) {
    state product = load_product(id)                  // in the first bytes
    state reviews = defer(load_reviews(id))           // streamed when it is ready
    <div>
        <h1>{product.name}</h1>
        {reviews == null ? <p>Loading reviews…</p> : <ReviewList reviews={reviews}/>}
    </div>
}
```

- The page is sent at once with `reviews` null: what the component shows for null is
  there (a skeleton, "Loading…"). The rest of the answer comes when `load_reviews`
  does: the component rendered again, its other states as they were (`load_product`
  is not called twice) and `reviews` worked out, which a line of script swaps in. The
  answer is one response, so search engines read the whole page, reviews included.
- The browser takes the component up once its part has come, not before: until then
  it is HTML (links work, nothing else does). Its part comes as soon as it is ready,
  and before the page counts as loaded.
- Only a component placed in the page defers. Inside another component, and in the
  browser (a page made there after `navigate`), `defer(x)` is `x`: there a state that
  calls the server is null until it answers anyway. Components inside a deferring one
  render again with it, their initializers too.
- Several deferred components follow in the order they are on the page, each as soon
  as the ones before it are sent. A deferred part cannot set cookies or the session
  (the answer's head is gone): do that in the route or a server fn.
- Without a connection to stream to (a test calling `handle`), for a prefetch, or for
  a mounted program, the parts are worked out first and the page is sent whole. A
  runtime error in one is logged, and its placeholder stays.

For data a page should not wait for at all (another site's API that may take seconds),
render a placeholder and call the `server fn` from `@mount`.

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
transaction they run one after another, since the others' SQL would not be part of it.
A function that fails stops the others at their next step (a call, a loop's round, a
builtin, a `sleep`, which ends at once), nested `parallel` too; what they did before it
stays done, and the call fails with the failure that happened. Error values are results
like any other and stop nothing. Inside the functions, `set_cookie`, `set_session`,
`end_session`, `session()`, `publish` and a replica's read-your-writes work as they do
outside, and what each sets goes into the answer in the functions' order, as if they had
run one after another; one that reads who the visitor is makes the answer private to them.

Each call is a `POST /_quillo/rpc/<name>` with a JSON array of arguments (or, when an
argument is a file, multipart: see [Uploads](#uploads)), checked against
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

From hand-written JavaScript: `await quillo.server.add_note("text")`, which resolves to
an object with `.error` rather than rejecting. `window.quillo` (`{ server, components,
rpc }`) is there once a program with `server fn`s has had its bundle loaded, which is
when a component is reached; a script that runs as the page loads includes `<script
src="/_quillo/quillo.js"></script>` first. A component cannot be mounted from a script:
components start from the server's HTML, or inside another component.

A call that must not happen twice (a payment, a transfer) is wrapped in
[`idempotent`](#idempotent-requests), in the `server fn` or a function it calls: the page
then sends a key with each call, and sends the call again when its answer does not come
back, and the server runs it once. Two clicks are one call: while a call runs, the same
call with the same arguments shares its answer and sends nothing, and a call that got no
answer keeps its key, so sending it again is safe. Any answer ends it, so the next
payment, even of the same amount, is a new one. A call with other arguments is another
call.

### Tools for AI clients

`fn mcp name(...)` is a tool: a function an AI client (Claude, an editor's assistant) can call
through the program's MCP door, with its `///` as the description the model reads and its
types as the schema. `server fn mcp name(...)` is a server fn the program's pages call too. To
the program both are plain functions: a call from Quillo code is a plain call, with no checks
and no limits.

```quillo
/// The orders placed today, newest first.
fn mcp orders_today(limit: int = 20) -> list with db {
    db.query("select id, total from orders order by id desc limit ?", limit)
}
```

- The tag is `mcp` after `fn` (or `server fn`) and before the name, and `mcp` stays an
  ordinary name everywhere else: `fn mcp(x)` is a function named `mcp`, and `fn mcp mcp()` a
  tool named `mcp`.
- A tool is an address: `private fn mcp` is an error, as `private server fn` is. It is
  declared at the top level, and a `view` or a `component` is not a tool.
- Its name is the tool's: `A-Z a-z 0-9 _ - .`, 1 to 128 characters. A tool's name is the
  program's in every file, so two files cannot both declare `total`.
- A plain `fn mcp` is not a page's to call: `/_quillo/rpc/<name>` answers 404, as for any
  plain function.
- `quillo check` warns of a tool with no `///`, and of a parameter with no type (the model
  guesses). A tool whose body uses an effect it does not declare is warned too, the warning
  naming what it uses (`with time`): without a `with`, a client is told it may change anything
  and reach anywhere. A tool that uses none is fine as it is. A parameter that cannot come as JSON (`fn`, `html`, `response`) is an
  error, and so is an answer that can only be a page (`redirect`, `html`, `respond`): a tool
  answers with data. One that only may be a page is a warning.
- `service mcp /billing = "billing.qwe"` serves billing as `service /billing` does, and its
  tools will be the program's, named `billing.<name>`. The named form, `service mcp crm =
  "https://..."`, for calling another MCP server, is refused for now.
- The API document marks a `server fn mcp` with `x-quillo-mcp: true`.
- What a client is told: each tool's arguments and result as JSON Schema 2020-12, the dialect
  MCP requires (`str?` is `["string", "null"]`, a record once under `$defs` by a local `$ref`,
  `int | str` a list of types, `dec` and money's amount a text with a `pattern`). The
  arguments and the records they hold refuse other fields (`additionalProperties: false`); a
  result says no such thing, since the server does not check it. A typed result is always an
  object: one that is not (`-> int`, `-> list`) is held as `result`, in the schema and in what
  the client gets (`{ "result": 3 }`). A tool with no `->` type has no result schema, and its
  answer goes as text alone.
- Two hints. `readOnlyHint` is true when Quillo can see the whole of what the tool may run (no
  function held in a variable or a parameter, no `js`, `run_script` or native plugin), it
  writes nothing (no `db` write, no builtin that changes something, such as a session, a
  cookie, `publish` or `media.close`, no file, no top-level `let` it changes, none in the
  defaults of a record it takes, gives or makes, and no function a statement calls that its
  database does not ship, as `select sync()`) and its `with` has none of `fs`, `net` and
  `http`. `openWorldHint` is false when it reaches no other site: no `net`, no `db.connect` or
  `db.shards`, no such function, `http` only to the program's own services whose code is in the
  program and stays inside, and all of it seen. Otherwise each is MCP's cautious default. A
  client may skip asking the person before a read-only tool, so Quillo says so only when it is
  true.
- "Only reads" is worked out, with no word to write: every `db.query` and `db.one` the tool
  may reach has a statement written in the program that starts with `select`, only reads, and
  calls only the functions its database ships (aggregates, text, numbers, dates, JSON,
  conditionals). A `with ... insert`, `insert ... returning`, `select ... for update`, a call
  of any other function (`dblink`, `sync()`) or a statement built at run time counts as a
  write. A read-only tool that may reach the database (its `with` names `db`, or it has none
  and calls `db`) runs inside a read-only transaction, so a write Quillo did not see is
  refused by the database (`db.error_kind` gives `"read_only"`); one that reaches none runs
  with none. Its arguments are made inside that transaction and under its `with`, so a
  record's default that needs an effect the `with` lacks fails as the body would
  (FEEDBACK56.md, Q-858).

**`quillo mcp app.qwe`** serves the tools to a client on the same machine, which starts it:

```json
{ "mcpServers": { "notes": { "command": "quillo", "args": ["mcp", "/home/ana/notes/main.qwe"] } } }
```

It checks the program (one that does not check does not start), runs its top level
(`db.open`, tables), then answers MCP on stdin and stdout, one JSON message a line. Routes are
not served. Only MCP's messages reach stdout: `print` goes to stderr under `quillo mcp`, and on
Unix whatever else the program or a plugin writes to stdout goes there too. `stdin`, `read_line`
and `exit` are errors. It ends when stdin closes, and on SIGTERM. There are no tokens on
stdio: the person who starts it is the caller.

Quillo speaks MCP's revision 2026-07-28 only. A client of the earlier revision, which opens with
`initialize` (Claude Code before 2.1.319, for one), is told the version is not supported. What a
client gets from a call:

- A typed result: `structuredContent` (held as `result` when it is not an object) and the same
  as JSON in a text block. An untyped one: the text block alone.
- `isError: true`, with text, for a tool's `err(v)`, an argument the door refuses (with the
  message `/_quillo/rpc` gives), a `reply` or a `respond` (with its text, or its status such as
  `404 Not Found`), a page, and `internal error` when the tool failed (the real message on
  stderr).
- The protocol's own errors for an unknown tool, a malformed message and the wrong version.
- An answer past what the door takes in (10 MiB of JSON, or 128 levels deep) is an `isError`
  saying so, never a cut or empty answer; a tool that crashes answers `internal error` and the
  door goes on serving.

A policy's `max_compute` and `max_body` hold `quillo mcp` as they hold `quillo run`. On
Windows, `print` goes to stderr, but other writes to stdout (a `.qwer` plugin's own output) are
not moved there, and break the session; it stops on Ctrl-C, Ctrl-Break or the console
closing, as on SIGTERM elsewhere.

The door over HTTP, `/_quillo/mcp`, is not built yet (FEEDBACK56.md, Q-853).

### What the browser can reach

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

  ```sh
  # .env, beside main.qwe
  PUBLIC_MAP_TOKEN=pk.test123
  STRIPE_KEY=sk_live_xyz
  ```

  ```quillo
  component Map() {
      state token = env("PUBLIC_MAP_TOKEN")              // written into the bundle
      <div class="map" data-token={token}>Map, token {token}</div>
  }

  server fn charge() -> bool = env("STRIPE_KEY") != null         // a secret stays on the server
  ```

  The page and its bundle hold `pk.test123`, and nothing of `sk_live_xyz`. Reading
  `env("STRIPE_KEY")` in the component instead stops the program as it loads (and `quillo
  check`): `` `env` is not available in the browser, where <Map> also runs; use a `server
  fn` (or name the variable PUBLIC_..., which says the browser may see it) ``.
- `http.get`, `http.post` and the rest work in a component's event handlers, `@mount`
  and the functions they call: the request goes from the page, with the browser's rules
  rather than the server's. The other site must allow this one (CORS: it answers with
  `access-control-allow-origin`), or the call is an error value that says what it may be
  (`http: no answer from api.example.com: it may not allow this page (CORS), or be plain
  http, or be unreachable; a server fn can call it from the server`). To reach your own
  server from a page that is not its own, a phone app's, call it from a `server fn` (the
  program's `http.*`, or [`site`](#calling-another-quillo-site)); the page's cookies are
  not sent to it (a path of the program's own, `/api/x`, goes to its own server, with
  its cookies); and it reaches what the visitor's machine can, such as a service on
  their own computer or network, not what the server can. Only `redirects: 0` and
  `timeout` of the options mean something there. Nothing goes through the
  server, which suits a site that limits each visitor, one that answers by where the
  visitor is, a visitor's own token kept in the page, and large files:

  ```quillo
  server fn upload_url(name: str) -> str {
      let me = session().user ?? reply 401 "Log in first"
      presign_put(me, name)                              // your bucket's signed URL
  }

  component Upload() {
      state done = false
      <input type="file" onchange={(e) => {
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
  server fn delete_note(id: int) -> map {
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
