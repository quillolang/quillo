# Feedback from designing MCP-driven page navigation: the AI moves the user's page, reads the site's pages, and talks from inside the page

| | |
|---|---|
| App | None: a design study. The question: what would it take for an AI to move the page a user has open on a Quillo site ("show me order 12"), to read the site's pages itself and follow their links, and to talk to the user from a chat box inside the page, with the developer writing no more than a tag on the routes it may use |
| Quillo | `0.7.0`, the release build of branch feedback55's working tree (it reports `8ee6f49`, 2026-11-05), the same binary as FEEDBACK56 |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | Headless Chrome over the protocol (`feedback/tools/chrome.mjs`, `drive.mjs`) on the hand-built version of the first part: a channel that remembers each user's pages, a component that calls `navigate` when a message comes, and a route standing in for the AI's tool. Four runs: signed in without a reload, signed in and reloaded, half a sentence typed, two tabs. `curl` for a signed-in page's HTML; `quillo docs` for a route's `///`. The MCP specification, revision 2026-07-28 (resources, subscriptions, URL-mode elicitation), the MCP Apps extension (Stable, 2026-01-26), the WebMCP Community Group draft and Chrome's and Edge's trial pages, and the Claude API reference for the in-page assistant's requests. The scripts are at the end |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section. Numbers start at Q-862, after FEEDBACK56.md's Q-861. This builds on FEEDBACK56: the MCP door, its tokens, its limits, `confirm` and `ask`. Nothing here works before FEEDBACK56's slices 1 to 5, and, under the rule of 2026-11-05, nothing is built until main is in sync, every test (CI too) is green and the docs are aligned.

The user chose all three on 2026-11-05: the AI moves the user's page (Part A), the AI reads the site's pages (Part B), an assistant inside the page (Part C).

What worked well: a server's message can move a page today, and the page keeps everything else. In the probe, a message on a channel made the page call `navigate("/orders/12")`. The order showed with no page load (a mark set on `window` before was still there), and Back went home. `navigate` already refuses `javascript:` URLs, and since the page fetches what it shows as the user, every route guard holds for a move as for a click. Route patterns are typed (`/orders/:id(int)`), and `quillo docs` already turns them into typed path parameters, which is what MCP's resource templates need. Components render their state on the server, so a route read on the server has its data in it. And live data already knows which topics each page read, which is what MCP's "this resource changed" notices need later.

## The answer

**Moving the page a user has open can only be done by a site that holds both ends, and Quillo does.** MCP has no way for a server to change what a browser tab shows:
- MCP Apps' `ui/open-link` asks the host to open a link in a new tab, and the host may refuse.
- URL-mode elicitation is for "sensitive interactions that must not pass through the MCP client" (sign-ins, payments), opens only with the user's consent, and must hide the page from the model.

An MCP server written apart from the site cannot reach the site's open pages at all. Quillo serves the MCP door (FEEDBACK56) and runs the runtime in every page, with a connection to each page already there for channels and live data. So Quillo can send the move from the door to the page.

What the developer writes, and what Quillo does:

```quillo
/// One order: its lines, its status and its invoice.
route mcp GET /orders/:id(int) with db = <html><body><OrderPage id={id}/></body></html>

/// Makes the invoice for an order, and shows it to the user.
fn mcp invoice(order: int) -> map with db, net {
    let inv = make_invoice(order)?
    show_page(`/invoices/${inv.id}`)      // the user's page moves to it
    inv
}

route GET /help = <html><body><Assistant/></body></html>   // a chat box in the page (Part C)
```

| Part | The developer writes | Quillo does |
|---|---|---|
| A. The AI moves the user's page | `route mcp` on the GET routes it may show, a `///` on each | the tools `quillo.show_page` and `quillo.open_pages`; finds the tab in front; asks on the page before losing typed text; tells the AI what happened |
| B. The AI reads the site's pages | the same tag | each tagged route as an MCP resource (a template when it has parameters), read as the token's user and turned into Markdown; the tool `quillo.read_page` for clients without resources |
| C. An assistant inside the page | `<Assistant/>` | the model's loop on the server, with the program's tools and the page it sits in; `confirm` and `ask` as buttons in the chat; streaming, limits, costs in the metrics |

Six things stand in the way today. Each is an issue below:

1. **The server cannot find a user's open pages** (Q-862): `channel.peers` gives connection ids, so the probe keeps its own list.
2. **A page that signs in without a reload stays anonymous to its channels** (Q-863): the probe's tool reached none of the user's pages.
3. **Moving a page loses what the user typed, with no question** (Q-864): "Dear support, the invoice for" was gone. The same happens on the user's own link clicks.
4. **The server cannot tell which tab is in front** (Q-865): both of the probe's tabs moved.
5. **A message to a page gets no answer** (Q-866): the server learns that it was queued, not what the page did.
6. **Nothing turns a page into text** (Q-867): a signed-in page was 3,534 bytes, 3,333 of them scripts, around 19 characters of text.

## Part A: the AI moves the user's page

### A.1 The tag

`route mcp GET /orders/:id(int) = ...`, the kind first and then `mcp`, as `fn mcp` has it (FEEDBACK56, Part A). `routes mcp /orders { ... }` tags each GET route of a group. The tag means: the AI may show this page to the user (here), and read it (Part B).

- Only GET routes. `route mcp POST ...` is an error: `a page the assistant may show or read is a GET route; to let it change something, write an fn mcp`.
- The route's `///` comment says what the page is, for the model (Q-854 seen again: today it is lost).
- `starts_declaration` (`src/parser.rs:140`) takes a `route` line only when its second word is a method in capitals, so it needs `mcp` too. Editors and the guide follow, as for `fn mcp`.

### A.2 The user's pages

The runtime keeps, for each session, the pages that session has open, without any code in the program (Q-862):

- A page of a program with a tagged route, or that calls `show_page` anywhere, opens its connection when it boots, as a page with a channel does today. It is kept under its session.
- **When its session changes, the page joins again** (Q-863). The server knows when an answer to the page's own request set or ended the session (a `server fn` calling `set_session`). It says so in a header, and the runtime opens its connection again, so the page is kept under the new session. Its channels join again too, with the new session in `peer.session`. That fixes Q-863 for every program with channels, not only for this.
- Each page reports, when they change: its path, its `<title>`, whether it is in front (`document.visibilityState`), when the user last did something on it, and whether it holds typed text not yet sent (A.4).
- In the program, `open_pages()` gives the current session's pages, `[{ id, path, title, front, idle }]`.

Pages are by session, not by user: a token is made in one browser's session (FEEDBACK56, C.3), so it reaches that browser's pages. Someone signed in on a phone and a laptop has two sessions (question 1).

### A.3 `show_page(path)` and `quillo.show_page`

`show_page(path, { page })` in the program, under `with net`, as `channel.send` is. The door also offers it as a tool of Quillo's own, with `quillo.open_pages`. Names under `quillo.` are kept for Quillo: a program's own tool names have no dot, and `service mcp` of a service named `quillo` is an error.

1. **The path must be a tagged route's**, matched as a request would be: `/orders/12` matches `/orders/:id(int)`, and `/orders/x` and `/admin` do not. Otherwise isError `not a page the assistant may show: /admin`. Only a path on this site: a full URL is refused.
2. **The tab:** the page given, else the session's page in front that the user used last, else the one used last. When none is open, the result says so and carries a link to the page, so the model can give it to the user.
3. **The move** goes on the page's connection as a reliable message, never as a datagram that may be lost (Q-866). The page calls `navigate(path)`, as a click on a link would, so a page that called `url()` changes without a load, and the server answers it as the user. A guard that refuses the user refuses the move.
4. **The answer** comes back from the page (Q-866), and is the tool's result: `{ shown: "/orders/12", title: "Order 12", front: true }`, `{ kept: "the user chose to stay" }`, or `{ kept: "no answer from the page in 10 s" }`. When the tab is not in front, the result says the user may not see it.
5. **A line on the page** says why it changed: `Shown by your assistant · Back`. Back is the browser's, since `navigate` adds to the history.

### A.4 Typed text and consent

- **Typed text** (Q-864). The runtime marks a page as holding typed text when a field changed and was not sent since: an input, a textarea or a `contenteditable`, but not password or hidden fields. A move asks first, on the page: `Your assistant wants to show Order 7. What you typed here would be lost. [Show it] [Stay]`. No answer in time means Stay.
- **The first move** of a session asks: `Let your assistant show you pages on this site? [Allow] [Not now]`. The answer is kept in the session. `Not now` is the tool's result, so the model can say so. This is the consent a developer would otherwise write (question 2).
- **Limits:** a move counts toward the token's limit (FEEDBACK56, C.4), and moves to one page are at least a second apart, so a looping agent cannot make a page flicker.

## Part B: the AI reads the site's pages

### B.1 Resources

Each tagged route is an MCP resource:

| MCP | From |
|---|---|
| `resources/list` | tagged routes with no parameters (`/orders`) |
| `resources/templates/list` | tagged routes with parameters, as RFC 6570 templates: `/orders/:id(int)` is `quillo://shop.example.com/orders/{id}`, and a rest parameter (`/files/*path`) is `{+path}`, which may hold slashes |
| `uri` | `quillo://<host><path>`. The spec says to use `https://` only for what the client can fetch itself, and a signed-in page needs the user's token. The page's real `https://` address is the first line of what is read, so the model can give it to the user |
| `description` | the route's `///` |
| `mimeType` | `text/markdown` |
| `resources/read` | B.2 |
| not found | `-32602`, never empty contents, as the spec requires: a path that matches no tagged route, and a route that answers 404 |

The list is the same for everyone (it is the program's tagged routes), so it can be cached as long as the program is not reloaded. What is read differs by user, and is never cached.

### B.2 Reading a page

`resources/read` runs the route as a GET from the token's user: its groups' middleware, its guards, its tenant, as a click by that user would. Then it turns the HTML into Markdown (Q-867):

- **What is kept:** headings, paragraphs, lists, tables, the text in components (their server HTML, `<quillo-island>`'s contents), images as their `alt`, links as Markdown links with the site's full address, and links to tagged routes with their `quillo://` URI, so the model can follow them.
- **What is dropped:** scripts, styles, `hidden` and `aria-hidden` elements, and every field's value. A form is listed by its labels and its button: `Form: Name, Email; button "Sign up"`. A model changes data through tools, not forms.
- **A redirect** is followed when it stays on the site and lands on a tagged route. Otherwise it is said: `this page sends to /login: the token's session is not signed in`.
- **Other answers:** JSON as `application/json`, text as text. A page over 100 KB of Markdown is cut, with a line saying so.
- **Parts made only in the browser** are not in the server's HTML, and the read says that parts of the page are made in the browser.
- **What other people wrote** on a page is data, not instructions, as FEEDBACK56's generated lines say for tool results.

### B.3 For clients without resources, and links in results

- `quillo.read_page(path)` is the same read as a tool. Many clients let the model call tools freely, but read resources only when the user picks one.
- `quillo.read_open_page()` reads the user's tab in front from the page itself, so it includes what was made in the browser. It leaves out field values and anything marked `data-private`, under the consent of A.4.
- `page_link(path)` in a tool's result is a `resource_link` to a tagged page: `fn mcp find_order(...)` can give the order and a link to its page.
- Later: when a client listens (`subscriptions/listen` with the page in `resourceSubscriptions`), live data's knowledge of what a page read tells the door when it changed, and the client gets `notifications/resources/updated`.

## Part C: an assistant inside the page

### C.1 What it is

`<Assistant/>`, a component of Quillo's (how a program takes it in is question 4), shows a chat box. The model's loop runs on the server, for that page:

- **Its tools are the program's `fn mcp`s**, with the same definitions, checks and errors as at the door, run in the process as the page's session. There is no token and no network hop: the page is already signed in. Plus `quillo.show_page`, `quillo.read_open_page` and `quillo.read_page`, which here mean *this* page: the move needs no consent bar, since the user is asking, but the typed-text question of A.4 still holds.
- **`confirm` and `ask`** show as buttons in the chat. Quillo functions run as coroutines that wait without holding anyone up, so here they can simply wait for the click. The rule of FEEDBACK56, Part E (ask before changing anything) holds anyway, so a function behaves the same at both doors.
- **What the model is told:** FEEDBACK56's generated lines, the developer's `//!` text, and the page's path and title. When the user moves, the new path goes in as a new message, not as a change to what came before (C.2).

One definition, three uses: a plain function to the program, a tool at the MCP door for outside clients, and a tool of the assistant in the page.

### C.2 The requests

Quillo is in Rust, which has no official Anthropic SDK, so the server sends the Messages API's requests itself over HTTPS:

- `POST https://api.anthropic.com/v1/messages`, with `x-api-key` from `ANTHROPIC_API_KEY` (the environment or `--env-file`, never sent to the page), `anthropic-version: 2026-10-21` and `content-type: application/json`.
- **Model:** `claude-opus-5-5` unless the program names another. Thinking stays as the model has it (adaptive; it cannot be turned off on this model). `output_config.effort` is set explicitly, since this model's default is `medium`. Which level a chat box wants is a setting, measured on real questions.
- **Refusals:** `fallbacks: "default"` with `anthropic-beta: server-side-fallback-2026-07-01`, so a refusal is tried again on another model in the same call. `stop_reason: "refusal"` is checked before the answer is read, and the chat says the assistant cannot help with that.
- **Streaming** (`stream: true`), each piece of text sent to the page as it comes, with `max_tokens` large enough not to cut an answer short.
- **Tools:** `input_schema` from FEEDBACK56's 2020-12 writer, `strict: true` where the schema allows it (it writes `required` and `additionalProperties: false`), and `eager_input_streaming: true`. The door's argument check then validates each input before anything runs, since a streamed input may be cut short. `tool_choice` is `auto`: forcing a tool is refused on this model. Several calls in one answer run, and their results go back together in one message.
- **Caching:** the tools and the system text are the same for every page of a program until it is reloaded, so one cache breakpoint goes after them. The page's path and title, which change, go after it as a mid-conversation system message (a `system` role in `messages`, which this model takes), so the cached part stays the same.
- **The conversation is kept on the server, append-only:** this model's thinking blocks are tied to the conversation they came from, and an edited history loses them. It ends when the page leaves (question 3).

### C.3 Limits, costs and checks

- **The program pays**, so the limits are its own: tokens per session per day and per address a minute. Over them, the chat says the assistant is busy.
- **Metrics:** `quillo_assistant_tokens_total{kind="input|output|cache_read|cache_write"}` and the requests' times, on `/_quillo/metrics`. Each conversation's use goes in the log with the request id.
- **Without a key,** `quillo check` warns, and `<Assistant/>` renders nothing.
- **Tests never call the real API.** A fake Messages server in the tests answers recorded streams (text, a tool call, two calls at once, a refusal, a cut stream). One run against the real API, which costs money, is the user's to allow.
- **Other providers** would come as plugins, as database drivers do (question 5).

## Part D: later

- **WebMCP.** A page can give its tools to the browser's own AI (`document.modelContext.registerTool`, a W3C Community Group draft). It is in an origin trial in Chrome (M149 to M156) and Edge (150). The same definitions would feed it, with `show_page` calling `navigate` in the same tab, with no server in between. But the draft changed five times in 2026 (it moved from `navigator` to `document` in May; `provideContext` went in March, `unregisterTool` the same month, `requestUserInteraction` in June, and the form-based API in October), and it has no way to confirm with the user. So it comes behind a flag, after the rest.
- **The page inside the AI client: MCP Apps** (`io.modelcontextprotocol/ui`, Stable since 2026-01-26; Claude on the web and desktop, ChatGPT, VS Code's Copilot, Cursor and others; not Claude Code). A tool can name a `ui://` resource, an HTML page the host shows in a sandboxed frame. When the user has no tab open, `quillo.show_page` could show the page in the chat instead. The frame cannot connect anywhere it has not declared (`connect-src 'none'` by default) and cannot move the page around it. So a Quillo component there needs its server calls declared, or made through the host's `tools/call`. A study of its own.
- **Not URL-mode elicitation** for showing pages. It is for what must not pass through the client, it opens only with consent, and the model must not see the page.

## Seen again

- **Q-854** (FEEDBACK56): a route's `///` is lost too. `/// One order: its lines and its status.` above `route GET /orders/:id(int)` leaves the OpenAPI operation with no `summary` or `description`.
- **Q-853 and Q-859** (FEEDBACK56): Parts A and B need the door and the token first.

## New issues

### Q-862. The server cannot find a user's open pages: `channel.peers` gives connection ids, and each program keeps its own list

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the probe, headless Chrome | 1 |

```quillo
let pages = {}

channel.on("nav", (msg, peer) => null, {
    join: (peer) => {
        let u = peer.session.user
        if u == null { return false }
        pages = set(pages, u, (pages[u] ?? []) + [peer.id])
        true
    },
    leave: (peer) => {
        let u = peer.session.user
        if u != null { pages = set(pages, u, (pages[u] ?? []) | filter((id) => id != peer.id)) }
    },
})
```

To show Ana a page, the probe needs her pages' ids. `channel.peers("nav")` gives every page's id and nothing else, so the program records who joined and removes who left, in a global that a `--scale` copy does not share. Each app that wants "this user's pages" writes this again.

**Suggestion:** A.2. `open_pages()` for the current session, kept by the runtime.

### Q-863. A page that signs in without a reload stays anonymous to its channels: the tool reached none of the user's pages

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the probe, headless Chrome, run A | 1 |

Open `/`, which joins `nav` and is refused (no one is signed in). Sign in from the page with a `server fn` that calls `set_session({ user: "ana" })`, as a sign-in form in a single-page app does. Then call the tool for Ana: `{"open":0,"reached":0}`. The page shows Ana signed in, and nothing can reach it until it is reloaded. The browser's console has `quillo: the server did not let this page join channel nav`, and the page never asks again. The guide says someone who signs in "is seen so once the page reconnects", but nothing makes it reconnect. A chat for signed-in people built as the guide's Lobby would have the same gap when the sign-in happens on the page, without a load (not run here).

**Suggestion:** A.2's header and reconnect, for every page with channels.

### Q-864. Moving a page loses what the user typed, with no question, and so does the user's own click

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the probe, headless Chrome, run C | 1 |

On Home, type `Dear support, the invoice for` in its field, then call the tool to show `/orders/7`. The order shows. Back shows Home with the field empty. The same happens when the user clicks a link to the order: Home's component leaves the page and takes the text with it. `runtime.js` has no `beforeunload` and nothing that knows a field holds unsent text. When a person clicks, it is their own doing. When an AI moves the page while they are typing, it is not.

**Suggestion:** A.4's question for every move the user did not make. For the user's own clicks, question 6.

### Q-865. The server cannot tell which of a user's tabs is in front: every tab moves

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the probe, headless Chrome, run D | 1 |

With two tabs open on the site, the tool reached both (`{"open":2,"reached":2}`), and both moved to `/orders/99`. A page tells the server nothing about itself after it connects: not whether it is in front, not what it shows, not when it was last used. A tool cannot move just the one the user is looking at, nor say that the user is looking at none.

**Suggestion:** A.2's reports and A.3's choice of tab.

### Q-866. A message to a page gets no answer: the server learns that it was queued, not what the page did

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the probe, the channels section of the guide | 1 |

`channel.send` gives whether the page was in the channel, so the probe's tool said `reached: 1` in runs B, C and D alike. It would say the same if the page had refused the path, stayed on a form, or lost the message. A small message sent without `{ reliable: true }` goes as a WebTransport datagram, which "may be lost", and the hand-built tool did not set it, as nothing near `navigate` in the guide says to. So an AI would tell the user "it is on your screen" when it may not be.

**Suggestion:** A.3's reliable move and the page's answer as the tool's result.

### Q-867. Nothing turns a page into text: a signed-in page was 3,534 bytes, 3,333 of them scripts

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `curl` with the session's cookie, a count of what a person sees | 1 |

`GET /orders/12` as Ana gave 3,534 bytes of HTML. 3,333 of them were the runtime's scripts, and the text a person sees was `/orders/12 Order 12`. A model fetching pages as they are spends nearly all its reading on scripts. `html` values are strings (`Value::Html(Arc<str>)`), and `markdown()` goes the other way, from Markdown to HTML. Nothing goes from a page to its text.

**Suggestion:** B.2's HTML-to-Markdown, in Rust, over the HTML a route renders.

## Proposed order of work

Each slice is useful and testable by itself: Rust tests and browser tests in `tests/feedback57.rs`, edge tests, `runtime.js` kept at 99.50% and Rust at 99%, the guide and the book in the same slice, a review agent after each, and a local commit per slice.

1. **Today's gaps, for every program with channels (Q-862, Q-863, Q-865).** The reconnect on a session change, the session's page list, the pages' reports, `open_pages()`. Tests: the probe's runs A and D give one page, then the one in front; a sign-out leaves the list; a `--scale` copy sees the same list (or the guide says it does not).
2. **The typed-text guard (Q-864)** for moves the user did not make. Tests: a field typed and not sent; one sent; a password field (not counted); `contenteditable`; a Stay and a time-out.
3. **The tag (A.1).** `route mcp`, `routes mcp`, the POST error, `starts_declaration`, editors, the guide.
4. **Moving pages (A.3, A.4, Q-866).** `show_page`, `quillo.show_page`, `quillo.open_pages`, the first-move consent, the line on the page, the answer. Tests: a path no tagged route matches; a full URL; no page open; a tab not in front; a guard that refuses the user; a page that leaves before it answers; two moves in a second.
5. **Reading pages (Part B, Q-867).** HTML to Markdown, resources and templates, `quillo.read_page`, `quillo.read_open_page`, `page_link`. Tests: each element kept and each dropped; a page that sends to `/login`; a 404 (`-32602`); a page over 100 KB; field values never read; links made absolute and tagged ones given their URI.
6. **The assistant (Part C).** The component, the loop, streaming, the in-page `confirm` and `ask`, the limits, the metrics, the fake Messages server. Tests: each recorded stream; a tool error; a refusal; a cut stream; a limit reached; no key. Then one real run, if the user allows it.
7. **Later (Part D):** resource updates through live data, WebMCP behind a flag, and a study of MCP Apps.

## Open questions for the user

1. **Pages by session or by user:** a token reaches its session's pages only (recommended first, as it needs nothing from the program), or the program names what a user is (`session().user`) so a laptop's token reaches the phone too?
2. **Consent to moves:** ask on the page at the first move of a session (recommended), or let the token be enough?
3. **The in-page conversation:** ends when the page leaves (recommended: nothing kept), or kept for the session for an hour, so it follows the user across pages loaded in full?
4. **How a program takes in `<Assistant/>`:** a component of Quillo's kit, taken with `use` as other kit components are (recommended), or a builtin that is always there?
5. **Other model providers:** Claude only at first (recommended), and others later as plugins, as database drivers are?
6. **The typed-text guard on the user's own clicks:** off, as today and as browsers do unless a site asks (recommended, with a way to ask for it on a form), or on by default?

## Smaller notes

- The resource URI is `quillo://`, and the spec says a custom scheme must follow RFC 3986. A host name with a port (`quillo://localhost:8080/orders/12`) is valid there; tests should hold one.
- `quillo.read_open_page()` and A.4's guard both need to know which fields hold typed text. They should share one list in the runtime, kept in its closure, not on `window`, as the rule against names on `window` asks.
- A page with no component loads no runtime today, only the small script for prefetching. For A.2, such pages need the part that keeps the connection, and only in a program that tags a route or calls `show_page`, so other programs' pages stay as light as they are.
- A tab in the background still answers a move (it runs), but browsers slow its timers. A.3's 10-second wait is for the answer, not for the page to be seen.
- The probe's `pages` global is per process. `--instances` and `--scale` copies each have their own, so the hand-built version finds only the pages connected to the copy that got the tool call. A.2's list has to be where the pages' connections are, which under `--split` is the app's process.

## Where the AI struggled

| What was tried | What happened | Whose mistake |
|---|---|---|
| `const [x, y] = await page.center("#draft")` in the driver | `(intermediate value) is not iterable`: `center` gives `{ x, y }`, as its line in `chrome.mjs` says | the AI's |
| `let nav = channel.open(...)` kept only to hold the channel open | `quillo check`: `unused variable nav (name it _nav if that is intended)`, which said what to do | the AI's, with a message that helped |

## Edge cases checked that behave well

- A server message that makes the page call `navigate` changes the page without a load: the probe's `window` mark was kept, and Back went home.
- A page refused at a channel's `join` says so in the browser's console.
- After a reload, the page joins with its session, and the tool reaches it.
- A route's typed parameter (`:id(int)`) is an integer path parameter in `quillo docs`, ready for a resource template.

## Scripts

The probe (`nav.qwe`), run with `quillo run nav.qwe --port 47858`:

```quillo
// By hand: a tool moves the user's open page. `pages` is each user's open pages.
let pages = {}

channel.on("nav", (msg, peer) => null, {
    join: (peer) => {
        let u = peer.session.user
        if u == null { return false }
        pages = set(pages, u, (pages[u] ?? []) + [peer.id])
        true
    },
    leave: (peer) => {
        let u = peer.session.user
        if u != null { pages = set(pages, u, (pages[u] ?? []) | filter((id) => id != peer.id)) }
    },
})

component App() {
    let path = url().path
    let _nav = channel.open("nav", (m) => navigate(m.path))
    <div>
        <p id="where">{path}</p>
        {match split(path, "/") | filter((s) => s != "") {
            ["orders", id] => <Order key={id} id={id}/>
            _ => <Home/>
        }}
    </div>
}

component Home() {
    <div><h1>Home</h1><input id="draft"/></div>
}

view Order(id: str) = <h1 id="order">Order {id}</h1>

server fn sign_in(name: str) -> bool {
    set_session({ user: name })
    true
}

route GET /signin/:name {
    set_session({ user: name })
    redirect("/")
}

// What a tool would do: show `path` on every page `user` has open.
route POST /tool/show { json: { user: str, path: str } } {
    let sent = (pages[user] ?? []) | map((id) => channel.send("nav", id, { path: path }))
    { open: len(pages[user] ?? []), reached: len(sent | filter((x) => x)) }
}

route GET /*rest = <html><body><App/></body></html>
```

The driver (`drive-nav.mjs`), run with `node feedback/tools/drive.mjs drive-nav.mjs`:

```js
const P = "http://127.0.0.1:47858";
const show = async (path) => (await fetch(P + "/tool/show", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ user: "ana", path }) })).json();
const state = (page) => page.ev(`return { path: location.pathname, where: document.querySelector('#where')?.textContent, order: document.querySelector('#order')?.textContent ?? null, draft: document.querySelector('#draft')?.value ?? null, kept: window.__mark === 1 }`);

export default async function ({ browser, page, sleep }) {
  const out = {};
  // A: opened before signing in, then signed in from the page itself (no reload).
  await page.go(P + "/"); await page.booted(); await sleep(800);
  await page.ev(`await fetch('/_quillo/rpc/sign_in', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '["ana"]' }); return 1`);
  await sleep(300);
  out.a_signed_in_without_reload = await show("/orders/12");     // {"open":0,"reached":0}
  // B: reloaded once signed in.
  await page.go(P + "/"); await page.booted(); await sleep(800);
  await page.ev(`window.__mark = 1; return 1`);
  out.b_show = await show("/orders/12");                          // {"open":1,"reached":1}
  await sleep(700);
  out.b_page = await state(page);                                 // /orders/12, "Order 12", kept: true
  // C: back on Home with half a sentence typed.
  await page.ev(`history.back(); return 1`); await sleep(700);
  const c = await page.center("#draft");
  await page.click(c.x, c.y); await page.type("Dear support, the invoice for");
  out.c_show = await show("/orders/7");
  await sleep(700);
  await page.ev(`history.back(); return 1`); await sleep(700);
  out.c_back = await state(page);                                 // draft: "" (the text is gone)
  // D: a second tab of the same user.
  const page2 = await browser.page();
  await page2.go(P + "/"); await page2.booted(); await sleep(800);
  out.d_show = await show("/orders/99");                          // {"open":2,"reached":2}: both moved
  return out;
}
```

A signed-in page's HTML and its text:

```sh
curl -s -c jar.txt -o /dev/null http://127.0.0.1:47859/signin/ana
curl -s -b jar.txt http://127.0.0.1:47859/orders/12 -o page.html     # 3,534 bytes; 3,333 in <script>
```

A route's `///` (`routedoc.qwe`), with `quillo docs routedoc.qwe`: the operation has `responses`, `parameters` and `x-quillo-at`, and no `summary` or `description`.

```quillo
/// One order: its lines and its status.
route GET /orders/:id(int) = <h1>Order {id}</h1>
```

Sources: the MCP specification 2026-07-28, <https://modelcontextprotocol.io/specification/2026-07-28> (`server/resources`, `basic/patterns/subscriptions`, `server/utilities/caching`, `client/elicitation`, `basic/patterns/mrtr`); MCP Apps, <https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx> and the client matrix at <https://modelcontextprotocol.io/extensions/client-matrix>; WebMCP, <https://webmachinelearning.github.io/webmcp/>, Chrome Status <https://chromestatus.com/feature/5117755740913664>, and Edge's trial page <https://developer.microsoft.com/en-us/microsoft-edge/origin-trials/trials/0b76fe60-b266-458e-a285-04e375c0c31a>.
