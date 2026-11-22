# Feedback from studying a routing system for single-page apps, and each page's control of the head

| | |
|---|---|
| App | None of its own: a study done at the user's request, 2026-11-22: "study for SPA routing system and header meta data control from each page" |
| Quillo | The released `quillo 0.8.9 (77ffcd0, 2026-11-20)` on the PATH (= tag v0.8.9). The guide and `src/client/runtime.js` were read at the tag. One `quillo check` was run with the branch's build (`0.8.9 (ecf5942)`, branch `feedback72-81`, another session's work, not a release), and says so where it is used |
| Model | Claude (Fable 5.2, `claude-fable-5-2`) |
| Tested with | `quillo check` and `quillo run` on macOS, curl, and headless Chrome through `feedback/tools/drive.mjs` (real mouse clicks, Back and Forward, a slower connection made with Chrome's own network settings). The programs and results are kept in `.git/wip/feedback84/`: `blog.qwe` (a single-page app), `spa.qwe` (the published example, quillolang/examples `e87822e`), `mpa.qwe` (pages as routes), `t_stmt.qwe`, `t_head.qwe`, `t_head2.qwe`, `t_body.qwe` (three ways to set a title), `head.qwe`, `ld.qwe`, `links.qwe`, and each `*.mjs` with its `*.result.json`. Only Chrome was run: no Firefox, no Safari, no screen reader, no crawler |

Numbers Q-1320 to Q-1330, from the block Q-1320 to Q-1339 (FEEDBACK83 holds Q-1300 to Q-1319). This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed. The repros are written in the report.

**How the request is read.**

- **A single-page app (SPA)** is a site where moving between pages does not load a new document: the page changes in place, and what the visitor was doing (a cart, a draft, a player) stays.
- **A routing system** is what decides, for an address, which page shows: the table of paths, their parameters, what is shown for none, and what moving between them does to history, scroll and focus.
- **Header meta data** is what a page puts in the document's `<head>`: its `<title>`, `<meta name="description">`, the canonical address, the tags that make a link's preview (`og:title`, `og:image`), structured data.
- **From each page** is read as: the page that knows the product's name is the one that says the title, whether it is a route, a view inside a layout, or a component the browser shows.

Quillo has two ways to make a site today, and the study measures both:

- **Pages as routes** (the default): each `route` returns a whole document. Moving is a page load, fetched ahead of the click.
- **One component that reads `url()`** (the guide's "Single-page apps"): one route serves every path, a component chooses what to show by the path, and links inside components change the page without a load.

What worked well (measured in Chrome, unless said):

- **A deep link gets the right page from the server.** `/posts/30` arrives with the post in its HTML, and with the right `<title>` when the route works it out from the path.
- **State survives moving inside an SPA.** A counter clicked twice still said 2 after six moves. A link written with the site's whole address is followed in place too.
- **With pages as routes, each page's head is its own**, and Back gives the page back whole: the counter still at 2, from the browser's own back/forward cache.
- **The page asked for ahead of the click is the one used.** The log has one `GET /about -> 200 prefetch` and no second request.
- **What goes into the head is escaped.** A title of `</title><script>alert(1)</script>` is written as text, a description's quotes as `&quot;`, and `</script>` inside a JSON-LD map as `</script>`.
- **Under `--base /shop`, a root-relative `href` gets the prefix**, the canonical link's included: `<link rel="canonical" href="/shop/posts/3">`.
- **A statement that reads `url().path` runs again when the page moves**, so one that sets the title follows every move, Back included (A.2).
- **`--watch` already knows how to bring a page up to a new server document**: it changes the body in place, keeps components' state, and sets the title and style sheets (`runtime.js`, read; development pages only).

## The answer

**Each of Quillo's two ways has half of what is asked. Routes have the table and the head. The one-component SPA has the state. The shortest path to both is to let the routes be the SPA's routing system: the browser asks for the next route's page and changes the document in place, head included.**

| | Pages as routes | One component with `url()` |
|---|---|---|
| The table of paths | the `route` lines: typed parameters, groups, guards, 404s, and (on the branch) links checked | written by hand in a `match`, and again for the status and the title (Q-1323) |
| The head of each page | the route's own | the first page's, for as long as the tab is open (Q-1320, measured) |
| State across pages | lost on every move (counter 2 → 0) | kept |
| Back | the page whole, with its scroll | scroll lost, 1500 → 0 (Q-1324) |
| Without JavaScript | works | reads only (FEEDBACK77) |
| Requests for one move | the page | the page, fetched ahead for its state, and often the server fn as well (Q-1330) |

That last row matters for the choice. Today's SPA already fetches the next page's whole HTML when a link is pointed at, keeps the components' state from it, and drops the rest, the `<head>` included. So changing the document from that same answer asks the server for nothing more.

**My recommendation, in this order:**

1. **A `<head>` block in any view or component (question 1, Part C.1).** What a page writes there goes to the document's head: on the server as the page is made, in the browser as the page changes. It fixes the head for both ways, and stands alone.
2. **`page_url()`**, the page's public address with its scheme and prefix, for canonical and `og:` tags (Q-1328).
3. **Routes changed in place (question 2, Part C.2).** A program says so once. Every same-site link then moves without a load, the head follows, and components that are on both pages keep their state.
4. **Manners for every in-place move, in both ways (Part C.4):** scroll given back on Back, focus moved and the new title announced, and `navigate` able to replace an entry or keep the scroll.
5. **`quillo check` says what it sees:** a component inside `<head>` (Q-1322), a `<title>` outside one (Q-1321), two titles, `json.stringify` in a JSON-LD script (Q-1329).
6. **Path patterns in a `match`**, for apps that stay one component (Part C.3). Smaller, and only worth it if such apps stay a first choice.

## Part A: what 0.8.9 does today

### A.1 A single-page app, walked through (`blog.qwe`)

One route for every path. Its head is written from the request's path. A component shows the page by `url().path`:

```quillo
route GET /*rest =
    <html lang="en">
        <head>
            <meta charset="utf-8"/>
            <title>{title_of(request.path)}</title>
            <meta name="description" content={`The page at ${request.path}`}/>
            <link rel="canonical" href={`https://blog.example${request.path}`}/>
        </head>
        <body><App/><Footer/></body>
    </html>
```

From the server (curl), each path has its own title: `/` `Home · Blog`, `/posts` `Posts · Blog`, `/posts/30` `Post 30 · Blog`. `/nope` and `/posts/999` answer 200, since this program did not write its table a second time for the status.

In Chrome, starting at `/`:

| Step | Address | `document.title` | Scroll | Focus | Counter | Page loaded again? |
|---|---|---|---|---|---|---|
| Loaded `/`, counter clicked twice | `/` | Home · Blog | 0 | body | 2 | — |
| Clicked Posts | `/posts` | **Home · Blog** | 0 | the link | 2 | no |
| Scrolled to 1500, clicked Post 30 | `/posts/30` | **Home · Blog** | 0 | **body** | 2 | no |
| Back | `/posts` | **Home · Blog** | **0** (was 1500) | body | 2 | no |
| Forward | `/posts/30` | **Home · Blog** | 0 | body | 2 | no |
| Clicked a link to no page | `/nope` | **Home · Blog** | 0 | the link | 2 | no |
| At `/` scrolled to 900, a button calls `navigate("?page=2")` | `/?page=2` | **Home · Blog** | **0** (was 900) | — | — | no |
| Clicked the footer's link, outside the component | `/about` | About · Blog | 0 | — | **0** | **yes** |

The description, the canonical link and `og:title` stayed the first page's too. There was no live region on the page at any step. A second run with `/posts` loaded from the server gave the same Back: scroll 0 at each of four readings, from 50 ms to over 2 s after, with the list on the page.

What one click on a link costs, with 150 ms added to each request (`ahead.mjs`):

| How the link was clicked | Requests | Shown after |
|---|---|---|
| Pointed at for 400 ms, then clicked | the page, 2,271 B | 2 ms |
| Pointed at for 100 ms, then clicked | the page, 2,271 B, and the server fn, 357 B | 155 ms |
| Clicked at once, as a touch does | the page, 2,271 B, and the server fn, 357 B | 160 ms |

### A.2 Three ways a page might set its title today

| Way | On the server | In Chrome | What is wrong with it |
|---|---|---|---|
| A statement in the component: `let doc = js("document")`, then `if doc != null { doc.title = title_of(path) }` (`t_stmt.qwe`) | the route's own `<title>` still has to be written | the title follows each move, and Back | the title is said twice, once for each side. A description, a canonical link or an `og:` tag each need a `querySelector` and a `setAttribute` |
| A component inside the head: `<head><Head/>…</head>` (`t_head.qwe`, `t_head2.qwe`) | `<head><quillo-island …><title>Home</title></quillo-island><meta charset…>…</head>` | the title follows | **the browser ends the head at the island.** The island and everything written after it were in `<body>`: the charset, the viewport, the description, the icon and the style. The head held only the loader's two scripts. `quillo check`: ok (Q-1322) |
| The page's component writes `<title>` and `<meta>` itself, with none in the head (`t_body.qwe`) | `<head><meta charset="utf-8"></head>`, the title inside the body | the title follows, since `document.title` reads the first `<title>` anywhere | not HTML: a title belongs in the head. Not tried with any crawler. `quillo check`: ok |

So only the first is sound, and it reaches the title alone.

### A.3 Pages as routes (`mpa.qwe`)

A layout view takes each page's title and description as props:

```quillo
view Page(title: str, description: str, children) =
    <html lang="en">
        <head><meta charset="utf-8"/><title>{title} · Blog</title><meta name="description" content={description}/></head>
        <body><nav><a href="/">Home</a> <a href="/about">About</a> <Counter/></nav>{children}</body>
    </html>

route GET / = <Page title="Home" description="The home page"><h2>Home</h2></Page>
route GET /about = <Page title="About" description="About this blog"><h2>About</h2></Page>
```

| Step | Title | Description | Counter | Page loaded again? |
|---|---|---|---|---|
| Loaded `/`, counter clicked twice | Home · Blog | The home page | 2 | — |
| Clicked About | About · Blog | About this blog | **0** | yes |
| Back | Home · Blog | The home page | 2 | no: the browser kept the page |

### A.4 The head with awkward values (`head.qwe`, `ld.qwe`, curl)

| Case | Result |
|---|---|
| A title of `</title><script>alert(1)</script>` | `<title>&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt; · Site</title>` |
| A description of `He said "hi" & left <b>` | `content="He said &quot;hi&quot; &amp; left &lt;b&gt;"` |
| Extra head tags passed to the layout as a prop, `head={[<meta name="robots" …/>, <link rel="canonical" …/>]}` | written in the head, in order |
| A view deep in the page writes `<title>Set from deep inside</title>` | written where it stands, in `<section>` in the body. The layout's title is the page's. `quillo check`: ok (Q-1321) |
| Two titles in one head | both written. `quillo check`: ok |
| JSON-LD as `<script type="application/ld+json">{item}</script>`, `item` a map | `{"@context":"https://schema.org",…}`, with `</script>` in a value written `</script>` |
| The same with `{json.stringify(item)}` | `"{\"@context\":\"https://schema.org\",…}"`: a JSON string, not an object (Q-1329) |
| A page with no `<head>` | sent as written, Quillo's scripts at the end of the body |

### A.5 Under a base path (`--base /shop`, asked for `/posts/3` as the proxy passes it on)

| What the page writes | Result | Right? |
|---|---|---|
| `<link rel="canonical" href={request.path}/>` | `href="/shop/posts/3"` | the prefix is there. The address has no host |
| `<meta property="og:url" content={`https://${request.host}${request.path}`}/>` | `content="https://shop.example/posts/3"` | no: the public address is `/shop/posts/3`, and `https` was written by hand |
| `<meta property="og:image" content="/img/card.png"/>` | as written | no: a preview needs the whole address |

`request` has `host` and `path`. It has no scheme and no prefix: `request.scheme`, `request.origin` and `request.url` are each refused by `quillo check` ("this map has no field"). Q-1328.

### A.6 What the checker can see

The branch's build has a link check (Q-1251). On a program with two routes it says `no route answers `/abuot`, so this link is a 404 (did you mean `/about`?)`. On `blog.qwe`, whose one route takes every path, it passes the link to `/nope`. A table written in a `match` is not one the checker reads.

## Part B: how others do it

From each one's documentation, not run here.

| Who | Where the table of paths is | How a page sets the head | What a move does |
|---|---|---|---|
| Turbo (Hotwire), htmx's `hx-boost` | the server's routes | each page's own `<head>`; Turbo merges it in, htmx takes the title | fetches the next page, replaces the body. Turbo keeps elements marked permanent |
| Astro's client router | the server's routes (files) | each page's own `<head>`, merged | swaps the page, with view transitions. `transition:persist` keeps an island's state |
| Phoenix LiveView | the server's routes | a `page_title` the page assigns, sent to the browser | the page changes over its socket |
| Inertia | the server's routes | a `<Head>` component in any page | the server sends the next page's data, the browser shows it |
| Next.js | folders | `metadata`, or `generateMetadata` when it needs data, in each page and layout, merged from the outside in | layouts keep their state. The new title is announced |
| React Router 7 | a table, or files | a `meta` function for each route. React 19 also moves a `<title>` or `<meta>` written in any component to the head | in place. Scroll is given back by a component it ships |
| SvelteKit | folders | `<svelte:head>` in any component | in place, focus moved, the title announced, scroll given back |
| Vue Router with Unhead, Nuxt | a table, or files | `useHead` and `useSeoMeta` in any component | in place |
| Angular | a table | `title` on a route, and `Title` and `Meta` services | in place |

What they agree on:

- **Where pages are the server's, the head needs nothing new.** The next page arrives with its own, and the browser takes it over.
- **Every router that lives in the browser had to add a way to set the head**, most of them years after the router itself.
- **The head is set from where the data is.** Either any component may add to it (Svelte, Vue, React 19, Inertia), or each route has a function that gets its data (Next.js, React Router).
- **A move in place owes the visitor what a page load gave for nothing:** the scroll on Back, focus at the top, the new page said aloud. The frameworks that build these in turn them on by default.
- **One table, known to the tools,** is what gives typed parameters and checked links.

## Part C: the design proposed

A sketch. The names are open. Nothing here is built.

### C.1 A `<head>` block in any view or component

```quillo
view ProductPage(p) =
    <article>
        <head>
            <title>{p.name} · Shop</title>
            <meta name="description" content={p.about}/>
            <link rel="canonical" href={page_url()}/>
            <meta property="og:image" content={page_url(p.image)}/>
            <script type="application/ld+json">{{ "@type": "Product", name: p.name }}</script>
        </head>
        <h2>{p.name}</h2>
    </article>
```

- **What it means:** a `<head>` written anywhere but as the document's own adds to the page's head.
- **One of a kind replaces:** `<title>`, a `<meta>` by its `name` or `property`, `<link rel="canonical">`. The innermost wins, so a page inside a layout overrides the layout's title, and the layout's stands where the page says nothing.
- **The rest add:** `<link rel="alternate">`, JSON-LD scripts, preloads.
- **On the server** they are written in the document's `<head>` before the page is sent, so crawlers and link previews, which mostly run no script, see them.
- **In the browser** they follow the state they read. When the view or component leaves the page, its tags leave, and the layout's show again.
- **Why a block and not every `<title>`:** SVG has a `<title>` of its own, which names a shape. A block says which is meant.
- **What it cannot do:** a part sent later (`defer`, a stream) arrives after the head has gone out. Its block takes effect in the browser only, and `quillo check` can say so.

`page_url()` gives this page's public address, and `page_url("/img/card.png")` that of a path: the scheme (the connection's, or a trusted proxy's `X-Forwarded-Proto`), the host, the prefix of `--base` or a mount, the path.

### C.2 Routes changed in place

The program says once that its pages stay (a switch at the top; the word is open). From then on, for every same-site link, in a component or not:

1. **The next page is asked for**, or taken from the request already made as the link was pointed at.
2. **A redirect is followed**, and the address bar gets where it ended.
3. **The head is taken over:** title, descriptions, canonical, alternates; style sheets added and removed, as `--watch` does today.
4. **The body is changed in place.** A component that is on both pages, at the same place or with the same `key`, keeps its state. The others start with the state the server wrote, with no `null` first and no second request.
5. **Scroll, focus and the announcement** as in C.4.
6. **A full load instead** whenever in-place is not safe: the answer is not a page, the two pages' own scripts in the head differ (the rule `--watch` has now), the link has `target`, `download` or `data-reload`, the address is another mount's, or anything fails on the way.

| What it settles | How |
|---|---|
| The table written by hand (Q-1323) | the `route` lines are the table: typed parameters, groups, guards, real 404s |
| The stale head (Q-1320) | each route's own head, taken over on each move |
| Two kinds of link (Q-1327) | every same-site link moves in place |
| Two requests for one click (Q-1330) | the page is the one request |
| Links the checker cannot see (A.6) | they are links between routes |
| Reading without JavaScript (FEEDBACK77) | the pages are plain pages |

What it costs:

- **Bytes on a move:** the page's HTML, not a server fn's JSON. Here, 2,271 B against 357 B for a post, compressed. Today's SPA fetches that page as well, whenever a link is pointed at.
- **The runtime grows** by the code that changes a page in place, which only `--watch` pages carry now. By the rule that no change slows Quillo, a program that does not turn this on must carry none of it, and the size and the time of a move are measured before it lands.
- **A program's own inline scripts do not run again** on a move. That is the known cost of this kind of navigation everywhere, and why step 6 falls back.

`url()` and `navigate()` stay as they are. In a program whose pages stay, `navigate` makes the same move.

### C.3 A table of paths for apps that stay one component

For a program that keeps one component and one route, the `match` on `split(path, "/")` can take a route's patterns:

```quillo
{match url().path {
    route /                  => <Home/>
    route /products          => <ProductList/>
    route /products/:id(int) => <ProductPage key={id} {id}/>
    _                        => <NotFound/>
}}
```

The parameters are typed as a route's are, `quillo check` can hold links to the table, and the server can answer 404 where the `_` arm was taken. It does nothing for the head, which C.1 covers.

### C.4 What every move in place owes the visitor

| | Today (measured) | Proposed |
|---|---|---|
| Scroll on a new page | top | top, or the `#spot` |
| Scroll on Back and Forward | lost: 1500 → 0 | given back, once the page has what it had |
| A change of the query only (`?page=2`, a filter) | scrolls to the top and adds an entry | `navigate("?page=2", { scroll: false })`, `{ replace: true }` |
| Focus | stays on a link that may be gone, then the body | moved to the page's `<main>`, or its first heading |
| Said to a screen reader | nothing: no live region, and the title does not change | the new title, in a live region Quillo keeps |
| History's names | every entry "Home · Blog" | each entry its page's title |

### C.5 Which first

C.1 and `page_url()` change no page that does not use them, and fix the head in both ways. C.4 helps the SPAs there are today. C.2 is the largest piece, and the one that makes the routes the routing system.

## New issues

### Q-1320. In a single-page app the head stays the first page's: title, description, canonical and `og:` tags never follow a move

| Kind | Detected by | Attempts |
|---|---|---|
| silent | headless Chrome | 1 |

Measured in A.1. The route writes the head once, for the path the visitor arrived at. After six moves `document.title` was still `Home · Blog`, and the description and canonical link still said `/`. The published example writes `<title>Shop</title>` for every page.

| Impact | |
|---|---|
| **Functionality** | The tab, the browser's history and Back menu, and a bookmark made on a product's page all say the first page's title. A tool that reads `document.title` on each move (analytics, a share button) sends the wrong one |
| **User** | A visitor with five tabs of one shop sees five times "Shop". A developer finds no word on it in the guide |
| **Requests** | No change |
| **Security** | No change |

The server's head is right for each address when the route works it out, so a crawler that loads each address sees the right one.

**Suggestion:** C.1, and C.2 where the program's pages are routes.

### Q-1321. A page inside a layout cannot set the head: a `<title>` or `<meta>` it writes stays in the body, and `quillo check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| limitation; silent | curl, `quillo check` | 1 |

Measured in A.4: a view that writes `<title>Set from deep inside</title>` gets it in its `<section>`, and the layout's title stays the page's. Two titles in one head are both written. Neither is warned of. The way that works is to pass every head value up through the route into the layout's props (A.3), so the view that loads a product cannot name the page after it.

| Impact | |
|---|---|
| **Functionality** | The head can be set only from the route. A page made of views three deep carries its title, description and preview image as props through each |
| **User** | A `<title>` written where it reads naturally does nothing, without a word |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** C.1. Until then, `quillo check` warns of a `<title>`, or a `<meta>` with a `name` or `property`, written outside `<head>` (SVG's `<title>` aside), and of two titles in one head.

### Q-1322. A component in `<head>` is written as a `<quillo-island>` there, which ends the head in the browser: the charset, viewport, description and styles after it land in the body

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl, headless Chrome, `quillo check` | 1 |

Measured in A.2 with `t_head2.qwe`: `<head><Head/><meta charset="utf-8"/><meta name="viewport" …/><meta name="description" …/><link rel="icon" …/><style>…</style></head>`. In Chrome each of them had `BODY` as its parent, the island too, and `document.head` held only the loader's two scripts. HTML allows only head elements in the head, and the parser closes it at the first other one. `quillo check` said ok, and the page looked right, since the title still showed.

| Impact | |
|---|---|
| **Functionality** | The page has an empty head: every tag written for it is in the body. A tool that looks for the description or the preview tags in the head finds none. (The charset still held, since Quillo says it in the answer's `content-type`.) |
| **User** | It is the first thing a developer tries, to make the title follow the page, and it seems to work |
| **Requests** | No change |
| **Security** | A `content-security-policy` written as a `<meta>` after the component would be in the body. By the CSP specification a policy in a `<meta>` counts only in the head (not run here) |

**Suggestion:** an error at load and in `quillo check`: `a component cannot stand in <head>: the browser would end the head there`, with C.1's block as the way once it exists.

### Q-1323. An SPA's table of paths is written by hand, two or three times, and the checker cannot read it

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | curl, headless Chrome, `quillo check` (the branch's build) | 1 |

The guide's own example writes the table as a `match` on `split(path, "/")` in the component. The published `spa.qwe` has it three times: a `page_of` function, the component's `match` on its answer, and the route's `match` for the status. `blog.qwe` did not, so `/nope` and `/posts/999` answered 200 with a page that says "Not found". A parameter is text until the program turns it (`int(id)`). The branch's link check passes a link to `/nope` (A.6).

| Impact | |
|---|---|
| **Functionality** | Typed parameters, 404s, groups and guards, all of which a `route` has, are written again by hand or left out |
| **User** | Three places to keep in step: the component's `match`, the route's status, the title |
| **Requests** | A wrong address answers 200, so a crawler keeps it and a monitor sees no error |
| **Security** | A guard written in the component does not hold on the server (FEEDBACK77's Q-1142) |

**Suggestion:** C.2, where the routes are the table. For apps that stay one component, C.3.

### Q-1324. Back loses the scroll position in a single-page app

| Kind | Detected by | Attempts |
|---|---|---|
| silent | headless Chrome | 2 |

Measured in A.1: scrolled to 1500 on the list, a post opened, Back: scroll 0, and still 0 two seconds later. The list's component had left the page, so on Back it was made again and asked the server again (`/_quillo/rpc/list_posts`). The browser tried to give the scroll back before there was a list to scroll. With pages as routes, Back gave the page back whole (A.3).

| Impact | |
|---|---|
| **Functionality** | Every Back from a detail page starts the list from the top |
| **User** | In a list of 60, a visitor reading item 30 scrolls there again after each one. This is the complaint SPAs are known for |
| **Requests** | Each Back asks the server for the list again |
| **Security** | No change |

**Suggestion:** C.4: keep each entry's scroll, and give it back once the page has what it had. With C.2, the page can be kept as well as the scroll.

### Q-1325. After a move in place, focus is left where it was and nothing is said: no live region, and the title does not change

| Kind | Detected by | Attempts |
|---|---|---|
| silent; accessibility | headless Chrome (no screen reader was run) | 1 |

Measured in A.1: after a click on a link that then left the page, `document.activeElement` was the body. There was no element with `aria-live`, `role="status"` or `role="alert"` at any step, and the title stayed the first page's (Q-1320). A page load gives all three for nothing: focus starts at the top, and a screen reader reads the new title.

| Impact | |
|---|---|
| **Functionality** | A move in place is silent to anyone who does not see the page |
| **User** | A keyboard user's next Tab starts from the top of the document, or from a link of the page before. A screen reader's user hears nothing after pressing Enter on a link |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** C.4: after a move, focus goes to the page's `<main>` or first heading, and the new title is put in a live region. On by default, as SvelteKit and Next.js have it.

### Q-1326. `navigate` has one form: it always adds a history entry and scrolls to the top

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | headless Chrome; `src/builtin_docs.rs` (`navigate(path)`) | 1 |

Measured in A.1: at `/`, scrolled to 900, `navigate("?page=2")` gave `/?page=2`, scroll 0, and one more history entry. A filter box that keeps its text in the query would jump to the top and add an entry at each key.

| Impact | |
|---|---|
| **Functionality** | The query cannot hold what a page is showing (a page number, a filter, a tab) without a jump and a long history |
| **User** | Such state is kept in component state, and is lost on a reload and missing from a shared link |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** `navigate(path, { replace: true, scroll: false })`.

### Q-1327. Two kinds of link on one page: one outside every component loads the page again and drops all state

| Kind | Detected by | Attempts |
|---|---|---|
| silent | headless Chrome; `runtime.js` (`followed`) | 1 |

Measured in A.1: the footer's `<a href="/about">`, in a view beside the app's component, loaded the page again, and the counter went from 2 to 0. The same link in the nav moves in place. The guide says "links inside components", and the runtime follows only a link inside an island. Nothing in the markup tells the two apart.

| Impact | |
|---|---|
| **Functionality** | Whether a link keeps the cart depends on which view it was written in |
| **User** | A developer finds out when a visitor's cart empties |
| **Requests** | A full page where a move was meant |
| **Security** | No change |

**Suggestion:** once a page follows `url()`, every same-site link on it moves in place, as C.2 has it. `data-reload` stays the way to ask for a load.

### Q-1328. A page cannot say its own public address: `request` has the host and the path, no scheme and no prefix

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | curl under `--base /shop`, `quillo check` | 1 |

Measured in A.5. A canonical link, `og:url` and `og:image` want the whole public address. Under `--base /shop`, a hand-built `og:url` came out as `https://shop.example/posts/3` for a page that is at `/shop/posts/3`, and its `https` was written by hand. A `content` attribute is not given the prefix, as an `href` is.

| Impact | |
|---|---|
| **Functionality** | A program that is right at `/` writes wrong addresses when a host puts it under a path, or mounts it |
| **User** | Each program keeps its public address in a variable of its own (`PUBLIC_URL`) |
| **Requests** | A preview's image is asked for at an address that does not answer |
| **Security** | No change. `request.host` is the visitor's `Host` header, so an address built from it is the visitor's to choose, as with any site that does so |

**Suggestion:** `page_url()` and `page_url(path)` (C.1), the scheme taken from the connection or a `--trust-proxy` proxy.

### Q-1329. `{json.stringify(x)}` in a JSON-LD script is written as a JSON string, not an object

| Kind | Detected by | Attempts |
|---|---|---|
| silent; docs | curl | 1 |

Measured in A.4. `<script type="application/ld+json">{item}</script>` writes the object, safely. `{json.stringify(item)}`, which is what one writes coming from JavaScript, writes `"{\"@context\":…}"`: valid JSON, and a string. A search engine reads no structured data from it, and says nothing.

| Impact | |
|---|---|
| **Functionality** | Structured data that no reader takes |
| **User** | Nothing fails. It is seen only in a search engine's testing tool |
| **Requests** | No change |
| **Security** | No change: both forms escape `</script>` |

**Suggestion:** the guide shows the `{item}` form for JSON-LD, and `quillo check` warns where a `json.stringify(...)` is the whole content of a `<script type="application/ld+json">`.

### Q-1330. A link tapped before its page arrives asks twice: the page, which is dropped, and the server fn

| Kind | Detected by | Attempts |
|---|---|---|
| perf | headless Chrome with 150 ms added to each request | 3 cases |

Measured in A.1. An SPA's link asks for its page's whole HTML when pointed at or pressed, to take the components' state from it. If the click comes before the answer, the component asks its server fn as well, and the page, when it arrives, is dropped. Pointed at for 400 ms: one request. Pointed at for 100 ms, or tapped: both, 2,271 B and 357 B. A touch has no pointing, so on a phone it is both each time.

| Impact | |
|---|---|
| **Functionality** | No change: the page shows |
| **User** | No faster for the first request, on the connections where it would matter |
| **Requests** | Two for each tap, the larger one unused. The server makes the whole page for nothing |
| **Security** | No change |

**Suggestion:** when the click comes while the page is on its way, wait for it rather than ask again (it is the earlier request). Or, with C.2, the page is the one request.

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| Asked the program under `--base /shop` for `/shop/products` | took `--base` for a mount | every page 404: the option is for a proxy that takes the prefix off first, as the guide says. Asked again for `/products` | one wrong finding nearly written |
| Clicked a button and a footer link that had scrolled out of view | the helper clicks at a point | the clicks landed on nothing, and two steps showed no change. Run again with the element scrolled into view | 1 run |
| Measured the double request three times on one post | simplest | the second and third came from the browser's cache: 0 B, no second request. A different post each time showed it | 1 run |
| Read `request.scheme` in a probe | most frameworks have one | `quillo check`: no such field, and the program did not start | 1 run |

## Limitations

- **Only Chrome.** Firefox and Safari keep pages for Back by rules of their own, and were not run. No screen reader was run: Q-1325 is what the page offers one, not what one said.
- **No crawler or link preview was tried.** What they read from a `<title>` in the body, or a head that landed in the body, is not known here.
- **The design is not built.** C.2 rests on reading `runtime.js`: that `--watch` changes a page in place and keeps state is its code today, not a measurement of it used for moving between pages.
- **Times were not compared** between a page load and a move in place: the machine was busy. Requests and bytes were.
- **Not probed:** mounted programs, tenants, `quillo app`, phones, pages kept offline, forms.
- **Part B is from documentation**, as each framework stood when read.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A deep link in an SPA | the right page in the server's HTML, with the right title when the route works it out |
| State across moves in place | kept (counter 2 after six moves) |
| A link written with the site's whole address, inside a component | moves in place |
| Back with pages as routes | the page whole, state and all |
| A page fetched ahead of its click | used: one request, logged `prefetch` |
| `</title><script>` in a title, quotes in a description | escaped |
| `</script>` in a JSON-LD map | written `</script>` |
| Head tags passed to a layout as a prop | written in the head |
| A root-relative `href` under `--base` | given the prefix |
| A statement setting `document.title` from `url().path` | follows every move, and Back |
| The published `spa.qwe` at the root | 404 for `/nope` and `/products/99`, 200 for the rest |

## Questions for the user

### 1. How should a page say what goes in the head?

**With a `<head>` block that any view or component may write.** It is HTML's own word, and it puts the title where the data is. It is needed in both ways of making a site: measured, a view's `<title>` stays in the body (Q-1321), and an SPA's head never changes (Q-1320).

| Impact | |
|---|---|
| **Functionality** | A `<head>` inside a view or component adds to the page's head. `<title>`, a `<meta>` by its name and the canonical link replace, innermost first. On the server they are in the document's head. In the browser they follow state |
| **User** | The page that loads the product names the page. Nothing is passed up through props |
| **Requests** | No change. A page with no such block is made as today |
| **Security** | The values are escaped as any attribute and text are. A block cannot write a script but JSON-LD, whose content is data |

The other two ways:

- **Any `<title>` or `<meta>` written anywhere is moved to the head** (React 19's way). Nothing new to learn, but SVG's own `<title>` must then be told apart, and a tag's place in the markup stops meaning where it goes.
- **A builtin, `page_head({ title, description, image })`** (Vue's and Nuxt's way). Short for the common three, but a second way to write what markup already says, and each new tag needs a new key.

**My recommendation:** the block. `page_url()` comes with it.

**The question:** shall a `<head>` block in any view or component set the page's head? If not, which of the other two?

### 2. Should the routes be the SPA's routing system, with moves made in place?

**Yes. The routes already are a routing system, with everything a hand-written `match` lacks, and today's SPA already fetches the next route's page.** It happens in every program that wants pages to keep state across moves: today that means leaving routes for one component and writing the table again (Q-1323).

| Impact | |
|---|---|
| **Functionality** | A switch at the top of a program. Then every same-site link moves in place: the head is the next route's, components on both pages keep their state, the rest start from the server's. A full load wherever in-place is not safe |
| **User** | No second table, no title to set by hand, one kind of link. A program made of routes becomes an SPA by one line |
| **Requests** | One for each move, the page. Today's SPA makes that one and often a second (Q-1330). The page is larger than a server fn's answer: 2,271 B against 357 B here |
| **Security** | Guards stay on the server, on each route, where they hold (FEEDBACK77's Q-1142). No page is shown that its route did not answer |

What today's one-component way has that this gives up: a move that sends only data. It stays, for programs that want it.

**My recommendation:** build it as an opt-in, after question 1's block. Keep `url()` and `navigate()` as they are. Add path patterns to `match` (C.3) only if one-component apps should stay a first choice.

**The question:** shall I plan routes changed in place as the SPA routing system? If not, the routing work is C.3 alone: path patterns in a `match`, with the head from question 1 and the manners of C.4.

### 3. With moves in place, should page state and a kept component's state survive a move?

**Yes: it is what an SPA is taken for.** It happens at every move once question 2 is built. Page state (`state cart = []` at the top) was decided on 2026-11-15 as "per page load", when every move was a page load.

| Impact | |
|---|---|
| **Functionality** | Page state keeps its value across moves in place, and starts over at a real page load. A component on both pages keeps its state. One that is only on the new page starts fresh |
| **User** | The cart, a draft and a player survive moving about. A developer who wants a fresh start gives the component a `key` from the address, as the guide says now |
| **Requests** | No change |
| **Security** | No change: page state is one visitor's page, as before, and never another's |

What starting over at each move has: every page begins clean, as it does today.

**My recommendation:** keep it across moves in place.

**The question:** shall page state and kept components' state survive a move in place? If not, each move starts every state afresh, and only the document's load is saved.
