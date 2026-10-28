# Same-page links jump to the top once a page reads `url()`

**Fixed** in d2c2aa4 ("Links and navigate() to a spot on a page go there as the browser
does, :target and all"). This is issue Q-001 in [feedback/INDEX.md](feedback/INDEX.md).

Once any component on a page has called `url()`, clicking a link to a spot on the same
page (`<a href="#target">`) inside a component scrolls to the top of the page, not to
the target. CSS `:target` is not set either. The address bar and `url().hash` are
right, so the bug is easy to miss.

Pages hit by this: tables of contents, "jump to reply" links (`#p12`), footnotes and
"back to top" links, on any single-page app.

- **Seen on:** 9abef4a (this repository) and wisp's development build of 2026-10-27
  (`d4cd171` plus uncommitted changes). The `navigate()` change in wisp does not
  affect it.
- **Browser:** Chrome, headless, with real mouse events sent through the DevTools
  Protocol (not `el.click()`).

## To reproduce

```quillo
view Body(note: str) =
    <div>
        <p id="top">{note}</p>
        <div style="height:1500px"></div>
        <a id="jump" href="#target">Jump to the target</a>
        <div style="height:2500px"></div>
        <h2 id="target">Target</h2>
        <div style="height:2000px"></div>
    </div>

component Routed() {
    let u = url()
    <Body note={`path ${u.path} hash [${u.hash}]`}/>
}

component Plain() {
    state n = 0
    <Body note="no url()"/>
}

route GET / = <html><body><Routed/></body></html>
route GET /plain = <html><body><Plain/></body></html>
```

1. `quillo run hash.qwe`
2. Open `/plain`, scroll down to the link and click it. The page scrolls to "Target".
3. Open `/`, scroll down to the link and click it. The page scrolls to the top.

## Measured

Both builds gave the same figures. Window 1000 × 800. The link was scrolled to the
middle of the window, at a scroll position of 1203, before the click.

| After the click | `/plain` (no `url()`) | `/` (reads `url()`) |
|---|---|---|
| Scroll position | 4088, at the target | **0, the top** |
| Target's distance from the top of the window | 0 | **4088 px** |
| `location.hash` | `#target` | `#target` |
| `history.length` | +1 | +1 |
| CSS `:target` | `#target` | **none** |
| `url().hash` | (not read) | `target` |
| `popstate` events | 1 | 0 |
| After Back | 1203, hash cleared | 1203, hash cleared |

**Expected:** `/` behaves like `/plain`. It should scroll to the target and set
`:target`, while `url().hash` follows the change.

## Cause

`navStart()` ([src/client/runtime.js:2871](src/client/runtime.js#L2871)) handles every
same-site link inside a `quillo-island`. It skips links with modifier keys, `target`,
`download` or `data-reload`, and links outside the program or under `/_quillo/`. It does
not skip links that differ from the current address only in the hash. Those go to
`navigate()`, which calls `history.pushState` and then `window.scrollTo(0, 0)`:

```js
function navigate(to) {
  navStart();
  const full = withBase(to);
  if (full !== location.pathname + location.search + location.hash) history.pushState(null, "", full);
  window.scrollTo(0, 0);
  navChanged();
  return null;
}
```

`pushState` does not scroll to the hash or set `:target`. The prefetcher already leaves
same-page `#` links alone; the router does not.

## Suggested fix

Let the browser handle same-page links. The browser scrolls to the target and sets
`:target`. It also fires `popstate` on that navigation (measured above: 1 event on
`/plain`). The `popstate` listener `navStart()` already registers then updates `url()`,
so `url().hash` still follows.

In the click handler, after the `/_quillo/` check:

```js
    if (path.startsWith("/_quillo/")) return;
    // A spot on this page: the browser scrolls there and sets :target, and its popstate updates url().
    if (u.hash && u.pathname === location.pathname && u.search === location.search) return;
    e.preventDefault();
```

## Related: `navigate()` with a hash

The same problem applies when `navigate` is called from code:

- `navigate("#comments")` scrolls to the top instead of to `#comments`.
- `navigate("/t/6#p12")` renders the new page and then stays at the top, not at
  `#p12`.

`navigate()` could scroll to the element the hash names, falling back to the top when
there is none. For another page, it should do so after the new page has rendered,
because the element does not exist before that.

## Test files

The program above is the repro, as
[feedback/repros/Q-001-hash-link-after-url.qwe](feedback/repros/Q-001-hash-link-after-url.qwe),
run by `node feedback/run.mjs Q-001`.
