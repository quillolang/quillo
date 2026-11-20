# Feedback from building thirty small apps for videos about Quillo

| | |
|---|---|
| App | None in `app/`: some thirty sample apps for a video series, each 20 to 80 lines, kept in `quillo-videos/videos/*/app` and published at github.com/quillolang/video-apps (a report card on `stats`, a fruit quiz on `vec`, a cannon game, a Mandelbrot poster, an arcade leaderboard, the Game of Life, a million-row phone book, a pizza shop's webhook, weather stations over UDP, command-line scripts, a spreadsheet on `run_script`, a playground on `check_program`, and more). Written at the user's word ("Any bug found in quillo?", then "Create FEEDBACK70.md to quillo-app1 for findings"; FEEDBACK70.md and FEEDBACK71.md were taken, so this is the next number) |
| Quillo | The released `quillo 0.8.6 (238dac2, 2026-11-16)`, from the apps' `get-quillo.sh`; the source read at this checkout (branch `security`) only to tell a design from a slip |
| Model | Claude (Opus 5.5, `claude-opus-5-5`), which built the apps and ran the probes below |
| Tested with | `quillo check`, `quillo run`, `quillo repl`, `quillo docs`; headless Chromium driven by Playwright (each video's screenshots); curl, `nc -u`; timings from the server's log and curl, on a laptop that was rendering video at the same time |

Numbers Q-1010 to Q-1019, the whole block agreed with the session that writes FEEDBACK71.md (which keeps Q-990 to Q-1009 for its fixes). Q-1015 to Q-1019 were added later, from five more videos, and Q-1030 to Q-1032 later still, from the block Q-1030 to Q-1039 agreed with the session that writes the security batch (Q-1020 to Q-1029 are FEEDBACK73's). Report only: no code changed, and the repros are written out below rather than added to `feedback/repros/`, since this checkout is another session's branch.

What worked well: almost everything the videos needed was already in the language, and fast. A Mandelbrot poster of 225,000 pixels took 29.9 s with complex numbers as maps, 5.6 s in plain floats with `QUILLO_JIT=0`, and 0.1 s compiled. A leaderboard view over a million plays went from 0.3 s a page to under a millisecond by taking `date.now()` out of the view, and a new play still showed on the very next page. A virtual list over a million entries answered each scroll's `server fn` call in 0.1 to 0.5 ms. `run_script` stopped every attempt to get out of its sandbox with a clear reason, and `check_program`'s messages taught the JavaScript habits better than the guide does.

## Seen again

None of the issues below is in INDEX.md, and none of INDEX.md's open issues came up.

## New issues

### Q-1010. A component whose only events are scrolls misses a scroll that comes without a pointer, a touch, a focus or a key

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (Playwright) | 2 |

A component's code comes when someone reaches for it: points at it, touches or focuses it, or presses a key in it. A component that is the page, or that runs by itself (`every`, `live`, `@mount`, ...), loads as soon as the page is idle. A component whose behaviour hangs on `onscroll` alone is neither. When its box is scrolled with no pointer, touch, focus or key (a script setting `scrollTop`, a link into the box, the browser putting a place back), the code never comes, the handler never runs, and the page goes on showing what the server rendered. In the phone book video this was a virtual list whose viewport moved to entry 466,675 and showed nothing: the rows stayed where entry 1 was. `onmount={show}` fixed it, as a mounted component loads at once. Scrolling with the wheel over the box works, since pointing at it loads the code first. A component that is the whole `<body>` works too, so the repro puts it beside a heading.

```quillo
component Scroller() {
    state at = 0
    <section>
        <div id="box" style="height: 100px; overflow-y: scroll" onscroll={(e) => { at = floor(e.target.scrollTop) }}>
            <div style="height: 5000px">tall</div>
        </div>
        <p id="at">scrolled to {at}</p>
    </section>
}
route GET / = <html><body><main><h1>A list</h1><Scroller/></main></body></html>
```

In Chromium, 1.5 s after the page loads, `document.querySelector("#box").scrollTop = 1200`; 1.5 s later the box is at 1200, the page says `scrolled to 0`, and no `/_quillo/` script was requested. With `#box` hovered and the wheel turned by 600 instead, the page says `scrolled to 600`. The same results with `@scroll`, with a named handler, and with a `server fn` for the state.

| Impact | |
|---|---|
| **Functionality** | What a scroll handler keeps (the rows to show, a "back to top" button, a reading position) is out of step with the box until someone points at it |
| **User** | A blank or wrong list after a jump; nothing in the log or the console |
| **Requests** | None: that is the problem (no bundle, no `server fn` call) |
| **Security** | No change |

**Suggestion:** treat `onscroll` (and `@scroll`) like `@mount`: a component that listens for scrolls loads when the page is idle. Or have the island's loader listen for `scroll` (it does not bubble, so in the capture phase) and, once the code is in, run the handler once on the element's current scroll position. Failing both, name scrolls in the guide's list of what does not load a component, with `onmount` as the way.

### Q-1011. A program with no routes that keeps running opens an HTTP port anyway, and does not start when that port is taken

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run-time error | 1 |

`quillo help` says `quillo run` serves a program "if it has routes", and the guide says a program with no routes is a script that runs from the top and ends, or is served when it has `server fn`s. A program with no routes and no `server fn`s that keeps running, because of a top-level `every` or `udp.listen`, listens on HTTP all the same (`quillo: 0 routes listening on http://127.0.0.1:8080`) and answers 404. So it cannot start beside the app it feeds: pretend weather stations, a script sending readings over UDP to a board on 8080, stop at once with `cannot listen on 127.0.0.1:8080: Address already in use`, and needed `--port 8081` for a port they never use.

```quillo
// tick.qwe: no routes, no server fn
every(1000, () => { print("tick") })
```

```sh
python3 -m http.server 8200 --bind 127.0.0.1 &      # anything on the port
quillo run tick.qwe --port 8200
# quillo: cannot listen on 127.0.0.1:8200: Address already in use (os error 48)    (exit 1)
quillo run tick.qwe --port 8198                      # with the port free
# quillo: 0 routes listening on http://127.0.0.1:8198
curl -s -o /dev/null -w "%{http_code}" localhost:8198/      # 404
```

A UDP echo with nothing else (`udp.listen(9123, (p) => udp.reply(p, p.text ?? ""))`) does the same.

| Impact | |
|---|---|
| **Functionality** | A sender, a sensor bridge or a timer script cannot run beside a server on the default port without `--port` |
| **User** | `Address already in use` for a port the program has no use for, and a `listening` line that suggests it serves something |
| **Requests** | An HTTP listener that answers 404 to everything |
| **Security** | A port open for no reason; on `--host 0.0.0.0` it is open to the network (answering only 404) |

**Suggestion:** open the HTTP port only for a program with routes, `server fn`s, channels or media; keep the process alive for `every` and `udp.listen` without it, and say so (`quillo: running (every 1000 ms, UDP 9123)`). Or, at the least, say in `quillo help` and the guide that such a program serves too, and why.

### Q-1012. `quillo check` on a script warns about the classes in the folder's stylesheets

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

A script kept beside an app (the weather stations next to the board, a tool next to a site) shares the folder, and `quillo check tool.qwe` reads the stylesheets under `public/` as its own: every class the app writes is "styled here, but nothing in the program writes it". The script has no routes, so it serves none of them.

```sh
# public/style.css:  .card { color: red; }
# main.qwe:  a route that writes <p.card>, and files("public", path)
# tool.qwe:  print(len(read_file("public/style.css") ?? ""))
quillo check main.qwe   # main.qwe: ok
quillo check tool.qwe   # ./public/style.css:1: warning: `.card` is styled here, but nothing in the program writes it
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A warning for each of the app's classes when the script is checked, which teaches to ignore the warning |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** check `public/` sheets only for a program that serves pages (routes or components); or only the sheets a program links to or serves with `files`.

### Q-1013. `run_script` has `date` but not `math`, `stats`, `re` or `format_number`, and calls what it left out an unknown name

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, docs | run-time error | 2 |

A spreadsheet whose cells take formulas is the guide's own kind of use for `run_script`, and formulas want maths and formatting. A script gets the builtins of `SAFE` in `src/script.rs` and the pure half of `date`, so `sqrt(16.0)` and `date.add_days(...)` work, but `math.sqrt(16.0)`, `stats.mean([1, 2, 3])`, `re.test("abc", "b")` and `format_number(1234.5, "en-US", 2)` are each `unknown name`. `range` and `repeat` are left out on purpose ("cannot make huge values from small ones"), which is sound, but the guide says only "the builtins that only compute (`len`, `upper`, `map`, `filter`, ...)", and the message reads like a typo.

```quillo
run_script("sqrt(16.0)", {})                        // 4.0
run_script("date.add_days(\"2026-11-17\", 1)", {})  // "2026-11-18"
run_script("math.sqrt(16.0)", {})                   // err("line 1: unknown name `math`")
run_script("stats.mean([1, 2, 3])", {})             // err("line 1: unknown name `stats`")
run_script("format_number(1234.5, \"en-US\", 2)", {}) // err("line 1: unknown name `format_number`")
run_script("range(3)", {})                          // err("line 1: unknown name `range`")
```

| Impact | |
|---|---|
| **Functionality** | Formulas cannot round for show, average a column, or test a pattern |
| **User** | "unknown name `math`" for a name the guide documents everywhere else |
| **Requests** | No change |
| **Security** | No change today. Each module would need the same care as `SAFE`: `vec.zeros`, `vec.full`, `vec.linspace` and `vec.arange` make big values from small ones; `stats` and `math` do not; `re` is safe as long as its engine stays linear |

**Suggestion:** give scripts `math`, `stats`, `re` and `format_number` (and `vec` without its makers), as `date` is given; list what a script has in the guide; and for a builtin left out on purpose, say so: "`range` is not available in a script: it can make a huge list from one number".

### Q-1014. The guide says the bodies of several routes on one path are a `oneOf`; overlapping ones are an `anyOf`

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading `quillo docs` | 0 |

The OpenAPI chapter says "Several routes on one method and path shape are one operation: their bodies a `oneOf`". With a catch-all after them, as a webhook needs, they are an `anyOf`, which is right (one body fits two of them), and matches the rule the same chapter gives for types: "A union that one value could fit twice ... is an `anyOf`, others a `oneOf`".

```quillo
route POST /hooks { json: { type: "order.placed", id: str } } = "placed"
route POST /hooks { json: { type: "order.cancelled", id: str } } = "cancelled"
route POST /hooks { json: { type: str } } = "ignored"       // without this line: oneOf
```

`quillo docs main.qwe | jq '.paths."/hooks".post.requestBody.content."application/json".schema | keys'` gives `["anyOf"]`, and `["oneOf"]` without the last route.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A reader expecting `oneOf` in a gateway's config finds `anyOf` |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** "their bodies a `oneOf`, or an `anyOf` when one body could fit two of them (a catch-all route)".

### Q-1015. A random value in a component's body is drawn twice, and the browser's replaces the server's when the component wakes, silently in production

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (Playwright) | 1 |

A component's body runs on the server to make the page, and again in the browser when the component is resumed. A `let` (or an expression in the markup) that calls `random_int`, `random`, `uuid` or `now` gets one value on each side. The browser finds its text differs from the server's, so it makes the component again, and draws a third time. The page shows the server's value until someone points at the component, then a new one. Under `--watch`, the console says so (`<Raffle> was made again, not taken up from the server's HTML: where it wrote "8631", the browser makes "5042"`). Without `--watch`, nothing says it at all. `quillo check` is silent in both, though it already warns about a lone assignment in a component's body. `state ticket = new_ticket()` is right: a state's first value is worked out once, on the server, and kept. The quiz video used this on purpose ("let, not state"), so it cost nothing there; in an app, it is a raffle ticket, an order reference or a "your code is" that changes under the user's pointer.

```quillo
fn new_ticket() -> int {
    print("a ticket is drawn")
    random_int(1000, 9999)
}

component Raffle(name: str) {
    let ticket = new_ticket()             // state ticket = new_ticket() keeps one
    state prize = ""
    <section.raffle>
        <p>{name}'s ticket: <b>#{ticket}</b></p>
        <button onclick={() => { prize = "a kite" }}>Draw a prize</button>
        {prize != "" && <p>{prize}</p>}
    </section>
}

route GET / = <html><body><main><h1>Lucky Draw</h1><Raffle name="Ada"/></main></body></html>
```

`quillo run main.qwe` (no `--watch`); in Chromium, the page says `#6300`; point at the raffle, and 1.2 s later it says `#9331`. The console has `a ticket is drawn` twice and no warning, and the server's log has it once. With `--watch`: the same, plus the warning above. `quillo check`: `ok`.

The guide's own example has this: `component Receipt(total: float, at: str) { <p>... ref {uuid()}</p> }`, said to show "a uuid, on the server and in the browser alike". Under `--watch`, pointing at it gives `<Receipt> was made again ...` and a new uuid. Without `--watch`, it kept the server's uuid in this test, as it has no handlers or state and seems never to be resumed. So the example is a quiet case of the issue, not a demonstration that the two sides agree.

| Impact | |
|---|---|
| **Functionality** | A value the user has read changes under their pointer; the component is made twice in the browser |
| **User** | A ticket, reference or code that is not the one the server rendered, logged or saved; nothing in the console without `--watch` |
| **Requests** | No change |
| **Security** | A reference shown to the user (an order's, a one-time code's) can differ from the one the server kept |

**Suggestion:** have `quillo check` warn when a component's body (a `let`, or the markup) calls `random`, `random_int`, `uuid` or `now`, directly or through a function the component uses, with the fix: "drawn again in the browser; `state x = ...` draws once, on the server". Change the guide's `Receipt` to take its reference as a prop, or to keep it in a `state`, and say why.

### Q-1016. `pages // 2`, Python's whole-number division, is `pages` and a comment, and `quillo check` says `ok`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | REPL, `quillo check` | 1 |

`//` starts a comment, so a Python habit for whole-number division keeps the left side and drops the rest without a word. `quillo check` already turns other habits into hints (`there is no \`**\`: \`pow(x, 2)\``, `there is no \`===\``, `there is no \`++\``, `there is no \`in\``), but this one parses, so nothing says it.

```quillo
// Halves: a Python habit.
let pages = 7
let half = pages // 2
print(half)
```

`quillo check main.qwe`: `main.qwe: ok (0 routes, 0 functions, 0 components)`; `quillo run main.qwe`: `7`. In `quillo repl`, `7 // 2` gives `7`, and `floor(7 / 2)` gives `3`. The "Possible or not" video asks it as a question, so it cost nothing there.

| Impact | |
|---|---|
| **Functionality** | Half of something is the whole of it: a page count, a midpoint, a split |
| **User** | A wrong number, with no error and no warning |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** have `quillo check` warn when a comment starts right after an expression on the same line and holds only a number or a name (`// 2`, `// n`): "`//` starts a comment here; whole-number division is `floor(a / b)`". A comment of words (`let n = 7 // pages in the book`) is left alone. Name it in the guide's list of habits from other languages, beside `**`.

### Q-1017. A `<style>` built from a value with a plain template or `+` is CSS injection, and `quillo check` warns of the same in SQL but not here

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check`, browser (Playwright) | 1 |

`` css`...` `` writes each `${...}` as one CSS value (its `{`, `}`, `;` and `\` as CSS's escapes), as the guide says. A plain template string or `+` does not, and `<style>{...}` takes its text as it is. So a value from a request ends the rule and adds its own. `quillo check` already warns when `db.query` gets SQL built from values ("a value containing a quote can change the query (SQL injection); write `?`"). The same mistake in a `<style>` passes.

```quillo
// Joining a value into SQL text, and into a <style>, to see what quillo check says of each.
db.open(":memory:")
route GET /a ? { id: str = "1" } = db.query(`select 1 where 1 = ${id}`)
route GET /b ? { c: str = "red" } = <html><body><style>{`p { color: ${c} }`}</style><p>hi</p></body></html>
route GET /c ? { c: str = "red" } = <html><body><style>{"p { color: " + c + " }"}</style><p>hi</p></body></html>
```

`quillo check main.qwe`: one warning, for line 3 (the SQL); none for lines 4 and 5, then `ok (3 routes, ...)`. In the CSS video's bakery, a gift card's colour came from `?color=`. The link `color=red} main{display:none} body::after{content:"Closed for good. Sorry!";font:700 34px Georgia}` made the page say only "Closed for good. Sorry!" (Chromium: `main` hidden, the `::after` text shown). With `` css`.gift { border-color: ${color} }` `` the same link gave `red\7d  main\7b display:none\7d ...`, one invalid value, and the page as it was.

| Impact | |
|---|---|
| **Functionality** | No change for honest values |
| **User** | A link can hide, rewrite or cover any part of the page, from text the server sent |
| **Requests** | A rule with `url(...)` makes every visitor's browser ask another server, with their address (where the site's own headers allow it) |
| **Security** | CSS injection from a query, a form or a stored value: defacing, phishing text over the page, and tracking |

**Suggestion:** have `quillo check` warn when the child of a `<style>` is a template string with `${...}`, or `+` with a value, and is not a `` css`...` `` (a `style=` attribute built the same way may deserve the same look; not tested here): "a value containing `}` can add rules (CSS injection); write `` css`...` ``". It is the SQL warning's twin, and the fix is three letters. The guide's line on `` css`...` `` could say what happens without it.

### Q-1018. An error dropped as the value of an `if` gets no warning from `quillo check`, though the run says "an error no statement took"

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` against the run | 1 |

`quillo check` warns of an error value dropped as a statement of its own, with the fix (`save(...)?` or `let _ = save(...)`). The same call as the only thing in an `if`'s block, an `if` whose value nothing takes, gets no warning. At run time, the program reports all three cases alike: `an error no statement took`. The kanban video's drop handler was written that way first, `if dragging != null { move(dragging, stage) }` followed by two more lines. A `server fn` that refused a move would have been dropped, with nothing said until it happened.

```quillo
// An error value dropped three ways, to see which quillo check says.
fn save(n: int) -> int ! str { if n < 0 { return err("negative") }; n }

fn a(n: int) {
    save(n)                         // a statement on its own
    print("a done")
}

fn b(n: int) {
    if n != 0 { save(n) }           // the value of an if, which nothing takes
    print("b done")
}

fn c(n: int) {
    let f = () => { if n != 0 { save(n) }; print("c done") }
    f()
}

a(-1)
b(-1)
c(-1)
```

`quillo check main.qwe`: one warning, line 5; `ok (0 routes, 4 functions, 0 components), 1 warning`. `quillo run main.qwe`: the same warning, then `quillo: error: main.qwe:5: an error no statement took, made at main.qwe:2: negative (...)`, and the same for lines 10 and 15.

| Impact | |
|---|---|
| **Functionality** | A failed save, move or send inside an `if` passes unseen until it happens |
| **User** | A change that did not happen, with the page carrying on as if it had |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** have the checker treat an `if` (or `match`) whose value nothing takes like a statement: an error-giving call as the value of any of its branches is dropped, with the same warning and fix. The run already reports it.

### Q-1019. `quillo check` says nothing of markup a keyboard can't reach: `onclick` on a `div` or a `span`, and an `<img>` with no `alt`

| Kind | Detected by | Attempts |
|---|---|---|
| missing check | the keyboard, Playwright's ARIA snapshot | 1 |

`quillo check` already knows the markup well (classes no sheet styles, a space lost between lines, a dynamic class). It says nothing of the most common accessibility mistakes, which a keyboard user meets at once. In the keyboard video's first version, a running club's sign-up worked with a mouse. With the keyboard, Tab went to the name, the email, then off the page and back to the name. The distance chips, the box to tick and the Join "button" were `<span>`s and a `<div>` with `onclick`, so Tab never reached them. Playwright's `ariaSnapshot()` of the form gave `text: 5 km 10 km`, `paragraph: I'll bring water...` and `text: Join Saturday's run`: no radio, no checkbox, no button.

```quillo
// A sign-up form that a keyboard can't finish, to see what quillo check says.
component Signup() {
    state agreed = false
    state sent = false
    <section>
        <input name="name" placeholder="Your name"/>
        <input name="email" placeholder="Email"/>
        <span class={{ box: true, on: agreed }} onclick={() => { agreed = !agreed }}></span> I agree to the rules
        <div class="next" onclick={() => { sent = agreed }}>Join</div>
        <img src="/logo.png"/>
        {sent && <p>Welcome!</p>}
    </section>
}
route GET / = <html><body><Signup/></body></html>
```

`quillo check main.qwe`: `main.qwe: ok (1 route, 0 functions, 1 component)`, with no warning.

| Impact | |
|---|---|
| **Functionality** | Nothing a keyboard can't reach can be used without a mouse |
| **User** | Keyboard and screen-reader users can't finish the form; a screen reader announces "Join" as text |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** warnings with the fix, as the habits from other languages have:
- `onclick` (or `@click`) on an element that is not interactive (`div`, `span`, `p`, `li`, ...) and has no `role` and `tabindex`: "a `<div>` with onclick can't be reached with the keyboard: use a `<button>`".
- An `<img>` with no `alt`: "`alt=\"\"` if it is decoration, or words for what it shows".
- A `<style>` or stylesheet rule `:focus { outline: none }` with no `:focus-visible` rule: "the keyboard's ring is gone everywhere: style `:focus-visible` instead".

An `<input>` named only by its placeholder is a softer case (Chromium names the box by its placeholder), so a hint at most.

### Q-1030. The guide writes `onclick` and `value={x}`, but `quillo new`, the hover and two hints still teach `@click` and `bind:value`

| Kind | Detected by | Attempts |
|---|---|---|
| inconsistency | the user, then `quillo new`, `quillo check`, `quillo lsp` | 1 |

Since 0.7 an event is a real HTML attribute (`onclick`, `oninput`, `onsubmit`, `onchange`), and what a person changes is two-way by itself when it shows a state: `<input value={draft}/>`, `checked={done}`. The 0.8.6 guide writes it that way everywhere, and says `@click` and `bind:` are the same in other words. But what Quillo itself writes and says still teaches the older spelling:

- `quillo new demo`: `ui/todos.qwe` has `<form @submit={...}>`, `<input bind:value={draft} .../>` and `@change={() => show(toggle(t.id))}`.
- `quillo new mini --minimal`: `ui/page.qwe` has `<button @click={() => { count += 1 }}>`.
- The hover over `component` from `quillo lsp`: "Handlers: `@click={() => { count += 1 }}`."
- Two warnings, given for a program written with `on...`: `<button onclik={...}>` gets "`@clik` is no event the browser knows: did you mean `@click`?", and `<input value={form.name}/>` gets "typing here changes nothing: ... take what changes with `@input={(e) => ...}`, or add `readonly`".
- Two hints from `quillo check`, given for a program written with `onclick`:

```quillo
// Plain event attributes, as the guide writes them, and what quillo check suggests for two mistakes.
server fn total() -> int = 42

component Counter() {
    state count = 0
    <div>
        <button onclick={count += 1}>+</button>
        <p>{total()}</p>
    </div>
}
route GET / = <html><body><Counter/></body></html>
```

`quillo check`: `main.qwe:7: error: an assignment is not a value: a handler is a function, `@click={() => { count += ... }}``. With line 7 made `onclick={() => { count += 1 }}`: `main.qwe:8: error: `total` waits on the server, so <Counter> can only call it from an event handler (@click={() => { ... }}) or to set state: state x = total(...)`.

So a program that starts from `quillo new`, or a person or model who learns from the hints, writes `@click` and `bind:value`, which the guide no longer shows. That is how the videos' apps came to use them: 29 of them, built in the starter's style, have `@click` and `bind:value`; the user pointed out the plain attributes, and the apps made from now on use them. Both spellings work (checked in Chromium on 0.8.6: `value={x}` on a text box, a checkbox's `checked`, a range, a number box and a `<select>`; `oninput` with `e.value`; `onsubmit={add}` with a function's name), so nothing breaks; the cost is two styles in one codebase, and the starter teaching the one the guide has moved away from.

| Impact | |
|---|---|
| **Functionality** | No change: both spellings work |
| **User** | A new program and the hints teach a style the guide no longer uses; code from both mixes them |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** write the templates, the hover and the hints the guide's way: `onsubmit`, `onchange`, `onclick` and `value={draft}` in `quillo new`, "Handlers: `onclick={() => { count += 1 }}`" in the hover, and `onclick={() => { ... }}` in both hints. A `quillo fmt` or `quillo check --fix` that turns `@click=` into `onclick=` and `bind:value=` into `value=` would bring older programs along.

### Q-1031. Vue's `@click.prevent` and `v-model` pass `quillo check` in silence, and do nothing

| Kind | Detected by | Attempts |
|---|---|---|
| missing check | browser (Playwright) | 1 |

The guide says there are no modifiers: `@click.prevent` is an event named `click.prevent`, which never fires. `quillo check` already knows the browser's event names (`@clik` gets "did you mean `@click`?", `@mouseEnter` gets "the browser's event names are lower case"), but a name with a modifier gets nothing. Vue's `v-model` and `v-on:click`, and Alpine's `x-model`, are plain attributes to Quillo, so they too pass and do nothing, though `quillo check` catches React's habits (`onClickCapture`, `setName`) and JavaScript's.

```quillo
// Vue's habits: a modifier on an event, and v-model. quillo check says ok to both.
component Order() {
    state n = 0
    state name = ""
    <div>
        <button id="vue" @click.prevent={() => { n += 1 }}>Vue's way</button>
        <button id="plain" onclick={() => { n += 1 }}>Plain</button>
        <input id="model" v-model="name"/>
        <p id="out">{n}|{name}</p>
    </div>
}
route GET / = <html><body><Order/></body></html>
```

`quillo check`: `main.qwe: ok (1 route, 0 functions, 1 component)`. In Chromium, three clicks on "Vue's way" left `0|`; one click on "Plain" made it `1|`, and typing "Ada" in the `v-model` box left `1|`, with nothing in the console. `@submit.prevent` is the same: `<form @submit.prevent={() => { sent += 1 }}><input name="q"/><button>Go</button></form>` passes `quillo check`, and in Chromium the button sent the form as plain HTML (`GET /?q=tea`, a new page), with `sent` still 0.

| Impact | |
|---|---|
| **Functionality** | A handler that never runs, or a field that follows nothing, with `ok` from the checker |
| **User** | The button or field does nothing, with no error anywhere |
| **Requests** | `@submit.prevent`: the form is sent to the server as plain HTML, where a handler was meant |
| **Security** | No change |

**Suggestion:** warnings with the fix, as for React's habits: "`@click.prevent` never fires: there are no modifiers; write `onclick={(e) => { e.preventDefault(); ... }}`" (and for `@submit.prevent`: "`onsubmit` already keeps the page from reloading"), and "`v-model` is Vue's: write `value={name}`, which is two-way by itself".

### Q-1032. Svelte's `on:click={...}` gets "takes only text written here, as the browser runs it as code"

| Kind | Detected by | Attempts |
|---|---|---|
| unclear message | `quillo check` | 1 |

`on:click={() => { n += 1 }}`, Svelte 4's spelling, and `onclick.prevent={...}` are read as HTML's own string handlers, as `onclick="..."` is, so the error is about text and code: "`on:click` in <button> takes only text written here, as the browser runs it as code". It is right that a lambda cannot go there, but it does not say what to write. `@mouseEnter` and `onClickCapture` show what a good message is here: what the habit is, and the spelling to use.

```quillo
// Svelte's spelling of an event.
component Pot() {
    state n = 0
    <button on:click={() => { n += 1 }}>{n}</button>
}
route GET / = <html><body><Pot/></body></html>
```

`quillo check`: `main.qwe:4: error: `on:click` in <button> takes only text written here, as the browser runs it as code`.

| Impact | |
|---|---|
| **Functionality** | No change: it is refused, rightly |
| **User** | Someone coming from Svelte reads about text and code, not about `onclick` |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** "`on:click` is Svelte's: write `onclick={...}`", and for `onclick.prevent`, the message for `@click.prevent` (Q-1031).

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `component ReportCard(start: [Pupil])` | TypeScript's `Pupil[]`, half remembered | `expected a type, found [`; it is `list<Pupil>` | 1 check |
| `map(around, ([dr, dc]) => ...)` | JavaScript's destructured parameters | "a function's parameters are plain names, not patterns: `(p) => p[0] + p[1]`": a model message | 1 run |
| `lasts[i / 20000]` | integer division in other languages | `cannot index list with float` when it ran; `floor(i / 20000)` | 1 run |
| `fn escape(cx, cy)` for the Mandelbrot | the natural name | the warning that it hides the builtin `escape`; renamed `rounds` | 1 check |
| `let route = "..."` in a probe | a plain word | "`route` is a keyword" | 1 check |
| Two elements at the root of a component | a fragment, as in JSX | "this element is not on the page: only the last one in a body is" | 1 check |
| `class={p.severity}` for `.error` and `.warning` | the data names the class | two unused-class warnings; the guide's `/* set elsewhere: ... */` was the way, which I found afterwards; I spelled the two out | 1 check |
| `curl -d '{"type": ...}'` to a `json:` route | it looks like JSON | curl sends it as a form, so no pattern matched; `curl --json` (the guide says a body is JSON by its content type, or with none and a `{`) | read first |
| `range(...)` and `repeat(...)` in a formula | they only compute | `unknown name` (Q-1013) | 2 runs |
| Blamed Quillo for `translateY(7e+06px)` | the attribute read back that way | Quillo writes `7000000px` on both sides; Chromium serializes a style set through the style object with an exponent | 1 probe |
| A virtual list with only `@scroll` | handlers load what they need | a blank list after a jump (Q-1010); `onmount` | 2 runs |
| `@click={...}` and `bind:value={x}` in 29 apps | the starter from `quillo new` (a video was about it) and `quillo check`'s hints write them | they work; the user pointed out that the guide writes `onclick` and `value={x}`, which new apps use (Q-1030) | 29 apps converted, then put back as their videos show them |
| Earlier apps: `pad_start`, `lines()`, `<>...</>` | other languages' builtins and JSX | none exists; `slice(str(10000 + n), 1)`, `split(s, "\n")`, one root element | 1 check each |
| Earlier apps: a variable named `money`, `type_of`, `remove`, a route parameter named `file` | plain words | each hides a builtin; the checker says so | 1 check each |

## Limitations

- `quillo repl` takes no program: a program's functions are tried by pasting them in, or with a small script.
- `image.convert` reads JPEG, PNG, WebP and GIF only, and nothing makes an image from pixels; the Mandelbrot poster is painted on a `<canvas>` in the browser from the server's numbers.
- A virtual list over a million rows needs a scroll track shorter than the rows' height (Firefox stops near 17.9 million pixels), with the scroll position read as a fraction. The app does that by hand.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `run_script` and the server's environment, files, network and database: `env("HOME")`, `read_file(...)`, `http.get(...)`, `db.query(...)`, `"HOME".env` | each an error value saying why; the rest of the sheet kept working |
| `run_script` with `let n = 0; while true { n += 1 }; n`, and a string doubled for ever | `the script took too many steps`; `the script made a value larger than 1 MB` |
| `run_script("fn steal() = 1", {})` | `a script cannot define functions (use (x) => ...)` |
| `check_program` on `name \|\| "anonymous"`, `items.push(1)`, `count && ...` | three warnings, each with the line to write instead |
| `check_program` on `${nam}` in a route with `:name`; on `len(42)`; on `s.lenght` | `did you mean name`; the types `len` takes; ``len(x)` is how many it holds`` |
| Six routes on `POST /hooks`, a literal `type` each, and a catch-all | each event to its route; `"stars": "five"` falls to the catch-all (first match wins); no `type` at all: 400, and the log says `json.type is missing, and must be the value the route asks for` |
| A UDP packet that is not JSON, and one with `c` as an int or no `wind` | skipped by a `match` with `_`; `c: int \| float as c` and `wind: int = 0` take the others |
| `live(board)` with readings over UDP every second, three stations | each open page followed every reading |
| A view reading `date.now()`, then not, a million plays behind it | 0.26 to 0.33 s every page; then 0.36 s, and 0.4 to 2.4 ms after; a new play: one render, then fast again; `--no-view-cache`: 0.23 to 0.35 s every page |
| The Mandelbrot in plain floats, called four times in a script | 76 to 148 ms each, the first no slower (a function with a loop is compiled on its first call) |
| A page of 225,000 points in a request: maps, floats with `QUILLO_JIT=0`, floats | 29.9 s, 5.6 s, 0.1 s, and the log's `an instance has been computing for 5 s without letting go` for the first two |
| A page with a million list items | 55 MB, served in 1.0 s; Chromium took 3 to 4.5 minutes to finish loading it (the laptop was busy) |
| A `server fn` called at every scroll and every keystroke | the latest answer won every time: no rows or messages from an older call |
| `every(floor(1000 / speed), step)` under `if running`, with the speed on a slider | started again at the new speed, and stopped with `running` |
| `stats.mean`, `median`, `mode`, `pstdev`, `zscore`, `histogram` with edges; `vec.dot`, `vec.norm`, `vec.argsort` | Python's answers; the same in the browser |
| A whole float interpolated in a component, `${top}px` with 7000000.0 | `7000000px` on the server and in the browser |
| A script with `args()`, `stdin()`, `exit(2)`, `env` and `.env` | each as the guide says; the exit status reached the shell |
| The first page after `quillo run`, in the Mandelbrot app | 0.95 s, then 0.11 s; not the compiler (above); not looked into further |
