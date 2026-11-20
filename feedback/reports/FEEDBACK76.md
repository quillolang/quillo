# Feedback from studying DOM query and manipulation in Quillo

| | |
|---|---|
| App | None of its own: a study done at the user's request, 2026-11-19: "We need quillo language to support dom query and manipulation, create FEEDBACK76.md for study". Then, the same day: "The qwertime.com have more js. The goal is also to find out what Quillo couldn't do why need js" (Part E: [qwertime.com](../../../qwertime.com) at `a4fdea9`, on Quillo 0.8.5 in production) |
| Quillo | The released `quillo 0.8.8 (f35bf2d, 2026-11-19)` on the PATH (= tag v0.8.8). The guide, `src/client/runtime.js` and `src/check.rs` were read at this checkout (branch `feedback75`, `54d20b9`, which is main), not built: it is Quillo Coder's working copy |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check` on 30 small programs (26 of what a model writes first for the DOM, 4 for Q-1136); `quillo run` and curl; headless Chrome 157 through `feedback/tools/chrome.mjs` (real keys and mouse, listeners counted with `DOMDebugger.getEventListeners`); the plugins `scrape` and `html_rewrite` built and run. For Part E: qwertime's `public/app.js` read line by line (79 features), its other scripts and docs, and four probes on 0.8.8 (a view with handlers, a statement after an update, a feed of 50 posts as views and as components, the worker's push events). The programs are kept in `.git/wip/feedback76/` (`habits/`, `browser/main.qwe` with its driver `drive.mjs`, `plugins/`, `qwertime/`) |

Numbers Q-1130 to Q-1139, from the block Q-1130 to Q-1149. Q-1120 is the highest number used so far, and Q-1121 to Q-1129 are left for Quillo Coder's open work. This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed. The repros are written in the report.

**How the request is read.**

- **DOM query** is finding elements of the page: by id, by CSS selector, all that match, inside one element, and the element an event came from.
- **DOM manipulation** is changing them from code: classes, styles, attributes, text, focus, the caret, scrolling, measuring, adding and moving elements, and listening on `window` and `document`.
- Both happen in the browser, in a component. On the server, the same words mean reading and changing HTML text (a page fetched with `http.get`, a feed, what a visitor wrote). That is covered in Part D.
- **Why a Quillo site still carries JavaScript** is the second question. qwertime.com is the largest Quillo site in production. Each of its scripts was sorted by what it needs, and each need was checked against 0.8.8: possible today, possible only in a component, built in since, or not possible. That is Part E.

What worked well:

- **The apps' own JavaScript can be Quillo now.** The forum, pm, cms and to-do apps each carry a helper file (`app/*/public/js/*.js`, 15 to 67 lines) for focus, the caret, scrolling a chat, a flash, a "/" shortcut, the tab's title and copy buttons. Each of those was written again in Quillo and ran in Chrome with the same result (Part A). Not run: `pm.go` and `forum.go`, which are `navigate(path)`, and `pm.dialogOpen`, one `js("document.querySelector", ...)`. Those files date from FEEDBACK1 to FEEDBACK5, before `page_updated()` and fields on the page's objects existed.
- **What is unsafe is refused, with a reason.** `el.innerHTML = "<b>hi</b>"` stopped with "`innerHTML` is not set from Quillo code: it runs text as code or HTML". A misspelt path gave the error value `js: document.querySelecter is not defined on this page`.
- **Hand-made changes and Quillo's updates mostly get along.** A class added by hand to an element with `class={...}` was kept when the class changed (`cold hand` became `hand hot`). An `<li>` appended by hand to a keyed list stayed, at the end, when the list changed (`z,a,b,c,stray`). Nothing was remade, and no mismatch was counted.
- **Listeners come off.** A `keydown` listener on `document`, added in `@mount` and taken off by the function `@mount` returns, was removed when its component left. Chrome counted 3 listeners mounted, 2 unmounted and 3 mounted again; 2 of them are Quillo's own.
- **Speed is not the problem.** 20,000 writes to an element from a Quillo handler took 9.2 to 10.8 ms through `el.dataset.n` and 11.7 to 13.7 ms through `el.classList.toggle`, against 2.9 to 3.3 ms and 1.6 to 1.8 ms in the page's own JavaScript (three runs). That is 0.5 to 0.7 µs a call from Quillo against 0.1 to 0.15 µs, which a handler that touches a few elements never notices.

## The answer

**Quillo already does DOM query and manipulation: each of the 21 tasks tried (Part A) could be written in Quillo today. But it does them through JavaScript's names in strings, page-wide and unchecked, so where a model goes wrong it is often silent or late. Three first attempts failed: a loop over `querySelectorAll`, `<window onkeydown>`, and a dialog moved to `<body>`, which stays after its component goes.**

- What exists: `@mount={(el) => ...}` gives an element, and its function runs when the element leaves. `js("document.querySelector", sel)` and `js_new(...)` reach the page. An element's fields and methods are fields (`el.value`, `el.focus()`, `el.scrollTop += 100`, `el.classList.add(...)`). `page_updated()` waits until the page shows what was set.
- What goes wrong, measured:
  - Looping over `querySelectorAll` stops the handler (Q-1130).
  - The guide's way to move an element to `<body>` leaves a dead copy on screen when its component goes (Q-1131).
  - `<window onkeydown>` and other unknown tags render, and their handlers never fire (Q-1132).
  - `quillo check` passes what the browser is certain to refuse (Q-1133, Q-1135).
  - The first names a model reaches for get only "unknown name", without the way Quillo does it (Q-1134).
- What is missing by design: a reference to an element without a callback, page events without four lines of add and remove, a query that stays inside the component, and DOM names that `quillo check` and the editor know.

**On the server, nothing is needed in the core.** The catalog's `scrape` and `html_rewrite` plugins query and change HTML by CSS selector. They built in 18 s and gave the right answers.

**Why qwertime.com carries 3,329 lines of JavaScript: mostly not because Quillo cannot do the work, but because its pages are views, and a view can carry no behaviour at all (Part E).** qwertime has 154 views and one component. Of the 71 features in the main part of its `app.js`:

- **63 could be Quillo code today, in a component:** state, handlers, dialogs, timers, server calls, the mascot library through `client ... use`, and the browser's APIs through `js`. That includes the four writes without a reload, as server functions and state. Their forms still post without JavaScript, since the server draws a component's HTML too.
- **3 are built into Quillo since 0.8.7:** the service worker, turning notifications on and off, and unbinding a device at sign-out.
- **2 could be done on the server** as the page is drawn.
- **2 are not built in, but a component can do them through `js`:** the install button (`beforeinstallprompt`), and reloading a page left open when a new version comes.
- **1 was not checked:** sending the device's subscription again each day.

A view can hold no behaviour: a handler or `@mount` written in one is dropped without a word (Q-1137). So with its pages as views, all 65 that a component could do had to be page JavaScript. What Quillo really cannot do:

- run after the page shows a statement's change (Q-1138);
- run on other websites (`embed.js` and the npm package);
- let a program change what its service worker does when a push comes, or handle `pushsubscriptionchange` (Q-1139);
- in-place forms for views.

Moving the feed to components costs, measured on 50 posts: 2.3 times the page's HTML (35,904 bytes against 15,451) and a 144 KB runtime (40.6 KB gzipped), loaded once.

**My recommendation, in this order:**

1. Fix the bugs: Q-1130 (loop over what the page gives), Q-1131 (a moved element goes with its component), Q-1132 (unknown tags warned of), Q-1136 (`markdown` in markup), Q-1137 (a handler in a view at least warned of), Q-1138 (`page_updated()` in a statement), Q-1139 (`pushsubscriptionchange`).
2. Make `quillo check` say what the browser will say: Q-1133 (denied fields, misspelt standard names) and Q-1135 (`js(...)` used while the server renders). Add the hints of Q-1134.
3. Then a small `dom` module (Part B, question 1): `dom.find` and `dom.find_all` inside the component, returning lists; `dom.on` for `window` and `document`, taken off by itself; `dom.focus`, `dom.scroll_to` and `dom.measure` with options. Plus `ref={x}` (question 2). Each of these takes away a pattern that was measured to cost lines or fail.
4. A guide section, "Changing the page by hand", with the table in Part A.3: what lasts through an update and what does not.
5. A guide section, "HTML on the server", that points to the two plugins (Part D).
6. Behaviour for views, question 4: in-place forms first. They would take over the core of qwertime's script (posting, replying, reacting and messages without a reload) with no script of the program's, and keep a plain form post without JavaScript.

## Part A: what works today, measured

All on 0.8.8 in Chrome 157, from `browser/main.qwe` and `browser/drive.mjs`. Results are in `browser/result.json` and `result2.json`.

### A.1 Query

| Task | Written as | Result |
|---|---|---|
| By id | `js("document.getElementById", "reply")` | the element |
| By selector, in the page | `js("document.querySelector", ".name:last-of-type")` | the element |
| Inside the component | `@mount={(el) => { el.querySelector(".mine").textContent = "found mine" }}` | `found mine` |
| The event's element | `onclick={(e) => { let li = e.target.closest("li"); ... }}` | `li` |
| All that match, counted | `len(js("document.querySelectorAll", ".row"))` | `2` |
| All that match, looped | `for row in js("document.querySelectorAll", ".row") { row.classList.toggle("on") }` | **stops: `list has no field classList`** (Q-1130) |
| All that match, looped | `for row in js("Array.from", js("document.querySelectorAll", ".row")) { ... }` | both toggled |
| Another component's element | `js("document.getElementById", "victim").textContent = "poked"` | changed: nothing keeps a component to its own |

### A.2 Manipulation, and the apps' helpers in Quillo

| Task (the app's helper) | In Quillo | Result |
|---|---|---|
| Classes, style, data, text | `el.classList.add("on"); el.style.background = "rgb(1, 2, 3)"; el.dataset.x = "1"; el.textContent = "hand"` | all four set |
| Make a button, listen, add it | `let b = js("document.createElement", "button")`, `b.addEventListener("click", () => {...})`, `el.appendChild(b)` | clicked, handler ran |
| Seen on screen | `js_new("IntersectionObserver", (entries) => { seen = ... })` with `() => o.disconnect()` | `true` |
| Measure | `e.target.getBoundingClientRect()`, then `r.width`, `r.height` | `123x45` |
| The tab's title (`todoApp.showLeft`) | `js("document").title = "(3) Inbox"` in a handler | set |
| Focus the input just added (`pm.focus`) | `names = names + ["new"]`, then `page_updated()`, then `.focus()` on the last | focused `new2`. **Without `page_updated()`, the old last input `Ana` was focused, silently** (documented) |
| Reply box: into view, focus, caret at the end (`forum.focusReply`) | `box.scrollIntoView({ behavior: "smooth", block: "center" })`, `box.focus()`, `box.setSelectionRange(len(box.value), len(box.value))` | focused, caret 14 of 14 |
| Where the caret is (`forum.caretOf`) | `box.selectionStart` | `14` |
| Newest message in view (`forum.scrollChat`) | `list.scrollTop = list.scrollHeight` after `page_updated()` | 92 = 152 − 60, at the bottom |
| Flash a block (`cms.reveal`) | `classList.remove("flash")`, `let _w = el.offsetWidth`, `classList.add("flash")` | `flash` (the read must be named `_w`, or `quillo check` warns it is unused) |
| "/" anywhere goes to search (forum, to-do) | `js("document.addEventListener", "keydown", f)` in `@mount`, with `() => js("document.removeEventListener", "keydown", f)` | worked, and the listener was removed when the component went (3, 2, 3) |
| Copy buttons on a post's markdown, live ones too (forum) | `js("Array.from", root.querySelectorAll("pre:not([data-copy])"))`, make and append a button, `js_new("MutationObserver", () => add(root))` | a button on the first `<pre>`, and on one added later |
| Click outside closes a menu | `document` click listener with `!el.contains(e.target)` | stays open on a click inside, closes on one outside |
| Move a dialog to `<body>` | `js("document.body.appendChild", el)` in `@mount` | moved, and it updates there. **When its component went, it stayed on screen, dead** (Q-1131) |
| Keys on the window | `<window onkeydown={(e) => { k = e.key }}/>` | **nothing: an unknown `<window>` element** (Q-1132) |

### A.3 Changing the page by hand: what lasts through Quillo's next update

Measured with `Clash` and `Victim` in `browser/main.qwe`. No guide section says this today.

| Changed by hand | Quillo then updates | What the page shows |
|---|---|---|
| `el.classList.add("hand")` on `<p class={hot ? "hot" : "cold"}>` | `hot` changes | `hand hot`: the hand-added class is kept |
| `el.textContent = "by hand"` on `<p>{label}</p>` | `label` changes | `two`: Quillo's text replaces it |
| An `<li>` appended to `<ul>{for i in items => <li key={i}>...}</ul>` | items added at both ends | `z,a,b,c,stray`: the stray item is kept, last |
| Another component's `{v}`, set to `poked` | that component sets `v` | `set again`: its own update replaces it |

LiveView makes the same promise for its `JS` commands ("DOM-patch aware": classes and attributes set by them survive the server's patches). Quillo already keeps hand-added classes, but it has never said so.

## Part B: what a `dom` module would add

### B.1 What others do (from their documentation, not run here)

| | Reference to an element | Query | Page events | After an update | Move elsewhere |
|---|---|---|---|---|---|
| React 19 | `useRef` + `ref={r}`; a ref callback may return its cleanup | none (refs only) | `useEffect` + add/remove | `flushSync`, layout effect | `createPortal` |
| Svelte 5 | `bind:this={el}`; `{@attach f}` | none | `<svelte:window onkeydown>`, `<svelte:document>`, `<svelte:body>` | `await tick()` | none built in |
| Vue 3.5 | `ref="box"` + `useTemplateRef("box")` | none | `onMounted` + add/remove | `await nextTick()` | `<Teleport to="body">` |
| Solid | `let el; <div ref={el}/>` | none | `onMount` + `onCleanup` | none needed | `<Portal>` |
| Alpine | `x-ref` + `$refs.box` | `$el` | `@keydown.window`, `@click.outside` | `$nextTick` | `x-teleport` |
| Elm | by id only, in `Browser.Dom`: `focus`, `blur`, `getElement`, `getViewportOf`, `setViewportOf` | none | subscriptions (`Browser.Events.onKeyDown`) | none needed | none |
| LiveView | `phx-hook` (`mounted`, `updated`, `destroyed`); `JS.add_class`, `JS.toggle`, `JS.focus`, `JS.show` on elements by selector | none | `phx-window-keydown`, `phx-click-away` | the hook's `updated` | none |
| Quillo 0.8.8 | `@mount={(el) => { box = el }}` with `state box = null` | `js("document.querySelector", ...)` | `@mount` + `js("document.addEventListener", ...)` + its remove | `page_updated()` | `js("document.body.appendChild", el)` |

Three things stand out:

- None of the component frameworks has a query: they reach elements by reference. Quillo has no reference without a callback, so its programs query the page instead, by id or class. That reaches other components' elements too.
- Page events are markup in Svelte, Alpine and LiveView. Quillo needs four lines.
- Elm, the strictest about the DOM, still gives a short, safe set: focus, blur, an element's box and scrolling. These are the tasks the apps' helpers do.

### B.2 A proposal

A `dom` module, compiled for the browser only. It uses the same checks as `js` (no `innerHTML`, no text run as code), and reaches only what it is called with.

```quillo
component Thread(msgs: list) {
    state text = ""
    dom.on("document", "keydown", (e) => {              // taken off when the component goes
        if e.key == "/" && !dom.typing() { e.preventDefault(); dom.focus(dom.find(".reply")) }
    })
    <div>
        <div class="chat">{for m in msgs => <p>{m}</p>}</div>
        <textarea class="reply" value={text}></textarea>
        <button onclick={() => {
            text = text + "\n> quoted"
            page_updated()
            let box = dom.find(".reply")                // inside this component only
            dom.scroll_to(box, { center: true, smooth: true })
            dom.focus(box, { caret: "end" })
            for p in dom.find_all(".chat p") { p.classList.remove("new") }   // a list
        }}>Quote</button>
    </div>
}
```

| Call | Gives | Replaces | Why it earns its place |
|---|---|---|---|
| `dom.find(sel)`, `dom.find_all(sel)` | an element or null; a list | `js("document.querySelector", ...)`, `js("Array.from", ...)` | Stays inside the calling component's own elements, so one component cannot reach another's by a shared class. Returns a list, so `for` works (Q-1130). |
| `dom.find_in(el, sel)`, `dom.find_all_in(el, sel)` | the same, inside `el` | `el.querySelector(...)` | For markup the program did not write as elements (`raw`, `markdown`): the copy buttons. |
| `dom.page_find(sel)`, `dom.page_find_all(sel)` | the same, in the whole page | `js("document.querySelector", ...)` | Reaching outside is still possible, but written as such. |
| `dom.on(target, event, f)` | null | `@mount` + `addEventListener` + the remove | `target` is `"window"`, `"document"` or an element. It starts and stops with the component, as `every` does, and inside an `if` it follows the condition. |
| `dom.focus(el, { caret })`, `dom.scroll_to(el, { center, smooth })`, `dom.scroll_end(el)` | null | three to five lines each in the helpers | Elm's set. `caret: "end"` or a number. |
| `dom.measure(el)` | `{ x, y, width, height }` | `getBoundingClientRect()` | A map, not a page object, so it can go in state. |
| `dom.typing()` | bool | `["INPUT", "TEXTAREA", "SELECT"].contains(js("document.activeElement").tagName)` | Both shortcut helpers write it, and both miss `contenteditable`. |

Rules:

- **Where it can be called.** A `dom.*` call works in `@mount`, in handlers, and in statements (which run in the browser). While the server renders, `dom.find` gives null and `dom.on` does nothing, so the same component renders on both sides. A field set or method call on what it gives, at a component's top level, is refused by `quillo check`, as Q-1135 asks for `js(...)`.
- **What `quillo check` knows.** Every name, its arguments and what it gives, so a misspelling is an error at check time, not a runtime error value.
- **What stays as it is.** `js` and `js_new` stay for libraries and anything not in the module.
- **Size.** The module goes into the runtime bundle only when a page uses it, as the runtime's other parts do. Pages without it do not grow.
- **Speed.** Measured on today's checks: about 0.5 µs a call. The module only adds work where the scope is checked (a `contains` per match), and that cost should be measured before and after, as the rules require.

### B.3 `ref={x}`

Today `<input ref={box}/>` gets the warning "`ref` is React's, and does nothing in a page", and every reference is three lines:

```quillo
state box = null
<input onmount={(el) => { box = el }}/>
// and nothing sets box back to null when the input goes
```

With `ref={box}`, Quillo would set `box` to the element when it is placed and back to null when it leaves. `box` is declared with `state box = null`, as today. This is what React and Solid write, and `quillo check` already finds every case. Of the 26 habit programs, 2 wrote `ref` (one with `useRef`) and a third Svelte's `bind:this`, for the same task. The checker would type `box` as an element or null, so `box.focus()` with no null check could be warned of.

## Part C: page events

`<window onkeydown>` is what Svelte writes (`<svelte:window>`), and it is what a model wrote here. Today it renders `<window></window>` and nothing fires (Q-1132). There are three ways out (question 3):

- `quillo check` warns of any tag that is not HTML, SVG, MathML or a custom element with a hyphen. This is needed in every case.
- `<window on...>` and `<document on...>` become special elements, as in Svelte. They render nothing, and their listeners start and stop with the component.
- `dom.on("window", ...)` only (Part B), with the warning pointing to it.

## Part D: on the server

Built and run (`plugins/server_dom.qwe`). The first build took 18 s, and later starts use the cached build.

| Call | Gave |
|---|---|
| `select_text(page, "h2.t")?` | `["Routes","Components"]` |
| `select_attr(page, "a", "href")?` | `["/docs/","https://github.com/quillolang"]` |
| `select_html(page, "article p")?` | the paragraph's inner HTML |
| `html_rewrite(page, [{ selector: "script", remove: true }, { selector: "a[href^=http]", set: { rel: "nofollow noopener", target: "_blank" } }, { selector: "h2", set: { class: "title" } }])?` | the script gone, the outside link marked, both `h2` given the class |
| `select_text(page, "p:nth-child(2n+1 x")` | an error value: `` `p:nth-child(2n+1 x` is not a selector `` |

`html_rewrite` also takes `remove_attributes`, `unwrap`, `text`, `html` and `append_html`, and `html_absolute_links` and `html_external_links` do the common jobs in one call. That covers querying and changing HTML text by selector. It does not cover walking a tree (parent, siblings) or querying one of Quillo's own `html` values without turning it into text, which only tests would want. No core work is proposed. The guide should point to both plugins, in a section "HTML on the server".

## Part E: qwertime.com's JavaScript, and what Quillo could not do

### E.1 What is there

qwertime.com at `a4fdea9` runs Quillo 0.8.5 in production on six servers. It has 154 views and one component (`Room`, `ui/rooms.qwe`). Its own scripts:

| File | Lines | Runs | What it does |
|---|---|---|---|
| `public/app.js` | 2,584 (163 KB, 49 KB gzipped; sent minified) | every page | 79 features (E.3), 144 `addEventListener` calls: 37 on `document`, 19 on `window`, 49 on elements the server drew, 37 on elements it builds |
| `public/gif.js` | 249 | the first time a GIF is made | GIFs of a mascot's gestures: each part's computed style copied at each moment of the animation, painted on a canvas, 256 colours, written as a GIF; packs as a .zip |
| `public/qr.js` | 187 | the promo pages | QR codes as SVG, written by hand (Reed-Solomon, mask choice) |
| `public/sw.js` | 87 | the service worker | the offline page; a push that carries nothing and asks `GET /push/latest`; the app's badge; a click focusing an open window; `pushsubscriptionchange` |
| `public/embed.js` | 38 | members' own websites | their mascot in a corner of the page, in an iframe |
| `public/embed-frame.js` | 19 | inside that iframe | a gesture now and then; its × tells the page around it |
| `packages/qwertime-mascots/index.js` | 165 | others' websites, as an ES module | hired mascots switched by page or on a timer |

The `Room` component also calls the page five times: `js("qtRooms.zone")`, `js("qtRooms.down", ...)`, `js("qtRooms.guard", ...)`, `js("qtRooms.toEnd")` and `js("navigator.clipboard.writeText", ...)`.

qwertime's ARCHITECTURE.md gives its reason: "Pages are drawn on the server (Quillo views, plain HTML). In the browser, `mascot.js` draws the mascots (SVG) and `app.js` adds what needs a script: live updates, reacting and posting without a reload, the photo viewer, the mascots' life on the page." And: "Without JavaScript, each is a plain form post and the page comes back." No comment in `app.js` or commit to it names Quillo, a FEEDBACK or a workaround for Quillo. Its workarounds are for browsers: iOS (lines 429-438, 2573-2575), Instagram (167-169), and a dialog focusing its first button (2298).

### E.2 The root: a view runs nothing in the browser (Q-1137)

A handler or `@mount` on an element of a view drawn by a route passes `quillo check`, and the page goes out without it, and without a script:

```quillo
view Post(id: int) = <article>
    <p>post {id}</p>
    <button onclick={() => print("clicked")}>react</button>
    <div onmount={(el) => { el.dataset.seen = "1" }}>m</div>
</article>
route GET / = <html><body><Post id={1}/></body></html>
```

```
<!doctype html><html><body><article><p>post 1</p><button>react</button><div>m</div></article></body></html>
```

So a Quillo program has two ways to make server-drawn markup do anything: turn it into a component, or write JavaScript beside it. qwertime took the second for everything but its chat room. The first costs this much, measured on 0.8.8 with a feed of 50 posts, each with two reaction buttons (`qwertime/feed.qwe`):

| | HTML | gzipped | Scripts |
|---|---|---|---|
| 50 views | 15,451 B | 1,016 B | none |
| 50 components (state for the counts, a `server fn` to react, the form kept for no JavaScript) | 35,904 B | 3,627 B | runtime 144,433 B (40,636 gzipped) + page bundle 2,348 B, each loaded once and kept by the browser |

The components' page keeps its forms as plain forms with `action` (`<form method="post" action="/p/0/react">`), so without JavaScript they post as any form does. Only the handlers need the script.

### E.3 Each feature of `app.js`, by what it needs

The 71 features of the main part (lines 26-2440) are listed by number in the inventory kept at `.git/wip/feedback76/qwertime/app-js-inventory.md`. Each is counted once, under what it needs most. The 8 of the chat room (lines 2442-2584) follow in E.4.

| What it needs | Features (inventory rows) | Count | In Quillo 0.8.8 |
|---|---|---|---|
| The mascot library on timers, observers and pointer events: random gestures, life in a post, hiding while idle, entrance, the up-close view, GIFs, share cards, rare and wandering mascots, the editor | 1, 7, 12, 29-33, 39, 49, 50, 52, 56-59, 61, 62, 67-70 | 22 | **In a component:** `client "..." use Mascot`, `every`, `js_new("IntersectionObserver", ...)`, pointer handlers. The library draws in a shadow root, which `el.shadowRoot` reaches. Not in a view (Q-1137) |
| Page logic: state, handlers, element fields, dialogs, timers, a server call (FAQ, post menus, search box, confirm dialog, menu sheet, composer counter, read more, photo viewer, waiting for a device's yes, undo in the editor) | 2-4, 9, 11, 22, 24-26, 34, 35, 37, 41, 48, 51, 53, 54, 64-66, 71 | 21 | **In a component**, with no `js` at all (`ask` for the confirm dialog, `el.showModal()` for a sheet). Not in a view |
| The browser's APIs: signature pad and cropper (pointer capture, a wheel that is not passive), the time zone, the share sheet, the clipboard, shrinking photos on a canvas and putting them back in the file input (`DataTransfer`), arrival stats and post views (`sendBeacon` as the page goes, `performance` navigation type, `IntersectionObserver`), history back, `elementsFromPoint` | 5, 6, 8, 10, 13, 23, 27, 28, 36, 38, 40, 46, 60 | 13 | **In a component, through `js` and `js_new`**, as Part A's tasks of the same kinds ran. A listener on `window` for `pagehide` is four lines of `@mount` until `dom.on` |
| PWA and push | 14-20 | 7 | **Built in since 0.8.7:** the worker (14), turning notifications on and off (`notification.ask`, `stop`, `state`; 17), a device unbound at sign-out (`end_session()`; 20). The offer banner (18) is component code over `notification.state`. **Not built in, but reachable through `js` in a component:** reloading a page left open when a new version comes (15: the built-in worker updates at the browser's own checks, on a navigation and once a day, and reloads no open page) and the install button (`beforeinstallprompt`, 16). Not checked: sending the subscription again each day (19) |
| Writing without a reload: react, post, reply, and a conversation's messages, each answered by the server's HTML put in place | 42-45 | 4 | **In a component**, as `server fn`s and state, the form left for no JavaScript. **Not for a view:** nothing posts a view's form in place and puts the server's answer in the page (question 4) |
| Live: badges, the tab's title, "N new posts", new messages (EventSource on `/live`) | 63 | 1 | **In a component:** `live()`, `listen(url, f)`, channels. Not for a view |
| Done on the server as the page is drawn | 47 (links to other sites get `target` and `rel`; app.js does it at the click, for links that arrive later), 55 (the theme switch's way back, which the request already knows) | 2 | **On the server**, no script |
| A browser's bug | 21 (iOS leaves the tab bar mid-screen) | 1 | **In a component**, through `js("visualViewport")` and listeners |

In all: 65 in a component (2 of them only through `js`, as nothing is built in), 3 built in, 2 on the server, 1 not checked. None of the 71 needs JavaScript that a component could not hold. They are JavaScript because the markup is a view's.

### E.4 The chat room: a component that still calls the page

| What `app.js` does for `Room` | Why it is JavaScript | In Quillo 0.8.8 |
|---|---|---|
| `qtRooms.zone()`: the browser's time zone | a one-line helper | the guide's recipe, `js("Intl.DateTimeFormat", []).resolvedOptions().timeZone`, in the component |
| `qtRooms.down()`: the newest line kept in sight, unless one has scrolled up | runs on the next frame (`requestAnimationFrame`), after the page shows the new line; called from a statement | **`page_updated()` in the statement does not wait: one line short, silently** (Q-1138). `run_after(0, ...)` in the statement works (measured: 126 of 126) |
| `qtRooms.toEnd()`: the caret at the end of the box after a name is tapped | the next frame | `page_updated()` in the handler, then `setSelectionRange`: measured in Part A |
| `qtRooms.guard(on)`: the host asked before leaving (`beforeunload`, and links caught in a dialog) | a page listener switched on and off by state | `@mount` listeners that read the state each time they fire; `dom.on` inside an `if` would follow it (Part B) |
| Up and Down through what one sent; Enter sends | key handling on the box | `onkeydown` with `selectionStart` in the component |
| "New lines ↓" and following the foot | a scroll listener | `onscroll` in the component |
| A colour of one's own ticks its choice | the input is in `ColorPick`, a view the server draws, not in the component | **not in a view** (Q-1137) |
| The room's height between the heading and the tab bar (`ResizeObserver`), and above a phone's keyboard (`visualViewport`) | CSS variables set from measures | `js_new("ResizeObserver", ...)`, `el.style.setProperty(...)` |

### E.5 The other scripts

| Script | Could Quillo do it? |
|---|---|
| `embed.js`, `packages/qwertime-mascots` | **No.** They run on other people's websites, which do not run Quillo. Quillo cannot build a component into a script, or a custom element, for another site |
| `embed-frame.js` | **Yes:** it runs inside a qwertime page (`/embed/<handle>`). A component with `every` and `js("window.parent.postMessage", ...)` does it |
| `sw.js` | **Mostly built in since 0.8.7** (the offline page, push, the badge, a click bringing an open window). Not built in: a push that carries nothing and asks the server what is new as it comes, which also lets it show nothing when the member has read it elsewhere. Web Push is encrypted end to end, so the built-in message hides its words from the push service as well. And `pushsubscriptionchange`, which Quillo's worker never handles (Q-1139) |
| `qr.js` | **Yes, on the server:** the catalog's `qr` plugin draws QR codes as SVG, so the promo page can carry the picture and no script |
| `gif.js` | **Partly.** The frames must be taken in the browser, from the library's animation as drawn. The GIF could be written by the catalog's `gif` plugin on the server from frames the page sends, or in a component. A GIF encoder written in Quillo and compiled for the browser was not measured |

## New issues

### Q-1130. Looping over a NodeList gives index and element pairs, so the loop's first field read stops the handler

| Kind | Detected by | Attempts |
|---|---|---|
| silent to the visitor (an error in the console only); `quillo check` passes | browser | 1 |

`iter` in the runtime takes a list, a map, text or null, and `isMap` is true for any object from the page that is not an array or an event. So a NodeList is looped as a map of its own keys, and each item is `[index, element]`. `len()` of the same NodeList is right (2), so the two disagree. Read from the code, not tried: an `HTMLCollection` goes the same way, and a page `Map` or `Set`, which has no own keys, loops zero times without a word.

```quillo
component Rows() {
    <button onclick={() => {
        for row in js("document.querySelectorAll", ".row") { row.classList.toggle("on") }
    }}>Pick</button>
}
route GET / = <html><body><p class="row">a</p><p class="row">b</p><Rows/></body></html>
```

The console shows `quillo: list has no field classList` and nothing changes. The UI kit writes `js("Array.from", ...)` around it, twice (`app/stays/ui/kit/carousel.qwe:108`, `select.qwe:85`).

| Impact | |
|---|---|
| **Functionality** | A loop over `querySelectorAll` stops at its first item. By the code, so do `children` and `getElementsByClassName`, and a page `Map` or `Set` loops zero times |
| **User** | The visitor sees a button that does nothing. The developer sees a console error about a list, where they looped over elements |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** `iter` takes what the page gives (a foreign object with `Symbol.iterator`, or array-like with `length`) as a list of its items, as `Array.from` would. A page `Map` then gives `[key, value]` pairs, as a Quillo map does. `len()` already agrees.

### Q-1131. An element moved out of its component stays on the page, dead, after the component goes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

The guide says "There are no portals and no refs: ... `js("document.body.appendChild", el)` moves one." Moved, the dialog still updates (its counter went to 1). When its parent stopped showing it, the dialog stayed in `<body>` with `n=1`, and its button no longer did anything.

```quillo
component Dialog() {
    state n = 0
    <div id="dlg" onmount={(el) => { js("document.body.appendChild", el) }}>
        <span>{n}</span><button onclick={() => { n += 1 }}>inc</button>
    </div>
}
component Host() {
    state open = true
    <div><button onclick={() => { open = false }}>close</button>{open && <Dialog/>}</div>
}
```

| Impact | |
|---|---|
| **Functionality** | A modal, toast or menu moved to `<body>` is never removed. Each opening leaves another copy |
| **User** | The visitor sees a closed dialog still on screen, and its buttons are dead |
| **Requests** | No change |
| **Security** | No change: the copy runs no handlers. A plain `<form method="post">` in it would still submit as HTML does (not tried) |

**Suggestion:** when a component goes, the runtime removes the elements it made wherever they are now, as it already removes their listeners and timers. Until then, the guide's line should add the cleanup: `@mount={(el) => { js("document.body.appendChild", el); () => el.remove() }}`.

### Q-1132. `<window onkeydown>`, `<document onclick>` and other unknown tags pass `quillo check`, and their handlers never fire

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser, then `quillo check` | 1 |

```quillo
component Keys() {
    state k = ""
    <div><window onkeydown={(e) => { k = e.key }}/><p>{k}</p></div>
}
```

The server sends `<window></window>`, and a key pressed in the page sets nothing. `<foo onclick>`, `<document onclick/>` and `<body2/>` pass `quillo check` too.

| Impact | |
|---|---|
| **Functionality** | A page shortcut written as Svelte writes it never works |
| **User** | The developer gets no word, and a model that knows Svelte writes it first |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** `quillo check` warns of a lowercase tag that is not HTML, SVG or MathML and has no hyphen (custom elements need one): "`<window>` is not an HTML element, and its handlers never fire; for keys on the window, write ... (Part C)". Whether `<window>` and `<document>` become real is question 3.

### Q-1133. `quillo check` passes what the browser is certain to refuse: `innerHTML` set on an element, and a misspelt name of `document`

| Kind | Detected by | Attempts |
|---|---|---|
| loud, but only in the browser's console | browser | 1 |

```quillo
component C() {
    <div onmount={(el) => { el.innerHTML = "<b>hi</b>" }}>box</div>
    <button onclick={() => { js("document.querySelecter", "#name").focus() }}>Go</button>
}
```

In Chrome: "`innerHTML` is not set from Quillo code: it runs text as code or HTML", and "error has no method `focus`" after the error value `js: document.querySelecter is not defined on this page`. `quillo check` says `ok` to both. The field names the runtime refuses are a fixed list (`innerHTML`, `outerHTML`, `srcdoc`, `on...`), and `js_path_problem` in `src/client.rs` already refuses `eval` and the rest at check time.

| Impact | |
|---|---|
| **Functionality** | The `@mount` or handler stops at that line, in the browser |
| **User** | The developer learns of it only by opening the page and its console. A model that does not run Chrome never does |
| **Requests** | No change |
| **Security** | No change: the runtime refuses it either way. The check only makes it sooner |

**Suggestion:** `quillo check` refuses a set of a literal `innerHTML`, `outerHTML`, `srcdoc` or `on...` field on any value that is not a map, with the runtime's words. For `js("document.x")`, `js("window.x")`, `js("navigator.x")` and `js("localStorage.x")`, it warns when `x` is not a member of the standard object (the names in TypeScript's `lib.dom.d.ts`), with the nearest one: "did you mean `querySelector`?". Libraries' paths stay unchecked.

### Q-1134. The first names a model writes get "unknown name", without the way Quillo does it

| Kind | Detected by | Attempts |
|---|---|---|
| docs (error messages) | `quillo check` | 1 each |

| Written first | `quillo check` says | What it could say |
|---|---|---|
| `document.querySelector("#name").focus()` | `unknown name document` | the page's `document` is `js("document.querySelector", "#name")`; an element of this component is given to `@mount` |
| `window.addEventListener("resize", ...)` | `unknown name window` | the same, with `js("window.addEventListener", ...)` in `@mount` and its remove in the function it returns |
| `tick()`, `nextTick(...)`, `requestAnimationFrame(...)` | `unknown name tick` (and the others) | `page_updated()` waits until the page shows what was set |
| `<Portal to="body">` | `unknown name Portal` | the recipe of Q-1131 |
| `<input bind:this={box}/>` | `bind:this is not something a person changes in <input> (see bind in the guide: value, checked, ...)` | `bind:this` is Svelte's: write `@mount={(el) => ...}`, which is given the element (as `ref` already says) |
| `$("#name").addClass("on")` | `unexpected character $` | no change needed: the rest of the line says enough |
| `useRef`, `useEffect` | `unknown name useRef` | the guide's table "Coming from other frameworks" has both; name it |

`ref={box}` already gets "`ref` is React's, and does nothing in a page: write `@mount={(el) => ...}`". That is the model for the others.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | One check instead of a search through the guide, for each of the most common DOM names |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** add the hints above, next to the existing `ref` hint.

### Q-1135. Setting a field of `js(...)` at a component's top level passes `quillo check`, and the server answers 500

| Kind | Detected by | Attempts |
|---|---|---|
| loud (a 500 for every request of the page) | curl | 1 |

```quillo
component C(n: int) {
    let d = js("document")
    d.title = `(${n}) Inbox`
    <p>{n}</p>
}
route GET / = <html><body><C n={3}/></body></html>
```

`quillo check`: `ok`. `GET /`: `500`, with `null has no field title to set: only the page's objects (an element, an event) do, in a component`. The guide says `js` gives null while a component renders on the server, so this can never work on the server.

| Impact | |
|---|---|
| **Functionality** | The page fails for everyone |
| **User** | Visitors get "internal error"; the developer finds out on the first request |
| **Requests** | Every request of the page is a 500 |
| **Security** | No change |

**Suggestion:** `quillo check` refuses a field set or a method call on what `js(...)` or `js_new(...)` gives, at a component's top level outside `@mount`, handlers and `let` functions: "this runs on the server too, where `js` gives null: put it in `@mount`". For the title itself, `dom` could have `dom.title(text)`, or the guide could say that `js("document").title = ...` goes in `@mount` or a handler.

### Q-1136. `markdown(...)` in a component's markup passes `quillo check`, `quillo run` refuses to start, and the fix it suggests fails each request

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo run` | 3 |

Not about the DOM: met while writing the copy-button probe.

```quillo
component P(md: str) {
    <div>{raw(markdown(md))}</div>
}
route GET / = <html><body><P md="x"/></body></html>
```

- `quillo check`: `ok (1 route, 0 functions, 1 component)`.
- `quillo run`: does not start, with "`markdown` waits on the server, so a component can only call it from an event handler (@click={() => { ... }}) or to set state: state x = markdown(...)".
- Following that, `state body = markdown(md)`: it starts, and each request is a 500, "state `body` of <P> is html, which cannot be sent to the browser".
- Passing it as a prop, `<P body={markdown("**x**")}/>` with `body: html`: a 500 too, "prop `body` of <P> is html, which cannot be sent to the browser".

The guide's own way works: `state html = str(markdown(text))` and `{raw(html)}` (Markdown, under the standard library). The message leaves out the `str(...)`.

| Impact | |
|---|---|
| **Functionality** | No change once written the guide's way |
| **User** | Three tries, and two different errors, to show markdown in a component. A model that trusts the message goes from a refused start to a 500 |
| **Requests** | Every request of that page is a 500 if the message is followed |
| **Security** | No change |

**Suggestion:** `quillo check` runs the same test as the browser compile does, so it reports the line `quillo run` refuses. The message says `state x = str(markdown(...))`, then `{raw(x)}`, as the guide does.

### Q-1137. A handler or `@mount` on a view's element is dropped without a word when the view is not inside a component

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl, reading qwertime | 1 |

The repro is in Part E.2. `quillo check` says `ok (1 route, 1 function, 0 components)`, and the page is sent without the handler, the `@mount`, or a script. The same view shown by a component does run them, since a view a component uses is compiled for the browser. Nothing in the guide says which, and nothing says why the button does nothing.

| Impact | |
|---|---|
| **Functionality** | Every handler in markup that only routes draw does nothing. A site that keeps its pages as views, as qwertime does for plain HTML, has no Quillo place for behaviour: its 65 features that a component could hold are a 2,584-line script |
| **User** | The developer sees a page that looks right and a button that does nothing, with no message anywhere. A model writes it first: it is how a component's markup is written |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** at least, `quillo check` and the load warn of a handler or `@mount` in a view that no component shows: "`onclick` in <Post> does nothing: <Post> is drawn only on the server (route GET /). Make it a component, or ...". Beyond that, question 4.

### Q-1138. `page_updated()` in a component's statement does not wait for the page, so a chat kept at its foot stops one line short

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 2 |

The way qwertime's room keeps its newest line in sight, written in Quillo: a statement that follows `lines`, waits for the page, then scrolls.

```quillo
component A() {
    state lines = ["1", "2", "3"]
    if len(lines) > 0 {
        page_updated()
        let box = js("document.getElementById", "a")
        if box != null { box.scrollTop = box.scrollHeight }
    }
    <div><div id="a" style="height: 60px; overflow: auto">{for l in lines => <p>{l}</p>}</div>
        <button onclick={() => { lines = lines + [`${len(lines) + 1}`] }}>add</button></div>
}
```

After two lines are added, the box is at 92 of 126: where the foot was before the last line showed. The same statement with `run_after(0, () => { ... })` instead of `page_updated()` is at 126 of 126. The first try, without `if box != null`, made the server answer 500 (Q-1135 again).

| Impact | |
|---|---|
| **Functionality** | Whatever a statement does with the page after `page_updated()` (scroll, measure, focus) sees the page before its change |
| **User** | The visitor sees the newest message hidden under the fold, each time one comes. The developer gets no message, and the guide says `page_updated()` "waits until the page shows what was just set" |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** `page_updated()` in a statement waits, as in a handler. Or, where it cannot, `quillo check` refuses it there and names `run_after(0, ...)`. The guide's "Components" says which works where.

### Q-1139. Quillo's service worker does not handle `pushsubscriptionchange`, so a device whose subscription the push service renews stops getting notifications

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading `src/client/sw.js` against qwertime's `public/sw.js` | 1 |

`pushsubscriptionchange` is the event a browser sends to the worker when the push service gives the device a new address (Firefox, when a subscription expires or its keys change). qwertime's `sw.js` subscribes again with the same key and tells the server (`POST /push/subscribe`). Neither `src/client/sw.js` nor `src/client/sw-push.js` names it, nor does the guide (0 matches for `subscriptionchange` in `src/` and `USERGUIDE.md`). Not run: the event cannot be made to fire on demand in Chrome.

| Impact | |
|---|---|
| **Functionality** | The server keeps sending to the old address, which the push service answers as gone. The device has no subscription until the visitor turns notifications on again |
| **User** | Notifications stop on that device without a word |
| **Requests** | Sends to a dead address, until the server drops it |
| **Security** | No change |

**Suggestion:** Quillo's worker handles `pushsubscriptionchange`. It subscribes again with the server's key (the old subscription's `applicationServerKey`, else `GET /_quillo/push`) and binds the new one with `POST /_quillo/push`, as a page does. The worker has no session of its own, so it either sends the old endpoint so that the server can move its binding, or the server keeps the binding by endpoint.

## Where the AI struggled

The 26 programs `habits/01` to `25` (and `19b`) are what a model writes first for each task, from React, Svelte, Vue, jQuery and plain JavaScript habits. One check each.

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `document.querySelector("#name").focus()` | JavaScript | `unknown name document` (Q-1134) | 1 check, then the guide |
| `<input ref={box}/>` | React, Solid | the warning names `@mount`, and `null has no method focus` | 1 check, fixed from the message |
| `<input bind:this={box}/>` | Svelte | an error that lists the `bind:`s, not `@mount` (Q-1134) | 1 check, then the guide |
| `for row in js("document.querySelectorAll", ".row")` | JavaScript's `for ... of` | check passes; in the page, `list has no field classList` (Q-1130) | 1 browser run, and the console |
| `<window onkeydown={...}/>` | Svelte | check passes; nothing fires (Q-1132) | 1 browser run, no message at all |
| `tick()` / `nextTick()` / `requestAnimationFrame()` | Svelte / Vue / JavaScript | `unknown name` (Q-1134) | 1 check, then the guide (`page_updated`) |
| `let d = js("document"); d.title = ...` at the top of a component | "it's the browser's" | check passes; 500 on the server (Q-1135) | 1 request |
| `{raw(markdown(md))}` in a component | it works in a view | check passes; the run refuses, then its advice gives a 500 (Q-1136) | 3 runs |
| `let w = el.offsetWidth` to restart an animation | JavaScript's reflow trick | warning: unused variable `w` | renamed `_w` |
| `pre.textContent` for a copy button's text | forum.js does the same | the copied text ends in "Copy", the button's own label | mine, not Quillo's: read the `<code>` |
| `onclick` and `@mount` in a view | as a component is written | check passes; nothing in the page runs (Q-1137) | 1 curl, then reading the HTML |
| `page_updated()` then scroll, in a statement | as in a handler | the server answered 500 (`js` is null there), then, guarded, one line short in the browser (Q-1138) | 2 runs; `run_after(0, ...)` worked |

## Limitations

- **No reference without a callback.** `@mount={(el) => { box = el }}` with `state box = null`. Nothing sets `box` back to null when the element goes (B.3).
- **No query inside the component.** `js("document.querySelector", ...)` reaches the whole page. `el.querySelector(...)` on an element from `@mount` stays inside it, but needs that element first.
- **No page events in markup.** `@mount`, add, and the function that removes it: four lines, measured to work.
- **No portals.** The recipe leaks (Q-1131).
- **What lasts through an update is not written down** (Part A.3).
- **`js` reaches globals by name**, so a page script that sets `window.pm` is the way an app shares JavaScript. That goes against the rule that JavaScript stays off `window`; a `client /assets/pm.mjs use pm` module already avoids it. qwertime's room does the same with `window.qtRooms`.
- **A view runs nothing in the browser** (Q-1137), and there is no in-between: markup is a component, with its state in the page and the runtime loaded, or it is plain HTML.
- **No script for other websites.** Quillo cannot build a component into a script or a custom element that another site loads (qwertime's `embed.js` and its npm package).
- **The service worker is Quillo's.** A program cannot change what happens when a push comes, for example asking the server what is new and showing nothing when it was read elsewhere, as qwertime's does.
- **Not built in, reachable through `js`:** the install prompt (`beforeinstallprompt`), and reloading a page left open when a new version of the worker comes.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `js_new("IntersectionObserver", f)` with a Quillo function setting state | `true` when on screen, observer disconnected by the returned function |
| `js_new("MutationObserver", ...)` over markup made by `markdown` | buttons added to a `<pre>` appended later |
| A Quillo function handed to `addEventListener` and then to `removeEventListener` | the same listener found and removed (3, 2, 3) |
| `e.preventDefault()`, `e.key`, `e.target.closest(...)` in a handler | as in JavaScript |
| A class added by hand, then `class={...}` changes | both kept |
| A child appended by hand to a keyed list, then the list changes | kept, last, no remake, no mismatch |
| `getBoundingClientRect()` | its fields read as fields (`123x45`) |
| A misspelt `js` path | an error value naming the path, not an exception |
| `innerHTML` set from Quillo | refused with its reason |
| 20,000 guarded writes in one handler | 9 to 14 ms, about 0.5 µs each |
| `select_text` with a broken selector (plugin) | an error value naming the selector |
| `run_after(0, ...)` in a statement, scrolling a list to its foot | at the foot after each line added (126 of 126) |
| A component's form with `action`, and a handler that calls `e.preventDefault()` | the server sends `<form method="post" action="/p/0/react"><button name="kind" value="like">`, a plain form that posts without JavaScript (the HTML checked; not posted with JavaScript off) |
| `end_session()` and push (read in the guide) | every device subscribed in that session unbound, which qwertime's logout does by hand |

## Questions for the user

### 1. Should Quillo have a `dom` module for the browser?

**Yes, a small one. Everything in it works today through `js`, so the module adds no new power. What it changes is where mistakes surface: at check time instead of in the console, and inside the component instead of across the page.** It matters each time a component focuses, scrolls, measures or listens on the page: all four apps did at least one, and wrote JavaScript files for it. Measured: all 21 tasks can be written through `js`, and 4 of the 7 issues above come from reaching the page through JavaScript's names (Q-1130, Q-1133 to Q-1135).

| Impact | |
|---|---|
| **Functionality** | `dom.find`, `dom.find_all` (inside the component, lists), `dom.find_in`, `dom.page_find`, `dom.on` (taken off by itself), `dom.focus`, `dom.scroll_to`, `dom.scroll_end`, `dom.measure`, `dom.typing` (Part B.2). `js` stays |
| **User** | Shorter components: the apps' helper files move into their components as Quillo, and none of it touches `window`. `quillo check` and the editor know every name |
| **Requests** | No change: all of it runs in the browser |
| **Security** | Better by default: a component's query does not reach another component's elements unless it says `page_find`. The same checks as `js` |

What today's way has that a module would lose: one way to do everything, which is JavaScript's own and found in every tutorial.

**My recommendation:** build it after the fixes, with the scope check measured for speed before and after.

**The question:** shall I plan the `dom` module as in Part B.2? If not, I write the guide section "Changing the page by hand" with the `js` recipes of Part A.2, and the Q-1133 to Q-1135 checks only.

### 2. Should `ref={x}` set `x` to the element, instead of warning?

**Yes. It is what a model writes first from React or Solid, `quillo check` already finds it, and it would also clear `x` when the element goes, which `@mount` does not do.** Measured: 2 of the 26 habit programs wrote `ref`, and a third `bind:this`, for the same task.

| Impact | |
|---|---|
| **Functionality** | `state box = null` with `<input ref={box}/>`: `box` is the element while it is on the page, null otherwise. `@mount` stays for anything more |
| **User** | One line instead of a callback, and no stale element after it goes |
| **Requests** | No change |
| **Security** | No change |

What the warning has that this loses: one way to reach an element, `@mount`, which also shows when it is placed.

**My recommendation:** make it work, typed as an element or null.

**The question:** shall `ref={x}` work as above? If not, the warning stays as it is, and `bind:this` gets the same warning (Q-1134).

### 3. Should `<window>` and `<document>` be elements with handlers, as in Svelte?

**The warning of Q-1132 is needed whatever the answer. Making them real is a choice of style.** It happens for every page shortcut, resize or outside click. Measured: today's four-line `@mount` way works and cleans up.

| Impact | |
|---|---|
| **Functionality** | `<window onkeydown={f}/>` and `<document onclick={f}/>` in a component render nothing, and listen while the component is shown (or while an `if` around them holds) |
| **User** | Page events written like element events, and what a model writes first works |
| **Requests** | No change |
| **Security** | No change |

What it loses: markup that is only HTML. With `dom.on` (question 1) there would be two ways to do the same thing.

**My recommendation:** `dom.on` only, with the Q-1132 warning pointing to it. That keeps one way, and keeps markup HTML. If question 1 is no, then the two elements.

**The question:** shall `<window>` and `<document>` stay unknown tags, warned of and pointed to `dom.on`? If not, I plan them as special elements.

### 4. Should a view's markup be able to do something in the browser without becoming a component?

**Yes, starting with forms that post in place. Today markup is either a component, with its state in the page and a runtime of 40.6 KB gzipped, or plain HTML that runs nothing, and a handler written in a view is dropped (Q-1137).** It happens on every server-drawn page that wants more than links and full-page posts. qwertime, Quillo's largest site, wrote 2,584 lines of JavaScript for it, and the core of them is posting, replying, reacting and messaging without a reload: the server's HTML put in place.

Three ways, not exclusive:

- **A. In-place forms.** An attribute on a view's form, such as `<form method="post" action="/posts" swap="#feed" at="top">`. Quillo's runtime sends it with `fetch` and puts the route's answer (a view) in place: replacing the target, or before, after, at the top or at the foot. Without JavaScript, it posts and the page comes back. The program writes no script. A small part of the runtime, loaded only by pages that have such a form.
- **B. Handlers in views.** A view's `onclick` and `@mount` compiled into a small script, with no state and no re-render: the element keeps the server's markup, and the handler can call the page, a `server fn` and the `dom` module. Everything qwertime's `app.js` does on timers and observers would become Quillo. It overlaps with components, so there would be two ways to give markup behaviour.
- **C. Neither.** Q-1137's warning, and the guide says to make such markup a component.

| Impact | A: in-place forms | B: handlers in views |
|---|---|---|
| **Functionality** | Forms in views post without a reload and show the server's answer in place; nothing else changes | Any view can carry handlers and `@mount`, which run in the browser without state |
| **User** | qwertime-style sites drop their fetch-and-swap code; no script to write for the common case; the page still works without JavaScript | Page scripts like `app.js` become Quillo, checked by `quillo check`; a model's first try (Q-1137) works |
| **Requests** | Each in-place post asks for HTML, as a full post does, but answers only the piece (a smaller answer than a full page) | No change |
| **Security** | The same CSRF gate as a form post (Sec-Fetch-Site and Origin); the answer is the program's own HTML, put in by the runtime, never `innerHTML` from data | The same checks as component code (no `innerHTML`, no text run as code) |

What today's way has that these would lose: one way to give markup behaviour (a component), and plain views that are known to ship no script at all.

**My recommendation:** A, after the bug fixes. It covers the largest and most-repeated part of qwertime's script with no program script at all, and it keeps the no-JavaScript fallback that qwertime chose views for. Then judge B after the `dom` module exists, as most of what B would do can then be a component without much cost. C's warning is needed in every case.

**The question:** shall I plan in-place forms for views (A)? If not, only Q-1137's warning, and the guide says to make such markup a component.
