# Feedback from checking Quillo single-page apps with JavaScript off

| | |
|---|---|
| App | None of its own: a check done at the user's request, 2026-11-19: "SPA should work on no-js, please check and make FEEDBACK77.md for the findings". The SPA checked is Quillo's own example, `spa.qwe` (quillolang/examples at `e87822e`), plus 12 small programs, one for each pattern a single-page app is made of |
| Quillo | The released `quillo 0.8.8 (f35bf2d, 2026-11-19)` on the PATH (= tag v0.8.8). The guide and `src/client/runtime.js` read at this checkout (branch `feedback75`, `54d20b9`, which is main), not built |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | headless Chrome 157 through `feedback/tools/chrome.mjs`, each scenario run twice with real mouse clicks and typing: once with JavaScript off (`Emulation.setScriptExecutionDisabled`), once with it on. curl for what the server sends. The programs, the driver and its results are kept in `.git/wip/feedback77/` (`spa.qwe`, `probes.qwe`, `drive.mjs`, `result.json`, `button.qwe`, `hash.qwe`) |

Numbers Q-1140 to Q-1146, from the block Q-1130 to Q-1149 (FEEDBACK76 has Q-1130 to Q-1139). This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed. The repros are written in the report.

**How the request is read.** "Work on no-js" is read as: a visitor whose browser runs no JavaScript can do what the app is for, not only read it. That visitor can be someone who turned scripts off, a browser or proxy that blocks them, a slow phone before the bundle arrives, or a crawler that does not run scripts. Reading means deep links, navigation, the right content and the right status. Doing means searching, adding, buying, deleting and moving between tabs and pages. Each pattern was checked both ways, so a difference between the two columns is what JavaScript adds.

The React and Vite recipe of FEEDBACK69 is not checked. A React app drawn in the browser shows nothing without JavaScript, and Quillo does not run React on the server, so there the only answer is a `<noscript>` line in its `index.html`.

What worked well:

- **Reading an SPA needs no JavaScript.** Every address of `spa.qwe` comes from the server with its content: the product list with its live stock, each product, the cart, the about page. Unknown addresses (`/nope`, `/products/99`, `/products/abc`) come with 404. With JavaScript off, each link click loaded the right page.
- **Islands hide nothing.** A component's markup is in the page as the server drew it, inside a `<quillo-island style="display:contents">`. No CSS keeps it hidden until a script runs.
- **What HTML does by itself works, and JavaScript leaves it alone.** A GET search form, links to `?tab=b` and `?page=2`, `<details>`, and a form with `method`, `action` and named fields posting to a route all gave the same result with JavaScript off and on.
- **A form can be written for both.** With `onsubmit` and also a real `method`, `action` and `name`, plus a route: without JavaScript the route ran, and with it the handler ran instead. The note was added once either way (N2), because `@submit` keeps the browser from posting too.

## The answer

**No: with JavaScript off, a Quillo SPA can be read but not used. Every action that is a handler does nothing, and nothing tells the visitor.** In `spa.qwe`, Add to cart is a dead button, so the cart's Remove and Check out are never even reached. The guide's own form pattern does worse: pressing Add reloads the page and throws away what was typed (Q-1140). A `defer`red part never shows (Q-1141). And one thing found on the way matters with JavaScript on too: a `navigate()` written as a guard in a component sends the guarded page to anyone (Q-1142).

| Pattern | JavaScript off | JavaScript on |
|---|---|---|
| Deep link, refresh, a link to another page of the app | ✓ the server's page (a full load) | ✓ changed in place |
| Unknown address | ✓ 404 with the app's not-found page | ✓ |
| Data a page loads (`state x = load()`, `live(load)`) | ✓ in the first HTML | ✓ |
| **Add to cart** in `spa.qwe` (a button calling a `server fn`, the cart in state) | ✗ nothing happens, "Cart: 0" | ✓ "Cart: 1 (€39)" |
| **The guide's note form** (`onsubmit` calls a `server fn`; the input follows `state`, with no `name`, `method` or `action`) | ✗ the page reloads, the words typed are gone, nothing saved (Q-1140) | ✓ saved |
| The same form, also given `method`, `action`, `name` and a route | ✓ saved, by the route | ✓ saved once, by the handler |
| A button with `onclick` inside a form with `action` | ✓ saved, by the route | ✗ **saved twice**: the handler, then the browser's post (Q-1143) |
| Search with a GET form, the list filtered by `url().query.q` | ✓ | ✓, but the whole page reloads, so the app's state is lost (Q-1145) |
| Tabs by `state`, set by a button | ✗ nothing | ✓ |
| Tabs by a link to `?tab=b` | ✓ | ✓ |
| Tabs by the hash (`#b`) | ✗ always the first: the server never gets the hash | ✓ (once compared with `"b"`, not `"#b"`: Q-1146) |
| "Load more" by a button | ✗ nothing | ✓ |
| "More" by a link to `?page=2` | ✓ | ✓ |
| A button that calls `navigate("/there")` | ✗ nothing | ✓ |
| `ask("Delete it?")` before deleting | ✗ nothing | ✓ the dialog |
| `<details>` | ✓ | ✓ |
| Something shown by `state`, toggled by a button | ✗ nothing | ✓ |
| A `defer`red part | ✗ "Loading reviews…" for good (Q-1141) | ✓ |
| A guard in the component: `if !signed { navigate("/login") }` | ✗ the guarded page; the server answers 200 to anyone (Q-1142) | moves away after the page has come |

**My recommendation, in this order:**

1. **Fix the silent ones now:**
   - Q-1140: `quillo check` warns of a form whose only way to send is `onsubmit`.
   - Q-1142: `navigate()` while the server renders answers 303, as `redirect` does.
   - Q-1143: a click handler on a submit button keeps the browser from posting, as `@submit` already does.
   - Q-1141: a `defer`red part also comes in `<noscript>`.
   - Q-1146: the guide says the hash has no `#`.
2. **Write a guide section, "Without JavaScript".** Navigate by links, not buttons; put tabs, search and pages in the query; give a form `method`, `action` and names next to its `onsubmit`; keep a cart in the session, not in state. Rewrite `spa.qwe` the same way, so Quillo's own SPA example works without JavaScript.
3. **Then forms that call a `server fn` without JavaScript** (question 1, option A). `<form action={add_note}>` would post to the `server fn` when there is no script and come back to the page, and be a call when there is one. Designed together with FEEDBACK76's in-place forms for views (its question 4), it is one feature: a form that works with no script, and is a call with one.

## Part A: what was run

### A.1 `spa.qwe`, Quillo's example SPA

What the server sends, by curl (the page as text, scripts and styles left out):

| Address | Status | What the page says |
|---|---|---|
| `/` | 200 | Home Products About Cart: 0 Welcome A tiny shop, as a single-page app. Open the products. |
| `/products` | 200 | … Products Desk lamp €39 Only 3 left Notebook €12 10 in stock Fountain pen €58 Only 1 left Desk mat €24 5 in stock |
| `/products/1` | 200 | … Desk lamp Warm light, adjustable arm, a dimmer in the base. €39 Only 3 left Add to cart ← All products |
| `/cart` | 200 | … Cart Your cart is empty. Browse products. |
| `/about` | 200 | … Every page here is also rendered on the server, so links can be shared and pages load with their content even before any JavaScript runs. |
| `/nope`, `/products/99`, `/products/abc` | 404 | … Not found Nothing at /nope. Back home. |

In Chrome, the same walk both ways: Products, then Desk lamp, then Add to cart, then Cart, then Check out.

| Step | JavaScript off | JavaScript on |
|---|---|---|
| Products | `/products`, page loaded, "Products" | `/products`, same page, "Products" |
| Desk lamp | `/products/1`, page loaded, "Desk lamp" | `/products/1`, same page |
| Add to cart | same page, nav says "Cart: 0", no message | "Cart: 1 (€39)" |
| Cart | "Your cart is empty." | "Desk lamp €39 Remove Total: €39 Check out" |
| Check out | no button to press | "Your cart is empty." (ordered) |

The about page's own words, "pages load with their content even before any JavaScript runs", hold. Pressing anything that is not a link does not.

### A.2 The patterns, one program each

`probes.qwe`, N1 to N12, plus `button.qwe` and `hash.qwe`. Results as the driver printed them are in `result.json`. The table in "The answer" is made from them.

## New issues

### Q-1140. The guide's form pattern, with JavaScript off, reloads the page and throws away what was typed

| Kind | Detected by | Attempts |
|---|---|---|
| silent (data lost) | browser, JavaScript off | 1 |

The guide writes its forms like this: all 5 of its `<form onsubmit=...>` examples (lines 1961, 3002, 3106, 3271 and 5606, Live data among them) have no `action`, and none of their inputs that follow `state` has a `name`:

```quillo
component NoteForm() {
    state items = live(load_notes)
    state draft = ""
    <div>
        <ul>{for n in (items ?? []) => <li>{n}</li>}</ul>
        <form onsubmit={() => { add_note(draft); draft = "" }}>
            <input value={draft} placeholder="A note"/>
            <button>Add</button>
        </form>
    </div>
}
```

With JavaScript off, typing "hello" and pressing Add sends the form as HTML does with no `method` and no `action`: a GET of the same address. With no `name` on the input, nothing goes with it. The page loads again with an empty box, the list is unchanged, and the server's count stays 0. With JavaScript on, the note is saved.

| Impact | |
|---|---|
| **Functionality** | Every form written this way does nothing without JavaScript, and loses its input |
| **User** | The visitor types, presses the button, and sees the page blink with their words gone. No message says why. The developer sees nothing either: it works with JavaScript on |
| **Requests** | A GET of the page instead of a call: one more page drawn, nothing written |
| **Security** | No change |

**Suggestion:**
- `quillo check` warns of a `<form>` with `onsubmit` and no `action`, in a component: "without JavaScript this form reloads the page and sends nothing: give it method, action and names, with a route, or see question 1".
- The guide's examples gain `method`, `action` and `name` wherever the form saves something.
- Longer term, question 1.

### Q-1141. A `defer`red part is never shown without JavaScript: the placeholder stays, and the content sits in a `<template>`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser, JavaScript off; curl | 1 |

```quillo
server fn slow_reviews() -> list {
    sleep(600)
    ["Great", "Fine"]
}
component Reviews() {
    state reviews = defer(slow_reviews())
    <div>{reviews == null ? <p>Loading reviews…</p> : <ul>{for r in reviews => <li>{r}</li>}</ul>}</div>
}
route GET /n7 = <html><body><h1>Product</h1><Reviews/></body></html>
```

The reviews come in the same answer, as the guide says. But they come as `<template data-qt="0">…<li>Great</li><li>Fine</li>…</template>` followed by `<script>window[Symbol.for("quillo")].swap("0")</script>`. With JavaScript off the page reads "Product Loading reviews…" for good. With it on, "Product Great Fine". The guide says of `defer`: "The answer is one response, so search engines read the whole page, reviews included." A reader without scripts does not see them.

| Impact | |
|---|---|
| **Functionality** | Every deferred part is missing without JavaScript |
| **User** | The visitor sees "Loading…" that never ends |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** after each deferred part's `<template>`, the server also sends it inside `<noscript>`, with a `<noscript><style>` that hides its placeholder. Without JavaScript the part then shows at the end of the page, out of its place but there. With JavaScript, `<noscript>` is text and changes nothing. The guide says where it lands (question 2).

### Q-1142. `navigate()` while the server renders does nothing, so a guard in a component sends the guarded page to anyone

| Kind | Detected by | Attempts |
|---|---|---|
| silent; security | curl, browser | 1 |

```quillo
component Secret() {
    let signed = url().query.me == "1"
    if !signed { navigate("/login") }
    <p>the secret page</p>
}
route GET /n8 = <html><body><Secret/></body></html>
```

`curl /n8` gives `200`, no `Location`, and the body `<p id="secret">the secret page</p>`. With JavaScript off, the browser shows the secret page. With it on, the browser moves to `/login` once the page has come, but the page had already come, with the secret in it. In the same component, `redirect` would be an error, while `navigate()` is taken and does nothing.

| Impact | |
|---|---|
| **Functionality** | A guard written in a component guards nothing on the server |
| **User** | Without JavaScript, a visitor sees a page meant to be closed to them. With it, the page flashes before the move |
| **Requests** | The page answers 200 where a 303 was meant |
| **Security** | Whatever the guarded page shows goes to anyone who asks without scripts (curl, a crawler, a browser with JavaScript off), and it is in the HTML that JavaScript visitors receive too. Data a `server fn` checks is safe; markup the component draws is not |

**Suggestion:** `navigate(path)` while the server renders a page, before anything is sent, ends the render with `303 See Other` to that path, as `redirect` does in a route. Once a `defer` has started streaming, it is an error in the log, and `quillo check` warns of a `navigate` that can run in a component's body. The guide's SPA section says to guard in the route (`auth.required`) or in the `server fn`, never only by `navigate` (question 3).

### Q-1143. A button with `onclick` inside a form with `action` runs twice with JavaScript on: the handler, then the browser's post

| Kind | Detected by | Attempts |
|---|---|---|
| silent (a write made twice) | browser | 1 |

The way to make a button work both ways that a developer tries first:

```quillo
server fn add() -> int { count += 1; count }
component Add() {
    state n = 0
    <form method="post" action="/add">
        <button onclick={() => { n = add() }}>Add</button> <span>{n}</span>
    </form>
}
route POST /add {
    count += 1
    redirect("/b")
}
```

Without JavaScript: `count` 1, by the route. With JavaScript: `count` **2**, and the page reloaded. The handler called `add()`, and then the browser sent the form, since a click handler, unlike `@submit`, does not stop it. The handler's `n = 1` is lost with the reload. The guide says `@submit` "does not reload the page" and that `e.preventDefault()` must come "before anything the handler waits for". It does not say what a click on a submit button does.

| Impact | |
|---|---|
| **Functionality** | Every action written this way runs twice with JavaScript, and the page reloads, losing state |
| **User** | Two orders, two posts, two votes from one click. The visitor sees a reload where the app meant to stay |
| **Requests** | One call and one form post for each click |
| **Security** | No change, but a payment or a transfer written this way is made twice |

**Suggestion:** a click handler on a submit button (a `<button>` with no `type`, or `type="submit"`) inside a form stops the browser's post, as `@submit` does, so the handler is the JavaScript way and the form the other. Or, if that is too wide, `quillo check` warns of it: "this button's form posts too: call e.preventDefault() first, or put the handler on the form's onsubmit".

### Q-1144. Without JavaScript, every handler of an SPA is a dead button, and nothing says so

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, silent to the visitor | browser, JavaScript off | 6 patterns |

Measured dead with JavaScript off: Add to cart (`spa.qwe`; its Remove and Check out cannot even be reached, the cart being empty), tabs set by `state`, "Load more", a button calling `navigate`, `ask()` before deleting, something toggled by `state`. Restock, a button of the same kind, was not pressed. Quillo writes no `<noscript>` (none in `src/server.rs` or `src/client/runtime.js`) and no message. The pages look the same either way.

| Impact | |
|---|---|
| **Functionality** | An SPA is read-only without JavaScript |
| **User** | Buttons that do nothing, with no word why |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:**
- The guide section "Without JavaScript" (recommendation 2), and `spa.qwe` rewritten by it: a cart in the session, posted by forms, enhanced by handlers.
- Optionally, the island's markup could carry what a script-less visitor needs to know. A page with components could have a `<noscript>` line, which a program sets or turns off (`app.toml`, as the offline page is).
- The real cure is question 1.

### Q-1145. A GET form in an SPA reloads the whole page with JavaScript on, so the app's state is lost

| Kind | Detected by | Attempts |
|---|---|---|
| silent (state lost) | browser | 1 |

N3: a search form, `<form method="get" action="/n3"><input name="q"/>`, in a component that reads `url().query.q`. With JavaScript on, links in the same component change the page in place. But the form's submit was a full page load ("page loaded" by the driver's mark), with the right result. In an SPA like `spa.qwe`, whose cart is component state, a search would empty the cart. The guide says "clicks on same-site links inside components change the page without reloading" and says nothing of GET forms.

| Impact | |
|---|---|
| **Functionality** | Search and filter forms leave the SPA |
| **User** | The cart, an open panel, a scrolled list: gone after a search |
| **Requests** | A full page instead of the app's own render |
| **Security** | No change |

**Suggestion:** a same-site GET form inside a component that reads `url()` is a navigation like a link: `navigate(action + "?" + its fields)`, with Back and Forward. Without JavaScript it stays a plain GET, which already works.

### Q-1146. `url().hash` is the hash without `#`, which the guide does not say, so `url().hash == "#b"` is never true

| Kind | Detected by | Attempts |
|---|---|---|
| docs; silent | browser | 2 |

Opened at `/h#b`, and after a click on `<a href="#b">`, `url().hash` is `b`. JavaScript's `location.hash` is `#b`, which is what the probe compared first, so its tab never changed, with JavaScript on as well. The guide says `` `url()` gives `{ path, query, hash }` `` and the builtin's line says the same. Neither says which.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A model or a developer coming from `location.hash` writes a comparison that is never true, with no word |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** the guide and `quillo docs url` say "`hash`, without the `#` (`b` for `#b`)". `quillo check` could warn of a literal starting with `#` compared with `url().hash`.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `url().hash == "#b"` | JavaScript's `location.hash` | never true, both ways (Q-1146) | 1 run to find, 1 probe to confirm |
| The note form as the guide writes it | it is the guide's | without JavaScript, typed words lost (Q-1140) | found only with JavaScript off |
| A button with `onclick` in a POST form, for both ways | how one enhances a form | saved twice with JavaScript (Q-1143) | found only by counting on the server |
| `if !signed { navigate("/login") }` as a guard | how a client-side router guards | the server sends the page anyway (Q-1142) | 1 curl |

## Limitations

- **State lives in the browser.** Component state is the page's: without JavaScript there is nowhere for a cart, a tab or a "shown 20 rows" to live between two requests. Without JavaScript, it can only be in the address (query), a form's fields, the session or the database.
- **No way to call a `server fn` without JavaScript.** A `server fn` is reached at `POST /_quillo/rpc/<name>` with JSON. A form cannot send that, so every write without JavaScript needs a route written beside it, and the `server fn` and the route repeat each other (N2).
- **The hash never reaches the server.** Content chosen by the hash cannot be right without JavaScript. CSS `:target` can show and hide by it, which is outside Quillo.
- **A React SPA** shows nothing without JavaScript (not run; see "How the request is read").

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A component's markup before any script | visible: `<quillo-island style="display:contents">`, no hiding CSS |
| `live(load)` in a state initializer | the data is in the first HTML (`/products` lists stock) |
| Unknown addresses in `spa.qwe` | 404 with the app's own not-found page, both ways |
| `onsubmit` with `method`, `action`, `name` and a route | saved once by the route without JavaScript, once by the handler with it |
| A GET search form | the same filtered list both ways |
| Links to `?tab=b` and `?page=2` | the same page both ways |
| `<details>` | opens both ways |
| A number field following `state`, with a `name`, posted to a route | the route got its value (`bought 1`) |
| `<form idempotent>` (checked earlier the same day) | its key is a hidden field drawn by the server: a double click runs once, with JavaScript off too |

## Questions for the user

### 1. Should a form that calls a `server fn` work without JavaScript?

**Yes, as a form action: it turns the most common silent loss (Q-1140) into a working page, and removes the route a program writes beside each `server fn` today.** It happens on every form that saves something. Measured: the guide's form loses its input without JavaScript, and the one that works both ways needs a route repeating the `server fn` (N2).

Three ways:

- **A. `server fn`s as form actions.**
  - **How it reads:** `<form action={add_note}>` with named fields (`<input name="text" value={draft}/>`).
  - **With JavaScript:** a call, as `onsubmit` is today, the fields passed by name.
  - **Without:** a POST to the `server fn`'s address, behind the same gate as a `server fn` (`Sec-Fetch-Site`, Origin). The `server fn` runs with the fields as its arguments and answers 303 back to the page the form was on, which is drawn again from the server's data. This is how Remix, SvelteKit and Next.js make their form actions work before scripts.
  - **What it covers:** every write whose result lives on the server: notes, Restock and buying in `spa.qwe`, deleting. Not a cart kept in state.
- **B. Every handler replayed on the server.**
  - **How it works:** each button in a component is drawn as a submit button of a form that carries the island's state (it is in the page already, as `data-s`) and which handler was pressed. The server runs the handler on that state and draws the page again with the new state.
  - **What it covers:** state tabs, "Load more", the cart.
  - **What it costs:** every click is a full page post, and the state comes back from the visitor, so the server must sign it or treat it as anything the visitor could send. Back resubmits.
- **C. Neither.** The guide section, the `spa.qwe` rewrite and the Q-1140 warning only.

| Impact | A: form actions | B: handlers replayed |
|---|---|---|
| **Functionality** | Forms that call a `server fn` work without JavaScript; buttons outside forms do not | Every handler works without JavaScript |
| **User** | Saving, deleting and buying work without scripts; one definition instead of a `server fn` and a route | The whole SPA works without scripts, one page load per click |
| **Requests** | Without JavaScript, a POST and a 303 for each send, as a plain form | Without JavaScript, a POST with the island's state for each click (the state's size each time) |
| **Security** | The `server fn`'s own gate and checks; the arguments are the visitor's, as they are today | State from the visitor must be signed, or every handler must treat it as untrusted |

What today's way has that A or B would lose: a `server fn` is only ever called with JSON from the program's own pages, which keeps its door simple.

**My recommendation:** A, designed with FEEDBACK76's in-place forms for views (its question 4), as one feature: a form posts without JavaScript and is a call, or an in-place post, with it. Not B: its cost and its trust in the visitor's state are too high for what it adds over A plus links.

**The question:** shall I plan `server fn`s as form actions (A)? If not, C: the guide section, the example rewrite and the Q-1140 warning.

### 2. Should a deferred part be shown without JavaScript, at the end of the page?

**Yes. Out of place is better than never: today a script-less visitor waits on "Loading…" for good (Q-1141).** It happens on every page with a `defer`. The content is already in the answer, inside a `<template>`.

| Impact | |
|---|---|
| **Functionality** | Without JavaScript each deferred part shows after the rest of the page, and its placeholder is hidden |
| **User** | Reviews at the bottom instead of nowhere; with JavaScript nothing changes |
| **Requests** | Each deferred part is sent twice in the answer (in the template and in `<noscript>`), so the answer grows by the parts' size |
| **Security** | No change: the same HTML, escaped as it is |

What today's way has: an answer that carries each part once.

**My recommendation:** the `<noscript>` copy, with the guide saying where it lands.

**The question:** shall deferred parts carry a `<noscript>` copy? If not, the guide says that `defer`red content needs JavaScript to show, and to avoid `defer` for what a page is for.

### 3. Should `navigate()` while the server renders be a redirect?

**Yes: it is what the code says, and today it sends the page it meant to keep from the visitor (Q-1142).** It happens in every component that guards by `navigate`, on the first request of every visitor: JavaScript on or off, the guarded page is in the HTML.

| Impact | |
|---|---|
| **Functionality** | `navigate("/login")` in a component's body answers `303` to `/login` when the server draws it, before anything is sent; after a `defer` has started, an error in the log |
| **User** | Without JavaScript, the visitor lands on `/login`; with it, no flash of the guarded page |
| **Requests** | A 303 instead of a 200 for those requests |
| **Security** | The guarded markup no longer goes out with the first answer. It is still no guard for data: `server fn`s and routes must check, as the guide says |

What today's way has: `navigate` means the same everywhere, a move of the browser's.

**My recommendation:** the 303, plus the guide line that a guard belongs in the route or the `server fn`.

**The question:** shall `navigate()` during a server render answer 303? If not, `quillo check` warns of a `navigate` in a component's body, and the guide says it does nothing on the server.
