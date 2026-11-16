# Feedback from studying the AI's two other doors: a Quillo component inside the AI client (MCP Apps) and a page's tools for the browser's own AI (WebMCP)

| | |
|---|---|
| App | None: a design study of the two items FEEDBACK57 left for later. First, MCP Apps: a tool's result shown to the user as a Quillo component inside Claude, ChatGPT, VS Code or Cursor. Second, WebMCP: a page giving its tools to the AI built into the browser. For each, what happens today, and what Quillo would do so the developer writes one word or nothing |
| Quillo | `0.7.0`, the release build of branch feedback55's working tree (it reports `8ee6f49`, 2026-11-05), the same binary as FEEDBACK56 and 57 |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | Headless Chrome 156 (`feedback/tools/chrome.mjs`, `drive.mjs`), with and without `--enable-features=WebMCPTesting`: a Quillo page that registers two tools by hand from a component through `js(...)`, and two forms carrying Chrome's form-tool attributes, all called through `document.modelContext.executeTool`. A Quillo page shown in a sandboxed `srcdoc` frame on another origin, under two content policies, as an AI client shows an MCP App, with Python's `http.server` as the host. `curl` for the runtime's size and the answers to a cross-origin call. The MCP Apps specification (Stable, 2026-01-26) and its draft; the WebMCP Community Group draft and its history; the feature names inside Chrome 156's binary. For Part C: a booking form the user found, in its own spelling and in Chrome's; `curl` on routes whose patterns hold a named set of strings; a Quillo page's links and buttons registered as tools and pressed through `executeTool`, `quillo check` on `data-mcp-desc` on elements and on a component, and the links and buttons of quillolang.org's pages counted. The scripts are at the end |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section. Numbers start at Q-868, after FEEDBACK57.md's Q-867. This builds on FEEDBACK56 (the MCP door, tool definitions, `confirm`) and FEEDBACK57 (`route mcp`, `show_page`, the typed-text guard). Under the rule of 2026-11-05, nothing is built until main is in sync, every test (CI too) is green and the docs are aligned.

What worked well:
- **Quillo code can already give tools to the browser's AI.** `js("document.modelContext.registerTool", {...})` from a component's `@mount` registered two tools. The Quillo functions in the map became functions the browser could call, and the call kept its object, with no "Illegal invocation". One tool called a `server fn` and gave the notes back. The other called `navigate` and moved the page with no load.
- **Chrome's form tools work on Quillo's plain forms.** `toolname`, `tooldescription` and `toolparamdescription` pass `quillo check`. Chrome made a tool of the form with the right schema: `email`, a required string, with its description.
- **The runtime already has the seam an MCP App needs.** A shell hands the runtime its `fetch` and its sockets with a `quillo-wire` event, as phone apps and `quillo app` do (`SHELL_JS`, `src/embed.rs:755`), and nothing goes on `window`.
- **The component ran inside the frame** once its scripts could load: its click handler ran and showed the call's error. Nothing in the runtime assumes it is the top window.
- **A component's code is small:** the runtime is 91,573 bytes (25,131 gzipped), the probe's bundle 1,057. A document carrying both inline is about 93 KB.

## The answer

The two doors go opposite ways:

| | MCP Apps | WebMCP |
|---|---|---|
| Where the AI is | the AI client: Claude on the web and desktop, ChatGPT, VS Code's Copilot, Microsoft 365 Copilot, Cursor, Goose and others; not Claude Code | the browser: Chrome's and Edge's own AI |
| What it gets | a component, shown to the user in the chat, fed by a tool's result | the page's tools, run in the page as the user, and each link and button with `data-mcp-desc` (Part C) |
| Its state | an official extension, Stable since 2026-01-26 | a Community Group draft that changed five times in 2026; an origin trial in Chrome (M149 to M156, and this machine has 156) and Edge (150); off by default |
| A Quillo page today | its text shows and nothing runs (Q-868). With the site's origin allowed, the scripts run but every call fails (Q-869) | works by hand: about 20 lines of `js(...)` per program, each schema written again, nothing to ask the user with (Q-871); a form tool's result is `null` (Q-870) |

**MCP Apps first**: it is stable, many clients show it, and it fits FEEDBACK56's door, since the tools are already there. The developer writes one word on a tool:

```quillo
/// One order, with its lines and its status.
fn mcp order(id: int) -> Order with db shows OrderCard = load_order(id)

component OrderCard(order: Order) {
    <div class="card"><h2>Order {order.id}</h2> ... </div>
}
```

The model gets the order as data, and the user sees `OrderCard` in the chat.

**WebMCP behind a flag**, made from what the program already says. Every `server fn mcp` (a function both pages and AI clients may call) and `quillo.show_page` are registered in the page, and so is each link and button with `data-mcp-desc` (Part C, decided 2026-11-05). There is no new word in the language: `data-mcp-desc` is an HTML attribute.

## Part A: MCP Apps, a component in the chat

### A.1 The word

`shows OrderCard` after the `with` (question 1). The component is checked: its first parameter must take the tool's return type, as a call `<OrderCard order={value}/>` would be checked. A view is accepted as well as a component. A function that `shows` needs a declared return type, so the component's parameter can be checked against it.

### A.2 What the door does

- The tool's definition gets `_meta.ui.resourceUri: "ui://main/OrderCard"`, with the default visibility (`["model", "app"]`): the model sees the tool, and the component may call it.
- `resources/read` of `ui://main/OrderCard` gives one HTML document, `text/html;profile=mcp-app`. It carries everything inline: the runtime, the component's bundle and the bundles of the components it uses, their styles, and the shell of A.3 (question 2). Nothing is asked of the site: under the default policy nothing undeclared loads, and a relative address resolves to the host anyway (Q-868).
- The document holds code, not data, so it is the same for every call and can be cached. The data comes with the tool's result.
- `server/discover` declares the extension `io.modelcontextprotocol/ui`. A client that does not show Apps ignores `_meta.ui` and gets the tool and its data as before.
- Each `client` library a component loads is written in `_meta.ui.csp.resourceDomains` from the program's own `client` lines, so the host lets it load. Quillo knows every one.

### A.3 The shell: the runtime's wire, over the host

Inside the frame, the runtime gets its way out from the shell, as in a phone app (`quillo-wire`):

- The shell sends `ui/initialize` to the host. When `ui/notifications/tool-result` comes, it renders `<OrderCard order={structuredContent}/>`, the same JSON the model got. `tool-input` can show a placeholder until then.
- **A component's calls go to the host as `tools/call`**, which the host passes to the door with the client's token. V2's wall (a preflight answered 405, a POST refused 403, no session in the frame: Q-869) is never met, and `session()` in the function is the token's, as at the door.
- **Which functions:** the compiler knows the `server fn`s a component can reach (its bundle calls them by name). The door lists each one with `_meta.ui.visibility: ["app"]`: the component may call it, the model does not see it, and the host refuses an App's call to a tool without `"app"`. A `server fn` is already an address anyone can call (`/_quillo/rpc`), so listing it for Apps opens nothing new (question 3).
- **Size:** the shell sends `ui/notifications/size-changed` when the component's height changes.
- **Links:** there is no page to move inside the chat. A link, or `navigate`, sends `ui/open-link` with the site's full address. The host may ask the user, or refuse.
- **Live data and channels** need a socket, which the frame may not open (`connect-src 'none'` by default), and `tools/call` cannot carry one. In an App, `live(...)` reads once, and `channel.open` gives an error value: `no channels in an AI client's frame`. `quillo check` warns when a component that a tool `shows` uses either.
- **Later:** the host's context (`ui/notifications/host-context-changed`, such as its theme; what it holds was not read for this study) as a value components read; `ui/update-model-context`, so what the user picks in the card reaches the model.

### A.4 A page in the chat

FEEDBACK57's `quillo.show_page`, when no tab is open and the client shows Apps, could show the page in the chat. The tool's `resourceUri` must be the same for every call, so its document would be a viewer that asks the door for the page (an app-only `quillo.render_page(path)`: the route rendered as the token's user, with its components' state) and then resumes its components. This comes after A.1 to A.3.

### A.5 Building on the stable spec

The stable spec is 2026-01-26. MCP's feature lifecycle expects extension revisions tied to the core's, and there is none for 2026-07-28 yet. The draft is still edited (2026-11-04: a file download, an App's own tools, and a mirror of those tools into WebMCP). Quillo follows the stable spec. ChatGPT reads `_meta.ui.resourceUri` (and `openai/outputTemplate` as another name), so one is enough.

## Part B: WebMCP, the page's tools for the browser's AI

### B.1 What Chrome 156 has

- `document.modelContext` exists only with the feature on (`--enable-features=WebMCPTesting` or `WebMCP`). Without it, `typeof document.modelContext` is `"undefined"`. It is never `navigator.modelContext` now: the draft moved it to `document` on 2026-05-27.
- `registerTool`, `getTools` and `executeTool` (the last two for agents inside the page). `executeTool` takes the input as an object: a JSON string is a `TypeError`.
- A DevTools domain, `WebMCP` (`invokeTool`, `toolsAdded`, `toolResponded`), which tests can drive.
- The form tools, which the draft removed on 2026-10-08, are still in Chrome. A form with `toolname` became a tool. Without `toolautosubmit`, the AI's call filled the field (`ana@example.com`) and waited for the user to press the button: the probe gave up after 8 s. With it, the form was sent and the page went to `/note` (`Added buy milk`), and the AI's call gave `null` (Q-870).

### B.2 What Quillo would do, behind a flag

`quillo run --webmcp`, or the same in the program's settings:

- **Tools from what the program says:** every `server fn mcp`, with its FEEDBACK56 definition: name, `///`, the 2020-12 schema. Its `execute` is the runtime's own call to `/_quillo/rpc`, as the user with their cookie. A plain `fn mcp` is not a page's to call (FEEDBACK56, A.1), so it is not in the page.
- **WebMCP's own hints:** `readOnlyHint` as at the door, `consequentialHint` when it is not read-only, and `untrustedContentHint` when it reads the database or the web (question 5).
- **Quillo's page tools:** `quillo.show_page` (tagged routes only, `navigate` in the same tab, FEEDBACK57's typed-text question) and `quillo.read_open_page`.
- **Links and buttons with `data-mcp-desc`:** Part C.
- **Asking the user** (Q-871): the draft has no way to do it (`requestUserInteraction` went in June 2026), but the user is on the page. When a function calls `confirm` or `ask`, the call's answer says so. The runtime shows its own bar on the page, as FEEDBACK57 A.4 does, and calls again with the answer, as at the MCP door (FEEDBACK56, Part E).
- **Lifetime:** registered once when the runtime boots, with an `AbortSignal` the runtime keeps. Gone with the document.
- **The trial:** a site in Chrome's or Edge's origin trial gets a token for its domain. `QUILLO_WEBMCP_TRIAL` puts it in an `Origin-Trial` header on the program's pages. The API exists only on HTTPS or `localhost` (`[SecureContext]`).
- **Nothing on `window`:** the code looks for `document.modelContext?.registerTool` and does nothing where it is not.
- **No form tools:** they are gone from the draft, a sent form gives the AI `null`, and the one that waits leaves the AI waiting.

### B.3 Why behind a flag

- **The draft changes too often:** in 2026, `provideContext` and `clearContext` went in March, `unregisterTool` in March, the API moved from `navigator` to `document` in May, `requestUserInteraction` went in June, and the form API in October.
- **Chrome's trial ends with Chrome 156.** An extension to M162 was asked for on 2026-09-28, and whether it was granted could not be confirmed. No browser has it on by default.
- **Its hints are not MCP's** (`untrustedContentHint` and `consequentialHint` have no MCP twin), and it has no way to confirm with the user.

Building it behind a flag costs little, since every piece comes from FEEDBACK56's definitions. Keeping it off by default costs nothing while the draft moves (question 4).

## Part C: links and buttons with `data-mcp-desc`

Decided with the user on 2026-11-05: links and buttons may carry `data-mcp-desc`, and WebMCP lists every element that does. The attribute is both the opt-in and the description. Nothing without it is offered.

```quillo
<a href="/orders" data-mcp-desc="Every order, newest first">Orders</a>
<button data-mcp-desc="Adds the teapot to the cart" onclick={() => { n = add_to_cart("teapot") }}>Add to cart</button>
```

### C.1 The attribute and the tool

- `data-mcp-desc` on an `<a href>` or a `<button>` is a plain HTML `data-` attribute. It passes through Quillo's markup as written: `quillo check` takes it on elements, and the probe's page carried it.
- **The tool's description** is the attribute's text. **Its name** comes from the element's `id`, else from its text (`follow_orders`, `press_add_to_cart`), made unique on the page. It takes no input: a press has no arguments.
- **The list follows the page.** An element is registered when it shows and removed when it leaves. The runtime knows when a component's blocks change, and static elements are registered when it boots. In the probe, a list made at boot had no tool for the `Order 12` link that appeared after a move.
- Only under the flag of B.2, and only where `document.modelContext` exists.

### C.2 Pressing

- **A tool's `execute` clicks the element.** The probe showed that a click made by script runs the Quillo handler: `press_add_to_cart` called `add_to_cart` (`POST /_quillo/rpc/add_to_cart`), and the count went to 1.
- **A link** is followed only to this site, as a click would, with FEEDBACK57's typed-text question.
- **A button that can change data asks first, on the page.** The compiler knows which `server fn`s a handler can call, and FEEDBACK56's walk knows which only read (Q-858). A button whose handler can reach one that writes, or one with no `with`, is pressed only after the user says yes. The bar shows the element's own description: `Your browser's assistant wants to press "Delete account": Deletes the account and every order; it cannot be undone. [Press it] [No]`. Its tool has `consequentialHint: true`. In the probe, `press_delete_account` ran at once (Q-873).
- A link, or a button that changes only the page, has `readOnlyHint: true` and is pressed without a question.

### C.3 What the AI gets back

In the probe, each press answered only where the page was (`{"path":"/","connected":true}`). The AI could not tell that the cart now held one item. The runtime sees each call a handler makes and its answer, so a press gives:
- the path and title after it
- each call and its answer (`add_to_cart` → `1`)
- each error value
- whether the user said no

### C.4 On components (Q-872)

Buttons are often components (the kit's are), and `<Btn label="Add to cart" data-mcp-desc="..."/>` is an error: a component's attributes are its parameters. A component cannot even declare it, since a parameter with dashes does not parse. Until question 6 is decided, a component takes a parameter of another name and writes it on its element:

```quillo
component Btn(label: str, desc: str? = null) {
    <button class="btn" data-mcp-desc={desc}>{label}</button>
}
```

### C.5 Forms, the same way (proposed)

A snippet the user found on 2026-11-05 writes the form API with hyphens (`tool-name`, `tool-description`, `tool-param-description`). **Chrome 156 does not read that spelling.** Of two copies of the same booking form, only the one written `toolname`, `tooldescription` and `toolparamdescription` became a tool. The schema Chrome made from that one was good:
- the date field was `format: "date"`, with `(Dates MUST be provided in 'YYYY-MM-DD' format.)` added to its description
- the `<select>` was an `enum`, each option's text its `title`
- `required` came from the field's attribute

Chrome refused `brake_job` before filling anything. Called with good values, it filled both fields and waited for the user to press Confirm Booking, with no answer to the AI (Q-870).

Under Part C's rule, a form is one more element that may carry `data-mcp-desc`, and Quillo makes the tool, rather than following the browser's spelling of the month:

```quillo
<form id="book_appointment" data-mcp-desc="Books a service appointment for the user." action="/book" method="post">
    <label>Date: <input type="date" name="appointment_date" required/></label>
    <label>Service:
        <select name="service" data-mcp-desc="The type of service requested.">
            <option value="oil_change">Oil change</option>
            <option value="tire_rotation">Tire rotation</option>
        </select>
    </label>
    <button>Confirm booking</button>
</form>
```

- **Its name** is the form's `id` (or `name`), else its button's text.
- **Its fields** are the named fields, each described by its own `data-mcp-desc`, else by its label. Their types come from the route the form posts to, when the server rendering the page matches it: `date`, `int`, a set of strings, required unless the pattern gives a default. Otherwise they come from the fields, as Chrome does. A field limited to a set of strings cannot be checked by its route today (Q-874).
- **Calling it fills the fields,** so the user sees what is sent. A form that posts asks first on the page, as a button that writes does (C.2), and the user's yes may be the form's own button. Then the runtime sends it and gives the AI the answer: the status and the answer's text, or the route's own words when its pattern refuses the form (`form.appointment_date is missing, and must be a str`), never `null`.
- **Chrome's own attributes are not written,** so the browser does not make a second tool of the same form.

### C.6 Where else the descriptions go

FEEDBACK57's `quillo.read_open_page` and `quillo.read_page` keep a described link's or button's description beside it, so an AI reading the page at the MCP door learns the same as the browser's.

## Seen again

- **Q-853** (FEEDBACK56): Part A needs the MCP door first.
- **Q-864** (FEEDBACK57): `quillo.show_page` in the page through WebMCP loses typed text as the channel-driven move did. The same guard covers it.

## New issues

### Q-868. A Quillo page shown in an AI client's frame loads nothing: its scripts are asked for by relative addresses, which resolve to the host

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | headless Chrome, the page in a sandboxed `srcdoc` frame on another origin (V1) | 1 |

```quillo
server fn tally() -> int = 42

component Card() {
    state n = "none"
    <div>
        <p id="static">Order 12: 3 lines, shipped</p>
        <button id="go" onclick={() => {
            let r = tally()
            n = is_err(r) ? `error: ${r.error}` : str(r)
        }}>Count</button>
        <p id="n">{n}</p>
    </div>
}

route GET / = <html><body><Card/></body></html>
```

The page's HTML, as `resources/read` would give it, was put in a frame on `127.0.0.1:47863` under a strict policy (`default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'`, close to MCP Apps' default). The text showed (`Order 12: 3 lines, shipped`). The page's two inline scripts ask for `/_quillo/r/9c0d6cd769b3bbe8.js` (the runtime) and `/_quillo/b/<hash>.js` (the bundle). Both resolved to the host's origin, `http://127.0.0.1:47863/_quillo/...`, and the policy blocked both. The button did nothing: `n` stayed `none`.

**Suggestion:** A.2's document, with everything inline.

### Q-869. A component's call from another origin cannot succeed: the preflight is answered 405, the call 403, and the frame has no session

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | headless Chrome (V2: the site's origin allowed, a `<base>` to the site), `curl` | 1 |

With the site allowed in the policy and a `<base href>` to it, the scripts loaded from the site and the component ran. Pressing Count gave `` error: server fn `tally` could not be reached: Failed to fetch ``. The browser asked first (`OPTIONS /_quillo/rpc/tally` → 405), and the call never left. Sent anyway (`curl` with `Origin: http://127.0.0.1:47863`), it is refused: `403 server fns can only be called from the program's own pages`. And a frame on the host's origin carries no cookie of the site's. All three are right for a browser, since they are what keeps another site's page from calling a user's functions. So inside an AI client, a component's calls have no way to the site but the host.

**Suggestion:** A.3's wire, `tools/call` through the host, with the token. No CORS on `/_quillo/rpc`.

### Q-870. A form tool sent by the browser's AI gives it `null`, and one without `toolautosubmit` leaves it waiting with no end

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | headless Chrome 156 with `--enable-features=WebMCPTesting` | 1 |

```quillo
route POST /note { form: { text: str } } = <p id="added">Added {text}</p>

// in the page:
<form id="quick" toolname="quick_note" tooldescription="Adds a note at once." toolautosubmit action="/note" method="post">
    <input name="text" toolparamdescription="The note" required/>
    <button>Add</button>
</form>
```

`executeTool(quick_note, { text: "buy milk" })` sent the form. The page went to `/note`, showing `Added buy milk`, and the call gave `null`: the AI cannot tell whether the note was added. The form API's explainer gives a page a way to answer (`SubmitEvent.respondWith`), which needs script on the form, and a Quillo route that answers a form with a page has none. The `sign_up` form, without `toolautosubmit`, was filled (`ana@example.com`) and its call was still waiting after 8 s, for the user to press the button: an AI that waits for it has no limit. The form API is gone from the draft, so this is a reason not to build on it, more than something to fix.

**Suggestion:** B.2: imperative tools from `server fn mcp`, whose result is the function's value. The guide can say why form tools are not used.

### Q-871. A tool registered by hand writes each schema again and has nothing to ask the user with

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the probe's `Tools` component | 1 |

```quillo
component Tools() {
    <span @mount={(el) => {
        js("document.modelContext.registerTool", {
            name: "show_order",
            description: "Shows one order on this page.",
            inputSchema: { type: "object", properties: { id: { type: "integer" } }, required: ["id"] },
            execute: (input) => {
                navigate(`/orders/${input.id}`)
                `shown /orders/${input.id}`
            },
        })
        js("document.modelContext.registerTool", {
            name: "list_notes",
            description: "The user's notes.",
            inputSchema: { type: "object", properties: {} },
            execute: (input) => json.stringify(list_notes()),
        })
    }}></span>
}
```

It works (see what worked well). But each tool's schema and description are written again beside the `server fn` they call (`list_notes` already says `-> list` and has a `///`), and they drift when the function changes. `show_order` accepts any `id` and moves the page whatever the user typed. `execute` cannot ask the user anything: the draft dropped `requestUserInteraction`. To stop offering the tools when the component leaves, the developer makes an `AbortController` with `js_new` and aborts it in `@mount`'s cleanup. None of this is hard, and all of it is what each program would write again.

**Suggestion:** B.2, under the flag.

### Q-872. `data-mcp-desc` cannot go on a component, nor can `aria-label`, and a component cannot declare either

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` | 1 |

```quillo
component Btn(label: str) {
    <button class="btn">{label}</button>
}

route GET / = <html><body>
    <a href="/orders" data-mcp-desc="Every order, newest first">Orders</a>
    <button data-mcp-desc="Deletes the account and every order; cannot be undone">Delete account</button>
    <Btn label="Add to cart" data-mcp-desc="Adds this teapot to the cart"/>
</body></html>
```

`desc.qwe:8: error: <Btn> has no parameter data-mcp-desc`. On `<a>` and `<button>` the attribute passes. On a component, it is taken as a parameter, and `Btn` has none. Declaring one fails too: `component Btn(label: str, data-mcp-desc: str?)` gives `expected ), found -`. `aria-label` is refused the same way (`<Btn> has no parameter aria-label`). For `class`, the message says what to do (`give it a class parameter and put it on its element`). For `data-` and `aria-` attributes, nothing says how. So a button component cannot be described for the AI, nor labelled for a screen reader, without a parameter of another name, written by hand on its element.

**Suggestion:** question 6. Meanwhile, the `class` message's advice for `data-` and `aria-` attributes, with C.4's example.

### Q-873. A button pressed by the browser's AI: no question even when it deletes, and no word back of what its calls did

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | headless Chrome 156, every link and button of the probe registered by hand | 1 |

With each link and button registered as a tool (a script, as a runtime would do it), `press_delete_account` ran at once, and `press_add_to_cart` answered `{"path":"/","connected":true}`, though its handler had called `add_to_cart` and the count was now 1. The page does not know which handlers can change data on the server, nor what a handler's calls answered. A list registered when the page booted also missed the `Order 12` link that a move showed later. Each program registering its own would meet all three.

**Suggestion:** C.1 to C.3.

### Q-874. A field limited to a set of strings cannot be checked by a form's or a query's pattern: named, it refuses every value; written out, it binds nothing

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check`, `curl` | 2 |

```quillo
type Service = "oil_change" | "tire_rotation"
type Booking = { appointment_date: date, service: Service }

route GET /q ? { service: Service } = `q ok ${service}`
route POST /r { form: Booking } = `r ok ${form.service}`
route POST /b { form: { appointment_date: str, service: Service } } = `b ok ${service}`
route POST /d { json: { appointment_date: date, service: Service } } = `d ok ${service}`
route POST /s { form: { service: "oil_change" | "tire_rotation" } } = `s ok ${request.form.service}`
```

| Request | Answer |
|---|---|
| `GET /q?service=tire_rotation` | 400, `the request does not match its pattern` |
| `POST /r`, `appointment_date=2026-11-12&service=tire_rotation` | 400, the same |
| `POST /b`, the same form | 400, the same |
| `POST /d`, the same as JSON | 200, `d ok tire_rotation` |
| `POST /s`, `service=tire_rotation` | 200 |
| `POST /s`, `service=brake_job` | 400, `` `form.service` is text, not what its pattern takes `` |

A named type of strings matches JSON, and refuses the same text from a form or a query, though the guide says a form's text is "converted to the numbers and booleans the record asks for, as query values are". The log's message does not say which field failed, as it does for a missing one. Written out in the pattern instead, the set checks the value but binds no name. `` `s ok ${service}` `` is `` unknown name `service` `` when the program loads, and the body has to read `request.form.service`. So a booking form's service, a size, a status, anything picked from a `<select>`, cannot be both checked by its route and used by its name today. The probe's own route for the pasted form failed both ways before it worked with `str`.

**Suggestion:** a query's or a form's text matches a named type of strings as JSON's does, and a record holding one (`{ form: Booking }`) works as the guide says. When a field fails, the log names it (`` `form.service` must be "oil_change" | "tire_rotation", got "brake_job" ``). A set written out in a pattern binds its field like any other type, or says, where the name is used, that it binds nothing and to name the type.

## Proposed order of work

Each slice with Rust and browser tests in `tests/feedback58.rs`, edge tests, `runtime.js` kept at 99.50% and Rust at 99%, the guide and the book in the same slice, a review agent after each, and a local commit per slice. Part A needs FEEDBACK56's slices 1 to 5. Part B needs FEEDBACK56's definitions and FEEDBACK57's slices 1 to 4.

0. **Q-874 first, apart from MCP.** It is a bug in today's routes, in 0.7.0's patterns, and any program with a `<select>` meets it, so it does not wait for the rest. Tests: a named set of strings in a query, a form, a form record and JSON; a value outside the set (the message names the field); a set written out in a pattern, bound.
1. **The App's document and its shell (Q-868, Q-869).** The runtime, the bundles and styles inline, and the shell's wire: `tools/call` for calls, no sockets. Tests: a test host page like the probe's, speaking the stable spec's messages (`ui/initialize`, `tool-result`, `tools/call`, `size-changed`, `open-link`); the document under the strict policy (no violations); its size.
2. **`shows` and the door (A.1, A.2, A.3).** The word, the checker's rules, `_meta.ui`, `ui://` resources, app-only tools for the `server fn`s a component reaches, `resourceDomains` from `client` lines. Tests: a component whose parameter does not take the return type; a `shows` without a return type; a component with `live` or `channel.open` (the warning); a `client` library.
3. **By hand in Claude on the desktop and in ChatGPT.** Both need the door reachable: stdio for Claude on the desktop, HTTPS for ChatGPT.
4. **WebMCP behind the flag (Part B, Q-870, Q-871).** Registration from `server fn mcp` and `route mcp`, the hints, the in-page question, the trial header. Tests in headless Chrome with `--enable-features=WebMCPTesting`, through `executeTool` and the DevTools `WebMCP` domain. Without the feature, nothing registered and nothing failing.
5. **Links and buttons (Part C, Q-872, Q-873), and forms if question 8 says so (C.5).** Registration of each element with `data-mcp-desc` as it shows and leaves, the press, the question for what writes, the answer, and question 6's choice for components. Tests through `executeTool`: a link; a button that only changes the page; one that writes (the bar, a yes, a no); an element shown after a move; one removed; two with the same text (unique names); a link to another site (not followed).
6. **A page in the chat (A.4)**, after FEEDBACK57.

## Open questions for the user

1. **The word for a tool's view:** `shows OrderCard` after the `with` (recommended), or another spelling?
2. **The App's document:** everything inline, about 93 KB, which works under any host's default policy (recommended), or scripts loaded from the site with its origin declared (a smaller document, if the host honours the declaration)?
3. **A component's calls:** every `server fn` it reaches is listed as an app-only tool automatically (recommended: each is already a public address), or only the functions tagged `mcp`?
4. **WebMCP:** built now behind a flag (recommended), or not until a browser ships it on by default?
5. **WebMCP's `untrustedContentHint`:** on for every tool that reads the database or the web (recommended: others may have written what it gives), or a word the developer writes?
6. **`data-` and `aria-` attributes on a component call:** go to the element the component writes first, as a `class` might (recommended: no parameter to declare, and kit components get them for free), or each component declares a parameter and puts it on its element (C.4)?
7. **A described button that can change data:** asked on the page first (recommended), or pressed at once, since the developer chose to describe it?
8. **Forms:** a form with `data-mcp-desc` is a tool that Quillo registers and sends itself, with its fields' types from its route, and the route's answer back to the AI (recommended, C.5)? Or the developer writes Chrome's `toolname` attributes by hand, which work today, but which the draft dropped and which give the AI `null` or no answer?

## Smaller notes

- In the frame, `location.origin` read `"null"`, while the browser sent `Origin: http://127.0.0.1:47863` with the call. Whatever the host does, the site sees an origin that is not its own.
- Chrome 156's binary also names `WebMCPDeclarativeFileInput` and `WebMCPFormAssociatedCustomElements`, which by their names are form tools with file fields and with custom elements in them (not tried). Chrome is going further with form tools than the draft, which dropped them: one more reason to wait for the two to agree.
- MCP Apps' draft mirrors an App's own tools into `document.modelContext`. If that lands, Parts A and B meet: a Quillo component in the chat would give its tools to the browser the chat runs in.
- `cannot call json: it is map, not a function`: the message could name what was meant, `json.stringify` or `json.parse`.
- The pasted form's `<label>`s are not tied to their fields (no `for`, not around the field), so a screen reader does not read them with the field, and C.5 could not use them as descriptions. `quillo check` could warn on a label tied to nothing.
- The pasted form has no `action` or `method`, so it sends a GET to the page it is on, with the fields in the query. Nothing in the snippet answers it.

## Where the AI struggled

| What was tried | What happened | Whose mistake |
|---|---|---|
| `json(list_notes())` to give a tool's result as text | `cannot call json: it is map, not a function`; `json.stringify` is the call | the AI's, with a message that could name it |
| `server fn count()` in the App probe | `` `count` hides the builtin `count` ``, which said what to do | the AI's, with a message that helped |
| `executeTool(tool, "{}")` with a JSON string | Chrome: `invalid input object: value is not an object` | the AI's |
| The first WebMCP run, with one time limit for the whole script | It printed nothing: the form tool's call never ends, so the script hung until killed. Each step now has its own 8-second limit | the AI's (the driver) |
| Expected `__quillo_booted` to tell whether the frame's runtime started | It stayed false in V2, though the click handler ran and showed the call's error. The handler's run is the evidence used | the AI's (the probe) |

## Edge cases checked that behave well

- Chrome's form attributes (`toolname`, `tooldescription`, `toolparamdescription`, `toolautosubmit`) pass `quillo check` and reach the page as written.
- A Quillo function in a map passed to `js(...)` is a function to the page's JavaScript, and one that calls a `server fn` gives a Promise of its value.
- `js("document.modelContext.registerTool", ...)` calls the method on its object.
- `navigate` from a tool's `execute` moves a page that called `url()` with no load.
- A call to `/_quillo/rpc` from another origin is refused twice: at the preflight (405) and at the call (403).
- A click made by script (`el.click()`, as a tool's `execute` does) runs a Quillo `onclick` handler, its `server fn` call included.
- `data-mcp-desc` on `<a>` and `<button>` passes `quillo check` and reaches the page as written, readable as `el.dataset.mcpDesc`.
- A form's `date` field matches a `date` in the route's pattern (`appointment_date=2026-11-12`).
- Chrome refuses a value outside a `<select>`'s options (`Invalid value "brake_job" for parameter service`) before it fills anything.

## Scripts

The WebMCP probe (`webmcp.qwe`), run with `quillo run webmcp.qwe --port 47860`. It is the `Tools` component of Q-871, with:

```quillo
let notes = ["Call the bank", "Order 12 shipped"]

/// The notes there are.
server fn list_notes() -> list = notes

component App() {
    let path = url().path
    <div>
        <p id="where">{path}</p>
        <Tools/>
        {match split(path, "/") | filter((s) => s != "") {
            ["orders", id] => <h1 id="order">Order {id}</h1>
            _ => <h1>Home</h1>
        }}
    </div>
}

route POST /note { form: { text: str } } = <p id="added">Added {text}</p>
route POST /signup { form: { email: str } } = <p>Signed up {email}</p>

route GET /*rest = <html><body><App/>
    <form id="signup" toolname="sign_up" tooldescription="Signs up for the newsletter." action="/signup" method="post">
        <input name="email" type="email" toolparamdescription="The address to send it to" required/>
        <button>Sign up</button>
    </form>
    <form id="quick" toolname="quick_note" tooldescription="Adds a note at once." toolautosubmit action="/note" method="post">
        <input name="text" toolparamdescription="The note" required/>
        <button>Add</button>
    </form>
</body></html>
```

Its driver, with `CHROME_FLAGS="--enable-features=WebMCPTesting" node feedback/tools/drive.mjs drive-webmcp.mjs`, and what came back:

```js
const call = (name, input) => `const t = (await document.modelContext.getTools()).find((x) => x.name === ${JSON.stringify(name)});
  return await document.modelContext.executeTool(t, ${JSON.stringify(input)});`;
// typeof document.modelContext       "object" with the feature, "undefined" without
// getTools(): list_notes, show_order, sign_up (sign_up's schema: email, a required string, with its description)
// call("list_notes", {})             "[\"Call the bank\",\"Order 12 shipped\"]"   (POST /_quillo/rpc/list_notes)
// call("show_order", { id: 12 })     "shown /orders/12"; the page at /orders/12, "Order 12", no load
// call("sign_up", { email: ... })    no answer in 8 s; the field holds ana@example.com; nothing posted
// call("quick_note", { text: ... })  null; the page at /note, "Added buy milk"
```

The App probe (`app.qwe`, the `Card` of Q-868), run with `quillo run app.qwe --port 47862`. The host pages were made from its HTML, with a policy and, for V2, a `<base>`, and served with `python3 -m http.server 47863 --bind 127.0.0.1`:

```python
rec = "<script>window.__v=[];document.addEventListener('securitypolicyviolation',e=>window.__v.push(e.violatedDirective+' '+e.blockedURI))</script>"
S = "http://127.0.0.1:47862"
variants = {
  "v1": ("default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'", ""),
  "v2": (f"default-src 'none'; script-src 'unsafe-inline' {S}; style-src 'unsafe-inline' {S}; img-src data:; connect-src {S} ws://127.0.0.1:47862", f'<base href="{S}/">'),
}
# each: <iframe sandbox="allow-scripts allow-same-origin" srcdoc="<the page with the policy and base in a <head>>">
```

```
V1: text shown; script-src blocked http://127.0.0.1:47863/_quillo/r/9c0d6cd769b3bbe8.js and .../_quillo/b/4253d7bb599c33dc.js; Count: "none"
V2: no violations; the site served both scripts; Count: "error: server fn `tally` could not be reached: Failed to fetch"
    (OPTIONS /_quillo/rpc/tally -> 405 in the site's log)
curl -X OPTIONS ... /_quillo/rpc/tally                                  405
curl -H 'Origin: http://127.0.0.1:47863' -d '[]' .../_quillo/rpc/tally  403 server fns can only be called from the program's own pages
the runtime 91,573 bytes (25,131 gzipped); the bundle 1,057 (590)
```

The links-and-buttons probe (`shop.qwe`), run with `quillo run shop.qwe --port 47864`. Its driver registered every link and button. Under the decided rule, only the three with `data-mcp-desc` would be:

```quillo
let cart = []

server fn add_to_cart(item: str) -> int {
    cart = cart + [item]
    len(cart)
}

component Shop() {
    let path = url().path
    state n = 0
    <div>
        <p id="where">{path}</p>
        <nav><a href="/">Home</a> <a href="/orders" data-mcp-desc="Every order, newest first">Orders</a></nav>
        <button id="add" data-mcp-desc="Adds the teapot to the cart" onclick={() => { n = add_to_cart("teapot") }}>Add to cart</button>
        <span id="n">{n}</span>
        <button id="icon" onclick={() => { n = 0 }}>🗑</button>
        <button id="del" onclick={() => { n = -1 }}>Delete account</button>
        {path == "/orders" ? <a id="o12" href="/orders/12">Order 12</a> : null}
    </div>
}

route GET /*rest = <html><body><Shop/></body></html>
```

```
tools: follow_home: Home; follow_orders: Every order, newest first; press_add_to_cart: Adds the teapot to the cart;
       press_delete_account: Delete account; press_unnamed: 🗑
press_add_to_cart     {"path":"/","connected":true}; the count 1; POST /_quillo/rpc/add_to_cart in the log
press_delete_account  {"path":"/","connected":true}; the count -1, with no question
follow_orders         {"path":"/orders","connected":true}; Order 12 now on the page, with no tool
quillolang.org        /: 102 links, 28 buttons, 3 forms; /tools/: 57 links, 0 buttons, 2 forms
```

The pasted form (`book.qwe`), run with `quillo run book.qwe --port 47866`: the snippet as found (form `a`), and the same with Chrome's spelling, posting to a route (form `b`):

```
getTools(): book_appointment_b only; form a (tool-name, tool-description, tool-param-description) is no tool
  appointment_date: { type: string, format: date, description: "The requested date for the booking. (Dates MUST be provided in 'YYYY-MM-DD' format.)" }
  service: { type: string, enum: [oil_change, tire_rotation], anyOf: [{ const: oil_change, title: "Oil Change" }, { const: tire_rotation, title: "Tire Rotation" }] }
  required: [appointment_date]
executeTool(b, { appointment_date: "2026-11-12", service: "tire_rotation" })   no answer in 4 s; both fields filled
executeTool(b, { ..., service: "brake_job" })                                  UnknownError: Invalid value "brake_job" for parameter service
```

Sources: MCP Apps, stable <https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx>, the extensions overview <https://modelcontextprotocol.io/docs/extensions/overview> and the client matrix <https://modelcontextprotocol.io/extensions/client-matrix>; ChatGPT's page <https://developers.openai.com/apps-sdk/mcp-apps-in-chatgpt>; WebMCP <https://webmachinelearning.github.io/webmcp/> and its repository <https://github.com/webmachinelearning/webmcp>; Chrome Status <https://chromestatus.com/feature/5117755740913664>; Edge's trial <https://developer.microsoft.com/en-us/microsoft-edge/origin-trials/trials/0b76fe60-b266-458e-a285-04e375c0c31a>.
